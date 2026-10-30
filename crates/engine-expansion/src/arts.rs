//! Pack Tactical Arts at runtime: the compiled arts, the queue bytes their
//! clips ride, the bank-walk extension the arts input matches them with, and
//! the chain pass.
//!
//! # The byte space
//!
//! Retail's arts queue (`actor[+0x1DF..]`) holds swings `0x0C..=0x0F`, the
//! starters `0x19` / `0x1A` and the art constants `0x1B..=0x32`; the anim
//! commit turns any byte `>= 0x10` into art-bank record `byte - 0x10`. A pack
//! art's segments take the bytes above retail's last constant, `0x33..=0x7F`
//! ([`FIRST_PACK_BYTE`]), allotted per character in key order, and the
//! battle form appends a clip per byte to the character's art bank: a copy of
//! the segment's retail clip with the pack's power bytes and beats. The
//! queue's mark bit `0x80` stays clear.
//!
//! # Matching
//!
//! A pack art is one more **normal** record of the character's bank walk
//! (`FUN_801EED1C`'s normalisation loop, `legaia_art::tokenize_bank`): its
//! row sits at the index of its first byte, past retail's 24 rows, so the
//! walk tries every retail record before it at each start and a retail art
//! still matches exactly as it did. The walk writes the `0x19` starter over
//! the run's last arrow and the art's first byte after it; [`expand_segments`]
//! then inserts the art's further segment bytes behind it.
//!
//! With no pack art known for the character the extension is empty and the
//! walk is retail's own.

use std::collections::{BTreeMap, BTreeSet};

use legaia_expansion_format::arts::{ArtLink, ArtSection, Learn, Rider};
use legaia_expansion_format::items::Character;

/// The first queue byte a pack art clip takes: one past the last art
/// constant (`0x32`).
pub const FIRST_PACK_BYTE: u8 = 0x33;
/// The last: below the queue's mark bit.
pub const LAST_PACK_BYTE: u8 = 0x7F;
/// The retail rows of a bank walk (`0x1B..=0x32`).
pub const RETAIL_ROWS: usize = 24;
/// The SpecialStarter a chain finisher plays behind.
pub const SPECIAL_STARTER: u8 = 0x1A;
/// The regular starter.
pub const STARTER: u8 = 0x19;
/// The starter mark that raises the `SUPER` banner on a SpecialStarter
/// commit (`legaia_engine_battle_vm::battle_action::SUPER_STARTER_MARK`).
pub const SUPER_MARK: u32 = 4;

/// The roster slot of a pack character.
pub fn roster_of(c: Character) -> u8 {
    c.slot()
}

/// One compiled segment: the retail clip it borrows and the pack's numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledSegment {
    /// The retail art constant whose clip plays (`0x1B..=0x32`).
    pub base: u8,
    /// The queue byte this segment rides.
    pub byte: u8,
    /// Replacement power bytes (empty keeps the clip's), at most four.
    pub power: Vec<u8>,
    /// Replacement beat frames (empty keeps the clip's).
    pub beats: Vec<u8>,
}

impl CompiledSegment {
    /// The four power bytes the clip's entry head carries once `clip`'s own
    /// run is overlaid: a pack byte where the segment names one, the clip's
    /// elsewhere.
    pub fn power_run(&self, clip: [u8; 4]) -> [u8; 4] {
        let mut out = clip;
        for (d, s) in out.iter_mut().zip(&self.power) {
            *d = *s;
        }
        out
    }

    /// The four beat frames (zero-terminated) over the clip's own.
    pub fn beat_run(&self, clip: [u8; 4]) -> [u8; 4] {
        if self.beats.is_empty() {
            return clip;
        }
        let mut out = [0u8; 4];
        for (d, s) in out.iter_mut().zip(&self.beats) {
            *d = *s;
        }
        out
    }
}

/// One compiled pack art.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledArt {
    /// The local key.
    pub key: String,
    pub character: Character,
    pub name: String,
    pub description: String,
    /// Command bytes `1..=4`; empty for a chain-only finisher.
    pub commands: Vec<u8>,
    pub ap: u8,
    pub segments: Vec<CompiledSegment>,
    pub damage_pct: u16,
    pub element: Option<u8>,
    pub rider: Option<Rider>,
    pub learn: Learn,
}

impl CompiledArt {
    /// The byte the bank walk writes for the art: its first segment's.
    pub fn first_byte(&self) -> u8 {
        self.segments.first().map_or(0, |s| s.byte)
    }

