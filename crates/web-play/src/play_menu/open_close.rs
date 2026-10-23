//! Opening, closing and querying the pause menu: the open paths, the settle
//! step, the row jump, key routing and the pending load-scene hand-off.

use super::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Open the retail pause menu. No-op with no disc loaded. The field is
    /// frozen by the page while [`Self::play_menu_is_open`] is true.
    ///
    /// The session's own open (`BootSession::open_field_menu`, reached
    /// through `BootSession::press_field_menu`): a [`FieldMenuSession`]
    /// seeded with the world's money + play time, the two row gates sampled
    /// into a [`FieldMenuGate`](legaia_engine_menus::field_menu::FieldMenuGate), and the world switched into
    /// [`SceneMode::Menu`](legaia_engine_mode::mode::SceneMode::Menu) so field dispatch suspends while the menu owns the
    /// frame.
    pub fn play_menu_open(&mut self) {
        // A menu-button press, answered by the engine's one rule
        // (`BootSession::press_field_menu`): a script's op-`0x49` save point /
        // ready check opens with no Start edge and past the engagement gate;
        // a Start edge opens where `World::field_menu_open_allowed` says so
        // (which refuses mid-dialogue, as retail's engaged-bit branch ahead of
        // the accept does); a press the menu lock refuses queues the deny
        // buzz. The native window asks the same call.
        // REF: FUN_801D01B0
        if !self.ensure_menu_assets() || self.play_menu.is_some() {
            return;
        }
        let Some(session) = self.scene_host.session_mut() else {
            return;
        };
        if let legaia_engine_session::PauseMenuPress::Opened { .. } = session.press_field_menu(true)
        {
            self.play_menu = Some(PlayMenu::new());
        }
    }

    /// Open the menu with no press at all - the boot title's Continue /
    /// Options rows, which land on a pause-menu sub-screen rather than on a
    /// Start press. Twin of the native window's `open_menu_row_from_title`,
    /// which calls `BootSession::open_field_menu` directly for the same
    /// reason.
    fn open_play_menu_unpressed(&mut self) -> bool {
        if !self.ensure_menu_assets() || self.play_menu.is_some() {
            return false;
        }
        let Some(session) = self.scene_host.session_mut() else {
            return false;
        };
        session.open_field_menu();
        if !session.field_menu_is_open() {
            return false;
        }
        // No press, so no wipe: the menu owns the frame from the open, as
        // the native title rows hand it straight to the boot-UI arm.
        let mut menu = PlayMenu::new();
        menu.owns_frame = true;
        self.play_menu = Some(menu);
        true
    }

    /// Run a just-opened pause menu's wipe out until the menu exists - the
    /// frames a player spends watching the field go dark. For drivers that
    /// open the menu and then press into it straight away.
    pub fn play_menu_settle(&mut self) {
        for _ in 0..64 {
            if self.play_menu.is_none() || self.pause_menu_spawned() {
                return;
            }
            self.play_menu_input(0);
        }
    }

    /// Whether the pause wipe has spawned the menu (always, outside a wipe).
    pub(crate) fn pause_menu_spawned(&self) -> bool {
        self.scene_host
            .session()
            .is_none_or(|s| s.pause_wipe().menu_spawned())
    }

    /// The open root picker: the session's `BootSession::field_menu`.
    pub(super) fn root_menu(&self) -> Option<&FieldMenuSession> {
        self.scene_host
            .session()
            .and_then(|s| s.field_menu.as_ref())
    }

    /// The open root picker, mutably.
    pub(super) fn root_menu_mut(&mut self) -> Option<&mut FieldMenuSession> {
        self.scene_host
            .session_mut()
            .and_then(|s| s.field_menu.as_mut())
    }

    /// Open the pause menu directly on one row's sub-screen, named the way
    /// [`FieldMenuRow::label`] names it (`"Load"`, `"Options"`, …).
    ///
    /// The boot title's Continue and Options rows land here: retail's title
    /// screen routes them to the same save-select and options screens the
    /// pause menu reaches, and the browser has no second copy of either. The
    /// row is opened through the session's own confirm routing, so a row the
    /// gate blocks stays blocked here too. Returns `false` when the row name
    /// is unknown or the row buzzes.
    pub fn play_menu_open_row(&mut self, row: &str) -> bool {
        let Some(row) = FieldMenuRow::ALL.iter().copied().find(|r| r.label() == row) else {
            return false;
        };
        if !self.open_play_menu_unpressed() {
            return false;
        }
        if !self.root_menu().is_some_and(|m| m.row_is_available(row)) {
            self.play_menu_close();
            return false;
        }
        // Drive the shared picker to the row rather than assigning its
        // cursor: `tick` is what puts the session in `Suspended`, which is
        // what `resume` needs to hand control back on close.
        let steps = usize::from(row.index());
        for _ in 0..steps {
            self.play_menu_input(PadButton::Down.mask());
        }
        self.play_menu_input(PadButton::Cross.mask());
        self.play_menu
            .as_ref()
            .is_some_and(|m| matches!(m.sub, Some(PlaySub::Session(_))))
    }

    /// Mark the open menu as a boot-title row's (see `PlayMenu::from_title`).
    pub(crate) fn play_menu_mark_from_title(&mut self) {
        if let Some(m) = self.play_menu.as_mut() {
            m.from_title = true;
        }
    }

    /// Close the menu (and any open sub-screen), restoring the scene mode the
    /// world ran when it opened - the browser twin of
    /// `BootSession::close_field_menu`.
    pub fn play_menu_close(&mut self) {
        if self.play_menu.take().is_none() {
            return;
        }
        // The session's own close: the suspended scene mode restored, the
        // word adopted on the seat at the close rather than at the next
        // tick's reconcile, and a scripted press's parked op released - the
        // call the native window makes.
        if let Some(session) = self.scene_host.session_mut() {
            session.close_field_menu();
        }
    }

    /// Whether a Start edge would open the pause menu right now:
    /// [`World::field_menu_open_allowed`] over the wire, so the page can grey
    /// its own affordance without re-deriving the rule.
    ///
    /// The page used to answer this itself by comparing
    /// [`Self::scene_mode`] against the string `"Field"`, with a comment
    /// asserting Start was inert "on the world map". Retail says otherwise -
    /// the overworld runs the same locomotion controller the field does - and
    /// a page-side copy of an engine rule is drift the drift gates cannot
    /// see, because no Rust symbol is missing.
    pub fn play_menu_can_open(&self) -> bool {
        self.scene_host
            .host()
            .is_some_and(|h| h.world.field_menu_open_allowed())
    }

    pub fn play_menu_is_open(&self) -> bool {
        self.play_menu.is_some()
    }

    /// Whether a pause-menu **sub-screen** owns the pad (Items, Magic,
    /// Status, Equip, Options, Load / Save, the Arts editor), as opposed to
    /// the root row list.
    ///
    /// The page reads it to decide what Start means. In the native window a
    /// sub-session gets the raw pressed mask and the root list never sees the
    /// press at all (`window/boot_cutscene.rs` returns out of the sub arm), so
    /// Start inside a sub-screen is that screen's business and at most walks
    /// back to the root list. The page treated Start as "close the menu"
    /// unconditionally, which threw away the open sub-session - a half-typed
    /// rebind, a staged equip pick - from a button that does nothing at all
    /// in the window.
    pub fn play_menu_sub_is_open(&self) -> bool {
        self.play_menu.as_ref().is_some_and(|m| m.sub.is_some())
    }

    /// Whether the current scene permits a menu Save
    /// ([`legaia_engine_menus::party_state::PartyState::scene_save_allowed`](legaia_engine_menus::party_state::PartyState::scene_save_allowed),
    /// seeded at scene load from the MAN header bit retail copies into
    /// `_DAT_8007B6A8`). The page shows the Save-here hint from this, and the
    /// menu's own Save row inks and buzzes from the same value through
    /// [`FieldMenuGate`](legaia_engine_menus::field_menu::FieldMenuGate).
    pub fn play_scene_save_allowed(&self) -> bool {
        self.scene_host
            .host()
            .is_some_and(|h| h.world.party.scene_save_allowed)
    }

    /// Hand the menu one physical key-down, as a browser
    /// `KeyboardEvent.code` (`"KeyZ"`, `"ShiftRight"`, ...).
    ///
    /// Only the Options screen's **Key Config** row consumes it, and only
    /// while it is awaiting a key. Everything else the menu does runs off the
    /// pad word, which is why this is a second entry point rather than a
    /// widened `play_menu_input`: a rebind has to see a key that is *not*
    /// bound to any button yet, and such a key contributes no pad bit at all.
    ///
    /// Returns `true` when the key was latched - i.e. the code is one the
    /// engine's shared vocabulary has a name for
    /// ([`legaia_engine_system::input::key_name_for_dom_code`]) and a menu is
    /// open. A page uses that to decide whether to swallow the event. Codes
    /// outside the vocabulary answer `false` rather than being stored under a
    /// guessed name, which is the same bound the native window's
    /// `keycode_to_name` filter imposes: neither host binds a key the table
    /// cannot spell.
    ///
    /// The page must skip auto-repeat (`KeyboardEvent.repeat`); the native
    /// window collapses its own repeat stream in `handle_key`.
    pub fn play_menu_key(&mut self, code: &str) -> bool {
        let Some(name) = legaia_engine_system::input::key_name_for_dom_code(code) else {
            return false;
        };
        match self.play_menu.as_mut() {
            Some(m) => {
                m.pending_key = Some(name.to_string());
                true
            }
            None => false,
        }
    }

    /// Take the CDNAME scene label an in-canvas card **Load** landed in, if
    /// one is waiting; `""` otherwise. The page polls this after driving the
    /// menu and, when it is a scene it can walk, enters it - retail resumes a
    /// save in the scene it was written in. Consuming clears it.
    pub fn play_menu_take_load_scene(&mut self) -> String {
        self.play_menu
            .as_mut()
            .and_then(|m| m.pending_load_scene.take())
            .unwrap_or_default()
    }
}
