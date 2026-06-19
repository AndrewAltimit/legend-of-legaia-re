//! High-level orchestration: read the current gameplay data off a disc, plan a
//! randomization from a seed, and write the plan back into a [`DiscPatcher`].
//!
//! This is the glue the top-level CLI drives. It keeps the per-module logic
//! (drop planning, slot re-pack, sector write-back) decoupled and testable while
//! giving the binary a single call per feature. It embeds no game bytes - every
//! value it reads comes from the user's own disc image at runtime.

use anyhow::{Context, Result};

use crate::casino::{self, CasinoExchange};
use crate::chest::SceneChests;
use crate::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};
use crate::door::SceneDoors;
use crate::drops::{CurrentDrop, DropAssignment, DropMode, plan_drops};
use crate::encounter::SceneEncounters;
use crate::house_door::SceneHouseDoors;
use crate::monster_stats;
use crate::rng::SplitMix64;
use crate::shop::SceneShops;

/// Read every monster's current drop (item id + chance) out of the
/// `battle_data` archive (PROT entry 867). Monsters with no drop are included
/// with `item == 0` so the planner can skip them consistently.
pub fn current_drops(patcher: &DiscPatcher) -> Result<Vec<CurrentDrop>> {
    let entry = patcher
        .read_entry(MONSTER_ARCHIVE_ENTRY)
        .context("read monster battle_data archive")?;
    let records =
        legaia_asset::monster_archive::records(&entry).context("decode monster archive records")?;
    Ok(records
        .iter()
        .map(|r| CurrentDrop {
            monster_id: r.id,
            item: r.drop_item,
            chance: r.drop_chance_pct,
        })
        .collect())
}

/// One treasure-chest give-item site: which scene bundle it lives in, the byte
/// offset of its `GIVE_ITEM` (`0x39`) operand inside the decoded MAN, and the
/// item id it currently grants. This is the population the chest randomizer
/// reassigns; listing it lets a user audit which items would change (e.g. to
/// keep quest items static).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChestSite {
    /// PROT entry index of the scene bundle holding this chest.
    pub entry_idx: usize,
    /// Byte offset of the give operand within the scene's decoded MAN. Stable
    /// per disc; identifies the site independent of item id.
    pub man_offset: usize,
    /// The item id the chest currently gives.
    pub item: u8,
}

/// Read every treasure-chest give-item site on the disc (the randomizable
/// population), in PROT-entry order. Mirrors [`current_drops`] for chests:
/// purely read-only, decodes each scene MAN once via [`SceneChests::locate`].
pub fn current_chests(patcher: &DiscPatcher) -> Result<Vec<ChestSite>> {
    let mut out = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(sc) = SceneChests::locate(&entry, idx) else {
            continue;
        };
        let items = sc.current_items();
        for (k, &off) in sc.sites.iter().enumerate() {
            out.push(ChestSite {
                entry_idx: idx,
                man_offset: off,
                item: items[k],
            });
        }
    }
    Ok(out)
}

/// Outcome of applying a drop plan.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DropApplyReport {
    /// Slots actually written.
    pub changed: usize,
    /// Monsters whose re-packed slot would not fit, so the edit was skipped
    /// (the original drop is kept). Our LZS re-packer isn't byte-identical to
    /// Sony's, so a record already near the `0x14000` slot limit can rarely
    /// overflow; skipping keeps the rest of the patch valid. See
    /// [`crate::monster`].
    pub skipped: Vec<u16>,
}

/// Apply a planned drop table to the disc image. Each assignment re-packs that
/// monster's slot in place (decompress -> set drop bytes -> recompress -> sector
/// write-back). A slot whose re-packed stream would overflow is skipped (and
/// recorded in the report) rather than aborting the whole run.
pub fn apply_drop_plan(
    patcher: &mut DiscPatcher,
    plan: &[DropAssignment],
) -> Result<DropApplyReport> {
    let mut report = DropApplyReport::default();
    for a in plan {
        let slot = patcher
            .monster_slot(a.monster_id)
            .with_context(|| format!("read monster {} slot", a.monster_id))?;
        let new_slot = match crate::monster::set_drop(&slot, a.item, a.chance) {
            Ok(s) => s,
            Err(_) => {
                // The only expected failure here is the slot-overflow guard;
                // a malformed slot would have failed in `current_drops` already.
                report.skipped.push(a.monster_id);
                continue;
            }
        };
        if new_slot != slot {
            patcher
                .patch_monster_slot(a.monster_id, &new_slot)
                .with_context(|| format!("write monster {} slot", a.monster_id))?;
            report.changed += 1;
        }
    }
    Ok(report)
}

