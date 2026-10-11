//! Per-packet decode: validate one tagged GP0 packet against its opcode's
//! expected length and lift the vertex / UV / colour fields.

use super::Prim;

pub(super) fn decode_in(pool: &[u8], pool_base: u32, _scratch: &mut Vec<u8>) -> Vec<Prim> {
    let pool_lo = pool_base & 0x00FF_FFFF;
    let pool_hi = pool_lo + pool.len() as u32;
    let mut consumed = vec![false; pool.len() / 4];
    let mut out = Vec::new();
    let n_words = pool.len() / 4;
    for w in 0..n_words {
        if consumed[w] {
            continue;
        }
        let i = w * 4;
        if i + 8 > pool.len() {
            break;
        }
        let tag = read_u32(pool, i);
        let length = ((tag >> 24) & 0xFF) as usize;
        let next_addr = tag & 0x00FF_FFFF;
        if !(1..=12).contains(&length) {
            continue;
        }
        if next_addr != 0x00FF_FFFF && !(pool_lo..pool_hi).contains(&next_addr) {
            continue;
        }
        // Payload starts at `i + 4`. Need at least `length` payload words.
        let payload_end = i + 4 + length * 4;
        if payload_end > pool.len() {
            continue;
        }
        // Read cmd byte (high byte of payload word 0).
        let cmd_word = read_u32(pool, i + 4);
        let cmd = ((cmd_word >> 24) & 0xFF) as u8;
        let (kind_ok, prim) = decode_packet(pool, i, cmd, length);
        if !kind_ok {
            continue;
        }
        if let Some(p) = prim {
            // Mark the tag + payload words as consumed so an inner false-positive
            // tag (e.g. a vertex word that looks like a chain link) can't emit
            // a phantom prim.
            for k in 0..=length {
                let cw = w + k;
                if cw < consumed.len() {
                    consumed[cw] = true;
                }
            }
            out.push(p);
        }
    }
    out
}

