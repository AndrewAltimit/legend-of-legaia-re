//! Battle-side inputs a scene entry seeds on the host: the battle SFX bank, the monster archive,
//! the tutorial script, the move-power table, the cast effect pool, party battle forms,
//! swing costs, monster action clips and the auto-combo inputs.

use super::*;

impl SceneHost {
    /// Copy the battle runtime SFX bank `bse.dat` (PROT 888) onto the world -
    /// battle init `FUN_800513F0` -> `FUN_8001FA88`, which reloads it into the
    /// current-bundle slot on every battle (so a fresh copy also discards the
    /// last battle's `+4` category writes). Read once per host.
    ///
    /// REF: FUN_8001FA88
    pub(super) fn install_battle_sfx_bank(&mut self) {
        if self.bse_bank_cache.is_none() {
            let bytes = match self.index.entry_bytes(BSE_BANK_PROT_ENTRY) {
                Ok(b) if legaia_container_formats::bse_bank::detect(&b).is_some() => b.to_vec(),
                Ok(_) => {
                    eprintln!("[scene] PROT {BSE_BANK_PROT_ENTRY} is not a bse bank");
                    Vec::new()
                }
                Err(err) => {
                    eprintln!("[scene] bse.dat (PROT {BSE_BANK_PROT_ENTRY}) load skipped: {err:#}");
                    Vec::new()
                }
            };
            self.bse_bank_cache = Some(Arc::new(bytes));
        }
        if let Some(bank) = self.bse_bank_cache.as_ref().filter(|b| !b.is_empty()) {
            let bank = Arc::clone(bank);
            self.world.install_battle_sfx_bank(&bank);
        }
    }

    /// Lazily load + cache the monster stat archive (PROT 867, extended
    /// footprint - the archive lives in the entry's trailing-gap sectors,
    /// not the small indexed payload, so `entry_bytes` would truncate it).
    /// Returns `None` if the entry can't be read.
    pub(super) fn monster_archive_bytes(&mut self) -> Option<Arc<Vec<u8>>> {
        if self.monster_archive_cache.is_none() {
            match self.index.entry_bytes_extended(MONSTER_ARCHIVE_PROT_ENTRY) {
                Ok(b) => self.monster_archive_cache = Some(Arc::new(b)),
                Err(err) => {
                    eprintln!(
                        "[scene] monster archive (PROT {MONSTER_ARCHIVE_PROT_ENTRY}) load skipped: {err:#}"
                    );
                    return None;
                }
            }
        }
        self.monster_archive_cache.clone()
    }

    /// Seat every player cast's summon-body stats from its own `summon.dat`
    /// actor record (extraction PROT 893) onto
    /// [`legaia_engine_core::world::DiscTables::summon_creatures`]: over the archive twin's
    /// stats where one is installed, as a record-only body where none is (the
    /// flutes and the big summons).
    ///
    /// REF: FUN_801F19EC (`FUN_80054CB0(record, 4)` at `0x801F1D1C`)
    pub(super) fn install_summon_cast_records(&mut self) {
        use legaia_actor_formats::summon_readef as sr;
        use legaia_engine_battle::monster_catalog as mc;
        let Ok(summon_dat) = self.index.entry_bytes(u32::from(sr::SUMMON_PROT_INDEX)) else {
            return;
        };
        for spell_id in sr::PLAYER_CAST_IDS {
            let Ok(head) = sr::cast_record_stats(&summon_dat, spell_id) else {
                continue;
            };
            let creatures = &mut self.world.tables.summon_creatures;
            if let Some(def) = creatures.get_mut(&spell_id) {
                mc::apply_cast_record(def, &head);
            } else {
                let name = sr::parse_cast(&summon_dat, spell_id)
                    .ok()
                    .and_then(|c| c.attack_name)
                    .unwrap_or_default();
                creatures.insert(spell_id, mc::monster_def_from_cast_record(name, &head));
            }
        }
    }

