//! Effect-VM host interface: the [`StateOutcome`] per-slot advance result and
//! the [`EffectHost`] engine-callback trait. Split out of `effect_vm.rs`.

use super::*;

/// Outcome of one master-slot state advance, returned by
/// [`EffectHost::advance_state`]. The pool uses this to update the slot's
/// state byte / lifecycle. Legacy-path only: the faithful walker
/// ([`Pool::tick_retail`]) derives the lifecycle from the catalog's spawn
/// records and animation frames and never consults this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateOutcome {
    /// Stay active, run again next frame (state byte stays at 0).
    Continue,
    /// Wait `frames` frames before the next advance. The pool encodes this
    /// using the retail `state = frames + 8` convention so the countdown
    /// path picks it up next tick.
    Wait { frames: u8 },
    /// Effect is done - the pool zeroes the master slot.
    Terminate,
}

/// Engine-side callbacks the effect VM dispatches into.
///
/// All methods have default impls so a minimal host (only RNG) compiles.
/// Each method documents which retail function it stands in for. The
/// faithful walker ([`Pool::tick_retail`]) uses only [`next_random`]; the
/// `advance_state` / `accumulate_child_motion` hooks belong to the legacy
/// [`Pool::tick`] shim.
///
/// [`next_random`]: EffectHost::next_random
pub trait EffectHost {
    /// Equivalent of `func_0x80056798` - uniform random `i32`. The retail
    /// PRNG is an LCG seeded by `_DAT_8007AB80`; engines plug whatever RNG
    /// they have. Default impl returns `0` (deterministic for tests).
    fn next_random(&mut self) -> i32 {
        0
    }

    /// Returns `true` if `effect_id` should be routed to the streaming-
    /// summon handler (`func_0x80050ed4`) instead of the generic spawn
    /// path. Retail special-cases `id == 4` and `id == 0x13`. Engines
    /// override to route their summon IDs.
    fn is_summon_effect(&self, _effect_id: u8) -> bool {
        false
    }

    /// Equivalent of `func_0x80050ed4(world_pos, &stack_buf, summon_table,
    /// 0x1000)` - the streaming-summon handler. Buffer size per slot is
    /// `0x10800 = 67584` bytes. Default no-op.
    fn handle_summon(&mut self, _effect_id: u8, _world_pos: [i16; 3], _angle: u16) {}

    /// Per-child-sprite random offset, computed by [`Pool::spawn`] when
    /// `flags & 0x01` is set. The retail code scribbles these back into
    /// the script bytes; the port exposes them to the host so engines
    /// store them next to their per-child render state. Default no-op.
    fn assign_child_random_offset(
        &mut self,
        _slot: usize,
        _child_idx: u8,
        _dx_world: i16,
        _dz_world: i16,
    ) {
    }

    /// Per-frame child-sprite motion integration for one active master
    /// slot. Runs **every frame for every active slot, independent of the
    /// master `state` byte** - the retail walker `FUN_801E0088` accumulates
    /// each child's position (`child+0xc/+0x10/+0x14 += velocity * accel *
    /// frame_delta`) in *both* the `state == 0` work loop and the
    /// `state != 0` countdown else-branch (`overlay_battle_801e0088.txt`,
    /// the two `*(int *)(pcVar7 + 0xc) = ... + ...` blocks). Only the
    /// *script advance* (the next-state read) is gated on `state == 0`; the
    /// position drift is not. So a child billboard keeps moving while its
    /// effect is in a "wait" state - this hook is what reproduces that drift
    /// (gating it behind [`advance_state`] froze waiting effects). Default
    /// no-op for hosts that don't render child motion.
    fn accumulate_child_motion(&mut self, _slot: usize, _master: &mut MasterSlot) {}

    /// Per-frame state advance for one active master slot. Engines do
    /// whatever per-effect *script* work they have (read the next state
    /// byte, emit GPU primitives, decrement counters) and return
    /// [`StateOutcome`] describing the lifecycle. Called only when the
    /// master `state` byte is `0` (the per-frame position integration that
    /// runs regardless of `state` lives in [`accumulate_child_motion`]).
    /// Default impl just terminates the slot - useful for engines that
    /// haven't wired the renderer yet.
    fn advance_state(&mut self, _slot: usize, _master: &mut MasterSlot) -> StateOutcome {
        StateOutcome::Terminate
    }
}
