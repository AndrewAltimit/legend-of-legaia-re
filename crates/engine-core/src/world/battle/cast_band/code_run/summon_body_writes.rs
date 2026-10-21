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
    /// `0x12` mode (the predamage port has one summon branch), and PROT 0929
    /// arm 16's and PROT 0934 arm `0x18`'s per-pass ladders.
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
    /// `0x801F6E98..`) and arm 4 shows it; arm 8 hides the party
    /// (`0x801F7428..`); arm `0xA`, the heal, shows the party again and hides
    /// the creature (`0x801F771C..`); arm `0xB` keeps the creature hidden. The
    /// engine's shown word is its resting `0` under a lowered flag. Arm 1's
    /// hide of the monster row is not carried: nothing in the module shows it
    /// again, and the band's own reset is not modelled.
    pub(in crate::world::battle::cast_band) fn apply_horn_writes(
        &mut self,
        phase_in: u8,
        passed: bool,
        seat_slot: u8,
    ) {
        use legaia_engine_battle_vm::battle_target_group::RENDER_FLAG_HIDDEN;
        if !passed {
            return;
        }
        let set = |a: &mut crate::world::Actor, hidden: bool| {
            a.battle.render_flag = if hidden { RENDER_FLAG_HIDDEN } else { 0 };
            a.battle.render_color = 0;
        };
        let creature = match phase_in {
            3 | 0xA | 0xB => Some(true),
            4 => Some(false),
            _ => None,
        };
        if let Some(hidden) = creature
            && let Some(a) = self.actors.get_mut(usize::from(seat_slot))
        {
            set(a, hidden);
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
    /// again for the attack (`0x801F7590..`), arm 17's hides it
    /// (`0x801F8454..`) and the hit arm 18 shows each struck monster
    /// (`0x801F87C4..`). The engine's shown word is its resting `0` under a
    /// lowered flag. Each pass also halts the parts the arm kills by their
    /// parked handles and runs out the loops arm 16 ends
    /// ([`legaia_engine_cast::cast_module_camera::mule_arm_kills`],
    /// [`legaia_engine_cast::cast_module_camera::mule_arm_loop_ends`]).
    pub(in crate::world::battle::cast_band) fn apply_mule_writes(
        &mut self,
        phase_in: u8,
        passed: bool,
    ) {
        use legaia_engine_battle_vm::battle_target_group::RENDER_FLAG_HIDDEN;
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
        let hidden = match (phase_in, passed) {
            (1, _) | (11 | 18, true) => false,
            (5 | 17, true) => true,
            _ => return,
        };
        let pc = usize::from(self.party.party_count);
        for a in self.actors.iter_mut().skip(pc) {
            if a.battle_monster_id.is_none() || a.battle.hp == 0 {
                continue;
            }
            a.battle.render_flag = if hidden { RENDER_FLAG_HIDDEN } else { 0 };
            a.battle.render_color = 0;
        }
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
