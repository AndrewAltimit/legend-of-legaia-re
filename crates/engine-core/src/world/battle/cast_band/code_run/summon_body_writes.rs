//! The summon-creature directors' writes outside the camera
//! ([`legaia_engine_cast::cast_module_camera`]'s whole-chain directors):
//! the colour words their arms drive on the creature and the victim,
//! PROT 0916's (Aluru) kill verdict and instant kill, and PROT 0931's (Jedo)
//! kill of the monster row.

use super::*;

/// The PROT entry of Jedo's module (`summon_jedo`).
const JEDO_ENTRY: u32 = 931;

impl World {
    /// Draw PROT 0916's arm-0 verdict once per cast and hold it on the
    /// module state ([`ModuleCamState::kill`]), the way retail parks it in
    /// `0x801F9010` before any later arm reads it. The skip condition is
    /// Nighto's: the scripted-fight byte and the victim record's `+0x20`,
    /// unless the victim carries `+0x16E` bit `4`.
    ///
    /// [`ModuleCamState::kill`]: legaia_engine_cast::cast_module_camera::ModuleCamState::kill
    ///
    /// REF: overlay_summon_aluru_0916_801f69f8 (`0x801F6D44..0x801F6E44`)
    pub(in crate::world::battle::cast_band) fn draw_aluru_verdict(
        &mut self,
        caster_slot: u8,
        victim_slot: u8,
        spell_id: u8,
    ) {
        use legaia_engine_cast::cast_module_camera as cam;
        if self.casting.module_cam.kill.is_some() {
            return;
        }
        let level = self.caster_magic_power_byte(caster_slot, spell_id);
        let skipped = self.battle_ctx.scripted_fight != 0
            && self
                .actors
                .get(victim_slot as usize)
                .and_then(|a| a.battle_monster_id)
                .and_then(|id| self.tables.monster_catalog.get(id))
                .is_some_and(|def| def.wide_texture_page != 0)
            && self.cast_view(victim_slot).flags
                & legaia_engine_cast::cast_module_ticks::FLAG_NON_TARGETABLE
                == 0;
        let roll = (!skipped).then(|| self.next_rand());
        self.casting.module_cam.kill = Some(cam::aluru_kill_verdict(roll, level));
    }

    /// The colour-word and HP stores of the summon-creature directors' arms
    /// on the pass the director just made for `phase_in` of module `entry`
    /// (`passed`: it left the arm).
    ///
    /// - The creature's `+0x1BA` as the director owns it
    ///   ([`ModuleCamState::creature_shatter`]): PROT 0916's arms 4..6
    ///   assemble it, PROT 0914's arm `0xE` breaks it up.
    /// - The victim's step ([`ModuleCamState::victim_shatter_step`]): PROT
    ///   0916's arm `0x13` on its kill path (`0x801F81AC..0x801F81BC`).
    /// - PROT 0916's arm `0x11`, kill path, on its passing tick: the victim's
    ///   HP to `0` (`sh zero,0x14C(s3)` at `0x801F7E14`), and the band's fold
    ///   owes nothing - the damage path is the only one that reaches the hit.
    /// - PROT 0917's arm `0xE` pass: the creature into the defeat fade
    ///   (`+0x21C = 2`, [`cam::BARRA_CREATURE_FADE_ARM`]).
    /// - PROT 0931 on a fight that is not scripted: arm 29 makes no damage
    ///   call (the fold is waived), arm 30 adds `delta << 2` to every living
    ///   monster's `+0x1BA` a pass (`0x801F86E0..0x801F874C`) and its exit
    ///   zeroes their HP (`0x801F8814`).
    ///
    /// [`ModuleCamState::creature_shatter`]: legaia_engine_cast::cast_module_camera::ModuleCamState::creature_shatter
    /// [`ModuleCamState::victim_shatter_step`]: legaia_engine_cast::cast_module_camera::ModuleCamState::victim_shatter_step
    pub(in crate::world::battle::cast_band) fn apply_summon_body_writes(
        &mut self,
        entry: u32,
        phase_in: u8,
        passed: bool,
        victim_slot: u8,
        seat_slot: u8,
    ) {
        use legaia_engine_cast::cast_module_camera as cam;
        let st = self.casting.module_cam;
        let victim = st.victim_slot.unwrap_or(victim_slot);
        if entry == cam::ALURU_ENTRY
            && phase_in == cam::ALURU_BLAST_ARM
            && passed
            && st.kill == Some(true)
        {
            let mut v = self.cast_view(victim);
            v.hp = 0;
            self.write_cast_view(victim, &v);
            self.casting.module_skips_fold = true;
        }
        if st.victim_shatter_step != 0
            && let Some(a) = self.actors.get_mut(usize::from(victim))
        {
            a.battle.shatter = a.battle.shatter.wrapping_add(st.victim_shatter_step);
        }
        if let Some(w) = st.creature_shatter
            && let Some(a) = self.actors.get_mut(usize::from(seat_slot))
        {
            a.battle.shatter = w;
        }
        if entry == 917
            && phase_in == cam::BARRA_CREATURE_FADE_ARM
            && passed
            && let Some(a) = self.actors.get_mut(usize::from(seat_slot))
        {
            a.battle.render_flag = 2;
        }
        if entry == JEDO_ENTRY && self.battle_ctx.scripted_fight == 0 {
            if phase_in == cam::JEDO_HIT_ARM && passed {
                self.casting.module_skips_fold = true;
            }
            if phase_in == cam::JEDO_KILL_ARM {
                for slot in self.living_monster_slots() {
                    if let Some(a) = self.actors.get_mut(slot) {
                        a.battle.shatter = a
                            .battle
                            .shatter
                            .wrapping_add(cam::JEDO_SHATTER_STEP_PER_TICK);
                    }
                }
                if passed {
                    for slot in self.living_monster_slots() {
                        let mut v = self.cast_view(slot as u8);
                        v.hp = 0;
                        self.write_cast_view(slot as u8, &v);
                    }
                }
            }
        }
    }