/// Plan and apply a drop randomization in one call. `item_pool` is only needed
/// for [`DropMode::Random`] (shuffle ignores it); pass an empty slice otherwise.
/// Returns the plan that was generated plus the apply report.
pub fn randomize_drops(
    patcher: &mut DiscPatcher,
    item_pool: &[u8],
    seed: u64,
    mode: DropMode,
) -> Result<(Vec<DropAssignment>, DropApplyReport)> {
    let current = current_drops(patcher)?;
    let plan = plan_drops(&current, item_pool, seed, mode);
    let report = apply_drop_plan(patcher, &plan)?;
    Ok((plan, report))
}

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
/// Three same-size edits: the setup detour + the routine blob in preserved
/// `SCUS_942.54` rodata padding, and the victory-mask widen in the battle-action
/// overlay's raw PROT entry. Fails (without touching the disc) if the build isn't
/// the recognized US layout.
pub fn inject_enemy_ally(patcher: &mut DiscPatcher, pct: u8) -> Result<EnemyAllyReport> {
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for enemy-ally injection")?;
    let overlay = patcher
        .read_entry(crate::enemy_ally::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .context("read battle-action overlay for enemy-ally injection")?;
    let plan = crate::enemy_ally::EnemyAllyInjection::plan(&scus, &overlay, pct)?;

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

    Ok(EnemyAllyReport { pct: plan.pct })
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

/// Outcome of injecting the custom-overlay vertical slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlaySliceReport {
    /// PROT entry (pochi slot) the custom overlay was written into.
    pub pochi_index: usize,
    /// Absolute disc LBA baked into the loader stub.
    pub lba: u32,
    /// Sectors the stub loads.
    pub sectors: u16,
}

/// Find a pochi-filler PROT slot whose on-disc footprint can hold `need_bytes`
/// (the "pochi" magic head marks reserved dev filler - safe to overwrite). Picks
/// the largest such slot (most headroom, deterministic by max-footprint then
/// lowest index). `None` if none qualifies.
fn find_pochi_host(patcher: &DiscPatcher, need_bytes: usize) -> Option<usize> {
    let mut best: Option<(u64, usize)> = None;
    for idx in 0..patcher.entry_count() {
        let Some(fp) = patcher.entry_footprint(idx) else {
            continue;
        };
        if (fp as usize) < need_bytes {
            continue;
        }
        let Ok(head) = patcher.read_entry(idx) else {
            continue;
        };
        if head.len() >= 5 && &head[0..5] == b"pochi" {
            let key = (fp, idx);
            if best.is_none_or(|(bf, bi)| fp > bf || (fp == bf && idx < bi)) {
                best = Some(key);
            }
        }
    }
    best.map(|(_, idx)| idx)
}

/// Inject the **custom-overlay vertical slice** (see [`crate::seru_overlay`]):
/// proves the retail custom-overlay load path end to end, triggered by **opening
/// a shop**. Overwrites a pochi slot with a tiny sentinel-writing overlay, bakes
/// a gap loader stub with that slot's real disc LBA, and detours the field-VM
/// op-0x49 arm edge (overlay 0897) into the stub. The stub gates on the sub-op
/// (only a merchant, sub-op `0`), FlushCaches, runs the overlay (which writes
/// [`crate::seru_overlay::SENTINEL`] to [`crate::seru_overlay::SENTINEL_ADDR`]),
/// then resumes the field VM - so the load fires when the player opens a vendor.
/// No Sony bytes.
pub fn inject_overlay_slice(patcher: &mut DiscPatcher) -> Result<OverlaySliceReport> {
    inject_overlay_slice_opts(patcher, true)
}

/// As [`inject_overlay_slice`], but `gated` selects whether the op-`0x49` stub's
/// sub-op gate is live (see [`crate::seru_overlay::assemble_shop_loader_stub_gated`]).
/// `gated = false` is the diagnostic build that fires on every op-`0x49` arm.
pub fn inject_overlay_slice_opts(
    patcher: &mut DiscPatcher,
    gated: bool,
) -> Result<OverlaySliceReport> {
    use crate::seru_overlay as ov;

    let overlay = ov::words_to_bytes(&ov::assemble_sentinel_overlay());
    let sectors = ov::sectors_for(overlay.len());

    // 1. Pick + overwrite a pochi host slot with the overlay.
    let pochi_index = find_pochi_host(patcher, overlay.len())
        .ok_or_else(|| anyhow::anyhow!("no pochi-filler slot large enough for the overlay"))?;
    let lba = patcher
        .entry_disc_lba(pochi_index)
        .ok_or_else(|| anyhow::anyhow!("pochi slot {pochi_index} has no disc LBA"))?;
    patcher
        .patch_prot_entry(pochi_index, 0, &overlay)
        .with_context(|| format!("write overlay into pochi slot {pochi_index}"))?;

    // 2. Bake the shop-gated loader stub into the preserved SCUS rodata gap.
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for overlay-slice stub")?;
    let stub = ov::words_to_bytes(&ov::assemble_shop_loader_stub_gated(lba, sectors, gated));
    let stub_off = legaia_asset::item_names::file_offset_for_va(&scus, ov::STUB_VA)
        .ok_or_else(|| anyhow::anyhow!("can't resolve stub VA {:#x} in SCUS", ov::STUB_VA))?;
    if scus
        .get(stub_off..stub_off + stub.len())
        .is_none_or(|r| r.iter().any(|&b| b != 0))
    {
        anyhow::bail!("stub region {:#x} is not all-zero dead space", ov::STUB_VA);
    }
    patcher
        .patch_named_file(SCUS_NAME, stub_off as u64, &stub)
        .context("write overlay-slice loader stub")?;

    // 3. Detour the field-VM op-0x49 arm edge (overlay 0897, raw - maps linearly
    //    from its base). Guard the displaced pair matches the recognized build.
    let overlay_entry = patcher
        .read_entry(ov::SHOP_OVERLAY_PROT_INDEX)
        .with_context(|| format!("read field overlay PROT {}", ov::SHOP_OVERLAY_PROT_INDEX))?;
    let hook_off = (ov::SHOP_HOOK_VA - ov::SHOP_OVERLAY_BASE) as usize;
    let at_hook: Vec<u32> = overlay_entry
        .get(hook_off..hook_off + 8)
        .ok_or_else(|| anyhow::anyhow!("hook offset past end of overlay entry"))?
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    if at_hook[..] != ov::SHOP_DISPLACED[..] {
        anyhow::bail!(
            "op-0x49 hook site does not match the recognized US build; refusing to patch"
        );
    }
    let detour: Vec<u8> = ov::detour_words()
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    patcher
        .patch_prot_entry(ov::SHOP_OVERLAY_PROT_INDEX, hook_off as u64, &detour)
        .context("write op-0x49 shop detour into the field overlay")?;

    Ok(OverlaySliceReport {
        pochi_index,
        lba,
        sectors,
    })
}

/// As [`inject_overlay_slice`], but hosts the overlay via the **dead dev-mode**
/// path (option 1): the op-0x49 detour only REQUESTS a repurposed dev game-mode
/// ([`ov::DEAD_MODE_INDEX`]), and that mode's INIT handler (our gap loader) does
/// the CD load in the safe between-frames context - avoiding the mid-tick
/// reentrancy that froze the raw stub. Edits: the pochi overlay, two gap
/// routines (trigger + mode-INIT loader), the op-0x49 detour, and the dead
/// mode's mode-table handler word (guarded against an unexpected build). All 7
/// minigames stay intact. No Sony bytes.
pub fn inject_overlay_slice_dead_mode(patcher: &mut DiscPatcher) -> Result<OverlaySliceReport> {
    use crate::seru_overlay as ov;

    let overlay = ov::words_to_bytes(&ov::assemble_sentinel_overlay());
    let sectors = ov::sectors_for(overlay.len());

    // 1. Pochi host + overlay.
    let pochi_index = find_pochi_host(patcher, overlay.len())
        .ok_or_else(|| anyhow::anyhow!("no pochi-filler slot large enough for the overlay"))?;
    let lba = patcher
        .entry_disc_lba(pochi_index)
        .ok_or_else(|| anyhow::anyhow!("pochi slot {pochi_index} has no disc LBA"))?;
    patcher
        .patch_prot_entry(pochi_index, 0, &overlay)
        .with_context(|| format!("write overlay into pochi slot {pochi_index}"))?;

    // 2. Gap routines: trigger @ TRIGGER_VA, mode-INIT loader @ MODE_INIT_VA.
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for dead-mode slice")?;
    let trigger = ov::words_to_bytes(&ov::assemble_mode_request_trigger());
    let loader = ov::words_to_bytes(&ov::assemble_mode_init_loader_stub(lba, sectors));
    let resolve_gap = |va: u32, len: usize| -> Result<usize> {
        let off = legaia_asset::item_names::file_offset_for_va(&scus, va)
            .ok_or_else(|| anyhow::anyhow!("can't resolve gap VA {va:#x} in SCUS"))?;
        if scus
            .get(off..off + len)
            .is_none_or(|r| r.iter().any(|&b| b != 0))
        {
            anyhow::bail!("gap region {va:#x} is not all-zero dead space");
        }
        Ok(off)
    };
    let trig_off = resolve_gap(ov::TRIGGER_VA, trigger.len())?;
    let load_off = resolve_gap(ov::MODE_INIT_VA, loader.len())?;
    patcher
        .patch_named_file(SCUS_NAME, trig_off as u64, &trigger)
        .context("write dead-mode trigger")?;
    patcher
        .patch_named_file(SCUS_NAME, load_off as u64, &loader)
        .context("write dead-mode mode-INIT loader")?;

    // 3. Detour the op-0x49 arm edge (overlay 0897) into the trigger.
    let overlay_entry = patcher
        .read_entry(ov::SHOP_OVERLAY_PROT_INDEX)
        .with_context(|| format!("read field overlay PROT {}", ov::SHOP_OVERLAY_PROT_INDEX))?;
    let hook_off = (ov::SHOP_HOOK_VA - ov::SHOP_OVERLAY_BASE) as usize;
    let at_hook: Vec<u32> = overlay_entry
        .get(hook_off..hook_off + 8)
        .ok_or_else(|| anyhow::anyhow!("hook offset past end of overlay entry"))?
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    if at_hook[..] != ov::SHOP_DISPLACED[..] {
        anyhow::bail!(
            "op-0x49 hook site does not match the recognized US build; refusing to patch"
        );
    }
    let detour: Vec<u8> = ov::detour_words()
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    patcher
        .patch_prot_entry(ov::SHOP_OVERLAY_PROT_INDEX, hook_off as u64, &detour)
        .context("write op-0x49 dead-mode detour into the field overlay")?;

    // 4. Repurpose the dead mode's mode-table handler -> our loader (guarded).
    let handler_va = ov::dead_mode_handler_va();
    let handler_off = legaia_asset::item_names::file_offset_for_va(&scus, handler_va)
        .ok_or_else(|| anyhow::anyhow!("can't resolve mode-table handler VA {handler_va:#x}"))?;
    let cur = scus
        .get(handler_off..handler_off + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| anyhow::anyhow!("mode-table handler offset out of range"))?;
    if cur != ov::DEAD_MODE_HANDLER_ORIG {
        anyhow::bail!(
            "dead-mode handler is {cur:#010x}, expected {:#010x}; refusing to patch",
            ov::DEAD_MODE_HANDLER_ORIG
        );
    }
    patcher
        .patch_named_file(
            SCUS_NAME,
            handler_off as u64,
            &ov::MODE_INIT_VA.to_le_bytes(),
        )
        .context("repurpose dead-mode mode-table handler")?;

    Ok(OverlaySliceReport {
        pochi_index,
        lba,
        sectors,
    })
}

/// As [`inject_overlay_slice`], but hosts the overlay via the **mode-24 warp**
/// (Fork A, new sub-id): the op-0x49 shop detour mirrors the op-0x3E minigame
/// warp (request mode 24 + our sub-id), and `FUN_80025980`'s per-sub-id
/// overlay-load call is detoured so that, for our sub-id, it baked-LBA-loads our
/// pochi overlay to slot A and runs it, then returns to the field via the
/// mode-24 return warp (`FUN_80026018` -> mode 2 scene reload). This is the
/// game's own clean teardown+reload path, avoiding the resume-in-place freezes.
/// Edits: the pochi overlay, two gap routines (warp trigger + FUN_80025980
/// redirect), the op-0x49 detour, and the `FUN_80025980` load-site detour (both
/// recognized-build guarded). All 7 minigames stay intact. No Sony bytes.
pub fn inject_overlay_slice_warp(patcher: &mut DiscPatcher) -> Result<OverlaySliceReport> {
    inject_overlay_slice_warp_opts(patcher, false)
}

/// As [`inject_overlay_slice_warp`], but `draw` selects the payload: `false` =
/// the sentinel slice (load proof, immediate field reload); `true` = the draw-side
/// overlay ([`crate::seru_overlay::assemble_draw_overlay`]) that hands off to mode
/// 13 and renders on screen each frame before returning to the field.
pub fn inject_overlay_slice_warp_opts(
    patcher: &mut DiscPatcher,
    draw: bool,
) -> Result<OverlaySliceReport> {
    use crate::seru_overlay as ov;

    let overlay = if draw {
        ov::words_to_bytes(&ov::assemble_draw_overlay())
    } else {
        ov::words_to_bytes(&ov::assemble_sentinel_overlay())
    };
    let sectors = ov::sectors_for(overlay.len());

    // 1. Pochi host + overlay.
    let pochi_index = find_pochi_host(patcher, overlay.len())
        .ok_or_else(|| anyhow::anyhow!("no pochi-filler slot large enough for the overlay"))?;
    let lba = patcher
        .entry_disc_lba(pochi_index)
        .ok_or_else(|| anyhow::anyhow!("pochi slot {pochi_index} has no disc LBA"))?;
    patcher
        .patch_prot_entry(pochi_index, 0, &overlay)
        .with_context(|| format!("write overlay into pochi slot {pochi_index}"))?;

    // 2. Gap routines: warp trigger + FUN_80025980 overlay-load redirect.
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for warp slice")?;
    // Fire-once guard is UNRELIABLE: its flag cell (0x8007AF28) sits in the gap
    // tail the game reuses at runtime, so it can read "already fired" before our
    // first warp and skip it. The DRAW build avoids it entirely - its overlay holds
    // the draw mode indefinitely, so the warp never returns to re-trigger (no loop
    // even without fire-once). The sentinel build keeps fire-once only because its
    // round-trip returns; that path needs a reliable flag location later.
    // Draw build = the real feature: gate to shop sub-op 0 (fires at a merchant,
    // mid-game) and skip fire-once (shops don't auto-retrigger). Sentinel build:
    // ungated + fire-once for the name-entry mechanism test.
    let trigger = ov::words_to_bytes(&ov::assemble_warp_trigger_stub_opts(
        ov::WARP_SUBID,
        !draw,
        draw,
    ));
    // Draw payload's INIT requests the persistent draw mode itself, so the redirect
    // must NOT call the return-warp (which would reload the field immediately).
    let redirect = ov::words_to_bytes(&ov::assemble_warp_init_redirect_opts(lba, sectors, !draw));
    let resolve_gap = |va: u32, len: usize| -> Result<usize> {
        let off = legaia_asset::item_names::file_offset_for_va(&scus, va)
            .ok_or_else(|| anyhow::anyhow!("can't resolve gap VA {va:#x} in SCUS"))?;
        if scus
            .get(off..off + len)
            .is_none_or(|r| r.iter().any(|&b| b != 0))
        {
            anyhow::bail!("gap region {va:#x} is not all-zero dead space");
        }
        Ok(off)
    };
    let trig_off = resolve_gap(ov::WARP_TRIGGER_VA, trigger.len())?;
    let redir_off = resolve_gap(ov::WARP_REDIRECT_VA, redirect.len())?;
    patcher
        .patch_named_file(SCUS_NAME, trig_off as u64, &trigger)
        .context("write warp trigger")?;
    patcher
        .patch_named_file(SCUS_NAME, redir_off as u64, &redirect)
        .context("write FUN_80025980 redirect")?;

    // 3. op-0x49 arm edge -> warp trigger.
    let overlay_entry = patcher
        .read_entry(ov::SHOP_OVERLAY_PROT_INDEX)
        .with_context(|| format!("read field overlay PROT {}", ov::SHOP_OVERLAY_PROT_INDEX))?;
    let hook_off = (ov::SHOP_HOOK_VA - ov::SHOP_OVERLAY_BASE) as usize;
    let at_hook: Vec<u32> = overlay_entry
        .get(hook_off..hook_off + 8)
        .ok_or_else(|| anyhow::anyhow!("hook offset past end of overlay entry"))?
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    if at_hook[..] != ov::SHOP_DISPLACED[..] {
        anyhow::bail!(
            "op-0x49 hook site does not match the recognized US build; refusing to patch"
        );
    }
    let detour: Vec<u8> = ov::detour_words()
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    patcher
        .patch_prot_entry(ov::SHOP_OVERLAY_PROT_INDEX, hook_off as u64, &detour)
        .context("write op-0x49 warp detour into the field overlay")?;

    // 4. FUN_80025980 overlay-load site -> redirect (guarded).
    let init_off = legaia_asset::item_names::file_offset_for_va(&scus, ov::WARP_INIT_DETOUR_VA)
        .ok_or_else(|| anyhow::anyhow!("can't resolve FUN_80025980 detour VA"))?;
    let at_init: Vec<u32> = scus
        .get(init_off..init_off + 8)
        .ok_or_else(|| anyhow::anyhow!("FUN_80025980 detour offset out of range"))?
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    if at_init[..] != ov::WARP_INIT_DISPLACED[..] {
        anyhow::bail!(
            "FUN_80025980 load site does not match the recognized US build; refusing to patch"
        );
    }
    let init_detour: Vec<u8> = ov::warp_init_detour_words()
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    patcher
        .patch_named_file(SCUS_NAME, init_off as u64, &init_detour)
        .context("write FUN_80025980 overlay-load redirect detour")?;

    Ok(OverlaySliceReport {
        pochi_index,
        lba,
        sectors,
    })
}

/// Inject the draw-side trade overlay triggered from the **shop picker**
/// (`FUN_801d4868`, overlay 0899) instead of the op-0x49 arm.
///
/// The op-0x49 arm trigger loses a same-frame race to the shop's menu-actor mode
/// 0x17. This routes the mode-24 warp through the picker renderer, which runs
/// every frame the settled Buy/Sell/Quit choice is on screen -- a quiet frame
/// with no competing transition. SQUARE arms the warp (a button the picker
/// ignores), so this is a decisive test of whether a mode-0x18 request issued
/// from a settled shop frame sticks. The load+run+return machinery (pochi
/// overlay, FUN_80025980 redirect, mode-24 return) is identical to the warp draw
/// build; only the trigger site differs.
pub fn inject_overlay_slice_picker(patcher: &mut DiscPatcher) -> Result<OverlaySliceReport> {
    use crate::seru_overlay as ov;

    let overlay = ov::words_to_bytes(&ov::assemble_draw_overlay());
    let sectors = ov::sectors_for(overlay.len());

    // 1. Pochi host + overlay.
    let pochi_index = find_pochi_host(patcher, overlay.len())
        .ok_or_else(|| anyhow::anyhow!("no pochi-filler slot large enough for the overlay"))?;
    let lba = patcher
        .entry_disc_lba(pochi_index)
        .ok_or_else(|| anyhow::anyhow!("pochi slot {pochi_index} has no disc LBA"))?;
    patcher
        .patch_prot_entry(pochi_index, 0, &overlay)
        .with_context(|| format!("write overlay into pochi slot {pochi_index}"))?;

    // 2. Gap routines: picker trigger (reuses the op-0x49 trigger slot) +
    //    FUN_80025980 overlay-load redirect. Draw payload holds the draw mode
    //    itself, so the redirect must NOT call the return-warp.
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for picker slice")?;
    let trigger = ov::words_to_bytes(&ov::assemble_picker_trade_detour_stub(ov::WARP_SUBID));
    let redirect = ov::words_to_bytes(&ov::assemble_warp_init_redirect_opts(lba, sectors, false));
    let resolve_gap = |va: u32, len: usize| -> Result<usize> {
        let off = legaia_asset::item_names::file_offset_for_va(&scus, va)
            .ok_or_else(|| anyhow::anyhow!("can't resolve gap VA {va:#x} in SCUS"))?;
        if scus
            .get(off..off + len)
            .is_none_or(|r| r.iter().any(|&b| b != 0))
        {
            anyhow::bail!("gap region {va:#x} is not all-zero dead space");
        }
        Ok(off)
    };
    let trig_off = resolve_gap(ov::PICKER_TRIGGER_VA, trigger.len())?;
    let redir_off = resolve_gap(ov::WARP_REDIRECT_VA, redirect.len())?;
    patcher
        .patch_named_file(SCUS_NAME, trig_off as u64, &trigger)
        .context("write picker trigger")?;
    patcher
        .patch_named_file(SCUS_NAME, redir_off as u64, &redirect)
        .context("write FUN_80025980 redirect")?;

    // 3. Picker renderer prologue -> picker trigger (overlay 0899, slot A, raw -
    //    maps linearly from base).
    let menu_entry = patcher
        .read_entry(ov::PICKER_MENU_PROT_INDEX)
        .with_context(|| format!("read menu overlay PROT {}", ov::PICKER_MENU_PROT_INDEX))?;
    let hook_off = (ov::PICKER_RENDER_VA - ov::SLOT_A_BASE) as usize;
    let at_hook: Vec<u32> = menu_entry
        .get(hook_off..hook_off + 8)
        .ok_or_else(|| anyhow::anyhow!("picker hook offset past end of menu overlay entry"))?
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    if at_hook[..] != ov::PICKER_DISPLACED[..] {
        anyhow::bail!("picker hook site does not match the recognized US build; refusing to patch");
    }
    let detour: Vec<u8> = ov::picker_detour_words()
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    patcher
        .patch_prot_entry(ov::PICKER_MENU_PROT_INDEX, hook_off as u64, &detour)
        .context("write picker trade detour into the menu overlay")?;

    // 4. FUN_80025980 overlay-load site -> redirect (guarded). SCUS-resident.
    let init_off = legaia_asset::item_names::file_offset_for_va(&scus, ov::WARP_INIT_DETOUR_VA)
        .ok_or_else(|| anyhow::anyhow!("can't resolve FUN_80025980 detour VA"))?;
    let at_init: Vec<u32> = scus
        .get(init_off..init_off + 8)
        .ok_or_else(|| anyhow::anyhow!("FUN_80025980 detour offset out of range"))?
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    if at_init[..] != ov::WARP_INIT_DISPLACED[..] {
        anyhow::bail!(
            "FUN_80025980 load site does not match the recognized US build; refusing to patch"
        );
    }
    let init_detour: Vec<u8> = ov::warp_init_detour_words()
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    patcher
        .patch_named_file(SCUS_NAME, init_off as u64, &init_detour)
        .context("write FUN_80025980 overlay-load redirect detour")?;

    Ok(OverlaySliceReport {
        pochi_index,
        lba,
        sectors,
    })
}

/// Add a native fourth "Trade" row to the shop Buy/Sell/Quit picker (overlay
/// 0899), trigger-agnostic: the cursor clamp is bumped 3 -> 4 and the renderer
/// draws/highlights the row in the game's own style. Selecting it is a clean
/// no-op (index-3 confirm falls through to the dispatcher's normal exit) until the
/// trade action is wired in. No pochi overlay, no warp infra -- only two PROT-0899
/// edits plus a draw stub + label in the SCUS gap.
pub fn inject_native_trade_row(patcher: &mut DiscPatcher) -> Result<()> {
    use crate::seru_overlay as ov;

    // 1. Cursor clamp 3 -> 4 in the dispatcher FUN_801dafd4 (overlay 0899).
    let menu_entry = patcher
        .read_entry(ov::PICKER_MENU_PROT_INDEX)
        .with_context(|| format!("read menu overlay PROT {}", ov::PICKER_MENU_PROT_INDEX))?;
    let read_word = |buf: &[u8], off: usize| -> Result<u32> {
        Ok(u32::from_le_bytes(
            buf.get(off..off + 4)
                .ok_or_else(|| anyhow::anyhow!("offset {off:#x} past end of menu overlay"))?
                .try_into()
                .unwrap(),
        ))
    };
    let clamp_off = (ov::CLAMP_VA - ov::SLOT_A_BASE) as usize;
    if read_word(&menu_entry, clamp_off)? != ov::CLAMP_OLD {
        anyhow::bail!(
            "picker clamp site does not match the recognized US build; refusing to patch"
        );
    }
    patcher
        .patch_prot_entry(
            ov::PICKER_MENU_PROT_INDEX,
            clamp_off as u64,
            &ov::CLAMP_NEW.to_le_bytes(),
        )
        .context("write picker cursor clamp 3->4")?;

    // 1b. Grow the picker box one row taller (sprite-def height for id 0x2a) so
    //     the 4th row sits inside the frame.
    let box_h_off = (ov::BOX_H_VA - ov::SLOT_A_BASE) as usize;
    let cur_h = u16::from_le_bytes(
        menu_entry
            .get(box_h_off..box_h_off + 2)
            .ok_or_else(|| anyhow::anyhow!("box height offset past end of menu overlay"))?
            .try_into()
            .unwrap(),
    );
    if cur_h != ov::BOX_H_OLD {
        anyhow::bail!(
            "picker box height does not match the recognized US build; refusing to patch"
        );
    }
    patcher
        .patch_prot_entry(
            ov::PICKER_MENU_PROT_INDEX,
            box_h_off as u64,
            &ov::BOX_H_NEW.to_le_bytes(),
        )
        .context("write picker box height (3->4 rows)")?;

    // 2. Renderer FUN_801d4868 in-body detour -> row-4 draw stub (overlay 0899).
    let row4_off = (ov::ROW4_DETOUR_VA - ov::SLOT_A_BASE) as usize;
    let at_row4: Vec<u32> = menu_entry
        .get(row4_off..row4_off + 8)
        .ok_or_else(|| anyhow::anyhow!("row-4 hook offset past end of menu overlay entry"))?
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    if at_row4[..] != ov::ROW4_DISPLACED[..] {
        anyhow::bail!(
            "renderer epilogue does not match the recognized US build; refusing to patch"
        );
    }
    let row4_detour: Vec<u8> = ov::row4_detour_words()
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    patcher
        .patch_prot_entry(ov::PICKER_MENU_PROT_INDEX, row4_off as u64, &row4_detour)
        .context("write renderer row-4 detour")?;

    // 3. Draw stub + "@Trade" label into 0899's run-C dead region (same host the
    //    full trade build uses; keeps everything off the contended SCUS gap).
    let stub = ov::words_to_bytes(&ov::assemble_row4_draw_stub());
    let write0899 = |p: &mut DiscPatcher, va: u32, bytes: &[u8], what: &str| -> Result<()> {
        let off = (va - ov::SLOT_A_BASE) as usize;
        if menu_entry
            .get(off..off + bytes.len())
            .is_none_or(|r| r.iter().any(|&b| b != 0))
        {
            anyhow::bail!("0899 region {va:#x} ({what}) is not all-zero dead space");
        }
        p.patch_prot_entry(ov::PICKER_MENU_PROT_INDEX, off as u64, bytes)
            .with_context(|| format!("write {what} into menu overlay 0899"))
    };
    write0899(patcher, ov::ROW4_STUB_VA, &stub, "row-4 draw stub")?;
    write0899(patcher, ov::TRADE_STR_VA, ov::TRADE_STR, "@Trade label")?;

    Ok(())
}

/// Full in-shop Trade vendor: Buy/Sell/**Trade**/Quit, and confirming Trade enters
/// a picker SUB-MODE (no warp) that draws the trade screen inside mode 0x17 and
/// returns to the picker on exit. Pure overlay-0899 + SCUS-gap edits; the shop is
/// never torn down. All PROT-0899 sites are guarded against the recognized US build.
pub fn inject_trade_full(patcher: &mut DiscPatcher, seed: u64) -> Result<()> {
    use crate::seru_overlay as ov;
    use legaia_asset::seru_trade as st;

    let base = ov::SLOT_A_BASE;
    let menu = patcher
        .read_entry(ov::PICKER_MENU_PROT_INDEX)
        .with_context(|| format!("read menu overlay PROT {}", ov::PICKER_MENU_PROT_INDEX))?;
    let word = |va: u32| -> Result<u32> {
        let o = (va - base) as usize;
        Ok(u32::from_le_bytes(
            menu.get(o..o + 4)
                .ok_or_else(|| anyhow::anyhow!("VA {va:#x} past end of menu overlay"))?
                .try_into()
                .unwrap(),
        ))
    };
    let words2 = |va: u32| -> Result<[u32; 2]> { Ok([word(va)?, word(va + 4)?]) };

    // --- Guard every overlay-0899 site against the recognized build ---
    if word(ov::CLAMP_VA)? != ov::CLAMP_OLD {
        anyhow::bail!("picker clamp site mismatch; refusing to patch");
    }
    let box_off = (ov::BOX_H_VA - base) as usize;
    if u16::from_le_bytes(menu[box_off..box_off + 2].try_into().unwrap()) != ov::BOX_H_OLD {
        anyhow::bail!("picker box-height site mismatch; refusing to patch");
    }
    if words2(ov::ROW2_STR_LOAD_VA)? != ov::ROW2_STR_LOAD_OLD {
        anyhow::bail!("row-2 string-load site mismatch; refusing to patch");
    }
    if words2(ov::ROW4_DETOUR_VA)? != ov::ROW4_DISPLACED {
        anyhow::bail!("renderer row-4 site mismatch; refusing to patch");
    }
    if words2(ov::TRADE_DISPATCH_VA)? != ov::TRADE_DISPATCH_DISPLACED {
        anyhow::bail!("dispatch site mismatch; refusing to patch");
    }
    if words2(ov::ENTRY_VA)? != ov::ENTRY_DISPLACED {
        anyhow::bail!("FUN_801dafd4 entry site mismatch; refusing to patch");
    }

    // --- Apply the overlay-0899 edits ---
    let le2 = |w: [u32; 2]| -> Vec<u8> { w.iter().flat_map(|x| x.to_le_bytes()).collect() };
    let prot = |p: &mut DiscPatcher, va: u32, bytes: &[u8], what: &str| -> Result<()> {
        p.patch_prot_entry(ov::PICKER_MENU_PROT_INDEX, (va - base) as u64, bytes)
            .with_context(|| format!("write {what}"))
    };
    prot(
        patcher,
        ov::CLAMP_VA,
        &ov::CLAMP_NEW.to_le_bytes(),
        "cursor clamp 3->4",
    )?;
    prot(
        patcher,
        ov::BOX_H_VA,
        &ov::BOX_H_NEW.to_le_bytes(),
        "box height (4 rows)",
    )?;
    prot(
        patcher,
        ov::ROW2_STR_LOAD_VA,
        &le2(ov::row2_str_load_new()),
        "row-2 string swap (-> @Trade)",
    )?;
    prot(
        patcher,
        ov::ROW4_DETOUR_VA,
        &le2(ov::row4_detour_words()),
        "renderer row-4 detour",
    )?;
    prot(
        patcher,
        ov::TRADE_DISPATCH_VA,
        &le2(ov::trade_dispatch_detour_words()),
        "Trade dispatch detour",
    )?;
    prot(
        patcher,
        ov::ENTRY_VA,
        &le2(ov::trade_entry_detour_words()),
        "FUN_801dafd4 entry detour",
    )?;

    // --- All seru-trade code + data lives in 0899's resident run-C dead region
    // (reference-free, all-zero, reloaded with the overlay). Nothing touches the SCUS
    // rodata gap, so seru trading is compatible with every gap-based feature
    // (bonus-equipment drops, flee-EXP, the Seru-Bell name). ---
    let row4 = ov::words_to_bytes(&ov::assemble_row4_draw_stub_str(ov::QUIT_STR_VA));
    let entry = ov::words_to_bytes(&ov::assemble_trade_entry_stub());
    let disp = ov::words_to_bytes(&ov::assemble_trade_dispatch_stub());
    let handler = ov::words_to_bytes(&ov::assemble_trade_handler());
    // The precomputed vendor schedule the handler indexes by play-time bucket: one
    // `[want, give, give_level]` per bucket, derived deterministically from `seed`.
    let bucket_table = st::bucket_table_to_bytes(&st::bucket_offers(
        seed,
        st::BUCKET_COUNT,
        &st::default_pool(),
    ));
    let blobs: [(u32, &[u8], &str); 10] = [
        (ov::TRADE_HANDLER_VA, &handler, "trade handler"),
        (ov::ENTRY_STUB_VA, &entry, "entry stub"),
        (ov::TRADE_DISPATCH_STUB_VA, &disp, "dispatch stub"),
        (ov::ROW4_STUB_VA, &row4, "row-4 draw stub"),
        (ov::TRADE_STR_VA, ov::TRADE_STR, "@Trade label"),
        (ov::TITLE_STR_VA, ov::TITLE_STR, "title string"),
        (
            ov::CONFIRM_PROMPT_STR_VA,
            ov::CONFIRM_PROMPT_STR,
            "confirm prompt",
        ),
        (ov::CONFIRM_YES_STR_VA, ov::CONFIRM_YES_STR, "confirm Yes"),
        (ov::CONFIRM_NO_STR_VA, ov::CONFIRM_NO_STR, "confirm No"),
        (ov::BUCKET_TABLE_VA, &bucket_table, "bucket schedule"),
    ];
    for (va, bytes, what) in blobs {
        if va < base || va + bytes.len() as u32 > ov::TRADE_HANDLER_END {
            anyhow::bail!("0899 blob {what} ({va:#x}) outside the run-C region");
        }
        let off = (va - base) as usize;
        if menu
            .get(off..off + bytes.len())
            .is_none_or(|r| r.iter().any(|&b| b != 0))
        {
            anyhow::bail!("0899 region {va:#x} ({what}) is not all-zero dead space");
        }
        patcher
            .patch_prot_entry(ov::HANDLER_OVL_PROT_INDEX, off as u64, bytes)
            .with_context(|| format!("write {what} into menu overlay 0899"))?;
    }
    Ok(())
}

/// Outcome of randomizing monster combat stats.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MonsterStatsReport {
    /// Monster slots actually rewritten.
    pub monsters_changed: usize,
    /// Total stat fields that changed across all rewritten monsters.
    pub fields_changed: usize,
    /// Monster ids whose re-packed slot would overflow the `0x14000` footprint,
    /// so the edit was skipped (the original stats are kept). Our LZS re-packer
    /// isn't byte-identical to Sony's, so a record already near the slot limit
    /// can rarely overflow; skipping keeps the rest of the patch valid (mirrors
    /// the drop randomizer, see [`crate::monster`]).
    pub skipped: Vec<u16>,
}

