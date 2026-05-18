//! Legaia LZS decompressor.
//!
//! PORT: FUN_8001A55C
//!
//! Reverse-engineered from `FUN_8001a55c` in `SCUS_942.54`. The algorithm:
//!
//! - 4096-byte ring buffer initialized to zero, write position starts at 0xFEE.
//! - LSB-first 8-bit control byte; the high bit of the in-register control is a
//!   `0x100` sentinel signalling "byte exhausted, fetch the next one".
//! - Control bit = 1 → emit one literal byte from the source.
//! - Control bit = 0 → read two bytes (b0, b1):
//!     * absolute window position = `b0 | ((b1 & 0xF0) << 4)` (12 bits)
//!     * length = `(b1 & 0x0F) + 3`
//!     * copy `length` bytes out of the ring buffer starting at that position;
//!       each emitted byte is also stored at the current write position, which
//!       advances mod 4096.
//! - The decompressed output size is supplied externally - there is no length
//!   prefix or end-of-stream marker.
//!
//! `.lzs` *files* are containers: a small u32 header table where pairs at
//! offsets `[2k]`/`[2k+1]` give `(decompressed_size, byte_offset_to_stream)`
//! for each section. `decompress_container` parses that.

use anyhow::{Result, bail};

const WINDOW_SIZE: usize = 0x1000;
const WINDOW_START_POS: usize = 0xFEE;

pub fn decompress(input: &[u8], expected_output_size: usize) -> Result<Vec<u8>> {
    decompress_tracked(input, expected_output_size).map(|(o, _)| o)
}

/// Same as [`decompress`] but also returns the number of input bytes consumed.
/// Useful for validating container sections - a section that consumes more
/// input bytes than the gap to the next section's offset is mis-parsed.
pub fn decompress_tracked(input: &[u8], expected_output_size: usize) -> Result<(Vec<u8>, usize)> {
    let mut window = [0u8; WINDOW_SIZE];
    let mut window_pos: usize = WINDOW_START_POS;
    let mut out: Vec<u8> = Vec::with_capacity(expected_output_size);
    let mut src = 0usize;
    let mut control: u32 = 0;

    while out.len() < expected_output_size {
        if (control & 0x100) == 0 {
            if src >= input.len() {
                bail!(
                    "EOF reading control byte at out={}/{}, src={}",
                    out.len(),
                    expected_output_size,
                    src
                );
            }
            control = (input[src] as u32) | 0xFF00;
            src += 1;
        }

        if (control & 1) != 0 {
            if src >= input.len() {
                bail!(
                    "EOF reading literal at out={}/{}",
                    out.len(),
                    expected_output_size
                );
            }
            let v = input[src];
            src += 1;
            out.push(v);
            window[window_pos] = v;
            window_pos = (window_pos + 1) & 0xFFF;
        } else {
            if src + 2 > input.len() {
                bail!(
                    "EOF reading back-ref at out={}/{}",
                    out.len(),
                    expected_output_size
                );
            }
            let b0 = input[src] as u32;
            let b1 = input[src + 1] as u32;
            src += 2;
            let base = b0 | ((b1 & 0xF0) << 4);
            let len = ((b1 & 0x0F) + 3) as usize;
            for n in 0..len {
                let read_pos = ((base + n as u32) & 0xFFF) as usize;
                let v = window[read_pos];
                out.push(v);
                window[window_pos] = v;
                window_pos = (window_pos + 1) & 0xFFF;
                if out.len() >= expected_output_size {
                    break;
                }
            }
        }
        control >>= 1;
    }

    Ok((out, src))
}

#[derive(Debug)]
pub struct ContainerSection {
    pub size: u32,
    pub byte_offset: u32,
}

#[derive(Debug)]
pub struct Container {
    pub header_meta: [u32; 2],
    pub sections: Vec<ContainerSection>,
}

/// Parse a Legaia `.lzs` container by scanning the header table for plausible
/// `(size, offset)` pairs. The first two u32s are header metadata (purpose
/// undocumented); subsequent pairs are sections. We stop when a pair would
/// overlap the previous section or run off the file.
pub fn parse_container(file: &[u8]) -> Result<Container> {
    if file.len() < 16 {
        bail!("file too small ({}b) for an LZS container", file.len());
    }
    let header_meta = [
        u32::from_le_bytes(file[0..4].try_into().unwrap()),
        u32::from_le_bytes(file[4..8].try_into().unwrap()),
    ];

    let max_pairs = (file.len() / 8).min(64);
    let mut sections = Vec::new();
    let mut last_end: u32 = 0;
    for k in 1..max_pairs {
        let p = k * 8;
        if p + 8 > file.len() {
            break;
        }
        let size = u32::from_le_bytes(file[p..p + 4].try_into().unwrap()) & 0x00FF_FFFF;
        let off = u32::from_le_bytes(file[p + 4..p + 8].try_into().unwrap());
        if size == 0 || off == 0 {
            break;
        }
        if (off as usize) >= file.len() {
            break;
        }
        if off < last_end {
            break;
        }
        sections.push(ContainerSection {
            size,
            byte_offset: off,
        });
        last_end = off;
    }
    if sections.is_empty() {
        bail!("no plausible sections found in header table");
    }
    Ok(Container {
        header_meta,
        sections,
    })
}

