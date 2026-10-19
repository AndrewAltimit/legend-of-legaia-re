//! The environment the hub actors tick against: equip restrictions, pad words, the per-frame
//! `HubEnv`, and the soft-reset / action effects applied back to the `World`.

use super::*;

impl World {
    /// Install the two equipment-table bytes the sub-panel's **candidate** arm
    /// needs and the world's own tables do not carry: the `+6` character mask
    /// and the `+7` slot byte of `DAT_80074F68`.
    ///
    /// Everything else the panel reads is already world state - the character
    /// records ([`legaia_engine_menus::party_state::PartyState::roster`]), the item property table's `+0` / `+1` bytes
    /// ([`crate::world::DiscTables::item_effects`]) and the five stat bonuses
    /// ([`crate::world::DiscTables::equipment_table`]). These two bytes are dropped by the
    /// modifier-only view a boot installs on the world, and survive only on the
    /// [`DiscEquipInfo`] a host builds for its menu runtime - hence the
    /// hand-in. Until one arrives the panel stays on retail's no-candidate arm
    /// rather than answering "can this character equip that" from a zero mask,
    /// which would paint the reject line over every entry.
    pub fn install_hub_equip_restrictions(&mut self, info: &DiscEquipInfo) {
        self.field_vm.submode_screen.equip_restrictions = (0..=u8::MAX)
            .filter_map(|id| info.entry(id).map(|e| (id, (e.mask, disc_slot_bits(e)))))
            .collect();
    }

    /// Set the menu class word `_DAT_8007BB9C` the sub-panel's candidate ladder
    /// reads (`0x1000` / `0x6000` / `0x9000` bag slot, `0x3000` the cursor as
    /// an item id, `0x4000` compare against an empty loadout, anything else no
    /// candidate at all).
    ///
    /// Honored only once [`World::install_hub_equip_restrictions`] has run -
    /// see that method for why.
    pub fn set_hub_equip_mode(&mut self, mode: u32) {
        self.field_vm.submode_screen.equip_mode = mode;
    }

    /// Project the world's equipment state onto the globals `FUN_801E5B4C`
    /// reads: the per-entry character records plus the two static resolver
    /// tables it walks them through.
    ///
    /// Retail addresses all of this directly - the save block at `0x80084140`,
    /// the item property table `DAT_80074368` and the equipment table
    /// `DAT_80074F68`. Each has a world-resident counterpart:
    ///
    /// | Retail read | World source |
    /// |---|---|
    /// | `char[+0x75E..]`, the five equip slots | [`legaia_engine_menus::party_state::PartyState::roster`] |
    /// | `char[+0x6DA/+0x6DC/+0x6DE]`, ATK / UDF / LDF | the same record's `+0x112` / `+0x114` / `+0x116` |
    /// | `DAT_80074368[id*0xC + 0/+1]`, kind + stat index | [`crate::world::DiscTables::item_effects`] |
    /// | `DAT_80074F68[row][+0..+4]`, the five bonuses | [`crate::world::DiscTables::equipment_table`], re-keyed row-wise |
    /// | `0x80084140 + 0x1818`, the bag id list | [`legaia_engine_menus::party_state::PartyState::inventory`] |
    /// | `0x8007B42C`, the weapon-slot table | [`RETAIL_WEAPON_SLOTS`] |
    ///
    /// The bonus rows arrive **keyed by item id** (that is the shape a battle
    /// aggregator wants) and the panel indexes them by the stat-table row an
    /// item's `+1` byte names, so they are re-keyed here through the same `+1`
    /// byte. Rows no equippable id reaches stay zero, which is what the disc
    /// holds for the one such row the panel can actually reach: item id `0`
    /// (the empty-slot sentinel) names row `0x6A`, and that row is eight zero
    /// bytes - so an empty slot contributes nothing without the port
    /// special-casing id `0`, exactly as retail does not.
    fn submode_equip_env(&self) -> HubEquipEnv {
        let mut env = HubEquipEnv {
            weapon_slots: RETAIL_WEAPON_SLOTS.to_vec(),
            ..HubEquipEnv::default()
        };
        // One record per distinct entry code. The code is a character index
        // (retail multiplies it by `0x414` against the save block), and the
        // engine's roster is that same index space.
        for &code in &self.party.active_party {
            if env.records.iter().any(|r| r.code == code) {
                continue;
            }
            let Some(rec) = self.party.roster.members.get(usize::from(code)) else {
                continue;
            };
            let live = rec.live_stats();
            env.records.push(HubEquipRecord {
                code,
                slots: hub_panel_slots(&rec.equipment().slots, usize::from(code)),
                base_stats: [live.atk as i32, live.udf as i32, live.ldf as i32],
            });
        }

        // The bag list the three inventory modes index with the shared cursor.
        // Retail's is the item window's own slot order; the engine's bag is a
        // map, so this is the id order `World::save_party` also writes.
        let mut bag: Vec<u8> = self
            .party
            .inventory
            .iter()
            .filter(|&(_, &count)| count > 0)
            .map(|(&id, _)| id)
            .collect();
        bag.sort_unstable();
        env.inventory = bag;

        let Some(items) = self.tables.item_effects.as_ref() else {
            // No item property table: the aggregation has nothing to resolve
            // an equipped id through, so every row prints its base stat. The
            // candidate ladder stays off with it.
            return env;
        };
        env.item_props = (0..=u8::MAX)
            .map(|id| ItemProps {
                kind: items.kind(id),
                stat_index: items.subtype(id),
            })
            .collect();
        for id in 0..=u8::MAX {
            if items.kind(id) != legaia_game_tables::equip_stats::KIND_EQUIPMENT {
                continue;
            }
            let Some(m) = self.tables.equipment_table.get(id) else {
                continue;
            };
            let row = usize::from(items.subtype(id));
            if env.equip_props.len() <= row {
                env.equip_props.resize(row + 1, EquipProps::default());
            }
            let props = &mut env.equip_props[row];
            // Record order is `[INT, ATK, UDF, LDF, SPD]` - the `+0` byte is
            // the INT bonus and lands last in retail's accumulator order, which
            // is why the modifier view names its fields rather than indexing.
            props.bonuses = [
                m.int as u8,
                m.atk as u8,
                m.udf as u8,
                m.ldf as u8,
                m.spd as u8,
            ];
            // Several ids can name the same row; both halves of what lands here
            // are that row's own bytes, so which id wrote it does not matter.
            if let Some(&(mask, slot_bits)) =
                self.field_vm.submode_screen.equip_restrictions.get(&id)
            {
                props.char_mask = mask;
                props.slot_bits = slot_bits;
            }
        }
        if !self.field_vm.submode_screen.equip_restrictions.is_empty() {
            env.mode = self.field_vm.submode_screen.equip_mode;
        }
        env
    }

