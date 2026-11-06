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
//! ([`legaia_engine_menus::spell_party_broadcast::broadcast`]), whose three `jal` sites on
//! the disc are all menu code: the Magic list builder (`FUN_80030628`,
//! `0x80031210`) and the two cast confirms `FUN_801D9280` / `FUN_801D9594`
//! (`0x801D954C` / `0x801D98B4`, PROT 0899). [`spell_affects_anyone`] is
//! that question for one spell id.
//!
//! REF: FUN_8003FB10 (the validator, ported as
//! `legaia_engine_battle_vm::battle_action::validate_action`)

use legaia_engine_battle_vm::battle_action::{
    ActionValidatorHost, RecordStat, SlotResources, validate_action,
};

use legaia_engine_core::world::World;
use legaia_engine_menus::spell_party_broadcast::{BroadcastRoster, SpellDispatchRecord, broadcast};

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
    ///
    /// Out of battle the validator's setup loop fills its pointer arrays for
    /// records `0..3` alone (`sltiu v0,v0,0x3` at `0x8003FC38`), so a slot
    /// past them - a present id of `3`, reached by arm `0x01`'s party walk -
    /// reads stale stack, not a record. That is the trait's `None`, the dead
    /// quad, never the fourth record's HP.
    fn slot_resources(&self, slot: u8) -> Option<SlotResources> {
        if slot >= 3 {
            return None;
        }
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
        present_list(self.world).len() as u8
    }

    /// `(&DAT_80084598)[index]`.
    fn party_member_slot(&self, index: u8) -> u8 {
        present_list(self.world)
            .get(usize::from(index))
            .copied()
            .unwrap_or(index)
    }

    /// `_DAT_1F800394`'s two scene gates, bits `0x100000` / `0x200000`: the
    /// region reader raises them, and arms `0x80` / `0x81` (Door of Light,
    /// Door of Wind) refuse while they stand.
    fn engine_flag_word(&self) -> u32 {
        let (light, wind) = self.world.region_door_gates();
        (u32::from(light) << 20) | (u32::from(wind) << 21)
    }

    /// `FUN_8003CE64` over the system-flag bank `DAT_80085758`, the bank the
    /// field VM's flag ops write (arms `0x80` / `0x81` test flags `5` / `6`).
    fn system_flag(&self, idx: u16) -> bool {
        self.world.system_flag_test(idx)
    }

    /// `_DAT_8007B600`, the Incense window (arm `0x82`).
    fn inventory_count(&self) -> i32 {
        self.world.locomotion.walk_regen_window
    }
}

/// The present-party list `0x80084598` for `DAT_80084594` entries
/// ([`World::present_party_list`]). Retail never runs the menu with it
/// empty; a world whose host never seated a present party falls back to the
/// roster in order.
fn present_list(world: &World) -> Vec<u8> {
    let list = world.present_party_list();
    if list.is_empty() {
        (0..world.party.roster.members.len().min(3) as u8).collect()
    } else {
        list
    }
}

/// The `FUN_8003053C` / `FUN_8003043C` broadcast of one `[arm, sub, flags]`
/// record over the present party, with the validator run against the roster
/// records.
fn record_affects_anyone(world: &World, rec: SpellDispatchRecord) -> bool {
    let slot_ids = present_list(world);
    let roster = BroadcastRoster {
        count: slot_ids.len() as u8,
        slot_ids,
    };
    let mut host = RosterValidator::new(world);
    let mut bits = 0u8;
    broadcast(rec, &roster, |arm, sub, slot| {
        u32::from(validate_action(&mut host, arm, sub, slot, &mut bits))
    }) != 0
}

/// Whether using item `id` from the field Items screen would do anything -
/// the Use list's applicability probe, which greys a row it answers `false`
/// for.
///
/// `FUN_8003043C` is the spell broadcast's twin over the item tables: the
/// item record's `+1` subtype (`DAT_80074369 + id * 0xC`) selects a 4-byte
/// effect descriptor at `DAT_800752C0`, whose `+0` / `+1` go to the validator
/// as its arm and sub-case and whose `+2` bit `0x20` makes one call on slot
/// `0` instead of one per present member (`0x8003043C..0x80030538`). So the
/// item answers exactly what the validator answers: an MP restore refuses a
/// fallen member, a stat seed refuses a member already at the stat's cap, a
/// Door refuses where the scene gates or flags `5` / `6` stand. `None` when
/// the disc item-effect table is not installed.
///
/// PORT: FUN_8003043c
pub fn item_affects_anyone(world: &World, id: u8) -> Option<bool> {
    let effect = world.tables.item_effects.as_ref()?.effect(id)?;
    Some(record_affects_anyone(
        world,
        SpellDispatchRecord {
            arg0: effect.class,
            arg1: effect.tier,
            flags: effect.flags,
        },
    ))
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
    Some(record_affects_anyone(
        world,
        SpellDispatchRecord {
            arg0: entry.class,
            arg1: entry.sub_class,
            flags: entry.target,
        },
    ))
}
