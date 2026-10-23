//! Runtime engine bindings for the browser: the **simulation** half of the
//! play page (`site/play.html`).
//!
//! [`LegaiaRuntime`] wraps a real [`legaia_engine_scene_host::scene_host::SceneHost`] -
//! the same host the native `legaia-engine play-window` drives - so the browser
//! runs the ported engine itself, not a re-implementation of it: the field /
//! event VM, the free-movement controller with its per-scene walkability grid,
//! floor-height sampling, NPC motion VMs, the interaction probe, and the
//! inline-script dialogue runner. The page's job each frame is only to hand it
//! a pad word, tick it, and draw what [`crate::play`] reports.
//!
//! ### Minimal mode (no disc)
//! `new()` constructs a bare `World` + `MenuRuntime` - enough to prove the
//! engine VMs compile to `wasm32-unknown-unknown` and the tick path is callable
//! from JS.
//!
//! ### Disc mode (after `load_disc`)
//! `load_disc` builds a `SceneHost` from the user's own image, in memory, in
//! their browser (nothing is uploaded). `enter_field(name)` then boots a named
//! CDNAME scene exactly as the native shell's `enter_field_scene` does, and
//! assembles the render state [`crate::play`] serves to the page.

#[cfg(target_arch = "wasm32")]
use legaia_engine_audio::AudioSink;
#[cfg(target_arch = "wasm32")]
use legaia_engine_audio::WebAudioOut;
use legaia_engine_core::world::World;
use legaia_engine_menu_runtime::menu_runtime::MenuRuntime;
use legaia_engine_menus::menu::{MenuInput, open as menu_open};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene_host::scene_host::{SceneHost, SceneTickEvent};
use wasm_bindgen::prelude::*;

use crate::play::{FieldRender, PlayerRig};

mod audio_exports;
mod disc_load;
mod frame;
mod frame_ticks;
mod input_exports;
mod options;
mod scene_setup;
mod state_json;
pub use frame_ticks::FrameTicksReport;
pub use options::OPTIONS_STORAGE_KEY;
pub(crate) use options::load_persisted_options;

/// Default BGM output gain for the play page, parked on
/// [`legaia_engine_audio::WebAudioOut`]'s post-mixer `GainNode` at
/// [`LegaiaRuntime::audio_init`] and retunable live through
/// [`LegaiaRuntime::audio_set_gain`].
///
/// In page-slider units: the slider's HTML `value` must stay equal to this
/// constant so the control starts where the audio actually is (`1` = "1x").
#[cfg(target_arch = "wasm32")]
const BGM_DEFAULT_GAIN: f32 = 1.0;