    /// Project the world's own state onto the globals the family reads.
    /// Union this vsync's pad edges into the submode latch.
    ///
    /// Called once per `World::tick`, which is once per vsync; the dispatcher
    /// that reads it runs once per game tick. See
    /// [`SubmodeScreen::pad_edge_latch`] for why the two rates have to be
    /// bridged rather than sampled independently.
    pub fn latch_submode_pad_edge(&mut self) {
        // Only while a screen is up: a latch that accumulated across a whole
        // scene would hand the next screen a press from minutes ago.
        if !self.field_vm.submode_screen.open {
            self.field_vm.submode_screen.pad_edge_latch = 0;
            return;
        }
        let (pad, prev) = self.submode_pad_words();
        self.field_vm.submode_screen.pad_edge_latch |= pad & !prev;
    }

    /// The current and previous pad words in the **packed** Legaia layout
    /// every mask this family tests is written in ([`SUBMODE_ACCEPT_MASK`],
    /// [`SUBMODE_BACK_MASK`], the hub's `PAD_CURSOR_*`).
    ///
    /// `World::input` holds the host's **raw** PSX word (`PadButton`: Cross
    /// `0x4000`, Down `0x0040`), while retail's dispatcher reads the packed
    /// words `FUN_8001822C` builds (Cross `0x0040`, Down `0x4000`). Testing the
    /// raw word against the packed masks cross-wired every submode screen:
    /// Cross did nothing, Down accepted, Right backed out and the face
    /// buttons stepped the cursor. The conversion is
    /// [`legaia_engine_system::pad::retail_packed`], the same one the Incense notice
    /// and both hosts' dev menus use.
    pub(super) fn submode_pad_words(&self) -> (u32, u32) {
        (
            u32::from(legaia_engine_system::pad::retail_packed(self.input.pad())),
            u32::from(legaia_engine_system::pad::retail_packed(
                self.input.pad_prev(),
            )),
        )
    }

    pub(super) fn submode_env(&self, frame_delta: u8) -> HubEnv {
        let (pad, prev) = self.submode_pad_words();
        let edge = (pad & !prev) | self.field_vm.submode_screen.pad_edge_latch;
        HubEnv {
            // `DAT_801F2734` is the submode context's state word, which
            // `open_submode` seeds and `World::field_vm.submode_context` mirrors.
            submode: self.field_vm.submode_context.first().copied().unwrap_or(0) as i32,
            pad_edge: edge,
            pad_held: pad,
            pad_repeat: edge,
            confirm_mask: SUBMODE_ACCEPT_MASK | SUBMODE_BACK_MASK,
            cancel_mask: SUBMODE_BACK_MASK,
            accept_mask: SUBMODE_ACCEPT_MASK,
            back_mask: SUBMODE_BACK_MASK,
            frame_delta: frame_delta.max(1) as i32,
            picker_result: self.field_vm.submode_screen.picker_result,
            cursor_row: self.field_vm.submode_screen.counter.cursor,
            cursor_row_alt: self.field_vm.submode_screen.counter.yes_no,
            // Retail reads the op-0x49 operand pointer here; the engine has no
            // RAM pointer, so it carries the "a screen is armed" truth value
            // the dispatcher's release arm actually branches on.
            board_flag: i32::from(self.field_vm.submode_screen.open),
            board_entries: self.field_vm.submode_screen.board_entries,
            // `DAT_80084594` / `DAT_80084598..` are the present-party roster;
            // the engine's mirror of retail's `0x8007BD10` list is
            // `World::party.active_party`.
            entry_count: self.party.active_party.len().min(u8::MAX as usize) as u8,
            entry_codes: self.party.active_party.clone(),
            gold: self.party.money,
            coin_bank: self.minigames.casino_coins.min(i32::MAX as u32) as i32,
            // Everything the entry list's per-entry sub-draw `FUN_801E5B4C`
            // reads out of RAM - see [`World::submode_equip_env`].
            equip: self.submode_equip_env(),
            ..HubEnv::default()
        }
    }

