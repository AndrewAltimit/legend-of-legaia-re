//! Disc-gated: Boss Rush (`legaia_engine_run_rules::boss_rush`) on the real
//! scenes.
//!
//! `boss_rush_rows_seat_their_bosses` pins every retail stage's scripted
//! row against its scene's formation table: the row must seat the stage's
//! boss. `boss_rush_runs_its_first_stages` starts a Boss Rush through the
//! session's run start (`BootSession::start_run`) and drives it with pad
//! presses through the first stages and their intermissions, the monsters'
//! HP cut to 1 once a fight is seated so the test measures the run's
//! machinery, not the party's balance.
//!
//! Skip-passes without `LEGAIA_DISC_BIN`.

use std::path::PathBuf;

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_run_rules::boss_rush::{Phase, RETAIL_STAGES};
use legaia_engine_run_rules::rules::{RunMode, RunRules};
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;

fn open_session() -> Option<NativeSession> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from);
    let Some(disc) = disc.filter(|p| p.exists()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or missing (disc-gated convention)");
        return None;
    };
    let cfg = BootConfig {
        scene: legaia_engine_session::session::DEFAULT_BOOT_SCENE.to_string(),
        enable_audio: false,
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut candidates = Vec::new();
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR") {
        candidates.push(PathBuf::from(d));
    }
    candidates.push(root.join("extracted"));
    for d in candidates {
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(NativeSession::open(&d, &cfg).expect("open NativeSession"));
        }
    }
    Some(NativeSession::open_disc(&disc, &cfg).expect("open NativeSession (disc)"))
}

fn opts() -> FieldLiveOpts {
    FieldLiveOpts {
        live_loop: true,
        player_battle: true,
        battle_bgm: None,
    }
}

fn host_drains(session: &mut NativeSession) {
    let w = &mut session.host.world;
    let _ = w.drain_field_events();
    let _ = w.drain_battle_events();
    let _ = w.drain_battle_hit_fx();
    let _ = w.drain_battle_hit_events();
    let _ = w.drain_battle_sfx_cues();
    let _ = w.drain_battle_shout_cues();
    let _ = w.drain_battle_xa_cues();
    let _ = w.drain_battle_xa_prestage();
    let _ = w.route_battle_effect_spawns();
    let _ = w.drain_battle_clut_stages();
    let _ = w.drain_minigame_sfx_cues();
}

#[test]
fn boss_rush_rows_seat_their_bosses() {
    let Some(mut session) = open_session() else {
        return;
    };
    let mut wrong = Vec::new();
    for s in RETAIL_STAGES {
        session.begin_new_game();
        session
            .enter_scene_live(s.scene, &opts())
            .unwrap_or_else(|e| panic!("enter {}: {e:#}", s.scene));
        let ids: Vec<u16> = session
            .host
            .world
            .tables
            .formation_table
            .formation(u16::from(s.row))
            .map(|f| f.slots.iter().map(|x| x.monster_id).collect())
            .unwrap_or_default();
        eprintln!("[boss-rush] {} {} row {}: {ids:?}", s.name, s.scene, s.row);
        if !ids.contains(&s.monster_id) {
            wrong.push(format!("{} ({} row {}: {ids:?})", s.name, s.scene, s.row));
        }
    }
    assert!(
        wrong.is_empty(),
        "stages whose row does not seat the boss: {wrong:?}"
    );
}

/// The fighter: Begin, Attack, Auto, confirm - every one a pad press.
fn fight_pad(session: &NativeSession) -> u16 {
    use legaia_engine_battle_menus::battle_input::CommandPhase;
    let w = &session.host.world;
    if !w.battle.tutorial_boxes.is_empty() {
        return PadButton::Cross.mask();
    }
    if w.battle.item_menu.is_some() {
        return PadButton::Circle.mask();
    }
    if let Some(cmd) = w.battle.command.as_ref() {
        return match &cmd.phase {
            CommandPhase::RoundPrompt { .. }
            | CommandPhase::Menu { .. }
            | CommandPhase::AttackMode { .. }
            | CommandPhase::CommitConfirm { .. } => PadButton::Left.mask(),
            CommandPhase::Targeting { .. } => PadButton::Cross.mask(),
            _ => 0,
        };
    }
    PadButton::Cross.mask()
}

const STAGES: usize = 3;
const TICK_BUDGET: u32 = 200_000;

#[test]
fn boss_rush_runs_its_first_stages() {
    let Some(mut session) = open_session() else {
        return;
    };
    let rules = RunRules {
        mode: RunMode::BossRush,
        ..Default::default()
    };
    let first = session.start_run(rules, None, &opts());
    assert_eq!(
        first,
        Some(RETAIL_STAGES[0].scene),
        "the first stage's scene"
    );
    assert_eq!(
        session.host.world.party.party_count, 3,
        "Vahn, Noa and Gala"
    );
    let mut prev = 0u16;
    let mut entered = vec![RETAIL_STAGES[0].scene];
    let mut intermissions = 0;
    for tick in 0..TICK_BUDGET {
        // The host's run step (the windows' `poll_run_rules`).
        if !legaia_engine_session::run_rules::boss_rush_screen_up(&session.host.world)
            && let legaia_engine_session::run_rules::BossRushHostStep::Entered(scene) =
                session.boss_rush_step(&opts())
        {
            entered.push(scene);
        }
        let w = &mut session.host.world;
        let run = w.run_rules.boss_rush.clone().expect("the run");
        if run.stage >= STAGES || matches!(run.phase, Phase::Finished { .. }) {
            assert_ne!(
                run.phase,
                Phase::Finished { cleared: false },
                "the run was lost at stage {} (tick {tick})",
                run.stage
            );
            eprintln!(
                "[boss-rush] {STAGES} stages in {tick} ticks, clock {} frames, scenes {entered:?}",
                run.frames
            );
            assert!(run.frames > 0);
            assert!(intermissions >= STAGES - 1);
            assert!(w.party.level_up_tracker.level[0] >= RETAIL_STAGES[STAGES - 1].level);
            return;
        }
        if w.run_rules.intermission.is_some() {
            // Next battle is the first row: Cross on it.
            intermissions += 1;
            assert!(
                w.boss_rush_intermission_confirm(),
                "Next closes the intermission"
            );
            continue;
        }
        if w.mode == SceneMode::Battle {
            let pc = usize::from(w.party.party_count);
            for a in w.actors.iter_mut().skip(pc).take(5) {
                if a.battle_monster_id.is_some() && a.battle.hp > 1 {
                    a.battle.hp = 1;
                }
            }
        }
        let want = if w.mode == SceneMode::Battle {
            fight_pad(&session)
        } else {
            0
        };
        let pad = if prev == 0 { want } else { 0 };
        prev = pad;
        session.host.world.set_pad(pad);
        session.tick().expect("tick");
        host_drains(&mut session);
    }
    panic!(
        "the Boss Rush did not clear {STAGES} stages in {TICK_BUDGET} ticks: {:?}",
        session.host.world.run_rules.boss_rush
    );
}
