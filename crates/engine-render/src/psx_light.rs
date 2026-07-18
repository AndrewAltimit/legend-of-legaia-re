//! PSX field lighting: the GPU's texture-blend modulation and the GTE depth
//! cue, as CPU kernels.
//!
//! REF: FUN_8002735C (baked-colour TMD renderer)
//! REF: FUN_80029888 (light-source TMD renderer)
//! REF: FUN_80043390 (ambient -> GTE cr13-15, far colour -> cr21-23)
//!
//! # What retail actually does
//!
//! Both retail TMD renderers walk the same per-mode descriptor table
//! (`DAT_8007326c`) and, between them, issue exactly **one** GTE colour op:
//! `DPCS` (`cop2 0x780010` - real command `0x10`, `sf = 1`), the depth cue.
//! Neither ever issues `NCDS` / `NCDT` / `NCS` / `NCT` / `NCCS` / `NCCT` / `CDP`
//! / `CC`, so **these two renderers consult no light source**: no light matrix,
//! no vertex normal transform. The field object/decoration path emits through a
//! separate dispatcher (`FUN_80043390`) whose kind-8..11 handlers *do* carry `NCC`
//! light ops, so it *could* light in principle - but a cold-boot capture shows it
//! does not. In a live `town01` field a `dirty_exec_hot` sweep of ~46M interpreted
//! instructions (idle + walk) lands entirely in the depth-cue fog body
//! `FUN_80045584` (`DPCT`+`DPCS`), with zero hits in the kind-8..11 NCC band
//! `[0x800445b0,0x80044798)` - specifically zero at the two light-op sites
//! `NCCT` `0x80044724` / `NCCS` `0x80044750`. This matches the battle, summon and
//! `map01` samples: across every robustly-sampled scene the field renders through
//! `DPCS`/`DPCT` depth cue and the NCC handlers are never observed executing. So
//! **this crate's baked-colour + depth-cue model is faithful to the field object
//! path too, not just the TMD mesh path** - retail runs no runtime light source on
//! the field. (The GTE light matrix `L` (cr8-12) and light-colour matrix `LC`
//! (cr16-20) *are* populated - `FUN_8005b648` / `FUN_8005b678` - and the only
//! functions that *statically* consume them via `NCCS`/`NCCT` are the four handlers
//! `FUN_8004409c` / `FUN_8004423c` / `FUN_80044434` / `FUN_800445b0` (dispatch kinds
//! 8..11), which stay dormant at runtime.) Two methodology notes: the recomp's GTE
//! ring records only `RTPS`/`RTPT`/`INTPL` (never `NCCS`/`NCCT`/`DPCS`/`DPCT`), so a
//! GTE-ring "zero NCC" is vacuous and only `dirty_exec_hot` is a valid probe here;
//! and `map01` dispatches through a different overlay table (`0x801F8968`) that never
//! reaches these SCUS handlers. Remaining caveat: the town sweep covered the
//! Mist-era prologue arrival area; `map02`/`map03` and free-roam towns are unreached
//! (blocked by the recomp savestate-load freeze).
//!
//! Field shading is therefore **baked into the TMD**. Every primitive carries a
//! colour word - `[R][G][B][GP0 code]`, the code byte being one of
//! `0x20`/`0x24`/`0x28`/`0x2C`/`0x30`/`0x34`/`0x38`/`0x3C` (plus `| 2` for the
//! semi-transparent variants) - which the renderer loads into the GTE's `RGBC`
//! register, passes through `DPCS`, and hands to the GPU as the packet colour.
//! The GPU then blends it with the texel:
//!
//! ```text
//! out = texel * colour / 128
//! ```
//!
//! so `0x80` is the neutral value (texel unchanged), below darkens, and above
//! brightens - up to `255/128` ~= 2x. That factor-of-two headroom is the whole
//! reason retail's field has more contrast than an unlit render: a town's
//! environment packs put ~81% of their colour components below `0x80` and ~12%
//! above it.
//!
//! # Provenance
//!
//! See `ghidra/scripts/funcs/8002735c.txt` and
//! `ghidra/scripts/funcs/80029888.txt` for the renderers (the `cop2 0x780010`
//! sites and the `ctc2 .., 0xa800/0xb000/0xb800` far-colour writes), and
//! `ghidra/scripts/funcs/80043390.txt` for the fade routine that stages the
//! far colour, the ambient/background colour (cr13-15) and `IR0`.
//!
//! The `IR0 = 0` default is not an assumption: a retail town0c save state's
//! GTE register file shows `RGB.Raw8 = 30 30 30 34` (a gouraud-textured prim,
//! GP0 code `0x34`) and an `RGB_FIFO` of `0x30, 0x60, 0x30` - the prim's three
//! baked corner colours, emerging from the depth-cue op **byte-unchanged**,
//! which only happens at `IR0 = 0`.
//!
//! These kernels mirror `psx_modulate` / `psx_depth_cue` in
//! [`crate::shaders::PSX_DITHER_WGSL`] and are kept in lockstep by the tests
//! below (which also assert the WGSL still carries the same divisor).

/// The PSX GPU's neutral texture-modulation colour: `texel * 0x80 / 128 = texel`.
/// The twin of `legaia_tmd::legaia_prims::MODULATION_NEUTRAL` (this crate does
/// not depend on `legaia-tmd`; the value is a property of the hardware blend).
pub const NEUTRAL: u8 = 0x80;

/// PSX GPU texture blending: `out = texel * colour / 128`, saturating at 1.0.
///
/// `texel` is in `0..=1`; `colour` is the primitive's raw colour byte
/// (`0..=255`). Mirrors the `psx_modulate` WGSL helper.
pub fn modulate(texel: [f32; 3], colour: [u8; 3]) -> [f32; 3] {
    let mut out = [0.0f32; 3];
    for i in 0..3 {
        out[i] = (texel[i] * f32::from(colour[i]) / 128.0).clamp(0.0, 1.0);
    }
    out
}

