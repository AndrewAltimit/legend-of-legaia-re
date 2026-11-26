//! The per-channel RGB colour modulate with saturation ([`scale_rgb24`],
//! `FUN_80046978`): the scale step of the battle band's armed screen wash.
//!
//! Read out of the instruction stream in the reference dump
//! (`ghidra/scripts/funcs/80046978.txt`), not the decompiled C. No
//! `SCUS_942.54` bytes live in this crate.
//!
//! REF: FUN_8004695C (the arm whose drain `scale_rgb24` is the maths of)
//! REF: FUN_80024EE4 (that drain's submit)
//!
//! # Why no host calls it
//!
//! [`scale_rgb24`] is the **drain of the armed wash**, and every part of that
//! protocol has an engine form - which is exactly why wiring it would be a
//! fake wire. Its retail caller is `FUN_80016444` (call site `80016748`,
//! ported in `engine-core::world::frame_tick`). `FUN_8004695C` arms
//! `gp[0x9D4]` / `gp[0x9D0]`; the two engine arm sites are
//! `engine-render::battle_intro`'s `PARTICLE_WASH_RGB` and
//! `swirl::LATE_WASH_RGB` (pushed once per frame, which is retail's re-arm),
//! and the submit `FUN_80024EE4(otlen - 1, 2, rgb)` is
//! `battle_intro::wash_prim`, the same farthest-bucket ABR-`2` full-screen
//! quad. What is inert is the **scale**, and the reason is the emitter's
//! clock, not the host's. `0x1F800393` is the adaptive frame-skip factor
//! (`docs/subsystems/actor-vm.md`, "Tick cadence"), and retail does run the
//! intro above `1`: on a walked fight the step falls `3`/`4` -> `2` -> `1` as
//! the intro's cost drops, and the scatter style's own body installs a floor
//! of `3` (`engine-core::world::frame_step_floor`). A retail frame at step
//! `s` advances the intro entity's clock by `s` and drains the wash scaled by
//! `s`. The port's emitter instead steps **once per clock unit**
//! (`BattleIntro::advance_to`, each step at `frame_step = 1`), so the same
//! clock span is `s` port steps that each drain the unscaled colour - the
//! scale is the frame-skip compensation the per-unit stepping already does.
//! For the curtain's display decay (`CURTAIN_DISPLAY_DECAY_5`, one 5-bit level
//! each time a host refreshes the page, once per presented frame, and the
//! entity clock moves one unit per world tick) the totals agree exactly,
//! since `0x08 * s >> 3 == s` and nothing saturates below `s = 32`. The
//! scatter style's `0x101010` comes off its display model the same way
//! (`SCATTER_DISPLAY_DECAY_5`, two levels a step), and the swirl's late wash
//! lands on a black backdrop, where a subtraction of any size is a no-op.
//! Calling this from `wash_prim` with the step would double-count the
//! cadence.
//!
//! It is ported because its edge case (the pre-multiply saturation) is
//! observable and testable on its own.