/// Read every populated monster's id + current combat stats (the
/// [`crate::monster_stats::STAT_FIELDS`] halfwords) out of the `battle_data`
/// archive. This is the population the stat randomizer redistributes.
pub fn current_monster_stats(patcher: &DiscPatcher) -> Result<Vec<monster_stats::StatAssignment>> {
    let entry = patcher
        .read_entry(MONSTER_ARCHIVE_ENTRY)
        .context("read monster battle_data archive")?;
    let records =
        legaia_asset::monster_archive::records(&entry).context("decode monster archive records")?;
    Ok(records
        .iter()
        .map(|r| monster_stats::StatAssignment {
            monster_id: r.id,
            stats: [
                r.hp,
                r.mp,
                r.attack(),
                r.defense_high(),
                r.defense_low(),
                r.agility(),
                r.speed(),
            ],
        })
        .collect())
}

/// Randomize every monster's combat stats in place (see [`crate::monster_stats`]).
/// Each monster's `0x14000`-byte slot is decompressed, the stat halfwords
/// rewritten, and recompressed back to the same footprint - a same-size,
/// in-place edit. A slot too tight to re-pack is skipped (recorded in the
/// report) rather than aborting the run. Returns the apply report.
pub fn randomize_monster_stats(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
) -> Result<MonsterStatsReport> {
    let current = current_monster_stats(patcher)?;
    let plan = monster_stats::plan_stats(&current, seed, mode);
    let mut report = MonsterStatsReport::default();
    for (cur, new) in current.iter().zip(&plan) {
        if cur.stats == new.stats {
            continue;
        }
        let slot = patcher
            .monster_slot(new.monster_id)
            .with_context(|| format!("read monster {} slot", new.monster_id))?;
        let new_slot = match monster_stats::set_stats(&slot, &new.stats) {
            Ok(s) => s,
            Err(_) => {
                // Expected only on the slot-overflow guard; a malformed slot
                // would have failed in `current_monster_stats` already.
                report.skipped.push(new.monster_id);
                continue;
            }
        };
        if new_slot != slot {
            patcher
                .patch_monster_slot(new.monster_id, &new_slot)
                .with_context(|| format!("write monster {} slot", new.monster_id))?;
            report.monsters_changed += 1;
            report.fields_changed += cur
                .stats
                .iter()
                .zip(&new.stats)
                .filter(|(a, b)| a != b)
                .count();
        }
    }
    Ok(report)
}

/// Build the `256`-entry "id is sellable" mask from the disc's SCUS item table:
/// an id with a `> 0` price. Used to validate + delimit town-shop records - the
/// record `count` over-counts the purchasable stock by a trailing run of
/// unsellable (price-`0`) template ids, so the sellable mask both rejects stray
/// `0x49` payloads and trims the padding out of the stock (see
/// [`legaia_asset::shop_stock`]). `None` if SCUS / its item table is absent (the
/// shop locator then falls back to structural-only validation).
fn sellable_item_mask(patcher: &DiscPatcher) -> Option<[bool; 256]> {
    let scus = patcher.read_named_file("SCUS_942.54")?;
    let mut mask = [false; 256];
    for (id, slot) in mask.iter_mut().enumerate() {
        *slot = legaia_asset::item_names::price_slot(&scus, id as u8)
            .is_some_and(|(_, price)| price > 0);
    }
    Some(mask)
}

/// Locate a scene's shops, using the SCUS item-name mask when available
/// (strict) and structural-only validation otherwise.
fn locate_shops(entry: &[u8], idx: usize, mask: Option<&[bool; 256]>) -> Option<SceneShops> {
    match mask {
        Some(m) => SceneShops::locate_with_items(entry, idx, m),
        None => SceneShops::locate(entry, idx),
    }
}

/// One town shop's current stock, for the read-only listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShopListing {
    /// PROT entry index of the scene bundle holding this shop.
    pub entry_idx: usize,
    /// On-screen shop name (e.g. "Variety Store").
    pub name: String,
    /// Item ids the shop currently sells, in display order.
    pub items: Vec<u8>,
}

/// Read every town-merchant shop on the disc (the randomizable population), in
/// PROT-entry then in-scene order. Mirrors [`current_chests`]: read-only, decodes
/// each scene MAN once via [`SceneShops::locate`].
pub fn current_shops(patcher: &DiscPatcher) -> Result<Vec<ShopListing>> {
    let mask = sellable_item_mask(patcher);
    let mut out = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(sc) = locate_shops(&entry, idx, mask.as_ref()) else {
            continue;
        };
        for shop in &sc.shops {
            out.push(ShopListing {
                entry_idx: idx,
                name: shop.name.clone(),
                items: shop.id_offsets.iter().map(|&o| sc.decoded[o]).collect(),
            });
        }
    }
    Ok(out)
}

/// Outcome of randomizing town shops.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ShopApplyReport {
    /// Scene bundles whose MAN was rewritten + written back.
    pub scenes_changed: usize,
    /// Total shop item-id bytes changed.
    pub items_changed: usize,
    /// Total shop item slots found (the randomizable population).
    pub slots_total: usize,
    /// Scene PROT-entry indices whose recompressed MAN would not fit, skipped.
    pub skipped: Vec<usize>,
}

/// Install the chest-found-equipment shop prices (see [`crate::item_price`]) by
/// patching the `SCUS_942.54` item table in place. Returns the number of price
/// fields changed. Idempotent (re-applying writes nothing). Safe no-op if SCUS
/// or the item table is absent.
pub fn apply_item_price_edits(patcher: &mut DiscPatcher) -> Result<usize> {
    let Some(scus) = patcher.read_named_file(crate::steal::SCUS_NAME) else {
        return Ok(0);
    };
    let patches = crate::item_price::price_patches(&scus)?;
    for (off, bytes) in &patches {
        patcher
            .patch_named_file(crate::steal::SCUS_NAME, *off as u64, bytes)
            .with_context(|| format!("write item price at SCUS offset {off}"))?;
    }
    Ok(patches.len())
}

/// Randomize town-merchant stock (field-VM shop op `0x49`; see [`crate::shop`]).
/// Shop item ids are global inventory ids, so this is a **global** reassignment
/// across every town shop on the disc: `Shuffle` redistributes the existing
/// shop-item multiset, `Random` draws each slot from the **sellable pool** -
/// items the game prices `> 0` (see [`crate::item_price::sellable_pool`]), which
/// excludes every quest / key / story item (all price `0`) so a shop can never
/// stock one. As a prerequisite this first prices the chest-found equipment
/// ([`apply_item_price_edits`]) so that gear is non-free and joins the sellable
/// pool. Only the item-id bytes are rewritten; each touched scene MAN is
/// recompressed and a scene whose MAN overflows is skipped.
pub fn randomize_shops(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
) -> Result<ShopApplyReport> {
    // Price the chest-found equipment so it is sellable (and not free) before we
    // read the sellable pool / stock it.
    apply_item_price_edits(patcher)?;

    // `Random` fill draws from the sellable pool (priced items only); `Shuffle`
    // redistributes the existing shop entries and ignores the pool.
    let item_pool: Vec<u8> = if mode == DropMode::Random {
        match patcher.read_named_file(crate::steal::SCUS_NAME) {
            Some(scus) => crate::item_price::sellable_pool(&scus)?,
            None => Vec::new(),
        }
    } else {
        Vec::new()
    };
    let item_pool = item_pool.as_slice();

    // Pass 1: collect every scene's shops (decoded MAN held for pass 2).
    let mask = sellable_item_mask(patcher);
    let mut scenes: Vec<SceneShops> = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        if let Some(sc) = locate_shops(&entry, idx, mask.as_ref()) {
            scenes.push(sc);
        }
    }

    // Per-scene ordered item-id slot offsets + originals.
    let offsets: Vec<Vec<usize>> = scenes.iter().map(|s| s.id_offsets()).collect();
    let originals: Vec<Vec<u8>> = scenes
        .iter()
        .zip(&offsets)
        .map(|(s, offs)| offs.iter().map(|&o| s.decoded[o]).collect())
        .collect();

    let mut report = ShopApplyReport {
        slots_total: offsets.iter().map(|o| o.len()).sum(),
        ..Default::default()
    };
    if report.slots_total == 0 {
        return Ok(report);
    }

    let mut skipped: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    let mut streams: Vec<(usize, u64, Vec<u8>)> = Vec::new();

    match mode {
        DropMode::Shuffle => {
            // Iteratively converge on a writable set: shuffle the originals of
            // the not-yet-skipped scenes among those same slots, repack, and fold
            // any fresh overflow into `skipped` (which only shrinks the pool, so
            // it converges and the multiset over written slots stays preserved).
            loop {
                for (i, sc) in scenes.iter_mut().enumerate() {
                    for (k, &o) in offsets[i].iter().enumerate() {
                        sc.set_id(o, originals[i][k]);
                    }
                }
                let mut pool: Vec<u8> = (0..scenes.len())
                    .filter(|i| !skipped.contains(i))
                    .flat_map(|i| originals[i].iter().copied())
                    .collect();
                let mut rng = SplitMix64::new(seed);
                rng.shuffle(&mut pool);
                let mut cur = 0usize;
                for (i, sc) in scenes.iter_mut().enumerate() {
                    if skipped.contains(&i) {
                        continue;
                    }
                    for &o in &offsets[i] {
                        sc.set_id(o, pool[cur]);
                        cur += 1;
                    }
                }
                streams.clear();
                let mut fresh_overflow = false;
                for (i, sc) in scenes.iter().enumerate() {
                    if skipped.contains(&i) {
                        continue;
                    }
                    match sc.repack() {
                        Some(stream) => streams.push((sc.entry_idx, sc.man_offset as u64, stream)),
                        None => {
                            skipped.insert(i);
                            fresh_overflow = true;
                        }
                    }
                }
                if !fresh_overflow {
                    break;
                }
            }
        }
        DropMode::Random => {
            if item_pool.is_empty() {
                return Ok(report);
            }
            let mut rng = SplitMix64::new(seed);
            for (i, sc) in scenes.iter_mut().enumerate() {
                for &o in &offsets[i] {
                    let v = item_pool[rng.below(item_pool.len())];
                    sc.set_id(o, v);
                }
                match sc.repack() {
                    Some(stream) => streams.push((sc.entry_idx, sc.man_offset as u64, stream)),
                    None => {
                        skipped.insert(i);
                    }
                }
            }
        }
    }

    // Tally changes (over non-skipped scenes) and write the streams back.
    for (i, sc) in scenes.iter().enumerate() {
        if skipped.contains(&i) {
            continue;
        }
        let changed = offsets[i]
            .iter()
            .enumerate()
            .filter(|&(k, &o)| sc.decoded[o] != originals[i][k])
            .count();
        if changed > 0 {
            report.scenes_changed += 1;
            report.items_changed += changed;
        }
    }
    for (entry_idx, man_offset, stream) in streams {
        patcher
            .patch_prot_entry(entry_idx, man_offset, &stream)
            .with_context(|| format!("write scene {entry_idx} shop MAN"))?;
    }
    report.skipped = skipped.into_iter().map(|i| scenes[i].entry_idx).collect();
    Ok(report)
}

