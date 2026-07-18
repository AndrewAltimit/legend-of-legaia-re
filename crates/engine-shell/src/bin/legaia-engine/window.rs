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
    CaptureImage, ColorSceneDraw, RenderTarget, Scene as RenderScene, SceneDraw, ShopRow, TextDraw,
    TextOverlay, UploadedColorMesh, UploadedFontAtlas, UploadedVram, UploadedVramMesh,
    capture_banner_draws_for, level_up_draws_for, shop_draws_for, text_draws_for,
    window::{EngineWindow, orbit_camera_mvp},
};
use legaia_engine_shell::BootSession;
use legaia_engine_shell::replay::{PadEvent, ReplayFile, ReplayMeta};
use legaia_font::Font;
use std::path::Path;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowId;

/// Deterministic screenshot harness for `play-window`: render one frame
/// offscreen at [`Self::capture_tick`] and write it to [`Self::path`], then
/// exit - and/or a periodic sweep ([`Self::sweep`]) that captures every N
/// ticks into a directory. [`Self::pad_script`] maps a world-tick to a
/// pad mask (one-tick edges, or held ranges via `FIRST-LAST:BUTTON`) so a
/// capture run can auto-open + navigate a menu - or hold a direction to
/// walk somewhere - without `xdotool`.
pub(crate) struct ScreenshotConfig {
    /// Single-shot output (`--screenshot`). `None` when only the periodic
    /// sweep is requested.
    pub path: Option<std::path::PathBuf>,
    pub capture_tick: u64,
    /// Periodic capture sweep (`--screenshot-every` + `--screenshot-dir`).
    pub sweep: Option<ScreenshotSweep>,
    /// tick -> pad button mask pressed for exactly that tick.
    pub pad_script: std::collections::HashMap<u64, u16>,
}

/// Periodic capture sweep for `play-window`: one PNG (`tick_%05d.png`) into
/// [`Self::dir`] every [`Self::every`] ticks; the run exits after the
/// capture at/past [`Self::last_tick`] (or when the window closes).
pub(crate) struct ScreenshotSweep {
    pub dir: std::path::PathBuf,
    pub every: u64,
    pub last_tick: Option<u64>,
}

impl ScreenshotConfig {
    /// Parse the CLI flags into a config. `None` when neither `--screenshot`
    /// nor `--screenshot-every` is present. Errors on an unparseable
    /// `--pad-script` entry or a half-specified sweep.
    pub(crate) fn from_args(
        path: Option<std::path::PathBuf>,
        capture_tick: u64,
        every: Option<u64>,
        dir: Option<std::path::PathBuf>,
        last_tick: Option<u64>,
        pad_script: Option<&str>,
    ) -> Result<Option<Self>> {
        let sweep = match (every, dir) {
            (Some(every), Some(dir)) => {
                anyhow::ensure!(every > 0, "--screenshot-every must be > 0");
                Some(ScreenshotSweep {
                    dir,
                    every,
                    last_tick,
                })
            }
            (None, None) => None,
            _ => anyhow::bail!("--screenshot-every and --screenshot-dir must be set together"),
        };
        if path.is_none() && sweep.is_none() {
            return Ok(None);
        }
        let mut script = std::collections::HashMap::new();
        if let Some(spec) = pad_script {
            for entry in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let (tick, btn) = entry
                    .split_once(':')
                    .with_context(|| format!("pad-script entry '{entry}' is not TICK:BUTTON"))?;
                let button = legaia_engine_core::input::PadButton::from_name(btn.trim())
                    .with_context(|| format!("pad-script button '{btn}' is not a pad button"))?;
                // `TICK` (one-tick edge) or `FIRST-LAST` (held across the
                // inclusive range - walking somewhere needs a held direction).
                let tick = tick.trim();
                let (first, last) =
                    match tick.split_once('-') {
                        Some((a, b)) => {
                            let a: u64 = a.trim().parse().with_context(|| {
                                format!("pad-script tick '{a}' is not a number")
                            })?;
                            let b: u64 = b.trim().parse().with_context(|| {
                                format!("pad-script tick '{b}' is not a number")
                            })?;
                            anyhow::ensure!(a <= b, "pad-script range '{tick}' is reversed");
                            (a, b)
                        }
                        None => {
                            let t: u64 = tick.parse().with_context(|| {
                                format!("pad-script tick '{tick}' is not a number")
                            })?;
                            (t, t)
                        }
                    };
                for t in first..=last {
                    *script.entry(t).or_insert(0) |= button.mask();
                }
            }
        }
        Ok(Some(Self {
            path,
            capture_tick,
            sweep,
            pad_script: script,
        }))
    }
}

