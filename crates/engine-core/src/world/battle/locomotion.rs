//! Battle-actor locomotion: the two retail per-frame position passes the
//! action SM does not own, plus the engine's copy of the retail range law.
//!
//! Retail splits battle movement across three sites, none of which is the
//! state machine's own arm bodies:
//!
//! - **The approach drive is animation root motion.** The battle anim-node
//!   tick `FUN_80047430` (`0x80047D20..0x80047E18`) steps a clip-playing
//!   actor's live position pair (`+0x34`/`+0x38`) along its facing by
//!   `trig * entry_speed * frame_dt * actor[+0x21D] >> 15` per tick - gated,
//!   for a positive speed, on the range check `FUN_8004E2F0` still failing,
//!   so the walk stops itself exactly on arrival. The SM's approach states
//!   only stage the walk clip and poll the range.
//!   [`World::tick_battle_locomotion`] is that drive's engine slot. There is
//!   **no walk-home leg**: an action leaves its combatants standing where it
//!   put them, and the seat pair the range law measures against is re-taken
//!   from the live pair at `DoneCleanup` - see
//!   [`World::tick_battle_locomotion`] for the capture evidence.
//! - **The separation pass** `FUN_80051078` / `FUN_80050BB8` runs on the
//!   line after the action SM, every live battle frame (`FUN_80046A20`:
//!   `jal 0x801E295C; jal 0x80051078`). [`World::tick_battle_separation`]
//!   sits at the same point in `live_battle_tick`, driving the
//!   `legaia_engine_vm::battle_separation` kernels.
//! - **The range law** is computed, not tabulated: `FUN_8004E2F0`
//!   (`legaia_engine_vm::battle_action::motion::range_metric`).
//!   [`World::battle_range_metric`] assembles its inputs from live engine
//!   state; the `BattleActionHost::range_check` impl delegates here.
//!
//! REF: FUN_80047430 (root-motion drive), FUN_80046A20 (per-frame ordering)
//! REF: FUN_8004E2F0 (range law), FUN_80051078 / FUN_80050BB8 (separation)

use super::*;
use vm::battle_action::motion;

/// Fallback root-motion pair `(entry_speed, +0x21D scale)` used when the
/// approaching actor's committed clip carries no entry-speed halfword (no
/// clip installed, or a head too short to carry `+0xC`). The values are the
/// captured retail Move drive (Gaza: entry `+0xC = 20`, `+0x21D = 8`,
/// ~19-20 units/vsync - `docs/subsystems/battle-action.md`, the `0x19` park
/// analysis). Driving the approach even without a clip is deliberate: it is
/// the engine-native form of the retail approach-park guard
/// (`legaia-patcher --approach-softlock-fix`) - an approach state always
/// closes.
const FALLBACK_APPROACH: (i16, u8) = (20, 8);

/// Retail-normal `actor[+0x21D]` speed scale (see
/// `legaia_asset::monster_archive` - the anim cursor doc pins normal `4`),
/// used when the actor's own `impact_step` byte is unset.
const DEFAULT_SPEED_SCALE: u8 = 4;

/// Separation body radius of a party slot. Retail reads
/// `(*(actor+0x22C))[+0x58]`; the enemy stager derives a monster's from its
/// size class as `size << 5`, but the party constant is not pinned in the
/// dumped corpus - the engine uses the roster-minimum monster radius
/// (size class 14, the smallest on the disc). Party seats sit >= 600 units
/// apart, so at threshold `(r1+r2)/6` this choice fires no party-party
/// nudge from authored seats either way.
const PARTY_SEPARATION_RADIUS: i16 = 14 << 5;

/// Byte offset of the root-motion speed halfword inside a committed clip's
/// effect-script head (the per-action entry's `+0xC`, carried by
/// `MonsterAnimation::effect_script`).
const ENTRY_SPEED_OFFSET: usize = 0xC;

impl World {
    /// The monster record `+0x1F` size class seated in `slot`, `0` for a
    /// party slot / empty slot / unresolved catalog (the same resolution the
    /// battle host's `monster_size_class` performs).
    fn battle_size_class_of(&self, slot: u8) -> u8 {
        let Some(id) = self
            .actors
            .get(slot as usize)
            .and_then(|a| a.battle_monster_id)
        else {
            return 0;
        };
        self.monster_catalog.get(id).map_or(0, |def| def.size_class)
    }

