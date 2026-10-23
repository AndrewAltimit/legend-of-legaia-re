//! The play window's state: [`PlayWindowApp`] and the render-side records
//! its fields hold (posed meshes, the ground source, NPC draws, lit meshes,
//! the water animation, the draw census).

use super::*;

/// The field ground's CPU mesh + per-vertex cell refs, as uploaded: the
/// source the visible-tile crop re-uploads a cropped index list from.
pub(crate) struct GroundSource {
    pub(crate) vmesh: legaia_tmd::mesh::VramMesh,
    pub(crate) flat_refs: Vec<[f32; 8]>,
    /// The heightfield the mesh was built from - its per-vertex corner tiers
    /// are what the live floor-height ladder re-resolves.
    pub(crate) hf: legaia_scene_formats::field_objects::WalkHeightfield,
    /// The live (scratchpad-frame) ladder `vmesh.positions` were last
    /// re-resolved against; `None` while they are the baked build.
    pub(crate) lut_applied: Option<[i16; 16]>,
}

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
    /// The record's `+0x1E` cull radius (the actor's `+0x58`), for the
    /// placed-object actor cull.
    pub cull_radius: u8,
}

pub(super) use legaia_engine_session::battle_vram::BattleMemberFace;

/// Windowed in-flow cutscene playback state: the decoded STR frames the
/// field VM's FMV-trigger op resolved to, plus the current frame cursor and
/// the live GPU upload. Held on [`PlayWindowApp`] while the world sits in
/// [`SceneMode::Cutscene`]; the world resumes (`finish_cutscene`) once the
/// frames drain. Paced to the stream's detected frame rate (wall-clock gated),
/// like [`StrPlayerApp`], so the movie isn't tied to the display refresh rate.
pub(super) struct WindowedCutscene {
    pub(super) frames: Vec<legaia_mdec::VideoFrame>,
    pub(super) idx: usize,
    pub(super) uploaded: Option<legaia_engine_render::UploadedTexture>,
    /// Wall-clock duration to hold each frame (from `StrTiming::frame_period`).
    pub(super) frame_period: std::time::Duration,
    /// When playback started; the wall-clock fallback frame index is
    /// `elapsed / period` (used only when there is no audio track).
    pub(super) clock: Option<std::time::Instant>,
    /// The cutscene's interleaved XA audio track, staged into the engine's
    /// audio output on the first render so its cursor (the video clock for A/V
    /// sync) starts with the picture. `None` for a video-only (extract-sourced)
    /// cutscene. Taken once.
    pub(super) pending_audio: Option<crate::cutscene_av::CutsceneAudio>,
    /// `true` once an audio track has been staged - the render loop then reads
    /// the audio cursor as the master clock instead of wall-clock.
    pub(super) has_audio: bool,
    /// The shared pacing rule (`legaia_engine_system::movie::cutscene::MovieClock`),
    /// which the page's movie lane runs too: the audio cursor, or the wall
    /// clock once the cursor has sat at zero past the stall window.
    pub(super) movie_clock: legaia_engine_system::movie::cutscene::MovieClock,
    /// The player aborted the movie (`fmv_skip_edge_hit`): the next redraw
    /// ends it as a drained one.
    pub(super) skipped: bool,
}

/// Pre-decoded publisher-logos atlas + GPU upload. Created once at boot
/// when the disc has a valid PROT 0895; reused by [`BootUiState::PublisherLogos`]
/// to sample one logo per frame.
pub(super) struct PublisherLogosAssets {
    /// Per-logo source rects in atlas pixels.
    pub(super) rects: [(u32, u32, u32, u32); legaia_engine_title::publisher_logos::LOGO_COUNT],
    /// GPU-resident sprite atlas (vertically stacked logos).
    pub(super) atlas: legaia_engine_render::UploadedSpriteAtlas,
}

/// Pre-decoded title-screen atlas + GPU upload. Created once at boot
/// when the disc has a valid PROT 0888 (`sound_data2` per CDNAME,
/// carries title art - see `legaia_ui_formats::title_pak`); reused by
/// [`BootUiState::Title`] to blit one quad per frame.
pub(super) struct TitleScreenAssets {
    /// Source rect to sample for the title quad
    /// (`(0, 0, width, height)` for the single-TIM layout).
    pub(super) rect: (u32, u32, u32, u32),
    /// GPU-resident sprite atlas (the 256×256 title TIM).
    pub(super) atlas: legaia_engine_render::UploadedSpriteAtlas,
}

/// Pre-decoded menu-glyph atlas + GPU upload. Created once at boot
/// when the disc has a readable `PROT.DAT` (the source TIM lives in
/// the unindexed 240 KB pre-`init_data` gap; see
/// [`legaia_ui_formats::menu_glyph_atlas`]). Reused by [`BootUiState::Title`]
/// to render the "NEW GAME" / "CONTINUE" / "OPTIONS" menu rows in
/// the retail small-caps font.
pub(super) struct MenuGlyphAssets {
    /// GPU-resident sprite atlas (the 256×256 menu-glyph TIM).
    pub(super) atlas: legaia_engine_render::UploadedSpriteAtlas,
}

/// Pre-decoded save-menu UI atlas + GPU upload. Created once at boot
/// when the disc has both a readable PROT 0899 (carries the SLOT 1 /
/// SLOT 2 pill sprites with CLUT 7) AND a readable PROT.DAT (carries
/// the system-UI sprite sheet at offset `0x018E0` with the 9-slice
/// panel chrome under CLUT row 2). Reused by [`BootUiState::Continue`]
/// to compose the retail 81×29 "Load" panel from 14 byte-pinned
/// textured-sprite primitives + the 2 slot pills over the dimmed
/// title background.
pub(super) struct SaveMenuAssets {
    /// Source rects for the 9-slice panel tiles + slot pills.
    pub(super) rects: legaia_engine_ui::SaveMenuAtlasRects,
    /// GPU-resident sprite atlas (composite 256×256: panel tiles from
    /// system-UI TIM + slot pills from PROT 0899's save-menu TIM).
    pub(super) atlas: legaia_engine_render::UploadedSpriteAtlas,
    /// Battle-HUD badge cells inside that atlas: the nine status-element
    /// word tags and the eight element badges, `None` per cell this bake
    /// could not reach a palette for.
    pub(super) badges: legaia_engine_ui::battle_hud_chrome::BattleBadgeRects,
    /// A fully-opaque 1x1 texel inside this bake
    /// ([`legaia_engine_ui::atlas_opaque_texel`]) - the surface the field
    /// party HUD's translucent plate is drawn on, so the plate can sit in the
    /// sprite half beneath the readout it backs.
    pub(super) solid: Option<(u32, u32, u32, u32)>,
}

/// Muscle Dome hub-screen assets: the two hub page TIMs (extraction 1220,
/// `other6.lzs` slot 0 - the pages retail uploads at VRAM (320,0)/(320,256))
/// baked per referenced 16-colour sub-palette into one GPU atlas, plus the
/// PROT 0977 sprite descriptor table the shared quad emitters
/// (`legaia_engine_ui::muscle_dome_hud`) place every screen from. Loaded on
/// the first Muscle Dome entry; `None` without a disc.
pub(super) struct MuscleHubAssets {
    /// `(sheet, sub-palette, atlas y offset)` of each baked page copy.
    /// `sheet` 0/1 = the (320,0)/(320,256) hub page (tpage bit 4); the
    /// sub-palette index is the packet CLUT's `& 0x3F` field, exactly the
    /// mapping the browser dome page uses.
    pub(super) blocks: Vec<(u8, u8, u32)>,
    /// The two pages' palette STP classes: whether a semi-transparent hub
    /// packet blends or draws opaque (`ringside_backdrop::HubPaletteStp`).
    pub(super) palette_stp: legaia_engine_ui::ringside_backdrop::HubPaletteStp,
    /// Pristine parsed sprite table, cloned per frame - the retail emitters
    /// write their variant back into the shared records.
    pub(super) table: Vec<legaia_engine_ui::muscle_dome_hud::HudSprite>,
    /// GPU-resident atlas (vertically stacked `(sheet, sub-palette)` blocks).
    pub(super) atlas: legaia_engine_render::UploadedSpriteAtlas,
    /// `(still variant, atlas y offset)` of each ringside still baked into
    /// the same atlas (`0` = extraction 1221, `1` = 1222), laid out as the
    /// VRAM region `(384, 0)` the loader uploads it to.
    pub(super) stills: Vec<(u32, u32)>,
    /// Atlas y of a small all-white block - the texel the untextured
    /// backdrop shade (`FUN_801D1610`) is drawn with.
    pub(super) white_y: u32,
}

/// One placed NPC's skinned mesh halves for one clip frame: the textured
/// (VRAM-sampled) half and the untextured (`F*`/`G*` vertex-colour) half.
/// Either can be absent when the source TMD carries no prims of that class.
pub(super) type NpcPosedHalves = (Option<UploadedVramMesh>, Option<UploadedColorMesh>);

