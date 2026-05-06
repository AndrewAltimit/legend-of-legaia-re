//! Phase 1 asset viewer.
//!
//! Modes:
//!
//! * `tim <PATH>` — display a single TIM file (the original Phase 1 demo).
//! * `tmd <PATH>` — display a Legaia TMD as a flat-shaded auto-rotating
//!   3D mesh. PATH may be a single `.tmd` file, or a directory in which
//!   case the viewer walks every `*.tmd` recursively and N/P/PgDn/PgUp
//!   cycle between them.
//! * `stage <PATH>` — render a stage-geometry PROT entry as a wireframe.
//! * `vab <PATH>` — load a VAB bank, play one sample.
//! * `prot <PROT.DAT>` — browse every PROT entry. Auto-detects what the
//!   entry contains via the categorize classifier and shows / plays the
//!   first viewable sub-asset. Currently handles:
//!   - `tim_passthrough` → display the TIM
//!   - `tim_pack` → display first sub-TIM that decodes
//!   - `data_field_streaming` → display first TIM_LIST sub-pack TIM
//!   - `scene_tmd_stream` → render the leading TMD as flat-shaded mesh
//!     (148 PROT entries, 2026-05-04)
//!   - `scene_vab_stream` → play sample 0 of the leading VAB bank
//!     (217 PROT entries, 2026-05-04)
//!   - VAB byte-search fallback for any class — finds VAB headers anywhere
//!     in the buffer (covers `unknown_*` entries with embedded banks)
//!
//! The PROT browser is the integration de-risk for Phase 1: proves the
//! engine plumbing handles classification → typed parse → asset upload
//! across a real disc dump, not just one hand-picked file.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use legaia_engine_audio::{AudioOut, DEFAULT_INPUT_RATE, Sequencer};
use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_render::{
    RenderTarget, Renderer, Scene as RenderScene, SceneDraw, TextDraw, TextOverlay,
    UploadedFontAtlas, UploadedLines, UploadedMesh, UploadedTexture, UploadedVram,
    UploadedVramMesh,
    glam::{Mat4, Vec3},
    legaia_tim::Vram,
};
use legaia_font::Font;
use legaia_prot::{archive::Archive, cdname};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

#[derive(Parser, Debug)]
#[command(about = "Asset viewer for Legend of Legaia.")]
struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Display a single TIM in a window.
    Tim {
        path: PathBuf,
        /// CLUT index to use (0 by default, ignored for direct-color TIMs).
        #[arg(long, default_value_t = 0)]
        clut: usize,
    },
    /// Play one VAG sample from a VAB bank.
    Vab {
        path: PathBuf,
        /// Sample index within the bank (0 = first).
        #[arg(long, default_value_t = 0)]
        sample: usize,
        /// Byte offset into `path` where the VAB header lives. Use this to
        /// pick a VAB embedded inside a PROT entry (run `vab list <path>`
        /// first to find the offset).
        #[arg(long, value_parser = parse_hex_u64, default_value_t = 0)]
        offset: u64,
        /// Sample rate to play at. PSX VAGs in Legaia are typically 22050.
        #[arg(long, default_value_t = DEFAULT_INPUT_RATE)]
        rate: u32,
    },
    /// Play a PsyQ SEQ file through the SsAPI-shape sequencer + a VAB bank.
    /// The viewer keeps a status window open showing tick / BPM / active-note
    /// counts while playback runs (Esc / window-close to stop).
    Seq {
        /// Path to the SEQ file (begins with `pQES`).
        seq: PathBuf,
        /// Path to the VAB bank (or any container with a VAB header at
        /// `--vab-offset`).
        vab: PathBuf,
        /// Byte offset of the VAB header inside `vab`. Use this to pick a
        /// bank embedded in a PROT entry; default 0 for a standalone bank.
        #[arg(long, value_parser = parse_hex_u64, default_value_t = 0)]
        vab_offset: u64,
        /// Loop the sequence at end-of-track (rewinds to event 0).
        #[arg(long, default_value_t = false)]
        looped: bool,
        /// Master sequencer volume, 0..=127.
        #[arg(long, default_value_t = 100)]
        master_vol: u8,
    },
    /// Render a stage-geometry PROT entry as a wireframe. PATH may be a
    /// single PROT-entry file (e.g. `extracted/PROT/0000_init_data.BIN`),
    /// or a directory in which case every file is scanned and N/P/PgDn/PgUp
    /// cycle through the ones that contain a stage table.
    Stage {
        path: PathBuf,
        /// Start at this index when walking a directory.
        #[arg(long, default_value_t = 0)]
        start: usize,
    },
    /// Browse every entry in a PROT.DAT archive.
    Prot {
        prot_dat: PathBuf,
        /// CDNAME.TXT for nicer entry titles. Optional.
        #[arg(long)]
        cdname: Option<PathBuf>,
        /// Start at this entry index.
        #[arg(long, default_value_t = 0)]
        start: u32,
    },
    /// Display a Legaia TMD as a flat-shaded, auto-rotating 3D mesh.
    /// PATH may be a single `.tmd` file, or a directory in which case the
    /// viewer walks every `*.tmd` recursively and N/P/PgDn/PgUp navigate
    /// between them.
    Tmd {
        path: PathBuf,
        /// Start at this index when walking a directory.
        #[arg(long, default_value_t = 0)]
        start: usize,
        /// Skip files smaller than this many bytes (directory mode only).
        /// Useful for filtering out scenery/effect props and only browsing
        /// character-scale meshes — try `--min-size 15000` for `battle_data`.
        #[arg(long, default_value_t = 0)]
        min_size: u64,
        /// Sort by file size descending (largest first) instead of by path.
        /// Pairs well with `--min-size` to surface main characters first.
        #[arg(long, default_value_t = false)]
        sort_by_size: bool,
        /// Filter directory listing by AABB shape (`character` = tall meshes
        /// w/ height > 1.5 × horizontal extent — Legaia heroes/NPCs;
        /// `arena` = wide flat meshes w/ height < 0.5 × horizontal extent —
        /// battle stages; `any` = no filter). Cheaper than `--min-size` for
        /// finding actual characters since size alone doesn't distinguish
        /// hero meshes from arena props.
        #[arg(long, value_enum, default_value_t = ShapeFilter::Any)]
        shape: ShapeFilter,
        /// Extra TIM directory to overlay into VRAM, before the mesh's own
        /// sibling tim_scan dir. Use this when a mesh's prims reference
        /// CLUT rows that live in a different PROT entry — e.g. level_up
        /// character meshes share their CLUTs with battle_data, so:
        ///   `--vram-extra-dir extracted/tim_scan/0866_battle_data`
        /// Repeatable. Order matters: each dir's TIMs overwrite earlier ones
        /// at overlapping VRAM addresses (matches PSX hardware DMA).
        #[arg(long)]
        vram_extra_dir: Vec<PathBuf>,
        /// Pre-load a known scene bundle into VRAM. Encodes the PROT entry
        /// sets that the runtime asset loader (`FUN_800520f0` + relatives in
        /// SCUS_942.54) loads together for a given scene. Use `battle` for
        /// any mesh in `level_up`, `battle_data`, or `monster_se` — these
        /// share the same character-body palette set.
        ///
        /// Acts as a shorthand for several `--vram-extra-dir` flags. Bundle
        /// dirs are loaded BEFORE explicit `--vram-extra-dir` paths.
        #[arg(long, value_enum)]
        bundle: Option<Bundle>,
        /// CDNAME-resolved per-scene bundle. Adds tim_scan dirs for every
        /// PROT entry in the named CDNAME block — mirrors what the runtime
        /// field/town loader (`FUN_8001f7c0` + `FUN_800255b8` in
        /// SCUS_942.54) co-loads for a scene. Use the CDNAME block name
        /// e.g. `--scene town01` / `--scene dolk` / `--scene cave01`.
        ///
        /// Requires `--cdname` (or a default CDNAME at the canonical
        /// location under `--extracted-root`).
        #[arg(long)]
        scene: Option<String>,
        /// CDNAME.TXT path for `--scene`. Defaults to
        /// `<extracted-root>/CDNAME.TXT`.
        #[arg(long)]
        cdname: Option<PathBuf>,
        /// Root path containing the `tim_scan/` directory tree. Bundles
        /// resolve their PROT-entry paths under this root. Defaults to
        /// `extracted/`.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
    },
    /// Boot a CDNAME scene as a playable field-mode demo: opens a window,
    /// uploads the scene's TMDs as actors, wires keyboard / gamepad input
    /// through `engine-core::input`, and overlays HUD text rendered via the
    /// extracted dialog font. The field VM ticks each frame against
    /// whatever bytecode is loaded (none by default — overlay-resident
    /// scripts aren't statically extractable yet, so the VM no-ops).
    ///
    /// First milestone toward the "playable demo" path: proves the engine
    /// layers (input + scene + render + text) compose end-to-end.
    Field {
        /// CDNAME scene name (e.g. `town01`, `dolk`, `cave01`).
        scene: String,
        /// Maximum actors to spawn. Capped at the number of distinct TMDs
        /// the scene's tmd_scan dir provides, then at
        /// `engine-core::world::MAX_ACTORS` (64).
        #[arg(long, default_value_t = 6)]
        max_actors: usize,
        /// `extracted/` root containing PROT.DAT + CDNAME.TXT + tmd_scan/
        /// + tim_scan/ + font/.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
    },
    /// Render a MES dialog blob: walks the bytecode interpreter
    /// (`legaia_mes::Interpreter`) through `DialogPlayer`, accumulates
    /// the emitted glyph bytes into a typewriter-paced page, lays them
    /// out via the extracted dialog font, and blits one quad per glyph
    /// into a centered dialog box. Cross (Z) advances past page breaks;
    /// Esc quits. First milestone for the MES path.
    Dialog {
        /// Path to the MES container (compact format with offset table).
        path: PathBuf,
        /// Message index inside the offset table (0-based).
        #[arg(long, default_value_t = 0)]
        message: usize,
        /// `extracted/` root containing `font/` artifacts produced by
        /// `font-extract`.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Typewriter pacing: frames per emitted glyph. 1 = fastest
        /// (one glyph per frame); higher values slow the page reveal.
        #[arg(long, default_value_t = 2)]
        glyphs_per_frame: u8,
    },
    /// Run the engine-core `World` composite over a CDNAME scene with N
    /// actors backed by TMDs from the scene's tmd_scan dir, rendered via
    /// the multi-actor pipeline. Each actor orbits its origin so the demo
    /// makes the World tick visible.
    World {
        /// CDNAME scene name (e.g. `town01`, `dolk`, `cave01`). The
        /// matching CDNAME block range is loaded via engine-core's
        /// `Scene::load`.
        scene: String,
        /// Number of actors to spawn. Capped at the number of distinct
        /// TMDs found in the scene's `tmd_scan/` dir, then at
        /// `engine-core::world::MAX_ACTORS` (64).
        #[arg(long, default_value_t = 6)]
        max_actors: usize,
        /// `extracted/` root containing PROT.DAT + CDNAME.TXT + tmd_scan/
        /// + tim_scan/.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Run the move VM with synthetic bytecode each frame (every actor
        /// gets `WORLD_SET → WAIT_SET → HALT`). Off by default; the demo
        /// just animates positions analytically through `World::tick`.
        #[arg(long, default_value_t = false)]
        with_move_vm: bool,
    },
}

/// Known scene-loader bundles, derived from static analysis of asset
/// loaders in SCUS_942.54 (see `docs/REVERSING.md` and the dump of
/// `FUN_800520f0` for the battle path).
#[derive(Copy, Clone, Debug, ValueEnum)]
enum Bundle {
    /// Battle / level_up / monster_se assets. Mirrors what `FUN_800520f0`
    /// loads at battle-scene init: sound_data + befect_data + player_data +
    /// sound_data2 (PROT entries 871-890). Includes the CLUT rows shared
    /// across all character bodies (e.g. row 484 lives in `0873_befect_data`).
    Battle,
}

