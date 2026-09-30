//! Delilas party swap: play as Gi / Lu / Che while the story's ravine
//! duels (and the Muscle Dome Master legs) field Vahn / Noa / Gala.
//!
//! A pure model-and-name identity swap over `legaia_asset::party_swap`:
//! each playable character's battle files rebuild around the mapped
//! sibling's model (their own animations, arts, stats and story are
//! untouched), each sibling's monster block rebuilds around the mapped
//! character's battle model (the Delilas movesets drive it - the duels
//! play exactly as before with the fighters exchanged), the monster
//! blocks are renamed to the characters they now depict, and the
//! new-game template names the party after the siblings. The mapping is
//! caller-chosen: any permutation of the three siblings over the three
//! characters.
//!
//! Field-map visuals swap too: PROT 0874's party field meshes + atlas
//! rebuild from the same monster models (`party_swap::fieldize`), so
//! walking around towns shows the same siblings the battles do.

use anyhow::{Context, Result, bail};

use legaia_asset::monster_archive;
use legaia_asset::new_game;
use legaia_asset::party_swap::{self, PlayerRig, fieldize, playerize, winpose};

/// PROT entry of `readef.DAT` (the battle side-band streaming slots).
pub const READEF_ENTRY: usize = 894;

/// PROT entry of the raw battle-action overlay (base VA `0x801CE818`) -
/// where the per-character attack-camera jump tables and the
/// per-character element table live.
const BATTLE_OVERLAY_ENTRY: usize = 898;

use crate::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};

mod apply;
mod moveset;
mod reskin;
mod signature;
mod timing;

pub use apply::*;
use moveset::*;
use reskin::*;
pub use signature::*;
use timing::*;

/// How much of the swapped hero's Tactical-Arts kit becomes the
/// sibling's.
///
/// See [`apply_delilas_moveset`] for what `Delilas` rebuilds and
/// [`retained_bank_rows`] for why the host arts it keeps cannot be
/// dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DelilasMoveMode {
    /// Every art keeps the host hero's animation; only the one
    /// reskinned Hyper plays the sibling's signature special.
    #[default]
    Hybrid,
    /// The hero's whole art-stream archive is rebuilt from the
    /// sibling's own motions, the arts that survive are renamed after
    /// the clip each plays, and every art the Supers and the Miracle do
    /// not need is blanked out of the matcher.
    Delilas,
}

impl std::str::FromStr for DelilasMoveMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "hybrid" => Ok(Self::Hybrid),
            "delilas" => Ok(Self::Delilas),
            other => Err(format!(
                "unknown Delilas move mode {other:?} (expected hybrid or delilas)"
            )),
        }
    }
}

impl std::fmt::Display for DelilasMoveMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Hybrid => "hybrid",
            Self::Delilas => "delilas",
        })
    }
}

/// One Delilas sibling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sibling {
    Gi,
    Che,
    Lu,
}

impl Sibling {
    /// The sibling's monster-archive id.
    pub fn monster_id(self) -> u16 {
        match self {
            Sibling::Gi => 162,
            Sibling::Che => 163,
            Sibling::Lu => 164,
        }
    }

    /// The retail monster-block display name.
    pub fn retail_block_name(self) -> &'static str {
        match self {
            Sibling::Gi => "Gi Delilas",
            Sibling::Che => "Che Delilas",
            Sibling::Lu => "Lu Delilas",
        }
    }

    /// The sibling's tile in the save-slot portrait sheet (PROT 899
    /// `0x1F908`, [`crate::save_icon`]), in the sheet's character order
    /// (0 Vahn, 1 Noa, 2 Gala, ...). Gi's tile matches his masked field
    /// head TIM (stone bundle, CLUT (128,481)) - in the field events the
    /// siblings wear masks, which is why an unmasked-face guess misreads
    /// this sheet.
    pub fn portrait_tile(self) -> usize {
        match self {
            Sibling::Che => 11,
            Sibling::Gi => 12,
            Sibling::Lu => 13,
        }
    }

    /// The party display name the sibling fights under.
    pub fn display_name(self) -> &'static str {
        match self {
            Sibling::Gi => "Gi",
            Sibling::Che => "Che",
            Sibling::Lu => "Lu",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "gi" => Some(Sibling::Gi),
            "che" => Some(Sibling::Che),
            "lu" => Some(Sibling::Lu),
            _ => None,
        }
    }
}

/// Which sibling replaces each playable character. Always a permutation
/// of all three ([`PartyMapping::parse`] enforces it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartyMapping {
    pub vahn: Sibling,
    pub noa: Sibling,
    pub gala: Sibling,
}

impl Default for PartyMapping {
    /// Archetype-aligned: Gi (the leader) for Vahn, Lu for Noa, Che (the
    /// bruiser) for Gala.
    fn default() -> Self {
        PartyMapping {
            vahn: Sibling::Gi,
            noa: Sibling::Lu,
            gala: Sibling::Che,
        }
    }
}

impl PartyMapping {
    /// Parse `"gi,lu,che"`-style mappings: three comma-separated sibling
    /// names in Vahn, Noa, Gala order, each used exactly once.
    pub fn parse(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split(',').collect();
        if parts.len() != 3 {
            bail!("expected three comma-separated siblings (e.g. gi,lu,che)");
        }
        let mut siblings = Vec::with_capacity(3);
        for p in &parts {
            let sib = Sibling::parse(p)
                .ok_or_else(|| anyhow::anyhow!("unknown sibling {p:?} (gi / che / lu)"))?;
            if siblings.contains(&sib) {
                bail!("sibling {p:?} used twice - the mapping must be a permutation");
            }
            siblings.push(sib);
        }
        Ok(PartyMapping {
            vahn: siblings[0],
            noa: siblings[1],
            gala: siblings[2],
        })
    }

