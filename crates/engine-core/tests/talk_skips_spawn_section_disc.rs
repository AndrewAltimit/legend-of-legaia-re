//! A talk resumes a placement record at its **interaction cursor**, never at
//! `script_pc0`: the spawn section is load-time script and does not re-run.
//!
//! `kor5` `P1[2]` is the witness. Its spawn section is `0x25`, `SET 0x619`, a
//! `CamCfg`, then the raw `0x21` terminator; the interaction begins one byte
//! past it (retail's dialog SM resumes `actor[+0x9E]` where the spawn slice
//! stopped). Entering a talk at `script_pc0` re-latched `0x619` on every
//! conversation - a flag the chain's walk-on beat `P2[4]` clears and retail
//! re-sets only on a MAN-loading scene entry.
//!
//! The test clears the flag after entry (as `P2[4]` does), talks to the
//! record through the shared conversation path, and requires the flag to stay
//! clear; a baseline check pins that the scene entry itself does set it, so
//! the talk leg is not vacuous.
//!
//! Disc-gated: skip-passes when `LEGAIA_DISC_BIN` is unset or `extracted/`
//! is absent.

use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::SceneHost;

const SCENE: &str = "kor5";
const SLOT: u8 = 2;
const FLAG: u16 = 0x619;

fn extracted_dir() -> Option<PathBuf> {
    ["extracted", "../extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

#[test]
fn talking_to_kor5_p1_2_does_not_rerun_its_spawn_section() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.world.toggles.use_vm_dialogue = true;
    host.enter_field_scene(SCENE, 0).expect("enter kor5");
    assert!(
        host.world.system_flag_test(FLAG),
        "baseline: the scene entry's spawn slice sets 0x619"
    );

    let record = host
        .world
        .npcs
        .dialog_prologue
        .get(&SLOT)
        .cloned()
        .expect("kor5 P1[2] is a talkable placement");
    assert_eq!(
        record.body.get(record.entry_pc - 1),
        Some(&0x21),
        "the interaction cursor sits one past the spawn section's 0x21"
    );

    host.world.system_flag_clear(FLAG);
    host.world.trigger_field_interact(0, SLOT);
    let mut talked = false;
    let mut pressed_last = false;
    for _ in 0..2000 {
        let waiting = host.world.dialog.inline.as_ref().is_some_and(|d| {
            d.panel
                .as_ref()
                .is_some_and(|p| p.is_waiting_for_input() || p.is_done() || p.menu_active())
        });
        talked |= host.world.dialog.inline.is_some();
        let mask = if waiting && !pressed_last {
            PadButton::Cross.mask()
        } else {
            0
        };
        pressed_last = mask != 0;
        host.world.set_pad(mask);
        host.tick().expect("tick");
        if talked && host.world.dialog.inline.is_none() {
            break;
        }
    }
    assert!(talked, "the interaction opened a conversation");
    assert!(
        !host.world.system_flag_test(FLAG),
        "the talk re-ran P1[2]'s spawn section and re-set 0x619"
    );
    eprintln!(
        "[ok] kor5 P1[2]: talk ran from +{:#x}, 0x619 stayed clear",
        record.entry_pc
    );
}
