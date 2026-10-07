//! Runtime engine bindings for the browser: the **simulation** half of the
//! play page (`site/play.html`).
//!
//! [`LegaiaRuntime`] wraps a real [`legaia_engine_core::scene::SceneHost`] -
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
use legaia_engine_core::menu_runtime::MenuRuntime;
use legaia_engine_core::scene::{SceneHost, SceneTickEvent};
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::menu::{MenuInput, open as menu_open};
use wasm_bindgen::prelude::*;

use crate::play::{FieldRender, PlayerRig};

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
    pub(crate) scene_anm: Option<legaia_asset::player_anm::PlayerAnmBundle>,
    /// The PROT 0874 §1 party locomotion bundle - the pose source for the
    /// global-pool specials (save point / party heads).
    pub(crate) locomotion_anm: Option<legaia_asset::player_anm::PlayerAnmBundle>,
    /// The scene's CLUT-walk (water / waterfall shimmer) animation state,
    /// rebuilt at every scene entry with its source strips parked into the
    /// host's VRAM. Only the walker half lives here - the ambient move-VM
    /// tree (jou's palette cyclers / lightning) is spawned into the live
    /// `World` by the scene host and drained by `step_field_vram_fx`.
    pub(crate) field_vram_anim: Option<crate::field_scene::FieldSceneAnim>,
    /// Set when a field VRAM effect (CLUT walker / ambient tree / scripted
    /// CLUT fx) changed texels; drained by [`Self::field_vram_take_dirty`]
    /// so the page re-uploads the VRAM texture only on real changes.
    pub(crate) field_vram_dirty: bool,
    /// Enhanced lighting's derived light sets for the running scene
    /// ([`crate::play_lighting`]), built on the first lit frame after a
    /// scene entry.
    pub(crate) scene_lights: Option<crate::play_lighting::SceneLightCache>,
    /// The field-to-battle transition's per-frame emitter - the same
    /// `legaia_engine_ui::battle_intro::BattleIntro` the native window arms.
    /// `Some` only while the encounter session sits in its `Transition`
    /// phase; owns the captured-field VRAM clone the style bodies sample.
    pub(crate) battle_intro: Option<legaia_engine_ui::battle_intro::BattleIntro>,
    /// The between-beat cutscene glide and its display-frame clock - the
    /// shared kernel (`frame_step::CutsceneGlide`) the native window owns
    /// one of too. Without it every `apply > 0` Camera Configure beat snapped
    /// on this host.
    pub(crate) cutscene_glide: legaia_engine_core::frame_step::CutsceneGlide,
    /// The engine camera [`Self::play_camera_vp`] last handed the page: the
    /// matrix and the frame it was built from. The field FX pass reads the
    /// frame back only when the page draws through that exact matrix, so the
    /// parts' `+0x52` camera-relative bits resolve against the camera on
    /// screen and never against one the page replaced with its own orbit.
    pub(crate) engine_camera:
        Option<([f32; 16], legaia_engine_core::camera_view::FieldCameraFrame)>,
    /// Wall-clock to sim-tick accumulator (`frame_step::SimStepper`), the
    /// native window's frame-step rule. The page's animation loop asks it
    /// how many ticks each display frame runs ([`Self::play_drain_sim_steps`]).
    pub(crate) sim_stepper: legaia_engine_core::frame_step::SimStepper,
    /// This tick's explicit camera-azimuth override (the VR first-person
    /// gaze), drained by the camera tick. `None` = the engine camera's own
    /// compass azimuth drives the d-pad remap, exactly as it does natively.
    pub(crate) camera_azimuth_override: Option<u16>,
    /// Lazily-built scene AABB - the **world-space** union of the scene's
    /// static env draws, through the same
    /// `engine_core::field_env::env_draws_world_aabb` kernel the native window
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
    pub(crate) disc_files: Vec<crate::disc::FileEntry>,
    /// Field party-status HUD driver (`FUN_801D0D38`): the idle countdown and
    /// the cached player position its decision kernel reads. The same state
    /// the native window holds - retail keeps it in overlay globals, so every
    /// host that draws a screen owns a copy.
    pub(crate) field_party_hud: legaia_engine_core::world_map_panel_host::FieldPartyHud,
    /// Scene the HUD driver was last armed for, so a scene change takes
    /// retail's rearm arm rather than comparing the new scene's player
    /// position against the old one's.
    pub(crate) field_party_hud_scene: Option<String>,
    /// This frame's passive-ability badge icons, already anchored in 320x240
    /// stage space. Resolved in `tick_field_party_hud` because the anchor
    /// needs the frame's view-projection (a `&mut self` read) and the draw
    /// pass does not have one; see [`crate::play_field_hud`].
    pub(crate) passive_hud_icons: Vec<legaia_engine_vm::field_passive_hud::HudIcon>,
    /// The lead's projected stage-Y (240-line PSX space) the page reports
    /// each frame off its own view-projection ([`Self::set_field_player_screen_y`]),
    /// the twin of the native window's `field_hud_projected_player_y`. `None`
    /// until the page reports one.
    pub(crate) field_hud_projected_y: Option<i16>,
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
    pub(crate) battle_intro_geom: Option<(u32, legaia_engine_ui::screen_prim::OverlayGeometry)>,
    /// The last tick's screen primitives before the shop fade, and the index
    /// the fade goes in at ([`crate::play_battle`]'s `rebuild_screen_geom`).
    pub(crate) screen_prims_base: (Vec<legaia_engine_ui::screen_prim::ScreenPrim>, usize),
    /// SCUS item-name table, parsed once at `load_disc` - the labels the field
    /// menu's Item screen shows. `None` on a PROT.DAT-only load (no executable).
    pub(crate) item_names: Option<legaia_asset::item_names::ItemNameTable>,
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
    /// Boot-chain title-screen session; `Some` while the title runs before a
    /// scene is entered ([`crate::boot_title`]).
    pub(crate) boot_title: Option<legaia_engine_core::title::TitleSession>,
    /// The title session **parked as a backdrop** while the boot Continue
    /// hand-off's save-select owns the screen. Retail keeps the title art up
    /// behind the Load panel at a dim; this page used to release the session
    /// at the hand-off and compose the panel over black
    /// ([`crate::boot_title::LegaiaRuntime::boot_title_backdrop_draws_json`]).
    pub(crate) boot_title_backdrop: Option<legaia_engine_core::title::TitleSession>,
    /// How many attract hand-offs the title has skipped this session. The
    /// countdown fires the same way it does natively, but this page has no
    /// STR/MDEC playback on the play path, so the movie is skipped and the
    /// count is what the page discloses instead of showing it.
    pub(crate) boot_title_attract_skips: u32,
    /// Disc-sourced title-screen art (PROT 0888), built with the title flow.
    pub(crate) title_atlas: Option<legaia_engine_core::title_screen_atlas::TitleScreenAtlas>,
    /// Publisher-logo boot phase, the stage **ahead** of the title card
    /// ([`crate::boot_title`]). `Some` while the logos play.
    pub(crate) boot_logos: Option<legaia_engine_core::publisher_logos::PublisherLogosSession>,
    /// Disc-sourced publisher-logo atlas (PROT 0895 `init.pak`), built on the
    /// first logo run and kept for the page load.
    pub(crate) boot_logos_atlas: Option<legaia_engine_core::publisher_logos::LogosAtlas>,
    /// The atlas build was attempted and failed (no disc, or `init.pak` did
    /// not parse), so it is not retried every frame.
    pub(crate) boot_logos_failed: bool,
    /// Disc-sourced **menu-glyph** atlas (`legaia_asset::menu_glyph_atlas`) -
    /// the small-caps sheet the title menu's NEW GAME / CONTINUE rows sample
    /// when the title art is absent, exactly as the native window does.
    pub(crate) menu_glyph_atlas: Option<legaia_engine_core::menu_glyph_atlas::MenuGlyphAtlas>,
    /// The **memory-card rack**: the player's own card images occupying the
    /// console's two ports ([`crate::cards`]). The in-canvas Load / Save
    /// screens read and write these, and the page exports them back out.
    pub(crate) cards: [Option<crate::cards::MountedCard>; crate::cards::CARD_SLOTS],
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
    pub(crate) battle_hud: legaia_engine_core::battle_hud::BattleHud,
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
    /// The page's sound-effect channel: disc descriptor bank, delay scheduler,
    /// footstep cadence ([`crate::play_sfx`]).
    pub(crate) sfx: crate::play_sfx::PlaySfx,
    /// The live party-wipe hand-off, when a wipe raised one: the same
    /// [`legaia_engine_core::game_over::GameOverSession`] the native window
    /// builds, holding for the same number of frames and resolving to the
    /// same single destination (the title screen). Not a panel - retail draws
    /// nothing here and offers no choice.
    pub(crate) game_over: Option<legaia_engine_core::game_over::GameOverSession>,
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
    pub(crate) equip_stats: Option<legaia_asset::equip_stats::EquipStatTable>,
    /// Spell / seru display names from the same executable, used to label the
    /// shop's seru-trade offers ("give (owner) -> receive"). `None` on a
    /// PROT.DAT-only load, where offers fall back to `Seru NN` exactly as the
    /// native window's do.
    pub(crate) seru_names: Option<legaia_asset::spell_names::SpellNameTable>,
    /// The live developer-menu screen, when the visitor's explicit opt-in
    /// has raised one ([`crate::play_dev_menu`]) - the browser twin of the
    /// native window's `LEGAIA_DEV_MENU` session. `None` while the opt-in is
    /// off, which is the shipped default.
    pub(crate) dev_menu: Option<legaia_engine_core::dev_menu_host::DevMenuSession>,
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
    pub(crate) options_state: legaia_engine_core::options::OptionsState,
    /// A title -> load score hand-off the page asked for before entering the
    /// save's scene ([`crate::play_bgm`]'s `play_bgm_title_handoff`): run by
    /// the scene entry once the save has landed, or by the next tick when
    /// the page declined the entry.
    pub(crate) bgm_handoff_pending: bool,
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
                let mut c = legaia_engine_core::camera::Camera::new();
                c.render_yaw_bias = legaia_engine_core::camera_view::retail_field_render_yaw_bias();
                // The follow distance is an OPTION, and the window applies it
                // at startup (`window/run.rs`). `Camera::new()`'s own default
                // is `Retail`, so a page that never read the option framed
                // every field frame ~35% closer than the window did.
                c.distance = options_state.camera_distance;
                c
            }),
            field: None,
            player: None,
            tile_mesh: None,
            actors: Default::default(),
            scene_anm: None,
            locomotion_anm: None,
            field_vram_anim: None,
            field_vram_dirty: false,
            scene_lights: None,
            battle_intro: None,
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
            screen_prims_base: (Vec::new(), 0),
            field_party_hud: Default::default(),
            field_party_hud_scene: None,
            passive_hud_icons: Vec::new(),
            field_hud_projected_y: None,
            pending_dynamic_mesh_slots: Vec::new(),
            dynamic_mesh_slots: Vec::new(),
            dynamic_mesh_cur: None,
            item_names: None,
            menu_font: None,
            menu_assets: None,
            play_menu: None,
            boot_title: None,
            boot_title_backdrop: None,
            boot_title_attract_skips: 0,
            title_atlas: None,
            boot_logos: None,
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
            live_battles: true,
            pending_card_resume: None,
            battle_bgm: None,
            battle_hud: legaia_engine_core::battle_hud::BattleHud::new(),
            encounter_banner: None,
            prev_scene_mode: None,
            battle_render: None,
            battle_render_generation: 0,
            battle_fx: Default::default(),
            summon_actor_slot: None,
            precise_movement_override: None,
            scus: None,
            sfx: Default::default(),
            game_over: None,
            #[cfg(target_arch = "wasm32")]
            audio_out: None,
            bgm_handoff_pending: false,
        }
    }

    /// Load a disc image from raw in-memory bytes.
    ///
    /// `raw_bytes` may be either a Mode2/2352 full disc image (`.bin`) - PROT.DAT
    /// and CDNAME.TXT are extracted via an ISO9660 walk - or the raw contents of
    /// `PROT.DAT`. `cdname_text` overrides any CDNAME.TXT found on the disc; pass
    /// an empty string to use the disc's own.
    ///
    /// Returns the number of PROT entries parsed. Nothing leaves the browser.
    pub fn load_disc(&mut self, raw_bytes: Vec<u8>, cdname_text: String) -> Result<u32, JsValue> {
        use crate::disc::{extract_cdname_txt, extract_prot_dat, extract_scus, is_mode2_2352_disc};

        self.disc_files = crate::disc::walk_iso_files(&raw_bytes);
        let (prot_bytes, auto_cdname, scus) = if is_mode2_2352_disc(&raw_bytes) {
            let prot = extract_prot_dat(&raw_bytes)
                .ok_or_else(|| JsValue::from_str("load_disc: PROT.DAT not found in disc image"))?;
            let cdname = extract_cdname_txt(&raw_bytes);
            let scus = extract_scus(&raw_bytes);
            (prot, cdname, scus)
        } else {
            (raw_bytes, None, None)
        };

        let cdname_resolved = if !cdname_text.is_empty() {
            Some(cdname_text.as_str())
        } else {
            auto_cdname.as_deref()
        };
        let mut host = SceneHost::from_prot_bytes(prot_bytes, cdname_resolved)
            .map_err(|e| JsValue::from_str(&format!("load_disc: {e}")))?;
        // Item-name labels for the field menu's Item screen (executable-only;
        // a PROT.DAT load has no SCUS and the menu shows raw ids instead).
        self.item_names = scus
            .as_ref()
            .and_then(|s| legaia_asset::item_names::ItemNameTable::from_scus(s));
        // The seru-trade offers' display names. The trade config itself, the
        // menu text and the static progression tables are the session's
        // installs (`BootSession::from_host`, below), as on the native boot.
        if let Some(s) = scus.as_ref() {
            self.seru_names = legaia_asset::spell_names::SpellNameTable::from_scus(s);
        }
        // Sound-effect descriptors from the same executable (`DAT_8006F198`,
        // see docs/formats/sfx-table.md). Data only - the program bank uploads
        // into the SPU lazily once audio is live ([`crate::play_sfx`]).
        if let Some(s) = scus.as_ref() {
            self.install_sfx_descriptors(s);
        }
        // The menu overlay (PROT 0899) also carries the Arrange display-order
        // table (FUN_801D64A8): install it so the Items screen's Arrange
        // command sorts by the retail rank rather than id order.
        if let Ok(overlay) = host
            .index
            .entry_bytes_extended(legaia_asset::menu_windows::MENU_OVERLAY_PROT_INDEX as u32)
        {
            host.world.install_menu_overlay_tables(&overlay);
        }
        // The real retail proportional dialog font, decoded straight from the
        // disc (no save state): the 4bpp font TIM in PROT.DAT + the SCUS width
        // table. This is the exact font the native pause menu draws; without it
        // the menu falls back to the built-in placeholder (fixed-width blocks).
        self.menu_font = host
            .index
            .prot_dat_raw_bytes(
                legaia_font::FONT_TIM_PROT_DAT_OFFSET,
                legaia_font::FONT_TIM_LEN,
            )
            .ok()
            .zip(scus.as_ref())
            .and_then(|(tim, scus)| {
                legaia_font::Font::from_disc_tim_and_scus(&tim, scus)
                    .map_err(|e| crate::console_log(&format!("dialog font decode failed: {e}")))
                    .ok()
                    .map(|font| {
                        // The `0xCE` escape sprites (buttons, icons), from
                        // the boot-resident TIMs at the head of PROT.DAT.
                        use legaia_font::escape_icons::{ICON_PROT_DAT_LEN, ICON_PROT_DAT_OFFSET};
                        match host
                            .index
                            .prot_dat_raw_bytes(ICON_PROT_DAT_OFFSET, ICON_PROT_DAT_LEN)
                        {
                            Ok(head) => font.with_escape_icons_from_disc(&head, scus),
                            Err(_) => font,
                        }
                    })
            });
        // Install the equipment / spell / item catalogs on the host world so the
        // pause menu's Equip / Magic / Items sub-screens read real disc data -
        // the same tables the native `play-window` boot installs in
        // `BootSession::enter_field_live`. Each prefers the disc-accurate
        // real-id table (parsed from `SCUS_942.54`) and falls back to the
        // fabricated-id vanilla catalog on a PROT.DAT-only load. Set once here;
        // they persist across scene entry (`enter_field_scene` never clears
        // them). Without them the sub-sessions build from empty catalogs and the
        // menu falls back to a generic frame.
        self.equip_stats = scus
            .as_ref()
            .and_then(|s| legaia_asset::equip_stats::EquipStatTable::from_scus(s));
        host.world.set_equipment_table(
            self.equip_stats
                .as_ref()
                .map(legaia_engine_core::equipment::equip_modifier_table_from_disc)
                .unwrap_or_else(|| {
                    legaia_engine_core::equipment::vanilla_equipment_catalog().to_modifier_table()
                }),
        );
        // The raw stat-bonus records as well: the Items screen's Throw Out
        // list reads each record's `+7` flags byte, which the derived modifier
        // table does not keep. Twin of the native boot's install in
        // `BootSession::open_with_source`.
        if let Some(table) = self.equip_stats.clone() {
            host.world.set_equip_stats(table);
        }
        // Retail-shaped equipment buy: this page draws the recipient picker
        // (window 36) and the stat-compare windows (25 / 41) over the parked
        // buy list ([`crate::play_shop`]), so opt into the flow and install
        // the disc restrictions the buy-list kind dispatch reads.
        if let Some(table) = self.equip_stats.as_ref() {
            let mut info = legaia_engine_core::equipment::DiscEquipInfo::from_disc(table);
            // The three Goods rows browse a class-2 id space the equipment
            // stat table does not hold; without this index they offer no
            // candidates at all. Twin of the native boot's install in
            // `legaia_engine_shell::boot`.
            if let Some(effects) = scus
                .as_ref()
                .and_then(|s| legaia_asset::item_effect::ItemEffectTable::from_scus(s))
            {
                info.install_goods(&effects);
            }
            self.menu.install_equip_info(info);
            self.menu.retail_equipment_buy = true;
        }
        // Static per-monster steal table (`DAT_80077828`) - the same install the
        // native boot does, so PROT 0941's Steal resolves a monster-seat victim
        // on this host too rather than drawing nothing.
        if let Some(steal) = scus
            .as_ref()
            .and_then(|s| legaia_asset::steal_table::StealTable::from_scus(s))
        {
            host.world.set_steal_table(steal);
        }
        host.world.set_spell_catalog(
            scus.as_ref()
                .and_then(|s| legaia_engine_core::retail_magic::seru_magic_catalog_from_scus(s))
                .unwrap_or_else(legaia_engine_core::retail_magic::retail_seru_magic_catalog),
        );
        host.world
            .set_item_catalog(legaia_engine_core::items::ItemCatalog::vanilla());
        // Real item-effect usability flags (cure/revive = battle-only, etc.);
        // applied after `set_item_catalog` since it rewrites the catalog. Absent
        // on a PROT.DAT-only load (catalog keeps its curated flags).
        if let Some(effects) = scus
            .as_ref()
            .and_then(|s| legaia_asset::item_effect::ItemEffectTable::from_scus(s))
        {
            host.world.set_item_effects(effects);
        }
        // Keep the executable bytes for the battle render's per-stage SCUS
        // tables (mirror list / outdoor-cue list). Nothing leaves the browser.
        self.scus = scus;

        // The session over the host: the native boot's own installs (menu
        // text, seru-trade config, progression tables, battle UI strings and
        // spell anim pairs, gold-shop item data, item effects, the new-game
        // defaults and the CDNAME map-id resolver) run here, through the one
        // constructor both hosts share.
        let count = host.index.entry_count() as u32;
        self.scene_host
            .install(host, self.scus.as_deref())
            .map_err(|e| JsValue::from_str(&format!("load_disc: {e:#}")))?;
        self.field = None;
        self.player = None;
        self.actors.clear();
        self.battle_render = None;
        // A new disc means a new PROT: drop any cached menu chrome / open menu /
        // title art.
        self.menu_assets = None;
        self.play_menu = None;
        self.boot_title = None;
        self.boot_title_backdrop = None;
        self.title_atlas = None;
        self.boot_logos = None;
        self.boot_logos_atlas = None;
        self.boot_logos_failed = false;
        self.menu_glyph_atlas = None;
        // A freshly installed world starts at `World::default()`'s toggles,
        // which are NOT the player's persisted options. The native window
        // re-asserts all four every tick precisely because a scene / New Game
        // transition reseeds world state; this page only pushed them on
        // Options-close, so a persisted "Run" or flash-guard-off never
        // applied at page load and was lost again after a door.
        self.apply_options_side_effects();
        Ok(count)
    }

    /// Raw-sector extent of a named file on the loaded disc image, as
    /// `{"lba": N, "size": bytes}` (`null` when the disc walk found no such
    /// path). `path` is relative to the ISO root, e.g. `XA2.XA` or
    /// `MOV/MV3.STR`; the page slices `discBytes` at `lba * 2352` for
    /// `ceil(size / 2048) * 2352` bytes and hands the sectors back through
    /// the consumer's own install call.
    pub fn disc_file_extent_json(&self, path: &str) -> String {
        let want = path.trim_start_matches('/').to_ascii_uppercase();
        self.disc_files
            .iter()
            .find(|f| f.path.to_ascii_uppercase() == want)
            .map(|f| serde_json::json!({ "lba": f.lba, "size": f.size }).to_string())
            .unwrap_or_else(|| "null".to_string())
    }

    /// `true` if a disc has been loaded.
    pub fn disc_loaded(&self) -> bool {
        self.scene_host.host().is_some()
    }

    /// Boot a named CDNAME scene (e.g. `"town01"`) and assemble everything the
    /// page draws. This is the real field entry: the scene's assets, the
    /// walkability grid + elevation overrides, the MAN system script, the player
    /// install, the encounter session. World-map labels (`map01`..`map03`) route
    /// through the world-map entry, which installs the overworld controller
    /// instead.
    ///
    /// Returns the same JSON as [`Self::state_json`]. Throws when the disc isn't
    /// loaded or the label is unknown.
    pub fn enter_field(&mut self, name: &str) -> Result<String, JsValue> {
        self.enter_field_core(name, false)
            .map_err(|e| JsValue::from_str(&e))
    }
}

