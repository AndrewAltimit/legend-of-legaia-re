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

/// A spawn whose program places it itself: every record below opens on a
/// `WORLD_SET` and a `RENDER_BANK_SET` (or is camera-locked), so the zeroed
/// stack trio the arm passes is only its seed.
const ZEROED: SpawnAnchor = LiteralScaled {
    pos: [0, 0, 0],
    rot: [0, 0, 0],
    scale: 0x1000,
};

// PROT 0962's `0xA5` body `FUN_801F69D8` (Cort's Ultra Charge), two arms.
// See `ghidra/scripts/funcs/overlay_cast_blade_breath_0962_801f69d8.txt`.
const E962_0: [ModuleSpawn; 4] = [
    // 0x801F6B5C: on the caster's `+0x34` / `+0x44` - the ring caller.
    spawn(0x801F86B8, SpawnAnchor::Caster { y: 0 }),
    // 0x801F6B74 / 0x801F6B8C / 0x801F6BA4: the shot trios; each record
    // waits, then turns itself to zero and locks to the camera.
    spawn(0x801F87F4, ZEROED),
    spawn(0x801F884C, ZEROED),
    spawn(0x801F88A4, ZEROED),
];
/// PROT 0962's three camera-locked shot discs (`E962_0`'s zeroed-trio
/// records, seated once by arm 0): each waits, turns itself to zero, locks
/// to the camera and grows its channel block a vsync at a time. The growth
/// is a clock of the frames since arm 0, which a capture reads back off the
/// part.
pub const ULTRA_CHARGE_SHOT_DISCS: [u32; 3] = [0x801F_87F4, 0x801F_884C, 0x801F_88A4];

// 0x801F6C68: arm 1 seats one ray on every pass, on the zeroed stack trio
// with a random roll `rand() % 0x1000` stored over its `z` angle
// (`0x801F6C34..0x801F6C6C`); the host supplies the roll as the arm point.
const E962_1_EVERY: [ModuleSpawn; 1] = [spawn(0x801F876C, SpawnAnchor::ArmPoint)];

// PROT 0938's `0xB7` body `FUN_801F69EC` (Cort's Mystic Circle).
// See `ghidra/scripts/funcs/overlay_cast_chaos_breath_0938_801f69ec.txt`.
const E938_0: [ModuleSpawn; 2] = [
    // 0x801F6B18 / 0x801F6B30: the circle and the ray caller, both
    // camera-locked by their own programs.
    spawn(0x801F7D5C, ZEROED),
    spawn(0x801F7DC4, ZEROED),
];
// 0x801F6CE0, on the countdown's expiry.
const E938_1: [ModuleSpawn; 1] = [spawn(0x801F7FE8, ZEROED)];
// 0x801F6DFC, on the caster's `+0x34` / `+0x44`.
const E938_2: [ModuleSpawn; 1] = [spawn(0x801F7F6C, SpawnAnchor::Caster { y: 0 })];
// 0x801F6FCC..0x801F7014, on the caster.
const E938_3: [ModuleSpawn; 4] = [
    spawn(0x801F7E3C, SpawnAnchor::Caster { y: 0 }),
    spawn(0x801F7E84, SpawnAnchor::Caster { y: 0 }),
    spawn(0x801F7EC8, SpawnAnchor::Caster { y: 0 }),
    spawn(0x801F7F10, SpawnAnchor::Caster { y: 0 }),
];

// PROT 0940's `0xAC` body `FUN_801F7240` (Cort's Mystic Shield), eight arms
// off the head table; every call is `FUN_80050ED4(caster + 0x34, caster +
// 0x44, record, 0x1000)`.
// See `ghidra/scripts/funcs/overlay_cast_glare_divide_0940_801f7240.txt`.
const CASTER: SpawnAnchor = SpawnAnchor::Caster { y: 0 };
// 0x801F73B4, on the countdown's expiry.
const E940_1: [ModuleSpawn; 1] = [spawn(0x801F82D4, CASTER)];
// 0x801F7514..0x801F755C, on the countdown's expiry.
const E940_3: [ModuleSpawn; 4] = [
    spawn(0x801F8530, CASTER),
    spawn(0x801F83B4, CASTER),
    spawn(0x801F841C, CASTER),
    spawn(0x801F8484, CASTER),
];
// 0x801F748C: arm 2's sparks, every eighth pass.
const E940_2_COUNTED: [ModuleSpawn; 1] = [spawn(0x801F8370, CASTER)];
// 0x801F7628 / 0x801F778C: arms 4 and 5, every eighth pass.
const E940_45_COUNTED: [ModuleSpawn; 1] = [spawn(0x801F84EC, CASTER)];

