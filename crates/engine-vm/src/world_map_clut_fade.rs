//! World-map single-source CLUT blend-to-target fade actor, ported
//! clean-room from `FUN_801E4D8C`.
//!
//! PORT: FUN_801E4D8C
//! REF: FUN_8003CE9C (misaligned-LE16 operand reads), FUN_8005842C
//! (`LoadImage`), FUN_800583C8 (`StoreImage`), FUN_80058104 (`DrawSync`)
//!
//! ## What it is, and how it differs from the `4C 61` cross-fade
//!
//! This is a **second, distinct** CLUT-fade actor in the world-map / field
//! overlay band (`overlay_world_map_top_801e4d8c.txt`, base `0x801C0000`). The
//! already-ported `FUN_801E4794` family ([`legaia_engine_core::clut_fx`])
//! fades one VRAM cell **A -> B** into a destination. `FUN_801E4D8C` instead
//! reads a **single** source CLUT out of VRAM and fades it toward a **flat
//! target colour** carried in its data record, by a fixed *endpoint fraction*,
//! over a duration - a "tint / dim a live CLUT toward one colour" actor rather
//! than an "interpolate between two rows" one.
//!
//! The actor struct is the standard field-actor block. The fields this reader
//! touches:
//!
//! | Offset | Meaning |
//! |---|---|
//! | `+0x10` | flags; completion sets bit `0x8` |
//! | `+0x54` | init latch (`0` = first tick, does the VRAM read + endpoint precompute) |
//! | `+0x80..+0x90` | per-entry STP (bit-15) bytes, one per CLUT entry |
//! | `+0x90` | pointer to the data record (operands below) |
//! | `+0x98` | pointer to the `0x60`-byte scratch buffer (16 entries x 6 bytes) |
//! | `+0x9e` | vsync accumulator (`i16`) |
//!
//! The data record at `+0x90`, read through `FUN_8003CE9C` (misaligned LE16)
//! and plain byte loads:
//!
//! | Record offset | Meaning |
//! |---|---|
//! | `+1`, `+3` | source/destination CLUT cell `(x, y)` in VRAM (16x1 row) |
//! | `+5`, `+6`, `+7` | flat target colour bytes `(R, G, B)`, 8-bit |
//! | `+8` | endpoint blend fraction, Q12 (`0..=0x1000`) |
//! | `+0xA` | fade duration in vsyncs |
//!
//! ## First tick (init, `+0x54 == 0`)
//!
//! `LoadImage`s the 16x1 CLUT row from VRAM, then for each of the 16 entries
//! decodes the BGR555 pixel into three 8-bit-scaled channel bytes
//! (`chan5 << 3`), stashes the STP bit at `+0x80+i`, and precomputes the
//! **endpoint** channel bytes as a partial blend toward the target:
//! `end = base + ((target - base) * frac >> 12)` (logical shift; retail stores
//! the result with `sb`, so it wraps as a byte). The scratch layout per entry
//! is `[base_b, base_g, base_r, end_b, end_g, end_r]`.
//!
//! ## Every tick
//!
//! `acc += speed` (the scratchpad frame-step byte `DAT_1F800393`, vsyncs per
//! game tick). While `acc < duration`, each entry interpolates
//! `base + (end - base) * acc / duration` per channel and repacks to BGR555
//! with its STP bit; the row is `StoreImage`d back. On `acc >= duration` the
//! row is repacked straight from the endpoint bytes, the scratch is freed and
//! flag bit `0x8` is set (the actor retires).
//!
//! Two faithful retail quirks are preserved verbatim: the endpoint precompute
//! uses a **logical** right shift over a possibly-negative product, and the
//! per-tick interpolation divides an `i32` product by the duration with an
//! **unsigned** `divu`. Both wrap rather than clamp; a target below the source
//! is arithmetically well-defined but does not produce a visually monotone
//! fade - that is how retail behaves.
//!
//! This module is the pure arithmetic kernel (decode / endpoint / per-tick
//! repack). The VRAM `LoadImage` / `StoreImage` and the `0x60`-byte scratch
//! allocation stay the host's job, exactly as `world_map_dim` keeps the
//! `AddPrim` posting out of the kernel.

/// Entries in the 16x1 CLUT row this actor fades.
pub const CLUT_ENTRIES: usize = 16;