/// A clip-less NPC's TMD, raw bytes and frame-0 rest pose (when its anim
/// record names one). See [`PlayWindowApp::npc_rest_srcs`].
pub(super) type NpcRestSrc = (legaia_tmd::Tmd, Vec<u8>, Option<Vec<([i16; 3], [i16; 3])>>);

/// `(placement slot, clip frame) -> skinned mesh`. See
/// [`PlayWindowApp::npc_pose_cache`].
pub(super) type NpcPoseCache = std::collections::HashMap<(u8, usize), NpcPosedHalves>;

/// The bone transforms one cached pose was skinned from - the correctness
/// oracle for [`NpcPoseCache`]. See [`PlayWindowApp::npc_pose_verify`].
pub(super) type NpcPoseVerify = std::collections::HashMap<(u8, usize), Vec<([i16; 3], [i16; 3])>>;

/// Windowed engine runner state. Owned by the winit event loop.
/// One Baka round-chrome draw plus its stamped glyph cell rect
/// (`glyph_u`-paged `u` + the widget record's `v/w/h`), when the draw pages
/// the glyph strip and the parsed widget table is resident.
pub(super) type ResolvedChromeDraw = (
    legaia_engine_minigames::baka_fighter_chrome::ChromeDraw,
    Option<(u8, u8, u8, u8)>,
);

/// How long after a first `Escape` a second one quits. One press only
/// arms the prompt: a player reaching for a pause key must not lose an
/// unsaved game to it.
pub(in crate::window) const QUIT_CONFIRM_WINDOW: std::time::Duration =
    std::time::Duration::from_secs(3);

/// How long a hotkey's notice line ("Ground fog: off") stays up.
pub(in crate::window) const NOTICE_DURATION: std::time::Duration =
    std::time::Duration::from_millis(2500);

/// What an `Escape` press does, given when the last unconfirmed one was.
pub(in crate::window) fn escape_quits(
    armed: Option<std::time::Instant>,
    now: std::time::Instant,
) -> bool {
    armed.is_some_and(|t| now.saturating_duration_since(t) <= QUIT_CONFIRM_WINDOW)
}

/// How long the start-up key hint stays up once the window first draws.
pub(in crate::window) const STARTUP_HINT_DURATION: std::time::Duration =
    std::time::Duration::from_secs(12);

/// The start-up key hint's line: a player who opens the window from Studio
/// or a file manager is told nowhere else that `F12` lists the keys.
pub(in crate::window) const STARTUP_HINT_TEXT: &str = "F12 or ? lists the keys - Esc twice quits";

/// The one-line key hint a player's window shows when it opens. Armed only
/// for an interactive window (`cmd_play_window_with_record`, no
/// `--screenshot` harness),
/// so a parity capture, a periodic sweep and the headless window never draw
/// it. Its clock starts on the first frame that asks, not at launch, so a
/// slow disc load or the intro movie does not eat it.
#[derive(Debug, Default)]
pub(in crate::window) struct StartupHint {
    armed: bool,
    shown_at: std::cell::Cell<Option<std::time::Instant>>,
    dismissed: bool,
}

impl StartupHint {
    pub(in crate::window) fn arm(&mut self) {
        self.armed = true;
    }

    /// The player has found the keys (opened the help panel): the hint is
    /// done.
    pub(in crate::window) fn dismiss(&mut self) {
        self.dismissed = true;
    }

    /// Whether the hint draws on a frame at `now`; the first call that
    /// answers yes starts its clock.
    pub(in crate::window) fn visible(&self, now: std::time::Instant) -> bool {
        if !self.armed || self.dismissed {
            return false;
        }
        let at = match self.shown_at.get() {
            Some(t) => t,
            None => {
                self.shown_at.set(Some(now));
                now
            }
        };
        now.saturating_duration_since(at) <= STARTUP_HINT_DURATION
    }
}