    /// The bytes the art occupies in a queue, in order.
    pub fn bytes(&self) -> impl Iterator<Item = u8> + '_ {
        self.segments.iter().map(|s| s.byte)
    }
}

/// A compiled chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledChain {
    pub key: String,
    pub character: Character,
    /// The arts the turn must end with: retail constants, or pack arts as
    /// their first byte.
    pub links: Vec<u8>,
    /// The finisher's index in [`CompiledArts::arts`].
    pub finisher: usize,
}

/// A pack's arts, compiled.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompiledArts {
    pub arts: Vec<CompiledArt>,
    pub chains: Vec<CompiledChain>,
}

impl CompiledArts {
    /// Compile `section`. Arts whose segments do not resolve, or that run
    /// past the character's byte budget, are left out (the validator reports
    /// both); a chain naming a missing art is dropped.
    pub fn compile(pack_id: &str, section: &ArtSection) -> Self {
        let mut next: BTreeMap<Character, u8> = BTreeMap::new();
        let mut arts = Vec::new();
        for (key, a) in &section.arts {
            let Some(commands) = a.command_bytes() else {
                continue;
            };
            let bases: Option<Vec<u8>> = a
                .segments
                .iter()
                .map(|s| s.art.constant(a.character))
                .collect();
            let Some(bases) = bases.filter(|b| !b.is_empty()) else {
                continue;
            };
            let first = *next.entry(a.character).or_insert(FIRST_PACK_BYTE);
            let Some(last) = first.checked_add(bases.len() as u8 - 1) else {
                continue;
            };
            if last > LAST_PACK_BYTE {
                continue;
            }
            next.insert(a.character, last + 1);
            let segments = a
                .segments
                .iter()
                .zip(bases)
                .enumerate()
                .map(|(i, (s, base))| CompiledSegment {
                    base,
                    byte: first + i as u8,
                    power: s.power.iter().filter_map(|p| p.byte()).take(4).collect(),
                    beats: s.beats.iter().copied().take(4).collect(),
                })
                .collect();
            arts.push(CompiledArt {
                key: key.clone(),
                character: a.character,
                name: a.name.trim().to_string(),
                description: a.description.clone(),
                commands,
                ap: a.ap.min(legaia_expansion_format::arts::MAX_AP),
                segments,
                damage_pct: a.damage_pct.max(1),
                element: a.element,
                rider: a.rider,
                learn: a.learn.clone(),
            });
        }
        let index = |k: &str| {
            let k = legaia_expansion_format::battle::local_key(pack_id, k)?;
            arts.iter().position(|a: &CompiledArt| a.key == k)
        };
        let mut chains = Vec::new();
        for (key, c) in &section.chains {
            let Some(finisher) = index(&c.finisher) else {
                continue;
            };
            let links: Option<Vec<u8>> = c
                .arts
                .iter()
                .map(|l| match l {
                    ArtLink::Retail(b) => Some(*b),
                    ArtLink::Pack(k) => index(k).map(|i| arts[i].first_byte()),
                })
                .collect();
            let Some(links) = links.filter(|l| !l.is_empty()) else {
                continue;
            };
            chains.push(CompiledChain {
                key: key.clone(),
                character: c.character,
                links,
                finisher,
            });
        }
        Self { arts, chains }
    }

