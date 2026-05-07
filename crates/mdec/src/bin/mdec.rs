//! `mdec` — CLI for PSX MDEC bitstream inspection and frame decoding.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use legaia_mdec::{MdecDecoder, str_sector::StrFrameAssembler};

#[derive(Parser)]
#[command(name = "mdec", about = "PSX MDEC bitstream tools")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Decode a raw MDEC BS payload file into a PNG.
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
        /// Output PNG path.
        #[arg(long, default_value = "frame.ppm")]
        out: PathBuf,
    },
    /// Scan a raw STR data file (2048-byte sectors, no subheaders) for video
    /// frames and report their dimensions and frame numbers.
    ScanStr {
        /// Path to a file containing raw 2048-byte STR sectors.
        #[arg()]
        str_file: PathBuf,
    },
    /// Decode frames from a raw STR data file (2048-byte sectors) and write
    /// each frame as a PPM image.
    DecodeStr {
        /// Path to a file containing raw 2048-byte STR sectors.
        #[arg()]
        str_file: PathBuf,
        /// Output directory for frame PPMs.
        #[arg(long, default_value = ".")]
        out_dir: PathBuf,
        /// Stop after decoding this many frames (0 = all).
        #[arg(long, default_value = "0")]
        max_frames: u32,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::DecodeFrame {
            bs_file,
            width,
            height,
            out,
        } => cmd_decode_frame(&bs_file, width, height, &out),
        Cmd::ScanStr { str_file } => cmd_scan_str(&str_file),
        Cmd::DecodeStr {
            str_file,
            out_dir,
            max_frames,
        } => cmd_decode_str(&str_file, &out_dir, max_frames),
    }
}

fn cmd_decode_frame(bs_file: &PathBuf, width: u32, height: u32, out: &PathBuf) -> Result<()> {
    let bs = std::fs::read(bs_file).with_context(|| format!("read {}", bs_file.display()))?;
    let dec = MdecDecoder::new(width, height);
    let rgba = dec
        .decode_frame(&bs)
        .with_context(|| format!("decode {}×{} frame", width, height))?;
    write_ppm(out, &rgba, width, height)?;
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
                "frame {:4}: {}×{}, qs={}, bs_ver={}",
                hdr.frame_number, hdr.width, hdr.height, hdr.quantize_scale, hdr.bs_version
            );
            frame_count += 1;
        }
    }
    println!("{} sectors, {} complete frames", n_sectors, frame_count);
    Ok(())
}

fn cmd_decode_str(str_file: &PathBuf, out_dir: &PathBuf, max_frames: u32) -> Result<()> {
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
                    let path = out_dir.join(format!("frame_{:04}.ppm", hdr.frame_number));
                    write_ppm(&path, &rgba, hdr.width as u32, hdr.height as u32)?;
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

/// Write an RGBA8 buffer as a PPM (portable pixmap) — no external image crate
/// needed.
fn write_ppm(path: &PathBuf, rgba: &[u8], width: u32, height: u32) -> Result<()> {
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
