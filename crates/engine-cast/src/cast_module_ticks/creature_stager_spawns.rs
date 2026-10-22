//! The stagers of the summon-creature modules (PROT 0914, 0915, 0917, 0921,
//! 0928, 0929, 0931..0933): the routines their parts reach through op
//! `0x20` (`(*gp[0x714])(part, arm, arg)`). Each arm seats records off the
//! calling part, the way [`super::horn_stager_spawns`] does for PROT 0930.
//! Pure: each arm returns what it seats ([`StagerSpawn`]) and the host seats
//! it.
//!
//! See `ghidra/scripts/funcs/overlay_summon_<label>_<entry>_<va>.txt` (the
//! second dump of each module is its stager).

use super::{StagerCaller, StagerSpawn};
use legaia_engine_system::trig::trig12;

fn spawn(record: u32, pos: [i16; 3], rot: [i16; 3], scale: u16) -> StagerSpawn {
    StagerSpawn {
        record,
        pos,
        rot,
        pokes: Vec::new(),
        patches: Vec::new(),
        halve_speed: false,
        scale,
    }
}

/// A seat on the calling part's point and angles at `scale`.
fn on(record: u32, caller: StagerCaller, scale: u16) -> StagerSpawn {
    spawn(record, caller.pos, caller.rot, scale)
}

/// `rand() % n` as the arms' `mult` / shift sequences compute it: `rand()`
/// (`FUN_80056798`) never returns a negative, so it is the plain remainder.
fn rem(rand: &mut dyn FnMut() -> u32, n: u32) -> i32 {
    (rand() % n) as i32
}

/// PROT 0914 (Gola Gola) - the stager `0x801F7A80`, six arms (jump table
/// `0x801F69D8`), every seat by `FUN_80021B04`:
///
/// | arm | seats |
/// |---:|---|
/// | 0 / 1 / 2 / 4 / 5 | `0x801F7C30` / `0x801F7C4C` / `0x801F7CFC` / `0x801F7E6C` / `0x801F7EC8` on the caller at its scale `+0x72` |
/// | 3 | thirty-two `0x801F7D48` on the caller's point at its scale, turned `0x80 (k + 1)` past its `+0x26` about Y, no X / Z turn (`0x801F7B08..0x801F7B50`) |
///
/// PORT: FUN_801F7A80 (PROT 0914 stager; every arm)
pub fn gola_gola_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    let s = caller.scale;
    match arm {
        0 => vec![on(0x801F_7C30, caller, s)],
        1 => vec![on(0x801F_7C4C, caller, s)],
        2 => vec![on(0x801F_7CFC, caller, s)],
        3 => (1..=32i32)
            .map(|k| {
                let yaw = ((i32::from(caller.rot[1]) + 0x80 * k) & 0xFFF) as i16;
                spawn(0x801F_7D48, caller.pos, [0, yaw, 0], s)
            })
            .collect(),
        4 => vec![on(0x801F_7E6C, caller, s)],
        5 => vec![on(0x801F_7EC8, caller, s)],
        _ => Vec::new(),
    }
}

/// PROT 0915 (Mushura) - the stager `0x801F7F34`: arm 0 seats `0x801F8004`,
/// arm 1 `0x801F80A4`, each on the caller at its scale `+0x72`.
///
/// PORT: FUN_801F7F34 (PROT 0915 stager; both arms)
pub fn mushura_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    match arm {
        0 => vec![on(0x801F_8004, caller, caller.scale)],
        1 => vec![on(0x801F_80A4, caller, caller.scale)],
        _ => Vec::new(),
    }
}

/// PROT 0917's stager counter word `0x801F9364`, which arm 1 steps.
pub const BARRA_STAGER_COUNTER: u32 = 0x801F_9364;