impl LegaiaRuntime {
    /// JsValue-free body of [`Self::enter_field`]. `resume` marks a card
    /// load's entry ([`crate::resume`]), whose save the shared kernel
    /// applies after this returns; `false` for a picker / door /
    /// opening-chain entry.
    pub(crate) fn enter_field_core(&mut self, name: &str, resume: bool) -> Result<String, String> {
        let host = self
            .scene_host
            .host_mut()
            .ok_or_else(|| "enter_field: call load_disc first".to_string())?;
        // Faithful-play arming, matching the native play-window's flags:
        // dialogue through the field VM (so branch handlers - flag sets,
        // GIVE_ITEM, scene changes - actually execute), retail's leading-edge
        // wall footprint, solid NPC bodies, per-step terrain follow, and NPCs
        // walking their MAN-authored routes.
        host.world.toggles.use_vm_dialogue = true;
        // The page lands its drawn frame for a framebuffer-reading `43 12`
        // copy (`play_land_frame_grab`), as the native window does.
        host.world.enable_frame_grab(true);
        host.world.locomotion.follow_terrain_height = true;
        host.world.locomotion.leading_edge_wall_probes = true;
        host.world.npcs.solid = true;
        host.world.npcs.animate = true;
        // Free-roam story staging for PICKER entries only - the engine's one
        // rule (`World::stage_picker_entry`), shared with the native
        // `--scene` entry. The opening chain's legs and the prologue skip's
        // `town01` re-enter through here too, and a card Load's resume is not
        // a picker visit either.
        host.world.stage_picker_entry(name, resume);
        let world_map = legaia_engine_core::scene::is_world_map_scene(name);
        if world_map {
            host.enter_world_map_scene(name)
                .map_err(|e| format!("enter_field({name}): {e:#}"))?;
            // Start in walk mode with the retail top-view debug camera
            // reachable through its own chord (`_DAT_8007B98C`), exactly as
            // the native window arms it on world-map entry
            // (`window/run.rs`). The controller, the chord and the camera are
            // all engine-side, so arming the same flag is the whole of what
            // this host needed to gain the top-view vantage.
            if let Some(ctrl) = host.world.world_map.ctrl.as_mut() {
                ctrl.debug_enabled = true;
                ctrl.view_mode = 0;
            }
        } else {
            host.enter_field_scene(name, 0)
                .map_err(|e| format!("enter_field({name}): {e:#}"))?;
        }
        // The scene host cleared the WORLD's camera state (timeline, op-0x45
        // params); this is the engine camera's half, which only an in-world
        // `SceneEntered` tick otherwise runs. Without it a scene picked
        // mid-cutscene kept the interrupted shot's globals and focus latch
        // and was framed by the old scene's camera. The native
        // `BootSession::enter_field_live` is the paired site; the glide
        // interpolator is this host's own and goes with it.
        self.scene_host.camera_mut().reset_for_scene_entry();
        self.cutscene_glide.reset();
        if !world_map {
            // Retail reaches the field through the mode table, not through a
            // call: whoever wants the field stores `MAIN INIT` (2) and mode
            // 2's handler stages the field overlay, calls the per-scene
            // initializer and hands the word to `MAIN MODE` at `0x80025E50`.
            // `BootSession::enter_field_live` performs exactly this pair, and
            // the overworld's entry (`enter_world_map_live`) performs
            // neither - so the branch above is the same branch the native
            // host takes. Without it this page's word arrived at `MAIN MODE`
            // through `adopt_world_mode` alone, one frame late and with no
            // INIT frame in the trace at all.
            let plan = self.seat_enter(legaia_engine_core::mode::GameMode::MainInit);
            debug_assert!(
                matches!(plan, Some(legaia_engine_core::mode::ModeInitPlan::Stage(st))
                    if st.overlay_entry == legaia_engine_vm::title_overlay::FIELD_SCENE_INIT_PC),
                "MAIN INIT's plan should name the per-scene initializer"
            );
        }
        if self.live_battles {
            self.arm_live_battles(name);
        }
        self.rebuild_render_state().map_err(js_error_text)?;
        // The seat heuristic is for interactive free-roam entry; the opening
        // chain's cutscene legs stage their own tableau (the timeline owns
        // actor placement) and must not have the anchor relocated under it.
        let in_opening = self.scene_host.host().is_some_and(|h| {
            h.world.cutscene.opening_chain_active || h.world.cutscene_timeline_active()
        });
        if !in_opening {
            self.seat_player();
        }
        // A deliberate scene boot restages BGM from scratch: clear the dedupe
        // latch, so the scene's own op-`0x35` start is honoured (and re-stages
        // its track's bank) even if it names the track already playing. No
        // scene bank is staged: retail loads a bank only with its track
        // (`legaia_engine_core::scene::SCENE_LOCAL_BGM_FALLBACK_ID`).
        // New scene -> drop any SFX cues still queued for the old one (the
        // native boot's `clear_sfx` on scene entry).
        self.on_scene_change_audio();
        if let Some(d) = self.scene_host.director_mut() {
            d.last_started = None;
        }
        // A card Load's score hand-off lands here: stop whatever was
        // sounding (the title theme), then restore the track the entry
        // started, so the scene's own start of that track finds it already
        // sounding.
        self.run_pending_bgm_handoff();
        Ok(self.state_json())
    }
}

