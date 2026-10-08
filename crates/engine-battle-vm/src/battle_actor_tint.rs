//! Battle-actor **tint pass**: the colour word (`+0x74`) and blend weight
//! (`+0x78`) the battle draw hands the GTE depth cue for each body.
//!
//! PORT: FUN_8004a908
//!
//! Live through `legaia_engine_core::world::World::battle_actor_draw_plan`,
//! which both play hosts call for every battle body every frame. The draw
//! pass `FUN_80048A08` stages `+0x74` as the GTE far colour and `+0x78` as
//! `IR0` (`gp[0x9D8]` / `gp[0x9DC]`), so a body's prims come out as
//! `baked + (far - baked) * IR0 / 0x1000` with the texel still multiplied
//! through - the hosts' `DrawCue` with a saturated ramp.
//!
//! Source: `ghidra/scripts/funcs/8004a908.txt` (disassembly). `s3` is the
//! render node, `s1 = *(DAT_801C9370 + seat*4)` the seated battle actor.
//!
//! # The arms, in order
//!
//! 1. **Cursor dim** (`s1[+0x21C] == 0xC8`, `0x8004A948`): colour
//!    `0x808080` when the seat's formation cell `0x8007BD09 + seat` holds
//!    monster id `0xA8`, else `0x010101`; weight `0x1000`, also written back
//!    to `s1[+0x0C]`. Nothing else runs.
//! 2. **No lanes** (`s1[+0x04] == 0`): the colour keeps only its top byte and
//!    the weight is untouched. The draw tick then skips the body unless its
//!    grey gate stamps it (`crate::battle_actor_tick`).
//! 3. Otherwise the node's position is transformed (`FUN_8003D344`, one
//!    `MVMVA` into `+0x2C..+0x34`) and `a3 = view_z / 16` (rounded toward
//!    zero) is set against `a2 = radius / 2`, the radius being
//!    `*(s1[+0x22C]) + 0x58` - the body radius the battle separation also
//!    reads. With `a3 < a2` (**near**, an unsigned compare), or a live blend
//!    `s1[+0x0C] != 0`, or the context byte `ctx[+0x243]` set, the colour is
//!    the lanes packed `>> 2` to 8 bits a channel, and the weight is the
//!    blend if set, else `a3 * 4` (near) or `a2 * 4`.
//! 4. **Depth cue** (far, no blend, `ctx[+0x243]` clear): each 10-bit lane is
//!    scaled by `a2 / a3` ([`depth_cue_scale_channel`]: clamped to the lane,
//!    floored at `4`), and the weight is `3 * (2*a3 - a2)` truncated to a
//!    halfword and saturated at `0x1000`. A far body is pushed toward a
//!    darker copy of its own colour - retail's battle distance fade.
//! 5. **Negative** (`DAT_8007BDA8` set, `s1[+0x21C] == 0`, the three channels
//!    equal): the colour is complemented ([`invert_bgr24`]) and the weight
//!    shifted `>> 3`. `DAT_8007BDA8` is the outdoor-stage flag
//!    (`battle_ground_grid::OutdoorCueTable`), so on the thirteen outdoor
//!    stages a far body is pushed a little *brighter* instead.
//! 6. **Status** (`s1[+0x16E]`): bit `0x1` -> `0xFF2020`, bit `0x2` ->
//!    `0xFF0420`, any of `0x380` -> `0xF020F0`, each with weight `0x800`,
//!    later bits winning.
//! 7. Bit 26 of the word: cleared when `a3 < 0x180` and none of
//!    `0x8300_0000` is set; otherwise set unless the seat is `7`.
//! 8. **Fade** (`s1[+0x226] != 0`): red and green scaled by
//!    `(0x80 - fade) / 0x80`, blue dropped, top byte `0x81` (additive).
//!
//! # Measured
//!
//! Recomputed from the seated-actor fields of 97 catalogued battle states
//! (every mednafen and PCSX-Redux battle capture in `scripts/scenarios.toml`),
//! this routine reproduces the node's stored `+0x74` / `+0x78` exactly for
//! 258 of the 266 drawn battle bodies; the depth-cue arm is taken by 54 of
//! them. The eight that differ are frames where a later writer touched the
//! node after the draw (a hit flash's `0x85......` word, a battle still
//! loading).

