//! PROT 0904's (Theeder) four beam packet builders against
//! `legaia_engine_ui::cast_theeder`: the prong `FUN_801F815C`, the charge
//! beam `FUN_801F83A4`, the sweep `FUN_801F8634` and the retract
//! `FUN_801F8B84`. The billboard projector `FUN_800195A8` is hooked to hand
//! back a generated screen point and depth per world point (the port takes
//! projected points as input), `rand()` is scripted, and every packet the
//! builder writes is decoded and held to the port's primitive: code,
//! corners, colours, texels, CLUT and page. The sweep's ring shift is held
//! to `engine-cast`'s `TheederTrail::sample` on a pass boundary.
//!
//! The ordering-table bucket is not compared: the port draws the overlay's
//! packets over the finished scene at fixed entries (disclosed in
//! `cast_theeder`), where retail links the depth-averaged ones at their
//! ends' mean depth.
//!
//! The file also walks tick bodies whole ([`walk`]): PROT 0904's and 0906's
//! from arm 0 to Done beside the band's module half, and the same walk over
//! the PROT 0920 and 0923 strikes that turn the seats they hit.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_cast::seru_magic_ticks::TheederTrail;
use legaia_engine_ui::cast_theeder::{
    Xy, charge_prims, prong_prim_widened, prong_widen, theeder_prims,
};
use legaia_render_kernels::cast_theeder_packet::TheederPacket;
use legaia_render_kernels::screen_prim::ScreenPrim;
use psx_retail_exec::{A0, Cpu, SP, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const MODULE_BASE: u32 = 0x801F_69D8;
const PRONG: u32 = 0x801F_815C;
const CHARGE: u32 = 0x801F_83A4;
const SWEEP: u32 = 0x801F_8634;
const RETRACT: u32 = 0x801F_8B84;
const PROJECT: u32 = 0x8001_95A8;
const RAND: u32 = 0x8005_6798;
const POOL_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const POOL: u32 = 0x8012_0000;
const OT: u32 = 0x8014_0000;
const OT_LEN: u32 = 0x1400;
/// The ring of trail tips (`hist[0]` newest) and its count word.
const RING: u32 = 0x801F_90FC;
const COUNT: u32 = 0x801F_91FC;
const POINT_A: u32 = 0x8011_0000;
const POINT_B: u32 = 0x8011_0010;

/// The screen point and depth the hooked projector gives a world point.
fn project(p: [i16; 3]) -> (Xy, u32) {
    let xy = (p[0] >> 3, p[2] >> 4);
    // Mostly far (no prong widening), now and then within `0x100`.
    let z = (p[1] as u16 as u32) & 0x3FF;
    (xy, z)
}

fn module_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem.load(
        MODULE_BASE,
        &disc().expect("disc").read_entry(904).expect("PROT 0904"),
    );
    cpu.hook(PROJECT, |r, m| {
        let a = r.r[A0];
        let p = [m.rd16(a) as i16, m.rd16(a + 2) as i16, m.rd16(a + 4) as i16];
        let ((x, y), z) = project(p);
        let out = m.rd32(r.r[SP] + 0x10);
        m.wr16(out, x as u16);
        m.wr16(out + 2, y as u16);
        r.r[V0] = z;
    });
    Some(cpu)
}

fn reset_pool(cpu: &mut Cpu) {
    cpu.mem.wr32(POOL_PTR, POOL);
    cpu.mem.wr32(OT_PTR, OT);
    cpu.mem.load(POOL, &[0xEE; 0x400]);
    for i in 0..OT_LEN {
        cpu.mem.wr32(OT + 4 * i, 0x00FF_FFFF);
    }
}

fn point(rng: &mut Rng) -> [i16; 3] {
    [
        (rng.next() as i16) >> 3,
        rng.next() as i16,
        (rng.next() as i16) >> 3,
    ]
}

fn put(cpu: &mut Cpu, at: u32, p: [i16; 3]) {
    for (k, v) in p.iter().enumerate() {
        cpu.mem.wr16(at + 2 * k as u32, *v as u16);
    }
    cpu.mem.wr16(at + 6, 0xA5A5);
}

/// A textured packet's texels, CLUT and page.
type Texture = ([(u8, u8); 4], u16, u16);

/// A packet as decoded, for both sides.
#[derive(Debug, PartialEq, Eq)]
struct Packet {
    code: u8,
    xy: [(i16, i16); 4],
    rgb: [[u8; 3]; 4],
    uv: Option<Texture>,
}

/// Every packet written since [`reset_pool`]: `POLY_FT4` (`0x2C..0x2F`, ten
/// words) or `POLY_G4` (`0x38..0x3B`, nine).
fn retail_packets(cpu: &mut Cpu) -> Vec<Packet> {
    let end = cpu.mem.rd32(POOL_PTR);
    let mut p = POOL;
    let mut out = Vec::new();
    while p < end {
        let code = cpu.mem.rd8(p + 7);
        let words = cpu.mem.rd8(p + 3);
        let xy = |m: &mut psx_retail_exec::Memory, o: u32| (m.rd16(o) as i16, m.rd16(o + 2) as i16);
        let rgb = |m: &mut psx_retail_exec::Memory, o: u32| [m.rd8(o), m.rd8(o + 1), m.rd8(o + 2)];
        if code & 0xFC == 0x2C {
            assert_eq!(words, 9, "an FT4 is nine words");
            let c = rgb(&mut cpu.mem, p + 4);
            out.push(Packet {
                code,
                xy: std::array::from_fn(|v| xy(&mut cpu.mem, p + 8 + 8 * v as u32)),
                rgb: [c; 4],
                uv: Some((
                    std::array::from_fn(|v| {
                        let o = p + 0xC + 8 * v as u32;
                        (cpu.mem.rd8(o), cpu.mem.rd8(o + 1))
                    }),
                    cpu.mem.rd16(p + 0xE),
                    cpu.mem.rd16(p + 0x16),
                )),
            });
            p += 0x28;
        } else {
            assert_eq!(code & 0xFC, 0x38, "packet code {code:#x}");
            assert_eq!(words, 8, "a G4 is eight words");
            out.push(Packet {
                code,
                xy: std::array::from_fn(|v| xy(&mut cpu.mem, p + 8 + 8 * v as u32)),
                rgb: std::array::from_fn(|v| rgb(&mut cpu.mem, p + 4 + 8 * v as u32)),
                uv: None,
            });
            p += 0x24;
        }
    }
    assert_eq!(p, end, "the pool ends on a packet");
    out
}

fn port_packet(prim: &ScreenPrim) -> Packet {
    match prim {
        ScreenPrim::Textured(q) => {
            assert!(q.gouraud.is_none());
            let c = q.color.to_le_bytes();
            // `0x2F`: textured, semi-transparent, raw texture (`0x808080`
            // modulation is the identity either way).
            assert_eq!(q.color, 0x80_80_80);
            Packet {
                code: 0x2C | u8::from(q.semi_transparent) << 1 | 1,
                xy: q.xy,
                rgb: [[c[2], c[1], c[0]]; 4],
                uv: Some((q.uv, q.clut, q.tpage)),
            }
        }
        ScreenPrim::Flat(f) => {
            let g = f.gouraud.expect("every Theeder G4 is gouraud");
            // `0x3B`: retail sets the raw-texture bit on these untextured
            // quads too, where the GPU ignores it.
            Packet {
                code: 0x38 | u8::from(f.semi_transparent) << 1 | 1,
                xy: f.xy,
                rgb: g.map(|c| [c[0], c[1], c[2]]),
                uv: None,
            }
        }
    }
}