    /// The slot's seat (anchor) pair - retail `+0x3C`/`+0x40`. Falls back to
    /// the live position for a not-yet-seeded actor (an unmoved actor's two
    /// pairs are equal by construction).
    fn battle_seat_of(&self, slot: usize) -> (i16, i16) {
        let Some(a) = self.actors.get(slot) else {
            return (0, 0);
        };
        a.battle
            .seat
            .unwrap_or((a.move_state.world_x, a.move_state.world_z))
    }

    /// PORT: FUN_8004E2F0 - the battle range / reach metric over live engine
    /// state: attacker **live** pair vs target **seat** pair, party reach
    /// offsets by character, monster size classes from the catalog. `1` for
    /// a slot that carries no actor (retail's head gate returns 1 for any
    /// slot `>= 8`, which is also how the all-target sentinel `8` reads).
    pub(crate) fn battle_range_metric(&self, attacker: u8, target: u8) -> u16 {
        // Retail head gate: any slot `>= 8` reads out-of-range 1 (`sltiu
        // a2,0x8` on both arguments) - which is also how the all-target
        // sentinel `8` reads.
        if attacker >= 8 || target >= 8 {
            return 1;
        }
        let (Some(att), Some(tgt)) = (
            self.actors.get(attacker as usize),
            self.actors.get(target as usize),
        ) else {
            return 1;
        };
        let pc = self.party_count;
        let attacker_party = attacker < pc;
        let target_party = target < pc;
        let attacker_pos = (att.move_state.world_x, att.move_state.world_z);
        let target_ref = tgt
            .battle
            .seat
            .unwrap_or((tgt.move_state.world_x, tgt.move_state.world_z));
        let inputs = motion::RangeInputs {
            attacker_party,
            target_party,
            attacker_reach: if attacker_party {
                motion::party_reach_offset(att.battle.character)
            } else {
                0
            },
            attacker_size: if attacker_party {
                0
            } else {
                self.battle_size_class_of(attacker)
            },
            target_size: if target_party {
                0
            } else {
                self.battle_size_class_of(target)
            },
            attacker_pos,
            target_ref,
        };
        // Retail: a = (bearing(target_ref -> attacker_live) + 0x800) & 0xFFF.
        let bearing = vm::battle_action::bearing_12bit_approx(
            target_ref.1,
            target_ref.0,
            attacker_pos.1,
            attacker_pos.0,
        );
        let angle = vm::battle_approach::approach_angle(bearing) as u16;
        let (sin, cos) = motion::trig12(angle);
        motion::range_metric(&inputs, sin, cos)
    }

    /// Seed every seated actor's anchor pair from its live position, once.
    /// Retail's setup writes the seat pair first and copies it into the live
    /// pair (`FUN_800513F0`); the engine's `enter_battle` writes the live
    /// pair from `battle_seats`, so the first battle tick mirrors it back.
    /// `finish_battle` clears the seats so the next battle re-seeds.
    fn seed_battle_seats(&mut self) {
        for a in self.actors.iter_mut() {
            if a.battle.seat.is_none() {
                a.battle.seat = Some((a.move_state.world_x, a.move_state.world_z));
            }
        }
    }

    /// The root-motion `(entry_speed, scale)` of `slot`'s committed clip:
    /// the effect-script head's `+0xC` halfword and the actor's `+0x21D`
    /// scale ([`DEFAULT_SPEED_SCALE`] when unset); [`FALLBACK_APPROACH`]
    /// when no committed clip carries a speed.
    fn battle_root_motion_of(&self, slot: usize) -> (i16, u8) {
        let Some(a) = self.actors.get(slot) else {
            return FALLBACK_APPROACH;
        };
        let speed = a
            .battle_effect_script
            .as_ref()
            .and_then(|s| s.get(ENTRY_SPEED_OFFSET..ENTRY_SPEED_OFFSET + 2))
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .filter(|&s| s != 0);
        match speed {
            Some(s) => {
                let scale = if a.battle.impact_step != 0 {
                    a.battle.impact_step
                } else {
                    DEFAULT_SPEED_SCALE
                };
                (s, scale)
            }
            None => FALLBACK_APPROACH,
        }
    }

