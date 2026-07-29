//! **Place names carried by a scene MAN** - the two MAN-resident of the
//! three data sites a location name lives at on this disc.
//!
//! A place name reaches the screen from three independent carriers, and
//! editing one changes exactly one display:
//!
//! | Carrier | Drives | Shape |
//! |---|---|---|
//! | `SCUS_942.54` `0x80073B18` | quick-travel / Door-of-Wind destination list | 16 fixed `0x20`-byte cells ([`crate::worldmap_menu`]) |
//! | MAN **section 5** body | the names drawn over the world map at their map positions | `[u8 count][count x 0x20 record]` |
//! | MAN **section 2** body | the banner shown on entering the scene | bare `strlen + 1` NUL-terminated string |
//!
//! This module covers the two MAN-resident ones. Both are reached through
//! the section chain [`crate::man_section`] already walks, so neither needs
//! a byte hunt.
//!
//! ## Section 2 - the scene display name (`_DAT_801C6EA0`)
//!
//! `FUN_8003AEB0` installs section 2's body pointer into `_DAT_801C6EA0`.
//! The field overlay's scene-entry state machines latch it
//! (`_DAT_8007B44C = _DAT_801C6EA0` at `0x801EAC7C` / `0x801EE638` /
//! `0x801EEAE4`) and the string is drawn by the glyph renderer
//! `FUN_80036888` - pinned live by breaking on that renderer across an
//! overworld->town transition, where the banner draw arrives with
//! `a0 == _DAT_801C6EA0` (`scripts/pcsx-redux/autorun_location_banner_source.lua`).
//! The menu overlay draws the same latched pointer on the save screen
//! (`0x801E1D9C`) and copies `0x24` bytes of it into the save block's
//! location field (`0x801E1A28` -> `0x80084340`).
//!
//! The body is **not** padded: it is exactly `strlen + 1` bytes, so a
//! longer name needs the section resized ([`rewrite_scene_name`]).
//!
//! ## Section 5 - the world-map location table (`DAT_80073EE0`)
//!
//! Section 5 is the chain terminator (`length == 0`) in every scene MAN, so
//! its "body" is whatever trails the chain. In the three **kingdom** MANs
//! (`map01` / `map02` / `map03`) that trailing data is a location table, and
//! the pointer `DAT_80073EE0` the walker installs is what the world-map
//! label pass reads:
//!
//! ```text
//! DAT_80073EE0[0]            u8   record count
//! DAT_80073EE0[1 + i*0x20]:
//!   +0x00  u8   region        0 = Drake, 1 = Sebucus, 2 = Karisto
//!   +0x01  u8   map x         world coords = x << 7
//!   +0x02  u8   map y         world coords = y << 7
//!   +0x03  u16  discovery flag index (queried through FUN_8003CE64)
//!   +0x05  3    reserved, zero in the retail corpus
//!   +0x08  0x18 name, NUL-padded ASCII
//! ```
//!
//! The consumer is the label pass at `0x801CEBB6..0x801CEC30` in the field
//! overlay: for every record whose region matches the live kingdom and whose
//! discovery flag is set (or with the debug-all flag `_DAT_8007B868`), it
//! projects `(x << 7, y << 7)` to screen through `FUN_8003D368` and draws
//! `record + 8` with `FUN_80036888`, then underlines it with the measured
//! width. That is the "names overlaid where the places are" display.
//!
//! All three kingdom MANs carry a **byte-identical** copy of the whole
//! table - each one is filtered by `region` at draw time - so renaming a
//! place means editing every kingdom MAN that carries the record, not just
//! the kingdom it sits in.
//!
//! No Sony bytes are embedded here; the module only locates and rewrites
//! strings the user's own disc already holds.

use crate::man_section::{self, ManFile};

/// Section index whose body is the scene's display name (`_DAT_801C6EA0`).
pub const SCENE_NAME_SECTION: usize = 2;

/// Section index whose body is the world-map location table (`DAT_80073EE0`).
/// It is the chain terminator, so the table is the MAN's trailing data.
pub const WORLD_MAP_TABLE_SECTION: usize = 5;

/// Stride of one world-map location record.
pub const WORLD_MAP_RECORD_STRIDE: usize = 0x20;

/// Byte offset of the name field inside a world-map location record.
pub const WORLD_MAP_NAME_OFFSET: usize = 0x08;

/// Bytes the world-map record's name field holds (NUL-padded), so the
/// longest name it can carry is one less.
pub const WORLD_MAP_NAME_CAPACITY: usize = WORLD_MAP_RECORD_STRIDE - WORLD_MAP_NAME_OFFSET;

/// Sanity cap on the record count byte. Retail is 29; anything past this is
/// a mis-detected terminator rather than a table.
pub const WORLD_MAP_MAX_RECORDS: usize = 64;

