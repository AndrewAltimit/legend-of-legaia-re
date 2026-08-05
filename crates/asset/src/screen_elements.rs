//! The **screen-element placement table** at `SCUS_942.54` VA `0x80076C10` -
//! [`RECORD_COUNT`] records of `0x18` bytes, the seat book for the battle
//! screen's chrome. (The "200 records" figure this line used to open with is
//! the superseded one; [`RECORD_COUNT`] carries both bounds that cut it.)
//!
//! Three subsystems index this array and each named it after itself; the record
//! layout settles what it is
//! ([`memory-map.md`](../../../docs/reference/memory-map.md#0x80076c10---one-table-three-names)).
//! What this module adds is that the table is **disc data**: it lives in the
//! executable's data segment with every seat already filled in, and the runtime
//! writes back only a handful of fields per record - the measured content width
//! and the string pointer, plus a live seat while an element slides.
//!
//! ## A record is a content box, and the plate is derived from it
//!
//! `+0x08` reads `0x0C` in every initialised record, and a packet walk of a
//! live battle frame shows what the surrounding chrome does with the box:
//!
//! ```text
//! glyph pen = (x, y - 2)
//! plate     = (x - 8, y - 6),  size (w + 16, 20)
//! ```
//!
//! One arithmetic covers the actor-name plaque, the full-width party readout
//! and every command chip. The engine-side law + the sprite rects it feeds are
//! in `legaia_engine_vm::battle_chrome`; the surfaces are documented in
//! [`battle.md`](../../../docs/subsystems/battle.md).
//!
//! ## Record layout (`0x18` bytes)
//!
//! | Offset | Type | Field |
//! |---|---|---|
//! | `+0x00` | u16 | element id pair (two bytes, usually equal) |
//! | `+0x02` / `+0x04` | i16 | x / y |
//! | `+0x06` / `+0x08` | i16 | content width / line height (`0x0C` throughout) |
//! | `+0x0A` / `+0x0C` | i16 | the second seat - the pair `FUN_801D5778` offsets by a screen width |
//! | `+0x0E` | u16 | kind pair - two indices into [`crate::ui_widgets`] |
//! | `+0x14` | u32 | payload pointer - the string being measured, or null |
//!
//! The kind pair is not an opaque style tag: each byte is a **widget-class
//! record index**, and the record is the sprite (or chain of sprites) the
//! element is framed in. `0x01` is the blue plate body, `0x02` the carved-gold
//! one, `0x03`/`0x04` the corner-framed window, `0x07` a roster panel, `0x2B`
//! the active-actor bar. The two bytes are equal on all but a handful of
//! records, where the pair reads `(gold, blue)`.
//!
//! The two seats are a **from / to** pair for the element's slide, and which
//! one is live depends on the direction it last travelled: the actor-name
//! plaque parks at `y = -24` (above the screen) and lives at `y = 14`, while a
//! parked command chip stores the reverse.

/// RAM address of the table's record 0.
pub const TABLE_VA: u32 = 0x8007_6C10;
/// Bytes per record.
pub const RECORD_STRIDE: usize = 0x18;

/// Initialised placement records - `0x80076C10..0x800775B8`.
///
/// Not 200. Two independent bounds cut the run far shorter than a `200 x 0x18`
/// read: index **129** lands exactly on `0x80077828`, the per-monster steal
/// table ([`crate::steal_table::TABLE_VA`]), so no more than 129 records can
/// exist at this stride; and the placement shape itself stops at 103 - every
/// record below it keeps all four coordinates within `+/-416`, while record
/// 105 already carries `-1000`, and the pair at 103 / 104 reads as a VRAM
/// rect rather than a screen seat.
pub const RECORD_COUNT: usize = 103;

/// The first record index that would overlap [`crate::steal_table::TABLE_VA`].
/// A hard ceiling on any re-derivation of [`RECORD_COUNT`].
pub const STEAL_TABLE_RECORD_INDEX: usize = 129;

/// The line height `+0x08` carries on the **plate-run** records - the plaque,
/// the status bar and the command chips - and therefore the family
/// [`ScreenElement::plate_at`] applies to.
///
/// It is not table-wide: the roster-panel records carry `50`, the framed
/// windows `120` / `42` / `26` / `28`, and a few unused rows `0`.
pub const LINE_HEIGHT: i16 = 0x0C;

/// Record whose content box is the battle **actor-name plaque**. Seats are disc
/// data (`(16, 14)` live, `(16, -24)` parked); the width is measured from the
/// name at runtime and reads `0` on the disc.
pub const RECORD_NAME_PLAQUE: usize = 68;
/// Record whose content box is the full-width **active-actor status bar**
/// (`(16, 194)`, width 288).
pub const RECORD_ACTIVE_BAR: usize = 7;
/// Records whose boxes are the three per-member **roster panels** (`88 x 50`
/// at `(11 | 114 | 216, 170)`; the first is re-seated per party size).
pub const RECORDS_PARTY_PANEL: [usize; 3] = [6, 78, 79];
/// Records whose boxes are the per-actor **command chips**, in
/// up / left / right / down order (interior 48, around `(228, 70)`).
pub const RECORDS_COMMAND_CHIP: [usize; 4] = [8, 9, 10, 11];

