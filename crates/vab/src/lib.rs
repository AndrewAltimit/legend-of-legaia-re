//! Sony VAB instrument-bank parser + VAG sample extractor.
//!
//! Layout (Sony PsyQ docs, version 7):
//!
//! ```text
//! 0x00 u32  magic   = 'pBAV'  (0x70424156 LE)
//! 0x04 u32  version (typically 7)
//! 0x08 u32  vab_id
//! 0x0C u32  fsize           total bank size in bytes
//! 0x10 u16  reserved
//! 0x12 u16 ps               number of programs in use
//! 0x14 u16 ts               total number of tones in use
//! 0x16 u16 vs               number of VAG samples
//! 0x18 u8   mvol            master volume
//! 0x19 u8   pan
//! 0x1A u8   attr1
//! 0x1B u8   attr2
//! 0x1C u32  reserved
//!
//! 0x20         ProgAtr[128]   16 bytes each = 2048 bytes (regardless of `ps`)
//! 0x820        VagAtr[16][ps] 32 bytes each, 16 tones per program slot
//!              -> tones section size = 512 * ps
//! +(2048+512*ps) u16 vag_table[256]
//!                first entry is master shift (often 0 in v7)
//!                entries 1..=vs hold cumulative VAG sizes / 8 (8-byte units)
//! + 0x200 (after table) VAG bodies (raw SPU ADPCM, 16-byte blocks)
//! ```
//!
//! Entries past `vs` in `vag_table` are zero. The decoder treats `vag_table[i+1]`
//! (in 8-byte units) as the *size* of sample `i`; samples are concatenated
//! immediately after the table. PSX SDK treats entry 0 as the master shift
//! / pitch correction; we preserve it but don't apply it.
//!
//! VAG body = stream of 16-byte SPU ADPCM blocks:
//! ```text
//! byte 0: (filter << 4) | shift   (filter in 0..=4)
//! byte 1: flag                    (1 = loop end+jump, 2 = loop sustain, 4 = loop start)
//! bytes 2..16: 14 nibble pairs, low nibble first = 28 4-bit samples
//! ```
//!
//! Shares the F0/F1 filter constants with [`legaia_xa`] — the algorithm is
//! identical to XA-ADPCM, only the block packaging differs.

use anyhow::{Result, bail};
use serde::Serialize;

