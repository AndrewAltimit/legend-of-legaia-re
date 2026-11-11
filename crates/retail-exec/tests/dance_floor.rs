//! The dance floor in PROT 0980: the per-dancer tick `FUN_801D1358` with its
//! award routine `FUN_801D1AF4`, hit judge `FUN_801D1960` and CPU feed
//! `FUN_801D1820` / `FUN_801D4040`, frame by frame on one beat clock (the
//! `FUN_801CF470` state 10 sum) against `DanceGame::advance` and `press`. Both
//! sides read the hall's own clip bank; its records' timing words are what
//! `FUN_800204F8` steps.
//!
//! Between frames the feed, the judge and the human's award are also called
//! on their own, from the synced state, against `feed_symbol`, `judge_code`
//! and `press` on a clone; the words they write are put back before the tick.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigames::dance::{DanceDir, DanceEvent, DanceGame, DanceMode};
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
const FEED: u32 = 0x801D_1820;
const JUDGE: u32 = 0x801D_1960;
const AWARD: u32 = 0x801D_1AF4;
const JUDGE_POINTS: u32 = 0x801D_608C;
const JUDGE_WEIGHT: u32 = 0x801D_6090;
/// The overlay's dance state and the dancers' actors: what a probe may write.
const PROBE_WINDOWS: [(u32, usize); 3] = [(0x801D_5000, 0x1200), (ACTORS, 0x300), (0x8007_B6D8, 4)];

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

/// A CPU with the dance overlay in, `rand()` on a shared stream, the hall's
/// clip bank seated as the offset table and timing words `FUN_800204F8`
/// steps, and the draws stubbed.
#[allow(clippy::type_complexity)]
fn hall() -> Option<(
    Cpu,
    Rc<Cell<u32>>,
    Vec<u8>,
    legaia_actor_formats::player_anm::PlayerAnmBundle,
)> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
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
    Some((cpu, state, overlay, bank_records))
}

#[test]
fn every_dancer_on_the_floor_dances_as_retail_dances() {
    differential!(
        prot(980, TICK),
        prot(980, FEED),
        prot(980, JUDGE),
        prot(980, AWARD)
    );
    let Some((mut cpu, state, overlay, bank_records)) = hall() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(0x8007_B888, BANK);
    let clean = cpu.mem.clone();
    let gp = cpu.regs.r[GP];

    let mut rng = Rng(0x4441_4E43);
    let (mut sequences, mut groovies, mut misses, mut cpu_points) = (0u32, 0u32, 0u32, 0u32);
    let (mut fed, mut judged, mut awards) = (0u32, [0u32; 3], 0u32);
    for scenario in 0..400 {
        let mode =
            [DanceMode::Qualifier, DanceMode::Finals, DanceMode::FreePlay][rng.below(3) as usize];
        let density = [8, 20, 40, 120][rng.below(4) as usize];
        let dt = [1u32, 3][rng.below(2) as usize];
        let seed = rng.next();

        let mut g = DanceGame::from_overlay_for_mode(&overlay, mode).expect("run");
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
            if frame % 7 == 0 {
                let probe = probe_leaves(&mut cpu, &g, n, rng.below(4));
                fed += probe.0;
                awards += probe.2;
                for (t, c) in judged.iter_mut().zip(probe.1) {
                    *t += c;
                }
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
        }
    }
    assert!(sequences > 100, "{sequences}");
    assert!(groovies > 10, "{groovies}");
    assert!(misses > 100, "{misses}");
    assert!(cpu_points > 0, "{cpu_points}");
    assert!(
        fed > 1000 && judged.iter().all(|&n| n > 500) && awards > 1000,
        "{fed} {judged:?} {awards}"
    );
}

fn row_of(cpu: &mut Cpu, s: usize) -> Row {
    let word = |cpu: &mut Cpu, base: u32| cpu.mem.rd32(base + s as u32 * 4);
    Row {
        score: word(cpu, SCORE),
        gauge: word(cpu, GAUGE),
        cursor: word(cpu, CURSOR),
        stock: word(cpu, STOCK),
        latch: word(cpu, LATCH),
        latch_timer: word(cpu, LATCH_TIMER) as i32,
        spin: word(cpu, SPIN),
        misses: word(cpu, MISSES),
        landed: word(cpu, LANDED) != 0,
        tri_cursor: word(cpu, TRI_CURSOR),
        tri_meter: word(cpu, TRI_METER) as i32,
        clip: cpu.mem.rd16(ACTORS + s as u32 * 0x100 + 0x5C) as i16,
    }
}

