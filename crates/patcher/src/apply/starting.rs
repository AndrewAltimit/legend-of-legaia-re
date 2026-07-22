//! New-game seeding (starting items / level / bag) + weapon-specialty randomization.

use super::*;

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
