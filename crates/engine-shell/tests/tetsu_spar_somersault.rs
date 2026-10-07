//! Disc-gated: the Rim Elm sparring fight ends once the player performs the
//! taught Hyper Art through the real command path.
//!
//! The fight is retail's four-lesson tutorial (overlay 967, see
//! `docs/subsystems/battle.md`): Attacks, Items, Spirit, then the
//! `[High] [Low] [High]` drill - Vahn's Somersault, `Up Down Up` - entered
//! through the `Command` chip. The drill is validated by the flow-state `90`
//! hook against the arts entry's swing buffer, the commit by the `110` hook,
//! and the fourth accepted lesson runs the completion tail whose countdown
//! takes the fight back to the field with the survived bit (story flag 1).
//!
//! Every input here is a pad press a player makes - the round prompt, the
//! ring, the attack-mode chip, the arts arrows, the review and the target
//! cursor - with no engine call past entering the battle. Variants cover the
//! Somersault inside a longer string, a wrong drill first, and the attack
//! lesson taken through `Command` rather than `Auto`.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_core::arts_command_input::ArtsInputPhase;
use legaia_engine_core::battle_input::CommandPhase;
use legaia_engine_core::battle_tutorial::{TUTORIAL_ARM_FLAG, TutorialLesson};
use legaia_engine_core::encounter_record::RIM_ELM_TRAINING_FORMATION_ID;
use legaia_engine_core::input::PadButton;
use legaia_engine_core::inventory_use::InventoryUseState;
use legaia_engine_core::world::SceneMode;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// How the hand plays the attack lessons.
#[derive(Clone)]
struct Plan {
    /// Lesson 0: take `Command` and enter these arrows (else `Auto`).
    lesson0_arrows: Option<Vec<PadButton>>,
    /// Lesson 3 attempts, in order; the last repeats.
    drill: Vec<Vec<PadButton>>,
}

fn open_spar(extracted: &std::path::Path) -> BootSession {
    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(extracted, &cfg).expect("open boot session");
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
    // The disc's own arm (town01's Tetsu record raises it two ops before its
    // battle-entry op), then the record's formation.
    w.system_flag_set(TUTORIAL_ARM_FLAG);
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
    assert!(
        session.host.world.battle.tutorial.is_some(),
        "the spar runs the tutorial"
    );
    session
}

/// The press a player makes this frame. `drill_tries` counts the lesson-3
/// arts entries opened so far.
fn hand(session: &BootSession, plan: &Plan, drill_tries: &mut usize, entry_open: &mut bool) -> u16 {
    let w = &session.host.world;
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
        let arrows: Vec<PadButton> = match lesson {
            Some(TutorialLesson::HyperArts) => {
                if !*entry_open {
                    *entry_open = true;
                    *drill_tries += 1;
                }
                let i = (*drill_tries - 1).min(plan.drill.len() - 1);
                plan.drill[i].clone()
            }
            _ => plan.lesson0_arrows.clone().unwrap_or_default(),
        };
        return match &arts.phase {
            ArtsInputPhase::Entering if arts.buffer.len() < arrows.len() => {
                arrows[arts.buffer.len()].mask()
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
                let command = match lesson {
                    Some(TutorialLesson::HyperArts) => true,
                    _ => plan.lesson0_arrows.is_some(),
                };
                if command {
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

struct Outcome {
    back_in_field: bool,
    scene: String,
    survived_flag: bool,
    max_lesson: u8,
    drill_tries: usize,
    ticks: u32,
}

fn play(plan: &Plan) -> Option<Outcome> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return None;
    };
    let mut session = open_spar(&extracted);
    let mut prev = 0u16;
    let mut drill_tries = 0usize;
    let mut entry_open = false;
    let mut max_lesson = 0u8;
    let mut ticks = 0u32;
    while session.host.world.mode == SceneMode::Battle && ticks < 40_000 {
        if let Some(t) = session.host.world.battle.tutorial.as_ref() {
            max_lesson = max_lesson.max(t.lesson);
        }
        let want = hand(&session, plan, &mut drill_tries, &mut entry_open);
        let pad = if prev == 0 { want } else { 0 };
        prev = pad;
        session.host.world.set_pad(pad);
        session.tick().expect("tick");
        ticks += 1;
    }
    let w = &session.host.world;
    eprintln!(
        "[ran] mode {:?} after {ticks} ticks, lesson reached {max_lesson}, drill tries {drill_tries}, flow {:?}",
        w.mode, w.battle.flow
    );
    Some(Outcome {
        back_in_field: w.mode == SceneMode::Field,
        scene: session
            .host
            .scene
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default(),
        survived_flag: w.system_flag_test(1),
        max_lesson,
        drill_tries,
        ticks,
    })
}

fn assert_spar_ended(o: &Outcome) {
    assert!(
        o.max_lesson >= 4,
        "the Somersault drill must be accepted (lesson reached {})",
        o.max_lesson
    );
    assert!(
        o.back_in_field,
        "the spar must close back to the field after the Somersault ({} ticks)",
        o.ticks
    );
    assert_eq!(o.scene, "town01");
    assert!(o.survived_flag, "the 967 exit arm raises story flag 1");
}

const SOMERSAULT: [PadButton; 3] = [PadButton::Up, PadButton::Down, PadButton::Up];

#[test]
fn somersault_ends_the_spar() {
    let plan = Plan {
        lesson0_arrows: None,
        drill: vec![SOMERSAULT.to_vec()],
    };
    let Some(o) = play(&plan) else { return };
    assert_spar_ended(&o);
    assert_eq!(o.drill_tries, 1, "a correct drill passes first time");
}

#[test]
fn somersault_after_a_leading_arrow_ends_the_spar() {
    // Retail matches the drill at buffer offsets 0..=2, so one leading arrow
    // still counts.
    let plan = Plan {
        lesson0_arrows: None,
        drill: vec![vec![
            PadButton::Left,
            PadButton::Up,
            PadButton::Down,
            PadButton::Up,
        ]],
    };
    let Some(o) = play(&plan) else { return };
    assert_spar_ended(&o);
}

#[test]
fn a_wrong_drill_rewinds_and_the_retry_ends_the_spar() {
    let plan = Plan {
        lesson0_arrows: None,
        drill: vec![
            vec![PadButton::Down, PadButton::Down, PadButton::Down],
            SOMERSAULT.to_vec(),
        ],
    };
    let Some(o) = play(&plan) else { return };
    assert_spar_ended(&o);
    assert!(o.drill_tries >= 2, "the wrong string is refused");
}

#[test]
fn the_attack_lesson_through_command_arts_still_advances() {
    let plan = Plan {
        lesson0_arrows: Some(vec![PadButton::Left, PadButton::Right]),
        drill: vec![SOMERSAULT.to_vec()],
    };
    let Some(o) = play(&plan) else { return };
    assert_spar_ended(&o);
}