// ---------------------------------------------------------------------------
// FUN_8004a908 - actor depth-cue brightness + negative-colour recolour
// ---------------------------------------------------------------------------

/// Scale one 10-bit colour channel by a depth ratio `num/den`, clamped so a
/// near actor never brightens past its base and a far one never fades to
/// black - the per-channel core of the actor colour/OTZ setup `FUN_8004a908`.
///
/// PORT: FUN_8004a908
///
/// The retail routine computes, per channel, exactly (`0x8004aac8`):
///
/// - `p = (raw * num) / den` - an unsigned 10-bit `*` 16-bit product then
///   `divu` (`den` is the transformed depth `>> 4`; `num` is the mesh's
///   half-range `mesh[+0x58]`).
/// - `if raw < p { p = raw }` - clamp to the base channel, so an actor closer
///   than the half-range stays at full brightness rather than over-driving.
/// - `if p == 0 { p = 4 }` - a dim floor; a fully-faded channel still shows a
///   4/1024 ember rather than pure black.
///
/// Retail guarantees `den >= 1` (the caller replaces a zero depth with 1
/// before this runs); this port maps `den == 0` to `1` to match. The result
/// is the scaled 10-bit value; retail then quantises it to 8 bits per channel
/// (`>> 2` / `& 0x3FC`) when packing the GPU colour word - that packing, and
/// the `FUN_8003d344` GTE transform the depth comes from, are render-track and
/// live in `docs/subsystems/battle.md`.
pub fn depth_cue_scale_channel(raw10: u16, num: u16, den: u16) -> u16 {
    let raw = (raw10 & 0x3FF) as u32;
    let den = den.max(1) as u32;
    let mut p = (raw * num as u32) / den;
    if raw < p {
        p = raw;
    }
    if p == 0 {
        p = 4;
    }
    p as u16
}

/// Invert the low 24 bits (`0x00BBGGRR`) of a packed actor colour word while
/// preserving the top byte - the "negative colour" status recolour from
/// `FUN_8004a908`.
///
/// PORT: FUN_8004a908
///
/// Off the disassembly (`0x8004abf4`): `out = (0xFFFFFF - (c & 0xFFFFFF)) |
/// (c & 0xFF000000)`. Retail applies it only when the three channels are
/// already equal (a greyscale word) - that guard belongs to the caller; this
/// function is the recolour itself, which is a plain complement of the colour
/// bits with the GPU code/attribute byte in bits 24..31 left intact.
pub fn invert_bgr24(color: u32) -> u32 {
    (0x00FF_FFFF - (color & 0x00FF_FFFF)) | (color & 0xFF00_0000)
}

/// `s1[+0x21C]` value of the target cursor's dimmed monsters.
pub const RENDER_FLAG_CURSOR_DIM: u8 = 0xC8;
/// Formation monster id the cursor-dim arm keeps neutral instead of black.
pub const CURSOR_DIM_NEUTRAL_MONSTER: u8 = 0xA8;
/// Full blend weight (`1.0` in the GTE's 4.12 `IR0`).
pub const WEIGHT_ONE: u16 = 0x1000;
/// Weight the status arms stamp.
pub const STATUS_WEIGHT: u16 = 0x800;
/// Colour a status bit `0x1` stamps.
pub const STATUS_COLOUR_1: u32 = 0x00FF_2020;
/// Colour a status bit `0x2` stamps.
pub const STATUS_COLOUR_2: u32 = 0x00FF_0420;
/// Colour any status bit of `0x380` stamps.
pub const STATUS_COLOUR_380: u32 = 0x00F0_20F0;
/// View depth (in `/16` units) below which bit 26 may be cleared.
pub const BIT26_NEAR_DEPTH: u32 = 0x180;
/// The colour word's bit 26.
pub const BIT26: u32 = 0x0400_0000;
/// Top-byte bits that keep bit 26 considered even for a near body.
pub const BIT26_KEEP_MASK: u32 = 0x8300_0000;
/// Top byte the fade arm stamps.
pub const FADE_TOP: u32 = 0x8100_0000;