/// The data record read at actor `+0x90`. Coordinates are VRAM framebuffer
/// cells; `target` is a flat 8-bit `(R, G, B)`; `frac` is Q12; `duration` is
/// in vsyncs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FadeRecord {
    /// Source/destination CLUT cell `(x, y)` (record `+1`, `+3`). The kernel
    /// does not use it - it is the host's `LoadImage`/`StoreImage` target -
    /// but it is decoded here to keep the record self-describing.
    pub cell: (i16, i16),
    /// Flat target colour bytes `(R, G, B)` (record `+5`, `+6`, `+7`).
    pub target: (u8, u8, u8),
    /// Endpoint blend fraction, Q12 (record `+8`). `0x1000` == blend fully to
    /// `target`; `0` == endpoint equals the loaded source.
    pub frac: i16,
    /// Fade duration in vsyncs (record `+0xA`).
    pub duration: i16,
}

impl FadeRecord {
    /// Decode the record the way retail does: `FUN_8003CE9C` is a
    /// sign-extending misaligned LE16 read, and the colour/duration bytes are
    /// plain byte loads. `bytes` must cover at least record offsets `0..=0xB`.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let w = |o: usize| i16::from_le_bytes([bytes[o], bytes[o + 1]]);
        Self {
            cell: (w(1), w(3)),
            target: (bytes[5], bytes[6], bytes[7]),
            frac: w(8),
            duration: w(0xA),
        }
    }
}

/// Per-entry scratch: the loaded source channel bytes and the precomputed
/// endpoint channel bytes, plus the STP bit. Mirrors retail's 6-byte record
/// (`[base_b, base_g, base_r, end_b, end_g, end_r]`) at scratch `+0x98` with
/// the STP byte at actor `+0x80`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct FadeTexel {
    base: [u8; 3],
    end: [u8; 3],
    stp: bool,
}

/// Decode a BGR555 pixel into the three 8-bit-scaled channel bytes retail
/// stores (`chan5 << 3`), in `[blue, green, red]` order, plus the STP bit.
///
/// Retail: `(v & 0x7C00) >> 7` (blue), `(v & 0x03E0) >> 2` (green),
/// `(v & 0x001F) << 3` (red), `v >> 15` (STP). Each channel byte lands in
/// `0..=0xF8`.
fn decode_pixel(v: u16) -> ([u8; 3], bool) {
    let b = ((v & 0x7C00) >> 7) as u8;
    let g = ((v & 0x03E0) >> 2) as u8;
    let r = ((v & 0x001F) << 3) as u8;
    ([b, g, r], v >> 15 != 0)
}

/// Endpoint channel byte: `base + ((target - base) * frac >> 12)`. Retail
/// computes the product as a 32-bit signed multiply, applies a **logical**
/// shift, and stores with `sb`, so the result wraps to a byte.
fn endpoint_channel(base: u8, target: u8, frac: i16) -> u8 {
    let diff = target as i32 - base as i32;
    let prod = (diff as u32).wrapping_mul(frac as u32);
    (base as u32).wrapping_add(prod >> 12) as u8
}

/// Per-tick interpolated channel term `(end - base) * acc / duration`. Retail
/// forms the product with a signed multiply (low 32 bits) then an **unsigned**
/// `divu`; the result is added to `base`. `acc` is non-negative here (it only
/// grows from `0`) and `duration` is non-zero on this path.
fn interp_term(base: u8, end: u8, acc: i16, duration: i16) -> u32 {
    let diff = end as i32 - base as i32;
    let prod = (diff as u32).wrapping_mul(acc as u32);
    prod / duration as u32
}

/// Repack a channel triple + STP into BGR555 the mid-fade way: each channel is
/// `base + interp_term`, then blue `<< 7 & 0x7C00`, green `<< 2 & 0x03E0`, red
/// `>> 3 & 0x001F`, STP at bit 15.
fn repack_mid(t: &FadeTexel, acc: i16, duration: i16) -> u16 {
    let bb = (t.base[0] as u32).wrapping_add(interp_term(t.base[0], t.end[0], acc, duration));
    let gg = (t.base[1] as u32).wrapping_add(interp_term(t.base[1], t.end[1], acc, duration));
    let rr = (t.base[2] as u32).wrapping_add(interp_term(t.base[2], t.end[2], acc, duration));
    let stp = (t.stp as u32) << 15;
    (stp | ((bb << 7) & 0x7C00) | ((gg << 2) & 0x03E0) | ((rr >> 3) & 0x001F)) as u16
}

