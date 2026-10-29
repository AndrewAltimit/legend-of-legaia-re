//! A party member's Tactical Arts as named animation clips.
//!
//! The decode and the curated-name resolution are shared with the site's
//! characters page: `legaia_actor_formats`' `loadout::decode_art_clips`
//! runs the ladder of `legaia_art::clip_ladder` over the character's art bank
//! and its `readef.DAT` ME archives. This module only hands it the curated
//! arts table (`legaia_gamedata`) in the ladder's shape.

use legaia_actor_formats::battle_char_assembly::loadout::{self, ArtClip};
use legaia_art::clip_ladder::CuratedArt;
use legaia_gamedata::{ArtKind, Character};

/// `readef.DAT`, the battle side-band file with the art `"ME"` archives.
pub const READEF_ENTRY: u32 = 894;

/// The curated arts of party member `cslot` (0 Vahn, 1 Noa, 2 Gala); Terra
/// has no arts table.
pub fn curated_arts(cslot: usize) -> Vec<CuratedArt> {
    let character = match cslot {
        0 => Character::Vahn,
        1 => Character::Noa,
        2 => Character::Gala,
        _ => return Vec::new(),
    };
    gamedata()
        .arts_for(character)
        .map(|a| CuratedArt {
            name: a.name.clone(),
            kind: match a.kind {
                ArtKind::Regular => "regular",
                ArtKind::Hyper => "hyper",
                ArtKind::Super => "super",
                ArtKind::Miracle => "miracle",
            },
            ap: a.ap,
            directions: a.directions.clone(),
            action_constant: a.action_constant,
        })
        .collect()
}

/// Party member `cslot`'s Tactical Arts as named clips, in the curated
/// table's order. `player` is the character's player file (PROT 863 +
/// cslot) and `readef` is PROT 894.
pub fn art_clips(player: &[u8], readef: &[u8], cslot: usize) -> Result<Vec<ArtClip>, String> {
    let curated = curated_arts(cslot);
    if curated.is_empty() {
        return Ok(Vec::new());
    }
    loadout::decode_art_clips(player, readef, cslot, &curated)
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

pub(crate) fn gamedata() -> &'static legaia_gamedata::Database {
    static DB: std::sync::OnceLock<legaia_gamedata::Database> = std::sync::OnceLock::new();
    DB.get_or_init(legaia_gamedata::Database::load)
}
