//! **Arts name-length fix, by ZetaPhoenix** - fixes retail's mis-centred
//! Super / Miracle Art name banner.
//!
//! ## The retail bug
//!
//! Two places store an art's display name: the SCUS arts-name table (the real
//! names of regular and Hyper Arts) and the arts animation data (placeholder
//! names for regular/Hyper Arts, the *real* names of Super Arts and the
//! Miracle Art finisher). The banner routine `FUN_8004AD80` first measures the
//! name behind the fixed pointer at `0x80076024` ("Vulture Blade", the
//! measure at `0x8004BBB4`), then a later check remeasures with the correct
//! table name - but **only for regular/Hyper Arts**. A Super or Miracle
//! finisher keeps Vulture Blade's width no matter which one fired, so its
//! banner is centred for the wrong name. Root cause traced by ZetaPhoenix.
//!
//! ## The fix
//!
//! ZetaPhoenix's patch: a 3-word detour at `0x8004BC3C` - the banner path's
//! `li a0,0x4C; jal FUN_801D8DE8; move a1,zero` tail - into a 17-instruction
//! routine that re-measures the **final** name pointer (`+0x74C` of the banner
//! block at `0x80076C10`, the very pointer the name installer just wrote),
//! recomputes `x = 160 - width/2`, stores it into the four banner X halfwords
//! (`+0x742`/`+0x73A`/`+0x72A`/`+0x722`), replays the three displaced words,
//! and returns to `0x8004BC48`. It corrects every banner - vanilla's five
//! Super Arts and Miracle finishers included - so it doubles as a standalone
//! retail bug fix and as an update to his Super Arts Pack.
//!
//! ## What is his, and the one relocation
//!
//! The instruction stream is ZetaPhoenix's, byte-for-byte as shipped in his
//! patch ([`ROUTINE_WORDS`] carries his words verbatim; the routine is
//! position-independent - every jump in it is absolute to a fixed target).
//! His patch parks the routine at `0x80079100`, a 128-byte all-zero run this
//! project's reference scan also finds unreferenced in every image - but the
//! address sits inside the `0x80078D00..0x80079800` SsAPI sound-table window,
//! the exact cluster where an earlier all-zero-therefore-dead assumption
//! produced the Healing-Leaf freeze (zero *padding between live tables* is
//! reachable by indexed reads a static scan cannot see). This carrier holds
//! injection sites to the read-watch standard, so it parks the routine in
//! **verified-dead arena 1** instead ([`crate::shiny_seru::ARENA1_VA`],
//! read-watch-verified unreferenced across a live battle with an item use, a
//! victory pose and a summon cast) and re-targets only the hook's `j` word.
//! Standalone the routine sits at the arena head; installed alongside the
//! Super Arts Pack it sits directly behind the pack's battle-load stub.

use anyhow::{Result, bail};

use crate::mips::{A1, addiu, j, lui};
use crate::shiny_seru::{ARENA1_END_VA, ARENA1_VA, Edit, SCUS_TABLE_RANGES};

/// The hook site: the banner path's `li a0,0x4C` at `0x8004BC3C`.
pub const HOOK_VA: u32 = 0x8004_BC3C;
/// The three retail words the hook displaces (`li a0,0x4C`,
/// `jal FUN_801D8DE8`, `move a1,zero`) - replayed verbatim as the routine's
/// own tail (words 12..=14 of [`ROUTINE_WORDS`]).
pub const HOOK_DISPLACED: [u32; 3] = [0x2404_004C, 0x0C07_637A, 0x0000_2821];
/// Where the routine returns (baked into his final `j`).
pub const HOOK_RET_VA: u32 = 0x8004_BC48;

/// Where ZetaPhoenix's own patch parks the routine. Recorded for provenance;
/// this carrier relocates (see the module docs) and never writes here.
pub const AUTHOR_ROUTINE_VA: u32 = 0x8007_9100;