/// A `JsValue` error as text, without touching the JS runtime off-wasm
/// (where every `JsValue` accessor panics) - the testable cores return
/// `String` errors.
fn js_error_text(e: JsValue) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        e.as_string()
            .unwrap_or_else(|| "rebuild_render_state failed".to_string())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = e;
        "rebuild_render_state failed".to_string()
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Route this frame's pad word into the engine. Bit layout is the PSX digital
    /// pad ([`legaia_engine_core::input::PadButton`]): `0x0008` Start, `0x0010`
    /// Up, `0x0020` Right, `0x0040` Down, `0x0080` Left, `0x1000` Triangle,
    /// `0x2000` Circle, `0x4000` Cross, `0x8000` Square. Edge detection is the
    /// engine's - just hand it the held set each frame.
    pub fn set_pad(&mut self, mask: u16) {
        match self.scene_host.host_mut() {
            Some(h) => h.world.set_pad(mask),
            None => self.world.set_pad(mask),
        }
    }

    /// Opt in / out of the engine's continuous locomotion decode
    /// ([`legaia_engine_core::world::FieldLocomotion::precise_movement`]): the camera
    /// azimuth rotates the movement vector at full angular resolution and the
    /// left analog stick ([`Self::set_left_stick`]) supplies an arbitrary
    /// screen angle. The play page's VR first-person mode drives this so
    /// "stick forward" walks exactly where the headset looks; the keyboard
    /// path keeps the retail quantised 8-way remap.
    pub fn set_precise_movement(&mut self, on: bool) {
        match self.scene_host.host_mut() {
            Some(h) => h.world.locomotion.precise_movement = on,
            None => self.world.locomotion.precise_movement = on,
        }
        // Persist it like the native window's `R` toggle does
        // (`legaia-options.toml`), so the page's checkbox survives a reload
        // and a trap-recovery rebuild re-applies it.
        if self.options_state.precise_movement != on {
            self.options_state.precise_movement = on;
            self.persist_and_apply_options();
        }
        // A live session override (VR first-person) still rules the world.
        if let Some(o) = self.precise_movement_override {
            match self.scene_host.host_mut() {
                Some(h) => h.world.locomotion.precise_movement = o,
                None => self.world.locomotion.precise_movement = o,
            }
        }
    }

    /// Lay a session-only precise-movement state over the persisted option,
    /// or lift it (`None`). The VR first-person drive needs free-angle
    /// locomotion for the stick while it is up; it used to get it through
    /// [`Self::set_precise_movement`], which persists, so leaving VR saved
    /// "off" over whatever the player had chosen. The override is re-applied
    /// by every options apply and never reaches the store.
    pub fn set_precise_movement_override(&mut self, on: Option<bool>) {
        self.precise_movement_override = on;
        self.apply_options_side_effects();
    }

    /// Whether precise (free-angle) movement is on - the persisted option
    /// the page's checkbox reflects on load.
    pub fn precise_movement(&self) -> bool {
        self.options_state.precise_movement
    }

    /// The lead's projected screen Y this frame in 240-line stage space, or
    /// [`NO_FIELD_PROJECTION`] for "not projectable" - the browser twin of
    /// the native window's `field_hud_projected_player_y`. The field party
    /// HUD's decision kernel reads it (retail compares the projected player
    /// against a band before the readout returns).
    ///
    /// The sentinel is an out-of-band value rather than "negative": a lead
    /// projected ABOVE the top of the stage has a negative stage Y, and the
    /// native window reports it as a number. Folding that into "no
    /// projection" narrowed the channel on this host only, which is exactly
    /// the shape `docs/tooling/host-drift.md` names.
    pub fn set_field_player_screen_y(&mut self, stage_y: i32) {
        self.field_hud_projected_y = (stage_y != NO_FIELD_PROJECTION)
            .then(|| stage_y.clamp(i16::MIN as i32, i16::MAX as i32) as i16);
    }

    /// Read back the value [`Self::set_field_player_screen_y`] holds, or
    /// [`NO_FIELD_PROJECTION`] for "not projectable" - for the page's
    /// diagnostics and the parity ladder, which needs the number the
    /// decision kernel is about to compare rather than the draw it produces.
    pub fn field_player_screen_y(&self) -> i32 {
        self.field_hud_projected_y
            .map_or(NO_FIELD_PROJECTION, i32::from)
    }

    /// Establish a fresh New Game slate - the browser twin of the native
    /// `BootSession::begin_new_game`: `World::begin_new_game` (flags, money,
    /// bag, clock, pending transitions) plus the SCUS starting party + bag.
    /// The page used to enter `opdeene` without this, so after a Continue,
    /// a card import or a picker visit a "New game" kept the old roster,
    /// gold and story flags.
    pub fn begin_new_game(&mut self) {
        // The title theme hands the score to the field: stop it so the
        // prologue's own BGM (or its scripted silence) owns the audio.
        if let Some(d) = self.scene_host.director_mut() {
            use legaia_engine_core::scene::BgmDirector;
            d.stop();
        }
        let Some(host) = self.scene_host.host_mut() else {
            self.world.begin_new_game();
            return;
        };
        let defaults = host.new_game_defaults.as_ref();
        host.world.begin_new_game_seeded(
            defaults.map(|d| &d.party),
            defaults.and_then(|d| d.inventory.as_ref()),
        );
    }

    /// Route this frame's left analog stick into the engine. PSX convention:
    /// signed bytes, X right-positive, Y **down**-positive; only read by the
    /// precise-locomotion decode ([`Self::set_precise_movement`]).
    pub fn set_left_stick(&mut self, x: i8, y: i8) {
        match self.scene_host.host_mut() {
            Some(h) => h.world.input.set_lstick((x, y)),
            None => self.world.input.set_lstick((x, y)),
        }
    }

    /// Override where the engine thinks the camera is looking for the next
    /// tick, so the free-movement controller remaps the d-pad
    /// camera-relative ("up" walks away from the camera). PSX 12-bit angle
    /// units (`4096` = a full turn); the field controller quantises it to the
    /// nearest quarter-turn, as retail does.
    ///
    /// An **override**, not the normal path: the engine camera publishes its
    /// own compass azimuth every tick
    /// ([`legaia_engine_core::camera::Camera::compass_azimuth_units`], which
    /// sums the scripted yaw, the drag-orbit and the host framing bias), and
    /// the native window has nothing that needs to speak over it. The page's
    /// VR first-person mode does: there the headset gaze *is* the heading, so
    /// it sets the azimuth outright for that tick.
    pub fn set_camera_azimuth(&mut self, units: u16) {
        self.camera_azimuth_override = Some(units % 4096);
        if let Some(h) = self.scene_host.host_mut() {
            h.world.locomotion.camera_azimuth = units % 4096;
        }
    }

    /// Advance the engine one frame. Returns `""` normally, or the label of the
    /// scene the engine just walked into (a door / warp) - the page rebuilds its
    /// render state whenever the return is non-empty.
    pub fn tick_frame(&mut self) -> Result<String, JsValue> {
        // A card Load / save import parks its save for the resume the page
        // lands in the same turn (`play_resume_save` consumes it). A park
        // nothing consumed must not re-apply the old save over the live party
        // at some later entry: the resume point has passed once the world
        // ticks, so the park does not outlive this frame. The native Load
        // lands and loads in one call (`BootSession::resume_save`) and parks
        // nothing.
        self.pending_card_resume = None;
        // The same for a score hand-off armed alongside it: with no entry to
        // run it, it runs now, over the scene still open.
        self.run_pending_bgm_handoff();
        // The audio director exists before the session ticks (off wasm it is
        // built on first use; on wasm once audio is up), so the session's
        // field SFX routing has it to route into.
        let _ = self.audio_director();
        let Some(session) = self.scene_host.session_mut() else {
            self.world.tick();
            return Ok(String::new());
        };
        // The naming prompt is modal: the field is frozen under it, and the
        // page's overlay steps it (`play_name_entry`). A catch-up tick that
        // lands while it is up runs nothing, so no stray pad edge reaches
        // the prompt through the session's own name-entry arm.
        if session.host.world.name_entry_active() {
            return Ok(String::new());
        }
        // A movie owns the frame. The native window freezes every world tick
        // under one (`run_ticks = 0` while its decoder handle is live), and so
        // does this host: under a movie the session does not tick. The FMV
        // service below still runs: it is what advances the picture and ends
        // the cutscene.
        let movie_held = self.fmv.armed_for().is_some();
        let event = if movie_held {
            SceneTickEvent::Stepped
        } else {
            // The native session's frame (`BootSession::tick`): the mode
            // seat's frame, the camera's half before the world tick (with the
            // page's own azimuth), the world tick, the tick's BGM events, the
            // camera's half after it, the SFX queue dropped on a door, the
            // field SFX routing, and the mode word adopted. The page owns the
            // pause menu, the field CD-XA lane and the per-tick queue drains,
            // which the session was told at install.
            session.camera_azimuth_override = self.camera_azimuth_override.take();
            session
                .tick()
                .map_err(|e| JsValue::from_str(&format!("tick: {e:#}")))?
        };
        // FMV beats: the movie path lives in [`crate::play_fmv`]; it hands
        // back the scene label when the post-movie hand-off entered one (the
        // session's hand-off, which resets the camera globals and drops the
        // SFX queue as a door does).
        let fmv_handoff_scene = self.service_cutscene_fmv();
        // ...and the rest of the frame tail is the world's, so it freezes
        // with the scene tick. A movie that ends this frame falls through, as
        // it does natively (the finish is drained before the window counts
        // its ticks).
        if movie_held && fmv_handoff_scene.is_empty() && self.fmv.armed_for().is_some() {
            return Ok(String::new());
        }
        // A scene swap - a door or the post-movie hand-off - restarts the
        // between-beat cutscene glide; the camera globals' reset is the
        // session's.
        if matches!(event, SceneTickEvent::SceneEntered { .. }) || !fmv_handoff_scene.is_empty() {
            self.cutscene_glide.reset();
        }
        // Advance the world's play clock off the page's wall clock, the same
        // delta-against-a-high-water-mark the native window runs.
        self.tick_play_clock();
        // Effect scene-graphs, ticked exactly where the native window ticks
        // them: drain the two production spawn requests (a player Seru-magic
        // cast, and a non-summon move whose power record carries a spawnable
        // effect list), then advance the summon / move-FX / field-FX
        // scene-graphs through the move VM. All three self-gate to a no-op
        // when nothing is live, and all three are host-agnostic simulation -
        // leaving them unticked in the browser meant a cast spawned an effect
        // that then never moved.
        self.tick_world_effects();
        // Fishing HUD one-shot banners ride the sim clock, not the page's
        // animation frame, so a heavy scene does not slow them down.
        self.tick_fishing_banners();
        // Field VRAM effects: CLUT-walk shimmer + ambient palette cyclers +
        // scripted CLUT fx, drained against the scene VRAM; the page re-reads
        // `field_vram_bytes` when `field_vram_take_dirty` reports a change.
        self.step_field_vram_fx();
        // Battle presentation: encounter-banner arming on the Field -> Battle
        // edge, battle-event fold, HUD row refresh, popup aging
        // ([`crate::play_battle`]). Cheap no-op outside battle.
        self.tick_battle_presentation();
        // In-world minigame presentation (casino / dance / arena sessions
        // the scene host installed): the draw-side state the page reads.
        self.tick_minigame_ui();
        // Sound-effect channel: feed the footstep cadence this tick's movement
        // magnitude, advance the delay scheduler, key whatever matured.
        //
        // **After** the two queue drains above, not before them. The native
        // window enqueues a battle tick's cues and calls `tick_sfx_frame` in
        // the same pass (`drain_and_log_battle_events`), so a cue whose
        // `timing_frames` is `0` - every strike impact, every minigame blip -
        // sounds on the tick that raised it. Advancing the scheduler first
        // made this host's copy of that cue wait a whole frame, which is not
        // a delay anyone can hear on its own but puts the impact one frame
        // off the animation it is supposed to land on.
        self.tick_sfx();
        // The field-to-battle intro emitter: armed while the encounter
        // session sits in `Transition`, dropped when it leaves; caches this
        // frame's screen-prim geometry for the page's pass. Cheap no-op
        // outside a transition.
        self.tick_battle_intro();
        // Party wipe: raise the game-over panel on the `World::game_over`
        // edge, the same probe the native window's redraw loop runs.
        self.poll_game_over();
        // Field party-status HUD countdown, ticked where the native window
        // ticks it (`FUN_801D0D38`); the draw pass reads the decision back.
        self.tick_field_party_hud();
        self.tick_passive_hud();
        // Developer menu (the visitor's explicit opt-in): ticked exactly
        // where the native window's redraw loop ticks its own, off the same
        // world pad words. A no-op while the opt-in is off.
        self.tick_dev_menu();
        // Drain every field-VM event the BGM router handed back (and, with
        // audio off, the BGM ones too) - the browser twin of the native
        // `drain_and_route_field_events`. `World::pending_field_events` is
        // only ever emptied by a consumer, and this host used to leave the
        // non-BGM events on it forever: a session's queue grew with every
        // camera beat, item grant and dialog open.
        self.drain_and_route_field_events_web();
        // The FMV hand-off loaded a scene without going through the field
        // VM's transition op, so it produces no `SceneEntered` event - the
        // page still has to rebuild, or it draws the old scene's meshes over
        // the new world. (Its SFX queue was already dropped above, before
        // this tick's ring ops were replayed.)
        //
        // A door (`SceneEntered`) rebuilds the same way. `town01` keeps its
        // establishing-sweep timeline: the page draws the name-entry overlay
        // its pinned op-0x49 opens (`crate::play_name_entry`), so the
        // suspended script has a surface to resume from. No bank is staged
        // and the dedupe latch is kept, so a track that carries across the
        // transition keeps its playhead and its samples (the native
        // `after_scene_swap` does the same).
        let entered = if !fmv_handoff_scene.is_empty() {
            fmv_handoff_scene
        } else if let SceneTickEvent::SceneEntered { name } = event {
            name
        } else {
            String::new()
        };
        if !entered.is_empty() {
            self.rebuild_render_state()?;
        }
        // The rest of the tick's tail runs on an entry tick too, as it does
        // in the native window's loop, which rebuilds and carries on. This
        // host used to return straight after the rebuild, so the entry
        // tick's rig change, merchant arm and NPC clip step were skipped.
        //
        // A `CC F8 50` re-staged the player's model this tick: rebuild the
        // rig from the new mesh, the native window's twin
        // (`rebind_live_npc_models` there drains the same signal).
        if self
            .scene_host
            .host_mut()
            .is_some_and(|h| h.world.take_player_rig_change())
        {
            self.build_player_rig();
        }
        // A field-VM op-0x49 sub-0 merchant armed a shop this tick: hand it to
        // the menu runtime so the page can open the store. The field VM stays
        // suspended (op-0x49 Armed) until `play_shop_input` sees the session
        // end and calls `finish_field_shop`.
        self.poll_field_shop();
        self.drive_npc_clips();
        Ok(entered)
    }

    /// One-line engine state for the HUD:
    /// ```text
    /// { "scene": "town01", "frame": 421, "mode": "Field",
    ///   "actors": 12, "npcs": 9,
    ///   "player": { "x": 2688, "y": -256, "z": 2432, "facing": 2048,
    ///               "walking": true },
    ///   "dialog": { "text": "...", "options": ["Yes", "No"], "cursor": 0 },
    ///   "bgm": { "requested": 2019, "playing": 2019 } }
    /// ```
    /// `dialog` is `null` when no box is up. `bgm.requested` is the id the
    /// simulation's last op-`0x35` selected and `bgm.playing` is the id the
    /// audio output holds - equal on a healthy frame, and the one externally
    /// visible signal that a music change resolved without reaching the
    /// sequencer.
    pub fn state_json(&self) -> String {
        let Some(h) = self.scene_host.host() else {
            return serde_json::json!({
                "scene": serde_json::Value::Null,
                "frame": self.world.frame,
                "mode": format!("{:?}", self.world.mode),
                "actors": 0,
                "npcs": 0,
                "player": serde_json::Value::Null,
                "dialog": serde_json::Value::Null,
                "bgm": self.bgm_value(),
            })
            .to_string();
        };
        let w = &h.world;
        let player = w
            .player_actor_slot
            .and_then(|s| w.actors.get(s as usize))
            .map(|a| {
                serde_json::json!({
                    "x": a.move_state.world_x,
                    "y": a.move_state.world_y,
                    "z": a.move_state.world_z,
                    "facing": a.move_state.render_26,
                    "walking": w.locomotion.player_anim.as_ref().is_some_and(|f| f.walking),
                })
            })
            .unwrap_or(serde_json::Value::Null);
        serde_json::json!({
            "scene": h.scene.as_ref().map(|s| s.name.clone()),
            "frame": w.frame,
            "mode": format!("{:?}", w.mode),
            "actors": w.actors.iter().filter(|a| a.active).count(),
            "npcs": self.actors.npcs.as_ref().map(|n| n.pack.entries.len()).unwrap_or(0),
            "player": player,
            "dialog": self.dialog_value(),
            "bgm": self.bgm_value(),
        })
        .to_string()
    }

    /// `{ "requested": id|null, "playing": id|null }` - what the simulation's
    /// last op-`0x35` selected versus what the audio output is actually
    /// sounding.
    ///
    /// The two are the same value on a healthy frame, and their *drift* is the
    /// only external symptom of a BGM change that resolved but never reached
    /// the sequencer. `playing` is `null` whenever audio is not up, which is
    /// the ordinary state before the first user gesture.
    fn bgm_value(&self) -> serde_json::Value {
        let requested = self
            .scene_host
            .host()
            .and_then(|h| h.world.audio.current_bgm)
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null);
        let playing = self
            .scene_host
            .director()
            .and_then(|d| d.last_started)
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null);
        serde_json::json!({ "requested": requested, "playing": playing })
    }

    /// Field pause-menu model: the party (battle order) + inventory + gold the
    /// page's menu overlay renders when the player presses Start. Shape:
    /// ```text
    /// { "gold": 240,
    ///   "party": [{ "name": "Vahn", "level": 1, "hp": 60, "hp_max": 60,
    ///               "mp": 8, "mp_max": 8 }, ...],
    ///   "items": [{ "id": 32, "name": "Healing Leaf", "count": 3 }, ...] }
    /// ```
    /// `null` before a disc scene is entered. Item labels come from the SCUS
    /// item-name table ([`Self::load_disc`]); a PROT.DAT-only load falls back to
    /// the raw id. The retail pause menu is a native-only draw path (glyph atlas
    /// + window-descriptor table); this feeds the browser's HTML overlay
    /// equivalent so Start still surfaces the party / items on the play page.
    pub fn field_menu_model_json(&self) -> String {
        let Some(h) = self.scene_host.host() else {
            return "null".to_string();
        };
        let w = &h.world;
        let order: Vec<usize> = if w.party.active_party.is_empty() {
            (0..w.party.roster.members.len()).collect()
        } else {
            w.party.active_party.iter().map(|&s| s as usize).collect()
        };
        let party: Vec<serde_json::Value> = order
            .iter()
            .filter_map(|&slot| {
                let m = w.party.roster.members.get(slot)?;
                // A never-populated roster slot decodes to an all-zero record
                // (empty name) - skip it so the menu shows only real members.
                let name = m.name();
                if name.trim().is_empty() {
                    return None;
                }
                let hms = m.hp_mp_sp();
                Some(serde_json::json!({
                    "name": name,
                    "level": m.level(),
                    "hp": hms.hp_cur, "hp_max": hms.hp_max,
                    "mp": hms.mp_cur, "mp_max": hms.mp_max,
                }))
            })
            .collect();
        let items: Vec<serde_json::Value> = MenuRuntime::inventory_items(w)
            .into_iter()
            .map(|(id, count)| {
                let name = self
                    .item_names
                    .as_ref()
                    .and_then(|t| t.name(id))
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("Item {id:#04x}"));
                serde_json::json!({ "id": id, "name": name, "count": count })
            })
            .collect();
        serde_json::json!({ "gold": w.party.money, "party": party, "items": items }).to_string()
    }

    /// Attempt to start the WebAudio backend. Must be called from a user-gesture
    /// handler (browser autoplay policy). `true` on success.
    ///
    /// Once up, the scene's BGM plays automatically: every [`Self::tick_frame`]
    /// routes the field VM's op-`0x35` music events through the same port-side
    /// VAB + SEQ + SPU path the audio audition page uses. This call also
    /// starts the track the scene's script last started - that start was
    /// routed before the output existed and so was dropped - and parks the
    /// default level ([`BGM_DEFAULT_GAIN`] slider units) on the output node
    /// so the level matches the page's slider. Browsers often open the
    /// `AudioContext` suspended even inside a gesture - call
    /// [`Self::audio_resume`] right after this to make it audible.
    pub fn audio_init(&mut self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            // Idempotent: a second output would replace the first, and the
            // page's whole audio state (director, staged banks, the playing
            // track) is built over the one it has.
            if self.audio_out.is_some() {
                return true;
            }
            match WebAudioOut::new() {
                Ok(out) => {
                    out.set_gain(BGM_DEFAULT_GAIN);
                    #[allow(clippy::arc_with_non_send_sync)]
                    let out = std::sync::Arc::new(out);
                    self.audio_out = Some(out);
                    // The director is built over the new output and staged
                    // at once.
                    self.scene_host.set_director(None);
                    let _ = self.audio_director();
                    // Every start routed before this was dropped with no
                    // director to hear it: bring the scene's track up now.
                    self.start_current_bgm_on_late_audio();
                    true
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("audio_init: {e}").into());
                    false
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        false
    }

    /// Resume the BGM `AudioContext`. Browsers construct it in `suspended`
    /// state even when the constructor runs inside a user gesture; the play
    /// page calls this from its gesture handler right after [`Self::audio_init`]
    /// to make the audio actually sound. Resolved no-op when audio isn't up.
    #[cfg(target_arch = "wasm32")]
    pub fn audio_resume(&self) -> js_sys::Promise {
        match self.audio_out.as_ref() {
            Some(out) => out.resume(),
            None => js_sys::Promise::resolve(&JsValue::UNDEFINED),
        }
    }

    /// Swap the score to the title theme - the browser twin of the native
    /// post-wipe `BootUiState::Title` hand-off: stop the running track,
    /// then start [`legaia_engine_core::music_labels::TITLE_THEME_BGM_ID`]
    /// as an owned-VAB global track. The page calls this when the
    /// party-wipe hold resolves to the title card, so the battle BGM never
    /// outlives the battle. `false` when audio is down or the disc entry
    /// doesn't resolve - the stop still ran, leaving silence rather than
    /// the stale track.
    pub fn play_title_bgm(&mut self) -> bool {
        use legaia_engine_core::scene::BgmDirector;
        let id = legaia_engine_core::music_labels::TITLE_THEME_BGM_ID;
        let Some(d) = self.audio_director() else {
            return false;
        };
        d.stop();
        let Some(Ok(Some(entry))) = self.scene_host.host().map(|h| h.music_bank_entry_bytes(id))
        else {
            return false;
        };
        let Some(d) = self.scene_host.director_mut() else {
            return false;
        };
        d.start_owned_vab(id, &entry);
        d.last_started == Some(id)
    }

    /// Set the BGM output gain in page-slider units: `1.0` is the page
    /// default ([`BGM_DEFAULT_GAIN`]); the slider spans 0x (mute) to 10x.
    /// No-op when audio isn't up.
    ///
    /// The site's browser master trim
    /// ([`legaia_engine_audio::webaudio::WEB_MASTER_TRIM`] - the same `0.25`
    /// `site/js/layout.js` publishes as `window.LEGAIA_MASTER_TRIM`) is
    /// applied by `WebAudioOut::set_gain` itself, so the slider value passes
    /// through untouched. This page used to multiply by its own copy of that
    /// factor first, applying the site trim TWICE and leaving the play page a
    /// factor of four - about 12 dB - under the minigames page, whose
    /// `site/js/minigame-bgm.js` output stage applies it once.
    #[cfg(target_arch = "wasm32")]
    pub fn audio_set_gain(&self, gain: f32) {
        if let Some(out) = self.audio_out.as_ref() {
            out.set_gain(gain);
        }
    }

    /// Whether the WebAudio backend is live (`audio_init` succeeded). The play
    /// page reads this to decide whether it still needs the user's audio-enable
    /// gesture.
    pub fn audio_ready(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            self.audio_out.is_some()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            false
        }
    }

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

    /// The live retail mode word (`_DAT_8007B83C`), its table name, and the
    /// front-end entry word beside it, as
    /// `{"word": <u32>, "name": "<MODE>", "entry_word": <u32>}`.
    ///
    /// The page's own read of the seat this host holds - which is what makes
    /// a browser mode trace possible at all. Before the seat existed the
    /// front end ran with no mode word, so the two hosts could not be
    /// compared on the one register retail's whole dispatch keys off.
    ///
    /// `word` is the **mode table index** the native oracle samples
    /// (`ModeSeat::game_mode`). It used to be the seat's `entry_word`
    /// (`_DAT_8007BB00`, the front-end flag `init.pak`'s hand-off reads),
    /// which is a different global: the JSON paired that flag's value with
    /// the mode's *name*, so a reader comparing the two hosts' words was
    /// comparing two different registers and could not see a divergence.
    ///
    /// `edges` is the count of mode changes the seat has taken, and it is
    /// the field that makes the two hosts comparable at the **INIT** modes.
    /// An INIT mode lasts one frame *inside* `ModeSeat::enter` - the call
    /// resolves the staging plan and hands the word to the mode's RUN
    /// sibling before returning - so no sampler outside the call ever
    /// observes the word sitting on one. What it leaves behind is the extra
    /// edge, so a host that reaches `MAIN MODE` by entering `MAIN INIT` and
    /// one that reaches it by adopting the world's scene mode walk the same
    /// words and differ here.
    pub fn mode_state_json(&self) -> String {
        serde_json::json!({
            "word": self.mode_word(),
            "name": self.scene_host.seat().mode_name(),
            "entry_word": self.scene_host.seat().entry_word(),
            "edges": self.scene_host.seat().edges(),
        })
        .to_string()
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
    /// call of [`legaia_engine_core::mode::ModeSeat::enter`], the entry point
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
        mode: legaia_engine_core::mode::GameMode,
    ) -> Option<legaia_engine_core::mode::ModeInitPlan> {
        let (world, seat) = self.scene_host.world_seat_mut(&mut self.world);
        seat.enter(mode, world)
    }

    /// Decode the live dialogue box (the field VM's inline-script runner) into
    /// the JSON the HUD prints. Glyph bytes are ASCII-compatible from `0x20`.
    fn dialog_value(&self) -> serde_json::Value {
        let Some(h) = self.scene_host.host() else {
            return serde_json::Value::Null;
        };
        let Some(id) = h.world.dialog.inline.as_ref() else {
            return serde_json::Value::Null;
        };
        let ascii = |bytes: &[u8]| -> String {
            bytes
                .iter()
                .map(|&b| {
                    if (0x20..=0x7E).contains(&b) {
                        b as char
                    } else {
                        ' '
                    }
                })
                .collect::<String>()
                .trim_end()
                .to_string()
        };
        let text = ascii(&id.page_bytes());
        if text.trim().is_empty() {
            return serde_json::Value::Null;
        }
        let options: Vec<String> = id
            .menu_active()
            .then(|| id.picker())
            .flatten()
            .map(|p| p.options.iter().map(|o| ascii(&o.label)).collect())
            .unwrap_or_default();
        serde_json::json!({
            "text": text,
            "options": options,
            "cursor": id.picker_cursor(),
        })
    }

    /// Rebuild the page-facing render state for the scene the host now holds:
    /// the assembled map, the lead's posed mesh, the NPC catalog. Runs on scene
    /// entry and on every door the engine walks through.
    fn rebuild_render_state(&mut self) -> Result<(), JsValue> {
        // A scene swap drops the previous scene's script-spawned actors
        // with it (the MAN loader's retire sweep); the page re-uploads
        // whatever the new scene spawns.
        self.pending_dynamic_mesh_slots.clear();
        self.dynamic_mesh_slots.clear();
        self.scene_aabb = None;
        self.dynamic_mesh_cur = None;
        self.field = None;
        self.player = None;
        self.actors.clear();
        self.scene_anm = None;
        self.locomotion_anm = None;
        self.field_vram_anim = None;
        let Some(host) = self.scene_host.host() else {
            return Ok(());
        };
        let (Some(scene), Some(res)) = (host.scene.as_ref(), host.resources.as_ref()) else {
            return Err(JsValue::from_str(
                "enter: the scene loaded but built no resources",
            ));
        };
        let name = scene.name.clone();
        let is_world_map = legaia_engine_core::scene::is_world_map_scene(&name);
        let follow = host.world.object_floor_follow_records();
        self.field = Some(crate::play::build_field_render(
            &host.index,
            scene,
            res,
            is_world_map,
            &host.world.hidden_object_records(),
            &host.world.object_render_scales(),
            &|r, x, z| host.world.object_floor_follow_y(&follow, r, x, z),
        ));
        // Pose sources, through the one resolver the native window's
        // `find_scene_anm_bundle` calls too (`npc_catalog::scene_anm_bundle`).
        // The scene bundle poses the MAN NPCs and the bound placed props; the
        // locomotion bundle poses the global-pool specials.
        self.scene_anm = legaia_engine_core::npc_catalog::scene_anm_bundle(scene);
        self.locomotion_anm = host
            .index
            .entry_bytes(legaia_asset::character_pack::PROT_ENTRY_INDEX)
            .ok()
            .and_then(|b| legaia_asset::character_pack::field_locomotion_anm(&b).ok());
        // The actor layer resolves against the same TMD pool + VRAM, plus the
        // world's global pool for the `model >= 0xF0` specials - everything
        // the native play-window draws.
        self.build_npc_clips();
        self.build_player_rig();
        // CLUT-walk shimmer (water / waterfall scenes): parse the bundle's
        // type-6 walker table and park its source strips into the host's
        // VRAM - this must land before the page's post-entry
        // `field_vram_bytes` upload, which is why it lives in the rebuild.
        // The ambient move-VM tree needs no sibling here: the scene host
        // spawned it into the live world at scene entry, and
        // `step_field_vram_fx` drains it against the same VRAM.
        self.field_vram_anim = None;
        if let Some(host) = self.scene_host.host_mut()
            && let (Some(scene), Some(res)) = (host.scene.as_ref(), host.resources.as_mut())
        {
            let frame_step = host.world.clock.frame_step.max(1);
            // One resolve + park for every host
            // (`legaia_engine_core::clut_walk_anim::ClutWalkAnim::install`):
            // the type-6 table on a field scene, the kingdom's slot-5 table
            // on an overworld, the Drake complement rows, and the legacy
            // ocean-head fallback.
            if let Some(install) = legaia_engine_core::clut_walk_anim::ClutWalkAnim::install(
                scene,
                &host.index,
                &mut res.vram,
            ) {
                if install.ocean_fallback {
                    crate::console_log(
                        "play: no slot-5 CLUT-walk table in the kingdom bundle; \
                         falling back to the legacy ocean-head cycle",
                    );
                }
                for (x, y) in &install.missing_cells {
                    crate::console_log(&format!(
                        "play: CLUT-walk source cell ({x}, {y}) has no VRAM data \
                         (strip residency gap)"
                    ));
                }
                self.field_vram_anim = Some(crate::field_scene::FieldSceneAnim::clut_only(
                    install.anim,
                    frame_step,
                ));
            }
        }
        Ok(())
    }

    /// Drain this sim tick's VRAM-mutating field effects against the host's
    /// scene VRAM - the browser twin of the native play-window's
    /// `apply_world_clut_fx` + water-CLUT animator: the type-6 CLUT-walk
    /// shimmer, the scripted `MoveImage` stamps, the ambient move-VM tree
    /// (jou's pulsating-flesh palette cyclers + lightning), and the CLUT-cell
    /// one-shots. Battle-guarded like the native path: while a battle is up
    /// the page's GPU texture holds the battle VRAM and a field re-upload
    /// would clobber it.
    fn step_field_vram_fx(&mut self) {
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        if host.world.mode == SceneMode::Battle {
            return;
        }
        let Some(res) = host.resources.as_mut() else {
            return;
        };
        let mut dirty = false;
        if let Some(anim) = self.field_vram_anim.as_mut() {
            // Live divisor, not the value this scene was rebuilt at - the
            // native animator reads `clock.frame_step` off the world on
            // every frame.
            anim.set_frame_step(host.world.clock.frame_step);
            dirty |= anim.tick(1, &mut res.vram);
        }
        // The scripted VRAM effects (op-`0x43` stamps + rect copies, the
        // ambient move-VM tree, CLUT-cell one-shots and blend fades) through
        // the shared frame-tail kernel the native window's
        // `apply_world_clut_fx` calls; the page presents one framebuffer
        // page, so the back-buffer bias is off.
        dirty |= host.world.step_field_vram_effects(&mut res.vram, false);
        self.field_vram_dirty |= dirty;
    }

    /// Rebuild the actor layer over the entered scene
    /// ([`crate::field_actors::FieldActors::rebuild`]).
    fn build_npc_clips(&mut self) {
        let Some(host) = self.scene_host.host_mut() else {
            self.actors.clear();
            return;
        };
        let banks = crate::field_actors::ActorBanks {
            scene_anm: self.scene_anm.as_ref(),
            locomotion_anm: self.locomotion_anm.as_ref(),
        };
        self.actors.rebuild(host, banks);
    }

    /// One sim tick of the actor layer's clip playback
    /// ([`crate::field_actors::FieldActors::drive`]).
    fn drive_npc_clips(&mut self) {
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        let banks = crate::field_actors::ActorBanks {
            scene_anm: self.scene_anm.as_ref(),
            locomotion_anm: self.locomotion_anm.as_ref(),
        };
        self.actors.drive(host, banks);
    }

    /// Put the player somewhere they can actually stand.
    ///
    /// Scene entry seats them at the retail **cold-boot spawn** - the fixed
    /// camera-window centre `FIELD_COLD_SPAWN_XZ` that `FUN_801D6704` uses on a
    /// non-warp entry. That is the right answer for `town01`, the one scene
    /// retail cold-boots into; every other scene is normally *entered through a
    /// door*, which overrides X/Z with the transition's entry tile. Dropping into
    /// one from the scene picker has no door to supply that, so the cold spawn can
    /// land outside the map entirely (a cave whose floor is nowhere near it).
    ///
    /// So: keep the cold spawn when it is walkable and inside the scene's
    /// populated area, and otherwise fall back to the walkable terrain tile
    /// closest to the middle of that area. Then sample the floor under the final
    /// position - the locomotion step is what normally does that, so without it
    /// the first frame would draw the character sunk into an elevated tier.
    fn seat_player(&mut self) {
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        let Some(slot) = host.world.player_actor_slot.map(|s| s as usize) else {
            return;
        };
        // Candidate seats, each `(x, z, drawn_y)`.
        //
        // The **walk-ground heightfield** is the scene's actual floor, so it is
        // the first choice: a cave's terrain-*tile* meshes are its rock walls,
        // not its ground, and seating on one of those buries the player inside a
        // boulder. Scenes with no resolvable floor grid fall back to the tile /
        // placement draws.
        //
        // Each candidate carries the height it is *drawn* at, which is not always
        // what the floor sampler reports (a scene whose floor lives in its
        // meshes rather than in the floor-height LUT samples as 0), so the seat
        // prefers the sampler and falls back to the drawn height.
        let tiles: Vec<(i32, i32, i32)> = self
            .field
            .as_ref()
            .map(|f| match f.ground.as_ref() {
                // Every 16th vertex is plenty: the grid is 128-unit tiles and a
                // 4000-quad heightfield would otherwise cost a needless scan.
                Some(hf) => hf
                    .positions
                    .iter()
                    .step_by(16)
                    .map(|p| (p[0] as i32, p[2] as i32, p[1] as i32))
                    .collect(),
                None => f
                    .terrain
                    .iter()
                    .chain(f.placements.iter())
                    .map(|d| (d.world_x, d.world_z, d.world_y))
                    .collect(),
            })
            .unwrap_or_default();
        let (sx, sz) = match host.world.actors.get(slot) {
            Some(a) => (a.move_state.world_x as i32, a.move_state.world_z as i32),
            None => return,
        };
        let mut x = sx;
        let mut z = sz;
        let mut y = host.world.sample_field_floor_height(sx, sz);
        if !tiles.is_empty() {
            let dist2 = |a: (i32, i32), b: (i32, i32)| {
                let (dx, dz) = ((a.0 - b.0) as i64, (a.1 - b.1) as i64);
                dx * dx + dz * dz
            };
            let n = tiles.len() as i64;
            let cx = (tiles.iter().map(|t| t.0 as i64).sum::<i64>() / n) as i32;
            let cz = (tiles.iter().map(|t| t.1 as i64).sum::<i64>() / n) as i32;
            // "Inside the map" = the cold spawn has floor under it. Measuring
            // that against the *nearest* ground tile, not the map centre, keeps
            // a perfectly good spawn near the edge of a big scene - and moving a
            // player who did not need moving is not free: the relocation target
            // can be a walk-on trigger tile (a town exit), which the engine would
            // fire the moment the first tick crosses onto it.
            let nearest = tiles
                .iter()
                .map(|&t| dist2((t.0, t.1), (sx, sz)))
                .min()
                .unwrap_or(i64::MAX);
            let cold_ok =
                !host.world.field_tile_is_wall(sx as i16, sz as i16) && nearest < 1200 * 1200;
            if !cold_ok {
                let mut best: Option<((i32, i32, i32), i64)> = None;
                for &t in &tiles {
                    if host.world.field_tile_is_wall(t.0 as i16, t.1 as i16) {
                        continue;
                    }
                    // Never seat onto a walk-on trigger tile: the first tick
                    // would fire it, and the scene the player just picked would
                    // warp out from under them (a town exit does exactly this).
                    if host.tile_has_walk_on_trigger(t.0 as i16, t.1 as i16) {
                        continue;
                    }
                    let d = dist2((t.0, t.1), (cx, cz));
                    if best.is_none_or(|(_, bd)| d < bd) {
                        best = Some((t, d));
                    }
                }
                if let Some(((bx, bz, by), _)) = best {
                    x = bx;
                    z = bz;
                    // Prefer the sampler when it has an answer (a town's floor
                    // tiers do come from the LUT); fall back to the height the
                    // tile is actually drawn at.
                    let sampled = host.world.sample_field_floor_height(bx, bz);
                    y = if sampled != 0 { sampled } else { by };
                }
            }
        }
        if let Some(p) = host.world.actors.get_mut(slot) {
            p.move_state.world_x = x as i16;
            p.move_state.world_z = z as i16;
            p.move_state.world_y = y as i16;
        }
    }

    /// Resolve the lead's field-form mesh out of the global TMD pool (PROT 0874
    /// §0, seeded by `enter_field_scene`) and install the idle / walk clip pair
    /// the world ticks into the player actor's `pose_frame`.
    ///
    /// Mirrors the native play-window's player bind: the disc TMD's object table
    /// is truncated to the clip's bone count (retail caps the live object count
    /// at 10 - groups 10/11 are equipment-swap templates and are never drawn), so
    /// bone `i` poses object `i`.
    /// REF: FUN_8001E890
    fn build_player_rig(&mut self) {
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        // The overworld draws the lead's field form too (retail walks the
        // same mesh across the continent; the native window binds it on
        // both), so a kingdom scene keeps the rig.
        if !matches!(host.world.mode, SceneMode::Field | SceneMode::WorldMap) {
            return;
        }
        let roster_lead = host.world.party.active_party.first().copied().unwrap_or(0) as usize;
        // The lead's field form, or the model a `CC F8 50` re-staged the
        // player onto - one resolution for both hosts
        // (`SceneHost::player_rig_mesh`).
        let Some(g) = host.player_rig_mesh() else {
            crate::console_log(&format!(
                "play: no rig mesh for roster slot {roster_lead} (model {:?})",
                host.world.locomotion.player_live_model
            ));
            return;
        };
        // The party locomotion bundle (PROT 0874 §1) banks the Vahn / Noa / Gala
        // trio only. The bone cap follows the MESH's slot (a scene-bank model
        // has none); the clip player stays the roster lead's, whose settle
        // pick binds scene records itself while the party-bank bit is down.
        let locomotion_bank = host
            .index
            .entry_bytes(legaia_asset::character_pack::PROT_ENTRY_INDEX)
            .ok()
            .and_then(|b| legaia_asset::character_pack::field_locomotion_anm(&b).ok());
        let lead = g.party_slot.unwrap_or(usize::MAX);
        let rec = |slot| legaia_asset::character_pack::locomotion_record_index(lead, slot);
        let bones = locomotion_bank
            .as_ref()
            .filter(|_| lead <= 2)
            .and_then(|bundle| {
                let idx = rec(legaia_asset::character_pack::LOCOMOTION_IDLE_SLOT);
                bundle.record(idx).ok().map(|r| r.bone_count as usize)
            });
        let mut tmd = g.tmd.clone();
        if let Some(b) = bones {
            tmd.objects.truncate(b);
        }
        let (base, object_ids, shading) =
            legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd, &g.raw);
        if base.indices.is_empty() {
            crate::console_log("play: the lead's field mesh has no renderable prims");
            return;
        }
        let flat = crate::packet_color::hybrid(&base, &shading);
        let posed = Vec::with_capacity(base.positions.len() * 3);
        self.player = Some(PlayerRig {
            base,
            object_ids,
            flat,
            posed,
        });
        // Live locomotion playback: the leader's whole bank, from which the
        // world's settle tail picks idle / walk / run / hop each field tick
        // and folds the pose into the player actor.
        let anim = locomotion_bank
            .as_ref()
            .filter(|_| roster_lead <= 2)
            .and_then(|bundle| {
                legaia_engine_core::field_anim::FieldPlayerAnim::from_locomotion_bank(
                    bundle,
                    roster_lead,
                )
            });
        host.world.set_field_player_anim(anim);
    }

    /// Drain this tick's field-VM BGM events into the page's director.
    /// Runs only while a director exists (on wasm: audio is up); until then
    /// the events are dropped by `drain_and_route_field_events_web`, as an
    /// unheard retail op-`0x35` would be, and the late-audio start
    /// (`start_current_bgm_on_late_audio`) brings the scene's track up.
    fn route_bgm(&mut self) {
        if self.audio_director().is_none() {
            return;
        }
        let Some((host, d)) = self.scene_host.host_director_mut() else {
            return;
        };
        if let Err(e) = host.route_bgm_events(d) {
            crate::console_log(&format!("play BGM: route failed: {e:#}"));
        }
    }
}

