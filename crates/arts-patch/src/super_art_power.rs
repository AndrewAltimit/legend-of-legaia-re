//! **Super Art** damage-power edits - the Tactical-Arts customization knob for
//! the five per-character combination finishers.
//!
//! ## Why the other arts knobs cannot reach a Super Art
//!
//! Every existing arts knob is keyed by something a Super Art does not have:
//!
//! - [`crate::arts`] (combo shuffle) and [`crate::arts_power`] address an art by
//!   its **input combo**. A Super Art is not entered as a combo at all - it is a
//!   find/replace over the finished action queue (`FUN_801EF9E4`), so its
//!   `record0` record carries no combo run at `+0`.
//! - [`crate::arts_ap_grant`] addresses an art by its **row in the static SCUS
//!   arts-name table** (`DAT_80075EC4`). That table holds exactly 45 records -
//!   15 per character - and none of them is a Super Art.
//! - There is no per-art AP cost to override in the first place: retail charges
//!   the *chain* arts and the Super itself is free (see
//!   [`docs/subsystems/arts-command-gauge.md`]).
//!
//! What a Super Art *does* have on disc is its own `0xD0`-stride art record, in
//! the same per-character array as every regular art, carrying its English name
//! at `+0x10` and its per-strike damage power bytes at `+0x24`. That record is
//! what this module edits.
//!
//! ## Locating a Super Art's record
//!
//! The `0xD0`-stride art array inside a player battle file's decoded `record0`
//! (PROT `0863` Vahn / `0864` Noa / `0865` Gala) is indexed by **action
//! constant**: the record for constant `c` sits at
//!
//! ```text
//! record_off = art_block_base + (c - GRID_BIAS) * ART_RECORD_STRIDE
//! ```
//!
//! with [`GRID_BIAS`] = `0x10`. The mapping is read off the queue-builder
//! `FUN_801EED1C`, which emits an art's constant as *row* `+ 0x18`
//! (`addiu v1,t3,0x18` / `sb v1,0x1df(v0)` at `0x801EF6F0`/`0x801EF6F8`), and the
//! array as enumerated by [`crate::arts_power::art_powers`] starts eight records
//! ahead of row 0 - so constant `0x19`/`0x1A` (the two Art Starters) land on the
//! two records named `"Starter"`, and constant `0x1B` (each character's Miracle
//! Art) on the record carrying that character's Miracle command string.
//!
//! Each Super Art's finisher constant is the `finisher` field of
//! [`legaia_art::SuperArt`], so the address is derived, not guessed - and it is
//! **self-checking**: the record's `+0x10` name is the Super Art's own English
//! name, byte-identical to `legaia_art::SUPER_ARTS`'s `name` for all fifteen
//! entries. [`super_art_powers`] returns a row only when that name matches, so a
//! wrong base or an unrecognized build yields nothing rather than a corrupt edit.
//!
//! ## Editing
//!
//! Same shape as [`crate::arts_power`]: decompress `record0`, overwrite each
//! currently-active power byte at `record + 0x24` (preserving the hit count, so a
//! non-hit slot is never promoted and a Super with no damage byte is left alone),
//! recompress to fit the original LZS footprint. No Sony bytes are added - only
//! power bytes already on the user's disc are rewritten.

use legaia_art::queue::Character;
use legaia_art::{SUPER_ARTS, SuperArt};

use crate::arts_power::{
    ART_RECORD_STRIDE, MAX_HITS, NAME_FIELD_OFF, POWER_FIELD_OFF, PowerEdit, active_power_len,
    find_records_by_combo, is_power_byte,
};

/// Difference between an art's **action constant** and its index in the
/// `0xD0`-stride record array as [`crate::arts_power::art_powers`] enumerates it.
pub const GRID_BIAS: u8 = 0x10;

/// PROT entry index of a character's player battle file (`record0` source).
pub fn player_entry_index(ch: Character) -> usize {
    crate::arts::player_entry_index(ch)
}

/// The five Super Arts of one character, in `legaia_art::SUPER_ARTS` order.
pub fn super_arts_for(ch: Character) -> Vec<&'static SuperArt> {
    SUPER_ARTS.iter().filter(|s| s.character == ch).collect()
}

