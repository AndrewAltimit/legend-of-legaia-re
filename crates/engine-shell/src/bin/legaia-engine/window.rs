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
    /// Field NPC / animated-prop draws, one per visible MAN partition-1
    /// placement (the retail per-scene actor pool). Positions are live: the
    /// draw follows `World::field_npc_positions` (motion-VM walkers move),
    /// falling back to the record's spawn tile. Built at scene load in
    /// `upload_assets`.
    field_npc_draws: Vec<FieldNpcDraw>,
    /// Per-NPC looping ANM clip players (the scene-bundle record named by the
    /// placement's anim byte), keyed by placement slot - drives live clip
    /// playback for the placed NPCs (idle sway / walk cycles). Rebuilt with
    /// `field_npc_draws`; `npc_anim_srcs` holds each animated NPC's truncated
    /// TMD + raw bytes for the per-frame posed re-upload (the same rebuild
    /// path the player's idle/walk pair uses).
    npc_clip_players:
        std::collections::HashMap<u8, legaia_engine_core::field_anim::FieldClipPlayer>,
    npc_anim_srcs: std::collections::HashMap<u8, (legaia_tmd::Tmd, Vec<u8>)>,
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
    /// The two fishing point-exchange venue pages (0 = Buma, 1 = Vidna),
    /// decoded from the fishing overlay when the minigame starts
    /// ([`legaia_asset::fishing_exchange`]) and named from the SCUS item
    /// table when readable. `P` toggles the list while fishing.
    fishing_prize_venues: Option<[legaia_engine_core::fishing::PrizeExchange; 2]>,
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
    /// The startup extracted-root path, kept unconditionally (unlike
    /// [`Self::extracted_root`], which is `None` in disc runs) so scene
    /// rebuilds after a door transition source the shared interior page from
    /// `PROT.DAT`'s unindexed head gap the same way the boot-time build did.
    /// May point at a non-existent directory in pure-disc runs - the interior
    /// page upload soft-fails there, matching the boot build.
    scene_rebuild_extracted_root: std::path::PathBuf,
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

#[path = "window/assets.rs"]
mod assets;
#[path = "window/battle.rs"]
mod battle;
#[path = "window/boot_cutscene.rs"]
mod boot_cutscene;
#[path = "window/camera.rs"]
mod camera;
#[path = "window/event_handler.rs"]
mod event_handler;
#[path = "window/field_render.rs"]
mod field_render;
#[path = "window/hud.rs"]
mod hud;
#[path = "window/menu_draws.rs"]
mod menu_draws;
#[path = "window/minigames.rs"]
mod minigames;
#[path = "window/record.rs"]
mod record;
#[path = "window/title_save_draws.rs"]
mod title_save_draws;

pub(crate) use record::cmd_record;
use record::{RecordLog, RecordTarget};

impl PlayWindowApp {
    /// Maximum number of battle-event log lines kept in the HUD ring.
    const BATTLE_EVENT_LOG_CAP: usize = 6;
}

// --- Field pause-menu window geometry (320x240 boot-UI stage pixels) ---
//
// The field-menu text builders lay glyphs out in stage pixels; these pens
// and window rects place them on the shared boot-UI stage, framed by
// `menu_window_chrome_draws_for`. Position/size are placement-approximate:
// the retail caller-supplied rect (`a0+0xa..+0x10` in `FUN_801D33D8`, see
// docs/subsystems/field-menu.md) is not yet pinned, so these are chosen to
// frame each screen's known content extent. The content offsets *within*
// the frame are the engine's existing per-menu layout.

/// Stage pen for the main field-menu list (title + rows + money/time).
const FIELD_MENU_TEXT_PEN: (i32, i32) = (32, 64);
/// 9-slice frame rect for the main field-menu list (x, y, w, h). Sized to
/// contain the whole engine text block (title + command rows + the
/// money/play-time footer). Retail splits the money and play-time into
/// their own small corner boxes; a single frame is the first faithful
/// pass until those satellite windows are pinned.
const FIELD_MENU_WINDOW: (i32, i32, i32, i32) = (16, 52, 224, 176);
/// 9-slice frame rect for the wide sub-screens (status / spells / items /
/// equip / arts) - a near-fullscreen window on the 320x240 stage.
const MENU_SUBWINDOW: (i32, i32, i32, i32) = (12, 16, 296, 212);

/// The single world-space Y negation of the **field render frame**: field
/// world state (actor positions, placement Y, the heightfield's baked
/// `-lut` corner heights) is kept in the retail Y-down convention (up =
/// negative Y), and each field camera post-multiplies this so the world
/// passes through exactly ONE net Y negation on the way to NDC - retail-up
/// renders screen-up. Field model matrices are therefore UN-flipped (their
/// PSX Y-down local vertices ride the same world negation). Battle and
/// world-map keep the older pairing (per-model `scale(1,-1,1)` + a camera
/// with no world negation), so this must not leak into those arms.
const FIELD_WORLD_FLIP: Mat4 = Mat4::from_cols_array(&[
    1.0, 0.0, 0.0, 0.0, //
    0.0, -1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
]);

/// The retail battle world scale: the battle base matrix `0x8007BF10` is
/// `16384 * I` (GTE `4096` = 1.0) in every catalogued battle savestate - a
/// **4.0x uniform scale** composed under the camera rotation. See
/// [`PlayWindowApp::battle_dome_camera_mvp`].
const BATTLE_WORLD_SCALE: f32 = 4.0;

