//! Phase 1 asset viewer.
//!
//! Modes:
//!
//! * `tim <PATH>` - display a single TIM file (the original Phase 1 demo).
//! * `tmd <PATH>` - display a Legaia TMD as a flat-shaded auto-rotating
//!   3D mesh. PATH may be a single `.tmd` file, or a directory in which
//!   case the viewer walks every `*.tmd` recursively and N/P/PgDn/PgUp
//!   cycle between them.
//! * `stage <PATH>` - render a stage-geometry PROT entry as a wireframe.
//! * `vab <PATH>` - load a VAB bank, play one sample.
//! * `prot <PROT.DAT>` - browse every PROT entry. Auto-detects what the
//!   entry contains via the categorize classifier and shows / plays the
//!   first viewable sub-asset. Currently handles:
//!   - `tim_passthrough` → display the TIM
//!   - `tim_pack` → display first sub-TIM that decodes
//!   - `data_field_streaming` → display first TIM_LIST sub-pack TIM
//!   - `scene_tmd_stream` → render the leading TMD as flat-shaded mesh
//!     (148 PROT entries, 2026-05-04)
//!   - `scene_vab_stream` → play sample 0 of the leading VAB bank
//!     (217 PROT entries, 2026-05-04)
//!   - VAB byte-search fallback for any class - finds VAB headers anywhere
//!     in the buffer (covers `unknown_*` entries with embedded banks)
//!
//! The PROT browser is the integration de-risk for Phase 1: proves the
//! engine plumbing handles classification → typed parse → asset upload
//! across a real disc dump, not just one hand-picked file.
//!
//! The per-mode implementations live in sibling submodules: single-file /
//! browser modes in [`browser`], the field / battle / world / dialog demos
//! in [`field_app`] / [`battle_app`] / [`world_app`] / [`dialog_app`], SEQ
//! playback in [`seq_play`], and the shared loaders / view payloads in
//! [`loaders`] / [`tmd_view`] / [`stage_view`] / [`display`] / [`common`].

mod battle_app;
mod browser;
mod cli;
mod common;
mod dialog_app;
mod display;
mod field_app;
mod loaders;
mod seq_play;
mod stage_view;
mod tmd_view;
mod world_app;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use legaia_engine_audio::DEFAULT_INPUT_RATE;
use legaia_prot::{archive::Archive, cdname};
use std::path::PathBuf;
use winit::event_loop::{ControlFlow, EventLoop};

use crate::battle_app::run_battle_scene;
use crate::browser::{App, Browser, MeshBrowser, StageBrowser};
use crate::cli::{Bundle, ShapeFilter, parse_hex_u64, scene_bundle_dirs};
use crate::common::collect_tmds;
use crate::dialog_app::run_dialog;
use crate::display::Display;
use crate::field_app::run_field;
use crate::loaders::{load_tim_path, load_tim_path_at_offset, load_vab_sample};
use crate::seq_play::run_seq_playback;
use crate::stage_view::{collect_stage_files, load_stage_for_view};
use crate::tmd_view::{TmdViewData, load_tmd_for_view};
use crate::world_app::run_world;