impl Bundle {
    /// Return the `tim_scan/<entry>/` directories this bundle overlays.
    /// Skips entries that don't exist on disk.
    fn dirs(self, root: &Path) -> Vec<PathBuf> {
        let entries: &[&str] = match self {
            // CDNAME blocks 865-890. FUN_800520f0 explicitly loads PROT
            // 0x367-0x36B (sound_data + befect_data) via etim.dat/etmd.dat/
            // vdf.dat dev paths and 0x36C (player_data) via the player
            // loader. battle_data (865-868) and monster_data (869) are
            // adjacent character-facing blocks that share the same VRAM
            // CLUT/sprite slots — observed empirically that level_up hero
            // meshes need CLUTs from both 0866_battle_data (row 490) and
            // 0873_befect_data (row 484, x=144).
            Bundle::Battle => &[
                "0865_battle_data",
                "0866_battle_data",
                "0867_battle_data",
                "0868_battle_data",
                "0869_monster_data",
                "0870_sound_data",
                "0871_sound_data",
                "0872_befect_data",
                "0873_befect_data",
                "0874_befect_data",
                "0875_befect_data",
                "0876_player_data",
                "0877_sound_data2",
                "0878_sound_data2",
                "0879_sound_data2",
                "0880_sound_data2",
                "0881_sound_data2",
                "0882_sound_data2",
                "0883_sound_data2",
                "0884_sound_data2",
                "0885_sound_data2",
                "0886_sound_data2",
                "0887_sound_data2",
                "0888_sound_data2",
                "0889_sound_data2",
                "0890_sound_data2",
            ],
        };
        let tim_root = root.join("tim_scan");
        entries
            .iter()
            .map(|e| tim_root.join(e))
            .filter(|p| p.is_dir())
            .collect()
    }
}

/// Resolve a CDNAME scene name to the list of `tim_scan/<entry>/` dirs
/// for every PROT entry in that block. Mirrors what the runtime field
/// loader (`FUN_8001f7c0` + `FUN_800255b8`) co-loads for one scene —
/// six file types per scene, all under the same CDNAME block.
///
/// Walks the `tim_scan/` directory tree to discover the actual entry
/// folder names (which include the index prefix, e.g. `0006_town01`).
/// Skips PROT indices in the block range that don't have a tim_scan dir
/// (e.g. stage-only entries with no TIMs).
fn scene_bundle_dirs(
    cdname_path: &Path,
    scene_name: &str,
    extracted_root: &Path,
) -> Result<Vec<PathBuf>> {
    let map = cdname::parse(cdname_path)
        .with_context(|| format!("parse CDNAME at {}", cdname_path.display()))?;
    let (start, end) = cdname::block_range_for_name(&map, scene_name).ok_or_else(|| {
        anyhow::anyhow!(
            "scene '{}' not found in CDNAME at {}",
            scene_name,
            cdname_path.display()
        )
    })?;
    // tim_scan dir names look like "<NNNN>_<scene>". Walk the dir and
    // pick entries whose numeric prefix falls in [start, end).
    let tim_root = extracted_root.join("tim_scan");
    let mut dirs: Vec<PathBuf> = Vec::new();
    let Ok(rd) = std::fs::read_dir(&tim_root) else {
        anyhow::bail!(
            "no tim_scan dir at {} — run `asset tim-scan` first",
            tim_root.display()
        );
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let Some(stem) = p.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some((num_str, _)) = stem.split_once('_') else {
            continue;
        };
        let Ok(idx) = num_str.parse::<u32>() else {
            continue;
        };
        if idx >= start && idx < end {
            dirs.push(p);
        }
    }
    dirs.sort();
    Ok(dirs)
}

/// Shape-based AABB filter for directory navigation.
#[derive(Copy, Clone, Debug, ValueEnum)]
enum ShapeFilter {
    /// No shape filter applied — every `*.tmd` file under the dir is included.
    Any,
    /// Tall meshes (height > 1.5 × max horizontal extent). Hero/NPC bodies.
    Character,
    /// Wide flat meshes (height < 0.5 × max horizontal extent). Battle arenas
    /// and floor pieces.
    Arena,
}

impl ShapeFilter {
    /// Decide whether `path` matches this filter. `Any` always returns true;
    /// other variants parse the TMD and check its AABB aspect.
    fn accepts(self, path: &Path) -> bool {
        match self {
            ShapeFilter::Any => true,
            ShapeFilter::Character | ShapeFilter::Arena => {
                let Ok(bytes) = std::fs::read(path) else {
                    return false;
                };
                let Ok(parsed) = legaia_tmd::parse(&bytes) else {
                    return false;
                };
                let Some(aabb) = tmd_aabb(&parsed) else {
                    return false;
                };
                let w = aabb.1[0] - aabb.0[0];
                let h = (aabb.1[1] - aabb.0[1]).abs();
                let d = aabb.1[2] - aabb.0[2];
                let horizontal = w.max(d).max(1.0);
                let aspect = h.max(1.0) / horizontal;
                match self {
                    ShapeFilter::Character => aspect > 1.5,
                    ShapeFilter::Arena => aspect < 0.5,
                    ShapeFilter::Any => true,
                }
            }
        }
    }
}

fn tmd_aabb(parsed: &legaia_tmd::Tmd) -> Option<([f32; 3], [f32; 3])> {
    let mut iter = parsed.objects.iter().flat_map(|o| o.vertices.iter());
    let first = iter.next()?;
    let mut lo = [first.x as f32, first.y as f32, first.z as f32];
    let mut hi = lo;
    for v in iter {
        let p = [v.x as f32, v.y as f32, v.z as f32];
        for i in 0..3 {
            if p[i] < lo[i] {
                lo[i] = p[i];
            }
            if p[i] > hi[i] {
                hi[i] = p[i];
            }
        }
    }
    Some((lo, hi))
}

fn load_tim(bytes: &[u8], clut_idx: usize) -> Result<(Vec<u8>, u32, u32)> {
    let tim = legaia_tim::parse(bytes).context("parse TIM")?;
    let rgba = legaia_tim::decode_rgba8(&tim, clut_idx).context("decode TIM to RGBA")?;
    Ok((rgba, tim.pixel_width() as u32, tim.pixel_height() as u32))
}

fn load_tim_path(path: &Path, clut_idx: usize) -> Result<(Vec<u8>, u32, u32)> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    load_tim(&bytes, clut_idx)
}

/// Decode VAG sample `idx` from a VAB header located at `offset` in `path`.
/// Returns mono i16 PCM.
fn load_vab_sample(path: &Path, offset: usize, idx: usize) -> Result<Vec<i16>> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    decode_vab_sample(&bytes, offset, idx)
}

fn decode_vab_sample(bytes: &[u8], offset: usize, idx: usize) -> Result<Vec<i16>> {
    let report = legaia_vab::parse(bytes, offset).context("parse VAB")?;
    let span = report
        .vag_samples
        .get(idx)
        .ok_or_else(|| anyhow::anyhow!("VAB has only {} samples", report.vag_samples.len()))?;
    let body = &bytes[span.byte_offset..span.byte_offset + span.size];
    legaia_vab::decode_vag(body).context("decode VAG body")
}

fn parse_hex_u64(s: &str) -> std::result::Result<u64, String> {
    if let Some(stripped) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(stripped, 16).map_err(|e| e.to_string())
    } else {
        s.parse::<u64>().map_err(|e| e.to_string())
    }
}

/// One screen of content the viewer can display.
struct Display {
    title: String,
    /// `(rgba, width, height)` if this entry has visual content.
    image: Option<(Vec<u8>, u32, u32)>,
    /// `(pcm, sample_rate)` if this entry has audible content.
    audio: Option<(Vec<i16>, u32)>,
    /// `(positions, indices)` if this entry is a 3D mesh (TMD viewer mode).
    /// Mutually exclusive with `vram_mesh`.
    mesh: Option<(Vec<[f32; 3]>, Vec<u32>)>,
    /// VRAM-mesh payload for proper PSX texture lookup (multi-page,
    /// per-prim CBA/TSB). Mutually exclusive with `mesh`.
    vram_mesh: Option<VramMeshPayload>,
    /// Wireframe payload (positions + per-vertex color + line indices) for
    /// the stage-geometry viewer. Mutually exclusive with `mesh`/`vram_mesh`.
    lines: Option<LinesPayload>,
}

/// CPU-side payload for the wireframe path. Built by [`load_stage_for_view`].
struct LinesPayload {
    positions: Vec<[f32; 3]>,
    colors: Vec<[u8; 4]>,
    indices: Vec<u32>,
}

/// CPU-side payload for the VRAM-mesh path. Built by [`load_tmd_for_view`];
/// uploaded to GPU on the renderer's thread.
struct VramMeshPayload {
    positions: Vec<[f32; 3]>,
    uvs: Vec<[u8; 2]>,
    cba_tsb: Vec<[u16; 2]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
    /// CPU-side VRAM holding every TIM in the source PROT entry, placed at
    /// its canonical fb_x/fb_y. The fragment shader does the page+CLUT
    /// lookup using each vertex's (cba, tsb).
    vram: Vram,
    /// Number of TIMs uploaded into `vram` (window-title context only).
    tim_count: usize,
    /// Source dir we pulled the TIMs from (window-title context).
    tim_dir_label: String,
}

impl Display {
    fn empty(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            image: None,
            audio: None,
            mesh: None,
            vram_mesh: None,
            lines: None,
        }
    }
}

/// Camera + animation state for the TMD viewer. Keeps the model spinning
/// at a constant angular velocity around its centroid.
struct MeshView {
    /// World-space center of the mesh AABB; the camera looks at this point.
    center: Vec3,
    /// Distance from the camera to `center`. Picked so the mesh fits in
    /// the viewport with a comfortable margin.
    distance: f32,
    /// Wall-clock origin for the rotation animation.
    started_at: Instant,
}

impl MeshView {
    fn from_aabb(lo: [f32; 3], hi: [f32; 3]) -> Self {
        let center = Vec3::new(
            0.5 * (lo[0] + hi[0]),
            0.5 * (lo[1] + hi[1]),
            0.5 * (lo[2] + hi[2]),
        );
        let extent = Vec3::new(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
        let radius = (0.5 * extent.length()).max(1.0);
        // Frame the bounding sphere with ~30° horizontal half-angle.
        let distance = radius / (30f32.to_radians().tan()) * 1.4;
        Self {
            center,
            distance,
            started_at: Instant::now(),
        }
    }

    fn mvp(&self, aspect: f32) -> Mat4 {
        let angle = self.started_at.elapsed().as_secs_f32() * 0.5; // ~28 deg/s
        // PSX has Y-down geometry, so flip Y in the model matrix to make the
        // mesh appear right-side-up under a Y-up camera.
        let model = Mat4::from_rotation_y(angle) * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
        let eye = self.center + Vec3::new(0.0, 0.0, self.distance);
        let view = Mat4::look_at_rh(eye, self.center, Vec3::Y);
        let near = (self.distance * 0.05).max(0.1);
        let far = self.distance * 4.0 + 100.0;
        let proj = Mat4::perspective_rh(60f32.to_radians(), aspect.max(0.01), near, far);
        proj * view * model
    }
}

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    audio: Option<AudioOut>,
    texture: Option<UploadedTexture>,
    mesh: Option<UploadedMesh>,
    /// VRAM-mesh upload (TMD + multi-TIM VRAM). Coexists with `mesh_view`.
    vram_mesh: Option<UploadedVramMesh>,
    /// PSX VRAM bound to the current `vram_mesh`.
    vram: Option<UploadedVram>,
    /// Wireframe upload for the stage-geometry viewer.
    lines: Option<UploadedLines>,
    mesh_view: Option<MeshView>,
    /// Initial display (set by main before the event loop starts).
    pending: Option<Display>,
    /// PROT browse state. None for single-file modes.
    browser: Option<Browser>,
    /// TMD directory-walk state. None for single-file `tmd` mode.
    mesh_browser: Option<MeshBrowser>,
    /// Stage-geometry directory-walk state. None for single-file `stage` mode.
    stage_browser: Option<StageBrowser>,
}

/// Directory walk for the `stage <DIR>` mode. Holds the list of files that
/// scanned positively as stage-geometry (so navigation skips entries with
/// no table) plus a CDNAME map for nicer titles.
struct StageBrowser {
    paths: Vec<PathBuf>,
    current: usize,
    root_label: String,
}

impl StageBrowser {
    fn count(&self) -> usize {
        self.paths.len()
    }
}

/// Directory walk for the `tmd <DIR>` mode. Holds the resolved list of
/// `.tmd` files so navigation is just an index step.
struct MeshBrowser {
    paths: Vec<PathBuf>,
    current: usize,
    /// User-supplied root, kept for window-title context.
    root_label: String,
    /// Extra TIM dirs to overlay before each mesh's sibling tim_scan dir.
    /// Constant for the lifetime of the browser; set from `--vram-extra-dir`.
    vram_extras: Vec<PathBuf>,
}

impl MeshBrowser {
    fn count(&self) -> usize {
        self.paths.len()
    }
}

struct Browser {
    archive: Archive,
    cdname: Option<cdname::IndexMap>,
    current: u32,
    last_count: u32,
}

impl Browser {
    fn name_for(&self, idx: u32) -> String {
        match self.cdname.as_ref().and_then(|m| cdname::block_for(m, idx)) {
            Some(name) => format!("{:04}_{}", idx, name),
            None => format!("{:04}", idx),
        }
    }

