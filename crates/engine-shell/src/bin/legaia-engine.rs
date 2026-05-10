//! Top-level engine driver. The "single command" that turns extracted-disc
//! bytes into a runtime view of any CDNAME scene.
//!
//! Subcommands:
//!
//! - `info` — headless one-line summary of a scene's resolved asset chain.
//! - `list-scenes` — every CDNAME scene name with its PROT range.
//! - `play` — headless engine tick: world + camera + audio, no window.
//! - `play-window` — windowed engine: opens a wgpu surface, renders scene
//!   TMDs against the software PSX VRAM each frame. Input: arrows = D-pad,
//!   Z = Cross, Esc = quit.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use glam::{Mat4, Vec3};
use legaia_engine_core::menu_runtime::{MenuInput, MenuRuntime, MenuState};
use legaia_engine_core::scene::{ProtIndex, Scene, SceneTickEvent};
use legaia_engine_core::scene_assets::SceneAssets;
use legaia_engine_core::scene_resources::SceneResources;
use legaia_engine_core::world::{AnimPlayer, SceneMode};
use legaia_engine_core::world_map::WorldMapController;
use legaia_engine_render::{
    RenderTarget, Scene as RenderScene, SceneDraw, ShopRow, TextDraw, TextOverlay,
    UploadedFontAtlas, UploadedVram, UploadedVramMesh, level_up_draws_for, shop_draws_for,
    text_draws_for,
    window::{EngineWindow, orbit_camera_mvp},
};
use legaia_engine_shell::{BootConfig, BootSession};
use legaia_font::Font;
use legaia_prot::cdname;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowId;

#[derive(Parser, Debug)]
#[command(
    name = "legaia-engine",
    about = "Top-level driver for the Legaia clean-room engine. Boots a CDNAME scene from extracted PROT bytes."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Build [`SceneResources`] for one scene and print a summary line. Use
    /// this to verify the asset chain produces the right state without
    /// firing up the windowed viewer.
    Info {
        /// CDNAME scene name (e.g. `town01`, `dolk`, `cave01`).
        #[arg(long)]
        scene: String,
        /// Extracted-root directory containing `PROT.DAT` + `CDNAME.TXT`.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Alternative source: read PROT.DAT + CDNAME.TXT directly from a
        /// `.bin` disc image. When provided, `--extracted-root` is ignored.
        #[arg(long)]
        disc: Option<PathBuf>,
    },
    /// List every distinct scene name the CDNAME map exposes, with the
    /// PROT entry range each one covers.
    ListScenes {
        /// Extracted-root directory containing `CDNAME.TXT`.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Alternative source: read CDNAME.TXT directly from a `.bin`
        /// disc image. When provided, `--extracted-root` is ignored.
        #[arg(long)]
        disc: Option<PathBuf>,
    },
    /// Save the current world's empty/default party to a slot file.
    /// Useful as a smoke test for the disk save path; engines drive this
    /// from menu mode at runtime.
    Save {
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Alternative source: read PROT.DAT + CDNAME.TXT directly from a
        /// `.bin` disc image. When provided, `--extracted-root` is ignored.
        #[arg(long)]
        disc: Option<PathBuf>,
        #[arg(long, default_value = "saves")]
        save_dir: PathBuf,
        #[arg(long, default_value_t = 0)]
        slot: u8,
        /// Number of party-record entries to materialise in the save.
        #[arg(long, default_value_t = 3)]
        party_size: usize,
    },
    /// Load a slot file into a fresh world and print the resulting roster
    /// shape. Mirror of `save` for round-trip testing.
    Load {
        #[arg(long, default_value = "saves")]
        save_dir: PathBuf,
        #[arg(long, default_value_t = 0)]
        slot: u8,
    },
    /// Boot the engine into a scene and tick it for `frames` frames.
    /// Drives the field VM, camera, BGM director, and per-actor move VMs;
    /// logs scene transitions and the per-frame BGM events. No window —
    /// for that, use `asset-viewer field <scene>`.
    ///
    /// When `--str-file` is provided the STR video is pre-decoded headlessly
    /// (frame count logged) before scene ticking begins. The scene label
    /// patterns that identify in-engine cutscenes (as opposed to FMV) are
    /// described by `engine_core::scene::is_cutscene_label`.
    Play {
        /// Starting scene name. Default: `town01`.
        #[arg(long, default_value = "town01")]
        scene: String,
        /// Extracted-root directory.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Alternative source: read PROT.DAT + CDNAME.TXT directly from a
        /// `.bin` disc image. When provided, `--extracted-root` is ignored.
        #[arg(long)]
        disc: Option<PathBuf>,
        /// Number of engine frames to run before exiting. `0` runs
        /// indefinitely.
        #[arg(long, default_value_t = 600)]
        frames: u64,
        /// Disable audio output. Useful for CI / headless smoke tests
        /// where cpal can't enumerate a device.
        #[arg(long, default_value_t = false)]
        no_audio: bool,
        /// Per-frame sleep in milliseconds. Default 16 ms ≈ 60 FPS for a
        /// realtime feel; set to `0` for "as fast as possible" smoke runs.
        #[arg(long, default_value_t = 16)]
        frame_ms: u64,
        /// Optional path to a raw PSX STR file. When provided, the video is
        /// pre-decoded headlessly and the frame count is printed before scene
        /// ticking begins. Use for `op*`/`ed*` scenes paired with FMV files.
        #[arg(long)]
        str_file: Option<PathBuf>,
    },
    /// Open a window, boot a scene, and run the engine with rendering.
    /// Accepts keyboard input (arrows = D-pad, Z = Cross, Esc = quit).
    ///
    /// When `--str-file` is provided the STR video plays first in a windowed
    /// player (same as `play-str`). After the video window closes the scene
    /// window opens and runs normally.
    PlayWindow {
        /// Starting scene name. Default: `town01`.
        #[arg(long, default_value = "town01")]
        scene: String,
        /// Extracted-root directory.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Alternative source: read PROT.DAT + CDNAME.TXT directly from a
        /// `.bin` disc image. When provided, `--extracted-root` is ignored.
        #[arg(long)]
        disc: Option<PathBuf>,
        /// Disable audio output.
        #[arg(long, default_value_t = false)]
        no_audio: bool,
        /// Enable world-map mode: installs a WorldMapController and shows
        /// the top-view camera globals in the HUD. Arrow keys scroll the
        /// top-view camera; Q/W adjust azimuth; A/S adjust zoom.
        #[arg(long, default_value_t = false)]
        world_map: bool,
        /// Optional path to a raw PSX STR file. When provided, the STR video
        /// plays in a window first (phase 1); the scene window opens after
        /// the video window closes (phase 2).
        #[arg(long)]
        str_file: Option<PathBuf>,
        /// Open the boot UI flow before entering the scene: title screen
        /// → save-select on Continue → field/encounter/battle on
        /// New Game. The default (`false`) behaviour is the legacy
        /// "jump straight to the scene" path.
        #[arg(long, default_value_t = false)]
        boot_ui: bool,
        /// Save directory used by `--boot-ui` for the save-select pass.
        #[arg(long, default_value = "saves")]
        save_dir: PathBuf,
    },
    /// Open a window and play back a raw PSX STR video file (2048-byte sectors,
    /// no CD subheaders) using the MDEC decoder.  Audio is not yet wired;
    /// video frames are rendered fullscreen at ~15 FPS (one frame per tick).
    ///
    /// Accepts raw STR data files written by `legaia-extract` or extracted
    /// directly from Mode 2 Form 1 CD sectors.  The Legaia-specific mapping
    /// from PROT entry to STR data is not yet traced; supply a raw file path.
    PlayStr {
        /// Path to a raw STR file (2048-byte sectors, no subheaders).
        #[arg()]
        str_file: PathBuf,
        /// Window width.
        #[arg(long, default_value_t = 640)]
        width: u32,
        /// Window height.
        #[arg(long, default_value_t = 480)]
        height: u32,
    },
    /// Show or update the keyboard-to-pad-button input mapping.
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// Drive a synthetic battle round end-to-end: party of 3 vs N
    /// monsters, headless ticking through `BattleSession` phases.
    /// Reports per-phase events for inspection.
    Battle {
        /// Number of monster slots (1..=5). Each is initialised with HP
        /// equal to `--monster-hp`.
        #[arg(long, default_value_t = 1)]
        monsters: u8,
        /// Per-monster initial HP.
        #[arg(long, default_value_t = 50)]
        monster_hp: u16,
        /// Maximum number of session ticks to run before exiting.
        #[arg(long, default_value_t = 256)]
        max_ticks: u64,
        /// Pre-seeded turn script — comma-separated key letters fed once
        /// per tick during the CommandInput phase. Each character maps
        /// to one input bit:
        ///   `R/L/U/D` direction; `c` cross; `o` circle; `t` triangle;
        ///   `s` square (Spirit); `S` start (commit). All other chars
        ///   advance one tick with no input. Default empty.
        #[arg(long, default_value = "")]
        script: String,
    },
    /// Drive an inventory-use session against a synthetic World. Prints
    /// the cursor moves + commit outcome.
    Inventory {
        /// Item id used by the synthetic session (default 0x01 = Healing Leaf).
        #[arg(long, default_value_t = 0x01)]
        item: u8,
        /// Number of party members.
        #[arg(long, default_value_t = 3)]
        party_size: u8,
        /// Pre-seeded input sequence (same letters as `Battle`).
        #[arg(long, default_value = "cc")]
        script: String,
    },
    /// Drive an equip session for a synthetic character. Reports state
    /// transitions + the final committed equipment row.
    Equip {
        /// Slot to edit (0..=7).
        #[arg(long, default_value_t = 0)]
        slot: u8,
        /// Item id to equip into `slot` (must be present in the synthetic
        /// inventory).
        #[arg(long, default_value_t = 0x05)]
        item: u8,
    },
    /// Replay a recorded GTE (cop2) trace file against a fresh emulator
    /// and report per-step register divergences. Useful for validating
    /// the emulator against captured retail RAM dumps.
    GteReplay {
        /// JSON trace path written by `engine-render::gte_trace::Cop2Trace`.
        #[arg(long)]
        trace: PathBuf,
        /// Print mismatch detail even when the trace replays cleanly
        /// (default off — silence is success).
        #[arg(long, default_value_t = false)]
        verbose: bool,
    },
    /// Drive a synthetic title screen → main-menu pick session.
    /// Reports per-tick events as the scripted input drives the SM.
    Title {
        /// Pre-seeded input sequence, one character per tick. `s` = start,
        /// `c` = cross, `o` = circle, `U`/`D` = up/down. All other chars
        /// advance one tick with no input.
        #[arg(long, default_value = "ssDc")]
        script: String,
        /// Treat the session as having no save data (Continue disabled).
        #[arg(long, default_value_t = false)]
        no_save: bool,
        /// Frames to spend in the fade-in phase before accepting input.
        #[arg(long, default_value_t = 4)]
        fade_frames: u16,
    },
    /// Drive a synthetic save-select session.
    SaveSelect {
        /// Mode: `load` (pick a non-empty slot) or `save` (pick any slot).
        #[arg(long, default_value = "load")]
        mode: String,
        /// Comma-separated slot presence mask (1 = present, 0 = empty).
        #[arg(long, default_value = "1,0,1")]
        slots: String,
        /// Pre-seeded input sequence (same letters as `Title`).
        #[arg(long, default_value = "cc")]
        script: String,
    },
    /// Roll a synthetic encounter session against a small table for `steps`
    /// steps. Reports the first triggered encounter (if any).
    Encounter {
        /// Trigger rate in 1/256 (default 64 ≈ 25%).
        #[arg(long, default_value_t = 64)]
        rate: u8,
        /// Number of steps to roll.
        #[arg(long, default_value_t = 100)]
        steps: u32,
        /// RNG seed (deterministic).
        #[arg(long, default_value_t = 0xDEAD_BEEF)]
        seed: u32,
    },
    /// Drive a synthetic battle target picker. Reports cursor moves +
    /// the resulting outcome.
    TargetPick {
        /// Target kind: one of `enemy`, `ally`, `ally-or-self`,
        /// `dead-ally`, `any-ally`, `all-enemies`, `all-allies`, `self`.
        #[arg(long, default_value = "enemy")]
        kind: String,
        /// Active actor slot (0..=2).
        #[arg(long, default_value_t = 0)]
        actor: u8,
        /// Pre-seeded input sequence.
        #[arg(long, default_value = "RRc")]
        script: String,
    },
    /// Drive a synthetic Tactical Arts chain editor session.
    ChainEditor {
        /// Character slot (0..=2).
        #[arg(long, default_value_t = 0)]
        char_slot: u8,
        /// Pre-seeded input sequence (`L`/`R`/`U`/`D` push directions;
        /// `c` = cross, `o` = circle, `t` = triangle, `n` = name-next).
        #[arg(long, default_value = "cLLLcc")]
        script: String,
    },
    /// Run the full Seru capture flow against the vanilla registry: roll
    /// `count` captures of a given Seru and report the resulting learn
    /// events.
    SeruCapture {
        /// Seru id to capture (default 1 = Spark).
        #[arg(long, default_value_t = 1)]
        seru: u16,
        /// Number of captures to roll.
        #[arg(long, default_value_t = 4)]
        count: u32,
        /// Comma-separated party slots (default `0,1,2`).
        #[arg(long, default_value = "0,1,2")]
        party: String,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigCmd {
    /// Print the current input mapping to stdout.
    Show {
        /// Path to the TOML config file (default: `legaia-input.toml`).
        #[arg(long, default_value = "legaia-input.toml")]
        config_file: PathBuf,
    },
    /// Set a single key binding. KEY is the user-friendly key name (e.g.
    /// `Z`, `Up`, `Enter`, `RShift`); BUTTON is the PSX pad button name
    /// (e.g. `Cross`, `Circle`, `Start`, `L1`).
    Set {
        /// Binding in KEY=BUTTON form, e.g. `--binding Z=Cross`.
        #[arg(long)]
        binding: String,
        /// Path to the TOML config file (default: `legaia-input.toml`).
        #[arg(long, default_value = "legaia-input.toml")]
        config_file: PathBuf,
    },
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Info {
            scene,
            extracted_root,
            disc,
        } => cmd_info(&scene, &extracted_root, disc.as_deref()),
        Cmd::ListScenes {
            extracted_root,
            disc,
        } => cmd_list_scenes(&extracted_root, disc.as_deref()),
        Cmd::Play {
            scene,
            extracted_root,
            disc,
            frames,
            no_audio,
            frame_ms,
            str_file,
        } => cmd_play(
            &scene,
            &extracted_root,
            disc.as_deref(),
            frames,
            !no_audio,
            frame_ms,
            str_file.as_deref(),
        ),
        Cmd::PlayWindow {
            scene,
            extracted_root,
            disc,
            no_audio,
            world_map,
            str_file,
            boot_ui,
            save_dir,
        } => cmd_play_window(
            &scene,
            &extracted_root,
            disc.as_deref(),
            !no_audio,
            world_map,
            str_file.as_deref(),
            boot_ui,
            &save_dir,
        ),
        Cmd::Save {
            extracted_root,
            disc,
            save_dir,
            slot,
            party_size,
        } => cmd_save(
            &extracted_root,
            disc.as_deref(),
            &save_dir,
            slot,
            party_size,
        ),
        Cmd::Load { save_dir, slot } => cmd_load(&save_dir, slot),
        Cmd::PlayStr {
            str_file,
            width,
            height,
        } => cmd_play_str(&str_file, width, height),
        Cmd::Config { cmd } => cmd_config(cmd),
        Cmd::Battle {
            monsters,
            monster_hp,
            max_ticks,
            script,
        } => cmd_battle(monsters, monster_hp, max_ticks, &script),
        Cmd::Inventory {
            item,
            party_size,
            script,
        } => cmd_inventory(item, party_size, &script),
        Cmd::Equip { slot, item } => cmd_equip(slot, item),
        Cmd::GteReplay { trace, verbose } => cmd_gte_replay(&trace, verbose),
        Cmd::Title {
            script,
            no_save,
            fade_frames,
        } => cmd_title(&script, no_save, fade_frames),
        Cmd::SaveSelect {
            mode,
            slots,
            script,
        } => cmd_save_select(&mode, &slots, &script),
        Cmd::Encounter { rate, steps, seed } => cmd_encounter(rate, steps, seed),
        Cmd::TargetPick {
            kind,
            actor,
            script,
        } => cmd_target_pick(&kind, actor, &script),
        Cmd::ChainEditor { char_slot, script } => cmd_chain_editor(char_slot, &script),
        Cmd::SeruCapture { seru, count, party } => cmd_seru_capture(seru, count, &party),
    }
}