/// Everything the tint pass reads, over typed fields.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BattleTintInputs {
    /// `s1[+0x04]` - three 10-bit colour lanes (`0x20080200` = neutral).
    pub lanes: u32,
    /// `s1[+0x08] & 0xFF00_0000` - the top byte packed onto the word.
    pub top: u32,
    /// `s1[+0x0C]` - the tint blend.
    pub blend: u32,
    /// `s1[+0x21C]` - the presentation / render flag.
    pub render_flag: u8,
    /// `s1[+0x16E]` - the status word.
    pub status: u16,
    /// `s1[+0x226]` - the fade amount.
    pub fade: u8,
    /// `*(s1[+0x22C]) + 0x58` - the body radius.
    pub radius: i16,
    /// The node's view-space depth (`+0x34` after the `MVMVA`).
    pub view_z: i32,
    /// `+0x5A` - the retail seat.
    pub seat: i16,
    /// `0x8007BD09 + seat` - the seat's formation cell.
    pub formation_cell: u8,
    /// `DAT_8007BDA8` - the outdoor-stage flag.
    pub outdoor: bool,
    /// `ctx[+0x243]` non-zero.
    pub ctx_243: bool,
    /// The node's current `+0x74`, whose top byte the no-lanes arm keeps.
    pub prev_colour: u32,
    /// The node's current `+0x78`, which the no-lanes arm leaves.
    pub prev_weight: u16,
}

/// Which arm set the colour - for tests and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TintArm {
    /// Arm 1.
    CursorDim,
    /// Arm 2.
    NoLanes,
    /// Arm 3.
    Plain,
    /// Arm 4.
    DepthCue,
}

/// The tint pass's writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleTint {
    /// `+0x74`.
    pub colour: u32,
    /// `+0x78`.
    pub weight: u16,
    /// The arm that produced the base colour.
    pub arm: TintArm,
    /// `Some(0x1000)` when the cursor-dim arm writes `s1[+0x0C]` back.
    pub blend_writeback: Option<u32>,
}

impl BattleTint {
    /// The colour's three 8-bit channels, `[r, g, b]`.
    pub fn rgb(&self) -> [u8; 3] {
        [
            self.colour as u8,
            (self.colour >> 8) as u8,
            (self.colour >> 16) as u8,
        ]
    }
}

/// `a3 = view_z / 16`, rounded toward zero (`bgez` / `addiu 0xf` / `sra 4`).
pub fn depth_units(view_z: i32) -> i32 {
    (if view_z < 0 { view_z + 15 } else { view_z }) >> 4
}

/// Pack three 10-bit lanes into the 8-bit-per-channel word (`>> 2` each).
fn pack_lanes(lanes: u32) -> u32 {
    ((lanes >> 2) & 0xFF) | ((lanes >> 4) & 0xFF00) | ((lanes >> 6) & 0x00FF_0000)
}