/// Read the casino prize-exchange table (PROT 0899), for the read-only listing.
/// Returns `None` if the entry / table can't be parsed.
pub fn current_casino(patcher: &DiscPatcher) -> Result<Option<CasinoExchange>> {
    let entry = patcher
        .read_entry(casino::CASINO_ENTRY)
        .context("read casino overlay entry 0899")?;
    Ok(CasinoExchange::parse(
        &entry,
        casino::CASINO_TABLE_OFFSET,
        casino::CASINO_BLOCK_COUNT,
    ))
}

/// Randomize the casino prize-exchange table (see [`crate::casino`]). A
/// same-size raw edit of PROT entry 0899 (no LZS), so it never overflows.
/// Returns the number of prize slots that changed.
pub fn randomize_casino(patcher: &mut DiscPatcher, seed: u64, mode: DropMode) -> Result<usize> {
    let mut entry = patcher
        .read_entry(casino::CASINO_ENTRY)
        .context("read casino overlay entry 0899")?;
    let Some(mut ex) = CasinoExchange::parse(
        &entry,
        casino::CASINO_TABLE_OFFSET,
        casino::CASINO_BLOCK_COUNT,
    ) else {
        return Ok(0);
    };
    let base = casino::CASINO_TABLE_OFFSET;
    let span = casino::CASINO_BLOCK_COUNT * casino::BLOCK_SIZE;
    let before = entry[base..base + span].to_vec();
    ex.randomize(seed, mode);
    ex.write_back(&mut entry);
    let after = &entry[base..base + span];
    let changed = before
        .chunks(casino::RECORD_SIZE)
        .zip(after.chunks(casino::RECORD_SIZE))
        .filter(|(a, b)| a != b)
        .count();
    if after != before.as_slice() {
        patcher
            .patch_prot_entry(casino::CASINO_ENTRY, base as u64, after)
            .context("write casino prize table")?;
    }
    Ok(changed)
}

/// Read the special-attack move-power column (the per-record `+0x00` power
/// halfword) from PROT 0898, for the read-only listing / the randomizer input.
/// Returns `None` if the battle-action overlay entry can't be parsed.
pub fn current_move_powers(patcher: &DiscPatcher) -> Result<Option<Vec<i16>>> {
    let entry = patcher
        .read_entry(legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .context("read battle-action overlay entry 0898")?;
    Ok(legaia_asset::move_power::parse(&entry)
        .map(|recs| recs.iter().map(|r| r.power_raw).collect()))
}

/// Randomize the special-attack power table (see [`crate::move_power`]). Rewrites
/// only each record's `+0x00` power halfword in PROT 0898 - a same-size raw edit
/// (no LZS). Returns the number of power values that changed.
pub fn randomize_move_powers(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
) -> Result<usize> {
    use legaia_asset::move_power::{
        BATTLE_ACTION_OVERLAY_PROT_INDEX, MOVE_POWER_RECORD_STRIDE, MOVE_POWER_TABLE_FILE_OFFSET,
    };
    let mut entry = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .context("read battle-action overlay entry 0898")?;
    let Some(records) = legaia_asset::move_power::parse(&entry) else {
        return Ok(0);
    };
    // Redistribute powers only among populated records. Empty (all-zero) records
    // - including the index-0 sentinel `parse` keys on - must stay zero, so a
    // move's power is never handed to an unused slot (nor a real move zeroed).
    let populated: Vec<usize> = records
        .iter()
        .enumerate()
        .filter(|(_, r)| !r.is_empty())
        .map(|(i, _)| i)
        .collect();
    let current: Vec<i16> = populated.iter().map(|&i| records[i].power_raw).collect();
    let plan = crate::move_power::plan_powers(&current, seed, mode);

    let table = MOVE_POWER_TABLE_FILE_OFFSET;
    let span = records.len() * MOVE_POWER_RECORD_STRIDE;
    let before = entry[table..table + span].to_vec();
    let mut changed = 0usize;
    for (k, &i) in populated.iter().enumerate() {
        if current[k] == plan[k] {
            continue;
        }
        let off = table + i * MOVE_POWER_RECORD_STRIDE;
        entry[off..off + 2].copy_from_slice(&plan[k].to_le_bytes());
        changed += 1;
    }
    let after = &entry[table..table + span];
    if after != before.as_slice() {
        patcher
            .patch_prot_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX, table as u64, after)
            .context("write move-power table")?;
    }
    Ok(changed)
}

/// Read the 8×8 element-affinity matrix (PROT 0898), flattened row-major
/// (`attacker * 8 + defender`), for the read-only listing / randomizer input.
/// `None` if the entry can't parse.
pub fn current_affinity_matrix(
    patcher: &DiscPatcher,
) -> Result<Option<[u8; crate::element_affinity::MATRIX_CELLS]>> {
    let entry = patcher
        .read_entry(legaia_asset::element_affinity::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .context("read battle-action overlay entry 0898")?;
    let Some(aff) = legaia_asset::element_affinity::ElementAffinity::parse(&entry) else {
        return Ok(None);
    };
    let ec = legaia_asset::element_affinity::ELEMENT_COUNT;
    let mut flat = [0u8; crate::element_affinity::MATRIX_CELLS];
    for (atk, row) in aff.matrix.iter().enumerate() {
        flat[atk * ec..atk * ec + row.len()].copy_from_slice(row);
    }
    Ok(Some(flat))
}

/// Randomize the element-affinity matrix (see [`crate::element_affinity`]).
/// Rewrites the 64 matrix bytes in PROT 0898 - a same-size raw edit (no LZS).
/// Returns the number of cells that changed.
pub fn randomize_element_affinity(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
) -> Result<usize> {
    use legaia_asset::element_affinity::{
        AFFINITY_MATRIX_FILE_OFFSET, BATTLE_ACTION_OVERLAY_PROT_INDEX,
    };
    let Some(current) = current_affinity_matrix(patcher)? else {
        return Ok(0);
    };
    let plan = crate::element_affinity::plan_matrix(&current, seed, mode);
    let changed = current.iter().zip(&plan).filter(|(a, b)| a != b).count();
    if plan != current {
        patcher
            .patch_prot_entry(
                BATTLE_ACTION_OVERLAY_PROT_INDEX,
                AFFINITY_MATRIX_FILE_OFFSET as u64,
                &plan,
            )
            .context("write element-affinity matrix")?;
    }
    Ok(changed)
}

/// One spell's current MP cost, for the read-only listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellCost {
    /// Spell id (index into the spell table).
    pub id: u8,
    /// Display name.
    pub name: String,
    /// MP cost (`stats +3`).
    pub mp: u8,
}

/// `SCUS_942.54` filename (the static-table container).
const SCUS_NAME: &str = "SCUS_942.54";

/// Read every named, costed spell's id + name + MP cost from the SCUS spell
/// table - the population the MP-cost randomizer redistributes. Empty / unnamed
/// internal-tier slots and zero-cost spells are excluded. `None` if SCUS / its
/// spell table can't be parsed.
pub fn current_spell_costs(patcher: &DiscPatcher) -> Result<Option<Vec<SpellCost>>> {
    let Some(scus) = patcher.read_named_file(SCUS_NAME) else {
        return Ok(None);
    };
    let Some(table) = legaia_asset::spell_names::SpellNameTable::from_scus(&scus) else {
        return Ok(None);
    };
    let mut out = Vec::new();
    for id in 0..=u8::MAX {
        let Some(entry) = table.entry(id) else { break };
        if let Some(name) = entry.name.as_deref().filter(|_| entry.mp > 0) {
            out.push(SpellCost {
                id,
                name: name.to_string(),
                mp: entry.mp,
            });
        }
    }
    Ok(Some(out))
}

/// Randomize spell MP costs in the SCUS spell table (see [`crate::spell_cost`]).
/// Rewrites only the `+3` cost byte of each named, costed spell - a same-size
/// in-place SCUS patch. Returns the number of spells whose cost changed.
pub fn randomize_spell_costs(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
) -> Result<usize> {
    use legaia_asset::spell_names::{RECORD_STRIDE, SPELL_COUNT};
    let Some(scus) = patcher.read_named_file(SCUS_NAME) else {
        return Ok(0);
    };
    let Some(table_off) = legaia_asset::spell_names::stats_file_offset(&scus) else {
        return Ok(0);
    };
    let span = SPELL_COUNT * RECORD_STRIDE;
    let Some(mut table) = scus.get(table_off..table_off + span).map(<[u8]>::to_vec) else {
        return Ok(0);
    };

    // Randomizable population: named spells with a non-zero MP cost.
    let names = legaia_asset::spell_names::SpellNameTable::from_scus(&scus);
    let ids: Vec<usize> = (0..SPELL_COUNT)
        .filter(|&id| {
            let cost = table[id * RECORD_STRIDE + 3];
            let named = names
                .as_ref()
                .and_then(|t| t.name(id as u8))
                .is_some_and(|n| !n.is_empty());
            cost > 0 && named
        })
        .collect();
    let current: Vec<u8> = ids
        .iter()
        .map(|&id| table[id * RECORD_STRIDE + 3])
        .collect();
    let plan = crate::spell_cost::plan_costs(&current, seed, mode);

    let mut changed = 0usize;
    for (k, &id) in ids.iter().enumerate() {
        let off = id * RECORD_STRIDE + 3;
        if table[off] != plan[k] {
            table[off] = plan[k];
            changed += 1;
        }
    }
    if changed > 0 {
        patcher
            .patch_named_file(SCUS_NAME, table_off as u64, &table)
            .context("write spell MP-cost table")?;
    }
    Ok(changed)
}

/// One equipment stat-bonus row, for the read-only listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquipBonusRow {
    /// Bonus-table row index (the `bonus_index` an item resolves to).
    pub row: usize,
    /// Slot category the row belongs to (`body` / `head` / `weapon` / `footwear`).
    pub slot: &'static str,
    /// The five stat bonuses `[INT, ATK, UDF, LDF, SPD]` (`+0..+4`).
    pub stats: [u8; 5],
    /// 1-based item ids that resolve to this row (a row can be shared).
    pub items: Vec<u8>,
}

/// Map an [`legaia_asset::equip_stats::EquipSlot`] to its CLI label.
fn equip_slot_name(slot: legaia_asset::equip_stats::EquipSlot) -> &'static str {
    use legaia_asset::equip_stats::EquipSlot;
    match slot {
        EquipSlot::Body => "body",
        EquipSlot::Head => "head",
        EquipSlot::Weapon => "weapon",
        EquipSlot::Footwear => "footwear",
    }
}

/// Read the equipment stat-bonus table (`DAT_80074F68`) off the disc's SCUS, in
/// row order, each with its slot category, decoded stats, and the item ids that
/// reference it. The randomizable population (see [`crate::equip_bonus`]).
/// `None` if SCUS / its bonus table can't be parsed.
pub fn current_equip_bonuses(patcher: &DiscPatcher) -> Result<Option<Vec<EquipBonusRow>>> {
    let Some(scus) = patcher.read_named_file(SCUS_NAME) else {
        return Ok(None);
    };
    let Some(table) = legaia_asset::equip_stats::EquipStatTable::from_scus(&scus) else {
        return Ok(None);
    };
    let items = table.items_for_rows();
    let rows = table
        .rows()
        .iter()
        .enumerate()
        .map(|(i, b)| EquipBonusRow {
            row: i,
            slot: equip_slot_name(b.slot()),
            stats: b.stat_bonus(),
            items: items.get(i).cloned().unwrap_or_default(),
        })
        .collect();
    Ok(Some(rows))
}

/// Randomize the equipment passive stat bonuses (see [`crate::equip_bonus`]).
/// Rewrites only the `+0..+4` stat tuple of each bonus row that at least one
/// equippable item references, reassigning it **within its slot category** - a
/// same-size in-place SCUS patch. The equip-character mask, accessory passive,
/// and slot type stay welded to their row. Returns the number of rows changed.
///
/// Operates on bonus rows (not item ids): several items can share one record,
/// so a per-id rewrite would double-edit a shared record. Rows no equippable
/// item reaches are left untouched, so an unused/garbage row can't hand a real
/// item a junk stat tuple.
pub fn randomize_equip_bonuses(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
) -> Result<usize> {
    use legaia_asset::equip_stats::{BONUS_STRIDE, EquipStatTable, bonus_table_file_offset};
    let Some(scus) = patcher.read_named_file(SCUS_NAME) else {
        return Ok(0);
    };
    let Some(table) = EquipStatTable::from_scus(&scus) else {
        return Ok(0);
    };
    let Some(off) = bonus_table_file_offset(&scus) else {
        return Ok(0);
    };

    let all_rows: Vec<[u8; 8]> = table.rows().iter().map(|b| b.raw).collect();
    let items = table.items_for_rows();
    // Only rows an equippable item actually resolves to participate.
    let participating: Vec<usize> = (0..all_rows.len())
        .filter(|&i| !items[i].is_empty())
        .collect();
    if participating.is_empty() {
        return Ok(0);
    }
    let sub: Vec<[u8; 8]> = participating.iter().map(|&i| all_rows[i]).collect();
    let planned = crate::equip_bonus::plan_bonus_shuffle(&sub, seed, mode);

    let mut new_rows = all_rows.clone();
    let mut changed = 0usize;
    for (k, &i) in participating.iter().enumerate() {
        if planned[k] != all_rows[i] {
            new_rows[i] = planned[k];
            changed += 1;
        }
    }
    if changed == 0 {
        return Ok(0);
    }
    let mut bytes = Vec::with_capacity(all_rows.len() * BONUS_STRIDE);
    for r in &new_rows {
        bytes.extend_from_slice(r);
    }
    patcher
        .patch_named_file(SCUS_NAME, off as u64, &bytes)
        .context("write equipment stat-bonus table")?;
    Ok(changed)
}

/// Outcome of randomizing scene encounters.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EncounterApplyReport {
    /// Scene bundles whose MAN was rewritten + written back.
    pub scenes_changed: usize,
    /// Total formation id bytes changed across all scenes.
    pub ids_changed: usize,
    /// Formation id slots (in written-back scenes) that ended up holding one of
    /// the `unused_enemies` ids - i.e. how many unused enemies the run actually
    /// placed. Always `0` unless `unused_enemies` was non-empty and the mode was
    /// [`DropMode::Random`].
    pub unused_placed: usize,
    /// Random formations forced down to a single enemy by the solo-strong option
    /// (an over-strong monster faced alone instead of in a pack). Always `0`
    /// unless [`randomize_encounters_full`] ran with a [`SoloStrongConfig`].
    pub solo_collapsed: usize,
    /// Scene PROT-entry indices whose recompressed MAN would not fit the
    /// original footprint, so the scene was left untouched.
    pub skipped: Vec<usize>,
}

/// Randomize every scene's random-encounter formations in place. For each scene
/// bundle the monster ids are reassigned from the scene's own id pool, the MAN
/// is recompressed, and - when it fits the original compressed footprint -
/// written back. Scenes whose re-pack overflows are recorded in `skipped` and
/// left unchanged.
///
/// `unused_enemies` is the curated set of monster ids no formation normally
/// references (see [`crate::unused::UNUSED_ENEMY_IDS`]). When non-empty *and*
/// `mode` is [`DropMode::Random`], those ids join each scene's candidate pool so
/// the run can spawn them - the battle loader streams a monster's archive slot
/// on demand by id, so an id outside the scene's own set still loads. Under
/// [`DropMode::Shuffle`] it has no effect (a multiset-preserving permutation
/// can't introduce a new id). Pass an empty slice to keep the prior behaviour;
/// the RNG stream is unchanged when it is empty, so existing results stay
/// byte-identical.
pub fn randomize_encounters(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
    unused_enemies: &[u8],
) -> Result<EncounterApplyReport> {
    let mut report = EncounterApplyReport::default();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(mut scene) = SceneEncounters::locate(&entry, idx) else {
            continue;
        };
        let changed = scene.randomize_with_extra(seed, mode, unused_enemies);
        if changed == 0 {
            continue;
        }
        match scene.repack() {
            Some(stream) => {
                patcher
                    .patch_prot_entry(idx, scene.man_offset as u64, &stream)
                    .with_context(|| format!("write scene {idx} MAN"))?;
                report.scenes_changed += 1;
                report.ids_changed += changed;
                if !unused_enemies.is_empty() {
                    report.unused_placed += scene.count_ids_in(unused_enemies);
                }
            }
            None => report.skipped.push(idx),
        }
    }
    Ok(report)
}