impl LegaiaRuntime {
    /// Consume this tick's remaining field-VM events - the browser twin of
    /// the native window's `drain_and_route_field_events`
    /// (`window/boot_cutscene.rs`). BGM events are normally consumed by
    /// `route_bgm` first; while audio is down (no `WebAudioOut` yet) they
    /// come through here and are dropped, exactly as an unheard retail
    /// op-`0x35` would be. `ActorSpawned` is noted for the page's dynamic
    /// mesh upload; everything else is presentation this host reads off the
    /// world's own state instead (the cutscene camera params, the dialog box).
    pub(crate) fn drain_and_route_field_events_web(&mut self) {
        use legaia_engine_core::field_events::FieldEvent;
        // Route any BGM event the frame's own steps raised since the routing
        // pass after the scene tick (a battle-presentation or minigame swap,
        // a dance song ending) before the drain below drops whatever is left.
        // The native `drain_and_route_field_events` opens with the same pass.
        self.route_bgm();
        let Some(host) = self.scene_host.host_mut() else {
            self.world.drain_field_events();
            return;
        };
        for ev in host.world.drain_field_events() {
            if let FieldEvent::ActorSpawned { slot, .. } = ev {
                let has_tmd = host
                    .world
                    .actors
                    .get(slot as usize)
                    .is_some_and(|a| a.tmd_ref.is_some());
                if has_tmd && !self.pending_dynamic_mesh_slots.contains(&slot) {
                    self.pending_dynamic_mesh_slots.push(slot);
                }
            }
        }
    }
}

