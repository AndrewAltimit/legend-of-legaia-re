//! The image channel of the retail comparison corpus: retail's displayed
//! frame out of a state's VRAM, the engine's frame out of `play-window`, and
//! the metric between them.
//!
//! # Alignment
//!
//! Retail draws a `320 x 224` area (the draw-environment pair
//! `FUN_8001DAF8` builds in its default-width / `0xE0`-height mode) into one
//! of two VRAM buffers and scans the display out from the same origin, so
//! the top `224` rows of the display crop are the drawn frame and the rest
//! is the unused tail of the `240`-line display. The engine renders the PSX
//! screen at an integer scale with the retail viewport map
//! (`ndc.y = 1 - 2 * screen_y / 240`, `engine-vm::battle_cam_script`), so
//! engine row `s * y` is retail row `y`: the comparison takes the engine
//! capture's top `224 * s` rows and box-filters them down by `s`.
//!
//! # Metric
//!
//! Two numbers, both over the aligned `320 x 224` pair:
//!
//! - `mae` - mean absolute error per channel per pixel, in 8-bit units;
//! - `within` - the fraction of `8 x 8` blocks whose mean colour is within
//!   [`BLOCK_TOLERANCE`] of retail's on every channel.
//!
//! `within` is the channel score. Block means rather than pixels because the
//! engine rasterises at its own resolution with its own filtering: a pixel
//! test would score dither, texture filtering and sub-pixel edges, while a
//! block test scores whether the same thing is in the same place in roughly
//! the same colour - wrong camera, missing geometry and wrong CLUTs all move
//! it, rasteriser noise does not.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// Width of the compared frame.
pub const FRAME_W: usize = 320;
/// Height of the compared frame (retail's draw area).
pub const FRAME_H: usize = 224;
/// Block edge for the `within` metric.
pub const BLOCK: usize = 8;
/// Per-channel tolerance on a block mean, 8-bit units.
pub const BLOCK_TOLERANCE: f64 = 24.0;
/// Mean luma below which a retail frame is a fade / black transition: the
/// image channel is not scored on it, since any engine frame that is also
/// dark would score as a match and any that is not would score as a miss,
/// and neither says anything about the scene.
pub const DARK_LUMA: f64 = 8.0;
/// World tick `play-window` captures at.
pub const CAPTURE_TICK: u64 = 120;

/// An RGB frame, `FRAME_W x FRAME_H`, 3 bytes per pixel.
#[derive(Clone)]
pub struct Frame {
    pub rgb: Vec<u8>,
}

impl Frame {
    /// Crop retail's drawn frame out of a 1 MiB VRAM image at the display
    /// rectangle's origin. `None` when the rectangle is not a `320`-wide
    /// display with room for the draw area.
    pub fn from_vram_display(vram: &[u8], rect: (u32, u32, u32, u32)) -> Option<Self> {
        let (x0, y0, w, h) = rect;
        if w as usize != FRAME_W || (h as usize) < FRAME_H || vram.len() != 1024 * 512 * 2 {
            return None;
        }
        let mut rgb = Vec::with_capacity(FRAME_W * FRAME_H * 3);
        for y in 0..FRAME_H {
            let vy = y0 as usize + y;
            if vy >= 512 {
                return None;
            }
            for x in 0..FRAME_W {
                let o = (vy * 1024 + x0 as usize + x) * 2;
                let px =
                    legaia_mednafen::bgr555_to_rgba8(u16::from_le_bytes([vram[o], vram[o + 1]]));
                rgb.extend_from_slice(&px[..3]);
            }
        }
        Some(Self { rgb })
    }

    /// Box-filter an engine capture (`width x height` RGBA) down to the
    /// compared frame, using its top `224 * scale` rows.
    pub fn from_engine_capture(rgba: &[u8], width: usize, height: usize) -> Result<Self> {
        if !width.is_multiple_of(FRAME_W) {
            bail!("engine capture width {width} is not a multiple of {FRAME_W}");
        }
        let s = width / FRAME_W;
        if height < FRAME_H * s {
            bail!(
                "engine capture {width}x{height} is shorter than {} rows",
                FRAME_H * s
            );
        }
        let mut rgb = Vec::with_capacity(FRAME_W * FRAME_H * 3);
        for y in 0..FRAME_H {
            for x in 0..FRAME_W {
                let mut acc = [0u32; 3];
                for dy in 0..s {
                    for dx in 0..s {
                        let o = ((y * s + dy) * width + x * s + dx) * 4;
                        for c in 0..3 {
                            acc[c] += u32::from(rgba[o + c]);
                        }
                    }
                }
                for a in acc {
                    rgb.push((a / (s * s) as u32) as u8);
                }
            }
        }
        Ok(Self { rgb })
    }

