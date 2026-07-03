//! `BattleActionHost` trait: the engine-side callbacks the battle-action state machine dispatches into.

use super::*;

/// Engine-side callbacks the battle action state machine dispatches into.
///
/// All methods have default impls so a minimal host (no rendering / no
/// effects) compiles. Each method documents which retail function it stands
/// in for. The host owns the full actor table - the state machine asks for
/// pointers via [`BattleActionHost::actor`] / [`BattleActionHost::actor_mut`]
/// and treats the returned `&mut BattleActor` as `(&DAT_801C9370)[idx]`.
pub trait BattleActionHost {
    /// Equivalent of `(&DAT_801C9370)[slot]` - read-only access to the actor
    /// pointed at by the table slot. Returning `None` aborts the step (the
    /// retail dispatcher silently exits when the active actor pointer is
    /// null).
    fn actor(&self, slot: u8) -> Option<&BattleActor>;

    /// Equivalent of `(&DAT_801C9370)[slot]` - mutable access. Same null
    /// semantics as [`BattleActionHost::actor`].
    fn actor_mut(&mut self, slot: u8) -> Option<&mut BattleActor>;

    /// Equivalent of `FUN_801D5854(actor_id, pose_id)` - per-actor pose
    /// driver. Default no-op.
    fn pose(&mut self, _actor_id: u8, _pose: Pose) {}

    /// Equivalent of `FUN_801D8DE8(effect_id, mode)` - battle UI element
    /// scheduler. `mode == 0` spawns / resets; `mode == 1` terminates /
    /// unloads. Default no-op.
    fn ui_element(&mut self, _effect_id: u8, _mode: u8) {}

    /// Equivalent of `FUN_8004E2F0(actor, target)` - battle range / LOS
    /// check. Returns 0 = "in range," non-zero = distance metric. Default
    /// returns 0 (always in range - useful for unit tests).
    fn range_check(&self, _actor_slot: u8, _target_slot: u8) -> u16 {
        0
    }

    /// Equivalent of `FUN_801EFE44` - battle camera bounds. Walks the 8-slot
    /// table for min/max. Default no-op.
    fn camera_bounds(&mut self) {}

    /// Equivalent of `FUN_801EED1C` - party setup hook (called for actors
    /// with slot < 3). Default no-op.
    fn party_setup(&mut self, _actor_slot: u8) {}

    /// Equivalent of `FUN_801E7320` - monster-AI setup hook. Default no-op.
    fn monster_setup(&mut self, _actor_slot: u8) {}

    /// Equivalent of `FUN_801DABA4` - recompute battle ordering. Default
    /// no-op.
    fn recompute_battle_order(&mut self) {}

    /// Capture-pose animation pick for the captured monster: retail
    /// `FUN_80050E2C(record + 0x4C, 1, record[0x4A])` selects an anim id
    /// from the monster archive record's action table
    /// (`(&DAT_801C9348)[slot - 3]`). `None` keeps the actor's queued anim
    /// unchanged (hosts that don't resolve monster records). Called by the
    /// `FUN_801E7824` port during `CaptureStart`.
    fn capture_anim(&mut self, _monster_slot: u8) -> Option<u8> {
        None
    }

    /// Equivalent of `func_0x80056798()` (PSX rand BIOS, `A0 0x2E`). Default
    /// returns 0 for deterministic tests.
    fn rng(&mut self) -> u32 {
        0
    }

    /// Equivalent of `func_0x8003F2B8(1)` - "pause until previous animation
    /// cleared" gate. Returns `true` when the previous action has fully
    /// drained. Default returns `true` (always cleared - useful for tests
    /// that fast-forward through transitions).
    fn previous_action_cleared(&self, _arg: u8) -> bool {
        true
    }

    /// Equivalent of `func_0x8003DE7C(1)` - sound-bank-ready gate. Default
    /// returns `true`.
    fn sound_bank_ready(&self, _arg: u8) -> bool {
        true
    }

    /// Equivalent of `func_0x8003EAE4(0, idx)` - load capture archive.
    /// Default no-op.
    fn load_capture_archive(&mut self, _idx: u8) {}

    /// Equivalent of `FUN_801DBF9C(party_slot, spell_id)` - spell-anim
    /// trigger. Default no-op.
    fn spell_anim_trigger(&mut self, _party_slot: u8, _spell_id: u8) {}

