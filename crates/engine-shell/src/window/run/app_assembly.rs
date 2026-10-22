//! Assembling the [`PlayWindowApp`] a launch runs: the session, the decoded
//! UI assets and the launch flags in, every other field at its start value.

use super::*;

/// What a launch hands [`PlayWindowApp::assemble`].
pub(super) struct WindowParts<'a> {
    pub(super) session: NativeSession,
    pub(super) font: Font,
    pub(super) scene_res: SceneResources,
    pub(super) publisher_logos_atlas_data: Option<legaia_engine_title::publisher_logos::LogosAtlas>,
    pub(super) title_screen_atlas_data:
        Option<legaia_engine_title::title_screen_atlas::TitleScreenAtlas>,
    pub(super) menu_glyph_atlas_data: Option<legaia_engine_menus::menu_glyph_atlas::MenuGlyphAtlas>,
    pub(super) save_menu_atlas_data: Option<legaia_engine_menus::save_menu_atlas::SaveMenuAtlas>,
    pub(super) menu_window_table: Option<legaia_game_tables::menu_windows::MenuWindowTable>,
    pub(super) mapping: legaia_engine_system::input::Mapping,
    pub(super) initial_boot_ui: BootUiState,
    pub(super) mounted_card: Option<MountedCard>,
    pub(super) port1_card: Option<MountedCard>,
    pub(super) screenshot: Option<ScreenshotConfig>,
    pub(super) record_to: Option<RecordTarget>,
    pub(super) field_live_opts: legaia_engine_session::session::FieldLiveOpts,
    pub(super) save_dir: &'a Path,
    pub(super) extracted_root: &'a Path,
    pub(super) disc: Option<&'a Path>,
}

