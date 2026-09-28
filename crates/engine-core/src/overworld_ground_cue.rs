//! The kingdom overworld ground's depth cue: the colour `FUN_801F89B8`
//! (PROT 0901) writes into each continent cell's `POLY_FT4`.
//!
//! The emitter is a leaf with no `ctc2`. Per cell it loads the colour word at
//! scratchpad `0x1F800398` into `RGBC`, sets `IR0` from the depth of the
//! cell's second corner and runs `DPCS`, storing the result as the packet
//! colour (`0x801F8D7C..0x801F8DBC`):
//!
//! ```text
//! 801f8d7c  lw    s2, 0x84(t6)     ; *0x1F800398 (t6 = 0x1F800314)
//! 801f8d88  mfc2  s1, SZ1
//! 801f8d8c  mtc2  s2, RGBC
//! 801f8d90  addi  s1, s1, -0x5000
//!           bgtz  s1 / move s1, zero   ; max(SZ1 - 0x5000, 0)
//! 801f8da0  srl   s1, s1, 3
//! 801f8da4  mtc2  s1, IR0
//! 801f8db0  dpcs
//! 801f8dbc  swc2  RGB2, 4(t5)      ; the packet's colour word
//! ```
//!
//! `SZ1` is the first vertex of the column step's second `RTPT`
//! (`0x801F8BA8`), whose `V0` is the floor tile one column on at the cell's
//! own row: the cell corner `(x1, z0)` - vertex 1 of the cell in the
//! [`legaia_asset::field_objects::WalkHeightfield`] order. `IR0` is not
//! clamped; `DPCS` saturates the colour instead.
//!
//! **Far colour.** The emitter's caller `FUN_801F69D8` calls `SetFarColor`
//! (`FUN_8005B7D8`, three `ctc2` of the arguments `<< 4`) with the literal
//! `(0x100, 0x100, 0x100)` at `0x801F729C`, after its decoration-cell loop
//! and before the `jal 0x801F89B8` at `0x801F733C`; nothing between the two
//! touches the GTE control file. So the ground hazes toward a far colour of
//! `0x1000` per channel - one past white - fixed in code, not read from any
//! scene or kingdom data. (The decoration cells the same routine submits
//! through `FUN_80043390` carry `a1 = 0x00D0D0D0` instead, so they haze
//! toward `0xD0`.)
//!
//! **Base colour.** `FUN_80026CE4` rewrites the low 24 bits of
//! `*0x1F800398` every frame from the word `0x8007B7B0`
//! (`0x80026D38..0x80026D60`, command byte `0x2C`). Every catalogued
//! overworld state holds `0x808080` there, and no scene MAN carries the
//! field-VM write to it (`4C 10`, zero sites disc-wide), so on the overworld
//! the base is the neutral `0x80` the port's ground cells already bake
//! ([`legaia_asset::field_objects::GROUND_PRIM_COLOR`]).
//!
//! Captured on `karisto_sol_pre_encounter` (PCSX-Redux, exec breakpoints at
//! the emitter's entry, the instruction after the `DPCS`, and the
//! `SetFarColor` site; `scripts/pcsx-redux/autorun_overworld_ground_far_colour.lua`):
//! `RFC = GFC = BFC = 0x1000` at every entry and every `DPCS`, `RGBC =
//! 0x2C808080 = *0x1F800398`, and [`ground_cue_color`] reproduces every one of six
//! thousand logged packet colours (`0x80..=0xA0`) from the logged `SZ1`.
//!
//! The mesh shaders apply this per cell on the overworld
//! (`OVERWORLD_GROUND_CUE_WGSL` in `engine-render`'s VRAM-mesh vertex stage,
//! `overworldGroundCue` in the play page's GLSL), reprojecting the corner
//! from the flat-depth references
//! ([`crate::overworld_draw_order::ground_flat_refs`]).

/// `SZ1` below this leaves the ground at its base colour
/// (`addi s1, s1, -0x5000` at `0x801F8D90`).
pub const GROUND_CUE_NEAR_SZ: u32 = 0x5000;

