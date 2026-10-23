//! Battle stat setters and the post-battle grants: XP, loot and steals.

use super::*;

/// The purse cap the battle spoils routine saturates at (`0x5F5E0FF`,
/// `FUN_8004E568` at `0x8004F0C4..0x8004F0FC`).
pub const BATTLE_GOLD_CAP: i32 = 99_999_999;

impl World {
    /// Set per-slot character max MP (mirrors `char_record[+0x140]`
    /// from the save record). Engines call this once per scene init -
    /// usually from `set_character_record_for_slot`. Unset slots default
    /// to `0`, which makes [`Self::use_item`] treat MP healing as a
    /// no-op for that slot.
    pub fn set_character_max_mp(&mut self, slot: u8, mp_max: u16) {
        let i = slot as usize;
        if i >= self.tables.character_max_mp.len() {
            self.tables.character_max_mp.resize(i + 1, 0);
        }
        self.tables.character_max_mp[i] = mp_max;
    }

    /// Reset every party-member's AP gauge for a new turn. Refills to
    /// `base_ap`, clears the Spirit-charged flag.
    pub fn reset_party_ap(&mut self) {
        for g in self.battle.ap_gauges.iter_mut() {
            g.reset_for_turn();
        }
    }

    /// Set the per-slot weapon attack used by Tactical-Art strike damage
    /// resolution. Engines call this when a character equips / unequips a
    /// weapon, or once at battle init from the active stat record.
    pub fn set_battle_attack(&mut self, slot: u8, atk: u16) {
        if let Some(s) = self.battle.attack.get_mut(slot as usize) {
            *s = atk;
        }
    }

    /// Set the per-slot magic attack scalar used by battle Magic damage
    /// resolution. Engines call this at battle init from the active stat
    /// record's magic stat.
    pub fn set_battle_magic(&mut self, slot: u8, mag: u16) {
        if let Some(s) = self.battle.magic.get_mut(slot as usize) {
            *s = mag;
        }
    }

    /// Set the per-slot generic defense - used when no UDF / LDF split is
    /// configured for the slot.
    pub fn set_battle_defense(&mut self, slot: u8, def: u16) {
        if let Some(s) = self.battle.defense.get_mut(slot as usize) {
            *s = def;
        }
    }

    /// Set per-slot UDF / LDF defense override. Replaces any prior value.
    /// Pass `None` to revert to [`Self::set_battle_defense`].
    pub fn set_battle_defense_split(&mut self, slot: u8, udf_ldf: Option<(u16, u16)>) {
        if let Some(s) = self.battle.defense_split.get_mut(slot as usize) {
            *s = udf_ldf;
        }
    }

    /// Resolve the defense value to use against a single Tactical-Art
    /// strike. Used by the world's `BattleActionHost::apply_art_strike`
    /// impl. Public so engines can call the same lookup directly when
    /// they want to apply art strikes outside the SM (e.g. for testing).
    pub fn resolve_battle_defense(
        &self,
        target_slot: u8,
        info: &legaia_engine_battle_vm::battle_action::ArtStrikeInfo,
    ) -> u16 {
        let idx = target_slot as usize;
        // If we have a UDF / LDF split for the slot, pick the half that
        // matches the strike's power target. Otherwise fall back to the
        // single defense value.
        if let Some(Some((udf, ldf))) = self.battle.defense_split.get(idx)
            && let Some(legaia_art::power::PowerByte::Damage(p)) = info.power
        {
            return match p.target {
                legaia_art::power::PowerTarget::Udf => *udf,
                legaia_art::power::PowerTarget::Ldf => *ldf,
            };
        }
        self.battle.defense.get(idx).copied().unwrap_or(0)
    }

