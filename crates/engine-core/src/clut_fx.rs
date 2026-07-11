//! Scripted CLUT-cell effect family - the field-VM `0x4C` n6 sub-`0x61`
//! one-shot cell write and the multi-frame cross-fade actor it spawns.
//!
//! PORT: FUN_801E4C58 (one-shot emitter + fade-actor spawn)
//! PORT: FUN_801E4794 (cross-fade state machine)
//! REF: FUN_8003CE9C (LE16 operand reads), FUN_80016B6C (the `DAT_1F800393`
//! frame-step writer the fade's per-tick advance multiplies by)
//!
//! The instruction is `[4C, 61, ...14 operand bytes]`; the emitter reads its
//! operands via the misaligned-LE16 helper `FUN_8003CE9C` relative to the
//! sub-byte pointer: `+1/+3` = CLUT cell A `(x, y)`, `+5/+7` = cell B,
//! `+9/+0xB` = destination cell, `+0xD` = frame count (all cells are 16x1
//! CLUT rows in VRAM framebuffer coordinates). A `B.y == 0` operand pair
//! means "B is the flat BGR555 colour `B.x`", not a VRAM cell.
//!
//! - `frames == 0` (`FUN_801E4C58` inline path): one-shot - `MoveImage` cell
//!   B onto the destination (or flat-fill the destination when `B.y == 0`).
//!   Cell A is not read on this path.
//! - `frames != 0`: spawn the fade actor (descriptor `DAT_801F2918`) whose
//!   per-tick handler is `FUN_801E4794`: first tick `StoreImage`s cells A and
//!   B, expands every entry into three per-channel 16-bit fixed-point values
//!   (5-bit channel at bits 10..14, 10 fraction bits below), and precomputes
//!   per-channel deltas `(B - A) / frames` (MIPS `div` - signed, truncating
//!   toward zero). Every tick the counter advances by the scratchpad
//!   frame-step byte `DAT_1F800393` (`dt`, vsyncs per game tick - see
//!   [`crate::world::World::frame_step`]); while `counter < frames` each
//!   channel accumulates `delta * dt` and the repacked row is `LoadImage`d to
//!   the destination, so the fade completes in `frames` *vsyncs* regardless
//!   of the frame-skip factor. On `counter >= frames`: `MoveImage` cell B to
//!   the destination (or flat-fill), free the scratch, clear the spawning
//!   script context's halt bit (`*(ctx+0x94) + 0x10 &= ~0x400`), retire.
//!
//! Dumps: `ghidra/scripts/funcs/801e4794.txt` (also
//! `overlay_world_map_801e4794.txt`) and
//! `ghidra/scripts/funcs/overlay_baka_fighter_801e4c58.txt` (the
//! `overlay_0897_801e4c58.txt` dump is bad-base data - see
//! `docs/reference/functions.md`).
//!
//! This module is the pure arithmetic kernel; the VRAM-touching driver
//! (StoreImage / LoadImage equivalents against the engine's software VRAM)
//! is [`crate::world::World::step_clut_fx`].

/// Entries in one 16x1 CLUT cell.
pub const CLUT_CELL_ENTRIES: usize = 16;

/// Decoded operands of one `4C 61` CLUT-cell effect instruction.
///
/// Coordinates are VRAM framebuffer cells (`x` in halfwords, `y` in rows);
/// `frames` is the fade length in vsyncs (`0` = one-shot).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClutCellFxOp {
    /// Cell A - the fade's *from* row (`+1/+3`). Unused by the one-shot.
    pub a: (i16, i16),
    /// Cell B - the fade's *to* row / the one-shot's source (`+5/+7`).
    /// `b.1 == 0` means `b.0` is a flat BGR555 colour, not a cell.
    pub b: (i16, i16),
    /// Destination cell (`+9/+0xB`).
    pub dest: (i16, i16),
    /// Fade length in vsyncs (`+0xD`); `0` selects the one-shot path.
    pub frames: i16,
}

impl ClutCellFxOp {
    /// Decode from the 14-byte operand slice the field VM hands
    /// `op4c_n6_sub_61_emitter` (instruction bytes `+2..+16`, i.e. operand
    /// offsets `+1..+15` relative to the `0x61` sub-byte).
    pub fn from_payload(payload: &[u8; 14]) -> Self {
        let w = |o: usize| i16::from_le_bytes([payload[o], payload[o + 1]]);
        Self {
            a: (w(0), w(2)),
            b: (w(4), w(6)),
            dest: (w(8), w(10)),
            frames: w(12),
        }
    }