pub(super) struct PlayWindowApp {
    pub(super) session: NativeSession,
    pub(super) font: Font,
    /// Pre-built scene resources (VRAM + TMDs). Consumed by `upload_assets`
    /// when the renderer is first attached; `None` after that.
    pub(super) scene_res: Option<SceneResources>,
    pub(super) win: EngineWindow,
    pub(super) font_atlas: Option<UploadedFontAtlas>,
    /// Publisher-logos atlas (built once from PROT 0895). `None` if the
    /// disc isn't loaded or init.pak doesn't parse.
    pub(super) publisher_logos: Option<PublisherLogosAssets>,
    /// CPU-side atlas data waiting for renderer upload. Moved into
    /// `publisher_logos` on the first frame the renderer is available.
    pub(super) pending_publisher_logos_atlas:
        Option<legaia_engine_title::publisher_logos::LogosAtlas>,
    /// Title-screen atlas (built once from PROT 0888). `None` if the
    /// disc isn't loaded or the title TIM doesn't parse.
    pub(super) title_screen: Option<TitleScreenAssets>,
    /// CPU-side title atlas waiting for renderer upload. Moved into
    /// `title_screen` on the first frame the renderer is available.
    pub(super) pending_title_screen_atlas:
        Option<legaia_engine_title::title_screen_atlas::TitleScreenAtlas>,
    /// Menu-glyph atlas (built once from raw `PROT.DAT` at 0x11218).
    /// `None` if the disc isn't loaded or the TIM doesn't parse.
    pub(super) menu_glyphs: Option<MenuGlyphAssets>,
    /// CPU-side menu-glyph atlas waiting for renderer upload. Moved
    /// into `menu_glyphs` on the first frame the renderer is available.
    pub(super) pending_menu_glyph_atlas:
        Option<legaia_engine_menus::menu_glyph_atlas::MenuGlyphAtlas>,
    /// Save-menu UI atlas (built once from PROT 0899). `None` if the
    /// disc isn't loaded or the save-menu TIM doesn't parse.
    pub(super) save_menu: Option<SaveMenuAssets>,
    /// CPU-side save-menu atlas waiting for renderer upload. Moved
    /// into `save_menu` on the first frame the renderer is available.
    pub(super) pending_save_menu_atlas: Option<legaia_engine_menus::save_menu_atlas::SaveMenuAtlas>,
    /// The menu overlay's window-descriptor table (PROT 0899 @0x15F20,
    /// `legaia_game_tables::menu_windows`), parsed once at boot: the retail
    /// window rect + content-renderer dispatch behind every pause-menu
    /// screen. `None` when the disc isn't loaded (pinned fallback rects
    /// in [`MENU_WINDOW_FALLBACK`] apply).
    pub(super) menu_window_table: Option<legaia_game_tables::menu_windows::MenuWindowTable>,
    /// Uploaded "It was the Seru." caption sprite atlas (opdeene's baked TIM,
    /// `World::cutscene.caption`). Uploaded lazily the first frame the caption
    /// image is present and dropped when it clears on scene change; holds
    /// `(atlas, width, height)`. The caption is a pre-rendered scene texture,
    /// not font text - see [`legaia_engine_scene::cutscene_caption`].
    pub(super) caption_atlas: Option<(legaia_engine_render::UploadedSpriteAtlas, u32, u32)>,
    pub(super) uploaded_vram: Option<UploadedVram>,
    pub(super) meshes: Vec<UploadedVramMesh>,
    /// Retained TMD data (struct + raw bytes) parallel to `meshes`, used to
    /// re-pose animated actor meshes each frame via `tmd_to_vram_mesh_posed`.
    pub(super) scene_tmd_data: Vec<(legaia_tmd::Tmd, Vec<u8>)>,
    /// Field static-geometry draws: `(index into `meshes`, world model matrix)`
    /// per placed environment object, resolved at scene load from the field
    /// map's object table (`legaia_scene_formats::field_objects`). Empty for non-field
    /// scenes. Drawn in `SceneMode::Field` so the town renders its buildings /
    /// terrain at their world positions instead of all at the origin.
    pub(super) field_placement_draws: Vec<(usize, Mat4)>,
    /// The scene's **posed** placed props (a bind naming a nonzero anim id), one
    /// entry per placement rather than one per `(mesh, anim)` pair: each keeps
    /// its own clip cursor, so touching one Rim Elm cupboard does not open the
    /// other three. Excluded from `field_placement_draws` /
    /// `field_placement_color_draws` - this list owns their draws.
    pub(super) field_posed_props: Vec<PosedPropDraw>,
    /// Raw TMDs of the posed props, indexed by `PosedMesh::tmd`. A moving prop
    /// re-poses from these at its live frame; the rest pose is baked once into
    /// `PosedPropDraw::baked`. (The clip source is `npc_anim_bundles.0`, the
    /// scene ANM bundle.)
    pub(super) field_posed_tmds: Vec<(legaia_tmd::Tmd, Vec<u8>)>,
    /// The current scene's TMD pack indexed by `pack_index` (= a field move-VM
    /// stager's relative `model_sel`): `res.tmds` filtered to the
    /// `scene_asset_table` bundle entry, in scan order - the same `env_tmds` the
    /// field-placement renderer + the asset viewer use, retail
    /// `DAT_8007C018[5..]`. Built at scene load (the `SceneResources` is consumed
    /// by `upload_assets`, so the field-FX render path keeps its own clone). Used
    /// to draw op-0x34-sub-3 field effects against the SCENE meshes, not the
    /// battle `global_tmd_pool`.
    pub(super) field_stager_tmds: Vec<(legaia_tmd::Tmd, Vec<u8>)>,
    /// Per env-pack-slot uploaded-mesh index (`meshes`), for the VDF morph
    /// substitution: `field_pack_mesh_idx[pack_slot]` names the static mesh
    /// a live morph rebuild stands in for. `None` for pack TMDs with no
    /// uploaded textured mesh. Rebuilt per scene in `upload_assets`.
    pub(super) field_pack_mesh_idx: Vec<Option<usize>>,
    /// Live VDF-morphed replacements keyed by uploaded-mesh index: when the
    /// world's morph deltas move (`World::take_morph_dirty_slots`), the
    /// affected pack mesh is rebuilt with the staged vertices
    /// (`ResolvedTmd::with_group_deltas` arithmetic) and every draw of that
    /// mesh substitutes this upload - the native side of the
    /// `FUN_8001C604` render substitution.
    pub(super) field_morph_live: std::collections::HashMap<usize, UploadedVramMesh>,
    /// Untextured (`F*`/`G*`) vertex-colour meshes for field props whose prims
    /// carry per-vertex colours instead of UVs (the textured VRAM-mesh path
    /// drops them). Parallel render list to `meshes`.
    pub(super) color_meshes: Vec<UploadedColorMesh>,
    /// Field static-geometry colour draws: `(index into `color_meshes`, world
    /// model)` for the untextured props. Drawn alongside `field_placement_draws`.
    pub(super) field_placement_color_draws: Vec<(usize, Mat4)>,
    /// Which placed-object sweep owns each draw of `field_placement_draws` /
    /// `field_placement_color_draws` (parallel lists): `Some` for the sub-area
    /// window sweep's placements, which the per-frame pass gates on the
    /// world's windowed static-object list through
    /// `legaia_engine_world_views::scene_staging::placed_draw_live` - the same kernel the browser play page
    /// asks.
    pub(super) field_placement_window_keys:
        Vec<Option<legaia_engine_scene::field_env::PlacedWindowKey>>,
    pub(super) field_placement_color_window_keys:
        Vec<Option<legaia_engine_scene::field_env::PlacedWindowKey>>,
    /// Grid cell + record cull radius per `field_placement_draws` entry, for
    /// the placed-object actor cull (`field_view_window::placed_actor_visible`).
    pub(super) field_placement_cell_keys: Vec<legaia_engine_scene::field_view_window::CellKey>,
    /// The same, per `field_placement_color_draws` entry.
    pub(super) field_placement_color_cell_keys:
        Vec<legaia_engine_scene::field_view_window::CellKey>,
    /// The bind record of each draw of `field_placement_draws` /
    /// `field_placement_color_draws` (parallel lists, `None` = unbound): the
    /// key the per-frame pass looks a script's live object displacement up by
    /// (`World::object_draw_displacements` - `chitei2`'s falling boulder).
    pub(super) field_placement_records: Vec<Option<usize>>,
    pub(super) field_placement_color_records: Vec<Option<usize>>,
    /// Scene-bank model id (= env-pack slot) -> uploaded mesh, for a placed
    /// object whose motion stream swapped its model (op `0x0E`,
    /// `World::object_live_models`); textured and colour bridges.
    pub(super) field_pack_meshes: Vec<Option<usize>>,
    pub(super) field_pack_color_meshes: Vec<Option<usize>>,
    /// Field-scene **terrain / ground** draws: `(uploaded-mesh index, world
    /// model)` per visible cell of the field `.MAP` object grid
    /// (`Scene::field_terrain_tiles`, the `CELL_VISIBLE` sweep - the dense
    /// ground / floor layer, as opposed to the placed-flag interactive objects
    /// in `field_placement_draws`). Empty for scenes with no field-map terrain.
    /// Drawn in `SceneMode::Field` UNDER the placed buildings so the town rests
    /// on its ground instead of floating over the bare clear colour.
    pub(super) field_terrain_draws: Vec<(usize, Mat4)>,
    /// The field's **light-source** row shading
    /// (`legaia_engine_field::field_lit_mesh`): per env mesh with lit-row
    /// vertices, the CPU mesh + its lit vertices (keyed by `meshes` index),
    /// the shaded per-rotation variants, which variant each terrain /
    /// placement draw takes (parallel to the two draw lists) and the light
    /// they were shaded under - a different live light rebuilds them.
    pub(super) field_lit: FieldLitMeshes,
    /// `LEGAIA_DIAG_DRAWS=<path>`: the per-draw census beside the uploaded
    /// meshes, written as texture families every frame
    /// (`legaia_engine_system::draw_census`). `None` with the variable unset.
    pub(super) draw_census: Option<DrawCensusDiag>,
    /// Live floor-height-ladder patch for the four field draw lists above: the
    /// per-draw ladder rungs plus the ladder currently folded into their Y, so
    /// a script that sets a rung oscillating (op `0x4C` nibble-9) moves the
    /// drawn ground and not only the walk heightfield. See
    /// [`field_render::FieldFloorWave`].
    pub(super) field_floor_wave: field_render::FieldFloorWave,
    /// Cross-draw coplanar-surface offsets for the current scene's resolved
    /// [`legaia_engine_scene::field_env::EnvDraw`] lists (terrain + placed
    /// layers combined), from `legaia_engine_scene::coplanar_draws`. Applied to
    /// each matching draw's world translation so overlapping same-plane tiles
    /// resolve to a stable winner instead of z-fighting (retail painter-orders
    /// them through the ordering table). Rebuilt per scene in `upload_assets`.
    pub(super) coplanar_env_offsets:
        std::collections::HashMap<legaia_engine_scene::field_env::EnvDraw, [f32; 3]>,
    /// Terrain tiles whose mesh is untextured (`F*`/`G*` vertex-colour prims
    /// only): `(index into `color_meshes`, world model)`, resolved through the
    /// colour-mesh bridge the same way `field_placement_color_draws` is. The
    /// textured bridge drops these tiles entirely (their VRAM mesh is empty),
    /// which rendered as holes in the town floor.
    pub(super) field_terrain_color_draws: Vec<(usize, Mat4)>,
    /// The field player's **untextured** mesh half: `(index into
    /// `color_meshes`, actor slot)`. The character field meshes are hybrids -
    /// pants / sleeves are F*/G* colour prims the VRAM pipeline can't carry -
    /// so the posed player draws in two passes; this one follows the actor's
    /// live model matrix each frame (unlike the static colour draws above).
    pub(super) player_color_draw: Option<(usize, usize)>,
    /// Field NPC / animated-prop draws, one per visible MAN partition-1
    /// placement (the retail per-scene actor pool). Positions are live: the
    /// draw follows `World::npcs.positions` (motion-VM walkers move),
    /// falling back to the record's spawn tile. Built at scene load in
    /// `upload_assets`.
    pub(super) field_npc_draws: Vec<FieldNpcDraw>,
    /// Per-NPC looping ANM clip players (the scene-bundle record named by the
    /// placement's anim byte), keyed by placement slot - drives live clip
    /// playback for the placed NPCs (idle sway / walk cycles). Rebuilt with
    /// `field_npc_draws`; `npc_anim_srcs` holds every drawn NPC's TMD + raw
    /// bytes (cut to its spawn clip's bone count when it spawned with one)
    /// for the per-frame posed re-upload (the same rebuild path the player's
    /// idle/walk pair uses) - a slot with no spawn clip takes its first one
    /// from an ANIMATE cue.
    pub(super) npc_clip_players:
        std::collections::HashMap<u8, legaia_engine_field::field_anim::FieldClipPlayer>,
    pub(super) npc_anim_srcs: std::collections::HashMap<u8, (legaia_tmd::Tmd, Vec<u8>)>,
    /// The mesh source + rest pose of every NPC **without** a clip player,
    /// kept so an op-`0x4B` VDF morph on it (`World::npc_morphed_tmd`) can
    /// re-stage its static mesh. Clip-driven NPCs morph through the pose
    /// pass off `npc_anim_srcs` instead.
    pub(super) npc_rest_srcs: std::collections::HashMap<u8, NpcRestSrc>,
    /// The re-staged static meshes of clip-less NPCs whose morph lanes are
    /// live, keyed by placement slot; drawn in place of the rest upload.
    pub(super) npc_morph_static: std::collections::HashMap<u8, NpcPosedHalves>,
    /// Memoised posed NPC meshes, keyed by `(placement slot, clip frame)`.
    ///
    /// An NPC's clip is a short **loop** over a fixed set of poses, so the
    /// posed mesh for a given `(slot, frame)` is a constant: rebuilding and
    /// re-uploading it every render frame recomputes the same bytes. The first
    /// visit to a frame skins + uploads it, every later visit reuses the GPU
    /// buffers. Bounded by `NPCs × clip length` (tens of small meshes).
    ///
    /// Invalidated wholesale when `upload_assets` rebuilds `npc_clip_players`
    /// (scene change), and per-slot when a channel clip cue (`A2` ExecMove / `4C 51` run) swaps
    /// that slot's clip - a new clip reuses the same low frame indices, so its
    /// entries must not alias the outgoing clip's.
    pub(super) npc_pose_cache: NpcPoseCache,
    /// Correctness oracle for [`Self::npc_pose_cache`], populated only under
    /// `LEGAIA_POSE_CACHE_VERIFY=1`: the bone transforms each cached key was
    /// built from. On every cache *hit* the incoming pose is compared against
    /// the stored one and a mismatch is logged as an error.
    ///
    /// Skinning is a pure function of `(TMD, bone transforms)`, so equal poses
    /// on a hit prove the cached GPU mesh is the mesh a rebuild would have
    /// produced - i.e. that the `(slot, frame)` key never aliases across
    /// clips. Empty (and free) when the env var is unset.
    pub(super) npc_pose_verify: NpcPoseVerify,
    /// The ANM bundles the placements resolved their clips through,
    /// retained past scene load so channel clip cues (`A2` / `4C 51`)
    /// (`World::npcs.anim_cues`) can re-target an NPC's clip player
    /// mid-scene (the prologue-vignette "characters doing things" beats).
    /// `.0` = the per-scene bundle, `.1` = the party locomotion bundle
    /// (PROT 0874 §1); `npc_bundle_special[slot]` records which of the two
    /// a placement poses from (`special_model` placements use the
    /// locomotion bundle).
    pub(super) npc_anim_bundles: (
        Option<legaia_actor_formats::player_anm::PlayerAnmBundle>,
        Option<legaia_actor_formats::player_anm::PlayerAnmBundle>,
    ),
    pub(super) npc_bundle_special: std::collections::HashMap<u8, bool>,
    /// The entered scene's actors are still to be bound to the world
    /// (`scene_actors::bind_scene_actors`): raised with each new scene's
    /// resources, lowered by the first `upload_assets` after it, renderer or
    /// not. A mid-scene re-upload (a model re-bind) does not bind again.
    pub(super) scene_actors_pending: bool,
    /// World-map continent terrain draws: `(uploaded-mesh index, world model)`
    /// per visible tile of the kingdom's `.MAP` object grid
    /// (`Scene::field_terrain_tiles`, the dense `FUN_801F69D8` continent layer).
    /// Empty off the world map. Drawn in `SceneMode::WorldMap` so the overworld
    /// shows its tiled ground / trees / mountains rather than a handful of
    /// landmark objects.
    pub(super) world_map_terrain_draws: Vec<(usize, Mat4)>,
    /// The untextured `F*`/`G*` half of the same world-map stamps: `(index
    /// into `color_meshes`, world model)`, resolved through the colour-mesh
    /// bridge exactly like `field_terrain_color_draws`. The landmark pack
    /// meshes are hybrids - Rim Elm's hut walls are textured quads, its four
    /// roofs are gouraud triangles - and retail's per-prim dispatch draws
    /// both families (the untextured slots of the world-map overlay's
    /// `0x801F8968` row are populated; see docs/subsystems/world-map.md), so
    /// a world-map frame that draws only `world_map_terrain_draws` shows
    /// roofless huts.
    pub(super) world_map_terrain_color_draws: Vec<(usize, Mat4)>,
    /// Parallel to `world_map_terrain_draws` / `_color_draws`: each
    /// landmark draw's bind record (`None` for a decoration), the key of
    /// its op-`4C 81` draw tint (`World::object_draw_tints`).
    pub(super) world_map_terrain_records: Vec<Option<usize>>,
    pub(super) world_map_terrain_color_records: Vec<Option<usize>>,
    /// Where the decoration layer starts in `world_map_terrain_draws` /
    /// `world_map_terrain_color_draws` (landmarks first): the draws from here
    /// on carry retail's per-object decoration depth cue
    /// (`overworld_ground_cue::decoration_draw_cue`).
    pub(super) world_map_deco_start: (usize, usize),
    /// Bulk **ground**: the heightfield surface built from the scene's
    /// `.MAP` floor grid (`Scene::walk_heightfield`), textured per cell from
    /// the terrain-type-keyed atlas (record `+0x14`/`+0x15`/`+0x16`). `None`
    /// for scenes with no field map / no `0x1000` ground layer. Drawn as the
    /// continent ground in `SceneMode::WorldMap` and as the town floor in
    /// `SceneMode::Field` (under the `0x2000` decor tiles + placed objects).
    /// Kept out of `meshes` (it has no `Tmd` / actor binding); drawn directly
    /// with a constant Y-flip model.
    pub(super) ground_heightfield: Option<UploadedVramMesh>,
    /// The ground's CPU mesh, kept so the visible-tile crop can re-upload a
    /// cropped index list without rebuilding the heightfield.
    pub(super) ground_src: Option<GroundSource>,
    /// The cropped ground for the current cell rectangle, keyed by
    /// `field_view_window::ViewCells::stamp`; rebuilt only when it moves.
    pub(super) ground_crop: Option<(u32, Option<UploadedVramMesh>)>,
    /// Grid cell + cull radius per `field_terrain_draws` entry, for the
    /// visible-tile crop (`field_view_window::terrain_draw_visible`).
    pub(super) field_terrain_cell_keys: Vec<legaia_engine_scene::field_view_window::CellKey>,
    /// The same, per `field_terrain_color_draws` entry.
    pub(super) field_terrain_color_cell_keys: Vec<legaia_engine_scene::field_view_window::CellKey>,
    /// Per `field_terrain_draws` entry: a camera-facing cell's record flags
    /// and rotation (`EnvDraw::view_skip`), re-placed every frame.
    pub(super) field_terrain_facing: Vec<Option<field_render::CameraFacing>>,
    /// The same, per `field_terrain_color_draws` entry.
    pub(super) field_terrain_color_facing: Vec<Option<field_render::CameraFacing>>,
    /// `C`-key toggle: when `true`, the field render uses the wide debug
    /// orbit vantage (`camera_mvp`) instead of the retail follow camera
    /// (`camera_view::field_follow_view`). Defaults to the retail view.
    pub(super) field_debug_camera: bool,
    /// The pause menu is up because the title's Options row opened it, not
    /// because Start did: its sub-screen's exit closes the whole menu and
    /// returns to the title (retail's title reaches the options screen and
    /// comes straight back). The browser play page keeps the same flag on
    /// its `PlayMenu`.
    pub(super) menu_from_title: bool,
    /// Kingdom slot-4 clip-bank inspection overlay, as raw line geometry
    /// `(positions, colors, line-indices)` in world space. `Some` only on a
    /// world-map scene when `LEGAIA_WORLDMAP_SLOT4=1` is set; `None`
    /// otherwise. Merged into the per-frame world-map `overlay_lines` buffer
    /// alongside the entity/player markers.
    ///
    /// Slot 4 is the world map's **animation bank**, not an object-mesh
    /// library, so each segment is a step of one animated part's decoded
    /// 12-bit translation across its clip (`translation_path_segments`) -
    /// object-local model space, drawn about the world origin because no
    /// actor owns these paths yet. See docs/formats/world-map-overlay.md.
    pub(super) world_map_slot4_lines: Option<LineGeometry>,
    /// World-map water/CLUT-cell animation. `Some` for a world-map kingdom
    /// scene: normally the disc-derived slot-5 CLUT-walk table (eight
    /// independent 16x1 `MoveImage` walkers - ocean head + shoreline/terrain
    /// shimmer cells), with the legacy single-cell ocean-head cycle kept only
    /// as a fallback for a bundle without slot 5. `None` off the world map.
    pub(super) ocean_anim: Option<WaterAnim>,
    /// Pristine CPU-side scene VRAM, cloned at scene-load before any battle
    /// edits. A battle injects monster texture pools into a working copy and
    /// re-uploads; leaving battle restores this base so the field renders
    /// with clean VRAM. `None` until the first scene loads.
    pub(super) cpu_vram_base: Option<legaia_tim::Vram>,
    /// The scene VRAM as it was before a dance staged its own HUD texture
    /// page over it (`stage_dance_hud_art`). `Some` exactly while a dance
    /// owns those rects; restoring from it is what makes the field's texels
    /// byte-exact on exit instead of re-derived.
    pub(super) dance_vram_restore: Option<legaia_tim::Vram>,
    /// Scene-mode from the previous frame, used to detect Field<->Battle
    /// transitions so monster meshes are uploaded / dropped exactly once.
    pub(super) prev_scene_mode: Option<SceneMode>,
    /// The field-to-battle intro transition's per-frame emitter, live only
    /// while the encounter session sits in its `Transition` phase.
    ///
    /// Owns the selected style's working set between frames and turns it into
    /// screen-space primitives; the transition *state machine* it rides is
    /// `World::battle.intro`, driven by `World::tick_encounter`. Armed by
    /// `take_battle_intro_prims` on the first transition frame and dropped when
    /// the phase ends. See `legaia_engine_render::battle_intro`.
    pub(super) battle_intro: Option<legaia_render_kernels::battle_intro::BattleIntro>,
    /// GPU upload of the intro's captured VRAM page, kept for the whole
    /// transition so every frame's strips / tiles sample the captured field
    /// frame (and the shatter's shade page), not just the capture frame's.
    /// Dropped with `battle_intro`.
    pub(super) battle_intro_vram: Option<legaia_engine_render::UploadedVram>,
    /// Lazily-cached monster stat archive (PROT 867) bytes, decoded once and
    /// reused for every battle so each transition doesn't re-decompress 16 MB.
    pub(super) monster_archive: Option<std::sync::Arc<Vec<u8>>>,
    /// `meshes.len()` at battle entry: the boundary appended battle monster
    /// meshes start at, so leaving battle truncates back to it.
    pub(super) battle_mesh_base: usize,
    /// The CPU-side rest mesh of every battle body registered this battle,
    /// keyed by its `meshes` index. A body that draws without a pose frame
    /// draws this mesh, and when its colour word raises semi-transparency
    /// (the capture / defeat fade, the near-camera ghost) the posed-override
    /// builder re-uploads it with `BattleActorDrawPlan::apply_body_blend`, as
    /// the browser page re-uploads its blended stream. Cleared on exit.
    pub(super) battle_rest_vmesh: std::collections::HashMap<usize, legaia_tmd::mesh::VramMesh>,
    /// `color_meshes.len()` at battle entry - the twin of `battle_mesh_base`
    /// for the untextured pipeline, so the backdrop shell's colour half is
    /// truncated away on battle exit alongside the textured meshes.
    pub(super) battle_color_mesh_base: usize,
    /// The battle VRAM (field base + injected monster textures) stashed by
    /// `enter_battle_render`, so a mid-battle player-summon spawn can inject its
    /// own creature texture into a clone and re-upload.
    pub(super) battle_vram: Option<legaia_tim::Vram>,
    /// Upload generation (see [`legaia_engine_render::UploadedVram::generation`])
    /// of the battle VRAM currently expected to be GPU-resident. `Some` only
    /// while a battle texture is uploaded; any other upload path bumping the
    /// renderer's generation mid-battle is a residency violation (the
    /// white-speckle class of bug), caught + self-healed by
    /// `check_battle_vram_residency`.
    pub(super) battle_vram_generation: Option<u64>,
    /// Per-member battle facial-animation state (face tracks + last stamp
    /// set), populated by `enter_battle_render` for each assembled party
    /// member whose band pixels are the real texture-pool uploads. Driven
    /// per tick by `tick_battle_vram_restamps`; cleared on battle exit.
    pub(super) battle_faces: Vec<BattleMemberFace>,
    /// The static SCUS face-frame and victory-mouth tables, lazily read from
    /// the boot source's `SCUS_942.54` on first battle entry
    /// (`legaia_engine_session::battle_vram::BattleFaceTables`, the page's
    /// too). Unavailable after a failed attempt (disc-free runs) - facial
    /// animation is skipped.
    pub(super) battle_face_tables: legaia_engine_session::battle_vram::BattleFaceTables,
    /// The opt-in developer menu (`LEGAIA_DEV_MENU=1`), created on the first
    /// ticked frame. `None` on every default run, which is what keeps the
    /// dev screen out of a player build.
    pub(super) dev_menu: Option<legaia_engine_menus::dev_menu_host::DevMenuSession>,
    /// This frame's dev-menu row-list draws, rebuilt by `tick_dev_menu` and
    /// appended by the (immutable) HUD builder. Empty unless the menu is on.
    pub(super) dev_menu_draws: Vec<TextDraw>,
    /// Whether the dev menu is showing the battle-records page
    /// (`FUN_801ED710`) instead of its row list. Square toggles it while the
    /// list has the pad; retail reaches the same readout from a row of the
    /// world-map dev menu this screen stands in for.
    pub(super) dev_menu_records: bool,
    /// The fishing HUD's five one-shot banner timers (hook / reel-in / miss /
    /// auxiliary / strike splash). Seeded from the session's phase edges and
    /// serviced once a frame in the redraw handler; the draws they produce are
    /// cached in `fishing_banner_draws` for the (immutable) HUD builder.
    pub(super) fishing_banners: legaia_engine_ui::FishingBanners,
    /// This frame's live fishing banner draws, produced by servicing
    /// `fishing_banners`. Empty whenever no banner is running.
    pub(super) fishing_banner_draws: Vec<legaia_engine_ui::HudDraw>,
    // The dance pre-song count-in and the Disco King how-to tutorial used to
    // live here, as a host phase holding the parsed game pending. They are
    // `World::minigames.dance_countin` / `dance_tutorial` now, stepped by the
    // world's own dance tick, so the browser play page counts in on the same
    // frames and the **door-warp** entry gets a count-in at all. The HUD
    // builder reads `dance_countin_banner` / `dance_tutorial_frame` off the
    // world. The fishing venue actors moved the same way, onto
    // `World::minigames.fishing_venue`.
    /// The duel overlay's parsed HUD widget table, for resolving chrome
    /// glyph draws (`parse_baka_hud`).
    pub(super) baka_hud_widgets:
        Option<Vec<legaia_minigame_formats::baka_opponents::BakaHudWidget>>,
    /// This frame's resolved chrome draws (the duel's own `BakaChrome`
    /// output): each with its stamped glyph cell rect (`u` paged through
    /// `glyph_u`, plus the record's `v/w/h`) when the draw pages the glyph
    /// strip and the widget table is resident.
    pub(super) baka_chrome_frame: Vec<ResolvedChromeDraw>,
    /// The duel's 3D surface (`legaia_engine_minigame_scenes::baka_duel_scene`): the
    /// engine cache every duel host drives once a frame.
    pub(super) baka_surface: legaia_engine_minigame_scenes::baka_duel_scene::BakaDuelSurface,
    /// This frame's GPU copy of that surface (see
    /// `PlayWindowApp::refresh_baka_duel_gpu`).
    pub(super) baka_gpu: Option<minigames::BakaDuelGpu>,
    /// The Muscle Dome's 3D arena surface - the engine kernel the browser
    /// play page drives too (`legaia_engine_minigame_scenes::muscle_dome_scene`).
    pub(super) muscle_surface: legaia_engine_minigame_scenes::muscle_dome_scene::MuscleDomeSurface,
    /// The dome surface on the GPU while a dome session is live.
    pub(super) muscle_gpu: Option<minigames::BakaDuelGpu>,
    /// The posed dome mesh `muscle_gpu` holds, so a frame whose pose did
    /// not move reuses the GPU buffers instead of building new ones.
    pub(super) muscle_mesh_key: Option<minigames::MeshKey>,
    /// The fishing pond surface (`legaia_engine_minigame_scenes::fishing_scene`) and its
    /// GPU copy while a fishing session is up.
    pub(super) fishing_surface: legaia_engine_minigame_scenes::fishing_scene::FishingSurface,
    pub(super) fishing_gpu: Option<minigames::BakaDuelGpu>,
    /// The slot machine's disc data, decoded on the first visit (`Some(None)`
    /// remembers a disc it did not decode on).
    pub(super) slot_cabinet_assets:
        Option<Option<std::sync::Arc<legaia_engine_ui::slot_cabinet::SlotCabinetAssets>>>,
    /// The machine's VRAM on the GPU while it is on screen.
    pub(super) slot_gpu: Option<UploadedVram>,
    /// The marquee's legend / blink counters.
    pub(super) slot_marquee_clock: legaia_engine_ui::slot_cabinet::SlotMarqueeClock,
    /// This frame's composed dot buffer.
    pub(super) slot_dots: Vec<u8>,
    /// The dance venue on the GPU while the dance entry's globals are staged
    /// (see `PlayWindowApp::sync_dance_venue`).
    pub(super) dance_venue_gpu: Option<minigames::DanceVenueGpu>,
    /// The staging generation whose venue build failed, so a disc without
    /// the venue is not re-read every frame.
    pub(super) dance_venue_failed: Option<u32>,
    /// The dance floor's bodies (`legaia_engine_minigame_scenes::dance_cast_scene`):
    /// the engine surface every dance host poses the cast through.
    pub(super) dance_cast_surface:
        legaia_engine_minigame_scenes::dance_cast_scene::DanceCastSurface,
    /// This frame's GPU copy of that surface (see
    /// `PlayWindowApp::refresh_dance_cast_gpu`).
    pub(super) dance_cast_gpu: Option<minigames::DanceCastGpu>,
    /// Muscle Dome hub-screen atlas + sprite table (see [`MuscleHubAssets`]).
    pub(super) muscle_hub: Option<MuscleHubAssets>,
    /// World actor slot the spawned player-summon creature occupies (`>= 8`, so
    /// it never collides with the party/monster battle slots), or `None`.
    pub(super) summon_actor_slot: Option<usize>,
    /// Mesh index of the battle-stage backdrop dome (the scene's
    /// `scene_tmd_stream` half-dome), drawn behind the actors. `None` when the
    /// scene has no stage or it failed to load.
    pub(super) battle_stage_mesh: Option<usize>,
    /// Index into `color_meshes` for the backdrop shell's **untextured** half -
    /// the `F*`/`G*` flat / gouraud panels (sky, painted wall faces, water
    /// bands) that carry a baked colour word instead of UVs, so the VRAM-mesh
    /// builder drops them. Retail draws them through the same actor as the
    /// textured prims; leaving them out punches holes in the arena shell.
    /// `None` when the stage shell has no untextured prims.
    pub(super) battle_stage_color_mesh: Option<usize>,
    /// Mesh index of the backdrop's drawn slot 1, one copy
    /// (`SceneHost::battle_stage_layers`): the object the backdrop draw
    /// turns about Y every frame - on a stage that keeps object 1, the
    /// horizon mist ribbon. Drawn twice a frame, once per backdrop copy,
    /// under `backdrop_slot_1_basis`. `None` on a one-object draw list.
    pub(super) battle_stage_spun_mesh: Option<usize>,
    /// The untextured half of [`Self::battle_stage_spun_mesh`], in
    /// `color_meshes`.
    pub(super) battle_stage_spun_color_mesh: Option<usize>,
    /// The object list and second-copy transform the stage shell meshes were
    /// built from (`SceneHost::battle_stage_object_indices`). A mid-fight
    /// change - the evolved-Cort arrival's slot-0 rebind - rebuilds them
    /// (`tick_battle_stage_shell`).
    pub(super) battle_stage_shell: Option<(
        Vec<usize>,
        legaia_scene_formats::battle_backdrop::SecondCopy,
    )>,
    /// Mesh index of the flat tiled ground grid drawn under the battle actors
    /// (retail's `func_0x801d02c0` grid), textured from the constant retail
    /// page/CLUT/UV-window address where the scene battle VRAM places its own
    /// ground tile. `None` outside a stage-dome battle.
    pub(super) battle_ground_mesh: Option<usize>,
    /// Far colour (display `0..1`) of the ground grid's GTE depth cue - the
    /// battle backdrop far colour retail stages at `0x8007BB48` and the grid
    /// emitter's four `DPCS` sites consume (capture-pinned: `(0x40,0x40,0x40)`
    /// on ordinary stages, `(0xFE,0xFE,0xFE)` on the `DAT_80078C1C` outdoor
    /// stages). Resolved per battle in `battle_stage::resolve_battle_stage`;
    /// `None` outside a stage-dome battle.
    pub(super) battle_ground_cue_far: Option<[f32; 3]>,
    /// The pre-cue vertex colour the uploaded ground grid was built with -
    /// the battle ambient it last followed (`sync_battle_ground_ambient`).
    pub(super) battle_ground_rgbc: [u8; 3],
    /// Whether the current battle stage is on the `DAT_80078C1C` outdoor
    /// table - the tint pass's `DAT_8007BDA8` input
    /// (`World::battle_actor_draw_plan`). Resolved per battle in
    /// `battle_stage::resolve_battle_stage`; `false` outside a stage-dome battle.
    pub(super) battle_stage_outdoor: bool,
    pub(super) scene_aabb: ([f32; 3], [f32; 3]),
    /// Current held-button bitmask (PSX pad encoding). Updated per key event.
    pub(super) pad: u16,
    /// Presses since the last frame, so a key tapped between two redraws
    /// still reaches a tick (the browser page's `pulse` set).
    pub(super) pad_taps: legaia_engine_system::input::PadTapLatch,
    /// The controller reader (`window::gamepad`); `None` in scripted and
    /// headless runs, in a build without the `gamepad` feature, and when the
    /// platform backend did not start.
    pub(super) gamepad: Option<super::gamepad::GamepadHost>,
    /// The folded controller word the last poll held, kept apart from the
    /// keyboard's [`Self::pad`] so a released button never clears a bit a
    /// key still holds; each tick sees the OR of the two.
    pub(super) gamepad_held: u16,
    /// Pad bits the `--simple-dialogue` picker owned and that are still held
    /// (`step_simple_dialog_picker`): hidden from the world until released.
    pub(super) picker_swallow: u16,
    /// Input binding loaded from file (or default).
    pub(super) mapping: legaia_engine_system::input::Mapping,
    /// Physical keys currently held.
    ///
    /// Only the **Key Config** sub-screen needs this: winit delivers a stream
    /// of `Pressed` events while a key is held, and a rebind that consumed
    /// every one of them would bind the confirm key to the row the instant
    /// the screen opened. The set turns that stream back into one edge per
    /// key. The pad word cannot serve: it only holds keys that are *already*
    /// bound to a button, and the whole point of the screen is to bind ones
    /// that are not.
    pub(super) keys_down: std::collections::HashSet<KeyCode>,
    /// Never write the options or bindings files: set by the headless
    /// window, whose key presses are a test's, not a player's.
    pub(super) persist_disabled: bool,
    /// The most recent fresh key-down's engine key **name**
    /// (`keycode_to_name`), consumed once by the pause menu's Key Config
    /// screen. `None` when nothing new was pressed since the last tick.
    ///
    /// The vocabulary is the binding table's own
    /// ([`legaia_engine_system::input::KEY_NAME_DOM_CODES`]), so the two hosts
    /// bind out of the same key set - a key the table has no name for is not
    /// bindable on either.
    pub(super) pending_key_name: Option<&'static str>,
    /// Menu runtime - drives shop / inn / status screens. Ticked per frame
    /// when `is_open()`; renders shop overlay via `shop_draws_for`.
    pub(super) menu_runtime: legaia_engine_menu_runtime::menu_runtime::MenuRuntime,
    /// Pad state from the previous frame - used to compute newly-pressed bits
    /// for boot-UI edge detection.
    pub(super) prev_pad: u16,
    /// Monotonic world-tick counter (drives the `--screenshot` pad script and
    /// capture timing). Incremented once per simulated tick.
    pub(super) tick_no: u64,
    /// Deterministic screenshot harness (`--screenshot`). When set, the pad
    /// script injects one-tick edges and the render path captures an offscreen
    /// PNG at `capture_tick`, then exits. `None` in normal interactive runs.
    pub(super) screenshot: Option<ScreenshotConfig>,
    /// Next world-tick at which the periodic screenshot sweep
    /// (`--screenshot-every`) captures. Advanced by `every` on each capture;
    /// unused when no sweep is configured.
    pub(super) sweep_next_tick: u64,
    /// Rolling battle-event log surfaced in the HUD. Each tick drains
    /// `World::pending_battle_events` and folds the most recent N entries
    /// into this ring buffer (`ApplyArtStrike` events also get applied to
    /// the target's `BattleActor::hp` via `World::fold_battle_event`). The
    /// log is empty until a battle SM actually fires.
    pub(super) battle_event_log: std::collections::VecDeque<String>,
    /// Encounter-transition banner: `(frames_remaining, formation_label)`.
    /// Armed once per `Field -> Battle` mode change by `sync_battle_render`
    /// (the same single-shot transition edge that stages the battle render),
    /// aged one frame per simulation tick in `drain_and_log_battle_events`,
    /// and drawn by `build_hud` through the shared
    /// `encounter_banner_draws_for` builder while frames remain.
    pub(super) encounter_banner: Option<(u16, String)>,
    /// Damage-popup / status model for the battle HUD. Fed each frame from
    /// `World::drain_battle_hit_fx` (floating numbers) + the live status
    /// tracker (per-slot icons); aged by `BattleHud::tick`. Popups + status
    /// letters are drawn anchored to the per-slot HP rows in `build_hud`.
    pub(super) battle_hud: legaia_engine_battle::battle_hud_state::BattleHud,
    /// Actor slots queued for render-side mesh upload. Populated when a
    /// `FieldEvent::ActorSpawned` fires with a non-`None` `Actor::tmd_ref`
    /// (the field-VM `0x4C 0xD8` synchronous-spawn path); drained in the
    /// next render pass, which materializes a [`UploadedVramMesh`] from
    /// the actor's global-pool TMD, appends it to `self.meshes` /
    /// `self.scene_tmd_data`, and sets `actor.tmd_binding` to the new
    /// mesh index so the per-frame draws iteration picks it up.
    pub(super) pending_dynamic_mesh_slots: Vec<u8>,
    /// Spawn slots whose `tmd_ref` mesh has already been uploaded + bound
    /// (idempotence for the drain above - the binding itself can't be the
    /// marker because `upload_assets` pre-binds every actor slot).
    pub(super) drained_spawn_slots: std::collections::HashSet<u8>,
    /// Tile-board tile-actor slots already queued for the mesh-upload drain.
    /// A board install spawns its actors through `World::spawn_field_actor`
    /// directly (no `ActorSpawned` event fires), so the redraw pass scans
    /// the board draw list and queues each slot once per install; cleared
    /// when the board tears down (draw list empties) so a later board's
    /// re-used slots re-upload their new templates.
    pub(super) tile_slots_queued: std::collections::HashSet<u8>,
    /// Boot-UI state. `BootUiState::Inactive` means the legacy
    /// "go straight to the scene" path; `Title` / `Continue` route
    /// player input through the boot sessions until the player picks
    /// New Game / Continue and the scene becomes interactive.
    pub(super) boot_ui: BootUiState,
    /// A real PSX memory-card image mounted in **port 2** (`--card`), or
    /// `None` for the empty port.
    ///
    /// Owned here because the rack and the block grid are rebuilt from it
    /// every time the save screen asks, and because a Load off that port
    /// reads the block out of these bytes. The browser play page's rack is
    /// the same model (`web-play`'s `cards.rs`): a host-kept card in port 1
    /// and a player's image in port 2.
    pub(super) card: Option<MountedCard>,
    /// The card in **port 1**: the window's own `memcard1.mcr` in the save
    /// directory ([`open_port_one_card`]), the twin of the card the browser
    /// page keeps in its storage. `None` only in a harness that mounts none.
    pub(super) port1_card: Option<MountedCard>,
    /// The save screen's shared driver: block-grid cursor + the card read
    /// behind it. Lives in `engine-core` so this window and the browser play
    /// page step the same cursor and gate the same confirms.
    pub(super) save_flow: legaia_engine_menus::save_screen::SaveScreenFlow,
    /// User-editable settings (BGM / SFX volume, message speed, …). Wired
    /// to the Options screen's [`OptionsSession`] and persisted via
    /// the engine's options round-trip path.
    pub(super) options_state: legaia_engine_menus::options::OptionsState,
    /// The engine-only knobs this session overrides without the player
    /// choosing it (`--retail`, `--no-volumetric-fog`, a recording). A save
    /// writes them as they were before the pin
    /// ([`legaia_engine_menus::options::OptionsPins::persistable`], the
    /// browser page's rule too).
    pub(super) options_pins: legaia_engine_menus::options::OptionsPins,
    /// Opt-in dynamic-lighting enhancement (NON-RETAIL - the field path has
    /// no light source). Seeded from `--dynamic-lighting`, toggled at
    /// runtime with the `I` key; mirrored into the renderer via
    /// [`legaia_engine_render::Renderer::set_dynamic_lighting`] and shown on
    /// the HUD status line. Default `false` = the faithful pixel-identical
    /// render.
    pub(super) dynamic_lighting: bool,
    /// Start in PSX-faithful rasterisation (`--psx-render` or
    /// `LEGAIA_PSX_RENDER=1`), applied to the renderer once it opens.
    pub(super) psx_render: bool,
    /// `--fullscreen` / `--windowed` / `--window-scale`, over the persisted
    /// `OptionsState::fullscreen` / `window_scale` ([`Self::window_setup`]).
    pub(super) window_launch: crate::window::WindowLaunch,
    /// Alt held (`ModifiersChanged`), for the Alt+Enter fullscreen toggle.
    pub(super) alt_held: bool,
    /// The fullscreen key's leading edge (a held key repeats).
    pub(super) fullscreen_key_held: bool,
    /// The occlusion fade's **visibility gate**: the static scene's
    /// world-space triangles (retail Y-down frame), rebuilt per scene load
    /// by `upload_assets` from the same resolved env draw list the render
    /// layers use. Each field frame the redraw pass ray-casts a 5-point
    /// eye->player cross against it and arms the fade only when EVERY
    /// sample is blocked - a partially visible character gets no fade at
    /// all ([`legaia_engine_scene::field_occlusion`]).
    pub(super) field_occluders: legaia_engine_scene::field_occlusion::FieldOccluders,
    /// Eased occlusion-fade strength (0..1) staged as the shader's fade
    /// blend: ramps toward the gate verdict a quarter of the gap per frame,
    /// so the screen-door dissolves in/out instead of popping while the
    /// gate flickers at cover edges. `Cell` because the redraw pass updates
    /// it while the renderer borrow is live.
    pub(super) occl_fade_strength: std::cell::Cell<legaia_engine_scene::field_occlusion::FadeRamp>,
    /// The current scene's derived point lights (world space), rebuilt by
    /// `upload_assets` at scene load and staged into the renderer each
    /// field frame together with the camera's view-projection.
    pub(super) scene_point_lights: Vec<legaia_render_kernels::scene_lighting::ScenePointLight>,
    /// The MAN scene-actor props' light sets (enhanced lighting), each in
    /// world space at its spawn anchor; placed at the actor's live position
    /// per frame by `scene_lighting::place_prop_lights`.
    pub(super) scene_prop_lights: Vec<legaia_render_kernels::scene_lighting::PropLights>,
    /// Mouse drag-orbit state: the last cursor X (window pixels) while the
    /// left button is held, `None` when not dragging. A horizontal drag in
    /// field free-roam rotates `session.camera.manual_orbit`, which both
    /// the follow camera's render yaw and the movement compass
    /// ([`legaia_engine_field::camera::Camera::compass_azimuth_units`]) read
    /// - so orbiting the view keeps "screen up walks away from the camera".
    pub(super) orbit_drag_last_x: Option<f64>,
    /// Mouse drag-orbit state: the last cursor Y, paired with
    /// [`Self::orbit_drag_last_x`]. Vertical drag tilts the follow camera
    /// (`Camera::manual_tilt`) or, under `F3`, steers
    /// [`Self::debug_orbit_pitch`].
    pub(super) orbit_drag_last_y: Option<f64>,
    /// When the left button was last pressed, for the double-click that
    /// resets the follow camera's orbit / tilt / zoom to retail framing
    /// (`Camera::reset_follow_knobs`) - the browser play page's `dblclick`.
    pub(super) last_left_press: Option<std::time::Instant>,
    /// The debug orbit vantage's pitch in radians. Default
    /// `atan(0.85)` - the angle the window's long-standing eye-height ratio
    /// encoded, so an untouched vantage frames exactly as before. Steered by
    /// a vertical left-drag at the browser play page's own rate and clamp
    /// (`window::camera::debug_orbit`).
    pub(super) debug_orbit_pitch: f32,
    /// The debug orbit vantage's continuous zoom, a multiplier on the framing
    /// half-extent the `T` distance preset already scales. Steered by the
    /// wheel at the page's own factors; `1.0` is the preset's own framing.
    pub(super) debug_orbit_zoom: f32,
    /// Latest cursor X (window pixels), fed by `CursorMoved` so a drag that
    /// starts before any motion has an anchor.
    pub(super) cursor_x: f64,
    /// Latest cursor Y, the pitch half of [`Self::cursor_x`]'s anchor.
    pub(super) cursor_y: f64,
    /// Field party-status HUD driver (`FUN_801D0D38`): the idle countdown and
    /// the cached player position the kernel decides from. Retail keeps them
    /// in overlay globals; every host owning a screen keeps its own copy.
    pub(super) field_party_hud: legaia_engine_field::world_map_panel_host::FieldPartyHud,
    /// Scene name the HUD driver was last armed for, so a scene change takes
    /// retail's rearm arm instead of comparing against the previous scene's
    /// player coordinates.
    pub(super) field_party_hud_scene: Option<String>,
    /// Whether the shell's own diagnostic rows draw. Off by default (retail
    /// draws no such text); `F1` toggles, and `LEGAIA_DIAG_HUD` starts it on
    /// so the existing diagnostic env var reaches this surface too.
    pub(super) diag_rows: bool,
    /// When `Escape` was last pressed without quitting: a second press
    /// within [`QUIT_CONFIRM_WINDOW`] quits, and the HUD shows the prompt
    /// meanwhile.
    pub(super) quit_prompt: Option<std::time::Instant>,
    /// The last hotkey's notice line and when it went up; drawn under the
    /// quit prompt for [`NOTICE_DURATION`].
    pub(super) notice: Option<(String, std::time::Instant)>,
    /// The hotkey help panel (`F12` / `?`) is up.
    pub(super) hotkey_help: bool,
    /// The start-up line pointing at that panel ([`StartupHint`]).
    pub(super) startup_hint: StartupHint,
    /// The help key held down, so its auto-repeat does not flip the panel.
    pub(super) help_key_held: Option<KeyCode>,
    /// The title bar text last written (`sync_minigame_caption_title`): the
    /// window's name, plus the live minigame's status caption.
    pub(super) caption_title: String,
    /// Phase J3 pad-capture state. `Some` when the user invoked the
    /// `record` subcommand; the keyboard handler appends transitions
    /// to `events` and the close handler flushes a `j-replay-v1` file
    /// to disk. `None` in plain `play-window` runs.
    pub(super) record_log: Option<RecordLog>,
    /// Field-live options (live loop / player-driven battle / battle BGM)
    /// captured at startup, so the boot-UI NEW GAME handler can re-enter the
    /// opening cutscene scene (`opdeene`) with the same arming the startup
    /// `enter_field_live` used.
    pub(super) field_live_opts: legaia_engine_session::session::FieldLiveOpts,
    /// Extracted-root directory, retained so the in-flow cutscene driver can
    /// resolve a field-VM FMV trigger's `MV*.STR` file (mirrors the headless
    /// `play` loop's `extracted_root.join(rel)`). `None` in disc-only runs,
    /// where the STR is read straight from the ISO via `disc_path`.
    pub(super) extracted_root: Option<std::path::PathBuf>,
    /// Disc image, retained so the in-flow cutscene driver can read a movie's
    /// raw 2352-byte sectors off the ISO and play its interleaved XA audio in
    /// sync with the video. `None` when running from an extracted root (where
    /// the cutscene plays video only, since the extract truncates the audio).
    pub(super) disc_path: Option<std::path::PathBuf>,
    /// Active windowed cutscene playback, when the field VM has flipped the
    /// world into [`SceneMode::Cutscene`] and the resolved STR decoded. While
    /// `Some`, world ticks are suspended and the window shows the video; the
    /// world resumes once the frames drain.
    pub(super) cutscene: Option<WindowedCutscene>,
    /// What the movie in flight did to the score - the engine-side policy
    /// the browser page's FMV lane consults too
    /// ([`legaia_engine_system::movie::movie_audio::MovieScore`]).
    pub(super) movie_score: legaia_engine_system::movie::movie_audio::MovieScore,
    /// Eases the in-engine cutscene camera between Camera Configure beats so
    /// the opening choreography blends instead of cutting, on the shared
    /// retail-display-frame clock (`frame_step::CutsceneGlide`, the browser
    /// page's twin). Reset (snaps) while no cutscene timeline is active and
    /// on every scene entry.
    pub(super) cutscene_glide: legaia_engine_field::frame_step::CutsceneGlide,
    /// Wall-clock to sim-tick accumulator: the shared frame-step rule
    /// (`frame_step::SimStepper`) the browser page drains through too.
    pub(super) sim_stepper: legaia_engine_field::frame_step::SimStepper,
    /// Active dialog box, mirroring `World::dialog.current`. Opened from the
    /// scene's MES container the frame a dialog request appears (field-VM
    /// op `0x3F` or the overworld talk-to), ticked for its typewriter reveal,
    /// and dropped when the world clears the request (the world owns dismissal
    /// via the field VM / overworld handler). `None` when no box is up.
    pub(super) active_dialog: Option<legaia_engine_dialog::dialog::OwnedDialogPanel>,
    /// Spell/seru display-name table, read once from the boot SCUS so the shop's
    /// Trade screens can label each offer ("Gimard (Vahn) -> Orb"). `None` on
    /// disc-free runs or before the first lookup.
    pub(super) seru_names: Option<legaia_game_tables::spell_names::SpellNameTable>,
}

