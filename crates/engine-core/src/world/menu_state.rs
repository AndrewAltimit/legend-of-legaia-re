//! Pause-menu runtime state: disc-parsed text / widget tables and the pending warp / escape requests.
//!
//! One subsystem's state, held as a unit on the composite [`World`].
//! Fields keep their retail provenance notes.

/// Pause-menu runtime state: disc-parsed text / widget tables and the pending warp / escape requests.
pub struct MenuState {
    /// Disc-derived pause-menu text (item names + descriptions, spell
    /// names / descriptions, accessory passive lines). `None` on a
    /// PROT.DAT-only load; install via [`crate::world::World::install_menu_text`] when
    /// the executable is reachable. The Items / Magic pause screens read
    /// it through `legaia_engine_world_views::pause_screens`.
    pub text: Option<legaia_engine_menus::pause_screens::MenuTextTables>,
    /// The Items screen's Arrange sort ranks, parsed from the menu
    /// overlay (PROT 0899 VA `0x801E4A88`,
    /// [`legaia_engine_menus::menu_arrange::parse_arrange_rank_table`]). `None` on a
    /// load without the overlay - Arrange then falls back to id order.
    pub arrange_rank: Option<legaia_engine_menus::menu_arrange::ArrangeRankTable>,
    /// Labels for the two entry-context screens the pause menu opens under
    /// kind [`legaia_engine_menus::pause_screens::ROOT_MENU_CONTEXT_LOCKED`] - the notice
    /// panel's lines (window `6`) and the ready check's headings (window
    /// `5`), read out of the same PROT 0899 image
    /// ([`legaia_engine_menus::pause_screens::ContextLockedLabels`]). Empty on a load
    /// without the overlay, and an empty set is what keeps the panels from
    /// drawing invented text.
    pub context_labels: legaia_engine_menus::pause_screens::ContextLockedLabels,
    /// The Door of Light / Incense confirm windows' prompt lines (windows
    /// `10` / `12`), read out of the same PROT 0899 image
    /// ([`legaia_engine_menus::pause_screens::UseRoutePrompts`]). Empty on a
    /// load without the overlay.
    pub use_route_prompts: legaia_engine_menus::pause_screens::UseRoutePrompts,
    /// The Throw Out confirm's labels (window 9), out of the same image
    /// ([`legaia_engine_menus::pause_screens::ThrowConfirmLabels`]).
    pub throw_confirm_labels: legaia_engine_menus::pause_screens::ThrowConfirmLabels,
    /// Window 7's raw sentence (the spell level-up notice), out of the same
    /// image at
    /// [`legaia_engine_menus::pause_screens::SPELL_LEVEL_NOTICE_TEMPLATE_VA`].
    pub spell_level_template: Option<Vec<u8>>,
    /// The shop windows' labels (picker rows, compare notes, quantity
    /// prompts, recipient rows, sell detail, Point Card toast), read out of
    /// the same PROT 0899 image ([`legaia_engine_menus::shop::ShopLabels`]).
    /// Empty on a load without the overlay: the shop then draws no label
    /// text rather than invented text.
    pub shop_labels: legaia_engine_menus::shop::ShopLabels,
    /// Window-widget bytecode programs resolved from the menu-overlay
    /// image (PROT 0899) by [`crate::world::World::install_menu_overlay_tables`] -
    /// the disc source the window-script VM (`legaia_engine_vm::run`,
    /// retail `FUN_801D6628`) interprets. `None` on a load without the
    /// overlay; the shop then opens without window choreography.
    pub widget_scripts: Option<legaia_engine_menus::menu_widget::MenuWidgetScripts>,
    /// The menu overlay's weapon **category / favour** table (PROT 0899 VA
    /// `0x801E4B88`, [`legaia_engine_menus::menu_item_category::parse_category_table`]) -
    /// the data `FUN_801DD0C0` walks and the Best-Equipment chooser scores
    /// its weapon candidates against. Empty on a load without the overlay,
    /// which is exactly the retail routine's empty-table arm (score 0 for
    /// every weapon, so the pick falls back to raw ATK).
    /// Installed by [`crate::world::World::install_menu_overlay_tables`].
    pub item_category: Vec<legaia_engine_menus::menu_item_category::CategoryEntry>,
    /// The quick-travel landmark tables out of `SCUS_942.54`
    /// (`DAT_80073A98` placement records + `DAT_80073B18` names,
    /// [`legaia_game_tables::worldmap_menu`]). Installed by
    /// [`crate::world::World::install_menu_text`]; `None` on a PROT.DAT-only load, and
    /// the pause menu's Door of Wind list is then empty rather than
    /// invented. Also the world-map landmark menu's source.
    pub worldmap_menu: Option<legaia_game_tables::worldmap_menu::WorldmapMenu>,
    /// Destination staged by a committed **Door of Wind** pause-menu use -
    /// retail's `0x80084624` / `0x80084628` / `0x8008462C` triple written
    /// by `FUN_801D8B90` phase 3 right before it hands the outer menu SM
    /// exit code [`legaia_engine_menus::pause_screens::MENU_EXIT_CODE_WORLD_MAP_WARP`].
    /// `None` until a warp commits; the world tick's
    /// [`crate::world::World::drain_staged_menu_warp`] resolves it through
    /// [`crate::world::DiscTables::scene_toc_names`] into the named scene transition the scene
    /// host consumes.
    pub pending_warp: Option<legaia_engine_menus::pause_screens::StagedWarp>,
    /// Set by a committed **Door of Light** pause-menu use - retail's
    /// `_DAT_8007B43C = 4` dungeon-escape handoff (`FUN_801D8A58`). `None`
    /// until an escape commits.
    pub pending_escape: bool,
    /// The pause-menu session's post-menu half while a Door item's travel art
    /// runs ([`crate::world::World::tick_pause_session`]). `None` otherwise.
    pub pause_session: Option<crate::world::pause_session::PauseSession>,
    /// The window list those programs drive
    /// ([`legaia_engine_menus::menu_widget::MenuWidgetState`], the `vm::Host` impl).
    /// Run against it via [`crate::world::World::run_shop_widget_open`] /
    /// [`crate::world::World::run_shop_widget_sell_away`].
    pub widgets: legaia_engine_menus::menu_widget::MenuWidgetState,
    /// The notify window's message template (menu overlay `0x801E4700`,
    /// [`legaia_engine_menus::pause_screens::notify_template_from_menu_overlay`]).
    /// Installed by [`crate::world::World::install_menu_overlay_tables`];
    /// `None` without the overlay, and no art-learned notice is composed then.
    pub notify_template: Option<Vec<u8>>,
    /// A pause-menu item use taught an art: the window-8 notice waiting for
    /// a host to park it on its menu runtime
    /// (`legaia_engine_menu_runtime::field_menu_dispatch::apply_inventory_outcome` fills it).
    pub pending_art_notice: Option<legaia_engine_menus::pause_screens::ArtLearnedNotice>,
}

