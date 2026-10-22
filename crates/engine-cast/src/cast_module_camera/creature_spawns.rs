//! The summon-creature modules' per-arm part spawns: every `FUN_80021B04`
//! and `FUN_80050ED4` call of PROT 0914, 0915, 0917, 0921, 0923 and
//! 0928..0934's tick bodies, with the record its `a2` names and the point,
//! angles and scale its `a0` / `a1` / `a3` carry, read off each image's
//! disassembly. The host seats them on the pass their arm makes the call
//! ([`ModuleProfile::stages_spawns`]), rather than the whole record set on
//! the stager's first tick.
//!
//! Generated from the dumps: each site is a straight-line walk of its arm
//! from the arm's entry (the stack trios the prologue zeroes, the halfword
//! stores into them, the `lui` / `addiu` of the record), with the loops and
//! the seat-relative points read by hand. Three kinds of point are not
//! carried: a `rand()` spread (PROT 0931 arms 5..7, PROT 0934 arms 20..24 -
//! the part is seated at the spread's centre), a height off the victim's
//! `+0x3E` (PROT 0917 arm `0xB`, seated at `z = 0`), and PROT 0929 arm 17's
//! world-anchor point and TR z scale (seated on the origin at unit scale).
//! A loop that seats the same record many times over a per-part velocity
//! write (PROT 0930 arm 6's sixteen passes) is seated once per call site.
//!
//! See `ghidra/scripts/funcs/overlay_summon_<label>_<entry>_<va>.txt`.

use super::*;

/// When a spawn call runs relative to its arm's gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnTiming {
    /// On the pass that leaves the arm.
    Pass,
    /// On every pass the arm runs, held or not.
    Every,
}

use SpawnAnchor::*;

