//! Installing the disc-sourced tables a battle reads: monster catalog, steal
//! table, equipment and accessory tables, menu text and the menu overlay
//! tables.

use super::*;

impl World {
    /// Replace just the monster stat catalog, leaving `formation_table`
    /// untouched. The MAN encounter source carries formation monster-ids but
    /// not stat blocks, so the host installs the stat catalog separately when
    /// the formations come from [`Self::install_man_encounter`].
    pub fn set_monster_catalog(
        &mut self,
        catalog: legaia_engine_battle::monster_catalog::MonsterCatalog,
    ) {
        self.tables.monster_catalog = catalog;
    }

    /// Install the static `SCUS_942.54` per-monster steal table
    /// (`DAT_80077828`). Boot wires this once per host, from the same
    /// executable the item / equipment / spell tables come from.
    ///
    /// Without it [`Self::apply_steal_from_table`] grants nothing and PROT
    /// 0941's Steal cannot resolve a monster-seat victim at all - the leg
    /// retail decides off this table.
    pub fn set_steal_table(&mut self, table: legaia_game_tables::steal_table::StealTable) {
        self.tables.steal_table = Some(table);
    }

    /// Install the raw equipment stat-bonus table (`DAT_80074F68`). The
    /// derived modifier table is a separate install
    /// ([`Self::set_equipment_table`]); this one keeps the record bytes the
    /// menu's list builders index by bonus row.
    pub fn set_equip_stats(&mut self, table: legaia_game_tables::equip_stats::EquipStatTable) {
        self.tables.equip_stats = Some(table);
    }

    /// Install the per-item battle-stat modifier table (weapon / armor /
    /// accessory bonuses). Boot wires this once; [`Self::seed_party_battle_stats`]
    /// folds the equipped items onto each party combatant at battle entry.
    pub fn set_equipment_table(
        &mut self,
        table: legaia_engine_battle::battle_stats::EquipmentTable,
    ) {
        self.tables.equipment_table = table;
    }

    /// Install the accessory ("Goods") passive-effect catalog (item id →
    /// passive index + party-wide scope flags, decoded from the executable -
    /// see [`legaia_engine_battle::accessory_passives::AccessoryPassives::from_disc`]).
    /// Boot wires this once; [`Self::refresh_party_ability_bits`] derives the
    /// per-character ability bitfields from it and
    /// [`Self::seed_party_battle_stats`] applies the percent stat boosts.
    pub fn set_accessory_passives(
        &mut self,
        p: legaia_engine_battle::accessory_passives::AccessoryPassives,
    ) {
        self.tables.accessory_passives = p;
    }

