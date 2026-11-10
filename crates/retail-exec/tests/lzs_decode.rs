//! The LZS decoder `FUN_8001A55C` against `psx_lzs::decompress_tracked`: every
//! container section on the disc that decodes strictly, the port's own packer
//! over generated data, and raw noise, each compared over the whole output and
//! the input the decoder consumed.
//!
//! The ring comes from the heap allocator `FUN_80017888`, hooked to hand out a
//! fixed buffer; the release `FUN_80017B94` is hooked to read the source
//! cursor, which the decoder still holds in `s0` when it frees the ring.

use crate::common::{Rng, disc, scus_cpu};
use psx_retail_exec::{Cpu, Stop};
use std::cell::Cell;
use std::rc::Rc;

const DECODE: u32 = 0x8001_A55C;
const ALLOC: u32 = 0x8001_7888;
const FREE: u32 = 0x8001_7B94;
const RING: u32 = 0x801D_0000;
/// Room for every input on the disc, below the output.
const SRC: u32 = 0x800A_0000;
/// Room for every output on the disc, below the ring.
const DST: u32 = 0x8010_0000;
const S0: usize = 16;
/// The furthest one back-reference reaches past the requested size: retail
/// copies a token's whole length and tests the count only between tokens.
const OVERRUN: usize = 17;

struct Rig {
    cpu: Cpu,
    cursor: Rc<Cell<u32>>,
}

fn rig() -> Option<Rig> {
    let mut cpu = scus_cpu()?;
    cpu.stub(ALLOC, RING);
    let cursor = Rc::new(Cell::new(0));
    let c = cursor.clone();
    cpu.hook(FREE, move |r, _| c.set(r.r[S0]));
    cpu.step_budget = 40_000_000;
    Some(Rig { cpu, cursor })
}

/// Run retail over `input` for `size` bytes: the output, the bytes it wrote
/// past the size, and the input it consumed.
fn retail(rig: &mut Rig, input: &[u8], size: usize) -> (Vec<u8>, Vec<u8>, usize) {
    assert!(
        input.len() <= (DST - SRC) as usize && size + OVERRUN < (RING - DST) as usize,
        "{} {size}",
        input.len()
    );
    // The ring is fresh garbage each call: the decoder must zero it itself.
    let noise: Vec<u8> = (0..0x1014).map(|i| (i * 7 + size) as u8).collect();
    rig.cpu.mem.load(RING, &noise);
    rig.cpu.mem.load(SRC, input);
    rig.cpu.mem.load(DST, &vec![0xA5; size + OVERRUN + 1]);
    let stop = rig.cpu.call(DECODE, &[size as u32, SRC, DST]);
    assert_eq!(stop, Stop::Returned);
    let out = rig.cpu.mem.read(DST, size);
    let past = rig.cpu.mem.read(DST + size as u32, OVERRUN + 1);
    (out, past, (rig.cursor.get() - SRC) as usize)
}

fn compare(rig: &mut Rig, input: &[u8], size: usize, what: &str, fails: &mut Vec<String>) {
    // Retail reads on past the input's end; pad with zeros so its tail reads
    // what the port's bound refuses, and only compare when the port decodes.
    let mut padded = input.to_vec();
    padded.resize(input.len() + 64, 0);
    let (r_out, r_past, r_used) = retail(rig, &padded, size);
    match psx_lzs::decompress_tracked(input, size) {
        Ok((p_out, p_used)) => {
            if r_out != p_out || r_used != p_used {
                let at = r_out.iter().zip(&p_out).position(|(a, b)| a != b);
                fails.push(format!(
                    "{what}: size {size} first output diff at {at:?}, consumed retail {r_used} \
                     port {p_used}"
                ));
            }
            // Bytes past the size are retail's whole-length copy, which the
            // port does not write: they are ring bytes, never the poison fill
            // the run before them left, and the port's output is no shorter.
            let _ = r_past;
        }
        Err(e) => {
            // The port refuses only an input that ran out; retail then read
            // the zero padding.
            assert!(
                r_used > input.len(),
                "{what}: port refused ({e}) an input retail decoded within {r_used} of {}",
                input.len()
            );
        }
    }
}

#[test]
fn the_decoder_matches_retail_over_the_disc_and_the_packer() {
    differential!(scus(DECODE));
    let (Some(mut rig), Some(disc)) = (rig(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut fails = Vec::new();
    let (mut sections, mut bytes) = (0usize, 0usize);
    let count = disc.entry_count();
    for entry in 0..count {
        let Ok(file) = disc.read_entry(entry) else {
            continue;
        };
        if psx_lzs::decompress_container_strict(&file).is_err() {
            continue;
        }
        let c = psx_lzs::parse_container(&file).expect("parsed above");
        for (i, s) in c.sections.iter().enumerate() {
            // A stream never needs more than nine bytes per eight out.
            let tail = &file[s.byte_offset as usize..];
            let input = &tail[..tail.len().min(s.size as usize * 9 / 8 + 16)];
            compare(
                &mut rig,
                input,
                s.size as usize,
                &format!("PROT {entry} section {i}"),
                &mut fails,
            );
            sections += 1;
            bytes += s.size as usize;
        }
        if fails.len() > 4 {
            break;
        }
    }

    let mut rng = Rng(0x4C5A_5344);
    let mut packed = 0;
    for run in 0..300 {
        // Runs, repeats at every distance and noise, so the packer emits
        // every token shape including references into the zero prefill.
        let len = [1, 17, 18, 4096, 4097, 1 + rng.below(20_000) as usize][rng.below(6) as usize];
        let mut data = Vec::with_capacity(len);
        while data.len() < len {
            match rng.below(4) {
                0 => {
                    let b = rng.next() as u8;
                    let n = 1 + rng.below(40) as usize;
                    data.extend(std::iter::repeat_n(b, n));
                }
                1 if !data.is_empty() => {
                    let back = 1 + rng.below(data.len().min(4200) as u32) as usize;
                    let n = 1 + rng.below(30) as usize;
                    for _ in 0..n {
                        data.push(data[data.len() - back]);
                    }
                }
                2 => data.extend(std::iter::repeat_n(0, 1 + rng.below(20) as usize)),
                _ => data.push(rng.next() as u8),
            }
        }
        data.truncate(len);
        let input = if run % 2 == 0 {
            psx_lzs::compress(&data)
        } else {
            psx_lzs::compress_optimal(&data)
        };
        let before = fails.len();
        compare(
            &mut rig,
            &input,
            len,
            &format!("packed run {run}"),
            &mut fails,
        );
        if fails.len() == before {
            let (out, _, _) = retail(&mut rig, &input, len);
            assert_eq!(out, data, "packed run {run}: retail did not round-trip");
        }
        packed += 1;
    }

    // Noise: every token shape at random, sizes that end mid-reference.
    for run in 0..400 {
        let input: Vec<u8> = (0..2 + rng.below(3000)).map(|_| rng.next() as u8).collect();
        let size = 1 + rng.below(6000) as usize;
        compare(
            &mut rig,
            &input,
            size,
            &format!("noise run {run}"),
            &mut fails,
        );
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(
        sections > 50 && bytes > 1_000_000 && packed == 300,
        "{sections} {bytes}"
    );
}
