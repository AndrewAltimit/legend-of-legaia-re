//! The part-called stagers of PROT 0905 (Vera), 0910 (Swordie), 0911 (Orb)
//! and 0913 (Nova): what each arm seats off the part that called it
//! ([`super::stager_spawns`] has the shapes and the other modules).

use super::stager_spawns::{StagerCaller, StagerSpawn};

fn on_caller(record: u32, caller: StagerCaller) -> StagerSpawn {
    StagerSpawn {
        record,
        pos: caller.pos,
        rot: caller.rot,
        pokes: Vec::new(),
        patches: Vec::new(),
        halve_speed: false,
        scale: 0x1000,
    }
}

/// The record PROT 0905's stager arm `0` seats eight times: a spark.
pub const VERA_SPARK_RECORD: u32 = 0x801F_8568;
/// The record whose op-`0x20` call (`STAGER 0`) opens PROT 0905's burst:
/// arm 5's first glow.
pub const VERA_SPARK_CALLER: u32 = 0x801F_8494;
/// The height PROT 0905's stager arm `0` lifts its caller to before it
/// seats the sparks there (`li v0,-0x1F4` / `sh v0,0x16(s3)` at
/// `0x801F80B0`). The retail capture holds the glow at `y = -500`.
pub const VERA_SPARK_HEIGHT: i16 = -0x1F4;

/// The stores PROT 0905's stager arm `0` makes into spark `i` (of eight)
/// once `FUN_80021B04` has seated it (`0x801F80EC..0x801F81A4`), off four
/// `rand()` draws `r0..r3`: the roll rate `+0x84` (`r0 & 0xFF` less `0x80`,
/// or less `0x61` below it, `>> 5`), the outer height `+0xBA = -0x80`, the
/// inner radius `+0xB4 = r1 % 15 + 0x10`, the outer `+0xB6 = r2 % 255 +
/// 0x200`, and the roll `+0x28 = (i << 9) + (r3 & 0x7F)`.
pub fn vera_spark_pokes(i: u16, r: [u32; 4]) -> Vec<(u16, u16)> {
    let b = (r[0] & 0xFF) as i32;
    let rate = if b - 0x80 >= 0 { b - 0x80 } else { b - 0x61 } >> 5;
    vec![
        (0x84, rate as u16),
        (0xBA, (-0x80i16) as u16),
        (0xB4, (r[1] % 15 + 0x10) as u16),
        (0xB6, (r[2] % 255 + 0x200) as u16),
        (0x28, (i << 9) + (r[3] & 0x7F) as u16),
    ]
}

/// PROT 0905 (Vera) - the stager `0x801F8078`.
///
/// | arm | caller | seats |
/// |---:|---|---|
/// | 0 | arm 5's first glow `0x801F8494` | eight sparks [`VERA_SPARK_RECORD`] at the caller's plane pair and [`VERA_SPARK_HEIGHT`], each fanned by [`vera_spark_pokes`] - the blue rays arm 8's burst opens with |
/// | 1 | - | `0x801F862C` on the caller |
///
/// Arm 0 also lifts its caller to [`VERA_SPARK_HEIGHT`]; that store is the
/// host's (it holds the caller).
///
/// PORT: FUN_801F8078 (PROT 0905's stager; both arms)
pub fn vera_stager_spawns(
    arm: u8,
    caller: StagerCaller,
    rand: &mut dyn FnMut() -> u32,
) -> Vec<StagerSpawn> {
    match arm {
        0 => (0..8u16)
            .map(|i| {
                let mut s = on_caller(VERA_SPARK_RECORD, caller);
                s.pos[1] = VERA_SPARK_HEIGHT;
                let r = [rand(), rand(), rand(), rand()];
                s.pokes = vera_spark_pokes(i, r);
                s
            })
            .collect(),
        1 => vec![on_caller(0x801F_862C, caller)],
        _ => Vec::new(),
    }
}

/// PROT 0910 (Swordie) - the stager `0x801F89D4` (jump table at
/// `0x801F69D8`): one record per arm on the caller, `0x801F8A9C` /
/// `0x801F8AF0` (the ground ring), `0x801F8BEC` / `0x801F8C34` (the hand
/// rays), `0x801F8D0C`.
///
/// PORT: FUN_801F89D4 (PROT 0910's stager)
pub fn swordie_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    let record = match arm {
        0 => 0x801F_8A9C,
        1 => 0x801F_8AF0,
        2 => 0x801F_8BEC,
        3 => 0x801F_8C34,
        4 => 0x801F_8D0C,
        _ => return Vec::new(),
    };
    vec![on_caller(record, caller)]
}

/// PROT 0911's stager arm `0`'s record (`addiu a2,a2,-0x7CD4` at
/// `0x801F8018`).
pub const ORB_STAGER_ARM0_RECORD: u32 = 0x801F_832C;

