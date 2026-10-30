//! The `World` side of the run rules (`legaia_engine_run_rules`): the rules
//! a run plays under, folded in at each reader they change.
//!
//! Every hook here is the identity under [`RunRules::default`] - the retail
//! game: a permille knob at `1000` hands the reader the value it read
//! before, a gate that is off changes nothing, the randomizer and New Game+
//! do nothing without their rule. So a world with no rules plays exactly as
//! retail does, and its saves carry no run block.
//!
//! The randomizer's picks come from `legaia_randomizer` - the disc patcher's
//! planners and safety lists - keyed by the run seed and the site (monster
//! id, scene and shop, scene and item), never by the order the player meets
//! the sites, and never through the game's own `rand()` stream.

use std::collections::BTreeMap;
use std::sync::Arc;

use legaia_engine_battle::encounter::EncounterRoll;
use legaia_engine_battle::monster_catalog::{FormationDef, MonsterDef};
use legaia_engine_run_rules::boss_rush::{BossRushRecord, BossRushRun, Intermission};
use legaia_engine_run_rules::rules::{CarrySet, Knobs, RunRules, mul_permille};
use legaia_engine_run_rules::save::RunSave;
use legaia_randomizer::items::ItemPools;

use super::World;

/// The first formation id a randomized encounter is registered under: its
/// source row's id with these bits set: clear of every scene row and below
/// the pack formations (`0xE000` up).
pub const RANDOM_FORMATION_BASE: u16 = 0xD000;

/// What a New Game+ keeps from the save it starts from, lifted off a world
/// that has loaded that save ([`World::ng_plus_snapshot`]).
#[derive(Debug, Clone, Default)]
pub struct NgPlusSnapshot {
    pub records: Vec<legaia_save::CharacterRecord>,
    pub per_char_ext: Vec<(u8, legaia_save::CharSaveExt)>,
    pub saved_chains: Vec<legaia_save::SavedChainRecord>,
    pub tactical_arts: Option<legaia_engine_battle::tactical_arts::TacticalArtsTracker>,
    pub seru_log: Option<legaia_engine_battle::seru_learning::SeruCaptureLog>,
    pub inventory: Vec<(u8, u8)>,
    pub money: i32,
    /// The pack store: trading cards, pack items and worn pack gear.
    pub pack_cards: Option<legaia_engine_modes::cards::economy::CardProgress>,
    pub pack_items: Option<legaia_engine_expansion::items::ItemProgress>,
    /// The source's own New Game+ cycle.
    pub cycle: u8,
}

/// A run the host has set up for the next New Game.
#[derive(Debug, Clone, Default)]
pub struct StagedRun {
    pub rules: RunRules,
    pub carry: Option<NgPlusSnapshot>,
}

/// The New Game rules screen while it is up: the rows and the saves its New
/// Game+ row offers, index-aligned with the setup's sources.
#[derive(Debug, Clone)]
pub struct RulesSetupSeat {
    pub setup: legaia_engine_run_rules::setup::RulesSetup,
    pub sources: Vec<legaia_save::SaveFile>,
}

/// The run-rule state a world carries.
#[derive(Debug, Clone, Default)]
pub struct RunRulesState {
    /// The rules in force.
    pub rules: RunRules,
    /// Set up by the host before New Game; taken by the New Game.
    pub staged: Option<StagedRun>,
    /// The New Game rules screen, while it is up (the hosts open it between
    /// the title's NEW GAME and the opening).
    pub setup: Option<Box<RulesSetupSeat>>,
    /// The ironman run's id (`0` outside ironman).
    pub run_id: u64,
    /// The Boss Rush in progress.
    pub boss_rush: Option<BossRushRun>,
    /// The Boss Rush intermission screen, while it is open.
    pub intermission: Option<Intermission>,
    /// The Boss Rush leaderboard.
    pub record: BossRushRecord,
    /// Ironman: the party fell; the host erases the run's save.
    pub ironman_fell: bool,
    /// Ironman: a scene was entered; the host autosaves the run.
    pub autosave_due: bool,
    /// The New Game+ carry between the two halves of a New Game.
    pending_carry: Option<NgPlusSnapshot>,
    /// The randomizer's item populations, built from the disc once.
    pools: Option<Arc<ItemPools>>,
    /// Catalog entries a fight's randomized swap replaced.
    shadowed: Vec<(u16, Option<MonsterDef>)>,
    /// The fight's randomized steals.
    steals: BTreeMap<u16, legaia_game_tables::steal_table::StealEntry>,
    /// Randomized formations registered this scene.
    random_formations: Vec<u16>,
}

