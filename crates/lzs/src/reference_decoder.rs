//! The straightforward LZS decoder, kept as the reference the production
//! decoder in `lib.rs` is compared against: it does what `FUN_8001A55C` does,
//! one output byte at a time through a real 4 KB ring.
//!
//! Test code only. `lib.rs` mounts it under `cfg(test)` for the synthetic
//! comparisons in `tests.rs`, and `legaia-disc-audit`'s
//! `lzs_decoder_reference_real` takes the same file by path for the comparison
//! over the disc's streams, so it stays out of every shipped build.

pub fn decode(input: &[u8], expected_output_size: usize) -> Result<(Vec<u8>, usize), String> {
    let mut window = [0u8; 0x1000];
    let mut window_pos: usize = 0xFEE;
    let reserve_hint = expected_output_size.min(input.len().saturating_mul(18).saturating_add(64));
    let mut out: Vec<u8> = Vec::with_capacity(reserve_hint);
    let mut src = 0usize;
    let mut control: u32 = 0;

    while out.len() < expected_output_size {
        if (control & 0x100) == 0 {
            if src >= input.len() {
                return Err(format!(
                    "EOF reading control byte at out={}/{}, src={}",
                    out.len(),
                    expected_output_size,
                    src
                ));
            }
            control = (input[src] as u32) | 0xFF00;
            src += 1;
        }

        if (control & 1) != 0 {
            if src >= input.len() {
                return Err(format!(
                    "EOF reading literal at out={}/{}",
                    out.len(),
                    expected_output_size
                ));
            }
            let v = input[src];
            src += 1;
            out.push(v);
            window[window_pos] = v;
            window_pos = (window_pos + 1) & 0xFFF;
        } else {
            if src + 2 > input.len() {
                return Err(format!(
                    "EOF reading back-ref at out={}/{}",
                    out.len(),
                    expected_output_size
                ));
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
