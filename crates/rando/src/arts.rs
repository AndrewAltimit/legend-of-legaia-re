//! Tactical-Arts button-combo randomization.
//!
//! Each art's button combo (the directional sequence the player enters to fire
//! it) is a glyph string in the static `SCUS_942.54` arts-name table region:
//! `[count][2-byte direction glyphs + one 0xFF06/0xFF09 separator marker]`.
//! Two pointers reach each combo string — the arts-name record's `+8` (the
//! Arts-menu **display**) and its `+0x10` description pointer (the in-battle
//! input **matcher** reads the string that immediately follows the
//! description). Both read the **same bytes**, so the only edit that keeps the
//! display and the trigger in sync is to rewrite the **glyph bytes in place**.
//! (Moving the `+8` pointer changes only the menu arrows and desyncs the
//! trigger — a bug an emulator playtest caught.)
//!
//! ## Why a global content permutation is correct
//!
//! Identical combos are deduplicated across characters: a Noa art's `+8` can
//! point at a Vahn art's combo string. So the editable unit is the **distinct
//! combo string**, not the art. The randomizer permutes the *contents* of the
//! distinct combo strings within each length class. Because every character's
//! arts map to **distinct** strings (combos are unique within a character on
//! the retail disc), a bijection over the distinct strings keeps each
//! character's combos distinct by construction — so "each art is a unique combo
//! within its character" holds automatically, and the **input count is
//! preserved** (permutation stays within a length class). The per-character
//! Miracle Art (`0xFF09` marker) strings are excluded.
//!
//! - [`ArtsMode::Shuffle`] permutes the existing combos among same-length
//!   strings (every combo stays one the game shipped, so no new input ambiguity
//!   is introduced).
//! - [`ArtsMode::Random`] assigns each string a fresh random combo of the same
//!   length (distinct within the length class).

use std::collections::{BTreeMap, HashSet};

use legaia_art::arts_table::{self, RawArtRecord};
use legaia_art::queue::{Character, Command};

use crate::rng::SplitMix64;

/// ISO 9660 file holding the arts-name table.
pub const SCUS_NAME: &str = "SCUS_942.54";

/// Shuffle (permute existing combos) vs Random (fresh combos), same length.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArtsMode {
    Shuffle,
    Random,
}

/// One art's current combo, for the read-only listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentArt {
    pub character: Character,
    pub index: u8,
    pub ap: u8,
    /// Decoded directional combo (separator marker stripped).
    pub commands: Vec<Command>,
    pub is_miracle: bool,
}

/// A planned in-place rewrite of one distinct combo string's direction glyphs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComboEdit {
    /// Virtual address of the combo string (its `+8`/`+0x10` target).
    pub cmd_ptr: u32,
    /// File offsets of each direction glyph's 2-byte entry (marker excluded).
    pub direction_slots: Vec<usize>,
    pub old_directions: Vec<Command>,
    pub new_directions: Vec<Command>,
}

/// The arts-name table located inside `SCUS_942.54`, ready to plan + emit
/// same-size glyph-byte patches.
pub struct ArtsEdits {
    scus: Vec<u8>,
    records: Vec<RawArtRecord>,
}

impl ArtsEdits {
    /// Locate + decode the arts table from a whole disc image. `None` when
    /// `SCUS_942.54` isn't present or isn't a parseable PSX-EXE.
    pub fn locate(image: &[u8]) -> Option<Self> {
        let scus = legaia_iso::iso9660::read_file_in_image(image, SCUS_NAME)?;
        Self::from_scus(&scus)
    }