/// The parts of a character record each carried part spans: levels (EXP,
/// grown stats, HP / MP), Seru magic (`+0x13C..+0x185`), arts
/// (`+0x185..+0x196`), equipment (`+0x196..+0x19E`). The ability bits
/// (`+0xF4..+0x104`) are rebuilt from the gear and are not copied.
const LEVEL_SPANS: [std::ops::Range<usize>; 2] = [0x000..0x0F4, 0x104..0x13C];
const SERU_SPAN: std::ops::Range<usize> = 0x13C..0x185;
const ARTS_SPAN: std::ops::Range<usize> = 0x185..0x196;
const EQUIP_SPAN: std::ops::Range<usize> = 0x196..0x19E;

/// `fresh` with the parts `carry` keeps copied from `src`.
pub fn carry_record(fresh: &[u8], src: &[u8], carry: &CarrySet) -> Vec<u8> {
    let mut out = fresh.to_vec();
    let mut copy = |r: std::ops::Range<usize>| {
        if r.end <= out.len() && r.end <= src.len() {
            out[r.clone()].copy_from_slice(&src[r]);
        }
    };
    if carry.levels {
        for r in LEVEL_SPANS {
            copy(r);
        }
    }
    if carry.seru {
        copy(SERU_SPAN);
    }
    if carry.arts {
        copy(ARTS_SPAN);
    }
    if carry.equipment {
        copy(EQUIP_SPAN);
    }
    out
}

impl World {
    /// The knob bundle in force.
    pub fn run_knobs(&self) -> Knobs {
        self.run_rules.rules.knobs()
    }

    /// Set up the rules (and the New Game+ carry) the next New Game starts
    /// under. `None` clears a staged run, so the next New Game is retail.
    pub fn stage_run(&mut self, run: Option<StagedRun>) {
        self.run_rules.staged = run;
    }

    /// The strongest present member's level.
    fn run_party_level(&self) -> u8 {
        (0..usize::from(self.party.party_count.clamp(1, 3)))
            .filter_map(|m| {
                self.party
                    .level_up_tracker
                    .level
                    .get(self.party_roster_slot(m))
                    .copied()
            })
            .max()
            .unwrap_or(1)
    }

    /// The enemy stat scale a fight seats `monster_id` at: the session's
    /// enemy-strength tweak times the run's difficulty and New Game+ curve.
    pub(in crate::world) fn run_enemy_stat_permille(&self, monster_id: u16) -> u16 {
        let rules = &self.run_rules.rules;
        let toggle = self.toggles.enemy_stat_permille;
        if rules.is_retail() {
            return toggle;
        }
        let r = rules.enemy_stat_permille(monster_id, self.run_party_level());
        mul_permille(u32::from(toggle), r).clamp(100, 10_000) as u16
    }

    /// The enemy attack-count scale: the tweak times the difficulty's.
    pub(in crate::world) fn run_enemy_attack_permille(&self) -> u16 {
        let t = self.toggles.enemy_attack_permille;
        mul_permille(u32::from(t), self.run_knobs().enemy_attack_permille).clamp(100, 10_000) as u16
    }

    /// The EXP rate: the tweak times the difficulty's.
    pub(in crate::world) fn run_exp_permille(&self) -> u16 {
        let t = self.toggles.exp_rate_permille;
        mul_permille(u32::from(t), self.run_knobs().exp_permille).clamp(1, 10_000) as u16
    }

    /// A victory's credited gold under the difficulty's gold scale.
    pub(in crate::world) fn run_scale_gold(&self, gold: u32) -> u32 {
        mul_permille(gold, self.run_knobs().gold_permille)
    }