/// ZetaPhoenix's routine, verbatim (decoded from his patch): re-measure the
/// installed name, centre the four banner X halfwords, replay the displaced
/// words, return. The final word is the return jump's delay-slot `nop` - part
/// of the claimed region.
pub const ROUTINE_WORDS: [u32; 18] = [
    0x8CA4_074C, // lw   a0, 0x74c(a1)      ; the installed banner-name pointer
    0x0C00_D7C1, // jal  0x80035F04         ; retail's text-width measure
    0x0000_0000, //  nop
    0x3C05_8007, // lui  a1, 0x8007         ; re-materialise the banner block
    0x24A5_6C10, // addiu a1, a1, 0x6c10    ;   base 0x80076C10 after the call
    0x0002_1043, // sra  v0, v0, 1          ; width / 2
    0x2403_00A0, // li   v1, 0xA0           ; screen centre x = 160
    0x0062_2023, // subu a0, v1, v0
    0xA4A4_0742, // sh   a0, 0x742(a1)      ; the four banner X halfwords
    0xA4A4_073A, // sh   a0, 0x73a(a1)
    0xA4A4_072A, // sh   a0, 0x72a(a1)
    0xA4A4_0722, // sh   a0, 0x722(a1)
    0x2404_004C, // li   a0, 0x4C           ; replay the displaced tail
    0x0C07_637A, // jal  0x801D8DE8
    0x2405_0000, //  li  a1, 0              ; (delay) his form of `move a1,zero`
    0x0801_2F12, // j    0x8004BC48         ; back into the banner path
    0x0000_0000, //  nop (delay)
    0x0000_0000, // nop - his reserved tail word, kept so the claim is his size
];
/// Bytes the routine region claims.
pub const ROUTINE_BYTES: u32 = ROUTINE_WORDS.len() as u32 * 4;

/// Assemble the 3-word hook: `lui a1,0x8007; j routine; addiu a1,a1,0x6c10` -
/// the delay slot finishes materialising `a1 = 0x80076C10` for the routine's
/// first load. Identical to ZetaPhoenix's hook except the `j` target.
pub fn assemble_hook(routine_va: u32) -> [u32; 3] {
    [lui(A1, 0x8007), j(routine_va), addiu(A1, A1, 0x6C10)]
}

/// A planned arts-name-fix injection: two same-size edits (hook + routine),
/// both in `SCUS_942.54`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtsNameFixInjection {
    /// Same-size edits (`None` target = `SCUS_942.54`).
    pub edits: Vec<Edit>,
    /// Where the routine was parked.
    pub routine_va: u32,
}

impl ArtsNameFixInjection {
    /// Plan the fix against a real `SCUS_942.54`, parking the routine at
    /// `routine_va` (arena 1 only). Refuses - writing nothing - unless the
    /// hook site carries its known retail words and the routine region is
    /// all-zero dead space inside arena 1, clear of every live table.
    pub fn plan(scus: &[u8], routine_va: u32) -> Result<Self> {
        if !routine_va.is_multiple_of(4) {
            bail!("arts-name-fix: routine VA {routine_va:#x} is not word-aligned");
        }
        if routine_va < ARENA1_VA || routine_va + ROUTINE_BYTES > ARENA1_END_VA {
            bail!(
                "arts-name-fix: routine region {routine_va:#x}..+{ROUTINE_BYTES} \
                 leaves arena 1 ({ARENA1_VA:#x}..{ARENA1_END_VA:#x})"
            );
        }
        for &(a, b) in SCUS_TABLE_RANGES {
            if routine_va < b && a < routine_va + ROUTINE_BYTES {
                bail!(
                    "arts-name-fix: routine region {routine_va:#x}..+{ROUTINE_BYTES} \
                     overlaps live table {a:#x}..{b:#x} - refusing"
                );
            }
        }

        let hook_off = scus_off(scus, HOOK_VA)?;
        for (i, &want) in HOOK_DISPLACED.iter().enumerate() {
            let got = crate::mips::read_word(scus, hook_off + i * 4)?;
            if got != want {
                bail!(
                    "arts-name-fix: SCUS {va:#x} = {got:#010x}, expected {want:#010x} \
                     (unrecognized build - nothing written)",
                    va = HOOK_VA + i as u32 * 4
                );
            }
        }

        let routine_off = scus_off(scus, routine_va)?;
        let region = scus
            .get(routine_off..routine_off + ROUTINE_BYTES as usize)
            .ok_or_else(|| anyhow::anyhow!("arts-name-fix: routine region past end of SCUS"))?;
        if region.iter().any(|&b| b != 0) {
            bail!(
                "arts-name-fix: routine region {routine_va:#x}..+{ROUTINE_BYTES} is not \
                 all-zero dead space (another injection holds it) - refusing"
            );
        }

        let words_to_bytes =
            |w: &[u32]| -> Vec<u8> { w.iter().flat_map(|x| x.to_le_bytes()).collect() };
        let edits = vec![
            Edit {
                prot_index: None,
                file_off: hook_off,
                bytes: words_to_bytes(&assemble_hook(routine_va)),
            },
            Edit {
                prot_index: None,
                file_off: routine_off,
                bytes: words_to_bytes(&ROUTINE_WORDS),
            },
        ];
        Ok(Self { edits, routine_va })
    }
}

