//! The dance hall sounds its own cues on the session both play hosts run.
//!
//! Every dance cue is a direct store into the SFX ring (`sh id,
//! DAT_8007B6D8[slot]`): the count-in's intro `0x200` (`FUN_801d2d98`), the
//! run start `0x201` (`FUN_801cf470`), the award's miss `0x210` and groovy
//! tiers (`FUN_801d1af4`). Ids `>= 0x200` resolve through the overlay's own
//! `efect.dat` (PROT 1228) into its class-2 bank PROT 1231 - a bank larger
//! than the port's shared SFX region, so it is staged across that region and
//! the BGM tail. Before, the count-in cue went through the cue dispatcher
//! (which reads `0x200` as a CD-XA voice and declines it), the miss and tier
//! cues and the good-step stings were never raised, and the bank stayed
//! closed: the hall had its song and nothing else.
//!
//! Driven over the device-free `TestAudioSink`, door warp and all.
//! Disc-gated: skips and passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_audio::{SPU_INTERNAL_RATE, TestAudioSink};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::SceneMode;
use legaia_engine_session::boot::FieldLiveOpts;
use legaia_engine_session::{BootConfig, BootSession};

const DANCE_SUB_ID: u8 = 6;

fn session() -> Option<BootSession<TestAudioSink>> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let host = legaia_engine_core::scene::SceneHost::open_disc(std::path::Path::new(&disc))
        .expect("open disc");
    Some(
        BootSession::from_host(host, None, &BootConfig::default(), || {
            Ok(TestAudioSink::new(SPU_INTERNAL_RATE))
        })
        .expect("boot session"),
    )
}

/// One tick and its fired cue ids.
fn step(s: &mut BootSession<TestAudioSink>, pad: u16) -> Vec<u16> {
    s.host.world.set_pad(pad);
    s.tick().expect("tick");
    let bgm = s.bgm.as_mut().expect("audio enabled");
    bgm.tick_sfx_frame()
        .fired
        .into_iter()
        .map(|(id, _)| id)
        .collect()
}

#[test]
fn the_dance_keys_its_own_cues_out_of_its_own_bank() {
    let Some(mut s) = session() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    s.enter_scene_live("koin3", &FieldLiveOpts::default())
        .expect("enter koin3");
    s.host.world.request_minigame_warp(DANCE_SUB_ID);
    let mut fired = Vec::new();
    for _ in 0..120 {
        fired.extend(step(&mut s, 0));
        if s.host.world.mode == SceneMode::Dance {
            break;
        }
    }
    assert_eq!(
        s.host.world.mode,
        SceneMode::Dance,
        "the door warp opens the hall"
    );

    // The count-in plays out; its intro cue keys a voice.
    for _ in 0..400 {
        fired.extend(step(&mut s, 0));
        if s.host.world.minigames.dance_status_visible() {
            break;
        }
    }
    assert!(s.host.world.minigames.dance_status_visible());
    let bgm = s.bgm.as_ref().expect("audio");
    assert_eq!(
        bgm.prot_for_slot(2),
        Some(legaia_asset::dance_art::DANCE_SFX_VAB_PROT_INDEX as u32),
        "the dance's class-2 bank is resident"
    );
    eprintln!(
        "[ran] count-in fired {fired:x?}; spill {:?}",
        bgm.bgm_tail().shared_spill()
    );
    assert!(
        fired.contains(&legaia_engine_core::dance::COUNTIN_INTRO_CUE),
        "the count-in intro cue keys a voice: {fired:x?}"
    );

    // Mash a button off the beat for a while: a miss keys its cue.
    let mut song = Vec::new();
    for i in 0..600 {
        let pad = if i % 7 == 0 {
            PadButton::Square.mask()
        } else {
            0
        };
        song.extend(step(&mut s, pad));
        if song.contains(&legaia_engine_core::dance::AWARD_MISS_CUE) {
            break;
        }
    }
    eprintln!("[ran] song fired {song:x?}");
    assert!(
        song.contains(&legaia_engine_core::dance::AWARD_MISS_CUE),
        "a judged miss keys the miss cue: {song:x?}"
    );
}