    fn entry_count(&self) -> u32 {
        self.archive.entries.len() as u32
    }
}

/// Try to produce a [`Display`] for one PROT entry by walking the known
/// sub-formats in priority order. Returns `None` if nothing renderable
/// could be extracted (caller advances past it).
fn display_for_prot_entry(name: &str, bytes: &[u8]) -> Option<Display> {
    let report = legaia_asset::categorize::classify(bytes);
    let title = format!("{}  ({}, {} bytes)", name, report.class.name(), bytes.len());

    // 1. TIM passthrough (first u32 == 0x00000010).
    if report.class == legaia_asset::categorize::Class::TimPassthrough
        && let Ok((rgba, w, h)) = load_tim(bytes, 0)
    {
        return Some(Display {
            title,
            image: Some((rgba, w, h)),
            audio: None,
            mesh: None,
            vram_mesh: None,
            lines: None,
        });
    }

    // 2. Standalone TIM-pack: take the first item.
    if report.class == legaia_asset::categorize::Class::TimPack {
        let items = legaia_prot::timpack::unpack(bytes);
        for item in &items {
            if let Ok((rgba, w, h)) = load_tim(item, 0) {
                return Some(Display {
                    title: format!("{} [pack:0/{}]", title, items.len()),
                    image: Some((rgba, w, h)),
                    audio: None,
                    mesh: None,
                    vram_mesh: None,
                    lines: None,
                });
            }
        }
    }

    // 3. DATA_FIELD streaming: walk chunks, find a TIM_LIST chunk with a
    // first sub-TIM that decodes.
    if report.class == legaia_asset::categorize::Class::DataFieldStreaming
        && let Ok(stream) = legaia_asset::parse_streaming(bytes, 4096)
    {
        for chunk in &stream.chunks {
            if chunk.type_byte != 0x01 {
                continue;
            }
            let data_start = chunk.header_offset + 4;
            let data_end = data_start + chunk.size as usize;
            if data_end > bytes.len() {
                continue;
            }
            let pack_data = &bytes[data_start..data_end];
            let Ok(items) = legaia_asset::pack::extract_pack(pack_data) else {
                continue;
            };
            for item in &items {
                if let Ok((rgba, w, h)) = load_tim(item, 0) {
                    return Some(Display {
                        title: format!("{} [stream:TIM_LIST]", title),
                        image: Some((rgba, w, h)),
                        audio: None,
                        mesh: None,
                        vram_mesh: None,
                        lines: None,
                    });
                }
            }
        }
    }

    // 4. Scene-TMD-prefixed stream: leading bare TMD at offset 4 is the
    // dominant scene-asset shape (148 PROT entries). Render flat-shaded —
    // no sibling TIM dir means no texturing, but the geometry is the
    // distinctive visual signal.
    if report.class == legaia_asset::categorize::Class::SceneTmdStream
        && let Some(s) = legaia_asset::scene_tmd_stream::detect(bytes)
        && let Ok(tmd) = legaia_tmd::parse(&bytes[s.tmd_range()])
    {
        let mesh = legaia_tmd::mesh::tmd_to_mesh(&tmd, &bytes[s.tmd_range()]);
        if !mesh.indices.is_empty() {
            return Some(Display {
                title: format!(
                    "{} [scene_tmd_stream: {} obj, {} verts, {} tris{}]",
                    title,
                    tmd.objects.len(),
                    mesh.positions.len(),
                    mesh.indices.len() / 3,
                    if s.tail_chunks.is_empty() {
                        String::new()
                    } else {
                        format!(", +{} tail chunks", s.tail_chunks.len())
                    },
                ),
                image: None,
                audio: None,
                mesh: Some((mesh.positions, mesh.indices)),
                vram_mesh: None,
                lines: None,
            });
        }
    }

    // 5. Scene-VAB-prefixed stream: leading VAB at offset 4 (217 PROT
    // entries — the dominant distributed-VAB carrier). Play sample 0 of
    // the embedded bank.
    if report.class == legaia_asset::categorize::Class::SceneVabStream
        && let Some(s) = legaia_asset::scene_vab_stream::detect(bytes)
        && let Ok(pcm) = decode_vab_sample(bytes, s.vab_range().start, 0)
    {
        return Some(Display {
            title: format!(
                "{} [scene_vab_stream: VAB v{}, ps={}, ts={}, sample 0]",
                title, s.vab_version, s.vab_ps, s.vab_ts
            ),
            image: None,
            audio: Some((pcm, DEFAULT_INPUT_RATE)),
            mesh: None,
            vram_mesh: None,
            lines: None,
        });
    }

    // 6. VAB bank fallback: scan the entry for a VAB header (the bank may
    // live at a non-zero offset inside a larger PROT entry — battle_data
    // and level_up entries hold theirs deep inside) and play sample 0 of
    // the first one we find.
    let vab_offsets = legaia_vab::find_vabs(bytes);
    if let Some(&off) = vab_offsets.first()
        && let Ok(pcm) = decode_vab_sample(bytes, off, 0)
    {
        return Some(Display {
            title: format!("{} [vab @ 0x{:X}, sample 0]", title, off),
            image: None,
            audio: Some((pcm, DEFAULT_INPUT_RATE)),
            mesh: None,
            vram_mesh: None,
            lines: None,
        });
    }

    // Nothing displayable.
    Some(Display::empty(title))
}

impl App {
    fn apply(&mut self, display: Display) {
        if let Some(w) = &self.window {
            w.set_title(&display.title);
        }
        if let Some(r) = &self.renderer {
            self.texture = match display.image {
                Some((rgba, w, h)) => match r.upload_texture(&rgba, w, h) {
                    Ok(t) => Some(t),
                    Err(e) => {
                        log::error!("upload texture failed: {e:#}");
                        None
                    }
                },
                None => None,
            };
            (self.mesh, self.mesh_view) = match display.mesh {
                Some((positions, indices)) => {
                    let aabb = mesh_aabb(&positions);
                    match r.upload_mesh(&positions, &indices) {
                        Ok(m) => (Some(m), Some(MeshView::from_aabb(aabb.0, aabb.1))),
                        Err(e) => {
                            log::error!("upload mesh failed: {e:#}");
                            (None, None)
                        }
                    }
                }
                None => (None, None),
            };
            // Replace any prior VRAM-mesh/VRAM pair with the new one (or
            // clear, if the next display has no VRAM mesh).
            (self.vram_mesh, self.vram) = match display.vram_mesh {
                Some(payload) => {
                    let aabb = mesh_aabb(&payload.positions);
                    let mesh_res = r.upload_vram_mesh(
                        &payload.positions,
                        &payload.uvs,
                        &payload.cba_tsb,
                        &payload.normals,
                        &payload.indices,
                    );
                    let vram_res = r.upload_vram(&payload.vram);
                    match (mesh_res, vram_res) {
                        (Ok(m), Ok(v)) => {
                            // Frame the camera on the VRAM mesh's AABB.
                            self.mesh_view = Some(MeshView::from_aabb(aabb.0, aabb.1));
                            (Some(m), Some(v))
                        }
                        (mesh_err, vram_err) => {
                            if let Err(e) = mesh_err {
                                log::error!("upload vram mesh failed: {e:#}");
                            }
                            if let Err(e) = vram_err {
                                log::error!("upload vram failed: {e:#}");
                            }
                            (None, None)
                        }
                    }
                }
                None => (None, None),
            };
            // Wireframe / stage-geometry upload.
            self.lines = match display.lines {
                Some(payload) => {
                    let aabb = mesh_aabb(&payload.positions);
                    match r.upload_lines(&payload.positions, &payload.colors, &payload.indices) {
                        Ok(l) => {
                            self.mesh_view = Some(MeshView::from_aabb(aabb.0, aabb.1));
                            Some(l)
                        }
                        Err(e) => {
                            log::error!("upload lines failed: {e:#}");
                            None
                        }
                    }
                }
                None => None,
            };
        }
        if let (Some(a), Some((pcm, rate))) = (&self.audio, display.audio) {
            a.play_pcm_mono(pcm, rate);
        } else if let Some(a) = &self.audio {
            a.stop();
        }
    }

    fn show_browser_current(&mut self) {
        let Some(b) = self.browser.as_mut() else {
            return;
        };
        let count = b.entry_count();
        let cursor = b.current.min(count.saturating_sub(1));
        let entry = b.archive.entries[cursor as usize].clone();
        let name = b.name_for(cursor);
        let mut buf = Vec::new();
        let display = match b.archive.read_entry(&entry, &mut buf) {
            Ok(()) => display_for_prot_entry(&name, &buf)
                .unwrap_or_else(|| Display::empty(format!("{} (read failed)", name))),
            Err(e) => Display::empty(format!("{} (io error: {e})", name)),
        };
        b.current = cursor;
        b.last_count = count;
        let help = " — [N]ext [P]rev [PgDn]+10 [PgUp]-10 [Esc] quit";
        let mut display = display;
        display.title.push_str(help);
        self.apply(display);
    }

    fn step(&mut self, delta: i32) {
        if self.browser.is_some() {
            self.step_prot(delta);
        } else if self.mesh_browser.is_some() {
            self.step_mesh(delta);
        } else if self.stage_browser.is_some() {
            self.step_stage(delta);
        }
    }

    fn step_stage(&mut self, delta: i32) {
        let Some(sb) = self.stage_browser.as_mut() else {
            return;
        };
        let count = sb.count() as i32;
        if count == 0 {
            return;
        }
        let next = (sb.current as i32 + delta).rem_euclid(count);
        sb.current = next as usize;
        self.show_stage_current();
    }

    fn show_stage_current(&mut self) {
        let (path, label, idx, total) = {
            let Some(sb) = self.stage_browser.as_ref() else {
                return;
            };
            (
                sb.paths[sb.current].clone(),
                sb.root_label.clone(),
                sb.current + 1,
                sb.paths.len(),
            )
        };
        let display = match load_stage_for_view(&path) {
            Ok(payload) => {
                let title = format!(
                    "STAGE [{}/{}] {}  ({} verts, {} lines)  — {}  [N]ext [P]rev [PgDn]+10 [PgUp]-10 [Esc] quit",
                    idx,
                    total,
                    short_path(&path),
                    payload.positions.len(),
                    payload.indices.len() / 2,
                    label,
                );
                Display {
                    title,
                    image: None,
                    audio: None,
                    mesh: None,
                    vram_mesh: None,
                    lines: Some(payload),
                }
            }
            Err(e) => Display::empty(format!(
                "STAGE [{}/{}] {} (load failed: {e})",
                idx,
                total,
                short_path(&path),
            )),
        };
        self.apply(display);
    }

    fn step_prot(&mut self, delta: i32) {
        let Some(b) = self.browser.as_mut() else {
            return;
        };
        let count = b.entry_count() as i32;
        if count == 0 {
            return;
        }
        let next = (b.current as i32 + delta).rem_euclid(count);
        b.current = next as u32;
        self.show_browser_current();
    }

    fn step_mesh(&mut self, delta: i32) {
        let Some(mb) = self.mesh_browser.as_mut() else {
            return;
        };
        let count = mb.count() as i32;
        if count == 0 {
            return;
        }
        let next = (mb.current as i32 + delta).rem_euclid(count);
        mb.current = next as usize;
        self.show_mesh_current();
    }