fn scripted_rand(cpu: &mut Cpu, seed: u64) -> Rc<RefCell<(Rng, Vec<u32>)>> {
    let draws: Rc<RefCell<(Rng, Vec<u32>)>> = Rc::new(RefCell::new((Rng(seed), Vec::new())));
    let d = draws.clone();
    cpu.hook(RAND, move |r, _| {
        let mut g = d.borrow_mut();
        let v = g.0.next() & 0x7FFF;
        g.1.push(v);
        r.r[V0] = v;
    });
    draws
}

#[test]
fn the_prong_matches_the_module() {
    differential!(prot(904, PRONG));
    let Some(mut cpu) = module_cpu() else {
        return;
    };
    let draws = scripted_rand(&mut cpu, 0x5052_4F4E);
    let mut rng = Rng(0x7468_6565);
    let mut widened = 0;
    for case in 0..3000 {
        let (a, b) = (point(&mut rng), point(&mut rng));
        put(&mut cpu, POINT_A, a);
        put(&mut cpu, POINT_B, b);
        reset_pool(&mut cpu);
        draws.borrow_mut().1.clear();
        assert_eq!(
            cpu.call(PRONG, &[POINT_A, POINT_B]),
            Stop::Returned,
            "case {case}"
        );
        let drawn = std::mem::take(&mut draws.borrow_mut().1);
        assert_eq!(drawn.len(), 2, "two draws, the second discarded");
        let ((pa, za), (pb, zb)) = (project(a), project(b));
        let (wa, wb) = (prong_widen(za), prong_widen(zb));
        widened += usize::from(wa != 0 || wb != 0);
        let want = port_packet(&prong_prim_widened(pa, wa, pb, wb, (drawn[0] & 3) as u8));
        let got = retail_packets(&mut cpu);
        assert_eq!(got, vec![want], "case {case}: a {a:?} b {b:?}");
    }
    assert!(widened > 100, "the near-camera arm ran {widened} times");
}

#[test]
fn the_charge_beam_matches_the_module() {
    differential!(prot(904, CHARGE));
    let Some(mut cpu) = module_cpu() else {
        return;
    };
    let mut rng = Rng(0x6368_6172);
    for case in 0..3000 {
        let (a, b) = (point(&mut rng), point(&mut rng));
        let level = rng.below(0x200) as u16;
        put(&mut cpu, POINT_A, a);
        put(&mut cpu, POINT_B, b);
        reset_pool(&mut cpu);
        assert_eq!(
            cpu.call(CHARGE, &[POINT_A, POINT_B, u32::from(level)]),
            Stop::Returned,
            "case {case}"
        );
        let want: Vec<Packet> = charge_prims(project(a).0, project(b).0, level)
            .iter()
            .map(port_packet)
            .collect();
        assert_eq!(
            retail_packets(&mut cpu),
            want,
            "case {case}: level {level:#x}"
        );
    }
}

fn seed_ring(cpu: &mut Cpu, rng: &mut Rng, n: u32) -> [[i16; 3]; 16] {
    let ring: [[i16; 3]; 16] = std::array::from_fn(|_| point(rng));
    for (i, p) in ring.iter().enumerate() {
        put(cpu, RING + 8 * i as u32, *p);
    }
    cpu.mem.wr32(COUNT, n);
    ring
}

fn read_ring(cpu: &mut Cpu) -> Vec<[i16; 3]> {
    (0..16u32)
        .map(|i| {
            let at = RING + 8 * i;
            [
                cpu.mem.rd16(at) as i16,
                cpu.mem.rd16(at + 2) as i16,
                cpu.mem.rd16(at + 4) as i16,
            ]
        })
        .collect()
}

fn port_fan(packet: &TheederPacket, trail: &[[i16; 3]]) -> Vec<Packet> {
    theeder_prims(packet, trail, |p| Some(project(p).0))
        .iter()
        .map(port_packet)
        .collect()
}

#[test]
fn the_sweep_matches_the_module() {
    differential!(prot(904, SWEEP));
    let Some(mut cpu) = module_cpu() else {
        return;
    };
    let mut rng = Rng(0x7377_6565);
    let mut full = 0;
    for case in 0..2000 {
        let (a, b) = (point(&mut rng), point(&mut rng));
        let n = rng.below(16);
        let before = seed_ring(&mut cpu, &mut rng, n);
        put(&mut cpu, POINT_A, a);
        put(&mut cpu, POINT_B, b);
        reset_pool(&mut cpu);
        assert_eq!(
            cpu.call(SWEEP, &[POINT_A, POINT_B]),
            Stop::Returned,
            "case {case}"
        );

        // The ring half, as the port's trail takes it on a pass boundary.
        let mut trail = TheederTrail::default();
        trail.hist = before;
        trail.n = n as usize;
        let drawn = trail.sample(b);
        let ring = read_ring(&mut cpu);
        assert_eq!(&ring[..], &trail.hist[..], "case {case}: the ring");
        assert_eq!(
            cpu.mem.rd32(COUNT),
            trail.n as u32,
            "case {case}: the count"
        );
        full += usize::from(n == 15);

        let want = port_fan(
            &TheederPacket::Sweep {
                mouth: a,
                tip: b,
                drawn,
            },
            &ring,
        );
        assert_eq!(retail_packets(&mut cpu), want, "case {case}: n {n}");
    }
    assert!(full > 50, "the capped count ran {full} times");
}

#[test]
fn the_retract_matches_the_module() {
    differential!(prot(904, RETRACT));
    let Some(mut cpu) = module_cpu() else {
        return;
    };
    let mut rng = Rng(0x7265_7472);
    for case in 0..2000 {
        let a = point(&mut rng);
        // Arm 13 runs while the count it returns is non-zero
        // (`bne v0,zero` at `0x801F7FBC`), and the sweep before it leaves at
        // least one, so a zero count never reaches it (retail would wrap it).
        let n = 1 + rng.below(15);
        let before = seed_ring(&mut cpu, &mut rng, n);
        put(&mut cpu, POINT_A, a);
        reset_pool(&mut cpu);
        assert_eq!(cpu.call(RETRACT, &[POINT_A]), Stop::Returned, "case {case}");
        let left = cpu.regs.r[V0];

        let mut trail = TheederTrail::default();
        trail.hist = before;
        trail.n = n as usize;
        let drawn = trail.drawn();
        trail.retract();
        assert_eq!(
            read_ring(&mut cpu),
            before.to_vec(),
            "case {case}: the ring stands"
        );
        assert_eq!(
            cpu.mem.rd32(COUNT),
            trail.n as u32,
            "case {case}: the count"
        );
        assert_eq!(left, trail.n as u32, "case {case}: what is left");

        let want = port_fan(&TheederPacket::Retract { mouth: a, drawn }, &before);
        assert_eq!(retail_packets(&mut cpu), want, "case {case}: n {n}");
    }
}

