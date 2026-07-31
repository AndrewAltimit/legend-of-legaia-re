//! Two more battle-overlay kernels driven from their retail call sites: the
//! AI queue assembler's auto-fill arm (`FUN_801F0450` at state `0x00`) and the
//! queued-magic follow-up guard (`FUN_801F3C34` at state `0x36`).
//!
//! Both were ported with correct kernels and no caller. The point of these is
//! the seam: that the SM reaches them, with the right per-character inputs,
//! and that the gates which keep them quiet in ordinary play really do.

use legaia_engine_vm::battle_action::{
    ActionCategory, ActionState, BattleActionCtx, BattleActionHost, BattleActor, step,
};
use legaia_engine_vm::battle_arts_auto_combo::{ART_ACTION_BIAS, AUTO_FILL_ABILITY_BIT};

#[derive(Default)]
struct Host {
    actors: Vec<BattleActor>,
    /// `+0xF8` word per party slot - the auto-fill gate's source.
    ability_high: [u32; 3],
    arts: [Vec<u8>; 3],
    spells: [(Vec<u8>, Vec<u8>); 3],
    /// Scripted RNG, consumed in call order.
    rolls: std::cell::RefCell<std::collections::VecDeque<u32>>,
    ui: Vec<(u8, u8)>,
}

impl Host {
    fn new() -> Self {
        let mut h = Host {
            actors: vec![BattleActor::default(); 8],
            ..Default::default()
        };
        for a in h.actors.iter_mut() {
            a.liveness = 100;
            a.hp = 100;
        }
        h
    }
    fn push_rolls(&mut self, rolls: &[u32]) {
        self.rolls.borrow_mut().extend(rolls.iter().copied());
    }
}

impl BattleActionHost for Host {
    fn actor(&self, slot: u8) -> Option<&BattleActor> {
        self.actors.get(slot as usize)
    }
    fn actor_mut(&mut self, slot: u8) -> Option<&mut BattleActor> {
        self.actors.get_mut(slot as usize)
    }
    fn rng(&mut self) -> u32 {
        self.rolls.borrow_mut().pop_front().unwrap_or(1)
    }
    fn character_ability_bits_high(&self, slot: u8) -> u32 {
        self.ability_high.get(slot as usize).copied().unwrap_or(0)
    }
    fn learned_arts(&self, slot: u8) -> Vec<u8> {
        self.arts.get(slot as usize).cloned().unwrap_or_default()
    }
    fn caster_spell_list(&self, slot: u8) -> Option<(Vec<u8>, Vec<u8>)> {
        self.spells.get(slot as usize).cloned()
    }
    fn ui_element(&mut self, id: u8, mode: u8) {
        self.ui.push((id, mode));
    }
}

/// Without the `+0xF8` passive bit nothing is written - which is every
/// ordinary party, so the arm is inert by data rather than by omission.
#[test]
fn the_auto_fill_arm_is_silent_without_the_passive_bit() {
    let mut host = Host::new();
    host.arts[0] = vec![9, 10];
    host.actors[0].params[0] = 0xAB;

    let mut ctx = BattleActionCtx::new();
    ctx.action_state = ActionState::Begin.as_byte();
    step(&mut host, &mut ctx);
    assert_eq!(host.actors[0].params[0], 0xAB, "queue untouched");
    assert_eq!(host.actors[0].action_category, 0);
}

/// With the bit set the arm stamps category `3`, rolls a monster target and
/// fills the queue with biased learned-art bytes.
#[test]
fn the_auto_fill_arm_writes_the_queue_from_the_learned_arts_list() {
    let mut host = Host::new();
    host.ability_high[0] = AUTO_FILL_ABILITY_BIT;
    host.arts[0] = vec![9, 10];
    // Two monsters seated; everything above slot 4 is empty.
    host.actors[5].liveness = 0;
    host.actors[6].liveness = 0;
    host.actors[7].liveness = 0;
    // Rolls, in call order: target draw, then (stop, index) pairs until a
    // stop roll divisible by 7 ends the fill.
    host.push_rolls(&[1, 1, 0, 1, 1, 7]);

    let mut ctx = BattleActionCtx::new();
    ctx.action_state = ActionState::Begin.as_byte();
    step(&mut host, &mut ctx);

    assert_eq!(
        host.actors[0].action_category,
        ActionCategory::Attack.as_byte()
    );
    assert!(
        (3..5).contains(&host.actors[0].active_target),
        "target {} is not a seated monster slot",
        host.actors[0].active_target
    );
    // Two arts kept, each biased by 0x1B, then the stream terminates.
    assert_eq!(host.actors[0].params[0], 9 + ART_ACTION_BIAS);
    assert_eq!(host.actors[0].params[1], 10 + ART_ACTION_BIAS);
    assert_eq!(host.actors[0].params[2], 0);
    // Slot 1 has no passive: untouched.
    assert_eq!(host.actors[1].params[0], 0);
}

