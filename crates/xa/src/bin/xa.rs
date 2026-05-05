use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use legaia_xa::{Channels, DecodeOptions};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "xa", about = "PSX XA-ADPCM decoder + WAV exporter")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print metadata: sound-group count, predicted output duration.
    Info {
        path: PathBuf,
        #[arg(long, value_enum, default_value_t = ChannelsArg::Mono)]
        channels: ChannelsArg,
        #[arg(long, default_value_t = 37800)]
        sample_rate: u32,
    },
    /// Decode a single .XA to .WAV.
    Convert {
        path: PathBuf,
        #[arg(short, long)]
        out: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = ChannelsArg::Mono)]
        channels: ChannelsArg,
        #[arg(long, default_value_t = 37800)]
        sample_rate: u32,
    },
    /// Convert every .XA under `dir` to .WAV.
    ConvertDir {
        dir: PathBuf,
        #[arg(short, long)]
        out: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = ChannelsArg::Mono)]
        channels: ChannelsArg,
        #[arg(long, default_value_t = 37800)]
        sample_rate: u32,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ChannelsArg {
    Mono,
    Stereo,
}

impl From<ChannelsArg> for Channels {
    fn from(a: ChannelsArg) -> Self {
        match a {
            ChannelsArg::Mono => Channels::Mono,
            ChannelsArg::Stereo => Channels::Stereo,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Info {
            path,
            channels,
            sample_rate,
        } => info(&path, channels.into(), sample_rate),
        Cmd::Convert {
            path,
            out,
            channels,
            sample_rate,
        } => convert(&path, out.as_deref(), channels.into(), sample_rate),
        Cmd::ConvertDir {
            dir,
            out,
            channels,
            sample_rate,
        } => convert_dir(&dir, out.as_deref(), channels.into(), sample_rate),
    }
}

fn info(path: &Path, channels: Channels, sample_rate: u32) -> Result<()> {
    let buf = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if buf.len() % legaia_xa::SOUND_GROUP_BYTES != 0 {
        bail!(
            "{}: size {} is not a multiple of 128 (XA sound group)",
            path.display(),
            buf.len()
        );
    }
    let n_groups = buf.len() / legaia_xa::SOUND_GROUP_BYTES;
    // 4-bit mode: 8 SUs × 28 samples per group; per-channel sample count =
    // total / channels.
    let total_samples = n_groups * legaia_xa::UNITS_PER_GROUP_4BIT * legaia_xa::SAMPLES_PER_UNIT;
    let per_channel = total_samples / channels.n() as usize;
    let dur_sec = per_channel as f64 / sample_rate as f64;
    println!("file:        {}", path.display());
    println!(
        "size:        {} bytes ({} sound groups)",
        buf.len(),
        n_groups
    );
    println!("channels:    {:?}", channels);
    println!("sample_rate: {} Hz", sample_rate);
    println!(
        "duration:    {:.3} sec ({} samples per channel)",
        dur_sec, per_channel
    );
    Ok(())
}

fn convert(path: &Path, out: Option<&Path>, channels: Channels, sample_rate: u32) -> Result<()> {
    let buf = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let opts = DecodeOptions {
        channels,
        sample_rate,
    };
    let (samples, report) = legaia_xa::decode(&buf, opts)?;
    let target: PathBuf = out
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| path.with_extension("wav"));
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    legaia_xa::write_wav(&target, &samples, channels, sample_rate)?;
    println!(
        "{} -> {} ({} groups, {} skipped as invalid, {} samples)",
        path.display(),
        target.display(),
        report.n_groups,
        report.n_groups_skipped,
        report.n_samples_interleaved
    );
    Ok(())
}

fn convert_dir(dir: &Path, out: Option<&Path>, channels: Channels, sample_rate: u32) -> Result<()> {
    let dir = dir
        .canonicalize()
        .with_context(|| format!("canonicalizing {}", dir.display()))?;
    let mut paths = vec![];
    walk_xa(&dir, &mut paths)?;
    if paths.is_empty() {
        bail!("no .xa files found under {}", dir.display());
    }
    let mut ok = 0usize;
    let mut fail = 0usize;
    for p in &paths {
        let target_root = match out {
            Some(o) => o.to_path_buf(),
            None => dir.clone(),
        };
        let rel = p.strip_prefix(&dir).unwrap_or(p);
        let target = target_root.join(rel).with_extension("wav");
        match convert(p, Some(&target), channels, sample_rate) {
            Ok(()) => ok += 1,
            Err(e) => {
                eprintln!("{}: {}", p.display(), e);
                fail += 1;
            }
        }
    }
    eprintln!(
        "converted {} OK, {} failed (out of {} files)",
        ok,
        fail,
        paths.len()
    );
    Ok(())
}

fn walk_xa(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_xa(&path, out)?;
        } else if path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case("xa"))
            .unwrap_or(false)
        {
            out.push(path);
        }
    }
    Ok(())
}