/// Write a [`CaptureImage`] (RGBA8, row-major) to a PNG file. Used by the
/// `--screenshot` harness in the redraw path.
pub(crate) fn write_capture_png(path: &Path, img: &CaptureImage) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating screenshot dir {}", dir.display()))?;
    }
    let file = std::fs::File::create(path)
        .with_context(|| format!("creating screenshot {}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .context("png header")?
        .write_image_data(&img.rgba)
        .context("png data")?;
    Ok(())
}

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

/// The uploaded mesh slots of one **posed** static-object placement: the
/// frame-0 rest pose of a multi-object env prop baked into a textured and/or an
/// untextured mesh. Either half can be absent (a prop with no textured prims,
/// or none that survive the VRAM filter).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PosedMesh {
    /// Index into `PlayWindowApp::meshes` (the VRAM/textured pipeline).
    pub vram: Option<usize>,
    /// Index into `PlayWindowApp::color_meshes` (the untextured pipeline).
    pub color: Option<usize>,
    /// Index into `PlayWindowApp::field_posed_tmds` - the raw TMD this prop
    /// re-poses from when its clip moves off frame 0.
    pub tmd: Option<usize>,
}

/// Posed static-object meshes keyed by `(res.tmds index, anim id)` - one baked
/// rest pose per distinct (env mesh, animation clip) pair the scene's placed
/// objects reference. See `PlayWindowApp::posed_placement_keys`.
pub(crate) type PosedPlacementMeshes = std::collections::HashMap<(usize, u8), PosedMesh>;

/// One **posed placed prop** draw: a multi-object env mesh whose TMD objects are
/// the bones of the clip its object bind names (Rim Elm's house doors, its
/// cupboards, the windmill).
///
/// A prop resting on frame 0 draws the shared baked rest mesh - the cheap path,
/// and where every prop sits until it is touched. Once its clip advances, the
/// draw pass rebuilds the pose from the raw TMD at the prop's live frame, so the
/// door actually swings.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PosedPropDraw {
    /// The placement's footprint-anchor tile - its key into
    /// `PlayWindowApp::field_prop_anims`.
    pub anchor: (u8, u8),
    /// The clip id the prop's bind names (ANM record `anim_id - 1`).
    pub anim_id: u8,
    /// World model matrix.
    pub model: Mat4,
    /// The baked frame-0 rest meshes + the raw TMD to re-pose from.
    pub baked: PosedMesh,
}

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

/// One placed NPC's skinned mesh halves for one clip frame: the textured
/// (VRAM-sampled) half and the untextured (`F*`/`G*` vertex-colour) half.
/// Either can be absent when the source TMD carries no prims of that class.
type NpcPosedHalves = (Option<UploadedVramMesh>, Option<UploadedColorMesh>);

/// `(placement slot, clip frame) -> skinned mesh`. See
/// [`PlayWindowApp::npc_pose_cache`].
type NpcPoseCache = std::collections::HashMap<(u8, usize), NpcPosedHalves>;

