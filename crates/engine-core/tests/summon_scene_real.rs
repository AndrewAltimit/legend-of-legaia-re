//! Disc-gated: the Gimard *Tail Fire* summon (PROT 0905) spawns and ticks
//! through the ported move VM.
//!
//! Pins, on real disc bytes, that the summon scene-graph driver
//! ([`legaia_engine_core::summon`]) seeds one move-VM actor per parsed part and
//! advances every part each frame through `legaia_engine_vm::move_vm` without
//! hitting an unimplemented opcode — the faithful per-part animation
//! computation. Skips when `LEGAIA_DISC_BIN` / `extracted/` is absent.

use std::path::PathBuf;

use legaia_asset::summon_overlay::{self, SUMMON_OVERLAY_LINK_BASE};
use legaia_engine_core::summon::SummonScene;
use legaia_engine_core::world::World;
use legaia_engine_vm::move_vm::MoveHost;
use legaia_prot::archive::Archive;

/// Minimal host with a real sin/cos LUT so rotation/tween ops produce nonzero
/// deltas (the engine's World host has the same LUT; this keeps the test
/// self-contained).
struct LutHost;
impl MoveHost for LutHost {
    fn rotation_lut(&self, index: u16) -> (i16, i16) {
        let a = (index as f64) * std::f64::consts::TAU / 4096.0;
        ((a.sin() * 4096.0) as i16, (a.cos() * 4096.0) as i16)
    }
}

const PROT_GIMARD_SUMMON_STAGER: usize = 905;
/// Engine pool base for `model_sel`-indexed meshes (Gimard fire mesh-set).
const GIMARD_TAIL_FIRE_MODEL_INDEX: usize = 26;

fn prot() -> Option<PathBuf> {
    for b in ["extracted", "../../extracted", "../extracted"] {
        let p = PathBuf::from(b).join("PROT.DAT");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

#[test]
fn gimard_summon_spawns_and_ticks_through_the_move_vm() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = prot() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive.entries[PROT_GIMARD_SUMMON_STAGER].clone();
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 0905");

    let overlay = summon_overlay::parse(&bytes, SUMMON_OVERLAY_LINK_BASE);
    let mut scene = SummonScene::spawn(
        &overlay,
        &bytes,
        GIMARD_TAIL_FIRE_MODEL_INDEX,
        [0, -300, -645], // a plausible cast-target origin
    );
    assert_eq!(
        scene.parts.len(),
        overlay.parts.len(),
        "one runtime state per parsed part"
    );
    assert!(
        scene.mesh_part_count() >= 1,
        "the summon has at least one mesh-bearing part"
    );

    // Tick a couple seconds of frames. The move VM must run every live part
    // each frame without panicking; some parts halt, some hold poses on their
    // wait-timers. We also confirm the scene makes progress (parts finish over
    // time) and that mesh parts produce render draws in the model-pool range.
    let mut host = LutHost;
    // Fingerprint the move-VM-driven fields that an advancing program touches.
    let fp = |s: &legaia_engine_vm::move_vm::ActorState| {
        (
            s.pc,
            s.world_x,
            s.world_y,
            s.world_z,
            s.y_rot,
            s.render_24,
            s.render_26,
            s.render_28,
            s.tween_src_x,
            s.tween_scale_x,
            s.wait_timer,
            s.flags,
        )
    };
    let snapshot0: Vec<_> = scene.parts.iter().map(|p| fp(&p.state)).collect();
    for _ in 0..180 {
        scene.tick(&mut host, 0x0200);
    }
    let any_state_changed = scene
        .parts
        .iter()
        .zip(&snapshot0)
        .any(|(p, s0)| fp(&p.state) != *s0);
    assert!(
        any_state_changed,
        "ticking must advance at least one part's move-VM state"
    );

    let draws = scene.part_draws();
    assert_eq!(
        draws.len(),
        scene.mesh_part_count(),
        "one draw per mesh part"
    );
    for d in &draws {
        assert!(
            (GIMARD_TAIL_FIRE_MODEL_INDEX..GIMARD_TAIL_FIRE_MODEL_INDEX + 64)
                .contains(&d.model_index),
            "model index {} should sit in the summon's mesh-set band",
            d.model_index
        );
    }
    eprintln!(
        "Gimard summon: {} parts ({} mesh), {} draws after {} frames; finished={}",
        scene.parts.len(),
        scene.mesh_part_count(),
        draws.len(),
        scene.frame,
        scene.finished()
    );
}

#[test]
fn world_spawns_and_ticks_the_gimard_summon() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = prot() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive.entries[PROT_GIMARD_SUMMON_STAGER].clone();
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 0905");
    let overlay = summon_overlay::parse(&bytes, SUMMON_OVERLAY_LINK_BASE);

    // Drive the whole spawn -> tick -> draw path through World (exercises the
    // borrow-split tick that runs the move VM with the World's host).
    let mut world = World::new();
    assert!(world.active_summon.is_none());
    world.spawn_summon(&overlay, &bytes, GIMARD_TAIL_FIRE_MODEL_INDEX, [0, 0, 0]);
    assert!(world.active_summon.is_some(), "summon spawned");
    assert!(
        !world.active_summon_part_draws().is_empty(),
        "mesh parts produce draws"
    );

    // Tick through the World host; the scene either keeps animating or drains
    // once every part finishes. Either way the call must not panic and the
    // draws stay in the model-pool band while it's alive.
    for _ in 0..600 {
        world.tick_summon(0x0400);
        for d in world.active_summon_part_draws() {
            assert!(d.model_index >= GIMARD_TAIL_FIRE_MODEL_INDEX);
        }
        if world.active_summon.is_none() {
            break;
        }
    }
    eprintln!(
        "World summon tick: active_after_600={}",
        world.active_summon.is_some()
    );
}
