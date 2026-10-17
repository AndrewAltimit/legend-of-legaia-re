//! Cast-module hit application: the area-of-effect runs, the capture roll, hit
//! finish, spirit refresh and the status-roll sweep.

use super::*;

impl World {
    /// Run the band's two whole-row AoE stagers - PROT 0927 (Juggernaut,
    /// `FUN_801F85A8`) and PROT 0966 (Evil Seru Magic, `FUN_801F8D64`) -
    /// which is where those two casts' damage actually lands in retail.
    ///
    /// Both sweep a seat range with the module's own guards (skip dead, skip
    /// `+0x16E & 4`) and both clamp to `HP - 1`, so neither **sweep** can
    /// kill. That is a property of these two stagers only - PROT 0927's own
    /// tick (`0x801F6A84`) uses the unsigned `sltu` clamp instead and is
    /// kill-capable (`legaia_engine_cast::cast_module_ticks`,
    /// `docs/subsystems/cast-module.md`). ESM also
    /// stages each victim's own `+0x1F1` reaction and drops its animation rate
    /// to `2`. The roll per seat is the module's **baked** power through the
    /// module's own wrapper, drawn off this world's RNG cursor so the draw
    /// order stays retail's.
    ///
    /// Returns `None` when the spell names neither module, so the ordinary
    /// [`Self::fold_pending_cast`] path is unaffected.
    pub fn run_cast_module_aoe(&mut self, spell_id: u8, arm: u8) -> Option<CastModuleCodeRun> {
        self.run_cast_module_aoe_as(self.battle_ctx.active_actor, spell_id, arm)
    }

    /// [`Self::run_cast_module_aoe`] at the cast band's fold seam: the caster
    /// is the [`PendingCast`]'s, not whoever the context happens to point at,
    /// and the arm is the working one of the module's nine.
    pub(in crate::world::battle::cast_band) fn run_cast_module_aoe_for(
        &mut self,
        caster: u8,
        spell_id: u8,
    ) -> Option<CastModuleCodeRun> {
        self.run_cast_module_aoe_as(caster, spell_id, AOE_STAGER_WORKING_ARM)
    }

    pub(in crate::world::battle::cast_band) fn run_cast_module_aoe_as(
        &mut self,
        caster: u8,
        spell_id: u8,
        arm: u8,
    ) -> Option<CastModuleCodeRun> {
        use legaia_engine_cast::cast_module_ticks as ticks;

        let entry = self.cast_module_for(spell_id)?;
        let shape = ticks::damage_shape_for(entry).filter(|s| s.never_kills)?;
        let ctx = self.cast_module_ctx();

        let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();

        // Pre-roll so the sweep borrows nothing from `self`, but roll only
        // for the seats the sweep will actually hit and in the order it visits
        // them: retail's loop calls the wrapper *after* its two skip guards,
        // so a dead or non-targetable seat draws nothing and the shared RNG
        // cursor must not advance for it either.
        let order: Vec<u8> = if entry == 966 {
            (0..ctx.party_count).collect()
        } else {
            (0..ctx.monster_count)
                .map(|i| ticks::FIRST_MONSTER_SEAT.saturating_add(i))
                .collect()
        };
        let mut rolls: Vec<(u8, i32)> = Vec::with_capacity(order.len());
        // `order` is in the kernels' retail-shaped row; the roll reads the
        // engine slot behind each seat.
        let map = self.cast_seat_map();
        for seat in order {
            let hittable = seats
                .get(seat as usize)
                .is_some_and(ticks::aoe_seat_is_hittable);
            let Some(target) = map.engine(seat).filter(|_| hittable) else {
                continue;
            };
            let roll = self
                .capture_module_roll(shape, caster, target)
                .unwrap_or_default();
            rolls.push((seat, roll));
        }
        self.refresh_seat_spirit(&mut seats);
        let take = |seat: u8| {
            rolls
                .iter()
                .find(|(s, _)| *s == seat)
                .map(|(_, r)| *r)
                .unwrap_or_default()
        };

        let hits = if entry == 966 {
            ticks::evil_seru_magic_stager(&ctx, &mut seats, arm, take)
        } else {
            ticks::juggernaut_stager(&ctx, &mut seats, arm, take)
        };
        self.write_cast_seat_row(&seats);
        let hits = self.cast_hits_to_engine(hits);
        Some(CastModuleCodeRun {
            prot_entry: entry,
            phase: ctx.phase,
            ctx_278: ctx.ctx_278,
            busy: false,
            tick_ported: true,
            aoe_hits: hits,
            item_refund: None,
            voided_accessory: None,
            element_change: None,
            camera_shot: None,
            camera_follow: None,
            camera_end_frame: false,
            camera_nudge: None,
            capture_drift: None,
            spawns: &[],
            every_spawns: &[],
            arm_point: None,
            vram_move: None,
            caption: None,
            fades: &[],
            kills_fades: false,
        })
    }