/// The bone transforms one cached pose was skinned from - the correctness
/// oracle for [`NpcPoseCache`]. See [`PlayWindowApp::npc_pose_verify`].
type NpcPoseVerify = std::collections::HashMap<(u8, usize), Vec<([i16; 3], [i16; 3])>>;

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
    /// The menu overlay's window-descriptor table (PROT 0899 @0x15F24,
    /// `legaia_asset::menu_windows`), parsed once at boot: the retail
    /// window rect + content-renderer dispatch behind every pause-menu
    /// screen. `None` when the disc isn't loaded (pinned fallback rects
    /// in [`MENU_WINDOW_FALLBACK`] apply).
    menu_window_table: Option<legaia_asset::menu_windows::MenuWindowTable>,
    /// Uploaded "It was the Seru." caption sprite atlas (opdeene's baked TIM,
    /// `World::cutscene_caption`). Uploaded lazily the first frame the caption
    /// image is present and dropped when it clears on scene change; holds
    /// `(atlas, width, height)`. The caption is a pre-rendered scene texture,
    /// not font text - see [`legaia_engine_core::cutscene_caption`].
    caption_atlas: Option<(legaia_engine_render::UploadedSpriteAtlas, u32, u32)>,
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
    /// The scene's **posed** placed props (a bind naming a nonzero anim id), one
    /// entry per placement rather than one per `(mesh, anim)` pair: each keeps
    /// its own clip cursor, so touching one Rim Elm cupboard does not open the
    /// other three. Excluded from `field_placement_draws` /
    /// `field_placement_color_draws` - this list owns their draws.
    field_posed_props: Vec<PosedPropDraw>,
    /// Raw TMDs of the posed props, indexed by `PosedMesh::tmd`. A moving prop
    /// re-poses from these at its live frame; the rest pose is baked once into
    /// `PosedPropDraw::baked`. (The clip source is `npc_anim_bundles.0`, the
    /// scene ANM bundle.)
    field_posed_tmds: Vec<(legaia_tmd::Tmd, Vec<u8>)>,
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
    /// Memoised posed NPC meshes, keyed by `(placement slot, clip frame)`.
    ///
    /// An NPC's clip is a short **loop** over a fixed set of poses, so the
    /// posed mesh for a given `(slot, frame)` is a constant: rebuilding and
    /// re-uploading it every render frame recomputes the same bytes. The first
    /// visit to a frame skins + uploads it, every later visit reuses the GPU
    /// buffers. Bounded by `NPCs × clip length` (tens of small meshes).
    ///
    /// Invalidated wholesale when `upload_assets` rebuilds `npc_clip_players`
    /// (scene change), and per-slot when a channel op-`0x4B` ANIMATE cue swaps
    /// that slot's clip - a new clip reuses the same low frame indices, so its
    /// entries must not alias the outgoing clip's.
    npc_pose_cache: NpcPoseCache,
    /// Correctness oracle for [`Self::npc_pose_cache`], populated only under
    /// `LEGAIA_POSE_CACHE_VERIFY=1`: the bone transforms each cached key was
    /// built from. On every cache *hit* the incoming pose is compared against
    /// the stored one and a mismatch is logged as an error.
    ///
    /// Skinning is a pure function of `(TMD, bone transforms)`, so equal poses
    /// on a hit prove the cached GPU mesh is the mesh a rebuild would have
    /// produced - i.e. that the `(slot, frame)` key never aliases across
    /// clips. Empty (and free) when the env var is unset.
    npc_pose_verify: NpcPoseVerify,
    /// The ANM bundles the placements resolved their clips through,
    /// retained past scene load so channel op-`0x4B` ANIMATE cues
    /// (`World::field_npc_anim_cues`) can re-target an NPC's clip player
    /// mid-scene (the prologue-vignette "characters doing things" beats).
    /// `.0` = the per-scene bundle, `.1` = the party locomotion bundle
    /// (PROT 0874 §1); `npc_bundle_special[slot]` records which of the two
    /// a placement poses from (`special_model` placements use the
    /// locomotion bundle).
    npc_anim_bundles: (
        Option<legaia_asset::player_anm::PlayerAnmBundle>,
        Option<legaia_asset::player_anm::PlayerAnmBundle>,
    ),
    npc_bundle_special: std::collections::HashMap<u8, bool>,
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
    /// World-map water/CLUT-cell animation. `Some` for a world-map kingdom
    /// scene: normally the disc-derived slot-5 CLUT-walk table (eight
    /// independent 16x1 `MoveImage` walkers - ocean head + shoreline/terrain
    /// shimmer cells), with the legacy single-cell ocean-head cycle kept only
    /// as a fallback for a bundle without slot 5. `None` off the world map.
    ocean_anim: Option<WaterAnim>,
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
    /// Monotonic world-tick counter (drives the `--screenshot` pad script and
    /// capture timing). Incremented once per simulated tick.
    tick_no: u64,
    /// Deterministic screenshot harness (`--screenshot`). When set, the pad
    /// script injects one-tick edges and the render path captures an offscreen
    /// PNG at `capture_tick`, then exits. `None` in normal interactive runs.
    screenshot: Option<ScreenshotConfig>,
    /// Next world-tick at which the periodic screenshot sweep
    /// (`--screenshot-every`) captures. Advanced by `every` on each capture;
    /// unused when no sweep is configured.
    sweep_next_tick: u64,
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
    /// Tile-board tile-actor slots already queued for the mesh-upload drain.
    /// A board install spawns its actors through `World::spawn_field_actor`
    /// directly (no `ActorSpawned` event fires), so the redraw pass scans
    /// the board draw list and queues each slot once per install; cleared
    /// when the board tears down (draw list empties) so a later board's
    /// re-used slots re-upload their new templates.
    tile_slots_queued: std::collections::HashSet<u8>,
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
    /// Opt-in dynamic-lighting enhancement (NON-RETAIL - the field path has
    /// no light source). Seeded from `--dynamic-lighting`, toggled at
    /// runtime with the `I` key; mirrored into the renderer via
    /// [`legaia_engine_render::Renderer::set_dynamic_lighting`] and shown on
    /// the HUD status line. Default `false` = the faithful pixel-identical
    /// render.
    dynamic_lighting: bool,
    /// Mouse drag-orbit state: the last cursor X (window pixels) while the
    /// left button is held, `None` when not dragging. A horizontal drag in
    /// field free-roam rotates `session.camera.manual_orbit`, which both
    /// the follow camera's render yaw and the movement compass
    /// ([`legaia_engine_core::camera::Camera::compass_azimuth_units`]) read
    /// - so orbiting the view keeps "screen up walks away from the camera".
    orbit_drag_last_x: Option<f64>,
    /// Latest cursor X (window pixels), fed by `CursorMoved` so a drag that
    /// starts before any motion has an anchor.
    cursor_x: f64,
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
    /// `apply == 0` Camera Configure beats drained from this frame's field
    /// events, replayed as snaps onto the interp before the glide arms. The
    /// field VM runs until yield, so a snap beat + glide beat pair with no
    /// yield between (map01's fly-in aerial snap -> apply-900 descent)
    /// commits in ONE tick and the merged `camera_state` only shows the
    /// glide's targets; the snap replays keep the glide's start pose retail
    /// (the captured fly-in trajectory starts exactly at the snapped pose).
    pending_camera_snaps: Vec<Vec<legaia_engine_vm::field::CameraParam>>,
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
#[path = "window/geometry.rs"]
mod geometry;
#[path = "window/hud.rs"]
mod hud;
#[path = "window/menu_draws.rs"]
mod menu_draws;
#[path = "window/minigames.rs"]
mod minigames;
#[path = "window/record.rs"]
mod record;
#[path = "window/run.rs"]
mod run;
#[path = "window/save_select_helpers.rs"]
mod save_select_helpers;
#[path = "window/str_player.rs"]
mod str_player;
#[path = "window/title_save_draws.rs"]
mod title_save_draws;