/// The scene's display name (MAN section 2) located in a decompressed MAN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneName {
    /// Byte offset of the section's u24 length prefix.
    pub section_offset: usize,
    /// Byte offset of the string itself (`section_offset + 3`).
    pub body_offset: usize,
    /// Declared body length - `name.len() + 1` in the retail corpus.
    pub body_len: usize,
    /// The decoded name (up to the first NUL).
    pub name: String,
}

/// One record of the world-map location table (MAN section 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldMapLocation {
    /// Zero-based record index.
    pub index: usize,
    /// Byte offset of the record in the MAN buffer.
    pub record_offset: usize,
    /// Kingdom the marker belongs to (`0` Drake, `1` Sebucus, `2` Karisto);
    /// the label pass only draws records matching the live kingdom.
    pub region: u8,
    /// Map X (world coordinate = `map_x << 7`).
    pub map_x: u8,
    /// Map Y (world coordinate = `map_y << 7`).
    pub map_y: u8,
    /// Discovery-flag index; the label only draws once `FUN_8003CE64`
    /// reports the flag set.
    pub discovery_flag: u16,
    /// The decoded name (up to the first NUL).
    pub name: String,
}

impl WorldMapLocation {
    /// Byte offset of this record's name field.
    pub fn name_offset(&self) -> usize {
        self.record_offset + WORLD_MAP_NAME_OFFSET
    }
}

/// The world-map location table located in a decompressed kingdom MAN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldMapTable {
    /// Byte offset of the record-count byte (= section 5's body offset).
    pub count_offset: usize,
    /// The records, in table order.
    pub locations: Vec<WorldMapLocation>,
}

/// Read a NUL-terminated ASCII string out of `buf`, stopping at the first
/// NUL or at `max` bytes. `None` when any byte before the terminator is not
/// printable ASCII - which is how a Shift-JIS scene name (several ending
/// scenes still carry the untranslated Japanese one) is kept out of the
/// rename corpus rather than mangled.
fn read_ascii(buf: &[u8], offset: usize, max: usize) -> Option<String> {
    let slice = buf.get(offset..offset + max)?;
    let end = slice.iter().position(|&b| b == 0).unwrap_or(max);
    let text = &slice[..end];
    if text.iter().any(|&b| !(0x20..0x7F).contains(&b)) {
        return None;
    }
    Some(String::from_utf8_lossy(text).into_owned())
}

/// Locate the scene's display name (section 2) in a decompressed MAN.
///
/// `None` when the MAN doesn't walk, the section is empty, or its body is
/// not printable ASCII (the untranslated Shift-JIS ending scenes).
pub fn scene_name(man: &[u8]) -> Option<SceneName> {
    let parsed = man_section::parse(man).ok()?;
    scene_name_of(&parsed, man)
}

/// [`scene_name`] against an already-walked MAN.
pub fn scene_name_of(parsed: &ManFile, man: &[u8]) -> Option<SceneName> {
    let section = parsed.sections[SCENE_NAME_SECTION];
    let body_len = section.length as usize;
    if body_len < 2 || section.end_offset() > man.len() {
        return None;
    }
    let name = read_ascii(man, section.body_offset(), body_len)?;
    if name.is_empty() {
        return None;
    }
    Some(SceneName {
        section_offset: section.offset,
        body_offset: section.body_offset(),
        body_len,
        name,
    })
}

/// Locate the world-map location table (the section-5 trailer) in a
/// decompressed MAN. `None` for every MAN that isn't a kingdom bundle's -
/// their trailer is a single zero count byte.
pub fn world_map_table(man: &[u8]) -> Option<WorldMapTable> {
    let parsed = man_section::parse(man).ok()?;
    world_map_table_of(&parsed, man)
}

/// [`world_map_table`] against an already-walked MAN.
pub fn world_map_table_of(parsed: &ManFile, man: &[u8]) -> Option<WorldMapTable> {
    let terminator = parsed.sections[WORLD_MAP_TABLE_SECTION];
    if !terminator.is_terminator() {
        return None;
    }
    let count_offset = terminator.body_offset();
    let count = *man.get(count_offset)? as usize;
    if count == 0 || count > WORLD_MAP_MAX_RECORDS {
        return None;
    }
    let base = count_offset + 1;
    if base + count * WORLD_MAP_RECORD_STRIDE > man.len() {
        return None;
    }
    let mut locations = Vec::with_capacity(count);
    for index in 0..count {
        let record_offset = base + index * WORLD_MAP_RECORD_STRIDE;
        let name = read_ascii(
            man,
            record_offset + WORLD_MAP_NAME_OFFSET,
            WORLD_MAP_NAME_CAPACITY,
        )?;
        locations.push(WorldMapLocation {
            index,
            record_offset,
            region: man[record_offset],
            map_x: man[record_offset + 1],
            map_y: man[record_offset + 2],
            discovery_flag: u16::from_le_bytes([man[record_offset + 3], man[record_offset + 4]]),
            name,
        });
    }
    Some(WorldMapTable {
        count_offset,
        locations,
    })
}