impl Default for LegaiaRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// `localStorage` key the play page's options live under - the browser twin
/// of the native window's `OPTIONS_CONFIG_FILE`.
///
/// The two hosts persist through the *same* serde impl on
/// [`legaia_engine_core::options::OptionsState`]; only the byte store
/// differs, because a browser has no filesystem. Before this the browser had
/// no store at all: the Sound row was decorative and every setting was lost
/// on reload, which reads as "the options screen does nothing" rather than as
/// a missing capability.
pub const OPTIONS_STORAGE_KEY: &str = "legaia.options";

/// Read the persisted options, falling back to [`Default`] when nothing is
/// stored, the store is unavailable (private mode, non-browser target) or the
/// stored JSON no longer parses. Mirrors `OptionsState::load_or_default`.
fn load_persisted_options() -> legaia_engine_core::options::OptionsState {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(raw) = options_storage().and_then(|s| s.get_item(OPTIONS_STORAGE_KEY).ok())
            && let Some(raw) = raw
            && let Ok(state) = serde_json::from_str(&raw)
        {
            return state;
        }
    }
    legaia_engine_core::options::OptionsState::default()
}

/// The page's `localStorage`, when this target has one and the browser lets
/// us reach it (it throws in private mode on some engines).
#[cfg(target_arch = "wasm32")]
fn options_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// Milliseconds since a fixed origin, from whatever clock this target has.
/// Only ever consumed as a delta ([`LegaiaRuntime::tick_play_clock`]), so the
/// origin itself is irrelevant.
fn wall_clock_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0)
    }
}