    pub fn is_empty(&self) -> bool {
        self.arts.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&CompiledArt> {
        self.arts.iter().find(|a| a.key == key)
    }

    /// `character`'s arts.
    pub fn of(&self, character: Character) -> impl Iterator<Item = &CompiledArt> {
        self.arts.iter().filter(move |a| a.character == character)
    }

    /// The art and segment queue byte `byte` names for `character`.
    pub fn segment(
        &self,
        character: Character,
        byte: u8,
    ) -> Option<(&CompiledArt, &CompiledSegment)> {
        if byte < FIRST_PACK_BYTE {
            return None;
        }
        self.of(character)
            .find_map(|a| a.segments.iter().find(|s| s.byte == byte).map(|s| (a, s)))
    }

    /// The art whose **first** byte is `byte`.
    pub fn art_at(&self, character: Character, byte: u8) -> Option<&CompiledArt> {
        self.of(character).find(|a| a.first_byte() == byte)
    }
}

/// The bank-walk rows a known pack art adds for `character`: `(queue byte,
/// command bytes)`, one per input-matchable known art. Feed them to
/// `legaia_art::bank_rows` after the retail records.
pub fn bank_extension(
    arts: &CompiledArts,
    character: Character,
    known: impl Fn(&CompiledArt) -> bool,
) -> Vec<(u8, Vec<u8>)> {
    arts.of(character)
        .filter(|a| !a.commands.is_empty() && known(a))
        .map(|a| (a.first_byte(), a.commands.clone()))
        .collect()
}

/// Insert each pack art's further segment bytes after its first, in a
/// zero-terminated queue window of `queue.len()` bytes; `marks` (the
/// builder's side array) moves with the bytes. Bytes pushed past the window's
/// last slot are dropped, and the window always ends in a terminator.
/// Returns the pack arts the queue performs, by index, in queue order.
pub fn expand_segments(
    arts: &CompiledArts,
    character: Character,
    queue: &mut [u8],
    marks: &mut [u32],
) -> Vec<usize> {
    let mut performed = Vec::new();
    let mut i = 0usize;
    let cap = queue.len().saturating_sub(1);
    while i < cap && queue[i] != 0 {
        let b = queue[i];
        if let Some(idx) = arts
            .arts
            .iter()
            .position(|a| a.character == character && a.first_byte() == b)
        {
            performed.push(idx);
            let rest: Vec<u8> = arts.arts[idx].bytes().skip(1).collect();
            for (k, &extra) in rest.iter().enumerate() {
                let at = i + 1 + k;
                if at >= cap {
                    break;
                }
                for j in (at..cap).rev() {
                    queue[j] = queue[j - 1];
                    if j < marks.len() {
                        marks[j] = marks[j - 1];
                    }
                }
                queue[at] = extra;
                if at < marks.len() {
                    marks[at] = 0;
                }
            }
            i += rest.len();
        }
        i += 1;
    }
    if let Some(last) = queue.last_mut() {
        *last = 0;
    }
    performed
}

/// The arts a queue performs, in order: retail art constants and pack arts'
/// first bytes (a pack art's later segment bytes are not arts of their own).
pub fn performed_arts(arts: &CompiledArts, character: Character, queue: &[u8]) -> Vec<u8> {
    queue
        .iter()
        .take_while(|&&b| b != 0)
        .copied()
        .filter(|&b| {
            (0x1B..=0x32).contains(&b)
                || (b >= FIRST_PACK_BYTE && arts.art_at(character, b).is_some())
        })
        .collect()
}

/// The chain pass: when the arts `queue` performs end with a chain's links,
/// append `0x1A` (marked for the `SUPER` banner) and the finisher's bytes.
/// Runs after the retail finish (Miracle, Super), so a retail Super Art
/// resolves first and a chain reads its result. Returns the finisher's index.
pub fn apply_chains(
    arts: &CompiledArts,
    character: Character,
    known: impl Fn(&CompiledArt) -> bool,
    queue: &mut [u8],
    marks: &mut [u32],
) -> Option<usize> {
    let performed = performed_arts(arts, character, queue);
    let chain = arts
        .chains
        .iter()
        .filter(|c| c.character == character && known(&arts.arts[c.finisher]))
        .filter(|c| performed.ends_with(&c.links))
        .max_by_key(|c| c.links.len())?;
    let fin = &arts.arts[chain.finisher];
    let len = queue.iter().position(|&b| b == 0).unwrap_or(queue.len());
    let need = 1 + fin.segments.len();
    if len + need >= queue.len() {
        return None;
    }
    queue[len] = SPECIAL_STARTER;
    if len < marks.len() {
        marks[len] = SUPER_MARK;
    }
    for (k, b) in fin.bytes().enumerate() {
        queue[len + 1 + k] = b;
    }
    Some(chain.finisher)
}

/// The arts the player knows, by local key; saved in tail record `5` of the
/// expansion block beside the pack spells.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtsProgress {
    pub learned: BTreeSet<String>,
}

/// The battle-side state of the pack's arts.
#[derive(Debug, Clone, Default)]
pub struct ArtsRuntime {
    pub compiled: Option<std::sync::Arc<CompiledArts>>,
    /// The pack art each battle actor's committed clip belongs to, by actor
    /// slot: `(art index, segment index)`, set by the anim commit.
    pub playing: BTreeMap<usize, (usize, usize)>,
    /// The pack arts the queue just built performs (indices), for the Spirit
    /// charge that follows the build.
    pub turn_performed: Vec<usize>,
}