/// Repack straight from the endpoint bytes (completion frame): red `>> 3`,
/// blue `& 0xF8 << 7`, green `& 0xF8 << 2`, STP at bit 15.
fn repack_end(t: &FadeTexel) -> u16 {
    let stp = (t.stp as u16) << 15;
    let b = ((t.end[0] & 0xF8) as u16) << 7;
    let g = ((t.end[1] & 0xF8) as u16) << 2;
    let r = (t.end[2] >> 3) as u16;
    stp | b | g | r
}

/// Result of one [`ClutBlendFade::tick`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FadeStep {
    /// Mid-fade: `StoreImage` this row back to the source cell.
    Row([u16; CLUT_ENTRIES]),
    /// `acc >= duration`: the completion row - the caller uploads it, frees
    /// the scratch and retires the actor (retail sets flag bit `0x8`).
    Done([u16; CLUT_ENTRIES]),
}

/// The single-source blend-to-target CLUT fade state.
///
/// PORT: FUN_801E4D8C
#[derive(Debug, Clone)]
pub struct ClutBlendFade {
    texels: [FadeTexel; CLUT_ENTRIES],
    /// Vsync accumulator (retail actor `+0x9e`, an `i16`).
    acc: i16,
    duration: i16,
}

impl ClutBlendFade {
    /// Init from the loaded CLUT row and the decoded record (retail's
    /// `+0x54 == 0` first-tick block): decode each source pixel and precompute
    /// its endpoint. The accumulator starts at `0`; the first [`Self::tick`]
    /// advances and renders it, matching retail's one-call init-plus-render.
    // PORT: FUN_801E4D8C (init block)
    pub fn new(loaded: &[u16; CLUT_ENTRIES], record: &FadeRecord) -> Self {
        let mut texels = [FadeTexel::default(); CLUT_ENTRIES];
        for (texel, &pixel) in texels.iter_mut().zip(loaded.iter()) {
            let (base, stp) = decode_pixel(pixel);
            let end = [
                endpoint_channel(base[0], record.target.2, record.frac),
                endpoint_channel(base[1], record.target.1, record.frac),
                endpoint_channel(base[2], record.target.0, record.frac),
            ];
            *texel = FadeTexel { base, end, stp };
        }
        Self {
            texels,
            acc: 0,
            duration: record.duration,
        }
    }

    /// Advance one game tick by `speed` vsyncs (the scratchpad frame-step byte
    /// `DAT_1F800393`) and produce the row to upload. Returns [`FadeStep::Row`]
    /// while `acc < duration` and [`FadeStep::Done`] once `acc >= duration`.
    // PORT: FUN_801E4D8C (per-tick render)
    pub fn tick(&mut self, speed: u8) -> FadeStep {
        self.acc = self.acc.wrapping_add(speed as i16);
        let mut row = [0u16; CLUT_ENTRIES];
        if self.acc < self.duration {
            for (out, t) in row.iter_mut().zip(self.texels.iter()) {
                *out = repack_mid(t, self.acc, self.duration);
            }
            FadeStep::Row(row)
        } else {
            for (out, t) in row.iter_mut().zip(self.texels.iter()) {
                *out = repack_end(t);
            }
            FadeStep::Done(row)
        }
    }