pub(crate) use record::cmd_record;
use record::{RecordLog, RecordTarget};
// Re-export the extracted geometry / driver / save-select / str-player
// items so the sibling window submodules (which `use super::*`) still see
// them at the same effective scope they had before the split.
pub(crate) use geometry::{
    LineGeometry, build_battle_ground_grid, effect_billboard_mesh, effect_sprite_line_geometry,
    heightfield_to_vram_mesh, world_map_entity_line_geometry, world_map_player_line_geometry,
    world_map_slot4_line_geometry,
};
pub(crate) use run::cmd_play_window;
// These two stay window-tree-private (their signatures reference the
// `pub(super)` record types); re-exported only so the sibling submodules
// that `use super::*` still resolve them unqualified.
pub(in crate::window) use run::{build_window_scene_resources, cmd_play_window_with_record};
pub(crate) use save_select_helpers::{
    build_slot_info_view, info_panel_slide_offset, scan_save_dir, slot_leader_char_id,
};
pub(crate) use str_player::{cmd_play_str, resolve_iso_file};

impl PlayWindowApp {
    /// Maximum number of battle-event log lines kept in the HUD ring.
    const BATTLE_EVENT_LOG_CAP: usize = 6;
}

// --- Field pause-menu window geometry (320x240 boot-UI stage pixels) ---
//
// The field-menu text builders lay glyphs out in stage pixels; the window
// rects come from the menu overlay's **window-descriptor table** (PROT
// 0899 @0x15F24, VA 0x801E473C - `legaia_asset::menu_windows`), parsed
// from the user's disc at boot into `PlayWindowApp::menu_window_table`.
// Each descriptor rect is the window's *content* origin/extent (the
// `a0+0xa..+0x10` rect the retail content renderers receive, e.g.
// `FUN_801D33D8`'s `WX`/`WY`); the caller-drawn 9-slice frame extends
// past it (`MenuWindowDescriptor::frame_rect`). The pinned fallback below
// mirrors the disc table for the ids the engine draws (the disc-gated
// `menu_windows_real` test asserts the mirror), so geometry stays retail
// even without a disc table.