/// Try to decode the packet starting at offset `i` in `pool` as the given
/// `cmd` with payload `length` words. Returns `(kind_matches, prim)`:
/// `kind_matches=false` ⇒ the cmd byte didn't match any known opcode (skip
/// without consuming); `kind_matches=true` with `Some(prim)` ⇒ accepted;
/// `kind_matches=true` with `None` ⇒ matched cmd but length disagreed
/// (also skip).
pub(super) fn decode_packet(pool: &[u8], i: usize, cmd: u8, length: usize) -> (bool, Option<Prim>) {
    match cmd {
        // POLY_FT4: 9 payload words.
        0x2C..=0x2F if length == 9 => {
            let (color, verts, uvs, clut, tpage) = decode_ft4(pool, i);
            (
                true,
                Some(Prim::PolyFt4 {
                    cmd,
                    color,
                    verts,
                    uvs,
                    clut,
                    tpage,
                }),
            )
        }
        // POLY_GT4: 12 payload words.
        0x3C..=0x3F if length == 12 => {
            let (colors, verts, uvs, clut, tpage) = decode_gt4(pool, i);
            (
                true,
                Some(Prim::PolyGt4 {
                    cmd,
                    colors,
                    verts,
                    uvs,
                    clut,
                    tpage,
                }),
            )
        }
        // POLY_FT3: 7 payload words - colour+cmd, then three `xy` / `uv`
        // pairs (`uv0 | clut`, `uv1 | tpage`, `uv2`), which is what
        // `decode_ft3` reads and what the retail emitters tag (`lui t8,0x700`
        // in `FUN_80044C14`, the field FT3 leaf). A `6` here dropped every
        // FT3 from every decoded display list, so a family with triangles
        // read as "retail draws none of them".
        0x24..=0x27 if length == 7 => {
            let (color, verts, uvs, clut, tpage) = decode_ft3(pool, i);
            (
                true,
                Some(Prim::PolyFt3 {
                    cmd,
                    color,
                    verts,
                    uvs,
                    clut,
                    tpage,
                }),
            )
        }
        // POLY_GT3: 9 payload words.
        0x34..=0x37 if length == 9 => {
            let (colors, verts, uvs, clut, tpage) = decode_gt3(pool, i);
            (
                true,
                Some(Prim::PolyGt3 {
                    cmd,
                    colors,
                    verts,
                    uvs,
                    clut,
                    tpage,
                }),
            )
        }
        // GP0 rectangle commands encode size in bits 4-3 of the opcode:
        // `00`=variable, `01`=1x1, `10`=8x8, `11`=16x16, with bit 2 = textured.
        // So 0x74..0x77 is the **8x8** textured sprite and 0x7C..0x7F the
        // **16x16** one - the reverse of an earlier reading here, which made
        // every 8x8 sprite decode as `Sprt16` and render at double size in the
        // web-viewer's `sprite_to_quad` call.
        //
        // SPRT_8 (fixed 8x8): 3 payload words.
        0x74..=0x77 if length == 3 => {
            let (color, pos, uv, clut) = decode_sprt(pool, i);
            (
                true,
                Some(Prim::Sprt8 {
                    cmd,
                    color,
                    pos,
                    uv,
                    clut,
                }),
            )
        }
        // SPRT (free size): 4 payload words - the fixed-size layout plus
        // `w | h<<16`. Retail's window chrome is built entirely from these.
        0x64..=0x67 if length == 4 => {
            let (color, pos, uv, clut) = decode_sprt(pool, i);
            let wh = read_u32(pool, i + 4 + 12);
            (
                true,
                Some(Prim::Sprt {
                    cmd,
                    color,
                    pos,
                    uv,
                    clut,
                    size: ((wh & 0xFFFF) as u16, ((wh >> 16) & 0xFFFF) as u16),
                }),
            )
        }
        // SPRT_16 (fixed 16x16): 3 payload words.
        0x7C..=0x7F if length == 3 => {
            let (color, pos, uv, clut) = decode_sprt(pool, i);
            (
                true,
                Some(Prim::Sprt16 {
                    cmd,
                    color,
                    pos,
                    uv,
                    clut,
                }),
            )
        }
        // POLY_F3 (flat untextured tri): 4 payload words.
        0x20..=0x23 if length == 4 => {
            let p = i + 4;
            let color = rgb(read_u32(pool, p));
            let verts = [
                vert(read_u32(pool, p + 4)),
                vert(read_u32(pool, p + 8)),
                vert(read_u32(pool, p + 12)),
            ];
            (true, Some(Prim::PolyF3 { cmd, color, verts }))
        }
        // POLY_F4 (flat untextured quad): 5 payload words.
        0x28..=0x2B if length == 5 => {
            let p = i + 4;
            let color = rgb(read_u32(pool, p));
            let verts = [
                vert(read_u32(pool, p + 4)),
                vert(read_u32(pool, p + 8)),
                vert(read_u32(pool, p + 12)),
                vert(read_u32(pool, p + 16)),
            ];
            (true, Some(Prim::PolyF4 { cmd, color, verts }))
        }
        // POLY_G3 (Gouraud untextured tri): 6 payload words.
        0x30..=0x33 if length == 6 => {
            let p = i + 4;
            let colors = [
                rgb(read_u32(pool, p)),
                rgb(read_u32(pool, p + 8)),
                rgb(read_u32(pool, p + 16)),
            ];
            let verts = [
                vert(read_u32(pool, p + 4)),
                vert(read_u32(pool, p + 12)),
                vert(read_u32(pool, p + 20)),
            ];
            (true, Some(Prim::PolyG3 { cmd, colors, verts }))
        }
        // POLY_G4 (Gouraud untextured quad): 8 payload words.
        0x38..=0x3B if length == 8 => {
            let p = i + 4;
            let colors = [
                rgb(read_u32(pool, p)),
                rgb(read_u32(pool, p + 8)),
                rgb(read_u32(pool, p + 16)),
                rgb(read_u32(pool, p + 24)),
            ];
            let verts = [
                vert(read_u32(pool, p + 4)),
                vert(read_u32(pool, p + 12)),
                vert(read_u32(pool, p + 20)),
                vert(read_u32(pool, p + 28)),
            ];
            (true, Some(Prim::PolyG4 { cmd, colors, verts }))
        }
        // Other known cmds we don't handle yet (POLY_F4, POLY_G4, tiles,
        // lines, GP0 control words). Treat as "known kind, wrong length"
        // so we skip without consuming.
        0x20..=0x3F | 0x40..=0x5F | 0x60..=0x7F | 0x80..=0x9F | 0xA0..=0xCF | 0xE0..=0xE6 => {
            (true, None)
        }
        _ => (false, None),
    }
}