// PROT 0946's one tick body `FUN_801F69FC` (Zeto's Call Wave `0x55` and Big
// Wave `0x56`; [`super::wave_camera`] has the arms). Big Wave's calls sit on
// the caster's `+0x34` / `+0x44`, Call Wave's one on its `+0x3C` / `+0x44`.
// See `ghidra/scripts/funcs/overlay_cast_call_wave_0946_801f69fc.txt`.
// 0x801F6C48: Call Wave's arm 0 branch, the stager caller.
const E946_0_CALL: [ModuleSpawn; 1] = [spawn(0x801F78CC, SpawnAnchor::CasterBody)];
// 0x801F6F1C: Big Wave's wave caller.
const E946_4: [ModuleSpawn; 1] = [spawn(0x801F7C44, CASTER)];
// 0x801F6F90..0x801F7074, on the countdown's expiry.
const E946_5: [ModuleSpawn; 9] = [
    spawn(0x801F7A50, CASTER),
    spawn(0x801F7AC4, CASTER),
    spawn(0x801F7B60, CASTER),
    spawn(0x801F7BAC, CASTER),
    spawn(0x801F7C84, CASTER),
    spawn(0x801F7CDC, CASTER),
    spawn(0x801F7D34, CASTER),
    spawn(0x801F7AA8, CASTER),
    spawn(0x801F7B0C, CASTER),
];
// 0x801F71E8..0x801F7250, on the countdown's expiry, after the arm sets the
// halt bit on every handle it keeps (`0x801F7174..0x801F71A8`).
const E946_6: [ModuleSpawn; 4] = [
    spawn(0x801F7D84, CASTER),
    spawn(0x801F7E4C, CASTER),
    spawn(0x801F7DE8, CASTER),
    spawn(0x801F7EA0, CASTER),
];

// PROT 0944's `0x37` body `FUN_801F6A04` (Cort's Guilty Cross), six arms.
// See `ghidra/scripts/funcs/overlay_cast_guilty_cross_0944_801f6a04.txt`.
// 0x801F6D48 / 0x801F6D60, on the countdown's expiry, on the zeroed stack
// trios: both records open on a `WORLD_SET` and lock to the camera.
const E944_1: [ModuleSpawn; 2] = [spawn(0x801F8098, ZEROED), spawn(0x801F80F0, ZEROED)];
// 0x801F6F84..0x801F6FB4, on the victim's `+0x34` / `+0x44`, after the arm
// flushes the pool (`0x801F6E50`); the third is lifted ([`capture_spawn_pokes`]).
const E944_2: [ModuleSpawn; 3] = [
    spawn(0x801F8148, SpawnAnchor::Victim),
    spawn(0x801F8198, SpawnAnchor::Victim),
    spawn(0x801F81E8, SpawnAnchor::Victim),
];
// 0x801F71F8..0x801F7228, on the victim, as the hit lands.
const E944_3: [ModuleSpawn; 3] = [
    spawn(0x801F8210, SpawnAnchor::Victim),
    spawn(0x801F82E0, SpawnAnchor::Victim),
    spawn(0x801F8320, SpawnAnchor::Victim),
];

// PROT 0961's body `FUN_801F69D8` (Final Crisis / Dead End Crisis), three
// arms. See `ghidra/scripts/funcs/overlay_cast_dead_end_crisis_0961_801f69d8.txt`.
// 0x801F6C64, arm 0: on the caster's `+0x34` / `+0x44`.
const E961_0: [ModuleSpawn; 1] = [spawn(0x801F7AB4, SpawnAnchor::Caster { y: 0 })];
/// The point PROT 0961's arms 2 and 3 seat on: the caster's trio, lowered
/// `0x200` on height and depth when the first formation monster is `0xB5`
/// (`bne v0,s6` at `0x801F6F54`, `bne v1,v0` at `0x801F7168`).
const E961_LOWERED: SpawnAnchor = SpawnAnchor::CasterShifted {
    dy: -0x200,
    dz: -0x200,
};
const E961_LEVEL: SpawnAnchor = SpawnAnchor::CasterShifted { dy: 0, dz: 0 };
const E961_2_RECORDS: [u32; 3] = [0x801F7CA0, 0x801F7D00, 0x801F7D60];
const E961_3_RECORDS: [u32; 11] = [
    0x801F7EDC, 0x801F7F30, 0x801F7F84, 0x801F7FD8, 0x801F8048, 0x801F802C, 0x801F8094, 0x801F80FC,
    0x801F8164, 0x801F81B0, 0x801F81FC,
];
const fn e961(records: [u32; 3], anchor: SpawnAnchor) -> [ModuleSpawn; 3] {
    [
        spawn(records[0], anchor),
        spawn(records[1], anchor),
        spawn(records[2], anchor),
    ]
}
const fn e961_3(anchor: SpawnAnchor) -> [ModuleSpawn; 11] {
    let r = E961_3_RECORDS;
    [
        spawn(r[0], anchor),
        spawn(r[1], anchor),
        spawn(r[2], anchor),
        spawn(r[3], anchor),
        spawn(r[4], anchor),
        spawn(r[5], anchor),
        spawn(r[6], anchor),
        spawn(r[7], anchor),
        spawn(r[8], anchor),
        spawn(r[9], anchor),
        spawn(r[10], anchor),
    ]
}
// 0x801F6F88..0x801F6FB8, arm 2's pass.
const E961_2: [ModuleSpawn; 3] = e961(E961_2_RECORDS, E961_LOWERED);
const E961_2_LEVEL: [ModuleSpawn; 3] = e961(E961_2_RECORDS, E961_LEVEL);
// 0x801F719C..0x801F728C, arm 3's pass, after its pool flush (`0x801F7134`).
const E961_3: [ModuleSpawn; 11] = e961_3(E961_LOWERED);
const E961_3_LEVEL: [ModuleSpawn; 11] = e961_3(E961_LEVEL);
/// The first formation monster PROT 0961's arms 2 and 3 lower their point
/// for.
const E961_LOWERED_FORMATION: u8 = 0xB5;

