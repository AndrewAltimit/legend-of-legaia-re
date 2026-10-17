//! What an in-flight capture seeds the engine with: the cast in flight, the
//! ground positions and their drift, status words, defeat lanes, facings and
//! the phase gate to stop on.

use super::*;

impl RetailBattle {
    /// The in-flight cast this capture holds, when the engine can replay it:
    /// a party seat running the summon band (`0x32..=0x36`) on a spell id.
    pub fn inflight_cast(&self) -> Option<legaia_engine_core::world::InflightCastSeed> {
        // The summon band itself, or the Done band a cast hands on to
        // (`0x37` / `0x38` then `0x50..=0x52`) while the caster still holds
        // the frame with its category `2` queued.
        let in_band = (0x32..=0x36).contains(&self.action_state)
            || (matches!(self.action_state, 0x37 | 0x38 | 0x50..=0x52)
                && self.queued_category == 2);
        (in_band
            && self.flow == 0xFF
            && self.active_actor < self.party_count
            && self.queued_action
                >= legaia_engine_battle_vm::battle_action::SPELL_TRIGGER_SUMMON_MIN_ID)
            .then(|| legaia_engine_core::world::InflightCastSeed {
                caster: self.active_actor,
                spell_id: self.queued_action,
                target: self.target_code,
                ground: self.engine_ground(),
            })
    }

    /// [`Self::ground`] re-keyed to engine battle slots: the party keeps its
    /// seats, monster pool slot `3 + m` is engine slot `party_count + m`.
    pub fn engine_ground(
        &self,
    ) -> [Option<[i16; 2]>; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS] {
        let mut out = [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS];
        let pc = usize::from(self.party_count);
        for (s, o) in out.iter_mut().enumerate().take(pc) {
            *o = self.ground.get(s).copied().flatten();
        }
        for m in 0..usize::from(self.monster_count) {
            if let Some(o) = out.get_mut(pc + m) {
                *o = self.ground.get(3 + m).copied().flatten();
            }
        }
        out
    }

    /// The ground pairs the seed puts on its first battle tick, by engine
    /// slot ([`BarSeed::ground`]).
    ///
    /// Retail walks nobody home after an action
    /// (`docs/subsystems/battle-action.md`, "Where an action leaves its
    /// combatants"), so a capture of a running fight stands its combatants
    /// wherever earlier rounds left them - a monster that struck earlier
    /// casts from beside the party - and every case that frames an actor
    /// (case 6 on a caster, case 0 on a member, case 9's formation box)
    /// frames that ground. A fresh entry's authored seats frame it elsewhere.
    /// The replayed cast carries the same pairs at its dispatch
    /// ([`Self::inflight_cast`]).
    ///
    /// Not placed on an opening capture, which is sampled at the flip before
    /// any round ran. The acting seat is placed too, even on a captured
    /// Attack whose `+0x34` / `+0x38` is a point on the walk the drive is
    /// about to replay: the walk ends at the target whatever it starts from,
    /// and leaving that seat home measured worse over the corpus.
    pub fn seeded_ground(
        &self,
    ) -> [Option<[i16; 2]>; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS] {
        if self.seed_plan() == SeedPlan::Opening {
            return [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS];
        }
        self.engine_ground()
    }
}

/// How [`RetailBattle::undrift`] moves the acting seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndriftActing {
    /// With its **target's** drift, when it walked at all: the pair keeps
    /// the first run's geometry (`battle_melee_hit_spark`'s Vahn stays on
    /// his side of the monster, at the same distance) and lands on retail's
    /// ground. A seat that did not walk (a caster) stays put.
    WithTarget,
    /// Back by its **own** drift, like every other combatant: the seat whose
    /// replayed travel is not its target's push (`rim_elm_gimard_seru_capture_*`'s
    /// Vahn ends a swing `108` units off the pair his target's drift names).
    Own,
}

impl UndriftActing {
    pub const ALL: [Self; 2] = [Self::WithTarget, Self::Own];

    /// How many times the rule is re-run on its own latest drift. The
    /// target's-drift rule is one step by construction (its acting seat is
    /// not placed by its own travel); the own-drift rule is a fixed-point
    /// iteration.
    pub const fn rounds(self) -> usize {
        match self {
            Self::WithTarget => 1,
            Self::Own => 3,
        }
    }
}

