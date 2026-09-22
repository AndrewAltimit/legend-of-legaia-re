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

    /// Returns `true` if `effect_id` makes the side call to
    /// `func_0x80050ed4` before the generic spawn (retail special-cases
    /// `id == 4` and `id == 0x13`; the effect is still spawned after it).
    /// Engines override to route their ids.
    fn is_summon_effect(&self, _effect_id: u8) -> bool {
        false
    }

    /// Equivalent of `func_0x80050ed4(world_pos, &{0, angle, 0}, descriptor,
    /// 0x1000)` with descriptor `0x801F5D90` (id 4) or `0x801F5CF8` (id
    /// `0x13`) - the move-VM actor spawner, run as a side call ahead of the
    /// ordinary effect spawn. Default no-op.
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
