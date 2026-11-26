//! Which screen owns a sim tick: the one order both play hosts hand each
//! tick through.
//!
//! A play host's tick goes to the first owner that takes it. The native
//! window's `sim_tick` and the browser page's `frame_ticks` used to spell
//! that order each, in their own words, and the two orders differed (the
//! page asked the pause menu first, the window the naming prompt). The order
//! is [`BootSession::claim_tick`] now, and each host only acts on the
//! [`TickOwner`] it hands back: the parts of an owner the session can do
//! itself - step the naming prompt, take the prologue hand-off, open the
//! pause menu - are done by the time it returns.

use legaia_engine_audio::AudioSink;

use super::{BootSession, PauseMenuPress};

/// What the host holds that the tick order asks about.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TickClaims {
    /// A screen the host holds owns the frame: the native window's boot UI
    /// (title, save select, the pause menu once spawned, the party-wipe
    /// hold), the page's spawned pause menu and party-wipe hold.
    pub host_screen: bool,
    /// A menu-overlay screen (shop, prize exchange) is open: it owns the
    /// pad, so no pause press reaches the field menu.
    pub overlay_open: bool,
    /// The open overlay screen suspends the field (a shop, the prize
    /// exchange): retail runs it at game mode `0x17` with the field overlay
    /// swapped out, so it takes the whole tick.
    pub overlay_suspends: bool,
    /// The host can draw a pause menu now (the browser page builds its menu
    /// assets on first use). The native window always can.
    pub pause_menu_ready: bool,
    /// The pad word held this tick. A tick the field does not run takes
    /// retail's pad pump from it (`InputState::pump_held`), which is what
    /// raises the cursor auto-repeat the pause menu, a shop and the naming
    /// prompt read; a field tick pumps through the host's `set_pad`.
    pub held: u16,
}

/// Who owns one sim tick ([`BootSession::claim_tick`]).
///
/// Tuple variants on purpose: the host-drift gate's frame-path tier reads a
/// host's arms by their header text, and a braced pattern would hide it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TickOwner {
    /// A host-held screen owns the frame ([`TickClaims::host_screen`]): the
    /// host steps it, and the world does not tick.
    HostScreen,
    /// The naming prompt took the tick and has been stepped on its edge
    /// (`step_name_entry_frame`). `true` = the name committed this tick.
    NameEntry(bool),
    /// Retail's prologue intro skip (`FUN_801D1344`): a Cross press during
    /// the opening chain handed off to the named scene. The host enters it
    /// (and rebuilds whatever it draws the scene with); the tick is spent.
    PrologueSkip(&'static str),
    /// The pause menu's wipe has spawned it: from this tick on the host's
    /// menu screen owns the frame. This tick is the hand-over (no input).
    PauseMenuSpawned,
    /// A pause press opened the menu (`press_field_menu`). `true` = a
    /// script's op-`0x49` press, which plays no confirm blip. The field
    /// darkens under the wipe from the next tick.
    PauseMenuOpened(bool),
    /// A menu-overlay screen that suspends the field (shop, prize exchange)
    /// takes the tick: the host steps its session on the tick's edge.
    OverlayScreen,
    /// The field ticks. `true` = on a neutral pad: the opening narration
    /// crawl or title card owns the pad, or an inn prompt is up over the
    /// field (the player must not walk while it is).
    Field(bool),
}

impl<S: AudioSink> BootSession<S> {
    /// Hand one sim tick to its owner, in the one order both play hosts
    /// share. `edge` is the tick's just-pressed pad bits.
    // REF: FUN_801D1344, FUN_801D01B0
    pub fn claim_tick(&mut self, claims: TickClaims, edge: u16) -> TickOwner {
        // The naming prompt reads the cursor auto-repeat on the tick it is
        // stepped, so its pump comes first; every other owner but the field
        // is pumped on the way out. The field's tick pumps in `set_pad`.
        let naming = !claims.host_screen && self.host.world.name_entry_active();
        if naming {
            self.host.world.input.pump_held(claims.held);
        }
        let owner = self.claim_tick_owner(claims, edge);
        match owner {
            TickOwner::Field(_) => {}
            // The list opens on a clean count, whatever was held coming in
            // (`FUN_80030628` zeroes the chord).
            TickOwner::PauseMenuOpened(_) | TickOwner::PauseMenuSpawned => {
                self.host.world.input.pump_held(claims.held);
                self.host.world.input.reset_menu_repeat();
            }
            _ if naming => {}
            _ => self.host.world.input.pump_held(claims.held),
        }
        owner
    }

