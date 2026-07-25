//! **Spirit AP tuning**: set how much AP the Spirit command charges into the
//! battle AP gauge (retail 32), from 0 (Spirit is defence-only) up to 100
//! (one Spirit press fills the whole gauge).
//!
//! ## Where the retail value lives
//!
//! The battle-action state machine `FUN_801E295C` (battle-action overlay,
//! PROT entry 898, base VA `0x801CE818`) applies the per-action AP accrual in
//! its state-`0x50` cleanup arm: `actor[+0x224]` is set to `8` for every
//! action, then overwritten with `0x20` when the action category
//! (`actor[+0x1DE]`) is `4` = Spirit, and finally added into the AP gauge
//! (`actor[+0x170]`, clamped at 100):
//!
//! ```text
//! 801e5d7c  li  v0,0x4            ; category test: 4 = Spirit
//! 801e5d80  bne v1,v0,0x801e5d8c
//! 801e5d84  _li v0,0x20           ; <- the Spirit accrual (site A)
//! 801e5d88  sb  v0,0x224(s3)
//! ```
//!
//! The gauge *widget* ramp target for the Spirit command is computed
//! separately by the state-`0x46` entry arm, mirroring the accrual plus the
//! AP-Boost equipment bits (`+0x28 = 0x20 + (0x20 >> 2)` for AP Boost 2,
//! `+0x23 = 0x20 + 0x20/10` for AP Boost 1):
//!
//! ```text
//! 801e5318  lhu   a1,0x170(s3)
//! 801e5320  addiu v0,a1,0x20      ; <- widget base target (site B)
//! 801e536c  _addiu v0,a1,0x28     ; <- AP Boost 2 target  (site C)
//! 801e5378  _addiu v0,a1,0x23     ; <- AP Boost 1 target  (site D)
//! ```
//!
//! All four sites are `addiu` immediates - the patch rewrites only the
//! low 16 bits of each word (opcode and registers untouched), so it is a
//! same-size in-place edit of the raw PROT 898 entry. Site A is the value the
//! engine actually grants; B..D keep the on-screen gauge animation honest
//! (retail clamps the widget at 100 downstream, which stays in place).
//!
//! ## Why there is no negative range
//!
//! "Spirit *costs* AP" is not expressible as an immediate edit: the accrual
//! is stored with `sb` and read back with `lbu` (unsigned), and the add/clamp
//! window (`addu` / `andi 0xffff` / `sltiu 0x65`) has no room for a
//! floor-at-zero - a negative byte reads as +200-odd and pins the gauge at
//! 100. A signed variant needs injected code (a detour into verified-dead
//! SCUS space, like the arts AP-grant hook), not a table edit.
//!
//! Stock words are cited from the project's own disassembly reference
//! (`docs/subsystems/battle-action.md`); no Sony bytes are embedded. An
//! unrecognized build is refused, never corrupted; re-planning with a
//! different value re-targets a previously patched image.

use anyhow::{Result, bail};

/// PROT entry index of the battle-action overlay hosting `FUN_801E295C`.
pub const BATTLE_ACTION_OVERLAY_PROT_INDEX: usize =
    legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX;

/// Load base VA of the battle-action overlay (raw entry: file offset =
/// `va - OVERLAY_BASE_VA`).
pub const OVERLAY_BASE_VA: u32 = legaia_asset::move_power::BATTLE_OVERLAY_BASE;

/// The retail Spirit accrual: 32 AP per Spirit action.
pub const RETAIL_SPIRIT_AP: u8 = 0x20;

/// The gauge ceiling; also the largest configurable accrual.
pub const MAX_SPIRIT_AP: u8 = 100;