const fn spawn(record: u32, anchor: SpawnAnchor) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(record),
        anchor,
    }
}
const S914_3: [ModuleSpawn; 1] = [
    // 0x801F6CDC
    spawn(0x801F7BA8, Creature),
];
const S914_11: [ModuleSpawn; 1] = [
    // 0x801F7320
    spawn(0x801F7C98, CasterCreatureRot),
];
const S915_3: [ModuleSpawn; 2] = [
    // 0x801F6D74
    spawn(
        0x801F7F98,
        VictimOffset {
            dx: 0,
            y: -0x190,
            dz: -0x800,
        },
    ),
    // 0x801F6D8C
    spawn(
        0x801F8050,
        VictimOffset {
            dx: 0,
            y: -0x190,
            dz: -0x800,
        },
    ),
];
const S915_9: [ModuleSpawn; 1] = [
    // 0x801F7634
    spawn(0x801F80F0, Creature),
];
const S915_12_EVERY: [ModuleSpawn; 1] = [
    // 0x801F7D74
    spawn(0x801F8108, Creature),
];
const S917_2: [ModuleSpawn; 3] = [
    // 0x801F6DA4
    spawn(
        0x801F85B8,
        LiteralScaled {
            pos: [0, 0, 4096],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6DBC
    spawn(
        0x801F8614,
        LiteralScaled {
            pos: [0, 0, 4096],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6DD4
    spawn(
        0x801F8670,
        LiteralScaled {
            pos: [0, 0, 4096],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S917_6: [ModuleSpawn; 2] = [
    // 0x801F73FC
    spawn(
        0x801F86CC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [192, 2176, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F741C
    spawn(
        0x801F8B18,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S917_11: [ModuleSpawn; 6] = [
    // 0x801F7980
    spawn(
        0x801F8C20,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [-4, 2304, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F79B4
    spawn(
        0x801F8F50,
        LiteralScaled {
            pos: [0, -2048, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F79D8
    spawn(
        0x801F8FA0,
        LiteralScaled {
            pos: [0, -2048, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F79F4
    spawn(
        0x801F8FF0,
        LiteralScaled {
            pos: [0, -2048, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7A10
    spawn(
        0x801F9040,
        LiteralScaled {
            pos: [0, -2048, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7A2C
    spawn(
        0x801F9090,
        LiteralScaled {
            pos: [0, -2048, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S917_13: [ModuleSpawn; 5] = [
    // 0x801F7EB8
    spawn(0x801F9104, VictimFlat),
    // 0x801F7ED0
    spawn(0x801F9150, VictimFlat),
    // 0x801F7EE8
    spawn(0x801F9240, VictimFlat),
    // 0x801F7F00
    spawn(0x801F927C, VictimFlat),
    // 0x801F7F18
    spawn(0x801F92CC, VictimFlat),
];
const S921_3: [ModuleSpawn; 16] = [
    // 0x801F6DFC
    spawn(
        0x801F8420,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E20
    spawn(
        0x801F8260,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E3C
    spawn(
        0x801F82B8,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E58
    spawn(
        0x801F832C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E74
    spawn(
        0x801F83A0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E90
    spawn(
        0x801F8594,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6EAC
    spawn(
        0x801F85FC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6EC8
    spawn(
        0x801F84F4,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6EE4
    spawn(
        0x801F8544,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6F00
    spawn(
        0x801F8664,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6F18
    spawn(
        0x801F86B8,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6F30
    spawn(
        0x801F8814,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6F48
    spawn(
        0x801F81C0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6F64
    spawn(
        0x801F8120,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6F80
    spawn(
        0x801F81A4,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6F9C
    spawn(
        0x801F8244,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S921_8: [ModuleSpawn; 5] = [
    // 0x801F7568
    spawn(
        0x801F870C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7580
    spawn(
        0x801F8764,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7598
    spawn(
        0x801F87BC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F75B0
    spawn(
        0x801F8878,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F75C8
    spawn(
        0x801F88C0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S921_9: [ModuleSpawn; 7] = [
    // 0x801F7964
    spawn(
        0x801F8908,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F797C
    spawn(
        0x801F8990,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7994
    spawn(
        0x801F8A18,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F79AC
    spawn(
        0x801F8A34,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F79C4
    spawn(
        0x801F8A50,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F79DC
    spawn(
        0x801F8A98,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F79F4
    spawn(
        0x801F8AE0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S921_10: [ModuleSpawn; 2] = [
    // 0x801F7B10
    spawn(
        0x801F8B4C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7B28
    spawn(
        0x801F8BB8,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S923_2: [ModuleSpawn; 5] = [
    // 0x801F6CEC
    spawn(
        0x801F8CB4,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6D04
    spawn(
        0x801F8D20,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6D1C
    spawn(
        0x801F8D8C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6D34
    spawn(
        0x801F8DF8,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6D4C
    spawn(
        0x801F8E88,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S923_4: [ModuleSpawn; 26] = [
    // 0x801F6F28
    spawn(
        0x801F928C,
        LiteralScaled {
            pos: [0, -704, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6F28
    spawn(
        0x801F928C,
        LiteralScaled {
            pos: [609, -352, 0],
            rot: [0, 0, 682],
            scale: 0x1000,
        },
    ),
    // 0x801F6F28
    spawn(
        0x801F928C,
        LiteralScaled {
            pos: [610, 350, 0],
            rot: [0, 0, 1364],
            scale: 0x1000,
        },
    ),
    // 0x801F6F28
    spawn(
        0x801F928C,
        LiteralScaled {
            pos: [2, 703, 0],
            rot: [0, 0, 2046],
            scale: 0x1000,
        },
    ),
    // 0x801F6F28
    spawn(
        0x801F928C,
        LiteralScaled {
            pos: [-608, 354, 0],
            rot: [0, 0, 2728],
            scale: 0x1000,
        },
    ),
    // 0x801F6F28
    spawn(
        0x801F928C,
        LiteralScaled {
            pos: [-611, -348, 0],
            rot: [0, 0, 3410],
            scale: 0x1000,
        },
    ),
    // 0x801F6F90
    spawn(
        0x801F8EE4,
        LiteralScaled {
            pos: [256, 0, 512],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6FAC
    spawn(
        0x801F8F4C,
        LiteralScaled {
            pos: [256, 0, 512],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6FC8
    spawn(
        0x801F8FB4,
        LiteralScaled {
            pos: [256, 0, 512],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6FE4
    spawn(
        0x801F901C,
        LiteralScaled {
            pos: [256, 0, 512],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7004
    spawn(
        0x801F8EE4,
        LiteralScaled {
            pos: [-256, 0, 512],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7018
    spawn(
        0x801F8F4C,
        LiteralScaled {
            pos: [-256, 0, 512],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F702C
    spawn(
        0x801F8FB4,
        LiteralScaled {
            pos: [-256, 0, 512],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7040
    spawn(
        0x801F901C,
        LiteralScaled {
            pos: [-256, 0, 512],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7060
    spawn(
        0x801F8EE4,
        LiteralScaled {
            pos: [512, 0, 768],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7074
    spawn(
        0x801F8F4C,
        LiteralScaled {
            pos: [512, 0, 768],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7088
    spawn(
        0x801F8FB4,
        LiteralScaled {
            pos: [512, 0, 768],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F709C
    spawn(
        0x801F901C,
        LiteralScaled {
            pos: [512, 0, 768],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F70BC
    spawn(
        0x801F8EE4,
        LiteralScaled {
            pos: [-512, 0, 768],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F70D0
    spawn(
        0x801F8F4C,
        LiteralScaled {
            pos: [-512, 0, 768],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F70E4
    spawn(
        0x801F8FB4,
        LiteralScaled {
            pos: [-512, 0, 768],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F70F8
    spawn(
        0x801F901C,
        LiteralScaled {
            pos: [-512, 0, 768],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7118
    spawn(0x801F9084, CreatureScaled { scale: 0x1000 }),
    // 0x801F7138
    spawn(0x801F916C, CreatureScaled { scale: 0x400 }),
    // 0x801F7150
    spawn(0x801F91C4, CreatureScaled { scale: 0x400 }),
    // 0x801F7168
    spawn(0x801F9228, CreatureScaled { scale: 0x400 }),
];
const S923_8: [ModuleSpawn; 14] = [
    // 0x801F769C
    spawn(
        0x801F92D8,
        LiteralScaled {
            pos: [0, -1024, 980],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F76C4
    spawn(
        0x801F9320,
        LiteralScaled {
            pos: [0, -1024, 980],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F76E4
    spawn(
        0x801F9368,
        LiteralScaled {
            pos: [0, -1024, 980],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7714
    spawn(
        0x801F92D8,
        LiteralScaled {
            pos: [176, -1000, 1392],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F772C
    spawn(
        0x801F9320,
        LiteralScaled {
            pos: [176, -1000, 1392],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7744
    spawn(
        0x801F9368,
        LiteralScaled {
            pos: [176, -1000, 1392],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F776C
    spawn(
        0x801F92D8,
        LiteralScaled {
            pos: [-160, -1000, 1384],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7784
    spawn(
        0x801F9320,
        LiteralScaled {
            pos: [-160, -1000, 1384],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F779C
    spawn(
        0x801F9368,
        LiteralScaled {
            pos: [-160, -1000, 1384],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F77CC
    spawn(
        0x801F940C,
        LiteralScaled {
            pos: [0, -1012, 1252],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F77E8
    spawn(
        0x801F93B0,
        LiteralScaled {
            pos: [0, -1012, 1252],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7808
    spawn(
        0x801F9564,
        LiteralScaled {
            pos: [0, -1024, 1252],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7824
    spawn(
        0x801F95CC,
        LiteralScaled {
            pos: [0, -1024, 1252],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7840
    spawn(
        0x801F94FC,
        LiteralScaled {
            pos: [0, -1024, 1252],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S923_10: [ModuleSpawn; 10] = [
    // 0x801F79C0
    spawn(
        0x801F9634,
        LiteralScaled {
            pos: [0, 0, -1252],
            rot: [256, 2560, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F79D8
    spawn(
        0x801F96A4,
        LiteralScaled {
            pos: [0, 0, -1252],
            rot: [256, 2560, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F79F0
    spawn(
        0x801F9720,
        LiteralScaled {
            pos: [0, 0, -1252],
            rot: [256, 2560, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7A08
    spawn(
        0x801F9778,
        LiteralScaled {
            pos: [0, 0, -1252],
            rot: [256, 2560, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7A20
    spawn(
        0x801F97D0,
        LiteralScaled {
            pos: [0, 0, -1252],
            rot: [256, 2560, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7A44
    spawn(
        0x801F9854,
        LiteralScaled {
            pos: [0, 0, -1252],
            rot: [256, 2560, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7A60
    spawn(
        0x801F98C0,
        LiteralScaled {
            pos: [0, 0, -1252],
            rot: [256, 2560, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7A7C
    spawn(
        0x801F9928,
        LiteralScaled {
            pos: [0, 0, -1252],
            rot: [256, 2560, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7A98
    spawn(
        0x801F9990,
        LiteralScaled {
            pos: [0, 0, -1252],
            rot: [256, 2560, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7AB4
    spawn(
        0x801F94DC,
        LiteralScaled {
            pos: [0, 0, -1252],
            rot: [256, 2560, 0],
            scale: 0x1000,
        },
    ),
];
const S923_12: [ModuleSpawn; 3] = [
    // 0x801F7DF4
    spawn(
        0x801F99E0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7E0C
    spawn(
        0x801F9A50,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7E24
    spawn(
        0x801FA458,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S923_14: [ModuleSpawn; 8] = [
    // 0x801F800C
    spawn(
        0x801F9FCC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F802C
    spawn(
        0x801FA074,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F80E0
    spawn(
        0x801FA090,
        LiteralScaled {
            pos: [0, 0, 315],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F80E0
    spawn(
        0x801FA090,
        LiteralScaled {
            pos: [272, 0, 157],
            rot: [0, 682, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F80E0
    spawn(
        0x801FA090,
        LiteralScaled {
            pos: [272, 0, -157],
            rot: [0, 1365, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F80E0
    spawn(
        0x801FA090,
        LiteralScaled {
            pos: [0, 0, -315],
            rot: [0, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F80E0
    spawn(
        0x801FA090,
        LiteralScaled {
            pos: [-272, 0, -157],
            rot: [0, 2730, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F80E0
    spawn(
        0x801FA090,
        LiteralScaled {
            pos: [-272, 0, 157],
            rot: [0, 3413, 0],
            scale: 0x1000,
        },
    ),
];
const S923_19: [ModuleSpawn; 12] = [
    // 0x801F8554
    spawn(
        0x801FA0F4,
        LiteralScaled {
            pos: [511, 0, 887],
            rot: [0, 341, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8554
    spawn(
        0x801FA158,
        LiteralScaled {
            pos: [511, 0, 887],
            rot: [0, 341, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8554
    spawn(
        0x801FA0F4,
        LiteralScaled {
            pos: [1023, 0, 1],
            rot: [0, 1023, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8554
    spawn(
        0x801FA158,
        LiteralScaled {
            pos: [1023, 0, 1],
            rot: [0, 1023, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8554
    spawn(
        0x801FA0F4,
        LiteralScaled {
            pos: [512, 0, -886],
            rot: [0, 1706, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8554
    spawn(
        0x801FA158,
        LiteralScaled {
            pos: [512, 0, -886],
            rot: [0, 1706, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8554
    spawn(
        0x801FA0F4,
        LiteralScaled {
            pos: [-511, 0, -887],
            rot: [0, 2389, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8554
    spawn(
        0x801FA158,
        LiteralScaled {
            pos: [-511, 0, -887],
            rot: [0, 2389, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8554
    spawn(
        0x801FA0F4,
        LiteralScaled {
            pos: [-1023, 0, -1],
            rot: [0, 3071, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8554
    spawn(
        0x801FA158,
        LiteralScaled {
            pos: [-1023, 0, -1],
            rot: [0, 3071, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8554
    spawn(
        0x801FA0F4,
        LiteralScaled {
            pos: [-512, 0, 886],
            rot: [0, 3754, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8554
    spawn(
        0x801FA158,
        LiteralScaled {
            pos: [-512, 0, 886],
            rot: [0, 3754, 0],
            scale: 0x1000,
        },
    ),
];
const S923_20: [ModuleSpawn; 7] = [
    // 0x801F8680
    spawn(
        0x801FA1AC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x800,
        },
    ),
    // 0x801F8698
    spawn(
        0x801FA204,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x800,
        },
    ),
    // 0x801F86B0
    spawn(
        0x801FA25C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x800,
        },
    ),
    // 0x801F86C8
    spawn(
        0x801FA2B4,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x800,
        },
    ),
    // 0x801F86E0
    spawn(
        0x801FA30C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x800,
        },
    ),
    // 0x801F86F8
    spawn(
        0x801FA3AC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x800,
        },
    ),
    // 0x801F8710
    spawn(
        0x801FA3FC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x800,
        },
    ),
];
const S928_2: [ModuleSpawn; 1] = [
    // 0x801F6E4C
    spawn(
        0x801F9208,
        LiteralScaled {
            pos: [0, 0, 8192],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S928_4: [ModuleSpawn; 5] = [
    // 0x801F7478
    spawn(0x801F9260, Creature),
    // 0x801F74A0
    spawn(0x801F928C, Creature),
    // 0x801F74B8
    spawn(0x801F92E8, Creature),
    // 0x801F74D0
    spawn(0x801F9330, Creature),
    // 0x801F74E8
    spawn(0x801F9378, Creature),
];
const S928_7: [ModuleSpawn; 1] = [
    // 0x801F7958
    spawn(
        0x801F9520,
        LiteralScaled {
            pos: [0, -160, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S928_8: [ModuleSpawn; 1] = [
    // 0x801F7A60
    spawn(
        0x801F9470,
        LiteralScaled {
            pos: [0, 0, 256],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S928_9: [ModuleSpawn; 1] = [
    // 0x801F7AE8
    spawn(
        0x801F9568,
        LiteralScaled {
            pos: [0, 0, 256],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S928_10: [ModuleSpawn; 5] = [
    // 0x801F7B50
    spawn(
        0x801F96E8,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7B74
    spawn(
        0x801F9624,
        LiteralScaled {
            pos: [0, 0, 256],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7B94
    spawn(
        0x801F9688,
        LiteralScaled {
            pos: [0, 0, 8192],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7BB0
    spawn(
        0x801F95CC,
        LiteralScaled {
            pos: [0, 0, 8192],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7BC8
    spawn(
        0x801F95CC,
        LiteralScaled {
            pos: [0, 0, 8192],
            rot: [0, 0, 2048],
            scale: 0x1800,
        },
    ),
];
const S928_12: [ModuleSpawn; 7] = [
    // 0x801F7D8C
    spawn(
        0x801F970C,
        LiteralScaled {
            pos: [0, 0, 8192],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7DA4
    spawn(
        0x801F976C,
        LiteralScaled {
            pos: [0, 0, 8192],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7DBC
    spawn(
        0x801F97B4,
        LiteralScaled {
            pos: [0, 0, 8192],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7DD4
    spawn(
        0x801F9864,
        LiteralScaled {
            pos: [0, 0, 8192],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7DEC
    spawn(
        0x801F990C,
        LiteralScaled {
            pos: [0, 0, 8192],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7E04
    spawn(
        0x801F9A1C,
        LiteralScaled {
            pos: [0, 0, 8192],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7E1C
    spawn(
        0x801F9AA8,
        LiteralScaled {
            pos: [0, 0, 8192],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S928_15: [ModuleSpawn; 5] = [
    // 0x801F8248
    spawn(
        0x801F9D90,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 2048, 0],
            scale: 0x2000,
        },
    ),
    // 0x801F8260
    spawn(
        0x801F9F88,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 2048, 0],
            scale: 0x2000,
        },
    ),
    // 0x801F8278
    spawn(
        0x801F9FCC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 2048, 0],
            scale: 0x2000,
        },
    ),
    // 0x801F8290
    spawn(
        0x801FA010,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 2048, 0],
            scale: 0x2000,
        },
    ),
    // 0x801F82A8
    spawn(
        0x801FA054,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 2048, 0],
            scale: 0x2000,
        },
    ),
];
const S928_17: [ModuleSpawn; 1] = [
    // 0x801F8660
    spawn(
        0x801F9E0C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S928_18: [ModuleSpawn; 1] = [
    // 0x801F87B8
    spawn(
        0x801F9DB4,
        LiteralScaled {
            pos: [0, -512, 0],
            rot: [0, 0, 0],
            scale: 0x2000,
        },
    ),
];
const S929_3: [ModuleSpawn; 1] = [
    // 0x801F6F18
    spawn(
        0x801F917C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S929_4: [ModuleSpawn; 1] = [
    // 0x801F6FB0
    spawn(
        0x801F9194,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S929_12: [ModuleSpawn; 3] = [
    // 0x801F77FC
    spawn(
        0x801F91C0,
        CreatureOffset {
            dx: 0,
            y: -0x1E0,
            dz: 0,
            rot: [0, 0x800, 0],
        },
    ),
    // 0x801F7818
    spawn(
        0x801F9220,
        CreatureOffset {
            dx: 0,
            y: -0x1E0,
            dz: 0,
            rot: [0, 0x800, 0],
        },
    ),
    // 0x801F7838
    spawn(
        0x801F9284,
        CreatureOffset {
            dx: 0,
            y: -0x1E0,
            dz: 0,
            rot: [0, 0x800, 0],
        },
    ),
];
const S929_14: [ModuleSpawn; 6] = [
    // 0x801F79C8
    spawn(
        0x801F93B8,
        LiteralScaled {
            pos: [0, 1024, 2048],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F79E0
    spawn(
        0x801F9468,
        LiteralScaled {
            pos: [0, 1024, 2048],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7A00
    spawn(
        0x801F9498,
        CreatureOffset {
            dx: 0,
            y: 0,
            dz: 0,
            rot: [0, 0, 0],
        },
    ),
    // 0x801F7A1C
    spawn(
        0x801F94E0,
        CreatureOffset {
            dx: 0,
            y: 0,
            dz: 0,
            rot: [0, 0, 0],
        },
    ),
    // 0x801F7A38
    spawn(
        0x801F9528,
        CreatureOffset {
            dx: 0,
            y: 0,
            dz: 0,
            rot: [0, 0, 0],
        },
    ),
    // 0x801F7A54
    spawn(
        0x801F95A8,
        CreatureOffset {
            dx: 0,
            y: 0,
            dz: 0,
            rot: [0, 0, 0],
        },
    ),
];
const S929_15: [ModuleSpawn; 4] = [
    // 0x801F7C40
    spawn(
        0x801F99EC,
        CreatureOffset {
            dx: 0,
            y: 0,
            dz: 0,
            rot: [0, 0, 0],
        },
    ),
    // 0x801F7C80
    spawn(
        0x801F9344,
        LiteralScaled {
            pos: [0, 0, 2048],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7C80
    spawn(
        0x801F9344,
        LiteralScaled {
            pos: [0, 0, 4096],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7CB8
    spawn(
        0x801F9398,
        LiteralScaled {
            pos: [0, 0, 6144],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S929_17: [ModuleSpawn; 5] = [
    // 0x801F8568
    spawn(
        0x801F96C0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8588
    spawn(
        0x801F9738,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F85A0
    spawn(
        0x801F9808,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F85B8
    spawn(
        0x801F9868,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F85D0
    spawn(
        0x801F98AC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S930_0: [ModuleSpawn; 1] = [
    // 0x801F6B20
    spawn(
        0x801F857C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S930_3: [ModuleSpawn; 6] = [
    // 0x801F6DBC
    spawn(
        0x801F86BC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6DD4
    spawn(
        0x801F8728,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6DEC
    spawn(
        0x801F8794,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E04
    spawn(
        0x801F8800,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E1C
    spawn(
        0x801F886C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E34
    spawn(
        0x801F88D8,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S930_4: [ModuleSpawn; 8] = [
    // 0x801F6FF8
    spawn(
        0x801F8964,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7010
    spawn(
        0x801F89B8,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7028
    spawn(
        0x801F8A0C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7040
    spawn(
        0x801F8A60,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7058
    spawn(
        0x801F8AB8,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7070
    spawn(
        0x801F8B0C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7088
    spawn(
        0x801F8B68,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F70A0
    spawn(
        0x801F8BC4,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S930_6: [ModuleSpawn; 4] = [
    // 0x801F717C
    spawn(
        0x801F8C44,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F71E8
    spawn(
        0x801F8CA0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7254
    spawn(
        0x801F8C44,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F72C0
    spawn(
        0x801F8CA0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S930_8: [ModuleSpawn; 2] = [
    // 0x801F7550
    spawn(
        0x801F8CFC,
        LiteralScaled {
            pos: [0, 0, -2048],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7568
    spawn(
        0x801F8D50,
        LiteralScaled {
            pos: [0, 0, -2048],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S930_9: [ModuleSpawn; 7] = [
    // 0x801F7620
    spawn(
        0x801F8DAC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7638
    spawn(
        0x801F8E00,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7650
    spawn(
        0x801F8E54,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7668
    spawn(
        0x801F8E7C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7680
    spawn(
        0x801F8FC4,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7698
    spawn(
        0x801F9018,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F76B0
    spawn(
        0x801F8F44,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S930_10: [ModuleSpawn; 16] = [
    // 0x801F77E8
    spawn(
        0x801F8EE0,
        LiteralScaled {
            pos: [0, 0, -400],
            rot: [0, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7800
    spawn(
        0x801F8A0C,
        LiteralScaled {
            pos: [0, 0, -400],
            rot: [0, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7818
    spawn(
        0x801F9124,
        LiteralScaled {
            pos: [0, 0, -400],
            rot: [0, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7830
    spawn(
        0x801F916C,
        LiteralScaled {
            pos: [0, 0, -400],
            rot: [0, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7898
    spawn(0x801F92D8, PartySeat(0)),
    // 0x801F7898
    spawn(0x801F9284, PartySeat(0)),
    // 0x801F7898
    spawn(0x801F9238, PartySeat(0)),
    // 0x801F7898
    spawn(0x801F91E4, PartySeat(0)),
    // 0x801F7898
    spawn(0x801F92D8, PartySeat(1)),
    // 0x801F7898
    spawn(0x801F9284, PartySeat(1)),
    // 0x801F7898
    spawn(0x801F9238, PartySeat(1)),
    // 0x801F7898
    spawn(0x801F91E4, PartySeat(1)),
    // 0x801F7898
    spawn(0x801F92D8, PartySeat(2)),
    // 0x801F7898
    spawn(0x801F9284, PartySeat(2)),
    // 0x801F7898
    spawn(0x801F9238, PartySeat(2)),
    // 0x801F7898
    spawn(0x801F91E4, PartySeat(2)),
];
const S931_3: [ModuleSpawn; 4] = [
    // 0x801F6D88
    spawn(
        0x801F8B68,
        LiteralScaled {
            pos: [0, -896, -1536],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6DAC
    spawn(
        0x801F8BC0,
        LiteralScaled {
            pos: [0, -896, -1536],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6DC8
    spawn(
        0x801F8C18,
        LiteralScaled {
            pos: [0, -896, -1536],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6DE4
    spawn(
        0x801F8C70,
        LiteralScaled {
            pos: [0, -896, -1536],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S931_4: [ModuleSpawn; 1] = [
    // 0x801F6EF0
    spawn(
        0x801F8CD8,
        CreatureOffset {
            dx: 0,
            y: -0x380,
            dz: 0,
            rot: [0, 0, 0],
        },
    ),
];
const S931_5_EVERY: [ModuleSpawn; 1] = [
    // 0x801F7028
    spawn(
        0x801F8E20,
        CreatureOffset {
            dx: 0,
            y: -0x600,
            dz: 0x180,
            rot: [0, 0, 0],
        },
    ),
];
const S931_6_EVERY: [ModuleSpawn; 1] = [
    // 0x801F71BC
    spawn(
        0x801F8E20,
        CreatureOffset {
            dx: 0,
            y: -0x600,
            dz: 0x180,
            rot: [0, 0, 0],
        },
    ),
];
const S931_7: [ModuleSpawn; 2] = [
    // 0x801F745C
    spawn(
        0x801F8D44,
        LiteralScaled {
            pos: [0, 0, 1536],
            rot: [0, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7474
    spawn(
        0x801F8DA4,
        LiteralScaled {
            pos: [0, 0, 1536],
            rot: [0, 2048, 0],
            scale: 0x1000,
        },
    ),
];
const S931_7_EVERY: [ModuleSpawn; 1] = [
    // 0x801F7340
    spawn(
        0x801F8E20,
        CreatureOffset {
            dx: 0,
            y: -0x600,
            dz: 0x180,
            rot: [0, 0, 0],
        },
    ),
];
const S931_13: [ModuleSpawn; 3] = [
    // 0x801F77E8
    spawn(
        0x801F8F08,
        LiteralScaled {
            pos: [0, 0, 1024],
            rot: [-128, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7800
    spawn(
        0x801F8F70,
        LiteralScaled {
            pos: [0, 0, 1024],
            rot: [-128, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7818
    spawn(
        0x801F8FC8,
        LiteralScaled {
            pos: [0, 0, 1024],
            rot: [-128, 2048, 0],
            scale: 0x1000,
        },
    ),
];
const S931_14: [ModuleSpawn; 1] = [
    // 0x801F7874
    spawn(
        0x801F9030,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S931_15: [ModuleSpawn; 1] = [
    // 0x801F7920
    spawn(
        0x801F90AC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S931_16: [ModuleSpawn; 4] = [
    // 0x801F79E0
    spawn(0x801F923C, Creature),
    // 0x801F79F8
    spawn(0x801F9290, Creature),
    // 0x801F7A10
    spawn(0x801F92DC, Creature),
    // 0x801F7A28
    spawn(0x801F9338, Creature),
];
const S931_19: [ModuleSpawn; 1] = [
    // 0x801F7B60
    spawn(
        0x801F9580,
        LiteralScaled {
            pos: [0, 0, 1024],
            rot: [64, 1952, 0],
            scale: 0x1000,
        },
    ),
];
const S931_22: [ModuleSpawn; 1] = [
    // 0x801F7CAC
    spawn(
        0x801F9104,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S931_26: [ModuleSpawn; 2] = [
    // 0x801F8160
    spawn(
        0x801F9134,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8178
    spawn(
        0x801F9188,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S931_29: [ModuleSpawn; 10] = [
    // 0x801F8310
    spawn(
        0x801F91D4,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8328
    spawn(
        0x801F9394,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F835C
    spawn(
        0x801F93E0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x800,
        },
    ),
    // 0x801F8370
    spawn(
        0x801F93E0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x400,
        },
    ),
    // 0x801F838C
    spawn(
        0x801F9448,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x800,
        },
    ),
    // 0x801F83A0
    spawn(
        0x801F9448,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x400,
        },
    ),
    // 0x801F83BC
    spawn(
        0x801F94B0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x800,
        },
    ),
    // 0x801F83D0
    spawn(
        0x801F94B0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x400,
        },
    ),
    // 0x801F83EC
    spawn(
        0x801F9518,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x800,
        },
    ),
    // 0x801F8400
    spawn(
        0x801F9518,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x400,
        },
    ),
];
const S932_3: [ModuleSpawn; 5] = [
    // 0x801F6E14
    spawn(
        0x801F875C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E2C
    spawn(
        0x801F87B4,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E44
    spawn(
        0x801F8814,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E5C
    spawn(
        0x801F886C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E74
    spawn(
        0x801F88C4,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S932_5: [ModuleSpawn; 1] = [
    // 0x801F6FD4
    spawn(
        0x801F84DC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S932_6: [ModuleSpawn; 5] = [
    // 0x801F70C8
    spawn(
        0x801F891C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F70E0
    spawn(
        0x801F8974,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F70F8
    spawn(
        0x801F89CC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7110
    spawn(
        0x801F8A2C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7128
    spawn(
        0x801F8A84,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S932_7: [ModuleSpawn; 24] = [
    // 0x801F71FC
    spawn(
        0x801F8548,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7220
    spawn(
        0x801F85A4,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F723C
    spawn(
        0x801F8600,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7254
    spawn(
        0x801F861C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F8638,
        LiteralScaled {
            pos: [176, 0, 1237],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F86A0,
        LiteralScaled {
            pos: [176, 0, 1237],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F8638,
        LiteralScaled {
            pos: [977, 0, 977],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F86A0,
        LiteralScaled {
            pos: [977, 0, 977],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F8638,
        LiteralScaled {
            pos: [1474, 0, 294],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F86A0,
        LiteralScaled {
            pos: [1474, 0, 294],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F8638,
        LiteralScaled {
            pos: [1475, 0, -548],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F86A0,
        LiteralScaled {
            pos: [1475, 0, -548],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F8638,
        LiteralScaled {
            pos: [979, 0, -1232],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F86A0,
        LiteralScaled {
            pos: [979, 0, -1232],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F8638,
        LiteralScaled {
            pos: [176, 0, -1493],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F86A0,
        LiteralScaled {
            pos: [176, 0, -1493],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F8638,
        LiteralScaled {
            pos: [-625, 0, -1233],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F86A0,
        LiteralScaled {
            pos: [-625, 0, -1233],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F8638,
        LiteralScaled {
            pos: [-1122, 0, -550],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F86A0,
        LiteralScaled {
            pos: [-1122, 0, -550],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F8638,
        LiteralScaled {
            pos: [-1123, 0, 292],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F86A0,
        LiteralScaled {
            pos: [-1123, 0, 292],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F8638,
        LiteralScaled {
            pos: [-627, 0, 976],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7308
    spawn(
        0x801F86A0,
        LiteralScaled {
            pos: [-627, 0, 976],
            rot: [256, 2048, 0],
            scale: 0x1000,
        },
    ),
];
const S932_15: [ModuleSpawn; 1] = [
    // 0x801F799C
    spawn(
        0x801F8AE4,
        LiteralScaled {
            pos: [0, 0, 448],
            rot: [-256, 2048, 0],
            scale: 0x1000,
        },
    ),
];
const S932_20: [ModuleSpawn; 4] = [
    // 0x801F7D94
    spawn(
        0x801F8C70,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7DAC
    spawn(
        0x801F8CC8,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7DC8
    spawn(
        0x801F8D20,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7DDC
    spawn(
        0x801F8D20,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S932_21: [ModuleSpawn; 4] = [
    // 0x801F80D8
    spawn(
        0x801F8D78,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x400,
        },
    ),
    // 0x801F80F0
    spawn(
        0x801F8DE4,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x400,
        },
    ),
    // 0x801F8108
    spawn(
        0x801F8E50,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x400,
        },
    ),
    // 0x801F8124
    spawn(
        0x801F8EBC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x400,
        },
    ),
];
const S933_2: [ModuleSpawn; 1] = [
    // 0x801F6DB8
    spawn(
        0x801F881C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S933_3: [ModuleSpawn; 3] = [
    // 0x801F6E38
    spawn(
        0x801F89A8,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E5C
    spawn(
        0x801F8A14,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E78
    spawn(
        0x801F8A80,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S933_5: [ModuleSpawn; 5] = [
    // 0x801F700C
    spawn(
        0x801F8AEC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7030
    spawn(
        0x801F8B40,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F704C
    spawn(
        0x801F8B94,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7068
    spawn(
        0x801F8BE8,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7084
    spawn(
        0x801F8C3C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S933_7: [ModuleSpawn; 1] = [
    // 0x801F730C
    spawn(
        0x801F8E5C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S933_8: [ModuleSpawn; 2] = [
    // 0x801F74B4
    spawn(
        0x801F8C90,
        LiteralScaled {
            pos: [0, 0, 1555],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F74CC
    spawn(
        0x801F8D08,
        LiteralScaled {
            pos: [0, 0, 1555],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S933_10: [ModuleSpawn; 1] = [
    // 0x801F77F8
    spawn(
        0x801F8E5C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S933_11: [ModuleSpawn; 1] = [
    // 0x801F78E8
    spawn(
        0x801F8D80,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S933_12: [ModuleSpawn; 6] = [
    // 0x801F7BD4
    spawn(
        0x801F8E04,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7BEC
    spawn(
        0x801F8EE0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7C04
    spawn(
        0x801F8F30,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7C1C
    spawn(
        0x801F8FDC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7C38
    spawn(
        0x801F8F80,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7C54
    spawn(
        0x801F9020,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S933_17: [ModuleSpawn; 1] = [
    // 0x801F7FF0
    spawn(
        0x801F903C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S933_18: [ModuleSpawn; 1] = [
    // 0x801F80FC
    spawn(
        0x801F9114,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S933_19: [ModuleSpawn; 5] = [
    // 0x801F82BC
    spawn(
        0x801F9180,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F82D4
    spawn(
        0x801F9248,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F82EC
    spawn(
        0x801F92AC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8304
    spawn(
        0x801F92F4,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F831C
    spawn(
        0x801F92D8,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S933_20: [ModuleSpawn; 1] = [
    // 0x801F860C
    spawn(
        0x801F9358,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S934_4: [ModuleSpawn; 19] = [
    // 0x801F6DA0
    spawn(
        0x801F9C08,
        LiteralScaled {
            pos: [0, 0, -1280],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6DB8
    spawn(
        0x801F9C5C,
        LiteralScaled {
            pos: [0, 0, -1280],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6DD0
    spawn(
        0x801F9F10,
        LiteralScaled {
            pos: [0, 0, -1280],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E48
    spawn(
        0x801F9D10,
        LiteralScaled {
            pos: [0, -64, -768],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E48
    spawn(
        0x801F9D70,
        LiteralScaled {
            pos: [0, -64, -768],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E48
    spawn(
        0x801F9D10,
        LiteralScaled {
            pos: [724, -64, -918],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E48
    spawn(
        0x801F9D70,
        LiteralScaled {
            pos: [724, -64, -918],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E48
    spawn(
        0x801F9D10,
        LiteralScaled {
            pos: [1024, -64, -1280],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E48
    spawn(
        0x801F9D70,
        LiteralScaled {
            pos: [1024, -64, -1280],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E48
    spawn(
        0x801F9D10,
        LiteralScaled {
            pos: [724, -64, -1642],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E48
    spawn(
        0x801F9D70,
        LiteralScaled {
            pos: [724, -64, -1642],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E48
    spawn(
        0x801F9D10,
        LiteralScaled {
            pos: [0, -64, -1792],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E48
    spawn(
        0x801F9D70,
        LiteralScaled {
            pos: [0, -64, -1792],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E48
    spawn(
        0x801F9D10,
        LiteralScaled {
            pos: [-724, -64, -1642],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E48
    spawn(
        0x801F9D70,
        LiteralScaled {
            pos: [-724, -64, -1642],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E48
    spawn(
        0x801F9D10,
        LiteralScaled {
            pos: [-1024, -64, -1280],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E48
    spawn(
        0x801F9D70,
        LiteralScaled {
            pos: [-1024, -64, -1280],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E48
    spawn(
        0x801F9D10,
        LiteralScaled {
            pos: [-724, -64, -918],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F6E48
    spawn(
        0x801F9D70,
        LiteralScaled {
            pos: [-724, -64, -918],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S934_10: [ModuleSpawn; 1] = [
    // 0x801F78F4
    spawn(
        0x801FA0F0,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S934_13: [ModuleSpawn; 3] = [
    // 0x801F79FC
    spawn(
        0x801F9FC4,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7A14
    spawn(
        0x801FA028,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7A2C
    spawn(
        0x801FA08C,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S934_14: [ModuleSpawn; 1] = [
    // 0x801F7AA4
    spawn(0x801FA110, Creature),
];
const S934_15: [ModuleSpawn; 12] = [
    // 0x801F7B84
    spawn(0x801FA170, Creature),
    // 0x801F7BA0
    spawn(
        0x801FA244,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7BB8
    spawn(
        0x801FA2AC,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7BD0
    spawn(
        0x801FA304,
        LiteralScaled {
            pos: [0, 0, 0],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7C50
    spawn(
        0x801FA1BC,
        LiteralScaled {
            pos: [0, -2048, -4096],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F7C50
    spawn(
        0x801FA1BC,
        LiteralScaled {
            pos: [724, -1748, -4096],
            rot: [0, 0, 512],
            scale: 0x1000,
        },
    ),
    // 0x801F7C50
    spawn(
        0x801FA1BC,
        LiteralScaled {
            pos: [1024, -1024, -4096],
            rot: [0, 0, 1024],
            scale: 0x1000,
        },
    ),
    // 0x801F7C50
    spawn(
        0x801FA1BC,
        LiteralScaled {
            pos: [724, -300, -4096],
            rot: [0, 0, 1536],
            scale: 0x1000,
        },
    ),
    // 0x801F7C50
    spawn(
        0x801FA1BC,
        LiteralScaled {
            pos: [0, 0, -4096],
            rot: [0, 0, 2048],
            scale: 0x1000,
        },
    ),
    // 0x801F7C50
    spawn(
        0x801FA1BC,
        LiteralScaled {
            pos: [-724, -300, -4096],
            rot: [0, 0, 2560],
            scale: 0x1000,
        },
    ),
    // 0x801F7C50
    spawn(
        0x801FA1BC,
        LiteralScaled {
            pos: [-1024, -1024, -4096],
            rot: [0, 0, 3072],
            scale: 0x1000,
        },
    ),
    // 0x801F7C50
    spawn(
        0x801FA1BC,
        LiteralScaled {
            pos: [-724, -1748, -4096],
            rot: [0, 0, 3584],
            scale: 0x1000,
        },
    ),
];
const S934_20: [ModuleSpawn; 19] = [
    // 0x801F81F4
    spawn(
        0x801FA200,
        LiteralScaled {
            pos: [-768, -1536, -1280],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F81F4
    spawn(
        0x801FA200,
        LiteralScaled {
            pos: [768, -1536, -1280],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F81F4
    spawn(
        0x801FA200,
        LiteralScaled {
            pos: [-768, -1536, -1280],
            rot: [0, 512, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F81F4
    spawn(
        0x801FA200,
        LiteralScaled {
            pos: [768, -1536, -1280],
            rot: [0, 512, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F81F4
    spawn(
        0x801FA200,
        LiteralScaled {
            pos: [-768, -1536, -1280],
            rot: [0, 1024, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F81F4
    spawn(
        0x801FA200,
        LiteralScaled {
            pos: [768, -1536, -1280],
            rot: [0, 1024, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F81F4
    spawn(
        0x801FA200,
        LiteralScaled {
            pos: [-768, -1536, -1280],
            rot: [0, 1536, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F81F4
    spawn(
        0x801FA200,
        LiteralScaled {
            pos: [768, -1536, -1280],
            rot: [0, 1536, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F81F4
    spawn(
        0x801FA200,
        LiteralScaled {
            pos: [-768, -1536, -1280],
            rot: [0, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F81F4
    spawn(
        0x801FA200,
        LiteralScaled {
            pos: [768, -1536, -1280],
            rot: [0, 2048, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F81F4
    spawn(
        0x801FA200,
        LiteralScaled {
            pos: [-768, -1536, -1280],
            rot: [0, 2560, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F81F4
    spawn(
        0x801FA200,
        LiteralScaled {
            pos: [768, -1536, -1280],
            rot: [0, 2560, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F81F4
    spawn(
        0x801FA200,
        LiteralScaled {
            pos: [-768, -1536, -1280],
            rot: [0, 3072, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F81F4
    spawn(
        0x801FA200,
        LiteralScaled {
            pos: [768, -1536, -1280],
            rot: [0, 3072, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F81F4
    spawn(
        0x801FA200,
        LiteralScaled {
            pos: [-768, -1536, -1280],
            rot: [0, 3584, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F81F4
    spawn(
        0x801FA200,
        LiteralScaled {
            pos: [768, -1536, -1280],
            rot: [0, 3584, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F828C
    spawn(
        0x801FA320,
        LiteralScaled {
            pos: [0, 0, 256],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8300
    spawn(
        0x801FA36C,
        LiteralScaled {
            pos: [0, 0, 256],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8374
    spawn(
        0x801FA3B8,
        LiteralScaled {
            pos: [0, 0, 256],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S934_21_EVERY: [ModuleSpawn; 3] = [
    // 0x801F8418
    spawn(
        0x801FA320,
        LiteralScaled {
            pos: [0, 0, 256],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F848C
    spawn(
        0x801FA36C,
        LiteralScaled {
            pos: [0, 0, 256],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8500
    spawn(
        0x801FA3B8,
        LiteralScaled {
            pos: [0, 0, 256],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S934_22_EVERY: [ModuleSpawn; 3] = [
    // 0x801F8788
    spawn(
        0x801FA320,
        LiteralScaled {
            pos: [0, 0, 256],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F87FC
    spawn(
        0x801FA36C,
        LiteralScaled {
            pos: [0, 0, 256],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8870
    spawn(
        0x801FA3B8,
        LiteralScaled {
            pos: [0, 0, 256],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S934_23_EVERY: [ModuleSpawn; 3] = [
    // 0x801F8A04
    spawn(
        0x801FA320,
        LiteralScaled {
            pos: [0, 0, 256],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8A78
    spawn(
        0x801FA36C,
        LiteralScaled {
            pos: [0, 0, 256],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8AEC
    spawn(
        0x801FA3B8,
        LiteralScaled {
            pos: [0, 0, 256],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];
const S934_24_EVERY: [ModuleSpawn; 3] = [
    // 0x801F8BEC
    spawn(
        0x801FA320,
        LiteralScaled {
            pos: [0, 0, 256],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8C60
    spawn(
        0x801FA36C,
        LiteralScaled {
            pos: [0, 0, 256],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
    // 0x801F8CD4
    spawn(
        0x801FA3B8,
        LiteralScaled {
            pos: [0, 0, 256],
            rot: [0, 0, 0],
            scale: 0x1000,
        },
    ),
];

/// Every record a summon-creature module seats on **every** pass of an arm
/// ([`creature_arm_spawns`]' [`SpawnTiming::Every`] tables).
pub const CREATURE_PER_PASS_SPAWNS: &[&[ModuleSpawn]] = &[
    &S915_12_EVERY,
    &S931_5_EVERY,
    &S931_6_EVERY,
    &S931_7_EVERY,
    &S934_21_EVERY,
    &S934_22_EVERY,
    &S934_23_EVERY,
    &S934_24_EVERY,
];

/// The slot-B records some module arm seats on every pass it runs, held or
/// not ([`CAPTURE_PER_PASS_SPAWNS`], [`CREATURE_PER_PASS_SPAWNS`]), and the
/// ones a ported stager seats on every call a looping part makes, off
/// `rand()` ([`crate::cast_module_ticks::HORN_SPARKLE_RECORDS`] and the
/// Iota, Palma and Mule volleys). How many of them are
/// live, how old each is and (for PROT 0962's rays and PROT 0930's
/// sparkles) which way each was rolled is the history of the passes the
/// frame driver made, and for PROT 0930 of the side-band wait its arm 2
/// holds on, which a replay at another frame cadence does not reproduce.
pub fn per_pass_spawn_records() -> impl Iterator<Item = u32> {
    super::CAPTURE_PER_PASS_SPAWNS
        .iter()
        .chain(CREATURE_PER_PASS_SPAWNS)
        .flat_map(|t| t.iter())
        .filter_map(|s| match s.record {
            SpawnRecord::Module(va) => Some(va),
            SpawnRecord::BattleProto(_) => None,
        })
        .chain(crate::cast_module_ticks::HORN_SPARKLE_RECORDS)
        .chain(crate::cast_module_ticks::IOTA_VOLLEY_RECORDS)
        .chain(crate::cast_module_ticks::PALMA_VOLLEY_RECORDS)
        .chain(crate::cast_module_ticks::MULE_VOLLEY_RECORDS)
}

/// The spawn calls module `prot_entry`'s arm `phase` makes with `timing`
/// ([`SpawnTiming`]); empty for an arm that makes none.
pub fn creature_arm_spawns(
    prot_entry: u32,
    phase: u8,
    timing: SpawnTiming,
) -> &'static [ModuleSpawn] {
    let every = timing == SpawnTiming::Every;
    match (prot_entry, phase, every) {
        (914, 0x3, false) => &S914_3,
        (914, 0xb, false) => &S914_11,
        (915, 0x3, false) => &S915_3,
        (915, 0x9, false) => &S915_9,
        (915, 0xc, true) => &S915_12_EVERY,
        (917, 0x2, false) => &S917_2,
        (917, 0x6, false) => &S917_6,
        (917, 0xb, false) => &S917_11,
        (917, 0xd, false) => &S917_13,
        (921, 0x3, false) => &S921_3,
        (921, 0x8, false) => &S921_8,
        (921, 0x9, false) => &S921_9,
        (921, 0xa, false) => &S921_10,
        (923, 0x2, false) => &S923_2,
        (923, 0x4, false) => &S923_4,
        (923, 0x8, false) => &S923_8,
        (923, 0xa, false) => &S923_10,
        (923, 0xc, false) => &S923_12,
        (923, 0xe, false) => &S923_14,
        (923, 0x13, false) => &S923_19,
        (923, 0x14, false) => &S923_20,
        (928, 0x2, false) => &S928_2,
        (928, 0x4, false) => &S928_4,
        (928, 0x7, false) => &S928_7,
        (928, 0x8, false) => &S928_8,
        (928, 0x9, false) => &S928_9,
        (928, 0xa, false) => &S928_10,
        (928, 0xc, false) => &S928_12,
        (928, 0xf, false) => &S928_15,
        (928, 0x11, false) => &S928_17,
        (928, 0x12, false) => &S928_18,
        (929, 0x3, false) => &S929_3,
        (929, 0x4, false) => &S929_4,
        (929, 0xc, false) => &S929_12,
        (929, 0xe, false) => &S929_14,
        (929, 0xf, false) => &S929_15,
        (929, 0x11, false) => &S929_17,
        (930, 0x0, false) => &S930_0,
        (930, 0x3, false) => &S930_3,
        (930, 0x4, false) => &S930_4,
        (930, 0x6, false) => &S930_6,
        (930, 0x8, false) => &S930_8,
        (930, 0x9, false) => &S930_9,
        (930, 0xa, false) => &S930_10,
        (931, 0x3, false) => &S931_3,
        (931, 0x4, false) => &S931_4,
        (931, 0x5, true) => &S931_5_EVERY,
        (931, 0x6, true) => &S931_6_EVERY,
        (931, 0x7, false) => &S931_7,
        (931, 0x7, true) => &S931_7_EVERY,
        (931, 0xd, false) => &S931_13,
        (931, 0xe, false) => &S931_14,
        (931, 0xf, false) => &S931_15,
        (931, 0x10, false) => &S931_16,
        (931, 0x13, false) => &S931_19,
        (931, 0x16, false) => &S931_22,
        (931, 0x1a, false) => &S931_26,
        (931, 0x1d, false) => &S931_29,
        (932, 0x3, false) => &S932_3,
        (932, 0x5, false) => &S932_5,
        (932, 0x6, false) => &S932_6,
        (932, 0x7, false) => &S932_7,
        (932, 0xf, false) => &S932_15,
        (932, 0x14, false) => &S932_20,
        (932, 0x15, false) => &S932_21,
        (933, 0x2, false) => &S933_2,
        (933, 0x3, false) => &S933_3,
        (933, 0x5, false) => &S933_5,
        (933, 0x7, false) => &S933_7,
        (933, 0x8, false) => &S933_8,
        (933, 0xa, false) => &S933_10,
        (933, 0xb, false) => &S933_11,
        (933, 0xc, false) => &S933_12,
        (933, 0x11, false) => &S933_17,
        (933, 0x12, false) => &S933_18,
        (933, 0x13, false) => &S933_19,
        (933, 0x14, false) => &S933_20,
        (934, 0x4, false) => &S934_4,
        (934, 0xa, false) => &S934_10,
        (934, 0xd, false) => &S934_13,
        (934, 0xe, false) => &S934_14,
        (934, 0xf, false) => &S934_15,
        (934, 0x14, false) => &S934_20,
        (934, 0x15, true) => &S934_21_EVERY,
        (934, 0x16, true) => &S934_22_EVERY,
        (934, 0x17, true) => &S934_23_EVERY,
        (934, 0x18, true) => &S934_24_EVERY,
        _ => &[],
    }
}

/// The PROT entries [`creature_arm_spawns`] covers.
pub const CREATURE_SPAWN_ENTRIES: [u32; 12] =
    [914, 915, 917, 921, 923, 928, 929, 930, 931, 932, 933, 934];
