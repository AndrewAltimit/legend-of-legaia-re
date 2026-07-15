//! `translate` subcommand implementations: export / init / stats / import.

use std::path::Path;

use anyhow::{Context, Result, bail};

use legaia_rando::disc::DiscPatcher;
use legaia_rando::translation::{
    LanguagePack, diff, export_pack, fit, import_pack, lift,
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
    resume: Option<&Path>,
    chunk: Option<usize>,
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
    let mut skeleton = pack.into_skeleton(lang, contributors);
    if let Some(prev) = resume {
        let seed = read_pack(prev)?;
        let n = skeleton.merge_translations(&seed);
        println!("resumed {n} translation(s) from {}", prev.display());
    }
    write_pack(&skeleton, output)?;
    println!(
        "wrote {} ({} entries, language {lang})",
        output.display(),
        skeleton.sections.total()
    );
    if let Some(size) = chunk {
        if size == 0 {
            bail!("--chunk must be at least 1");
        }
        let chunks = skeleton.split_chunks(size);
        let stem = output
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "pack".into());
        let dir = output.parent().unwrap_or(Path::new("."));
        for (i, c) in chunks.iter().enumerate() {
            let path = dir.join(format!("{stem}.{:03}.yaml", i + 1));
            write_pack(c, &path)?;
        }
        println!(
            "wrote {} chunk file(s) of <= {size} entries ({stem}.001.yaml ...)",
            chunks.len()
        );
    }
    Ok(())
}

pub(crate) fn cmd_strip(pack_path: &Path, output: &Path, notes: Option<&str>) -> Result<()> {
    let pack = read_pack(pack_path)?;
    let total = pack.sections.total();
    let mut dist = pack.strip_sources();
    if let Some(n) = notes {
        dist.notes = n.to_string();
    }
    let kept = dist.sections.total();
    write_pack(&dist, output)?;
    println!(
        "wrote {} ({kept} translated entr{} kept of {total}; source text stripped)",
        output.display(),
        if kept == 1 { "y" } else { "ies" }
    );
    Ok(())
}

pub(crate) fn cmd_merge(base: &Path, packs: &[std::path::PathBuf], output: &Path) -> Result<()> {
    let mut merged = read_pack(base)?;
    let mut total = 0usize;
    for p in packs {
        let other = read_pack(p)?;
        let n = merged.merge_translations(&other);
        total += n;
        println!("  {}: {n} translation(s)", p.display());
    }
    write_pack(&merged, output)?;
    println!(
        "wrote {} ({total} translation(s) merged; {} / {} filled)",
        output.display(),
        merged.sections.filled(),
        merged.sections.total()
    );
    Ok(())
}

