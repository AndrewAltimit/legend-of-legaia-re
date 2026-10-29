//! The op-`0x20` calls a module's own parts make into its stager, for the
//! modules whose stager arms only seat more records: PROT 0905 (Vera), 0906
//! (Gizam), 0907 (Nighto), 0908 (Zenoir), 0910 (Swordie), 0911 (Orb), 0912
//! (Freed), 0913 (Nova), the summon-creature modules 0914, 0915, 0917, 0921,
//! 0928..0933, and the capture cells PROT 0938, 0944, 0946, 0961, 0962 and 0965
//! (PROT 0940's stager is a bare `jr ra`). PROT 0909's stager also steps the
//! module's phase and runs through [`World::run_viguro_stager_calls`].

use super::*;
use legaia_engine_cast::cast_module_ticks::{
    CALL_WAVE_WAVE_RECORD, FREED_STAGER_FADE, MULE_STAGER_FADE_ARM, MULE_STAGER_FADES,
    blade_breath_cell_stager_spawns, call_wave_stager_spawns, chaos_breath_cell_stager_spawns,
    dead_end_crisis_stager_spawns, doomsday_stager_spawns, freed_stager_spawns,
    gizam_stager_spawns, guilty_cross_cell_stager_spawns, horn_stager_spawns, nighto_stager_spawns,
    nova_stager_spawns, orb_stager_spawns, swordie_stager_spawns, vera_stager_spawns,
    zenoir_stager_spawns,
};
use legaia_engine_cast::cast_module_ticks::{
    MEGATON_PRESS_HELD_WHITE, MEGATON_PRESS_WHITE_OUT, megaton_press_stager_spawns,
    plasma_strike_stager_spawns,
};
use legaia_engine_cast::cast_module_ticks::{
    barra_stager_spawns, gola_gola_stager_spawns, iota_stager_spawns, jedo_stager_spawns,
    meta_stager_spawns, mule_stager_spawns, mushura_stager_spawns, palma_stager_spawns,
    terra_stager_spawns,
};
use legaia_engine_cast::cast_module_ticks::{
    kemaro_stager_spawns, ozma_stager_spawns, ultimate_rave_stager_spawns,
};

/// The slot PROT 0959 parks its held white in (`sw v0,0x102C(ctx)` at
/// `0x801F85FC`).
const MEGATON_PRESS_FADE_SLOT: legaia_overlay_images::cast_presentation::HandleSlot =
    legaia_overlay_images::cast_presentation::HandleSlot::Ctx(0x102C);

impl World {
    /// Whether module `entry`'s stager arms are run off its parts' op-`0x20`
    /// calls by [`Self::run_part_stager_calls`].
    pub(in crate::world::battle::cast_band) fn runs_part_stager_calls(entry: u32) -> bool {
        matches!(
            entry,
            905 | 906
                | 907
                | 908
                | 910
                | 911
                | 912
                | 913
                | 914
                | 915
                | 917
                | 918
                | 921
                | 924
                | 925
                | 928
                | 929
                | 930
                | 931
                | 932
                | 933
                | 934
                | 938
                | 940
                | 944
                | 946
                | 959
                | 960
                | 961
                | 962
                | 965
        )
    }

