//! The creature's run in PROT 0915 (Mushura): its fall and spin down the
//! victim's lane, its tint, and arm `0xC`'s chase through the row.
//!
//! See `ghidra/scripts/funcs/overlay_summon_mushura_0915_801f69d8.txt`.

use super::*;
use legaia_engine_cast::cast_module_camera::{ModuleSeat, heading};

/// The colour word arm `0xA` parks the creature on (`0x801F7700`): a red
/// lane, the others out.
const RED_WORD: u32 = 0x3FF;
/// The turn's half circle, in the bearing's 12-bit units.
const HALF_TURN: i32 = 0x800;
/// A full circle.
const FULL_TURN: i32 = 0x1000;

/// The chase target an empty retail seat leaves (`0x801F8154` naming a
/// seat the engine seats nobody in).
const MUSHURA_EMPTY_SEAT: u8 = 0xFF;

/// The size class `FUN_8004E2F0` reads for a seat with no monster: its record
/// pointer `0x801C9348[seat - 3]` is null, so `lbu s2,0x1f(v0)` reads RAM
/// `0x0000001F`, a byte of the kernel's low area that holds `0x81` in every
/// retail RAM image of the cast. Against Mushura's `32` that is a reach of
/// `3 * (32 + 0x81) / 5 * 16 = 1536` units.
const NULL_RECORD_SIZE: u8 = 0x81;