/// Site A - the state-`0x50` Spirit accrual immediate
/// (`addiu v0,zero,0x20` at `0x801E5D84`).
pub const GRANT_VA: u32 = 0x801E_5D84;
/// Site B - the state-`0x46` gauge-widget base target
/// (`addiu v0,a1,0x20` at `0x801E5320`).
pub const WIDGET_BASE_VA: u32 = 0x801E_5320;
/// Site C - the widget target under the AP Boost 2 equipment bit
/// (`addiu v0,a1,0x28` at `0x801E536C`).
pub const WIDGET_BOOST2_VA: u32 = 0x801E_536C;
/// Site D - the widget target under the AP Boost 1 equipment bit
/// (`addiu v0,a1,0x23` at `0x801E5378`).
pub const WIDGET_BOOST1_VA: u32 = 0x801E_5378;

/// `addiu v0,zero,imm` upper half (site A).
const GRANT_PREFIX: u32 = 0x2402_0000;
/// `addiu v0,a1,imm` upper half (sites B..D).
const WIDGET_PREFIX: u32 = 0x24A2_0000;

/// Largest immediate any site may legitimately hold: a boosted widget target
/// for the maximum accrual (`100 + 100/4 = 125`). Anything above that is not
/// a value this patch (or retail) ever writes.
const MAX_SITE_IMM: u32 = MAX_SPIRIT_AP as u32 + MAX_SPIRIT_AP as u32 / 4;

/// The four patch sites: `(va, imm_prefix, retail_imm, imm_for(ap))`.
fn sites(ap: u8) -> [(u32, u32, u16); 4] {
    let n = ap as u16;
    [
        (GRANT_VA, GRANT_PREFIX, n),
        (WIDGET_BASE_VA, WIDGET_PREFIX, n),
        (WIDGET_BOOST2_VA, WIDGET_PREFIX, n + n / 4),
        (WIDGET_BOOST1_VA, WIDGET_PREFIX, n + n / 10),
    ]
}

/// Context fingerprint: words adjacent to each site that hold whether or not
/// the sites were previously patched. `(va, stock_word)`.
const CONTEXT: [(u32, u32); 9] = [
    (0x801E_5D7C, 0x2402_0004), // li  v0,0x4        (category test)
    (0x801E_5D80, 0x1462_0002), // bne v1,v0,+2      (skip if not Spirit)
    (0x801E_5D88, 0xA262_0224), // sb  v0,0x224(s3)  (accrual store)
    (0x801E_5D8C, 0x92A2_0002), // lbu v0,0x2(s5)
    (0x801E_5318, 0x9665_0170), // lhu a1,0x170(s3)  (gauge load)
    (0x801E_5324, 0xA6E2_0008), // sh  v0,0x8(s7)    (widget base store)
    (0x801E_5368, 0x1440_0004), // bne v0,zero,+4    (AP Boost 2 test)
    (0x801E_5374, 0x1040_0002), // beq v0,zero,+2    (AP Boost 1 test)
    (0x801E_537C, 0xA6E2_0008), // sh  v0,0x8(s7)    (boosted store)
];

fn word_at(overlay: &[u8], va: u32) -> Result<u32> {
    let off = (va - OVERLAY_BASE_VA) as usize;
    let b = overlay.get(off..off + 4).ok_or_else(|| {
        anyhow::anyhow!("overlay entry too short for word at {va:#x} (+{off:#x})")
    })?;
    Ok(u32::from_le_bytes(b.try_into().unwrap()))
}

/// Verify the context fingerprint and that every site word has the expected
/// opcode/register prefix with a plausible immediate. Returns the current
/// per-site immediates on success.
fn recognize(overlay: &[u8]) -> Result<[u32; 4]> {
    for (va, want) in CONTEXT {
        let got = word_at(overlay, va)?;
        if got != want {
            bail!(
                "context word {va:#x} = {got:#010x}, expected {want:#010x} \
                 (battle-action Spirit accrual) - unrecognized build, refusing to patch"
            );
        }
    }
    let mut imms = [0u32; 4];
    for (i, (va, prefix, _)) in sites(0).iter().enumerate() {
        let got = word_at(overlay, *va)?;
        if got & 0xFFFF_0000 != *prefix || (got & 0xFFFF) > MAX_SITE_IMM {
            bail!(
                "site word {va:#x} = {got:#010x} is neither the stock Spirit accrual \
                 nor a previously configured value - unrecognized build, refusing to patch"
            );
        }
        imms[i] = got & 0xFFFF;
    }
    Ok(imms)
}

