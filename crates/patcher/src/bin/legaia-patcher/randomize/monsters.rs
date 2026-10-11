//! Monster-record stages: drops, stats, the difficulty / EXP / catch-rate / attack-count scales, move power, element affinity, spell costs and steals.

use anyhow::Result;

use legaia_patcher::apply;

use super::Run;
use crate::cli::mode_str;

/// Normal drop table first: reassign the monsters that already drop something.
pub(super) fn drops(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let seed = run.seed;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    let pool = &run.pool;
    let mode = args.drops.mode();
    if let Some(mode) = mode {
        let (plan, report) = apply::randomize_drops(patcher, pool, seed, mode)?;
        println!(
            "drops: {} of {} monsters reassigned ({:?})",
            report.changed,
            plan.len(),
            mode
        );
        manifest.push(format!("drops = {:?}", mode_str(mode)));
        manifest.push(format!(
            "drops_changed = {}  # of {} dropping monsters",
            report.changed,
            plan.len()
        ));
        if !report.skipped.is_empty() {
            println!(
                "  note: {} slot(s) too full to re-pack, left unchanged: {:?}",
                report.skipped.len(),
                report.skipped
            );
            manifest.push(format!("drops_skipped = {:?}", report.skipped));
        }
    } else {
        println!("drops: untouched");
        manifest.push("drops = \"none\"".to_string());
    }
    Ok(())
}

/// Monster combat stats (`--monster-stats`).
pub(super) fn monster_stats(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let seed = run.seed;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    let monster_stats_mode = args.monster_stats.mode();
    if let Some(monster_stats_mode) = monster_stats_mode {
        let report = apply::randomize_monster_stats(patcher, seed, monster_stats_mode)?;
        println!(
            "monster stats: {} monsters changed, {} fields ({:?})",
            report.monsters_changed, report.fields_changed, monster_stats_mode
        );
        manifest.push(format!(
            "monster_stats = {:?}",
            mode_str(monster_stats_mode)
        ));
        manifest.push(format!(
            "monster_stats_monsters_changed = {}",
            report.monsters_changed
        ));
        manifest.push(format!(
            "monster_stats_fields_changed = {}",
            report.fields_changed
        ));
        if !report.skipped.is_empty() {
            println!(
                "  note: {} monster slot(s) too tight to re-pack, left unchanged: {:?}",
                report.skipped.len(),
                report.skipped
            );
            manifest.push(format!("monster_stats_skipped = {:?}", report.skipped));
        }
    } else {
        println!("monster stats: untouched");
        manifest.push("monster_stats = \"none\"".to_string());
    }
    Ok(())
}

/// Enemy difficulty scale: a multiplier over every monster's combat stats,
/// one scale for random encounters and one for bosses (equal, for a
/// whole-roster dial). Seedless, and sequenced *after* the stat randomizer so
/// it scales whatever that pass dealt out (both read the roster off the disc).
pub(super) fn enemy_stat_scale(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if let Some(scale) = args.enemy_stat_scale {
        let report = apply::scale_monster_stats_profile(patcher, scale)?;
        println!(
            "enemy stat scale: {scale} ({} monsters changed, {} fields)",
            report.monsters_changed, report.fields_changed
        );
        if !scale.is_uniform() {
            println!(
                "  split: {} of those are bosses (scripted-only fights)",
                report.bosses_changed
            );
            manifest.push(format!(
                "enemy_stat_scale_bosses_changed = {}",
                report.bosses_changed
            ));
        }
        if !report.skipped.is_empty() {
            println!(
                "  note: {} monster slot(s) too tight to re-pack, left unchanged: {:?}",
                report.skipped.len(),
                report.skipped
            );
            manifest.push(format!("enemy_stat_scale_skipped = {:?}", report.skipped));
        }
        manifest.push(format!("enemy_stat_scale = {:?}", scale.to_string()));
        manifest.push(format!(
            "enemy_stat_scale_monsters_changed = {}",
            report.monsters_changed
        ));
    } else {
        println!("enemy stat scale: 1x (retail)");
        manifest.push("enemy_stat_scale = \"retail\"".to_string());
    }
    Ok(())
}

/// EXP multiplier: scales every monster's base-EXP reward halfword.
/// Seedless, like the difficulty scale.
pub(super) fn exp_scale(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if let Some(scale) = args.exp_scale {
        let report = apply::scale_monster_exp(patcher, scale)?;
        println!(
            "exp scale: {scale} ({} monsters changed)",
            report.monsters_changed
        );
        if !report.skipped.is_empty() {
            println!(
                "  note: {} monster slot(s) too tight to re-pack, left unchanged: {:?}",
                report.skipped.len(),
                report.skipped
            );
            manifest.push(format!("exp_scale_skipped = {:?}", report.skipped));
        }
        manifest.push(format!("exp_scale = {:?}", scale.to_string()));
        manifest.push(format!(
            "exp_scale_monsters_changed = {}",
            report.monsters_changed
        ));
    } else {
        println!("exp scale: 1x (retail)");
        manifest.push("exp_scale = \"retail\"".to_string());
    }
    Ok(())
}