    /// True when cell B is a flat BGR555 colour (`B.y == 0`), not a cell.
    pub fn b_is_flat(&self) -> bool {
        self.b.1 == 0
    }
}

/// Result of one [`ClutFade::step`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClutFadeStep {
    /// Mid-fade: upload this row to the destination cell (`LoadImage`).
    Row([u16; CLUT_CELL_ENTRIES]),
    /// `counter >= frames`: the fade is over. The caller performs the
    /// completion write (copy cell B / flat-fill) and retires the effect.
    Done,
}

/// The cross-fade arithmetic state - retail's `0xE0`-byte scratch buffer.
///
/// PORT: FUN_801E4794
#[derive(Debug, Clone)]
pub struct ClutFade {
    /// Per-entry per-channel accumulators (16 entries x `[B, G, R]`), the
    /// channel value at bits 10..14 with 10 fraction bits - retail scratch
    /// `+0x00..+0x60` (cell A expanded in place).
    cur: [u16; CLUT_CELL_ENTRIES * 3],
    /// Per-channel signed steps `(B - A) / frames` - retail scratch
    /// `+0x60..+0xC0` (cell B expanded, then divided in place).
    delta: [i16; CLUT_CELL_ENTRIES * 3],
    /// Vsync counter (retail `ctx+0x54`, a u16 the compare reads as i16).
    counter: u16,
    /// Total fade length in vsyncs (operand `+0xD`).
    frames: i16,
}

/// Expand one BGR555 entry into the retail per-channel fixed-point triple
/// `[blue, green, red]`, each 5-bit channel placed at bits 10..14 (blue is
/// already there; green `<< 5`; red `<< 10`). The STP bit (0x8000) is
/// dropped - the fade output never carries it.
fn expand_entry(v: u16) -> [u16; 3] {
    [(v & 0x7C00), (v & 0x03E0) << 5, (v & 0x001F) << 10]
}

impl ClutFade {
    /// First-tick precompute: expand rows A and B per channel and derive the
    /// per-channel deltas `(B - A) / frames` (signed division truncating
    /// toward zero, as the MIPS `div` in the dump).
    ///
    /// `frames` must be non-zero (the emitter routes `frames == 0` to the
    /// one-shot path; retail would `break` on the divide).
    pub fn new(
        a_row: &[u16; CLUT_CELL_ENTRIES],
        b_row: &[u16; CLUT_CELL_ENTRIES],
        frames: i16,
    ) -> Self {
        debug_assert!(frames != 0, "frames == 0 is the one-shot path");
        let mut cur = [0u16; CLUT_CELL_ENTRIES * 3];
        let mut delta = [0i16; CLUT_CELL_ENTRIES * 3];
        for i in 0..CLUT_CELL_ENTRIES {
            let a = expand_entry(a_row[i]);
            let b = expand_entry(b_row[i]);
            for c in 0..3 {
                cur[i * 3 + c] = a[c];
                delta[i * 3 + c] = ((i32::from(b[c]) - i32::from(a[c])) / i32::from(frames)) as i16;
            }
        }
        Self {
            cur,
            delta,
            counter: 0,
            frames,
        }
    }

    /// One game tick: `counter += dt`; while `counter < frames` accumulate
    /// `delta * dt` per channel and return the repacked row; once
    /// `counter >= frames` return [`ClutFadeStep::Done`] (the completion
    /// write is the caller's - retail `MoveImage`s cell B / flat-fills).
    ///
    /// Retail runs this once per game tick with `dt` = the adaptive
    /// frame-step byte `DAT_1F800393`, so a `frames = 128` fade takes 64
    /// ticks at `dt = 2` and 43 at `dt = 3` - always ~128 vsyncs.
    pub fn step(&mut self, dt: u8) -> ClutFadeStep {
        self.counter = self.counter.wrapping_add(u16::from(dt));
        if (self.counter as i16) >= self.frames {
            return ClutFadeStep::Done;
        }
        let mut row = [0u16; CLUT_CELL_ENTRIES];
        for (i, out) in row.iter_mut().enumerate() {
            for c in 0..3 {
                let k = i * 3 + c;
                self.cur[k] =
                    (i32::from(self.cur[k]) + i32::from(self.delta[k]) * i32::from(dt)) as u16;
            }
            *out = (self.cur[i * 3] & 0x7C00)
                | ((self.cur[i * 3 + 1] & 0x7C00) >> 5)
                | ((self.cur[i * 3 + 2] & 0x7C00) >> 10);
        }
        ClutFadeStep::Row(row)
    }