/// The Spirit accrual currently on the image (the site-A immediate), after
/// verifying the build is recognized. Retail reads 32.
pub fn current(overlay: &[u8]) -> Result<u8> {
    Ok(recognize(overlay)?[0] as u8)
}

/// A planned Spirit AP edit: same-size word rewrites in the battle-action
/// overlay PROT entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpiritAp {
    /// Per site: `(file offset within the overlay entry, replacement word)`.
    pub writes: Vec<(usize, u32)>,
    /// The accrual the image held before this plan (site A immediate).
    pub previous: u8,
}

/// Plan setting the Spirit accrual to `ap` (0..=100) against the raw
/// battle-action overlay entry. Returns `Ok(None)` when the image already
/// holds `ap` at every site (idempotent no-op). Refuses an unrecognized
/// build rather than corrupting it.
pub fn plan(overlay: &[u8], ap: u8) -> Result<Option<SpiritAp>> {
    if ap > MAX_SPIRIT_AP {
        bail!("spirit AP must be 0..={MAX_SPIRIT_AP}, got {ap}");
    }
    let imms = recognize(overlay)?;
    let previous = imms[0] as u8;
    let mut writes = Vec::new();
    for (i, (va, prefix, imm)) in sites(ap).iter().enumerate() {
        if imms[i] == *imm as u32 {
            continue;
        }
        let off = (*va - OVERLAY_BASE_VA) as usize;
        writes.push((off, prefix | *imm as u32));
    }
    if writes.is_empty() {
        return Ok(None);
    }
    Ok(Some(SpiritAp { writes, previous }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic overlay holding the stock context + site words.
    fn synth_overlay() -> Vec<u8> {
        let mut ov = vec![0u8; 0x18000];
        let mut put = |va: u32, w: u32| {
            let off = (va - OVERLAY_BASE_VA) as usize;
            ov[off..off + 4].copy_from_slice(&w.to_le_bytes());
        };
        for (va, w) in CONTEXT {
            put(va, w);
        }
        put(GRANT_VA, GRANT_PREFIX | 0x20);
        put(WIDGET_BASE_VA, WIDGET_PREFIX | 0x20);
        put(WIDGET_BOOST2_VA, WIDGET_PREFIX | 0x28);
        put(WIDGET_BOOST1_VA, WIDGET_PREFIX | 0x23);
        ov
    }

    #[test]
    fn retail_immediates_match_the_slider_math() {
        // The stock widget immediates are exactly what `sites(32)` computes:
        // 0x28 = 32 + 32/4, 0x23 = 32 + 32/10.
        let s = sites(RETAIL_SPIRIT_AP);
        assert_eq!(s[0].2, 0x20);
        assert_eq!(s[1].2, 0x20);
        assert_eq!(s[2].2, 0x28);
        assert_eq!(s[3].2, 0x23);
    }

    #[test]
    fn stock_image_reads_retail_and_plans_nothing_at_32() {
        let ov = synth_overlay();
        assert_eq!(current(&ov).unwrap(), RETAIL_SPIRIT_AP);
        assert_eq!(plan(&ov, RETAIL_SPIRIT_AP).unwrap(), None);
    }

    #[test]
    fn plan_rewrites_all_four_immediates() {
        let ov = synth_overlay();
        let p = plan(&ov, 100).unwrap().unwrap();
        assert_eq!(p.previous, RETAIL_SPIRIT_AP);
        assert_eq!(p.writes.len(), 4);
        let by_off: std::collections::BTreeMap<usize, u32> = p.writes.iter().copied().collect();
        assert_eq!(
            by_off[&((GRANT_VA - OVERLAY_BASE_VA) as usize)],
            GRANT_PREFIX | 100
        );
        assert_eq!(
            by_off[&((WIDGET_BASE_VA - OVERLAY_BASE_VA) as usize)],
            WIDGET_PREFIX | 100
        );
        assert_eq!(
            by_off[&((WIDGET_BOOST2_VA - OVERLAY_BASE_VA) as usize)],
            WIDGET_PREFIX | 125
        );
        assert_eq!(
            by_off[&((WIDGET_BOOST1_VA - OVERLAY_BASE_VA) as usize)],
            WIDGET_PREFIX | 110
        );
    }

    #[test]
    fn zero_disables_the_grant_entirely() {
        let ov = synth_overlay();
        let p = plan(&ov, 0).unwrap().unwrap();
        assert_eq!(p.writes.len(), 4);
        for (_, w) in &p.writes {
            assert_eq!(w & 0xFFFF, 0, "every immediate drops to zero");
        }
    }

    #[test]
    fn repatch_retargets_a_previously_patched_image() {
        let mut ov = synth_overlay();
        let p = plan(&ov, 50).unwrap().unwrap();
        for (off, w) in &p.writes {
            ov[*off..*off + 4].copy_from_slice(&w.to_le_bytes());
        }
        assert_eq!(current(&ov).unwrap(), 50);
        // Re-plan back to retail from the patched image.
        let back = plan(&ov, RETAIL_SPIRIT_AP).unwrap().unwrap();
        assert_eq!(back.previous, 50);
        assert_eq!(back.writes.len(), 4);
        // And planning the held value is a no-op.
        assert_eq!(plan(&ov, 50).unwrap(), None);
    }

    #[test]
    fn boost_immediates_track_the_retail_ratio() {
        // 50 -> 50 + 12 (>>2 equivalent: 50/4) and 50 + 5 (50/10).
        let ov = synth_overlay();
        let p = plan(&ov, 50).unwrap().unwrap();
        let by_off: std::collections::BTreeMap<usize, u32> = p.writes.iter().copied().collect();
        assert_eq!(
            by_off[&((WIDGET_BOOST2_VA - OVERLAY_BASE_VA) as usize)] & 0xFFFF,
            62
        );
        assert_eq!(
            by_off[&((WIDGET_BOOST1_VA - OVERLAY_BASE_VA) as usize)] & 0xFFFF,
            55
        );
    }

    #[test]
    fn refuses_out_of_range_and_unrecognized_builds() {
        let ov = synth_overlay();
        assert!(plan(&ov, 101).is_err());
        // Perturbed context word.
        let mut bad = ov.clone();
        let off = (0x801E_5D88 - OVERLAY_BASE_VA) as usize;
        bad[off] ^= 0xFF;
        assert!(plan(&bad, 10).is_err());
        // Perturbed site opcode.
        let mut bad2 = ov.clone();
        let off = (GRANT_VA - OVERLAY_BASE_VA) as usize + 3;
        bad2[off] = 0x3C; // no longer addiu v0,zero
        assert!(plan(&bad2, 10).is_err());
        // Truncated overlay.
        assert!(plan(&ov[..0x100], 10).is_err());
    }

    #[test]
    fn site_offsets_are_linear_from_base() {
        assert_eq!((GRANT_VA - OVERLAY_BASE_VA) as usize, 0x1756C);
        assert_eq!((WIDGET_BASE_VA - OVERLAY_BASE_VA) as usize, 0x16B08);
        assert_eq!((WIDGET_BOOST2_VA - OVERLAY_BASE_VA) as usize, 0x16B54);
        assert_eq!((WIDGET_BOOST1_VA - OVERLAY_BASE_VA) as usize, 0x16B60);
    }
}