fn cmd_info(
    scene_name: &str,
    extracted_root: &std::path::Path,
    disc: Option<&std::path::Path>,
) -> Result<()> {
    let index = open_index_from_args(extracted_root, disc)?;
    let scene =
        Scene::load(&index, scene_name).with_context(|| format!("load scene '{scene_name}'"))?;
    let assets = SceneAssets::build(&scene);
    let resources = SceneResources::build(&scene)?;

    println!("scene '{}'", scene.name);
    println!(
        "  CDNAME range:           PROT [{}..{})",
        scene.start, scene.end
    );
    println!("  entries swept:          {}", scene.entries.len());
    println!(
        "  TIMs uploaded to VRAM:  {} (parse failures: {})",
        resources.tim_count, resources.tim_parse_failures
    );
    println!("  TMDs parsed:            {}", resources.tmds.len());
    println!(
        "  MES container:          {}",
        if assets.mes.is_some() {
            "present"
        } else {
            "absent"
        }
    );
    println!(
        "  SEQ entries (raw):      {} (in stream wrappers: {})",
        assets.seq_entries.len(),
        assets.seq_in_stream_entries.len()
    );
    println!("  VAB entries:            {}", assets.vab_entries.len());
    println!("  Event-script records:   {}", assets.event_records.len());
    Ok(())
}

fn cmd_play(
    scene: &str,
    extracted_root: &std::path::Path,
    disc: Option<&std::path::Path>,
    frames: u64,
    enable_audio: bool,
    frame_ms: u64,
    str_file: Option<&Path>,
) -> Result<()> {
    // Auto-resolve a `--scene op*` / `--scene edteien` request to its
    // paired FMV via `legaia_engine_core::scene::cutscene_str_for` when the
    // user didn't explicitly pass `--str-file` and the extracted root
    // has the file on disk.
    let auto_str = match (str_file, disc) {
        (Some(_), _) => None,
        (None, None) => legaia_engine_core::scene::cutscene_str_for(scene)
            .map(|rel| extracted_root.join(rel))
            .filter(|p| p.exists()),
        // Disc-mode resolution would need an ISO9660 read; punt.
        (None, Some(_)) => None,
    };
    let resolved_str: Option<&Path> = str_file.or(auto_str.as_deref());

    // If a STR file was supplied (explicitly or auto-resolved), pre-decode
    // it headlessly and log the frame count. This is phase 1 for
    // `op*`/`ed*` in-engine cutscene scenes where an FMV precedes the
    // dialogue-overlay scene proper. The scene ticking (phase 2) runs
    // unconditionally after this block.
    if let Some(str_path) = resolved_str {
        use legaia_mdec::{MdecDecoder, str_sector::StrFrameAssembler};
        let data = std::fs::read(str_path)
            .with_context(|| format!("read STR file {}", str_path.display()))?;
        let n_sectors = data.len() / 2048;
        let mut asm = StrFrameAssembler::new();
        let mut decoded = 0usize;
        for i in 0..n_sectors {
            let sector = &data[i * 2048..(i + 1) * 2048];
            if let Some((hdr, bs)) = asm.push_sector(sector)? {
                let dec = MdecDecoder::new(hdr.width as u32, hdr.height as u32);
                if dec.decode_frame(&bs).is_ok() {
                    decoded += 1;
                }
            }
        }
        println!(
            "play: pre-decoded {} STR frames from {}",
            decoded,
            str_path.display()
        );
    }

    let cfg = BootConfig {
        scene: scene.to_string(),
        enable_audio,
    };
    let mut session = match disc {
        Some(disc_path) => BootSession::open_disc(disc_path, &cfg)?,
        None => BootSession::open(extracted_root, &cfg)?,
    };
    println!(
        "play: scene='{}' frames={} audio={} (entries={}, MES={}, VAB={}, SEQ={})",
        scene,
        if frames == 0 {
            "∞".into()
        } else {
            frames.to_string()
        },
        if session.audio.is_some() { "on" } else { "off" },
        session
            .host
            .scene
            .as_ref()
            .map(|s| s.entries.len())
            .unwrap_or(0),
        if session
            .host
            .assets
            .as_ref()
            .map(|a| a.mes.is_some())
            .unwrap_or(false)
        {
            "yes"
        } else {
            "no"
        },
        session
            .host
            .assets
            .as_ref()
            .map(|a| a.vab_entries.len())
            .unwrap_or(0),
        session
            .host
            .assets
            .as_ref()
            .map(|a| a.seq_entries.len() + a.seq_in_stream_entries.len())
            .unwrap_or(0),
    );

    let mut transitions = 0u64;
    let mut bgm_events = 0u64;
    let mut last_log = 0u64;
    let mut tick_count = 0u64;
    while frames == 0 || tick_count < frames {
        let event = session.tick()?;
        match event {
            SceneTickEvent::SceneEntered { name } => {
                transitions += 1;
                println!("frame {}: entered scene '{}'", tick_count, name);
            }
            SceneTickEvent::UnknownMapId { map_id } => {
                println!(
                    "frame {}: scene_transition({}) had no mapped scene",
                    tick_count, map_id
                );
            }
            SceneTickEvent::Stepped => {}
        }
        if let Some(bgm) = session.bgm.as_ref()
            && bgm.last_started.is_some()
        {
            bgm_events = bgm_events.max(1);
        }
        if tick_count - last_log >= 60 {
            last_log = tick_count;
            log::info!(
                "frame {}: world.frame={}, transitions={}, bgm_started={}",
                tick_count,
                session.host.world.frame,
                transitions,
                bgm_events
            );
        }
        if frame_ms > 0 {
            std::thread::sleep(Duration::from_millis(frame_ms));
        }
        tick_count += 1;
    }
    println!(
        "exit: ticked {} frames, world.frame={}, transitions={}",
        tick_count, session.host.world.frame, transitions
    );
    Ok(())
}

