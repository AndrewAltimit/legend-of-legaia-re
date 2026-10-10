//! The strike band's per-pass facing store, driven through the action state
//! machine.
//!
//! States `0x1E` (the strike loop) and `0x1F` (the recovery wait) each store
//! `(bearing + 0x800) & 0xFFF` into the acting actor's `+0x46` on every pass,
//! after the framing call and before the `+0x1DC` bit-1 test, measured from
//! the attacker's live pair `+0x34` / `+0x38` to the target's **body pair**
//! `+0x3C` / `+0x40`. The approach states measure to the target's live pair.
//!
//! REF: FUN_801E295C (`0x801E36F0..0x801E371C`, `0x801E3AD0..0x801E3AFC`)

use legaia_engine_vm::battle_action::{
    ActionState, ActorFlags, BattleActionCtx, BattleActionHost, BattleActor, step,
};

/// A host whose actors carry a live pair and a separate body pair.
struct PairedHost {
    actors: Vec<BattleActor>,
    live: Vec<(i16, i16)>,
    body: Vec<(i16, i16)>,
}

impl PairedHost {
    /// Party slot 0 on the origin, one monster whose live pair is due `+Z`
    /// and whose body pair is due `+X`.
    fn new() -> Self {
        let mut actors = vec![
            BattleActor {
                liveness: 1,
                ..Default::default()
            };
            2
        ];
        actors[0].active_target = 1;
        actors[0].action_category = 3; // Attack
        PairedHost {
            actors,
            live: vec![(0, 0), (0, 800)],
            body: vec![(0, 0), (800, 0)],
        }
    }
    fn step_in(&mut self, state: ActionState) -> u16 {
        let mut ctx = BattleActionCtx {
            action_state: state.as_byte(),
            active_actor: 0,
            ..Default::default()
        };
        step(self, &mut ctx);
        self.actors[0].facing_angle
    }
}

impl BattleActionHost for PairedHost {
    fn actor(&self, slot: u8) -> Option<&BattleActor> {
        self.actors.get(slot as usize)
    }
    fn actor_mut(&mut self, slot: u8) -> Option<&mut BattleActor> {
        self.actors.get_mut(slot as usize)
    }
    fn actor_position(&self, slot: u8) -> Option<(i16, i16)> {
        self.live.get(slot as usize).copied()
    }
    fn actor_anchor(&self, slot: u8) -> Option<(i16, i16)> {
        self.body.get(slot as usize).copied()
    }
    fn party_count(&self) -> u8 {
        1
    }
}

/// 12-bit circle: `+Z` is `0x000`, `+X` is `0x400`.
const TOWARDS_LIVE: u16 = 0x000;
const TOWARDS_BODY: u16 = 0x400;

#[test]
fn the_strike_loop_faces_the_targets_body_pair_while_a_swing_is_in_flight() {
    let mut host = PairedHost::new();
    host.actors[0].facing_angle = 0x123;
    // A swing in flight (`+0x1DC` bit 1): the pass stages nothing, and the
    // store still runs - it sits ahead of the bit test.
    host.actors[0].flag_bits.set(ActorFlags::ADVANCE_DONE);
    assert_eq!(host.step_in(ActionState::AttackChain), TOWARDS_BODY);
}

#[test]
fn the_strike_loop_follows_a_target_its_hits_move() {
    let mut host = PairedHost::new();
    host.actors[0].flag_bits.set(ActorFlags::ADVANCE_DONE);
    assert_eq!(host.step_in(ActionState::AttackChain), TOWARDS_BODY);
    // The target's body moves round to `-X`; the next pass turns with it.
    host.body[1] = (-800, 0);
    assert_eq!(host.step_in(ActionState::AttackChain), 0xC00);
}

#[test]
fn the_recovery_wait_faces_the_targets_body_pair() {
    let mut host = PairedHost::new();
    host.actors[0].flag_bits.set(ActorFlags::ADVANCE_DONE);
    assert_eq!(host.step_in(ActionState::AttackRecovery), TOWARDS_BODY);
}

#[test]
fn the_approach_states_still_face_the_targets_live_pair() {
    let mut host = PairedHost::new();
    assert_eq!(host.step_in(ActionState::AttackFace), TOWARDS_LIVE);
}

#[test]
fn a_group_target_code_leaves_the_heading_alone() {
    let mut host = PairedHost::new();
    host.actors[0].facing_angle = 0x123;
    host.actors[0].active_target = 8;
    host.actors[0].flag_bits.set(ActorFlags::ADVANCE_DONE);
    assert_eq!(host.step_in(ActionState::AttackChain), 0x123);
}
