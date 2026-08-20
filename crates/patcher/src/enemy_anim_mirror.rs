//! Enemy-side animation mirror for `--delilas-party`: after
//! `apply_delilas_party` re-skins monster blocks 162/163/164 with the
//! mapped heroes' battle models, this pass rewrites the swapped blocks'
//! animation entries with the HERO's own clips
//! (`legaia_asset::party_swap::enemy_anim`), so the Nivora ravine duels
//! fight a Vahn/Noa/Gala who stand, react and strike like themselves
//! instead of posing the hero mesh with Delilas choreography.
//!
//! ## Ordering contract
//!
//! This pass MUST run **after** the party-swap model loop has patched the
//! monster slots (it rewrites the swapped block in place), and its retail
//! inputs MUST be captured **before** any patching: `apply_delilas_party`
//! rewrites the player files (playerize), the readef ME slots (the
//! signature reskin + `--delilas-moves`) and the monster archive itself,
//! so post-patch copies would hand the retarget the wrong data. The
//! [`RetailSources`] struct is that capture; `apply_delilas_party`
//! already reads all three images up front and can pass them through.
//!
//! ## What the cast modules pin
//!
//! The per-spell cast modules (PROT 958/959/960) stage archive entries by
//! **raw index** on the casting monster and their staged indices are
//! compiled MIPS - see `docs/formats/monster-animation.md` § "A special
//! attack can be a chain of entries". The module code is untouched; only
//! the CONTENT of the staged entries changes, every rewritten staged
//! entry keeps at least `MIN_STAGED_FRAMES` keyframes, and the entry
//! count / index space of each block is preserved.

use anyhow::{Context, Result, bail};

use legaia_asset::monster_archive;
use legaia_asset::party_swap::enemy_anim::{self, MirrorOptions};

use crate::delilas_party::{PartyMapping, Sibling};
use crate::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};

/// The retail images the retarget reads, captured before any patching.
pub struct RetailSources<'a> {
    /// PROT 867 (`monster_data` battle archive), pre-swap.
    pub archive: &'a [u8],
    /// PROT 863/864/865 (`PLAYER1..3`), pre-playerize, slot-indexed
    /// (0 = Vahn, 1 = Noa, 2 = Gala).
    pub players: [&'a [u8]; 3],
    /// PROT 894 (`readef.DAT`), pre-reskin.
    pub readef: &'a [u8],
}

/// The archive entries each sibling's cast module stages by raw index
/// (caster chain in stage order, plus the closing entry where the module
/// stages one). Probe-traced for Che (`10 -> 11`) and Lu
/// (`14 -> 12 -> 13`, close `15`); Gi's chain (`10 -> 11 -> 12`, close
/// `13`) is the module-scan + player-corroborated reading recorded in
/// `docs/formats/monster-animation.md`.
pub fn staged_entries(sibling: Sibling) -> (&'static [usize], Option<usize>) {
    match sibling {
        Sibling::Gi => (&[10, 11, 12], Some(13)),
        Sibling::Che => (&[10, 11], None),
        Sibling::Lu => (&[14, 12, 13], Some(15)),
    }
}

/// The drop ladder: when the re-encoded slot misses the fixed `0x14000`
/// archive budget, optional entry families are given up front-to-back.
/// The idle and the module-staged special chain are never dropped.
const LADDER: [MirrorOptions; 4] = [
    MirrorOptions::ALL,
    MirrorOptions {
        attacks: false,
        walk: true,
        reactions: true,
    },
    MirrorOptions {
        attacks: false,
        walk: false,
        reactions: true,
    },
    MirrorOptions::NONE,
];

/// Rewrite each swapped block's animation entries with the mapped hero's
/// own clips. Idempotent (entry content is a pure function of the retail
/// sources). Errors if a target block does not carry the mapped hero's
/// name - i.e. if the party-swap model loop has not run yet.
pub fn apply_enemy_anim_mirror(
    patcher: &mut DiscPatcher,
    mapping: &PartyMapping,
    retail: &RetailSources<'_>,
) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    let current_archive = patcher
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .context("read monster archive")?;
    for (_, rig, slot, who, sibling) in mapping.pairs() {
        let id = sibling.monster_id();
        let name = monster_archive::record(&current_archive, id)?
            .map(|r| r.name)
            .ok_or_else(|| anyhow::anyhow!("monster id {id}: empty slot"))?;
        if name != who {
            bail!(
                "monster id {id} is named {name:?}, expected {who:?} - the enemy \
                 anim mirror must run after the party-swap model loop"
            );
        }
        let current_block = monster_archive::decode_block(&current_archive, id)?
            .ok_or_else(|| anyhow::anyhow!("monster id {id}: block does not decode"))?;
        let (staged, close) = staged_entries(sibling);

        let mut done = false;
        for (step, opts) in LADDER.iter().enumerate() {
            let mirrored = enemy_anim::mirror_block_animations(
                &current_block,
                retail.archive,
                id,
                retail.players[slot],
                retail.readef,
                slot,
                rig,
                staged,
                close,
                opts,
            )
            .with_context(|| format!("{who} anim mirror for monster {id}"))?;
            match monster_archive::encode_slot(&mirrored.block) {
                Ok(new_slot) => {
                    patcher
                        .patch_monster_slot(id, &new_slot)
                        .with_context(|| format!("write monster {id} slot"))?;
                    notes.push(format!(
                        "{who} (monster {id}): {} entries rewritten with {who}'s own clips{}",
                        mirrored.rewritten.len(),
                        if step > 0 {
                            format!(" (budget ladder step {step}: {opts:?})")
                        } else {
                            String::new()
                        }
                    ));
                    for n in mirrored.notes {
                        notes.push(format!("{who} (monster {id}): {n}"));
                    }
                    done = true;
                    break;
                }
                Err(e) => {
                    if step + 1 == LADDER.len() {
                        return Err(e).with_context(|| {
                            format!("{who} (monster {id}): mirrored block misses the slot budget")
                        });
                    }
                    notes.push(format!(
                        "{who} (monster {id}): ladder step {step} over budget ({e:#}); \
                         dropping an entry family"
                    ));
                }
            }
        }
        if !done {
            bail!("{who} (monster {id}): no ladder step fit the archive slot");
        }
    }
    Ok(notes)
}
