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
use legaia_asset::party_swap::{self, PlayerRig, fieldize, playerize};

use crate::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};

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
pub fn apply_delilas_party(
    patcher: &mut DiscPatcher,
    mapping: &PartyMapping,
) -> Result<DelilasPartyReport> {
    let mut report = DelilasPartyReport::default();
    let archive = patcher
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .context("read monster archive")?;

    // Baseline pass before any write: every target block's name must be
    // its retail sibling name (fresh) or its mapped character's (already
    // applied).
    let mut fresh = Vec::new();
    for (entry, _, _, who, sibling) in mapping.pairs() {
        let id = sibling.monster_id();
        let name = monster_archive::record(&archive, id)?
            .map(|r| r.name)
            .ok_or_else(|| anyhow::anyhow!("monster id {id}: empty slot"))?;
        if name == who {
            continue; // this pairing is already applied
        }
        if name != sibling.retail_block_name() {
            bail!(
                "monster id {id} is named {name:?} - neither retail \
                 ({:?}) nor swapped ({who:?}); refusing to touch an \
                 unrecognized build",
                sibling.retail_block_name()
            );
        }
        fresh.push(entry);
    }

    for (entry, rig, template_slot, who, sibling) in mapping.pairs() {
        if !fresh.contains(&entry) {
            continue;
        }
        let id = sibling.monster_id();
        let player_file = patcher
            .read_entry_footprint(entry)
            .with_context(|| format!("read player file PROT {entry}"))?;

        // Enemy side: the sibling's block wears the character's model
        // and name.
        let swapped = party_swap::swap_into_block(&player_file, rig, &archive, id)
            .with_context(|| format!("{who} -> monster {id}"))?;
        let mut block = swapped.block;
        rename_block(&mut block, who).with_context(|| format!("rename monster {id} to {who:?}"))?;
        let slot = monster_archive::encode_slot(&block)
            .with_context(|| format!("re-encode monster {id}"))?;
        patcher.patch_monster_slot(id, &slot)?;

        // Player side: the character wears the sibling's model.
        let entry_len = patcher
            .read_entry(entry)
            .with_context(|| format!("read PROT entry {entry}"))?
            .len();
        let playerized =
            playerize::playerize_player_file(&player_file, entry_len, rig, &archive, id)
                .with_context(|| format!("{who} <- monster {id}"))?;
        patcher.patch_prot_entry(entry, 0, &playerized.file)?;

        // New-game template name (fixed 10-byte NUL-padded field; only
        // affects new games - existing saves keep their stored names).
        let scus = patcher
            .read_named_file(crate::steal::SCUS_NAME)
            .ok_or_else(|| anyhow::anyhow!("SCUS_942.54 not found"))?;
        let tmpl_off = new_game::party_template_file_offset(&scus)
            .ok_or_else(|| anyhow::anyhow!("starting-party template not found in SCUS"))?
            as u64;
        let name_off = tmpl_off + (template_slot * new_game::RECORD_STRIDE) as u64 + 16;
        let mut field = vec![0u8; new_game::NAME_LEN];
        field[..sibling.display_name().len()].copy_from_slice(sibling.display_name().as_bytes());
        patcher
            .patch_named_file(crate::steal::SCUS_NAME, name_off, &field)
            .with_context(|| format!("write template name for slot {template_slot}"))?;

        report.changed = true;
        for w in swapped.warnings.iter().chain(playerized.warnings.iter()) {
            report
                .notes
                .push(format!("{who} <-> {}: {w}", sibling.display_name()));
        }
    }

    // Field forms: rebuild PROT 0874 so the party walks around as the
    // mapped siblings too (built from the same monster models as the
    // battle side, so both forms match). Runs only alongside a fresh
    // apply - an already-swapped 0874 must not re-convert.
    if report.changed {
        let field_entry = fieldize::PROT_ENTRY_INDEX;
        let prot_0874 = patcher
            .read_entry_footprint(field_entry)
            .context("read PROT 0874")?;
        let entry_len = patcher
            .read_entry(field_entry)
            .context("PROT 0874 length")?
            .len();
        let fieldized = fieldize::fieldize_pack(
            &prot_0874,
            entry_len,
            &archive,
            [
                mapping.vahn.monster_id(),
                mapping.noa.monster_id(),
                mapping.gala.monster_id(),
            ],
        )
        .context("rebuild field forms (PROT 0874)")?;
        patcher.patch_prot_entry(field_entry, 0, &fieldized.entry)?;
        for w in &fieldized.warnings {
            report.notes.push(format!("field: {w}"));
        }
    }
    Ok(report)
}