/// Bridge object the play page instantiates once. Holds a `World` +
/// `MenuRuntime` for the disc-free path, and - once `load_disc` has run - a
/// `SceneHost` plus the render state for the scene it is running.
#[wasm_bindgen]
pub struct LegaiaRuntime {
    pub(crate) world: World,
    pub(crate) menu: MenuRuntime,
    pub(crate) scene_host: crate::host_slot::HostSlot,
    /// Assembled static map for the current scene.
    pub(crate) field: Option<FieldRender>,
    /// Lead party member's field-form mesh.
    pub(crate) player: Option<PlayerRig>,
    /// Bumped each time [`Self::player`] is rebuilt. A mid-scene rebuild
    /// (a lead change, a `CC F8 50` re-stage) swaps the mesh's topology, so
    /// the page must re-upload the whole geometry, not just the per-frame
    /// posed positions (`FrameTicksReport::player_rig_rebuilt`).
    pub(crate) player_rig_gen: u32,
    /// The tile-board actor mesh staged for upload, one slot at a time
    /// (`crate::play_tile_board`). Board cells all draw the same handful of
    /// template meshes, so the page uploads each slot once per scene.
    pub(crate) tile_mesh: Option<crate::play_tile_board::StagedTileMesh>,
    /// The scene's MAN-placed actor layer (catalog, live clip players, morph
    /// generations, built mesh) - [`crate::field_actors::FieldActors`], the
    /// one implementation the map viewer runs too. Driven one tick per
    /// [`Self::tick_frame`] (the sim clock, not the render clock).
    pub(crate) actors: crate::field_actors::FieldActors,
    /// The scene's ANM bundle (the pose source for scene NPCs **and** placed
    /// props), resolved once per scene the way the native window's
    /// `find_scene_anm_bundle` does: entry-major, descriptor-count seed
    /// `[3, 5, 6, 7]` minor.
    pub(crate) scene_anm: Option<legaia_actor_formats::player_anm::PlayerAnmBundle>,
    /// The PROT 0874 §1 party locomotion bundle - the pose source for the
    /// global-pool specials (save point / party heads).
    pub(crate) locomotion_anm: Option<legaia_actor_formats::player_anm::PlayerAnmBundle>,
    /// The scene's CLUT-walk (water / waterfall shimmer) animation state,
    /// rebuilt at every scene entry with its source strips parked into the
    /// host's VRAM. Only the walker half lives here - the ambient move-VM
    /// tree (jou's palette cyclers / lightning) is spawned into the live
    /// `World` by the scene host and drained by `step_field_vram_fx`.
    pub(crate) field_vram_anim: Option<crate::field_scene_anim::FieldSceneAnim>,
    /// Set when a field VRAM effect (CLUT walker / ambient tree / scripted
    /// CLUT fx) changed texels; drained by [`Self::field_vram_take_dirty`]
    /// so the page re-uploads the VRAM texture only on real changes.
    pub(crate) field_vram_dirty: bool,
    /// Enhanced lighting's derived light sets for the running scene
    /// ([`crate::play_lighting`]), built on the first lit frame after a
    /// scene entry.
    pub(crate) scene_lights: Option<crate::play_lighting::SceneLightCache>,
    /// The field-to-battle transition's per-frame emitter - the same
    /// `legaia_render_kernels::battle_intro::BattleIntro` the native window arms.
    /// `Some` only while the encounter session sits in its `Transition`
    /// phase; owns the captured-field VRAM clone the style bodies sample.
    pub(crate) battle_intro: Option<legaia_render_kernels::battle_intro::BattleIntro>,
    // --- lane B2 ---
    /// The page reads its finished frame back for the intro capture
    /// ([`LegaiaRuntime::play_set_intro_readback`]). Only then does the
    /// arming frame leave the transition layer out for the readback to see
    /// the bare field: a host that never lands a capture (a headless run, a
    /// cached page without the readback) keeps the transition on screen.
    pub(crate) intro_readback: bool,
    /// The between-beat cutscene glide and its display-frame clock - the
    /// shared kernel (`frame_step::CutsceneGlide`) the native window owns
    /// one of too. Without it every `apply > 0` Camera Configure beat snapped
    /// on this host.
    pub(crate) cutscene_glide: legaia_engine_field::frame_step::CutsceneGlide,
    /// The engine camera [`Self::play_camera_vp`] last handed the page: the
    /// matrix and the frame it was built from. The field FX pass reads the
    /// frame back only when the page draws through that exact matrix, so the
    /// parts' `+0x52` camera-relative bits resolve against the camera on
    /// screen and never against one the page replaced with its own orbit.
    pub(crate) engine_camera: Option<(
        [f32; 16],
        legaia_engine_field::camera_frame::FieldCameraFrame,
    )>,
    /// Wall-clock to sim-tick accumulator (`frame_step::SimStepper`), the
    /// native window's frame-step rule. The page's animation loop asks it
    /// how many ticks each display frame runs ([`Self::play_drain_sim_steps`]).
    pub(crate) sim_stepper: legaia_engine_field::frame_step::SimStepper,
    /// This tick's explicit camera-azimuth override (the VR first-person
    /// gaze), drained by the camera tick. `None` = the engine camera's own
    /// compass azimuth drives the d-pad remap, exactly as it does natively.
    pub(crate) camera_azimuth_override: Option<u16>,
    /// Lazily-built scene AABB - the **world-space** union of the scene's
    /// static env draws, through the same
    /// `engine_scene::field_env::env_draws_world_aabb` kernel the native window
    /// calls. Used only by the world map's top-view debug camera. Cleared on
    /// every scene rebuild.
    pub(crate) scene_aabb: Option<([f32; 3], [f32; 3])>,
    /// The `(a, b)` of `ndc = a + b / w` the last field-FX projection
    /// (fog sheets, drop shadows) handed its depths through
    /// (`FogView::depth_affine`) - what the page inverts to put those
    /// depths back into `w` ([`Self::play_fx_depth_affine`]).
    pub(crate) fx_depth_affine: Option<(f32, f32)>,
    /// FMV (STR / MDEC) playback state ([`crate::play_fmv`]).
    pub(crate) fmv: crate::play_fmv::FmvState,
    /// In-world minigame presentation state ([`crate::play_minigames`]):
    /// the draw/input side of the sessions `SceneHost::tick` installs when a
    /// scene script warps into a casino / dance hall / arena.
    pub(crate) minigame_ui: crate::play_minigames::MinigameUi,
    /// Mid-battle VRAM re-upload channel ([`crate::play_battle_vram`]).
    pub(crate) battle_vram: crate::play_battle_vram::BattleVramChannel,
    /// Every file the ISO9660 walk found on the loaded disc image, with its
    /// raw-sector extent, so the page can slice a named file (an `XA*.XA`
    /// voice bank, a `MOV/MV*.STR` movie) out of the bytes it still holds
    /// without the runtime keeping a second 700 MB copy.
    pub(crate) disc_files: Vec<legaia_iso::image::FileEntry>,
    /// Field party-status HUD driver (`FUN_801D0D38`): the idle countdown and
    /// the cached player position its decision kernel reads. The same state
    /// the native window holds - retail keeps it in overlay globals, so every
    /// host that draws a screen owns a copy.
    pub(crate) field_party_hud: legaia_engine_field::world_map_panel_host::FieldPartyHud,
    /// Scene the HUD driver was last armed for, so a scene change takes
    /// retail's rearm arm rather than comparing the new scene's player
    /// position against the old one's.
    pub(crate) field_party_hud_scene: Option<String>,
    /// This frame's passive-ability badge icons, already anchored in 320x240
    /// stage space. Resolved in `tick_field_party_hud` because the anchor
    /// needs the frame's view-projection (a `&mut self` read) and the draw
    /// pass does not have one; see [`crate::play_field_hud`].
    pub(crate) passive_hud_icons: Vec<legaia_engine_field::field_passive_hud::HudIcon>,
    /// The lead's projected stage-Y (240-line PSX space) the page reports
    /// each frame off its own view-projection ([`Self::set_field_player_screen_y`]),
    /// the twin of the native window's `field_hud_projected_player_y`. `None`
    /// until the page reports one.
    pub(crate) field_hud_projected_y: Option<i16>,
    /// Whether the page has reported a projection at all
    /// ([`Self::set_field_player_screen_y`]). Until it has, the HUD kernel
    /// is handed the stand-in; once it has, a `None` above is the lead
    /// genuinely not projecting, which the kernel reads as the native
    /// window's does.
    pub(crate) field_hud_projection_reported: bool,
    /// Actor slots a field-VM `0x4C 0xD8` spawn (`FieldEvent::ActorSpawned`)
    /// gave a TMD reference this session - the native window's
    /// `pending_dynamic_mesh_slots`. The page drains them through
    /// [`Self::play_take_dynamic_mesh_slots`] and uploads each slot's mesh.
    pub(crate) pending_dynamic_mesh_slots: Vec<u8>,
    /// Actor slots the page has uploaded a dynamic (script-spawned) mesh
    /// for, drawn each frame from `play_dynamic_actor_transforms`.
    pub(crate) dynamic_mesh_slots: Vec<u8>,
    /// The dynamic actor mesh staged for the page's upload, one slot at a
    /// time (`play_dynamic_actor_mesh` + the `play_dynamic_mesh_*` reads).
    pub(crate) dynamic_mesh_cur: Option<crate::play::StagedActorMesh>,
    /// This frame's transition primitives, already ordered into drawable
    /// geometry by the shared `screen_prim` builder. Built once per
    /// [`crate::play_battle`] intro tick (the emitter mutates working sets,
    /// so the per-frame accessors must not re-tick) with the primitive count
    /// alongside for the page's early-out.
    pub(crate) battle_intro_geom:
        Option<(u32, legaia_render_kernels::screen_prim::OverlayGeometry)>,
    /// How many of [`Self::battle_intro_geom`]'s leading runs are the
    /// **under** list (drawn before the page's text layer); the rest are
    /// `over`. See `rebuild_screen_geom`.
    pub(crate) screen_over_run_start: u32,
    /// How many of the screen-prim runs - the first ones - are the `under`
    /// list's backdrops, which the page draws before the 3D scene
    /// ([`Self::play_screen_prim_backdrop_run_end`]).
    pub(crate) screen_backdrop_run_end: u32,
    /// The last tick's host-projected screen layers, recomposed with the
    /// world's live layers and the shop / pause fades by
    /// [`crate::play_battle`]'s `rebuild_screen_geom`.
    pub(crate) screen_prims_base: legaia_engine_screens::screen_layers::HostScreenPrims,
    /// The camera-occlusion fade's strength ramp
    /// (`engine-scene::field_occlusion::FadeRamp`), stepped once per drawn
    /// field frame by [`Self::play_occlusion_fade`] - the native window's
    /// `occl_fade_strength` twin.
    pub(crate) occl_fade: legaia_engine_scene::field_occlusion::FadeRamp,
    /// SCUS item-name table, parsed once at `load_disc` - the labels the field
    /// menu's Item screen shows. `None` on a PROT.DAT-only load (no executable).
    pub(crate) item_names: Option<legaia_game_tables::item_names::ItemNameTable>,
    /// The real proportional retail dialog font, decoded straight from the disc
    /// (`PROT.DAT` font TIM + the SCUS width table) at `load_disc` - the same
    /// glyphs + advances the native pause menu draws. `None` (built-in
    /// placeholder used instead) on a PROT.DAT-only load or if the font TIM
    /// doesn't resolve.
    pub(crate) menu_font: Option<legaia_font::Font>,
    /// Disc-sourced pause-menu chrome + font + window table, built lazily the
    /// first time the retail pause menu opens ([`crate::play_menu`]).
    pub(crate) menu_assets: Option<crate::play_menu::PlayMenuAssets>,
    /// Live pause-menu navigation state; `Some` while the menu is up.
    pub(crate) play_menu: Option<crate::play_menu::PlayMenu>,
    /// The boot front end (publisher logos, then the title card) - the
    /// session's one front end ([`legaia_engine_session::FrontEnd`]), which
    /// the native window steps too. `Some` while the logos or the title run
    /// before a scene is entered ([`crate::boot_title`]).
    pub(crate) boot_front_end: Option<legaia_engine_session::FrontEnd>,
    /// The title's Continue screen while it owns the frame - the session's
    /// one owner ([`legaia_engine_session::ContinueScreen`]), the native
    /// window's too ([`crate::boot_continue`]). Retail keeps the title art up
    /// behind it at a dim
    /// ([`crate::boot_title::LegaiaRuntime::boot_title_backdrop_draws_json`]).
    pub(crate) boot_continue: Option<legaia_engine_session::ContinueScreen>,
    /// How many attract hand-offs the title has skipped this session. The
    /// countdown fires the same way it does natively, but this page has no
    /// STR/MDEC playback on the play path, so the movie is skipped and the
    /// count is what the page discloses instead of showing it.
    pub(crate) boot_title_attract_skips: u32,
    /// Disc-sourced title-screen art (PROT 0888), built with the title flow.
    pub(crate) title_atlas: Option<legaia_engine_title::title_screen_atlas::TitleScreenAtlas>,
    /// Disc-sourced publisher-logo atlas (PROT 0895 `init.pak`), built on the
    /// first logo run and kept for the page load.
    pub(crate) boot_logos_atlas: Option<legaia_engine_title::publisher_logos::LogosAtlas>,
    /// The atlas build was attempted and failed (no disc, or `init.pak` did
    /// not parse), so it is not retried every frame.
    pub(crate) boot_logos_failed: bool,
    /// Disc-sourced **menu-glyph** atlas (`legaia_ui_formats::menu_glyph_atlas`) -
    /// the small-caps sheet the title menu's NEW GAME / CONTINUE rows sample
    /// when the title art is absent, exactly as the native window does.
    pub(crate) menu_glyph_atlas: Option<legaia_engine_menus::menu_glyph_atlas::MenuGlyphAtlas>,
    /// The **memory-card rack**: the player's own card images occupying the
    /// console's two ports ([`crate::cards`]). The in-canvas Load / Save
    /// screens read and write these, and the page exports them back out.
    pub(crate) cards: [Option<legaia_save::emu::MountedCard>; crate::cards::CARD_SLOTS],
    /// Per rack slot: an in-game Save wrote the card and the page has not
    /// yet stored it back into browser storage
    /// ([`Self::card_take_written`]). Distinct from the card's own `dirty`
    /// bit, which means "not exported to the emulator yet".
    pub(crate) cards_written: [bool; crate::cards::CARD_SLOTS],
    /// Fishing HUD one-shot banner timers (hook / reel-in / miss / auxiliary /
    /// strike splash), serviced once per sim tick by
    /// [`Self::tick_fishing_banners`] - the browser twin of the native window's
    /// same-named field ([`crate::play_fishing`]).
    pub(crate) fishing_banners: legaia_engine_ui::FishingBanners,
    /// This tick's live banner draws, folded into the fishing HUD list.
    pub(crate) fishing_banner_draws: Vec<legaia_engine_ui::HudDraw>,
    /// Whether [`Self::enter_field`] arms the live gameplay loop (step-driven
    /// random encounters, Field -> Battle -> Field with loot). Defaults on -
    /// the page is the playable host - and is the browser twin of the native
    /// window's `--live-loop` / `--player-battle` flags. [`Self::set_live_battles`]
    /// turns it off for walk-only sessions.
    pub(crate) live_battles: bool,
    /// The save a card **Load** or a save import lifted, parked with its
    /// resume label until the page lands it through
    /// [`Self::play_resume_save`] ([`crate::resume`]), which enters the
    /// saved scene and re-applies the save after the swap - the native
    /// `BootSession::resume_save` order (enter, then load).
    pub(crate) pending_card_resume: Option<crate::resume::ParkedResume>,
    /// Battle<->Field BGM swap track override, the browser twin of the native
    /// window's `--battle-bgm <id>`. `None` = no page-side override, so the
    /// shipped default battle theme plays (`LiveLoopOpts::playable`);
    /// `Some(0)` = swap disabled; any other id replaces the track. Routed
    /// through the same director as field op-`0x35` starts (scene-local ids
    /// via the scene's asset table, `>= 2000` via the global `music_01`
    /// pool). Before this the browser never called `World::set_battle_bgm`
    /// at all, so a battle could not swap music no matter what the page
    /// wanted.
    pub(crate) battle_bgm: Option<u16>,
    /// Battle HUD model (per-slot HP / MP / AP rows, damage popups), refreshed
    /// each battle tick by the shared `engine-core` fold and projected into
    /// the shared `battle_hud_draws_for` builder ([`crate::play_battle`]).
    pub(crate) battle_hud: legaia_engine_battle::battle_hud_state::BattleHud,
    /// Encounter-transition banner: `(frames_remaining, formation_label)`,
    /// armed once per `Field -> Battle` mode edge.
    pub(crate) encounter_banner: Option<(u16, String)>,
    /// Last observed scene mode, so battle enter / exit presentation runs on
    /// mode *edges* (the browser twin of the native `sync_battle_render` latch).
    pub(crate) prev_scene_mode: Option<SceneMode>,
    /// The battle 3D render state (battle VRAM + backdrop / grid / actor
    /// meshes), built on the `Field -> Battle` edge and dropped on exit -
    /// the browser twin of the native `enter_battle_render` working set
    /// ([`crate::play_battle_render`]). `None` outside battle.
    pub(crate) battle_render: Option<crate::play_battle_render::BattleRender>,
    /// Monotonic battle-render build counter; the page re-uploads the battle
    /// scene when `play_battle_generation` changes.
    pub(crate) battle_render_generation: u32,
    /// This frame's built battle FX geometry (effect-pool billboards + the 3D
    /// FX model draw list), rebuilt by `play_battle_fx_sync` and read back by
    /// the `play_battle_fx_*` accessors ([`crate::play_battle_fx`]).
    pub(crate) battle_fx: crate::play_battle_fx::BattleFxFrame,
    /// Actor-table slot the mid-battle summon creature is seated in, once one
    /// has spawned - the browser twin of the native window's
    /// `summon_actor_slot`, kept so a second cast reuses the same seat.
    pub(crate) summon_actor_slot: Option<usize>,
    /// A session-only precise-movement override (VR first-person), laid over
    /// the persisted option by every options apply and never written to the
    /// store. `None` = the player's own setting rules.
    pub(crate) precise_movement_override: Option<bool>,
    /// Raw `SCUS_942.54` bytes (in the visitor's own browser, like the disc
    /// itself), kept for the battle render's per-stage tables: the backdrop
    /// second-copy mirror list (`DAT_80078B50`) and the ground-grid outdoor
    /// cue table (`DAT_80078C1C`). `None` on a PROT.DAT-only load - the
    /// battle render then takes the half-turn / indoor-grey fallbacks.
    pub(crate) scus: Option<Vec<u8>>,
    /// The field VM opened a menu-overlay screen on this tick (the poll in
    /// [`Self::tick_frame`] took its first step), so
    /// [`Self::play_shop_after_tick`] skips its step - the native window's
    /// `shop_opened_this_tick`.
    pub(crate) shop_opened_this_tick: bool,
    /// The page's sound-effect channel: disc descriptor bank, delay scheduler,
    /// footstep cadence ([`crate::play_sfx`]).
    pub(crate) sfx: crate::play_sfx::PlaySfx,
    /// The live party-wipe hand-off, when a wipe raised one: the same
    /// [`legaia_engine_menus::game_over::GameOverSession`] the native window
    /// builds, holding for the same number of frames and resolving to the
    /// same single destination (the title screen). Not a panel - retail draws
    /// nothing here and offers no choice.
    pub(crate) game_over: Option<legaia_engine_menus::game_over::GameOverSession>,
    /// Live WebAudio output + its from-scratch SPU. Crate-visible so the SFX
    /// channel ([`crate::play_sfx`]) can key one-shot cues into the same SPU the
    /// BGM sequencer feeds - one mixer, as on hardware.
    #[cfg(target_arch = "wasm32")]
    pub(crate) audio_out: Option<std::sync::Arc<WebAudioOut>>,
    /// The parsed `SCUS_942.54` equipment stat-bonus table
    /// (`DAT_80074F68`), kept for the shop's retail descriptor windows:
    /// the sell-detail panel's passive chain reads the equip record's `+5`
    /// byte and the stat-compare windows read the bonus columns. `None` on
    /// a PROT.DAT-only load.
    pub(crate) equip_stats: Option<legaia_game_tables::equip_stats::EquipStatTable>,
    /// Spell / seru display names from the same executable, used to label the
    /// shop's seru-trade offers ("give (owner) -> receive"). `None` on a
    /// PROT.DAT-only load, where offers fall back to `Seru NN` exactly as the
    /// native window's do.
    pub(crate) seru_names: Option<legaia_game_tables::spell_names::SpellNameTable>,
    /// The live developer-menu screen, when the visitor's explicit opt-in
    /// has raised one ([`crate::play_dev_menu`]) - the browser twin of the
    /// native window's `LEGAIA_DEV_MENU` session. `None` while the opt-in is
    /// off, which is the shipped default.
    pub(crate) dev_menu: Option<legaia_engine_menus::dev_menu_host::DevMenuSession>,
    /// Whether the dev menu's Records page (Square from the row list) is up.
    pub(crate) dev_menu_records: bool,
    /// The visitor's session-only dev-menu opt-in
    /// ([`LegaiaRuntime::play_dev_menu_set_enabled`]). Deliberately not
    /// persisted and not URL-readable - see [`crate::play_dev_menu`].
    pub(crate) dev_menu_enabled: bool,
    /// Live settings the pause menu's Options screen edits. The native window
    /// keeps the same state on `PlayWindowApp` and persists it to
    /// `legaia-options.toml`; this host keeps it for the session, which is
    /// what makes the screen *remember* an edit. Without it every open
    /// rebuilt from `OptionsState::default()` and every change was discarded
    /// on close - the screen looked wired and did nothing.
    ///
    /// Persisted across page reloads in `localStorage` under
    /// [`OPTIONS_STORAGE_KEY`] - the browser twin of the native window's
    /// `OPTIONS_CONFIG_FILE` TOML round-trip, through the same serde impl.
    /// The commit path is [`Self::persist_and_apply_options`], mirroring the
    /// native `persist_and_apply_options` leg for leg: apply the live audio
    /// side effects, then write the state out.
    pub(crate) options_state: legaia_engine_menus::options::OptionsState,
    /// The engine-only knobs the retail box pins for the session
    /// ([`legaia_engine_menus::options::OptionsPins`]): the store gets them as
    /// they were before the pin, the native window's rule.
    pub(crate) options_pins: legaia_engine_menus::options::OptionsPins,
    /// A title -> load score hand-off the page asked for before entering the
    /// save's scene ([`crate::play_bgm`]'s `play_bgm_title_handoff`): run by
    /// the scene entry once the save has landed, or by the next tick when
    /// the page declined the entry.
    pub(crate) bgm_handoff_pending: bool,
    /// The native launch switches the page throws at runtime
    /// ([`crate::play_toggles`]), re-asserted on every scene entry.
    pub(crate) page_toggles: crate::play_toggles::PageToggles,
}

