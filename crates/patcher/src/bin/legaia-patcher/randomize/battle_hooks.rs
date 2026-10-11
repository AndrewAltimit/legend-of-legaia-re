//! Battle code-hook stages: bonus drops, flee EXP, enemy ally, HP bars, shiny Seru, the softlock and jewel fixes, and the Spirit / damage AP dials.

use anyhow::Result;

use legaia_patcher::apply;

use super::Run;

/// Equipment-as-drops layers *on top* via a code hook into the battle-end
/// reward routine: a low-chance roll grants one extra random equipment piece
/// in addition to the normal drop above, which is never disturbed.
pub(super) fn equipment_drops(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if args.equipment_drops {
        let report = apply::inject_equipment_bonus_drop(patcher, args.equipment_drop_chance)?;
        println!(
            "equipment-drops: bonus equipment drop injected ({}% chance per battle, \
             {} gear ids in pool)",
            report.chance_pct, report.table_len
        );
        manifest.push("equipment_drops = true".to_string());
        manifest.push(format!("equipment_drop_chance = {}", report.chance_pct));
        manifest.push(format!("equipment_pool = {}", report.table_len));
    } else {
        manifest.push("equipment_drops = false".to_string());
    }
    Ok(())
}

/// Run-away EXP: a code hook in the escape teardown banks a slice of the fled
/// formation's experience into the party (vanilla gives nothing for fleeing).
pub(super) fn flee_exp(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if args.flee_exp {
        let report = apply::inject_flee_exp(patcher, args.flee_exp_pct)?;
        println!(
            "flee-exp: {}% of a fled fight's experience banked into the party",
            report.pct
        );
        manifest.push("flee_exp = true".to_string());
        manifest.push(format!("flee_exp_pct = {}", report.pct));
    } else {
        manifest.push("flee_exp = false".to_string());
    }
    Ok(())
}

/// Enemy ally ("charm"): a code hook in battle setup flags the frontmost enemy
/// with the AI-delegated bits so it fights for the party (works on bosses), and
/// a one-word widen of the victory check keeps it from being an enemy you must
/// defeat.
pub(super) fn enemy_ally(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if args.enemy_ally {
        let report = apply::inject_enemy_ally(patcher, args.enemy_ally_pct)?;
        println!(
            "enemy-ally: {}% chance per battle a random enemy fights on your side",
            report.pct
        );
        manifest.push("enemy_ally = true".to_string());
        manifest.push(format!("enemy_ally_pct = {}", report.pct));
    } else {
        manifest.push("enemy_ally = false".to_string());
    }
    Ok(())
}

/// Enemy HP bars: a per-frame gauge over each living monster, drawn with
/// retail's own AP-plate tiles + gauge primitive from a detour at the head
/// of the damage-popup renderer. Purely cosmetic; claims no arena bytes.
pub(super) fn enemy_hp_bar(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if args.enemy_hp_bar {
        let report = apply::inject_enemy_hp_bar(patcher)?;
        println!(
            "enemy-hp-bar: red HP gauge over every enemy ({} overlay + {} SCUS words, {} edits)",
            report.overlay_words, report.scus_words, report.edits
        );
        manifest.push("enemy_hp_bar = true".to_string());
    } else {
        manifest.push("enemy_hp_bar = false".to_string());
    }
    Ok(())
}

/// Shiny Seru: a code hook in battle setup boosts a rare capturable enemy's
/// stats +35% and marks it; the capture/damage hooks flag the captured Seru
/// (persistent byte at record+0x1C0, kept off the level byte so the Seru still
/// levels up + displays normally) so its spell deals +35% damage forever.
/// Routines live in verified-dead SCUS arenas outside every live table.
/// Shiny-Seru and the Delilas Challenge both need the verified-dead SCUS
/// arena bytes at 0x8007AE00 (shiny's routine arena; the dome course's seed
/// routine + template cave), so they cannot coexist. The Delilas Challenge
/// takes precedence - it is the headline feature the presets carry - and
/// shiny-Seru yields with a note rather than aborting the run.
pub(super) fn shiny_seru(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if args.shiny_seru && args.delilas_challenge {
        println!(
            "shiny-seru: skipped (shares SCUS arena bytes with --delilas-challenge, which wins)"
        );
        manifest.push("shiny_seru = skipped_delilas_conflict".to_string());
    } else if args.shiny_seru {
        let report = apply::inject_shiny_seru(patcher, args.shiny_pct)?;
        println!(
            "shiny-seru: {}% chance per battle a capturable enemy is shiny (+35% stats, \
             +35% damage when captured)",
            report.pct
        );
        manifest.push("shiny_seru = true".to_string());
        manifest.push(format!("shiny_pct = {}", report.pct));
    } else {
        manifest.push("shiny_seru = false".to_string());
    }
    Ok(())
}

