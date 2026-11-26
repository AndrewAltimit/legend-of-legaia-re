//! The summon modules' cameras and rumble (`cast_module_camera::module_profile`
//! and `cast_module_rumble`): each image run off its slot-B base from arm 0
//! to its Done, pass by pass, against its director - every module whose
//! director carries the whole phase chain, and the paced directors a walk of
//! the director alone can pace ([`PACED`]).
//!
//! Each pass feeds the director what the host feeds it
//! (`World::feed_seru_cam`, the band's seat reads, `keep_row_tr_y`, the
//! creature's clip, cursor and colour word) off the retail RAM the pass opens
//! on, runs the image's tick and compares: whether the arm holds, the phase it
//! leaves for, the tween the builder `FUN_801D829C` starts (its frame count)
//! and where the camera is headed after the pass - the angle, TR and focus
//! globals (`0x8007B790..`, `0x800840B8..`, `0x80089118..`) with the
//! builder's target landed at once, against the director's shot, TR z store,
//! drift and nudge in the band's order. A cut and a straight store of the
//! globals land alike. The rumble words (`0x8007B92C`, `0x8007B930`, the two
//! rates and the hold) are compared with `module_rumble_due`.
//!
//! Every call out of the image returns `0` but the builder (logged and
//! landed), `rand()` (a scripted stream), the bearing (`bearing_12bit_approx`,
//! its own differential), the group aim `FUN_801DCEAC` (run live), the damage
//! wrapper (scripted rolls) and the drive poll `FUN_8003F2B8` (busy for the
//! first passes of the module's poll arm). The
//! battle frame's scalar is the band's and its step `1`, so one pass is one
//! engine tick. Each pass opens with every seat but the creature idle, every
//! node cursor far along, the creature playing what its arms staged and its
//! park reached, and each living monster's tint word up, so the settle, clip
//! and frame waits pass as the host's do once its creature has played. Each
//! module casts on the target kind its spell has in the mid-cast captures.

use crate::common::{disc, scus_cpu};
use legaia_engine_battle_vm::battle_action::bearing_12bit_approx;
use legaia_engine_battle_vm::battle_cam_script::prescale_tr_z;
use legaia_engine_battle_vm::battle_target_group::{GroupAim, MIN_GROUP_EXTENT};
use legaia_engine_cast::cast_module_camera::{
    self as cam, ArmDirection, ModuleCamSeats, ModuleCamState, ModuleSeat, SINGLE_BODY,
    SPEED_SCALAR, puera_crystal_fall,
};
use legaia_engine_cast::cast_module_rumble::{
    MODULE_RUMBLE, RumbleStore, module_rumble_due, ozma_spray,
};
use legaia_engine_system::rumble::BattleRumbleWrite;
use legaia_render_kernels::cast_aluru::{ALURU_BAND_ABR_MODE, aluru_band_prims};
use legaia_render_kernels::screen_prim::ScreenPrim;
use psx_retail_exec::{A0, A1, A2, A3, Cpu, Memory, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const BASE: u32 = 0x801F_69D8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const RECORDS: u32 = 0x801C_9348;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: u32 = 0x400;
const NODES: u32 = 0x8014_0000;
const MONSTER_RECORD: u32 = 0x8015_0000;
const RAND: u32 = 0x8005_6798;
const BUILDER: u32 = 0x801D_829C;
const BEARING: u32 = 0x8001_9B28;
const AIM: u32 = 0x801D_CEAC;
/// The cast damage wrapper the summon modules roll their own hits through.
const WRAPPER: u32 = 0x801D_D0AC;
/// The party's character records (`0x414` each), which the wrapper's
/// finisher reads the cast's spell level from and banks the spell XP on.
const CHAR_RECORDS: u32 = 0x8008_4708;
const CHAR_STRIDE: u32 = 0x414;
/// The magic level a live-wrapper run casts at: under 3, so neither side
/// stages the Seru side effect `FUN_801F3D3C` (stubbed in the walk).
const LIVE_LEVEL: u8 = 2;
const GROUP_LIST: u32 = 0x801F_6978;
/// The part pool, and where the walk seats PROT 0933's whirls: `(pool slot,
/// record, level +0x78)`.
const PART_POOL: u32 = 0x801C_90F0;
const WHIRL_PARTS: u32 = 0x8017_8000;
const TERRA_WHIRLS: [(u32, u32, u16); 2] =
    [(0x10, 0x801F_8838, 0x1000), (0x11, 0x801F_88F0, 0x900)];
/// `AddPrim`, and the scratch the walk hands PROT 0916's band arms for
/// their packets.
const ADD_PRIM: u32 = 0x8003_D2C4;
const SET_DRAW_MODE: u32 = 0x8005_9010;
const PACKETS: u32 = 0x8017_0000;
/// The special-battle word, and the value an arena leg opens it on.
const SPECIAL_WORD: u32 = 0x8007_BAC0;
const ARENA_WORD: u32 = 0x200;
/// `FUN_8004E2F0`, the range poll.
const RANGE: u32 = 0x8004_E2F0;
/// The target-size framing `FUN_801F0348`, which stores `ctx[+0x6D0]`.
const FRAME_SIZE: u32 = 0x801F_0348;
/// The battle's outcome byte the range poll gates on.
const BATTLE_OUTCOME: u32 = 0x8007_BD71;
/// `FUN_8003F2B8`, the drive poll `CdSync(1)` each module's poll arm makes.
const DRIVE_POLL: u32 = 0x8003_F2B8;
/// `FUN_801F19EC`, the battle overlay's creature seat installer each seat
/// arm calls.
const INSTALLER: u32 = 0x801F_19EC;
/// The calls of each module's drive poll the walk answers busy.
const DRIVE_BUSY_CALLS: u32 = 3;

/// The `k`th roll on `seat`: small, middling and past a monster's HP in
/// turn, so the floors, the `HP - 1` clamps and the kills all run. The two
/// past the first four reach the arms that scale a roll before they clamp
/// it: `2` quartered is `0`, which PROT 0915's hits floor at `1`
/// (`0x801F7164..0x801F7170`, `0x801F78F0..0x801F78FC`), and `0x500`
/// quartered (`320`) or taken by two thirds (`853`) still runs past the
/// walk's `300` HP (PROT 0915's arm-7 `HP - 1` clamp at `0x801F7184`,
/// PROT 0908's at `0x801F7794` and its sweep's kill at `0x801F7F04`).
/// The run's frame step turns the cycle, so a module's first roll on a seat
/// is not the same one at every step.
fn scripted_roll(seat: usize, k: u32) -> u32 {
    [0x10, 0x5A, 0x7D, 0x180, 0x2, 0x500][(seat + k as usize + usize::from(walk_step())) % 6]
}
const OUT_CENTRE: u32 = 0x8011_0000;
const OUT_EXTENT: u32 = 0x8011_0100;
const SCALAR: u32 = 0x1F80_037D;
const STEP: u32 = 0x1F80_0393;
const FORMATION: u32 = 0x8007_BD0C;
const ROSTER: u32 = 0x8007_BD10;
/// The rumble words: pulse, level, their rates and the hold
/// (`0x8007B92C`, `0x8007B930`, `gp+0xA30`, `gp+0xA34`, `gp+0xA1C`).
const RUMBLE: [u32; 5] = [
    0x8007_B92C,
    0x8007_B930,
    0x8007_BD48,
    0x8007_BD4C,
    0x8007_BD34,
];
const RUMBLE_SENTINEL: u32 = 0x5A5A_5A5A;
const ANGLES: u32 = 0x8007_B790;
const TR: u32 = 0x8008_40B8;
const FOCUS: u32 = 0x8008_9118;
/// PROT 0934's shake words: the pass word and the rise word.
const OZMA_PASS_WORD: u32 = 0x801F_A408;
const OZMA_RISE_WORD: u32 = 0x801F_A40C;
/// The caster (a party seat) and its victim (the first monster).
const CASTER: u32 = 0;
const VICTIM: u32 = 3;
const CREATURE: u32 = 7;
const MONSTERS: u32 = 2;

/// One tween-builder call, as its arguments name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shot {
    angles: [i16; 3],
    tr: [i16; 3],
    focus: [i16; 3],
    frames: u16,
}

/// Where the camera is headed: the globals as a shot or a store leaves
/// them - pitch, yaw and roll `0x8007B790..`, TR `0x800840B8..` (TR z
/// prescaled) and the raw focus trio `0x80089118..`. Retail's builder is
/// logged and landed at once, so the globals always read the target its
/// stores and drifts act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Target {
    angles: [i32; 3],
    tr: [i32; 3],
    focus: [i32; 3],
}

impl Target {
    fn read(m: &mut Memory) -> Self {
        Self {
            angles: [0, 2, 4].map(|o| i32::from(m.rd16(ANGLES + o) & 0xFFF)),
            tr: [0, 4, 8].map(|o| m.rd32(TR + o) as i32),
            focus: [0, 4, 8].map(|o| i32::from(m.rd16(FOCUS + o) as i16)),
        }
    }

    fn land(&mut self, s: &Shot) {
        self.angles = s.angles.map(|a| i32::from(a) & 0xFFF);
        self.tr = [
            i32::from(s.tr[0]),
            i32::from(s.tr[1]),
            prescale_tr_z(i32::from(s.tr[2])) as i32,
        ];
        self.focus = s.focus.map(i32::from);
    }

    fn write(&self, m: &mut Memory) {
        for i in 0..3u32 {
            m.wr16(ANGLES + 2 * i, self.angles[i as usize] as u16);
            m.wr32(TR + 4 * i, self.tr[i as usize] as u32);
            m.wr16(FOCUS + 4 * i, self.focus[i as usize] as u16);
        }
    }

    fn same(&self, o: &Self) -> bool {
        (0..3).all(|i| (self.angles[i] - o.angles[i]) & 0xFFF == 0)
            && self.tr == o.tr
            && self.focus == o.focus
    }

    /// The port's pass on this target, in the band's order
    /// (`World::summon_stager_tick`): the drift, the shot, the TR z store,
    /// a drift that belongs after the shot, the nudge.
    fn apply(&mut self, arm: &ArmDirection) {
        let drift = |t: &mut Self| {
            if let Some(d) = arm.drift {
                t.angles[0] += i32::from(d.pitch);
                t.angles[1] += i32::from(d.yaw);
                t.tr[0] += i32::from(d.tr_x);
                t.tr[1] += i32::from(d.tr_y);
                t.tr[2] += i32::from(d.tr_z);
            }
        };
        if !arm.drift_after_shot {
            drift(self);
        }
        if let Some(s) = arm.shot {
            self.land(&Shot {
                angles: s.angles,
                tr: s.tr,
                focus: s.focus,
                frames: s.frames,
            });
        }
        if let Some(z) = arm.tr_z_store {
            self.tr[2] = z;
        }
        if arm.drift_after_shot {
            drift(self);
        }
        if let Some(n) = arm.nudge {
            self.angles[0] += i32::from(n.pitch);
            self.tr[0] += i32::from(n.tr_x);
            self.tr[1] += i32::from(n.tr_y);
            self.tr[2] += i32::from(n.tr_z);
        }
        for a in &mut self.angles {
            *a &= 0xFFF;
        }
    }
}

fn trio(m: &mut Memory, a: u32) -> [i16; 3] {
    [m.rd16(a) as i16, m.rd16(a + 2) as i16, m.rd16(a + 4) as i16]
}