/// PROT 0917 (Barra) - the stager `0x801F82D8`, five arms (jump table
/// `0x801F69D8`), every seat by `FUN_80021B04` on the caller at `0x1000`:
///
/// | arm | seats |
/// |---:|---|
/// | 0 | by the call's argument `0..=7` (table `0x801F69F0`): `0x801F88F8 + 0x44 * arg` |
/// | 1 | by the module word `0x801F9364` (`0..=3`): `0x801F8B40` / `0x801F8B78` / `0x801F8BB0` / `0x801F8BE8`, then the word steps `(w + 1) & 3` (`0x801F83DC..0x801F848C`) |
/// | 2 | by the argument `0..=7` (table `0x801F6A10`): `0x801F8D50 + 0x40 * arg` |
/// | 3 | `0x801F90A8` and `0x801F91FC` |
/// | 4 | `0x801F91A4` |
///
/// PORT: FUN_801F82D8 (PROT 0917 stager; every arm)
pub fn barra_stager_spawns(arm: u8, caller: StagerCaller, counter: &mut u16) -> Vec<StagerSpawn> {
    let arg = caller.arg;
    match arm {
        0 if (0..8).contains(&arg) => vec![on(0x801F_88F8 + 0x44 * arg as u32, caller, 0x1000)],
        1 => {
            let rec = match *counter {
                0 => Some(0x801F_8B40),
                1 => Some(0x801F_8B78),
                2 => Some(0x801F_8BB0),
                3 => Some(0x801F_8BE8),
                _ => None,
            };
            *counter = counter.wrapping_add(1) & 3;
            rec.map(|r| vec![on(r, caller, 0x1000)]).unwrap_or_default()
        }
        2 if (0..8).contains(&arg) => vec![on(0x801F_8D50 + 0x40 * arg as u32, caller, 0x1000)],
        3 => vec![
            on(0x801F_90A8, caller, 0x1000),
            on(0x801F_91FC, caller, 0x1000),
        ],
        4 => vec![on(0x801F_91A4, caller, 0x1000)],
        _ => Vec::new(),
    }
}

/// PROT 0921 (Iota) - the stager `0x801F800C`, one arm (`a1 == 0`).
///
/// It seats `0x801F8448` on the caller and draws an angle
/// `a = rand % 4095`, storing `+0x3C = trunc(sin a / 4)` and
/// `+0x3E = trunc(cos a / 4)` into that part; it then seats `0x801F8498` on
/// the caller and stores `+0x84 = (rand & 0xFF) - 0x80` and
/// `+0x28 = rand & 0xFFF` into it (`0x801F8028..0x801F8104`).
///
/// PORT: FUN_801F800C (PROT 0921 stager)
pub fn iota_stager_spawns(
    arm: u8,
    caller: StagerCaller,
    rand: &mut dyn FnMut() -> u32,
) -> Vec<StagerSpawn> {
    if arm != 0 {
        return Vec::new();
    }
    let mut first = on(0x801F_8448, caller, 0x1000);
    let a = rem(rand, 4095) as u16;
    let (sin, cos) = trig12(a);
    first.pokes = vec![
        (0x3C, (i32::from(sin) / 4) as u16),
        (0x3E, (i32::from(cos) / 4) as u16),
    ];
    let mut second = on(0x801F_8498, caller, 0x1000);
    let spin = (rand() & 0xFF) as i32 - 0x80;
    let roll = rand() & 0xFFF;
    second.pokes = vec![(0x84, spin as u16), (0x28, roll as u16)];
    vec![first, second]
}

/// PROT 0921's two records its stager seats on every call.
pub const IOTA_VOLLEY_RECORDS: [u32; 2] = [0x801F_8448, 0x801F_8498];

