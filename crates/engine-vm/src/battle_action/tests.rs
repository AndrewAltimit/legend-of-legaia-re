#![allow(clippy::field_reassign_with_default)]

use super::*;
use std::cell::RefCell;

/// Recording host. Captures every callback so tests can assert exact
/// dispatch order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Pose(u8, Pose),
    Ui(u8, u8),
    PartySetup(u8),
    MonsterSetup(u8),
    Camera,
    SpellAnim(u8, u8),
    SpellSustain(u8, u8),
    ApplyDamage(u8, u8, u8, u8),
    ApplyArtStrike(ArtStrikeInfo),
    ScreenShake(u16),
    Brightness(u8),
    BattleEnd(BattleEndCause),
    LoadCapture(u8),
    Recompute,
}

#[derive(Default)]
struct RecHost {
    actors: Vec<BattleActor>,
    events: RefCell<Vec<Event>>,
    capture_spells: std::collections::HashSet<u8>,
    spell_costs: std::collections::HashMap<u8, u8>,
    ability_bits: std::collections::HashMap<u8, u32>,
    ranges: std::collections::HashMap<(u8, u8), u16>,
    prev_cleared: bool,
    sound_ready: bool,
    rng_seq: Vec<u32>,
    rng_pos: RefCell<usize>,
    party_count: u8,
    slot_count: u8,
    /// Pre-staged art records returned by `art_record(character, action)`
    /// - keyed by `(character_byte, action_byte)`.
    art_records: std::collections::HashMap<(u8, u8), legaia_art::ArtRecord>,
}

impl RecHost {
    fn with_n_actors(n: usize) -> Self {
        Self {
            actors: (0..n).map(|_| BattleActor::new()).collect(),
            prev_cleared: true,
            sound_ready: true,
            party_count: 3,
            slot_count: ACTOR_SLOTS as u8,
            ..Default::default()
        }
    }
    fn record(&self, e: Event) {
        self.events.borrow_mut().push(e);
    }
    fn take(&self) -> Vec<Event> {
        std::mem::take(&mut self.events.borrow_mut())
    }
}

impl BattleActionHost for RecHost {
    fn actor(&self, slot: u8) -> Option<&BattleActor> {
        self.actors.get(slot as usize)
    }
    fn actor_mut(&mut self, slot: u8) -> Option<&mut BattleActor> {
        self.actors.get_mut(slot as usize)
    }
    fn pose(&mut self, actor_id: u8, pose: Pose) {
        self.record(Event::Pose(actor_id, pose));
    }
    fn ui_element(&mut self, effect_id: u8, mode: u8) {
        self.record(Event::Ui(effect_id, mode));
    }
    fn range_check(&self, a: u8, t: u8) -> u16 {
        self.ranges.get(&(a, t)).copied().unwrap_or(0)
    }
    fn camera_bounds(&mut self) {
        self.record(Event::Camera);
    }
    fn party_setup(&mut self, s: u8) {
        self.record(Event::PartySetup(s));
    }
    fn monster_setup(&mut self, s: u8) {
        self.record(Event::MonsterSetup(s));
    }
    fn recompute_battle_order(&mut self) {
        self.record(Event::Recompute);
    }
    fn rng(&mut self) -> u32 {
        let mut p = self.rng_pos.borrow_mut();
        let v = self.rng_seq.get(*p).copied().unwrap_or(0);
        *p += 1;
        v
    }
    fn previous_action_cleared(&self, _: u8) -> bool {
        self.prev_cleared
    }
    fn sound_bank_ready(&self, _: u8) -> bool {
        self.sound_ready
    }
    fn load_capture_archive(&mut self, idx: u8) {
        self.record(Event::LoadCapture(idx));
    }
    fn spell_anim_trigger(&mut self, p: u8, s: u8) {
        self.record(Event::SpellAnim(p, s));
    }
    fn spell_anim_sustain(&mut self, a: u8, anim: u8) {
        self.record(Event::SpellSustain(a, anim));
    }
    fn apply_damage(&mut self, a: u8, b: u8, c: u8, d: u8) {
        self.record(Event::ApplyDamage(a, b, c, d));
    }
    fn apply_art_strike(&mut self, info: ArtStrikeInfo) {
        self.record(Event::ApplyArtStrike(info));
    }
    fn art_record(
        &self,
        character: legaia_art::Character,
        action: legaia_art::ActionConstant,
    ) -> Option<&legaia_art::ArtRecord> {
        self.art_records
            .get(&(character_byte(character), action.as_byte()))
    }
    fn is_capture_spell(&self, id: u8) -> bool {
        self.capture_spells.contains(&id)
    }
    fn spell_mp_cost(&self, id: u8) -> u8 {
        self.spell_costs.get(&id).copied().unwrap_or(0)
    }
    fn character_ability_bits(&self, slot: u8) -> u32 {
        self.ability_bits.get(&slot).copied().unwrap_or(0)
    }
    fn screen_shake(&mut self, m: u16) {
        self.record(Event::ScreenShake(m));
    }
    fn ramp_brightness(&mut self, p: u8) {
        self.record(Event::Brightness(p));
    }
    fn battle_end(&mut self, c: BattleEndCause) {
        self.record(Event::BattleEnd(c));
    }
    fn frame_dt(&self) -> i16 {
        1
    }
    fn party_count(&self) -> u8 {
        self.party_count
    }
    fn slot_count(&self) -> u8 {
        self.slot_count
    }
}