    /// One tick of the battle locomotion drives - the engine slot of the
    /// anim tick's root-motion term (`FUN_80047430`,
    /// `0x80047D20..0x80047E18`). Runs ahead of the action SM step, matching
    /// retail's frame order (the actor-list anim tick runs before
    /// `FUN_80046A20`'s SM dispatch).
    ///
    /// One drive and one commit:
    ///
    /// - **Approach** (acting actor, states `0x15`/`0x16`/`0x19`): step the
    ///   live pair along the facing toward the target, gated on the range
    ///   check still failing - the retail positive-speed gate, which stops
    ///   the walk exactly on arrival - and clamped so a step never crosses
    ///   the target's live position.
    /// - **Ground commit** (state `0x50`, once per action): copy every living
    ///   actor's live pair onto its seat pair. **Nothing walks home.**
    ///
    /// ## Why there is no walk-home leg
    ///
    /// Retail does not return a combatant to its authored formation seat when
    /// an action ends - it leaves it standing where the fight put it, and the
    /// pair the range law measures against moves with it. Four capture-library
    /// states of the same solo fight make that measurable: two read the
    /// authored formation (party `z = -800`, monster `z = +800`, 1600 apart)
    /// and two - later in the same fight - read the party member at
    /// `z ~ -540` and the monster at `z ~ -250`, ~300 apart and both far off
    /// the formation, with each actor's `+0x3C`/`+0x40` pair sitting within
    /// ~110 units of its live `+0x34`/`+0x38` pair in every one of them. An
    /// actor that had been walked home could not read those positions, and a
    /// fixed seat could not stay that close to a live pair that has moved.
    ///
    /// So the engine commits the ground instead of undoing it: the seat is
    /// held still for the duration of an action (a stable goal for the
    /// approach and a stable reference for the separation pass) and then
    /// re-taken from the live pair at `DoneCleanup`. The invariant the
    /// seat-measured range law needs - a parked actor is *at* the pair the
    /// gate measures - survives by the same route retail keeps it, and the
    /// next attacker walks at where its target actually stands.
    ///
    /// What is deliberately **not** modelled is retail's recovery backstep
    /// (the recover clip's own negative-speed root motion): it is clip-timed
    /// and no clip-duration source is decoded, so the attacker ends the action
    /// on the range boundary the arrival shove pushed the target out to rather
    /// than a step behind it.
    pub(in crate::world) fn tick_battle_locomotion(&mut self) {
        use vm::battle_action::ActionState;
        self.seed_battle_seats();
        let Some(state) = ActionState::from_byte(self.battle_ctx.action_state) else {
            return;
        };
        let active = self.battle_ctx.active_actor as usize;
        let approaching = matches!(
            state,
            ActionState::AttackWindup | ActionState::AttackAdvance | ActionState::AttackShortStep
        );
        if approaching && active < self.actors.len() {
            self.drive_attack_approach(active);
        }
        if state == ActionState::DoneCleanup {
            self.commit_battle_ground();
        }
    }

    /// Re-take every living actor's seat pair from its live pair - the
    /// end-of-action ground commit described on
    /// [`Self::tick_battle_locomotion`]. A downed actor keeps its seat so the
    /// slot it fell in stays the reference a revive re-enters at.
    fn commit_battle_ground(&mut self) {
        for a in self.actors.iter_mut() {
            if a.battle.liveness == 0 {
                continue;
            }
            a.battle.seat = Some((a.move_state.world_x, a.move_state.world_z));
        }
    }

    /// Approach leg: facing recompute + range-gated root-motion step toward
    /// the target (see [`Self::tick_battle_locomotion`]).
    fn drive_attack_approach(&mut self, slot: usize) {
        let Some(a) = self.actors.get(slot) else {
            return;
        };
        let target = a.battle.active_target;
        let (ax, az) = (a.move_state.world_x, a.move_state.world_z);
        let Some(t) = self.actors.get(target as usize) else {
            return;
        };
        let (tx, tz) = (t.move_state.world_x, t.move_state.world_z);
        // Retail facing: bearing(target_live -> attacker_live) + half turn
        // (`0x801E32EC..0x801E3318` and the sibling state heads).
        let facing =
            vm::battle_action::bearing_12bit_approx(tz, tx, az, ax).wrapping_add(0x800) & 0xFFF;
        self.actors[slot].battle.facing_angle = facing;
        // The positive-speed gate: no step once in range.
        if self.battle_range_metric(slot as u8, target) == 0 {
            return;
        }
        let (speed, scale) = self.battle_root_motion_of(slot);
        let (sin, cos) = motion::trig12(facing);
        let (dx, dz) = motion::root_motion_step(sin, cos, speed.abs(), 1, scale);
        // Per-axis clamp at the target's live position: a step never crosses
        // the body it is walking at (engine guard - retail's gate alone
        // suffices when the target sits at its seat).
        let clamp = |cur: i16, step: i32, dest: i16| -> i16 {
            let next = i32::from(cur) + step;
            let (lo, hi) = if cur <= dest {
                (cur, dest)
            } else {
                (dest, cur)
            };
            next.clamp(i32::from(lo), i32::from(hi)) as i16
        };
        let ms = &mut self.actors[slot].move_state;
        ms.world_x = clamp(ms.world_x, dx, tx);
        ms.world_z = clamp(ms.world_z, dz, tz);
    }

