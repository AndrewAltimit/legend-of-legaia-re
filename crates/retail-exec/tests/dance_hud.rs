//! The dance hall's HUD in PROT 0980, each routine whole: the driver
//! `FUN_801D231C` against `dance_hud_draws`, the beat track `FUN_801D2524`
//! against `DanceGame::beat_track_emits` with its draw area and the chain
//! flash, the count-in banner `FUN_801D2D98` against
//! `dance_countin_banner_envelope` and `CountInBanner::quads`, and the hit
//! sting `FUN_801D3D78` against `dance_hit_sting_cues`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigames::dance::{
    BeatTrackEmit, COUNTIN_INTRO_CUE, DanceGame, DanceHudDraw, DanceMode, beat_track_clip,
    chain_flash_step, dance_countin_banner_envelope, dance_hit_sting_cues, dance_hud_draws,
};
use psx_retail_exec::{A0, A1, A2, A3, Cpu, SP, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const DRIVER: u32 = 0x801D_231C;
const TRACK: u32 = 0x801D_2524;
const COUNT_IN: u32 = 0x801D_2D98;
const STING: u32 = 0x801D_3D78;
const NUMBER: u32 = 0x801D_32F8;
const QUAD: u32 = 0x801D_2F38;
const THOUSANDS: u32 = 0x801D_3E28;
const VOICE_ATTR: u32 = 0x8006_5034;
const ADD_PRIM: u32 = 0x8003_D2C4;
const WIDGETS: u32 = 0x801D_46CC;
const MODE: u32 = 0x801D_514C;
const SCORE: u32 = 0x801D_53CC;
const GAUGE: u32 = 0x801D_544C;
const STOCK: u32 = 0x801D_534C;
const PHASE: u32 = 0x801D_581C;
const CHAIN_FLASH: u32 = 0x801D_558C;
const TRACK_FLASH: u32 = 0x801D_512C;
const INTRO_LATCH: u32 = 0x801D_5134;
const BUCKET: u32 = 0x801D_5154;
const RIVAL_HUD: u32 = 0x8007_B6D0;
const CUE: u32 = 0x8007_B6D8;
const VOLUME: u32 = 0x8008_4580;
const FRAME_STEP: u32 = 0x1F80_0393;
/// The frame context: the packet cursor at `+0x8C`, the draw environment's
/// origin and extent at `+0x74..+0x7A`, the buffer index at `+0x7C`, the
/// ordering table at `+0xE0`.
const CTX: u32 = 0x1F80_0314;
const PACKETS: u32 = 0x8010_0000;
const OT: u32 = 0x8018_0000;

fn overlay_cpu() -> Option<(Cpu, Vec<u8>)> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    let overlay = d.read_entry(980).expect("PROT 0980");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    Some((cpu, overlay))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Call {
    Number { x: i32, y: i32, value: u32 },
    Quad { x: i32, y: i32, widget: u32 },
    Level { gauge: u32, x: i32, y: i32 },
    Track { slot: u32, x: i32, y: i32 },
}

#[test]
fn the_hud_driver_draws_what_retail_draws() {
    differential!(prot(980, DRIVER));
    let Some((mut cpu, _)) = overlay_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let calls = Rc::new(RefCell::new(Vec::<Call>::new()));
    let c = calls.clone();
    cpu.hook(NUMBER, move |r, m| {
        assert_eq!(r.r[A0], 0, "the score style");
        let (b, s) = (m.rd32(r.r[SP] + 0x10), m.rd32(r.r[SP] + 0x14));
        assert_eq!((b, s), (0x80, 0x1000));
        c.borrow_mut().push(Call::Number {
            x: r.r[A1] as i32,
            y: r.r[A2] as i32,
            value: r.r[A3],
        });
    });
    let c = calls.clone();
    cpu.hook(QUAD, move |r, m| {
        assert_eq!((r.r[A3], m.rd32(r.r[SP] + 0x10)), (0x80, 0x1000));
        c.borrow_mut().push(Call::Quad {
            x: r.r[A0] as i32,
            y: r.r[A1] as i32,
            widget: r.r[A2],
        });
    });
    let c = calls.clone();
    cpu.hook(THOUSANDS, move |r, _| {
        c.borrow_mut().push(Call::Level {
            gauge: r.r[A0],
            x: r.r[A1] as i32,
            y: r.r[A2] as i32,
        });
    });
    let c = calls.clone();
    cpu.hook(TRACK, move |r, _| {
        c.borrow_mut().push(Call::Track {
            slot: r.r[A0],
            x: r.r[A1] as i32,
            y: r.r[A2] as i32,
        });
    });
    let mut rng = Rng(0x4855_4421);
    let mut lens = std::collections::BTreeSet::new();
    for round in 0..4000 {
        let mode = rng.below(4);
        let rival = rng.below(2) != 0;
        let scores = [rng.below(10_000), rng.below(10_000), rng.below(10_000)];
        let gauges = [rng.below(4000), rng.below(4000), rng.below(4000)];
        cpu.mem.wr32(MODE, mode);
        cpu.mem
            .wr32(RIVAL_HUD, u32::from(rival) * (1 + rng.below(9)));
        for i in 0..3u32 {
            cpu.mem.wr32(SCORE + 4 * i, scores[i as usize]);
            cpu.mem.wr32(GAUGE + 4 * i, gauges[i as usize]);
        }
        calls.borrow_mut().clear();
        assert_eq!(cpu.call(DRIVER, &[0, 0, 0, 0]), Stop::Returned);
        let port: Vec<Call> = dance_hud_draws(mode, scores, gauges, rival)
            .into_iter()
            .map(|d| match d {
                DanceHudDraw::Score { x, y, value, .. } => Call::Number {
                    x: i32::from(x),
                    y: i32::from(y),
                    value,
                },
                DanceHudDraw::ScoreBox { x, y } => Call::Quad {
                    x: i32::from(x),
                    y: i32::from(y),
                    widget: 8,
                },
                DanceHudDraw::Gauge { x, y, value, .. } => Call::Level {
                    gauge: value,
                    x: i32::from(x),
                    y: i32::from(y),
                },
                DanceHudDraw::BeatTrack { slot, x, y } => Call::Track {
                    slot: slot as u32,
                    x: i32::from(x),
                    y: i32::from(y),
                },
            })
            .collect();
        assert_eq!(
            *calls.borrow(),
            port,
            "round {round} mode {mode} rival {rival}"
        );
        lens.insert(port.len());
    }
    // Solo, solo with the rivals' tracks or versus, versus with them.
    assert_eq!(lens.into_iter().collect::<Vec<_>>(), [4, 8, 12]);
}

/// One `FUN_801D2F38` call of the track, with the CLUT word its widget's
/// record held at the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrackQuad {
    x: i32,
    y: i32,
    widget: u32,
    brightness: u32,
    clut: u16,
}

