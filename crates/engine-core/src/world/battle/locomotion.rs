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
//! - **An attacker inside its reach backs off.** The same tick, just ahead
//!   of the root-motion term (`0x80047C3C..0x80047D1C`), steps the acting
//!   seat of an Attack back along its facing while the range law reads in
//!   range ([`World::drive_in_range_backoff`]), so a strike loop holds its
//!   attacker at the edge of its reach.
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
//!   `legaia_engine_battle_vm::battle_separation` kernels.
//! - **The range law** is computed, not tabulated: `FUN_8004E2F0`
//!   (`legaia_engine_battle_vm::battle_action::motion::range_metric`).
//!   [`World::battle_range_metric`] assembles its inputs from live engine
//!   state; the `BattleActionHost::range_check` impl delegates here.
//!
//! REF: FUN_80047430 (root-motion drive), FUN_80046A20 (per-frame ordering)
//! REF: FUN_8004E2F0 (range law), FUN_80051078 / FUN_80050BB8 (separation)

use super::*;
use legaia_engine_battle_vm::battle_action::motion;

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
/// `legaia_actor_formats::monster_archive` - the anim cursor doc pins normal `4`),
/// used when the actor's own `impact_step` byte is unset.
const DEFAULT_SPEED_SCALE: u8 = 4;

/// The action category the in-range back-off applies to (`li v0,0x3` against
/// `+0x1DE` at `0x80047C68`): Attack.
const BACKOFF_CATEGORY: u8 = 3;

/// The in-range back-off as a root speed: its `>> 13` (`0x80047CCC` /
/// `0x80047D14`) over the root term's `>> 15` is a factor of four. The
/// product is the forward one; the step subtracts it (`subu`).
const BACKOFF_SPEED: i16 = 4;

/// Bound on [`World::commit_reach_snap`]'s walk: retail's loops have none.
/// At `trig >> 10` a step (four units) this covers a gap of `16384` units,
/// more than the stage.
const REACH_SNAP_MAX_STEPS: usize = 4096;

/// Separation body radius of a party seat: the `+0x58` word of the actor's
/// node (`*(actor+0x22C)`) that `FUN_80050BB8` reads. The seat stager
/// `FUN_800513F0` writes it as `0x280` (`li v0,0x280` / `sh v0,0x58(s1)` at
/// `0x8005151C`) and overwrites it with `0x400` when the seat's roster id
/// `DAT_8007BD10[i]` is `4` (`0x80051528..0x80051538`). Every catalogued
/// battle capture reads `640` on Vahn / Noa / Gala and `1024` on Terra.
const PARTY_SEPARATION_RADIUS: i16 = 0x280;

/// [`PARTY_SEPARATION_RADIUS`] for the roster-id-`4` seat (Terra).
const TERRA_SEPARATION_RADIUS: i16 = 0x400;

/// The 1-based roster id (`DAT_8007BD10[i]`) whose seat takes
/// [`TERRA_SEPARATION_RADIUS`].
const TERRA_ROSTER_ID: u8 = 4;

/// The render scale `+0x72` the body-pair store multiplies by: the actor
/// allocator's `0x1000` (`FUN_80020DE0`), 4.12 unity.
const BATTLE_RENDER_SCALE: i32 = 0x1000;

