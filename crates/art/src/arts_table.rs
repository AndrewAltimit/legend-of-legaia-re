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

/// One arts-table record with the raw editing metadata an in-place rewriter
/// needs: the record's own file offset, the `+8` command-glyph pointer, plus
/// the decoded view. The `+8` pointer is the lever the arts-combo randomizer
/// reassigns (the glyph string it points at is the SOLE in-memory/on-disc
/// representation of the art's button combo, shared/deduplicated across
/// characters - so reassigning the per-record pointer is safe where editing
/// the shared string bytes would not be).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawArtRecord {
    /// Byte offset of this 20-byte record inside the `SCUS_942.54` image.
    pub record_file_offset: usize,
    pub character: Character,
    /// Display index within the character's list (`0` = Miracle Art).
    pub index: u8,
    pub ap: u8,
    /// Virtual address stored in the record's `+8` command-glyph pointer.
    pub cmd_ptr: u32,
    /// Decoded directional command (glyph marker stripped).
    pub commands: Vec<Command>,
    /// `true` for the index-`0` Miracle Art row (or a row whose glyph string
    /// carries the `0xFF09` Miracle separator marker).
    pub is_miracle: bool,
}

impl RawArtRecord {
    /// File offset of the `+8` command-glyph pointer word itself (what an
    /// editor overwrites to reassign the combo).
    pub fn cmd_ptr_file_offset(&self) -> usize {
        self.record_file_offset + 8
    }
}

/// Parse the arts-name table into raw editing records (file offset + `+8`
/// pointer + decoded combo per record), up to the `(99, 99)` sentinel.
/// `None` if the image isn't a PSX-EXE or the table is out of range.
///
/// Sibling of [`parse_from_scus`] for tooling that must rewrite the table in
/// place rather than just read it.
pub fn raw_records_from_scus(scus: &[u8]) -> Option<Vec<RawArtRecord>> {
    let map = ExeMap::parse(scus)?;
    let mut out = Vec::new();
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
        let (commands, miracle_marker) =
            read_commands(scus, &map, cmd_ptr).unwrap_or((Vec::new(), false));
        out.push(RawArtRecord {
            record_file_offset: o,
            character,
            index: col,
            ap,
            cmd_ptr,
            commands,
            is_miracle: col == 0 || miracle_marker,
        });
    }
    Some(out)
}

/// A queryable view over the decoded arts-name table.
///
/// The SCUS table is the executable's **ground-truth** source for each art's
/// command sequence + AP. This wrapper is the validation oracle the
/// best-effort PROT `0x05C4` art-record parser ([`crate::parse::parse_record`])
/// and the curated `legaia-gamedata` `directions` column are checked against:
/// a decoded command sequence either resolves to a named art here or it
/// disagrees with the executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtsOracle {
    entries: Vec<ArtTableEntry>,
}

impl ArtsOracle {
    /// Build the oracle from a `SCUS_942.54` image. `None` if the image isn't
    /// a PSX-EXE or the table is out of range (see [`parse_from_scus`]).
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        Some(Self {
            entries: parse_from_scus(scus)?,
        })
    }

    /// Build directly from a decoded entry list (tests / non-SCUS callers).
    pub fn from_entries(entries: Vec<ArtTableEntry>) -> Self {
        Self { entries }
    }

    /// All decoded entries.
    pub fn entries(&self) -> &[ArtTableEntry] {
        &self.entries
    }

    /// Find an art by case-insensitive display-name match.
    pub fn by_name(&self, name: &str) -> Option<&ArtTableEntry> {
        let n = name.trim().to_ascii_lowercase();
        self.entries
            .iter()
            .find(|e| e.name.to_ascii_lowercase() == n)
    }

    /// Find the art whose decoded command sequence exactly matches
    /// `commands` for `character`. This is the contract a command decoder
    /// (the PROT `0x05C4` parser, or a player's live input) must satisfy:
    /// the bytes it produced map to exactly one named art. Empty sequences
    /// (the Miracle-art rows carry only the separator marker) never match.
    pub fn by_command(&self, character: Character, commands: &[Command]) -> Option<&ArtTableEntry> {
        if commands.is_empty() {
            return None;
        }
        self.entries
            .iter()
            .find(|e| e.character == character && e.commands == commands)
    }

    /// Find an art by `(character, display index)`.
    pub fn by_character_index(&self, character: Character, index: u8) -> Option<&ArtTableEntry> {
        self.entries
            .iter()
            .find(|e| e.character == character && e.index == index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(character: Character, index: u8, name: &str, commands: Vec<Command>) -> ArtTableEntry {
        ArtTableEntry {
            character,
            index,
            name: name.to_string(),
            ap: 0,
            is_miracle: index == 0,
            commands,
        }
    }

    #[test]
    fn oracle_resolves_a_decoded_command_sequence_to_one_art() {
        use Command::*;
        let oracle = ArtsOracle::from_entries(vec![
            entry(Character::Vahn, 1, "Power Punch", vec![Right, Right]),
            entry(Character::Vahn, 2, "Hyper Elbow", vec![Left, Right, Left]),
            entry(Character::Noa, 1, "Twin Cut", vec![Right, Right]),
        ]);

        // A parser that decodes [L,R,L] for Vahn must land on Hyper Elbow.
        let hit = oracle
            .by_command(Character::Vahn, &[Left, Right, Left])
            .expect("command resolves");
        assert_eq!(hit.name, "Hyper Elbow");

        // Same command bytes, different character -> different art.
        assert_eq!(
            oracle
                .by_command(Character::Noa, &[Right, Right])
                .map(|e| e.name.as_str()),
            Some("Twin Cut")
        );

        // A sequence no art uses doesn't resolve.
        assert!(oracle.by_command(Character::Vahn, &[Up, Up, Up]).is_none());
        // The empty Miracle-marker sequence never matches.
        assert!(oracle.by_command(Character::Vahn, &[]).is_none());

        // Name + index lookups.
        assert_eq!(oracle.by_name("hyper elbow").map(|e| e.index), Some(2));
        assert_eq!(
            oracle
                .by_character_index(Character::Noa, 1)
                .map(|e| e.name.as_str()),
            Some("Twin Cut")
        );
    }

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
