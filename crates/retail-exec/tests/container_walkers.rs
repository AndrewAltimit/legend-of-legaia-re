//! The three `SCUS_942.54` walkers over the container layer's two shapes,
//! against `legaia_container_formats`:
//!
//! - `FUN_80020224`, the descriptor-pair walker, against
//!   `walk_descriptor_pairs`: every dispatch `FUN_8001F05C(base + offset,
//!   type_size, param, 0)` in order and the OR of their returns, over
//!   non-positive and filled counts.
//! - `FUN_8002541C`'s DATA_FIELD branch (asset id `0x14`), against
//!   `parse_streaming`: every chunk it hands the dispatcher
//!   (`FUN_8001F05C(body, header, 0, 1)`) and the summary word.
//! - `FUN_8001FE70`, the type-`0x02`-terminated walker, against
//!   `parse_streaming_with(.., TypeTwo)`: every TIM chunk it uploads
//!   (`FUN_800198E0`) and the body it stops on.
//!
//! The buffers are generated chunk streams, and the disc's own DATA_FIELD
//! carriers run through the `0x14` branch too.

use crate::common::{Rng, disc, scus_cpu};
use legaia_container_formats::{
    StreamTerminator, parse_streaming, parse_streaming_with, walk_descriptor_pairs,
};
use psx_retail_exec::{A0, A1, A2, A3, Cpu, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const PAIRS: u32 = 0x8002_0224;
const DRIVER: u32 = 0x8002_541C;
const SOUND_WALK: u32 = 0x8001_FE70;
const DISPATCH: u32 = 0x8001_F05C;
const UPLOAD: u32 = 0x8001_98E0;
const ALLOC: u32 = 0x8001_7888;
/// The load buffer every walker reads, `_DAT_8007B85C`.
const BUFFER_PTR: u32 = 0x8007_B85C;
const BUF: u32 = 0x8010_0000;
const PARAM: u32 = 0x8011_2233;

type Calls = Rc<RefCell<Vec<[u32; 4]>>>;

/// A CPU whose dispatcher logs its four arguments and answers a scripted bit.
fn walker_cpu() -> Option<(Cpu, Calls)> {
    let mut cpu = scus_cpu()?;
    let calls: Calls = Rc::default();
    for va in [
        0x8005_67A8, // printf
        0x8005_8104, // cache flush
        0x8005_FB84, // VSync
        0x8003_DE7C, // CD wait
        0x8003_D26C, // block copy
    ] {
        cpu.stub(va, 0);
    }
    cpu.stub(ALLOC, 0x8018_0000);
    {
        let calls = calls.clone();
        cpu.hook(DISPATCH, move |r, _| {
            let mut c = calls.borrow_mut();
            c.push([r.r[A0], r.r[A1], r.r[A2], r.r[A3]]);
            r.r[V0] = 1 << (c.len() % 29);
        });
    }
    {
        let calls = calls.clone();
        cpu.hook(UPLOAD, move |r, _| {
            calls.borrow_mut().push([r.r[A0], u32::MAX, 0, 0]);
            r.r[V0] = 0;
        });
    }
    cpu.mem.wr32(BUFFER_PTR, BUF);
    Some((cpu, calls))
}

/// A chunk stream: `n` chunks of `types`, then the terminator header (and its
/// body, for a type-`0x02` end).
fn stream(rng: &mut Rng, types: &[u8], end: u32) -> Vec<u8> {
    let mut b = Vec::new();
    let n = rng.below(10);
    for _ in 0..n {
        let t = types[rng.below(types.len() as u32) as usize];
        let size = 4 * (1 + rng.below(24));
        b.extend_from_slice(&((u32::from(t) << 24) | size).to_le_bytes());
        b.extend((0..size).map(|_| rng.next() as u8));
    }
    b.extend_from_slice(&end.to_le_bytes());
    if end & 0xFF_FFFF != 0 {
        b.extend((0..(end & 0xFF_FFFF)).map(|_| rng.next() as u8));
    }
    b.extend((0..32).map(|_| rng.next() as u8));
    b
}

#[test]
fn the_descriptor_walk_dispatches_what_retail_dispatches() {
    differential!(scus(PAIRS));
    let Some((mut cpu, calls)) = walker_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5041_4952);
    let (mut empty, mut walked) = (0, 0);
    for round in 0..4_000 {
        let count: i32 = match rng.below(6) {
            0 => 0,
            1 => -(1 + rng.below(3) as i32),
            _ => 1 + rng.below(12) as i32,
        };
        let mut file = Vec::new();
        file.extend_from_slice(&(count as u32).to_le_bytes());
        file.extend_from_slice(&rng.next().to_le_bytes());
        for _ in 0..count.max(0) {
            file.extend_from_slice(&rng.next().to_le_bytes());
            file.extend_from_slice(&(rng.below(0x8000) & !3).to_le_bytes());
        }
        file.extend((0..16).map(|_| rng.next() as u8));
        cpu.mem.load(BUF, &file);
        calls.borrow_mut().clear();
        assert_eq!(cpu.call(PAIRS, &[PARAM]), Stop::Returned, "round {round}");
        let retail_or = cpu.regs.r[V0];
        let retail: Vec<[u32; 4]> = calls.borrow().clone();

        let mut port = Vec::new();
        let mut n = 0usize;
        let port_or = walk_descriptor_pairs(&file, |d| {
            n += 1;
            port.push([
                BUF.wrapping_add(d.data_offset),
                (u32::from(d.type_byte) << 24) | d.size,
                PARAM,
                0,
            ]);
            1 << (n % 29)
        })
        .expect("walk");
        assert_eq!(retail, port, "round {round}: dispatches");
        assert_eq!(retail_or, port_or, "round {round}: summary");
        if count > 0 {
            walked += 1;
        } else {
            empty += 1;
        }
    }
    assert!(empty > 500 && walked > 2_000, "{empty} {walked}");
}

