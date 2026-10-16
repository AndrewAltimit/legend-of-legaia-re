//! The op-`0x20` calls a module's own parts make into its stager, for the
//! modules whose stager arms only seat more records: PROT 0905 (Vera), 0906
//! (Gizam), 0907 (Nighto), 0908 (Zenoir), 0910 (Swordie), 0911 (Orb), 0912
//! (Freed) and 0913 (Nova). PROT 0909's stager also steps the module's
//! phase and runs through [`World::run_viguro_stager_calls`].

use super::*;
use legaia_engine_battle_vm::cast_module_ticks::{
    FREED_STAGER_FADE, StagerSpawn, freed_stager_spawns, gizam_stager_spawns, nighto_stager_spawns,
    nova_stager_spawns, orb_stager_spawns, swordie_stager_spawns, vera_stager_spawns,
    zenoir_stager_spawns,
};

impl World {
    /// Whether module `entry`'s stager arms are run off its parts' op-`0x20`
    /// calls by [`Self::run_part_stager_calls`].
    pub(in crate::world::battle::cast_band) fn runs_part_stager_calls(entry: u32) -> bool {
        matches!(entry, 905 | 906 | 907 | 908 | 910 | 911 | 912 | 913)
    }

    /// Run the op-`0x20` calls module `entry`'s parts made since its last
    /// tick through the module's stager, in call order, seating what each
    /// arm seats ([`gizam_stager_spawns`], [`nighto_stager_spawns`],
    /// [`zenoir_stager_spawns`], [`freed_stager_spawns`]). PROT 0906's
    /// arms 0..2 (the flash pair, the creature shown, the phase release)
    /// stay on its director's phase-5 clock; PROT 0912's arm 6 is a fade.
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
        let calls = std::mem::take(&mut self.casting.module_stager_calls);
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
            if entry == 912 && arm == 6 {
                let mut fade = None;
                legaia_engine_system::fade::spawn_fade(
                    &mut fade,
                    &legaia_engine_battle_vm::battle_action::summon_fade_template(
                        &FREED_STAGER_FADE,
                    ),
                    1,
                );
                self.presentation.module_fades.extend(fade);
                continue;
            }
            // PROT 0905's arm 0 first lifts its caller, arm 5's first glow,
            // to the sparks' height (`sh v0,0x16(s3)` at `0x801F80B0`).
            if entry == 905
                && arm == 0
                && let Some(glow) = self.casting.active_summon.as_mut().and_then(|s| {
                    s.parts.iter_mut().find(|p| {
                        p.tag == Some(legaia_engine_battle_vm::cast_module_ticks::VERA_SPARK_CALLER)
                    })
                })
            {
                glow.state.world_y = legaia_engine_battle_vm::cast_module_ticks::VERA_SPARK_HEIGHT;
            }
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
                _ => Vec::new(),
            };
            for s in &spawns {
                self.seat_stager_spawn(entry, s);
            }
        }
    }

    /// Seat one stager arm's `FUN_80021B04` call into the cast's running
    /// scene: the record parsed against its image's whole record set, with
    /// the arm's bytecode stores applied to the copy it is seated from, and
    /// its stores into the seated part queued behind the part's seat run.
    fn seat_stager_spawn(&mut self, entry: u32, spawn: &StagerSpawn) {
        const LINK_BASE: u32 = legaia_overlay_images::summon_overlay::SUMMON_OVERLAY_LINK_BASE;
        let Some(module) = self
            .casting
            .effect_pool
            .as_ref()
            .and_then(|p| p.module(entry))
        else {
            return;
        };
        let Some(off) = spawn.record.checked_sub(LINK_BASE).map(|o| o as usize) else {
            return;
        };
        let bytes: std::sync::Arc<[u8]> = if spawn.patches.is_empty() {
            module.bytes.clone()
        } else {
            let mut b = module.bytes.to_vec();
            for &(va, value) in &spawn.patches {
                let Some(at) = va.checked_sub(LINK_BASE).map(|o| o as usize) else {
                    continue;
                };
                if let Some(w) = b.get_mut(at..at + 2) {
                    w.copy_from_slice(&value.to_le_bytes());
                }
            }
            b.into()
        };
        let mut offs: Vec<usize> = module.parts.iter().map(|p| p.record_off).collect();
        offs.push(off);
        let picked: Vec<_> = legaia_overlay_images::summon_overlay::parse_records_at(&bytes, &offs)
            .into_iter()
            .filter(|p| p.record_off == off)
            .collect();
        if picked.is_empty() {
            return;
        }
        let scene = self.casting.active_summon.get_or_insert_with(|| {
            legaia_engine_effects::summon::SummonScene::spawn_parts(
                &[],
                &bytes,
                crate::scene_host::EFFECT_MODEL_LIBRARY_BASE,
                spawn.pos,
            )
        });
        let first = scene.parts.len();
        scene.push_parts_scaled(&picked, &bytes, spawn.pos, spawn.rot, spawn.scale);
        for part in &mut scene.parts[first..] {
            part.tag.get_or_insert(spawn.record);
            part.seat_pokes.extend_from_slice(&spawn.pokes);
            part.seat_halve_speed = spawn.halve_speed;
        }
    }
}
