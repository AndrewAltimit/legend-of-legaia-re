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
    /// Not carried: the monsters' half render scale (`+0x72 = 0x800`, and
    /// `+0x58` halved) through arms 0..26, which the battle body draw does
    /// not read.
    ///
    /// PORT: overlay_summon_juggernaut_0927_801f6a84 (PROT 0927; the seat, row-1 and backdrop writes of arms 0, 2, 18, 19, 21, 26, 27 and 28)
    pub(in crate::world::battle::cast_band) fn apply_juggernaut_writes(
        &mut self,
        phase: u8,
        passed: bool,
        monster_count: u8,
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
        // down on every tick.
        if phase == 0 && passed {
            self.casting.module_shake = 0;
        }
        self.juggernaut_shake();
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
            self.juggernaut_pool_flush();
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
                self.casting.module_cam.creature_floor_clip = false;
                self.casting.module_creature_clip_kind = 0;
                if let Some(a) = creature.and_then(|c| self.actors.get_mut(c)) {
                    a.battle.floor_clip = false;
                }
            }
            27 => {
                self.juggernaut_restore_monsters(&monster_slots);
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

    /// Run the op-`0x20` calls PROT 0927's parts made since its last tick
    /// through its stager `FUN_801F85A8`, in call order: the spawn arms seat
    /// their records on the caller, arms 3..5 raise the salmon fades and the
    /// shake word, and arm 4's monster-row sweep lands the cast's outcome
    /// (the band's fold, once: retail's arm 4 is where the damage numbers
    /// come up, long before the body's arm 26).
    ///
    /// REF: FUN_801F85A8
    pub(in crate::world::battle::cast_band) fn run_juggernaut_stager_calls(&mut self) {
        use legaia_engine_cast::cast_module_ticks as ticks;
        let calls = std::mem::take(&mut self.casting.module_stager_calls);
        for (arm, caller) in calls {
            for s in ticks::juggernaut_stager_spawns(arm, caller) {
                self.seat_stager_spawn(927, &s);
            }
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
                self.fold_pending_cast();
            }
        }
    }

    /// PROT 0927's epilogue shake, once per tick of its body
    /// ([`legaia_engine_cast::cast_module_ticks::juggernaut_shake_step`]).
    fn juggernaut_shake(&mut self) {
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

    /// PROT 0927's pool flush on arms 26 and 27 (`jal 0x80050E74`): every
    /// part it seated through the pool wrapper `FUN_80050ED4` - its own
    /// arms' and its stager's - halts; the two records it seats through
    /// `FUN_80021B04` run on.
    pub(in crate::world::battle::cast_band) fn juggernaut_pool_flush(&mut self) {
        use legaia_engine_cast::cast_module_camera::JUGGERNAUT_UNPOOLED;
        if let Some(scene) = self.casting.active_summon.as_mut() {
            for part in &mut scene.parts {
                if part.tag.is_some_and(|t| !JUGGERNAUT_UNPOOLED.contains(&t)) {
                    part.finished = true;
                }
            }
        }
    }
}