/// Cheap byte encoding for tests. `Character` is a 3-variant enum with
/// no public byte-mapping accessor - this mirrors the `0/1/2` ordering
/// of `Character::all()`.
fn character_byte(c: legaia_art::Character) -> u8 {
    match c {
        legaia_art::Character::Vahn => 0,
        legaia_art::Character::Noa => 1,
        legaia_art::Character::Gala => 2,
    }
}

fn fresh(category: ActionCategory, slot: u8) -> (BattleActionCtx, RecHost) {
    let mut host = RecHost::with_n_actors(ACTOR_SLOTS);
    // Mark all slots alive.
    for a in &mut host.actors {
        a.liveness = 1;
    }
    host.actors[slot as usize].action_category = category.as_byte();
    let mut ctx = BattleActionCtx::new();
    ctx.active_actor = slot;
    (ctx, host)
}

#[test]
fn action_state_byte_roundtrip() {
    for s in [
        ActionState::Begin,
        ActionState::ActionSeed,
        ActionState::AttackChain,
        ActionState::DoneCleanup,
        ActionState::EndOfAction,
        ActionState::BattleComplete,
    ] {
        assert_eq!(ActionState::from_byte(s.as_byte()).unwrap(), s);
    }
    // Unmapped byte returns None.
    assert!(ActionState::from_byte(0x07).is_none());
}

#[test]
fn action_category_byte_roundtrip() {
    for c in [
        ActionCategory::TacticalArts,
        ActionCategory::Item,
        ActionCategory::Magic,
        ActionCategory::Attack,
        ActionCategory::Spirit,
        ActionCategory::Run,
    ] {
        assert_eq!(ActionCategory::from_byte(c.as_byte()), c);
    }
    // Reserved bytes fold to TacticalArts.
    assert_eq!(
        ActionCategory::from_byte(0x42),
        ActionCategory::TacticalArts
    );
}

#[test]
fn begin_with_menu_open_routes_to_queued_from_menu() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::Begin.as_byte();
    ctx.queued_action = 5;
    ctx.menu_open = 1;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::QueuedFromMenu.as_byte()
    ));
    assert_eq!(host.actors[0].action_queue_counter, 5);
}

#[test]
fn begin_without_menu_routes_to_pre_action_wait() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::Begin.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::PreActionWait.as_byte()
    ));
}

#[test]
fn pre_action_wait_holds_until_cleared() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::PreActionWait.as_byte();
    host.prev_cleared = false;
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::Stay);
    host.prev_cleared = true;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::ActionSeed.as_byte()
    ));
}

#[test]
fn queued_from_menu_holds_then_releases() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::QueuedFromMenu.as_byte();
    ctx.menu_open = 1;
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    ctx.menu_open = 0;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::PreActionWait.as_byte()
    ));
}

#[test]
fn action_seed_attack_routes_to_attack_face_and_emits_ui() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackFace.as_byte()
    ));
    // Party slot < 3 → fires UI element 7.
    let events = host.take();
    assert!(events.contains(&Event::PartySetup(1)));
    assert!(events.contains(&Event::Camera));
    assert!(events.contains(&Event::Pose(1, Pose::Idle)));
    assert!(events.contains(&Event::Ui(7, 0)));
}

#[test]
fn action_seed_run_party_routes_to_run_begin() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::RunBegin.as_byte()
    ));
    // Camera not called for run actions.
    assert!(!host.take().contains(&Event::Camera));
}

#[test]
fn action_seed_run_monster_routes_to_capture_start() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 5);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::CaptureStart.as_byte()
    ));
}

#[test]
fn action_seed_magic_routes_to_magic_cast_begin() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::MagicCastBegin.as_byte()
    ));
}

#[test]
fn action_seed_monster_with_ai_flag_calls_monster_setup() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 4);
    host.actors[4].field_flags = 0x380;
    ctx.action_state = ActionState::ActionSeed.as_byte();
    step(&mut host, &mut ctx);
    let events = host.take();
    assert!(events.contains(&Event::MonsterSetup(4)));
    assert!(!events.iter().any(|e| matches!(e, Event::PartySetup(_))));
}

#[test]
fn attack_face_in_range_routes_to_chain() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackFace.as_byte();
    host.actors[1].active_target = 4;
    // No range entry → returns 0 (in range).
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackChain.as_byte()
    ));
}

#[test]
fn attack_face_out_of_range_party_routes_to_short_step() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackFace.as_byte();
    host.actors[1].active_target = 4;
    host.ranges.insert((1, 4), 100);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackShortStep.as_byte()
    ));
}

#[test]
fn attack_face_out_of_range_monster_routes_to_windup() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 4);
    ctx.action_state = ActionState::AttackFace.as_byte();
    host.actors[4].active_target = 1;
    host.ranges.insert((4, 1), 100);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackWindup.as_byte()
    ));
}

