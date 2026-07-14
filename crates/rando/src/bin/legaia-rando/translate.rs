//! `translate` subcommand implementations: export / init / stats / import.

use std::path::Path;

use anyhow::{Context, Result, bail};

use legaia_rando::disc::DiscPatcher;
use legaia_rando::translation::{
    LanguagePack, export_pack, import_pack,
    markup::{self, Target},
};
use legaia_rando::{apply, ppf};

use crate::util::{cue_contents, load_image};

fn read_pack(path: &Path) -> Result<LanguagePack> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("read pack {}", path.display()))?;
    LanguagePack::from_yaml(&text)
}

fn write_pack(pack: &LanguagePack, path: &Path) -> Result<()> {
    std::fs::write(path, pack.to_yaml()?).with_context(|| format!("write pack {}", path.display()))
}

fn print_coverage(pack: &LanguagePack) {
    println!("language: {}", pack.language);
    let mut ttot = 0usize;
    let mut ttr = 0usize;
    for (name, translated, total) in pack.coverage() {
        println!("  {name:<20} {translated:>6} / {total:<6} translated");
        ttot += total;
        ttr += translated;
    }
    println!("  {:<20} {ttr:>6} / {ttot:<6} translated", "TOTAL");
}

pub(crate) fn cmd_export(input: &Path, output: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let pack = export_pack(&patcher)?;
    write_pack(&pack, output)?;
    print_coverage(&pack);
    println!("wrote {}", output.display());
    println!(
        "NB: the pack contains the game's text - keep it out of version control / \
         redistribution."
    );
    Ok(())
}

pub(crate) fn cmd_init(
    lang: &str,
    from: Option<&Path>,
    input: Option<&Path>,
    contributors: Vec<String>,
    output: &Path,
) -> Result<()> {
    let pack = match (from, input) {
        (Some(p), _) => read_pack(p)?,
        (None, Some(disc)) => {
            let image = load_image(disc)?;
            let patcher = DiscPatcher::open(image).context("parse disc image")?;
            export_pack(&patcher)?
        }
        (None, None) => bail!("pass --from <pack.yaml> or --input <disc.bin>"),
    };
    let skeleton = pack.into_skeleton(lang, contributors);
    write_pack(&skeleton, output)?;
    println!(
        "wrote {} ({} entries, language {lang})",
        output.display(),
        skeleton.sections.total()
    );
    Ok(())
}

pub(crate) fn cmd_stats(pack_path: &Path) -> Result<()> {
    let pack = read_pack(pack_path)?;
    print_coverage(&pack);

    // Dry-run validation of every filled entry: encodability + byte budget.
    let mut problems = 0usize;
    for (section, entries) in pack.sections.iter() {
        for e in entries {
            if e.translation.trim().is_empty() {
                continue;
            }
            let target = if e.key.starts_with("scus:") {
                Target::CString
            } else {
                Target::Segment
            };
            match markup::encode(&e.translation, target) {
                Err(issues) => {
                    problems += 1;
                    println!("[{section}] {}:", e.key);
                    for i in issues {
                        println!("    {i}");
                    }
                }
                Ok(bytes) if bytes.len() > e.budget => {
                    problems += 1;
                    println!(
                        "[{section}] {}: {} bytes over budget ({} > {})",
                        e.key,
                        bytes.len() - e.budget,
                        bytes.len(),
                        e.budget
                    );
                }
                Ok(_) => {}
            }
        }
    }
    if problems == 0 {
        println!("all filled entries encode within budget");
    } else {
        println!(
            "{problems} entr{} need fixing",
            if problems == 1 { "y" } else { "ies" }
        );
    }
    Ok(())
}

pub(crate) fn cmd_import(
    input: &Path,
    pack_path: &Path,
    output: Option<&Path>,
    patch: Option<&Path>,
) -> Result<()> {
    if output.is_none() && patch.is_none() {
        bail!("pass --output <patched.bin> and/or --patch <out.ppf>");
    }
    let pack = read_pack(pack_path)?;
    let original = load_image(input)?;
    let mut patcher = DiscPatcher::open(original.clone()).context("parse disc image")?;
    let report = import_pack(&mut patcher, &pack)?;

    println!(
        "applied {} entr{}, {} already applied, {} untranslated (left vanilla)",
        report.applied,
        if report.applied == 1 { "y" } else { "ies" },
        report.already_applied,
        report.untranslated
    );
    for (key, msg) in &report.issues {
        println!("  [skip] {key}: {msg}");
    }

    let patched = patcher.into_image();
    if let Some(ppf_path) = patch {
        let runs = ppf::diff_runs(&original, &patched);
        let desc = format!("Legaia translation pack ({})", pack.language);
        std::fs::write(ppf_path, ppf::write_ppf3(&desc, &runs))
            .with_context(|| format!("write PPF {}", ppf_path.display()))?;
        println!("wrote {} ({} change runs)", ppf_path.display(), runs.len());
    }
    if let Some(out) = output {
        std::fs::write(out, &patched)
            .with_context(|| format!("write patched image {}", out.display()))?;
        let cue = out.with_extension("cue");
        let bin_name = out
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "patched.bin".to_string());
        std::fs::write(&cue, cue_contents(&bin_name))
            .with_context(|| format!("write cue {}", cue.display()))?;
        println!("wrote {} (+ {})", out.display(), cue.display());
        // Same sanity check the randomizer runs: the patched image still parses.
        let check = DiscPatcher::open(patched).context("re-parse patched image")?;
        let _ = apply::current_drops(&check)?;
    }
    if !report.issues.is_empty() {
        println!(
            "{} entr{} skipped - fix and re-run (import is idempotent)",
            report.issues.len(),
            if report.issues.len() == 1 {
                "y was"
            } else {
                "ies were"
            }
        );
    }
    Ok(())
}