    /// The non-lethal pre-hits four summon-creature modules land before the
    /// band's fold, on the pass that makes them: each target's roll is the
    /// summon branch of `FUN_801DD0AC` ([`Self::player_summon_predamage`]),
    /// clamped to `HP - 1`
    /// ([`legaia_engine_cast::seru_magic_ticks::summon_pre_hit`]).
    ///
    /// - PROT 0915 arm 7's pass: the latched victim, the roll quartered and
    ///   floored at `1` (`0x801F7004..`); skipped for a victim with
    ///   `+0x16E` bit `4`.
    /// - PROT 0928 arm `0xF`: once, on the first pass its word falls below
    ///   `0x1C1` (`0x801F80C4..0x801F81EC`), every living targetable monster.
    /// - PROT 0932 arm `0x12` and PROT 0933 arm `0xC`, on their pass: every
    ///   living monster.
    ///
    /// Not carried: the per-seat tallies `0x801F6978` / `0x801F6980` the
    /// quartered roll is checked against, the first argument's `0x10` /
    /// `0x12` mode (the predamage port has one summon branch), and PROT 0934
    /// arm `0x18`'s per-pass ladder. PROT 0929 arm 16's is
    /// [`Self::mule_wave_hit`].
    pub(in crate::world::battle::cast_band) fn apply_summon_pre_hits(
        &mut self,
        entry: u32,
        phase_in: u8,
        passed: bool,
        caster: u8,
        spell_id: u8,
    ) {
        use legaia_engine_cast::cast_module_ticks::FLAG_NON_TARGETABLE;
        let st = self.casting.module_cam;
        let targets: Vec<u8> = match (entry, phase_in) {
            (915, 7) if passed => st
                .victim_slot
                .filter(|&v| self.cast_view(v).flags & FLAG_NON_TARGETABLE == 0)
                .into_iter()
                .collect(),
            (928, 0xF) if st.countdown.0 < 0x1C1 && st.wait != (0xF, 1) => {
                // Once: the arm hides the row after its sweep, so later
                // passes find nothing to hit.
                self.casting.module_cam.wait = (0xF, 1);
                self.living_monster_slots()
                    .into_iter()
                    .map(|s| s as u8)
                    .filter(|&s| self.cast_view(s).flags & FLAG_NON_TARGETABLE == 0)
                    .collect()
            }
            (932, 0x12) | (933, 0xC) if passed => self
                .living_monster_slots()
                .into_iter()
                .map(|s| s as u8)
                .collect(),
            _ => return,
        };
        for t in targets {
            let Some(mut dmg) = self.player_summon_predamage(caster, t, spell_id) else {
                continue;
            };
            if entry == 915 {
                dmg = (dmg >> 2).max(1);
            }
            let mut v = self.cast_view(t);
            if v.hp == 0 {
                continue;
            }
            legaia_engine_cast::seru_magic_ticks::summon_pre_hit(&mut v, u32::from(dmg));
            self.write_cast_view(t, &v);
        }
    }