    fn show_mesh_current(&mut self) {
        let (path, label, idx, total, extras) = {
            let Some(mb) = self.mesh_browser.as_ref() else {
                return;
            };
            let path = mb.paths[mb.current].clone();
            (
                path,
                mb.root_label.clone(),
                mb.current + 1,
                mb.paths.len(),
                mb.vram_extras.clone(),
            )
        };
        let display = match load_tmd_for_view(&path, &extras) {
            Ok(TmdViewData::Flat { positions, indices }) => {
                let title = format!(
                    "TMD [{}/{}] {}  ({} verts, {} tris) untextured  — {}  [N]ext [P]rev [PgDn]+10 [PgUp]-10 [Esc] quit",
                    idx,
                    total,
                    short_path(&path),
                    positions.len(),
                    indices.len() / 3,
                    label,
                );
                Display {
                    title,
                    image: None,
                    audio: None,
                    mesh: Some((positions, indices)),
                    vram_mesh: None,
                    lines: None,
                }
            }
            Ok(TmdViewData::Vram(payload)) => {
                let title = format!(
                    "TMD [{}/{}] {}  ({} tri-verts) vram={} TIMs from {}  — {}  [N]ext [P]rev [PgDn]+10 [PgUp]-10 [Esc] quit",
                    idx,
                    total,
                    short_path(&path),
                    payload.indices.len() / 3,
                    payload.tim_count,
                    payload.tim_dir_label,
                    label,
                );
                Display {
                    title,
                    image: None,
                    audio: None,
                    mesh: None,
                    vram_mesh: Some(payload),
                    lines: None,
                }
            }
            Err(e) => Display::empty(format!(
                "TMD [{}/{}] {} (load failed: {e})",
                idx,
                total,
                short_path(&path),
            )),
        };
        self.apply(display);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = WindowAttributes::default()
            .with_title("legaia asset viewer")
            .with_inner_size(winit::dpi::LogicalSize::new(1024, 768));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        let size = window.inner_size();
        let renderer =
            Renderer::new(window.clone(), size.width, size.height).expect("create renderer");
        self.window = Some(window);
        self.renderer = Some(renderer);
        // Audio is best-effort: if no device is available the viewer still
        // renders images.
        match AudioOut::new() {
            Ok(a) => self.audio = Some(a),
            Err(e) => log::warn!("audio init failed (continuing without): {e:#}"),
        }
        if let Some(d) = self.pending.take() {
            self.apply(d);
        }
        if self.browser.is_some() {
            self.show_browser_current();
        }
        if self.mesh_browser.is_some() {
            self.show_mesh_current();
        }
        if self.stage_browser.is_some() {
            self.show_stage_current();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.renderer {
                    r.resize(size.width, size.height);
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        physical_key: PhysicalKey::Code(code),
                        ..
                    },
                ..
            } => match code {
                KeyCode::Escape => event_loop.exit(),
                KeyCode::ArrowRight | KeyCode::KeyN | KeyCode::Space => self.step(1),
                KeyCode::ArrowLeft | KeyCode::KeyP => self.step(-1),
                KeyCode::PageDown => self.step(10),
                KeyCode::PageUp => self.step(-10),
                _ => {}
            },
            WindowEvent::RedrawRequested => {
                if let Some(r) = &self.renderer {
                    // Priority: VRAM mesh > flat mesh > wireframe > 2D texture > clear.
                    let target = if let (Some(vm), Some(vram), Some(view)) = (
                        self.vram_mesh.as_ref(),
                        self.vram.as_ref(),
                        self.mesh_view.as_ref(),
                    ) {
                        let (w, h) = r.surface_size();
                        let aspect = w as f32 / h.max(1) as f32;
                        RenderTarget::VramMesh {
                            mesh: vm,
                            vram,
                            mvp: view.mvp(aspect),
                        }
                    } else if let (Some(mesh), Some(view)) =
                        (self.mesh.as_ref(), self.mesh_view.as_ref())
                    {
                        let (w, h) = r.surface_size();
                        let aspect = w as f32 / h.max(1) as f32;
                        RenderTarget::Mesh {
                            mesh,
                            mvp: view.mvp(aspect),
                        }
                    } else if let (Some(lines), Some(view)) =
                        (self.lines.as_ref(), self.mesh_view.as_ref())
                    {
                        let (w, h) = r.surface_size();
                        let aspect = w as f32 / h.max(1) as f32;
                        RenderTarget::Lines {
                            mesh: lines,
                            mvp: view.mvp(aspect),
                        }
                    } else if let Some(t) = self.texture.as_ref() {
                        RenderTarget::Texture(t)
                    } else {
                        RenderTarget::Clear
                    };
                    if let Err(e) = r.render(target) {
                        log::error!("render error: {e:#}");
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }
}

/// Headless SEQ playback. Opens AudioOut, builds a `Sequencer` over the
/// parsed SEQ + uploaded VAB, attaches it, and prints progress until
/// end-of-track (or forever if `--looped`). Ctrl-C to exit.
fn run_seq_playback(
    seq_path: &Path,
    vab_path: &Path,
    vab_offset: usize,
    looped: bool,
    master_vol: u8,
) -> Result<()> {
    use legaia_engine_audio::spu::ram::SPU_RAM_BYTES;
    use legaia_engine_audio::{Spu, SpuAllocator, VabBank};
    use legaia_seq::Seq;

    let seq_bytes =
        std::fs::read(seq_path).with_context(|| format!("read {}", seq_path.display()))?;
    let seq = Seq::parse(&seq_bytes).context("parse SEQ")?;
    log::info!(
        "seq: {} events, {} ticks, init {:.1} BPM @ {} PPQN",
        seq.events.len(),
        seq.total_ticks(),
        seq.header.bpm(),
        seq.header.ppqn
    );

    let vab_bytes =
        std::fs::read(vab_path).with_context(|| format!("read {}", vab_path.display()))?;
    let report = legaia_vab::parse(&vab_bytes, vab_offset).context("parse VAB")?;
    log::info!(
        "vab: {} programs, {} samples (offset 0x{:X})",
        report.header.ps,
        report.vag_samples.len(),
        vab_offset
    );

    let audio = AudioOut::new().context("open audio output")?;

    // Build the bank inside the AudioOut's SPU (via with_spu) so the
    // sequencer's SPU references match the playback SPU. Reserve the
    // first 4 KB for voice 0 / scratchpad, allocate from there onward.
    let bank = audio.with_spu(|spu: &mut Spu| {
        let mut alloc = SpuAllocator::new(0x1000, SPU_RAM_BYTES as u32 - 0x1000);
        VabBank::upload(spu, &mut alloc, &report, &vab_bytes)
    });

    let mut sequencer = Sequencer::new(seq, bank);
    sequencer.set_master_vol(master_vol);
    if looped {
        sequencer.set_loop_to(0);
    }
    audio.attach_sequencer(sequencer);

    log::info!(
        "playing SEQ {} (vab {} @ 0x{:X}, looped={}, master_vol={})",
        seq_path.display(),
        vab_path.display(),
        vab_offset,
        looped,
        master_vol
    );

    // Poll the sequencer's progress at ~10 Hz, print one status line per
    // change, and exit when finished (or never, if --looped).
    let start = Instant::now();
    let mut last_tick: u64 = 0;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let Some(p) = audio.sequencer_progress() else {
            break;
        };
        if p.tick != last_tick {
            log::info!(
                "  +{:.1}s tick={} bpm={:.1} active={}",
                start.elapsed().as_secs_f32(),
                p.tick,
                p.bpm,
                p.active_notes
            );
            last_tick = p.tick;
        }
        if p.finished {
            log::info!(
                "end of track ({:.1}s elapsed)",
                start.elapsed().as_secs_f32()
            );
            break;
        }
    }
    audio.detach_sequencer();
    Ok(())
}

// ---- world (multi-actor) viewer -------------------------------------------
//
// `asset-viewer world <SCENE>` runs the engine-core `World` composite over a
// real CDNAME scene. It loads up to N TMDs from `<extracted>/tmd_scan/<entry>/`
// for entries inside the scene's CDNAME block, builds VRAM from the matching
// `tim_scan/` dirs, spawns one actor per TMD, and ticks the World at ~60 Hz.
//
// Each actor's MVP is computed from its `move_state.world_x/y/z`. The default
// path animates positions analytically (sinusoidal orbit) so the multi-actor
// renderer is exercised without depending on real per-scene move bytecode.
// `--with-move-vm` instead loads a synthetic `WORLD_SET → WAIT_SET → HALT`
// program per actor so the move-VM port runs every tick.

struct WorldActorMesh {
    mesh: legaia_engine_render::UploadedVramMesh,
    /// AABB of the local TMD geometry (pre-transform). Used to size the
    /// camera frustum.
    aabb_lo: [f32; 3],
    aabb_hi: [f32; 3],
}

fn run_world(
    scene_name: &str,
    max_actors: usize,
    extracted_root: &Path,
    with_move_vm: bool,
) -> Result<()> {
    use legaia_engine_core::scene::ProtIndex;
    use legaia_engine_core::world::{SceneMode, World};

    if max_actors == 0 {
        anyhow::bail!("max_actors must be >= 1");
    }

    let prot_path = extracted_root.join("PROT.DAT");
    let cdname_path = extracted_root.join("CDNAME.TXT");
    if !prot_path.exists() {
        anyhow::bail!("missing {} (run legaia-extract first)", prot_path.display());
    }
    if !cdname_path.exists() {
        anyhow::bail!(
            "missing {} (run legaia-extract first)",
            cdname_path.display()
        );
    }
    let index = ProtIndex::open_extracted(extracted_root)
        .with_context(|| format!("open ProtIndex at {}", extracted_root.display()))?;
    let (start, end) = index.block_range(scene_name).ok_or_else(|| {
        anyhow::anyhow!(
            "scene '{}' not found in CDNAME map at {}",
            scene_name,
            cdname_path.display()
        )
    })?;
    log::info!(
        "scene '{}' covers PROT [{}..{}) ({} entries)",
        scene_name,
        start,
        end,
        end - start
    );

    // Collect TMDs from the scene block's tmd_scan/ subdirs.
    let tmd_paths = collect_scene_tmds(extracted_root, start, end);
    if tmd_paths.is_empty() {
        anyhow::bail!(
            "no TMDs found in tmd_scan for scene '{}' (PROT block {}..{})",
            scene_name,
            start,
            end
        );
    }
    let actor_count = tmd_paths.len().min(max_actors);
    log::info!(
        "loaded {} TMD(s) under tmd_scan; spawning {} actor(s)",
        tmd_paths.len(),
        actor_count
    );

    // Build a shared VRAM from every tim_scan/ dir in the scene block.
    let tim_dirs = collect_scene_tim_dirs(extracted_root, start, end);
    let tim_dir_refs: Vec<&Path> = tim_dirs.iter().map(|p| p.as_path()).collect();
    let (vram, tim_count) = build_vram_from_dirs(&tim_dir_refs);
    log::info!(
        "built VRAM from {} TIM(s) across {} tim_scan dir(s)",
        tim_count,
        tim_dirs.len()
    );

    // Build the world composite + spawn the actors with the picked TMDs.
    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    let radius = 800.0_f32;
    for i in 0..actor_count {
        let theta = (i as f32) * std::f32::consts::TAU / (actor_count as f32);
        let x = (radius * theta.cos()) as i16;
        let z = (radius * theta.sin()) as i16;
        let actor = world.spawn_actor(i);
        actor.move_state.world_x = x;
        actor.move_state.world_y = 0;
        actor.move_state.world_z = z;
        if with_move_vm {
            // Synthetic: WORLD_SET (x, y, z) → WAIT_SET 8 → HALT.
            world.set_move_bytecode(
                i,
                Some(vec![0x0007, x as u16, 0, z as u16, 0x0009, 8, 0x0008]),
            );
        }
    }

    // Hand off to the windowing loop.
    let event_loop = EventLoop::new().context("create event loop")?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = WorldApp {
        title: format!(
            "World — scene '{}' [{} actor(s), tim_count={}]",
            scene_name, actor_count, tim_count
        ),
        window: None,
        renderer: None,
        vram_cpu: Some(vram),
        uploaded_vram: None,
        tmd_paths,
        meshes: Vec::new(),
        world,
        actor_count,
        with_move_vm,
        scene_aabb: ([-radius, -200.0, -radius], [radius, 600.0, radius]),
        started_at: Instant::now(),
        last_tick: Instant::now(),
    };
    event_loop.run_app(&mut app).context("event loop")?;
    Ok(())
}

/// Collect every `tmd_scan/<NNNN_label>/*.tmd` for entries within
/// `[start, end)`. Returns paths sorted by entry index then filename so the
/// per-actor mesh assignment is deterministic.
fn collect_scene_tmds(extracted_root: &Path, start: u32, end: u32) -> Vec<PathBuf> {
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
fn collect_scene_tim_dirs(extracted_root: &Path, start: u32, end: u32) -> Vec<PathBuf> {
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

/// Convert a laid-out string into [`TextDraw`]s anchored at `(pen_x, pen_y)`
/// with the supplied tint. Glyph atlas coordinates come from the layout;
/// destination coordinates are pen-relative pixels with one quad per glyph.
fn text_draws_for(layout: &legaia_font::Layout, pen: (i32, i32), color: [f32; 4]) -> Vec<TextDraw> {
    layout
        .glyphs
        .iter()
        .map(|g| TextDraw {
            dst: (pen.0 + g.dst_x, pen.1 + g.dst_y, g.width, g.height),
            src: (g.atlas_x, g.atlas_y, g.width, g.height),
            color,
        })
        .collect()
}

/// Map winit physical keys to PSX pad button bits. Keyboard mapping mirrors
/// the conventional emulator default:
///
/// - Arrows → D-pad
/// - Z → Cross, X → Square, A → Triangle, S → Circle
/// - Enter → Start, Right Shift → Select
/// - Q / W → L1 / R1, 1 / 2 → L2 / R2
fn keymap_pad(code: KeyCode) -> Option<PadButton> {
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
fn pad_button_label(b: PadButton) -> &'static str {
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

fn run_field(scene_name: &str, max_actors: usize, extracted_root: &Path) -> Result<()> {
    use legaia_engine_core::scene::ProtIndex;
    use legaia_engine_core::world::{SceneMode, World};

    if max_actors == 0 {
        anyhow::bail!("max_actors must be >= 1");
    }
    let prot_path = extracted_root.join("PROT.DAT");
    let cdname_path = extracted_root.join("CDNAME.TXT");
    if !prot_path.exists() {
        anyhow::bail!("missing {} (run legaia-extract first)", prot_path.display());
    }
    if !cdname_path.exists() {
        anyhow::bail!(
            "missing {} (run legaia-extract first)",
            cdname_path.display()
        );
    }
    let font = Font::load_from_extracted(extracted_root).with_context(|| {
        format!(
            "load extracted font under {} (run legaia-extract first?)",
            extracted_root.display()
        )
    })?;
    let index = ProtIndex::open_extracted(extracted_root)
        .with_context(|| format!("open ProtIndex at {}", extracted_root.display()))?;
    let (start, end) = index.block_range(scene_name).ok_or_else(|| {
        anyhow::anyhow!(
            "scene '{}' not found in CDNAME map at {}",
            scene_name,
            cdname_path.display()
        )
    })?;
    log::info!(
        "scene '{}' covers PROT [{}..{}) ({} entries)",
        scene_name,
        start,
        end,
        end - start
    );
    let tmd_paths = collect_scene_tmds(extracted_root, start, end);
    let actor_count = tmd_paths.len().min(max_actors);
    let tim_dirs = collect_scene_tim_dirs(extracted_root, start, end);
    let tim_dir_refs: Vec<&Path> = tim_dirs.iter().map(|p| p.as_path()).collect();
    let (vram, tim_count) = build_vram_from_dirs(&tim_dir_refs);
    log::info!(
        "field scene: {} actors over {} TIM(s) across {} tim_scan dir(s)",
        actor_count,
        tim_count,
        tim_dirs.len()
    );

    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    let radius = 800.0_f32;
    for i in 0..actor_count {
        let theta = (i as f32) * std::f32::consts::TAU / (actor_count.max(1) as f32);
        let x = (radius * theta.cos()) as i16;
        let z = (radius * theta.sin()) as i16;
        let actor = world.spawn_actor(i);
        actor.move_state.world_x = x;
        actor.move_state.world_y = 0;
        actor.move_state.world_z = z;
    }

    let event_loop = EventLoop::new().context("create event loop")?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = FieldApp {
        title: format!(
            "Field — scene '{}' [{} actors, tim_count={}]",
            scene_name, actor_count, tim_count
        ),
        scene_name: scene_name.to_string(),
        scene_range: (start, end),
        actor_count,
        window: None,
        renderer: None,
        font,
        font_atlas: None,
        vram_cpu: Some(vram),
        uploaded_vram: None,
        tmd_paths,
        meshes: Vec::new(),
        world,
        scene_aabb: ([-radius, -200.0, -radius], [radius, 600.0, radius]),
        input: InputState::new(),
        last_dt_ms: 16,
        started_at: Instant::now(),
        last_tick: Instant::now(),
    };
    event_loop.run_app(&mut app).context("event loop")?;
    Ok(())
}

/// Field-mode viewer state. Owned by the winit event loop.
struct FieldApp {
    title: String,
    scene_name: String,
    scene_range: (u32, u32),
    actor_count: usize,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    font: Font,
    font_atlas: Option<UploadedFontAtlas>,
    vram_cpu: Option<Vram>,
    uploaded_vram: Option<UploadedVram>,
    tmd_paths: Vec<PathBuf>,
    meshes: Vec<WorldActorMesh>,
    world: legaia_engine_core::world::World,
    /// Synthetic AABB enclosing every spawn point — drives the camera.
    scene_aabb: ([f32; 3], [f32; 3]),
    input: InputState,
    /// Last per-frame delta in ms, smoothed for the FPS HUD readout.
    last_dt_ms: u32,
    started_at: Instant,
    last_tick: Instant,
}

impl FieldApp {
    /// Upload TMDs as actor meshes plus the shared VRAM and font atlas.
    /// Must be called once a renderer is attached.
    fn upload_assets(&mut self) {
        let Some(r) = &self.renderer else {
            return;
        };
        for path in &self.tmd_paths {
            let bytes = match std::fs::read(path) {
                Ok(b) => b,
                Err(e) => {
                    log::warn!("skip TMD {}: read error: {e}", path.display());
                    continue;
                }
            };
            let tmd = match legaia_tmd::parse(&bytes) {
                Ok(t) => t,
                Err(e) => {
                    log::warn!("skip TMD {}: parse error: {e}", path.display());
                    continue;
                }
            };
            let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &bytes);
            if vmesh.indices.is_empty() {
                continue;
            }
            let aabb = mesh_aabb(&vmesh.positions);
            match r.upload_vram_mesh(
                &vmesh.positions,
                &vmesh.uvs,
                &vmesh.cba_tsb,
                &vmesh.normals,
                &vmesh.indices,
            ) {
                Ok(mesh) => self.meshes.push(WorldActorMesh {
                    mesh,
                    aabb_lo: aabb.0,
                    aabb_hi: aabb.1,
                }),
                Err(e) => log::warn!("skip TMD {}: upload error: {e}", path.display()),
            }
            if self.meshes.len() >= self.actor_count {
                break;
            }
        }
        if let Some(vram_cpu) = self.vram_cpu.take() {
            match r.upload_vram(&vram_cpu) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("VRAM upload failed: {e:#}"),
            }
        }
        let (aw, ah) = self.font.atlas_dimensions();
        match r.upload_font_atlas(self.font.atlas_rgba(), aw, ah) {
            Ok(a) => self.font_atlas = Some(a),
            Err(e) => log::error!("font atlas upload failed: {e:#}"),
        }
        // Recompute scene AABB from the union of every mesh's local AABB +
        // its current position so the camera frames the whole scene.
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for (slot, m) in self.meshes.iter().enumerate() {
            let actor = &self.world.actors[slot];
            let cx = actor.move_state.world_x as f32;
            let cy = actor.move_state.world_y as f32;
            let cz = actor.move_state.world_z as f32;
            for ax in 0..3 {
                let lo_world = [m.aabb_lo[0] + cx, m.aabb_lo[1] + cy, m.aabb_lo[2] + cz][ax];
                let hi_world = [m.aabb_hi[0] + cx, m.aabb_hi[1] + cy, m.aabb_hi[2] + cz][ax];
                if lo_world < lo[ax] {
                    lo[ax] = lo_world;
                }
                if hi_world > hi[ax] {
                    hi[ax] = hi_world;
                }
            }
        }
        if lo[0].is_finite() && hi[0].is_finite() {
            self.scene_aabb = (lo, hi);
        }
    }

    fn camera_mvp(&self, aspect: f32) -> Mat4 {
        let (lo, hi) = self.scene_aabb;
        let center = Vec3::new(
            0.5 * (lo[0] + hi[0]),
            0.5 * (lo[1] + hi[1]),
            0.5 * (lo[2] + hi[2]),
        );
        let extent = Vec3::new(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
        let radius = (0.5 * extent.length()).max(1.0);
        let distance = radius / (30f32.to_radians().tan()) * 1.6;
        let angle = self.started_at.elapsed().as_secs_f32() * 0.25;
        let eye = center
            + Vec3::new(
                distance * angle.cos(),
                -distance * 0.4,
                distance * angle.sin(),
            );
        let view = Mat4::look_at_rh(eye, center, Vec3::Y);
        let near = (distance * 0.05).max(0.1);
        let far = distance * 4.0 + 1000.0;
        let proj = Mat4::perspective_rh(60f32.to_radians(), aspect.max(0.01), near, far);
        proj * view
    }

    fn actor_model(&self, slot: usize) -> Mat4 {
        let a = &self.world.actors[slot];
        let pos = Vec3::new(
            a.move_state.world_x as f32,
            a.move_state.world_y as f32,
            a.move_state.world_z as f32,
        );
        let spin = self.started_at.elapsed().as_secs_f32() * 0.6
            + (slot as f32) * std::f32::consts::FRAC_PI_2;
        Mat4::from_translation(pos)
            * Mat4::from_rotation_y(spin)
            * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))
    }

    /// Build the HUD overlay for this frame. White text on the upper-left
    /// shows scene name + frame info; the bottom strip shows the live pad
    /// state. Returns an empty list if the font atlas hasn't uploaded yet.
    fn build_hud(&self) -> Vec<TextDraw> {
        if self.font_atlas.is_none() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let white = [1.0, 1.0, 1.0, 1.0];
        let dim = [0.7, 0.85, 1.0, 1.0];

        let line1 = format!(
            "scene {}  prot[{}..{})  actors {}",
            self.scene_name, self.scene_range.0, self.scene_range.1, self.actor_count
        );
        let layout1 = self.font.layout_ascii(&line1);
        out.extend(text_draws_for(&layout1, (8, 8), white));

        let fps = 1000u32.checked_div(self.last_dt_ms).unwrap_or(0);
        let line2 = format!(
            "frame {}   {:>3} fps   t {:.1}s",
            self.world.frame,
            fps,
            self.started_at.elapsed().as_secs_f32()
        );
        let layout2 = self.font.layout_ascii(&line2);
        out.extend(text_draws_for(&layout2, (8, 26), dim));

        // Pad state — show one cell per logical button.
        let buttons = [
            PadButton::Up,
            PadButton::Down,
            PadButton::Left,
            PadButton::Right,
            PadButton::Cross,
            PadButton::Circle,
            PadButton::Square,
            PadButton::Triangle,
            PadButton::Start,
            PadButton::Select,
            PadButton::L1,
            PadButton::R1,
            PadButton::L2,
            PadButton::R2,
        ];
        let mut pad_str = String::with_capacity(64);
        for b in buttons {
            let label = pad_button_label(b);
            if self.input.pressed(b) {
                pad_str.push_str(label);
            } else {
                for _ in 0..label.len() {
                    pad_str.push('-');
                }
            }
            pad_str.push(' ');
        }
        let layout3 = self.font.layout_ascii(&pad_str);
        let (_, h) = self
            .renderer
            .as_ref()
            .map(|r| r.surface_size())
            .unwrap_or((960, 720));
        out.extend(text_draws_for(
            &layout3,
            (8, (h as i32) - 24),
            [0.95, 0.95, 0.6, 1.0],
        ));
        out
    }
}

impl ApplicationHandler for FieldApp {
    fn resumed(&mut self, evl: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = WindowAttributes::default()
            .with_title(&self.title)
            .with_inner_size(winit::dpi::LogicalSize::new(960.0, 720.0));
        let window = match evl.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::error!("create_window: {e:#}");
                evl.exit();
                return;
            }
        };
        let size = window.inner_size();
        let renderer = match Renderer::new(window.clone(), size.width, size.height) {
            Ok(r) => r,
            Err(e) => {
                log::error!("Renderer::new: {e:#}");
                evl.exit();
                return;
            }
        };
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.upload_assets();
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn window_event(&mut self, evl: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => evl.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size.width, size.height);
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state,
                        ..
                    },
                ..
            } => {
                if matches!(code, KeyCode::Escape) && state == ElementState::Pressed {
                    evl.exit();
                    return;
                }
                if let Some(button) = keymap_pad(code) {
                    let mut mask = self.input.pad();
                    if state == ElementState::Pressed {
                        mask |= button.mask();
                    } else {
                        mask &= !button.mask();
                    }
                    // Defer the edge update until the next tick; we only
                    // mutate the held-mask snapshot here so multiple keys
                    // pressed within one frame all coalesce into the same
                    // tick.
                    self.input.set_pad(mask);
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = now
                    .duration_since(self.last_tick)
                    .min(std::time::Duration::from_secs(1));
                self.last_dt_ms = dt.as_millis().min(1000) as u32;
                self.last_tick = now;
                let target_frames = (dt.as_secs_f32() * 60.0).round() as u32;
                for _ in 0..target_frames.min(8) {
                    self.world.tick();
                }
                // Move the player slot 0 on D-pad input — gives the demo a
                // visible response to keyboard / gamepad input even with no
                // field bytecode loaded.
                let speed = 6.0_f32;
                if self.actor_count > 0 {
                    let actor = &mut self.world.actors[0];
                    if self.input.pressed(PadButton::Right) {
                        actor.move_state.world_x = (actor.move_state.world_x as f32 + speed) as i16;
                    }
                    if self.input.pressed(PadButton::Left) {
                        actor.move_state.world_x = (actor.move_state.world_x as f32 - speed) as i16;
                    }
                    if self.input.pressed(PadButton::Up) {
                        actor.move_state.world_z = (actor.move_state.world_z as f32 - speed) as i16;
                    }
                    if self.input.pressed(PadButton::Down) {
                        actor.move_state.world_z = (actor.move_state.world_z as f32 + speed) as i16;
                    }
                }
                if let (Some(r), Some(vram), Some(atlas)) = (
                    &self.renderer,
                    self.uploaded_vram.as_ref(),
                    self.font_atlas.as_ref(),
                ) {
                    let (w, h) = r.surface_size();
                    let aspect = w as f32 / h.max(1) as f32;
                    let cam = self.camera_mvp(aspect);
                    let draws: Vec<SceneDraw<'_>> = self
                        .meshes
                        .iter()
                        .enumerate()
                        .map(|(slot, m)| SceneDraw {
                            mesh: &m.mesh,
                            mvp: cam * self.actor_model(slot),
                        })
                        .collect();
                    let hud = self.build_hud();
                    let overlay = TextOverlay { atlas, draws: &hud };
                    let scene = RenderScene {
                        vram,
                        draws: &draws,
                        overlay_lines: None,
                        overlay_text: Some(&overlay),
                    };
                    if let Err(e) = r.render(RenderTarget::Scene(&scene)) {
                        log::error!("render error: {e:#}");
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }
}

/// Multi-actor world viewer state. Owned by the winit event loop.
struct WorldApp {
    title: String,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    vram_cpu: Option<Vram>,
    uploaded_vram: Option<UploadedVram>,
    tmd_paths: Vec<PathBuf>,
    meshes: Vec<WorldActorMesh>,
    world: legaia_engine_core::world::World,
    actor_count: usize,
    with_move_vm: bool,
    /// Synthetic AABB enclosing every spawn point — drives the camera.
    scene_aabb: ([f32; 3], [f32; 3]),
    started_at: Instant,
    last_tick: Instant,
}

impl WorldApp {
    fn upload_meshes(&mut self) {
        let Some(r) = &self.renderer else {
            return;
        };
        for path in &self.tmd_paths {
            let bytes = match std::fs::read(path) {
                Ok(b) => b,
                Err(e) => {
                    log::warn!("skip TMD {}: read error: {e}", path.display());
                    continue;
                }
            };
            let tmd = match legaia_tmd::parse(&bytes) {
                Ok(t) => t,
                Err(e) => {
                    log::warn!("skip TMD {}: parse error: {e}", path.display());
                    continue;
                }
            };
            let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &bytes);
            if vmesh.indices.is_empty() {
                log::warn!(
                    "skip TMD {}: zero triangles after primitive walk",
                    path.display()
                );
                continue;
            }
            let aabb = mesh_aabb(&vmesh.positions);
            match r.upload_vram_mesh(
                &vmesh.positions,
                &vmesh.uvs,
                &vmesh.cba_tsb,
                &vmesh.normals,
                &vmesh.indices,
            ) {
                Ok(mesh) => self.meshes.push(WorldActorMesh {
                    mesh,
                    aabb_lo: aabb.0,
                    aabb_hi: aabb.1,
                }),
                Err(e) => log::warn!("skip TMD {}: upload error: {e}", path.display()),
            }
            if self.meshes.len() >= self.actor_count {
                break;
            }
        }
        if self.meshes.is_empty() {
            log::error!("no TMDs uploaded successfully — nothing to draw");
        }
        if let Some(vram_cpu) = self.vram_cpu.take() {
            match r.upload_vram(&vram_cpu) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("VRAM upload failed: {e:#}"),
            }
        }
        // Recompute scene AABB from the union of every mesh's local AABB +
        // its current position so the camera frames the whole scene.
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for (slot, m) in self.meshes.iter().enumerate() {
            let actor = &self.world.actors[slot];
            let cx = actor.move_state.world_x as f32;
            let cy = actor.move_state.world_y as f32;
            let cz = actor.move_state.world_z as f32;
            for ax in 0..3 {
                let lo_world = [m.aabb_lo[0] + cx, m.aabb_lo[1] + cy, m.aabb_lo[2] + cz][ax];
                let hi_world = [m.aabb_hi[0] + cx, m.aabb_hi[1] + cy, m.aabb_hi[2] + cz][ax];
                if lo_world < lo[ax] {
                    lo[ax] = lo_world;
                }
                if hi_world > hi[ax] {
                    hi[ax] = hi_world;
                }
            }
        }
        if lo[0].is_finite() && hi[0].is_finite() {
            self.scene_aabb = (lo, hi);
        }
    }

    /// Compute the camera MVP for this frame. Orbits the scene center.
    fn camera_mvp(&self, aspect: f32) -> Mat4 {
        let (lo, hi) = self.scene_aabb;
        let center = Vec3::new(
            0.5 * (lo[0] + hi[0]),
            0.5 * (lo[1] + hi[1]),
            0.5 * (lo[2] + hi[2]),
        );
        let extent = Vec3::new(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
        let radius = (0.5 * extent.length()).max(1.0);
        let distance = radius / (30f32.to_radians().tan()) * 1.6;
        let angle = self.started_at.elapsed().as_secs_f32() * 0.25;
        let eye = center
            + Vec3::new(
                distance * angle.cos(),
                -distance * 0.4,
                distance * angle.sin(),
            );
        let view = Mat4::look_at_rh(eye, center, Vec3::Y);
        let near = (distance * 0.05).max(0.1);
        let far = distance * 4.0 + 1000.0;
        let proj = Mat4::perspective_rh(60f32.to_radians(), aspect.max(0.01), near, far);
        proj * view
    }

    /// Per-actor model matrix. PSX has Y-down geometry — flip Y in the
    /// model so the meshes appear right-side-up in the Y-up camera.
    fn actor_model(&self, slot: usize) -> Mat4 {
        let a = &self.world.actors[slot];
        let pos = Vec3::new(
            a.move_state.world_x as f32,
            a.move_state.world_y as f32,
            a.move_state.world_z as f32,
        );
        // Slight per-actor spin so individual meshes are visibly animated.
        let spin = self.started_at.elapsed().as_secs_f32() * 0.6
            + (slot as f32) * std::f32::consts::FRAC_PI_2;
        Mat4::from_translation(pos)
            * Mat4::from_rotation_y(spin)
            * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))
    }
}

