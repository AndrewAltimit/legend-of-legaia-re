//! **Spirit AP tuning**: set how much AP the Spirit command charges into the
//! battle AP gauge (retail 32), from 0 (Spirit is defence-only) up to 100
//! (one Spirit press fills the whole gauge), or **negative** - a Spirit press
//! that *drains* the gauge instead.
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
//! All four sites are `addiu` immediates - a **positive** setting rewrites
//! only the low 16 bits of each word (opcode and registers untouched). Site A
//! is the value the engine actually grants; B..D keep the on-screen gauge
//! animation honest (retail clamps the widget at 100 downstream, which stays
//! in place).
//!
//! ## Negative settings
//!
//! A negative setting stores the accrual as a **two's-complement byte** at the
//! same site A (`addiu v0,zero,-N` -> `sb` writes `0x100-N`) and rewrites the
//! consumer so the signed value is honoured. Retail reads the byte back
//! unsigned and adds it with only a ceiling clamp, which would read `-32` as
//! `+224` and pin the gauge at 100; four things change instead:
//!
//! 1. **Signed read + floor at zero** in the state-`0x50` add/clamp tail. The
//!    ceiling clamp is still needed there (the same tail also applies the `+8`
//!    every non-Spirit action grants), so the over-100 arm is relocated into
//!    the AP-Boost-1 block that step 3 makes dead:
//!
//!    ```text
//!    801e5e68  lb   v0,0x224(s3)        ; signed accrual
//!    801e5e74  addu v1,v1,v0            ; gauge + delta   (unchanged)
//!    801e5e78  bgez v1,0x801e5e84
//!    801e5e7c  _slti v0,v1,0x65         ; delay slot: ceiling test
//!    801e5e80  move v1,zero             ; floor at 0
//!    801e5e84  beq  v0,zero,0x801e5e40  ; over 100 -> scratch
//!    801e5e88  _sh  v1,0x170(s3)        ; delay slot: the store
//!    801e5e8c  nop
//!    ; scratch, in the dead AP-Boost-1 block:
//!    801e5e40  li   v1,0x64
//!    801e5e44  sh   v1,0x170(s3)
//!    801e5e48  j    0x801e5e90
//!    801e5e4c  _nop
//!    ```
//!
//! 2. **A floor on the widget ramp target**, replacing the now-unreachable
//!    ceiling clamp at `0x801E5380` (a drain can never target over 100).
//! 3. **The two AP-Boost arms are neutralized** - their guard branches become
//!    unconditional - because both read `+0x224` unsigned. An AP-Boost
//!    accessory therefore neither deepens nor softens a drain; it is inert
//!    while the setting is negative.
//! 4. **The widget targets B..D all become `-N`**, since no boost applies.
//!
//! Stock words are cited from the project's own disassembly reference
//! (`docs/subsystems/battle-action.md`); no Sony bytes are embedded. An
//! unrecognized build is refused, never corrupted; re-planning with a
//! different value re-targets a previously patched image in either direction.

use anyhow::{Result, bail};

/// PROT entry index of the battle-action overlay hosting `FUN_801E295C`.
pub const BATTLE_ACTION_OVERLAY_PROT_INDEX: usize =
    legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX;

/// Load base VA of the battle-action overlay (raw entry: file offset =
/// `va - OVERLAY_BASE_VA`).
pub const OVERLAY_BASE_VA: u32 = legaia_asset::move_power::BATTLE_OVERLAY_BASE;

/// The retail Spirit accrual: 32 AP per Spirit action.
pub const RETAIL_SPIRIT_AP: i16 = 0x20;

/// The gauge ceiling; also the largest configurable accrual in either
/// direction.
pub const MAX_SPIRIT_AP: i16 = 100;

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

/// Largest immediate a *positive* site may legitimately hold: a boosted
/// widget target for the maximum accrual (`100 + 100/4 = 125`).
const MAX_SITE_IMM: u32 = MAX_SPIRIT_AP as u32 + MAX_SPIRIT_AP as u32 / 4;

