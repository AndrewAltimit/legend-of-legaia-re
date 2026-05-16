// Brute-force scan every PROT entry for an LZS stream that decompresses to
// the title-overlay code. This variant uses an INLINE decoder that returns
// any successfully-decoded bytes even on EOF (so partial decompressions are
// still searchable for the fingerprint).

use std::env;
use std::fs;
use std::path::PathBuf;

const WINDOW_SIZE: usize = 0x1000;
const WINDOW_START_POS: usize = 0xFEE;

/// Decode LZS, returning whatever output was produced before any error.
fn decompress_lossy(input: &[u8], max_output: usize) -> Vec<u8> {
    let mut window = [0u8; WINDOW_SIZE];
    let mut window_pos: usize = WINDOW_START_POS;
    let mut out: Vec<u8> = Vec::with_capacity(max_output.min(64 * 1024));
    let mut src = 0usize;
    let mut control: u32 = 0;
    let mut steps = 0usize;

    while out.len() < max_output {
        steps += 1;
        // Safety: cap on iterations to avoid infinite loops on malformed input.
        if steps > 4_000_000 {
            break;
        }
        if (control & 0x100) == 0 {
            if src >= input.len() {
                break;
            }
            control = (input[src] as u32) | 0xFF00;
            src += 1;
        }
        if (control & 1) != 0 {
            if src >= input.len() {
                break;
            }
            let v = input[src];
            src += 1;
            out.push(v);
            window[window_pos] = v;
            window_pos = (window_pos + 1) & 0xFFF;
        } else {
            if src + 2 > input.len() {
                break;
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
                if out.len() >= max_output {
                    break;
                }
            }
        }
        control >>= 1;
    }
    out
}

fn main() {
    let mut args = env::args().skip(1);
    let overlay_path = args.next().expect("usage: <overlay> <prot_dir>");
    let prot_dir = args.next().expect("usage: <overlay> <prot_dir>");
    let overlay = fs::read(&overlay_path).expect("read overlay");

    // Multi-fingerprint search: use 3 distinct unique-within-overlay slices.
    let fps: Vec<(usize, &[u8])> = vec![
        (0x1D35C, &overlay[0x1D35C..0x1D35C + 32]), // title-tick body
        (0x1B000, &overlay[0x1B000..0x1B000 + 32]), // mid-tick code
        (0x18000, &overlay[0x18000..0x18000 + 32]), // misc code
    ];
    for (off, fp) in &fps {
        eprintln!("FP@0x{:x}: {}", off, hex(fp));
    }

    let mut entries: Vec<PathBuf> = fs::read_dir(&prot_dir)
        .expect("read prot dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("BIN"))
        .collect();
    entries.sort();
    eprintln!("Scanning {} PROT entries (lossy decoder)\n", entries.len());

    let max_out = 384 * 1024;
    let mut total_hits = 0;

    for path in &entries {
        let raw = match fs::read(path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if raw.len() < 32 {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        // Try raw LZS at every 4-byte boundary in the first 4 KiB of the file.
        // This is the densest reasonable sweep without exploding runtime.
        let mut found = false;
        for skip in (0..raw.len().min(0x1000)).step_by(4) {
            if skip + 32 >= raw.len() {
                break;
            }
            let stream = &raw[skip..];
            let out = decompress_lossy(stream, max_out);
            if out.len() < 64 {
                continue;
            }
            for (fp_off, fp) in &fps {
                if let Some(pos) = find_subslice(&out, fp) {
                    println!(
                        "HIT {} skip=0x{:x} decoded={}b fp@0x{:x} match_at=0x{:x}",
                        name,
                        skip,
                        out.len(),
                        fp_off,
                        pos
                    );
                    total_hits += 1;
                    found = true;
                    break;
                }
            }
            if found {
                break;
            }
        }
    }
    eprintln!("\nDone. Total hits: {}", total_hits);
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    let n = needle.len();
    for i in 0..=haystack.len() - n {
        if &haystack[i..i + n] == needle {
            return Some(i);
        }
    }
    None
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}
