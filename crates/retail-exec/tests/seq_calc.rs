//! libsnd's per-frame sequencer tier in `SCUS_942.54` against
//! `legaia_engine_audio::{seq_calc, seq_events}`: the tempo slide
//! `FUN_800649B0`, the two volume slides `FUN_8006320C` / `FUN_8006352C`, the
//! track-end handler `FUN_80063AA8` with the chained restart `FUN_80064090`,
//! the stop and start arms `FUN_800638D8` / `FUN_8006418C`, the event decoder
//! `FUN_80063CEC` and the delta-time pump `FUN_80063974` / `FUN_800639A0`,
//! and the frame dispatch over them, `SsSeqCalc` `FUN_80062F98`, with every
//! handler hooked to a scripted flag scramble both sides apply.
//!
//! Each runs on a channel record at `*(0x801CD2C0 + slot * 4) + channel *
//! 0xB0`, seeded at random, and the whole `0xB0` bytes are compared with the
//! port's [`SeqChannel`] written back over the same seed, so a write the port
//! does not model shows as a byte the seed did not hold.
//!
//! What runs for real beside them: the channel-volume getter `FUN_800683D8`,
//! the sequence-volume set `FUN_80067E9C` (with no voice owned, so it is the
//! clamp and the store) and the varint reader `FUN_80061C68`. The voice sweep
//! `FUN_800684CC` is logged; the five installed event handlers at
//! `0x801CD220` are logged with their arguments, and for the pump they advance
//! the stream the way `seq_events::run_handler_tail` says they do.

