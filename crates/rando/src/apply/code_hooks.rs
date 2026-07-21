//! MIPS-detour code-hook + config-blob injections (bonus drop, flee-EXP, enemy-ally charm, shiny Seru, seru-trade config, Seru Bell name).

use super::*;

/// Outcome of injecting the bonus equipment drop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquipmentDropReport {
    /// Number of equipment ids embedded in the injected table (the gear the
    /// extra drop can roll).
    pub table_len: usize,
    /// The low-chance gate the routine rolls (percent, once per battle).
    pub chance_pct: u8,
}

/// Inject the **additive** bonus equipment drop (see [`crate::bonus_drop`]): a
/// code hook into the battle-end reward routine that, on a `chance_pct` roll
/// once per battle, grants one extra random equipment id picked from the disc's
/// own equipment pool - *on top of* the normal drop, which is left untouched.
///
/// Two same-size `SCUS_942.54` edits: the detour at the reward-routine hook and
/// the routine + id-table blob in preserved rodata padding. Fails (without
/// touching the disc) if the build isn't the recognized US layout.
pub fn inject_equipment_bonus_drop(
    patcher: &mut DiscPatcher,
    chance_pct: u8,
) -> Result<EquipmentDropReport> {
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for equipment-drop injection")?;
    let ids = crate::equipment::equipment_ids(&scus).context("build equipment id pool")?;
    let plan = crate::bonus_drop::BonusDropInjection::plan(&scus, &ids, chance_pct)?;

    // Detour at the hook site, then the routine + table blob.
    let detour: Vec<u8> = crate::bonus_drop::detour_words()
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    patcher
        .patch_named_file(SCUS_NAME, plan.hook_off as u64, &detour)
        .context("write reward-routine detour")?;
    patcher
        .patch_named_file(SCUS_NAME, plan.blob_off as u64, &plan.blob)
        .context("write injected routine + equipment table")?;

    Ok(EquipmentDropReport {
        table_len: plan.table_len,
        chance_pct: plan.chance_pct,
    })
}

/// Outcome of injecting the run-away EXP reward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FleeExpReport {
    /// Percentage of the formation's experience banked into each party member on a
    /// successful escape.
    pub pct: u8,
}

/// Inject the **run-away EXP reward** (see [`crate::flee_exp`]): a code hook into
/// the battle-action escape teardown that, whenever the party successfully flees,
/// banks `pct`% of the fled formation's experience into every party member's
/// cumulative-XP cell. Vanilla awards nothing for running.
///
/// Two same-size edits: the detour at the escape-teardown hook (the battle-action
/// overlay's raw PROT entry) and the routine blob in preserved `SCUS_942.54`
/// rodata padding (placed past the bonus-equipment routine so both hooks coexist).
/// Fails (without touching the disc) if the build isn't the recognized US layout.
pub fn inject_flee_exp(patcher: &mut DiscPatcher, pct: u8) -> Result<FleeExpReport> {
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for flee-EXP injection")?;
    let overlay = patcher
        .read_entry(crate::flee_exp::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .context("read battle-action overlay for flee-EXP injection")?;
    let plan = crate::flee_exp::FleeExpInjection::plan(&scus, &overlay, pct)?;

    // The escape-teardown detour lives in the overlay PROT entry (raw, linear
    // from base); the routine blob lives in the SCUS rodata gap.
    let detour: Vec<u8> = plan.detour.iter().flat_map(|w| w.to_le_bytes()).collect();
    patcher
        .patch_prot_entry(
            crate::flee_exp::BATTLE_ACTION_OVERLAY_PROT_INDEX,
            plan.overlay_hook_off as u64,
            &detour,
        )
        .context("write escape-teardown detour")?;
    patcher
        .patch_named_file(SCUS_NAME, plan.routine_off as u64, &plan.blob)
        .context("write injected flee-EXP routine")?;

    Ok(FleeExpReport { pct: plan.pct })
}

/// Outcome of injecting the enemy-ally ("charm") feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyAllyReport {
    /// Per-battle probability (percent) that an enemy is charmed onto the party's
    /// side.
    pub pct: u8,
}