/// Offline validation: encodability + the pack's own budget. For a
/// distributable (source-less) pack the budget is only a hint, so this is a
/// pre-check - `--input` runs the real thing.
fn offline_check(pack: &LanguagePack) -> usize {
    let mut problems = 0usize;
    for (section, entries) in pack.sections.iter() {
        for e in entries {
            if !e.is_filled() {
                continue;
            }
            let target = if e.key.starts_with("scus:") || e.key.starts_with("ui:") {
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
    problems
}

pub(crate) fn cmd_stats(pack_path: &Path, input: Option<&Path>) -> Result<()> {
    let pack = read_pack(pack_path)?;
    print_coverage(&pack);
    let mut problems = offline_check(&pack);

    // With a disc: plan every entry exactly as `import` would, in memory. This
    // measures each target's real byte budget on the disc, which is the only
    // way to validate a distributable pack (its budgets are hints).
    if let Some(disc) = input {
        let image = load_image(disc)?;
        let mut patcher = DiscPatcher::open(image).context("parse disc image")?;
        let report = import_pack(&mut patcher, &pack)?;
        println!(
            "dry run vs {}: {} would apply, {} already applied, {} skipped",
            disc.display(),
            report.applied,
            report.already_applied,
            report.issues.len()
        );
        for (key, msg) in &report.issues {
            println!("  [skip] {key}: {msg}");
        }
        problems += report.issues.len();
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

pub(crate) fn cmd_diff_disc(input: &Path, other: &Path) -> Result<()> {
    let a = DiscPatcher::open(load_image(input)?).context("parse target disc image")?;
    let b = DiscPatcher::open(load_image(other)?).context("parse other disc image")?;
    let rep = diff::diff_disc(&a, &b);

    println!("target: {}", input.display());
    println!("other:  {}", other.display());
    println!(
        "PROT entries: target={} other={} (LBA-aligned by index: {} / {})",
        rep.entries_a,
        rep.entries_b,
        rep.entries_lba_aligned,
        rep.entries_a.min(rep.entries_b),
    );

    let dump = |name: &str, d: &diff::DomainStats| {
        println!("=== {name} ===");
        println!(
            "  entries with segments: target={} other={} both={}",
            d.entries_a, d.entries_b, d.entries_both
        );
        println!(
            "  total qualifying segments: target={} other={}",
            d.total_segs_a, d.total_segs_b
        );
        println!(
            "  order-pairable (sum min per entry): {} = {:.1}% of corpus (needs reconcile: {} lines)",
            d.order_pairable,
            d.order_pairable_pct(),
            d.order_delta,
        );
        println!(
            "  order-paired fit: {} = {:.1}% fit target budget, {} overflow",
            d.order_fit,
            d.order_fit_pct(),
            d.order_overflow,
        );
        if d.order_overflow > 0 {
            println!(
                "    order overflow bytes: total={} avg={:.1} max={}",
                d.order_overflow_bytes_total,
                d.order_overflow_bytes_total as f64 / d.order_overflow as f64,
                d.order_overflow_bytes_max
            );
        }
        println!(
            "  strict count-match (lower bound): {} / {} entries ({:.1}%), {} paired, {:.1}% fit",
            d.count_matched_entries,
            d.entries_both,
            d.count_match_pct(),
            d.paired_segments,
            d.fit_pct(),
        );
    };
    dump("scene MAN dialog", &rep.man);
    dump("raw event-script carriers", &rep.raw);

    println!("=== other-disc high glyph bytes (0x7F..; accented-Latin tiles) ===");
    println!("  distinct high bytes: {}", rep.high_byte_census.len());
    let mut rows: Vec<(u8, u64)> = rep.high_byte_census.iter().map(|(&b, &c)| (b, c)).collect();
    rows.sort_by_key(|y| std::cmp::Reverse(y.1));
    for chunk in rows.chunks(6) {
        let line: Vec<String> = chunk
            .iter()
            .map(|(b, c)| format!("0x{b:02x}={c}"))
            .collect();
        println!("  {}", line.join("  "));
    }
    Ok(())
}

pub(crate) fn cmd_lift_official(from: &Path, target: &Path, output: &Path) -> Result<()> {
    let source = DiscPatcher::open(load_image(from)?).context("parse source (PAL) disc image")?;
    let usa = DiscPatcher::open(load_image(target)?).context("parse target (USA) disc image")?;
    let (pack, rep) = lift::lift_official(&usa, &source)?;
    write_pack(&pack, output)?;

    println!("lifted {} localization from {}", rep.language, rep.exe_name);
    println!("name tables:");
    for t in &rep.tables {
        if t.located {
            println!(
                "  {:<20} located @ 0x{:08x} ({:.0}% valid), {} strings paired",
                t.name,
                t.pal_base,
                t.valid_fraction * 100.0,
                t.paired
            );
        } else {
            println!(
                "  {:<20} NOT located (pinned base failed validation)",
                t.name
            );
        }
    }
    println!(
        "  scus strings: {} filled, {} unmapped; party names: {} / {} filled",
        rep.names_filled, rep.names_unmapped, rep.party_filled, rep.party_total
    );
    let pct = |n: usize, d: usize| {
        if d == 0 {
            100.0
        } else {
            100.0 * n as f64 / d as f64
        }
    };
    println!(
        "dialog (MAN): {} / {} paired ({:.1}%), {} unpaired",
        rep.man_paired,
        rep.man_total,
        pct(rep.man_paired, rep.man_total),
        rep.man_unpaired(),
    );
    println!(
        "dialog (raw): {} / {} paired ({:.1}%), {} unpaired",
        rep.raw_paired,
        rep.raw_total,
        pct(rep.raw_paired, rep.raw_total),
        rep.raw_unpaired(),
    );
    println!("wrote {}", output.display());
    println!(
        "NB: this pack contains the game's text - keep it out of version control / \
         redistribution."
    );
    Ok(())
}

pub(crate) fn cmd_fit_report(from: &Path, target: &Path) -> Result<()> {
    let source = DiscPatcher::open(load_image(from)?).context("parse source (PAL) disc image")?;
    let usa = DiscPatcher::open(load_image(target)?).context("parse target (USA) disc image")?;
    let rep = fit::lift_and_measure(&usa, &source)?;

    let pct = |n: usize, d: usize| {
        if d == 0 {
            0.0
        } else {
            100.0 * n as f64 / d as f64
        }
    };
    println!("=== fit report: {} ===", rep.language);
    println!(
        "pooled names ({} lines): per-string fit {} ({:.1}%)",
        rep.name_lines,
        rep.name_perstring_fit,
        pct(rep.name_perstring_fit, rep.name_lines)
    );
    println!("MAN dialog:");
    println!(
        "  lines {} - per-string fit {} ({:.1}%)",
        rep.man_lines,
        rep.man_perstring_fit,
        pct(rep.man_perstring_fit, rep.man_lines)
    );
    println!(
        "  per-MAN (in-place growth) fit {} lines ({:.1}%), residual {} lines",
        rep.man_lines_perman_fit,
        pct(rep.man_lines_perman_fit, rep.man_lines),
        rep.man_lines_residual
    );
    println!(
        "  MAN entries {}: fit-in-place {}, residual {} (overflow {} + structural {})",
        rep.man_entries,
        rep.man_entries_fit,
        rep.man_entries_residual_overflow + rep.man_entries_residual_structural,
        rep.man_entries_residual_overflow,
        rep.man_entries_residual_structural,
    );
    if !rep.residual_deficits.is_empty() {
        let sum: usize = rep.residual_deficits.iter().sum();
        println!(
            "  residual compressed deficits: max {} B, avg {} B, all within one sector: {}",
            rep.residual_deficit_max(),
            sum / rep.residual_deficits.len(),
            rep.all_residuals_within_one_sector(),
        );
    }
    println!(
        "raw carriers ({} lines, same-size only): per-string fit {} ({:.1}%)",
        rep.raw_lines,
        rep.raw_perstring_fit,
        pct(rep.raw_perstring_fit, rep.raw_lines)
    );
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
    for s in report.section_counts(&pack) {
        if s.filled == 0 {
            continue;
        }
        println!(
            "  {:20} {:5} of {:5} applied{}",
            s.name,
            s.applied + s.already_applied,
            s.filled,
            if s.skipped > 0 {
                format!(" ({} skipped)", s.skipped)
            } else {
                String::new()
            }
        );
    }
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
