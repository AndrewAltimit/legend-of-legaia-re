//! Disc-gated: the arts voice of a real battle, cue by cue and in the mix.
//!
//! The fight is Rim Elm's sparring tutorial (`town01`, formation
//! [`RIM_ELM_TRAINING_FORMATION_ID`]), driven by pad presses through its four
//! lessons; the fourth is Vahn's Somersault, `Up Down Up`, his **first** use
//! of the art. The session is booted the way both play hosts boot it: the
//! disc's `SCUS_942.54` through `BootSession::from_host` (which installs the
//! arts-voice selector's tables) and the native boot's XA banks.
//!
//! What the anim commit `FUN_8004AD80` voices, in order
//! (`docs/subsystems/battle-action-helpers.md`, "Battle voice cues"):
//!
//! 1. the Items lesson's cast voice and the Spirit lesson's charge line
//!    (`FUN_801F3990` / the cue track - not this lane's subject, pinned so
//!    the sequence is whole);
//! 2. the Somersault's `0x1A` starter commit - a first use, so the build
//!    loop's starter mark selects the generic line, `0x101` = `XA1.XA`
//!    channel 1;
//! 3. the Somersault clip's own commit, under the solo latch the starter's
//!    entry raised (`ctx[+0x243]`): the arts-voice selector `FUN_8004C140`,
//!    resolved by the world - `XA2.XA` (Vahn), and channel `0xC` because the
//!    first formation id is Tetsu's `0x4F`.
//!
//! The mix half runs the same session twice, once with the XA banks staged
//! and once without, and differences the two outputs: nothing else in the
//! session depends on the banks, so the difference is exactly the CD-XA
//! voice - silent before the first clip, and present after each.
//!
//! Skip-passes without `LEGAIA_DISC_BIN`.

use std::path::{Path, PathBuf};

use legaia_engine_audio::{SPU_INTERNAL_RATE, TestAudioSink};
use legaia_engine_battle::arts_command_input::ArtsInputPhase;
use legaia_engine_battle::battle_tutorial::{TUTORIAL_ARM_FLAG, TutorialLesson};
use legaia_engine_battle_menus::battle_input::CommandPhase;
use legaia_engine_menus::inventory_use::InventoryUseState;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_session::BootSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;
use legaia_scene_formats::encounter_record::RIM_ELM_TRAINING_FORMATION_ID;

type Session = BootSession<TestAudioSink>;

fn disc_path() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then_some(p)
}

fn open(disc: &Path, voiced: bool) -> Session {
    use legaia_engine_scene::vfs::Vfs;
    let host = legaia_engine_scene_host::scene_host::SceneHost::open_disc(disc).expect("open disc");
    let scus = legaia_engine_scene::vfs::DiscVfs::open(disc)
        .expect("disc vfs")
        .read("SCUS_942.54")
        .expect("SCUS_942.54");
    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: true,
    };
    let mut s = BootSession::from_host(host, Some(&scus), &cfg, || {
        Ok(TestAudioSink::new(SPU_INTERNAL_RATE))
    })
    .expect("boot session");
    assert!(
        s.host.world.audio.arts_voice_tables.is_some(),
        "the boot installs the selector's tables"
    );
    if voiced {
        let d = s.bgm.as_mut().expect("audio enabled");
        d.set_shout_bank(legaia_engine_session::native::read_arts_shout_bank(disc).unwrap());
        d.set_xa_clip_bank(legaia_engine_session::native::read_battle_xa_clip_bank(disc).unwrap());
        assert!(d.set_xa_lazy_source(disc) > 0);
    }
    s.enter_field_live(
        "town01",
        &FieldLiveOpts {
            live_loop: true,
            ..Default::default()
        },
    )
    .expect("enter field live");
    let w = &mut s.host.world;
    w.battle.player_driven = true;
    w.system_flag_set(TUTORIAL_ARM_FLAG);
    w.system_flag_clear(1);
    assert_eq!(
        w.install_man_formation(RIM_ELM_TRAINING_FORMATION_ID),
        Some(RIM_ELM_TRAINING_FORMATION_ID)
    );
    assert!(w.on_field_step(), "the scripted formation triggers");
    for _ in 0..600 {
        s.tick().expect("tick");
        if s.host.world.mode == SceneMode::Battle {
            break;
        }
    }
    assert_eq!(s.host.world.mode, SceneMode::Battle);
    s
}