/// How wide a net the encounter randomizer casts when reassigning a scene's
/// monsters - the *pool* a random encounter is drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncounterScope {
    /// Each scene draws only from its own monster ids - the classic per-scene
    /// behaviour ([`randomize_encounters`]). Difficulty stays local; a swap can
    /// never surprise you with a monster the area didn't already host.
    Scene,
    /// "Within a region": a scene draws from the union of every monster across
    /// its **kingdom** (Drake / Sebucus / Karisto, see [`crate::kingdom`]).
    /// Late-game Drake monsters can show up in early Drake, but no monster ever
    /// crosses a kingdom boundary.
    Kingdom,
    /// "Across regions": a scene draws from every monster on the disc, so a
    /// late-game Karisto monster can appear in the opening Drake caves.
    World,
}

/// Tag mixed into the cross-scene shuffle RNG so a scoped shuffle is independent
/// of the per-scene seeding used by [`randomize_encounters`].
const SCOPED_SHUFFLE_NONCE: u64 = 0x5343_4F50_4544_0001; // "SCOPED\0\1"

/// Randomize scene encounters with a pool [`EncounterScope`] wider than a single
/// scene. `Scene` scope delegates to [`randomize_encounters`] unchanged.
///
/// For `Kingdom` / `World` scope the disc is processed in two passes:
///
/// 1. **Collect** every scene's encounter data and (for `Kingdom`) its kingdom,
///    then build the per-group monster pool - the union of each group's scenes'
///    random-encounter ids (bosses excluded, exactly as the per-scene path).
/// 2. **Reassign**: under [`DropMode::Random`] each scene's random slots are
///    redrawn from its group pool ([`SceneEncounters::fill_random_slots_from_pool`]);
///    under [`DropMode::Shuffle`] the whole group's random ids are pooled,
///    permuted once, and redistributed across the group's scenes
///    ([`SceneEncounters::random_slot_ids`] / [`SceneEncounters::apply_random_slots`]),
///    so the group-wide multiset is preserved but monsters move between scenes
///    (and, for `World`, between kingdoms).
///
/// `Kingdom` scope needs the disc's `CDNAME.TXT` to resolve kingdoms; if it
/// can't be parsed the call errors (every retail disc carries it). `World` scope
/// never touches CDNAME. `unused_enemies` behaves as in [`randomize_encounters`]
/// (it widens the pool under `Random` only). The result is deterministic for a
/// fixed `seed`, independent of patcher iteration order.
pub fn randomize_encounters_scoped(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
    scope: EncounterScope,
    unused_enemies: &[u8],
) -> Result<EncounterApplyReport> {
    use std::collections::BTreeMap;

    if scope == EncounterScope::Scene {
        return randomize_encounters(patcher, seed, mode, unused_enemies);
    }

    // Kingdom scope needs the CDNAME partition; World lumps everything together.
    let kingdom_map = if scope == EncounterScope::Kingdom {
        let cdname = patcher
            .cdname()
            .context("read CDNAME.TXT for kingdom-scoped encounters")?;
        Some(crate::kingdom::KingdomMap::from_cdname(&cdname).context(
            "CDNAME.TXT is missing the map01/map02 kingdom anchors; \
                 use --encounter-scope world instead",
        )?)
    } else {
        None
    };

    // Pass 1: locate every scene and assign it a group key (0 = the single
    // World group; otherwise the kingdom's tag). Scenes are kept so their MAN is
    // only decompressed once.
    struct Located {
        idx: usize,
        group: u64,
        scene: SceneEncounters,
    }
    let mut located: Vec<Located> = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(scene) = SceneEncounters::locate(&entry, idx) else {
            continue;
        };
        let group = match kingdom_map {
            Some(km) => km.kingdom_for_extraction_index(idx).seed_tag(),
            None => 0,
        };
        located.push(Located { idx, group, scene });
    }

    // Per-group monster pool: the union of every member scene's random-encounter
    // ids (boss ids are already excluded by `monster_pool`). Under Random the
    // unused-enemy ids widen each pool.
    let mut pools: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
    for l in &located {
        let pool = pools.entry(l.group).or_default();
        for id in l.scene.monster_pool() {
            if !pool.contains(&id) {
                pool.push(id);
            }
        }
    }
    if mode == DropMode::Random {
        for pool in pools.values_mut() {
            for &id in unused_enemies {
                if !pool.contains(&id) {
                    pool.push(id);
                }
            }
            pool.sort_unstable();
        }
    }

    // Pass 2: reassign + write back. The two modes differ enough - Random fills
    // each scene independently; Shuffle moves ids *between* scenes and so must
    // keep the group multiset intact across re-pack failures - that they own
    // their write-back loops.
    let mut report = EncounterApplyReport::default();
    match mode {
        DropMode::Random => {
            for l in &mut located {
                let pool = pools.get(&l.group).map(Vec::as_slice).unwrap_or(&[]);
                let changed = l.scene.fill_random_slots_from_pool(seed, pool);
                if changed == 0 {
                    continue;
                }
                match l.scene.repack() {
                    Some(stream) => {
                        patcher
                            .patch_prot_entry(l.idx, l.scene.man_offset as u64, &stream)
                            .with_context(|| format!("write scene {} MAN", l.idx))?;
                        report.scenes_changed += 1;
                        report.ids_changed += changed;
                        if !unused_enemies.is_empty() {
                            report.unused_placed += l.scene.count_ids_in(unused_enemies);
                        }
                    }
                    None => report.skipped.push(l.idx),
                }
            }
        }
        DropMode::Shuffle => {
            // A cross-scene shuffle conserves the group-wide id multiset only if
            // every shuffled scene is actually written. A scene whose mutated
            // MAN no longer fits its compressed footprint can't be - and if it
            // silently kept its original ids, the ids it was *given* would
            // vanish while the ids it *gave away* would duplicate. So lock any
            // such scene to its original (removing it from the pool) and reshuffle
            // the rest. Each round locks at least one more scene, so this reaches
            // a fixpoint where every still-shuffled scene re-packs cleanly and
            // the conserved multiset is exactly the unlocked scenes' originals.
            let orig_ids: Vec<Vec<u8>> =
                located.iter().map(|l| l.scene.random_slot_ids()).collect();
            let mut locked = vec![false; located.len()];
            let mut streams: Vec<Option<Vec<u8>>> = vec![None; located.len()];
            loop {
                // Restore every scene to its original ids, so a freshly-locked
                // scene reverts and locked scenes stay put.
                for (i, l) in located.iter_mut().enumerate() {
                    l.scene.apply_random_slots(&orig_ids[i]);
                }
                // Group the still-unlocked members, in ascending scene order.
                let mut groups: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
                for (i, l) in located.iter().enumerate() {
                    if !locked[i] {
                        groups.entry(l.group).or_default().push(i);
                    }
                }
                for (gid, members) in &groups {
                    let mut all_ids: Vec<u8> = Vec::new();
                    for &mi in members {
                        all_ids.extend_from_slice(&orig_ids[mi]);
                    }
                    let mut rng = SplitMix64::new(
                        seed ^ gid.wrapping_mul(0x9E3779B97F4A7C15) ^ SCOPED_SHUFFLE_NONCE,
                    );
                    rng.shuffle(&mut all_ids);
                    let mut cursor = 0;
                    for &mi in members {
                        let n = orig_ids[mi].len();
                        located[mi]
                            .scene
                            .apply_random_slots(&all_ids[cursor..cursor + n]);
                        cursor += n;
                    }
                }
                // Try to re-pack every changed, unlocked scene; lock new failures.
                let mut new_failure = false;
                for i in 0..located.len() {
                    if locked[i] {
                        continue;
                    }
                    if located[i].scene.random_slot_ids() == orig_ids[i] {
                        streams[i] = None; // no-op assignment, nothing to write
                        continue;
                    }
                    match located[i].scene.repack() {
                        Some(stream) => streams[i] = Some(stream),
                        None => {
                            locked[i] = true;
                            streams[i] = None;
                            new_failure = true;
                        }
                    }
                }
                if !new_failure {
                    break;
                }
            }
            // Commit: write the scenes that re-packed; the locked ones were
            // restored to their originals, so report them as skipped.
            for i in 0..located.len() {
                if let Some(stream) = &streams[i] {
                    let l = &located[i];
                    patcher
                        .patch_prot_entry(l.idx, l.scene.man_offset as u64, stream)
                        .with_context(|| format!("write scene {} MAN", l.idx))?;
                    report.scenes_changed += 1;
                    report.ids_changed += located[i]
                        .scene
                        .random_slot_ids()
                        .iter()
                        .zip(&orig_ids[i])
                        .filter(|(a, b)| a != b)
                        .count();
                } else if locked[i] {
                    report.skipped.push(located[i].idx);
                }
            }
        }
    }
    Ok(report)
}

/// Default "strong fight" cut-off for [`SoloStrongConfig`]: a random formation
/// is forced solo when its strongest monster's combat power is at least **twice**
/// (`200`%) the area's native average. Twice the local norm reads as "much
/// stronger than this area expects" without firing on the ordinary spread of a
/// scene's own monsters.
pub const DEFAULT_SOLO_STRONG_THRESHOLD_PCT: u16 = 200;

/// Configuration for the "limit strong fights to a solo enemy" encounter option
/// ([`randomize_encounters_full`]).
#[derive(Debug, Clone, Copy)]
pub struct SoloStrongConfig {
    /// A random formation is collapsed to a single enemy when its strongest
    /// monster's [`crate::monster_stats::combat_power`] is at least this percent
    /// of the scene's native average power. `200` = "twice the area's normal
    /// strength". Values `<= 100` would flag ordinary monsters, so the sensible
    /// range is `> 100`; `0` disables the pass.
    pub threshold_pct: u16,
}

impl Default for SoloStrongConfig {
    fn default() -> Self {
        Self {
            threshold_pct: DEFAULT_SOLO_STRONG_THRESHOLD_PCT,
        }
    }
}

/// Build the per-id combat-power lookup the solo-strong-encounter option compares
/// formations against. Reads the same `battle_data` archive (PROT entry 867) as
/// the stat randomizer and scores each monster by
/// [`crate::monster_stats::combat_power`].
pub fn monster_power_table(patcher: &DiscPatcher) -> Result<crate::encounter::MonsterPowerTable> {
    let stats = current_monster_stats(patcher)?;
    Ok(crate::encounter::MonsterPowerTable::from_powers(
        stats
            .iter()
            .map(|a| (a.monster_id, monster_stats::combat_power(&a.stats))),
    ))
}

/// Record each locatable scene's **native** combat-power baseline (mean random
/// monster power), keyed by PROT entry index. Read **before** any encounter edit
/// so the baseline is the area's authored difficulty, not the post-randomization
/// monsters - see [`encounter::SceneEncounters::baseline_power`].
fn solo_strong_baselines(
    patcher: &DiscPatcher,
    table: &crate::encounter::MonsterPowerTable,
) -> Result<std::collections::HashMap<usize, u32>> {
    let mut out = std::collections::HashMap::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        if let Some(scene) = SceneEncounters::locate(&entry, idx)
            && let Some(base) = scene.baseline_power(table)
        {
            out.insert(idx, base);
        }
    }
    Ok(out)
}

/// Enforce the solo-strong rule across every scene as a post-pass over the
/// **already-randomized** scenes: collapse any random formation whose strongest
/// monster clears `cfg.threshold_pct`% of that scene's pre-saved native baseline
/// down to that lone monster. Decoupled from how the ids were assigned (Scene /
/// Kingdom / World, Shuffle / Random), so it composes with every encounter mode.
/// Returns the number of formations collapsed, id bytes zeroed, and any scene
/// whose collapsed MAN no longer re-packs (left untouched).
fn enforce_solo_strong_encounters(
    patcher: &mut DiscPatcher,
    table: &crate::encounter::MonsterPowerTable,
    baselines: &std::collections::HashMap<usize, u32>,
    cfg: SoloStrongConfig,
) -> Result<(usize, usize, Vec<usize>)> {
    let mut collapsed = 0;
    let mut zeroed = 0;
    let mut skipped = Vec::new();
    for idx in 0..patcher.entry_count() {
        let Some(&baseline) = baselines.get(&idx) else {
            continue;
        };
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(mut scene) = SceneEncounters::locate(&entry, idx) else {
            continue;
        };
        let (c, z) = scene.enforce_solo_strong(table, baseline, cfg.threshold_pct);
        if c == 0 {
            continue;
        }
        match scene.repack() {
            Some(stream) => {
                patcher
                    .patch_prot_entry(idx, scene.man_offset as u64, &stream)
                    .with_context(|| format!("write scene {idx} MAN (solo-strong)"))?;
                collapsed += c;
                zeroed += z;
            }
            None => skipped.push(idx),
        }
    }
    Ok((collapsed, zeroed, skipped))
}

/// Randomize scene encounters ([`randomize_encounters_scoped`]) and, when `solo`
/// is set, additionally enforce the **solo-strong** rule: any random formation
/// that ends up holding a monster much stronger than the area's natives is forced
/// to that single enemy instead of a pack of 2+ (see
/// [`encounter::SceneEncounters::enforce_solo_strong`]).
///
/// The solo-strong pass is computed against each scene's **native** baseline
/// (captured before randomizing) and applied as a post-pass over the randomized
/// scenes, so it composes with every scope (Scene / Kingdom / World) and mode
/// (Shuffle / Random) without perturbing the multiset bookkeeping of the
/// underlying scoped randomization. `solo == None` reproduces
/// [`randomize_encounters_scoped`] byte-for-byte (the archive is not even read),
/// so existing runs are unchanged.
pub fn randomize_encounters_full(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
    scope: EncounterScope,
    unused_enemies: &[u8],
    solo: Option<SoloStrongConfig>,
) -> Result<EncounterApplyReport> {
    // Capture the native baselines (and the power table) BEFORE any edit, so the
    // "strong" judgement is against the area's authored difficulty.
    let solo_ctx = match solo {
        Some(cfg) => {
            let table = monster_power_table(patcher)?;
            let baselines = solo_strong_baselines(patcher, &table)?;
            Some((cfg, table, baselines))
        }
        None => None,
    };

    let mut report = randomize_encounters_scoped(patcher, seed, mode, scope, unused_enemies)?;

    if let Some((cfg, table, baselines)) = solo_ctx {
        let (collapsed, zeroed, skipped) =
            enforce_solo_strong_encounters(patcher, &table, &baselines, cfg)?;
        report.solo_collapsed += collapsed;
        report.ids_changed += zeroed;
        for idx in skipped {
            if !report.skipped.contains(&idx) {
                report.skipped.push(idx);
            }
        }
    }
    Ok(report)
}

/// Outcome of randomizing treasure-chest contents.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ChestApplyReport {
    /// Scene bundles whose MAN was rewritten + written back.
    pub scenes_changed: usize,
    /// Total chest item bytes changed.
    pub items_changed: usize,
    /// Total chest give-item sites found (the randomizable population).
    pub sites_total: usize,
    /// Scene PROT-entry indices whose recompressed MAN would not fit, skipped.
    pub skipped: Vec<usize>,
}