impl World {
    /// PROT 0915's creature, on each pass of the arms that move it:
    ///
    /// | arm | every pass | its pass |
    /// |---|---|---|
    /// | 5 | - | the creature on the slanted clip row |
    /// | 6 | `+0x38 += 3 * step` | the spin word zeroed; the victim `+0x38 += radius / 6` |
    /// | 7, 8 | spin word `+= step >> 3`, facing `= word * -0x14`, `+0x38 += k * step - word / 4` (`k` `3`, `2`), bank `-sin(word * 32) / 8` | arm 8: facing `0xB00`, bank `0` |
    /// | 9 | - | the creature's facing on its target; the victim `+0x38 += 0xCA`; the creature's soft-red state `7`; the row shown |
    /// | `0xA` | - | the creature parked on word `0x3FF` |
    /// | `0xB` | - | the creature's target `+0x1DD = 9` |
    /// | `0xC` | [`Self::mushura_chase`], ahead of the director ([`Self::mushura_chase_ahead`]) | the creature's fade `+0x21C = 2` |
    /// | `0xD` | the bank eased level | - |
    ///
    /// Arm `0xC` also banks the creature (`+0x48`, `BattleActor::render_roll`) as it turns.
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
        // Arm 0's pass scatters the monster seats the fight left empty: for
        // each retail seat `3 + i`, `i` from `ctx[+1]` to `3`, the ground pair
        // `+0x34` / `+0x38` takes `rand() % 1025 - 0x200` each, `x` first
        // (`0x801F6BC8..0x801F6C58`, `jal 0x80056798` at `0x801F6BEC` /
        // `0x801F6C1C`). Six words on a lone-monster fight, drawn ahead of
        // every later deal of the cast.
        if phase == 0 && passed {
            let map = self.cast_seat_map();
            for i in monster_count..4 {
                let x = (self.next_rand() % 1025) as i16 - 0x200;
                let z = (self.next_rand() % 1025) as i16 - 0x200;
                if let Some(a) = map
                    .engine(3 + i)
                    .and_then(|e| self.actors.get_mut(usize::from(e)))
                    .filter(|a| !a.active && a.battle_monster_id.is_none())
                {
                    a.move_state.world_x = x;
                    a.move_state.world_z = z;
                }
            }
        }
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
        // Arm 8's pass writes `3` into the rate byte `+0x78` of the entry its
        // creature's cast record `0x801C9348[4]` points at from `+0x4C`
        // (`lw v0,-0x6ca8(v0)` / `lw v1,0x4c(v0)` / `sb v0,0x78(v1)`,
        // `0x801F7324..0x801F7338`): idle `0`'s, the clip the creature plays
        // there. The entry is archive RAM, so every later commit of it keeps
        // the byte: on the recording the idle advances `24` a pass from arm 9
        // on where it ran `32`, and its walk dust falls with it.
        if phase == 8
            && passed
            && let Some(a) = self.actors.get_mut(usize::from(seat))
        {
            const ARM_8_RECORD_RATE: u8 = 3;
            if let Some(entry) = a
                .battle_action_clips
                .as_mut()
                .and_then(|c| std::sync::Arc::make_mut(c).get_mut(0))
                .and_then(|c| c.as_mut())
            {
                entry.rate = ARM_8_RECORD_RATE;
            }
            if a.battle.current_anim == 0
                && let Some(p) = a.battle_animation.as_mut()
            {
                p.set_clip_rate(ARM_8_RECORD_RATE);
            }
        }
        // Arm 6's pass, after its shot: the victim steps back a sixth of its
        // body radius, `+0x38 += *(+0x22C)[+0x58] / 6` (`lw v0,0x22c(s1)`,
        // `lhu a0,0x58(v0)`, the `0x2AAAAAAB` divide and `sh v0,0x38(s1)` at
        // `0x801F6FB0..0x801F7000`): `112` on Gobu Gobu's `672`, the stand
        // the reaction arm 7 stages slides on from.
        if phase == 6 && passed {
            let push = self.battle_body_radius(victim) / 6;
            if let Some(v) = self.actors.get_mut(usize::from(victim)) {
                v.move_state.world_z = v.move_state.world_z.wrapping_add(push);
            }
        }
        let step = legaia_engine_cast::cast_module_camera::MODULE_DRAIN_PER_TICK;
        let pass_ends = self.module_tick_closes_pass();
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
                // creature `36` units short of retail's `-140`. The pass is
                // the battle frame's, whatever its step: a fixed four ticks
                // took the quarter off a step-2 or step-1 pass's word once in
                // every two or four passes.
                let back = if pass_ends { *spin / 4 } else { 0 };
                a.move_state.world_z = a.move_state.world_z.wrapping_add((k * step - back) as i16);
                a.battle.facing_angle = (*spin * -0x14) as u16 & 0xFFF;
                // ... and the fall wobbles: bank `-sin(word * 32) / 8`
                // (`0x801F70A8..0x801F70D4`, `0x801F72F0..0x801F7320`), level
                // again on arm 8's pass (`0x801F734C`).
                let sin = i32::from(legaia_engine_system::trig::sin12((*spin * 32) as u16));
                a.battle.render_roll = if phase == 8 && passed {
                    0
                } else {
                    ((-sin / 8) & 0xFFF) as u16
                };
            }
            // Arm `0xD` levels the bank the chase left, `step * scalar` a
            // pass (`0x801F7DBC..0x801F7E64`).
            (0xD, _) => {
                let bank = i32::from(a.battle.render_roll & 0xFFF);
                a.battle.render_roll = if bank == 0 {
                    0
                } else if bank < 0x800 {
                    (bank - step).max(0) as u16
                } else if 0x1000 - bank < step {
                    0
                } else {
                    ((bank + step) & 0xFFF) as u16
                };
                // Arm 0xD frames the anchor arm 0xC left, not the live stand.
                return;
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
        // The arm runs once a pass: the hit loop, the trade and the turn sit
        // in one call of the module (`0x801F7878..0x801F7D18`), so a pass the
        // port runs as several one-vsync ticks rolls the trade's `rand()` and
        // turns the creature on its opening tick only. Rolled on every tick, a
        // target on an empty seat (in reach from anywhere) was traded up to a
        // step's worth of times a pass: four draws where a `--watch` of
        // `0x801F8154` reads one store a pass.
        if !self.module_tick_opens_pass() {
            return;
        }
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
        // The creature's own size: the reach reads seat 7's record
        // `0x801C9348[4]` (`lbu s2,0x1f(v0)` at `0x8004E394`), the cast
        // record the seat arm copied in (Mushura's `32`).
        let size = self
            .tables
            .summon_creatures
            .get(&spell_id)
            .map_or(0, |d| d.size_class);
        for (e, _) in self.ra_seru_row(monster_count) {
            let s = e as u8;
            let v = self.cast_view(s);
            let in_reach = self.creature_range_metric((here.x, here.z), size, s) == 0;
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
            // The bearing reads the struck body's pair `+0x3C` / `+0x40`
            // (`lh a2,0x40(v0)` / `lh a3,0x3c(v0)` at `0x801F79F8`), the
            // pair the draw stored, not its live stand.
            let Some((bx, bz)) = self.actors.get(e).map(|m| {
                m.battle
                    .seat
                    .unwrap_or((m.move_state.world_x, m.move_state.world_z))
            }) else {
                continue;
            };
            let to_it = heading(
                here,
                ModuleSeat {
                    x: bx,
                    y: 0,
                    z: bz,
                    facing: 0,
                },
            );
            let (bearing, own) = closest_turn(i32::from(to_it), facing);
            // Dead ahead (`bne a0,s1` at `0x801F7A40` not taken) the arm
            // draws a `rand()` it never reads and turns the body `0xC00`,
            // as it does for a bearing past the creature's own
            // (`0x801F7A48..0x801F7A60`); behind it, `0x400`. A recording's
            // `--range-trace` reads Gobu Gobu's facing `0xC00` off the hit
            // that finds it straight down the creature's line.
            let off = if bearing == own {
                let _ = self.next_rand();
                0xC00
            } else if bearing > own {
                0xC00
            } else {
                0x400
            };
            if let Some(m) = self.actors.get_mut(e) {
                m.battle.facing_angle = ((own + off) & 0xFFF) as u16;
            }
        }
        // The chase target `0x801F8154`. A target in reach is traded by
        // `do t = rand() % 4 + 3 while t == old` (`0x801F7ABC..0x801F7AE8`):
        // the four monster seats whatever the row holds, standing or not, so
        // a seat with no monster in it can come up. Such a seat's body pair
        // reads `(0, 0)` and its size the null record's
        // ([`NULL_RECORD_SIZE`]), so it is in reach from anywhere near the
        // field and traded again on the next pass: the creature keeps
        // chasing the row, a pass's turn toward the origin at most.
        let _ = monster_count;
        let mut target = self.casting.module_mushura_target;
        let empty = target == MUSHURA_EMPTY_SEAT
            || self
                .actors
                .get(usize::from(target))
                .is_none_or(|a| a.battle_monster_id.is_none());
        let reached = if empty {
            self.empty_seat_in_reach((here.x, here.z), size)
        } else {
            self.creature_range_metric((here.x, here.z), size, target) == 0
        };
        if reached {
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
        // The turn and the bank are a pass's law (`(step * scalar) << 3` and
        // `step * scalar`, `0x801F7B74..0x801F7D10`) on the frame step the
        // pass runs at (`DAT_1F800393`), since a facing within the pass's
        // rate of the bearing holds rather than snaps.
        let step = self.module_frame_step();
        // The bearing reads the target's body pair `+0x3C` / `+0x40`
        // (`lh a2,0x40(v0)` / `lh a3,0x3c(v0)` at `0x801F7B18`), which an
        // empty seat holds at `(0, 0)`.
        let empty = target == MUSHURA_EMPTY_SEAT
            || self
                .actors
                .get(usize::from(target))
                .is_none_or(|a| a.battle_monster_id.is_none());
        let (tx, tz) = if empty {
            (0, 0)
        } else {
            let Some(pair) = self.live_body_pair(usize::from(target)) else {
                return;
            };
            pair
        };
        let bearing = heading(
            here,
            ModuleSeat {
                x: tx,
                y: 0,
                z: tz,
                facing: 0,
            },
        );
        let pass = i32::from(step) * legaia_engine_cast::cast_module_camera::MODULE_DRAIN_PER_TICK;
        let Some(a) = self.actors.get_mut(usize::from(seat)) else {
            return;
        };
        let bank = i32::from(a.battle.render_roll);
        if i32::from(bearing & 0xFFF) == facing {
            // On the bearing: the bank eases back to level (`0x801F7C68..`).
            a.battle.render_roll = if bank == 0 {
                0
            } else if bank < 0x800 {
                if bank < pass { 0 } else { (bank - pass) as u16 }
            } else if 0x1000 - bank < pass {
                0
            } else {
                ((bank + pass) & 0xFFF) as u16
            };
            return;
        }
        let (to, from) = closest_turn(i32::from(bearing & 0xFFF), facing);
        let rate = pass << 3;
        if to < from {
            // Turning down: the facing holds inside a pass's rate
            // (`0x801F7B8C..0x801F7BA8`) and the body banks down to `-0x100`
            // (`0xF00`, `0x801F7BC4..0x801F7BEC`).
            if from >= to + rate {
                a.battle.facing_angle = ((from - rate) & 0xFFF) as u16;
            }
            let b = (bank - pass) & 0xFFF;
            a.battle.render_roll = if (0xC01..0xF00).contains(&b) {
                0xF00
            } else {
                b as u16
            };
        } else {
            // Turning up (`0x801F7C08..0x801F7C24`), the bank up to `0x100`
            // (`0x801F7C40..0x801F7C64`): any word past it, the far side of
            // level included, stores `0x100`.
            if to >= from + rate {
                a.battle.facing_angle = ((from + rate) & 0xFFF) as u16;
            }
            let b = (bank + pass) & 0xFFF;
            a.battle.render_roll = if b < 0x101 { b as u16 } else { 0x100 };
        }
    }
}

impl World {
    /// `FUN_8004E2F0(7, seat) == 0` for a monster seat with no monster: the
    /// target's body pair `(0, 0)` and the null record's size
    /// ([`NULL_RECORD_SIZE`]) against the creature's `size`.
    fn empty_seat_in_reach(&self, pos: (i16, i16), size: u8) -> bool {
        use legaia_engine_battle_vm::battle_action::motion;
        let inputs = motion::RangeInputs {
            attacker_party: false,
            target_party: false,
            attacker_reach: 0,
            attacker_size: size,
            target_size: NULL_RECORD_SIZE,
            attacker_pos: pos,
            target_ref: (0, 0),
        };
        let bearing =
            legaia_engine_battle_vm::battle_action::bearing_12bit_approx(0, 0, pos.1, pos.0);
        let angle = legaia_engine_battle_vm::battle_approach::approach_angle(bearing) as u16;
        let (sin, cos) = legaia_engine_system::trig::trig12(angle);
        motion::range_metric(&inputs, sin, cos) == 0
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