impl RetailBattle {
    /// This capture with every combatant's ground pair moved back by what
    /// the engine's replay of the action moved it
    /// (`EngineBattle::ground_drift`), or `None` when nothing moved enough
    /// to matter ([`UNDRIFT_MIN`]).
    ///
    /// A capture inside an action holds its combatants where the action had
    /// **already** moved them - the knockback a strike landed, a target
    /// shoved back by a hit - and the seed places them there before the
    /// drive replays that same action from its start, so every push lands a
    /// second time (`battle_gimard_tail_fire_a`: Vahn ends 129 units behind
    /// his captured pair, and the framing that follows him loses Gimard off
    /// the edge). No word in the capture holds the pre-action ground, but
    /// the engine's own replay measures the push: seeding
    /// `captured - drift` stands each combatant on retail's pair at the
    /// phase.
    ///
    /// The acting seat is the exception. Its own drift is its approach,
    /// which ends at its target from wherever it starts, and the direction
    /// it walks in is the heading every framing case subtracts - so taking
    /// the walk off its start turns the shot. `acting` picks its rule
    /// ([`UndriftActing`]); the caller tries both and keeps the run that
    /// lands nearer retail's ground.
    pub fn undrift(
        &self,
        drift: &[Option<[i32; 2]>; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS],
        acting_rule: UndriftActing,
    ) -> Option<Self> {
        // Past the end signal the sequencer poses the winners, and a win
        // pose's own travel is not a push the capture holds twice.
        if self.span_gate.is_end() {
            return None;
        }
        let pc = usize::from(self.party_count);
        let significant =
            |d: Option<[i32; 2]>| d.filter(|[dx, dz]| dx.abs().max(dz.abs()) >= UNDRIFT_MIN);
        let acting = usize::from(engine_seat(self.active_actor, self.party_count));
        let target = (self.target_code < 8)
            .then(|| usize::from(engine_seat(self.target_code, self.party_count)));
        let measured = *drift;
        let mut drift = measured;
        if let Some(own) = drift.get_mut(acting) {
            let target_drift = target
                .filter(|&t| t != acting)
                .and_then(|t| measured.get(t).copied().flatten());
            *own = match acting_rule {
                UndriftActing::WithTarget => {
                    significant(*own).and_then(|_| significant(target_drift))
                }
                UndriftActing::Own => significant(*own),
            };
        }
        let mut out = self.clone();
        let mut moved = false;
        for (slot, d) in drift.iter().enumerate() {
            let Some([dx, dz]) = significant(*d) else {
                continue;
            };
            let pool = if slot < pc { slot } else { 3 + (slot - pc) };
            let Some(Some([x, z])) = out.ground.get_mut(pool) else {
                continue;
            };
            let clamp = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
            *x = clamp(i32::from(*x) - dx);
            *z = clamp(i32::from(*z) - dz);
            moved = true;
        }
        moved.then_some(out)
    }

    /// [`Self::status`] re-keyed to engine battle slots.
    pub fn engine_status(&self) -> [Option<u16>; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS] {
        let mut out = [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS];
        let pc = usize::from(self.party_count);
        for (s, o) in out.iter_mut().enumerate() {
            let pool = if s < pc { s } else { 3 + (s - pc) };
            *o = self.status.get(pool).copied().flatten();
        }
        out
    }

    /// Per engine battle slot, whether the combatant had already acted in
    /// the round a pad-driven **party action** capture was taken in: alive,
    /// not the acting seat, and its initiative key [`Self::init_keys`] spent.
    ///
    /// The drive replays the capture's round from its prompt. Left with a
    /// key, such a combatant acts a second time before the capture's seat
    /// does: it walks off the ground the seed stood it on, and it swings at
    /// a party the capture holds already hurt by its first swing.
    /// `player_steal_skeleton_banner`'s lone Vahn stands on `17` HP behind
    /// two skeletons whose keys are spent - replayed, they kill him on all
    /// but one of the streams, and on that one the second skeleton walks
    /// `740` units before he moves.
    ///
    /// A victim the seed revives for the replayed kill
    /// ([`Self::action_victims`]) keeps its key, though the capture reads it
    /// spent as well: sat out, the two `rim_elm_gimard_seru_capture_*` drives
    /// frame Vahn's last swing some fifty yaw units further off retail's.
    ///
    /// A capture of a **monster's** action keeps every key: its drive plays
    /// the party's turns on purpose (the Spirit commits that keep a caster
    /// standing), and with the party sat out the three parked-Gaza frames
    /// measured worse (`image` `.718` to `.551` on the first).
    pub fn engine_spent_keys(&self) -> [bool; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS] {
        let mut out = [false; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS];
        let SeedPlan::Action { seat, .. } = self.seed_plan() else {
            return out;
        };
        if seat >= 3 {
            return out;
        }
        let pc = usize::from(self.party_count);
        let acting = usize::from(engine_seat(self.active_actor, self.party_count));
        for (s, o) in out.iter_mut().enumerate() {
            let pool = if s < pc { s } else { 3 + (s - pc) };
            let alive = if s < pc {
                self.party.get(s).copied().flatten()
            } else {
                self.monsters.get(s - pc).copied().flatten()
            }
            .is_some_and(|c| c.hp != 0);
            *o = s != acting && alive && self.init_keys.get(pool).copied().flatten() == Some(0);
        }
        out
    }