    fn px(&self, x: usize, y: usize) -> [u8; 3] {
        let o = (y * FRAME_W + x) * 3;
        [self.rgb[o], self.rgb[o + 1], self.rgb[o + 2]]
    }
}

/// The pixel comparison of one state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageScore {
    pub mae: f64,
    pub within: f64,
    /// Retail frame's mean luma, for spotting a black / faded retail frame.
    pub retail_luma: f64,
    pub engine_luma: f64,
    pub note: String,
}

/// Mean BT.601 luma of a frame.
pub fn luma(f: &Frame) -> f64 {
    let mut s = 0.0;
    for p in f.rgb.as_chunks::<3>().0 {
        s += 0.299 * f64::from(p[0]) + 0.587 * f64::from(p[1]) + 0.114 * f64::from(p[2]);
    }
    s / (FRAME_W * FRAME_H) as f64
}

/// Score an engine frame against retail's.
pub fn score(retail: &Frame, engine: &Frame) -> ImageScore {
    let mut abs = 0u64;
    for (a, b) in retail.rgb.iter().zip(&engine.rgb) {
        abs += u64::from(a.abs_diff(*b));
    }
    let mae = abs as f64 / retail.rgb.len() as f64;
    let (bw, bh) = (FRAME_W / BLOCK, FRAME_H / BLOCK);
    let mut ok = 0usize;
    for by in 0..bh {
        for bx in 0..bw {
            let mut sr = [0f64; 3];
            let mut se = [0f64; 3];
            for y in by * BLOCK..(by + 1) * BLOCK {
                for x in bx * BLOCK..(bx + 1) * BLOCK {
                    let (r, e) = (retail.px(x, y), engine.px(x, y));
                    for c in 0..3 {
                        sr[c] += f64::from(r[c]);
                        se[c] += f64::from(e[c]);
                    }
                }
            }
            let n = (BLOCK * BLOCK) as f64;
            if (0..3).all(|c| ((sr[c] - se[c]) / n).abs() <= BLOCK_TOLERANCE) {
                ok += 1;
            }
        }
    }
    let (rl, el) = (luma(retail), luma(engine));
    let note = if rl < DARK_LUMA {
        "retail frame is near-black".to_string()
    } else if el < DARK_LUMA {
        "engine frame is near-black".to_string()
    } else {
        String::new()
    };
    ImageScore {
        mae,
        within: ok as f64 / (bw * bh) as f64,
        retail_luma: rl,
        engine_luma: el,
        note,
    }
}

fn read_png_rgba(path: &Path) -> Result<(Vec<u8>, usize, usize)> {
    let dec = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?,
    ));
    let mut reader = dec.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf)?;
    buf.truncate(info.buffer_size());
    let (w, h) = (info.width as usize, info.height as usize);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        other => bail!("unsupported PNG colour type {other:?}"),
    };
    Ok((rgba, w, h))
}

