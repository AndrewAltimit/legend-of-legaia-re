//! Planning and writing the whole mod: the injected regions, the retail-word
//! guards on every hook site, and the installer.

use super::hook_asm::{
    assemble_count, assemble_fill, assemble_id, assemble_pager, assemble_performed,
};
use super::rows::{SuperArtRow, super_art_rows};
use super::{
    APPLIER_VA, APPLIER_W0, ARROWS_STRIDE, ARTS_CHARACTERS, COUNT_RET_VA, GLYPH_BUF_BYTES,
    GLYPH_FN_VA, GLYPH_FN_W0, HOOK_COUNT_VA, HOOK_COUNT_W0, HOOK_COUNT_W1, HOOK_ID_VA, HOOK_ID_W0,
    HOOK_ID_W1, HOOK_PERFORMED_VA, HOOK_PERFORMED_W0, HOOK_PERFORMED_W1, ID_RET_VA,
    OVERLAY_BASE_VA, OVERLAY_PROT_INDEX, PAGER_COUNT_W, PAGER_LAST_W, PAGER_MASK_W,
    PAGER_SOUND_FN_VA, PAGER_VA, PAGER_W0, PAGER_WORDS, PERFORMED_RET_VA, PERFORMED_RET_W,
    SCAN_HEAD_VA, SCAN_HEAD_W, SCAN_HIT_VA, SCAN_HIT_W, SCRATCH_ARROWS_OFF, SCRATCH_BYTES,
    SUP_STRIDE, SUPER_ARTS_PER_CHAR,
};
use anyhow::{Result, bail};
use legaia_code_hooks::arenas::{
    ARENA1_END_VA, ARENA1_VA, ARENA2_END_VA, ARENA2_VA, Edit, OVERLAY_TABLE_RANGES,
    SCUS_GAP_END_VA, SCUS_GAP_VA, SCUS_TABLE_RANGES, SLOT6_END_VA, SLOT6_VA,
};
use legaia_code_hooks::mips::*;

// --- Planning ----------------------------------------------------------------

/// A planned "show Super Arts" injection: every same-size write, plus the exact
/// landing addresses so an oracle can pin them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuperArtListInjection {
    pub edits: Vec<Edit>,
    /// The fifteen derived rows, in `character * 5 + sorted_index` order.
    pub rows: Vec<SuperArtRow>,
    pub count_va: u32,
    pub id_va: u32,
    pub fill_va: u32,
    pub performed_va: u32,
    pub sup_va: u32,
    pub arrows_va: u32,
    pub scratch_va: u32,
    pub buf_va: u32,
}