impl ApplicationHandler for WorldApp {
    fn resumed(&mut self, evl: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = WindowAttributes::default()
            .with_title(&self.title)
            .with_inner_size(winit::dpi::LogicalSize::new(960.0, 720.0));
        let window = match evl.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::error!("create_window: {e:#}");
                evl.exit();
                return;
            }
        };
        let size = window.inner_size();
        let renderer = match Renderer::new(window.clone(), size.width, size.height) {
            Ok(r) => r,
            Err(e) => {
                log::error!("Renderer::new: {e:#}");
                evl.exit();
                return;
            }
        };
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.upload_meshes();
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn window_event(&mut self, evl: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => evl.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size.width, size.height);
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(KeyCode::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => evl.exit(),
            WindowEvent::RedrawRequested => {
                // Tick the world at the actual elapsed dt, capped to one
                // second so a paused window doesn't fast-forward thousands
                // of frames on resume.
                let now = Instant::now();
                let dt = now
                    .duration_since(self.last_tick)
                    .min(std::time::Duration::from_secs(1));
                self.last_tick = now;
                let target_frames = (dt.as_secs_f32() * 60.0).round() as u32;
                for _ in 0..target_frames.min(8) {
                    self.world.tick();
                }
                // Analytic motion for the demo: gently orbit each actor's
                // initial position. Move-VM mode (when wired in) drives
                // positions through the move bytecode instead, so only
                // animate analytically when the VM isn't.
                if !self.with_move_vm {
                    let t = self.started_at.elapsed().as_secs_f32();
                    for slot in 0..self.actor_count {
                        let actor = &mut self.world.actors[slot];
                        let theta = (slot as f32) * std::f32::consts::TAU
                            / (self.actor_count as f32)
                            + t * 0.3;
                        let r = 800.0_f32;
                        actor.move_state.world_x = (r * theta.cos()) as i16;
                        actor.move_state.world_z = (r * theta.sin()) as i16;
                        actor.move_state.world_y = (60.0 * (t + slot as f32).sin()) as i16;
                    }
                }
                if let (Some(r), Some(vram)) = (&self.renderer, self.uploaded_vram.as_ref()) {
                    let (w, h) = r.surface_size();
                    let aspect = w as f32 / h.max(1) as f32;
                    let cam = self.camera_mvp(aspect);
                    let draws: Vec<SceneDraw<'_>> = self
                        .meshes
                        .iter()
                        .enumerate()
                        .map(|(slot, m)| SceneDraw {
                            mesh: &m.mesh,
                            mvp: cam * self.actor_model(slot),
                        })
                        .collect();
                    let scene = RenderScene {
                        vram,
                        draws: &draws,
                        overlay_lines: None,
                        overlay_text: None,
                    };
                    if let Err(e) = r.render(RenderTarget::Scene(&scene)) {
                        log::error!("render error: {e:#}");
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse();

    if let Cmd::Seq {
        seq,
        vab,
        vab_offset,
        looped,
        master_vol,
    } = args.cmd
    {
        return run_seq_playback(&seq, &vab, vab_offset as usize, looped, master_vol);
    }

    if let Cmd::World {
        scene,
        max_actors,
        extracted_root,
        with_move_vm,
    } = args.cmd
    {
        return run_world(&scene, max_actors, &extracted_root, with_move_vm);
    }

    if let Cmd::Field {
        scene,
        max_actors,
        extracted_root,
    } = args.cmd
    {
        return run_field(&scene, max_actors, &extracted_root);
    }

    if let Cmd::Dialog {
        path,
        message,
        extracted_root,
        glyphs_per_frame,
    } = args.cmd
    {
        return run_dialog(&path, message, &extracted_root, glyphs_per_frame);
    }

    let (pending, browser, mesh_browser, stage_browser) = match args.cmd {
        Cmd::Tim { path, clut } => {
            let (rgba, w, h) = load_tim_path(&path, clut)?;
            log::info!("loaded TIM {}x{} ({} bytes RGBA)", w, h, rgba.len());
            (
                Some(Display {
                    title: format!("TIM {}", path.display()),
                    image: Some((rgba, w, h)),
                    audio: None,
                    mesh: None,
                    vram_mesh: None,
                    lines: None,
                }),
                None,
                None,
                None,
            )
        }
        Cmd::Vab {
            path,
            sample,
            offset,
            rate,
        } => {
            let pcm = load_vab_sample(&path, offset as usize, sample)?;
            log::info!(
                "loaded VAB sample {} ({} samples, {} Hz, vab @ 0x{:X})",
                sample,
                pcm.len(),
                rate,
                offset
            );
            (
                Some(Display {
                    title: format!("VAB {} @ 0x{:X} sample {}", path.display(), offset, sample),
                    image: None,
                    audio: Some((pcm, rate)),
                    mesh: None,
                    vram_mesh: None,
                    lines: None,
                }),
                None,
                None,
                None,
            )
        }
        Cmd::Stage { path, start } => {
            if path.is_dir() {
                let mut paths = collect_stage_files(&path);
                paths.sort();
                if paths.is_empty() {
                    anyhow::bail!("no stage-geometry entries found under {}", path.display());
                }
                let start = start.min(paths.len() - 1);
                log::info!(
                    "STAGE browser: {} files under {} (start={})",
                    paths.len(),
                    path.display(),
                    start
                );
                (
                    None,
                    None,
                    None,
                    Some(StageBrowser {
                        paths,
                        current: start,
                        root_label: path.display().to_string(),
                    }),
                )
            } else {
                let payload = load_stage_for_view(&path)?;
                log::info!(
                    "loaded STAGE {} ({} verts, {} lines)",
                    path.display(),
                    payload.positions.len(),
                    payload.indices.len() / 2
                );
                (
                    Some(Display {
                        title: format!(
                            "STAGE {} ({} verts, {} lines)",
                            path.display(),
                            payload.positions.len(),
                            payload.indices.len() / 2
                        ),
                        image: None,
                        audio: None,
                        mesh: None,
                        vram_mesh: None,
                        lines: Some(payload),
                    }),
                    None,
                    None,
                    None,
                )
            }
        }
        Cmd::Tmd {
            path,
            start,
            min_size,
            sort_by_size,
            shape,
            vram_extra_dir,
            bundle,
            scene,
            cdname,
            extracted_root,
        } => {
            // Bundle dirs resolve first, then per-scene CDNAME bundle, then
            // user-specified extras (closer-to-mesh wins on VRAM collisions).
            let mut combined_extras: Vec<PathBuf> = Vec::new();
            if let Some(b) = bundle {
                let dirs = b.dirs(&extracted_root);
                log::info!(
                    "bundle={:?}: prepending {} extra TIM dirs from {}",
                    b,
                    dirs.len(),
                    extracted_root.display()
                );
                combined_extras.extend(dirs);
            }
            if let Some(name) = scene.as_deref() {
                let cdname_path = cdname
                    .clone()
                    .unwrap_or_else(|| extracted_root.join("CDNAME.TXT"));
                let dirs = scene_bundle_dirs(&cdname_path, name, &extracted_root)?;
                log::info!(
                    "scene={}: prepending {} TIM dirs from CDNAME block",
                    name,
                    dirs.len()
                );
                combined_extras.extend(dirs);
            }
            combined_extras.extend(vram_extra_dir);
            let vram_extra_dir = combined_extras;
            if path.is_dir() {
                let mut paths = collect_tmds(&path);
                if min_size > 0 {
                    paths.retain(|p| {
                        std::fs::metadata(p)
                            .map(|m| m.len() >= min_size)
                            .unwrap_or(false)
                    });
                }
                if !matches!(shape, ShapeFilter::Any) {
                    paths.retain(|p| shape.accepts(p));
                }
                if sort_by_size {
                    paths.sort_by_cached_key(|p| {
                        std::cmp::Reverse(std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
                    });
                } else {
                    paths.sort();
                }
                if paths.is_empty() {
                    anyhow::bail!(
                        "no .tmd files{} found under {}",
                        if min_size > 0 {
                            format!(" ≥ {} bytes", min_size)
                        } else {
                            String::new()
                        },
                        path.display()
                    );
                }
                let start = start.min(paths.len() - 1);
                log::info!(
                    "TMD browser: {} files under {} (start={}, min_size={}, sort_by_size={})",
                    paths.len(),
                    path.display(),
                    start,
                    min_size,
                    sort_by_size,
                );
                (
                    None,
                    None,
                    Some(MeshBrowser {
                        paths,
                        current: start,
                        root_label: path.display().to_string(),
                        vram_extras: vram_extra_dir,
                    }),
                    None,
                )
            } else {
                let display = match load_tmd_for_view(&path, &vram_extra_dir)? {
                    TmdViewData::Flat { positions, indices } => {
                        log::info!(
                            "loaded TMD {} ({} verts, {} tris) [no paired TIM]",
                            path.display(),
                            positions.len(),
                            indices.len() / 3
                        );
                        Display {
                            title: format!(
                                "TMD {} ({} verts, {} tris)",
                                path.display(),
                                positions.len(),
                                indices.len() / 3
                            ),
                            image: None,
                            audio: None,
                            mesh: Some((positions, indices)),
                            vram_mesh: None,
                            lines: None,
                        }
                    }
                    TmdViewData::Vram(payload) => {
                        log::info!(
                            "loaded TMD {} ({} tri-verts) with VRAM ({} TIMs from {})",
                            path.display(),
                            payload.indices.len() / 3,
                            payload.tim_count,
                            payload.tim_dir_label
                        );
                        Display {
                            title: format!(
                                "TMD {} (vram: {} TIMs from {})",
                                path.display(),
                                payload.tim_count,
                                payload.tim_dir_label
                            ),
                            image: None,
                            audio: None,
                            mesh: None,
                            vram_mesh: Some(payload),
                            lines: None,
                        }
                    }
                };
                (Some(display), None, None, None)
            }
        }
        Cmd::Prot {
            prot_dat,
            cdname: cdname_path,
            start,
        } => {
            let archive =
                Archive::open(&prot_dat).with_context(|| format!("open {}", prot_dat.display()))?;
            let cdname = if let Some(p) = cdname_path {
                Some(cdname::parse(&p).with_context(|| format!("parse {}", p.display()))?)
            } else {
                None
            };
            log::info!(
                "PROT {}: {} entries, starting at {}",
                prot_dat.display(),
                archive.entries.len(),
                start
            );
            (
                None,
                Some(Browser {
                    archive,
                    cdname,
                    current: start,
                    last_count: 0,
                }),
                None,
                None,
            )
        }
        Cmd::Seq { .. } => unreachable!("Cmd::Seq handled by the early-return path above"),
        Cmd::World { .. } => unreachable!("Cmd::World handled by the early-return path above"),
        Cmd::Field { .. } => unreachable!("Cmd::Field handled by the early-return path above"),
        Cmd::Dialog { .. } => unreachable!("Cmd::Dialog handled by the early-return path above"),
    };

    let event_loop = EventLoop::new().context("create event loop")?;
    // The TMD viewer needs continuous redraws to animate; everything else
    // can wait on input/redraw events.
    let needs_animation = pending
        .as_ref()
        .is_some_and(|d| d.mesh.is_some() || d.vram_mesh.is_some() || d.lines.is_some())
        || mesh_browser.is_some()
        || stage_browser.is_some();
    event_loop.set_control_flow(if needs_animation {
        ControlFlow::Poll
    } else {
        ControlFlow::Wait
    });

    let mut app = App {
        window: None,
        renderer: None,
        audio: None,
        texture: None,
        mesh: None,
        vram_mesh: None,
        vram: None,
        lines: None,
        mesh_view: None,
        pending,
        browser,
        mesh_browser,
        stage_browser,
    };
    event_loop.run_app(&mut app).context("run event loop")?;
    Ok(())
}

/// Loader result: either a flat-shaded mesh, or a VRAM-textured one with
/// every TIM in the source PROT entry uploaded to a software VRAM. Picked
/// by whether [`sibling_tim_dir`] turns up a directory with TIMs in it.
enum TmdViewData {
    Flat {
        positions: Vec<[f32; 3]>,
        indices: Vec<u32>,
    },
    Vram(VramMeshPayload),
}

fn load_tmd_for_view(tmd_path: &Path, vram_extras: &[PathBuf]) -> Result<TmdViewData> {
    let bytes = std::fs::read(tmd_path).with_context(|| format!("read {}", tmd_path.display()))?;
    let tmd =
        legaia_tmd::parse(&bytes).with_context(|| format!("parse TMD {}", tmd_path.display()))?;
    let sibling = sibling_tim_dir(tmd_path);
    if sibling.is_some() || !vram_extras.is_empty() {
        let vram_mesh = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &bytes);
        if !vram_mesh.indices.is_empty() {
            // Order: extras first (shared/base data), sibling last (so the
            // mesh's own scene data overlays the base on collision).
            let mut dirs: Vec<&Path> = vram_extras.iter().map(|p| p.as_path()).collect();
            if let Some(s) = sibling.as_ref() {
                dirs.push(s.as_path());
            }
            let (vram, tim_count) = build_vram_from_dirs(&dirs);
            if tim_count > 0 {
                warn_unfilled_cluts(&vram_mesh, &vram);
                let label = match sibling.as_ref() {
                    Some(s) if vram_extras.is_empty() => short_path(s),
                    Some(s) => format!("{} + {} extra dir(s)", short_path(s), vram_extras.len()),
                    None => format!("{} extra dir(s)", vram_extras.len()),
                };
                return Ok(TmdViewData::Vram(VramMeshPayload {
                    positions: vram_mesh.positions,
                    uvs: vram_mesh.uvs,
                    cba_tsb: vram_mesh.cba_tsb,
                    normals: vram_mesh.normals,
                    indices: vram_mesh.indices,
                    vram,
                    tim_count,
                    tim_dir_label: label,
                }));
            }
        }
    }
    let mesh = legaia_tmd::mesh::tmd_to_mesh(&tmd, &bytes);
    Ok(TmdViewData::Flat {
        positions: mesh.positions,
        indices: mesh.indices,
    })
}

/// Walk each dir in order, uploading every `.tim` to a single fresh VRAM.
/// Later dirs overwrite earlier ones at overlapping VRAM addresses
/// (matches PSX hardware: later DMA writes win). Bad / unparseable TIMs
/// are skipped silently. Returns the VRAM and the total successful uploads.
fn build_vram_from_dirs(dirs: &[&Path]) -> (Vram, usize) {
    let mut vram = Vram::new();
    let mut count = 0usize;
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("tim")) {
                continue;
            }
            let Ok(buf) = std::fs::read(&p) else {
                continue;
            };
            let Ok(tim) = legaia_tim::parse(&buf) else {
                continue;
            };
            vram.upload_tim(&tim);
            count += 1;
        }
    }
    (vram, count)
}