    /// Distribute `xp_reward` (the summed enemy EXP) to the surviving party
    /// members after a `BattleEndCause::MonsterWipe`. Mirrors the retail split
    /// ([`legaia_engine_battle_vm::battle_formulas::victory_exp_per_member`], `FUN_8004E568`):
    ///
    /// - The summed reward is scaled by 3/4 (`v - (v >> 2)`), then **ceiling**-
    ///   divided among the surviving (HP > 0) members - not a floor-divide of
    ///   the raw sum.
    /// - Dead members (HP == 0) receive zero XP and are excluded from the divisor.
    ///
    /// For each member that crosses a level threshold, bumps the roster
    /// record's HP/MP maxima, resyncs the live `BattleActor` mirror, pushes
    /// a [`BattleEvent::LevelUp`], and appends a [`LevelUpResult`] to the
    /// returned vec.
    ///
    /// If every party member is dead (TPK) but the caller still invokes this
    /// (e.g. a Phoenix Down style revive-after-victory), the split degenerates
    /// to a no-op - there are no alive recipients.
    pub fn apply_battle_xp(&mut self, xp_reward: u32) -> Vec<LevelUpResult> {
        let party_count = self.party.party_count as usize;
        // Living-member count drives the divisor. We pull HP from
        // `BattleActor` (the live mirror) so the resolver sees the
        // post-battle state, not the record's saved HP.
        let alive: Vec<u8> = (0..party_count as u8)
            .filter(|&i| self.actors.get(i as usize).is_some_and(|a| a.battle.hp > 0))
            .collect();
        if alive.is_empty() {
            return Vec::new();
        }
        // Retail scales the summed EXP by 3/4 then ceiling-divides among the
        // living members (`FUN_8004E568` `8004e568.txt:461`), NOT a plain
        // floor-divide of the raw sum.
        let per_member_xp = legaia_engine_battle_vm::battle_formulas::victory_exp_per_member(
            xp_reward,
            alive.len() as u32,
        );
        if per_member_xp == 0 {
            return Vec::new();
        }
        let mut results = Vec::new();
        for member in alive {
            // XP / level state belongs to the CHARACTER (roster slot), while
            // the live HP/MP resync targets the battle ordinal's actor mirror.
            let char_id = self.party_roster_slot(member as usize) as u8;
            let result = self.party.level_up_tracker.grant_xp(char_id, per_member_xp);
            // Retail (`FUN_801E9504`) maintains the record's cumulative XP
            // (+0x0), next-level threshold (+0x4, slots-1/2 corrected), and
            // displayed-level byte (+0x130) on every grant; the Status menu
            // (`FUN_801D33D8`) draws those words verbatim. Mirror them so the
            // engine's status screen stays truthful even when no threshold
            // was crossed.
            let slot = char_id as usize;
            if slot < self.party.level_up_tracker.level.len() {
                let cur_level = self.party.level_up_tracker.level[slot];
                let next = self
                    .party
                    .level_up_tracker
                    .threshold_for(slot, cur_level)
                    .unwrap_or(0);
                if let Some(rec) = self.party.roster.members.get_mut(slot) {
                    rec.set_cumulative_xp(self.party.level_up_tracker.xp[slot]);
                    rec.set_next_level_xp(next);
                    rec.set_magic_rank(cur_level);
                }
            }
            let Some(result) = result else {
                continue;
            };
            if let Some(rec) = self.party.roster.members.get_mut(char_id as usize) {
                LevelUpTracker::apply_to_record(&result, rec);
            }
            // The grown base window reaches the live maxima through the
            // aggregator's rebuild, which adds the equipment's boosts
            // (`FUN_80042558`). Record only: mid-battle the actor holds the
            // live HP.
            self.rebuild_record_live_hp_mp(char_id as usize);
            let new_hms = self
                .party
                .roster
                .members
                .get(char_id as usize)
                .map(|r| r.hp_mp_sp());
            // Only the **maximum** moves. The live HP / MP on the battle
            // actor are the authoritative post-fight values - the record's
            // `hp_cur` is the pre-fight snapshot until `persist_battle_party_hp`
            // runs at the end of `finish_battle`, so copying it back here
            // would undo the whole fight's damage. It did exactly that while
            // `apply_to_record` also restored `cur` to `max`: the pair read as
            // "levelling up heals you", which retail's `FUN_801E9504` does not
            // do (see [`LevelUpTracker::apply_to_record`]).
            if let (Some(actor), Some(hms)) = (self.actors.get_mut(member as usize), new_hms) {
                actor.battle.max_hp = hms.hp_max;
            }
            self.pending_battle_events.push(BattleEvent::LevelUp {
                char_id,
                new_level: result.new_level,
                hp_gained: result.hp_gained,
                mp_gained: result.mp_gained,
            });
            // One fight can level several members. The banner is a single
            // slot, so the first leveller takes it and the rest queue behind
            // it - assigning here unconditionally meant every leveller after
            // the first overwrote its predecessor within the same frame, and
            // a battle that levelled three showed one banner.
            let banner = LevelUpBanner {
                char_id,
                new_level: result.new_level,
                hp_gained: result.hp_gained,
                mp_gained: result.mp_gained,
                frames_remaining: LevelUpBanner::DEFAULT_FRAMES,
            };
            if self.party.current_level_up_banner.is_none() {
                self.party.current_level_up_banner = Some(banner);
            } else {
                self.party.pending_level_up_banners.push_back(banner);
            }
            results.push(result);
        }
        results
    }

