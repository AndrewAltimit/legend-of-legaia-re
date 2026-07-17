use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use legaia_xa::{Channels, DecodeOptions};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "xa", version, about = "PSX XA-ADPCM decoder + WAV exporter")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print metadata: sound-group count, predicted output duration.
    ///
    /// Input: a subheader-stripped Form-1 .XA dump (e.g. extracted/XA*.XA
    /// from `legaia-extract` / `disc-extract extract`). The sample rate /
    /// channel mode must be guessed here - prefer `demux-disc-all` on the
    /// raw disc .bin for correct pacing.
    Info {
        path: PathBuf,
        #[arg(long, value_enum, default_value_t = ChannelsArg::Mono)]
        channels: ChannelsArg,
        #[arg(long, default_value_t = 37800)]
        sample_rate: u32,
        #[arg(long, value_enum, default_value_t = BitsArg::Four)]
        bits: BitsArg,
    },
    /// Decode a single .XA to .WAV.
    ///
    /// Input: a subheader-stripped Form-1 .XA dump (e.g. extracted/XA*.XA
    /// from `legaia-extract`). Rate/channels are guesses here - prefer
    /// `demux-disc-all` on the raw disc .bin for correct pacing.
    Convert {
        path: PathBuf,
        #[arg(short, long)]
        out: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = ChannelsArg::Mono)]
        channels: ChannelsArg,
        #[arg(long, default_value_t = 37800)]
        sample_rate: u32,
        #[arg(long, value_enum, default_value_t = BitsArg::Four)]
        bits: BitsArg,
    },
    /// Convert every .XA under `dir` to .WAV.
    ///
    /// Input: a directory of subheader-stripped Form-1 .XA dumps (e.g. the
    /// extracted/ tree from `legaia-extract`). Prefer `demux-disc-all` on
    /// the raw disc .bin for correct pacing.
    ConvertDir {
        dir: PathBuf,
        #[arg(short, long)]
        out: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = ChannelsArg::Mono)]
        channels: ChannelsArg,
        #[arg(long, default_value_t = 37800)]
        sample_rate: u32,
        #[arg(long, value_enum, default_value_t = BitsArg::Four)]
        bits: BitsArg,
    },
    /// Walk the disc's ISO9660 tree and demux EVERY `*.XA` file, one WAV
    /// per `(file_no, ch_no)` channel, each decoded at its true per-sector
    /// sample rate / channel mode read from the CD-XA subheaders. This is
    /// the correct-pacing path: no `--sample-rate` guess, and each track
    /// gets its own rate. Prefer this over `convert`/`convert-dir`, which
    /// operate on subheader-stripped Form-1 dumps and must guess the rate.
    DemuxDiscAll {
        /// Path to the disc image (`.bin`, Mode 2 / 2352 raw sectors).
        bin: PathBuf,
        /// Output directory; WAVs land under `<out>/<xa-stem>_fileN_chM.wav`.
        /// The default is resolved against the current directory.
        #[arg(short, long, default_value = "extracted/xa_demux")]
        out: PathBuf,
    },
    /// Demux a CD-XA stream directly off a `.bin` disc image and write
    /// one WAV per `(file_no, ch_no)` channel. Solves the "Form 1
    /// truncation + multi-channel mux collapse" problem on Legaia's
    /// `XA*.XA` files (see `docs/formats/xa.md`).
    DemuxDisc {
        /// Path to the disc image (`.bin`, Mode 2 / 2352 raw sectors).
        bin: PathBuf,
        /// Starting LBA of the XA file on disc. Read it out of the
        /// ISO9660 directory entry; e.g. `XA1.XA` is at LBA 59449 on
        /// the NA Legaia disc.
        #[arg(long)]
        lba: u32,
        /// File size as reported by the directory entry (used to
        /// determine sector count).
        #[arg(long)]
        size: u32,
        /// Output directory; one WAV per channel lands here. The default is
        /// resolved against the current directory.
        #[arg(short, long, default_value = "extracted/xa_demux")]
        out: PathBuf,
        /// Optional name prefix for the output WAVs (default: derived
        /// from `--lba`).
        #[arg(long)]
        prefix: Option<String>,
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

#[derive(Clone, Copy, ValueEnum)]
enum BitsArg {
    #[value(name = "4")]
    Four,
    #[value(name = "8")]
    Eight,
}

impl From<BitsArg> for legaia_xa::BitsPerSample {
    fn from(a: BitsArg) -> Self {
        match a {
            BitsArg::Four => legaia_xa::BitsPerSample::Four,
            BitsArg::Eight => legaia_xa::BitsPerSample::Eight,
        }
    }
}

/// Rust ignores SIGPIPE by default; restore SIG_DFL so `xa ... | head`
/// exits quietly instead of panicking on a broken pipe.
fn reset_sigpipe() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

fn main() -> Result<()> {
    reset_sigpipe();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Info {
            path,
            channels,
            sample_rate,
            bits,
        } => info(&path, channels.into(), sample_rate, bits.into()),
        Cmd::Convert {
            path,
            out,
            channels,
            sample_rate,
            bits,
        } => convert(
            &path,
            out.as_deref(),
            channels.into(),
            sample_rate,
            bits.into(),
        ),
        Cmd::ConvertDir {
            dir,
            out,
            channels,
            sample_rate,
            bits,
        } => convert_dir(
            &dir,
            out.as_deref(),
            channels.into(),
            sample_rate,
            bits.into(),
        ),
        Cmd::DemuxDiscAll { bin, out } => demux_disc_all(&bin, &out),
        Cmd::DemuxDisc {
            bin,
            lba,
            size,
            out,
            prefix,
        } => demux_disc(&bin, lba, size, &out, prefix.as_deref()),
    }
}

