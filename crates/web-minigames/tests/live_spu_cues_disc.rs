//! Disc-gated: every minigame cue on this page keys a voice on the live SPU and
//! mixes to exactly what the play hosts' director mixes for the same cue.
//!
//! The page used to play its slot, dance and Muscle Dome cues as PCM decoded
//! out of the VAB at hand-set gains: no ADSR, no retail volume or pan, no reverb
//! send, and no cue at `0x100` or up on the live path. The oracle here is the
//! director itself: the same id, the same banks staged the way each game's door
//! warp stages them, over the same device-free sink. The two renders must be
//! the same samples, not merely both loud.
//!
//! Skips and passes without `LEGAIA_DISC_BIN`.

use legaia_engine_audio::{AudioSink, SPU_INTERNAL_RATE, TestAudioSink};
use legaia_engine_core::world::SideBandBank;
use legaia_engine_minigames::dance::{STING_TIER_VARIANT, dance_hit_sting_cues};
use legaia_engine_minigames::slot_machine::SPIN_VOICE;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_mode::sfx_residency::SfxBankResidency;
use legaia_engine_session::AudioDirector;
use legaia_game_tables::sfx_table::{SLOT0_SYSTEM_BANK_PROT_INDEX, SfxTable};
use legaia_minigame_formats::{dance_art, minigame_sfx};
use legaia_web_minigames::LegaiaMinigames;
use std::sync::{Arc, OnceLock};

/// A second of mix: any cue's attack and the start of its release.
const RENDER_FRAMES: usize = SPU_INTERNAL_RATE as usize;

fn disc() -> Option<&'static [u8]> {
    static BYTES: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    BYTES
        .get_or_init(|| std::fs::read(std::env::var_os("LEGAIA_DISC_BIN")?).ok())
        .as_deref()
}

fn page(game: &str) -> Option<LegaiaMinigames> {
    let mut mg = LegaiaMinigames::new();
    mg.load_disc(disc()?.to_vec()).ok()?;
    mg.minigame_audio_enter(game);
    assert!(mg.minigame_audio_open());
    Some(mg)
}

/// The director as the play hosts stage it on the door warp into `mode`.
fn native(mode: SceneMode, bundle_prot: Option<u32>) -> AudioDirector<TestAudioSink> {
    let bytes = disc().expect("disc");
    let scus = legaia_iso::image::extract_scus(bytes).expect("SCUS");
    let prot = legaia_iso::image::extract_prot_dat(bytes).expect("PROT");
    let entries = legaia_prot::toc_index::parse_prot_toc(&prot).expect("TOC");
    let read = |e: u32| -> Option<Vec<u8>> {
        let m = entries.get(e as usize)?;
        let (o, n) = (m.byte_offset as usize, m.size_bytes as usize);
        prot.get(o..o + n).map(<[u8]>::to_vec)
    };
    #[allow(clippy::arc_with_non_send_sync)]
    let mut d = AudioDirector::new(Arc::new(TestAudioSink::new(SPU_INTERNAL_RATE)));
    let table = SfxTable::from_scus(&scus).expect("table");
    d.set_sfx_bank(legaia_engine_audio::SfxBank::from_descriptors(
        table
            .active()
            .map(|(i, r)| (i, r.program, r.tone, r.note, r.flags)),
    ));
    d.set_sfx_cue_slots(table.cue_slots());
    assert!(d.stage_resident_slot0(read(SLOT0_SYSTEM_BANK_PROT_INDEX).unwrap()));
    let bundle = bundle_prot.and_then(read).unwrap_or_default();
    let side = (mode == SceneMode::MuscleDome).then_some(SideBandBank::ARENA);
    d.sync_field_sfx(&bundle, false, side, read);
    let want = SfxBankResidency::default().sync(
                mode,
                "",
                None,
                legaia_engine_mode::sfx_residency::SharedRegionBank::CLASS2,
            );
    assert!(d.sync_shared_region(want, read), "{mode:?}'s region staged");
    d
}

fn render_native(d: &AudioDirector<TestAudioSink>) -> Vec<(i16, i16)> {
    (0..RENDER_FRAMES).map(|_| d.audio().next_frame()).collect()
}

fn peak(pcm: &[(i16, i16)]) -> i32 {
    pcm.iter()
        .map(|&(l, r)| i32::from(l).abs().max(i32::from(r).abs()))
        .max()
        .unwrap_or(0)
}