/// The retail overworld (walk-view) world scale: the same base matrix holds
/// `24576 * I` = **6.0x** in the world-map resident savestates
/// (`sebucus_overworld_resident` / `karisto_overworld_resident`).
const WORLD_MAP_WORLD_SCALE: f32 = 6.0;

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
/// One field NPC / animated-prop draw, resolved at scene load from a MAN
/// partition-1 placement record (the retail per-scene actor pool, stride
/// `0xD8`). Runtime-pinned resolution (anchor-C town01 census, 53/53 animated
/// actors):
///
///  - model byte `< 0xF0` -> scene TMD index `model` (retail registers the
///    scene TMD list into the `0x8007C018` pool at slot `model + 5`; the five
///    head slots are the party + savepoint meshes);
///  - model byte `>= 0xF0` -> global-pool head slot `model - 0xF0`;
///  - the record's `anim_id` byte (installed into actor `+0x5C`) = ANM record
///    index + 1 in the **scene bundle** (normal models) or the **PROT 0874 §1
///    locomotion bundle** (special models); `0` = no clip (unposed prop).
struct FieldNpcDraw {
    /// Partition-1 record index - the key `World::field_npc_positions` tracks
    /// live walker positions under.
    slot: u8,
    /// Textured mesh half (`self.meshes`), `None` when the model carries no
    /// textured prims.
    mesh_idx: Option<usize>,
    /// Untextured F*/G* colour half (`self.color_meshes`).
    color_idx: Option<usize>,
    /// Spawn world position (fallback when the world has no live position).
    spawn: (i16, i16),
}

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
/// stream that supplies those is unpinned), at raw retail Y-down
/// coordinates (the world-map cameras compose the single world negation). Colour is keyed by body `kind`
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
            // Raw retail Y-down coordinates: the world-map cameras compose
            // FIELD_WORLD_FLIP, so no per-vertex negation.
            pos.push([v[0] as f32, v[1] as f32, v[2] as f32]);
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

/// Build the play-window's render-side [`SceneResources`] for the host's
/// currently loaded scene: the shared blocks (`init_data` + `player_data`)
/// stay resident, the load kind mirrors the host's `enter_field_scene`
/// selection (WorldMap for `map\d\d`, Field otherwise), and the two
/// always-resident VRAM extras (field character atlas + shared interior
/// page) are layered on. Used both for the initial scene at window boot and
/// to REBUILD the render state after a door transition
/// (`SceneTickEvent::SceneEntered`) swaps the host's scene.
///
/// `extracted_root` sources the shared interior page (it lives in
/// PROT.DAT's unindexed head gap, unreachable through per-entry reads);
/// `None` soft-skips that upload.
fn build_window_scene_resources(
    session: &BootSession,
    extracted_root: Option<&Path>,
) -> Result<SceneResources> {
    let s = session
        .host
        .scene
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no scene loaded on the host"))?;
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
    let load_kind = if legaia_engine_core::scene::is_world_map_scene(&s.name) {
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
    match extracted_root {
        Some(root) => match legaia_asset::interior_page::read_from_prot_dat(&root.join("PROT.DAT"))
        {
            Ok(tim) => {
                legaia_asset::interior_page::upload_to_vram(&tim, &mut res.vram);
                log::info!("play-window: shared interior page uploaded (row-510 strip CLUT)");
            }
            Err(err) => {
                log::warn!("play-window: shared interior page skipped: {err:#}");
            }
        },
        None => log::warn!("play-window: shared interior page skipped: no extracted root"),
    }
    Ok(res)
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

    // Debug start-position override: `LEGAIA_START_TILE=X,Z` seats the player
    // at that tile's centre after boot (tile*128+0x40, the op-0x3F entry-tile
    // mapping). Useful for parking on the overworld continent - a direct
    // `--world-map` boot has no door warp to seat the player, and the ocean
    // is unwalkable, so the (0,0) default strands the marker at sea. E.g.
    // Rim Elm's retail map01 arrival is `LEGAIA_START_TILE=96,25`.
    if let Ok(spec) = std::env::var("LEGAIA_START_TILE") {
        let parts: Vec<i32> = spec
            .split(',')
            .filter_map(|p| p.trim().parse().ok())
            .collect();
        if let [tx, tz] = parts[..] {
            let (cx, cz) = (tx.clamp(0, 255) as u8, tz.clamp(0, 255) as u8);
            session.host.world.seat_player_at_tile(cx, cz);
            log::info!("play-window: LEGAIA_START_TILE seated player at tile ({cx},{cz})");
        } else {
            log::warn!("play-window: LEGAIA_START_TILE ignored (want \"X,Z\"): {spec:?}");
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

    let scene_res = build_window_scene_resources(&session, Some(extracted_root))?;
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
        fishing_prize_venues: None,
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
        field_npc_draws: Vec::new(),
        npc_clip_players: std::collections::HashMap::new(),
        npc_anim_srcs: std::collections::HashMap::new(),
        boot_ui: initial_boot_ui,
        save_dir: save_dir.to_path_buf(),
        options_state: legaia_engine_core::options::OptionsState::default(),
        record_log: record_to.map(RecordLog::from_target),
        field_live_opts,
        // In-flow cutscene STR resolves from the extracted root (video only)
        // or, when booting from a disc image, straight from the ISO with its
        // interleaved XA audio. Exactly one of these is set.
        extracted_root: disc.map_or_else(|| Some(extracted_root.to_path_buf()), |_| None),
        scene_rebuild_extracted_root: extracted_root.to_path_buf(),
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
