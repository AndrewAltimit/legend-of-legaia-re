//! Disc-gated: Biron Monastery's dinner (`bylon` `P2[12]`, then Master
//! Zopu's talk `P2[13]`).
//!
//! - **The meal hall door opens.** `P2[12]` opens it with `CC 04 36` (`+0x62
//!   |= 0x28A`: re-seat on the last frame, reversed, hold) and `B1 04 00`.
//!   The door object's channel kept the restart bit of its spawn's `4C 35`
//!   after the prop's clip tick had consumed it, so the op moved only the
//!   reverse bit and the door stood shut through the dinner
//!   (`World::mirror_object_tick_bits`).
//! - **The score comes back when the hall cheers.** `P2[13]` fades the dinner
//!   score with a timed release (`35 C8 00 05`) under Zopu's "I will tell
//!   everyone", and replays it with `35 00 00 02` / `35 00 00 04` once the
//!   monks have heard. Retail's replay `FUN_80026478` re-applies the halved
//!   audio level (`FUN_8002657C(_DAT_8007B910 >> 1)`); a replay that kept the
//!   faded volume ran silent until the scene changed. The next fade belongs
//!   after the meal (`0x1377`, the second `35 C8 00 05`).
//! - **The plates say nothing.** The plates (`P1[39..=46]`) are textless
//!   records: `31 1D`, `31 00`, `31 1F`, `4C 42 50 ..`, `38 82 00`, then a
//!   `21` / `26 FE FF` park. A talk starts one past the spawn's `21` and ends
//!   on the next with no box; the old first-talk fallback read the `1F`
//!   operand of `31 1F` as a text lead and showed "LBP" (`4C 42 50`).
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset.

use legaia_engine_audio::AudioSink;
use legaia_engine_session::session::FieldLiveOpts;
use legaia_engine_session::{BootConfig, BootSession};
use legaia_engine_system::pad::PadButton;

type Sink = legaia_engine_audio::AudioOut;

/// `bylon` has 35 partition-0 and 56 partition-1 records, so `P2[n]` is
/// global record `91 + n` to op `0x44`.
const P2_BASE: u8 = 35 + 56;
/// The dinner's opening (Maya's grief, the party score).
const DINNER_START: u8 = P2_BASE + 12;
/// Zopu's talk: the favour, the cheer, the forest choice, Songi.
const DINNER_TALK: u8 = P2_BASE + 13;
/// The meal hall door: partition-0 record 4, the object at tile (51, 102).
const HALL_DOOR: usize = 4;

/// One tick with its vsyncs of audio pulled (44100 / 60 samples a vsync,
/// the frame step's vsyncs a tick, as a host paced by vsync pulls them);
/// returns the tick's summed square per vsync.
fn pump(s: &mut BootSession<Sink>, pad: u16) -> f64 {
    s.host.world.set_pad(pad);
    s.tick().expect("tick");
    s.step_audio_frame();
    let vsyncs = usize::from(s.host.world.clock.frame_step.max(1));
    s.audio.as_ref().expect("audio enabled").with_core(|c| {
        (0..735 * vsyncs)
            .map(|_| {
                let (l, r) = c.next_frame();
                f64::from(l).powi(2) + f64::from(r).powi(2)
            })
            .sum::<f64>()
            / vsyncs as f64
    })
}

/// A dialogue beat waits for Cross: press 2 frames in 16.
fn cross(f: u32) -> u16 {
    if f % 16 < 2 {
        PadButton::Cross.mask()
    } else {
        0
    }
}

/// `bylon` entered before the dinner (the intro's `0x293` up, the door shut
/// by its spawn).
fn dinner_session() -> Option<BootSession<Sink>> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    };
    let mut s = BootSession::<Sink>::open_disc_detached_audio(
        std::path::Path::new(&disc),
        &BootConfig::default(),
    )
    .expect("boot session");
    s.host.world.system_flag_set(0x293);
    s.enter_scene_live("bylon", &FieldLiveOpts::default())
        .expect("enter bylon");
    for _ in 0..30 {
        pump(&mut s, 0);
    }
    Some(s)
}

/// Queue `record` and play `frames` ticks of it, paging its boxes.
fn play(s: &mut BootSession<Sink>, record: u8, frames: u32) {
    s.host.world.field_vm.pending_record_spawns.push(record);
    for f in 0..frames {
        pump(s, cross(f));
    }
}

