//! Unified Legaia extraction pipeline. Runs disc → PROT → categorize →
//! streaming-format extract → TIM-to-PNG in one shot.
//!
//! Wraps the per-crate library APIs; equivalent to running `disc-extract
//! extract`, `prot-extract extract`, `asset scan-stream`/`extract`, and
//! `tim convert-dir` in sequence, but with one CLI and one output tree.
//! The pipeline itself is the crate's library (`legaia_extract::run`), which
//! `legaia-studio` drives too; this file is only the argument parser and a
//! printing [`legaia_extract::Reporter`].

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(
    name = "legaia-extract",
    version,
    about = "Run the full Legaia extraction pipeline (disc → PROT → categorize → sub-assets → PNG → XA → font)"
)]
struct Cli {
    /// Legend of Legaia (USA) disc image: a raw Mode2/2352 `.bin` dump, or
    /// its `.cue` sheet (the referenced BINARY track is resolved
    /// automatically).
    bin: PathBuf,
    /// Output directory (resolved against the current directory when
    /// relative). Created if missing. Existing files are overwritten.
    #[arg(long, default_value = "extracted")]
    out: PathBuf,
    /// Skip the disc verification step (don't compute SHA-256).
    #[arg(long)]
    skip_verify: bool,
    /// Skip converting the streaming-container TIMs to PNG (a quick step;
    /// the bulk texture inventory is the TIM-catalog TSVs, see
    /// --skip-catalog).
    #[arg(long)]
    skip_png: bool,
    /// Skip CD-XA demux → per-channel WAV (the streamed-audio step).
    #[arg(long)]
    skip_xa: bool,
    /// Skip writing the TIM-catalog TSVs (the texture inventory step).
    #[arg(long)]
    skip_catalog: bool,
    /// Skip building the dialog-font artifacts (`font/` dir the engine and
    /// asset-viewer load text from).
    #[arg(long)]
    skip_font: bool,
    /// Also write every TIM in every PROT entry (raw and LZS-compressed) as
    /// `.tim` + `.png` under `tim_scan/` - `asset tim-scan` + `tim
    /// convert-dir` in one step.
    #[arg(long)]
    all_textures: bool,
    /// Print one line per file written.
    #[arg(short, long)]
    verbose: bool,
}

/// Restore default SIGPIPE behaviour so piping into `head` etc. exits
/// quietly instead of panicking on a broken-pipe write.
fn reset_sigpipe() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

/// Prints the pipeline's messages the way this binary always has: status
/// lines prefixed, per-file lines bare, warnings on stderr.
struct Print;

impl legaia_extract::Reporter for Print {
    fn log(&self, msg: &str) {
        println!("[legaia-extract] {}", msg);
    }
    fn detail(&self, msg: &str) {
        println!("{}", msg);
    }
    fn warn(&self, msg: &str) {
        eprintln!("{}", msg);
    }
}

fn main() -> Result<()> {
    reset_sigpipe();
    let cli = Cli::parse();
    let opts = legaia_extract::Options {
        bin: cli.bin,
        out: cli.out,
        skip_verify: cli.skip_verify,
        skip_png: cli.skip_png,
        skip_xa: cli.skip_xa,
        skip_catalog: cli.skip_catalog,
        skip_font: cli.skip_font,
        all_textures: cli.all_textures,
        verbose: cli.verbose,
    };
    legaia_extract::run(&opts, &Print)?;
    Ok(())
}
