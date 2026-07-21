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

/// The prologue **palette-collapse law** applied to one BGR555 word - the
/// retail gold grade's asset half, mirrored from the `palette_law_word` WGSL
/// helper.
///
/// Capture-pinned: during the retail opening (recomp, cold boot) every CLUT
/// entry the `opdeene` scene uploaded reads back as this function of the disc
/// TIM's entry - `L = max(r, g, b)`, output `(L, max(L - 1, 0), L >> 1)`,
/// STP bit preserved - with zero mismatches across the graded terrain rows.
/// Because a 4/8bpp texel *is* a palette entry, applying the law per decoded
/// texel is exactly equivalent to the CLUT rewrite.
pub fn palette_law(word: u16) -> u16 {
    let r = word & 0x1F;
    let g = (word >> 5) & 0x1F;
    let b = (word >> 10) & 0x1F;
    let l = r.max(g).max(b);
    (word & 0x8000) | ((l >> 1) << 10) | (l.saturating_sub(1) << 5) | l
}

/// The packet-colour half of the palette grade: collapse a loaded TMD colour
/// word (`0..=255` byte units) to `gold * max(r, g, b)`, leaving the exact
/// neutral `0x80` word untouched. Mirrors `palette_collapse_prim` (WGSL).
///
/// The retail opening's GP0 stream shows loaded-TMD prims drawing an amber
/// family `~(M, 0.94*M, 0.43*M)` while the runtime-emitted ground quads keep
/// their neutral `0x80,0x80,0x80` modulation - the two facts this split
/// reproduces.
pub fn palette_collapse(colour: [u8; 3], gold: [f32; 3]) -> [f32; 3] {
    if colour == [NEUTRAL; 3] {
        return [f32::from(NEUTRAL); 3];
    }
    let m = f32::from(colour[0].max(colour[1]).max(colour[2]));
    [gold[0] * m, gold[1] * m, gold[2] * m]
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

    /// The palette-collapse law's fixed points and shape: black and the
    /// STP-only word are unchanged (transparency judgements survive the law),
    /// a pure green entry collapses to the warm `(L, L-1, L/2)` gold, and a
    /// grey ramp keeps its luminance while gaining the gold chroma.
    #[test]
    fn palette_law_shape() {
        assert_eq!(palette_law(0x0000), 0x0000);
        assert_eq!(palette_law(0x8000), 0x8000);
        // Pure green, L = 20: -> (20, 19, 10).
        let w = 20u16 << 5;
        let out = palette_law(w);
        assert_eq!(out & 0x1F, 20);
        assert_eq!((out >> 5) & 0x1F, 19);
        assert_eq!((out >> 10) & 0x1F, 10);
        // Grey keeps its level on the R channel (L = the grey level).
        for l in 0u16..32 {
            let grey = l | (l << 5) | (l << 10);
            let g = palette_law(grey);
            assert_eq!(g & 0x1F, l, "R carries the luminance");
            assert_eq!((g >> 5) & 0x1F, l.saturating_sub(1));
            assert_eq!((g >> 10) & 0x1F, l >> 1);
        }
        // STP bit rides through on a coloured entry too.
        assert_eq!(palette_law(0x8000 | w) & 0x8000, 0x8000);
    }

    /// Packet-colour collapse: exact neutral is the fixed point (the ground
    /// tile kernel's runtime-emitted word), everything else lands on the
    /// gold ray scaled by the max component - the amber family the retail
    /// opening's GP0 stream carries.
    #[test]
    fn palette_collapse_neutral_fixed_point_and_gold_ray() {
        let gold = [1.0, 0.94, 0.43];
        approx(palette_collapse([NEUTRAL; 3], gold), [128.0; 3]);
        // A full-colour authored word collapses to max * gold.
        let out = palette_collapse([0x38, 0x60, 0x18], gold);
        approx(out, [96.0, 90.24, 41.28]);
        // One-off from neutral is NOT the fixed point - only the exact
        // synthetic word is.
        let off = palette_collapse([0x80, 0x80, 0x7F], gold);
        assert!((off[2] - 128.0 * 0.43).abs() < 1e-3);
    }

    /// The WGSL twins of the palette law must stay present and in shape.
    #[test]
    fn wgsl_carries_the_palette_law() {
        let src = crate::shaders::PSX_DITHER_WGSL;
        assert!(src.contains("fn palette_law_word"));
        assert!(src.contains("fn palette_collapse_prim"));
        // The law's three channel expressions.
        assert!(src.contains("let l = max(r, max(g, b));"));
        assert!(src.contains("((l >> 1u) << 10u)"));
        // The neutral fixed point on the packet side.
        assert!(src.contains("prim.r == 128.0 && prim.g == 128.0 && prim.b == 128.0"));
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
        assert!(src.contains("mix(rgb, far_rgb, ir0)"));
        // The per-render-node depth-cue ramp: disabled (`enable <= 0`) must
        // fall back to the constant IR0 so the unfogged default stays the
        // identity.
        assert!(src.contains("fn cue_ramp_ir0"));
        assert!(src.contains("if (ramp.w <= 0.0) {\n        return cue_a;\n    }"));
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
