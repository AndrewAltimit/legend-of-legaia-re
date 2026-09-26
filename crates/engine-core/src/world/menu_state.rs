//! Pause-menu runtime state: disc-parsed text / widget tables and the pending warp / escape requests.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

/// Pause-menu runtime state: disc-parsed text / widget tables and the pending warp / escape requests.
pub struct MenuState {
    /// Disc-derived pause-menu text (item names + descriptions, spell
    /// names / descriptions, accessory passive lines). `None` on a
    /// PROT.DAT-only load; install via [`crate::world::World::install_menu_text`] when
    /// the executable is reachable. The Items / Magic pause screens read
    /// it through [`crate::pause_screens`].
    pub text: Option<crate::pause_screens::MenuTextTables>,
    /// The Items screen's Arrange sort ranks, parsed from the menu
    /// overlay (PROT 0899 VA `0x801E4A88`,
    /// [`crate::menu_arrange::parse_arrange_rank_table`]). `None` on a
    /// load without the overlay - Arrange then falls back to id order.
    pub arrange_rank: Option<crate::menu_arrange::ArrangeRankTable>,
    /// Labels for the two entry-context screens the pause menu opens under
    /// kind [`crate::pause_screens::ROOT_MENU_CONTEXT_LOCKED`] - the notice
    /// panel's lines (window `6`) and the ready check's headings (window
    /// `5`), read out of the same PROT 0899 image
    /// ([`crate::pause_screens::ContextLockedLabels`]). Empty on a load
    /// without the overlay, and an empty set is what keeps the panels from
    /// drawing invented text.
    pub context_labels: crate::pause_screens::ContextLockedLabels,
    /// Window-widget bytecode programs resolved from the menu-overlay
    /// image (PROT 0899) by [`crate::world::World::install_menu_overlay_tables`] -
    /// the disc source the window-script VM (`legaia_engine_vm::run`,
    /// retail `FUN_801D6628`) interprets. `None` on a load without the
    /// overlay; the shop then opens without window choreography.
    pub widget_scripts: Option<crate::menu_widget::MenuWidgetScripts>,
    /// The menu overlay's weapon **category / favour** table (PROT 0899 VA
    /// `0x801E4B88`, [`crate::menu_item_category::parse_category_table`]) -
    /// the data `FUN_801DD0C0` walks and the Best-Equipment chooser scores
    /// its weapon candidates against. Empty on a load without the overlay,
    /// which is exactly the retail routine's empty-table arm (score 0 for
    /// every weapon, so the pick falls back to raw ATK).
    /// Installed by [`crate::world::World::install_menu_overlay_tables`].
    pub item_category: Vec<crate::menu_item_category::CategoryEntry>,
    /// The quick-travel landmark tables out of `SCUS_942.54`
    /// (`DAT_80073A98` placement records + `DAT_80073B18` names,
    /// [`legaia_asset::worldmap_menu`]). Installed by
    /// [`crate::world::World::install_menu_text`]; `None` on a PROT.DAT-only load, and
    /// the pause menu's Door of Wind list is then empty rather than
    /// invented. Also the world-map landmark menu's source.
    pub worldmap_menu: Option<legaia_asset::worldmap_menu::WorldmapMenu>,
    /// Destination staged by a committed **Door of Wind** pause-menu use -
    /// retail's `0x80084624` / `0x80084628` / `0x8008462C` triple written
    /// by `FUN_801D8B90` phase 3 right before it hands the outer menu SM
    /// exit code [`crate::pause_screens::MENU_EXIT_CODE_WORLD_MAP_WARP`].
    /// `None` until a warp commits; the world tick's
    /// [`crate::world::World::drain_staged_menu_warp`] resolves it through
    /// [`crate::world::DiscTables::scene_toc_names`] into the named scene transition the scene
    /// host consumes.
    pub pending_warp: Option<crate::pause_screens::StagedWarp>,
    /// Set by a committed **Door of Light** pause-menu use - retail's
    /// `_DAT_8007B43C = 4` dungeon-escape handoff (`FUN_801D8A58`). `None`
    /// until an escape commits.
    pub pending_escape: bool,
    /// The window list those programs drive
    /// ([`crate::menu_widget::MenuWidgetState`], the `vm::Host` impl).
    /// Run against it via [`crate::world::World::run_shop_widget_open`] /
    /// [`crate::world::World::run_shop_widget_sell_away`].
    pub widgets: crate::menu_widget::MenuWidgetState,
    /// The notify window's message template (menu overlay `0x801E4700`,
    /// [`crate::pause_screens::notify_template_from_menu_overlay`]).
    /// Installed by [`crate::world::World::install_menu_overlay_tables`];
    /// `None` without the overlay, and no art-learned notice is composed then.
    pub notify_template: Option<Vec<u8>>,
    /// A pause-menu item use taught an art: the window-8 notice waiting for
    /// a host to park it on its menu runtime
    /// (`crate::field_menu_dispatch::apply_inventory_outcome` fills it).
    pub pending_art_notice: Option<crate::pause_screens::ArtLearnedNotice>,
}

impl MenuState {
    pub fn new() -> Self {
        Self {
            text: None,
            arrange_rank: None,
            context_labels: Default::default(),
            widget_scripts: None,
            item_category: Vec::new(),
            worldmap_menu: None,
            pending_warp: None,
            pending_escape: false,
            widgets: Default::default(),
            notify_template: None,
            pending_art_notice: None,
        }
    }
}

impl Default for MenuState {
    fn default() -> Self {
        Self::new()
    }
}
