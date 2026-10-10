//! Tactical-Art **damage power** edits (an "arts power-down" / rebalance knob).
//!
//! A party member's Tactical Art does **not** take its damage from the
//! move-power table (that table is special-attack-only - see
//! [`docs/formats/move-power.md`]); it takes it from a **per-strike power byte**
//! baked into the character's art record. The damage kernel `FUN_801ec3e4`
//! reads the current strike's power byte `v` at `param_2[actor+0x1F4]`, then
//! computes the multiplier `mult = MULT[(v - 0xC) % 5]` with
//! `MULT = [12, 18, 20, 22, 28]` (the table at overlay 0898 VA `0x801F64EC`,
//! byte-verified) and picks the defence facet by `(v - 0xC) % 10 < 5` (UDF/high
//! else LDF/low). So the whole damage tier of an art strike is that one byte.
//!
//! ## Where the bytes live on disc (pinned)
//!
//! Each character's art records are a fixed `0xD0`-stride array inside the
//! **decoded `record0`** of that character's player battle file (PROT `0863`
//! Vahn / `0864` Noa / `0865` Gala - the same `record0` the arts-combo
//! randomizer edits, [`crate::arts`]). Reached at `record0[+0x58]`
//! (a block-relative offset resolved to the art block at load), a record is:
//!
//! ```text
//! +0x00  u8[]  combo (1=L,2=R,3=D,4=U), 0-terminated   (the matcher key)
//! +0x10  char  art name field (dev/JP name; absent -> zeros for regular arts)
//! +0x24  u8[3..=4]  DAMAGE POWER BYTES  <- what this module edits
//! ```
//!
//! The `+0x24` power bytes were pinned by decoding all three player files:
//! every art record carries 3 or 4 bytes in `0x0C..=0x1F` there, the values
//! ascend within a combo and track art strength (recognisable arts - Hurricane
//! Kick, Vulture Blade, Super Tempest, Heaven's Drop, Neo Static Raising - carry
//! sensible tiers), and the kernel's `MULT` table decodes them exactly as the
//! documented power encoding says. There is **no** display copy to keep in sync
//! (unlike the combo, whose menu arrows live in SCUS): the kernel reads only
//! these bytes.
//!
//! ## Editing
//!
//! An art is targeted by its **input combo** (`RDLDL`, `UDU`, ...). The editor
//! decompresses `record0`, finds the record whose `+0` combo matches (clean
//! start, 0-terminated, validated by in-range power bytes at `+0x24`), overwrites
//! each currently-active power byte with the new value (preserving the hit
//! count), and recompresses to fit the original LZS footprint. No Sony bytes: it
//! only rewrites power bytes already on the user's disc.

use legaia_art::queue::{Character, Command};

/// Byte offset of the power-byte array within a `0xD0`-stride art record.
pub const POWER_FIELD_OFF: usize = 0x24;
/// Byte offset of the art name field within a record.
pub const NAME_FIELD_OFF: usize = 0x10;
/// Fixed stride of the per-character art records inside the decoded `record0`.
pub const ART_RECORD_STRIDE: usize = 0xD0;
/// Max power bytes per art strike group.
pub const MAX_HITS: usize = 4;

/// The kernel's damage multiplier table (`0x801F64EC` in overlay 0898).
pub const MULT: [u8; 5] = [12, 18, 20, 22, 28];

/// `true` if `v` is a valid power-encoding byte (a real damage tier the kernel
/// decodes). `0x0C..=0x1F` map to a `(defence facet, multiplier)`; other values
/// deal no damage.
pub fn is_power_byte(v: u8) -> bool {
    (0x0C..=0x1F).contains(&v)
}

/// Decode a power byte into its `(defence_is_upper, multiplier)` tier, or `None`
/// when it isn't a damage byte.
pub fn power_tier(v: u8) -> Option<(bool, u8)> {
    if !is_power_byte(v) {
        return None;
    }
    let k = (v as usize - 0x0C) % 5;
    let upper = (v as usize - 0x0C) % 10 < 5;
    Some((upper, MULT[k]))
}

