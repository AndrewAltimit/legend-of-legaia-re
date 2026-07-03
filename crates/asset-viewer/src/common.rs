//! Shared helpers used across the viewer's per-mode apps: the per-actor
//! mesh wrapper, AABB math, path shortening, PSX pad keymapping, and the
//! scene-directory collectors.

use legaia_engine_core::input::PadButton;
use std::path::{Path, PathBuf};
use winit::keyboard::KeyCode;

pub(crate) struct WorldActorMesh {
    pub(crate) mesh: legaia_engine_render::UploadedVramMesh,
    /// AABB of the local TMD geometry (pre-transform). Used to size the
    /// camera frustum.
    pub(crate) aabb_lo: [f32; 3],
    pub(crate) aabb_hi: [f32; 3],
}

pub(crate) fn mesh_aabb(positions: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    if positions.is_empty() {
        return ([0.0; 3], [0.0; 3]);
    }
    let mut lo = positions[0];
    let mut hi = positions[0];
    for p in &positions[1..] {
        for i in 0..3 {
            if p[i] < lo[i] {
                lo[i] = p[i];
            }
            if p[i] > hi[i] {
                hi[i] = p[i];
            }
        }
    }
    (lo, hi)
}

/// Trim a path for window-title use: keep the last 3 components.
pub(crate) fn short_path(p: &Path) -> String {
    let comps: Vec<_> = p.components().collect();
    let take = comps.len().min(3);
    let tail = &comps[comps.len() - take..];
    let mut s = String::new();
    for (i, c) in tail.iter().enumerate() {
        if i > 0 {
            s.push('/');
        }
        s.push_str(&c.as_os_str().to_string_lossy());
    }
    s
}

/// Map winit physical keys to PSX pad button bits. Keyboard mapping mirrors
/// the conventional emulator default:
///
/// - Arrows → D-pad
/// - Z → Cross, X → Square, A → Triangle, S → Circle
/// - Enter → Start, Right Shift → Select
/// - Q / W → L1 / R1, 1 / 2 → L2 / R2
pub(crate) fn keymap_pad(code: KeyCode) -> Option<PadButton> {
    Some(match code {
        KeyCode::ArrowUp => PadButton::Up,
        KeyCode::ArrowDown => PadButton::Down,
        KeyCode::ArrowLeft => PadButton::Left,
        KeyCode::ArrowRight => PadButton::Right,
        KeyCode::KeyZ => PadButton::Cross,
        KeyCode::KeyX => PadButton::Square,
        KeyCode::KeyA => PadButton::Triangle,
        KeyCode::KeyS => PadButton::Circle,
        KeyCode::Enter => PadButton::Start,
        KeyCode::ShiftRight => PadButton::Select,
        KeyCode::KeyQ => PadButton::L1,
        KeyCode::KeyW => PadButton::R1,
        KeyCode::Digit1 => PadButton::L2,
        KeyCode::Digit2 => PadButton::R2,
        _ => return None,
    })
}

/// Friendly button-name string for HUD readouts. Returns `"_"` for
/// unset bits so the readout has a fixed grid shape.
pub(crate) fn pad_button_label(b: PadButton) -> &'static str {
    match b {
        PadButton::Up => "U",
        PadButton::Down => "D",
        PadButton::Left => "L",
        PadButton::Right => "R",
        PadButton::Cross => "X",
        PadButton::Circle => "O",
        PadButton::Square => "[]",
        PadButton::Triangle => "/\\",
        PadButton::Start => "ST",
        PadButton::Select => "SE",
        PadButton::L1 => "L1",
        PadButton::R1 => "R1",
        PadButton::L2 => "L2",
        PadButton::R2 => "R2",
        PadButton::L3 => "L3",
        PadButton::R3 => "R3",
    }
}

/// Collect every `tmd_scan/<NNNN_label>/*.tmd` for entries within
/// `[start, end)`. Returns paths sorted by entry index then filename so the
/// per-actor mesh assignment is deterministic.
pub(crate) fn collect_scene_tmds(extracted_root: &Path, start: u32, end: u32) -> Vec<PathBuf> {
    let tmd_root = extracted_root.join("tmd_scan");
    let Ok(rd) = std::fs::read_dir(&tmd_root) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in rd.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let Some(name) = p.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(idx) = parse_prot_dir_index(name) else {
            continue;
        };
        if idx < start || idx >= end {
            continue;
        }
        let Ok(inner) = std::fs::read_dir(&p) else {
            continue;
        };
        for ent in inner.flatten() {
            let q = ent.path();
            if q.extension().is_some_and(|e| e.eq_ignore_ascii_case("tmd")) {
                paths.push(q);
            }
        }
    }
    paths.sort();
    paths
}

/// Collect every `tim_scan/<NNNN_label>/` directory for entries in the
/// scene block. Used to populate the shared VRAM.
pub(crate) fn collect_scene_tim_dirs(extracted_root: &Path, start: u32, end: u32) -> Vec<PathBuf> {
    let tim_root = extracted_root.join("tim_scan");
    let Ok(rd) = std::fs::read_dir(&tim_root) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = Vec::new();
    for entry in rd.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let Some(name) = p.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(idx) = parse_prot_dir_index(name) else {
            continue;
        };
        if idx < start || idx >= end {
            continue;
        }
        dirs.push(p);
    }
    dirs.sort();
    dirs
}

/// Parse the leading `NNNN_` index from `tim_scan/0123_label/` etc.
fn parse_prot_dir_index(name: &str) -> Option<u32> {
    let (lead, _) = name.split_once('_')?;
    lead.parse().ok()
}

/// Recursively collect every `*.tmd` file (case-insensitive) under `root`.
/// Symlinks are not followed; unreadable subdirectories are skipped silently.
pub(crate) fn collect_tmds(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                walk(&path, out);
            } else if ft.is_file()
                && path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case("tmd"))
            {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}
