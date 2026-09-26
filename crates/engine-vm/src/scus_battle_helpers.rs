//! Small arithmetic kernels lifted from the `SCUS_942.54` battle band.
//!
//! PORT: FUN_80055854, FUN_80046978
//!
//! Two self-contained whole routines: a two-level variable-length word-record
//! copier ([`copy_nested_records`]) and a per-channel RGB colour-modulate with
//! saturation ([`scale_rgb24`]).
//!
//! On top of those, this module carries the reusable *arithmetic cores* of
//! several larger battle-actor routines whose bodies are otherwise
//! render-track (they call the GTE, write GPU primitive packets through the
//! scratchpad OT pointer `_DAT_1f8003a0`, or drive dozens of battle globals)
//! and therefore are **not** ported whole in the engine. Each core
//! is the faithful, testable computation the retail routine repeats inline:
//!
//! - [`bgr555_to_grey`] - the desaturate step of the stone/petrify CLUT-fade
//!   builder `FUN_8004ce2c`. **Wired**: `engine-core::battle_status_clut`
//!   calls it per latched party slot; see the note below.
//!
//! The two cores of the battle tint pass `FUN_8004a908` that used to live
//! here (the depth-brightness ramp and the negative-colour recolour) moved to
//! [`crate::battle_actor_tint`] when that routine was ported whole.
//!
//! Every claim below is read out of the instruction stream in the reference
//! dumps, not the decompiled C.
//!
//! REF: FUN_8004695C (the arm whose drain `scale_rgb24` is the maths of)
//! REF: FUN_80024EE4 (that drain's submit)
//!
//! ## Port boundary
//!
//! No `SCUS_942.54` bytes live in this crate. The reference dumps
//! (`ghidra/scripts/funcs/80055854.txt`, `80046978.txt`, `8004ce2c.txt`,
//! `8004a908.txt`) are the *spec*. The render-track parents of the extracted
//! cores are documented, with provenance, in `docs/subsystems/battle.md`.
//!
//! # NOT WIRED
//!
//! Two of the four rows below: [`scale_rgb24`] and [`copy_nested_records`].
//! [`bgr555_to_grey`] is live through `engine-core::battle_status_clut`, and
//! the tint-pass cores are live in [`crate::battle_actor_tint`], which both
//! play hosts reach per battle body per frame. The notes those three used to carry are kept
//! below, rewritten, because *how* each got a consumer is the pattern the
//! other two still lack.
//!
//! "The battle path is expected to grow a consumer" is a forecast, not a
//! reason, and it is not the one that holds. Each kernel is the arithmetic
//! core of a routine whose *body* the port deliberately stops
//! short of, so what has to exist first is that body's engine equivalent -
//! and each is a different missing thing:
//!
//! And it is worth being exact about which half is missing, because it is not
//! the caller. Every one of these four has a retail caller, every caller is
//! dumped, and every caller is already ported somewhere in the workspace:
//!
//! | Kernel | Retail caller | Call site | Port of the caller |
//! |---|---|---|---|
//! | [`scale_rgb24`] | `FUN_80016444` | `80016748` | `engine-core::world::frame_tick` |
//! | the tint-pass cores (now [`crate::battle_actor_tint`]) | `FUN_80047430`, `FUN_800480D8` | `800476C8`; `800481B4`/`8004825C`/`800482E8` | `engine-vm::battle_hp_bar`; `engine-vm::battle_actor_tint` (live) |
//! | [`bgr555_to_grey`] | `FUN_8004DA00` | `8004DC4C` | `engine-audio::battle_voice` |
//! | [`copy_nested_records`] | `FUN_80052FA0` | `80053438`, `800534F4` | `asset::battle_char_palette` |
//!
//! So the shape is not "waiting for a caller" - it is that each ported caller
//! reached the same outcome by a different route, over typed engine state
//! instead of the raw words / globals / framebuffer strips these kernels
//! read. A wire would have to re-introduce the retail representation on the
//! engine side purely so the kernel had something to chew, which is the
//! definition of a fake wire. Per kernel:
//!
//! * [`copy_nested_records`] stages a nested record block between two raw
//!   `u32` buffers. The engine has no such buffer: animation and keyframe
//!   data arrive as parsed `legaia_asset` types and are handed to the
//!   renderer as typed clips, never staged word-wise, so no caller holds a
//!   `&mut [u32]` destination for it to advance.
//! * [`scale_rgb24`] is the **drain of the armed wash**, and every part of that
//!   protocol now has an engine form - which is exactly why wiring it would be
//!   a fake wire. `FUN_8004695C` arms `gp[0x9D4]` / `gp[0x9D0]`; the two engine
//!   arm sites are `engine-render::battle_intro`'s `PARTICLE_WASH_RGB` and
//!   `swirl::LATE_WASH_RGB` (pushed once per frame, which is retail's re-arm),
//!   and the submit `FUN_80024EE4(otlen - 1, 2, rgb)` is
//!   `battle_intro::wash_prim`, the same farthest-bucket ABR-`2` full-screen
//!   quad. What is inert is the **scale**: `0x1F800393` is the adaptive
//!   frame-skip factor (`docs/subsystems/actor-vm.md`, "Tick cadence"), every
//!   port host ticks at cadence `1`, and `scale_rgb24(rgb, 1)` is the identity
//!   on a 24-bit colour. Calling it from `wash_prim` would move the
//!   reachability graph and change no pixel. It becomes a real wire the day a
//!   host runs a cadence above `1`.
//! * [`bgr555_to_grey`] does **not** read a captured framebuffer. That reading
//!   is withdrawn - `docs/subsystems/battle.md` ("CLUT status recolour") has
//!   the right one, and this module disagreed with it. The source is the battle
//!   context's own per-actor **240-entry palette** at `ctx[+0x894]`
//!   (`0x8004D6F8 lhu v1,0x894(v0)`), the greyed copy is staged at `ctx[+0xE34]`
//!   and a 1-pixel-tall rect uploads it to VRAM CLUT row `481 + slot`
//!   (`0x8004D764..0x8004D798`). Those rows are not missing either:
//!   `legaia_asset::battle_char_palette` decodes the party CLUTs and the loader
//!   STP-copies them to `481 + slot`.
//!
//!   The three things listed here as missing between those ends - a per-actor
//!   palette copy, the `actor[+0x220..=+0x223]` latch, and a mid-battle CLUT
//!   re-upload path - now exist: `engine-core::battle_status_clut` holds the
//!   copy and the latch, `BattleHud::sync_status` arms it, and the native
//!   window's `tick_battle_status_clut` runs the pass against the stashed
//!   battle VRAM. The third clause was the one that was closest to already
//!   being false: `tick_battle_face_stamps` had been mutating battle VRAM
//!   mid-battle and re-uploading with the resident-generation bookkeeping for
//!   some time; it just moved texels rather than CLUT rows.
//!
//!   Two honest limits on that wire. The **Rot arm** is still out: it tints
//!   over a per-character index window from `DAT_80078630`, which no crate
//!   parses.
//!
//!   And the pass, though live, still does not fire in ordinary play - but the
//!   reason recorded here was wrong, and half of it is now fixed. The claim
//!   was "the port has no monster-side `enemy_effect` source at all". It has
//!   one, and it is not an art record: retail's enemy-special status source is
//!   the **move-power record's `+0x0A` impact-effect selector**, which
//!   `FUN_801DEA50` parks at `ctx+0x1014` and `FUN_801E09F8` branches on at
//!   the impact phase. `legaia_asset::move_power` has parsed that byte all
//!   along; nothing applied it. `engine-core`'s
//!   `World::apply_enemy_move_status` now does, so a monster special puts
//!   Venom / Toxic / Rot on a *party* slot in play.
//!
//!   What is left is narrower and is a property of the **byte space**, not of
//!   the wiring: no `+0x0A` selector value maps to Stone. That ladder ends at
//!   `5` (`0x801E1620` tests only `5`), and `FUN_801EC3E4`'s sibling ladder
//!   adds only `6` = Curse. Petrification's applier is elsewhere entirely -
//!   `ori 0x4` at `0x80041CF4` / `0x80041DE4`, in the SCUS `0x80041...`
//!   item/effect band around `FUN_800402F4` - and that one is unported. Since
//!   `BattleHud::sync_status` arms the CLUT latch on `StatusKind::Stone` only,
//!   rows `481..=483` stay unexercised until that applier lands.
//! * The tint-pass cores belong to the actor
//!   tint pass, whose depth term comes from the GTE transform `FUN_8003D344`
//!   per actor per frame. The earlier note here said nothing on the CPU side
//!   holds a `(num, den)` pair; the pair is `(radius / 2, view_z / 16)`, and
//!   the view depth is one row of the shared battle camera
//!   (`battle_cam_script::battle_view_depth`), so the whole routine is now
//!   ported as `crate::battle_actor_tint` and these two are its arithmetic.
//!
//! They are ported because each edge case (the do-while floor, the
//! pre-multiply saturation, the luminance clamp, the min-4 dim floor) is
//! observable and testable on its own.

