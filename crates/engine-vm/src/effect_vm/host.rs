//! Effect-VM host interface: the [`EffectHost`] engine-callback trait. Split
//! out of `effect_vm.rs`.

/// Engine-side callbacks the effect VM dispatches into.
///
/// All methods have default impls so a minimal host (only RNG) compiles.
/// Each method documents which retail function it stands in for. The
/// faithful walker ([`Pool::tick_retail`]) uses only [`next_random`]; the
/// remaining hooks belong to the spawn path ([`Pool::spawn`]).
///
/// [`next_random`]: EffectHost::next_random
/// [`Pool::tick_retail`]: super::Pool::tick_retail
/// [`Pool::spawn`]: super::Pool::spawn
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

    /// Per-child-sprite random offset, computed by [`Pool::spawn`](super::Pool::spawn)
    /// when `flags & 0x01` is set. The retail code scribbles these back into
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
}