    /// Whether the acting seat has committed a strike onto its target this
    /// action: past the approach states, and inside the strike loop `0x1E`
    /// past the first staged swing - a committed swing clip, or a strike
    /// cursor `ctx[+0x15]` beyond the first queue byte. The loop stages one
    /// byte per swing and the swing clip commits at the idle cycle's end
    /// (`player_steal_skeleton_pre`: cursor `1`, clip `0`, the first swing
    /// `0x0F` staged).
    fn strike_committed(&self) -> bool {
        match self.action_state {
            0x14..=0x19 => false,
            0x1E => self.caster_clip != 0 || self.strike_cursor > 1,
            _ => true,
        }
    }

    /// [`Self::defeat_lanes`] re-keyed to engine battle slots.
    pub fn engine_defeat_lanes(
        &self,
    ) -> [Option<u32>; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS] {
        let mut out = [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS];
        let pc = usize::from(self.party_count);
        for (s, o) in out.iter_mut().enumerate() {
            let pool = if s < pc { s } else { 3 + (s - pc) };
            *o = self.defeat_lanes.get(pool).copied().flatten();
        }
        out
    }

    /// [`Self::facing`] re-keyed to engine battle slots, placed where
    /// [`Self::seeded_ground`] places a ground pair.
    ///
    /// - **Past the end signal**, every placed combatant: the sequencer has
    ///   stopped the action SM, so nothing recomputes a heading before the
    ///   results framing reads it.
    /// - **In a running fight**, only a combatant whose turn the capture holds
    ///   spent ([`Self::engine_spent_keys`]). The drive replays the round, and
    ///   the replayed turns set the headings of everyone who acts in it; a
    ///   spent combatant sits that round out, so the heading its own earlier
    ///   action left it on is one nothing in the replay restores. Seeding
    ///   every combatant's heading moved the corpus both ways (camera up on
    ///   most, frames down on more than rose).
    /// - **The action's target** only before the attacker's first strike has
    ///   committed. The strike commit turns its defender onto the attacker
    ///   (`0x801EEC34..0x801EECBC`), so after it the captured heading is the
    ///   action's own output, which the replay produces itself
    ///   (`rim_elm_gimard_victory`'s Gimard, struck twice). Before it, the
    ///   target's heading is what its own earlier turn left, and it moves the
    ///   approach: the body pair the range law `FUN_8004E2F0` measures is the
    ///   live pair plus the facing-rotated pose centroid, so the attacker
    ///   stops a walk step sooner or later, and the post-strike framing aims
    ///   at the pair (`player_steal_skeleton_pre`: the skeleton at `0x900`).
    pub fn seeded_facing(&self) -> [Option<u16>; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS] {
        let mut out = [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS];
        let end = self.span_gate.is_end();
        let mut spent = self.engine_spent_keys();
        if self.target_code < 8
            && self.strike_committed()
            && let Some(t) =
                spent.get_mut(usize::from(engine_seat(self.target_code, self.party_count)))
        {
            *t = false;
        }
        // The acting seat itself, on a capture taken before the attack band's
        // first face store (state `0x14` recomputes `+0x46` from the target
        // bearing, `0x801E3078..0x801E3088`): its heading is the one its
        // last turn left, which the replayed prompt does not restore, and the
        // seed's case-6 arm subtracts it (`super_queue_replace_*`: `34`).
        // The camera frames the seed's pass on both SM cadences
        // (`World::battle_camera_state`), so the heading reaches the frame
        // on either.
        if matches!(self.seed_plan(), SeedPlan::Action { .. })
            && self.action_state <= 0x14
            && let Some(a) = spent.get_mut(usize::from(engine_seat(
                self.active_actor,
                self.party_count,
            )))
        {
            *a = true;
        }
        let ground = self.seeded_ground();
        let pc = usize::from(self.party_count);
        for (s, o) in out.iter_mut().enumerate() {
            if ground[s].is_none() || !(end || spent[s]) {
                continue;
            }
            let pool = if s < pc { s } else { 3 + (s - pc) };
            *o = self.facing.get(pool).copied().flatten();
        }
        out
    }