    /// Resolve the victory spoils for `formation` (the reward half of
    /// `FUN_8004E568`): accumulate each dead enemy's gold as `gold >> 1`,
    /// finalize it through the +25% bonus + halve
    /// ([`legaia_engine_battle_vm::battle_formulas::victory_gold_finalize`]) and add it to
    /// [`legaia_engine_menus::party_state::PartyState::money`]; sum the enemy EXP and distribute it (scaled 3/4,
    /// ceiling-split) via [`World::apply_battle_xp`]; roll the one drop
    /// retail offers ([`legaia_engine_battle_vm::battle_formulas::victory_drop_roll`], one BIOS
    /// `rand()` per enemy seat plus the 1-in-4 gate). Returns the aggregated
    /// [`BattleRewards`] (`gold` is the **credited** amount, not the raw sum) so
    /// engines can surface the post-battle banner ("got N XP, M gold,
    /// learned spell X").
    ///
    /// Monsters whose ids aren't in `catalog` contribute zero - the call
    /// silently skips them rather than failing, so a partially-populated
    /// catalog still drives a battle-end transition.
    pub fn apply_battle_loot(
        &mut self,
        formation: &legaia_engine_battle::monster_catalog::FormationDef,
        catalog: &legaia_engine_battle::monster_catalog::MonsterCatalog,
    ) -> BattleRewards {
        let mut xp_total: u32 = 0;
        // Accumulated `gold >> 1` over dead enemies (the victory-gold accumulator
        // in `FUN_8004E568`); finalized below via the `>> 1` halve + optional
        // +25% bonus. NOT the raw record-gold sum.
        let mut gold_acc: u32 = 0;
        let mut drop_seats = Vec::with_capacity(formation.slots.len());
        // Captured seats cannot supply the drop (actor `+0x227`, bumped by the
        // capture takedown). The engine logs captures by monster id, so each
        // logged id claims the earliest formation seat carrying it.
        let mut captured = self.seru.battle_captures.clone();
        for slot in &formation.slots {
            let taken = captured
                .iter()
                .position(|&id| id == slot.monster_id)
                .map(|i| captured.remove(i))
                .is_some();
            let Some(def) = catalog.get(slot.monster_id) else {
                continue;
            };
            // `WorldToggles::exp_rate_permille` (the `--exp-scale` twin)
            // scales each record's EXP before the sum, as the patch scales
            // the record field; `1000` is the identity.
            xp_total = xp_total.saturating_add(u32::from(crate::world::scale_exp_permille(
                def.exp,
                self.toggles.exp_rate_permille,
            )));
            gold_acc = gold_acc.saturating_add(
                legaia_engine_battle_vm::battle_formulas::victory_gold_per_monster(def.gold),
            );
            drop_seats.push(legaia_engine_battle_vm::battle_formulas::VictoryDropSeat {
                item: def.drop_item.unwrap_or(0),
                chance_pct: def.drop_chance_pct,
                captured: taken,
            });
        }
        // The drop roll (`0x8004F3D8..0x8004F5A0`). The Items Up bonus reads
        // the living members' second ability word (`+0xF8 & 0x20000`, the
        // bit the steal attack's doubling also reads); the no-reward gate is
        // the whole special-battle word `_DAT_8007BAC0` (`0x8004F480`), so an
        // arena leg and a Ra-Seru-forbidden fight both skip the seat loop.
        let party_n = self.party.party_count as usize;
        let items_up = (0..party_n).any(|i| {
            self.actors.get(i).is_some_and(|a| a.battle.hp > 0)
                && self
                    .party
                    .roster
                    .members
                    .get(self.party_roster_slot(i))
                    .is_some_and(|rec| {
                        let b = rec.ability_bits();
                        u32::from_le_bytes([b[4], b[5], b[6], b[7]])
                            & legaia_engine_battle::battle_steal::ITEMS_UP_BIT
                            != 0
                    })
        });
        let no_reward = self.special_battle_word() != 0;
        let item = legaia_engine_battle_vm::battle_formulas::victory_drop_roll(
            &drop_seats,
            items_up,
            no_reward,
            || self.next_rand(),
        );
        let mut drops: Vec<u8> = Vec::new();
        // `FUN_80042F4C(item) == 99` skips the grant (`0x8004F5A8..0x8004F5BC`).
        if item != 0
            && self.party.inventory.get(&item).copied().unwrap_or(0)
                != legaia_engine_battle::battle_steal::HELD_CAP
        {
            drops.push(item);
            let _ = self.party.inventory.add(item, 1);
        }
        // The bonus equipment drop (`WorldToggles::bonus_drop`, the
        // `--equipment-drops` twin) rolls here, where the hook joins: right
        // after the normal drop grant, at `0x8004F610`. The no-reward arm
        // reaches the same join (`0x8004F570` -> `0x8004F5A0` -> `0x8004F610`),
        // so a special battle rolls it too, as on a patched disc.
        self.roll_bonus_equipment_drop();
        // The +25% gold bonus fires when a living party member carries bit
        // `0x10000` of the SECOND ability word (`FUN_8004E568` tests the u32
        // at record `+0xF8`): overall bit 48 of the `+0xF4` bitfield = byte 6,
        // mask 0x01 = accessory passive index 0x30 ("Gold Boost", the Golden
        // Book - see `docs/formats/accessory-passive-table.md`). "Living" =
        // post-battle battle HP > 0, the same set `apply_battle_xp` divides
        // EXP among.
        let party_count = self.party.party_count as usize;
        let more_gold = (0..party_count).any(|i| {
            self.actors.get(i).is_some_and(|a| a.battle.hp > 0)
                && self
                    .party
                    .roster
                    .members
                    .get(self.party_roster_slot(i))
                    .is_some_and(|rec| rec.ability_bits()[6] & 0x01 != 0)
        });
        // The same word zeroes the gold after the Golden Book bonus and before
        // the halve (`0x8004F0AC..0x8004F0BC`, `sw zero, 0xA3C(gp)`), and
        // each member's EXP share (`0x8004F274..0x8004F284`, `s6 = 0`, so the
        // doubled-EXP bit adds nothing either). The per-member level check
        // `FUN_801E9504` still runs, on an unchanged total.
        let gold_credited = if no_reward {
            0
        } else {
            legaia_engine_battle_vm::battle_formulas::victory_gold_finalize(gold_acc, more_gold)
        };
        if no_reward {
            xp_total = 0;
        }

        let alive = (0..usize::from(self.party.party_count))
            .filter(|&i| self.actors.get(i).is_some_and(|a| a.battle.hp > 0))
            .count() as u32;
        let xp_share =
            legaia_engine_battle_vm::battle_formulas::victory_exp_per_member(xp_total, alive);
        let level_ups = if xp_total > 0 {
            self.apply_battle_xp(xp_total)
        } else {
            Vec::new()
        };
        // The spoils routine adds the gold to the purse and saturates it at
        // 99,999,999 (`lui a3,0x5f5` / `ori a3,a3,0xe0ff` at `0x8004F0C4`,
        // `slt` + store at `0x8004F0EC..0x8004F0FC`) - a cap ten times the
        // shop sale's ([`legaia_engine_menus::shop::GOLD_CAP`]).
        let new_money = (self.party.money as i64).saturating_add(gold_credited as i64);
        self.party.money = new_money.clamp(i32::MIN as i64, BATTLE_GOLD_CAP as i64) as i32;
        // NB: no scripted-boss "victory latch" here. The first-visit gate
        // flag (rikuroa Caruban = `0x142`) lands by EXECUTING the scene's
        // post-victory partition-2 record: the post-battle field return
        // re-runs the scene-entry script `P1[0]`, whose staged-marker test
        // (`0x289`, SET by the stager record `P1[3]`'s own `52 89` when the
        // approach dispatch ran it pre-battle) spawns `P2[50]` through the
        // gated record dispatch, and that record's own `51 42` script bytes
        // set the flag. See `SceneHost::tick`'s battle-return arm.
        BattleRewards {
            xp: xp_total,
            xp_share,
            gold: gold_credited,
            level_ups,
            drops,
        }
    }

