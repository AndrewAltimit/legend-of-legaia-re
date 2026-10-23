//! Player-driven battle command menu and the Arts / Magic / Item submenu
//! drivers.

use super::*;

mod arts_action;
mod arts_input;
mod command_ring;
mod party_dispatch;
mod spell_item_menus;
pub mod ui_cues;

/// The first **normal**-art action constant (art ordinal `4`). The queue
/// builder `FUN_801EED1C` tokenizes only ordinals `>= 4`: the Miracle Art
/// (ordinal `0`) and the three Hyper Arts (`1..=3`) take its other arm
/// (`sltiu a1,a0,0x4` at `0x801EF330`), which writes nothing unless the
/// slot's `+0x25F` Miracle marker is armed.
const NORMAL_ART_MIN_CONSTANT: u8 = 0x1F;

/// The bias between an art's action-queue byte and its id in the character
/// record's learned-art list `+0x185..`. The builder `FUN_801EED1C` walks
/// the art grid with `s3` from `0xB` (`li s3,0xb` at `0x801EF2EC`), writes
/// the matched art's queue byte as `s3 + 0x10` (`addiu v0,s3,0x10` at
/// `0x801EF794`) and hands the learn check `s3 - 0xB` (`addiu a1,s3,-0xb`
/// at `0x801EF438`): list id = queue byte - `0x1B`, so the first normal art
/// `0x1F` is id `4` and the Hyper Arts sit below it.
const ART_RECORD_ID_BIAS: u8 = 0x1B;

/// Whether the command ring refuses `arm` for an actor whose `+0x16E` word is
/// `status`. `FUN_801D0748`'s ring state tests the word on the press itself
/// and answers a refusal with cue `0x23` (`0x801D2968`):
///
/// - **Attack** (Left) with all three Rot limbs set, `status & 0x38 == 0x38`
///   (`0x801D1560..0x801D156C`) - the arts entry would have no direction left;
/// - **Magic** (Right) under Curse, `status & 0x1000` (`0x801D1434..0x801D1440`).
///
/// The same two tests pick the ring's crosses (`FUN_801DBD04(0xA0, 0x42)` at
/// `0x801D1328`, `FUN_801DBEC4(0xF8, 0x42)` at `0x801D135C`). The Magic arm's
/// other refusals - no Ra-Seru bound (`ctx[+0x25F + member] == 0`) and the
/// special word's `0x200` - are not status bits.
///
/// PORT: FUN_801D0748 (the ring's status refusals, `0x801D1434..0x801D1440` / `0x801D1560..0x801D156C`)
pub(crate) fn ring_arm_refused(
    status: u16,
    arm: legaia_engine_battle_menus::battle_input::BattleCommand,
) -> bool {
    use legaia_engine_battle_menus::battle_input::BattleCommand;
    match arm {
        BattleCommand::Attack => status & 0x38 == 0x38,
        BattleCommand::Magic => status & 0x1000 != 0,
        _ => false,
    }
}

/// The spirit clip a Spirit commit stages behind the action (`+0x1E7`,
/// `li v0,0x10` at `0x801D16A8`); the item commit's is `9`.
pub(crate) const SPIRIT_COMMIT_CLIP: u8 = 0x10;

/// The clip the ring's **Item** and **Magic** arms stage behind the action
/// (`+0x1E7`): `li v0,0x9` / `sb v0,0x1e7(v1)` at `0x801D13E4..0x801D13E8`
/// (Item, beside `+0x1DE = 1`) and `0x801D14BC..0x801D14C0` (Magic, beside
/// `+0x1DE = 2`). Action state `0x3C` stages whatever the byte holds as the
/// queued clip (`lbu v0,0x1e7(s3)` / `sb v0,0x1da(s3)` at
/// `0x801E3B4C..0x801E3B54`), and nothing between a Spirit turn and the next
/// commit clears it - so an arm that does not write its own value inherits
/// the Spirit clip and the Spirit clip's cue track.
pub(crate) const CAST_COMMIT_CLIP: u8 = 9;

