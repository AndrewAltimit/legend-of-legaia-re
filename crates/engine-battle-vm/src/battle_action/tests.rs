#![allow(clippy::field_reassign_with_default)]

use super::*;
use std::cell::RefCell;

mod arts;
mod attack;
mod done;
mod end_of_action;
mod hp_bar_drain;
mod magic;
mod round_trips;
mod run_capture;
mod seed;
mod small_party;
mod spirit;
mod target_cursor;

/// Recording host. Captures every callback so tests can assert exact
/// dispatch order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Pose(u8, Pose),
    Ui(u8, u8),
    PartySetup(u8),
    MonsterSetup(u8),
    Camera,
    SpellAnim(u8, u8),
    SpellSustain(u8, u8),
    ApplyDamage(u8, u8, u8, u8),
    ApplyArtStrike(ArtStrikeInfo),
    Brightness(u8),
    BattleEnd(BattleEndCause),
    LoadCapture(u8),
    Recompute,
    VictoryStage(u8),
    Sfx(u16),
    CueSpawn(u8),
}

#[derive(Default)]
struct RecHost {
    actors: Vec<BattleActor>,
    events: RefCell<Vec<Event>>,
    capture_spells: std::collections::HashSet<u8>,
    spell_costs: std::collections::HashMap<u8, u8>,
    /// Spell-table `+0` class bytes. Absent = "no table for this id", which
    /// is what a host without a disc image reports.
    spell_classes: std::collections::HashMap<u8, u8>,
    ability_bits: std::collections::HashMap<u8, u32>,
    /// Record word one (`+0xF8`) per party slot - the auto-fill passive bit.
    ability_bits_high: std::collections::HashMap<u8, u32>,
    ranges: std::collections::HashMap<(u8, u8), u16>,
    prev_cleared: bool,
    sound_ready: bool,
    rng_seq: Vec<u32>,
    rng_pos: RefCell<usize>,
    pad_word: u16,
    party_count: u8,
    slot_count: u8,
    first_monster_id: u8,
    /// Pre-staged art records returned by `art_record(character, action)`
    /// - keyed by `(character_byte, action_byte)`.
    art_records: std::collections::HashMap<(u8, u8), legaia_art::ArtRecord>,
    /// Per-slot equipment ATK bonus bytes returned by
    /// `equip_attack_bonuses`.
    equip_atk: std::collections::HashMap<u8, [u8; 5]>,
    /// Party slots reported as NOT seated by `slot_seated` (empty = every
    /// slot seated, the trait default).
    unseated: std::collections::HashSet<u8>,
    /// How many more times `capture_stager_tick` reports busy - the stand-in
    /// for a resident slot-B module that stages for N frames.
    capture_busy_frames: RefCell<u32>,
    /// How many times `capture_stager_tick` was entered.
    capture_ticks: RefCell<u32>,
    /// Per-slot monster action-tag tables (`monster_action_tags`); absent =
    /// the host resolves no record for that slot.
    monster_tags: std::collections::HashMap<u8, Vec<u8>>,
    /// `_DAT_8007BAC0` as `special_battle_word` reports it.
    special_word: u32,
    /// The strike queue `begin_counterattack` stages on the counterer
    /// (`None` = the host cannot stage a counter).
    counter_queue: Option<Vec<u8>>,
    /// Status bits a typed tracker would pack into `+0x16E` on top of the
    /// actor view's `field_flags` (`status_word`).
    packed_status: std::collections::HashMap<u8, u16>,
    /// `(groups, clut_map)` as `cue_tables` reports them (`None` = no
    /// battle-action overlay, the trait default).
    cue_tables: Option<(Vec<u8>, Vec<u8>)>,
    /// `spirit_turn_ap`'s answer (`None` = the trait default, retail).
    spirit_ap: Option<i16>,
}

impl RecHost {
    fn with_n_actors(n: usize) -> Self {
        Self {
            actors: (0..n).map(|_| BattleActor::new()).collect(),
            prev_cleared: true,
            sound_ready: true,
            party_count: 3,
            slot_count: ACTOR_SLOTS as u8,
            ..Default::default()
        }
    }
    fn record(&self, e: Event) {
        self.events.borrow_mut().push(e);
    }
    fn take(&self) -> Vec<Event> {
        std::mem::take(&mut self.events.borrow_mut())
    }
}