/// Randomize treasure-chest contents (field-VM `GIVE_ITEM` op `0x39`). Chest
/// item ids are global inventory ids (any item works anywhere), so this is a
/// **global** reassignment across every chest on the disc: `Shuffle`
/// redistributes the existing chest-item multiset, `Random` draws each from
/// `item_pool`. Only sites reachable by a clean field-VM walk are touched (see
/// [`crate::chest`]). Scenes whose recompressed MAN overflows are skipped.
///
/// `keep_static` is a set of item ids to leave untouched: any chest whose
/// **original** item is in the set keeps that item (it is excluded from the
/// shuffle multiset entirely, so it never moves and no other chest receives it),
/// and the id is dropped from the `Random` fill pool so it can't be duplicated
/// into another chest. This is how quest / key items stay where the player
/// expects them (see [`crate::items::DEFAULT_STATIC_CHEST_ITEMS`]). Pass an empty
/// set to randomize everything.
pub fn randomize_chests(
    patcher: &mut DiscPatcher,
    item_pool: &[u8],
    seed: u64,
    mode: DropMode,
    keep_static: &std::collections::BTreeSet<u8>,
) -> Result<ChestApplyReport> {
    // Pass 1: collect every scene's chest sites + current items (decoded MAN
    // held for pass 2 so we don't decode twice).
    let mut scenes: Vec<SceneChests> = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        if let Some(sc) = SceneChests::locate(&entry, idx) {
            scenes.push(sc);
        }
    }

    let mut report = ChestApplyReport {
        sites_total: scenes.iter().map(|s| s.sites.len()).sum(),
        ..Default::default()
    };
    if report.sites_total == 0 {
        return Ok(report);
    }

    // Original item id at each (scene, site), kept so a skipped scene can be
    // restored and excluded from the shuffle pool.
    let originals: Vec<Vec<u8>> = scenes.iter().map(|s| s.current_items()).collect();
    let entry_indices: Vec<usize> = scenes.iter().map(|s| s.entry_idx).collect();

    // Indices of scenes whose recompressed MAN won't fit; these keep their
    // original items and are excluded from the (multiset-preserving) shuffle
    // pool. Determined iteratively: a fresh overflow shrinks the pool and we
    // re-plan, so the writable set converges (it only ever shrinks).
    let mut skipped: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();

    match mode {
        DropMode::Shuffle => {
            // Each pass: shuffle the original items of the not-yet-skipped scenes
            // among those same sites, repack, and fold any fresh overflow into
            // `skipped` for the next pass. A permutation over the writable set
            // preserves the chest-item multiset both over the written sites and
            // globally (skipped items never enter the pool, so they neither
            // appear nor disappear).
            let mut streams: Vec<(usize, u64, Vec<u8>)> = Vec::new();
            loop {
                // Restore every site to its original, then assign the shuffle.
                for (i, sc) in scenes.iter_mut().enumerate() {
                    for (k, &orig) in originals[i].iter().enumerate() {
                        sc.set_site(k, orig);
                    }
                }
                // The shuffle pool is the originals of non-skipped, non-static
                // sites only; static items stay put (already restored above) and
                // never enter the pool, so the multiset over the shuffled sites
                // is preserved and a static item can't land in another chest.
                let mut pool: Vec<u8> = (0..scenes.len())
                    .filter(|i| !skipped.contains(i))
                    .flat_map(|i| originals[i].iter().copied())
                    .filter(|item| !keep_static.contains(item))
                    .collect();
                let mut rng = SplitMix64::new(seed);
                rng.shuffle(&mut pool);
                let mut cur = 0usize;
                for (i, sc) in scenes.iter_mut().enumerate() {
                    if skipped.contains(&i) {
                        continue;
                    }
                    for (k, &orig) in originals[i].iter().enumerate() {
                        if keep_static.contains(&orig) {
                            continue; // static site keeps its restored original
                        }
                        sc.set_site(k, pool[cur]);
                        cur += 1;
                    }
                }

                streams.clear();
                let mut fresh_overflow = false;
                for (i, sc) in scenes.iter().enumerate() {
                    if skipped.contains(&i) {
                        continue;
                    }
                    match sc.repack() {
                        Some(stream) => streams.push((sc.entry_idx, sc.man_offset as u64, stream)),
                        None => {
                            skipped.insert(i);
                            fresh_overflow = true;
                        }
                    }
                }
                if !fresh_overflow {
                    break;
                }
            }

            for (i, sc) in scenes.iter().enumerate() {
                if skipped.contains(&i) {
                    continue;
                }
                let changed = sc
                    .sites
                    .iter()
                    .enumerate()
                    .filter(|&(k, &off)| sc.decoded[off] != originals[i][k])
                    .count();
                if changed > 0 {
                    report.scenes_changed += 1;
                    report.items_changed += changed;
                }
            }
            for (entry_idx, man_offset, stream) in streams {
                patcher
                    .patch_prot_entry(entry_idx, man_offset, &stream)
                    .with_context(|| format!("write scene {entry_idx} MAN"))?;
            }
        }
        DropMode::Random => {
            // Static items are dropped from the fill pool so a random chest can't
            // duplicate a quest / key item elsewhere.
            let fill_pool: Vec<u8> = item_pool
                .iter()
                .copied()
                .filter(|item| !keep_static.contains(item))
                .collect();
            if fill_pool.is_empty() {
                return Ok(report);
            }
            // Each site is independent, so an overflowing scene just reverts
            // (no multiset to preserve under Random).
            let mut rng = SplitMix64::new(seed);
            for (i, sc) in scenes.iter_mut().enumerate() {
                let mut changed = 0;
                for k in 0..sc.sites.len() {
                    // A chest whose original item is static keeps it (and consumes
                    // no rng draw, so the stream past it is unaffected by which
                    // sites are static - only the pool composition is).
                    if keep_static.contains(&sc.decoded[sc.sites[k]]) {
                        continue;
                    }
                    let v = fill_pool[rng.below(fill_pool.len())];
                    if sc.decoded[sc.sites[k]] != v {
                        sc.set_site(k, v);
                        changed += 1;
                    }
                }
                if changed == 0 {
                    continue;
                }
                match sc.repack() {
                    Some(stream) => {
                        patcher
                            .patch_prot_entry(sc.entry_idx, sc.man_offset as u64, &stream)
                            .with_context(|| format!("write scene {} MAN", sc.entry_idx))?;
                        report.scenes_changed += 1;
                        report.items_changed += changed;
                    }
                    None => {
                        skipped.insert(i);
                    }
                }
            }
        }
    }

    report.skipped = skipped.into_iter().map(|i| entry_indices[i]).collect();
    Ok(report)
}

/// The CDNAME scene label for a PROT entry, read from the disc's `CDNAME.TXT`
/// (or `"?"` when the map is unavailable). Returns the parsed map so callers can
/// label many entries without re-reading.
pub fn cdname_map(patcher: &DiscPatcher) -> std::collections::BTreeMap<u32, String> {
    patcher
        .read_named_file("CDNAME.TXT")
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|t| legaia_prot::cdname::parse_str(&t).ok())
        .unwrap_or_default()
}

/// One scene-transition ("door / exit") site: where it lives and where it goes.
/// This is the audit surface the door randomizer reassigns. `home_scene` is the
/// CDNAME label of the scene the door is in; `dest_scene` is the inline
/// destination name the `0x3F` op carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoorSite {
    /// PROT entry index of the scene bundle holding this door.
    pub entry_idx: usize,
    /// CDNAME label of the scene the door lives in (e.g. `"town01"`).
    pub home_scene: String,
    /// Byte offset of the `0x3F` op within the scene's decompressed MAN.
    pub op_pc: usize,
    /// Partition of the carrying record (almost always 2).
    pub partition: usize,
    /// Destination-scene `i16` index the op carries.
    pub index: i16,
    /// Destination CDNAME scene label (e.g. `"map01"`).
    pub dest_scene: String,
    /// Destination entry-tile X / Z bytes.
    pub entry_x: u8,
    pub entry_z: u8,
    /// Facing/depth selector byte.
    pub dir: u8,
}

/// Read every scene-transition door on the disc (the randomizable population),
/// in PROT-entry then op-offset order. Purely read-only; decodes each scene MAN
/// once via [`SceneDoors::locate`] and labels the home scene from `CDNAME.TXT`.
pub fn current_doors(patcher: &DiscPatcher) -> Result<Vec<DoorSite>> {
    let cd = cdname_map(patcher);
    let mut out = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(doors) = SceneDoors::locate(&entry, idx) else {
            continue;
        };
        let home = legaia_prot::cdname::block_for(&cd, idx as u32)
            .unwrap_or("?")
            .to_string();
        for s in &doors.sites {
            out.push(DoorSite {
                entry_idx: idx,
                home_scene: home.clone(),
                op_pc: s.op_pc,
                partition: s.partition,
                index: s.index,
                dest_scene: s.name.clone(),
                entry_x: s.entry_x,
                entry_z: s.entry_z,
                dir: s.dir,
            });
        }
    }
    Ok(out)
}

/// A complete scene-transition destination descriptor (everything a `0x3F` op
/// carries). The atomic unit the door randomizer moves between sites: moving the
/// whole descriptor keeps the destination scene, its entry tile, and facing
/// internally consistent.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Dest {
    index: i16,
    name: Vec<u8>,
    entry_x: u8,
    entry_z: u8,
    dir: u8,
}

/// How scene-transition doors are reconnected.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DoorCoupling {
    /// Bidirectional: re-pair doors into two-way connections so walking through
    /// a door and turning around returns you the way you came. Doors that have
    /// no reverse partner (dead-end / one-way story warps) fall back to the
    /// decoupled assignment and are counted in [`DoorApplyReport::unpaired`].
    Coupled,
    /// One-way: every door's destination is reassigned independently, so going
    /// back through the destination's own doors is not guaranteed to return you.
    Decoupled,
}

/// Outcome of randomizing scene-transition doors.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DoorApplyReport {
    /// Scene bundles whose MAN was rewritten + written back.
    pub scenes_changed: usize,
    /// Total door sites whose destination changed.
    pub sites_changed: usize,
    /// Total door sites found (the randomizable population).
    pub sites_total: usize,
    /// Coupled mode only: door sites with no reverse partner (dead-end / one-way
    /// story warps, or doors orphaned by an unequal-direction connection), left
    /// at their original destination because they can't be made two-way.
    pub unpaired: usize,
    /// Coupled mode only: matched connections left at their original
    /// destination because one endpoint's scene couldn't be rebuilt (e.g. an
    /// overflowing overworld hub). Both ends are reverted so the connection
    /// stays genuinely two-way rather than half-applied (a one-way warp).
    pub coupled_kept_original: usize,
    /// Scene PROT-entry indices whose rebuilt MAN overflowed the compressed
    /// footprint or failed validation, so the scene kept its original doors.
    pub skipped: Vec<usize>,
}

/// Build a random involution over the door sites and assign each its new
/// destination so the connection is **bidirectional**: for matched sites `A` and
/// `B`, `A` is sent to where `B` is reached from (`dest(partner_orig(B))`) and
/// vice versa, so walking `A → B`'s doorstep lands you on `B`, whose new
/// destination is `A`'s doorstep. `partner_orig(X)` is the reverse door (same
/// scene-pair, opposite direction); sites without one - or the odd site left
/// over by an odd matching - are **left at their original destination**
/// (untouched) and returned as the `unpaired` count: a door with no clean reverse
/// can't be made two-way, so coupled mode leaves it vanilla rather than giving it
/// a one-way reassignment. `homes[i]` is the CDNAME label of site `i`'s home
/// scene.
///
/// `plan_doors_coupled`'s return: `(dest_new, unpaired_count, matched_pairs,
/// original_partner)`. See its doc for what each element means.
type CoupledPlan = (Vec<Dest>, usize, Vec<(usize, usize)>, Vec<Option<usize>>);

/// Also returns the matched `(a, b)` pairs (the new involution) and the original
/// `partner` array (each site's reverse door, if any). [`randomize_doors`] needs
/// both: a connection is only truly bidirectional if every site it touches gets
/// written, and a site's edit depends on **both** its matched partner and its
/// original reverse. When a scene can't be rebuilt (e.g. an overflowing overworld
/// hub), the revert must propagate transitively along both involutions, or a
/// one-way warp masquerading as coupled survives.
fn plan_doors_coupled(origs: &[Dest], homes: &[String], rng: &mut SplitMix64) -> CoupledPlan {
    use std::collections::HashMap;
    let n = origs.len();
    let name_of = |d: &Dest| String::from_utf8_lossy(&d.name).into_owned();

    // Group site indices by (home_scene, dest_scene) - both directions of a
    // connection live in mirror groups (a,b) / (b,a).
    let mut groups: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for (i, d) in origs.iter().enumerate() {
        groups
            .entry((homes[i].clone(), name_of(d)))
            .or_default()
            .push(i);
    }
    let mut partner = vec![None; n];
    let mut done: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let keys: Vec<(String, String)> = groups.keys().cloned().collect();
    for key in keys {
        if done.contains(&key) {
            continue;
        }
        let (h, d) = key.clone();
        if h == d {
            // Self-scene connection: pair consecutive sites within the group.
            let g = &groups[&key];
            let mut it = g.iter();
            while let (Some(&a), Some(&b)) = (it.next(), it.next()) {
                partner[a] = Some(b);
                partner[b] = Some(a);
            }
            done.insert(key);
            continue;
        }
        let rev = (d.clone(), h.clone());
        done.insert(key.clone());
        done.insert(rev.clone());
        if let (Some(g1), Some(g2)) = (groups.get(&key), groups.get(&rev)) {
            // Only couple a connection whose two directions have the SAME number
            // of doors. If they differ, pairing `min(len)` of them would leave
            // the majority direction's excess doors at their original
            // destination while their lone reverse gets matched away - producing
            // a dangling one-way edge (`HA→HB` survives, `HB→HA` vanishes). The
            // safe choice is to leave the whole unbalanced connection static so
            // both directions stay intact (it's reported in `unpaired`).
            if g1.len() == g2.len() {
                for i in 0..g1.len() {
                    partner[g1[i]] = Some(g2[i]);
                    partner[g2[i]] = Some(g1[i]);
                }
            }
        }
    }

    let mut dest_new = origs.to_vec();

    // Match partnered doors into the new involution, constrained to
    // **length-preserving** swaps. When `a` matches `b`, `a` receives the
    // descriptor `origs[partner[b]]` (its name is `b`'s home-scene label) and
    // vice versa. The rewrite keeps the MAN's decompressed size unchanged - so no
    // scene grows and none can overflow on recompress - exactly when the name
    // lengths line up: `len(home[b]) == len(dest[a])` and `len(home[a]) ==
    // len(dest[b])`. Bucketing each door by `key = (len(home), len(dest))` and
    // matching a `(p, q)` bucket against the mirror `(q, p)` bucket guarantees
    // both. This is what lets coupled mode randomize the overworld hubs (which
    // can't be grown in place) while staying two-way; the variable-length
    // relocation path is reserved for decoupled mode.
    let key = |i: usize| (homes[i].len(), origs[i].name.len());
    let mut buckets: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (i, p) in partner.iter().enumerate() {
        if p.is_some() {
            buckets.entry(key(i)).or_default().push(i);
        }
    }
    let mut matched_pairs: Vec<(usize, usize)> = Vec::new();
    let mut handled: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    let mut bkeys: Vec<(usize, usize)> = buckets.keys().copied().collect();
    bkeys.sort_unstable(); // deterministic iteration order
    for k in bkeys {
        if handled.contains(&k) {
            continue;
        }
        let (p, q) = k;
        if p == q {
            handled.insert(k);
            let mut g = buckets[&k].clone();
            rng.shuffle(&mut g);
            let mut i = 0;
            while i + 1 < g.len() {
                matched_pairs.push((g[i], g[i + 1]));
                i += 2;
            }
        } else {
            let rk = (q, p);
            handled.insert(k);
            handled.insert(rk);
            let mut ga = buckets.get(&k).cloned().unwrap_or_default();
            let mut gb = buckets.get(&rk).cloned().unwrap_or_default();
            rng.shuffle(&mut ga);
            rng.shuffle(&mut gb);
            for i in 0..ga.len().min(gb.len()) {
                matched_pairs.push((ga[i], gb[i]));
            }
        }
    }
    for &(a, b) in &matched_pairs {
        dest_new[a] = origs[partner[b].unwrap()].clone();
        dest_new[b] = origs[partner[a].unwrap()].clone();
    }

    // Unpaired: every door not placed in a matched pair - no reverse partner
    // (dead-end / one-way story warp, or a door orphaned by an unequal-direction
    // connection) or no length-compatible partner to swap with. These keep their
    // ORIGINAL destination (coupled mode never gives a door a one-way
    // reassignment), so `dest_new` already holds the right bytes - only the count
    // is reported.
    let matched: std::collections::HashSet<usize> =
        matched_pairs.iter().flat_map(|&(a, b)| [a, b]).collect();
    let unpaired = (0..n).filter(|i| !matched.contains(i)).count();
    (dest_new, unpaired, matched_pairs, partner)
}