fn cmd_save(
    extracted_root: &std::path::Path,
    disc: Option<&std::path::Path>,
    save_dir: &std::path::Path,
    slot: u8,
    party_size: usize,
) -> Result<()> {
    use legaia_engine_core::menu_runtime::MenuRuntime;
    use legaia_engine_core::world::World;
    use legaia_save::{CharacterRecord, Party};

    let _ = (extracted_root, disc);
    let mut world = World::default();
    let members = (0..party_size).map(|_| CharacterRecord::zeroed()).collect();
    world.load_party(Party { members });
    world.story_flags = 0;
    world.money = 0;
    let runtime = MenuRuntime::new(save_dir.to_path_buf());
    let path = runtime.save_to_slot(&mut world, slot)?;
    let sf = world.save_full();
    println!(
        "saved slot {} to {} (party={}, story_flags={:#010X}, money={}, inventory={})",
        slot,
        path.display(),
        sf.party.members.len(),
        sf.ext.story_flags,
        sf.ext.money,
        sf.ext.inventory.len()
    );
    Ok(())
}

fn cmd_load(save_dir: &std::path::Path, slot: u8) -> Result<()> {
    use legaia_engine_core::menu_runtime::MenuRuntime;
    use legaia_engine_core::world::World;

    let runtime = MenuRuntime::new(save_dir.to_path_buf());
    let mut world = World::default();
    let path = runtime.load_from_slot(&mut world, slot)?;
    println!(
        "loaded slot {} from {} (party={}, story_flags={:#010X}, money={}, inventory={}, actors={})",
        slot,
        path.display(),
        world.roster.members.len(),
        world.story_flags,
        world.money,
        world.inventory.len(),
        world.actors.iter().filter(|a| a.active).count()
    );
    Ok(())
}

fn cmd_list_scenes(extracted_root: &std::path::Path, disc: Option<&std::path::Path>) -> Result<()> {
    let map: cdname::IndexMap = if let Some(disc_path) = disc {
        // Pull CDNAME.TXT bytes out of the disc image once.
        use legaia_engine_core::{DiscVfs, Vfs};
        let vfs = DiscVfs::open(disc_path)?;
        let bytes = vfs
            .read("cdname.txt")
            .or_else(|_| vfs.read("data/cdname.txt"))
            .context("CDNAME.TXT not present in disc image")?;
        let text = String::from_utf8(bytes).context("CDNAME.TXT is not valid UTF-8")?;
        cdname::parse_str(&text)?
    } else {
        let cdname_path = extracted_root.join("CDNAME.TXT");
        if !cdname_path.exists() {
            anyhow::bail!(
                "missing {} (run `legaia-extract` first or pass --disc PATH)",
                cdname_path.display()
            );
        }
        cdname::parse(&cdname_path).with_context(|| format!("parse {}", cdname_path.display()))?
    };

    let mut names: Vec<String> = map.values().cloned().collect();
    names.sort();
    names.dedup();

    println!("{} distinct scene names:", names.len());
    for name in &names {
        if let Some((start, end)) = cdname::block_range_for_name(&map, name) {
            println!(
                "  {:<24} PROT [{}..{}) ({} entries)",
                name,
                start,
                end,
                end - start
            );
        }
    }
    Ok(())
}

/// Open a `ProtIndex` from either an extracted directory (default) or a
/// disc image (when `--disc` was provided). Used by subcommands that
/// accept either source.
fn open_index_from_args(
    extracted_root: &std::path::Path,
    disc: Option<&std::path::Path>,
) -> Result<ProtIndex> {
    if let Some(disc_path) = disc {
        use legaia_engine_core::{DiscVfs, Vfs};
        let vfs = DiscVfs::open(disc_path)
            .with_context(|| format!("open disc image {}", disc_path.display()))?;
        let prot_bytes = vfs
            .read("prot.dat")
            .context("PROT.DAT not present in disc image")?;
        let cdname_text = vfs
            .read("cdname.txt")
            .or_else(|_| vfs.read("data/cdname.txt"))
            .ok()
            .map(|b| String::from_utf8(b).context("CDNAME.TXT is not valid UTF-8"))
            .transpose()?;
        ProtIndex::from_bytes(prot_bytes, cdname_text.as_deref())
            .with_context(|| format!("build ProtIndex from {}", disc_path.display()))
    } else {
        let prot = extracted_root.join("PROT.DAT");
        if !prot.exists() {
            anyhow::bail!(
                "missing {} (run `legaia-extract` first or pass --disc PATH)",
                prot.display()
            );
        }
        ProtIndex::open_extracted(extracted_root)
            .with_context(|| format!("open ProtIndex at {}", extracted_root.display()))
    }
}

// ---------------------------------------------------------------------------
// config
// ---------------------------------------------------------------------------

