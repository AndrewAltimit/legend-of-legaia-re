//! The capture-class modules' per-arm part spawns: PROT 0966 (Evil Seru
//! Magic)'s tick body `FUN_801F6A74`, whose every `FUN_80050ED4` /
//! `FUN_80021B04` call seats a record at a literal point on its arm's pass,
//! read off the disassembly the way [`super::creature_arm_spawns`] is. The
//! host seats them there rather than the whole record set at the caster when
//! the capture band pages the module in (`0x6E`), which drew the records a
//! frame band early and in the wrong place.
//!
//! See `ghidra/scripts/funcs/overlay_cast_evil_seru_magic_0966_801f6a74.txt`.

use super::*;
use SpawnAnchor::LiteralScaled;

const fn spawn(record: u32, anchor: SpawnAnchor) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(record),
        anchor,
    }
}
const E966_2: [ModuleSpawn; 4] = [
    // 0x801F6E98
    spawn(
        0x801F9160,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6EB0
    spawn(
        0x801F91BC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6EC8
    spawn(
        0x801F9218,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6EE0
    spawn(
        0x801F92D0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const E966_4: [ModuleSpawn; 1] = [
    // 0x801F6FB8
    spawn(
        0x801F9274,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const E966_10: [ModuleSpawn; 15] = [
    // 0x801F714C
    spawn(
        0x801F9328,
        LiteralScaled {
            pos: [0, 0, 400],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7164
    spawn(
        0x801F937C,
        LiteralScaled {
            pos: [0, 0, 400],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F717C
    spawn(
        0x801F93E4,
        LiteralScaled {
            pos: [0, 0, 400],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7194
    spawn(
        0x801F9430,
        LiteralScaled {
            pos: [0, 0, 400],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F71AC
    spawn(
        0x801F9478,
        LiteralScaled {
            pos: [0, 0, 400],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F71C4
    spawn(
        0x801F94D0,
        LiteralScaled {
            pos: [0, 0, 400],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F71DC
    spawn(
        0x801F94F0,
        LiteralScaled {
            pos: [0, 0, 400],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F71F4
    spawn(
        0x801F957C,
        LiteralScaled {
            pos: [0, 0, 400],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F720C
    spawn(
        0x801F95C8,
        LiteralScaled {
            pos: [0, 0, 400],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7224
    spawn(
        0x801F9610,
        LiteralScaled {
            pos: [0, 0, 400],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F723C
    spawn(
        0x801F9668,
        LiteralScaled {
            pos: [0, 0, 400],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7254
    spawn(
        0x801F96B8,
        LiteralScaled {
            pos: [0, 0, 400],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F726C
    spawn(
        0x801F9724,
        LiteralScaled {
            pos: [0, 0, 400],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7284
    spawn(
        0x801F9854,
        LiteralScaled {
            pos: [0, 0, 400],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F729C
    spawn(
        0x801F98B4,
        LiteralScaled {
            pos: [0, 0, 400],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const E966_23: [ModuleSpawn; 18] = [
    // 0x801F8074
    spawn(
        0x801F9914,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8098
    spawn(
        0x801F9AD0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F80B4
    spawn(
        0x801F9B48,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F80D0
    spawn(
        0x801F9BC4,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F80EC
    spawn(
        0x801F9C40,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8108
    spawn(
        0x801F9CB8,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8124
    spawn(
        0x801F9D34,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8140
    spawn(
        0x801F9DB0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F815C
    spawn(
        0x801F9F54,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8178
    spawn(
        0x801F9FB4,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8194
    spawn(
        0x801FA018,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F81B0
    spawn(
        0x801FA07C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F81CC
    spawn(
        0x801FA0E0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F81E8
    spawn(
        0x801FA144,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8204
    spawn(
        0x801F99A8,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F821C
    spawn(
        0x801F9A3C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8234
    spawn(
        0x801F9E2C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F824C
    spawn(
        0x801F9EC0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const E966_26: [ModuleSpawn; 8] = [
    // 0x801F8428
    spawn(
        0x801FA1A8,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8440
    spawn(
        0x801FA208,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8458
    spawn(
        0x801FA284,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8470
    spawn(
        0x801FA2D0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8488
    spawn(
        0x801FA388,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F84A0
    spawn(
        0x801FA3D0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F84B8
    spawn(
        0x801FA418,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F84D0
    spawn(
        0x801FA268,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];

/// The capture-class entries whose spawns are seated per arm instead of by
/// the pager.
pub const CAPTURE_SPAWN_ENTRIES: [u32; 1] = [966];

/// The spawn calls capture-class module `prot_entry`'s arm `phase` makes on
/// its pass; empty for an arm that makes none.
pub fn capture_arm_spawns(prot_entry: u32, phase: u8) -> &'static [ModuleSpawn] {
    match (prot_entry, phase) {
        (966, 2) => &E966_2,
        (966, 4) => &E966_4,
        (966, 10) => &E966_10,
        (966, 23) => &E966_23,
        (966, 26) => &E966_26,
        _ => &[],
    }
}
