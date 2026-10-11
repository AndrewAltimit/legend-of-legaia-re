//! Equipment stages: weapon specialty, manual swing-cost / owner edits, stat bonuses and equip masks.

use anyhow::{Result, bail};

use legaia_patcher::apply;

use super::Run;
use crate::cli::mode_str;

/// Favored weapon class per character (`--weapon-specialty`).
pub(super) fn weapon_specialty(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let seed = run.seed;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if args.weapon_specialty {
        let report = apply::randomize_weapon_specialty(patcher, seed)?;
        let map = report
            .assignments
            .iter()
            .map(|a| format!("{}->{}", a.character, a.to))
            .collect::<Vec<_>>()
            .join(", ");
        let skip_note = if report.weapons_skipped_fit > 0 {
            format!(", {} skipped (slot too tight)", report.weapons_skipped_fit)
        } else {
            String::new()
        };
        println!(
            "weapon specialty: reassigned ({map}); {} weapon(s) rewritten{skip_note}",
            report.weapons_changed
        );
        manifest.push("weapon_specialty = true".to_string());
        for a in &report.assignments {
            manifest.push(format!("weapon_specialty_{} = {}", a.character, a.to));
        }
        manifest.push(format!(
            "weapon_specialty_weapons_changed = {}",
            report.weapons_changed
        ));
        if report.weapons_skipped_fit > 0 {
            manifest.push(format!(
                "weapon_specialty_skipped_fit = {}",
                report.weapons_skipped_fit
            ));
        }
    } else {
        println!("weapon specialty: untouched");
        manifest.push("weapon_specialty = false".to_string());
    }
    Ok(())
}

/// Manual equipment edits (swing costs / equip owners) run after the
/// specialty shuffle so a named cost always wins over the permutation.
pub(super) fn equipment_edits(run: &mut Run<'_>) -> Result<u32> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    let mut equipment_edits =
        apply::parse_edit_lists(&args.swing_cost.join(","), &args.equip_owner.join(","))?;
    equipment_edits.skip_model_transplant = args.no_model_transplant;
    equipment_edits.allow_relayout = args.allow_relayout;
    if args.allow_relayout && args.output.is_none() && !args.dry_run {
        bail!(
            "--allow-relayout can grow the image; write --output <patched.bin> (a PPF cannot carry a relayout)"
        );
    }
    let mut relayout_sectors = 0u32;
    if !equipment_edits.is_empty() {
        let rep = apply::apply_equipment_edits(patcher, &equipment_edits)?;
        relayout_sectors = rep.relayout_sectors;
        println!(
            "equipment: {} swing cost(s) rewritten, {} owner row(s) changed",
            rep.costs_changed, rep.owners_changed
        );
        for (c, what) in &rep.costs_no_section {
            println!("  note: {c} has no section or record for {what} - no cost to set");
        }
        for (c, what) in &rep.costs_skipped_fit {
            println!("  skipped: {c} {what} does not recompress into its slot");
        }
        for id in &rep.owners_not_equipment {
            println!("  note: item 0x{id:02X} is not equipment - owner not changed");
        }
        for (id, sib) in &rep.owners_shared_rows {
            let s: Vec<String> = sib.iter().map(|i| format!("0x{i:02X}")).collect();
            println!(
                "  note: item 0x{id:02X} shares its row with {} - they moved too",
                s.join(", ")
            );
        }
        for n in apply::fall_through_notes(&rep.owners_without_section) {
            println!("  note: {n}");
        }
        for n in apply::transplant_notes(&rep) {
            println!("  note: {n}");
        }
        for t in &rep.models_transplanted {
            manifest.push(format!(
                "weapon_model_{}_0x{:02X} = \"{}\"",
                t.character, t.item, t.source
            ));
        }
        for e in &equipment_edits.costs {
            manifest.push(format!(
                "swing_cost_{}_{} = {}",
                legaia_patcher::weapon_specialty::PLAYERS[e.character].name,
                e.label().replace(':', "_"),
                e.cost
            ));
        }
        for e in &equipment_edits.owners {
            manifest.push(format!(
                "equip_owner_0x{:02X} = \"{}\"",
                e.item_id,
                apply::owner_label(e.mask)
            ));
        }
    }
    Ok(relayout_sectors)
}

/// Equipment passive stat bonuses (`--equip-bonus`).
pub(super) fn equip_bonus(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let seed = run.seed;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    let equip_bonus_mode = args.equip_bonus.mode();
    if let Some(equip_bonus_mode) = equip_bonus_mode {
        let changed = apply::randomize_equip_bonuses(patcher, seed, equip_bonus_mode)?;
        println!("equip bonuses: {changed} bonus row(s) changed ({equip_bonus_mode:?})");
        manifest.push(format!("equip_bonus = {:?}", mode_str(equip_bonus_mode)));
        manifest.push(format!("equip_bonus_changed = {changed}"));
    } else {
        println!("equip bonuses: untouched");
        manifest.push("equip_bonus = \"none\"".to_string());
    }
    Ok(())
}

/// Equip-character masks (`--equip-mask`).
pub(super) fn equip_mask(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let seed = run.seed;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    let equip_mask_mode = args.equip_mask.mode();
    if let Some(equip_mask_mode) = equip_mask_mode {
        let changed = apply::randomize_equip_masks(patcher, seed, equip_mask_mode)?;
        println!("equip masks: {changed} bonus row(s) changed ({equip_mask_mode:?})");
        manifest.push(format!("equip_mask = {:?}", mode_str(equip_mask_mode)));
        manifest.push(format!("equip_mask_changed = {changed}"));
    } else {
        println!("equip masks: untouched");
        manifest.push("equip_mask = \"none\"".to_string());
    }
    Ok(())
}
