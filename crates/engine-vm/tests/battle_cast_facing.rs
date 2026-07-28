//! The cast-begin facing store, driven through the action state machine
//! rather than through its kernels.
//!
//! Retail's `0x801E4334..0x801E43A4` is one block with two arms - a target
//! slot's seat, or the centroid `FUN_801DCEAC` folds out of a target-group
//! code - and both end at the same bearing / half-turn / mask / `+0x46` store.
//! These tests step `battle_action::step` into `MagicCastBegin` and read the
//! actor's facing back, so what is under test is the SM arm, not
//! `target_group_aim` on its own.
//!
//! REF: FUN_801E295C (`0x801E4334..0x801E43A4`), FUN_801DCEAC

use legaia_engine_vm::battle_action::{
    ACTOR_SLOTS, ActionState, BattleActionCtx, BattleActionHost, BattleActor, step,
};
use legaia_engine_vm::battle_target_group::RENDER_FLAG_HIDDEN;

/// Minimal host with seats: the only thing beyond the default trait bodies is
/// the actor table and [`BattleActionHost::actor_position`].
struct SeatedHost {
    actors: Vec<BattleActor>,
    seats: Vec<Option<(i16, i16)>>,
    party_count: u8,
}

impl SeatedHost {
    /// `party_count` party slots then `monster_count` monster slots, every one
    /// alive and drawn, seated where the caller says.
    fn new(party_count: u8, seats: &[(i16, i16)]) -> Self {
        SeatedHost {
            actors: seats
                .iter()
                .map(|_| BattleActor {
                    liveness: 1,
                    ..Default::default()
                })
                .collect(),
            seats: seats.iter().map(|&s| Some(s)).collect(),
            party_count,
        }
    }
    fn cast(&mut self, actor: u8, target: u8) -> u16 {
        self.actors[actor as usize].action_category = 2; // Magic
        self.actors[actor as usize].active_target = target;
        let mut ctx = BattleActionCtx {
            action_state: ActionState::MagicCastBegin.as_byte(),
            active_actor: actor,
            ..Default::default()
        };
        step(self, &mut ctx);
        self.actors[actor as usize].facing_angle
    }
}

impl BattleActionHost for SeatedHost {
    fn actor(&self, slot: u8) -> Option<&BattleActor> {
        self.actors.get(slot as usize)
    }
    fn actor_mut(&mut self, slot: u8) -> Option<&mut BattleActor> {
        self.actors.get_mut(slot as usize)
    }
    fn actor_position(&self, slot: u8) -> Option<(i16, i16)> {
        self.seats.get(slot as usize).copied().flatten()
    }
    fn party_count(&self) -> u8 {
        self.party_count
    }
}

/// Three party seats at the retail-shaped negative-Z row, four monsters facing
/// them. Party 0 sits on the origin so the expected bearings are readable.
fn standard_field() -> SeatedHost {
    SeatedHost::new(
        3,
        &[
            (0, 0),       // party 0 - the caster
            (-700, -100), // party 1
            (700, -100),  // party 2
            (0, 800),     // monster 0 - due +Z
            (800, 0),     // monster 1 - due +X
            (0, -800),    // monster 2 - due -Z
            (-800, 0),    // monster 3 - due -X
        ],
    )
}

/// The single-target arm: `bearing(target.z, target.x, actor.z, actor.x)` then
/// `+ 0x800`, masked - which lands on the compass direction of the target.
#[test]
fn cast_begin_faces_a_single_target_slot() {
    // 12-bit circle: +Z is 0x000, +X is 0x400, -Z is 0x800, -X is 0xC00.
    for (target, want) in [(3u8, 0x000u16), (4, 0x400), (5, 0x800), (6, 0xC00)] {
        let mut host = standard_field();
        assert_eq!(host.cast(0, target), want, "target slot {target}");
    }
}