impl BattleActionHost for RecHost {
    fn actor(&self, slot: u8) -> Option<&BattleActor> {
        self.actors.get(slot as usize)
    }
    fn slot_seated(&self, slot: u8) -> bool {
        !self.unseated.contains(&slot)
    }
    fn actor_mut(&mut self, slot: u8) -> Option<&mut BattleActor> {
        self.actors.get_mut(slot as usize)
    }
    fn pose(&mut self, actor_id: u8, pose: Pose) {
        self.record(Event::Pose(actor_id, pose));
    }
    fn ui_element(&mut self, effect_id: u8, mode: u8) {
        self.record(Event::Ui(effect_id, mode));
    }
    fn begin_counterattack(&mut self, counterer: u8, _attacker: u8) -> bool {
        let Some(q) = self.counter_queue.clone() else {
            return false;
        };
        self.record(Event::Ui(0x66, 0));
        let c = &mut self.actors[counterer as usize];
        c.params = [0; ACTION_PARAM_BYTES];
        c.params[..q.len()].copy_from_slice(&q);
        c.strike_index = 0;
        c.action_category = ActionCategory::Attack.as_byte();
        true
    }
    fn range_check(&self, a: u8, t: u8) -> u16 {
        self.ranges.get(&(a, t)).copied().unwrap_or(0)
    }
    fn seed_formation_recentre(&mut self, _actor: u8) {
        self.record(Event::Camera);
    }
    fn party_setup(&mut self, s: u8) {
        self.record(Event::PartySetup(s));
    }
    fn monster_setup(&mut self, s: u8) {
        self.record(Event::MonsterSetup(s));
    }
    fn recompute_battle_order(&mut self) {
        self.record(Event::Recompute);
    }
    fn first_monster_id(&self) -> u8 {
        self.first_monster_id
    }
    fn special_battle_word(&self) -> u32 {
        self.special_word
    }
    fn status_word(&self, slot: u8) -> u16 {
        self.actor(slot).map_or(0, |a| a.field_flags)
            | self.packed_status.get(&slot).copied().unwrap_or(0)
    }
    fn monster_action_tags(&self, slot: u8) -> Option<Vec<u8>> {
        self.monster_tags.get(&slot).cloned()
    }
    fn victory_stage(&mut self, party_slot: u8) {
        self.record(Event::VictoryStage(party_slot));
    }
    fn pad_word(&self) -> u16 {
        self.pad_word
    }
    fn rng(&mut self) -> u32 {
        let mut p = self.rng_pos.borrow_mut();
        let v = self.rng_seq.get(*p).copied().unwrap_or(0);
        *p += 1;
        v
    }
    fn previous_action_cleared(&self, _: u8) -> bool {
        self.prev_cleared
    }
    fn sound_bank_ready(&self, _: u8) -> bool {
        self.sound_ready
    }
    fn load_capture_archive(&mut self, idx: u8) {
        self.record(Event::LoadCapture(idx));
    }
    fn capture_stager_tick(&mut self) -> bool {
        *self.capture_ticks.borrow_mut() += 1;
        let mut left = self.capture_busy_frames.borrow_mut();
        if *left == 0 {
            return false;
        }
        *left -= 1;
        true
    }
    fn spell_anim_trigger(&mut self, p: u8, s: u8) {
        self.record(Event::SpellAnim(p, s));
    }
    fn spell_anim_sustain(&mut self, a: u8, anim: u8) {
        self.record(Event::SpellSustain(a, anim));
    }
    fn apply_damage(&mut self, a: u8, b: u8, c: u8, d: u8) {
        self.record(Event::ApplyDamage(a, b, c, d));
    }
    fn apply_art_strike(&mut self, info: ArtStrikeInfo) {
        self.record(Event::ApplyArtStrike(info));
    }
    fn one_shot_sfx(&mut self, cue_id: u16) {
        self.record(Event::Sfx(cue_id));
    }
    fn cue_tables(&self) -> Option<(&[u8], &[u8])> {
        self.cue_tables
            .as_ref()
            .map(|(g, c)| (g.as_slice(), c.as_slice()))
    }
    fn spawn_cue(&mut self, actor_slot: u8, _spawn: crate::battle_cue_group::CueSpawn) {
        self.record(Event::CueSpawn(actor_slot));
    }
    fn equip_attack_bonuses(&self, party_slot: u8) -> [u8; 5] {
        self.equip_atk.get(&party_slot).copied().unwrap_or([0; 5])
    }
    fn art_record(
        &self,
        character: legaia_art::Character,
        action: legaia_art::ActionConstant,
    ) -> Option<&legaia_art::ArtRecord> {
        self.art_records
            .get(&(character_byte(character), action.as_byte()))
    }
    fn spell_class_byte(&self, id: u8) -> Option<u8> {
        self.spell_classes.get(&id).copied()
    }
    fn is_capture_spell(&self, id: u8) -> bool {
        self.capture_spells.contains(&id)
    }
    fn spell_mp_cost(&self, id: u8) -> u8 {
        self.spell_costs.get(&id).copied().unwrap_or(0)
    }
    fn character_ability_bits(&self, slot: u8) -> u32 {
        self.ability_bits.get(&slot).copied().unwrap_or(0)
    }
    fn character_ability_bits_high(&self, slot: u8) -> u32 {
        self.ability_bits_high.get(&slot).copied().unwrap_or(0)
    }
    fn spirit_turn_ap(&self) -> i16 {
        self.spirit_ap.unwrap_or(SPIRIT_AP_RETAIL)
    }
    fn duck_audio_level(&mut self, p: u8) {
        self.record(Event::Brightness(p));
    }
    fn battle_end(&mut self, c: BattleEndCause) {
        self.record(Event::BattleEnd(c));
    }
    fn frame_dt(&self) -> i16 {
        1
    }
    fn party_count(&self) -> u8 {
        self.party_count
    }
    fn slot_count(&self) -> u8 {
        self.slot_count
    }
}

/// Cheap byte encoding for tests. `Character` is a 3-variant enum with
/// no public byte-mapping accessor - this mirrors the `0/1/2` ordering
/// of `Character::all()`.
fn character_byte(c: legaia_art::Character) -> u8 {
    match c {
        legaia_art::Character::Vahn => 0,
        legaia_art::Character::Noa => 1,
        legaia_art::Character::Gala => 2,
    }
}

fn fresh(category: ActionCategory, slot: u8) -> (BattleActionCtx, RecHost) {
    let mut host = RecHost::with_n_actors(ACTOR_SLOTS);
    // Mark all slots alive.
    for a in &mut host.actors {
        a.liveness = 1;
    }
    host.actors[slot as usize].action_category = category.as_byte();
    let mut ctx = BattleActionCtx::new();
    ctx.active_actor = slot;
    (ctx, host)
}
