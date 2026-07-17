//! `mdec` - CLI for PSX MDEC bitstream inspection and frame decoding.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use legaia_mdec::{MdecDecoder, str_sector::StrFrameAssembler};

#[derive(Parser)]
#[command(name = "mdec", version, about = "PSX MDEC bitstream tools")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

/// Output image container. PNG is the default; PPM (P6, no dependencies on
/// any image viewer plugin) is kept for pipelines built on the old default.
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ImageFormat {
    Png,
    Ppm,
}

impl ImageFormat {
    fn ext(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Ppm => "ppm",
        }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Decode a raw MDEC BS payload file into a single image.
    ///
    /// Input: a raw BS payload (no STR sector headers), e.g. sliced out of a
    /// movie by other tooling. For whole movies, use `decode-str` on a .STR
    /// from `legaia-extract <disc.bin> --out extracted` (extracted/MOV/).
    DecodeFrame {
        /// Path to a raw BS payload file (no STR sector headers).
        #[arg()]
        bs_file: PathBuf,
        /// Frame width in pixels (must be a multiple of 16).
        #[arg(long, default_value = "320")]
        width: u32,
        /// Frame height in pixels (must be a multiple of 16).
        #[arg(long, default_value = "240")]
        height: u32,
        /// Output image path. Default: `frame.png` (or `frame.ppm` with
        /// `--format ppm`), written to the current directory.
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Output image format (default: png).
        #[arg(long, value_enum, default_value_t = ImageFormat::Png)]
        format: ImageFormat,
    },
    /// Scan a raw STR data file (2048-byte sectors, no subheaders) for video
    /// frames and report their dimensions and frame numbers.
    ///
    /// Input: a .STR movie extracted by `legaia-extract <disc.bin> --out
    /// extracted` (see extracted/MOV/*.STR).
    ScanStr {
        /// Path to a file containing raw 2048-byte STR sectors.
        #[arg()]
        str_file: PathBuf,
    },
    /// Decode frames from a raw STR data file (2048-byte sectors) and write
    /// each frame as an image (`frame_<NNNN>.png` by default).
    ///
    /// Input: a .STR movie extracted by `legaia-extract <disc.bin> --out
    /// extracted` (see extracted/MOV/*.STR).
    DecodeStr {
        /// Path to a file containing raw 2048-byte STR sectors.
        #[arg()]
        str_file: PathBuf,
        /// Output directory for the frame images. Default: the current
        /// directory.
        #[arg(short = 'o', long, default_value = ".")]
        out_dir: PathBuf,
        /// Stop after decoding this many frames (0 = all).
        #[arg(long, default_value = "0")]
        max_frames: u32,
        /// Output image format (default: png).
        #[arg(long, value_enum, default_value_t = ImageFormat::Png)]
        format: ImageFormat,
    },
}

/// Rust ignores SIGPIPE by default; restore SIG_DFL so `mdec ... | head`
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
        Cmd::DecodeFrame {
            bs_file,
            width,
            height,
            out,
            format,
        } => cmd_decode_frame(&bs_file, width, height, out.as_deref(), format),
        Cmd::ScanStr { str_file } => cmd_scan_str(&str_file),
        Cmd::DecodeStr {
            str_file,
            out_dir,
            max_frames,
            format,
        } => cmd_decode_str(&str_file, &out_dir, max_frames, format),
    }
}

fn cmd_decode_frame(
    bs_file: &Path,
    width: u32,
    height: u32,
    out: Option<&Path>,
    format: ImageFormat,
) -> Result<()> {
    let bs = std::fs::read(bs_file).with_context(|| format!("read {}", bs_file.display()))?;
    let dec = MdecDecoder::new(width, height);
    let rgba = dec
        .decode_frame(&bs)
        .with_context(|| format!("decode {}×{} frame", width, height))?;
    let out: PathBuf = out
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(format!("frame.{}", format.ext())));
    write_image(&out, &rgba, width, height, format)?;
    println!("wrote {}×{} frame to {}", width, height, out.display());
    Ok(())
}

