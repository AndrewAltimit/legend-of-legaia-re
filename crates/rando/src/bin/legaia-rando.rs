//! `legaia-rando` — top-level randomizer / disc patcher CLI.
//!
//! Reads a **user-supplied** retail Legaia disc image, plans a randomization
//! from a seed, and emits a portable PPF 3.0 patch (the redistributable
//! deliverable) plus, optionally, a full patched `.bin` copy for local play.
//! The patched `.bin` contains Sony bytes and must never be shared — the
//! shareable artifacts are the patcher and the seed.
//!
//! ```text
//! legaia-rando randomize --input DISC.bin --seed mysrun --drops shuffle --patch out.ppf
//! legaia-rando drops     --input DISC.bin      # read-only: list current monster drops
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};

use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;
use legaia_rando::items::valid_item_pool;
use legaia_rando::ppf;

#[derive(Parser)]
#[command(
    name = "legaia-rando",
    about = "Legend of Legaia randomizer / disc patcher (operates on a user-supplied disc)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Plan a randomization from a seed and write a PPF patch (and optionally a
    /// patched disc image copy).
    Randomize(RandomizeArgs),
    /// Read-only: list every monster's current item drop.
    Drops {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Apply a PPF patch to a copy of a disc and confirm it applies cleanly
    /// (records applied, the result still parses). Use this to check that a
    /// shared patch + seed match your own disc before playing.
    Verify {
        /// Path to the user's retail disc image the patch targets.
        #[arg(long)]
        input: PathBuf,
        /// The PPF 3.0 patch to apply.
        #[arg(long)]
        patch: PathBuf,
        /// Optionally write the patched image here (for local play only).
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[derive(Parser)]
struct RandomizeArgs {
    /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
    #[arg(long)]
    input: PathBuf,
    /// Seed for reproducibility. Either a number (decimal or `0x`-hex) or any
    /// string (hashed to a number). The resolved numeric seed is always
    /// printed so a run can be reproduced exactly. If omitted, one is drawn
    /// from the system clock.
    #[arg(long)]
    seed: Option<String>,
    /// How monster item drops are reassigned.
    #[arg(long, value_enum, default_value_t = DropArg::Shuffle)]
    drops: DropArg,
    /// How random-encounter formations are reassigned (within each scene's own
    /// monster pool, so every monster stays scene-loaded).
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    encounters: DropArg,
    /// Write the portable PPF 3.0 patch here (defaults to `<input>.ppf`).
    #[arg(long)]
    patch: Option<PathBuf>,
    /// Also write a full patched disc-image copy here (contains Sony bytes —
    /// for local play only, never redistribute).
    #[arg(long)]
    output: Option<PathBuf>,
    /// Write a reproducibility manifest (seed + options + change summary) here.
    /// Safe to share alongside the PPF — it embeds no game bytes.
    #[arg(long)]
    manifest: Option<PathBuf>,
    /// Plan and report the run but write no files (patch / output / manifest).
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[derive(Copy, Clone, ValueEnum)]
enum DropArg {
    /// Redistribute the existing values (drops / encounter ids).
    Shuffle,
    /// Draw each value uniformly from the valid pool.
    Random,
    /// Leave untouched.
    None,
}

/// Lowercase name of a mode for the manifest (valid-TOML string value).
fn mode_str(mode: DropMode) -> &'static str {
    match mode {
        DropMode::Shuffle => "shuffle",
        DropMode::Random => "random",
    }
}

impl DropArg {
    fn mode(self) -> Option<DropMode> {
        match self {
            DropArg::Shuffle => Some(DropMode::Shuffle),
            DropArg::Random => Some(DropMode::Random),
            DropArg::None => None,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Drops { input } => cmd_drops(&input),
        Cmd::Randomize(args) => cmd_randomize(args),
        Cmd::Verify {
            input,
            patch,
            output,
        } => cmd_verify(&input, &patch, output.as_deref()),
    }
}

/// Resolve a user seed string to a numeric seed. A plain number is used
/// directly; anything else is hashed with FNV-1a-64 so a memorable string seed
/// is stable across runs and platforms.
fn resolve_seed(seed: &str) -> u64 {
    let t = seed.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X"))
        && let Ok(v) = u64::from_str_radix(hex, 16)
    {
        return v;
    }
    if let Ok(v) = t.parse::<u64>() {
        return v;
    }
    // FNV-1a-64 of the raw string.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in t.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn clock_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15)
}

fn load_image(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("read disc image {}", path.display()))
}

fn cmd_drops(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let drops = apply::current_drops(&patcher)?;
    let item_names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
    let mut n = 0;
    for d in &drops {
        if d.item == 0 {
            continue;
        }
        let name = item_names
            .as_ref()
            .and_then(|t| t.name(d.item))
            .unwrap_or("?");
        println!(
            "monster {:>3}  drop item {:>3} ({:<16})  {:>3}%",
            d.monster_id, d.item, name, d.chance
        );
        n += 1;
    }
    println!("{n} monsters have a drop (of {} slots)", drops.len());
    Ok(())
}

fn cmd_randomize(args: RandomizeArgs) -> Result<()> {
    let seed = match &args.seed {
        Some(s) => resolve_seed(s),
        None => clock_seed(),
    };
    let original = load_image(&args.input)?;
    let mut patcher = DiscPatcher::open(original.clone()).context("parse disc image")?;

    let mode = args.drops.mode();
    let enc_mode = args.encounters.mode();

    println!("seed: {seed} (0x{seed:016X})");
    // Manifest lines accumulate the run's options + outcome for reproducibility.
    let mut manifest = vec![
        "# legaia-rando run manifest".to_string(),
        format!("seed = {seed}  # 0x{seed:016X}"),
        format!("input = {:?}", args.input.display().to_string()),
    ];

    if let Some(mode) = mode {
        // Random mode needs the valid item pool from SCUS; shuffle does not.
        let pool = if mode == DropMode::Random {
            let scus = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
                .context("SCUS_942.54 not found in disc image (needed for --drops random)")?;
            valid_item_pool(&scus).context("build valid item pool from SCUS")?
        } else {
            Vec::new()
        };
        let (plan, report) = apply::randomize_drops(&mut patcher, &pool, seed, mode)?;
        println!(
            "drops: {} of {} monsters reassigned ({:?})",
            report.changed,
            plan.len(),
            mode
        );
        manifest.push(format!("drops = {:?}", mode_str(mode)));
        manifest.push(format!(
            "drops_changed = {}  # of {} dropping monsters",
            report.changed,
            plan.len()
        ));
        if !report.skipped.is_empty() {
            println!(
                "  note: {} slot(s) too full to re-pack, left unchanged: {:?}",
                report.skipped.len(),
                report.skipped
            );
            manifest.push(format!("drops_skipped = {:?}", report.skipped));
        }
    } else {
        println!("drops: untouched");
        manifest.push("drops = \"none\"".to_string());
    }

    if let Some(enc_mode) = enc_mode {
        let report = apply::randomize_encounters(&mut patcher, seed, enc_mode)?;
        println!(
            "encounters: {} scenes rewritten, {} ids changed ({:?})",
            report.scenes_changed, report.ids_changed, enc_mode
        );
        manifest.push(format!("encounters = {:?}", mode_str(enc_mode)));
        manifest.push(format!(
            "encounters_scenes_changed = {}",
            report.scenes_changed
        ));
        manifest.push(format!("encounters_ids_changed = {}", report.ids_changed));
        if !report.skipped.is_empty() {
            println!(
                "  note: {} scene MAN(s) too tight to re-pack, left unchanged: {:?}",
                report.skipped.len(),
                report.skipped
            );
            manifest.push(format!("encounters_skipped = {:?}", report.skipped));
        }
    } else {
        println!("encounters: untouched");
        manifest.push("encounters = \"none\"".to_string());
    }

    // Diff original vs patched -> PPF.
    let patched = patcher.into_image();
    if patched.len() != original.len() {
        bail!("patched image changed size — refusing to emit (all edits must be same-size)");
    }
    let runs = ppf::diff_runs(&original, &patched);
    let changed_bytes: usize = runs.iter().map(|r| r.bytes.len()).sum();
    manifest.push(format!("ppf_records = {}", runs.len()));
    manifest.push(format!("bytes_changed = {changed_bytes}"));

    if runs.is_empty() {
        println!("note: no bytes changed (nothing to randomize for these options)");
    }

    if args.dry_run {
        println!(
            "dry run: would write a {}-record PPF ({} bytes changed); no files written",
            runs.len(),
            changed_bytes
        );
        return Ok(());
    }

    let desc = format!("Legend of Legaia randomizer seed {seed}");
    let ppf_bytes = ppf::write_ppf3(&desc, &runs);
    let patch_path = args
        .patch
        .clone()
        .unwrap_or_else(|| with_extension(&args.input, "ppf"));
    std::fs::write(&patch_path, &ppf_bytes)
        .with_context(|| format!("write patch {}", patch_path.display()))?;
    println!(
        "patch: {} ({} records, {} bytes changed)",
        patch_path.display(),
        runs.len(),
        changed_bytes
    );

    if let Some(out) = &args.output {
        std::fs::write(out, &patched).with_context(|| format!("write {}", out.display()))?;
        println!(
            "patched image: {} (contains Sony bytes — do not redistribute)",
            out.display()
        );
    }

    if let Some(mpath) = &args.manifest {
        let mut text = manifest.join("\n");
        text.push('\n');
        std::fs::write(mpath, text)
            .with_context(|| format!("write manifest {}", mpath.display()))?;
        println!("manifest: {}", mpath.display());
    }

    Ok(())
}

/// Apply a PPF to a copy of the disc and confirm the result still parses.
fn cmd_verify(input: &Path, patch: &Path, output: Option<&Path>) -> Result<()> {
    let mut image = load_image(input)?;
    let ppf = std::fs::read(patch).with_context(|| format!("read patch {}", patch.display()))?;
    let applied =
        legaia_rando::ppf::apply_ppf3(&mut image, &ppf).context("apply PPF to disc image")?;
    // Re-parse the patched image end to end as a sanity check.
    let patcher = DiscPatcher::open(image).context("patched image no longer parses as a disc")?;
    let drops = apply::current_drops(&patcher)
        .map(|d| d.iter().filter(|x| x.item != 0).count())
        .unwrap_or(0);
    println!(
        "verify OK: {applied} PPF records applied; disc parses ({} PROT entries, {drops} monster drops)",
        patcher.entry_count()
    );
    if let Some(out) = output {
        std::fs::write(out, patcher.image()).with_context(|| format!("write {}", out.display()))?;
        println!(
            "patched image: {} (contains Sony bytes — do not redistribute)",
            out.display()
        );
    }
    Ok(())
}

/// `<stem>.<ext>` next to the input path (e.g. `disc.bin` -> `disc.ppf`).
fn with_extension(input: &Path, ext: &str) -> PathBuf {
    let mut p = input.to_path_buf();
    p.set_extension(ext);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_resolution_is_stable_and_parses_numbers() {
        // Numbers are used directly (decimal + hex).
        assert_eq!(resolve_seed("42"), 42);
        assert_eq!(resolve_seed("0x1F"), 0x1F);
        assert_eq!(resolve_seed("0XFF"), 0xFF);
        // A non-numeric string hashes stably (reproducibility contract) and the
        // same string always maps to the same seed.
        let a = resolve_seed("my cool run");
        assert_eq!(a, resolve_seed("my cool run"));
        assert_ne!(a, resolve_seed("my other run"));
        // A string that isn't a bare number doesn't collide with the number path.
        assert_ne!(resolve_seed("42x"), 42);
    }

    #[test]
    fn mode_str_is_lowercase() {
        assert_eq!(mode_str(DropMode::Shuffle), "shuffle");
        assert_eq!(mode_str(DropMode::Random), "random");
    }
}