    /// Install the sparring-tutorial prompt corpus onto the world from PROT
    /// entry 0967 (the battle-stage slot-B overlay), once per host.
    ///
    /// This is text only - it installs no arming. Whether the tutorial runs is
    /// decided by the disc, at battle entry, from the system-flag arm
    /// ([`legaia_engine_battle::battle_tutorial::TUTORIAL_ARM_FLAG`]). Loading it here rather
    /// than in a host is what keeps the two hosts on one model: neither has to
    /// know the tutorial exists, and a host that forgot to read the corpus
    /// used to be a host that could never show a prompt.
    ///
    /// A read/parse failure leaves an empty corpus, which arms normally and
    /// resolves no box text, so a partial disc still plays the fight.
    pub(super) fn ensure_battle_tutorial_script(&mut self) {
        if self.battle_tutorial_loaded {
            return;
        }
        self.battle_tutorial_loaded = true;
        let script = crate::disc_readers::battle_tutorial::script_from_prot(&self.index);
        if script.is_empty() {
            eprintln!(
                "[scene] sparring-tutorial prompts (PROT {}) unreadable - the fight runs \
                 without its boxes",
                legaia_engine_battle::battle_tutorial::OVERLAY_967_PROT_INDEX
            );
        }
        self.world.set_battle_tutorial_script(script);
    }

    /// Install the name-entry screen's prompts from the field overlay's
    /// pointer table (PROT 0897,
    /// [`legaia_ui_formats::name_entry_prompts`]) once a read succeeds. A
    /// failed read leaves [`legaia_engine_core::world::DiscTables::name_entry_prompts`]
    /// `None`, where the screen draws the port's English.
    pub(super) fn ensure_name_entry_prompts(&mut self) {
        use legaia_ui_formats::name_entry_prompts as nep;
        if self.world.tables.name_entry_prompts.is_some() {
            return;
        }
        if let Ok(bytes) = self.index.entry_bytes(nep::FIELD_OVERLAY_PROT_INDEX as u32) {
            self.world.tables.name_entry_prompts = Some(nep::NameEntryPrompts::from_field_overlay(
                &bytes,
                nep::FIELD_OVERLAY_BASE_VA,
            ));
        }
    }

