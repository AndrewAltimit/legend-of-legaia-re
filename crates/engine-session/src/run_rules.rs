//! The run rules both play hosts run: the New Game rules screen between the
//! title's NEW GAME and the opening, the run's start (story or Boss Rush),
//! the Boss Rush's stage entries and intermission, and the ironman save
//! rules over the card ports.
//!
//! One owner, as [`crate::title_continue`] is for Continue: the screen sits
//! on the world ([`open_run_rules_screen`]), so the shared screen layers
//! draw it on both hosts (`legaia_engine_screens::run_rules`); each host
//! feeds it the frame's pad edge ([`step_run_rules_screen`]). On *Start* the
//! host calls [`start_run_on`], which stages the rules on the world and
//! starts the story ([`crate::session::start_new_game_on`]) or a Boss Rush,
//! entering scenes through the host's own [`NewGameHost`]. A Boss Rush is
//! then driven by [`boss_rush_step_on`] each frame.
//!
//! The screen opens on the retail rules with the cursor on *Start*, so a
//! single confirm is the original New Game, and the world never sees a
//! staged run unless the player changed a rule.

use legaia_engine_core::world::run_rules::{RulesSetupSeat, StagedRun};
use legaia_engine_core::world::run_rules_boss_rush::BossRushDue;
use legaia_engine_run_rules::rules::{RunMode, RunRules};
use legaia_engine_run_rules::save::RunSave;
use legaia_engine_run_rules::setup::{RulesSetup, SetupInput, SetupOutcome};
use legaia_engine_save_state::WorldSaveState;

use legaia_engine_audio::AudioSink;

use crate::session::{BootSession, NewGameHost, start_new_game_on};
use crate::title_continue::{CARD_PORTS, CardPorts};

const PAD_UP: u16 = 0x0010;
const PAD_RIGHT: u16 = 0x0020;
const PAD_DOWN: u16 = 0x0040;
const PAD_LEFT: u16 = 0x0080;
const PAD_CIRCLE: u16 = 0x2000;
const PAD_CROSS: u16 = 0x4000;

/// The setup screen's input off a pad edge word (retail bit layout:
/// D-pad `0x10..0x80`, Cross confirms, Circle backs out).
pub fn setup_input(edge: u16) -> SetupInput {
    SetupInput {
        up: edge & PAD_UP != 0,
        down: edge & PAD_DOWN != 0,
        left: edge & PAD_LEFT != 0,
        right: edge & PAD_RIGHT != 0,
        confirm: edge & PAD_CROSS != 0,
        cancel: edge & PAD_CIRCLE != 0,
    }
}

/// The run block a save carries, decoded (`None` for a save without one).
pub fn save_run_rules(sf: &legaia_save::SaveFile) -> Option<RunSave> {
    sf.ext_v2.run_rules.as_deref().and_then(RunSave::decode)
}

/// What one [`step_run_rules_screen`] did.
#[derive(Debug, Clone)]
pub enum RunRulesStep {
    /// Still up (or not open at all).
    Running,
    /// Backed out: the host puts the title back.
    Cancelled,
    /// Start the run: the host calls [`start_run_on`] with these.
    Start {
        rules: RunRules,
        source: Option<Box<legaia_save::SaveFile>>,
    },
}