/// Boot-UI state machine. Drives the pre-scene UI when `--boot-ui` is
/// supplied to play-window. The default `Inactive` variant is what
/// every other path uses (no boot UI).
#[allow(clippy::large_enum_variant)]
pub(super) enum BootUiState {
    /// No boot UI - engine ticks the scene normally.
    Inactive,
    /// The boot front end: the publisher logos (PROT 0895), then the title
    /// card and its attract movie - the session's one front end
    /// ([`legaia_engine_session::FrontEnd`]), which the browser play page
    /// steps too. The logo atlas + per-logo rects live on [`PlayWindowApp`]
    /// so the renderer can sample one quad per frame.
    FrontEnd(legaia_engine_session::FrontEnd),
    /// The title's Continue screen: the retail save-select over the two
    /// card ports, the session's one owner
    /// ([`legaia_engine_session::ContinueScreen`]), which the browser play
    /// page steps too. This arm supplies the ports and lands a loaded save.
    Continue(legaia_engine_session::ContinueScreen),
    /// Field (pause) menu is active. The menu session itself is hosted by
    /// the [`NativeSession`]
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
        sub: Option<legaia_engine_menu_runtime::field_menu_dispatch::FieldMenuSubsession>,
    },
    /// Game-over panel is active after a party wipe.
    #[allow(dead_code)]
    GameOver(legaia_engine_menus::game_over::GameOverSession),
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
pub(super) const FIELD_WORLD_FLIP: Mat4 = Mat4::from_cols_array(&[
    1.0, 0.0, 0.0, 0.0, //
    0.0, -1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
]);