// ---------------------------------------------------------------------------
// Sites that only a negative setting rewrites
// ---------------------------------------------------------------------------

/// The state-`0x46` widget clamp (`0x801E5380` loads, these words clamp).
/// Retail caps the ramp target at 100; a drain floors it at 0 instead.
const WIDGET_CLAMP_VAS: [u32; 5] = [
    0x801E_5384,
    0x801E_5388,
    0x801E_538C,
    0x801E_5394,
    0x801E_5398,
];
/// Retail: `slti v0,v0,0x65` / `bne` over `li v0,0x64` + `sh v0,0x8(s7)`.
const WIDGET_CLAMP_RETAIL: [u32; 5] = [
    0x0000_0000, // nop                      (load delay)
    0x2842_0065, // slti v0,v0,0x65
    0x1440_0004, // bne  v0,zero,0x801e53a0
    0x2402_0064, // li   v0,0x64
    0xA6E2_0008, // sh   v0,0x8(s7)
];
/// Draining: floor the ramp target at 0; the ceiling is unreachable.
const WIDGET_CLAMP_FLOOR: [u32; 5] = [
    0x0441_0002, // bgez v0,0x801e5390
    0x0000_0000, // nop
    0xA6E0_0008, // sh   zero,0x8(s7)
    0x0000_0000, // nop
    0x0000_0000, // nop
];

/// Guard branches of the state-`0x50` AP-Boost arms. Both read `+0x224`
/// unsigned, so a drain makes them unconditional (the arms never run).
const BOOST2_BR_VA: u32 = 0x801E_5DE0;
const BOOST2_BR_RETAIL: u32 = 0x1040_0006; // beq v0,zero,0x801e5dfc
const BOOST2_BR_SKIP: u32 = 0x1000_0006; // beq zero,zero,0x801e5dfc
const BOOST1_BR_VA: u32 = 0x801E_5E38;
const BOOST1_BR_RETAIL: u32 = 0x1040_000B; // beq v0,zero,0x801e5e68
const BOOST1_BR_SKIP: u32 = 0x1000_000B; // beq zero,zero,0x801e5e68

/// Head of the AP-Boost-1 arm, dead once `BOOST1_BR_VA` is unconditional -
/// reused as the drain tail's "over 100" scratch.
const SCRATCH_VAS: [u32; 4] = [0x801E_5E40, 0x801E_5E44, 0x801E_5E48, 0x801E_5E4C];
const SCRATCH_RETAIL: [u32; 4] = [
    0x9263_0224, // lbu   v1,0x224(s3)
    0x3C02_CCCC, // lui   v0,0xcccc
    0x3442_CCCD, // ori   v0,v0,0xcccd
    0x0062_0019, // multu v1,v0
];
const SCRATCH_DRAIN: [u32; 4] = [
    0x2403_0064, // li v1,0x64
    0xA663_0170, // sh v1,0x170(s3)
    0x0807_97A4, // j  0x801e5e90
    0x0000_0000, // nop
];

/// The state-`0x50` accrual read + add/clamp tail. `0x801E5E6C`/`0x801E5E70`/
/// `0x801E5E74` sit between and are unchanged context.
const TAIL_VAS: [u32; 7] = [
    0x801E_5E68,
    0x801E_5E78,
    0x801E_5E7C,
    0x801E_5E80,
    0x801E_5E84,
    0x801E_5E88,
    0x801E_5E8C,
];
const TAIL_RETAIL: [u32; 7] = [
    0x9262_0224, // lbu   v0,0x224(s3)
    0x3062_FFFF, // andi  v0,v1,0xffff
    0x2C42_0065, // sltiu v0,v0,0x65
    0x1440_0003, // bne   v0,zero,0x801e5e90
    0xA663_0170, // _sh   v1,0x170(s3)
    0x2402_0064, // li    v0,0x64
    0xA662_0170, // sh    v0,0x170(s3)
];
const TAIL_SIGNED: [u32; 7] = [
    0x8262_0224, // lb    v0,0x224(s3)       (sign-extend)
    0x0461_0002, // bgez  v1,0x801e5e84
    0x2862_0065, // _slti v0,v1,0x65         (delay: ceiling test)
    0x0000_1821, // move  v1,zero            (floor at 0)
    0x1040_FFEE, // beq   v0,zero,0x801e5e40 (over 100 -> scratch)
    0xA663_0170, // _sh   v1,0x170(s3)
    0x0000_0000, // nop
];