/// Open the New Game rules screen on `world` over the saves on `ports`
/// (every readable save is a New Game+ source), with `entropy` seeding the
/// seed rolls. The screen lives on the world
/// ([`legaia_engine_core::world::run_rules::RunRulesState::setup`]), so the
/// shared screen layers draw it on both hosts.
pub fn open_run_rules_screen(
    world: &mut legaia_engine_core::world::World,
    ports: CardPorts<'_>,
    entropy: u64,
) {
    let mut labels = Vec::new();
    let mut sources = Vec::new();
    for (port, card) in ports.iter().enumerate().take(CARD_PORTS) {
        let Some(card) = card else { continue };
        for n in 0..legaia_engine_menus::save_screen::SLOT_GRID_CELLS as usize {
            let Some((sf, _)) = card.save_numbered(n) else {
                continue;
            };
            let cycle = save_run_rules(&sf).map_or(0, |r| r.rules.ng_plus_cycle());
            let lead = sf
                .leader_summary()
                .map_or_else(String::new, |l| format!(" {} Lv{}", l.name, l.level));
            labels.push((format!("Card {} #{}{lead}", port + 1, n + 1), cycle));
            sources.push(sf);
        }
    }
    world.run_rules.setup = Some(Box::new(RulesSetupSeat {
        setup: RulesSetup::new(labels, entropy),
        sources,
    }));
}

/// Whether the New Game rules screen is up.
pub fn run_rules_screen_open(world: &legaia_engine_core::world::World) -> bool {
    world.run_rules.setup.is_some()
}

/// Step the rules screen one frame on `edge`, the frame's just-pressed pad
/// bits. A finished screen (started or backed out) is closed.
pub fn step_run_rules_screen(
    world: &mut legaia_engine_core::world::World,
    edge: u16,
) -> RunRulesStep {
    let Some(seat) = world.run_rules.setup.as_mut() else {
        return RunRulesStep::Running;
    };
    let step = match seat.setup.step(setup_input(edge)) {
        SetupOutcome::Open => return RunRulesStep::Running,
        SetupOutcome::Cancelled => RunRulesStep::Cancelled,
        SetupOutcome::Start { rules, ng_source } => RunRulesStep::Start {
            rules,
            source: ng_source
                .and_then(|i| seat.sources.get(i).cloned())
                .map(Box::new),
        },
    };
    world.run_rules.setup = None;
    step
}

/// Type a randomizer seed into the open rules screen (a host with a
/// keyboard or a text box).
pub fn set_run_rules_seed(world: &mut legaia_engine_core::world::World, seed: &str) {
    if let Some(seat) = world.run_rules.setup.as_mut() {
        seat.setup.set_seed(seed);
    }
}

impl<S: AudioSink> BootSession<S> {
    /// Stage `rules` (and, for New Game+, the carry off `source`) for the
    /// next New Game. Retail rules with no source stage nothing.
    ///
    /// The carry is lifted by loading the source into the world first
    /// (the ordinary [`WorldSaveState::load_full`]), so it carries exactly
    /// what a Continue of that save would have; the New Game then resets
    /// the world and folds the kept parts back in.
    pub fn stage_run_rules(&mut self, rules: RunRules, source: Option<legaia_save::SaveFile>) {
        let world = &mut self.host.world;
        if rules.is_retail() && source.is_none() {
            world.stage_run(None);
            return;
        }
        let carry = source.filter(|_| rules.ng_plus.is_some()).map(|sf| {
            world.load_full(sf);
            world.ng_plus_snapshot()
        });
        world.stage_run(Some(StagedRun { rules, carry }));
    }
}

/// Start the run the rules screen confirmed, on any host: stage it, then the
/// story's New Game ([`start_new_game_on`]) or a Boss Rush (the slate, the
/// Boss Rush party, the first stage's scene and fight). Returns the scene
/// entered.
pub fn start_run_on<H: NewGameHost>(
    host: &mut H,
    rules: RunRules,
    source: Option<legaia_save::SaveFile>,
) -> Option<&'static str> {
    host.new_game_session()?.stage_run_rules(rules, source);
    start_staged_run_on(host)
}

/// Whether the run staged on `world` is a Boss Rush.
pub fn boss_rush_staged(world: &legaia_engine_core::world::World) -> bool {
    world
        .run_rules
        .staged
        .as_ref()
        .is_some_and(|s| s.rules.mode == RunMode::BossRush)
}