impl LegaiaRuntime {
    /// Tick the world's effect scene-graphs - the browser twin of the native
    /// window's per-frame effects block, now leg for leg:
    ///
    /// * `take_pending_summon_spawn` -> [`Self::spawn_summon_creature_web`]:
    ///   a player Seru-magic cast's namesake `battle_data` creature, seated
    ///   and drawn through the enemy animation pipeline
    ///   ([`crate::play_battle_fx`]). This used to be *deliberately* left
    ///   undrained, on the reasoning that the browser had no battle 3D layer
    ///   to draw it into - the layer exists now, so the request is honoured.
    /// * `take_pending_move_fx_spawn` ->
    ///   [`World::spawn_move_fx`](legaia_engine_core::world::World::spawn_move_fx),
    ///   whose parts ride the FX part-draw seam.
    /// * the spawned move's sound cue through the retail dispatch decode
    ///   (`classify_cue` = `FUN_8004FCC8`) into the page's SFX scheduler - a
    ///   `Ring` cue's `ring_value` is the `SfxBank` descriptor id. Voice cues
    ///   (`id >= 0x100`) are streamed XA triggers neither host has a lane for.
    /// * `tick_summon` / `tick_move_fx` / `tick_field_fx` at the retail
    ///   `0x0400` anim-speed step. Each self-gates to a no-op when nothing is
    ///   live.
    fn tick_world_effects(&mut self) {
        // The native window's order, leg for leg: seat the summon creature,
        // spawn the move-FX, then tick the scene graphs - so a summon cast
        // ticks its creature's first frame on the same tick on both hosts.
        // The creature seat needs `&mut self`, so it runs between two host
        // borrows.
        let summon = match self.scene_host.host_mut() {
            Some(host) => host.world.take_pending_summon_spawn(),
            None => return,
        };
        if let Some((spell_id, _origin)) = summon {
            self.spawn_summon_creature_web(spell_id);
        }
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        let world = &mut host.world;
        let mut cue = None;
        if let Some((move_id, origin)) = world.take_pending_move_fx_spawn()
            && world.spawn_move_fx(move_id, origin)
        {
            cue = world.take_pending_move_fx_cue();
        }
        // The shared frame-tail kernel the native window's loop calls.
        world.tick_effect_scene_graphs();
        // The full ring value goes through: `enqueue_sfx` takes
        // `impl Into<u16>`, and the `u8::try_from` this used to narrow
        // through dropped cue id `0`, whose ring value is `0xFFFF`.
        if let Some(cue) = cue
            && let legaia_engine_audio::CueDispatch::Ring { ring_value, .. } =
                legaia_engine_audio::classify_cue(cue as u32)
        {
            self.enqueue_sfx(ring_value, 0);
        }
    }

