//! Kingdom-bundle slot-5 CLUT-walk animation table.
//!
//! Slot 5 (the type-byte `0x06` slot) of each world-map kingdom bundle
//! (PROT 0085 `map01` / 0244 `map02` / 0391 `map03`) is an LZS-compressed
//! 516-byte table that drives the world map's palette-cell animations -
//! the rolling ocean waves plus the shoreline / river / terrain shimmer
//! cells. The decoded table is **byte-identical across the three
//! kingdoms** (the per-kingdom colours come from the parked source
//! strips, not from this table).
//!
//! ## Format
//!
//! ```text
//! +0x00  u32  count                 ; 8 in retail
//! +0x04  u32  entry_offsets[count]  ; relative to the table base
//! per entry:
//!   +0x00  u8   kind                ; 1 in every retail entry
//!   +0x01  u8   nframes
//!   +0x02  u16  cumulative_size     ; running total of entry bytes
//!                                   ; (header + frames) through this entry
//!   +0x04  u16  dest_x              ; VRAM halfword x of the 16x1 dest cell
//!   +0x06  u16  dest_y
//!   then nframes x 8-byte frames:
//!   +0x00  u8   0
//!   +0x01  u8   hold_vsyncs         ; accumulator threshold, in vsyncs
//!   +0x02  u16  0
//!   +0x04  u16  src_x               ; VRAM halfword x of the 16x1 source
//!   +0x06  u16  src_y               ; strip cell (park rows 498/501..505)
//! ```
//!
//! ## Runtime semantics
//!
//! The asset-type dispatcher `FUN_8001f05c` case 6 installs the decoded
//! table at `DAT_8007B7C8`; field init `FUN_801d6704` spawns one actor
//! per entry via `FUN_80024cfc`. The SCUS actor walker `FUN_8001ada4`
//! case 0xB steps each actor independently: `acc += dt` per game tick
//! (`dt` = the adaptive frame-step byte `DAT_1F800393`; overworld 3,
//! town 2), and when `acc >= frame.hold_vsyncs` it emits a libgpu
//! `MoveImage` of `RECT{src_x, src_y, 16, 1}` onto `(dest_x, dest_y)`,
//! zeroes the accumulator, and advances the frame index (wrapping at
//! `nframes`). The accumulator initialises to 100, so every entry's
//! first copy fires on the first game tick after scene entry. The real
//! repeat interval is therefore `ceil(hold_vsyncs / dt) * dt` vsyncs.
//!
//! Engine consumer: the `play-window` water animator
//! (`crates/engine-shell/src/bin/legaia-engine/window/field_render.rs`).

use crate::kingdom_bundle;

/// Kingdom-bundle slot index carrying the CLUT-walk table.
pub const KINGDOM_SLOT: u8 = 5;

/// Width (in VRAM halfwords) of every copy this table drives. The walker
/// emits fixed `16x1` rects; only the coordinates come from the table.
pub const COPY_WIDTH: u16 = 16;

/// Accumulator seed the runtime spawns each entry's actor with: `100`
/// exceeds every retail hold value, so the first copy fires immediately
/// at scene entry (`FUN_80024cfc` spawn path).
pub const ACCUMULATOR_SEED: u32 = 100;

/// One animation step: park-strip source cell + how long the previous
/// frame holds before this one is copied in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClutWalkFrame {
    /// Accumulator threshold in vsyncs (the `acc += DAT_1F800393` clock).
    pub hold_vsyncs: u8,
    /// VRAM halfword x of the 16x1 source strip cell.
    pub src_x: u16,
    /// VRAM y of the source strip cell (rows 498 / 501..505 in retail).
    pub src_y: u16,
}

/// One table entry = one independent walker actor: a fixed 16x1 CLUT
/// destination cell plus its frame loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClutWalkEntry {
    /// Entry kind byte; `1` in every retail entry.
    pub kind: u8,
    /// Running total of entry bytes (header + frames) through this entry,
    /// as stored on disc. Kept for structural validation.
    pub cumulative_size: u16,
    /// VRAM halfword x of the 16x1 destination CLUT cell.
    pub dest_x: u16,
    /// VRAM y of the destination CLUT cell.
    pub dest_y: u16,
    /// Frame loop, walked with wrap-around.
    pub frames: Vec<ClutWalkFrame>,
}

/// The decoded slot-5 table: eight independent walker entries in retail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClutWalkTable {
    pub entries: Vec<ClutWalkEntry>,
}

impl ClutWalkTable {
    /// The set of 16-halfword-wide destination cells this table rewrites,
    /// as `(row, col_start..col_start+16)`. This is the disc-derived
    /// ground truth behind the VRAM parity oracle's world-map CLUT-cycle
    /// exclusion set.
    pub fn dest_cells(&self) -> Vec<(u16, std::ops::Range<u16>)> {
        self.entries
            .iter()
            .map(|e| (e.dest_y, e.dest_x..e.dest_x + COPY_WIDTH))
            .collect()
    }
}