    /// Install the battle-action move-power table onto the world from PROT
    /// entry 0898 (the battle-action overlay), once per host. The monster
    /// special-attack damage path reads it to roll faithful per-move damage;
    /// a read/parse failure leaves [`legaia_engine_core::world::DiscTables::move_power`] `None`
    /// (the placeholder damage path stays active) and is not retried.
    pub(super) fn ensure_move_power_table(&mut self) {
        if self.move_power_loaded {
            return;
        }
        self.move_power_loaded = true;
        let entry = legaia_engine_battle::move_power::BATTLE_ACTION_OVERLAY_PROT_ENTRY;
        match self.index.entry_bytes(entry) {
            Ok(bytes) => {
                if let Some(cat) =
                    legaia_engine_battle::move_power::MovePowerCatalog::from_overlay_0898(&bytes)
                {
                    self.world.tables.move_power = Some(cat);
                    // Retain the overlay so the move-FX render path can read the
                    // 0x801f6324 prototype records' move-VM bytecode.
                    self.world.tables.move_power_overlay =
                        Some(std::sync::Arc::from(bytes.as_slice()));
                } else {
                    eprintln!(
                        "[scene] move-power table (PROT {entry}) parse failed - placeholder damage stays active"
                    );
                }
                // The element-affinity matrix + per-character element table are
                // sibling static data in the same overlay, so parse them from the
                // same bytes. A failure leaves the neutral 100% multiplier active.
                if let Some(aff) = legaia_game_tables::element_affinity::parse(&bytes) {
                    self.world.tables.element_affinity = Some(aff);
                } else {
                    eprintln!(
                        "[scene] element-affinity tables (PROT {entry}) parse failed - neutral affinity stays active"
                    );
                }
                // The battle camera's per-character height table (0x801F4D2C)
                // is sibling static data in the same overlay. A failure leaves
                // the camera on its single traced fallback height.
                if let Some(heights) = legaia_game_tables::battle_camera_table::parse(&bytes) {
                    self.world.tables.battle_camera_heights = Some(heights);
                } else {
                    eprintln!(
                        "[scene] battle-camera height table (PROT {entry}) parse failed - fallback height stays active"
                    );
                }
                // The Seru-absorb banner's caption pieces (0x801F4DFC /
                // 0x801F4C28) are sibling rodata in the same overlay.
                self.world.tables.absorb_caption =
                    legaia_game_tables::absorb_caption::parse(&bytes);
                // The loss window's text (0x801F4C78 / 0x801F4C94) is sibling
                // rodata in the same overlay.
                self.world.tables.defeat_text =
                    legaia_engine_battle_vm::battle_party_panel::DefeatText::parse(&bytes);
                // The victory report's pieces (0x801F4C2C / 0x801F4C38) sit
                // beside them.
                self.world.tables.victory_text =
                    legaia_engine_battle_vm::battle_party_panel::VictoryText::parse(&bytes);
                // The Seru side-effect table (0x801F6870) is sibling static
                // data in the same overlay. A failure leaves the stager off,
                // so levelled Seru casts carry no secondary debuff.
                if let Some(side) =
                    legaia_game_tables::seru_side_effect::SeruSideEffectTable::parse(&bytes)
                {
                    self.world.tables.seru_side_effects = Some(side);
                } else {
                    eprintln!(
                        "[scene] Seru side-effect table (PROT {entry}) parse failed - levelled casts stage no debuff"
                    );
                }
            }
            Err(err) => {
                eprintln!("[scene] battle-action overlay (PROT {entry}) load skipped: {err:#}");
            }
        }
    }

    /// Install the **cast-effect pool** onto the world - the DATA half of the
    /// slot-B cast-module band (PROT 0903..0966), once per host.
    ///
    /// Retail streams one of these 64 entries per cast, at the cast; the port
    /// reads all 64 once instead, because the band is small static data and the
    /// alternative is a disc read inside a battle frame. A cast then resolves
    /// its module through PROT 0898's own dispatch arithmetic
    /// (`legaia_engine_battle_vm::battle_cast_dispatch`) and stages that module's
    /// spawn records ([`legaia_engine_core::world::World::spawn_cast_module_fx`]).
    ///
    /// Entries that fail to read are skipped rather than aborting the install -
    /// a cast whose module is missing simply stages no records, which is the
    /// disc-free behaviour. `see docs/subsystems/cast-module.md`.
    pub(super) fn ensure_cast_effect_pool(&mut self) {
        use legaia_overlay_images::cast_effect_pool::{
            CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST, CastEffectPool,
        };
        if self.cast_effect_pool_loaded {
            return;
        }
        self.cast_effect_pool_loaded = true;
        let mut pool = CastEffectPool::new();
        for entry in CAST_MODULE_PROT_FIRST..=CAST_MODULE_PROT_LAST {
            if let Ok(bytes) = self.index.entry_bytes(entry) {
                pool.insert(entry, &bytes);
            }
        }
        if pool.is_empty() {
            eprintln!(
                "[scene] cast-effect pool (PROT {CAST_MODULE_PROT_FIRST}..{CAST_MODULE_PROT_LAST}) \
                 unreadable - casts stage no module records"
            );
            return;
        }
        self.world
            .install_cast_effect_pool(std::sync::Arc::new(pool));
        // The attack names the summon modules' caption arms print: each
        // player cast's `summon.dat` actor record `rec[0]` string.
        if let Ok(summon_dat) = self.index.entry_bytes(u32::from(
            legaia_actor_formats::summon_readef::SUMMON_PROT_INDEX,
        )) {
            let names: std::collections::BTreeMap<u8, String> =
                legaia_actor_formats::summon_readef::PLAYER_CAST_IDS
                    .filter_map(|id| {
                        let cast = legaia_actor_formats::summon_readef::parse_cast(&summon_dat, id)
                            .ok()?;
                        Some((id, cast.attack_name?))
                    })
                    .collect();
            self.world.tables.summon_attack_names = std::sync::Arc::new(names);
            self.world.tables.summon_dat = Some(std::sync::Arc::from(summon_dat.as_slice()));
        }
    }

