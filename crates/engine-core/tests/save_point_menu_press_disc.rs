//! Disc-gated: talking to a **save point** opens the pause menu on the save
//! screen, through the production interaction path, and the menu's close
//! resumes the record once.
//!
//! ## Retail
//!
//! A save point's partition-1 record is `31 02 21` (spawn section) followed by
//! `49 01 00 21` (interaction section). A PCSX-Redux capture at the `town01`
//! save point (`scripts/pcsx-redux/autorun_save_point_press.lua`, from the
//! `town01_field_card_boot` library state) logs, one hit each: the op-`0x49`
//! Idle arm's park store (`0x801E09A8`), the subsystem actor's enter half
//! `FUN_801F1278`, its state pick `FUN_801F1F4C` with `+0x50 = 7`, and the
//! pause-menu session `FUN_801ED308` with `+0x50 = 0x30`; the game mode turns
//! `23` with no Start press. After the player backs out, the dispatcher's
//! retire arm stores the Done sentinel (`0x801F16AC`) and the park clears and
//! stays clear - the interaction stops on its `0x21`.
//!
//! ## What this pins
//!
//! For every placement whose record carries a clean `49 01`: the interact arms
//! a run, the run parks on the op and reports a scripted menu press
//! ([`World::scripted_menu_open_pending`]) with the entry kind `1` that opens
//! the save screen; a release (the hosts' menu-close path) resumes the record;
//! and no second press follows.
//!
//! Structural assertions only. Skips (passes) without `LEGAIA_DISC_BIN` /
//! `extracted/`.

use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::{CLEAN_RESYNC_INSNS, partition_record_span};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::field_disasm::{InsnInfo, LinearWalker};

fn extracted_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from)
        && d.join("PROT.DAT").exists()
    {
        return Some(d);
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// `(scene, MAN, partition-1 slot)` for every record with a clean `49 01`.
fn save_point_slots(index: &ProtIndex) -> Vec<(String, Vec<u8>, u8)> {
    let mut out = Vec::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(index, &name) else {
            continue;
        };
        let Ok(Some(man)) = scene.field_man_payload(index) else {
            continue;
        };
        let Ok(mf) = legaia_asset::man_section::parse(&man) else {
            continue;
        };
        let count = (*mf.header.partition_counts.get(1).unwrap_or(&0)).max(0) as usize;
        for record in 0..count {
            let Some((start, pc0, len)) = partition_record_span(&mf, &man, 1, record) else {
                continue;
            };
            let body = &man[start..start + len];
            let mut ok_run = CLEAN_RESYNC_INSNS;
            let mut hit = false;
            for insn in LinearWalker::new(body, pc0) {
                let Ok(insn) = insn else {
                    ok_run = 0;
                    continue;
                };
                let clean = ok_run >= CLEAN_RESYNC_INSNS;
                ok_run += 1;
                if clean && matches!(insn.info, InsnInfo::StateResume { sub_op: 1, .. }) {
                    hit = true;
                }
            }
            if hit && let Ok(slot) = u8::try_from(record) {
                out.push((name.clone(), man.clone(), slot));
            }
        }
    }
    out
}

#[test]
fn talking_to_a_save_point_presses_the_menu_and_resumes_once() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let slots = save_point_slots(&index);
    assert!(
        slots.len() >= 40,
        "expected the disc's save points, found {}",
        slots.len()
    );

    let mut pressed = 0usize;
    let mut not_installed = Vec::new();
    for (scene, man, slot) in &slots {
        let mf = legaia_asset::man_section::parse(man).expect("parse MAN");
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.install_field_carriers_from_man(&mf, man);
        world.install_field_player(0);
        world.toggles.use_vm_dialogue = true;
        if !world.npcs.dialog_prologue.contains_key(slot) {
            not_installed.push(format!("{scene} P1[{slot}]"));
            continue;
        }
        world.trigger_field_interact(0, *slot);
        let mut frames = 0;
        while frames < 120 && !world.scripted_menu_open_pending() {
            let _ = world.tick();
            frames += 1;
        }
        assert!(
            world.scripted_menu_open_pending(),
            "{scene} P1[{slot}]: the save point never pressed the menu"
        );
        assert_eq!(
            world.menu_entry_context_kind(),
            Some(1),
            "{scene} P1[{slot}]"
        );
        assert!(
            !world.field_vm.submode_screen.open,
            "{scene} P1[{slot}]: a submode screen opened for a -1 row"
        );
        pressed += 1;

        // The host opens the menu; the park holds while it is up.
        world.note_scripted_menu_opened();
        for _ in 0..60 {
            let _ = world.tick();
        }
        assert_eq!(
            world.menu_entry_context_kind(),
            Some(1),
            "{scene} P1[{slot}]"
        );

        // The close resumes the record once.
        assert!(world.release_menu_entry_context_park());
        for _ in 0..120 {
            let _ = world.tick();
            assert!(
                !world.scripted_menu_open_pending(),
                "{scene} P1[{slot}]: the resumed save point pressed the menu again"
            );
        }
        assert_eq!(world.menu_entry_context_kind(), None, "{scene} P1[{slot}]");
        assert!(
            !world.dialogue_owns_input(),
            "{scene} P1[{slot}]: the interaction did not end on its 0x21"
        );
    }
    eprintln!(
        "save points: {} records, {pressed} pressed the menu, not installed: {:?}",
        slots.len(),
        not_installed
    );
    assert!(
        not_installed.is_empty(),
        "save-point records with no interaction installed: {not_installed:?}"
    );
}