    /// The bonus equipment pool: every weapon, armour and accessory the
    /// curated gamedata names whose name resolves to an id in the disc's own
    /// item-name table, ascending - the same join the patcher's
    /// `--equipment-drops` embeds (`Database::equipment_pool_by_name`, shared
    /// with `legaia_patcher::equipment`). Built once from the installed
    /// executable and cached; empty without one.
    pub fn bonus_drop_pool(&mut self) -> Vec<u8> {
        if self.tables.bonus_drop_pool.is_none() {
            let pool = self
                .tables
                .scus
                .as_deref()
                .and_then(legaia_game_tables::item_names::ItemNameTable::from_scus)
                .map(|names| {
                    legaia_gamedata::Database::load()
                        .equipment_pool_by_name(|id| names.name(id).map(str::to_string))
                        .into_iter()
                        .map(|(id, _)| id)
                        .collect()
                })
                .unwrap_or_default();
            self.tables.bonus_drop_pool = Some(pool);
        }
        self.tables.bonus_drop_pool.clone().unwrap_or_default()
    }

    /// The bonus equipment drop's roll (`WorldToggles::bonus_drop`): the hook's
    /// `rand() % 100 < chance`, then `rand() % pool_len` picks the id, which
    /// goes straight into the bag - silent, as the hook's grant is (no
    /// results-window line). Off, or with no pool, nothing is drawn. Returns
    /// the granted id.
    pub(in crate::world) fn roll_bonus_equipment_drop(&mut self) -> Option<u8> {
        if !self.toggles.bonus_drop {
            return None;
        }
        let pool = self.bonus_drop_pool();
        if pool.is_empty() {
            return None;
        }
        if self.next_rand() % 100 >= crate::world::BONUS_DROP_CHANCE_PCT {
            return None;
        }
        let id = pool[self.next_rand() as usize % pool.len()];
        let _ = self.party.inventory.add(id, 1);
        log::info!("battle end: bonus equipment drop {id:#04x}");
        Some(id)
    }