/// Pinned content rects mirroring the disc descriptor table:
/// `(descriptor id, (x, y, w, h))`.
#[rustfmt::skip]
const MENU_WINDOW_FALLBACK: [(usize, (i32, i32, i32, i32)); 15] = {
    use legaia_asset::menu_windows::window_ids as w;
    [
        (w::TAB_EQUIP, (16, 12, 60, 12)),
        (w::TAB_STATUS, (12, 12, 60, 12)),
        (w::TAB_OPTIONS, (16, 12, 60, 12)),
        (w::EQUIP_PARTY, (14, 42, 80, 38)),
        (w::EQUIP_MAIN, (14, 96, 292, 108)),
        (w::EQUIP_LIST, (174, 22, 132, 182)),
        (w::STATUS_PARTY_LIST, (14, 38, 60, 38)),
        (w::STATUS_CONDITION, (14, 92, 60, 10)),
        (w::STATUS_MAIN, (90, 16, 218, 188)),
        (w::STATUS_SUMMARY, (14, 134, 60, 70)),
        (w::OPTIONS_MAIN, (24, 40, 256, 148)),
        // The options value popup's descriptor x/w (y/h are stamped per
        // open - see `options_popup_rect`).
        (w::OPTIONS_POPUP, (170, 132, 128, 36)),
        (w::TOP_MONEY_TIME, (24, 178, 104, 24)),
        (w::TOP_COMMAND_LIST, (24, 24, 104, 94)),
        (w::TOP_INFO_PANEL, (144, 24, 152, 180)),
    ]
};