    /// The phase the capture's **displayed frame** sits at: [`Self::phase_gate`]
    /// with the flash's age taken back by [`Self::display_lag`]. The RAM
    /// channels are sampled on the RAM's phase; the image is the frame the
    /// display was scanning out, two game frames older.
    pub fn display_phase_gate(&self) -> Option<PhaseGate> {
        let mut g = self.phase_gate()?;
        // The displayed frame is the pass that entered the state.
        if self.display_lag > 0 {
            g.camera_pass = false;
        }
        if let Some(f) = g.fade.as_mut() {
            f.age = f.age.saturating_sub(self.display_lag);
        }
        for a in [g.cam_accum.as_mut(), g.accum_align.as_mut()]
            .into_iter()
            .flatten()
        {
            *a = a.saturating_sub(u32::from(self.display_lag) * 8);
        }
        if let Some(d) = g.done_hold.as_mut() {
            // The countdown drains by the frame step a frame; the displayed
            // frame is that many vsyncs older, so its countdown was higher.
            *d = d.saturating_add(self.display_lag as i16);
        }
        if let Some(c) = g.module_countdown.as_mut() {
            // The countdown drains `scalar` a vsync; the displayed frame's
            // word was that much higher.
            *c += i32::from(self.display_lag)
                * legaia_engine_battle_vm::cast_module_camera::MODULE_DRAIN_PER_TICK;
        }
        if let Some(y) = g.walk_yaw.as_mut() {
            // `6 * scalar` a vsync, taken back no further than the seat.
            let per_vsync = 6 * legaia_engine_battle_vm::cast_module_camera::MODULE_DRAIN_PER_TICK;
            let back = (i32::from(self.display_lag) * per_vsync).min(walk_swing(i32::from(*y)));
            *y = ((i32::from(*y) - back) & 0xFFF) as u16;
        }
        Some(g)
    }

    /// The phase an in-flight capture's engine frame is gated on.
    pub fn phase_gate(&self) -> Option<PhaseGate> {
        self.inflight_cast()?;
        // The player summon block is linear: PROT `903 + (id - 0x81)`.
        let entry = u32::from(self.queued_action).wrapping_sub(0x81) + 903;
        let directed = (0x81..=0xA0).contains(&self.queued_action)
            && legaia_engine_battle_vm::cast_module_camera::module_profile(entry)
                .is_some_and(|p| p.paces_band());
        let module_phase =
            (directed && (0x35..=0x36).contains(&self.action_state)).then_some(self.module_phase);
        // A `0x33` frame before the flash-in is placed by how long the invoke
        // clip has run: the accumulator its commit zeroed.
        let cam_accum = (self.action_state == 0x33
            && self.summon_fade.is_none()
            && self.caster_clip == SUMMON_INVOKE_CLIP)
            .then_some(self.cam_accum);
        // PROT 0903's walk arm hands the camera to case 6 on the walking
        // creature; its yaw base says how far in the frame is.
        let walk_yaw = (entry == GIMARD_MODULE && module_phase == Some(GIMARD_WALK_ARM))
            .then_some(self.walk_yaw_base);
        let done_hold = match self.span_gate {
            SpanGate::DoneHold { timer } => Some(timer),
            _ => None,
        };
        // PROT 0903's countdown-paced arms: how far into the arm the frame is.
        let module_countdown = (entry == GIMARD_MODULE
            && module_phase.is_some_and(|p| (1..GIMARD_WALK_ARM).contains(&p)))
        .then_some(self.gimard_countdown);
        // The close-up's accumulator is a clock of its own - a second one
        // beside the flash's age, and a pass-quantised one in retail - so
        // it is aligned once the gate holds.
        let accum_align = ((0x33..=0x34).contains(&self.action_state)
            && self.caster_clip == SUMMON_INVOKE_CLIP)
            .then_some(self.cam_accum);
        // A frame inside the stream poll's arm is placed by the arm alone.
        let stream_wait = module_phase.is_some_and(|p| {
            legaia_engine_battle_vm::cast_module_camera::stream_poll_arm(entry) == Some(p)
        });
        Some(PhaseGate {
            action_state: self.action_state,
            fade: self.summon_fade,
            module_phase,
            cam_accum,
            walk_yaw,
            done_hold,
            module_countdown,
            accum_align,
            // The summon head's RAM is a pass in (`PhaseGate::steer`).
            camera_pass: self.action_state == SUMMON_INVOKE_STATE,
            stream_wait,
        })
    }
}
