//! The PsyQ `St` streaming library's sector ring off the disc: `StSetRing`
//! `FUN_8005BBF8`, `StSetStream` `FUN_8005EDC4` with its window installer
//! `FUN_8005F004`, the per-sector demux `FUN_8005F024` on its memory-source
//! path with the frame latch `FUN_8005ECD4` beneath it (entered only
//! through the demux, so not declared), `StGetNext`
//! `FUN_8005EF40` and `StFreeRing` `FUN_8005EE4C`, driven as one random
//! stream of operations over generated movies (broken chunk runs, foreign
//! sectors, seeks, end frames, ring wraps and back-pressure) against
//! `StRing`. After every operation: each slot's status word, the cursors and
//! the window (`StRing::cursors`), the trace code, and what a read or free
//! returned, the frame's payload bytes included.

use crate::common::{Rng, scus_cpu};
use psx_mdec::st_ring::{StRing, StStatus};
use psx_mdec::str_sector::{SECTOR_PAYLOAD_BYTES, VIDEO_SECTOR_MAGIC};
use psx_retail_exec::{Cpu, Memory, Stop, V0};

const SET_RING: u32 = 0x8005_BBF8;
const SET_STREAM: u32 = 0x8005_EDC4;
const SET_MASK: u32 = 0x8005_F004;
const DEMUX: u32 = 0x8005_F024;
const GET_NEXT: u32 = 0x8005_EF40;
const FREE_RING: u32 = 0x8005_EE4C;
/// `CdReady`-shaped status poll the demux opens with; answered "ready".
const CD_POLL: u32 = 0x8005_BEAC;

const RING: u32 = 0x8019_0000;
const SRC: u32 = 0x8010_0000;
const OUT: u32 = 0x8010_F000;
const HW: u32 = 0x8010_E000;
const STACK: u32 = 0x801F_FF00;

const WRITE_SLOT: u32 = 0x801C_ADBC;
const FRAME_START: u32 = 0x801C_ADC0;
const READ_SLOT: u32 = 0x801C_ADC4;
const PENDING: u32 = 0x801C_ADB4;
const NEXT_CHUNK: u32 = 0x801C_AD94;
const CUR_FRAME: u32 = 0x801C_AD90;
const FILTER: u32 = 0x801C_ADB0;
const SEEK: u32 = 0x801C_ADD0;
const START: u32 = 0x801C_ADAC;
const END: u32 = 0x801C_ADCC;
const MODE: u32 = 0x801C_AD98;
const MEM_SOURCE: u32 = 0x801C_ADC8;
const MEM_COUNTER: u32 = 0x801C_ADB8;
const NO_DRAIN: u32 = 0x801C_D208;
const TRACE: u32 = 0x8007_97B8;
const UNTOUCHED: u32 = 0xEE;

fn st_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    // The CD register pointers the demux pokes, onto scratch RAM.
    for (k, va) in [
        0x8007_9770u32,
        0x8007_9778,
        0x8007_977C,
        0x8007_9780,
        0x8007_9784,
    ]
    .iter()
    .enumerate()
    {
        cpu.mem.wr32(*va, HW + 0x10 * k as u32);
    }
    cpu.mem.wr32(0x8007_97A0, HW + 0x80);
    cpu.hook(CD_POLL, |r, m| {
        m.wr8(r.r[psx_retail_exec::A1], 0);
        r.r[V0] = 0;
    });
    Some(cpu)
}

fn sector(rng: &mut Rng, frame: u32, chunk: u16, chunks: u16, filter: u16) -> Vec<u8> {
    let mut s = vec![0u8; 2048];
    for b in s.iter_mut() {
        *b = rng.next() as u8;
    }
    s[0..2].copy_from_slice(&VIDEO_SECTOR_MAGIC.to_le_bytes());
    s[2..4].copy_from_slice(&(0x8001u16 | filter << 10).to_le_bytes());
    s[4..6].copy_from_slice(&chunk.to_le_bytes());
    s[6..8].copy_from_slice(&chunks.to_le_bytes());
    s[8..12].copy_from_slice(&frame.to_le_bytes());
    let size = u32::from(chunks) * SECTOR_PAYLOAD_BYTES as u32 - rng.below(0x100);
    s[12..16].copy_from_slice(&size.to_le_bytes());
    s
}