/// Start the run staged on the host's world (its rules screen's Start, see
/// [`BootSession::stage_run_rules`]): a Boss Rush when its rules say so,
/// else the story's New Game ([`start_new_game_on`]) - which, with nothing
/// staged, is exactly the retail New Game.
pub fn start_staged_run_on<H: NewGameHost>(host: &mut H) -> Option<&'static str> {
    if !boss_rush_staged(&host.new_game_session()?.host.world) {
        return start_new_game_on(host);
    }
    let session = host.new_game_session()?;
    session.begin_new_game_slate();
    let pack = boss_rush_pack_stages(&session.host.world);
    session.host.world.begin_boss_rush(&pack);
    match boss_rush_step_on(host) {
        BossRushHostStep::Entered(scene) => Some(scene),
        _ => None,
    }
}

/// The active pack's bosses a Boss Rush appends, at the level of the
/// strongest retail stage plus five per pack boss.
fn boss_rush_pack_stages(world: &legaia_engine_core::world::World) -> Vec<(String, u8)> {
    let Some(c) = world.expansion.battle.compiled.as_ref() else {
        return Vec::new();
    };
    let base = legaia_engine_run_rules::boss_rush::RETAIL_STAGES
        .last()
        .map_or(30, |s| s.level);
    c.bosses
        .keys()
        .enumerate()
        .map(|(i, k)| (k.clone(), base.saturating_add(5 * (i as u8 + 1)).min(99)))
        .collect()
}

/// What [`boss_rush_step_on`] did this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BossRushHostStep {
    /// Nothing due (no Boss Rush, or a fight or the intermission is on).
    Idle,
    /// A stage's scene was entered and its fight armed: the host rebuilds
    /// what it draws, as after a New Game's opening.
    Entered(&'static str),
    /// A stage was armed where the party stands.
    Armed,
    /// The stage would not start; the run is over.
    Failed,
}

/// Drive a Boss Rush one frame on any host: tick its clock, and when it
/// waits on the next stage enter that stage's scene through the host and arm
/// its fight.
pub fn boss_rush_step_on<H: NewGameHost>(host: &mut H) -> BossRushHostStep {
    let Some(session) = host.new_game_session() else {
        return BossRushHostStep::Idle;
    };
    let world = &mut session.host.world;
    world.boss_rush_tick();
    let Some(due) = world.boss_rush_due() else {
        return BossRushHostStep::Idle;
    };
    let entered = match due {
        BossRushDue::EnterScene(scene) => {
            if let Err(e) = host.enter_opening_scene(scene) {
                log::error!("boss rush: could not enter '{scene}': {e}");
            }
            Some(scene)
        }
        BossRushDue::Arm => None,
    };
    let Some(session) = host.new_game_session() else {
        return BossRushHostStep::Failed;
    };
    if !session.host.world.boss_rush_arm_stage() {
        return BossRushHostStep::Failed;
    }
    entered.map_or(BossRushHostStep::Armed, BossRushHostStep::Entered)
}

/// The intermission or result screen's input: Up / Down move, Cross acts.
/// Returns `true` when the result screen was dismissed (the host returns to
/// the title; the run is dropped).
pub fn boss_rush_screen_input(world: &mut legaia_engine_core::world::World, edge: u16) -> bool {
    if world.boss_rush_result().is_some() {
        if edge & PAD_CROSS != 0 {
            world.end_boss_rush();
            return true;
        }
        return false;
    }
    if world.run_rules.intermission.is_none() {
        return false;
    }
    if edge & PAD_UP != 0 {
        world.boss_rush_intermission_move(-1);
    } else if edge & PAD_DOWN != 0 {
        world.boss_rush_intermission_move(1);
    } else if edge & PAD_CROSS != 0 {
        world.boss_rush_intermission_confirm();
    }
    false
}

/// Whether the Boss Rush screen owns the frame (the host draws it and feeds
/// it the pad instead of the field).
pub fn boss_rush_screen_up(world: &legaia_engine_core::world::World) -> bool {
    world.run_rules.intermission.is_some() || world.boss_rush_result().is_some()
}
