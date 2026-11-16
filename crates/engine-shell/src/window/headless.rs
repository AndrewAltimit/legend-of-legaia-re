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
        let save_dir = std::env::temp_dir().join("legaia-headless-window-saves");
        Self::open_with(disc, scene, false, None, &save_dir)
    }

    /// [`Self::open`] over the save directory `save_dir`
    /// (`play-window --save-dir <save_dir>`): where the Tools page's session
    /// export and its report file land.
    pub fn open_in(disc: &Path, scene: &str, save_dir: &Path) -> Result<Self> {
        Self::open_with(disc, scene, false, None, save_dir)
    }

    /// The Tools page's *Export session* row: the running game as an
    /// `.lgsf` file in the save directory; the path, or the refusal.
    pub fn export_session(&mut self) -> std::result::Result<std::path::PathBuf, String> {
        self.app.export_session()
    }

    /// The Tools page's *Copy report* text.
    pub fn session_report(&self) -> String {
        self.app.session_report()
    }

    /// One press of the Keys page's reset row (two presses reset).
    pub fn keys_reset_press(&mut self) {
        self.app.keys_reset_press();
    }

    /// The Keys page's footer line as it draws for a panel that holds the
    /// game and a keyboard player.
    pub fn keys_footer(&self) -> String {
        super::hud::keys_footer(self.app.keys_reset, true, None)
    }

    /// [`Self::open`] as `play-window --boot-ui [--card <card>]` launches:
    /// the boot front end (publisher logos, then the title card) owns the
    /// frames until the title resolves, over the pre-booted `scene`. `card`
    /// is the memory-card image mounted in **port 2**, the one `--card`
    /// names; port 1 is the window's own card in a scratch save directory.
    /// Nothing this window saves reaches either file.
    pub fn open_boot_ui(disc: &Path, scene: &str, card: Option<&Path>) -> Result<Self> {
        let save_dir = std::env::temp_dir().join("legaia-headless-window-saves");
        Self::open_with(disc, scene, true, card, &save_dir)
    }

    /// [`Self::open_boot_ui`] over the save directory `save_dir`, as
    /// `play-window --boot-ui --save-dir <save_dir>` launches: port 1 is the
    /// card file there (`memcard1.mcr`), which a Save writes back to disk
    /// and a later window over the same directory reads - a quit and
    /// relaunch. The options and bindings files stay unwritten.
    pub fn open_boot_ui_in(
        disc: &Path,
        scene: &str,
        card: Option<&Path>,
        save_dir: &Path,
    ) -> Result<Self> {
        let mut w = Self::open_with(disc, scene, true, card, save_dir)?;
        w.app.persist_cards = true;
        Ok(w)
    }

    fn open_with(
        disc: &Path,
        scene: &str,
        boot_ui: bool,
        card: Option<&Path>,
        save_dir: &Path,
    ) -> Result<Self> {
        let mut app = assemble_play_window(PlayWindowLaunch {
            scene,
            extracted_root: Path::new("extracted"),
            disc: Some(disc),
            enable_audio: true,
            audio_detached: true,
            world_map: false,
            boot_ui,
            save_dir,
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

    /// Hold physical controller buttons - a bit per W3C standard index
    /// (`1 << 0` is the bottom face button) - and a left-stick position (`y`
    /// down-positive) from the next [`Self::controller_frame`] on. Unlike
    /// [`Self::set_controller`] the poll folds them through the window's
    /// live table and dead zone, so a Key Config bind and the options' stick
    /// row reach them, and a fresh press is handed to Key Config as a
    /// capture, as a real controller's is.
    pub fn set_controller_buttons(&mut self, held: u32, stick: (f32, f32)) {
        self.app
            .gamepad
            .get_or_insert_with(super::gamepad::GamepadHost::scripted)
            .set_scripted_buttons(held, stick);
    }

    /// The live binding table (keys and controller), as Key Config leaves
    /// it, mutable for a test that binds the way the screen would.
    pub fn mapping_mut(&mut self) -> &mut legaia_engine_system::input::Mapping {
        &mut self.app.mapping
    }

    /// The live options (`OptionsState`), mutable for a test that steps a
    /// row the panel would.
    pub fn options_mut(&mut self) -> &mut legaia_engine_menus::options::OptionsState {
        &mut self.app.options_state
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

    /// Activate the expansion pack in directory `pack` and enter its map
    /// `map` over the running scene through the session's entry
    /// (`BootSession::enter_expansion_map_live`, what `--expansion --map`
    /// runs at launch), then rebuild the scene render state as a pack
    /// reload does.
    pub fn enter_expansion_map(&mut self, pack: &Path, map: &str) -> Result<()> {
        let pack = legaia_expansion_format::ExpansionPack::from_dir(pack)?;
        self.app
            .session
            .host
            .activate_expansion_pack(std::sync::Arc::new(pack))?;
        let opts = self.app.field_live_opts.clone();
        self.app.session.enter_expansion_map_live(map, "", &opts)?;
        self.app.rebuild_scene_render_state();
        Ok(())
    }

    /// Stage an in-window movie of `frames` blank frames, as the redraw's
    /// movie start does once a STR has decoded: the world's ticks are held
    /// under it until its frames drain or it is skipped. The headless
    /// driver draws nothing, so nothing advances the movie's frame.
    pub fn stage_movie(&mut self, frames: usize) {
        let blank = psx_mdec::VideoFrame {
            rgba: Vec::new(),
            width: 0,
            height: 0,
            frame_number: 0,
        };
        self.app.stage_windowed_cutscene((
            vec![blank; frames],
            std::time::Duration::from_millis(66),
            None,
        ));
    }

    /// Put the window in a room over `link`, as `--join` does over a socket:
    /// the room's frame then runs on every tick ([`Self::room_frames`]).
    pub fn join_room(
        &mut self,
        link: Box<dyn legaia_netplay::transport::Transport>,
        code: legaia_netplay::protocol::RoomCode,
    ) {
        self.app.netplay = Some(super::netplay::NetHost::joined(link, code));
    }

    /// Room frames the window's room controller has stepped
    /// (`NetPlay::frames`), `None` out of a room.
    pub fn room_frames(&self) -> Option<u32> {
        self.app.netplay.as_ref().map(|n| n.net.frames())
    }

    /// The field VRAM the window's draws sample (`cpu_vram_base`): the
    /// scene build with what the window layers on it.
    pub fn field_vram(&self) -> Option<&psx_tim::Vram> {
        self.app.cpu_vram_base.as_ref()
    }

    /// The session the window runs.
    pub fn session(&self) -> &NativeSession {
        &self.app.session
    }

    /// The session, mutably (a test's seeding).
    pub fn session_mut(&mut self) -> &mut NativeSession {
        &mut self.app.session
    }

    /// The window's menu-overlay screen (shop, prize exchange).
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

    /// The phase of the pause menu's Load / Save screen, while the window
    /// routes the frame to it (the menu's `FieldMenu` arm with a Save
    /// sub-session pushed).
    pub fn save_select_phase(&self) -> Option<legaia_engine_menus::save_select::SelectPhase> {
        use legaia_engine_menu_runtime::field_menu_dispatch::FieldMenuSubsession;
        match &self.app.boot_ui {
            BootUiState::FieldMenu {
                sub: Some(FieldMenuSubsession::Save(s)),
            } => Some(s.phase()),
            _ => None,
        }
    }

    /// Whether the window's pause menu owns the frame (the boot-UI arm).
    pub fn boot_ui_active(&self) -> bool {
        self.app.boot_ui.is_active()
    }
}