    /// `(player entry, rig, template slot, character name, sibling)` per
    /// playable character.
    pub fn pairs(&self) -> [(usize, &'static PlayerRig, usize, &'static str, Sibling); 3] {
        [
            (863, &party_swap::RIG_VAHN_GALA, 0, "Vahn", self.vahn),
            (864, &party_swap::RIG_NOA, 1, "Noa", self.noa),
            (865, &party_swap::RIG_VAHN_GALA, 2, "Gala", self.gala),
        ]
    }
}

/// Rename a decoded monster block's display name in place. The new name
/// (plus the retail `0x01` colour-escape prefix when present) must fit
/// the old string's byte span; the tail NUL-pads.
pub fn rename_block(block: &mut [u8], new_name: &str) -> Result<()> {
    let name_off = u32::from_le_bytes(
        block
            .get(0..4)
            .ok_or_else(|| anyhow::anyhow!("block too short"))?
            .try_into()
            .unwrap(),
    ) as usize;
    let region = block
        .get_mut(name_off..)
        .ok_or_else(|| anyhow::anyhow!("name offset {name_off:#x} out of range"))?;
    let prefix = usize::from(region.first() == Some(&0x01));
    let old_len = region[prefix..]
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| anyhow::anyhow!("unterminated name string"))?;
    if new_name.len() > old_len {
        bail!(
            "name {new_name:?} ({} bytes) does not fit the {} -byte slot",
            new_name.len(),
            old_len
        );
    }
    region[prefix..prefix + old_len].fill(0);
    region[prefix..prefix + new_name.len()].copy_from_slice(new_name.as_bytes());
    Ok(())
}

/// Report of one [`apply_delilas_party`] run.
#[derive(Debug, Default)]
pub struct DelilasPartyReport {
    /// `false` when every pairing was already applied.
    pub changed: bool,
    /// Human-readable per-pair notes (scales, texture downscales).
    pub notes: Vec<String>,
}

/// Apply the party swap onto the disc: monster blocks re-skinned +
/// renamed, player battle files rebuilt, new-game template renamed.
/// Idempotent - a block already carrying its mapped character's name is
/// skipped whole; an unrecognized name (neither retail nor applied)
/// aborts before any write.
/// Whether the signature cast route may claim the SCUS injection arena.
///
/// The arena is shared with `--shiny-seru`, `--show-super-arts` and
/// `--arts-ap-grant`/`--arts-ap-cost`; when any of those is enabled the
/// FRONTEND passes [`CastRoutePolicy::ArenaTaken`] and the route downgrades
/// to the art-side signature up front - order-independently, leaving the
/// player files and cast module untouched - instead of racing the other
/// feature for the bytes (one apply order used to hard-error the whole
/// patch, the other silently shipped a half-installed route).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastRoutePolicy {
    /// No arena-claiming feature is enabled: install the cast route.
    Install,
    /// An arena feature is enabled: keep the art-side signature, say so.
    ArenaTaken,
}

/// Per-run visual options of the party swap, off by default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DelilasPartyOptions {
    /// Keep Che's welded hammer-fist on the swapped mesh (his authored
    /// giant hammer) instead of mirroring the other fist in; the host's
    /// own weapon is then NOT fused into his hand. Clips that assume a
    /// hand-sized part swing the hammer wide - the comparison trade the
    /// flag opts into. Che only: Gi's welded blade-fist stays replaced
    /// (its reach is what caused the catalogued Spirit-charge streak).
    pub keep_che_hammer: bool,
}

/// Replace every word-boundary occurrence of `from` in `text` with `to`.
/// `None` when `from` does not occur as a whole word. A boundary is any
/// non-alphanumeric byte (markup braces, punctuation, spaces) or the
/// string edge, so "Lu" matches in "I am Lu Delilas!" but not in "Lucky".
fn replace_word(text: &str, from: &str, to: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let f = from.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    let mut hit = false;
    while i < bytes.len() {
        let is_match = bytes[i..].starts_with(f)
            && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric())
            && (i + f.len() == bytes.len() || !bytes[i + f.len()].is_ascii_alphanumeric());
        if is_match {
            out.push_str(to);
            i += f.len();
            hit = true;
        } else {
            // Advance one UTF-8 scalar so `out` stays valid text.
            let step = text[i..].chars().next().map_or(1, char::len_utf8);
            out.push_str(&text[i..i + step]);
            i += step;
        }
    }
    hit.then_some(out)
}

pub fn apply_delilas_party(
    patcher: &mut DiscPatcher,
    mapping: &PartyMapping,
    arts_voice: crate::delilas_voice_fx::ArtsVoiceMode,
    move_mode: DelilasMoveMode,
    cast_route: CastRoutePolicy,
) -> Result<DelilasPartyReport> {
    apply_delilas_party_with(
        patcher,
        mapping,
        arts_voice,
        move_mode,
        cast_route,
        DelilasPartyOptions::default(),
    )
}

#[cfg(test)]
mod tests;