    /// Equivalent of `FUN_801DC0A0(actor_id, anim_id)` - sustained spell
    /// animation. Default no-op.
    fn spell_anim_sustain(&mut self, _actor_id: u8, _anim_id: u8) {}

    /// Equivalent of `func_0x800402F4(icon, page, target_slot, party_slot)` -
    /// damage application primitive. Default no-op.
    fn apply_damage(&mut self, _icon: u8, _page: u8, _target_slot: u8, _party_slot: u8) {}

    /// Apply one Tactical-Art strike with the power-byte / hit-timing values
    /// pulled from the active art record.
    ///
    /// Called by [`ActionState::AttackChain`] in place of [`apply_damage`]
    /// when the active actor's `chosen_art` is set and `art_record` returns
    /// a record. `info` carries the per-strike values the SM read from the
    /// art's `power` + `dmg_timing` + `enemy_effect` + `hit_cues`. Engines
    /// translate these into HP deduction + status effect + sound/visual
    /// cues - the SM only resolves the values, it does not apply them.
    ///
    /// Default no-op. Engines that don't override fall through to
    /// [`apply_damage`] as well (the SM still calls that for backward
    /// compatibility), so a host that hasn't wired arts yet keeps working.
    fn apply_art_strike(&mut self, _info: ArtStrikeInfo) {}

    /// Returns `true` if the spell at `spell_id` is a capture-class spell
    /// (first byte of its table entry is `'c'`). Drives the
    /// `MagicCastBegin → MagicCaptureBranch` route. Default returns `false`.
    fn is_capture_spell(&self, _spell_id: u8) -> bool {
        false
    }

    /// Lookup the MP cost for a spell. Retail reads
    /// `&DAT_800754D0 + spell_id*0xC + 3`. Default returns 0.
    fn spell_mp_cost(&self, _spell_id: u8) -> u8 {
        0
    }

    /// Returns the character ability bitmask at `0x80084708 + (party_id-1) *
    /// 0x414 + 0xF4`. Bit `0x20` reduces MP cost by half, `0x10` by a quarter
    /// (`0x20` wins when both are set); `0x100` / `0x200` scale impact
    /// magnitude; etc. Default returns 0.
    fn character_ability_bits(&self, _party_slot: u8) -> u32 {
        0
    }

    /// Equivalent of the screen-shake driver - sets the global `_DAT_800840BC`
    /// to `0x500` (small kick). Default no-op.
    fn screen_shake(&mut self, _magnitude: u16) {}

    /// Equivalent of the brightness ramp at states `SummonSustain` /
    /// `MagicCaptureFade` - clamps `_DAT_8007B910` toward a target.
    /// Default no-op.
    fn ramp_brightness(&mut self, _target_pct: u8) {}

    /// Notify the host the battle is ending. The state machine sets the
    /// retail `DAT_8007BD71 = 0xFE`; engines wire this to "unload battle
    /// overlay." Default no-op.
    fn battle_end(&mut self, _cause: BattleEndCause) {}

    /// Frame delta-time tick used by `frame_timer` decrement. Retail reads
    /// `DAT_1F800393` (the per-frame dt byte). Default returns 1 - one tick
    /// per step.
    fn frame_dt(&self) -> i16 {
        1
    }

    /// Iteration helper - number of party slots in the table (slots `0..3`
    /// are party). Default is 3. Engines override if the layout differs.
    fn party_count(&self) -> u8 {
        3
    }

    /// Iteration helper - total slot count (default `8`).
    fn slot_count(&self) -> u8 {
        ACTOR_SLOTS as u8
    }

    /// Look up the [`legaia_art::ArtRecord`] for an actor's chosen art. The
    /// state machine reads this on Tactical Arts windup to fetch power
    /// bytes, hit timing, repeat-frame data, and the status effect to
    /// apply on hit.
    ///
    /// Default returns `None` - pure-host tests don't need art data, and
    /// the SM falls back to attack-chain default damage when an art record
    /// is unavailable.
    fn art_record(
        &self,
        _character: legaia_art::Character,
        _action: legaia_art::ActionConstant,
    ) -> Option<&legaia_art::ArtRecord> {
        None
    }
}