fn cmd_scan_str(str_file: &PathBuf) -> Result<()> {
    let data = std::fs::read(str_file).with_context(|| format!("read {}", str_file.display()))?;
    if data.len() % 2048 != 0 {
        eprintln!(
            "warning: file size {} is not a multiple of 2048",
            data.len()
        );
    }
    let n_sectors = data.len() / 2048;
    let mut asm = StrFrameAssembler::new();
    let mut frame_count = 0u32;
    for i in 0..n_sectors {
        let sector = &data[i * 2048..(i + 1) * 2048];
        if let Some((hdr, _bs)) = asm.push_sector(sector)? {
            println!(
                "frame {:4}: {}×{}, frame_size={}",
                hdr.frame_number, hdr.width, hdr.height, hdr.frame_size_bytes
            );
            frame_count += 1;
        }
    }
    let timing = legaia_mdec::str_sector::analyze_str_timing(&data);
    println!("{} sectors, {} complete frames", n_sectors, frame_count);
    println!(
        "{:.3} sectors/frame -> {:.2} fps (2x CD rate); duration {:.1}s",
        timing.sectors_per_frame,
        timing.fps,
        timing.frame_count as f64 * timing.frame_period().as_secs_f64()
    );
    Ok(())
}

fn cmd_decode_str(
    str_file: &PathBuf,
    out_dir: &PathBuf,
    max_frames: u32,
    format: ImageFormat,
) -> Result<()> {
    std::fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    let data = std::fs::read(str_file).with_context(|| format!("read {}", str_file.display()))?;
    let n_sectors = data.len() / 2048;
    let mut asm = StrFrameAssembler::new();
    let mut frame_count = 0u32;
    for i in 0..n_sectors {
        let sector = &data[i * 2048..(i + 1) * 2048];
        if let Some((hdr, bs)) = asm.push_sector(sector)? {
            let dec = MdecDecoder::new(hdr.width as u32, hdr.height as u32);
            match dec.decode_frame(&bs) {
                Ok(rgba) => {
                    let path =
                        out_dir.join(format!("frame_{:04}.{}", hdr.frame_number, format.ext()));
                    write_image(&path, &rgba, hdr.width as u32, hdr.height as u32, format)?;
                }
                Err(e) => eprintln!("frame {}: decode error: {}", hdr.frame_number, e),
            }
            frame_count += 1;
            if max_frames > 0 && frame_count >= max_frames {
                break;
            }
        }
    }
    println!("decoded {} frames to {}", frame_count, out_dir.display());
    Ok(())
}

fn write_image(
    path: &Path,
    rgba: &[u8],
    width: u32,
    height: u32,
    format: ImageFormat,
) -> Result<()> {
    match format {
        ImageFormat::Png => write_png(path, rgba, width, height),
        ImageFormat::Ppm => write_ppm(path, rgba, width, height),
    }
}

/// Write an RGBA8 buffer as an RGBA PNG.
fn write_png(path: &Path, rgba: &[u8], width: u32, height: u32) -> Result<()> {
    let f = std::io::BufWriter::new(
        std::fs::File::create(path).with_context(|| format!("create {}", path.display()))?,
    );
    let mut enc = png::Encoder::new(f, width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc
        .write_header()
        .with_context(|| format!("write PNG header {}", path.display()))?;
    writer
        .write_image_data(rgba)
        .with_context(|| format!("write PNG data {}", path.display()))?;
    Ok(())
}

/// Write an RGBA8 buffer as a PPM (portable pixmap) - no external image crate
/// needed.
fn write_ppm(path: &Path, rgba: &[u8], width: u32, height: u32) -> Result<()> {
    use std::io::Write;
    let mut f = std::io::BufWriter::new(
        std::fs::File::create(path).with_context(|| format!("create {}", path.display()))?,
    );
    write!(f, "P6\n{} {}\n255\n", width, height)?;
    for chunk in rgba.chunks_exact(4) {
        f.write_all(&chunk[..3])?; // RGB only
    }
    Ok(())
}
