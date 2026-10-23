//! Equipment: bonus rows, weapon specialty, the swing-cost and owner editor.

use super::*;

pub(super) fn equip_bonus(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { equip_bonus, .. } = *run.opts;
    match parse_mode(equip_bonus) {
        Some(m) => {
            let changed = apply::randomize_equip_bonuses(&mut run.patcher, run.seed, m)
                .map_err(|e| format!("equip-bonus: {e}"))?;
            run.summary.push_str(&format!(
                "equip-bonus: {changed} bonus row(s) changed ({equip_bonus})\n"
            ));
        }
        None => run.summary.push_str("equip-bonus: untouched\n"),
    }
    Ok(())
}

pub(super) fn weapon_specialty(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        weapon_specialty, ..
    } = *run.opts;
    if weapon_specialty {
        let rep = apply::randomize_weapon_specialty(&mut run.patcher, run.seed)
            .map_err(|e| format!("weapon-specialty: {e}"))?;
        let map = rep
            .assignments
            .iter()
            .map(|a| format!("{}->{}", a.character, a.to))
            .collect::<Vec<_>>()
            .join(", ");
        run.summary.push_str(&format!(
            "weapon-specialty: reassigned ({map}); {} weapon(s) rewritten\n",
            rep.weapons_changed
        ));
    } else {
        run.summary.push_str("weapon-specialty: untouched\n");
    }
    Ok(())
}

pub(super) fn equipment_edits(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        swing_costs,
        equip_owners,
        ..
    } = *run.opts;
    let equipment_edits = apply::parse_edit_lists(swing_costs, equip_owners)
        .map_err(|e| format!("equipment: {e}"))?;
    if equipment_edits.is_empty() {
        run.summary.push_str("equipment: untouched\n");
    } else {
        let rep = apply::apply_equipment_edits(&mut run.patcher, &equipment_edits)
            .map_err(|e| format!("equipment: {e}"))?;
        run.summary.push_str(&format!(
            "equipment: {} swing cost(s) rewritten, {} owner row(s) changed\n",
            rep.costs_changed, rep.owners_changed
        ));
        for (c, what) in &rep.costs_no_section {
            run.summary.push_str(&format!(
                "  {c} has no battle section or record for {what}; no cost to set\n"
            ));
        }
        for (c, what) in &rep.costs_skipped_fit {
            run.summary.push_str(&format!(
                "  skipped: {c} {what} does not recompress into its slot\n"
            ));
        }
        for (id, sib) in &rep.owners_shared_rows {
            let s: Vec<String> = sib.iter().map(|i| format!("0x{i:02X}")).collect();
            run.summary.push_str(&format!(
                "  item 0x{id:02X} shares its stat row with {}; their owners moved too\n",
                s.join(", ")
            ));
        }
        for n in apply::fall_through_notes(&rep.owners_without_section) {
            run.summary.push_str(&format!("  {n}\n"));
        }
        for n in apply::transplant_notes(&rep) {
            run.summary.push_str(&format!("  {n}\n"));
        }
    }
    Ok(())
}
