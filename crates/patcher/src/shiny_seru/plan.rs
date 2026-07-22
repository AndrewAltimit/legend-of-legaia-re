//! The planned injection: the same-size [`Edit`] set, the [`ShinySeruInjection`]
//! type, the hook resolvers + dead-space / live-table guards, and the
//! bump-allocated code-cave layout that stitches every routine into the image.

use anyhow::{Result, bail};

use legaia_asset::item_names;

use super::encode::{j, nop};
use super::layout::*;
use super::routines::*;
use crate::mips::read_word;

/// One same-size write into a target file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// `None` = `SCUS_942.54`; `Some(idx)` = PROT entry `idx` (raw).
    pub prot_index: Option<usize>,
    /// File offset within that target.
    pub file_off: usize,
    /// Little-endian bytes to write.
    pub bytes: Vec<u8>,
}

/// A planned shiny-Seru injection: all the same-size writes + the chosen `pct`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShinySeruInjection {
    pub edits: Vec<Edit>,
    pub pct: u8,
}

fn words_to_bytes(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Resolve a SCUS VA to a file offset and confirm the two hook words: the first
/// must equal `expect_w0` (build fingerprint); the pair is returned to replay.
fn scus_hook(scus: &[u8], va: u32, expect_w0: u32) -> Result<(usize, [u32; 2])> {
    let off = item_names::file_offset_for_va(scus, va)
        .ok_or_else(|| anyhow::anyhow!("can't resolve SCUS VA {va:#x}"))?;
    let w0 = read_word(scus, off)?;
    let w1 = read_word(scus, off + 4)?;
    if w0 != expect_w0 {
        bail!("SCUS hook {va:#x} = {w0:#010x}, expected {expect_w0:#010x} (unrecognized build)");
    }
    Ok((off, [w0, w1]))
}

/// Same for an overlay VA (`file_off = va - OVERLAY_BASE_VA`).
fn ov_hook(overlay: &[u8], va: u32, expect_w0: u32) -> Result<(usize, [u32; 2])> {
    let off = (va - OVERLAY_BASE_VA) as usize;
    let w0 = read_word(overlay, off)?;
    let w1 = read_word(overlay, off + 4)?;
    if w0 != expect_w0 {
        bail!("overlay hook {va:#x} = {w0:#010x}, expected {expect_w0:#010x} (unrecognized build)");
    }
    Ok((off, [w0, w1]))
}

/// Refuse if `[va, va+len)` overlaps any known live data table - even all-zero
/// bytes there are indexed at runtime (the victory mouth-override table, the
/// move-power table). This is the structural guard that makes "is it zero?"
/// insufficient: a region must also be **outside every table** to be safe.
pub(crate) fn assert_not_in_tables(
    va: u32,
    len: u32,
    ranges: &[(u32, u32)],
    what: &str,
) -> Result<()> {
    let end = va.saturating_add(len);
    for &(a, b) in ranges {
        if va < b && a < end {
            bail!(
                "shiny {what} region {va:#x}..+{len} overlaps live table {a:#x}..{b:#x} \
                 (zero-padded but indexed at runtime) - refusing"
            );
        }
    }
    Ok(())
}

/// Confirm `[va, va+len)` is all-zero dead space in `buf` (file offset = `off`).
fn assert_zero(buf: &[u8], off: usize, len: usize, va: u32) -> Result<()> {
    let region = buf
        .get(off..off + len)
        .ok_or_else(|| anyhow::anyhow!("region {va:#x}..+{len} past end of file"))?;
    if region.iter().any(|&b| b != 0) {
        bail!("region {va:#x}..+{len} is not all-zero dead space (build / collision) - refusing");
    }
    Ok(())
}

impl ShinySeruInjection {
    /// Plan all edits for `pct`% shiny capturable enemies. Needs the
    /// `SCUS_942.54` image, the battle-action overlay (0898) and the menu
    /// overlay (0899) raw PROT entries. Refuses (without touching anything) if
    /// the build isn't the recognized US layout or a routine region isn't dead.
    pub fn plan(
        scus: &[u8],
        ov0898: &[u8],
        ov0899: &[u8],
        pct: u8,
        capturable_ids: &[u16],
    ) -> Result<Self> {
        if pct == 0 || pct > 100 {
            bail!("shiny-seru percent {pct} out of range 1..=100");
        }

        // Resolve + fingerprint every hook (also captures the words to replay).
        let setup = scus_hook(scus, HOOK_SETUP_VA, HOOK_SETUP_W0)?;
        let capture = ov_hook(ov0898, HOOK_CAPTURE_VA, HOOK_CAPTURE_W0)?;
        let grant = ov_hook(ov0898, HOOK_GRANT_VA, HOOK_GRANT_W0)?;
        let gshift = ov_hook(ov0898, HOOK_GRANT_SHIFT_VA, HOOK_GRANT_SHIFT_W0)?;
        let damage = ov_hook(ov0898, HOOK_DAMAGE_VA, HOOK_DAMAGE_W0)?;
        let menu = ov_hook(ov0899, HOOK_MENU_VA, HOOK_MENU_W0)?;
        let bmenu = ov_hook(ov0898, HOOK_BMENU_LVL_VA, HOOK_BMENU_LVL_W0)?;
        let banner = scus_hook(scus, HOOK_BANNER_VA, HOOK_BANNER_W0)?;
        let fade = scus_hook(scus, HOOK_FADE_VA, HOOK_FADE_W0)?;

        // Sanity: no overlay-0898 hook site sits inside the move-power table
        // window (where the old cave wrongly lived).
        for (va, name) in [
            (HOOK_CAPTURE_VA, "capture-hook"),
            (HOOK_GRANT_VA, "grant-hook"),
            (HOOK_GRANT_SHIFT_VA, "grant-shift-hook"),
            (HOOK_DAMAGE_VA, "damage-hook"),
            (HOOK_BMENU_LVL_VA, "bmenu-hook"),
        ] {
            assert_not_in_tables(va, 8, OVERLAY_TABLE_RANGES, name)?;
        }

        // Capturable allowlist bitmap + the +35% string (data; placed in gap 1,
        // after scratch + B + C1 - all read-watch-verified-dead on a live battle).
        let bitmap = build_bitmap(capturable_ids);
        let banner_str = banner_string();
        let bitmap_va = SHINY_CAST_FLAG_VA - BITMAP_BYTES as u32;

        // Bump-allocator over a verified-dead arena: place a routine, advance the
        // cursor, and refuse if it overruns the arena OR overlaps any known live
        // table (the trap that put the previous layout inside the victory
        // mouth-override table and the move-power table - zero slots that the game
        // still indexes).
        let place =
            |cursor: &mut u32, end: u32, words: Vec<u32>, what: &str| -> Result<(u32, Vec<u32>)> {
                let va = *cursor;
                // A routine is the target of a `j` detour; `j` drops the low 2
                // bits, so an unaligned entry jumps into garbage. Refuse it.
                if va & 3 != 0 {
                    bail!("shiny {what} routine VA {va:#x} is not 4-byte aligned");
                }
                let len = (words.len() * 4) as u32;
                if va + len > end {
                    bail!("shiny {what} routine overruns its arena end {end:#x}");
                }
                assert_not_in_tables(va, len, SCUS_TABLE_RANGES, what)?;
                *cursor += len;
                Ok((va, words))
            };

        // --- gap 1 (before the steal table): scratch + setup (B) + capture (C1)
        //     + bitmap + cast flag + +35% string (data) -------------------------
        let scratch_va = SCUS_GAP_VA;
        let mut gap1 = SCUS_GAP_VA + 4; // 4-byte scratch word reserved first
        let (b_va, b_words) = place(
            &mut gap1,
            SCUS_GAP_END_VA,
            assemble_setup(pct, bitmap_va, setup.1, HOOK_SETUP_VA + 8),
            "setup",
        )?;
        let (c1_va, c1_words) = place(
            &mut gap1,
            SCUS_GAP_END_VA,
            assemble_capture_copy(scratch_va, capture.1, HOOK_CAPTURE_VA + 8),
            "capture",
        )?;
        // gap-1 data region: bitmap, then the 1-byte cast flag, then the string.
        debug_assert_eq!(gap1, bitmap_va, "bitmap follows scratch+B+C1 in gap 1");
        debug_assert_eq!(
            SHINY_CAST_FLAG_VA,
            bitmap_va + BITMAP_BYTES as u32,
            "cast flag after bitmap"
        );
        debug_assert_eq!(
            BANNER_STR_VA,
            SHINY_CAST_FLAG_VA + 1,
            "string after cast flag"
        );
        let gap1_data_span = BITMAP_BYTES as u32 + 1 + banner_str.len() as u32;
        if bitmap_va + gap1_data_span > SCUS_GAP_END_VA {
            bail!("gap-1 data overruns the steal table at {SCUS_GAP_END_VA:#x}");
        }
        assert_not_in_tables(bitmap_va, gap1_data_span, SCUS_TABLE_RANGES, "gap1-data")?;
        gap1 += gap1_data_span;

        // --- arena 1: damage (D), grant (C2), grant-shift (K2), in-battle-menu
        //     flag (H), field-menu colour (F) ----------------------------------
        let mut a1 = ARENA1_VA;
        let (d_va, d_words) = place(
            &mut a1,
            ARENA1_END_VA,
            assemble_damage(damage.1, HOOK_DAMAGE_VA + 8),
            "damage",
        )?;
        let (c2_va, c2_words) = place(
            &mut a1,
            ARENA1_END_VA,
            assemble_grant_shiny(scratch_va, grant.1, HOOK_GRANT_VA + 8),
            "grant",
        )?;
        let (k2_va, k2_words) = place(
            &mut a1,
            ARENA1_END_VA,
            assemble_grant_shift(gshift.1, HOOK_GRANT_SHIFT_VA + 8),
            "grant-shift",
        )?;
        let (h_va, h_words) = place(
            &mut a1,
            ARENA1_END_VA,
            assemble_bmenu(bmenu.1, HOOK_BMENU_LVL_VA + 8),
            "battle-menu",
        )?;
        let (f_va, f_words) = place(
            &mut a1,
            ARENA1_END_VA,
            assemble_menu_color(menu.1, HOOK_MENU_VA + 8),
            "menu",
        )?;
        debug_assert_eq!(k2_va, SHIFT_RUN_VA, "grant-shift VA matches the const");
        debug_assert_eq!(h_va, BMENU_RUN_VA, "bmenu VA matches the const");
        debug_assert_eq!(f_va, MENU_RUN_VA, "menu VA matches the const");

        // --- slot 6: summon-fade (K) ---------------------------------------
        let mut s6 = SLOT6_VA;
        let (k_va, k_words) = place(
            &mut s6,
            SLOT6_END_VA,
            assemble_summon_fade(fade.1, HOOK_FADE_RET_VA),
            "summon-fade",
        )?;
        debug_assert_eq!(k_va, SUMMON_FADE_RUN_VA, "summon-fade VA matches the const");

        // --- arena 2: +35% cast-banner routine (J) -------------------------
        let banner_words = assemble_banner_replace(BANNER_STR_VA, banner.1, HOOK_BANNER_RET_VA);
        let banner_len = (banner_words.len() * 4) as u32;
        assert_not_in_tables(BANNER_RUN_VA, banner_len, SCUS_TABLE_RANGES, "banner")?;
        if BANNER_RUN_VA + banner_len > ARENA2_END_VA {
            bail!("banner routine overruns arena 2 end {ARENA2_END_VA:#x}");
        }

        // --- dead-space guards: every region must be all-zero in the clean image
        //     (necessary, not sufficient - the regions are also read-watch-verified
        //     unreferenced on a live battle, the part a static check can't prove). -
        let dead = |va: u32, len: usize, what: &str| -> Result<()> {
            let off = item_names::file_offset_for_va(scus, va)
                .ok_or_else(|| anyhow::anyhow!("can't resolve {what} VA {va:#x}"))?;
            assert_zero(scus, off, len, va)
        };
        dead(SCUS_GAP_VA, (gap1 - SCUS_GAP_VA) as usize, "gap1")?;
        dead(ARENA1_VA, (a1 - ARENA1_VA) as usize, "arena1")?;
        dead(SLOT6_VA, (s6 - SLOT6_VA) as usize, "slot6")?;
        dead(BANNER_RUN_VA, banner_len as usize, "banner")?;

        // --- collect all edits ---------------------------------------------
        let detour = |target_va: u32| -> Vec<u8> { words_to_bytes(&[j(target_va), nop()]) };
        let scus_off = |va: u32| item_names::file_offset_for_va(scus, va).unwrap();

        let mut edits = vec![
            // Detours (each [j, nop] over the two displaced words). The routines
            // they target are all SCUS-resident now (no more 0898 cave).
            Edit {
                prot_index: None,
                file_off: setup.0,
                bytes: detour(b_va),
            },
            Edit {
                prot_index: Some(BATTLE_ACTION_OVERLAY_PROT_INDEX),
                file_off: capture.0,
                bytes: detour(c1_va),
            },
            Edit {
                prot_index: Some(BATTLE_ACTION_OVERLAY_PROT_INDEX),
                file_off: grant.0,
                bytes: detour(c2_va),
            },
            Edit {
                prot_index: Some(BATTLE_ACTION_OVERLAY_PROT_INDEX),
                file_off: gshift.0,
                bytes: detour(k2_va),
            },
            Edit {
                prot_index: Some(BATTLE_ACTION_OVERLAY_PROT_INDEX),
                file_off: damage.0,
                bytes: detour(d_va),
            },
            Edit {
                prot_index: Some(MENU_OVERLAY_PROT_INDEX),
                file_off: menu.0,
                bytes: detour(f_va),
            },
            Edit {
                prot_index: Some(BATTLE_ACTION_OVERLAY_PROT_INDEX),
                file_off: bmenu.0,
                bytes: detour(h_va),
            },
            Edit {
                prot_index: None,
                file_off: banner.0,
                bytes: detour(BANNER_RUN_VA),
            },
            Edit {
                prot_index: None,
                file_off: fade.0,
                bytes: detour(k_va),
            },
            // SCUS-hosted routines + data.
            Edit {
                prot_index: None,
                file_off: scus_off(b_va),
                bytes: words_to_bytes(&b_words),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(c1_va),
                bytes: words_to_bytes(&c1_words),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(d_va),
                bytes: words_to_bytes(&d_words),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(c2_va),
                bytes: words_to_bytes(&c2_words),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(k_va),
                bytes: words_to_bytes(&k_words),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(k2_va),
                bytes: words_to_bytes(&k2_words),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(f_va),
                bytes: words_to_bytes(&f_words),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(h_va),
                bytes: words_to_bytes(&h_words),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(BANNER_RUN_VA),
                bytes: words_to_bytes(&banner_words),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(BANNER_STR_VA),
                bytes: banner_str.clone(),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(bitmap_va),
                bytes: bitmap.clone(),
            },
        ];
        edits.shrink_to_fit();

        Ok(Self { edits, pct })
    }
}