    /// PROT 0930's (Horn) seat visibility, on the passes that write it: arm
    /// 1 shows the party (`+0x04 = 0x20080200`, `+0x21C = 0`,
    /// `0x801F6C78..`), arm 3 seats the creature hidden (`+0x21C = 0xFF`, `+0x04 = 0`,
    /// `0x801F6E98..`) and arm 4 shows it white (`+0x04 = 0x3FFFFFFF`,
    /// `+0x21C = 0`, `0x801F6FD4..0x801F6FFC`), the idle arm of the tint
    /// state machine easing it back to neutral; arm 8 hides the party
    /// (`0x801F7428..`); arm `0xA`, the heal, shows the party again and
    /// relights the creature for the closing burst: `+0x04 = 0x3FFFFFFF`,
    /// `+0x0C = 0x1000`, `+0x21C = 0xFF` (the hold state, which leaves the
    /// word drawn), standing at `+0x36 = 0x480`, `+0x38 = -0x200`, facing
    /// `0` (`0x801F77A8..0x801F77EC`); arm `0xB`'s pass hides it
    /// (`+0x21C = 0xFF`, `+0x04 = 0`, `0x801F7D04..0x801F7D10`). The engine's
    /// shown word is its resting `0` under a lowered flag. Arm 1's hide of
    /// the monster row is not carried: nothing in the module shows it again,
    /// and the band's own reset is not modelled.
    pub(in crate::world::battle::cast_band) fn apply_horn_writes(
        &mut self,
        phase_in: u8,
        passed: bool,
        seat_slot: u8,
    ) {
        use legaia_engine_battle_vm::battle_target_group::RENDER_FLAG_HIDDEN;
        /// The white word arms 4 and `0xA` store (`lui 0x3FFF; ori 0xFFFF`).
        const WHITE: u32 = 0x3FFF_FFFF;
        if !passed {
            return;
        }
        let set = |a: &mut crate::world::Actor, hidden: bool| {
            a.battle.render_flag = if hidden { RENDER_FLAG_HIDDEN } else { 0 };
            a.battle.render_color = 0;
        };
        if let Some(a) = self.actors.get_mut(usize::from(seat_slot)) {
            match phase_in {
                3 | 0xB => set(a, true),
                4 => {
                    a.battle.render_flag = 0;
                    a.battle.render_color = WHITE;
                }
                0xA => {
                    a.battle.render_flag = RENDER_FLAG_HIDDEN;
                    a.battle.render_color = WHITE;
                    a.battle.render_blend = 0x1000;
                    a.move_state.world_y = 0x480;
                    a.move_state.world_z = -0x200;
                    a.battle.facing_angle = 0;
                }
                _ => {}
            }
        }
        let party = match phase_in {
            1 | 0xA => Some(false),
            8 => Some(true),
            _ => None,
        };
        if let Some(hidden) = party {
            let pc = usize::from(self.party.party_count);
            for a in self.actors.iter_mut().take(pc) {
                set(a, hidden);
            }
        }
    }

    /// PROT 0929's (Mule) monster-row visibility. Arm 1 shows every living
    /// monster on each of its passes, held or not (`+0x04 = 0x20080200`,
    /// `+0x21C = 0`, `0x801F6D04..0x801F6D50`, ahead of the stream-token
    /// gate), so the row stands in the underwater backdrop while the bubbles
    /// rise; arm 5's pass hides it for the creature's entrance
    /// (`+0x04 = 0`, `+0x21C = 0xFF`, `0x801F7040..`), arm 11's shows it
    /// again for the attack, turned to face the party at half its render
    /// scale (`+0x46 = 0x800`, node `+0x72 >>= 1`, `0x801F7590..0x801F75C8`;
    /// the arm also halves `+0x21D`, `0x801F75D4..0x801F75E0`, which is not
    /// carried: under it the engine's hit reaction ran arm 19's settle poll
    /// `210` vsyncs against retail's `143`), arm 16's pass hides it,
    /// doubles the scale back and drops it to the floor (`+0x36 = 0`,
    /// `0x801F842C..0x801F8460`; arm 17 starts at `0x801F8488`), and the hit
    /// arm 18 shows each struck monster (`0x801F87C4..`). The engine's shown
    /// word is its resting `0` under a lowered flag. Each pass also halts the parts the arm kills by their
    /// parked handles and runs out the loops arm 16 ends
    /// ([`legaia_engine_cast::cast_module_camera::mule_arm_kills`],
    /// [`legaia_engine_cast::cast_module_camera::mule_arm_loop_ends`]).
    /// PROT 0929's per-pass tail (`0x801F8B08..0x801F8BF8`): arm 0 clears
    /// the follow word and arm 9's pass sets it; while it is set every pass
    /// ends by pointing the scanline backdrop's sample origin
    /// (`ctx[+0x285]` / `+0x286`) at the live camera, after the arm's own
    /// stores ([`legaia_engine_cast::cast_module_camera::mule_backdrop_origin`]).
    pub(in crate::world::battle::cast_band) fn mule_backdrop_follow(
        &mut self,
        phase_in: u8,
        passed: bool,
    ) {
        use legaia_engine_cast::cast_module_camera::{
            MULE_BACKDROP_FOLLOW_ARM, mule_backdrop_origin,
        };
        if phase_in == 0 {
            self.casting.module_mule_backdrop_follow = false;
            return;
        }
        if phase_in == MULE_BACKDROP_FOLLOW_ARM && passed {
            self.casting.module_mule_backdrop_follow = true;
        }
        if !self.casting.module_mule_backdrop_follow {
            return;
        }
        let pose = self.battle_cam_pose();
        let (u, v) = mule_backdrop_origin(
            pose.pitch as i32,
            pose.yaw as i32,
            pose.tr[0] as i32,
            pose.tr[1] as i32,
        );
        let wash = &mut self.casting.module_pres.wash;
        wash.u_base = u;
        wash.v_base = v;
    }