    /// Re-read the party's per-equipment battle inputs - the swing costs and
    /// the Auto attack's pool-arm inputs - from the player battle files.
    ///
    /// Scene entry runs this, which is when retail selects the equipment
    /// sections. A card load whose save is applied **after** the scene entry
    /// (both hosts' resume order: enter, then hydrate) calls it once more,
    /// so the loaded equipment is what prices the arts input - retail's
    /// card load hydrates first and then loads the scene, so the sections it
    /// selects are the save's own.
    pub fn refresh_party_battle_inputs(&mut self) {
        self.refresh_battle_swing_costs();
        self.refresh_battle_auto_combo_inputs();
    }

    /// Refresh [`legaia_engine_core::world::BattleState::swing_costs`] from the player
    /// battle files: for each roster character (Vahn / Noa / Gala = PROT
    /// 863 / 864 / 865), splice that character's *equipped* sections and
    /// read each swing record's `+0x74` AP cost
    /// (`legaia_actor_formats::battle_char_assembly::swing_command_costs`, slots
    /// `0xC..=0xF` = Left / Right / Down / Up). A slot whose section can't
    /// be resolved keeps the favored base `0x1E` - the disc-free
    /// behaviour.
    ///
    /// Runs at scene entry, since equipment can only change in the field -
    /// and that is when retail does it too: the player files stream, and
    /// the equipment sections are re-selected, on a scene load (world map
    /// or field entry), never on a battle start; a battle splices the
    /// sections already resident (read-watchpoint on the equipped-item
    /// bytes: the case-4 compare of `FUN_80052770` reads them across the
    /// scene load, the battle only touches them from the splice). The
    /// Muscle Dome reads the same bytes through the same function, which
    /// is what keeps the two input screens pricing a swing identically.
    ///
    /// REF: FUN_800557B8
    /// REF: FUN_80052770
    pub(super) fn refresh_battle_swing_costs(&mut self) {
        /// PROT entries of the three weapon-carrying player battle files.
        const PLAYER_FILE_PROT: [u32; 3] = [863, 864, 865];
        for (slot, &prot) in PLAYER_FILE_PROT.iter().enumerate() {
            let Some(record) = self.world.party.roster.members.get(slot) else {
                continue;
            };
            // The assembler keys sections off the first five equipment
            // bytes (`+0x196..+0x19B`); the record carries eight.
            let s = record.equipment().slots;
            let equipped = [s[0], s[1], s[2], s[3], s[4]];
            let Ok(bytes) = self.index.entry_bytes(prot) else {
                continue;
            };
            let Some(pack) = legaia_actor_formats::battle_data_pack::detect(&bytes) else {
                continue;
            };
            let Ok(costs) = legaia_actor_formats::battle_char_assembly::swing_command_costs(
                &bytes, &pack, &equipped,
            ) else {
                continue;
            };
            for (i, c) in costs.iter().enumerate() {
                if let Some(c) = c {
                    self.world.battle.swing_costs[slot][i] = *c as u16;
                }
            }
        }
    }

