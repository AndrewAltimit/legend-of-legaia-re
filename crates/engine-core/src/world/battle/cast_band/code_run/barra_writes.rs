//! The seat writes PROT 0917 (Barra)'s arms make that its director
//! (`cast_module_camera::barra_direct`) does not carry: the victim taken off
//! its seat onto the stage origin, carried up into the creature's arc,
//! walked round beside it, hung, dropped and put back, and the creature's
//! own flight. `s3` is the victim `actor_table[caster[+0x1DD]]`, `s4` the
//! creature `actor_table[7]` (`0x801F6A68..0x801F6A98`).
//!
//! The arms share the module word `0x801F934C`, the director's countdown
//! ([`legaia_engine_cast::cast_module_camera::ModuleCamState::countdown`]);
//! `w` below is its value after the pass's step and before an arm's re-arm.

use super::*;

/// The neutral tint word an arm shows a seat with (`lui 0x2008; ori 0x200`).
const SHOWN_TINT: u32 = 0x2008_0200;
/// `+0x21C = 0xFF`: the seat is hidden.
const HIDDEN: u8 = 0xFF;

/// `+0x21C = 8`: the presentation tick `FUN_80050120`'s green arm, which
/// arm `0xB`'s pass puts both seats on and arm `0xC`'s takes them off.
const BARRA_GREEN_TINT: u8 = 8;

/// The height the victim and the creature hang at from arm `0xC`'s pass
/// (`li v0,-0x2000` / `sh v0,0x36(s3)` / `sh v0,0x36(s4)` at
/// `0x801F7B14..0x801F7B24`).
const BARRA_HANG_Y: i16 = -0x2000;

/// Arm 6's arc (`0x801F7100..0x801F71E4`, `0x801F7248..0x801F72B8`): for
/// the word `w`, the sine table at `trunc((w << 11) / 160)` less `0x1000`
/// and `trunc((w << 14) / 160) - 0x2000` - the height and depth a seat
/// climbs through as `w` runs to `0xA0` (the `0x66666667` multiply is the
/// divide by 160).
pub fn barra_arc(w: i32) -> (i16, i16) {
    let a = (w << 11) / 160;
    let (sin, _) = legaia_engine_system::trig::trig12((a & 0xFFF) as u16);
    let y = i32::from(sin) - 0x1000;
    let z = (w << 14) / 160 - 0x2000;
    (y as i16, z as i16)
}

/// Arm 9's circle (`0x801F76CC..0x801F7770`) for the word `w`: the creature
/// at `z = 2 sin(8 w)`, `y = cos(8 w) - 0x1000`, and the victim hung `drop`
/// off it along the same angle, `trunc(sin * drop / 4096)` /
/// `trunc(cos * drop / 4096)` back. Returns `(creature (y, z), victim (y, z))`.
pub fn barra_circle(w: i32, drop: i16) -> ((i16, i16), (i16, i16)) {
    let (sin, cos) = legaia_engine_system::trig::trig12(((w << 3) & 0xFFF) as u16);
    let (sin, cos) = (i32::from(sin), i32::from(cos));
    let cz = 2 * sin;
    let cy = cos - 0x1000;
    let d = i32::from(drop);
    let vz = cz - sin * d / 4096;
    let vy = cy - cos * d / 4096;
    ((cy as i16, cz as i16), (vy as i16, vz as i16))
}

/// Arms `0xD` / `0xE`'s creature flight (`0x801F7CB0..0x801F7D30`,
/// `0x801F7F50..0x801F7FD4`) for the word `w`: with `t = 12 w`,
/// `y = trunc(15 sin t / 8) - 0x2000` and `z = 0x1000 - 2 drop - cos t`.
/// Returns `(y, z)`.
pub fn barra_flight(w: i32, drop: i16) -> (i16, i16) {
    let t = (3 * w) << 2;
    let (sin, cos) = legaia_engine_system::trig::trig12((t & 0xFFF) as u16);
    let y = 15 * i32::from(sin) / 8 - 0x2000;
    let z = 0x1000 - 2 * i32::from(drop) - i32::from(cos);
    (y as i16, z as i16)
}

/// The pitch arm 9's pass leaves both seats at (`li v0,0xc00` /
/// `sh v0,0x44(s4)` / `sh v0,0x44(s3)` at `0x801F77A8..0x801F77B4`): turned
/// a quarter, nose-down, into the dive.
const BARRA_DIVE_PITCH: u16 = 0xC00;

/// Arms `0xD` / `0xE`'s creature pitch for the word `w`
/// (`sll v1,a2,0x2` / `addiu v1,v1,0xc00` / `andi 0xfff` at
/// `0x801F7D00..0x801F7D08`): `12 w + 0xC00`.
pub fn barra_flight_pitch(w: i32) -> u16 {
    ((12 * w + 0xC00) & 0xFFF) as u16
}

