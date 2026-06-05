//! Tactical-Arts button-combo randomization.
//!
//! Each art's button combo (the directional sequence the player enters to fire
//! it) lives in the static `SCUS_942.54` arts-name table `DAT_80075EC4`: every
//! 20-byte record carries, at `+8`, a pointer to a glyph string
//! `[count][2-byte glyphs + one 0xFF06/0xFF09 separator marker]`. That glyph
//! string is the **sole** in-memory/on-disc representation of the combo —
//! there is no separate "command-byte record" (the long-standing "PROT 0x05C4
//! art records hold the matched bytes at +0x00" claim is falsified: 0x05C4 is
//! not even a valid PROT index, and the combos appear nowhere in RAM or on disc
//! as a contiguous direction run). Since the game matches combos and there is
//! only one copy, the matcher derives from this string, so editing it changes
//! both the gameplay trigger and the Arts-menu display. See
//! `docs/formats/art-data.md`.
//!
//! ## Why reassign pointers, not edit string bytes
//!
//! Identical combos are **deduplicated** across characters: Vahn's Cyclone and
//! Noa's Swan Driver point at the *same* `D U U U` string. Overwriting a shared
//! string's bytes would corrupt the other character's art. The safe lever is to
//! reassign each record's `+8` **pointer** (a same-size 4-byte write) so it
//! points at a *different existing* combo string — no byte edits, no relocation,
//! no sharing hazard.
//!
//! Vanilla combos are already unique *within* a character (they repeat only
//! across characters), so a within-character permutation preserves the
//! "each art is a unique combo" invariant by construction. The per-character
//! Miracle Art (record index 0, `0xFF09` marker, queue-clear trigger) is left
//! untouched.
//!
//! **Input count is preserved:** an art is only ever given a combo with the
//! same number of directions it started with (a 4-input art stays 4 inputs),
//! so each art's AP / available-spaces balance is kept. Reassignment therefore
//! happens within each character's per-length groups.
//!
//! - [`ArtsMode::Shuffle`] permutes each character's own combos among its
//!   same-length arts. (A length the character has only one art of can't
//!   shuffle, so that art keeps its combo.)
//! - [`ArtsMode::Random`] draws each art a same-length combo from the global
//!   pool of every regular art's combo, so a Vahn art can take a same-length
//!   combo that vanilla only Gala had.

use legaia_art::arts_table::{self, RawArtRecord};
use legaia_art::queue::{Character, Command};

use crate::rng::SplitMix64;

/// ISO 9660 file holding the arts-name table.
pub const SCUS_NAME: &str = "SCUS_942.54";

/// Shuffle (within-character) vs Random (from the global combo pool).
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

/// A planned `+8` pointer reassignment for one art record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtAssignment {
    pub character: Character,
    pub index: u8,
    /// File offset of the record's `+8` command pointer word.
    pub cmd_ptr_file_offset: usize,
    pub old_cmd_ptr: u32,
    pub new_cmd_ptr: u32,
    /// Decoded combo the new pointer resolves to.
    pub new_commands: Vec<Command>,
}