/// The pose centroid (`FUN_8004998C`): the shared kernel
/// [`motion::pose_centroid_xz`].
pub(crate) fn pose_centroid_xz(parts: &[([i16; 3], [i16; 3])]) -> (i16, i16) {
    motion::pose_centroid_xz(parts)
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

/// The channels of a battle actor's decoded pose the body-pair store
/// averages: the playing stream's **own** parts. `FUN_8004998C` sums one
/// translation per stream part and divides by the stream's part count, the
/// byte at `*(entry + 0x88)` (`lbu v1,0x0(s4)` / `div` at
/// `0x8004A3DC..0x8004A42C`). A party clip expanded per assembled object
/// (`expand_animation_for_objects`) carries further channels past those -
/// equipment objects that copy their attach bone's channel - and averaged in
/// they pulled Vahn's invoke-clip centroid some nine units off retail's
/// (`x` `30` against `39` on keyframes 2..5, the summon close-up's focus).
/// `None` for an actor with no clip or no pose.
pub(in crate::world) fn stream_pose(a: &crate::world::Actor) -> Option<&[([i16; 3], [i16; 3])]> {
    let player = a.battle_animation.as_ref()?;
    let parts = &a.pose_frame.as_ref()?.bone_outputs;
    let n = usize::from(player.variant_track().stream_parts);
    Some(if n != 0 && n < parts.len() {
        &parts[..n]
    } else {
        parts
    })
}

/// The body-pair offset (`FUN_8004998C`): the shared kernel
/// [`motion::body_offset`].
pub(crate) fn body_offset(facing: u16, cx: i16, cz: i16, scale: i32) -> (i16, i16) {
    motion::body_offset(facing, cx, cz, scale)
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
        let cy = stream_pose(a).map_or(0, pose_centroid_y);
        let y = a.move_state.world_y.wrapping_add(cy);
        Some([f32::from(x), f32::from(y), f32::from(z)])
    }

    /// The trio the value readout anchors seat `slot`'s numeral on: the
    /// display trio the previous pass's draw stored
    /// ([`crate::world::BattleState::readout_trios`]), the live one when no
    /// pass has taken it.
    pub fn battle_readout_trio(&self, slot: usize) -> Option<[f32; 3]> {
        self.battle
            .readout_trios
            .get(slot)
            .copied()
            .flatten()
            .or_else(|| self.battle_display_trio(slot))
    }

    /// The monster record `+0x1F` size class seated in `slot`, `0` for a
    /// party slot / empty slot / unresolved catalog (the same resolution the
    /// battle host's `monster_size_class` performs).
    pub(in crate::world) fn battle_size_class_of(&self, slot: u8) -> u8 {
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
            // `DAT_80078878[DAT_8007BD10[slot] - 1]`: keyed on the seat's
            // roster character. `BattleActor::character` is the arts-path
            // lookup key and is written only when an arts queue is armed, so
            // keying the reach on it gave every plain Attack by Noa or Gala
            // Vahn's `+43` (a shorter reach that walked them in further).
            attacker_reach: if attacker_party {
                motion::PARTY_REACH_OFFSETS
                    .get(self.party_roster_slot(usize::from(attacker)))
                    .copied()
                    .unwrap_or(0)
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
        let bearing = legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
            target_ref.1,
            target_ref.0,
            attacker_pos.1,
            attacker_pos.0,
        );
        let angle = legaia_engine_battle_vm::battle_approach::approach_angle(bearing) as u16;
        let (sin, cos) = legaia_engine_system::trig::trig12(angle);
        motion::range_metric(&inputs, sin, cos)
    }

    /// [`Self::battle_range_metric`] for the summon creature, which retail
    /// seats at actor-table slot `7` - a non-party slot, so the metric takes
    /// its monster arm with the creature's own `+0x1F` size class. The engine
    /// seats the creature outside the eight-slot table (`8 + party_count`),
    /// where the metric's head gate would read every call as out of range, so
    /// the creature's live pair and size are passed in.
    ///
    /// REF: FUN_8004E2F0 (called as `FUN_8004E2F0(7, victim)` by PROT 0903's
    /// walk arm, `0x801F740C`)
    pub(in crate::world) fn creature_range_metric(
        &self,
        creature_pos: (i16, i16),
        creature_size: u8,
        target: u8,
    ) -> u16 {
        if self.battle.end.is_some() {
            return 1;
        }
        let Some(tgt) = self.actors.get(usize::from(target)) else {
            return 1;
        };
        let target_party = target < self.party.party_count;
        let inputs = motion::RangeInputs {
            attacker_party: false,
            target_party,
            attacker_reach: 0,
            attacker_size: creature_size,
            target_size: if target_party {
                0
            } else {
                self.battle_size_class_of(target)
            },
            attacker_pos: creature_pos,
            target_ref: tgt
                .battle
                .seat
                .unwrap_or((tgt.move_state.world_x, tgt.move_state.world_z)),
        };
        let bearing = legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
            inputs.target_ref.1,
            inputs.target_ref.0,
            creature_pos.1,
            creature_pos.0,
        );
        let angle = legaia_engine_battle_vm::battle_approach::approach_angle(bearing) as u16;
        let (sin, cos) = legaia_engine_system::trig::trig12(angle);
        motion::range_metric(&inputs, sin, cos)
    }

    /// Seed every seated actor's anchor pair from its live position, once.
    /// Retail's setup writes the seat pair first and copies it into the live
    /// pair (`FUN_800513F0`); the engine's `enter_battle` writes the live
    /// pair from `battle_seats`, so the first battle tick mirrors it back.
    /// `finish_battle` clears the seats so the next battle re-seeds.
    pub(in crate::world) fn seed_battle_seats(&mut self) {
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
    pub(in crate::world) fn battle_playing_root_motion(&self, slot: usize) -> Option<(i16, u8)> {
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
        self.tick_battle_locomotion_dt(1);
    }

    /// [`Self::tick_battle_locomotion`] over `dt` vsyncs at once: retail's
    /// anim tick moves each actor `trig * speed * dt * scale >> 15` on one
    /// pass behind **one** range poll (`lbu v0,0x393` into the root-motion
    /// product at `0x80047D20..0x80047E18`). On retail's cadence the pass
    /// runs it once with `dt` the frame step; a range a pass's first vsync
    /// would close does not stop its second.
    pub(in crate::world) fn tick_battle_locomotion_dt(&mut self, dt: u8) {
        use legaia_engine_battle_vm::battle_action::ActionState;
        self.seed_battle_seats();
        // On retail's cadence the pass took the pairs from the last pass's
        // pose before its anim advance (`World::refresh_battle_body_pairs`).
        if !self.toggles.battle_sm_frame_cadence {
            self.refresh_battle_body_pairs();
        }
        let Some(state) = ActionState::from_byte(self.battle_ctx.action_state) else {
            return;
        };
        let active = self.battle_ctx.active_actor as usize;
        let approaching = matches!(
            state,
            ActionState::AttackWindup | ActionState::AttackAdvance | ActionState::AttackShortStep
        );
        if active < self.actors.len() {
            self.drive_in_range_backoff(active, dt);
        }
        if approaching && active < self.actors.len() {
            self.drive_attack_approach(active, dt);
        }
        for i in 0..self.actors.len() {
            if approaching && i == active {
                // The approach drive above already walked this clip.
                continue;
            }
            self.drive_playing_root_motion(i, dt);
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
    /// Engine choices, each noted where it differs: on the per-vsync
    /// cadence the refresh runs at the head of the locomotion pass, not at
    /// the end of the previous frame's draw - the live pairs it reads are
    /// the same ones (nothing moves them between retail's draw and its next
    /// anim tick), only the pose is one frame newer. On retail's cadence it
    /// runs ahead of the pass's anim advance, so the pass measures and
    /// frames the pose the last pass drew, as retail's does (the summon
    /// close-up's focus is the caster's pair, and read off the new pose it
    /// led retail's by a pass: `theeder_summon_mid_cast`). The render scale is the allocator's `0x1000` (no battle
    /// path the port models rewrites `+0x72`). The `+0x44` pitch arm
    /// (`0x8004A534..0x8004A5F8`), which only re-derives `z` when a battle
    /// actor carries a pitch, is not taken - the port's battle actors carry
    /// none. An actor with no pose (no clip player) takes a zero centroid,
    /// so its pair is its live pair.
    // PORT: FUN_8004998C (`0x8004A3DC..0x8004A5F8`, the body-pair store)
    /// [`Self::refresh_battle_body_pairs`] for a caller outside the world
    /// that has just moved combatants' live pairs or headings (a capture
    /// seat standing them on a capture's ground): the pairs the range law,
    /// the separation pass and the framings read follow at once.
    pub fn refresh_battle_body_pairs_now(&mut self) {
        self.refresh_battle_body_pairs();
    }

    pub(in crate::world) fn refresh_battle_body_pairs(&mut self) {
        for a in self.actors.iter_mut() {
            if a.battle.seat.is_none() {
                continue;
            }
            let centroid = stream_pose(a).map(pose_centroid_xz);
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
    fn drive_playing_root_motion(&mut self, slot: usize, dt: u8) {
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
        let (sin, cos) = legaia_engine_system::trig::trig12(facing);
        let raw = motion::root_motion_raw(sin, cos, speed, dt, scale);
        let frame = self.battle_frame_id();
        let (dx, dz) = self.actors[slot]
            .battle
            .root_carry
            .step_in_frame(frame, raw);
        let ms = &mut self.actors[slot].move_state;
        ms.world_x = ms.world_x.wrapping_add(dx as i16);
        ms.world_z = ms.world_z.wrapping_add(dz as i16);
    }

    /// The anim tick's **in-range back-off** (`FUN_80047430`,
    /// `0x80047C3C..0x80047D1C`), the term just ahead of the root-motion
    /// step. On every tick it measures the actor against its own target
    /// `+0x1DD` (`jal 0x8004E2F0` at `0x80047C40`), and when the range law
    /// reads in range (`0`), the command flow `ctx[+0x06]` is `0xFF`, the
    /// actor's category `+0x1DE` is `3` (Attack) and the actor is the acting
    /// seat `ctx[+0x13]`, it steps the live pair back along its facing:
    /// `+0x38 -= (cos[f] * DAT_1F800393 * +0x21D) >> 13`, and `+0x34` the same
    /// with `sin` (`0x80047C90..0x80047D1C`). So an attacker standing inside
    /// its reach drifts back toward the edge of it for as long as its attack
    /// runs - the counterweight to its strike clips' positive root speed,
    /// which steps it forward only while out of range. The shift is `13`
    /// against the root term's `15`: a speed of `4` on the same law, subtracted.
    // PORT: FUN_80047430 (`0x80047C3C..0x80047D1C`, the in-range back-off)
    fn drive_in_range_backoff(&mut self, slot: usize, dt: u8) {
        use crate::battle_round::RoundPhase;
        if self.battle.round_flow.phase != RoundPhase::Execute {
            return;
        }
        let (facing, target, category, rate) = {
            let a = &self.actors[slot];
            (
                a.battle.facing_angle,
                a.battle.active_target,
                a.battle.action_category,
                a.battle.anim_rate.get(),
            )
        };
        if category != BACKOFF_CATEGORY
            || target >= 8
            || self.battle_range_metric(slot as u8, target) != 0
        {
            return;
        }
        // A frozen actor (`+0x21D == 0`) multiplies the step to nothing.
        //
        // Retail shifts the **forward** product and subtracts it
        // (`sra v1,t1,0xd` / `subu` at `0x80047CCC..0x80047CD0`), so the
        // step is `-floor(trig * dt * scale / 8192)`. Shifting the negated
        // product instead floors the other way: a unit further back, every
        // in-range frame, on every axis whose step carries a fraction, which
        // over a strike string parks the attacker outside where retail's
        // law leaves it.
        let (sin, cos) = legaia_engine_system::trig::trig12(facing);
        let raw = motion::root_motion_raw(sin, cos, BACKOFF_SPEED, dt, rate);
        let frame = self.battle_frame_id();
        let (dx, dz) = self.actors[slot]
            .battle
            .backoff_carry
            .step_in_frame(frame, raw);
        let ms = &mut self.actors[slot].move_state;
        ms.world_x = ms.world_x.wrapping_sub(dx as i16);
        ms.world_z = ms.world_z.wrapping_sub(dz as i16);
    }

    /// The clip commit's reach snap (`FUN_8004AD80`, `0x8004BF7C..0x8004C118`).
    ///
    /// When the acting actor commits a clip while the action SM sits in the
    /// strike loop `0x1E`, the commit's tail measures it against its target
    /// with the range law and walks its live pair onto the edge of its reach
    /// in one go: out of range, it steps **forward** along the bearing to
    /// the target's body pair until the law reads in range; in range, it
    /// steps **back** until the law reads out of range. Each step re-faces
    /// the actor (`+0x46 = (bearing(target +0x40/+0x3C -> actor
    /// +0x38/+0x34) + 0x800) & 0xFFF`) and moves the pair by `trig >> 10`
    /// (`sll 0x10` / `sra 0x1A`, four units at full scale), and both loops
    /// run at least once (`do`-`while`). So every swing of a string opens
    /// with the attacker one step either side of the boundary, whatever the
    /// previous clip's root motion and the target's knockback did to the
    /// gap: a retail trace of Vahn's `0x0D` -> `0x0F` commit steps him 60
    /// units forward in one frame (`autorun_melee_string_trace.lua`, the
    /// range poll's return address `0x8004C05C` repeating down to `0`).
    ///
    /// The engine had no counterpart, so a string's gap was set by root
    /// motion alone and drifted outside retail's.
    ///
    /// Retail's loops are unbounded: a target the law can never reach (the
    /// all-target sentinel, a slot `>= 8`) hangs them. The port bounds them
    /// at [`REACH_SNAP_MAX_STEPS`] and skips a target `>= 8`.
    // PORT: FUN_8004AD80 (`0x8004BF7C..0x8004C118`, the strike-loop reach snap)
    pub(in crate::world) fn commit_reach_snap(&mut self, slot: usize) {
        let Some(a) = self.actors.get(slot) else {
            return;
        };
        let target = a.battle.active_target;
        if target >= 8 || self.actors.get(usize::from(target)).is_none() {
            return;
        }
        let forward = self.battle_range_metric(slot as u8, target) != 0;
        for _ in 0..REACH_SNAP_MAX_STEPS {
            let (tx, tz) = self.battle_seat_of(usize::from(target));
            let ms = &self.actors[slot].move_state;
            let facing = legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
                tz, tx, ms.world_z, ms.world_x,
            )
            .wrapping_add(0x800)
                & 0xFFF;
            let (sin, cos) = legaia_engine_system::trig::trig12(facing);
            let (dx, dz) = (sin >> 10, cos >> 10);
            let a = &mut self.actors[slot];
            a.battle.facing_angle = facing;
            let ms = &mut a.move_state;
            if forward {
                ms.world_x = ms.world_x.wrapping_add(dx);
                ms.world_z = ms.world_z.wrapping_add(dz);
            } else {
                ms.world_x = ms.world_x.wrapping_sub(dx);
                ms.world_z = ms.world_z.wrapping_sub(dz);
            }
            let in_range = self.battle_range_metric(slot as u8, target) == 0;
            if in_range == forward {
                break;
            }
        }
    }

    /// Approach leg: facing recompute + range-gated root-motion step toward
    /// the target (see [`Self::tick_battle_locomotion`]).
    fn drive_attack_approach(&mut self, slot: usize, dt: u8) {
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
        let facing = legaia_engine_battle_vm::battle_action::bearing_12bit_approx(tz, tx, az, ax)
            .wrapping_add(0x800)
            & 0xFFF;
        self.actors[slot].battle.facing_angle = facing;
        // The positive-speed gate: no step once in range.
        if self.battle_range_metric(slot as u8, target) == 0 {
            return;
        }
        // A replayed retail park holds this seat's walk
        // ([`BattleState::approach_hold`]).
        if self.battle.approach_hold == Some(slot as u8) {
            return;
        }
        let (speed, scale) = self.battle_root_motion_of(slot);
        let (sin, cos) = legaia_engine_system::trig::trig12(facing);
        let raw = motion::root_motion_raw(sin, cos, speed.abs(), dt, scale);
        let frame = self.battle_frame_id();
        let (dx, dz) = self.actors[slot]
            .battle
            .root_carry
            .step_in_frame(frame, raw);
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

    /// The separation body radius of battle `slot`, the node `+0x58` word the
    /// seat stager writes: a party seat's [`PARTY_SEPARATION_RADIUS`] (or
    /// [`TERRA_SEPARATION_RADIUS`] on the roster-id-`4` seat), a monster's
    /// size class `<< 5` (`0x800518C4..0x800518D4`).
    ///
    /// REF: FUN_800513F0
    pub(in crate::world) fn battle_separation_radius(&self, slot: usize) -> i16 {
        let s = slot as u8;
        if s < self.party.party_count {
            if self.battle_party_char_id(s) == TERRA_ROSTER_ID {
                TERRA_SEPARATION_RADIUS
            } else {
                PARTY_SEPARATION_RADIUS
            }
        } else {
            i16::from(self.battle_size_class_of(s)) << 5
        }
    }

    /// PORT-adjacent driver for `FUN_80051078` / `FUN_80050BB8` (the kernels
    /// live in `legaia_engine_battle_vm::battle_separation`): one all-pairs
    /// separation pass in the retail visitation order, on the line after the
    /// action SM step - retail's exact call slot (`FUN_80046A20`).
    ///
    /// Retail's liveness test reads the slot pointer plus the actor's
    /// `+0x4` word; the engine substitutes its `liveness` halfword (the
    /// `+0x4` render word is not faithfully maintained here). Overlap is
    /// measured on the **seat** pairs and the nudge moves the **live**
    /// pairs, exactly as the kernel's field mapping documents. Radii:
    /// [`Self::battle_separation_radius`].
    ///
    /// REF: FUN_80051078, FUN_80050BB8, FUN_80046A20
    pub(in crate::world) fn tick_battle_separation(&mut self) {
        use legaia_engine_battle_vm::battle_separation::{
            SEPARATION_SLOTS, SepActor, push_apart, separation_pass,
        };
        let n = self.actors.len().min(SEPARATION_SLOTS);
        if n < 2 {
            return;
        }
        let mut alive = [false; SEPARATION_SLOTS];
        let mut seps = [SepActor::default(); SEPARATION_SLOTS];
        for (i, sep) in seps.iter_mut().enumerate().take(n) {
            let a = &self.actors[i];
            alive[i] = a.battle.liveness != 0;
            let radius = self.battle_separation_radius(i);
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
            let bearing =
                legaia_engine_battle_vm::battle_action::bearing_12bit_approx(b.z, b.x, a.z, a.x);
            let angle = bearing.wrapping_add(0x800) & 0xFFF;
            let (sin, cos) = legaia_engine_system::trig::trig12(angle);
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
    use legaia_engine_battle_vm::battle_action::{ActionState, StepOutcome};

    /// A world in battle mode with 1 party member and 1 monster at the
    /// authored solo seats (0,-800) vs (0,+800), on the per-vsync cadence:
    /// these drive the locomotion seat a tick at a time, which re-takes the
    /// body pairs at the head of each call (on retail's cadence the anim
    /// pass's draw takes them instead, `World::tick_battle_anim_pass`).
    fn battle_world() -> World {
        let mut w = World::new();
        w.toggles.battle_sm_frame_cadence = false;
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

    /// `FUN_80047430`'s in-range back-off (`0x80047C3C..0x80047D1C`): the
    /// acting attacker of an Attack, while actions execute, steps back along
    /// its facing by `(trig * step * +0x21D) >> 13` a frame for as long as
    /// the range law reads in range, so it settles just outside its reach;
    /// any other category stands still.
    #[test]
    fn an_attacker_inside_its_reach_backs_off_to_the_edge() {
        let run = |category: u8| {
            let mut w = battle_world();
            w.actors[0].battle.active_target = 1;
            w.actors[0].battle.action_category = category;
            w.actors[1].battle.liveness = 1;
            w.battle_ctx.active_actor = 0;
            w.battle_ctx.action_state = ActionState::AttackChain.as_byte();
            w.battle.round_flow.phase = crate::battle_round::RoundPhase::Execute;
            w.tick_battle_locomotion(); // seed seats
            // Stand the attacker deep inside its reach, facing the target.
            let (tx, tz) = (
                w.actors[1].move_state.world_x,
                w.actors[1].move_state.world_z,
            );
            w.actors[0].move_state.world_x = tx;
            w.actors[0].move_state.world_z = tz - 100;
            w.actors[0].battle.facing_angle = 0;
            for _ in 0..400 {
                w.tick_battle_locomotion();
            }
            (
                w.actors[0].move_state.world_z,
                tz,
                w.battle_range_metric(0, 1),
            )
        };
        let (z, tz, metric) = run(3);
        assert!(z < tz - 100, "backed away from the target: {z}");
        assert_ne!(metric, 0, "settled outside its reach");
        let (z_other, tz_other, _) = run(2);
        assert_eq!(
            z_other,
            tz_other - 100,
            "a non-Attack category stands still"
        );
    }

    /// `FUN_8004AD80`'s strike-loop tail: a commit by the acting actor in
    /// `0x1E` walks it in `trig >> 10` steps onto the edge of its reach -
    /// forward until the range law reads in range when it starts out of
    /// range, back until it reads out of range when it starts inside - and
    /// faces it at the target. Outside `0x1E`, or for another actor, a
    /// commit leaves the pair alone.
    #[test]
    fn a_strike_loop_commit_snaps_the_attacker_to_its_reach() {
        let setup = |z: i16, state: ActionState| {
            let mut w = battle_world();
            w.actors[0].battle.active_target = 1;
            w.actors[0].battle.action_category = 3;
            w.actors[1].battle.liveness = 1;
            w.battle_ctx.active_actor = 0;
            w.battle_ctx.action_state = state.as_byte();
            w.seed_battle_seats();
            w.actors[0].move_state.world_x = 0;
            w.actors[0].move_state.world_z = z;
            w
        };
        // Far out of range: one commit walks it in, to the first step that
        // reads in range.
        let mut w = setup(-800, ActionState::AttackChain);
        assert_ne!(w.battle_range_metric(0, 1), 0);
        w.note_active_clip_commit(0);
        let z_in = w.actors[0].move_state.world_z;
        assert!(z_in > -800);
        assert_eq!(w.battle_range_metric(0, 1), 0);
        let mut back = setup(z_in - 4, ActionState::AttackChain);
        assert_ne!(back.battle_range_metric(0, 1), 0, "one step short is out");
        assert_eq!(w.actors[0].battle.facing_angle, 0, "faced along +Z");
        // Deep inside: it backs out to the first step that reads out.
        back.actors[0].move_state.world_z = 700;
        assert_eq!(back.battle_range_metric(0, 1), 0);
        back.note_active_clip_commit(0);
        let z_out = back.actors[0].move_state.world_z;
        assert!(z_out < 700);
        assert_ne!(back.battle_range_metric(0, 1), 0);
        // Another state: no snap.
        let mut idle = setup(-800, ActionState::AttackWindup);
        idle.note_active_clip_commit(0);
        assert_eq!(idle.actors[0].move_state.world_z, -800);
        // Another actor's commit: no snap.
        let mut other = setup(-800, ActionState::AttackChain);
        other.note_active_clip_commit(1);
        assert_eq!(other.actors[0].move_state.world_z, -800);
    }

    /// A seat named by `approach_hold` (a replayed retail park) turns onto
    /// its target but takes no approach step; any other seat walks.
    #[test]
    fn a_held_approach_turns_but_does_not_walk() {
        let run = |hold: Option<u8>| {
            let mut w = battle_world();
            w.actors[0].battle.active_target = 1;
            w.actors[0].battle.action_category = 3;
            w.actors[1].battle.liveness = 1;
            w.battle_ctx.active_actor = 0;
            w.battle_ctx.action_state = ActionState::AttackShortStep.as_byte();
            w.battle.approach_hold = hold;
            let start = (
                w.actors[0].move_state.world_x,
                w.actors[0].move_state.world_z,
            );
            for _ in 0..20 {
                w.tick_battle_locomotion();
            }
            (
                start,
                (
                    w.actors[0].move_state.world_x,
                    w.actors[0].move_state.world_z,
                ),
            )
        };
        let (start, held) = run(Some(0));
        assert_eq!(held, start);
        let (start, walked) = run(Some(1));
        assert_ne!(walked, start);
        let (start, free) = run(None);
        assert_eq!(free, walked);
        assert_ne!(free, start);
    }

    /// The back-off's rounding: retail shifts the forward product and
    /// subtracts it (`sra v1,t1,0xd` / `subu`, `0x80047CCC..0x80047CD0`), so
    /// one battle frame moves the pair by `-floor(trig * step * scale /
    /// 8192)` per axis - never the floor of the negated product, which steps
    /// a unit further back on every fractional axis.
    #[test]
    fn the_back_off_floors_the_forward_product() {
        let mut w = battle_world();
        w.actors[0].battle.active_target = 1;
        w.actors[0].battle.action_category = 3;
        w.actors[1].battle.liveness = 1;
        w.battle_ctx.active_actor = 0;
        w.battle_ctx.action_state = ActionState::AttackChain.as_byte();
        w.battle.round_flow.phase = crate::battle_round::RoundPhase::Execute;
        w.tick_battle_locomotion(); // seed seats
        let (tx, tz) = (
            w.actors[1].move_state.world_x,
            w.actors[1].move_state.world_z,
        );
        w.actors[0].move_state.world_x = tx;
        w.actors[0].move_state.world_z = tz - 100;
        // A heading whose sine and cosine both leave a fraction.
        let facing = 100u16;
        w.actors[0].battle.facing_angle = facing;
        let scale = w.actors[0].battle.anim_rate.get();
        let scale = if scale == 0 {
            DEFAULT_SPEED_SCALE
        } else {
            scale
        };
        let (sin, cos) = legaia_engine_system::trig::trig12(facing);
        let step = |trig: i16| -((i32::from(trig) * 2 * i32::from(scale)) >> 13);
        // One battle frame of two vsyncs, on a fresh frame (the carry starts
        // over), with the attacker still inside its reach.
        let frame = w.battle_frame_id();
        while w.battle_frame_id() == frame {
            w.clock.display_frames += 1;
        }
        assert_eq!(w.battle_range_metric(0, 1), 0);
        let (x0, z0) = (
            w.actors[0].move_state.world_x,
            w.actors[0].move_state.world_z,
        );
        w.tick_battle_locomotion();
        w.tick_battle_locomotion();
        let (x1, z1) = (
            w.actors[0].move_state.world_x,
            w.actors[0].move_state.world_z,
        );
        assert_eq!(w.actors[0].battle.facing_angle, facing);
        assert_eq!(
            (i32::from(x1 - x0), i32::from(z1 - z0)),
            (step(sin), step(cos))
        );
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

    /// The body pair averages the playing stream's own parts: a clip
    /// expanded per assembled object carries equipment channels past them,
    /// and those stay out of the centroid.
    #[test]
    fn the_body_pair_averages_only_the_streams_parts() {
        use legaia_actor_formats::monster_archive::{MonsterAnimation, PartPose};
        let pose = |x: i16| PartPose {
            tx: x,
            ..PartPose::default()
        };
        let mut anim = MonsterAnimation {
            variant_track: Default::default(),
            action_id: 0,
            rate: 1,
            attach_key: 0,
            solo_flag: 0,
            impact_class: 0,
            effect_script: Vec::new(),
            part_count: 3,
            frame_count: 2,
            frames: vec![vec![pose(10), pose(20), pose(300)]; 2],
        };
        anim.variant_track.stream_parts = 2;
        let mut a = crate::world::Actor::default();
        a.battle_animation = legaia_engine_battle::battle_anim::MonsterAnimPlayer::new(&anim);
        a.pose_frame = Some(a.battle_animation.as_mut().unwrap().seat_cursor(0));
        assert_eq!(stream_pose(&a).map(pose_centroid_xz), Some((15, 0)));
        // An unrecorded count (a synthetic clip) keeps every channel.
        anim.variant_track.stream_parts = 0;
        a.battle_animation = legaia_engine_battle::battle_anim::MonsterAnimPlayer::new(&anim);
        assert_eq!(stream_pose(&a).map(pose_centroid_xz), Some((110, 0)));
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
        w.toggles.battle_sm_frame_cadence = false;
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

    /// The party radius is the seat stager's `0x280` (`0x400` on Terra's
    /// roster-id-4 seat), what every battle capture reads at the node's
    /// `+0x58` - not a monster size class.
    #[test]
    fn party_separation_radius_is_the_seat_stagers() {
        let mut w = World::new();
        w.enter_battle(2, 1);
        w.party.active_party = vec![0, 1];
        assert_eq!(w.battle_separation_radius(0), 640);
        assert_eq!(w.battle_separation_radius(1), 640);
        w.party.active_party = vec![0, 3];
        assert_eq!(w.battle_separation_radius(1), 1024, "Terra's seat");
    }

    /// Two party members pushed into each other part until the projected gap
    /// reaches retail's `(640 + 640) / 6 = 213` and no further: the spacing
    /// the separation pass holds between party members whom an action has
    /// walked together. The port's old `448` radius held them at `149`.
    #[test]
    fn overlapping_party_members_part_to_the_retail_gap() {
        let mut w = World::new();
        w.toggles.battle_sm_frame_cadence = false;
        w.enter_battle(2, 1);
        w.party.active_party = vec![0, 1];
        // Monster out of play; the two party seats 40 apart on X.
        w.actors[2].battle.liveness = 0;
        w.actors[0].move_state.world_x = 0;
        w.actors[0].move_state.world_z = -800;
        w.actors[1].move_state.world_x = 40;
        w.actors[1].move_state.world_z = -800;
        for _ in 0..400 {
            w.tick_battle_locomotion();
            w.tick_battle_separation();
        }
        let gap =
            i32::from(w.actors[1].move_state.world_x) - i32::from(w.actors[0].move_state.world_x);
        assert!(
            (213..213 + 8).contains(&gap),
            "party members part to the retail threshold, got {gap}"
        );
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
        // Near-coincident seats overlap: party radius 640 alone gives
        // threshold (640+0)/6 = 106 > proj 8, so both ordered pairs nudge the
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
        // The strike clip's commit in `0x1E` backs it out of its reach by
        // whole `trig >> 10` steps (`World::commit_reach_snap`), so it ends
        // a few units short of the deepest point, never back home.
        assert!(
            end_dist <= max_dist && max_dist - end_dist <= 32,
            "the attacker must hold the ground it closed to: {end_dist} vs {max_dist}"
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