    /// Build directly from a `SCUS_942.54` image.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let records = arts_table::raw_records_from_scus(scus)?;
        Some(Self {
            scus: scus.to_vec(),
            records,
        })
    }

    /// All decoded records (Miracle rows included).
    pub fn records(&self) -> &[RawArtRecord] {
        &self.records
    }

    /// Every art's current combo, in table order.
    pub fn current(&self) -> Vec<CurrentArt> {
        self.records
            .iter()
            .map(|r| CurrentArt {
                character: r.character,
                index: r.index,
                ap: r.ap,
                commands: r.commands.clone(),
                is_miracle: r.is_miracle,
            })
            .collect()
    }

    /// The distinct **regular** combo strings, keyed by their virtual address,
    /// each with its editing layout. A miracle string (or one a regular record
    /// happens to share with a miracle) is excluded.
    fn distinct_strings(&self) -> BTreeMap<u32, arts_table::ComboStringLayout> {
        let mut out = BTreeMap::new();
        for r in &self.records {
            if r.is_miracle {
                continue;
            }
            if out.contains_key(&r.cmd_ptr) {
                continue;
            }
            if let Some(layout) = arts_table::combo_string_layout(&self.scus, r.cmd_ptr)
                && !layout.is_miracle
                && !layout.directions.is_empty()
            {
                out.insert(r.cmd_ptr, layout);
            }
        }
        out
    }

    /// Plan an in-place combo rewrite from a seed. Each distinct combo string
    /// keeps its input count; the result keeps every character's combos unique.
    pub fn plan(&self, seed: u64, mode: ArtsMode) -> Vec<ComboEdit> {
        let mut rng = SplitMix64::new(seed);
        let strings = self.distinct_strings();
        // Group distinct strings by direction count (length), ascending ptr for
        // determinism.
        let mut by_len: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
        for (ptr, layout) in &strings {
            by_len
                .entry(layout.directions.len())
                .or_default()
                .push(*ptr);
        }

        let mut edits = Vec::new();
        for (len, ptrs) in by_len {
            let current: Vec<Vec<Command>> =
                ptrs.iter().map(|p| strings[p].directions.clone()).collect();
            let new_seqs: Vec<Vec<Command>> = match mode {
                ArtsMode::Shuffle => {
                    let mut s = current.clone();
                    rng.shuffle(&mut s);
                    // Anti-identity: if a multi-string class landed unchanged,
                    // rotate so combos actually move.
                    if s.len() >= 2 && s == current {
                        s.rotate_left(1);
                    }
                    s
                }
                ArtsMode::Random => random_distinct_combos(&mut rng, len, ptrs.len()),
            };
            for (ptr, new) in ptrs.iter().zip(new_seqs) {
                let layout = &strings[ptr];
                edits.push(ComboEdit {
                    cmd_ptr: *ptr,
                    direction_slots: layout.direction_slots.clone(),
                    old_directions: layout.directions.clone(),
                    new_directions: new,
                });
            }
        }
        edits
    }

    /// Turn a plan into `(scus_file_offset, glyph_bytes)` 2-byte patches,
    /// dropping strings whose combo is unchanged. Each patch overwrites one
    /// direction glyph in place (the marker entry is never touched).
    pub fn glyph_patches(&self, plan: &[ComboEdit]) -> Vec<(u64, [u8; 2])> {
        let mut out = Vec::new();
        for e in plan {
            if e.new_directions == e.old_directions {
                continue;
            }
            debug_assert_eq!(e.direction_slots.len(), e.new_directions.len());
            for (slot, dir) in e.direction_slots.iter().zip(&e.new_directions) {
                out.push((*slot as u64, arts_table::command_to_glyph(*dir)));
            }
        }
        out
    }

    /// Number of distinct combo strings the plan changes.
    pub fn strings_changed(&self, plan: &[ComboEdit]) -> usize {
        plan.iter()
            .filter(|e| e.new_directions != e.old_directions)
            .count()
    }

    /// Number of **arts** the plan changes (a string serves one or more arts).
    pub fn arts_changed(&self, plan: &[ComboEdit]) -> usize {
        let changed: HashSet<u32> = plan
            .iter()
            .filter(|e| e.new_directions != e.old_directions)
            .map(|e| e.cmd_ptr)
            .collect();
        self.records
            .iter()
            .filter(|r| !r.is_miracle && changed.contains(&r.cmd_ptr))
            .count()
    }

    /// Total regular arts considered.
    pub fn regular_art_count(&self) -> usize {
        self.records.iter().filter(|r| !r.is_miracle).count()
    }
}

const DIRS: [Command; 4] = [Command::Left, Command::Right, Command::Down, Command::Up];

/// Generate `n` distinct random combos of length `len`.
fn random_distinct_combos(rng: &mut SplitMix64, len: usize, n: usize) -> Vec<Vec<Command>> {
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    let mut out = Vec::new();
    // 4^len distinct combos exist; n is always far smaller, so this terminates.
    while out.len() < n {
        let seq: Vec<Command> = (0..len).map(|_| DIRS[rng.below(4)]).collect();
        let key: Vec<u8> = seq.iter().map(|c| c.as_byte()).collect();
        if seen.insert(key) {
            out.push(seq);
        }
    }
    out
}

