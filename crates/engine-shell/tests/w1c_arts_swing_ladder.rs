//! Arts-swing ladder: the three side-band layers a **swung art** drives that
//! no canonical replay ladder's coverage ever joined.
//!
//! `docs/tooling/reach-triage.md` proposes this ladder as "an art that swings:
//! shout bank, XA clip, face stamps". The three rows are in three different
//! crates and only one of them is reached by *driving* anything:
//!
//! | layer | routine | how it is reached |
//! |---|---|---|
//! | arts-voice shout | `FUN_8004C140` (`legaia_engine_audio::shout`) | the executed art's `BattleShoutCue` resolved against the disc-built bank |
//! | face stamps | `FUN_8004C7B4` (`legaia_asset::face_anim`) | the swung clip's frame counter, with and without the victory-window mouth override |
//! | XA clip start | `FUN_8003D53C` (`legaia_engine_shell::xa_clip`) | the voice-cue census the `xa-cue` CLI runs |
//!
//! The third is **not** on the shout's path and this ladder does not pretend
//! it is: `FUN_8004C140` fires the clip starter with a channel from a runtime
//! candidate pool, while `FUN_8004FCC8`'s `(id - 0x100)` arithmetic is the
//! *menu voice* dispatcher, and the engine has no streamed-voice device for
//! it at all (see the `xa_clip` module docs). What the ladder can do honestly
//! is run the census that library entry point exists for, against the real
//! disc, which is exactly what its one production caller does.
//!
//! The art under test is Vahn's Somersault (action constant `0x27`) - the
//! capture-verified arts-voice anchor - typed through the retail per-press
//! Arts command input as its own one-press command string.
//!
//! Coverage export (what wires this into the reach report):
//!
//! ```text
//! cargo llvm-cov -p legaia-engine-shell --test w1c_arts_swing_ladder \
//!     --json --output-path target/cov-w1c_arts_swing_ladder.json
//! ```
//!
//! **No `--release`.** An optimised build inlines the small kernels and
//! leaves their out-of-line coverage records at zero, which the reach
//! report cannot tell from "never called".
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset. CI runs without disc data.

use std::path::PathBuf;

use legaia_engine_core::arts_command_input::ArtsInputScreen;
use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};

/// Vahn's Somersault. The pool anchor a live retail trace pinned.
const SOMERSAULT: u8 = 0x27;

fn disc_path() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then_some(p)
}

// ---------------------------------------------------------------------------
// The swing
// ---------------------------------------------------------------------------

