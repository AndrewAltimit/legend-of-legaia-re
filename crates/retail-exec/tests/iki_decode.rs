//! The STR overlay's Iki frame converter off the disc (PROT 0970, base
//! `0x801CE818`): the qscale/DC table's LZSS `FUN_801D0604` and the
//! bitstream-to-MDEC-code walk `FUN_801D0378` above it, run over the real
//! frames of the disc's movies against `psx_mdec::iki_lzss_decompress` and
//! `psx_mdec::iki_mdec_codes`. The converter's whole output buffer is
//! compared, header copy and `0xFE00` padding included, so every MDEC word the
//! DMA would carry is checked; `MdecDecoder::decode_frame` turns the same list
//! into pixels.

use crate::common::{Rng, disc, scus_cpu};
use psx_mdec::str_sector::StrFrameAssembler;
use psx_retail_exec::{Cpu, Stop, V0};

const LZSS: u32 = 0x801D_0604;
const CONVERT: u32 = 0x801D_0378;
const BASE: u32 = 0x801C_E818;
const FRAME: u32 = 0x8014_0000;
const OUT: u32 = 0x8010_0000;
const SRC: u32 = 0x8014_0000;

fn str_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem.load(BASE, &disc()?.read_entry(970).ok()?);
    cpu.step_budget = 200_000_000;
    Some(cpu)
}

/// Every video frame payload of a movie, with its header's dimensions.
fn movie_frames(name: &str) -> Vec<(u16, u16, Vec<u8>)> {
    let d = disc().expect("disc");
    let (lba, size) =
        psx_iso::iso9660::find_path_in_image(d.image(), &format!("MOV/{name}")).expect(name);
    let data = d
        .read_disc_sectors(lba, size.div_ceil(2048))
        .expect("movie sectors");
    let mut asm = StrFrameAssembler::new();
    let mut out = Vec::new();
    for sector in data.as_chunks::<2048>().0 {
        if let Ok(Some((hdr, bs))) = asm.push_sector(sector) {
            out.push((hdr.width, hdr.height, bs));
        }
    }
    out
}

#[test]
fn the_iki_table_lzss_matches_the_overlay() {
    differential!(prot(970, LZSS));
    let Some(mut cpu) = str_cpu() else {
        return;
    };
    let mut rng = Rng(0x1C1_1255);
    let (mut refs, mut long_refs, mut clamped) = (0usize, 0usize, 0usize);
    for case in 0..3000 {
        // A stream the encoder could have written: literals and back
        // references that stay inside what is already out, short and long
        // offsets, lengths that run past the requested size.
        let out_len = 1 + rng.below(if case % 4 == 0 { 4000 } else { 300 }) as usize;
        let mut src = Vec::new();
        let mut made = 0usize;
        while made < out_len {
            let flag_at = src.len();
            src.push(0u8);
            for bit in 0..8 {
                if made >= out_len {
                    break;
                }
                if made > 0 && rng.below(3) == 0 {
                    src[flag_at] |= 1 << bit;
                    let cap = if rng.below(8) == 0 { 256 } else { 12 };
                    let size = rng.below(cap) as usize;
                    let off = 1 + rng.below(made.min(0x8000) as u32) as usize;
                    src.push(size as u8);
                    if off - 1 > 0x7F || rng.below(4) == 0 {
                        src.push(0x80 | ((off - 1) >> 8) as u8);
                        src.push((off - 1) as u8);
                        long_refs += 1;
                    } else {
                        src.push((off - 1) as u8);
                    }
                    refs += 1;
                    if made + size + 3 > out_len {
                        clamped += 1;
                    }
                    made += size + 3;
                } else {
                    src.push(rng.next() as u8);
                    made += 1;
                }
            }
        }
        // Retail reads a byte past a final literal run; give it one.
        src.extend_from_slice(&[0, 0, 0]);
        cpu.mem.load(SRC, &src);
        cpu.mem.load(OUT, &vec![0xEE; out_len + 0x40]);
        assert_eq!(
            cpu.call(LZSS, &[OUT, SRC, out_len as u32]),
            Stop::Returned,
            "case {case}"
        );
        assert_eq!(cpu.regs.r[V0], OUT);
        let retail = cpu.mem.read(OUT, out_len + 0x40);
        let mut port = psx_mdec::iki_lzss_decompress(&src, out_len);
        port.resize(out_len + 0x40, 0xEE);
        assert_eq!(retail, port, "case {case}: out_len {out_len}");
    }
    assert!(refs > 10_000 && long_refs > 1000 && clamped > 100);
}

#[test]
fn the_iki_converter_matches_the_overlay_on_the_disc_movies() {
    differential!(prot(970, CONVERT));
    let Some(mut cpu) = str_cpu() else {
        return;
    };
    let (mut frames, mut words, mut escapes) = (0usize, 0usize, 0usize);
    for (name, step) in [
        ("MV1.STR", 37),
        ("MV2.STR", 1),
        ("MV3.STR", 23),
        ("MV4.STR", 11),
        ("MV5.STR", 29),
        ("MV6.STR", 31),
    ] {
        for (i, (w, h, bs)) in movie_frames(name).into_iter().enumerate() {
            if i % step != 0 {
                continue;
            }
            let port = psx_mdec::iki_mdec_codes(&bs).expect("an Iki frame");
            assert_eq!(u16::from_le_bytes([bs[4], bs[5]]), w);
            assert_eq!(u16::from_le_bytes([bs[6], bs[7]]), h);
            // The converter reads up to two halfwords past its last code.
            let mut frame = bs.clone();
            frame.extend_from_slice(&[0; 8]);
            cpu.mem.load(FRAME, &frame);
            cpu.mem.load(OUT, &vec![0xEE; port.len() * 2 + 0x40]);
            assert_eq!(
                cpu.call(CONVERT, &[FRAME, OUT]),
                Stop::Returned,
                "{name} #{i}"
            );
            let retail = cpu.mem.read(OUT, port.len() * 2 + 0x40);
            let mut port_bytes: Vec<u8> = port.iter().flat_map(|c| c.to_le_bytes()).collect();
            port_bytes.resize(retail.len(), 0xEE);
            if retail != port_bytes {
                let at = retail
                    .iter()
                    .zip(&port_bytes)
                    .position(|(a, b)| a != b)
                    .unwrap();
                panic!(
                    "{name} frame #{i}: first difference at code {} (retail {:#06x}, port {:#06x})",
                    at / 2,
                    u16::from_le_bytes([retail[at & !1], retail[at | 1]]),
                    u16::from_le_bytes([port_bytes[at & !1], port_bytes[at | 1]]),
                );
            }
            frames += 1;
            words += port.len();
            // A run past the VLC table's longest (31) only comes by escape.
            escapes += port
                .iter()
                .filter(|&&c| c != 0xFE00 && c >> 10 > 31)
                .count();
        }
    }
    assert!(
        frames > 400 && words > 1_000_000 && escapes > 0,
        "{frames} {words} {escapes}"
    );
}