    /// Run-away EXP (`WorldToggles::flee_exp`, the engine twin of the
    /// patcher's `--flee-exp` hook into the escape teardown): bank
    /// [`crate::world::FLEE_EXP_PCT`] percent of the fled formation's listed
    /// EXP - every formation seat, at the [`crate::world::WorldToggles::exp_rate_permille`]
    /// scale - into each party member's cumulative XP. Banked, not levelled,
    /// as the hook does: the share lands in the XP total the status screen
    /// shows, and the next won battle's level check crosses any threshold it
    /// reached. Each total saturates at the game's `9,999,999`. A special
    /// battle (`_DAT_8007BAC0 != 0`) pays nothing, as its win does not.
    /// Returns the per-member amount banked (`0` when the toggle is off).
    pub(in crate::world) fn bank_flee_exp(&mut self) -> u32 {
        const XP_CAP: u32 = 9_999_999;
        if !self.toggles.flee_exp || self.special_battle_word() != 0 {
            return 0;
        }
        let Some(formation) = self.battle.active_formation.as_ref() else {
            return 0;
        };
        let rate = self.toggles.exp_rate_permille;
        let total: u32 = formation
            .slots
            .iter()
            .filter_map(|slot| self.tables.monster_catalog.get(slot.monster_id))
            .map(|def| u32::from(crate::world::scale_exp_permille(def.exp, rate)))
            .fold(0u32, u32::saturating_add);
        let share = total.saturating_mul(crate::world::FLEE_EXP_PCT) / 100;
        if share == 0 {
            return 0;
        }
        for member in 0..self.party.party_count as usize {
            let slot = self.party_roster_slot(member);
            let tracker = &mut self.party.level_up_tracker;
            let Some(xp) = tracker.xp.get_mut(slot) else {
                continue;
            };
            *xp = xp.saturating_add(share).min(XP_CAP);
            let banked = *xp;
            if let Some(rec) = self.party.roster.members.get_mut(slot) {
                rec.set_cumulative_xp(banked);
            }
        }
        share
    }

    /// Resolve a **steal** attempt against `monster_id` using the per-monster
    /// steal table (the Evil God Icon mechanic). Rolls the monster's steal
    /// chance against the deterministic world RNG; on success the stolen item is
    /// added to [`legaia_engine_menus::party_state::PartyState::inventory`] and its id returned. Returns `None` when the
    /// monster has no steal (item `0` / chance `0`) or the roll misses.
    ///
    /// The steal item + chance live in a static `SCUS_942.54` table
    /// (`DAT_80077828`), **not** the monster record, so the caller passes the
    /// parsed [`legaia_game_tables::steal_table::StealTable`] (the engine reads the
    /// disc-resident data the randomizer edits). This is the steal counterpart
    /// to the drop grant in [`Self::apply_battle_loot`]: a percent roll
    /// (`rand % 100 < chance`) then the same `inventory` add. See
    /// `docs/formats/steal-table.md`.
    pub fn apply_steal(
        &mut self,
        monster_id: u16,
        steal_table: &legaia_game_tables::steal_table::StealTable,
    ) -> Option<u8> {
        let entry = steal_table.entry(monster_id).filter(|e| e.is_stealable())?;
        let roll = (self.next_rand() % 100) as u8;
        if roll < entry.chance_pct {
            let _ = self.party.inventory.add(entry.item_id, 1);
            Some(entry.item_id)
        } else {
            None
        }
    }
}