    /// The current vsync accumulator (`+0x9e`).
    pub fn accumulator(&self) -> i16 {
        self.acc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_decode_reads_retail_offsets() {
        // cell(+1,+3), target(+5,+6,+7), frac(+8), duration(+0xA).
        let bytes = [
            0x00, 0x40, 0x00, 0x0A, 0x00, 0x10, 0x20, 0x30, 0x00, 0x10, 0x20, 0x00,
        ];
        let r = FadeRecord::from_bytes(&bytes);
        assert_eq!(r.cell, (0x40, 0x0A));
        assert_eq!(r.target, (0x10, 0x20, 0x30));
        assert_eq!(r.frac, 0x1000);
        assert_eq!(r.duration, 0x20);
    }

    #[test]
    fn decode_pixel_matches_retail_channel_math() {
        // Pure white, STP set: 0xFFFF -> each channel 0x1F<<3 = 0xF8, stp on.
        let (c, stp) = decode_pixel(0xFFFF);
        assert_eq!(c, [0xF8, 0xF8, 0xF8]);
        assert!(stp);
        // Pure red (bits 0..4): channel byte 0xF8 on red, others 0, no STP.
        let (c, stp) = decode_pixel(0x001F);
        assert_eq!(c, [0, 0, 0xF8]);
        assert!(!stp);
    }

    #[test]
    fn endpoint_frac_zero_is_the_source() {
        // frac 0 -> endpoint == base for every channel.
        assert_eq!(endpoint_channel(0x40, 0xF8, 0), 0x40);
    }

    #[test]
    fn endpoint_frac_full_reaches_target() {
        // frac 0x1000 (Q12 one) -> base + (target-base) == target.
        assert_eq!(endpoint_channel(0x40, 0xF8, 0x1000), 0xF8);
        assert_eq!(endpoint_channel(0xF8, 0x00, 0x1000), 0x00);
    }

    #[test]
    fn endpoint_half_frac_is_midpoint() {
        // frac 0x800 == 0.5: 0x40 + (0xF8-0x40)/2 = 0x40 + 0x5C = 0x9C.
        assert_eq!(endpoint_channel(0x40, 0xF8, 0x800), 0x9C);
    }

    #[test]
    fn completion_row_repacks_from_endpoint() {
        // A source that is already the target (frac full) fades to a stable
        // endpoint; the Done row round-trips that colour.
        let loaded = [0x001Fu16; CLUT_ENTRIES]; // pure red
        let rec = FadeRecord {
            cell: (0, 0),
            target: (0xF8, 0, 0), // R target = source -> endpoint stays red
            frac: 0x1000,
            duration: 4,
        };
        let mut fade = ClutBlendFade::new(&loaded, &rec);
        // Drive past the duration.
        let step = fade.tick(0x40);
        match step {
            FadeStep::Done(row) => {
                // endpoint red 0xF8 -> repack_end red = 0xF8>>3 = 0x1F.
                assert_eq!(row[0] & 0x001F, 0x1F);
                assert_eq!(row[0] & 0x7C00, 0); // no blue
                assert_eq!(row[0] & 0x03E0, 0); // no green
            }
            FadeStep::Row(_) => panic!("expected Done past duration"),
        }
    }

    #[test]
    fn mid_fade_before_duration_returns_row() {
        let loaded = [0x0000u16; CLUT_ENTRIES];
        let rec = FadeRecord {
            cell: (0, 0),
            target: (0xF8, 0xF8, 0xF8),
            frac: 0x1000,
            duration: 100,
        };
        let mut fade = ClutBlendFade::new(&loaded, &rec);
        // acc = 50 < 100 -> mid-fade row, roughly half toward white.
        match fade.tick(50) {
            FadeStep::Row(row) => {
                // base 0, end 0xF8, acc/dur = 0.5 -> red term ~0x7C, >>3 -> ~0xF.
                let red5 = row[0] & 0x001F;
                assert!((0x0C..=0x10).contains(&red5), "red5={red5:#x}");
            }
            FadeStep::Done(_) => panic!("50 < 100 should be mid-fade"),
        }
    }

    #[test]
    fn stp_bit_survives_both_paths() {
        let loaded = [0x8000u16; CLUT_ENTRIES]; // black, STP set
        let rec = FadeRecord {
            cell: (0, 0),
            target: (0, 0, 0),
            frac: 0x1000,
            duration: 2,
        };
        let mut fade = ClutBlendFade::new(&loaded, &rec);
        // First tick: acc=1 < 2 -> mid row, STP kept.
        match fade.tick(1) {
            FadeStep::Row(row) => assert_eq!(row[0] & 0x8000, 0x8000),
            FadeStep::Done(_) => panic!("acc 1 < 2"),
        }
        // Second tick: acc=2 >= 2 -> Done row, STP kept.
        match fade.tick(1) {
            FadeStep::Done(row) => assert_eq!(row[0] & 0x8000, 0x8000),
            FadeStep::Row(_) => panic!("acc 2 >= 2"),
        }
    }

    #[test]
    fn accumulator_advances_by_speed() {
        let loaded = [0u16; CLUT_ENTRIES];
        let rec = FadeRecord {
            cell: (0, 0),
            target: (0, 0, 0),
            frac: 0,
            duration: 1000,
        };
        let mut fade = ClutBlendFade::new(&loaded, &rec);
        fade.tick(3);
        fade.tick(4);
        assert_eq!(fade.accumulator(), 7);
    }
}