/// The retail battle world scale: the battle base matrix `0x8007BF10` is
/// `16384 * I` (GTE `4096` = 1.0) in every catalogued battle savestate - a
/// **4.0x uniform scale** composed under the camera rotation. The one value
/// both hosts read (`legaia_engine_session::battle_fx::BATTLE_WORLD_SCALE`).
/// See [`PlayWindowApp::battle_dome_camera_mvp`].
pub(super) const BATTLE_WORLD_SCALE: f32 = legaia_engine_session::battle_fx::BATTLE_WORLD_SCALE;

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
pub(super) struct FieldNpcDraw {
    /// Partition-1 record index - the key `World::npcs.positions` tracks
    /// live walker positions under.
    pub(super) slot: u8,
    /// Textured mesh half (`self.meshes`), `None` when the model carries no
    /// textured prims.
    pub(super) mesh_idx: Option<usize>,
    /// Untextured F*/G* colour half (`self.color_meshes`).
    pub(super) color_idx: Option<usize>,
    /// Spawn world position (fallback when the world has no live position).
    pub(super) spawn: (i16, i16),
    /// The **raw model id** this draw's mesh was built from, in the operand
    /// space the model pool indexes (`< 0xF0` scene bank, `>= 0xF0` player
    /// bank). Normally the placement's own `model_index`; a scripted mesh
    /// re-bind (motion-VM op `0x0E`) makes it the id
    /// `World::field_npc_live_model` reports, and comparing the two is how
    /// `rebind_live_npc_models` notices a swap the world made after the
    /// upload ran.
    pub(super) bound_model: i16,
}