    /// The field encounter-rate scale the region step drains by.
    pub fn run_encounter_rate_permille(&self) -> u16 {
        self.run_knobs().encounter_rate_permille
    }

    /// A shop buy price under the difficulty's price scale (at least `1`
    /// for a priced item).
    pub fn run_scale_price(&self, price: u32) -> u32 {
        let p = self.run_knobs().price_permille;
        if p == 1000 || price == 0 {
            return price;
        }
        mul_permille(price, p).max(1)
    }

    /// A healing item's restore amount under the difficulty's potency.
    pub fn run_scale_heal(&self, amount: u32) -> u32 {
        let p = self.run_knobs().heal_permille;
        if p == 1000 || amount == 0 {
            return amount;
        }
        mul_permille(amount, p).max(1)
    }

    /// A healing item's outcome under the difficulty's potency: the HP / MP
    /// it restores scaled, held to what the target is missing.
    pub(in crate::world) fn run_scale_item_outcome(
        &self,
        outcome: legaia_engine_menus::items::ItemOutcome,
        target: &legaia_engine_menus::items::TargetSnapshot,
    ) -> legaia_engine_menus::items::ItemOutcome {
        use legaia_engine_menus::items::ItemOutcome as O;
        if self.run_knobs().heal_permille == 1000 {
            return outcome;
        }
        let scale = |amount: u16, missing: u16| {
            (self
                .run_scale_heal(u32::from(amount))
                .min(u32::from(missing))) as u16
        };
        match outcome {
            O::HealedHp { amount } => O::HealedHp {
                amount: scale(amount, target.hp_max.saturating_sub(target.hp)),
            },
            O::HealedMp { amount } => O::HealedMp {
                amount: scale(amount, target.mp_max.saturating_sub(target.mp)),
            },
            other => other,
        }
    }

    /// An AP grant (Spirit turn or hit taken) under the difficulty's AP
    /// scale; negative (draining) settings pass through.
    pub fn run_scale_ap(&self, ap: i16) -> i16 {
        let p = self.run_knobs().ap_gain_permille;
        if p == 1000 || ap <= 0 {
            return ap;
        }
        mul_permille(ap as u32, p).min(i16::MAX as u32) as i16
    }

    /// Whether a monster's single-target attack picks the weakest member:
    /// the session switch, or the difficulty's.
    pub fn run_smart_monsters(&self) -> bool {
        self.toggles.smarter_monster_targeting || self.run_knobs().smart_monsters
    }

    /// Whether the difficulty refuses the party's Run.
    pub fn run_no_escape(&self) -> bool {
        self.run_knobs().no_escape
    }

    /// Whether the run allows items in battle (hardcore ironman does not).
    pub fn run_battle_items_allowed(&self) -> bool {
        !self.run_rules.rules.ironman.is_some_and(|i| i.hardcore)
    }

    // ---- New Game ---------------------------------------------------------

    /// The New Game's half of the rules: the staged run's rules become the
    /// world's (a New Game with nothing staged is retail), a fresh ironman
    /// run id is drawn, and the Boss Rush leaderboard survives. Called from
    /// [`World::begin_new_game`]; the New Game+ carry waits for
    /// [`Self::finish_run_new_game`].
    pub(crate) fn begin_run_new_game(&mut self) {
        let staged = self.run_rules.staged.take().unwrap_or_default();
        let record = std::mem::take(&mut self.run_rules.record);
        let pools = self.run_rules.pools.take();
        self.run_rules = RunRulesState {
            rules: staged.rules,
            record,
            pools,
            ..Default::default()
        };
        if self.run_rules.rules.ironman.is_some() {
            // Any non-zero id unique to this New Game will do.
            let r = (self.next_rand() as u64) << 32 | self.next_rand() as u64;
            self.run_rules.run_id = r | 1;
        }
        self.run_rules.pending_carry = staged.carry;
    }