    pub(in crate::world::battle::cast_band) fn apply_mule_writes(
        &mut self,
        phase_in: u8,
        passed: bool,
        monster_count: u8,
        caster: u8,
        spell_id: u8,
    ) {
        use legaia_engine_battle_vm::battle_target_group::RENDER_FLAG_HIDDEN;
        self.mule_row_seat(phase_in, passed, monster_count, caster, spell_id);
        // Arms 15 and 16 copy the creature's display trio over the position
        // of two of arm 14's parked parts on every pass, held or not
        // (`lwl` / `lwr` of `+0x3C..+0x43` into `+0x14..+0x1B` through the
        // handles at `0x801F9A98` / `0x801F9A9C`; `0x801F7BD8..0x801F7C20`,
        // `0x801F81BC..0x801F820C`), so they ride with the creature.
        if matches!(phase_in, 15 | 16)
            && let Some(body) = self
                .casting
                .summon_actor_slot
                .and_then(|c| self.battle_display_trio(usize::from(c)))
            && let Some(scene) = self.casting.active_summon.as_mut()
        {
            for part in scene.parts.iter_mut().filter(|p| {
                !p.finished
                    && p.tag.is_some_and(|t| {
                        legaia_engine_cast::cast_module_camera::MULE_CREATURE_RIDERS.contains(&t)
                    })
            }) {
                part.state.world_x = body[0] as i16;
                part.state.world_y = body[1] as i16;
                part.state.world_z = body[2] as i16;
            }
        }
        if passed && let Some(scene) = self.casting.active_summon.as_mut() {
            let kills = legaia_engine_cast::cast_module_camera::mule_arm_kills(phase_in);
            let ends = legaia_engine_cast::cast_module_camera::mule_arm_loop_ends(phase_in);
            for part in &mut scene.parts {
                if part.tag.is_some_and(|t| kills.contains(&t)) {
                    part.finished = true;
                } else if part.tag.is_some_and(|t| ends.contains(&t)) {
                    part.state.field_8c = 0;
                }
            }
        }
        // The creature: arm 11's pass lifts it to `+0x36 = -0x100` facing
        // `0` (`0x801F7668..0x801F7674`), arm 14's lights it white and holds
        // the word (`+0x04 = 0x3FFFFFFF`, `+0x0C = 0x1000`, `+0x21C = 0xFF`,
        // `0x801F7AB0..0x801F7AF0`, beside its clip-2 stage), and arm 16's
        // hides it (`+0x21C = 0xFF`, `+0x04 = 0`, `0x801F83C4..0x801F83D0`).
        if passed
            && matches!(phase_in, 11 | 14 | 16)
            && let Some(c) = self
                .casting
                .summon_actor_slot
                .and_then(|s| self.actors.get_mut(usize::from(s)))
        {
            match phase_in {
                11 => {
                    c.move_state.world_y = -0x100;
                    c.battle.facing_angle = 0;
                }
                14 => {
                    c.battle.render_color = 0x3FFF_FFFF;
                    c.battle.render_blend = 0x1000;
                    c.battle.render_flag = RENDER_FLAG_HIDDEN;
                }
                _ => {
                    c.battle.render_color = 0;
                    c.battle.render_flag = RENDER_FLAG_HIDDEN;
                }
            }
        }
        let hidden = match (phase_in, passed) {
            (1, _) | (11 | 18, true) => false,
            (5 | 16, true) => true,
            _ => return,
        };
        let pc = usize::from(self.party.party_count);
        for a in self.actors.iter_mut().skip(pc) {
            if a.battle_monster_id.is_none() || a.battle.hp == 0 {
                continue;
            }
            a.battle.render_flag = if hidden { RENDER_FLAG_HIDDEN } else { 0 };
            a.battle.render_color = 0;
            let scale = a.battle.render_scale.unwrap_or(0x1000);
            match (phase_in, passed) {
                (11, true) => {
                    a.battle.facing_angle = 0x800;
                    a.battle.render_scale = Some(scale >> 1);
                }
                (16, true) => {
                    a.battle.render_scale = Some(scale.wrapping_shl(1));
                    a.move_state.world_y = 0;
                    // `sh zero,0x48` (`0x801F8444`): the roll back to rest.
                    a.battle.render_roll = 0;
                }
                _ => {}
            }
        }
    }