/// The page's render of `id` against the director's, sample for sample.
fn assert_cue_matches(game: &str, mode: SceneMode, bundle_prot: Option<u32>, id: u16) {
    let mut mg = page(game).unwrap();
    assert!(mg.minigame_sfx_cue(id), "{game} cue {id:#x} keyed no voice");
    let voices = mg.audio_live_voices_for_test();
    let got = mg.audio_render_for_test(RENDER_FRAMES);

    let mut d = native(mode, bundle_prot);
    let report = if id < 0x100 {
        d.fire_now(id)
    } else {
        d.fire_ring_now(id as i16)
    };
    assert!(!report.fired.is_empty(), "native {game} cue {id:#x}");
    let native_voices = d.audio().with_spu(|spu| {
        spu.voices
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.is_off())
            .map(|(i, _)| i as u8)
            .collect::<Vec<_>>()
    });
    let want = render_native(&d);

    assert!(peak(&want) > 16, "{game} cue {id:#x} is not silence");
    assert_eq!(
        voices, native_voices,
        "{game} cue {id:#x} keys the drainer's voices"
    );
    let first_diff = got.iter().zip(&want).position(|(a, b)| a != b);
    assert_eq!(
        first_diff, None,
        "{game} cue {id:#x} mix differs from native"
    );
}

#[test]
fn slot_machine_cues_mix_as_on_the_play_hosts() {
    if disc().is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    }
    let bank = Some(minigame_sfx::SLOT_SFX_BANK_PROT_INDEX as u32);
    for id in [
        minigame_sfx::CUE_SLOT_REEL_STOP,
        minigame_sfx::CUE_SLOT_PAYOUT_TICK,
        minigame_sfx::CUE_SLOT_REACH,
        minigame_sfx::CUE_SLOT_REACH_1,
        minigame_sfx::CUE_SLOT_REACH_2,
    ] {
        assert_cue_matches("slot", SceneMode::SlotMachine, bank, id);
    }
    // The picker's static confirm, off the system bank.
    assert_cue_matches("slot", SceneMode::SlotMachine, bank, 0x20);
}

#[test]
fn dance_cues_and_stings_mix_as_on_the_play_hosts() {
    if disc().is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    }
    let bank = Some(dance_art::DANCE_SFX_BANK_PROT_INDEX as u32);
    // The count-in's intro is `0x200`, which the page's old live path refused.
    for id in [0x200u16, 0x201, 0x202, 0x203, 0x205, 0x210] {
        assert_cue_matches("dance", SceneMode::Dance, bank, id);
    }
    // The direct-keyed sting: both voices, at the cold-reset voice volume.
    for r in [0u16, 1, 2, STING_TIER_VARIANT] {
        let mut mg = page("dance").unwrap();
        assert_eq!(mg.dance_sting_key(r), 2, "sting {r} keys both voices");
        let got = mg.audio_render_for_test(RENDER_FRAMES);
        let mut d = native(SceneMode::Dance, bank);
        let word = legaia_engine_menus::new_game::GAME_STATE_COLD_RESET.voice_volume as u32;
        for c in dance_hit_sting_cues(r, word) {
            assert!(
                d.key_on_voice_attr(legaia_engine_audio::VoiceAttr::from_cue_words(
                    c.voice,
                    c.vab_program_tone,
                    c.note_and_fine,
                    c.volume,
                ))
            );
        }
        let want = render_native(&d);
        assert!(peak(&want) > 16, "sting {r} is not silence");
        assert_eq!(got, want, "sting {r} mix differs from native");
    }
}

#[test]
fn muscle_dome_cues_mix_as_on_the_play_hosts() {
    if disc().is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    }
    let bank = Some(minigame_sfx::ARENA_SFX_BUNDLE_PROT_INDEX as u32);
    // The blips, the melee impact, and the tally's arena-bank "ka-chings".
    for id in [0x20u16, 0x21, 0x22, 0x09, 0x202, 0x203] {
        assert_cue_matches("muscle", SceneMode::MuscleDome, bank, id);
    }
}

#[test]
fn baka_fighter_cues_mix_as_on_the_play_hosts() {
    if disc().is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    }
    for id in [0x09u16, 0x20, 0x21] {
        assert_cue_matches("baka", SceneMode::BakaFighter, None, id);
    }
}

/// The reel motor is a voice with a lifetime: `slot_step` keys it as the reels
/// start and releases it when they stop. It used to be a looped buffer the page
/// timed off the state it read back.
#[test]
fn the_reel_motor_is_keyed_and_released_by_the_machine() {
    let Some(mut mg) = page("slot") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(!mg.audio_live_voices_for_test().contains(&SPIN_VOICE));
    // Cross spins at idle.
    const CROSS: u32 = 0x40;
    assert!(mg.slot_start(1000));
    mg.slot_step(CROSS);
    for _ in 0..10 {
        mg.slot_step(0);
    }
    assert!(
        mg.audio_live_voices_for_test().contains(&SPIN_VOICE),
        "the spin keys the motor voice"
    );
    let spinning = mg.audio_render_for_test(4096);
    assert!(peak(&spinning) > 0, "the motor sounds");
    // Leaving mid-spin must not strand the motor.
    mg.minigame_audio_enter("baka");
    let _ = mg.audio_render_for_test(RENDER_FRAMES);
    assert!(
        !mg.audio_live_voices_for_test().contains(&SPIN_VOICE),
        "leaving the tab releases the motor"
    );
}

