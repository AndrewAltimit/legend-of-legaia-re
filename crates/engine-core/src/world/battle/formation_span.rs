//! The per-round formation squash + recentre - retail's battle-flow case `0`
//! (`FUN_801D388C`, `jal 0x801DB318` at `0x801D3908`), reached from the round
//! start `0x801D0EE4` right after the actor sweep `FUN_801D88CC` and the
//! initiative seeder `FUN_801DA780`, and again from the ring cancelled back to
//! the round prompt (case `2`, `0x801D11E0`).
//!
//! The kernel is `legaia_engine_battle_vm::battle_action::normalize_formation_span`;
//! this module maps the engine's actor table onto the retail pool it walks.
//!
//! ## The two pools differ in layout, not in seating
//!
//! Retail seats party member `i` in pool slot `i` and monster `k` in pool slot
//! `3 + k` whatever the party size (`FUN_800513F0`: the party loop indexes
//! `0x801C9370 + i*4`, the monster loop `0x801C9370 + (k+3)*4` at
//! `0x8005185C`), so a party of one leaves slots `1` and `2` empty. The engine
//! compacts monsters down to `party_count + k`. The seat each combatant gets is
//! the same on both - party member `i` takes row `party_count` entry `i`,
//! monster `k` takes row `monster_count` entry `k` - so the difference is only
//! an index space, and this walk converts it: engine party slot `i` is retail
//! slot `i`, engine monster slot `party_count + k` is retail slot `3 + k`.
//!
//! The kernel includes slots `0..3` **unconditionally** and slots `3..7` only
//! with a live `+0x14C`, so an absent party seat still contributes its
//! position to the extents. Those actors are at the origin in every catalogued
//! capture of a fresh small-party battle (five solo / duo states), and the
//! origin lies inside every authored formation's box - so the engine passes
//! them as `(0, 0)` and the phantom slots cannot move the result - at the
//! first round. Each empty seat is a struct of its own (a solo capture's
//! table holds distinct pointers at slots `1` and `2`, `HP 0`), which this
//! walk, the action seed's recentre and the melee make-way step all move, so
//! later in a fight the empty seats stand off the origin and weigh in the
//! box (`gimard_summon_start` reads both at `(-82, 543)` against the lone
//! member's `(82, -542)`, which centres that box on the origin). The engine keeps them in
//! [`BattleState::empty_seat_ground`] and moves them with the rest.
//!
//! ## What it does to a fight
//!
//! Nothing walks a combatant home after an action
//! ([`World::tick_battle_locomotion`]), so the formation wanders over a fight;
//! this pass pulls it back each round. Its visible effect on an authored
//! formation is the recentre alone: a party of three against one monster
//! spans `z = -825 ..= 800`, whose centroid `(-25 as u32) >> 1` truncates to
//! `-13`, and every combatant moves `+13` in Z. Every catalogued three-on-one
//! capture reads exactly that (party `z = -812 / -762`, monster `z = 813`),
//! which is where an earlier "uniform mid-battle drift" reading came from.
//!
//! ## The seat follows
//!
//! The kernel writes only the live pair `+0x34` / `+0x38`. The engine's seat
//! pair (`BattleActor::seat`, the reference the range law and the separation
//! pass measure against) is shifted by the same per-actor delta. Retail's
//! `+0x3C` / `+0x40` tracks the live pair at a fixed pose offset rather than
//! staying where the formation was: the Tetsu fight reads the solo party at
//! live `z = -800`, `+0x3C` pair `-862`, and a recentred three-on-one reads the
//! lead at live `-812`, pair `-874` - the same `-62` apart, moved `+13`
//! together. Leaving the engine's seat behind would put an attacker's range
//! reference a formation shift away from the body it walks at, and the
//! approach would never arrive.
//!
//! The kernel's other output, the camera-focus shift on `_DAT_80089118` /
//! `_DAT_80089120`, is taken into locals and dropped: the flow case that runs
//! the squash immediately re-arms the far framing `FUN_801D5854(0, 9)`
//! (`0x801D4908`), which re-derives its focus from the live actor table, and
//! the engine's far framing reads the same live table every pass
//! ([`crate::battle_cam_inputs::battle_formation_box`]).

use super::*;
use legaia_engine_battle_vm::battle_action::{
    FormationPos, normalize_formation_span, seed_formation_recentre,
};

/// Retail's pool width the kernel walks (`sltiu v0,v0,0x7` at `0x801DB3D4`):
/// party slots `0..3`, monster slots `3..7`.
const RETAIL_POOL_WALK: usize = 7;