impl PlayWindowApp {
    /// The window state a launch starts from.
    pub(super) fn assemble(parts: WindowParts<'_>) -> Self {
        let WindowParts {
            session,
            font,
            scene_res,
            publisher_logos_atlas_data,
            title_screen_atlas_data,
            menu_glyph_atlas_data,
            save_menu_atlas_data,
            menu_window_table,
            mapping,
            initial_boot_ui,
            mounted_card,
            port1_card,
            screenshot,
            record_to,
            field_live_opts,
            save_dir,
            extracted_root,
            disc,
        } = parts;
        PlayWindowApp {
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
            menu_window_table,
            caption_atlas: None,
            uploaded_vram: None,
            meshes: Vec::new(),
            scene_tmd_data: Vec::new(),
            field_placement_draws: Vec::new(),
            field_posed_props: Vec::new(),
            field_posed_tmds: Vec::new(),
            field_stager_tmds: Vec::new(),
            field_pack_mesh_idx: Vec::new(),
            field_morph_live: std::collections::HashMap::new(),
            color_meshes: Vec::new(),
            field_placement_color_draws: Vec::new(),
            field_placement_window_keys: Vec::new(),
            field_placement_records: Vec::new(),
            field_placement_color_records: Vec::new(),
            field_pack_meshes: Vec::new(),
            field_pack_color_meshes: Vec::new(),
            field_placement_color_window_keys: Vec::new(),
            field_placement_cell_keys: Vec::new(),
            field_placement_color_cell_keys: Vec::new(),
            field_terrain_draws: Vec::new(),
            field_lit: Default::default(),
            draw_census: std::env::var_os("LEGAIA_DIAG_DRAWS").map(|_| Default::default()),
            field_floor_wave: Default::default(),
            coplanar_env_offsets: std::collections::HashMap::new(),
            field_terrain_color_draws: Vec::new(),
            world_map_terrain_draws: Vec::new(),
            world_map_terrain_color_draws: Vec::new(),
            world_map_terrain_records: Vec::new(),
            world_map_terrain_color_records: Vec::new(),
            world_map_deco_start: (0, 0),
            ground_heightfield: None,
            ground_src: None,
            ground_crop: None,
            field_terrain_cell_keys: Vec::new(),
            field_terrain_color_cell_keys: Vec::new(),
            field_terrain_facing: Vec::new(),
            field_terrain_color_facing: Vec::new(),
            // Headless capture harnesses can't press `F3`; let them start on the
            // wide debug vantage via the env switch.
            field_debug_camera: std::env::var_os("LEGAIA_FIELD_DEBUG_CAM").is_some(),
            menu_from_title: false,
            world_map_slot4_lines: None,
            ocean_anim: None,
            cpu_vram_base: None,
            dance_vram_restore: None,
            battle_vram: None,
            battle_intro: None,
            battle_intro_vram: None,
            battle_vram_generation: None,
            battle_faces: Vec::new(),
            battle_face_tables: Default::default(),
            dev_menu: None,
            dev_menu_draws: Vec::new(),
            dev_menu_records: false,
            fishing_banners: Default::default(),
            fishing_banner_draws: Vec::new(),
            baka_hud_widgets: None,
            baka_chrome_frame: Vec::new(),
            baka_surface: Default::default(),
            baka_gpu: None,
            muscle_surface: Default::default(),
            muscle_gpu: None,
            muscle_mesh_key: None,
            fishing_surface: Default::default(),
            fishing_gpu: None,
            slot_cabinet_assets: None,
            slot_gpu: None,
            slot_marquee_clock: Default::default(),
            slot_dots: Vec::new(),
            dance_venue_gpu: None,
            dance_venue_failed: None,
            dance_cast_surface: Default::default(),
            dance_cast_gpu: None,
            muscle_hub: None,
            summon_actor_slot: None,
            battle_stage_mesh: None,
            battle_stage_spun_mesh: None,
            battle_stage_spun_color_mesh: None,
            battle_stage_color_mesh: None,
            battle_stage_shell: None,
            battle_ground_mesh: None,
            battle_ground_cue_far: None,
            battle_ground_rgbc: legaia_engine_battle_vm::battle_ground_grid::GRID_RGBC_SETTLED,
            battle_stage_outdoor: false,
            prev_scene_mode: None,
            monster_archive: None,
            battle_mesh_base: 0,
            battle_rest_vmesh: std::collections::HashMap::new(),
            battle_color_mesh_base: 0,
            scene_aabb: ([f32::NEG_INFINITY; 3], [f32::INFINITY; 3]),
            pad: 0,
            mapping,
            keys_down: std::collections::HashSet::new(),
            persist_disabled: false,
            pending_key_name: None,
            menu_runtime: MenuRuntime::new(save_dir.to_path_buf()),
            prev_pad: 0,
            pad_taps: Default::default(),
            gamepad: None,
            gamepad_held: 0,
            picker_swallow: 0,
            tick_no: 0,
            screenshot,
            sweep_next_tick: 0,
            battle_event_log: std::collections::VecDeque::new(),
            encounter_banner: None,
            battle_hud: legaia_engine_battle::battle_hud_state::BattleHud::new(),
            pending_dynamic_mesh_slots: Vec::new(),
            drained_spawn_slots: std::collections::HashSet::new(),
            tile_slots_queued: std::collections::HashSet::new(),
            player_color_draw: None,
            field_npc_draws: Vec::new(),
            npc_clip_players: std::collections::HashMap::new(),
            npc_anim_srcs: std::collections::HashMap::new(),
            npc_rest_srcs: std::collections::HashMap::new(),
            npc_morph_static: std::collections::HashMap::new(),
            npc_pose_cache: std::collections::HashMap::new(),
            npc_pose_verify: std::collections::HashMap::new(),
            npc_anim_bundles: (None, None),
            npc_bundle_special: std::collections::HashMap::new(),
            scene_actors_pending: true,
            boot_ui: initial_boot_ui,
            card: mounted_card,
            port1_card,
            save_flow: legaia_engine_menus::save_screen::SaveScreenFlow::new(),
            options_pins: Default::default(),
            options_state: legaia_engine_menus::options::OptionsState::load_or_default(
                &std::path::PathBuf::from(OPTIONS_CONFIG_FILE),
            ),
            record_log: record_to.map(RecordLog::from_target),
            field_live_opts,
            // In-flow cutscene STR resolves from the extracted root (video only)
            // or, when booting from a disc image, straight from the ISO with its
            // interleaved XA audio. Exactly one of these is set.
            extracted_root: disc.map_or_else(|| Some(extracted_root.to_path_buf()), |_| None),
            disc_path: disc.map(|d| d.to_path_buf()),
            cutscene: None,
            movie_score: legaia_engine_system::movie::movie_audio::MovieScore::new(),
            cutscene_glide: legaia_engine_field::frame_step::CutsceneGlide::new(),
            sim_stepper: legaia_engine_field::frame_step::SimStepper::new(),
            active_dialog: None,
            seru_names: None,
            // Resolved against the persisted option right below, once the
            // options file has loaded.
            dynamic_lighting: false,
            psx_render: std::env::var_os("LEGAIA_PSX_RENDER").is_some(),
            window_launch: Default::default(),
            alt_held: false,
            fullscreen_key_held: false,
            field_occluders: Default::default(),
            occl_fade_strength: std::cell::Cell::new(Default::default()),
            scene_point_lights: Vec::new(),
            scene_prop_lights: Vec::new(),
            orbit_drag_last_x: None,
            orbit_drag_last_y: None,
            last_left_press: None,
            // `atan(0.85)`: the angle the window's long-standing eye-height
            // ratio encoded, so an untouched debug vantage is unchanged.
            debug_orbit_pitch: 0.85f32.atan(),
            debug_orbit_zoom: 1.0,
            cursor_x: 0.0,
            cursor_y: 0.0,
            field_party_hud: Default::default(),
            field_party_hud_scene: None,
            diag_rows: legaia_engine_ui::diag_hud_enabled(),
            quit_prompt: None,
            notice: None,
            hotkey_help: false,
            // Armed by `cmd_play_window_with_record` for an interactive
            // window only.
            startup_hint: Default::default(),
            help_key_held: None,
            caption_title: crate::window::minigames::WINDOW_TITLE.to_string(),
        }
    }