    /// Install every seated monster's archive-order action clips - the
    /// `+0x4C` entry table the battle action SM's tag lookups
    /// (`FUN_80050E2C`, `World::battle_monster_action_tags`) and the reaction
    /// family read - the tick a battle is up, for any monster that has none.
    ///
    /// Retail's battle loader stages them with the monster at battle load,
    /// independent of anything drawn. Each host used to install them from its
    /// own render build - the native window only after its GPU upload, past
    /// the first multi-tick loop - so a monster without a mesh, or a battle's
    /// first ticks, read no tag table. Positional, holes kept: a monster's
    /// staged anim ids are these indices.
    ///
    /// REF: FUN_80050E2C
    pub(super) fn install_battle_monster_action_clips(&mut self) {
        let pending: Vec<(usize, u16)> = self
            .world
            .actors
            .iter()
            .enumerate()
            .filter(|(_, a)| a.battle_action_clips.is_none())
            .filter_map(|(i, a)| a.battle_monster_id.map(|id| (i, id)))
            .collect();
        // Each monster's clip morphs, the table its entries' `+0x04` /
        // `+0x08` lane indices address (`FUN_800542C8` resolves them at load).
        let morph_pending: Vec<(usize, u16)> = self
            .world
            .actors
            .iter()
            .enumerate()
            .filter(|(_, a)| a.battle_clip_morphs.is_none())
            .filter_map(|(i, a)| a.battle_monster_id.map(|id| (i, id)))
            .collect();
        if pending.is_empty() && morph_pending.is_empty() {
            return;
        }
        let Some(archive) = self.monster_archive_bytes() else {
            return;
        };
        for (slot, id) in morph_pending {
            if let Ok(Some(table)) =
                legaia_actor_formats::monster_archive::clip_morph_table(&archive, id)
            {
                self.world
                    .set_actor_battle_clip_morphs(slot, Arc::new(table));
            }
        }
        for (slot, id) in pending {
            if let Ok(Some(clips)) =
                legaia_actor_formats::monster_archive::animations_by_entry(&archive, id)
                && clips.iter().any(Option::is_some)
            {
                self.world
                    .set_actor_battle_action_clips(slot, Arc::new(clips));
            }
            // ...and its idle loop, the clip the loader seats the monster
            // on. The play hosts installed it from their render build
            // (`World::install_monster_battle_form`), so a headless session's
            // monster stood clip-less until the SM first posed it: its first
            // walk began from another cursor, came into range a tick away
            // from the window's, and the two runs of one fight diverged
            // (`battle_gaza2_park_0x19_target_vahn`'s capture child never
            // reached the phase its headless replay did).
            let unposed = self
                .world
                .actors
                .get(slot)
                .is_some_and(|a| a.battle_animation.is_none());
            if unposed
                && let Ok(Some(idle)) =
                    legaia_actor_formats::monster_archive::idle_animation(&archive, id)
                && let Some(player) =
                    legaia_engine_battle::battle_anim::MonsterAnimPlayer::new(&idle)
            {
                self.world.set_actor_battle_animation(slot, player);
            }
        }
    }

    /// Build and install the party's battle forms for the fight in progress,
    /// once per [`legaia_engine_core::world::BattleState::entry_serial`]: each member's
    /// idle and action clips (the swings the hit events are paced by), art
    /// bank and art records (what the arts input tokenizes) - see
    /// [`legaia_engine_core::battle_party_form::install_party_battle_forms`].
    ///
    /// Retail's battle loader assembles the party with the fight, whether or
    /// not anything draws it. [`Self::tick`] runs this the tick a battle is
    /// up, so every session gets it - the play hosts and headless drivers
    /// alike; a host that enters a battle outside a tick (a debug
    /// `World::enter_battle`) calls it before it builds its render. A no-op
    /// outside battle and on a repeat call within the same fight.
    ///
    /// REF: FUN_800513F0
    pub fn ensure_battle_party_forms(&mut self) {
        if !matches!(self.world.mode, legaia_engine_mode::mode::SceneMode::Battle) {
            return;
        }
        let serial = self.world.battle.entry_serial;
        if self
            .battle_party_forms
            .as_ref()
            .is_some_and(|f| f.battle_serial == serial)
        {
            return;
        }
        let forms = legaia_engine_core::battle_party_form::install_party_battle_forms(
            &self.index,
            &mut self.world,
        );
        self.battle_party_forms = Some(forms);
    }

