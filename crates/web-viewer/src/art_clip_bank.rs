//! A character's Tactical Arts as **named clips** for the equipment viewer.
//!
//! The arts page (`arts_view`) plays each curated art on the all-defaults
//! battle model; the characters page shows the model at a chosen loadout and
//! wants the same clips - weapon in hand - listed next to the action bank, so
//! `set_equipped_character` can hand every art over as one more labelled clip
//! and the `.glb` export bakes them alongside the actions and swings.
//!
//! The decode and the naming are shared with Legaia Studio:
//! `legaia_actor_formats`' `loadout::decode_art_clips` resolves each curated
//! art to its bank record and chain with `legaia_art::clip_ladder` (mirrored
//! 1:1 by `tests/arts_view_real.rs` for the arts page's JS). This module
//! reads the two entries off the disc and hands over the curated arts table
//! (`legaia_gamedata`) in the ladder's shape.
//!
//! The decode is per **character**, not per loadout: an art clip is a
//! whole-body keyframe stream out of `readef.DAT`, independent of what the
//! character wears (only the four weapon swings are equipment-spliced), so
//! the caller caches the result and re-expands it per assembled object.

use legaia_actor_formats::battle_char_assembly::loadout;
use legaia_art::clip_ladder::CuratedArt;
use legaia_gamedata::{ArtKind, Character};

/// `readef.DAT` (extraction PROT entry 894) - the battle side-band file
/// carrying each character's art `"ME"` keyframe archives.
const READEF_PROT_INDEX: u32 = 894;

/// The loadout kernel's [`ArtClip`] - the data shape the shared decode
/// fills.
pub(crate) use legaia_actor_formats::battle_char_assembly::loadout::ArtClip;

/// The `kind` tag the summary JSON carries per art clip.
fn kind_tag(kind: ArtKind) -> &'static str {
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

/// The curated arts table of `character`, in the ladder's shape.
fn curated(character: Character) -> Vec<CuratedArt> {
    gamedata_db()
        .arts_for(character)
        .map(|a| CuratedArt {
            name: a.name.clone(),
            kind: kind_tag(a.kind),
            ap: a.ap,
            directions: a.directions.clone(),
            action_constant: a.action_constant,
        })
        .collect()
}

/// Decode character `cslot`'s (0 Vahn / 1 Noa / 2 Gala) Tactical Arts as
/// named clips, in the curated table's order. Arts whose keyframe stream is
/// missing or does not decode on this disc are skipped, not invented; an
/// unresolvable bank (no player file, no art bank) is an error, and a
/// character without a curated table (Terra) yields an empty list.
pub(crate) fn decode_art_clips(
    prot: &[u8],
    entries: &[legaia_prot::toc_index::EntryMeta],
    cslot: usize,
) -> Result<Vec<ArtClip>, String> {
    let Some(character) = character_of(cslot) else {
        return Ok(Vec::new());
    };
    let prot_index = crate::equipment_view::PLAYER_FILE_BASE + cslot as u32;
    let raw = entry_bytes(prot, entries, prot_index)
        .ok_or_else(|| format!("player file (PROT {prot_index}) not present"))?;
    let readef = entry_bytes(prot, entries, READEF_PROT_INDEX)
        .ok_or_else(|| format!("readef.DAT (PROT {READEF_PROT_INDEX}) not present"))?;
    loadout::decode_art_clips(raw, readef, cslot, &curated(character))
}

fn gamedata_db() -> &'static legaia_gamedata::Database {
    static DB: std::sync::OnceLock<legaia_gamedata::Database> = std::sync::OnceLock::new();
    DB.get_or_init(legaia_gamedata::Database::load)
}

fn entry_bytes<'a>(
    prot: &'a [u8],
    entries: &[legaia_prot::toc_index::EntryMeta],
    index: u32,
) -> Option<&'a [u8]> {
    let meta = entries.iter().find(|e| e.index == index)?;
    let off = meta.byte_offset as usize;
    let end = off.checked_add(meta.size_bytes as usize)?;
    prot.get(off..end.min(prot.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_curated_character_has_a_table_and_terra_has_none() {
        assert!(character_of(3).is_none());
        for c in 0..3 {
            let ch = character_of(c).unwrap();
            assert!(curated(ch).len() >= 15, "{ch:?} arts table");
        }
    }
}
