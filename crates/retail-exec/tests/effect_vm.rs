//! The battle effect VM off the disc (PROT 0898): the init and pack fixup
//! `FUN_801DE914` over the real `efect.dat`, the spawn API `FUN_801DFDF0` and
//! the per-frame walker `FUN_801E0080` against `effect_vm::Pool` and the
//! catalog the engine parses from the same file. Frames of random spawns
//! (every effect id the catalog holds, ids `4` / `0x13` with their side call,
//! random positions and angles) interleave with walker calls at frame steps
//! `0..=3`; after each the master and child slots are compared, and every
//! packet pass 2 emits is held to `Pool::child_billboards`: the projector's
//! inputs, the colour, the texel corners, the CLUT and the page.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_vm::effect_vm::{
    CHILD_SLOT_BYTES, EffectCatalog, EffectHost, MASTER_SLOT_BYTES, MAX_MASTER_SLOTS, Pool,
    PoolHead,
};
use psx_retail_exec::{A0, A1, A2, Cpu, Stop, V0};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

const INIT: u32 = 0x801D_E914;
const SPAWN: u32 = 0x801D_FDF0;
const WALK: u32 = 0x801E_0080;
const OVERLAY_BASE: u32 = 0x801C_E818;
const RAND: u32 = 0x8005_6798;
const PART_SPAWN: u32 = 0x8005_0ED4;
const PROJECT: u32 = 0x8001_95A8;
const POOL_PTR: u32 = 0x8007_BD30;
const READY: u32 = 0x8007_BD58;
const END_SIGNAL: u32 = 0x8007_BD71;
const BUF_PTR: u32 = 0x8007_BD5C;
const POOL: u32 = 0x8011_0000;
const BUF: u32 = 0x8012_0000;
const POS: u32 = 0x8010_0000;
const DT: u32 = 0x1F80_0393;
const PRIM_PTR: u32 = 0x1F80_03A0;
const PRIMS: u32 = 0x8018_0000;
const OT_PTR: u32 = 0x1F80_03F4;
const OT: u32 = 0x8017_0000;
const OT_INDEX: u32 = 0x40;

/// One shared `rand()` stream for both sides.
type Stream = Rc<RefCell<VecDeque<u32>>>;

/// The side calls retail made: descriptor, position, angle.
type Parts = Rc<RefCell<Vec<(u32, [i16; 3], u16)>>>;

/// The projector's inputs: centre, half-width, half-height.
type Projected = Rc<RefCell<Vec<([i16; 3], u32, u32)>>>;

struct Host {
    stream: Stream,
    parts: Vec<(u8, [i16; 3], u16)>,
}

impl EffectHost for Host {
    fn next_random(&mut self) -> i32 {
        self.stream.borrow_mut().pop_front().expect("a port draw") as i32
    }
    fn is_summon_effect(&self, id: u8) -> bool {
        id == 4 || id == 0x13
    }
    fn handle_summon(&mut self, id: u8, pos: [i16; 3], angle: u16) {
        self.parts.push((id, pos, angle));
    }
}

fn retail_cpu(stream: &Stream, parts: &Parts) -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    let d = disc()?;
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(898).expect("PROT 0898"));
    cpu.mem.load(BUF, &d.read_entry(873).expect("PROT 0873"));
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_122C);
    cpu.mem.wr32(POOL_PTR, POOL);
    cpu.mem.wr32(BUF_PTR, BUF);
    cpu.mem.wr8(END_SIGNAL, 0xFF);
    cpu.mem.wr32(OT_PTR, OT);
    let s = stream.clone();
    cpu.hook(RAND, move |r, _| {
        r.r[V0] = s.borrow_mut().pop_front().expect("a retail draw")
    });
    let p = parts.clone();
    cpu.hook(PART_SPAWN, move |r, m| {
        let at = r.r[A0];
        let angle = m.rd16(r.r[A1] + 2);
        p.borrow_mut().push((
            r.r[A2],
            std::array::from_fn(|k| m.rd16(at + 2 * k as u32) as i16),
            angle,
        ));
    });
    Some(cpu)
}