// ---------------------------------------------------------------------------
// The tick bodies walked: PROT 0904's `0x801F69D8` and PROT 0906's
// `0x801F69F4`, arm 0 to Done, beside the band's module half.
// ---------------------------------------------------------------------------

mod walk {
    use crate::common::{disc, scus_cpu};
    use legaia_engine_battle_vm::battle_action::bearing_12bit_approx;
    use legaia_engine_cast::cast_module_camera::SPEED_SCALAR;
    use legaia_engine_core::world::World;
    use psx_retail_exec::{A0, A2, Cpu, Memory, Stop, V0};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    pub(super) const BASE: u32 = 0x801F_69D8;
    const OVERLAY_BASE: u32 = 0x801C_E818;
    const CTX_PTR: u32 = 0x8007_BD24;
    const ACTORS: u32 = 0x801C_9370;
    const RECORDS: u32 = 0x801C_9348;
    pub(super) const CTX: u32 = 0x8012_0000;
    pub(super) const ACTOR: u32 = 0x8013_0000;
    pub(super) const STRIDE: u32 = 0x400;
    const NODES: u32 = 0x8014_0000;
    const MONSTER_RECORD: u32 = 0x8015_0000;
    const RAND: u32 = 0x8005_6798;
    const BUILDER: u32 = 0x801D_829C;
    const BEARING: u32 = 0x8001_9B28;
    const AIM: u32 = 0x801D_CEAC;
    const WRAPPER: u32 = 0x801D_D0AC;
    /// The camera framing `FUN_801F0348`, run live: it stores `ctx[+0x6D0]`
    /// off the victim's record size class, the depth PROT 0906's
    /// single-seat exit shot reads.
    const FRAMING: u32 = 0x801F_0348;
    /// The drive poll `CdSync(1)` each module's poll arm makes, answered busy
    /// for the arm's first [`DRIVE_BUSY_PASSES`] passes, as the summon walk
    /// answers it.
    const DRIVE_POLL: u32 = 0x8003_F2B8;
    const DRIVE_BUSY_PASSES: u32 = 3;
    const GROUP_LIST: u32 = 0x801F_6978;
    const SCALAR: u32 = 0x1F80_037D;
    const STEP: u32 = 0x1F80_0393;
    const FORMATION: u32 = 0x8007_BD0C;
    const ROSTER: u32 = 0x8007_BD10;
    const POOL_PTR: u32 = 0x1F80_03A0;
    const OT_PTR: u32 = 0x1F80_03F4;
    const POOL: u32 = 0x8017_0000;
    const OT: u32 = 0x8018_0000;
    pub(super) const CREATURE: u32 = 7;

    /// Where the seats stand: the caster's party seat, the monster count and
    /// the victim a seat-target cast is aimed at.
    #[derive(Clone, Copy, Debug)]
    pub(super) struct Layout {
        pub caster: u32,
        pub monsters: u32,
        pub victim: u32,
        pub retail_seats: bool,
        /// The sides swapped (z negated, the party facing `0`), so the
        /// creature faces the other half-turn.
        pub mirror: bool,
    }

    pub(super) const LAYOUTS: [Layout; 5] = [
        Layout {
            caster: 0,
            monsters: 2,
            victim: 3,
            retail_seats: false,
            mirror: false,
        },
        Layout {
            caster: 2,
            monsters: 4,
            victim: 6,
            retail_seats: true,
            mirror: false,
        },
        Layout {
            caster: 1,
            monsters: 3,
            victim: 3,
            retail_seats: true,
            mirror: false,
        },
        Layout {
            caster: 0,
            monsters: 4,
            victim: 6,
            retail_seats: true,
            mirror: true,
        },
        Layout {
            caster: 2,
            monsters: 3,
            victim: 3,
            retail_seats: true,
            mirror: true,
        },
    ];

    impl Layout {
        fn ground(&self, s: u32) -> (i16, i16) {
            let (x, z) = self.ground_unmirrored(s);
            (x, if self.mirror { z.wrapping_neg() } else { z })
        }

        fn ground_unmirrored(&self, s: u32) -> (i16, i16) {
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
    }

