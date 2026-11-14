//! The seat, stage and creature writes PROT 0927 (Juggernaut)'s arms make
//! that its tick body's port (`cast_module_ticks::juggernaut_tick`) does not
//! carry: who is drawn, where the monsters stand, the creature's emergence
//! through object-effect row 1, and the backdrop drop.

use super::*;

/// The neutral tint word an arm shows a seat with (`lui 0x2008; ori 0x200`).
const SHOWN_TINT: u32 = 0x2008_0200;
/// `+0x21C = 0xFF`: the seat is hidden.
const HIDDEN: u8 = 0xFF;

impl World {
    /// The epilogue's copy on every pass, held or not (`0x801F832C..`): arm
    /// 23's first kept part carries the position its program moves, and the
    /// other thirteen handles take its `+0x14..+0x1B`, so the rings and the
    /// ray fan stand on one moving point.
    ///
    /// PORT: overlay_summon_juggernaut_0927_801f6a84 (PROT 0927; the position copy at `0x801F832C..0x801F8570`)
    fn juggernaut_ride_lead(&mut self) {
        use legaia_engine_cast::cast_module_camera::{
            JUGGERNAUT_ARM23_LEAD, JUGGERNAUT_ARM23_RIDERS,
        };
        self.parts_ride_lead(JUGGERNAUT_ARM23_LEAD, &JUGGERNAUT_ARM23_RIDERS);
    }

    /// PROT 0966's copy of the same shape (`0x801F8AF0..0x801F8D30`).
    ///
    /// PORT: overlay_cast_evil_seru_magic_0966_801f6a74 (PROT 0966; the position copy at `0x801F8AF0..0x801F8D30`)
    pub(in crate::world::battle) fn evil_seru_ride_lead(&mut self) {
        use legaia_engine_cast::cast_module_camera::{
            EVIL_SERU_ARM23_LEAD, EVIL_SERU_ARM23_RIDERS,
        };
        self.parts_ride_lead(EVIL_SERU_ARM23_LEAD, &EVIL_SERU_ARM23_RIDERS);
    }

    fn parts_ride_lead(&mut self, lead: u32, riders: &[u32]) {
        let Some(scene) = self.casting.active_summon.as_mut() else {
            return;
        };
        let Some(lead) = scene
            .parts
            .iter()
            .find(|p| !p.finished && p.tag == Some(lead))
        else {
            return;
        };
        let (x, y, z) = (lead.state.world_x, lead.state.world_y, lead.state.world_z);
        for part in scene
            .parts
            .iter_mut()
            .filter(|p| !p.finished && p.tag.is_some_and(|t| riders.contains(&t)))
        {
            part.state.world_x = x;
            part.state.world_y = y;
            part.state.world_z = z;
        }
    }

