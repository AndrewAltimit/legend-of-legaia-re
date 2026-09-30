//! Language packs: validate, lift, export and strip, plus the import-report JSON.
//! Split out of `rom_patcher.rs`.

use super::*;

/// Short human label for a skip diagnostic, for the per-reason breakdown the
/// page shows ("over budget", "does not recompress", ...).
pub(crate) fn issue_reason(msg: &str) -> &'static str {
    if msg.contains("recompresses") {
        "scene dialog does not recompress into its footprint"
    } else if msg.contains("no free run") {
        "name longer than the name tables have room for"
    } else if msg.contains("a monster name holds at most") {
        "monster name longer than 15 bytes"
    } else if msg.contains("budget") {
        "over budget"
    } else if msg.contains("not encodable") || msg.contains("doesn't encode") {
        "not encodable in the retail glyph set"
    } else if msg.contains("not built for this image")
        || msg.contains("don't match the pack source")
    {
        "not on this disc (wrong image or conflicting patch)"
    } else {
        "other (see console)"
    }
}

/// `{ language, applied, already_applied, skipped, untranslated, sections:
/// [{name, total, filled, applied, already_applied, skipped}], reasons:
/// [{reason, count}] }` - the per-section coverage report the page renders
/// after a language patch.
pub(crate) fn lang_report_json(
    language: &str,
    report: &ImportReport,
    sections: &[legaia_patcher::translation::SectionCounts],
) -> Result<JsValue, JsValue> {
    let out = Object::new();
    Reflect::set(&out, &"language".into(), &language.into())?;
    let num = |v: usize| JsValue::from_f64(v as f64);
    Reflect::set(&out, &"applied".into(), &num(report.applied))?;
    Reflect::set(
        &out,
        &"already_applied".into(),
        &num(report.already_applied),
    )?;
    Reflect::set(&out, &"skipped".into(), &num(report.issues.len()))?;
    Reflect::set(&out, &"untranslated".into(), &num(report.untranslated))?;
    let arr = js_sys::Array::new();
    for s in sections {
        let row = Object::new();
        Reflect::set(&row, &"name".into(), &s.name.into())?;
        Reflect::set(&row, &"total".into(), &num(s.total))?;
        Reflect::set(&row, &"filled".into(), &num(s.filled))?;
        Reflect::set(&row, &"applied".into(), &num(s.applied))?;
        Reflect::set(&row, &"already_applied".into(), &num(s.already_applied))?;
        Reflect::set(&row, &"skipped".into(), &num(s.skipped))?;
        arr.push(&row);
    }
    Reflect::set(&out, &"sections".into(), &arr)?;
    let mut reasons: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
    for (_, msg) in &report.issues {
        *reasons.entry(issue_reason(msg)).or_default() += 1;
    }
    let rarr = js_sys::Array::new();
    for (reason, count) in reasons {
        let row = Object::new();
        Reflect::set(&row, &"reason".into(), &reason.into())?;
        Reflect::set(&row, &"count".into(), &num(count))?;
        rarr.push(&row);
    }
    Reflect::set(&out, &"reasons".into(), &rarr)?;
    Reflect::set(
        &out,
        &"relayout_entries".into(),
        &num(report.relayout_entries),
    )?;
    Reflect::set(
        &out,
        &"relayout_sectors".into(),
        &num(report.relayout_sectors_added as usize),
    )?;
    Reflect::set(
        &out,
        &"relocated_names".into(),
        &num(report.relocated_names),
    )?;
    Reflect::set(
        &out,
        &"grown_monster_names".into(),
        &num(report.grown_monster_names),
    )?;
    Reflect::set(
        &out,
        &"relocated_strings".into(),
        &num(report.relocated_strings),
    )?;
    // Every skipped line, by key, so a translator can find and shorten it in
    // their own copy of the pack (the page offers these as a CSV download).
    let iarr = js_sys::Array::new();
    for (key, msg) in &report.issues {
        let row = Object::new();
        Reflect::set(&row, &"key".into(), &key.as_str().into())?;
        Reflect::set(&row, &"reason".into(), &issue_reason(msg).into())?;
        Reflect::set(&row, &"message".into(), &msg.as_str().into())?;
        iarr.push(&row);
    }
    Reflect::set(&out, &"issues".into(), &iarr)?;
    Ok(out.into())
}

