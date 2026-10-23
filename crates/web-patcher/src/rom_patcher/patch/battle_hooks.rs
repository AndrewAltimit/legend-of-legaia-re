//! The battle code hooks and fixes, and the two AP-gauge knobs.

use super::*;

/// Equipment-as-drops layers on top via a code hook into the battle-end
/// reward routine: a low-chance roll grants one extra random equipment piece
/// in addition to the normal drop, which is never disturbed.
pub(super) fn equipment_drops(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        equipment_drops, ..
    } = *run.opts;
    if equipment_drops {
        let rep = apply::inject_equipment_bonus_drop(
            &mut run.patcher,
            legaia_code_hooks::bonus_drop::DEFAULT_CHANCE_PCT,
        )
        .map_err(|e| format!("equipment drops: {e}"))?;
        run.summary.push_str(&format!(
            "equipment-drops: bonus drop injected ({}% per battle, {} gear ids in pool)\n",
            rep.chance_pct, rep.table_len
        ));
    }
    Ok(())
}

/// Run-away EXP: a code hook in the escape teardown banks a slice of a fled
/// fight's experience into the party (vanilla gives nothing for fleeing).
pub(super) fn flee_exp(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { flee_exp, .. } = *run.opts;
    if flee_exp {
        let rep =
            apply::inject_flee_exp(&mut run.patcher, legaia_code_hooks::flee_exp::DEFAULT_PCT)
                .map_err(|e| format!("flee-exp: {e}"))?;
        run.summary.push_str(&format!(
            "flee-exp: {}% of a fled fight's experience banked into the party\n",
            rep.pct
        ));
    } else {
        run.summary.push_str("flee-exp: untouched\n");
    }
    Ok(())
}

/// Enemy ally ("charm"): a code hook in battle setup flags the frontmost enemy
/// so it fights on the player's side (works on bosses); a one-word widen of the
/// victory check keeps the charmed enemy from being one you must defeat.
pub(super) fn enemy_ally(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { enemy_ally, .. } = *run.opts;
    if enemy_ally {
        let rep =
            apply::inject_enemy_ally(&mut run.patcher, legaia_patcher::enemy_ally::DEFAULT_PCT)
                .map_err(|e| format!("enemy-ally: {e}"))?;
        run.summary.push_str(&format!(
            "enemy-ally: {}% chance per battle a random enemy fights on your side\n",
            rep.pct
        ));
    } else {
        run.summary.push_str("enemy-ally: untouched\n");
    }
    Ok(())
}

/// Enemy HP bars: a per-frame gauge over each living monster, drawn with
/// retail's own AP-plate tiles + gauge primitive. Cosmetic; no arena bytes.
pub(super) fn enemy_hp_bar(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { enemy_hp_bar, .. } = *run.opts;
    if enemy_hp_bar {
        let rep = apply::inject_enemy_hp_bar(&mut run.patcher)
            .map_err(|e| format!("enemy-hp-bar: {e}"))?;
        run.summary.push_str(&format!(
            "enemy-hp-bar: red HP gauge over every enemy ({} edits)\n",
            rep.edits
        ));
    } else {
        run.summary.push_str("enemy-hp-bar: untouched\n");
    }
    Ok(())
}

/// Shiny Seru: a code hook boosts a rare capturable enemy's stats +35%; the
/// capture/damage hooks make its captured Seru deal +35% damage forever.
/// It shares the verified-dead SCUS arena bytes (0x8007AE00) with the
/// Delilas Challenge's dome-course cave, so the two cannot coexist; the
/// Delilas Challenge takes precedence and shiny-Seru yields with a note.
pub(super) fn shiny_seru(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        shiny_seru,
        delilas_challenge,
        ..
    } = *run.opts;
    if shiny_seru && delilas_challenge {
        run.summary.push_str(
            "shiny-seru: skipped (shares SCUS arena bytes with the Delilas Challenge, which wins)\n",
        );
    } else if shiny_seru {
        let rep =
            apply::inject_shiny_seru(&mut run.patcher, legaia_code_hooks::shiny_seru::DEFAULT_PCT)
                .map_err(|e| format!("shiny-seru: {e}"))?;
        run.summary.push_str(&format!(
            "shiny-seru: {}% chance per battle a capturable enemy is shiny (+35% stats / damage)\n",
            rep.pct
        ));
    } else {
        run.summary.push_str("shiny-seru: untouched\n");
    }
    Ok(())
}

