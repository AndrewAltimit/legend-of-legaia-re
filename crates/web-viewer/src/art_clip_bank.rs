//! A character's Tactical Arts as **named clips** for the equipment viewer.
//!
//! The arts page (`arts_view`) plays each curated art on the all-defaults
//! battle model; the characters page shows the model at a chosen loadout and
//! wants the same clips - weapon in hand - listed next to the action bank, so
//! `set_equipped_character` can hand every art over as one more labelled clip
//! and the `.glb` export bakes them alongside the actions and swings.
//!
//! What this module owns is the **naming**: the on-disc art bank
//! ([`bca::art_animation_bank`]) carries a staged anim id, an optional
//! dev-name and the matcher combo per record, and nothing else - the player
//! facing names live in the curated arts table (`legaia_gamedata`). Each
//! curated art is resolved to its bank record through the ladder the arts
//! page uses (mirrored 1:1 by `tests/arts_view_real.rs`):
//!
//! 0. `action_constant == anim_id` (exact on retail);
//! 1. name + combo both match a named record;
//! 2. combo match (>= 2 directions; named records preferred);
//! 3. name match (placeholder-combo Super-Art tail records preferred).
//!
//! and its **chain** - the record plus every immediately following record
//! sharing its non-empty name or its full combo - is concatenated into one
//! clip (Noa's Hurricane Kick ships as three consecutive strike records).
//! Curated rows that land on a record another row already claimed (the three
//! Hurricane Kick levels share one anim id) collapse into that one clip.
//!
//! The decode is per **character**, not per loadout: an art clip is a
//! whole-body keyframe stream out of `readef.DAT`, independent of what the
//! character wears (only the four weapon swings are equipment-spliced), so
//! the caller caches the result and re-expands it per assembled object.

use legaia_asset::battle_char_assembly as bca;
use legaia_asset::monster_archive::MonsterAnimation;
use legaia_gamedata::{ArtKind, Character};

use crate::disc;

/// `readef.DAT` (extraction PROT entry 894) - the battle side-band file
/// carrying each character's art `"ME"` keyframe archives.
const READEF_PROT_INDEX: u32 = 894;

/// The loadout kernel's [`ArtClip`] - the data shape this module fills.
/// The struct lives with the shared kernel
/// (`legaia_asset::battle_char_assembly::loadout`) so a native consumer can
/// take the same clips; the curated-name resolution ladder below stays
/// host-side because it needs `legaia_gamedata` and the disc TOC.
pub(crate) use legaia_asset::battle_char_assembly::loadout::ArtClip;

/// The `kind` tag the summary JSON carries per art clip.
pub(crate) fn kind_tag(kind: ArtKind) -> &'static str {
    match kind {
        ArtKind::Regular => "regular",
        ArtKind::Hyper => "hyper",
        ArtKind::Super => "super",
        ArtKind::Miracle => "miracle",
    }
}