/// Inject the **enemy-ally ("charm")** feature (see [`crate::enemy_ally`]): a code
/// hook into battle setup that, with `pct`% probability per battle, flags the
/// frontmost enemy with the AI-delegated bits (`+0x16E |= 0x380`) so it fights on
/// the player's side - an uncontrolled ally that can appear in any fight,
/// including bosses. A companion one-word widen of the victory check stops a
/// charmed enemy from counting as an enemy you must defeat.
///
/// The victory-mask widen desyncs the state-`0x5A` wipe scan from the initiative
/// scheduler, which can let a living charmed monster be the acting actor at
/// victory and drive the win-pose staging out of bounds (the charm battle
/// softlock). So this **always** applies the [`crate::charm_fix`] guard alongside
/// the charm edits: the widen and its softlock fix ship together.
///
/// Five same-size edits: the setup detour + the routine blob in preserved
/// `SCUS_942.54` rodata padding, and the victory-mask widen in the battle-action
/// overlay's raw PROT entry (the charm feature); plus the victory-arm guard
/// detour (overlay) + guard blob (SCUS) that closes the softlock. Fails (without
/// touching the disc) if the build isn't the recognized US layout.
pub fn inject_enemy_ally(patcher: &mut DiscPatcher, pct: u8) -> Result<EnemyAllyReport> {
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for enemy-ally injection")?;
    let overlay = patcher
        .read_entry(crate::enemy_ally::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .context("read battle-action overlay for enemy-ally injection")?;
    let plan = crate::enemy_ally::EnemyAllyInjection::plan(&scus, &overlay, pct)?;
    // Plan the softlock guard against the *pristine* build before any write, so a
    // partial patch never lands (both features validate the known US layout first).
    let fix = crate::charm_fix::CharmVictoryFix::plan(&scus, &overlay)?;

    // Setup detour + routine live in SCUS; the victory-mask widen lives in the
    // battle-action overlay PROT entry (raw, linear from base).
    let detour: Vec<u8> = plan.detour.iter().flat_map(|w| w.to_le_bytes()).collect();
    patcher
        .patch_named_file(SCUS_NAME, plan.scus_hook_off as u64, &detour)
        .context("write battle-setup detour")?;
    patcher
        .patch_named_file(SCUS_NAME, plan.routine_off as u64, &plan.blob)
        .context("write injected enemy-ally routine")?;
    patcher
        .patch_prot_entry(
            crate::enemy_ally::BATTLE_ACTION_OVERLAY_PROT_INDEX,
            plan.overlay_victory_off as u64,
            &plan.victory_word.to_le_bytes(),
        )
        .context("write victory-mask widen")?;

    // Softlock fix: the one-word overlay detour + the guard blob in SCUS.
    patcher
        .patch_prot_entry(
            crate::charm_fix::BATTLE_ACTION_OVERLAY_PROT_INDEX,
            fix.overlay_hook_off as u64,
            &fix.detour.to_le_bytes(),
        )
        .context("write charm victory-arm guard detour")?;
    patcher
        .patch_named_file(SCUS_NAME, fix.routine_off as u64, &fix.blob)
        .context("write charm victory-arm guard routine")?;

    Ok(EnemyAllyReport { pct: plan.pct })
}

/// Outcome of enabling shiny Seru.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShinySeruReport {
    /// Per-battle probability (percent) that a capturable enemy spawns shiny.
    pub pct: u8,
}

/// Inject the **shiny Seru** feature (see [`crate::shiny_seru`]): with `pct`%
/// probability per battle, the frontmost *capturable* enemy spawns with +35%
/// stats; capturing it flags the learned Seru so its spell deals +35% damage
/// forever (the flag persists in the spell-level byte's high bit and is masked
/// from the level-up + menu readers).
///
/// Eight same-size detours + their routines, split between a new preserved
/// `SCUS_942.54` rodata gap and the battle-action overlay's move-power padding
/// (both reference-free, and disjoint from every other gap feature so they
/// compose). Fails (without touching the disc) if the build isn't the
/// recognized US layout or a routine region isn't dead space.
pub fn inject_shiny_seru(patcher: &mut DiscPatcher, pct: u8) -> Result<ShinySeruReport> {
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for shiny-seru injection")?;
    let ov0898 = patcher
        .read_entry(crate::shiny_seru::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .context("read battle-action overlay (0898) for shiny-seru injection")?;
    let ov0899 = patcher
        .read_entry(crate::shiny_seru::MENU_OVERLAY_PROT_INDEX)
        .context("read menu overlay (0899) for shiny-seru injection")?;
    // Derive the capturable-Seru monster ids from the disc's monster names so
    // the allowlist tracks the actual `battle_data` archive (no hardcoded ids).
    let archive = patcher
        .read_entry(MONSTER_ARCHIVE_ENTRY)
        .context("read monster battle_data archive for shiny-seru allowlist")?;
    let capturable = crate::shiny_seru::capturable_monster_ids(&archive)
        .context("derive capturable-Seru ids")?;
    let plan =
        crate::shiny_seru::ShinySeruInjection::plan(&scus, &ov0898, &ov0899, pct, &capturable)?;

    for edit in &plan.edits {
        match edit.prot_index {
            None => patcher
                .patch_named_file(SCUS_NAME, edit.file_off as u64, &edit.bytes)
                .with_context(|| format!("write shiny-seru SCUS edit at {:#x}", edit.file_off))?,
            Some(idx) => patcher
                .patch_prot_entry(idx, edit.file_off as u64, &edit.bytes)
                .with_context(|| {
                    format!("write shiny-seru PROT {idx} edit at {:#x}", edit.file_off)
                })?,
        }
    }

    Ok(ShinySeruReport { pct: plan.pct })
}

/// Outcome of enabling seru trading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeruTradeReport {
    /// The config written to the disc (enabled flag, master seed, offer cap).
    pub config: legaia_asset::seru_trade::SeruTradeConfig,
}