    /// Advance the world's play clock off the page's wall clock - the browser
    /// twin of the native window's `tick_play_clock`, called once per
    /// [`LegaiaRuntime::tick_frame`].
    ///
    /// Whole seconds only, and by delta rather than absolutely, so a loaded
    /// save keeps its accumulated total. The page used to substitute
    /// `world.frame / 60` at the one place the clock was *drawn*, which left
    /// [`legaia_engine_core::world::FrameClock::play_time_seconds`] frozen at
    /// whatever a load put there - so the H:MM:SS box reset on every page
    /// load, ignored a loaded save's hours, and, worse, a save written from
    /// the browser recorded the *loaded* play time rather than the played one.
    ///
    /// The origin and high-water mark are the world's
    /// ([`legaia_engine_core::world::World::tick_play_clock`], the kernel the
    /// native window calls too), so New Game restarts both. This page used to
    /// keep them itself and reset only the origin on New Game, which froze
    /// play time after a second New Game until the wall clock caught up with
    /// the old mark.
    pub(crate) fn tick_play_clock(&mut self) {
        let now_secs = wall_clock_ms() / 1000.0;
        if let Some(host) = self.scene_host.host_mut() {
            host.world.tick_play_clock(now_secs);
        }
    }

    /// Apply the live side effects of [`Self::options_state`] and persist it -
    /// the browser twin of the native window's `persist_and_apply_options`,
    /// called from the same place (an Options sub-session closing).
    pub(crate) fn persist_and_apply_options(&mut self) {
        self.apply_options_side_effects();
        #[cfg(target_arch = "wasm32")]
        if let Some(store) = options_storage()
            && let Ok(json) = serde_json::to_string(&self.options_state)
            && store.set_item(OPTIONS_STORAGE_KEY, &json).is_err()
        {
            crate::console_log("options: localStorage write failed");
        }
    }