/// Why a place-name rewrite was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceNameError {
    /// The name has a byte the dialog font can't render here.
    NotAscii,
    /// The name is empty (the banner would draw nothing).
    Empty,
    /// The name doesn't fit the target field.
    TooLong { len: usize, capacity: usize },
    /// The MAN doesn't carry the requested carrier.
    NoSuchCarrier,
}

impl std::fmt::Display for PlaceNameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAscii => write!(
                f,
                "name has non-ASCII bytes (the dialog font renders the ASCII set here)"
            ),
            Self::Empty => write!(f, "name is empty"),
            Self::TooLong { len, capacity } => {
                write!(f, "name is {len} bytes; the field holds at most {capacity}")
            }
            Self::NoSuchCarrier => write!(f, "this MAN carries no such place-name field"),
        }
    }
}

impl std::error::Error for PlaceNameError {}

/// Validate a replacement place name against a field capacity (the capacity
/// **includes** the terminating NUL).
pub fn validate_name(new_name: &str, capacity: usize) -> Result<(), PlaceNameError> {
    if new_name.is_empty() {
        return Err(PlaceNameError::Empty);
    }
    if !new_name.is_ascii() || new_name.bytes().any(|b| !(0x20..0x7F).contains(&b)) {
        return Err(PlaceNameError::NotAscii);
    }
    if new_name.len() + 1 > capacity {
        return Err(PlaceNameError::TooLong {
            len: new_name.len(),
            capacity: capacity - 1,
        });
    }
    Ok(())
}

/// Overwrite one world-map record's name **in place**. The record's name
/// field is a fixed 24 bytes, so this never changes the MAN's size; the
/// tail past the new name is zeroed so no stale bytes survive.
pub fn set_world_map_name(
    man: &mut [u8],
    location: &WorldMapLocation,
    new_name: &str,
) -> Result<(), PlaceNameError> {
    validate_name(new_name, WORLD_MAP_NAME_CAPACITY)?;
    let at = location.name_offset();
    let field = man
        .get_mut(at..at + WORLD_MAP_NAME_CAPACITY)
        .ok_or(PlaceNameError::NoSuchCarrier)?;
    field.fill(0);
    field[..new_name.len()].copy_from_slice(new_name.as_bytes());
    Ok(())
}