fn cmd_config(cmd: ConfigCmd) -> Result<()> {
    use legaia_engine_core::input::Mapping;
    match cmd {
        ConfigCmd::Show { config_file } => {
            let mapping = Mapping::load_or_default(&config_file);
            let mut pairs: Vec<_> = mapping.bindings.iter().collect();
            pairs.sort_by_key(|(k, _)| k.as_str());
            println!("input mapping ({})", config_file.display());
            for (key, btn) in &pairs {
                println!("  {key:<12} → {btn}");
            }
        }
        ConfigCmd::Set {
            binding,
            config_file,
        } => {
            let Some((key, btn)) = binding.split_once('=') else {
                anyhow::bail!("--binding must be KEY=BUTTON (e.g. Z=Cross)");
            };
            let key = key.trim().to_string();
            let btn = btn.trim().to_string();
            // Validate that the button name is known.
            if legaia_engine_core::input::PadButton::from_name(&btn).is_none() {
                anyhow::bail!(
                    "unknown pad button '{}'; valid names: Select L3 R3 Start Up Right Down Left L2 R2 L1 R1 Triangle Circle Cross Square",
                    btn
                );
            }
            let mut mapping = Mapping::load_or_default(&config_file);
            mapping.bindings.insert(key.clone(), btn.clone());
            mapping.save(&config_file)?;
            println!("binding saved: {key} → {btn} ({})", config_file.display());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// play-window
// ---------------------------------------------------------------------------

/// Windowed engine runner state. Owned by the winit event loop.
struct PlayWindowApp {
    session: BootSession,
    font: Font,
    /// Pre-built scene resources (VRAM + TMDs). Consumed by `upload_assets`
    /// when the renderer is first attached; `None` after that.
    scene_res: Option<SceneResources>,
    win: EngineWindow,
    font_atlas: Option<UploadedFontAtlas>,
    uploaded_vram: Option<UploadedVram>,
    meshes: Vec<UploadedVramMesh>,
    /// Retained TMD data (struct + raw bytes) parallel to `meshes`, used to
    /// re-pose animated actor meshes each frame via `tmd_to_vram_mesh_posed`.
    scene_tmd_data: Vec<(legaia_tmd::Tmd, Vec<u8>)>,
    scene_aabb: ([f32; 3], [f32; 3]),
    /// Current held-button bitmask (PSX pad encoding). Updated per key event.
    pad: u16,
    /// Input binding loaded from file (or default).
    mapping: legaia_engine_core::input::Mapping,
    /// Menu runtime — drives shop / inn / status screens. Ticked per frame
    /// when `is_open()`; renders shop overlay via `shop_draws_for`.
    menu_runtime: legaia_engine_core::menu_runtime::MenuRuntime,
    /// World-map camera controller. `Some` when `--world-map` was passed;
    /// ticked each frame alongside the session.
    world_map_ctrl: Option<WorldMapController>,
    /// Pad state from the previous frame — used to compute newly-pressed bits
    /// for the world-map toggle combo.
    prev_pad: u16,
    /// Rolling battle-event log surfaced in the HUD. Each tick drains
    /// `World::pending_battle_events` and folds the most recent N entries
    /// into this ring buffer (`ApplyArtStrike` events also get applied to
    /// the target's `BattleActor::hp` via `World::fold_battle_event`). The
    /// log is empty until a battle SM actually fires.
    battle_event_log: std::collections::VecDeque<String>,
    /// Boot-UI state. `BootUiState::Inactive` means the legacy
    /// "go straight to the scene" path; `Title` / `SaveSelect` route
    /// player input through the boot sessions until the player picks
    /// New Game / Continue and the scene becomes interactive.
    boot_ui: BootUiState,
    /// Save directory the save-select session reads / writes against.
    save_dir: std::path::PathBuf,
}

/// Boot-UI state machine. Drives the pre-scene UI when `--boot-ui` is
/// supplied to play-window. The default `Inactive` variant is what
/// every other path uses (no boot UI).
enum BootUiState {
    /// No boot UI — engine ticks the scene normally.
    Inactive,
    /// Title screen is active. Pad input drives the
    /// [`legaia_engine_core::title::TitleSession`].
    Title(legaia_engine_core::title::TitleSession),
    /// Save-select panel is active.
    SaveSelect(legaia_engine_core::save_select::SaveSelectSession),
}

impl BootUiState {
    fn is_active(&self) -> bool {
        !matches!(self, BootUiState::Inactive)
    }
}

impl PlayWindowApp {
    /// Maximum number of battle-event log lines kept in the HUD ring.
    const BATTLE_EVENT_LOG_CAP: usize = 6;

    /// Tick the boot-UI state machine (when active) using the latest
    /// pad bitmask. Returns `true` if the boot UI is still active and
    /// the scene tick should be skipped this frame.
    fn tick_boot_ui(&mut self) -> bool {
        // Build edge-triggered "newly pressed" mask so menu navigation
        // doesn't auto-repeat on held keys.
        let pressed = self.pad & !self.prev_pad;
        let cross = pressed & 0x4000 != 0;
        let circle = pressed & 0x2000 != 0;
        let triangle = pressed & 0x1000 != 0;
        let start = pressed & 0x0008 != 0;
        let up = pressed & 0x0010 != 0;
        let down = pressed & 0x0040 != 0;
        let left = pressed & 0x0080 != 0;
        let right = pressed & 0x0020 != 0;

        match &mut self.boot_ui {
            BootUiState::Inactive => false,
            BootUiState::Title(session) => {
                use legaia_engine_core::title::{TitleEvent, TitleInput, TitleOutcome};
                let input = TitleInput {
                    up,
                    down,
                    cross,
                    start,
                    circle,
                };
                let events = session.tick(input);
                for ev in &events {
                    match ev {
                        TitleEvent::NewGameSelected => {
                            log::info!("title: New Game");
                        }
                        TitleEvent::ContinueSelected => {
                            log::info!("title: Continue");
                        }
                        TitleEvent::OptionsSelected => {
                            log::info!("title: Options (not yet wired)");
                        }
                        _ => {}
                    }
                }
                if let Some(outcome) = session.outcome() {
                    match outcome {
                        TitleOutcome::NewGame => {
                            // Drop straight into the scene.
                            self.boot_ui = BootUiState::Inactive;
                        }
                        TitleOutcome::Continue => {
                            // Open the save-select panel against `save_dir`.
                            let snapshots = scan_save_dir(&self.save_dir);
                            self.boot_ui = BootUiState::SaveSelect(
                                legaia_engine_core::save_select::SaveSelectSession::new(
                                    legaia_engine_core::save_select::SaveSelectMode::Load,
                                    snapshots,
                                ),
                            );
                        }
                        TitleOutcome::Options => {
                            // Not yet wired — fall through to the scene.
                            self.boot_ui = BootUiState::Inactive;
                        }
                    }
                }
                true
            }
            BootUiState::SaveSelect(session) => {
                use legaia_engine_core::save_select::{SelectInput, SelectOutcome};
                let input = SelectInput {
                    up,
                    down,
                    left,
                    right,
                    cross,
                    circle,
                    triangle,
                };
                let _ = session.tick(input);
                if let Some(outcome) = session.outcome() {
                    match outcome {
                        SelectOutcome::Loaded(slot) => {
                            // Hydrate the world from the slot file.
                            let runtime = legaia_engine_core::menu_runtime::MenuRuntime::new(
                                self.save_dir.clone(),
                            );
                            match runtime.load_from_slot(&mut self.session.host.world, slot) {
                                Ok(p) => log::info!("loaded slot {} from {}", slot, p.display()),
                                Err(e) => log::warn!("load slot {slot} failed: {e:#}"),
                            }
                            self.boot_ui = BootUiState::Inactive;
                        }
                        SelectOutcome::Cancelled => {
                            // Back to title.
                            self.boot_ui =
                                BootUiState::Title(legaia_engine_core::title::TitleSession::new());
                        }
                        SelectOutcome::Saved(_) | SelectOutcome::Deleted(_) => {
                            // Save-select in Load mode shouldn't emit these,
                            // but degrade gracefully.
                            self.boot_ui = BootUiState::Inactive;
                        }
                    }
                }
                true
            }
        }
    }

    /// Build text draws for the active boot UI (when applicable).
    fn boot_ui_draws(&self) -> Vec<TextDraw> {
        match &self.boot_ui {
            BootUiState::Inactive => Vec::new(),
            BootUiState::Title(s) => {
                use legaia_engine_core::title::TitlePhase;
                let (phase_id, cursor) = match s.phase() {
                    TitlePhase::FadeIn { .. } => (0, 0),
                    TitlePhase::PressStart { .. } => (1, 0),
                    TitlePhase::MainMenu { cursor } => (2, cursor),
                    TitlePhase::Done(_) => return Vec::new(),
                };
                let blink_on = match s.phase() {
                    TitlePhase::PressStart { blink_phase } => blink_phase < s.blink_period / 2,
                    _ => true,
                };
                legaia_engine_render::title_draws_for(
                    &self.font,
                    phase_id,
                    cursor,
                    s.continue_enabled,
                    blink_on,
                    (96, 100),
                )
            }
            BootUiState::SaveSelect(s) => {
                use legaia_engine_core::save_select::SelectPhase;
                let rows: Vec<legaia_engine_render::SaveSelectRow<'_>> = s
                    .slots()
                    .iter()
                    .map(|snap| legaia_engine_render::SaveSelectRow {
                        label: &snap.label,
                        present: snap.present,
                        party_lv: snap.party_lv,
                        play_time_seconds: snap.play_time_seconds,
                        money: snap.money,
                        location: &snap.location,
                    })
                    .collect();
                let (cursor, confirm) = match s.phase() {
                    SelectPhase::Browsing { cursor } => (cursor as usize, None),
                    SelectPhase::ConfirmLoad { slot, cursor } => {
                        (slot as usize, Some(("Load this slot?", cursor)))
                    }
                    SelectPhase::ConfirmOverwrite { slot, cursor } => {
                        (slot as usize, Some(("Overwrite slot?", cursor)))
                    }
                    SelectPhase::ConfirmDelete { slot, cursor } => {
                        (slot as usize, Some(("Delete slot?", cursor)))
                    }
                    SelectPhase::Done(_) => return Vec::new(),
                };
                legaia_engine_render::save_select_draws_for(
                    &self.font,
                    "LOAD",
                    &rows,
                    cursor,
                    confirm,
                    (16, 32),
                )
            }
        }
    }

    /// Drain world battle events, fold each into HP / status state, and
    /// append a one-line summary to the HUD ring. Called once per simulation
    /// tick from the redraw handler.
    fn drain_and_log_battle_events(&mut self) {
        let events = self.session.host.world.drain_battle_events();
        for ev in events {
            // Apply the gameplay-state side (currently only `ApplyArtStrike`
            // mutates HP / status; other events are visual-only here).
            self.session.host.world.fold_battle_event(&ev);
            // Surface in the HUD ring.
            if self.battle_event_log.len() >= Self::BATTLE_EVENT_LOG_CAP {
                self.battle_event_log.pop_front();
            }
            self.battle_event_log.push_back(ev.summary());
        }
    }
}

impl PlayWindowApp {
    fn upload_assets(&mut self) {
        let Some(res) = self.scene_res.take() else {
            return;
        };
        let (vram_opt, font_opt, meshes, tmd_data, lo, hi) = {
            let Some(r) = self.win.renderer.as_ref() else {
                self.scene_res = Some(res);
                return;
            };
            let vram = r
                .upload_vram(&res.vram)
                .map_err(|e| log::error!("VRAM upload: {e:#}"))
                .ok();
            let font = r
                .upload_font(&self.font)
                .map_err(|e| log::error!("font upload: {e:#}"))
                .ok();
            let mut meshes = Vec::new();
            let mut tmd_data: Vec<(legaia_tmd::Tmd, Vec<u8>)> = Vec::new();
            let mut lo = [f32::INFINITY; 3];
            let mut hi = [f32::NEG_INFINITY; 3];
            for rtmd in &res.tmds {
                let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&rtmd.tmd, &rtmd.raw);
                if vmesh.indices.is_empty() {
                    continue;
                }
                let (mlo, mhi) = vmesh.aabb();
                for ax in 0..3 {
                    if mlo[ax] < lo[ax] {
                        lo[ax] = mlo[ax];
                    }
                    if mhi[ax] > hi[ax] {
                        hi[ax] = mhi[ax];
                    }
                }
                match r.upload_vram_mesh(
                    &vmesh.positions,
                    &vmesh.uvs,
                    &vmesh.cba_tsb,
                    &vmesh.normals,
                    &vmesh.indices,
                ) {
                    Ok(m) => {
                        tmd_data.push((rtmd.tmd.clone(), rtmd.raw.clone()));
                        meshes.push(m);
                    }
                    Err(e) => log::warn!("TMD upload skipped: {e:#}"),
                }
            }
            (vram, font, meshes, tmd_data, lo, hi)
        };
        if let Some(v) = vram_opt {
            self.uploaded_vram = Some(v);
        }
        if let Some(a) = font_opt {
            self.font_atlas = Some(a);
        }
        self.meshes = meshes;
        self.scene_tmd_data = tmd_data;
        if lo[0].is_finite() {
            self.scene_aabb = (lo, hi);
        }
        // Bind each uploaded mesh slot to the matching actor and wire up the
        // idle animation (record 0) when the scene carries an ANM pack for
        // that actor. Registration order: actor K → TMD slot K, mirroring
        // the retail `0x8007C018` table written by `FUN_8001E890`.
        let world = &mut self.session.host.world;
        for i in 0..self.scene_tmd_data.len() {
            world.set_actor_tmd_binding(i, i);
            if let Some(pack) = res.anm_pack_for_actor(i)
                && let Some(record_bytes) = pack.record_bytes(0)
            {
                let bone_count = self.scene_tmd_data[i].0.objects.len();
                match AnimPlayer::new(record_bytes.to_vec(), bone_count) {
                    Ok(player) => {
                        world.set_actor_animation(i, player);
                        log::info!("play-window: actor {i} animated ({bone_count} bones)");
                    }
                    Err(e) => log::warn!("play-window: actor {i} ANM init failed: {e:#}"),
                }
            }
        }
        log::info!(
            "play-window: {} meshes uploaded, VRAM {}",
            self.meshes.len(),
            if self.uploaded_vram.is_some() {
                "ready"
            } else {
                "failed"
            }
        );
    }

    fn camera_mvp(&self, aspect: f32) -> Mat4 {
        orbit_camera_mvp(
            self.scene_aabb.0,
            self.scene_aabb.1,
            0.25,
            0.4,
            self.win.elapsed_secs(),
            aspect,
        )
    }

    fn actor_model(&self, slot: usize) -> Mat4 {
        let a = &self.session.host.world.actors[slot];
        let pos = Vec3::new(
            a.move_state.world_x as f32,
            a.move_state.world_y as f32,
            a.move_state.world_z as f32,
        );
        Mat4::from_translation(pos) * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))
    }

    fn build_hud(&self, _w: u32, _h: u32) -> Vec<TextDraw> {
        let Some(atlas) = &self.font_atlas else {
            return Vec::new();
        };
        let _ = atlas;
        // Boot UI is fullscreen — when active, suppress every other HUD layer
        // and just render the active panel (title screen / save-select).
        if self.boot_ui.is_active() {
            return self.boot_ui_draws();
        }
        let white = [1.0f32, 1.0, 1.0, 1.0];
        let dim = [0.7f32, 0.85, 1.0, 1.0];
        let scene_name = self
            .session
            .host
            .scene
            .as_ref()
            .map(|s| s.name.as_str())
            .unwrap_or("(none)");
        let line1 = format!(
            "scene {}  frame {}  meshes {}",
            scene_name,
            self.session.host.world.frame,
            self.meshes.len()
        );
        let layout1 = self.font.layout_ascii(&line1);
        let mut out = text_draws_for(&layout1, (8, 8), white);
        let audio_str = if self.session.audio.is_some() {
            "audio on"
        } else {
            "no audio"
        };
        let line2 = format!(
            "t {:.1}s  {}  arrows=dpad Z=X",
            self.win.elapsed_secs(),
            audio_str
        );
        let layout2 = self.font.layout_ascii(&line2);
        out.extend(text_draws_for(&layout2, (8, 26), dim));
        if let Some(ctrl) = &self.world_map_ctrl {
            let mode_str = if ctrl.is_top_view() {
                "top-view"
            } else {
                "walk"
            };
            let line3 = format!(
                "world-map {} | cam ({},{}) az {} zoom {}",
                mode_str, ctrl.camera_x, ctrl.camera_z, ctrl.azimuth, ctrl.zoom
            );
            let layout3 = self.font.layout_ascii(&line3);
            out.extend(text_draws_for(&layout3, (8, 44), white));
        }
        // Shop / inn overlay: rendered at the bottom of the screen when the menu
        // runtime is in any shop, inn, or confirmation state.
        if self.menu_runtime.is_open() {
            let label = self.menu_runtime.current_label();
            if let Some(shop) = &self.menu_runtime.shop_session {
                let state = MenuState::from_byte(self.menu_runtime.ctx_state());
                let cursor = self.menu_runtime.cursor() as usize;
                let gold = self.session.host.world.money;
                let (title, rows, show_gold) = match state {
                    Some(MenuState::ShopBuy) => {
                        let rows: Vec<ShopRow<'_>> = shop
                            .inventory
                            .items
                            .iter()
                            .map(|item| ShopRow {
                                label: "Item",
                                price: Some(item.price),
                            })
                            .collect();
                        (label, rows, Some(gold))
                    }
                    Some(MenuState::ShopSell) => {
                        let inv_items = MenuRuntime::inventory_items(&self.session.host.world);
                        let rows: Vec<ShopRow<'_>> = inv_items
                            .iter()
                            .map(|(_id, _qty)| ShopRow {
                                label: "Item",
                                price: None,
                            })
                            .collect();
                        (label, rows, Some(gold))
                    }
                    Some(MenuState::ShopQuantity) => {
                        let rows: Vec<ShopRow<'_>> = (1u32..=9)
                            .map(|_| ShopRow {
                                label: "qty",
                                price: None,
                            })
                            .collect();
                        (label, rows, None)
                    }
                    Some(MenuState::ShopConfirm) => {
                        let rows = vec![
                            ShopRow {
                                label: "Yes",
                                price: None,
                            },
                            ShopRow {
                                label: "No",
                                price: None,
                            },
                        ];
                        (label, rows, Some(gold))
                    }
                    _ => (label, Vec::new(), None),
                };
                if !rows.is_empty() {
                    let shop_draws =
                        shop_draws_for(&self.font, title, &rows, cursor, show_gold, (8, 140));
                    out.extend(shop_draws);
                }
            } else if self.menu_runtime.inn_session.is_some() {
                // Inn overlay: cost prompt with Yes / No cursor.
                let state = MenuState::from_byte(self.menu_runtime.ctx_state());
                let cursor = self.menu_runtime.cursor() as usize;
                let cost = self
                    .menu_runtime
                    .inn_session
                    .as_ref()
                    .map(|s| s.cost)
                    .unwrap_or(0);
                let gold = self.session.host.world.money;
                match state {
                    Some(MenuState::InnConfirm) => {
                        let title = format!("INN  Rest for {}G?", cost);
                        let rows = vec![
                            ShopRow {
                                label: "Yes",
                                price: None,
                            },
                            ShopRow {
                                label: "No",
                                price: None,
                            },
                        ];
                        let inn_draws =
                            shop_draws_for(&self.font, &title, &rows, cursor, Some(gold), (8, 140));
                        out.extend(inn_draws);
                    }
                    Some(MenuState::InnSleep) => {
                        let layout = self.font.layout_ascii("Resting...");
                        out.extend(text_draws_for(&layout, (8, 140), white));
                    }
                    _ => {
                        let menu_label = format!("[{}]", label);
                        let ml_layout = self.font.layout_ascii(&menu_label);
                        out.extend(text_draws_for(&ml_layout, (8, 140), white));
                    }
                }
            } else {
                // Non-shop, non-inn menu: show current mode label.
                let menu_label = format!("[{}]", label);
                let ml_layout = self.font.layout_ascii(&menu_label);
                out.extend(text_draws_for(&ml_layout, (8, 140), white));
            }
        }
        // Battle-event log: rendered along the right edge when non-empty.
        // Most recent at the bottom of the column.
        if !self.battle_event_log.is_empty() {
            let log_color = [1.0f32, 0.95, 0.7, 1.0];
            let line_height = 14;
            let bottom_y = 280;
            let n = self.battle_event_log.len();
            for (i, line) in self.battle_event_log.iter().enumerate() {
                let layout = self.font.layout_ascii(line);
                let y = bottom_y - ((n - 1 - i) as i32) * line_height;
                out.extend(text_draws_for(&layout, (220, y), log_color));
            }
        }
        // Level-up banner: rendered near the top when active after a battle win.
        if let Some(banner) = &self.session.host.world.current_level_up_banner {
            let draws = level_up_draws_for(
                &self.font,
                banner.char_id,
                banner.new_level,
                banner.hp_gained,
                banner.mp_gained,
                (8, 60),
            );
            out.extend(draws);
        }
        out
    }
}