/// Diagnostic: scan distinct CBA values referenced by the mesh and check
/// whether the corresponding CLUT row in VRAM has any non-zero data.
/// Empty rows mean the CLUT lives in a PROT entry we didn't load — the
/// user probably wants to add it via `--vram-extra-dir`.
fn warn_unfilled_cluts(mesh: &legaia_tmd::mesh::VramMesh, vram: &Vram) {
    let mut missing_rows: std::collections::BTreeSet<u16> = std::collections::BTreeSet::new();
    let mut seen_cba: std::collections::BTreeSet<u16> = std::collections::BTreeSet::new();
    for ct in &mesh.cba_tsb {
        let cba = ct[0];
        if cba == 0 || !seen_cba.insert(cba) {
            continue;
        }
        let cy = ((cba >> 6) & 0x1FF) as usize;
        let cx_base = ((cba & 0x3F) * 16) as usize;
        // Sample 16 entries (one 4bpp palette). If all zero, this CLUT
        // wasn't uploaded — we'd render this prim with garbage.
        let any = (0..16).any(|i| vram.pixel(cx_base + i, cy) != 0);
        if !any {
            missing_rows.insert(cba >> 6);
        }
    }
    if !missing_rows.is_empty() {
        log::warn!(
            "VRAM is missing CLUT data for rows {:?} — mesh prims will sample zeros. Try --vram-extra-dir extracted/tim_scan/0866_battle_data (battle palettes are shared across level_up / town entries).",
            missing_rows.iter().collect::<Vec<_>>()
        );
    }
}