#[derive(Parser, Debug)]
#[command(about = "Asset viewer for Legend of Legaia.")]
struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Display a single TIM in a window. PATH may be a standalone
    /// `.tim` file, a PROT entry containing a TIM at its start, or
    /// `extracted/PROT.DAT` itself paired with `--offset` for TIMs
    /// in the unindexed pre-`init_data` gap (system-UI sheet at
    /// `0x018E0`, menu-glyph atlas at `0x11218`, etc.).
    Tim {
        path: PathBuf,
        /// CLUT index to use (0 by default, ignored for direct-color TIMs).
        #[arg(long, default_value_t = 0)]
        clut: usize,
        /// Byte offset where the TIM header lives. Default 0 = read
        /// from the start of the file. Use hex (e.g. `0x018E0`) for
        /// known gap-resident TIMs in `PROT.DAT`.
        #[arg(long, value_parser = parse_hex_u64, default_value_t = 0)]
        offset: u64,
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
        /// character-scale meshes - try `--min-size 15000` for `battle_data`.
        #[arg(long, default_value_t = 0)]
        min_size: u64,
        /// Sort by file size descending (largest first) instead of by path.
        /// Pairs well with `--min-size` to surface main characters first.
        #[arg(long, default_value_t = false)]
        sort_by_size: bool,
        /// Filter directory listing by AABB shape (`character` = tall meshes
        /// w/ height > 1.5 × horizontal extent - Legaia heroes/NPCs;
        /// `arena` = wide flat meshes w/ height < 0.5 × horizontal extent -
        /// battle stages; `any` = no filter). Cheaper than `--min-size` for
        /// finding actual characters since size alone doesn't distinguish
        /// hero meshes from arena props.
        #[arg(long, value_enum, default_value_t = ShapeFilter::Any)]
        shape: ShapeFilter,
        /// Extra TIM directory to overlay into VRAM, before the mesh's own
        /// sibling tim_scan dir. Use this when a mesh's prims reference
        /// CLUT rows that live in a different PROT entry - e.g. level_up
        /// character meshes share their CLUTs with battle_data, so:
        ///   `--vram-extra-dir extracted/tim_scan/0866_battle_data`
        /// Repeatable. Order matters: each dir's TIMs overwrite earlier ones
        /// at overlapping VRAM addresses (matches PSX hardware DMA).
        #[arg(long)]
        vram_extra_dir: Vec<PathBuf>,
        /// Pre-load a known scene bundle into VRAM. Encodes the PROT entry
        /// sets that the runtime asset loader (`FUN_800520f0` + relatives in
        /// SCUS_942.54) loads together for a given scene. Use `battle` for
        /// any mesh in `level_up`, `battle_data`, or `monster_se` - these
        /// share the same character-body palette set.
        ///
        /// Acts as a shorthand for several `--vram-extra-dir` flags. Bundle
        /// dirs are loaded BEFORE explicit `--vram-extra-dir` paths.
        #[arg(long, value_enum)]
        bundle: Option<Bundle>,
        /// CDNAME-resolved per-scene bundle. Adds tim_scan dirs for every
        /// PROT entry in the named CDNAME block - mirrors what the runtime
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
        /// Skip the VRAM textured path entirely; render unlit flat-shaded
        /// geometry. Useful when you want to inspect a mesh's silhouette
        /// without battling palette guesses (the runtime LoadImage trace
        /// for field/town scenes is not yet available, so some palette
        /// rows always render as garbage). Aliased as `--flat-shaded`.
        #[arg(long, alias = "flat-shaded", default_value_t = false)]
        no_textures: bool,
    },
    /// Boot a CDNAME scene as a playable field-mode demo: opens a window,
    /// uploads the scene's TMDs as actors, wires keyboard / gamepad input
    /// through `engine-core::input`, and overlays HUD text rendered via the
    /// extracted dialog font. When the scene carries an event-script PROT
    /// entry (`SceneEventScripts` or `SceneScriptedAssetTable`), the field
    /// VM is wired to drive its records - the HUD shows VM PC, last
    /// StepResult, and a running tally of opcode types observed
    /// (Advance / Yield / Halt / Pending / Unknown) so missing FieldHost
    /// hooks surface immediately. Scenes with no event-script entry fall
    /// back to camera spin + manual input.
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
        /// Initial event-script record to load. Most scenes have one
        /// "scene-enter" record at index 0; later indices typically hold
        /// per-NPC dialogue, pickup, and trigger scripts.
        #[arg(long, default_value_t = 0)]
        record: usize,
        /// When the active record reaches Halt or Unknown, automatically
        /// advance to the next record. Defaults on so a single session
        /// exercises every record in order.
        #[arg(long, default_value_t = true)]
        cycle_records: bool,
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
    /// Boot a battle scene driven by the engine-vm battle-action state
    /// machine. Loads the canonical battle bundle (PROT 865-890), spawns
    /// 3 party + 5 monster actor slots, and ticks `World::tick` each frame
    /// in `SceneMode::Battle`. HUD shows the current `ActionState`,
    /// queued action, per-slot liveness, and any `BattleEndCause` the SM
    /// emits. Cycles through canned actions on input so the loop is
    /// visible end-to-end.
    BattleScene {
        /// `extracted/` root containing PROT.DAT + CDNAME.TXT + tmd_scan/
        /// + tim_scan/.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Initial queued action ID (0=Tactical Arts, 1=Item, 2=Magic,
        /// 3=Attack, 4=Spirit, 5=Run). Default 3 (Attack).
        #[arg(long, default_value_t = 3)]
        queued_action: u8,
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

    if let Cmd::BattleScene {
        extracted_root,
        queued_action,
    } = args.cmd
    {
        return run_battle_scene(&extracted_root, queued_action);
    }

    if let Cmd::Field {
        scene,
        max_actors,
        extracted_root,
        record,
        cycle_records,
    } = args.cmd
    {
        return run_field(&scene, max_actors, &extracted_root, record, cycle_records);
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
        Cmd::Tim { path, clut, offset } => {
            let (rgba, w, h) = if offset > 0 {
                load_tim_path_at_offset(&path, offset, clut)?
            } else {
                load_tim_path(&path, clut)?
            };
            log::info!(
                "loaded TIM {}x{} ({} bytes RGBA){}",
                w,
                h,
                rgba.len(),
                if offset > 0 {
                    format!(" @ 0x{offset:X}")
                } else {
                    String::new()
                }
            );
            let title = if offset > 0 {
                format!("TIM {} @ 0x{:X} (CLUT {})", path.display(), offset, clut)
            } else {
                format!("TIM {}", path.display())
            };
            (
                Some(Display {
                    title,
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
            no_textures,
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
                        no_textures,
                    }),
                    None,
                )
            } else {
                let display = match load_tmd_for_view(&path, &vram_extra_dir, no_textures)? {
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
        Cmd::BattleScene { .. } => {
            unreachable!("Cmd::BattleScene handled by the early-return path above")
        }
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