#[test]
fn the_beat_track_emits_what_retail_emits() {
    differential!(prot(980, TRACK));
    let Some((mut cpu, overlay)) = overlay_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let quads = Rc::new(RefCell::new(Vec::<TrackQuad>::new()));
    // The draw-area packets, each with how many quads were out when it was
    // linked.
    let areas = Rc::new(RefCell::new(Vec::<(usize, u32, u32)>::new()));
    let q = quads.clone();
    cpu.hook(QUAD, move |r, m| {
        assert_eq!(m.rd32(r.r[SP] + 0x10), 0x1000, "the scale");
        let widget = r.r[A2];
        q.borrow_mut().push(TrackQuad {
            x: r.r[A0] as i32,
            y: r.r[A1] as i32,
            widget,
            brightness: r.r[A3],
            clut: m.rd16(WIDGETS + widget * 0x14 + 6),
        });
    });
    let (q, a) = (quads.clone(), areas.clone());
    cpu.hook(ADD_PRIM, move |r, m| {
        let p = r.r[A1];
        assert_eq!(m.rd32(p), 0x0200_0000, "a two-word packet");
        a.borrow_mut()
            .push((q.borrow().len(), m.rd32(p + 4), m.rd32(p + 8)));
    });
    let clean_widgets = cpu.mem.read(WIDGETS, 0x30 * 0x14);
    let mut rng = Rng(0x5452_4143);
    let (mut flashes, mut lit, mut levels) = (0, 0, [0u32; 3]);
    for round in 0..6000 {
        let mode = [DanceMode::Qualifier, DanceMode::Finals, DanceMode::FreePlay][round % 3];
        let mut g = DanceGame::from_overlay_for_mode(&overlay, mode).expect("run");
        let n = g.dancer_count();
        let slot = rng.below(n as u32) as usize;
        // The words the track reads, through the lesson's hall where the run
        // exposes them (slot 0) and the retail side for the rest.
        let phase = rng.below(0x2320);
        let stock = rng.below(4);
        // The gauge stops at 2999: the chart has three rows.
        let gauge0 = rng.below(3000);
        let mut hall = g.tutorial_hall(Default::default());
        hall.phase = phase;
        hall.stock = stock;
        hall.gauge = gauge0 as i32;
        g.apply_tutorial_hall(&hall);
        let slot = if rng.below(2) == 0 { 0 } else { slot };
        let gauge = g.dancer_gauge(slot);
        let flash = [0, 0, 0x10, 5, 1][rng.below(5) as usize];
        g.seat_chain_flash(flash);
        let dt = [1u8, 2, 3, 3][rng.below(4) as usize];
        let (x, y) = (rng.below(200) as i16 + 8, rng.below(200) as i16 + 16);
        let buffer = rng.below(2);
        let (ox, oy, w, h) = (rng.below(8), rng.below(8), 320 - rng.below(8), 240);

        cpu.mem.load(WIDGETS, &clean_widgets);
        cpu.mem.wr32(GAUGE + 4 * slot as u32, gauge);
        cpu.mem.wr32(PHASE, phase);
        cpu.mem.wr32(STOCK, stock);
        cpu.mem.wr32(STOCK + 4, 3);
        cpu.mem.wr32(STOCK + 8, 3);
        cpu.mem.wr32(CHAIN_FLASH, flash as u32);
        cpu.mem.wr32(TRACK_FLASH, 0);
        cpu.mem.wr32(BUCKET, 3);
        cpu.mem.wr8(FRAME_STEP, dt);
        cpu.mem.wr32(CTX + 0x8C, PACKETS);
        cpu.mem.wr32(CTX + 0xE0, OT);
        cpu.mem.wr16(CTX + 0x74, ox as u16);
        cpu.mem.wr16(CTX + 0x76, oy as u16);
        cpu.mem.wr16(CTX + 0x78, w as u16);
        cpu.mem.wr16(CTX + 0x7A, h as u16);
        cpu.mem.wr8(CTX + 0x7C, buffer as u8);
        quads.borrow_mut().clear();
        areas.borrow_mut().clear();

        assert_eq!(
            cpu.call(TRACK, &[slot as u32, x as u32, y as u32]),
            Stop::Returned
        );

        let emits: Vec<BeatTrackEmit> = g.beat_track_emits(slot, x, y);
        let port: Vec<TrackQuad> = emits
            .iter()
            .map(|e| TrackQuad {
                x: i32::from(e.x),
                y: i32::from(e.y),
                widget: e.widget,
                brightness: e.brightness as u32,
                clut: e.clut.unwrap_or_else(|| {
                    let o = e.widget as usize * 0x14 + 6;
                    u16::from_le_bytes([clean_widgets[o], clean_widgets[o + 1]])
                }),
            })
            .collect();
        let ctx = format!("round {round} slot {slot} gauge {gauge} phase {phase:#x} flash {flash}");
        assert_eq!(*quads.borrow(), port, "{ctx}");
        // The two draw-area packets bracket exactly the clipped calls: the
        // first linked (painted last) restores the frame's own area, the
        // second opens the track's.
        let first = emits.iter().position(|e| e.clipped).expect("clipped calls");
        let last = emits
            .iter()
            .rposition(|e| e.clipped)
            .expect("clipped calls")
            + 1;
        assert!(emits[first..last].iter().all(|e| e.clipped), "{ctx}");
        let row = buffer * 240;
        let (lo, hi) = beat_track_clip(x);
        let want = vec![
            (
                first,
                0xE300_0000 + ((oy + row) << 10) + ox,
                0xE400_0000 + ((h + row) << 10) + w - 1,
            ),
            (
                last,
                0xE300_0000 + ((oy + row) << 10) + lo as u32,
                0xE400_0000 + ((h + row) << 10) + (hi as u32 - 1),
            ),
        ];
        assert_eq!(*areas.borrow(), want, "{ctx}: the draw areas");
        assert_eq!(
            cpu.mem.rd32(CHAIN_FLASH) as i32,
            chain_flash_step(flash, u32::from(dt)),
            "{ctx}: the chain flash"
        );
        // A flash beat arms the combo call; the run arms it on its own
        // track's beat in `advance`, by the same test.
        let armed = cpu.mem.rd32(TRACK_FLASH) == 0x10;
        assert_eq!(
            armed,
            emits[1].clut == Some(0x7D0D),
            "{ctx}: the combo call's arm"
        );
        flashes += u32::from(armed);
        lit += u32::from(emits[4].brightness == 0xFF);
        levels[(gauge / 1000) as usize] += 1;
    }
    assert!(flashes > 50 && lit > 500, "flashes {flashes} lit {lit}");
    assert!(levels.iter().all(|&n| n > 100), "{levels:?}");
}

