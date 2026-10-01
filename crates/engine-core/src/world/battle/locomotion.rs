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
//!   put them - see [`World::tick_battle_locomotion`] for the capture
//!   evidence.
//! - **The body pair is re-derived every frame.** `+0x3C`/`+0x40`, the pair
//!   the range law measures a target by and the separation pass measures
//!   overlap on, is the live pair plus the facing-rotated pose centroid,
//!   written by the pose decoder `FUN_8004998C` on every drawn frame
//!   ([`World::refresh_battle_body_pairs`]). It is stamped with the
//!   formation seat at setup, which is where the "seat" name the port's
//!   field carries comes from, but it does not hold still.
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

/// The render scale `+0x72` the body-pair store multiplies by: the actor
/// allocator's `0x1000` (`FUN_80020DE0`), 4.12 unity.
const BATTLE_RENDER_SCALE: i32 = 0x1000;

/// The pose centroid `FUN_8004998C` accumulates: each part's translation
/// summed in 16-bit halfwords (`lhu`/`addu`/`sh`), then divided by the part
/// count with a truncating signed `div` (`0x8004A3DC..0x8004A42C`). Returns
/// the `(x, z)` pair; `(0, 0)` for an empty pose.
pub(crate) fn pose_centroid_xz(parts: &[([i16; 3], [i16; 3])]) -> (i16, i16) {
    let n = parts.len();
    if n == 0 {
        return (0, 0);
    }
    let (mut sx, mut sz) = (0i16, 0i16);
    for (t, _) in parts {
        sx = sx.wrapping_add(t[0]);
        sz = sz.wrapping_add(t[2]);
    }
    let n = n as i32;
    ((i32::from(sx) / n) as i16, (i32::from(sz) / n) as i16)
}

/// The Y half of [`pose_centroid_xz`]: the same 16-bit sum and truncating
/// divide, stored by `FUN_8004998C` as the display height
/// `+0x3E = +0x36 + (cy * +0x72) >> 12` (`0x8004A430..0x8004A458`) - no
/// facing rotation, since the rotation is about Y.
pub(crate) fn pose_centroid_y(parts: &[([i16; 3], [i16; 3])]) -> i16 {
    let n = parts.len();
    if n == 0 {
        return 0;
    }
    let mut sy = 0i16;
    for (t, _) in parts {
        sy = sy.wrapping_add(t[1]);
    }
    (i32::from(sy) / n as i32) as i16
}

/// The facing-rotated, scaled centroid offset the body-pair store adds to
/// the live pair (`FUN_8004998C` `0x8004A43C..0x8004A538`), with the retail
/// multiply order and arithmetic shifts. The `0xFFF - f` reads are the
/// table's own mirrored index, not a negated angle.
pub(crate) fn body_offset(facing: u16, cx: i16, cz: i16, scale: i32) -> (i16, i16) {
    let f = facing & 0xFFF;
    let m = 0xFFF - f;
    let (cx, cz) = (i32::from(cx), i32::from(cz));
    let sin = |a: u16| i32::from(motion::sin12(a));
    let cos = |a: u16| i32::from(motion::trig12(a).1);
    let x = ((((sin(f) * cz) >> 12) + ((cos(m) * cx) >> 12)) * scale) >> 12;
    let z = ((((sin(m) * cx) >> 12) + ((cos(f) * cz) >> 12)) * scale) >> 12;
    (x as i16, z as i16)
}

