//! Monster records: drops, steals, stats and their scales, move power, element affinity, spell costs.

use super::*;

/// Normal drop table first: reassign the monsters that already drop something.
pub(super) fn drops(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { drops, .. } = *run.opts;
    match parse_mode(drops) {
        Some(m) => {
            let (plan, rep) = apply::randomize_drops(&mut run.patcher, &run.pool, run.seed, m)
                .map_err(|e| format!("drops: {e}"))?;
            run.summary.push_str(&format!(
                "drops: {} of {} reassigned ({})\n",
                rep.changed,
                plan.len(),
                drops
            ));
            if !rep.skipped.is_empty() {
                run.summary.push_str(&format!(
                    "  {} slot(s) too full to re-pack\n",
                    rep.skipped.len()
                ));
            }
        }
        None => run.summary.push_str("drops: untouched\n"),
    }
    Ok(())
}

pub(super) fn monster_stats(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { monster_stats, .. } = *run.opts;
    match parse_mode(monster_stats) {
        Some(m) => {
            let rep = apply::randomize_monster_stats(&mut run.patcher, run.seed, m)
                .map_err(|e| format!("monster-stats: {e}"))?;
            run.summary.push_str(&format!(
                "monster-stats: {} monsters changed, {} fields ({})\n",
                rep.monsters_changed, rep.fields_changed, monster_stats
            ));
        }
        None => run.summary.push_str("monster-stats: untouched\n"),
    }
    Ok(())
}

/// Enemy difficulty scale: a multiplier over every monster's combat stats,
/// with its own value for random encounters and for bosses (empty or `1` =
/// retail). Sequenced after the stat randomizer so it scales whatever that
/// pass dealt out. The per-group split rides inside this same string - the
/// page emits `regular:...|boss:...` when the two halves differ - so widening
/// the knob needed no new argument on this boundary.
pub(super) fn enemy_stat_scale(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        enemy_stat_scale, ..
    } = *run.opts;
    let enemy_stat_scale = enemy_stat_scale.trim();
    if enemy_stat_scale.is_empty() {
        run.summary.push_str("enemy-stat-scale: 1x (retail)\n");
    } else {
        match legaia_patcher::monster_stats::ScaleProfile::parse(enemy_stat_scale) {
            Ok(scale) if scale.is_retail() => {
                run.summary.push_str("enemy-stat-scale: 1x (retail)\n");
            }
            Ok(scale) => match apply::scale_monster_stats_profile(&mut run.patcher, scale) {
                Ok(rep) if scale.is_uniform() => run.summary.push_str(&format!(
                    "enemy-stat-scale: {scale} ({} monsters changed, {} stats)\n",
                    rep.monsters_changed, rep.fields_changed
                )),
                Ok(rep) => run.summary.push_str(&format!(
                    "enemy-stat-scale: {scale} ({} monsters changed incl. {} bosses, {} stats)\n",
                    rep.monsters_changed, rep.bosses_changed, rep.fields_changed
                )),
                Err(e) => run.summary.push_str(&format!("enemy-stat-scale: {e}\n")),
            },
            Err(e) => run
                .summary
                .push_str(&format!("enemy-stat-scale: skipped - {e}\n")),
        }
    }
    Ok(())
}

/// EXP multiplier: scales every monster's base-EXP reward halfword (empty
/// or 1 = retail). Seedless, same shape as the difficulty scale above.
pub(super) fn exp_scale(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { exp_scale, .. } = *run.opts;
    let exp_scale = exp_scale.trim();
    if exp_scale.is_empty() {
        run.summary.push_str("exp-scale: 1x (retail)\n");
    } else {
        match legaia_patcher::monster_stats::ScalePermille::parse(exp_scale) {
            Ok(scale) if scale.is_retail() => {
                run.summary.push_str("exp-scale: 1x (retail)\n");
            }
            Ok(scale) => match apply::scale_monster_exp(&mut run.patcher, scale) {
                Ok(rep) => run.summary.push_str(&format!(
                    "exp-scale: {scale} ({} monsters changed)\n",
                    rep.monsters_changed
                )),
                Err(e) => run.summary.push_str(&format!("exp-scale: {e}\n")),
            },
            Err(e) => run.summary.push_str(&format!("exp-scale: skipped - {e}\n")),
        }
    }
    Ok(())
}