/// Content rect used for the sub-screens whose retail window sets are not
/// yet capture-pinned (Items / Spells / Arts) - a near-fullscreen window
/// on the 320x240 stage.
const MENU_SUBWINDOW_CONTENT: (i32, i32, i32, i32) = (18, 18, 284, 200);

/// Options round-trip file (sibling of `legaia-input.toml`). Holds the
/// retail settings plus the engine-only knobs (BGM / SFX volume, message
/// speed) that the pause-menu options screen doesn't show.
const OPTIONS_CONFIG_FILE: &str = "legaia-options.toml";

impl PlayWindowApp {
    /// Content rect for a menu window id: the disc-parsed descriptor when
    /// available, else the pinned mirror in [`MENU_WINDOW_FALLBACK`].
    fn menu_window_rect(&self, id: usize) -> (i32, i32, i32, i32) {
        if let Some(d) = self.menu_window_table.as_ref().and_then(|t| t.window(id)) {
            return d.rect();
        }
        MENU_WINDOW_FALLBACK
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, r)| *r)
            .unwrap_or(MENU_SUBWINDOW_CONTENT)
    }

    /// Content-origin pen for a menu window id.
    fn menu_window_pen(&self, id: usize) -> (i32, i32) {
        let (x, y, _, _) = self.menu_window_rect(id);
        (x, y)
    }

    /// Frame rect (the 9-slice chrome box) for a menu window id: the
    /// retail border art extends 8 px past the content rect on every
    /// side (prim-scan pinned; `MenuWindowDescriptor::frame_rect`).
    fn menu_window_frame_rect(&self, id: usize) -> (i32, i32, i32, i32) {
        let (x, y, w, h) = self.menu_window_rect(id);
        (x - 8, y - 8, w + 16, h + 16)
    }

    /// Apply the live side-effects of [`Self::options_state`] (currently
    /// the Sound Stereo/Monaural downmix on the audio output) and persist
    /// the state to [`OPTIONS_CONFIG_FILE`]. Called when an options
    /// session closes.
    pub(super) fn persist_and_apply_options(&self) {
        self.apply_options_side_effects();
        let path = std::path::PathBuf::from(OPTIONS_CONFIG_FILE);
        if let Err(e) = self.options_state.save(&path) {
            log::warn!("options: save to {} failed: {e:#}", path.display());
        }
    }

    /// Push the current options into their live consumers without touching
    /// disk (also called once at startup after the config file loads).
    pub(super) fn apply_options_side_effects(&self) {
        if let Some(audio) = self.session.audio.as_ref() {
            audio.set_mono(matches!(
                self.options_state.audio,
                legaia_engine_core::options::AudioMode::Mono
            ));
            // Master mute (engine-only knob; `V` toggles it live). The
            // mixer gate zeroes the output while the sequencer / SPU keep
            // ticking, so unmuting resumes mid-track in sync.
            audio.set_muted(self.options_state.muted);
        }
    }

    /// Content rect for the options value popup (window descriptor id 47:
    /// x/w from the table, y/h stamped per-open by the retail SM
    /// `FUN_801da9f8` off the id-48 settings window's y + the cursor
    /// row's layout offset).
    fn options_popup_rect(
        &self,
        popup: &legaia_engine_core::options::OptionsPopup,
    ) -> (i32, i32, i32, i32) {
        use legaia_asset::menu_windows::window_ids;
        let (px, _, pw, _) = self.menu_window_rect(window_ids::OPTIONS_POPUP);
        let (_, sy, _, _) = self.menu_window_rect(window_ids::OPTIONS_MAIN);
        legaia_engine_core::options::options_popup_content_rect(
            sy,
            px,
            pw,
            popup.row,
            popup.choices.len(),
        )
    }
}