pub fn decompress_container(file: &[u8]) -> Result<Vec<Vec<u8>>> {
    let c = parse_container(file)?;
    let mut out = Vec::with_capacity(c.sections.len());
    for s in &c.sections {
        let stream = &file[s.byte_offset as usize..];
        out.push(decompress(stream, s.size as usize)?);
    }
    Ok(out)
}

/// Decompress every section while validating that each section's input
/// consumption stays within its gap to the next section's offset. Returns
/// `Err` if any section overruns or fails to decode - i.e., the file
/// parsed as a container heuristically but isn't actually a real one.
///
/// Use this to avoid the false-positive trap where the loose
/// [`parse_container`] header heuristic matches a non-LZS file (e.g. a
/// flat u32 offset table) and the greedy decoder happily synthesises bytes
/// from whatever follows. Verified against PROT.DAT: this rejects ~16 false
/// positives that the lenient decoder accepts.
pub fn decompress_container_strict(file: &[u8]) -> Result<Vec<Vec<u8>>> {
    let c = parse_container(file)?;
    let mut out = Vec::with_capacity(c.sections.len());
    for (i, sec) in c.sections.iter().enumerate() {
        let start = sec.byte_offset as usize;
        if start >= file.len() {
            bail!("section {} starts past EOF", i);
        }
        let stream = &file[start..];
        let (decoded, consumed) = decompress_tracked(stream, sec.size as usize)?;
        // The next section's start is the upper bound on bytes we may consume.
        // For the last section, allow up to EOF.
        let upper = if i + 1 < c.sections.len() {
            c.sections[i + 1].byte_offset as usize
        } else {
            file.len()
        };
        let max_consume = upper.saturating_sub(start);
        if consumed > max_consume {
            bail!(
                "section {} consumed {} input bytes but only {} available before next section",
                i,
                consumed,
                max_consume
            );
        }
        out.push(decoded);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_zero_output() {
        let v = decompress(&[], 0).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn pure_literals() {
        // control byte 0xFF (8 literals), then 8 bytes
        let input = [0xFF, b'A', b'B', b'C', b'D', b'E', b'F', b'G', b'H'];
        let out = decompress(&input, 8).unwrap();
        assert_eq!(out, b"ABCDEFGH");
    }

    #[test]
    fn tracked_reports_input_consumption() {
        // 8 literals consume 1 control + 8 bytes = 9 input bytes.
        let input = [
            0xFF, b'A', b'B', b'C', b'D', b'E', b'F', b'G', b'H', 0xDE, 0xAD,
        ];
        let (out, consumed) = decompress_tracked(&input, 8).unwrap();
        assert_eq!(out, b"ABCDEFGH");
        assert_eq!(
            consumed, 9,
            "should not eat trailing bytes past the target output"
        );
    }

    #[test]
    fn strict_container_rejects_overrun() {
        // Build a fake container header where section 0 claims to decode
        // 100 bytes but only has 4 input bytes before section 1's offset.
        // The greedy decoder will read past - strict must reject.
        let mut file = Vec::new();
        // meta: [0, 0]
        file.extend_from_slice(&0u32.to_le_bytes());
        file.extend_from_slice(&0u32.to_le_bytes());
        // pair 1: size=100, off=24
        file.extend_from_slice(&100u32.to_le_bytes());
        file.extend_from_slice(&24u32.to_le_bytes());
        // pair 2: size=10, off=28  (only 4 bytes after section 0 starts)
        file.extend_from_slice(&10u32.to_le_bytes());
        file.extend_from_slice(&28u32.to_le_bytes());
        // padding to offset 24
        while file.len() < 24 {
            file.push(0);
        }
        // section 0 stream: 4 bytes that the greedy decoder happily synthesizes from
        file.extend_from_slice(&[0xFF, b'A', b'B', b'C']);
        // section 1 stream: more bytes after
        file.extend(std::iter::repeat_n(0, 200));
        // Lenient decoder accepts (greedy):
        assert!(decompress_container(&file).is_ok(), "lenient should accept");
        // Strict rejects because section 0 wants 100 bytes but only has 4 input bytes.
        assert!(
            decompress_container_strict(&file).is_err(),
            "strict must reject overrun"
        );
    }

    #[test]
    fn back_reference_reads_zeros_from_initial_window() {
        // control 0x00 -> 8 back-refs. First back-ref: b0=0xEE, b1=0xF0
        //   abs window pos = 0xEE | ((0xF0 & 0xF0) << 4) = 0xEE | 0xF00 = 0xFEE
        //   length = (0xF0 & 0xF) + 3 = 3
        // Reading from window[0xFEE..0xFF1] = three zero bytes.
        let input = [0x00, 0xEE, 0xF0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let out = decompress(&input, 3).unwrap();
        assert_eq!(out, &[0, 0, 0]);
    }
}