fn u16_at(buf: &[u8], off: usize) -> Result<u16, String> {
    buf.get(off..off + 2)
        .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| format!("truncated at 0x{off:X}"))
}

fn u32_at(buf: &[u8], off: usize) -> Result<u32, String> {
    buf.get(off..off + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| format!("truncated at 0x{off:X}"))
}

/// Parse a decoded slot-5 buffer (the 516-byte table in retail).
///
/// Structural validation only - counts, offsets, and per-entry bounds;
/// the frame padding bytes and the `kind` byte are surfaced, not
/// enforced, so a variant table still parses.
pub fn parse(buf: &[u8]) -> Result<ClutWalkTable, String> {
    let count = u32_at(buf, 0)? as usize;
    if count == 0 || count > 64 {
        return Err(format!("implausible entry count {count}"));
    }
    let mut entries = Vec::with_capacity(count);
    for k in 0..count {
        let off = u32_at(buf, 4 + k * 4)? as usize;
        if off + 8 > buf.len() {
            return Err(format!("entry {k} offset 0x{off:X} out of range"));
        }
        let kind = buf[off];
        let nframes = buf[off + 1] as usize;
        if nframes == 0 {
            return Err(format!("entry {k} has zero frames"));
        }
        let cumulative_size = u16_at(buf, off + 2)?;
        let dest_x = u16_at(buf, off + 4)?;
        let dest_y = u16_at(buf, off + 6)?;
        let mut frames = Vec::with_capacity(nframes);
        for f in 0..nframes {
            let fo = off + 8 + f * 8;
            if fo + 8 > buf.len() {
                return Err(format!("entry {k} frame {f} out of range"));
            }
            frames.push(ClutWalkFrame {
                hold_vsyncs: buf[fo + 1],
                src_x: u16_at(buf, fo + 4)?,
                src_y: u16_at(buf, fo + 6)?,
            });
        }
        entries.push(ClutWalkEntry {
            kind,
            cumulative_size,
            dest_x,
            dest_y,
            frames,
        });
    }
    Ok(ClutWalkTable { entries })
}

/// Decode a kingdom PROT entry buffer's slot 5 and parse it. The
/// convenience path the engine uses at scene resolve; errors if the
/// bundle has no locatable 7-asset table, the slot fails LZS decode, or
/// the decoded bytes don't parse as a CLUT-walk table.
pub fn from_kingdom_entry(entry: &[u8]) -> Result<ClutWalkTable, String> {
    let decoded = kingdom_bundle::decode_slot(entry, KINGDOM_SLOT)?;
    parse(&decoded)
}

/// A parked source strip from the kingdom bundle's slot-0 TIM_LIST: a
/// raw VRAM CLUT-block record (NOT a TIM - no `0x10` magic, so plain
/// TIM walkers skip it) that the retail loader `LoadImage`s verbatim.
/// These park the walk-table's per-kingdom source frames in the VRAM
/// rows the walker copies from (rows 498 / 499 / 502..505 in retail;
/// rows 500 / 501 / 506..509 arrive as ordinary TIM CLUTs instead).
#[derive(Clone, Debug)]
pub struct ParkStrip {
    /// VRAM halfword x of the strip.
    pub fb_x: u16,
    /// VRAM y of the strip.
    pub fb_y: u16,
    /// Width in halfwords (256 in retail).
    pub w: u16,
    /// Height in rows (1 in retail).
    pub h: u16,
    /// `w * h` BGR555 halfwords, little-endian.
    pub data: Vec<u8>,
}