/// The arm runs once per battle, not once per action - the same guard the
/// formation latch uses.
#[test]
fn the_auto_fill_arm_runs_once_per_battle() {
    let mut host = Host::new();
    host.ability_high[0] = AUTO_FILL_ABILITY_BIT;
    host.arts[0] = vec![9];
    host.push_rolls(&[1, 1, 0, 7]);

    let mut ctx = BattleActionCtx::new();
    ctx.action_state = ActionState::Begin.as_byte();
    step(&mut host, &mut ctx);
    let first = host.actors[0].params;
    let left = host.rolls.borrow().len();

    // Re-enter Begin (the port re-arms it per action; retail does not).
    ctx.action_state = ActionState::Begin.as_byte();
    step(&mut host, &mut ctx);
    assert_eq!(host.actors[0].params, first, "queue not refilled");
    assert_eq!(
        host.rolls.borrow().len(),
        left,
        "no RNG draws on the second pass"
    );
}

/// The follow-up guard prints its message from state `0x36`, gated on the
/// caster's own spell level.
#[test]
fn the_summon_return_guard_prints_only_for_a_level_three_spell() {
    let mut spells = (vec![0u8; 36], vec![0u8; 36]);
    spells.0[5] = 0x88;
    spells.1[5] = 3;

    let mut host = Host::new();
    host.spells[1] = spells.clone();
    host.actors[1].params[0] = 0x88;

    let mut ctx = BattleActionCtx::new();
    ctx.active_actor = 1;
    ctx.action_state = ActionState::SummonReturn.as_byte();
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::SummonVerifyAlive.as_byte());
    assert!(host.ui.contains(&(0x66, 0)), "{:?}", host.ui);
    assert_eq!(ctx.message_id, 0x66);

    // Level 2: silent.
    let mut host = Host::new();
    let mut low = spells.clone();
    low.1[5] = 2;
    host.spells[1] = low;
    host.actors[1].params[0] = 0x88;
    let mut ctx = BattleActionCtx::new();
    ctx.active_actor = 1;
    ctx.action_state = ActionState::SummonReturn.as_byte();
    step(&mut host, &mut ctx);
    assert!(host.ui.is_empty());
    assert_eq!(ctx.message_id, 0);

    // A pending follow-up also suppresses it.
    let mut host = Host::new();
    host.spells[1] = spells;
    host.actors[1].params[0] = 0x88;
    let mut ctx = BattleActionCtx::new();
    ctx.active_actor = 1;
    ctx.action_state = ActionState::SummonReturn.as_byte();
    ctx.follow_up_pending = 1;
    step(&mut host, &mut ctx);
    assert!(host.ui.is_empty());
}

/// The three excluded action ids never reach the caster record at all.
#[test]
fn the_guard_skips_the_three_excluded_action_ids() {
    for action in [0x85u8, 0x8E, 0x96, 0xFF] {
        let mut spells = (vec![0u8; 36], vec![0u8; 36]);
        spells.0[0] = action;
        spells.1[0] = 9;
        let mut host = Host::new();
        host.spells[1] = spells;
        host.actors[1].params[0] = action;
        let mut ctx = BattleActionCtx::new();
        ctx.active_actor = 1;
        ctx.action_state = ActionState::SummonReturn.as_byte();
        step(&mut host, &mut ctx);
        assert!(host.ui.is_empty(), "action {action:#04x} should be skipped");
    }
}