impl World {
    /// Apply PROT 0917's arm `phase` writes for one pass of its director;
    /// `passed` is whether the arm's gate let it through, `before` the word
    /// the pass opened on.
    ///
    /// | arm | writes |
    /// |---|---|
    /// | 4 (pass, `0x801F6ED0..0x801F6F68`) | the victim's `+0x34..+0x3B` and facing saved (`0x801F9354`, `0x801F935C`), then `x = 0`, `z = 0`, facing `0`, shown; the creature hidden |
    /// | 5 (pass, `0x801F6FE8..0x801F7038`) | the drop `0x801F935E = trunc(victim[+0x3E] * 4 / 3)`; the creature shown at `y = drop - 0x1000`, `z = -0x2000` |
    /// | 6 (every pass) | under `w <= 0xA0` the creature on [`barra_arc`] (`y` raised by `drop`), from `w = 0x51` the victim on it too; on the pass the victim at `y = -drop`, `z = 0`, the creature at `y = 0`, `z = 0` |
    /// | 9 (every pass) | under `w <= 0x80` the pair on [`barra_circle`]; on the pass both at `y = -0x800`, the creature at `z = -2 drop`, the victim at `z = -drop` |
    /// | `0xB` (every pass, `0x801F784C..0x801F7880`) | both drop `8 p` a pass; on the pass both at `y = -0x800`, tinted green (`+0x21C = 8`) |
    /// | `0xC` (pass) | both at `y = -0x2000`, both on the tint SM's neutral arm (`+0x21C = 0`), the creature's word shown |
    /// | `0xD` (every pass) | the victim falls `16 p` a pass (the director's ramp), the creature on [`barra_flight`]; on the landing the victim at `y = 0` |
    /// | `0xE` (every pass) | the creature on [`barra_flight`] |
    /// | `0xF` (every pass, `0x801F8018..0x801F8060`; pass `0x801F81B4..0x801F8210`) | the creature climbs `8 p` and draws back `16 p` a pass; on the pass the victim's `x`, `z` and facing put back, the creature at `y = 0` |
    ///
    /// The pitch `+0x44` (`BattleActor::render_pitch`): arm 9 turns both
    /// through `w << 3` and leaves them at [`BARRA_DIVE_PITCH`], arms `0xD`
    /// / `0xE` turn the creature by [`barra_flight_pitch`], and the landing
    /// and arm `0xF`'s pass put the victim and the creature upright.
    ///
    /// PORT: overlay_summon_barra_0917_801f6a30 (PROT 0917; the victim and creature seat writes of arms 4, 5, 6, 9, 0xB, 0xC, 0xD, 0xE and 0xF)
    pub(in crate::world::battle::cast_band) fn apply_barra_writes(
        &mut self,
        phase: u8,
        passed: bool,
        before: i32,
        victim_slot: u8,
    ) {
        let p = legaia_engine_cast::cast_module_camera::SPEED_SCALAR;
        let creature = self.casting.summon_actor_slot.map(usize::from);
        let v = usize::from(victim_slot);
        let drop = self.casting.module_barra_drop;
        // The word after the pass's step, ahead of any re-arm the pass made.
        let stepped = before + p / 8;
        let w = self.casting.module_cam.countdown.0;
        let set_y_z = |a: &mut crate::world::Actor, y: i16, z: Option<i16>| {
            a.move_state.world_y = y;
            if let Some(z) = z {
                a.move_state.world_z = z;
            }
        };
        match phase {
            4 if passed => {
                self.casting.module_saved_poses.clear();
                if let Some(a) = self.actors.get_mut(v) {
                    self.casting.module_saved_poses.push((
                        v,
                        [
                            a.move_state.world_x,
                            a.move_state.world_y,
                            a.move_state.world_z,
                        ],
                        a.battle.facing_angle,
                    ));
                    a.move_state.world_x = 0;
                    a.move_state.world_z = 0;
                    a.battle.facing_angle = 0;
                    a.battle.render_flag = 0;
                    a.battle.render_color = SHOWN_TINT;
                }
                if let Some(a) = creature.and_then(|c| self.actors.get_mut(c)) {
                    a.battle.render_color = 0;
                    a.battle.render_flag = HIDDEN;
                }
            }
            5 if passed => {
                // `lh v1,0x3e(s3)`, `* 4`, the signed divide by three.
                let h = self.battle_display_trio(v).map_or(0, |d| d[1] as i32);
                let drop = ((h * 4) / 3) as i16;
                self.casting.module_barra_drop = drop;
                if let Some(a) = creature.and_then(|c| self.actors.get_mut(c)) {
                    a.battle.render_color = SHOWN_TINT;
                    a.battle.render_flag = 0;
                    set_y_z(a, drop.wrapping_sub(0x1000), Some(-0x2000));
                }
            }
            6 if passed => {
                if let Some(a) = self.actors.get_mut(v) {
                    set_y_z(a, drop.wrapping_neg(), Some(0));
                }
                if let Some(a) = creature.and_then(|c| self.actors.get_mut(c)) {
                    set_y_z(a, 0, Some(0));
                }
            }
            6 => {
                if w < 0xA1 {
                    let (y, z) = barra_arc(w);
                    if let Some(a) = creature.and_then(|c| self.actors.get_mut(c)) {
                        set_y_z(a, drop.wrapping_add(y), Some(z));
                    }
                    if w >= 0x51
                        && let Some(a) = self.actors.get_mut(v)
                    {
                        set_y_z(a, y, Some(z));
                    }
                }
            }
            9 if passed => {
                if let Some(a) = creature.and_then(|c| self.actors.get_mut(c)) {
                    set_y_z(a, -0x800, Some(drop.wrapping_mul(2).wrapping_neg()));
                    a.battle.render_pitch = BARRA_DIVE_PITCH;
                }
                if let Some(a) = self.actors.get_mut(v) {
                    set_y_z(a, -0x800, Some(drop.wrapping_neg()));
                    a.battle.render_pitch = BARRA_DIVE_PITCH;
                }
            }
            9 => {
                let ((cy, cz), (vy, vz)) = barra_circle(w, drop);
                let pitch = ((w << 3) & 0xFFF) as u16;
                if let Some(a) = creature.and_then(|c| self.actors.get_mut(c)) {
                    set_y_z(a, cy, Some(cz));
                    a.battle.render_pitch = pitch;
                }
                if let Some(a) = self.actors.get_mut(v) {
                    set_y_z(a, vy, Some(vz));
                    a.battle.render_pitch = pitch;
                }
            }
            0xB => {
                for slot in [Some(v), creature].into_iter().flatten() {
                    if let Some(a) = self.actors.get_mut(slot) {
                        let y = if passed {
                            // `sb s7,0x21c` with `s7 = 8` (`0x801F7950` /
                            // `0x801F7954`): the tint SM's green arm.
                            a.battle.render_flag = BARRA_GREEN_TINT;
                            -0x800
                        } else {
                            a.move_state.world_y.wrapping_add((8 * p) as i16)
                        };
                        set_y_z(a, y, None);
                    }
                }
            }
            0xC if passed => {
                for slot in [Some(v), creature].into_iter().flatten() {
                    if let Some(a) = self.actors.get_mut(slot) {
                        set_y_z(a, BARRA_HANG_Y, None);
                        a.battle.render_flag = 0;
                    }
                }
                if let Some(a) = creature.and_then(|c| self.actors.get_mut(c)) {
                    a.battle.render_color = SHOWN_TINT;
                }
            }
            0xD | 0xE => {
                // Arm `0xE`'s pass re-arms the word: its flight still runs
                // on the stepped value (`0x801F7F50` ahead of the `slti`).
                let w = if phase == 0xE && passed { stepped } else { w };
                let (y, z) = barra_flight(w, drop);
                if let Some(a) = creature.and_then(|c| self.actors.get_mut(c)) {
                    set_y_z(a, y, Some(z));
                    a.battle.render_pitch = barra_flight_pitch(w);
                }
                if phase == 0xD
                    && let Some(a) = self.actors.get_mut(v)
                {
                    let y = if passed {
                        // `sh zero,0x36(s3)` / `sh zero,0x44(s3)`
                        // (`0x801F7DE4..0x801F7DEC`): landed, upright.
                        a.battle.render_pitch = 0;
                        0
                    } else {
                        self.casting.module_cam.ramp.clamp(-0x8000, 0) as i16
                    };
                    set_y_z(a, y, None);
                }
            }
            0xF => {
                if let Some(a) = creature.and_then(|c| self.actors.get_mut(c)) {
                    a.move_state.world_z = a.move_state.world_z.wrapping_add((16 * p) as i16);
                    if passed {
                        a.battle.render_pitch = 0;
                    }
                    a.move_state.world_y = if passed {
                        0
                    } else {
                        a.move_state.world_y.wrapping_sub((8 * p) as i16)
                    };
                }
                if passed
                    && let Some(&(slot, pos, facing)) = self.casting.module_saved_poses.first()
                    && slot == v
                    && let Some(a) = self.actors.get_mut(slot)
                {
                    a.move_state.world_x = pos[0];
                    a.move_state.world_z = pos[2];
                    a.battle.facing_angle = facing;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arc_climbs_from_the_seat_line_to_the_far_side() {
        // `w = 0x50`: a quarter turn in, sine one - on the seat line.
        assert_eq!(barra_arc(0x50), (0, ((0x50 << 14) / 160 - 0x2000) as i16));
        // `w = 0xA0`: half a turn, sine zero - `0x1000` up, `0x2000` out.
        assert_eq!(barra_arc(0xA0), (-0x1000, 0x2000));
    }

    #[test]
    fn the_victim_hangs_drop_below_the_creature_on_the_circle() {
        // `w = 0`: angle zero - the creature on the origin, the victim
        // `drop` under it.
        assert_eq!(barra_circle(0, -0x200), ((0, 0), (0x200, 0)));
    }

    #[test]
    fn the_flight_starts_at_the_hang() {
        assert_eq!(barra_flight(0, 0), (-0x2000, 0));
    }
}