    /// PROT 0929's (Mule) monster-row seat. Arm 0 parks each living
    /// monster's ground trio and facing (`0x801F9A58` / `0x801F9A78`) and
    /// stands it on the alternate-family seat `0x80077628 + (count + 3) *
    /// 0x20` at half its `x` and half its `z` less `0x226`, facing `0x800`
    /// (`0x801F6C04..0x801F6CF0`); the last arm `0xFF` puts every living
    /// monster back on its parked trio and facing (`0x801F8A70..0x801F8AE8`).
    /// Arm 16's every pass walks the row's wave front `0x801F9AC8` (from
    /// arm 15's `-0x100`, `+ step * scalar / 2` a pass) and lifts each
    /// monster it has passed `4 * step * scalar` a pass up to `+0x36 = -0x100`
    /// (`0x801F7D9C..0x801F8108`) and rolls it `+0x48 += 16 * step *
    /// scalar` while the Mystic Shield is down (`0x801F8110..0x801F8164`);
    /// the strike on each passed monster is [`Self::mule_wave_hit`].
    fn mule_row_seat(
        &mut self,
        phase: u8,
        passed: bool,
        monster_count: u8,
        caster: u8,
        spell_id: u8,
    ) {
        let monsters = usize::from(monster_count);
        let map = self.cast_seat_map();
        let slots: Vec<(usize, usize)> = (0..monsters)
            .filter_map(|i| map.engine(3 + i as u8).map(|e| (i, usize::from(e))))
            .filter(|&(_, e)| self.actors.get(e).is_some_and(|a| a.battle.hp != 0))
            .collect();
        let d = legaia_engine_cast::cast_module_camera::DELTA_PER_TICK;
        let t = d * legaia_engine_cast::cast_module_camera::SPEED_SCALAR;
        match (phase, passed) {
            (0, true) => {
                self.casting.module_saved_poses.clear();
                for &(i, e) in &slots {
                    let seat =
                        legaia_engine_battle::battle_seats::monster_seat(monsters as u8, i, true);
                    let Some(a) = self.actors.get_mut(e) else {
                        continue;
                    };
                    let m = &mut a.move_state;
                    self.casting.module_saved_poses.push((
                        e,
                        [m.world_x, m.world_y, m.world_z],
                        a.battle.facing_angle,
                    ));
                    m.world_x = seat.x / 2;
                    m.world_z = seat.z / 2 - 0x226;
                    a.battle.facing_angle = 0x800;
                }
            }
            (15, true) => self.casting.module_mule_front = -0x100,
            (18, _) => self.battle.combo_cluster_held = false,
            (16, _) => {
                // The wave's popups go up without opening the combo cluster:
                // each hit clears the readout flag the damage kernel raised
                // (`sb zero,0x8007B64C` at `0x801F7E60`), as PROT 0959's
                // wrapper hits do. The hit arm 18's own kernel call raises it
                // again, so the hold ends there.
                self.battle.combo_cluster_held = true;
                self.casting.module_mule_front += t / 2;
                let front = self.casting.module_mule_front;
                let shield = self.mystic_shield_up();
                for &(_, e) in &slots {
                    self.mule_wave_hit(e, front, shield, caster, spell_id);
                    let Some(a) = self.actors.get_mut(e) else {
                        continue;
                    };
                    let m = &mut a.move_state;
                    if i32::from(m.world_z) >= front {
                        continue;
                    }
                    if m.world_y >= -0xFF {
                        m.world_y = m.world_y.wrapping_sub((4 * t) as i16);
                    }
                    // The roll `+0x48 += 16 * step * scalar` (`0x801F8110..
                    // 0x801F8164`), skipped while the Mystic Shield is up.
                    if !shield {
                        a.battle.render_roll =
                            a.battle.render_roll.wrapping_add((16 * t) as u16) & 0xFFF;
                    }
                }
            }
            (0xFF, _) => {
                for &(e, pos, facing) in &self.casting.module_saved_poses {
                    if !slots.iter().any(|&(_, s)| s == e) {
                        continue;
                    }
                    if let Some(a) = self.actors.get_mut(e) {
                        [
                            a.move_state.world_x,
                            a.move_state.world_y,
                            a.move_state.world_z,
                        ] = pos;
                        a.battle.facing_angle = facing;
                    }
                }
            }
            _ => {}
        }
    }