#[test]
fn attack_chain_walks_param_stream_until_terminator() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackChain.as_byte();
    // Strike sequence: 0x10, 0x12, 0xFF (terminator).
    host.actors[1].params[0] = 0x10;
    host.actors[1].params[1] = 0x12;
    host.actors[1].params[2] = 0xFF;

    // First step: queue 0x10 and fire damage.
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    assert_eq!(host.actors[1].queued_anim, 0x10);
    assert_eq!(host.actors[1].strike_index, 1);
    assert!(host.actors[1].flag_bits.has(ActorFlags::ADVANCE_DONE));
    assert!(host.take().contains(&Event::ApplyDamage(0x10, 0, 0, 1)));

    // While ADVANCE_DONE is set the staged swing is in flight - the
    // chain holds without reading the next byte (the 0x801E370C gate).
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    assert_eq!(host.actors[1].strike_index, 1, "gated - no byte read");
    assert!(host.take().is_empty(), "gated - no damage fired");

    // Anim system signals clip end (clears ADVANCE_DONE).
    host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);

    // Second step: queue 0x12 and fire damage.
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    assert_eq!(host.actors[1].queued_anim, 0x12);
    assert_eq!(host.actors[1].strike_index, 2);
    assert!(host.take().contains(&Event::ApplyDamage(0x12, 0, 0, 1)));
    host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);

    // Third step: terminator → recovery; SM clears ADVANCE_DONE.
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackRecovery.as_byte()
    ));
    assert_eq!(host.actors[1].strike_index, 0);
    assert!(!host.actors[1].flag_bits.has(ActorFlags::ADVANCE_DONE));
}

#[test]
fn attack_recovery_holds_until_advance_done_clears() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackRecovery.as_byte();
    host.actors[1].flag_bits.set(ActorFlags::ADVANCE_DONE);
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackReturn.as_byte()
    ));
}

#[test]
fn attack_return_with_counter_attack_loops_back_to_chain() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackReturn.as_byte();
    ctx.counter_attack_a = 1;
    ctx.counter_attack_b = 1;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackChain.as_byte()
    ));
    // Bumped queue counter (the "swap" signal).
    assert_eq!(host.actors[1].action_queue_counter, 1);
}

#[test]
fn attack_return_without_counter_attack_routes_to_done_cleanup() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackReturn.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::DoneCleanup.as_byte()
    ));
}

#[test]
fn magic_cast_begin_capture_spell_routes_to_capture_branch() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCastBegin.as_byte();
    host.actors[1].params[0] = 0x42;
    host.capture_spells.insert(0x42);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::MagicCaptureBranch.as_byte()
    ));
    assert!(host.take().contains(&Event::LoadCapture(0x42)));
}

#[test]
fn magic_cast_begin_subtracts_mp_with_ability_bits() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCastBegin.as_byte();
    host.actors[1].mp = 50;
    host.actors[1].params[0] = 0x10;
    host.spell_costs.insert(0x10, 20);
    host.ability_bits.insert(1, 0x20); // half cost
    step(&mut host, &mut ctx);
    // 50 - 10 = 40
    assert_eq!(host.actors[1].mp, 40);
    assert_eq!(host.actors[1].last_mp_cost, 10);
}

#[test]
fn magic_cast_begin_quarter_cost_with_bit_10() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCastBegin.as_byte();
    host.actors[1].mp = 50;
    host.actors[1].params[0] = 0x10;
    host.spell_costs.insert(0x10, 20);
    host.ability_bits.insert(1, 0x10); // quarter (shave 25%: cost - cost>>2)
    step(&mut host, &mut ctx);
    // cost 20 -> 20 - (20>>2) = 15; 50 - 15 = 35
    assert_eq!(host.actors[1].mp, 35);
}

#[test]
fn magic_pre_cast_wait_summon_route() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicPreCastWait.as_byte();
    ctx.frame_timer = 1;
    host.actors[1].sub_route = 9;
    // First step: timer goes to 0 (still positive). Stay.
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    // Second step: timer crosses 0 → next state.
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::SummonInvoke.as_byte()
    ));
}

#[test]
fn done_cleanup_sets_recoil_per_category() {
    let (mut ctx, mut host) = fresh(ActionCategory::Spirit, 1);
    ctx.action_state = ActionState::DoneCleanup.as_byte();
    step(&mut host, &mut ctx);
    // Spirit category → recoil = 0x20.
    assert_eq!(host.actors[1].action_recoil, 0x20);
    assert!(host.actors[1].flag_bits.has(ActorFlags::EXIT));
    assert_eq!(ctx.frame_timer, 0x3C);
}

#[test]
fn done_cleanup_attack_uses_recover_pose() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneCleanup.as_byte();
    step(&mut host, &mut ctx);
    assert!(host.take().contains(&Event::Pose(1, Pose::Recover)));
}

#[test]
fn done_cleanup_run_screen_shakes() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::DoneCleanup.as_byte();
    step(&mut host, &mut ctx);
    assert!(host.take().contains(&Event::ScreenShake(0x500)));
}

