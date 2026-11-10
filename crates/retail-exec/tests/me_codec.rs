//! The `"ME"` art-animation archive reader `FUN_8002B28C` and its
//! channel-delta codec `FUN_8002A9CC` against `me_archive::{parse,
//! decode_channel_delta}`.
//!
//! The reader runs over every archive at the head of a `0x10800`-byte slot of
//! `summon.dat` / `readef.DAT` (extraction 893 / 894), every entry, the
//! decoded keyframe stream compared whole. The codec then runs alone over
//! generated streams that reach every selector (12-bit literal, positive and
//! negative delta off the previous part, positive and negative nibble), at
//! part and frame counts past the shipped ones.

use crate::common::{Rng, disc, scus_cpu};
use legaia_actor_formats::me_archive;
use psx_retail_exec::{Stop, V0};

const READ: u32 = 0x8002_B28C;
const CODEC: u32 = 0x8002_A9CC;
const SRC: u32 = 0x8010_0000;
const DST: u32 = 0x8013_0000;
const SLOT: usize = 0x10800;

#[test]
fn the_archive_reader_matches_retail_over_the_disc() {
    differential!(scus(READ), scus(CODEC));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (mut archives, mut entries, mut fails) = (0, 0, Vec::new());
    for prot in [893usize, 894] {
        let bytes = d.read_entry(prot).expect("side-band entry");
        for slot in bytes.chunks(SLOT) {
            let Ok(arc) = me_archive::parse(slot) else {
                continue;
            };
            archives += 1;
            cpu.mem.load(SRC, slot);
            for n in 0..arc.len() {
                let Ok(port) = arc.entry(n) else { continue };
                cpu.mem.load(DST, &vec![0xEE; port.len() + 64]);
                assert_eq!(cpu.call(READ, &[SRC, DST, n as u32]), Stop::Returned);
                let retail = cpu.mem.read(DST, port.len());
                let past = cpu.mem.read(DST + port.len() as u32, 64);
                entries += 1;
                if (retail != port || past.iter().any(|b| *b != 0xEE)) && fails.len() < 6 {
                    let at = retail.iter().zip(&port).position(|(a, b)| a != b);
                    fails.push(format!("PROT {prot} entry {n}: first diff {at:?}"));
                }
            }
        }
    }
    // The codec on its own, once, so the declaration's entry is direct.
    let src = gen_stream(&mut Rng(1), 3, 4);
    cpu.mem.load(SRC, &src);
    assert_eq!(cpu.call(CODEC, &[DST, SRC]), Stop::Returned);
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(archives >= 6 && entries > 50, "{archives} {entries}");
}

/// A codec stream of `parts` x `frames` values with every selector drawn at
/// random, and room behind each stream for whatever the draws consume.
fn gen_stream(rng: &mut Rng, parts: usize, frames: usize) -> Vec<u8> {
    let values = parts * 6 * frames;
    let mut bits: Vec<u8> = Vec::new();
    for _ in 0..values {
        let sel: &[u8] = match rng.below(5) {
            0 => &[1],
            1 => &[0, 1],
            2 => &[0, 0, 1],
            3 => &[0, 0, 0, 1],
            _ => &[0, 0, 0, 0],
        };
        bits.extend_from_slice(sel);
    }
    let mut bit_bytes = vec![0u8; bits.len().div_ceil(8) + 4];
    for (i, b) in bits.iter().enumerate() {
        bit_bytes[i >> 3] |= b << (7 - (i & 7));
    }
    let nibs: Vec<u8> = (0..values / 2 + 4).map(|_| rng.next() as u8).collect();
    let mut bytes = vec![parts as u8, frames as u8];
    bytes.extend((0..values + 4).map(|_| rng.next() as u8));
    let mut src = vec![0x40 | (rng.below(0x40) as u8)];
    src.extend_from_slice(&(bit_bytes.len() as u16).to_le_bytes());
    src.extend_from_slice(&((bit_bytes.len() + nibs.len()) as u16).to_le_bytes());
    src.extend_from_slice(&bit_bytes);
    src.extend_from_slice(&nibs);
    src.extend_from_slice(&bytes);
    src
}

#[test]
fn the_codec_matches_retail_over_generated_streams() {
    differential!(scus(CODEC));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4D45_434F);
    let mut fails = Vec::new();
    for run in 0..3000 {
        // The scratch tables hold `parts * 6` channels twice; the shipped art
        // clips carry up to about twenty parts.
        let parts = 1 + rng.below(24) as usize;
        let frames = 1 + rng.below(40) as usize;
        let src = gen_stream(&mut rng, parts, frames);
        let port = me_archive::decode_channel_delta(&src).expect("generated stream decodes");
        cpu.mem.load(SRC, &src);
        cpu.mem.load(DST, &vec![0xEE; port.len() + 16]);
        assert_eq!(cpu.call(CODEC, &[DST, SRC]), Stop::Returned);
        let retail = cpu.mem.read(DST, port.len());
        let len = cpu.regs.r[V0] as usize;
        if (retail != port || len != port.len()) && fails.len() < 6 {
            let at = retail.iter().zip(&port).position(|(a, b)| a != b);
            fails.push(format!(
                "run {run} parts {parts} frames {frames}: first diff {at:?}, retail length {len} \
                 port {}",
                port.len()
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}