    /// One pass of PROT 0929 arm 16's wave hit on monster slot `slot`
    /// (`0x801F7DD8..0x801F80A8`). A living, targetable monster with no clip
    /// playing (`+0x1D9 == 0`) that the wave front `front` has passed, with
    /// the Mystic Shield down, takes a third of the summon roll
    /// `FUN_801DD0AC(0x10, 7, seat)` - `rand() % 16 + 16` when that comes to
    /// zero - plus `rand() % (hit / 8 + 1)`, its popup pushed with that
    /// amount (`FUN_801F44A0`), the HP write clamped to `HP - 1` (the wave
    /// cannot kill) and its reaction clip `+0x1EF` (else `+0x1F0`) staged with
    /// the restage bits `5`. A living monster the wave does not hit this pass
    /// has its staged clip cleared (`sb zero,0x1DA`, `0x801F809C..0x801F80A8`).
    ///
    /// Not carried: the damage tallies the roll is booked against
    /// (`0x8007BD14`, or `0x801F6980 + seat * 2` under a group target), and
    /// the byte `0x8007B64C` the hit clears.
    fn mule_wave_hit(&mut self, slot: usize, front: i32, shield: bool, caster: u8, spell_id: u8) {
        use legaia_engine_cast::cast_module_ticks::FLAG_NON_TARGETABLE;
        let s = slot as u8;
        let v = self.cast_view(s);
        if v.hp == 0 {
            return;
        }
        let z = self
            .actors
            .get(slot)
            .map_or(0, |a| i32::from(a.move_state.world_z));
        // `+0x1D9 == 0`: no clip playing - the engine's reaction channel
        // included, where a staged reaction plays (`battle_current_anim`).
        let hittable = v.flags & FLAG_NON_TARGETABLE == 0 && self.battle_current_anim(slot) == 0;
        if !hittable || z >= front || shield {
            let mut v = v;
            v.staged_anim = 0;
            self.write_cast_view(s, &v);
            return;
        }
        let Some(roll) = self.player_summon_predamage(caster, s, spell_id) else {
            return;
        };
        let mut hit = u32::from(roll) / 3;
        if hit == 0 {
            hit = self.next_rand() % 16 + 16;
        }
        hit += self.next_rand() % (hit / 8 + 1);
        self.battle
            .hit_fx
            .push(legaia_engine_battle::battle_events::BattleHitFx {
                target_slot: s,
                amount: hit.min(u32::from(u16::MAX)) as u16,
                is_heal: false,
                is_crit: false,
            });
        let mut v = self.cast_view(s);
        legaia_engine_cast::seru_magic_ticks::summon_pre_hit(&mut v, hit);
        // Arm 16 always ORs both restage bits (`ori v0,v0,0x5`).
        v.restage |= 0x05;
        self.write_cast_view(s, &v);
    }

    /// PROT 0915's (Mushura) arms 0 and 1 show the latched victim on every
    /// pass (`sb zero,0x21C` / `sw 0x20080200,0x4` at `0x801F6BB8` /
    /// `0x801F6CB8`): the band's freeze hid it, and the opening framings are
    /// on it. The engine's shown word is its resting `0` under a lowered
    /// flag.
    pub(in crate::world::battle::cast_band) fn apply_mushura_writes(
        &mut self,
        phase_in: u8,
        victim_slot: u8,
    ) {
        if phase_in > 1 {
            return;
        }
        let victim = self.casting.module_cam.victim_slot.unwrap_or(victim_slot);
        if let Some(a) = self.actors.get_mut(usize::from(victim)) {
            a.battle.render_flag = 0;
            a.battle.render_color = 0;
        }
    }