// ---------------------------------------------------------------------------
// FUN_80055854 - two-level nested word-record copy
// ---------------------------------------------------------------------------

/// Copy a two-level, variable-length record structure of 32-bit words -
/// `FUN_80055854`.
///
/// PORT: FUN_80055854
/// REPLACED-BY: `legaia_asset::battle_char_palette`, the port of this
/// routine's one retail caller `FUN_80052FA0`, which parses the nested block
/// into typed records instead of staging it word-wise between two raw `u32`
/// buffers. Nothing in the engine holds a `&mut [u32]` destination for this
/// to advance, because the destination it would fill does not exist as a
/// representation - the same substitution the rest of this module's
/// "Port boundary" note describes, and a call site would have to
/// re-introduce retail's staging buffer purely so the kernel had something
/// to chew.
///
/// The retail routine takes a source and destination `u32*` and walks a
/// nested table, returning the advanced destination pointer (i.e. the
/// number of words written). The layout, straight off the loads/stores:
///
/// - **Outer header**: three words. Words 0 and 1 are copied verbatim;
///   word 2 is the outer record count (`t0`). If it is `<= 0` the routine
///   stops after the header (`blez t0` at `0x80055884`).
/// - For each of `count` outer records:
///   - **Inner header**: three words. Words 0 and 1 verbatim; word 2 is
///     the inner element count (`v0`).
///   - **Inner body**: `inner_count * 2` words (`sll a2, v0, 1` at
///     `0x800558bc`), copied verbatim. A non-positive doubled count skips
///     the body (`blez a2`).
///
/// Both count tests are signed (`blez` / `slt`), and both loops are
/// `while`-shaped (the guard precedes the body), so a zero count copies
/// only the header - unlike the `do-while` copier in
/// [`scus_core_helpers::copy_blocks_32`](crate::scus_core_helpers::copy_blocks_32).
///
/// This port reads whole words from `src` and writes them into `dst`,
/// returning the count of words written. It stops early - returning what
/// it has copied - if either slice runs short, where retail (which does no
/// bounds check) would stride into adjacent memory.
pub fn copy_nested_records(src: &[u32], dst: &mut [u32]) -> usize {
    let mut si = 0usize;
    let mut di = 0usize;

    // Helper: copy one word src[si] -> dst[di], advancing both. Returns
    // false when either side is exhausted.
    macro_rules! copy_one {
        () => {{
            match (src.get(si), dst.get_mut(di)) {
                (Some(&w), Some(slot)) => {
                    *slot = w;
                    si += 1;
                    di += 1;
                    true
                }
                _ => return di,
            }
        }};
    }

    // Outer header: word0, word1, then the outer count word.
    if !copy_one!() {
        return di;
    }
    if !copy_one!() {
        return di;
    }
    let outer_count = match src.get(si) {
        Some(&w) => w as i32,
        None => return di,
    };
    if !copy_one!() {
        return di;
    }

    if outer_count <= 0 {
        return di;
    }

    for _ in 0..outer_count {
        // Inner header: word0, word1, then the inner count word.
        if !copy_one!() {
            return di;
        }
        if !copy_one!() {
            return di;
        }
        let inner_count = match src.get(si) {
            Some(&w) => w as i32,
            None => return di,
        };
        if !copy_one!() {
            return di;
        }

        // Body: inner_count * 2 words.
        let body = inner_count.wrapping_mul(2);
        if body > 0 {
            for _ in 0..body {
                if !copy_one!() {
                    return di;
                }
            }
        }
    }

    di
}