fn compare_slots(cpu: &mut Cpu, pool: &Pool, label: &str) {
    let pack1 = cpu.mem.rd32(POOL + 0xC);
    let pack0 = cpu.mem.rd32(POOL + 8);
    for (i, m) in pool.master_slots.iter().enumerate() {
        let at = POOL + 0x1010 + (i * MASTER_SLOT_BYTES) as u32;
        let count = cpu.mem.rd8(at);
        assert_eq!(count, m.child_count, "{label}: master {i} count");
        if count == 0 {
            continue;
        }
        let retail = (
            cpu.mem.rd8(at + 1),
            cpu.mem.rd8(at + 2),
            cpu.mem.rd8(at + 3),
            cpu.mem.rd16(at + 4),
            [8, 0xC, 0x10].map(|o| cpu.mem.rd32(at + o) as i32),
            cpu.mem.rd32(at + 0x18),
        );
        let script = cpu.mem.rd32(pack1 + 4 * u32::from(m.ui_id)) + 4;
        let port = (
            m.flags,
            m.spawn_cursor,
            m.state,
            m.angle,
            [m.pos_x, m.pos_y, m.pos_z],
            script + m.script_offset,
        );
        assert_eq!(retail, port, "{label}: master {i}");
    }
    for (i, c) in pool.children.iter().enumerate() {
        let at = POOL + 0x10 + (i * CHILD_SLOT_BYTES) as u32;
        let count = cpu.mem.rd8(at);
        assert_eq!(count, c.frame_count, "{label}: child {i} count");
        if count == 0 {
            continue;
        }
        let retail = (
            cpu.mem.rd8(at + 1),
            cpu.mem.rd8(at + 2),
            cpu.mem.rd8(at + 3),
            [4, 6, 8].map(|o| cpu.mem.rd16(at + o) as i16),
            [0xC, 0x10, 0x14].map(|o| cpu.mem.rd32(at + o) as i32),
            cpu.mem.rd32(at + 0x1C),
        );
        let anim = cpu.mem.rd32(pack0 + 4 * u32::from(c.anim_id)) + 2;
        let port = (
            c.mirror,
            c.frame_cursor,
            c.wait,
            c.velocity,
            c.pos,
            anim + 6 * u32::from(c.frame_cursor),
        );
        assert_eq!(retail, port, "{label}: child {i}");
    }
}