/// The summary lines a relayout and a name move add (empty when neither
/// happened).
pub(super) fn relayout_line(report: &ImportReport) -> String {
    let mut out = String::new();
    if report.relayout_entries > 0 {
        out.push_str(&format!(
            "  disc relayout: {} scene(s) grew by {} sector(s) total (image +{} bytes)\n",
            report.relayout_entries,
            report.relayout_sectors_added,
            report.relayout_sectors_added as u64 * 2352
        ));
    }
    if report.relocated_names + report.grown_monster_names > 0 {
        out.push_str(&format!(
            "  longer names: {} name(s) moved to free table space, {} monster record(s) grown\n",
            report.relocated_names, report.grown_monster_names
        ));
    }
    if report.relocated_strings > 0 {
        out.push_str(&format!(
            "  longer labels: {} menu / system string(s) moved, every reference rewritten\n",
            report.relocated_strings
        ));
    }
    out
}

/// Validate a `legaia-text-pack-v1` YAML document **against the user's own
/// disc**, client-side. Returns `{ ok, language, applied, skipped, message }`:
/// `applied` is how many entries would be written, `skipped` how many the disc
/// rejected (over budget or not matching this image), and `message` a short
/// human summary. This is the same dry run the CLI's `translate stats --input`
/// does - the only way to check a distributable pack's budgets, which are
/// hints until a disc is there to measure. Nothing is written.
///
/// `relayout` dry-runs the whole-sector disc relayout the same way
/// [`patch_rom`]'s `lang_relayout` applies it, so the counts match what a
/// relayout patch would land; the report then carries `relayout_entries` /
/// `relayout_sectors`.
#[wasm_bindgen]
pub fn validate_lang_pack(
    image: Vec<u8>,
    pack_yaml: &str,
    relayout: Option<bool>,
) -> Result<JsValue, JsValue> {
    let pack = LanguagePack::from_yaml(pack_yaml).map_err(|e| err(format!("parse pack: {e}")))?;
    let mut patcher = DiscPatcher::open(image).map_err(|e| err(format!("parse disc: {e}")))?;
    let report = if relayout.unwrap_or(false) {
        import_pack_relayout(&mut patcher, &pack)
    } else {
        import_pack(&mut patcher, &pack)
    }
    .map_err(|e| err(format!("dry run: {e}")))?;
    let out = Object::new();
    Reflect::set(&out, &"ok".into(), &JsValue::from_bool(true))?;
    Reflect::set(&out, &"language".into(), &pack.language.as_str().into())?;
    Reflect::set(
        &out,
        &"applied".into(),
        &JsValue::from_f64(report.applied as f64),
    )?;
    Reflect::set(
        &out,
        &"skipped".into(),
        &JsValue::from_f64(report.issues.len() as f64),
    )?;
    let mut msg = format!(
        "{} strings would be translated, {} skipped (over budget or not on this disc)",
        report.applied,
        report.issues.len()
    );
    if report.relayout_entries > 0 {
        msg.push_str(&format!(
            "; the relayout grows {} scene(s) by {} sector(s)",
            report.relayout_entries, report.relayout_sectors_added
        ));
    }
    Reflect::set(&out, &"message".into(), &msg.into())?;
    let sections = report.section_counts(&pack);
    Reflect::set(
        &out,
        &"report".into(),
        &lang_report_json(&pack.language, &report, &sections)?,
    )?;
    Ok(out.into())
}

