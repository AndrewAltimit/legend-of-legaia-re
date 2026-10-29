//! Barra's flight kernels: the circle, arc and dive the creature flies and the
//! pitch it takes. `engine-core`'s `world::battle::cast_band::code_run::barra_writes`
//! applies them to the seated creature.

/// The neutral tint word an arm shows a seat with (`lui 0x2008; ori 0x200`).
pub const SHOWN_TINT: u32 = 0x2008_0200;

/// `+0x21C = 0xFF`: the seat is hidden.
pub const HIDDEN: u8 = 0xFF;

/// `+0x21C = 8`: the presentation tick `FUN_80050120`'s green arm, which
/// arm `0xB`'s pass puts both seats on and arm `0xC`'s takes them off.
pub const BARRA_GREEN_TINT: u8 = 8;

/// The height the victim and the creature hang at from arm `0xC`'s pass
/// (`li v0,-0x2000` / `sh v0,0x36(s3)` / `sh v0,0x36(s4)` at
/// `0x801F7B14..0x801F7B24`).
pub const BARRA_HANG_Y: i16 = -0x2000;

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
pub const BARRA_DIVE_PITCH: u16 = 0xC00;

/// Arms `0xD` / `0xE`'s creature pitch for the word `w`
/// (`sll v1,a2,0x2` / `addiu v1,v1,0xc00` / `andi 0xfff` at
/// `0x801F7D00..0x801F7D08`): `12 w + 0xC00`.
pub fn barra_flight_pitch(w: i32) -> u16 {
    ((12 * w + 0xC00) & 0xFFF) as u16
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