/// Jewel fix: retarget the boss cinematic casts' damage calls from the
/// resist-ladder-bypassing wrapper to the guard-respecting one, so elemental
/// jewels / guards / All Guard apply to Xain's Bloody Horns / Terio Punch,
/// Cort's Guilty Cross, and the Delilas trio's signature moves. Seedless.
pub(super) fn jewel_fix(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { jewel_fix, .. } = *run.opts;
    if jewel_fix {
        let rep =
            apply::apply_jewel_fix(&mut run.patcher).map_err(|e| format!("jewel-fix: {e}"))?;
        run.summary.push_str(&format!(
            "jewel-fix: {} boss-cast damage calls now respect elemental guards\n",
            rep.sites_patched
        ));
    } else {
        run.summary.push_str("jewel-fix: untouched\n");
    }
    Ok(())
}

/// Attack-approach softlock fix: nine words in the battle overlay so a
/// monster whose approach animation dies mid-approach is re-staged and
/// resumes walking instead of parking the battle in the state-0x19 range
/// poll forever. Seedless.
pub(super) fn approach_softlock_fix(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        approach_softlock_fix,
        ..
    } = *run.opts;
    if approach_softlock_fix {
        let rep = apply::apply_approach_fix(&mut run.patcher)
            .map_err(|e| format!("approach-softlock-fix: {e}"))?;
        run.summary.push_str(if rep.changed {
            "approach-softlock-fix: battle overlay patched (dead approach animations are re-staged)\n"
        } else {
            "approach-softlock-fix: already applied\n"
        });
    } else {
        run.summary.push_str("approach-softlock-fix: untouched\n");
    }
    Ok(())
}

/// Spirit AP: the AP the Spirit command charges into the battle gauge
/// (retail 32; 0 = defence-only, 100 = full gauge, negative = Spirit drains
/// the gauge). A single value (empty = untouched); four immediate words in
/// the battle overlay, plus the signed accrual tail when negative.
pub(super) fn spirit_ap(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { spirit_ap, .. } = *run.opts;
    let spirit_ap = spirit_ap.trim();
    if spirit_ap.is_empty() {
        run.summary.push_str("spirit-ap: untouched\n");
    } else {
        match spirit_ap.parse::<i16>() {
            Ok(ap) if (-100..=100).contains(&ap) => {
                match apply::apply_spirit_ap(&mut run.patcher, ap) {
                    Ok(rep) if !rep.changed => run
                        .summary
                        .push_str(&format!("spirit-ap: already {ap} AP per Spirit\n")),
                    Ok(rep) => run.summary.push_str(&format!(
                        "spirit-ap: {} -> {ap} AP per Spirit (retail 32)\n",
                        rep.previous
                    )),
                    Err(e) => run.summary.push_str(&format!("spirit-ap: {e}\n")),
                }
            }
            _ => run.summary.push_str(&format!(
                "spirit-ap: skipped out-of-range value {spirit_ap:?} (want -100..=100)\n"
            )),
        }
    }
    Ok(())
}

/// Enemy-damage AP: AP granted per 100% of max HP lost (retail 100; 0 =
/// damage never feeds the gauge, negative = being hit drains it). A single
/// value (empty = untouched); the damage finisher's scale chain in the
/// battle overlay, plus its accrual tail when negative.
pub(super) fn damage_ap(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { damage_ap, .. } = *run.opts;
    let damage_ap = damage_ap.trim();
    if damage_ap.is_empty() {
        run.summary.push_str("damage-ap: untouched\n");
    } else {
        match damage_ap.parse::<i16>() {
            Ok(v) if (-200..=200).contains(&v) => match apply::apply_damage_ap(&mut run.patcher, v)
            {
                Ok(rep) if !rep.changed => run.summary.push_str(&format!(
                    "damage-ap: already {v} AP per 100% max-HP damage\n"
                )),
                Ok(rep) => run.summary.push_str(&format!(
                    "damage-ap: {} -> {v} AP per 100% max-HP damage (retail 100)\n",
                    rep.previous
                )),
                Err(e) => run.summary.push_str(&format!("damage-ap: {e}\n")),
            },
            _ => run.summary.push_str(&format!(
                "damage-ap: skipped out-of-range value {damage_ap:?} (want -200..=200)\n"
            )),
        }
    }
    Ok(())
}
