//! Windowed (winit + wgpu) drivers: the `play-window` / `record` engine
//! viewer (`PlayWindowApp`) and the `play-str` MDEC movie player
//! (`StrPlayerApp`), plus their geometry/asset helpers.

use anyhow::{Context, Result};
use glam::{Mat4, Vec3, Vec4};
use legaia_engine_core::menu_runtime::{MenuInput, MenuRuntime, MenuState};
use legaia_engine_core::scene::Scene;
use legaia_engine_core::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};
use legaia_engine_core::world::{AnimPlayer, SceneMode};
use legaia_engine_render::{
    ColorSceneDraw, RenderTarget, Scene as RenderScene, SceneDraw, ShopRow, TextDraw, TextOverlay,
    UploadedColorMesh, UploadedFontAtlas, UploadedVram, UploadedVramMesh, capture_banner_draws_for,
    level_up_draws_for, shop_draws_for, text_draws_for,
    window::{EngineWindow, orbit_camera_mvp},
};
use legaia_engine_shell::replay::{PadEvent, ReplayFile, ReplayMeta};
use legaia_engine_shell::{BootConfig, BootSession};
use legaia_font::Font;
use std::path::Path;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowId;

/// Raw `LineList` geometry: `(positions, per-vertex colours, line indices)`.
/// The geometry helpers (`world_map_*_line_geometry`) emit this shape; it is
/// uploaded via `Renderer::upload_lines`.
type LineGeometry = (Vec<[f32; 3]>, Vec<[u8; 4]>, Vec<u32>);

/// One assembled party member ready for the battle render:
/// `(assembled character, texture uploads, idle clip, per-slot action clips,
/// art-animation bank, per-slot face tracks, per-art-record face tracks)`.
/// The action clips cover the record[0] slots plus the equipment-spliced
/// weapon swings (runtime slots `0xC..0xF`); the bank and its face tracks
/// are indexed by art record (staged id `- 0x10`) for the `FUN_8004AD80`
/// commit. Produced by `PlayWindowApp::assembled_party_battle_mesh`.
type AssembledPartyMesh = (
    legaia_asset::battle_char_assembly::AssembledCharacter,
    Vec<legaia_asset::battle_char_assembly::TextureUpload>,
    legaia_asset::monster_archive::MonsterAnimation,
    Vec<Option<legaia_asset::monster_archive::MonsterAnimation>>,
    Vec<Option<legaia_asset::monster_archive::MonsterAnimation>>,
    Vec<Option<legaia_asset::face_anim::FaceTracks>>,
    Vec<Option<legaia_asset::face_anim::FaceTracks>>,
);

/// Per-party-member battle facial-animation state: the member's per-action
/// face tracks (entry `+0x8C` eyes / `+0x98` mouth, indexed by the playing
/// clip's `action_id`) plus the last applied stamp set, so the VRAM is only
/// re-uploaded when the stamped face actually changes. See
/// `legaia_asset::face_anim` / `tick_battle_face_stamps`.
struct BattleMemberFace {
    /// World actor slot (= present-party band ordinal, 0..3).
    actor_slot: usize,
    /// Character index (0 Vahn / 1 Noa / 2 Gala; Terra never gets an entry -
    /// the retail animator skips char 3).
    char_index: usize,
    /// Face tracks indexed by action slot (record[0] entries + the
    /// equipment-spliced swing slots 0xC..0xF).
    tracks: Vec<Option<legaia_asset::face_anim::FaceTracks>>,
    /// Face tracks of the art-bank records' embedded entries, indexed by
    /// bank record (= playing clip `action_id - 0x10`). Retail reads these
    /// through the `FUN_8004AD80`-installed entry pointer (bank record
    /// `+0x24`), i.e. record `+0xB0` eyes / `+0xBC` mouth - the mid-battle
    /// art-strike faces.
    art_tracks: Vec<Option<legaia_asset::face_anim::FaceTracks>>,
    /// Stamp set applied on the previous frame (`None` = nothing applied
    /// yet, force the first stamp).
    last_stamps: Option<Vec<legaia_asset::face_anim::FaceStamp>>,
    /// Victory-window frame counter - the engine's per-member equivalent
    /// of the retail global `gp+0x9EA` (reset to 0 when the win pose is
    /// staged, advanced per frame; the stamp pass halves it). `Some`
    /// while the member's mouth-override window is open, `None` outside
    /// it.
    art_counter: Option<u16>,
}

/// Thin shim that opens a `play-window` session with the pad-capture
/// hook armed. Identical UX to `play-window`; the only added behaviour
/// is that every pad-mask transition is appended to a `Vec<PadEvent>`
/// on `PlayWindowApp` and flushed to `out` as a `j-replay-v1` file on
/// window close.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_record(
    out: &Path,
    scene: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    enable_audio: bool,
    world_map: bool,
    save_dir: &Path,
    scenario: Option<&str>,
    rng_seed: u32,
) -> Result<()> {
    cmd_play_window_with_record(
        scene,
        extracted_root,
        disc,
        enable_audio,
        world_map,
        None,
        false,
        save_dir,
        None,
        None,
        false,
        false,
        false,
        None,
        false,
        false,
        false,
        false,
        false,
        false,
        None,
        Some(RecordTarget {
            out: out.to_path_buf(),
            scenario: scenario.map(str::to_string),
            rng_seed,
        }),
    )
}

/// Bundle of "where to write the captured replay + how to label it".
/// Threaded through into [`PlayWindowApp::record_log`] so the keyboard
/// handler can append events and the close handler can flush.
struct RecordTarget {
    out: std::path::PathBuf,
    scenario: Option<String>,
    rng_seed: u32,
}

/// Per-tick recorded-pad-event buffer + flush state. Lives on
/// [`PlayWindowApp`] when the user invoked the `record` subcommand;
/// `None` for plain `play-window` runs so the keyboard handler pays
/// nothing in the common case.
struct RecordLog {
    out_path: std::path::PathBuf,
    events: Vec<PadEvent>,
    /// Previous pad value the log saw. The keyboard handler dedups so a
    /// stream of "press, press, press" key events from auto-repeat
    /// collapses to a single PadEvent.
    last_pad: u16,
    scenario: Option<String>,
    rng_seed: u32,
    /// Highest frame index observed during the run. Used to populate
    /// `meta.frames` so the on-disk file faithfully describes the
    /// recorded duration.
    last_frame: u64,
    /// Once the file has been written, additional Close events become
    /// no-ops (winit can deliver CloseRequested + the loop's exit drop
    /// both).
    flushed: bool,
}

impl RecordLog {
    fn from_target(target: RecordTarget) -> Self {
        Self {
            out_path: target.out,
            events: Vec::new(),
            last_pad: 0,
            scenario: target.scenario,
            rng_seed: target.rng_seed,
            last_frame: 0,
            flushed: false,
        }
    }

    /// Record a pad transition iff `pad` differs from the previously
    /// logged value. Caller is responsible for emitting events in
    /// frame-ascending order (the keyboard handler always does).
    fn record_transition(&mut self, frame: u64, pad: u16) {
        if pad == self.last_pad {
            return;
        }
        self.events.push(PadEvent { frame, pad });
        self.last_pad = pad;
        if frame > self.last_frame {
            self.last_frame = frame;
        }
    }

    /// Note the frame counter advanced past `frame` without a pad
    /// change. Keeps `meta.frames` honest when the user closes the
    /// window with no input held.
    fn observe_frame(&mut self, frame: u64) {
        if frame > self.last_frame {
            self.last_frame = frame;
        }
    }

    /// Flush to disk. Idempotent.
    fn flush(&mut self) -> Result<()> {
        if self.flushed {
            return Ok(());
        }
        let meta = ReplayMeta {
            schema: legaia_engine_shell::replay::REPLAY_SCHEMA_V1.to_string(),
            scenario: self.scenario.clone(),
            rng_seed: self.rng_seed,
            frames: self.last_frame,
        };
        let mut file = ReplayFile::new(meta);
        file.events = self.events.clone();
        file.validate()?;
        file.write_to(&self.out_path)?;
        self.flushed = true;
        eprintln!(
            "record: wrote {} event(s) covering {} frame(s) -> {}",
            file.events.len(),
            file.meta.frames,
            self.out_path.display()
        );
        Ok(())
    }
}

/// Windowed in-flow cutscene playback state: the decoded STR frames the
/// field VM's FMV-trigger op resolved to, plus the current frame cursor and
/// the live GPU upload. Held on [`PlayWindowApp`] while the world sits in
/// [`SceneMode::Cutscene`]; the world resumes (`finish_cutscene`) once the
/// frames drain. Paced to the stream's detected frame rate (wall-clock gated),
/// like [`StrPlayerApp`], so the movie isn't tied to the display refresh rate.
struct WindowedCutscene {
    frames: Vec<legaia_mdec::VideoFrame>,
    idx: usize,
    uploaded: Option<legaia_engine_render::UploadedTexture>,
    /// Wall-clock duration to hold each frame (from `StrTiming::frame_period`).
    frame_period: std::time::Duration,
    /// When playback started; the wall-clock fallback frame index is
    /// `elapsed / period` (used only when there is no audio track).
    clock: Option<std::time::Instant>,
    /// The cutscene's interleaved XA audio track, staged into the engine's
    /// audio output on the first render so its cursor (the video clock for A/V
    /// sync) starts with the picture. `None` for a video-only (extract-sourced)
    /// cutscene. Taken once.
    pending_audio: Option<legaia_engine_shell::cutscene_av::CutsceneAudio>,
    /// `true` once an audio track has been staged - the render loop then reads
    /// the audio cursor as the master clock instead of wall-clock.
    has_audio: bool,
}

/// Pre-decoded publisher-logos atlas + GPU upload. Created once at boot
/// when the disc has a valid PROT 0895; reused by [`BootUiState::PublisherLogos`]
/// to sample one logo per frame.
struct PublisherLogosAssets {
    /// Per-logo source rects in atlas pixels.
    rects: [(u32, u32, u32, u32); legaia_engine_core::publisher_logos::LOGO_COUNT],
    /// GPU-resident sprite atlas (vertically stacked logos).
    atlas: legaia_engine_render::UploadedSpriteAtlas,
}

/// Pre-decoded title-screen atlas + GPU upload. Created once at boot
/// when the disc has a valid PROT 0888 (`sound_data2` per CDNAME,
/// carries title art - see `legaia_asset::title_pak`); reused by
/// [`BootUiState::Title`] to blit one quad per frame.
struct TitleScreenAssets {
    /// Source rect to sample for the title quad
    /// (`(0, 0, width, height)` for the single-TIM layout).
    rect: (u32, u32, u32, u32),
    /// GPU-resident sprite atlas (the 256×256 title TIM).
    atlas: legaia_engine_render::UploadedSpriteAtlas,
}

/// Pre-decoded menu-glyph atlas + GPU upload. Created once at boot
/// when the disc has a readable `PROT.DAT` (the source TIM lives in
/// the unindexed 240 KB pre-`init_data` gap; see
/// [`legaia_asset::menu_glyph_atlas`]). Reused by [`BootUiState::Title`]
/// to render the "NEW GAME" / "CONTINUE" / "OPTIONS" menu rows in
/// the retail small-caps font.
struct MenuGlyphAssets {
    /// GPU-resident sprite atlas (the 256×256 menu-glyph TIM).
    atlas: legaia_engine_render::UploadedSpriteAtlas,
}

/// Pre-decoded save-menu UI atlas + GPU upload. Created once at boot
/// when the disc has both a readable PROT 0899 (carries the SLOT 1 /
/// SLOT 2 pill sprites with CLUT 7) AND a readable PROT.DAT (carries
/// the system-UI sprite sheet at offset `0x018E0` with the 9-slice
/// panel chrome under CLUT row 2). Reused by [`BootUiState::SaveSelect`]
/// to compose the retail 81×29 "Load" panel from 14 byte-pinned
/// textured-sprite primitives + the 2 slot pills over the dimmed
/// title background.
struct SaveMenuAssets {
    /// Source rects for the 9-slice panel tiles + slot pills.
    rects: legaia_engine_render::SaveMenuAtlasRects,
    /// GPU-resident sprite atlas (composite 256×256: panel tiles from
    /// system-UI TIM + slot pills from PROT 0899's save-menu TIM).
    atlas: legaia_engine_render::UploadedSpriteAtlas,
}

/// Windowed engine runner state. Owned by the winit event loop.
struct PlayWindowApp {
    session: BootSession,
    font: Font,
    /// Pre-built scene resources (VRAM + TMDs). Consumed by `upload_assets`
    /// when the renderer is first attached; `None` after that.
    scene_res: Option<SceneResources>,
    win: EngineWindow,
    font_atlas: Option<UploadedFontAtlas>,
    /// Publisher-logos atlas (built once from PROT 0895). `None` if the
    /// disc isn't loaded or init.pak doesn't parse.
    publisher_logos: Option<PublisherLogosAssets>,
    /// CPU-side atlas data waiting for renderer upload. Moved into
    /// `publisher_logos` on the first frame the renderer is available.
    pending_publisher_logos_atlas: Option<legaia_engine_core::publisher_logos::LogosAtlas>,
    /// Title-screen atlas (built once from PROT 0888). `None` if the
    /// disc isn't loaded or the title TIM doesn't parse.
    title_screen: Option<TitleScreenAssets>,
    /// CPU-side title atlas waiting for renderer upload. Moved into
    /// `title_screen` on the first frame the renderer is available.
    pending_title_screen_atlas: Option<legaia_engine_core::title_screen_atlas::TitleScreenAtlas>,
    /// Menu-glyph atlas (built once from raw `PROT.DAT` at 0x11218).
    /// `None` if the disc isn't loaded or the TIM doesn't parse.
    menu_glyphs: Option<MenuGlyphAssets>,
    /// CPU-side menu-glyph atlas waiting for renderer upload. Moved
    /// into `menu_glyphs` on the first frame the renderer is available.
    pending_menu_glyph_atlas: Option<legaia_engine_core::menu_glyph_atlas::MenuGlyphAtlas>,
    /// Save-menu UI atlas (built once from PROT 0899). `None` if the
    /// disc isn't loaded or the save-menu TIM doesn't parse.
    save_menu: Option<SaveMenuAssets>,
    /// CPU-side save-menu atlas waiting for renderer upload. Moved
    /// into `save_menu` on the first frame the renderer is available.
    pending_save_menu_atlas: Option<legaia_engine_core::save_menu_atlas::SaveMenuAtlas>,
    uploaded_vram: Option<UploadedVram>,
    meshes: Vec<UploadedVramMesh>,
    /// Retained TMD data (struct + raw bytes) parallel to `meshes`, used to
    /// re-pose animated actor meshes each frame via `tmd_to_vram_mesh_posed`.
    scene_tmd_data: Vec<(legaia_tmd::Tmd, Vec<u8>)>,
    /// Field static-geometry draws: `(index into `meshes`, world model matrix)`
    /// per placed environment object, resolved at scene load from the field
    /// map's object table (`legaia_asset::field_objects`). Empty for non-field
    /// scenes. Drawn in `SceneMode::Field` so the town renders its buildings /
    /// terrain at their world positions instead of all at the origin.
    field_placement_draws: Vec<(usize, Mat4)>,
    /// The current scene's TMD pack indexed by `pack_index` (= a field move-VM
    /// stager's relative `model_sel`): `res.tmds` filtered to the
    /// `scene_asset_table` bundle entry, in scan order - the same `env_tmds` the
    /// field-placement renderer + the asset viewer use, retail
    /// `DAT_8007C018[5..]`. Built at scene load (the `SceneResources` is consumed
    /// by `upload_assets`, so the field-FX render path keeps its own clone). Used
    /// to draw op-0x34-sub-3 field effects against the SCENE meshes, not the
    /// battle `global_tmd_pool`.
    field_stager_tmds: Vec<(legaia_tmd::Tmd, Vec<u8>)>,
    /// Untextured (`F*`/`G*`) vertex-colour meshes for field props whose prims
    /// carry per-vertex colours instead of UVs (the textured VRAM-mesh path
    /// drops them). Parallel render list to `meshes`.
    color_meshes: Vec<UploadedColorMesh>,
    /// Field static-geometry colour draws: `(index into `color_meshes`, world
    /// model)` for the untextured props. Drawn alongside `field_placement_draws`.
    field_placement_color_draws: Vec<(usize, Mat4)>,
    /// Field-scene **terrain / ground** draws: `(uploaded-mesh index, world
    /// model)` per visible cell of the field `.MAP` object grid
    /// (`Scene::field_terrain_tiles`, the `CELL_VISIBLE` sweep - the dense
    /// ground / floor layer, as opposed to the placed-flag interactive objects
    /// in `field_placement_draws`). Empty for scenes with no field-map terrain.
    /// Drawn in `SceneMode::Field` UNDER the placed buildings so the town rests
    /// on its ground instead of floating over the bare clear colour.
    field_terrain_draws: Vec<(usize, Mat4)>,
    /// Terrain tiles whose mesh is untextured (`F*`/`G*` vertex-colour prims
    /// only): `(index into `color_meshes`, world model)`, resolved through the
    /// colour-mesh bridge the same way `field_placement_color_draws` is. The
    /// textured bridge drops these tiles entirely (their VRAM mesh is empty),
    /// which rendered as holes in the town floor.
    field_terrain_color_draws: Vec<(usize, Mat4)>,
    /// The field player's **untextured** mesh half: `(index into
    /// `color_meshes`, actor slot)`. The character field meshes are hybrids -
    /// pants / sleeves are F*/G* colour prims the VRAM pipeline can't carry -
    /// so the posed player draws in two passes; this one follows the actor's
    /// live model matrix each frame (unlike the static colour draws above).
    player_color_draw: Option<(usize, usize)>,
    /// World-map continent terrain draws: `(uploaded-mesh index, world model)`
    /// per visible tile of the kingdom's `.MAP` object grid
    /// (`Scene::field_terrain_tiles`, the dense `FUN_801F69D8` continent layer).
    /// Empty off the world map. Drawn in `SceneMode::WorldMap` so the overworld
    /// shows its tiled ground / trees / mountains rather than a handful of
    /// landmark objects.
    world_map_terrain_draws: Vec<(usize, Mat4)>,
    /// Bulk **ground**: the heightfield surface built from the scene's
    /// `.MAP` floor grid (`Scene::walk_heightfield`), textured per cell from
    /// the terrain-type-keyed atlas (record `+0x14`/`+0x15`/`+0x16`). `None`
    /// for scenes with no field map / no `0x1000` ground layer. Drawn as the
    /// continent ground in `SceneMode::WorldMap` and as the town floor in
    /// `SceneMode::Field` (under the `0x2000` decor tiles + placed objects).
    /// Kept out of `meshes` (it has no `Tmd` / actor binding); drawn directly
    /// with a constant Y-flip model.
    ground_heightfield: Option<UploadedVramMesh>,
    /// `C`-key toggle: when `true`, the field render uses the wide debug
    /// orbit vantage (`camera_mvp`) instead of the retail follow camera
    /// (`field_follow_camera_mvp`). Defaults to the retail view.
    field_debug_camera: bool,
    /// Kingdom slot-4 vertex-pool inspection wireframe, as raw line geometry
    /// `(positions, colors, line-indices)` in world space. `Some` only on a
    /// world-map scene when `LEGAIA_WORLDMAP_SLOT4=1` is set; `None`
    /// otherwise. Merged into the per-frame world-map `overlay_lines` buffer
    /// alongside the entity/player markers. This visualises the decoded
    /// per-kingdom object-mesh library (`SceneResources::world_map_slot4`);
    /// the segments use the group-polyline inspection convention because the
    /// faithful triangle topology + per-object placement transform live in an
    /// unpinned cluster-A command stream (see docs/formats/world-map-overlay.md).
    world_map_slot4_lines: Option<LineGeometry>,
    /// World-map ocean CLUT animation. `Some` for a world-map kingdom scene that
    /// ships the ocean tile; drives the 13-frame rolling-wave by overwriting the
    /// first 16 CLUT entries at VRAM `(0, 506)` each animation step (the retail
    /// per-frame DMA target). `None` off the world map.
    ocean_anim: Option<OceanAnim>,
    /// Pristine CPU-side scene VRAM, cloned at scene-load before any battle
    /// edits. A battle injects monster texture pools into a working copy and
    /// re-uploads; leaving battle restores this base so the field renders
    /// with clean VRAM. `None` until the first scene loads.
    cpu_vram_base: Option<legaia_tim::Vram>,
    /// Scene-mode from the previous frame, used to detect Field<->Battle
    /// transitions so monster meshes are uploaded / dropped exactly once.
    prev_scene_mode: Option<SceneMode>,
    /// Lazily-cached monster stat archive (PROT 867) bytes, decoded once and
    /// reused for every battle so each transition doesn't re-decompress 16 MB.
    monster_archive: Option<std::sync::Arc<Vec<u8>>>,
    /// `meshes.len()` at battle entry: the boundary appended battle monster
    /// meshes start at, so leaving battle truncates back to it.
    battle_mesh_base: usize,
    /// The battle VRAM (field base + injected monster textures) stashed by
    /// `enter_battle_render`, so a mid-battle player-summon spawn can inject its
    /// own creature texture into a clone and re-upload.
    battle_vram: Option<legaia_tim::Vram>,
    /// Upload generation (see [`legaia_engine_render::UploadedVram::generation`])
    /// of the battle VRAM currently expected to be GPU-resident. `Some` only
    /// while a battle texture is uploaded; any other upload path bumping the
    /// renderer's generation mid-battle is a residency violation (the
    /// white-speckle class of bug), caught + self-healed by
    /// `check_battle_vram_residency`.
    battle_vram_generation: Option<u64>,
    /// Number of monster texture slots in use this battle (0..=4). A player
    /// summon reuses the next free slot for its creature texture.
    battle_tex_slots_used: u8,
    /// Per-member battle facial-animation state (face tracks + last stamp
    /// set), populated by `enter_battle_render` for each assembled party
    /// member whose band pixels are the real texture-pool uploads. Driven
    /// per tick by `tick_battle_face_stamps`; cleared on battle exit.
    battle_faces: Vec<BattleMemberFace>,
    /// The static SCUS face-frame tables (`legaia_asset::face_anim`),
    /// lazily read from the boot source's `SCUS_942.54` on first battle
    /// entry. `None` after a failed attempt (disc-free runs) - facial
    /// animation is skipped.
    face_tables: Option<legaia_asset::face_anim::FaceFrameTables>,
    /// The static SCUS victory-window mouth-override table (the
    /// `0x80077E80` per-(char, staged-id) tracks), loaded alongside
    /// `face_tables`. `None` skips the override (the entry tracks still
    /// animate).
    art_mouth_tables: Option<legaia_asset::face_anim::ArtMouthTables>,
    /// Whether the lazy `face_tables` load already ran (so a missing
    /// executable is only probed once).
    face_tables_attempted: bool,
    /// World actor slot the spawned player-summon creature occupies (`>= 8`, so
    /// it never collides with the party/monster battle slots), or `None`.
    summon_actor_slot: Option<usize>,
    /// Mesh index of the battle-stage backdrop dome (the scene's
    /// `scene_tmd_stream` half-dome), drawn behind the actors. `None` when the
    /// scene has no stage or it failed to load.
    battle_stage_mesh: Option<usize>,
    /// Mesh index of the flat tiled ground grid drawn under the battle actors
    /// (retail's `func_0x801d02c0` grass grid). Reuses the stage dome's grass
    /// texel so it samples real grass from the battle VRAM. `None` outside a
    /// stage-dome battle or when the dome has no usable ground texel.
    battle_ground_mesh: Option<usize>,
    scene_aabb: ([f32; 3], [f32; 3]),
    /// Current held-button bitmask (PSX pad encoding). Updated per key event.
    pad: u16,
    /// Input binding loaded from file (or default).
    mapping: legaia_engine_core::input::Mapping,
    /// Menu runtime - drives shop / inn / status screens. Ticked per frame
    /// when `is_open()`; renders shop overlay via `shop_draws_for`.
    menu_runtime: legaia_engine_core::menu_runtime::MenuRuntime,
    /// Pad state from the previous frame - used to compute newly-pressed bits
    /// for boot-UI edge detection.
    prev_pad: u16,
    /// Rolling battle-event log surfaced in the HUD. Each tick drains
    /// `World::pending_battle_events` and folds the most recent N entries
    /// into this ring buffer (`ApplyArtStrike` events also get applied to
    /// the target's `BattleActor::hp` via `World::fold_battle_event`). The
    /// log is empty until a battle SM actually fires.
    battle_event_log: std::collections::VecDeque<String>,
    /// Damage-popup / status model for the battle HUD. Fed each frame from
    /// `World::drain_battle_hit_fx` (floating numbers) + the live status
    /// tracker (per-slot icons); aged by `BattleHud::tick`. Popups + status
    /// letters are drawn anchored to the per-slot HP rows in `build_hud`.
    battle_hud: legaia_engine_core::battle_hud::BattleHud,
    /// Actor slots queued for render-side mesh upload. Populated when a
    /// `FieldEvent::ActorSpawned` fires with a non-`None` `Actor::tmd_ref`
    /// (the field-VM `0x4C 0xD8` synchronous-spawn path); drained in the
    /// next render pass, which materializes a [`UploadedVramMesh`] from
    /// the actor's global-pool TMD, appends it to `self.meshes` /
    /// `self.scene_tmd_data`, and sets `actor.tmd_binding` to the new
    /// mesh index so the per-frame draws iteration picks it up.
    pending_dynamic_mesh_slots: Vec<u8>,
    /// Spawn slots whose `tmd_ref` mesh has already been uploaded + bound
    /// (idempotence for the drain above - the binding itself can't be the
    /// marker because `upload_assets` pre-binds every actor slot).
    drained_spawn_slots: std::collections::HashSet<u8>,
    /// Boot-UI state. `BootUiState::Inactive` means the legacy
    /// "go straight to the scene" path; `Title` / `SaveSelect` route
    /// player input through the boot sessions until the player picks
    /// New Game / Continue and the scene becomes interactive.
    boot_ui: BootUiState,
    /// Save directory the save-select session reads / writes against.
    save_dir: std::path::PathBuf,
    /// User-editable settings (BGM / SFX volume, message speed, …). Wired
    /// to the Options screen's [`OptionsSession`] and persisted via
    /// the engine's options round-trip path.
    options_state: legaia_engine_core::options::OptionsState,
    /// Phase J3 pad-capture state. `Some` when the user invoked the
    /// `record` subcommand; the keyboard handler appends transitions
    /// to `events` and the close handler flushes a `j-replay-v1` file
    /// to disk. `None` in plain `play-window` runs.
    record_log: Option<RecordLog>,
    /// Field-live options (live loop / player-driven battle / battle BGM)
    /// captured at startup, so the boot-UI NEW GAME handler can re-enter the
    /// opening cutscene scene (`opdeene`) with the same arming the startup
    /// `enter_field_live` used.
    field_live_opts: legaia_engine_shell::boot::FieldLiveOpts,
    /// Extracted-root directory, retained so the in-flow cutscene driver can
    /// resolve a field-VM FMV trigger's `MV*.STR` file (mirrors the headless
    /// `play` loop's `extracted_root.join(rel)`). `None` in disc-only runs,
    /// where the STR is read straight from the ISO via `disc_path`.
    extracted_root: Option<std::path::PathBuf>,
    /// Disc image, retained so the in-flow cutscene driver can read a movie's
    /// raw 2352-byte sectors off the ISO and play its interleaved XA audio in
    /// sync with the video. `None` when running from an extracted root (where
    /// the cutscene plays video only, since the extract truncates the audio).
    disc_path: Option<std::path::PathBuf>,
    /// Active windowed cutscene playback, when the field VM has flipped the
    /// world into [`SceneMode::Cutscene`] and the resolved STR decoded. While
    /// `Some`, world ticks are suspended and the window shows the video; the
    /// world resumes once the frames drain.
    cutscene: Option<WindowedCutscene>,
    /// Eases the in-engine cutscene camera between Camera Configure beats so
    /// the opening choreography blends instead of cutting. Reset (snaps) while
    /// no cutscene timeline is active.
    cutscene_cam_interp: legaia_engine_render::window::CutsceneCameraInterp,
    /// Active dialog box, mirroring `World::current_dialog`. Opened from the
    /// scene's MES container the frame a dialog request appears (field-VM
    /// op `0x3F` or the overworld talk-to), ticked for its typewriter reveal,
    /// and dropped when the world clears the request (the world owns dismissal
    /// via the field VM / overworld handler). `None` when no box is up.
    active_dialog: Option<legaia_engine_core::dialog::OwnedDialogPanel>,
    /// Spell/seru display-name table, read once from the boot SCUS so the shop's
    /// Trade screens can label each offer ("Gimard (Vahn) -> Orb"). `None` on
    /// disc-free runs or before the first lookup.
    seru_names: Option<legaia_asset::spell_names::SpellNameTable>,
}

/// Boot-UI state machine. Drives the pre-scene UI when `--boot-ui` is
/// supplied to play-window. The default `Inactive` variant is what
/// every other path uses (no boot UI).
#[allow(clippy::large_enum_variant)]
enum BootUiState {
    /// No boot UI - engine ticks the scene normally.
    Inactive,
    /// Boot publisher logos (PROT 0895). Runs before the title screen.
    /// The atlas + per-logo rects live on [`PlayWindowApp`] so the
    /// renderer can sample one quad per frame.
    PublisherLogos(legaia_engine_core::publisher_logos::PublisherLogosSession),
    /// Title screen is active. Pad input drives the
    /// [`legaia_engine_core::title::TitleSession`].
    Title(legaia_engine_core::title::TitleSession),
    /// Save-select panel is active.
    SaveSelect(legaia_engine_core::save_select::SaveSelectSession),
    /// Options / config panel is active.
    Options(legaia_engine_core::options::OptionsSession),
    /// Field (pause) menu is active. The menu session itself is hosted by
    /// the [`BootSession`]
    /// (`session.field_menu`, the retail CARD mode pair / `game_mode 0x17`;
    /// the world runs `SceneMode::Menu` while it is open). This arm only
    /// routes the window's input + draws to it, so dropping out returns
    /// control to the field tick.
    ///
    /// `sub` holds the active sub-session pushed by
    /// `FieldMenuOutcome::Confirmed(row)` (Status, Equip, Spells, Items,
    /// Save, Options, Arts) - when `Some`, input + draws route to the
    /// sub instead of the menu and the menu sits in `Suspended`
    /// underneath.
    FieldMenu {
        sub: Option<legaia_engine_core::field_menu_dispatch::FieldMenuSubsession>,
    },
    /// Game-over panel is active after a party wipe.
    #[allow(dead_code)]
    GameOver(legaia_engine_core::game_over::GameOverSession),
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
            BootUiState::PublisherLogos(session) => {
                // Start (or Cross) skips the boot sequence.
                if start || cross {
                    session.request_skip();
                }
                session.tick();
                if session.is_done() {
                    // Hand off to the title screen with the
                    // continue-enabled flag set per save-slot scan.
                    let snapshots = scan_save_dir(&self.save_dir);
                    let any_present = snapshots.iter().any(|s| s.present);
                    self.boot_ui = if any_present {
                        BootUiState::Title(legaia_engine_core::title::TitleSession::new())
                    } else {
                        BootUiState::Title(
                            legaia_engine_core::title::TitleSession::without_save_data(),
                        )
                    };
                }
                true
            }
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
                            // The selection event is informational; the Options
                            // panel opens when the title session resolves to
                            // `TitleOutcome::Options` below.
                            log::info!("title: Options");
                        }
                        _ => {}
                    }
                }
                if let Some(outcome) = session.outcome() {
                    match outcome {
                        TitleOutcome::NewGame => {
                            // Mirror the retail NEW GAME → field-launch
                            // (master mode 2 → mode 3): establish a fresh slate
                            // and seed the starting party (Vahn) from the disc's
                            // SCUS template, then enter the prologue cutscene
                            // scene `opdeene` (the front-end launcher's opening
                            // scene id, verified live), which hands off to the
                            // interactive `town01`. See docs/subsystems/boot.md
                            // "New Game boot chain".
                            self.session.begin_new_game();
                            let cutscene = legaia_asset::new_game::OPENING_CUTSCENE_SCENE;
                            match self
                                .session
                                .enter_field_live(cutscene, &self.field_live_opts)
                            {
                                Ok(mode) => {
                                    // The cutscene -> Rim Elm handoff is now armed
                                    // inside `enter_field_scene` by walking opdeene's
                                    // MAN cutscene-timeline for the real `GFLAG_SET 26`
                                    // write (World::arm_prologue_handoff_from_man), so
                                    // no blind arm is needed here. The confirm-gated
                                    // transition still fires in the field tick below
                                    // (World::take_prologue_handoff).
                                    log::info!(
                                        "new game: seeded party_count={}, entered opening cutscene \
                                         '{cutscene}' (mode={mode:?})",
                                        self.session.host.world.party_count,
                                    )
                                }
                                Err(e) => log::warn!(
                                    "new game: enter opening cutscene '{cutscene}' failed ({e:#}); \
                                     staying on the pre-booted scene"
                                ),
                            }
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
                            self.boot_ui = BootUiState::Options(
                                legaia_engine_core::options::OptionsSession::new(
                                    self.options_state.clone(),
                                ),
                            );
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
            BootUiState::Options(session) => {
                use legaia_engine_core::options::{OptionsInput, OptionsOutcome};
                let input = OptionsInput {
                    up,
                    down,
                    left,
                    right,
                    cross,
                    circle,
                    start,
                };
                let _ = session.tick(input);
                if let Some(outcome) = session.outcome() {
                    match outcome {
                        OptionsOutcome::Confirmed => {
                            self.options_state = session.state().clone();
                        }
                        OptionsOutcome::Cancelled => {
                            session.revert_if_cancelled();
                        }
                    }
                    // After options, route back to Title so the player can
                    // pick New Game / Continue (matches retail flow).
                    self.boot_ui =
                        BootUiState::Title(legaia_engine_core::title::TitleSession::new());
                }
                true
            }
            BootUiState::FieldMenu { sub } => {
                use legaia_engine_core::field_menu::{FieldMenuInput, FieldMenuOutcome};
                use legaia_engine_core::field_menu_dispatch::{
                    FieldMenuSubsession, apply_arts_outcome, apply_equip_outcome,
                    apply_inventory_outcome, apply_spell_outcome,
                };
                // The menu session is hosted by the BootSession (so headless
                // drivers share it); if it vanished out from under the UI
                // arm, drop back to the scene.
                if self.session.field_menu.is_none() {
                    self.boot_ui = BootUiState::Inactive;
                    return true;
                }
                if let Some(active_sub) = sub.as_mut() {
                    // A sub-session is open - route input + check for done.
                    active_sub.tick_pad_edge(pressed);
                    if active_sub.is_done() {
                        // Drain into world side-effects + handle save.
                        let finished = sub.take().expect("sub was Some");
                        match finished {
                            FieldMenuSubsession::Items(s) => {
                                apply_inventory_outcome(&s, &mut self.session.host.world);
                            }
                            FieldMenuSubsession::Equip { session, char_slot } => {
                                let _ = apply_equip_outcome(
                                    &session,
                                    char_slot,
                                    &mut self.session.host.world,
                                );
                            }
                            FieldMenuSubsession::Spells(s) => {
                                apply_spell_outcome(&s, &mut self.session.host.world);
                            }
                            FieldMenuSubsession::Arts(editor) => {
                                // Persist the edit back into the world's saved
                                // chains so the next battle's Arts rows reflect
                                // it: lift the live library, apply the editor
                                // outcome, store it back (World::chain_library
                                // <-> store_chain_library bridge over
                                // World::saved_chains).
                                let mut library = self.session.host.world.chain_library();
                                if apply_arts_outcome(editor, &mut library).is_ok() {
                                    self.session.host.world.store_chain_library(&library);
                                }
                            }
                            FieldMenuSubsession::Status(_) => {}
                            FieldMenuSubsession::Save(s) => {
                                use legaia_engine_core::save_select::SelectOutcome;
                                if let Some(SelectOutcome::Saved(slot)) = s.outcome() {
                                    let runtime =
                                        legaia_engine_core::menu_runtime::MenuRuntime::new(
                                            self.save_dir.clone(),
                                        );
                                    match runtime.save_to_slot(&mut self.session.host.world, slot) {
                                        Ok(p) => log::info!(
                                            "field menu: saved slot {} to {}",
                                            slot,
                                            p.display()
                                        ),
                                        Err(e) => {
                                            log::warn!("field menu: save slot {slot} failed: {e:#}")
                                        }
                                    }
                                }
                            }
                            FieldMenuSubsession::Config(mut o) => {
                                o.revert_if_cancelled();
                                self.options_state = o.state().clone();
                            }
                        }
                        if let Some(menu) = self.session.field_menu.as_mut() {
                            let _ = menu.resume(false);
                        }
                    }
                    return true;
                }
                let input = FieldMenuInput {
                    up,
                    down,
                    cross,
                    circle,
                    start,
                };
                // After Cross on a row the menu phase becomes Suspended.
                // Build the matching sub-session and route control there.
                let suspended_row = match self.session.field_menu.as_mut() {
                    Some(menu) => {
                        let _ = menu.tick(input);
                        match menu.phase() {
                            legaia_engine_core::field_menu::FieldMenuPhase::Suspended { row } => {
                                Some(row)
                            }
                            _ => None,
                        }
                    }
                    None => None,
                };
                if let Some(row) = suspended_row {
                    let snapshots = scan_save_dir(&self.save_dir);
                    // Build sub-sessions from the DISC tables the boot path
                    // already installed on the world (spell table, equipment
                    // bonus table) plus the live saved-chain library - not
                    // throwaway vanilla()/new() placeholders, which ignored
                    // any randomizer/disc data and dropped Arts edits.
                    let world = &self.session.host.world;
                    let chain_library = world.chain_library();
                    *sub = Some(FieldMenuSubsession::build(
                        row,
                        world,
                        &self.options_state,
                        &snapshots,
                        &chain_library,
                        &world.spell_catalog,
                        &world.equipment_table,
                    ));
                }
                let outcome = self.session.field_menu.as_ref().and_then(|m| m.outcome());
                if let Some(outcome) = outcome {
                    match outcome {
                        FieldMenuOutcome::Closed | FieldMenuOutcome::Confirmed(_) => {
                            // Closed = player backed out; Confirmed = a
                            // sub-session signaled "close menu entirely" via
                            // resume(true). Either way restore the suspended
                            // scene mode and drop straight to the scene.
                            self.session.close_field_menu();
                            self.boot_ui = BootUiState::Inactive;
                        }
                    }
                }
                true
            }
            BootUiState::GameOver(session) => {
                use legaia_engine_core::game_over::{GameOverInput, GameOverOutcome};
                let input = GameOverInput { up, down, cross };
                let _ = session.tick(input);
                if let Some(outcome) = session.outcome() {
                    match outcome {
                        GameOverOutcome::Continue => {
                            let snapshots = scan_save_dir(&self.save_dir);
                            self.boot_ui = BootUiState::SaveSelect(
                                legaia_engine_core::save_select::SaveSelectSession::new(
                                    legaia_engine_core::save_select::SaveSelectMode::Load,
                                    snapshots,
                                ),
                            );
                        }
                        GameOverOutcome::Retry | GameOverOutcome::Quit => {
                            // Retry → drop to scene; Quit → back to title.
                            self.boot_ui = match outcome {
                                GameOverOutcome::Quit => BootUiState::Title(
                                    legaia_engine_core::title::TitleSession::new(),
                                ),
                                _ => BootUiState::Inactive,
                            };
                        }
                    }
                }
                true
            }
        }
    }

    /// Build text draws for the active boot UI (when applicable).
    fn boot_ui_draws(&self, surface_w: u32, surface_h: u32) -> Vec<TextDraw> {
        match &self.boot_ui {
            BootUiState::Inactive => Vec::new(),
            BootUiState::PublisherLogos(_) => {
                // The publisher logos are drawn via the sprite overlay
                // (see `publisher_logo_sprite_draw`); no font text.
                Vec::new()
            }
            BootUiState::Title(s) => {
                use legaia_engine_core::title::TitlePhase;
                let (phase_id, cursor) = match s.phase() {
                    TitlePhase::FadeIn { .. } => (0, 0),
                    TitlePhase::PressStart { .. } => (1, 0),
                    TitlePhase::MainMenu { cursor } => (2, cursor),
                    TitlePhase::Done(_) => return Vec::new(),
                };
                // When the title-screen atlas is uploaded, the
                // main-menu rows render through the sprite path,
                // sampling NEW GAME / CONTINUE sub-rects from the
                // title TIM directly (retail-faithful). Suppress
                // the dialog-font fallback for phase 2 so the rows
                // aren't double-drawn. Earlier phases (fade /
                // press-start) still use the dialog font for their
                // prompt text.
                if phase_id == 2 && self.title_screen.is_some() {
                    return Vec::new();
                }
                let blink_on = match s.phase() {
                    TitlePhase::PressStart { blink_phase } => blink_phase < s.blink_period / 2,
                    _ => true,
                };
                // When the PROT 0888 title atlas is loaded, anchor the
                // menu text to the same centred + integer-scaled 256×256
                // stage `title_screen_sprite_draws` uses, so the menu
                // sits between the wordmark band (ends at src y=140)
                // and the press-start / copyright bands (start at src
                // y=178). Without an atlas we keep the legacy
                // (96, 100) pen so the no-disc fallback still renders.
                let atlas_present = self.title_screen.is_some();
                let pen = if atlas_present {
                    let atlas_w: u32 = 256;
                    let atlas_h: u32 = 256;
                    let scale = (surface_w / atlas_w.max(1))
                        .min(surface_h / atlas_h.max(1))
                        .clamp(1, 4) as i32;
                    let stage_x0 = (surface_w as i32 - (atlas_w as i32) * scale) / 2;
                    let stage_y0 = (surface_h as i32 - (atlas_h as i32) * scale) / 2;
                    // src-y=148 sits between the wordmark and the
                    // press-start/copyright bands; src-x=104 centres
                    // a ~6-glyph menu row inside the 256-wide stage.
                    (stage_x0 + 104 * scale, stage_y0 + 148 * scale)
                } else {
                    (96, 100)
                };
                legaia_engine_render::title_draws_for(
                    &self.font,
                    phase_id,
                    cursor,
                    s.continue_enabled,
                    blink_on,
                    atlas_present,
                    pen,
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
                let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
                let (cursor, confirm) = match s.phase() {
                    SelectPhase::Browsing { cursor } => (cursor as usize, None),
                    SelectPhase::NowChecking { slot, .. } => (slot as usize, None),
                    SelectPhase::SlotPreview { slot } => (slot as usize, None),
                    SelectPhase::ConfirmOverwrite { slot, cursor } => {
                        (slot as usize, Some(("Overwrite slot?", cursor)))
                    }
                    SelectPhase::ConfirmDelete { slot, cursor } => {
                        (slot as usize, Some(("Delete slot?", cursor)))
                    }
                    SelectPhase::Done(_) => return Vec::new(),
                };
                // Always emit the base save-select chrome text ("Load"
                // title) so it stays visible in every Load-mode phase.
                // Skip the ASCII `>` cursor when the sprite-based
                // pointing-finger cursor is being emitted alongside
                // (i.e. when the save-menu atlas is loaded).
                let emit_text_cursor = self.save_menu.is_none();
                let mut out = legaia_engine_render::save_select_draws_for(
                    &self.font,
                    "Load",
                    &rows,
                    cursor,
                    confirm,
                    stage_origin,
                    stage_scale,
                    emit_text_cursor,
                );
                // Phase-specific overlays.
                match s.phase() {
                    SelectPhase::NowChecking { .. } => {
                        // Retail slide: dialog x slides from
                        // NOW_CHECKING_SLIDE_START_X (416) to
                        // NOW_CHECKING_SLIDE_TARGET_X (160) over 16
                        // frames. Use the session's animation t to
                        // compute the per-frame x offset relative to
                        // the at-target rendering position.
                        let pos_x = legaia_engine_core::save_select::interpolate_anim(
                            (legaia_engine_render::NOW_CHECKING_SLIDE_START_X, 0),
                            (legaia_engine_render::NOW_CHECKING_SLIDE_TARGET_X, 0),
                            s.slide_anim_t(),
                        )
                        .0;
                        let slide_offset =
                            (pos_x - legaia_engine_render::NOW_CHECKING_SLIDE_TARGET_X, 0);
                        out.extend(legaia_engine_render::now_checking_text_draws_for(
                            &self.font,
                            stage_origin,
                            stage_scale,
                            slide_offset,
                        ));
                    }
                    SelectPhase::SlotPreview { slot } => {
                        let info = build_slot_info_view(s.slots(), slot);
                        let view = info.as_ref().map(|i| i.as_view());
                        let panel_y_offset = info_panel_slide_offset(s);
                        out.extend(legaia_engine_render::slot_info_panel_text_draws_for(
                            &self.font,
                            view.as_ref(),
                            panel_y_offset,
                            stage_origin,
                            stage_scale,
                        ));
                    }
                    _ => {}
                }
                out
            }
            BootUiState::Options(s) => {
                let rows = s.state().rows();
                let row_views: Vec<legaia_engine_render::OptionsRowView<'_>> = rows
                    .iter()
                    .map(|r| legaia_engine_render::OptionsRowView {
                        label: r.label,
                        value: &r.value,
                    })
                    .collect();
                legaia_engine_render::options_draws_for(
                    &self.font,
                    &row_views,
                    s.cursor(),
                    (96, 80),
                )
            }
            BootUiState::FieldMenu { sub } => {
                if let Some(active_sub) = sub {
                    // Render the active sub-session's overlay. Each branch
                    // builds the matching plain-data view + calls the
                    // shipped `*_draws_for` helper.
                    return self.field_menu_sub_draws(active_sub);
                }
                // The menu session lives on the BootSession (the headless
                // host of the CARD/menu mode); the window only renders it.
                let Some(menu) = self.session.field_menu.as_ref() else {
                    return Vec::new();
                };
                let view = menu.view();
                let row_views: Vec<legaia_engine_render::FieldMenuRowView<'_>> = view
                    .rows
                    .iter()
                    .map(|r| legaia_engine_render::FieldMenuRowView {
                        label: r.label,
                        enabled: r.enabled,
                    })
                    .collect();
                legaia_engine_render::field_menu_draws_for(
                    &self.font,
                    &row_views,
                    view.cursor,
                    view.money,
                    view.play_time_seconds,
                    (32, 64),
                )
            }
            BootUiState::GameOver(s) => legaia_engine_render::game_over_draws_for(
                &self.font,
                s.cursor(),
                s.continue_enabled,
                (96, 100),
            ),
        }
    }

    /// Drain world field events and route them to whichever subsystem
    /// owns them. Currently:
    /// - [`FieldEvent::ActorSpawned`]: when the actor carries a non-`None`
    ///   `Actor::tmd_ref` (the `0x4C 0xD8` synchronous-spawn path), queue
    ///   the slot in [`Self::pending_dynamic_mesh_slots`] so the next
    ///   render pass uploads its mesh. ActorSpawned events without a
    ///   `tmd_ref` (the `0x4C 0x80` halt-acquire-gated bytecode-only
    ///   path) are dropped silently here - those actors have no visual
    ///   in this renderer until their bytecode runs.
    /// - All other events: not relevant to the play-window renderer yet,
    ///   surfaced via the HUD log instead by callers that want them.
    fn drain_and_route_field_events(&mut self) {
        use legaia_engine_core::field_events::FieldEvent;
        let world = &mut self.session.host.world;
        let events = world.drain_field_events();
        for ev in events {
            if let FieldEvent::ActorSpawned { slot, .. } = ev {
                let has_tmd = world
                    .actors
                    .get(slot as usize)
                    .is_some_and(|a| a.tmd_ref.is_some());
                if has_tmd {
                    self.pending_dynamic_mesh_slots.push(slot);
                }
            }
        }
    }

    /// Start windowed cutscene playback when the world has flipped into
    /// [`SceneMode::Cutscene`] (a field-VM FMV-trigger op fired). Resolves the
    /// active FMV's `MV*.STR` and decodes it: from the disc image (raw 2352-
    /// byte sectors, so the interleaved XA audio plays in sync) when booting
    /// from a disc, otherwise the video-only Form-1 extract under the extracted
    /// root. A cut/missing slot, an unresolvable path, or a decode that yields
    /// no frames drains the trigger immediately via `finish_cutscene` (no-op),
    /// matching the headless `play` loop. Leaves `self.cutscene = None` when
    /// nothing starts.
    /// Narrow a whole-`MVn.STR`-file sector span to just the segment a given
    /// `fmv_id` plays, using the FMV dispatch table decoded from the cutscene
    /// overlay (PROT 0970). One `MVn.STR` can carry several cutscenes by frame
    /// range (e.g. `MV3.STR` -> fmv 1 / 2 / ...), so without this an `fmv_id`
    /// that seeks into the file would play from the wrong frame. Returns
    /// `(start_lba, sector_count)`; falls back to the whole file
    /// (`(file_lba, file_sectors)`) when the table / entry is unavailable.
    fn fmv_segment_window(&self, fmv_id: i16, file_lba: u32, file_sectors: u32) -> (u32, u32) {
        use legaia_asset::fmv_dispatch::{FmvTable, STR_OVERLAY_PROT_INDEX};
        let table = self
            .session
            .host
            .index
            .entry_bytes(STR_OVERLAY_PROT_INDEX)
            .ok()
            .and_then(|b| FmvTable::from_str_overlay(&b[..]));
        legaia_engine_shell::cutscene_av::fmv_segment_window(
            table.as_ref().and_then(|t| t.entry(fmv_id)),
            file_lba,
            file_sectors,
        )
    }

    fn try_start_windowed_cutscene(&mut self) {
        use legaia_engine_shell::cutscene_av::{decode_str_av_from_disc, decode_str_video_only};
        let Some(fmv_id) = self.session.host.world.active_fmv() else {
            return;
        };
        let Some(rel) = self.session.host.world.active_fmv_str_filename() else {
            log::info!("cutscene: fmv_id={fmv_id} (cut/unmapped slot); skipping");
            self.session.host.world.finish_cutscene();
            return;
        };

        let decoded: Option<(Vec<legaia_mdec::VideoFrame>, std::time::Duration, _)> = if let Some(
            disc_path,
        ) =
            self.disc_path.as_ref()
        {
            match resolve_iso_file(disc_path, Path::new(&rel)) {
                Ok((lba, size)) => {
                    let total = size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
                    // Narrow to the fmv_id's frame-range segment (multi-cutscene
                    // files like MV3.STR carry several fmv_ids by frame range).
                    let (lba, count) = self.fmv_segment_window(fmv_id, lba, total);
                    match decode_str_av_from_disc(disc_path, lba, count) {
                        Ok(av) if !av.frames.is_empty() => {
                            log::info!(
                                "cutscene: playing fmv_id={fmv_id} {rel} from disc \
                                     ({} frames, {:.2} fps, audio: {})",
                                av.frames.len(),
                                av.timing.fps,
                                if av.audio.is_some() { "yes" } else { "no" }
                            );
                            Some((av.frames, av.timing.frame_period(), av.audio))
                        }
                        Ok(_) => {
                            log::warn!("cutscene: fmv_id={fmv_id} {rel} decoded no frames");
                            None
                        }
                        Err(e) => {
                            log::warn!(
                                "cutscene: fmv_id={fmv_id} {rel} disc decode failed ({e:#})"
                            );
                            None
                        }
                    }
                }
                Err(e) => {
                    log::warn!("cutscene: fmv_id={fmv_id} {rel} not on disc ({e:#})");
                    None
                }
            }
        } else if let Some(root) = self.extracted_root.as_ref() {
            let path = root.join(rel);
            match decode_str_video_only(&path) {
                Ok((frames, timing)) if !frames.is_empty() => {
                    log::info!(
                        "cutscene: playing fmv_id={fmv_id} {rel} ({} frames, {:.2} fps, no audio)",
                        frames.len(),
                        timing.fps
                    );
                    Some((frames, timing.frame_period(), None))
                }
                Ok(_) => {
                    log::warn!("cutscene: fmv_id={fmv_id} {rel} decoded no frames; skipping");
                    None
                }
                Err(e) => {
                    log::warn!(
                        "cutscene: fmv_id={fmv_id} {} decode failed ({e:#}); skipping",
                        path.display()
                    );
                    None
                }
            }
        } else {
            log::info!("cutscene: fmv_id={fmv_id} (no disc / extracted root); skipping");
            None
        };

        match decoded {
            Some((frames, frame_period, audio)) => {
                self.cutscene = Some(WindowedCutscene {
                    frames,
                    idx: 0,
                    uploaded: None,
                    frame_period,
                    clock: None,
                    pending_audio: audio,
                    has_audio: false,
                });
            }
            None => {
                // Drain the trigger so the field resumes next frame.
                self.session.host.world.finish_cutscene();
            }
        }
    }

    /// Render the active cutscene's current frame, paced to the stream's
    /// detected frame rate. The visible frame is `elapsed / frame_period`, so
    /// playback runs at the movie's real ~15 fps regardless of the display
    /// refresh rate (frames are held, or dropped if the host falls behind).
    /// `idx` tracks the due frame so the drain check at the top of the redraw
    /// handler resumes the field once the full duration has elapsed.
    fn render_windowed_cutscene(&mut self) {
        // Clone the audio handle before borrowing the renderer / cutscene so
        // staging the track and reading its cursor don't alias `self`.
        let audio_out = self.session.audio.clone();
        let Some(renderer) = self.win.renderer.as_ref() else {
            return;
        };
        if let Some(c) = self.cutscene.as_mut() {
            // Stage the interleaved audio on the first render so the audio
            // cursor (the A/V-sync master clock) starts with the picture. Pause
            // the scene sequencer so the cutscene track isn't layered over BGM.
            if let (Some(out), Some(track)) = (audio_out.as_ref(), c.pending_audio.take()) {
                out.set_sequencer_paused(true);
                out.play_xa(track.pcm, track.sample_rate, track.channels, false, 0x4000);
                c.has_audio = true;
            }
            let now = std::time::Instant::now();
            let start = *c.clock.get_or_insert(now);
            let elapsed = now.duration_since(start).as_secs_f64();
            // A/V sync: drive the visible frame off the audio cursor while a
            // track is playing, else off wall-clock. `idx` reaching the frame
            // count signals end-of-playback to the drain check.
            let audio_secs = if c.has_audio {
                audio_out.as_ref().and_then(|o| o.xa_cursor_secs())
            } else {
                None
            };
            let due = legaia_engine_shell::cutscene_av::due_video_frame(
                audio_secs,
                elapsed,
                c.frame_period.as_secs_f64(),
            );
            c.idx = due;
            let show = due.min(c.frames.len().saturating_sub(1));
            if let Some(f) = c.frames.get(show) {
                match renderer.upload_texture(&f.rgba, f.width, f.height) {
                    Ok(tex) => c.uploaded = Some(tex),
                    Err(e) => log::warn!("cutscene upload: {e}"),
                }
            }
            match c.uploaded.as_ref() {
                Some(tex) => {
                    let _ = renderer.render(RenderTarget::Texture(tex));
                }
                None => {
                    let _ = renderer.render(RenderTarget::Clear);
                }
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

        // Floating damage / heal numbers: the live battle loop resolves and
        // applies HP itself, then queues a presentation-only FX per strike.
        // Feed those into the popup model and log the magnitude (the typed
        // events above are consumed inside the live loop, so this is the
        // only place per-strike damage surfaces while live).
        let fx = self.session.host.world.drain_battle_hit_fx();
        for f in fx {
            if f.is_heal {
                self.battle_hud.push_heal(f.target_slot, f.amount);
            } else if f.is_crit {
                self.battle_hud.push_popup(
                    legaia_engine_core::battle_hud::DamagePopup::damage(f.target_slot, f.amount)
                        .crit(),
                );
            } else {
                self.battle_hud.push_damage(f.target_slot, f.amount);
            }
            if self.battle_event_log.len() >= Self::BATTLE_EVENT_LOG_CAP {
                self.battle_event_log.pop_front();
            }
            let sign = if f.is_heal { '+' } else { '-' };
            self.battle_event_log
                .push_back(format!("slot {} {}{} HP", f.target_slot, sign, f.amount));
        }

        // Battle sound cues: the art-strike outcomes resolve per-strike SFX
        // cues (kind = the SfxBank id, played directly without classify_cue).
        // Enqueue each into the director's SfxScheduler at its strike-relative
        // timing, then advance the scheduler one frame and fire any matured cue
        // through the scene VAB. Drain first so the world borrow ends before
        // the director borrow. SFX touch the SPU only - no RNG - so battle
        // determinism stays bit-exact.
        let cues = self.session.host.world.drain_battle_sfx_cues();
        if let Some(bgm) = self.session.bgm.as_mut() {
            for cue in &cues {
                bgm.enqueue_sfx(cue.kind, cue.timing_frames, cue.actor_slot, cue.target_slot);
            }
            for (id, voice) in bgm.tick_sfx_frame() {
                log::debug!("battle SFX cue {id:#04x} fired on voice {voice}");
            }
        } else {
            for cue in &cues {
                log::debug!(
                    "battle SFX cue {:#04x} @ +{} frames (actor {} -> target {}; no audio)",
                    cue.kind,
                    cue.timing_frames,
                    cue.actor_slot,
                    cue.target_slot
                );
            }
        }

        // Refresh per-slot status icons + age the popups one frame.
        if self.session.host.world.mode == SceneMode::Battle {
            for slot in 0..self.battle_hud.slots.len() as u8 {
                self.battle_hud
                    .sync_status(slot, &self.session.host.world.status_effects);
            }
        }
        self.battle_hud.tick();
    }

    /// Build [`TextDraw`]s for an active field-menu sub-session. Each
    /// variant maps to the matching `*_draws_for` helper in
    /// `legaia-engine-render`. Renderer-side state stays in this method
    /// so the sub-session enums in `legaia-engine-core` can stay
    /// renderer-agnostic.
    fn field_menu_sub_draws(
        &self,
        sub: &legaia_engine_core::field_menu_dispatch::FieldMenuSubsession,
    ) -> Vec<TextDraw> {
        use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
        match sub {
            FieldMenuSubsession::Status(s) => {
                let Some(snap) = s.current() else {
                    return Vec::new();
                };
                let stat_rows: Vec<legaia_engine_render::StatusStatRow<'_>> = snap
                    .stats
                    .iter()
                    .zip(snap.stat_labels.iter())
                    .map(|(v, l)| legaia_engine_render::StatusStatRow {
                        label: l,
                        value: *v as u32,
                    })
                    .collect();
                let equip_rows: Vec<(&str, &str)> = snap
                    .equip
                    .iter()
                    .map(|e| (e.label, e.item_name.as_str()))
                    .collect();
                let view = legaia_engine_render::StatusPanelView {
                    name: &snap.name,
                    level: snap.level,
                    xp: snap.xp,
                    xp_to_next: snap.xp_to_next,
                    hp: snap.hp,
                    hp_max: snap.hp_max,
                    mp: snap.mp,
                    mp_max: snap.mp_max,
                    ap: snap.ap,
                    ap_max: snap.ap_max,
                    stat_rows: &stat_rows,
                    equip_rows: &equip_rows,
                };
                legaia_engine_render::status_screen_draws_for(
                    &self.font,
                    &view,
                    Some("L1/R1: Switch  Circle: Back"),
                    (32, 32),
                )
            }
            FieldMenuSubsession::Config(s) => {
                let rows = s.state().rows();
                let row_views: Vec<legaia_engine_render::OptionsRowView<'_>> = rows
                    .iter()
                    .map(|r| legaia_engine_render::OptionsRowView {
                        label: r.label,
                        value: &r.value,
                    })
                    .collect();
                legaia_engine_render::options_draws_for(
                    &self.font,
                    &row_views,
                    s.cursor(),
                    (96, 80),
                )
            }
            FieldMenuSubsession::Save(s) => {
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
                    // Load-mode NowChecking / SlotPreview phases render
                    // separately (see slot_preview_draws / now_checking
                    // overlay below); pass through to a plain cursor.
                    SelectPhase::NowChecking { slot, .. } | SelectPhase::SlotPreview { slot } => {
                        (slot as usize, None)
                    }
                    SelectPhase::ConfirmOverwrite { slot, cursor } => {
                        (slot as usize, Some(("Overwrite slot?", cursor)))
                    }
                    SelectPhase::ConfirmDelete { slot, cursor } => {
                        (slot as usize, Some(("Delete slot?", cursor)))
                    }
                    SelectPhase::Done(_) => return Vec::new(),
                };
                // Field-menu Save subsession reuses the load-screen
                // chrome stage so the panel/pill sprites match retail
                // positions even when entered mid-game.
                let (sw, sh) = self
                    .win
                    .renderer()
                    .map(|r| r.surface_size())
                    .unwrap_or((1, 1));
                let (stage_origin, stage_scale) = self.save_select_stage(sw, sh);
                let emit_text_cursor = self.save_menu.is_none();
                legaia_engine_render::save_select_draws_for(
                    &self.font,
                    "Save",
                    &rows,
                    cursor,
                    confirm,
                    stage_origin,
                    stage_scale,
                    emit_text_cursor,
                )
            }
            FieldMenuSubsession::Spells(s) => {
                use legaia_engine_core::spell_menu::SpellMenuPhase;
                let names: Vec<&str> = s.party().iter().map(|c| c.name.as_str()).collect();
                let hp: Vec<(u16, u16)> = s.party().iter().map(|c| (c.hp, c.hp)).collect();
                let mp: Vec<(u16, u16)> = s.party().iter().map(|c| (c.mp, c.mp)).collect();
                let spell_rows = s.current_spell_rows();
                let spell_views: Vec<legaia_engine_render::SpellRowView<'_>> = spell_rows
                    .iter()
                    .map(|sr| legaia_engine_render::SpellRowView {
                        name: sr.name.as_str(),
                        mp_cost: sr.mp_cost,
                        admissible: sr.admissible,
                    })
                    .collect();
                let target_views: Vec<legaia_engine_render::SpellTargetView<'_>> = s
                    .targets()
                    .iter()
                    .map(|t| legaia_engine_render::SpellTargetView {
                        name: t.name.as_str(),
                        hp: t.hp,
                        hp_max: t.hp_max,
                        alive: t.alive(),
                    })
                    .collect();
                let (selected_caster, selected_spell, phase, cursor) = match s.phase() {
                    SpellMenuPhase::CharSelect { cursor } => (None, None, 0u8, *cursor),
                    SpellMenuPhase::SpellSelect { caster, cursor } => {
                        (Some(*caster), None, 1u8, *cursor)
                    }
                    SpellMenuPhase::TargetSelect {
                        caster,
                        spell_id,
                        cursor,
                    } => (Some(*caster), Some(*spell_id), 2u8, *cursor),
                    SpellMenuPhase::Done(_) => return Vec::new(),
                };
                let names_arr: Vec<&str> = names.to_vec();
                let args = legaia_engine_render::SpellMenuDrawArgs {
                    party_names: &names_arr,
                    party_hp: &hp,
                    party_mp: &mp,
                    selected_caster,
                    spells: &spell_views,
                    selected_spell,
                    targets: &target_views,
                    selected_target: None,
                    cursor,
                    phase,
                };
                legaia_engine_render::spell_menu_draws_for(&self.font, args, (32, 32))
            }
            FieldMenuSubsession::Items(s) => self.items_session_draws(s),
            FieldMenuSubsession::Equip { session, char_slot } => {
                self.equip_session_draws(session, *char_slot)
            }
            FieldMenuSubsession::Arts(s) => self.arts_session_draws(s),
        }
    }

    /// Build draws for the inventory item-use overlay. Resolves item
    /// names through `ItemCatalog`, party / monster targets through the
    /// session's `targets` field. Drives both browsing and target-select
    /// phases via `inventory_use_draws_for`.
    fn items_session_draws(
        &self,
        s: &legaia_engine_core::inventory_use::InventoryUseSession,
    ) -> Vec<TextDraw> {
        use legaia_engine_core::inventory_use::InventoryUseState;
        // Each visible item row needs its name + count + admissibility.
        // The session's `filtered_items` already lists indices into
        // `items` that pass the context filter; we render every owned
        // item but dim the ones outside the filter.
        let filter_set: std::collections::HashSet<usize> =
            s.filtered_items.iter().copied().collect();
        // Count duplicate item-ids so the overlay shows one row per
        // unique id rather than one row per stack slot.
        let mut counts: std::collections::HashMap<u8, u8> = std::collections::HashMap::new();
        for id in &s.items {
            *counts.entry(*id).or_insert(0) =
                counts.get(id).copied().unwrap_or(0).saturating_add(1);
        }
        // Stable order from first-seen.
        let mut seen: std::collections::HashSet<u8> = std::collections::HashSet::new();
        let mut row_data: Vec<(String, u8, bool)> = Vec::new();
        for (i, id) in s.items.iter().enumerate() {
            if !seen.insert(*id) {
                continue;
            }
            let entry = s.catalog.get(*id);
            let name = entry
                .map(|e| e.name.to_string())
                .unwrap_or_else(|| format!("Item {id:02X}"));
            let count = counts.get(id).copied().unwrap_or(1);
            let admissible = filter_set.contains(&i);
            row_data.push((name, count, admissible));
        }
        let item_rows: Vec<legaia_engine_render::InventoryItemRow<'_>> = row_data
            .iter()
            .map(|(n, c, a)| legaia_engine_render::InventoryItemRow {
                name: n,
                count: *c,
                admissible: *a,
            })
            .collect();
        let target_rows: Vec<legaia_engine_render::InventoryTargetRow<'_>> = s
            .targets
            .iter()
            .map(|t| legaia_engine_render::InventoryTargetRow {
                name: &t.name,
                hp: t.hp,
                hp_max: t.hp_max,
                mp: t.mp,
                mp_max: t.mp_max,
                alive: t.alive,
            })
            .collect();
        let (phase, cursor) = match s.state {
            InventoryUseState::Browsing { cursor } => (0u8, cursor as u8),
            InventoryUseState::TargetSelect { cursor, .. } => (1u8, cursor as u8),
            _ => (0u8, 0),
        };
        let selected_item_name = s.current_item().map(|e| e.name);
        let in_battle = matches!(
            s.context,
            legaia_engine_core::inventory_use::InventoryContext::Battle
        );
        let args = legaia_engine_render::InventoryUseDrawArgs {
            items: &item_rows,
            targets: &target_rows,
            in_battle,
            cursor,
            phase,
            selected_item_name,
        };
        legaia_engine_render::inventory_use_draws_for(&self.font, args, (16, 32))
    }

    /// Build draws for the equipment overlay. Resolves slot labels
    /// through `EquipSlot::label`, candidate names from the engine's
    /// equipment catalog, and per-candidate stat deltas by diffing the
    /// active modifier against the slot's current occupant.
    fn equip_session_draws(
        &self,
        session: &legaia_engine_core::equip_session::EquipSession,
        char_slot: u8,
    ) -> Vec<TextDraw> {
        use legaia_engine_core::equip_session::EquipState;
        use legaia_engine_core::equipment::EquipSlot;

        // Display name comes from the world's roster snapshot; fall back
        // to "Slot N" if the world doesn't have a record for the slot.
        let names = legaia_engine_core::field_menu_dispatch::roster_names(&self.session.host.world);
        let character_name = names
            .get(char_slot as usize)
            .cloned()
            .unwrap_or_else(|| format!("Slot {}", char_slot + 1));

        let record = session.record();
        let mut slot_label_buf: Vec<String> = Vec::with_capacity(8);
        for i in 0..8u8 {
            let label = EquipSlot::from_index(i)
                .map(|s| s.label().to_string())
                .unwrap_or_else(|| format!("Slot {i}"));
            slot_label_buf.push(label);
        }
        let mut slot_item_buf: Vec<String> = Vec::with_capacity(8);
        for &id in record.equip.iter() {
            slot_item_buf.push(if id == 0 {
                "(empty)".to_string()
            } else {
                format!("Item {id:02X}")
            });
        }
        let slot_rows: Vec<legaia_engine_render::EquipSlotRow<'_>> = (0..8usize)
            .map(|i| legaia_engine_render::EquipSlotRow {
                label: &slot_label_buf[i],
                current_name: &slot_item_buf[i],
            })
            .collect();

        let (phase, cursor, active_slot, confirm_label_owned) = match session.state() {
            EquipState::SlotPicker { cursor } => (
                legaia_engine_render::EquipDrawPhase::SlotPicker,
                cursor as u16,
                cursor,
                None,
            ),
            EquipState::ItemPicker { slot, cursor } => (
                legaia_engine_render::EquipDrawPhase::ItemPicker,
                cursor,
                slot,
                None,
            ),
            EquipState::Confirm {
                slot,
                item_id,
                cursor,
            } => {
                let label = format!("Equip Item {item_id:02X}?");
                (
                    legaia_engine_render::EquipDrawPhase::Confirm,
                    cursor as u16,
                    slot,
                    Some(label),
                )
            }
            EquipState::Done(_) => (legaia_engine_render::EquipDrawPhase::SlotPicker, 0, 0, None),
        };

        // Candidates only matter when we're past the slot picker.
        let (candidate_names, candidate_meta): (Vec<String>, Vec<(u8, i16, i16)>) =
            if phase == legaia_engine_render::EquipDrawPhase::SlotPicker {
                (Vec::new(), Vec::new())
            } else {
                let items = session.items_for_slot(active_slot);
                let current_id = record.equip[active_slot as usize];
                let current_mod = session
                    .equipment()
                    .get(current_id)
                    .copied()
                    .unwrap_or_default();
                let names: Vec<String> = items
                    .iter()
                    .map(|it| format!("Item {:02X}", it.id))
                    .collect();
                let meta: Vec<(u8, i16, i16)> = items
                    .iter()
                    .map(|it| {
                        let cand_mod = session.equipment().get(it.id).copied().unwrap_or_default();
                        let count = session.inventory().get(&it.id).copied().unwrap_or(0);
                        (
                            count,
                            cand_mod.atk - current_mod.atk,
                            cand_mod.udf - current_mod.udf,
                        )
                    })
                    .collect();
                (names, meta)
            };
        let candidate_rows: Vec<legaia_engine_render::EquipCandidateRow<'_>> = candidate_meta
            .iter()
            .enumerate()
            .map(
                |(i, (count, da, du))| legaia_engine_render::EquipCandidateRow {
                    name: &candidate_names[i],
                    count: *count,
                    atk_delta: *da,
                    udf_delta: *du,
                },
            )
            .collect();

        let args = legaia_engine_render::EquipDrawArgs {
            character_name: &character_name,
            slots: &slot_rows,
            candidates: &candidate_rows,
            phase,
            cursor,
            active_slot,
            confirm_label: confirm_label_owned.as_deref(),
        };
        legaia_engine_render::equipment_session_draws_for(&self.font, args, (16, 32))
    }

    /// Build draws for the Tactical Arts editor overlay. Pulls the
    /// saved-chain library snapshot the editor took at construction; the
    /// editor's `library_view` is the authoritative source until the
    /// engine calls `apply_outcome`.
    fn arts_session_draws(
        &self,
        s: &legaia_engine_core::tactical_arts_editor::ChainEditor,
    ) -> Vec<TextDraw> {
        use legaia_engine_core::tactical_arts_editor::{ChainLibrary, EditorPhase};
        let char_slot = s.char_slot();
        let names = legaia_engine_core::field_menu_dispatch::roster_names(&self.session.host.world);
        let character_name = names
            .get(char_slot as usize)
            .cloned()
            .unwrap_or_else(|| format!("Slot {}", char_slot + 1));

        let saved = s.library_view();
        let pretty_buf: Vec<String> = saved.iter().map(|c| c.pretty_sequence()).collect();
        let saved_rows: Vec<legaia_engine_render::ArtsChainRow<'_>> = saved
            .iter()
            .enumerate()
            .map(|(i, c)| legaia_engine_render::ArtsChainRow {
                name: &c.name,
                pretty_sequence: &pretty_buf[i],
            })
            .collect();

        let (phase_tag, browse_cursor, editing_pretty_owned, editing_len, naming_name_owned) =
            match s.phase() {
                EditorPhase::Browsing { cursor } => (
                    legaia_engine_render::ArtsEditorPhase::Browsing,
                    *cursor,
                    String::new(),
                    0usize,
                    String::new(),
                ),
                EditorPhase::Editing { working } => {
                    let pretty = working
                        .iter()
                        .map(|c| match c {
                            legaia_art::queue::Command::Left => "L",
                            legaia_art::queue::Command::Right => "R",
                            legaia_art::queue::Command::Up => "U",
                            legaia_art::queue::Command::Down => "D",
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    (
                        legaia_engine_render::ArtsEditorPhase::Editing,
                        0u8,
                        pretty,
                        working.len(),
                        String::new(),
                    )
                }
                EditorPhase::Naming { working, name } => {
                    let pretty = working
                        .iter()
                        .map(|c| match c {
                            legaia_art::queue::Command::Left => "L",
                            legaia_art::queue::Command::Right => "R",
                            legaia_art::queue::Command::Up => "U",
                            legaia_art::queue::Command::Down => "D",
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    (
                        legaia_engine_render::ArtsEditorPhase::Naming,
                        0u8,
                        pretty,
                        working.len(),
                        name.clone(),
                    )
                }
                EditorPhase::Done(_) => (
                    legaia_engine_render::ArtsEditorPhase::Browsing,
                    0u8,
                    String::new(),
                    0usize,
                    String::new(),
                ),
            };

        let can_add_new = saved.len() < ChainLibrary::MAX_SLOTS;
        let args = legaia_engine_render::ArtsEditorDrawArgs {
            character_name: &character_name,
            phase: phase_tag,
            saved: &saved_rows,
            browse_cursor,
            editing_pretty: &editing_pretty_owned,
            editing_len,
            min_len: ChainLibrary::MIN_LEN,
            max_len: ChainLibrary::MAX_LEN,
            naming_name: &naming_name_owned,
            can_add_new,
        };
        legaia_engine_render::tactical_arts_editor_draws_for(&self.font, args, (16, 32))
    }
}

impl PlayWindowApp {
    /// Resolve the field static-geometry placement draws for the current
    /// scene: each placed environment object's scene-pack mesh paired with a
    /// world model matrix. Built from the field map's object table
    /// (`Scene::field_object_placements`) and the scene_asset_table TMD pack;
    /// the per-object pack index resolves via `legaia_asset::field_objects`.
    ///
    /// `tmd_src_index[j]` is the `res.tmds` index of uploaded mesh `j` (meshes
    /// skip empty-prim TMDs, so this bridges back). Returns empty for scenes
    /// with no field map / no bundle (e.g. battle or world-map blocks).
    ///
    /// World Y is left at the ground plane for now; the per-tile floor-height
    /// LUT (MAN header) is a separate refinement.
    fn resolve_field_placement_draws(
        &self,
        res: &SceneResources,
        tmd_src_index: &[usize],
    ) -> Vec<(usize, Mat4)> {
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Vec::new();
        };
        let placements = match scene.field_object_placements(&self.session.host.index) {
            Ok(Some(p)) if !p.is_empty() => p,
            _ => return Vec::new(),
        };
        self.resolve_placement_draws(res, tmd_src_index, &placements)
    }

    /// Resolve the field scene's **terrain / ground** tiles (the `CELL_VISIBLE`
    /// sweep in `Scene::field_terrain_tiles`) to `(mesh, model)` draws, the same
    /// way `resolve_field_placement_draws` resolves the placed objects. This is
    /// the town's floor / ground layer; without it a field scene renders its
    /// buildings floating over the bare clear colour.
    fn resolve_field_terrain_draws(
        &self,
        res: &SceneResources,
        tmd_src_index: &[usize],
    ) -> Vec<(usize, Mat4)> {
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Vec::new();
        };
        let tiles = match scene.field_terrain_tiles(&self.session.host.index) {
            Ok(Some(t)) if !t.is_empty() => t,
            _ => return Vec::new(),
        };
        self.resolve_placement_draws(res, tmd_src_index, &tiles)
    }

    /// World-map continent terrain draws: the dense visible-tile set
    /// (`Scene::field_terrain_tiles`, the `FUN_801F69D8` overhead sweep) rather
    /// than the placed-flag interactive objects. Tiles whose pack index falls
    /// outside the loaded slot-1 landmark pack (they reference the wider global
    /// TMD pool, not yet loaded for the world map) resolve to no mesh and are
    /// skipped by `resolve_placement_draws`.
    fn resolve_world_map_terrain_draws(
        &self,
        res: &SceneResources,
        tmd_src_index: &[usize],
    ) -> Vec<(usize, Mat4)> {
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Vec::new();
        };
        // Free-roam walk view: read the *walk* `.MAP` (`Scene::walk_field_map_
        // index`, the `block_start - 2` entry the runtime resolves through
        // `toc[idx+2]`) and sweep its `0x1000`-gated continent (`walk_terrain_
        // tiles`), then the placed-flag landmarks from the same `.MAP`. The
        // earlier path read the within-block decoy entry with the overhead
        // `0x2000` gate, which for the kingdoms resolved a different map and
        // produced the sparse mesh scatter.
        // Only the sparse placed landmarks (FUN_8003A55C, flags & 0x4) resolve
        // to slot-1 pack meshes via record[+0x10]+prefix. The bulk continent
        // ground is NOT per-cell pack meshes (the old `walk_terrain_tiles`
        // sweep floods 97% of cells with pool-5 because their record[+0x10] is
        // 0); it is the heightfield surface built separately in `upload_assets`
        // (`Scene::walk_heightfield`). See docs/subsystems/world-map.md.
        let tiles = match scene.walk_object_placements(&self.session.host.index) {
            Ok(Some(t)) => t,
            _ => Vec::new(),
        };
        if tiles.is_empty() {
            return Vec::new();
        }
        self.resolve_placement_draws(res, tmd_src_index, &tiles)
    }

    /// Resolve the world-map ocean CLUT animation for the active scene: scan the
    /// scene's PROT entries for the kingdom bundle, decode its slot-0 TIM_LIST,
    /// and pull the ocean tile's 13-frame CLUT animation table
    /// ([`legaia_asset::ocean::find_ocean_assets`]). Returns `None` for a
    /// non-world-map scene or a bundle without the ocean tile. The ocean tile
    /// texture and its base CLUT are already uploaded into VRAM by the slot-0
    /// TIM pass; this only recovers the per-frame palette overrides.
    fn resolve_ocean_anim(&self) -> Option<OceanAnim> {
        let scene = self.session.host.scene.as_ref()?;
        if !legaia_engine_core::scene::is_world_map_scene(&scene.name) {
            return None;
        }
        for entry in &scene.entries {
            let Ok(slot0) = legaia_asset::kingdom_bundle::decode_slot(&entry.bytes, 0) else {
                continue;
            };
            if let Some(ocean) = legaia_asset::ocean::find_ocean_assets(&slot0)
                && ocean.animation_frames.len() >= 32
            {
                return Some(OceanAnim {
                    frames: ocean.animation_frames,
                    cur: 0,
                    tick: 0,
                });
            }
        }
        None
    }

    /// Advance the world-map ocean CLUT animation one sim tick. When the frame
    /// cursor crosses [`OCEAN_ANIM_TICKS_PER_FRAME`], write the next frame's 16
    /// BGR555 entries into the CPU VRAM CLUT row at `(0, 506)` and re-upload the
    /// VRAM so the heightfield's water cells (which sample that CLUT) shimmer.
    /// No-op when no ocean animation is loaded. Cheap: the whole-VRAM re-upload
    /// fires only on a frame change (~10x/s), not every render frame.
    fn advance_ocean_animation(&mut self) {
        // While a battle is up the GPU texture holds the BATTLE VRAM (party
        // band + palettes + monster pages); the ocean cells aren't visible
        // under the battle stage, and re-uploading the field snapshot here
        // would clobber that texture so every battle mesh samples field
        // bytes (white speckle on the party band). Hold the shimmer until
        // the field VRAM is restored at battle exit.
        if self.session.host.world.mode == SceneMode::Battle {
            return;
        }
        let frame: [u8; 32] = {
            let Some(anim) = self.ocean_anim.as_mut() else {
                return;
            };
            anim.tick += 1;
            if anim.tick < OCEAN_ANIM_TICKS_PER_FRAME {
                return;
            }
            anim.tick = 0;
            let nframes = anim.frames.len() / 32;
            if nframes == 0 {
                return;
            }
            anim.cur = (anim.cur + 1) % nframes;
            let off = anim.cur * 32;
            anim.frames[off..off + 32].try_into().unwrap()
        };
        let Some(base) = self.cpu_vram_base.as_mut() else {
            return;
        };
        // CLUT row at VRAM (0, 506) - the retail per-frame ocean DMA target.
        base.write_clut_row(0, 506, &frame);
        if let Some(r) = self.win.renderer.as_ref() {
            match r.upload_vram(base) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("play-window: ocean CLUT re-upload: {e:#}"),
            }
        }
    }

    /// Shared placement -> world-transform resolver for both the field static-
    /// object layer and the world-map continent terrain. Maps each placement's
    /// scene-pack mesh index through the uploaded-mesh bridge and builds its
    /// world model matrix.
    fn resolve_placement_draws(
        &self,
        res: &SceneResources,
        tmd_src_index: &[usize],
        placements: &[legaia_asset::field_objects::Placement],
    ) -> Vec<(usize, Mat4)> {
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Vec::new();
        };
        if placements.is_empty() {
            return Vec::new();
        }
        // Per-tile floor-height LUT (MAN header). World Y for a placed object
        // is `-lut[tile_floor_nibble] + y_off`; without it the town renders on
        // a flat plane (Rim Elm is on a cliff with real elevation changes).
        let floor_lut = scene
            .field_floor_height_lut(&self.session.host.index)
            .ok()
            .flatten();
        // The environment meshes are the scene_asset_table bundle entry's TMD
        // pack, in scan order; `pack_index` indexes that subset of `res.tmds`.
        let Some(bundle_entry) =
            legaia_engine_core::scene_bundle::find_bundle(scene).map(|b| b.entry_idx())
        else {
            return Vec::new();
        };
        let env_tmds: Vec<usize> = res
            .tmds
            .iter()
            .enumerate()
            .filter(|(_, t)| t.entry_idx == bundle_entry)
            .map(|(i, _)| i)
            .collect();
        // res.tmds index -> uploaded-mesh index (None where the mesh was
        // dropped for having no renderable prims).
        let mut res_to_mesh: Vec<Option<usize>> = vec![None; res.tmds.len()];
        for (mesh_idx, &src) in tmd_src_index.iter().enumerate() {
            if let Some(slot) = res_to_mesh.get_mut(src) {
                *slot = Some(mesh_idx);
            }
        }
        let diag = std::env::var_os("LEGAIA_DIAG_PLACE").is_some();
        let mut draws = Vec::new();
        for p in placements {
            let Some(pack_index) = p.pack_index else {
                if diag {
                    log::info!(
                        "DIAG place drop: no pack_index at ({}, {})",
                        p.world_x,
                        p.world_z
                    );
                }
                continue;
            };
            let Some(&res_idx) = env_tmds.get(pack_index as usize) else {
                if diag {
                    log::info!(
                        "DIAG place drop: pack_index {} out of range ({} env tmds) at ({}, {})",
                        pack_index,
                        env_tmds.len(),
                        p.world_x,
                        p.world_z
                    );
                }
                continue;
            };
            let Some(mesh_idx) = res_to_mesh[res_idx] else {
                if diag {
                    log::info!(
                        "DIAG place drop: pack {} (res {}) not in this mesh bridge at ({}, {})",
                        pack_index,
                        res_idx,
                        p.world_x,
                        p.world_z
                    );
                }
                continue;
            };
            // World Y from the floor-height LUT (`-lut[nibble] + y_off`), or
            // the ground plane when the LUT / nibble is unavailable.
            let world_y = match (floor_lut, p.floor_nibble) {
                (Some(lut), Some(nib)) => -(lut[(nib & 0x0F) as usize] as i32) + p.y_off as i32,
                _ => 0,
            };
            // PSX field coords (same convention as actor positions), Y-flipped
            // to match the geometry like `actor_model`.
            let model = Mat4::from_translation(Vec3::new(
                p.world_x as f32,
                world_y as f32,
                p.world_z as f32,
            )) * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
            draws.push((mesh_idx, model));
        }
        log::info!(
            "play-window: {} field placement draws ({} placements, {} env meshes)",
            draws.len(),
            placements.len(),
            env_tmds.len(),
        );
        draws
    }

    fn upload_assets(&mut self) {
        let Some(res) = self.scene_res.take() else {
            return;
        };
        let (
            vram_opt,
            font_opt,
            meshes,
            tmd_data,
            tmd_src_index,
            color_meshes,
            color_tmd_src_index,
            lo,
            hi,
            world_map_hf,
        ) = {
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
            // `res.tmds` index for each pushed mesh (meshes skip empty-prim
            // TMDs, so this is the bridge back to a `ResolvedTmd`).
            let mut tmd_src_index: Vec<usize> = Vec::new();
            // Untextured (F*/G*) prop meshes + their res.tmds-index bridge,
            // parallel to `meshes` / `tmd_src_index` but on the colour pipeline.
            let mut color_meshes: Vec<UploadedColorMesh> = Vec::new();
            let mut color_tmd_src_index: Vec<usize> = Vec::new();
            let mut lo = [f32::INFINITY; 3];
            let mut hi = [f32::NEG_INFINITY; 3];
            for (src_i, rtmd) in res.tmds.iter().enumerate() {
                // Build this mesh's untextured (F*/G*) vertex-colour primitives
                // and upload them to the colour pipeline. `tmd_to_color_mesh`
                // skips textured groups, so these are DISJOINT from the VRAM-mesh
                // primitives below: a mesh carrying both textured and untextured
                // primitives now renders BOTH halves rather than only the
                // textured one (the old code built the colour mesh only when the
                // textured build came back empty, silently dropping the
                // untextured half of a mixed mesh). A mesh whose only textured
                // prims reference missing CLUTs yields an empty colour mesh AND
                // an empty filtered VRAM mesh, so it correctly stays dropped.
                let cmesh = legaia_tmd::mesh::tmd_to_color_mesh(&rtmd.tmd, &rtmd.raw);
                if !cmesh.is_empty() {
                    for p in &cmesh.positions {
                        for ax in 0..3 {
                            if p[ax] < lo[ax] {
                                lo[ax] = p[ax];
                            }
                            if p[ax] > hi[ax] {
                                hi[ax] = p[ax];
                            }
                        }
                    }
                    match r.upload_color_mesh_blended(
                        &cmesh.positions,
                        &cmesh.colors,
                        &cmesh.indices,
                        &cmesh.blend,
                    ) {
                        Ok(m) => {
                            color_meshes.push(m);
                            color_tmd_src_index.push(src_i);
                        }
                        Err(e) => log::warn!("color mesh upload skipped: {e:#}"),
                    }
                }

                // Use the VRAM-aware filter so prims whose CBA / TSB sample
                // un-uploaded regions get dropped at mesh-build time. This
                // matches the asset-viewer's cleanup and avoids the "flat
                // green CLUT[0]" shells over correctly-textured geometry
                // that the unfiltered builder produces.
                let vmesh = rtmd.build_filtered_vram_mesh(&res.vram);
                if vmesh.indices.is_empty() {
                    if std::env::var_os("LEGAIA_DIAG_PLACE").is_some() {
                        let (_, stats) = rtmd.build_filtered_vram_mesh_reasoned(&res.vram);
                        let mut missing = std::collections::BTreeSet::new();
                        let _ = legaia_tmd::mesh::tmd_to_vram_mesh_filtered(
                            &rtmd.tmd,
                            &rtmd.raw,
                            |cba, tsb, uvs| {
                                if !res.vram.prim_has_texture_data(cba, tsb, uvs) {
                                    missing.insert((cba, tsb));
                                }
                                false
                            },
                        );
                        let decoded: Vec<String> = missing
                            .iter()
                            .map(|&(cba, tsb)| {
                                format!(
                                    "cba={cba:#x} CLUT({},{}) tsb={tsb:#x} page({},{}) bpp{}",
                                    (cba & 0x3f) * 16,
                                    cba >> 6,
                                    (tsb & 0xf) * 64,
                                    ((tsb >> 4) & 1) * 256,
                                    4 << ((tsb >> 7) & 3)
                                )
                            })
                            .collect();
                        log::info!(
                            "DIAG mesh drop: res {} (entry {} off {:#x}): textured filter empty \
                             ({stats:?}, color mesh empty: {}) wants: {}",
                            src_i,
                            rtmd.entry_idx,
                            rtmd.offset,
                            cmesh.is_empty(),
                            decoded.join(" | ")
                        );
                    }
                    // No textured prims survived the VRAM filter; the colour prims
                    // (if any) were already uploaded above.
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
                        tmd_src_index.push(src_i);
                    }
                    Err(e) => log::warn!("TMD upload skipped: {e:#}"),
                }
            }
            // Bulk **ground** heightfield: the surface built from the `.MAP`
            // floor grid (the pack meshes are only the placed objects /
            // landmarks, not a per-cell ground mesh), textured per cell from
            // the terrain-type-keyed multi-page atlas - the heightfield
            // bakes the per-cell tile UV (`+0x14`) and page+palette
            // (`+0x15`/`+0x16..+0x18`) so grass / mountain / water cells sample
            // their own VRAM page. See docs/subsystems/world-map.md
            // "Ground texturing".
            //
            // This is NOT world-map-only: town `.MAP`s carry the same
            // `0x1000`-gated ground layer (town01: ~1.9k ground cells vs the
            // 208 `0x2000` decor tiles, most with `record[+0x10] = 0` = no
            // pack mesh), and the retail town VRAM has the terrain atlas
            // pages resident. Without this surface the town floor renders
            // as holes wherever no `0x2000` tile covers a cell.
            let mut world_map_hf: Option<UploadedVramMesh> = None;
            if let Some(scene) = self.session.host.scene.as_ref()
                && let Ok(Some(hf)) = scene.walk_heightfield(&self.session.host.index)
                && !hf.indices.is_empty()
            {
                let vmesh = heightfield_to_vram_mesh(&hf);
                match r.upload_vram_mesh(
                    &vmesh.positions,
                    &vmesh.uvs,
                    &vmesh.cba_tsb,
                    &vmesh.normals,
                    &vmesh.indices,
                ) {
                    Ok(m) => {
                        log::info!(
                            "play-window: ground heightfield {} quads ({} verts)",
                            hf.quad_count(),
                            hf.positions.len()
                        );
                        world_map_hf = Some(m);
                    }
                    Err(e) => log::warn!("heightfield upload skipped: {e:#}"),
                }
            }
            (
                vram,
                font,
                meshes,
                tmd_data,
                tmd_src_index,
                color_meshes,
                color_tmd_src_index,
                lo,
                hi,
                world_map_hf,
            )
        };
        // Kingdom slot-4 inspection wireframe (env-gated). Build the
        // world-space line geometry once at scene load; the per-frame
        // world-map render merges it into the overlay-lines buffer.
        let world_map_slot4_lines = if std::env::var_os("LEGAIA_WORLDMAP_SLOT4").is_some() {
            res.world_map_slot4.as_ref().and_then(|slot| {
                let (p, c, i) = world_map_slot4_line_geometry(slot);
                if i.is_empty() {
                    None
                } else {
                    log::info!(
                        "play-window: world-map slot-4 wireframe {} bodies, {} segments",
                        slot.bodies.len(),
                        i.len() / 2
                    );
                    Some((p, c, i))
                }
            })
        } else {
            None
        };
        // Resolve the field static-geometry placement draws: each placed
        // environment object -> its scene-pack mesh -> a world transform.
        // Built here (not per-frame) because the placement table + pack are
        // fixed for the scene; the field draw branch just replays the list.
        let field_placement_draws = self.resolve_field_placement_draws(&res, &tmd_src_index);
        // Same resolver, but bridged through the colour-mesh list: the untextured
        // props' placement transforms map to `color_meshes` indices.
        let field_placement_color_draws =
            self.resolve_field_placement_draws(&res, &color_tmd_src_index);
        let field_terrain_draws = self.resolve_field_terrain_draws(&res, &tmd_src_index);
        // Untextured ground tiles resolve through the colour-mesh bridge (the
        // textured bridge has no entry for them - they'd render as floor holes).
        let field_terrain_color_draws =
            self.resolve_field_terrain_draws(&res, &color_tmd_src_index);
        log::info!(
            "play-window: {} field terrain draws (ground layer, +{} colour tiles)",
            field_terrain_draws.len(),
            field_terrain_color_draws.len()
        );
        let world_map_terrain_draws = self.resolve_world_map_terrain_draws(&res, &tmd_src_index);
        // Field move-VM stager scene-pack TMD list: `env_tmds` (res.tmds @ the
        // scene_asset_table bundle entry, scan order) = retail `DAT_8007C018[5..]`,
        // indexed by a field stager's relative `model_sel`. Kept as its own clone
        // because `res` is consumed below; the field-FX render path looks meshes up
        // here instead of the battle `global_tmd_pool`.
        let field_stager_tmds: Vec<(legaia_tmd::Tmd, Vec<u8>)> = self
            .session
            .host
            .scene
            .as_ref()
            .and_then(|scene| {
                legaia_engine_core::scene_bundle::find_bundle(scene).map(|b| b.entry_idx())
            })
            .map(|bundle_entry| {
                res.tmds
                    .iter()
                    .filter(|t| t.entry_idx == bundle_entry)
                    .map(|t| (t.tmd.clone(), t.raw.clone()))
                    .collect()
            })
            .unwrap_or_default();
        self.field_stager_tmds = field_stager_tmds;
        // Keep a clean CPU copy of the scene VRAM so a battle can inject
        // monster textures into a throwaway clone and restore this on exit.
        self.cpu_vram_base = Some(res.vram.clone());
        if let Some(v) = vram_opt {
            self.uploaded_vram = Some(v);
        }
        if let Some(a) = font_opt {
            self.font_atlas = Some(a);
        }
        // Upload the publisher-logos atlas (if pre-decoded at boot) now
        // that the renderer is live.
        if let (Some(atlas_data), Some(r)) = (
            self.pending_publisher_logos_atlas.take(),
            self.win.renderer.as_ref(),
        ) {
            match r.upload_sprite_atlas(&atlas_data.rgba, atlas_data.width, atlas_data.height) {
                Ok(atlas) => {
                    log::info!(
                        "play-window: publisher-logos atlas uploaded ({}x{})",
                        atlas_data.width,
                        atlas_data.height
                    );
                    self.publisher_logos = Some(PublisherLogosAssets {
                        rects: atlas_data.rects,
                        atlas,
                    });
                }
                Err(e) => log::warn!("publisher-logos atlas upload skipped: {e:#}"),
            }
        }
        // Upload the title-screen atlas the same way.
        if let (Some(atlas_data), Some(r)) = (
            self.pending_title_screen_atlas.take(),
            self.win.renderer.as_ref(),
        ) {
            match r.upload_sprite_atlas(&atlas_data.rgba, atlas_data.width, atlas_data.height) {
                Ok(atlas) => {
                    log::info!(
                        "play-window: title-screen atlas uploaded ({}x{})",
                        atlas_data.width,
                        atlas_data.height
                    );
                    self.title_screen = Some(TitleScreenAssets {
                        rect: atlas_data.rect,
                        atlas,
                    });
                }
                Err(e) => log::warn!("title-screen atlas upload skipped: {e:#}"),
            }
        }
        // Upload the menu-glyph atlas the same way.
        if let (Some(atlas_data), Some(r)) = (
            self.pending_menu_glyph_atlas.take(),
            self.win.renderer.as_ref(),
        ) {
            match r.upload_sprite_atlas(&atlas_data.rgba, atlas_data.width, atlas_data.height) {
                Ok(atlas) => {
                    log::info!(
                        "play-window: menu-glyph atlas uploaded ({}x{})",
                        atlas_data.width,
                        atlas_data.height
                    );
                    self.menu_glyphs = Some(MenuGlyphAssets { atlas });
                }
                Err(e) => log::warn!("menu-glyph atlas upload skipped: {e:#}"),
            }
        }
        // Upload the save-menu UI atlas (PROT 0899) the same way.
        if let (Some(atlas_data), Some(r)) = (
            self.pending_save_menu_atlas.take(),
            self.win.renderer.as_ref(),
        ) {
            match r.upload_sprite_atlas(&atlas_data.rgba, atlas_data.width, atlas_data.height) {
                Ok(atlas) => {
                    log::info!(
                        "play-window: save-menu atlas uploaded ({}x{})",
                        atlas_data.width,
                        atlas_data.height
                    );
                    let rects = legaia_engine_render::SaveMenuAtlasRects {
                        panel_tl: atlas_data.band_panel_tl(),
                        panel_tr: atlas_data.band_panel_tr(),
                        panel_bl: atlas_data.band_panel_bl(),
                        panel_br: atlas_data.band_panel_br(),
                        panel_top: atlas_data.band_panel_top(),
                        panel_bot: atlas_data.band_panel_bot(),
                        panel_left: atlas_data.band_panel_left(),
                        panel_right: atlas_data.band_panel_right(),
                        slot1: atlas_data.band_slot1(),
                        slot2: atlas_data.band_slot2(),
                        cursor: atlas_data.band_cursor(),
                        panel_interior: atlas_data.band_panel_interior(),
                        load_empty_frame: Some(atlas_data.band_load_empty_frame()),
                        load_portrait_by_char: [
                            atlas_data.band_load_portrait(0),
                            atlas_data.band_load_portrait(1),
                            atlas_data.band_load_portrait(2),
                        ],
                    };
                    self.save_menu = Some(SaveMenuAssets { rects, atlas });
                }
                Err(e) => log::warn!("save-menu atlas upload skipped: {e:#}"),
            }
        }
        self.meshes = meshes;
        self.scene_tmd_data = tmd_data;
        self.field_terrain_draws = field_terrain_draws;
        self.field_terrain_color_draws = field_terrain_color_draws;
        self.field_placement_draws = field_placement_draws;
        self.color_meshes = color_meshes;
        self.field_placement_color_draws = field_placement_color_draws;
        self.world_map_terrain_draws = world_map_terrain_draws;
        self.ground_heightfield = world_map_hf;
        self.world_map_slot4_lines = world_map_slot4_lines;
        // World-map ocean: recover the 13-frame CLUT animation for the kingdom
        // (the ocean texture + base CLUT are already uploaded by the slot-0 TIM
        // pass). `None` off the world map, so the per-tick advance self-gates.
        self.ocean_anim = self.resolve_ocean_anim();
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
        // Bind the PLAYER's real character mesh. Town scripts install the
        // player engine-side (`install_field_player`), not via the field-VM
        // `0x4C 0xD8` spawn, so no `tmd_ref` spawn event ever fires for it -
        // and the naive pre-bind above points actor 0 at whatever scene TMD
        // shares its slot index (usually an init_data UI mesh = invisible
        // player). Resolve the lead's field mesh from the global TMD pool
        // (PROT 0874 §0, retail `DAT_8007C018[0..4]`, seeded by
        // `enter_field_scene`) and override the placeholder binding.
        self.player_color_draw = None;
        if world.mode == SceneMode::Field
            && let Some(pslot) = world.player_actor_slot
        {
            let lead = world.active_party.first().copied().unwrap_or(0) as usize;
            let gtmd = world
                .global_tmd_pool
                .get(lead)
                .and_then(|s| s.as_ref())
                .map(std::sync::Arc::clone);
            if let Some(g) = gtmd {
                // Rest pose: frame 0 of the character's standing-idle clip in
                // the party locomotion ANM bundle (PROT 0874 §1, bank slot 1;
                // pinned live - see `character_pack::LOCOMOTION_IDLE_SLOT`).
                // Retail caps the live object count to the clip's bone count
                // (10; groups 10/11 are equipment-swap templates, never drawn
                // - `FUN_8001E890` / `FUN_80024d78`), so truncate the disc
                // TMD's 12-object table to match before posing bone i ->
                // object i.
                // (bone_count, per-bone (translation, rotation) pairs)
                type IdlePose = (usize, Vec<([i16; 3], [i16; 3])>);
                let idle_pose: Option<IdlePose> = self
                    .session
                    .host
                    .index
                    .entry_bytes(legaia_asset::character_pack::PROT_ENTRY_INDEX)
                    .ok()
                    .and_then(|b| legaia_asset::character_pack::field_locomotion_anm(&b).ok())
                    .and_then(|bundle| {
                        if lead > 2 {
                            return None; // banks cover the Vahn/Noa/Gala trio
                        }
                        let rec_idx = legaia_asset::character_pack::locomotion_record_index(
                            lead,
                            legaia_asset::character_pack::LOCOMOTION_IDLE_SLOT,
                        );
                        let rec = bundle.record(rec_idx).ok()?;
                        let bones = rec.bone_count as usize;
                        let offsets: Vec<([i16; 3], [i16; 3])> = (0..bones)
                            .map(|bidx| match bundle.bone_transform(rec_idx, 0, bidx) {
                                Some(t) => (
                                    [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                                    [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                                ),
                                None => ([0; 3], [0; 3]),
                            })
                            .collect();
                        Some((bones, offsets))
                    });
                let vmesh = match &idle_pose {
                    Some((bones, offsets)) => {
                        let mut tmd = g.tmd.clone();
                        tmd.objects.truncate(*bones);
                        legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(&tmd, &g.raw, offsets)
                    }
                    None => legaia_tmd::mesh::tmd_to_vram_mesh(&g.tmd, &g.raw),
                };
                if !vmesh.indices.is_empty()
                    && let Some(r) = self.win.renderer.as_ref()
                {
                    match r.upload_vram_mesh(
                        &vmesh.positions,
                        &vmesh.uvs,
                        &vmesh.cba_tsb,
                        &vmesh.normals,
                        &vmesh.indices,
                    ) {
                        Ok(m) => {
                            let new_idx = self.meshes.len();
                            self.meshes.push(m);
                            self.scene_tmd_data.push((g.tmd.clone(), g.raw.clone()));
                            world.set_actor_tmd_binding(pslot as usize, new_idx);
                            self.drained_spawn_slots.insert(pslot);
                            // The untextured half (pants / sleeves are F*/G*
                            // colour prims the VRAM mesh can't carry) rides
                            // the colour pipeline, posed with the same bone
                            // set, drawn per-frame at the actor's live model.
                            if let Some((bones, offsets)) = &idle_pose {
                                let mut tmd = g.tmd.clone();
                                tmd.objects.truncate(*bones);
                                let cmesh = legaia_tmd::mesh::tmd_to_color_mesh_posed_rot(
                                    &tmd, &g.raw, offsets,
                                );
                                if !cmesh.is_empty() {
                                    match r.upload_color_mesh_blended(
                                        &cmesh.positions,
                                        &cmesh.colors,
                                        &cmesh.indices,
                                        &cmesh.blend,
                                    ) {
                                        Ok(cm) => {
                                            let cidx = self.color_meshes.len();
                                            self.color_meshes.push(cm);
                                            self.player_color_draw = Some((cidx, pslot as usize));
                                        }
                                        Err(e) => log::warn!(
                                            "play-window: player colour half upload: {e:#}"
                                        ),
                                    }
                                }
                            }
                            log::info!(
                                "play-window: player (roster {lead}) -> pool mesh slot {new_idx} \
                                 ({}{})",
                                if idle_pose.is_some() {
                                    "rest-posed from locomotion idle frame 0"
                                } else {
                                    "unposed: locomotion bundle unavailable"
                                },
                                if self.player_color_draw.is_some() {
                                    ", + colour half"
                                } else {
                                    ""
                                }
                            );
                        }
                        Err(e) => log::warn!("play-window: player mesh upload: {e:#}"),
                    }
                }
            } else {
                log::warn!(
                    "play-window: global TMD pool has no entry for roster slot {lead}; \
                     player keeps the placeholder binding"
                );
            }
        }
        // Initial floor snap: locomotion only re-samples the floor height on a
        // step, so an actor standing still since spawn keeps `world_y = 0`
        // while the town ground sits at a LUT-elevated tier - it renders
        // buried under (or floating over) the terrain until it first moves.
        // Snap every bound actor that still has the flat default.
        if world.follow_terrain_height && world.mode == SceneMode::Field {
            for i in 0..world.actors.len() {
                if world.actors[i].tmd_binding.is_none() {
                    continue;
                }
                let ms = &world.actors[i].move_state;
                if ms.world_y != 0 {
                    continue;
                }
                let y = world.sample_field_floor_height(ms.world_x as i32, ms.world_z as i32);
                world.actors[i].move_state.world_y = y as i16;
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
        // Frame the player's vicinity, not the whole scene. Loading the full
        // town environment-geometry pack makes `scene_aabb` (the union of every
        // mesh's local extent) span thousands of units, so fitting it pulls the
        // orbit camera far enough out that the actually-drawn terrain near the
        // player shrinks to a speck. Until per-mesh world placement lands, build
        // a fixed-size framing box around the player actor (the field draws
        // actor-bound meshes at actor positions) so the view stays close.
        const FIELD_VIEW_HALF: f32 = 700.0;
        let (lo, hi) = self
            .session
            .host
            .world
            .actors
            .first()
            .filter(|p| p.active || p.tmd_binding.is_some())
            .map(|p| {
                let (cx, cy, cz) = (
                    p.move_state.world_x as f32,
                    p.move_state.world_y as f32,
                    p.move_state.world_z as f32,
                );
                (
                    [
                        cx - FIELD_VIEW_HALF,
                        cy - FIELD_VIEW_HALF,
                        cz - FIELD_VIEW_HALF,
                    ],
                    [
                        cx + FIELD_VIEW_HALF,
                        cy + FIELD_VIEW_HALF,
                        cz + FIELD_VIEW_HALF,
                    ],
                )
            })
            .unwrap_or((self.scene_aabb.0, self.scene_aabb.1));
        // Retail's field camera is a *fixed* per-scene 3/4 vantage that follows
        // the player, NOT a spinning orbit. Passing `elapsed_secs` here made the
        // camera rotate continuously (after ~15 s it points up at the sky with
        // the town splayed at the edges). Freeze the orbit angle to a fixed
        // diagonal and steepen the eye height to a town-like overhead pitch. The
        // AABB is still the player-centred box, so the view tracks the player.
        //
        // `orbit_camera_mvp` derives its azimuth from `elapsed_secs *
        // orbit_speed`; feed a constant "time" so the azimuth is fixed at
        // `FIELD_ORBIT_ANGLE`. Height ratio `FIELD_EYE_HEIGHT` sets the pitch
        // (`atan(height) ≈ 40deg`), matching Rim Elm's overhead framing.
        const FIELD_ORBIT_SPEED: f32 = 0.25;
        const FIELD_ORBIT_ANGLE: f32 = 0.75;
        const FIELD_EYE_HEIGHT: f32 = 0.85;
        let fixed_time = FIELD_ORBIT_ANGLE / FIELD_ORBIT_SPEED;
        orbit_camera_mvp(
            lo,
            hi,
            FIELD_ORBIT_SPEED,
            FIELD_EYE_HEIGHT,
            fixed_time,
            aspect,
        )
    }

    /// The retail **field follow camera**, parametrized from the town01
    /// anchor savestate's camera globals (see docs/subsystems/cutscene.md for
    /// the global map): pitch `_DAT_8007B790 = 450` (~39.6 deg down-tilt),
    /// yaw `_DAT_8007B792 = -160`, roll 0, GTE `H = _DAT_8007B6F4 = 512`.
    /// The look-at target is the player anchor - retail's follow-cam
    /// (`FUN_801DBE9C`) folds `-(anchor X/Z)` into the focus globals each
    /// frame. The eye-space depth is an engine calibration (retail's exact
    /// field TR composition isn't pinned yet - the offset trio in the
    /// savestate doesn't project to the observed framing); `FIELD_CAM_DEPTH`
    /// is fitted so the player's on-screen height matches the retail frame
    /// (~55 px of 240 for the ~130-unit mesh at H = 512).
    ///
    /// Falls back to the fixed orbit vantage (`camera_mvp`) when no player
    /// actor exists to follow.
    fn field_follow_camera_mvp(&self, aspect: f32) -> Option<Mat4> {
        const PITCH_UNITS: f32 = 450.0;
        const YAW_UNITS: f32 = -160.0;
        const FIELD_H: f32 = 512.0;
        const FIELD_CAM_DEPTH: f32 = 1200.0;
        let world = &self.session.host.world;
        let p = world
            .actors
            .first()
            .filter(|p| p.active || p.tmd_binding.is_some())?;
        let (wx, wz) = (p.move_state.world_x, p.move_state.world_z);
        // Anchor the look-at to the floor under the player, not the actor's
        // raw Y: `follow_terrain_height` is opt-in, so `world_y` is usually 0
        // while the town ground sits at a LUT-elevated tier - targeting y=0
        // there points the camera under the ground. The sampler returns the
        // retail-negated tier (up = negative, matching the placement world
        // Y); the camera target is subtracted post-Y-flip (PSX space), so
        // negate back.
        let floor_y = world.sample_field_floor_height(wx as i32, wz as i32);
        let target = Vec3::new(wx as f32, -floor_y as f32, wz as f32);
        let to_rad = |units: f32| units / 4096.0 * std::f32::consts::TAU;
        Some(Self::psx_camera_mvp(
            to_rad(PITCH_UNITS),
            to_rad(YAW_UNITS),
            FIELD_H,
            Vec3::new(0.0, 0.0, FIELD_CAM_DEPTH),
            target,
            aspect,
        ))
    }

    /// Battle camera: frame the **monster** actors (the ones carrying a bound
    /// mesh + idle animation) rather than the player vicinity. The live-loop
    /// seats battle actors around the world origin (`enter_battle(.., 600)` -
    /// party at `x=-600`, monsters at `x=+600`), far from the field player's
    /// world coords, so `camera_mvp`'s player-centred box leaves the enemies
    /// entirely off-screen. Framing the enemy cluster (gently orbiting) puts
    /// the animated monsters centre-frame and at a useful size.
    fn battle_camera_mvp(&self, aspect: f32) -> Mat4 {
        let world = &self.session.host.world;
        let pc = world.party_count as usize;
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        let mut any = false;
        for (i, a) in world.actors.iter().enumerate() {
            // Monster slots only (party occupies slots 0..party_count and isn't
            // mesh-bound in the play-window battle path anyway).
            if i < pc || a.tmd_binding.is_none() {
                continue;
            }
            let p = [
                a.move_state.world_x as f32,
                a.move_state.world_y as f32,
                a.move_state.world_z as f32,
            ];
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
            any = true;
        }
        if !any {
            // No bound monsters yet - fall back to the field framing.
            return self.camera_mvp(aspect);
        }
        // The bare position box collapses to a point/line; expand it to enclose
        // the monster mesh bodies (a few hundred units tall/wide).
        const M: f32 = 450.0;
        for k in 0..3 {
            lo[k] -= M;
            hi[k] += M;
        }
        // Gentle orbit (slower than the field's 0.25) so the animated enemies
        // read in 3D from several angles without spinning fast.
        orbit_camera_mvp(lo, hi, 0.12, 0.35, self.win.elapsed_secs(), aspect)
    }

    /// Battle orbit yaw in radians, at the **retail rate**. The battle tick
    /// (`FUN_801D0748`) decrements the camera yaw `_DAT_8007b792` by
    /// `DAT_1f800393 * 2` (≈2) per frame while idle: ≈ -4 units/frame, and a
    /// PSX turn is 4096 units, so the idle orbit is `4*60/4096` turn/s ≈ 0.059
    /// turn/s. Decreasing yaw = retail's spin sense.
    fn battle_orbit_yaw_rad(&self) -> f32 {
        const RETAIL_UNITS_PER_SEC: f32 = 4.0 * 60.0; // -4 u/frame at 60 fps
        -self.win.elapsed_secs() * RETAIL_UNITS_PER_SEC / 4096.0 * std::f32::consts::TAU
    }

    /// The **exact** retail overworld-battle camera (game mode `0x15`), pinned
    /// from the four fingerprinted `overworld_battle_bg_angle_*` saves and
    /// `FUN_80026988`/`FUN_80026f50`. For a PSX (Y-down) world vertex `v` retail
    /// computes `screen = H * (R*v + TR) / Ze` with
    ///   `R  = Rx(pitch=32u) * Ry(yaw)`         (12-bit angles, 4096 = 360°),
    ///   `TR = (0, 1280, 7680)`                 (eye-space: depth 7680, height 1280),
    ///   `H  = 256`                             (GTE projection focal length),
    /// the look-at target is the world origin, and PSX screen `+Y` is **down**
    /// with screen-centre `(160, 120)` over the 320x240 frame.
    ///
    /// The engine draws its meshes Y-flipped (`scale(1,-1,1)` = `F`, PSX Y-down
    /// -> renderer Y-up), so this builds `cam = Proj_H * T(TR) * R * F`: every
    /// battle draw is `cam * model` where `model` already carries an `F` (the
    /// dome's plain flip, the actors' `Translate * F`), and `F*F = I` recovers
    /// the raw PSX vertex the retail transform expects. Verified by projecting
    /// PROT 88's dome through this matrix and matching the savestate framebuffer
    /// (sky / mountain-ring / horizon). See `project_battle_camera_re`.
    /// The exact retail dome projection (`tr = (0,1280,7680)`), kept as the
    /// camera-RE reference and the regression-test target. The live battle uses
    /// the unified [`battle_dome_camera_mvp`] (closer depth) for a coherent
    /// single-camera scene; this stays as the pinned ground truth.
    #[allow(dead_code)]
    fn retail_battle_mvp(yaw_rad: f32, aspect: f32) -> Mat4 {
        Self::battle_mvp_with_tr(yaw_rad, Vec3::new(0.0, 1280.0, 7680.0), aspect)
    }

    /// The shared battle projection-times-view for a given eye-space translation
    /// `tr`. Retail keeps a single rotation `R = Rx(32u)·Ry(yaw)` (stored
    /// rotation-only in `DAT_8007bf10`) and applies the translation per draw
    /// class: the backdrop gets `tr = (0, 1280, 7680)` (pushed far), the actors
    /// get their own (closer) translation off the rotation-only matrix
    /// ([`FUN_80048A08`] composes each actor's world transform onto `8007bf10`,
    /// NOT onto the backdrop's `7680`-deep matrix). Sharing `R` keeps the
    /// foreground and backdrop orbiting in lock-step; the differing `tr.z`
    /// is what lets the party read large while the dome sits on the horizon.
    fn battle_mvp_with_tr(yaw_rad: f32, tr: Vec3, aspect: f32) -> Mat4 {
        const PITCH_UNITS: f32 = 32.0;
        let pitch = PITCH_UNITS / 4096.0 * std::f32::consts::TAU;
        Self::psx_camera_mvp(pitch, yaw_rad, 256.0, tr, Vec3::ZERO, aspect)
    }

    /// Shared PSX-projection camera: `screen = H * (R*(v - target) + tr) / Ze`
    /// with `R = Rx(pitch)·Ry(yaw)` (the retail GTE camera-rotation build
    /// `FUN_8001CF50`), `tr` the post-rotation eye-space translation, `target`
    /// the world-space look-at (retail folds it into the GTE translation as
    /// the negated focus trio `_DAT_80089118/1C/20`), and `H` the GTE
    /// projection register (`_DAT_8007B6F4`). The battle camera is this with
    /// `target = origin`, `H = 256`; the field camera drives `target` from
    /// the player anchor with the savestate-pinned angle globals.
    ///
    /// The engine draws its meshes Y-flipped (`scale(1,-1,1)` = `F`, PSX
    /// Y-down -> renderer Y-up); every draw's `model` carries an `F`, and
    /// `F*F = I` recovers the raw PSX vertex the retail transform expects.
    fn psx_camera_mvp(
        pitch_rad: f32,
        yaw_rad: f32,
        h: f32,
        tr: Vec3,
        target: Vec3,
        aspect: f32,
    ) -> Mat4 {
        let r = Mat4::from_rotation_x(pitch_rad) * Mat4::from_rotation_y(yaw_rad);
        let t = Mat4::from_translation(tr);
        let f = Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
        // PSX perspective onto a 320x240 frame: ndc.x = H*Ex/(160*Ez),
        // ndc.y = -H*Ey/(120*Ez) (PSX +Y down -> NDC up), clip.w = Ez, depth
        // mapped to wgpu [0,1]. Correct X for non-4:3 viewports so the 4:3
        // retail framing holds at any window size.
        let (near, far) = (4.0f32, 60000.0f32);
        let a = far / (far - near);
        let b = -near * far / (far - near);
        let aspect_fix = (4.0 / 3.0) / aspect.max(0.01);
        let proj = Mat4::from_cols(
            Vec4::new(h / 160.0 * aspect_fix, 0.0, 0.0, 0.0),
            Vec4::new(0.0, -h / 120.0, 0.0, 0.0),
            Vec4::new(0.0, 0.0, a, 1.0),
            Vec4::new(0.0, 0.0, b, 0.0),
        );
        proj * t * r * Mat4::from_translation(-target) * f
    }

    /// The single battle camera, used for **everything** in a stage-dome battle
    /// (dome, ground grid, and actors) so the scene reads as one coherent space
    /// rather than two overlapping layers. The retail dome projection wants
    /// `tr.z = 7680`, but that shoves the foreground actors onto the same far
    /// plane (tiny); driving a *separate* close camera for the actors instead
    /// split the horizon (the grass grid and the dome no longer met). A single
    /// middle depth keeps the dome's huge radius reading at the horizon while
    /// the party/enemies near the origin read at roughly retail scale. `tr.y`
    /// holds the dome's `1/6` down-shift ratio so the action sits just below
    /// centre. (The exact `tr.z = 7680` projection is preserved in
    /// [`retail_battle_mvp`] for the camera-RE reference + regression test.)
    fn battle_dome_camera_mvp(&self, aspect: f32) -> Mat4 {
        // Unified close depth so the SMALL battle meshes (party + monsters are
        // only ~130-370 units tall) read at a usable size while the dome's huge
        // radius still sits on the horizon. The probe-confirmed exact dome
        // projection is tr.z=7680, but at that true depth these small meshes are
        // only a few pixels (retail draws the actors off a separate close
        // matrix, not the backdrop's 7680 plane). Kept as the playable
        // single-camera compromise; the exact projection lives in
        // `retail_battle_mvp`.
        const DEPTH: f32 = 1500.0;
        Self::battle_mvp_with_tr(
            self.battle_orbit_yaw_rad(),
            Vec3::new(0.0, DEPTH / 6.0, DEPTH),
            aspect,
        )
    }

    /// Camera parameters for the cutscene shot, decoded from the cutscene
    /// timeline's executed op-`0x45` Camera Configure params (read from
    /// `World::camera_state`, committed by `FUN_801DE084`). Returns
    /// `(look_at, yaw_radians, fov_radians)`:
    ///
    /// - **look_at**: the camera focus. Retail stores the *negated* focus X / Z
    ///   in params 6 / 8 (`_DAT_80089118` / `_DAT_80089120` = the GTE
    ///   translation `-focus`; the follow-cam `FUN_801DBE9C` sets them to
    ///   `-(anchor X/Z)`), so X / Z are negated back to world space here; Y
    ///   (param 7) is stored un-negated. Any axis the cutscene hasn't staged
    ///   yet falls back to the lead actor (the cutscene anchor), then the
    ///   scene-AABB centre.
    /// - **yaw**: param 1 (`_DAT_8007b792`, camera yaw), PSX `4096` = full turn.
    /// - **fov**: derived from param 9 (`_DAT_8007b6f4`), which retail writes to
    ///   the GTE H projection register - the focal length. PSX projects onto a
    ///   ~240-tall frame, so the vertical FOV is `2*atan(120 / H)`. Inferred;
    ///   falls back to 60 deg when the param is absent or degenerate.
    fn cutscene_view(&self) -> ([f32; 3], f32, f32, f32) {
        use std::f32::consts::TAU;
        let world = &self.session.host.world;
        let params = &world.camera_state.params;
        let param = |slot: u8| {
            params
                .iter()
                .find(|p| p.slot == slot)
                .map(|p| p.value as i16 as f32)
        };
        let (px, py, pz) = world
            .actors
            .first()
            .filter(|a| a.active || a.tmd_binding.is_some())
            .map(|a| {
                (
                    a.move_state.world_x as f32,
                    a.move_state.world_y as f32,
                    a.move_state.world_z as f32,
                )
            })
            .unwrap_or_else(|| {
                (
                    (self.scene_aabb.0[0] + self.scene_aabb.1[0]) * 0.5,
                    (self.scene_aabb.0[1] + self.scene_aabb.1[1]) * 0.5,
                    (self.scene_aabb.0[2] + self.scene_aabb.1[2]) * 0.5,
                )
            });
        let look_at = [
            param(6).map(|v| -v).unwrap_or(px),
            param(7).unwrap_or(py),
            param(8).map(|v| -v).unwrap_or(pz),
        ];
        let yaw = param(1).map(|v| v / 4096.0 * TAU).unwrap_or(0.0);
        // Slot 0 = op-0x45 camera pitch (`_DAT_8007B790`, GTE RotMatrixX angle,
        // 12-bit / 4096 = 360 deg). Beats that omit it default to the prior
        // fixed ~24 deg downward framing so absent-pitch shots are unchanged.
        let pitch = param(0)
            .map(|v| v / 4096.0 * TAU)
            .unwrap_or_else(|| 0.45f32.atan());
        let fov = param(9)
            .filter(|&h| h > 1.0)
            .map(|h| 2.0 * (120.0 / h).atan())
            .unwrap_or(60f32.to_radians());
        (look_at, pitch, yaw, fov)
    }

    fn actor_model(&self, slot: usize) -> Mat4 {
        let a = &self.session.host.world.actors[slot];
        let pos = Vec3::new(
            a.move_state.world_x as f32,
            a.move_state.world_y as f32,
            a.move_state.world_z as f32,
        );
        // Field actors carry their heading in `render_26` (PSX 12-bit angle,
        // maintained by the locomotion controller); retail builds the actor
        // matrix from the rotation trio before the per-bone pose is composed
        // onto it (`FUN_8001B964` -> `FUN_80026988`). Y-rotation commutes with
        // the Y-flip, so the order against the scale is immaterial.
        let yaw = if self.session.host.world.mode == SceneMode::Field {
            (a.move_state.render_26 as f32) / 4096.0 * std::f32::consts::TAU
        } else {
            0.0
        };
        Mat4::from_translation(pos)
            * Mat4::from_rotation_y(yaw)
            * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))
    }

    /// The monster stat archive (PROT 867) bytes, decoded + cached on first
    /// use. `None` if no disc is attached or the entry can't be read.
    fn monster_archive_bytes(&mut self) -> Option<std::sync::Arc<Vec<u8>>> {
        if self.monster_archive.is_none() {
            const MONSTER_ARCHIVE_PROT_ENTRY: u32 = 867;
            match self
                .session
                .host
                .index
                .entry_bytes_extended(MONSTER_ARCHIVE_PROT_ENTRY)
            {
                Ok(b) => self.monster_archive = Some(std::sync::Arc::new(b)),
                Err(e) => {
                    log::warn!("play-window: monster archive (PROT 867) load skipped: {e:#}");
                    return None;
                }
            }
        }
        self.monster_archive.clone()
    }

    /// Load the Noa dance overlay (PROT 0980), decode its baked step chart, and
    /// start a dance run in the world (suspending the current scene). Returns
    /// `false` (and logs) when no disc is attached or the chart can't decode.
    ///
    /// Mirrors the disc-gated `dance_minigame_real` test's overlay path: read
    /// the raw PROT entry, lift it to its statically-recovered loaded form via
    /// [`static_overlay::as_loaded`], then parse through
    /// [`DanceGame::from_overlay`].
    fn start_dance_minigame(&mut self, long_song: bool) -> bool {
        use legaia_asset::static_overlay;
        let Some(rec) = static_overlay::overlay_map()
            .by_prot_index(legaia_asset::dance_chart::DANCE_OVERLAY_PROT_INDEX as u32)
        else {
            log::warn!("dance: overlay 0980 absent from the static-overlay map");
            return false;
        };
        let raw = match self.session.host.index.entry_bytes_extended(rec.prot_index) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("dance: PROT {} read failed: {e:#}", rec.prot_index);
                return false;
            }
        };
        let loaded = match static_overlay::as_loaded(&raw, rec) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("dance: as_loaded failed: {e:#}");
                return false;
            }
        };
        match legaia_engine_core::dance::DanceGame::from_overlay(&loaded, long_song) {
            Some(game) => {
                self.session.host.world.enter_dance(game);
                true
            }
            None => {
                log::warn!("dance: step-chart parse failed");
                false
            }
        }
    }

    /// Load the fishing overlay (PROT 0972), decode its per-species table, and
    /// start a fishing session in the world (suspending the current scene).
    /// Returns `false` (and logs) when no disc is attached or the table can't
    /// decode. Mirrors [`Self::start_dance_minigame`]'s overlay path.
    ///
    /// The rod stat + persistent record start at defaults (the save-block
    /// fishing record isn't loaded into this dev entry point).
    fn start_fishing_minigame(&mut self) -> bool {
        use legaia_asset::static_overlay;
        let Some(rec) = static_overlay::overlay_map()
            .by_prot_index(legaia_asset::fishing_species::FISHING_OVERLAY_PROT_INDEX as u32)
        else {
            log::warn!("fishing: overlay 0972 absent from the static-overlay map");
            return false;
        };
        let raw = match self.session.host.index.entry_bytes_extended(rec.prot_index) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("fishing: PROT {} read failed: {e:#}", rec.prot_index);
                return false;
            }
        };
        let loaded = match static_overlay::as_loaded(&raw, rec) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("fishing: as_loaded failed: {e:#}");
                return false;
            }
        };
        let Some(species) = legaia_asset::fishing_species::parse(&loaded) else {
            log::warn!("fishing: species-table parse failed");
            return false;
        };
        // Default rod stat + empty record for the dev entry point.
        const DEV_ROD_STAT: i32 = 4;
        let session = legaia_engine_core::fishing::FishingSession::new(
            species,
            DEV_ROD_STAT,
            legaia_engine_core::fishing::FishingRecord::default(),
        );
        self.session.host.world.enter_fishing(session);
        true
    }

    /// React to a `Field <-> Battle` scene-mode change once per transition:
    /// on entering battle, decode each enemy's mesh and inject it; on leaving,
    /// restore the clean field VRAM and drop the battle meshes. Called each
    /// frame before the render borrows `uploaded_vram`.
    fn sync_battle_render(&mut self) {
        let mode = self.session.host.world.mode;
        let prev = self.prev_scene_mode.replace(mode);
        if prev == Some(mode) {
            return;
        }
        match (prev, mode) {
            (_, SceneMode::Battle) => self.enter_battle_render(),
            (Some(SceneMode::Battle), _) => self.exit_battle_render(),
            _ => {}
        }
    }

    /// Bridge the decoded monster meshes for the current battle into the draw
    /// list: inject each enemy's texture pool into a clone of the field VRAM
    /// at the loader's per-slot coords, upload the relocated mesh, and bind it
    /// to the enemy actor. Re-uploads the edited VRAM so the injected texture
    /// pages resolve.
    /// Build the current scene's battle-stage backdrop, if it has one. Returns
    /// the battle VRAM (scene + stage-dome textures resident) and the stage
    /// dome's `(Tmd, raw)`. The faithful battle backdrop is the scene's
    /// `scene_tmd_stream` half-dome (sky + mountain ring + ground); building the
    /// scene in `SceneLoadKind::Battle` makes that dome TMD + its textures
    /// resident (the Field build excludes them). `None` when the scene has no
    /// stage entry.
    fn build_battle_stage(&self) -> Option<(legaia_tim::Vram, (legaia_tmd::Tmd, Vec<u8>))> {
        let scene = self.session.host.scene.as_ref()?;
        let scene_name = scene.name.clone();
        let stage_entry = *self
            .session
            .host
            .index
            .battle_stage_entries(&scene_name)
            .first()?;
        let mut shared: Vec<Scene> = Vec::new();
        for name in FIELD_SHARED_BLOCKS {
            if let Ok(s) = Scene::load(&self.session.host.index, name) {
                shared.push(s);
            }
        }
        let refs: Vec<&Scene> = shared.iter().collect();
        let (res, _) = SceneResources::build_targeted_with_options(
            scene,
            &refs,
            BuildOptions {
                kind: SceneLoadKind::Battle,
                upload_all_tims: true,
            },
        )
        .ok()?;
        // The stage dome is the leading TMD of the scene_tmd_stream stage entry.
        let dome = res.tmds.iter().find(|t| t.entry_idx == stage_entry)?;
        log::info!(
            "play-window: battle stage = scene '{scene_name}' PROT {stage_entry} \
             ({} objects)",
            dome.tmd.objects.len()
        );
        Some((res.vram.clone(), (dome.tmd.clone(), dome.raw.clone())))
    }

    fn enter_battle_render(&mut self) {
        let monsters = self.session.host.world.battle_monster_slots();
        if monsters.is_empty() {
            return;
        }
        let Some(field_base) = self.cpu_vram_base.clone() else {
            return;
        };
        let Some(archive) = self.monster_archive_bytes() else {
            return;
        };
        // Fresh battle: drop any previous battle's facial-animation state
        // (re-registered per assembled member below) and make sure the
        // static SCUS face-frame tables are loaded. Done before the
        // renderer borrow below (both take `&mut self`).
        self.battle_faces.clear();
        self.load_face_tables();
        // Build the battle-stage backdrop (the scene's scene_tmd_stream
        // half-dome). Its VRAM (scene + stage-dome textures resident) becomes
        // the battle base, so the dome renders textured behind the actors;
        // fall back to the field VRAM when the scene has no stage.
        let stage = self.build_battle_stage();
        let base = match &stage {
            Some((sv, _)) => sv.clone(),
            None => field_base,
        };
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };

        // Work on a throwaway copy so the field VRAM stays clean for the
        // restore on battle exit.
        let mut vram = base;
        // Blit the battle effect-texture atlas (PROT 870) into the battle VRAM
        // copy. Its pages land at fb_y=0 in the same columns the field stage
        // textures occupy, so this is a battle-only upload that battle exit
        // discards (the field VRAM base is untouched). Byte-verified against
        // live battle captures; soft-fails so a missing disc entry just leaves
        // the flame pages absent.
        if let Err(e) = legaia_engine_core::scene::upload_flame_atlas_into_vram(
            &self.session.host.index,
            &mut vram,
            true,
        ) {
            log::warn!("play-window: flame-atlas VRAM upload skipped: {e:#}");
        }
        self.battle_mesh_base = self.meshes.len();
        // Upload the stage dome mesh (drawn as the backdrop). Its textures live
        // in the stage VRAM, so build it unfiltered (all textured prims are
        // resident). Appended after `battle_mesh_base`, so battle exit truncates
        // it away with the monster meshes.
        self.battle_stage_mesh = None;
        if let Some((_, (tmd, raw))) = &stage {
            let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(tmd, raw);
            if !vmesh.indices.is_empty()
                && let Ok(m) = r.upload_vram_mesh(
                    &vmesh.positions,
                    &vmesh.uvs,
                    &vmesh.cba_tsb,
                    &vmesh.normals,
                    &vmesh.indices,
                )
            {
                self.battle_stage_mesh = Some(self.meshes.len());
                self.meshes.push(m);
                self.scene_tmd_data.push((tmd.clone(), raw.clone()));
                // Flat tiled ground grid under the actors (retail's
                // `func_0x801d02c0` grass grid). Reuse the dome's grass texel so
                // it samples real grass from the battle VRAM; drawn with the
                // actor camera so the party stands on it.
                self.battle_ground_mesh = None;
                if let Some(grid) = build_battle_ground_grid(&vmesh)
                    && let Ok(gm) = r.upload_vram_mesh(
                        &grid.positions,
                        &grid.uvs,
                        &grid.cba_tsb,
                        &grid.normals,
                        &grid.indices,
                    )
                {
                    self.battle_ground_mesh = Some(self.meshes.len());
                    self.meshes.push(gm);
                    self.scene_tmd_data.push((tmd.clone(), raw.clone())); // keep meshes/data aligned
                }
            }
        }
        let mut bound = 0usize;
        for (actor_idx, monster_id, slot) in monsters {
            let mesh = match legaia_asset::monster_archive::mesh(&archive, monster_id) {
                Ok(Some(m)) => m,
                Ok(None) => continue,
                Err(e) => {
                    log::warn!("play-window: monster {monster_id} mesh decode failed: {e:#}");
                    continue;
                }
            };
            // Parse the embedded TMD up front so it can be retained parallel
            // to `meshes`; `battle_render_mesh` only yields a mesh when this
            // same parse succeeds.
            let Ok(tmd) = legaia_tmd::parse(mesh.tmd_bytes()) else {
                continue;
            };
            let Some(vmesh) = mesh.battle_render_mesh(slot, &mut vram) else {
                continue;
            };
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
                Ok(m) => {
                    let idx = self.meshes.len();
                    self.meshes.push(m);
                    // Keep `scene_tmd_data` length-parallel with `meshes`.
                    self.scene_tmd_data.push((tmd, mesh.tmd_bytes().to_vec()));
                    self.session.host.world.actors[actor_idx].tmd_binding = Some(idx);
                    // Record the texture slot so the posed-animation rebuild can
                    // re-apply the per-slot CBA/TSB relocation (the raw-TMD posed
                    // mesh otherwise carries the nominal on-disc addresses and
                    // samples the wrong VRAM page → untextured/white).
                    self.session.host.world.actors[actor_idx].battle_tex_slot = Some(slot);
                    // Attach the monster's idle clip so its limbs move in
                    // battle. The clip's part count should equal the TMD object
                    // count (one rigid transform per object); MonsterAnimPlayer
                    // tolerates a mismatch (extra objects stay at rest).
                    match legaia_asset::monster_archive::idle_animation(&archive, monster_id) {
                        Ok(Some(idle)) => {
                            if let Some(player) =
                                legaia_engine_core::battle_anim::MonsterAnimPlayer::new(&idle)
                            {
                                self.session
                                    .host
                                    .world
                                    .set_actor_battle_animation(actor_idx, player);
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            log::warn!("play-window: monster {monster_id} idle anim decode: {e:#}")
                        }
                    }
                    // Install the full archive-order action-clip set so the
                    // hit-reaction family (action tags 2..5, the retail
                    // `+0x1EF` map) can play when this monster takes damage.
                    match legaia_asset::monster_archive::animations(&archive, monster_id) {
                        Ok(Some(anims)) if !anims.is_empty() => {
                            let clips: Vec<_> = anims.into_iter().map(Some).collect();
                            self.session.host.world.set_actor_battle_action_clips(
                                actor_idx,
                                std::sync::Arc::new(clips),
                            );
                        }
                        Ok(_) => {}
                        Err(e) => {
                            log::warn!("play-window: monster {monster_id} action clips: {e:#}")
                        }
                    }
                    bound += 1;
                }
                Err(e) => log::warn!("play-window: monster {monster_id} mesh upload: {e:#}"),
            }
        }

        // Load the REAL battle party meshes and bind them to the party actor
        // slots. The faithful source is the retail battle loader's per-character
        // ASSEMBLY: each member's mesh is spliced from their player battle file's
        // equipment-id sections (`legaia_asset::battle_char_assembly`, extraction
        // PROT 863..865) and relocated into the slot's runtime VRAM band
        // (`relocate_tsb_cba`, the registration-time TSB/CBA pass of
        // FUN_800513F0). The band's PIXELS come from the same player file: the
        // equipped sections' texture pools + the two record[0] image blocks,
        // uploaded at the pinned `FUN_80052FA0`/`FUN_80053B9C` placement
        // (`character_texture_uploads`; byte-exact vs live battle VRAM - see
        // docs/formats/battle-data-pack.md § Texture-pool VRAM placement).
        // PROT 1204 (the Baka Fighter / default-equipment sibling pack) stays
        // as the per-member fallback: its meshes when assembly fails, its
        // atlases (a 73-98% approximation of the band) when the texture-pool
        // decode fails. Each character's decoded battle palette overlays the
        // rows its mesh CBA samples (= 481 + slot after relocation).
        let mut party_bound = 0usize;
        let party_count = self.session.host.world.party_count as usize;
        if party_count > 0
            && let Ok(pack_raw) = self
                .session
                .host
                .index
                .entry_bytes_extended(legaia_asset::battle_char_pack::PROT_ENTRY_INDEX)
            && let Ok(pack) = legaia_asset::battle_char_pack::parse(&pack_raw)
        {
            // Upload the 7 character atlases (256x256 4bpp + their CLUT rows)
            // at their declared authoring rects (the Baka Fighter layout the
            // fallback meshes sample).
            for atlas in &pack.atlases {
                if let Ok(tim) = legaia_tim::parse(&atlas.tim_bytes) {
                    vram.upload_tim(&tim);
                }
            }
            // The battle party mesh is a set of object-local TMD pieces (head,
            // torso, limbs) - NOT pre-assembled. The retail battle sockets the
            // ASSEMBLED mesh with the character's own idle keyframe stream from
            // record[0] of the same player file (monster-format `[parts]
            // [frames][9-byte TRS]`, parts = skeleton bones; equipment extras
            // ride their attach bone - `assembled_party_battle_mesh` returns it
            // expanded per object). PROT 1203 is NOT that source: its banks are
            // authored against PROT 1204's own object order, which differs from
            // the assembled blob's sorted bone-tag order per character - posing
            // the assembled mesh from 1203 mis-sockets joints and splits the
            // duplicate equipment pieces apart. 1203 stays as the rest-pose
            // source for the 1204 FALLBACK mesh only (banks Vahn 0-8 /
            // Noa 9-17 / Gala 18-26, idle = each bank's first record; bone i =
            // 1204 object i).
            let battle_anm = self
                .session
                .host
                .index
                .entry_bytes_extended(1203)
                .ok()
                .and_then(|raw| {
                    [6usize, 3, 5, 7].iter().find_map(|&dc| {
                        legaia_asset::player_anm::find_in_entry(&raw, dc)
                            .into_iter()
                            .next()
                    })
                });
            // Per-member: assemble the battle mesh from the occupying
            // character's player file (fallback: the static PROT 1204 slot),
            // overlay its battle palette onto the CLUT rows the mesh samples,
            // upload, bind to the party actor. The retail rule (live-verified
            // for all four characters, incl. Terra): the CHARACTER picks the
            // content - player file + palette = extraction PROT `863 +
            // char_slot` (raw TOC 0x361-0x364; see docs/formats/cdname.md
            // numbering space) - while the present-party ORDINAL picks the
            // runtime texture band (`relocate_tsb_cba` x = 0x200 + i*0x80,
            // CLUT row 481 + i). `World::active_party` supplies the mapping;
            // empty = the identity Vahn/Noa/Gala default.
            for member in 0..party_count.min(3) {
                let cslot = self.session.host.world.party_roster_slot(member);
                // (tmd, raw bytes, per-object idle clip). The assembled path
                // poses from the player file's own idle stream (already
                // expanded so channel i drives object i, extras included);
                // the fallback 1204 mesh has no stream (`None`) and poses
                // from PROT 1203 with the identity object->bone mapping.
                let mut source: Option<(
                    legaia_tmd::Tmd,
                    Vec<u8>,
                    Option<legaia_asset::monster_archive::MonsterAnimation>,
                )> = None;
                let mut tex_uploads: Vec<legaia_asset::battle_char_assembly::TextureUpload> =
                    Vec::new();
                let mut action_clips: Option<
                    Vec<Option<legaia_asset::monster_archive::MonsterAnimation>>,
                > = None;
                let mut art_bank: Option<
                    Vec<Option<legaia_asset::monster_archive::MonsterAnimation>>,
                > = None;
                let mut face_tracks: Option<Vec<Option<legaia_asset::face_anim::FaceTracks>>> =
                    None;
                let mut art_face_tracks: Vec<Option<legaia_asset::face_anim::FaceTracks>> =
                    Vec::new();
                if let Some((asm, uploads, idle, clips, bank, faces, art_faces)) =
                    self.assembled_party_battle_mesh(cslot, member)
                {
                    tex_uploads = uploads;
                    action_clips = Some(clips);
                    art_bank = Some(bank);
                    face_tracks = Some(faces);
                    art_face_tracks = art_faces;
                    match legaia_tmd::parse(&asm.tmd) {
                        Ok(tmd) => source = Some((tmd, asm.tmd, Some(idle))),
                        Err(e) => log::warn!(
                            "play-window: party {cslot} assembled TMD parse: {e:#} \
                             (falling back to PROT 1204)"
                        ),
                    }
                }
                let assembled = source.is_some();
                if source.is_none()
                    && let Some(slot) = pack.slot(cslot)
                    && let Ok(tmd) = legaia_tmd::parse(&slot.tmd_bytes)
                {
                    source = Some((tmd, slot.tmd_bytes.clone(), None));
                }
                let Some((tmd, tmd_bytes, idle_anim)) = source else {
                    continue;
                };
                // Band pixels for the relocated mesh: the equipped sections'
                // texture pools + record[0] image blocks at the pinned
                // FUN_80052FA0 placement; the 1204 atlas pair approximates
                // the band only when the pool decode failed. Fallback 1204
                // meshes sample the authoring rects uploaded above instead.
                if assembled {
                    if tex_uploads.is_empty() {
                        // Approximation content = the CHARACTER's atlas pair;
                        // destination = the MEMBER ordinal's runtime band.
                        for half in 0..2usize {
                            if let Some(atlas) = pack.atlases.get(cslot * 2 + half)
                                && let Ok(tim) = legaia_tim::parse(&atlas.tim_bytes)
                            {
                                let img = &tim.image;
                                vram.write_block(
                                    512 + (member * 2 + half) as u16 * 64,
                                    256,
                                    img.fb_w,
                                    img.h,
                                    &img.data,
                                );
                            }
                        }
                    } else {
                        for u in &tex_uploads {
                            vram.write_block(u.fb_x(), u.fb_y(), u.rect.w, u.rect.h, &u.pixels);
                            if !u.clut.is_empty() {
                                vram.write_clut_row(u.clut_x, u.clut_row(), &u.clut_bytes());
                            }
                        }
                    }
                }
                // Rest pose: frame 0 of the assembled mesh's own idle stream
                // (the combat stance retail holds at battle start). Fallback
                // 1204 meshes pose from PROT 1203 instead - banks (per
                // docs/formats/character-mesh.md): records 0-8 = Vahn
                // (15-bone), 9-17 = Noa (16), 18-26 = Gala (15); the FIRST
                // record of each bank is that character's idle rest pose,
                // bone i driving 1204 object i.
                let bone_offsets: Vec<([i16; 3], [i16; 3])> = match (&idle_anim, &battle_anm) {
                    (Some(anim), _) => anim
                        .frames
                        .first()
                        .map(|f0| {
                            f0.iter()
                                .map(|p| {
                                    ([p.tx, p.ty, p.tz], [p.rx as i16, p.ry as i16, p.rz as i16])
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    (None, Some(b)) => {
                        // Idle record per 1203 bank. The banks cover the
                        // Vahn/Noa/Gala trio only - a Terra fallback mesh
                        // (no bank) renders unposed.
                        match [0usize, 9, 18].get(cslot) {
                            Some(&rec) => (0..tmd.objects.len())
                                .map(|o| match b.bone_transform(rec, 0, o) {
                                    Some(t) => (
                                        [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                                        [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                                    ),
                                    None => ([0; 3], [0; 3]),
                                })
                                .collect(),
                            None => Vec::new(),
                        }
                    }
                    (None, None) => Vec::new(),
                };
                let vmesh = if bone_offsets.is_empty() {
                    legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &tmd_bytes)
                } else {
                    legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(&tmd, &tmd_bytes, &bone_offsets)
                };
                if vmesh.indices.is_empty() {
                    continue;
                }
                // Rows the mesh CBA samples, and (for collect) the columns.
                let mut rows: Vec<u16> =
                    vmesh.cba_tsb.iter().map(|c| (c[0] >> 6) & 0x1FF).collect();
                rows.sort_unstable();
                rows.dedup();
                let mut cols: Vec<u16> = vmesh.cba_tsb.iter().map(|c| (c[0] & 0x3F) * 16).collect();
                cols.sort_unstable();
                cols.dedup();
                // Decode + overlay the battle palette from the character's own
                // player file. Vahn (863) = byte-exact parse_record; the other
                // characters (864..866, incl. Terra) = equipment-robust collect.
                let pal = match cslot {
                    0 => self
                        .session
                        .host
                        .index
                        .entry_bytes_extended(863)
                        .ok()
                        .and_then(|f| {
                            let rec0 = legaia_asset::battle_char_palette::find_record0(&f)?;
                            legaia_asset::battle_char_palette::parse_record(&f, rec0).ok()
                        }),
                    1..=3 => self
                        .session
                        .host
                        .index
                        .entry_bytes_extended(863 + cslot as u32)
                        .ok()
                        .and_then(|f| {
                            legaia_asset::battle_char_palette::collect_palette(&f, 0, &cols).ok()
                        }),
                    _ => None,
                };
                if let Some(pal) = pal {
                    // STP-set palette bands onto each row the mesh CBA samples.
                    for &row in &rows {
                        for band in &pal.bands {
                            let bytes: Vec<u8> = band
                                .vram_words()
                                .iter()
                                .flat_map(|w| w.to_le_bytes())
                                .collect();
                            vram.write_clut_row(band.base, row, &bytes);
                        }
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
                        let idx = self.meshes.len();
                        self.meshes.push(m);
                        self.scene_tmd_data.push((tmd, tmd_bytes));
                        self.session.host.world.actors[member].tmd_binding = Some(idx);
                        // Assembled meshes loop their own idle clip, exactly
                        // like monsters (the per-frame posed path rebuilds
                        // from the relocated TMD bytes, so texture
                        // addressing stays in the slot band).
                        if let Some(anim) = &idle_anim
                            && let Some(player) =
                                legaia_engine_core::battle_anim::MonsterAnimPlayer::new(anim)
                        {
                            self.session
                                .host
                                .world
                                .set_actor_battle_animation(member, player);
                        }
                        // Hand the SM pose hook the full action-clip set
                        // (record[0] slots + the equipment-spliced swings at
                        // 0xC..0xF) so ready / recover / defeat AND the
                        // staged attack-band swings play their real streams.
                        if let Some(clips) = action_clips.take() {
                            self.session
                                .host
                                .world
                                .set_actor_battle_action_clips(member, std::sync::Arc::new(clips));
                        }
                        // And the art bank, so staged ids >= 0x10 resolve
                        // through the ME-archive streams (FUN_8004AD80).
                        if let Some(bank) = art_bank.take().filter(|b| !b.is_empty()) {
                            self.session
                                .host
                                .world
                                .set_actor_battle_art_bank(member, std::sync::Arc::new(bank));
                        }
                        // Facial animation (FUN_8004C7B4): register the
                        // member's per-action face tracks so the per-tick
                        // stamp pass (`tick_battle_face_stamps`) re-stamps
                        // the current eye/mouth frame onto the band's live
                        // face rows. Only meaningful when the band holds
                        // the REAL texture-pool pixels (the face-frame
                        // strip the stamps copy from); the retail animator
                        // covers chars 0..2 (Terra is skipped) on bands
                        // 0..2.
                        if assembled
                            && !tex_uploads.is_empty()
                            && cslot < legaia_asset::face_anim::FACE_CHAR_COUNT
                            && member < legaia_asset::face_anim::FACE_SLOT_COUNT
                            && let Some(tracks) = face_tracks.take()
                        {
                            self.battle_faces.push(BattleMemberFace {
                                actor_slot: member,
                                char_index: cslot,
                                tracks,
                                art_tracks: std::mem::take(&mut art_face_tracks),
                                last_stamps: None,
                                art_counter: None,
                            });
                        }
                        party_bound += 1;
                    }
                    Err(e) => log::warn!("play-window: party {cslot} mesh upload: {e:#}"),
                }
            }
        }

        if bound > 0 || party_bound > 0 {
            match r.upload_vram(&vram) {
                Ok(v) => {
                    self.battle_vram_generation = Some(v.generation());
                    self.uploaded_vram = Some(v);
                }
                Err(e) => log::error!("play-window: battle VRAM re-upload: {e:#}"),
            }
            log::info!(
                "play-window: battle render bound {bound} monster + {party_bound} party mesh(es)"
            );
        }
        // Stash the battle VRAM + the monster-slot count so a mid-battle player
        // summon can inject its creature texture into the next free slot.
        self.battle_tex_slots_used = (bound as u8).min(4);
        self.battle_vram = Some(vram);
    }

    /// Assemble one party member's battle mesh the way the retail battle
    /// loader does: read the character's player battle file (extraction PROT
    /// `863 + cslot`, the `data\battle\PLAYER<n>` files), select its five
    /// equipment sections by the roster record's equipped item ids
    /// (`+0x196..+0x19A`), splice them into the merged TMD
    /// (`legaia_asset::battle_char_assembly`), then apply the
    /// registration-time TSB/CBA relocation into the present-party
    /// ORDINAL's runtime VRAM band (`relocate_tsb_cba` over `band`;
    /// texpages `512 + 128*band` / `+64` at `y = 256`, CLUT row
    /// `481 + band` - the live-verified retail rule: the band follows the
    /// party position, the content follows the character). Also decodes
    /// the matching band
    /// *pixels* - the equipped sections' texture pools + record[0] image
    /// blocks at the pinned placement (`character_texture_uploads`; empty
    /// with a warning when that decode fails, so the caller can fall back
    /// to the 1204 atlas approximation) - and the character's **idle battle
    /// animation** from record[0] of the same file
    /// (`idle_battle_animation`, the retail pose source for the assembled
    /// mesh), expanded per assembled object (`expand_animation_for_objects`
    /// over `anm_bones`, so channel `i` drives TMD object `i`, equipment
    /// extras riding their attach bone). `None` (with a warning) when the
    /// mesh assembly or the idle-stream decode fails - the caller falls
    /// back to the slot's static PROT 1204 mesh, whose rest pose
    /// (PROT 1203, identity object->bone) is known-correct.
    fn assembled_party_battle_mesh(&self, cslot: usize, band: usize) -> Option<AssembledPartyMesh> {
        let prot = 863 + cslot as u32;
        let raw = match self.session.host.index.entry_bytes_extended(prot) {
            Ok(raw) => raw,
            Err(e) => {
                log::warn!("play-window: party {cslot} player file (PROT {prot}): {e:#}");
                return None;
            }
        };
        let pack = match legaia_asset::battle_data_pack::parse(&raw) {
            Ok(pack) => pack,
            Err(e) => {
                log::warn!("play-window: party {cslot} player-file pack parse: {e:#}");
                return None;
            }
        };
        // Equipped item ids from the canonical roster record; an absent or
        // zeroed record assembles the all-default (unequipped) sections.
        let equipped: [u8; 5] = self
            .session
            .host
            .world
            .roster
            .members
            .get(cslot)
            .map(|rec| {
                let slots = rec.equipment().slots;
                [slots[0], slots[1], slots[2], slots[3], slots[4]]
            })
            .unwrap_or_default();
        let mut asm =
            match legaia_asset::battle_char_assembly::assemble_character(&raw, &pack, &equipped) {
                Ok(asm) => asm,
                Err(e) => {
                    log::warn!(
                        "play-window: party {cslot} battle-mesh assembly \
                         (equipped {equipped:02x?}): {e:#}"
                    );
                    return None;
                }
            };
        if let Err(e) =
            legaia_asset::battle_char_assembly::relocate_tsb_cba(&mut asm.tmd, band as u8)
        {
            log::warn!("play-window: party {cslot} TSB/CBA relocation: {e:#}");
            return None;
        }
        // The assembled mesh's pose source: the character's own idle stream
        // (record[0] action slot 0), expanded so channel i drives object i.
        let idle = match legaia_asset::battle_char_assembly::idle_battle_animation(&raw) {
            Ok(Some(anim)) => legaia_asset::battle_char_assembly::expand_animation_for_objects(
                &anim,
                &asm.anm_bones,
            ),
            Ok(None) => {
                log::warn!("play-window: party {cslot} player file carries no idle stream");
                return None;
            }
            Err(e) => {
                log::warn!("play-window: party {cslot} idle-stream decode: {e:#}");
                return None;
            }
        };
        // Band pixels at the pinned FUN_80052FA0 placement. A failure here
        // degrades to the 1204 atlas approximation, not to a mesh fallback.
        let uploads = legaia_asset::battle_char_assembly::character_texture_uploads(
            &raw, &pack, &equipped, band as u8,
        )
        .unwrap_or_else(|e| {
            log::warn!("play-window: party {cslot} texture-pool decode: {e:#}");
            Vec::new()
        });
        // Every populated action-stream slot, expanded the same way as the
        // idle: the battle-action SM's pose hook switches between these
        // (ready / recover / defeat one-shots over the idle loop).
        let mut clips: Vec<Option<legaia_asset::monster_archive::MonsterAnimation>> =
            vec![None; legaia_asset::battle_char_assembly::ACTION_SLOT_COUNT];
        // Per-action facial keyframe tracks (entry +0x8C eyes / +0x98
        // mouth), keyed like `clips` by the playing clip's action_id; the
        // per-frame facial animator (`tick_battle_face_stamps`) looks the
        // playing clip's tracks up here. Record[0] entries first, the
        // equipment-spliced swing entries' tracks land on slots 0xC..0xF
        // in the swing loop below.
        let mut faces: Vec<Option<legaia_asset::face_anim::FaceTracks>> =
            vec![None; legaia_asset::battle_char_assembly::ACTION_SLOT_COUNT];
        match legaia_asset::face_anim::battle_face_tracks(&raw) {
            Ok(t) => faces = t,
            Err(e) => log::warn!("play-window: party {cslot} face-track decode: {e:#}"),
        }
        match legaia_asset::battle_char_assembly::battle_animations(&raw) {
            Ok(anims) => {
                for a in &anims {
                    if let Some(slot) = clips.get_mut(a.action_id as usize) {
                        *slot = Some(
                            legaia_asset::battle_char_assembly::expand_animation_for_objects(
                                a,
                                &asm.anm_bones,
                            ),
                        );
                    }
                }
            }
            Err(e) => log::warn!("play-window: party {cslot} action-stream decode: {e:#}"),
        }
        // The four direction-command weapon swings spliced from the equipped
        // sections (runtime slots 0xC..0xF) - per-equipment animations the
        // attack chain stages as anim ids 0x0C..0x0F.
        match legaia_asset::battle_char_assembly::swing_battle_animations(&raw, &pack, &equipped) {
            Ok(swings) => {
                for s in &swings {
                    if let Some(slot) = clips.get_mut(s.slot as usize) {
                        *slot = Some(
                            legaia_asset::battle_char_assembly::expand_animation_for_objects(
                                &s.anim,
                                &asm.anm_bones,
                            ),
                        );
                    }
                    if let Some(face) = faces.get_mut(s.slot as usize) {
                        *face = s.face;
                    }
                }
            }
            Err(e) => log::warn!("play-window: party {cslot} swing decode: {e:#}"),
        }
        // The art-animation bank (record[0] +0x58): each record's keyframe
        // stream resolves through the character's readef.DAT "ME" archive
        // (main slot 3*char+1, base slot 3*char+2 for rate_alt == 0xFF
        // records). The staged-anim commit materializes bank record
        // `id - 0x10` into dynamic slot 0x10/0x11 (FUN_8004AD80).
        let (art_bank, art_faces) = self.party_art_bank(&raw, cslot, &asm.anm_bones);
        Some((asm, uploads, idle, clips, art_bank, faces, art_faces))
    }

    /// Decode one character's art-animation bank into commit-ready clips
    /// plus the records' embedded-entry face tracks (both indexed by bank
    /// record). Failures degrade per record (a `None` slot commits as a
    /// zero-length clip) or to an empty bank with a warning.
    fn party_art_bank(
        &self,
        raw: &[u8],
        cslot: usize,
        anm_bones: &[u8],
    ) -> (
        Vec<Option<legaia_asset::monster_archive::MonsterAnimation>>,
        Vec<Option<legaia_asset::face_anim::FaceTracks>>,
    ) {
        let record0 = match legaia_asset::battle_char_assembly::decode_record0(raw) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("play-window: party {cslot} record[0] decode for art bank: {e:#}");
                return (Vec::new(), Vec::new());
            }
        };
        let records = match legaia_asset::battle_char_assembly::art_animation_bank(&record0) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("play-window: party {cslot} art-bank parse: {e:#}");
                return (Vec::new(), Vec::new());
            }
        };
        // The embedded entries' face tracks (record +0xB0 / +0xBC) come
        // straight off the bank records - no ME archive involved.
        let faces: Vec<Option<legaia_asset::face_anim::FaceTracks>> =
            records.iter().map(|r| r.face).collect();
        // readef.DAT (extraction PROT 894) - the battle side-band file whose
        // 0x10800-byte slots carry the per-character "ME" stream archives.
        let readef = match self.session.host.index.entry_bytes_extended(894) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("play-window: readef.DAT (PROT 894) read: {e:#}");
                return (Vec::new(), faces);
            }
        };
        let main = legaia_asset::battle_char_assembly::art_me_archive(&readef, cslot, false);
        let base = legaia_asset::battle_char_assembly::art_me_archive(&readef, cslot, true);
        let mut bank = vec![None; records.len()];
        for rec in &records {
            let archive = if rec.uses_base_archive() {
                &base
            } else {
                &main
            };
            let Ok(archive) = archive else { continue };
            match legaia_asset::battle_char_assembly::art_animation(rec, archive) {
                Ok(anim) => {
                    bank[rec.index] = Some(
                        legaia_asset::battle_char_assembly::expand_animation_for_objects(
                            &anim, anm_bones,
                        ),
                    );
                }
                Err(e) => log::warn!(
                    "play-window: party {cslot} art record {} ({}): {e:#}",
                    rec.index,
                    rec.name
                ),
            }
        }
        (bank, faces)
    }

    /// Spawn the player Seru-magic summon as a battle creature, the faithful
    /// render (the summon reuses its namesake `battle_data` enemy creature -
    /// see `summon::summon_creature_id`). Loads that creature's mesh + texture
    /// (`battle_render_mesh`) into a free battle texture slot and binds it to a
    /// high actor slot with its idle [`MonsterAnimPlayer`], so the existing
    /// battle render animates + textures it exactly like an enemy. Replaces the
    /// move-VM `SummonScene` stand-in for the visual. No-op outside battle.
    fn spawn_summon_creature(&mut self, spell_id: u8) {
        if self.session.host.world.mode != SceneMode::Battle {
            return;
        }
        let Some(archive) = self.monster_archive_bytes() else {
            return;
        };
        let Some(creature) = legaia_engine_core::summon::summon_creature_id(spell_id, &archive)
        else {
            return;
        };
        let Some(mut vram) = self.battle_vram.clone() else {
            return;
        };
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };
        let mesh = match legaia_asset::monster_archive::mesh(&archive, creature) {
            Ok(Some(m)) => m,
            _ => return,
        };
        let Ok(tmd) = legaia_tmd::parse(mesh.tmd_bytes()) else {
            return;
        };
        // Inject the creature texture into the next free battle slot.
        let tex_slot = self.battle_tex_slots_used.min(4);
        let Some(vmesh) = mesh.battle_render_mesh(tex_slot, &mut vram) else {
            return;
        };
        if vmesh.indices.is_empty() {
            return;
        }
        let uploaded = match r.upload_vram_mesh(
            &vmesh.positions,
            &vmesh.uvs,
            &vmesh.cba_tsb,
            &vmesh.normals,
            &vmesh.indices,
        ) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("play-window: summon mesh upload: {e:#}");
                return;
            }
        };
        let idx = self.meshes.len();
        self.meshes.push(uploaded);
        self.scene_tmd_data.push((tmd, mesh.tmd_bytes().to_vec()));
        match r.upload_vram(&vram) {
            Ok(v) => {
                // A mid-battle summon spawn is a legitimate battle-VRAM
                // refresh: re-stamp the expected resident generation.
                self.battle_vram_generation = Some(v.generation());
                self.uploaded_vram = Some(v);
            }
            Err(e) => log::error!("play-window: summon VRAM re-upload: {e:#}"),
        }
        // Keep the stashed battle VRAM in sync with what's now resident, so
        // later battle-VRAM refreshes (the residency heal, the per-frame
        // face stamps) don't drop the injected summon texture.
        self.battle_vram = Some(vram);

        // Seat the summon in a free high actor slot (>= 8) so it never collides
        // with the party/monster battle slots. Place it on the party side
        // (`enter_battle` seats party at `x = -600`, enemies at `x = +600`), in
        // front of the party and clearly clear of the enemy cluster, so the
        // battle camera frames it distinct from the enemies it attacks.
        let slot = self
            .summon_actor_slot
            .unwrap_or_else(|| 8 + (self.session.host.world.party_count as usize));
        self.summon_actor_slot = Some(slot);
        if let Some(a) = self.session.host.world.actors.get_mut(slot) {
            a.active = true;
            a.tmd_binding = Some(idx);
            a.battle_tex_slot = Some(tex_slot);
            a.move_state.world_x = -350;
            a.move_state.world_y = 0;
            a.move_state.world_z = 0;
        }
        if let Ok(Some(idle)) = legaia_asset::monster_archive::idle_animation(&archive, creature)
            && let Some(player) = legaia_engine_core::battle_anim::MonsterAnimPlayer::new(&idle)
        {
            self.session
                .host
                .world
                .set_actor_battle_animation(slot, player);
        }
        log::info!(
            "play-window: summon spell {spell_id:#04x} -> battle_data creature {creature} \
             (mesh slot {idx}, tex slot {tex_slot}, actor slot {slot})"
        );
    }

    /// Leave battle: restore the clean field VRAM and drop the appended
    /// battle monster meshes (the field actor table was already restored from
    /// the pre-battle snapshot, so those slots no longer reference them).
    fn exit_battle_render(&mut self) {
        if let (Some(r), Some(base)) = (self.win.renderer.as_ref(), self.cpu_vram_base.as_ref()) {
            match r.upload_vram(base) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("play-window: field VRAM restore: {e:#}"),
            }
        }
        let keep = self.battle_mesh_base.min(self.meshes.len());
        self.meshes.truncate(keep);
        self.scene_tmd_data
            .truncate(keep.min(self.scene_tmd_data.len()));
        // Tear down a spawned player-summon creature.
        if let Some(slot) = self.summon_actor_slot.take()
            && let Some(a) = self.session.host.world.actors.get_mut(slot)
        {
            a.active = false;
            a.tmd_binding = None;
            a.battle_tex_slot = None;
            a.battle_animation = None;
            a.pose_frame = None;
        }
        self.battle_vram = None;
        self.battle_vram_generation = None;
        self.battle_tex_slots_used = 0;
        self.battle_stage_mesh = None;
        self.battle_ground_mesh = None;
        self.battle_faces.clear();
    }

    /// Lazily read the static face-frame tables out of the boot source's
    /// `SCUS_942.54` (`legaia_asset::face_anim::FaceFrameTables`). A failed
    /// attempt (disc-free run / unparsable executable) is remembered so the
    /// probe only runs once; facial animation is simply skipped then.
    fn load_face_tables(&mut self) {
        if self.face_tables_attempted {
            return;
        }
        self.face_tables_attempted = true;
        use legaia_engine_core::Vfs;
        let scus = if let Some(root) = self.extracted_root.as_deref() {
            legaia_engine_core::DirVfs::new(root)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else if let Some(disc) = self.disc_path.as_deref() {
            legaia_engine_core::DiscVfs::open(disc)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else {
            None
        };
        self.face_tables = scus
            .as_deref()
            .and_then(legaia_asset::face_anim::FaceFrameTables::from_scus);
        self.art_mouth_tables = scus
            .as_deref()
            .and_then(legaia_asset::face_anim::ArtMouthTables::from_scus);
        if self.face_tables.is_none() {
            log::info!(
                "play-window: SCUS face-frame tables unavailable - battle faces stay neutral"
            );
        }
    }

    /// Lazily read the spell/seru display-name table from the boot SCUS so the
    /// seru-trade overlay can label each offer. Cached after the first read;
    /// a disc-free run leaves it `None` (offers fall back to "Seru NN").
    fn ensure_seru_names(&mut self) {
        if self.seru_names.is_some() {
            return;
        }
        use legaia_engine_core::Vfs;
        let scus = if let Some(root) = self.extracted_root.as_deref() {
            legaia_engine_core::DirVfs::new(root)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else if let Some(disc) = self.disc_path.as_deref() {
            legaia_engine_core::DiscVfs::open(disc)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else {
            None
        };
        self.seru_names = scus
            .as_deref()
            .and_then(legaia_asset::spell_names::SpellNameTable::from_scus);
    }

    /// Render the seru-trade screens of the shop menu: the offer list
    /// (`ShopTrade`) or the yes/no confirm (`ShopTradeConfirm`). Each offer is
    /// labelled "give (owner) -> receive" with names from the boot SCUS.
    fn draw_shop_trade(&self, out: &mut Vec<TextDraw>, state: Option<MenuState>, cursor: usize) {
        let name_of = |id: u8| -> String {
            self.seru_names
                .as_ref()
                .and_then(|t| t.name(id))
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("Seru {id:02X}"))
        };
        let owner_of = |slot: u8| -> String {
            self.session
                .host
                .world
                .roster
                .members
                .get(slot as usize)
                .map(|m| m.name())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("P{slot}"))
        };
        match state {
            Some(MenuState::ShopTrade) => {
                let mut labels: Vec<String> = Vec::new();
                match self.menu_runtime.trade_session.as_ref() {
                    Some(t) if !t.offers.is_empty() => {
                        for o in &t.offers {
                            labels.push(format!(
                                "{} ({}) -> {}",
                                name_of(o.give.seru_id),
                                owner_of(o.give.owner_slot),
                                name_of(o.receive_seru_id),
                            ));
                        }
                    }
                    _ => labels.push("(no trades offered)".to_string()),
                }
                let rows: Vec<ShopRow<'_>> = labels
                    .iter()
                    .map(|l| ShopRow {
                        label: l.as_str(),
                        price: None,
                    })
                    .collect();
                out.extend(shop_draws_for(
                    &self.font,
                    "SHOP - TRADE SERU",
                    &rows,
                    cursor,
                    None,
                    (8, 140),
                ));
            }
            Some(MenuState::ShopTradeConfirm) => {
                let title = match self.menu_runtime.pending_trade_offer() {
                    Some(o) => format!(
                        "Trade {} for {}?",
                        name_of(o.give.seru_id),
                        name_of(o.receive_seru_id),
                    ),
                    None => "Trade?".to_string(),
                };
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
                out.extend(shop_draws_for(
                    &self.font,
                    &title,
                    &rows,
                    cursor,
                    None,
                    (8, 140),
                ));
            }
            _ => {}
        }
    }

    /// Per-tick battle facial animation: re-stamp each registered party
    /// member's current eye + mouth face frame onto the band's live face
    /// rows, exactly like the retail per-frame animator. The playing clip's
    /// `action_id` selects the member's face tracks, its integer keyframe
    /// cursor is the frame counter, and the selected frames are VRAM-to-VRAM
    /// copies from the band's face-frame strip (`Vram::move_image`, the
    /// `MoveImage` stamp). During the victory window - the battle ended in
    /// a monster wipe while a member still plays a dynamic-art-slot clip
    /// (staged id `0x11..=0x18`, e.g. the killing art) - the mouth records
    /// come from the static `0x80077E80` override table and the animator
    /// clocks on the halved victory counter instead (the win-quote mouth
    /// flap). The GPU texture is only re-uploaded on a frame
    /// whose stamp set differs from the previous one (retail re-issues
    /// identical `MoveImage`s every frame; the visible result is the same).
    // PORT: FUN_80047430 (facial-animator dispatch): per visible party
    // node, call the stamp pass with (band slot, char index, cursor
    // keyframes, playing action entry), skipping char 3 (Terra) and bands
    // >= 3. The stamp-selection half is `FaceFrameTables::stamps_with_art_window`
    // (PORT: FUN_8004C7B4 in legaia_asset::face_anim).
    fn tick_battle_face_stamps(&mut self) {
        use legaia_asset::face_anim::{ART_BAND_FIRST, ART_BAND_LAST, ArtMouthOverride};
        use legaia_engine_vm::battle_action::BattleEndCause;
        if self.session.host.world.mode != SceneMode::Battle || self.battle_faces.is_empty() {
            return;
        }
        let Some(tables) = self.face_tables.as_ref() else {
            return;
        };
        let art_tables = self.art_mouth_tables.as_ref();
        let Some(vram) = self.battle_vram.as_mut() else {
            return;
        };
        // Retail gate 1 of the victory-window mouth override: the
        // battle-end signal `DAT_8007BD71 == 0xFE` raised by a monster
        // wipe (the SM `0x5A` arm; the engine mirror is the
        // `BattleActionHost::battle_end` latch). Gates 2/3 - the victory
        // sequencer's phase halfword `ctx+0x6CE != 0` and the celebration
        // flag `DAT_8007BD60 & 0x80` (which the party-wipe path clears) -
        // are the retail victory presentation's internal progress flags;
        // the engine has no victory sequencer, so "the won battle is
        // still on screen" stands in for them. Escapes also raise 0xFE
        // but never set the celebration flag, so they stay excluded.
        let victory_window =
            self.session.host.world.battle_end == Some(BattleEndCause::MonsterWipe);
        let mut changed = false;
        for mf in &mut self.battle_faces {
            let Some(actor) = self.session.host.world.actors.get(mf.actor_slot) else {
                continue;
            };
            // No player yet = the rest pose: behaves like clip frame 0 of
            // the (track-less) idle, i.e. the neutral face.
            let (action_id, frame) = actor
                .battle_animation
                .as_ref()
                .map(|p| (p.action_id(), p.current_frame()))
                .unwrap_or((0, 0));
            // Track source. Staged ids >= 0x10 are art-bank clips: retail
            // materializes bank record `id - 0x10` and installs its
            // embedded entry (record +0x24) as the action-table pointer
            // (FUN_8004AD80), so the animator reads THAT entry's tracks
            // (record +0xB0 eyes / +0xBC mouth). Below the art base, the
            // playing clip's action slot picks the record[0] / swing entry.
            let tracks = if action_id >= legaia_asset::battle_char_assembly::ART_ANIM_ID_BASE {
                mf.art_tracks
                    .get(
                        (action_id - legaia_asset::battle_char_assembly::ART_ANIM_ID_BASE) as usize,
                    )
                    .and_then(|t| t.as_ref())
            } else {
                mf.tracks.get(action_id as usize).and_then(|t| t.as_ref())
            };
            // Retail gate 4: the member's last-staged anim id
            // (`actor[+0x1DB]`; the engine's art-bank clips carry it as
            // their `action_id`) sits in the dynamic-art-slot band
            // `0x11..=0x18`. Open the override window, clocking the
            // member's `gp+0x9EA` mirror from 0; closed, the counter
            // resets.
            let art_mouth = if victory_window
                && (ART_BAND_FIRST..=ART_BAND_LAST).contains(&action_id)
                && let Some(track) = art_tables.and_then(|t| t.track(mf.char_index, action_id))
            {
                let counter = mf.art_counter.unwrap_or(0);
                mf.art_counter = Some(counter.saturating_add(1));
                Some(ArtMouthOverride { track, counter })
            } else {
                mf.art_counter = None;
                None
            };
            // The retail mouth-neutral gate: character-record word `+0xF8`
            // bit 0x2000 = ability bitfield (`+0xF4`) bit 45 (passive 0x2D
            // Rage, Evil Medallion). Read it off the occupying character's
            // rebuilt ability bytes (byte 5 bit 0x20).
            let world = &self.session.host.world;
            let force_neutral_mouth = world
                .roster
                .members
                .get(world.party_roster_slot(mf.actor_slot))
                .map(|m| m.ability_bits()[5] & 0x20 != 0)
                .unwrap_or(false);
            let stamps = tables.stamps_with_art_window(
                mf.char_index,
                mf.actor_slot,
                tracks,
                frame,
                art_mouth,
                force_neutral_mouth,
            );
            if mf.last_stamps.as_deref() != Some(&stamps) {
                for s in &stamps {
                    vram.move_image(s.src_x, s.src_y, s.w, s.h, s.dst_x, s.dst_y);
                }
                mf.last_stamps = Some(stamps);
                changed = true;
            }
        }
        if !changed {
            return;
        }
        if let (Some(r), Some(vram)) = (self.win.renderer.as_ref(), self.battle_vram.as_ref()) {
            match r.upload_vram(vram) {
                Ok(v) => {
                    // A face re-stamp is a legitimate battle-VRAM refresh:
                    // move the expected resident generation along with it.
                    self.battle_vram_generation = Some(v.generation());
                    self.uploaded_vram = Some(v);
                }
                Err(e) => log::error!("play-window: face-stamp VRAM re-upload: {e:#}"),
            }
        }
    }

    /// Residency guard: while a battle texture is expected to be GPU-resident,
    /// verify no other path re-uploaded VRAM over it this frame. The
    /// white-speckle party bug (a background CLUT animator re-uploading the
    /// field snapshot mid-battle) was invisible to every CPU-side VRAM oracle
    /// because they never check *which* upload the draw samples. On a
    /// violation: log loudly, fail debug builds, and self-heal by re-uploading
    /// the stashed battle VRAM.
    fn check_battle_vram_residency(&mut self) {
        if self.session.host.world.mode != SceneMode::Battle {
            return;
        }
        let Some(expected) = self.battle_vram_generation else {
            return;
        };
        let current = self.uploaded_vram.as_ref().map(|v| v.generation());
        if current == Some(expected) {
            return;
        }
        log::error!(
            "play-window: battle VRAM clobbered mid-battle (expected upload \
             generation {expected}, GPU holds {current:?}); re-uploading the \
             battle texture"
        );
        debug_assert!(
            false,
            "battle VRAM residency violated: expected generation {expected}, found {current:?}"
        );
        if let (Some(r), Some(vram)) = (self.win.renderer.as_ref(), self.battle_vram.as_ref()) {
            match r.upload_vram(vram) {
                Ok(v) => {
                    self.battle_vram_generation = Some(v.generation());
                    self.uploaded_vram = Some(v);
                }
                Err(e) => log::error!("play-window: battle VRAM re-upload (heal): {e:#}"),
            }
        }
    }

    /// Build the per-strip [`legaia_engine_render::SpriteDraw`] list for
    /// the active publisher logo.
    ///
    /// PROKION and SCEA are stored as vertically-packed sprite atlases
    /// (see [`legaia_engine_core::publisher_logos::STRIPS_PER_LOGO`]);
    /// retail unfolds them by drawing the `N` strips side-by-side. We
    /// compute one [`SpriteDraw`] per strip, all sharing the session's
    /// current alpha, then integer-scale + centre the unfolded layout.
    /// Returns an empty vec when boot-UI isn't `PublisherLogos` or the
    /// atlas wasn't uploaded.
    fn publisher_logo_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        let BootUiState::PublisherLogos(session) = &self.boot_ui else {
            return Vec::new();
        };
        let Some(assets) = self.publisher_logos.as_ref() else {
            return Vec::new();
        };
        let idx = session.current_logo();
        if idx >= legaia_engine_core::publisher_logos::LOGO_COUNT {
            return Vec::new();
        }
        let (sx, sy, sw, sh) = assets.rects[idx];
        if sw == 0 || sh == 0 {
            return Vec::new();
        }
        let (cols, rows) = legaia_engine_core::publisher_logos::STRIP_GRID[idx];
        let cols = cols.max(1);
        let rows = rows.max(1);
        let strips_total = cols * rows;
        let strip_h_src = sh / strips_total;
        if strip_h_src == 0 {
            return Vec::new();
        }
        let unfolded_w = sw * cols;
        let unfolded_h = strip_h_src * rows;
        // Integer-multiple up-scale that fits inside the surface, capped
        // at 4× to keep logos crisp at typical 960×720. `max(1)` falls
        // back to native size (and accepts clipping) for layouts wider
        // than the surface.
        let scale_w = surface_w / unfolded_w.max(1);
        let scale_h = surface_h / unfolded_h.max(1);
        let scale = scale_w.min(scale_h).clamp(1, 4);
        let strip_w_dst = sw * scale;
        let strip_h_dst = strip_h_src * scale;
        let dst_w_total = unfolded_w * scale;
        let dst_h_total = unfolded_h * scale;
        let dst_x0 = (surface_w as i32 - dst_w_total as i32) / 2;
        let dst_y0 = (surface_h as i32 - dst_h_total as i32) / 2;
        let alpha = session.alpha().clamp(0.0, 1.0);
        let color = [1.0, 1.0, 1.0, alpha];
        // Source strips are stored column-major: source strip index
        // `s = c * rows + r` lands at output (col c, row r).
        let mut out = Vec::with_capacity(strips_total as usize);
        for r in 0..rows {
            for c in 0..cols {
                let s = c * rows + r;
                let src_y = sy + s * strip_h_src;
                let dst_x = dst_x0 + (c * strip_w_dst) as i32;
                let dst_y = dst_y0 + (r * strip_h_dst) as i32;
                out.push(legaia_engine_render::SpriteDraw {
                    dst: (dst_x, dst_y, strip_w_dst, strip_h_dst),
                    src: (sx, src_y, sw, strip_h_src),
                    color,
                });
            }
        }
        out
    }

    /// Canonical PSX-framebuffer (320×240) stage origin + scale, shared
    /// by every boot-UI element (title art, save-select chrome, slot
    /// pills, cursor, menu glyphs). Every retail-pinned position is
    /// expressed in 320×240 framebuffer pixels, so this is the single
    /// stage transform that maps them to screen coords. Using the same
    /// stage for the title art AND the save-select panel ensures
    /// relative positions remain correct at any window resolution.
    fn save_select_stage(&self, surface_w: u32, surface_h: u32) -> ((i32, i32), u32) {
        let stage_w = legaia_engine_render::BOOT_UI_STAGE_W;
        let stage_h = legaia_engine_render::BOOT_UI_STAGE_H;
        let scale = (surface_w / stage_w).min(surface_h / stage_h).clamp(1, 4);
        let sw = stage_w * scale;
        let sh = stage_h * scale;
        let x0 = (surface_w as i32 - sw as i32) / 2;
        let y0 = (surface_h as i32 - sh as i32) / 2;
        ((x0, y0), scale)
    }

    /// Build the [`legaia_engine_render::SpriteDraw`] list for the
    /// retail save-screen chrome (panel frame + slot pills). Anchored
    /// at the same 256×256 stage origin the title atlas uses so the
    /// chrome overlays the title art at retail-equivalent positions.
    ///
    /// Returns an empty vec when the save-menu atlas wasn't uploaded
    /// (e.g. running without a disc) or when the boot UI isn't in a
    /// SaveSelect / field-Save sub-state.
    fn save_select_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        let Some(assets) = self.save_menu.as_ref() else {
            return Vec::new();
        };
        use legaia_engine_core::save_select::{SaveSelectSession, SelectPhase};
        // The save-select session (or field-menu Save sub-session) that
        // drives both pill chrome and any retail Load-mode overlays.
        let session: &SaveSelectSession = match &self.boot_ui {
            BootUiState::SaveSelect(s) => s,
            BootUiState::FieldMenu {
                sub: Some(active), ..
            } => {
                use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
                if let FieldMenuSubsession::Save(s) = active {
                    s
                } else {
                    return Vec::new();
                }
            }
            _ => return Vec::new(),
        };
        let slot_count = session.slots().len().min(2);
        let cursor_row = (session.current_slot() as usize).min(1);
        // Retail draws every visible slot pill during Browsing and the
        // Confirm prompts, but hides the non-selected pills once a
        // slot has been confirmed for load (NowChecking + SlotPreview
        // both show only the picked pill). Build the pill slice
        // accordingly so the sprite chrome matches retail. AND retail
        // relocates that single visible pill up under the Load panel
        // (SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE) during Load-active.
        // The relocation is animated - mode 2 of FUN_801E1C1C slides
        // the slot composite linearly from screen `(136, 96)` (=
        // param_3=0xa0 with `sVar6 -= 0x18` x-shift, param_4=0x60) to
        // `(24, 40)` over 16 frames, driven by `DAT_801ef194`. We
        // interpolate against `session.slide_anim_t()` so the engine
        // matches retail's slide-in.
        let (pills, pill_anchor): (Vec<u8>, (i32, i32)) = match session.phase() {
            SelectPhase::NowChecking { slot, .. } | SelectPhase::SlotPreview { slot } => {
                // Slide start: retail mode-2 start `(160, 96)` minus
                // the `-0x18` x-shift baked into the inline emit
                // before the GPU command -> top-left `(136, 96)`.
                const SLIDE_START_TOPLEFT: (i32, i32) = (136, 96);
                let pos = session.interpolate(
                    SLIDE_START_TOPLEFT,
                    legaia_engine_render::SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE,
                );
                (vec![slot], pos)
            }
            _ => (
                (0..slot_count as u8).collect(),
                legaia_engine_render::SAVE_SELECT_SLOT1_POS,
            ),
        };
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        let mut draws = legaia_engine_render::save_select_chrome_draws_for(
            &assets.rects,
            &pills,
            pill_anchor,
            stage_origin,
            stage_scale,
        );
        // Pointing-finger cursor sprite - retail's small white hand
        // pointing at the selected slot pill, byte-pinned to CLUT row
        // 7 of the system-UI TIM. Emit last so it draws on top of
        // the pills. Suppress during NowChecking (dialog covers the
        // pill row) and SlotPreview (the grid emits its own cursor
        // on the focused cell).
        let emit_pill_cursor = !matches!(
            session.phase(),
            SelectPhase::NowChecking { .. } | SelectPhase::SlotPreview { .. }
        );
        if slot_count > 0 && emit_pill_cursor {
            draws.push(legaia_engine_render::save_select_cursor_draw_for(
                &assets.rects,
                cursor_row,
                stage_origin,
                stage_scale,
            ));
        }
        // Phase-specific overlays: SlotPreview shows the 5×3 grid + a
        // bottom info panel; NowChecking shows a centered dialog box
        // with the "Now checking. Do not remove MEMORY CARD" message.
        match session.phase() {
            SelectPhase::SlotPreview { slot } => {
                // Build per-cell views from the session's slot
                // snapshots. Each cell maps to one memory-card block;
                // up to 15 cells (5×3 grid).
                let cells: Vec<legaia_engine_render::SlotGridCell> = (0..15)
                    .map(|i| {
                        session
                            .slots()
                            .get(i)
                            .map(|s| legaia_engine_render::SlotGridCell {
                                present: s.present,
                                portrait_char_id: if s.present {
                                    Some(slot_leader_char_id(s))
                                } else {
                                    None
                                },
                            })
                            .unwrap_or_default()
                    })
                    .collect();
                draws.extend(legaia_engine_render::slot_preview_grid_draws_for(
                    &assets.rects,
                    &cells,
                    slot,
                    stage_origin,
                    stage_scale,
                ));
                let info = build_slot_info_view(session.slots(), slot);
                let view = info.as_ref().map(|i| i.as_view());
                let panel_y_offset = info_panel_slide_offset(session);
                draws.extend(legaia_engine_render::slot_info_panel_draws_for(
                    &assets.rects,
                    view.as_ref(),
                    panel_y_offset,
                    stage_origin,
                    stage_scale,
                ));
            }
            SelectPhase::NowChecking { .. } => {
                // Slide the panel left-from-right alongside the text,
                // matching retail mode-0's `pos = (416, 112) -> (160,
                // 112)` interpolation.
                let pos_x = legaia_engine_core::save_select::interpolate_anim(
                    (legaia_engine_render::NOW_CHECKING_SLIDE_START_X, 0),
                    (legaia_engine_render::NOW_CHECKING_SLIDE_TARGET_X, 0),
                    session.slide_anim_t(),
                )
                .0;
                let slide_offset = (pos_x - legaia_engine_render::NOW_CHECKING_SLIDE_TARGET_X, 0);
                draws.extend(legaia_engine_render::now_checking_panel_draws_for(
                    &assets.rects,
                    stage_origin,
                    stage_scale,
                    slide_offset,
                ));
            }
            _ => {}
        }
        draws
    }

    /// Build the [`legaia_engine_render::SpriteDraw`] list for the
    /// title-screen quad. Composes the retail title screen by drawing
    /// per-band sub-rects of the PROT 0888 title TIM: orb + wordmark
    /// always, "PRESS START BUTTON" only during the PressStart phase,
    /// and the two copyright lines in every post-fade phase. The
    /// `<DEMO>` band and the small "NEW GAME CONTINUE" footer band are
    /// intentionally skipped - the former is a demo-build leftover
    /// retail never draws, the latter is replaced by larger
    /// font-rendered menu labels (see [`Self::boot_ui_draws`]).
    ///
    /// Each band is positioned at its source `y` within a centred,
    /// integer-scaled 256×256 stage. Returns an empty vec when
    /// boot-UI isn't `Title`, the atlas wasn't uploaded, or the title
    /// session has reached [`legaia_engine_core::title::TitlePhase::Done`].
    fn title_screen_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        // Active during both the Title phases and the SaveSelect boot
        // sub-state. SaveSelect dims the bands to ~45 % brightness so
        // the panel + slot pills layered on top read clearly. Retail
        // pivots to pure black once a slot is confirmed (NowChecking /
        // SlotPreview): the dialog + portrait grid + info panel are
        // composed against black, never the title art.
        let title_session: Option<&legaia_engine_core::title::TitleSession> = match &self.boot_ui {
            BootUiState::Title(s) => Some(s),
            BootUiState::SaveSelect(s) => {
                use legaia_engine_core::save_select::SelectPhase;
                if matches!(
                    s.phase(),
                    SelectPhase::NowChecking { .. } | SelectPhase::SlotPreview { .. }
                ) {
                    return Vec::new();
                }
                None
            }
            _ => return Vec::new(),
        };
        let (alpha, dim) = if let Some(session) = title_session {
            if matches!(
                session.phase(),
                legaia_engine_core::title::TitlePhase::Done(_)
            ) {
                return Vec::new();
            }
            let alpha = match session.phase() {
                legaia_engine_core::title::TitlePhase::FadeIn { frames_remaining } => {
                    let total = session.fade_in_frames.max(1) as f32;
                    1.0 - (frames_remaining as f32 / total).clamp(0.0, 1.0)
                }
                _ => 1.0,
            };
            (alpha, false)
        } else {
            (1.0, true)
        };
        let Some(assets) = self.title_screen.as_ref() else {
            return Vec::new();
        };
        let (_atlas_x, _atlas_y, atlas_w, atlas_h) = assets.rect;
        if atlas_w == 0 || atlas_h == 0 {
            return Vec::new();
        }
        // Share the canonical PSX framebuffer (320×240) stage with
        // every other boot-UI element so the title art aligns with
        // the save-select panel, slot pills, and cursor - all of
        // which use retail-pinned framebuffer coords. The title TIM's
        // bands are sampled at their natural src (sx, sy) but drawn
        // at dst (TITLE_ART_POS + sx, TITLE_ART_POS + sy), i.e.
        // offset by retail's title-quad top-left placement.
        let ((stage_x0, stage_y0), scale) = self.save_select_stage(surface_w, surface_h);
        let lum = if dim { 0.45 } else { 1.0 };
        let color = [lum, lum, lum, alpha];
        let emit_press_start = matches!(
            &self.boot_ui,
            BootUiState::Title(s)
                if matches!(s.phase(), legaia_engine_core::title::TitlePhase::PressStart { .. })
        );
        use legaia_asset::title_pak;
        // Each entry: (src_rect, dst_x_src, dst_y_src, tint). Most
        // bands draw at their own (src_x, src_y); the menu rows are
        // sampled from a packed single-row band and re-positioned so
        // "NEW GAME" sits at src_y=143 and "CONTINUE" at src_y=159
        // (matching the retail stacked layout, which puts these
        // ~14 px apart between the wordmark and the copyright lines).
        let scale_i32 = scale as i32;
        let mut out: Vec<legaia_engine_render::SpriteDraw> = Vec::new();
        // `dst_src_x/y` are coords inside the title TIM's source rect
        // (0..256, 0..256). We offset by TITLE_ART_POS so the result
        // lands at retail's framebuffer position.
        let title_pos_x = legaia_engine_render::TITLE_ART_POS.0;
        let title_pos_y = legaia_engine_render::TITLE_ART_POS.1;
        let push_band = |out: &mut Vec<legaia_engine_render::SpriteDraw>,
                         src: (u32, u32, u32, u32),
                         dst_src_x: i32,
                         dst_src_y: i32,
                         tint: [f32; 4]| {
            let (sx, sy, sw, sh) = src;
            out.push(legaia_engine_render::SpriteDraw {
                dst: (
                    stage_x0 + (title_pos_x + dst_src_x) * scale_i32,
                    stage_y0 + (title_pos_y + dst_src_y) * scale_i32,
                    sw * scale,
                    sh * scale,
                ),
                src: (sx, sy, sw, sh),
                color: tint,
            });
        };

        // Wordmark always.
        let wm = title_pak::TITLE_BAND_WORDMARK;
        push_band(&mut out, wm, wm.0 as i32, wm.1 as i32, color);

        // PressStart prompt only during that phase.
        if emit_press_start {
            let ps = title_pak::TITLE_BAND_PRESS_START;
            push_band(&mut out, ps, ps.0 as i32, ps.1 as i32, color);
        }

        // Main-menu rows (NEW GAME / CONTINUE) - drawn during MainMenu
        // (selected row bright, unselected dim) and also during
        // SaveSelect (both dim - they sit in the background behind the
        // slot pills and don't reflect a live cursor).
        let menu_state: Option<(u8, bool)> = match &self.boot_ui {
            BootUiState::Title(s) => match s.phase() {
                legaia_engine_core::title::TitlePhase::MainMenu { cursor } => Some((cursor, true)),
                _ => None,
            },
            BootUiState::SaveSelect(_) => Some((1, false)),
            _ => None,
        };
        if let Some((cursor, has_focus)) = menu_state {
            let row_white = color;
            let row_dim = [color[0] * 0.5, color[1] * 0.5, color[2] * 0.5, color[3]];
            let ng = title_pak::TITLE_BAND_MENU_NEW_GAME;
            let co = title_pak::TITLE_BAND_MENU_CONTINUE;
            // Center inside the title-art width so the rows sit on
            // the screen's horizontal center (fb_x=160) after the
            // TITLE_ART_POS.x=33 offset is applied by push_band.
            let title_art_w = legaia_engine_render::TITLE_ART_SIZE.0 as u32;
            let ng_x = ((title_art_w - ng.2) / 2) as i32;
            let co_x = ((title_art_w - co.2) / 2) as i32;
            // Sit the menu between wordmark (ends y~141) and copyrights (start y~195).
            let ng_y: i32 = 154;
            let co_y: i32 = ng_y + ng.3 as i32 + 4;
            let ng_tint = if has_focus && cursor == 0 {
                row_white
            } else {
                row_dim
            };
            let co_tint = if has_focus && cursor == 1 {
                row_white
            } else {
                row_dim
            };
            push_band(&mut out, ng, ng_x, ng_y, ng_tint);
            push_band(&mut out, co, co_x, co_y, co_tint);
        }

        // Copyright lines always (post-fade).
        let tm = title_pak::TITLE_BAND_TM_COPYRIGHT;
        push_band(&mut out, tm, tm.0 as i32, tm.1 as i32, color);
        let cc = title_pak::TITLE_BAND_C_COPYRIGHT;
        push_band(&mut out, cc, cc.0 as i32, cc.1 as i32, color);
        out
    }

    /// **Deprecated path** kept as a no-disc fallback. The retail title
    /// menu now renders via `title_screen_sprite_draws` sampling the
    /// dedicated NEW GAME / CONTINUE sub-rects from the title TIM
    /// (PROT 0888 @ y=227..237). When the title atlas is present this
    /// method returns an empty vec so the title-TIM path is the
    /// single source of menu glyphs.
    ///
    /// Returns an empty vec when:
    /// - boot UI isn't [`BootUiState::Title`], or
    /// - the title session has already reached
    ///   [`legaia_engine_core::title::TitlePhase::Done`], or
    /// - the title-screen atlas IS uploaded (retail-faithful path
    ///   covers the menu rows itself), or
    /// - the menu-glyph atlas wasn't uploaded, or
    /// - the title phase isn't `MainMenu`.
    fn title_menu_glyph_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        let BootUiState::Title(session) = &self.boot_ui else {
            return Vec::new();
        };
        if self.menu_glyphs.is_none() {
            return Vec::new();
        }
        // When the title-screen atlas is loaded, the retail-faithful
        // path inside `title_screen_sprite_draws` already emits the
        // NEW GAME / CONTINUE rows from the title TIM itself - skip
        // the debug-atlas fallback to avoid double-rendering.
        if self.title_screen.is_some() {
            return Vec::new();
        }
        use legaia_engine_core::title::TitlePhase;
        let (phase_id, cursor) = match session.phase() {
            TitlePhase::MainMenu { cursor } => (2u8, cursor),
            _ => return Vec::new(),
        };
        // Anchor inside the same centred + integer-scaled 256×256
        // title stage that `title_screen_sprite_draws` uses. The menu
        // rows sit between the wordmark band (ends at src y=140) and
        // the copyright bands (start at src y=195) - the menu-glyph
        // cell is 14 px tall at 1× and we render at 2× the title-art
        // scale for retail-faithful sizing (~28 px atlas-pixels per
        // row, two rows + gutter = ~60 px in source).
        let atlas_w: u32 = 256;
        let atlas_h: u32 = 256;
        let title_scale = (surface_w / atlas_w.max(1))
            .min(surface_h / atlas_h.max(1))
            .clamp(1, 4);
        let title_scale_i32 = title_scale as i32;
        let stage_x0 = (surface_w as i32 - (atlas_w as i32) * title_scale_i32) / 2;
        let stage_y0 = (surface_h as i32 - (atlas_h as i32) * title_scale_i32) / 2;
        // Render menu glyphs at 2× the title-art scale so the letters
        // match the retail proportion (~28 px tall in framebuffer
        // pixels at 1×). "NEW GAME" is 8 cells × 8 px × 2 = 128 px at
        // 1× glyph_scale, then × title_scale for the on-screen size.
        let glyph_scale = title_scale;
        let menu_w_src = 8 * 8; // 8 chars × 8 px (1× glyph multiplier)
        // Centre horizontally inside the 256-wide title stage.
        let pen_src_x = (atlas_w as i32 - menu_w_src) / 2;
        let pen_src_y = 152;
        let pen = (
            stage_x0 + pen_src_x * title_scale_i32,
            stage_y0 + pen_src_y * title_scale_i32,
        );
        legaia_engine_render::title_menu_draws_for(
            phase_id,
            cursor,
            session.continue_enabled,
            pen,
            glyph_scale,
        )
    }

    /// Keep the rendered dialog panel ([`Self::active_dialog`]) in sync with
    /// the world's pending dialog request.
    ///
    /// The world owns dismissal: the field VM's op-`0x4C` dialog-advance hook
    /// and the overworld talk-to handler both clear `World::current_dialog` on
    /// a confirm/cancel press. This method only mirrors that state into a
    /// visible, typed-out box - it opens a panel from the scene's MES the frame
    /// a request appears, ticks its typewriter reveal, and drops the panel the
    /// frame the world clears the request. It never clears `current_dialog`
    /// itself, so it can't race the world's dismiss.
    fn sync_dialog_panel(&mut self) {
        // When the inline-script field-VM runner owns dialogue, it manages its
        // own box (rendered from `world.inline_dialogue`); don't also open the
        // simplified panel.
        if self.session.host.world.use_vm_dialogue {
            self.active_dialog = None;
            return;
        }
        if self.session.host.world.current_dialog.is_none() {
            self.active_dialog = None;
            return;
        }
        if self.active_dialog.is_none()
            && let Some(mut panel) = self.session.host.open_pending_dialog()
        {
            panel.set_glyphs_per_frame(2);
            self.active_dialog = Some(panel);
        }
        if let Some(panel) = self.active_dialog.as_mut() {
            panel.tick();
        }
    }

    fn build_hud(&self, w: u32, h: u32) -> Vec<TextDraw> {
        let Some(atlas) = &self.font_atlas else {
            return Vec::new();
        };
        let _ = atlas;
        // Boot UI is fullscreen - when active, suppress every other HUD layer
        // and just render the active panel (title screen / save-select).
        if self.boot_ui.is_active() {
            return self.boot_ui_draws(w, h);
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
        if let Some(ctrl) = &self.session.host.world.world_map_ctrl {
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
        // Dance minigame HUD: the running score / groove gauge / active lane,
        // the arrow the current beat calls for, and the last press judgement.
        // The three arrows map to the retail pad bits (Left/Right/Up).
        if self.session.host.world.mode == SceneMode::Dance
            && let Some(g) = &self.session.host.world.dance
        {
            let arrow = match g.required_symbol() {
                Some(1) => "< (Left)",
                Some(2) => "> (Right)",
                Some(3) => "^ (Up)",
                _ => "- (rest)",
            };
            use legaia_engine_core::dance::Judge;
            let judge = match self.session.host.world.dance_last_judge {
                Some(Judge::Sequence { .. }) => "SEQUENCE!",
                Some(Judge::Hit { .. }) => "HIT",
                Some(Judge::Miss) => "miss",
                None => "",
            };
            let dl1 = format!(
                "DANCE  score {}  gauge {}  lane {}",
                g.score(),
                g.gauge(),
                g.lane()
            );
            let ly1 = self.font.layout_ascii(&dl1);
            out.extend(text_draws_for(&ly1, (8, 62), white));
            let dl2 = format!("press {arrow}   {judge}   (K = quit)");
            let ly2 = self.font.layout_ascii(&dl2);
            out.extend(text_draws_for(&ly2, (8, 80), dim));
        }
        // Fishing minigame HUD: the phase-specific line (cast-power bar while
        // casting; tension + strength while fighting; the catch result when
        // done) plus the running point total.
        if self.session.host.world.mode == SceneMode::Fishing
            && let Some(s) = &self.session.host.world.fishing
        {
            use legaia_engine_core::fishing::{FightOutcome, FishingPhase};
            let line = match s.phase() {
                FishingPhase::Casting => {
                    format!("FISHING  cast power {}  (Cross = cast)", s.cast_power())
                }
                FishingPhase::Fighting => {
                    let (tension, strength) = s
                        .fight()
                        .map(|f| (f.tension(), f.strength()))
                        .unwrap_or((0, 0));
                    format!(
                        "FISHING  tension {tension}/{}  strength {strength}  (hold Cross/Circle to reel)",
                        legaia_engine_core::fishing::TENSION_MAX
                    )
                }
                FishingPhase::Done => match s.last_outcome() {
                    Some(FightOutcome::Landed { points }) => {
                        format!("FISHING  landed! +{points} points  (Cross = recast)")
                    }
                    Some(FightOutcome::Snapped) => {
                        "FISHING  the line snapped!  (Cross = recast)".to_string()
                    }
                    _ => "FISHING  (Cross = recast)".to_string(),
                },
            };
            let ly = self.font.layout_ascii(&line);
            out.extend(text_draws_for(&ly, (8, 62), white));
            let pts = format!(
                "points {}   best {}   (L = quit)",
                s.record().points,
                s.record().best_points
            );
            let ly2 = self.font.layout_ascii(&pts);
            out.extend(text_draws_for(&ly2, (8, 80), dim));
        }
        // Shop / inn overlay: rendered at the bottom of the screen when the menu
        // runtime is in any shop, inn, or confirmation state.
        if self.menu_runtime.is_open() {
            let label = self.menu_runtime.current_label();
            if let Some(shop) = &self.menu_runtime.shop_session {
                let state = MenuState::from_byte(self.menu_runtime.ctx_state());
                let cursor = self.menu_runtime.cursor() as usize;
                let gold = self.session.host.world.money;
                // The seru-trade screens carry dynamic, owned-string labels, so
                // render them directly (the generic `(title, rows)` path below
                // only handles `'static` labels).
                let trade_state = matches!(
                    state,
                    Some(MenuState::ShopTrade) | Some(MenuState::ShopTradeConfirm)
                );
                if trade_state {
                    self.draw_shop_trade(&mut out, state, cursor);
                }
                let (title, rows, show_gold) = match state {
                    _ if trade_state => (label, Vec::new(), None),
                    // Top picker: Buy / Sell / (Trade) / Exit, matching the
                    // runtime's dynamic row layout.
                    Some(MenuState::ShopMenu) => {
                        let rows: Vec<ShopRow<'_>> =
                            legaia_engine_core::menu_runtime::shop_menu_rows(
                                self.session.host.world.seru_trade_enabled(),
                            )
                            .iter()
                            .map(|s| ShopRow {
                                label: match s {
                                    MenuState::ShopBuy => "Buy",
                                    MenuState::ShopSell => "Sell",
                                    MenuState::ShopTrade => "Trade Seru",
                                    _ => "Exit",
                                },
                                price: None,
                            })
                            .collect();
                        (label, rows, Some(gold))
                    }
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
        // Battle HUD: party + monster HP plus, when the battle is
        // player-driven, the live command menu / target cursor. Only drawn in
        // SceneMode::Battle; harmless when the live loop is off (it just never
        // enters battle).
        if self.session.host.world.mode == SceneMode::Battle {
            use legaia_engine_core::battle_input::{BattleCommand, CommandPhase};
            use legaia_engine_core::target_picker::{CursorRow, PickerState};
            let bw = &self.session.host.world;
            let pc = (bw.party_count.clamp(1, 3) as usize).min(bw.actors.len());
            let down_color = [0.6f32, 0.6, 0.6, 1.0];
            let enemy_color = [1.0f32, 0.7, 0.6, 1.0];

            // Per-actor-index row Y, recorded as rows are drawn so popups +
            // status icons anchor to the right slot even though the monster
            // loop skips empty slots.
            let mut row_y: [Option<i32>; 8] = [None; 8];
            let mut y = 60i32;
            // Row label = the occupying character's roster name (the
            // present-party composition maps battle ordinal -> character);
            // "P<n>" when the roster has no record for the slot.
            let party_names = legaia_engine_core::field_menu_dispatch::roster_names(bw);
            for (i, a) in bw.actors.iter().take(pc).enumerate() {
                let name = party_names
                    .get(bw.party_roster_slot(i))
                    .filter(|n| !n.is_empty())
                    .map(|n| format!("{n:<8}"))
                    .unwrap_or_else(|| format!("P{:<7}", i + 1));
                let line = format!("{name}HP {:>4}/{:<4}", a.battle.hp, a.battle.max_hp);
                let color = if a.battle.liveness != 0 {
                    white
                } else {
                    down_color
                };
                out.extend(text_draws_for(
                    &self.font.layout_ascii(&line),
                    (8, y),
                    color,
                ));
                if i < row_y.len() {
                    row_y[i] = Some(y);
                }
                y += 16;
            }
            y += 8;
            for (mi, a) in bw.actors.iter().skip(pc).enumerate() {
                if a.battle.max_hp == 0 {
                    continue;
                }
                let alive = a.battle.liveness != 0;
                let line = format!(
                    "M{}  HP {:>4}/{:<4}{}",
                    mi + 1,
                    a.battle.hp,
                    a.battle.max_hp,
                    if alive { "" } else { "  DOWN" }
                );
                let color = if alive { enemy_color } else { down_color };
                out.extend(text_draws_for(
                    &self.font.layout_ascii(&line),
                    (8, y),
                    color,
                ));
                let actor_idx = pc + mi;
                if actor_idx < row_y.len() {
                    row_y[actor_idx] = Some(y);
                }
                y += 16;
            }

            // Status-effect icon strip per slot (single-letter abbreviations
            // from the live tracker), drawn to the right of the HP row.
            let status_color = [1.0f32, 0.95, 0.4, 1.0];
            for (slot, anchor) in row_y.iter().enumerate() {
                let Some(ry) = anchor else { continue };
                let letters = self.battle_hud.slots[slot].status_letters();
                for (k, letter) in letters.iter().enumerate() {
                    let s = (*letter as char).to_string();
                    out.extend(text_draws_for(
                        &self.font.layout_ascii(&s),
                        (170 + k as i32 * 8, *ry),
                        status_color,
                    ));
                }
            }

            // Floating damage / heal numbers, anchored just above each slot's
            // HP row and fading with the popup's remaining lifetime.
            let dmg_color = [0.5f32, 0.85, 1.0, 1.0];
            let heal_color = [0.5f32, 1.0, 0.5, 1.0];
            let crit_color = [1.0f32, 0.95, 0.4, 1.0];
            for p in self.battle_hud.popup_views() {
                let Some(Some(ry)) = row_y.get(p.slot as usize) else {
                    continue;
                };
                let base = if p.is_heal {
                    heal_color
                } else if p.is_crit {
                    crit_color
                } else {
                    dmg_color
                };
                let color = [base[0], base[1], base[2], base[3] * p.alpha.clamp(0.0, 1.0)];
                let text = if let Some(letter) = p.status_letter {
                    format!("[{}]", letter as char)
                } else if p.is_heal {
                    format!("+{}", p.amount)
                } else {
                    format!("-{}", p.amount)
                };
                out.extend(text_draws_for(
                    &self.font.layout_ascii(&text),
                    (120, *ry - 14),
                    color,
                ));
            }

            // Player-driven submenus (opened from the Arts / Magic / Item
            // commands). Each parks both the SM and the command session while
            // open, so it takes priority over the command menu.
            if let Some(arts) = &bw.battle_arts_menu {
                use legaia_engine_core::battle_arts::ArtsPhase;
                let menu_x = 8i32;
                let mut my = 210i32;
                match &arts.phase {
                    ArtsPhase::Select { cursor } => {
                        let header = format!("P{} - arts:", arts.actor + 1);
                        out.extend(text_draws_for(
                            &self.font.layout_ascii(&header),
                            (menu_x, my),
                            white,
                        ));
                        my += 16;
                        if arts.arts.is_empty() {
                            out.extend(text_draws_for(
                                &self.font.layout_ascii("  (no saved arts)"),
                                (menu_x + 8, my),
                                down_color,
                            ));
                        }
                        for (i, row) in arts.arts.iter().enumerate() {
                            let sel = i as u8 == *cursor;
                            let marker = if sel { ">" } else { " " };
                            let line = match (row.miracle, row.super_art) {
                                (Some(name), _) => {
                                    format!("{} {} x{} *{}*", marker, row.name, row.hits(), name)
                                }
                                (None, Some(name)) => {
                                    format!("{} {} x{} <{}>", marker, row.name, row.hits(), name)
                                }
                                (None, None) => format!("{} {} x{}", marker, row.name, row.hits()),
                            };
                            let color = if sel { white } else { dim };
                            out.extend(text_draws_for(
                                &self.font.layout_ascii(&line),
                                (menu_x + 8, my),
                                color,
                            ));
                            my += 14;
                        }
                    }
                    ArtsPhase::Targeting { picker, .. } => {
                        let line = match picker.state() {
                            PickerState::Cursor {
                                row: CursorRow::Enemy,
                                slot,
                            } => format!("art -> target M{}", slot + 1),
                            PickerState::Cursor {
                                row: CursorRow::Ally,
                                slot,
                            } => format!("art -> target P{}", slot + 1),
                            _ => "art -> select target".to_string(),
                        };
                        out.extend(text_draws_for(
                            &self.font.layout_ascii(&line),
                            (menu_x, my),
                            white,
                        ));
                        my += 14;
                        out.extend(text_draws_for(
                            &self
                                .font
                                .layout_ascii("Left/Right=move  Cross=confirm  Circle=back"),
                            (menu_x, my),
                            dim,
                        ));
                    }
                    _ => {}
                }
            } else if let Some(spell) = &bw.battle_spell_menu {
                use legaia_engine_core::battle_magic::SpellPhase;
                let menu_x = 8i32;
                let mut my = 210i32;
                match &spell.phase {
                    SpellPhase::Select { cursor } => {
                        let header = format!("P{} - magic:", spell.actor + 1);
                        out.extend(text_draws_for(
                            &self.font.layout_ascii(&header),
                            (menu_x, my),
                            white,
                        ));
                        my += 16;
                        if spell.spells.is_empty() {
                            out.extend(text_draws_for(
                                &self.font.layout_ascii("  (no spells)"),
                                (menu_x + 8, my),
                                down_color,
                            ));
                        }
                        for (i, row) in spell.spells.iter().enumerate() {
                            let sel = i as u8 == *cursor;
                            let marker = if sel { ">" } else { " " };
                            let line = format!("{} {} {:>2}MP", marker, row.name, row.mp_cost);
                            let color = if !row.affordable {
                                down_color
                            } else if sel {
                                white
                            } else {
                                dim
                            };
                            out.extend(text_draws_for(
                                &self.font.layout_ascii(&line),
                                (menu_x + 8, my),
                                color,
                            ));
                            my += 14;
                        }
                    }
                    SpellPhase::Targeting { picker, .. } => {
                        let line = match picker.state() {
                            PickerState::Cursor {
                                row: CursorRow::Enemy,
                                slot,
                            } => format!("cast -> target M{}", slot + 1),
                            PickerState::Cursor {
                                row: CursorRow::Ally,
                                slot,
                            } => format!("cast -> target P{}", slot + 1),
                            _ => "cast -> select target".to_string(),
                        };
                        out.extend(text_draws_for(
                            &self.font.layout_ascii(&line),
                            (menu_x, my),
                            white,
                        ));
                        my += 14;
                        out.extend(text_draws_for(
                            &self
                                .font
                                .layout_ascii("Left/Right=move  Cross=confirm  Circle=back"),
                            (menu_x, my),
                            dim,
                        ));
                    }
                    _ => {}
                }
            } else if let Some(menu) = &bw.battle_item_menu {
                out.extend(self.items_session_draws(menu));
            } else if let Some(cmd) = &bw.battle_command {
                let menu_x = 8i32;
                let mut my = 210i32;
                match &cmd.phase {
                    CommandPhase::Menu { .. } => {
                        let header = format!("P{} - command:", cmd.actor + 1);
                        out.extend(text_draws_for(
                            &self.font.layout_ascii(&header),
                            (menu_x, my),
                            white,
                        ));
                        my += 16;
                        let cur = cmd.menu_command();
                        for c in BattleCommand::MENU {
                            let marker = if Some(c) == cur { ">" } else { " " };
                            let line = if c.enabled() {
                                format!("{} {}", marker, c.label())
                            } else {
                                format!("{} {} --", marker, c.label())
                            };
                            let color = if Some(c) == cur {
                                white
                            } else if c.enabled() {
                                dim
                            } else {
                                down_color
                            };
                            out.extend(text_draws_for(
                                &self.font.layout_ascii(&line),
                                (menu_x + 8, my),
                                color,
                            ));
                            my += 14;
                        }
                    }
                    CommandPhase::Targeting { command, picker } => {
                        let line = match picker.state() {
                            PickerState::Cursor {
                                row: CursorRow::Enemy,
                                slot,
                            } => format!("{} -> target M{}", command.label(), slot + 1),
                            PickerState::Cursor {
                                row: CursorRow::Ally,
                                slot,
                            } => format!("{} -> target P{}", command.label(), slot + 1),
                            _ => format!("{} -> select target", command.label()),
                        };
                        out.extend(text_draws_for(
                            &self.font.layout_ascii(&line),
                            (menu_x, my),
                            white,
                        ));
                        my += 14;
                        let hint = "Left/Right=move  Cross=confirm  Circle=back";
                        out.extend(text_draws_for(
                            &self.font.layout_ascii(hint),
                            (menu_x, my),
                            dim,
                        ));
                    }
                    _ => {}
                }
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
        // Seru-capture banner: shown after a battle in which a Seru was
        // captured (and, if a threshold was crossed, a spell learned).
        if let Some(banner) = &self.session.host.world.current_capture_banner
            && let Some(text) = banner.current_banner()
        {
            out.extend(capture_banner_draws_for(&self.font, &text, (8, 40)));
        }
        // Opening-cutscene narration: the `opdeene` prologue subtitle pages,
        // centered near the bottom of the screen one page at a time.
        if let Some(narration) = &self.session.host.world.cutscene_narration
            && let Some(text) = narration.current_text()
        {
            let gold = [1.0f32, 0.92, 0.6, 1.0];
            let center_x = (w / 2) as i32;
            // Retail draws the subtitle at Y=180 on a 240px-tall virtual
            // screen (FUN_8003C764) - 3/4 down; scale to the real surface.
            let top_y = (h as i32 * 3 / 4).min(h as i32 - 16).max(0);
            out.extend(legaia_engine_render::cutscene_narration_draws_for(
                &self.font, text, center_x, top_y, gold,
            ));
        }
        // Name-entry overlay: the opening `town01` lead-character naming prompt.
        if let Some(entry) = &self.session.host.world.name_entry {
            use legaia_engine_core::name_entry::{CHAR_CELLS, GRID, GRID_COLS};
            let (grid_cursor, control_cursor) = if entry.cursor < CHAR_CELLS {
                (
                    Some((entry.cursor / GRID_COLS, entry.cursor % GRID_COLS)),
                    None,
                )
            } else {
                // Map the control-row column to a button index (Back=0/Space=1/End=2)
                // via the cell's resolved action.
                use legaia_engine_core::name_entry::Control;
                let ctrl = entry.control_at(entry.cursor);
                let idx = match ctrl {
                    Some(Control::Backspace) => Some(0),
                    Some(Control::Space) => Some(1),
                    Some(Control::End) => Some(2),
                    _ => None,
                };
                (None, idx)
            };
            let view = legaia_engine_render::NameEntryView {
                grid_rows: &GRID,
                control_labels: &["Back", "Space", "End"],
                name: &entry.name,
                grid_cursor,
                control_cursor,
                confirming: entry.state == legaia_engine_core::name_entry::NameEntryState::Confirm,
                confirm_yes: entry.confirm_yes,
                caret_on: (self.session.host.world.frame / 16).is_multiple_of(2),
            };
            out.extend(legaia_engine_render::name_entry_draws_for(
                &self.font,
                &view,
                (32, 24),
            ));
        }
        // Dialog box: the active NPC / event message, typed out one line near
        // the bottom of the screen (1/8 from the left, ~70% down - the
        // single-line layout the retail field VM emits). The panel mirrors
        // `World::current_dialog`; the world owns dismissal.
        if let Some(panel) = self.active_dialog.as_ref() {
            let to_ascii = |bytes: &[u8]| -> String {
                bytes
                    .iter()
                    .map(|&b| {
                        if (0x20..=0x7E).contains(&b) {
                            b as char
                        } else {
                            '?'
                        }
                    })
                    .collect()
            };
            let page = to_ascii(&panel.page_bytes());
            let layout = self.font.layout_ascii(&page);
            let pen = ((w as i32) / 8, (h as i32) * 7 / 10);
            out.extend(text_draws_for(&layout, pen, [1.0, 1.0, 1.0, 1.0]));

            // Multiple-choice menu: draw the decoded option labels under the
            // prompt, one row each, with a `>` cursor on the highlighted option
            // (the picker decoded from the inline interaction script).
            if panel.menu_active()
                && let Some(picker) = panel.picker()
            {
                // The proportional dialog font is a ~14px cell; one row per
                // option below the prompt.
                let line_h = 16i32;
                let cursor = panel.picker_cursor();
                for (i, opt) in picker.options.iter().enumerate() {
                    let selected = i == cursor;
                    let marker = if selected { "> " } else { "  " };
                    let label = format!("{marker}{}", to_ascii(&opt.label));
                    let row_layout = self.font.layout_ascii(&label);
                    let row_pen = (pen.0 + (w as i32) / 16, pen.1 + line_h * (i as i32 + 1));
                    let color = if selected {
                        [1.0, 1.0, 0.6, 1.0]
                    } else {
                        [0.8, 0.85, 1.0, 1.0]
                    };
                    out.extend(text_draws_for(&row_layout, row_pen, color));
                }
            }
        }

        // Inline-script field-VM runner box (the `--vm-dialogue` faithful path).
        // Same layout as the simplified panel, but the source is
        // `world.inline_dialogue`, which the world ticks itself.
        if let Some(id) = self.session.host.world.inline_dialogue.as_ref() {
            let to_ascii = |bytes: &[u8]| -> String {
                bytes
                    .iter()
                    .map(|&b| {
                        if (0x20..=0x7E).contains(&b) {
                            b as char
                        } else {
                            '?'
                        }
                    })
                    .collect()
            };
            let page = to_ascii(&id.page_bytes());
            if !page.is_empty() {
                let layout = self.font.layout_ascii(&page);
                let pen = ((w as i32) / 8, (h as i32) * 7 / 10);
                out.extend(text_draws_for(&layout, pen, [1.0, 1.0, 1.0, 1.0]));
                if id.menu_active()
                    && let Some(picker) = id.picker()
                {
                    let line_h = 16i32;
                    let cursor = id.picker_cursor();
                    for (i, opt) in picker.options.iter().enumerate() {
                        let selected = i == cursor;
                        let marker = if selected { "> " } else { "  " };
                        let label = format!("{marker}{}", to_ascii(&opt.label));
                        let row_layout = self.font.layout_ascii(&label);
                        let row_pen = (pen.0 + (w as i32) / 16, pen.1 + line_h * (i as i32 + 1));
                        let color = if selected {
                            [1.0, 1.0, 0.6, 1.0]
                        } else {
                            [0.8, 0.85, 1.0, 1.0]
                        };
                        out.extend(text_draws_for(&row_layout, row_pen, color));
                    }
                }
            }
        }
        out
    }
}

impl ApplicationHandler for PlayWindowApp {
    fn resumed(&mut self, evl: &ActiveEventLoop) {
        if !self.win.open(evl, "legaia-engine") {
            return;
        }
        // Opt-in PSX-faithful rendering: affine (perspective-incorrect) UV
        // warp + sub-pixel vertex jitter + 15-bit BGR555 ordered dithering on
        // the 3D mesh pipelines. Off by default (clean modern output); enable
        // with `LEGAIA_PSX_RENDER=1`.
        if std::env::var_os("LEGAIA_PSX_RENDER").is_some()
            && let Some(r) = self.win.renderer.as_ref()
        {
            r.set_psx_mode(true);
            log::info!("play-window: PSX-faithful render mode enabled");
        }
        self.upload_assets();
        self.win.request_redraw();
    }

    fn window_event(&mut self, evl: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                // Flush any pending record log before exiting so an Escape /
                // window-close mid-session produces a usable replay file.
                if let Some(log) = self.record_log.as_mut()
                    && let Err(e) = log.flush()
                {
                    log::error!("record: flush on CloseRequested failed: {e:#}");
                }
                evl.exit();
            }
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
                    if let Some(log) = self.record_log.as_mut()
                        && let Err(e) = log.flush()
                    {
                        log::error!("record: flush on Escape failed: {e:#}");
                    }
                    evl.exit();
                    return;
                }
                // Dev affordance: spawn a debug effect marker at the player so
                // the effect-pool render bridge can be exercised by hand
                // before the runtime effect catalog is wired into battle-enter.
                if matches!(code, KeyCode::KeyE)
                    && state == ElementState::Pressed
                    && !self.boot_ui.is_active()
                {
                    let pos = self
                        .session
                        .host
                        .world
                        .actors
                        .iter()
                        .find(|a| a.active)
                        .map(|a| {
                            [
                                a.move_state.world_x as f32,
                                a.move_state.world_y as f32,
                                a.move_state.world_z as f32,
                            ]
                        })
                        .unwrap_or([0.0, 0.0, 0.0]);
                    self.session.host.world.spawn_debug_effect(pos);
                    return;
                }
                // `F`: seat a synthetic *Tail Fire* effect carrying its `etmd`
                // 3D model (global TMD pool index 4, textured by the resident
                // `etim` texels) at the first active actor, exercising the
                // model render path with a fixed model index. The data-driven
                // effect-id -> etmd-model selection is decoded (move-power
                // record `+0x12`/`+0x16` effect-id lists -> the `0x801F6324`
                // prototype table -> record `model_sel` -> `global_tmd_pool
                // [model_sel + 3]`; `legaia_asset::move_power::EffectListEntry`
                // + `World::spawn_move_fx`, exercised by the `H` key); `F` is
                // just the simpler fixed-model dev hand-spawn.
                if matches!(code, KeyCode::KeyF)
                    && state == ElementState::Pressed
                    && !self.boot_ui.is_active()
                {
                    let pos = self
                        .session
                        .host
                        .world
                        .actors
                        .iter()
                        .find(|a| a.active)
                        .map(|a| {
                            [
                                a.move_state.world_x as f32,
                                a.move_state.world_y as f32,
                                a.move_state.world_z as f32,
                            ]
                        })
                        .unwrap_or([0.0, 0.0, 0.0]);
                    // Prefer the real Gimard *Tail Fire* mesh from the PROT
                    // 0871 effect-model library when it's resident; fall back
                    // to the PROT 0874 §0 preview stand-in otherwise.
                    let model_index = if self
                        .session
                        .host
                        .world
                        .global_tmd(legaia_engine_core::scene::GIMARD_TAIL_FIRE_MODEL_INDEX as i16)
                        .is_some()
                    {
                        legaia_engine_core::scene::GIMARD_TAIL_FIRE_MODEL_INDEX
                    } else {
                        legaia_engine_core::scene::ETMD_TAIL_FIRE_MODEL_INDEX
                    };
                    self.session
                        .host
                        .world
                        .spawn_debug_effect_model(pos, model_index);
                    return;
                }
                // `G`: debug-spawn the Gimard *Burning Attack* summon scene-
                // graph (extraction PROT 0903). Loads the stager overlay, parses
                // records, and seats a `SummonScene` at the first active actor;
                // it then animates each frame via `tick_summon` (the move VM)
                // and renders through the summon-part draw block below. Not the
                // production cast-band trigger (state 0x29) -- a hand-spawn to
                // exercise the driver, like the `F`-key static effect spawn.
                if matches!(code, KeyCode::KeyG)
                    && state == ElementState::Pressed
                    && !self.boot_ui.is_active()
                {
                    // In battle, debug-spawn the faithful summon creature
                    // (Gimard, 0x81) through the battle-creature render path.
                    if self.session.host.world.mode == SceneMode::Battle {
                        self.spawn_summon_creature(0x81);
                        return;
                    }
                    const PROT_GIMARD_SUMMON_STAGER: u32 = 903;
                    let origin = self
                        .session
                        .host
                        .world
                        .actors
                        .iter()
                        .find(|a| a.active)
                        .map(|a| {
                            [
                                a.move_state.world_x,
                                a.move_state.world_y,
                                a.move_state.world_z,
                            ]
                        })
                        .unwrap_or([0, 0, 0]);
                    // Read the stager's TOC-gap LBA footprint, not the
                    // indexed sub-region: the stager `.BIN`s over-read into
                    // the next entry, so the spawn-site pointer table must be
                    // parsed against the trimmed window (mirrors the disc-gated
                    // `summon_overlay_real` test's `unique_content_len` trim).
                    match self
                        .session
                        .host
                        .index
                        .entry_bytes_lba_footprint(PROT_GIMARD_SUMMON_STAGER)
                    {
                        Ok(bytes) => {
                            let overlay = legaia_asset::summon_overlay::parse(
                                &bytes,
                                legaia_asset::summon_overlay::SUMMON_OVERLAY_LINK_BASE,
                            );
                            self.session.host.world.spawn_summon(
                                &overlay,
                                &bytes,
                                legaia_engine_core::scene::GIMARD_TAIL_FIRE_MODEL_INDEX,
                                origin,
                            );
                            log::info!(
                                "spawned Gimard summon: {} parts at {origin:?}",
                                overlay.parts.len()
                            );
                        }
                        Err(e) => log::warn!("summon spawn: read summon stager PROT entry: {e:#}"),
                    }
                    return;
                }
                // `H`: debug-spawn a battle move's effect-FX scene-graph (the
                // move-power `0x801f6324` prototype records, summon-format
                // move-VM parts spawned through `FUN_80021B04`). Battle only -
                // the move-power table + overlay are battle-context. Move id
                // 0x06 is the worked example with library-mesh Spawn entries
                // (0x27 / 0x28); its parts resolve into the effect-model library
                // `global_tmd_pool[model_sel + 3]` and animate via `tick_move_fx`,
                // rendered by the move-FX draw block below.
                if matches!(code, KeyCode::KeyH)
                    && state == ElementState::Pressed
                    && !self.boot_ui.is_active()
                {
                    if self.session.host.world.mode != SceneMode::Battle {
                        log::info!("move-FX spawn (H) is battle-only");
                        return;
                    }
                    // Enumerate every move the parsed move-power table can render
                    // as a 3D scene-graph and cycle through them across presses
                    // (data-driven, so the preview reflects the real overlay
                    // rather than one hard-coded id). The 0x06 worked example -
                    // library-mesh Spawn entries 0x27/0x28 resolving into
                    // `global_tmd_pool[model_sel + 3]` - is the starting point
                    // when present.
                    let spawnable = self
                        .session
                        .host
                        .world
                        .move_power
                        .as_ref()
                        .map(|c| c.spawnable_move_ids())
                        .unwrap_or_default();
                    if spawnable.is_empty() {
                        log::info!(
                            "move-FX spawn (H): move-power table not installed / no spawnable moves"
                        );
                        return;
                    }
                    use std::sync::atomic::{AtomicUsize, Ordering};
                    static MOVE_FX_CYCLE: AtomicUsize = AtomicUsize::new(0);
                    let start = spawnable.iter().position(|&id| id == 0x06).unwrap_or(0);
                    let n = MOVE_FX_CYCLE.fetch_add(1, Ordering::Relaxed);
                    let slot = (start + n) % spawnable.len();
                    let move_fx_id = spawnable[slot];
                    log::info!(
                        "move-FX preview {}/{}: move {move_fx_id:#04x} (spawnable {:#04x?})",
                        slot + 1,
                        spawnable.len(),
                        spawnable
                    );
                    let origin = self
                        .session
                        .host
                        .world
                        .actors
                        .iter()
                        .find(|a| a.active)
                        .map(|a| {
                            [
                                a.move_state.world_x,
                                a.move_state.world_y,
                                a.move_state.world_z,
                            ]
                        })
                        .unwrap_or([0, 0, 0]);
                    if self.session.host.world.spawn_move_fx(move_fx_id, origin) {
                        log::info!(
                            "spawned move-FX for move {move_fx_id:#04x}: {} mesh parts at {origin:?}",
                            self.session.host.world.active_move_fx_part_draws().len()
                        );
                        // Consume the surfaced presentation fields: the trail
                        // texpage (render layer's streak pass) and the sound cue
                        // (routed through the FUN_8004fcc8 dispatch decode). The
                        // host has no battle SFX bank wired yet, so the cue is
                        // resolved + logged rather than fired through the SPU.
                        if let Some(trail) = self.session.host.world.active_move_fx_trail_texpage()
                        {
                            log::info!("  move-FX trail texpage = {trail:#06x}");
                        }
                        if let Some(cue) = self.session.host.world.take_pending_move_fx_cue() {
                            let dispatch = legaia_engine_audio::classify_cue(cue as u32);
                            log::info!("  move-FX sound cue {cue:#04x} -> {dispatch:?}");
                        }
                    } else {
                        log::info!(
                            "move-FX spawn for move {move_fx_id:#04x} produced no parts \
                             (table not installed / no spawnable entries)"
                        );
                    }
                    return;
                }
                // `J`: debug-spawn one field move-VM scene-graph effect from the
                // current scene's prescript stager table (the op-0x34-sub-3 /
                // `FUN_800252EC` path), cycling through the table across presses.
                // Exercises the field-FX tick/draw wiring without waiting for the
                // field VM to execute an op-0x34-sub-3; the production trigger is
                // `FieldHostImpl::effect_anim_trigger`.
                if matches!(code, KeyCode::KeyJ)
                    && state == ElementState::Pressed
                    && !self.boot_ui.is_active()
                {
                    let count = self.session.host.world.field_stagers.len();
                    if count == 0 {
                        log::info!("field-FX spawn (J): no prescript stager table for this scene");
                        return;
                    }
                    let origin = self
                        .session
                        .host
                        .world
                        .actors
                        .iter()
                        .find(|a| a.active)
                        .map(|a| {
                            [
                                a.move_state.world_x,
                                a.move_state.world_y,
                                a.move_state.world_z,
                            ]
                        })
                        .unwrap_or([0, 0, 0]);
                    use std::sync::atomic::{AtomicUsize, Ordering};
                    static FIELD_FX_CYCLE: AtomicUsize = AtomicUsize::new(0);
                    let id = FIELD_FX_CYCLE.fetch_add(1, Ordering::Relaxed) % count;
                    // Debug ergonomics: isolate one record per press (the
                    // production op-0x34-sub-3 path lets them stack).
                    self.session.host.world.active_field_fx.clear();
                    if self.session.host.world.spawn_field_stager(id, origin) {
                        let mesh: Vec<usize> = self
                            .session
                            .host
                            .world
                            .active_field_fx_part_draws()
                            .iter()
                            .map(|d| d.model_index)
                            .collect();
                        let nodes: Vec<_> = self
                            .session
                            .host
                            .world
                            .active_field_fx_render_nodes()
                            .iter()
                            .map(|n| n.mode)
                            .collect();
                        log::info!(
                            "field-FX spawn {}/{count} at {origin:?}: {} mesh parts (model_index {mesh:?}), {} render-mode nodes {nodes:?}",
                            id + 1,
                            mesh.len(),
                            nodes.len(),
                        );
                    }
                    return;
                }
                // `K`: toggle the Noa dance (rhythm) minigame. Loads the dance
                // overlay (PROT 0980), suspends the current scene, and runs the
                // beat clock + hit judge; Left/Right/Up are the three arrows.
                // Pressing K again aborts and logs the final score. The song
                // also ends itself after its length limit (see below).
                if matches!(code, KeyCode::KeyK)
                    && state == ElementState::Pressed
                    && !self.boot_ui.is_active()
                {
                    if self.session.host.world.mode == SceneMode::Dance {
                        if let Some(g) = self.session.host.world.exit_dance() {
                            log::info!(
                                "dance: aborted at score {} (pass={})",
                                g.score(),
                                g.passed()
                            );
                        }
                    } else if self.start_dance_minigame(false) {
                        log::info!("dance: started - Left/Right/Up are the arrows, K to quit");
                    }
                    return;
                }
                // `L`: toggle the fishing minigame. Loads the fishing overlay
                // (PROT 0972), suspends the current scene, and runs the cast /
                // fight / score loop; Cross locks the cast and reels (reel A),
                // Circle is reel B. Pressing L again leaves and logs the points.
                if matches!(code, KeyCode::KeyL)
                    && state == ElementState::Pressed
                    && !self.boot_ui.is_active()
                {
                    if self.session.host.world.mode == SceneMode::Fishing {
                        if let Some(s) = self.session.host.world.exit_fishing() {
                            log::info!(
                                "fishing: left with {} points (best {})",
                                s.record().points,
                                s.record().best_points
                            );
                        }
                    } else if self.start_fishing_minigame() {
                        log::info!(
                            "fishing: started - Cross casts/reels(A), Circle reels(B), L to quit"
                        );
                    }
                    return;
                }
                // `C`: toggle the field camera between the retail follow view
                // (savestate-pinned pitch/yaw/H, player-anchored - the
                // faithful framing) and the wide debug orbit vantage (better
                // for eyeballing scene completeness).
                if matches!(code, KeyCode::KeyC)
                    && state == ElementState::Pressed
                    && !self.boot_ui.is_active()
                {
                    self.field_debug_camera = !self.field_debug_camera;
                    log::info!(
                        "camera: field vantage = {}",
                        if self.field_debug_camera {
                            "debug orbit"
                        } else {
                            "retail follow"
                        }
                    );
                    return;
                }
                // `N`: open the name-entry overlay for the lead character. The
                // NEW GAME flow now opens it automatically at the `opdeene` ->
                // `town01` opening hand-off (see the prologue-handoff block
                // below); this key is a dev hand-trigger to exercise the ported
                // overlay outside that flow. The exact in-script field-VM op
                // that opens it mid-establishing-sequence is still an open RE
                // thread.
                if matches!(code, KeyCode::KeyN)
                    && state == ElementState::Pressed
                    && !self.boot_ui.is_active()
                    && !self.session.host.world.name_entry_active()
                {
                    self.session.host.world.open_name_entry(0);
                    return;
                }
                // Menu input: while the active dialog box is a multiple-choice
                // menu (a picker decoded from the inline interaction script),
                // Up/Down move the option cursor and a confirm button applies
                // the chosen option's relative jump (`FUN_80038050`) instead of
                // driving movement / dismissing the box. Resolve the key->button
                // first so the immutable `mapping` borrow ends before the
                // mutable `active_dialog` borrow.
                if state == ElementState::Pressed {
                    let is_confirm = matches!(
                        self.mapping.pad_button_for_key(keycode_to_name(code)),
                        Some(
                            legaia_engine_core::input::PadButton::Cross
                                | legaia_engine_core::input::PadButton::Circle
                        )
                    );
                    if let Some(panel) = self.active_dialog.as_mut()
                        && panel.menu_active()
                    {
                        if matches!(code, KeyCode::ArrowUp) {
                            panel.move_picker_cursor(-1);
                            return;
                        }
                        if matches!(code, KeyCode::ArrowDown) {
                            panel.move_picker_cursor(1);
                            return;
                        }
                        if is_confirm {
                            panel.confirm_menu();
                            return;
                        }
                    }
                }
                let key_name = keycode_to_name(code);
                if let Some(button) = self.mapping.pad_button_for_key(key_name) {
                    let prev = self.pad;
                    if state == ElementState::Pressed {
                        self.pad |= button.mask();
                    } else {
                        self.pad &= !button.mask();
                    }
                    // Record the transition iff the pad actually changed
                    // (auto-repeat sends a stream of Pressed events with
                    // identical mask; dedup in RecordLog::record_transition).
                    if self.pad != prev
                        && let Some(log) = self.record_log.as_mut()
                    {
                        log.record_transition(self.session.frames, self.pad);
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                let dt = self.win.advance_tick(100);
                // Drain up to 4 ticks per render frame so we never spiral
                // but can still catch up from minor vsync jitter.
                let ticks = self.win.drain_ticks(dt, 4);
                // In-flow windowed cutscene: when the field VM's FMV-trigger
                // op flips the world into SceneMode::Cutscene and the STR has
                // decoded, suspend world ticks and play the video in-window.
                // Once its frames drain, resume the field (`finish_cutscene`).
                if self
                    .cutscene
                    .as_ref()
                    .is_some_and(|c| c.idx >= c.frames.len())
                {
                    // Stop the cutscene audio and resume the scene sequencer
                    // (BGM was paused while the movie played).
                    if let Some(out) = self.session.audio.as_ref() {
                        out.stop_xa();
                        out.set_sequencer_paused(false);
                    }
                    self.session.host.world.finish_cutscene();
                    self.cutscene = None;
                }
                let run_ticks = if self.cutscene.is_some() { 0 } else { ticks };
                for _ in 0..run_ticks {
                    // When the boot UI is active, route input there and skip
                    // the scene tick - the player hasn't entered the world
                    // yet (or has paused into save-select).
                    if self.boot_ui.is_active() {
                        let _ = self.tick_boot_ui();
                        self.prev_pad = self.pad;
                        continue;
                    }
                    // Start in field opens the pause menu. Edge-detect so a
                    // held key doesn't auto-reopen.
                    let pressed_edge = self.pad & !self.prev_pad;
                    // Name-entry overlay is modal: while it's open the field is
                    // frozen and every pad edge routes into the entry SM (one
                    // cell / glyph per press). Mirrors the opening `town01`
                    // naming prompt, which suspends the field VM.
                    if self.session.host.world.name_entry_active() {
                        let p = pressed_edge;
                        let input = legaia_engine_core::name_entry::NameEntryInput {
                            up: p & 0x0010 != 0,
                            down: p & 0x0040 != 0,
                            left: p & 0x0080 != 0,
                            right: p & 0x0020 != 0,
                            confirm: p & 0x4000 != 0, // Cross
                            cancel: p & 0x1000 != 0,  // Triangle
                        };
                        self.session.host.world.step_name_entry(input);
                        // Keep the frame counter advancing so the caret blinks.
                        self.session.host.world.frame =
                            self.session.host.world.frame.wrapping_add(1);
                        self.prev_pad = self.pad;
                        continue;
                    }
                    // Opening-cutscene narration plays first. While its
                    // subtitle pages are on screen the field is held and a
                    // confirm press (Cross) skips to the next page; the
                    // per-page timer (World::tick) auto-advances otherwise.
                    // Only once the narration completes does a confirm reach
                    // the hand-off gate below - so the prologue narration
                    // precedes the Rim Elm transition, mirroring retail order.
                    if self.session.host.world.cutscene_narration_active() {
                        if pressed_edge & 0x4000 != 0 {
                            self.session.host.world.skip_cutscene_narration();
                        }
                        // Freeze player movement (pad held at 0) but keep the
                        // world ticking so the narration's per-page timer
                        // advances and the scene still renders.
                        self.session.host.world.set_pad(0);
                        if let Err(e) = self.session.tick() {
                            log::error!("session tick (narration): {e:#}");
                        }
                        self.prev_pad = self.pad;
                        continue;
                    }
                    // Prologue cutscene -> Rim Elm handoff. While in `opdeene`
                    // with the trigger armed, a confirm press (Cross) hands off
                    // to `town01`, mirroring FUN_801D1344's flag + pad gate.
                    if let Some(target) = self
                        .session
                        .host
                        .world
                        .take_prologue_handoff(pressed_edge & 0x4000 != 0)
                    {
                        match self.session.enter_field_live(target, &self.field_live_opts) {
                            Ok(mode) => {
                                log::info!("prologue handoff: entered '{target}' (mode={mode:?})");
                                // `enter_field_scene` installs `town01`'s opening
                                // cutscene timeline (gated on the prologue hand-off):
                                // the establishing camera + Vahn's scripted walk-out
                                // play, and the name-entry overlay opens when the
                                // timeline reaches its pinned op-`0x49` STATE_RESUME
                                // (P2[3] body `0x02c6`) - the faithful in-script
                                // trigger, not a blind host call at the hand-off.
                            }
                            Err(e) => {
                                log::warn!("prologue handoff: enter '{target}' failed ({e:#})")
                            }
                        }
                        self.prev_pad = self.pad;
                        continue;
                    }
                    if pressed_edge & 0x0008 != 0 && !self.menu_runtime.is_open() {
                        // Start: open the BootSession-hosted pause menu (the
                        // retail CARD pair, game_mode 0x17 - the world holds
                        // SceneMode::Menu while it is open) and route the
                        // window's input + draws to it via the boot-UI arm.
                        self.session.open_field_menu();
                        self.boot_ui = BootUiState::FieldMenu { sub: None };
                        self.prev_pad = self.pad;
                        continue;
                    }
                    // Route this frame's pad into the engine before the
                    // tick so World::tick's mode dispatch (world-map
                    // controller, field-VM dialog-advance poll) sees real
                    // input. Edge detection lives in World.input. While a
                    // menu-runtime overlay (shop / inn) is up the pad drives
                    // the menu, not the field, so feed the field a neutral pad
                    // (the player must not walk while shopping).
                    let field_pad = if self.menu_runtime.is_open() {
                        0
                    } else {
                        self.pad
                    };
                    self.session.host.world.set_pad(field_pad);
                    if let Err(e) = self.session.tick() {
                        log::error!("session tick: {e:#}");
                    }
                    // Dance minigame auto-end: `tick_dance` restores the scene
                    // mode when the song timer runs out but leaves the game
                    // installed for one frame. Detect that (mode no longer
                    // Dance while a game is still present), log the final grade,
                    // and clear it.
                    if self.session.host.world.mode != SceneMode::Dance
                        && let Some(g) = self.session.host.world.exit_dance()
                    {
                        log::info!(
                            "dance: song finished - score {} (pass={})",
                            g.score(),
                            g.passed()
                        );
                    }
                    // A field-VM shop op (`0x49` sub-0 inline shop record) opened
                    // a priced gold shop this tick: hand the player into its buy
                    // list. The field VM is suspended (op-0x49 Armed) until the
                    // player leaves, at which point `finish_field_shop` (below)
                    // lets it resume past the merchant op.
                    if let Some(shop) = self.session.host.world.take_pending_field_shop() {
                        // Open the top-level Buy / Sell / Trade picker (Trade row
                        // present only when the disc enabled seru trading). Names
                        // for the trade rows come from the boot SCUS.
                        if self.session.host.world.seru_trade_enabled() {
                            self.ensure_seru_names();
                        }
                        self.menu_runtime.open_shop_menu(shop);
                    }
                    // Production cast-band trigger: a player Seru-magic cast
                    // (spell id 0x81..=0x8b) requests a summon spawn. The
                    // faithful render is the namesake battle_data creature drawn
                    // through the enemy animation pipeline (the summon reuses
                    // that creature's mesh + per-object TRS animation), so spawn
                    // it as a battle creature rather than the move-VM scene-graph
                    // stand-in (`summon::summon_creature_id`).
                    if let Some((spell_id, _origin)) =
                        self.session.host.world.take_pending_summon_spawn()
                    {
                        self.spawn_summon_creature(spell_id);
                    }
                    // Production move-FX trigger: a non-summon spell cast or
                    // enemy special whose move-power record carries a spawnable
                    // effect list requests its `0x801f6324` scene-graph spawn at
                    // the target's battle position. Seat it through the same
                    // move-VM path the `H` debug key and field FX use.
                    if let Some((move_id, origin)) =
                        self.session.host.world.take_pending_move_fx_spawn()
                        && self.session.host.world.spawn_move_fx(move_id, origin)
                    {
                        // Route the move's sound cue the same way the field-FX /
                        // debug path does (classify only; move-FX cue playback is
                        // not yet wired to the SFX ring).
                        if let Some(cue) = self.session.host.world.take_pending_move_fx_cue() {
                            let dispatch = legaia_engine_audio::classify_cue(cue as u32);
                            log::debug!("battle move-FX cue {cue:#04x} -> {dispatch:?}");
                        }
                    }
                    // Advance an active Seru-magic summon scene-graph (the cast
                    // above, or the `G` debug spawn) through the move VM.
                    self.session.host.world.tick_summon(0x0400);
                    // Advance an active battle move-FX scene-graph (the `H` debug
                    // spawn) through the same move VM.
                    self.session.host.world.tick_move_fx(0x0400);
                    // Advance any field move-VM scene-graph effects spawned by the
                    // field-VM op 0x34 sub-3 ("Play 3D animation") - the per-scene
                    // prescript stagers (`FUN_800252EC` → `FUN_80021DF4`). No-op
                    // when none are live (off the field / no trigger fired).
                    self.session.host.world.tick_field_fx(0x0400);
                    // In battle, advance each monster actor's per-object idle
                    // animation into its `pose_frame` (the render pass below
                    // deforms the mesh via the rigid `posed_rot` builder).
                    if self.session.host.world.mode == SceneMode::Battle {
                        self.session.host.world.tick_battle_animations();
                        // ...and re-stamp the party's eye/mouth face frames
                        // from the playing clips' facial tracks (the retail
                        // per-frame facial animator).
                        self.tick_battle_face_stamps();
                    }
                    // World-map ocean shimmer: cycle the 13-frame CLUT animation
                    // (self-gates to None off the world map).
                    self.advance_ocean_animation();
                    // Catch any path that re-uploaded VRAM over the battle
                    // texture this frame (and restore it).
                    self.check_battle_vram_residency();
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
                    // A field-VM-triggered shop the player has now closed: tell
                    // the world so the suspended op-0x49 resumes (Armed -> Done)
                    // and the field VM advances past the merchant op next tick.
                    if self.session.host.world.field_shop_open && !self.menu_runtime.is_open() {
                        self.session.host.world.finish_field_shop();
                    }
                    self.prev_pad = self.pad;
                    // Record-mode: advance the log's frame counter so
                    // `meta.frames` reflects the recorded duration even
                    // when the user closes mid-run with no pad transitions.
                    if let Some(log) = self.record_log.as_mut() {
                        log.observe_frame(self.session.frames);
                    }
                    // Drain whatever battle events the SM fired this tick,
                    // fold their gameplay-state side into the world (HP /
                    // status), and ring them into the HUD log.
                    self.drain_and_log_battle_events();
                    // Route field events: ActorSpawned events whose actor
                    // carries a `tmd_ref` queue a render-pass mesh upload
                    // so spawn-record actors appear in the scene.
                    self.drain_and_route_field_events();
                    // Mirror the world's dialog request into a rendered,
                    // typed-out panel (opened from the scene MES, dropped when
                    // the world dismisses the box).
                    self.sync_dialog_panel();
                }
                // A tick this frame may have flipped the world into
                // SceneMode::Cutscene (field-VM FMV-trigger op). Start
                // windowed STR playback if so; a cut/missing slot drains the
                // trigger as a no-op (mirrors the headless `play` loop).
                if self.cutscene.is_none() {
                    self.try_start_windowed_cutscene();
                }
                // While a cutscene plays, the window shows the video and the
                // scene render is skipped entirely.
                if self.cutscene.is_some() {
                    self.render_windowed_cutscene();
                    self.win.request_redraw();
                    return;
                }
                // On a Field<->Battle transition, upload/drop monster meshes
                // and swap the VRAM. Must run before the render borrows
                // `uploaded_vram` below (this method may re-upload it).
                self.sync_battle_render();
                // Ease the in-engine cutscene camera between Camera Configure
                // beats. Done here (outside the renderer borrow below) so the
                // interpolator can take `&mut self`; while no cutscene timeline
                // owns the scene the interp is reset so the next opening shot
                // snaps in rather than sweeping from a stale pose.
                let cutscene_cam = if self.session.host.world.mode != SceneMode::WorldMap
                    && self.session.host.world.cutscene_timeline_active()
                {
                    let (look_at, pitch, yaw, fov) = self.cutscene_view();
                    // ~0.15/frame ease => a few-frame blend at the redraw cadence.
                    Some(
                        self.cutscene_cam_interp
                            .approach(look_at, pitch, yaw, fov, 0.15),
                    )
                } else {
                    self.cutscene_cam_interp.reset();
                    None
                };
                if let (Some(r), Some(vram), Some(atlas)) = (
                    self.win.renderer.as_ref(),
                    self.uploaded_vram.as_ref(),
                    self.font_atlas.as_ref(),
                ) {
                    let (w, h) = r.surface_size();
                    let aspect = w as f32 / h.max(1) as f32;
                    // World-map mode frames the loaded map with the
                    // controller-driven camera (azimuth / zoom / pan); an active
                    // in-engine cutscene (opdeene opening prologue) frames the
                    // cutscene's executed op-0x45 camera target; every other mode
                    // uses the orbit camera.
                    let in_world_map = self.session.host.world.mode == SceneMode::WorldMap;
                    let cam = if in_world_map {
                        let world = &self.session.host.world;
                        let (az, zoom, px, pz, walk_mode) = world
                            .world_map_ctrl
                            .as_ref()
                            .map(|c| (c.azimuth, c.zoom, c.camera_x, c.camera_z, !c.is_top_view()))
                            .unwrap_or((0, 0, 0, 0, true));
                        // In walk mode the camera follows the player: pan so the
                        // framing centre tracks the player's world position
                        // (the AABB-relative offset world_map_camera_mvp adds to
                        // its centre). Top-view debug keeps the controller scroll.
                        let (pan_x, pan_z) = if walk_mode {
                            let center = [
                                (self.scene_aabb.0[0] + self.scene_aabb.1[0]) * 0.5,
                                (self.scene_aabb.0[2] + self.scene_aabb.1[2]) * 0.5,
                            ];
                            world
                                .player_actor_slot
                                .and_then(|s| world.actors.get(s as usize))
                                .map(|a| {
                                    (
                                        (a.move_state.world_x as f32 - center[0]) as i32,
                                        (a.move_state.world_z as f32 - center[1]) as i32,
                                    )
                                })
                                .unwrap_or((px, pz))
                        } else {
                            (px, pz)
                        };
                        // Walk view frames a fixed WORLD-space radius around the
                        // player rather than the (small, object-local) kingdom-
                        // pack AABB - the continent terrain now draws at world
                        // tile coordinates (`field_placement_draws`), so the
                        // pack-AABB radius would frame only the one tile under
                        // the player. Keep the box centred at the pack-AABB
                        // centre (the pan re-centres it on the player) and widen
                        // it; top-view keeps the full-pack framing for the
                        // overhead continent sweep.
                        let (cam_lo, cam_hi) = if walk_mode {
                            // Frame a wide world-space radius around the player so
                            // the overworld reads at retail's overhead scale (the
                            // walk camera also sits steeper - see
                            // `walk_view_camera_mvp`).
                            const WALK_HALF: f32 = 4200.0;
                            let cx = (self.scene_aabb.0[0] + self.scene_aabb.1[0]) * 0.5;
                            let cz = (self.scene_aabb.0[2] + self.scene_aabb.1[2]) * 0.5;
                            (
                                [cx - WALK_HALF, self.scene_aabb.0[1], cz - WALK_HALF],
                                [cx + WALK_HALF, self.scene_aabb.1[1], cz + WALK_HALF],
                            )
                        } else {
                            (self.scene_aabb.0, self.scene_aabb.1)
                        };
                        if walk_mode {
                            legaia_engine_render::window::walk_view_camera_mvp(
                                cam_lo, cam_hi, az, zoom, pan_x, pan_z, aspect,
                            )
                        } else {
                            legaia_engine_render::window::world_map_camera_mvp(
                                cam_lo, cam_hi, az, zoom, pan_x, pan_z, aspect,
                            )
                        }
                    } else if let Some((look_at, pitch, yaw, fov)) = cutscene_cam {
                        legaia_engine_render::window::cutscene_camera_mvp(
                            look_at,
                            pitch,
                            yaw,
                            fov,
                            self.scene_aabb.0,
                            self.scene_aabb.1,
                            aspect,
                        )
                    } else if self.session.host.world.mode == SceneMode::Battle {
                        if self.battle_stage_mesh.is_some() {
                            // Stage-dome battle: low front-facing shot into the
                            // dome (grass foreground, mountains on the horizon).
                            self.battle_dome_camera_mvp(aspect)
                        } else {
                            // No stage: frame the animated enemies (the battle
                            // actors live at the world origin).
                            self.battle_camera_mvp(aspect)
                        }
                    } else if self.field_debug_camera {
                        // Wide debug orbit vantage (`C` toggles).
                        self.camera_mvp(aspect)
                    } else {
                        // Field: the retail follow camera (savestate-pinned
                        // pitch/yaw/H, player-anchored) when a player actor
                        // exists; the fixed debug orbit vantage otherwise.
                        self.field_follow_camera_mvp(aspect)
                            .unwrap_or_else(|| self.camera_mvp(aspect))
                    };
                    // Drain queued spawn slots: build a VRAM mesh from each
                    // actor's `tmd_ref` (global-pool TMD that the field-VM
                    // 0x4C 0xD8 host hook installed) and append it to
                    // `self.meshes` / `self.scene_tmd_data`, then bind
                    // `actor.tmd_binding` to the new mesh index so the
                    // draws iteration below picks it up. Idempotent: if
                    // the actor already has a binding (e.g. an earlier
                    // pass already uploaded), the spawn is skipped.
                    let pending = std::mem::take(&mut self.pending_dynamic_mesh_slots);
                    for slot in pending {
                        let actor = match self.session.host.world.actors.get(slot as usize) {
                            Some(a) => a,
                            None => continue,
                        };
                        // Idempotence is tracked per-slot, NOT by "already has
                        // a binding": `upload_assets` naively pre-binds every
                        // actor K -> scene TMD slot K, and the player's spawn
                        // (its `tmd_ref` = the real character mesh from the
                        // global pool) must override that placeholder or the
                        // player renders as whatever scene mesh happened to
                        // share its slot index (usually invisible).
                        if self.drained_spawn_slots.contains(&slot) {
                            continue;
                        }
                        let Some(gtmd) = actor.tmd_ref.as_ref().map(std::sync::Arc::clone) else {
                            continue;
                        };
                        let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&gtmd.tmd, &gtmd.raw);
                        if vmesh.indices.is_empty() {
                            log::warn!(
                                "play-window: spawn slot {slot} has TMD with 0 indices; skipping"
                            );
                            continue;
                        }
                        match r.upload_vram_mesh(
                            &vmesh.positions,
                            &vmesh.uvs,
                            &vmesh.cba_tsb,
                            &vmesh.normals,
                            &vmesh.indices,
                        ) {
                            Ok(m) => {
                                let new_idx = self.meshes.len();
                                self.meshes.push(m);
                                self.scene_tmd_data
                                    .push((gtmd.tmd.clone(), gtmd.raw.clone()));
                                self.session.host.world.actors[slot as usize].tmd_binding =
                                    Some(new_idx);
                                self.drained_spawn_slots.insert(slot);
                                log::info!("play-window: spawn slot {slot} -> mesh slot {new_idx}");
                            }
                            Err(e) => log::warn!("spawn mesh upload: {e:#}"),
                        }
                    }
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
                        // Battle actors carry a per-object rigid-transform clip
                        // (rotation matters), so use the full `R·v + T` builder;
                        // field actors keep the translation-only ANM path
                        // unchanged.
                        let mut vmesh = if actor.battle_animation.is_some() {
                            legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(
                                tmd,
                                raw,
                                &pose.bone_outputs,
                            )
                        } else {
                            legaia_tmd::mesh::tmd_to_vram_mesh_posed(tmd, raw, &pose.bone_outputs)
                        };
                        // The posed mesh is rebuilt from the raw TMD, so its
                        // CBA/TSB are the nominal on-disc defaults. Re-apply the
                        // per-slot relocation `battle_render_mesh` did for the
                        // rest mesh, or the animated monster samples the wrong
                        // VRAM page and renders white.
                        if let Some(slot) = actor.battle_tex_slot {
                            for ct in &mut vmesh.cba_tsb {
                                ct[0] = legaia_asset::monster_archive::relocate_cba(ct[0], slot);
                                ct[1] = legaia_asset::monster_archive::relocate_tsb(ct[1], slot);
                            }
                        }
                        if vmesh.indices.is_empty() {
                            continue;
                        }
                        if std::env::var_os("LEGAIA_DIAG_POSE").is_some() {
                            let (lo, hi) = vmesh.aabb();
                            log::info!(
                                "DIAG pose: tmd {tmd_idx} verts {} aabb {lo:?}..{hi:?} \
                                 bones[0..2]={:?}",
                                vmesh.positions.len(),
                                &pose.bone_outputs[..pose.bone_outputs.len().min(2)]
                            );
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

                    // Iterate every actor that has a `tmd_binding`. Scene-init
                    // actors (slots 0..N from `init_scene_animations`) have
                    // their bindings set but aren't necessarily `.active` -
                    // the original draws iteration walked meshes directly,
                    // so we preserve that behaviour by not gating on
                    // `.active` here. Dynamically spawned actors set both
                    // `.active` and a binding to their freshly uploaded
                    // mesh slot (beyond `scene_tmd_data.len()`) via the
                    // spawn pass above.
                    //
                    // Suppress 3D draws while the boot UI is active so the
                    // last-loaded scene (e.g. a town) doesn't show through
                    // behind publisher logos / title / save-select.
                    let mut draws: Vec<SceneDraw<'_>> = Vec::new();
                    // Untextured (F*/G*) field props, drawn on the colour
                    // pipeline alongside the textured `draws`.
                    let mut color_draws: Vec<ColorSceneDraw<'_>> = Vec::new();
                    if self.boot_ui.is_active() {
                        // Boot UI is fullscreen - suppress 3D draws.
                    } else if in_world_map {
                        // World-map continent = two layers, both in the shared
                        // player / entity-marker world frame:
                        //
                        // 1. The **ground** is a heightfield surface
                        //    (`ground_heightfield`) built from the walk
                        //    `.MAP` floor grid (`Scene::walk_heightfield`,
                        //    elevation per `FUN_80019278`). It draws with a
                        //    provisional uniform ground texel: per-tile
                        //    texturing has no clean source - the record `+0x14`
                        //    byte is terrain-type metadata, not an atlas
                        //    selector (no draw path reads it; see
                        //    docs/subsystems/world-map.md "Open (texturing)").
                        // 2. The sparse **placed landmarks** (trees / mountains
                        //    / castle) are slot-1 pack meshes positioned per
                        //    occupied tile (`world_map_terrain_draws`, the
                        //    `flags & 0x4` set resolved via record[+0x10]+prefix).
                        //
                        // The earlier per-cell pack-mesh sweep that stamped a
                        // mesh on every `0x1000` cell was wrong (it flooded the
                        // map with pool-5; see docs/subsystems/world-map.md).
                        let yflip = Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
                        if let Some(hf_mesh) = self.ground_heightfield.as_ref() {
                            draws.push(SceneDraw {
                                mesh: hf_mesh,
                                mvp: cam * yflip,
                            });
                        }
                        for (mesh_idx, model) in self.world_map_terrain_draws.iter() {
                            if let Some(mesh) = self.meshes.get(*mesh_idx) {
                                draws.push(SceneDraw {
                                    mesh,
                                    mvp: cam * *model,
                                });
                            }
                        }
                        // Last-resort fallback: nothing resolved at all -> draw
                        // the whole pack at pack-local coords so the map isn't
                        // blank.
                        if self.ground_heightfield.is_none()
                            && self.world_map_terrain_draws.is_empty()
                        {
                            for mesh in &self.meshes {
                                draws.push(SceneDraw {
                                    mesh,
                                    mvp: cam * yflip,
                                });
                            }
                        }
                    } else {
                        let in_battle = self.session.host.world.mode == SceneMode::Battle;
                        if in_battle {
                            // Battle backdrop: the scene's `scene_tmd_stream`
                            // dome (PROT 88 for the overworld map01 battle) -
                            // sky hemisphere + mountain arc + grass - drawn at its
                            // **raw world coordinates** under the exact retail
                            // orbit camera (`retail_battle_mvp`). `model = F`
                            // (plain Y-flip): the camera bakes in `F`, so
                            // `cam * F` recovers the raw PSX vertex the retail
                            // transform expects.
                            //
                            // Drawn ONCE, world-fixed - matching retail, which
                            // sets the dome up as a background **actor**
                            // (`FUN_800513F0`: `tmd_register` -> `DAT_8007C018[]`
                            // + `FUN_80020de0` actor_alloc + `FUN_80020f88` link)
                            // rendered by the normal actor path `FUN_80048A08`.
                            // The dome is a FRONT half (verts `Z in [-1260,
                            // +12155]`), so it is NOT a full surround: as the
                            // camera orbits, different portions of the front arc
                            // come into view and the rest of the horizon is open
                            // sky/grass. The retail captures bear this out -
                            // mountains cover only 44–81% of the horizon columns
                            // depending on angle (NOT a ring). Earlier engine
                            // builds added a 180° mirror to "complete" the
                            // surround; that over-fills what retail leaves as a
                            // gap, so it is removed. See
                            // `project_battle_backdrop_is_prot88_dome`.
                            if let Some(stage_idx) = self.battle_stage_mesh
                                && let Some(mesh) = self.meshes.get(stage_idx)
                            {
                                let flip = Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
                                // Front half-dome at raw world coords.
                                draws.push(SceneDraw {
                                    mesh,
                                    mvp: cam * flip,
                                });
                                // The dome geometry is a FRONT half (Z>=0), so a
                                // single instance leaves the back of the horizon
                                // open. Draw a 180-deg-Y mirror so the mountain
                                // ring + sky complete the full circle around the
                                // actors. (Retail's dome is a front half with
                                // partial coverage; the mirror reads fuller.)
                                let back = flip * Mat4::from_rotation_y(std::f32::consts::PI);
                                draws.push(SceneDraw {
                                    mesh,
                                    mvp: cam * back,
                                });
                            }
                        } else {
                            // Bulk ground FIRST: the `.MAP` floor-grid
                            // heightfield (the `0x1000` ground layer - most
                            // town floor cells have NO pack mesh, so without
                            // this surface they render as holes).
                            if let Some(hf_mesh) = self.ground_heightfield.as_ref() {
                                let yflip = Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
                                draws.push(SceneDraw {
                                    mesh: hf_mesh,
                                    mvp: cam * yflip,
                                });
                            }
                            // Then the terrain / decor tile layer (drawn under
                            // the buildings): the `CELL_VISIBLE` field-map tiles
                            // (stone plaza, paths, riverbank).
                            for (mesh_idx, model) in &self.field_terrain_draws {
                                if let Some(mesh) = self.meshes.get(*mesh_idx) {
                                    draws.push(SceneDraw {
                                        mesh,
                                        mvp: cam * *model,
                                    });
                                }
                            }
                            // Untextured ground tiles (vertex-colour meshes the
                            // textured bridge has no entry for) - without these
                            // the floor shows holes where a tile's mesh carries
                            // no textured prims.
                            for (mesh_idx, model) in &self.field_terrain_color_draws {
                                if let Some(mesh) = self.color_meshes.get(*mesh_idx) {
                                    color_draws.push(ColorSceneDraw {
                                        mesh,
                                        mvp: cam * *model,
                                    });
                                }
                            }
                            // Static environment geometry: draw each placed
                            // building / terrain mesh at its world transform
                            // (resolved at scene load in
                            // `resolve_field_placement_draws`).
                            for (mesh_idx, model) in &self.field_placement_draws {
                                if let Some(mesh) = self.meshes.get(*mesh_idx) {
                                    draws.push(SceneDraw {
                                        mesh,
                                        mvp: cam * *model,
                                    });
                                }
                            }
                            // Untextured props (the F*/G* meshes the VRAM path
                            // drops) on the colour pipeline, same transforms.
                            for (mesh_idx, model) in &self.field_placement_color_draws {
                                if let Some(mesh) = self.color_meshes.get(*mesh_idx) {
                                    color_draws.push(ColorSceneDraw {
                                        mesh,
                                        mvp: cam * *model,
                                    });
                                }
                            }
                            // The player's untextured mesh half (pants /
                            // sleeves), following the actor's live transform.
                            if let Some((cidx, slot)) = self.player_color_draw
                                && let Some(mesh) = self.color_meshes.get(cidx)
                            {
                                color_draws.push(ColorSceneDraw {
                                    mesh,
                                    mvp: cam * self.actor_model(slot),
                                });
                            }
                        }
                        // Single battle camera for the actors too (same `cam` as
                        // the dome + grid) so the whole scene shares one space.
                        let actor_cam = cam;
                        // Flat tiled ground grid (retail's func_0x801d02c0 grass)
                        // under the actors, on the same battle camera so the
                        // party stands on it and the foreground reads as grass
                        // instead of the bare clear colour. `cam` bakes in the
                        // Y-flip, so `* flip` recovers the raw PSX y=0 plane.
                        if in_battle
                            && let Some(gi) = self.battle_ground_mesh
                            && let Some(gmesh) = self.meshes.get(gi)
                        {
                            let flip = Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
                            draws.push(SceneDraw {
                                mesh: gmesh,
                                mvp: cam * flip,
                            });
                        }
                        for (i, actor) in self.session.host.world.actors.iter().enumerate() {
                            let Some(tmd_idx) = actor.tmd_binding else {
                                continue;
                            };
                            // In a stage-dome battle, draw only the ACTIVE battle
                            // actors (party + monsters). The scene-init actors
                            // (bound but inactive, parked at the origin) would
                            // otherwise pile their meshes at world (0,0,0) - the
                            // "duplicate Vahn" + scattered scene geometry.
                            if in_battle && self.battle_stage_mesh.is_some() && !actor.active {
                                continue;
                            }
                            let mesh = posed_overrides
                                .get(tmd_idx)
                                .and_then(|o| o.as_ref())
                                .or_else(|| self.meshes.get(tmd_idx));
                            if let Some(mesh) = mesh {
                                draws.push(SceneDraw {
                                    mesh,
                                    mvp: actor_cam * self.actor_model(i),
                                });
                            }
                        }
                    }
                    let hud = self.build_hud(w, h);
                    let overlay = TextOverlay { atlas, draws: &hud };

                    // Boot-phase sprite overlay: alternates between the
                    // publisher-logos atlas (during PublisherLogos) and
                    // the title-screen atlas (during Title). PROKION/SCEA
                    // are vertically-packed sprite atlases -
                    // `publisher_logo_sprite_draws` unfolds them into N
                    // side-by-side strips; Contrail/WARNING + the title
                    // TIM produce a single quad each.
                    let logo_draw_vec = self.publisher_logo_sprite_draws(w, h);
                    let title_draw_vec = self.title_screen_sprite_draws(w, h);
                    let menu_glyph_draw_vec = self.title_menu_glyph_sprite_draws(w, h);
                    let save_chrome_draw_vec = self.save_select_chrome_sprite_draws(w, h);
                    let logo_overlay = self.publisher_logos.as_ref().map(|p| TextOverlay {
                        atlas: &p.atlas,
                        draws: &logo_draw_vec,
                    });
                    let title_overlay = self.title_screen.as_ref().map(|t| TextOverlay {
                        atlas: &t.atlas,
                        draws: &title_draw_vec,
                    });
                    let menu_glyph_overlay = self.menu_glyphs.as_ref().map(|m| TextOverlay {
                        atlas: &m.atlas,
                        draws: &menu_glyph_draw_vec,
                    });
                    let save_chrome_overlay = self.save_menu.as_ref().map(|sm| TextOverlay {
                        atlas: &sm.atlas,
                        draws: &save_chrome_draw_vec,
                    });

                    // Force a pure-black background during boot UI so the
                    // logos / title / save-select panels read on PSX-style
                    // black instead of the default dark-blue clear. In a
                    // stage-dome battle clear to a sky blue so the gaps the
                    // front-half dome leaves open read as sky (like retail)
                    // rather than the bare grey clear.
                    let scene_clear = if self.boot_ui.is_active() {
                        Some([0.0, 0.0, 0.0, 1.0])
                    } else if self.session.host.world.mode == SceneMode::Battle
                        && self.battle_stage_mesh.is_some()
                    {
                        Some([0.32, 0.46, 0.66, 1.0])
                    } else {
                        None
                    };

                    // Slot 1: logos OR title-art bands (title still
                    // emits during SaveSelect, dimmed). Slot 2: either
                    // the save-menu chrome (panel + slot pills) when
                    // SaveSelect is active, or the menu-glyph atlas
                    // (deprecated no-disc title-menu fallback) otherwise.
                    let sprites_slot_1 = if !logo_draw_vec.is_empty() {
                        logo_overlay.as_ref()
                    } else if !title_draw_vec.is_empty() {
                        title_overlay.as_ref()
                    } else {
                        None
                    };
                    let sprites_slot_2 = if !save_chrome_draw_vec.is_empty() {
                        save_chrome_overlay.as_ref()
                    } else if !menu_glyph_draw_vec.is_empty() {
                        menu_glyph_overlay.as_ref()
                    } else {
                        None
                    };
                    // Effect-pool billboards: bridge live effect child sprites
                    // into the renderer as faithful camera-facing quads sized
                    // and UV-addressed from the effect bundle's inline atlas
                    // (`World::active_effect_sprites`). Each draws two ways: a
                    // textured quad sampling the scene VRAM at the sprite's
                    // atlas page/clut/uv (the retail FUN_801E0088 pass-2 path -
                    // invisible while the texel-source upload is unpinned, real
                    // once it lands), plus a tinted outline through the Lines
                    // pipeline so the spawn is visible now. See
                    // docs/subsystems/effect-vm.md.
                    let (effect_billboard, effect_lines) = if self.boot_ui.is_active() {
                        (None, None)
                    } else {
                        let sprites = self.session.host.world.active_effect_sprites();
                        if sprites.is_empty() {
                            (None, None)
                        } else {
                            // Camera right/up in world space (clip-space basis
                            // dirs mapped back through the inverse MVP).
                            let inv = cam.inverse();
                            let right = inv.transform_vector3(Vec3::X).normalize_or_zero();
                            let up = inv.transform_vector3(Vec3::Y).normalize_or_zero();
                            let mesh = effect_billboard_mesh(r, &sprites, right, up);
                            let (pos, col, idx) = effect_sprite_line_geometry(&sprites, right, up);
                            let lines = match r.upload_lines(&pos, &col, &idx) {
                                Ok(m) => Some(m),
                                Err(e) => {
                                    log::warn!("effect outline lines upload: {e:#}");
                                    None
                                }
                            };
                            (mesh, lines)
                        }
                    };
                    if let Some(mesh) = effect_billboard.as_ref() {
                        draws.push(SceneDraw { mesh, mvp: cam });
                    }
                    // World-map overlay lines: a kind-coded upright marker for
                    // each placed entity (portal / NPC / encounter zone, from
                    // `World::world_map_entity_markers`) plus the player marker
                    // (`World::world_map_player_marker`) - the player's own mesh
                    // isn't drawn in world-map mode. Both build into one Lines
                    // mesh, routed through the same overlay slot as the effect
                    // outlines (mutually exclusive: no effects spawn on the
                    // world map). Without this the installed entities + player
                    // carry positions but never appear on screen.
                    let world_map_entity_lines = if in_world_map && !self.boot_ui.is_active() {
                        let markers = self.session.host.world.world_map_entity_markers();
                        let mut pos: Vec<[f32; 3]> = Vec::new();
                        let mut col: Vec<[u8; 4]> = Vec::new();
                        let mut idx: Vec<u32> = Vec::new();
                        if !markers.is_empty() {
                            let (p, c, i) = world_map_entity_line_geometry(
                                &markers,
                                self.scene_aabb.0,
                                self.scene_aabb.1,
                            );
                            pos = p;
                            col = c;
                            idx = i;
                        }
                        if let Some(player) = self.session.host.world.world_map_player_marker() {
                            let (p, c, i) = world_map_player_line_geometry(
                                &player,
                                self.scene_aabb.0,
                                self.scene_aabb.1,
                            );
                            let base = pos.len() as u32;
                            pos.extend(p);
                            col.extend(c);
                            idx.extend(i.into_iter().map(|v| v + base));
                        }
                        // Merge the env-gated slot-4 inspection wireframe (built
                        // once at scene load) into the same overlay-lines buffer.
                        if let Some((p, c, i)) = self.world_map_slot4_lines.as_ref() {
                            let base = pos.len() as u32;
                            pos.extend_from_slice(p);
                            col.extend_from_slice(c);
                            idx.extend(i.iter().map(|v| v + base));
                        }
                        if idx.is_empty() {
                            None
                        } else {
                            match r.upload_lines(&pos, &col, &idx) {
                                Ok(m) => Some(m),
                                Err(e) => {
                                    log::warn!("world-map overlay marker lines upload: {e:#}");
                                    None
                                }
                            }
                        }
                    } else {
                        None
                    };
                    // Effect 3D models (`etmd.dat`): spell effects like Tail
                    // Fire are small Gouraud-shaded `etmd` meshes textured by
                    // the resident `etim` texels, not billboards. Build a
                    // per-frame VRAM mesh + transform for each live effect that
                    // has a model assigned (same Y-flip model-matrix convention
                    // as `actor_model`). Held in a local Vec so the meshes
                    // outlive the render borrow.
                    let mut effect_model_draws: Vec<(UploadedVramMesh, Mat4)> = Vec::new();
                    if !self.boot_ui.is_active() && !in_world_map {
                        for em in self.session.host.world.active_effect_models() {
                            let Some(gtmd) = self
                                .session
                                .host
                                .world
                                .global_tmd(em.tmd_index as i16)
                                .map(std::sync::Arc::clone)
                            else {
                                continue;
                            };
                            let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&gtmd.tmd, &gtmd.raw);
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
                                Ok(m) => {
                                    let model = Mat4::from_translation(Vec3::from(em.world_pos))
                                        * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
                                    effect_model_draws.push((m, model));
                                }
                                Err(e) => log::warn!("effect model mesh upload: {e:#}"),
                            }
                        }
                    }
                    for (mesh, model) in &effect_model_draws {
                        draws.push(SceneDraw {
                            mesh,
                            mvp: cam * *model,
                        });
                    }

                    // Active Seru-magic summon scene-graph (debug-spawned via
                    // `G`): one textured mesh per move-VM-driven part, posed by
                    // the part's interpreted transform (world pos + rotation
                    // banks). The animation computation is faithful (move VM);
                    // the transform composition is the open PROT 0900 piece.
                    let mut summon_part_draws: Vec<(UploadedVramMesh, Mat4)> = Vec::new();
                    if !self.boot_ui.is_active() && !in_world_map {
                        // Summon parts and battle move-FX parts render identically
                        // (move-VM scene-graph parts resolving into the battle
                        // `global_tmd_pool` = PROT 0871 effect library). FIELD
                        // move-VM effects are drawn separately below: their meshes
                        // live in the SCENE's TMD pack, not the battle pool.
                        let part_draws = self
                            .session
                            .host
                            .world
                            .active_summon_part_draws()
                            .into_iter()
                            .chain(self.session.host.world.active_move_fx_part_draws());
                        for sp in part_draws {
                            let Some(gtmd) = self
                                .session
                                .host
                                .world
                                .global_tmd(sp.model_index as i16)
                                .map(std::sync::Arc::clone)
                            else {
                                continue;
                            };
                            let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&gtmd.tmd, &gtmd.raw);
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
                                Ok(m) => {
                                    let model = Mat4::from_translation(Vec3::from(sp.world_pos))
                                        * Mat4::from_rotation_y(sp.rot[1])
                                        * Mat4::from_rotation_x(sp.rot[0])
                                        * Mat4::from_rotation_z(sp.rot[2])
                                        * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
                                    summon_part_draws.push((m, model));
                                }
                                Err(e) => log::warn!("summon/move-FX part mesh upload: {e:#}"),
                            }
                        }
                    }
                    for (mesh, model) in &summon_part_draws {
                        draws.push(SceneDraw {
                            mesh,
                            mvp: cam * *model,
                        });
                    }
                    // Field move-VM effect parts (op 0x34 sub-3 stagers): resolve
                    // each mesh part against the SCENE's TMD pack - `env_tmds` =
                    // `res.tmds` filtered to the scene_asset_table bundle entry,
                    // the same source the field-placement renderer + the
                    // asset-viewer use - NOT the battle `global_tmd_pool`. Retail
                    // resolves a field stager's mesh as `DAT_8007C018[model_sel +
                    // DAT_8007B6F8]`, where `DAT_8007B6F8 = 5` is the character-mesh
                    // prefix and `DAT_8007C018[5..]` is exactly this scene pack; so
                    // the part's relative `model_sel` (spawn base 0, surfaced as
                    // `model_index`) indexes `env_tmds` directly, mirroring how a
                    // placement's `pack_index` does.
                    let mut field_fx_draws: Vec<(UploadedVramMesh, Mat4)> = Vec::new();
                    if !self.boot_ui.is_active() && !in_world_map {
                        for fp in self.session.host.world.active_field_fx_part_draws() {
                            // `model_index` = the stager record's relative
                            // `model_sel` (spawn base 0) → the scene TMD pack
                            // (`field_stager_tmds`, the env_tmds / asset-viewer
                            // source = DAT_8007C018[5 + model_sel]).
                            let Some((tmd, raw)) = self.field_stager_tmds.get(fp.model_index)
                            else {
                                continue;
                            };
                            let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(tmd, raw);
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
                                Ok(m) => {
                                    let model = Mat4::from_translation(Vec3::from(fp.world_pos))
                                        * Mat4::from_rotation_y(fp.rot[1])
                                        * Mat4::from_rotation_x(fp.rot[0])
                                        * Mat4::from_rotation_z(fp.rot[2])
                                        * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
                                    field_fx_draws.push((m, model));
                                }
                                Err(e) => log::warn!("field-FX part mesh upload: {e:#}"),
                            }
                        }
                    }
                    for (mesh, model) in &field_fx_draws {
                        draws.push(SceneDraw {
                            mesh,
                            mvp: cam * *model,
                        });
                    }
                    let scene = RenderScene {
                        vram,
                        draws: &draws,
                        color_draws: &color_draws,
                        overlay_lines: world_map_entity_lines
                            .as_ref()
                            .or(effect_lines.as_ref())
                            .map(|m| (m, cam)),
                        overlay_sprites: sprites_slot_1,
                        overlay_sprites_2: sprites_slot_2,
                        overlay_text: Some(&overlay),
                        clear_color: scene_clear,
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

/// World-unit size of one texel when drawing an effect billboard (the atlas
/// stores sprite extents in texels; the renderer scales them to world units).
const EFFECT_TEXEL_WORLD: f32 = 1.0;

/// The four world-space corners of a camera-facing billboard for `sprite`,
/// using the camera's world `right`/`up` basis. Order: TL, TR, BL, BR.
fn effect_sprite_corners(
    sprite: &legaia_engine_core::world::EffectSprite,
    right: Vec3,
    up: Vec3,
) -> [Vec3; 4] {
    let c = Vec3::from(sprite.world_pos);
    let hw = sprite.size[0] * 0.5 * EFFECT_TEXEL_WORLD;
    let hh = sprite.size[1] * 0.5 * EFFECT_TEXEL_WORLD;
    let rx = right * hw;
    let uy = up * hh;
    [c - rx + uy, c + rx + uy, c - rx - uy, c + rx - uy]
}

/// Build a textured billboard mesh for the live effect sprites: one
/// camera-facing quad per child, sampling the scene VRAM at the sprite's
/// atlas `(u, v)` / `tpage` / `clut`. Mirrors the retail per-frame walker
/// (`FUN_801E0088` pass 2), which emits one GPU sprite primitive per child.
///
/// The texel-source upload for battle effects is not yet pinned, so a quad
/// over empty VRAM samples all-zero texels which the VRAM-mesh shader
/// discards (clean, not garbage); real pixels appear once that upload lands.
/// Returns `None` when there is nothing to draw.
fn effect_billboard_mesh(
    r: &legaia_engine_render::Renderer,
    sprites: &[legaia_engine_core::world::EffectSprite],
    right: Vec3,
    up: Vec3,
) -> Option<UploadedVramMesh> {
    if sprites.is_empty() {
        return None;
    }
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(sprites.len() * 4);
    let mut uvs: Vec<[u8; 2]> = Vec::with_capacity(sprites.len() * 4);
    let mut cba_tsb: Vec<[u16; 2]> = Vec::with_capacity(sprites.len() * 4);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(sprites.len() * 4);
    let mut indices: Vec<u32> = Vec::with_capacity(sprites.len() * 6);
    // Quad faces the camera; a single normal toward the viewer keeps the
    // lambert term stable rather than relying on the derivative fallback.
    let face = right.cross(up).normalize_or_zero().to_array();
    for s in sprites {
        let [u0, v0] = s.uv;
        let u1 = u0.saturating_add(s.uv_size[0].saturating_sub(1)).min(255) as u8;
        let v1 = v0.saturating_add(s.uv_size[1].saturating_sub(1)).min(255) as u8;
        let u0 = (u0 & 0xFF) as u8;
        let v0 = (v0 & 0xFF) as u8;
        let corners = effect_sprite_corners(s, right, up);
        let corner_uv = [[u0, v0], [u1, v0], [u0, v1], [u1, v1]];
        let base = positions.len() as u32;
        for (corner, uv) in corners.iter().zip(corner_uv) {
            positions.push(corner.to_array());
            uvs.push(uv);
            cba_tsb.push([s.clut, s.page]);
            normals.push(face);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    }
    match r.upload_vram_mesh(&positions, &uvs, &cba_tsb, &normals, &indices) {
        Ok(m) => Some(m),
        Err(e) => {
            log::warn!("effect billboard mesh upload: {e:#}");
            None
        }
    }
}

/// Build a tinted outline for each effect billboard through the Lines
/// pipeline (a camera-facing rectangle, sized from the sprite atlas, faded by
/// age). This keeps spawned effects visible while the textured-quad's VRAM
/// source is unpinned - the billboard's geometry and animation are faithful
/// even when its texels are not yet resident.
fn effect_sprite_line_geometry(
    sprites: &[legaia_engine_core::world::EffectSprite],
    right: Vec3,
    up: Vec3,
) -> (Vec<[f32; 3]>, Vec<[u8; 4]>, Vec<u32>) {
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(sprites.len() * 4);
    let mut col: Vec<[u8; 4]> = Vec::with_capacity(sprites.len() * 4);
    let mut idx: Vec<u32> = Vec::with_capacity(sprites.len() * 8);
    for s in sprites {
        let [tl, tr, bl, br] = effect_sprite_corners(s, right, up);
        // Warm spark colour, dimmed as the effect ages toward retirement.
        let fade = (1.0 - s.age01).clamp(0.0, 1.0);
        let c = [
            (80.0 + 175.0 * fade) as u8,
            (200.0 * fade) as u8,
            (255.0 * fade) as u8,
            255,
        ];
        let base = pos.len() as u32;
        for corner in [tl, tr, br, bl] {
            pos.push(corner.to_array());
            col.push(c);
        }
        // Four edges of the rectangle (LineList).
        for &(a, b) in &[(0u32, 1u32), (1, 2), (2, 3), (3, 0)] {
            idx.push(base + a);
            idx.push(base + b);
        }
    }
    (pos, col, idx)
}

/// RGBA colour of a world-map entity marker, keyed by its kind: portals
/// (town/dungeon entrances) cyan, NPCs green, encounter zones warm red.
fn world_map_entity_marker_color(kind: legaia_engine_core::world::WorldMapEntityKind) -> [u8; 4] {
    use legaia_engine_core::world::WorldMapEntityKind as K;
    match kind {
        K::Portal => [0, 200, 255, 255],
        K::Npc => [80, 220, 80, 255],
        K::EncounterZone => [230, 80, 40, 255],
    }
}

/// World-map ocean CLUT animation state. Holds the 13 BGR555 frames (32 bytes
/// each) decoded from the kingdom bundle and the current frame cursor + tick
/// accumulator. Each step overwrites the first 16 CLUT entries at VRAM
/// `(0, 506)` with the next frame, reproducing the retail rolling-wave DMA.
struct OceanAnim {
    /// 13 frames × 32 bytes (16 BGR555 entries each), as decoded by
    /// [`legaia_asset::ocean::find_ocean_assets`].
    frames: Vec<u8>,
    /// Current frame index (`0..frames.len()/32`).
    cur: usize,
    /// Sim-tick accumulator; the frame advances every
    /// [`OCEAN_ANIM_TICKS_PER_FRAME`] ticks.
    tick: u32,
}

/// Sim ticks between ocean-CLUT frame advances. A gentle shimmer: the 13-frame
/// cycle completes in ~1.3 s at 60 Hz. The exact retail DMA cadence isn't
/// pinned, so this is a tuned approximation, not a parity figure.
const OCEAN_ANIM_TICKS_PER_FRAME: u32 = 6;

/// Convert a [`WalkHeightfield`] into a renderer [`VramMesh`]. The heightfield
/// supplies per-vertex UVs (the `+0x14` atlas tile) **and** per-vertex
/// `[clut, tpage]` (the cell's terrain page + palette from `+0x15` /
/// `+0x16..+0x18`), so grass / mountain / water / forest cells each sample their
/// own VRAM page within the single ground mesh. Normals are left at the
/// `[0,0,0]` sentinel so the shader derives screen-space normals (flat-lit).
/// See docs/subsystems/world-map.md "Ground texturing".
fn heightfield_to_vram_mesh(
    hf: &legaia_asset::field_objects::WalkHeightfield,
) -> legaia_tmd::mesh::VramMesh {
    let n = hf.positions.len();
    legaia_tmd::mesh::VramMesh {
        positions: hf.positions.clone(),
        uvs: hf.uvs.clone(),
        // Per-cell terrain page + palette (multi-page terrain atlas).
        cba_tsb: hf.cba_tsb.clone(),
        normals: vec![[0.0, 0.0, 0.0]; n],
        indices: hf.indices.clone(),
    }
}

/// MAN), so they sit correctly relative to the player even while the kingdom
/// terrain mesh renders at its own pack-local coordinates.
fn world_map_entity_line_geometry(
    markers: &[legaia_engine_core::world::WorldMapEntityMarker],
    aabb_lo: [f32; 3],
    aabb_hi: [f32; 3],
) -> (Vec<[f32; 3]>, Vec<[u8; 4]>, Vec<u32>) {
    let diag = (Vec3::from(aabb_hi) - Vec3::from(aabb_lo))
        .length()
        .max(1.0);
    let post_h = diag * 0.06;
    let arm = diag * 0.02;
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(markers.len() * 6);
    let mut col: Vec<[u8; 4]> = Vec::with_capacity(markers.len() * 6);
    let mut idx: Vec<u32> = Vec::with_capacity(markers.len() * 6);
    for m in markers {
        let [x, y, z] = m.world_pos;
        let c = world_map_entity_marker_color(m.kind);
        let base = pos.len() as u32;
        // 0: base, 1: top (up = world -Y under the geometry convention),
        // 2..=5: base-cross arm ends along +/-X and +/-Z.
        let verts = [
            [x, y, z],
            [x, y - post_h, z],
            [x - arm, y, z],
            [x + arm, y, z],
            [x, y, z - arm],
            [x, y, z + arm],
        ];
        for v in verts {
            pos.push(v);
            col.push(c);
        }
        // Vertical post + the two base-cross segments.
        for &(a, b) in &[(0u32, 1u32), (2, 3), (4, 5)] {
            idx.push(base + a);
            idx.push(base + b);
        }
    }
    (pos, col, idx)
}

/// Build a LineList for the overworld player marker: a taller upright post (so
/// the player reads above the kind-coded entity markers), a base cross, and a
/// facing tick pointing in the player's heading. White-yellow, sized relative
/// to the scene AABB. Same Y-flip convention as the entity markers.
fn world_map_player_line_geometry(
    marker: &legaia_engine_core::world::WorldMapPlayerMarker,
    aabb_lo: [f32; 3],
    aabb_hi: [f32; 3],
) -> (Vec<[f32; 3]>, Vec<[u8; 4]>, Vec<u32>) {
    let diag = (Vec3::from(aabb_hi) - Vec3::from(aabb_lo))
        .length()
        .max(1.0);
    let post_h = diag * 0.09;
    let arm = diag * 0.025;
    let tick = diag * 0.05;
    let [x, y, z] = marker.world_pos;
    let c = [255u8, 230, 60, 255];
    // Heading: PSX 12-bit angle, 0 = +Z, quarter turn (1024) = +X.
    let angle = (marker.facing as f32) / 4096.0 * std::f32::consts::TAU;
    let (sin, cos) = angle.sin_cos();
    let verts = [
        [x, y, z],                           // 0 base
        [x, y - post_h, z],                  // 1 top
        [x - arm, y, z],                     // 2 -X arm
        [x + arm, y, z],                     // 3 +X arm
        [x, y, z - arm],                     // 4 -Z arm
        [x, y, z + arm],                     // 5 +Z arm
        [x + sin * tick, y, z + cos * tick], // 6 facing tick end
    ];
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(7);
    let mut col: Vec<[u8; 4]> = Vec::with_capacity(7);
    for v in verts {
        pos.push(v);
        col.push(c);
    }
    // Post + base-cross (X/Z arms) + facing tick.
    let idx = vec![0, 1, 2, 3, 4, 5, 0, 6];
    (pos, col, idx)
}

/// Build a LineList wireframe of a kingdom's decoded slot-4 vertex pool
/// (`SceneResources::world_map_slot4`), as world-space `(positions, colors,
/// indices)`. Each body's records are emitted at their raw object-local
/// coordinates (no per-object placement transform - the cluster-A command
/// stream that supplies those is unpinned), Y-negated to match the
/// heightfield's `cam * yflip` frame. Colour is keyed by body `kind`
/// (`1` = the shared universal mesh set, `2` = kingdom-specific objects,
/// `4` = wide-extent bodies) so the per-kingdom assembly structure reads
/// at a glance. Returns empty geometry when no body yields a segment.
///
/// This is an env-gated inspection overlay (`LEGAIA_WORLDMAP_SLOT4=1`); the
/// group-polyline segment topology is the documented inspection convention,
/// not the faithful triangle topology (see
/// `legaia_asset::world_map_overlay::wireframe_segments_3d`).
fn world_map_slot4_line_geometry(
    slot: &legaia_asset::world_map_overlay::KingdomSlot4,
) -> LineGeometry {
    let opts = legaia_asset::world_map_overlay::WireframeOptions::default();
    let segs = legaia_asset::world_map_overlay::wireframe_segments_3d(slot, &opts);
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(segs.len() * 2);
    let mut col: Vec<[u8; 4]> = Vec::with_capacity(segs.len() * 2);
    let mut idx: Vec<u32> = Vec::with_capacity(segs.len() * 2);
    for s in &segs {
        let c = match s.kind {
            1 => [120u8, 200, 255, 255], // shared universal bodies (cyan)
            2 => [255u8, 160, 90, 255],  // kingdom-specific objects (orange)
            4 => [200u8, 120, 255, 255], // wide-extent bodies (violet)
            _ => [180u8, 180, 180, 255],
        };
        let base = pos.len() as u32;
        for v in [s.a, s.b] {
            pos.push([v[0] as f32, -(v[1] as f32), v[2] as f32]);
            col.push(c);
        }
        idx.push(base);
        idx.push(base + 1);
    }
    (pos, col, idx)
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

/// Parse a GameShark `.gs.txt` or Mednafen `.cht` cheat file and
/// apply every entry to `world` through the
/// [`legaia_engine_core::cheat_applier`] registry. Logs per-entry
/// status to stderr.
fn apply_cheat_file(
    world: &mut legaia_engine_core::world::World,
    path: &Path,
    strict: bool,
) -> Result<()> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut db = if path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("cht"))
        .unwrap_or(false)
    {
        legaia_cheats::parse_mednafen_cht(&text)?
    } else {
        legaia_cheats::parse_gs_text(&text)?
    };
    db.dedupe_identical();
    let opts = legaia_engine_core::cheat_applier::ApplyOptions {
        execute_conditionals: !strict,
        skip_unmapped: false,
    };
    let report = legaia_engine_core::cheat_applier::apply(world, &db, opts);
    eprintln!(
        "Cheat report ({} entries, {} writes; {} applied, {} unmapped, {} unknown):",
        report.per_entry.len(),
        report.total_writes,
        report.applied,
        report.unmapped,
        report.unknown_addresses
    );
    for entry in &report.per_entry {
        let total = entry.applied + entry.skipped;
        let tag = if entry.applied == total {
            "ok  "
        } else if entry.applied == 0 {
            "skip"
        } else {
            "part"
        };
        eprintln!(
            "  {tag}  {:.<60} {}/{} writes",
            entry.description, entry.applied, total
        );
    }
    Ok(())
}

/// Parse a `--party` composition spec: comma-separated character names
/// (case-insensitive `vahn`/`noa`/`gala`/`terra`) or 0-based roster
/// indices, in battle order.
fn parse_party_spec(spec: &str) -> Result<Vec<u8>> {
    spec.split(',')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|t| match t.to_ascii_lowercase().as_str() {
            "vahn" => Ok(0u8),
            "noa" => Ok(1),
            "gala" => Ok(2),
            "terra" => Ok(3),
            other => other.parse::<u8>().map_err(|_| {
                anyhow::anyhow!(
                    "unknown party member '{t}' (use vahn/noa/gala/terra or a roster index)"
                )
            }),
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_play_window(
    scene: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    enable_audio: bool,
    world_map: bool,
    str_file: Option<&Path>,
    boot_ui: bool,
    save_dir: &Path,
    cutscene_map_path: Option<&Path>,
    cheat_file: Option<&Path>,
    cheat_strict: bool,
    live_loop: bool,
    player_battle: bool,
    party: Option<&str>,
    vm_dialogue: bool,
    terrain_y: bool,
    edge_collision: bool,
    solid_npcs: bool,
    live_npcs: bool,
    damage_finish: bool,
    battle_bgm: Option<u16>,
) -> Result<()> {
    cmd_play_window_with_record(
        scene,
        extracted_root,
        disc,
        enable_audio,
        world_map,
        str_file,
        boot_ui,
        save_dir,
        cutscene_map_path,
        cheat_file,
        cheat_strict,
        live_loop,
        player_battle,
        party,
        vm_dialogue,
        terrain_y,
        edge_collision,
        solid_npcs,
        live_npcs,
        damage_finish,
        battle_bgm,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn cmd_play_window_with_record(
    scene: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    enable_audio: bool,
    world_map: bool,
    str_file: Option<&Path>,
    boot_ui: bool,
    save_dir: &Path,
    cutscene_map_path: Option<&Path>,
    cheat_file: Option<&Path>,
    cheat_strict: bool,
    live_loop: bool,
    player_battle: bool,
    party: Option<&str>,
    vm_dialogue: bool,
    terrain_y: bool,
    edge_collision: bool,
    solid_npcs: bool,
    live_npcs: bool,
    damage_finish: bool,
    battle_bgm: Option<u16>,
    record_to: Option<RecordTarget>,
) -> Result<()> {
    // Optional cutscene-map override layered on top of the heuristic.
    let cutscene_map = if let Some(p) = cutscene_map_path {
        legaia_engine_core::scene::CutsceneMap::from_toml_path(p)
            .with_context(|| format!("load cutscene map {}", p.display()))?
    } else {
        legaia_engine_core::scene::CutsceneMap::default()
    };
    if cutscene_map_path.is_some() {
        eprintln!(
            "info: cutscene-map loaded with {} explicit entry/entries",
            cutscene_map.len()
        );
    }
    // Auto-resolve op*/ed* cutscene scenes to their MV*.STR file when
    // the user didn't pass --str-file but the extracted root has the
    // file on disk. Mirrors the same convenience path in cmd_play.
    let auto_str = match (str_file, disc) {
        (Some(_), _) => None,
        (None, None) => cutscene_map
            .resolve(scene)
            .map(|rel| extracted_root.join(rel))
            .filter(|p| p.exists()),
        (None, Some(_)) => None,
    };
    let resolved_str: Option<&Path> = str_file.or(auto_str.as_deref());
    // Phase 1: if a STR file is provided (or auto-resolved), play the
    // video in a window first. The user closes (or ESC) the STR window,
    // then the scene window opens.
    //
    // When booting from a disc image we can resolve the scene's movie inside
    // the ISO and play it with its interleaved XA audio (read raw 2352-byte
    // sectors). Otherwise we fall back to the filesystem (video only).
    if let Some(str_path) = resolved_str {
        cmd_play_str(str_path, None, 640, 480)?;
    } else if let (Some(disc_path), None) = (disc, str_file) {
        // Disc mode, no explicit file: resolve the scene's MV*.STR via the
        // cutscene map / heuristic and play it from the disc with audio.
        if let Some(rel) = cutscene_map.resolve(scene) {
            let iso_path = Path::new(&rel);
            match cmd_play_str(iso_path, Some(disc_path), 640, 480) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("info: scene '{scene}' STR '{rel}' not played from disc ({e:#})")
                }
            }
        }
    }

    let cfg = BootConfig {
        scene: scene.to_string(),
        enable_audio,
    };
    let mut session = match disc {
        Some(disc_path) => BootSession::open_disc(disc_path, &cfg)?,
        None => BootSession::open(extracted_root, &cfg)?,
    };
    // Drive field dialogue through the inline-script field-VM runner so branch
    // handlers execute (flag-sets / scene-changes / GIVE_ITEM). On by default;
    // `--simple-dialogue` clears it to fall back to the plain typewriter panel.
    session.host.world.use_vm_dialogue = vm_dialogue;
    // Opt-in: snap the player's Y to the per-scene floor height each
    // locomotion step. Off by default → flat-Y behaviour preserved.
    session.host.world.follow_terrain_height = terrain_y;
    // Opt-in: retail's three-probe leading-edge wall footprint (the
    // `DAT_801f2214` standoff). Off by default → candidate-centre test.
    session.host.world.leading_edge_wall_probes = edge_collision;
    session.host.world.solid_field_npcs = solid_npcs;
    // Opt-in: walk field NPCs along their MAN-authored routes through the
    // motion VM. Off by default -> NPCs rest at their placement anchors.
    session.host.world.animate_field_npcs = live_npcs;
    // Opt-in: route live basic-attack damage through the retail damage
    // finisher (9999 cap + no-damage floor). Off by default → flat path.
    session.host.world.use_damage_finish = damage_finish;
    // Opt-in, NON-FAITHFUL QoL: redirect a monster's single-target attack to
    // the lowest-HP living party member (the faithful default is a uniform
    // random target). Enable with `LEGAIA_SMART_MONSTERS=1`. The RNG stream is
    // unchanged, so determinism within a run is preserved.
    session.host.world.smarter_monster_targeting =
        std::env::var_os("LEGAIA_SMART_MONSTERS").is_some();
    // Field-live arming, built once and reused: at startup for the direct path
    // and later by the boot-UI NEW GAME handler when it enters `opdeene`.
    let field_live_opts = legaia_engine_shell::boot::FieldLiveOpts {
        live_loop,
        player_battle,
        battle_bgm,
    };
    if world_map {
        // Load the scene's resources, route its region-keyed encounter table
        // onto the overworld, install the player, and enter world-map mode
        // (camera controller included). World::tick drives locomotion + the
        // per-tile encounter roll from the pad routed via world.set_pad.
        match session.enter_world_map_live(scene, &field_live_opts) {
            Ok(mode) => {
                log::info!("play-window: entered world-map scene '{scene}' (mode={mode:?})")
            }
            Err(e) => log::warn!("play-window: enter_world_map_live('{scene}') failed: {e:#}"),
        }
        // Start in walk mode so the d-pad walks the overworld player (and the
        // per-tile encounter roll fires). The top-view debug camera (orbit /
        // zoom / pan) stays reachable via the toggle combo (debug_enabled).
        if let Some(ctrl) = session.host.world.world_map_ctrl.as_mut() {
            ctrl.debug_enabled = true;
            ctrl.view_mode = 0;
        }
    }
    if !world_map {
        // Drop into the live field scene (run record 0, install the encounter
        // table, arm the live loop). Shared with the v0.1 oracle + headless
        // drivers via `BootSession::enter_field_live`.
        let opts = field_live_opts.clone();
        match session.enter_field_live(scene, &opts) {
            Ok(mode) => log::info!("play-window: entered field scene '{scene}' (mode={mode:?})"),
            Err(e) => log::warn!("play-window: enter_field_live('{scene}') failed: {e:#}"),
        }
    }

    // Play-window demo seeding (NOT part of the shared field-live core): when
    // a player-driven battle is requested but the boot save carries no items /
    // saved chains, seed a couple so the Item / Arts submenus are exercisable
    // by hand. No-ops when the save already has inventory / chains.
    if player_battle {
        let world = &mut session.host.world;
        if world.inventory.is_empty() {
            world.inventory.insert(0x01, 5); // Healing Leaf
            world.inventory.insert(0x13, 3); // Bomb (offensive)
        }
        if world.saved_chains.is_empty() {
            use legaia_save::SavedChainRecord;
            for slot in 0u8..3 {
                world.saved_chains.push(SavedChainRecord {
                    char_slot: slot,
                    name: "Quick".into(),
                    sequence: vec![1, 2],
                });
                world.saved_chains.push(SavedChainRecord {
                    char_slot: slot,
                    name: "Combo".into(),
                    sequence: vec![1, 2, 3, 4],
                });
            }
            // Stage a demo art record per character so the "Combo" chain
            // (it ends in Up) resolves through the real art-power path -
            // two damage strikes that burn the target. "Quick" has no
            // matching record and falls back to the synthetic profile.
            use legaia_art::power::PowerByte;
            use legaia_art::queue::{ActionConstant, Command};
            use legaia_art::record::EnemyEffect;
            for character in legaia_art::Character::all() {
                world.set_art_record(
                    character,
                    ActionConstant::Art1B,
                    legaia_art::ArtRecord {
                        action: ActionConstant::Art1B,
                        commands: vec![Command::Up],
                        anim_index: 0,
                        anim_extra: vec![],
                        name: None,
                        power: vec![PowerByte::from_byte(0x18), PowerByte::from_byte(0x1D)],
                        dmg_timing: vec![],
                        effect_cues: Default::default(),
                        hit_cues: vec![],
                        identifier: 0,
                        anim_speed: 0,
                        enemy_effect: EnemyEffect::Toxic,
                        repeat_frames: Default::default(),
                        background: 0,
                        runtime_address: None,
                    },
                );
            }
        }
    }

    // Apply the cheat file (if any) to the live World before building
    // scene resources. The applier mutates `world.roster` /
    // `world.money` / `world.play_time_seconds` etc. through the
    // ram_map registry.
    if let Some(path) = cheat_file {
        apply_cheat_file(&mut session.host.world, path, cheat_strict)?;
    }

    // Install the requested present-party composition (after the roster +
    // cheats have settled, so the actor reseed reads final records). The
    // flag overrides whatever composition the boot save carried.
    if let Some(spec) = party {
        let slots = parse_party_spec(spec)?;
        let world = &mut session.host.world;
        world.set_active_party(slots.clone());
        if world.active_party.len() != slots.len() {
            log::warn!(
                "play-window: --party {spec}: kept the first {} of {slots:?} \
                 (3 on-screen positions)",
                world.active_party.len()
            );
        }
        log::info!(
            "play-window: present party = {:?} (roster slots, battle order)",
            world.active_party
        );
    }

    let scene_res = {
        let s = session
            .host
            .scene
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no scene loaded after BootSession::open"))?;
        // Load the shared blocks (`init_data` + `player_data`) so the
        // player TMD + shared UI atlas stay resident across field
        // transitions, then build with the targeted VRAM-upload
        // heuristic. Without this every prim sampled non-uploaded
        // VRAM regions and the filter dropped 100% of the mesh.
        let mut shared_scenes: Vec<Scene> = Vec::new();
        for name in FIELD_SHARED_BLOCKS {
            match Scene::load(&session.host.index, name) {
                Ok(sc) => shared_scenes.push(sc),
                Err(e) => log::warn!("play-window: shared block '{name}' not loaded: {e:#}"),
            }
        }
        let shared_refs: Vec<&Scene> = shared_scenes.iter().collect();
        // Field-load model (matches retail FUN_8001F7C0 + the engine's
        // `enter_field_scene`): `SceneLoadKind::Field` skips the battle-
        // character `scene_tmd_stream` meshes, and the TMD scan now pulls
        // the town's environment geometry out of the scene_asset_table's
        // LZS-packed mesh pack (previously invisible to the raw scanner,
        // which left the field with a single stray battle mesh). Upload
        // every TIM, as retail's field loader DMAs the whole atlas - the
        // town meshes sample texture pages across all of VRAM, so a
        // render-targeted upload drops most of their prims.
        // World-map scenes (`map\d\d`) draw the kingdom-bundle slot-1
        // landmark pack, not the generic field sweep. Mirror the host's
        // `enter_field_scene` kind selection so the rendered meshes match the
        // gameplay-side resources (otherwise the window draws the Field-mode
        // 2-mesh fallback while the host loaded the full 40-TMD pack).
        let load_kind = if legaia_engine_core::scene::is_world_map_scene(scene) {
            SceneLoadKind::WorldMap
        } else {
            SceneLoadKind::Field
        };
        let (mut res, _stats) = SceneResources::build_targeted_with_options(
            s,
            &shared_refs,
            BuildOptions {
                kind: load_kind,
                upload_all_tims: true,
            },
        )?;
        // Field-character atlas upload (PROT 0874 §2, the `FUN_800198e0`
        // chain): entries 1/2/3 are the Vahn/Noa/Gala atlas pages whose
        // palettes live as **flat strips** on CLUT row 478. The generic TIM
        // scan uploads the pages but places these CLUTs as declared rects
        // (rows 478..481 col 0), so the meshes sample an unpopulated row and
        // the VRAM filter drops them - the invisible-player symptom. Retail
        // field load uploads the pack with strip semantics; replicate that.
        // (NOT the etim effect pool - uploading that battle-resident pool
        // into field VRAM clobbers pages the town meshes sample.)
        match session
            .host
            .index
            .entry_bytes(legaia_asset::field_char_textures::PROT_ENTRY_INDEX)
            .and_then(|b| legaia_asset::field_char_textures::parse(&b))
        {
            Ok(mut pack) => {
                // Entries 1/2/3 only (the character atlas pages). The pack's
                // other entries land on pages the town env meshes sample -
                // uploading them here drops ~26 scene meshes to the filter.
                pack.textures.retain(|t| (1..=3).contains(&t.index));
                pack.upload_to_vram(&mut res.vram, false);
                log::info!(
                    "play-window: field char atlas uploaded ({} TIMs, strip CLUTs)",
                    pack.textures.len()
                );
            }
            Err(err) => {
                log::warn!("play-window: field char atlas upload skipped: {err:#}");
            }
        }
        // Shared interior page (texpage (960,256) + the flat 256-entry strip
        // CLUT on row 510): resident in retail VRAM from the opening onward,
        // sampled by town env meshes (23 town01 tile instances incl. the
        // spawn plaza). It lives in PROT.DAT's unindexed head gap (before
        // the first TOC entry's data), so no per-entry read can source it.
        match legaia_asset::interior_page::read_from_prot_dat(&extracted_root.join("PROT.DAT")) {
            Ok(tim) => {
                legaia_asset::interior_page::upload_to_vram(&tim, &mut res.vram);
                log::info!("play-window: shared interior page uploaded (row-510 strip CLUT)");
            }
            Err(err) => {
                log::warn!("play-window: shared interior page skipped: {err:#}");
            }
        }
        res
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
    // Try to decode the publisher logos from PROT 0895 (init.pak) up
    // front. Falls back silently when the disc isn't loaded or the
    // entry doesn't parse - retail discs always have it.
    let publisher_logos_atlas_data = match session
        .host
        .index
        .entry_bytes(legaia_asset::init_pak::PROT_INDEX as u32)
    {
        Ok(b) => match legaia_engine_core::publisher_logos::build_atlas_from_init_pak(&b) {
            Ok(a) => {
                log::info!(
                    "play-window: publisher-logos atlas built ({}x{}, {} logos)",
                    a.width,
                    a.height,
                    a.rects.len()
                );
                Some(a)
            }
            Err(e) => {
                log::warn!("play-window: publisher-logos build failed: {e:#}");
                None
            }
        },
        Err(e) => {
            log::warn!("play-window: PROT 0895 read failed: {e:#}");
            None
        }
    };

    // Try to decode the title-screen TIM from PROT 0888 (`sound_data2`
    // per CDNAME, actually carries title art) up front. Falls back
    // silently when the disc isn't loaded or the entry doesn't parse -
    // retail discs always have it.
    let title_screen_atlas_data = match session
        .host
        .index
        .entry_bytes(legaia_asset::title_pak::PROT_INDEX_PRIMARY as u32)
    {
        Ok(b) => match legaia_engine_core::title_screen_atlas::build_atlas_from_prot_888(
            &b,
            legaia_asset::title_pak::TITLE_TIM_OFFSET,
        ) {
            Ok(a) => {
                log::info!(
                    "play-window: title-screen atlas built ({}x{})",
                    a.width,
                    a.height
                );
                Some(a)
            }
            Err(e) => {
                log::warn!("play-window: title-screen build failed: {e:#}");
                None
            }
        },
        Err(e) => {
            log::warn!("play-window: PROT 0888 read failed: {e:#}");
            None
        }
    };

    // Try to decode the menu-glyph atlas from the unindexed pre-init_data
    // gap in `PROT.DAT` (offset `0x11218`). Carries the small-caps font
    // retail samples for "NEW GAME" / "CONTINUE" menu rows. The
    // per-entry extractor never visits this gap, so we read PROT.DAT
    // raw bytes - see `legaia_asset::menu_glyph_atlas`.
    let menu_glyph_atlas_data = match session.host.index.prot_dat_raw_bytes(
        legaia_asset::menu_glyph_atlas::PROT_DAT_OFFSET,
        legaia_asset::menu_glyph_atlas::TIM_SIZE,
    ) {
        Ok(b) => match legaia_engine_core::menu_glyph_atlas::build_atlas_from_prot_dat_slice(&b) {
            Ok(a) => {
                log::info!(
                    "play-window: menu-glyph atlas built ({}x{})",
                    a.width,
                    a.height
                );
                Some(a)
            }
            Err(e) => {
                log::warn!("play-window: menu-glyph build failed: {e:#}");
                None
            }
        },
        Err(e) => {
            log::warn!("play-window: PROT.DAT raw read failed: {e:#}");
            None
        }
    };

    // Try to decode the save-menu UI atlas. Needs TWO disc sources:
    //   1. PROT 0899's extended footprint @ `OVERLAY_SAVE_MENU_TIM_OFFSET`
    //      carries the SLOT 1 / SLOT 2 pill sprites (CLUT 7).
    //   2. Raw PROT.DAT @ `OVERLAY_SYSTEM_UI_TIM_OFFSET = 0x018E0`
    //      carries the 9-slice panel chrome (CLUT row 2).
    // The atlas builder composites both into one 256x256 RGBA atlas;
    // see `crates/engine-core/src/save_menu_atlas.rs`. The 9-slice
    // tile geometry was pinned via `scripts/pcsx-redux/scan_panel_prims.py`
    // against sstate9's RAM dump - every primitive's source u/v + CLUT
    // is byte-pinned to the retail render.
    let save_menu_atlas_data = match (
        session
            .host
            .index
            .entry_bytes_extended(legaia_asset::title_pak::PROT_INDEX_OVERLAY as u32),
        // Pull a slice that covers BOTH the system-UI sheet (panel
        // chrome, cursor) AND the load-screen portrait + frame TIMs
        // (`OVERLAY_LOAD_PORTRAIT_TIM_OFFSET`..end of
        // `OVERLAY_LOAD_EMPTY_FRAME_TIM`). The slice starts at the
        // system-UI TIM header so existing offsets stay
        // slice-relative; `build_atlas` handles both shapes.
        {
            let base = legaia_asset::title_pak::OVERLAY_SYSTEM_UI_TIM_OFFSET;
            let end = legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_OFFSET
                + legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_SIZE;
            session
                .host
                .index
                .prot_dat_raw_bytes(base as u64, end - base)
        },
    ) {
        (Ok(pill_bytes), Ok(panel_bytes)) => {
            match legaia_engine_core::save_menu_atlas::build_atlas(&panel_bytes, &pill_bytes) {
                Ok(a) => {
                    log::info!(
                        "play-window: save-menu atlas built ({}x{}) - 9-slice from PROT.DAT[0x018E0] + pills from PROT 0899",
                        a.width,
                        a.height
                    );
                    Some(a)
                }
                Err(e) => {
                    log::warn!("play-window: save-menu build failed: {e:#}");
                    None
                }
            }
        }
        (Err(e), _) => {
            log::warn!("play-window: PROT 0899 read failed: {e:#}");
            None
        }
        (_, Err(e)) => {
            log::warn!("play-window: PROT.DAT raw read failed: {e:#}");
            None
        }
    };

    let initial_boot_ui = if boot_ui {
        if publisher_logos_atlas_data.is_some() {
            BootUiState::PublisherLogos(
                legaia_engine_core::publisher_logos::PublisherLogosSession::new(),
            )
        } else {
            let snapshots = scan_save_dir(save_dir);
            let any_present = snapshots.iter().any(|s| s.present);
            if any_present {
                BootUiState::Title(legaia_engine_core::title::TitleSession::new())
            } else {
                BootUiState::Title(legaia_engine_core::title::TitleSession::without_save_data())
            }
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
        publisher_logos: None,
        pending_publisher_logos_atlas: publisher_logos_atlas_data,
        title_screen: None,
        pending_title_screen_atlas: title_screen_atlas_data,
        menu_glyphs: None,
        pending_menu_glyph_atlas: menu_glyph_atlas_data,
        save_menu: None,
        pending_save_menu_atlas: save_menu_atlas_data,
        uploaded_vram: None,
        meshes: Vec::new(),
        scene_tmd_data: Vec::new(),
        field_placement_draws: Vec::new(),
        field_stager_tmds: Vec::new(),
        color_meshes: Vec::new(),
        field_placement_color_draws: Vec::new(),
        field_terrain_draws: Vec::new(),
        field_terrain_color_draws: Vec::new(),
        world_map_terrain_draws: Vec::new(),
        ground_heightfield: None,
        // Headless capture harnesses can't press `C`; let them start on the
        // wide debug vantage via the env switch.
        field_debug_camera: std::env::var_os("LEGAIA_FIELD_DEBUG_CAM").is_some(),
        world_map_slot4_lines: None,
        ocean_anim: None,
        cpu_vram_base: None,
        battle_vram: None,
        battle_vram_generation: None,
        battle_tex_slots_used: 0,
        battle_faces: Vec::new(),
        face_tables: None,
        art_mouth_tables: None,
        face_tables_attempted: false,
        summon_actor_slot: None,
        battle_stage_mesh: None,
        battle_ground_mesh: None,
        prev_scene_mode: None,
        monster_archive: None,
        battle_mesh_base: 0,
        scene_aabb: ([f32::NEG_INFINITY; 3], [f32::INFINITY; 3]),
        pad: 0,
        mapping,
        menu_runtime: MenuRuntime::new(save_dir.to_path_buf()),
        prev_pad: 0,
        battle_event_log: std::collections::VecDeque::new(),
        battle_hud: legaia_engine_core::battle_hud::BattleHud::new(),
        pending_dynamic_mesh_slots: Vec::new(),
        drained_spawn_slots: std::collections::HashSet::new(),
        player_color_draw: None,
        boot_ui: initial_boot_ui,
        save_dir: save_dir.to_path_buf(),
        options_state: legaia_engine_core::options::OptionsState::default(),
        record_log: record_to.map(RecordLog::from_target),
        field_live_opts,
        // In-flow cutscene STR resolves from the extracted root (video only)
        // or, when booting from a disc image, straight from the ISO with its
        // interleaved XA audio. Exactly one of these is set.
        extracted_root: disc.map_or_else(|| Some(extracted_root.to_path_buf()), |_| None),
        disc_path: disc.map(|d| d.to_path_buf()),
        cutscene: None,
        cutscene_cam_interp: legaia_engine_render::window::CutsceneCameraInterp::new(),
        active_dialog: None,
        seru_names: None,
    };

    let event_loop = EventLoop::new().context("create event loop")?;
    event_loop.run_app(&mut app).context("event loop")?;
    // After the event loop returns, flush any pending record log. The
    // Escape / CloseRequested handlers also flush proactively so a
    // mid-run crash still produces a partial replay file - the trailing
    // flush is the safety net.
    if let Some(log) = app.record_log.as_mut()
        && let Err(e) = log.flush()
    {
        log::error!("record: flush on exit failed: {e:#}");
    }
    Ok(())
}

/// Walk `save_dir` and build per-slot `SlotSnapshot` entries from any
/// LGSF v1 / v2 files found there. Empty slots produce
/// `SlotSnapshot::empty(slot)`. Up to 8 slots are scanned (the retail
/// PSX memory card supports 15 blocks; engines wishing to scan more can
/// drive their own scanner and feed the result into `SaveSelectSession`).
/// Pluck the lead-character roster index out of a [`SlotSnapshot`] for
/// the load-screen portrait grid. The snapshot already exposes the
/// leader's char_id (scan_save_dir picks it from the parsed
/// [`legaia_save::SaveFile`]); this thin helper exists so render-time
/// call sites read clearly.
fn slot_leader_char_id(snap: &legaia_engine_core::save_select::SlotSnapshot) -> u8 {
    snap.leader_char_id
}

/// Build a per-frame [`legaia_engine_render::SlotInfoView`] for the
/// info panel shown at the bottom of the slot-preview screen.
/// Returns `None` for empty slots (the info panel renders only when
/// a save is present).
fn build_slot_info_view(
    slots: &[legaia_engine_core::save_select::SlotSnapshot],
    cursor_slot: u8,
) -> Option<SlotInfoOwned> {
    let snap = slots.get(cursor_slot as usize)?;
    if !snap.present {
        return None;
    }
    Some(SlotInfoOwned {
        slot_no: snap.slot.saturating_add(1),
        location: snap.location.clone(),
        play_time: snap.play_time_string(),
        leader_name: snap.leader_name.clone(),
        leader_level: snap.party_lv,
        leader_hp: snap.leader_hp,
        leader_mp: snap.leader_mp,
        leader_char_id: snap.leader_char_id,
    })
}

/// Compute the slide-in y-offset (delta from parked y) for the
/// bottom info panel. Mirrors retail FUN_801E08D8's inline
/// `local_34 = (anim_t * -0x100) / 0xFFF >> 12 + 0x18A`: the panel
/// slides from `INFO_PANEL_OFFSCREEN_Y = 394` (off-screen below) up
/// to `INFO_PANEL_PARKED_Y = 138` (parked under load chrome) as
/// `info_panel_slide_anim_t` ramps 0 → 4096. Returns the delta from
/// parked y, so 0 = fully landed.
fn info_panel_slide_offset(session: &legaia_engine_core::save_select::SaveSelectSession) -> i32 {
    let (_, y) = legaia_engine_core::save_select::interpolate_anim(
        (0, legaia_engine_core::save_select::INFO_PANEL_OFFSCREEN_Y),
        (0, legaia_engine_core::save_select::INFO_PANEL_PARKED_Y),
        session.info_panel_slide_anim_t(),
    );
    y - legaia_engine_core::save_select::INFO_PANEL_PARKED_Y
}

/// Owned-string flavour of [`legaia_engine_render::SlotInfoView`] used
/// to keep the strings alive across the render call. The borrowed
/// view referenced by the renderer is taken via [`Self::as_view`].
struct SlotInfoOwned {
    slot_no: u8,
    location: String,
    play_time: String,
    leader_name: String,
    leader_level: u8,
    leader_hp: (u16, u16),
    leader_mp: (u16, u16),
    leader_char_id: u8,
}

impl SlotInfoOwned {
    fn as_view(&self) -> legaia_engine_render::SlotInfoView<'_> {
        legaia_engine_render::SlotInfoView {
            slot_no: self.slot_no,
            location: &self.location,
            play_time: &self.play_time,
            leader_name: &self.leader_name,
            leader_level: self.leader_level,
            leader_hp: self.leader_hp,
            leader_mp: self.leader_mp,
            leader_char_id: self.leader_char_id,
        }
    }
}

fn scan_save_dir(save_dir: &Path) -> Vec<legaia_engine_core::save_select::SlotSnapshot> {
    use legaia_engine_core::menu_runtime::SAVE_EXT;
    use legaia_engine_core::save_select::SlotSnapshot;
    // Scan up to 15 slots (one per retail PSX memory-card block) so
    // the load-screen 5×3 grid can render every potential slot.
    const MAX_SLOTS: u8 = 15;
    let mut out = Vec::with_capacity(MAX_SLOTS as usize);
    for slot in 0..MAX_SLOTS {
        // Saves are written by the field menu via `MenuRuntime` as
        // `<dir>/slot_NN.<SAVE_EXT>` (zero-padded slot, see
        // `menu_runtime::slot_path`). The title-screen and
        // save-select scanners must use the same shape; an earlier
        // mismatch (`slot_N.lgsf`) made every save invisible at boot,
        // greying out Continue even with valid saves on disk.
        let path = save_dir.join(format!("slot_{slot:02}.{SAVE_EXT}"));
        let snap = match std::fs::read(&path).ok().and_then(|b| {
            legaia_save::SaveFile::parse(&b)
                .ok()
                .map(|sf| (b.len(), sf))
        }) {
            Some((_, sf)) => {
                // Read the cumulative XP value from the active-party
                // leader's record (`+0x04..+0x06`, pinned by the captured
                // level-up observation triplet) and infer the level from
                // the retail XP table.
                // Engines that capture the actual level byte can override.
                let leader = sf.party.members.first();
                let lv = leader
                    .map(|r| legaia_save::level_for_cumulative_xp(r.cumulative_xp() as u32))
                    .unwrap_or(1);
                let leader_hp = leader
                    .map(|r| {
                        let v = r.hp_mp_sp();
                        (v.hp_cur, v.hp_max)
                    })
                    .unwrap_or((0, 0));
                let leader_mp = leader
                    .map(|r| {
                        let v = r.hp_mp_sp();
                        (v.mp_cur, v.mp_max)
                    })
                    .unwrap_or((0, 0));
                // Retail saves serialise the scene name into the SC
                // block (`+0x200..0x208`, ASCII null-padded). Our LGSF
                // saves don't carry that field yet, so default to the
                // most-common starting kingdom; engines that capture
                // it can override.
                let _ = sf.ext_v2.active_party.is_empty(); // kept-for-future-use
                let location = "Drake Kingdom".to_string();
                SlotSnapshot {
                    slot,
                    present: true,
                    label: format!("Slot {slot}"),
                    play_time_seconds: sf.ext_v2.play_time_seconds,
                    party_lv: lv,
                    location,
                    money: sf.ext.money.max(0) as u32,
                    // Lead char is always Vahn (char_id=0) in retail
                    // Legaia - Vahn is the protagonist and slot 0 of
                    // the SC character record array.
                    leader_char_id: 0,
                    leader_name: "Vahn".to_string(),
                    leader_hp,
                    leader_mp,
                }
            }
            None => SlotSnapshot::empty(slot),
        };
        out.push(snap);
    }
    out
}

pub(crate) fn cmd_play_str(
    str_file: &Path,
    disc: Option<&Path>,
    _win_width: u32,
    _win_height: u32,
) -> Result<()> {
    use legaia_engine_shell::cutscene_av::{
        CutsceneAudio, decode_str_av_from_disc, decode_str_video_only,
    };

    // With a disc image the STR is read as raw 2352-byte sectors so its
    // interleaved XA audio track comes along; without one we play the
    // (video-only) Form-1 extract from the filesystem.
    let (frames, timing, audio): (_, _, Option<CutsceneAudio>) = if let Some(disc_path) = disc {
        let (lba, size) = resolve_iso_file(disc_path, str_file)?;
        let count = size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
        let av = decode_str_av_from_disc(disc_path, lba, count)
            .with_context(|| format!("decode STR {} from disc", str_file.display()))?;
        (av.frames, av.timing, av.audio)
    } else {
        let (f, t) = decode_str_video_only(str_file)?;
        (f, t, None)
    };
    if frames.is_empty() {
        anyhow::bail!("no video frames found in {}", str_file.display());
    }
    println!(
        "play-str: {} frames, {}×{}, {:.2} fps, audio: {}",
        frames.len(),
        frames[0].width,
        frames[0].height,
        timing.fps,
        match &audio {
            Some(a) => format!(
                "{:.1} kHz {} ({:.1}s)",
                a.sample_rate as f64 / 1000.0,
                if matches!(a.channels, legaia_xa::Channels::Stereo) {
                    "stereo"
                } else {
                    "mono"
                },
                a.duration_secs()
            ),
            None => "none".into(),
        }
    );

    // Open the audio device only when there is a track to play. A device
    // failure (CI / headless) degrades to wall-clock-paced video, not an error.
    let audio_out = if audio.is_some() {
        match legaia_engine_audio::AudioOut::new() {
            Ok(a) => Some(a),
            Err(e) => {
                log::warn!("play-str: audio device unavailable ({e:#}); playing video only");
                None
            }
        }
    } else {
        None
    };

    let mut app = StrPlayerApp {
        win: EngineWindow::new(),
        frames,
        uploaded: None,
        frame_period: timing.frame_period(),
        clock: None,
        audio_out,
        pending_audio: audio,
    };
    let event_loop = EventLoop::new().context("create event loop")?;
    event_loop.run_app(&mut app).context("event loop")?;
    Ok(())
}

/// Resolve an ISO9660 path inside a disc image to its `(lba, size)`. Matches
/// case-insensitively and tolerates a leading slash. Errors if not found.
fn resolve_iso_file(disc_path: &Path, iso_path: &Path) -> Result<(u32, u32)> {
    use legaia_iso::iso9660;
    let want = iso_path
        .to_string_lossy()
        .trim_start_matches('/')
        .replace('\\', "/")
        .to_ascii_uppercase();
    let mut disc = legaia_iso::raw::RawDisc::open(disc_path)
        .with_context(|| format!("open disc {}", disc_path.display()))?;
    let vol = iso9660::read_volume(&mut disc).context("read ISO volume")?;
    let files = iso9660::walk_files(&mut disc, &vol.root).context("walk ISO files")?;
    files
        .into_iter()
        .find(|(p, _)| p.to_ascii_uppercase() == want)
        .map(|(_, rec)| (rec.lba, rec.size))
        .ok_or_else(|| anyhow::anyhow!("{} not found on disc {}", want, disc_path.display()))
}

struct StrPlayerApp {
    win: EngineWindow,
    frames: Vec<legaia_mdec::VideoFrame>,
    uploaded: Option<legaia_engine_render::UploadedTexture>,
    /// Wall-clock duration to hold each frame (from the stream's detected fps).
    frame_period: std::time::Duration,
    /// When playback started; the wall-clock fallback frame index is
    /// `elapsed / frame_period` (used only when no audio track is playing).
    clock: Option<std::time::Instant>,
    /// Live audio output, present only when an interleaved XA track decoded
    /// and the device opened. The video clock reads its cursor for A/V sync.
    /// Owned solely by the player (single-threaded), so no `Arc` is needed.
    audio_out: Option<legaia_engine_audio::AudioOut>,
    /// The decoded audio track, staged into `audio_out` on the first redraw so
    /// the audio cursor and the video start together. Taken once.
    pending_audio: Option<legaia_engine_shell::cutscene_av::CutsceneAudio>,
}

/// Build the flat tiled battle ground grid (retail's `func_0x801d02c0` grass
/// grid): a `(N+1)x(N+1)` vertex grid of quads on the PSX `y=0` plane centred at
/// the world origin, every vertex sampling the stage dome's **grass texel** so
/// it reads as real grass from the battle VRAM instead of the bare clear colour.
/// Returns `None` if the dome has no ground-plane (`|y|` small) textured vertex
/// to borrow the texel from. Drawn with the actor camera so the party stands on
/// it; coarse cells are fine because every vertex samples the same texel.
fn build_battle_ground_grid(
    dome: &legaia_tmd::mesh::VramMesh,
) -> Option<legaia_tmd::mesh::VramMesh> {
    // Borrow the dome's GRASS texture, targeting the exact tile retail's grid
    // (func_0x801d02c0) uses. `mednafen-state prim-trace` on the real map01
    // battle shows those ground tiles at uv ~ (132..140, 2..13) with their own
    // CBA/TSB. PROT 88 is the same TMD, so the dome's grass vertices carry the
    // same UVs - find the flat ground vertex nearest that tile centre, take its
    // CBA/TSB, and tile that small window. (Earlier picks - "first textured
    // vertex", "largest XZ area + bbox centre" - landed on the ground-mist
    // object or a 2-tone checker region of the texture, hence the checkerboard.)
    const GU0: u8 = 132;
    const GU1: u8 = 140;
    const GV0: u8 = 2;
    const GV1: u8 = 13;
    let tcu = ((GU0 as u16 + GU1 as u16) / 2) as i32;
    let tcv = ((GV0 as u16 + GV1 as u16) / 2) as i32;
    let best = (0..dome.positions.len())
        .filter(|&i| dome.positions[i][1].abs() < 5.0 && dome.cba_tsb[i] != [0, 0])
        .min_by_key(|&i| {
            let [u, v] = dome.uvs[i];
            (u as i32 - tcu).pow(2) + (v as i32 - tcv).pow(2)
        })?;
    let cba_tsb = dome.cba_tsb[best];
    let [bu, bv] = dome.uvs[best];
    log::info!(
        "battle ground grid: grass tile uv [{GU0}..{GU1}]x[{GV0}..{GV1}] cba_tsb={cba_tsb:?} (nearest dome vert uv=({bu},{bv}))"
    );
    let (u0, u1, v0, v1) = (GU0, GU1, GV0, GV1);

    const N: i32 = 28; // cells per side (retail func_0x801d02c0 grid)
    const P: f32 = 512.0; // retail func_0x801d02c0 cell pitch (0x200) -> ~+/-16384 extent
    let mut m = legaia_tmd::mesh::VramMesh {
        positions: Vec::new(),
        uvs: Vec::new(),
        cba_tsb: Vec::new(),
        indices: Vec::new(),
        normals: Vec::new(),
    };
    // Per-cell quads (own 4 vertices each) so EVERY cell maps to the same full
    // grass UV tile `[u0..u1]x[v0..v1]`. Shared-vertex grids forced a single UV
    // per vertex, which (alternating box corners by parity) made adjacent cells
    // sample different texture columns -> green-vs-dirt whole-cell jumps. With
    // each cell carrying the whole tile, the grass repeats uniformly.
    let half = N / 2;
    for iz in 0..N {
        for ix in 0..N {
            let (x0, z0) = ((ix - half) as f32 * P, (iz - half) as f32 * P);
            let (x1, z1) = (x0 + P, z0 + P);
            let base = m.positions.len() as u32;
            for (x, z, u, v) in [
                (x0, z0, u0, v0),
                (x1, z0, u1, v0),
                (x0, z1, u0, v1),
                (x1, z1, u1, v1),
            ] {
                m.positions.push([x, 0.0, z]);
                m.uvs.push([u, v]);
                m.cba_tsb.push(cba_tsb);
                m.normals.push([0.0, -1.0, 0.0]); // PSX up = -y (flat ground faces up)
            }
            m.indices
                .extend([base, base + 2, base + 1, base + 1, base + 2, base + 3]);
        }
    }
    Some(m)
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
                // Stage the audio on the first redraw so its cursor (the video
                // clock) starts when the picture does.
                if let (Some(out), Some(track)) =
                    (self.audio_out.as_ref(), self.pending_audio.take())
                {
                    out.play_xa(track.pcm, track.sample_rate, track.channels, false, 0x4000);
                }
                // A/V sync: drive the visible frame off the audio cursor when a
                // track is playing (audio is the hardware-paced master clock);
                // otherwise pace off wall-clock. Once the due frame passes the
                // last decoded frame, playback is done and the window closes.
                let now = std::time::Instant::now();
                let start = *self.clock.get_or_insert(now);
                let wall = now.duration_since(start).as_secs_f64();
                let audio_secs = self.audio_out.as_ref().and_then(|o| o.xa_cursor_secs());
                let due = legaia_engine_shell::cutscene_av::due_video_frame(
                    audio_secs,
                    wall,
                    self.frame_period.as_secs_f64(),
                );
                if due >= self.frames.len() {
                    if let Some(out) = self.audio_out.as_ref() {
                        out.stop_xa();
                    }
                    event_loop.exit();
                    return;
                }
                if let Some(renderer) = self.win.renderer() {
                    let f = &self.frames[due];
                    match renderer.upload_texture(&f.rgba, f.width, f.height) {
                        Ok(tex) => self.uploaded = Some(tex),
                        Err(e) => log::warn!("upload: {e}"),
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

#[cfg(test)]
mod battle_camera_tests {
    use super::PlayWindowApp;
    use glam::Vec4;
    use std::f32::consts::TAU;

    /// `retail_battle_mvp` must reproduce the exact retail overworld-battle
    /// projection `screen = H*(Rx(32u)*Ry(yaw)*v + (0,1280,7680))/Ze`, H=256,
    /// PSX +Y down, screen-centre (160,120) over 320x240 - pinned from the
    /// `overworld_battle_bg_angle_*` saves + `FUN_80026988`. Disc-free: pure
    /// math. Guards the glam matrix construction against regression.
    #[test]
    fn retail_battle_mvp_matches_psx_projection() {
        // Hand-rolled retail projection (the savestate-verified reference).
        fn handrolled(v: [f32; 3], yaw_u: f32) -> Option<(f32, f32)> {
            let yaw = yaw_u / 4096.0 * TAU;
            let pitch = 32.0 / 4096.0 * TAU;
            let (sy, cy) = yaw.sin_cos();
            let (sp, cp) = pitch.sin_cos();
            let ry = [cy * v[0] + sy * v[2], v[1], -sy * v[0] + cy * v[2]];
            let e = [ry[0], cp * ry[1] - sp * ry[2], sp * ry[1] + cp * ry[2]];
            let ez = e[2] + 7680.0;
            if ez <= 1.0 {
                return None;
            }
            Some((
                256.0 * e[0] / ez + 160.0,
                256.0 * (e[1] + 1280.0) / ez + 120.0,
            ))
        }
        // Sample several world points and orbit angles (4:3 so aspect_fix == 1).
        let mvp_aspect = 4.0 / 3.0;
        for &yaw_u in &[0.0f32, 224.0, 1024.0, 2632.0, 3136.0, 3808.0] {
            let mvp = PlayWindowApp::retail_battle_mvp(yaw_u / 4096.0 * TAU, mvp_aspect);
            for &v in &[
                [1000.0f32, -500.0, 3000.0],
                [-2000.0, 0.0, 6000.0],
                [0.0, -3000.0, -800.0],
                [5000.0, 12.0, 5000.0],
            ] {
                // Engine draws `cam * model` with `model` carrying the Y-flip F,
                // so the dome sample is `cam * F * v_psx` == flip v.y first.
                let clip = mvp * Vec4::new(v[0], -v[1], v[2], 1.0);
                if clip.w <= 1.0 {
                    continue;
                }
                let ndc = (clip.x / clip.w, clip.y / clip.w);
                let sx = 160.0 + ndc.0 * 160.0;
                let sy = 120.0 - ndc.1 * 120.0; // NDC up+ -> PSX screen down+
                if let Some((hx, hy)) = handrolled(v, yaw_u) {
                    let d = ((sx - hx).powi(2) + (sy - hy).powi(2)).sqrt();
                    assert!(
                        d < 0.05,
                        "yaw={yaw_u} v={v:?}: {d}px off ({sx},{sy} vs {hx},{hy})"
                    );
                }
            }
        }
    }
}