    /// The New Game's second half, after the starting party and bag are
    /// seeded: the New Game+ carry and the randomizer's starting equipment.
    pub(crate) fn finish_run_new_game(&mut self) {
        let carry = self.run_rules.pending_carry.take();
        if let (Some(snap), Some(ng)) = (carry, self.run_rules.rules.ng_plus) {
            self.apply_ng_plus(snap, &ng.carry);
        }
        if self
            .run_rules
            .rules
            .randomizer
            .as_ref()
            .is_some_and(|r| r.starting_equipment)
        {
            self.randomize_starting_equipment();
        }
        self.sync_run_pack_flags();
    }

    /// What a New Game+ would keep from this world's progress: call on a
    /// world that has just loaded the source save.
    pub fn ng_plus_snapshot(&self) -> NgPlusSnapshot {
        let p = &self.expansion.quests.progress;
        let has_pack = self.expansion.pack.is_some();
        NgPlusSnapshot {
            records: self.party.roster.members.clone(),
            per_char_ext: self.party.per_char_ext.clone(),
            saved_chains: self.party.saved_chains.clone(),
            tactical_arts: Some(self.party.tactical_arts.clone()),
            seru_log: Some(self.seru.log.clone()),
            inventory: self
                .party
                .inventory
                .iter()
                .map(|(&id, &n)| (id, n))
                .collect(),
            money: self.party.money,
            pack_cards: has_pack.then(|| p.cards.clone()),
            pack_items: has_pack.then(|| p.items.clone()),
            cycle: self.run_rules.rules.ng_plus_cycle(),
        }
    }

    /// Fold a New Game+ carry into the freshly seeded party.
    fn apply_ng_plus(&mut self, snap: NgPlusSnapshot, carry: &CarrySet) {
        let keep: Vec<u8> = self.run_item_pools().keep.clone();
        for (slot, src) in snap.records.iter().enumerate() {
            if src.hp_mp_sp().hp_max == 0 {
                continue;
            }
            while self.party.roster.members.len() <= slot {
                self.party
                    .roster
                    .members
                    .push(legaia_save::CharacterRecord::zeroed());
            }
            let fresh = &self.party.roster.members[slot];
            // A slot the New Game has not seeded (everyone but Vahn) takes the
            // template row the story would seed it from, so the join keeps
            // the carried record.
            let base = if fresh.hp_mp_sp().hp_max == 0 {
                self.tables
                    .starting_party
                    .as_ref()
                    .and_then(|t| t.member(slot))
                    .map(legaia_engine_menus::new_game::starting_record)
                    .unwrap_or_else(|| src.clone())
            } else {
                fresh.clone()
            };
            let raw = carry_record(&base.raw, &src.raw, carry);
            self.party.roster.members[slot].raw = raw;
            let rec = &self.party.roster.members[slot];
            if slot < self.party.level_up_tracker.level.len() {
                self.party.level_up_tracker.xp[slot] = rec.cumulative_xp();
                self.party.level_up_tracker.level[slot] = rec.level().max(1);
            }
            self.rebuild_record_live_hp_mp(slot);
        }
        if carry.seru || carry.arts {
            let mut ext = snap.per_char_ext.clone();
            for (_, ce) in ext.iter_mut() {
                if !carry.arts {
                    ce.learned_arts_mask = 0;
                    ce.active_chains = Default::default();
                }
                if !carry.seru {
                    ce.spells.clear();
                    ce.seru_captures.clear();
                    ce.shiny_spells.clear();
                }
            }
            self.party.per_char_ext = ext;
        }
        if carry.arts {
            self.party.saved_chains = snap.saved_chains.clone();
            if let Some(t) = snap.tactical_arts.clone() {
                self.party.tactical_arts = t;
            }
        }
        if carry.seru
            && let Some(log) = snap.seru_log.clone()
        {
            self.seru.log = log;
        }
        if carry.items {
            // Quest and key items stay where the story hands them over.
            for &(id, count) in &snap.inventory {
                if id != 0 && count != 0 && !keep.contains(&id) {
                    let _ = self.party.inventory.add(id, count);
                }
            }
        }
        if carry.gold {
            self.party.money = snap.money.max(self.party.money);
        }
        let progress = &mut self.expansion.quests.progress;
        if carry.cards
            && let Some(c) = snap.pack_cards
        {
            progress.cards = c;
        }
        if let Some(items) = snap.pack_items {
            if carry.items {
                progress.items.bag = items.bag;
            }
            if carry.equipment {
                progress.items.worn = items.worn;
            }
        }
        // Vahn's live mirror off the merged record.
        if let Some(rec) = self.party.roster.members.first() {
            let hms = rec.hp_mp_sp();
            let a = &mut self.actors[0];
            a.battle.hp = hms.hp_cur;
            a.battle.max_hp = hms.hp_max;
            a.battle.mp = hms.mp_cur;
        }
        self.seed_party_battle_stats();
    }