#[test]
fn the_count_in_banner_slides_as_retails_does() {
    differential!(prot(980, COUNT_IN));
    let Some((mut cpu, _)) = overlay_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    type Quads = Rc<RefCell<Vec<(i32, i32, u32, i32, u32, u8)>>>;
    let quads: Quads = Rc::new(RefCell::new(Vec::new()));
    let q = quads.clone();
    cpu.hook(QUAD, move |r, m| {
        q.borrow_mut().push((
            r.r[A0] as i32,
            r.r[A1] as i32,
            r.r[A2],
            r.r[A3] as i32,
            m.rd32(r.r[SP] + 0x10),
            m.rd8(WIDGETS + 0xF),
        ));
    });
    let (mut cues, mut holds, mut halves) = (0, 0, 0);
    for frame in -40..400 {
        for latch in [0u32, 1] {
            cpu.mem.wr32(INTRO_LATCH, latch);
            cpu.mem.wr16(CUE, 0);
            cpu.mem.wr8(WIDGETS + 0xF, 0x55);
            quads.borrow_mut().clear();
            assert_eq!(cpu.call(COUNT_IN, &[frame as u32]), Stop::Returned);
            let b = dance_countin_banner_envelope(frame);
            let port: Vec<_> = b
                .quads()
                .iter()
                .map(|q| (q.x, q.y, 0, q.brightness, 0x1000, u8::from(q.translucent)))
                .collect();
            assert_eq!(*quads.borrow(), port, "frame {frame}");
            // The intro cue fires once, on the first frame past the slide-in,
            // whichever segment that frame is in.
            let fires = frame >= 0x1E && latch == 0;
            assert_eq!(
                (cpu.mem.rd16(CUE), cpu.mem.rd32(INTRO_LATCH)),
                (
                    if fires { COUNTIN_INTRO_CUE } else { 0 },
                    u32::from(latch != 0 || fires)
                ),
                "frame {frame} latch {latch}"
            );
            cues += u32::from(fires);
            holds += u32::from(b.hold);
            halves += u32::from(!b.hold);
        }
    }
    assert!(cues > 100 && holds > 50 && halves > 100);
}