fn character_of(cslot: usize) -> Option<Character> {
    Some(match cslot {
        0 => Character::Vahn,
        1 => Character::Noa,
        2 => Character::Gala,
        _ => return None,
    })
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// What the ladder reads off a bank record - implemented for the on-disc
/// [`bca::ArtAnimRecord`] and for the unit tests' bare tuples.
pub(crate) trait BankRecord {
    fn anim_id(&self) -> u8;
    fn name(&self) -> &str;
    fn combo(&self) -> &[u8];
}

impl BankRecord for bca::ArtAnimRecord {
    fn anim_id(&self) -> u8 {
        self.anim_id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn combo(&self) -> &[u8] {
        &self.combo
    }
}

/// The resolution ladder (see the module doc). Returns the bank index.
pub(crate) fn resolve_art<R: BankRecord>(
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
        let hits: Vec<usize> = bank
            .iter()
            .enumerate()
            .filter(|(_, r)| r.combo() == directions)
            .map(|(i, _)| i)
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
    let hits: Vec<usize> = bank
        .iter()
        .enumerate()
        .filter(|(_, r)| !r.name().is_empty() && norm(r.name()) == want)
        .map(|(i, _)| i)
        .collect();
    hits.iter()
        .find(|&&i| bank[i].combo().len() <= 1)
        .or(hits.first())
        .copied()
}

/// The strike-segment chain: `first` plus every immediately following record
/// sharing its non-empty name OR its full (>= 2 direction) combo.
pub(crate) fn chain_of<R: BankRecord>(bank: &[R], first: usize) -> Vec<usize> {
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

/// Concatenate a chain's decoded segments into one clip. Segments share the
/// rig width; the clip takes the first segment's rate byte and identity.
fn concat(segments: &[MonsterAnimation]) -> Option<MonsterAnimation> {
    let first = segments.first()?;
    let mut out = MonsterAnimation {
        action_id: first.action_id,
        attach_key: first.attach_key,
        rate: first.rate,
        part_count: first.part_count,
        frame_count: 0,
        frames: Vec::new(),
        effect_script: first.effect_script.clone(),
    };
    for s in segments {
        out.frames.extend(s.frames.iter().cloned());
    }
    out.frame_count = out.frames.len();
    Some(out)
}

/// Decode character `cslot`'s (0 Vahn / 1 Noa / 2 Gala) Tactical Arts as
/// named clips, in the curated table's order. Arts whose keyframe stream is
/// missing or does not decode on this disc are skipped, not invented; an
/// unresolvable bank (no player file, no art bank) is an error, and a
/// character without a curated table (Terra) yields an empty list.
pub(crate) fn decode_art_clips(
    prot: &[u8],
    entries: &[disc::EntryMeta],
    cslot: usize,
) -> Result<Vec<ArtClip>, String> {
    let Some(character) = character_of(cslot) else {
        return Ok(Vec::new());
    };
    let prot_index = crate::equipment_view::PLAYER_FILE_BASE + cslot as u32;
    let raw = entry_bytes(prot, entries, prot_index)
        .ok_or_else(|| format!("player file (PROT {prot_index}) not present"))?;
    let record0 = bca::decode_record0(raw).map_err(|e| format!("record[0]: {e:#}"))?;
    let bank = bca::art_animation_bank(&record0).map_err(|e| format!("art bank: {e:#}"))?;
    let readef = entry_bytes(prot, entries, READEF_PROT_INDEX)
        .ok_or_else(|| format!("readef.DAT (PROT {READEF_PROT_INDEX}) not present"))?;
    let main = bca::art_me_archive(readef, cslot, false).ok();
    let base = bca::art_me_archive(readef, cslot, true).ok();

    // Decode each bank record once (segments are shared between chains).
    let decoded: Vec<Option<MonsterAnimation>> = bank
        .iter()
        .map(|rec| {
            let archive = if rec.uses_base_archive() {
                base.as_ref()
            } else {
                main.as_ref()
            };
            archive.and_then(|a| bca::art_animation(rec, a).ok())
        })
        .collect();

    let db = gamedata_db();
    let mut claimed: Vec<usize> = Vec::new();
    let mut out = Vec::new();
    for art in db.arts_for(character) {
        let Some(first) = resolve_art(&bank, art.action_constant, &art.name, &art.directions)
        else {
            continue;
        };
        if claimed.contains(&first) {
            continue;
        }
        let chain: Vec<usize> = chain_of(&bank, first)
            .into_iter()
            .filter(|&i| decoded[i].as_ref().is_some_and(|a| a.frame_count > 0))
            .collect();
        let segments: Vec<MonsterAnimation> =
            chain.iter().filter_map(|&i| decoded[i].clone()).collect();
        let Some(anim) = concat(&segments) else {
            continue;
        };
        claimed.push(first);
        out.push(ArtClip {
            name: art.name.clone(),
            kind: kind_tag(art.kind),
            ap: art.ap,
            directions: art.directions.clone(),
            anim_id: bank[first].anim_id,
            segments: segments.len(),
            anim,
        });
    }
    Ok(out)
}

fn gamedata_db() -> &'static legaia_gamedata::Database {
    static DB: std::sync::OnceLock<legaia_gamedata::Database> = std::sync::OnceLock::new();
    DB.get_or_init(legaia_gamedata::Database::load)
}

fn entry_bytes<'a>(prot: &'a [u8], entries: &[disc::EntryMeta], index: u32) -> Option<&'a [u8]> {
    let meta = entries.iter().find(|e| e.index == index)?;
    let off = meta.byte_offset as usize;
    let end = off.checked_add(meta.size_bytes as usize)?;
    prot.get(off..end.min(prot.len()))
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

    #[test]
    fn ladder_prefers_action_constant_then_exact_then_combo_then_name() {
        let bank = vec![
            rec(0x10, "", &[3, 3]),
            rec(0x1C, "", &[1, 4, 2]),
            rec(0x1D, "", &[1, 4, 2]),
            rec(0x1E, "", &[1, 4, 2]),
            rec(0x20, "Beatfire", &[4, 4, 1]),
            rec(0x2A, "Rolling Combo", &[3, 4]),
            rec(0x2B, "Rolling Combo", &[0]),
        ];
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
    fn concat_sums_frames_and_keeps_the_first_rate() {
        let seg = |rate: u8, n: usize| MonsterAnimation {
            action_id: 0x1C,
            attach_key: 0,
            rate,
            part_count: 2,
            frame_count: n,
            frames: vec![vec![Default::default(); 2]; n],
            effect_script: Vec::new(),
        };
        let a = concat(&[seg(2, 5), seg(1, 7)]).unwrap();
        assert_eq!(a.frame_count, 12);
        assert_eq!(a.frames.len(), 12);
        assert_eq!(a.rate, 2);
        assert!(concat(&[]).is_none());
    }

    #[test]
    fn every_curated_character_has_a_table_and_terra_has_none() {
        assert!(character_of(3).is_none());
        for c in 0..3 {
            let ch = character_of(c).unwrap();
            assert!(
                gamedata_db().arts_for(ch).count() >= 15,
                "{ch:?} arts table"
            );
        }
    }
}
