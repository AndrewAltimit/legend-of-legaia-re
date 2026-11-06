//! The rates a move-VM part ran in the program segments before its current
//! one, so a channel taken back the display lag follows a rate its program
//! switched inside the lag.
//!
//! A part's program runs to a `WAIT_SET` (op `0x09`, `+0x54 = v1 << 3`),
//! which ends the pass's loop with the PC past it; the part tick then counts
//! `+0x54` down `8` a vsync and resumes the program at zero. The rate ops in
//! the segment that ran before a wait set the rates for that wait:
//! `0x1C` the length rate `+0xCA`, `0x0D` / `0x2B` / `0x37` / `0x2D` / `0x35`
//! the depth-cue rate `+0x90`, `0x14` the radius / height rates
//! `+0xC0..+0xC6` (stored `v << 3` into the animation block at `+0xAC`).
//!
//! So `(set - +0x54) / 8` is how many vsyncs the current rates have run, and
//! a lag longer than that reaches into the previous wait, on the previous
//! segment's rates. PROT 0964's rays (`0x801F9008`) widen at `+0xCA = 96`
//! for thirty-one waits, then switch to `-96` (`0x1C 12`, `0x09 31`,
//! `0x1C -12`, `0x09 31`): `battle_rugi_rogue_rogue_thunder_mid_cast` holds
//! four rays on the switch pass at length `384`, which a rewind on the
//! current rate alone took back to `480` where the frame on screen drew
//! `288`.

use super::*;

/// One program segment: the rates in force while its wait ran, and the
/// vsyncs they integrated for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Segment {
    /// `+0xCA`, `+0x90`, `+0xC0..+0xC6` (`None` = not written yet).
    rates: [Option<i32>; 6],
    wait_vsyncs: i32,
}

/// The rate offsets [`Segment::rates`] tracks, in order.
const RATE_OFFSETS: [u16; 6] = [0xCA, 0x90, 0xC0, 0xC2, 0xC4, 0xC6];

/// The per-vsync count-down of the wait `+0x54`.
const WAIT_PER_VSYNC: i32 = 8;

/// Halfwords an op occupies, for the fixed-size ops a straight-line program
/// can hold (`move_vm::dispatch`). `None` for the loops, the variable-size
/// ops, the overlay escape and the halt: a program holding one is not walked.
fn op_size(op: u16) -> Option<usize> {
    Some(match op {
        0x02 | 0x03 | 0x06 | 0x09 | 0x0D | 0x0E | 0x0F | 0x10 | 0x11 | 0x12 | 0x15 | 0x16
        | 0x17 | 0x1C | 0x1D | 0x25 | 0x28 | 0x29 | 0x2A | 0x31 | 0x32 | 0x38 | 0x3E | 0x3F
        | 0x41 => 2,
        0x22 | 0x30 | 0x33 | 0x3A | 0x3B | 0x43 => 1,
        0x20 | 0x24 | 0x27 | 0x35 | 0x36 | 0x37 => 3,
        0x00 | 0x01 | 0x04 | 0x05 | 0x07 | 0x2B | 0x2D | 0x2E | 0x39 | 0x44 | 0x46 => 4,
        0x14 | 0x26 | 0x2C => 5,
        0x0C => 6,
        0x21 | 0x40 => 7,
        0x1E | 0x1F | 0x45 => 8,
        0x34 => 9,
        0x23 => 0xD,
        0x42 => 0xF,
        0x13 => 0x10,
        _ => return None,
    })
}

/// The segments of part `node`'s program up to its PC, oldest first, and how
/// many vsyncs the newest has run. `None` unless the PC stands just past a
/// `WAIT_SET` and the walk from the record's first op lands on it.
fn segments(ram: &[u8], node: u32) -> Option<(Vec<Segment>, i32)> {
    let rec = game_anchors::u32_at(ram, node + 0x48);
    let pc = usize::try_from(game_anchors::i16_at(ram, node + 0x70)).ok()?;
    if !in_ram(rec) || pc < 4 {
        return None;
    }
    let hw = |i: usize| game_anchors::u16_at(ram, rec + 2 * i as u32);
    if hw(pc - 2) != 0x09 {
        return None;
    }
    let mut rates: [Option<i32>; 6] = [None; 6];
    let mut out = Vec::new();
    // Halfword 0 is the record's model select, halfword 1 its pad: the
    // program opens at 2.
    let mut i = 2usize;
    while i < pc {
        let op = hw(i);
        let size = op_size(op)?;
        let v = |k: usize| i32::from(hw(i + k) as i16);
        match op {
            // The vsync the wait runs out on still integrates on its
            // rates before the program resumes, so a wait of `n` units
            // spans `n / 8 + 1` vsyncs of them.
            0x09 => out.push(Segment {
                rates,
                wait_vsyncs: (v(1) << 3) / WAIT_PER_VSYNC + 1,
            }),
            0x1C => rates[0] = Some(v(1) << 3),
            0x0D => rates[1] = Some(v(1) << 3),
            0x2B | 0x37 => rates[1] = Some(v(1)),
            0x2D | 0x35 => rates[1] = Some(rates[1].unwrap_or(0) + v(1)),
            0x14 => {
                for k in 0..4 {
                    rates[2 + k] = Some(v(1 + k) << 3);
                }
            }
            _ => {}
        }
        i += size;
    }
    if i != pc {
        return None;
    }
    let set = i32::from(hw(pc - 1) as i16) << 3;
    let wait = i32::from(game_anchors::i16_at(ram, node + 0x54));
    let ran = (set - wait) / WAIT_PER_VSYNC;
    (ran >= 0).then_some((out, ran))
}

/// How far channel `rate_off`'s word moved over the `lag` vsyncs before the
/// RAM's pass, in the channel's own units (the rate `>> 3` a vsync), walking
/// back through the segments whose waits the lag reaches. `None` when the
/// program cannot be walked, the lag stays inside the current wait (the
/// caller's single-rate rewind is then exact), or it reaches past the oldest
/// segment.
pub(super) fn rewound_delta(ram: &[u8], node: u32, rate_off: u16, lag: i32) -> Option<i32> {
    let slot = RATE_OFFSETS.iter().position(|&o| o == rate_off)?;
    let (segs, ran) = segments(ram, node)?;
    if lag <= ran {
        return None;
    }
    let cur = segs.last()?.rates[slot].unwrap_or(0);
    let mut delta = (cur >> 3) * ran;
    let mut left = lag - ran;
    for seg in segs.iter().rev().skip(1) {
        let take = left.min(seg.wait_vsyncs);
        delta += (seg.rates[slot].unwrap_or(0) >> 3) * take;
        left -= take;
        if left == 0 {
            return Some(delta);
        }
    }
    None
}
