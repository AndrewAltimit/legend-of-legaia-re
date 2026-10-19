//! Disc-gated regression: **a casino minigame entered from its seat comes up
//! and comes back.**
//!
//! Every `koin1` cabinet record (P1[51..=53] Baka Fighter, P1[54..=56] slot
//! machine) closes its seat the same way:
//!
//! ```text
//! +0x65  34 01 FF FF FF 1E 00   op 0x34: subtractive (blend 2) white wash, held
//! +0x6C  4A 1E 00 / 4A 08 00    wait it out
//! +0x72  3E 67|68 ..            mode-24 door warp
//! +0x78  21                     end of the interaction pass
//! +0x79  34 01 FF FF FF 00 00   (next pass) walk the wash out
//! +0x80  34 01 00 00 00 1E 00   clear it, then stand the player up
//! ```
//!
//! and the system script's loop (`P1[0]` `+0x12D..`) re-engages the record
//! off the seat's own flag (`0x433..0x435`, `0x4B6..0x4B8`) with
//! `B1 <id> 08`, so the next pass runs from `+0x79` once the field is back.
//!
//! Two defects sat on that path, on both play hosts (the kernels are
//! `engine-core`'s):
//!
//! - **The wash pushed under the minigame.** The tween actor kept emitting
//!   its full-strength subtractive white over the Baka duel, so the 3D arena
//!   drew black and only the screen-space HUD (the round digit) showed. The
//!   `B` launcher armed no wash, so it worked.
//! - **The pass end fell back to the refusal line.** The talk runner read the
//!   `21` at `+0x78` as a prologue that never reached its box and jumped to
//!   the record's first text segment - the "You don't have any tokens"
//!   refusal - so the `+0x79` pass never ran, the wash was never cleared and
//!   the field came back black behind that box, whichever way the machine
//!   was left.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md convention).

use std::path::PathBuf;

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from)
        && d.join("PROT.DAT").exists()
    {
        return Some(d);
    }
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extracted");
    if d.join("PROT.DAT").exists() {
        Some(d)
    } else {
        eprintln!("[skip] extracted/ missing - run legaia-extract first");
        None
    }
}

/// `koin1` with a stocked coin bank and the field-VM talk runner on (the
/// play hosts' setting), the player engaged at cabinet record `slot`, ticked
/// until the seat's warp has entered `mode`.
fn seated_into(extracted: &PathBuf, slot: u8, mode: SceneMode) -> SceneHost {
    let mut host = SceneHost::open_extracted(extracted).expect("open SceneHost");
    host.enter_field_scene("koin1", 0).expect("enter koin1");
    for _ in 0..90 {
        let _ = host.tick();
    }
    host.world.minigames.casino_coins = 100;
    host.world.toggles.use_vm_dialogue = true;
    host.world.trigger_field_interact(0, slot);
    host.world.set_pad(0);
    for _ in 0..2000 {
        if host.world.mode == mode {
            break;
        }
        let _ = host.tick();
    }
    assert_eq!(
        host.world.mode, mode,
        "koin1 P1[{slot}]: the seat never reached its warp"
    );
    host
}

fn press(host: &mut SceneHost, b: PadButton) {
    host.world.set_pad(b.mask());
    let _ = host.tick();
    host.world.set_pad(0);
    for _ in 0..3 {
        let _ = host.tick();
    }
}

#[test]
fn a_seat_entered_duel_is_not_under_the_seat_wash() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let mut host = seated_into(&extracted, 52, SceneMode::BakaFighter);
    for _ in 0..30 {
        let _ = host.tick();
    }
    // Non-vacuity: the seat's wash is still installed underneath (the
    // field's tween actor, which comes back with the field) ...
    assert_eq!(
        host.world.held_scene_grade(),
        Some((2, [0xFF, 0xFF, 0xFF])),
        "the seat's subtractive white wash is the state this test is about"
    );
    // ... and none of it reaches the duel's frame.
    assert!(
        host.world.screen_tint_push_args().is_empty(),
        "a field colour push drew over the Baka duel: {:?}",
        host.world.screen_tint_push_args()
    );
}

/// Leave the machine, then let the field run: the seat's next pass must clear
/// the wash and open no box.
fn assert_clean_return(host: &mut SceneHost, how: &str) {
    for _ in 0..300 {
        let _ = host.tick();
    }
    assert_eq!(
        host.world.mode,
        SceneMode::Field,
        "{how}: back in the field"
    );
    assert!(
        host.world
            .dialog
            .inline
            .as_ref()
            .is_none_or(|r| r.panel.is_none()),
        "{how}: a box is open after the return (the refusal-line fallback)"
    );
    assert_eq!(
        host.world.held_scene_grade(),
        None,
        "{how}: the seat's wash is still installed - the `+0x79` pass never ran"
    );
    assert!(
        host.world.screen_tint_push_args().is_empty(),
        "{how}: a colour push is still covering the field: {:?}",
        host.world.screen_tint_push_args()
    );
}

#[test]
fn leaving_the_slot_machine_by_start_restores_the_field() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let mut host = seated_into(&extracted, 54, SceneMode::SlotMachine);
    for _ in 0..60 {
        let _ = host.tick();
    }
    press(&mut host, PadButton::Start);
    assert_clean_return(&mut host, "Start escape");
}

#[test]
fn leaving_the_slot_machine_by_its_quit_row_restores_the_field() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let mut host = seated_into(&extracted, 54, SceneMode::SlotMachine);
    for _ in 0..60 {
        let _ = host.tick();
    }
    // Triangle opens the cash-out picker, row 1 is Quit, and the leave fade
    // runs the bank commit and the return warp.
    press(&mut host, PadButton::Triangle);
    for _ in 0..20 {
        let _ = host.tick();
    }
    press(&mut host, PadButton::Down);
    press(&mut host, PadButton::Cross);
    for _ in 0..200 {
        if host.world.mode == SceneMode::Field {
            break;
        }
        let _ = host.tick();
    }
    assert_clean_return(&mut host, "Quit row");
}
