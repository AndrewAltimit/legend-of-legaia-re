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
//! its own differential) and the group aim `FUN_801DCEAC` (run live). The
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
    SPEED_SCALAR,
};
use legaia_engine_cast::cast_module_rumble::{
    MODULE_RUMBLE, RumbleStore, module_rumble_due, ozma_spray,
};
use legaia_engine_system::rumble::BattleRumbleWrite;
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
/// target kind is pinned by no capture; they run on a seat. The other paced
/// directors (PROT 0903, 0904, 0906, 0909..0913, 0918, 0919, 0922) hand
/// their holds to the tick body's own port and arm a passed arm's shot on
/// the body's next tick (`ModuleProfile::body_paced`), which a walk of the
/// director alone cannot pace.
const PACED: [(u32, u32, u8); 6] = [
    (905, VERA, 2),
    (907, NIGHTO, 3),
    (908, ZENOIR, 3),
    (924, ULTIMATE_RAVE, 3),
    (925, SPIKEFISH, 3),
    (927, JUGGERNAUT, 9),
];

const VERA: u32 = 0x801F_69D8;
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

/// A summon's spell id: PROT 0914 casts `0x8C`, one id an image up.
fn action_of(entry: u32) -> u8 {
    if entry < 914 {
        (0x81 + entry - 903) as u8
    } else {
        (0x8C + entry - 914) as u8
    }
}