/// The action category whose recentre frames the target alone (`+0x1DE ==
/// 3`, Attack; `li v0,0x3` in `FUN_801EFE44`'s box loop).
const ATTACK_CATEGORY: u8 = 3;

impl World {
    /// The retail pool slot an engine battle slot occupies, or `None` for a
    /// slot retail's seven-slot walk does not reach (a fifth monster, or an
    /// actor past the combatant table).
    pub fn retail_battle_pool_slot(&self, engine_slot: usize) -> Option<usize> {
        let seats = self.battle_seat_map();
        let e = u8::try_from(engine_slot).ok()?;
        // A monster seat carries its id; one a fixture seated without an id
        // (`World::enter_battle` alone) is still a live seated body.
        let monster = engine_slot < 8
            && self.actors.get(engine_slot).is_some_and(|a| {
                a.battle_monster_id.is_some() || (a.active && a.battle.liveness != 0)
            });
        if !seats.is_party(e) && !monster {
            return None;
        }
        let retail = usize::from(seats.retail(e).get());
        (retail < RETAIL_POOL_WALK).then_some(retail)
    }

    /// Run the round-start formation squash + recentre over the live battle
    /// positions.
    ///
    /// REF: FUN_801D388C (flow case `0` / `2`: the `jal 0x801DB318` at
    /// `0x801D3908`; the `FUN_801D5854(0, 9)` re-frame that follows is the
    /// engine's per-pass far framing)
    pub(in crate::world) fn normalize_battle_formation(&mut self) {
        self.move_battle_pool(|pos, alive| {
            // The camera-focus pair: see the module docs for why it is local.
            let (mut focus_x, mut focus_z) = (0i32, 0i32);
            normalize_formation_span(pos, alive, &mut focus_x, &mut focus_z);
        });
    }

    /// Run the action seed's formation recentre `FUN_801EFE44` for the
    /// acting engine slot `actor`: the round-start squash, then the formation
    /// moved so the action's box - the target alone for an Attack - sits on
    /// the stage origin. Retail runs it at state `0x0C` for every category
    /// but Run (`0x801E2D44`) and on the monster Run arm (`0x801E2DE8`).
    ///
    /// The camera-focus shift is dropped as the round-start one is: the
    /// seed's next call is the action framing `FUN_801D5854(actor, 6)`,
    /// which frames the acting actor's live position.
    ///
    /// REF: FUN_801EFE44
    pub(in crate::world) fn seed_battle_formation_recentre(&mut self, actor: u8) {
        if self.battle.seed_recentre_held {
            return;
        }
        let Some(a) = self.actors.get(usize::from(actor)) else {
            return;
        };
        let attack = a.battle.action_category == ATTACK_CATEGORY;
        let target_engine = a.battle.active_target;
        let target = if target_engine >= 8 {
            target_engine
        } else {
            match self.retail_battle_pool_slot(usize::from(target_engine)) {
                Some(r) => r as u8,
                // A target off the pool frames nothing: the box is empty and
                // the recentre is zero.
                None => 0xFF,
            }
        };
        self.move_battle_pool(|pos, alive| {
            let (mut focus_x, mut focus_z) = (0i32, 0i32);
            seed_formation_recentre(pos, alive, attack, target, &mut focus_x, &mut focus_z);
        });
    }

    /// Lift the live battle positions onto retail's seven-slot pool, let
    /// `f` move them, and write the result back, carrying each actor's seat
    /// pair along by the same delta (the module docs' "The seat follows").
    fn move_battle_pool(&mut self, f: impl FnOnce(&mut [FormationPos], &[bool])) {
        // An empty seat stands where the walks left its struct.
        let mut pos = self
            .battle
            .empty_seat_ground
            .map(|(x, z)| FormationPos { x, z });
        let mut alive = [false; RETAIL_POOL_WALK];
        let mut owner = [None::<usize>; RETAIL_POOL_WALK];
        for (slot, a) in self.actors.iter().enumerate().take(8) {
            let Some(r) = self.retail_battle_pool_slot(slot) else {
                continue;
            };
            pos[r] = FormationPos {
                x: a.move_state.world_x,
                z: a.move_state.world_z,
            };
            alive[r] = a.battle.liveness != 0;
            owner[r] = Some(slot);
        }
        f(&mut pos, &alive);
        for (r, slot) in owner.iter().enumerate() {
            if slot.is_none() {
                self.battle.empty_seat_ground[r] = (pos[r].x, pos[r].z);
            }
            if let Some(slot) = *slot
                && let Some(a) = self.actors.get_mut(slot)
            {
                let dx = pos[r].x.wrapping_sub(a.move_state.world_x);
                let dz = pos[r].z.wrapping_sub(a.move_state.world_z);
                a.move_state.world_x = pos[r].x;
                a.move_state.world_z = pos[r].z;
                // The seat pair moves with the live pair: see the module
                // docs' "The seat follows".
                if let Some((sx, sz)) = a.battle.seat.as_mut() {
                    *sx = sx.wrapping_add(dx);
                    *sz = sz.wrapping_add(dz);
                }
            }
        }
    }
    /// The live pair of retail pool slot `pool`: its engine actor's, or the
    /// empty seat's ([`BattleState::empty_seat_ground`]).
    pub fn battle_pool_position(&self, pool: u8) -> Option<(i16, i16)> {
        if usize::from(pool) >= RETAIL_POOL_WALK {
            return None;
        }
        match self.battle_pool_owner(pool) {
            Some(slot) => self
                .actors
                .get(slot)
                .map(|a| (a.move_state.world_x, a.move_state.world_z)),
            None => Some(self.battle.empty_seat_ground[usize::from(pool)]),
        }
    }

