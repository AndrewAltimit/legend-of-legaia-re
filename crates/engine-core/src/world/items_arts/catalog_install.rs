//! Installing the item, item-effect, spell and art-record catalogs.

use super::*;

impl World {
    /// Set the item catalog the battle / field menu consults for item
    /// actions. Replaces any prior catalog. Engines populate this at
    /// boot time (typically from the vanilla catalog).
    ///
    /// When a real on-disc item-effect table has been installed via
    /// [`Self::set_item_effects`], its field/battle usability flags are applied
    /// onto the new catalog so the item-menu gating matches retail.
    pub fn set_item_catalog(&mut self, catalog: legaia_engine_menus::items::ItemCatalog) {
        let mut catalog = catalog;
        if let Some(table) = &self.tables.item_effects {
            catalog.apply_effect_flags(table);
            catalog.apply_stat_items(table);
            catalog.apply_buff_items(table);
            catalog.apply_action_gauge_items(table);
            catalog.apply_arts_book_items(table);
            catalog.apply_point_card_items(table);
        }
        self.tables.item_catalog = catalog;
    }

    /// Install the real on-disc item-effect descriptor table. Subsequent
    /// [`Self::set_item_catalog`] calls apply its usability flags; this also
    /// re-applies them to the catalog already installed.
    pub fn set_item_effects(&mut self, table: legaia_game_tables::item_effect::ItemEffectTable) {
        self.tables.item_catalog.apply_effect_flags(&table);
        self.tables.item_catalog.apply_stat_items(&table);
        self.tables.item_catalog.apply_buff_items(&table);
        self.tables.item_catalog.apply_action_gauge_items(&table);
        self.tables.item_catalog.apply_arts_book_items(&table);
        self.tables.item_catalog.apply_point_card_items(&table);
        self.tables.item_effects = Some(table);
    }

    /// Install the spell catalog used by the player-driven battle Magic
    /// submenu. Engines call this at battle init (commonly
    /// [`legaia_engine_battle::spells::SpellCatalog::vanilla`]).
    pub fn set_spell_catalog(&mut self, catalog: legaia_engine_battle::spells::SpellCatalog) {
        self.tables.spell_catalog = catalog;
    }

    /// Stage one decoded art record for the player-driven battle Arts submenu,
    /// keyed by `(character, art constant)`. Engines call this at battle init
    /// for every art the party can run (parsed from disc PROT entry `0x05C4`)
    /// so a saved chain ending in that art deals its real per-strike power.
    pub fn set_art_record(
        &mut self,
        character: legaia_art::Character,
        action: legaia_art::ActionConstant,
        record: legaia_art::ArtRecord,
    ) {
        self.tables.art_records.insert((character, action), record);
    }

    /// Install a character's art records **from its art-animation bank** -
    /// the per-character player file's `record[0] +0x58` records
    /// ([`legaia_actor_formats::battle_char_assembly::art_animation_bank`]), which
    /// are the records the retail queue-builder walks: record `k` is action
    /// constant `0x10 + k`, its `+0x00` combo is the arrow string the
    /// tokenizer matches (`FUN_801EED1C` `0x801EF3BC..0x801EF3EC`) and its
    /// embedded entry carries the power run the hit-event driver resolves
    /// from. Only the named art records (`0x1B+`) are installed; the base
    /// ids below carry no combo. The side data retail's kernel never reads
    /// (status effect, per-hit cues) stays at its defaults - a host with a
    /// richer source can overwrite through [`World::set_art_record`].
    ///
    /// Both hosts call this at battle entry, next to the bank's clip
    /// install, so the live arts input tokenizes real arts.
    pub fn install_art_bank_records(
        &mut self,
        character: legaia_art::Character,
        records: &[legaia_actor_formats::battle_char_assembly::ArtAnimRecord],
    ) {
        use legaia_art::{ActionConstant, Command};
        for rec in records {
            let Some(action) = ActionConstant::from_byte(rec.anim_id).filter(|a| a.is_art()) else {
                continue;
            };
            let commands: Vec<Command> = rec
                .combo
                .iter()
                .filter_map(|&b| Command::from_byte(b))
                .collect();
            let power = if rec.entry_tag == 0 {
                Vec::new()
            } else {
                vec![legaia_art::power::PowerByte::from_byte(rec.entry_tag)]
            };
            let record = legaia_art::ArtRecord {
                action,
                commands,
                anim_index: rec.index as u8,
                anim_extra: Vec::new(),
                name: (!rec.name.is_empty()).then(|| rec.name.clone()),
                power,
                dmg_timing: Vec::new(),
                effect_cues: Default::default(),
                hit_cues: Vec::new(),
                identifier: 0,
                anim_speed: rec.rate,
                enemy_effect: legaia_art::EnemyEffect::None,
                repeat_frames: Default::default(),
                background: 0,
                runtime_address: None,
            };
            self.tables.art_records.insert((character, action), record);
        }
    }

    /// Bulk-install art records (see [`World::set_art_record`]). Existing
    /// entries for the same key are replaced.
    pub fn set_art_records(
        &mut self,
        records: impl IntoIterator<
            Item = (
                (legaia_art::Character, legaia_art::ActionConstant),
                legaia_art::ArtRecord,
            ),
        >,
    ) {
        self.tables.art_records.extend(records);
    }
}