fn seed(cpu: &mut Cpu, module: &[u8], action: u8, code: u8) {
    cpu.mem.load(BASE, module);
    for b in 0..0x1100 {
        cpu.mem.wr8(CTX + b, 0);
    }
    cpu.mem.wr8(CTX, 3);
    cpu.mem.wr8(CTX + 1, MONSTERS as u8);
    cpu.mem.wr8(CTX + 0x13, CASTER as u8);
    cpu.mem.wr16(CTX + 0x6D0, 0x600);
    for s in 0..8u32 {
        let a = ACTOR + s * STRIDE;
        cpu.mem.wr32(ACTORS + s * 4, a);
        for o in (0..STRIDE).step_by(4) {
            cpu.mem.wr32(a + o, 0);
        }
        let live = s < 3 || (3..3 + MONSTERS).contains(&s) || s == CREATURE;
        let (x, z) = match s {
            0..=2 => (-0x200 + 0x200 * s as i16, 0x600),
            7 => (0, 0),
            _ => (-0x180 + 0x300 * (s as i16 - 3), -0x600),
        };
        for (o, v) in [(0x34u32, x), (0x38, z), (0x3C, x), (0x40, z)] {
            cpu.mem.wr16(a + o, v as u16);
        }
        cpu.mem.wr16(a + 0x46, if s < 3 { 0x800 } else { 0 });
        cpu.mem.wr16(a + 0x14C, if live { 300 } else { 0 });
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
    cpu.mem.wr8(ACTOR + CASTER * STRIDE + 0x1DD, code);
    cpu.mem.wr8(ACTOR + CREATURE * STRIDE + 0x1DD, code);
    for i in 0..5 {
        cpu.mem.wr32(RECORDS + i * 4, MONSTER_RECORD);
    }
    cpu.mem.wr8(SCALAR, SPEED_SCALAR as u8);
    cpu.mem.wr8(STEP, 1);
    for i in 0..5 {
        cpu.mem
            .wr8(FORMATION + i, if i < MONSTERS { 0x10 } else { 0 });
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
        prot(927, JUGGERNAUT)
    );
    let (Some(_), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = disc.read_entry(898).expect("PROT 0898");
    let mut failures = Vec::new();
    let all: Vec<(u32, u32, u8)> = BODIES.iter().chain(&PACED).copied().collect();
    for (entry, va, code) in all {
        let module = disc.read_entry(entry as usize).expect("module");
        let action = action_of(entry);
        let profile = cam::module_profile(entry).expect("directed");
        let shots = Rc::new(RefCell::new(Vec::<Shot>::new()));
        let mut cpu = scus_cpu().expect("SCUS");
        cpu.mem.load(OVERLAY_BASE, &overlay);
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
        cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
        for target in jal_targets(&module) {
            if ![AIM, BUILDER, BEARING, RAND].contains(&target) {
                cpu.stub(target, 0);
            }
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
        {
            seed(&mut cpu, &module, action, code);
            let mut st = ModuleCamState {
                lead_roster: 1,
                opens_pass: true,
                frame_step: 1,
                victim_settled: true,
                caster_in_reach: true,
                party_settled: true,
                row_settled: true,
                creature_arrived: true,
                creature_in_range: Some(true),
                ..ModuleCamState::default()
            };
            let caster_seat = seat_at(&mut cpu.mem, CASTER, false);
            let victim = if code < 8 { u32::from(code) } else { VICTIM };
            let mut max_roll = 0;
            let mut rumbles = 0u32;
            let mut bad = 0usize;
            let mut finished = false;
            let mut path = Vec::new();
            for pass in 0..20_000u32 {
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
                    // its park by the next pass.
                    if s == CREATURE && cpu.mem.rd8(a + 0x21B) == 0xFF {
                        cpu.mem.wr8(a + 0x21B, 0);
                    }
                    let staged = cpu.mem.rd8(a + 0x1DA);
                    cpu.mem
                        .wr8(a + 0x1D9, if s == CREATURE { staged } else { 0 });
                    cpu.mem.wr16(NODES + s * 0x100 + 0x68, 0x7FFF);
                    for o in [0u32, 2, 4] {
                        let v = cpu.mem.rd16(a + 0x34 + o);
                        cpu.mem.wr16(a + 0x3C + o, v);
                    }
                }
                let phase = cpu.mem.rd8(CTX + 0x279);
                // PROT 0931's arm `0x16` waits for its creature's cursor to stand
                // on `0x120` exactly (`0x801F7C80`).
                if (entry, phase) == (931, 0x16) {
                    cpu.mem.wr16(NODES + CREATURE * 0x100 + 0x68, 0x120);
                }
                // The fades that ease the creature's colour word `+0x04` to
                // zero are stubbed; PROT 0915's arm `0xD` and PROT 0917's
                // `0xF` wait on it.
                if matches!((entry, phase), (915, 0xD) | (917, 0xF)) {
                    cpu.mem.wr32(ACTOR + CREATURE * STRIDE + 4, 0);
                }
                let t0 = Target::read(&mut cpu.mem);
                let live_code = cpu.mem.rd8(ACTOR + CASTER * STRIDE + 0x1DD);
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
                let mut group = |code: u8| {
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
                };
                st.row_aim = group(9);
                st.target_aim = if live_code < 8 {
                    let s = seat_at(&mut cpu.mem, u32::from(live_code), false);
                    Some(GroupAim {
                        centroid_x: s.x.wrapping_neg(),
                        centroid_z: s.z.wrapping_neg(),
                        extent: MIN_GROUP_EXTENT,
                    })
                } else {
                    group(live_code)
                };
                // `keep_row_tr_y`: `-4 * +0x3E` averaged over the living row.
                let mut row = Vec::new();
                for s in 3..3 + MONSTERS {
                    let a = ACTOR + s * STRIDE;
                    if cpu.mem.rd16(a + 0x14C) != 0 {
                        row.push(-4 * i32::from(cpu.mem.rd16(a + 0x3E) as i16));
                    }
                }
                if !row.is_empty() {
                    st.row_tr_y = (row.iter().sum::<i32>() / row.len() as i32) as i16;
                }
                st.sideband_busy = cpu.mem.rd8(CTX + 0x276) != 0;
                let c = CREATURE;
                st.creature_live = st.creature.map(|_| seat_at(&mut cpu.mem, c, false));
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
                    // The engine reads the shake's low bit per four vsyncs (one
                    // battle frame of the arms' captures); this run's pass is one.
                    st.ozma_shake = (
                        cpu.mem.rd32(OZMA_PASS_WORD) * 4,
                        cpu.mem.rd32(OZMA_RISE_WORD) as i32,
                    );
                }
                shots.borrow_mut().clear();
                draws.borrow_mut().clear();
                for w in RUMBLE {
                    cpu.mem.wr32(w, RUMBLE_SENTINEL);
                }
                let countdown_before = st.countdown.0;
                let stop = cpu.call(va, &[]);
                if stop != Stop::Returned {
                    failures.push(format!("PROT {entry:04} pass {pass} arm {phase}: {stop:?}"));
                    break;
                }
                // PROT 0915's chase runs ahead of the director
                // (`World::mushura_chase_ahead`): the director reads the
                // creature as this pass's turn left it.
                if (entry, phase) == (915, 0xC) {
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
                let arm = (profile.direct)(&mut st, phase, seats);
                let mut port = t0;
                port.apply(&arm);
                let busy = cpu.regs.r[V0] != 0;
                let after = cpu.mem.rd8(CTX + 0x279);
                let retail = Target::read(&mut cpu.mem);
                let shot = shots.borrow().last().copied();
                let held = busy && after == phase;
                if !held && path.last() != Some(&phase) {
                    path.push(phase);
                }
                let port_after = if arm.hold {
                    phase
                } else {
                    arm.jump.unwrap_or(phase.wrapping_add(1))
                };
                let mut diffs = Vec::new();
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
                let due: Vec<_> =
                    module_rumble_due(entry, SINGLE_BODY, phase, !held, countdown_before).collect();
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
                if rumble_ok && !due.is_empty() {
                    rumbles += 1;
                }
                if !rumble_ok {
                    diffs.push(format!("rumble retail {stored:?} {rates:?} port {due:?}"));
                }
                // A terminal arm stores no phase; the port stands at `0xFF`.
                let terminal = !busy && (phase == 0xFF || port_after == 0xFF);
                // A director beside a ported tick body leaves the phase to
                // the body: only its hold is its own.
                let walks = profile.director_body;
                if !terminal && (held != arm.hold || (walks && !held && after != port_after)) {
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
                            "PROT {entry:04} target {code} pass {pass} arm {phase:#x}: {}",
                            diffs.join("; ")
                        ));
                    }
                }
                if !busy {
                    finished = true;
                    break;
                }
            }
            // Every carried rumble write of the module was compared.
            let carried = MODULE_RUMBLE.iter().filter(|m| m.entry == entry).count();
            if carried > 0 && rumbles == 0 {
                failures.push(format!("PROT {entry:04}: no rumble write compared"));
            }
            if max_roll != 0 {
                eprintln!("PROT {entry:04}: retail rolls the camera up to {max_roll}");
            }
            eprintln!(
                "PROT {entry:04} target {code}: {bad} differing passes, arms {:x?}{}",
                path,
                if finished { " Done" } else { "" }
            );
            if !finished {
                failures.push(format!(
                    "PROT {entry:04} target {code}: no finish, stuck at arm {:#x}",
                    cpu.mem.rd8(CTX + 0x279)
                ));
            }
        }
    }
    let n = failures.len();
    failures.truncate(600);
    assert!(n == 0, "{n} mismatches:\n{}", failures.join("\n"));
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