/// The single world-space Y negation of the **field render frame**: field
/// world state (actor positions, placement Y, the heightfield's baked
/// `-lut` corner heights) is kept in the retail Y-down convention (up =
/// negative Y), and each field camera post-multiplies this so the world
/// passes through exactly ONE net Y negation on the way to NDC - retail-up
/// renders screen-up. Field model matrices are therefore UN-flipped (their
/// PSX Y-down local vertices ride the same world negation). Battle and
/// world-map keep the older pairing (per-model `scale(1,-1,1)` + a camera
/// with no world negation), so this must not leak into those arms.
/// The follow camera's fixed base yaw (PSX 12-bit units, savestate-pinned
/// `_DAT_8007B792 = -160` on the town01 anchor). Shared between the render
/// camera (`field_follow_camera_mvp`) and the movement-compass bias pushed
/// into the engine-core camera at startup (`render_yaw_bias` - the compass
/// sense is the negation: `alpha = -psi` for the PSX GTE camera).
const FIELD_FOLLOW_YAW_UNITS: f32 = -160.0;

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

/// World-map water/CLUT-cell animation state: the disc-derived kingdom
/// slot-5 CLUT-walk table when the bundle ships it (every retail kingdom
/// does), or the legacy single-cell ocean-head cycle as the fallback.
enum WaterAnim {
    Walk(ClutWalkAnim),
    Ocean(OceanAnim),
}

/// Table-driven CLUT-walk animator: one independent accumulator per table
/// entry, all sharing the same game-tick clock (retail spawns one walker
/// actor per entry, and every accumulator steps by the same per-frame
/// `DAT_1F800393` dt, so the entries stay phase-locked to a common epoch).
// REF: FUN_80024cfc - the per-entry actor spawn (accumulator at +0x68,
// seeded to 100 so every entry's first copy fires at scene entry).
struct ClutWalkAnim {
    /// The parsed kingdom slot-5 table ([`legaia_asset::clut_walk`]).
    table: legaia_asset::clut_walk::ClutWalkTable,
    /// Per-entry `(accumulator vsyncs, frame index)`, indexed like
    /// `table.entries`.
    state: Vec<(u32, usize)>,
    /// Vsyncs counted toward the next retail *game tick* (a game tick spans
    /// `World::frame_step` vsyncs - the retail `DAT_1F800393` adaptive
    /// frame-skip factor written by `FUN_80016B6C`).
    vsyncs_to_game_tick: u32,
}

/// Legacy ocean-head cycle, kept only as the fallback for a kingdom bundle
/// without a parseable slot-5 CLUT-walk table (no retail bundle hits this;
/// it keeps the most visible effect - the sea shimmer - alive on a modified
/// or damaged disc rather than freezing the ocean).
struct OceanAnim {
    /// 13 frames × 32 bytes (16 BGR555 entries each), as decoded by
    /// [`legaia_asset::ocean::find_ocean_assets`].
    frames: Vec<u8>,
    /// Current frame index (`0..frames.len()/32`).
    cur: usize,
    /// Vsyncs counted toward the next retail *game tick* (see
    /// [`ClutWalkAnim::vsyncs_to_game_tick`]).
    vsyncs_to_game_tick: u32,
    /// Vsync accumulator toward the next frame advance; each game tick adds
    /// `frame_step` vsyncs and the frame advances (accumulator reset to
    /// zero, the retail walker semantic) every
    /// [`OCEAN_ANIM_VSYNCS_PER_FRAME`].
    vsync_accum: u32,
}

/// Fallback-path vsyncs between ocean-head frame advances: the
/// `hold_vsyncs` of the slot-5 ocean-head entry (`(0, 506)`, hold 8 - see
/// [`legaia_asset::clut_walk`]), so even the fallback runs the disc-derived
/// cadence (8 banked vsyncs -> a copy every 9 vsyncs at the overworld's
/// `dt = 3`). The table-driven path reads the per-entry holds directly.
const OCEAN_ANIM_VSYNCS_PER_FRAME: u32 = 8;

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