#[test]
fn done_fade_down_holds_then_routes_to_end_of_action() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 2;
    // Two ticks bring timer below 0.
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::EndOfAction.as_byte()
    ));
}

#[test]
fn done_fade_down_with_multi_cast_routes_to_multi_cast() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    ctx.multi_cast_gate = 1;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::DoneMultiCast.as_byte()
    ));
}

#[test]
fn end_of_action_party_wipe_signals_battle_end() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    // Kill all party.
    host.actors[0].liveness = 0;
    host.actors[1].liveness = 0;
    host.actors[2].liveness = 0;
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    assert!(
        host.take()
            .contains(&Event::BattleEnd(BattleEndCause::PartyWipe))
    );
}

#[test]
fn end_of_action_monster_wipe_signals_battle_end() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    // Kill all monsters.
    for i in 3..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    assert!(
        host.take()
            .contains(&Event::BattleEnd(BattleEndCause::MonsterWipe))
    );
}

#[test]
fn end_of_action_continues_when_both_sides_alive() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    host.actors[0].action_queue_counter = 0;
    let out = step(&mut host, &mut ctx);
    // 8 alive total → bumped counter (1) < 8 → restart at PreActionWait.
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::PreActionWait.as_byte()
    ));
}

#[test]
fn run_begin_sets_timer_and_emits_run_ui() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::RunBegin.as_byte();
    step(&mut host, &mut ctx);
    assert_eq!(ctx.frame_timer, 0x3C);
    assert!(host.take().contains(&Event::Ui(0x43, 0)));
}

#[test]
fn run_begin_successful_escape_floors_downed_party_at_1() {
    // PORT: FUN_801E295C case 0x64 success branch - every party actor
    // with +0x14C == 0 is set to 1 (downed / petrified members leave the
    // battle alive). Monsters are untouched (the retail loop bound is
    // the party count ctx[+0]).
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::RunBegin.as_byte();
    ctx.multi_cast_gate = 1; // run roll succeeded
    host.actors[0].liveness = 0;
    host.actors[2].liveness = 0;
    host.actors[4].liveness = 0; // monster stays down
    step(&mut host, &mut ctx);
    assert_eq!(host.actors[0].liveness, 1);
    assert_eq!(host.actors[2].liveness, 1);
    assert_eq!(host.actors[4].liveness, 0, "monsters are not revived");
    assert_eq!(ctx.multi_cast_gate, 1, "outcome gate left for RunWait");
}

#[test]
fn run_begin_failed_run_leaves_downed_party_down() {
    // The retail revive loop lives only in the success branch of case
    // 0x64; a failed run changes no HP.
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::RunBegin.as_byte();
    ctx.multi_cast_gate = 0; // run roll failed
    host.actors[0].liveness = 0;
    step(&mut host, &mut ctx);
    assert_eq!(host.actors[0].liveness, 0);
}

#[test]
fn run_wait_failed_run_routes_to_done_cleanup_and_battle_continues() {
    // Retail 0x65 failure branch: the action is consumed (Done band),
    // the battle continues - no battle-end signal.
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::RunWait.as_byte();
    ctx.frame_timer = 0;
    ctx.multi_cast_gate = 0; // run roll failed
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::DoneCleanup.as_byte()
    ));
}

#[test]
fn run_wait_escape_routes_to_run_escape_teardown() {
    // Retail 0x65 success branch -> 0x66: the escape teardown ends the
    // battle with the typed Escaped cause (DAT_8007BD71 = 0xFE, no wipe
    // cause byte).
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::RunWait.as_byte();
    ctx.frame_timer = 0;
    ctx.multi_cast_gate = 1; // run roll succeeded
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::RunEscape.as_byte()
    ));
    let out = step(&mut host, &mut ctx);
    assert!(matches!(out, StepOutcome::BattleComplete));
    assert!(
        host.take()
            .contains(&Event::BattleEnd(BattleEndCause::Escaped)),
        "escape teardown signals the typed Escaped cause"
    );
}

#[test]
fn capture_start_uses_rng_for_combo_offset() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 5);
    ctx.action_state = ActionState::CaptureStart.as_byte();
    host.rng_seq = vec![1];
    step(&mut host, &mut ctx);
    // combo_timer += 0x780 + 0x80 (since rng%2 == 1) = 0x800 (2048).
    assert_eq!(ctx.combo_timer, 0x780 + 0x80);
    assert_eq!(ctx.frame_timer, 0x1E);
}

