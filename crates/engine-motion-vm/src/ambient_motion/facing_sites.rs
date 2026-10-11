//! Static scan of a motion script for its facing writes, and their replay.

use super::*;

/// A `0x04` / `0x0D` site found by walking a variant's bytecode - the shape
/// the disc census and the oracle both consume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmbientFacingSite {
    /// Byte offset within the variant's bytecode.
    pub offset: usize,
    /// `0x04` or `0x0D`.
    pub op: u8,
    /// Compass LUT index, `b1 & 7`.
    pub lut_index: u8,
    /// `b1 & 0x80` - set forces the decreasing direction.
    pub decreasing: bool,
    /// `0x04`: `b2 & 0x7F` frames. `0x0D`: the 16-bit `b2 | b3 << 8`.
    pub duration: i32,
}

impl AmbientFacingSite {
    /// Endpoint heading in retail space.
    pub fn target(&self) -> u16 {
        heading_lut_retail(self.lut_index)
    }
}

/// Walk one variant's bytecode and collect every `0x04` / `0x0D` site,
/// stepping non-facing ops by [`op_width`]. Stops at the first op whose
/// width is unknown.
pub fn facing_sites(code: &[u8]) -> Vec<AmbientFacingSite> {
    let mut out = Vec::new();
    let mut pc = 0usize;
    while pc < code.len() {
        let op = code[pc];
        let Some(width) = op_width(op) else { break };
        if pc + width > code.len() {
            break;
        }
        let body = &code[pc..pc + width];
        match op {
            0x04 => out.push(AmbientFacingSite {
                offset: pc,
                op,
                lut_index: body[1] & 7,
                decreasing: body[1] & 0x80 != 0,
                duration: i32::from(body[2] & 0x7F),
            }),
            0x0D => out.push(AmbientFacingSite {
                offset: pc,
                op,
                lut_index: body[1] & 7,
                decreasing: body[1] & 0x80 != 0,
                duration: i32::from(i16::from_le_bytes([body[2], body[3]])),
            }),
            _ => {}
        }
        pc += width;
    }
    out
}

/// Replay one facing site standalone until it retires, returning the raw
/// per-tick heading trace (the value after each tick, including the
/// terminal one). `speed` is `_DAT_1F800393`.
///
/// The oracle uses this to assert a disc-sourced op lands on its compass
/// endpoint in its authored budget.
pub fn replay_site(site: &AmbientFacingSite, start_heading: u16, speed: u8) -> Vec<u16> {
    let code: Vec<u8> = match site.op {
        0x04 => vec![
            0x04,
            (site.lut_index & 7) | if site.decreasing { 0x80 } else { 0 },
            (site.duration as u8) & 0x7F,
        ],
        _ => {
            let d = (site.duration as i16).to_le_bytes();
            vec![
                0x0D,
                (site.lut_index & 7) | if site.decreasing { 0x80 } else { 0 },
                d[0],
                d[1],
            ]
        }
    };
    let mut vm = AmbientMotion::new(1, start_heading);
    let mut trace = Vec::new();
    // A budget of frames + 2 is enough for `0x04` (frames stepping ticks,
    // one terminal); `0x0D` retires in `ceil(duration / speed) + 1`.
    let cap = (site.duration.max(0) as usize / usize::from(speed.max(1)))
        + site.duration.max(0) as usize
        + 8;
    for _ in 0..cap {
        vm.tick(&code, speed);
        trace.push(vm.heading);
        if usize::from(vm.pc) >= code.len() {
            break;
        }
    }
    trace
}