/// Seru catch-rate override: one flat percent into every capturable
/// record's catch-chance byte.
pub(super) fn seru_catch_rate(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if let Some(pct) = args.seru_catch_rate {
        let report = apply::set_seru_catch_rate(patcher, pct)?;
        println!(
            "seru catch rate: {pct}% ({} monsters changed)",
            report.monsters_changed
        );
        if !report.skipped.is_empty() {
            println!(
                "  note: {} monster slot(s) too tight to re-pack, left unchanged: {:?}",
                report.skipped.len(),
                report.skipped
            );
            manifest.push(format!("seru_catch_rate_skipped = {:?}", report.skipped));
        }
        manifest.push(format!("seru_catch_rate = {pct}"));
        manifest.push(format!(
            "seru_catch_rate_monsters_changed = {}",
            report.monsters_changed
        ));
    } else {
        println!("seru catch rate: retail");
        manifest.push("seru_catch_rate = \"retail\"".to_string());
    }
    Ok(())
}

/// Enemy attack-count multiplier: divides each attack entry's AGL-cost
/// byte so the per-round AGL budget affords more (or fewer) strikes.
/// Seedless, like the difficulty scale.
pub(super) fn enemy_attack_count(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if let Some(scale) = args.enemy_attack_count {
        let report = apply::scale_enemy_attack_count(patcher, scale)?;
        println!(
            "enemy attack count: {scale} ({} monsters changed, {} attack entries)",
            report.monsters_changed, report.entries_changed
        );
        if !report.skipped.is_empty() {
            println!(
                "  note: {} monster slot(s) too tight to re-pack, left unchanged: {:?}",
                report.skipped.len(),
                report.skipped
            );
            manifest.push(format!("enemy_attack_count_skipped = {:?}", report.skipped));
        }
        manifest.push(format!("enemy_attack_count = {:?}", scale.to_string()));
        manifest.push(format!(
            "enemy_attack_count_monsters_changed = {}",
            report.monsters_changed
        ));
    } else {
        println!("enemy attack count: 1x (retail)");
        manifest.push("enemy_attack_count = \"retail\"".to_string());
    }
    Ok(())
}

/// Special-attack power (`--move-power`).
pub(super) fn move_power(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let seed = run.seed;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    let move_power_mode = args.move_power.mode();
    if let Some(move_power_mode) = move_power_mode {
        let changed = apply::randomize_move_powers(patcher, seed, move_power_mode)?;
        println!("move power: {changed} special-attack power(s) changed ({move_power_mode:?})");
        manifest.push(format!("move_power = {:?}", mode_str(move_power_mode)));
        manifest.push(format!("move_power_changed = {changed}"));
    } else {
        println!("move power: untouched");
        manifest.push("move_power = \"none\"".to_string());
    }
    Ok(())
}

/// The element-affinity matrix (`--element-affinity`).
pub(super) fn element_affinity(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let seed = run.seed;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    let element_affinity_mode = args.element_affinity.mode();
    if let Some(element_affinity_mode) = element_affinity_mode {
        let changed = apply::randomize_element_affinity(patcher, seed, element_affinity_mode)?;
        println!("element affinity: {changed} matrix cell(s) changed ({element_affinity_mode:?})");
        manifest.push(format!(
            "element_affinity = {:?}",
            mode_str(element_affinity_mode)
        ));
        manifest.push(format!("element_affinity_changed = {changed}"));
    } else {
        println!("element affinity: untouched");
        manifest.push("element_affinity = \"none\"".to_string());
    }
    Ok(())
}

/// Spell MP costs (`--spell-cost`).
pub(super) fn spell_cost(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let seed = run.seed;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    let spell_cost_mode = args.spell_cost.mode();
    if let Some(spell_cost_mode) = spell_cost_mode {
        let changed = apply::randomize_spell_costs(patcher, seed, spell_cost_mode)?;
        println!("spell costs: {changed} spell MP cost(s) changed ({spell_cost_mode:?})");
        manifest.push(format!("spell_cost = {:?}", mode_str(spell_cost_mode)));
        manifest.push(format!("spell_cost_changed = {changed}"));
    } else {
        println!("spell costs: untouched");
        manifest.push("spell_cost = \"none\"".to_string());
    }
    Ok(())
}

/// Per-monster steal items (`--steals`).
pub(super) fn steals(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let seed = run.seed;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    let pool = &run.pool;
    let steal_mode = args.steals.mode();
    if let Some(steal_mode) = steal_mode {
        let (plan, report) = apply::randomize_steals(patcher, pool, seed, steal_mode)?;
        println!(
            "steals: {} of {} stealable monsters reassigned ({:?})",
            report.items_changed,
            plan.len(),
            steal_mode
        );
        manifest.push(format!("steals = {:?}", mode_str(steal_mode)));
        manifest.push(format!(
            "steals_changed = {}  # of {} stealable monsters",
            report.items_changed, report.monsters
        ));
    } else {
        println!("steals: untouched");
        manifest.push("steals = \"none\"".to_string());
    }
    Ok(())
}