/// PSX-EXE `t_addr` -> file-offset resolver for `SCUS_942.54`'s data segment.
pub(crate) struct ExeMap {
    t_addr: u32,
    t_size: u32,
}

impl ExeMap {
    pub(crate) fn parse(scus: &[u8]) -> Option<Self> {
        if scus.len() < 0x800 || &scus[0..8] != b"PS-X EXE" {
            return None;
        }
        let t_addr = u32::from_le_bytes(scus[0x18..0x1C].try_into().ok()?);
        let t_size = u32::from_le_bytes(scus[0x1C..0x20].try_into().ok()?);
        Some(Self { t_addr, t_size })
    }

    pub(crate) fn off(&self, va: u32) -> Option<usize> {
        if va < self.t_addr || va >= self.t_addr.checked_add(self.t_size)? {
            return None;
        }
        Some((va - self.t_addr) as usize + 0x800)
    }
}

/// File offset of the table's record 0 within a `SCUS_942.54` image.
pub fn table_file_offset(scus: &[u8]) -> Option<usize> {
    ExeMap::parse(scus)?.off(TABLE_VA)
}

/// One `0x18`-byte screen-element placement record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScreenElement {
    /// `+0x00` - element id pair.
    pub id: u16,
    /// `+0x02` / `+0x04`.
    pub seat: (i16, i16),
    /// `+0x06` - content width; `0` when the runtime measures it from a string.
    pub width: i16,
    /// `+0x08` - line height.
    pub height: i16,
    /// `+0x0A` / `+0x0C` - the second seat of the slide pair.
    pub alt_seat: (i16, i16),
    /// `+0x0E` - kind pair (chrome style).
    pub kind: u16,
    /// `+0x14` - payload pointer.
    pub payload: u32,
}

impl ScreenElement {
    /// Glyph pen for a content box seated at `seat`.
    pub const fn pen(&self) -> (i16, i16) {
        (self.seat.0, self.seat.1 - 2)
    }

    /// Glyph pen for the box seated at [`ScreenElement::alt_seat`].
    pub const fn alt_pen(&self) -> (i16, i16) {
        (self.alt_seat.0, self.alt_seat.1 - 2)
    }

    /// Plate rect `(x, y, w, h)` framing the box at `pen`, for a run whose
    /// interior is `width` pixels wide.
    pub const fn plate_at(pen: (i16, i16), width: i16) -> (i16, i16, i16, i16) {
        (pen.0 - 8, pen.1 - 4, width + 16, 20)
    }
}

/// The decoded table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScreenElementTable {
    records: Vec<ScreenElement>,
}

impl ScreenElementTable {
    /// Parse the table out of a `SCUS_942.54` image.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let map = ExeMap::parse(scus)?;
        let base = map.off(TABLE_VA)?;
        let mut records = Vec::with_capacity(RECORD_COUNT);
        for i in 0..RECORD_COUNT {
            let o = base + i * RECORD_STRIDE;
            let r = scus.get(o..o + RECORD_STRIDE)?;
            let u16at = |k: usize| u16::from_le_bytes([r[k], r[k + 1]]);
            let i16at = |k: usize| u16at(k) as i16;
            records.push(ScreenElement {
                id: u16at(0x00),
                seat: (i16at(0x02), i16at(0x04)),
                width: i16at(0x06),
                height: i16at(0x08),
                alt_seat: (i16at(0x0A), i16at(0x0C)),
                kind: u16at(0x0E),
                payload: u32::from_le_bytes([r[0x14], r[0x15], r[0x16], r[0x17]]),
            });
        }
        Some(Self { records })
    }

    /// Every record, in table order.
    pub fn records(&self) -> &[ScreenElement] {
        &self.records
    }

    /// Record `index`, if the table is that long.
    pub fn get(&self, index: usize) -> Option<ScreenElement> {
        self.records.get(index).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plate_derivation_matches_the_captured_surfaces() {
        // Content boxes read off the live table, plates read off the packets.
        let plaque = ScreenElement {
            seat: (16, 14),
            width: 63,
            ..Default::default()
        };
        assert_eq!(plaque.pen(), (16, 12));
        assert_eq!(ScreenElement::plate_at(plaque.pen(), 63), (8, 8, 79, 20));

        let bar = ScreenElement {
            seat: (16, 194),
            width: 288,
            ..Default::default()
        };
        assert_eq!(bar.pen(), (16, 192));
        assert_eq!(ScreenElement::plate_at(bar.pen(), 288), (8, 188, 304, 20));

        let chip = ScreenElement {
            seat: (204, 34),
            width: 48,
            ..Default::default()
        };
        assert_eq!(chip.pen(), (204, 32));
        assert_eq!(ScreenElement::plate_at(chip.pen(), 48), (196, 28, 64, 20));
    }

    #[test]
    fn rejects_a_non_exe() {
        assert!(ScreenElementTable::from_scus(b"not an exe").is_none());
        assert!(table_file_offset(b"not an exe").is_none());
    }
}
