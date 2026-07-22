//! Per-monster steal-item + Tactical-Arts button-combo randomization.

use super::*;

/// One monster's current steal: monster id, item id, and steal chance percent.
/// Mirrors [`CurrentDrop`] for the steal table (see [`crate::steal`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StealSite {
    pub monster_id: u16,
    pub item: u8,
    pub chance: u8,
}

/// Read every stealable monster's current steal (item + chance) out of the
/// static `SCUS_942.54` steal table (`DAT_80077828`). Non-stealable monsters
/// (`item == 0` or `chance == 0`) are omitted. Purely read-only - the audit
/// surface for deciding what a steal randomization would change.
pub fn current_steals(patcher: &DiscPatcher) -> Result<Vec<StealSite>> {
    let edits = crate::steal::StealEdits::locate(patcher.image())
        .context("locate SCUS_942.54 steal table")?;
    Ok(edits
        .current()
        .into_iter()
        .map(|c| StealSite {
            monster_id: c.monster_id,
            item: c.item,
            chance: c.chance,
        })
        .collect())
}

/// Outcome of randomizing per-monster steal items.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StealApplyReport {
    /// Steal-item bytes actually written (no-op reassignments are skipped).
    pub items_changed: usize,
    /// Stealable monsters considered for reassignment.
    pub monsters: usize,
}

/// Randomize the per-monster steal items in place. The steal table is a static
/// `SCUS_942.54` table, so each edit is a single same-size byte overwrite of the
/// item (the steal *chance* is preserved) - no re-pack, nothing skipped.
/// `Shuffle` redistributes the existing steal-item multiset among the stealable
/// monsters; `Random` draws each item from `item_pool`. Returns the plan plus
/// the apply report.
pub fn randomize_steals(
    patcher: &mut DiscPatcher,
    item_pool: &[u8],
    seed: u64,
    mode: DropMode,
) -> Result<(Vec<DropAssignment>, StealApplyReport)> {
    let edits = crate::steal::StealEdits::locate(patcher.image())
        .context("locate SCUS_942.54 steal table")?;
    let plan = edits.plan(item_pool, seed, mode);
    let monsters = plan.len();
    let patches = edits.item_patches(&plan);
    let mut report = StealApplyReport {
        items_changed: 0,
        monsters,
    };
    for (off, item) in patches {
        patcher
            .patch_named_file(crate::steal::SCUS_NAME, off, &[item])
            .with_context(|| format!("write steal item at SCUS offset {off:#x}"))?;
        report.items_changed += 1;
    }
    Ok((plan, report))
}

/// One art's current button combo, for the read-only `arts` listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtSite {
    pub character: legaia_art::queue::Character,
    pub index: u8,
    pub ap: u8,
    /// Decoded combo (separator marker stripped).
    pub commands: Vec<legaia_art::queue::Command>,
    pub is_miracle: bool,
}

/// Read every Tactical Art's current button combo out of the static
/// `SCUS_942.54` arts-name table (`DAT_80075EC4`). Purely read-only - the audit
/// surface for what an arts-combo randomization would change. Includes the
/// per-character Miracle Art rows (flagged `is_miracle`), which the randomizer
/// leaves untouched.
pub fn current_arts(patcher: &DiscPatcher) -> Result<Vec<ArtSite>> {
    let edits = crate::arts::ArtsEdits::locate(patcher.image())
        .context("locate SCUS_942.54 arts-name table")?;
    Ok(edits
        .current()
        .into_iter()
        .map(|c| ArtSite {
            character: c.character,
            index: c.index,
            ap: c.ap,
            commands: c.commands,
            is_miracle: c.is_miracle,
        })
        .collect())
}

/// Outcome of randomizing Tactical-Arts button combos.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ArtsApplyReport {
    /// `+8` command pointers actually rewritten (no-op reassignments skipped).
    pub combos_changed: usize,
    /// Regular (non-Miracle) arts considered for reassignment.
    pub arts: usize,
}

/// Randomize each art's button combo by rewriting its directional **glyph
/// bytes in place** (same-size 2-byte SCUS edits - no re-pack, nothing
/// skipped). The bytes are the single copy both the Arts-menu display and the
/// in-battle input matcher read, so the trigger and the display stay in sync.
/// Input counts are preserved and each character's combos stay unique; the
/// per-character Miracle Art strings are left untouched. `Shuffle` permutes the
/// existing combos among same-length strings; `Random` writes fresh same-length
/// combos. Returns the plan plus the apply report.
pub fn randomize_arts(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: crate::arts::ArtsMode,
) -> Result<(Vec<crate::arts::ComboEdit>, ArtsApplyReport)> {
    let edits = crate::arts::ArtsEdits::locate(patcher.image())
        .context("locate SCUS_942.54 arts-name table")?;
    let plan = edits.plan(seed, mode);
    let report = ArtsApplyReport {
        combos_changed: edits.arts_changed(&plan),
        arts: edits.regular_art_count(),
    };
    // 1. The in-battle/menu input MATCHER: rewrite the 1-4 combo in each
    //    character's player-data record0 (the bytes the trigger actually reads).
    for ch in legaia_art::queue::Character::all() {
        let char_edits = edits.player_edits(&plan, ch);
        if char_edits.is_empty() {
            continue;
        }
        let index = crate::arts::player_entry_index(ch);
        let entry = patcher
            .read_entry(index)
            .with_context(|| format!("read player file PROT {index}"))?;
        if let Some((lzs_off, recompressed)) =
            crate::arts::patch_player_record0(&entry, &char_edits)
        {
            patcher
                .patch_prot_entry(index, lzs_off as u64, &recompressed)
                .with_context(|| format!("write player file PROT {index} record0"))?;
        }
    }
    // 2. The Arts-menu DISPLAY: rewrite the SCUS glyph string to the same combo
    //    so the shown arrows match the (now-patched) trigger.
    for (off, bytes) in edits.glyph_patches(&plan) {
        patcher
            .patch_named_file(crate::arts::SCUS_NAME, off, &bytes)
            .with_context(|| format!("write art combo glyph at SCUS offset {off:#x}"))?;
    }
    Ok((plan, report))
}