/// A closed slot is silent, on the page as on the play hosts: the field bank
/// (slot 6) is not resident in the dance.
#[test]
fn a_cue_on_a_slot_this_game_closed_keys_nothing() {
    let Some(mut mg) = page("dance") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let scus = legaia_iso::image::extract_scus(disc().unwrap()).unwrap();
    let table = SfxTable::from_scus(&scus).unwrap();
    let id = table
        .cue_slots()
        .find(|&(_, slot)| slot == 6)
        .map(|(id, _)| id)
        .expect("a category-6 cue");
    assert!(!mg.minigame_sfx_cue(u16::from(id)));
    // The fishing hook and catch cues are category 6 as well: silent on the
    // page as on the play hosts, which stage only the region's own bank.
    let mut mg = page("fishing").unwrap();
    for id in [0x3Au16, 0x2B] {
        assert!(!mg.minigame_sfx_cue(id), "cue {id:#x}");
    }
}

/// A voice-band id is declined, not truncated into a static row.
#[test]
fn a_voice_band_id_is_declined() {
    let Some(mut mg) = page("slot") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(!mg.minigame_sfx_cue(0x120));
    assert!(mg.audio_live_voices_for_test().is_empty());
}

/// Fishing's own cues resolve through the venue scene's bundle into the
/// overlay's slot-2 bank.
#[test]
fn fishing_cues_resolve_off_the_venue_bundle() {
    let Some(mut mg) = page("fishing") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    for id in [0x201u16, 0x204, 0x206] {
        assert!(mg.minigame_sfx_cue(id), "fishing cue {id:#x}");
    }
    assert!(peak(&mg.audio_render_for_test(RENDER_FRAMES)) > 16);
}

/// One whole spin through the machine's own kernel: the ring cues it raises
/// sound on the live SPU, the way `World::route_slot_sounds` hands them over.
#[test]
fn a_spin_sounds_the_machines_ring_cues() {
    let Some(mut mg) = page("slot") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    const CROSS: u32 = 0x40;
    const SQUARE: u32 = 0x80;
    const CIRCLE: u32 = 0x20;
    assert!(mg.slot_start(1000));
    mg.slot_step(CROSS);
    for _ in 0..60 {
        mg.slot_step(0);
    }
    for stop in [SQUARE, CROSS, CIRCLE] {
        mg.slot_step(stop);
        for _ in 0..60 {
            mg.slot_step(0);
        }
    }
    let due = mg.audio_ring_due_for_test();
    let stops = due
        .iter()
        .filter(|&&id| id as u16 == minigame_sfx::CUE_SLOT_REEL_STOP)
        .count();
    assert_eq!(stops, 3, "one click per reel stop: {due:x?}");
    assert!(peak(&mg.audio_render_for_test(RENDER_FRAMES)) > 16);
}

/// A cast raises its landing cue through the pond's own ring store.
#[test]
fn a_fishing_cast_rings_the_lure_landing() {
    let Some(mut mg) = page("fishing") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(mg.fishing_pond_start(0, 1, 2, 60, 0, 0, 0, 0, 0xBADF00D));
    for f in 0..1200 {
        // Wind up, lock the power meter, then let the lure fly.
        let _ = mg.fishing_pond_tick(0, f == 0 || f == 30, 0);
        if mg.audio_ring_due_for_test().contains(&0x204) {
            return;
        }
    }
    panic!("no lure landing: {:x?}", mg.audio_ring_due_for_test());
}

/// The INTERVAL screen's four "ka-ching" ring cues come due on retail's
/// stagger, off the arena's own bundle, and each resolves to a voice in the
/// arena's side bank.
#[test]
fn the_interval_tally_rings_its_four_cues_on_the_stagger() {
    let Some(mut mg) = page("muscle") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(mg.muscle_contest_start(0, 0));
    assert!(mg.muscle_report_leg(true, 0, 3, 1000, 0, 0) >= 0);
    mg.muscle_tally_voice_reset();
    let mut at = Vec::new();
    for t in 1..=120 {
        let before = mg.audio_ring_due_for_test().len();
        mg.muscle_tally_voice(t);
        at.extend(
            mg.audio_ring_due_for_test()[before..]
                .iter()
                .map(|&id| (t - 1, id)),
        );
    }
    assert_eq!(
        at,
        [(0, 0x202), (0x1E, 0x202), (0x3C, 0x202), (0x5A, 0x203)],
        "HUB_TALLY_CUES on HUB_TALLY_CUE_STAGGER"
    );
    assert!(peak(&mg.audio_render_for_test(RENDER_FRAMES)) > 16);
}