/// Sentinel [`LegaiaRuntime::set_field_player_screen_y`] reads as "the lead
/// did not project this frame" (behind the near plane, or no view-projection
/// built yet). Out of band on purpose - every in-range stage Y, negative
/// included, is a number the kernel is entitled to compare.
pub const NO_FIELD_PROJECTION: i32 = i32::MIN;

#[wasm_bindgen]
impl LegaiaRuntime {
    #[wasm_bindgen(constructor)]
    pub fn new() -> LegaiaRuntime {
        console_error_panic_hook::set_once();
        let mut world = World::default();
        world.spawn_actor(0).default_pos = legaia_engine_vm::Position::new(0, 0);
        world.mode = SceneMode::Title;
        let menu = MenuRuntime::new("/saves");
        let options_state = load_persisted_options();
        Self {
            world,
            menu,
            scene_host: crate::host_slot::HostSlot::new({
                let mut c = legaia_engine_field::camera::Camera::new();
                c.render_yaw_bias =
                    legaia_engine_field::camera_frame::retail_field_render_yaw_bias();
                // The follow distance is an OPTION, and the window applies it
                // at startup (`window/run.rs`). `Camera::new()`'s own default
                // is `Retail`, so a page that never read the option framed
                // every field frame ~35% closer than the window did.
                c.distance = options_state.camera_distance;
                c
            }),
            field: None,
            player: None,
            player_rig_gen: 0,
            tile_mesh: None,
            actors: Default::default(),
            scene_anm: None,
            locomotion_anm: None,
            field_vram_anim: None,
            field_vram_dirty: false,
            scene_lights: None,
            battle_intro: None,
            intro_readback: false,
            // The host framing bias the retail follow view is rendered with,
            // pushed in exactly where the native window pushes it
            // (`window/run.rs`), through the one shared expression.
            cutscene_glide: Default::default(),
            engine_camera: None,
            sim_stepper: Default::default(),
            camera_azimuth_override: None,
            scene_aabb: None,
            fx_depth_affine: None,
            fmv: Default::default(),
            minigame_ui: Default::default(),
            battle_vram: Default::default(),
            disc_files: Vec::new(),
            battle_intro_geom: None,
            screen_over_run_start: 0,
            screen_backdrop_run_end: 0,
            screen_prims_base: Default::default(),
            occl_fade: Default::default(),
            field_party_hud: Default::default(),
            field_party_hud_scene: None,
            passive_hud_icons: Vec::new(),
            field_hud_projected_y: None,
            field_hud_projection_reported: false,
            pending_dynamic_mesh_slots: Vec::new(),
            dynamic_mesh_slots: Vec::new(),
            dynamic_mesh_cur: None,
            item_names: None,
            menu_font: None,
            menu_assets: None,
            play_menu: None,
            boot_front_end: None,
            boot_continue: None,
            boot_title_attract_skips: 0,
            title_atlas: None,
            boot_logos_atlas: None,
            boot_logos_failed: false,
            menu_glyph_atlas: None,
            cards: [const { None }; crate::cards::CARD_SLOTS],
            cards_written: [false; crate::cards::CARD_SLOTS],
            fishing_banners: Default::default(),
            fishing_banner_draws: Vec::new(),
            equip_stats: None,
            seru_names: None,
            dev_menu: None,
            dev_menu_records: false,
            dev_menu_enabled: false,
            options_state,
            options_pins: Default::default(),
            live_battles: true,
            pending_card_resume: None,
            battle_bgm: None,
            battle_hud: legaia_engine_battle::battle_hud_state::BattleHud::new(),
            encounter_banner: None,
            prev_scene_mode: None,
            battle_render: None,
            battle_render_generation: 0,
            battle_fx: Default::default(),
            summon_actor_slot: None,
            precise_movement_override: None,
            scus: None,
            shop_opened_this_tick: false,
            sfx: Default::default(),
            game_over: None,
            #[cfg(target_arch = "wasm32")]
            audio_out: None,
            bgm_handoff_pending: false,
            page_toggles: Default::default(),
        }
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Frame counter.
    pub fn frame(&self) -> u64 {
        match self.scene_host.host() {
            Some(h) => h.world.frame,
            None => self.world.frame,
        }
    }

    /// Active scene mode as a stable enum string (`Field`, `WorldMap`, ...).
    pub fn scene_mode(&self) -> String {
        match self.scene_host.host() {
            Some(h) => format!("{:?}", h.world.mode),
            None => format!("{:?}", self.world.mode),
        }
    }

    /// Open the disc-free scaffold menu (the headless [`MenuRuntime`] - the
    /// retail pause menu's screens are a native-only draw path today).
    pub fn open_menu(&mut self) {
        menu_open(&mut self.menu.ctx);
    }

    pub fn menu_is_open(&self) -> bool {
        self.menu.is_open()
    }

    pub fn menu_label(&self) -> String {
        self.menu.current_label().to_string()
    }

    /// Tick the scaffold menu with a packed button mask
    /// (`cross | circle<<1 | triangle<<2 | square<<3 | up<<4 | down<<5 |
    /// left<<6 | right<<7`).
    pub fn menu_tick(&mut self, button_mask: u8) -> JsValue {
        let input = MenuInput {
            cross: button_mask & 0x01 != 0,
            circle: button_mask & 0x02 != 0,
            triangle: button_mask & 0x04 != 0,
            square: button_mask & 0x08 != 0,
            up: button_mask & 0x10 != 0,
            down: button_mask & 0x20 != 0,
            left: button_mask & 0x40 != 0,
            right: button_mask & 0x80 != 0,
        };
        let event = self.menu.tick(&mut self.world, input);
        JsValue::from_str(&format!("{event:?}"))
    }
}

impl LegaiaRuntime {
    /// The **active** world: the scene host's once a disc is loaded, the
    /// disc-free scaffold world otherwise. Save import/export
    /// (`crate::session_save`) targets this so a session saved on the play
    /// page captures the world the engine is actually simulating.
    pub(crate) fn world_mut(&mut self) -> &mut World {
        match self.scene_host.host_mut() {
            Some(h) => &mut h.world,
            None => &mut self.world,
        }
    }

