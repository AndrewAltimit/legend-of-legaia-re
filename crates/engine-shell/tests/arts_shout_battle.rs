//! Disc-gated end-to-end check of the battle **arts-voice shout** path:
//! drive a Tactical Art through the live player-driven battle session and
//! assert the shout's CD-XA PCM lands in the audio mix at the expected time.
//!
//! Art under test: Vahn's Somersault record (action constant `0x27`), entered
//! as its own one-press command string (`Up`) in the retail per-press Arts
//! command input - the capture-verified arts-voice anchor (a live retail
//! trace of Somersault fired `FUN_8003D53C(XA2, chan 0/6, ...)` from
//! `FUN_8004C140`). The test:
//!
//! 1. builds the same [`legaia_engine_audio::ArtsShoutBank`] the boot path
//!    stages (`read_arts_shout_bank`: XA2/XA4/XA6 channel demux + SCUS cue
//!    tables) from the real disc;
//! 2. walks a `World` into a battle and *types* the art through the live Arts
//!    command input, draining the [`BattleShoutCue`] queue each tick;
//! 3. resolves the cue against the bank and feeds the clip through the
//!    device-free [`legaia_engine_audio::OfflineMixer`] with the modeled
//!    CD-response start delay, asserting silence during the delay window and
//!    non-silent shout PCM after it (audio trails the animation, never leads);
//! 4. keeps a baseline pass: the same battle WITHOUT the staged art record
//!    emits no shout cue (synthetic arts are silent), so the positive
//!    assertion is non-vacuous.
//!
//! Skips (passing) when `LEGAIA_DISC_BIN` is unset, like every disc test.
//!
//! [`BattleShoutCue`]: legaia_engine_core::battle_events::BattleShoutCue

use std::path::PathBuf;

use legaia_engine_core::arts_command_input::ArtsInputScreen;
use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};

fn disc_path() -> Option<PathBuf> {
    let p = std::env::var_os("LEGAIA_DISC_BIN")?;
    let p = PathBuf::from(p);
    p.is_file().then_some(p)
}

/// Stage Vahn's Somersault (action `0x27`) as a one-command art record so a
/// flat `Up` chain matches it and the Arts row carries the real action
/// constant.
fn stage_somersault(w: &mut World) {
    let action = legaia_art::ActionConstant::from_byte(0x27).unwrap();
    let rec = legaia_art::ArtRecord {
        action,
        commands: vec![legaia_art::Command::Up],
        anim_index: 0,
        anim_extra: vec![],
        name: None,
        power: vec![legaia_art::power::PowerByte::from_byte(0x16); 2],
        dmg_timing: vec![],
        effect_cues: Default::default(),
        hit_cues: vec![],
        identifier: 0,
        anim_speed: 0,
        enemy_effect: legaia_art::EnemyEffect::None,
        repeat_frames: Default::default(),
        background: 0,
        runtime_address: None,
    };
    w.set_art_record(legaia_art::Character::Vahn, action, rec);
}

/// A live-loop world one walk-step away from a scripted encounter. Mirrors
/// `engine-core/tests/battle_shout_cue.rs`'s setup.
fn build_world(with_record: bool) -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 90);
    }
    w.load_party(legaia_save::Party::zeroed(3));
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    if with_record {
        stage_somersault(&mut w);
    }

    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.field_camera_azimuth = 0;

    use legaia_engine_core::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("arts_shout_test");
    table.set_trigger_rate(0xFF);
    table.push(EncounterEntry::new(1, 1));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 2;
    session.grace_frames = 2;
    w.set_encounter_session(Some(session));

    w.mode = SceneMode::Field;
    w.live_gameplay_loop = true;
    w.battle_player_driven = true;
    w
}