/// Seru catch-rate override: one flat percent into every capturable
/// record's catch-chance byte (empty = retail per-monster rates).
pub(super) fn seru_catch_rate(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        seru_catch_rate, ..
    } = *run.opts;
    let seru_catch_rate = seru_catch_rate.trim();
    if seru_catch_rate.is_empty() {
        run.summary.push_str("seru-catch-rate: retail\n");
    } else {
        match legaia_patcher::rewards::parse_catch_rate(seru_catch_rate) {
            Ok(pct) => match apply::set_seru_catch_rate(&mut run.patcher, pct) {
                Ok(rep) => run.summary.push_str(&format!(
                    "seru-catch-rate: {pct}% ({} monsters changed)\n",
                    rep.monsters_changed
                )),
                Err(e) => run.summary.push_str(&format!("seru-catch-rate: {e}\n")),
            },
            Err(e) => run
                .summary
                .push_str(&format!("seru-catch-rate: skipped - {e}\n")),
        }
    }
    Ok(())
}

/// Enemy attack-count multiplier: divides each attack entry's AGL-cost
/// byte so the per-round AGL budget affords more (or fewer) strikes
/// (empty or 1 = retail). Seedless, same shape as the scales above.
pub(super) fn enemy_attack_count(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        enemy_attack_count, ..
    } = *run.opts;
    let enemy_attack_count = enemy_attack_count.trim();
    if enemy_attack_count.is_empty() {
        run.summary.push_str("enemy-attack-count: 1x (retail)\n");
    } else {
        match legaia_patcher::monster_stats::ScalePermille::parse(enemy_attack_count) {
            Ok(scale) if scale.is_retail() => {
                run.summary.push_str("enemy-attack-count: 1x (retail)\n");
            }
            Ok(scale) => match apply::scale_enemy_attack_count(&mut run.patcher, scale) {
                Ok(rep) => run.summary.push_str(&format!(
                    "enemy-attack-count: {scale} ({} monsters changed, {} attack entries)\n",
                    rep.monsters_changed, rep.entries_changed
                )),
                Err(e) => run.summary.push_str(&format!("enemy-attack-count: {e}\n")),
            },
            Err(e) => run
                .summary
                .push_str(&format!("enemy-attack-count: skipped - {e}\n")),
        }
    }
    Ok(())
}

pub(super) fn move_power(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { move_power, .. } = *run.opts;
    match parse_mode(move_power) {
        Some(m) => {
            let changed = apply::randomize_move_powers(&mut run.patcher, run.seed, m)
                .map_err(|e| format!("move-power: {e}"))?;
            run.summary.push_str(&format!(
                "move-power: {changed} special-attack power(s) changed ({move_power})\n"
            ));
        }
        None => run.summary.push_str("move-power: untouched\n"),
    }
    Ok(())
}

pub(super) fn element_affinity(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        element_affinity, ..
    } = *run.opts;
    match parse_mode(element_affinity) {
        Some(m) => {
            let changed = apply::randomize_element_affinity(&mut run.patcher, run.seed, m)
                .map_err(|e| format!("element-affinity: {e}"))?;
            run.summary.push_str(&format!(
                "element-affinity: {changed} matrix cell(s) changed ({element_affinity})\n"
            ));
        }
        None => run.summary.push_str("element-affinity: untouched\n"),
    }
    Ok(())
}

pub(super) fn spell_cost(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { spell_cost, .. } = *run.opts;
    match parse_mode(spell_cost) {
        Some(m) => {
            let changed = apply::randomize_spell_costs(&mut run.patcher, run.seed, m)
                .map_err(|e| format!("spell-cost: {e}"))?;
            run.summary.push_str(&format!(
                "spell-cost: {changed} spell MP cost(s) changed ({spell_cost})\n"
            ));
        }
        None => run.summary.push_str("spell-cost: untouched\n"),
    }
    Ok(())
}

pub(super) fn steals(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { steals, .. } = *run.opts;
    match parse_mode(steals) {
        Some(m) => {
            let (plan, rep) = apply::randomize_steals(&mut run.patcher, &run.pool, run.seed, m)
                .map_err(|e| format!("steals: {e}"))?;
            run.summary.push_str(&format!(
                "steals: {} of {} stealable monsters reassigned ({})\n",
                rep.items_changed,
                plan.len(),
                steals
            ));
        }
        None => run.summary.push_str("steals: untouched\n"),
    }
    Ok(())
}