/// Find the TIM directory that holds every TIM from the same PROT entry
/// as `tmd_path`. Convention: the bulk-scan extractors write TMDs to
/// `extracted/tmd_scan/<entry>/raw_off<HEX>.tmd` and TIMs to
/// `extracted/tim_scan/<entry>/raw_off<HEX>_<W>x<H>_<BPP>bpp.tim`.
/// Returns the matching `tim_scan/<entry>/` if it exists.
fn sibling_tim_dir(tmd_path: &Path) -> Option<PathBuf> {
    let entry_dir = tmd_path.parent()?;
    let entry_name = entry_dir.file_name()?;
    let scan_root = entry_dir.parent()?.parent()?; // up two: tmd_scan → extracted
    let tim_dir = scan_root.join("tim_scan").join(entry_name);
    tim_dir.is_dir().then_some(tim_dir)
}

/// Build a renderable wireframe payload from a stage-geometry PROT entry.
/// Each record becomes a line loop (4 segments — degenerate triangle quads
/// collapse one edge naturally). Vertex coords come from the parsed pool;
/// PSX is Y-down so we flip Y at upload so up-is-up in the viewer camera.
fn load_stage_for_view(path: &Path) -> Result<LinesPayload> {
    let raw = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let stage = legaia_asset::stage_geom::parse(&raw)
        .ok_or_else(|| anyhow::anyhow!("no stage table in {}", path.display()))?;
    let largest = stage
        .tables
        .iter()
        .max_by_key(|t| t.records)
        .ok_or_else(|| anyhow::anyhow!("empty table list"))?;

    let vert_count = stage.vertex_count();
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(vert_count);
    let mut colors: Vec<[u8; 4]> = Vec::with_capacity(vert_count);
    // Walk vertex pool to compute AABB on Y for depth-shaded coloring.
    let mut min_y: f32 = f32::MAX;
    let mut max_y: f32 = f32::MIN;
    for i in 0..vert_count {
        let v = stage.vertex(&raw, i).expect("in range");
        let py = -(v.y as f32); // PSX Y-down -> renderer Y-up
        if py < min_y {
            min_y = py;
        }
        if py > max_y {
            max_y = py;
        }
        positions.push([v.x as f32, py, v.z as f32]);
        colors.push([0xFF, 0xFF, 0xFF, 0xFF]); // overwritten below
    }
    let y_span = (max_y - min_y).max(1.0);
    for (i, c) in colors.iter_mut().enumerate() {
        let py = positions[i][1];
        // Cool→warm gradient on Y so the eye reads height. Top = warm.
        let t = ((py - min_y) / y_span).clamp(0.0, 1.0);
        let r = (60.0 + 195.0 * t) as u8;
        let g = (140.0 + 60.0 * (1.0 - (t - 0.5).abs() * 2.0)) as u8;
        let b = (220.0 - 160.0 * t) as u8;
        *c = [r, g, b, 0xFF];
    }

    // Build line indices: for each in-range record, emit 4 segments
    // forming the quad outline. Degenerate quads (idx[3] == idx[0]) just
    // add one zero-length edge — harmless.
    let mut indices: Vec<u32> = Vec::with_capacity(largest.records * 8);
    let mut emitted = 0usize;
    let mut skipped = 0usize;
    for rec in legaia_asset::stage_geom::records(&raw, largest) {
        let Some(idx) = stage.quad_vertex_indices(&rec) else {
            skipped += 1;
            continue;
        };
        // Range check (parse already guarantees but belt-and-braces).
        if idx.iter().any(|&i| i >= vert_count) {
            skipped += 1;
            continue;
        }
        let a = idx[0] as u32;
        let b = idx[1] as u32;
        let c = idx[2] as u32;
        let d = idx[3] as u32;
        // Quad outline: a-b, b-c, c-d, d-a.
        indices.extend_from_slice(&[a, b, b, c, c, d, d, a]);
        emitted += 1;
    }
    if skipped > 0 {
        log::warn!(
            "stage {}: skipped {} of {} records (out-of-range indices)",
            path.display(),
            skipped,
            largest.records
        );
    }
    log::info!(
        "stage {}: {} verts, {} records -> {} line segments",
        path.display(),
        vert_count,
        emitted,
        indices.len() / 2,
    );
    Ok(LinesPayload {
        positions,
        colors,
        indices,
    })
}