/// Scale each 8-bit channel of a packed `0x00BBGGRR` colour by `scale`,
/// saturating each product at `0xFF` - the arithmetic core of
/// `FUN_80046978`.
///
/// PORT: FUN_80046978
/// REPLACED-BY: `legaia_render_kernels::battle_intro::wash_prim` plus its two arm
/// constants (`PARTICLE_WASH_RGB`, `battle_intro_swirl::LATE_WASH_RGB`),
/// which push the same farthest-bucket ABR-2 full-screen quad retail's
/// submit `FUN_80024EE4(otlen - 1, 2, rgb)` builds, once per frame on both
/// hosts. Every part of the armed-wash protocol therefore has a live engine
/// form except the *scale*, and the scale is the cadence compensation the
/// emitter already performs: the factor is `0x1F800393`, the adaptive
/// frame-skip step, and where retail drains one wash scaled by a step `s`
/// (`3`/`4` early in a walked fight's intro, a floor of `3` under the scatter
/// style), `BattleIntro::advance_to` runs `s` unit steps over the same clock
/// span, each draining the unscaled colour. A call from `wash_prim` with the
/// step would count the cadence twice. See the module heading for why the
/// totals agree.
///
/// Retail unpacks the stored colour word into its low three bytes, and for
/// each channel computes `channel * scale` (an 8-bit `* 8-bit` product, at
/// most `0xFE01`), clamping to `0xFF` when the product reaches `0x100`
/// (`slti ..., 0x100`). The three clamped channels are repacked in the
/// original `R | G<<8 | B<<16` order. The top byte is dropped - the routine
/// never reads or reassembles it.
///
/// The full `FUN_80046978` is a triggered submit and is **not** reproduced
/// here: it early-outs unless the trigger word `gp[0x9D4]` is non-zero,
/// clears that word, reads the stored colour from `gp[0x9D0]` and the
/// scale byte from `0x1F800393` (a scratchpad global), and hands the packed
/// result to `FUN_80024EE4(id - 1, 2, packed)` where `id` is the `u16` at
/// `0x1F8003A6`. Those globals and the submit belong to the caller; this
/// function is the colour maths, which is the reusable and testable part.
pub fn scale_rgb24(packed: u32, scale: u8) -> u32 {
    let s = scale as u32;
    let chan = |shift: u32| -> u32 {
        let c = (packed >> shift) & 0xFF;
        let p = c * s;
        if p >= 0x100 { 0xFF } else { p }
    };
    let r = chan(0);
    let g = chan(8);
    let b = chan(16);
    r | (g << 8) | (b << 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_rgb_identity_at_scale_one() {
        assert_eq!(scale_rgb24(0x00_12_34_56, 1), 0x00_12_34_56);
    }

    #[test]
    fn scale_rgb_zero_scale_blacks_out() {
        assert_eq!(scale_rgb24(0x00_FF_FF_FF, 0), 0);
    }

    #[test]
    fn scale_rgb_saturates_each_channel_independently() {
        // R=0x08 *4 = 0x20 (no clamp), G=0x40 *4 = 0x100 -> 0xFF,
        // B=0x80 *4 = 0x200 -> 0xFF.
        let packed = 0x00_80_40_08;
        assert_eq!(scale_rgb24(packed, 4), 0x00_FF_FF_20);
    }

    #[test]
    fn scale_rgb_clamp_threshold_is_0x100() {
        // 0x33 * 5 = 0xFF -> exactly under 0x100, kept.
        assert_eq!(scale_rgb24(0x00_00_00_33, 5) & 0xFF, 0xFF);
        // 0x34 * 5 = 0x104 -> clamps to 0xFF.
        assert_eq!(scale_rgb24(0x00_00_00_34, 5) & 0xFF, 0xFF);
        // 0x33 * 4 = 0xCC -> below threshold, kept as product.
        assert_eq!(scale_rgb24(0x00_00_00_33, 4) & 0xFF, 0xCC);
    }

    /// The replacement's arithmetic: one retail drain at step `s` takes the
    /// same 5-bit levels off a 15-bpp pixel as `s` unscaled drains, for the
    /// curtain's `0x80808` wash at every step the intro runs (`1..=4`) and
    /// well past it. This is what lets the emitter's once-per-clock-unit
    /// stepping stand in for the scale.
    #[test]
    fn one_scaled_drain_is_s_unscaled_drains_in_five_bit_levels() {
        let level = |rgb: u32| (rgb & 0xFF) >> 3;
        for s in 1..=31u8 {
            let scaled = level(scale_rgb24(0x0008_0808, s));
            assert_eq!(scaled, u32::from(s) * level(0x0008_0808), "step {s}");
        }
    }

    #[test]
    fn scale_rgb_drops_the_top_byte() {
        // A non-zero alpha/top byte must not appear in the result.
        assert_eq!(scale_rgb24(0xFF_00_00_00, 3) & 0xFF00_0000, 0);
    }
}