use crate::common::{Rng, diff_offsets, scus_cpu};
use legaia_engine_audio::seq_calc::{
    SeqChannel, SlideDir, TrackEnd, flag, tempo_slide_tick, track_end, volume_slide_tick,
};
use legaia_engine_audio::seq_events::{
    SeqEvent, decode_event, handler_operand_bytes, pump_delta_time, restart_channel, start_channel,
    stop_channel,
};
use psx_retail_exec::{A0, A1, A2, A3, Cpu, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const TEMPO: u32 = 0x8006_49B0;
const VOL_UP: u32 = 0x8006_320C;
const VOL_DOWN: u32 = 0x8006_352C;
const TRACK_END: u32 = 0x8006_3AA8;
const RESTART: u32 = 0x8006_4090;
const STOP: u32 = 0x8006_38D8;
const START: u32 = 0x8006_418C;
const DECODE: u32 = 0x8006_3CEC;
const PUMP: u32 = 0x8006_3974;
const PUMP_BODY: u32 = 0x8006_39A0;
const NOTE_KILL: u32 = 0x8006_84CC;

const SLOT_TABLE: u32 = 0x801C_D2C0;
const DIVISOR: u32 = 0x801C_D2BC;
/// The voice count `FUN_80067E9C` walks; zero leaves it the clamp and store.
const VOICE_COUNT: u32 = 0x801C_E344;
const HANDLERS: u32 = 0x801C_D220;
/// Where the fake handler entries sit: one hook per vector slot.
const FAKE_HANDLER: u32 = 0x8FF0_0000;

const RECORDS: u32 = 0x8012_0000;
const SLOT_STRIDE: u32 = 0x1000;
const STRIDE: u32 = 0xB0;
const STREAM: u32 = 0x8014_0000;
const STREAM_LEN: usize = 0x4000;

fn rec_addr(slot: u8, chan: u8) -> u32 {
    RECORDS + u32::from(slot) * SLOT_STRIDE + u32::from(chan) * STRIDE
}

/// Lift the record bytes into the port's channel. Stream pointers are taken
/// relative to `base`, the port's stream index.
fn lift(b: &[u8], base: u32) -> SeqChannel {
    let w = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let h = |o: usize| u16::from_le_bytes(b[o..o + 2].try_into().unwrap());
    SeqChannel {
        cursor: w(0).wrapping_sub(base),
        start: w(4).wrapping_sub(base),
        loop_cursor: w(8).wrapping_sub(base),
        alt_start: w(0xC).wrapping_sub(base),
        loop_marker: w(0x10).wrapping_sub(base),
        playing: b[0x14],
        running_status: b[0x16],
        midi_channel: b[0x17],
        running_lo: b[0x1C],
        repeat_target: b[0x20],
        repeat_count: b[0x21],
        chain_slot: b[0x22],
        chain_channel: b[0x23],
        slide_span: h(0x48) as i16,
        slide_level: h(0x4A),
        slide_step: h(0x4C) as i16,
        tempo_step: h(0x4E) as i16,
        resolution: h(0x50) as i16,
        sub_frame: h(0x52) as i16,
        tick_budget: h(0x54) as i16,
        vol: (h(0x58), h(0x5A)),
        vol_cache: (h(0x5C), h(0x5E)),
        tick_accum: w(0x88),
        pending_wait: w(0x90),
        tempo: w(0x94),
        flags: w(0x98),
        slide_total: w(0x9C),
        slide_remaining: w(0xA0) as i32,
        tempo_remaining: w(0xA8) as i32,
        tempo_target: w(0xAC),
    }
}

/// Write the port's channel back over the seed bytes.
fn seat(b: &mut [u8], c: &SeqChannel, base: u32) {
    let mut w = |o: usize, v: u32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    w(0, c.cursor.wrapping_add(base));
    w(4, c.start.wrapping_add(base));
    w(8, c.loop_cursor.wrapping_add(base));
    w(0xC, c.alt_start.wrapping_add(base));
    w(0x10, c.loop_marker.wrapping_add(base));
    w(0x88, c.tick_accum);
    w(0x90, c.pending_wait);
    w(0x94, c.tempo);
    w(0x98, c.flags);
    w(0x9C, c.slide_total);
    w(0xA0, c.slide_remaining as u32);
    w(0xA8, c.tempo_remaining as u32);
    w(0xAC, c.tempo_target);
    let mut h = |o: usize, v: u16| b[o..o + 2].copy_from_slice(&v.to_le_bytes());
    h(0x48, c.slide_span as u16);
    h(0x4A, c.slide_level);
    h(0x4C, c.slide_step as u16);
    h(0x4E, c.tempo_step as u16);
    h(0x50, c.resolution as u16);
    h(0x52, c.sub_frame as u16);
    h(0x54, c.tick_budget as u16);
    h(0x58, c.vol.0);
    h(0x5A, c.vol.1);
    h(0x5C, c.vol_cache.0);
    h(0x5E, c.vol_cache.1);
    b[0x14] = c.playing;
    b[0x16] = c.running_status;
    b[0x17] = c.midi_channel;
    b[0x1C] = c.running_lo;
    b[0x20] = c.repeat_target;
    b[0x21] = c.repeat_count;
    b[0x22] = c.chain_slot;
    b[0x23] = c.chain_channel;
}

/// A CPU with the slot table pointing at the record block, no voices owned,
/// and the voice sweep logged as `(slot | channel << 8)`.
fn rig() -> Option<(Cpu, Rc<RefCell<Vec<u16>>>)> {
    let mut cpu = scus_cpu()?;
    for s in 0..4 {
        cpu.mem.wr32(SLOT_TABLE + s * 4, RECORDS + s * SLOT_STRIDE);
    }
    cpu.mem.wr8(VOICE_COUNT, 0);
    let kills = Rc::new(RefCell::new(Vec::new()));
    let k = kills.clone();
    cpu.hook(NOTE_KILL, move |r, _| k.borrow_mut().push(r.r[A0] as u16));
    Some((cpu, kills))
}

/// A random record, with the fields each kernel branches on drawn from the
/// shapes the game reaches and their edges.
fn gen_record(rng: &mut Rng) -> Vec<u8> {
    let mut b: Vec<u8> = (0..STRIDE).map(|_| rng.next() as u8).collect();
    let mut w = |o: usize, v: u32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    let small = |rng: &mut Rng| match rng.below(4) {
        0 => rng.below(3) as i32 - 1,
        1 => rng.below(40) as i32,
        2 => rng.below(400) as i32 - 20,
        _ => rng.next() as i32,
    };
    w(0x88, rng.below(100_000));
    w(0x90, small(rng) as u32);
    w(
        0x94,
        [rng.below(300), rng.below(4), rng.next()][rng.below(3) as usize],
    );
    w(0x98, rng.next() & 0x7FF);
    w(0x9C, small(rng) as u32);
    w(0xA0, small(rng) as u32);
    w(0xA8, small(rng) as u32);
    w(
        0xAC,
        [rng.below(300), rng.below(4), rng.next()][rng.below(3) as usize],
    );
    let mut h = |o: usize, v: u16| b[o..o + 2].copy_from_slice(&v.to_le_bytes());
    h(
        0x48,
        [rng.below(0x100), rng.next()][rng.below(2) as usize] as u16,
    );
    h(
        0x4A,
        [rng.below(0x90), rng.below(3), rng.next()][rng.below(3) as usize] as u16,
    );
    let step = match rng.below(5) {
        0 => 0,
        1 => rng.below(8) as i16,
        2 => -(rng.below(8) as i16),
        3 => -(rng.below(0x90) as i16),
        _ => rng.next() as i16,
    };
    h(0x4C, step as u16);
    h(0x4E, (rng.below(16) as i16 - 8) as u16);
    h(
        0x50,
        [480, 96, 1 + rng.below(1000), rng.next()][rng.below(4) as usize] as u16,
    );
    let vol = |rng: &mut Rng| -> u16 {
        [rng.below(0x80) as u16, 0x7F, 0, 0x7E, 1, rng.next() as u16][rng.below(6) as usize]
    };
    let (l, r) = (vol(rng), vol(rng));
    h(0x58, l);
    h(0x5A, r);
    b[0x20] = [0, 1, 2, rng.below(5) as u8][rng.below(4) as usize];
    b[0x21] = rng.below(4) as u8;
    b[0x22] = if rng.below(2) == 0 {
        0xFF
    } else {
        rng.below(4) as u8
    };
    b[0x23] = rng.below(4) as u8;
    b
}

fn load_record(cpu: &mut Cpu, slot: u8, chan: u8, b: &[u8]) {
    cpu.mem.load(rec_addr(slot, chan), b);
}

fn check(what: &str, run: u32, seed: &[u8], retail: &[u8], port: &[u8], fails: &mut Vec<String>) {
    if retail != port {
        fails.push(format!(
            "{what} run {run}: seed {:?}\n {:?}",
            lift(seed, 0),
            diff_offsets(retail, port)
        ));
    }
}

#[test]
fn the_tempo_slide_matches_retail() {
    differential!(scus(TEMPO));
    let Some((mut cpu, _)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5445_4D50);
    let (mut fails, mut finished, mut moved, mut skipped) = (Vec::new(), 0, 0, 0);
    for run in 0..60_000 {
        let (slot, chan) = (rng.below(4) as u8, rng.below(16) as u8);
        let divisor = [60, 60, 50, 1 + rng.below(200)][rng.below(4) as usize];
        cpu.mem.wr32(DIVISOR, divisor);
        let seed = gen_record(&mut rng);
        load_record(&mut cpu, slot, chan, &seed);
        assert_eq!(cpu.call(TEMPO, &[slot.into(), chan.into()]), Stop::Returned);
        let retail = cpu.mem.read(rec_addr(slot, chan), STRIDE as usize);

        let mut c = lift(&seed, 0);
        let before = c.tempo;
        let t = tempo_slide_tick(&mut c, divisor);
        let mut port = seed.clone();
        seat(&mut port, &c, 0);
        finished += u32::from(t.finished);
        skipped += u32::from(t.skipped);
        moved += u32::from(c.tempo != before);
        check("tempo", run, &seed, &retail, &port, &mut fails);
        if fails.len() > 4 {
            break;
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(
        finished > 1000 && moved > 1000 && skipped > 1000,
        "{finished} {moved} {skipped}"
    );
}

/// The setter `FUN_80067E9C` with no voice owned: the pair stored, each side
/// clamped to `0x7F`.
fn set_seq_vol(c: &mut SeqChannel, (l, r): (u16, u16)) {
    c.vol = (l.min(0x7F), r.min(0x7F));
}

#[test]
fn the_volume_slides_match_retail() {
    differential!(scus(VOL_UP), scus(VOL_DOWN));
    let Some((mut cpu, _)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x564F_4C53);
    let (mut fails, mut commits, mut saturated, mut settled) = (Vec::new(), 0, 0, 0);
    for run in 0..80_000 {
        let (slot, chan) = (rng.below(4) as u8, rng.below(16) as u8);
        let dir = if rng.below(2) == 0 {
            SlideDir::Up
        } else {
            SlideDir::Down
        };
        let seed = gen_record(&mut rng);
        load_record(&mut cpu, slot, chan, &seed);
        let entry = match dir {
            SlideDir::Up => VOL_UP,
            SlideDir::Down => VOL_DOWN,
        };
        assert_eq!(cpu.call(entry, &[slot.into(), chan.into()]), Stop::Returned);
        let retail = cpu.mem.read(rec_addr(slot, chan), STRIDE as usize);

        let mut c = lift(&seed, 0);
        let live = c.vol;
        let t = volume_slide_tick(&mut c, dir, live);
        if let Some(pair) = t.commit {
            set_seq_vol(&mut c, pair);
            commits += 1;
        }
        saturated += u32::from(t.saturated);
        settled += u32::from(t.settled);
        let mut port = seed.clone();
        seat(&mut port, &c, 0);
        check(&format!("{dir:?}"), run, &seed, &retail, &port, &mut fails);
        if fails.len() > 4 {
            break;
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(
        commits > 2000 && saturated > 500 && settled > 2000,
        "{commits} {saturated} {settled}"
    );
}

#[test]
fn the_track_end_and_transport_arms_match_retail() {
    differential!(scus(TRACK_END), scus(RESTART), scus(STOP), scus(START));
    let Some((mut cpu, kills)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5452_4B45);
    let (mut fails, mut chained, mut looped, mut repeated) = (Vec::new(), 0, 0, 0);
    for run in 0..40_000 {
        let (slot, chan) = (rng.below(4) as u8, rng.below(16) as u8);
        let seed = gen_record(&mut rng);
        // The chain target is a second record, so the restart's writes land
        // apart from the handler's own.
        let mut other = gen_record(&mut rng);
        let (cs, cc) = (seed[0x22], seed[0x23]);
        let chain_live = cs != 0xFF && (cs, cc) != (slot, chan);
        if cs != 0xFF && !chain_live {
            continue;
        }
        load_record(&mut cpu, slot, chan, &seed);
        if chain_live {
            load_record(&mut cpu, cs, cc, &other);
        }
        let arm = rng.below(4);
        let entry = [TRACK_END, STOP, START, RESTART][arm as usize];
        kills.borrow_mut().clear();
        assert_eq!(cpu.call(entry, &[slot.into(), chan.into()]), Stop::Returned);
        let retail = cpu.mem.read(rec_addr(slot, chan), STRIDE as usize);
        let retail_other = chain_live.then(|| cpu.mem.read(rec_addr(cs, cc), STRIDE as usize));
        let retail_kills = kills.borrow().clone();

        let mut c = lift(&seed, 0);
        let handle = u16::from(slot) | u16::from(chan) << 8;
        let mut port_kills = Vec::new();
        match arm {
            0 => match track_end(&mut c) {
                TrackEnd::LoopForever => looped += 1,
                TrackEnd::Repeat { .. } => repeated += 1,
                TrackEnd::Finished { chain } => {
                    if let Some((s, ch)) = chain {
                        let mut o = lift(&other, 0);
                        restart_channel(&mut o);
                        seat(&mut other, &o, 0);
                        port_kills.push(handle);
                        chained += 1;
                        assert_eq!((s, ch), (cs, cc));
                    }
                    port_kills.push(handle);
                }
            },
            1 => {
                stop_channel(&mut c);
                port_kills.push(handle);
            }
            2 => start_channel(&mut c),
            _ => restart_channel(&mut c),
        }
        let mut port = seed.clone();
        seat(&mut port, &c, 0);
        check(
            &format!("arm {arm}"),
            run,
            &seed,
            &retail,
            &port,
            &mut fails,
        );
        if let Some(ro) = retail_other {
            check("chained", run, &seed, &ro, &other, &mut fails);
        }
        if retail_kills != port_kills {
            fails.push(format!(
                "arm {arm} run {run}: kills {retail_kills:?} vs {port_kills:?}"
            ));
        }
        if fails.len() > 4 {
            break;
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(
        chained > 100 && looped > 1000 && repeated > 300,
        "{chained} {looped} {repeated}"
    );
}

/// What a fake handler saw: the vector slot it was reached through and its
/// `a2` / `a3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Dispatch {
    slot: u32,
    a2: u8,
    a3: u8,
}

/// A stream of plausible events (fresh and running status, every class the
/// decoder knows, unknown ones, metas and end-of-track) with varint deltas.
fn gen_stream(rng: &mut Rng) -> Vec<u8> {
    let mut s = Vec::with_capacity(STREAM_LEN);
    let delta = |rng: &mut Rng, s: &mut Vec<u8>| match rng.below(5) {
        0 => s.push(0),
        1 => s.push(rng.below(0x80) as u8),
        2 => {
            s.push(0x80 | rng.below(0x80) as u8);
            s.push(rng.below(0x80) as u8);
        }
        3 => {
            s.push(0x81);
            s.push(0x80 | rng.below(0x80) as u8);
            s.push(rng.below(0x80) as u8);
        }
        _ => s.push(1 + rng.below(8) as u8),
    };
    while s.len() < STREAM_LEN - 16 {
        let fresh = rng.below(3) != 0;
        let class = [0x90, 0x90, 0xB0, 0xC0, 0xE0, 0xF0, 0x80, 0xA0, 0xD0][rng.below(9) as usize];
        if fresh {
            let st = if class == 0xF0 {
                [0xFF, 0xFF, 0xF0 | rng.below(16) as u8][rng.below(3) as usize]
            } else {
                class | rng.below(16) as u8
            };
            s.push(st);
        }
        match class {
            0x90 => {
                if fresh {
                    s.push(rng.below(0x80) as u8);
                }
                s.push(rng.below(0x80) as u8);
                delta(rng, &mut s);
            }
            0xB0 | 0xE0 => {
                s.push(rng.below(0x80) as u8);
                s.push(rng.below(0x80) as u8);
                delta(rng, &mut s);
            }
            0xC0 => {
                s.push(rng.below(0x80) as u8);
                delta(rng, &mut s);
            }
            0xF0 => {
                if rng.below(8) == 0 {
                    s.push(0x2F);
                } else {
                    s.push(0x51);
                    for _ in 0..3 {
                        s.push(rng.next() as u8);
                    }
                    delta(rng, &mut s);
                }
            }
            _ => s.push(rng.below(0x80) as u8),
        }
    }
    s.resize(STREAM_LEN, 0);
    s
}

/// A record playing `STREAM`, with its pointers inside the stream.
fn gen_playing_record(rng: &mut Rng, stream: &[u8]) -> Vec<u8> {
    let mut b = gen_record(rng);
    let at = |rng: &mut Rng| STREAM + rng.below(STREAM_LEN as u32 / 2);
    let cursor = at(rng);
    let marker = if rng.below(4) == 0 { cursor } else { at(rng) };
    for (o, v) in [
        (0, cursor),
        (4, at(rng)),
        (8, at(rng)),
        (0xC, at(rng)),
        (0x10, marker),
    ] {
        b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    b[0x16] = [0x90, 0xB0, 0xC0, 0xE0, 0xFF, 0x80, 0][rng.below(7) as usize];
    // No chain: a chained restart edits another record, which the decoder's
    // port reports rather than performs (the track-end test runs it).
    b[0x22] = 0xFF;
    let _ = stream;
    b
}

fn install_handlers(cpu: &mut Cpu, log: &Rc<RefCell<Vec<Dispatch>>>, advance: bool) {
    for slot in 0..5u32 {
        let entry = FAKE_HANDLER + slot * 8;
        cpu.mem.wr32(HANDLERS + slot * 4, entry);
        let l = log.clone();
        cpu.hook(entry, move |r, m| {
            l.borrow_mut().push(Dispatch {
                slot,
                a2: r.r[A2] as u8,
                a3: r.r[A3] as u8,
            });
            if !advance {
                return;
            }
            // The stream advance `seq_events::run_handler_tail` models: the
            // handler's own operands, then the delta into `+0x90` (all but
            // the note handler, which reads none).
            let rec = m.rd32(SLOT_TABLE + (r.r[A0] & 0xFFFF) * 4) + (r.r[A1] & 0xFFFF) * STRIDE;
            let operands = [0u32, 0, 1, 3, 1][slot as usize];
            let mut cur = m.rd32(rec) + operands;
            if slot != 0 {
                let first = m.rd8(cur);
                cur += 1;
                let mut v = u32::from(first);
                if first & 0x80 != 0 {
                    v = u32::from(first & 0x7F);
                    loop {
                        let b = m.rd8(cur);
                        cur += 1;
                        v = (v << 7) + u32::from(b & 0x7F);
                        if b & 0x80 == 0 {
                            break;
                        }
                    }
                }
                let scaled = v.wrapping_mul(10);
                if first != 0 {
                    let acc = m.rd32(rec + 0x88);
                    m.wr32(rec + 0x88, acc.wrapping_add(scaled));
                }
                m.wr32(rec + 0x90, scaled);
            }
            m.wr32(rec, cur);
        });
    }
}

/// The dispatch the port's event implies, in the vector's slot order (note,
/// program change, pitch bend, meta, control change).
fn port_dispatch(ev: SeqEvent, status: u8, fresh_operand: Option<u8>) -> Option<Dispatch> {
    let _ = fresh_operand;
    match ev {
        SeqEvent::Note { note, velocity, .. } => Some(Dispatch {
            slot: 0,
            a2: note,
            a3: velocity,
        }),
        SeqEvent::ProgramChange(v) => Some(Dispatch {
            slot: 1,
            a2: v,
            a3: 0,
        }),
        SeqEvent::PitchBend => Some(Dispatch {
            slot: 2,
            a2: 0,
            a3: 0,
        }),
        SeqEvent::Meta(k) => Some(Dispatch {
            slot: 3,
            a2: k,
            a3: 0,
        }),
        SeqEvent::ControlChange(v) => Some(Dispatch {
            slot: 4,
            a2: v,
            a3: 0,
        }),
        _ => {
            let _ = status;
            None
        }
    }
}

#[test]
fn the_event_decoder_matches_retail() {
    differential!(scus(DECODE));
    let Some((mut cpu, kills)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let log = Rc::new(RefCell::new(Vec::new()));
    install_handlers(&mut cpu, &log, false);
    let mut rng = Rng(0x4445_434F);
    let mut fails = Vec::new();
    let mut seen = [0u32; 8];
    for stream_no in 0..12 {
        let stream = gen_stream(&mut rng);
        cpu.mem.load(STREAM, &stream);
        for run in 0..4000 {
            let (slot, chan) = (rng.below(4) as u8, rng.below(16) as u8);
            let seed = gen_playing_record(&mut rng, &stream);
            load_record(&mut cpu, slot, chan, &seed);
            log.borrow_mut().clear();
            kills.borrow_mut().clear();
            assert_eq!(
                cpu.call(DECODE, &[slot.into(), chan.into()]),
                Stop::Returned
            );
            let retail = cpu.mem.read(rec_addr(slot, chan), STRIDE as usize);
            let retail_log = log.borrow().clone();

            let mut c = lift(&seed, STREAM);
            let ev = decode_event(&mut c, &stream);
            let status = stream[(lift(&seed, STREAM).cursor) as usize];
            let mut port = seed.clone();
            seat(&mut port, &c, STREAM);
            // The pitch-bend operand the handler receives is never read by
            // the decoder; take retail's so the slot alone is compared.
            let mut port_log: Vec<Dispatch> = port_dispatch(ev, status, None).into_iter().collect();
            if let (Some(p), Some(r)) = (port_log.first_mut(), retail_log.first())
                && p.slot == 2
            {
                p.a2 = r.a2;
            }
            if let Some(p) = port_log.first_mut()
                && p.slot != 0
                && let Some(r) = retail_log.first()
            {
                p.a3 = r.a3;
            }
            seen[match ev {
                SeqEvent::Note { .. } => 0,
                SeqEvent::ControlChange(_) => 1,
                SeqEvent::ProgramChange(_) => 2,
                SeqEvent::PitchBend => 3,
                SeqEvent::Meta(_) => 4,
                SeqEvent::EndOfTrack(_) => 5,
                SeqEvent::LoopMarker(_) => 6,
                _ => 7,
            }] += 1;
            check(
                &format!("stream {stream_no} {ev:?}"),
                run,
                &seed,
                &retail,
                &port,
                &mut fails,
            );
            if retail_log != port_log {
                fails.push(format!(
                    "stream {stream_no} run {run} {ev:?}: dispatch {retail_log:?} vs {port_log:?}"
                ));
            }
            if fails.len() > 4 {
                break;
            }
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(seen.iter().all(|&n| n > 50), "{seen:?}");
}

#[test]
fn the_delta_time_pump_matches_retail() {
    differential!(scus(PUMP), scus(PUMP_BODY));
    let Some((mut cpu, _)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let log = Rc::new(RefCell::new(Vec::new()));
    install_handlers(&mut cpu, &log, true);
    cpu.step_budget = 400_000;
    let mut rng = Rng(0x5055_4D50);
    let (mut fails, mut ran, mut divided, mut waited) = (Vec::new(), 0, 0, 0);
    for stream_no in 0..12 {
        let stream = gen_stream(&mut rng);
        cpu.mem.load(STREAM, &stream);
        for run in 0..3000 {
            let (slot, chan) = (rng.below(4) as u8, rng.below(16) as u8);
            let mut seed = gen_playing_record(&mut rng, &stream);
            // The budget and the wait the pump weighs, in the tenths a real
            // tempo installs; the loop marker kept off this channel, so a
            // track end never re-enters at random.
            let budget: i16 = [160, 191, 107, 1 + rng.below(400) as i16][rng.below(4) as usize];
            let wait: i32 = match rng.below(4) {
                0 => 0,
                1 => i32::from(budget) - rng.below(40) as i32,
                2 => i32::from(budget) + 1 + rng.below(400) as i32,
                _ => rng.below(2000) as i32 - 100,
            };
            let sub: i16 = [-1, -1, 0, 1 + rng.below(4) as i16][rng.below(4) as usize];
            seed[0x54..0x56].copy_from_slice(&budget.to_le_bytes());
            seed[0x90..0x94].copy_from_slice(&wait.to_le_bytes());
            seed[0x52..0x54].copy_from_slice(&sub.to_le_bytes());
            seed[0x20] = 0; // loop for ever: an end-of-track rewinds
            let flags = u32::from_le_bytes(seed[0x98..0x9C].try_into().unwrap()) & !flag::LOOP_ALT;
            seed[0x98..0x9C].copy_from_slice(&flags.to_le_bytes());
            load_record(&mut cpu, slot, chan, &seed);
            let entry = if run % 2 == 0 { PUMP } else { PUMP_BODY };
            let stop = cpu.call(entry, &[slot.into(), chan.into()]);
            let mut c = lift(&seed, STREAM);
            let out = pump_delta_time(&mut c, &stream);
            use legaia_engine_audio::seq_events::PumpOutcome as P;
            if stop != Stop::Returned {
                // A stream that never supplies a wait spins in retail; the
                // port bounds the same run.
                assert!(
                    matches!(out, P::Runaway(_)),
                    "stream {stream_no} run {run}: retail {stop:?}, port {out:?}"
                );
                continue;
            }
            let retail = cpu.mem.read(rec_addr(slot, chan), STRIDE as usize);
            match &out {
                P::Ran(_) => ran += 1,
                P::Divided => divided += 1,
                P::Waited => waited += 1,
                _ => {}
            }
            let mut port = seed.clone();
            seat(&mut port, &c, STREAM);
            check(
                &format!("stream {stream_no} {out:?}"),
                run,
                &seed,
                &retail,
                &port,
                &mut fails,
            );
            if fails.len() > 4 {
                break;
            }
        }
    }
    let _ = handler_operand_bytes;
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(
        ran > 3000 && divided > 1000 && waited > 1000,
        "{ran} {divided} {waited}"
    );
}

const SEQ_CALC: u32 = 0x8006_2F98;
const REWIND: u32 = 0x8006_41EC;
const FLUSH: u32 = 0x8006_5BAC;
const BUSY: u32 = 0x801C_D2B4;
const SLOT_MASK: u32 = 0x801C_D2B8;
const SLOT_COUNT: u32 = 0x801C_DB40;
const CHANNEL_COUNT: u32 = 0x801C_DB42;

/// What a handler does to the flag word in this test, the same on both
/// sides: a fixed scramble of the word and the call, so a handler can clear
/// (or set) the bits the dispatch tests after it.
fn scramble(call: u32, flags: u32) -> u32 {
    let h = (flags ^ call.wrapping_mul(0x9E37_79B9)).wrapping_mul(0x85EB_CA6B);
    flags ^ ((h >> 13) & 0x1FF)
}

fn call_id(c: legaia_engine_audio::seq_calc::SeqCall) -> u32 {
    use legaia_engine_audio::seq_calc::SeqCall as C;
    match c {
        C::Pump => PUMP,
        C::VolUp => VOL_UP,
        C::VolDown => VOL_DOWN,
        C::Tempo => TEMPO,
        C::Stop => STOP,
        C::Start => START,
        C::Rewind => REWIND,
    }
}

#[test]
fn the_frame_dispatch_matches_retail() {
    differential!(scus(SEQ_CALC));
    use legaia_engine_audio::seq_calc::{SeqCalcState, seq_calc};
    let Some((mut cpu, _)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let trace: Rc<RefCell<Vec<(i16, i16, u32)>>> = Rc::new(RefCell::new(Vec::new()));
    let flushes = Rc::new(std::cell::Cell::new(0));
    for at in [PUMP, VOL_UP, VOL_DOWN, TEMPO, STOP, START, REWIND] {
        let t = trace.clone();
        cpu.hook(at, move |r, m| {
            let (slot, chan) = (r.r[A0] as i16, r.r[A1] as i16);
            t.borrow_mut().push((slot, chan, at));
            let rec = m.rd32(SLOT_TABLE + slot as u32 * 4) + chan as u32 * STRIDE;
            let f = m.rd32(rec + 0x98);
            m.wr32(rec + 0x98, scramble(at, f));
        });
    }
    let f = flushes.clone();
    cpu.hook(FLUSH, move |_, _| f.set(f.get() + 1));
    let mut rng = Rng(0x4341_4C43);
    let (mut fails, mut calls, mut busy_runs) = (Vec::new(), 0, 0);
    for run in 0..20_000 {
        let slots = rng.below(5) as i16;
        let chans = [0, 1, 4, 16, rng.below(17) as i16][rng.below(5) as usize];
        let mask = [u32::MAX, 0, 1, 0b1010, rng.next()][rng.below(5) as usize];
        let busy = rng.below(10) == 0;
        cpu.mem.wr32(BUSY, u32::from(busy));
        cpu.mem.wr32(SLOT_MASK, mask);
        cpu.mem.wr16(SLOT_COUNT, slots as u16);
        cpu.mem.wr16(CHANNEL_COUNT, chans as u16);
        let mut seeds = Vec::new();
        for s in 0..4u8 {
            let mut row = Vec::new();
            for c in 0..16u8 {
                let mut b = gen_record(&mut rng);
                let flags = rng.next() & 0x7FF;
                b[0x98..0x9C].copy_from_slice(&flags.to_le_bytes());
                load_record(&mut cpu, s, c, &b);
                row.push(b);
            }
            seeds.push(row);
        }
        trace.borrow_mut().clear();
        flushes.set(0);
        assert_eq!(cpu.call(SEQ_CALC, &[]), Stop::Returned);
        let retail_trace = trace.borrow().clone();

        let mut state = SeqCalcState {
            busy,
            slot_mask: mask,
            slot_count: slots,
            channel_count: chans,
        };
        let mut channels: Vec<Vec<SeqChannel>> = seeds
            .iter()
            .map(|row| row.iter().map(|b| lift(b, 0)).collect())
            .collect();
        let port = seq_calc(&mut state, &mut channels, |call, ch| {
            ch.flags = scramble(call_id(call), ch.flags);
        });
        let port_trace: Vec<(i16, i16, u32)> = port
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|(s, c, call)| (s, c, call_id(call)))
            .collect();
        if retail_trace != port_trace && fails.len() < 6 {
            fails.push(format!(
                "run {run} slots {slots} chans {chans} mask {mask:#x} busy {busy}: trace \
                 {} vs {}",
                retail_trace.len(),
                port_trace.len()
            ));
        }
        assert_eq!(cpu.mem.rd32(BUSY), u32::from(busy), "run {run}: the latch");
        assert_eq!(
            flushes.get(),
            u32::from(!busy),
            "run {run}: one flush a frame"
        );
        for s in 0..4u8 {
            for c in 0..16u8 {
                let got = cpu.mem.rd32(rec_addr(s, c) + 0x98);
                let want = channels[s as usize][c as usize].flags;
                if got != want && fails.len() < 6 {
                    fails.push(format!(
                        "run {run} slot {s} chan {c}: flags {got:#x} vs {want:#x}"
                    ));
                }
            }
        }
        calls += port_trace.len();
        busy_runs += u32::from(port.is_none());
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(calls > 100_000 && busy_runs > 1000, "{calls} {busy_runs}");
}