/// PROT 0928 (Palma) - the stager `0x801F8E68`, seven arms (jump table
/// `0x801F69D8`):
///
/// | arm | seats |
/// |---:|---|
/// | 0 / 1 | `0x801F93D0` / `0x801F9424` on the caller at its scale |
/// | 2 / 3 / 6 | `0x801F94D4` / `0x801F9B48` / `0x801F9EB4` on the caller |
/// | 4 | `0x801F9CD0`, `0x801F9D20`, `0x801F9C64` on the caller; `0x801F9B9C` and `0x801F9C00` at `(0, 0, 0x7D00)` turned by the caller, scale `0x4000`; one `0x801F9D68` at `(0, 0, 0x7D00)`, then forty-eight more scattered over `rand % 4095` (`x - 0x800`, `y - 0x800`, `0x7D00 - z`) and turned `-trunc(x / 16)` about Y, each scale `0x1800` and stored `+0x3E = trunc(y / 8)` (`0x801F8F0C..0x801F90C8`) |
/// | 5 | eight `0x801F9F08` on the caller: `+0x54 = k`, `+0x84 = trunc(((rand & 0xFF) - 0x80) / 8)`, `+0xB4 = 0x100`, `+0xB8 = +0xBA = -0x300`, `+0xB6 = 0x1000`, `+0x28 = 0x100 + 0x200 k`; then `0x801F9CD0`, `0x801F9D20`, `0x801F9B9C`, `0x801F9C00` at `(0, 0, 0x80)`, unturned, scale `0x40` (`0x801F90D0..0x801F91C4`) |
///
/// PORT: FUN_801F8E68 (PROT 0928 stager; every arm, arm 4's handle store
/// `0x801FA0E0` not carried)
pub fn palma_stager_spawns(
    arm: u8,
    caller: StagerCaller,
    rand: &mut dyn FnMut() -> u32,
) -> Vec<StagerSpawn> {
    let s = caller.scale;
    match arm {
        0 => vec![on(0x801F_93D0, caller, s)],
        1 => vec![on(0x801F_9424, caller, s)],
        2 => vec![on(0x801F_94D4, caller, 0x1000)],
        3 => vec![on(0x801F_9B48, caller, 0x1000)],
        4 => {
            let far = [0, 0, 0x7D00];
            let mut out = vec![
                on(0x801F_9CD0, caller, 0x1000),
                on(0x801F_9D20, caller, 0x1000),
                on(0x801F_9C64, caller, 0x1000),
                spawn(0x801F_9B9C, far, caller.rot, 0x4000),
                spawn(0x801F_9C00, far, caller.rot, 0x4000),
                spawn(0x801F_9D68, far, [0; 3], 0x1800),
            ];
            for _ in 0..48 {
                let x = rem(rand, 4095) - 0x800;
                let y = rem(rand, 4095) - 0x800;
                let z = 0x7D00 - rem(rand, 4095);
                let mut sp = spawn(
                    0x801F_9D68,
                    [x as i16, y as i16, z as i16],
                    [0, (-(x / 16)) as i16, 0],
                    0x1800,
                );
                sp.pokes = vec![(0x3E, (y / 8) as u16)];
                out.push(sp);
            }
            out
        }
        5 => {
            let mut out: Vec<StagerSpawn> = (0..8u16)
                .map(|k| {
                    let mut sp = on(0x801F_9F08, caller, 0x1000);
                    let spin = ((rand() & 0xFF) as i32 - 0x80) / 8;
                    sp.pokes = vec![
                        (0x54, k),
                        (0x84, spin as u16),
                        (0xB4, 0x100),
                        (0xB8, (-0x300i16) as u16),
                        (0xBA, (-0x300i16) as u16),
                        (0xB6, 0x1000),
                        (0x28, 0x100 + 0x200 * k),
                    ];
                    sp
                })
                .collect();
            for r in [0x801F_9CD0, 0x801F_9D20, 0x801F_9B9C, 0x801F_9C00] {
                out.push(spawn(r, [0, 0, 0x80], [0; 3], 0x40));
            }
            out
        }
        6 => vec![on(0x801F_9EB4, caller, 0x1000)],
        _ => Vec::new(),
    }
}

/// PROT 0928's records its stager scatters off `rand()`.
pub const PALMA_VOLLEY_RECORDS: [u32; 2] = [0x801F_9D68, 0x801F_9F08];

/// PROT 0929's stager arm 3 (`0x801F8E30..0x801F8EAC`): two
/// `FUN_80024E80(0x801C9070, 1)` calls over one template, rewritten between
/// them. Both are kind `1` (additive) over `0x20` frames with no hold: a
/// rise from black to white, then a fall from white to black that waits the
/// rise out (`+0x14 = 0x20`, the start delay). The template's id word
/// `+0x18` is not written; the call stamps id `1`.
///
/// ```text
/// sh 1,(t)  sh 0x20,2(t)  sh 0,6/8/A(t)    sh 0xFF,E/10/12(t)  sh 0,14(t)  sh 0,16(t)
/// sh 1,(t)  sh 0x20,2(t)  sh 0xFF,6/8/A(t) sh 0,E/10/12(t)     sh 0x20,14(t) sh 0,16(t)
/// ```
pub const MULE_STAGER_FADES: [legaia_engine_battle_vm::battle_action::SummonFadeTemplate; 2] = [
    legaia_engine_battle_vm::battle_action::SummonFadeTemplate {
        kind: 1,
        duration: 0x20,
        start_rgb: [0, 0, 0],
        end_rgb: [0xFF, 0xFF, 0xFF],
        delay: 0,
        hold: 0,
    },
    legaia_engine_battle_vm::battle_action::SummonFadeTemplate {
        kind: 1,
        duration: 0x20,
        start_rgb: [0xFF, 0xFF, 0xFF],
        end_rgb: [0, 0, 0],
        delay: 0x20,
        hold: 0,
    },
];