/// The entry command a saved swing byte stands for: `0x0C..=0x0F` are Left,
/// Right, Down, Up (`legaia_art::Command::as_action`), anything else ends
/// the string.
fn swing_byte_command(b: u8) -> Option<legaia_art::Command> {
    use legaia_art::Command;
    [Command::Left, Command::Right, Command::Down, Command::Up]
        .into_iter()
        .find(|c| c.as_action().as_byte() == b)
}

#[cfg(test)]
mod ap_used_down_tests {
    use crate::world::World;

    /// A one-member battle world with `bits` installed as the character
    /// record's `+0xF4..` ability bitfield, ready for an arts commit.
    fn world_with_ability_bits(bits: [u8; legaia_save::ABILITY_BITS_LEN]) -> World {
        let mut w = World::new();
        while w.actors.len() < 4 {
            w.actors.push(crate::world::Actor::default());
        }
        w.party.party_count = 1;
        let mut party = legaia_save::Party::zeroed(1);
        party.members[0].set_ability_bits(bits);
        w.load_party(party);
        for a in w.actors.iter_mut().take(4) {
            a.active = true;
            a.battle.hp = 500;
            a.battle.max_hp = 500;
            a.battle.liveness = 1;
            a.battle.spirit_gauge = 100;
        }
        w
    }

    /// Word 1 (`+0xF8`) is where the halving flag lives, not word 0
    /// (`+0xF4`, the War God Icon's word) and not the battle actor.
    #[test]
    fn the_halving_flag_reads_record_word_one() {
        let mut bits = [0u8; legaia_save::ABILITY_BITS_LEN];
        bits[4..8].copy_from_slice(&legaia_engine_battle::ap_gauge::AP_USED_DOWN_BIT.to_le_bytes());
        let w = world_with_ability_bits(bits);
        assert_eq!(
            w.character_ability_bits_word1(0) & legaia_engine_battle::ap_gauge::AP_USED_DOWN_BIT,
            legaia_engine_battle::ap_gauge::AP_USED_DOWN_BIT
        );

        // The same value in word 0 is a different passive entirely.
        let mut wrong = [0u8; legaia_save::ABILITY_BITS_LEN];
        wrong[0..4]
            .copy_from_slice(&legaia_engine_battle::ap_gauge::AP_USED_DOWN_BIT.to_le_bytes());
        let w = world_with_ability_bits(wrong);
        assert_eq!(w.character_ability_bits_word1(0), 0);
    }

    /// The passive bit is index `0x2B` of the accessory-passive space -
    /// word `index >> 5`, bit `1 << (index & 0x1F)`.
    #[test]
    fn the_passive_index_and_the_bit_agree() {
        let (word, mask) = legaia_game_tables::accessory_passive::bit_location(
            legaia_engine_battle::ap_gauge::AP_USED_DOWN_PASSIVE,
        );
        assert_eq!(word, 1);
        assert_eq!(mask, legaia_engine_battle::ap_gauge::AP_USED_DOWN_BIT);
    }

    /// End to end: the same committed arts turn costs half the Spirit when
    /// the caster carries the passive, because the *multiplier* is shifted
    /// before the command-count multiply (`srl t4,t4,0x1` at `0x801EF378`).
    #[test]
    fn the_passive_halves_a_committed_turns_spirit_charge() {
        let catalog = [(legaia_art::ActionConstant::Art1B, 3u8)];
        let performed = [legaia_art::ActionConstant::Art1B];
        let full =
            legaia_engine_battle::ap_gauge::arts_turn_spirit_cost(&catalog, &performed, false);
        let half =
            legaia_engine_battle::ap_gauge::arts_turn_spirit_cost(&catalog, &performed, true);
        assert_eq!(full, 33, "11 x 3");
        assert_eq!(half, 15, "(11 >> 1) x 3 - the shift is on the multiplier");
        assert_ne!(full, half * 2, "which is why it is not simply half");
    }
}
