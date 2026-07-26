//! Disc-gated: player Seru-magic summon stagers spawn and tick through the
//! ported move VM.
//!
//! Pins, on real disc bytes, that the summon scene-graph driver
//! ([`legaia_engine_core::summon`]) seeds one move-VM actor per parsed part and
//! advances every part each frame through `legaia_engine_vm::move_vm` without
//! hitting an unimplemented opcode - the faithful per-part animation
//! computation. Skips when `LEGAIA_DISC_BIN` / `extracted/` is absent.
//!
//! ## Two legs, because a stager's own footprint is smaller than it looks
//!
//! A stager entry is exactly its TOC gap (`toc[p+3] - toc[p+2]` sectors,
//! retail's own `FUN_8003E68C`), and every stager shares one link base, so a
//! record pointer is only meaningful against *its own* entry's bytes. Read
//! through the superseded over-reading entry size, a stager's buffer ran on
//! into the next two stagers and neighbour call sites resolved pointers that
//! landed on this entry's bytes at the neighbour's offsets - record-shaped
//! garbage that reads as mesh parts. Both legs below therefore assert the read
//! length **is** the TOC gap before parsing.
//!
//! - **Gimard** (`0x81` → PROT 0903) is the driver leg: within its own
//!   footprint it is a pure transform rig, every part `model_sel == -1`.
//!   That is the corpus fact `docs/reference/re-settled-threads.md` records
//!   for this family, and it is why the draw path needs a second leg.
//! - **Nighto** (`0x85` → PROT 0907) is the draw leg: one genuine
//!   mesh-bearing part inside its own footprint, so `part_draws` and the
//!   model-pool band assertion run on real geometry instead of an empty list.

use std::path::{Path, PathBuf};

use legaia_asset::summon_overlay::{self, SUMMON_OVERLAY_LINK_BASE};
use legaia_engine_core::summon::{SummonScene, summon_stager_prot_entry};
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

/// Gimard, the first player Seru-magic summon. `903 + (id - 0x81)` puts its
/// stager at extraction PROT **0903**; the historical "Gimard = 0905" label
/// was an off-by-2 in that index math (0905 is the `0x83` slot).
const GIMARD_SPELL_ID: u8 = 0x81;

/// Nighto (`0x85` → PROT 0907, head title "Hell's Music"). The one player
/// stager whose own TOC-gap footprint carries a mesh-bearing part record.
const NIGHTO_SPELL_ID: u8 = 0x85;

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

/// Read one stager's bytes and assert they are exactly the entry's TOC gap -
/// the guard that keeps a future over-read from silently resurrecting the
/// neighbour-owned "mesh" records this test used to lean on.
fn stager_bytes(prot: &Path, spell_id: u8) -> (u32, Vec<u8>) {
    let idx = summon_stager_prot_entry(spell_id)
        .unwrap_or_else(|| panic!("spell 0x{spell_id:02X} has a stager entry"));
    let mut archive = Archive::open(prot).expect("open PROT.DAT");
    let entry = archive.entries[idx as usize].clone();
    let next = archive.entries[idx as usize + 1].clone();
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .unwrap_or_else(|e| panic!("read PROT {idx:04}: {e:#}"));
    assert_eq!(
        bytes.len(),
        summon_overlay::unique_content_len(bytes.len(), entry.start_lba, next.start_lba),
        "PROT {idx:04} must be read at its TOC-gap footprint - a longer buffer \
         resolves the NEXT stagers' record pointers against this entry's bytes",
    );
    (idx, bytes)
}

#[test]
fn summon_stagers_spawn_and_tick_through_the_move_vm() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = prot() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };

    // (spell id, label, expected mesh-bearing part count within the entry's
    // own TOC-gap footprint). Gimard's rig is all transform nodes; Nighto
    // carries exactly one mesh record, which is what keeps the draw
    // assertions below non-vacuous.
    let legs: [(u8, &str, usize); 2] = [
        (GIMARD_SPELL_ID, "Gimard", 0),
        (NIGHTO_SPELL_ID, "Nighto", 1),
    ];

    // Fingerprint the move-VM-driven fields that an advancing program touches.
    let fingerprint = |s: &legaia_engine_vm::move_vm::ActorState| {
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

    for (spell_id, label, expect_mesh) in legs {
        let (idx, bytes) = stager_bytes(&prot, spell_id);
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
            "{label} (PROT {idx:04}): one runtime state per parsed part",
        );
        assert!(
            !scene.parts.is_empty(),
            "{label} (PROT {idx:04}): the stager must recover part records",
        );
        assert_eq!(
            scene.mesh_part_count(),
            expect_mesh,
            "{label} (PROT {idx:04}): mesh-bearing part count inside the entry's \
             own footprint (transform nodes carry no library mesh)",
        );

        // Tick a couple seconds of frames. The move VM must run every live part
        // each frame without panicking; some parts halt, some hold poses on
        // their wait-timers. Confirm the scene makes progress, and that any
        // mesh part produces a draw in the model-pool range.
        let mut host = LutHost;
        let snapshot0: Vec<_> = scene.parts.iter().map(|p| fingerprint(&p.state)).collect();
        for _ in 0..180 {
            scene.tick(&mut host, 0x0200);
        }
        let any_state_changed = scene
            .parts
            .iter()
            .zip(&snapshot0)
            .any(|(p, s0)| fingerprint(&p.state) != *s0);
        assert!(
            any_state_changed,
            "{label} (PROT {idx:04}): ticking must advance at least one part's \
             move-VM state",
        );

        let draws = scene.part_draws();
        assert_eq!(
            draws.len(),
            scene.mesh_part_count(),
            "{label} (PROT {idx:04}): one draw per mesh part",
        );
        for d in &draws {
            assert!(
                (GIMARD_TAIL_FIRE_MODEL_INDEX..GIMARD_TAIL_FIRE_MODEL_INDEX + 64)
                    .contains(&d.model_index),
                "{label} (PROT {idx:04}): model index {} should sit in the \
                 summon's mesh-set band",
                d.model_index,
            );
        }
        eprintln!(
            "{label} summon (PROT {idx:04}): {} parts ({} mesh), {} draws after \
             {} frames; finished={}",
            scene.parts.len(),
            scene.mesh_part_count(),
            draws.len(),
            scene.frame,
            scene.finished(),
        );
    }
}

#[test]
fn world_spawns_and_ticks_a_summon() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = prot() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };

    // Nighto, so the draw list is genuinely non-empty (Gimard's own footprint
    // is a pure transform rig - see the module docs).
    let (idx, bytes) = stager_bytes(&prot, NIGHTO_SPELL_ID);
    let overlay = summon_overlay::parse(&bytes, SUMMON_OVERLAY_LINK_BASE);

    // Drive the whole spawn -> tick -> draw path through World (exercises the
    // borrow-split tick that runs the move VM with the World's host).
    let mut world = World::new();
    assert!(world.active_summon.is_none());
    world.spawn_summon(&overlay, &bytes, GIMARD_TAIL_FIRE_MODEL_INDEX, [0, 0, 0]);
    assert!(world.active_summon.is_some(), "summon spawned");
    assert!(
        !world.active_summon_part_draws().is_empty(),
        "PROT {idx:04} has a mesh-bearing part, so it must produce draws",
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
        "World summon tick (PROT {idx:04}): active_after_600={}",
        world.active_summon.is_some()
    );
}