/// PROT 0929's stager arm that spawns [`MULE_STAGER_FADES`].
pub const MULE_STAGER_FADE_ARM: u8 = 3;

/// PROT 0929 (Mule) - the stager `0x801F8C30`, nine arms (jump table
/// `0x801F69D8`):
///
/// | arm | seats |
/// |---:|---|
/// | 0 / 4 | `0x801F90A4` / `0x801F95DC` at `(rand % 1025 - 0x200, 0, rand % 1025 - 0x200)`, turned by the caller, at its scale |
/// | 1 | twenty-four `0x801F90A4` at `(rand % 513 - 0x100, -(rand % 257), -(rand % 33) - 0x48A)`, turned by the caller, at its scale |
/// | 2 | eight `0x801F92DC` on the caller: `+0x54 = k`, `+0x84 = 0x40`, `+0xB4 = +0xB8 = +0xBA = 0`, `+0xB6 = 0x600`, `+0x28 = 0x200 k` |
/// | 3 | no seat: the two screen fades [`MULE_STAGER_FADES`] |
/// | 5 / 6 / 7 | `0x801F9944` / `0x801F9998` / `0x801F9A0C` on the caller at its scale |
/// | 8 | four `0x801F98D8` on the caller: `+0xBA = -0x100`, `+0xB8 = 0`, `+0x84 = rand % 33 - 0x10`, `+0xB6 = 0x400`, `+0xB4 = rand % 129 + 0x80`, `+0x28 = (caller +0x28 + 0x400 k + rand % 128) & 0xFFF` |
///
/// PORT: FUN_801F8C30 (PROT 0929 stager; every spawn arm)
pub fn mule_stager_spawns(
    arm: u8,
    caller: StagerCaller,
    rand: &mut dyn FnMut() -> u32,
) -> Vec<StagerSpawn> {
    let s = caller.scale;
    match arm {
        0 | 4 => {
            let x = rem(rand, 1025) - 0x200;
            let z = rem(rand, 1025) - 0x200;
            let rec = if arm == 0 { 0x801F_90A4 } else { 0x801F_95DC };
            vec![spawn(rec, [x as i16, 0, z as i16], caller.rot, s)]
        }
        1 => (0..24)
            .map(|_| {
                let x = rem(rand, 513) - 0x100;
                let y = -rem(rand, 257);
                let z = -rem(rand, 33) - 0x48A;
                spawn(0x801F_90A4, [x as i16, y as i16, z as i16], caller.rot, s)
            })
            .collect(),
        2 => (0..8u16)
            .map(|k| {
                let mut sp = on(0x801F_92DC, caller, 0x1000);
                sp.pokes = vec![
                    (0x54, k),
                    (0x84, 0x40),
                    (0xB8, 0),
                    (0xBA, 0),
                    (0xB4, 0),
                    (0xB6, 0x600),
                    (0x28, k << 9),
                ];
                sp
            })
            .collect(),
        5 => vec![on(0x801F_9944, caller, s)],
        6 => vec![on(0x801F_9998, caller, s)],
        7 => vec![on(0x801F_9A0C, caller, s)],
        8 => (0..4i32)
            .map(|k| {
                let mut sp = on(0x801F_98D8, caller, 0x1000);
                let spin = rem(rand, 33) - 0x10;
                let b4 = rem(rand, 129) + 0x80;
                let roll = (i32::from(caller.rot[2]) + (k << 10) + rem(rand, 128)) & 0xFFF;
                sp.pokes = vec![
                    (0xBA, (-0x100i16) as u16),
                    (0xB8, 0),
                    (0x84, spin as u16),
                    (0xB6, 0x400),
                    (0xB4, b4 as u16),
                    (0x28, roll as u16),
                ];
                sp
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// PROT 0929's records its stager places off `rand()`.
pub const MULE_VOLLEY_RECORDS: [u32; 3] = [0x801F_90A4, 0x801F_95DC, 0x801F_98D8];

/// PROT 0931 (Jedo) - the stager `0x801F8ADC`: arms 0 / 1 / 2 seat
/// `0x801F8C8C` / `0x801F8CF8` / `0x801F95AC` on the caller (`FUN_80050ED4`).
///
/// PORT: FUN_801F8ADC (PROT 0931 stager; every arm)
pub fn jedo_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    let rec = match arm {
        0 => 0x801F_8C8C,
        1 => 0x801F_8CF8,
        2 => 0x801F_95AC,
        _ => return Vec::new(),
    };
    vec![on(rec, caller, 0x1000)]
}

/// PROT 0932 (Meta) - the stager `0x801F84A4`: arm 0 seats `0x801F8B68` on
/// the caller at its scale (`lh a3,0x72`).
///
/// PORT: FUN_801F84A4 (PROT 0932 stager)
pub fn meta_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    if arm == 0 {
        vec![on(0x801F_8B68, caller, caller.scale)]
    } else {
        Vec::new()
    }
}

/// PROT 0933 (Terra) - the stager `0x801F8748`: arm 0 seats `0x801F8838`
/// and `0x801F88F0`, arms 1 / 2 / 3 `0x801F8E88` / `0x801F90A8` /
/// `0x801F91E4`, each on the caller (`FUN_80050ED4`).
///
/// PORT: FUN_801F8748 (PROT 0933 stager; every arm)
pub fn terra_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    match arm {
        0 => vec![
            on(0x801F_8838, caller, 0x1000),
            on(0x801F_88F0, caller, 0x1000),
        ],
        1 => vec![on(0x801F_8E88, caller, 0x1000)],
        2 => vec![on(0x801F_90A8, caller, 0x1000)],
        3 => vec![on(0x801F_91E4, caller, 0x1000)],
        _ => Vec::new(),
    }
}

/// The records each ported summon-creature stager seats itself, which the
/// host's whole-set seat of the module's unnamed records leaves out.
pub fn creature_stager_records(entry: u32) -> &'static [u32] {
    match entry {
        914 => &[
            0x801F_7C30,
            0x801F_7C4C,
            0x801F_7CFC,
            0x801F_7D48,
            0x801F_7E6C,
            0x801F_7EC8,
        ],
        915 => &[0x801F_8004, 0x801F_80A4],
        917 => &[
            0x801F_88F8,
            0x801F_893C,
            0x801F_8980,
            0x801F_89C4,
            0x801F_8A08,
            0x801F_8A4C,
            0x801F_8A90,
            0x801F_8AD4,
            0x801F_8B40,
            0x801F_8B78,
            0x801F_8BB0,
            0x801F_8BE8,
            0x801F_8D50,
            0x801F_8D90,
            0x801F_8DD0,
            0x801F_8E10,
            0x801F_8E50,
            0x801F_8E90,
            0x801F_8ED0,
            0x801F_8F10,
            0x801F_90A8,
            0x801F_91FC,
            0x801F_91A4,
        ],
        921 => &IOTA_VOLLEY_RECORDS,
        928 => &[
            0x801F_93D0,
            0x801F_9424,
            0x801F_94D4,
            0x801F_9B48,
            0x801F_9CD0,
            0x801F_9D20,
            0x801F_9C64,
            0x801F_9B9C,
            0x801F_9C00,
            0x801F_9D68,
            0x801F_9F08,
            0x801F_9EB4,
        ],
        929 => &[
            0x801F_90A4,
            0x801F_95DC,
            0x801F_92DC,
            0x801F_9944,
            0x801F_9998,
            0x801F_9A0C,
            0x801F_98D8,
        ],
        931 => &[0x801F_8C8C, 0x801F_8CF8, 0x801F_95AC],
        932 => &[0x801F_8B68],
        933 => &[
            0x801F_8838,
            0x801F_88F0,
            0x801F_8E88,
            0x801F_90A8,
            0x801F_91E4,
        ],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counter() -> impl FnMut() -> u32 {
        let mut n = 0u32;
        move || {
            n = n.wrapping_add(0x1357);
            n & 0x7FFF
        }
    }

    fn caller() -> StagerCaller {
        StagerCaller {
            pos: [10, -20, 30],
            rot: [0, 0x7C0, 0x123],
            scale: 0x800,
            ..Default::default()
        }
    }

    #[test]
    fn gola_gola_ring_steps_round_the_callers_heading() {
        let s = gola_gola_stager_spawns(3, caller());
        assert_eq!(s.len(), 32);
        assert_eq!(s[0].rot, [0, 0x840, 0]);
        assert_eq!(s[31].rot, [0, ((0x7C0 + 32 * 0x80) & 0xFFF) as i16, 0]);
        assert!(
            s.iter()
                .all(|sp| sp.scale == 0x800 && sp.pos == caller().pos)
        );
    }

    #[test]
    fn barra_arm_one_cycles_its_counter() {
        let mut c = 0u16;
        let recs: Vec<u32> = (0..5)
            .map(|_| barra_stager_spawns(1, caller(), &mut c)[0].record)
            .collect();
        assert_eq!(
            recs,
            [
                0x801F_8B40,
                0x801F_8B78,
                0x801F_8BB0,
                0x801F_8BE8,
                0x801F_8B40
            ]
        );
        let mut c2 = 0;
        let c0 = StagerCaller { arg: 3, ..caller() };
        assert_eq!(barra_stager_spawns(0, c0, &mut c2)[0].record, 0x801F_89C4);
        assert_eq!(barra_stager_spawns(2, c0, &mut c2)[0].record, 0x801F_8E10);
    }

    #[test]
    fn iota_and_mule_take_their_rands_in_range() {
        let mut r = counter();
        let s = iota_stager_spawns(0, caller(), &mut r);
        assert_eq!(s.len(), 2);
        let x = s[0].pokes[0].1 as i16;
        assert!(x.abs() <= 1024);
        let s = mule_stager_spawns(1, caller(), &mut r);
        assert_eq!(s.len(), 24);
        for sp in &s {
            assert!((-0x100..=0x100).contains(&sp.pos[0]));
            assert!((-256..=0).contains(&sp.pos[1]));
            assert!((-0x48A - 32..=-0x48A).contains(&sp.pos[2]));
        }
        let s = mule_stager_spawns(8, caller(), &mut r);
        assert_eq!(s.len(), 4);
        let roll = s[1].pokes.iter().find(|p| p.0 == 0x28).unwrap().1 as i32;
        let base = (0x123 + 0x400) & 0xFFF;
        assert!((roll - base).rem_euclid(0x1000) < 128);
    }

    #[test]
    fn palma_arm_four_scatters_forty_eight() {
        let mut r = counter();
        let s = palma_stager_spawns(4, caller(), &mut r);
        assert_eq!(s.len(), 6 + 48);
        for sp in &s[6..] {
            assert_eq!(sp.record, 0x801F_9D68);
            assert!(sp.pos[2] <= 0x7D00 && sp.pos[2] > 0x7D00 - 4095);
            assert_eq!(sp.rot[1], -(sp.pos[0] / 16));
        }
        let s = palma_stager_spawns(5, caller(), &mut r);
        assert_eq!(s.len(), 12);
    }

    /// Every record an arm seats is listed as the stager's own.
    #[test]
    fn listed_records_cover_every_arm() {
        let mut r = counter();
        let mut c = 0u16;
        for (entry, arms) in [
            (914u32, 6u8),
            (915, 2),
            (917, 5),
            (921, 1),
            (928, 7),
            (929, 9),
            (931, 3),
            (932, 1),
            (933, 4),
        ] {
            for arm in 0..arms {
                for arg in 0..8 {
                    let cl = StagerCaller { arg, ..caller() };
                    let s = match entry {
                        914 => gola_gola_stager_spawns(arm, cl),
                        915 => mushura_stager_spawns(arm, cl),
                        917 => barra_stager_spawns(arm, cl, &mut c),
                        921 => iota_stager_spawns(arm, cl, &mut r),
                        928 => palma_stager_spawns(arm, cl, &mut r),
                        929 => mule_stager_spawns(arm, cl, &mut r),
                        931 => jedo_stager_spawns(arm, cl),
                        932 => meta_stager_spawns(arm, cl),
                        _ => terra_stager_spawns(arm, cl),
                    };
                    for sp in s {
                        assert!(
                            creature_stager_records(entry).contains(&sp.record),
                            "{entry} arm {arm}: {:x}",
                            sp.record
                        );
                    }
                }
            }
        }
    }
}