/// The arts-name table located inside `SCUS_942.54`, ready to plan + emit
/// same-size pointer patches.
pub struct ArtsEdits {
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
        Some(Self {
            records: arts_table::raw_records_from_scus(scus)?,
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

    /// Regular (randomizable) records for a character, in table order.
    fn regular(&self, ch: Character) -> Vec<&RawArtRecord> {
        self.records
            .iter()
            .filter(|r| r.character == ch && !r.is_miracle)
            .collect()
    }

    /// Distinct regular-art combos grouped by input count (combo length),
    /// deduplicated by pointer (the rodata strings are already deduplicated by
    /// combo, so one pointer == one distinct combo). A length's list always
    /// contains every character's own combos of that length, so it's never too
    /// small to fill a same-length class. Stable table order within a length.
    fn global_by_len(&self) -> std::collections::BTreeMap<usize, Vec<(u32, Vec<Command>)>> {
        let mut seen = std::collections::HashSet::new();
        let mut by_len: std::collections::BTreeMap<usize, Vec<(u32, Vec<Command>)>> =
            std::collections::BTreeMap::new();
        for r in &self.records {
            if r.is_miracle {
                continue;
            }
            if seen.insert(r.cmd_ptr) {
                by_len
                    .entry(r.commands.len())
                    .or_default()
                    .push((r.cmd_ptr, r.commands.clone()));
            }
        }
        by_len
    }

    /// Plan a per-character reassignment from a seed. Each character is planned
    /// independently, and **the input count is preserved**: an art is only ever
    /// given a combo with the same number of directions it started with, so a
    /// 4-input art stays 4 inputs (its AP / available-spaces balance is kept).
    /// The result reassigns every regular art a combo that's unique within its
    /// character.
    ///
    /// Reassignment happens within each character's per-length groups: an
    /// art's combo is only swapped with another combo of the same length. A
    /// length a character has only one art of can't shuffle (it stays put), but
    /// [`ArtsMode::Random`] can still re-combo it from another character's
    /// same-length combos.
    pub fn plan(&self, seed: u64, mode: ArtsMode) -> Vec<ArtAssignment> {
        let mut rng = SplitMix64::new(seed);
        let mut out = Vec::new();
        let global_by_len = self.global_by_len();
        for ch in Character::all() {
            // Group this character's regular arts by combo length, table order
            // preserved within each length.
            let mut by_len: std::collections::BTreeMap<usize, Vec<&RawArtRecord>> =
                std::collections::BTreeMap::new();
            for r in self.regular(ch) {
                by_len.entry(r.commands.len()).or_default().push(r);
            }
            for (len, group) in by_len {
                let k = group.len();
                // Same-length source combos for this group.
                let mut sources: Vec<(u32, Vec<Command>)> = match mode {
                    ArtsMode::Shuffle => {
                        let mut s: Vec<(u32, Vec<Command>)> = group
                            .iter()
                            .map(|r| (r.cmd_ptr, r.commands.clone()))
                            .collect();
                        rng.shuffle(&mut s);
                        // If a multi-art group landed on the identity, rotate so
                        // at least two arts actually swap.
                        if k >= 2
                            && s.iter()
                                .zip(group.iter())
                                .all(|((p, _), r)| *p == r.cmd_ptr)
                        {
                            s.rotate_left(1);
                        }
                        s
                    }
                    ArtsMode::Random => {
                        let mut pool = global_by_len.get(&len).cloned().unwrap_or_default();
                        rng.shuffle(&mut pool);
                        pool.truncate(k);
                        pool
                    }
                };
                // `pool` for Random always has >= k entries (it contains this
                // character's own k same-length combos), so `sources` is full;
                // guard the length anyway.
                debug_assert_eq!(sources.len(), k);
                for (rec, (new_ptr, new_commands)) in group.iter().zip(sources.drain(..)) {
                    out.push(ArtAssignment {
                        character: ch,
                        index: rec.index,
                        cmd_ptr_file_offset: rec.cmd_ptr_file_offset(),
                        old_cmd_ptr: rec.cmd_ptr,
                        new_cmd_ptr: new_ptr,
                        new_commands,
                    });
                }
            }
        }
        out
    }

    /// Turn a plan into `(scus_file_offset, le_u32_bytes)` pointer patches,
    /// dropping no-op assignments (new pointer equals current).
    pub fn pointer_patches(&self, plan: &[ArtAssignment]) -> Vec<(u64, [u8; 4])> {
        plan.iter()
            .filter(|a| a.new_cmd_ptr != a.old_cmd_ptr)
            .map(|a| (a.cmd_ptr_file_offset as u64, a.new_cmd_ptr.to_le_bytes()))
            .collect()
    }
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

    /// Synthesize a 3-art-per-character table sharing one combo across Vahn/Noa
    /// (the cross-character dedup the real table exhibits).
    fn synth() -> ArtsEdits {
        fn rec(off: usize, ch: Character, idx: u8, ptr: u32, cmds: Vec<Command>) -> RawArtRecord {
            RawArtRecord {
                record_file_offset: off,
                character: ch,
                index: idx,
                ap: 18,
                cmd_ptr: ptr,
                commands: cmds,
                is_miracle: idx == 0,
            }
        }
        ArtsEdits {
            records: vec![
                // Vahn: miracle idx0 + 3 regulars.
                rec(0x000, Character::Vahn, 0, 0xAAA0, vec![Right, Down, Left]),
                rec(0x014, Character::Vahn, 1, 0xBB00, vec![Left, Left, Down]),
                rec(0x028, Character::Vahn, 2, 0xBB10, vec![Up, Down, Up]),
                rec(0x03C, Character::Vahn, 3, 0xBB20, vec![Down, Up, Up, Up]), // shared
                // Noa: miracle idx0 + 3 regulars (one shares 0xBB20 with Vahn).
                rec(0x050, Character::Noa, 0, 0xAAA1, vec![Left, Up, Right]),
                rec(0x064, Character::Noa, 1, 0xCC00, vec![Right, Right, Left]),
                rec(0x078, Character::Noa, 2, 0xCC10, vec![Left, Right, Down]),
                rec(0x08C, Character::Noa, 3, 0xBB20, vec![Down, Up, Up, Up]), // shared ptr
            ],
        }
    }

    #[test]
    fn shuffle_keeps_each_characters_combo_multiset_and_uniqueness() {
        let a = synth();
        let plan = a.plan(0x1234, ArtsMode::Shuffle);
        for ch in [Character::Vahn, Character::Noa] {
            let assigns: Vec<&ArtAssignment> = plan.iter().filter(|p| p.character == ch).collect();
            assert_eq!(assigns.len(), 3, "3 regular arts per character");
            // Multiset of new combos == multiset of the character's own combos.
            let as_bytes = |c: &[Command]| c.iter().map(|d| d.as_byte()).collect::<Vec<u8>>();
            let mut new: Vec<Vec<u8>> = assigns.iter().map(|p| as_bytes(&p.new_commands)).collect();
            let mut want: Vec<Vec<u8>> = a
                .regular(ch)
                .iter()
                .map(|r| as_bytes(&r.commands))
                .collect();
            new.sort();
            want.sort();
            assert_eq!(new, want, "shuffle preserves the per-character combo set");
            // Unique within the character.
            let mut ptrs: Vec<u32> = assigns.iter().map(|p| p.new_cmd_ptr).collect();
            ptrs.sort_unstable();
            ptrs.dedup();
            assert_eq!(ptrs.len(), 3, "combos unique within the character");
        }
    }

    #[test]
    fn random_draws_distinct_combos_and_stays_unique() {
        let a = synth();
        let plan = a.plan(7, ArtsMode::Random);
        for ch in [Character::Vahn, Character::Noa] {
            let assigns: Vec<&ArtAssignment> = plan.iter().filter(|p| p.character == ch).collect();
            let mut ptrs: Vec<u32> = assigns.iter().map(|p| p.new_cmd_ptr).collect();
            ptrs.sort_unstable();
            ptrs.dedup();
            assert_eq!(ptrs.len(), assigns.len(), "unique within character");
        }
    }

    #[test]
    fn miracle_art_is_never_touched() {
        let a = synth();
        for mode in [ArtsMode::Shuffle, ArtsMode::Random] {
            let plan = a.plan(99, mode);
            assert!(
                plan.iter().all(|p| p.index != 0),
                "the index-0 Miracle Art must be excluded"
            );
        }
    }

    #[test]
    fn pointer_patches_are_four_bytes_le_and_skip_noops() {
        let a = synth();
        let plan = a.plan(0x55, ArtsMode::Shuffle);
        let patches = a.pointer_patches(&plan);
        // Every patch targets a record's +8 word.
        let valid: Vec<usize> = a
            .records
            .iter()
            .filter(|r| !r.is_miracle)
            .map(|r| r.cmd_ptr_file_offset())
            .collect();
        for (off, bytes) in &patches {
            assert!(valid.contains(&(*off as usize)), "patch hits a +8 word");
            assert_eq!(bytes.len(), 4);
        }
        // No-ops are dropped: every emitted patch genuinely changes the pointer.
        for a2 in plan.iter().filter(|p| p.new_cmd_ptr == p.old_cmd_ptr) {
            assert!(
                !patches
                    .iter()
                    .any(|(o, _)| *o == a2.cmd_ptr_file_offset as u64),
                "an unchanged pointer must not be patched"
            );
        }
    }

    #[test]
    fn every_art_keeps_its_input_count() {
        let a = synth();
        for mode in [ArtsMode::Shuffle, ArtsMode::Random] {
            let plan = a.plan(0xABCDE, mode);
            for p in &plan {
                let orig = a
                    .records
                    .iter()
                    .find(|r| r.character == p.character && r.index == p.index)
                    .unwrap();
                assert_eq!(
                    p.new_commands.len(),
                    orig.commands.len(),
                    "{:?} art {} changed input count under {mode:?}",
                    p.character,
                    p.index
                );
            }
        }
    }

    #[test]
    fn deterministic_for_a_fixed_seed() {
        let a = synth();
        assert_eq!(
            a.plan(0xC0FFEE, ArtsMode::Shuffle),
            a.plan(0xC0FFEE, ArtsMode::Shuffle)
        );
        assert_eq!(
            a.plan(0xC0FFEE, ArtsMode::Random),
            a.plan(0xC0FFEE, ArtsMode::Random)
        );
    }
}
