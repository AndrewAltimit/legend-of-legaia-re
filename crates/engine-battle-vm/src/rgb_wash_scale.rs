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
//! quad. What is inert is the **scale**: `0x1F800393` is the adaptive
//! frame-skip factor (`docs/subsystems/actor-vm.md`, "Tick cadence"), every
//! port host ticks at cadence `1`, and `scale_rgb24(rgb, 1)` is the identity
//! on a 24-bit colour. Calling it from `wash_prim` would move the
//! reachability graph and change no pixel. It becomes a real wire the day a
//! host runs a cadence above `1`.
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
/// form except the *scale*, and that one has no layer to come from: the
/// factor is `0x1F800393`, the adaptive frame-skip cadence, and every port
/// host ticks at cadence 1, where this function is the identity on a 24-bit
/// colour. A call from `wash_prim` would move the reachability graph and
/// change no pixel. It becomes a real wire only if a host ever runs a
/// cadence above 1 - a scheduling change, not a missing call site.
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

    #[test]
    fn scale_rgb_drops_the_top_byte() {
        // A non-zero alpha/top byte must not appear in the result.
        assert_eq!(scale_rgb24(0xFF_00_00_00, 3) & 0xFF00_0000, 0);
    }
}