fn scus_off(scus: &[u8], va: u32) -> Result<usize> {
    legaia_asset::item_names::file_offset_for_va(scus, va)
        .ok_or_else(|| anyhow::anyhow!("arts-name-fix: can't resolve SCUS VA {va:#x}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mips_sim::Cpu;

    /// The routine replays exactly the words the hook displaces, and its
    /// return jump targets the word after the hook - the pairing that pins
    /// the hook site from the routine alone.
    #[test]
    fn routine_replays_the_displaced_tail_and_returns_after_the_hook() {
        assert_eq!(ROUTINE_WORDS[12], HOOK_DISPLACED[0], "li a0,0x4C");
        assert_eq!(ROUTINE_WORDS[13], HOOK_DISPLACED[1], "jal FUN_801D8DE8");
        // His `li a1,0` and retail's `move a1,zero` both leave a1 = 0.
        assert_eq!(ROUTINE_WORDS[14], 0x2405_0000, "a1 = 0, his encoding");
        assert_eq!(
            ROUTINE_WORDS[15],
            j(HOOK_RET_VA),
            "returns to the word after the hook"
        );
        assert_eq!(HOOK_RET_VA, HOOK_VA + 12, "three displaced words");
    }

    /// The hook materialises `a1 = 0x80076C10` across its delay slot, exactly
    /// as ZetaPhoenix's hook does.
    #[test]
    fn hook_materialises_the_banner_block_base() {
        let hook = assemble_hook(ARENA1_VA);
        assert_eq!(hook[0], 0x3C05_8007, "lui a1,0x8007");
        assert_eq!(hook[1], j(ARENA1_VA));
        assert_eq!(hook[2], 0x24A5_6C10, "addiu a1,a1,0x6c10");
    }

    /// Run hook + routine in the interpreter over a fake banner block: the
    /// four X halfwords must become `160 - width/2` for the *installed* name,
    /// and the displaced tail must be replayed (`a0 = 0x4C`, `a1 = 0`) before
    /// control returns to `0x8004BC48`.
    #[test]
    fn routine_centres_the_banner_for_the_installed_name() {
        const BANNER: u32 = 0x8007_6C10;
        const NAME: u32 = 0x8009_5000;
        const MEASURE: u32 = 0x8003_5F04;
        const REDRAW: u32 = 0x801D_8DE8;
        for width in [26u32, 91, 120] {
            let mut cpu = Cpu::new();
            cpu.load_words(HOOK_VA, &assemble_hook(ARENA1_VA));
            cpu.load_words(ARENA1_VA, &ROUTINE_WORDS);
            // measure stub: v0 = width; redraw stub: return immediately.
            cpu.load_words(
                MEASURE,
                &[crate::mips::addiu(
                    crate::mips::V0,
                    crate::mips::ZERO,
                    width as u16,
                )],
            );
            cpu.load_words(MEASURE + 4, &[crate::mips::jr(crate::mips::RA), 0]);
            cpu.load_words(REDRAW, &[crate::mips::jr(crate::mips::RA), 0]);
            cpu.wr32(BANNER + 0x74C, NAME);
            cpu.r[29] = 0x801F_F000; // sp
            cpu.pc = HOOK_VA;
            cpu.run_until(&[HOOK_RET_VA]);
            let want = (160 - width / 2) as u16;
            for off in [0x742u32, 0x73A, 0x72A, 0x722] {
                assert_eq!(cpu.rd16(BANNER + off), want, "banner X at +{off:#x}");
            }
            assert_eq!(cpu.r[4], 0x4C, "replayed a0 = 0x4C");
            assert_eq!(cpu.r[5], 0, "replayed a1 = 0");
        }
    }

    /// The routine fits arena 1 both standalone (arena head) and behind the
    /// Super Arts Pack's battle-load stub.
    #[test]
    fn routine_fits_both_arena_placements() {
        const { assert!(ARENA1_VA + ROUTINE_BYTES <= ARENA1_END_VA) };
        const { assert!(crate::super_arts_pack::ARENA_USED_END_VA + ROUTINE_BYTES <= ARENA1_END_VA) };
    }

    /// The author's own parking address is inside the guarded SsAPI window -
    /// the reason this carrier relocates. If the guarded ranges ever change so
    /// that `0x80079100` clears them, this test flags the relocation for
    /// re-evaluation.
    #[test]
    fn author_va_is_inside_a_guarded_range_which_is_why_we_relocate() {
        let hit = SCUS_TABLE_RANGES
            .iter()
            .any(|&(a, b)| AUTHOR_ROUTINE_VA < b && a < AUTHOR_ROUTINE_VA + ROUTINE_BYTES);
        assert!(
            hit,
            "0x80079100 no longer overlaps a guarded table range - revisit \
             installing the routine at the author's own address"
        );
    }
}