fn words_to_bytes(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Resolve a SCUS VA to a file offset in the image.
fn scus_off(scus: &[u8], va: u32) -> Result<usize> {
    legaia_game_tables::item_names::file_offset_for_va(scus, va)
        .ok_or_else(|| anyhow::anyhow!("show-super-arts: can't resolve SCUS VA {va:#x}"))
}

/// Read a `u32` from `scus` at `va` and require it to be `expect`.
fn expect_scus(scus: &[u8], va: u32, expect: u32) -> Result<u32> {
    let off = scus_off(scus, va)?;
    let got = read_word(scus, off)?;
    if got != expect {
        bail!(
            "show-super-arts: SCUS {va:#x} = {got:#010x}, expected {expect:#010x} \
             (unrecognized build - nothing written)"
        );
    }
    Ok(got)
}

/// Read a `u32` from the overlay at `va` and require it to be `expect`.
fn expect_overlay(ov: &[u8], va: u32, expect: u32) -> Result<u32> {
    let off = (va - OVERLAY_BASE_VA) as usize;
    let got = read_word(ov, off)?;
    if got != expect {
        bail!(
            "show-super-arts: PROT {OVERLAY_PROT_INDEX} {va:#x} = {got:#010x}, \
             expected {expect:#010x} (unrecognized build - nothing written)"
        );
    }
    Ok(got)
}

/// Refuse a region that overlaps a live data table, even if its bytes are zero.
fn assert_not_in_tables(va: u32, len: u32, ranges: &[(u32, u32)], what: &str) -> Result<()> {
    let end = va.saturating_add(len);
    for &(a, b) in ranges {
        if va < b && a < end {
            bail!(
                "show-super-arts {what} region {va:#x}..+{len} overlaps live table \
                 {a:#x}..{b:#x} - refusing"
            );
        }
    }
    Ok(())
}

/// Confirm a hosted span really is all-zero dead space in this image.
fn assert_zero(scus: &[u8], va: u32, len: usize) -> Result<()> {
    let off = scus_off(scus, va)?;
    let region = scus
        .get(off..off + len)
        .ok_or_else(|| anyhow::anyhow!("show-super-arts: {va:#x}..+{len} past end of SCUS"))?;
    if region.iter().any(|&b| b != 0) {
        bail!(
            "show-super-arts: {va:#x}..+{len} is not all-zero dead space \
             (build / collision) - refusing"
        );
    }
    Ok(())
}

/// One placed span inside a dead-space region, checked against its end.
struct Region {
    va: u32,
    end: u32,
    cursor: u32,
    name: &'static str,
}

impl Region {
    fn new(va: u32, end: u32, name: &'static str) -> Self {
        Self {
            va,
            end,
            cursor: va,
            name,
        }
    }

    /// Carve `len` bytes, rounding the start up to `align` first.
    fn take(&mut self, len: u32, align: u32, what: &str) -> Result<u32> {
        let at = self.cursor.next_multiple_of(align);
        let end = at + len;
        if end > self.end {
            bail!(
                "show-super-arts: {what} ({len} B) overruns {} {:#x}..{:#x} \
                 ({} B already used)",
                self.name,
                self.va,
                self.end,
                self.cursor - self.va
            );
        }
        self.cursor = end;
        Ok(at)
    }

    fn used(&self) -> u32 {
        self.cursor - self.va
    }
}

impl SuperArtListInjection {
    /// Plan every edit. Needs the `SCUS_942.54` image (hook sites, arts-name
    /// table, dead-space hosts) and the raw PROT 0898 entry (the pager and the
    /// applier). Refuses, without touching anything, if the build isn't the
    /// recognized US layout, a hosted region isn't dead space, a Super Art's
    /// chain or input doesn't resolve against this disc's arts-name table, or a
    /// routine overruns its region.
    pub fn plan(scus: &[u8], ov0898: &[u8]) -> Result<Self> {
        // Fingerprint every site first; capture the words the routines replay.
        let count_disp = [
            expect_scus(scus, HOOK_COUNT_VA, HOOK_COUNT_W0)?,
            expect_scus(scus, HOOK_COUNT_VA + 4, HOOK_COUNT_W1)?,
        ];
        let id_disp = [
            expect_scus(scus, HOOK_ID_VA, HOOK_ID_W0)?,
            expect_scus(scus, HOOK_ID_VA + 4, HOOK_ID_W1)?,
        ];
        // Never written, but the hooks' correctness depends on both staying put:
        // a plain row returns in front of the scan and a Super Art row enters
        // the scan's hit arm.
        expect_scus(scus, SCAN_HEAD_VA, SCAN_HEAD_W)?;
        expect_scus(scus, SCAN_HIT_VA, SCAN_HIT_W)?;
        expect_scus(scus, GLYPH_FN_VA, GLYPH_FN_W0)?;
        expect_overlay(ov0898, PAGER_VA, PAGER_W0)?;
        expect_overlay(ov0898, PAGER_VA + 0x10, PAGER_MASK_W)?;
        expect_overlay(ov0898, PAGER_VA + 0x44, PAGER_COUNT_W)?;
        expect_overlay(ov0898, PAGER_VA + 0x12C, jal(PAGER_SOUND_FN_VA))?;
        expect_overlay(
            ov0898,
            PAGER_VA + (PAGER_WORDS as u32 - 1) * 4,
            PAGER_LAST_W,
        )?;
        expect_overlay(ov0898, APPLIER_VA, APPLIER_W0)?;
        expect_overlay(ov0898, HOOK_PERFORMED_VA, HOOK_PERFORMED_W0)?;
        expect_overlay(ov0898, HOOK_PERFORMED_VA + 4, HOOK_PERFORMED_W1)?;
        expect_overlay(ov0898, PERFORMED_RET_VA, PERFORMED_RET_W)?;
        assert_not_in_tables(
            PAGER_VA,
            (PAGER_WORDS * 4) as u32,
            OVERLAY_TABLE_RANGES,
            "pager",
        )?;

        // The fifteen rows, derived from this disc's own tables.
        let rows = super_art_rows(scus)?;
        let want = ARTS_CHARACTERS * SUPER_ARTS_PER_CHAR;
        if rows.len() != want {
            bail!(
                "show-super-arts: derived {} rows, expected {want}",
                rows.len()
            );
        }

        // --- Placement -------------------------------------------------------
        let mut gap = Region::new(SCUS_GAP_VA, SCUS_GAP_END_VA, "the SCUS rodata gap");
        let mut arena1 = Region::new(ARENA1_VA, ARENA1_END_VA, "arena 1");
        let mut arena2 = Region::new(ARENA2_VA, ARENA2_END_VA, "arena 2");
        let mut slot6 = Region::new(SLOT6_VA, SLOT6_END_VA, "slot 6");

        let id_len = (assemble_id(id_disp, 0, 0, 0, 0).len() * 4) as u32;
        let id_va = gap.take(id_len, 4, "the id routine")?;
        let scratch_va = gap.take(SCRATCH_BYTES as u32, 4, "the scratch record")?;
        let buf_va = gap.take(GLYPH_BUF_BYTES as u32, 1, "the glyph buffer")?;

        let fill_len = (assemble_fill(0, 0, 0, 0).len() * 4) as u32;
        let fill_va = arena1.take(fill_len, 4, "the fill routine")?;
        let arrows_va = arena1.take(want as u32 * ARROWS_STRIDE, 4, "the packed arrows")?;

        let count_len = (assemble_count(count_disp, 0).len() * 4) as u32;
        let count_va = arena2.take(count_len, 4, "the count routine")?;

        let sup_va = slot6.take(want as u32 * SUP_STRIDE, 4, "the Super Art records")?;

        // Every hosted span has to be dead space and outside every live table.
        for (r, what) in [
            (&gap, "gap 1"),
            (&arena1, "arena 1"),
            (&arena2, "arena 2"),
            (&slot6, "slot 6"),
        ] {
            assert_not_in_tables(r.va, r.used(), SCUS_TABLE_RANGES, what)?;
            assert_zero(scus, r.va, r.used() as usize)?;
        }

        // --- Assembly --------------------------------------------------------
        let count = assemble_count(count_disp, COUNT_RET_VA);
        let id = assemble_id(id_disp, id_va, sup_va, fill_va, ID_RET_VA);
        let fill = assemble_fill(fill_va, sup_va, arrows_va, scratch_va);
        debug_assert_eq!((id.len() * 4) as u32, id_len);
        debug_assert_eq!((fill.len() * 4) as u32, fill_len);

        let sup: Vec<u8> = rows.iter().flat_map(|r| r.sup_record()).collect();
        let arrows: Vec<u8> = rows.iter().flat_map(|r| r.packed_arrows()).collect();
        // The scratch record: `+8` is fixed at the glyph buffer; `+2` and `+0xC`
        // are filled per row by (F).
        let mut scratch = vec![0u8; SCRATCH_BYTES];
        let a = SCRATCH_ARROWS_OFF as usize;
        scratch[a..a + 4].copy_from_slice(&buf_va.to_le_bytes());

        // The pager replacement plus routine (W) in its tail, nop-padded out to
        // the original body length so no stale retail instruction survives.
        let mut pager = assemble_pager(PAGER_VA);
        let performed_va = PAGER_VA + (pager.len() as u32) * 4;
        let performed = assemble_performed(PERFORMED_RET_VA);
        pager.extend_from_slice(&performed);
        if pager.len() > PAGER_WORDS {
            bail!(
                "show-super-arts: the replacement pager + performed routine ({} words) does \
                 not fit FUN_801D3748 ({PAGER_WORDS} words)",
                pager.len()
            );
        }
        pager.resize(PAGER_WORDS, nop());

        let detour = |target_va: u32| -> Vec<u8> { words_to_bytes(&[j(target_va), nop()]) };
        let scus_edit = |off: usize, bytes: Vec<u8>| Edit {
            prot_index: None,
            file_off: off,
            bytes,
        };
        let ov_edit = |va: u32, bytes: Vec<u8>| Edit {
            prot_index: Some(OVERLAY_PROT_INDEX),
            file_off: (va - OVERLAY_BASE_VA) as usize,
            bytes,
        };
        let edits = vec![
            // Detours over the renderer's two sites (two words each).
            scus_edit(scus_off(scus, HOOK_COUNT_VA)?, detour(count_va)),
            scus_edit(scus_off(scus, HOOK_ID_VA)?, detour(id_va)),
            // Routines + tables into the verified-dead SCUS regions.
            scus_edit(scus_off(scus, count_va)?, words_to_bytes(&count)),
            scus_edit(scus_off(scus, id_va)?, words_to_bytes(&id)),
            scus_edit(scus_off(scus, fill_va)?, words_to_bytes(&fill)),
            scus_edit(scus_off(scus, sup_va)?, sup),
            scus_edit(scus_off(scus, arrows_va)?, arrows),
            scus_edit(scus_off(scus, scratch_va)?, scratch),
            // The pager (with W in its tail), replaced whole inside the overlay,
            // and the applier's two-word detour into W.
            ov_edit(PAGER_VA, words_to_bytes(&pager)),
            ov_edit(HOOK_PERFORMED_VA, detour(performed_va)),
        ];

        Ok(Self {
            edits,
            rows,
            count_va,
            id_va,
            fill_va,
            performed_va,
            sup_va,
            arrows_va,
            scratch_va,
            buf_va,
        })
    }
}
