//! The STR overlay's STRv2 / v3 side off the disc (PROT 0970, base
//! `0x801CE818`): the VLC table unpacker `FUN_801F1A00` against
//! `psx_mdec::strv2_table`, and the bitstream decoder `FUN_801D070C` against
//! `psx_mdec::strv2_decode::decode_frame`. No movie on the released disc is
//! STRv2, so the decoder runs over generated bitstreams through the table the
//! retail unpacker built, v2 and v3 headers both, in one call and in the
//! retail chunked form (a small output limit, then resumed calls) the MDEC
//! feed drives.

use crate::common::{Rng, disc, scus_cpu};
use psx_mdec::strv2_table::{STRV2_PACKED_VA, STRV2_TABLE_BYTES, STRV2_TABLE_VA};
use psx_retail_exec::{Cpu, Stop, V0};

const UNPACK: u32 = 0x801F_1A00;
const DECODE: u32 = 0x801D_070C;
const BASE: u32 = 0x801C_E818;
/// The decoder's chunk size in words (`t6 = out + 2 * [0x801D06D8]`).
const CHUNK: u32 = 0x801D_06D8;
const FRAME: u32 = 0x8010_0000;
const OUT: u32 = 0x8014_0000;

fn str_cpu() -> Option<(Cpu, Vec<u8>)> {
    let mut cpu = scus_cpu()?;
    let image = disc()?.read_entry(970).ok()?;
    cpu.mem.load(BASE, &image);
    cpu.step_budget = 400_000_000;
    Some((cpu, image))
}

#[test]
fn the_strv2_table_unpack_matches_the_overlay() {
    differential!(prot(970, UNPACK));
    let Some((mut cpu, image)) = str_cpu() else {
        return;
    };
    // The shipped blob first.
    assert_eq!(cpu.call(UNPACK, &[STRV2_TABLE_VA]), Stop::Returned);
    let retail = cpu.mem.read(STRV2_TABLE_VA, STRV2_TABLE_BYTES);
    let port: Vec<u8> = psx_mdec::strv2_table::unpack_from_overlay(&image, BASE)
        .expect("the shipped table")
        .iter()
        .flat_map(|h| h.to_le_bytes())
        .collect();
    assert_eq!(retail, port, "the shipped VLC table");

    // Then generated streams in the blob's place: literal runs, back
    // references at short and long distances, mode switches back to
    // literals, landing exactly on the table's size: the routine itself sits
    // right above the table, so a longer stream would overwrite its own code.
    let mut rng = Rng(0x5752_7632);
    let mut copies = 0usize;
    for case in 0..40 {
        let mut packed = Vec::new();
        let mut made = 0usize;
        let mut distance = 0usize;
        while made < STRV2_TABLE_BYTES {
            match rng.below(6) {
                0 if made > 0 => {
                    // A distance escape (`0xF100..=0xFEFF`, distances 1..=0xE00).
                    distance = 1 + rng.below(made.min(0xE00) as u32) as usize;
                    let word = distance as u32 + 0xF0FF;
                    packed.push((word >> 8) as u8);
                    packed.push(word as u8);
                }
                1 => {
                    packed.push(0xF0);
                    distance = 0;
                }
                _ => {
                    let run = (1 + rng.below(0x40) as usize).min(STRV2_TABLE_BYTES - made);
                    packed.push((run - 1) as u8);
                    if distance == 0 {
                        packed.extend((0..run).map(|_| rng.next() as u8));
                    } else {
                        copies += run;
                    }
                    made += run;
                }
            }
        }
        packed.extend_from_slice(&[0xFF, 0xFF]);
        cpu.mem.load(STRV2_PACKED_VA, &packed);
        cpu.mem.load(STRV2_TABLE_VA, &vec![0xEE; made]);
        assert_eq!(cpu.call(UNPACK, &[STRV2_TABLE_VA]), Stop::Returned);
        let retail = cpu.mem.read(STRV2_TABLE_VA, made);
        let mut port = psx_mdec::strv2_table::unpack_lz(&packed).expect("generated stream");
        psx_mdec::strv2_table::de_delta(&mut port).expect("long enough");
        assert_eq!(retail, port, "case {case}");
    }
    assert!(copies > 100_000);
}

#[test]
fn the_strv2_decoder_matches_the_overlay() {
    differential!(prot(970, DECODE));
    let Some((mut cpu, image)) = str_cpu() else {
        return;
    };
    let table = psx_mdec::strv2_table::unpack_from_overlay(&image, BASE).expect("table");
    let table_bytes: Vec<u8> = table.iter().flat_map(|h| h.to_le_bytes()).collect();
    cpu.mem.load(STRV2_TABLE_VA, &table_bytes);
    let mut rng = Rng(0x0053_5452);
    let (mut v2, mut v3, mut chunked, mut skipped, mut codes) = (0, 0, 0, 0, 0usize);
    for case in 0..600 {
        let version = if case % 2 == 0 { 2 } else { 3 };
        let mut frame: Vec<u16> = vec![rng.next() as u16, 0x3800, rng.below(64) as u16, version];
        frame.extend((0..6000).map(|_| rng.next() as u16));
        let Ok(port) = psx_mdec::strv2_decode::decode_frame(&frame, &table) else {
            // The end marker never came up inside the generated words.
            skipped += 1;
            continue;
        };
        let bytes: Vec<u8> = frame.iter().flat_map(|h| h.to_le_bytes()).collect();
        cpu.mem.load(FRAME, &bytes);
        let span = port.len() * 2 + 0x40;
        cpu.mem.load(OUT, &vec![0xEE; span]);
        // Every third frame in small chunks, resumed with `a0 = 0` until the
        // routine reports the frame done.
        let chunk = if case % 3 == 0 {
            0x40 + rng.below(0x200)
        } else {
            0x40_0000
        };
        cpu.mem.wr32(CHUNK, chunk);
        let mut calls = 0;
        let mut a0 = FRAME;
        loop {
            // The resumed call sets its limit from the cursor it saved.
            assert_eq!(cpu.call(DECODE, &[a0, OUT, STRV2_TABLE_VA]), Stop::Returned);
            calls += 1;
            if cpu.regs.r[V0] == 0 {
                break;
            }
            assert_eq!(cpu.regs.r[V0], 1);
            a0 = 0;
            assert!(calls < 100_000);
        }
        if calls > 1 {
            chunked += 1;
        }
        let retail = cpu.mem.read(OUT, span);
        let mut port_bytes: Vec<u8> = port.iter().flat_map(|c| c.to_le_bytes()).collect();
        port_bytes.resize(span, 0xEE);
        if retail != port_bytes {
            let at = retail
                .iter()
                .zip(&port_bytes)
                .position(|(a, b)| a != b)
                .unwrap();
            panic!(
                "case {case} (v{version}, {calls} calls): first difference at code {} \
                 (retail {:#06x}, port {:#06x})",
                at / 2,
                u16::from_le_bytes([retail[at & !1], retail[at | 1]]),
                u16::from_le_bytes([port_bytes[at & !1], port_bytes[at | 1]]),
            );
        }
        codes += port.len();
        if version == 2 {
            v2 += 1;
        } else {
            v3 += 1;
        }
    }
    eprintln!("v2 {v2} v3 {v3} chunked {chunked} skipped {skipped} codes {codes}");
    assert!(
        v2 > 50 && v3 > 50 && chunked > 20,
        "v2 {v2} v3 {v3} chunked {chunked}"
    );
}
