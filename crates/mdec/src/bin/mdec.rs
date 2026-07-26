//! `mdec` - CLI for PSX MDEC bitstream inspection and frame decoding.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use legaia_mdec::{
    MdecDecoder,
    str_player::{Bitstream, FmvSlot, PumpIdle, StrPlayer, seek_sector_offset},
    str_sector::StrFrameAssembler,
    strv2_table,
};

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
    /// Unpack the STRv2/v3 VLC lookup table out of the STR/MDEC overlay
    /// (`FUN_801F1A00`) and report its shape.
    ///
    /// Input: the raw PROT 0970 entry - `asset overlay` writes it, or take
    /// `extracted/overlays/overlay_cutscene_str_0970.bin`. Retail movies are
    /// Iki-coded and never touch this table; it exists for the two dev slots.
    Strv2Table {
        /// Path to the raw STR/MDEC overlay image (PROT 0970).
        #[arg()]
        overlay: PathBuf,
        /// Load base of the overlay image.
        #[arg(long, value_parser = parse_u32, default_value = "0x801CE818")]
        base: u32,
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
        /// First frame of the segment (1-based, retail's dispatch-slot `+0x08`).
        /// 0 = start at the first frame in the file.
        ///
        /// One `MVn.STR` can carry several cutscenes as abutting frame ranges
        /// (`MV3.STR` carries four); this is the retail seek that picks one.
        #[arg(long, default_value = "0")]
        start_frame: u32,
        /// Last frame of the segment, inclusive (dispatch-slot `+0x0C`).
        /// 0 = decode to the end of the file.
        #[arg(long, default_value = "0")]
        end_frame: u32,
    },
    /// Print the **retail** decode plan for an STR segment: what
    /// `FUN_801CF098` would program the MDEC with, which VRAM rects it would
    /// decode into and display from, and the DMA-0 slice walk that fills them.
    ///
    /// Nothing is decoded. The port presents a frame as an RGBA texture and
    /// never writes MDEC registers or VRAM, so this is the one place the
    /// hardware-facing half of `str_player` can be read against a real movie -
    /// including the per-frame dimension override, which the play loop takes
    /// from the sector header rather than from the dispatch slot.
    StrPlan {
        /// Path to a file containing raw 2048-byte STR sectors.
        #[arg()]
        str_file: PathBuf,
        /// Dispatch-slot `+0x04`: the 24-bit colour flag. Off means 15-bit.
        #[arg(long)]
        colour: bool,
        /// Dispatch-slot `+0x10` / `+0x14`: the decode rect's VRAM origin.
        #[arg(long, default_value = "0")]
        fb_x: i16,
        /// Decode rect VRAM y.
        #[arg(long, default_value = "0")]
        fb_y: i16,
        /// Dispatch-slot `+0x08`: first frame of the segment (1-based).
        #[arg(long, default_value = "1")]
        start_frame: u32,
        /// Dispatch-slot `+0x0C`: last frame of the segment, inclusive.
        /// 0 = to the end of the file.
        #[arg(long, default_value = "0")]
        end_frame: u32,
        /// How many DMA-0 slice completions to walk. The walk stops early once
        /// the cursor has flipped buffers twice.
        #[arg(long, default_value = "24")]
        slices: usize,
    },
}

/// Accept `0x`-prefixed hex as well as decimal for address-shaped options.
fn parse_u32(s: &str) -> Result<u32, std::num::ParseIntError> {
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => s.parse(),
    }
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
            start_frame,
            end_frame,
        } => cmd_decode_str(
            &str_file,
            &out_dir,
            max_frames,
            format,
            start_frame,
            end_frame,
        ),
        Cmd::Strv2Table { overlay, base } => cmd_strv2_table(&overlay, base),
        Cmd::StrPlan {
            str_file,
            colour,
            fb_x,
            fb_y,
            start_frame,
            end_frame,
            slices,
        } => cmd_str_plan(
            &str_file,
            colour,
            fb_x,
            fb_y,
            start_frame,
            end_frame,
            slices,
        ),
    }
}

