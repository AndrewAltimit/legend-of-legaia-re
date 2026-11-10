//! The dance overlay's floor pass `FUN_801D2A10` (PROT 0980) against
//! `legaia_engine_field::minigame_floor`: the height-ramp install
//! (`height_ramp`), the per-cell spawn resolve (`floor_tile_spawns`) and the
//! marker-template pick, through `MarkerFloor::build`.
//!
//! The kind-1 record lookup `FUN_801D3EC0` is hooked: it answers from the
//! same trigger list the port's build is handed, and the cell it is asked
//! about is part of the comparison. The actor allocator `FUN_80024C88` is
//! hooked and logs the template, the transform it was handed and the two
//! template words the pass stages; the clip opener `FUN_8003C8F0` and its
//! frame reader `FUN_8003D0BC` answer fixed values, since the port's
//! flipbook does not run through them.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::dance_marker::MarkerScript;
use legaia_engine_field::field_regions::TileTrigger;
use legaia_engine_field::minigame_floor::{
    FloorGrid, MarkerFloor, MarkerTemplate, floor_tile_spawns, height_ramp, marker_template,
};
use psx_retail_exec::{A0, A1, A2, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const FLOOR_PASS: u32 = 0x801D_2A10;
const LOOKUP: u32 = 0x801D_3EC0;
const ALLOC: u32 = 0x8002_4C88;
const CLIP_OPEN: u32 = 0x8003_C8F0;
const CLIP_FRAME: u32 = 0x8003_D0BC;

const PLAIN: u32 = 0x801D_42FC;
const MARKER: u32 = 0x801D_4314;
const RAMP: u32 = 0x1F80_035C;
const FLOOR_PTR: u32 = 0x1F80_03EC;

const FLOOR: u32 = 0x8010_0000;
const FLOOR_LEN: usize = 0x14000;
const RECORDS: u32 = 0x8016_0000;
const ACTORS: u32 = 0x8017_0000;
const ACTOR_STRIDE: u32 = 0x100;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Spawn {
    template: u32,
    pos: [i16; 3],
    tile: u16,
    draw_mode: i16,
}

#[test]
fn the_dance_floor_pass_matches_retail() {
    differential!(prot(980, FLOOR_PASS));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(980).expect("PROT 0980"));
    let spawns = Rc::new(RefCell::new(Vec::<Spawn>::new()));
    let asked = Rc::new(RefCell::new(Vec::<(i16, i16)>::new()));
    let triggers = Rc::new(RefCell::new(Vec::<TileTrigger>::new()));
    let s = spawns.clone();
    cpu.hook(ALLOC, move |r, m| {
        let p = r.r[A0];
        let n = s.borrow().len() as u32;
        s.borrow_mut().push(Spawn {
            template: r.r[A1],
            pos: [0, 2, 4].map(|o| m.rd16(p + o) as i16),
            tile: m.rd16(PLAIN + 4),
            draw_mode: m.rd16(PLAIN + 0x14) as i16,
        });
        r.r[V0] = ACTORS + ACTOR_STRIDE * n;
    });
    let (a, t) = (asked.clone(), triggers.clone());
    cpu.hook(LOOKUP, move |r, m| {
        assert_eq!(r.r[A0], 1, "the kind-1 table");
        let (x, z) = (r.r[A1] as i16, r.r[A2] as i16);
        a.borrow_mut().push((x, z));
        let hit = t
            .borrow()
            .iter()
            .position(|tr| i16::from(tr.tile_x) == x && i16::from(tr.tile_z) == z);
        r.r[V0] = match hit {
            Some(i) => {
                let tr = t.borrow()[i];
                let at = RECORDS + 4 * i as u32;
                m.wr8(at, tr.tile_x);
                m.wr8(at + 1, tr.tile_z);
                m.wr8(at + 2, tr.record);
                m.wr8(at + 3, tr.gate);
                at
            }
            None => 0,
        };
    });
    cpu.hook(CLIP_OPEN, |r, _| r.r[V0] = 0x8018_0000);
    cpu.hook(CLIP_FRAME, |r, m| {
        m.wr8(0x8018_0004, 0x33);
        r.r[V0] = 0x8018_0004;
    });

    let mut rng = Rng(0x464C_4F52);
    let (mut plain, mut markers, mut offset_markers) = (0, 0, 0);
    for round in 0..80 {
        let mut buf = vec![0u8; FLOOR_LEN];
        // Tile records: half drawn, small neighbour deltas (some off the
        // grid's edge), offsets and the flag bits.
        let n_tiles = 1 + rng.below(24) as usize;
        for t in 0..n_tiles {
            let b = t * 0x20;
            for o in [0usize, 2, 4, 8, 0xA, 0xC] {
                let v = rng.next() as u16 >> rng.below(8);
                buf[b + o..b + o + 2].copy_from_slice(&v.to_le_bytes());
            }
            buf[b + 6] = (rng.below(5) as i8 - 2) as u8;
            buf[b + 7] = (rng.below(5) as i8 - 2) as u8;
            if rng.below(3) == 0 {
                buf[b + 6] = 0;
                buf[b + 7] = 0;
            }
            let flags = (rng.next() as u16 & 0x1802) | if rng.below(3) != 0 { 4 } else { 0 };
            buf[b + 0x12..b + 0x14].copy_from_slice(&flags.to_le_bytes());
            buf[b + 0x1E] = rng.below(2) as u8;
        }
        for i in 0..0x4000 {
            buf[0x4000 + i] = rng.below(256) as u8;
        }
        for i in 0..0x4000 {
            let w = rng.below(n_tiles as u32) as u16 | (rng.next() as u16 & 0xFE00);
            buf[0x8000 + 2 * i..0x8000 + 2 * i + 2].copy_from_slice(&w.to_le_bytes());
        }
        let x0 = rng.below(0x80) as i32 - 2;
        let z0 = rng.below(0x80) as i32 - 2;
        let (x0, z0) = (x0.max(0), z0.max(0));
        let w = (1 + rng.below(16) as i32).min(0x80 - x0);
        let h = (1 + rng.below(16) as i32).min(0x80 - z0);
        // Trigger records over the rect and its rim, half of them markers.
        let mut trig = Vec::new();
        for _ in 0..rng.below(60) {
            let tx = (x0 + rng.below(w as u32 + 4) as i32 - 2).clamp(0, 0x7F) as u8;
            let tz = (z0 + rng.below(h as u32 + 4) as i32 - 2).clamp(0, 0x7F) as u8;
            if trig.iter().any(|t: &TileTrigger| (t.tile_x, t.tile_z) == (tx, tz)) {
                continue;
            }
            let record = if rng.below(2) == 0 {
                5 + rng.below(4) as u8
            } else {
                rng.below(256) as u8
            };
            trig.push(TileTrigger {
                tile_x: tx,
                tile_z: tz,
                record,
                gate: rng.below(2) as u8,
            });
        }
        *triggers.borrow_mut() = trig.clone();
        for (i, b) in buf.iter().enumerate() {
            cpu.mem.wr8(FLOOR + i as u32, *b);
        }
        cpu.mem.wr32(FLOOR_PTR, FLOOR);
        for i in 0..16 {
            cpu.mem.wr16(RAMP + 2 * i, 0xA5A5);
        }
        for i in 0..0x40 {
            for o in (0..ACTOR_STRIDE).step_by(4) {
                cpu.mem.wr32(ACTORS + ACTOR_STRIDE * i + o, 0);
            }
        }
        spawns.borrow_mut().clear();
        asked.borrow_mut().clear();
        assert_eq!(
            cpu.call(FLOOR_PASS, &[x0 as u32, z0 as u32, w as u32, h as u32]),
            Stop::Returned,
            "round {round}"
        );
        let ctx = format!("round {round} rect ({x0}, {z0}) {w}x{h}");

        let ramp = height_ramp();
        let got_ramp: Vec<i16> = (0..16).map(|i| cpu.mem.rd16(RAMP + 2 * i) as i16).collect();
        assert_eq!(got_ramp, ramp, "the ramp: {ctx}");

        let grid = FloorGrid::new(&buf);
        let resolved = floor_tile_spawns(grid, &ramp, x0, z0, w, h, false);
        let script = MarkerScript::default();
        let floor = MarkerFloor::build(grid, &ramp, x0, z0, w, h, false, &trig, &[], script);
        let got = spawns.borrow().clone();
        // Every drawn cell asks the lookup; one with no record spawns nothing.
        assert_eq!(asked.borrow().len(), resolved.len(), "lookups: {ctx}");
        let mut want = Vec::new();
        let mut want_markers = Vec::new();
        for (sp, &(ax, az)) in resolved.iter().zip(asked.borrow().iter()) {
            let clip = trig
                .iter()
                .find(|t| (i16::from(t.tile_x), i16::from(t.tile_z)) == (ax, az))
                .map_or(0, |t| u16::from(t.record) + 1);
            let Some(tmpl) = marker_template(clip) else {
                continue;
            };
            want.push(Spawn {
                template: match tmpl {
                    MarkerTemplate::Marker { .. } => MARKER,
                    MarkerTemplate::Plain => PLAIN,
                },
                pos: sp.pos,
                tile: sp.tile_id,
                draw_mode: sp.draw_mode,
            });
            if let MarkerTemplate::Marker { sub_index } = tmpl {
                want_markers.push(((sp.cell_x, sp.cell_z), sp.pos, sp.rot, sub_index, sp));
            }
        }
        assert_eq!(got, want, "spawns: {ctx}");
        for (k, sp) in got.iter().enumerate() {
            let actor = ACTORS + ACTOR_STRIDE * k as u32;
            let r = resolved
                .iter()
                .find(|r| r.pos == sp.pos && r.tile_id == sp.tile)
                .expect("a resolved cell");
            let rot = [0x24, 0x26, 0x28].map(|o| cpu.mem.rd16(actor + o));
            assert_eq!(rot, r.rot, "actor {k} rot: {ctx}");
            assert_eq!(cpu.mem.rd16(actor + 0x60), r.tile_id, "actor {k} tile: {ctx}");
            let f74 = cpu.mem.rd32(actor + 0x74);
            assert_eq!(f74 & 0x4000_0000 != 0, r.actor_74_tag, "actor {k}: {ctx}");
            assert_eq!(f74 & 0x1000_0000 != 0, r.actor_74_flag, "actor {k}: {ctx}");
        }
        // The port's pool holds exactly retail's marker spawns.
        let pool: Vec<_> = floor
            .tiles()
            .iter()
            .map(|t| (t.cell, t.pos, t.rot, t.actor.class))
            .collect();
        let retail_pool: Vec<_> = want_markers
            .iter()
            .map(|m| (m.0, m.1, m.2, m.3))
            .collect();
        assert_eq!(pool, retail_pool, "marker pool: {ctx}");
        for (k, sp) in got.iter().enumerate() {
            if sp.template == MARKER {
                let actor = ACTORS + ACTOR_STRIDE * k as u32;
                let class = want_markers.iter().find(|m| m.1 == sp.pos).unwrap().3;
                assert_eq!(cpu.mem.rd16(actor + 0x50), class, "actor {k} class: {ctx}");
            }
        }
        plain += got.iter().filter(|s| s.template == PLAIN).count();
        markers += want_markers.len();
        offset_markers += want_markers
            .iter()
            .filter(|m| {
                let rec = grid.tile(m.4.tile_id);
                rec.nbr_dx != 0 || rec.nbr_dz != 0
            })
            .count();
    }
    assert!(
        plain > 200 && markers > 150 && offset_markers > 100,
        "{plain} {markers} {offset_markers}"
    );
}