impl World {
    /// The display trio `+0x3C / +0x3E / +0x40` of battle seat `slot`: the
    /// body pair ([`Self::refresh_battle_body_pairs`]) plus the display
    /// height, the live `+0x36` raised by the pose centroid's Y
    /// ([`pose_centroid_y`]). Falls back to the live position for a seat with
    /// no body pair or no decoded pose.
    pub fn battle_display_trio(&self, slot: usize) -> Option<[f32; 3]> {
        let a = self.actors.get(slot)?;
        let (x, z) = a
            .battle
            .seat
            .unwrap_or((a.move_state.world_x, a.move_state.world_z));
        let cy = a
            .battle_animation
            .as_ref()
            .and(a.pose_frame.as_ref())
            .map_or(0, |p| pose_centroid_y(&p.bone_outputs));
        let y = a.move_state.world_y.wrapping_add(cy);
        Some([f32::from(x), f32::from(y), f32::from(z)])
    }

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
        self.tables
            .monster_catalog
            .get(id)
            .map_or(0, |def| def.size_class)
    }

    /// The slot's seat (anchor) pair - retail `+0x3C`/`+0x40`. Falls back to
    /// the live position for a not-yet-seeded actor (an unmoved actor's two
    /// pairs are equal by construction).
    pub(in crate::world) fn battle_seat_of(&self, slot: usize) -> (i16, i16) {
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
    /// slot `>= 8`, which is also how the all-target sentinel `8` reads), and
    /// `1` once the battle has ended.
    ///
    /// The party / monster split is the port's seat layout: retail tests
    /// `slot < 3` (`sltiu v0,a2,3` at `0x8004E338`) because its actor table
    /// keeps the party at fixed seats `0..=2`, where the engine seats the
    /// monsters straight after the party, so the same question is
    /// `slot < party_count` here.
    pub(crate) fn battle_range_metric(&self, attacker: u8, target: u8) -> u16 {
        // Retail's first test is the battle-end byte: `lbu v1,-0x428f(v1)`
        // (`0x8007BD71`) against `0xFF` at `0x8004E2F4..0x8004E310` sends every
        // call to the out-of-range `1` exit once the wipe / escape teardowns
        // have stored `0xFE` there. The engine's end signal is
        // `battle.end`.
        if self.battle.end.is_some() {
            return 1;
        }
        // Then any slot `>= 8` reads out-of-range 1 (`sltiu a2,0x8` on both
        // arguments) - which is also how the all-target sentinel `8` reads.
        if attacker >= 8 || target >= 8 {
            return 1;
        }
        let (Some(att), Some(tgt)) = (
            self.actors.get(attacker as usize),
            self.actors.get(target as usize),
        ) else {
            return 1;
        };
        let pc = self.party.party_count;
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

    /// The signed root-motion speed (`+0x0C`) of the clip `slot` is
    /// **playing** - read off the player itself, so a pose / reaction /
    /// staged clip each answer their own entry - with the actor's `+0x21D`
    /// scale ([`DEFAULT_SPEED_SCALE`] when unset). `None` when no clip is
    /// playing or the playing clip carries no speed.
    fn battle_playing_root_motion(&self, slot: usize) -> Option<(i16, u8)> {
        let a = self.actors.get(slot)?;
        let speed = a.battle_animation.as_ref()?.root_speed();
        if speed == 0 {
            return None;
        }
        let scale = if a.battle.anim_rate.get() != 0 {
            a.battle.anim_rate.get()
        } else {
            DEFAULT_SPEED_SCALE
        };
        Some((speed, scale))
    }

    /// The approach drive's `(entry_speed, scale)`: the playing clip's
    /// positive speed, else [`FALLBACK_APPROACH`] (a clip-less host, or a
    /// walk staged but not yet playing).
    fn battle_root_motion_of(&self, slot: usize) -> (i16, u8) {
        match self.battle_playing_root_motion(slot) {
            Some((s, scale)) if s > 0 => (s, scale),
            _ => FALLBACK_APPROACH,
        }
    }

    /// One tick of the battle locomotion drives - the engine slot of the
    /// anim tick's root-motion term (`FUN_80047430`,
    /// `0x80047D20..0x80047E18`). Runs ahead of the action SM step, matching
    /// retail's frame order (the actor-list anim tick runs before
    /// `FUN_80046A20`'s SM dispatch).
    ///
    /// Opens with the **body-pair refresh** ([`Self::refresh_battle_body_pairs`]):
    /// every actor's `+0x3C`/`+0x40` pair is re-derived from its live pair
    /// and its pose, which is what the range law and the separation pass
    /// read this frame. Then one drive:
    ///
    /// - **Approach** (acting actor, states `0x15`/`0x16`/`0x19`): step the
    ///   live pair along the facing toward the target, gated on the range
    ///   check still failing - the retail positive-speed gate, which stops
    ///   the walk exactly on arrival - and clamped so a step never crosses
    ///   the target's live position.
    ///
    /// **Nothing walks home.** Retail does not return a combatant to its
    /// authored formation seat when an action ends - it leaves it standing
    /// where the fight put it. Four capture-library states of the same solo
    /// fight make that measurable: two read the authored formation (party
    /// `z = -800`, monster `z = +800`, 1600 apart) and two - later in the same
    /// fight - read the party member at `z ~ -540` and the monster at
    /// `z ~ -250`, both far off the formation, with each actor's
    /// `+0x3C`/`+0x40` pair sitting within ~110 units of its live
    /// `+0x34`/`+0x38` pair in every one of them. That ~110 is the pose
    /// centroid the refresh adds: the pair is not a seat at all.
    ///
    /// ## The backstep
    ///
    /// The retail term is signed (`lh v0,0xc(s3)` at `0x80047D34`):
    /// `bltz` routes a **negative** speed straight to the step
    /// (`0x80047D64`) with no range test, a positive one steps only while
    /// the range poll still fails. In the four player files the negative
    /// speeds sit on the knockdown (entry 4: `-2` to `-12`), the block
    /// (entry `0x0B`, `-4`) and some flinches - a struck actor slides back
    /// as it reacts. Entry 8 carries `0`: it is the downed party member's
    /// kneel, not a recover backstep, and the SM never stages it on a living
    /// attacker. The party arts carry positive speeds (a Somersault reads
    /// `+4`, a Cyclone `+6`) and drift the attacker into its target as they
    /// play. [`Self::drive_playing_root_motion`] applies that law to every
    /// actor whose playing clip carries a speed, except one holding retail's
    /// `+0x1DC` bit 3 latch (`0x80047D20`). Only two writers raise it: the
    /// commit of a **tag-8** entry - the last link of a downed party member's
    /// knockdown -> `7` -> `8` chain - and the monster-death arm
    /// (`world::battle::clip_ladder`); a living knockdown's own commit clears
    /// `+0x1DC`, so a knockdown's root speed moves the actor.
    pub(in crate::world) fn tick_battle_locomotion(&mut self) {
        use vm::battle_action::ActionState;
        self.seed_battle_seats();
        self.refresh_battle_body_pairs();
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
        for i in 0..self.actors.len() {
            if approaching && i == active {
                // The approach drive above already walked this clip.
                continue;
            }
            self.drive_playing_root_motion(i);
        }
    }

    /// Re-derive every seated actor's `+0x3C`/`+0x40` **body pair** from its
    /// live pair and its current pose - the store retail's pose decoder
    /// makes on every drawn frame.
    ///
    /// `FUN_8004998C` (called per actor from the battle draw callback
    /// `FUN_80048A08`) sums the decoded pose's per-part translations into
    /// scratch `+0x120/+0x122/+0x124` (halfword adds, `0x8004A380..0x8004A3B8`
    /// and the interpolating twin at `0x8004A230..0x8004A258`), divides each
    /// by the part count (`div`, `0x8004A3DC..0x8004A42C`) - the pose's
    /// **centroid** - then rotates the `(x, z)` centroid by the facing
    /// `+0x46`, scales by the render scale `+0x72`, and adds it to the live
    /// pair:
    ///
    /// ```text
    /// +0x3C = +0x34 + ((sin[f]*cz >> 12) + (cos[0xFFF-f]*cx >> 12)) * s >> 12
    /// +0x40 = +0x38 + ((sin[0xFFF-f]*cx >> 12) + (cos[f]*cz >> 12)) * s >> 12
    /// ```
    ///
    /// (`0x8004A43C..0x8004A538`; `sin` = `*0x8007B81C`, `cos` =
    /// `*0x8007B7F8`). So the pair is the actor's body position, re-taken
    /// from the live pair every frame - never a fixed seat. Everything that
    /// measures an actor from outside reads it: the range law's target side
    /// (`FUN_8004E2F0`) and both sides of the separation pass
    /// (`FUN_80050BB8`). The engine held it still for the length of an action
    /// instead, and that is what let a boss fight park forever: the
    /// separation pass nudges the **live** pairs of two actors whose pairs
    /// overlap, so two party members whose held pairs overlapped were pushed
    /// apart every frame without the overlap ever clearing, walked tens of
    /// thousands of units off the stage, and the monster's approach - clamped
    /// at its target's live position, measured against the stale pair -
    /// never came in range.
    ///
    /// Engine choices, each noted where it differs: the refresh runs at the
    /// head of the locomotion pass, not at the end of the previous frame's
    /// draw - the live pairs it reads are the same ones (nothing moves them
    /// between retail's draw and its next anim tick), only the pose is one
    /// frame newer. The render scale is the allocator's `0x1000` (no battle
    /// path the port models rewrites `+0x72`). The `+0x44` pitch arm
    /// (`0x8004A534..0x8004A5F8`), which only re-derives `z` when a battle
    /// actor carries a pitch, is not taken - the port's battle actors carry
    /// none. An actor with no pose (no clip player) takes a zero centroid,
    /// so its pair is its live pair.
    // PORT: FUN_8004998C (`0x8004A3DC..0x8004A5F8`, the body-pair store)
    pub(in crate::world) fn refresh_battle_body_pairs(&mut self) {
        for a in self.actors.iter_mut() {
            if a.battle.seat.is_none() {
                continue;
            }
            let centroid = if a.battle_animation.is_some() {
                a.pose_frame
                    .as_ref()
                    .map(|p| pose_centroid_xz(&p.bone_outputs))
            } else {
                None
            };
            let (dx, dz) = centroid
                .map(|(cx, cz)| body_offset(a.battle.facing_angle, cx, cz, BATTLE_RENDER_SCALE))
                .unwrap_or((0, 0));
            a.battle.seat = Some((
                a.move_state.world_x.wrapping_add(dx),
                a.move_state.world_z.wrapping_add(dz),
            ));
        }
    }

    /// One tick of the playing clip's signed root motion for a non-approach
    /// actor (see [`Self::tick_battle_locomotion`] § The backstep): a
    /// negative speed steps back along the facing unconditionally, a
    /// positive one steps forward only while out of range of the actor's
    /// target. An actor holding the `+0x1DC` bit-3 latch does not move.
    // PORT: FUN_80047430 (`0x80047D20..0x80047E18`, the signed root-motion
    // term; the approach half is `drive_attack_approach`)
    fn drive_playing_root_motion(&mut self, slot: usize) {
        let Some((speed, scale)) = self.battle_playing_root_motion(slot) else {
            return;
        };
        let (facing, target, latched) = {
            let a = &self.actors[slot];
            (
                a.battle.facing_angle,
                a.battle.active_target,
                a.battle
                    .flag_bits
                    .has(super::clip_ladder::ANIM_FLAG_ROOT_LATCH),
            )
        };
        if latched {
            return;
        }
        if speed > 0 && self.battle_range_metric(slot as u8, target) == 0 {
            return;
        }
        let (sin, cos) = motion::trig12(facing);
        let (dx, dz) = motion::root_motion_step(sin, cos, speed, 1, scale);
        let ms = &mut self.actors[slot].move_state;
        ms.world_x = ms.world_x.wrapping_add(dx as i16);
        ms.world_z = ms.world_z.wrapping_add(dz as i16);
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
            let radius = if (i as u8) < self.party.party_count {
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
        // The body pair follows the live pair (no pose here, so a zero
        // centroid): it is re-taken at the head of every pass.
        w.tick_battle_locomotion();
        assert_eq!(w.battle_seat_of(0), (0, arrived_z));
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
    fn the_body_pair_follows_the_live_pair_every_frame() {
        // Retail's pose decoder re-takes `+0x3C`/`+0x40` from the live pair
        // on every drawn frame (`FUN_8004998C`); the engine used to hold it
        // for a whole action and re-take it only at DoneCleanup.
        let mut w = battle_world();
        w.actors[0].battle.liveness = 1;
        w.actors[1].battle.liveness = 0;
        w.tick_battle_locomotion(); // seed
        for (slot, state) in [
            (0usize, ActionState::AttackChain),
            (1usize, ActionState::DoneCleanup),
        ] {
            let seat = w.battle_seat_of(slot);
            let moved = (seat.0 + 7, seat.1 + 1360);
            w.actors[slot].move_state.world_x = moved.0;
            w.actors[slot].move_state.world_z = moved.1;
            w.battle_ctx.action_state = state.as_byte();
            w.tick_battle_locomotion();
            assert_eq!(
                w.battle_seat_of(slot),
                moved,
                "slot {slot} in {state:?}: the body pair is the live pair"
            );
        }
    }

    #[test]
    fn body_offset_rotates_the_centroid_by_the_facing() {
        // Facing 0: the centroid passes straight through (the `0xFFF`
        // mirror's sin(0xFFF) = -6 truncates away for small centroids).
        let (x, z) = body_offset(0, 0, 100, BATTLE_RENDER_SCALE);
        assert!(
            x.abs() <= 1 && (99..=100).contains(&z),
            "facing 0 ({x},{z})"
        );
        // A half turn flips it.
        let (x, z) = body_offset(0x800, 0, 100, BATTLE_RENDER_SCALE);
        assert!(
            x.abs() <= 1 && (-101..=-99).contains(&z),
            "half turn ({x},{z})"
        );
        // A quarter turn swings z into x.
        let (x, z) = body_offset(0x400, 0, 100, BATTLE_RENDER_SCALE);
        assert!(
            (99..=100).contains(&x) && z.abs() <= 1,
            "quarter turn ({x},{z})"
        );
        // Centroid: halfword sum, truncating divide by the part count.
        let parts = [
            ([10, 0, -7], [0; 3]),
            ([11, 5, 0], [0; 3]),
            ([0, 0, 0], [0; 3]),
        ];
        assert_eq!(pose_centroid_xz(&parts), (7, -2));
        assert_eq!(pose_centroid_xz(&[]), (0, 0));
    }

    #[test]
    fn overlapping_pairs_separate_instead_of_running_away() {
        // The taiku / rugi boss park: two party members whose pairs overlap.
        // The separation pass nudges the live pairs; with the body pair
        // re-taken from the live pair each frame the overlap clears in a
        // handful of frames. Held still (the old engine seat), the same
        // overlap nudged the live pairs every frame for the whole action and
        // flung both members tens of thousands of units off the stage.
        let mut w = World::new();
        w.enter_battle(2, 1);
        for i in 0..3 {
            w.actors[i].battle.liveness = 1;
            w.actors[i].battle.seat = None;
        }
        w.actors[0].move_state.world_x = 0;
        w.actors[0].move_state.world_z = 648;
        w.actors[1].move_state.world_x = 68;
        w.actors[1].move_state.world_z = 595;
        w.battle_ctx.action_state = ActionState::AttackChain.as_byte();
        w.battle_ctx.active_actor = 2;
        for _ in 0..3000 {
            w.tick_battle_locomotion();
            w.tick_battle_separation();
        }
        for i in 0..2 {
            let (x, z) = (
                w.actors[i].move_state.world_x,
                w.actors[i].move_state.world_z,
            );
            assert!(
                x.abs() < 1000 && (0..2000).contains(&z),
                "party slot {i} ran away to ({x},{z})"
            );
        }
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
        // ... and its body pair stands on that ground.
        w.tick_battle_locomotion();
        assert_eq!(
            w.battle_seat_of(0),
            end,
            "the body pair follows the ground the action ended on"
        );
    }
}