/// Print the retail decode plan for an STR segment. Decodes nothing.
#[allow(clippy::too_many_arguments)]
fn cmd_str_plan(
    str_file: &Path,
    colour: bool,
    fb_x: i16,
    fb_y: i16,
    start_frame: u32,
    end_frame: u32,
    slices: usize,
) -> Result<()> {
    use legaia_mdec::str_player::{
        Bitstream, FmvSlot, SLICE_W_16BPP, SLICE_W_24BPP, StrPlayer, end_of_stream,
        slice_word_count, vram_units,
    };

    let data = std::fs::read(str_file).with_context(|| format!("read {}", str_file.display()))?;
    let n_sectors = data.len() / 2048;

    // Frame geometry comes from the movie's own sector headers, which is what
    // the play loop's frame poll (`FUN_801CF740`) does too - the dispatch
    // slot's `+0x18`/`+0x1C` only seed the decode context.
    let mut asm = StrFrameAssembler::new();
    let mut first: Option<(u32, u32, u32)> = None;
    for i in 0..n_sectors {
        if let Some((hdr, _)) = asm.push_sector(&data[i * 2048..(i + 1) * 2048])? {
            first = Some((hdr.width as u32, hdr.height as u32, hdr.frame_number));
            break;
        }
    }
    let (hdr_w, hdr_h, first_frame) = first.context("no complete video frame in this file")?;

    let slot = FmvSlot {
        colour,
        start_frame,
        end_frame,
        fb_x: fb_x as u32,
        fb_y: fb_y as u32,
        width: hdr_w,
        height: hdr_h,
    };
    let mut player = StrPlayer::open(slot, Bitstream::Iki);

    println!("file:   {} ({n_sectors} sectors)", str_file.display());
    println!("header: first frame {first_frame}, {hdr_w}x{hdr_h}");
    println!(
        "slot:   colour {colour}, fb ({fb_x}, {fb_y}), frames {start_frame}..={}",
        if end_frame == 0 {
            "EOF".to_string()
        } else {
            end_frame.to_string()
        }
    );
    println!(
        "seek:   {} sectors from the stream origin ((start - 1) * 10)",
        player.seek_sector_offset()
    );
    println!(
        "vram:   {hdr_w} px -> {} cells; slice column {} cells",
        vram_units(hdr_w as i32, colour),
        if colour { SLICE_W_24BPP } else { SLICE_W_16BPP }
    );
    println!(
        "mdec:   control word {:#010x} from 0 (flags {})",
        player.mdec_control(0),
        if colour { 3 } else { 2 }
    );
    println!(
        "display rect (the buffer NOT being decoded into): {:?}",
        player.display_rect()
    );
    for (i, r) in player.env().frame_buf.iter().enumerate() {
        println!("  frame buf {i}: {r:?}");
    }
    println!("  slice rect:  {:?}", player.env().slice);

    // The per-frame override the play loop applies from the sector header. It
    // is a no-op whenever the header agrees with the slot, which is the retail
    // case; printing both is how a disagreement becomes visible.
    let before = *player.env();
    player
        .env_mut()
        .apply_frame_dimensions(hdr_w as u16, hdr_h as u16);
    if *player.env() == before {
        println!("frame-dimension override: no change (header agrees with the slot)");
    } else {
        println!("frame-dimension override: rects became");
        for (i, r) in player.env().frame_buf.iter().enumerate() {
            println!("  frame buf {i}: {r:?}");
        }
    }

    println!("DMA-0 slice walk ({slices} completion(s) at most):");
    let mut flips = 0;
    for i in 0..slices {
        let Some(step) = player.env_mut().advance_slice() else {
            println!("  slice callback is not armed");
            break;
        };
        let words = step
            .kick_words
            .map(|w| w.to_string())
            .unwrap_or_else(|| "-".into());
        println!(
            "  {i:3}  LoadImage {:?} from staging {}  next kick {words} words{}",
            step.load_rect,
            step.load_buffer,
            if step.flipped {
                "   <- buffer complete, rects flipped"
            } else {
                ""
            }
        );
        if step.flipped {
            flips += 1;
            if flips == 2 {
                break;
            }
        }
    }
    println!(
        "one full column of {hdr_h} rows is {} words",
        slice_word_count(
            if colour { SLICE_W_24BPP } else { SLICE_W_16BPP },
            hdr_h as i16
        )
    );
    if end_frame != 0 {
        println!(
            "end-of-stream latches at frame {end_frame} (first frame reaches it: {})",
            end_of_stream(first_frame, end_frame)
        );
    }
    Ok(())
}

