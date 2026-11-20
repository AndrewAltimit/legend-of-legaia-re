//! Disc-gated: where `town0d`'s Mei (flat index 54) stands once the Rim Elm
//! revisit arrival `P2[32]` has run.
//!
//! - **Before the beat.** `P2[32]` seats her at the table with
//!   `CC 36 51 5F 0E 02 42` (`+0x29E`): tile `(95, 14)`, compass index 2. The
//!   `town0d_mei_closeup` capture is parked on the beat's `4A 46 00` at
//!   `+0x2D7`, with her on that seat.
//! - **The beat's close.** `CC 36 51 60 0F 07 42` (`+0x449`) seats her on tile
//!   `(96, 15)` = `(12352, 1984)` with compass index 7, `0xE00` (the seat arm
//!   at `0x801E1904..0x801E1940`: `(b & 0x7F) * 128 + 0x40`, and the
//!   `0x80073F04` LUT entry `depth & 0xF`). The `town0d_house_interior` /
//!   `town0d_house_free` captures were taken in the same visit, after the
//!   beat retired, and hold her there.
//!
//! A card load past the beat runs no `P2[32]`, so the retail-compare seed
//! (a fresh entry with the capture's flags) stands her where her record does;
//! this test pins that the engine itself reaches the beat's seat.
//!
//! Skips + passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_scene_host::scene_host::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

/// Mei's flat MAN index (the `CC 36 ..` target byte).
const MEI: u16 = 0x36;

fn mei_slot(host: &SceneHost) -> u8 {
    host.world
        .field_vm
        .channels
        .iter()
        .find(|c| !c.object_bind && c.ctx.script_id == MEI)
        .and_then(|c| u8::try_from(c.placement_index).ok())
        .expect("Mei has a placement channel")
}

#[test]
fn town0d_p2_32_closes_with_mei_on_its_last_seat() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("town0d", 0).expect("enter town0d");
    eprintln!("[ran] town0d");
    let slot = mei_slot(&host);

    let man = host
        .scene
        .as_ref()
        .expect("scene")
        .field_man_payload(&host.index)
        .expect("read MAN")
        .expect("town0d carries a MAN");
    let man_file = legaia_scene_formats::man_section::parse(&man).expect("parse MAN");
    let (start, _, len) =
        legaia_engine_field::man_field_scripts::partition_record_span(&man_file, &man, 2, 32)
            .expect("town0d P2[32]");
    let body = &man[start..start + len];
    // The arrival runs on its own: the entry spawns `P2[32]` as a concurrent
    // context. Installing it a second time would replay its opening seats
    // over the first copy's close.
    let is_arrival = |tl: &legaia_engine_field::cutscene_timeline::CutsceneTimeline| {
        !tl.done && tl.bytecode.as_slice() == body
    };
    let running = |host: &SceneHost| {
        host.world
            .cutscene
            .timeline
            .as_ref()
            .is_some_and(is_arrival)
            || host.world.field_vm.helper_contexts.iter().any(is_arrival)
    };
    let cross = legaia_engine_system::input::PadButton::Cross.mask();
    let (mut seen, mut done) = (false, false);
    for t in 0..12_000u32 {
        host.world
            .input
            .set_pad(if t.is_multiple_of(2) { cross } else { 0 });
        host.tick().expect("tick");
        let _ = host.step_headless_frame_tail();
        let now = running(&host);
        seen |= now;
        if seen && !now {
            done = true;
            break;
        }
    }
    assert!(seen, "the town0d entry runs P2[32]");
    assert!(done, "town0d P2[32] runs to its end");
    let pos = host.world.npcs.positions.get(&slot).copied();
    let heading = host.world.npcs.heading(slot).wrapping_sub(0x800) & 0x0FFF;
    eprintln!("Mei slot {slot} at {pos:?} heading {heading:#05x}");
    assert_eq!(
        pos,
        Some((12352, 1984)),
        "P2[32]'s `CC 36 51 60 0F 07 42` seats Mei on tile (96, 15)"
    );
    assert_eq!(heading, 0xE00, "compass index 7 through the 0x80073F04 LUT");
}