fn port_row(g: &DanceGame, s: usize) -> Row {
    let p = g.dancer_probe(s);
    Row {
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
    }
}

/// Call the feed and the judge for every dancer, and the human's award for
/// one press, each from the synced state between frames; compare and put the
/// words back. Returns how many feeds answered a symbol, the judge's codes and
/// whether an award was compared.
fn probe_leaves(cpu: &mut Cpu, g: &DanceGame, n: u32, press: u32) -> (u32, [u32; 3], u32) {
    let saved: Vec<Vec<u8>> = PROBE_WINDOWS
        .iter()
        .map(|&(a, len)| cpu.mem.read(a, len))
        .collect();
    let restore = |cpu: &mut Cpu| {
        for (&(a, _), bytes) in PROBE_WINDOWS.iter().zip(&saved) {
            cpu.mem.load(a, bytes);
        }
    };
    let (mut fed, mut codes) = (0, [0; 3]);
    for s in 0..n as usize {
        assert_eq!(cpu.call(FEED, &[s as u32]), Stop::Returned);
        let retail = (
            cpu.regs.r[V0],
            cpu.mem.rd32(TRI_CURSOR + s as u32 * 4),
            cpu.mem.rd32(TRI_METER + s as u32 * 4) as i32,
        );
        restore(cpu);
        let mut c = g.clone();
        let sym = c.feed_symbol(s);
        let p = c.dancer_probe(s);
        assert_eq!(
            retail,
            (u32::from(sym), p.tri_cursor as u32, p.tri_meter),
            "feed {s}"
        );
        fed += u32::from(sym != 0);

        for dir in [DanceDir::A, DanceDir::B] {
            let lane = g.dancer_lane(s) as u32;
            cpu.mem.wr32(JUDGE_WEIGHT, 0);
            assert_eq!(
                cpu.call(JUDGE, &[s as u32, lane, dir as u32]),
                Stop::Returned
            );
            let code = cpu.regs.r[V0];
            let retail = (
                code,
                if code == 0 {
                    0
                } else {
                    cpu.mem.rd32(JUDGE_WEIGHT)
                },
                cpu.mem.rd32(JUDGE_POINTS),
                cpu.mem.rd32(CURSOR + s as u32 * 4),
            );
            restore(cpu);
            let mut c = g.clone();
            let (pc, weight, points) = c.judge_code(s, dir);
            assert_eq!(
                retail,
                (pc, weight, points, c.dancer_probe(s).cursor),
                "judge {s} {dir:?} lane {lane}"
            );
            codes[code as usize] += 1;
        }
    }
    // The human's award for one press (or none).
    let (edge, dir) = [
        (0, None),
        (0x80, Some(DanceDir::A)),
        (0x20, Some(DanceDir::B)),
        (0x10, Some(DanceDir::C)),
    ][press as usize];
    // The tick holds the award back while the human's note latch or move
    // clip runs (`DanceEvent::Ignored`); the routine itself has no such gate,
    // so only a press the tick would hand it is compared.
    let mut c = g.clone();
    let accepted = dir
        .map(|d| c.press(d))
        .filter(|e| *e != DanceEvent::Ignored);
    let awarded = u32::from(accepted.is_some());
    if let (Some(dir), Some(_)) = (dir, accepted) {
        cpu.mem.wr32(PAD_EDGE, edge);
        assert_eq!(cpu.call(AWARD, &[0]), Stop::Returned);
        // The clip the award stores is a staging value the tick rebinds;
        // the port's press binds the final clip, so it is left out here.
        let retail = Row {
            clip: 0,
            ..row_of(cpu, 0)
        };
        restore(cpu);
        let port = Row {
            clip: 0,
            ..port_row(&c, 0)
        };
        assert_eq!(retail, port, "award {dir:?}");
    }
    (fed, codes, awarded)
}

const MARKER: u32 = 0x801D_0640;