    /// The live retail mode word (`_DAT_8007B83C`) - the 28-entry mode
    /// table's index, which is what the native mode-trace oracle samples off
    /// its own seat (`ModeSeat::game_mode`).
    pub(crate) fn mode_word(&self) -> u32 {
        self.scene_host.seat().game_mode().as_index() as u32
    }

    /// Enter an INIT mode **now**, against the active world - the browser's
    /// call of [`legaia_engine_mode::mode::ModeSeat::enter`], the entry point
    /// `BootSession` calls at the same two junctures (field entry through
    /// `MAIN INIT`, the pause menu through `CARD INIT`).
    ///
    /// The per-frame [`Self::tick_mode_seat`] is not a substitute for it.
    /// `adopt_world_mode` writes the RUN word a scene mode maps to and never
    /// the INIT one, so a host that only adopts skips the INIT frame
    /// altogether **and** skips the mode-change edge's pad swallow - which is
    /// what previously delivered the Start press that opened the pause menu to
    /// the menu as its own first input on this host and not on the native one.
    ///
    pub(crate) fn seat_enter(
        &mut self,
        mode: legaia_engine_mode::mode::GameMode,
    ) -> Option<legaia_engine_mode::mode::ModeInitPlan> {
        let (world, seat) = self.scene_host.world_seat_mut(&mut self.world);
        seat.enter(mode, world)
    }
}

impl Default for LegaiaRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl LegaiaRuntime {
    /// The engine session this page runs, once a disc is loaded - read-only,
    /// for native tests that sample it beside another host's
    /// (`legaia_engine_session::host_trace`). Not a wasm export.
    pub fn debug_session(
        &self,
    ) -> Option<&legaia_engine_session::BootSession<legaia_engine_audio::TestAudioSink>> {
        self.scene_host.session()
    }

    /// The page's menu-overlay screen (shop, prize exchange, inn) - read-only,
    /// for the same native tests. Not a wasm export.
    pub fn debug_menu_runtime(&self) -> &legaia_engine_menu_runtime::menu_runtime::MenuRuntime {
        &self.menu
    }

    /// The page's field party-status readout state - read-only, for the same
    /// native tests. Not a wasm export.
    pub fn debug_field_party_hud(
        &self,
    ) -> &legaia_engine_field::world_map_panel_host::FieldPartyHud {
        &self.field_party_hud
    }

    /// Build the page's audio director now, as a browser's `audio_init`
    /// after the first user gesture does (off wasm it runs over a
    /// device-free sink), so a native test compares a page whose audio is
    /// up from the first frame. Not a wasm export.
    pub fn debug_attach_audio(&mut self) -> bool {
        self.audio_director().is_some()
    }
}
