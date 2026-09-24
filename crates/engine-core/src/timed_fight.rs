//! Koru's **timed fight**: the `Turns Left / HP Left` strip and the turn
//! limit behind it.
//!
//! ## The gate is a monster id
//!
//! Every draw site of the strip in the battle overlay (PROT 0898) tests the
//! first byte of the formation cell `0x8007BD0C` against `0xB6` before it does
//! anything else (`0x801D0F18` in the round-start arm, `0x801D11EC` in the
//! ring's cancel arm, `0x801D30E4` in the commit-confirm `Reselect` arm).
//! `0x8007BD0C` is the four-slot monster-id cell the encounter reader fills,
//! so the gate reads "the formation's first monster is `0xB6`", and monster
//! `0xB6` in the monster archive (PROT 0867) is **Koru**.
//!
//! ## The turn limit is Koru's own script
//!
//! No code compares the round counter `ctx[+0x28A]` against a bound and ends
//! the battle. The limit is Koru's per-monster AI arm: the monster-AI switch
//! of the battle overlay indexes its jump table at `0x801CF1CC` by
//! `formation_cell[slot] - 4` (`0x801EA9C0..0x801EA9FC`), entry `178` is Koru's
//! (`0x801EB52C`), and that arm dispatches on the round counter
//! (`sltiu v0,v1,5` at `0x801EB540`, table `0x801CF49C`): rounds `0..=3` cast
//! Koru's four set pieces (spell ids `0xA2`, `0xA3`, `0xA4`, `0xA5`) and round
//! `4` casts `0xA1`, the all-party finisher. That arm is already the engine's
//! (`crate::monster_ai::decide`, case `0xB6`), driven off the same round
//! counter (`World::battle_mode`), so the limit runs wherever a Koru fight
//! runs; what this module adds is the readout.
//!
//! ## The strip
//!
//! The round-start arm (`FUN_801D0748` state `0x14`, `0x801D0F0C..0x801D1028`)
//! computes both numbers and registers them:
//!
//! * `DAT_801F6958 = 4 - ctx[+0x28A]` (`0x801D0F9C..0x801D0FA4`), one digit;
//! * `DAT_801F6959 = hp * 100 / max_hp` of actor-table entry 3 - the first
//!   **monster** seat - through the shift-add chain at `0x801D0F38..0x801D0F4C`,
//!   three digits;
//! * `FUN_8003541C(1, 0, 0x801CE818, 0x10, 0x0E, 0x120, 0x0C, 0x44)` - text
//!   actor key `1`, the format string at the head of PROT 0898 (file offset
//!   `0x0`), a `288 x 12` box at `(16, 14)`, the same style word the
//!   non-waiting tutorial boxes register with;
//! * two `FUN_8003563C(1, &value, 1, x, 0, digits, 7)` records on the same
//!   key, at `x = 0x68` (one digit) and `x = 0xD2` (three).
//!
//! The ring's cancel back to the round prompt and the commit confirm's
//! `Reselect` re-register the same strip from the two stored bytes
//! (`0x801D11F8..0x801D1274`, `0x801D30F4..0x801D317C`).
//!
//! **Lifetime, from the bytes.** The strip is a text actor on the `gp+0x148`
//! list, and that list is drained whole by `FUN_800355F0` in two places: the
//! intro countdown arm right before it stores `0x14` (`0x801D0EB4`), and
//! `FUN_801D99BC` (`0x801D9A24`), which the `0xFE` arm calls as the round
//! starts to play out (`0x801D31E8`). So the strip is up from each round's
//! start until `Begin` is taken - the whole command phase - and gone for the
//! round's action playback. That is [`strip_visible`].
//!
//! PORT: FUN_801D0748 (state `0x14`'s strip arm, `0x801D0F0C..0x801D1028`)

/// Monster id the strip's draw sites gate on (`*(u8*)0x8007BD0C == 0xB6`):
/// Koru.
pub const TIMED_FIGHT_MONSTER_ID: u8 = 0xB6;