    /// The mutation half of [`Self::battle_pool_position`].
    pub fn set_battle_pool_position(&mut self, pool: u8, x: i16, z: i16) {
        if usize::from(pool) >= RETAIL_POOL_WALK {
            return;
        }
        match self.battle_pool_owner(pool) {
            Some(slot) => {
                if let Some(a) = self.actors.get_mut(slot) {
                    a.move_state.world_x = x;
                    a.move_state.world_z = z;
                }
            }
            None => self.battle.empty_seat_ground[usize::from(pool)] = (x, z),
        }
    }

    /// Stand empty pool seat `pool` at `(x, z)` - a capture seed's hand-off
    /// of where retail's struct stands. A seat an actor occupies is left to
    /// the actor.
    pub fn seat_empty_battle_seat(&mut self, pool: u8, x: i16, z: i16) {
        if usize::from(pool) < RETAIL_POOL_WALK && self.battle_pool_owner(pool).is_none() {
            self.battle.empty_seat_ground[usize::from(pool)] = (x, z);
        }
    }

    /// The engine actor seated in retail pool slot `pool`, if any.
    fn battle_pool_owner(&self, pool: u8) -> Option<usize> {
        let e = usize::from(self.battle_seat_map().engine(pool)?);
        (self.retail_battle_pool_slot(e) == Some(usize::from(pool))).then_some(e)
    }

    /// Re-seat the monster block on seat family `family` of the table at
    /// `0x80077608`.
    ///
    /// `FUN_800513F0` picks the monster row as `ctx[+1] + sp[+0x20] + s4`
    /// (`0x80051838..0x8005184C`), two independent `+4` addends:
    ///
    /// * `sp[+0x20]` is `(DAT_8007BD60 >> 5) & 4` (`0x80051430`, the same word
    ///   it stores to the scripted-fight byte `ctx[+0x287]`);
    /// * `s4 = 4` is the map-gated arm (`0x800517A0..0x800517E0`): the loaded
    ///   scene's define `_DAT_80084540` is `0x0C` / `0x15` (`town0b` /
    ///   `town0c`) and the first monster id `0x8007BD0C` is in `0x3D..=0x3F`.
    ///
    /// `family` counts the addends that fire. One selects rows `5..8`
    /// ([`legaia_engine_battle::battle_seats::MONSTER_SEATS_ALT`]; counts one and two are
    /// authored identical to the normal family, three and four differ). Both
    /// select rows `9..12`, which the disc leaves zero-filled - every monster
    /// on the stage origin - and the port reproduces that.
    ///
    /// REF: FUN_800513F0 (the monster row index, `0x80051838..0x8005184C`)
    pub(in crate::world) fn seat_monster_family(&mut self, monster_count: u8, family: u8) {
        if family == 0 {
            return;
        }
        let pc = usize::from(self.party.party_count.clamp(1, 3));
        for k in 0..usize::from(monster_count) {
            let s = if family == 1 {
                legaia_engine_battle::battle_seats::monster_seat(monster_count, k, true)
            } else {
                legaia_engine_battle::battle_seats::Seat { x: 0, y: 0, z: 0 }
            };
            let Some(a) = self.actors.get_mut(pc + k) else {
                break;
            };
            a.move_state.world_x = s.x;
            a.move_state.world_y = s.y;
            a.move_state.world_z = s.z;
        }
    }
}