impl ApplicationHandler for PlayWindowApp {
    fn resumed(&mut self, evl: &ActiveEventLoop) {
        if !self.win.open(evl, "legaia-engine") {
            return;
        }
        self.upload_assets();
        self.win.request_redraw();
    }

    fn window_event(&mut self, evl: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => evl.exit(),
            WindowEvent::Resized(size) => self.win.handle_resize(size.width, size.height),
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
                let key_name = keycode_to_name(code);
                if let Some(button) = self.mapping.pad_button_for_key(key_name) {
                    if state == ElementState::Pressed {
                        self.pad |= button.mask();
                    } else {
                        self.pad &= !button.mask();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                let dt = self.win.advance_tick(100);
                // Drain up to 4 ticks per render frame so we never spiral
                // but can still catch up from minor vsync jitter.
                let ticks = self.win.drain_ticks(dt, 4);
                for _ in 0..ticks {
                    // When the boot UI is active, route input there and skip
                    // the scene tick — the player hasn't entered the world
                    // yet (or has paused into save-select).
                    if self.boot_ui.is_active() {
                        let _ = self.tick_boot_ui();
                        self.prev_pad = self.pad;
                        continue;
                    }
                    if let Err(e) = self.session.tick() {
                        log::error!("session tick: {e:#}");
                    }
                    if self.menu_runtime.is_open() {
                        let p = self.pad;
                        let input = MenuInput {
                            cross: p & 0x4000 != 0,
                            circle: p & 0x2000 != 0,
                            triangle: p & 0x1000 != 0,
                            square: p & 0x8000 != 0,
                            up: p & 0x0010 != 0,
                            down: p & 0x0040 != 0,
                            left: p & 0x0080 != 0,
                            right: p & 0x0020 != 0,
                        };
                        self.menu_runtime.tick(&mut self.session.host.world, input);
                    }
                    if let Some(ctrl) = &mut self.world_map_ctrl {
                        let newly_pressed = self.pad & !self.prev_pad;
                        ctrl.tick(self.pad, newly_pressed);
                    }
                    self.prev_pad = self.pad;
                    // Drain whatever battle events the SM fired this tick,
                    // fold their gameplay-state side into the world (HP /
                    // status), and ring them into the HUD log.
                    self.drain_and_log_battle_events();
                }
                if let (Some(r), Some(vram), Some(atlas)) = (
                    self.win.renderer.as_ref(),
                    self.uploaded_vram.as_ref(),
                    self.font_atlas.as_ref(),
                ) {
                    let (w, h) = r.surface_size();
                    let aspect = w as f32 / h.max(1) as f32;
                    let cam = self.camera_mvp(aspect);
                    // For each active actor with a tmd_binding and a current
                    // pose_frame, regenerate and re-upload the posed mesh.
                    // posed_overrides[i] replaces meshes[i] when present.
                    let mut posed_overrides: Vec<Option<UploadedVramMesh>> =
                        (0..self.scene_tmd_data.len()).map(|_| None).collect();
                    for actor in &self.session.host.world.actors {
                        if !actor.active {
                            continue;
                        }
                        let (Some(tmd_idx), Some(pose)) = (actor.tmd_binding, &actor.pose_frame)
                        else {
                            continue;
                        };
                        let Some((tmd, raw)) = self.scene_tmd_data.get(tmd_idx) else {
                            continue;
                        };
                        let vmesh =
                            legaia_tmd::mesh::tmd_to_vram_mesh_posed(tmd, raw, &pose.bone_outputs);
                        if vmesh.indices.is_empty() {
                            continue;
                        }
                        match r.upload_vram_mesh(
                            &vmesh.positions,
                            &vmesh.uvs,
                            &vmesh.cba_tsb,
                            &vmesh.normals,
                            &vmesh.indices,
                        ) {
                            Ok(m) => posed_overrides[tmd_idx] = Some(m),
                            Err(e) => log::warn!("posed mesh upload: {e:#}"),
                        }
                    }

                    let draws: Vec<SceneDraw<'_>> = self
                        .meshes
                        .iter()
                        .enumerate()
                        .map(|(i, static_m)| {
                            let mesh = posed_overrides
                                .get(i)
                                .and_then(|o| o.as_ref())
                                .unwrap_or(static_m);
                            SceneDraw {
                                mesh,
                                mvp: cam * self.actor_model(i),
                            }
                        })
                        .collect();
                    let hud = self.build_hud(w, h);
                    let overlay = TextOverlay { atlas, draws: &hud };
                    let scene = RenderScene {
                        vram,
                        draws: &draws,
                        overlay_lines: None,
                        overlay_sprites: None,
                        overlay_text: Some(&overlay),
                    };
                    if let Err(e) = r.render(RenderTarget::Scene(&scene)) {
                        log::error!("render: {e:#}");
                    }
                }
                self.win.request_redraw();
            }
            _ => {}
        }
    }
}