/// The numerator of the `Turns Left` digit: the strip prints
/// `4 - ctx[+0x28A]` (`0x801D0F9C..0x801D0FA4`).
pub const TIMED_FIGHT_TURN_LIMIT: u32 = 4;

/// The `HP Left` readout's scale: `hp * 100 / max_hp`, a plain percentage
/// (the MIPS shift-add chain `((hp<<1 + hp)<<3 + hp)<<2`).
pub const HP_LEFT_SCALE: u32 = 100;

// The strip's geometry (the registered rect and the two number offsets) is
// the draw half's: `legaia_engine_ui::battle_timed_fight_strip`.

/// The timed fight's `Turns Left` digit for round counter `turn`
/// (`ctx[+0x28A]`), floored at zero.
///
/// Retail stores the byte `4 - turn` unclamped; the fight never reaches a
/// counter above four with Koru standing, because round `4` is the finisher
/// (`crate::monster_ai::decide`, case `0xB6`), so the floor only shows on a
/// party that survives it.
pub fn timed_fight_turns_left(turn: u32) -> u32 {
    TIMED_FIGHT_TURN_LIMIT.saturating_sub(turn)
}

/// The `HP Left` percentage for one monster seat's live `hp` / `max_hp`
/// (`0` for an unseated or zero-max record rather than retail's divide trap).
pub fn hp_left_percent(hp: u16, max_hp: u16) -> u32 {
    if max_hp == 0 {
        return 0;
    }
    u32::from(hp) * HP_LEFT_SCALE / u32::from(max_hp)
}

/// One frame's strip readout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedFightStrip {
    /// The format string off the user's PROT 0898 (the two field labels).
    pub label: String,
    /// `4 - ctx[+0x28A]`, floored.
    pub turns_left: u32,
    /// The first monster seat's HP percentage.
    pub hp_left: u32,
}

/// `true` while the battle's command phase is up - from the round start to
/// the `Begin` that plays the round out - which is the strip's lifetime (see
/// the module docs).
pub fn strip_visible(world: &crate::world::World) -> bool {
    world.mode == crate::world::SceneMode::Battle
        && world.battle.flow != crate::battle_flow::BattleFlowState::Idle
}

/// The strip for this frame, or `None` when this is not Koru's fight, the
/// round is playing out, or the host read no battle-overlay strings (the
/// label is disc text and the port carries no copy of it).
///
/// The gate reads the first **monster** seat's id - the port seats the
/// formation's slot 0 at actor index `party_count`
/// ([`crate::world::World::battle_monster_slots`]), where retail's fixed table
/// keeps it at index 3 - and the percentage reads the same seat.
pub fn timed_fight_strip(world: &crate::world::World) -> Option<TimedFightStrip> {
    if !strip_visible(world) {
        return None;
    }
    let (idx, id, _) = world
        .battle_monster_slots()
        .into_iter()
        .find(|&(_, _, slot)| slot == 0)?;
    if id != u16::from(TIMED_FIGHT_MONSTER_ID) {
        return None;
    }
    let a = world.actors.get(idx)?;
    let label = world
        .battle
        .ui_strings
        .get(legaia_asset::battle_ui_strings::BattleUiLabel::TimedFightStrip)?
        .to_string();
    Some(TimedFightStrip {
        label,
        turns_left: timed_fight_turns_left(u32::from(world.battle_mode())),
        hp_left: hp_left_percent(a.battle.hp, a.battle.max_hp),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_left_counts_down_from_four_and_floors() {
        assert_eq!(timed_fight_turns_left(0), 4);
        assert_eq!(timed_fight_turns_left(3), 1);
        assert_eq!(timed_fight_turns_left(4), 0);
        assert_eq!(timed_fight_turns_left(99), 0, "floored, not wrapped");
    }

    #[test]
    fn hp_left_is_a_plain_percentage() {
        assert_eq!(hp_left_percent(20_000, 20_000), 100);
        assert_eq!(hp_left_percent(9_999, 20_000), 49);
        assert_eq!(hp_left_percent(0, 20_000), 0);
        assert_eq!(hp_left_percent(5, 0), 0);
    }
}