    /// One hit off a module's own damage shape: its baked power, its wrapper,
    /// this world's RNG cursor. The raw signed wrapper return - the caller
    /// applies the module's clamp.
    pub(in crate::world::battle::cast_band) fn capture_module_roll(
        &mut self,
        shape: &legaia_engine_cast::cast_module_ticks::CastDamageShape,
        attacker: u8,
        target: u8,
    ) -> Option<i32> {
        use legaia_engine_battle_vm::battle_damage_wrappers::{WrapperAttacker, WrapperDefender};
        use legaia_engine_cast::cast_module_ticks::roll_module_hit;

        let element_affinity_pct = self.enemy_affinity_pct(attacker, target);
        let attacker_hp = self.actors.get(attacker as usize)?.battle.hp;
        let defender = self.summon_roll_defender(target)?;
        let a = WrapperAttacker {
            hp: attacker_hp,
            agl: self
                .battle
                .accuracy
                .get(attacker as usize)
                .copied()
                .unwrap_or(0),
            spell_power: self
                .battle
                .attack
                .get(attacker as usize)
                .copied()
                .unwrap_or(0),
            status: 0,
        };
        let d = WrapperDefender {
            hp: defender.hp,
            agl: defender.agl,
            stat_a: defender.stat_a,
            stat_b: defender.stat_b,
            status: 0,
            guard: 0,
        };
        let rng = [
            self.next_rand() as u16,
            self.next_rand() as u16,
            self.next_rand() as u16,
        ];
        let net = roll_module_hit(shape, 0, &a, &d, element_affinity_pct, rng, || {
            self.next_rand() as u16
        });
        Some(self.finish_module_hit(shape, attacker, target, net))
    }

    /// The finisher half of a module wrapper hit. Both per-move wrappers end
    /// in the shared finisher: `FUN_801DD4B0` calls `jal 0x801DDB30` at
    /// `0x801DD678` on its own attacker / defender roll words and returns
    /// their difference afterwards (`subu v0,v1,v0` at `0x801DD6A8`), so the
    /// net a module stores into `+0x14C` is **post**-finisher: the party
    /// resist ladder (skipped by `FUN_801DD6B4`'s `param_5 = 1`), Mystic
    /// Shield's enemy-defender halve, the guard halve (`+0x1DE == 4`), the
    /// no-damage floor and the `9999` cap - and the finisher's spirit stage
    /// fills the defender's gauge from the same hit. The earlier port
    /// returned the raw wrapper net, so a guarding member took Cort's Mystic
    /// Circle whole and gained no Spirit from it.
    ///
    /// `SharedSummon` (PROT 0927, `FUN_801DD0AC`'s summon branch) keeps the
    /// raw net: its finisher arguments (attacker slot `7`, the power-percent
    /// scale) are the shared kernel's own and are not modelled here.
    ///
    /// PORT: FUN_801DD4B0 (`0x801DD66C..0x801DD6AC`, the finisher call and return)
    pub(in crate::world::battle::cast_band) fn finish_module_hit(
        &mut self,
        shape: &legaia_engine_cast::cast_module_ticks::CastDamageShape,
        attacker: u8,
        target: u8,
        net: i32,
    ) -> i32 {
        use legaia_engine_battle_vm::battle_damage_wrappers::{
            ATK_WRAPPER_BYPASSES_PARTY_RESIST, INT_WRAPPER_BYPASSES_PARTY_RESIST,
        };
        use legaia_engine_battle_vm::battle_formulas::{DamageFinish, damage_finish_lazy};
        use legaia_engine_cast::cast_module_ticks::CastWrapper;

        let bypass_party_resist = match shape.wrapper {
            CastWrapper::Respect => INT_WRAPPER_BYPASSES_PARTY_RESIST,
            CastWrapper::Bypass => ATK_WRAPPER_BYPASSES_PARTY_RESIST,
            CastWrapper::SharedSummon => return net,
        };
        let party_count = self.party.party_count;
        let attacker_element = self.monster_seat_element(attacker as usize).unwrap_or(7);
        let finish = DamageFinish {
            predamage: net.max(0) as u32,
            attacker_slot: if attacker < party_count { 0 } else { 3 },
            defender_slot: if target < party_count { 0 } else { 3 },
            attacker_element,
            defender_resist: self.defender_resist(target),
            defender_guarding: self
                .battle
                .guarding
                .get(target as usize)
                .copied()
                .unwrap_or(false),
            enemy_defender_halve: self.mystic_shield_up(),
            bypass_party_resist,
            summon_power_pct: 100,
            floor_rand: 0,
        };
        let over = damage_finish_lazy(&finish, || self.next_rand() as u16).min(9999);
        self.accrue_spirit_gauge(target, over as u16);
        over as i32
    }

