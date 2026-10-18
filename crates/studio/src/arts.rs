//! A party member's Tactical Arts as named animation clips.
//!
//! The on-disc art bank (`battle_char_assembly::art_animation_bank`, inside
//! the player file's `record[0]`) carries a staged anim id, an optional dev
//! name and the matcher combo per record; the keyframes live in the
//! character's `"ME"` archives in `readef.DAT` (PROT 894,
//! `docs/formats/summon-readef.md`). The player-facing names come from the
//! curated arts table (`legaia_gamedata`). Each curated art is resolved to
//! its bank record with the same ladder the site's characters and arts pages
//! use (`crates/web-viewer/src/art_clip_bank.rs`, kept in step with it):
//!
//! 0. `action_constant == anim_id`;
//! 1. name and combo both match a named record;
//! 2. combo match (two or more directions; named records preferred);
//! 3. name match (placeholder-combo records preferred).
//!
//! The record plus every immediately following record sharing its name or
//! its full combo is one clip (Noa's Hurricane Kick ships as three strike
//! records). The result is what `loadout::build` takes as its `arts`
//! argument; it is per character, not per loadout.

use legaia_actor_formats::battle_char_assembly::{self as bca, loadout::ArtClip};
use legaia_actor_formats::monster_archive::MonsterAnimation;
use legaia_gamedata::{ArtKind, Character};

/// `readef.DAT`, the battle side-band file with the art `"ME"` archives.
pub const READEF_ENTRY: u32 = 894;

fn character_of(cslot: usize) -> Option<Character> {
    Some(match cslot {
        0 => Character::Vahn,
        1 => Character::Noa,
        2 => Character::Gala,
        _ => return None,
    })
}

fn kind_tag(kind: ArtKind) -> &'static str {
    match kind {
        ArtKind::Regular => "regular",
        ArtKind::Hyper => "hyper",
        ArtKind::Super => "super",
        ArtKind::Miracle => "miracle",
    }
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase()
}

/// The resolution ladder (module doc). Returns the bank index.
fn resolve(
    bank: &[bca::ArtAnimRecord],
    action_constant: Option<u8>,
    name: &str,
    directions: &[u8],
) -> Option<usize> {
    if let Some(ac) = action_constant
        && ac >= 0x10
        && let Some(i) = bank.iter().position(|r| r.anim_id == ac)
    {
        return Some(i);
    }
    let want = norm(name);
    if let Some(i) = bank
        .iter()
        .position(|r| !r.name.is_empty() && norm(&r.name) == want && r.combo == directions)
    {
        return Some(i);
    }
    if directions.len() >= 2 {
        let hits: Vec<usize> = (0..bank.len())
            .filter(|&i| bank[i].combo == directions)
            .collect();
        if let Some(&i) = hits
            .iter()
            .find(|&&i| !bank[i].name.is_empty())
            .or(hits.first())
        {
            return Some(i);
        }
    }
    let hits: Vec<usize> = (0..bank.len())
        .filter(|&i| !bank[i].name.is_empty() && norm(&bank[i].name) == want)
        .collect();
    hits.iter()
        .find(|&&i| bank[i].combo.len() <= 1)
        .or(hits.first())
        .copied()
}

/// `first` plus every immediately following record sharing its non-empty
/// name or its full (two or more direction) combo.
fn chain_of(bank: &[bca::ArtAnimRecord], first: usize) -> Vec<usize> {
    let name = &bank[first].name;
    let combo = &bank[first].combo;
    let mut chain = vec![first];
    for (i, r) in bank.iter().enumerate().skip(first + 1) {
        let same_name = !name.is_empty() && &r.name == name;
        let same_combo = combo.len() >= 2 && &r.combo == combo;
        if !(same_name || same_combo) {
            break;
        }
        chain.push(i);
    }
    chain
}

fn concat(segments: &[MonsterAnimation]) -> Option<MonsterAnimation> {
    let mut out = segments.first()?.clone();
    out.frames = segments.iter().flat_map(|s| s.frames.clone()).collect();
    out.frame_count = out.frames.len();
    Some(out)
}

/// Party member `cslot`'s (0 Vahn, 1 Noa, 2 Gala) Tactical Arts as named
/// clips, in the curated table's order. `player` is the character's player
/// file (PROT 863 + cslot) and `readef` is PROT 894. An art whose keyframes
/// do not decode on this disc is left out; Terra has no arts table and gets
/// none.
pub fn art_clips(player: &[u8], readef: &[u8], cslot: usize) -> Result<Vec<ArtClip>, String> {
    let Some(character) = character_of(cslot) else {
        return Ok(Vec::new());
    };
    let record0 = bca::decode_record0(player).map_err(|e| format!("record[0]: {e:#}"))?;
    let bank = bca::art_animation_bank(&record0).map_err(|e| format!("art bank: {e:#}"))?;
    let main = bca::art_me_archive(readef, cslot, false).ok();
    let base = bca::art_me_archive(readef, cslot, true).ok();
    let decoded: Vec<Option<MonsterAnimation>> = bank
        .iter()
        .map(|rec| {
            let archive = if rec.uses_base_archive() {
                base.as_ref()
            } else {
                main.as_ref()
            };
            archive
                .and_then(|a| bca::art_animation(rec, a).ok())
                .filter(|a| a.frame_count > 0)
        })
        .collect();

    let db = gamedata();
    let mut claimed = Vec::new();
    let mut out = Vec::new();
    for art in db.arts_for(character) {
        let Some(first) = resolve(&bank, art.action_constant, &art.name, &art.directions) else {
            continue;
        };
        if claimed.contains(&first) {
            continue;
        }
        let segments: Vec<MonsterAnimation> = chain_of(&bank, first)
            .into_iter()
            .filter_map(|i| decoded[i].clone())
            .collect();
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

/// An art's input as direction letters (`1=L 2=R 3=D 4=U`), e.g.
/// `L U U U U D R`. Letters rather than arrows: egui's bundled fonts carry
/// no arrow glyphs.
pub fn arrows(directions: &[u8]) -> String {
    directions
        .iter()
        .map(|d| match d {
            1 => "L",
            2 => "R",
            3 => "D",
            4 => "U",
            _ => "?",
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn gamedata() -> &'static legaia_gamedata::Database {
    static DB: std::sync::OnceLock<legaia_gamedata::Database> = std::sync::OnceLock::new();
    DB.get_or_init(legaia_gamedata::Database::load)
}
