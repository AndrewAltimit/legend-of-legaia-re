//! Run-rules runs: `<label>+rules=<preset>` plays the run under the port's
//! own run rules (`engine-run-rules`), staged the way the New Game rules
//! screen stages them (`BootSession::stage_run_rules`, then the New Game
//! slate), so the difficulty knobs, the in-engine randomizer, ironman's card
//! rules, New Game+'s carry and the Boss Rush stage loop all run under the
//! detectors.

use legaia_engine_run_rules::rules::{
    CarrySet, Difficulty, Ironman, NgPlus, Randomizer, RunMode, RunRules,
};

use legaia_engine_save_state::WorldSaveState;

use super::*;

/// The presets a `+rules=` suffix names.
pub(super) const RULES_PRESETS: [&str; 8] = [
    "easy",
    "hard",
    "nightmare",
    "rando",
    "ironman",
    "hardcore",
    "ngplus",
    "bossrush",
];

/// `"town01+rules=hard"` -> `("town01", Some("hard"))`. The suffix is the
/// outermost of a label's (before an `@<save>`).
pub(super) fn split_rules(label: &str) -> (&str, Option<&str>) {
    match label.rsplit_once("+rules=") {
        Some((scene, preset)) => (scene, Some(preset)),
        None => (label, None),
    }
}

/// The rules preset `name` stands for, with the randomizer seed drawn from
/// the run seed so every soak seed is its own shuffle. `None` for a name
/// that is not a preset.
pub(super) fn rules_preset(name: &str, run_seed: u64) -> Option<RunRules> {
    let mut r = RunRules::default();
    match name {
        "easy" => r.difficulty = Difficulty::Easy,
        "hard" => r.difficulty = Difficulty::Hard,
        "nightmare" => r.difficulty = Difficulty::Nightmare,
        "rando" => r.randomizer = Some(Randomizer::all(format!("soak-{run_seed:x}"))),
        "ironman" => r.ironman = Some(Ironman { hardcore: false }),
        "hardcore" => r.ironman = Some(Ironman { hardcore: true }),
        "ngplus" => {
            r.ng_plus = Some(NgPlus {
                cycle: 1,
                carry: CarrySet::default(),
            })
        }
        "bossrush" => r.mode = RunMode::BossRush,
        _ => return None,
    }
    Some(r)
}

/// One frame of the run rules' host duties, the play window's
/// `poll_run_rules`: a Boss Rush's due stage entered and armed, its
/// intermission or result screen fed the frame's pad edge, and ironman's
/// card rules applied to the soak card. Returns `true` when the Boss Rush
/// screen owns the frame (the host skips the field tick, as both hosts'
/// boot UI does) and `Some(())` in the second slot once the result screen
/// is dismissed - the run is over.
pub(super) fn step_run_rules(
    session: &mut NativeSession,
    opts: &FieldLiveOpts,
    edge: u16,
    card: &mut SoakCard,
) -> (bool, bool) {
    use legaia_engine_session::run_rules::{boss_rush_screen_input, boss_rush_screen_up};
    let _ = session.boss_rush_step(opts);
    let mut screen = false;
    let mut over = false;
    if boss_rush_screen_up(&session.host.world) {
        screen = true;
        over = boss_rush_screen_input(&mut session.host.world, edge);
    }
    let w = &mut session.host.world;
    if w.take_ironman_erase().is_some() {
        card.clear();
    }
    if w.take_run_autosave() {
        let sf = session.host.world.save_full();
        let resume = session.current_resume();
        card.insert((0, 0), (sf, resume));
    }
    (screen, over)
}