/// The press a player makes this frame (the spar test's hand, Somersault
/// drill only).
fn hand(s: &Session, entry_open: &mut bool) -> u16 {
    const DRILL: [PadButton; 3] = [PadButton::Up, PadButton::Down, PadButton::Up];
    let w = &s.host.world;
    if !w.battle.tutorial_boxes.is_empty() {
        return PadButton::Cross.mask();
    }
    let lesson = w.battle.tutorial.as_ref().map(|t| t.lesson());
    if let Some(menu) = w.battle.item_menu.as_ref() {
        return match &menu.state {
            InventoryUseState::Browsing { .. } if !menu.filtered_items.is_empty() => {
                PadButton::Cross.mask()
            }
            InventoryUseState::TargetSelect { .. } => PadButton::Cross.mask(),
            _ => PadButton::Circle.mask(),
        };
    }
    if let Some(arts) = w.battle.arts_input.as_ref() {
        *entry_open = true;
        return match &arts.phase {
            ArtsInputPhase::Entering if arts.buffer.len() < DRILL.len() => {
                DRILL[arts.buffer.len()].mask()
            }
            _ => PadButton::Cross.mask(),
        };
    }
    *entry_open = false;
    if let Some(cmd) = w.battle.command.as_ref() {
        return match &cmd.phase {
            CommandPhase::Menu { .. } if lesson == Some(TutorialLesson::Items) => {
                PadButton::Up.mask()
            }
            CommandPhase::Menu { .. } if lesson == Some(TutorialLesson::Spirit) => {
                PadButton::Down.mask()
            }
            CommandPhase::AttackMode { .. } => {
                if lesson == Some(TutorialLesson::HyperArts) {
                    PadButton::Right.mask()
                } else {
                    PadButton::Left.mask()
                }
            }
            CommandPhase::RoundPrompt { .. }
            | CommandPhase::Menu { .. }
            | CommandPhase::CommitConfirm { .. } => PadButton::Left.mask(),
            CommandPhase::Targeting { .. } => PadButton::Cross.mask(),
            _ => 0,
        };
    }
    PadButton::Cross.mask()
}

struct Run {
    /// `(tick, clip slot, channel, dur)` of every battle CD-XA request.
    clips: Vec<(u32, u32, u32, u32)>,
    /// Every arts-voice selector call: `(tick, cslot, action, resolved)`.
    shouts: Vec<(u32, u8, u8, bool)>,
    /// The mixed output, one video frame of left samples per tick.
    pcm: Vec<Vec<i16>>,
    clips_fired: u32,
}

fn play(disc: &Path, voiced: bool) -> Run {
    let mut s = open(disc, voiced);
    let mut hud = legaia_engine_battle::battle_hud_state::BattleHud::new();
    let mut banner = None;
    let (mut prev, mut entry_open) = (0u16, false);
    let mut run = Run {
        clips: Vec::new(),
        shouts: Vec::new(),
        pcm: Vec::new(),
        clips_fired: 0,
    };
    let mut tick = 0u32;
    while s.host.world.mode == SceneMode::Battle && tick < 40_000 {
        let want = hand(&s, &mut entry_open);
        let pad = if prev == 0 { want } else { 0 };
        prev = pad;
        s.host.world.set_pad(pad);
        s.tick().expect("tick");
        tick += 1;
        let w = &s.host.world;
        run.clips.extend(
            w.audio
                .battle_xa_cues
                .iter()
                .map(|c| (tick, c.clip, c.channel, c.duration_sectors)),
        );
        run.shouts.extend(
            w.audio
                .battle_shout_cues
                .iter()
                .map(|c| (tick, c.cslot, c.action, c.resolved)),
        );
        // The host's battle-event step and audio frame, as both play hosts
        // run them.
        let _ = s.step_battle_presentation(&mut hud, &mut banner);
        let _ = s.step_audio_frame();
        let sink = s.bgm.as_ref().expect("audio").audio().clone();
        run.pcm.push(
            (0..sink.frames_per_video_frame())
                .map(|_| sink.next_frame().0)
                .collect(),
        );
    }
    assert_eq!(
        s.host.world.mode,
        SceneMode::Field,
        "the spar closes back to the field after the Somersault ({tick} ticks)"
    );
    run.clips_fired = s.bgm.as_ref().unwrap().xa_stats().clips_fired;
    run
}