    /// Apply what one tick of the soft reset (slot [`SOFT_RESET_SLOT`]) asked
    /// for.
    ///
    /// - `DrawRecords` positions the records screen
    ///   ([`legaia_engine_scene_host::scene_host::SceneHost::soft_reset_records_pen`]).
    /// - `WhiteFade` is `FUN_801D58F0(2, 0, 0xFFFFFF, 0, 0x78, -1)`
    ///   (`0x801EDFC8..0x801EDFE8`): a kind-2 fade from black to white over
    ///   `0x78` frames, held.
    /// - `ReloadExecutable` is `FUN_80017714` reloading the boot executable
    ///   (`0x801EE04C`). The port has no image to reload; the destination is
    ///   the title screen, which the hosts reach through the same title
    ///   hand-off field-VM op `4C EA` raises ([`World::game_over`]). Retail's
    ///   reboot also replays the boot logos on the way; the port does not.
    pub(super) fn apply_soft_reset_effects(
        &mut self,
        fx: &[legaia_engine_field::world_map_panel_actors::SoftResetEffect],
    ) {
        use legaia_engine_field::world_map_panel_actors::SoftResetEffect as Fx;
        for e in fx {
            match *e {
                Fx::DrawRecords { y } => {
                    self.field_vm.submode_screen.soft_reset.records_y = Some(y)
                }
                Fx::WhiteFade { frames } => {
                    let template = legaia_engine_system::fade::FadeTemplate {
                        kind: 2,
                        duration: frames as i16,
                        start_rgb: [0, 0, 0],
                        end_rgb: [0xFF, 0xFF, 0xFF],
                        mode: [0, -1, 0],
                    };
                    legaia_engine_system::fade::spawn_fade(
                        &mut self.presentation.fade,
                        &template,
                        0,
                    );
                }
                Fx::ReloadExecutable => {
                    if !self.field_vm.submode_screen.soft_reset.reloaded {
                        self.field_vm.submode_screen.soft_reset.reloaded = true;
                        self.game_over = true;
                    }
                }
                Fx::ArmReset | Fx::TickTextActors => {}
            }
        }
    }

    /// Where the soft reset's records screen draws this frame, in retail
    /// 320x240 stage pixels: `(0x20, slide)`. `None` unless slot
    /// [`SOFT_RESET_SLOT`] is up and has drawn.
    pub fn soft_reset_records_pen(&self) -> Option<(i32, i32)> {
        let s = &self.field_vm.submode_screen;
        if !s.open || s.actor.state != SOFT_RESET_SLOT {
            return None;
        }
        s.soft_reset.records_y.map(|y| (SOFT_RESET_RECORDS_X, y))
    }

    /// Apply the side effects the last tick reported.
    pub(super) fn apply_submode_actions(&mut self) {
        let actions = self.field_vm.submode_screen.frame.actions.clone();
        for a in actions {
            match a {
                // The one action that moves persistent state: coins into the
                // casino bank `DAT_800845A4`, gold out of `DAT_8008459C`.
                HubAction::BuyCoins { coins, gold_cost } => {
                    if coins <= 0 {
                        continue;
                    }
                    self.minigames.casino_coins = self
                        .minigames
                        .casino_coins
                        .saturating_add(coins as u32)
                        .min(legaia_save::minigame_save::COIN_BANK_MAX as u32);
                    self.party.money = self.party.money.saturating_sub(gold_cost).max(0);
                }
                HubAction::ClearCursorRow => self.field_vm.submode_screen.counter.cursor = 0,
                // The hub's two ring producers cross to the host ring the way
                // field-VM op `0x36`'s do: `FUN_80035BD0(id)` overwrites the
                // last-written slot, `FUN_80035B50(id)` pushes.
                HubAction::ConfirmCue(id) => self.replace_last_sfx_cue(i16::from(id)),
                HubAction::EntryCue(id) => self.push_sfx_cue(i16::from(id)),
                _ => {}
            }
        }
    }
}
