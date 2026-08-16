//! Show Super Arts on the in-battle Tactical-Arts list. See
//! [`crate::super_art_list`] for the four hook sites, what "unlocked" means, and
//! why a row draws a name and an AP cost but no arrows.

use super::*;

use legaia_art::queue::Character;

/// One Super Art as the injection will present it, for reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuperArtListRow {
    pub character: Character,
    pub name: String,
    /// The chain arts that have to be learned before the row appears, named as
    /// this disc's own arts-name table spells them.
    pub chain: Vec<String>,
    /// The chain's summed AP cost - what the row displays.
    pub ap: u8,
}

/// Outcome of enabling the Super Arts list rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuperArtListReport {
    /// The fifteen rows, in `character * 5 + k` order.
    pub rows: Vec<SuperArtListRow>,
    /// Where the injected code and tables landed (for the oracle / manifest).
    pub sub_va: u32,
    pub count_va: u32,
    pub id_va: u32,
    pub rec_va: u32,
    pub masktab_va: u32,
    pub scratch_va: u32,
}

impl SuperArtListReport {
    /// The fifteen display names, in table order.
    pub fn names(&self) -> Vec<String> {
        self.rows.iter().map(|r| r.name.clone()).collect()
    }
}

/// Inject the **show Super Arts** feature: put each of Vahn's, Noa's and Gala's
/// unlocked Super Arts at the head of the arts list the Triangle button opens in
/// battle, which retail draws not at all. A row appears only once every art in
/// that Super Art's trigger chain is in the character's learned-art list, and it
/// carries the Super Art's name plus the chain's summed AP cost.
///
/// Three same-size detours into the SCUS list renderer `FUN_80034358` plus their
/// routines, four small tables and a scratch record in verified-dead SCUS
/// regions, and a wholesale in-place replacement of the list pager
/// `FUN_801D3748` in the battle-action overlay (PROT 0898) so the page offset
/// can reach the added rows.
///
/// **Mutually exclusive with `--shiny-seru`, `--arts-ap-grant` / `--arts-ap-cost`
/// and `--delilas-challenge`** - they all reuse the same dead-space bytes. Fails
/// (without touching the disc) if the build isn't the recognized US layout, a
/// hosted region isn't dead space, a trigger chain doesn't resolve against this
/// disc's own arts-name table, or a Super Art's record can't be located in the
/// player battle file the runtime name chase will read.
pub fn inject_super_art_list(patcher: &mut DiscPatcher) -> Result<SuperArtListReport> {
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for show-super-arts injection")?;
    let ov0898 = patcher
        .read_entry(crate::super_art_list::OVERLAY_PROT_INDEX)
        .context("read battle-action overlay (0898) for show-super-arts injection")?;
    let plan = crate::super_art_list::SuperArtListInjection::plan(&scus, &ov0898)?;

    // The row's name is chased at runtime through
    // `DAT_801C9360[char] -> +0x58 -> +4 -> (finisher - 0x10) * 0xD0 -> +0x10`.
    // The same arithmetic off the player battle file's decoded `record0` is what
    // `super_art_power` locates a Super Art's record with, and it only returns a
    // row when that record's `+0x10` name matches the Super Art's own name - so
    // running it here proves, on the user's disc, that the chase lands on the
    // right record before a single byte is written.
    for ch in Character::all() {
        let index = crate::super_art_power::player_entry_index(ch);
        let entry = patcher
            .read_entry(index)
            .with_context(|| format!("read player battle file PROT {index}"))?;
        let located =
            crate::super_art_power::super_art_powers(&scus, &entry, ch).unwrap_or_default();
        for row in plan.rows.iter().filter(|r| r.character == ch) {
            if !located.iter().any(|l| l.finisher == row.finisher) {
                anyhow::bail!(
                    "show-super-arts: {:?}'s Super Art {} does not carry its name at the \
                     record the runtime name chase resolves (PROT {index} record0, finisher \
                     {:#x}) - unrecognized build, nothing written",
                    ch,
                    row.name,
                    row.finisher
                );
            }
        }
    }

    for edit in &plan.edits {
        match edit.prot_index {
            None => patcher
                .patch_named_file(SCUS_NAME, edit.file_off as u64, &edit.bytes)
                .with_context(|| {
                    format!("write show-super-arts SCUS edit at {:#x}", edit.file_off)
                })?,
            Some(idx) => patcher
                .patch_prot_entry(idx, edit.file_off as u64, &edit.bytes)
                .with_context(|| {
                    format!(
                        "write show-super-arts PROT {idx} edit at {:#x}",
                        edit.file_off
                    )
                })?,
        }
    }

    Ok(SuperArtListReport {
        rows: plan
            .rows
            .iter()
            .map(|r| SuperArtListRow {
                character: r.character,
                name: r.name.to_string(),
                chain: r.chain_names.clone(),
                ap: r.ap,
            })
            .collect(),
        sub_va: plan.sub_va,
        count_va: plan.count_va,
        id_va: plan.id_va,
        rec_va: plan.rec_va,
        masktab_va: plan.masktab_va,
        scratch_va: plan.scratch_va,
    })
}