/// One Super Art's on-disc record, located and name-validated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuperArtPower {
    pub character: Character,
    /// Display name (`legaia_art::SuperArt::name`), which the record's `+0x10`
    /// field reproduces byte-for-byte.
    pub name: &'static str,
    /// The finisher action constant this record is addressed by.
    pub finisher: u8,
    /// File offset of the record inside the decoded `record0`.
    pub record_off: usize,
    /// Active power bytes at `+0x24` (length = hit count, 0..=4).
    pub power: Vec<u8>,
}

impl SuperArtPower {
    /// `"0x16 0x16"`-style rendering of the active power bytes.
    pub fn power_str(&self) -> String {
        if self.power.is_empty() {
            return "-".to_string();
        }
        self.power
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Read the ASCII name field at `record_off + 0x10` (stops at the first byte
/// outside printable ASCII; the field is a fixed `0x10..0x24` slot).
fn record_name(dec: &[u8], record_off: usize) -> String {
    dec.get(record_off + NAME_FIELD_OFF..record_off + POWER_FIELD_OFF)
        .unwrap_or(&[])
        .iter()
        .take_while(|&&b| (0x20..0x7f).contains(&b))
        .map(|&b| b as char)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Base offset of the `0xD0`-stride art array in a decoded `record0`, solved
/// from the character's *regular* arts: each SCUS arts-name-table row `n` is
/// action constant `0x1B + n`, so a record found by its combo pins
/// `base = off - (0x1B + n - GRID_BIAS) * stride`. The modal candidate across
/// every regular art wins, so one art whose combo also occurs as data elsewhere
/// cannot move the answer.
pub fn art_block_base(scus: &[u8], dec: &[u8], ch: Character) -> Option<usize> {
    let rows = legaia_art::arts_table::raw_records_from_scus(scus)?;
    let mut votes: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
    for r in rows.iter().filter(|r| r.character == ch) {
        if r.commands.is_empty() {
            continue;
        }
        // Constant for display row n is 0x1B + n; its grid index is that minus
        // GRID_BIAS.
        let grid = (0x1Bu16 + u16::from(r.index)).checked_sub(u16::from(GRID_BIAS))? as usize;
        let span = grid.checked_mul(ART_RECORD_STRIDE)?;
        for off in find_records_by_combo(dec, &r.commands) {
            if let Some(base) = off.checked_sub(span) {
                *votes.entry(base).or_default() += 1;
            }
        }
    }
    // Highest vote count wins; ties break on the lowest base for determinism.
    votes
        .into_iter()
        .max_by_key(|&(base, n)| (n, std::cmp::Reverse(base)))
        .map(|(base, _)| base)
}

/// Locate every Super Art record of `ch` in a decoded `record0`. A row is
/// returned only when the record's `+0x10` name matches the Super Art's name,
/// so an unrecognized build silently yields fewer rows instead of a bad address.
pub fn super_art_powers_in(scus: &[u8], dec: &[u8], ch: Character) -> Vec<SuperArtPower> {
    let Some(base) = art_block_base(scus, dec, ch) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for s in super_arts_for(ch) {
        let Some(grid) = s.finisher.checked_sub(GRID_BIAS) else {
            continue;
        };
        let off = base + usize::from(grid) * ART_RECORD_STRIDE;
        if off + ART_RECORD_STRIDE > dec.len() {
            continue;
        }
        if !record_name(dec, off).eq_ignore_ascii_case(s.name) {
            continue;
        }
        let hits = active_power_len(dec, off);
        out.push(SuperArtPower {
            character: ch,
            name: s.name,
            finisher: s.finisher,
            record_off: off,
            power: dec[off + POWER_FIELD_OFF..off + POWER_FIELD_OFF + hits].to_vec(),
        });
    }
    out
}

/// [`super_art_powers_in`] straight off a raw player-file PROT entry.
pub fn super_art_powers(scus: &[u8], entry: &[u8], ch: Character) -> Option<Vec<SuperArtPower>> {
    let dec = crate::arts::player_record0_decoded(entry)?;
    Some(super_art_powers_in(scus, &dec, ch))
}

/// Resolve a user-typed Super Art name to its table entry. Matching ignores
/// case, spaces and punctuation, so `tri-somersault`, `TriSomersault` and
/// `Tri Somersault` all resolve. `character` narrows an otherwise ambiguous
/// name; the shipped fifteen names are all distinct, so it is normally
/// redundant.
pub fn find_super_art(name: &str, character: Option<Character>) -> Vec<&'static SuperArt> {
    let key = normalize_name(name);
    if key.is_empty() {
        return Vec::new();
    }
    SUPER_ARTS
        .iter()
        .filter(|s| character.is_none_or(|c| c == s.character))
        .filter(|s| normalize_name(s.name) == key)
        .collect()
}

/// Lowercase alphanumerics only - the comparison key for [`find_super_art`].
pub fn normalize_name(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Overwrite the active power bytes of the Super Arts named by `edits`
/// (`finisher -> new power value`) inside a decoded `record0`. Returns the edits
/// actually applied.
fn apply_super_power_edits(
    scus: &[u8],
    dec: &mut [u8],
    ch: Character,
    edits: &[(u8, u8)],
) -> Vec<PowerEdit> {
    let located = super_art_powers_in(scus, dec, ch);
    let mut applied = Vec::new();
    for (finisher, value) in edits {
        let Some(row) = located.iter().find(|r| r.finisher == *finisher) else {
            continue;
        };
        let hits = row.power.len();
        if hits == 0 {
            continue; // no damage byte to edit
        }
        let base = row.record_off + POWER_FIELD_OFF;
        let new_power = vec![*value; hits];
        if dec[base..base + hits] == new_power[..] {
            continue; // idempotent
        }
        let old_power = dec[base..base + hits].to_vec();
        dec[base..base + hits].copy_from_slice(&new_power);
        applied.push(PowerEdit {
            record_off: row.record_off,
            // A Super Art has no input combo; the empty vector is the honest
            // value and the report prints the name instead.
            combo: Vec::new(),
            old_power,
            new_power,
        });
    }
    applied
}

/// Rewrite the power bytes of the Super Arts named by `edits` inside a
/// player-data entry's `record0`, returning `(lzs_file_offset, recompressed,
/// applied)` to splice back, or `None` when `record0` cannot be decoded, nothing
/// matched, or the recompressed stream would not fit the original footprint.
///
/// Mirrors [`crate::arts_power::patch_player_record0_power`]'s decompress / edit
/// / recompress-to-fit flow.
pub fn patch_player_record0_super_power(
    scus: &[u8],
    entry: &[u8],
    ch: Character,
    edits: &[(u8, u8)],
) -> Option<(usize, Vec<u8>, Vec<PowerEdit>)> {
    let region = crate::arts::record0_lzs_region(entry)?;
    let mut decoded = legaia_lzs::decompress(entry.get(region.lzs_off..)?, region.budget).ok()?;
    let applied = apply_super_power_edits(scus, &mut decoded, ch, edits);
    if applied.is_empty() {
        return None;
    }
    let recompressed = legaia_lzs::compress(&decoded);
    if recompressed.len() > region.avail {
        return None;
    }
    Some((region.lzs_off, recompressed, applied))
}

/// `true` when `v` is an accepted power value: a damage tier (`0x0C..=0x1F`) or
/// `0`, which disables the strike.
pub fn is_accepted_power(v: u8) -> bool {
    v == 0 || is_power_byte(v)
}

/// Largest number of per-strike power bytes one record can carry.
pub const MAX_POWER_BYTES: usize = MAX_HITS;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_super_art_is_addressable_by_its_finisher() {
        // The whole addressing scheme is `grid = finisher - GRID_BIAS`, so every
        // shipped finisher must sit above the bias and inside a byte.
        for s in SUPER_ARTS {
            assert!(
                s.finisher > GRID_BIAS,
                "{}: finisher {:#04x} below the grid bias",
                s.name,
                s.finisher
            );
        }
    }

    #[test]
    fn each_character_has_five_supers_with_distinct_finishers() {
        for ch in Character::all() {
            let list = super_arts_for(ch);
            if list.is_empty() {
                continue; // Terra has no Tactical Arts
            }
            assert_eq!(list.len(), 5, "{ch:?} Super Art count");
            let mut fin: Vec<u8> = list.iter().map(|s| s.finisher).collect();
            fin.sort_unstable();
            fin.dedup();
            assert_eq!(fin.len(), 5, "{ch:?} finishers distinct");
        }
    }

    #[test]
    fn names_resolve_case_and_punctuation_insensitively() {
        for spelling in ["Tri-Somersault", "tri somersault", "TRISOMERSAULT"] {
            let hits = find_super_art(spelling, None);
            assert_eq!(hits.len(), 1, "{spelling:?} resolves uniquely");
            assert_eq!(hits[0].finisher, 0x2B);
            assert_eq!(hits[0].character, Character::Vahn);
        }
        assert!(find_super_art("Somersault", None).is_empty());
        assert!(find_super_art("", None).is_empty());
    }

    #[test]
    fn every_shipped_name_is_uniquely_resolvable() {
        // The CLI takes a bare name, so no two Super Arts may normalize alike.
        for s in SUPER_ARTS {
            assert_eq!(
                find_super_art(s.name, None).len(),
                1,
                "{} must resolve uniquely",
                s.name
            );
        }
    }

    #[test]
    fn character_filter_narrows_the_match() {
        assert!(find_super_art("Tri-Somersault", Some(Character::Vahn)).len() == 1);
        assert!(find_super_art("Tri-Somersault", Some(Character::Gala)).is_empty());
    }

    #[test]
    fn power_values_are_gated_to_tiers_or_zero() {
        assert!(is_accepted_power(0));
        assert!(is_accepted_power(0x0C));
        assert!(is_accepted_power(0x1F));
        assert!(!is_accepted_power(0x0B));
        assert!(!is_accepted_power(0x20));
    }

    /// Build a synthetic decoded `record0` holding a plausible art array so the
    /// locator + editor can be exercised without the disc: the regular arts sit
    /// at their constant-derived slots and Vahn's five Supers at theirs.
    fn synthetic(scus: &[u8]) -> (Vec<u8>, usize) {
        let base = 0x400usize;
        let mut dec = vec![0u8; base + 40 * ART_RECORD_STRIDE];
        dec[0] = 0xFF; // non-direction lead-in, so record 0 is a clean start
        let rows = legaia_art::arts_table::raw_records_from_scus(scus).unwrap();
        for r in rows.iter().filter(|r| r.character == Character::Vahn) {
            if r.commands.is_empty() {
                continue;
            }
            let grid = (0x1B + usize::from(r.index)) - usize::from(GRID_BIAS);
            let off = base + grid * ART_RECORD_STRIDE;
            for (i, c) in r.commands.iter().enumerate() {
                dec[off + i] = c.as_byte();
            }
            dec[off + r.commands.len()] = 0;
            dec[off + POWER_FIELD_OFF] = 0x18;
        }
        for s in super_arts_for(Character::Vahn) {
            let grid = usize::from(s.finisher - GRID_BIAS);
            let off = base + grid * ART_RECORD_STRIDE;
            dec[off] = 3; // the stub direction byte the retail records carry
            dec[off + 1] = 0;
            let nb = s.name.as_bytes();
            dec[off + NAME_FIELD_OFF..off + NAME_FIELD_OFF + nb.len()].copy_from_slice(nb);
            dec[off + POWER_FIELD_OFF] = 0x16;
            dec[off + POWER_FIELD_OFF + 1] = 0x1A;
        }
        (dec, base)
    }

    /// A minimal PSX-EXE carrying a real-shaped arts-name table for Vahn.
    fn synth_scus() -> Vec<u8> {
        let t_addr: u32 = 0x8001_0000;
        let t_size: u32 = 0x0006_7000;
        let mut img = vec![0u8; 0x800 + t_size as usize];
        img[0..8].copy_from_slice(b"PS-X EXE");
        img[0x18..0x1C].copy_from_slice(&t_addr.to_le_bytes());
        img[0x1C..0x20].copy_from_slice(&t_size.to_le_bytes());
        let fo = |va: u32| (va - t_addr + 0x800) as usize;
        let g = legaia_art::arts_table::command_to_glyph;
        // Three distinct combos at known VAs, then three arts pointing at them.
        let combos: [(u32, &[u8]); 3] = [
            (0x8007_4000, &[2, 3, 1, 3, 1]),
            (0x8007_4020, &[2, 2, 3, 1]),
            (0x8007_4040, &[4, 3, 4]),
        ];
        for (va, dirs) in combos {
            let o = fo(va);
            img[o] = dirs.len() as u8 + 1;
            let mut p = o + 1;
            img[p] = 0xFF;
            img[p + 1] = 0x06; // regular-art marker leads
            p += 2;
            for d in dirs {
                let gg = g(legaia_art::queue::Command::from_byte(*d).unwrap());
                img[p] = gg[0];
                img[p + 1] = gg[1];
                p += 2;
            }
        }
        let table = 0x8007_5EC4u32;
        let put = |img: &mut [u8], rec: usize, ch: u8, idx: u8, cmd: u32| {
            let o = fo(table + (rec as u32) * 0x14);
            img[o] = ch;
            img[o + 1] = idx;
            img[o + 2] = 24;
            img[o + 8..o + 12].copy_from_slice(&cmd.to_le_bytes());
        };
        put(&mut img, 0, 0, 1, 0x8007_4000);
        put(&mut img, 1, 0, 2, 0x8007_4020);
        put(&mut img, 2, 0, 12, 0x8007_4040);
        let s = fo(table + 3 * 0x14);
        img[s] = 99;
        img[s + 1] = 99;
        img
    }

    #[test]
    fn locator_solves_the_base_and_validates_by_name() {
        let scus = synth_scus();
        let (dec, base) = synthetic(&scus);
        assert_eq!(art_block_base(&scus, &dec, Character::Vahn), Some(base));
        let found = super_art_powers_in(&scus, &dec, Character::Vahn);
        assert_eq!(found.len(), 5, "all five Vahn Supers located");
        let tri = found.iter().find(|r| r.name == "Tri-Somersault").unwrap();
        assert_eq!(tri.finisher, 0x2B);
        assert_eq!(
            tri.record_off,
            base + usize::from(0x2Bu8 - GRID_BIAS) * ART_RECORD_STRIDE
        );
        assert_eq!(tri.power, vec![0x16, 0x1A]);
    }

    #[test]
    fn a_wrong_name_field_drops_the_row_instead_of_editing_it() {
        let scus = synth_scus();
        let (mut dec, base) = synthetic(&scus);
        // Corrupt Tri-Somersault's name field: the row must vanish, and the four
        // siblings must survive - a build check, not a crash.
        let off = base + usize::from(0x2Bu8 - GRID_BIAS) * ART_RECORD_STRIDE;
        dec[off + NAME_FIELD_OFF..off + NAME_FIELD_OFF + 4].copy_from_slice(b"Zzz.");
        let found = super_art_powers_in(&scus, &dec, Character::Vahn);
        assert_eq!(found.len(), 4);
        assert!(!found.iter().any(|r| r.name == "Tri-Somersault"));
        // ... and an edit aimed at it is a no-op rather than a wild write.
        let applied = apply_super_power_edits(&scus, &mut dec, Character::Vahn, &[(0x2B, 0x0C)]);
        assert!(applied.is_empty());
        assert_eq!(dec[off + POWER_FIELD_OFF], 0x16, "power byte untouched");
    }

    #[test]
    fn edits_preserve_the_hit_count_and_are_idempotent() {
        let scus = synth_scus();
        let (mut dec, base) = synthetic(&scus);
        let off = base + usize::from(0x2Bu8 - GRID_BIAS) * ART_RECORD_STRIDE;
        let applied = apply_super_power_edits(&scus, &mut dec, Character::Vahn, &[(0x2B, 0x0C)]);
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].old_power, vec![0x16, 0x1A]);
        assert_eq!(applied[0].new_power, vec![0x0C, 0x0C]);
        // Two active bytes only - the third slot is not promoted to a hit.
        assert_eq!(
            &dec[off + POWER_FIELD_OFF..off + POWER_FIELD_OFF + 3],
            &[0x0C, 0x0C, 0x00]
        );
        assert!(
            apply_super_power_edits(&scus, &mut dec, Character::Vahn, &[(0x2B, 0x0C)]).is_empty(),
            "re-applying the same value is a no-op"
        );
    }

    #[test]
    fn a_super_with_no_damage_byte_is_left_alone() {
        let scus = synth_scus();
        let (mut dec, base) = synthetic(&scus);
        let off = base + usize::from(0x2Cu8 - GRID_BIAS) * ART_RECORD_STRIDE;
        dec[off + POWER_FIELD_OFF] = 0;
        dec[off + POWER_FIELD_OFF + 1] = 0;
        let applied = apply_super_power_edits(&scus, &mut dec, Character::Vahn, &[(0x2C, 0x0C)]);
        assert!(applied.is_empty());
        assert_eq!(dec[off + POWER_FIELD_OFF], 0);
    }
}