/// Run the tint pass.
pub fn battle_actor_tint(i: &BattleTintInputs) -> BattleTint {
    if i.render_flag == RENDER_FLAG_CURSOR_DIM {
        let colour = if i.formation_cell == CURSOR_DIM_NEUTRAL_MONSTER {
            0x0080_8080
        } else {
            0x0001_0101
        };
        return BattleTint {
            colour,
            weight: WEIGHT_ONE,
            arm: TintArm::CursorDim,
            blend_writeback: Some(u32::from(WEIGHT_ONE)),
        };
    }
    if i.lanes == 0 {
        return BattleTint {
            colour: i.prev_colour & 0xFF00_0000,
            weight: i.prev_weight,
            arm: TintArm::NoLanes,
            blend_writeback: None,
        };
    }

    let mut a3 = depth_units(i.view_z);
    let a2 = i32::from(i.radius) >> 1;
    let near = (a3 as u32) < (a2 as u32);
    let (mut colour, mut weight, arm);
    if near || i.blend != 0 || i.ctx_243 {
        colour = pack_lanes(i.lanes) | i.top;
        weight = if i.blend != 0 {
            i.blend as u16
        } else if near {
            (a3 << 2) as u16
        } else {
            (a2 << 2) as u16
        };
        arm = TintArm::Plain;
    } else {
        if a3 == 0 {
            a3 = 1;
        }
        weight = (3 * (2 * a3 - a2)) as u16;
        if weight > WEIGHT_ONE {
            weight = WEIGHT_ONE;
        }
        let ch = |shift: u32| {
            depth_cue_scale_channel(((i.lanes >> shift) & 0x3FF) as u16, a2 as u16, a3 as u16)
                as u32
        };
        let (r, g, b) = (ch(0), ch(10), ch(20));
        colour = (r >> 2) | ((g & 0x3FC) << 6) | ((b & 0x3FC) << 14) | i.top;
        arm = TintArm::DepthCue;
    }

    if i.outdoor && i.render_flag == 0 {
        let (r, g, b) = (colour & 0xFF, (colour >> 8) & 0xFF, (colour >> 16) & 0xFF);
        if r == g && r == b {
            colour = invert_bgr24(colour);
            weight >>= 3;
        }
    }
    let top = i.top;
    if i.status & 0x1 != 0 {
        colour = top | STATUS_COLOUR_1;
        weight = STATUS_WEIGHT;
    }
    if i.status & 0x2 != 0 {
        colour = top | STATUS_COLOUR_2;
        weight = STATUS_WEIGHT;
    }
    if i.status & 0x380 != 0 {
        colour = top | STATUS_COLOUR_380;
        weight = STATUS_WEIGHT;
    }
    // Near with no mode bits, or seat 7: cleared (`0x8004ACB8..0x8004AD08`).
    if ((a3 as u32) < BIT26_NEAR_DEPTH && colour & BIT26_KEEP_MASK == 0) || i.seat == 7 {
        colour &= !BIT26;
    } else {
        colour |= BIT26;
    }
    if i.fade != 0 {
        let k = 0x80 - u32::from(i.fade);
        let r = ((colour & 0xFF) * k) >> 7;
        let g = (((colour >> 8) & 0xFF) * k) >> 7;
        colour = r | (g << 8) | FADE_TOP;
    }
    BattleTint {
        colour,
        weight,
        arm,
        blend_writeback: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEUTRAL: u32 = 0x2008_0200;

    fn body(view_z: i32) -> BattleTintInputs {
        BattleTintInputs {
            lanes: NEUTRAL,
            radius: 640,
            view_z,
            ..Default::default()
        }
    }

    #[test]
    fn a_near_body_keeps_its_lanes_and_weights_by_depth() {
        // a2 = 320; view 4000 -> a3 = 250 < a2.
        let t = battle_actor_tint(&body(4000));
        assert_eq!(t.arm, TintArm::Plain);
        assert_eq!(t.colour & 0xFF_FFFF, 0x80_8080);
        assert_eq!(t.weight, 250 * 4);
        assert_eq!(t.colour & BIT26, 0, "near, no mode bits: bit 26 clear");
    }

    #[test]
    fn a_far_body_takes_the_depth_cue() {
        // Measured: party_battle_gobu_gobu-shaped seat, z 8117, radius 640
        // indoor -> colour 0x50 per channel, weight 0x822 (captured node).
        let t = battle_actor_tint(&body(8117));
        assert_eq!(t.arm, TintArm::DepthCue);
        assert_eq!(t.colour & 0xFF_FFFF, 0x50_5050);
        assert_eq!(t.weight, 0x822);
        assert_ne!(t.colour & BIT26, 0);
    }

    #[test]
    fn outdoor_stages_invert_the_grey_and_shrink_the_weight() {
        // Captured: z 8986, radius 640, outdoor -> 0xB6B6B6 / 0x12C.
        let mut i = body(8986);
        i.outdoor = true;
        let t = battle_actor_tint(&i);
        assert_eq!(t.colour & 0xFF_FFFF, 0xB6_B6B6);
        assert_eq!(t.weight, 0x12C);
    }

    #[test]
    fn a_live_blend_overrides_the_depth_arm() {
        let mut i = body(9000);
        i.blend = 0x1000;
        i.lanes = 0x0E03_831F; // a hit flash's lanes
        let t = battle_actor_tint(&i);
        assert_eq!(t.arm, TintArm::Plain);
        assert_eq!(t.weight, 0x1000);
        assert_eq!(t.colour & 0xFF_FFFF, pack_lanes(0x0E03_831F));
    }

    #[test]
    fn no_lanes_keeps_the_top_byte_and_the_weight() {
        let mut i = body(5000);
        i.lanes = 0;
        i.prev_colour = 0x8123_4567;
        i.prev_weight = 0x333;
        let t = battle_actor_tint(&i);
        assert_eq!(t.arm, TintArm::NoLanes);
        assert_eq!(t.colour, 0x8100_0000);
        assert_eq!(t.weight, 0x333);
    }

    #[test]
    fn cursor_dim_is_black_unless_the_monster_is_a8() {
        let mut i = body(5000);
        i.render_flag = RENDER_FLAG_CURSOR_DIM;
        let t = battle_actor_tint(&i);
        assert_eq!((t.colour, t.weight), (0x01_0101, 0x1000));
        assert_eq!(t.blend_writeback, Some(0x1000));
        i.formation_cell = CURSOR_DIM_NEUTRAL_MONSTER;
        assert_eq!(battle_actor_tint(&i).colour, 0x80_8080);
    }

    #[test]
    fn status_bits_stamp_their_colours_later_bits_winning() {
        let mut i = body(3000);
        i.status = 0x1;
        assert_eq!(battle_actor_tint(&i).colour & 0xFF_FFFF, STATUS_COLOUR_1);
        i.status = 0x3;
        assert_eq!(battle_actor_tint(&i).colour & 0xFF_FFFF, STATUS_COLOUR_2);
        i.status = 0x103;
        let t = battle_actor_tint(&i);
        assert_eq!(t.colour & 0xFF_FFFF, STATUS_COLOUR_380);
        assert_eq!(t.weight, STATUS_WEIGHT);
    }

    #[test]
    fn seat_seven_never_takes_bit_26() {
        let mut i = body(9000);
        i.seat = 7;
        assert_eq!(battle_actor_tint(&i).colour & BIT26, 0);
        i.seat = 3;
        assert_ne!(battle_actor_tint(&i).colour & BIT26, 0);
    }

    #[test]
    fn the_fade_drops_blue_and_goes_additive() {
        let mut i = body(3000);
        i.fade = 0x40;
        let t = battle_actor_tint(&i);
        assert_eq!(t.colour, FADE_TOP | 0x40 | (0x40 << 8));
    }

    #[test]
    fn depth_cue_full_brightness_when_closer_than_half_range() {
        // den (depth) < num (half-range) -> (raw*num)/den >= raw -> clamp to raw.
        assert_eq!(depth_cue_scale_channel(0x200, 0x40, 0x10), 0x200);
    }

    #[test]
    fn depth_cue_dims_when_farther_than_half_range() {
        // raw=0x100, num=8, den=0x20 -> (0x100*8)/0x20 = 0x40.
        assert_eq!(depth_cue_scale_channel(0x100, 8, 0x20), 0x40);
    }

    #[test]
    fn depth_cue_floor_is_four_not_zero() {
        // Product rounds to 0 -> dim floor of 4.
        assert_eq!(depth_cue_scale_channel(1, 1, 0x40), 4);
    }

    #[test]
    fn depth_cue_masks_input_to_ten_bits_and_guards_zero_den() {
        // Bits above bit 9 in raw are dropped before scaling.
        assert_eq!(
            depth_cue_scale_channel(0xFC00 | 0x100, 1, 1),
            depth_cue_scale_channel(0x100, 1, 1)
        );
        // den == 0 is treated as 1 (retail guarantees >= 1 upstream).
        assert_eq!(depth_cue_scale_channel(0x080, 1, 0), 0x080);
    }

    #[test]
    fn invert_complements_colour_bits_keeps_top_byte() {
        assert_eq!(invert_bgr24(0x00_00_00_00), 0x00FF_FFFF);
        assert_eq!(invert_bgr24(0x00_FF_FF_FF), 0x0000_0000);
        // Top byte (GPU code/attr) survives untouched.
        assert_eq!(invert_bgr24(0xC5_10_20_30), 0xC5_EF_DF_CF);
    }

    #[test]
    fn invert_is_self_inverse_on_the_colour_bits() {
        for c in [0x00_12_34_56u32, 0x81_00_80_FF, 0x00_7F_7F_7F] {
            assert_eq!(invert_bgr24(invert_bgr24(c)), c);
        }
    }

    #[test]
    fn depth_units_round_toward_zero() {
        assert_eq!(depth_units(31), 1);
        assert_eq!(depth_units(-31), -1);
        assert_eq!(depth_units(-16), -1);
    }
}
