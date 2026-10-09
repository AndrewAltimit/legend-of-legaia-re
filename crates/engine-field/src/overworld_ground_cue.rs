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

/// The overworld **decoration cells**' depth cue: one `IR0` per object, taken
/// from the depth of the object's origin, toward the far colour `0xD0`.
///
/// The decoration sweep in `FUN_801F69D8` (PROT 0901) transforms each cell's
/// object origin by the camera (`MVMVA` at `0x801F7058`, stored to scratch
/// `0x1F8002E0..E8`), loads the result into `TR` (`0x801F71E0..0x801F71F4`)
/// and forms the dispatcher's third argument from its `TRZ`:
///
/// ```text
/// 801f71f8  lw    v0, 0x40(s1)      ; TRZ (s1 = 0x1F8002A8)
/// 801f7200  addiu a2, v0, -0x5000
///           bgez / clear a2          ; max(TRZ - 0x5000, 0)
///           sra   a2, a2, 3
/// 801f7214  slti  v0, a2, 0x1001
///           li    a2, 0x1000         ; min(.., 0x1000)
/// 801f721c  lui   a1, 0xd0 / ori a1, a1, 0xd0d0   ; a1 = 0x00D0D0D0
/// 801f7254  jal   0x80043390
/// ```
///
/// `FUN_80043390` turns a non-zero `a2` into the GTE far colour (each byte of
/// `a1` `<< 4`, `& 0xFFFE`, into `RFC/GFC/BFC` at `0x800434B0..0x800434D0`)
/// and parks `a2` at scratch `0x1F800038`, which the PROT 0901 prim handlers
/// load into `IR0` before their `DPCS` (`lwc2 IR0, -0x2dc(t2)` at
/// `0x801F7A44`). So every prim of one decoration hazes by the same `IR0`.
///
/// The record's `+0x1E` / `+0x12 & 0x800` bits raise `a1`'s top byte, which
/// makes the dispatcher OR `1` into `a2` (`0x800433C0..0x800433CC`): an
/// `IR0` of at most `1 / 0x1000` more, below one colour step, and not
/// modelled.
///
/// The hosts stage the result as a per-draw constant cue
/// ([`decoration_draw_cue`]). The placed landmarks are not this sweep's
/// (`FUN_8003A55C`'s actors, skipped by the `+0x12 & 4` test at
/// `0x801F6EE8`) and take no cue from it. All eight PROT 0901 prim leaves
/// (`0x801F7644` .. `0x801F8690`, textured and untextured) run the load.
pub const DECORATION_FAR_BYTE: u8 = 0xD0;

/// `TRZ` below this leaves a decoration at its packet colour
/// (`addiu a2, v0, -0x5000` at `0x801F7200`).
pub const DECORATION_CUE_NEAR_Z: i64 = 0x5000;

/// The clamp on a decoration's `IR0` (`slti v0, a2, 0x1001` / `li a2,
/// 0x1000` at `0x801F7214..0x801F7220`).
pub const DECORATION_IR0_MAX: i32 = 0x1000;

/// The far colour a decoration hazes toward, in the control registers'
/// units: the byte `<< 4`, low bit cleared (`0x800434B0..0x800434C4`).
pub const DECORATION_FAR_COLOR: i32 = ((DECORATION_FAR_BYTE as i32) << 4) & 0xFFFE;

/// `IR0` for a decoration whose origin sits at camera-space depth `trz`.
pub fn decoration_cue_ir0(trz: i64) -> i32 {
    (((trz - DECORATION_CUE_NEAR_Z).max(0) >> 3) as i32).min(DECORATION_IR0_MAX)
}

/// The packet colour a decoration prim of colour `base` draws with when its
/// object's origin sits at camera-space depth `trz`.
///
/// REF: FUN_801F69D8
pub fn decoration_cue_color(base: [u8; 3], trz: i64) -> [u8; 3] {
    let ir0 = decoration_cue_ir0(trz);
    base.map(|c| dpcs_channel(c, DECORATION_FAR_COLOR, ir0))
}

/// A decoration draw's cue as the hosts stage it: the far colour in display
/// `0..1` units and `IR0` in `1.0 = 0x1000` units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecorationCue {
    pub far: [f32; 3],
    pub ir0: f32,
}

/// The cue for one decoration draw whose model origin projects at
/// `origin_clip_w` under a frame whose `clip.w`-to-`SZ` factor is `sz_scale`
/// (`legaia_engine_core::overworld_curvature::frame_curve_scale`, the factor the ground
/// cue reads). `None` off the overworld (`sz_scale == 0`) and for a near
/// object (`IR0 == 0`), both of which draw uncued.
pub fn decoration_draw_cue(origin_clip_w: f32, sz_scale: f32) -> Option<DecorationCue> {
    if sz_scale <= 0.0 || !origin_clip_w.is_finite() {
        return None;
    }
    let trz = (origin_clip_w * sz_scale).round() as i64;
    let ir0 = decoration_cue_ir0(trz);
    if ir0 == 0 {
        return None;
    }
    let far = f32::from(DECORATION_FAR_BYTE) / 255.0;
    Some(DecorationCue {
        far: [far; 3],
        ir0: ir0 as f32 / 4096.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoration_ir0_follows_the_origin_depth_and_clamps() {
        assert_eq!(decoration_cue_ir0(-100), 0);
        assert_eq!(decoration_cue_ir0(0x5000), 0);
        assert_eq!(decoration_cue_ir0(0x5007), 0);
        assert_eq!(decoration_cue_ir0(0x5008), 1);
        assert_eq!(decoration_cue_ir0(0x5000 + 8 * 0x800), 0x800);
        assert_eq!(decoration_cue_ir0(0x5000 + 8 * 0x1000), 0x1000);
        assert_eq!(decoration_cue_ir0(0x7FFF_FFFF), 0x1000);
    }

    #[test]
    fn decoration_hazes_toward_d0() {
        assert_eq!(DECORATION_FAR_COLOR, 0xD00);
        assert_eq!(decoration_cue_color([0x80; 3], 0), [0x80; 3]);
        // Saturated IR0 lands on the far colour whatever the base.
        assert_eq!(decoration_cue_color([0x80; 3], 0x20000), [0xD0; 3]);
        assert_eq!(decoration_cue_color([0x10, 0xFF, 0x80], 0x20000), [0xD0; 3]);
        // Half way: 0x80 + (0xD0 - 0x80) / 2.
        assert_eq!(
            decoration_cue_color([0x80; 3], 0x5000 + 8 * 0x800),
            [0xA8; 3]
        );
    }

    #[test]
    fn draw_cue_is_off_the_overworld_and_for_near_objects() {
        assert_eq!(decoration_draw_cue(0x9000 as f32, 0.0), None);
        assert_eq!(decoration_draw_cue(0x4000 as f32, 1.0), None);
        let c = decoration_draw_cue(0x5000 as f32 + 8.0 * 2048.0, 1.0).unwrap();
        assert_eq!(c.ir0, 0.5);
        assert_eq!(c.far, [208.0 / 255.0; 3]);
        // A 1x frame (the field / scripted cameras) scales clip.w up to SZ.
        let s = decoration_draw_cue((0x5000 as f32 + 8.0 * 2048.0) / 6.0, 6.0).unwrap();
        assert!((s.ir0 - 0.5).abs() < 1e-3);
    }

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