/// Rebuild a MAN with section 2's body replaced by `new_name`.
///
/// The section body is exactly `strlen + 1`, so a name of a different length
/// resizes the section. Nothing outside the chain has to be fixed up: the
/// partition record-offset tables and the header's `u24_at_28` both address
/// the region **before** section 0, and sections 3..5 are reached by walking
/// the chain (`next = section + 3 + length`), so rewriting section 2's
/// length prefix relocates every later section correctly by construction.
/// The caller must still rewrite the scene bundle's descriptor size word,
/// since the MAN's decompressed size is stored only there.
pub fn rewrite_scene_name(man: &[u8], new_name: &str) -> Result<Vec<u8>, PlaceNameError> {
    // Section 2 is not padded, so its capacity is whatever we make it; cap
    // at the tightest sibling carrier so one name can live at all three
    // sites.
    validate_name(new_name, WORLD_MAP_NAME_CAPACITY)?;
    let current = scene_name(man).ok_or(PlaceNameError::NoSuchCarrier)?;
    let body_len = new_name.len() + 1;
    let mut out = Vec::with_capacity(man.len() + body_len);
    out.extend_from_slice(&man[..current.section_offset]);
    out.extend_from_slice(&(body_len as u32).to_le_bytes()[..3]);
    out.extend_from_slice(new_name.as_bytes());
    out.push(0);
    out.extend_from_slice(&man[current.body_offset + current.body_len..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal MAN whose chain carries `scene` in section 2 and
    /// `places` as the section-5 trailer.
    fn synth(scene: &str, places: &[(u8, u8, u8, u16, &str)]) -> Vec<u8> {
        let mut man = vec![0u8; man_section::RECORDS_BEGIN_OFFSET];
        // No records in any partition, so the data region starts at 0x2B and
        // section 0 sits at its head.
        man[man_section::U24_AT_28_OFFSET] = 0;
        let push_section = |man: &mut Vec<u8>, body: &[u8]| {
            man.extend_from_slice(&(body.len() as u32).to_le_bytes()[..3]);
            man.extend_from_slice(body);
        };
        push_section(&mut man, &[0xAA; 4]); // 0 encounter
        push_section(&mut man, &[0xBB; 3]); // 1 motion
        let mut name = scene.as_bytes().to_vec();
        name.push(0);
        push_section(&mut man, &name); // 2 scene display name
        push_section(&mut man, &[0xCC; 5]); // 3 zone records
        push_section(&mut man, &[0xDD; 2]); // 4
        push_section(&mut man, &[]); // 5 terminator
        man.push(places.len() as u8);
        for &(region, x, y, flag, text) in places {
            let mut rec = [0u8; WORLD_MAP_RECORD_STRIDE];
            rec[0] = region;
            rec[1] = x;
            rec[2] = y;
            rec[3..5].copy_from_slice(&flag.to_le_bytes());
            rec[WORLD_MAP_NAME_OFFSET..WORLD_MAP_NAME_OFFSET + text.len()]
                .copy_from_slice(text.as_bytes());
            man.extend_from_slice(&rec);
        }
        man
    }

    #[test]
    fn reads_the_scene_display_name() {
        let man = synth("Rim Elm", &[]);
        let got = scene_name(&man).expect("section 2 name");
        assert_eq!(got.name, "Rim Elm");
        assert_eq!(got.body_len, 8);
    }

    #[test]
    fn reads_the_world_map_table() {
        let man = synth(
            "Drake Kingdom",
            &[
                (0, 93, 24, 0x0484, "Rim Elm"),
                (2, 64, 101, 0x049A, "Conkram"),
            ],
        );
        let table = world_map_table(&man).expect("section 5 trailer");
        assert_eq!(table.locations.len(), 2);
        assert_eq!(table.locations[0].name, "Rim Elm");
        assert_eq!(table.locations[0].region, 0);
        assert_eq!(table.locations[0].map_x, 93);
        assert_eq!(table.locations[0].discovery_flag, 0x0484);
        assert_eq!(table.locations[1].name, "Conkram");
        assert_eq!(table.locations[1].region, 2);
    }

    #[test]
    fn a_zero_count_trailer_is_not_a_table() {
        // The 90-scene corpus: only the three kingdom MANs carry records.
        let man = synth("Rim Elm", &[]);
        assert!(world_map_table(&man).is_none());
    }

    #[test]
    fn world_map_rename_is_same_size_and_zero_pads() {
        let mut man = synth("Drake Kingdom", &[(0, 93, 24, 0x0484, "Rim Elm")]);
        let before = man.len();
        let table = world_map_table(&man).unwrap();
        set_world_map_name(&mut man, &table.locations[0], "Elm").unwrap();
        assert_eq!(man.len(), before);
        let after = world_map_table(&man).unwrap();
        assert_eq!(after.locations[0].name, "Elm");
        // No stale tail from the longer previous name.
        let at = after.locations[0].name_offset();
        assert!(
            man[at + 3..at + WORLD_MAP_NAME_CAPACITY]
                .iter()
                .all(|&b| b == 0)
        );
    }

    #[test]
    fn scene_rename_resizes_the_section_and_keeps_the_chain() {
        let man = synth(
            "Rim Elm",
            &[
                (0, 93, 24, 0x0484, "Rim Elm"),
                (1, 58, 27, 0x048F, "Jeremi"),
            ],
        );
        let grown = rewrite_scene_name(&man, "Rim Elm Village").unwrap();
        assert_eq!(
            grown.len(),
            man.len() + ("Rim Elm Village".len() - "Rim Elm".len())
        );
        assert_eq!(scene_name(&grown).unwrap().name, "Rim Elm Village");
        // Every later section still walks, including the trailing table.
        let table = world_map_table(&grown).expect("trailer survived the resize");
        assert_eq!(table.locations.len(), 2);
        assert_eq!(table.locations[1].name, "Jeremi");
        // And shrinking round-trips back to the original bytes.
        let shrunk = rewrite_scene_name(&grown, "Rim Elm").unwrap();
        assert_eq!(shrunk, man);
    }

    #[test]
    fn refuses_empty_non_ascii_and_oversized_names() {
        let man = synth("Rim Elm", &[]);
        assert_eq!(rewrite_scene_name(&man, ""), Err(PlaceNameError::Empty));
        assert_eq!(
            rewrite_scene_name(&man, "Vïdna"),
            Err(PlaceNameError::NotAscii)
        );
        assert!(matches!(
            rewrite_scene_name(&man, &"x".repeat(WORLD_MAP_NAME_CAPACITY)),
            Err(PlaceNameError::TooLong { .. })
        ));
    }

    #[test]
    fn a_shift_jis_scene_name_is_not_offered_for_rename() {
        // Several ending scenes still carry the untranslated Japanese name;
        // reading them as ASCII would mangle the bytes, so they read as absent.
        let mut man = synth("x", &[]);
        let slot = scene_name(&man).unwrap();
        man[slot.body_offset] = 0x83; // Shift-JIS lead byte
        assert!(scene_name(&man).is_none());
    }
}