    /// PORT-adjacent driver for `FUN_80051078` / `FUN_80050BB8` (the kernels
    /// live in `legaia_engine_vm::battle_separation`): one all-pairs
    /// separation pass in the retail visitation order, on the line after the
    /// action SM step - retail's exact call slot (`FUN_80046A20`).
    ///
    /// Retail's liveness test reads the slot pointer plus the actor's
    /// `+0x4` word; the engine substitutes its `liveness` halfword (the
    /// `+0x4` render word is not faithfully maintained here). Overlap is
    /// measured on the **seat** pairs and the nudge moves the **live**
    /// pairs, exactly as the kernel's field mapping documents. Radii:
    /// monster `size_class << 5` (the enemy stager's derivation), party
    /// [`PARTY_SEPARATION_RADIUS`].
    ///
    /// REF: FUN_80051078, FUN_80050BB8, FUN_80046A20
    pub(in crate::world) fn tick_battle_separation(&mut self) {
        use vm::battle_separation::{SEPARATION_SLOTS, SepActor, push_apart, separation_pass};
        let n = self.actors.len().min(SEPARATION_SLOTS);
        if n < 2 {
            return;
        }
        let mut alive = [false; SEPARATION_SLOTS];
        let mut seps = [SepActor::default(); SEPARATION_SLOTS];
        for (i, sep) in seps.iter_mut().enumerate().take(n) {
            let a = &self.actors[i];
            alive[i] = a.battle.liveness != 0;
            let radius = if (i as u8) < self.party_count {
                PARTY_SEPARATION_RADIUS
            } else {
                i16::from(self.battle_size_class_of(i as u8)) << 5
            };
            let seat = self.battle_seat_of(i);
            *sep = SepActor {
                radius,
                x: seat.0,
                z: seat.1,
                acc_x: a.move_state.world_x as u16,
                acc_z: a.move_state.world_z as u16,
            };
        }
        separation_pass(&alive, |i, j| {
            // The pair angle: `(FUN_80019B28(b.z, b.x, a.z, a.x) + 0x800)
            // & 0xFFF` over the seat pairs (`0x80050C0C..0x80050C28`).
            let (a, b) = (seps[i], seps[j]);
            let bearing = vm::battle_action::bearing_12bit_approx(b.z, b.x, a.z, a.x);
            let angle = bearing.wrapping_add(0x800) & 0xFFF;
            let (sin, cos) = motion::trig12(angle);
            let (lo, hi) = if i < j { (i, j) } else { (j, i) };
            let (left, right) = seps.split_at_mut(hi);
            let (ai, aj) = if i < j {
                (&mut left[lo], &mut right[0])
            } else {
                (&mut right[0], &mut left[lo])
            };
            push_apart(ai, aj, sin, cos);
        });
        for (i, sep) in seps.iter().enumerate().take(n) {
            let ms = &mut self.actors[i].move_state;
            ms.world_x = sep.acc_x as i16;
            ms.world_z = sep.acc_z as i16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vm::battle_action::{ActionState, StepOutcome};

    /// A world in battle mode with 1 party member and 1 monster at the
    /// authored solo seats (0,-800) vs (0,+800).
    fn battle_world() -> World {
        let mut w = World::new();
        w.enter_battle(1, 1);
        w
    }

    #[test]
    fn range_metric_reads_seats_and_thresholds() {
        let w = battle_world();
        // Party -> monster across 1600 units: far out of range whatever the
        // (catalog-less) size class resolves to.
        assert_ne!(w.battle_range_metric(0, 1), 0);
        // Missing slots read as out-of-range 1 (retail's >= 8 head gate).
        assert_eq!(w.battle_range_metric(0, 9), 1);
    }

    #[test]
    fn approach_walks_the_attacker_in_and_the_strike_lands() {
        let mut w = battle_world();
        w.actors[0].battle.active_target = 1;
        w.actors[0].battle.action_category = 3; // attack
        w.actors[1].battle.liveness = 1;
        w.battle_ctx.active_actor = 0;
        w.battle_ctx.action_state = ActionState::AttackShortStep.as_byte();
        let start_z = w.actors[0].move_state.world_z;
        // Drive the locomotion pass alone: the attacker must close on the
        // target's seat and the range gate must stop it exactly in range.
        let mut ticks = 0;
        while w.battle_range_metric(0, 1) != 0 && ticks < 500 {
            w.tick_battle_locomotion();
            ticks += 1;
        }
        assert_eq!(w.battle_range_metric(0, 1), 0, "approach never arrived");
        assert!(ticks > 0);
        let arrived_z = w.actors[0].move_state.world_z;
        assert!(
            arrived_z > start_z,
            "party attacker moved toward +Z: {start_z} -> {arrived_z}"
        );
        // One more pass is a no-op (the positive-speed gate).
        w.tick_battle_locomotion();
        assert_eq!(w.actors[0].move_state.world_z, arrived_z);
        // The seat stayed put - only the live pair walked.
        assert_eq!(w.battle_seat_of(0), (0, start_z));
    }

    #[test]
    fn nothing_walks_an_arrived_attacker_home() {
        // The recovery / done band must leave an arrived attacker exactly
        // where the strike left it. Retail does; the port used to ramp it
        // back to the authored seat over the whole Done budget.
        let mut w = battle_world();
        w.actors[0].battle.liveness = 1;
        w.tick_battle_locomotion(); // seed seats
        let seat = w.battle_seat_of(0);
        let arrived = (seat.0 + 7, seat.1 + 1360);
        for state in [
            ActionState::AttackRecovery,
            ActionState::AttackReturn,
            ActionState::DoneFadeDown,
            ActionState::EndOfAction,
        ] {
            w.actors[0].move_state.world_x = arrived.0;
            w.actors[0].move_state.world_z = arrived.1;
            w.battle_ctx.active_actor = 0;
            w.battle_ctx.action_state = state.as_byte();
            for _ in 0..200 {
                w.tick_battle_locomotion();
            }
            assert_eq!(
                (
                    w.actors[0].move_state.world_x,
                    w.actors[0].move_state.world_z
                ),
                arrived,
                "state {state:?} moved the attacker off the ground it ended on"
            );
        }
    }

    #[test]
    fn done_cleanup_commits_the_ground_as_the_new_seat() {
        let mut w = battle_world();
        w.actors[0].battle.liveness = 1;
        w.actors[1].battle.liveness = 1;
        w.tick_battle_locomotion(); // seed seats
        let seat0 = w.battle_seat_of(0);
        let arrived = (seat0.0 + 7, seat0.1 + 1360);
        w.actors[0].move_state.world_x = arrived.0;
        w.actors[0].move_state.world_z = arrived.1;
        // Mid-action the seat is still the one the approach aimed at.
        w.battle_ctx.active_actor = 0;
        w.battle_ctx.action_state = ActionState::AttackChain.as_byte();
        w.tick_battle_locomotion();
        assert_eq!(w.battle_seat_of(0), seat0, "the seat holds during an action");
        // The action's cleanup state re-takes it from the live pair.
        w.battle_ctx.action_state = ActionState::DoneCleanup.as_byte();
        w.tick_battle_locomotion();
        assert_eq!(
            w.battle_seat_of(0),
            arrived,
            "DoneCleanup commits the ground the action ended on"
        );
        // ... which is what keeps the seat-measured range law honest: the
        // next attacker now walks at where this actor actually stands.
        assert_eq!(
            w.battle_range_metric(1, 0),
            w.battle_range_metric(1, 0),
            "range metric resolves against the committed seat"
        );
    }

    #[test]
    fn a_downed_actor_keeps_its_seat_through_the_commit() {
        let mut w = battle_world();
        w.actors[0].battle.liveness = 1;
        w.actors[1].battle.liveness = 0;
        w.tick_battle_locomotion();
        let seat1 = w.battle_seat_of(1);
        w.actors[1].move_state.world_x = seat1.0 + 500;
        w.battle_ctx.action_state = ActionState::DoneCleanup.as_byte();
        w.tick_battle_locomotion();
        assert_eq!(w.battle_seat_of(1), seat1, "a downed slot is not re-seated");
    }

    #[test]
    fn separation_is_a_no_op_on_authored_seats_and_fires_on_overlap() {
        let mut w = battle_world();
        w.actors[0].battle.liveness = 1;
        w.actors[1].battle.liveness = 1;
        w.tick_battle_locomotion(); // seed seats
        // Authored solo seats sit 1600 apart - far beyond any threshold.
        let before: Vec<(i16, i16)> = w
            .actors
            .iter()
            .map(|a| (a.move_state.world_x, a.move_state.world_z))
            .collect();
        w.tick_battle_separation();
        let after: Vec<(i16, i16)> = w
            .actors
            .iter()
            .map(|a| (a.move_state.world_x, a.move_state.world_z))
            .collect();
        assert_eq!(before, after, "authored seats never overlap the threshold");
        // Near-coincident seats overlap: party radius 448 alone gives
        // threshold (448+0)/6 = 74 > proj 8, so both ordered pairs nudge the
        // live positions apart along the seat axis. (Exactly-coincident
        // seats are the degenerate case where the two ordered pairs read the
        // same atan2(0,0) bearing and cancel - retail arithmetic too.)
        let seat = w.battle_seat_of(0);
        w.actors[1].battle.seat = Some((seat.0, seat.1 + 8));
        w.actors[1].move_state.world_x = seat.0;
        w.actors[1].move_state.world_z = seat.1 + 8;
        let z0_before = w.actors[0].move_state.world_z;
        let z1_before = w.actors[1].move_state.world_z;
        w.tick_battle_separation();
        let z0 = w.actors[0].move_state.world_z;
        let z1 = w.actors[1].move_state.world_z;
        assert!(
            z0 < z0_before && z1 > z1_before,
            "overlapping seats must push the live pair apart: \
             {z0_before}->{z0}, {z1_before}->{z1}"
        );
    }

    #[test]
    fn attack_chain_walks_the_attacker_in_and_leaves_it_there() {
        // The SM + locomotion pair, interleaved like `live_battle_tick`
        // does: a party attacker seated 1600 units from its target must
        // physically walk in (AttackShortStep holds while out of range),
        // land the staged strike, and then STAY on the ground it fought on -
        // with that ground committed as its new seat.
        let mut w = battle_world();
        for i in 0..2 {
            w.actors[i].battle.liveness = 1;
            w.actors[i].battle.hp = 100;
            w.actors[i].battle.max_hp = 100;
        }
        w.actors[0].battle.active_target = 1;
        w.actors[0].battle.action_category = 3; // attack
        w.actors[0].battle.params[0] = 0x0C; // one swing byte, then terminator
        w.battle_ctx.active_actor = 0;
        w.battle_ctx.action_state = ActionState::AttackFace.as_byte();
        let seat0 = w.battle_seat_of(0);
        let mut moved = false;
        let mut struck = false;
        let mut max_dist = 0i32;
        for _ in 0..1000 {
            w.tick_battle_locomotion();
            let o = w.step_battle();
            let pos = (
                w.actors[0].move_state.world_x,
                w.actors[0].move_state.world_z,
            );
            if pos != seat0 {
                moved = true;
            }
            max_dist = max_dist.max(i32::from(pos.1) - i32::from(seat0.1));
            if let StepOutcome::Transition { from, to } = o
                && from == ActionState::AttackChain.as_byte()
                && to == ActionState::AttackRecovery.as_byte()
            {
                struck = true;
            }
            if w.battle_ctx.action_state == ActionState::EndOfAction.as_byte() {
                break;
            }
        }
        assert!(moved, "the attacker never left its seat");
        assert!(
            struck,
            "the strike edge never fired - the walk never arrived"
        );
        // No walk-home: the attacker ends the action at its closed-in
        // distance, not back at the authored seat.
        let end = (
            w.actors[0].move_state.world_x,
            w.actors[0].move_state.world_z,
        );
        let end_dist = i32::from(end.1) - i32::from(seat0.1);
        assert_eq!(
            end_dist, max_dist,
            "the attacker must hold the ground it closed to"
        );
        // ... and that ground is now its seat (committed at DoneCleanup).
        assert_eq!(
            w.battle_seat_of(0),
            end,
            "the action's ground was committed as the new seat"
        );
    }
}