/// Unpack the STRv2/v3 VLC table (`FUN_801F1A00`) and report its shape.
fn cmd_strv2_table(overlay_path: &Path, base: u32) -> Result<()> {
    let overlay =
        std::fs::read(overlay_path).with_context(|| format!("read {}", overlay_path.display()))?;
    let table = strv2_table::unpack_from_overlay(&overlay, base)
        .context("unpack the STRv2 VLC table (FUN_801F1A00)")?;
    let nonzero = table.iter().filter(|&&v| v != 0).count();
    println!(
        "packed at {:#010x} (overlay offset {:#x}) -> {} u16 entries ({} bytes) at {:#010x}",
        strv2_table::STRV2_PACKED_VA,
        strv2_table::STRV2_PACKED_VA - base,
        table.len(),
        table.len() * 2,
        strv2_table::STRV2_TABLE_VA
    );
    println!(
        "table ends at {:#010x}, flush against FUN_801F1A00; {} of {} entries non-zero",
        strv2_table::STRV2_TABLE_VA as usize + table.len() * 2,
        nonzero,
        table.len()
    );
    Ok(())
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
    start_frame: u32,
    end_frame: u32,
) -> Result<()> {
    std::fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    let data = std::fs::read(str_file).with_context(|| format!("read {}", str_file.display()))?;
    let n_sectors = data.len() / 2048;

    // Drive the retail play loop: the sector ring (`FUN_801CF988`) plus the
    // frame pump (`FUN_801CFA14`), so a `--start-frame` / `--end-frame` window
    // selects exactly the segment the FMV dispatch slot would.
    let slot = FmvSlot {
        colour: true,
        start_frame,
        end_frame,
        fb_x: 0,
        fb_y: 0,
        width: 320,
        height: 240,
    };
    let mut player = StrPlayer::open(slot, Bitstream::Iki);
    if start_frame != 0 {
        println!(
            "segment: frames {}..={} (retail seeks {} sectors in)",
            start_frame,
            if end_frame == 0 {
                "EOF".to_string()
            } else {
                end_frame.to_string()
            },
            seek_sector_offset(start_frame)
        );
    }

    let mut frame_count = 0u32;
    'sectors: for i in 0..n_sectors {
        player.deliver_sector(&data[i * 2048..(i + 1) * 2048]);
        loop {
            match player.next_frame() {
                Ok(frame) => {
                    let dec = MdecDecoder::new(slot.width, slot.height);
                    match dec.decode_frame(&frame.bitstream) {
                        Ok(rgba) => {
                            let path = out_dir.join(format!(
                                "frame_{:04}.{}",
                                frame.frame_number,
                                format.ext()
                            ));
                            write_image(&path, &rgba, slot.width, slot.height, format)?;
                        }
                        Err(e) => eprintln!("frame {}: decode error: {}", frame.frame_number, e),
                    }
                    frame_count += 1;
                    if max_frames > 0 && frame_count >= max_frames {
                        break 'sectors;
                    }
                }
                Err(PumpIdle::NeedSectors) => break,
                Err(PumpIdle::Finished) => break 'sectors,
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