/// Enable **seru trading** (see [`crate::seru_trade`]): write a small config blob
/// (enabled flag + master `seed` + per-vendor offer cap) into preserved
/// `SCUS_942.54` rodata padding. The clean-room engine reads the blob and, at
/// runtime, lets vendors offer to swap one of a character's seru for a different
/// one - the offers reseeding every two in-game hours from the same seed.
///
/// A single same-size, in-place edit. Re-running with a new seed overwrites the
/// prior blob. Fails (without touching the disc) if the build isn't the
/// recognized layout (the target rodata region isn't dead space).
pub fn enable_seru_trades(
    patcher: &mut DiscPatcher,
    seed: u64,
    max_offers: u8,
) -> Result<SeruTradeReport> {
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for seru-trade config")?;
    let config = legaia_asset::seru_trade::SeruTradeConfig {
        enabled: true,
        seed,
        max_offers: max_offers.max(1),
    };
    let plan = crate::seru_trade::SeruTradePlan::plan(&scus, config)?;
    patcher
        .patch_named_file(SCUS_NAME, plan.config_off as u64, &plan.blob)
        .context("write seru-trade config blob")?;
    Ok(SeruTradeReport {
        config: plan.config,
    })
}

/// Read back the seru-trade config currently on the disc (`None` if seru trading
/// isn't enabled / no blob is present). Used by the read-only listing and the
/// round-trip oracle.
pub fn current_seru_trade(
    patcher: &DiscPatcher,
) -> Option<legaia_asset::seru_trade::SeruTradeConfig> {
    let scus = patcher.read_named_file(SCUS_NAME)?;
    legaia_asset::seru_trade::SeruTradeConfig::from_scus(&scus)
}

/// Give the unnamed accessory (item `0xFD`) the display name **"Seru Bell"** so
/// the `--unused-items` toggle hands out a presentable item instead of a blank.
/// Writes the name into reclaimable `SCUS_942.54` tail space and repoints only
/// `0xFD`'s name pointer (the other ids sharing the empty-string slot keep it).
///
/// Idempotent: if `0xFD` already resolves to a name (e.g. on an
/// already-patched image) it does nothing. Returns the name that was set, or
/// `None` if it was already named or the SCUS layout couldn't be resolved.
pub fn inject_seru_bell_name(patcher: &mut DiscPatcher) -> Result<Option<String>> {
    use crate::item_name::{NameInjection, SERU_BELL_ID, SERU_BELL_NAME};
    let scus = patcher
        .read_named_file(crate::steal::SCUS_NAME)
        .context("read SCUS_942.54")?;
    // Skip if it already has a name (don't stack injections on re-runs).
    if let Some(table) = legaia_asset::item_names::ItemNameTable::from_scus(&scus)
        && table.name(SERU_BELL_ID).is_some()
    {
        return Ok(None);
    }
    let Some(plan) = NameInjection::plan(&scus, SERU_BELL_ID, SERU_BELL_NAME) else {
        return Ok(None);
    };
    // Two same-size writes: the string bytes, then the repointed pointer word.
    patcher
        .patch_named_file(
            crate::steal::SCUS_NAME,
            plan.string_file_off as u64,
            &plan.name_bytes,
        )
        .context("write Seru Bell name string")?;
    patcher
        .patch_named_file(
            crate::steal::SCUS_NAME,
            plan.ptr_file_off as u64,
            &plan.string_va.to_le_bytes(),
        )
        .context("repoint accessory 0xFD name pointer")?;
    Ok(Some(SERU_BELL_NAME.to_string()))
}