    /// Vsyncs elapsed so far (the retail `ctx+0x54` counter).
    pub fn counter(&self) -> u16 {
        self.counter
    }
}

/// Build the raw B row for a flat-colour operand pair (`B.y == 0`): retail
/// fills the scratch B slots with `flat | (A_entry & 0x8000)` - the flat
/// colour with each A entry's STP bit carried over (the expansion then
/// drops STP anyway, but the raw row matters for byte-parity of the
/// scratch).
pub fn flat_b_row(a_row: &[u16; CLUT_CELL_ENTRIES], flat: u16) -> [u16; CLUT_CELL_ENTRIES] {
    let mut out = [0u16; CLUT_CELL_ENTRIES];
    for (o, a) in out.iter_mut().zip(a_row.iter()) {
        *o = flat | (a & 0x8000);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `frames = 128` fade completes in 64 steps at `dt = 2` and 43 steps
    /// at `dt = 3` - both ~128 vsyncs (the dt=3 run overshoots to 129, the
    /// retail `>=` compare).
    #[test]
    fn completion_is_vsync_denominated() {
        let a = [0u16; 16];
        let b = [0x7FFFu16; 16];
        for (dt, expect_steps, expect_vsyncs) in [(2u8, 64u32, 128u16), (3, 43, 129)] {
            let mut fade = ClutFade::new(&a, &b, 128);
            let mut steps = 0;
            loop {
                steps += 1;
                match fade.step(dt) {
                    ClutFadeStep::Row(_) => continue,
                    ClutFadeStep::Done => break,
                }
            }
            assert_eq!(steps, expect_steps, "dt={dt}");
            assert_eq!(fade.counter(), expect_vsyncs, "dt={dt}");
        }
    }

    /// Deltas divide truncating toward zero and the per-tick advance is
    /// `delta * dt` in the 10-fraction-bit space; the packed row reads the
    /// channel bits back out of bits 10..14.
    #[test]
    fn delta_arithmetic_matches_retail_fixed_point() {
        // A = black, B = pure red 31 (bits 0..4). Red expands to 31 << 10 =
        // 0x7C00; delta = 0x7C00 / 128 = 0xF8 per vsync.
        let a = [0u16; 16];
        let b = [0x001Fu16; 16];
        let mut fade = ClutFade::new(&a, &b, 128);
        // First step at dt = 2: red accumulator = 2 * 0xF8 = 0x1F0 ->
        // channel bits (>= 0x400 per level) still 0 -> packed red = 0.
        match fade.step(2) {
            ClutFadeStep::Row(row) => assert_eq!(row[0], 0),
            ClutFadeStep::Done => panic!("fade ended early"),
        }
        // After 4 more steps (5 total = 10 vsyncs) the accumulator is
        // 5 * 0x1F0 = 0x9B0 -> red level 2 (0x9B0 >> 10).
        let mut last = 0u16;
        for _ in 0..4 {
            if let ClutFadeStep::Row(row) = fade.step(2) {
                last = row[0];
            }
        }
        assert_eq!(last, 2, "red level after 10 accumulated vsyncs");
        // Truncating division: (0x7C00 - 0) / 96 = 330 (truncated from
        // 330.66..); i32 semantics match MIPS div for negative deltas too.
        let fade2 = ClutFade::new(&[0x001F; 16], &[0; 16], 96);
        assert_eq!(fade2.delta[2], -(0x7C00 / 96));
    }

    #[test]
    fn payload_decode_matches_operand_offsets() {
        // map01 fade op #1 (MAN offset 0xf03), instruction bytes +2..+16.
        let payload: [u8; 14] = [
            0x00, 0x00, 0xF2, 0x01, 0x70, 0x00, 0xF3, 0x01, 0x00, 0x00, 0xF2, 0x01, 0x80, 0x00,
        ];
        let op = ClutCellFxOp::from_payload(&payload);
        assert_eq!(op.a, (0, 498));
        assert_eq!(op.b, (112, 499));
        assert_eq!(op.dest, (0, 498));
        assert_eq!(op.frames, 128);
        assert!(!op.b_is_flat());
    }

    #[test]
    fn flat_b_carries_stp_bits() {
        let mut a = [0u16; 16];
        a[3] = 0x8000 | 0x1234;
        let b = flat_b_row(&a, 0x03E0);
        assert_eq!(b[0], 0x03E0);
        assert_eq!(b[3], 0x83E0);
    }
}