    /// The fight in progress's party battle forms, for a renderer: meshes,
    /// rest poses, face tracks and the VRAM writes to replay into its battle
    /// VRAM. `None` outside battle or before
    /// [`Self::ensure_battle_party_forms`] ran for this fight.
    pub fn battle_party_forms(
        &self,
    ) -> Option<&legaia_engine_battle::battle_party_form::BattlePartyForms> {
        self.battle_party_forms
            .as_ref()
            .filter(|f| f.battle_serial == self.world.battle.entry_serial)
    }

    /// Refresh the **Auto** attack's disc inputs
    /// ([`legaia_engine_core::world::AutoComboState`]): per roster character, the four
    /// direction commands' leading entry bytes (the pool arm's weight input)
    /// and the raw art-animation bank records (the insertion tail's walk),
    /// plus the four status-guard masks in the battle overlay
    /// (`0x801F672C`, PROT 0898). Same files and same moment as
    /// [`Self::refresh_battle_swing_costs`]; anything that fails to resolve
    /// is left as it was, which a disc-free host reads as "default weights,
    /// no arts, no guards".
    ///
    /// REF: FUN_801F0450
    pub(super) fn refresh_battle_auto_combo_inputs(&mut self) {
        use legaia_actor_formats::battle_char_assembly as bca;
        const PLAYER_FILE_PROT: [u32; 3] = [863, 864, 865];
        const GUARD_TABLE_VA: u32 = 0x801F_672C;
        for (slot, &prot) in PLAYER_FILE_PROT.iter().enumerate() {
            let Some(record) = self.world.party.roster.members.get(slot) else {
                continue;
            };
            let s = record.equipment().slots;
            let equipped = [s[0], s[1], s[2], s[3], s[4]];
            let Ok(bytes) = self.index.entry_bytes_extended(prot) else {
                continue;
            };
            let inputs = &mut self.world.battle.auto_combo.inputs[slot];
            if let Some(pack) = legaia_actor_formats::battle_data_pack::detect(&bytes)
                && let Ok(swings) = bca::swing_battle_animations(&bytes, &pack, &equipped)
            {
                let mut heads = [[0u8; 4]; 4];
                for sw in &swings {
                    if let Some(i) = sw.slot.checked_sub(bca::SWING_SLOT_BASE)
                        && let Some(h) = heads.get_mut(usize::from(i))
                    {
                        for (d, b) in h.iter_mut().zip(sw.anim.effect_script.iter()) {
                            *d = *b;
                        }
                    }
                }
                inputs.command_heads = Some(heads);
            }
            if let Ok(record0) = bca::decode_record0(&bytes)
                && let Some(bank_off) = record0
                    .get(0x58..0x5C)
                    .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) as usize)
                && let Some(count) = record0.get(bank_off).copied()
            {
                let first = bank_off + 4;
                inputs.art_records = (0..usize::from(count))
                    .filter_map(|i| {
                        let at = first + i * bca::ART_RECORD_STRIDE;
                        record0
                            .get(at..at + bca::ART_RECORD_STRIDE)
                            .and_then(|r| <[u8; 0xD0]>::try_from(r).ok())
                    })
                    .collect();
                inputs.art_count = count;
            }
        }
        if let Some(rec) =
            legaia_overlay_images::static_overlay::overlay_map().by_label("battle_action")
            && let Ok(bytes) = self.index.entry_bytes(rec.prot_index)
            && let Ok(image) = legaia_overlay_images::static_overlay::as_loaded(&bytes, rec)
            && let Some(off) = GUARD_TABLE_VA.checked_sub(rec.base_va)
            && let Some(raw) = image.get(off as usize..off as usize + 8)
        {
            let mut guards = [0u16; 4];
            for (i, g) in guards.iter_mut().enumerate() {
                *g = u16::from_le_bytes([raw[i * 2], raw[i * 2 + 1]]);
            }
            self.world.battle.auto_combo.guards = Some(guards);
        }
    }
}