/// Map a winit `KeyCode` to the user-friendly key name used in
/// [`legaia_engine_core::input::Mapping`]. Returns `""` for keys outside
/// the default set.
fn keycode_to_name(code: KeyCode) -> &'static str {
    match code {
        KeyCode::ArrowUp => "Up",
        KeyCode::ArrowDown => "Down",
        KeyCode::ArrowLeft => "Left",
        KeyCode::ArrowRight => "Right",
        KeyCode::KeyZ => "Z",
        KeyCode::KeyX => "X",
        KeyCode::KeyA => "A",
        KeyCode::KeyS => "S",
        KeyCode::KeyQ => "Q",
        KeyCode::KeyW => "W",
        KeyCode::Enter => "Enter",
        KeyCode::ShiftRight => "RShift",
        KeyCode::Digit1 => "1",
        KeyCode::Digit2 => "2",
        _ => "",
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_play_window(
    scene: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    enable_audio: bool,
    world_map: bool,
    str_file: Option<&Path>,
    boot_ui: bool,
    save_dir: &Path,
) -> Result<()> {
    // Auto-resolve op*/ed* cutscene scenes to their MV*.STR file when
    // the user didn't pass --str-file but the extracted root has the
    // file on disk. Mirrors the same convenience path in cmd_play.
    let auto_str = match (str_file, disc) {
        (Some(_), _) => None,
        (None, None) => legaia_engine_core::scene::cutscene_str_for(scene)
            .map(|rel| extracted_root.join(rel))
            .filter(|p| p.exists()),
        (None, Some(_)) => None,
    };
    let resolved_str: Option<&Path> = str_file.or(auto_str.as_deref());
    // Phase 1: if a STR file is provided (or auto-resolved), play the
    // video in a window first. The user closes (or ESC) the STR window,
    // then the scene window opens.
    if let Some(str_path) = resolved_str {
        cmd_play_str(str_path, 640, 480)?;
    }

    let cfg = BootConfig {
        scene: scene.to_string(),
        enable_audio,
    };
    let mut session = match disc {
        Some(disc_path) => BootSession::open_disc(disc_path, &cfg)?,
        None => BootSession::open(extracted_root, &cfg)?,
    };
    if world_map {
        session.host.world.mode = SceneMode::WorldMap;
    }

    // Wire vanilla encounter + monster tables so triggered encounters can
    // resolve to a concrete monster set. Engines that load real disc-side
    // tables override this via `World::set_formation_table` later.
    {
        let world = &mut session.host.world;
        world.set_formation_table(
            legaia_engine_core::monster_catalog::vanilla_formation_table(),
            legaia_engine_core::monster_catalog::vanilla_monster_catalog(),
        );
        // Default early-game encounter table — the scene-loader path will
        // replace this when real disc data lands. Still useful for the
        // default `town01` boot so the encounter trigger fires on step.
        let table = legaia_engine_core::monster_catalog::default_early_encounter_table(scene);
        if matches!(world.mode, SceneMode::Field) {
            world.set_encounter_session(Some(
                legaia_engine_core::encounter::EncounterSession::new(
                    legaia_engine_core::encounter::EncounterTracker::new(table),
                ),
            ));
        }
    }

    let scene_res = {
        let s = session
            .host
            .scene
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no scene loaded after BootSession::open"))?;
        SceneResources::build(s)?
    };
    log::info!(
        "play-window: scene '{}', {} TMDs, {} TIMs",
        scene,
        scene_res.tmds.len(),
        scene_res.tim_count
    );

    let font = Font::load_or_placeholder(extracted_root);

    let mapping = legaia_engine_core::input::Mapping::load_or_default(&std::path::PathBuf::from(
        "legaia-input.toml",
    ));
    let world_map_ctrl = if world_map {
        Some(WorldMapController::new())
    } else {
        None
    };
    let initial_boot_ui = if boot_ui {
        let snapshots = scan_save_dir(save_dir);
        let any_present = snapshots.iter().any(|s| s.present);
        if any_present {
            BootUiState::Title(legaia_engine_core::title::TitleSession::new())
        } else {
            BootUiState::Title(legaia_engine_core::title::TitleSession::without_save_data())
        }
    } else {
        BootUiState::Inactive
    };
    let mut app = PlayWindowApp {
        session,
        font,
        scene_res: Some(scene_res),
        win: EngineWindow::new(),
        font_atlas: None,
        uploaded_vram: None,
        meshes: Vec::new(),
        scene_tmd_data: Vec::new(),
        scene_aabb: ([f32::NEG_INFINITY; 3], [f32::INFINITY; 3]),
        pad: 0,
        mapping,
        menu_runtime: MenuRuntime::new(save_dir.to_path_buf()),
        world_map_ctrl,
        prev_pad: 0,
        battle_event_log: std::collections::VecDeque::new(),
        boot_ui: initial_boot_ui,
        save_dir: save_dir.to_path_buf(),
    };

    let event_loop = EventLoop::new().context("create event loop")?;
    event_loop.run_app(&mut app).context("event loop")?;
    Ok(())
}

/// Walk `save_dir` and build per-slot `SlotSnapshot` entries from any
/// LGSF v1 / v2 files found there. Empty slots produce
/// `SlotSnapshot::empty(slot)`. Up to 8 slots are scanned (the retail
/// PSX memory card supports 15 blocks; engines wishing to scan more can
/// drive their own scanner and feed the result into `SaveSelectSession`).
fn scan_save_dir(save_dir: &Path) -> Vec<legaia_engine_core::save_select::SlotSnapshot> {
    use legaia_engine_core::save_select::SlotSnapshot;
    let mut out = Vec::with_capacity(3);
    for slot in 0..3u8 {
        let path = save_dir.join(format!("slot_{slot}.lgsf"));
        let snap = match std::fs::read(&path).ok().and_then(|b| {
            legaia_save::SaveFile::parse(&b)
                .ok()
                .map(|sf| (b.len(), sf))
        }) {
            Some((_, sf)) => {
                // CharacterRecord doesn't expose `level()` yet — use the
                // active-party leader's HP/MP cap as a coarse stand-in
                // for display. Engines that reverse-engineer the level
                // byte will replace this.
                let lv = sf
                    .party
                    .members
                    .first()
                    .map(|r| {
                        let hms = r.hp_mp_sp();
                        // Approximate level = max(1, hp_max / 20).
                        (hms.hp_max / 20).clamp(1, 99) as u8
                    })
                    .unwrap_or(1);
                SlotSnapshot {
                    slot,
                    present: true,
                    label: format!("Slot {slot}"),
                    play_time_seconds: sf.ext_v2.play_time_seconds,
                    party_lv: lv,
                    location: String::from("Field"),
                    money: sf.ext.money.max(0) as u32,
                }
            }
            None => SlotSnapshot::empty(slot),
        };
        out.push(snap);
    }
    out
}

// ── STR video player ────────────────────────────────────────────────────────

fn cmd_play_str(str_file: &Path, _win_width: u32, _win_height: u32) -> Result<()> {
    use legaia_mdec::{MdecDecoder, VideoFrame, str_sector::StrFrameAssembler};

    let data = std::fs::read(str_file).with_context(|| format!("read {}", str_file.display()))?;
    if data.len() % 2048 != 0 {
        log::warn!(
            "play-str: file size {} is not a multiple of 2048",
            data.len()
        );
    }
    let n_sectors = data.len() / 2048;

    // Pre-decode all frames into RGBA buffers.
    let mut asm = StrFrameAssembler::new();
    let mut frames: Vec<VideoFrame> = Vec::new();
    for i in 0..n_sectors {
        let sector = &data[i * 2048..(i + 1) * 2048];
        if let Some((hdr, bs)) = asm.push_sector(sector)? {
            let dec = MdecDecoder::new(hdr.width as u32, hdr.height as u32);
            match dec.decode_frame(&bs) {
                Ok(rgba) => frames.push(VideoFrame {
                    rgba,
                    width: hdr.width as u32,
                    height: hdr.height as u32,
                    frame_number: hdr.frame_number as u32,
                }),
                Err(e) => log::warn!("frame {}: decode error: {e}", hdr.frame_number),
            }
        }
    }
    if frames.is_empty() {
        anyhow::bail!("no video frames found in {}", str_file.display());
    }
    println!(
        "play-str: {} frames, {}×{}",
        frames.len(),
        frames[0].width,
        frames[0].height
    );

    let mut app = StrPlayerApp {
        win: EngineWindow::new(),
        frames,
        frame_idx: 0,
        uploaded: None,
    };
    let event_loop = EventLoop::new().context("create event loop")?;
    event_loop.run_app(&mut app).context("event loop")?;
    Ok(())
}

struct StrPlayerApp {
    win: EngineWindow,
    frames: Vec<legaia_mdec::VideoFrame>,
    frame_idx: usize,
    uploaded: Option<legaia_engine_render::UploadedTexture>,
}

impl ApplicationHandler for StrPlayerApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.win.open(event_loop, "legaia-engine play-str");
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        physical_key: PhysicalKey::Code(KeyCode::Escape),
                        ..
                    },
                ..
            } => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.win.handle_resize(size.width, size.height);
            }
            WindowEvent::RedrawRequested => {
                if let Some(renderer) = self.win.renderer() {
                    if self.frame_idx < self.frames.len() {
                        let f = &self.frames[self.frame_idx];
                        match renderer.upload_texture(&f.rgba, f.width, f.height) {
                            Ok(tex) => {
                                self.uploaded = Some(tex);
                            }
                            Err(e) => log::warn!("upload: {e}"),
                        }
                        self.frame_idx += 1;
                    }
                    if let Some(tex) = &self.uploaded {
                        let _ = renderer.render(RenderTarget::Texture(tex));
                    } else {
                        let _ = renderer.render(RenderTarget::Clear);
                    }
                }
                self.win.request_redraw();
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------
// Battle / Inventory / Equip / GteReplay subcommands
// ---------------------------------------------------------------------

/// Drive a synthetic [`BattleSession`] end-to-end. Reports per-frame
/// session events and the final phase. Intended as a smoke test for the
/// orchestrator wiring; engines that want a full UI use `play-window`
/// (which can host a `BattleSession` via the renderer's HUD draws).
fn cmd_battle(monsters: u8, monster_hp: u16, max_ticks: u64, script: &str) -> Result<()> {
    use legaia_art::Character;
    use legaia_engine_core::ap_gauge::ApGauge;
    use legaia_engine_core::battle_session::{
        BattlePhase, BattleSession, SessionInput, SessionSlotInfo,
    };
    use legaia_engine_core::battle_stats::StatRecord;
    use legaia_engine_core::world::{Actor, World};

    let mut session = BattleSession::new();
    session.set_party([Character::Vahn, Character::Noa, Character::Gala]);
    let names = ["Vahn", "Noa", "Gala"];
    for (i, name) in names.iter().enumerate() {
        session.set_slot_info(
            i as u8,
            SessionSlotInfo {
                name: (*name).into(),
                is_party: true,
                record: Some(StatRecord {
                    base_attack: 50,
                    base_udf: 30,
                    base_ldf: 25,
                    base_accuracy: 80,
                    base_evasion: 20,
                    ..Default::default()
                }),
                mp_max: 30,
            },
        );
    }
    let monster_count = monsters.min(5);
    for i in 0..monster_count {
        session.set_slot_info(
            3 + i,
            SessionSlotInfo {
                name: format!("Mon{i}"),
                is_party: false,
                record: Some(StatRecord {
                    base_attack: 30,
                    base_udf: 20,
                    base_ldf: 15,
                    base_accuracy: 70,
                    base_evasion: 10,
                    ..Default::default()
                }),
                mp_max: 0,
            },
        );
    }
    session.set_monster_count(monster_count);

    let mut world = World::new();
    while world.actors.len() < 8 {
        world.actors.push(Actor::default());
    }
    for i in 0..3 {
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
        world.actors[i].battle.mp = 30;
        world.ap_gauges[i] = ApGauge::with_base(8);
    }
    for i in 0..monster_count as usize {
        world.actors[3 + i].battle.hp = monster_hp;
        world.actors[3 + i].battle.max_hp = monster_hp;
    }

    session.begin_round(&mut world);
    println!(
        "battle: party=3 monsters={} phase={:?}",
        monster_count,
        session.phase()
    );

    let mut script_iter = script.chars();
    let mut total_events = 0usize;
    for tick in 0..max_ticks {
        let mut input = SessionInput::default();
        if let Some(c) = script_iter.next() {
            apply_script_char(c, &mut input);
        }
        let events = session.tick(&mut world, input);
        if !events.is_empty() {
            total_events += events.len();
            for ev in &events {
                println!("[t{tick}] {ev:?}");
            }
        }
        if session.is_done() {
            println!("battle ended at tick {tick}: {:?}", session.phase());
            break;
        }
        if matches!(session.phase(), BattlePhase::Idle) {
            break;
        }
    }
    println!(
        "battle: total_events={} final_phase={:?} hud_active_slots={}",
        total_events,
        session.phase(),
        session.hud.active_slots()
    );
    Ok(())
}

fn apply_script_char(c: char, input: &mut legaia_engine_core::battle_session::SessionInput) {
    use legaia_engine_core::battle_session::SessionInput as SI;
    let _: &SI = input;
    match c {
        'R' => input.right = true,
        'L' => input.left = true,
        'U' => input.up = true,
        'D' => input.down = true,
        'c' => input.cross = true,
        'o' => input.circle = true,
        't' => input.triangle = true,
        's' => input.square = true,
        'S' => input.start = true,
        _ => {}
    }
}

/// Drive a synthetic [`InventoryUseSession`] against a small world.
/// Reports cursor moves + the final outcome.
fn cmd_inventory(item: u8, party_size: u8, script: &str) -> Result<()> {
    use legaia_engine_core::inventory_use::{
        InventoryContext, InventoryUseInput, InventoryUseSession, TargetRow,
    };
    use legaia_engine_core::items::ItemCatalog;

    let catalog = ItemCatalog::vanilla();
    if catalog.get(item).is_none() {
        anyhow::bail!(
            "item id 0x{item:02X} not in vanilla catalog — pick from 0x10..0x41 or extend the catalog"
        );
    }
    let mut targets: Vec<TargetRow> = Vec::new();
    for i in 0..party_size {
        targets.push(TargetRow::new(i, format!("Slot{i}")).with_stats(50, 100, 10, 30));
    }

    let mut session =
        InventoryUseSession::new(catalog, vec![item], targets, InventoryContext::Field);
    println!("inventory: item=0x{item:02X} party_size={party_size}");
    for (idx, c) in script.chars().enumerate() {
        let input = match c {
            'U' => InventoryUseInput::Up,
            'D' => InventoryUseInput::Down,
            'c' => InventoryUseInput::Confirm,
            'o' => InventoryUseInput::Cancel,
            _ => continue,
        };
        session.input(input);
        let evs = session.drain_events();
        for ev in &evs {
            println!("[s{idx}={c}] {ev:?}");
        }
        if session.is_done() {
            break;
        }
    }
    println!("inventory: state={:?}", session.state);
    Ok(())
}

/// Run an equip session that confirms `item` into `slot`. Useful as a
/// smoke test for the SM and the BattleStats recompute path.
fn cmd_equip(slot: u8, item: u8) -> Result<()> {
    use legaia_engine_core::battle_stats::{
        EquipmentTable, ItemModifier, StatRecord, StatusModifiers,
    };
    use legaia_engine_core::equip_session::{EquipInput, EquipOutcome, EquipSession};
    use std::collections::HashMap;

    let record = StatRecord {
        base_attack: 50,
        base_udf: 30,
        base_ldf: 25,
        base_accuracy: 80,
        base_evasion: 20,
        equip: [0; 8],
    };
    let mut inv = HashMap::new();
    // Re-encode the item id so its implied slot matches the requested
    // slot — the synthetic test catalog uses `id >> 5` as the slot bits.
    let encoded_id = (slot << 5) | (item & 0x1F);
    inv.insert(encoded_id, 1);
    let mut eq = EquipmentTable::new();
    eq.set(
        encoded_id,
        ItemModifier {
            atk: 10,
            ..Default::default()
        },
    );
    let mut session = EquipSession::new(record, inv, eq, StatusModifiers::default(), Vec::new());

    println!("equip: requested slot={slot} item=0x{item:02X} (encoded 0x{encoded_id:02X})");

    // Drive: down `slot` times to reach the slot, cross to enter picker,
    // cross to confirm item, cross to commit.
    let mut step_count = 0;
    for _ in 0..slot {
        session.input(EquipInput {
            down: true,
            ..Default::default()
        });
        step_count += 1;
    }
    session.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    step_count += 1;
    session.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    step_count += 1;
    session.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    step_count += 1;

    println!(
        "equip: drove {step_count} inputs; outcome={:?}",
        session.outcome()
    );
    if let Some(EquipOutcome::Committed {
        added,
        slot: out_slot,
        removed,
    }) = session.outcome()
    {
        println!("equip: committed slot={out_slot} added=0x{added:02X} removed=0x{removed:02X}");
        println!(
            "equip: post-commit ATK={} (record.equip[{}]=0x{:02X})",
            session.preview_stats.atk,
            out_slot,
            session.record().equip[out_slot as usize]
        );
    }
    Ok(())
}

/// Load a JSON Cop2Trace and replay it through a fresh emulator. Reports
/// any per-step register divergence; exits 0 on clean replay.
fn cmd_gte_replay(trace_path: &Path, verbose: bool) -> Result<()> {
    use legaia_engine_render::gte_trace::Cop2Trace;
    let bytes = std::fs::read(trace_path)
        .with_context(|| format!("read trace file {}", trace_path.display()))?;
    let json = std::str::from_utf8(&bytes).context("trace file is not valid UTF-8")?;
    let trace = Cop2Trace::read_json(json).context("parse trace JSON")?;
    println!(
        "gte-replay: loaded {} steps (label={})",
        trace.len(),
        trace.label.as_deref().unwrap_or("<none>")
    );
    let mismatches = trace.replay();
    if mismatches.is_empty() {
        println!("gte-replay: clean — every step replayed bit-exact");
        if verbose {
            println!("gte-replay: trace label = {:?}", trace.label);
        }
        return Ok(());
    }
    eprintln!(
        "gte-replay: {} step(s) diverged from the recorded snapshot",
        mismatches.len()
    );
    for m in &mismatches {
        eprintln!("  step {} ({}):", m.step, m.op);
        for f in &m.fields {
            eprintln!(
                "    {} expected={} actual={}",
                f.field, f.expected, f.actual
            );
        }
    }
    anyhow::bail!("trace replay produced mismatches");
}

/// Map an input letter to a [`legaia_engine_core::title::TitleInput`] mask.
fn title_input_for(c: char) -> legaia_engine_core::title::TitleInput {
    use legaia_engine_core::title::TitleInput;
    let mut i = TitleInput::default();
    match c {
        's' => i.start = true,
        'c' => i.cross = true,
        'o' => i.circle = true,
        'U' => i.up = true,
        'D' => i.down = true,
        _ => {}
    }
    i
}

fn cmd_title(script: &str, no_save: bool, fade_frames: u16) -> Result<()> {
    use legaia_engine_core::title::{TitleEvent, TitleSession};
    let mut s = if no_save {
        TitleSession::without_save_data()
    } else {
        TitleSession::new()
    };
    s.fade_in_frames = fade_frames;
    s.skip_fade_in();
    println!("title: starting (no_save={no_save})");
    for (i, ch) in script.chars().enumerate() {
        if s.is_done() {
            break;
        }
        let evs = s.tick(title_input_for(ch));
        for e in evs {
            match e {
                TitleEvent::CursorMoved { row } => println!("  tick {i}: cursor → {row}"),
                TitleEvent::StartPressed => println!("  tick {i}: start pressed"),
                TitleEvent::MenuConfirmed { row } => println!("  tick {i}: confirmed row {row}"),
                TitleEvent::NewGameSelected => println!("  tick {i}: NewGame"),
                TitleEvent::ContinueSelected => println!("  tick {i}: Continue"),
                TitleEvent::OptionsSelected => println!("  tick {i}: Options"),
                TitleEvent::FadeInDone => println!("  tick {i}: fade-in done"),
            }
        }
    }
    println!("title: outcome = {:?}", s.outcome());
    Ok(())
}

fn select_input_for(c: char) -> legaia_engine_core::save_select::SelectInput {
    use legaia_engine_core::save_select::SelectInput;
    let mut i = SelectInput::default();
    match c {
        'c' => i.cross = true,
        'o' => i.circle = true,
        't' => i.triangle = true,
        'U' => i.up = true,
        'D' => i.down = true,
        'L' => i.left = true,
        'R' => i.right = true,
        _ => {}
    }
    i
}

fn cmd_save_select(mode: &str, slots: &str, script: &str) -> Result<()> {
    use legaia_engine_core::save_select::{
        SaveSelectMode, SaveSelectSession, SelectEvent, SlotSnapshot,
    };
    let mode = match mode.to_ascii_lowercase().as_str() {
        "load" => SaveSelectMode::Load,
        "save" => SaveSelectMode::Save,
        other => anyhow::bail!("unknown save-select mode: {other}"),
    };
    let snapshots: Vec<SlotSnapshot> = slots
        .split(',')
        .enumerate()
        .map(|(i, p)| {
            let present = p.trim() == "1";
            if present {
                SlotSnapshot {
                    slot: i as u8,
                    present: true,
                    label: format!("Slot {i}: Vahn  Lv 5"),
                    play_time_seconds: 1234,
                    party_lv: 5,
                    location: "Town01".into(),
                    money: 100,
                }
            } else {
                SlotSnapshot::empty(i as u8)
            }
        })
        .collect();
    let mut s = SaveSelectSession::new(mode, snapshots);
    println!(
        "save-select: mode={:?}, {} slot(s)",
        s.mode(),
        s.slots().len()
    );
    for (i, ch) in script.chars().enumerate() {
        if s.is_done() {
            break;
        }
        let evs = s.tick(select_input_for(ch));
        for e in evs {
            match e {
                SelectEvent::CursorMoved { slot } => {
                    println!("  tick {i}: cursor → slot {slot}")
                }
                SelectEvent::EnteredConfirm { slot, kind } => {
                    println!("  tick {i}: entered {:?} confirm on slot {slot}", kind)
                }
                SelectEvent::Confirmed { slot, kind } => {
                    println!("  tick {i}: confirmed {:?} on slot {slot}", kind)
                }
                SelectEvent::ConfirmCancelled { slot, kind } => {
                    println!("  tick {i}: cancelled {:?} on slot {slot}", kind)
                }
                SelectEvent::InvalidConfirm => println!("  tick {i}: invalid confirm"),
                SelectEvent::Cancelled => println!("  tick {i}: cancelled"),
            }
        }
    }
    println!("save-select: outcome = {:?}", s.outcome());
    Ok(())
}

fn cmd_encounter(rate: u8, steps: u32, seed: u32) -> Result<()> {
    use legaia_engine_core::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("test_scene");
    table.set_trigger_rate(rate);
    table.push(EncounterEntry::new(1, 50));
    table.push(EncounterEntry::new(2, 30));
    table.push(EncounterEntry::new(3, 20));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    let mut rng = seed;
    let mut hit_step = None;
    for step in 0..steps {
        // xorshift32
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        if session.on_step(rng) {
            hit_step = Some(step);
            break;
        }
    }
    if let Some(s) = hit_step {
        // Drain through transition.
        for _ in 0..session.transition_frames + 1 {
            session.tick_frame();
        }
        if let Some(roll) = session.drain_triggered() {
            println!(
                "encounter: triggered at step {s} → formation {} (roll q8={})",
                roll.formation_id, roll.roll_q8
            );
        } else {
            println!("encounter: triggered at step {s} but transition lost");
        }
    } else {
        println!("encounter: no trigger after {steps} step(s)");
    }
    println!(
        "encounter: total_steps={} steps_since_last={}",
        session.tracker().total_steps(),
        session.tracker().steps_since_last_battle()
    );
    Ok(())
}

fn picker_input_for(c: char) -> legaia_engine_core::target_picker::PickerInput {
    use legaia_engine_core::target_picker::PickerInput;
    let mut i = PickerInput::default();
    match c {
        'c' => i.cross = true,
        'o' => i.circle = true,
        'L' => i.left = true,
        'R' => i.right = true,
        'U' => i.up = true,
        'D' => i.down = true,
        _ => {}
    }
    i
}

fn cmd_target_pick(kind: &str, actor: u8, script: &str) -> Result<()> {
    use legaia_engine_core::target_picker::{
        PickerEvent, SlotState, TargetKind, TargetPickerSession,
    };
    let kind = match kind.to_ascii_lowercase().as_str() {
        "enemy" => TargetKind::SingleEnemy,
        "ally" => TargetKind::SingleAlly,
        "ally-or-self" => TargetKind::SingleAllyOrSelf,
        "dead-ally" => TargetKind::DeadAlly,
        "any-ally" => TargetKind::AnyAlly,
        "all-enemies" => TargetKind::AllEnemies,
        "all-allies" => TargetKind::AllAllies,
        "self" => TargetKind::Self_,
        other => anyhow::bail!("unknown target kind: {other}"),
    };
    let party = [SlotState::alive(true, true); 3];
    let monsters = [SlotState::alive(true, true); 5];
    let mut s = TargetPickerSession::new(kind, actor, party, monsters);
    println!("target-pick: kind={:?} actor={actor}", s.kind());
    for ch in script.chars() {
        if s.is_done() {
            break;
        }
        s.input(picker_input_for(ch));
        for e in s.drain_events() {
            match e {
                PickerEvent::CursorMoved { row, slot } => {
                    println!("  cursor → {:?} slot {slot}", row)
                }
                PickerEvent::RowSwitched { row, slot } => {
                    println!("  row switched → {:?} slot {slot}", row)
                }
                PickerEvent::Confirmed { row, slot } => {
                    println!("  confirmed {:?} slot {slot}", row)
                }
                PickerEvent::SweepConfirmed { row } => {
                    println!("  sweep confirmed {:?}", row)
                }
                PickerEvent::Cancelled => println!("  cancelled"),
                PickerEvent::InvalidConfirm => println!("  invalid confirm"),
            }
        }
    }
    println!("target-pick: outcome = {:?}", s.outcome());
    Ok(())
}

fn editor_input_for(c: char) -> legaia_engine_core::tactical_arts_editor::EditInput {
    use legaia_engine_core::tactical_arts_editor::EditInput;
    let mut i = EditInput::default();
    match c {
        'L' => i.left = true,
        'R' => i.right = true,
        'U' => i.up = true,
        'D' => i.down = true,
        'c' => i.cross = true,
        'o' => i.circle = true,
        't' => i.triangle = true,
        'n' => i.name_next = true,
        _ => {}
    }
    i
}

fn cmd_chain_editor(char_slot: u8, script: &str) -> Result<()> {
    use legaia_engine_core::tactical_arts_editor::{ChainEditor, ChainLibrary, EditEvent};
    let lib = ChainLibrary::new();
    let mut ed = ChainEditor::new(char_slot, &lib);
    println!("chain-editor: char_slot={char_slot}");
    for ch in script.chars() {
        if ed.is_done() {
            break;
        }
        for e in ed.tick(editor_input_for(ch)) {
            match e {
                EditEvent::BrowseCursorMoved { row } => println!("  cursor → row {row}"),
                EditEvent::EnteredEdit { editing_slot } => {
                    println!("  entered edit slot={:?}", editing_slot)
                }
                EditEvent::SequenceAppended { command, len } => {
                    println!("  appended {:?} (len={len})", command)
                }
                EditEvent::SequencePopped { len } => println!("  popped (len={len})"),
                EditEvent::InvalidCommit { len } => println!("  invalid commit at len {len}"),
                EditEvent::EnteredNaming => println!("  entered naming"),
                EditEvent::Saved { slot } => println!("  saved slot {slot}"),
                EditEvent::Replaced { slot } => println!("  replaced slot {slot}"),
                EditEvent::Deleted { slot } => println!("  deleted slot {slot}"),
                EditEvent::Cancelled => println!("  cancelled"),
            }
        }
    }
    println!("chain-editor: outcome = {:?}", ed.outcome());
    Ok(())
}

fn cmd_seru_capture(seru: u16, count: u32, party: &str) -> Result<()> {
    use legaia_engine_core::seru_learning::{SeruCaptureLog, SeruRegistry, record_capture};
    let registry = SeruRegistry::vanilla();
    let party: Vec<u8> = party
        .split(',')
        .filter_map(|s| s.trim().parse::<u8>().ok())
        .collect();
    let mut log = SeruCaptureLog::new();
    println!("seru-capture: seru={seru} count={count} party={:?}", party);
    for i in 0..count {
        let out = record_capture(&registry, &mut log, seru, &party);
        if !out.accepted {
            println!("  capture {i}: rejected (unknown seru)");
            return Ok(());
        }
        if !out.learns.is_empty() {
            for ev in &out.learns {
                println!(
                    "  capture {i}: char {} learned spell {:#04x} from seru {}",
                    ev.char_slot, ev.spell_id, ev.seru_id
                );
            }
        }
    }
    println!(
        "seru-capture: final per-char totals: {:?}",
        party
            .iter()
            .map(|c| (*c, log.total_points(*c)))
            .collect::<Vec<_>>()
    );
    for c in &party {
        println!("  char {c} learned spells: {:?}", log.learned_spells(*c));
    }
    Ok(())
}
