//! Page ladder for the two status marks a command surface stamps over its
//! chips - a `docs/tooling/reach-triage.md` gate pair, both behind an ailment
//! on the acting member's `+0x16E` word:
//!
//! - the **Curse** plate over the ring's Magic chip (`FUN_801DBEC4`,
//!   `battle_party_panel::curse_plate_on_chip`), drawn while bit `0x1000` is
//!   set (`0x801D1330..0x801D1360`);
//! - the **Rot** stamp over a rotted limb's direction chip on the arts entry
//!   (`FUN_801DBDDC`, `battle_party_panel::rot_stamp_on_arts_chip`), drawn
//!   after the D-pad glyph for each rolled limb bit
//!   (`0x801D1DA8..0x801D1E54`).
//!
//! No pad stream earns either: an enemy has to land the ailment first. The
//! gate is the one piece of state it is, so the ladder seeds it the way an
//! inflicting strike does - `debug_apply_battle_status` writes the world's
//! status tracker, the same tracker a hit writes - and does the rest by pad:
//! `Begin` on the round prompt, Left for the ring's Attack arm, Right for
//! `Command` on the attack-mode prompt.
//!
//! Each mark is scored by contrast against the same fight with no ailment,
//! driven by the same pad stream: the cursed ring must carry the plate the
//! shared builder places, and the rotted arts entry must draw more sprites
//! than the healthy one. Both hosts draw both marks through one `engine-ui`
//! builder (`battle_command_menu_sprites`, `arts_input_rot_stamp_draws`), so
//! the page is the host a test can drive.
//!
//! Rendered at 960x720: stage scale 3, origin `(0, 0)`. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_ui::battle_command_ui as bcu;
use legaia_web_viewer::runtime::LegaiaRuntime;

const LEFT: u16 = 0x0080;
const RIGHT: u16 = 0x0020;

/// `dst` of the Curse plate at the page's 960x720 stage, from the shared
/// builder - so the expectation is not transcribed from either host.
fn curse_dst() -> [i64; 4] {
    let d = bcu::curse_plate_sprite((0, 0, 64, 16), bcu::RASERU_MARK_ANCHOR, (0, 0), 3).dst;
    [d.0 as i64, d.1 as i64, d.2 as i64, d.3 as i64]
}

fn sprites(rt: &mut LegaiaRuntime) -> Vec<[i64; 4]> {
    let v: serde_json::Value =
        serde_json::from_str(&rt.play_overlay_draws_json(960, 720)).expect("overlay json");
    v["sprites"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|s| std::array::from_fn(|i| s["dst"][i].as_i64().unwrap_or(i64::MIN)))
        .collect()
}

/// Whether the command ring's plates draw this frame (its Up arm's plate:
/// a 20-px-tall quad on the command cluster's top row).
fn ring_up(sprites: &[[i64; 4]]) -> bool {
    let (_, up_y) = bcu::CLUSTER_COMMAND.plate_origin(bcu::ChipSeat::Up);
    sprites
        .iter()
        .any(|d| d[3] == 60 && d[1] == i64::from(up_y) * 3)
}

/// What the run saw: the first ring frame's sprites and the arts entry's.
struct Run {
    ring: Vec<[i64; 4]>,
    arts: Vec<[i64; 4]>,
}

/// A seeded party on `map01`, a forced fight, `status` applied to the lead
/// as the battle opens, then `Begin` -> Attack -> `Command`.
fn run(status: Option<&str>) -> Option<Run> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).expect("read disc");
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load disc");
    rt.debug_enter_town01_opening()
        .expect("enter the town01 opening");
    for _ in 0..8 {
        rt.tick_frame().expect("tick");
    }
    rt.enter_field("map01").expect("enter map01");
    for _ in 0..5 {
        rt.tick_frame().expect("tick");
    }
    assert!(rt.debug_force_battle(-1), "map01 arms a fight");
    let mut applied = status.is_none();
    let mut ring = None;
    for f in 0..1200u32 {
        if rt.play_battle_active() {
            if !applied {
                applied = rt.debug_apply_battle_status(0, status.expect("status"));
            }
            let s = sprites(&mut rt);
            if applied && ring_up(&s) {
                ring = Some(s);
                break;
            }
        }
        // Answer the round prompt with Begin.
        let pad = if rt.play_battle_active() && f % 20 == 19 {
            LEFT
        } else {
            0
        };
        rt.set_pad(pad);
        rt.tick_frame().expect("tick");
    }
    let ring = ring.expect("the fight opened the command ring");
    // The ring can open on the frame the Begin press is still down, and a
    // press is an edge: release first or the Left below is no press at all.
    rt.set_pad(0);
    for _ in 0..4 {
        rt.tick_frame().expect("tick");
    }
    // Left = the ring's Attack arm, then Right = `Command` on the attack-mode
    // prompt, which opens the arts entry.
    for pad in [LEFT, RIGHT] {
        rt.set_pad(pad);
        rt.tick_frame().expect("tick");
        rt.set_pad(0);
        for _ in 0..10 {
            rt.tick_frame().expect("tick");
        }
    }
    let arts = sprites(&mut rt);
    Some(Run { ring, arts })
}

#[test]
fn a_cursed_lead_draws_the_curse_plate_on_the_page_ring() {
    let Some(healthy) = run(None) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let cursed = run(Some("Curse")).expect("disc present");
    let want = curse_dst();
    assert!(
        !healthy.ring.contains(&want),
        "a healthy lead's ring drew the Curse plate"
    );
    assert!(
        cursed.ring.contains(&want),
        "a cursed lead's ring drew no Curse plate at {want:?}"
    );
    eprintln!("[ok] curse plate: on the cursed ring, absent from the healthy one");
}

#[test]
fn a_rotted_limb_stamps_its_arts_chip_on_the_page() {
    let Some(healthy) = run(None) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let rotted = run(Some("Rot")).expect("disc present");
    // The stamp is `FUN_801DBDDC`'s quad: 24 rows tall (`y-8 ..= y+0xF`), so
    // 72 surface pixels at stage scale 3. The party HUD's own Rot icon is
    // also new on the rotted frame and is not this - the height tells them
    // apart.
    let extra: Vec<_> = rotted
        .arts
        .iter()
        .filter(|d| !healthy.arts.contains(d) && d[3] == 24 * 3)
        .collect();
    eprintln!("rotted-only sprites: {extra:?}");
    assert!(
        !extra.is_empty(),
        "the rotted lead's arts entry drew nothing the healthy one did not \
         (healthy {} sprites, rotted {})",
        healthy.arts.len(),
        rotted.arts.len()
    );
    eprintln!(
        "[ok] rot stamp: {} sprite(s) only the rotted arts entry draws",
        extra.len()
    );
}
