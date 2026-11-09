//! `legaia-engine retail-compare`: run the retail comparison corpus and write
//! the human report. The logic is [`crate::retail_compare`]; this is the
//! argument surface and the file output.

use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::retail_compare::{
    Baseline, IMAGE_CHANNELS, RunOptions, SmokeFrame, markdown_report, resolve_dirs,
    run_corpus_timed, summarise,
};
use crate::retail_compare_image::FrameRender;

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
    /// Diagnostic: apply the whole retail save before the scene entry as
    /// well as after, entering through `enter_scene_live` with no resume
    /// landing (the default is the engine's own card-load order: story
    /// flags, the landing, then the whole save).
    #[arg(long, default_value_t = false)]
    pub flags_first: bool,
    /// Write the committed score baseline to this path (no pixels, no RAM).
    #[arg(long)]
    pub write_baseline: Option<PathBuf>,
    /// Check the run against this baseline and exit non-zero on any drop.
    #[arg(long)]
    pub check_baseline: Option<PathBuf>,
    /// How the image child draws its frame. `psx` (the default) is the
    /// accuracy metric the baseline holds; `clean` and `enhanced` are the
    /// smoke tier, checked with `--smoke-check` and never written to or
    /// checked against the ratchet.
    #[arg(long, value_enum, default_value_t = FrameRender::Psx)]
    pub render: FrameRender,
    /// Smoke tier: report every frame of a state this baseline scores that
    /// reads as broken against its accuracy frame, and exit non-zero on any
    /// (see `Baseline::smoke_breaks`). Needs `--smoke-reference`.
    #[arg(long, requires = "smoke_reference")]
    pub smoke_check: Option<PathBuf>,
    /// An accuracy run's output directory (`--images`, same engine): the frames
    /// `--smoke-check` measures against.
    #[arg(long)]
    pub smoke_reference: Option<PathBuf>,
    /// Capture-coverage census: read every corpus state's scene, class and
    /// on-screen battle action plus the disc's monster / spell / formation
    /// lists, write them as JSON to this path and exit without seeding the
    /// engine ([`crate::retail_coverage`], `docs/tooling/capture-coverage.md`).
    #[arg(long)]
    pub census: Option<PathBuf>,
    /// States run at once, each on a worker thread with its own image child.
    /// `report.json` is the same at any count; states with one label run in
    /// turn. Default 1.
    #[arg(long, default_value_t = 1)]
    pub jobs: usize,
    /// Reuse (and fill) per-state results under this directory, keyed on the
    /// binary, the disc, the state and the environment
    /// ([`crate::retail_compare_cache`]). Refused with the ratchet flags.
    #[arg(long)]
    pub cache: Option<PathBuf>,
    /// One line on stderr as each state finishes.
    #[arg(long, default_value_t = false)]
    pub progress: bool,
}