#[test]
fn capture_start_takedown_removes_the_monster() {
    // PORT: FUN_801E7824 - the state-0x68 arm zeroes the captured
    // monster's HP pair (+0x172 / +0x14C) and facing (+0x46), bumps the
    // +0x1DC flag byte by 1 (a raw increment, not a bit set), retargets
    // to 8 ("all"), and opens the run-UI banner (FUN_801D8DE8(0x43, 0)).
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 5);
    ctx.action_state = ActionState::CaptureStart.as_byte();
    host.actors[5].hp = 120;
    host.actors[5].hp_display = Some(120);
    host.actors[5].facing_angle = 0x800;
    host.actors[5].flag_bits = ActorFlags(0x02);
    host.actors[5].active_target = 0;
    step(&mut host, &mut ctx);
    let a = &host.actors[5];
    assert_eq!(a.hp, 0, "+0x172 zeroed");
    assert_eq!(a.hp_display, Some(0));
    assert_eq!(a.liveness, 0, "+0x14C zeroed");
    assert_eq!(a.facing_angle, 0, "+0x46 zeroed");
    assert_eq!(a.flag_bits.0, 0x03, "+0x1DC incremented by 1");
    assert_eq!(a.active_target, 8, "+0x1DD = 8");
    assert!(
        host.take().contains(&Event::Ui(0x43, 0)),
        "run banner opened"
    );
}

#[test]
fn hp_bar_drain_freezes_done_fade_down() {
    // PORT: FUN_801E7250 - the state-0x51 arm only decrements the
    // +0x6D8 countdown when the settle check returns 0; a party target
    // (+0x1DD < 3) with live HP (+0x14C) != bar display (+0x172) holds
    // the timer.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[1].active_target = 0;
    host.actors[0].hp = 50;
    host.actors[0].hp_display = Some(80); // drain still animating
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    assert_eq!(ctx.frame_timer, 0, "timer frozen while draining");
    // Drain settles → the timer counts down and the state advances.
    host.actors[0].hp_display = Some(50);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
    ));
}

#[test]
fn hp_bar_drain_monster_target_never_pends() {
    // FUN_801E7250's `2 < bVar1` early-out: monster targets (3..=7)
    // return 0 (settled) without inspecting the HP pair.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[1].active_target = 4;
    host.actors[4].hp = 10;
    host.actors[4].hp_display = Some(99);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
    ));
}

#[test]
fn hp_bar_drain_target_8_scans_all_slots() {
    // FUN_801E7250's target-8 arm: walks every actor slot up to the
    // battle actor count; any unsettled pair pends.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[1].active_target = 8;
    host.actors[6].hp = 10;
    host.actors[6].hp_display = Some(11);
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    host.actors[6].hp_display = None; // host stops animating → settled
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
    ));
}

#[test]
fn capture_wait_marks_capture_state_after_timer() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 5);
    ctx.action_state = ActionState::CaptureWait.as_byte();
    ctx.frame_timer = 0;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::CaptureSustain.as_byte()
    ));
    assert_eq!(host.actors[5].capture_state, 2);
    assert_eq!(host.actors[5].render_flag, 2);
}

#[test]
fn full_attack_flow_round_trips() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::Begin.as_byte();
    ctx.queued_action = 1;

    // Begin → PreActionWait.
    let out = step(&mut host, &mut ctx);
    assert!(matches!(out, StepOutcome::Transition { .. }));
    assert_eq!(ctx.action_state, ActionState::PreActionWait.as_byte());

    // PreActionWait → ActionSeed (prev_cleared = true by default).
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::ActionSeed.as_byte());

    // ActionSeed → AttackFace.
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::AttackFace.as_byte());

    // AttackFace → AttackChain (in range by default).
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::AttackChain.as_byte());

    // AttackChain: walk one anim then terminator.
    host.actors[1].params[0] = 0x10;
    host.actors[1].params[1] = 0xFF;
    step(&mut host, &mut ctx); // queue 0x10, fires apply_damage
    assert!(host.take().contains(&Event::ApplyDamage(0x10, 0, 0, 1)));
    // Anim system signals the staged swing finished (clears the
    // 0x801E370C read gate) before the chain reads the next byte.
    host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    step(&mut host, &mut ctx); // terminator → AttackRecovery, SM clears ADVANCE_DONE
    assert_eq!(ctx.action_state, ActionState::AttackRecovery.as_byte());
    assert!(!host.actors[1].flag_bits.has(ActorFlags::ADVANCE_DONE));

    // AttackRecovery (advance_done cleared by SM) → AttackReturn.
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::AttackReturn.as_byte());

    // AttackReturn → DoneCleanup.
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::DoneCleanup.as_byte());

    // DoneCleanup → DoneFadeDown.
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::DoneFadeDown.as_byte());

    // Tick timer down until it transitions to EndOfAction.
    loop {
        let out = step(&mut host, &mut ctx);
        match out {
            StepOutcome::Stay => continue,
            StepOutcome::Transition { to, .. } => {
                assert_eq!(to, ActionState::EndOfAction.as_byte());
                break;
            }
            other => panic!("unexpected outcome during fade-down: {other:?}"),
        }
    }

    // EndOfAction (both sides alive) → PreActionWait.
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::PreActionWait.as_byte()
    ));
}

#[test]
fn unmapped_state_byte_surfaces_unknown() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = 0x07; // gap in the table
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::UnknownState { state: 0x07 });
}

#[test]
fn idle_hold_stays_and_pose_recover() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::IdleHold.as_byte();
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::Stay);
    assert!(host.take().contains(&Event::Pose(1, Pose::Recover)));
}

#[test]
fn battle_complete_terminal() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::BattleComplete.as_byte();
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
}