    /// Push the current options into their live consumers without touching
    /// the store (also called once when audio comes up, so a persisted
    /// Monaural / muted state applies to a fresh `AudioContext`).
    ///
    /// The same two audio knobs the native window applies - the retail
    /// options screen's Stereo / Monaural row and the engine-only master
    /// mute - plus the one simulation knob (`precise_movement`).
    /// `bgm_volume` / `sfx_volume` are read by neither host today: that is a
    /// host-identical gap, not drift.
    pub(crate) fn apply_options_side_effects(&mut self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(audio) = self.audio_out.as_ref() {
            audio.set_mono(matches!(
                self.options_state.audio,
                legaia_engine_core::options::AudioMode::Mono
            ));
            audio.set_muted(self.options_state.muted);
        }
        if let Some(host) = self.scene_host.host_mut() {
            // The simulation knobs (precise movement, Field Move default,
            // reduce flashing, battle Select Attack) through the one push the
            // native window re-asserts each tick.
            self.options_state.apply_to_world(&mut host.world);
            if let Some(on) = self.precise_movement_override {
                host.world.locomotion.precise_movement = on;
            }
        }
        // The follow-camera distance preset, the same host knob the native
        // window re-asserts each tick (`window/event_handler/redraw.rs`).
        self.scene_host.camera_mut().distance = self.options_state.camera_distance;
    }
}
