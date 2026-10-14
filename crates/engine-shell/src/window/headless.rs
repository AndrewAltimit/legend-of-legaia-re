//! The play window with no window: the [`PlayWindowApp`] a `play-window`
//! launch assembles, driven one display frame at a time with a caller-given
//! pad word and tick count, and no renderer.
//!
//! This exists so the native host's **own** frame body - the per-tick arms
//! of `sim_tick` (pause menu, name entry, shop freeze, prologue hand-off)
//! and the steps around the session tick - can be compared against the
//! browser play page's under one pad script, in a cargo test. Driving
//! `NativeSession` directly compares the session, which both hosts already
//! share; the drift lives in what each host wraps around it.
//!
//! A frame here is the redraw minus the draw: the sim ticks
//! (`run_sim_ticks`), then the prep steps that move world state from the
//! draw path - the battle frame edge, the cutscene camera glide and the
//! fog pool's render step. Every renderer-bound step self-gates on the
//! renderer being absent.

use super::*;

/// The native play window, headless. See the module docs.
pub struct HeadlessPlayWindow {
    app: PlayWindowApp,
}

impl HeadlessPlayWindow {
    /// Assemble the window `play-window --disc <disc> --scene <scene>` would
    /// run, with every command-line switch at its default, audio off and the
    /// persisted options file ignored (the options start at their defaults,
    /// as a fresh browser profile's do).
    pub fn open(disc: &Path, scene: &str) -> Result<Self> {
        let save_dir = std::env::temp_dir().join("legaia-headless-window-saves");
        let mut app = assemble_play_window(PlayWindowLaunch {
            scene,
            extracted_root: Path::new("extracted"),
            disc: Some(disc),
            enable_audio: false,
            world_map: false,
            boot_ui: false,
            save_dir: &save_dir,
            card: None,
            cheat_file: None,
            cheat_strict: false,
            live_loop: true,
            player_battle: true,
            party: None,
            vm_dialogue: true,
            terrain_y: true,
            edge_collision: true,
            solid_npcs: true,
            live_npcs: true,
            damage_finish: true,
            battle_bgm: None,
            screenshot: None,
            seed_party: false,
            battle: None,
            fidelity: LaunchFidelity::enhanced(),
            debug_seeds: DebugSeeds::default(),
            record_to: None,
        })?;
        // The persisted options file in the working directory is this
        // machine's, not the launch's: start from the defaults, then push
        // them into their consumers the way the launch did.
        app.options_state = legaia_engine_menus::options::OptionsState::default();
        app.session.camera.distance = app.options_state.camera_distance;
        app.session.host.world.locomotion.precise_movement = app.options_state.precise_movement;
        app.session.host.world.locomotion.run_default =
            app.options_state.field_move == legaia_engine_menus::option_values::FieldMoveOpt::Run;
        // The window's `resumed` upload, which with no renderer runs only its
        // world-side half (`upload_assets`).
        app.upload_assets();
        Ok(Self { app })
    }

    /// One display frame: hold `pad` (the word the key events maintain) and
    /// run `ticks` sim ticks, then the draw path's world-moving steps.
    pub fn display_frame(&mut self, pad: u16, ticks: u32) {
        self.ticks(pad, ticks);
        self.prep();
    }

    /// The frame's tick half alone: hold `pad` and run `ticks` sim ticks.
    pub fn ticks(&mut self, pad: u16, ticks: u32) {
        self.app.pad = pad;
        self.app.run_sim_ticks(ticks);
    }

    /// The frame's draw-path half alone: the steps after the ticks that move
    /// world state.
    pub fn prep(&mut self) {
        self.app.headless_frame_prep();
    }

    /// The session the window runs.
    pub fn session(&self) -> &NativeSession {
        &self.app.session
    }

    /// The session, mutably (a test's seeding).
    pub fn session_mut(&mut self) -> &mut NativeSession {
        &mut self.app.session
    }

    /// The window's menu-overlay screen (shop, prize exchange, inn).
    pub fn menu_runtime(&self) -> &legaia_engine_core::menu_runtime::MenuRuntime {
        &self.app.menu_runtime
    }

    /// Whether the window's pause menu owns the frame (the boot-UI arm).
    pub fn boot_ui_active(&self) -> bool {
        self.app.boot_ui.is_active()
    }
}