/// How the `play-window` child enters the scene.
#[derive(Clone, Copy)]
pub enum FrameEntry<'a> {
    /// A card-load resume of the retail save (`--resume-save`), the order
    /// the headless channels seed with ([`crate::boot::BootSession::resume_save`]):
    /// the save's scene entered at the save's position with no free-roam
    /// picker staging, then the world hydrated from the save. Live NPCs
    /// stay on, as in the headless session: a placement's script runs only
    /// while its actor is engaged, so an idle resume holds the seat.
    Resume(&'a legaia_save::SaveFile),
    /// The `--scene` door entry, with these system-flag bits raised before
    /// it (`--set-flag`) so the entry scripts branch on retail's flags. The
    /// battle half falls back to it when a state's save window does not lift;
    /// otherwise it resumes like the field half and `--battle` forces the
    /// fight off the resumed scene.
    Door(&'a [u16]),
}

/// Render the engine's frame for a seated field state through `play-window`.
///
/// The child runs from a scratch directory under `out_dir` (the window
/// resolves its options file and save directory against its cwd), resumes
/// the retail save through `--resume-save` (a scratch LGSF file carrying
/// `scene` as its resume point), is seated on retail's `(x, z)` through
/// `LEGAIA_SEAT` - the same seat the headless channels take after their
/// resume - and captures at [`CAPTURE_TICK`]. `hud_countdown` is retail's
/// field party HUD countdown in the state (`_DAT_801F348C`), handed to the
/// child as `LEGAIA_HUD_COUNTDOWN` so the idle readout's phase matches the
/// retail instant rather than the settle window's length. `camera_block` is
/// the state's camera parameter block, handed over as
/// `LEGAIA_SEAT_CAMERA_BLOCK` so the seat's snap composes from it, and
/// `seat_focus` retail's focus pair when it is not on the player, handed
/// over as `LEGAIA_SEAT_FOCUS` (see `RetailObs::seat_focus`). `extra_env` is
/// passed through as is (`LEGAIA_SEAT_LATCHES`).
#[allow(clippy::too_many_arguments)]
pub fn engine_frame(
    exe: &Path,
    extracted: &Path,
    scene: &str,
    x: i16,
    z: i16,
    out_dir: Option<&Path>,
    label: &str,
    save: &legaia_save::SaveFile,
    hud_countdown: Option<i16>,
    camera_block: Option<&legaia_engine_core::camera_zone::CameraZoneConfig>,
    seat_focus: Option<[i32; 2]>,
    extra_env: &[(&str, String)],
) -> Result<Frame> {
    let env: Vec<(&str, String)> = hud_countdown
        .map(|n| ("LEGAIA_HUD_COUNTDOWN", n.to_string()))
        .into_iter()
        .chain(camera_block.map(|b| {
            (
                "LEGAIA_SEAT_CAMERA_BLOCK",
                crate::retail_compare::camera_block_env(b),
            )
        }))
        .chain(seat_focus.map(|[fx, fz]| ("LEGAIA_SEAT_FOCUS", format!("{fx},{fz}"))))
        .chain(extra_env.iter().cloned())
        .collect();
    engine_frame_with(
        exe,
        extracted,
        scene,
        Some((x, z)),
        &[],
        &env,
        CAPTURE_TICK,
        out_dir,
        label,
        FrameEntry::Resume(save),
    )
}

/// [`engine_frame`] with the seat optional, extra `play-window` arguments
/// (the battle half passes `--battle <row>` / `--party`), and the capture
/// tick and the entry chosen by the caller, plus extra environment for the
/// child (the battle half's `LEGAIA_BATTLE_STAGE`).
#[allow(clippy::too_many_arguments)]
pub fn engine_frame_with(
    exe: &Path,
    extracted: &Path,
    scene: &str,
    seat: Option<(i16, i16)>,
    extra: &[String],
    env: &[(&str, String)],
    tick: u64,
    out_dir: Option<&Path>,
    label: &str,
    entry: FrameEntry<'_>,
) -> Result<Frame> {
    let base: PathBuf = out_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::temp_dir().join("legaia-retail-compare"));
    let work = base.join("engine");
    std::fs::create_dir_all(&work)?;
    // The window's interactive default frames the field further out than
    // retail (`CameraDistance::Far`); the comparand is retail's own frame,
    // so the child reads a scratch options file pinning the retail vantage.
    // `reduce_flashing` is the photosensitivity slew on the ambient CLUT
    // cyclers, on by default and not retail: the comparand is retail's own
    // palette step.
    std::fs::write(
        work.join("legaia-options.toml"),
        "camera_distance = \"retail\"\nreduce_flashing = false\n",
    )?;
    let shot = work.join(format!("{label}.png"));
    let _ = std::fs::remove_file(&shot);
    let extracted = std::fs::canonicalize(extracted)?;
    let mut cmd = Command::new(exe);
    cmd.current_dir(&work);
    if let Some((x, z)) = seat {
        cmd.env("LEGAIA_SEAT", format!("{x},{z}"));
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    // The comparison is against retail: every presentation enhancement the
    // window defaults on stays off - the volumetric ground fog, enhanced
    // lighting, the camera-occlusion fade (which dissolves the walls around
    // a hidden player that retail draws opaque) and the scene-entry VDF
    // pulse retail never arms.
    cmd.args([
        "play-window",
        "--no-audio",
        "--no-volumetric-fog",
        "--no-dynamic-lighting",
        "--no-occlusion-fade",
        "--no-entry-pulse",
        "--scene",
        scene,
    ])
    .args(extra)
    .arg("--extracted-root")
    .arg(&extracted)
    .arg("--screenshot")
    .arg(&shot)
    .args(["--screenshot-tick", &tick.to_string()]);
    match entry {
        FrameEntry::Resume(save) => {
            let file = work.join(format!("{label}.lgsf"));
            let resume = legaia_save::SaveResume {
                scene: scene.to_string(),
                location: String::new(),
            };
            std::fs::write(&file, save.write_with_resume(&resume))
                .with_context(|| format!("write {}", file.display()))?;
            cmd.arg("--resume-save").arg(&file);
        }
        FrameEntry::Door(flags) => {
            cmd.args(
                flags
                    .iter()
                    .flat_map(|f| ["--set-flag".to_string(), f.to_string()]),
            );
        }
    }
    let out = cmd.output().context("spawn play-window")?;
    // `LEGAIA_RC_CHILD_LOG=1` keeps the child's stderr beside its frame, for
    // the trace hooks the capture gates carry, and the child's environment
    // and arguments beside that, so a frame can be re-run by hand.
    if std::env::var_os("LEGAIA_RC_CHILD_LOG").is_some() {
        let _ = std::fs::write(shot.with_extension("stderr.log"), &out.stderr);
        let mut line = format!("cd '{}' && ", work.display());
        for (k, v) in cmd.get_envs() {
            if let Some(v) = v {
                line.push_str(&format!(
                    "{}='{}' ",
                    k.to_string_lossy(),
                    v.to_string_lossy()
                ));
            }
        }
        line.push_str(&exe.display().to_string());
        for a in cmd.get_args() {
            line.push_str(&format!(" '{}'", a.to_string_lossy()));
        }
        line.push('\n');
        let _ = std::fs::write(shot.with_extension("cmd.sh"), line);
    }
    if !shot.exists() {
        let tail: String = String::from_utf8_lossy(&out.stderr)
            .lines()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .join(" | ");
        bail!(
            "play-window wrote no screenshot (status {}): {tail}",
            out.status
        );
    }
    let (rgba, w, h) = read_png_rgba(&shot)?;
    Frame::from_engine_capture(&rgba, w, h)
}

/// Write `retail | engine | |diff|` as one PNG for a human reader.
pub fn write_side_by_side(path: &Path, retail: &Frame, engine: &Frame) -> Result<()> {
    let w = FRAME_W * 3;
    let mut rgb = Vec::with_capacity(w * FRAME_H * 3);
    for y in 0..FRAME_H {
        for x in 0..FRAME_W {
            rgb.extend_from_slice(&retail.px(x, y));
        }
        for x in 0..FRAME_W {
            rgb.extend_from_slice(&engine.px(x, y));
        }
        for x in 0..FRAME_W {
            let (r, e) = (retail.px(x, y), engine.px(x, y));
            let d = (0..3).map(|c| r[c].abs_diff(e[c])).max().unwrap_or(0);
            let v = d.saturating_mul(2);
            rgb.extend_from_slice(&[v, v, v]);
        }
    }
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, FRAME_H as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&rgb)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(v: u8) -> Frame {
        Frame {
            rgb: vec![v; FRAME_W * FRAME_H * 3],
        }
    }

    #[test]
    fn identical_frames_score_one() {
        let s = score(&solid(100), &solid(100));
        assert_eq!(s.within, 1.0);
        assert_eq!(s.mae, 0.0);
    }

    #[test]
    fn the_block_tolerance_is_inclusive_and_bounded() {
        assert_eq!(score(&solid(100), &solid(124)).within, 1.0);
        assert_eq!(score(&solid(100), &solid(125)).within, 0.0);
    }

    #[test]
    fn engine_capture_box_filters_by_the_integer_scale() {
        // A 3x capture whose top 672 rows are grey 90 and whose tail is
        // white: the tail is outside retail's draw area and must not leak.
        let (w, h) = (960usize, 720usize);
        let mut rgba = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let o = (y * w + x) * 4;
                let v = if y < 672 { 90 } else { 255 };
                rgba[o..o + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        let f = Frame::from_engine_capture(&rgba, w, h).unwrap();
        assert!(f.rgb.iter().all(|&v| v == 90));
    }

    #[test]
    fn vram_crop_takes_the_draw_area_at_the_display_origin() {
        let mut vram = vec![0u8; 1024 * 512 * 2];
        // Pure red (r5 = 31) at the display origin (0, 244).
        let o = (244 * 1024) * 2;
        vram[o..o + 2].copy_from_slice(&0x001Fu16.to_le_bytes());
        let f = Frame::from_vram_display(&vram, (0, 244, 320, 240)).unwrap();
        assert_eq!(f.px(0, 0), [255, 0, 0]);
        assert!(Frame::from_vram_display(&vram, (0, 4, 256, 240)).is_none());
    }
}
