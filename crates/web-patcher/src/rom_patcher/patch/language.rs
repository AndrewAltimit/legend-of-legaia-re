//! The two language-pack phases: dialog text first, name tables last.

use super::*;

/// Language pack, phase 1 of 2: the dialog sections (`man:` / `raw:` keys)
/// go FIRST, before any data randomization - a dialog edit is keyed by a
/// byte offset into a scene's decompressed MAN, and the door / starting-bag
/// passes relocate those records. The SCUS name sections go LAST (after
/// every randomizer pass), because passes that classify items by their
/// English names - the equipment-drop gear pool - must still see the
/// retail names; nothing in the randomizer relocates a SCUS string, so
/// translating them at the end is always safe.
pub(super) fn dialog_text(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        lang_pack,
        lang_relayout,
        ..
    } = *run.opts;
    let lang_pack = lang_pack.trim();
    run.parsed_pack = if lang_pack.is_empty() {
        None
    } else {
        Some(LanguagePack::from_yaml(lang_pack).map_err(|e| format!("language pack: {e}"))?)
    };
    if let Some(pack) = &run.parsed_pack {
        let report = import_pack_phase(
            &mut run.patcher,
            pack,
            ImportPhase::DialogOnly,
            lang_relayout,
        )
        .map_err(|e| format!("apply language pack (dialog): {e}"))?;
        run.lang_report.merge(report);
    }
    Ok(())
}

/// Language pack, phase 2 of 2: the SCUS name-table sections (see the
/// phase-1 comment above for why they come after every randomizer pass).
pub(super) fn name_tables(run: &mut PatchRun<'_>) -> StepResult {
    let mut lang_line = String::from("language: untouched (English)\n");
    if let Some(pack) = &run.parsed_pack {
        let report = import_pack_phase(&mut run.patcher, pack, ImportPhase::NamesOnly, false)
            .map_err(|e| format!("apply language pack (names): {e}"))?;
        run.lang_report.merge(report);
        let sections = run.lang_report.section_counts(pack);
        lang_line = format!(
            "language ({}): {} strings translated{}\n{}",
            pack.language,
            run.lang_report.applied + run.lang_report.already_applied,
            if run.lang_report.issues.is_empty() {
                String::new()
            } else {
                format!(
                    " ({} line(s) skipped - over budget, non-encodable or not on this disc)",
                    run.lang_report.issues.len()
                )
            },
            relayout_line(&run.lang_report),
        );
        run.lang_sections = Some(sections);
    }
    run.summary.insert_str(0, &lang_line);
    Ok(())
}
