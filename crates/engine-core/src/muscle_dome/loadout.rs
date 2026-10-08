//! Each dome fighter's magic / art loadout, built out of live `World`
//! state. The stat profiles and damage model this feeds live in
//! `legaia_engine_minigames::muscle_dome` (re-exported by the parent).

use super::*;

/// The equipment-slot index the Ra-Seru gate reads: `+0x199` for every
/// character but Noa, whose arm reads `+0x198`. Same pair
/// [`crate::battle_hud::battle_member_has_raseru`] carries - kept here as
/// its roster-slot twin, because a dome fighter has no battle ordinal until
/// the leg hands off to the battle.
///
/// REF: FUN_80053CB8 (`0x800541EC..0x80054258`)
pub(super) const RASERU_SLOT: usize = 3;
/// Noa's arm of the same gate.
pub(super) const RASERU_SLOT_NOA: usize = 2;
/// The roster slot Noa occupies (`DAT_8007BD10` character id `2`).
pub(super) const NOA_ROSTER_SLOT: usize = 1;

/// Build a dome fighter's [`DomeMagic`] out of a live world's roster - the
/// one door both native dome entry paths (the arena-door warp and the
/// window's own dome entry) install through, so neither grows a rule of its
/// own.
///
/// `roster_slot` is the character occupying the fighter seat; `special` is
/// the battle's [`SPECIAL_ITEM_FORBIDDEN`] / [`SPECIAL_MAGIC_FORBIDDEN`]
/// word. The learned block is the roster record's own spell list unioned
/// with anything captured this session, exactly as the regular battle's
/// magic submenu builds it (`World::build_battle_spell_session`), so a dome
/// cast offers the same rows the battle does.
///
/// Returns `None` when the roster has no such member.
pub fn magic_loadout_for(
    world: &crate::world::World,
    roster_slot: usize,
    special: u32,
) -> Option<DomeMagic> {
    let member = world.party.roster.members.get(roster_slot)?;
    let list = member.spell_list();
    let n = (list.count as usize).min(list.ids.len());
    let mut learned: Vec<u8> = list.ids[..n].to_vec();
    for &sid in world.seru.log.learned_spells(roster_slot as u8) {
        if !learned.contains(&sid) {
            learned.push(sid);
        }
    }
    let spells: Vec<crate::spells::SpellDef> = learned
        .iter()
        .filter_map(|id| world.tables.spell_catalog.get(*id).cloned())
        .collect();
    let live = member.live_stats();
    let gauge = member.hp_mp_sp();
    let slot = if roster_slot == NOA_ROSTER_SLOT {
        RASERU_SLOT_NOA
    } else {
        RASERU_SLOT
    };
    let has_raseru = member.equipment().slots[slot] != 0;
    Some(DomeMagic {
        ring: DomeRing {
            special,
            // A dome fighter enters the leg unafflicted: the status halfword
            // is a battle actor's, and the leg's actors are staged by the
            // battle the arena hands off to.
            status: 0,
            has_raseru,
        },
        mp: gauge.mp_cur,
        mp_max: gauge.mp_max,
        ability_bits: world
            .party
            .character_ability_bits
            .get(roster_slot)
            .copied()
            .unwrap_or(0) as u8,
        magic_power: live.int,
        spells,
    })
}

/// One fighter's **normal-art catalog** for [`MuscleDomeSession::install_art_catalog`],
/// filtered out of a world's art records the way the retail queue builder's
/// inner loop filters them: this character's rows only, the **normal** arts
/// only, and combos of two arrows or more.
///
/// "Normal" is the constant band `>= 0x1F`. The builder routes ordinals
/// `0..=3` - the Miracle Art and the three Hyper Arts - through a different
/// arm (`sltiu a1,a0,0x4` at `0x801EF330`) that, with the slot's `+0x25F`
/// marker clear, writes nothing, so their combo bytes never tokenize as arts.
/// The two-arrow floor is the builder's `s1 == 1` exit
/// (`0x801EF420..0x801EF434`): a fully matched one-arrow string is left
/// unrewritten, and letting one match would steal an arrow from every art
/// containing it. Sorted by constant, the grid order the loop walks.
///
/// Both dome hosts build their catalog through this one filter so neither can
/// grow a rule of its own.
///
/// REF: FUN_801EED1C (`0x801EF330`, `0x801EF420..0x801EF434`)
/// Lowest action constant that is a **normal** art - the band the queue
/// builder's inner loop tokenizes. Below it sit the Miracle Art and the three
/// Hyper Arts, which the builder routes elsewhere. The battle command flow's
/// own queue builder holds the same bound for the same reason.
pub(super) const NORMAL_ART_MIN_CONSTANT: u8 = 0x1F;

pub fn art_catalog_for(
    records: &std::collections::HashMap<
        (legaia_art::Character, legaia_art::ActionConstant),
        legaia_art::ArtRecord,
    >,
    character: legaia_art::Character,
) -> Vec<(legaia_art::ActionConstant, Vec<legaia_art::Command>)> {
    let mut rows: Vec<(legaia_art::ActionConstant, Vec<legaia_art::Command>)> = records
        .iter()
        .filter(|((ch, action), rec)| {
            *ch == character
                && action.is_art()
                && action.as_byte() >= NORMAL_ART_MIN_CONSTANT
                && rec.commands.len() >= 2
        })
        .map(|((_, action), rec)| (*action, rec.commands.clone()))
        .collect();
    rows.sort_by_key(|(a, _)| a.as_byte());
    rows
}

/// Map a dealt direction's action byte (`0x0C` Left, `0x0D` Right, `0x0E`
/// Down, `0x0F` Up - the deck table `DAT_801f4b8c`'s ids) onto the arrow the
/// tokenizer reads. `None` for anything outside that band.
pub(super) fn dome_command_of_action_byte(b: u8) -> Option<legaia_art::Command> {
    match b {
        0x0C => Some(legaia_art::Command::Left),
        0x0D => Some(legaia_art::Command::Right),
        0x0E => Some(legaia_art::Command::Down),
        0x0F => Some(legaia_art::Command::Up),
        _ => None,
    }
}