/// GTE depth cue (`DPCS`): `out = c + (fc - c) * ir0`, saturating at 1.0.
///
/// `far` is the GTE far colour (cr21-23) in `0..=1`; `ir0` is the blend factor
/// in `0..=1` (the hardware's `0..=0x1000`). `ir0 = 0` is the identity - the
/// unfogged field case. Mirrors the `psx_depth_cue` WGSL helper.
pub fn depth_cue(rgb: [f32; 3], far: [f32; 3], ir0: f32) -> [f32; 3] {
    if ir0 <= 0.0 {
        return rgb;
    }
    let mut out = [0.0f32; 3];
    for i in 0..3 {
        out[i] = (rgb[i] + (far[i] - rgb[i]) * ir0).clamp(0.0, 1.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: [f32; 3], b: [f32; 3]) {
        for i in 0..3 {
            assert!((a[i] - b[i]).abs() < 1e-5, "{a:?} != {b:?}");
        }
    }

    /// `0x80` is the GPU's identity: the texel comes out exactly as it went in.
    /// This is why an engine that ignores the prim colour still gets retail's
    /// *median* right - a chunk of the corpus really is authored at `0x80` -
    /// while losing both tails.
    #[test]
    fn neutral_colour_leaves_the_texel_alone() {
        let texel = [0.25, 0.5, 1.0];
        approx(modulate(texel, [NEUTRAL; 3]), texel);
    }

    /// Above `0x80` BRIGHTENS - the headroom the PSX blend has and a plain
    /// `texel * lambert` shade does not. `0xFF` is very nearly a doubling.
    #[test]
    fn colour_above_neutral_brightens_up_to_about_2x() {
        // 0xFF / 128 = 1.9921875
        approx(modulate([0.25, 0.25, 0.25], [0xFF; 3]), [0.49804688; 3]);
        // ...and saturates rather than wrapping.
        approx(modulate([0.9, 0.9, 0.9], [0xFF; 3]), [1.0; 3]);
    }

    /// Below `0x80` darkens, linearly in the colour byte.
    #[test]
    fn colour_below_neutral_darkens() {
        approx(modulate([1.0; 3], [0x40; 3]), [0.5; 3]);
        approx(modulate([1.0; 3], [0x00; 3]), [0.0; 3]);
    }

    /// Hand-worked against a real town0c gouraud-textured prim: descriptor row
    /// 5 (`GT3`, GP0 code `0x34`) with baked corner colours `0x80`, `0x60`,
    /// `0x50` - i.e. the same texel is drawn at 1.0x, 0.75x and 0.625x across
    /// the face. That per-corner ramp IS the shading.
    #[test]
    fn retail_gouraud_corner_ramp() {
        let texel = [0.8, 0.8, 0.8];
        approx(modulate(texel, [0x80; 3]), [0.8; 3]);
        approx(modulate(texel, [0x60; 3]), [0.6; 3]);
        approx(modulate(texel, [0x50; 3]), [0.5; 3]);
    }

    /// The retail-capture oracle: at `IR0 = 0` the depth cue is the identity,
    /// which is what a town0c save state's GTE `RGB_FIFO` shows (the prim's
    /// baked colours `0x30 / 0x60 / 0x30` come back out unchanged).
    #[test]
    fn depth_cue_is_identity_at_ir0_zero() {
        let far = [1.0, 1.0, 1.0];
        for c in [0x30u8, 0x60, 0x30] {
            let v = f32::from(c) / 255.0;
            approx(depth_cue([v; 3], far, 0.0), [v; 3]);
        }
    }

    /// At full blend the colour is replaced by the far colour.
    #[test]
    fn depth_cue_at_full_blend_is_the_far_colour() {
        approx(depth_cue([0.0; 3], [1.0, 0.5, 0.25], 1.0), [1.0, 0.5, 0.25]);
        // Half-way is a plain lerp.
        approx(depth_cue([0.0; 3], [1.0, 1.0, 1.0], 0.5), [0.5; 3]);
    }

    /// The WGSL and the CPU mirror must not drift: the shader has to carry the
    /// `/ 128.0` divisor (the whole point - a `/ 255.0` normalise would silently
    /// turn the blend into a pure darken and throw the brightening tail away)
    /// and the depth-cue lerp.
    #[test]
    fn wgsl_matches_the_cpu_mirror() {
        let src = crate::shaders::PSX_DITHER_WGSL;
        assert!(
            src.contains("texel * colour / 128.0"),
            "psx_modulate must divide by 128 (the PSX texture-blend divisor)"
        );
        assert!(src.contains("fn psx_modulate"));
        assert!(src.contains("fn psx_depth_cue"));
        assert!(src.contains("mix(rgb, cue.rgb, cue.a)"));
        // No always-on synthetic light may creep back into the retail
        // helpers: the ONLY light in the prelude is the opt-in `dyn_light`
        // enhancement, and it must early-return the input unchanged when its
        // enable (`light_dir.w`) is zero - the staged default. The retail
        // modulate/depth-cue bodies stay light-free.
        let retail_end = src.find("fn dyn_light").unwrap_or(src.len());
        let retail =
            &src[..retail_end.min(src.find("Opt-in dynamic-lighting").unwrap_or(src.len()))];
        assert!(
            !retail.contains("lambert") && !retail.contains("light_dir"),
            "no synthetic light may leak into the retail shader helpers"
        );
        assert!(
            src.contains("if (light_dir.w < 0.5) {\n        return rgb;\n    }"),
            "dyn_light must be an exact identity when disabled (the default)"
        );
    }
}
