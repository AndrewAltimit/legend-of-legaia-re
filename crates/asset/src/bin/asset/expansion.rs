//! `asset expansion ...`: validate, bundle, unbundle and summarise
//! expansion packs. Disc-free: a pack names disc content by coordinate, so
//! none of these read a disc.

use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use legaia_expansion_format::bundle::{self, read_bundle, write_bundle};
use legaia_expansion_format::manifest::SCHEMA_VERSION;
use legaia_expansion_format::validate::{Issue, Severity, has_errors, validate_files};
use legaia_expansion_format::{
    DOC_FILES, ExpansionPack, PackFiles, SECTION_FILES, read_dir_files, read_files,
};

use crate::cli_args::ExpansionCmd;

pub(super) fn run(cmd: ExpansionCmd) -> Result<()> {
    match cmd {
        ExpansionCmd::Validate { pack, strict } => validate_cmd(&pack, strict),
        ExpansionCmd::Bundle { dir, out } => bundle_cmd(&dir, &out),
        ExpansionCmd::Unbundle { file, out } => unbundle_cmd(&file, &out),
        ExpansionCmd::Info { pack, json } => match pack {
            Some(pack) => info_cmd(&pack, json),
            None => format_info(json),
        },
    }
}

fn print_issues(issues: &[Issue]) {
    for issue in issues {
        eprintln!("{issue}");
    }
}

fn counts(issues: &[Issue]) -> (usize, usize) {
    let errors = issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .count();
    (errors, issues.len() - errors)
}

fn validate_cmd(pack: &Path, strict: bool) -> Result<()> {
    let files = read_files(pack)?;
    let issues = validate_files(&files, None);
    print_issues(&issues);
    let (errors, warnings) = counts(&issues);
    println!(
        "{}: {errors} error(s), {warnings} warning(s)",
        pack.display()
    );
    if errors > 0 || (strict && warnings > 0) {
        bail!("{} failed validation", pack.display());
    }
    Ok(())
}

fn bundle_cmd(dir: &Path, out: &Path) -> Result<()> {
    ensure!(dir.is_dir(), "{} is not a pack directory", dir.display());
    let files = read_dir_files(dir)?;
    let issues = validate_files(&files, None);
    print_issues(&issues);
    if has_errors(&issues) {
        bail!("{} failed validation; not bundled", dir.display());
    }
    let bytes = write_bundle(&files)?;
    std::fs::write(out, &bytes).with_context(|| format!("writing {}", out.display()))?;
    println!(
        "{}: {} files, {} bytes",
        out.display(),
        files.len(),
        bytes.len()
    );
    Ok(())
}

fn unbundle_cmd(file: &Path, out: &Path) -> Result<()> {
    let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    // `read_bundle` rejects any path that is absolute or climbs out with
    // `..`, so every entry lands inside `out`.
    let files = read_bundle(&bytes).with_context(|| format!("reading {}", file.display()))?;
    if out.exists() {
        ensure!(
            out.read_dir()?.next().is_none(),
            "{} exists and is not empty",
            out.display()
        );
    }
    for (path, data) in &files {
        let dest = out.join(path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(&dest, data).with_context(|| format!("writing {}", dest.display()))?;
    }
    println!("{}: {} files", out.display(), files.len());
    Ok(())
}

fn info_cmd(path: &Path, json: bool) -> Result<()> {
    let files: PackFiles = read_files(path)?;
    let pack = ExpansionPack::from_files(&files)?;
    let issues = validate_files(&files, None);
    let (errors, warnings) = counts(&issues);
    let m = &pack.manifest;
    let sections: Vec<&str> = SECTION_FILES
        .iter()
        .copied()
        .filter(|f| files.contains_key(*f))
        .collect();
    let total: usize = files.values().map(Vec::len).sum();
    if json {
        let v = serde_json::json!({
            "manifest": m,
            "sections": sections,
            "maps": pack.maps.keys().collect::<Vec<_>>(),
            "assets": pack.assets.len(),
            "files": files.len(),
            "bytes": total,
            "errors": errors,
            "warnings": warnings,
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    println!("{} {} ({})", m.name, m.version, m.id);
    if !m.authors.is_empty() {
        println!("  authors:   {}", m.authors.join(", "));
    }
    if !m.license.is_empty() {
        println!("  license:   {}", m.license);
    }
    if let Some(h) = &m.homepage {
        println!("  homepage:  {h}");
    }
    println!("  schema:    {}   disc: {}", m.schema, m.disc);
    for (label, list) in [
        ("tags", &m.tags),
        ("requires", &m.requires),
        ("conflicts", &m.conflicts),
    ] {
        if !list.is_empty() {
            println!("  {label:<10} {}", list.join(", "));
        }
    }
    println!(
        "  sections:  {}",
        if sections.is_empty() {
            "(none)".to_string()
        } else {
            sections.join(", ")
        }
    );
    println!("  maps:      {}", pack.maps.len());
    println!("  assets:    {}", pack.assets.len());
    println!("  files:     {} ({total} bytes)", files.len());
    println!("  validate:  {errors} error(s), {warnings} warning(s)");
    if !m.description.is_empty() {
        println!();
        println!("{}", m.description.trim());
    }
    Ok(())
}

fn format_info(json: bool) -> Result<()> {
    if json {
        let v = serde_json::json!({
            "schema": SCHEMA_VERSION,
            "bundle_version": bundle::BUNDLE_VERSION,
            "bundle_extension": bundle::EXTENSION,
            "sections": SECTION_FILES,
            "docs": DOC_FILES,
            "asset_extensions": legaia_expansion_format::validate::ASSET_EXTENSIONS,
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    println!(
        "expansion pack schema {SCHEMA_VERSION}, .{} bundle version {}",
        bundle::EXTENSION,
        bundle::BUNDLE_VERSION
    );
    println!();
    println!("  pack.toml           manifest (required)");
    for f in SECTION_FILES {
        println!("  {f:<19} content section");
    }
    println!("  maps/<id>.toml      one custom map per file");
    println!(
        "  assets/...          the modder's own files: {}",
        legaia_expansion_format::validate::ASSET_EXTENSIONS.join(", ")
    );
    println!("  {}", DOC_FILES.join(", "));
    println!();
    println!("Reference: docs/formats/expansion-pack.md");
    Ok(())
}
