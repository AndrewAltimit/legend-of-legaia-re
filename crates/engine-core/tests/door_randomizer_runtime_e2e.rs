//! Disc-gated end-to-end oracle for the **door (scene-transition) randomizer at
//! runtime** - the fourth-kind sibling of the chest / drop / encounter / steal
//! oracles.
//!
//! The randomizer's own disc-gated tests (`crates/patcher/tests/door_patch_real`)
//! prove the patch is *written* faithfully (the `0x3F` op's inline destination
//! name is resized + relocated, the MAN re-parses, sectors stay EDC/ECC-valid).
//! What they don't prove is that a runtime actually *reads the patched
//! destination and warps there*.
//!
//! A mednafen savestate is a trap here for the same reason as the other oracles:
//! a scene's MAN is resident in RAM the moment you're standing in it, so loading
//! a patched disc on such a state still warps to the *original* destination
//! (read from the already-loaded RAM copy). A patched door is only observed
//! after a fresh scene load re-streams the MAN - which is exactly what the
//! clean-room engine does. The mechanism was pinned by a live PCSX-Redux trace
//! (the `drake_castle_to_worldmap` capture): a door destination is a partition-2
//! MAN record the controller reaches by setting the field-VM bytecode base to
//! `man_base + data_region + partition2[slot]`, then running the record's tiny
//! script, whose `0x3F` op warps by the inline name.
//!
//! So this test:
//!   1. patches Rim Elm's (`town01`, PROT 4) single exit - op `0x6f95`,
//!      originally `-> map01` - to a **differently-named** scene (`keikoku`,
//!      exercising the variable-length resize), on a scratch copy of the disc,
//!   2. re-decodes the patched scene MAN off the patched image (the bytes a
//!      fresh scene load would stream),
//!   3. drives the patched `0x3F` op through the real field VM
//!      (`World::load_field_script` + `tick`, the same op handler the engine
//!      runs on a warp),
//!   4. asserts the runtime stages a transition to the **patched** scene, never
//!      the original.
//!
//! A baseline pass over the *unpatched* exit first confirms it warps to `map01`,
//! so the patched assertion can't pass vacuously. Skips without `LEGAIA_DISC_BIN`.

use legaia_asset::man_edit::DestEdit;
use legaia_asset::scene_asset_table::encode_size_word;
use legaia_engine_core::world::{SceneMode, World};
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::door::SceneDoors;

/// Rim Elm scene bundle PROT entry.
const TOWN01_ENTRY: usize = 4;
/// town01's single exit op offset (pinned by the door census + the slot-9
/// Rim Elm -> overworld capture).
const EXIT_OP_PC: usize = 0x6f95;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Drive the `0x3F` op at `op_pc` in a decompressed MAN through the field VM and
/// return the destination scene name it stages, if any. The script is the MAN
/// from the op onward, so `tick()` executes the `0x3F` as its first op.
fn warp_destination(decoded: &[u8], op_pc: usize) -> Option<String> {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.load_field_script(decoded[op_pc..].to_vec());
    let _ = world.tick();
    world
        .pending_named_scene_transition
        .as_ref()
        .map(|(name, _, _, _)| name.clone())
}

#[test]
fn runtime_warps_to_the_patched_door_destination() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    // --- baseline: the unpatched exit warps to map01 ---
    let base = DiscPatcher::open(original.clone()).expect("open");
    let entry = base.read_entry(TOWN01_ENTRY).expect("read town01");
    let doors = SceneDoors::locate(&entry, TOWN01_ENTRY).expect("town01 has doors");
    let site = doors
        .sites
        .iter()
        .find(|s| s.op_pc == EXIT_OP_PC)
        .expect("town01 exit op present");
    assert_eq!(site.name, "map01", "baseline exit -> map01");
    assert_eq!(
        warp_destination(&doors.decoded, EXIT_OP_PC).as_deref(),
        Some("map01"),
        "the unpatched exit must warp to map01 (non-vacuous baseline)"
    );

    // --- patch: retarget the exit to a differently-named scene ---
    const REPLACEMENT: &str = "keikoku"; // 7 chars vs "map01" 5 -> grows the op
    let edit = DestEdit {
        op_pc: EXIT_OP_PC,
        index: 111, // keikoku's index (cosmetic for this op-handler test)
        name: REPLACEMENT.as_bytes().to_vec(),
        entry_x: site.entry_x,
        entry_z: site.entry_z,
        dir: site.dir,
    };
    let (stream, new_size) = doors.rebuild(&[edit]).expect("rebuild fits + validates");

    let mut patcher = DiscPatcher::open(original.clone()).expect("open scratch");
    patcher
        .patch_prot_entry(
            TOWN01_ENTRY,
            doors.man_descriptor_off as u64,
            &encode_size_word(0x03, new_size).to_le_bytes(),
        )
        .expect("patch MAN size word");
    patcher
        .patch_prot_entry(TOWN01_ENTRY, doors.man_offset as u64, &stream)
        .expect("patch MAN stream");
    let patched = patcher.into_image();

    // --- re-decode off the patched image + drive the field VM ---
    let p2 = DiscPatcher::open(patched).expect("re-open patched");
    let entry2 = p2.read_entry(TOWN01_ENTRY).expect("read patched town01");
    let doors2 = SceneDoors::locate(&entry2, TOWN01_ENTRY).expect("patched town01 doors");
    let site2 = doors2
        .sites
        .iter()
        .find(|s| s.name == REPLACEMENT)
        .expect("patched exit names the replacement scene");
    // op_pc is unchanged (the opcode byte stays put; the name after it grew).
    assert_eq!(site2.op_pc, EXIT_OP_PC);

    let dest = warp_destination(&doors2.decoded, site2.op_pc);
    assert_eq!(
        dest.as_deref(),
        Some(REPLACEMENT),
        "the runtime must warp to the PATCHED destination, never the original map01"
    );
    assert_ne!(dest.as_deref(), Some("map01"));
}