    /// Retail-shaped equipment buy: this window draws the recipient picker
    /// (window 36) and the two stat-compare windows (25 / 41) over the parked
    /// buy list (`legaia_engine_screens`), so opt into the flow and install
    /// the disc restrictions the buy-list kind dispatch reads. Same arming
    /// the browser play page performs at `load_disc`; without the table the
    /// route falls back to the quantity picker, so the opt-in is gated on it.
    ///
    /// Then the launch's option overrides and the persisted options pushed
    /// into their live consumers, before the loop starts.
    pub(super) fn apply_launch_options(&mut self, fidelity: crate::window::LaunchFidelity) {
        let crate::window::LaunchFidelity {
            volumetric_fog,
            dynamic_lighting,
            ..
        } = fidelity;
        // `--retail` pins the persisted engine-only options for this session
        // only, the way the fog override below does: nothing is written back.
        if fidelity.retail {
            self.options_pins.pin_retail(&mut self.options_state);
        }
        self.psx_render |= fidelity.psx_render;
        self.window_launch = fidelity.window;
        if let Some(info) = self.session.equip_restrictions.clone() {
            self.menu_runtime.install_equip_info(info);
            self.menu_runtime.retail_equipment_buy = true;
        }

        // `--no-volumetric-fog` (and a recording, which stays on the faithful
        // render) lowers the volumetric ground-fog option for this session
        // only: the knob is pinned, so a save writes the stored value back
        // (it used to write the override into the options file on the next
        // options close or hotkey).
        if !volumetric_fog {
            self.options_pins.pin(
                &self.options_state,
                legaia_engine_menus::options::options_pin::VOLUMETRIC_FOG,
            );
            self.options_state.volumetric_fog = false;
        }
        // `--no-dyn-shadows` / `--no-occlusion-fade`: the same session-only
        // pin over the persisted render options the `Y` / `F4` keys and the
        // page's boxes edit.
        if !fidelity.dyn_shadows {
            self.options_pins.pin(
                &self.options_state,
                legaia_engine_menus::options::options_pin::LIGHT_SHADOWS,
            );
            self.options_state.light_shadows = false;
        }
        if !fidelity.occlusion_fade {
            self.options_pins.pin(
                &self.options_state,
                legaia_engine_menus::options::options_pin::OCCLUSION_FADE,
            );
            self.options_state.occlusion_fade = false;
        }
        // `LEGAIA_BATTLE_CAMERA_OPTION=<0|1|2>`: the retail-compare image
        // child plays the capture's own Battle Camera word (`0x800846C0`).
        // Pinned like the fog: a session override, which neither a loaded
        // save's word nor the options file moves.
        if let Some(v) = std::env::var("LEGAIA_BATTLE_CAMERA_OPTION")
            .ok()
            .and_then(|v| v.trim().parse::<u8>().ok())
        {
            self.options_pins.pin(
                &self.options_state,
                legaia_engine_menus::options::OptionsSetting::BattleCamera.pin_bit(),
            );
            self.options_state.battle_camera =
                legaia_engine_system::option_values::BattleCameraOpt::from_word(v);
        }
        // Push the loaded options into their live consumers (audio downmix)
        // before the loop starts.
        self.apply_options_side_effects();

        // Enhanced lighting: the persisted option (default on) unless the
        // command line forced it (`--dynamic-lighting` / `--no-dynamic-lighting`;
        // replays force it off).
        self.dynamic_lighting = dynamic_lighting.unwrap_or(self.options_state.enhanced_lighting);

        // Camera framing + movement toggles from the persisted options file:
        // the distance preset (default `far` - a bit more on screen than
        // retail; `T` cycles) and the precise-movement toggle (`R`). The
        // compass bias tells the engine-core camera what fixed yaw this
        // window's follow camera renders at (compass sense = the negated PSX
        // render yaw), so `NativeSession::tick`'s d-pad remap feed tracks the
        // on-screen view exactly - including after a left-mouse drag-orbit.
        self.session.camera.render_yaw_bias =
            legaia_engine_field::camera_frame::retail_field_render_yaw_bias();
        // The options' simulation knobs (precise movement, Field Move - pause
        // menu Walk / Run, retail config word 0x800846CC, which the run BUTTON
        // inverts per frame, see `World::field_run_active` - reduce flashing,
        // the battle options, the tile windows, the ground fog) and the
        // distance preset, through the session step the page also runs at
        // disc load, so the world holds them from the launch on rather than
        // from the first tick's re-assert.
        self.session.apply_option_knobs(&self.options_state, None);
        log::info!(
            "camera: distance = {} (T cycles); precise movement {} (R toggles); drag to orbit",
            self.options_state.camera_distance.label(),
            if self.options_state.precise_movement {
                "ON"
            } else {
                "off"
            }
        );
        log::info!(
            "field move: {} by default (hold the run button to invert)",
            if self.session.host.world.locomotion.run_default {
                "RUN"
            } else {
                "walk"
            }
        );
    }
}
