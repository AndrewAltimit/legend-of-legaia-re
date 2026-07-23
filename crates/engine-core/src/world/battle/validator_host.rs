//! [`ActionValidatorHost`] for [`World`], and the per-slot validity-mask pass
//! the battle target rows are built from.
//!
//! `FUN_8003FB10` is retail's single gate between "the cursor is on this slot"
//! and "this action may run against it". It writes its answer twice: as the
//! return value *and* as a bit in the per-slot validity byte at `gp + 0x9A8`,
//! and the menu greying reads the byte. [`World::action_validity_mask`] is that
//! byte: it runs one arm of the validator over a list of candidate slots,
//! accumulating into a single mask exactly as retail's repeated calls
//! accumulate into `gp + 0x9A8`.
//!
//! The battle target rows in [`super::command_flow`] are built from this mask
//! rather than from an inline `liveness != 0` test, so the selectability
//! decision runs through the ported kernel.
//!
//! REF: FUN_8003FB10, FUN_80046898

use super::*;

use legaia_engine_vm::battle_action::{ActionValidatorHost, SlotResources, validate_action};

/// Validator arm `0x05` - "the slot is alive". The arm the battle target rows
/// use: clear the slot's bit, re-set it when the actor's `+0x14C` is non-zero.
pub(in crate::world) const ARM_ALIVE: u8 = 0x05;

/// Borrowed [`ActionValidatorHost`] view of a [`World`].
///
/// The validator only ever reads, but [`validate_action`] takes `&mut H`
/// (retail's arms write the validity byte through the host's `gp`), so the
/// wrapper holds a shared borrow and the mask is threaded separately.
pub(in crate::world) struct WorldActionValidator<'a> {
    world: &'a World,
}

impl ActionValidatorHost for WorldActionValidator<'_> {
    /// `_DAT_8007B83C == 0x15`. The engine's equivalent is the scene mode.
    fn in_battle(&self) -> bool {
        matches!(self.world.mode, SceneMode::Battle)
    }

    /// Battle-actor `+0x14C/+0x14E/+0x150/+0x152`.
    ///
    /// Retail's `+0x14C` is one halfword serving as both live HP **and** the
    /// liveness flag; the port splits it into `BattleActor::hp` and
    /// `BattleActor::liveness`, and `liveness` is the authoritative one (the
    /// capture and petrify paths clear it without zeroing HP). The quad
    /// therefore reports `0` for a slot the engine has marked down and the
    /// live HP - floored at 1 so a live slot is never mistaken for a corpse -
    /// otherwise. Slots past the actor table return `None`, which the
    /// validator treats as an all-zero quad.
    fn slot_resources(&self, slot: u8) -> Option<SlotResources> {
        let a = &self.world.actors.get(slot as usize)?.battle;
        let hp = if a.liveness == 0 { 0 } else { a.hp.max(1) };
        Some(SlotResources {
            hp,
            hp_max: a.max_hp,
            mp: a.mp,
            mp_max: self
                .world
                .character_max_mp
                .get(slot as usize)
                .copied()
                .unwrap_or(a.mp),
        })
    }

    /// Battle-actor `+0x16E` - the per-actor flag bank the port keeps as
    /// `BattleActor::field_flags`.
    fn status_word(&self, slot: u8) -> u16 {
        self.world
            .actors
            .get(slot as usize)
            .map(|a| a.battle.field_flags)
            .unwrap_or(0)
    }

    /// `DAT_80084594` - the present-party member count.
    fn party_count(&self) -> u8 {
        self.world.party_count.clamp(1, 3)
    }
}

impl World {
    /// The per-slot validity byte (retail `gp + 0x9A8`) for `arm` over
    /// `slots`, computed by [`validate_action`].
    ///
    /// One call per candidate slot, accumulating into one byte - the shape
    /// retail's menu passes use, where each arm clears its own slot's bit
    /// before deciding whether to re-set it. Bit `n` is set when slot `n`
    /// passes; slots `>= 8` truncate to no bit at all, matching the retail
    /// byte store.
    ///
    /// PORT: FUN_8003FB10 (the caller-side validity-byte pass)
    pub(in crate::world) fn action_validity_mask(&self, arm: u8, sub_case: u8, slots: &[u8]) -> u8 {
        let mut bits = 0u8;
        let mut host = WorldActionValidator { world: self };
        for &slot in slots {
            validate_action(&mut host, arm, sub_case, slot, &mut bits);
        }
        bits
    }

    /// The `(party, monsters)` slot rows every in-battle target picker is
    /// opened with.
    ///
    /// `present` is table occupancy (the slot is configured for battle at
    /// all); the **selectable** bit is the validator's, read out of the
    /// [`ARM_ALIVE`] validity byte rather than tested inline. Party rows are
    /// slots `0..party_count`, monster rows the five slots above them - the
    /// engine's compact seating, so the whole set fits the byte's eight bits.
    pub(in crate::world) fn battle_target_rows(
        &self,
    ) -> (
        [crate::target_picker::SlotState; 3],
        [crate::target_picker::SlotState; 5],
    ) {
        use crate::target_picker::SlotState;
        let party_count = self.party_count.clamp(1, 3);
        let slots: Vec<u8> = (0..(party_count as usize + 5).min(8) as u8).collect();
        let mask = self.action_validity_mask(ARM_ALIVE, 0, &slots);
        let slot_at = |idx: usize| -> SlotState {
            match self.actors.get(idx) {
                Some(a) if a.battle.max_hp > 0 => {
                    SlotState::alive(true, idx < 8 && mask & (1u8 << idx) != 0)
                }
                _ => SlotState::default(),
            }
        };
        let mut party = [SlotState::default(); 3];
        for (i, p) in party.iter_mut().enumerate().take(party_count as usize) {
            *p = slot_at(i);
        }
        let mut monsters = [SlotState::default(); 5];
        for (i, m) in monsters.iter_mut().enumerate() {
            *m = slot_at(party_count as usize + i);
        }
        (party, monsters)
    }
}