/// Context fingerprint: words adjacent to each site that are never rewritten,
/// so an image that is not retail's Spirit accrual is refused outright.
const CONTEXT: [(u32, u32); 19] = [
    (0x801E_5318, 0x9665_0170), // lhu   a1,0x170(s3)  (gauge load, widget)
    (0x801E_5324, 0xA6E2_0008), // sh    v0,0x8(s7)    (widget base store)
    (0x801E_5368, 0x1440_0004), // bne   v0,zero,+4    (AP Boost 2 test)
    (0x801E_5374, 0x1040_0002), // beq   v0,zero,+2    (AP Boost 1 test)
    (0x801E_537C, 0xA6E2_0008), // sh    v0,0x8(s7)    (boosted store)
    (0x801E_5380, 0x86E2_0008), // lh    v0,0x8(s7)    (widget clamp load)
    (0x801E_5390, 0x2402_0020), // li    v0,0x20
    (0x801E_539C, 0x2402_0020), // li    v0,0x20
    (0x801E_53A0, 0xA6E2_0002), // sh    v0,0x2(s7)    (widget clamp join)
    (0x801E_5D60, 0x9264_0224), // lbu   a0,0x224(s3)  (cleanup arm head)
    (0x801E_5D68, 0x2403_0008), // li    v1,0x8        (default accrual)
    (0x801E_5D7C, 0x2402_0004), // li    v0,0x4        (category test)
    (0x801E_5D80, 0x1462_0002), // bne   v1,v0,+2      (skip if not Spirit)
    (0x801E_5D88, 0xA262_0224), // sb    v0,0x224(s3)  (accrual store)
    (0x801E_5D8C, 0x92A2_0002), // lbu   v0,0x2(s5)
    (0x801E_5E6C, 0x9663_0170), // lhu   v1,0x170(s3)  (gauge load)
    (0x801E_5E70, 0xA260_0224), // sb    zero,0x224(s3) (accrual consumed)
    (0x801E_5E74, 0x0062_1821), // addu  v1,v1,v0      (gauge + delta)
    (0x801E_5E90, 0x9262_01DC), // lbu   v0,0x1dc(s3)  (tail join)
];

fn word_at(overlay: &[u8], va: u32) -> Result<u32> {
    let off = (va - OVERLAY_BASE_VA) as usize;
    let b = overlay.get(off..off + 4).ok_or_else(|| {
        anyhow::anyhow!("overlay entry too short for word at {va:#x} (+{off:#x})")
    })?;
    Ok(u32::from_le_bytes(b.try_into().unwrap()))
}

/// The four `addiu` immediates for `ap`. Positive settings mirror retail's
/// AP-Boost widget arithmetic (`+25%` / `+10%`); a drain uses `-N` at every
/// site because the boost arms are switched off.
fn site_immediates(ap: i16) -> [u16; 4] {
    if ap < 0 {
        [ap as u16; 4]
    } else {
        let n = ap as u16;
        [n, n, n + n / 4, n + n / 10]
    }
}