/// The capture-class entries whose spawns are seated per arm instead of by
/// the pager.
pub const CAPTURE_SPAWN_ENTRIES: [u32; 7] = [938, 940, 944, 946, 961, 962, 966];

/// Halfword stores a capture arm makes into a part it has just seated, as
/// `(actor offset, value)`, applied after the part's seat run: PROT 0944's
/// arm 2 parks the handle of `0x801F81E8` at `0x801F836C` and stores
/// `+0x16 = -0x1200`, `+0x42 = 1` (`0x801F6FBC..0x801F6FD0`).
pub fn capture_spawn_pokes(prot_entry: u32, record: u32) -> &'static [(u16, u16)] {
    match (prot_entry, record) {
        (944, 0x801F_81E8) => &[(0x16, 0xEE00), (0x42, 1)],
        _ => &[],
    }
}

/// A capture arm's spawn made on a pass counted by the body's own frame
/// counter: PROT 0940's word `0x801F8650`, which arms 2, 4 and 5 read and
/// bump once a pass and arms 1 and 3 zero as they pass
/// ([`capture_arm_resets_pass_counter`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CountedArmSpawns {
    /// The spawns go in on a pass whose counter, read before the bump, has
    /// none of these bits set.
    pub mask: u32,
    pub spawns: &'static [ModuleSpawn],
    /// An `efect.dat` pool spawn `FUN_801DFDF0(id, caster + 0x34, caster[+0x46])`
    /// made the same way, `(id, mask)`.
    pub effect: Option<(u8, u32)>,
}

/// The counted spawns capture-class module `prot_entry`'s arm `phase` makes
/// on every pass, held or not:
///
/// | Arm | Counter test | Calls |
/// |---:|---|---|
/// | 0940 2 | `& 3`, then `& 7` (`0x801F7440` / `0x801F7470`) | effect `3` on the caster (`0x801F7460`); `0x801F8370` |
/// | 0940 4, 5 | `& 7` (`0x801F75A4` / `0x801F7708`) | `0x801F84EC` |
pub fn capture_arm_counted(prot_entry: u32, phase: u8) -> Option<CountedArmSpawns> {
    match (prot_entry, phase) {
        (940, 2) => Some(CountedArmSpawns {
            mask: 7,
            spawns: &E940_2_COUNTED,
            effect: Some((3, 3)),
        }),
        (940, 4 | 5) => Some(CountedArmSpawns {
            mask: 7,
            spawns: &E940_45_COUNTED,
            effect: None,
        }),
        _ => None,
    }
}

/// Whether capture-class module `prot_entry`'s arm `phase` zeroes the body's
/// frame counter as it passes: PROT 0940's arms 1 and 3
/// (`sw zero,-0x79B0` at `0x801F73A8` / `0x801F7568`).
pub fn capture_arm_resets_pass_counter(prot_entry: u32, phase: u8) -> bool {
    matches!((prot_entry, phase), (940, 1 | 3))
}