/// Decode one demuxed channel stream to a WAV, mapping the stream's reported
/// `bits_per_sample` (4 or 8) to the decoder width. Streams of any other width
/// are skipped with a warning rather than mis-decoded; returns `false` when
/// skipped.
fn write_channel_wav(s: &legaia_xa::demux::ChannelStream, path: &Path) -> Result<bool> {
    let bits = match s.bits_per_sample {
        4 => legaia_xa::BitsPerSample::Four,
        8 => legaia_xa::BitsPerSample::Eight,
        other => {
            eprintln!(
                "  file={:<3} ch={:<3} SKIPPED: {other}-bit ADPCM unsupported (4-bit / 8-bit only)",
                s.file_no, s.ch_no
            );
            return Ok(false);
        }
    };
    let opts = DecodeOptions {
        channels: if s.stereo {
            Channels::Stereo
        } else {
            Channels::Mono
        },
        sample_rate: s.sample_rate,
        bits,
    };
    let (samples, report) = legaia_xa::decode(&s.audio, opts)?;
    legaia_xa::write_wav(path, &samples, opts.channels, opts.sample_rate)?;
    let dur = samples.len() as f64 / opts.sample_rate as f64 / opts.channels.n() as f64;
    println!(
        "  file={:<3} ch={:<3} {:>4} sectors {:>5} groups ({} skip) {:>5}Hz {:<6} {:.2}s -> {}",
        s.file_no,
        s.ch_no,
        s.sector_count,
        report.n_groups,
        report.n_groups_skipped,
        s.sample_rate,
        if s.stereo { "stereo" } else { "mono" },
        dur,
        path.display()
    );
    Ok(true)
}

fn demux_disc_all(bin: &Path, out_dir: &Path) -> Result<()> {
    let files = legaia_xa::demux::demux_disc_all(bin)
        .with_context(|| format!("demux all XA on {}", bin.display()))?;
    if files.is_empty() {
        bail!(
            "no .XA files found in the ISO9660 tree of {}",
            bin.display()
        );
    }
    std::fs::create_dir_all(out_dir)?;
    let mut total_channels = 0usize;
    for f in &files {
        // Output stem from the on-disc filename (e.g. `XA/XA1.XA` -> `XA1`):
        // basename with a single trailing extension removed.
        let base = f.path.rsplit('/').next().unwrap_or(&f.path);
        let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(base);
        let stem = if stem.is_empty() {
            format!("lba{}", f.start_lba)
        } else {
            stem.to_string()
        };
        println!(
            "{} (LBA {}): {} channel(s)",
            f.path,
            f.start_lba,
            f.streams.len()
        );
        for s in &f.streams {
            let path = out_dir.join(format!("{stem}_file{}_ch{}.wav", s.file_no, s.ch_no));
            if write_channel_wav(s, &path)? {
                total_channels += 1;
            }
        }
    }
    println!(
        "demuxed {} XA file(s), {} channel(s) written",
        files.len(),
        total_channels
    );
    Ok(())
}

fn demux_disc(bin: &Path, lba: u32, size: u32, out_dir: &Path, prefix: Option<&str>) -> Result<()> {
    let streams = legaia_xa::demux::demux_file(bin, lba, size)
        .with_context(|| format!("demux {} @ LBA {} size {}", bin.display(), lba, size))?;
    if streams.is_empty() {
        bail!(
            "no audio sectors found at LBA {} (size {} bytes) - not a CD-XA stream?",
            lba,
            size
        );
    }
    std::fs::create_dir_all(out_dir)?;
    let prefix = prefix
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("lba{lba}"));
    for s in &streams {
        let path = out_dir.join(format!("{prefix}_file{}_ch{}.wav", s.file_no, s.ch_no));
        write_channel_wav(s, &path)?;
    }
    Ok(())
}

fn info(
    path: &Path,
    channels: Channels,
    sample_rate: u32,
    bits: legaia_xa::BitsPerSample,
) -> Result<()> {
    let buf = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if buf.len() % legaia_xa::SOUND_GROUP_BYTES != 0 {
        bail!(
            "{}: size {} is not a multiple of 128 (XA sound group)",
            path.display(),
            buf.len()
        );
    }
    let n_groups = buf.len() / legaia_xa::SOUND_GROUP_BYTES;
    // SUs/group depends on the sample width (8 for 4-bit, 4 for 8-bit);
    // per-channel sample count = total / channels.
    let total_samples = n_groups * bits.units_per_group() * legaia_xa::SAMPLES_PER_UNIT;
    let per_channel = total_samples / channels.n() as usize;
    let dur_sec = per_channel as f64 / sample_rate as f64;
    println!("file:        {}", path.display());
    println!(
        "size:        {} bytes ({} sound groups)",
        buf.len(),
        n_groups
    );
    println!("channels:    {:?}", channels);
    println!("bits:        {:?}", bits);
    println!("sample_rate: {} Hz", sample_rate);
    println!(
        "duration:    {:.3} sec ({} samples per channel)",
        dur_sec, per_channel
    );
    Ok(())
}

fn convert(
    path: &Path,
    out: Option<&Path>,
    channels: Channels,
    sample_rate: u32,
    bits: legaia_xa::BitsPerSample,
) -> Result<()> {
    let buf = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let opts = DecodeOptions {
        channels,
        sample_rate,
        bits,
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

fn convert_dir(
    dir: &Path,
    out: Option<&Path>,
    channels: Channels,
    sample_rate: u32,
    bits: legaia_xa::BitsPerSample,
) -> Result<()> {
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
        match convert(p, Some(&target), channels, sample_rate, bits) {
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