    /// The engine slots of the seated monsters with HP left.
    /// PROT 0928's (Palma) monster-row visibility. Arm 1 shows every living
    /// monster of seats 3..6 on each of its passes, held or not
    /// (`+0x21C = 0`, `+0x04 = 0x20080200`, `0x801F6CFC..0x801F6D48`, ahead
    /// of the side-band token gate at `0x801F6D58`), so the row the band hid
    /// stands in the opening framing; arm 4's pass hides it again
    /// (`+0x21C = 0xFF`, `+0x04 = 0`, `0x801F73BC..0x801F73FC`) as the
    /// creature is seated. The engine's shown word is its resting `0` under a
    /// lowered flag. The row's later choreography, seat by seat over seats
    /// 3..6 (a seat with HP left is "living"):
    ///
    /// - arm 0 parks each living seat's ground trio and facing
    ///   (`0x801FA0A8` / `0x801FA0C8`) and pulls its ground pair to an
    ///   eighth (`sra 3` into `+0x34` / `+0x38`, `0x801F6C44..0x801F6CF0`);
    /// - arm `0xE`'s pass re-shows the living row on the far plane
    ///   (`+0x38 = -0x4000`, facing `0`) and bumps a dead seat's zero facing
    ///   to `1`, the mark the later arms skip it by (`0x801F7F3C..0x801F7FD0`);
    /// - arm `0xF` holds the living row on that plane every pass; once its
    ///   word is under `0x1C1` it hides every seat (`0x801F81CC..0x801F81E4`,
    ///   after the pre-hit), and its pass lights each unmarked seat, the
    ///   colour word `0x3C0` under the soft-red state `7`, standing at
    ///   `+0x38 = |+0x34| / 4 - 0x400` (`0x801F82CC..0x801F8370`);
    /// - arm `0x10` keeps that stand every pass (`0x801F8418..0x801F8484`);
    /// - arm `0x11`'s pass hides the living row (`0x801F8694..0x801F86DC`);
    /// - the hit arm `0x13`'s pass puts each living seat's parked trio and
    ///   facing back, turns it from the creature (`FUN_80019B28` + `0x800`)
    ///   and shows it lit `0x3C0` (`0x801F892C..0x801F8B40`).
    ///
    /// Not carried: the render node's colour word `+0x74 = 0x00F00000`, the
    /// `+0x1F6` cue cursor arm `0xF` stores and the node `+0x26` / `+0x68`
    /// halfwords arms `0xE` / `0x10` zero and set.
    pub(in crate::world::battle::cast_band) fn apply_palma_writes(
        &mut self,
        phase_in: u8,
        passed: bool,
    ) {
        use legaia_engine_battle_vm::battle_formulas::STATE_RED_SOFT;
        use legaia_engine_battle_vm::battle_target_group::RENDER_FLAG_HIDDEN;
        /// The lit colour word arms `0xF` and `0x13` store (`li 0x3C0`).
        const LIT: u32 = 0x3C0;
        /// The far plane arms `0xE` / `0xF` stand the row on.
        const FAR_Z: i16 = -0x4000;
        let pc = usize::from(self.party.party_count);
        let seats: Vec<usize> = (pc..pc + 4)
            .filter(|&s| {
                self.actors
                    .get(s)
                    .is_some_and(|a| a.active && a.battle_monster_id.is_some())
            })
            .collect();
        let living = |w: &Self, s: usize| w.actors.get(s).is_some_and(|a| a.battle.hp != 0);
        let stand = |x: i16| ((i32::from(x).abs() >> 2) - 0x400) as i16;
        match (phase_in, passed) {
            (0, true) => {
                self.casting.module_saved_poses.clear();
                for &s in &seats {
                    if !living(self, s) {
                        continue;
                    }
                    let a = &mut self.actors[s];
                    let m = &mut a.move_state;
                    self.casting.module_saved_poses.push((
                        s,
                        [m.world_x, m.world_y, m.world_z],
                        a.battle.facing_angle,
                    ));
                    m.world_x /= 8;
                    m.world_z /= 8;
                }
            }
            // Arm 5's pass lowers the creature's tint state
            // (`sb zero,0x21C(s4)` at `0x801F75DC`): the red word arm 4 held
            // eases back to neutral as the creature rises out of the planet.
            // The rate drops to the `4` the dispatch left in `a3`
            // (`li a3,0x4` at `0x801F6A7C`, `sb a3,0x21D(s4)` at `0x801F75E8`).
            (5, true) => {
                if let Some(a) = self
                    .casting
                    .summon_actor_slot
                    .and_then(|s| self.actors.get_mut(usize::from(s)))
                {
                    a.battle.render_flag = 0;
                    a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(4);
                    // `sb zero,0x21B` / `sh zero,0x176` (`0x801F75E0..0x801F75E4`):
                    // the window arm 4's restage seeded on clip 0 is released,
                    // so the clip runs on to its natural end and commits the
                    // clip 1 this pass stages.
                    a.battle.hit_count_bound = 0;
                    if let Some(p) = a.battle_animation.as_mut() {
                        p.release_loop_window();
                    }
                }
            }
            (1, _) | (4, true) => {
                let hidden = phase_in == 4;
                for slot in self.living_monster_slots().into_iter().take(4) {
                    if let Some(a) = self.actors.get_mut(slot) {
                        a.battle.render_flag = if hidden { RENDER_FLAG_HIDDEN } else { 0 };
                        a.battle.render_color = 0;
                    }
                }
            }
            (0xE, true) => {
                for &s in &seats {
                    let live = living(self, s);
                    let a = &mut self.actors[s];
                    if live {
                        a.battle.render_color = 0;
                        a.battle.render_flag = 0;
                        a.move_state.world_z = FAR_Z;
                        a.battle.facing_angle = 0;
                    } else if a.battle.facing_angle == 0 {
                        a.battle.facing_angle = 1;
                    }
                }
            }
            (0xF, _) => {
                let swept = self.casting.module_cam.countdown.0 < 0x1C1;
                for &s in &seats {
                    let live = living(self, s);
                    let a = &mut self.actors[s];
                    if live {
                        a.move_state.world_z = FAR_Z;
                    }
                    if swept {
                        a.battle.render_color = 0;
                        a.battle.render_flag = RENDER_FLAG_HIDDEN;
                    }
                    if passed && a.battle.facing_angle == 0 {
                        a.move_state.world_z = stand(a.move_state.world_x);
                        a.battle.render_color = LIT;
                        a.battle.render_flag = STATE_RED_SOFT;
                    }
                }
            }
            (0x10, _) => {
                for &s in &seats {
                    let a = &mut self.actors[s];
                    if a.battle.facing_angle == 0 {
                        a.move_state.world_z = stand(a.move_state.world_x);
                    }
                }
            }
            (0x11, true) => {
                for &s in &seats {
                    if living(self, s) {
                        let a = &mut self.actors[s];
                        a.battle.render_color = 0;
                        a.battle.render_flag = RENDER_FLAG_HIDDEN;
                    }
                }
            }
            (0x13, true) => {
                let creature = self
                    .casting
                    .module_cam
                    .creature
                    .or(self.casting.module_cam.creature_live);
                for &s in &seats {
                    if !living(self, s) {
                        continue;
                    }
                    let saved = self
                        .casting
                        .module_saved_poses
                        .iter()
                        .find(|p| p.0 == s)
                        .copied();
                    let a = &mut self.actors[s];
                    if let Some((_, pos, facing)) = saved {
                        let m = &mut a.move_state;
                        [m.world_x, m.world_y, m.world_z] = pos;
                        a.battle.facing_angle = facing;
                    }
                    if let Some(c) = creature {
                        let m = &a.move_state;
                        let h = legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
                            c.z, c.x, m.world_z, m.world_x,
                        );
                        a.battle.facing_angle = h.wrapping_add(0x800) & 0xFFF;
                    }
                    a.battle.render_color = LIT;
                    a.battle.render_flag = 0;
                }
            }
            _ => {}
        }
    }

    /// PROT 0923's (Gilium) creature flight through its portal. Arm 3's
    /// pass seats the creature at `(0, -0x100, -0x800)`, facing `0`
    /// (`0x801F6DB8..0x801F6DD4`, after `FUN_801F19EC`); each pass of arm 4
    /// that still finds its word positive flies it `24` a frame delta along
    /// `+0x38` and `2` down `+0x36` (`0x801F6E30..0x801F6E58`), and arm 4's
    /// pass stands it at `+0x38 = 0x400` (`0x801F718C`). A retail cast in arm
    /// 4 holds the creature at `z = -1184` on its way in and at `0x400` from
    /// arm 5 on; without the flight the engine left it on arm 3's seat, behind
    /// the camera, and drew no creature from the portal on.
    ///
    /// Arm 3 also zeroes the creature's colour word, which the tint pass's
    /// decay arm eases back to neutral (the fade-in), and its render-node
    /// scale `+0x72` (`sh zero,0x72(v1)` at `0x801F6DDC`), which each
    /// draining pass of arm 4 grows by `32 * delta`
    /// (`0x801F6E00..0x801F6E1C`, ahead of the countdown test) and arm 4's
    /// exit stands at `0x1000` (`0x801F71A4`): the creature swells out of
    /// the portal from nothing, the draw `FUN_80048A08` scaling its matrix
    /// by the word.
    pub(in crate::world::battle::cast_band) fn apply_gilium_writes(
        &mut self,
        phase_in: u8,
        passed: bool,
    ) {
        let d = legaia_engine_cast::cast_module_camera::DELTA_PER_TICK as i16;
        let Some(a) = self
            .casting
            .summon_actor_slot
            .and_then(|s| self.actors.get_mut(usize::from(s)))
        else {
            return;
        };
        let m = &mut a.move_state;
        match (phase_in, passed) {
            (3, true) => {
                [m.world_x, m.world_y, m.world_z] = [0, -0x100, -0x800];
                a.battle.facing_angle = 0;
                // `sw zero,0x4(s6)` at `0x801F6DD8`: the creature's colour
                // word, which the tint pass's decay arm (`+0x21C == 0`)
                // eases back up to neutral `8` lanes a vsync - its fade-in
                // through the portal.
                a.battle.render_color = 0;
                a.battle.render_scale = Some(0);
            }
            (4, false) => {
                m.world_z = m.world_z.wrapping_add(24 * d);
                m.world_y = m.world_y.wrapping_add(2 * d);
                if let Some(s) = a.battle.render_scale.as_mut() {
                    *s = s.wrapping_add(32 * d as u16);
                }
            }
            // Arm `0xB`'s pass hides the creature for the rest of the cast
            // (`sw zero,0x4(s6)` / `sb 0xFF,0x21C(s6)` at
            // `0x801F7D38..0x801F7D40`; no later arm stores either).
            (0xB, true) => {
                a.battle.render_color = 0;
                a.battle.render_flag =
                    legaia_engine_battle_vm::battle_target_group::RENDER_FLAG_HIDDEN;
            }
            (4, true) => {
                // The pass that drains the countdown still flies before it
                // falls into the exit (`bgtz a1` at `0x801F6E6C` falls
                // through to `0x801F6E74`), which stands Z but not the
                // height: the creature lands at `+0x36 = 0`.
                m.world_y = m.world_y.wrapping_add(2 * d);
                m.world_z = 0x400;
                a.battle.render_scale = Some(0x1000);
            }
            _ => {}
        }
    }

    fn living_monster_slots(&self) -> Vec<usize> {
        let pc = usize::from(self.party.party_count);
        (pc..self.actors.len().min(pc + 5))
            .filter(|&s| {
                self.actors
                    .get(s)
                    .is_some_and(|a| a.active && a.battle_monster_id.is_some() && a.battle.hp != 0)
            })
            .collect()
    }

    /// PROT 0916's light bands for this frame: the spread its arms
    /// `0x11..=0x13` drew them at on the module's last pass, which
    /// `legaia_render_kernels::cast_aluru` builds for both hosts. `None`
    /// outside a live Aluru cast or on a pass that drew none.
    pub fn aluru_band_draw(&self) -> Option<i16> {
        let st = self.casting.summon_stager.as_ref()?;
        (self.mode == legaia_engine_mode::mode::SceneMode::Battle
            && self.cast_module_for(st.spell_id)
                == Some(legaia_engine_cast::cast_module_camera::ALURU_ENTRY))
        .then_some(self.casting.module_cam.band_spread)
        .flatten()
    }
}
