//! The dance hall's return re-runs `koin3`'s scene entry, which spawns the
//! Disco King's verdict.
//!
//! Retail's minigame return warp `FUN_80026018` restores the departure scene
//! and latches mode 2 (MAIN INIT) with the field-entry word at 2 - the warp
//! arm a battle's return takes - and the MAN loader re-runs the entry script
//! `P1[0]`. `koin3`'s entry tests `0x59B` at `+0x16C`, the marker the how-to
//! record `P2[5]` raises before its `3E 6A` warp (`+0x1B5`), clears it and
//! spawns `44 56` = `P2[8]`, the Disco King's points line, which ends by
//! parking Noa (flat `75`, `A3 4B 7F 7F` at `+0x2C4`). The placements
//! themselves survive the round trip (the warp arm re-hydrates the actor
//! lists, `FUN_801D7518`, rather than respawning them), so Noa stands on the
//! seat `P2[5]` gave her until the verdict parks her: the state
//! `koin3_disco_king_points_box` holds her at `(6080, 13120)` on the points
//! line. The engine re-ran the entry only on a battle's return, so the field
//! came back from the lesson with `0x59B` up, Noa standing and no verdict.
//!
//! Driven on the session both play hosts run, the lesson entered through its
//! own record and left with Start. Disc-gated: skips and passes when
//! `LEGAIA_DISC_BIN` is unset.

use legaia_engine_audio::{SPU_INTERNAL_RATE, TestAudioSink};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_session::session::FieldLiveOpts;
use legaia_engine_session::{BootConfig, BootSession};
use legaia_engine_system::input::PadButton;

/// `koin3` `P2[5]`, the how-to lesson's record: `N0 + N1 + 5` = `36 + 42 + 5`.
const HOWTO_RECORD: u8 = 0x53;
/// The lesson's marker, raised before the warp and tested by the entry.
const HOWTO_MARKER: u16 = 0x59B;
/// Noa's placement slot (flat `75` = `N0 + 39`).
const NOA_SLOT: u8 = 39;
/// `FIELD_OFFMAP_HIDE_XZ`, the park seat.
const PARKED: (i16, i16) = (16320, 16320);

fn session() -> Option<BootSession<TestAudioSink>> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let host =
        legaia_engine_scene_host::scene_host::SceneHost::open_disc(std::path::Path::new(&disc))
            .expect("open disc");
    Some(
        BootSession::from_host(host, None, &BootConfig::default(), || {
            Ok(TestAudioSink::new(SPU_INTERNAL_RATE))
        })
        .expect("boot session"),
    )
}

fn step(s: &mut BootSession<TestAudioSink>, pad: u16) {
    s.host.world.set_pad(pad);
    s.tick().expect("tick");
}

/// Cross on every fourth tick: pages the boxes and confirms the picker's
/// first arm (the lesson).
fn press(i: usize) -> u16 {
    if i.is_multiple_of(4) {
        PadButton::Cross.mask()
    } else {
        0
    }
}

#[test]
fn the_lessons_return_runs_the_disco_kings_verdict() {
    let Some(mut s) = session() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    s.enter_scene_live("koin3", &FieldLiveOpts::default())
        .expect("enter koin3");
    for _ in 0..30 {
        step(&mut s, 0);
    }
    s.host
        .world
        .field_vm
        .pending_record_spawns
        .push(HOWTO_RECORD);
    let mut t = 0;
    while s.host.world.mode != SceneMode::Dance && t < 4000 {
        step(&mut s, press(t));
        t += 1;
    }
    assert_eq!(s.host.world.mode, SceneMode::Dance, "the lesson warps");
    let staged = s.host.world.npcs.positions.get(&NOA_SLOT).copied();
    assert_eq!(staged, Some((6080, 13120)), "the lesson stages Noa");
    assert!(s.host.world.system_flag_test(HOWTO_MARKER));

    step(&mut s, 0);
    step(&mut s, PadButton::Start.mask());
    step(&mut s, 0);
    assert_eq!(s.host.world.mode, SceneMode::Field, "back on the field");
    assert_eq!(
        s.host.world.npcs.positions.get(&NOA_SLOT).copied(),
        staged,
        "the placements survive the round trip"
    );
    step(&mut s, 0);
    assert!(
        !s.host.world.system_flag_test(HOWTO_MARKER),
        "the re-run entry takes the marker down"
    );
    assert_eq!(s.host.bgm_track_word, Some(2055), "the dance floor's track");
    let mut parked_at = None;
    for i in 0..900 {
        step(&mut s, press(i));
        if s.host.world.npcs.positions.get(&NOA_SLOT).copied() == Some(PARKED) {
            parked_at = Some(i);
            break;
        }
    }
    eprintln!("[ran] verdict parked Noa after {parked_at:?} ticks");
    assert!(parked_at.is_some(), "the verdict runs to its close");
}