/// Retail's `beq v0, t2` at `0x801E4350`: an actor targeting itself keeps
/// whatever facing it had.
#[test]
fn an_actor_targeting_itself_keeps_its_facing() {
    let mut host = standard_field();
    host.actors[0].facing_angle = 0x123;
    assert_eq!(host.cast(0, 0), 0x123);
}

/// The group arm. Code `9` is the enemy row (retail slots `3..7`); the four
/// monsters above are symmetric about the origin, so their centroid is the
/// origin and the bearing degenerates - move them all to `+X` and the caster
/// must turn to `0x400`.
#[test]
fn cast_begin_faces_a_target_group_centroid() {
    let mut host = SeatedHost::new(
        3,
        &[
            (0, 0),
            (-700, -100),
            (700, -100),
            (900, -300),
            (900, -100),
            (900, 100),
            (900, 300),
        ],
    );
    assert_eq!(host.cast(0, 9), 0x400);
    // Code `8` is the party. Its centroid is `x = (0 + -700 + 700)/3 = 0`,
    // `z = (0 + -100 + -100)/3 = -66`, so from the monster at `(900, -300)` it
    // lies mostly at `-X` and a little at `+Z` - past `0xC00`, short of a full
    // turn.
    let facing = host.cast(3, 8);
    assert!(
        (0xC00..=0xFFF).contains(&facing),
        "party centroid is left of and slightly ahead of the monster: {facing:#x}"
    );
}

/// The `+0x21C` gate: a monster hidden by the summon fade drops out of the
/// centroid, which moves the answer.
#[test]
fn a_hidden_monster_is_dropped_from_the_group_centroid() {
    let mut host = SeatedHost::new(3, &[(0, 0), (-700, -100), (700, -100), (800, 0), (-800, 0)]);
    // Both monsters live: centroid is the origin, and `bearing_12bit` reports
    // `0` for a zero delta, so the store lands on the half-turn itself.
    assert_eq!(host.cast(0, 9), 0x800);
    // Hide the -X one and the group collapses onto +X.
    host.actors[4].render_flag = RENDER_FLAG_HIDDEN;
    assert_eq!(host.cast(0, 9), 0x400);
}

/// An all-dead group has no centroid, so retail's divide never runs and the
/// facing is left where it was.
#[test]
fn an_all_hidden_group_leaves_the_facing_alone() {
    let mut host = SeatedHost::new(3, &[(0, 0), (-700, -100), (700, -100), (800, 0), (-800, 0)]);
    host.actors[3].render_flag = RENDER_FLAG_HIDDEN;
    host.actors[4].render_flag = RENDER_FLAG_HIDDEN;
    host.actors[0].facing_angle = 0x321;
    assert_eq!(host.cast(0, 9), 0x321);
}

/// The group walk is indexed in retail numbering, so a reduced party still
/// selects the right band: with two party members the engine seats monsters at
/// slot 2, and code `9` must still reach them.
#[test]
fn group_codes_index_retail_slots_under_a_reduced_party() {
    let mut host = SeatedHost::new(2, &[(0, 0), (-700, -100), (900, -200), (900, 200)]);
    // Monsters are engine slots 2 and 3 = retail slots 3 and 4.
    assert_eq!(host.cast(0, 9), 0x400);
    // And code `8` (retail 0..3) must not pull them in: with only two party
    // members the third retail slot is empty, so the centroid is the party's.
    let facing = host.cast(0, 8);
    assert!(
        (0x800..=0xFFF).contains(&facing),
        "party 1 is at -X/-Z of the caster: {facing:#x}"
    );
}

/// A host that reports no seats gets the pre-accessor behaviour: the SM still
/// runs, and the facing is untouched.
#[test]
fn a_host_without_positions_leaves_the_facing_untouched() {
    let mut host = standard_field();
    host.seats = vec![None; ACTOR_SLOTS];
    host.actors[0].facing_angle = 0x0ABC;
    assert_eq!(host.cast(0, 3), 0x0ABC);
}