fn jal_targets(module: &[u8]) -> Vec<u32> {
    let mut out: Vec<u32> = module
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .filter(|w| w >> 26 == 3)
        .map(|w| ((w & 0x03FF_FFFF) << 2) | 0x8000_0000)
        .filter(|&t| !(BASE..BASE + 0x8000).contains(&t))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn seat_at(m: &mut Memory, slot: u32, display: bool) -> ModuleSeat {
    let a = ACTOR + slot * STRIDE;
    let o = if display { 0x3C } else { 0x34 };
    ModuleSeat {
        x: m.rd16(a + o) as i16,
        y: m.rd16(a + o + 2) as i16,
        z: m.rd16(a + o + 4) as i16,
        facing: m.rd16(a + 0x46) & 0xFFF,
    }
}

/// The summon modules whose director carries the whole phase chain, as
/// `(entry, tick body, the cast's target code)`: the code each spell's own
/// targeting gives it, as the mid-cast captures hold it on the caster's
/// `+0x1DD` (a seat for Mushura, Barra and Aluru, the party for Horn, the
/// monster row for the rest).
const BODIES: [(u32, u32, u8); 14] = [
    (914, GOLA_GOLA, 9),
    (915, MUSHURA, 3),
    (916, ALURU, 3),
    (917, BARRA, 3),
    (920, SLIPPERY, 9),
    (921, IOTA, 9),
    (923, GILIUM, 9),
    (928, PALMA, 9),
    (929, MULE, 9),
    (930, HORN, 8),
    (931, JEDO, 9),
    (932, META, 9),
    (933, TERRA, 9),
    (934, OZMA, 9),
];

/// The directors beside a ported tick body whose arms they pace, as `(entry,
/// tick body, target code)`. Vera heals a party seat. PROT 0924's and 0925's
/// target kind is pinned by no capture; they run on a seat. PROT 0903 runs
/// with its CD word idle and its range poll answered (below); PROT 0918 with
/// its part handles real and its arm `0xE` framing call live; PROT 0919 on
/// the party. The other paced directors (PROT 0904, 0906, 0909..0913, 0922)
/// hand
/// their holds to the tick body's own port and arm a passed arm's shot on
/// the body's next tick (`ModuleProfile::body_paced`), which a walk of the
/// director alone cannot pace.
const PACED: [(u32, u32, u8); 10] = [
    (903, GIMARD, 3),
    (918, KEMARO, 3),
    (919, SPOON, 8),
    (905, VERA, 2),
    (907, NIGHTO, 3),
    (908, ZENOIR, 3),
    (924, ULTIMATE_RAVE, 3),
    (925, SPIKEFISH, 3),
    (927, JUGGERNAUT, 9),
    (922, PUERA, 9),
];

const VERA: u32 = 0x801F_69D8;
const GIMARD: u32 = 0x801F_69D8;
const KEMARO: u32 = 0x801F_6C70;
const SPOON: u32 = 0x801F_69D8;
const ZENOIR: u32 = 0x801F_69D8;
const ULTIMATE_RAVE: u32 = 0x801F_6A18;
const SPIKEFISH: u32 = 0x801F_6A00;
const JUGGERNAUT: u32 = 0x801F_6A84;
const NIGHTO: u32 = 0x801F_69E8;
const GOLA_GOLA: u32 = 0x801F_69F0;
const MUSHURA: u32 = 0x801F_69D8;
const ALURU: u32 = 0x801F_69F8;
const BARRA: u32 = 0x801F_6A30;
const SLIPPERY: u32 = 0x801F_69D8;
const IOTA: u32 = 0x801F_6A08;
const GILIUM: u32 = 0x801F_69D8;
const PALMA: u32 = 0x801F_69F4;
const MULE: u32 = 0x801F_69FC;
const HORN: u32 = 0x801F_6A74;
const JEDO: u32 = 0x801F_6A58;
const META: u32 = 0x801F_6A34;
const TERRA: u32 = 0x801F_6A30;
const OZMA: u32 = 0x801F_6A40;
const PUERA: u32 = 0x801F_6A3C;
/// The part spawn `FUN_80021B04`, the pool the walk hands PROT 0922's parts
/// from, and arm 18's 23 crystal handles (`0x801F9F90 + 0x58`).
const SPAWN_PART: u32 = 0x8002_1B04;
/// The Battles vibration option word: non-zero turns the pad's rumble off.
const VIBRATION_OFF: u32 = 0x8008_45C8;
/// The part pool's wrapper round [`SPAWN_PART`], on the same arguments.
const SPAWN_POOLED: u32 = 0x8005_0ED4;
const PARTS: u32 = 0x8016_0000;
const PUERA_CRYSTALS: u32 = 0x801F_9FE8;
/// PROT 0919's twenty eye-part handles, four records an eye.
const SPOON_EYES: u32 = 0x801F_8E6C;

/// A summon's spell id: PROT 0914 casts `0x8C`, one id an image up.
fn action_of(entry: u32) -> u8 {
    if entry < 914 {
        (0x81 + entry - 903) as u8
    } else {
        (0x8C + entry - 914) as u8
    }
}

/// Where the seats stand for one run: the caster's party seat, the monster
/// count, and each seat's ground pair. The first is the walk's own row; the
/// others are retail's stage seats (`legaia_engine_battle::battle_seats`,
/// mirrored in z to this walk's sides) with the caster on a wing and the
/// victim across the row, so a creature's turn and a camera's drift run
/// both ways.
#[derive(Clone, Copy, Debug)]
struct Layout {
    caster: u32,
    monsters: u32,
    /// The victim seat a seat-target body is cast at.
    victim: u32,
    retail_seats: bool,
}

const LAYOUTS: [Layout; 4] = [
    Layout {
        caster: CASTER,
        monsters: MONSTERS,
        victim: VICTIM,
        retail_seats: false,
    },
    Layout {
        caster: 2,
        monsters: 4,
        victim: 6,
        retail_seats: true,
    },
    Layout {
        caster: 1,
        monsters: 3,
        victim: 3,
        retail_seats: true,
    },
    // A lone monster: the row walks over a row of one, and PROT 0933's
    // orbit runs on the one-monster seat (`0x801F7644..0x801F77B4`).
    Layout {
        caster: 0,
        monsters: 1,
        victim: 3,
        retail_seats: true,
    },
];

impl Layout {
    fn ground(&self, s: u32) -> (i16, i16) {
        use legaia_engine_battle::battle_seats::{monster_seat, party_seat};
        if !self.retail_seats {
            return match s {
                0..=2 => (-0x200 + 0x200 * s as i16, 0x600),
                7 => (0, 0),
                _ => (-0x180 + 0x300 * (s as i16 - 3), -0x600),
            };
        }
        match s {
            0..=2 => {
                let p = party_seat(3, s as usize);
                (p.x, -p.z)
            }
            7 => (0, 0),
            _ => {
                let m = monster_seat(self.monsters as u8, (s - 3) as usize, false);
                (m.x, -m.z)
            }
        }
    }

    /// The target code a body cast on `code` takes here: a monster seat is
    /// this layout's victim, a party seat or a group stays.
    fn code(&self, code: u8) -> u8 {
        if (3..8).contains(&code) {
            self.victim as u8
        } else {
            code
        }
    }
}

thread_local! {
    /// The battle frame step the walk runs at, in vsyncs: retail's
    /// `0x1F800393`, the director's `frame_step` and the shadow's frame
    /// clock. A console's CD timing runs battle at steps above `1`.
    static WALK_STEP: std::cell::Cell<u8> = const { std::cell::Cell::new(1) };
}

fn walk_step() -> u8 {
    WALK_STEP.with(|s| s.get())
}

fn seed(cpu: &mut Cpu, module: &[u8], action: u8, code: u8, layout: Layout) {
    cpu.mem.load(BASE, module);
    for b in 0..0x1100 {
        cpu.mem.wr8(CTX + b, 0);
    }
    cpu.mem.wr8(CTX, 3);
    cpu.mem.wr8(CTX + 1, layout.monsters as u8);
    cpu.mem.wr8(CTX + 0x13, layout.caster as u8);
    cpu.mem.wr16(CTX + 0x6D0, 0x600);
    for s in 0..8u32 {
        let a = ACTOR + s * STRIDE;
        cpu.mem.wr32(ACTORS + s * 4, a);
        for o in (0..STRIDE).step_by(4) {
            cpu.mem.wr32(a + o, 0);
        }
        let live = s < 3 || (3..3 + layout.monsters).contains(&s) || s == CREATURE;
        // A monster seat the formation leaves empty keeps the zeroed record
        // the battle's seating leaves it: its ground and body pairs read
        // `(0, 0)`, which PROT 0915's chase aims at when it trades its
        // target onto such a seat (`0x801F7B04..0x801F7B1C`).
        let (x, z) = if live || s < 3 {
            layout.ground(s)
        } else {
            (0, 0)
        };
        for (o, v) in [(0x34u32, x), (0x38, z), (0x3C, x), (0x40, z)] {
            cpu.mem.wr16(a + o, v as u16);
        }
        cpu.mem.wr16(a + 0x46, if s < 3 { 0x800 } else { 0 });
        cpu.mem.wr16(a + 0x14C, if live { 300 } else { 0 });
        // Max HP above it, so a restore has room (PROT 0905's clamp and
        // PROT 0930's full heal read `+0x14E`).
        cpu.mem.wr16(a + 0x14E, if live { 400 } else { 0 });
        // The word the group aim takes a monster seat to be up by.
        // The seating's tint word on each occupied monster seat
        // (`FUN_800513F0`, `0x80051874`), which the group aim reads as up.
        let up = live && (3..7).contains(&s);
        cpu.mem.wr32(a + 4, if up { 0x1004_0100 } else { 0 });
        cpu.mem.wr16(a + 0x168, 100);
        cpu.mem.wr16(a + 0x16A, 100);
        cpu.mem.wr8(a + 0x1DF, action);
        cpu.mem.wr8(a + 0x21D, 8);
        let node = NODES + s * 0x100;
        cpu.mem.wr32(a + 0x22C, node);
        cpu.mem.wr16(node + 0x58, 0x100);
        cpu.mem.wr16(node + 0x72, 0x1000);
    }
    // The cast's target code, which the creature's installer `FUN_801F19EC`
    // copies onto the creature seat (`0x801F1D98..0x801F1DB8`).
    cpu.mem.wr8(ACTOR + layout.caster * STRIDE + 0x1DD, code);
    cpu.mem.wr8(ACTOR + CREATURE * STRIDE + 0x1DD, code);
    // The rest of what the installer leaves on the creature seat: colour word
    // `-1` (`0x801F1D5C`), tint state `0` (`0x801F1E7C`) and animation rate
    // `2` (`0x801F1DFC`). The host seats the shadow's creature the same way
    // (`World::seat_summon_actor`). Its ground and facing stay the walk's:
    // the camera half frames the creature off the walk's seat, and the
    // shadow syncs both each pass.
    let c = ACTOR + CREATURE * STRIDE;
    cpu.mem.wr32(c + 4, u32::MAX);
    cpu.mem.wr8(c + 0x21C, 0);
    cpu.mem.wr8(c + 0x21D, 2);
    // The monster records `0x801C9348[seat - 3]`: one per seated monster
    // and the creature's (`[4]`); a seat the formation leaves empty holds a
    // null pointer, so a read off it lands in the kernel's low RAM, whose
    // byte `0x1F` reads `0x81` in every retail image of a cast (the size
    // class PROT 0915's reach poll takes for such a seat, `0x8004E394`).
    for i in 0..5 {
        let seated = i < layout.monsters || i == 4;
        cpu.mem
            .wr32(RECORDS + i * 4, if seated { MONSTER_RECORD } else { 0 });
    }
    cpu.mem.wr8(0x1F, 0x81);
    cpu.mem.wr8(SCALAR, SPEED_SCALAR as u8);
    // The battle's outcome byte reads `0xFF` while the fight runs; the range
    // poll `FUN_8004E2F0` answers out of reach on anything else
    // (`0x8004E2F4..0x8004E310`).
    cpu.mem.wr8(BATTLE_OUTCOME, 0xFF);
    cpu.mem.wr8(STEP, walk_step());
    for i in 0..5 {
        cpu.mem
            .wr8(FORMATION + i, if i < layout.monsters { 0x10 } else { 0 });
    }
    for i in 0..3 {
        cpu.mem.wr8(ROSTER + i, 1 + i as u8);
    }
    Target {
        angles: [0x40, 0x800, 0],
        tr: [0, 0x400, 0x1000],
        focus: [0, 0, 0],
    }
    .write(&mut cpu.mem);
}

/// The records a live-wrapper run adds to [`seed`]'s: the caster's character
/// record carrying the cast's spell at [`LIVE_LEVEL`] in its first slot
/// (`+0x13D`, `+0x161`) and no XP, and a spread of attack, defence and
/// intelligence on every seat (`+0x158`, `+0x15C`, `+0x160`, `+0x168`), so
/// the rolls land small and large hits and the spirit and XP tails both run.
fn seed_live(cpu: &mut Cpu, layout: Layout, action: u8) {
    for m in 0..3u32 {
        let rec = CHAR_RECORDS + m * CHAR_STRIDE;
        for b in (0..CHAR_STRIDE).step_by(4) {
            cpu.mem.wr32(rec + b, 0);
        }
    }
    let rec = CHAR_RECORDS + layout.caster * CHAR_STRIDE;
    cpu.mem.wr8(rec + 0x13D, action);
    cpu.mem.wr8(rec + 0x161, LIVE_LEVEL);
    for s in 0..8u32 {
        let a = ACTOR + s * STRIDE;
        let k = (s + layout.monsters) as u16;
        cpu.mem.wr16(a + 0x158, 20 + 17 * k);
        cpu.mem.wr16(a + 0x15C, 5 * k);
        cpu.mem.wr16(a + 0x160, 3 * k);
        cpu.mem.wr16(a + 0x168, 40 + 23 * k);
    }
}

#[test]
fn the_summon_cameras_frame_and_pace_as_retail_does() {
    differential!(
        prot(914, GOLA_GOLA),
        prot(915, MUSHURA),
        prot(916, ALURU),
        prot(917, BARRA),
        prot(920, SLIPPERY),
        prot(921, IOTA),
        prot(923, GILIUM),
        prot(928, PALMA),
        prot(929, MULE),
        prot(930, HORN),
        prot(931, JEDO),
        prot(932, META),
        prot(933, TERRA),
        prot(934, OZMA),
        prot(905, VERA),
        prot(907, NIGHTO),
        prot(908, ZENOIR),
        prot(924, ULTIMATE_RAVE),
        prot(925, SPIKEFISH),
        prot(927, JUGGERNAUT),
        prot(922, PUERA)
    );
    let (Some(_), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = disc.read_entry(898).expect("PROT 0898");
    let mut failures = Vec::new();
    let mut sideband_held = 0usize;
    let (mut spawns_matched, mut spawn_failures) = (0usize, Vec::<String>::new());
    let (mut shadow_passes, mut shadow_left) = (0usize, 0usize);
    let mut creature_passes = 0usize;
    let mut fold_passes = 0usize;
    let mut drive_polls_held = 0usize;
    let mut chase_hits = 0usize;
    let mut shadow_failures = Vec::<String>::new();
    let mut shadow_leaves = Vec::<String>::new();
    let all: Vec<(u32, u32, u8)> = BODIES.iter().chain(&PACED).copied().collect();
    // Each module runs twice: on the full row, then with the second monster
    // seat fallen and faded (`+0x14C = 0`, `+0x04 = 0`), which the row walks
    // (settles, sweeps, shows) skip or wait past.
    // Each runs on every layout.
    // And at frame steps 1 to 4 (`WALK_STEP`).
    // A third run on the full row takes the damage wrapper live on both
    // sides (`live`): retail's `FUN_801DD0AC` and the shadow's
    // `World::summon_module_roll` and fold on the same seat stats and
    // `rand()` stream, so the finisher's spirit and spell XP are compared
    // beside the HP.
    let runs = WALK_STEPS.iter().flat_map(|&step| {
        all.iter().flat_map(move |&b| {
            LAYOUTS.iter().flat_map(move |&l| {
                [
                    (b, false, l, step, false),
                    (b, true, l, step, false),
                    (b, false, l, step, true),
                ]
            })
        })
    });
    // Each mismatch carries the step its run had, and whether it was live.
    let mut marks = [0usize; 3];
    let mut was_live = false;
    let tag = |f: &mut Vec<String>,
               s: &mut Vec<String>,
               sp: &mut Vec<String>,
               m: &mut [usize; 3],
               live: bool| {
        let step = walk_step();
        for (v, k) in [(f, 0), (s, 1), (sp, 2)] {
            for x in &mut v[m[k]..] {
                x.push_str(&format!(" [step {step}]"));
                if live {
                    x.push_str(" [live]");
                }
            }
            m[k] = v.len();
        }
    };
    let mut band_passes = 0usize;
    let mut whirl_passes = 0usize;
    let mut gimard_far_passes = 0usize;
    let (mut spoon_eyes, mut spoon_signs) = (0usize, 0u8);
    let mut live_runs = 0usize;
    let mut live_spirit = 0usize;
    let mut live_xp = 0usize;
    for ((entry, va, code), fallen, layout, step, live) in runs {
        tag(
            &mut failures,
            &mut shadow_failures,
            &mut spawn_failures,
            &mut marks,
            was_live,
        );
        was_live = live;
        live_runs += usize::from(live);
        WALK_STEP.with(|s| s.set(step));
        let code = layout.code(code);
        let module = disc.read_entry(entry as usize).expect("module");
        let action = action_of(entry);
        let profile = cam::module_profile(entry).expect("directed");
        let shots = Rc::new(RefCell::new(Vec::<Shot>::new()));
        let special_word = if live && entry == 931 && step % 2 == 0 {
            ARENA_WORD
        } else {
            0
        };
        let mut cpu = scus_cpu().expect("SCUS");
        cpu.mem.load(OVERLAY_BASE, &overlay);
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
        cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
        for target in jal_targets(&module) {
            // PROT 0915's chase reads the range poll live: its creature
            // stands on the row in turn (below).
            let live_reach = matches!(entry, 915 | 903) && target == RANGE;
            // PROT 0918's arm `0xE` re-frames the camera off the victim's
            // size (`jal 0x801F0348` at `0x801F7EA4`), whose `ctx[+0x6D0]`
            // its later arms read: it runs live, as the host's
            // `cast_target_frame_depth` does beside it.
            let live_reach = live_reach || (entry == 918 && target == FRAME_SIZE);
            if ![AIM, BUILDER, BEARING, RAND, WRAPPER, DRIVE_POLL, INSTALLER].contains(&target)
                && !live_reach
            {
                cpu.stub(target, 0);
            }
        }
        // PROT 0903's arm 11 walks its creature in until the range poll
        // `FUN_8004E2F0(7, victim)` reads zero (`0x801F740C..0x801F7418`):
        // the walk answers out of reach on the arm's first calls, a count
        // keyed on the run, and the shadow's walk-in reads the same answer.
        let gimard_far = Rc::new(std::cell::Cell::new(0u32));
        if entry == 903 {
            let far = gimard_far.clone();
            cpu.hook(RANGE, move |r, _| {
                let left = far.get();
                far.set(left.saturating_sub(1));
                r.r[V0] = u32::from(left > 0);
            });
            gimard_far.set(1 + u32::from(step) + layout.monsters + u32::from(fallen));
        }
        // The drive poll answers busy on its first calls from the module's
        // poll arm (`legaia_engine_battle_vm::battle_cast_cue::module_drive_arms`),
        // as a seek still in flight does, and idle after. The calls are
        // counted, not the passes: an arm that drains its own countdown ahead
        // of the poll (PROT 0923, `bgtz a1` at `0x801F6CAC`, and the arm 2 of
        // 0920, 0921, 0922 and 0924) first calls it once that countdown is
        // out, so its busy passes are a seek outlasting the countdown.
        let drive = legaia_engine_battle_vm::battle_cast_cue::module_drive_arms(&module);
        let drive_busy = Rc::new(std::cell::Cell::new(0u32));
        let busy_calls = Rc::new(std::cell::Cell::new(0u32));
        {
            let busy = drive_busy.clone();
            let calls = busy_calls.clone();
            cpu.hook(DRIVE_POLL, move |r, _| {
                r.r[V0] = busy.get();
                calls.set(calls.get() + busy.get());
            });
        }
        // The seat installer stores on the creature seat what
        // `FUN_801F19EC` stores, but for the node it builds (the walk's
        // stays): the caster's ground trio on `+0x34` and `+0x3C`
        // (`0x801F1E04..0x801F1E40`, off the node `FUN_80024C88` builds on
        // the caster's `+0x34`), the caster's facing `+0x46`
        // (`0x801F1D94`), colour word `-1` and mode word `0`
        // (`0x801F1D5C`, `0x801F1D68`), the caster's target `+0x1DD`
        // (`0x801F1DB8`), clip `+0x1D9 = 0` (`0x801F1DD8`), rate `2`
        // (`0x801F1DFC`), tint state and impact selector `0`
        // (`0x801F1E7C`, `0x801F1E88`), the shatter word `+0x1BA`
        // (`0x801F1E94`) and the committed-id ring `+0x1FB..+0x21B`.
        let installs = Rc::new(std::cell::Cell::new(0u32));
        {
            let installs = installs.clone();
            cpu.hook(INSTALLER, move |_, m| {
                installs.set(installs.get() + 1);
                let caster = ACTOR + u32::from(m.rd8(CTX + 0x13)) * STRIDE;
                let c = ACTOR + CREATURE * STRIDE;
                for o in (0..8).step_by(2) {
                    let v = m.rd16(caster + 0x34 + o);
                    m.wr16(c + 0x34 + o, v);
                    m.wr16(c + 0x3C + o, v);
                }
                m.wr16(c + 0x44, 0);
                let facing = m.rd16(caster + 0x46);
                m.wr16(c + 0x46, facing);
                m.wr32(c + 4, u32::MAX);
                m.wr32(c + 8, 0);
                let target = m.rd8(caster + 0x1DD);
                m.wr8(c + 0x1DD, target);
                m.wr8(c + 0x1D9, 0);
                m.wr8(c + 0x21D, 2);
                m.wr8(c + 0x21C, 0);
                m.wr8(c + 0x21F, 0);
                m.wr16(c + 0x1BA, 0);
                for o in 0..0x20 {
                    m.wr8(c + 0x1FB + o, 0);
                }
            });
        }
        // The damage wrapper `FUN_801DD0AC(_, 7, seat)` answers each call on
        // a seat with the next of that seat's scripted rolls, and the
        // shadow's module rolls take the same (`summon_roll_override`), so
        // each module's own HP arithmetic is compared on equal rolls.
        // A Stoned defender's roll is `0` on both: the wrapper returns
        // `[sp+0x18] - [sp+0x1C]` (`0x801DD4A4`) and its finisher sets the
        // first to the second for a defender with `+0x16E & 4`
        // (`0x801DE2DC..0x801DE2F8`).
        let stone_mask = Rc::new(std::cell::Cell::new(0u8));
        if live {
            legaia_engine_core::world::summon_roll_override::set(None);
        } else {
            let calls = Rc::new(RefCell::new([0u32; 8]));
            cpu.hook(WRAPPER, move |r, m| {
                let seat = (r.r[A2] & 7) as usize;
                let mut c = calls.borrow_mut();
                let stoned = m.rd16(ACTOR + seat as u32 * STRIDE + 0x16E) & 4 != 0;
                r.r[V0] = if stoned {
                    0
                } else {
                    scripted_roll(seat, c[seat])
                };
                c[seat] += 1;
            });
            let calls = RefCell::new([0u32; 8]);
            let stone_mask = stone_mask.clone();
            legaia_engine_core::world::summon_roll_override::set(Some(Box::new(move |seat| {
                let seat = usize::from(seat & 7);
                let mut c = calls.borrow_mut();
                let v = if stone_mask.get() & (1 << seat) != 0 {
                    0
                } else {
                    scripted_roll(seat, c[seat])
                };
                c[seat] += 1;
                v
            })));
        }
        // PROT 0922 keeps the handles of the parts it spawns and moves them
        // (arm 19's crystal re-seat, arm 20's fall): each spawn gets a part.
        // Every part spawn and its pool wrapper is logged as `(record,
        // point, angles)` for the director's spawns.
        // PROT 0916's inline band packets: every `AddPrim(ot, packet)`
        // (`FUN_8003D2C4`) its arms 0x11..0x13 make, as the packet bytes.
        // Its `SetDrawMode(packet, 0, 0, tpage, 0)` (`FUN_80059010`) logs the
        // tpage it is handed.
        let prims = Rc::new(RefCell::new(Vec::<Vec<u8>>::new()));
        let modes = Rc::new(RefCell::new(Vec::<u32>::new()));
        if entry == 916 {
            let prims = prims.clone();
            cpu.hook(ADD_PRIM, move |r, m| {
                prims.borrow_mut().push(m.read(r.r[A1], 0x24));
            });
            let modes = modes.clone();
            cpu.hook(SET_DRAW_MODE, move |r, _| {
                modes.borrow_mut().push(r.r[A3]);
            });
        }
        let spawn_log = Rc::new(RefCell::new(Vec::<(u32, [i16; 3], [i16; 3])>::new()));
        let next = Rc::new(std::cell::Cell::new(PARTS));
        for spawn in [SPAWN_PART, SPAWN_POOLED] {
            let next = next.clone();
            let log = spawn_log.clone();
            cpu.hook(spawn, move |r, m| {
                log.borrow_mut()
                    .push((r.r[A2], trio(m, r.r[A0]), trio(m, r.r[A1])));
                r.r[V0] = 0;
                if matches!(entry, 918 | 919 | 922 | 933) {
                    let at = next.get();
                    next.set(at + 0x80);
                    for o in (0..0x80).step_by(4) {
                        m.wr32(at + o, 0);
                    }
                    r.r[V0] = at;
                }
            });
        }
        {
            let shots = shots.clone();
            cpu.hook(BUILDER, move |r, m| {
                let shot = Shot {
                    angles: trio(m, r.r[A0]),
                    tr: trio(m, r.r[A1]),
                    focus: trio(m, r.r[A2]),
                    frames: r.r[A3] as u16,
                };
                let mut t = Target::read(m);
                t.land(&shot);
                t.write(m);
                shots.borrow_mut().push(shot);
            });
        }
        let stream = Rc::new(std::cell::Cell::new(1u32));
        let draws = Rc::new(RefCell::new(Vec::<u32>::new()));
        {
            let stream = stream.clone();
            let draws = draws.clone();
            cpu.hook(RAND, move |r, _| {
                let n = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
                stream.set(n);
                r.r[V0] = (n >> 16) & 0x7FFF;
                draws.borrow_mut().push(r.r[V0]);
            });
        }
        cpu.hook(BEARING, |r, _| {
            let a = |i: usize| r.r[A0 + i] as i16;
            r.r[V0] = u32::from(bearing_12bit_approx(a(0), a(1), a(2), a(3)));
        });
        // The fallen-seat runs also play with the Battles vibration row off
        // (`0x800845C8 != 0`), which every rumble store tests first.
        let vibration_off = fallen;
        {
            seed(&mut cpu, &module, action, code, layout);
            if live {
                seed_live(&mut cpu, layout, action);
            }
            cpu.mem.wr32(VIBRATION_OFF, u32::from(vibration_off));
            // The even-step live runs of PROT 0931 are an arena leg: the
            // special-battle word `0x8007BAC0` skips the hit arm's three
            // spell XP a kill (`0x801F8668..0x801F8674`) and the finisher's
            // own tail (`0x801DE450`).
            cpu.mem.wr32(SPECIAL_WORD, special_word);
            // PROT 0925's fallen runs have a party member down, whom the
            // flee arm 8 stands back up on 1 HP (`0x801F796C..0x801F7984`).
            if entry == 925 && fallen {
                let s = (layout.caster + 1) % 3;
                cpu.mem.wr16(ACTOR + s * STRIDE + 0x14C, 0);
            }
            // PROT 0931's lone monster stands raised, so arm 26's cut on the
            // row's middle (`-4 * +0x3E`) stands TR y above `0x400` and arm
            // 31's exit leaves it there (`slti 0x400` at 0x801F8894).
            if entry == 931 && layout.monsters == 1 {
                cpu.mem.wr16(ACTOR + 3 * STRIDE + 0x36, (-0x140i16) as u16);
            }
            // A lone monster stands (a fight with none standing is over): its
            // fallen run is the Stone alone.
            if fallen && layout.monsters > 1 {
                let s = if layout.victim + 1 < 3 + layout.monsters {
                    layout.victim + 1
                } else {
                    3
                };
                let a = ACTOR + s * STRIDE;
                cpu.mem.wr16(a + 0x14C, 0);
                cpu.mem.wr32(a + 4, 0);
            }
            // The fallen runs also stand the last monster seat in Stone
            // (`+0x16E = 4`), which the modules' hit, tint and settle loops
            // skip (`andi 4` ahead of each hit).
            let stoned = fallen.then_some(2 + layout.monsters);
            if let Some(s) = stoned {
                cpu.mem.wr16(ACTOR + s * STRIDE + 0x16E, 4);
            }
            stone_mask.set(stoned.map_or(0, |s| 1 << s));
            // The cast's group list `0x801F6978` (four seats, `0xFF` past
            // the last), as the band fills it at the cast: the living
            // monsters. Mule's wave divides a roll by three only for a seat
            // on it (`0x801F7EA8..0x801F7ED0`).
            // The fallen runs leave seat 3 off it, as for a monster that
            // stood back up after the list was filled.
            let living: Vec<u8> = (3..7u32)
                .filter(|&s| cpu.mem.rd16(ACTOR + s * STRIDE + 0x14C) != 0)
                .filter(|&s| !(fallen && s == 3 && layout.monsters > 1))
                .map(|s| s as u8)
                .collect();
            for i in 0..4u32 {
                cpu.mem.wr8(
                    GROUP_LIST + i,
                    living.get(i as usize).copied().unwrap_or(0xFF),
                );
            }
            // Each monster's reaction run `+0x1EF..+0x1F2`, which a hit arm
            // stages from: the flinch, its fallback, the knockdown and the
            // knockdown gate (set on every other seat). PROT 0903's even-step
            // runs give its victim no flinch, so its hit stages the fallback
            // (`0x801F7524..0x801F753C`).
            for s in 3..7u32 {
                let a = ACTOR + s * STRIDE;
                // PROT 0915's even-step runs do the same for its arm-7 hit
                // (`0x801F7228..0x801F7238`).
                let no_flinch =
                    s == 4 || (matches!(entry, 903 | 915) && s == layout.victim && step % 2 == 0);
                cpu.mem
                    .wr8(a + 0x1EF, if no_flinch { 0 } else { 0x10 + s as u8 });
                cpu.mem.wr8(a + 0x1F0, 0x20 + s as u8);
                cpu.mem.wr8(a + 0x1F1, 0x30 + s as u8);
                // The knockdown gate turns with the step, so each seat
                // takes the flinch fork at some steps and the knockdown at
                // others.
                cpu.mem.wr8(a + 0x1F2, ((s + u32::from(step)) & 1) as u8);
            }
            // State `0x34`'s hide loop, which runs before the module's first
            // pass (`0x801E4B30..0x801E4B6C`, `battle_action::summon`): every
            // party seat and every living monster of seats `0..7`, word `0`
            // and the hidden marker.
            for s in 0..7u32 {
                let a = ACTOR + s * STRIDE;
                if s < 3 || cpu.mem.rd16(a + 0x14C) != 0 {
                    cpu.mem.wr32(a + 4, 0);
                    cpu.mem.wr8(a + 0x21C, 0xFF);
                }
            }
            let mut dead_passes = [0usize; 4];
            let mut st = ModuleCamState {
                lead_roster: 1,
                opens_pass: true,
                frame_step: walk_step(),
                victim_settled: true,
                caster_in_reach: true,
                party_settled: true,
                row_settled: true,
                creature_arrived: true,
                creature_in_range: Some(true),
                ..ModuleCamState::default()
            };
            // PROT 0919's caster turned off the axis, so the creature the
            // installer stands on it faces an angle with both a sine and a
            // cosine (its eye parts, `0x801F7164..0x801F76F0`).
            let spoon_facing = (entry == 919)
                .then(|| (0x800 + [0x123i32, -0x2B1, 0x3C7, -0x099][usize::from(step) % 4]) as u16);
            if let Some(f) = spoon_facing {
                cpu.mem.wr16(ACTOR + layout.caster * STRIDE + 0x46, f);
            }
            let caster_seat = seat_at(&mut cpu.mem, layout.caster, false);
            let victim = if code < 8 {
                u32::from(code)
            } else {
                layout.victim
            };
            let mut max_roll = 0;
            let mut rumbles = 0u32;
            let mut crystal_landings = 0usize;
            let mut staged_writes = 0usize;
            let mut bad = 0usize;
            let mut finished = false;
            let mut path = Vec::new();
            // PROT 0922's seat writes (`World::apply_puera_writes`, through
            // `BattleArmDrivers`) on a world seated from the table: the
            // monster row and the creature, synced off retail before each
            // pass and compared after it.
            let mut puera = (entry == 922).then(|| {
                let mut w = legaia_engine_core::world::World::new();
                w.load_party(legaia_save::Party::zeroed(3));
                w.enter_battle(3, layout.monsters as u8);
                w.casting.summon_actor_slot = Some(CREATURE as u8);
                w
            });
            let mut puera_compared = std::collections::BTreeSet::new();
            let mut sideband_passes = 0u32;
            // PROT 0908's and 0928's fallen runs start each clip their arms
            // stage on the creature two passes late, the clip before it
            // still playing: 0908's arms 6 and 7 hold on the clip that plays
            // (`0x801F7328..0x801F7330`, `0x801F754C..0x801F7554`), 0928's
            // on clip 2 (`0x801F7890..0x801F7898`).
            let mut zenoir_clip = (0u8, 0u8);
            let mut park_passes = 0u32;
            let mut mushura_fade = 0u32;
            let mut spent_side_passes = 0u32;
            busy_calls.set(0);
            // The band shadow: a world seated off the same records, the
            // band's module half (`World::drive_summon_module_pass`) run on
            // retail's phase beside each pass, every seat compared after it.
            // So are PROT 0918's: its arm `0x13` lays a victim the spear
            // killed in front of the caster (`0x801F89BC..0x801F8A48`).
            let scripted = matches!(entry, 931 | 918) && fallen;
            // PROT 0918's even-step runs cast on one of the two monsters arm
            // `0x12` stands further back (`0x4B` / `0xB3` in the victim's
            // formation cell, `0x801F86C4..0x801F86E0`).
            let set_back = (entry == 918 && step % 2 == 0 && victim >= 3)
                .then_some([0x4Bu8, 0xB3][usize::from(step / 2) % 2]);
            if let Some(id) = set_back {
                cpu.mem.wr8(FORMATION + victim - 3, id);
            }
            let mut shadow = summon_shadow(&mut cpu, layout, action, live.then_some(&overlay[..]));
            if let (Some(f), Some(w)) = (spoon_facing, shadow.as_mut()) {
                w.actors[layout.caster as usize].battle.facing_angle = f;
            }
            if let (Some(id), Some(w)) = (set_back, shadow.as_mut()) {
                w.actors[victim as usize].battle_monster_id = Some(u16::from(id));
            }
            if let Some(w) = shadow.as_mut()
                && fallen
            {
                w.casting.module_group_seats &= !1;
            }
            // PROT 0931's fallen runs are a scripted fight (`ctx[+0x287]`):
            // its arm 29 rolls the wrapper on every living monster
            // (`0x801F833C..0x801F8548`) where a free fight banishes them.
            if let Some(w) = shadow.as_mut() {
                w.battle.special_word = special_word;
            }
            if let Some(w) = shadow.as_mut()
                && scripted
            {
                w.battle_ctx.scripted_fight = 1;
                w.battle.scripted_fight = true;
            }
            if let (Some(w), Some(s)) = (shadow.as_mut(), stoned) {
                w.battle.status_effects.apply(
                    s as u8,
                    legaia_engine_battle_vm::status_effects::StatusKind::Stone,
                );
            }
            let mut shadow_after: Vec<u32> = Vec::new();
            // Whether each side has seated the creature: the port at the
            // start for a module with no seat arm, retail once its seat arm
            // calls the installer.
            let seat_arm = cam::module_profile(entry).and_then(|p| p.seat_arm);
            let mut port_seated = seat_arm.is_none();
            installs.set(0);
            // The fallen runs stand in for the action SM's band word
            // `ctx[+0x6D8]` on both sides (`battle_action::summon`): `0x34`
            // arms it to `0x78` ahead of the module's first pass
            // (`0x801E4B10..0x801E4B20`), `0x35` drains it by the frame step
            // ahead of each pass (`0x801E4BCC..0x801E4BDC`), clamps a spent
            // word to `1` while the side-band stage `ctx[+0x276]` stands
            // (`0x801E4C60..0x801E4C78`), and leaves for `0x36` once the pass
            // leaves it negative, zeroing it (`0x801E4C84..0x801E4C9C`). A
            // module that keeps its countdown in the word (PROT 0904, 0907,
            // 0908, 0916) shares it; the shadow takes it through the World's
            // own seam (`World::summon_band_word_in` / `_out`). Every
            // side-band arm (PROT 0916's arm 1 steps to its wait arm 2 on a
            // standing word, `0x801F6EF8`) runs on it as in a cast.
            let (mut sm, mut port_sm, mut port_word) = (0x34u8, 0x34u8, 0x78i16);
            if fallen {
                cpu.mem.wr16(CTX + 0x6D8, 0x78);
            }
            if scripted {
                cpu.mem.wr8(CTX + 0x287, 1);
            }
            // PROT 0933's whirl pool: the stager's two whirl records seated
            // in the part pool (`0x801C90F0`), one on a level `+0x78` past
            // `0xA00` and one under it, beside the module's own part
            // (`0x801F941C`), which arms 6 and 7 step over
            // (`0x801F70E0..0x801F7160`, `0x801F724C..0x801F72CC`); the
            // shadow's scene holds the same two, tagged by record.
            // PROT 0903's arm 2 loads the creature's stream once the CD
            // word `0x8007BDB0` reads idle (`-1`, `0x801F6CD4..0x801F6CF4`),
            // then waits on it reading the stream's `7`; the engine holds the
            // creature resident.
            if entry == 903 {
                cpu.mem.wr32(0x8007_BDB0, u32::MAX);
            }
            // PROT 0919's eye poses: the battle context's five pose objects
            // `ctx[+0x6F4 + 12 * eye]` (x / y / z), which arms 4..7 stand the
            // eye parts on, offsets of both signs keyed on the run.
            let spoon_poses: [[i16; 3]; 5] = std::array::from_fn(|eye| {
                let k =
                    (eye as i32 + 1) * 0x97 + i32::from(step) * 0x3B + layout.caster as i32 * 0x51;
                [
                    ((k * 7) % 0x500 - 0x280) as i16,
                    ((k * 3) % 0x300 - 0x200) as i16,
                    ((k * 11) % 0x600 - 0x300) as i16,
                ]
            });
            if entry == 919 {
                for (eye, p) in spoon_poses.iter().enumerate() {
                    for (o, v) in p.iter().enumerate() {
                        cpu.mem
                            .wr16(CTX + 0x6F4 + 12 * eye as u32 + 2 * o as u32, *v as u16);
                    }
                }
            }
            let whirls = entry == 933;
            let mut whirl_retired = [false; 2];
            if whirls {
                for (k, &(slot, _, level)) in TERRA_WHIRLS.iter().enumerate() {
                    let at = WHIRL_PARTS + 0x100 * k as u32;
                    for o in (0..0x80).step_by(4) {
                        cpu.mem.wr32(at + o, 0);
                    }
                    cpu.mem.wr16(at + 0x78, level);
                    cpu.mem.wr32(PART_POOL + 4 * slot, at);
                }
                let own = WHIRL_PARTS + 0x300;
                cpu.mem.wr16(own + 0x78, 0x1000);
                cpu.mem.wr32(PART_POOL + 4 * 0x20, own);
                // Seated as the handle the module keeps: its own spawns
                // (handed objects, as PROT 0922's) overwrite it later.
                cpu.mem.wr32(0x801F_941C, own);
                if let Some(w) = shadow.as_mut() {
                    let offs: Vec<usize> = TERRA_WHIRLS
                        .iter()
                        .map(|&(_, rec, _)| (rec - BASE) as usize)
                        .collect();
                    let parts =
                        legaia_overlay_images::summon_overlay::parse_records_at(&module, &offs);
                    let mut scene = legaia_engine_effects::summon::SummonScene::spawn_parts_tagged(
                        &parts, &module, 0, [0; 3], BASE,
                    );
                    for (p, &(_, _, level)) in scene.parts.iter_mut().zip(&TERRA_WHIRLS) {
                        p.state.field_78 = level;
                        p.state.field_7a = 0;
                        p.state.world_y = 0;
                    }
                    assert_eq!(scene.parts.len(), 2, "the two whirls seat");
                    w.casting.active_summon = Some(scene);
                }
            }
            for pass in 0..20_000u32 {
                let far_pass = cpu.mem.rd8(CTX + 0x279) == 11 && gimard_far.get() > 0;
                gimard_far_passes += usize::from(far_pass);
                st.creature_arrived = !far_pass;
                if fallen {
                    let step = walk_step() as i16;
                    let staged = cpu.mem.rd8(CTX + 0x276) != 0;
                    let drain = |w: i16| {
                        let w = w.wrapping_sub(step);
                        if w < 0 && staged { 1 } else { w }
                    };
                    if sm == 0x35 {
                        let w = drain(cpu.mem.rd16(CTX + 0x6D8) as i16);
                        cpu.mem.wr16(CTX + 0x6D8, w as u16);
                    }
                    if port_sm == 0x35 {
                        port_word = drain(port_word);
                    }
                }
                // Every seat idle and far along its clip, its display pair on its
                // ground pair as the draw pass leaves it.
                for s in 0..8u32 {
                    let a = ACTOR + s * STRIDE;
                    // The creature plays what its arms stage at once; every
                    // other seat stands idle.
                    // The per-frame tint tween `FUN_80050120` walks a living
                    // monster's word back up from a fade's zero.
                    if (3..7).contains(&s)
                        && cpu.mem.rd16(a + 0x14C) != 0
                        && cpu.mem.rd32(a + 4) == 0
                        && cpu.mem.rd8(a + 0x21C) < 0x0B
                    {
                        cpu.mem.wr32(a + 4, 0x1004_0100);
                    }
                    // A parked clip (`+0x21B` re-armed to `0xFF`) reaches
                    // its park by the next pass; on the fallen runs a pass
                    // later, so the arms that wait on the park (PROT 0928's
                    // `beq 0xFF` at `0x801F79DC`, `0x801F7A78`, `0x801F7B00`)
                    // hold a pass on it.
                    if s == CREATURE && cpu.mem.rd8(a + 0x21B) == 0xFF {
                        park_passes += 1;
                        if !fallen || park_passes > 1 {
                            cpu.mem.wr8(a + 0x21B, 0);
                            park_passes = 0;
                        }
                    }
                    let staged = cpu.mem.rd8(a + 0x1DA);
                    // On the fallen runs PROT 0933's creature is still on clip
                    // `4` through arm `0x12`, whose pass stages `5` on it again
                    // (`0x801F80BC..0x801F80D0`).
                    let lag = entry == 933
                        && fallen
                        && s == CREATURE
                        && staged == 5
                        && cpu.mem.rd8(CTX + 0x279) == 0x12;
                    // PROT 0915's chase skips a monster still in a clip
                    // (`lbu 0x1d9` / `bnez` at `0x801F7870..0x801F7878`):
                    // each monster seat plays its staged clip a pass in
                    // four of the arm's.
                    let chase_clip = entry == 915
                        && (3..7).contains(&s)
                        && cpu.mem.rd8(CTX + 0x279) == 0xC
                        && (pass + s) % 4 == 0;
                    let playing = match (s == CREATURE, lag) {
                        (false, _) if chase_clip => staged.max(1),
                        (true, true) => 4,
                        (true, false) if matches!(entry, 908 | 928) && fallen => {
                            let (playing, late) = &mut zenoir_clip;
                            if staged != *playing {
                                if *late == 0 {
                                    *late = 2;
                                } else {
                                    *late -= 1;
                                    if *late == 0 {
                                        *playing = staged;
                                    }
                                }
                            }
                            *playing
                        }
                        (true, false) => staged,
                        _ => 0,
                    };
                    cpu.mem.wr8(a + 0x1D9, playing);
                    // PROT 0922's arm 8 walks the creature's cursor back from
                    // the `0x1D0` arm 7 parked it on (`0x801F7118..0x801F7178`):
                    // the cursor stays the arm's own.
                    let rewinding = entry == 922 && s == CREATURE && cpu.mem.rd8(CTX + 0x279) == 8;
                    if !rewinding {
                        cpu.mem.wr16(NODES + s * 0x100 + 0x68, 0x7FFF);
                    }
                    // A monster seat the formation left empty is not drawn:
                    // its body pair stays the zeroed record's `(0, 0)`
                    // whatever PROT 0915's arm 0 scatters onto its ground
                    // pair (`0x801F6BC8..0x801F6C58`).
                    let empty = (3 + layout.monsters..7).contains(&s);
                    for o in [0u32, 2, 4] {
                        let v = if empty { 0 } else { cpu.mem.rd16(a + 0x34 + o) };
                        cpu.mem.wr16(a + 0x3C + o, v);
                    }
                }
                let phase = cpu.mem.rd8(CTX + 0x279);
                // The drive poll's busy passes.
                let polls_busy = drive.is_some_and(|d| d.poll_arm == phase)
                    && busy_calls.get() < DRIVE_BUSY_CALLS;
                drive_busy.set(u32::from(polls_busy));
                let calls_before = busy_calls.get();
                // The side-band applier's stage `ctx[+0x276]` stands up for the
                // first passes of the arm that waits on it
                // (`cam::sideband_wait_arm`), as a turn's side-band read does.
                // It stands up again for the first passes the arm opens on
                // its module word spent, where the arms that push behind the
                // word's gate push nothing (`cam::polled_hold`).
                if Some(phase) == cam::sideband_wait_arm(entry) {
                    sideband_passes += 1;
                    if st.countdown.0 <= 0 {
                        spent_side_passes += 1;
                    }
                    // PROT 0923's fallen runs hold it up until arm 1 has
                    // drained its word (`0x200` at `8 * step` a pass), as
                    // `gilium_summon_mid_cast` stands there `56` past it:
                    // the arm then re-enters on a spent word (`blez` at
                    // `0x801F6BF4`).
                    let long =
                        entry == 923 && fallen && spent_side_passes == 0 && sideband_passes <= 80;
                    let up = sideband_passes <= 3 || long || (1..=3).contains(&spent_side_passes);
                    cpu.mem.wr8(CTX + 0x276, if up { 8 } else { 0 });
                    sideband_held += usize::from(up);
                }
                // PROT 0915's arm `0xC` chases the row on the creature's own
                // root motion, which the walk does not play: the creature
                // stands on a monster seat in turn, a few passes on each, so
                // the arm's live reach poll finds the row in and out of reach.
                let mut seeded_bank = None;
                if (entry, phase) == (915, 0xC) {
                    let s = 3 + (pass / 4) % layout.monsters;
                    let (x, z) = layout.ground(s);
                    let c = ACTOR + CREATURE * STRIDE;
                    for (o, v) in [(0x34u32, x), (0x38, z), (0x3C, x), (0x40, z)] {
                        cpu.mem.wr16(c + o, v as u16);
                    }
                    // Every sixth pass the creature comes into the turn
                    // banked as a long chase leaves it, past either level
                    // side, and on every other such pass already facing its
                    // target's bearing: the turn's ease on the bearing
                    // (`0x801F7C68..0x801F7D10`) and its clamp at `0xF00`
                    // (`0x801F7BD8..0x801F7BEC`).
                    if pass % 6 == 5 {
                        let bank =
                            [0xF08u16, 0x804, 0x4, 0xFFC, 0x7F8, 0xC08][(pass / 12) as usize % 6];
                        cpu.mem.wr16(c + 0x48, bank);
                        seeded_bank = Some(bank);
                        if pass % 12 == 11 {
                            let t = cpu.mem.rd32(0x801F_8154);
                            let ta = cpu.mem.rd32(ACTORS + 4 * (t & 7));
                            let b = bearing_12bit_approx(
                                z,
                                x,
                                cpu.mem.rd16(ta + 0x40) as i16,
                                cpu.mem.rd16(ta + 0x3C) as i16,
                            );
                            cpu.mem.wr16(c + 0x46, b & 0xFFF);
                        } else {
                            // ... and on the others facing near zero and
                            // standing a step to the monster's east, so the
                            // struck monster's bearing lies more than a half
                            // turn above the facing (`0x801F7A18..0x801F7A2C`).
                            cpu.mem.wr16(c + 0x46, 0x10);
                            for o in [0x34u32, 0x3C] {
                                cpu.mem.wr16(c + o, (x + 0x40) as u16);
                            }
                        }
                    }
                }
                // Arm `0xD` levels a bank on its first pass that stands
                // within a pass of level, from below (`0x801F7E28..0x801F7E38`)
                // or above (`0x801F7DE8..0x801F7DF0`).
                if (entry, phase) == (915, 0xD) && mushura_fade == 1 {
                    let bank = if walk_step().is_multiple_of(2) {
                        0xFFC
                    } else {
                        0x4
                    };
                    cpu.mem.wr16(ACTOR + CREATURE * STRIDE + 0x48, bank);
                    seeded_bank = Some(bank);
                }
                // PROT 0931's arm `0x16` waits for its creature's cursor to stand
                // on `0x120` exactly (`0x801F7C80`).
                if (entry, phase) == (931, 0x16) {
                    cpu.mem.wr16(NODES + CREATURE * 0x100 + 0x68, 0x120);
                }
                // PROT 0916's fallen runs, and its odd-step live runs, take the
                // kill path: arm 0's verdict
                // word `0x801F9010` reads `0` (a kill) on both sides from the
                // pass after it, which arms 0x11 to 0x14 fork on
                // (`0x801F7DC4`, `0x801F8598`, `0x801F871C`).
                // PROT 0907's fallen runs, and its odd-step live runs, take the
                // instant death: arm 0's fork word `0x801F8534` reads `0`
                // (`rand() % 8 == 0`), which arm 13 forks on (`0x801F7E04`);
                // the fallen runs land it unresisted (`0x801F853C = 0`,
                // `0x801F7E50`). The shadow reads both words off retail.
                if entry == 907 && (fallen || (live && step % 2 == 1)) && phase > 0 {
                    cpu.mem.wr32(0x801F_8534, 0);
                    if fallen {
                        cpu.mem.wr32(0x801F_853C, 0);
                    }
                }
                if entry == 916 && (fallen || (live && step % 2 == 1)) && phase > 0 {
                    if st.kill != Some(true) {
                        cpu.mem.wr32(0x801F_9010, 0);
                        st.kill = Some(true);
                    }
                    if let Some(w) = shadow.as_mut() {
                        assert!(
                            w.casting.module_cam.kill.is_some(),
                            "the shadow drew no verdict"
                        );
                        w.casting.module_cam.kill = Some(true);
                    }
                }
                // The fades that ease the creature's colour word `+0x04` to
                // zero are stubbed; PROT 0915's arm `0xD` and PROT 0917's
                // `0xF` wait on it. PROT 0915's word stands up for the arm's
                // first passes (`0x801F7EB0..0x801F7EB8`), so the arm holds
                // and its bank ease (`0x801F7DBC..0x801F7E64`) runs on.
                if (entry, phase) == (915, 0xD) {
                    mushura_fade += 1;
                    let up = mushura_fade <= 6 / u32::from(walk_step()) + 1;
                    cpu.mem.wr32(
                        ACTOR + CREATURE * STRIDE + 4,
                        if up { 0x0804_0201 } else { 0 },
                    );
                } else if (entry, phase) == (917, 0xF) {
                    cpu.mem.wr32(ACTOR + CREATURE * STRIDE + 4, 0);
                }
                // A monster a hit felled fades out: the tint pass eases its
                // word to zero outside the module, which the walk stands in
                // for once its word has stood six passes after the kill. The settle gates read it as
                // retail's do (a dead seat settled once its word is zero, a
                // living one once its clip is).
                for s in 3..7u32 {
                    let a = ACTOR + s * STRIDE;
                    let dead = cpu.mem.rd16(a + 0x14C) == 0 && cpu.mem.rd32(a + 4) != 0;
                    let k = (s - 3) as usize;
                    dead_passes[k] = if dead { dead_passes[k] + 1 } else { 0 };
                    if dead_passes[k] == 7 {
                        cpu.mem.wr32(a + 4, 0);
                    }
                }
                let settled = |m: &mut psx_retail_exec::Memory, s: u32| {
                    let a = ACTOR + s * STRIDE;
                    if m.rd16(a + 0x14C) == 0 {
                        m.rd32(a + 4) == 0
                    } else {
                        m.rd8(a + 0x1D9) == 0
                    }
                };
                st.row_settled = (3..3 + layout.monsters).all(|s| settled(&mut cpu.mem, s));
                st.victim_settled = settled(&mut cpu.mem, victim);
                // The victim's display seat `+0x3C..+0x40`, which the host
                // hands the director (`World::module_body_seat`): PROT
                // 0918's arm `0xE` stands its spear at four thirds of its
                // height (`lh 0x3e(s4)` at `0x801F7F5C`).
                st.victim_body = Some(seat_at(&mut cpu.mem, victim, true));
                // PROT 0918's arm `0xE` re-frames off the victim's size at the
                // head of its pass (`jal 0x801F0348` at `0x801F7EA4`), ahead
                // of the shot that reads `ctx[+0x6D0]`: the director reads the
                // depth that call leaves, as the host's
                // `cast_target_frame_depth` hands it. The call is a pure
                // store of the word, so the pass makes it again unchanged.
                if (entry, phase) == (918, 0xE) {
                    assert_eq!(cpu.call(FRAME_SIZE, &[]), Stop::Returned);
                }
                let t0 = Target::read(&mut cpu.mem);
                let live_code = cpu.mem.rd8(ACTOR + layout.caster * STRIDE + 0x1DD);
                let seats = ModuleCamSeats {
                    // The engine carries no module's party re-seat (PROT
                    // 0930's): the caster stands where the cast found it.
                    caster: caster_seat,
                    // A seat target is its own victim (PROT 0905 heals a
                    // party seat); a group's latched victim is its first.
                    victim: seat_at(&mut cpu.mem, victim, false),
                    band_timer: i32::from(cpu.mem.rd16(CTX + 0x6D8) as i16),
                    action,
                    depth_raw: i32::from(cpu.mem.rd16(CTX + 0x6D0) as i16),
                    victim_radius: cpu.mem.rd16(NODES + victim * 0x100 + 0x58) as i16,
                    caster_scale: 0x1000,
                    ..ModuleCamSeats::default()
                };
                // `feed_seru_cam`.
                st.live_yaw = Some(t0.angles[1] as u16);
                st.live_pitch_tr = Some((t0.angles[0], t0.tr));
                st.target_code = Some(live_code);
                let any_up = (3..7u32).any(|s| cpu.mem.rd32(ACTOR + s * STRIDE + 4) != 0);
                st.row_aim = group_aim(&mut cpu, 9, any_up);
                // PROT 0920's arm 8 shows every living monster before its aim
                // (`0x801F752C..0x801F755C`, then `jal 0x801DCEAC`): the aim
                // is measured on the row as that pass shows it, its words put
                // back after.
                let shown_first = (entry, phase) == (920, 8);
                let words: Vec<u32> = (3..7u32)
                    .map(|s| cpu.mem.rd32(ACTOR + s * STRIDE + 4))
                    .collect();
                if shown_first {
                    for s in 3..7u32 {
                        if cpu.mem.rd16(ACTOR + s * STRIDE + 0x14C) != 0 {
                            cpu.mem.wr32(ACTOR + s * STRIDE + 4, 0x2008_0200);
                        }
                    }
                }
                let any_up = any_up
                    || (shown_first
                        && (3..7u32).any(|s| cpu.mem.rd32(ACTOR + s * STRIDE + 4) != 0));
                st.target_aim = if live_code < 8 {
                    let s = seat_at(&mut cpu.mem, u32::from(live_code), false);
                    Some(GroupAim {
                        centroid_x: s.x.wrapping_neg(),
                        centroid_z: s.z.wrapping_neg(),
                        extent: MIN_GROUP_EXTENT,
                    })
                } else {
                    group_aim(&mut cpu, live_code, any_up)
                };
                if shown_first {
                    for (s, w) in (3..7u32).zip(&words) {
                        cpu.mem.wr32(ACTOR + s * STRIDE + 4, *w);
                    }
                }
                // `keep_row_tr_y`: `-4 * +0x3E` averaged over the living row.
                let mut row = Vec::new();
                for s in 3..3 + layout.monsters {
                    let a = ACTOR + s * STRIDE;
                    if cpu.mem.rd16(a + 0x14C) != 0 {
                        row.push(-4 * i32::from(cpu.mem.rd16(a + 0x3E) as i16));
                    }
                }
                if !row.is_empty() {
                    st.row_tr_y = (row.iter().sum::<i32>() / row.len() as i32) as i16;
                }
                // `feed_seru_cam`: the first monster seat's HP.
                st.lead_monster_down = cpu.mem.rd16(ACTOR + 3 * STRIDE + 0x14C) == 0;
                st.sideband_busy = cpu.mem.rd8(CTX + 0x276) != 0;
                let c = CREATURE;
                // The creature's live seat, which the host feeds every tick
                // its seat is up: once the installer has seated it (or a
                // director has placed it).
                let seated = cam::module_profile(entry)
                    .and_then(|p| p.seat_arm)
                    .is_none()
                    || installs.get() > 0;
                st.creature_live =
                    (seated || st.creature.is_some()).then(|| seat_at(&mut cpu.mem, c, false));
                st.creature_body = st.creature.map(|_| {
                    let s = seat_at(&mut cpu.mem, c, true);
                    (s.x, s.z)
                });
                st.creature_clip = Some(cpu.mem.rd8(ACTOR + c * STRIDE + 0x1D9));
                st.creature_word_up = cpu.mem.rd32(ACTOR + c * STRIDE + 4) != 0;
                // PROT 0915's run is the host's (`World::apply_mushura_run`),
                // which leaves the creature's live stand in the director's seat.
                if entry == 915 && st.creature.is_some() {
                    st.creature = Some(seat_at(&mut cpu.mem, c, false));
                }
                st.creature_frame = Some(cpu.mem.rd16(NODES + c * 0x100 + 0x68));
                st.creature_loops = Some(cpu.mem.rd8(ACTOR + c * STRIDE + 0x21B));
                if entry == 934 {
                    // The pass word and the rise word as retail's pass opens on
                    // them; the director reads and bumps the word once a pass.
                    st.ozma_shake = (
                        cpu.mem.rd32(OZMA_PASS_WORD),
                        cpu.mem.rd32(OZMA_RISE_WORD) as i32,
                    );
                }
                shots.borrow_mut().clear();
                draws.borrow_mut().clear();
                for w in RUMBLE {
                    cpu.mem.wr32(w, RUMBLE_SENTINEL);
                }
                let countdown_before = st.countdown.0;
                // PROT 0922's arm 20 drops its 23 crystals every pass, held or
                // not, and raises the rumble on each that lands
                // (`0x801F85F8..0x801F8690`): the engine runs the fall off
                // `puera_crystal_fall` (`World::apply_puera_writes`).
                let crystals: Vec<(u32, i16)> = if (entry, phase) == (922, 20) {
                    (0..23u32)
                        .map(|i| {
                            let h = cpu.mem.rd32(PUERA_CRYSTALS + 4 * i);
                            (h, cpu.mem.rd16(h + 0x16) as i16)
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                // The seats as this pass finds them.
                let puera_seats = [3u32, 4, CREATURE];
                if let Some(w) = puera.as_mut() {
                    for &s in &puera_seats {
                        let a = ACTOR + s * STRIDE;
                        let pos = trio(&mut cpu.mem, a + 0x34);
                        let act = &mut w.actors[s as usize];
                        act.active = true;
                        act.battle.hp = cpu.mem.rd16(a + 0x14C);
                        [
                            act.move_state.world_x,
                            act.move_state.world_y,
                            act.move_state.world_z,
                        ] = pos;
                        act.battle.facing_angle = cpu.mem.rd16(a + 0x46);
                        act.battle.render_flag = cpu.mem.rd8(a + 0x21C);
                        act.battle.render_color = cpu.mem.rd32(a + 4);
                    }
                }
                // The seats the host resolves a spawn's anchor on
                // (`World::module_anchor_context`), as this pass opens.
                let anchor_seats = cam::AnchorSeats {
                    caster: Some(caster_seat),
                    caster_body: Some(seat_at(&mut cpu.mem, layout.caster, true)),
                    victim_body: Some(seat_at(&mut cpu.mem, victim, true)),
                    creature_body: Some(seat_at(&mut cpu.mem, CREATURE, true)),
                    party: std::array::from_fn(|n| {
                        (cpu.mem.rd16(ACTOR + n as u32 * STRIDE + 0x14C) != 0)
                            .then(|| seat_at(&mut cpu.mem, n as u32, false))
                    }),
                    depth: cpu.mem.rd16(CTX + 0x6D0) as i16,
                };
                let anchor_victim = seat_at(&mut cpu.mem, victim, false);
                // The creature's live seat, which the host feeds every tick
                // its seat is up (`module_cam.creature_live`).
                let creature_live = seat_at(&mut cpu.mem, CREATURE, false);
                let shadow_phase = shadow.as_ref().map(|w| w.casting.module_phase);
                let mut fold_pass = false;
                if shadow.is_some() && shadow_phase != Some(phase) {
                    shadow = None;
                    shadow_left += usize::from(!step_gap(entry, Some(phase), walk_step()));
                    shadow_leaves.push(format!(
                        "PROT {entry:04} target {code} fallen {fallen} caster {}: retail arm {phase:#x} band {shadow_phase:x?} at pass {pass}",
                        layout.caster
                    ));
                }
                if let Some(w) = shadow.as_mut() {
                    for s in 0..8u32 {
                        let a = ACTOR + s * STRIDE;
                        let act = &mut w.actors[s as usize];
                        act.battle.hp = cpu.mem.rd16(a + 0x14C);
                        let word = cpu.mem.rd32(a + 4);
                        if shadow_after.get(s as usize).is_some_and(|&was| was != word) {
                            act.battle.render_color = word;
                        }
                        // Every seat stands idle as the walk leaves it
                        // (`+0x1D9`), which Mushura's chase tests.
                        act.battle.current_anim = cpu.mem.rd8(a + 0x1D9);
                        if s == CREATURE {
                            let p = trio(&mut cpu.mem, a + 0x34);
                            act.move_state.world_x = p[0];
                            act.move_state.world_y = p[1];
                            act.move_state.world_z = p[2];
                            act.battle.facing_angle = cpu.mem.rd16(a + 0x46);
                            if let Some(b) = seeded_bank {
                                act.battle.render_roll = b;
                            }
                            // The loop counter `+0x21B` the walk's clip parks
                            // on, as the anim tick keeps it.
                            act.battle.hit_count_bound = cpu.mem.rd8(a + 0x21B);
                        }
                    }
                    // The monsters' staged reaction `+0x1DA` / `+0x1DC`, as
                    // this pass finds them.
                    for s in 3..(3 + layout.monsters).min(7) {
                        let r = crate::cast_view::read_actor(&mut cpu.mem, ACTOR + s * STRIDE);
                        let mut v = w.drive_cast_view(s as u8);
                        v.staged_anim = r.staged_anim;
                        v.restage = r.restage;
                        v.hp_bar_delta = r.hp_bar_delta;
                        v.root_speed = r.root_speed;
                        v.render_21f = r.render_21f;
                        w.drive_write_cast_view(s as u8, &v);
                        w.actors[s as usize].battle.facing_angle =
                            cpu.mem.rd16(ACTOR + s * STRIDE + 0x46);
                        // The ground trio and the animation rate, as this pass
                        // finds them.
                        let a = ACTOR + s * STRIDE;
                        let p = trio(&mut cpu.mem, a + 0x34);
                        let act = &mut w.actors[s as usize];
                        [
                            act.move_state.world_x,
                            act.move_state.world_y,
                            act.move_state.world_z,
                        ] = p;
                        act.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(
                            cpu.mem.rd8(a + 0x21D),
                        );
                    }
                    // The walk stubs the range poll `FUN_8004E2F0(7, victim)`
                    // in range, where its creature never walks; the shadow's
                    // PROT 0908 arm 8 reads range off the creature's pair, so
                    // it stands on the victim's for that arm.
                    if (entry, phase) == (908, 8) {
                        let (x, z) = {
                            let v = &w.actors[victim as usize].move_state;
                            (v.world_x, v.world_z)
                        };
                        let c = &mut w.actors[CREATURE as usize].move_state;
                        c.world_x = x;
                        c.world_z = z;
                    }
                    // PROT 0907's verdict as retail's arm 0 drew it (its
                    // fork word `0x801F8534` and resist word `0x801F853C`):
                    // the shadow's own draw reads its zeroed party's levels.
                    if entry == 907 && pass > 0 {
                        use legaia_engine_cast::seru_magic_ticks::NightoOutcome as O;
                        let fork = cpu.mem.rd32(0x801F_8534) != 0;
                        let resisted = cpu.mem.rd32(0x801F_853C) != 0;
                        w.casting.module_nighto_outcome = Some(match (fork, resisted) {
                            (false, false) => O::Kill,
                            (false, true) => O::KillResisted,
                            (true, false) => O::Confuse,
                            (true, true) => O::ConfuseResisted,
                        });
                    }
                    // The band's word `ctx[+0x6D8]` the paced arms poll
                    // (PROT 0907's arm 3), as the SM leaves it this pass.
                    w.battle_ctx.frame_timer = if fallen {
                        port_word as _
                    } else {
                        cpu.mem.rd16(CTX + 0x6D8) as i16 as _
                    };
                    w.casting.band_word_live = None;
                    let port_band = fallen && port_sm <= 0x35;
                    if port_band {
                        w.drive_summon_band_word_in(port_word);
                    }
                    w.rng_state = stream.get();
                    // The side-band stage the walk stands up, as a turn's
                    // side-band read raises it.
                    w.casting.module_cam.sideband_busy = cpu.mem.rd8(CTX + 0x276) != 0;
                    // The drive as the poll finds it: its seek issued, in
                    // flight for the busy passes (`World::module_drive_holds`).
                    w.casting.drive_arms = drive.map(|d| (d, true));
                    w.casting.drive.busy = if polls_busy { 0x100 } else { 0 };
                    // One battle frame of `walk_step` vsyncs a pass.
                    w.clock.display_frames += u64::from(walk_step());
                    // PROT 0920's hit turns its row from the camera's focus
                    // globals (`0x801F793C..0x801F7968`): the shadow's camera
                    // stands on the focus this pass opens on.
                    w.battle.stage_camera = ((entry, phase) == (920, 9)).then(|| {
                        legaia_engine_battle::battle_stage_module::StageCamera {
                            pitch: 0,
                            yaw: 0,
                            tr: [0; 3],
                            focus: t0.focus.map(|v| -v),
                        }
                    });
                    let run = w
                        .drive_summon_module_frame(layout.caster as u8, action, !far_pass)
                        .pop()
                        .flatten();
                    if port_band {
                        if let Some(o) = w.drive_summon_band_word_out() {
                            port_word = o;
                        }
                        if port_sm == 0x34 {
                            port_sm = 0x35;
                        } else if port_word < 0 {
                            port_word = 0;
                            port_sm = 0x36;
                        }
                    }
                    w.battle.stage_camera = None;
                    // The band's fold, on the tick the pass leaves the hit
                    // arm (`summon_stager_tick`'s strike): retail's module
                    // lands the same hit inside its hit arm's pass.
                    // The wrapper's rolls on retail's stream: the band's
                    // module half draws for the spawns it stages (PROT
                    // 0920's arms 5 to 9 draw some hundreds a pass), where
                    // this walk's retail runs no part.
                    if live {
                        w.rng_state = stream.get();
                    }
                    fold_pass = w.drive_summon_strike(layout.caster as u8, action, run.as_ref());
                    fold_passes += usize::from(fold_pass);
                    // The stager's owed seat, landed after the pass that
                    // reaches the seat arm (`summon_stager_tick`), on the
                    // caster with the seat arm's stores: retail's installer
                    // runs inside the same pass.
                    if !port_seated
                        && let Some(arm) = seat_arm
                        && legaia_engine_battle_loop::module_seat_arm::seat_due(
                            Some(entry),
                            arm,
                            w.casting.module_phase,
                        )
                    {
                        w.seat_summon_actor(CREATURE as usize);
                        // The seat pass's node tick lands the seat arm's held
                        // stores behind the installer's resets
                        // (`World::run_seat_pass_node_tick`).
                        w.drive_seat_pass_writes();
                        port_seated = true;
                    }
                }
                spawn_log.borrow_mut().clear();
                prims.borrow_mut().clear();
                modes.borrow_mut().clear();
                // The part PROT 0933 keeps the handle of stands in the pool.
                if whirls {
                    let own = cpu.mem.rd32(0x801F_941C);
                    cpu.mem.wr32(PART_POOL + 4 * 0x20, own);
                }
                // The packet cursor `0x1F8003A0` and the ordering table
                // `0x1F8003F4` the band arms write through.
                cpu.mem.wr32(0x1F80_03A0, PACKETS);
                cpu.mem.wr32(0x1F80_03F4, PACKETS + 0x1000);
                let hp_before: Vec<u16> = (3..7u32)
                    .map(|s| cpu.mem.rd16(ACTOR + s * STRIDE + 0x14C))
                    .collect();
                let stop = cpu.call(va, &[]);
                if fallen {
                    if sm == 0x34 {
                        sm = 0x35;
                    } else if sm == 0x35 && (cpu.mem.rd16(CTX + 0x6D8) as i16) < 0 {
                        cpu.mem.wr16(CTX + 0x6D8, 0);
                        sm = 0x36;
                    }
                }
                if (entry, phase) == (915, 0xC) {
                    chase_hits += (3..7u32)
                        .zip(&hp_before)
                        .filter(|&(s, &h)| cpu.mem.rd16(ACTOR + s * STRIDE + 0x14C) != h)
                        .count();
                }
                if stop != Stop::Returned {
                    failures.push(format!("PROT {entry:04} pass {pass} arm {phase}: {stop:?}"));
                    break;
                }
                drive_polls_held += (busy_calls.get() - calls_before) as usize;
                // PROT 0915's chase runs ahead of the director
                // (`World::mushura_chase_ahead`): the director reads the
                // creature as this pass's turn left it.
                if entry == 915 && matches!(phase, 7 | 8 | 0xC) {
                    st.creature = Some(seat_at(&mut cpu.mem, CREATURE, false));
                }
                // PROT 0934's spray loop opens the pass on six draws, x then
                // y for each of its three records; the third point is what
                // arm `0x15`'s exit cut aims at (`World::draw_ozma_spray`).
                let spray = draws.borrow().clone();
                if entry == 934 && (0x15..=0x18).contains(&phase) && spray.len() >= 6 {
                    let point = |r: u32| (r % 0x101) as i16 - 0x80;
                    st.ozma_spray_point = Some((point(spray[4]), point(spray[5])));
                }
                // The host's side-band poll (`World::module_code_run`): on the
                // wait arm with the stage up, an arm that would pass keeps its
                // drain and its drift and holds.
                let polling = (st.sideband_busy && cam::sideband_wait_arm(entry) == Some(phase))
                    || polls_busy;
                // A pass is `walk_step` one-vsync director ticks, as the host
                // runs it (`World::module_pass`): the first opens the frame,
                // each carries the ticks still owed after it, and the tick an
                // arm hands over on ends the pass.
                let step = walk_step();
                let mut ticks: Vec<cam::ArmDirection> = Vec::new();
                let mut tick_before: Vec<i32> = Vec::new();
                // The camera as the ticks so far leave it, which the host
                // feeds each tick (`feed_seru_cam`).
                let mut port = t0;
                for i in 0..step {
                    if i > 0 {
                        st.live_yaw = Some(port.angles[1] as u16);
                        st.live_pitch_tr = Some((port.angles[0], port.tr));
                        // A seek the last tick wrote is on the node now.
                        if let Some(seek) = st.creature_seek {
                            st.creature_frame = Some(seek);
                        }
                    }
                    st.opens_pass = i == 0;
                    st.latch_pass_word();
                    tick_before.push(st.countdown.0);
                    st.countdown.1 = step - 1 - i;
                    let tick = if polling {
                        let mut ahead = st;
                        let d = (profile.direct)(&mut ahead, phase, seats);
                        if d.hold {
                            st = ahead;
                            d
                        } else {
                            let hold = cam::polled_hold(entry, phase, d, &st);
                            st.countdown =
                                cam::polled_countdown(profile.direct, &st, &ahead, phase, seats);
                            hold
                        }
                    } else {
                        (profile.direct)(&mut st, phase, seats)
                    };
                    ticks.push(tick);
                    port.apply(&tick);
                    if !tick.hold {
                        break;
                    }
                }
                st.opens_pass = true;
                st.countdown.1 = 0;
                let owed = step as usize - ticks.len();
                let arm = *ticks.last().expect("a tick");
                // PROT 0916's bands, against `aluru_band_prims` at the spread
                // the director leaves its pass on: each `POLY_G4`'s code,
                // four colours and four corners, and the draw mode's blend.
                if entry == 916 {
                    let got = prims.borrow().clone();
                    let port = st.band_spread.map(aluru_band_prims).unwrap_or_default();
                    let want: Vec<Vec<u8>> = port
                        .iter()
                        .filter_map(|p| match p {
                            ScreenPrim::Flat(q) => Some(q),
                            _ => None,
                        })
                        .map(|q| {
                            let mut b = vec![0u8; 0x24];
                            let g = q.gouraud.unwrap_or([q.color; 4]);
                            for v in 0..4 {
                                b[4 + 8 * v..7 + 8 * v].copy_from_slice(&g[v][..3]);
                                b[8 + 8 * v..0xA + 8 * v].copy_from_slice(&q.xy[v].0.to_le_bytes());
                                b[0xA + 8 * v..0xC + 8 * v]
                                    .copy_from_slice(&q.xy[v].1.to_le_bytes());
                            }
                            b[7] = if q.semi_transparent { 0x3A } else { 0x38 } | 1;
                            b
                        })
                        .collect();
                    // Retail's bands, then its `SetDrawMode` (the tpage's ABR
                    // at bits 5..6).
                    let bands: Vec<_> = got.iter().filter(|p| p[7] >> 4 == 3).collect();
                    let bands: Vec<Vec<u8>> = bands
                        .into_iter()
                        .map(|p| {
                            let mut b = p.clone();
                            b[..4].fill(0);
                            for v in 1..4 {
                                b[7 + 8 * v] = 0;
                            }
                            b
                        })
                        .collect();
                    let abr = modes.borrow().first().map(|t| ((t >> 5) & 3) as u8);
                    let want_abr = (!want.is_empty()).then_some(ALURU_BAND_ABR_MODE);
                    band_passes += usize::from(!bands.is_empty());
                    if bands != want || abr != want_abr {
                        shadow_failures.push(format!(
                            "PROT 0916 target {code} fallen {fallen} caster {} of {} pass {pass} arm {phase:#x}: bands retail {:x?} abr {abr:?}, port spread {:?}",
                            layout.caster,
                            layout.monsters,
                            bands.iter().map(|b| [b[0xA], b[0x1A]]).collect::<Vec<_>>(),
                            st.band_spread
                        ));
                    }
                }
                // PROT 0919's eye parts against `spoon_eye_seat` on the
                // creature as the pass leaves it: retail's handles at
                // `0x801F8E6C`, four records an eye.
                if entry == 919 && (4..=7).contains(&phase) {
                    let c = ACTOR + CREATURE * STRIDE;
                    let (cx, cz, f) = (
                        cpu.mem.rd16(c + 0x34) as i16,
                        cpu.mem.rd16(c + 0x38) as i16,
                        cpu.mem.rd16(c + 0x46),
                    );
                    for (eye, pose) in spoon_poses.iter().enumerate() {
                        let want = cam::spoon_eye_seat(cx, cz, f, *pose, phase, eye);
                        for r in 0..4u32 {
                            let h = cpu.mem.rd32(SPOON_EYES + 4 * (eye as u32 * 4 + r));
                            if h == 0 {
                                continue;
                            }
                            let got = trio(&mut cpu.mem, h + 0x14);
                            spoon_eyes += 1;
                            spoon_signs |= u8::from(pose[0] < 0)
                                | u8::from(pose[2] < 0) << 1
                                | u8::from(pose[0] > 0) << 2
                                | u8::from(pose[2] > 0) << 3;
                            if got != want {
                                shadow_failures.push(format!(
                                    "PROT 0919 caster {} of {} pass {pass} arm {phase:#x}: eye {eye} record {r} retail {got:x?} port {want:x?}",
                                    layout.caster, layout.monsters
                                ));
                            }
                        }
                    }
                }
                // PROT 0933's whirls, against the shadow's scene parts: the
                // pass's `+0x7A += step`, `+0x16 += 4 * step` and the retire
                // (`+0x10 | 8`) of a part under level `0xA01`, while the part
                // stands (retail's pool keeps stepping a retired part, which
                // the part pass then frees).
                if whirls
                    && let Some(w) = shadow.as_ref()
                    && let Some(scene) = w.casting.active_summon.as_ref()
                {
                    for (k, &(_, rec, _)) in TERRA_WHIRLS.iter().enumerate() {
                        let at = WHIRL_PARTS + 0x100 * k as u32;
                        let Some(p) = scene.parts.iter().find(|p| p.tag == Some(rec)) else {
                            continue;
                        };
                        let retired = cpu.mem.rd32(at + 0x10) & 8 != 0;
                        let got = (
                            cpu.mem.rd16(at + 0x7A),
                            cpu.mem.rd16(at + 0x16) as i16,
                            retired,
                        );
                        let port = (p.state.field_7a, p.state.world_y, p.finished);
                        if !whirl_retired[k] && got != port {
                            shadow_failures.push(format!(
                                "PROT 0933 target {code} fallen {fallen} caster {} of {} pass {pass} arm {phase:#x}: whirl {k} 7A/16/retired retail {got:x?} port {port:x?}",
                                layout.caster, layout.monsters
                            ));
                        }
                        whirl_retired[k] |= retired;
                        whirl_passes += usize::from(matches!(phase, 6 | 7));
                    }
                }
                // The records the director reports this pass, each on the
                // point its anchor resolves to, against one of retail's
                // spawn calls with the same record, point and angles.
                // PROT 0934's spray points are the host's draws.
                {
                    let mut left = spawn_log.borrow().clone();
                    let pass_spawns = ticks
                        .iter()
                        .flat_map(|t| t.spawns.iter().map(move |sp| (sp, t.arm_point, t.shot)));
                    for (sp, arm_point, arm_shot) in pass_spawns {
                        let record = match sp.record {
                            cam::SpawnRecord::Module(r) => r,
                            cam::SpawnRecord::BattleProto(p) => cpu.mem.rd32(p),
                        };
                        if entry == 934 && matches!(record, 0x801F_A320 | 0x801F_A36C | 0x801F_A3B8)
                        {
                            continue;
                        }
                        let point = match sp.anchor {
                            cam::SpawnAnchor::ArmPoint => arm_point,
                            a => cam::spawn_anchor_point_with(
                                a,
                                st.creature.or(st.creature_live).or(Some(creature_live)),
                                Some(anchor_victim),
                                arm_shot,
                                anchor_seats,
                            ),
                        };
                        let Some((pos, rot)) = point else {
                            spawn_failures.push(format!(
                                "PROT {entry:04} pass {pass} arm {phase:#x}: spawn {record:#x} anchor {:?} unresolved",
                                sp.anchor
                            ));
                            continue;
                        };
                        // Angles compare as the 12-bit turns the record
                        // reads: a director's shot holds its yaw masked
                        // where retail's stack holds the raw halfword.
                        let turn = |a: [i16; 3]| a.map(|v| v & 0xFFF);
                        match left
                            .iter()
                            .position(|&x| (x.0, x.1, turn(x.2)) == (record, pos, turn(rot)))
                        {
                            Some(i) => {
                                left.remove(i);
                                spawns_matched += 1;
                            }
                            None => spawn_failures.push(format!(
                                "PROT {entry:04} target {code} fallen {fallen} pass {pass} arm {phase:#x}: spawn {record:#x} {:?} at {pos:?} {rot:?}: retail {:x?}",
                                sp.anchor,
                                left.iter().filter(|x| x.0 == record).collect::<Vec<_>>()
                            )),
                        }
                    }
                }
                if let Some(w) = shadow.as_ref() {
                    // The seated seats: an unseated monster seat is no actor
                    // the engine draws.
                    // The creature's seat 7 too, once both sides have seated
                    // it: retail's seat arm calls the installer, the host
                    // seats it as the band's pass reaches that arm
                    // (`World::seat_summon_actor`). A side that seats it on
                    // another pass is a mismatch of its own.
                    let retail_seated = seat_arm.is_none() || installs.get() > 0;
                    if retail_seated != port_seated {
                        shadow_failures.push(format!(
                            "PROT {entry:04} target {code} fallen {fallen} caster {} of {} pass {pass} arm {phase:#x}: seat 7 seated retail {retail_seated} port {port_seated}",
                            layout.caster, layout.monsters
                        ));
                    }
                    let creature = retail_seated && port_seated;
                    creature_passes += usize::from(creature);
                    // The creature's target code `+0x1DD`, which its root
                    // motion's range poll reads.
                    if creature {
                        let rt = cpu.mem.rd8(ACTOR + CREATURE * STRIDE + 0x1DD);
                        let pt = w.actors[CREATURE as usize].battle.active_target;
                        if rt != pt {
                            shadow_failures.push(format!(
                                "PROT {entry:04} target {code} fallen {fallen} caster {} of {} pass {pass} arm {phase:#x}: seat 7 target retail {rt:#x} port {pt:#x}",
                                layout.caster, layout.monsters
                            ));
                        }
                    }
                    let seated = (0..(3 + layout.monsters as usize).min(7))
                        .chain(creature.then_some(CREATURE as usize));
                    for s in seated {
                        let a = ACTOR + s as u32 * STRIDE;
                        let got = (
                            cpu.mem.rd32(a + 4),
                            cpu.mem.rd8(a + 0x21C),
                            cpu.mem.rd16(a + 0x16E),
                            cpu.mem.rd16(a + 0x14C),
                        );
                        let b = &w.actors[s].battle;
                        // The summon band's shown word is the engine's resting
                        // `0` under a lowered flag (`apply_mushura_writes`,
                        // `apply_palma_writes`): retail's `0x20080200` there.
                        let shown = |c: u32, f: u8| if f == 0 && c == 0x2008_0200 { 0 } else { c };
                        let got = (shown(got.0, got.1), got.1, got.2, got.3);
                        let port = (
                            shown(b.render_color, b.render_flag),
                            b.render_flag,
                            w.raw_status_word(s as u8),
                            b.hp,
                        );
                        if s >= 3 {
                            let r = crate::cast_view::read_actor(&mut cpu.mem, a);
                            let v = w.drive_cast_view(s as u8);
                            let rs = r.staged_anim;
                            let ps = v.staged_anim;
                            // The fold's pass also writes the hit's bar
                            // delta, the hit word `+0x0C`, the `+0x21F`
                            // byte and the victim's facing, on the struck
                            // row (the creature is no fold target).
                            if fold_pass && s != CREATURE as usize {
                                let rf = (
                                    r.hp_bar_delta,
                                    r.root_speed,
                                    r.render_21f,
                                    cpu.mem.rd16(a + 0x46) & 0xFFF,
                                );
                                let pf = (
                                    v.hp_bar_delta,
                                    v.root_speed,
                                    v.render_21f,
                                    w.actors[s].battle.facing_angle & 0xFFF,
                                );
                                if rf != pf {
                                    shadow_failures.push(format!(
                                        "PROT {entry:04} target {code} fallen {fallen} caster {} of {} pass {pass} arm {phase:#x}: seat {s} fold bar/0C/21F/facing retail {rf:x?} port {pf:x?}",
                                        layout.caster, layout.monsters
                                    ));
                                }
                            }
                            if rs != ps {
                                shadow_failures.push(format!(
                                    "PROT {entry:04} target {code} fallen {fallen} caster {} of {} pass {pass} arm {phase:#x}: seat {s} staged retail ({rs:x}) port ({ps:x})",
                                    layout.caster, layout.monsters
                                ));
                            }
                        }
                        if got != port {
                            shadow_failures.push(format!(
                                "PROT {entry:04} target {code} fallen {fallen} caster {} of {} pass {pass} arm {phase:#x}: seat {s} tint/flag/status/hp retail {got:x?} port {port:x?}",
                                layout.caster, layout.monsters
                            ));
                        }
                        // The finisher's spirit gain on the struck seat
                        // (`+0x170`), with the wrapper live.
                        if live {
                            let rs = cpu.mem.rd16(a + 0x170);
                            let ps = w.actors[s].battle.spirit_gauge;
                            live_spirit += usize::from(rs != 0);
                            if rs != ps {
                                shadow_failures.push(format!(
                                    "PROT {entry:04} target {code} fallen {fallen} caster {} of {} pass {pass} arm {phase:#x}: seat {s} spirit retail {rs} port {ps}",
                                    layout.caster, layout.monsters
                                ));
                            }
                        }
                    }
                    // The row's ground trio, animation rate and render scale,
                    // on dead seats too. The creature's seat 7 is compared as
                    // well, but for the arms of [`CREATURE_GAPS`].
                    let rows =
                        (3..(3 + layout.monsters).min(7)).chain(creature.then_some(CREATURE));
                    for s in rows {
                        let a = ACTOR + s * STRIDE;
                        let act = &w.actors[s as usize];
                        let gap = |part| s == CREATURE && creature_gap(entry, phase, part);
                        let trio_out = gap(CreatureGap::Trio);
                        let rate_out = gap(CreatureGap::Rate);
                        let got = (
                            (!trio_out).then(|| trio(&mut cpu.mem, a + 0x34)),
                            (!rate_out).then(|| cpu.mem.rd8(a + 0x21D)),
                        );
                        let port = (
                            (!trio_out).then_some([
                                act.move_state.world_x,
                                act.move_state.world_y,
                                act.move_state.world_z,
                            ]),
                            (!rate_out).then(|| act.battle.anim_rate.get()),
                        );
                        // The render node's scale `+0x72`, against the seat's
                        // `render_scale` (unity when unset): PROT 0921's exit
                        // leaves a Stoned seat shrunken in the retail mode
                        // (`WorldToggles::stone_scale_fix` off).
                        let rs = cpu.mem.rd16(NODES + s * 0x100 + 0x72);
                        let ps = act.battle.render_scale.unwrap_or(0x1000);
                        if rs != ps {
                            shadow_failures.push(format!(
                                "PROT {entry:04} target {code} fallen {fallen} caster {} of {} pass {pass} arm {phase:#x}: seat {s} scale retail {rs:#x} port {ps:#x}",
                                layout.caster, layout.monsters
                            ));
                        }
                        if got != port {
                            shadow_failures.push(format!(
                                "PROT {entry:04} target {code} fallen {fallen} caster {} of {} pass {pass} arm {phase:#x}: seat {s} trio/rate retail {got:x?} port {port:x?}",
                                layout.caster, layout.monsters
                            ));
                        }
                        // PROT 0915's creature banks as it falls, turns and
                        // levels out (`+0x48`, arms 7, 8, 0xC and 0xD), which
                        // the host draws as the body's roll.
                        if entry == 915 && s == CREATURE {
                            let rr = cpu.mem.rd16(a + 0x48) & 0xFFF;
                            let pr = act.battle.render_roll & 0xFFF;
                            if rr != pr {
                                shadow_failures.push(format!(
                                    "PROT {entry:04} target {code} fallen {fallen} caster {} of {} pass {pass} arm {phase:#x}: creature bank retail {rr:#x} port {pr:#x}",
                                    layout.caster, layout.monsters
                                ));
                            }
                        }
                    }
                    shadow_passes += 1;
                }
                shadow_after = (0..8u32)
                    .map(|s| cpu.mem.rd32(ACTOR + s * STRIDE + 4))
                    .collect();
                // The ticks of the pass the handing-over tick leaves owed:
                // its drift and nudge, as retail's one `delta`-scaled pass
                // makes them (`World::drift_rest_of_pass`). A drift ahead of
                // the tick's shot owes none: the shot is built after the
                // pass's whole drift (`World::summon_stager_tick`), and one
                // after it is the next pass's.
                let owed_drift = (arm.shot.is_none() && !arm.drift_after_shot)
                    .then_some(arm.rest_drift.unwrap_or(arm.drift))
                    .flatten();
                for _ in 0..owed {
                    port.apply(&cam::ArmDirection {
                        drift: owed_drift,
                        nudge: arm.nudge,
                        ..cam::ArmDirection::HOLD
                    });
                }
                let busy = cpu.regs.r[V0] != 0;
                let after = cpu.mem.rd8(CTX + 0x279);
                let retail = Target::read(&mut cpu.mem);
                let shot = shots.borrow().last().copied();
                let held = busy && after == phase;
                let mut diffs_early: Vec<String> = Vec::new();
                if !held && path.last() != Some(&phase) {
                    path.push(phase);
                }
                let port_after = if arm.hold {
                    phase
                } else {
                    arm.jump.unwrap_or(phase.wrapping_add(1))
                };
                let mut diffs = std::mem::take(&mut diffs_early);
                // The arms whose seat writes `apply_puera_writes` carries; the
                // creature's arm-3 seat and arm-4 slide are the seat-arm
                // tables' (`module_seat_arm`), run per engine tick.
                if let Some(w) = puera
                    .as_mut()
                    .filter(|_| matches!(phase, 0 | 11 | 12 | 18 | 19 | 24))
                {
                    use legaia_engine_core::world::arm_drivers::BattleArmDrivers;
                    // Once a tick, as the host's pass runs them.
                    let n = tick_before.len();
                    for (i, &before) in tick_before.iter().enumerate() {
                        let passed = i + 1 == n && !held;
                        w.casting.module_cam.countdown.1 = walk_step() - 1 - i as u8;
                        w.puera_writes(phase, passed, before, layout.monsters as u8);
                    }
                    for &s in &puera_seats {
                        let a = ACTOR + s * STRIDE;
                        let retail_seat = (
                            trio(&mut cpu.mem, a + 0x34),
                            cpu.mem.rd16(a + 0x46) & 0xFFF,
                            cpu.mem.rd8(a + 0x21C),
                            cpu.mem.rd32(a + 4),
                        );
                        let act = &w.actors[s as usize];
                        let port_seat = (
                            [
                                act.move_state.world_x,
                                act.move_state.world_y,
                                act.move_state.world_z,
                            ],
                            act.battle.facing_angle & 0xFFF,
                            act.battle.render_flag,
                            act.battle.render_color,
                        );
                        if retail_seat != port_seat {
                            diffs.push(format!(
                                "seat {s} retail {retail_seat:x?} port {port_seat:x?}"
                            ));
                        }
                        puera_compared.insert((phase, s == CREATURE, held));
                    }
                }
                // PROT 0922's creature writes, which the host carries out off
                // the director's state: the staged clip `+0x1DA`, the rate
                // `+0x21D` and the node cursor `+0x68` (arms 5, 7..10, 18).
                if entry == 922 {
                    let a = ACTOR + CREATURE * STRIDE;
                    let node = NODES + CREATURE * 0x100;
                    if let Some(stage) = st.creature_stage.take() {
                        staged_writes += 1;
                        if cpu.mem.rd8(a + 0x1DA) != stage {
                            diffs.push(format!(
                                "stage retail {} port {stage}",
                                cpu.mem.rd8(a + 0x1DA)
                            ));
                        }
                    }
                    if let Some(rate) = st.creature_rate.take()
                        && cpu.mem.rd8(a + 0x21D) != rate
                    {
                        diffs.push(format!(
                            "rate retail {} port {rate}",
                            cpu.mem.rd8(a + 0x21D)
                        ));
                    }
                    if let Some(seek) = st.creature_seek.take()
                        && cpu.mem.rd16(node + 0x68) != seek
                    {
                        diffs.push(format!(
                            "cursor retail {:#x} port {seek:#x}",
                            cpu.mem.rd16(node + 0x68)
                        ));
                    }
                    st.creature_restage = false;
                }
                // The rumble stores (`cast_module_rumble`), behind the
                // Battles vibration word `0x800845C8` left on.
                let words = RUMBLE.map(|w| cpu.mem.rd32(w));
                let stored = (words[0] != RUMBLE_SENTINEL).then(|| BattleRumbleWrite {
                    pulse: words[0] as i32,
                    level: words[1] as i32,
                    pulse_rate: words[2] as i32,
                    level_rate: words[3] as i32,
                });
                let rates =
                    (words[2] != RUMBLE_SENTINEL).then(|| (words[2] as i32, words[3] as i32));
                // `World::module_rumble` with the row off stores nothing.
                let due: Vec<_> =
                    module_rumble_due(entry, SINGLE_BODY, phase, !held, countdown_before)
                        .filter(|_| !vibration_off)
                        .collect();
                let mut landed = false;
                for &(h, y) in &crystals {
                    let (y, l) =
                        puera_crystal_fall(y, (SPEED_SCALAR << 7) * i32::from(walk_step()));
                    landed |= l;
                    if cpu.mem.rd16(h + 0x16) as i16 != y {
                        diffs_early.push(format!(
                            "crystal {h:#x} retail {} port {y}",
                            cpu.mem.rd16(h + 0x16) as i16
                        ));
                    }
                }
                crystal_landings += usize::from(landed);
                let pass_draws = draws.borrow().clone();
                let rumble_ok = match due.as_slice() {
                    [] => rates.is_none(),
                    [m] => match m.store {
                        RumbleStore::Write(w) => stored == Some(w) && words[4] == 0,
                        RumbleStore::Rates {
                            pulse_rate,
                            level_rate,
                        } => stored.is_none() && rates == Some((pulse_rate, level_rate)),
                        RumbleStore::OzmaSpray => pass_draws
                            .windows(2)
                            .any(|d| stored == Some(ozma_spray(d[0], d[1]))),
                        RumbleStore::PueraAlternate { .. } => true,
                    },
                    _ => false,
                };
                // Arm 20's landing store: `FF / FF / 0x10 / 0x10`, hold `0`.
                // With the vibration row off it stores nothing
                // (`World::apply_puera_writes`).
                let rumble_ok = if crystals.is_empty() {
                    rumble_ok
                } else if landed && vibration_off {
                    stored.is_none() && rates.is_none()
                } else if landed {
                    stored
                        == Some(BattleRumbleWrite {
                            pulse: 0xFF,
                            level: 0xFF,
                            pulse_rate: 0x10,
                            level_rate: 0x10,
                        })
                        && words[4] == 0
                } else {
                    due.is_empty() && rates.is_none()
                };
                if rumble_ok && !due.is_empty() {
                    rumbles += 1;
                }
                // A live wrapper's finisher jolts the pad on each struck seat
                // (`0x801DE2E4..0x801DE364`, `World::finisher_struck`) over
                // the module's own stores; the stubbed runs compare those.
                if !rumble_ok && !live {
                    diffs.push(format!("rumble retail {stored:?} {rates:?} port {due:?}"));
                }
                // A terminal arm stores no phase; the port stands at `0xFF`.
                let terminal = !busy && (phase == 0xFF || port_after == 0xFF);
                // A director beside a ported tick body leaves the phase to
                // the body: only its hold is its own.
                let walks = profile.director_body;
                // PROT 0907's kill leg (arm `0xE`) holds on the victim's fade
                // and, resisted, on its word: the tick body's call
                // (`nighto_tick`), which the band shadow runs and compares;
                // the director drains the word and passes.
                let body_holds = (entry, phase) == (907, 0xE);
                if !terminal
                    && !body_holds
                    && (held != arm.hold || (walks && !held && after != port_after))
                {
                    diffs.push(format!(
                        "pace retail {held}/{after:#x} port {}/{port_after:#x}",
                        arm.hold
                    ));
                }
                // The tween: a cut (`0` or `1` frames) and a store land alike.
                let frames = |f: Option<u16>| f.filter(|&f| f > 1);
                let port_frames = arm.shot.map(|s| s.frames);
                if frames(shot.map(|s| s.frames)) != frames(port_frames) {
                    diffs.push(format!(
                        "tween retail {:?} port {:?}",
                        shot.map(|s| s.frames),
                        port_frames
                    ));
                }
                // What the port is known to leave out, compared as the port
                // holds it.
                let mut retail = retail;
                for &(e, arms, gap, _) in &KNOWN_GAPS {
                    if e == entry && arms.contains(&phase) {
                        match gap {
                            Gap::Roll => {
                                max_roll = max_roll.max(sign12(retail.angles[2]).abs());
                                retail.angles[2] = port.angles[2];
                            }
                            Gap::ExitShot if !held => {
                                retail = port;
                                diffs.retain(|d| !d.starts_with("tween"));
                            }
                            Gap::ExitShot => {}
                        }
                    }
                }
                if !retail.same(&port) {
                    diffs.push(format!("target retail {retail:?} port {port:?}"));
                }
                if !diffs.is_empty() {
                    bad += 1;
                    if bad <= 12 {
                        failures.push(format!(
                            "PROT {entry:04} target {code} fallen {fallen} caster {} of {} pass {pass} arm {phase:#x}: {}",
                            layout.caster,
                            layout.monsters,
                            diffs.join("; ")
                        ));
                    }
                }
                if !busy {
                    finished = true;
                    break;
                }
            }
            // `apply_puera_writes` compared on every arm it carries, passed,
            // and on the counting passes of the arms that write while held.
            let want = [0u8, 11, 12, 18, 19, 24]
                .map(|p| (p, false))
                .into_iter()
                .chain([(11, true), (12, true), (19, true)]);
            for (p, h) in want {
                if entry == 922 && !puera_compared.contains(&(p, true, h)) {
                    failures.push(format!(
                        "PROT 0922: arm {p} (held {h}) seat writes not compared"
                    ));
                }
            }
            if entry == 922 && (crystal_landings == 0 || staged_writes < 5) {
                failures.push(format!(
                    "PROT 0922: {crystal_landings} crystal landings, {staged_writes} creature stages"
                ));
            }
            // Every carried rumble write of the module was compared.
            let carried = MODULE_RUMBLE.iter().filter(|m| m.entry == entry).count();
            if carried > 0 && rumbles == 0 && !vibration_off {
                failures.push(format!("PROT {entry:04}: no rumble write compared"));
            }
            if max_roll != 0 {
                eprintln!("PROT {entry:04}: retail rolls the camera up to {max_roll}");
            }
            eprintln!(
                "PROT {entry:04} target {code} fallen {fallen} caster {} of {}: {bad} differing passes, arms {:x?}{}",
                layout.caster,
                layout.monsters,
                path,
                if finished { " Done" } else { "" }
            );
            // The spell XP the finisher banks on the caster's record
            // (`+0x8`, its first spell slot), against the shadow's banked or
            // held gain.
            if live && let Some(w) = shadow.as_mut() {
                let rx = cpu.mem.rd32(CHAR_RECORDS + layout.caster * CHAR_STRIDE + 8);
                let px = legaia_engine_battle::magic_xp::spell_xp(
                    &w.party.roster.members[layout.caster as usize],
                    0,
                );
                live_xp += usize::from(rx != 0);
                if rx != px {
                    shadow_failures.push(format!(
                        "PROT {entry:04} target {code} fallen {fallen} caster {} of {}: spell XP retail {rx} port {px}",
                        layout.caster, layout.monsters
                    ));
                }
            }
            if !finished {
                failures.push(format!(
                    "PROT {entry:04} target {code} fallen {fallen} caster {} of {}: no finish, stuck at arm {:#x}",
                    layout.caster,
                    layout.monsters,
                    cpu.mem.rd8(CTX + 0x279)
                ));
            }
        }
    }
    tag(
        &mut failures,
        &mut shadow_failures,
        &mut spawn_failures,
        &mut marks,
        was_live,
    );
    WALK_STEP.with(|s| s.set(1));
    eprintln!(
        "summon walk: {live_runs} live-wrapper runs, {live_spirit} spirit gains and {live_xp} spell XP gains compared"
    );
    assert!(
        live_spirit > 10_000 && live_xp > 100,
        "{live_spirit} spirit gains, {live_xp} spell XP gains"
    );
    // The step-above-1 rows still open ([`STEP_GAPS`]).
    let gapped = failures.len() + shadow_failures.len() + spawn_failures.len();
    failures.retain(|m| !step_gapped(m));
    shadow_failures.retain(|m| !step_gapped(m));
    spawn_failures.retain(|m| !step_gapped(m));
    eprintln!(
        "summon walk: {} step-gap lines left out",
        gapped - failures.len() - shadow_failures.len() - spawn_failures.len()
    );
    assert!(sideband_held > 100, "{sideband_held} side-band passes held");
    eprintln!(
        "summon shadow: {shadow_passes} passes compared ({fold_passes} folds), {shadow_left} casts left retail's phase walk, {} mismatches",
        shadow_failures.len()
    );
    // The casts whose band leaves retail's walk: the paced bodies (PROT
    // 0905, 0907, 0908) whose tick ports pace an arm the walk's director
    // does not.
    for l in &shadow_leaves {
        eprintln!("{l}");
    }
    legaia_engine_core::world::summon_roll_override::set(None);
    if let Ok(path) = std::env::var("W54_SHADOW_DUMP") {
        let _ = std::fs::write(&path, shadow_failures.join("\n"));
    }
    if let Ok(path) = std::env::var("W54_CAMERA_DUMP") {
        let all: Vec<&String> = failures
            .iter()
            .chain(&spawn_failures)
            .chain(&shadow_leaves)
            .collect();
        let _ = std::fs::write(
            &path,
            all.iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    assert!(
        shadow_passes >= 200_000,
        "summon shadow passes {shadow_passes}"
    );
    eprintln!("summon shadow: seat 7 compared on {creature_passes} passes");
    assert!(
        creature_passes >= 60_000,
        "summon shadow seat 7 passes {creature_passes}"
    );
    // Every directed hit's fold ran on its pass, but PROT 0927's, which its
    // stager sweeps spend outside the walk.
    assert!(fold_passes >= 120, "summon shadow folds {fold_passes}");
    // Every module's drive poll held busy on both sides, and no shadow left
    // retail's phase walk over it.
    assert!(
        drive_polls_held >= 300,
        "{drive_polls_held} busy drive polls"
    );
    assert_eq!(shadow_left, 0, "casts left retail's phase walk");
    assert!(band_passes >= 200, "PROT 0916: {band_passes} band passes");
    assert!(
        whirl_passes >= 100,
        "PROT 0933: {whirl_passes} whirl passes"
    );
    assert!(
        spoon_eyes >= 1000 && spoon_signs == 0xF,
        "PROT 0919: {spoon_eyes} eye parts compared, signs {spoon_signs:#x}"
    );
    assert!(
        gimard_far_passes >= 100,
        "PROT 0903: {gimard_far_passes} walk-in passes out of reach"
    );
    // PROT 0915's chase struck the row through its live reach poll.
    assert!(chase_hits >= 50, "PROT 0915 chase: {chase_hits} hits");
    assert!(
        shadow_failures.is_empty(),
        "{} summon shadow mismatches:\n{}",
        shadow_failures.len(),
        shadow_failures[..shadow_failures.len().min(40)].join("\n")
    );
    eprintln!(
        "director spawns: {spawns_matched} matched, {} mismatches",
        spawn_failures.len()
    );
    assert!(
        spawns_matched >= 800,
        "director spawns matched {spawns_matched}"
    );
    assert!(
        spawn_failures.is_empty(),
        "{} director spawn mismatches:\n{}",
        spawn_failures.len(),
        spawn_failures[..spawn_failures.len().min(40)].join("\n")
    );
    let n = failures.len();
    failures.truncate(600);
    assert!(n == 0, "{n} mismatches:\n{}", failures.join("\n"));
}

/// `FUN_801DCEAC(code, ..)` run live, when the party is asked for or any
/// monster's word is up (a row hidden whole has no aim).
fn group_aim(cpu: &mut Cpu, code: u8, any_up: bool) -> Option<GroupAim> {
    (code == 8 || any_up).then(|| {
        assert_eq!(
            cpu.call(AIM, &[u32::from(code), OUT_CENTRE, OUT_EXTENT]),
            Stop::Returned
        );
        GroupAim {
            centroid_x: cpu.mem.rd16(OUT_CENTRE) as i16,
            centroid_z: cpu.mem.rd16(OUT_CENTRE + 4) as i16,
            extent: cpu.mem.rd16(OUT_EXTENT + 4) as i16,
        }
    })
}

/// A part of seat 7 the shadow leaves out on some arms.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CreatureGap {
    /// The ground trio.
    Trio,
    /// The animation rate `+0x21D`.
    Rate,
}

/// The arms whose creature seat the shadow cannot compare whole, as
/// `(entry, arms, the part, why)`; seat 7 is compared on every other pass
/// of every module.
const CREATURE_GAPS: [(u32, std::ops::RangeInclusive<u8>, CreatureGap, &str); 4] = [
    (
        905,
        0xA..=0xA,
        CreatureGap::Trio,
        "PROT 0905's arm 0xA rise (`+0x36 -= p / 2`, 0x801F7FF0) lands through the \
         camera nudge, which the host applies on the next tick's drift seam",
    ),
    (
        908,
        8..=8,
        CreatureGap::Trio,
        "the walk stands the shadow's creature on the victim for PROT 0908's arm 8 \
         range poll (above)",
    ),
    (
        922,
        7..=7,
        CreatureGap::Rate,
        "PROT 0922's arm 7 parks the rate once the clip cursor `+0x68` reads 0x1C0, \
         which the shadow's seat (no clip player) does not carry",
    ),
    (
        927,
        12..=18,
        CreatureGap::Rate,
        "PROT 0927's arms 12 and 14 freeze the rate once the clip cursor `+0x68` reads \
         0x240 / 0x2D0 (0x801F70F0, 0x801F724C), which the shadow's seat does not carry",
    ),
];

/// Whether `part` of seat 7 is out of the comparison on PROT `entry`'s `arm`.
fn creature_gap(entry: u32, arm: u8, part: CreatureGap) -> bool {
    CREATURE_GAPS
        .iter()
        .any(|(e, arms, p, _)| *e == entry && arms.contains(&arm) && *p == part)
}

/// A world in battle seated as `cpu`'s actor records are, the creature on
/// seat 7, with `action`'s summon armed (`World::arm_summon_stager`).
/// The frame steps every walk runs at.
const WALK_STEPS: [u8; 4] = [1, 2, 3, 4];

/// The arms left out of both walks (this one and `cast_theeder`'s tick
/// bodies) at frame steps above 1 only, as `(entry, first arm, last arm)`;
/// at step 1 every arm is compared. None is left out: Aluru's arm `0x14` and
/// Barra's arm `0xF` wait on a dead victim's colour word alone, as retail's
/// do (`0x801F8870` in PROT 0916, `0x801F81A4` in PROT 0917), where the port
/// had also let go on the down clip, which above step 1 came in ahead of the
/// fade's end.
pub(crate) const STEP_GAPS: [(u32, u8, u8); 0] = [];

/// Whether `(entry, arm)` at `step` is a [`STEP_GAPS`] row (`arm` `None`:
/// any of the entry's rows, for a message naming no arm).
pub(crate) fn step_gap(entry: u32, arm: Option<u8>, step: u8) -> bool {
    step > 1
        && STEP_GAPS
            .iter()
            .any(|&(e, lo, hi)| e == entry && arm.is_none_or(|a| (lo..=hi).contains(&a)))
}

/// [`step_gap`] read off a walk's mismatch line: `PROT NNNN`, `arm 0x..`
/// and `step N` (or `[step N]`) in it.
pub(crate) fn step_gapped(msg: &str) -> bool {
    let num = |key: &str, radix: u32| -> Option<u32> {
        let i = msg.find(key)? + key.len();
        let digits: String = msg[i..].chars().take_while(|c| c.is_digit(radix)).collect();
        u32::from_str_radix(&digits, radix).ok()
    };
    let (Some(entry), Some(step)) = (
        num("PROT ", 10),
        num("[step ", 10).or_else(|| num(" step ", 10)),
    ) else {
        return false;
    };
    step_gap(entry, num("arm 0x", 16).map(|a| a as u8), step as u8)
}

fn summon_shadow(
    cpu: &mut Cpu,
    layout: Layout,
    action: u8,
    live: Option<&[u8]>,
) -> Option<legaia_engine_core::world::World> {
    let mut w = legaia_engine_core::world::World::new();
    // The shadow plays the retail-faithful mode, which every comparison
    // child runs (the Stoned-seat rescale among its switches).
    w.toggles.pin_retail();
    let mut party = legaia_save::Party::zeroed(3);
    if live.is_some() {
        // The caster's spell list as [`seed_live`]'s record holds it.
        let c = layout.caster as usize;
        let mut list = party.members[c].spell_list();
        list.ids[0] = action;
        list.levels[0] = LIVE_LEVEL;
        party.members[c].set_spell_list(list);
    }
    w.load_party(party);
    w.enter_battle(3, layout.monsters as u8);
    while w.actors.len() < 12 {
        w.actors.push(Default::default());
    }
    for s in 0..8u32 {
        let a = ACTOR + s * STRIDE;
        let p = trio(&mut cpu.mem, a + 0x34);
        let act = &mut w.actors[s as usize];
        // The creature's seat is out until the host seats it
        // (`seat_creature`).
        act.active = s < 3 + layout.monsters;
        act.battle.hp = cpu.mem.rd16(a + 0x14C);
        act.battle.max_hp = cpu.mem.rd16(a + 0x14E).max(act.battle.hp);
        act.battle.liveness = u16::from(act.battle.hp > 0);
        act.battle.render_color = cpu.mem.rd32(a + 4);
        act.battle.render_flag = cpu.mem.rd8(a + 0x21C);
        act.battle.facing_angle = cpu.mem.rd16(a + 0x46);
        act.battle.active_target = cpu.mem.rd8(a + 0x1DD);
        act.battle.anim_rate =
            legaia_engine_battle_vm::battle_anim_rate::AnimRate(cpu.mem.rd8(a + 0x21D));
        // The render node's body radius `+0x58`, as the walk seats it.
        act.battle.render_radius = Some(cpu.mem.rd16(NODES + s * 0x100 + 0x58) as i16);
        act.move_state.world_x = p[0];
        act.move_state.world_y = p[1];
        act.move_state.world_z = p[2];
        if (3..3 + layout.monsters).contains(&s) {
            act.battle_monster_id = Some(1);
        }
    }
    if let Some(overlay) = live {
        // The wrapper's tables and every seat's stats as [`seed_live`]
        // wrote them; the creature's are the summon body's record.
        w.tables.element_affinity = legaia_game_tables::element_affinity::parse(overlay);
        w.tables.move_power =
            legaia_engine_battle::move_power::MovePowerCatalog::from_overlay_0898(overlay);
        for s in 0..8u32 {
            let a = ACTOR + s * STRIDE;
            let (atk, udf, ldf, int) = (
                cpu.mem.rd16(a + 0x158),
                cpu.mem.rd16(a + 0x15C),
                cpu.mem.rd16(a + 0x160),
                cpu.mem.rd16(a + 0x168),
            );
            let i = s as usize;
            if let Some(v) = w.battle.accuracy.get_mut(i) {
                *v = int;
            }
            if let Some(v) = w.battle.attack.get_mut(i) {
                *v = atk;
            }
            if let Some(v) = w.battle.defense_split.get_mut(i) {
                *v = Some((udf, ldf));
            }
            if (3..3 + layout.monsters).contains(&s) {
                w.actors[i].battle_element = Some(cpu.mem.rd8(MONSTER_RECORD + 0x1D));
            }
        }
        let a = ACTOR + CREATURE * STRIDE;
        let mut def = legaia_engine_battle::monster_catalog::MonsterDef::new(
            0x100,
            "body",
            cpu.mem.rd16(a + 0x14C),
            cpu.mem.rd16(a + 0x158),
        );
        def.intel = cpu.mem.rd16(a + 0x168);
        def.element = cpu.mem.rd8(MONSTER_RECORD + 0x1D);
        w.tables.summon_creatures.insert(action, def);
    }
    w.battle_ctx.active_actor = layout.caster as u8;
    w.casting.summon_actor_slot = Some(CREATURE as u8);
    // The creature carries a clip set, so the clip it plays (`+0x1D9`, the
    // walk's synced each pass) reaches the module half's clip gates
    // (`World::feed_module_creature`) as the host's seated creature's does.
    w.actors[CREATURE as usize].battle_action_clips = Some(std::sync::Arc::new(vec![None; 8]));
    // The walk's battle frame (`walk_step`): every shadow tick opens a
    // pass, as each of retail's calls does.
    w.battle.frame_clock.step = walk_step();
    w.battle.frame_clock.span_extra = 0;
    // The reaction run `+0x1EF..+0x1F2`, as the records hold it: a seat
    // with no clips reads it off its action bytes `+0x1DF..`.
    for s in 3..(3 + layout.monsters).min(7) {
        let a = ACTOR + s * STRIDE;
        for o in 0x1EFu32..=0x1F2 {
            w.actors[s as usize].battle.params[(o - 0x1DF) as usize] = cpu.mem.rd8(a + o);
        }
    }
    w.cast_module_for(action)?;
    w.arm_summon_stager(layout.caster as u8, action);
    // The cast the band's fold owes, as the Magic submenu arms it
    // (`World::arm_player_cast`): the spell record off the executable, on
    // the targets its shape names from the walk's target code.
    static CATALOG: std::sync::OnceLock<Option<legaia_engine_battle::spells::SpellCatalog>> =
        std::sync::OnceLock::new();
    let catalog = CATALOG
        .get_or_init(|| {
            crate::common::scus()
                .and_then(|s| legaia_engine_battle::retail_magic::seru_magic_catalog_from_scus(&s))
        })
        .clone()?;
    if let Some(def) = catalog.get(action).cloned() {
        use legaia_engine_battle::spells::SpellTarget;
        let code = cpu.mem.rd8(ACTOR + layout.caster * STRIDE + 0x1DD);
        let targets: Vec<u8> = match def.target {
            SpellTarget::AllEnemies => (3..3 + layout.monsters as u8).collect(),
            SpellTarget::AllAllies => (0..3).collect(),
            _ if code < 8 => vec![code],
            _ => vec![layout.victim as u8],
        };
        w.casting.pending_cast = Some(legaia_engine_battle_loop::cast_band::PendingCast {
            caster: layout.caster as u8,
            spell_id: action,
            targets,
            on_flight: false,
        });
    }
    // PROT 0927's stager sweeps (arms 3 and 4 of `FUN_801F85A8`, which its
    // parts call) spend a party cast's fold; the walk runs no part, so the
    // shadow's cast arrives at arm 26 with the fold already spent.
    if w.cast_module_for(action) == Some(927) {
        w.casting.pending_cast = None;
    }
    w.set_spell_catalog(catalog);
    // PROT 0916's verdict: the walk's director runs the damage path (its
    // `kill` left unset), which is the path retail's roll takes on this
    // walk's zeroed records; the world's roll reads its own party's levels.
    w.casting.module_cam.kill = Some(false);
    // The creature seat as the host installs it (`FUN_801F19EC`'s port): a
    // module with no seat arm before its first pass, as the stager's armed
    // phase seats it; the rest once the band's pass reaches the seat arm
    // its profile names (`module_seat_arm::seat_due`), as the stager's owed
    // seat lands.
    if cam::module_profile(w.cast_module_for(action)?)
        .and_then(|p| p.seat_arm)
        .is_none()
    {
        seat_creature(cpu, &mut w);
    }
    Some(w)
}

/// The host's seat of the creature (`World::seat_summon_actor`), its ground
/// and facing then put back on the walk's: the walk stands the creature
/// where retail's arms leave it, and the shadow syncs that each pass.
fn seat_creature(cpu: &mut Cpu, w: &mut legaia_engine_core::world::World) {
    w.seat_summon_actor(CREATURE as usize);
    let a = ACTOR + CREATURE * STRIDE;
    let p = trio(&mut cpu.mem, a + 0x34);
    let act = &mut w.actors[CREATURE as usize];
    [
        act.move_state.world_x,
        act.move_state.world_y,
        act.move_state.world_z,
    ] = p;
    act.battle.facing_angle = cpu.mem.rd16(a + 0x46);
}

/// A part of the camera the port is known to leave out.
#[derive(Clone, Copy)]
enum Gap {
    /// The roll angle `0x8007B794`: the engine's battle camera has no roll.
    Roll,
    /// The shot of the pass that leaves the arm, which another band makes.
    ExitShot,
}

/// The arms whose camera the port is known to leave a part of out, as
/// `(entry, arms, the part, why)`; the rest of each pass is compared.
const KNOWN_GAPS: [(u32, &[u8], Gap, &str); 2] = [
    (
        928,
        &[3, 4],
        Gap::Roll,
        "PROT 0928's sweep stores a roll the battle camera has no channel for",
    ),
    (
        925,
        &[8],
        Gap::ExitShot,
        "PROT 0925's arm 8 stages the flee, whose reverse-angle tween (0x801F78C8) is \
         the run band's (documented on spikefish_direct)",
    ),
];

/// A 12-bit angle as a signed one.
fn sign12(a: i32) -> i32 {
    let a = a & 0xFFF;
    if a >= 0x800 { a - 0x1000 } else { a }
}

/// The side-band arm of PROT 0904 (arm 1) and PROT 0906 (arm 2), whose tick
/// bodies own their phase: each image's arm run once with the band word
/// `ctx[+0x6D8]` out and once with it standing, beside a shadow world's
/// module pass on the same arm, and the phase each leaves compared.
#[test]
fn the_side_band_arm_steps_over_the_band_word_poll_as_retail_does() {
    use legaia_engine_cast::seru_magic_ticks::{GIZAM_TICK, THEEDER_TICK};
    differential!(prot(904, 0x801F_69D8), prot(906, 0x801F_69F4));
    let (Some(_), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = disc.read_entry(898).expect("PROT 0898");
    let layout = LAYOUTS[0];
    let mut compared = 0;
    // At frame steps 1 to 3 (`WALK_STEP`): the arm's band-word test is once
    // a pass.
    let runs = [1u8, 2, 3].into_iter().flat_map(|step| {
        [(904u32, THEEDER_TICK, 1u8), (906, GIZAM_TICK, 2)]
            .into_iter()
            .flat_map(move |b| [0u16, 0x40].map(|word| (b, word, step)))
    });
    for ((entry, va, arm), word, step) in runs {
        WALK_STEP.with(|s| s.set(step));
        {
            let module = disc.read_entry(entry as usize).expect("module");
            let action = action_of(entry);
            let mut cpu = scus_cpu().expect("SCUS");
            cpu.mem.load(OVERLAY_BASE, &overlay);
            cpu.mem.wr32(CTX_PTR, CTX);
            cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
            cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
            for target in jal_targets(&module) {
                if ![BEARING, RAND].contains(&target) {
                    cpu.stub(target, 0);
                }
            }
            cpu.hook(RAND, |r, _| r.r[V0] = 0);
            cpu.hook(BEARING, |r, _| {
                let a = |i: usize| r.r[A0 + i] as i16;
                r.r[V0] = u32::from(bearing_12bit_approx(a(0), a(1), a(2), a(3)));
            });
            seed(&mut cpu, &module, action, layout.code(9), layout);
            let Some(mut w) = summon_shadow(&mut cpu, layout, action, None) else {
                panic!("PROT {entry:04}: no shadow");
            };
            cpu.mem.wr8(CTX + 0x279, arm);
            cpu.mem.wr8(CTX + 0x276, 0);
            cpu.mem.wr16(CTX + 0x6D8, word);
            w.casting.module_phase = arm;
            w.battle_ctx.frame_timer = word as i16 as _;
            w.casting.band_word_live = None;
            w.casting.module_cam.sideband_busy = false;
            w.drive_summon_module_pass(layout.caster as u8, action, true);
            assert_eq!(cpu.call(va, &[]), Stop::Returned, "PROT {entry:04}");
            let retail = cpu.mem.rd8(CTX + 0x279);
            assert_eq!(
                w.casting.module_phase, retail,
                "PROT {entry:04} arm {arm} band word {word:#x} step {step}: port phase vs retail"
            );
            compared += 1;
        }
    }
    WALK_STEP.with(|s| s.set(1));
    assert_eq!(compared, 12);
}

/// PROT 0925's arm 8 on a scripted fight, which takes the round tail
/// (`bne` on `ctx[+0x287]` at `0x801F73F0` to `0x801F79B8`) in place of the
/// flee: the image's arm run over party rows of one to three seats, a member
/// down, members with an item turn that had or had not a turn left (`+0x1DE`,
/// `+0x16C`), against `spikefish_round_tail` - the turn cursor `ctx[+0x1A]`,
/// every party seat's `+0x16C` and the item refunds `FUN_800421D4(item, 1)` in
/// order.
///
/// Each row runs at frame steps 1 to 4 on countdown words either side of the
/// pass's drain: the arm drains its word by the bare step (`subu` / `bgtz`
/// at `0x801F737C..0x801F7380`) after adding `step << 3` to the creature's
/// shatter word (`0x801F7344..0x801F7358`). The shadow runs the pass as the
/// host does (`World::module_pass`): `step` ticks of the director
/// `spikefish_direct`, the countdown's rest `step - 1 - i`, stopping on the
/// tick the gate lets through, each tick adding `spikefish_shatter_add`.
/// Compared: held or passed, the re-armed word, and the shatter word.
#[test]
fn the_spikefish_round_tail_counts_and_refunds_as_retail_does() {
    use legaia_engine_cast::cast_module_camera::summon_holds::spikefish_direct;
    use legaia_engine_cast::cast_module_ticks::{
        CastActorState, CastModuleCtx, spikefish_round_tail, spikefish_shatter_add,
    };
    differential!(prot(925, 0x801F_6A00));
    let (Some(_), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const REFUND: u32 = 0x8004_21D4;
    const COUNTDOWN: u32 = 0x801F_800C;
    let overlay = disc.read_entry(898).expect("PROT 0898");
    let module = disc.read_entry(925).expect("PROT 0925");
    let (mut compared, mut held, mut early) = (0, 0, 0);
    // `(hp, +0x1DE, +0x16C, +0x1DF)` per seat.
    let rows: [&[(u16, u8, u16, u8)]; 5] = [
        &[(300, 0, 0, 0)],
        &[(300, 1, 7, 0x2A), (0, 1, 3, 0x2B), (120, 4, 9, 0x2C)],
        &[(300, 1, 0, 0x2A), (40, 0, 5, 0), (0, 0, 0, 0)],
        &[(0, 1, 2, 0x31), (300, 1, 1, 0x32)],
        &[(300, 3, 4, 0), (300, 1, 6, 0x40), (300, 1, 0, 0x41)],
    ];
    for (k, row) in rows.iter().enumerate() {
        for step in 1..=4u8 {
            for word in [-3i32, 0, 1, 2, 3, 4, 5] {
                let c = format!("row {k} step {step} word {word}");
                let mut cpu = scus_cpu().expect("SCUS");
                cpu.mem.load(OVERLAY_BASE, &overlay);
                cpu.mem.load(BASE, &module);
                cpu.mem.wr32(CTX_PTR, CTX);
                cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
                cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
                for target in jal_targets(&module) {
                    cpu.stub(target, 0);
                }
                let refunds = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
                {
                    let refunds = refunds.clone();
                    cpu.hook(REFUND, move |r, _| {
                        refunds.borrow_mut().push((r.r[A0], r.r[A1]));
                        r.r[V0] = 0;
                    });
                }
                for b in 0..0x1100 {
                    cpu.mem.wr8(CTX + b, 0);
                }
                cpu.mem.wr8(CTX, row.len() as u8);
                cpu.mem.wr8(CTX + 1, 2);
                cpu.mem.wr8(CTX + 0x1A, 5);
                cpu.mem.wr8(CTX + 0x279, 8);
                cpu.mem.wr8(CTX + 0x287, 1);
                cpu.mem.wr32(COUNTDOWN, word as u32);
                cpu.mem.wr8(SCALAR, SPEED_SCALAR as u8);
                cpu.mem.wr8(STEP, step);
                for s in 0..8u32 {
                    let a = ACTOR + s * STRIDE;
                    cpu.mem.wr32(ACTORS + s * 4, a);
                    for o in (0..STRIDE).step_by(4) {
                        cpu.mem.wr32(a + o, 0);
                    }
                    let node = NODES + s * 0x100;
                    cpu.mem.wr32(a + 0x22C, node);
                }
                let shatter0 = 0x40u16;
                cpu.mem.wr16(ACTOR + CREATURE * STRIDE + 0x1BA, shatter0);
                let mut party: Vec<CastActorState> = Vec::new();
                for (s, &(hp, cat, key, item)) in row.iter().enumerate() {
                    let a = ACTOR + s as u32 * STRIDE;
                    cpu.mem.wr16(a + 0x14C, hp);
                    cpu.mem.wr8(a + 0x1DE, cat);
                    cpu.mem.wr16(a + 0x16C, key);
                    cpu.mem.wr8(a + 0x1DF, item);
                    party.push(CastActorState {
                        hp,
                        action_category: cat,
                        init_key: key,
                        queued_action: item,
                        ..Default::default()
                    });
                }
                assert_eq!(cpu.call(SPIKEFISH, &[]), Stop::Returned, "{c}");
                // The host's pass.
                let mut st = ModuleCamState {
                    frame_step: step,
                    ..ModuleCamState::default()
                };
                st.countdown.0 = word;
                let (mut shatter, mut passed) = (shatter0, false);
                for i in 0..step {
                    st.countdown.1 = step - 1 - i;
                    st.opens_pass = i == 0;
                    let dir = spikefish_direct(&mut st, 8, ModuleCamSeats::default());
                    shatter =
                        shatter.wrapping_add(spikefish_shatter_add(!dir.hold, st.countdown.1));
                    if !dir.hold {
                        passed = true;
                        early += usize::from(i + 1 < step);
                        break;
                    }
                }
                let retail_shatter = cpu.mem.rd16(ACTOR + CREATURE * STRIDE + 0x1BA);
                assert_eq!(retail_shatter, shatter, "{c}: the creature's shatter word");
                assert_eq!(
                    cpu.mem.rd32(COUNTDOWN) as i32,
                    st.countdown.0,
                    "{c}: the countdown"
                );
                let left = cpu.mem.rd8(CTX + 0x279) == 9;
                assert_eq!(left, passed, "{c}: retail leaves {left}, port {passed}");
                if !passed {
                    assert!(refunds.borrow().is_empty(), "{c}: a held pass refunds");
                    assert_eq!(cpu.mem.rd8(CTX + 0x1A), 5, "{c}: a held pass counts");
                    held += 1;
                    continue;
                }
                let mut ctx = CastModuleCtx {
                    turn_cursor: 5,
                    ..Default::default()
                };
                let port_refunds = spikefish_round_tail(&mut ctx, &mut party);
                assert_eq!(cpu.mem.rd8(CTX + 0x1A), ctx.turn_cursor, "{c}: turn cursor");
                for (s, seat) in party.iter().enumerate() {
                    let a = ACTOR + s as u32 * STRIDE;
                    assert_eq!(
                        cpu.mem.rd16(a + 0x16C),
                        seat.init_key,
                        "{c} seat {s}: +0x16C"
                    );
                }
                let retail: Vec<(u32, u32)> = refunds.borrow().clone();
                let port: Vec<(u32, u32)> =
                    port_refunds.iter().map(|&i| (u32::from(i), 1)).collect();
                assert_eq!(retail, port, "{c}: refunds");
                compared += 1;
            }
        }
    }
    assert!(
        compared > 60 && held > 20 && early > 10,
        "{compared} {held} {early}"
    );
}

/// PROT 0903's settle arm 12 (`0x801F7628..0x801F76D8`) at frame steps 1 to
/// 4, on countdown words either side of the pass's drain and a victim
/// standing idle or not, fallen faded or not: retail drains `0x801F7960` by
/// `scalar * step` while it is positive, polls the settle once it is spent
/// (storing `0xFF`), and on every pass, the leaving one included, adds
/// `2 * step * scalar` to the creature's shatter word `+0x1BA`. The shadow
/// runs the pass as the host does: `step` ticks of `gimard_direct`, the
/// countdown's rest, the settle on the tick the gate lets through, each
/// tick adding `gimard_shatter_add`.
#[test]
fn gimards_settle_arm_breaks_the_creature_up_as_retail_does() {
    use legaia_engine_cast::cast_module_camera::gimard_direct;
    use legaia_engine_cast::seru_magic_ticks::gimard_shatter_add;
    differential!(prot(903, 0x801F_69D8));
    let (Some(_), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const COUNTDOWN: u32 = 0x801F_7960;
    const VICTIM: u32 = 4;
    let overlay = disc.read_entry(898).expect("PROT 0898");
    let module = disc.read_entry(903).expect("PROT 0903");
    let (mut compared, mut early) = (0usize, 0usize);
    for step in 1..=4u8 {
        for word in [-8i32, 0, 1, 8, 15, 16, 24, 31, 32, 40] {
            // (hp, clip, colour word): standing idle, standing busy, fallen
            // faded, fallen lit.
            for (hp, clip, colour) in [
                (100u16, 0u8, 0x2008_0200u32),
                (100, 3, 0x2008_0200),
                (0, 8, 0),
                (0, 8, 0x2008_0200),
            ] {
                let c = format!("step {step} word {word} hp {hp} clip {clip} word4 {colour:#x}");
                let mut cpu = scus_cpu().expect("SCUS");
                cpu.mem.load(OVERLAY_BASE, &overlay);
                cpu.mem.load(BASE, &module);
                cpu.mem.wr32(CTX_PTR, CTX);
                cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
                cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
                for target in jal_targets(&module) {
                    cpu.stub(target, 0);
                }
                for b in 0..0x1100 {
                    cpu.mem.wr8(CTX + b, 0);
                }
                cpu.mem.wr8(CTX, 3);
                cpu.mem.wr8(CTX + 1, 2);
                cpu.mem.wr8(CTX + 0x13, 0);
                cpu.mem.wr8(CTX + 0x279, 12);
                cpu.mem.wr32(COUNTDOWN, word as u32);
                cpu.mem.wr8(SCALAR, SPEED_SCALAR as u8);
                cpu.mem.wr8(STEP, step);
                for s in 0..8u32 {
                    let a = ACTOR + s * STRIDE;
                    cpu.mem.wr32(ACTORS + s * 4, a);
                    for o in (0..STRIDE).step_by(4) {
                        cpu.mem.wr32(a + o, 0);
                    }
                    cpu.mem.wr16(a + 0x14C, 100);
                    cpu.mem.wr32(a + 0x22C, NODES + s * 0x100);
                }
                cpu.mem.wr8(ACTOR + 0x1DD, VICTIM as u8);
                let va = ACTOR + VICTIM * STRIDE;
                cpu.mem.wr16(va + 0x14C, hp);
                cpu.mem.wr8(va + 0x1D9, clip);
                cpu.mem.wr32(va + 4, colour);
                let shatter0 = 0x100u16;
                cpu.mem.wr16(ACTOR + CREATURE * STRIDE + 0x1BA, shatter0);
                assert_eq!(cpu.call(0x801F_69D8, &[]), Stop::Returned, "{c}");
                // The host's settle off the same bytes: a standing victim
                // idle, a fallen one faded out.
                let settled = if hp != 0 { clip == 0 } else { colour == 0 };
                let mut st = ModuleCamState {
                    frame_step: step,
                    ..ModuleCamState::default()
                };
                st.countdown.0 = word;
                let (mut shatter, mut left) = (shatter0, false);
                for i in 0..step {
                    st.countdown.1 = step - 1 - i;
                    st.opens_pass = i == 0;
                    let held = gimard_direct(&mut st, 12, ModuleCamSeats::default()).hold;
                    let leaving = !held && settled;
                    shatter = shatter.wrapping_add(gimard_shatter_add(leaving, st.countdown.1));
                    if leaving {
                        left = true;
                        early += usize::from(i + 1 < step);
                        break;
                    }
                }
                let retail_left = cpu.mem.rd8(CTX + 0x279) == 0xFF;
                assert_eq!(retail_left, left, "{c}: leaves");
                assert_eq!(
                    cpu.mem.rd16(ACTOR + CREATURE * STRIDE + 0x1BA),
                    shatter,
                    "{c}: the creature's shatter word"
                );
                assert_eq!(
                    cpu.mem.rd32(COUNTDOWN) as i32,
                    st.countdown.0,
                    "{c}: the countdown"
                );
                compared += 1;
            }
        }
    }
    assert!(compared == 160 && early > 10, "{compared} {early}");
}

/// PROT 0933's orbit radii (arms 9 and 0xB, `0x801F7658..0x801F76F4` and
/// `0x801F79D8..0x801F7A74`) over every monster seat of both seat families
/// (`legaia_engine_battle::battle_seats`): the arm takes `a = ratan2(z', x)`
/// of the seat its arm 0 copied (`z' = z - 0x613`, 0x801F6C9C), then
/// `|sin(a) * x| + |cos(a) * z'|`. Every authored seat stands at `z` from 600
/// to 1400, so `z'` is negative and `cos(a) * z'` is too: the rounding
/// fix-up's `bgez` and the `abs`'s `bgez` on that product (0x801F76D4,
/// 0x801F76E8, 0x801F7A54, 0x801F7A68) only fall through. The run reads
/// both products off SCUS's own `ratan2` and tables.
#[test]
fn terras_orbit_products_keep_their_sign_on_every_seat() {
    use legaia_engine_battle::battle_seats::{MONSTER_SEATS, MONSTER_SEATS_ALT};
    not_a_differential!(
        "a reachability measure: retail's ratan2 and tables over the authored seats, no port side"
    );
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
    let mut seats = 0usize;
    for (family, table) in [MONSTER_SEATS, MONSTER_SEATS_ALT].iter().enumerate() {
        for (row, cells) in table.iter().enumerate() {
            for seat in cells.iter().take(row + 1) {
                let (x, z) = (i32::from(seat.x), i32::from(seat.z) - 0x613);
                assert_eq!(cpu.call(0x8001_9B28, &[z as u32, x as u32]), Stop::Returned);
                let a = cpu.regs.r[V0] as i16 as i32;
                let sin = cpu.mem.rd16(0x8007_0A2Cu32.wrapping_add((2 * a) as u32)) as i16 as i32;
                let cos = cpu.mem.rd16(0x8007_122Cu32.wrapping_add((2 * a) as u32)) as i16 as i32;
                let p = cos * z;
                assert!(
                    p < 0 && p >> 12 < 0,
                    "family {family} row {row} seat ({x}, {z}): a {a:#x} sin {sin} cos {cos}"
                );
                seats += 1;
            }
        }
    }
    assert_eq!(seats, 20);
}
