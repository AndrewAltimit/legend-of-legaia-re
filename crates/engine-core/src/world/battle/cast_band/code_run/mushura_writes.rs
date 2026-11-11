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
/// The frame step arms 7 and 8 run at. Their term `word / 4` comes off once a
/// frame where the frame product comes off per vsync, so the engine takes it
/// whole on every fourth tick: retail's stand reads `-552` entering arm 8 and
/// `-140` leaving.
const ARM_FRAME_STEP: i32 = 4;

/// The chase target an empty retail seat leaves (`0x801F8154` naming a
/// seat the engine seats nobody in).
const MUSHURA_EMPTY_SEAT: u8 = 0xFF;

impl World {
    /// PROT 0915's creature, on each pass of the arms that move it:
    ///
    /// | arm | every pass | its pass |
    /// |---|---|---|
    /// | 5 | - | the creature on the slanted clip row |
    /// | 6 | `+0x38 += 3 * step` | the spin word zeroed |
    /// | 7, 8 | spin word `+= step >> 3`, facing `= word * -0x14`, `+0x38 += k * step - word / 4` (`k` `3`, `2`) | arm 8: facing `0xB00` |
    /// | 9 | - | the creature's facing on its target; the victim `+0x38 += 0xCA`; the creature's soft-red state `7`; the row shown |
    /// | `0xA` | - | the creature parked on word `0x3FF` |
    /// | `0xB` | - | the creature's target `+0x1DD = 9` |
    /// | `0xC` | [`Self::mushura_chase`], ahead of the director ([`Self::mushura_chase_ahead`]) | the creature's fade `+0x21C = 2` |
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
    ) {
        // The module's two wrapper sites are its only hits: nothing rolls the
        // whole power afterwards.
        self.casting.module_skips_fold = true;
        // Arm 5's pass raises `+0x42 = 2` over object-effect row 1, tilted by
        // `-0x400` and cut at the victim's plane less `0x800`; arm 9's drops it
        // to the floor row: the creature comes up out of a slanted plane.
        if phase == 5 && passed {
            let z = self
                .actors
                .get(usize::from(victim))
                .map_or(0, |v| v.move_state.world_z);
            self.object_effect
                .write(1, [-0x400, 0, 0, z.wrapping_sub(0x800), 0x7FFF]);
            self.casting.module_creature_clip_kind = 2;
            self.casting.module_cam.creature_floor_clip = true;
            if let Some(a) = self.actors.get_mut(usize::from(seat)) {
                a.battle.floor_clip = true;
            }
        }
        if phase == 9 && passed {
            self.casting.module_creature_clip_kind = 1;
        }
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
                // The frame product runs a vsync a tick; the word's quarter
                // comes off once a pass, on the tick that closes it (the word
                // has grown a pass's `(step * scalar) >> 3`), whole: shared
                // out a tick at a time it truncated away and stood the
                // creature `36` units short of retail's `-140`.
                let pass_ends = *spin % ARM_FRAME_STEP == 0;
                let back = if pass_ends { *spin / 4 } else { 0 };
                a.move_state.world_z = a.move_state.world_z.wrapping_add((k * step - back) as i16);
                a.battle.facing_angle = (*spin * -0x14) as u16 & 0xFFF;
            }
            (9, true) => {
                a.battle.render_flag = 7;
                // The arm's pass turns the creature on its target
                // (`jal 0x80019B28`, `sh v0,0x46(s4)` at `0x801F7578..0x801F7594`),
                // off the spin arm 8 left, before it moves the victim: arm
                // `0xC`'s chase runs along this heading.
                let here = ModuleSeat {
                    x: a.move_state.world_x,
                    y: 0,
                    z: a.move_state.world_z,
                    facing: 0,
                };
                if let Some(v) = self.actors.get(usize::from(victim)) {
                    let there = ModuleSeat {
                        x: v.move_state.world_x,
                        y: 0,
                        z: v.move_state.world_z,
                        facing: 0,
                    };
                    let bearing = heading(here, there);
                    if let Some(a) = self.actors.get_mut(usize::from(seat)) {
                        a.battle.facing_angle = bearing & 0xFFF;
                    }
                }
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
                // `lui v0,0x8100` / `sw v0,0x8(s4)` at `0x801F7708..0x801F7710`:
                // the mode word draws the parked body additive.
                a.battle.flag_word = 0x8100_0000;
            }
            // `+0x1DD = 9`: no one seat's range stops the clip's root motion
            // any more (`0x801F77B4`), and the arm stores the running clip's
            // root speed `0x50` straight into its entry (`0x801F77A0..0x801F77AC`):
            // the disc's is `0`, so nothing else moves the creature.
            (0xB, true) => {
                // `sw zero,0x8(s4)` at `0x801F77E0`: opaque again.
                a.battle.flag_word = 0;
                a.battle.active_target = 9;
                if let Some(p) = a.battle_animation.as_mut() {
                    p.set_root_speed(0x50);
                }
            }
            // The chase itself ran ahead of the director
            // ([`Self::mushura_chase_ahead`]).
            (0xC, _) => {
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

    /// Arm `0xC`'s chase, run ahead of the module's director: the arm turns
    /// the creature before its every-pass framing reads the creature's
    /// heading (the chase at `0x801F7878..0x801F7D18` precedes the shot), so
    /// the framing follows this pass's turn rather than the last one's. The
    /// director then reads the creature's stand off `module_cam.creature`.
    pub(in crate::world::battle::cast_band) fn mushura_chase_ahead(
        &mut self,
        monster_count: u8,
        seat: u8,
        caster: u8,
        spell_id: u8,
    ) {
        self.mushura_chase(monster_count, seat, caster, spell_id);
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
            let Some(roll) = self.summon_module_roll(caster, s, spell_id) else {
                continue;
            };
            // The readout flag kept down (`0x801F78AC`).
            self.battle.combo_cluster_held = true;
            let hit = (roll >> 2).max(1);
            // The popup goes up ahead of the clamp (`0x801F7944`).
            self.battle
                .hit_fx
                .push(legaia_engine_battle::battle_events::BattleHitFx {
                    target_slot: s,
                    amount: hit.min(u32::from(u16::MAX)) as u16,
                    is_heal: false,
                    is_crit: false,
                });
            let mut v = self.cast_view(s);
            // Clamped to live HP: this hit is the one that kills
            // (`0x801F795C..0x801F7990`), and the fold owes nothing more.
            let dmg = hit.min(u32::from(v.hp));
            v.hp_bar_delta = v.hp_bar_delta.wrapping_add(dmg as i32);
            v.hp = (u32::from(v.hp) - dmg) as u16;
            if v.hp == 0 {
                v.staged_anim = v.knockdown_anim;
                v.restage = 1;
            } else {
                v.staged_anim = if v.reaction_alt != 0 {
                    v.reaction_alt
                } else {
                    v.reaction_alt2
                };
                v.restage = 5;
            }
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
        // The chase target `0x801F8154`. A target in reach is traded by
        // `do t = rand() % 4 + 3 while t == old` (`0x801F7ABC..0x801F7AE8`):
        // the four monster seats whatever the row holds, standing or not, so
        // a seat with no monster in it can come up. The engine has no actor
        // there; it holds `0xFF`, which no metric reaches and no turn reads.
        let _ = monster_count;
        let mut target = self.casting.module_mushura_target;
        if self.creature_range_metric((here.x, here.z), 0, target) == 0 {
            let map = self.cast_seat_map();
            let old = if target == MUSHURA_EMPTY_SEAT {
                self.casting.module_mushura_retail_seat
            } else {
                map.retail(target)
            };
            let seat = loop {
                let t = (self.next_rand() % 4) as u8 + 3;
                if t != old {
                    break t;
                }
            };
            target = map.engine(seat).unwrap_or(MUSHURA_EMPTY_SEAT);
            self.casting.module_mushura_target = target;
            self.casting.module_mushura_retail_seat = seat;
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