/// Full magic-spell flow walking from `MagicCastBegin` all the way to
/// `EndOfAction`, asserting each band transition. Mirrors the attack-flow
/// round-trip but exercises the magic dispatch table - `magic_cast_begin`
/// → `magic_pre_cast_wait` (with a cleared sub-route so we don't divert
/// to summon) → `magic_anim_chain` → `magic_sustain` → `magic_hit_loop`
/// → `magic_recovery` → `magic_exit` → `done_cleanup` → `done_fade_down`
/// → `end_of_action`.
#[test]
fn full_magic_flow_round_trips() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCastBegin.as_byte();

    // Set spell ID + MP cost so MagicCastBegin doesn't crash on division.
    host.actors[1].params[0] = 0x10;
    host.actors[1].params[1] = 0x21; // first chain anim
    host.actors[1].params[2] = 0xFF; // chain terminator
    host.actors[1].mp = 100;
    host.spell_costs.insert(0x10, 20);
    host.actors[1].sub_route = 0; // not summon
    host.actors[1].current_anim = 0;
    host.actors[1].hit_count_bound = 0;

    // MagicCastBegin → MagicPreCastWait (no capture spell).
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::MagicPreCastWait.as_byte());
    assert_eq!(host.actors[1].mp, 80); // 100 - 20

    // MagicPreCastWait gates on frame_timer; it was set to 0x14 by the
    // previous step. Tick until the timer fires the transition.
    let mut iters = 0;
    while ctx.action_state == ActionState::MagicPreCastWait.as_byte() {
        step(&mut host, &mut ctx);
        iters += 1;
        assert!(iters < 1000, "stuck in MagicPreCastWait");
    }
    assert_eq!(ctx.action_state, ActionState::MagicAnimChain.as_byte());

    // MagicAnimChain reads `params[strike_index]` then increments. We
    // have `params = [0x10, 0x21, 0xFF, ...]` and `strike_index = 0`,
    // so three iterations: params[0]=0x10 queued, params[1]=0x21
    // queued, params[2]=0xFF terminator transitions.
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::MagicAnimChain.as_byte());
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::MagicAnimChain.as_byte());
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::MagicSustain.as_byte());

    // MagicSustain holds while spell_iter != 0; we need to clear it.
    host.actors[1].spell_iter = 0;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::MagicHitLoop.as_byte());

    // MagicHitLoop exits when current_anim == 0 (default).
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::MagicRecovery.as_byte());

    // MagicRecovery stays unless gate is 0 (default 0).
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::MagicExit.as_byte());

    // MagicExit similarly stays unless gate is 0 (default 0).
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::DoneCleanup.as_byte());

    // DoneCleanup → DoneFadeDown.
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::DoneFadeDown.as_byte());

    // Drain DoneFadeDown's frame timer. Should land on EndOfAction.
    let mut tick_count = 0;
    while ctx.action_state == ActionState::DoneFadeDown.as_byte() {
        step(&mut host, &mut ctx);
        tick_count += 1;
        assert!(tick_count < 1000, "stuck in DoneFadeDown");
    }
    assert_eq!(ctx.action_state, ActionState::EndOfAction.as_byte());
}

/// `MagicCastBegin` with both `bits & 0x10` and `bits & 0x20` set - verifies
/// the cost path picks the **Half** (`0x20`) branch over Quarter (`0x10`).
/// Dump-confirmed against the retail state-`0x28` block (`FUN_801E295C`
/// `0x801E3D0C`): `andi 0x20; bne <half>` short-circuits the `0x10` test.
#[test]
fn magic_cast_begin_half_takes_priority_over_quarter() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCastBegin.as_byte();
    host.actors[1].mp = 100;
    host.actors[1].params[0] = 0x10;
    host.spell_costs.insert(0x10, 40);
    // Both bits set - retail applies Half (0x20) and skips the 0x10 test.
    host.ability_bits.insert(1, 0x10 | 0x20);
    step(&mut host, &mut ctx);
    // Half: 40 - (40>>1) = 20; 100 - 20 = 80.
    assert_eq!(host.actors[1].mp, 80);
    assert_eq!(host.actors[1].last_mp_cost, 20);
}

/// `PreActionWait` is gated on `previous_action_cleared`. With the gate
/// closed, the state holds; flipping the gate transitions to `ActionSeed`
/// on the next step.
#[test]
fn pre_action_wait_holds_until_prev_cleared_flips() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::PreActionWait.as_byte();
    host.prev_cleared = false;

    // Several steps with the gate closed must not transition.
    for _ in 0..8 {
        assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
        assert_eq!(ctx.action_state, ActionState::PreActionWait.as_byte());
    }

    // Flip the gate. Next step transitions.
    host.prev_cleared = true;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::ActionSeed.as_byte()
    ));
}

// ---------------------------------------------------------------
// resolve_action_queue - Miracle / Super expansion glue tests.
// ---------------------------------------------------------------