pub(super) fn read_u32(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
}

fn vert(w: u32) -> (i16, i16) {
    let x = (w & 0xFFFF) as i16; // sign-extended via i16 cast
    let y = ((w >> 16) & 0xFFFF) as i16;
    (x, y)
}

fn rgb(w: u32) -> [u8; 3] {
    [
        (w & 0xFF) as u8,
        ((w >> 8) & 0xFF) as u8,
        ((w >> 16) & 0xFF) as u8,
    ]
}

/// Decoder return shapes for polygon variants. Aliased here to satisfy
/// the `type_complexity` lint (each decoder returns the per-prim fields
/// minus the shared `cmd` byte). `(color(s), verts, uvs, clut, tpage)`.
type Ft4Fields = ([u8; 3], [(i16, i16); 4], [(u8, u8); 4], u16, u16);
type Gt4Fields = ([[u8; 3]; 4], [(i16, i16); 4], [(u8, u8); 4], u16, u16);
type Ft3Fields = ([u8; 3], [(i16, i16); 3], [(u8, u8); 3], u16, u16);
type Gt3Fields = ([[u8; 3]; 3], [(i16, i16); 3], [(u8, u8); 3], u16, u16);

/// POLY_FT4 layout (payload, 9 words starting at offset 4 past the tag):
///
/// ```text
/// +0  [cmd  | R0 | G0 | B0]
/// +1  [x0   | y0]
/// +2  [u0   | v0 | clut15]
/// +3  [x1   | y1]
/// +4  [u1   | v1 | tpage15]
/// +5  [x2   | y2]
/// +6  [u2   | v2 | pad]
/// +7  [x3   | y3]
/// +8  [u3   | v3 | pad]
/// ```
fn decode_ft4(pool: &[u8], i: usize) -> Ft4Fields {
    let p = i + 4;
    let cmd_word = read_u32(pool, p);
    let color = rgb(cmd_word);
    let v0 = vert(read_u32(pool, p + 4));
    let uv_clut = read_u32(pool, p + 8);
    let v1 = vert(read_u32(pool, p + 12));
    let uv_tpage = read_u32(pool, p + 16);
    let v2 = vert(read_u32(pool, p + 20));
    let uv2_word = read_u32(pool, p + 24);
    let v3 = vert(read_u32(pool, p + 28));
    let uv3_word = read_u32(pool, p + 32);
    let uvs = [
        ((uv_clut & 0xFF) as u8, ((uv_clut >> 8) & 0xFF) as u8),
        ((uv_tpage & 0xFF) as u8, ((uv_tpage >> 8) & 0xFF) as u8),
        ((uv2_word & 0xFF) as u8, ((uv2_word >> 8) & 0xFF) as u8),
        ((uv3_word & 0xFF) as u8, ((uv3_word >> 8) & 0xFF) as u8),
    ];
    let clut = ((uv_clut >> 16) & 0xFFFF) as u16;
    let tpage = ((uv_tpage >> 16) & 0xFFFF) as u16;
    (color, [v0, v1, v2, v3], uvs, clut, tpage)
}