/// Format a combo as `"R D L"` for listings.
pub fn pretty_combo(commands: &[Command]) -> String {
    commands
        .iter()
        .map(|c| match c {
            Command::Left => "L",
            Command::Right => "R",
            Command::Down => "D",
            Command::Up => "U",
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use Command::*;

    /// Build a tiny PSX-EXE image with a hand-laid arts table + combo strings so
    /// the byte-level editing can be exercised without the disc. Layout:
    /// header (0x800) then data at t_addr; we place combo strings + records.
    fn synth_scus() -> Vec<u8> {
        // Real exe geometry (raw_records_from_scus reads the table at the fixed
        // VA 0x80075EC4). file = va - t_addr + 0x800.
        let t_addr: u32 = 0x8001_0000;
        let t_size: u32 = 0x0006_7000;
        let mut img = vec![0u8; 0x800 + t_size as usize];
        img[0..8].copy_from_slice(b"PS-X EXE");
        img[0x18..0x1C].copy_from_slice(&t_addr.to_le_bytes());
        img[0x1C..0x20].copy_from_slice(&t_size.to_le_bytes());
        let fo = |va: u32| (va - t_addr + 0x800) as usize;
        let g = arts_table::command_to_glyph;
        // Write a combo string [count][glyphs.. + marker] at `va`. `miracle`
        // selects the 0xFF09 (vs 0xFF06) separator the runtime keys on.
        let write_combo =
            |img: &mut [u8], va: u32, dirs: &[Command], marker_at: usize, miracle: bool| {
                let o = fo(va);
                let count = dirs.len() + 1;
                img[o] = count as u8;
                let mut p = o + 1;
                let mut di = 0;
                for k in 0..count {
                    if k == marker_at {
                        img[p] = 0xFF;
                        img[p + 1] = if miracle { 0x09 } else { 0x06 };
                    } else {
                        let gg = g(dirs[di]);
                        img[p] = gg[0];
                        img[p + 1] = gg[1];
                        di += 1;
                    }
                    p += 2;
                }
            };
        // Regular combo strings (VA, dirs, marker position), placed before the
        // arts table (which is fixed at 0x80075EC4).
        let combos: [(u32, Vec<Command>, usize); 5] = [
            (0x8007_4000, vec![Left, Left, Down], 0),         // len3 A
            (0x8007_4010, vec![Up, Down, Up], 1),             // len3 B (Somersault-like)
            (0x8007_4020, vec![Right, Right, Left, Down], 0), // len4
            (0x8007_4030, vec![Down, Up, Up, Up], 2),         // len4 (shared)
            (0x8007_4040, vec![Right, Down, Left, Down, Left], 0), // len5
        ];
        for (va, dirs, m) in &combos {
            write_combo(&mut img, *va, dirs, *m, false);
        }
        // Miracle string (0xFF09 marker) — must be excluded from edits.
        write_combo(
            &mut img,
            0x8007_4050,
            &[Right, Down, Left, Up, Left],
            0,
            true,
        );
        // Arts records at the table base (stride 0x14). char,idx,ap at +0..+3,
        // cmd_ptr at +8. We give Vahn 3 + Noa 3 regular arts (idx0 = miracle,
        // skipped). Noa idx10 SHARES Vahn idx12's len3-B string (cross-char dedup).
        let table = 0x8007_5EC4u32;
        let put = |img: &mut [u8], rec: usize, ch: u8, idx: u8, ap: u8, cmd: u32| {
            let o = fo(table + (rec as u32) * 0x14);
            img[o] = ch;
            img[o + 1] = idx;
            img[o + 2] = ap;
            img[o + 8..o + 12].copy_from_slice(&cmd.to_le_bytes());
            // name + aux ptrs left zero (fine for these tests).
        };
        // Vahn: miracle (0xFF09 string), then 3 regulars (len5, len4, len3-B).
        put(&mut img, 0, 0, 0, 99, 0x8007_4050);
        put(&mut img, 1, 0, 1, 50, 0x8007_4040);
        put(&mut img, 2, 0, 2, 40, 0x8007_4020);
        put(&mut img, 3, 0, 12, 18, 0x8007_4010);
        // Noa: miracle, then 3 regulars (len4-shared, len3-A, len3-B-shared).
        put(&mut img, 4, 1, 0, 99, 0x8007_4050);
        put(&mut img, 5, 1, 1, 70, 0x8007_4030);
        put(&mut img, 6, 1, 4, 50, 0x8007_4000);
        put(&mut img, 7, 1, 10, 24, 0x8007_4010); // shares Vahn idx12's string
        // Sentinel.
        let s = fo(table + 8 * 0x14);
        img[s] = 99;
        img[s + 1] = 99;
        img
    }

    fn edits() -> ArtsEdits {
        ArtsEdits::from_scus(&synth_scus()).expect("parse synth scus")
    }

    /// Apply a plan's glyph patches to a copy of the SCUS and return it.
    fn apply(e: &ArtsEdits, plan: &[ComboEdit]) -> Vec<u8> {
        let mut img = e.scus.clone();
        for (off, bytes) in e.glyph_patches(plan) {
            img[off as usize..off as usize + 2].copy_from_slice(&bytes);
        }
        img
    }

    fn art_combo(scus: &[u8], ch: Character, idx: u8) -> Vec<Command> {
        let recs = arts_table::raw_records_from_scus(scus).unwrap();
        let r = recs
            .iter()
            .find(|r| r.character == ch && r.index == idx)
            .unwrap();
        arts_table::combo_string_layout(scus, r.cmd_ptr)
            .unwrap()
            .directions
    }

    #[test]
    fn shuffle_preserves_lengths_and_within_character_uniqueness() {
        let e = edits();
        let plan = e.plan(0x1234, ArtsMode::Shuffle);
        let scus = apply(&e, &plan);
        for ch in [Character::Vahn, Character::Noa] {
            let recs = arts_table::raw_records_from_scus(&scus).unwrap();
            let combos: Vec<Vec<u8>> = recs
                .iter()
                .filter(|r| r.character == ch && !r.is_miracle)
                .map(|r| {
                    art_combo(&scus, ch, r.index)
                        .iter()
                        .map(|c| c.as_byte())
                        .collect()
                })
                .collect();
            // unique within character
            let set: HashSet<&Vec<u8>> = combos.iter().collect();
            assert_eq!(set.len(), combos.len(), "{ch:?} combos unique");
        }
        // lengths preserved for every art
        for ch in [Character::Vahn, Character::Noa] {
            for r in e
                .records
                .iter()
                .filter(|r| r.character == ch && !r.is_miracle)
            {
                assert_eq!(
                    art_combo(&scus, ch, r.index).len(),
                    r.commands.len(),
                    "{ch:?} idx{} length preserved",
                    r.index
                );
            }
        }
        assert!(e.strings_changed(&plan) > 0);
    }

    #[test]
    fn editing_bytes_updates_the_matchers_copy_not_just_a_pointer() {
        // The regression that motivated this: the combo BYTES (what the matcher
        // reads) must change, not a display pointer. Assert the on-disc glyph
        // bytes at a changed string differ from vanilla.
        let e = edits();
        let plan = e.plan(0x55, ArtsMode::Shuffle);
        let scus = apply(&e, &plan);
        let changed = plan
            .iter()
            .find(|p| p.new_directions != p.old_directions)
            .expect("some string changed");
        let layout = arts_table::combo_string_layout(&scus, changed.cmd_ptr).unwrap();
        assert_eq!(
            layout.directions, changed.new_directions,
            "the patched glyph bytes decode to the new combo"
        );
        assert_ne!(layout.directions, changed.old_directions);
    }

    #[test]
    fn shared_string_moves_both_arts_together_and_stays_unique() {
        // Vahn idx12 and Noa idx10 share a string. Both must change together to
        // the same new combo (no desync), each unique within its character.
        let e = edits();
        let plan = e.plan(7, ArtsMode::Shuffle);
        let scus = apply(&e, &plan);
        let vahn = art_combo(&scus, Character::Vahn, 12);
        let noa = art_combo(&scus, Character::Noa, 10);
        assert_eq!(vahn, noa, "shared string keeps both arts in sync");
    }

    #[test]
    fn random_keeps_lengths_and_uniqueness() {
        let e = edits();
        let plan = e.plan(99, ArtsMode::Random);
        let scus = apply(&e, &plan);
        for ch in [Character::Vahn, Character::Noa] {
            let mut combos: Vec<Vec<u8>> = Vec::new();
            for r in e
                .records
                .iter()
                .filter(|r| r.character == ch && !r.is_miracle)
            {
                let c = art_combo(&scus, ch, r.index);
                assert_eq!(c.len(), r.commands.len(), "length preserved");
                combos.push(c.iter().map(|x| x.as_byte()).collect());
            }
            let set: HashSet<&Vec<u8>> = combos.iter().collect();
            assert_eq!(set.len(), combos.len(), "{ch:?} unique");
        }
    }

    #[test]
    fn miracle_strings_are_never_patched() {
        let e = edits();
        for mode in [ArtsMode::Shuffle, ArtsMode::Random] {
            let plan = e.plan(3, mode);
            // The miracle combo string (0x80001050, 0xFF09 marker) is excluded
            // from distinct_strings, so no edit targets it.
            for edit in &plan {
                assert_ne!(edit.cmd_ptr, 0x8007_4050, "miracle string not edited");
            }
        }
    }

    #[test]
    fn deterministic_for_a_fixed_seed() {
        let e = edits();
        assert_eq!(
            e.plan(0xC0FFEE, ArtsMode::Shuffle),
            e.plan(0xC0FFEE, ArtsMode::Shuffle)
        );
        assert_eq!(
            e.plan(0xC0FFEE, ArtsMode::Random),
            e.plan(0xC0FFEE, ArtsMode::Random)
        );
    }
}
