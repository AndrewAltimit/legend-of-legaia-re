//! The **out-of-battle** host of the action validator `FUN_8003FB10`, and the
//! pause Magic screen's "would this spell do anything" question built on it.
//!
//! The validator reads its resources from one of two places, chosen by the
//! game-mode word: the battle-actor table in battle, the character records
//! (`0x80084708 + slot*0x414`, HP/MP pairs at `+0x104..+0x10A`, status word
//! `+0x12E`) everywhere else. `World`'s battle host
//! (`world/battle/validator_host.rs`) answers the first; [`RosterValidator`]
//! answers the second, over the roster records the pause menu edits.
//!
//! Its consumer is the spell-record broadcast `FUN_8003053C`
//! ([`crate::spell_party_broadcast::broadcast`]), whose three `jal` sites on
//! the disc are all menu code: the Magic list builder (`FUN_80030628`,
//! `0x80031210`) and the two cast confirms `FUN_801D9280` / `FUN_801D9594`
//! (`0x801D954C` / `0x801D98B4`, PROT 0899). [`spell_affects_anyone`] is
//! that question for one spell id.
//!
//! REF: FUN_8003FB10 (the validator, ported as
//! `legaia_engine_vm::battle_action::validate_action`)

use legaia_engine_vm::battle_action::{
    ActionValidatorHost, RecordStat, SlotResources, validate_action,
};

use crate::spell_party_broadcast::{BroadcastRoster, SpellDispatchRecord, broadcast};
use crate::world::World;

/// [`ActionValidatorHost`] over the roster records - the validator's
/// out-of-battle resource source.
pub struct RosterValidator<'a> {
    world: &'a World,
}

impl<'a> RosterValidator<'a> {
    pub fn new(world: &'a World) -> Self {
        Self { world }
    }
}

impl ActionValidatorHost for RosterValidator<'_> {
    /// The pause menu never runs in battle (`_DAT_8007B83C != 0x15`).
    fn in_battle(&self) -> bool {
        false
    }

    /// Record `+0x106` / `+0x104` / `+0x10A` / `+0x108`.
    fn slot_resources(&self, slot: u8) -> Option<SlotResources> {
        let hms = self
            .world
            .party
            .roster
            .members
            .get(slot as usize)?
            .hp_mp_sp();
        Some(SlotResources {
            hp: hms.hp_cur,
            hp_max: hms.hp_max,
            mp: hms.mp_cur,
            mp_max: hms.mp_max,
        })
    }

    /// Record `+0x12E`, the packed ailment word - kept by the engine as the
    /// status tracker's `display_flags` (the Status screen reads the same).
    fn status_word(&self, slot: u8) -> u16 {
        self.world.battle.status_effects.display_flags(slot)
    }

    /// The arm-`0x06` stat-cap walker's record reads.
    fn record_stat(&self, slot: u8, stat: RecordStat) -> u16 {
        let Some(rec) = self.world.party.roster.members.get(slot as usize) else {
            return 0;
        };
        let hms = rec.hp_mp_sp();
        let live = rec.live_stats();
        match stat {
            RecordStat::HpMax => hms.hp_max,
            RecordStat::MpMax => hms.mp_max,
            RecordStat::Agl => live.agl,
            RecordStat::Atk => live.atk,
            RecordStat::Udf => live.udf,
            RecordStat::Ldf => live.ldf,
            RecordStat::Spd => live.spd,
            RecordStat::Int => live.int,
        }
    }

    /// `DAT_80084594`.
    fn party_count(&self) -> u8 {
        present_count(self.world)
    }

    /// `(&DAT_80084598)[index]`.
    fn party_member_slot(&self, index: u8) -> u8 {
        self.world.party_roster_slot(usize::from(index)) as u8
    }

    /// `_DAT_8007B600`, the Incense window (arm `0x82`).
    fn inventory_count(&self) -> i32 {
        self.world.locomotion.walk_regen_window
    }
}

/// The present-party count `DAT_80084594`. Retail never runs the menu with
/// it at zero; a world whose host never seated a present party falls back to
/// the roster in order, the same fallback [`World::party_roster_slot`] takes.
fn present_count(world: &World) -> u8 {
    match world.party.party_count {
        0 => world.party.roster.members.len().min(3) as u8,
        n => n.min(3),
    }
}

/// Whether casting `spell_id` from the pause menu would do anything - the
/// `FUN_8003053C` broadcast over the present party, with the validator run
/// against the roster records.
///
/// The record bytes are the static spell table's `+0` / `+1` (forwarded to
/// the validator as its arm and sub-case) and `+2` (bit `0x20` = one call on
/// slot 0 instead of one per member). `None` when the disc spell table is
/// not installed - the question has no data to answer from, and the caller
/// keeps its own gate.
pub fn spell_affects_anyone(world: &World, spell_id: u8) -> Option<bool> {
    let entry = world
        .menu
        .text
        .as_ref()?
        .spell_names
        .as_ref()?
        .entry(spell_id)?;
    let rec = SpellDispatchRecord {
        arg0: entry.class,
        arg1: entry.sub_class,
        flags: entry.target,
    };
    let count = present_count(world);
    let roster = BroadcastRoster {
        count,
        slot_ids: (0..count)
            .map(|i| world.party_roster_slot(usize::from(i)) as u8)
            .collect(),
    };
    let mut host = RosterValidator::new(world);
    let mut bits = 0u8;
    let hit = broadcast(rec, &roster, |arm, sub, slot| {
        u32::from(validate_action(&mut host, arm, sub, slot, &mut bits))
    });
    Some(hit != 0)
}