    /// The run's flags in the active pack's flag bank, so a pack can gate
    /// New Game+ content (`requires = { flags = ["run.ng_plus"] }`): the flag
    /// `run.ng_plus` and the counter `run.ng_plus_cycle`, plus `run.hard`
    /// (Hard or Nightmare), `run.ironman` and `run.randomizer`. Nothing is
    /// written without a pack, and nothing is raised under retail rules.
    pub fn sync_run_pack_flags(&mut self) {
        if self.expansion.pack.is_none() {
            return;
        }
        use legaia_engine_run_rules::rules::Difficulty;
        let r = self.run_rules.rules.clone();
        let flags = &mut self.expansion.quests.progress.flags;
        let mut raise = |name: &str, on: bool| {
            if on {
                flags.set(name);
            }
        };
        raise("run.ng_plus", r.ng_plus.is_some());
        raise(
            "run.hard",
            matches!(r.difficulty, Difficulty::Hard | Difficulty::Nightmare),
        );
        raise("run.ironman", r.ironman.is_some());
        raise("run.randomizer", r.randomizer.is_some());
        if r.ng_plus.is_some() {
            flags
                .counters
                .insert("run.ng_plus_cycle".into(), i64::from(r.ng_plus_cycle()));
        }
    }

    // ---- Randomizer -------------------------------------------------------

    /// The randomizer's item pools, from the disc's item table (empty
    /// without one: nothing is reassigned).
    fn run_item_pools(&mut self) -> Arc<ItemPools> {
        if let Some(p) = self.run_rules.pools.clone() {
            return p;
        }
        let pools = Arc::new(match self.tables.scus.as_deref() {
            Some(scus) => {
                let names = legaia_game_tables::item_names::ItemNameTable::from_scus(scus);
                ItemPools::from_items(|id| {
                    let named = names.as_ref().is_some_and(|t| t.name(id).is_some());
                    let price = legaia_game_tables::item_names::item_price(scus, id)?;
                    (named || price > 0).then_some((named, u32::from(price)))
                })
            }
            None => ItemPools::default(),
        });
        self.run_rules.pools = Some(pools.clone());
        pools
    }

    /// Build the randomizer's item pools now, if a randomizer is on, so a
    /// reader holding a borrow of the world can use them.
    pub(crate) fn prime_run_item_pools(&mut self) {
        if self.run_rules.rules.randomizer.is_some() {
            let _ = self.run_item_pools();
        }
    }

    fn run_randomizer(&self) -> Option<&legaia_engine_run_rules::rules::Randomizer> {
        self.run_rules.rules.randomizer.as_ref()
    }