/// Randomize scene-transition doors, one-way ([`DoorCoupling::Decoupled`]) or
/// bidirectional ([`DoorCoupling::Coupled`]). Each door's destination descriptor
/// (scene + entry tile + facing) is the atomic unit moved between sites.
///
/// - **Decoupled**: `Shuffle` permutes the existing destinations across all
///   doors (every scene stays reachable as some door's target); `Random` draws
///   each door's destination from the global pool.
/// - **Coupled**: re-pairs doors into two-way connections (the `mode` is treated
///   as the matching's randomness; doors with no reverse partner get the
///   decoupled fallback - see `plan_doors_coupled`).
///
/// Because the destination name is variable length, this uses the
/// [`crate::door::SceneDoors::rebuild`] relocation path (decompress → resize the
/// `0x3F` ops → fix the partition tables / section offset / intra-record jumps →
/// recompress → rewrite the descriptor size word). A scene whose rebuilt MAN
/// overflows its compressed footprint or fails validation keeps its original
/// doors and is recorded in `skipped`.
pub fn randomize_doors(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
    coupling: DoorCoupling,
) -> Result<DoorApplyReport> {
    use legaia_asset::man_edit::DestEdit;
    use legaia_asset::scene_asset_table::encode_size_word;

    let cd = cdname_map(patcher);

    // Pass 1: collect every scene's doors (decoded MAN held for pass 2).
    let mut scenes: Vec<SceneDoors> = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        if let Some(d) = SceneDoors::locate(&entry, idx) {
            scenes.push(d);
        }
    }

    // Flatten to a global ordered list of original destinations + home labels.
    let mut origs: Vec<Dest> = Vec::new();
    let mut homes: Vec<String> = Vec::new();
    for s in &scenes {
        let home = legaia_prot::cdname::block_for(&cd, s.entry_idx as u32)
            .unwrap_or("?")
            .to_string();
        for site in &s.sites {
            origs.push(Dest {
                index: site.index,
                name: site.name.clone().into_bytes(),
                entry_x: site.entry_x,
                entry_z: site.entry_z,
                dir: site.dir,
            });
            homes.push(home.clone());
        }
    }

    let mut report = DoorApplyReport {
        sites_total: origs.len(),
        ..Default::default()
    };
    if origs.is_empty() {
        return Ok(report);
    }

    // Plan the new destination for each global site index. `match_of` is the new
    // (coupled) involution; `partner_of` is each site's original reverse door.
    let n = origs.len();
    let mut rng = SplitMix64::new(seed);
    let mut match_of: Vec<Option<usize>> = vec![None; n];
    let mut partner_of: Vec<Option<usize>> = vec![None; n];
    let new_descs: Vec<Dest> = match coupling {
        DoorCoupling::Coupled => {
            let (descs, unpaired, pairs, partner) = plan_doors_coupled(&origs, &homes, &mut rng);
            report.unpaired = unpaired;
            for (a, b) in pairs {
                match_of[a] = Some(b);
                match_of[b] = Some(a);
            }
            partner_of = partner;
            descs
        }
        DoorCoupling::Decoupled => match mode {
            DropMode::Shuffle => {
                let mut v = origs.clone();
                rng.shuffle(&mut v);
                v
            }
            DropMode::Random => (0..origs.len())
                .map(|_| origs[rng.below(origs.len())].clone())
                .collect(),
        },
    };

    // Map each global site index to its scene (for the coupled revert pass).
    let mut site_scene = vec![0usize; origs.len()];
    {
        let mut g = 0usize;
        for (si, s) in scenes.iter().enumerate() {
            for _ in 0..s.sites.len() {
                site_scene[g] = si;
                g += 1;
            }
        }
    }

    // Pass 2: rebuild each scene from the planned destinations and collect the
    // streams to write. `forced` is the set of global site indices pinned to
    // their ORIGINAL destination (a forced site contributes no edit).
    //
    // The coupled revert is a transitive closure. If a site can't be written -
    // its scene overflows - it keeps its original destination, which only stays a
    // valid two-way connection if its matched partner *and* its original reverse
    // also keep theirs. So a forced site forces both `match_of[X]` (the new
    // partner, which was sending players to X) and `partner_of[X]` (the original
    // reverse, which X's now-reverted destination points back at). Propagating
    // along both involutions reverts whole alternating cycles, never leaving a
    // dangling one-way edge. Forcing only removes edits (shrinks scenes), so the
    // skipped set only shrinks and the loop converges (decoupled mode has empty
    // involutions, so it runs once and reverts nothing).
    let mut forced: std::collections::HashSet<usize> = std::collections::HashSet::new();
    // Per scene: (scene index, rebuilt stream, new decompressed size, edit count).
    let mut streams: Vec<(usize, Vec<u8>, u32, usize)> = Vec::new();
    let mut skipped_scenes: std::collections::HashSet<usize> = std::collections::HashSet::new();
    loop {
        streams.clear();
        skipped_scenes.clear();
        let mut g = 0usize;
        for (si, scene) in scenes.iter().enumerate() {
            let base = g;
            g += scene.sites.len();
            let mut edits: Vec<DestEdit> = Vec::new();
            for (k, site) in scene.sites.iter().enumerate() {
                if forced.contains(&(base + k)) {
                    continue; // pinned to original - no edit
                }
                let d = &new_descs[base + k];
                let unchanged = d.index == site.index
                    && d.name == site.name.as_bytes()
                    && d.entry_x == site.entry_x
                    && d.entry_z == site.entry_z
                    && d.dir == site.dir;
                if unchanged {
                    continue;
                }
                edits.push(DestEdit {
                    op_pc: site.op_pc,
                    index: d.index,
                    name: d.name.clone(),
                    entry_x: d.entry_x,
                    entry_z: d.entry_z,
                    dir: d.dir,
                });
            }
            if edits.is_empty() {
                continue;
            }
            match scene.rebuild(&edits) {
                Some((stream, new_size)) => streams.push((si, stream, new_size, edits.len())),
                None => {
                    skipped_scenes.insert(si);
                }
            }
        }

        // Seed the closure with every coupled site in an overflowing scene, then
        // propagate along both involutions until no new site is forced.
        let mut stack: Vec<usize> = (0..n)
            .filter(|&i| {
                !forced.contains(&i)
                    && (match_of[i].is_some() || partner_of[i].is_some())
                    && skipped_scenes.contains(&site_scene[i])
            })
            .collect();
        let mut new_force = false;
        while let Some(x) = stack.pop() {
            if !forced.insert(x) {
                continue;
            }
            new_force = true;
            if let Some(y) = match_of[x]
                && !forced.contains(&y)
            {
                stack.push(y);
            }
            if let Some(p) = partner_of[x]
                && !forced.contains(&p)
            {
                stack.push(p);
            }
        }
        if !new_force {
            break;
        }
    }

    // Coupled sites reverted to original (their cycle touched an un-writable
    // scene), reported so the user knows what stayed vanilla to keep returns
    // honest.
    report.coupled_kept_original = (0..n)
        .filter(|i| forced.contains(i) && (match_of[*i].is_some() || partner_of[*i].is_some()))
        .count();

    // Write the converged set.
    for (si, stream, new_size, n_edits) in &streams {
        let scene = &scenes[*si];
        patcher
            .patch_prot_entry(
                scene.entry_idx,
                scene.man_descriptor_off as u64,
                &encode_size_word(0x03, *new_size).to_le_bytes(),
            )
            .with_context(|| format!("write scene {} MAN size word", scene.entry_idx))?;
        patcher
            .patch_prot_entry(scene.entry_idx, scene.man_offset as u64, stream)
            .with_context(|| format!("write scene {} MAN", scene.entry_idx))?;
        report.scenes_changed += 1;
        report.sites_changed += n_edits;
    }
    report.skipped = skipped_scenes
        .iter()
        .map(|&si| scenes[si].entry_idx)
        .collect();
    report.skipped.sort_unstable();
    Ok(report)
}

/// Outcome of randomizing intra-town (house / interior) doors.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HouseDoorApplyReport {
    /// Scene bundles whose MAN was rewritten + written back.
    pub scenes_changed: usize,
    /// Total door-warp target tiles whose operand changed.
    pub sites_changed: usize,
    /// Total classified door-warp sites found (IN + OUT classes).
    pub sites_total: usize,
    /// Scene PROT-entry indices whose recompressed MAN overflowed (kept original).
    pub skipped: Vec<usize>,
}

/// Read every classified intra-town door warp on the disc (the house-door
/// population), in PROT-entry order: the cross-context player `MOVE_TO`s
/// (`0xA3 0xF8`) in named partition-0 door records (see
/// [`crate::house_door`]). NPC / prop / cutscene movement is excluded by
/// construction. Purely read-only audit surface.
pub fn current_house_doors(patcher: &DiscPatcher) -> Result<Vec<(usize, u8, u8)>> {
    let mut out = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(sd) = SceneHouseDoors::locate(&entry, idx) else {
            continue;
        };
        for (xb, zb) in sd.current_targets() {
            out.push((idx, xb & 0x7F, zb & 0x7F));
        }
    }
    Ok(out)
}

/// Randomize intra-town (house / interior) doors by a **per-scene,
/// class-preserving shuffle** of the player door-warp target tiles: `IN`-class
/// (interior landing) targets permute among `IN` sites, `OUT`-class (exterior
/// doorstep) targets among `OUT` sites (see [`crate::house_door`] - exiting
/// any interior always lands back outside, so no softlock is constructible).
/// Each scene's MAN is recompressed (same-size operand edits) and written back
/// when it fits; a scene that overflows keeps its original tiles. Only
/// [`DropMode::Shuffle`] is meaningful (a random draw would place the player
/// off-map), so a non-shuffle mode is a no-op.
pub fn randomize_house_doors(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
) -> Result<HouseDoorApplyReport> {
    let mut report = HouseDoorApplyReport::default();
    if !crate::house_door::supported_mode(mode) {
        return Ok(report);
    }
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(mut scene) = SceneHouseDoors::locate(&entry, idx) else {
            continue;
        };
        report.sites_total += scene.sites.len();
        let changed = scene.shuffle(seed);
        if changed == 0 {
            continue;
        }
        match scene.repack() {
            Some(stream) => {
                patcher
                    .patch_prot_entry(idx, scene.man_offset as u64, &stream)
                    .with_context(|| format!("write scene {idx} MAN"))?;
                report.scenes_changed += 1;
                report.sites_changed += changed;
            }
            None => report.skipped.push(idx),
        }
    }
    Ok(report)
}

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

/// Read the new game's current starting inventory (`(item_id, count)` slots) by
/// decoding the seed code region in `SCUS_942.54`. Vanilla retail is a single
/// slot - Healing Leaf (`0x77`) ×5. Purely read-only.
pub fn current_starting_items(patcher: &DiscPatcher) -> Result<Vec<(u8, u8)>> {
    let scus = patcher
        .read_named_file(crate::steal::SCUS_NAME)
        .context("read SCUS_942.54")?;
    let inv = legaia_asset::new_game::StartingInventory::from_scus(&scus)
        .context("decode starting-inventory seed")?;
    Ok(inv.items().to_vec())
}

/// Read whether the new game currently presets the all-towns Door-of-Wind warp
/// bitmask (the `--all-warps` toggle). Purely read-only.
pub fn current_all_warps(patcher: &DiscPatcher) -> Result<bool> {
    let scus = patcher
        .read_named_file(crate::steal::SCUS_NAME)
        .context("read SCUS_942.54")?;
    Ok(legaia_asset::new_game::scus_unlocks_all_warps(&scus).unwrap_or(false))
}

/// Outcome of randomizing the new game's starting seed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StartingItemsApplyReport {
    /// Number of starting-item slots written (`0..=MAX_STARTING_ITEMS`).
    pub items_set: usize,
    /// The seeded `(item_id, count)` slots, for the manifest / CLI summary.
    pub items: Vec<(u8, u8)>,
    /// Whether the all-towns Door-of-Wind warp bitmask was preset.
    pub all_warps: bool,
}

/// Rewrite the new game's starting-seed code in `SCUS_942.54` from
/// [`StartingSeedOptions`].
///
/// Two independent reclaimable regions of `FUN_80034A6C` are patched in place
/// (same-size, no executable growth): the inventory-seed region takes the first
/// [`crate::starting_items::INV_REGION_SLOTS`] `(id, count)` slots, and the
/// warp-preset region takes EITHER the visited-towns warp preset (when
/// `all_warps` is set) OR the last couple of item slots that overflow the
/// inventory region (when it is not). So convenience items and a full random
/// fill stay additive up to the combined capacity instead of crowding each
/// other out. [`plan_seed`] resolves the options into the concrete plan,
/// capacity-aware. With inactive options nothing is written (callers guard on
/// [`StartingSeedOptions::is_active`]). Deterministic in `(seed, opts)`.
///
/// [`StartingSeedOptions`]: crate::starting_items::StartingSeedOptions
/// [`plan_seed`]: crate::starting_items::plan_seed
pub fn randomize_starting_items(
    patcher: &mut DiscPatcher,
    seed: u64,
    opts: &crate::starting_items::StartingSeedOptions,
) -> Result<StartingItemsApplyReport> {
    let scus = patcher
        .read_named_file(crate::steal::SCUS_NAME)
        .context("read SCUS_942.54")?;
    let inv_off = legaia_asset::new_game::starting_inv_seed_file_offset(&scus)
        .context("locate starting-inventory seed region in SCUS_942.54")? as u64;
    let plan = crate::starting_items::plan_seed(seed, opts);

    // Inventory seed region: always rewritten when active (this also drops the
    // zero-loop, which the warp region's writes below rely on surviving).
    let inv_patch = crate::starting_items::build_seed_patch_for(&plan);
    patcher
        .patch_named_file(crate::steal::SCUS_NAME, inv_off, &inv_patch)
        .with_context(|| format!("write starting-item seed at SCUS offset {inv_off:#x}"))?;

    // Warp-preset region: holds the visited-towns bitmask when all-warps is on,
    // otherwise the item slots that overflow the inventory region (if any). When
    // neither applies it keeps its original (redundant) bytes.
    let overflow = crate::starting_items::overflow_items(&plan);
    if plan.all_warps || !overflow.is_empty() {
        let warp_off = legaia_asset::new_game::warp_seed_file_offset(&scus)
            .context("locate warp-preset region in SCUS_942.54")? as u64;
        let warp_patch = if plan.all_warps {
            crate::starting_items::build_warp_patch()
        } else {
            crate::starting_items::build_warp_items_patch(overflow)
        };
        patcher
            .patch_named_file(crate::steal::SCUS_NAME, warp_off, &warp_patch)
            .with_context(|| format!("write warp-preset region at SCUS offset {warp_off:#x}"))?;
    }

    Ok(StartingItemsApplyReport {
        items_set: plan.items.len(),
        items: plan.items,
        all_warps: plan.all_warps,
    })
}

/// Read the new game's current starting level for slot 0. Retail seeds the lead
/// character's experience cell `+0x0` to `0` (level 1, since the level is derived
/// from cumulative experience). When the starting-level randomizer is applied, the
/// seeded experience rides in the `addiu $t0, $zero, imm` preload at
/// [`legaia_asset::new_game::CURRENT_XP_PRELOAD_VA`]; this reads that immediate (when
/// present) and derives the level from the disc XP thresholds. Purely read-only.
pub fn current_starting_level(patcher: &DiscPatcher) -> Result<u8> {
    let scus = patcher
        .read_named_file(crate::steal::SCUS_NAME)
        .context("read SCUS_942.54")?;
    let off = legaia_asset::new_game::scus_file_offset(
        &scus,
        legaia_asset::new_game::CURRENT_XP_PRELOAD_VA,
    )
    .context("locate current-XP preload in SCUS_942.54")?;
    let word = u32::from_le_bytes(
        scus.get(off..off + 4)
            .context("current-XP preload out of range")?
            .try_into()
            .unwrap(),
    );
    // Vanilla holds the slot-3 `+0x4` store here, not an `addiu $t0` (`0x2408`); only
    // the randomizer's preload carries a seeded experience value. Absent it, level 1.
    if word >> 16 != 0x2408 {
        return Ok(1);
    }
    let xp = (word & 0xFFFF) as u32;
    let thresholds = legaia_asset::level_up_tables::xp_thresholds_from_scus(&scus)
        .context("read XP thresholds from SCUS_942.54")?;
    // Level N when reach(N) < xp <= reach(N+1); reach(m) = thresholds[m - 2]. The
    // seeded value is the band midpoint, so it lands strictly inside level N.
    let mut level = 1u8;
    for (i, &reach) in thresholds.iter().enumerate() {
        if xp > reach {
            level = (i as u8 + 2).min(legaia_asset::level_up_tables::MAX_LEVEL as u8);
        } else {
            break;
        }
    }
    Ok(level)
}

/// Outcome of seeding the new game's starting level.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StartingLevelReport {
    /// The starting level seeded.
    pub level: u8,
    /// In-band cumulative XP for the level, written to **every** growth slot's `+0x0`
    /// cell (Vahn / Noa / Gala).
    pub current_xp: u16,
    /// Next-level XP threshold, written to **every** growth slot's `+0x4` cell.
    pub next_threshold: u16,
    /// The level-`N` stats written into the lead (slot 0) template, in template
    /// order (`hp, mp, agl, atk, udf, ldf, spd, int`). Equal to `party_stats[0]`.
    pub stats: [u16; 8],
    /// The number of party slots seeded to level `N` (Vahn / Noa / Gala - the
    /// growth-capable slots). The displayed level (`+0x130`) is stamped on every
    /// roster slot by the seed loop; these are the slots whose stats were also
    /// leveled to match.
    pub slots_leveled: usize,
}