/// Lift the text of **another Latin-script disc** the user also owns - an
/// official PAL localization (FR / DE / IT measured; ES / EU-English located at
/// run time) or a fan-patched disc of any Latin build, a patched USA disc
/// included - re-keyed onto their USA disc's coordinate space.
///
/// Same user-supplied-asset model as the base disc: `source_image` is the
/// user's own second `.bin`, it is read in this tab, and neither image is
/// uploaded anywhere. `language`, when given, restamps the pack (a fan patch's
/// language is not in the exe name; the build's own code is the default). The
/// result is a **working** pack (`source:` = USA text, `translation:` = the
/// other disc's text) that
/// the page feeds straight back into [`patch_rom`]'s `lang_pack` argument, so
/// the official text goes through the exact same two-phase import - and the
/// same per-section coverage report - as any community pack. Both discs are
/// consumed and dropped when this returns, so the caller can re-supply the USA
/// image for the patch run without holding two copies at once.
///
/// The pack is filled with the game's copyrighted text: it belongs in the
/// user's browser (or their own scratchpad), never in the repo.
///
/// `fold_accents` rewrites the accented glyph cells the NTSC font does not
/// draw onto plain ASCII - `Epee` for `Épée`. With it off the raw PAL accent
/// bytes are kept and the pack is stamped `accents: font`, so the import
/// writes the accent font and they draw; either way the count is reported,
/// never silent.
///
/// Returns `{ yaml, language, exe, build, summary, tables: [{name, located,
/// pal_base, valid_pct, paired}], names_filled, names_unmapped, party_filled,
/// party_total, man_total, man_paired, raw_total, raw_paired, folded,
/// unfolded }`.
#[wasm_bindgen]
pub fn lift_official_pack(
    target_image: Vec<u8>,
    source_image: Vec<u8>,
    fold_accents: bool,
    language: Option<String>,
) -> Result<JsValue, JsValue> {
    let target =
        DiscPatcher::open(target_image).map_err(|e| err(format!("parse USA disc: {e}")))?;
    let source =
        DiscPatcher::open(source_image).map_err(|e| err(format!("parse source disc: {e}")))?;
    let (mut pack, mut rep) =
        lift::lift_official(&target, &source).map_err(|e| err(format!("lift: {e}")))?;
    if let Some(lang) = language
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
    {
        pack.language = lang.clone();
        rep.language = lang;
    }
    // Free the source disc as early as possible - two full images plus the pack
    // is the peak allocation of the whole page.
    drop(source);
    drop(target);

    let fold = if fold_accents {
        lift::fold_pack_accents(&mut pack)
    } else {
        // The PAL accent bytes sit on the accent font's layout: stamp the
        // pack so the import writes that font and they draw.
        pack.accents = legaia_patcher::translation::accents::AccentMode::Font
            .header()
            .to_string();
        Default::default()
    };
    let yaml = pack.to_yaml().map_err(|e| err(format!("emit YAML: {e}")))?;

    let num = |v: usize| JsValue::from_f64(v as f64);
    let out = Object::new();
    Reflect::set(&out, &"yaml".into(), &yaml.as_str().into())?;
    Reflect::set(&out, &"language".into(), &rep.language.as_str().into())?;
    Reflect::set(&out, &"exe".into(), &rep.exe_name.as_str().into())?;
    Reflect::set(&out, &"build".into(), &rep.build_label.as_str().into())?;
    let tables = js_sys::Array::new();
    for t in &rep.tables {
        let row = Object::new();
        Reflect::set(&row, &"name".into(), &t.name.into())?;
        Reflect::set(&row, &"located".into(), &JsValue::from_bool(t.located))?;
        Reflect::set(
            &row,
            &"pal_base".into(),
            &format!("0x{:08x}", t.pal_base).into(),
        )?;
        Reflect::set(
            &row,
            &"valid_pct".into(),
            &JsValue::from_f64(t.valid_fraction * 100.0),
        )?;
        Reflect::set(&row, &"paired".into(), &num(t.paired))?;
        tables.push(&row);
    }
    Reflect::set(&out, &"tables".into(), &tables)?;
    Reflect::set(&out, &"names_filled".into(), &num(rep.names_filled))?;
    Reflect::set(&out, &"names_unmapped".into(), &num(rep.names_unmapped))?;
    Reflect::set(&out, &"party_filled".into(), &num(rep.party_filled))?;
    Reflect::set(&out, &"party_total".into(), &num(rep.party_total))?;
    Reflect::set(&out, &"man_total".into(), &num(rep.man_total))?;
    Reflect::set(&out, &"man_paired".into(), &num(rep.man_paired))?;
    Reflect::set(&out, &"raw_total".into(), &num(rep.raw_total))?;
    Reflect::set(&out, &"raw_paired".into(), &num(rep.raw_paired))?;
    Reflect::set(&out, &"folded".into(), &num(fold.folded))?;
    Reflect::set(&out, &"unfolded".into(), &num(fold.unmapped))?;

    // A short text block for the status panel. Counts only - no game text.
    let mut summary = format!(
        "lifted {} text from {} ({})\n",
        rep.language, rep.exe_name, rep.build_label
    );
    for t in &rep.tables {
        summary.push_str(&if t.located {
            format!(
                "  {}: located @ 0x{:08x} ({:.0}% valid), {} names paired\n",
                t.name,
                t.pal_base,
                t.valid_fraction * 100.0,
                t.paired
            )
        } else {
            format!("  {}: NOT located - left English\n", t.name)
        });
    }
    summary.push_str(&format!(
        "  party names: {}/{} paired\n  scene dialog: {}/{} lines paired\n  \
         event-script text: {}/{} lines paired\n",
        rep.party_filled,
        rep.party_total,
        rep.man_paired,
        rep.man_total,
        rep.raw_paired,
        rep.raw_total
    ));
    summary.push_str(&if fold_accents {
        format!(
            "  accents: {} folded to ASCII ({} non-accent symbol cell(s) left as-is)\n",
            fold.folded, fold.unmapped
        )
    } else {
        "  accents: kept as PAL bytes; the pack asks for the accent font (accents: font), \
         so the patch draws them\n"
            .to_string()
    });
    summary.push_str(
        "  menu / system UI strings: not lifted - the overlay string pools sit at \
         region-specific addresses, so those labels stay English\n",
    );
    summary.push_str(
        "Lifting only re-keys the text; how much of it fits the USA disc's \
         sector-aligned scenes is the coverage report after patching.\n",
    );
    Reflect::set(&out, &"summary".into(), &summary.as_str().into())?;
    Ok(out.into())
}