    /// Seat the randomizer's numbers for a fight on `formation`: each
    /// formation monster's stats moved inside the band, its drop and steal
    /// reassigned from the sellable pool. Runs after the expansion pack's
    /// swap and before any stat is read; a monster the pack swapped keeps
    /// the pack's numbers.
    pub(in crate::world) fn run_rules_prepare_battle(&mut self, formation: &FormationDef) {
        self.run_rules_end_battle();
        let Some(r) = self.run_randomizer().cloned() else {
            return;
        };
        if !(r.monster_stats || r.drops || r.steals) {
            return;
        }
        let seed = r.seed_value();
        let pools = self.run_item_pools();
        let pack_swapped: Vec<u16> = self.expansion.battle.shadowed.iter().map(|s| s.0).collect();
        let mut ids: Vec<u16> = formation.slots.iter().map(|s| s.monster_id).collect();
        ids.sort_unstable();
        ids.dedup();
        for id in ids {
            if pack_swapped.contains(&id) {
                continue;
            }
            let Some(def) = self.tables.monster_catalog.get(id).cloned() else {
                continue;
            };
            let mut d = def.clone();
            if r.monster_stats {
                use legaia_randomizer::bands::{Band, band_stats};
                let [hp, mp] = band_stats(seed, id, [d.hp, d.mp], Band::DEFAULT);
                d.hp = hp;
                d.mp = mp;
                if d.raw_stats == [0u16; 6] {
                    let s = band_stats(
                        seed,
                        id,
                        [d.attack, d.udf, d.ldf, d.intel, d.speed],
                        Band::DEFAULT,
                    );
                    [d.attack, d.udf, d.ldf, d.intel, d.speed] = s;
                } else {
                    let mut five = [0u16; 5];
                    five.copy_from_slice(&d.raw_stats[1..]);
                    let s = band_stats(seed, id, five, Band::DEFAULT);
                    d.raw_stats[1..].copy_from_slice(&s);
                }
            }
            if r.drops
                && let Some(item) = d.drop_item
            {
                d.drop_item = Some(legaia_randomizer::sites::reassign_item(
                    seed,
                    "drop",
                    &id.to_le_bytes(),
                    item,
                    &pools.keep,
                    &pools.sellable,
                ));
            }
            if r.steals
                && let Some(e) = self.tables.steal_table.as_ref().and_then(|t| t.entry(id))
                && e.item_id != 0
            {
                let item_id = legaia_randomizer::sites::reassign_item(
                    seed,
                    "steal",
                    &id.to_le_bytes(),
                    e.item_id,
                    &pools.keep,
                    &pools.sellable,
                );
                self.run_rules.steals.insert(
                    id,
                    legaia_game_tables::steal_table::StealEntry {
                        chance_pct: e.chance_pct,
                        item_id,
                    },
                );
            }
            if d != def {
                let old = self.tables.monster_catalog.by_id.insert(id, d);
                self.run_rules.shadowed.push((id, old));
            }
        }
    }

    /// Put back the catalog entries the randomizer swapped for a fight.
    pub(in crate::world) fn run_rules_end_battle(&mut self) {
        for (id, old) in std::mem::take(&mut self.run_rules.shadowed)
            .into_iter()
            .rev()
        {
            match old {
                Some(def) => {
                    self.tables.monster_catalog.by_id.insert(id, def);
                }
                None => {
                    self.tables.monster_catalog.by_id.remove(&id);
                }
            }
        }
        self.run_rules.steals.clear();
    }

    /// The steal entry the randomizer gives `monster_id` this fight.
    pub(in crate::world) fn run_rules_steal_override(
        &self,
        monster_id: u16,
    ) -> Option<legaia_game_tables::steal_table::StealEntry> {
        self.run_rules.steals.get(&monster_id).copied()
    }

    /// The item a field give (chest, gift) hands over under the randomizer:
    /// `item_id` reassigned from the sellable pool per scene and item, quest
    /// and key items kept.
    pub(crate) fn run_rules_give_item(&mut self, item_id: u8) -> u8 {
        let Some(r) = self.run_randomizer().filter(|r| r.chests).cloned() else {
            return item_id;
        };
        let pools = self.run_item_pools();
        let mut key = self.active_scene_label.as_bytes().to_vec();
        key.push(item_id);
        legaia_randomizer::sites::reassign_item(
            r.seed_value(),
            "chest",
            &key,
            item_id,
            &pools.keep,
            &pools.sellable,
        )
    }

