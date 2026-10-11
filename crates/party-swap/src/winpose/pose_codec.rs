//! The pose math and wire encoding the win-pose rebuild shares: 3x3 products,
//! Euler extraction, part pack / unpack, and the channel delta encoder.

use super::super::*;

pub(crate) fn mmul(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut m = [[0.0f32; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

pub(crate) fn transpose(a: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut m = [[0.0f32; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = a[j][i];
        }
    }
    m
}

/// Extract PSX euler angles (1/4096 turns, `R = Rz * Ry * Rx`) from a
/// rotation matrix - the inverse of `rot_matrix`. Near gimbal lock
/// (`|cos y| ~ 0`) the x/z split is degenerate; z folds to 0 there.
pub(crate) fn to_euler(m: &[[f32; 3]; 3]) -> (u16, u16, u16) {
    let sy = (-m[2][0]).clamp(-1.0, 1.0);
    let y = sy.asin();
    let (x, z) = if y.cos().abs() > 1e-4 {
        (m[2][1].atan2(m[2][2]), m[1][0].atan2(m[0][0]))
    } else {
        ((-m[1][2]).atan2(m[1][1]), 0.0)
    };
    let unit = |a: f32| -> u16 {
        let u = (a / std::f32::consts::TAU * 4096.0).round() as i32;
        (u.rem_euclid(4096)) as u16
    };
    (unit(x), unit(y), unit(z))
}

/// Pack one part pose into the 9-byte stream record (six 12-bit fields;
/// the `FUN_8004998C` unpack layout in reverse).
pub(crate) fn pack_part(p: &PartPose) -> [u8; 9] {
    let f = [
        (p.tx as u16) & 0xFFF,
        (p.ty as u16) & 0xFFF,
        (p.tz as u16) & 0xFFF,
        p.rx & 0xFFF,
        p.ry & 0xFFF,
        p.rz & 0xFFF,
    ];
    let mut out = [0u8; 9];
    for pair in 0..3 {
        let (a, b) = (f[pair * 2], f[pair * 2 + 1]);
        out[pair * 3] = a as u8;
        out[pair * 3 + 1] = b as u8;
        out[pair * 3 + 2] = ((a >> 8) as u8 & 0x0F) | ((b >> 4) as u8 & 0xF0);
    }
    out
}

/// Inverse of [`pack_part`]: one 9-byte record to a part pose
/// (12-bit fields, translations sign-extended).
pub(crate) fn unpack_part(b: &[u8]) -> PartPose {
    let mut f = [0u16; 6];
    for pair in 0..3 {
        let (lo, hi, ext) = (
            b[pair * 3] as u16,
            b[pair * 3 + 1] as u16,
            b[pair * 3 + 2] as u16,
        );
        f[pair * 2] = lo | ((ext & 0x0F) << 8);
        f[pair * 2 + 1] = hi | ((ext & 0xF0) << 4);
    }
    let sx = |v: u16| -> i16 {
        if v & 0x800 != 0 {
            (v | 0xF000) as i16
        } else {
            v as i16
        }
    };
    PartPose {
        tx: sx(f[0]),
        ty: sx(f[1]),
        tz: sx(f[2]),
        rx: f[3],
        ry: f[4],
        rz: f[5],
    }
}

/// Encode a decoded stream (`[parts][frames][9-byte records]`) with the
/// retail channel-delta codec - the exact inverse of
/// [`me_archive::decode_channel_delta`], choosing the cheapest selector
/// arm per value. Round-trips bit-exact through the decoder.
pub fn encode_channel_delta(decoded: &[u8]) -> Result<Vec<u8>> {
    if decoded.len() < 2 {
        bail!("stream shorter than its parts/frames head");
    }
    let parts = decoded[0] as usize;
    let frames = decoded[1] as usize;
    let channels = parts * 6;
    if decoded.len() < 2 + parts * frames * 9 {
        bail!("stream shorter than its frame data");
    }

    // Target 12-bit channel values per frame, from the packed records.
    let mut target: Vec<Vec<u16>> = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut row = Vec::with_capacity(channels);
        for p in 0..parts {
            let o = 2 + (f * parts + p) * 9;
            let b = &decoded[o..o + 9];
            for pair in 0..3 {
                let a = b[pair * 3] as u16 | ((b[pair * 3 + 2] as u16 & 0x0F) << 8);
                let v = b[pair * 3 + 1] as u16 | ((b[pair * 3 + 2] as u16 & 0xF0) << 4);
                row.push(a);
                row.push(v);
            }
        }
        target.push(row);
    }

    let mut bits: Vec<u8> = Vec::new(); // one selector bit per push
    let mut nibs: Vec<u8> = Vec::new(); // 4-bit operands
    let mut bytes: Vec<u8> = vec![parts as u8, frames as u8];

    let mut delta = vec![0u16; channels];
    let mut acc = vec![0u16; channels];
    for (f, row) in target.iter().enumerate() {
        for c in 0..channels {
            // The delta value the decoder must produce for this slot.
            let v = if f == 0 {
                if c < 6 {
                    row[c]
                } else {
                    row[c].wrapping_sub(row[c - 6]) & 0xFFF
                }
            } else {
                row[c].wrapping_sub(acc[c]) & 0xFFF
            };
            let prev = delta[if c < 6 { c + channels - 6 } else { c - 6 }];
            let dp = v.wrapping_sub(prev) & 0xFFF;
            if dp < 0x10 {
                // `01`: previous-part delta + nibble.
                bits.extend_from_slice(&[0, 1]);
                nibs.push(dp as u8);
            } else if dp >= 0xFF0 {
                // `001`: previous-part delta + negative nibble.
                bits.extend_from_slice(&[0, 0, 1]);
                nibs.push((dp & 0xF) as u8);
            } else if v < 0x10 {
                // `0001`: literal nibble.
                bits.extend_from_slice(&[0, 0, 0, 1]);
                nibs.push(v as u8);
            } else if v >= 0xFF0 {
                // `0000`: literal negative nibble.
                bits.extend_from_slice(&[0, 0, 0, 0]);
                nibs.push((v & 0xF) as u8);
            } else {
                // `1`: 12-bit literal (nibble high, byte low).
                bits.push(1);
                nibs.push((v >> 8) as u8);
                bytes.push(v as u8);
            }
            delta[c] = v;
        }
        if f == 0 {
            acc[..6].copy_from_slice(&delta[..6]);
            for c in 6..channels {
                acc[c] = delta[c].wrapping_add(acc[c - 6]) & 0xFFF;
            }
        } else {
            for c in 0..channels {
                acc[c] = acc[c].wrapping_add(delta[c]) & 0xFFF;
            }
        }
    }

    let mut bit_bytes = vec![0u8; bits.len().div_ceil(8)];
    for (i, b) in bits.iter().enumerate() {
        bit_bytes[i >> 3] |= b << (7 - (i & 7));
    }
    let mut nib_bytes = vec![0u8; nibs.len().div_ceil(2)];
    for (i, n) in nibs.iter().enumerate() {
        nib_bytes[i >> 1] |= if i & 1 == 0 { n << 4 } else { *n };
    }
    let nibble_off = bit_bytes.len();
    let byte_off = nibble_off + nib_bytes.len();
    if nibble_off > u16::MAX as usize || byte_off > u16::MAX as usize {
        bail!("codec streams exceed the 16-bit header offsets");
    }
    let mut out = Vec::with_capacity(5 + byte_off + bytes.len());
    out.push(0x40);
    out.extend_from_slice(&(nibble_off as u16).to_le_bytes());
    out.extend_from_slice(&(byte_off as u16).to_le_bytes());
    out.extend_from_slice(&bit_bytes);
    out.extend_from_slice(&nib_bytes);
    out.extend_from_slice(&bytes);
    Ok(out)
}
