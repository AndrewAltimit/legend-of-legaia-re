//! PROT 0916's (Aluru) writes outside the camera: the arm-0 kill verdict,
//! the instant kill of arm `0x11`, and the colour words its arms 4..6 and
//! `0x13` drive on the creature and on a killed victim.

use super::*;

impl World {
    /// Draw PROT 0916's arm-0 verdict once per cast and hold it on the
    /// module state ([`ModuleCamState::kill`]), the way retail parks it in
    /// `0x801F9010` before any later arm reads it. The skip condition is
    /// Nighto's: the scripted-fight byte and the victim record's `+0x20`,
    /// unless the victim carries `+0x16E` bit `4`.
    ///
    /// [`ModuleCamState::kill`]: legaia_engine_battle_vm::cast_module_camera::ModuleCamState::kill
    ///
    /// REF: overlay_summon_aluru_0916_801f69f8 (`0x801F6D44..0x801F6E44`)
    pub(in crate::world::battle::cast_band) fn draw_aluru_verdict(
        &mut self,
        caster_slot: u8,
        victim_slot: u8,
        spell_id: u8,
    ) {
        use legaia_engine_battle_vm::cast_module_camera as cam;
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
                & legaia_engine_battle_vm::cast_module_ticks::FLAG_NON_TARGETABLE
                == 0;
        let roll = (!skipped).then(|| self.next_rand());
        self.casting.module_cam.kill = Some(cam::aluru_kill_verdict(roll, level));
    }

    /// The colour-word and HP stores of PROT 0916's arms on the pass the
    /// director just made for `phase_in` (`passed`: it left the arm).
    ///
    /// - Arm `0x11`, kill path, on its passing tick: the victim's HP to `0`
    ///   (`sh zero,0x14C(s3)` at `0x801F7E14`), and the band's fold owes
    ///   nothing - the damage path is the only one that reaches the hit.
    /// - Arm `0x13`, kill path: `+0x1BA += 2 * frame * scalar` on the victim
    ///   every pass (`0x801F81AC..0x801F81BC`), the break-up the bands play
    ///   over.
    /// - The creature's word, as arms 4..6 assemble it (`0x800` stored by
    ///   arm 4, drained to `0x400` and then `0`).
    pub(in crate::world::battle::cast_band) fn apply_aluru_writes(
        &mut self,
        phase_in: u8,
        passed: bool,
        victim_slot: u8,
        seat_slot: u8,
    ) {
        use legaia_engine_battle_vm::cast_module_camera as cam;
        let st = self.casting.module_cam;
        let victim = st.victim_slot.unwrap_or(victim_slot);
        if phase_in == cam::ALURU_BLAST_ARM && passed && st.kill == Some(true) {
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
    }

    /// PROT 0916's light bands for this frame: the spread its arms
    /// `0x11..=0x13` drew them at on the module's last pass, which
    /// `legaia_render_kernels::cast_aluru` builds for both hosts. `None`
    /// outside a live Aluru cast or on a pass that drew none.
    pub fn aluru_band_draw(&self) -> Option<i16> {
        let st = self.casting.summon_stager.as_ref()?;
        (self.mode == legaia_engine_mode::mode::SceneMode::Battle
            && self.cast_module_for(st.spell_id)
                == Some(legaia_engine_battle_vm::cast_module_camera::ALURU_ENTRY))
        .then_some(self.casting.module_cam.band_spread)
        .flatten()
    }
}
