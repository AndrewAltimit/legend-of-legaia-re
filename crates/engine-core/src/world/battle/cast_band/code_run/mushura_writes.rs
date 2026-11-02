//! The creature's run in PROT 0915 (Mushura): its fall and spin down the
//! victim's lane, its tint, and arm `0xC`'s chase through the row.
//!
//! See `ghidra/scripts/funcs/overlay_summon_mushura_0915_801f69d8.txt`.

use super::*;
use legaia_engine_cast::cast_module_camera::{ModuleSeat, heading};

/// The colour word arm `0xA` parks the creature on (`0x801F7700`): a red
/// lane, the others out.
const RED_WORD: u32 = 0x3FF;
/// Arm `0xC`'s turn rate in the frame delta: eight units a tick.
const TURN_RATE: i32 = 8;
/// The turn's half circle, in the bearing's 12-bit units.
const HALF_TURN: i32 = 0x800;
/// A full circle.
const FULL_TURN: i32 = 0x1000;

impl World {
    /// PROT 0915's creature, on each pass of the arms that move it:
    ///
    /// | arm | every pass | its pass |
    /// |---|---|---|
    /// | 6 | `+0x38 += 3 * step` | the spin word zeroed |
    /// | 7, 8 | spin word `+= step >> 3`, facing `= word * -0x14`, `+0x38 += k * step - word / 4` (`k` `3`, `2`) | arm 8: facing `0xB00` |
    /// | 9 | - | the victim `+0x38 += 0xCA`; the creature's soft-red state `7`; the row shown |
    /// | `0xA` | - | the creature parked on word `0x3FF` |
    /// | `0xB` | - | the creature's target `+0x1DD = 9` |
    /// | `0xC` | [`Self::mushura_chase`] | the creature's fade `+0x21C = 2` |
    ///
    /// Not carried: `+0x48`, the creature's bank, which no engine draw reads.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::world::battle::cast_band) fn apply_mushura_run(
        &mut self,
        phase: u8,
        passed: bool,
        monster_count: u8,
        victim: u8,
        seat: u8,
        caster: u8,
        spell_id: u8,
    ) {
        let step = legaia_engine_cast::cast_module_camera::MODULE_DRAIN_PER_TICK;
        let Some(a) = self.actors.get_mut(usize::from(seat)) else {
            return;
        };
        let spin = &mut self.casting.module_mushura_spin;
        match (phase, passed) {
            (6, _) => {
                a.move_state.world_z = a.move_state.world_z.wrapping_add((3 * step) as i16);
                if passed {
                    *spin = 0;
                }
            }
            (7 | 8, _) => {
                *spin += step >> 3;
                let k = if phase == 7 { 3 } else { 2 };
                a.move_state.world_z = a
                    .move_state
                    .world_z
                    .wrapping_add((k * step - *spin / 4) as i16);
                a.battle.facing_angle = (*spin * -0x14) as u16 & 0xFFF;
            }
            (9, true) => {
                a.battle.render_flag = 7;
                if let Some(v) = self.actors.get_mut(usize::from(victim)) {
                    v.move_state.world_z = v.move_state.world_z.wrapping_add(0xCA);
                }
                for (e, _) in self.ra_seru_row(monster_count) {
                    if let Some(m) = self.actors.get_mut(e) {
                        m.battle.render_flag = 0;
                        m.battle.render_color = 0;
                    }
                }
                self.casting.module_mushura_target = victim;
            }
            (0xA, true) => {
                a.battle.render_color = RED_WORD;
                a.battle.render_flag = 0xFF;
            }
            // `+0x1DD = 9`: no one seat's range stops the clip's root motion
            // any more, so the creature runs (`0x801F77B4`).
            (0xB, true) => a.battle.active_target = 9,
            (0xC, _) => {
                self.mushura_chase(monster_count, seat, caster, spell_id);
                if passed && let Some(a) = self.actors.get_mut(usize::from(seat)) {
                    a.battle.render_flag = 2;
                    a.battle.capture_state = 2;
                }
            }
            _ => return,
        }
        // The arms' framings read the creature's live stand.
        if let Some(a) = self.actors.get(usize::from(seat)) {
            self.casting.module_cam.creature = Some(ModuleSeat {
                x: a.move_state.world_x,
                y: a.move_state.world_y,
                z: a.move_state.world_z,
                facing: a.battle.facing_angle,
            });
        }
    }

    /// One pass of arm `0xC` (`0x801F7878..0x801F7D18`). Every living,
    /// targetable monster in reach (`FUN_8004E2F0(7, seat) == 0`) with no clip
    /// playing takes the summon roll quartered, floored at `1`, its popup
    /// pushed; its reaction staged with the restage bits `5` and its facing
    /// set a quarter turn off the creature's, on the side the creature's
    /// bearing to it falls. The creature then holds its chase: a target in
    /// reach is traded for a random other seat, and its facing closes on the
    /// target's bearing at eight units a tick.
    ///
    /// The engine clamps these hits to `HP - 1`: the band's fold on the arm's
    /// pass lands the last of the roll, where retail's own write can kill.
    fn mushura_chase(&mut self, monster_count: u8, seat: u8, caster: u8, spell_id: u8) {
        use legaia_engine_cast::cast_module_ticks::FLAG_NON_TARGETABLE;
        let Some(c) = self.actors.get(usize::from(seat)) else {
            return;
        };
        let here = ModuleSeat {
            x: c.move_state.world_x,
            y: 0,
            z: c.move_state.world_z,
            facing: c.battle.facing_angle,
        };
        let facing = i32::from(here.facing);
        for (e, _) in self.ra_seru_row(monster_count) {
            let s = e as u8;
            let v = self.cast_view(s);
            let in_reach = self.creature_range_metric((here.x, here.z), 0, s) == 0;
            if !in_reach || v.flags & FLAG_NON_TARGETABLE != 0 || self.battle_current_anim(e) != 0 {
                continue;
            }
            let Some(roll) = self.player_summon_predamage(caster, s, spell_id) else {
                continue;
            };
            // The readout flag kept down (`0x801F78AC`).
            self.battle.combo_cluster_held = true;
            let hit = u32::from(roll >> 2).max(1);
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
            v.staged_anim = if v.reaction_alt != 0 {
                v.reaction_alt
            } else {
                v.reaction_alt2
            };
            v.restage |= 0x05;
            self.write_cast_view(s, &v);
            let Some(m) = self.actors.get_mut(e) else {
                continue;
            };
            let to_it = heading(
                here,
                ModuleSeat {
                    x: m.move_state.world_x,
                    y: 0,
                    z: m.move_state.world_z,
                    facing: 0,
                },
            );
            let (bearing, own) = closest_turn(i32::from(to_it), facing);
            let off = if bearing > own { 0xC00 } else { 0x400 };
            m.battle.facing_angle = ((own + off) & 0xFFF) as u16;
        }
        // The chase target `0x801F8154`.
        let row = self.ra_seru_row(monster_count);
        let mut target = self.casting.module_mushura_target;
        if row.len() > 1 && self.creature_range_metric((here.x, here.z), 0, target) == 0 {
            let others: Vec<u8> = row
                .iter()
                .map(|&(e, _)| e as u8)
                .filter(|&e| e != target)
                .collect();
            target = others[self.next_rand() as usize % others.len()];
            self.casting.module_mushura_target = target;
        }
        let Some(t) = self.actors.get(usize::from(target)) else {
            return;
        };
        let bearing = heading(
            here,
            ModuleSeat {
                x: t.move_state.world_x,
                y: 0,
                z: t.move_state.world_z,
                facing: 0,
            },
        );
        let (to, from) = closest_turn(i32::from(bearing), facing);
        let rate = TURN_RATE * legaia_engine_cast::cast_module_camera::MODULE_DRAIN_PER_TICK;
        let next = if (to - from).abs() < rate {
            to
        } else if to < from {
            from - rate
        } else {
            from + rate
        };
        if let Some(a) = self.actors.get_mut(usize::from(seat)) {
            a.battle.facing_angle = (next & 0xFFF) as u16;
        }
    }
}

/// Two 12-bit angles lifted so their difference is the short way round
/// (`0x801F7C4C..0x801F7C80`).
fn closest_turn(to: i32, from: i32) -> (i32, i32) {
    let mut a = to + FULL_TURN;
    let mut b = from + FULL_TURN;
    if a - b >= HALF_TURN {
        a = to;
    }
    if b - a >= HALF_TURN {
        b = from;
    }
    (a, b)
}