/// The complete set of `(va, word)` the image should hold for `ap`.
fn desired(ap: i16) -> Vec<(u32, u32)> {
    let draining = ap < 0;
    let imm = site_immediates(ap);
    let mut out = vec![
        (GRANT_VA, GRANT_PREFIX | imm[0] as u32),
        (WIDGET_BASE_VA, WIDGET_PREFIX | imm[1] as u32),
        (WIDGET_BOOST2_VA, WIDGET_PREFIX | imm[2] as u32),
        (WIDGET_BOOST1_VA, WIDGET_PREFIX | imm[3] as u32),
        (
            BOOST2_BR_VA,
            if draining {
                BOOST2_BR_SKIP
            } else {
                BOOST2_BR_RETAIL
            },
        ),
        (
            BOOST1_BR_VA,
            if draining {
                BOOST1_BR_SKIP
            } else {
                BOOST1_BR_RETAIL
            },
        ),
    ];
    let widget = if draining {
        WIDGET_CLAMP_FLOOR
    } else {
        WIDGET_CLAMP_RETAIL
    };
    for (va, word) in WIDGET_CLAMP_VAS.iter().zip(widget) {
        out.push((*va, word));
    }
    let scratch = if draining {
        SCRATCH_DRAIN
    } else {
        SCRATCH_RETAIL
    };
    for (va, word) in SCRATCH_VAS.iter().zip(scratch) {
        out.push((*va, word));
    }
    let tail = if draining { TAIL_SIGNED } else { TAIL_RETAIL };
    for (va, word) in TAIL_VAS.iter().zip(tail) {
        out.push((*va, word));
    }
    out
}

/// Verify the context fingerprint and that every site currently holds either
/// its retail word or a word this patch writes. Returns the configured value.
fn recognize(overlay: &[u8]) -> Result<i16> {
    for (va, want) in CONTEXT {
        let got = word_at(overlay, va)?;
        if got != want {
            bail!(
                "context word {va:#x} = {got:#010x}, expected {want:#010x} \
                 (battle-action Spirit accrual) - unrecognized build, refusing to patch"
            );
        }
    }

    // Sign comes from the accrual tail, magnitude from site A.
    let tail: Vec<u32> = TAIL_VAS
        .iter()
        .map(|va| word_at(overlay, *va))
        .collect::<Result<_>>()?;
    let draining = if tail == TAIL_RETAIL {
        false
    } else if tail == TAIL_SIGNED {
        true
    } else {
        bail!(
            "spirit-AP accrual tail at {:#x} is neither the stock accrual nor a \
             previously configured drain - unrecognized build, refusing to patch",
            TAIL_VAS[0]
        );
    };

    let mut imms = [0u16; 4];
    for (i, va) in [GRANT_VA, WIDGET_BASE_VA, WIDGET_BOOST2_VA, WIDGET_BOOST1_VA]
        .iter()
        .enumerate()
    {
        let got = word_at(overlay, *va)?;
        let prefix = if i == 0 { GRANT_PREFIX } else { WIDGET_PREFIX };
        let imm = (got & 0xFFFF) as u16;
        let plausible = if draining {
            (imm as i16) >= -MAX_SPIRIT_AP && (imm as i16) <= 0
        } else {
            got & 0xFFFF <= MAX_SITE_IMM
        };
        if got & 0xFFFF_0000 != prefix || !plausible {
            bail!(
                "site word {va:#x} = {got:#010x} is neither the stock Spirit accrual \
                 nor a previously configured value - unrecognized build, refusing to patch"
            );
        }
        imms[i] = imm;
    }
    Ok(imms[0] as i16)
}

/// The Spirit accrual currently on the image (the site-A immediate), after
/// verifying the build is recognized. Retail reads 32.
pub fn current(overlay: &[u8]) -> Result<i16> {
    recognize(overlay)
}

/// A planned Spirit AP edit: same-size word rewrites in the battle-action
/// overlay PROT entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpiritAp {
    /// Per site: `(file offset within the overlay entry, replacement word)`.
    pub writes: Vec<(usize, u32)>,
    /// The accrual the image held before this plan (site A immediate).
    pub previous: i16,
}