/// A live-loop world one walk-step away from a scripted encounter, with
/// Somersault staged as a one-command art record so a flat `Up` chain matches
/// it and the Arts row carries the real action constant.
fn world_with_somersault() -> World {
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

    let action = legaia_art::ActionConstant::from_byte(SOMERSAULT).expect("0x27 is an action");
    w.set_art_record(
        legaia_art::Character::Vahn,
        action,
        legaia_art::ArtRecord {
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
        },
    );

    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.field_camera_azimuth = 0;

    use legaia_engine_core::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("w1c_arts_swing");
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

/// What the driven swing observed, per frame it was live.
struct Swing {
    shouts: Vec<legaia_engine_core::battle_events::BattleShoutCue>,
    /// Every `(staged action id, clip frame)` the swinging member's battle
    /// animation passed through - the exact pair the retail facial animator
    /// is handed each frame (`FUN_80047430` -> `FUN_8004C7B4`).
    clip_frames: Vec<(u8, i16)>,
}

/// Walk into the fight and type one art: command ring -> `Attack` -> the
/// `Command` chip (the directional entry) -> `Up` -> Cross, then the target.
/// Later party turns fall back to `Auto` so exactly one arts entry is driven.
fn drive_the_swing(w: &mut World) -> Swing {
    use legaia_engine_core::battle_input::{AttackMode, BattleCommand, CommandPhase};

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
    assert!(entered, "walking must trigger Field -> Battle");

    let mut out = Swing {
        shouts: Vec::new(),
        clip_frames: Vec::new(),
    };
    let combo = [PadButton::Up];
    let mut next_dir = 0usize;
    let mut press = true;
    let mut opened_input = false;
    let mut arts_turns = 0usize;
    for _ in 0..4000 {
        let pad = if !press {
            0
        } else if let Some(view) = w.arts_input_view() {
            opened_input = true;
            match view.phase {
                ArtsInputScreen::Entering if next_dir < combo.len() => {
                    let dir = combo[next_dir];
                    next_dir += 1;
                    InputState::mask_of([dir])
                }
                _ => InputState::mask_of([PadButton::Cross]),
            }
        } else if let Some(cmd) = w.battle_command.as_ref() {
            match cmd.phase {
                CommandPhase::Menu { .. } if cmd.menu_command() != Some(BattleCommand::Attack) => {
                    InputState::mask_of([PadButton::Left])
                }
                CommandPhase::AttackMode { .. } => {
                    let want = if arts_turns == 0 {
                        AttackMode::Command
                    } else {
                        AttackMode::Auto
                    };
                    if cmd.attack_mode() == Some(want) {
                        InputState::mask_of([PadButton::Cross])
                    } else if want == AttackMode::Auto {
                        InputState::mask_of([PadButton::Left])
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
        out.shouts.extend(w.drain_battle_shout_cues());
        if let Some(p) = w.actors.first().and_then(|a| a.battle_animation.as_ref()) {
            out.clip_frames.push((p.action_id(), p.current_frame()));
        }
        if w.mode == SceneMode::Field && w.last_battle_rewards.is_some() {
            break;
        }
    }
    assert!(
        opened_input,
        "the Arts command must open the per-press input"
    );
    assert_eq!(arts_turns, 1, "exactly one arts entry was driven");
    assert_eq!(w.mode, SceneMode::Field, "the battle resolved");
    out
}

// ---------------------------------------------------------------------------
// The ladder
// ---------------------------------------------------------------------------

#[test]
fn w1c_arts_swing_ladder() {
    let Some(disc) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };

    // --- rung 1: the swing itself. -----------------------------------------
    let mut w = world_with_somersault();
    let swing = drive_the_swing(&mut w);
    assert_eq!(
        swing.shouts.len(),
        1,
        "exactly one shout cue per executed art: {:?}",
        swing.shouts
    );
    assert_eq!(swing.shouts[0].cslot, 0, "Vahn's clip file (XA2)");
    assert_eq!(swing.shouts[0].action, SOMERSAULT);
    // The clip frames are **not** asserted, and the reason is a host boundary
    // worth recording rather than papering over: `Actor::battle_animation` is
    // installed by the *rendering* host when it assembles the member's battle
    // mesh (`assembled_party_battle_mesh`), so a headless `World` swings an
    // art with no animation player attached and the facial animator has no
    // frame source at all here. Rung 3 therefore drives `FUN_8004C7B4` over
    // the whole clamped frame domain instead of over the frames this swing
    // happened to reach.
    eprintln!(
        "[rung 1] swing: shout cue {:?}, {} frames carried an animation player",
        swing.shouts[0],
        swing.clip_frames.len()
    );

    // --- rung 2: the shout bank's channel pick (FUN_8004C140). -------------
    let mut bank = legaia_engine_shell::boot::read_arts_shout_bank(&disc)
        .expect("arts-voice shout bank stages from the disc");
    let pool: Vec<u8> = bank
        .pool(swing.shouts[0].cslot, swing.shouts[0].action)
        .expect("the swung art has an arts-voice pool")
        .to_vec();
    assert!(!pool.is_empty() && pool.iter().all(|&c| c < 16));

    // The selector's stated intent is a **no-immediate-repeat** pick over the
    // pool, not a fixed channel: firing the same art twice in a row must
    // change channel whenever the pool has an alternative, and every pick
    // must come from the pool. A selector that ignored the pool, or one that
    // ignored the repeat state, both look identical from a single call.
    let mut picks = Vec::new();
    for _ in 0..6 {
        let ch = bank
            .pick_channel(swing.shouts[0].cslot, swing.shouts[0].action)
            .expect("a voiced art resolves a channel");
        assert!(pool.contains(&ch), "pick {ch} is outside the pool {pool:?}");
        picks.push(ch);
    }
    if pool.len() > 1 {
        assert!(
            picks.windows(2).all(|p| p[0] != p[1]),
            "back-to-back picks must not repeat a channel: {picks:?} over pool {pool:?}"
        );
    }
    let (channel, clip) = bank
        .shout(swing.shouts[0].cslot, swing.shouts[0].action)
        .expect("the cue resolves to a decoded clip");
    assert!(pool.contains(&channel));
    assert!(
        clip.pcm.iter().any(|&s| s.unsigned_abs() > 500),
        "the resolved shout clip is audibly non-silent"
    );
    // An unvoiced action stays silent rather than borrowing a neighbour's
    // pool - the negative half of the same selector.
    assert!(
        bank.pool(swing.shouts[0].cslot, 0xFF).is_none(),
        "an action with no arts-voice row must have no pool"
    );
    eprintln!("[rung 2] shout: pool {pool:?}, picks {picks:?}");

    // --- rung 3: the facial animator (FUN_8004C7B4). -----------------------
    use legaia_asset::face_anim::{
        ART_BAND_FIRST, ART_BAND_LAST, ArtMouthOverride, ArtMouthTables, FaceFrameTables,
        battle_face_tracks,
    };
    use legaia_engine_core::Vfs;
    let vfs = legaia_engine_core::DiscVfs::open(&disc).expect("open disc");
    let scus = vfs.read("SCUS_942.54").expect("read SCUS");
    let frames = FaceFrameTables::from_scus(&scus).expect("face-frame tables parse");
    let art_tables = ArtMouthTables::from_scus(&scus).expect("art-mouth override table parses");

    // Vahn's per-action entry tracks, straight out of his player battle file.
    let prot = vfs.read("PROT.DAT").expect("read PROT.DAT");
    let index = legaia_engine_core::scene::ProtIndex::from_bytes(prot, None).expect("PROT index");
    let vahn = index
        .entry_bytes_extended(863)
        .expect("Vahn's player battle file (PROT 863)");
    let entry_tracks = battle_face_tracks(&vahn).expect("decode Vahn's face tracks");

    // The neutral pass - what a member with no active record on either track
    // stamps. Everything below is measured as a *departure* from it, because
    // "how many stamps came out" is not the observable: one active mouth
    // record plus a neutral eye fallback is two stamps, and so is the wholly
    // neutral pass. Only the rects differ.
    let neutral = frames.stamps_with_art_window(0, 0, None, 0, None, false);
    assert_eq!(neutral.len(), 2, "the neutral pass is one mouth + one eye");

    // Every stamp pass emits at least a mouth and an eye: both passes fall
    // back to the neutral frame when no record is active, so no frame can
    // leave a member's face rows unwritten. That is what the two neutral
    // re-stamps exist for, and it is what a "stamp only while a record is
    // active" reading would silently break.
    let mut animated: Vec<(usize, usize)> = Vec::new();
    for (slot, tracks) in entry_tracks.iter().enumerate() {
        let Some(tracks) = tracks else { continue };
        let mut moved = 0usize;
        for f in 0..=0xFFi16 {
            let stamps = frames.stamps_with_art_window(0, 0, Some(tracks), f, None, false);
            assert!(
                stamps.len() >= 2,
                "entry {slot} frame {f}: {} stamps (need a mouth + an eye)",
                stamps.len()
            );
            if stamps != neutral {
                moved += 1;
            }
        }
        if moved > 0 {
            animated.push((slot, moved));
        }
        // The **idle** entry is the load-bearing negative: retail's resting
        // party faces are the re-stamped neutral frames, so slot 0 must
        // depart from neutral on no frame at all.
        if slot == 0 {
            assert_eq!(
                moved, 0,
                "the idle entry must stamp the neutral face on every frame"
            );
        }
    }
    assert!(
        !animated.is_empty(),
        "no Vahn action entry moved a face off the neutral frame"
    );
    let voiced = (
        animated[0].0,
        entry_tracks[animated[0].0].expect("checked above"),
    );
    let with_records = animated[0].1;

    // The frame counter CLAMPS at 0xFE rather than wrapping, so every frame
    // past it stamps what 0xFE stamps.
    let at_clamp = frames.stamps_with_art_window(0, 0, Some(&voiced.1), 0xFE, None, false);
    for f in [0xFFi16, 0x1FF, i16::MAX] {
        assert_eq!(
            frames.stamps_with_art_window(0, 0, Some(&voiced.1), f, None, false),
            at_clamp,
            "clip frame {f} must clamp onto 0xFE"
        );
    }

    // The victory-window override: the mouth records come from the static
    // `0x80077E80` band track and the counter for BOTH passes becomes the
    // victory counter halved - so an override at counter `2n` must stamp the
    // same eyes as a plain pass at clip frame `n`, whatever the clip frame
    // handed in. Retail replaces the counter local before the clamp, which is
    // why the eye pass clocks on it too; a port that only re-sourced the
    // mouth would pass every "does the mouth change" check and fail this one.
    let mut band_checked = 0usize;
    for band in ART_BAND_FIRST..=ART_BAND_LAST {
        let Some(track) = art_tables.track(0, band) else {
            continue;
        };
        for n in [0i16, 3, 9, 40] {
            let over = frames.stamps_with_art_window(
                0,
                0,
                Some(&voiced.1),
                0x7F,
                Some(ArtMouthOverride {
                    track,
                    counter: (n as u16) * 2,
                }),
                false,
            );
            let plain = frames.stamps_with_art_window(0, 0, Some(&voiced.1), n, None, false);
            let eyes = |v: &[legaia_asset::face_anim::FaceStamp]| v.last().copied();
            assert_eq!(
                eyes(&over),
                eyes(&plain),
                "band {band:#04x} counter {}: the override must reclock the EYE pass too",
                n * 2
            );
            band_checked += 1;
        }
    }
    assert!(band_checked > 0, "no art band carried an override track");

    // `force_neutral_mouth` (character-record `+0xF8` bit 0x2000) re-stamps
    // the neutral mouth on top of whatever was active, so the pass can only
    // grow.
    for f in [0i16, 0x20, 0x60] {
        let plain = frames.stamps_with_art_window(0, 0, Some(&voiced.1), f, None, false);
        let forced = frames.stamps_with_art_window(0, 0, Some(&voiced.1), f, None, true);
        assert!(
            forced.len() >= plain.len(),
            "frame {f}: the neutral re-stamp removed a stamp"
        );
    }
    eprintln!(
        "[rung 3] face: entry slot {} live on {with_records} of 256 clip frames, \
         {band_checked} override probes",
        voiced.0
    );

    // --- rung 4: the XA clip census (FUN_8003D53C). ------------------------
    use legaia_engine_shell::xa_clip::{
        MAX_CLIP_DURATION_SECTORS, clip_duration_is_clamped, clip_end_lba_offset, report_cues,
        voice_clip_duration_sectors,
    };
    // The end-LBA offset is monotone in the duration and saturates exactly at
    // the starter's own clamp - the two properties the drive's stop poll
    // depends on (`CdlGetlocP` against `start_lba + offset`).
    let mut prev = 0u32;
    for d in (0..=MAX_CLIP_DURATION_SECTORS).step_by(97) {
        let off = clip_end_lba_offset(d);
        assert!(off >= prev, "end offset went backwards at duration {d}");
        prev = off;
        assert!(!clip_duration_is_clamped(d));
    }
    let capped = clip_end_lba_offset(MAX_CLIP_DURATION_SECTORS);
    for over in [1u32, 1000, u32::MAX / 2] {
        let d = MAX_CLIP_DURATION_SECTORS + over;
        assert!(clip_duration_is_clamped(d));
        assert_eq!(
            clip_end_lba_offset(d),
            capped,
            "a duration past the clamp must reuse the clamped offset"
        );
    }
    // The census the `xa-cue` CLI runs, over the whole voice-cue byte and one
    // id below the base (the SFX-queue path). `xa_dir` is left `None`: the
    // extracted tree is not a disc-gated prerequisite.
    let ids: Vec<u32> = (0x100u32..0x130).chain(std::iter::once(0x40)).collect();
    let report = report_cues(&ids, 200, None);
    assert!(
        report.lines().count() > ids.len(),
        "the census must print a row per cue plus its header"
    );
    assert!(
        report.contains("(SFX queue)"),
        "a sub-0x100 id must be reported as the SFX-queue path, not as a clip"
    );
    assert_eq!(voice_clip_duration_sectors(200), 120);
    eprintln!("[rung 4] xa-cue census: {} rows", report.lines().count());

    eprintln!("w1c_arts_swing_ladder: 4/4 rungs cleared");
}
