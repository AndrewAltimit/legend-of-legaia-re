//! The damage numerals the **displayed** frame drew, read off the two
//! packet arenas' digit quads.

use super::*;

/// The readout's digit quad `FUN_801DF6B8` emits per cell: a `POLY_FT4`
/// (tag length `9`) in colour `0x2C808080`, its texels a 24-texel cell of
/// the glyph row `v = 64`.
const DIGIT_COLOUR: u32 = 0x2C80_8080;
const DIGIT_ROW_V: u8 = 64;
/// The combo cluster's digits share the quad; they sit on rows from `142`
/// down, where no numeral is drawn (the readout's rest row is `32` and its
/// runs rise toward it).
const CLUSTER_TOP_Y: i16 = 140;

/// One digit quad: top-left, extent, digit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DigitQuad {
    pub x: i16,
    pub y: i16,
    pub w: i16,
    pub digit: u8,
    pub(super) addr: u32,
}

/// Every numeral digit quad in main RAM, in address order.
pub(super) fn digit_quads(ram: &[u8]) -> Vec<DigitQuad> {
    let mut out = Vec::new();
    let len = ram.len().min(0x20_0000);
    let mut off = 0usize;
    while off + 0x28 <= len {
        let w0 = u32::from_le_bytes(ram[off..off + 4].try_into().unwrap());
        let w1 = u32::from_le_bytes(ram[off + 4..off + 8].try_into().unwrap());
        if w0 >> 24 == 9 && w1 == DIGIT_COLOUR {
            let h = |o: usize| i16::from_le_bytes([ram[off + o], ram[off + o + 1]]);
            let (x0, y0, x1, y2) = (h(8), h(10), h(16), h(26));
            let (u0, v0, v2) = (ram[off + 12], ram[off + 13], ram[off + 29]);
            if v0 == DIGIT_ROW_V
                && v2 == DIGIT_ROW_V + 23
                && u0 % 24 == 0
                && y0 < CLUSTER_TOP_Y
                && x1 >= x0
                && y2 >= y0
            {
                out.push(DigitQuad {
                    x: x0,
                    y: y0,
                    w: x1 - x0 + 1,
                    digit: (u0 / 24 + 1) % 10,
                    addr: 0x8000_0000 + off as u32,
                });
            }
        }
        off += 4;
    }
    out
}

/// The two arenas' numeral quads, split at the widest address gap: the
/// lower and the higher arena. `None` unless both hold the same number of
/// digits.
fn arena_runs(ram: &[u8]) -> Option<(Vec<DigitQuad>, Vec<DigitQuad>)> {
    let quads = digit_quads(ram);
    if quads.len() < 2 {
        return None;
    }
    let cut = (1..quads.len()).max_by_key(|&i| quads[i].addr - quads[i - 1].addr)?;
    let (a, b) = quads.split_at(cut);
    (a.len() == b.len()).then(|| (a.to_vec(), b.to_vec()))
}

/// Is the packet at `addr` linked into the ordering table whose bucket
/// array starts at `table`? A tag word's low 24 bits name the next packet,
/// so the walk runs the links backwards from the packet until it reaches a
/// bucket; the two tables' bucket arrays sit `0x416C` bytes apart, which
/// bounds one array.
fn linked_into(prev: &std::collections::HashMap<u32, u32>, addr: u32, table: u32) -> bool {
    let mut at = addr & 0x00FF_FFFF;
    for _ in 0..0x4000 {
        let Some(&p) = prev.get(&at) else {
            return false;
        };
        let full = 0x8000_0000 | p;
        if (table..table + 0x4000).contains(&full) {
            return true;
        }
        at = p;
    }
    false
}

/// The numeral cells the displayed frame drew: the older arena's quads
/// when the screen shows the older table ([`DisplayPhase::back`] `1`), and
/// those taken one more pass back - each corner stepped back by the older
/// pass's own move - when it shows the pass before it (`2`). The newer
/// arena is the one linked into the newest ordering table
/// ([`DisplayPhase::newest`]). A newer run on the rest row `32` stood
/// higher than the row lets it draw, so its step back is unknown and the
/// cells are not read (the age seat, [`popup_age_seat`], stands in).
pub fn displayed_popup_cells(ram: &[u8]) -> Option<Vec<DigitQuad>> {
    let (a, b) = arena_runs(ram)?;
    let phase = display_phase(ram);
    let table = phase.newest?;
    let len = ram.len().min(0x20_0000) & !3;
    let mut prev = std::collections::HashMap::new();
    for off in (0..len).step_by(4) {
        let w = u32::from_le_bytes(ram[off..off + 4].try_into().unwrap());
        let next = w & 0x00FF_FFFF;
        if next < 0x20_0000 && next != 0 && w >> 24 <= 0x10 {
            prev.insert(next, off as u32);
        }
    }
    let (older, newer) = match (
        linked_into(&prev, a[0].addr, table),
        linked_into(&prev, b[0].addr, table),
    ) {
        (true, false) => (b, a),
        (false, true) => (a, b),
        _ => return None,
    };
    if phase.back == 1 {
        return Some(older);
    }
    if newer.iter().any(|q| q.y <= 32) && older.iter().any(|q| q.y > 32) {
        return None;
    }
    Some(step_back(&older, &newer))
}

/// Each of the older run's cells taken one more pass back, by the move the
/// older pass made from the newer one's: corners and extent, bounded by the
/// rest row `32` and the 24-px cap as `FUN_801DF6B8` bounds the draw.
pub(super) fn step_back(older: &[DigitQuad], newer: &[DigitQuad]) -> Vec<DigitQuad> {
    older
        .iter()
        .zip(newer)
        .map(|(o, n)| DigitQuad {
            x: 2 * o.x - n.x,
            y: (2 * o.y - n.y).max(32),
            w: (2 * o.w - n.w).clamp(1, 24),
            ..*o
        })
        .collect()
}

/// `x:y:w:digit,...` for `LEGAIA_SEAT_POPUP_CELLS`.
pub fn popup_cells_to_env(cells: &[DigitQuad]) -> String {
    cells
        .iter()
        .map(|c| format!("{}:{}:{}:{}", c.x, c.y, c.w, c.digit))
        .collect::<Vec<_>>()
        .join(",")
}