/// Walk `root` for files that parse as stage-geometry PROT entries. Used
/// by `stage <DIR>` mode to skip non-stage entries during navigation.
fn collect_stage_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(root) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_file() {
            continue;
        }
        let Ok(raw) = std::fs::read(&path) else {
            continue;
        };
        if legaia_asset::stage_geom::parse(&raw).is_some() {
            out.push(path);
        }
    }
    out
}

/// Recursively collect every `*.tmd` file (case-insensitive) under `root`.
/// Symlinks are not followed; unreadable subdirectories are skipped silently.
fn collect_tmds(root: &Path) -> Vec<PathBuf> {
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

/// Trim a path for window-title use: keep the last 3 components.
fn short_path(p: &Path) -> String {
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

fn mesh_aabb(positions: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
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

// ---------------------------------------------------------------------------
// Dialog viewer
// ---------------------------------------------------------------------------

/// Boot the dialog runner. Loads the MES blob, picks the message at
/// `message_index`, and hands it to a winit-driven [`DialogApp`].
fn run_dialog(
    path: &Path,
    message_index: usize,
    extracted_root: &Path,
    glyphs_per_frame: u8,
) -> Result<()> {
    let buf = std::fs::read(path).with_context(|| format!("read MES {}", path.display()))?;
    // Parse the blob to confirm it's a compact MES with an offset table; we
    // re-parse inside `DialogApp` so the interpreter borrows `buf` directly.
    let blob = legaia_mes::parse(&buf).with_context(|| format!("parse MES {}", path.display()))?;
    let table_len = blob
        .offset_table
        .as_ref()
        .map(|t| t.len())
        .unwrap_or_default();
    if table_len == 0 {
        anyhow::bail!(
            "MES {} has no offset table — only Compact-format blobs are renderable",
            path.display()
        );
    }
    if message_index >= table_len {
        anyhow::bail!(
            "message {} out of range (offset table has {} entries)",
            message_index,
            table_len,
        );
    }

    let font = Font::load_from_extracted(extracted_root).with_context(|| {
        format!(
            "load extracted font under {} (run `font-extract` first?)",
            extracted_root.display()
        )
    })?;

    let event_loop = EventLoop::new().context("create event loop")?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = DialogApp {
        title: format!("MES {} — message {}", path.display(), message_index),
        path_label: path.display().to_string(),
        message_index,
        message_count: table_len,
        buf,
        font,
        glyphs_per_frame: glyphs_per_frame.max(1),
        page_glyphs: Vec::new(),
        log: Vec::new(),
        page_break: false,
        done: false,
        frame_count: 0,
        window: None,
        renderer: None,
        font_atlas: None,
    };
    app.reset_player()
        .with_context(|| "build initial dialog player")?;
    event_loop.run_app(&mut app).context("event loop")?;
    Ok(())
}

/// Dialog viewer state. Owns the MES buffer, the font, and a running
/// [`legaia_mes::DialogPlayer`] that emits one event per render frame.
struct DialogApp {
    title: String,
    path_label: String,
    message_index: usize,
    message_count: usize,
    buf: Vec<u8>,
    font: Font,
    glyphs_per_frame: u8,
    /// Glyph bytes that have been "typed out" so far on the current page.
    /// Reset on page break dismissal.
    page_glyphs: Vec<u8>,
    /// Last 3 control / unknown events for the status line.
    log: Vec<String>,
    page_break: bool,
    done: bool,
    frame_count: u64,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    font_atlas: Option<UploadedFontAtlas>,
}

impl DialogApp {
    /// Pull events from a freshly-built [`legaia_mes::DialogPlayer`] until
    /// the stream blocks (page break, done, idle, or the buffer ends), then
    /// return how many glyphs were appended this frame.
    ///
    /// Each `tick` of `DialogPlayer` advances at most one event when
    /// pacing allows, so we run it once per frame and stash the result in
    /// `page_glyphs` / `log`.
    fn step_player(&mut self) {
        if self.done || self.page_break {
            return;
        }
        // Build the player on each frame: cheap, and avoids a self-referential
        // borrow of `self.buf`.
        let mut player = match self.build_player() {
            Ok(p) => p,
            Err(e) => {
                log::error!("rebuild dialog player: {e:#}");
                self.done = true;
                return;
            }
        };
        // Replay glyphs already emitted this page so the player is in sync —
        // but only the count, not the events: the interpreter resumes from
        // the beginning of the message, so we discard `self.page_glyphs.len()`
        // events to fast-forward.
        for _ in 0..self.page_glyphs.len() {
            let _ = player.tick();
        }
        // Do one pacing tick: emit at most one new event.
        match player.tick() {
            legaia_mes::PlayerState::Idle => {}
            legaia_mes::PlayerState::Glyph(g) => self.page_glyphs.push(g),
            legaia_mes::PlayerState::PageBreak => {
                self.page_break = true;
                self.push_log("[PAGE]".to_string());
            }
            legaia_mes::PlayerState::WaitingForInput => {
                self.page_break = true;
            }
            legaia_mes::PlayerState::Control(ev) => {
                self.push_log(format!("{ev:?}"));
            }
            legaia_mes::PlayerState::Done => {
                self.done = true;
                self.push_log("[END]".to_string());
            }
        }
    }

    fn build_player(&self) -> Result<legaia_mes::DialogPlayer<'_>> {
        // `Interpreter::new_compact` borrows `&MesBlob`, which would tie the
        // returned player's lifetime to a local. Recompute the PC from the
        // offset table and use `Interpreter::new_at` to avoid the borrow.
        let blob = legaia_mes::parse(&self.buf)?;
        let table = blob.offset_table.as_ref().ok_or_else(|| {
            anyhow::anyhow!("MES blob has no offset table — only Compact format supported")
        })?;
        let entry = table
            .get(self.message_index)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("message index {} out of range", self.message_index))?;
        let bytecode_off = blob
            .bytecode_offset
            .ok_or_else(|| anyhow::anyhow!("MES blob has no bytecode offset (not Compact?)"))?;
        let pc = bytecode_off + entry as usize;
        if pc >= self.buf.len() {
            anyhow::bail!("computed pc {pc} past buffer end {}", self.buf.len());
        }
        let interp = legaia_mes::Interpreter::new_at(&self.buf, pc);
        let mut player = legaia_mes::DialogPlayer::new(interp);
        player.set_glyphs_per_frame(self.glyphs_per_frame);
        Ok(player)
    }

    /// Reset the per-page state for `self.message_index`. Called at startup
    /// and whenever the user picks a different message.
    fn reset_player(&mut self) -> Result<()> {
        // Validate the message index is reachable now.
        let _ = self.build_player()?;
        self.page_glyphs.clear();
        self.log.clear();
        self.page_break = false;
        self.done = false;
        Ok(())
    }

    fn push_log(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > 3 {
            let drain = self.log.len() - 3;
            self.log.drain(..drain);
        }
    }

    /// Build the per-frame [`TextDraw`] list: the dialog window's text plus
    /// a status footer.
    fn build_text(&self, surface: (u32, u32)) -> Vec<TextDraw> {
        let Some(_atlas) = &self.font_atlas else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let white = [1.0, 1.0, 1.0, 1.0];
        let dim = [0.7, 0.85, 1.0, 1.0];
        let yellow = [1.0, 0.95, 0.55, 1.0];

        // Top-left header.
        let header = format!(
            "MES  msg {} / {}  ({} bytes)  fr {}",
            self.message_index,
            self.message_count,
            self.buf.len(),
            self.frame_count
        );
        let layout = self.font.layout_ascii(&header);
        out.extend(text_draws_for(&layout, (8, 8), dim));

        // Path on second line.
        let layout = self.font.layout_ascii(&self.path_label);
        out.extend(text_draws_for(&layout, (8, 26), dim));

        // Centered "dialog box" — pen near the lower-third of the surface.
        let (w, h) = surface;
        let pen_x = ((w as i32) / 6).max(16);
        let pen_y = ((h as i32) * 2 / 3).max(64);
        let layout = self.font.layout(&self.page_glyphs);
        out.extend(text_draws_for(&layout, (pen_x, pen_y), white));

        // Status footer.
        let footer = if self.done {
            "[end of message]   N: next message    P: previous   R: reset    Esc: quit".to_string()
        } else if self.page_break {
            "[page break]   Z / Enter: continue    R: reset    Esc: quit".to_string()
        } else {
            format!(
                "playing... {} glyphs   pace {} fr/glyph    R: reset    Esc: quit",
                self.page_glyphs.len(),
                self.glyphs_per_frame,
            )
        };
        let layout = self.font.layout_ascii(&footer);
        out.extend(text_draws_for(&layout, (8, (h as i32) - 36), yellow));

        // Recent control log.
        let mut log_y = (h as i32) - 22;
        for line in self.log.iter().rev().take(3) {
            let layout = self.font.layout_ascii(line);
            out.extend(text_draws_for(&layout, (8, log_y), dim));
            log_y -= 14;
        }
        out
    }

    fn upload_font(&mut self) {
        let Some(r) = &self.renderer else {
            return;
        };
        let (aw, ah) = self.font.atlas_dimensions();
        match r.upload_font_atlas(self.font.atlas_rgba(), aw, ah) {
            Ok(a) => self.font_atlas = Some(a),
            Err(e) => log::error!("font atlas upload failed: {e:#}"),
        }
    }

    fn advance_page(&mut self) {
        if self.page_break {
            self.page_break = false;
            // Drop the typed-out glyphs of the prior page so the player
            // continues into the next page from a fresh visual state.
            self.page_glyphs.clear();
        }
    }

    fn jump_message(&mut self, delta: i32) {
        let next = (self.message_index as i32) + delta;
        if next < 0 || (next as usize) >= self.message_count {
            return;
        }
        self.message_index = next as usize;
        if let Err(e) = self.reset_player() {
            log::warn!("can't jump to message {next}: {e:#}");
        }
    }
}

impl ApplicationHandler for DialogApp {
    fn resumed(&mut self, evl: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = WindowAttributes::default()
            .with_title(&self.title)
            .with_inner_size(winit::dpi::LogicalSize::new(720.0, 480.0));
        let window = match evl.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::error!("create_window: {e:#}");
                evl.exit();
                return;
            }
        };
        let size = window.inner_size();
        let renderer = match Renderer::new(window.clone(), size.width, size.height) {
            Ok(r) => r,
            Err(e) => {
                log::error!("Renderer::new: {e:#}");
                evl.exit();
                return;
            }
        };
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.upload_font();
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn window_event(&mut self, evl: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => evl.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size.width, size.height);
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => match code {
                KeyCode::Escape => evl.exit(),
                KeyCode::KeyZ | KeyCode::Enter => self.advance_page(),
                KeyCode::KeyR => {
                    if let Err(e) = self.reset_player() {
                        log::warn!("reset failed: {e:#}");
                    }
                }
                KeyCode::KeyN | KeyCode::PageDown => self.jump_message(1),
                KeyCode::KeyP | KeyCode::PageUp => self.jump_message(-1),
                _ => {}
            },
            WindowEvent::RedrawRequested => {
                self.frame_count += 1;
                self.step_player();
                if let (Some(r), Some(atlas)) = (&self.renderer, self.font_atlas.as_ref()) {
                    let (w, h) = r.surface_size();
                    let draws = self.build_text((w, h));
                    let overlay = TextOverlay {
                        atlas,
                        draws: &draws,
                    };
                    if let Err(e) = r.render(RenderTarget::TextOnly(&overlay)) {
                        log::error!("render error: {e:#}");
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }
}