    /// Carry the spirit gauges [`Self::finish_module_hit`] filled into seat
    /// snapshots taken before the rolls, so the tick's write-back does not
    /// restore the pre-hit gauge. `seats` is the retail-shaped row
    /// ([`Self::cast_seat_row`]).
    pub(in crate::world::battle::cast_band) fn refresh_seat_spirit(
        &self,
        seats: &mut [legaia_engine_cast::cast_module_ticks::CastActorState],
    ) {
        let map = self.cast_seat_map();
        for (seat, st) in seats.iter_mut().enumerate() {
            if let Some(a) = map
                .engine(seat as u8)
                .and_then(|e| self.actors.get(usize::from(e)))
            {
                st.spirit_gauge = a.battle.spirit_gauge;
            }
        }
    }

    /// Pre-roll the status draws one whole-row sweep makes, in the order
    /// retail visits its seats: the two `FUN_80056798` calls PROT 0938's
    /// `0x4E` body makes per hittable seat (`0x801F7888` / `0x801F78B0`),
    /// which decide Venom then Toxic.
    ///
    /// The **damage** roll comes first, per seat, because retail's loop calls
    /// the wrapper before the status draws: each of the three sweeps opens
    /// its seat body with `jal 0x801DD4B0` on its own baked power
    /// ([`legaia_engine_cast::cast_module_ticks::sweep_damage_shape_for`]) and stores the
    /// clamped net into `+0x14C` itself. So these bodies **own** the cast's
    /// HP outcome the way the PROT 0927 / 0966 stagers do, and
    /// [`Self::fold_pending_cast`] must not also run the generic fold - which
    /// is the double-application trap. The clamp is the kill-capable one
    /// (`sltu` at `0x801F77EC` / `0x801F7118` / `0x801F77E0`), unlike the two
    /// stagers' `HP - 1`.
    ///
    /// Returns `(seat, damage, (status_a, status_b))` in visit order.
    pub(in crate::world::battle::cast_band) fn sweep_status_rolls(
        &mut self,
        ctx: &legaia_engine_cast::cast_module_ticks::CastModuleCtx,
        seats: &[legaia_engine_cast::cast_module_ticks::CastActorState],
        body: u32,
        caster: u8,
    ) -> Vec<(u8, i32, (u32, u32))> {
        use legaia_engine_cast::cast_module_ticks as ticks;
        // Only the body's own sweep arm draws anything: retail reaches the
        // wrapper and the two status calls inside that one arm, so rolling on
        // every re-entry would run the shared `rand()` cursor forward on
        // frames retail never draws.
        let sweep_arm = match body {
            ticks::CHAOS_BREATH_TICK => ticks::CHAOS_BREATH_SWEEP_ARM,
            ticks::MYSTIC_CIRCLE_TICK => ticks::MYSTIC_CIRCLE_SWEEP_ARM,
            _ => ticks::DOOMSDAY_SWEEP_ARM,
        };
        if ctx.phase != sweep_arm {
            return Vec::new();
        }
        let shape = ticks::sweep_damage_shape_for(body);
        let mut out = Vec::new();
        for seat in 0..ctx.party_count {
            let Some(s) = seats.get(seat as usize) else {
                continue;
            };
            // PROT 0938's `0xB7` body and PROT 0965's skip only a dead seat;
            // the `0x4E` body also skips `+0x16E & 4`.
            let hittable = if body == ticks::CHAOS_BREATH_TICK {
                ticks::aoe_seat_is_hittable(s)
            } else {
                s.hp != 0
            };
            if !hittable {
                continue;
            }
            let damage = match shape {
                Some(sh) => self.capture_module_roll(sh, caster, seat).unwrap_or(0),
                None => 0,
            };
            out.push((seat, damage, (self.next_rand(), self.next_rand())));
        }
        out
    }
}