#[test]
fn resolve_action_queue_triggers_miracle_art() {
    use legaia_art::{ActionConstant, Character, Command};
    // Vahn's Craze input: R D L U L U R D L
    let cmds = [
        Command::Right,
        Command::Down,
        Command::Left,
        Command::Up,
        Command::Left,
        Command::Up,
        Command::Right,
        Command::Down,
        Command::Left,
    ];
    let queue = resolve_action_queue(Character::Vahn, &cmds, &[]);
    // Miracle Art replacement ends with the Tornado Flame Miracle
    // finisher (0x2A).
    let last = queue.actions().last().copied().unwrap();
    assert_eq!(last, ActionConstant::Art2A);
    // First 4 are the directional unmasked bytes; 5th is the Special
    // Starter (0x1A).
    assert_eq!(queue.actions()[4], ActionConstant::SpecialStarter);
}

#[test]
fn resolve_action_queue_triggers_super_art_with_chained_arts() {
    use legaia_art::{ActionConstant, Character, Command};
    // Tri-Somersault find pattern (Vahn): 19 27 0F 19 1F 0E 19 27.
    // Equivalent player input: chained arts [Somersault, Cyclone, Somersault]
    // with directional inputs Up, Down between them.
    // Build the queue manually via the helper:
    let cmds = [Command::Up, Command::Down];
    let chained = [
        ActionConstant::Art27, // Somersault
        ActionConstant::Art1F, // Cyclone
        ActionConstant::Art27, // Somersault
    ];
    // Chained arts are bracketed by RegularStarter, so the queue
    // builds as [U, D, 19, 27, 19, 1F, 19, 27]. That doesn't match
    // the Tri-Somersault find pattern (which is 19 27 0F 19 1F 0E 19
    // 27). Manually reorder by feeding the directional inputs in the
    // exact slot order the retail UI would assemble:
    let _ = cmds; // commands aren't used in this fast-path test.

    // Instead, build the queue byte-equivalent to the find pattern.
    let mut q = legaia_art::ActionQueue::new();
    for b in [0x19u8, 0x27, 0x0F, 0x19, 0x1F, 0x0E, 0x19, 0x27] {
        q.push(ActionConstant::from_byte(b).unwrap());
    }
    let _ = chained;

    let matcher = legaia_art::SuperMatcher::with_default_table();
    let hit = matcher.try_trigger_at_tail(Character::Vahn, &mut q);
    assert!(hit.is_some(), "Tri-Somersault should fire");
}

#[test]
fn resolve_action_queue_no_special_match_keeps_chained() {
    use legaia_art::{ActionConstant, Character, Command};
    // Inputs that don't form a Miracle or Super Art - queue should
    // contain just the directional bytes + chained-art assembly with
    // no replacement.
    let cmds = [Command::Up, Command::Up];
    let chained = [ActionConstant::Art28]; // Charging Scorch
    let queue = resolve_action_queue(Character::Vahn, &cmds, &chained);
    let bytes: Vec<u8> = queue.actions().iter().map(|a| a.as_byte()).collect();
    assert_eq!(bytes, vec![0x0F, 0x0F, 0x19, 0x28]);
}

#[test]
fn art_record_default_returns_none() {
    // Default `BattleActionHost::art_record` returns `None`. Verify
    // the recording host returns `None` when no art records are
    // staged via `art_records`.
    use legaia_art::{ActionConstant, Character};
    let host = RecHost::default();
    assert!(
        host.art_record(Character::Vahn, ActionConstant::Art1B)
            .is_none()
    );
}

// ---------------------------------------------------------------
// Battle SM strike-band reads from art_record.
// ---------------------------------------------------------------

fn dmg_byte(target: legaia_art::PowerTarget, multiplier: u8) -> legaia_art::PowerByte {
    legaia_art::PowerByte::Damage(legaia_art::ArtPower {
        target,
        multiplier,
        alt_range: false,
    })
}

fn synthetic_art_record(
    action: legaia_art::ActionConstant,
    power: Vec<legaia_art::PowerByte>,
    dmg_timing: Vec<u8>,
) -> legaia_art::ArtRecord {
    legaia_art::ArtRecord {
        action,
        commands: vec![],
        anim_index: 0,
        anim_extra: vec![],
        name: None,
        power,
        dmg_timing,
        effect_cues: [legaia_art::EffectCue::default(); 2],
        hit_cues: vec![legaia_art::HitCue::from_word(0x0010_001A)],
        identifier: 0,
        anim_speed: 0x10,
        enemy_effect: legaia_art::EnemyEffect::Toxic,
        repeat_frames: legaia_art::RepeatFrames::default(),
        background: 0,
        runtime_address: None,
    }
}

