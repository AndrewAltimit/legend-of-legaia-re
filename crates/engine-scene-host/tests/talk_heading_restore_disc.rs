//! Disc-gated: the dialog SM's heading restore on talkers whose own record
//! turns them.
//!
//! The exit paths of `FUN_80039B7C` write the touch-time save `+0x5A` back
//! into `+0x26` whenever the talker's `+0x10 & 0x400000` is clear
//! (`0x80039CC8..0x80039CE8`, `0x80039E9C..0x80039EBC`), with no compare
//! against the snapped bearing. These placements turn themselves inside
//! their talks and never raise `0x400000` (`31 16`), so each ends its talk
//! on the heading it stood with before the press:
//!
//! - `bubu1` `P1[2]` / `P1[3]`: a face-at on the other (`CC 19 85 00 00 1A`,
//!   `CC 1A 85 00 00 19`).
//! - `nilboa` `P1[6]`: its talk loop's actor ops leave it on another heading
//!   (`0xFCD` in the engine's space against a pre-talk `0xA00`) when the last
//!   page closes; the restore turns it back.
//!
//! `doman` `P1[28]` (a face-at, then `38 82 1E` before the record ends) is
//! parked off-map on a fresh entry and is not driven here.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Talk to placement `slot` of `scene`: the heading before the press, the
/// first heading inside the talk, the heading once it ends, and the last
/// heading seen while it ran. `None` when the scene has no talk to drive.
fn talk(scene: &str, slot: u8) -> Option<(i16, i16, i16, i16)> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return None;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.world.toggles.use_vm_dialogue = true;
    host.enter_field_scene(scene, 0).expect("enter scene");
    for _ in 0..90 {
        let _ = host.world.tick();
    }
    let (tx, tz) = *host.world.npcs.positions.get(&slot)?;
    let p = usize::from(host.world.player_actor_slot.expect("player"));
    let faced = [
        (128i16, 0i16),
        (-128, 0),
        (0, 128),
        (0, -128),
        (96, 96),
        (-96, -96),
    ]
    .into_iter()
    .any(|(dx, dz)| {
        (0..0x1000).step_by(0x100).any(|r| {
            let ms = &mut host.world.actors[p].move_state;
            ms.world_x = tx + dx;
            ms.world_z = tz + dz;
            ms.render_26 = r as _;
            host.world.field_interact_probe_slot() == Some(slot)
        })
    });
    if !faced {
        eprintln!(
            "[skip] {scene} slot {slot}: no seat faces it at {:?}",
            (tx, tz)
        );
        return None;
    }
    host.world.set_pad(0);
    let _ = host.world.tick();
    let before = host.world.npcs.heading(slot);
    host.world.set_pad(0x4000);
    let _ = host.world.tick();
    let first = host.world.npcs.heading(slot);
    let mut last = first;
    for i in 0..1500 {
        let talking = host.world.dialogue_owns_input();
        if !talking && i > 4 {
            break;
        }
        last = host.world.npcs.heading(slot);
        host.world.set_pad(if i % 20 == 10 { 0x4000 } else { 0 });
        let _ = host.world.tick();
    }
    assert!(
        !host.world.dialogue_owns_input(),
        "{scene} slot {slot}: the talk ends"
    );
    let after = host.world.npcs.heading(slot);
    eprintln!(
        "[ok] {scene} slot {slot}: before {before:#05x} first {first:#05x} last-in-talk {last:#05x} after {after:#05x}"
    );
    Some((before, first, after, last))
}

#[test]
fn turned_talkers_end_on_their_pre_talk_heading() {
    for (scene, slot, turned) in [
        ("bubu1", 2u8, false),
        ("bubu1", 3, false),
        ("nilboa", 6, true),
    ] {
        let Some((before, _first, after, last)) = talk(scene, slot) else {
            continue;
        };
        if turned {
            assert_ne!(last, before, "{scene} slot {slot}: the talk turns it");
        }
        assert_eq!(
            after, before,
            "{scene} slot {slot}: the restore puts +0x5A back"
        );
    }
}