/// Jewel fix: retarget the boss cinematic casts' damage jals from the
/// resist-ladder-bypassing wrapper to the guard-respecting one, so elemental
/// jewels / guards / All Guard apply to Xain's Bloody Horns / Terio Punch
/// (+ Bull Charge), Cort's Guilty Cross, and the Delilas
/// trio's Blazing Slash / Megaton Press / Plasma Strike. Seedless - a fixed
/// thirteen-word edit across the six cast modules.
pub(super) fn jewel_fix(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if args.jewel_fix {
        let report = apply::apply_jewel_fix(patcher)?;
        println!(
            "jewel-fix: {} damage calls retargeted to the guard-respecting wrapper \
             (Bloody Horns / Terio Punch / Bull Charge / Guilty Cross / \
             Blazing Slash / Megaton Press / Plasma Strike now respect elemental guards)",
            report.sites_patched
        );
        manifest.push("jewel_fix = true".to_string());
    } else {
        manifest.push("jewel_fix = false".to_string());
    }
    Ok(())
}

/// Attack-approach softlock fix: rewrite the state-0x19 poll's redundant
/// facing recompute into a guard that re-stages a monster's dead approach
/// animation (the summon-then-melee clip death behind the "endless camera
/// orbit"). Seedless - nine words in the battle overlay.
pub(super) fn approach_softlock_fix(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if args.approach_softlock_fix {
        let report = apply::apply_approach_fix(patcher)?;
        println!(
            "approach-softlock-fix: {}",
            if report.changed {
                "battle overlay patched (a dead approach animation is re-staged; the monster resumes walking)"
            } else {
                "already applied (no change)"
            }
        );
        manifest.push("approach_softlock_fix = true".to_string());
    } else {
        manifest.push("approach_softlock_fix = false".to_string());
    }
    Ok(())
}

/// Spirit AP tuning: set the AP the Spirit command charges into the battle
/// gauge (retail 32; 0 = defence-only, 100 = full gauge, negative = Spirit
/// drains the gauge). Seedless - four immediate words in the battle overlay
/// (the accrual + the widget ramp targets that mirror it), plus the signed
/// accrual tail when the value is negative.
pub(super) fn spirit_ap(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if let Some(ap) = args.spirit_ap {
        let report = apply::apply_spirit_ap(patcher, ap)?;
        println!(
            "spirit-ap: {}",
            if report.changed {
                format!("{} -> {ap} AP per Spirit (retail 32)", report.previous)
            } else {
                format!("already {ap} AP per Spirit (no change)")
            }
        );
        manifest.push(format!("spirit_ap = {ap}"));
    } else {
        manifest.push("spirit_ap = \"retail\"".to_string());
    }
    Ok(())
}

/// Enemy-damage AP tuning: AP granted per 100% of max HP lost (retail 100;
/// 0 = damage never feeds the gauge, negative = being hit drains it).
/// Seedless - the damage finisher's scale chain in the battle overlay.
pub(super) fn damage_ap(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if let Some(value) = args.damage_ap {
        let report = apply::apply_damage_ap(patcher, value)?;
        println!(
            "damage-ap: {}",
            if report.changed {
                format!(
                    "{} -> {value} AP per 100% max-HP damage (retail 100)",
                    report.previous
                )
            } else {
                format!("already {value} AP per 100% max-HP damage (no change)")
            }
        );
        manifest.push(format!("damage_ap = {value}"));
    } else {
        manifest.push("damage_ap = \"retail\"".to_string());
    }
    Ok(())
}
