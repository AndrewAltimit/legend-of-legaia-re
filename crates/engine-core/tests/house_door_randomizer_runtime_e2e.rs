//! Disc-gated end-to-end oracle for the **house-door (intra-town) randomizer
//! at runtime** - the sibling of the chest / door / encounter / steal oracles.
//!
//! The randomizer's own disc-gated tests prove the patch is *written*
//! faithfully (`crates/rando/tests/house_door_patch_real`: per-scene,
//! per-class target multisets preserved, sectors EDC/ECC-valid) and that the
//! classified population is the audited one
//! (`crates/rando/tests/house_door_classifier_real`). What they don't prove is
//! that a runtime actually *reads the patched operand bytes and warps the
//! player there*.
//!
//! A mednafen savestate is a trap here for the same reason as the other
//! oracles: a scene's MAN is resident in RAM the moment you're standing in the
//! town, so loading a patched disc on such a state still warps to the
//! *original* interior. A patched door is only observed after a fresh scene
//! load re-streams the MAN - which is exactly what the clean-room engine does.
//!
//! The mechanism was pinned by a live PCSX-Redux range write-watch
//! (`probe.step.find_writer`): entering Mei's house executes the field-VM
//! dispatcher's `case 0x23` with operands `(0x61, 0x36)` - tile `(97, 54)`,
//! world `(0x30C0, 0x1B40)` via `tile * 128 + 0x40`. On disc that op is the
//! cross-context player MOVE_TO `0xA3 0xF8 0x61 0x36` in town01's partition-0
//! record named "...IN" (see `docs/tooling/randomizer.md` § House doors).
//!
//! So this test:
//!   1. baselines the unpatched op: drives the door record's bytecode from the
//!      warp op through the real field VM (`World::load_field_script` + `tick`)
//!      and asserts a `MoveTo` event at exactly the captured world coords
//!      `(12480, 6976)`,
//!   2. patches the house doors on a scratch copy of the disc
//!      (`apply::randomize_house_doors`) and re-decodes the patched MAN off the
//!      patched image,
//!   3. drives the same op offset again and asserts the runtime now warps to
//!      the **patched** tile's world coords - and not to Mei's interior.
//!
//! town01 has two distinct IN-class targets, so the shuffle's anti-identity
//! guard guarantees the Mei's-house entry changes under any seed (non-vacuous).
//! Skips without `LEGAIA_DISC_BIN`.

use legaia_engine_core::field_events::FieldEvent;
use legaia_engine_core::world::{SceneMode, World};
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;
use legaia_rando::house_door::{DoorSide, SceneHouseDoors};

/// Rim Elm scene bundle PROT entry.
const TOWN01_ENTRY: usize = 4;
/// The captured Mei's-house interior tile (PCSX-Redux `find_writer`).
const MEI_TILE: (u8, u8) = (97, 54);

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Field-VM grid byte → world coordinate (`(b & 0x7F) * 0x80 + 0x40`, plus
/// `0x40` when the half-tile bit is set) - the retail `case 0x23` conversion.
fn grid_to_world(b: u8) -> u16 {
    let base = u16::from(b & 0x7F) * 0x80 + 0x40;
    if b & 0x80 != 0 { base + 0x40 } else { base }
}

/// Drive the bytecode at `op_pc` (a `0xA3 0xF8 xb zb` player warp) through the
/// real field VM and return the first `MoveTo` event's world coords.
fn warp_world_coords(decoded: &[u8], op_pc: usize) -> Option<(u16, u16)> {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.load_field_script(decoded[op_pc..].to_vec());
    let _ = world.tick();
    world.drain_field_events().into_iter().find_map(|ev| {
        if let FieldEvent::MoveTo {
            world_x, world_z, ..
        } = ev
        {
            Some((world_x, world_z))
        } else {
            None
        }
    })
}

#[test]
fn runtime_warps_to_the_patched_house_interior() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    // --- baseline: the unpatched Mei's-house entry warps to (97, 54) ---
    let base = DiscPatcher::open(original.clone()).expect("open");
    let entry = base.read_entry(TOWN01_ENTRY).expect("read town01");
    let doors = SceneHouseDoors::locate(&entry, TOWN01_ENTRY).expect("town01 has door warps");
    let mei_pos = doors
        .sites
        .iter()
        .zip(doors.current_targets())
        .position(|(s, (xb, zb))| s.side == DoorSide::In && (xb & 0x7F, zb & 0x7F) == MEI_TILE)
        .expect("town01 carries the Mei's-house IN warp");
    let mei_op_pc = doors.sites[mei_pos].op_pc;

    let baseline =
        warp_world_coords(&doors.decoded, mei_op_pc).expect("baseline script emits a MoveTo event");
    assert_eq!(
        baseline,
        (0x30C0, 0x1B40),
        "unpatched warp must land at the live-captured Mei's-house world coords"
    );

    // --- patch the house doors on a scratch copy of the disc ---
    let seed = 0x4D45_4953_4841_5546; // arbitrary fixed seed
    let mut patcher = DiscPatcher::open(original).expect("open scratch");
    let report =
        apply::randomize_house_doors(&mut patcher, seed, DropMode::Shuffle).expect("shuffle");
    assert!(
        report.sites_changed > 0,
        "the shuffle must change something"
    );

    // --- re-decode the patched MAN off the patched image ---
    let patched_entry = patcher.read_entry(TOWN01_ENTRY).expect("read patched");
    let patched_doors =
        SceneHouseDoors::locate(&patched_entry, TOWN01_ENTRY).expect("patched town01 doors");
    // Same-size operand edit: the op offset is unchanged.
    let (new_xb, new_zb) = patched_doors
        .sites
        .iter()
        .zip(patched_doors.current_targets())
        .find_map(|(s, t)| (s.op_pc == mei_op_pc).then_some(t))
        .expect("the Mei warp op survives at its offset");
    assert_ne!(
        (new_xb & 0x7F, new_zb & 0x7F),
        MEI_TILE,
        "town01 has two distinct IN targets, so the Mei entry must have moved"
    );

    // --- the runtime reads the patched bytes and warps there ---
    let warped = warp_world_coords(&patched_doors.decoded, mei_op_pc)
        .expect("patched script emits a MoveTo event");
    assert_eq!(
        warped,
        (grid_to_world(new_xb), grid_to_world(new_zb)),
        "runtime must warp to the patched interior tile"
    );
    assert_ne!(
        warped,
        (0x30C0, 0x1B40),
        "runtime must no longer warp to the original Mei's-house interior"
    );
}