/// Walk into the battle and type one art through the live Arts command input:
/// command menu → Arts → one `Up` press → Cross to end the entry → Begin →
/// target. Shout cues are drained every tick (they must be captured before the
/// battle teardown clears the queue). Later party turns fall back to Attack so
/// the cue list measures exactly one arts entry. Returns the accumulated cues.
///
/// This is the pad path a player walks: under the retail model the Arts
/// command opens a per-press directional entry, so a driver that only ever
/// presses Cross never reaches an executed art.
fn drive_art_and_collect_shouts(
    w: &mut World,
) -> Vec<legaia_engine_core::battle_events::BattleShoutCue> {
    use legaia_engine_core::battle_input::BattleCommand;

    let up = InputState::mask_of([PadButton::Up]);
    let mut entered = false;
    for _ in 0..6000 {
        w.set_pad(up);
        w.tick();
        if w.mode == SceneMode::Battle {
            entered = true;
            break;
        }
    }
    assert!(entered, "walking should trigger Field -> Battle");

    // Somersault's whole command string is one `Up`.
    let combo = [PadButton::Up];
    let mut next_dir = 0usize;
    let mut shouts = Vec::new();
    let mut press = true;
    let mut opened_input = false;
    let mut arts_turns = 0usize;
    for _ in 0..4000 {
        let pad = if !press {
            0
        } else if let Some(view) = w.arts_input_view() {
            opened_input = true;
            match view.phase {
                ArtsInputScreen::Entering => {
                    if next_dir < combo.len() {
                        let dir = combo[next_dir];
                        next_dir += 1;
                        InputState::mask_of([dir])
                    } else {
                        InputState::mask_of([PadButton::Cross])
                    }
                }
                // Review → Begin|Reselect (cursor 0 = Begin) → target picker.
                _ => InputState::mask_of([PadButton::Cross]),
            }
        } else if let Some(cmd) = w.battle_command.as_ref() {
            // Retail's open flow: `Begin` on the round prompt, the ring's
            // `Attack` arm, then the `Auto | Command` prompt - `Command` is
            // the directional arts entry, `Auto` the plain swing.
            use legaia_engine_core::battle_input::{AttackMode, CommandPhase};
            match cmd.phase {
                CommandPhase::Menu { .. } if cmd.menu_command() != Some(BattleCommand::Attack) => {
                    InputState::mask_of([PadButton::Down])
                }
                CommandPhase::AttackMode { .. } => {
                    let want = if arts_turns == 0 {
                        AttackMode::Command
                    } else {
                        AttackMode::Auto
                    };
                    if cmd.attack_mode() == Some(want) {
                        InputState::mask_of([PadButton::Cross])
                    } else {
                        InputState::mask_of([PadButton::Right])
                    }
                }
                _ => InputState::mask_of([PadButton::Cross]),
            }
        } else {
            0
        };
        let input_was_open = w.arts_input_active();
        w.set_pad(pad);
        press = !press;
        w.tick();
        if input_was_open && !w.arts_input_active() {
            arts_turns += 1;
        }
        shouts.extend(w.drain_battle_shout_cues());
        if w.mode == SceneMode::Field && w.last_battle_rewards.is_some() {
            break;
        }
    }
    assert!(
        opened_input,
        "the Arts command must open the per-press input session"
    );
    assert_eq!(arts_turns, 1, "exactly one arts entry was driven");
    // Non-vacuity: an aborted entry (or a spun-out loop) would leave the world
    // in Battle, and the shout-free baseline would pass for the wrong reason.
    assert_eq!(w.mode, SceneMode::Field, "the battle resolved");
    assert!(
        w.last_battle_rewards.is_some(),
        "the party won, so an art actually landed"
    );
    shouts
}

#[test]
fn art_shout_pcm_lands_in_the_mix_after_the_cd_delay() {
    let Some(disc) = disc_path() else {
        eprintln!("LEGAIA_DISC_BIN not set - skipping");
        return;
    };

    // --- The bank the boot path stages, from the real disc. ---
    let mut bank = legaia_engine_shell::boot::read_arts_shout_bank(&disc)
        .expect("arts-voice shout bank stages from the disc");
    let pool: Vec<u8> = bank
        .pool(0, 0x27)
        .expect("Vahn Somersault (0x27) has an arts-voice pool")
        .to_vec();
    assert!(!pool.is_empty() && pool.iter().all(|&c| c < 16));
    // Capture-verified anchor: the retail trace fired Somersault on XA2
    // channels 0 and 6.
    assert!(
        pool.contains(&0) && pool.contains(&6),
        "Somersault pool carries the capture-verified channels 0+6: {pool:?}"
    );

    // --- Drive the art through the live battle session. ---
    let mut w = build_world(true);
    let shouts = drive_art_and_collect_shouts(&mut w);
    assert_eq!(
        shouts.len(),
        1,
        "exactly one shout cue per executed art: {shouts:?}"
    );
    assert_eq!(shouts[0].cslot, 0, "Vahn's clip file (XA2)");
    assert_eq!(shouts[0].action, 0x27, "keyed on the art's action constant");

    // --- Resolve the cue and mix it the way the director does. ---
    let (channel, clip) = bank
        .shout(shouts[0].cslot, shouts[0].action)
        .expect("cue resolves to a decoded clip");
    assert!(
        pool.contains(&channel),
        "picked channel comes from the pool"
    );
    assert!(!clip.pcm.is_empty(), "decoded shout PCM is non-empty");
    assert!(
        clip.pcm.iter().any(|&s| s.unsigned_abs() > 500),
        "shout clip is audibly non-silent"
    );
    let clip_rate = clip.sample_rate;
    let clip_pcm = clip.pcm.clone();

    let delay = legaia_engine_audio::SHOUT_CD_RESPONSE_DELAY;
    let mut mix = legaia_engine_audio::OfflineMixer::new(44_100);
    mix.play_xa_shout(
        clip_pcm,
        clip_rate,
        legaia_xa::Channels::Mono,
        0x4000,
        delay,
    );
    // Silence during the modeled CD-response window: the shout must trail
    // the (animation-start) request, never lead it. The resampler's
    // two-sample interpolation window adds one frame of latency, so probe
    // strictly inside the delay.
    let mut peak_during_delay = 0u16;
    for _ in 0..delay.saturating_sub(2) {
        let (l, r) = mix.next_frame();
        peak_during_delay = peak_during_delay
            .max(l.unsigned_abs())
            .max(r.unsigned_abs());
    }
    assert_eq!(
        peak_during_delay, 0,
        "no shout PCM may sound before the CD-response delay elapses"
    );
    // ...then the shout PCM lands in the mix (probe a 1 s window).
    let mut peak_after = 0u16;
    for _ in 0..44_100 {
        let (l, r) = mix.next_frame();
        peak_after = peak_after.max(l.unsigned_abs()).max(r.unsigned_abs());
    }
    assert!(
        peak_after > 500,
        "shout PCM reaches the mix after the delay (peak {peak_after})"
    );

    // --- Baseline: no staged record -> synthetic silent art, no cue. ---
    let mut w = build_world(false);
    let shouts = drive_art_and_collect_shouts(&mut w);
    assert!(
        shouts.is_empty(),
        "a synthetic art without a matched record stays silent: {shouts:?}"
    );
}