/// The step-marker tile's flipbook `FUN_801D0640` (PROT 0980, the dance
/// overlay) frame by frame against `step_marker` over the overlay's own
/// marker script (`MarkerScript::from_overlay`): the `+0x54` countdown, the
/// `+0x9C` cursor and its wrap on the next pair's sentinel, the mesh handed
/// to the model swap `FUN_80024E08` (with the `_DAT_8007B6F8` bias), and the
/// clip-selector gate.
#[test]
fn the_step_marker_flipbook_matches_retail() {
    differential!(prot(980, MARKER));
    let (Some(mut cpu), Some(d)) = (crate::common::scus_cpu(), crate::common::disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use legaia_engine_field::dance_marker::{MarkerActor, MarkerScript, step_marker};
    use std::cell::RefCell;
    use std::rc::Rc;
    let overlay = d.read_entry(980).expect("PROT 0980");
    cpu.mem.load(0x801C_E818, &overlay);
    let script = MarkerScript::from_overlay(&overlay, 0x801C_E818).expect("marker script");
    const ACTOR: u32 = 0x8013_0000;
    let calls: Rc<RefCell<Vec<(u32, i16)>>> = Rc::default();
    {
        let calls = calls.clone();
        cpu.hook(0x8002_4E08, move |r, _| {
            assert_eq!(r.r[psx_retail_exec::A0], ACTOR);
            calls
                .borrow_mut()
                .push((0x8002_4E08, r.r[psx_retail_exec::A1] as i16));
        });
    }
    {
        let calls = calls.clone();
        cpu.hook(0x8002_04F8, move |_, _| {
            calls.borrow_mut().push((0x8002_04F8, 0))
        });
    }
    let mut rng = crate::common::Rng(0x4D41_524B);
    let (mut stepped, mut wrapped) = (0, 0);
    for run in 0..400 {
        let class = rng.below(4) as u16;
        if script.steps(usize::from(class)) == 0 {
            continue;
        }
        let mut actor = MarkerActor {
            class,
            cursor: 2 * rng.below(script.steps(usize::from(class)) as u32) as u16,
            timer: rng.below(8) as i16 - 1,
            mesh: None,
        };
        let bias = rng.next() as i16 >> 4;
        cpu.mem.wr16(0x8007_B6F8, bias as u16);
        cpu.mem.wr16(ACTOR + 0x50, actor.class);
        cpu.mem.wr16(ACTOR + 0x9C, actor.cursor);
        cpu.mem.wr16(ACTOR + 0x54, actor.timer as u16);
        for frame in 0..300 {
            let delta = [1u8, 1, 2, 3][rng.below(4) as usize];
            let clip = [0i16, 1, -1][rng.below(3) as usize];
            let flags = rng.next();
            cpu.mem.wr8(0x1F80_0393, delta);
            cpu.mem.wr16(ACTOR + 0x5C, clip as u16);
            cpu.mem.wr32(ACTOR + 0x10, flags);
            calls.borrow_mut().clear();
            assert_eq!(cpu.call(MARKER, &[ACTOR]), Stop::Returned);
            let before = actor;
            let step = step_marker(&mut actor, &script, delta, bias, clip, flags);
            let mut want = Vec::new();
            if let Some(m) = step.staged_mesh {
                want.push((0x8002_4E08, m));
                stepped += 1;
                wrapped += usize::from(actor.cursor == 0);
            }
            if step.run_clip_selector {
                want.push((0x8002_04F8, 0));
            }
            let label = format!("run {run} frame {frame} class {class} before {before:?}");
            assert_eq!(*calls.borrow(), want, "{label}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x54) as i16, actor.timer, "{label}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x9C), actor.cursor, "{label}");
        }
    }
    assert!(stepped > 5000 && wrapped > 500, "{stepped} {wrapped}");
}