/// Locate the parked CLUT strip records inside a decoded kingdom slot-0
/// buffer (the TIM_LIST pack the ocean tile also lives in).
///
/// Record shape (532 bytes in retail): `[u32, u32]` prefix (semantics
/// unpinned) + a bare PSX TIM CLUT block `[u32 blen][u16 x][u16 y]
/// [u16 w][u16 h][2*w*h bytes]`. Detection is structural: the block
/// length must equal `12 + 2*w*h`, the rect must land inside the CLUT
/// band (`y >= 448`), and the record must not carry the TIM magic.
pub fn park_strips(slot0: &[u8]) -> Vec<ParkStrip> {
    let mut strips = Vec::new();
    let Some(count) = slot0
        .get(0..4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
    else {
        return strips;
    };
    for k in 0..count {
        let Some(woff) = slot0
            .get(4 + k * 4..8 + k * 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
        else {
            break;
        };
        let bo = woff.saturating_mul(4);
        if bo + 20 > slot0.len() {
            continue;
        }
        let magic = u32::from_le_bytes(slot0[bo..bo + 4].try_into().unwrap());
        if magic == 0x10 {
            continue; // ordinary TIM; the TIM upload pass owns it
        }
        let blen = u32::from_le_bytes(slot0[bo + 8..bo + 12].try_into().unwrap()) as usize;
        let x = u16::from_le_bytes(slot0[bo + 12..bo + 14].try_into().unwrap());
        let y = u16::from_le_bytes(slot0[bo + 14..bo + 16].try_into().unwrap());
        let w = u16::from_le_bytes(slot0[bo + 16..bo + 18].try_into().unwrap());
        let h = u16::from_le_bytes(slot0[bo + 18..bo + 20].try_into().unwrap());
        let data_len = 2 * (w as usize) * (h as usize);
        if w == 0
            || h == 0
            || blen != 12 + data_len
            || !(448..512).contains(&y)
            || (x as usize) + (w as usize) > 1024
            || bo + 20 + data_len > slot0.len()
        {
            continue;
        }
        strips.push(ParkStrip {
            fb_x: x,
            fb_y: y,
            w,
            h,
            data: slot0[bo + 20..bo + 20 + data_len].to_vec(),
        });
    }
    strips
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal two-entry table in the disc layout.
    fn synthetic() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&2u32.to_le_bytes());
        // entry offsets: header = 4 + 2*4 = 12; entry0 at 12 (8 + 2*8 = 24
        // bytes), entry1 at 36.
        b.extend_from_slice(&12u32.to_le_bytes());
        b.extend_from_slice(&36u32.to_le_bytes());
        // entry 0: kind 1, 2 frames, cum 24, dest (0, 506)
        b.extend_from_slice(&[1, 2]);
        b.extend_from_slice(&24u16.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&506u16.to_le_bytes());
        for (hold, sx) in [(8u8, 0u16), (8, 16)] {
            b.extend_from_slice(&[0, hold]);
            b.extend_from_slice(&0u16.to_le_bytes());
            b.extend_from_slice(&sx.to_le_bytes());
            b.extend_from_slice(&505u16.to_le_bytes());
        }
        // entry 1: kind 1, 1 frame, cum 40, dest (48, 500)
        b.extend_from_slice(&[1, 1]);
        b.extend_from_slice(&40u16.to_le_bytes());
        b.extend_from_slice(&48u16.to_le_bytes());
        b.extend_from_slice(&500u16.to_le_bytes());
        b.extend_from_slice(&[0, 6]);
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&160u16.to_le_bytes());
        b.extend_from_slice(&498u16.to_le_bytes());
        b
    }

    #[test]
    fn parses_synthetic_table() {
        let t = parse(&synthetic()).expect("parse");
        assert_eq!(t.entries.len(), 2);
        assert_eq!(t.entries[0].dest_y, 506);
        assert_eq!(t.entries[0].frames.len(), 2);
        assert_eq!(t.entries[0].frames[1].src_x, 16);
        assert_eq!(t.entries[0].frames[1].src_y, 505);
        assert_eq!(t.entries[1].dest_x, 48);
        assert_eq!(t.entries[1].frames[0].hold_vsyncs, 6);
        assert_eq!(
            t.dest_cells(),
            vec![(506, 0..16), (500, 48..64)],
            "dest cells derive from the entries"
        );
    }

    #[test]
    fn rejects_truncated_and_empty() {
        assert!(parse(&[]).is_err());
        assert!(parse(&0u32.to_le_bytes()).is_err());
        let mut t = synthetic();
        t.truncate(20);
        assert!(parse(&t).is_err());
        // frame region truncated
        let mut t = synthetic();
        t.truncate(30);
        assert!(parse(&t).is_err());
    }

    #[test]
    fn park_strips_locates_clut_block_records() {
        // Pack: [count=2][word_offsets], entry 0 = a real-TIM-magic record
        // (skipped), entry 1 = a 4-entry CLUT-block strip at (0, 505).
        let mut b = Vec::new();
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&3u32.to_le_bytes()); // word offset 3 = byte 12
        b.extend_from_slice(&7u32.to_le_bytes()); // word offset 7 = byte 28
        // entry 0: TIM magic
        b.extend_from_slice(&0x10u32.to_le_bytes());
        b.extend_from_slice(&[0u8; 12]);
        // entry 1: [u32 a][u32 b][u32 blen=12+8][x=0][y=505][w=4][h=1][8 bytes]
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&20u32.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&505u16.to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&[0xAA; 8]);
        let strips = park_strips(&b);
        assert_eq!(strips.len(), 1);
        assert_eq!((strips[0].fb_x, strips[0].fb_y), (0, 505));
        assert_eq!((strips[0].w, strips[0].h), (4, 1));
        assert_eq!(strips[0].data, vec![0xAA; 8]);
    }

    #[test]
    fn rejects_zero_frame_entry() {
        let mut t = synthetic();
        t[13] = 0; // entry 0 nframes
        assert!(parse(&t).is_err());
    }
}