    /// The stock a town shop sells under the randomizer: each slot of
    /// `record_ids` reassigned from the sellable pool per scene, shop and
    /// slot. `None` keeps the shop's own stock.
    ///
    /// Reads the pools [`Self::prime_run_item_pools`] built.
    pub(crate) fn run_rules_shop_stock(
        &self,
        shop_name: &str,
        record_ids: &[u8],
    ) -> Option<Vec<u8>> {
        let r = self.run_randomizer().filter(|r| r.shops)?;
        let pools = self.run_rules.pools.clone()?;
        if pools.sellable.is_empty() {
            return None;
        }
        let seed = r.seed_value();
        let mut used: Vec<u8> = Vec::new();
        Some(
            record_ids
                .iter()
                .enumerate()
                .map(|(i, &id)| {
                    let mut key = self.active_scene_label.as_bytes().to_vec();
                    key.extend_from_slice(shop_name.as_bytes());
                    key.push(i as u8);
                    // A shop lists each item once: walk the site's stream past
                    // an item an earlier slot took.
                    let mut rng = legaia_randomizer::sites::site_rng(seed, "shop", &key);
                    let mut pick = id;
                    if id != 0 && !pools.keep.contains(&id) {
                        for _ in 0..8 {
                            pick = pools.sellable[rng.below(pools.sellable.len())];
                            if !used.contains(&pick) {
                                break;
                            }
                        }
                    }
                    used.push(pick);
                    pick
                })
                .collect(),
        )
    }

    /// The fight a random encounter roll starts under the randomizer: the
    /// rolled formation's seats redrawn from the scene's own random-encounter
    /// monsters ([`legaia_randomizer::encounters::reroll_formation`]),
    /// registered as a fresh formation. Scripted fights never pass here,
    /// story bosses and the tutorial enemies never enter the draw, and a pack
    /// formation keeps the pack's monsters.
    pub(in crate::world) fn run_rules_substitute_roll(
        &mut self,
        roll: EncounterRoll,
    ) -> EncounterRoll {
        let Some(r) = self.run_randomizer().filter(|r| r.encounters).cloned() else {
            return roll;
        };
        let Some(def) = self
            .tables
            .formation_table
            .formation(roll.formation_id)
            .cloned()
        else {
            return roll;
        };
        if roll.formation_id >= 0x100 || def.slots.is_empty() {
            return roll;
        }
        use legaia_randomizer::monsters::PROTECTED_MONSTER_IDS;
        let mut pool: Vec<u16> = self
            .tables
            .formation_table
            .by_id
            .iter()
            .filter(|(id, _)| **id < 0x100)
            .flat_map(|(_, f)| f.slots.iter().map(|s| s.monster_id))
            .filter(|id| !PROTECTED_MONSTER_IDS.contains(id))
            .filter(|id| self.tables.monster_catalog.get(*id).is_some())
            .collect();
        pool.sort_unstable();
        pool.dedup();
        let slots: Vec<u16> = def.slots.iter().map(|s| s.monster_id).collect();
        let ids = legaia_randomizer::encounters::reroll_formation(
            r.seed_value(),
            &self.active_scene_label,
            roll.formation_id,
            &slots,
            &pool,
        );
        if ids == slots {
            return roll;
        }
        let id = RANDOM_FORMATION_BASE | roll.formation_id;
        let mut f = def.clone();
        f.formation_id = id;
        for (s, m) in f.slots.iter_mut().zip(ids) {
            s.monster_id = m;
        }
        self.tables.formation_table.insert(f);
        self.run_rules.random_formations.push(id);
        log::info!(
            "run rules: randomized encounter in '{}' -> {:?}",
            self.active_scene_label,
            self.tables.formation_table.formation(id).map(|f| &f.slots)
        );
        EncounterRoll {
            formation_id: id,
            ..roll
        }
    }

    /// Whether the randomizer redraws random encounters.
    pub(in crate::world) fn run_rules_rerolls_encounters(&self) -> bool {
        self.run_randomizer().is_some_and(|r| r.encounters)
    }

    /// Give Vahn a random weapon and body armour he can wear, from the
    /// disc's equipment table, keyed by the seed.
    fn randomize_starting_equipment(&mut self) {
        use legaia_game_tables::equip_stats::EquipSlot;
        let Some(r) = self.run_randomizer().cloned() else {
            return;
        };
        let Some(table) = self.tables.equip_stats.clone() else {
            return;
        };
        let pools = self.run_item_pools();
        let seed = r.seed_value();
        let Some(rec) = self.party.roster.members.first_mut() else {
            return;
        };
        let mut eq = rec.equipment();
        for (cat, byte) in [(EquipSlot::Weapon, 2usize), (EquipSlot::Body, 0)] {
            let pool: Vec<u8> = (1u8..=255)
                .filter(|&id| pools.sellable.contains(&id))
                .filter(|&id| {
                    table.bonus(id).is_some_and(|b| {
                        b.slot() == cat && b.equips_party_slot(0) && !b.is_ra_seru()
                    })
                })
                .collect();
            if let Some(id) =
                legaia_randomizer::sites::pick(seed, "start-equip", &[byte as u8], &pool)
            {
                eq.slots[byte] = id;
            }
        }
        rec.set_equipment(eq);
        self.rebuild_record_live_hp_mp(0);
        self.seed_party_battle_stats();
    }