#[test]
fn spar_somersault_voices_the_starter_line_then_the_shout_and_both_reach_the_mix() {
    let Some(disc) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let voiced = play(&disc, true);
    eprintln!("[ran] clips {:?}", voiced.clips);
    eprintln!("[ran] shouts {:?}", voiced.shouts);

    // --- The cue sequence. ---
    let seq: Vec<(u32, u32)> = voiced.clips.iter().map(|c| (c.1, c.2)).collect();
    assert_eq!(
        seq,
        vec![(26, 0), (0, 0), (0, 1), (1, 0x0C)],
        "item voice, Spirit line, the NEW ARTS starter line (XA1 ch1), the shout (XA2 ch 0xC)"
    );
    let starter = voiced.clips[2];
    let shout = voiced.clips[3];
    assert!(
        shout.0 > starter.0,
        "the shout follows the starter's commit"
    );
    // `dur = (dur_table[0 * 0x10 + 0xC] * 0x3C + 99) / 100`, off the disc.
    let tables = legaia_engine_scene::vfs::DiscVfs::open(&disc)
        .ok()
        .and_then(|v| {
            use legaia_engine_scene::vfs::Vfs;
            v.read("SCUS_942.54").ok()
        })
        .and_then(|scus| legaia_art::arts_voice::ArtsVoiceSelectorTables::parse_from_scus(&scus))
        .expect("selector tables");
    let raw = u32::from(tables.byte(legaia_art::arts_voice::DUR_TABLE_VA + 0x0C));
    assert_eq!(shout.3, (raw * 0x3C).div_ceil(100), "the shout's read span");
    assert_eq!(
        voiced.shouts,
        vec![(shout.0, 0, 0x27, true)],
        "one selector call, on the Somersault's commit, resolved by the world"
    );
    assert_eq!(voiced.clips_fired, 4, "every request reached the mixer");

    // --- The mix: voiced minus silent is the CD-XA voice. ---
    let silent = play(&disc, false);
    assert_eq!(
        silent
            .clips
            .iter()
            .map(|c| (c.0, c.1, c.2))
            .collect::<Vec<_>>(),
        voiced
            .clips
            .iter()
            .map(|c| (c.0, c.1, c.2))
            .collect::<Vec<_>>(),
        "the banks change no gameplay: the same requests on the same ticks"
    );
    assert_eq!(silent.clips_fired, 0, "nothing is staged, nothing plays");
    let diff = |t: u32| -> u64 {
        let (a, b) = (&voiced.pcm[t as usize - 1], &silent.pcm[t as usize - 1]);
        a.iter()
            .zip(b)
            .map(|(x, y)| u64::from((i32::from(*x) - i32::from(*y)).unsigned_abs()))
            .sum()
    };
    let first = voiced.clips[0].0;
    assert!(
        (1..first).all(|t| diff(t) == 0),
        "no XA difference before the first clip request"
    );
    // Each request takes the one drive: whatever was sounding stops on the
    // request frame (the shout cuts the NEW ARTS line short), the CD-response delay (`SHOUT_CD_RESPONSE_DELAY`, 6615
    // samples - eight whole 735-sample ticks) holds the new clip silent, and
    // then it plays.
    let delay_ticks = (legaia_engine_audio::SHOUT_CD_RESPONSE_DELAY as usize / 735) as u32;
    for &(t, slot, ch, _) in &voiced.clips {
        let held: u64 = (t..t + delay_ticks).map(diff).sum();
        let energy: u64 = (t + delay_ticks + 1..t + 45).map(diff).sum();
        eprintln!(
            "[mix] XA{} ch {ch} at tick {t}: held {held}, then |voiced - silent| = {energy}",
            slot + 1
        );
        // A clip cut on the request leaves the resampler's two-sample
        // interpolation history, a few units of PCM.
        assert!(
            held < 1_000,
            "XA{} channel {ch}: silent through the CD delay ({held})",
            slot + 1
        );
        assert!(
            energy > 100_000,
            "XA{} channel {ch} reaches the mix within 0.75 s of its request",
            slot + 1
        );
    }
}