// ---------------------------------------------------------------------------
// FUN_80046978 - per-channel RGB colour modulate with saturation
// ---------------------------------------------------------------------------

/// Scale each 8-bit channel of a packed `0x00BBGGRR` colour by `scale`,
/// saturating each product at `0xFF` - the arithmetic core of
/// `FUN_80046978`.
///
/// PORT: FUN_80046978
/// REPLACED-BY: `legaia_engine_ui::battle_intro::wash_prim` plus its two arm
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

// ---------------------------------------------------------------------------
// FUN_8004ce2c - stone/petrify grey-out CLUT-fade luminance
// ---------------------------------------------------------------------------

/// Desaturate one 15-bit `BGR555` pixel to grey - the arithmetic core of the
/// petrify CLUT-fade builder `FUN_8004ce2c`.
///
/// PORT: FUN_8004ce2c
///
/// The retail routine walks the acting actor's 240-entry palette copy in the
/// battle context (`ctx[+0x894]`), and for each entry computes a single
/// luminance value and writes it into all three 5-bit channels, staging the
/// grey CLUT at `ctx[+0xE34]` for upload to VRAM row `481 + slot`. It is a
/// palette, not a framebuffer strip - the pixels on screen are never read.
/// Straight off the disassembly (`0x8004d700`):
///
/// - `r = pixel & 0x1F`, `g = (pixel >> 5) & 0x1F`, `b = (pixel >> 10) & 0x1F`.
/// - `lum = (r + g + b) >> 2` (an arithmetic shift; the sum is non-negative
///   so it is a plain floor-divide by four - note the divisor is 4, not 3, so
///   this is a deliberately-dimmed average, max `0x17`).
/// - `if lum > 0x1F { lum = 0x1F }` (`sltiu ..., 0x20`).
/// - repack `lum | (lum << 5) | (lum << 10)`.
///
/// The top `STP` bit (`0x8000`) is **not** set by this variant - it is added
/// by the sibling brightened path (`lum * 3 / 2`), which is a separate ramp
/// and not reproduced here. The surrounding routine (packet build via
/// `_DAT_1f8003a0`, `FUN_800583c8` submit) is render-track; see
/// `docs/subsystems/battle.md`.
pub fn bgr555_to_grey(pixel: u16) -> u16 {
    let r = pixel & 0x1F;
    let g = (pixel >> 5) & 0x1F;
    let b = (pixel >> 10) & 0x1F;
    let lum = ((r + g + b) >> 2).min(0x1F);
    lum | (lum << 5) | (lum << 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_copy_header_only_when_outer_count_is_zero() {
        // Outer header [w0, w1, 0] with a non-zero count word absent.
        let src = [0xAAAA_AAAA, 0xBBBB_BBBB, 0, 0xDEAD_BEEF];
        let mut dst = [0u32; 8];
        let written = copy_nested_records(&src, &mut dst);
        assert_eq!(written, 3, "blez outer count stops after the header");
        assert_eq!(&dst[..3], &[0xAAAA_AAAA, 0xBBBB_BBBB, 0]);
        assert_eq!(dst[3], 0, "trailing word untouched");
    }

    #[test]
    fn nested_copy_one_outer_one_inner_pair() {
        // Outer: [h0, h1, count=1]
        //   Inner: [i0, i1, inner=2] then 2*2 = 4 body words.
        let src = [
            0x0000_00A0, // h0
            0x0000_00A1, // h1
            1,           // outer count
            0x0000_00B0, // inner h0
            0x0000_00B1, // inner h1
            2,           // inner count
            0x10,
            0x11,
            0x12,
            0x13, // 4 body words
        ];
        let mut dst = [0u32; 16];
        let written = copy_nested_records(&src, &mut dst);
        assert_eq!(written, src.len());
        assert_eq!(&dst[..src.len()], &src[..]);
    }

    #[test]
    fn nested_copy_inner_body_is_double_the_inner_count() {
        // inner count 3 -> 6 body words.
        let mut src = vec![0xF0, 0xF1, 1, 0xE0, 0xE1, 3];
        src.extend([0x20, 0x21, 0x22, 0x23, 0x24, 0x25]);
        let mut dst = [0u32; 16];
        let written = copy_nested_records(&src, &mut dst);
        assert_eq!(written, 12);
        assert_eq!(&dst[..12], &src[..]);
    }

    #[test]
    fn nested_copy_zero_inner_count_copies_only_inner_header() {
        let src = [0xA0, 0xA1, 1, 0xB0, 0xB1, 0, 0x99];
        let mut dst = [0u32; 8];
        let written = copy_nested_records(&src, &mut dst);
        assert_eq!(written, 6, "blez a2 skips the body");
        assert_eq!(&dst[..6], &src[..6]);
        assert_eq!(dst[6], 0);
    }

    #[test]
    fn nested_copy_stops_when_destination_is_short() {
        let src = [0xA0, 0xA1, 1, 0xB0, 0xB1, 1, 0x30, 0x31];
        let mut dst = [0u32; 4];
        let written = copy_nested_records(&src, &mut dst);
        assert_eq!(written, 4, "no overrun past dst");
        assert_eq!(&dst, &[0xA0, 0xA1, 1, 0xB0]);
    }

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

    #[test]
    fn grey_replicates_luminance_into_all_channels() {
        // White (all 0x1F): sum 0x5D >> 2 = 0x17, clamp keeps 0x17.
        let g = bgr555_to_grey(0x7FFF);
        let lum = g & 0x1F;
        assert_eq!(lum, 0x17);
        assert_eq!((g >> 5) & 0x1F, lum);
        assert_eq!((g >> 10) & 0x1F, lum);
        assert_eq!(g & 0x8000, 0, "STP bit is never set by this variant");
    }

    #[test]
    fn grey_black_stays_black() {
        assert_eq!(bgr555_to_grey(0x0000), 0);
    }

    #[test]
    fn grey_averages_the_three_5bit_channels_floor_divided_by_four() {
        // r=0x1F, g=0, b=0 -> sum 0x1F >> 2 = 7.
        let g = bgr555_to_grey(0x001F);
        assert_eq!(g & 0x1F, 7);
        assert_eq!(g, 7 | (7 << 5) | (7 << 10));
    }

    #[test]
    fn grey_output_is_a_valid_15bit_word_with_equal_channels() {
        // A single 555 pixel can never exceed lum 0x17, so the top bit stays
        // clear and all three channels always match for every input.
        for p in [0x7FFFu16, 0x03FF, 0x7C1F, 0x0000] {
            let g = bgr555_to_grey(p);
            let lum = g & 0x1F;
            assert_eq!((g >> 5) & 0x1F, lum);
            assert_eq!((g >> 10) & 0x1F, lum);
            assert!(g < 0x8000, "no STP bit, so top bit clear");
        }
    }
}