/// PROT entry index of a character's player battle file (`record0` source).
pub fn player_entry_index(ch: Character) -> usize {
    crate::arts::player_entry_index(ch)
}

/// One art record's power info, for listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtPower {
    /// File offset of the record start (`+0` combo) within the decoded `record0`.
    pub record_off: usize,
    /// Directional combo (`1=L,2=R,3=D,4=U`).
    pub combo: Vec<Command>,
    /// Embedded dev/JP name (empty for regular arts with no name field).
    pub name: String,
    /// The active power bytes at `+0x24` (length = hit count, 1..=4).
    pub power: Vec<u8>,
}

impl ArtPower {
    /// Combo rendered as `L/R/D/U` glyphs.
    pub fn combo_str(&self) -> String {
        self.combo.iter().map(command_glyph).collect()
    }
}

/// Map a directional command to its `L/R/D/U` glyph.
pub fn command_glyph(c: &Command) -> char {
    match c.as_byte() {
        1 => 'L',
        2 => 'R',
        3 => 'D',
        4 => 'U',
        _ => '?',
    }
}

/// Parse a combo token (`"RDLDL"`) into directional commands. Case-insensitive;
/// only `L/R/D/U` allowed. Returns `None` on any other character or empty input.
pub fn parse_combo(s: &str) -> Option<Vec<Command>> {
    if s.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(s.len());
    for ch in s.chars() {
        let b = match ch.to_ascii_uppercase() {
            'L' => 1,
            'R' => 2,
            'D' => 3,
            'U' => 4,
            _ => return None,
        };
        out.push(Command::from_byte(b)?);
    }
    Some(out)
}

fn is_dir(b: u8) -> bool {
    (1..=4).contains(&b)
}

/// Count the active (in-range) power bytes at `record_off + 0x24`, up to 4.
pub fn active_power_len(dec: &[u8], record_off: usize) -> usize {
    let base = record_off + POWER_FIELD_OFF;
    let mut n = 0;
    while n < MAX_HITS {
        match dec.get(base + n) {
            Some(&b) if is_power_byte(b) => n += 1,
            _ => break,
        }
    }
    n
}

/// File offsets of the art records whose `+0` combo equals `combo`: a clean-start
/// (preceding byte not a direction), 0-terminated run in the decoded `record0`.
/// A combo is unique per character, but a multi-level art (e.g. Noa's Hurricane
/// Kick) can carry several records with the same combo - all are returned.
pub fn find_records_by_combo(dec: &[u8], combo: &[Command]) -> Vec<usize> {
    if combo.is_empty() {
        return Vec::new();
    }
    let mut needle: Vec<u8> = combo.iter().map(|c| c.as_byte()).collect();
    needle.push(0);
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = dec[from..]
        .windows(needle.len())
        .position(|w| w == needle.as_slice())
    {
        let p = from + rel;
        if p == 0 || !is_dir(dec[p - 1]) {
            out.push(p);
        }
        from = p + 1;
    }
    out
}

/// Read the art record that starts at `off`: a 0-terminated `1..4` combo at `+0`
/// and its active power bytes at `+0x24`. `None` if `off` isn't a combo record.
fn read_record_at(dec: &[u8], off: usize) -> Option<ArtPower> {
    let mut j = off;
    while j < dec.len() && is_dir(dec[j]) {
        j += 1;
    }
    let comlen = j - off;
    if !(1..=9).contains(&comlen) || dec.get(j) != Some(&0) {
        return None;
    }
    let hits = active_power_len(dec, off);
    let combo: Vec<Command> = dec[off..j]
        .iter()
        .filter_map(|&b| Command::from_byte(b))
        .collect();
    let name: String = dec
        .get(off + NAME_FIELD_OFF..)
        .unwrap_or(&[])
        .iter()
        .take_while(|&&b| (0x20..0x7f).contains(&b))
        .map(|&b| b as char)
        .collect();
    let power = dec
        .get(off + POWER_FIELD_OFF..off + POWER_FIELD_OFF + hits)?
        .to_vec();
    Some(ArtPower {
        record_off: off,
        combo,
        name: name.trim().to_string(),
        power,
    })
}