/// Export a **working** language pack (source-bearing, all `translation:`
/// fields empty) from the user's own disc, as YAML text they can download and
/// fill in. This is the authoring on-ramp - the community can produce their own
/// packs without any tooling beyond the browser. The exported text is the
/// user's own disc data and never leaves the browser.
///
/// `language` stamps the pack header (`fr`, `de`, ...); pass `en` for a plain
/// source dump. `resume`, when given, is an existing pack (a shipped
/// distributable one, or the translator's own) whose filled translations are
/// copied onto the matching keys - the CLI's `translate init --resume`, so a
/// translator can keep working on a published pack with the English next to
/// each line. Returns the YAML string.
#[wasm_bindgen]
pub fn export_lang_pack(
    image: Vec<u8>,
    language: &str,
    resume: Option<String>,
) -> Result<String, JsValue> {
    let patcher = DiscPatcher::open(image).map_err(|e| err(format!("parse disc: {e}")))?;
    let pack = export_pack(&patcher).map_err(|e| err(format!("export: {e}")))?;
    let mut pack = if language.is_empty() || language == "en" {
        pack
    } else {
        pack.into_skeleton(language, Vec::new())
    };
    if let Some(prev) = resume.as_deref().map(str::trim).filter(|y| !y.is_empty()) {
        let seed =
            LanguagePack::from_yaml(prev).map_err(|e| err(format!("parse resume pack: {e}")))?;
        pack.merge_translations(&seed);
    }
    pack.to_yaml().map_err(|e| err(format!("emit YAML: {e}")))
}

/// Turn a filled working pack into the **distributable** shape (the CLI's
/// `translate strip`): only the filled entries, keyed by disc coordinate, with
/// every `source:` / `context:` field removed, so none of the game's own text
/// survives. This is the file a translator shares. Needs no disc. Returns
/// `{ yaml, kept, total }`.
#[wasm_bindgen]
pub fn strip_lang_pack(pack_yaml: &str) -> Result<JsValue, JsValue> {
    let pack = LanguagePack::from_yaml(pack_yaml).map_err(|e| err(format!("parse pack: {e}")))?;
    let total = pack.sections.total();
    let dist = pack.strip_sources();
    let kept = dist.sections.total();
    let yaml = dist.to_yaml().map_err(|e| err(format!("emit YAML: {e}")))?;
    let out = Object::new();
    Reflect::set(&out, &"yaml".into(), &yaml.into())?;
    Reflect::set(&out, &"kept".into(), &JsValue::from_f64(kept as f64))?;
    Reflect::set(&out, &"total".into(), &JsValue::from_f64(total as f64))?;
    Ok(out.into())
}