#[test]
fn the_effect_pool_matches_retail() {
    differential!(prot(898, INIT), prot(898, SPAWN), prot(898, WALK));
    let stream: Stream = Rc::default();
    let parts: Parts = Rc::default();
    let Some(mut cpu) = retail_cpu(&stream, &parts) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let catalog = EffectCatalog::from_efect_dat_bytes(&disc().unwrap().read_entry(873).unwrap());
    assert!(catalog.len() > 0x13);
    // The projector: record its inputs, hand back a fixed bucket.
    let projected: Projected = Rc::default();
    let pr = projected.clone();
    cpu.hook(PROJECT, move |r, m| {
        let at = r.r[A0];
        pr.borrow_mut().push((
            std::array::from_fn(|k| m.rd16(at + 2 * k as u32) as i16),
            r.r[A1],
            r.r[A2],
        ));
        r.r[V0] = OT_INDEX;
    });

    assert_eq!(cpu.call(INIT, &[0x1000, 0xA00]), Stop::Returned);
    assert_eq!(cpu.mem.rd8(READY), 1, "init raises the ready byte");
    let head = PoolHead::default();
    assert_eq!(
        (cpu.mem.rd16(POOL), cpu.mem.rd16(POOL + 2)),
        (head.motion_scale, head.sprite_scale)
    );
    assert_eq!(cpu.mem.rd32(POOL + 4), BUF + 8, "atlas base");

    let mut rng = Rng(0x4546_4658);
    let (mut spawned, mut seeded_max, mut drawn, mut side) = (0usize, 0usize, 0usize, 0usize);
    let mut ended = 0;
    for run in 0..40 {
        // A fresh pool each run, from the retail init's cleared block.
        assert_eq!(cpu.call(INIT, &[0x1000, 0xA00]), Stop::Returned);
        let mut pool = Pool::new();
        let mut host = Host {
            stream: stream.clone(),
            parts: Vec::new(),
        };
        parts.borrow_mut().clear();
        for frame in 0..200 {
            let label = format!("run {run} frame {frame}");
            // Spawns.
            for _ in 0..[0, 0, 1, 1, 2, 6][rng.below(6) as usize] {
                let id = if rng.below(8) == 0 {
                    [4u8, 0x13][rng.below(2) as usize]
                } else {
                    rng.below(catalog.len() as u32) as u8
                };
                let pos: [i16; 3] = std::array::from_fn(|_| rng.next() as i16 >> 4);
                let angle = rng.next() as u16;
                for (k, v) in pos.iter().enumerate() {
                    cpu.mem.wr16(POS + 2 * k as u32, *v as u16);
                }
                let draws: Vec<u32> = (0..1024).map(|_| rng.next() & 0x7FFF).collect();
                *stream.borrow_mut() = draws.iter().copied().collect();
                assert_eq!(
                    cpu.call(SPAWN, &[u32::from(id), POS, u32::from(angle)]),
                    Stop::Returned,
                    "{label}"
                );
                let left = stream.borrow().len();
                *stream.borrow_mut() = draws.into_iter().collect();
                let got = pool.spawn_by_ui_id(&mut host, id, pos, angle, &catalog);
                assert_eq!(stream.borrow().len(), left, "{label}: spawn {id} draws");
                let ret = cpu.regs.r[V0] & 0xFF;
                assert_eq!(
                    got.map_or(MAX_MASTER_SLOTS as u32, |s| s as u32),
                    ret,
                    "{label}: spawn {id} slot"
                );
                spawned += usize::from(got.is_some());
                compare_slots(&mut cpu, &pool, &format!("{label} spawn {id}"));
            }
            assert_eq!(
                parts.borrow().len(),
                host.parts.len(),
                "{label}: side calls"
            );
            for ((desc, rp, ra), (id, pp, pa)) in parts.borrow().iter().zip(&host.parts) {
                let want = if *id == 4 { 0x801F_5D90 } else { 0x801F_5CF8 };
                assert_eq!((*desc, *rp, *ra), (want, *pp, *pa), "{label}: side call");
            }
            side = side.max(host.parts.len());

            // With the battle-end byte down, neither entry touches the pool.
            if frame % 16 == 15 {
                let before = cpu.mem.read(POOL, 0x1390);
                cpu.mem.wr8(END_SIGNAL, 0xFE);
                cpu.mem.wr8(DT, 1);
                cpu.mem.wr32(PRIM_PTR, PRIMS);
                stream.borrow_mut().clear();
                assert_eq!(cpu.call(SPAWN, &[1, POS, 0]), Stop::Returned);
                assert_eq!(cpu.call(WALK, &[]), Stop::Returned);
                assert_eq!(cpu.mem.read(POOL, 0x1390), before, "{label}: ended");
                assert_eq!(cpu.mem.rd32(PRIM_PTR), PRIMS, "{label}: ended, drawn");
                cpu.mem.wr8(END_SIGNAL, 0xFF);
                ended += 1;
            }

            // The walker.
            let dt = [0u8, 1, 1, 1, 2, 3][rng.below(6) as usize];
            cpu.mem.wr8(DT, dt);
            cpu.mem.wr32(PRIM_PTR, PRIMS);
            projected.borrow_mut().clear();
            let draws: Vec<u32> = (0..4096).map(|_| rng.next() & 0x7FFF).collect();
            *stream.borrow_mut() = draws.iter().copied().collect();
            assert_eq!(cpu.call(WALK, &[]), Stop::Returned, "{label}");
            let left = stream.borrow().len();
            *stream.borrow_mut() = draws.into_iter().collect();
            pool.tick_retail(&mut host, &catalog, dt);
            assert_eq!(stream.borrow().len(), left, "{label}: walker draws");
            compare_slots(&mut cpu, &pool, &label);
            seeded_max = seeded_max.max(pool.active_child_count());

            // Pass 2: one packet per live child, in slot order.
            let boards = pool.child_billboards(&catalog);
            let proj = projected.borrow();
            assert_eq!(proj.len(), boards.len(), "{label}: packets");
            for (k, (b, (p, w, h))) in boards.iter().zip(proj.iter()).enumerate() {
                let label = format!("{label} packet {k}");
                assert_eq!(
                    (*p, *w as i32, *h as i32),
                    (b.pos, b.world_w, b.world_h),
                    "{label}: projector inputs"
                );
                let at = PRIMS + 0x28 * k as u32;
                let rgb = cpu.mem.rd32(at + 4);
                assert_eq!(
                    rgb,
                    0x2E00_0000 | (u32::from(b.brightness) * 0x0001_0101),
                    "{label}: colour"
                );
                let e = b.entry;
                let (u0, u1) = (e.u, e.u.wrapping_add(e.w).wrapping_sub(1));
                let (v0, v1) = (e.v, e.v.wrapping_add(e.h).wrapping_sub(1));
                let (ul, ur) = if b.flip_h { (u1, u0) } else { (u0, u1) };
                let (vt, vb) = if b.flip_v { (v1, v0) } else { (v0, v1) };
                let uv: Vec<(u8, u8)> = [0xC, 0x14, 0x1C, 0x24]
                    .iter()
                    .map(|o| (cpu.mem.rd8(at + o), cpu.mem.rd8(at + o + 1)))
                    .collect();
                assert_eq!(
                    uv,
                    [(ul, vt), (ur, vt), (ul, vb), (ur, vb)],
                    "{label}: texels"
                );
                assert_eq!(
                    (cpu.mem.rd16(at + 0xE), cpu.mem.rd16(at + 0x16)),
                    (e.clut, e.page),
                    "{label}: clut / page"
                );
            }
            drawn += boards.len();
        }
    }
    assert!(
        spawned > 2000 && seeded_max > 60 && drawn > 50_000 && side > 0 && ended > 100,
        "spawned {spawned} children {seeded_max} drawn {drawn} side {side}"
    );
}

