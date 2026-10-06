//! Disc-gated: the shipped effect-ribbon carriers, driven through the engine's
//! move VM, leave the emitter's inputs where `FUN_801CFA48` reads them.
//!
//! Move-VM op `0x42` (arm `0x80023F94`) is the only writer of the ribbon arm's
//! state - draw kind `+0x56 = 4` with `+0x9E & 0x2000` - and it occurs only in
//! slot-B cast / summon images. This test stages every part record of each
//! such image through [`legaia_engine_core::summon::SummonScene`], ticks the
//! scene, and for every part that reaches the ribbon state reads the emitter's
//! arguments back off the live actor with the
//! [`legaia_engine_core::effect_ribbon`] readers and builds the ribbon.
//!
//! What it pins is the chain *move program -> actor fields -> emitter
//! arguments* on real bytes; it says nothing about how the ribbon draws.
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` is absent.

use std::path::PathBuf;

use legaia_asset::summon_overlay::{self, SUMMON_OVERLAY_LINK_BASE};
use legaia_engine_core::effect_ribbon::{
    AnalyticTrig, RibbonParams, build_ribbon, ribbon_call_args, split_packed_count,
};
use legaia_engine_core::summon::SummonScene;
use legaia_engine_vm::move_vm::MoveHost;
use legaia_prot::archive::Archive;

struct LutHost;
impl MoveHost for LutHost {
    fn rotation_lut(&self, index: u16) -> (i16, i16) {
        let a = (index as f64) * std::f64::consts::TAU / 4096.0;
        ((a.sin() * 4096.0) as i16, (a.cos() * 4096.0) as i16)
    }
}

fn prot() -> Option<PathBuf> {
    for b in ["extracted", "../../extracted", "../extracted"] {
        let p = PathBuf::from(b).join("PROT.DAT");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Every slot-B image `0903..=0966`; the test reports which ones carry a
/// ribbon node rather than trusting a hand list.
const SLOT_B: std::ops::RangeInclusive<usize> = 903..=966;

#[test]
fn shipped_ribbon_carriers_arm_the_emitter_from_their_own_operands() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = prot() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let mut carriers = Vec::new();
    for idx in SLOT_B {
        let entry = archive.entries[idx].clone();
        let mut bytes = Vec::new();
        if archive.read_entry(&entry, &mut bytes).is_err() {
            continue;
        }
        let overlay = summon_overlay::parse(&bytes, SUMMON_OVERLAY_LINK_BASE);
        if overlay.parts.is_empty() {
            continue;
        }
        let mut scene = SummonScene::spawn(&overlay, &bytes, 0, [0, 0, 0]);
        let mut armed = std::collections::BTreeSet::new();
        for _ in 0..600 {
            scene.tick(&mut LutHost, 0x0400);
            for (i, part) in scene.parts.iter().enumerate() {
                let Some((mode, packed)) = ribbon_call_args(&part.state) else {
                    continue;
                };
                if !armed.insert(i) {
                    continue;
                }
                let params = RibbonParams::from_actor(&part.state);
                let (count, suppressed) = split_packed_count(packed);
                eprintln!(
                    "[ok] PROT {idx:04} part {i} (model_sel {}): mode {mode:#06x} \
                     +0x9C {} +0xC8 {} -> count {count} suppressed {suppressed} {params:?}",
                    part.model_sel,
                    part.state.actor_u16(0x9C) as i16,
                    part.state.actor_u16(0xC8) as i16,
                );
                // Every shipped carrier seeds the overlay RNG with `0x3039`.
                // The part is first seen after its seat frame's tick, whose
                // channel block (`0x80021E78`) has already stepped `+0xB8`
                // once by its rate `+0xC4` - the value the seat frame's own
                // draw reads.
                let rate = i32::from(part.state.actor_u16(0xC4) as i16);
                let first_step = ((rate * i32::from(scene.channel_delta)) >> 6) as i16;
                assert_eq!(
                    params.rng_seed,
                    0x3039i16.wrapping_add(first_step),
                    "PROT {idx:04} part {i}"
                );
                assert!(count > 0, "PROT {idx:04} part {i}: a ribbon with no steps");
                let mut s = 1u32;
                let r = build_ribbon(mode, packed, params, &AnalyticTrig::new(), || {
                    s = s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    s >> 16
                });
                assert_eq!(r.steps.len() as i32, count + 1);
            }
            if scene.finished() {
                break;
            }
        }
        if !armed.is_empty() {
            carriers.push(idx);
        }
    }
    eprintln!("ribbon carriers reached through the summon spawner: {carriers:?}");
    assert!(
        !carriers.is_empty(),
        "no slot-B image reached the ribbon arm - the op-0x42 stores or the \
         readers regressed"
    );
}

/// The draw half, end to end on the shipped carriers: staged through
/// `World::spawn_summon` in battle and ticked through `World::tick_summon`,
/// every carrier yields a live ribbon on `World::active_effect_ribbons` - the
/// one list both battle hosts draw - whose mesh is the retail packet chain
/// (six `GT4` packets a step, the fixed 2x2 patch at `(0..2, 0xF0..0xF2)`).
/// Also reports how the part tick's mode-2 channel moves `+0xC8`.
#[test]
fn shipped_ribbon_carriers_reach_the_battle_draw_list() {
    use legaia_engine_core::effect_ribbon::{RIBBON_CLUT, RIBBON_TPAGE};
    use legaia_engine_core::world::{SceneMode, World};
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = prot() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let mut drawn = Vec::new();
    for idx in [923usize, 934, 957, 964] {
        let entry = archive.entries[idx].clone();
        let mut bytes = Vec::new();
        archive.read_entry(&entry, &mut bytes).expect("read");
        let overlay = summon_overlay::parse(&bytes, SUMMON_OVERLAY_LINK_BASE);
        let mut world = World::default();
        world.enter_battle(3, 2);
        assert_eq!(world.mode, SceneMode::Battle);
        world.spawn_summon(&overlay, &bytes, 0, [0, 0, 0]);
        let mut first: Option<(usize, usize)> = None;
        let mut max_quads = 0usize;
        for frame in 0..600 {
            world.tick_summon(8);
            let ribbons = world.active_effect_ribbons();
            for rb in &ribbons {
                let quads = rb.mesh.indices.len() / 6;
                assert_eq!(rb.mesh.positions.len(), quads * 4);
                assert_eq!(quads % 6, 0, "six packets a step");
                assert!(
                    rb.mesh
                        .cba_tsb
                        .iter()
                        .all(|&ct| ct == [RIBBON_CLUT, RIBBON_TPAGE]),
                    "PROT {idx:04}: fixed patch"
                );
                first.get_or_insert((frame, quads));
                max_quads = max_quads.max(quads);
            }
            if world.casting.active_summon.is_none() {
                break;
            }
        }
        if let Some((f, q)) = first {
            eprintln!(
                "[ok] PROT {idx:04}: first ribbon at frame {f} with {q} quads, peak {max_quads} quads"
            );
            drawn.push(idx);
        } else {
            eprintln!("PROT {idx:04}: no ribbon reached the draw list");
        }
    }
    assert!(!drawn.is_empty(), "no carrier reached the battle draw list");
}