/// Whether the body `body` of capture-class module `prot_entry` (the
/// trampoline's pick for the queued action, [`crate::cast_module_ticks::capture_tick_body`])
/// has its spawns seated per arm. A multi-body cell keeps the pager
/// placement for the bodies whose arms are not ported: PROT 0938's Chaos
/// Breath and PROT 0962's other bodies.
pub fn capture_spawns_per_arm(prot_entry: u32, body: Option<u32>) -> bool {
    match prot_entry {
        938 => body == Some(0x801F_69EC),
        940 => body == Some(0x801F_7240),
        944 => body == Some(0x801F_6A04),
        946 => true,
        961 => true,
        962 => body == Some(0x801F_69D8),
        966 => true,
        _ => false,
    }
}

/// The capture-class arms that flush the part pool (`jal 0x80050E74`) on
/// their pass, ahead of their own spawns: every part the module seated
/// through `FUN_80050ED4` gets its halt bit. PROT 0938's arms 2 and 3
/// (`0x801F6DE4`, `0x801F6FA4`), PROT 0940's arm 7 (`0x801F7870`) and
/// PROT 0944's arms 2 and 4 (`0x801F6E50`, `0x801F7268`).
///
/// PROT 0946's arm 6 is the same reach by another route: it sets the halt
/// bit on every word of its handle table `0x801F7F30` (`0x801F7174..
/// 0x801F71A8`), which holds the stager's waves, the wave caller and arm 5's
/// nine records - every part a Big Wave cast has seated.
pub fn capture_arm_flushes_pool(prot_entry: u32, phase: u8) -> bool {
    matches!(
        (prot_entry, phase),
        (938, 2 | 3) | (940, 7) | (944, 2 | 4) | (946, 6) | (961, 1 | 3 | 5)
    )
}

/// The spawn calls capture-class module `prot_entry`'s arm `phase` makes on
/// **every** pass it runs, held or not; empty for an arm that makes none.
/// PROT 0962's arm 1 (Ultra Charge's rays).
pub fn capture_arm_spawns_every(prot_entry: u32, phase: u8) -> &'static [ModuleSpawn] {
    match (prot_entry, phase) {
        (962, 1) => &E962_1_EVERY,
        _ => &[],
    }
}

/// Every record a capture-class module seats on **every** pass of an arm
/// ([`capture_arm_spawns_every`]'s tables).
pub const CAPTURE_PER_PASS_SPAWNS: &[&[ModuleSpawn]] = &[&E962_1_EVERY];

/// [`capture_arm_spawns`] for the action the cast runs: PROT 0946's arm 0
/// seats its stager caller on Call Wave's branch only (Big Wave's,
/// `0x801F6B08..0x801F6C34`, seats nothing), the branch
/// [`super::wave_camera`] keys on the action id.
pub fn capture_arm_spawns_for(prot_entry: u32, phase: u8, action: u8) -> &'static [ModuleSpawn] {
    match (prot_entry, phase) {
        (946, 0) if action == 0x56 => &[],
        _ => capture_arm_spawns(prot_entry, phase),
    }
}

/// [`capture_arm_spawns_for`] in a battle whose first formation monster is
/// `first_monster` (`0x8007BD0C`): PROT 0961's arms 2 and 3 seat level with
/// the caster outside the evolved Cort's formation.
pub fn capture_arm_spawns_in(
    prot_entry: u32,
    phase: u8,
    action: u8,
    first_monster: u8,
) -> &'static [ModuleSpawn] {
    match (prot_entry, phase) {
        (961, 2) if first_monster != E961_LOWERED_FORMATION => &E961_2_LEVEL,
        (961, 3) if first_monster != E961_LOWERED_FORMATION => &E961_3_LEVEL,
        _ => capture_arm_spawns_for(prot_entry, phase, action),
    }
}

/// The spawn calls capture-class module `prot_entry`'s arm `phase` makes on
/// its pass; empty for an arm that makes none. A call one action's branch
/// alone makes is listed ([`capture_arm_spawns_for`] filters it).
pub fn capture_arm_spawns(prot_entry: u32, phase: u8) -> &'static [ModuleSpawn] {
    match (prot_entry, phase) {
        (940, 1) => &E940_1,
        (940, 3) => &E940_3,
        (944, 1) => &E944_1,
        (944, 2) => &E944_2,
        (944, 3) => &E944_3,
        (946, 0) => &E946_0_CALL,
        (946, 4) => &E946_4,
        (946, 5) => &E946_5,
        (946, 6) => &E946_6,
        (938, 0) => &E938_0,
        (938, 1) => &E938_1,
        (938, 2) => &E938_2,
        (938, 3) => &E938_3,
        (961, 0) => &E961_0,
        (961, 2) => &E961_2,
        (961, 3) => &E961_3,
        (962, 0) => &E962_0,
        (966, 2) => &E966_2,
        (966, 4) => &E966_4,
        (966, 10) => &E966_10,
        (966, 23) => &E966_23,
        (966, 26) => &E966_26,
        _ => &[],
    }
}