/// Right shift from `SZ1 - GROUND_CUE_NEAR_SZ` to `IR0`
/// (`srl s1, s1, 3` at `0x801F8DA0`).
pub const GROUND_CUE_IR0_SHIFT: u32 = 3;

/// The GTE far colour the ground is drawn with, per channel, in the control
/// registers' units (`SetFarColor(0x100, 0x100, 0x100)` at `0x801F729C`,
/// stored `<< 4`).
pub const GROUND_FAR_COLOR: i32 = 0x100 << 4;

/// `IR0` for a cell whose second corner projects at `sz1`.
pub fn ground_cue_ir0(sz1: u32) -> i32 {
    (sz1.saturating_sub(GROUND_CUE_NEAR_SZ) >> GROUND_CUE_IR0_SHIFT) as i32
}

/// One channel through `DPCS` with `sf = 1`, `lm = 0`: `IR = sat16((FC << 12
/// - C << 16) >> 12)`, `MAC = C << 16 + IR * IR0`, colour `= sat8((MAC >> 12)
/// >> 4)`. `far` is in the control registers' units (`8.4` fixed point).
pub fn dpcs_channel(c: u8, far: i32, ir0: i32) -> u8 {
    let base = i64::from(c) << 16;
    let ir = (((i64::from(far) << 12) - base) >> 12).clamp(-0x8000, 0x7FFF);
    let mac = (base + ir * i64::from(ir0)) >> 12;
    (mac >> 4).clamp(0, 255) as u8
}

/// The packet colour `FUN_801F89B8` writes for a cell of base colour `base`
/// whose second corner (`(x1, z0)`) projects at `sz1`.
///
/// REF: FUN_801F89B8
pub fn ground_cue_color(base: [u8; 3], sz1: u32) -> [u8; 3] {
    let ir0 = ground_cue_ir0(sz1);
    base.map(|c| dpcs_channel(c, GROUND_FAR_COLOR, ir0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(SZ1, IR0, packet colour)` rows logged at `0x801F8DB4` on
    /// `karisto_sol_pre_encounter` (`RGBC = 0x2C808080`, far `0x1000`): the
    /// first row seen at each of a spread of colours.
    const CAPTURED: [(u32, i32, u8); 8] = [
        (0x1588, 0x000, 0x80),
        (0x51C9, 0x039, 0x81),
        (0x5310, 0x062, 0x83),
        (0x5806, 0x100, 0x88),
        (0x60F4, 0x21E, 0x90),
        (0x6860, 0x30C, 0x98),
        (0x6F12, 0x3E2, 0x9F),
        (0x7013, 0x402, 0xA0),
    ];

    #[test]
    fn captured_rows_reproduce() {
        for (sz1, ir0, rgb) in CAPTURED {
            assert_eq!(ground_cue_ir0(sz1), ir0, "SZ1 {sz1:#x}");
            assert_eq!(ground_cue_color([0x80; 3], sz1), [rgb; 3], "SZ1 {sz1:#x}");
        }
    }

    #[test]
    fn near_ground_keeps_its_base_and_far_ground_saturates() {
        assert_eq!(ground_cue_color([0x80; 3], 0), [0x80; 3]);
        assert_eq!(ground_cue_color([0x80; 3], GROUND_CUE_NEAR_SZ), [0x80; 3]);
        // 128 + IR0 / 32 with IR0 = (SZ1 - 0x5000) >> 3: white at 0xCF00.
        assert_eq!(ground_cue_color([0x80; 3], 0xCEFF), [0xFE; 3]);
        assert_eq!(ground_cue_color([0x80; 3], 0xCF00), [0xFF; 3]);
        assert_eq!(ground_cue_color([0x80; 3], 0xFFFF), [0xFF; 3]);
    }

    #[test]
    fn dpcs_matches_the_closed_form_at_the_neutral_base() {
        for sz1 in (0..0xFFFFu32).step_by(97) {
            let ir0 = ground_cue_ir0(sz1);
            let want = (128 + (ir0 >> 5)).min(255) as u8;
            assert_eq!(
                dpcs_channel(0x80, GROUND_FAR_COLOR, ir0),
                want,
                "SZ1 {sz1:#x}"
            );
        }
    }
}