    pub(super) fn trio(m: &mut Memory, a: u32) -> [i16; 3] {
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

    /// The `k`th roll on `seat`: small, middling and past a monster's HP.
    pub(super) fn scripted_roll(seat: usize, k: u32) -> u32 {
        [0x10, 0x5A, 0x7D, 0x180][(seat + k as usize) % 4]
    }

    /// A rig for one module image on one layout: the battle overlay, the
    /// image, every call out stubbed but the bearing, `rand()` (an LCG), the
    /// target-group aim and the damage wrapper (scripted rolls), and the
    /// tween builder (logged).
    pub(super) struct Rig {
        pub cpu: Cpu,
        pub shots: Rc<RefCell<Vec<[i16; 10]>>>,
        pub stream: Rc<Cell<u32>>,
        pub struck: Rc<RefCell<Vec<u8>>>,
        /// What the drive poll `FUN_8003F2B8` answers: busy while non-zero.
        pub drive_busy: Rc<Cell<u32>>,
        /// The module's seek and poll arms
        /// (`battle_cast_cue::module_drive_arms`).
        pub drive: Option<legaia_engine_battle_vm::battle_cast_cue::ModuleDriveArms>,
    }

    pub(super) fn rig(entry: u32, action: u8, code: u8, layout: Layout) -> Option<Rig> {
        let d = disc()?;
        let mut cpu = scus_cpu()?;
        let overlay = d.read_entry(898).expect("PROT 0898");
        let module = d.read_entry(entry as usize).expect("module");
        cpu.mem.load(OVERLAY_BASE, &overlay);
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
        cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
        cpu.mem.wr32(POOL_PTR, POOL);
        cpu.mem.wr32(OT_PTR, OT);
        for target in jal_targets(&module) {
            if ![AIM, BUILDER, BEARING, RAND, WRAPPER, FRAMING, DRIVE_POLL].contains(&target) {
                cpu.stub(target, 0);
            }
        }
        let drive = legaia_engine_battle_vm::battle_cast_cue::module_drive_arms(&module);
        let drive_busy = Rc::new(Cell::new(0u32));
        {
            let busy = drive_busy.clone();
            cpu.hook(DRIVE_POLL, move |r, _| r.r[V0] = busy.get());
        }
        let struck = Rc::new(RefCell::new(Vec::new()));
        {
            let calls = Rc::new(RefCell::new([0u32; 8]));
            let struck = struck.clone();
            cpu.hook(WRAPPER, move |r, _| {
                let seat = (r.r[A2] & 7) as usize;
                let mut c = calls.borrow_mut();
                r.r[V0] = scripted_roll(seat, c[seat]);
                c[seat] += 1;
                struck.borrow_mut().push(seat as u8);
            });
            let calls = RefCell::new([0u32; 8]);
            legaia_engine_core::world::summon_roll_override::set(Some(Box::new(move |seat| {
                let seat = usize::from(seat & 7);
                let mut c = calls.borrow_mut();
                let v = scripted_roll(seat, c[seat]);
                c[seat] += 1;
                v
            })));
        }
        let shots = Rc::new(RefCell::new(Vec::new()));
        {
            let shots = shots.clone();
            cpu.hook(BUILDER, move |r, m| {
                let mut s = [0i16; 10];
                for (k, base) in [r.r[A0], r.r[A0 + 1], r.r[A0 + 2]].into_iter().enumerate() {
                    s[3 * k..3 * k + 3].copy_from_slice(&trio(m, base));
                }
                s[9] = r.r[A0 + 3] as i16;
                shots.borrow_mut().push(s);
            });
        }
        let stream = Rc::new(Cell::new(1u32));
        {
            let stream = stream.clone();
            cpu.hook(RAND, move |r, _| {
                let n = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
                stream.set(n);
                r.r[V0] = (n >> 16) & 0x7FFF;
            });
        }
        cpu.hook(BEARING, |r, _| {
            let a = |i: usize| r.r[A0 + i] as i16;
            r.r[V0] = u32::from(bearing_12bit_approx(a(0), a(1), a(2), a(3)));
        });
        cpu.mem.load(BASE, &module);
        seed(&mut cpu, action, code, layout);
        Some(Rig {
            cpu,
            shots,
            stream,
            struck,
            drive_busy,
            drive,
        })
    }

    fn seed(cpu: &mut Cpu, action: u8, code: u8, layout: Layout) {
        for b in 0..0x1100 {
            cpu.mem.wr8(CTX + b, 0);
        }
        cpu.mem.wr8(CTX, 3);
        cpu.mem.wr8(CTX + 1, layout.monsters as u8);
        cpu.mem.wr8(CTX + 0x13, layout.caster as u8);
        cpu.mem.wr16(CTX + 0x6D0, 0xC00);
        for s in 0..8u32 {
            let a = ACTOR + s * STRIDE;
            cpu.mem.wr32(ACTORS + s * 4, a);
            for o in (0..STRIDE).step_by(4) {
                cpu.mem.wr32(a + o, 0);
            }
            let live = s < 3 || (3..3 + layout.monsters).contains(&s) || s == CREATURE;
            let (x, z) = layout.ground(s);
            for (o, v) in [(0x34u32, x), (0x38, z), (0x3C, x), (0x40, z)] {
                cpu.mem.wr16(a + o, v as u16);
            }
            let party_facing = if layout.mirror { 0 } else { 0x800 };
            let row_facing = if layout.mirror { 0x800 } else { 0 };
            cpu.mem
                .wr16(a + 0x46, if s < 3 { party_facing } else { row_facing });
            cpu.mem.wr16(a + 0x14C, if live { 300 } else { 0 });
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
        cpu.mem.wr8(ACTOR + layout.caster * STRIDE + 0x1DD, code);
        cpu.mem.wr8(ACTOR + CREATURE * STRIDE + 0x1DD, code);
        for i in 0..5 {
            cpu.mem.wr32(RECORDS + i * 4, MONSTER_RECORD);
        }
        cpu.mem.wr8(SCALAR, SPEED_SCALAR as u8);
        cpu.mem.wr8(STEP, 1);
        for i in 0..5 {
            cpu.mem
                .wr8(FORMATION + i, if i < layout.monsters { 0x10 } else { 0 });
        }
        for i in 0..3 {
            cpu.mem.wr8(ROSTER + i, 1 + i as u8);
        }
        // Each monster's reaction run `+0x1EF..+0x1F2`.
        for s in 3..7u32 {
            let a = ACTOR + s * STRIDE;
            cpu.mem
                .wr8(a + 0x1EF, if s == 4 { 0 } else { 0x10 + s as u8 });
            cpu.mem.wr8(a + 0x1F0, 0x20 + s as u8);
            cpu.mem.wr8(a + 0x1F1, 0x30 + s as u8);
            cpu.mem.wr8(a + 0x1F2, (s & 1) as u8);
        }
        // State `0x34`'s hide loop ahead of the module's first pass.
        for s in 0..7u32 {
            let a = ACTOR + s * STRIDE;
            if s < 3 || cpu.mem.rd16(a + 0x14C) != 0 {
                cpu.mem.wr32(a + 4, 0);
                cpu.mem.wr8(a + 0x21C, 0xFF);
            }
        }
    }

    /// The band's world seated off the rig's records.
    pub(super) fn shadow(cpu: &mut Cpu, layout: Layout, action: u8) -> Option<World> {
        let mut w = World::new();
        w.load_party(legaia_save::Party::zeroed(3));
        w.enter_battle(3, layout.monsters as u8);
        while w.actors.len() < 12 {
            w.actors.push(Default::default());
        }
        for s in 0..8u32 {
            let a = ACTOR + s * STRIDE;
            let p = trio(&mut cpu.mem, a + 0x34);
            let act = &mut w.actors[s as usize];
            act.active = s < 3 + layout.monsters || s == CREATURE;
            act.battle.hp = cpu.mem.rd16(a + 0x14C);
            act.battle.max_hp = cpu.mem.rd16(a + 0x14E).max(act.battle.hp);
            act.battle.liveness = u16::from(act.battle.hp > 0);
            act.battle.render_color = cpu.mem.rd32(a + 4);
            act.battle.render_flag = cpu.mem.rd8(a + 0x21C);
            act.battle.facing_angle = cpu.mem.rd16(a + 0x46);
            act.battle.active_target = cpu.mem.rd8(a + 0x1DD);
            act.battle.anim_rate =
                legaia_engine_battle_vm::battle_anim_rate::AnimRate(cpu.mem.rd8(a + 0x21D));
            act.move_state.world_x = p[0];
            act.move_state.world_y = p[1];
            act.move_state.world_z = p[2];
            if (3..3 + layout.monsters).contains(&s) {
                act.battle_monster_id = Some(1);
            }
        }
        w.battle_ctx.active_actor = layout.caster as u8;
        w.casting.summon_actor_slot = Some(CREATURE as u8);
        for s in 3..(3 + layout.monsters).min(7) {
            let a = ACTOR + s * STRIDE;
            for o in 0x1EFu32..=0x1F2 {
                w.actors[s as usize].battle.params[(o - 0x1DF) as usize] = cpu.mem.rd8(a + o);
            }
        }
        w.cast_module_for(action)?;
        w.arm_summon_stager(layout.caster as u8, action);
        w.casting.module_cam.kill = Some(false);
        Some(w)
    }

    /// Ready every seat for the next pass as the walk's draw pass leaves it:
    /// idle clips (the creature playing what its arms staged), a parked
    /// clip's loop count off its sentinel, the cursor far along, the display
    /// pair on the live pair.
    ///
    /// `playing[s]` is the clip a non-creature seat plays this pass (`0`
    /// idle), and `hold_loops` keeps the creature's loop count on its `0xFF`
    /// sentinel, a clip that has not reached its park yet.
    pub(super) fn idle(cpu: &mut Cpu, playing: &[u8; 8], hold_loops: bool) {
        for s in 0..8u32 {
            let a = ACTOR + s * STRIDE;
            if (3..7).contains(&s)
                && cpu.mem.rd16(a + 0x14C) != 0
                && cpu.mem.rd32(a + 4) == 0
                && cpu.mem.rd8(a + 0x21C) < 0x0B
            {
                cpu.mem.wr32(a + 4, 0x1004_0100);
            }
            if s == CREATURE && hold_loops {
                cpu.mem.wr8(a + 0x21B, 0xFF);
            } else if s == CREATURE && cpu.mem.rd8(a + 0x21B) == 0xFF {
                cpu.mem.wr8(a + 0x21B, 0);
            }
            let staged = cpu.mem.rd8(a + 0x1DA);
            cpu.mem.wr8(
                a + 0x1D9,
                if s == CREATURE {
                    staged
                } else {
                    playing[s as usize]
                },
            );
            cpu.mem.wr16(NODES + s * 0x100 + 0x68, 0x7FFF);
            // The draw tick's tint pass refreshes the node's colour word
            // `+0x74` off the seat's `+0x04`, which PROT 0904's row poll reads.
            let word = cpu.mem.rd32(a + 4);
            cpu.mem.wr32(NODES + s * 0x100 + 0x74, word);
            for o in [0u32, 2, 4] {
                let v = cpu.mem.rd16(a + 0x34 + o);
                cpu.mem.wr16(a + 0x3C + o, v);
            }
        }
    }

    /// A felled monster fades out: the tint pass eases its word to zero
    /// outside the module, which the walk stands in for once the word has
    /// stood six passes after the kill (as the summon walk does).
    pub(super) fn fade(cpu: &mut Cpu, dead: &mut [usize; 4], after: usize) {
        for s in 3..7u32 {
            let a = ACTOR + s * STRIDE;
            let down = cpu.mem.rd16(a + 0x14C) == 0 && cpu.mem.rd32(a + 4) != 0;
            let k = (s - 3) as usize;
            dead[k] = if down { dead[k] + 1 } else { 0 };
            if dead[k] == after {
                cpu.mem.wr32(a + 4, 0);
            }
        }
    }

    pub(super) fn run(cpu: &mut Cpu, va: u32) -> bool {
        assert_eq!(cpu.call(va, &[]), Stop::Returned);
        cpu.regs.r[V0] != 0
    }
    /// One tick body the walk runs: its owning entry, its entry VA, the
    /// cast's action id, its side-band wait arm and the cast's target code.
    #[derive(Clone, Copy)]
    pub(super) struct Body {
        pub entry: u32,
        pub va: u32,
        pub action: u8,
        pub side: u8,
        pub code: u8,
    }

    /// What a walk compared, and every mismatch it found.
    #[derive(Default)]
    pub(super) struct Walked {
        pub failures: Vec<String>,
        pub compared: usize,
        pub shots_matched: usize,
        pub cadence_misses: usize,
        pub retail_strikes: std::collections::BTreeSet<(u32, u8, u8)>,
        pub port_strikes: std::collections::BTreeSet<(u32, u8, u8)>,
        /// Per body: the hit-arm passes that struck a seat.
        pub hits: Vec<usize>,
        /// Per body: the seat passes whose facing a strike had turned, matched.
        pub faced: Vec<usize>,
    }

    /// Walk each body arm 0 to Done on every layout, beside the band's
    /// module half (see the test below for what is compared).
    pub(super) fn walk_bodies(bodies: &[Body]) -> Walked {
        use legaia_engine_cast::cast_module_camera as cam;
        use legaia_engine_cast::seru_magic_ticks::THEEDER_TRAIL_PASS_TICKS;
        let mut out = Walked {
            hits: vec![0; bodies.len()],
            faced: vec![0; bodies.len()],
            ..Walked::default()
        };
        let Walked {
            failures,
            compared,
            shots_matched,
            cadence_misses,
            retail_strikes,
            port_strikes,
            hits,
            faced,
        } = &mut out;
        for (
            k,
            &Body {
                entry,
                va,
                action,
                side,
                code: code0,
            },
        ) in bodies.iter().enumerate()
        {
            let profile = cam::module_profile(entry).expect("directed");
            for layout in LAYOUTS {
                // The fallen run also casts on one seat, with the band's timer
                // still running into the side-band arm.
                for fallen in [false, true] {
                    let code = if fallen && (3..8).contains(&code0) || fallen && code0 == 9 {
                        layout.victim as u8
                    } else {
                        code0
                    };
                    let what = format!(
                        "PROT {entry:04} caster {} of {} fallen {fallen} target {code}",
                        layout.caster, layout.monsters
                    );
                    let mut r = rig(entry, action, code, layout).expect("rig");
                    let mut band_timer: u16 = if fallen { 12 } else { 0 };
                    let mut poll_passes = 0u32;
                    // The fallen run: the seat after the victim down and faded,
                    // the last monster seat in Stone.
                    let stoned = fallen.then_some(2 + layout.monsters);
                    if fallen {
                        let s = if layout.victim + 1 < 3 + layout.monsters {
                            layout.victim + 1
                        } else {
                            3
                        };
                        let a = ACTOR + s * STRIDE;
                        r.cpu.mem.wr16(a + 0x14C, 0);
                        r.cpu.mem.wr32(a + 4, 0);
                        r.cpu.mem.wr8(a + 0x21C, 0);
                        r.cpu.mem.wr16(ACTOR + stoned.unwrap() * STRIDE + 0x16E, 4);
                    }
                    // The cast's group list `0x801F6978` (four seats, `0xFF`
                    // past the last), as the band fills it at the cast: the
                    // living monsters (Mule's wave divides a roll by three
                    // only for a seat on it).
                    let living: Vec<u8> = (3..7u32)
                        .filter(|&s| r.cpu.mem.rd16(ACTOR + s * STRIDE + 0x14C) != 0)
                        .map(|s| s as u8)
                        .collect();
                    for i in 0..4u32 {
                        r.cpu.mem.wr8(
                            GROUP_LIST + i,
                            living.get(i as usize).copied().unwrap_or(0xFF),
                        );
                    }
                    let mut w = shadow(&mut r.cpu, layout, action).expect("shadow");
                    let seeded_facing = if layout.mirror { 0x800 } else { 0 };
                    if let Some(s) = stoned {
                        w.battle.status_effects.apply(
                            s as u8,
                            legaia_engine_battle_vm::status_effects::StatusKind::Stone,
                        );
                    }
                    let mut path = Vec::new();
                    let mut done = false;
                    let mut side_passes = 0;
                    let mut dead = [0usize; 4];
                    let mut react = [(0u16, 0u8); 8];
                    let mut swept = (
                        std::collections::BTreeSet::new(),
                        std::collections::BTreeSet::new(),
                    );
                    let mut loop_passes = 0u32;
                    let (mut retract_passes, mut retract_ticks) = (0u32, 0u32);
                    for pass in 0..20_000u32 {
                        let phase = r.cpu.mem.rd8(CTX + 0x279);
                        // The fallen run's clip states: a struck monster's
                        // reaction, the creature's unparked attack clip.
                        let playing: [u8; 8] =
                            std::array::from_fn(|s| if react[s].0 > 0 { react[s].1 } else { 0 });
                        for x in &mut react {
                            x.0 = x.0.saturating_sub(1);
                        }
                        if fallen && phase == 7 {
                            loop_passes += 1;
                        }
                        fade(&mut r.cpu, &mut dead, if fallen { 150 } else { 7 });
                        idle(
                            &mut r.cpu,
                            &playing,
                            fallen && phase == 7 && loop_passes <= 3,
                        );
                        // The band's frame timer `ctx[+0x6D8]`, counting down
                        // under the side-band arm and the poll after it (a
                        // summon module keeps its own countdown there).
                        if matches!(entry, 904 | 906) && phase <= side + 1 {
                            r.cpu.mem.wr16(CTX + 0x6D8, band_timer);
                            band_timer = band_timer.saturating_sub(1);
                        }
                        if phase == side {
                            side_passes += 1;
                            r.cpu
                                .mem
                                .wr8(CTX + 0x276, if side_passes <= 3 { 8 } else { 0 });
                        }
                        if w.casting.module_phase != phase {
                            failures.push(format!(
                                "{what} pass {pass}: retail arm {phase:#x} port {:#x}",
                                w.casting.module_phase
                            ));
                            break;
                        }
                        for s in 0..8u32 {
                            let a = ACTOR + s * STRIDE;
                            let act = &mut w.actors[s as usize];
                            act.battle.hp = r.cpu.mem.rd16(a + 0x14C);
                            // The colour word as the walk's tint pass leaves
                            // it: a pass compares what it writes over it.
                            act.battle.render_color = r.cpu.mem.rd32(a + 4);
                            if (3..7).contains(&s) {
                                act.battle.current_anim = r.cpu.mem.rd8(a + 0x1D9);
                                act.battle_reaction_entry = None;
                            }
                            if s == CREATURE {
                                act.battle.current_anim = r.cpu.mem.rd8(a + 0x1D9);
                                act.battle.hit_count_bound = r.cpu.mem.rd8(a + 0x21B);
                                let p = trio(&mut r.cpu.mem, a + 0x34);
                                act.move_state.world_x = p[0];
                                act.move_state.world_y = p[1];
                                act.move_state.world_z = p[2];
                                act.battle.facing_angle = r.cpu.mem.rd16(a + 0x46);
                            }
                        }
                        for s in 3..(3 + layout.monsters).min(7) {
                            let rv =
                                crate::cast_view::read_actor(&mut r.cpu.mem, ACTOR + s * STRIDE);
                            let mut v = w.drive_cast_view(s as u8);
                            v.staged_anim = rv.staged_anim;
                            v.restage = rv.restage;
                            w.drive_write_cast_view(s as u8, &v);
                        }
                        // The range poll `FUN_8004E2F0(7, victim)` the rig
                        // stubs in range, where the creature never walks:
                        // PROT 0908's arm 8 reads the port's range off the
                        // creature's pair, so it stands on the victim's
                        // there (as the summon walk does).
                        if (entry, phase) == (908, 8) {
                            let victim = if code < 8 { code } else { layout.victim as u8 };
                            let v = &w.actors[usize::from(victim)].move_state;
                            let (x, z) = (v.world_x, v.world_z);
                            let c = &mut w.actors[CREATURE as usize].move_state;
                            c.world_x = x;
                            c.world_z = z;
                        }
                        // PROT 0907's verdict as retail's arm 0 drew it (its
                        // fork word `0x801F8534` and resist word
                        // `0x801F853C`): the shadow's own draw reads its
                        // zeroed party's levels.
                        if entry == 907 && pass > 0 {
                            use legaia_engine_cast::seru_magic_ticks::NightoOutcome as O;
                            let fork = r.cpu.mem.rd32(0x801F_8534) != 0;
                            let resisted = r.cpu.mem.rd32(0x801F_853C) != 0;
                            w.casting.module_nighto_outcome = Some(match (fork, resisted) {
                                (false, false) => O::Kill,
                                (false, true) => O::KillResisted,
                                (true, false) => O::Confuse,
                                (true, true) => O::ConfuseResisted,
                            });
                        }
                        // PROT 0931's arm `0x16` waits for its creature's
                        // cursor to stand on `0x120` exactly (`0x801F7C80`).
                        if (entry, phase) == (931, 0x16) {
                            r.cpu.mem.wr16(NODES + CREATURE * 0x100 + 0x68, 0x120);
                        }
                        w.battle_ctx.frame_timer = r.cpu.mem.rd16(CTX + 0x6D8) as i16 as _;
                        w.casting.band_word_live = None;
                        w.rng_state = r.stream.get();
                        w.casting.module_cam.sideband_busy = r.cpu.mem.rd8(CTX + 0x276) != 0;
                        // The drive as the poll finds it: its seek issued, in
                        // flight for the busy passes (`World::module_drive_holds`).
                        let polls_busy = r.drive.is_some_and(|d| d.poll_arm == phase) && {
                            poll_passes += 1;
                            poll_passes <= DRIVE_BUSY_PASSES
                        };
                        r.drive_busy.set(u32::from(polls_busy));
                        w.casting.drive_arms = r.drive.map(|d| (d, true));
                        w.casting.drive.busy = if polls_busy { 0x100 } else { 0 };
                        w.clock.display_frames += 1;
                        let words_before: Vec<u32> =
                            (0..8).map(|s| w.actors[s].battle.render_color).collect();
                        let port_run =
                            w.drive_summon_module_pass(layout.caster as u8, action, true);
                        r.struck.borrow_mut().clear();
                        r.shots.borrow_mut().clear();
                        let staged_before: Vec<(u8, u8)> = (0..8u32)
                            .map(|s| {
                                let a = ACTOR + s * STRIDE;
                                (r.cpu.mem.rd8(a + 0x1DA), r.cpu.mem.rd8(a + 0x1DC))
                            })
                            .collect();
                        let busy = run(&mut r.cpu, va);
                        for s in 3..7u32 {
                            let a = ACTOR + s * STRIDE;
                            let now = (r.cpu.mem.rd8(a + 0x1DA), r.cpu.mem.rd8(a + 0x1DC));
                            if fallen && now != staged_before[s as usize] && now.0 != 0 {
                                react[s as usize] = (250, now.0);
                            }
                        }
                        let mut after = r.cpu.mem.rd8(CTX + 0x279);
                        if path.last() != Some(&after) {
                            path.push(after);
                        }
                        let hit_pass = profile.hit_arm == Some(phase);
                        // The shot the pass builds (`FUN_801D829C`), against the
                        // one the band's run arms.
                        let retail_shot = r.shots.borrow().last().copied();
                        let port_shot = port_run.as_ref().and_then(|p| p.camera_shot).map(|s| {
                            let mut o = [0i16; 10];
                            o[..3].copy_from_slice(&s.angles);
                            o[3..6].copy_from_slice(&s.tr);
                            o[6..9].copy_from_slice(&s.focus);
                            o[9] = s.frames as i16;
                            o
                        });
                        let turn = |s: Option<[i16; 10]>| {
                            s.map(|mut s| {
                                for a in &mut s[..3] {
                                    *a &= 0xFFF;
                                }
                                s
                            })
                        };
                        // The arm-8 shot frames the creature the walk stood
                        // on the victim (above).
                        // A summon director's camera is the summon walk's
                        // (`summon_cameras`), which lands each shot on the
                        // camera globals and compares the target: here the
                        // shadow has no battle camera for its live yaw and
                        // TR, so only the Seru bodies' shots are compared.
                        if entry < 914
                            && turn(retail_shot) != turn(port_shot)
                            && (entry, phase) != (908, 8)
                        {
                            failures.push(format!(
                                "{what} pass {pass} arm {phase:#x}: shot retail {retail_shot:?} port {port_shot:?}"
                            ));
                        } else if retail_shot.is_some() {
                            *shots_matched += 1;
                        }
                        if hit_pass && !r.struck.borrow().is_empty() {
                            hits[k] += 1;
                        }
                        // PROT 0904's retract, on the port's cadence.
                        if entry == 904 && phase == 13 {
                            retract_passes += 1;
                            retract_ticks += 1;
                            while after != 13 && w.casting.module_phase == 13 && retract_ticks < 200
                            {
                                w.clock.display_frames += 1;
                                w.drive_summon_module_pass(layout.caster as u8, action, true);
                                retract_ticks += 1;
                            }
                        }
                        // PROT 0906's phase hole, released on the port's clock.
                        if entry == 906 && after == 5 && w.casting.module_phase == 6 {
                            r.cpu.mem.wr8(CTX + 0x279, 6);
                            after = 6;
                        }
                        // A body that is done returns `0`; some store no
                        // `0xFF` first (PROT 0921's arm `0xB`), where the
                        // port's phase stands at `0xFF`.
                        let terminal = !busy && (after == 0xFF || w.casting.module_phase == 0xFF);
                        if !terminal && w.casting.module_phase != after {
                            failures.push(format!(
                                "{what} pass {pass} arm {phase:#x}: retail leaves for {after:#x}, port {:#x}",
                                w.casting.module_phase
                            ));
                            break;
                        }
                        // The sweep's strikes: the port tests its cone on its
                        // step-4 ticks (`theeder_pass_tested`), this walk's
                        // retail on every pass, so which pass strikes a seat is
                        // not compared - what a strike stores is, on both sides.
                        let sweeping = entry == 904 && phase == 12;
                        if sweeping {
                            for s in 3..(3 + layout.monsters).min(7) {
                                let a = ACTOR + s * STRIDE;
                                let b = &w.actors[s as usize].battle;
                                if r.struck.borrow().contains(&(s as u8)) {
                                    swept.0.insert(s as u8);
                                    retail_strikes.insert((
                                        r.cpu.mem.rd32(a + 4),
                                        r.cpu.mem.rd8(a + 0x21C),
                                        r.cpu.mem.rd8(a + 0x21F),
                                    ));
                                }
                                if b.render_color != words_before[s as usize] && b.render_flag == 0
                                {
                                    swept.1.insert(s as u8);
                                    port_strikes.insert((
                                        b.render_color,
                                        b.render_flag,
                                        b.impact_state,
                                    ));
                                }
                            }
                        }
                        if entry == 904 && phase == 12 && after != 12 {
                            // The port's tested words (`0x40 + 0x100 * k`) are
                            // among this walk's (`0x40 * k`) and the cone is the
                            // same, so every seat the port struck retail struck;
                            // retail may strike more.
                            if !swept.1.is_subset(&swept.0) {
                                failures.push(format!(
                                    "{what}: the sweep struck retail {:?} port {:?}",
                                    swept.0, swept.1
                                ));
                            }
                            *cadence_misses += swept.0.difference(&swept.1).count();
                            // The sweep's seats, re-seated off retail's for the
                            // arms after it.
                            for s in 3..(3 + layout.monsters).min(7) {
                                let a = ACTOR + s * STRIDE;
                                let rv = crate::cast_view::read_actor(&mut r.cpu.mem, a);
                                let act = &mut w.actors[s as usize];
                                act.battle.render_color = rv.present_04;
                                act.battle.render_flag = rv.render_flag;
                                act.battle.impact_state = rv.render_21f;
                                act.battle.facing_angle = r.cpu.mem.rd16(a + 0x46);
                                let mut v = w.drive_cast_view(s as u8);
                                v.staged_anim = rv.staged_anim;
                                v.restage = rv.restage;
                                w.drive_write_cast_view(s as u8, &v);
                            }
                        }
                        for s in 0..(3 + layout.monsters).min(7) {
                            if sweeping && s >= 3 {
                                continue;
                            }
                            let a = ACTOR + s * STRIDE;
                            let b = &w.actors[s as usize].battle;
                            let shown =
                                |c: u32, f: u8| if f == 0 && c == 0x2008_0200 { 0 } else { c };
                            let rhp = r.cpu.mem.rd16(a + 0x14C);
                            let got = (
                                shown(r.cpu.mem.rd32(a + 4), r.cpu.mem.rd8(a + 0x21C)),
                                r.cpu.mem.rd8(a + 0x21C),
                                r.cpu.mem.rd16(a + 0x16E),
                                if hit_pass { b.hp } else { rhp },
                            );
                            let port = (
                                shown(b.render_color, b.render_flag),
                                b.render_flag,
                                w.raw_status_word(s as u8),
                                b.hp,
                            );
                            if got != port {
                                failures.push(format!(
                                    "{what} pass {pass} arm {phase:#x}: seat {s} tint/flag/status/hp retail {got:x?} port {port:x?}"
                                ));
                            }
                            // The facing a strike turns a monster to.
                            let (rf, pf) =
                                (r.cpu.mem.rd16(a + 0x46) & 0xFFF, b.facing_angle & 0xFFF);
                            if s >= 3 && rf != pf {
                                failures.push(format!(
                                    "{what} pass {pass} arm {phase:#x}: seat {s} facing retail {rf:#x} port {pf:#x}"
                                ));
                            } else if s >= 3 && rf != seeded_facing {
                                faced[k] += 1;
                            }
                            if s >= 3 && !hit_pass {
                                let rs =
                                    crate::cast_view::read_actor(&mut r.cpu.mem, a).staged_anim;
                                let ps = w.drive_cast_view(s as u8).staged_anim;
                                if rs != ps {
                                    failures.push(format!(
                                        "{what} pass {pass} arm {phase:#x}: seat {s} staged retail {rs:#x} port {ps:#x}"
                                    ));
                                }
                            }
                        }
                        *compared += 1;
                        if !busy {
                            done = true;
                            break;
                        }
                    }
                    eprintln!(
                        "{what}: arms {path:x?} done {done}; retract {retract_passes} passes / {retract_ticks} ticks"
                    );
                    if !done {
                        failures.push(format!(
                            "{what}: no finish, stuck at arm {:#x}",
                            r.cpu.mem.rd8(CTX + 0x279)
                        ));
                    }
                    if entry == 904 {
                        // One count drop a pass on retail, one each
                        // `THEEDER_TRAIL_PASS_TICKS` ticks on the port.
                        let want = retract_passes * u32::from(THEEDER_TRAIL_PASS_TICKS);
                        if retract_passes == 0
                            || retract_ticks.abs_diff(want) > u32::from(THEEDER_TRAIL_PASS_TICKS)
                        {
                            failures.push(format!(
                                "{what}: retract {retract_passes} passes, port {retract_ticks} ticks"
                            ));
                        }
                    }
                }
            }
        }
        legaia_engine_core::world::summon_roll_override::set(None);
        out
    }
}

/// PROT 0904's and PROT 0906's tick bodies walked arm 0 to Done on every
/// layout (two of them with the sides swapped, so the creature faces the
/// other half-turn), the drive poll busy for its arm's first passes, beside the band's module half
/// (`World::drive_summon_module_pass`: the director `theeder_direct` /
/// `gizam_direct` and the tick port `theeder_tick` / `gizam_tick`) stepped on
/// retail's phase. Each layout runs twice: on the monster row as a group,
/// idle and fading a kill in a few passes; and on one seat with a monster
/// fallen and one stoned, the band's timer running into the side-band arm,
/// the creature's attack clip off its park for arm 7's first passes, each
/// struck monster playing its reaction for 250 passes and a kill fading
/// slowly, so every wait on them holds. After each pass the port's phase
/// must stand where retail's does, the shot it builds must be retail's, and
/// every seated seat's colour word, render flag, status word and HP, and
/// each monster's staged reaction and facing, must match - but on the hit
/// arm, whose HP and reaction are the band's fold.
///
/// Two places pace apart on purpose, each walked through rather than
/// compared: PROT 0904's arm 13 retracts its trail once per
/// `THEEDER_TRAIL_PASS_TICKS` engine ticks, the frame step the port's trail
/// is measured at, where this walk's pass is one vsync (the port is ticked on
/// until it leaves the arm, and the extra ticks are held to the cadence); and
/// PROT 0906's phase 5 names no arm, its release the spawned record's
/// stager arm 2 (`ctx[+0x279] += 1`), which the walk stores on the pass the
/// port's director lets its clock out.
#[test]
fn the_theeder_and_gizam_bodies_walk_as_the_band_does() {
    differential!(prot(904, 0x801F_69D8), prot(906, 0x801F_69F4));
    use walk::{Body, walk_bodies};
    if disc().is_none() || scus_cpu().is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let mut out = walk_bodies(&[
        Body {
            entry: 904,
            va: 0x801F_69D8,
            action: 0x82,
            side: 1,
            code: 9,
        },
        Body {
            entry: 906,
            va: 0x801F_69F4,
            action: 0x84,
            side: 2,
            code: 9,
        },
    ]);
    eprintln!(
        "{} passes compared, hits {:?}, struck facings {:?}, {} shots matched, {} PROT 0904 strikes off the port's tested words",
        out.compared, out.hits, out.faced, out.shots_matched, out.cadence_misses
    );
    // What PROT 0904's strike stores: the colour word, flag and selector.
    assert!(!out.retail_strikes.is_empty());
    assert_eq!(
        out.retail_strikes, out.port_strikes,
        "PROT 0904 strike stores, retail vs port"
    );
    assert!(out.hits.iter().all(|&n| n > 0), "{:?}", out.hits);
    assert!(out.faced[1] > 0, "{:?}", out.faced);
    let n = out.failures.len();
    out.failures.truncate(60);
    assert!(n == 0, "{n} mismatches:\n{}", out.failures.join("\n"));
}

/// The same walk over the summon and Seru bodies whose strikes turn the
/// seats they hit, each monster's facing compared after every pass: PROT
/// 0905 (Vera), 0908 (Zenoir: arm 6 turns the creature and victim to each
/// other, arm 11's splash faces each seat to the creature), 0916 (Aluru: the
/// damage path faces the victim to the creature), 0920 (Slippery: arm 9
/// faces each seat to the camera's focus point), 0921 (Iota: arm `0xA`
/// faces the row from `(0, 0x400)`, a Stoned seat skipped), 0923 (Gilium:
/// arm `0x15` faces each to the stage's origin), 0927 (Juggernaut), 0930
/// (Horn), 0932 (Meta: arm `0x15` faces each out from `(0, 0x200)`) and 0934
/// (Ozma), and the bodies whose waits the variants hold beyond the summon
/// walk's: PROT 0907, 0914, 0918, 0924, 0925, 0928, 0929 and 0931. A summon
/// director's shot is the summon walk's to compare, which lands it on the
/// camera globals; only the Seru bodies' shots are compared here. The walk's builder does not land a shot on the
/// focus globals and the shadow's world has no battle camera, so both sides
/// face Slippery's struck seats to the origin: that store's form is compared,
/// not the framed point. PROT 0920's arm 10 also waits for the row, which
/// the slow fades and long reactions hold.
#[test]
fn the_summon_strikes_turn_their_seats_as_retail_does() {
    differential!(
        prot(905, 0x801F_69D8),
        prot(907, 0x801F_69E8),
        prot(908, 0x801F_69D8),
        prot(914, 0x801F_69F0),
        prot(916, 0x801F_69F8),
        prot(918, 0x801F_6C70),
        prot(920, 0x801F_69D8),
        prot(921, 0x801F_6A08),
        prot(923, 0x801F_69D8),
        prot(924, 0x801F_6A18),
        prot(925, 0x801F_6A00),
        prot(927, 0x801F_6A84),
        prot(928, 0x801F_69F4),
        prot(929, 0x801F_69FC),
        prot(930, 0x801F_6A74),
        prot(931, 0x801F_6A58),
        prot(932, 0x801F_6A34),
        prot(934, 0x801F_6A40)
    );
    use legaia_engine_cast::cast_module_camera::sideband_wait_arm;
    use walk::{Body, walk_bodies};
    if disc().is_none() || scus_cpu().is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    // `(entry, tick body, the cast's target code)`, as the summon walk casts
    // them.
    let bodies: Vec<Body> = [
        (905u32, 0x801F_69D8u32, 2u8),
        (907, 0x801F_69E8, 3),
        (908, 0x801F_69D8, 3),
        (914, 0x801F_69F0, 9),
        (916, 0x801F_69F8, 3),
        (918, 0x801F_6C70, 3),
        (920, 0x801F_69D8, 9),
        (921, 0x801F_6A08, 9),
        (923, 0x801F_69D8, 9),
        (924, 0x801F_6A18, 3),
        (925, 0x801F_6A00, 3),
        (927, 0x801F_6A84, 9),
        (928, 0x801F_69F4, 9),
        (929, 0x801F_69FC, 9),
        (930, 0x801F_6A74, 8),
        (931, 0x801F_6A58, 9),
        (932, 0x801F_6A34, 9),
        (934, 0x801F_6A40, 9),
    ]
    .into_iter()
    .map(|(entry, va, code)| Body {
        entry,
        va,
        action: if entry < 914 {
            (0x81 + entry - 903) as u8
        } else {
            (0x8C + entry - 914) as u8
        },
        side: sideband_wait_arm(entry).unwrap_or(0xF0),
        code,
    })
    .collect();
    let mut out = walk_bodies(&bodies);
    eprintln!(
        "{} passes compared, hits {:?}, struck facings {:?}, {} shots matched",
        out.compared, out.hits, out.faced, out.shots_matched
    );
    // Each turning body turned a seat on a pass that was compared.
    for (b, &n) in bodies.iter().zip(&out.faced) {
        if matches!(b.entry, 908 | 916 | 920 | 921 | 923 | 932) {
            assert!(n > 0, "PROT {:04}: no struck facing compared", b.entry);
        }
    }
    let n = out.failures.len();
    out.failures.truncate(60);
    assert!(n == 0, "{n} mismatches:\n{}", out.failures.join("\n"));
}