pub fn run(args: RetailCompareArgs) -> Result<()> {
    let (m, l, e) = resolve_dirs();
    let manifest_path = args.manifest.or(m).context("no scripts/scenarios.toml")?;
    let library = args.library.or(l).context("no save library")?;
    let extracted = args.extracted_root.or(e).context("no extracted disc")?;
    let manifest = psx_ram_image::ScenarioManifest::from_path(&manifest_path)?;
    if let Some(p) = &args.census {
        let c = crate::retail_coverage::census(
            &extracted,
            &library,
            &manifest,
            args.filter.as_deref(),
            args.jobs,
        )?;
        std::fs::write(p, serde_json::to_string_pretty(&c)?)?;
        println!(
            "retail-compare census: {} states {:?}, {} monsters, {} spells, {} scenes -> {}",
            c.states.len(),
            crate::retail_coverage::class_counts(&c),
            c.disc.monsters.len(),
            c.disc.spells.len(),
            c.disc.scenes.len(),
            p.display()
        );
        return Ok(());
    }
    // The ratchet holds the accuracy mode's scores only: a smoke-tier render
    // neither writes nor checks it.
    anyhow::ensure!(
        args.render == FrameRender::Psx
            || (args.write_baseline.is_none() && args.check_baseline.is_none()),
        "--render {:?} is the smoke tier: use --smoke-check, not the baseline ratchet",
        args.render
    );
    // The ratchet only ever takes a fresh measurement.
    anyhow::ensure!(
        args.cache.is_none()
            || (args.write_baseline.is_none()
                && args.check_baseline.is_none()
                && args.smoke_check.is_none()),
        "--cache is for iterating: a baseline write or check always measures"
    );
    crate::retail_compare_image::set_frame_render(args.render);
    let exe = std::env::current_exe()?;
    std::fs::create_dir_all(&args.out)?;
    let out = std::fs::canonicalize(&args.out)?;
    let (reports, timings) = run_corpus_timed(&RunOptions {
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
        jobs: args.jobs,
        cache: args.cache.as_deref(),
        progress: args.progress,
    })?;
    // Beside the report rather than in it: `report.json` is a pure function
    // of the states and the binary.
    std::fs::write(
        out.join("timings.json"),
        serde_json::to_string_pretty(&timings)?,
    )?;
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
        "retail-compare: {} states, {} seeded ({} on a patched disc), mean state score {:.3} \
         retail -> {}",
        summary.states,
        summary.seeded,
        summary.patched,
        summary.mean_state_score,
        out.join("report.md").display()
    );
    let cached = timings.iter().filter(|t| t.cached).count();
    if cached > 0 {
        println!(
            "  {cached} of {} state(s) reused from the cache",
            timings.len()
        );
    }
    for (ch, (mean, n)) in &summary.channels {
        println!("  {ch:<10} {mean:.3} over {n}");
    }
    for (ch, (mean, n)) in &summary.patched_channels {
        println!("  patched {ch:<10} {mean:.3} over {n}");
    }
    if let Some(p) = args.write_baseline {
        // A filtered or display-less run measures a subset, so it updates
        // the states it seeded and keeps the rest of an existing baseline
        // rather than rewriting the file from the subset; a seeded state's
        // channel set is this run's, plus the image channel a display-less
        // run did not score.
        let prior: Option<Baseline> = std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok());
        let unmeasured: &[&str] = if args.images { &[] } else { IMAGE_CHANNELS };
        let failed_frames: BTreeSet<&str> = reports
            .iter()
            .filter(|r| r.frame_failed())
            .map(|r| r.label.as_str())
            .collect();
        for label in &failed_frames {
            eprintln!("[frame-failed] {label}: no frame was made; its image score is kept");
        }
        let b = Baseline::from_reports(&reports).merged_over(prior, unmeasured, &failed_frames);
        std::fs::write(&p, serde_json::to_string_pretty(&b)? + "\n")?;
        println!("wrote baseline {}", p.display());
    }
    if let Some(p) = args.check_baseline {
        let b: Baseline = serde_json::from_str(&std::fs::read_to_string(&p)?)?;
        let allow: &[&str] = if args.images { &[] } else { IMAGE_CHANNELS };
        let regs = b.regressions(&reports, allow);
        for r in &regs {
            eprintln!("[regression] {r}");
        }
        for r in b.patched_drift(&reports, allow) {
            eprintln!("[patched-drift] {r}");
        }
        for r in b.fallback_skips(&reports) {
            eprintln!("[skip] {r}");
        }
        anyhow::ensure!(
            regs.is_empty(),
            "{} score(s) dropped below the baseline",
            regs.len()
        );
        println!("baseline {} holds", p.display());
    }
    if let Some(p) = args.smoke_check {
        anyhow::ensure!(args.images, "--smoke-check scores frames: pass --images");
        let b: Baseline = serde_json::from_str(&std::fs::read_to_string(&p)?)?;
        let reference = args
            .smoke_reference
            .as_deref()
            .context("--smoke-check needs --smoke-reference")?;
        let frame_of = |label: &str| -> Option<SmokeFrame> {
            let name = format!("{label}.png");
            let accuracy = crate::retail_compare_image::engine_panel_rgba(&reference.join(&name))?;
            let frame = crate::retail_compare_image::engine_panel_rgba(&out.join(&name))?;
            SmokeFrame::measure(&accuracy, &frame)
        };
        let breaks = b.smoke_breaks(&reports, &frame_of);
        for r in &breaks {
            eprintln!("[smoke-break] {r}");
        }
        anyhow::ensure!(
            breaks.is_empty(),
            "{} frame(s) in render mode {:?} read as broken against the accuracy frames",
            breaks.len(),
            args.render
        );
        println!(
            "smoke tier {:?} holds against the accuracy frames in {}",
            args.render,
            reference.display()
        );
    }
    Ok(())
}
