//! Arts-name table parser (`DAT_80075EC4` in `SCUS_942.54`).
//!
//! This is the static table the MES interpreter's `0xC5` substitution code
//! reads (see `docs/formats/art-data.md` and `docs/formats/mes.md`). It holds,
//! for every Tactical Art, the display **name**, **AP cost**, and the
//! **command-input glyph string** - the arrow sequence shown in the arts menu.
//! Decoding the glyph string recovers the real on-disc directional command for
//! each art, an independent source from (and validation oracle for) the
//! best-effort PROT `0x05C4` art-record parser in [`crate::parse`].
//!
//! ## Record layout (20 bytes, stride `0x14`, sorted by character)
//!
//! | Offset | Field |
//! |---|---|
//! | `+0` u8 | character (`0` Vahn, `1` Noa, `2` Gala) |
//! | `+1` u8 | art display index |
//! | `+2` u8 | AP cost |
//! | `+8` u32 | pointer to the command-glyph string |
//! | `+0xC` u32 | pointer to the name string |
//!
//! A `(99, 99)` record named `"End"` terminates the table. Each character's
//! index-`0` entry is the Miracle Art.
//!
//! ## Command-glyph string
//!
//! `[count u8]` followed by `count` two-byte glyph codes. The arrow glyphs map
//! to [`Command`] directions; a one-off `0xFF XX` marker (`0xFF06` regular /
//! `0xFF09` Miracle) separates the sequence and is not a direction:
//!
//! | Glyph | Direction |
//! |---|---|
//! | `0x81A9` | Left (Arms for Vahn/Gala, Ra-Seru for Noa) |
//! | `0x81A8` | Right |
//! | `0x81AB` | Down |
//! | `0x81AA` | Up |
//!
//! The codes encode the physical d-pad direction; the logical action
//! (Arms / Ra-Seru) depends on the character's handedness.

use crate::queue::{Character, Command};

/// RAM address of the arts-name table.
pub const TABLE_VA: u32 = 0x8007_5EC4;
/// Per-record stride.
pub const RECORD_STRIDE: usize = 0x14;
/// Row/col key of the terminating sentinel record.
pub const SENTINEL_KEY: u8 = 99;

/// One decoded arts-name-table entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtTableEntry {
    pub character: Character,
    /// Display index within the character's list (`0` = Miracle Art).
    pub index: u8,
    pub name: String,
    pub ap: u8,
    /// Directional command, glyph marker stripped. Empty for the Miracle Art
    /// rows when only the marker is present.
    pub commands: Vec<Command>,
    /// `true` for the per-character index-`0` Miracle Art row.
    pub is_miracle: bool,
}

/// Map a two-byte command glyph to a [`Command`]. Returns `None` for the
/// `0xFF XX` separator marker (and any unrecognised code).
pub fn glyph_to_command(hi: u8, lo: u8) -> Option<Command> {
    match (hi, lo) {
        (0x81, 0xA9) => Some(Command::Left),
        (0x81, 0xA8) => Some(Command::Right),
        (0x81, 0xAB) => Some(Command::Down),
        (0x81, 0xAA) => Some(Command::Up),
        _ => None,
    }
}

/// PSX-EXE `t_addr` → file-offset resolver. `SCUS_942.54` loads its data
/// segment at `t_addr` from file offset `0x800`.
struct ExeMap {
    t_addr: u32,
    t_size: u32,
}

impl ExeMap {
    fn parse(scus: &[u8]) -> Option<Self> {
        if scus.len() < 0x800 || &scus[0..8] != b"PS-X EXE" {
            return None;
        }
        let t_addr = u32::from_le_bytes(scus[0x18..0x1C].try_into().ok()?);
        let t_size = u32::from_le_bytes(scus[0x1C..0x20].try_into().ok()?);
        Some(Self { t_addr, t_size })
    }

    /// File offset for a virtual address, or `None` if outside the data
    /// segment.
    fn off(&self, va: u32) -> Option<usize> {
        if va < self.t_addr || va >= self.t_addr.checked_add(self.t_size)? {
            return None;
        }
        Some((va - self.t_addr) as usize + 0x800)
    }
}