/// Enumerate the art records in a decoded `record0` by locating each with a
/// clean-start combo that carries at least two power bytes, establishing the
/// per-character `0xD0` record grid, then walking the **contiguous** run of
/// records on that grid (so 1-hit and 0-hit records - including the Miracle Art -
/// are captured). This is the disc-only enumeration; the CLI listing joins the
/// SCUS arts table for names/AP/order (see [`labeled_art_powers`]).
pub fn art_powers(dec: &[u8]) -> Vec<ArtPower> {
    // Seeds: clean-start combos with >= 2 power bytes (strong art-record signal).
    let mut seeds: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while i < dec.len() {
        if is_dir(dec[i]) && (i == 0 || !is_dir(dec[i - 1])) {
            if read_record_at(dec, i).map(|r| r.power.len()) >= Some(2) {
                seeds.push(i);
            }
            while i < dec.len() && is_dir(dec[i]) {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    if seeds.is_empty() {
        return Vec::new();
    }
    // Dominant grid residue.
    let mut counts: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
    for &s in &seeds {
        *counts.entry(s % ART_RECORD_STRIDE).or_default() += 1;
    }
    let residue = *counts.iter().max_by_key(|&(_, &c)| c).unwrap().0;
    // Extend the contiguous run of valid combo records around each seed.
    let mut found: std::collections::BTreeMap<usize, ArtPower> = std::collections::BTreeMap::new();
    for &s in &seeds {
        if s % ART_RECORD_STRIDE != residue {
            continue;
        }
        // backward
        let mut off = s as isize;
        while off >= 0 {
            match read_record_at(dec, off as usize) {
                Some(rec) => {
                    found.insert(off as usize, rec);
                    off -= ART_RECORD_STRIDE as isize;
                }
                None => break,
            }
        }
        // forward
        let mut off = s + ART_RECORD_STRIDE;
        while let Some(rec) = read_record_at(dec, off) {
            found.insert(off, rec);
            off += ART_RECORD_STRIDE;
        }
    }
    found.into_values().collect()
}

/// One art's power info joined to its SCUS arts-table identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabeledArtPower {
    pub character: Character,
    /// Display index within the character's list (`0` = Miracle Art).
    pub index: u8,
    pub ap: u8,
    pub is_miracle: bool,
    pub combo: Vec<Command>,
    /// Record offsets (usually one; multi-level arts carry several).
    pub record_offs: Vec<usize>,
    /// Active power bytes of the first matching record (empty = no damage byte).
    pub power: Vec<u8>,
}

impl LabeledArtPower {
    pub fn combo_str(&self) -> String {
        self.combo.iter().map(command_glyph).collect()
    }
}

/// Join the SCUS arts table (identities: character, display index, AP, combo)
/// with the decoded `record0` power bytes for one character. Every art the SCUS
/// table lists for `ch` is returned, in table order, with the power bytes read
/// from its `record0` record at `+0x24`. `None` if either source can't be parsed.
pub fn labeled_art_powers(
    scus: &[u8],
    entry: &[u8],
    ch: Character,
) -> Option<Vec<LabeledArtPower>> {
    let dec = crate::arts::player_record0_decoded(entry)?;
    let recs = legaia_art::arts_table::raw_records_from_scus(scus)?;
    let mut out = Vec::new();
    for r in recs.iter().filter(|r| r.character == ch) {
        if r.commands.is_empty() {
            continue;
        }
        let offs = find_records_by_combo(&dec, &r.commands);
        let power = offs
            .first()
            .map(|&o| {
                let hits = active_power_len(&dec, o);
                dec[o + POWER_FIELD_OFF..o + POWER_FIELD_OFF + hits].to_vec()
            })
            .unwrap_or_default();
        out.push(LabeledArtPower {
            character: ch,
            index: r.index,
            ap: r.ap,
            is_miracle: r.is_miracle,
            combo: r.commands.clone(),
            record_offs: offs,
            power,
        });
    }
    Some(out)
}

/// List one character's art powers (unlabeled), decoding `record0` from the raw
/// player file entry. `None` if the entry can't be decoded.
pub fn list_from_entry(entry: &[u8]) -> Option<Vec<ArtPower>> {
    let dec = crate::arts::player_record0_decoded(entry)?;
    Some(art_powers(&dec))
}

/// A planned power edit: which record, and the before/after power bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PowerEdit {
    pub record_off: usize,
    pub combo: Vec<Command>,
    pub old_power: Vec<u8>,
    pub new_power: Vec<u8>,
}

/// Overwrite every active power byte of each record whose `+0` combo matches
/// one of `edits`' combos with that edit's `value`. Returns the edits actually
/// applied (records whose bytes changed). Preserves the hit count - only the
/// currently in-range power bytes at `+0x24` are touched, so a non-hit slot is
/// never promoted to a hit and an art with no damage byte is left untouched.
fn apply_power_edits(dec: &mut [u8], edits: &[(Vec<Command>, u8)]) -> Vec<PowerEdit> {
    let mut applied = Vec::new();
    for (combo, value) in edits {
        for off in find_records_by_combo(dec, combo) {
            let hits = active_power_len(dec, off);
            if hits == 0 {
                continue; // no damage byte to edit
            }
            let base = off + POWER_FIELD_OFF;
            let new_power = vec![*value; hits];
            if dec[base..base + hits] == new_power[..] {
                continue; // already set - keep idempotent
            }
            let old_power = dec[base..base + hits].to_vec();
            dec[base..base + hits].copy_from_slice(&new_power);
            applied.push(PowerEdit {
                record_off: off,
                combo: combo.clone(),
                old_power,
                new_power,
            });
        }
    }
    applied
}

/// Rewrite the power bytes of the arts named by `edits` (combo -> new power
/// value) inside a player-data entry's `record0`, returning
/// `(lzs_file_offset, recompressed_stream, applied_edits)` to splice back, or
/// `None` if `record0` can't be decoded, no combo matched, or the recompressed
/// stream wouldn't fit the original footprint.
///
/// Mirrors [`crate::arts::patch_player_record0`]'s decompress / edit /
/// recompress-to-fit flow.
pub fn patch_player_record0_power(
    entry: &[u8],
    edits: &[(Vec<Command>, u8)],
) -> Option<(usize, Vec<u8>, Vec<PowerEdit>)> {
    let ro = crate::arts::record0_lzs_region(entry)?;
    let mut decoded = legaia_lzs::decompress(entry.get(ro.lzs_off..)?, ro.budget).ok()?;
    let applied = apply_power_edits(&mut decoded, edits);
    if applied.is_empty() {
        return None;
    }
    let recompressed = legaia_lzs::compress(&decoded);
    if recompressed.len() > ro.avail {
        return None;
    }
    Some((ro.lzs_off, recompressed, applied))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(s: &str) -> Vec<Command> {
        parse_combo(s).unwrap()
    }

    /// Build a synthetic decoded `record0` with two art records on the 0xD0 grid.
    fn synthetic() -> Vec<u8> {
        let mut dec = vec![0u8; 0x60 + 3 * ART_RECORD_STRIDE];
        // A leading offset-table-ish header (non-combo) at 0x00 to force a
        // clean start at the first record.
        dec[0] = 0xFF;
        // record A at 0x60: combo "RDLDL", power [0x1d,0x19,0x1f,0x1a]
        put_record(
            &mut dec,
            0x60,
            &[2, 3, 1, 3, 1],
            b"Fiery Miyawaki",
            &[0x1d, 0x19, 0x1f, 0x1a],
        );
        // record B at 0x130: combo "RRDL", power [0x19,0x1a,0x1f]
        put_record(
            &mut dec,
            0x130,
            &[2, 2, 3, 1],
            b"Beatdunk",
            &[0x19, 0x1a, 0x1f],
        );
        dec
    }

    fn put_record(dec: &mut [u8], off: usize, combo: &[u8], name: &[u8], power: &[u8]) {
        dec[off..off + combo.len()].copy_from_slice(combo);
        dec[off + combo.len()] = 0; // terminator
        dec[off + NAME_FIELD_OFF..off + NAME_FIELD_OFF + name.len()].copy_from_slice(name);
        dec[off + POWER_FIELD_OFF..off + POWER_FIELD_OFF + power.len()].copy_from_slice(power);
    }

    #[test]
    fn parses_combos() {
        assert_eq!(
            cmd("RDLDL").iter().map(|c| c.as_byte()).collect::<Vec<_>>(),
            vec![2, 3, 1, 3, 1]
        );
        assert!(parse_combo("").is_none());
        assert!(parse_combo("XYZ").is_none());
    }

    #[test]
    fn power_tiers_decode() {
        assert_eq!(power_tier(0x1d), Some((false, 20))); // LDF x20
        assert_eq!(power_tier(0x19), Some((true, 22))); // UDF x22
        assert_eq!(power_tier(0x1a), Some((true, 28)));
        assert_eq!(power_tier(0x1f), Some((false, 28)));
        assert_eq!(power_tier(0x00), None);
    }

    #[test]
    fn enumerates_records() {
        let dec = synthetic();
        let recs = art_powers(&dec);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].combo_str(), "RDLDL");
        assert_eq!(recs[0].name, "Fiery Miyawaki");
        assert_eq!(recs[0].power, vec![0x1d, 0x19, 0x1f, 0x1a]);
        assert_eq!(recs[1].combo_str(), "RRDL");
        assert_eq!(recs[1].power, vec![0x19, 0x1a, 0x1f]); // 3 hits
    }

    #[test]
    fn edits_preserve_hit_count() {
        let mut dec = synthetic();
        let applied = apply_power_edits(&mut dec, &[(cmd("RRDL"), 0x0C)]);
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].old_power, vec![0x19, 0x1a, 0x1f]);
        assert_eq!(applied[0].new_power, vec![0x0C, 0x0C, 0x0C]);
        // Only 3 bytes touched; the 4th slot at +0x27 stays 0 (not promoted to a hit).
        assert_eq!(
            &dec[0x130 + POWER_FIELD_OFF..0x130 + POWER_FIELD_OFF + 4],
            &[0x0C, 0x0C, 0x0C, 0x00]
        );
        // Re-enumerate: still a 3-hit art, now all tier 0x0C.
        let recs = art_powers(&dec);
        let b = recs.iter().find(|r| r.combo_str() == "RRDL").unwrap();
        assert_eq!(b.power, vec![0x0C, 0x0C, 0x0C]);
    }

    #[test]
    fn edit_is_idempotent() {
        let mut dec = synthetic();
        let _ = apply_power_edits(&mut dec, &[(cmd("RDLDL"), 0x10)]);
        let again = apply_power_edits(&mut dec, &[(cmd("RDLDL"), 0x10)]);
        assert!(again.is_empty(), "re-applying the same value is a no-op");
    }

    #[test]
    fn unknown_combo_no_edit() {
        let mut dec = synthetic();
        let applied = apply_power_edits(&mut dec, &[(cmd("UUUU"), 0x10)]);
        assert!(applied.is_empty());
    }
}