    /// Run the op-`0x20` calls module `entry`'s parts made since its last
    /// tick through the module's stager, in call order, seating what each
    /// arm seats ([`gizam_stager_spawns`], [`nighto_stager_spawns`],
    /// [`zenoir_stager_spawns`], [`freed_stager_spawns`]). PROT 0906's
    /// arms 0..2 (the flash pair, the creature shown, the phase release)
    /// stay on its director's phase-5 clock; PROT 0912's arm 6 is a fade,
    /// PROT 0929's arm 3 two.
    ///
    /// Retail runs the call inside the part's own move-VM step, so its
    /// records are seated on that frame; the engine queues the call
    /// (`MoveVmHostImpl::ext_20`) and seats them on the module's next tick,
    /// as it does PROT 0909's.
    ///
    /// REF: FUN_801F7740 (PROT 0906's stager), FUN_801F7FA8 (PROT 0907's),
    /// FUN_801F8310 (PROT 0908's), FUN_801F835C (PROT 0912's), each reached
    /// through `gp[+0x714]`
    pub(in crate::world::battle::cast_band) fn run_part_stager_calls(&mut self, entry: u32) {
        // The seats of the arms run inside their caller's step (PROT
        // 0925's), in call order.
        for s in std::mem::take(&mut self.casting.module_stager_seats) {
            self.casting.seat_stager_spawn(entry, &s);
        }
        let calls = std::mem::take(&mut self.casting.module_stager_calls);
        // A capture cell's stager runs only for the body whose arms seat
        // per arm; its other bodies keep the pager's whole-set placement,
        // which already holds the records the stager would seat.
        if matches!(entry, 938 | 940 | 944 | 959 | 960 | 962)
            && self
                .casting
                .capture_spell
                .and_then(|s| self.capture_per_arm_entry(s))
                != Some(entry)
        {
            return;
        }
        // PROT 0908's arm 1 and PROT 0906's arms 4 / 5 read the creature's
        // body point `+0x3C..+0x40`.
        let body = self
            .casting
            .module_cam
            .creature
            .map(|c| (c, self.casting.module_cam.creature_live))
            .map(|(c, live)| match live {
                Some(l) => [l.x, c.y, l.z],
                None => [c.x, c.y, c.z],
            })
            .or_else(|| {
                self.casting
                    .module_cam
                    .creature_live
                    .map(|l| [l.x, l.y, l.z])
            })
            .unwrap_or([0; 3]);
        for (arm, caller) in calls {
            self.casting.stager_arm_calls_run = self.casting.stager_arm_calls_run.wrapping_add(1);
            let stager_fades: &[_] = match (entry, arm) {
                (912, 6) => std::slice::from_ref(&FREED_STAGER_FADE),
                (929, MULE_STAGER_FADE_ARM) => &MULE_STAGER_FADES,
                _ => &[],
            };
            if !stager_fades.is_empty() {
                for t in stager_fades {
                    let mut fade = None;
                    legaia_engine_system::fade::spawn_fade(
                        &mut fade,
                        &legaia_engine_battle_vm::battle_action::summon_fade_template(t),
                        1,
                    );
                    self.presentation.module_fades.extend(fade);
                }
                continue;
            }
            // PROT 0905's arm 0 first lifts its caller, arm 5's first glow,
            // to the sparks' height (`sh v0,0x16(s3)` at `0x801F80B0`).
            if entry == 905
                && arm == 0
                && let Some(glow) = self.casting.active_summon.as_mut().and_then(|s| {
                    s.parts.iter_mut().find(|p| {
                        p.tag == Some(legaia_engine_cast::cast_module_ticks::VERA_SPARK_CALLER)
                    })
                })
            {
                glow.state.world_y = legaia_engine_cast::cast_module_ticks::VERA_SPARK_HEIGHT;
            }
            // PROT 0946's arm 1 fills a sixteen-slot handle table nothing
            // empties during the cast: the waves this cast has seated.
            let waves = self.casting.active_summon.as_ref().map_or(0, |sc| {
                sc.parts
                    .iter()
                    .filter(|p| p.tag == Some(CALL_WAVE_WAVE_RECORD))
                    .count()
            });
            let phase = self.casting.module_phase;
            let mut barra_counter = self.casting.module_barra_counter;
            let mut ozma_word = self.casting.module_ozma_word;
            let mut rand = || self.next_rand();
            let spawns = match entry {
                906 => gizam_stager_spawns(arm, caller, body, &mut rand),
                907 => nighto_stager_spawns(arm, caller, &mut rand),
                908 => zenoir_stager_spawns(arm, caller, body, &mut rand),
                905 => vera_stager_spawns(arm, caller, &mut rand),
                910 => swordie_stager_spawns(arm, caller),
                911 => orb_stager_spawns(arm, caller),
                913 => nova_stager_spawns(arm, caller),
                912 => freed_stager_spawns(arm, caller, &mut rand),
                930 => horn_stager_spawns(arm, caller, &mut rand),
                914 => gola_gola_stager_spawns(arm, caller),
                915 => mushura_stager_spawns(arm, caller),
                917 => barra_stager_spawns(arm, caller, &mut barra_counter),
                921 => iota_stager_spawns(arm, caller, &mut rand),
                928 => palma_stager_spawns(arm, caller, &mut rand),
                929 => mule_stager_spawns(arm, caller, &mut rand),
                931 => jedo_stager_spawns(arm, caller),
                932 => meta_stager_spawns(arm, caller),
                933 => terra_stager_spawns(arm, caller),
                918 => kemaro_stager_spawns(arm, caller),
                924 => ultimate_rave_stager_spawns(arm, caller),
                934 => ozma_stager_spawns(arm, caller, &mut ozma_word, &mut rand),
                938 => chaos_breath_cell_stager_spawns(arm, caller),
                // PROT 0940's stager `FUN_801F82CC` is a bare `jr ra`.
                940 => Vec::new(),
                944 => guilty_cross_cell_stager_spawns(arm, caller),
                959 => megaton_press_stager_spawns(arm, caller, &mut rand),
                960 => plasma_strike_stager_spawns(arm, caller),
                946 => call_wave_stager_spawns(arm, caller, phase, waves, &mut rand),
                961 => dead_end_crisis_stager_spawns(arm, caller, &mut rand),
                962 => blade_breath_cell_stager_spawns(arm, caller),
                965 => doomsday_stager_spawns(arm, caller),
                _ => Vec::new(),
            };
            self.casting.module_barra_counter = barra_counter;
            self.casting.module_ozma_word = ozma_word;
            // PROT 0959's arm 2 flushes the pool, kills the held white its
            // arm 3 parked at `ctx[+0x102C]` and fades white out before its
            // seats (`0x801F8478..0x801F84D4`).
            if entry == 959 && arm == 2 {
                self.casting.flush_module_part_pool();
                self.casting
                    .module_pres
                    .fades
                    .retain(|f| f.slot != Some(MEGATON_PRESS_FADE_SLOT));
                self.casting
                    .push_megaton_press_fade(&MEGATON_PRESS_WHITE_OUT, 0x801F_84D4, None);
            }
            for s in &spawns {
                self.casting.seat_stager_spawn(entry, s);
            }
            if entry == 959 && arm == 3 {
                self.casting.push_megaton_press_fade(
                    &MEGATON_PRESS_HELD_WHITE,
                    0x801F_85E8,
                    Some(MEGATON_PRESS_FADE_SLOT),
                );
            }
        }
    }

    /// Spawn one of PROT 0959's stager fades into the presentation hook's
    /// fade list, parked at `slot` (which the module's arm `0x0F` and the
    /// stager's arm 2 kill).
    pub(in crate::world::battle::cast_band) fn push_stager_fade(
        &mut self,
        t: &legaia_engine_battle_vm::battle_action::SummonFadeTemplate,
        site: u32,
        slot: Option<legaia_overlay_images::cast_presentation::HandleSlot>,
    ) {
        self.casting.push_megaton_press_fade(t, site, slot);
    }
}
