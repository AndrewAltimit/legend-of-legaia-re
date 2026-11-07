//! The dance floor in PROT 0980: the per-dancer tick `FUN_801D1358` with its
//! award routine `FUN_801D1AF4`, hit judge `FUN_801D1960` and CPU feed
//! `FUN_801D1820` / `FUN_801D4040`, frame by frame on one beat clock (the
//! `FUN_801CF470` state 10 sum) against `DanceGame::advance` and `press`. Both
//! sides read the hall's own clip bank; its records' timing words are what
//! `FUN_800204F8` steps.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigames::dance::{DanceDir, DanceGame, DanceMode};
use legaia_minigame_formats::dance_cast as dc;
use psx_retail_exec::{Cpu, GP, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const TICK: u32 = 0x801D_1358;
const ACTORS: u32 = 0x8014_0000;
const BANK: u32 = 0x8015_0000;

const STATE: u32 = 0x801D_5334;
const MODE: u32 = 0x801D_514C;
const PHASE: u32 = 0x801D_581C;
const FEEDBACK: u32 = 0x801D_5144;
const SCORE: u32 = 0x801D_53CC;
const GAUGE: u32 = 0x801D_544C;
const CURSOR: u32 = 0x801D_550C;
const STOCK: u32 = 0x801D_534C;
const KIND: u32 = 0x801D_540C;
const LATCH: u32 = 0x801D_548C;
const LATCH_TIMER: u32 = 0x801D_54CC;
const SPIN: u32 = 0x801D_564C;
const MISSES: u32 = 0x801D_568C;
const LANDED: u32 = 0x801D_570C;
const TRI_CURSOR: u32 = 0x801D_574C;
const TRI_METER: u32 = 0x801D_578C;
const PAD_EDGE: u32 = 0x8007_B874;

#[derive(Debug, PartialEq, Eq)]
struct Row {
    score: u32,
    gauge: u32,
    cursor: u32,
    stock: u32,
    latch: u32,
    latch_timer: i32,
    spin: u32,
    misses: u32,
    landed: bool,
    tri_cursor: u32,
    tri_meter: i32,
    clip: i16,
}

#[test]
fn every_dancer_on_the_floor_dances_as_retail_dances() {
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use legaia_engine_scene::vfs::Vfs;
    let vfs = legaia_engine_scene::vfs::DiscVfs::open(std::env::var_os("LEGAIA_DISC_BIN").unwrap())
        .expect("disc");
    let prot = vfs.read("prot.dat").expect("PROT.DAT");
    let names = vfs
        .read("cdname.txt")
        .or_else(|_| vfs.read("data/cdname.txt"))
        .expect("CDNAME.TXT");
    let index = legaia_engine_scene::scene::ProtIndex::from_bytes(
        prot,
        Some(std::str::from_utf8(&names).expect("text")),
    )
    .expect("index");
    let bank_records = legaia_engine_minigame_scenes::dance_venue::dance_clip_bank(&index)
        .expect("the hall's clip bank");
    let overlay = d.read_entry(980).expect("PROT 0980");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(0x8005_6798, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    // The face strip, the stings, the sprite parts.
    for va in [
        0x801D_03C4,
        0x801D_3D78,
        0x801D_3FD0,
        0x801D_40DC,
        0x8001_A068,
    ] {
        cpu.stub(va, 0);
    }
    // The clip bank: an offset table, then each record's three timing fields.
    for id in 1..=60u32 {
        let off = 0x1000 + id * 0x10;
        cpu.mem.wr32(BANK + id * 4, off);
        if let Ok(rec) = bank_records.record_lenient(id as usize - 1) {
            cpu.mem.wr8(BANK + off + 1, u8::from(rec.blends()));
            cpu.mem.wr16(BANK + off + 2, rec.frame_count);
            cpu.mem.wr8(BANK + off + 6, (rec.flag & 0xFF) as u8);
        }
    }
    cpu.mem.wr32(0x8007_B888, BANK);
    let clean = cpu.mem.clone();
    let gp = cpu.regs.r[GP];

    let mut rng = Rng(0x4441_4E43);
    let (mut sequences, mut groovies, mut misses, mut cpu_points) = (0u32, 0u32, 0u32, 0u32);
    for scenario in 0..400 {
        let mode =
            [DanceMode::Qualifier, DanceMode::Finals, DanceMode::FreePlay][rng.below(3) as usize];
        let density = [8, 20, 40, 120][rng.below(4) as usize];
        let dt = [1u32, 3][rng.below(2) as usize];
        let seed = rng.next();

        let mut g = DanceGame::from_overlay_for_mode(&overlay, mode, false).expect("run");
        g.attach_clip_bank(&bank_records);

        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        state.set(seed);
        cpu.mem.wr32(STATE, 10);
        cpu.mem.wr32(MODE, mode as u32);
        cpu.mem.wr8(0x1F80_0393, dt as u8);
        let n = g.dancer_count() as u32;
        for s in 0..n {
            let a = ACTORS + s * 0x100;
            let kind = g.dancer_kind(s as usize) as u32;
            let desc = dc::KIND_TABLE_VA + kind * dc::KIND_STRIDE as u32;
            let idle = cpu.mem.rd16(desc + 0x10);
            let rate = cpu.mem.rd16(desc + 0x14);
            cpu.mem.wr32(a + 0x48, desc);
            cpu.mem.wr16(a + 0x5A, s as u16);
            cpu.mem.wr16(a + 0x5C, idle & 0x1FF);
            cpu.mem.wr16(a + 0x5E, 0xFFFF);
            cpu.mem.wr16(a + 0x6A, rate);
            cpu.mem.wr32(KIND + s * 4, kind);
            cpu.mem.wr32(STOCK + s * 4, 3);
        }
        let mut phase = 0u32;
        for frame in 0..4000 {
            if g.song_over() {
                break;
            }
            let edge: u32 = if rng.below(density) == 0 {
                [0x80, 0x20, 0x10][rng.below(3) as usize]
            } else {
                0
            };
            phase = (phase + dt * 10) % 0x2320;
            cpu.mem.wr32(PHASE, phase);
            cpu.mem.wr32(PAD_EDGE, edge);
            for s in 0..n {
                let stop = cpu.call(TICK, &[ACTORS + s * 0x100]);
                assert_eq!(stop, Stop::Returned, "frame {frame} dancer {s}");
            }

            g.advance_pressing(
                dt,
                match edge {
                    0x80 => Some(DanceDir::A),
                    0x20 => Some(DanceDir::B),
                    0x10 => Some(DanceDir::C),
                    _ => None,
                },
            );

            for s in 0..n as usize {
                let word = |cpu: &mut Cpu, base: u32| cpu.mem.rd32(base + s as u32 * 4);
                let retail = Row {
                    score: word(&mut cpu, SCORE),
                    gauge: word(&mut cpu, GAUGE),
                    cursor: word(&mut cpu, CURSOR),
                    stock: word(&mut cpu, STOCK),
                    latch: word(&mut cpu, LATCH),
                    latch_timer: word(&mut cpu, LATCH_TIMER) as i32,
                    spin: word(&mut cpu, SPIN),
                    misses: word(&mut cpu, MISSES),
                    landed: word(&mut cpu, LANDED) != 0,
                    tri_cursor: word(&mut cpu, TRI_CURSOR),
                    tri_meter: word(&mut cpu, TRI_METER) as i32,
                    clip: cpu.mem.rd16(ACTORS + s as u32 * 0x100 + 0x5C) as i16,
                };
                let p = g.dancer_probe(s);
                let port = Row {
                    score: p.score,
                    gauge: p.gauge,
                    cursor: p.cursor,
                    stock: p.triangles,
                    latch: p.latch,
                    latch_timer: p.latch_timer,
                    spin: p.spin_turns,
                    misses: p.misses,
                    landed: p.landed,
                    tri_cursor: p.tri_cursor as u32,
                    tri_meter: p.tri_meter,
                    clip: p.clip,
                };
                assert_eq!(
                    retail, port,
                    "scenario {scenario} mode {mode:?} density {density} dt {dt} frame {frame} dancer {s} phase {phase:#x} edge {edge:#x}"
                );
                if s > 0 {
                    cpu_points = cpu_points.max(retail.score);
                }
            }
            sequences += u32::from(cpu.mem.rd32(0x801D_5164) != 0);
            groovies += u32::from(cpu.mem.rd32(FEEDBACK) == 0x3C);
            misses += cpu.mem.rd32(MISSES).min(1);
            let fb = cpu.mem.rd32(FEEDBACK);
            cpu.mem.wr32(FEEDBACK, fb.saturating_sub(1));
        }
    }
    assert!(sequences > 100, "{sequences}");
    assert!(groovies > 10, "{groovies}");
    assert!(misses > 100, "{misses}");
    assert!(cpu_points > 0, "{cpu_points}");
}