    /// Apply PROT 0927's arm `phase` writes for one pass of its director;
    /// `passed` is whether the arm's countdown let it through.
    ///
    /// | arm | writes (on its pass) |
    /// |---|---|
    /// | 0 (`0x801F6B30..0x801F6D0C`) | each live monster: pose saved, moved to the alternate-family seat `0x80077628 + (count + 3) * 0x20` at `x / 3`, `z / 2`, facing `0`, hidden; the backdrop pair's `+0x78 = 0x1000` |
    /// | 2 (`0x801F6DC8..0x801F7044`) | object-effect row 1 = angles `0`, bounds `(-0x300, 0)`, creature `+0x42 = 2`, seated at `(0, _, 0x1C0)` facing `0x800`; party hidden; each live monster shown |
    /// | 18 (`0x801F7400..0x801F74D4`) | row 1 = angles `(0x400, 0, 0)`, bounds `(-0x800, 0x800)`; creature at `y = 0x100`, `z = 0x800`; each live monster at `z = -0x400`, hidden |
    /// | 19 (`0x801F750C..0x801F7520`, every pass) | creature `z -= 3` a tick |
    /// | 21 (`0x801F772C..0x801F77A4`) | each live monster shown |
    /// | 26 (`0x801F7CEC`) | creature `+0x42 = 0` |
    /// | 27 (`0x801F805C..0x801F8170`) | each live monster back on its saved pose; creature hidden; caster shown |
    /// | 28 (`0x801F81D8..0x801F828C`, every pass) | each live monster back on its saved pose |
    ///
    /// Arm 0 also halves each monster's render scale (`+0x72 = 0x800`) and
    /// arm 27 puts it back to `0x1000`; not carried: the body radius `+0x58`
    /// arm 0 halves beside it.
    ///
    /// PORT: overlay_summon_juggernaut_0927_801f6a84 (PROT 0927; the seat, row-1 and backdrop writes of arms 0, 2, 18, 19, 21, 26, 27 and 28)
    pub(in crate::world::battle::cast_band) fn apply_juggernaut_writes(
        &mut self,
        phase: u8,
        passed: bool,
        monster_count: u8,
        spell_id: u8,
    ) {
        let party = usize::from(self.party.party_count);
        let monsters = usize::from(monster_count);
        let map = self.cast_seat_map();
        let monster_slots: Vec<(usize, usize)> = (0..monsters)
            .filter_map(|i| map.engine(3 + i as u8).map(|e| (i, usize::from(e))))
            .filter(|&(_, e)| self.actors.get(e).is_some_and(|a| a.battle.hp != 0))
            .collect();
        let creature = self.casting.summon_actor_slot.map(usize::from);
        // Arm 0 zeroes the shake word (`0x801F6B3C`); the epilogue runs it
        // down once a pass.
        if phase == 0 && passed {
            self.casting.module_shake = 0;
        }
        self.juggernaut_shake();
        self.juggernaut_ride_lead();
        // Arm 19 walks the creature every pass, ahead of its gate.
        if phase == 19
            && let Some(a) = creature.and_then(|c| self.actors.get_mut(c))
        {
            a.move_state.world_z = a.move_state.world_z.wrapping_sub(3);
        }
        // The creature's clips and rates (`s3 = actor_table[7]`): arms 19 and
        // 24 store the playing clip every pass with no restage bump
        // (`0x801F7524`, `0x801F7AD4`); arms 12 and 14 freeze the rate once
        // the clip frame `+0x68` reaches `0x240` / `0x2D0` (`0x801F70F0`,
        // `0x801F724C`); arms 11, 13, 18 and 23 set the rate and step the
        // staged clip on their pass.
        if let Some(slot) = creature.map(|c| c as u8) {
            let mut s = self.cast_view(slot);
            // The render node's clip cursor `+0x68` (`lh v0,0x68(v0)`).
            let frame = self
                .actors
                .get(usize::from(slot))
                .and_then(|a| a.battle_animation.as_ref())
                .map_or(0, |p| p.cursor_sixteenths());
            match phase {
                19 => s.staged_anim = 3,
                24 => s.staged_anim = 5,
                12 if frame >= 0x240 => s.anim_rate = 0,
                14 if frame >= 0x2D0 => s.anim_rate = 0,
                _ => {}
            }
            if passed {
                match phase {
                    11 => s.anim_rate = 8,
                    13 | 18 => {
                        s.anim_rate = 8;
                        let next = s.staged_anim.wrapping_add(1);
                        legaia_engine_cast::cast_module_ticks::stage_clip(&mut s, next);
                    }
                    23 => {
                        s.anim_rate = 2;
                        let next = s.staged_anim.wrapping_add(1);
                        legaia_engine_cast::cast_module_ticks::stage_clip(&mut s, next);
                    }
                    _ => {}
                }
            }
            self.write_cast_view(slot, &s);
        }
        if phase == 28 {
            self.juggernaut_restore_monsters(&monster_slots);
        }
        if !passed {
            return;
        }
        // The flush runs ahead of the arm's own spawns, which the band seats
        // after the module's pass.
        if legaia_engine_cast::cast_module_camera::juggernaut_flushes_pool(phase) {
            self.casting.juggernaut_pool_flush();
        }
        match phase {
            0 => {
                self.casting.module_saved_poses.clear();
                for &(i, e) in &monster_slots {
                    let seat =
                        legaia_engine_battle::battle_seats::monster_seat(monsters as u8, i, true);
                    let Some(a) = self.actors.get_mut(e) else {
                        continue;
                    };
                    self.casting.module_saved_poses.push((
                        e,
                        [
                            a.move_state.world_x,
                            a.move_state.world_y,
                            a.move_state.world_z,
                        ],
                        a.battle.facing_angle,
                    ));
                    a.move_state.world_x = seat.x / 3;
                    a.move_state.world_z = seat.z / 2;
                    a.battle.facing_angle = 0;
                    // The render node at half scale (`sh 0x800,0x72` at
                    // `0x801F6C7C`), back to unity on arm 27 (`0x801F80F0`).
                    a.battle.render_scale = Some(0x800);
                    a.battle.render_color = 0;
                    a.battle.render_flag = HIDDEN;
                }
                self.battle.backdrop_cue =
                    legaia_engine_battle_vm::battle_ground_grid::BACKDROP_CUE_FULL;
            }
            2 => {
                self.object_effect.write(1, [0, 0, 0, -0x300, 0]);
                self.casting.module_creature_clip_kind = 2;
                self.casting.module_cam.creature_floor_clip = true;
                if let Some(a) = creature.and_then(|c| self.actors.get_mut(c)) {
                    a.battle.floor_clip = true;
                }
                for s in 0..party {
                    if let Some(a) = map
                        .engine(s as u8)
                        .and_then(|e| self.actors.get_mut(usize::from(e)))
                    {
                        a.battle.render_color = 0;
                        a.battle.render_flag = HIDDEN;
                    }
                }
                for &(_, e) in &monster_slots {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.battle.render_color = SHOWN_TINT;
                        a.battle.render_flag = 0;
                    }
                }
            }
            18 => {
                self.object_effect.write(1, [0x400, 0, 0, -0x800, 0x800]);
                if let Some(a) = creature.and_then(|c| self.actors.get_mut(c)) {
                    a.move_state.world_y = 0x100;
                    a.move_state.world_z = 0x800;
                }
                for &(_, e) in &monster_slots {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.move_state.world_z = -0x400;
                        a.battle.render_color = 0;
                        a.battle.render_flag = HIDDEN;
                    }
                }
            }
            21 => {
                for &(_, e) in &monster_slots {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.battle.render_color = SHOWN_TINT;
                        a.battle.render_flag = 0;
                    }
                }
            }
            26 => {
                self.juggernaut_final_hits(spell_id);
                self.casting.module_cam.creature_floor_clip = false;
                self.casting.module_creature_clip_kind = 0;
                if let Some(a) = creature.and_then(|c| self.actors.get_mut(c)) {
                    a.battle.floor_clip = false;
                }
            }
            27 => {
                self.juggernaut_restore_monsters(&monster_slots);
                for &(_, e) in &monster_slots {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.battle.render_scale = Some(0x1000);
                    }
                }
                if let Some(a) = creature.and_then(|c| self.actors.get_mut(c)) {
                    a.battle.render_color = 0;
                    a.battle.render_flag = HIDDEN;
                }
                let caster = usize::from(self.battle_ctx.active_actor);
                if let Some(a) = self.actors.get_mut(caster) {
                    a.battle.render_color = SHOWN_TINT;
                    a.battle.render_flag = 0;
                }
            }
            _ => {}
        }
    }

    /// Arms 27 / 28's restore of the poses arm 0 saved (`0x801F80B0..`,
    /// `0x801F823C..`): position and facing, for each live monster.
    fn juggernaut_restore_monsters(&mut self, live: &[(usize, usize)]) {
        for &(slot, pos, facing) in &self.casting.module_saved_poses {
            if !live.iter().any(|&(_, e)| e == slot) {
                continue;
            }
            if let Some(a) = self.actors.get_mut(slot) {
                [
                    a.move_state.world_x,
                    a.move_state.world_y,
                    a.move_state.world_z,
                ] = pos;
                a.battle.facing_angle = facing;
            }
        }
    }

    /// One sweep of the stager's arms 3 and 4 (arm 3 falls into arm 4 at
    /// `0x801F8698`). A party cast makes it on each of those calls and the rolls
    /// are its damage, so the band folds nothing and only banks their XP; a
    /// monster's cast of the id has no such roll and lands through the fold.
    fn juggernaut_stager_sweep(&mut self, spell_id: u8) {
        // The band has its stager out of the world for this tick.
        let caster = self.battle_ctx.active_actor;
        if caster >= self.party.party_count {
            self.fold_pending_cast();
            return;
        }
        self.run_cast_module_aoe_as(caster, spell_id, super::super::AOE_STAGER_WORKING_ARM);
        if self.casting.pending_cast.is_some() {
            self.casting.module_skips_fold = true;
            self.fold_pending_cast();
        }
        self.bank_juggernaut_hit_xp(caster, spell_id);
    }

    /// The fold banks only the first sweep's rolls, and the later hits have no
    /// fold to bank through.
    fn bank_juggernaut_hit_xp(&mut self, caster: u8, spell_id: u8) {
        let gain = std::mem::take(&mut self.casting.module_hit_xp);
        if gain != 0 {
            self.accrue_summon_spell_xp(caster, spell_id, gain);
        }
    }

    /// The kill-capable hit on arm 26's pass (`0x801F7DD4..0x801F7F24`): every
    /// live monster takes the summon roll again, clamped to its HP, staging its
    /// reaction with the animation rate at `2`.
    fn juggernaut_final_hits(&mut self, spell_id: u8) {
        use legaia_engine_cast::cast_module_ticks as ticks;
        // The band has its stager out of the world for this tick.
        let caster = self.battle_ctx.active_actor;
        if caster >= self.party.party_count {
            return;
        }
        let ctx = self.cast_module_ctx();
        let map = self.cast_seat_map();
        let mut seats = self.cast_seat_row();
        for i in 0..ctx.monster_count {
            let seat = ticks::FIRST_MONSTER_SEAT.saturating_add(i);
            let hittable = seats
                .get(usize::from(seat))
                .is_some_and(ticks::aoe_seat_is_hittable);
            let Some(target) = map.engine(seat).filter(|_| hittable) else {
                continue;
            };
            let roll = self
                .summon_module_roll(caster, target, spell_id)
                .map_or(0, |r| r as i32);
            if let Some(v) = seats.get_mut(usize::from(seat)) {
                ticks::apply_hit_floor_zero(v, roll);
                legaia_engine_cast::seru_magic_ticks::stage_reaction_bits(v);
                v.anim_rate = 2;
            }
        }
        self.write_cast_seat_row(&seats);
        self.bank_juggernaut_hit_xp(caster, spell_id);
    }

    /// Run the op-`0x20` calls PROT 0927's parts made since its last tick
    /// through its stager `FUN_801F85A8`, in call order (the spawn arms'
    /// seats are made on the caller's frame, behind the part tick): arms 3..5 raise the salmon fades and the
    /// shake word, and arm 4's monster-row sweep lands the cast's outcome
    /// (the band's fold, once: retail's arm 4 is where the damage numbers
    /// come up, long before the body's arm 26).
    ///
    /// REF: FUN_801F85A8
    pub(in crate::world::battle::cast_band) fn run_juggernaut_stager_calls(
        &mut self,
        spell_id: u8,
    ) {
        use legaia_engine_cast::cast_module_ticks as ticks;
        let calls = std::mem::take(&mut self.casting.module_stager_calls);
        // The spawn arms' seats were made on the caller's frame
        // (`World::run_summon_part_stager_calls`).
        for (arm, _) in calls {
            if let Some(t) = ticks::juggernaut_stager_fade(arm) {
                let mut fade = None;
                legaia_engine_system::fade::spawn_fade(
                    &mut fade,
                    &legaia_engine_battle_vm::battle_action::summon_fade_template(&t),
                    1,
                );
                self.presentation.module_fades.extend(fade);
            }
            self.casting.module_shake += ticks::juggernaut_stager_shake(arm);
            if arm == super::super::AOE_STAGER_WORKING_ARM || arm == 3 {
                self.juggernaut_stager_sweep(spell_id);
            }
        }
    }

    /// PROT 0927's epilogue shake, once per call of its body
    /// ([`legaia_engine_cast::cast_module_ticks::juggernaut_shake_step`]);
    /// PROT 0966's body carries the same epilogue over its own word.
    ///
    /// Retail enters the body once a battle frame, so the word steps once a
    /// pass, whatever the frame step: the `0x99` recording's arm 12 holds
    /// TR y on `-208 + 16 t` plus an offset that alternates by `8 * w` a
    /// frame for twenty frames (stager arm 4's `0x14`). Stepped on every
    /// one-vsync tick of a step-`4` pass, the word ran out in five frames,
    /// netting `-16` a frame where retail swings.
    pub(in crate::world::battle::cast_band) fn juggernaut_shake(&mut self) {
        if !self.module_tick_opens_pass() {
            return;
        }
        let dy = legaia_engine_cast::cast_module_ticks::juggernaut_shake_step(
            &mut self.casting.module_shake,
        );
        if dy != 0
            && let Some(cam) = self.battle.camera.as_mut()
        {
            cam.drift_module(0, 0, dy as i16, 0);
        }
    }

    /// The NUL-terminated string at slot-B VA `va` of spell `spell_id`'s
    /// module image ([`ModuleCaption::ModuleString`]), read off the disc's
    /// copy the effect pool holds. `None` disc-free.
    ///
    /// [`ModuleCaption::ModuleString`]: legaia_engine_cast::cast_module_camera::ModuleCaption::ModuleString
    pub fn cast_module_string(&self, spell_id: u8, va: u32) -> Option<String> {
        const LINK_BASE: u32 = legaia_overlay_images::summon_overlay::SUMMON_OVERLAY_LINK_BASE;
        let entry = self.cast_module_for(spell_id)?;
        let module = self.casting.effect_pool.as_ref()?.module(entry)?;
        let off = va.checked_sub(LINK_BASE)? as usize;
        let tail = module.bytes.get(off..)?;
        let end = tail.iter().position(|&b| b == 0)?;
        std::str::from_utf8(&tail[..end]).ok().map(str::to_string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_cast::cast_module_camera::{JUGGERNAUT_ARM23_LEAD, JUGGERNAUT_ARM23_RIDERS};

    /// The epilogue stands arm 23's other kept parts on the first one's point,
    /// and a part outside the handle table keeps its own.
    #[test]
    fn the_kept_parts_ride_the_first() {
        let mut w = World::new();
        let part = |tag: u32, x: i16| {
            let mut p = legaia_engine_effects::summon::SummonPartRuntime {
                model_sel: -1,
                reserved: 0,
                buf: vec![0x08],
                state: Default::default(),
                finished: false,
                origin: Some([0; 3]),
                tag: Some(tag),
                seat_pokes: Vec::new(),
                seat_halve_speed: false,
                halted_walks: 0,
                walk_reached: false,
            };
            p.state.world_x = x;
            p
        };
        let mut scene =
            legaia_engine_effects::summon::SummonScene::spawn_parts(&[], &[], 0, [0; 3]);
        scene.parts.push(part(JUGGERNAUT_ARM23_LEAD, 77));
        scene.parts.push(part(JUGGERNAUT_ARM23_RIDERS[12], 0));
        scene.parts.push(part(0x801F_91D0, 5));
        w.casting.active_summon = Some(scene);
        w.juggernaut_ride_lead();
        let parts = &w.casting.active_summon.as_ref().unwrap().parts;
        assert_eq!(parts[1].state.world_x, 77);
        assert_eq!(parts[2].state.world_x, 5);
    }
}