/// POLY_GT4 layout (12 payload words). Per-vertex (color | xy | uv*) repeats:
///
/// ```text
/// +0  [cmd  | R0 | G0 | B0]
/// +1  [x0   | y0]
/// +2  [u0   | v0 | clut15]
/// +3  [R1   | G1 | B1 | pad]
/// +4  [x1   | y1]
/// +5  [u1   | v1 | tpage15]
/// +6  [R2   | G2 | B2 | pad]
/// +7  [x2   | y2]
/// +8  [u2   | v2 | pad]
/// +9  [R3   | G3 | B3 | pad]
/// +10 [x3   | y3]
/// +11 [u3   | v3 | pad]
/// ```
fn decode_gt4(pool: &[u8], i: usize) -> Gt4Fields {
    let p = i + 4;
    let c0 = rgb(read_u32(pool, p));
    let v0 = vert(read_u32(pool, p + 4));
    let uv0_clut = read_u32(pool, p + 8);
    let c1 = rgb(read_u32(pool, p + 12));
    let v1 = vert(read_u32(pool, p + 16));
    let uv1_tpage = read_u32(pool, p + 20);
    let c2 = rgb(read_u32(pool, p + 24));
    let v2 = vert(read_u32(pool, p + 28));
    let uv2 = read_u32(pool, p + 32);
    let c3 = rgb(read_u32(pool, p + 36));
    let v3 = vert(read_u32(pool, p + 40));
    let uv3 = read_u32(pool, p + 44);
    let uvs = [
        ((uv0_clut & 0xFF) as u8, ((uv0_clut >> 8) & 0xFF) as u8),
        ((uv1_tpage & 0xFF) as u8, ((uv1_tpage >> 8) & 0xFF) as u8),
        ((uv2 & 0xFF) as u8, ((uv2 >> 8) & 0xFF) as u8),
        ((uv3 & 0xFF) as u8, ((uv3 >> 8) & 0xFF) as u8),
    ];
    let clut = ((uv0_clut >> 16) & 0xFFFF) as u16;
    let tpage = ((uv1_tpage >> 16) & 0xFFFF) as u16;
    ([c0, c1, c2, c3], [v0, v1, v2, v3], uvs, clut, tpage)
}

fn decode_ft3(pool: &[u8], i: usize) -> Ft3Fields {
    let p = i + 4;
    let color = rgb(read_u32(pool, p));
    let v0 = vert(read_u32(pool, p + 4));
    let uv0_clut = read_u32(pool, p + 8);
    let v1 = vert(read_u32(pool, p + 12));
    let uv1_tpage = read_u32(pool, p + 16);
    let v2 = vert(read_u32(pool, p + 20));
    let uv2 = read_u32(pool, p + 24);
    let uvs = [
        ((uv0_clut & 0xFF) as u8, ((uv0_clut >> 8) & 0xFF) as u8),
        ((uv1_tpage & 0xFF) as u8, ((uv1_tpage >> 8) & 0xFF) as u8),
        ((uv2 & 0xFF) as u8, ((uv2 >> 8) & 0xFF) as u8),
    ];
    let clut = ((uv0_clut >> 16) & 0xFFFF) as u16;
    let tpage = ((uv1_tpage >> 16) & 0xFFFF) as u16;
    (color, [v0, v1, v2], uvs, clut, tpage)
}

fn decode_gt3(pool: &[u8], i: usize) -> Gt3Fields {
    let p = i + 4;
    let c0 = rgb(read_u32(pool, p));
    let v0 = vert(read_u32(pool, p + 4));
    let uv0_clut = read_u32(pool, p + 8);
    let c1 = rgb(read_u32(pool, p + 12));
    let v1 = vert(read_u32(pool, p + 16));
    let uv1_tpage = read_u32(pool, p + 20);
    let c2 = rgb(read_u32(pool, p + 24));
    let v2 = vert(read_u32(pool, p + 28));
    let uv2 = read_u32(pool, p + 32);
    let uvs = [
        ((uv0_clut & 0xFF) as u8, ((uv0_clut >> 8) & 0xFF) as u8),
        ((uv1_tpage & 0xFF) as u8, ((uv1_tpage >> 8) & 0xFF) as u8),
        ((uv2 & 0xFF) as u8, ((uv2 >> 8) & 0xFF) as u8),
    ];
    let clut = ((uv0_clut >> 16) & 0xFFFF) as u16;
    let tpage = ((uv1_tpage >> 16) & 0xFFFF) as u16;
    ([c0, c1, c2], [v0, v1, v2], uvs, clut, tpage)
}

/// Sprite (`SPRT_8` / `SPRT_16`, and the first three words of the free-size
/// `SPRT`) layout:
///
/// ```text
/// +0 [cmd | R | G | B]
/// +1 [x   | y]
/// +2 [u   | v | clut15]
/// ```
fn decode_sprt(pool: &[u8], i: usize) -> ([u8; 3], (i16, i16), (u8, u8), u16) {
    let p = i + 4;
    let color = rgb(read_u32(pool, p));
    let pos = vert(read_u32(pool, p + 4));
    let uv_clut = read_u32(pool, p + 8);
    let uv = ((uv_clut & 0xFF) as u8, ((uv_clut >> 8) & 0xFF) as u8);
    let clut = ((uv_clut >> 16) & 0xFFFF) as u16;
    (color, pos, uv, clut)
}