    // ---- Ironman ----------------------------------------------------------

    /// Ironman: the party fell. Marks the run for its save's erasure (the
    /// host's, on its card) and returns whether it did.
    pub fn run_rules_party_fell(&mut self) -> bool {
        if self.run_rules.rules.ironman.is_none() {
            return false;
        }
        self.run_rules.ironman_fell = true;
        true
    }

    /// Take the ironman erase request: `Some(run_id)` once after the party
    /// fell.
    pub fn take_ironman_erase(&mut self) -> Option<u64> {
        std::mem::take(&mut self.run_rules.ironman_fell).then_some(self.run_rules.run_id)
    }

    /// A scene was entered: an ironman run owes an autosave.
    pub fn run_rules_scene_entered(&mut self) {
        self.run_rules.random_formations.clear();
        if self.run_rules.rules.ironman.is_some() && self.run_rules.boss_rush.is_none() {
            self.run_rules.autosave_due = true;
        }
    }

    /// Take the autosave request.
    pub fn take_run_autosave(&mut self) -> bool {
        std::mem::take(&mut self.run_rules.autosave_due)
    }

    /// The ironman run id a save of this world carries (`0` outside one).
    pub fn ironman_run_id(&self) -> u64 {
        self.run_rules.run_id
    }

    // ---- Save -------------------------------------------------------------

    /// The run block a save of this world carries, `None` for a retail run
    /// with no Boss Rush record - so such saves are byte-identical.
    pub fn run_rules_save_block(&self) -> Option<Vec<u8>> {
        RunSave {
            v: 0,
            rules: self.run_rules.rules.clone(),
            boss_rush: self.run_rules.boss_rush.clone(),
            record: self.run_rules.record.clone(),
            run_id: self.run_rules.run_id,
        }
        .encode()
    }

    /// Restore a loaded save's run block (`None`: it carries none, so the
    /// run is retail).
    pub fn run_rules_load_block(&mut self, block: Option<&[u8]>) {
        let s = block.and_then(RunSave::decode).unwrap_or_default();
        let pools = self.run_rules.pools.take();
        self.run_rules = RunRulesState {
            rules: s.rules,
            run_id: s.run_id,
            boss_rush: s.boss_rush,
            record: s.record,
            pools,
            ..Default::default()
        };
    }

    /// The one line the pause menu shows for the run, `None` under retail
    /// rules.
    pub fn run_rules_summary(&self) -> Option<String> {
        self.run_rules.rules.summary()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carry_copies_only_the_chosen_spans() {
        let fresh = vec![0u8; 0x414];
        let src = vec![7u8; 0x414];
        let none = CarrySet {
            levels: false,
            equipment: false,
            items: false,
            seru: false,
            arts: false,
            gold: false,
            cards: false,
        };
        assert_eq!(carry_record(&fresh, &src, &none), fresh);
        let all = carry_record(&fresh, &src, &CarrySet::default());
        assert_eq!(all[0x00], 7);
        assert_eq!(all[0xF4], 0, "ability bits are rebuilt, not copied");
        assert_eq!(all[0x130], 7);
        assert_eq!(all[0x196], 7);
        assert_eq!(
            all[0x2A7], 0,
            "the name and the rest stay the fresh record's"
        );
        let eq_only = carry_record(
            &fresh,
            &src,
            &CarrySet {
                equipment: true,
                ..none
            },
        );
        assert_eq!((eq_only[0x130], eq_only[0x196]), (0, 7));
    }
}