    /// Install the pause-menu text tables (item names/descriptions, spell
    /// names/descriptions, accessory passive lines) from a `SCUS_942.54`
    /// image. Boot wires this once when the executable is reachable; the
    /// Items / Magic pause screens read it through
    /// [`legaia_engine_menus::pause_screens::MenuTextTables`].
    pub fn install_menu_text(&mut self, scus: &[u8]) {
        self.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables::from_scus(scus));
        // The same image carries the quick-travel landmark tables
        // (`DAT_80073A98` placements + `DAT_80073B18` names). Installing
        // them on this one call means both hosts get the pause menu's Door
        // of Wind destination list from the boot step they already make -
        // neither can ship the route with an empty list while the other
        // fills it.
        if let Ok(menu) = legaia_game_tables::worldmap_menu::parse_scus(scus) {
            self.menu.worldmap_menu = Some(menu);
        }
        // The battle draw's Rot limb ranges (`0x80077998`) - the one
        // per-object colour rule of `FUN_80048A08`, read by both hosts'
        // battle actor draws through [`Self::battle_limb_dim_plan`].
        if let Some(t) = legaia_engine_battle_vm::battle_actor_draw::RotLimbTable::from_scus(scus) {
            self.tables.rot_limb_table = Some(t);
        }
    }

    /// Install the menu-overlay data tables (PROT 0899 as-loaded image):
    /// currently the Items screen's Arrange sort ranks
    /// (`DAT_801E4A88`, [`legaia_engine_menus::menu_arrange`]). Best-effort - a short
    /// image leaves the fallback in place. Boot wires this alongside the
    /// window-descriptor parse when the overlay entry is reachable.
    pub fn install_menu_overlay_tables(&mut self, overlay: &[u8]) {
        if let Ok(rank) = legaia_engine_menus::menu_arrange::parse_arrange_rank_table(overlay) {
            self.menu.arrange_rank = Some(rank);
        }
        // The weapon category / favour table `FUN_801DD0C0` walks
        // (`DAT_801E4B88`). Without it the Best-Equipment chooser scores
        // every weapon 0 for category and picks on raw ATK alone - which is
        // retail's empty-table arm, not a fallback the engine invented, but
        // it is also not what a retail disc produces.
        if let Ok(table) = legaia_engine_menus::menu_item_category::parse_category_table(overlay) {
            self.menu.item_category = table;
        }
        // The same image carries the two entry-context screens' label
        // strings (`FUN_801D6360` / `FUN_801D61B0` load them straight out of
        // the overlay's own pool). Installing them here rather than at
        // menu-open means both hosts get them from the one call they already
        // make, so neither can ship the panels text-less while the other
        // draws them.
        let labels =
            legaia_engine_menus::pause_screens::ContextLockedLabels::from_menu_overlay(overlay);
        if labels.is_installed() {
            self.menu.context_labels = labels;
        }
        // The special Use routes' confirm prompts (`FUN_801D1DAC` /
        // `FUN_801D1F10` draw them out of the same pool).
        let prompts =
            legaia_engine_menus::pause_screens::UseRoutePrompts::from_menu_overlay(overlay);
        if prompts.is_installed() {
            self.menu.use_route_prompts = prompts;
        }
        let throw =
            legaia_engine_menus::pause_screens::ThrowConfirmLabels::from_menu_overlay(overlay);
        if throw.is_installed() {
            self.menu.throw_confirm_labels = throw;
        }
        if let Some(t) = legaia_engine_menus::pause_screens::menu_overlay_bytes(
            overlay,
            legaia_engine_menus::pause_screens::SPELL_LEVEL_NOTICE_TEMPLATE_VA,
        )
        .filter(|t| t.first() == Some(&0xC3))
        {
            self.menu.spell_level_template = Some(t);
        }
        // The shop windows' label strings live in the same pool
        // (`FUN_801D4868` .. `FUN_801DCE20` load them by VA).
        let shop = legaia_engine_menus::shop::ShopLabels::from_menu_overlay(overlay);
        if shop.is_installed() {
            self.menu.shop_labels = shop;
        }
        // The notify window's message template (`0x801E4700`) - window 8,
        // the art-learned notice a Hyper-Art book raises.
        if let Some(t) =
            legaia_engine_menus::pause_screens::notify_template_from_menu_overlay(overlay)
        {
            self.menu.notify_template = Some(t);
        }
        // The window-widget bytecode programs the window-script VM
        // (`legaia_engine_vm::run`, retail `FUN_801D6628`) interprets -
        // resident data in this same image (`legaia_game_tables::widget_script`).
        // The window-descriptor table seeds the choreography's home
        // positions (the per-instruction `x`/`y` pair the VM reads off the
        // record).
        if let Ok(table) = legaia_game_tables::menu_windows::parse(overlay) {
            self.menu.widgets.set_defaults_from_table(&table);
        }
        // The casino prize table (file `0x15D00`, four 0x60-byte blocks) -
        // the record base window 44's renderer (`FUN_801D5DE0`) indexes.
        // Feeds `World::try_arm_prize_exchange` (op-0x49 sub-7).
        if let Some(blocks) = legaia_engine_minigames::prize_exchange::parse_blocks(overlay) {
            self.shops.prize_blocks = blocks;
        }
        if let Some(scripts) =
            legaia_engine_menus::menu_widget::MenuWidgetScripts::resolve_from_overlay(overlay)
        {
            self.menu.widget_scripts = Some(scripts);
        }
    }
}
