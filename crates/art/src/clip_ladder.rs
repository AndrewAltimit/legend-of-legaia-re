//! Naming a character's on-disc art animations: which record of the
//! `record[0]` art bank each curated Tactical Art plays.
//!
//! The art bank (`battle_char_assembly::art_animation_bank` in
//! `legaia-actor-formats`) carries a staged anim id, an optional dev name and
//! the matcher combo per record, and nothing else; the player-facing names
//! live in the curated arts table (`legaia_gamedata`). Each curated art is
//! resolved to its bank record through a ladder:
//!
//! 0. `action_constant == anim_id` (exact on retail);
//! 1. name and combo both match a named record;
//! 2. combo match (two or more directions; named records preferred);
//! 3. name match (placeholder-combo Super-Art tail records preferred).
//!
//! Its **chain** - the record plus every immediately following record
//! sharing its non-empty name or its full combo - is one clip (Noa's
//! Hurricane Kick ships as three consecutive strike records). Curated rows
//! that land on a record another row already claimed (the three Hurricane
//! Kick levels share one anim id) collapse into that one clip.
//!
//! This module is the ladder alone, over any record type ([`BankRecord`])
//! and plain curated rows ([`CuratedArt`]), so it depends on neither the
//! archive decoder nor the curated tables; `legaia_actor_formats`'
//! `loadout::decode_art_clips` runs it on the disc bank, and the site's
//! characters page and Legaia Studio both call that.

/// What the ladder reads off one art-bank record.
pub trait BankRecord {
    fn anim_id(&self) -> u8;
    fn name(&self) -> &str;
    fn combo(&self) -> &[u8];
}

/// One curated art, as the ladder and the clip it names need it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuratedArt {
    /// Display name (arts table).
    pub name: String,
    /// `regular` / `hyper` / `super` / `miracle`.
    pub kind: &'static str,
    /// AP the art costs when fully expressed.
    pub ap: u32,
    /// Direction bytes of the input (`1=L 2=R 3=D 4=U`).
    pub directions: Vec<u8>,
    /// The art's slot in the action-constant table, when it has one.
    pub action_constant: Option<u8>,
}

/// One curated art resolved to the bank: the records its clip concatenates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedClip {
    /// Index into the curated list.
    pub art: usize,
    /// The bank record the art resolved to.
    pub first: usize,
    /// `first` plus its chain, keeping only records that decode.
    pub chain: Vec<usize>,
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase()
}

/// The resolution ladder (module doc). Returns the bank index.
pub fn resolve_art<R: BankRecord>(
    bank: &[R],
    action_constant: Option<u8>,
    name: &str,
    directions: &[u8],
) -> Option<usize> {
    // 0. action constant (the staged anim id space starts at 0x10)
    if let Some(ac) = action_constant
        && ac >= 0x10
        && let Some(i) = bank.iter().position(|r| r.anim_id() == ac)
    {
        return Some(i);
    }
    let want = norm(name);
    // 1. exact: a named record whose name AND combo match
    if let Some(i) = bank
        .iter()
        .position(|r| !r.name().is_empty() && norm(r.name()) == want && r.combo() == directions)
    {
        return Some(i);
    }
    // 2. combo (>= 2 directions; named records preferred)
    if directions.len() >= 2 {
        let hits: Vec<usize> = (0..bank.len())
            .filter(|&i| bank[i].combo() == directions)
            .collect();
        if let Some(&i) = hits
            .iter()
            .find(|&&i| !bank[i].name().is_empty())
            .or(hits.first())
        {
            return Some(i);
        }
    }
    // 3. name (placeholder-combo records preferred: the Super-Art tails)
    let hits: Vec<usize> = (0..bank.len())
        .filter(|&i| !bank[i].name().is_empty() && norm(bank[i].name()) == want)
        .collect();
    hits.iter()
        .find(|&&i| bank[i].combo().len() <= 1)
        .or(hits.first())
        .copied()
}

/// The strike-segment chain: `first` plus every immediately following record
/// sharing its non-empty name OR its full (two or more direction) combo.
pub fn chain_of<R: BankRecord>(bank: &[R], first: usize) -> Vec<usize> {
    let mut chain = vec![first];
    let name = bank[first].name();
    let combo = bank[first].combo();
    for (i, r) in bank.iter().enumerate().skip(first + 1) {
        let same_name = !name.is_empty() && r.name() == name;
        let same_combo = combo.len() >= 2 && r.combo() == combo;
        if !(same_name || same_combo) {
            break;
        }
        chain.push(i);
    }
    chain
}