/// Whole songs under the dance state machine `FUN_801CF470`, from state `6`
/// (the clocks' reset) through the song, the finale and the wipe to the
/// results state, in the pool walk's order (the state machine, then the
/// dancers by slot: the actor list of `minigame_dance_retail_dancing`),
/// against `DanceGame::advance_pressing`. The beat clock here is the state
/// machine's own.
#[test]
fn whole_songs_run_as_retail_runs_them() {
    differential!(prot(980, SM), prot(980, TICK));
    const SM: u32 = 0x801C_F470;
    const SM_ACTOR: u32 = ACTORS + 0x800;
    const SONG: u32 = 0x801D_5820;
    let Some((mut cpu, state, overlay, bank_records)) = hall() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The HUD driver, the banner quads, the finale's sprite parts, the
    // teardown, the fade, the stream calls, the debug prints.
    for va in [
        0x801D_231C,
        0x801D_2F38,
        0x801D_32F8,
        0x801D_414C,
        0x8002_4EE4,
        0x8001_FC00,
        0x8001_E54C,
        0x8003_DE7C,
        0x8002_6478,
        0x8005_67A8,
    ] {
        cpu.stub(va, 0);
    }
    cpu.mem.wr32(0x8007_B888, BANK);
    cpu.mem.wr32(0x8007_B85C, 0x8010_0000);
    let clean = cpu.mem.clone();
    let gp = cpu.regs.r[GP];

    let mut rng = Rng(0x534F_4E47);
    let mut finals = Vec::new();
    let mut anchored = false;
    for scenario in 0..24 {
        let mode = [DanceMode::Qualifier, DanceMode::Finals, DanceMode::FreePlay][scenario % 3];
        // Scenario 0 keeps its hands off, as the retail capture's run did.
        let density = [0, 8, 20, 40, 120][scenario.min(1) * (1 + rng.below(4) as usize)];
        // The hall's frame step: `DAT_1F800393` reads `3` in every retail
        // capture of the dance, and the finale's sprite programs are timed
        // to it.
        let dt = 3u32;
        let seed = rng.next();

        let mut g = DanceGame::from_overlay_for_mode(&overlay, mode).expect("run");
        g.attach_clip_bank(&bank_records);

        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        state.set(seed);
        cpu.mem.wr32(STATE, 6);
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
        let mut frame = 0;
        loop {
            let edge: u32 = if density != 0 && rng.below(density as u32) == 0 {
                [0x80, 0x20, 0x10][rng.below(3) as usize]
            } else {
                0
            };
            cpu.mem.wr32(PAD_EDGE, edge);
            assert_eq!(cpu.call(SM, &[SM_ACTOR]), Stop::Returned, "frame {frame}");
            let now = cpu.mem.rd32(STATE);
            for s in 0..n {
                let stop = cpu.call(TICK, &[ACTORS + s * 0x100]);
                assert_eq!(stop, Stop::Returned, "frame {frame} dancer {s}");
            }
            // The frame state `0xC` hands over still ticks the dancers.
            if (10..=12).contains(&now) || now == 0x14 {
                g.advance_pressing(
                    dt,
                    match edge {
                        0x80 => Some(DanceDir::A),
                        0x20 => Some(DanceDir::B),
                        0x10 => Some(DanceDir::C),
                        _ => None,
                    },
                );
            }
            let retail: Vec<Row> = (0..n as usize).map(|s| row_of(&mut cpu, s)).collect();
            let port: Vec<Row> = (0..n as usize).map(|s| port_row(&g, s)).collect();
            assert_eq!(
                (retail, now == 0x14, (10..=12).contains(&now) && now != 10),
                (port, g.finished(), g.in_finale()),
                "scenario {scenario} mode {mode:?} density {density} dt {dt} frame {frame} state {now:#x} phase {:#x} song {:#x} wipe {:#x}",
                cpu.mem.rd32(PHASE),
                cpu.mem.rd32(SONG),
                cpu.mem.rd32(0x801D_515C),
            );
            // The words the how-to script reads off the floor: the miss
            // window, the triangle window (which only that script steps) and
            // the frames danced.
            let hall = g.tutorial_hall(Default::default());
            assert_eq!(
                (
                    cpu.mem.rd32(0x801D_5140),
                    cpu.mem.rd32(FEEDBACK),
                    cpu.mem.rd32(0x801D_5150)
                ),
                (
                    hall.miss_window as u32,
                    hall.feedback as u32,
                    hall.progress as u32
                ),
                "scenario {scenario} frame {frame} state {now:#x}: the lesson's words"
            );
            // The retail capture `minigame_dance_retail_judged`, a hands-off
            // qualifier, holds the two rivals on 295 and 280 at this point of
            // the song.
            if scenario == 0 && cpu.mem.rd32(SONG) == 19800 {
                let scores: Vec<u32> = (0..n as usize).map(|s| g.dancer_score(s)).collect();
                assert_eq!(scores, [0, 295, 280], "the capture's tally");
                anchored = true;
            }
            frame += 1;
            assert!(frame < 20_000, "scenario {scenario}: the song never ends");
            if now == 0x14 {
                break;
            }
        }
        finals.push(
            (0..n as usize)
                .map(|s| g.dancer_score(s))
                .collect::<Vec<_>>(),
        );
    }
    eprintln!("closing tallies {finals:?}");
    assert!(anchored, "the hands-off run passed the capture's point");
}