/// One image's copy of the plain floor pass.
struct FloorPass {
    entry: u32,
    prot: usize,
    template: u32,
    counter: u32,
    /// The dance copy's two gates (`0x8007B868` bit 2, `0x8007B8B8`).
    gated: bool,
}

fn run_floor_pass(pass: FloorPass, seed: u64) {
    use legaia_engine_field::minigame_floor::floor_pass_spawns;
    const LIST_PTR: u32 = 0x8007_C36C;
    const LIST: u32 = 0x8016_8000;
    const OLD: u32 = 0x8016_9000;
    const DEV: u32 = 0x8007_B868;
    const MODE_WORD: u32 = 0x8007_B8B8;
    const RECT: u32 = 0x1F80_0384;
    const FREE: u32 = 0x8001_7B94;
    const UNLINK: u32 = 0x8002_4DFC;
    const RELEASE: u32 = 0x8002_04A4;
    const PRINT: u32 = 0x8005_67A8;

    let disc = disc().expect("checked by the caller");
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(pass.prot).expect("overlay"));
    let spawns = Rc::new(RefCell::new(Vec::<Spawn>::new()));
    let freed = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let (s, tmpl) = (spawns.clone(), pass.template);
    cpu.hook(ALLOC, move |r, m| {
        assert_eq!(r.r[A2], LIST, "the floor list");
        let p = r.r[A0];
        let n = s.borrow().len() as u32;
        s.borrow_mut().push(Spawn {
            template: r.r[A1],
            pos: [0, 2, 4].map(|o| m.rd16(p + o) as i16),
            tile: m.rd16(tmpl + 4),
            draw_mode: m.rd16(tmpl + 0x14) as i16,
        });
        r.r[V0] = ACTORS + ACTOR_STRIDE * n;
    });
    for va in [FREE, UNLINK, RELEASE] {
        let f = freed.clone();
        cpu.hook(va, move |r, _| f.borrow_mut().push((va, r.r[A0])));
    }
    cpu.hook(PRINT, |_, _| {});

    let mut rng = Rng(seed);
    let (mut spawned, mut blocked, mut col_major, mut gated) = (0, 0, 0, 0);
    for round in 0..80 {
        let mut buf = vec![0u8; FLOOR_LEN];
        let n_tiles = 1 + rng.below(24) as usize;
        for t in 0..n_tiles {
            let b = t * 0x20;
            for o in [0usize, 2, 4, 8, 0xA, 0xC] {
                let v = rng.next() as u16 >> rng.below(8);
                buf[b + o..b + o + 2].copy_from_slice(&v.to_le_bytes());
            }
            buf[b + 6] = (rng.below(5) as i8 - 2) as u8;
            buf[b + 7] = (rng.below(5) as i8 - 2) as u8;
            let flags = (rng.next() as u16 & 0x1802) | if rng.below(3) != 0 { 4 } else { 0 };
            buf[b + 0x12..b + 0x14].copy_from_slice(&flags.to_le_bytes());
            buf[b + 0x1E] = rng.below(2) as u8;
        }
        for i in 0..0x4000 {
            buf[0x4000 + i] = rng.below(256) as u8;
        }
        for i in 0..0x4000 {
            let w = rng.below(n_tiles as u32) as u16
                | (rng.next() as u16 & 0xFA00)
                | if rng.below(4) == 0 { 0x400 } else { 0 };
            buf[0x8000 + 2 * i..0x8000 + 2 * i + 2].copy_from_slice(&w.to_le_bytes());
        }
        let x0 = rng.below(0x7C) as i32;
        let z0 = rng.below(0x7C) as i32;
        let x1 = match rng.below(6) {
            0 => x0,
            1 => x0 - 1,
            _ => (x0 + 1 + rng.below(12) as i32).min(0x80),
        }
        .max(0);
        let z1 = (z0 + 1 + rng.below(12) as i32).min(0x80);
        let ramp: Vec<i16> = (0..16).map(|_| rng.next() as i16 >> 4).collect();
        let (dev, mode) = if pass.gated && rng.below(5) == 0 {
            if rng.below(2) == 0 {
                (2 | rng.next(), 0)
            } else {
                (rng.next() & !2, 1 + rng.below(4))
            }
        } else {
            (rng.next() & !2, 0)
        };
        let shut = pass.gated && (dev & 2 != 0 || mode != 0);

        for (i, b) in buf.iter().enumerate() {
            cpu.mem.wr8(FLOOR + i as u32, *b);
        }
        cpu.mem.wr32(FLOOR_PTR, FLOOR);
        for (i, v) in ramp.iter().enumerate() {
            cpu.mem.wr16(RAMP + 2 * i as u32, *v as u16);
        }
        for (i, v) in [x0, z0, x1, z1].iter().enumerate() {
            cpu.mem.wr8(RECT + i as u32, *v as u8);
        }
        cpu.mem.wr32(DEV, dev);
        cpu.mem.wr32(MODE_WORD, mode);
        cpu.mem.wr32(pass.counter, 0x5A5A);
        // The list to tear down: three actors, the middle one owning a side
        // buffer.
        let old: Vec<u32> = (0..3).map(|i| OLD + 0x100 * i).collect();
        cpu.mem.wr32(LIST_PTR, LIST);
        cpu.mem.wr32(LIST, old[0]);
        for (i, &a) in old.iter().enumerate() {
            cpu.mem.wr32(a, old.get(i + 1).copied().unwrap_or(0));
            cpu.mem.wr32(a + 0x10, if i == 1 { 0x800 } else { 0x400 });
            cpu.mem.wr32(a + 0x44, 0x8017_F000 + i as u32);
        }
        for i in 0..0x80 {
            for o in (0..ACTOR_STRIDE).step_by(4) {
                cpu.mem.wr32(ACTORS + ACTOR_STRIDE * i + o, 0);
            }
        }
        spawns.borrow_mut().clear();
        freed.borrow_mut().clear();
        assert_eq!(cpu.call(pass.entry, &[]), Stop::Returned, "round {round}");
        let ctx = format!("round {round} rect ({x0}, {z0})..({x1}, {z1}) dev {dev:#x} mode {mode}");

        if shut {
            assert!(spawns.borrow().is_empty() && freed.borrow().is_empty(), "{ctx}");
            assert_eq!(cpu.mem.rd32(pass.counter), 0, "the count resets: {ctx}");
            gated += 1;
            continue;
        }
        let mut want_freed = Vec::new();
        for (i, &a) in old.iter().enumerate() {
            if i == 1 {
                want_freed.push((FREE, 0x8017_F001));
            }
            want_freed.push((UNLINK, a));
            want_freed.push((RELEASE, a));
        }
        assert_eq!(*freed.borrow(), want_freed, "teardown: {ctx}");

        let grid = FloorGrid::new(&buf);
        let resolved = floor_pass_spawns(grid, &ramp, x0, z0, x1, z1);
        let want: Vec<Spawn> = resolved
            .iter()
            .map(|r| Spawn {
                template: pass.template,
                pos: r.pos,
                tile: r.tile_id,
                draw_mode: r.draw_mode,
            })
            .collect();
        assert_eq!(*spawns.borrow(), want, "spawns: {ctx}");
        assert_eq!(cpu.mem.rd32(pass.counter), resolved.len() as u32, "count: {ctx}");
        for (k, r) in resolved.iter().enumerate() {
            let actor = ACTORS + ACTOR_STRIDE * k as u32;
            let rot = [0x24, 0x26, 0x28].map(|o| cpu.mem.rd16(actor + o));
            assert_eq!(rot, r.rot, "actor {k} rot: {ctx}");
            assert_eq!(cpu.mem.rd16(actor + 0x60), r.tile_id, "actor {k}: {ctx}");
            let f74 = cpu.mem.rd32(actor + 0x74);
            assert_eq!(f74 & 0x4000_0000 != 0, r.actor_74_tag, "actor {k}: {ctx}");
            assert_eq!(f74 & 0x1000_0000 != 0, r.actor_74_flag, "actor {k}: {ctx}");
            assert_eq!(cpu.mem.rd32(actor + 0x10) & 4 != 0, r.actor_10_flag, "actor {k}: {ctx}");
        }
        spawned += resolved.len();
        col_major += u32::from(resolved.windows(2).any(|w| w[1].cell_x > w[0].cell_x));
        let drawn_rec = (x0..x1)
            .flat_map(|x| (z0..z1).map(move |z| (x, z)))
            .filter(|&(x, z)| {
                let r = grid.tile(grid.tile_id(x, z));
                let (nx, nz) = (x + i32::from(r.nbr_dx), z + i32::from(r.nbr_dz));
                r.flags & 4 != 0
                    && (0..0x80).contains(&nx)
                    && (0..0x80).contains(&nz)
                    && grid.cell(nx, nz) & 0x400 != 0
            })
            .count();
        blocked += drawn_rec;
    }
    assert!(
        spawned > 600 && blocked > 100 && col_major > 30 && (!pass.gated || gated > 5),
        "{spawned} {blocked} {col_major} {gated}"
    );
}

#[test]
fn the_dance_floor_pass_plain_walk_matches_retail() {
    differential!(prot(980, 0x801D_3A2C));
    if disc().is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    run_floor_pass(
        FloorPass {
            entry: 0x801D_3A2C,
            prot: 980,
            template: PLAIN,
            counter: 0x801D_6084,
            gated: true,
        },
        0x5041_5353,
    );
}

#[test]
fn the_fishing_floor_pass_matches_retail() {
    differential!(prot(972, 0x801D_6BBC));
    if disc().is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    run_floor_pass(
        FloorPass {
            entry: 0x801D_6BBC,
            prot: 972,
            template: 0x801D_8118,
            counter: 0x801D_90A4,
            gated: false,
        },
        0x4649_5348,
    );
}