/// What the walker's branches cost on PCSX-Redux's timing (two cycles an
/// instruction, one more a data access, the GTE free): random spawns and
/// walks at steps `1..=4` run on the retail walker, the projector
/// `FUN_800195A8` and the ordering-table link included, beside the port's
/// pool, whose [`WalkWork`] counts the branches each walk took. A
/// least-squares fit of the cycles over those counts and the children pass 2
/// draws prices each branch (`frame_clock::EFFECT_WALK_*`).
#[test]
fn the_walker_costs_its_branches() {
    not_a_differential!(
        "counts the retail walker's cycles to price the census's pool term; the walker is held to the port by the_effect_pool_matches_retail"
    );
    let stream: Stream = Rc::default();
    let parts: Parts = Rc::default();
    let Some(mut cpu) = retail_cpu(&stream, &parts) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let catalog = EffectCatalog::from_efect_dat_bytes(&disc().unwrap().read_entry(873).unwrap());
    let mut rng = Rng(0x0C05_7C05);
    // (cycles, [call, sweeps, masters, spawns, child waits, advances,
    // overrun advances, drawn])
    let mut rows: Vec<(f64, [f64; N])> = Vec::new();
    for run in 0..30 {
        assert_eq!(cpu.call(INIT, &[0x1000, 0xA00]), Stop::Returned);
        let mut pool = Pool::new();
        let mut host = Host {
            stream: stream.clone(),
            parts: Vec::new(),
        };
        let busy = 1 + run % 6;
        for _frame in 0..120 {
            for _ in 0..rng.below(busy as u32) {
                let id = rng.below(catalog.len() as u32) as u8;
                if id == 4 || id == 0x13 {
                    continue;
                }
                let pos: [i16; 3] = std::array::from_fn(|_| rng.next() as i16 >> 6);
                for (k, v) in pos.iter().enumerate() {
                    cpu.mem.wr16(POS + 2 * k as u32, *v as u16);
                }
                let draws: Vec<u32> = (0..1024).map(|_| rng.next() & 0x7FFF).collect();
                *stream.borrow_mut() = draws.iter().copied().collect();
                cpu.call(SPAWN, &[u32::from(id), POS, 0]);
                *stream.borrow_mut() = draws.into_iter().collect();
                pool.spawn_by_ui_id(&mut host, id, pos, 0, &catalog);
            }
            let dt = 1 + rng.below(4) as u8;
            cpu.mem.wr8(DT, dt);
            cpu.mem.wr32(PRIM_PTR, PRIMS);
            let draws: Vec<u32> = (0..4096).map(|_| rng.next() & 0x7FFF).collect();
            *stream.borrow_mut() = draws.iter().copied().collect();
            assert_eq!(cpu.call(WALK, &[]), Stop::Returned);
            let cycles = (2 * cpu.steps + cpu.mem_ops) as f64;
            *stream.borrow_mut() = draws.into_iter().collect();
            let before = pool.work;
            pool.tick_retail(&mut host, &catalog, dt);
            let w = pool.work.since(&before);
            let drawn = pool.active_child_count();
            rows.push((
                cycles,
                [
                    w.calls,
                    w.sweeps,
                    w.masters,
                    w.spawns,
                    w.child_waits,
                    w.advances,
                    w.overruns,
                    drawn as u64,
                ]
                .map(|v| v as f64),
            ));
        }
    }
    if let Ok(path) = std::env::var("LEGAIA_WALKER_ROWS") {
        let text: String = rows
            .iter()
            .map(|(y, x)| format!("{y} {}\n", x.map(|v| v.to_string()).join(" ")))
            .collect();
        let _ = std::fs::write(path, text);
    }
    let beta = least_squares(&rows);
    const CYCLES_PER_HBLANK: f64 = 3413.0 * 7.0 / 11.0;
    let names = [
        "call",
        "sweep",
        "master",
        "spawn",
        "child wait",
        "advance",
        "overrun",
        "drawn",
    ];
    for (n, b) in names.iter().zip(beta) {
        eprintln!(
            "walker {n}: {b:.1} cycles ({:.4} hblanks)",
            b / CYCLES_PER_HBLANK
        );
    }
    let rms = (rows
        .iter()
        .map(|(y, x)| (y - x.iter().zip(beta).map(|(a, b)| a * b).sum::<f64>()).powi(2))
        .sum::<f64>()
        / rows.len() as f64)
        .sqrt();
    eprintln!("walker fit rms {rms:.0} cycles over {} walks", rows.len());
    assert!(rows.len() > 3000 && rms < 3000.0, "{rms}");
}

/// The walker cost fit's regressors.
const N: usize = 8;

/// Ordinary least squares (normal equations, Gaussian elimination with
/// partial pivoting).
fn least_squares(rows: &[(f64, [f64; N])]) -> [f64; N] {
    let mut a = [[0.0f64; N + 1]; N];
    for (y, x) in rows {
        for i in 0..N {
            for j in 0..N {
                a[i][j] += x[i] * x[j];
            }
            a[i][N] += x[i] * y;
        }
    }
    for c in 0..N {
        let p = (c..N)
            .max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))
            .unwrap();
        a.swap(c, p);
        for r in 0..N {
            if r != c && a[c][c] != 0.0 {
                let f = a[r][c] / a[c][c];
                let pivot = a[c];
                for (x, p) in a[r].iter_mut().zip(pivot).skip(c) {
                    *x -= f * p;
                }
            }
        }
    }
    std::array::from_fn(|i| {
        if a[i][i] != 0.0 {
            a[i][N] / a[i][i]
        } else {
            0.0
        }
    })
}
