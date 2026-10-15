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
use legaia_engine_system::input::PadButton;

use super::{BootSession, PauseMenuPress};

/// What the host holds that the tick order asks about.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TickClaims {
    /// A screen the host holds owns the frame: the native window's boot UI
    /// (title, save select, the pause menu once spawned, the party-wipe
    /// hold), the page's spawned pause menu and party-wipe hold.
    pub host_screen: bool,
    /// A menu-overlay screen (shop, prize exchange, inn) is open: it owns the
    /// pad, so no pause press reaches the field menu.
    pub overlay_open: bool,
    /// The open overlay screen suspends the field (a shop, the prize
    /// exchange): retail runs it at game mode `0x17` with the field overlay
    /// swapped out, so it takes the whole tick. An inn prompt does not.
    pub overlay_suspends: bool,
    /// The host can draw a pause menu now (the browser page builds its menu
    /// assets on first use). The native window always can.
    pub pause_menu_ready: bool,
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
        // trigger bit armed, a Cross press skips the whole remaining opening.
        if let Some(target) = self
            .host
            .world
            .take_prologue_handoff(edge & PadButton::Cross.mask() != 0)
        {
            return TickOwner::PrologueSkip(target);
        }
        // The pause menu spawns partway through the field's wipe to black
        // (`pause_wipe`): until then the field keeps ticking under the wipe
        // (with the world in `Menu`, so the player stands still).
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
        if !claims.overlay_open
            && claims.pause_menu_ready
            && let PauseMenuPress::Opened { scripted } =
                self.press_field_menu(edge & PadButton::Start.mask() != 0)
        {
            return TickOwner::PauseMenuOpened(scripted);
        }
        if claims.overlay_suspends {
            return TickOwner::OverlayScreen;
        }
        TickOwner::Field(self.field_pad_neutral(claims))
    }

    /// Whether the field ticks on a neutral pad: the narration crawl or
    /// title card owns the pad (the timeline drives the scene), or an
    /// overlay screen is up.
    fn field_pad_neutral(&self, claims: TickClaims) -> bool {
        let world = &self.host.world;
        claims.overlay_open || world.cutscene_narration_active() || world.cutscene.card.is_some()
    }
}