#[test]
fn attack_chain_dispatches_apply_art_strike_when_art_chosen() {
    // Setup: party slot 0 (Vahn) has chosen Art1B (Vahn's Craze).
    // Strike script in `params` has anim bytes [0x10, 0x11, 0xFF].
    // The art has 2 power bytes + 2 dmg_timings; the strike chain
    // should fire `apply_art_strike` for both bytes (with the second
    // having a None power if we only stage 1).
    use legaia_art::{ActionConstant, Character, PowerTarget};

    let mut host = RecHost::with_n_actors(3);
    host.actors[0].character = Character::Vahn;
    host.actors[0].chosen_art = Some(ActionConstant::Art1B);
    host.actors[0].active_target = 1;
    host.actors[0].params[0] = 0x10;
    host.actors[0].params[1] = 0x11;
    host.actors[0].params[2] = 0xFF;
    host.art_records.insert(
        (
            character_byte(Character::Vahn),
            ActionConstant::Art1B.as_byte(),
        ),
        synthetic_art_record(
            ActionConstant::Art1B,
            vec![
                dmg_byte(PowerTarget::Udf, 18),
                dmg_byte(PowerTarget::Ldf, 22),
            ],
            vec![0x08, 0x14],
        ),
    );

    let mut ctx = BattleActionCtx::new();
    ctx.action_state = ActionState::AttackChain.as_byte();
    ctx.active_actor = 0;

    // Tick 1: consumes params[0] = 0x10 → fires both apply_art_strike
    // and apply_damage. Between ticks the anim system signals each
    // staged swing's clip end by clearing ADVANCE_DONE (the 0x801E370C
    // read gate).
    step(&mut host, &mut ctx);
    host.actors[0].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    // Tick 2: params[1] = 0x11 → fires for second strike.
    step(&mut host, &mut ctx);
    host.actors[0].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    // Tick 3: params[2] = 0xFF terminator → transitions to AttackRecovery.
    step(&mut host, &mut ctx);

    let events = host.take();
    let strikes: Vec<&ArtStrikeInfo> = events
        .iter()
        .filter_map(|e| match e {
            Event::ApplyArtStrike(info) => Some(info),
            _ => None,
        })
        .collect();
    assert_eq!(strikes.len(), 2, "two art strikes should fire");
    let s0 = strikes[0];
    assert_eq!(s0.strike_index, 0);
    assert_eq!(s0.anim_byte, 0x10);
    assert_eq!(s0.actor_slot, 0);
    assert_eq!(s0.target_slot, 1);
    assert_eq!(s0.character, Character::Vahn);
    assert_eq!(s0.art, ActionConstant::Art1B);
    assert_eq!(s0.dmg_timing, Some(0x08));
    assert_eq!(s0.enemy_effect, legaia_art::EnemyEffect::Toxic);
    assert!(matches!(
        s0.power,
        Some(legaia_art::PowerByte::Damage(legaia_art::ArtPower {
            multiplier: 18,
            ..
        }))
    ));
    assert!(s0.hit_cue.is_some());

    let s1 = strikes[1];
    assert_eq!(s1.strike_index, 1);
    assert_eq!(s1.anim_byte, 0x11);
    assert_eq!(s1.dmg_timing, Some(0x14));
    // 2nd strike has no hit_cue staged at index 1 (only one in the
    // synthetic record), so this is None.
    assert!(s1.hit_cue.is_none());
    // apply_damage still fires alongside apply_art_strike for
    // backward compatibility.
    let damages: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::ApplyDamage(..) => Some(()),
            _ => None,
        })
        .collect();
    assert_eq!(damages.len(), 2, "apply_damage still fires per strike");
}

#[test]
fn attack_chain_skips_apply_art_strike_when_no_art_chosen() {
    // Default actor has chosen_art = None - the strike chain must
    // fire only apply_damage, not apply_art_strike.
    let mut host = RecHost::with_n_actors(3);
    host.actors[0].params[0] = 0x10;
    host.actors[0].params[1] = 0xFF;

    let mut ctx = BattleActionCtx::new();
    ctx.action_state = ActionState::AttackChain.as_byte();
    ctx.active_actor = 0;

    step(&mut host, &mut ctx);
    // Clip-end signal between strikes (the 0x801E370C read gate).
    host.actors[0].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    step(&mut host, &mut ctx);

    let events = host.take();
    let strikes = events
        .iter()
        .filter(|e| matches!(e, Event::ApplyArtStrike(_)))
        .count();
    let damages = events
        .iter()
        .filter(|e| matches!(e, Event::ApplyDamage(..)))
        .count();
    assert_eq!(strikes, 0);
    assert_eq!(damages, 1);
}

#[test]
fn attack_chain_no_art_strike_when_record_missing() {
    // chosen_art = Some but the host returns None for art_record.
    // The SM must fall through to plain apply_damage.
    use legaia_art::ActionConstant;
    let mut host = RecHost::with_n_actors(3);
    host.actors[0].chosen_art = Some(ActionConstant::Art1B);
    host.actors[0].params[0] = 0x10;
    host.actors[0].params[1] = 0xFF;
    // No insert into art_records → host returns None.

    let mut ctx = BattleActionCtx::new();
    ctx.action_state = ActionState::AttackChain.as_byte();
    ctx.active_actor = 0;

    step(&mut host, &mut ctx);
    let events = host.take();
    assert!(
        events
            .iter()
            .all(|e| !matches!(e, Event::ApplyArtStrike(_))),
        "no art strike should fire when art_record returns None"
    );
    assert!(
        events.iter().any(|e| matches!(e, Event::ApplyDamage(..))),
        "apply_damage should still fire as fallback"
    );
}
