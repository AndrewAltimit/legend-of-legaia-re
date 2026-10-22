//! Disc-gated: what the ring's `Attack` arm opens, on the real town01 fight,
//! with and without the sparring tutorial (overlay 967) running.
//!
//! `Attack` is the door to the `Auto` | `Command` attack-mode prompt (retail
//! flow state `0x78`), not to the arts entry: with the "Select Attack" option
//! on its default `Select`, the ring's Left arm stores `0x78` (`FUN_801D0748`
//! `0x801D1604`). On that prompt only the Right arm reaches the directional
//! arts entry (`andi v0,s2,0x2000` at `0x801D1728` -> `0x50`); the Left arm
//! **and** the confirm mask both take `Auto` (`0x801D1774..0x801D1794` ->
//! `0x5A`, the target cursor), so a player who presses Cross through the
//! battle - or a driver that presses Left - commits a plain attack and never
//! sees the arrows. The same fall-through makes the ring's confirm mask take
//! `Attack` (`0x801D152C..0x801D1548`).
//!
//! The tutorial's `0x78` hook (`0x801F6D30`) only queues boxes - free choice
//! in the attacks lesson, a forced `[Command]` in the hyper-arts drill - so
//! the spar's first lesson walks the same three surfaces as any fight.
//!
//! Every input is a pad press through the session tick both play hosts run.
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_battle::battle_tutorial::TUTORIAL_ARM_FLAG;
use legaia_engine_battle_menus::battle_flow::BattleFlowState;
use legaia_engine_battle_menus::battle_input::CommandPhase;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;
use legaia_scene_formats::encounter_record::RIM_ELM_TRAINING_FORMATION_ID;

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
    eprintln!("[skip] extracted/ missing");
    None
}

/// The town01 Tetsu formation, entered as the spar (`tutorial`) or, with the
/// disc's one-shot arm cleared, as an ordinary player-driven fight.
fn open_fight(tutorial: bool) -> Option<NativeSession> {
    let extracted = extracted_dir()?;
    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(&extracted, &cfg).expect("open boot session");
    session
        .enter_field_live(
            "town01",
            &FieldLiveOpts {
                live_loop: true,
                ..Default::default()
            },
        )
        .expect("enter field live");
    let w = &mut session.host.world;
    w.battle.player_driven = true;
    if tutorial {
        w.system_flag_set(TUTORIAL_ARM_FLAG);
    } else {
        w.system_flag_clear(TUTORIAL_ARM_FLAG);
    }
    w.system_flag_clear(1);
    assert_eq!(
        w.install_man_formation(RIM_ELM_TRAINING_FORMATION_ID),
        Some(RIM_ELM_TRAINING_FORMATION_ID)
    );
    assert!(w.on_field_step(), "the scripted formation triggers");
    for _ in 0..600 {
        session.tick().expect("tick");
        if session.host.world.mode == SceneMode::Battle {
            break;
        }
    }
    assert_eq!(session.host.world.mode, SceneMode::Battle);
    assert_eq!(
        session.host.world.battle.tutorial.is_some(),
        tutorial,
        "the tutorial runs exactly when the disc's arm is raised"
    );
    Some(session)
}

/// One press frame, then release frames until the press has been taken.
fn tap(session: &mut NativeSession, button: PadButton) {
    session.host.world.set_pad(button.mask());
    session.tick().expect("tick");
    session.host.world.set_pad(0);
    session.tick().expect("tick");
}

fn phase(session: &NativeSession) -> Option<CommandPhase> {
    session
        .host
        .world
        .battle
        .command
        .as_ref()
        .map(|c| c.phase.clone())
}

/// Page the boxes, take `Begin`, and press `ring` on the command ring; stop
/// on the first frame the attack-mode prompt is up.
fn reach_attack_mode(session: &mut NativeSession, ring: PadButton) {
    for _ in 0..4000 {
        let w = &session.host.world;
        assert!(
            w.battle.arts_input.is_none(),
            "the arts entry opened before the attack-mode prompt"
        );
        if !w.battle.tutorial_boxes.is_empty() {
            tap(session, PadButton::Cross);
            continue;
        }
        match phase(session) {
            Some(CommandPhase::AttackMode { .. }) => {
                assert_eq!(
                    session.host.world.battle.flow,
                    BattleFlowState::AttackModePrompt,
                    "the prompt raises retail's 0x78"
                );
                return;
            }
            Some(CommandPhase::RoundPrompt { .. }) => tap(session, PadButton::Left),
            Some(CommandPhase::Menu { .. }) => tap(session, ring),
            Some(other) => panic!("the ring's Attack arm skipped the prompt: {other:?}"),
            None => {
                session.host.world.set_pad(0);
                session.tick().expect("tick");
            }
        }
    }
    panic!("the attack-mode prompt never came up");
}

/// Press `button` on the prompt and report whether the arts entry opens
/// before the member's commit lands.
fn arts_entry_opens(session: &mut NativeSession, button: PadButton) -> bool {
    tap(session, button);
    for _ in 0..600 {
        let w = &session.host.world;
        if w.battle.arts_input.is_some() {
            assert_eq!(w.battle.flow, BattleFlowState::ArtsCommandEntry);
            return true;
        }
        if !w.battle.tutorial_boxes.is_empty() {
            tap(session, PadButton::Cross);
            continue;
        }
        match phase(session) {
            Some(CommandPhase::Targeting { .. }) => tap(session, PadButton::Cross),
            // Committed: the confirm screen, the next member's ring, or the
            // round already running.
            Some(CommandPhase::CommitConfirm { .. } | CommandPhase::Menu { .. }) | None => {
                return false;
            }
            _ => {
                session.host.world.set_pad(0);
                session.tick().expect("tick");
            }
        }
    }
    panic!("the prompt's press never resolved");
}

fn check(tutorial: bool) {
    // Right on the prompt is `Command`: the directional entry.
    let Some(mut s) = open_fight(tutorial) else {
        return;
    };
    reach_attack_mode(&mut s, PadButton::Left);
    assert!(
        arts_entry_opens(&mut s, PadButton::Right),
        "Command opens the arts entry (tutorial {tutorial})"
    );

    // Left on the prompt is `Auto`: a plain attack, no arrows.
    let mut s = open_fight(tutorial).expect("disc present");
    reach_attack_mode(&mut s, PadButton::Left);
    assert!(
        !arts_entry_opens(&mut s, PadButton::Left),
        "Auto skips the arts entry (tutorial {tutorial})"
    );

    // Cross all the way: the ring's confirm takes Attack and the prompt's
    // confirm takes Auto, so the arts entry is never shown.
    let mut s = open_fight(tutorial).expect("disc present");
    reach_attack_mode(&mut s, PadButton::Cross);
    assert!(
        !arts_entry_opens(&mut s, PadButton::Cross),
        "the confirm mask is Auto on the prompt (tutorial {tutorial})"
    );
}

#[test]
fn an_ordinary_fight_reaches_the_arts_entry_only_through_command() {
    check(false);
}

#[test]
fn the_spars_attack_lesson_walks_the_same_prompt() {
    check(true);
}