impl MenuState {
    pub fn new() -> Self {
        Self {
            text: None,
            arrange_rank: None,
            context_labels: Default::default(),
            use_route_prompts: Default::default(),
            throw_confirm_labels: Default::default(),
            spell_level_template: None,
            shop_labels: Default::default(),
            widget_scripts: None,
            item_category: Vec::new(),
            worldmap_menu: None,
            pending_warp: None,
            pending_escape: false,
            pause_session: None,
            widgets: Default::default(),
            notify_template: None,
            pending_art_notice: None,
        }
    }

    /// The display label for item `id`: its SCUS table name, or - on a
    /// load without the executable - the raw id in hex (`Item 2A`), the
    /// form the engine's own pause-screen builders already print.
    ///
    /// The shop's list and panes print this on both hosts. The fallback used
    /// to be spelled per call site and had drifted (`item 42` in the native
    /// shop, `Item 2A` on the page and in the native bag), so one nameless id
    /// read differently in the shop than in the bag, and on the native window
    /// than on the page.
    pub fn item_label(&self, id: u8) -> String {
        self.text
            .as_ref()
            .and_then(|t| t.item_name(id))
            .map(str::to_string)
            .unwrap_or_else(|| format!("Item {id:02X}"))
    }
}

impl Default for MenuState {
    fn default() -> Self {
        Self::new()
    }
}