/// PROT 0911 (Orb) - the stager `0x801F7FE8`: arm `0` seats
/// [`ORB_STAGER_ARM0_RECORD`] on the caller at `0x1000`, arm `1` seats
/// `0x801F8498` at the caller's own `+0x72` (`lh a3,0x72(v1)` at
/// `0x801F8028`). Record `0x801F82D4` (arm 5's) calls arm `0` eleven times
/// sixteen frames apart, record `0x801F847C` (arm 9's) arm `1` three times.
///
/// PORT: FUN_801F7FE8 (PROT 0911's stager)
pub fn orb_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    match arm {
        0 => vec![on_caller(ORB_STAGER_ARM0_RECORD, caller)],
        1 => {
            let mut s = on_caller(0x801F_8498, caller);
            s.scale = caller.scale;
            vec![s]
        }
        _ => Vec::new(),
    }
}

/// The record PROT 0913's stager arm `0` seats six times.
pub const NOVA_RAY_RECORD: u32 = 0x801F_8850;

/// The stores PROT 0913's stager arm `0` makes into ray `i` (of six) once
/// `FUN_80021B04` has seated it (`0x801F86AC..0x801F86D8`): the wait `i`,
/// heights `-0x20` / `-0x80`, radii `0x20` / `0x200`, no turn rate, and the
/// yaw `0x2AA * i` - the red fan of the vortex's first flash.
pub fn nova_ray_pokes(i: u16) -> Vec<(u16, u16)> {
    vec![
        (0x54, i),
        (0xB8, (-0x20i16) as u16),
        (0xBA, (-0x80i16) as u16),
        (0xB4, 0x20),
        (0x82, 0),
        (0xB6, 0x200),
        (0x26, 0x2AA * i),
    ]
}

/// PROT 0913 (Nova) - the stager `0x801F864C` (jump table at `0x801F69D8`).
///
/// | arm | seats on the caller |
/// |---:|---|
/// | 0 | six rays [`NOVA_RAY_RECORD`], each given [`nova_ray_pokes`] |
/// | 1 | `0x801F8914` and `0x801F8960`, the vortex rings |
/// | 2 | nothing |
/// | 3 / 4 / 5 | `0x801F8E88` / `0x801F8FCC` / `0x801F9088` at the caller's `+0x72` (`lhu a3,0x72(s0)`) |
///
/// PORT: FUN_801F864C (PROT 0913's stager; every arm)
pub fn nova_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    let scaled = |record: u32| {
        let mut s = on_caller(record, caller);
        s.scale = caller.scale;
        vec![s]
    };
    match arm {
        0 => (0..6u16)
            .map(|i| {
                let mut s = on_caller(NOVA_RAY_RECORD, caller);
                s.pokes = nova_ray_pokes(i);
                s
            })
            .collect(),
        1 => vec![
            on_caller(0x801F_8914, caller),
            on_caller(0x801F_8960, caller),
        ],
        3 => scaled(0x801F_8E88),
        4 => scaled(0x801F_8FCC),
        5 => scaled(0x801F_9088),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caller() -> StagerCaller {
        StagerCaller {
            pos: [10, -20, 30],
            rot: [0, 0x123, 0],
            scale: 0x666,
            ..Default::default()
        }
    }

    #[test]
    fn vera_sparks_fan_round_the_burst() {
        let w = vera_spark_pokes(3, [0x90, 20, 300, 0x85]);
        assert_eq!(w[0], (0x84, 0));
        assert_eq!(w[2], (0xB4, 20 % 15 + 0x10));
        assert_eq!(w[3], (0xB6, 300 % 255 + 0x200));
        assert_eq!(w[4], (0x28, (3 << 9) + 5));
        // Below `0x80` the byte is taken less `0x61`, an arithmetic shift.
        assert_eq!(
            vera_spark_pokes(0, [0x01, 0, 0, 0])[0],
            (0x84, (-3i16) as u16)
        );
        let mut n = 0;
        let s = vera_stager_spawns(0, caller(), &mut || {
            n += 1;
            n
        });
        assert_eq!(s.len(), 8);
        assert!(s.iter().all(|s| s.pos == [10, VERA_SPARK_HEIGHT, 30]));
    }

    #[test]
    fn the_seru_stagers_seat_on_their_callers() {
        assert_eq!(swordie_stager_spawns(4, caller())[0].record, 0x801F_8D0C);
        assert!(swordie_stager_spawns(5, caller()).is_empty());
        assert_eq!(orb_stager_spawns(0, caller())[0].scale, 0x1000);
        assert_eq!(orb_stager_spawns(1, caller())[0].scale, 0x666);
        let rays = nova_stager_spawns(0, caller());
        assert_eq!(rays.len(), 6);
        assert_eq!(rays[5].pokes[6], (0x26, 0xD52));
        assert_eq!(nova_stager_spawns(1, caller()).len(), 2);
        assert!(nova_stager_spawns(2, caller()).is_empty());
        assert_eq!(nova_stager_spawns(3, caller())[0].scale, 0x666);
    }
}