#[test]
fn the_hit_sting_keys_the_voices_retail_keys() {
    differential!(prot(980, STING));
    let Some((mut cpu, _)) = overlay_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    type Keys = Rc<RefCell<Vec<[i32; 8]>>>;
    let keys: Keys = Rc::new(RefCell::new(Vec::new()));
    let k = keys.clone();
    cpu.hook(VOICE_ATTR, move |r, m| {
        let sp = r.r[SP];
        k.borrow_mut().push([
            r.r[A0] as i32,
            r.r[A1] as i32,
            r.r[A2] as i32,
            r.r[A3] as i32,
            m.rd32(sp + 0x10) as i32,
            m.rd32(sp + 0x14) as i32,
            m.rd32(sp + 0x18) as i32,
            m.rd32(sp + 0x1C) as i32,
        ]);
    });
    let mut rng = Rng(0x5354_494E);
    for round in 0..2000 {
        let r = if round < 8 { round } else { rng.below(64) };
        let volume = if round % 5 == 0 {
            rng.next()
        } else {
            rng.below(0x80)
        };
        cpu.mem.wr32(VOLUME, volume);
        keys.borrow_mut().clear();
        assert_eq!(cpu.call(STING, &[r]), Stop::Returned);
        let port: Vec<[i32; 8]> = dance_hit_sting_cues(r as u16, volume)
            .iter()
            .map(|c| {
                [
                    c.voice as i32,
                    c.vab_program_tone.0,
                    c.vab_program_tone.1,
                    c.vab_program_tone.2,
                    c.note_and_fine.0,
                    c.note_and_fine.1,
                    c.volume.0,
                    c.volume.1,
                ]
            })
            .collect();
        assert_eq!(*keys.borrow(), port, "sting {r} volume {volume:#x}");
    }
}