fn retail_cursors(m: &mut Memory) -> psx_mdec::st_ring::StCursors {
    psx_mdec::st_ring::StCursors {
        write_slot: m.rd32(WRITE_SLOT) as usize,
        frame_start_slot: m.rd32(FRAME_START) as usize,
        read_slot: m.rd32(READ_SLOT) as usize,
        cur_frame: m.rd32(CUR_FRAME),
        next_chunk: u32::from(m.rd16(NEXT_CHUNK)),
        stream_filter: m.rd32(FILTER) as u16,
        seek_armed: m.rd32(SEEK) != 0,
        start_frame: m.rd32(START),
        end_frame: m.rd32(END),
        frame_pending: m.rd32(PENDING) != 0,
        mode_flag: m.rd32(MODE) != 0,
    }
}

#[test]
fn the_st_ring_matches_the_library() {
    differential!(
        scus(SET_RING),
        scus(SET_STREAM),
        scus(DEMUX),
        scus(GET_NEXT),
        scus(FREE_RING)
    );
    let Some(mut cpu) = st_cpu() else {
        return;
    };
    let mut rng = Rng(0x5354_5249);
    let mut traces = [0usize; 11];
    let (mut reads, mut frees, mut payload_bytes) = (0usize, 0usize, 0usize);
    for movie in 0..300 {
        let n = 4 + rng.below(0x1D) as usize;
        let start = if movie % 3 == 0 { 2 + rng.below(6) } else { 0 };
        let end = match movie % 4 {
            0 => 0,
            1 => u32::MAX,
            _ => start + 4 + rng.below(30),
        };
        // A fresh library: the globals the two calls leave alone start clear.
        for va in [
            WRITE_SLOT,
            FRAME_START,
            READ_SLOT,
            PENDING,
            FILTER,
            SEEK,
            START,
            END,
        ] {
            cpu.mem.wr32(va, 0);
        }
        cpu.mem.load(RING, &vec![0xA5; n * 2048]);
        assert_eq!(cpu.call(SET_RING, &[RING, n as u32]), Stop::Returned);
        cpu.mem.wr32(STACK + 0x10, 0);
        assert_eq!(cpu.call(SET_STREAM, &[0, start, end, 0]), Stop::Returned);
        cpu.mem.wr32(MEM_SOURCE, SRC);
        cpu.mem.wr32(NO_DRAIN, 1);
        let mut ring = StRing::set_ring(n);
        ring.set_stream(0, start, end);

        let mut frame = 1u32;
        let mut chunk = 0u16;
        // A frame never holds the whole ring: one that did would run off its
        // end in retail, the room test passing it at slot 0.
        let most = 6.min(n as u32 - 1);
        let mut chunks = 1 + rng.below(most) as u16;
        let mut held: Vec<usize> = Vec::new();
        for op in 0..400 {
            let ctx = format!("movie {movie} op {op} n {n} start {start} end {end:#x}");
            match rng.below(10) {
                0..=5 => {
                    // The next sector of the movie, now and then broken.
                    let (mut f, mut c, mut k, mut filter) = (frame, chunk, chunks, 0u16);
                    match rng.below(40) {
                        0 => c = c.wrapping_add(1),
                        1 => f += 1,
                        2 => filter = 1,
                        _ => {}
                    }
                    let mut s = sector(&mut rng, f, c, k, filter);
                    if rng.below(60) == 0 {
                        s[0] ^= 0x40;
                    }
                    cpu.mem.wr32(MEM_COUNTER, 0);
                    cpu.mem.load(SRC, &s);
                    cpu.mem.wr32(TRACE, UNTOUCHED);
                    let seeking = ring.cursors().seek_armed && ring.cursors().start_frame != 0;
                    assert_eq!(cpu.call(DEMUX, &[]), Stop::Returned, "{ctx}");
                    let step = ring.deliver_sector(&s);
                    let trace = cpu.mem.rd32(TRACE);
                    if trace == UNTOUCHED {
                        assert!(
                            seeking && step.status == StStatus::NotForStream,
                            "a silent drop is the seek's, {ctx}: port {step:?}"
                        );
                    } else {
                        assert_eq!(trace, step.status as u32, "trace, {ctx}");
                        traces[trace as usize] += 1;
                    }
                    // Advance the movie on an accepted sector.
                    if step.status == StStatus::Accepted || rng.below(3) == 0 {
                        chunk += 1;
                        if chunk >= chunks {
                            chunk = 0;
                            frame += 1;
                            chunks = 1 + rng.below(most) as u16;
                        }
                    }
                    if step.end_of_stream {
                        frame = start.max(1);
                        chunk = 0;
                    }
                }
                6..=7 => {
                    assert_eq!(cpu.call(GET_NEXT, &[OUT, OUT + 4]), Stop::Returned, "{ctx}");
                    let got = cpu.regs.r[V0];
                    let port = ring.get_next();
                    assert_eq!(got == 0, port.is_some(), "get_next, {ctx}");
                    if let Some(f) = port {
                        let payload = cpu.mem.rd32(OUT);
                        let header = cpu.mem.rd32(OUT + 4);
                        let base = RING + 32 * n as u32;
                        assert_eq!(payload, base + f.offset as u32, "payload address, {ctx}");
                        assert_eq!(header, RING + 32 * f.slot as u32, "header address, {ctx}");
                        assert_eq!(cpu.mem.rd32(header + 8), f.frame_number, "{ctx}");
                        assert_eq!(cpu.mem.rd32(header + 0xC), f.frame_size_bytes, "{ctx}");
                        assert_eq!(
                            usize::from(cpu.mem.rd16(header + 6)) * SECTOR_PAYLOAD_BYTES,
                            f.len,
                            "{ctx}"
                        );
                        let bytes = cpu.mem.read(payload, f.len);
                        assert!(
                            bytes == ring.payload()[f.offset..f.offset + f.len],
                            "payload bytes, {ctx}"
                        );
                        payload_bytes += f.len;
                        held.push(f.slot);
                        reads += 1;
                    }
                }
                _ => {
                    // Free the oldest held frame, or a slot nobody holds.
                    let slot = if held.is_empty() || rng.below(8) == 0 {
                        rng.below(n as u32) as usize
                    } else {
                        held.remove(0)
                    };
                    let addr = RING + 32 * n as u32 + (slot * SECTOR_PAYLOAD_BYTES) as u32;
                    assert_eq!(cpu.call(FREE_RING, &[addr]), Stop::Returned, "{ctx}");
                    let ok = ring.free_ring(slot);
                    assert_eq!(cpu.regs.r[V0] == 0, ok, "free_ring {slot}, {ctx}");
                    frees += usize::from(ok);
                }
            }
            for i in 0..n {
                let got = cpu.mem.rd16(RING + 32 * i as u32);
                assert_eq!(got, ring.slot_status(i), "slot {i} status, {ctx}");
            }
            let mut want = ring.cursors();
            // The port has no memory source and keeps no pending latch across
            // a call: the latch runs inline on both sides here.
            want.mode_flag = false;
            assert_eq!(retail_cursors(&mut cpu.mem), want, "cursors, {ctx}");
        }
    }
    assert!(
        reads > 1000
            && frees > 500
            && payload_bytes > 0
            && [4, 5, 6, 7, 8, 10].iter().all(|t| traces[*t] > 0),
        "reads {reads} frees {frees} traces {traces:?}"
    );
}

/// The window installer `FUN_8005F004` on its own: the three words it stores
/// against `StRing::set_mask`.
#[test]
fn the_window_installer_matches_set_mask() {
    differential!(scus(SET_MASK));
    let Some(mut cpu) = st_cpu() else {
        return;
    };
    let mut rng = Rng(0x4D41_534B);
    let mut ring = StRing::retail();
    for _ in 0..200 {
        let (seek, start, end) = (rng.below(2), rng.next(), rng.next());
        assert_eq!(cpu.call(SET_MASK, &[seek, start, end]), Stop::Returned);
        ring.set_mask(seek != 0, start, end);
        let c = ring.cursors();
        assert_eq!(
            (
                cpu.mem.rd32(SEEK) != 0,
                cpu.mem.rd32(START),
                cpu.mem.rd32(END)
            ),
            (c.seek_armed, c.start_frame, c.end_frame)
        );
    }
}