/// Run the `0x14` branch over `buf` and the port over the same bytes.
fn data_field(cpu: &mut Cpu, calls: &Calls, buf: &[u8]) -> Result<(), String> {
    cpu.mem.load(BUF, buf);
    calls.borrow_mut().clear();
    // `FUN_800255B8`, the file load, answers the size it read.
    cpu.stub(0x8002_55B8, buf.len() as u32);
    assert_eq!(cpu.call(DRIVER, &[0x14]), Stop::Returned);
    let retail_or = cpu.regs.r[V0];
    let retail: Vec<[u32; 4]> = calls.borrow().clone();
    let report = parse_streaming(buf, 1 << 20).map_err(|e| e.to_string())?;
    let port: Vec<[u32; 4]> = report
        .chunks
        .iter()
        .map(|c| {
            [
                BUF + c.header_offset as u32 + 4,
                (u32::from(c.type_byte) << 24) | c.size,
                0,
                1,
            ]
        })
        .collect();
    let port_or = (1..=port.len()).fold(0u32, |a, n| a | 1 << (n % 29));
    if retail != port {
        return Err(format!(
            "dispatches retail {} port {} (terminated {})",
            retail.len(),
            port.len(),
            report.terminated
        ));
    }
    if !report.terminated {
        return Err("the port did not reach the terminator".into());
    }
    if retail_or != port_or {
        return Err(format!("summary retail {retail_or:#x} port {port_or:#x}"));
    }
    Ok(())
}

#[test]
fn the_data_field_walk_dispatches_what_retail_dispatches() {
    differential!(scus(DRIVER));
    let Some((mut cpu, calls)) = walker_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4446_4C44);
    for round in 0..3_000 {
        let end = (rng.below(256) << 24) & 0xFF00_0000;
        let buf = stream(&mut rng, &[0, 1, 2, 4, 6, 9, 0x14, 0x81], end);
        data_field(&mut cpu, &calls, &buf).unwrap_or_else(|e| panic!("round {round}: {e}"));
    }
    // The disc's DATA_FIELD carriers: every PROT entry whose stream the port
    // reads to a terminator.
    let disc = disc().expect("disc");
    let mut carriers = 0;
    for entry in 0..disc.entry_count() {
        let Ok(bytes) = disc.read_entry(entry) else {
            continue;
        };
        let Ok(r) = parse_streaming(&bytes, 4096) else {
            continue;
        };
        if !r.terminated || !r.all_known_types || !r.all_magic_ok || r.chunks.len() < 2 {
            continue;
        }
        let buf = &bytes[..r.bytes_consumed];
        data_field(&mut cpu, &calls, buf).unwrap_or_else(|e| panic!("PROT {entry:04}: {e}"));
        carriers += 1;
    }
    assert!(carriers > 20, "{carriers} disc carriers");
}

#[test]
fn the_sound_pack_walk_uploads_what_retail_uploads() {
    differential!(scus(SOUND_WALK));
    let Some((mut cpu, calls)) = walker_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x534E_4457);
    let (mut uploads, mut type_two, mut zero) = (0, 0, 0);
    for round in 0..4_000 {
        // The lead chunk is copied whole and never type-tested; the rest are
        // walked to a type-`0x02` chunk or a zero-size header.
        let lead_type = [0u8, 2, 3, 4, 5][rng.below(5) as usize];
        let lead_size = 4 * (1 + rng.below(16));
        let end = if rng.below(2) == 0 {
            type_two += 1;
            0x0200_0000 | (4 * (1 + rng.below(16)))
        } else {
            zero += 1;
            [0u32, 0x0300_0000, 0x0400_0000][rng.below(3) as usize]
        };
        let mut buf = Vec::new();
        buf.extend_from_slice(&((u32::from(lead_type) << 24) | lead_size).to_le_bytes());
        buf.extend((0..lead_size).map(|_| rng.next() as u8));
        let lead_len = buf.len();
        buf.extend(stream(&mut rng, &[0, 1, 1, 3, 4, 5], end));
        cpu.mem.load(BUF, &buf);
        calls.borrow_mut().clear();
        assert_eq!(
            cpu.call(SOUND_WALK, &[BUF]),
            Stop::Returned,
            "round {round}"
        );
        let retail_end = cpu.regs.r[V0];
        let retail: Vec<u32> = calls.borrow().iter().map(|c| c[0]).collect();

        let report = parse_streaming_with(&buf, 1 << 20, StreamTerminator::TypeTwo).expect("walk");
        assert!(report.terminated, "round {round}: no terminator");
        assert_eq!(report.chunks[0].header_offset, 0);
        let port: Vec<u32> = report.chunks[1..]
            .iter()
            .filter(|c| c.type_byte == 1)
            .map(|c| BUF + c.header_offset as u32 + 4)
            .collect();
        uploads += port.len();
        assert_eq!(retail, port, "round {round}: uploads");
        // Where the walk stopped: the type-`0x02` chunk's body, or one word
        // past a zero-size header.
        let last = report.chunks.last().expect("chunks");
        let port_end = if last.type_byte == 2 && last.header_offset >= lead_len {
            BUF + last.header_offset as u32 + 4
        } else {
            BUF + report.bytes_consumed as u32
        };
        assert_eq!(retail_end, port_end, "round {round}: end");
    }
    assert!(uploads > 5_000 && type_two > 1_000 && zero > 1_000);
}