/// The hall door's clip cursor and frame count.
fn door(s: &BootSession<Sink>) -> (i16, u16) {
    s.host
        .world
        .props
        .bank
        .props
        .values()
        .find(|p| p.record == HALL_DOOR)
        .map(|p| (p.anim.cursor, p.anim.frames))
        .expect("the hall door's prop")
}

#[test]
fn the_dinner_opens_the_meal_hall_door() {
    let Some(mut s) = dinner_session() else {
        return;
    };
    let (cursor, frames) = door(&s);
    assert!(
        frames > 1,
        "the door binds its swing clip ({frames} frames)"
    );
    assert_eq!(cursor, 0, "the spawn holds the door shut on frame 0");
    play(&mut s, DINNER_START, 120);
    let (cursor, frames) = door(&s);
    assert!(
        i32::from(cursor) >= (i32::from(frames) - 1) * 16,
        "P2[12]'s `CC 04 36` seats the door on its last frame (open): cursor {cursor}, {frames} frames"
    );
}

#[test]
fn the_dinner_score_returns_when_the_hall_cheers() {
    let Some(mut s) = dinner_session() else {
        return;
    };
    play(&mut s, DINNER_START, 3000);
    assert!(
        s.audio
            .as_ref()
            .and_then(|a| a.sequencer_progress())
            .is_some(),
        "P2[12] leaves the dinner score attached"
    );
    s.host
        .world
        .field_vm
        .pending_record_spawns
        .push(DINNER_TALK);
    // Walk Zopu's talk to the first fade's pause, then to the replay that
    // follows it (the gate reopens).
    let mut faded = false;
    let mut replay_at = None;
    let mut window = 0.0f64;
    let mut heard = None;
    for f in 0..6000u32 {
        let e = pump(&mut s, cross(f));
        let audio = s.audio.as_ref().expect("audio enabled");
        let paused = audio.sequencer_paused();
        faded |= paused;
        if faded && replay_at.is_none() && !paused && audio.sequencer_progress().is_some() {
            replay_at = Some(f);
        }
        let Some(at) = replay_at else {
            continue;
        };
        // Past the replay's first half-second, listen for two seconds.
        if f > at + 30 {
            window += e;
        }
        if f == at + 150 {
            let vol = audio.sequencer_progress().map(|p| p.master_vol);
            let rms = (window / (735.0 * 2.0 * 120.0)).sqrt();
            heard = Some((vol, rms));
            break;
        }
    }
    let replay_at = replay_at.expect("Zopu's talk fades the score and replays it (`2 · 4`)");
    let (vol, rms) = heard.expect("the replay runs two seconds before anything else");
    eprintln!("[dinner] replay at frame {replay_at}: sequence volume {vol:?}, rms {rms:.0}");
    assert_eq!(
        vol,
        Some(107),
        "the replay re-applies the halved audio level (`FUN_80026478`), not the faded volume"
    );
    assert!(
        rms > 200.0,
        "the replayed score is audible under the cheer (rms {rms:.0})"
    );
}

#[test]
fn the_dinner_plates_say_nothing() {
    let Some(mut s) = dinner_session() else {
        return;
    };
    play(&mut s, DINNER_START, 3000);
    s.host.world.toggles.use_vm_dialogue = true;
    let hide = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;
    let mut talked = 0;
    for slot in 39u8..=46 {
        let Some(&(x, z)) = s.host.world.npcs.positions.get(&slot) else {
            continue;
        };
        if (x, z) == (hide, hide) || s.host.world.field_placement_retired(slot) {
            continue;
        }
        talked += 1;
        let pslot = usize::from(s.host.world.player_actor_slot.expect("a player"));
        {
            let ms = &mut s.host.world.actors[pslot].move_state;
            ms.world_x = x;
            ms.world_z = z + 128;
        }
        s.host.world.trigger_field_interact(0, slot);
        for f in 0..60 {
            pump(
                &mut s,
                if f % 4 == 0 {
                    PadButton::Cross.mask()
                } else {
                    0
                },
            );
            assert!(
                !s.host
                    .world
                    .dialog
                    .inline
                    .as_ref()
                    .is_some_and(|d| d.panel.is_some()),
                "plate P1[{slot}] opened a box: its record has no text"
            );
        }
    }
    assert!(
        talked >= 4,
        "the dinner sets the plates on the table ({talked} reached)"
    );
}