/// On-disk bytes are `p B A V` (`0x70 0x42 0x41 0x56`); read as a little-endian
/// u32 that's `0x56414270`. The PsyQ macro `VABp` makes the byte order obvious.
pub const VAB_MAGIC: u32 = 0x5641_4270;
pub const VAB_HEADER_SIZE: usize = 0x20;
pub const PROGRAMS_TABLE_SIZE: usize = 16 * 128; // 2048
pub const TONE_SIZE: usize = 32;
pub const TONES_PER_PROGRAM: usize = 16;
pub const VAG_TABLE_ENTRIES: usize = 256;
pub const VAG_BLOCK_BYTES: usize = 16;
pub const SAMPLES_PER_BLOCK: usize = 28;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct VabHeader {
    pub magic: u32,
    pub version: u32,
    pub vab_id: u32,
    pub fsize: u32,
    pub ps: u16,
    pub ts: u16,
    pub vs: u16,
    pub mvol: u8,
    pub pan: u8,
    pub attr1: u8,
    pub attr2: u8,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ProgAtr {
    pub tones: u8, // number of tones used in this program
    pub mvol: u8,
    pub prior: u8,
    pub mode: u8,
    pub mpan: u8,
    pub reserved0: u8,
    pub attr: u16,
    pub reserved1: u32,
    pub reserved2: u32,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct VagAtr {
    pub prior: u8,
    pub mode: u8,
    pub vol: u8,
    pub pan: u8,
    pub center: u8,
    pub shift: u8,
    pub min: u8,
    pub max: u8,
    pub vibw: u8,
    pub vibt: u8,
    pub porw: u8,
    pub port: u8,
    pub pbmin: u8,
    pub pbmax: u8,
    pub reserved1: u8,
    pub reserved2: u8,
    pub adsr1: u16,
    pub adsr2: u16,
    pub prog: i16,
    pub vag: i16,
    pub reserved3: [u16; 4],
}

#[derive(Debug, Clone, Serialize)]
pub struct VabReport {
    pub header: VabHeader,
    pub header_offset: usize,
    pub programs: Vec<ProgAtr>,
    pub tones: Vec<Vec<VagAtr>>,
    /// Byte offset (within input buffer) + size of each VAG sample body.
    pub vag_samples: Vec<VagSampleSpan>,
    pub master_shift: u16,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct VagSampleSpan {
    pub index: usize,
    pub byte_offset: usize,
    pub size: usize,
}

/// Parse a VAB header at `offset` in `buf`. Returns `Err` if magic mismatches
/// or fields are out of range.
pub fn parse_header(buf: &[u8], offset: usize) -> Result<VabHeader> {
    let h = buf
        .get(offset..offset + VAB_HEADER_SIZE)
        .ok_or_else(|| anyhow::anyhow!("offset 0x{:X} + header size past buffer end", offset))?;
    let magic = u32::from_le_bytes(h[0..4].try_into().unwrap());
    if magic != VAB_MAGIC {
        bail!(
            "magic mismatch at 0x{:X}: got 0x{:08X}, expected 0x{:08X} ('pBAV')",
            offset,
            magic,
            VAB_MAGIC
        );
    }
    let version = u32::from_le_bytes(h[4..8].try_into().unwrap());
    let vab_id = u32::from_le_bytes(h[8..12].try_into().unwrap());
    let fsize = u32::from_le_bytes(h[12..16].try_into().unwrap());
    let ps = u16::from_le_bytes(h[18..20].try_into().unwrap());
    let ts = u16::from_le_bytes(h[20..22].try_into().unwrap());
    let vs = u16::from_le_bytes(h[22..24].try_into().unwrap());
    let mvol = h[24];
    let pan = h[25];
    let attr1 = h[26];
    let attr2 = h[27];

    if version > 10 {
        bail!("implausible VAB version {}", version);
    }
    if ps == 0 || ps > 128 {
        bail!("VAB programs count out of range: {}", ps);
    }
    if vs == 0 || vs as usize > VAG_TABLE_ENTRIES {
        bail!("VAB samples count out of range: {}", vs);
    }
    Ok(VabHeader {
        magic,
        version,
        vab_id,
        fsize,
        ps,
        ts,
        vs,
        mvol,
        pan,
        attr1,
        attr2,
    })
}

/// Parse a complete VAB at `offset`: header, programs, tones, VAG sample table.
pub fn parse(buf: &[u8], offset: usize) -> Result<VabReport> {
    let header = parse_header(buf, offset)?;
    let ps = header.ps as usize;
    let vs = header.vs as usize;

    let prog_off = offset + VAB_HEADER_SIZE;
    let tone_off = prog_off + PROGRAMS_TABLE_SIZE;
    let table_off = tone_off + TONE_SIZE * TONES_PER_PROGRAM * ps;
    let vag_bodies_off = table_off + 2 * VAG_TABLE_ENTRIES;

    if offset + header.fsize as usize > buf.len() {
        bail!(
            "VAB at 0x{:X} claims fsize {} but only {} bytes remain",
            offset,
            header.fsize,
            buf.len() - offset
        );
    }

    // Programs: 128 fixed slots even though only `ps` are in use.
    let mut programs = Vec::with_capacity(128);
    for i in 0..128 {
        let p = prog_off + i * 16;
        let s = &buf[p..p + 16];
        programs.push(ProgAtr {
            tones: s[0],
            mvol: s[1],
            prior: s[2],
            mode: s[3],
            mpan: s[4],
            reserved0: s[5],
            attr: u16::from_le_bytes(s[6..8].try_into().unwrap()),
            reserved1: u32::from_le_bytes(s[8..12].try_into().unwrap()),
            reserved2: u32::from_le_bytes(s[12..16].try_into().unwrap()),
        });
    }

    // Tones: 16 per program × ps programs.
    let mut tones = Vec::with_capacity(ps);
    for prog_idx in 0..ps {
        let mut row = Vec::with_capacity(TONES_PER_PROGRAM);
        for tone_idx in 0..TONES_PER_PROGRAM {
            let p = tone_off + (prog_idx * TONES_PER_PROGRAM + tone_idx) * TONE_SIZE;
            let s = &buf[p..p + TONE_SIZE];
            row.push(VagAtr {
                prior: s[0],
                mode: s[1],
                vol: s[2],
                pan: s[3],
                center: s[4],
                shift: s[5],
                min: s[6],
                max: s[7],
                vibw: s[8],
                vibt: s[9],
                porw: s[10],
                port: s[11],
                pbmin: s[12],
                pbmax: s[13],
                reserved1: s[14],
                reserved2: s[15],
                adsr1: u16::from_le_bytes(s[16..18].try_into().unwrap()),
                adsr2: u16::from_le_bytes(s[18..20].try_into().unwrap()),
                prog: i16::from_le_bytes(s[20..22].try_into().unwrap()),
                vag: i16::from_le_bytes(s[22..24].try_into().unwrap()),
                reserved3: [
                    u16::from_le_bytes(s[24..26].try_into().unwrap()),
                    u16::from_le_bytes(s[26..28].try_into().unwrap()),
                    u16::from_le_bytes(s[28..30].try_into().unwrap()),
                    u16::from_le_bytes(s[30..32].try_into().unwrap()),
                ],
            });
        }
        tones.push(row);
    }

    // VAG offset table: 256 u16 entries; entry 0 is master shift, entries 1..=vs
    // are sample sizes in 8-byte units. Sample bodies start at vag_bodies_off.
    let table = &buf[table_off..table_off + 2 * VAG_TABLE_ENTRIES];
    let entries: Vec<u16> = (0..VAG_TABLE_ENTRIES)
        .map(|i| u16::from_le_bytes(table[i * 2..i * 2 + 2].try_into().unwrap()))
        .collect();
    let master_shift = entries[0];

    let mut samples = Vec::with_capacity(vs);
    let mut cursor = vag_bodies_off;
    for i in 0..vs {
        let size_units = entries[i + 1] as usize;
        let size = size_units * 8;
        if cursor + size > offset + header.fsize as usize {
            bail!(
                "VAG sample {} (size {}) overruns VAB region (vab end = 0x{:X}, sample end = 0x{:X})",
                i,
                size,
                offset + header.fsize as usize,
                cursor + size
            );
        }
        samples.push(VagSampleSpan {
            index: i,
            byte_offset: cursor,
            size,
        });
        cursor += size;
    }

    Ok(VabReport {
        header,
        header_offset: offset,
        programs,
        tones,
        vag_samples: samples,
        master_shift,
    })
}

/// Find every standalone VAB header in `buf`. Walks for the magic byte
/// pattern and validates each candidate by trying [`parse_header`].
pub fn find_vabs(buf: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let pattern = VAB_MAGIC.to_le_bytes();
    let mut i = 0;
    while i + 4 <= buf.len() {
        if buf[i..i + 4] == pattern && parse_header(buf, i).is_ok() {
            out.push(i);
            i += VAB_HEADER_SIZE;
        } else {
            i += 1;
        }
    }
    out
}

/// Decode one VAG sample body (a stream of 16-byte SPU ADPCM blocks) into
/// signed 16-bit PCM. Stops at end-of-buffer or at a block whose flag byte
/// has the loop-end bit (bit 0) set.
pub fn decode_vag(buf: &[u8]) -> Result<Vec<i16>> {
    if !buf.len().is_multiple_of(VAG_BLOCK_BYTES) {
        bail!(
            "VAG body length {} is not a multiple of {} (block size)",
            buf.len(),
            VAG_BLOCK_BYTES
        );
    }
    let n_blocks = buf.len() / VAG_BLOCK_BYTES;
    let mut out: Vec<i16> = Vec::with_capacity(n_blocks * SAMPLES_PER_BLOCK);
    let mut prev1: i32 = 0;
    let mut prev2: i32 = 0;

    for b in 0..n_blocks {
        let block = &buf[b * VAG_BLOCK_BYTES..(b + 1) * VAG_BLOCK_BYTES];
        let header_byte = block[0];
        let filter = ((header_byte >> 4) & 0x0F) as usize;
        let shift = (header_byte & 0x0F) as i32;
        let flag = block[1];

        // End-of-stream sentinel: many Legaia VAB samples mark end with
        // `flag & 0x01` AND fill the header byte with garbage (filter > 4).
        // Treat both signals as "stop here, don't decode the sentinel".
        if flag & 0x01 != 0 || filter > 4 {
            break;
        }

        let f0 = legaia_xa::F0[filter];
        let f1 = legaia_xa::F1[filter];

        // Decode 14 bytes = 28 nibble samples (low nibble first).
        for &byte in &block[2..16] {
            for nibble_idx in 0..2 {
                let nibble = if nibble_idx == 0 {
                    byte & 0x0F
                } else {
                    (byte >> 4) & 0x0F
                };
                // Sign-extend 4-bit signed nibble.
                let s = ((nibble as i8) << 4) >> 4;
                let mut sample = (s as i32) << (12 - shift);
                sample += (prev1 * f0 + prev2 * f1 + 32) >> 6;
                let clamped = sample.clamp(i16::MIN as i32, i16::MAX as i32);
                out.push(clamped as i16);
                prev2 = prev1;
                prev1 = clamped;
            }
        }
    }
    Ok(out)
}

/// Write samples as a mono 16-bit PCM WAV.
pub fn write_wav<W: std::io::Write>(
    mut w: W,
    samples: &[i16],
    sample_rate: u32,
) -> std::io::Result<()> {
    let n_bytes = (samples.len() * 2) as u32;
    let total_size = 36 + n_bytes;
    w.write_all(b"RIFF")?;
    w.write_all(&total_size.to_le_bytes())?;
    w.write_all(b"WAVE")?;
    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?; // fmt chunk size
    w.write_all(&1u16.to_le_bytes())?; // PCM
    w.write_all(&1u16.to_le_bytes())?; // 1 channel
    w.write_all(&sample_rate.to_le_bytes())?;
    let byte_rate = sample_rate * 2;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&2u16.to_le_bytes())?; // block align
    w.write_all(&16u16.to_le_bytes())?; // bits per sample
    w.write_all(b"data")?;
    w.write_all(&n_bytes.to_le_bytes())?;
    for s in samples {
        w.write_all(&s.to_le_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_vab(ps: u16, vs: u16, vag_sizes_8b: &[u16]) -> Vec<u8> {
        assert_eq!(vag_sizes_8b.len(), vs as usize);
        let total_vag = vag_sizes_8b.iter().map(|&s| s as usize * 8).sum::<usize>();
        let header_size = VAB_HEADER_SIZE
            + PROGRAMS_TABLE_SIZE
            + TONE_SIZE * TONES_PER_PROGRAM * ps as usize
            + 2 * VAG_TABLE_ENTRIES;
        let fsize = header_size + total_vag;
        let mut buf = vec![0u8; fsize];

        // Header
        buf[0..4].copy_from_slice(&VAB_MAGIC.to_le_bytes());
        buf[4..8].copy_from_slice(&7u32.to_le_bytes());
        buf[12..16].copy_from_slice(&(fsize as u32).to_le_bytes());
        buf[18..20].copy_from_slice(&ps.to_le_bytes());
        buf[20..22].copy_from_slice(&0u16.to_le_bytes()); // ts
        buf[22..24].copy_from_slice(&vs.to_le_bytes());

        // VAG offset table
        let table_off =
            VAB_HEADER_SIZE + PROGRAMS_TABLE_SIZE + TONE_SIZE * TONES_PER_PROGRAM * ps as usize;
        for (i, &sz) in vag_sizes_8b.iter().enumerate() {
            let p = table_off + (i + 1) * 2;
            buf[p..p + 2].copy_from_slice(&sz.to_le_bytes());
        }
        buf
    }

    #[test]
    fn parse_synthetic() {
        let buf = synthetic_vab(2, 3, &[10, 20, 5]);
        let report = parse(&buf, 0).unwrap();
        assert_eq!(report.header.ps, 2);
        assert_eq!(report.header.vs, 3);
        assert_eq!(report.vag_samples.len(), 3);
        assert_eq!(report.vag_samples[0].size, 80);
        assert_eq!(report.vag_samples[1].size, 160);
        assert_eq!(report.vag_samples[2].size, 40);
        // sample[0] starts right after the table
        let expected = VAB_HEADER_SIZE
            + PROGRAMS_TABLE_SIZE
            + TONE_SIZE * TONES_PER_PROGRAM * 2
            + 2 * VAG_TABLE_ENTRIES;
        assert_eq!(report.vag_samples[0].byte_offset, expected);
    }

    #[test]
    fn find_in_padded_buffer() {
        let vab = synthetic_vab(2, 1, &[1]);
        let mut padded = vec![0xCDu8; 100];
        padded.extend_from_slice(&vab);
        padded.extend_from_slice(&[0xCDu8; 200]);
        let hits = find_vabs(&padded);
        assert_eq!(hits, vec![100]);
    }

    #[test]
    fn header_rejects_bad_magic() {
        let mut buf = vec![0u8; 0x100];
        buf[0..4].copy_from_slice(b"FAIL");
        assert!(parse_header(&buf, 0).is_err());
    }

    #[test]
    fn decode_vag_silence_block_yields_zeros() {
        // One block: filter=0 shift=0, all-zero samples, no end flag.
        // Loop runs to natural end of buffer.
        let block = vec![0u8; VAG_BLOCK_BYTES];
        let pcm = decode_vag(&block).unwrap();
        assert_eq!(pcm.len(), SAMPLES_PER_BLOCK);
        assert!(pcm.iter().all(|&s| s == 0));
    }

    #[test]
    fn decode_vag_treats_bad_filter_as_eos() {
        // First block: filter=0 silence; second block: filter=7 garbage =>
        // decoder should stop after block 0 rather than erroring.
        let mut buf = vec![0u8; VAG_BLOCK_BYTES * 2];
        buf[VAG_BLOCK_BYTES] = 0x70; // filter=7 in second block
        let pcm = decode_vag(&buf).unwrap();
        assert_eq!(pcm.len(), SAMPLES_PER_BLOCK);
    }

    #[test]
    fn decode_vag_honors_end_flag() {
        // Block 0 with end-flag should NOT be decoded (Legaia convention:
        // end-marker block carries no playable data, often garbage filter).
        let mut block = vec![0u8; VAG_BLOCK_BYTES];
        block[0] = 0xF0; // filter=15 -- garbage
        block[1] = 0x01; // end flag
        let pcm = decode_vag(&block).unwrap();
        assert!(pcm.is_empty());
    }
}