/// Read an ASCII name string, stripping MES control prefixes (`0xCE XX [space]`
/// colour / character-name substitutions) and any other control bytes.
fn read_name(scus: &[u8], map: &ExeMap, va: u32) -> Option<String> {
    let start = map.off(va)?;
    let mut out = String::new();
    let mut i = start;
    while i < scus.len() {
        let b = scus[i];
        if b == 0 {
            break;
        }
        if b == 0xCE {
            // 0xCE + control byte (+ an optional trailing space).
            i += 2;
            if scus.get(i) == Some(&0x20) {
                i += 1;
            }
            continue;
        }
        if (0x20..0x7F).contains(&b) {
            out.push(b as char);
        }
        i += 1;
    }
    Some(out)
}

/// Read the command-glyph string into a direction sequence. Returns the
/// commands (marker stripped) and whether the separator was the Miracle
/// (`0xFF09`) marker.
fn read_commands(scus: &[u8], map: &ExeMap, va: u32) -> Option<(Vec<Command>, bool)> {
    let o = map.off(va)?;
    let count = *scus.get(o)? as usize;
    let mut cmds = Vec::with_capacity(count);
    let mut miracle_marker = false;
    for k in 0..count {
        let p = o + 1 + k * 2;
        let hi = *scus.get(p)?;
        let lo = *scus.get(p + 1)?;
        match glyph_to_command(hi, lo) {
            Some(c) => cmds.push(c),
            None => {
                if hi == 0xFF && lo == 0x09 {
                    miracle_marker = true;
                }
            }
        }
    }
    Some((cmds, miracle_marker))
}

fn character_for_row(row: u8) -> Option<Character> {
    match row {
        0 => Some(Character::Vahn),
        1 => Some(Character::Noa),
        2 => Some(Character::Gala),
        _ => None,
    }
}

/// Parse the arts-name table out of a `SCUS_942.54` image. Returns every art
/// record up to the `(99, 99)` sentinel. `None` if the image isn't a PSX-EXE
/// or the table address is out of range.
pub fn parse_from_scus(scus: &[u8]) -> Option<Vec<ArtTableEntry>> {
    let map = ExeMap::parse(scus)?;
    let mut out = Vec::new();
    // Defensive cap: the real table is 45 records + a sentinel.
    for i in 0..64usize {
        let rec_va = TABLE_VA + (i * RECORD_STRIDE) as u32;
        let o = map.off(rec_va)?;
        let rec = scus.get(o..o + RECORD_STRIDE)?;
        let row = rec[0];
        let col = rec[1];
        if row == SENTINEL_KEY {
            break;
        }
        let Some(character) = character_for_row(row) else {
            break;
        };
        let ap = rec[2];
        let cmd_ptr = u32::from_le_bytes(rec[8..0xC].try_into().ok()?);
        let name_ptr = u32::from_le_bytes(rec[0xC..0x10].try_into().ok()?);
        let name = read_name(scus, &map, name_ptr).unwrap_or_default();
        let (commands, miracle_marker) =
            read_commands(scus, &map, cmd_ptr).unwrap_or((Vec::new(), false));
        out.push(ArtTableEntry {
            character,
            index: col,
            name,
            ap,
            commands,
            is_miracle: col == 0 || miracle_marker,
        });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_map_is_a_bijection_over_the_four_arrows() {
        assert_eq!(glyph_to_command(0x81, 0xA9), Some(Command::Left));
        assert_eq!(glyph_to_command(0x81, 0xA8), Some(Command::Right));
        assert_eq!(glyph_to_command(0x81, 0xAB), Some(Command::Down));
        assert_eq!(glyph_to_command(0x81, 0xAA), Some(Command::Up));
        // The separator marker is not a direction.
        assert_eq!(glyph_to_command(0xFF, 0x06), None);
        assert_eq!(glyph_to_command(0xFF, 0x09), None);
    }

    #[test]
    fn non_psx_exe_returns_none() {
        assert!(parse_from_scus(b"not an exe").is_none());
        assert!(parse_from_scus(&[0u8; 0x900]).is_none());
    }
}