    /// [`Self::claim_tick`]'s order, without the pad pump.
    fn claim_tick_owner(&mut self, claims: TickClaims, edge: u16) -> TickOwner {
        // Every tick handed out is a vsync the game ran, whoever owns it:
        // retail's play counter runs under the pause menu, the title and a
        // movie as it does on the field (`count_play_tick`).
        self.count_play_tick();
        // A host-held screen first: the player has not entered the world, or
        // has paused out of it.
        if claims.host_screen {
            return TickOwner::HostScreen;
        }
        // The naming prompt is modal: retail parks the opening's script on
        // it, so while it is up it takes the tick.
        if self.step_name_entry_frame(edge) {
            return TickOwner::NameEntry(!self.host.world.name_entry_active());
        }
        // The prologue intro skip: while the opening chain plays with the
        // trigger bit armed, a held Select skips the whole remaining opening
        // (retail tests the held word, `FUN_801D1344`); Cross, which a player
        // presses to hurry the narration along, does not.
        if let Some(target) = self
            .host
            .world
            .take_prologue_handoff(legaia_engine_core::world::prologue_skip_held(claims.held))
        {
            return TickOwner::PrologueSkip(target);
        }
        // The pause menu spawns partway through the field's wipe to black
        // (`pause_wipe`): until then the field keeps ticking under the wipe
        // (the player held by the engaged bit the press raised).
        if self.field_menu_is_open() {
            if self.pause_wipe().menu_spawned() {
                return TickOwner::PauseMenuSpawned;
            }
            return TickOwner::Field(self.field_pad_neutral(claims));
        }
        // The press: a script's op-`0x49` save point / ready check, a Start
        // edge, the deny buzz - the one rule `press_field_menu` holds. A
        // menu-overlay screen owns the pad, so no press reaches the menu
        // while one is up.
        // Retail's accept is the field controller's (`0x801D0250`), on the
        // edge of the pad the frame pacer sampled: under the retail pad
        // pipeline the press is that word's, as the walk's is.
        let menu_edge = if self.host.world.retail_pad_pipeline_live() {
            let game = self.host.world.preview_frame_pad(claims.held);
            game & !self.host.world.input.pad()
        } else {
            edge
        };
        if !claims.overlay_open
            && claims.pause_menu_ready
            && let PauseMenuPress::Opened { scripted } =
                self.press_field_menu(menu_edge & super::MENU_BUTTONS != 0)
        {
            return TickOwner::PauseMenuOpened(scripted);
        }
        if claims.overlay_suspends {
            return TickOwner::OverlayScreen;
        }
        TickOwner::Field(self.field_pad_neutral(claims))
    }

    /// Count one tick on the world's play clock when the mode word's flag
    /// bit says retail's counter runs (`GameMode::play_counter_runs`). Called
    /// by [`Self::claim_tick`]; a host that spends further ticks without
    /// claiming them (the page's party-wipe hold) calls it for each.
    pub fn count_play_tick(&mut self) {
        if self.mode_seat.game_mode().play_counter_runs() {
            self.host.world.count_play_tick();
        }
    }

    /// Whether the field ticks on a neutral pad: the narration crawl or
    /// title card owns the pad (the timeline drives the scene), or an
    /// overlay screen is up.
    fn field_pad_neutral(&self, claims: TickClaims) -> bool {
        claims.overlay_open || self.world_holds_field_pad()
    }

    /// The half of [`Self::claim_tick`]'s neutral-pad rule the world itself
    /// decides: the narration crawl or a title card owns the pad. A headless
    /// driver that feeds the world's pad directly (the full-game ladder's
    /// hand) asks this before its `set_pad`, so a surface that eats the pad
    /// on both play hosts eats it there too - the shape of the Uru Mais
    /// softlock, where a card left up held the field's pad neutral under a
    /// box the player had to turn.
    pub fn world_holds_field_pad(&self) -> bool {
        world_holds_field_pad(&self.host.world)
    }
}

/// [`BootSession::world_holds_field_pad`] over a bare world: the narration
/// crawl or a title card owns the field's pad. For a driver that ticks a
/// `SceneHost` without a session (`critical_path_replay`), so it gates its
/// presses as both play hosts do.
pub fn world_holds_field_pad(world: &legaia_engine_core::world::World) -> bool {
    world.cutscene_narration_active() || world.cutscene.card.is_some()
}