/// Seed the new game so the starting party begins at `level` instead of level 1
/// (see [`crate::starting_level`]).
///
/// Same-size in-place edits in `SCUS_942.54` across the seed routine `FUN_800560B4`
/// and the starting-party template:
/// - the seed loop's displayed-level literal + store, so it writes `+0x130 = level`
///   (keeping magic rank `+0x131 = 1`) into **every** party record;
/// - each growth-capable slot's eight `u16` template stats (Vahn / Noa / Gala),
///   recomputed to the level via the disc's own growth curves, so the displayed
///   level the loop stamps and the stats stay coherent across the roster (the 4th
///   slot, Terra, has no growth curve and keeps its base stats);
/// - **each growth-capable slot's** (Vahn / Noa / Gala) current-experience cell
///   `+0x0`, seeded to an in-band level-`N` value via a single `$t0` preload + three
///   `sw $t0, <+0x0>($s0)` stores that repurpose the slot-3 / slot-1 / slot-2
///   next-level-threshold seeds (and one redundant `lui`), and each slot's
///   next-level-threshold cell (`+0x4`), set to `reach(level + 1)`, so every
///   character's status readout is coherent - not just the lead's (an earlier version
///   seeded only slot 0, leaving Noa with experience `0` and Gala with a stale level-1
///   threshold). All three `+0x4` cells take the same threshold; the small per-slot
///   `FUN_801E9504` correction is re-applied by the applier on each character's first
///   post-seed level-up.
///
/// `level` must be in
/// [`crate::starting_level::MIN_STARTING_LEVEL`]`..=`[`crate::starting_level::MAX_STARTING_LEVEL`];
/// callers guard on [`crate::starting_level::is_active`]. Deterministic.
pub fn apply_starting_level(patcher: &mut DiscPatcher, level: u8) -> Result<StartingLevelReport> {
    use legaia_asset::new_game::{
        CURRENT_XP_PRELOAD_VA, CURRENT_XP_STORE_VA, GALA_XP_STORE_VA, LEVEL_SEED_VA,
        LEVEL_STORE_REDUNDANT_VA, LEVEL_STORE_VA, NOA_XP_STORE_VA, RECORD_STRIDE,
        STARTING_XP_SEED_VA, live_record_xp_offset, party_template_file_offset, scus_file_offset,
    };
    let scus = patcher
        .read_named_file(crate::steal::SCUS_NAME)
        .context("read SCUS_942.54")?;
    let plan = crate::starting_level::plan(&scus, level)?;

    // Each seed-routine instruction the level edit rewrites, with its 4-byte
    // replacement word. See `crate::starting_level` for the encodings + rationale:
    // the next-level threshold (+0x4) and current experience (+0x0, via one $t0 preload
    // + one store per growth slot) make every character's readouts + progression
    // coherent, and the loop's level literal + stores set the displayed-level cell
    // +0x130 (keeping magic rank +0x131 at 1) so the character actually reads as
    // level N. The three experience stores all source $t0 (the shared preload) and
    // dropping the per-slot threshold reloads leaves $v0 holding reach(N+1) for the
    // unmodified Noa/Gala +0x4 stores downstream.
    let edits: [(u32, [u8; 4]); 8] = [
        (
            STARTING_XP_SEED_VA,
            crate::starting_level::next_threshold_instruction(plan.next_threshold),
        ),
        (
            CURRENT_XP_PRELOAD_VA,
            crate::starting_level::current_xp_preload_instruction(plan.current_xp),
        ),
        // Three cumulative-experience stores `sw $t0, <+0x0>($s0)`, one per growth slot
        // (Vahn / Noa / Gala), repurposing the slot-1 / slot-2 threshold literals and a
        // redundant `lui`.
        (
            CURRENT_XP_STORE_VA,
            crate::starting_level::cumulative_xp_store_instruction(live_record_xp_offset(0)),
        ),
        (
            NOA_XP_STORE_VA,
            crate::starting_level::cumulative_xp_store_instruction(live_record_xp_offset(1)),
        ),
        (
            GALA_XP_STORE_VA,
            crate::starting_level::cumulative_xp_store_instruction(live_record_xp_offset(2)),
        ),
        (
            LEVEL_SEED_VA,
            crate::starting_level::level_literal_instruction(plan.level),
        ),
        (
            LEVEL_STORE_VA,
            crate::starting_level::level_store_instruction(),
        ),
        (
            LEVEL_STORE_REDUNDANT_VA,
            crate::starting_level::nop_instruction(),
        ),
    ];
    for (va, word) in edits {
        let off = scus_file_offset(&scus, va)
            .with_context(|| format!("locate seed instruction {va:#x} in SCUS_942.54"))?
            as u64;
        patcher
            .patch_named_file(crate::steal::SCUS_NAME, off, &word)
            .with_context(|| format!("write seed instruction at SCUS offset {off:#x}"))?;
    }

    // Write each growth-capable slot's level-N stats into its template stat block
    // (the first 16 of each record's RECORD_STRIDE bytes; the name field follows).
    // The seed loop copies these into the live records, so every leveled slot's
    // stats match the displayed level the loop stamps.
    let tmpl_off = party_template_file_offset(&scus)
        .context("locate starting-party template in SCUS_942.54")? as u64;
    for (slot, stats) in plan.party_stats.iter().enumerate() {
        let off = tmpl_off + (slot * RECORD_STRIDE) as u64;
        patcher
            .patch_named_file(
                crate::steal::SCUS_NAME,
                off,
                &crate::starting_level::stat_block(stats),
            )
            .with_context(|| {
                format!("write level-{level} stats for slot {slot} at SCUS offset {off:#x}")
            })?;
    }

    Ok(StartingLevelReport {
        level: plan.level,
        current_xp: plan.current_xp,
        next_threshold: plan.next_threshold,
        stats: plan.stats,
        slots_leveled: plan.party_stats.len(),
    })
}

/// Outcome of the starting-bag script injection.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StartingBagReport {
    /// PROT entry whose MAN was injected (the opening scene's), if it applied.
    pub scene_entry: Option<usize>,
    /// The `(item_id, count)` bag granted.
    pub items: Vec<(u8, u8)>,
    /// The persistent story-flag bit guarding the once-only grant.
    pub guard_bit: u16,
    /// `true` when the grant block was injected + written back.
    pub applied: bool,
}

/// Seed an arbitrarily large starting bag by injecting a guarded run of silent
/// `GIVE_ITEM` ops into the opening scene's entry script (see
/// [`crate::starting_bag`]). This lifts the 7-slot cap of the direct
/// [`randomize_starting_items`] seed: the bag holds every `(id, count)` in `items`
/// (convenience items + the full requested random fill).
///
/// The opening interactive scene is `town01` ([`legaia_asset::new_game::OPENING_SCENE`]);
/// its MAN entry is found through `CDNAME.TXT`, the guarded block is spliced at the
/// entry script's first opcode, the MAN is recompressed in place, and the
/// descriptor's decompressed-size word is bumped. The guard (persistent SC story
/// flag `guard_bit`) keeps the grant to the first visit. If the scene can't be
/// located, carries an absolute reference that makes the insert unsafe, or the
/// recompressed MAN overflows its footprint, nothing is written and
/// `report.applied` is `false`. Deterministic.
pub fn apply_starting_bag(
    patcher: &mut DiscPatcher,
    items: &[(u8, u8)],
    guard_bit: u16,
) -> Result<StartingBagReport> {
    use legaia_asset::scene_asset_table::encode_size_word;
    let mut report = StartingBagReport {
        items: items.to_vec(),
        guard_bit,
        ..Default::default()
    };
    if items.iter().all(|&(_, c)| c == 0) {
        return Ok(report);
    }

    let cdname = patcher
        .cdname()
        .context("read CDNAME.TXT for the starting-bag scene")?;
    let scene = legaia_asset::new_game::OPENING_SCENE; // "town01"
    let (raw_start, raw_end) = legaia_prot::cdname::block_range_for_name(&cdname, scene)
        .with_context(|| format!("CDNAME.TXT has no {scene} block"))?;
    // CDNAME #define numbers are raw-TOC indices; extraction = raw - 2.
    let ext_start =
        (raw_start as i64 - legaia_prot::cdname::RAW_TOC_INDEX_OFFSET as i64).max(0) as usize;
    let ext_end =
        (raw_end as i64 - legaia_prot::cdname::RAW_TOC_INDEX_OFFSET as i64).max(0) as usize;

    for ext in ext_start..ext_end.min(patcher.entry_count()) {
        let entry = patcher
            .read_entry(ext)
            .with_context(|| format!("read PROT entry {ext}"))?;
        let Some(inj) = crate::starting_bag::SceneBagInject::locate(&entry, ext) else {
            continue;
        };
        let Some((stream, new_size)) = inj.rebuild(items, guard_bit) else {
            continue;
        };
        patcher
            .patch_prot_entry(
                ext,
                inj.man_descriptor_off() as u64,
                &encode_size_word(0x03, new_size).to_le_bytes(),
            )
            .with_context(|| format!("write {scene} MAN size word"))?;
        patcher
            .patch_prot_entry(ext, inj.man_offset() as u64, &stream)
            .with_context(|| format!("write {scene} MAN"))?;
        report.scene_entry = Some(ext);
        report.applied = true;
        break;
    }
    Ok(report)
}

/// One character's weapon-specialty reassignment, for the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecialtyAssignment {
    /// Character name.
    pub character: String,
    /// The family this character specialized in before.
    pub from: String,
    /// The family this character specializes in after.
    pub to: String,
}

/// Outcome of a weapon-specialty randomization.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpecialtyReport {
    /// Per-character favored-family reassignment.
    pub assignments: Vec<SpecialtyAssignment>,
    /// Weapon sections whose arm cost was rewritten.
    pub weapons_changed: usize,
    /// Weapon sections skipped because the re-compressed section wouldn't fit
    /// its slot footprint.
    pub weapons_skipped_fit: usize,
}

/// Read the current favored family of each character straight from its player
/// battle file (the family whose weapons carry [`FAVORED_COST`]), for the
/// read-only listing. Returns `None` when the player files aren't present.
pub fn current_specialties(patcher: &DiscPatcher) -> Result<Vec<SpecialtyAssignment>> {
    use crate::weapon_specialty::{self, Family};
    use legaia_asset::battle_data_pack;
    let mut out = Vec::new();
    for player in &weapon_specialty::PLAYERS {
        let Ok(buf) = patcher.read_entry(player.entry) else {
            continue;
        };
        let Some(pack) = battle_data_pack::detect(&buf) else {
            continue;
        };
        // Tally, per family, how many of that family's weapons read FAVORED_COST.
        let mut favored_hits = [(0usize, 0usize); 3]; // (favored, total) per family
        for (idx, rec) in pack.records.iter().enumerate() {
            let Some(fam) = weapon_specialty::weapon_family(rec.id as u8) else {
                continue;
            };
            let Ok(dec) = battle_data_pack::decode_record(&buf, &pack, idx) else {
                continue;
            };
            let Some(coff) = weapon_specialty::arm_cost_offset(&dec.bytes) else {
                continue;
            };
            let fi = match fam {
                Family::Blade => 0,
                Family::Claw => 1,
                Family::Club => 2,
            };
            favored_hits[fi].1 += 1;
            if dec.bytes[coff] == weapon_specialty::FAVORED_COST {
                favored_hits[fi].0 += 1;
            }
        }
        // The favored family is the one with the highest favored ratio.
        let fams = [Family::Blade, Family::Claw, Family::Club];
        let cur = (0..3)
            .filter(|&i| favored_hits[i].1 > 0)
            .max_by(|&a, &b| {
                let ra = favored_hits[a].0 as f64 / favored_hits[a].1 as f64;
                let rb = favored_hits[b].0 as f64 / favored_hits[b].1 as f64;
                ra.partial_cmp(&rb).unwrap()
            })
            .map(|i| fams[i])
            .unwrap_or(player.vanilla);
        out.push(SpecialtyAssignment {
            character: player.name.to_string(),
            from: player.vanilla.label().to_string(),
            to: cur.label().to_string(),
        });
    }
    Ok(out)
}

/// Randomize each character's weapon specialty (see [`crate::weapon_specialty`]).
///
/// Permutes the three favored families among Vahn / Noa / Gala and rewrites the
/// per-(character, weapon) arm-cost byte in the player battle files so each
/// character's new favored family is single-cost and every other class is
/// off-class. The byte sits inside an LZS-compressed section, so each touched
/// section is decompressed, edited, and re-compressed; a section whose
/// re-compressed stream wouldn't fit its slot is left unchanged (counted in the
/// report) rather than aborting the run.
pub fn randomize_weapon_specialty(patcher: &mut DiscPatcher, seed: u64) -> Result<SpecialtyReport> {
    use crate::weapon_specialty;
    use legaia_asset::battle_data_pack;

    let favored = weapon_specialty::plan_favored(seed);
    let mut report = SpecialtyReport::default();

    for (i, player) in weapon_specialty::PLAYERS.iter().enumerate() {
        let new_fav = favored[i];
        report.assignments.push(SpecialtyAssignment {
            character: player.name.to_string(),
            from: player.vanilla.label().to_string(),
            to: new_fav.label().to_string(),
        });

        let Ok(buf) = patcher.read_entry(player.entry) else {
            continue;
        };
        let Some(pack) = battle_data_pack::detect(&buf) else {
            continue;
        };

        for (idx, rec) in pack.records.iter().enumerate() {
            let Some(fam) = weapon_specialty::weapon_family(rec.id as u8) else {
                continue;
            };
            let Ok(dec) = battle_data_pack::decode_record(&buf, &pack, idx) else {
                continue;
            };
            let Some(coff) = weapon_specialty::arm_cost_offset(&dec.bytes) else {
                continue;
            };
            let new_cost = weapon_specialty::cost_for(fam, new_fav);
            if dec.bytes[coff] == new_cost {
                continue; // already correct (idempotent)
            }
            let mut decoded = dec.bytes.clone();
            decoded[coff] = new_cost;
            let recompressed = legaia_lzs::compress(&decoded);

            // Available footprint for the LZS stream: the slot minus its 4-byte
            // decoded-size prefix. A stream too large to re-pack is skipped.
            let avail = (rec.size as usize).saturating_sub(4);
            if recompressed.len() > avail {
                report.weapons_skipped_fit += 1;
                continue;
            }
            let stream_off = rec.file_offset(pack.data_base) + 4;
            patcher
                .patch_prot_entry(player.entry, stream_off as u64, &recompressed)
                .with_context(|| {
                    format!(
                        "write arm cost for {} weapon id 0x{:02x}",
                        player.name, rec.id
                    )
                })?;
            report.weapons_changed += 1;
        }
    }
    Ok(report)
}

#[cfg(test)]
mod door_plan_tests {
    use super::*;

    fn dest(name: &str, ex: u8) -> Dest {
        Dest {
            index: 0,
            name: name.as_bytes().to_vec(),
            entry_x: ex,
            entry_z: 0,
            dir: 0,
        }
    }

    /// Sorted multiset of (name, entry_x) - the descriptor identity for the
    /// permutation check.
    fn multiset(v: &[Dest]) -> Vec<(Vec<u8>, u8)> {
        let mut m: Vec<_> = v.iter().map(|d| (d.name.clone(), d.entry_x)).collect();
        m.sort();
        m
    }

    #[test]
    fn coupled_preserves_multiset_and_is_deterministic_when_all_paired() {
        // Two clean connections: town<->map and cave<->map.
        //   site 0: home town -> dest map (entry 0x10)   [A]
        //   site 1: home map  -> dest town (entry 0x20)  [B = reverse of A]
        //   site 2: home cave -> dest map (entry 0x30)   [C]
        //   site 3: home map  -> dest cave (entry 0x40)  [D = reverse of C]
        let origs = vec![
            dest("map", 0x10),
            dest("town", 0x20),
            dest("map", 0x30),
            dest("cave", 0x40),
        ];
        let homes = vec![
            "town".to_string(),
            "map".to_string(),
            "cave".to_string(),
            "map".to_string(),
        ];
        let mut rng = SplitMix64::new(0xC0FFEE);
        let (out, unpaired, _pairs, _partner) = plan_doors_coupled(&origs, &homes, &mut rng);
        assert_eq!(unpaired, 0, "all four sites have a reverse partner");
        // Coupling only moves existing descriptors -> multiset preserved.
        assert_eq!(multiset(&out), multiset(&origs));
        // Deterministic for a fixed seed.
        let mut rng2 = SplitMix64::new(0xC0FFEE);
        let (out2, _, _, _) = plan_doors_coupled(&origs, &homes, &mut rng2);
        assert_eq!(out, out2);
        // Scene-level edge multiset stays symmetric: for the new graph, every
        // (home -> dest) edge has a matching (dest -> home) edge.
        let mut edges: Vec<(String, String)> = out
            .iter()
            .zip(&homes)
            .map(|(d, h)| (h.clone(), String::from_utf8_lossy(&d.name).into_owned()))
            .collect();
        edges.sort();
        for (a, b) in &edges {
            assert!(
                edges.iter().any(|(x, y)| x == b && y == a),
                "edge {a}->{b} has no reverse"
            );
        }
    }

    #[test]
    fn coupled_counts_a_dead_end_site_as_unpaired() {
        // A one-way story warp: site 0 home A -> dest B, but no B -> A door.
        // Plus a clean pair (1<->2) so something is matchable.
        let origs = vec![dest("b", 1), dest("y", 2), dest("x", 3)];
        let homes = vec!["a".to_string(), "x".to_string(), "y".to_string()];
        let mut rng = SplitMix64::new(7);
        let (_out, unpaired, _pairs, _partner) = plan_doors_coupled(&origs, &homes, &mut rng);
        // site 0 (a->b) has no reverse; sites 1 (x->y) and 2 (y->x) pair.
        assert_eq!(unpaired, 1);
    }
}
