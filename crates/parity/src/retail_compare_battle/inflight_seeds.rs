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
    /// the walk off its start turns the shot. An acting seat that walked
    /// moves with its **target's** drift instead: the pair keeps the
    /// geometry of the first run (`battle_melee_hit_spark`'s Vahn stays on
    /// his side of the monster, at the same distance) and lands on retail's
    /// ground. One that did not walk (a caster) stays put.
    pub fn undrift(
        &self,
        drift: &[Option<[i32; 2]>; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS],
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
            *own = significant(*own).and_then(|_| significant(target_drift));
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
    /// [`Self::seeded_ground`] places a ground pair - on a capture past the
    /// end signal only. There the sequencer has stopped the action SM, so
    /// nothing recomputes a heading before the results framing reads it. A
    /// capture of a running fight replays its rounds, whose own swings set
    /// the headings; seeding the captured ones there moved the corpus both
    /// ways (camera up on most, frames down on more than rose).
    pub fn seeded_facing(&self) -> [Option<u16>; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS] {
        let mut out = [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS];
        if !self.span_gate.is_end() {
            return out;
        }
        let ground = self.seeded_ground();
        let pc = usize::from(self.party_count);
        for (s, o) in out.iter_mut().enumerate() {
            if ground[s].is_none() {
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
        if let Some(f) = g.fade.as_mut() {
            f.age = f.age.saturating_sub(self.display_lag);
        }
        if let Some(a) = g.cam_accum.as_mut() {
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
        Some(PhaseGate {
            action_state: self.action_state,
            fade: self.summon_fade,
            module_phase,
            cam_accum,
            walk_yaw,
            done_hold,
            module_countdown,
        })
    }
}
