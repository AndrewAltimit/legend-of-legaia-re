//! `legaia-engine retail-compare`: run the retail comparison corpus and write
//! the human report. The logic is [`crate::retail_compare`]; this is the
//! argument surface and the file output.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::retail_compare::{
    Baseline, RunOptions, markdown_report, resolve_dirs, run_corpus, summarise,
};

/// Seed the engine from every seedable retail save state in the library and
/// score it against retail per channel (scene, mode, position, footing,
/// camera, BGM, party, flags, inventory, and - with `--images` - the
/// displayed frame). Writes `report.md` + `report.json` (+ side-by-side PNGs)
/// under `--out`, which must be a gitignored directory: the PNGs carry
/// retail pixels. See `docs/tooling/retail-compare.md`.
#[derive(clap::Args, Debug)]
pub struct RetailCompareArgs {
    /// Save-state library (default: `$LEGAIA_SAVES_LIBRARY`, else
    /// `saves/library`).
    #[arg(long)]
    pub library: Option<PathBuf>,
    /// Extracted disc (default: `$LEGAIA_EXTRACTED_DIR`, else `extracted`).
    #[arg(long)]
    pub extracted_root: Option<PathBuf>,
    /// Scenario manifest (default: `scripts/scenarios.toml`).
    #[arg(long)]
    pub manifest: Option<PathBuf>,
    /// Report directory (gitignored).
    #[arg(long, default_value = "captures/retail-compare")]
    pub out: PathBuf,
    /// Also render each seeded state through `play-window` (needs a
    /// display) and score the frame.
    #[arg(long, default_value_t = false)]
    pub images: bool,
    /// Only states whose label contains this substring.
    #[arg(long)]
    pub filter: Option<String>,
    /// Diagnostic: apply the retail save before the scene entry as well as
    /// after, so the entry scripts see the retail story flags (the default
    /// is the engine's own card-load order, which hydrates after entry).
    #[arg(long, default_value_t = false)]
    pub flags_first: bool,
    /// Write the committed score baseline to this path (no pixels, no RAM).
    #[arg(long)]
    pub write_baseline: Option<PathBuf>,
    /// Check the run against this baseline and exit non-zero on any drop.
    #[arg(long)]
    pub check_baseline: Option<PathBuf>,
}

pub fn run(args: RetailCompareArgs) -> Result<()> {
    let (m, l, e) = resolve_dirs();
    let manifest_path = args.manifest.or(m).context("no scripts/scenarios.toml")?;
    let library = args.library.or(l).context("no save library")?;
    let extracted = args.extracted_root.or(e).context("no extracted disc")?;
    let manifest = legaia_mednafen::ScenarioManifest::from_path(&manifest_path)?;
    let exe = std::env::current_exe()?;
    std::fs::create_dir_all(&args.out)?;
    let out = std::fs::canonicalize(&args.out)?;
    let reports = run_corpus(&RunOptions {
        extracted: &extracted,
        library: &library,
        manifest: &manifest,
        engine_exe: args.images.then_some(exe.as_path()),
        out_dir: Some(&out),
        filter: args.filter.as_deref(),
        order: if args.flags_first {
            crate::retail_compare::SeedOrder::FlagsFirst
        } else {
            crate::retail_compare::SeedOrder::Resume
        },
    })?;
    let summary = summarise(&reports);
    std::fs::write(out.join("report.md"), markdown_report(&reports, &summary))?;
    std::fs::write(
        out.join("report.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "summary": summary,
            "states": reports,
        }))?,
    )?;
    println!(
        "retail-compare: {} states, {} seeded, mean state score {:.3} -> {}",
        summary.states,
        summary.seeded,
        summary.mean_state_score,
        out.join("report.md").display()
    );
    for (ch, (mean, n)) in &summary.channels {
        println!("  {ch:<10} {mean:.3} over {n}");
    }
    if let Some(p) = args.write_baseline {
        let b = Baseline::from_reports(&reports);
        std::fs::write(&p, serde_json::to_string_pretty(&b)? + "\n")?;
        println!("wrote baseline {}", p.display());
    }
    if let Some(p) = args.check_baseline {
        let b: Baseline = serde_json::from_str(&std::fs::read_to_string(&p)?)?;
        let allow: &[&str] = if args.images { &[] } else { &["image"] };
        let regs = b.regressions(&reports, allow);
        for r in &regs {
            eprintln!("[regression] {r}");
        }
        anyhow::ensure!(
            regs.is_empty(),
            "{} score(s) dropped below the baseline",
            regs.len()
        );
        println!("baseline {} holds", p.display());
    }
    Ok(())
}