impl ArtsRuntime {
    /// The arts, when the pack has any.
    pub fn arts(&self) -> Option<&CompiledArts> {
        self.compiled.as_deref().filter(|c| !c.is_empty())
    }
}

/// The AP the pack arts in `performed` cost, halved by AP Used Down.
pub fn pack_ap_cost(arts: &CompiledArts, performed: &[usize], halved: bool) -> u16 {
    performed
        .iter()
        .map(|&i| {
            let ap = u16::from(arts.arts[i].ap);
            if halved { ap >> 1 } else { ap }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section() -> ArtSection {
        toml::from_str(
            r#"
            [arts.ember]
            character = "vahn"
            name = "Ember Cascade"
            commands = "LRDUR"
            ap = 30
            segments = [{ art = "Pyro Pummel", power = ["U22", "L22"] }, { art = 0x27, power = ["L28"] }]

            [arts.nova]
            character = "vahn"
            name = "Rebirth Nova"
            segments = [{ art = 0x2A }]

            [arts.kick]
            character = "noa"
            name = "Gale Lance"
            commands = "UDUD"
            segments = [{ art = 0x29 }]

            [chains.nova]
            character = "vahn"
            arts = ["ember", 0x27]
            finisher = "nova"
            "#,
        )
        .unwrap()
    }

    #[test]
    fn bytes_are_allotted_per_character_in_key_order() {
        let c = CompiledArts::compile("t", &section());
        let ember = c.get("ember").unwrap();
        let nova = c.get("nova").unwrap();
        assert_eq!(ember.bytes().collect::<Vec<_>>(), [0x33, 0x34]);
        assert_eq!(nova.first_byte(), 0x35);
        assert_eq!(c.get("kick").unwrap().first_byte(), 0x33, "Noa's own space");
        assert_eq!(ember.segments[0].base, 0x23);
        assert_eq!(ember.segments[0].power, [0x19, 0x1E]);
        assert_eq!(
            ember.segments[0].power_run([0x16, 0x16, 0, 0]),
            [0x19, 0x1E, 0, 0]
        );
        let (a, s) = c.segment(Character::Vahn, 0x34).unwrap();
        assert_eq!((a.key.as_str(), s.base), ("ember", 0x27));
        assert!(c.segment(Character::Noa, 0x34).is_none());
        assert_eq!(c.chains[0].links, [0x33, 0x27]);
    }

    #[test]
    fn segments_expand_behind_the_first_byte() {
        let c = CompiledArts::compile("t", &section());
        let mut q = [0u8; 19];
        q[..4].copy_from_slice(&[0x0C, 0x19, 0x33, 0x0E]);
        let mut marks = [0u32; 19];
        marks[1] = 1;
        let performed = expand_segments(&c, Character::Vahn, &mut q, &mut marks);
        assert_eq!(&q[..6], &[0x0C, 0x19, 0x33, 0x34, 0x0E, 0]);
        assert_eq!(marks[1], 1);
        assert_eq!(performed, [0]);
    }

    #[test]
    fn a_chain_appends_the_super_starter_and_finisher() {
        let c = CompiledArts::compile("t", &section());
        let mut q = [0u8; 19];
        q[..6].copy_from_slice(&[0x19, 0x33, 0x34, 0x0E, 0x19, 0x27]);
        let mut marks = [0u32; 19];
        let fin = apply_chains(&c, Character::Vahn, |_| true, &mut q, &mut marks);
        assert_eq!(fin, c.arts.iter().position(|a| a.key == "nova"));
        assert_eq!(
            &q[..9],
            &[0x19, 0x33, 0x34, 0x0E, 0x19, 0x27, 0x1A, 0x35, 0]
        );
        assert_eq!(marks[6], SUPER_MARK);
        // An unknown finisher does not fire.
        let mut q2 = [0u8; 19];
        q2[..6].copy_from_slice(&[0x19, 0x33, 0x34, 0x0E, 0x19, 0x27]);
        assert_eq!(
            apply_chains(&c, Character::Vahn, |_| false, &mut q2, &mut marks),
            None
        );
    }

    #[test]
    fn chain_only_finishers_add_no_bank_row() {
        let c = CompiledArts::compile("t", &section());
        let rows = bank_extension(&c, Character::Vahn, |_| true);
        assert_eq!(rows, [(0x33, vec![1, 2, 3, 4, 2])]);
        assert!(bank_extension(&c, Character::Vahn, |_| false).is_empty());
    }
}