/// Plan setting the Spirit accrual to `ap` (`-100..=100`, retail 32) against
/// the raw battle-action overlay entry. Returns `Ok(None)` when the image
/// already holds `ap` at every site (idempotent no-op). Refuses an
/// unrecognized build rather than corrupting it.
pub fn plan(overlay: &[u8], ap: i16) -> Result<Option<SpiritAp>> {
    if !(-MAX_SPIRIT_AP..=MAX_SPIRIT_AP).contains(&ap) {
        bail!(
            "spirit AP must be {}..={MAX_SPIRIT_AP}, got {ap}",
            -MAX_SPIRIT_AP
        );
    }
    let previous = recognize(overlay)?;
    let mut writes = Vec::new();
    for (va, word) in desired(ap) {
        if word_at(overlay, va)? == word {
            continue;
        }
        writes.push(((va - OVERLAY_BASE_VA) as usize, word));
    }
    if writes.is_empty() {
        return Ok(None);
    }
    Ok(Some(SpiritAp { writes, previous }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(ov: &mut [u8], va: u32, w: u32) {
        let off = (va - OVERLAY_BASE_VA) as usize;
        ov[off..off + 4].copy_from_slice(&w.to_le_bytes());
    }

    /// Build a synthetic overlay holding the stock context + site words.
    fn synth_overlay() -> Vec<u8> {
        let mut ov = vec![0u8; 0x18000];
        for (va, w) in CONTEXT {
            put(&mut ov, va, w);
        }
        for (va, w) in desired(RETAIL_SPIRIT_AP) {
            put(&mut ov, va, w);
        }
        ov
    }

    fn apply(ov: &mut [u8], ap: i16) {
        if let Some(p) = plan(ov, ap).unwrap() {
            for (off, w) in &p.writes {
                ov[*off..*off + 4].copy_from_slice(&w.to_le_bytes());
            }
        }
    }

    #[test]
    fn retail_immediates_match_the_slider_math() {
        // The stock widget immediates are exactly what `site_immediates(32)`
        // computes: 0x28 = 32 + 32/4, 0x23 = 32 + 32/10.
        assert_eq!(site_immediates(RETAIL_SPIRIT_AP), [0x20, 0x20, 0x28, 0x23]);
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
        assert_eq!(
            p.writes.len(),
            4,
            "a positive value touches only the immediates"
        );
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
        apply(&mut ov, 50);
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
        // 50 -> 50 + 12 (50/4) and 50 + 5 (50/10).
        assert_eq!(site_immediates(50), [50, 50, 62, 55]);
    }

    #[test]
    fn negative_rewrites_the_tail_widget_and_boost_guards() {
        let mut ov = synth_overlay();
        apply(&mut ov, -25);
        assert_eq!(current(&ov).unwrap(), -25);
        // Site A holds the two's-complement immediate; `sb` stores 0xE7.
        let grant = word_at(&ov, GRANT_VA).unwrap();
        assert_eq!(grant & 0xFFFF, 0xFFE7);
        assert_eq!((grant & 0xFF) as u8, 0xE7);
        // All three widget targets use the same -N (no boost applies).
        for va in [WIDGET_BASE_VA, WIDGET_BOOST2_VA, WIDGET_BOOST1_VA] {
            assert_eq!(word_at(&ov, va).unwrap(), WIDGET_PREFIX | 0xFFE7);
        }
        assert_eq!(word_at(&ov, TAIL_VAS[0]).unwrap(), TAIL_SIGNED[0]);
        assert_eq!(word_at(&ov, BOOST2_BR_VA).unwrap(), BOOST2_BR_SKIP);
        assert_eq!(word_at(&ov, BOOST1_BR_VA).unwrap(), BOOST1_BR_SKIP);
        assert_eq!(word_at(&ov, SCRATCH_VAS[0]).unwrap(), SCRATCH_DRAIN[0]);
        assert_eq!(
            word_at(&ov, WIDGET_CLAMP_VAS[0]).unwrap(),
            WIDGET_CLAMP_FLOOR[0]
        );
    }

    #[test]
    fn round_trips_through_negative_back_to_retail() {
        let stock = synth_overlay();
        let mut ov = stock.clone();
        apply(&mut ov, -100);
        assert_eq!(current(&ov).unwrap(), -100);
        apply(&mut ov, 77);
        assert_eq!(current(&ov).unwrap(), 77);
        apply(&mut ov, RETAIL_SPIRIT_AP);
        assert_eq!(current(&ov).unwrap(), RETAIL_SPIRIT_AP);
        assert_eq!(ov, stock, "restoring retail restores every stock word");
        assert_eq!(plan(&ov, RETAIL_SPIRIT_AP).unwrap(), None);
    }

    #[test]
    fn refuses_out_of_range_and_unrecognized_builds() {
        let ov = synth_overlay();
        assert!(plan(&ov, 101).is_err());
        assert!(plan(&ov, -101).is_err());
        // Perturbed context word.
        let mut bad = ov.clone();
        put(&mut bad, 0x801E_5D88, 0xDEAD_BEEF);
        assert!(plan(&bad, 10).is_err());
        // Perturbed site opcode.
        let mut bad2 = ov.clone();
        put(&mut bad2, GRANT_VA, 0x3C02_0020); // lui, no longer addiu v0,zero
        assert!(plan(&bad2, 10).is_err());
        // Perturbed tail.
        let mut bad3 = ov.clone();
        put(&mut bad3, TAIL_VAS[4], 0x1234_5678);
        assert!(plan(&bad3, 10).is_err());
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

    /// The drain tail is hand-assembled; check every branch/jump it adds
    /// resolves to the instruction the comments claim, without a disc.
    #[test]
    fn hand_assembled_control_flow_resolves() {
        // bgez v1,+2 at 0x801E5E78 -> 0x801E5E84 (skips the floor).
        let off = (TAIL_SIGNED[1] & 0xFFFF) as i16 as i32;
        assert_eq!((TAIL_VAS[1] as i32 + 4 + off * 4) as u32, TAIL_VAS[4]);
        // beq v0,zero,-18 at 0x801E5E84 -> the scratch at 0x801E5E40.
        let off = (TAIL_SIGNED[4] & 0xFFFF) as i16 as i32;
        assert_eq!((TAIL_VAS[4] as i32 + 4 + off * 4) as u32, SCRATCH_VAS[0]);
        // j at 0x801E5E48 -> 0x801E5E90, the tail's join point.
        let target = (SCRATCH_VAS[2] & 0xF000_0000) | ((SCRATCH_DRAIN[2] & 0x03FF_FFFF) << 2);
        assert_eq!(target, 0x801E_5E90);
        // bgez v0,+2 at 0x801E5384 -> 0x801E5390 (skips the widget floor store).
        let off = (WIDGET_CLAMP_FLOOR[0] & 0xFFFF) as i16 as i32;
        assert_eq!(
            (WIDGET_CLAMP_VAS[0] as i32 + 4 + off * 4) as u32,
            0x801E_5390
        );
        // The neutralized boost guards keep retail's displacement.
        assert_eq!(BOOST2_BR_SKIP & 0xFFFF, BOOST2_BR_RETAIL & 0xFFFF);
        assert_eq!(BOOST1_BR_SKIP & 0xFFFF, BOOST1_BR_RETAIL & 0xFFFF);
        assert_eq!(BOOST1_BR_SKIP >> 16, 0x1000);
        // ...and boost-1's guard lands past the scratch, on the tail head.
        let off = (BOOST1_BR_RETAIL & 0xFFFF) as i16 as i32;
        assert_eq!((BOOST1_BR_VA as i32 + 4 + off * 4) as u32, TAIL_VAS[0]);
    }
}