/// The field env meshes' light-source row shading (see
/// `PlayWindowApp::field_lit` and `legaia_engine_field::field_lit_mesh`).
#[derive(Default)]
pub(super) struct FieldLitMeshes {
    /// `meshes` index -> the processed CPU mesh and its lit vertices, for
    /// every uploaded env mesh carrying a lit-row vertex.
    pub(super) sources: std::collections::HashMap<
        usize,
        (
            legaia_tmd::mesh::VramMesh,
            Vec<Option<legaia_tmd::mesh::LitVertex>>,
        ),
    >,
    /// The shaded variants, one per `(mesh, draw rotation)`.
    pub(super) meshes: Vec<UploadedVramMesh>,
    /// Parallel to `field_terrain_draws`: the variant each draw takes.
    pub(super) terrain: Vec<Option<usize>>,
    /// Parallel to `field_placement_draws`: the variant each draw takes.
    pub(super) placement: Vec<Option<usize>>,
    /// Parallel to `field_posed_props`: the variant each prop's frame-0
    /// rest mesh takes.
    pub(super) posed: Vec<Option<usize>>,
    /// The light the variants were shaded under.
    pub(super) light: Option<legaia_render_kernels::field_light::FieldLight>,
}

/// The scene's CLUT-walk shimmer (`legaia_engine_scene::clut_walk_anim`, the
/// one stepper both hosts run) and this host's vsync count toward the next
/// retail game tick (a game tick spans `World::clock.frame_step` vsyncs - the
/// retail `DAT_1F800393` factor).
pub(super) struct WaterAnim {
    pub(super) anim: legaia_engine_scene::clut_walk_anim::ClutWalkAnim,
    pub(super) vsyncs_to_game_tick: u32,
}

/// The meshes' census for `LEGAIA_DIAG_DRAWS`, aligned with the upload lists
/// it shadows.
#[derive(Default)]
pub(crate) struct DrawCensusDiag {
    /// Parallel to `meshes` (shorter when a later append has no census).
    pub(super) meshes: Vec<Option<legaia_engine_system::draw_census::MeshCensus>>,
    /// Parallel to `field_lit.meshes`.
    pub(super) lit: Vec<legaia_engine_system::draw_census::MeshCensus>,
    /// The full walk-ground mesh and the visible-tile crop of it.
    pub(super) ground: Option<legaia_engine_system::draw_census::MeshCensus>,
    pub(super) ground_crop: Option<legaia_engine_system::draw_census::MeshCensus>,
    /// The VDF morph rebuilds, keyed like `field_morph_live`.
    pub(super) morph:
        std::collections::HashMap<usize, legaia_engine_system::draw_census::MeshCensus>,
}
