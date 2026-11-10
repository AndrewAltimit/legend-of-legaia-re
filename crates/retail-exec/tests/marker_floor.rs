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
