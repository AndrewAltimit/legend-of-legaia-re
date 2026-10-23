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
    /// run, with every command-line switch at its default, the audio director
    /// over a device-less output (`AudioOut::detached`: it stages and routes
    /// every command, and nothing is heard) and the persisted options file
    /// ignored (the options start at their defaults,
    /// as a fresh browser profile's do).
    pub fn open(disc: &Path, scene: &str) -> Result<Self> {
        Self::open_with(disc, scene, false, None)
    }

    /// [`Self::open`] as `play-window --boot-ui [--card <card>]` launches:
    /// the boot front end (publisher logos, then the title card) owns the
    /// frames until the title resolves, over the pre-booted `scene`. `card`
    /// is the memory-card image mounted in **port 2**, the one `--card`
    /// names; port 1 is the window's own card in a scratch save directory.
    /// Nothing this window saves reaches either file.
    pub fn open_boot_ui(disc: &Path, scene: &str, card: Option<&Path>) -> Result<Self> {
        Self::open_with(disc, scene, true, card)
    }

    fn open_with(disc: &Path, scene: &str, boot_ui: bool, card: Option<&Path>) -> Result<Self> {
        let save_dir = std::env::temp_dir().join("legaia-headless-window-saves");
        let mut app = assemble_play_window(PlayWindowLaunch {
            scene,
            extracted_root: Path::new("extracted"),
            disc: Some(disc),
            enable_audio: true,
            audio_detached: true,
            world_map: false,
            boot_ui,
            save_dir: &save_dir,
            card,
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
        app.persist_disabled = true;
        app.options_state = legaia_engine_menus::options::OptionsState::default();
        app.session.apply_option_knobs(&app.options_state, None);
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

    /// Hold `word` (a PSX pad word) on the window's controller from the next
    /// [`Self::controller_frame`] on, installing a scripted controller the
    /// first time (`window::gamepad`). The frame reads it the way a redraw
    /// reads a real controller: polled once ahead of the ticks, kept apart
    /// from the keyboard's word, ORed into every tick.
    pub fn set_controller(&mut self, word: u16) {
        self.app
            .gamepad
            .get_or_insert_with(super::gamepad::GamepadHost::scripted)
            .set_scripted(word);
    }

    /// One display frame with the keyboard holding `pad` and the controller
    /// polled first, as the redraw orders them (`run_frame_ticks`).
    pub fn controller_frame(&mut self, pad: u16, ticks: u32) {
        self.app.pad = pad;
        self.app.poll_gamepad();
        self.app.run_sim_ticks(ticks);
        self.prep();
    }

    /// The frame's draw-path half alone: the steps after the ticks that move
    /// world state.
    pub fn prep(&mut self) {
        self.app.headless_frame_prep();
    }

    /// One physical key press and release, delivered as the window's
    /// keyboard handler receives it (`handle_key`: the hotkeys, the minigame
    /// and debug keys, then the pad mapping). `code` is the
    /// `KeyboardEvent.code` spelling (`F8`, `KeyI`); `false` when no key of
    /// that name exists. Nothing it toggles is persisted.
    pub fn press_key(&mut self, code: &str) -> bool {
        let Some(key) =
            super::launch_options::key_from_name(code.strip_prefix("Key").unwrap_or(code))
        else {
            return false;
        };
        self.app.handle_key(key, ElementState::Pressed);
        self.app.handle_key(key, ElementState::Released);
        true
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
    pub fn menu_runtime(&self) -> &legaia_engine_menu_runtime::menu_runtime::MenuRuntime {
        &self.app.menu_runtime
    }

    /// The window's field party-status readout state (`FUN_801D0D38`'s
    /// countdown, cached position and last decision).
    pub fn field_party_hud(&self) -> &legaia_engine_field::world_map_panel_host::FieldPartyHud {
        &self.app.field_party_hud
    }

    /// The boot front end, while it owns the frame (`open_boot_ui`).
    pub fn front_end(&self) -> Option<&legaia_engine_session::FrontEnd> {
        match &self.app.boot_ui {
            BootUiState::FrontEnd(fe) => Some(fe),
            _ => None,
        }
    }

    /// The title's Continue screen, while it owns the frame.
    pub fn continue_screen(&self) -> Option<&legaia_engine_session::ContinueScreen> {
        match &self.app.boot_ui {
            BootUiState::Continue(c) => Some(c),
            _ => None,
        }
    }

    /// The title's Continue screen, mutably (a test shortening its beats).
    pub fn continue_screen_mut(&mut self) -> Option<&mut legaia_engine_session::ContinueScreen> {
        match &mut self.app.boot_ui {
            BootUiState::Continue(c) => Some(c),
            _ => None,
        }
    }

    /// Whether the window's pause menu owns the frame (the boot-UI arm).
    pub fn boot_ui_active(&self) -> bool {
        self.app.boot_ui.is_active()
    }
}