/// Resolve every curated art of one character, in the curated order: its
/// bank record and chain, dropping records `decodes` rejects, an art whose
/// chain is left empty, and an art whose record another art already
/// claimed.
pub fn plan_clips<R: BankRecord>(
    bank: &[R],
    arts: &[CuratedArt],
    decodes: impl Fn(usize) -> bool,
) -> Vec<PlannedClip> {
    let mut out: Vec<PlannedClip> = Vec::new();
    for (k, art) in arts.iter().enumerate() {
        let Some(first) = resolve_art(bank, art.action_constant, &art.name, &art.directions) else {
            continue;
        };
        if out.iter().any(|p| p.first == first) {
            continue;
        }
        let chain: Vec<usize> = chain_of(bank, first)
            .into_iter()
            .filter(|&i| decodes(i))
            .collect();
        if chain.is_empty() {
            continue;
        }
        out.push(PlannedClip {
            art: k,
            first,
            chain,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Rec(u8, &'static str, Vec<u8>);
    impl BankRecord for Rec {
        fn anim_id(&self) -> u8 {
            self.0
        }
        fn name(&self) -> &str {
            self.1
        }
        fn combo(&self) -> &[u8] {
            &self.2
        }
    }
    fn rec(anim_id: u8, name: &'static str, combo: &[u8]) -> Rec {
        Rec(anim_id, name, combo.to_vec())
    }

    fn bank() -> Vec<Rec> {
        vec![
            rec(0x10, "", &[3, 3]),
            rec(0x1C, "", &[1, 4, 2]),
            rec(0x1D, "", &[1, 4, 2]),
            rec(0x1E, "", &[1, 4, 2]),
            rec(0x20, "Beatfire", &[4, 4, 1]),
            rec(0x2A, "Rolling Combo", &[3, 4]),
            rec(0x2B, "Rolling Combo", &[0]),
        ]
    }

    #[test]
    fn ladder_prefers_action_constant_then_exact_then_combo_then_name() {
        let bank = bank();
        // 0. action constant wins even over a name match elsewhere.
        assert_eq!(
            resolve_art(&bank, Some(0x1C), "Hurricane Kick", &[1, 4, 2]),
            Some(1)
        );
        // 2. combo lands on the first record with that combo.
        assert_eq!(
            resolve_art(&bank, None, "Hurricane Kick", &[1, 4, 2]),
            Some(1)
        );
        // 2. a named record is preferred among combo hits.
        assert_eq!(
            resolve_art(&bank, None, "Tornado Flame", &[4, 4, 1]),
            Some(4)
        );
        // 3. name: the placeholder-combo tail is preferred.
        assert_eq!(
            resolve_art(&bank, None, "Rolling Combo", &[9, 9, 9, 9]),
            Some(6)
        );
        // Nothing matches.
        assert_eq!(resolve_art(&bank, None, "Nope", &[7, 7]), None);
        // The three Hurricane Kick strikes chain by combo; the tail record
        // chains by name.
        assert_eq!(chain_of(&bank, 1), vec![1, 2, 3]);
        assert_eq!(chain_of(&bank, 5), vec![5, 6]);
        assert_eq!(chain_of(&bank, 4), vec![4]);
    }

    #[test]
    fn plan_collapses_shared_records_and_drops_undecodable_ones() {
        let bank = bank();
        let art = |name: &str, dirs: &[u8], ac: Option<u8>| CuratedArt {
            name: name.into(),
            kind: "regular",
            ap: 18,
            directions: dirs.to_vec(),
            action_constant: ac,
        };
        let arts = [
            art("Hurricane Kick", &[1, 4, 2], Some(0x1C)),
            art("Hurricane Kick Lv2", &[1, 4, 2], Some(0x1C)),
            art("Tornado Flame", &[4, 4, 1], None),
            art("Nope", &[7, 7], None),
        ];
        // Record 2 does not decode: the chain skips it.
        let plan = plan_clips(&bank, &arts, |i| i != 2);
        assert_eq!(
            plan,
            vec![
                PlannedClip {
                    art: 0,
                    first: 1,
                    chain: vec![1, 3]
                },
                PlannedClip {
                    art: 2,
                    first: 4,
                    chain: vec![4]
                },
            ]
        );
        // Nothing decodes: nothing is planned.
        assert!(plan_clips(&bank, &arts, |_| false).is_empty());
    }
}
