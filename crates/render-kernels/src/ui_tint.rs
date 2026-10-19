//! How a 2D overlay quad's tint lands on a texel: retail's 5-bit texture
//! modulation, and the font CLUTs' own shadow colours.
//!
//! **Modulation is 5-bit.** The GPU multiplies a texel's 5-bit component by
//! the primitive's colour word and truncates, `(T5 * C) >> 7`, before the
//! result reaches the 15-bit framebuffer - so a tinted texel lands on the
//! 5-bit grid, rounded down. An 8-bit multiply lands up to a grid step
//! higher: the pause menu's marbled window fill (class-0 bands, a grey
//! gouraud ramp over the patch) drew `(21, 29, 125)` where retail shows
//! `(16, 24, 123)`. [`modulate5`] is the retail law; an overlay colour
//! `c` stands for the colour word `C = c * 128`.
//!
//! **A font shadow is its CLUT's own entry.** The dialog font page uses three
//! palette indices: `0` transparent, `14` the drop shadow, `15` the fill. A
//! text ink is a CLUT pick (`DAT_8007B454`, CLUT `(96 + 16 * ink, 510)`),
//! so the shadow changes with the ink: grey `(4, 4, 4)` under the tier-7
//! grey, `(8, 8, 4)` under gold, `(4, 8, 8)` under teal ([`INK_SHADOWS`],
//! read off a retail frame's VRAM). The engine's font atlas whitewashes the
//! fill and marks shadow texels with alpha [`legaia_font::FONT_SHADOW_ALPHA`];
//! a marked texel under a known ink draws that ink's shadow, unmodulated.
//!
//! One law, three consumers: the native text shader (its WGSL is generated
//! from this table by [`wgsl_ui_tint`]), the browser overlay's tinted-atlas
//! cache ([`tint_atlas_rgb`], through the wasm runtime), and the CPU paths
//! that rasterise the same draws ([`tint_texel`]).

use legaia_font::FONT_SHADOW_ALPHA;

/// Slack added before truncating `T5 * c`, so an ink stored as `T / 31` in
/// `f32` lands on `T` and not one below it.
pub const QUANT_EPS: f32 = 1.0e-3;

/// One font CLUT's fill (entry 15) and drop shadow (entry 14), 5-bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InkShadow {
    /// The `DAT_8007B454` ink index the CLUT is staged by.
    pub ink: u8,
    /// Entry 15: the glyph fill, which is the ink colour.
    pub fill: [u8; 3],
    /// Entry 14: the drop shadow under that fill.
    pub shadow: [u8; 3],
}

/// The font CLUT row at VRAM y `510`, `x = 96 + 16 * ink`, entries 15 / 14,
/// as a retail frame holds them (`menu_options_field`'s VRAM). Ink 8 shares
/// ink 7's fill with a lighter shadow and is left out: an engine ink of that
/// grey is ink 7, the one every menu and battle caption stages.
pub const INK_SHADOWS: [InkShadow; 9] = [
    InkShadow {
        ink: 0,
        fill: [16, 16, 16],
        shadow: [4, 4, 4],
    },
    InkShadow {
        ink: 1,
        fill: [13, 13, 28],
        shadow: [3, 3, 5],
    },
    InkShadow {
        ink: 2,
        fill: [28, 4, 0],
        shadow: [8, 4, 4],
    },
    InkShadow {
        ink: 3,
        fill: [28, 13, 28],
        shadow: [8, 2, 8],
    },
    InkShadow {
        ink: 4,
        fill: [13, 27, 13],
        shadow: [3, 8, 3],
    },
    InkShadow {
        ink: 5,
        fill: [8, 27, 27],
        shadow: [4, 8, 8],
    },
    InkShadow {
        ink: 6,
        fill: [28, 21, 0],
        shadow: [8, 8, 4],
    },
    InkShadow {
        ink: 7,
        fill: [25, 25, 25],
        shadow: [4, 4, 4],
    },
    InkShadow {
        ink: 9,
        fill: [27, 11, 0],
        shadow: [8, 6, 3],
    },
];

/// How close `c * 31` must sit to a CLUT fill for the tint to be that ink.
const INK_MATCH: f32 = 0.01;

/// A 5-bit component as the framebuffer shows it, `(v << 3) | (v >> 2)`.
pub const fn expand5(v: u8) -> u8 {
    (v << 3) | (v >> 2)
}

/// The 5-bit ink a tint colour component stands for: `T / 31`.
pub const fn ink_component(t: u8) -> f32 {
    t as f32 / 31.0
}

/// Retail's texture modulation of one 8-bit texel component by tint `c`
/// (`c = C / 128`): truncate the texel to 5 bits, multiply, truncate, clamp
/// to 31, show it as 8-bit.
pub fn modulate5(texel: u8, c: f32) -> u8 {
    let t5 = f32::from(texel >> 3);
    let o = (t5 * c + QUANT_EPS).floor().clamp(0.0, 31.0);
    expand5(o as u8)
}

/// The ink a tint colour is, when it is one of the font CLUTs' fills.
pub fn ink_of(rgb: [f32; 3]) -> Option<&'static InkShadow> {
    INK_SHADOWS
        .iter()
        .find(|e| (0..3).all(|k| (rgb[k] * 31.0 - f32::from(e.fill[k])).abs() < INK_MATCH))
}

/// The 8-bit shadow colour a font glyph drawn in `rgb` carries, when `rgb`
/// is a known ink.
pub fn ink_shadow(rgb: [f32; 3]) -> Option<[u8; 3]> {
    ink_of(rgb).map(|e| e.shadow.map(expand5))
}

/// One texel through a tint, colour only (`[r, g, b, texel alpha]`): a font
/// shadow under a known ink takes that ink's shadow and is opaque; an opaque
/// texel is modulated 5-bit when `quantize`; everything else takes the plain
/// 8-bit multiply.
///
/// A partly transparent texel (`0 < alpha < 255`) is an engine stand-in for a
/// retail semi-transparency (the dialogue box's pre-baked `B/2 + F/2` pair is
/// one alpha-191 sprite), whose own 5-bit arithmetic the alpha blend does not
/// reproduce, so it stays on the 8-bit multiply.
pub fn tint_texel(texel: [u8; 4], rgb: [f32; 3], quantize: bool) -> [u8; 4] {
    let mut a = texel[3];
    if a == FONT_SHADOW_ALPHA {
        a = 255;
        if let Some(s) = ink_shadow(rgb) {
            return [s[0], s[1], s[2], a];
        }
    }
    let quantize = quantize && a == 255;
    let mut out = [0u8, 0, 0, a];
    for k in 0..3 {
        out[k] = if quantize {
            modulate5(texel[k], rgb[k])
        } else {
            (f32::from(texel[k]) * rgb[k]).round().clamp(0.0, 255.0) as u8
        };
    }
    out
}

/// A whole RGBA8 atlas through [`tint_texel`]: the browser overlay's
/// per-colour tinted copy.
pub fn tint_atlas_rgb(rgba: &[u8], rgb: [f32; 3], quantize: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    for px in rgba.as_chunks::<4>().0 {
        out.extend_from_slice(&tint_texel(*px, rgb, quantize));
    }
    out
}

/// The WGSL twin of this module for the native text shader: `ui_tint(texel,
/// tint, quantize)` returning the tinted colour (alpha untouched, a font
/// shadow's made opaque), with [`INK_SHADOWS`] compiled in.
pub fn wgsl_ui_tint() -> String {
    let mut s = String::new();
    s.push_str(
        "// Generated from legaia_render_kernels::ui_tint - edit the Rust table, not this.\n",
    );
    s.push_str(&format!(
        "const UI_QUANT_EPS: f32 = {QUANT_EPS:?};\nconst UI_SHADOW_ALPHA: f32 = {}.0;\n",
        FONT_SHADOW_ALPHA
    ));
    s.push_str(
        "fn ui_expand5(o: vec3<f32>) -> vec3<f32> {\n    return (o * 8.0 + floor(o / 4.0)) / 255.0;\n}\n",
    );
    s.push_str(
        "fn ui_modulate5(t: vec3<f32>, c: vec3<f32>) -> vec3<f32> {\n    let t5 = floor(round(t * 255.0) / 8.0);\n    let o = clamp(floor(t5 * c + vec3<f32>(UI_QUANT_EPS)), vec3<f32>(0.0), vec3<f32>(31.0));\n    return ui_expand5(o);\n}\n",
    );
    s.push_str("fn ui_ink_shadow(c: vec3<f32>) -> vec4<f32> {\n    let k = c * 31.0;\n");
    for e in INK_SHADOWS {
        s.push_str(&format!(
            "    if (all(abs(k - vec3<f32>({}.0, {}.0, {}.0)) < vec3<f32>({INK_MATCH:?}))) {{ return vec4<f32>(ui_expand5(vec3<f32>({}.0, {}.0, {}.0)), 1.0); }}\n",
            e.fill[0], e.fill[1], e.fill[2], e.shadow[0], e.shadow[1], e.shadow[2]
        ));
    }
    s.push_str("    return vec4<f32>(0.0, 0.0, 0.0, -1.0);\n}\n");
    s.push_str(
        "fn ui_tint(texel: vec4<f32>, c: vec3<f32>, quantize: bool) -> vec4<f32> {\n    var a = texel.a;\n    if (abs(texel.a * 255.0 - UI_SHADOW_ALPHA) < 0.5) {\n        a = 1.0;\n        let s = ui_ink_shadow(c);\n        if (s.w > 0.0) { return vec4<f32>(s.rgb, a); }\n    }\n    if (quantize && a > 0.999) { return vec4<f32>(ui_modulate5(texel.rgb, c), a); }\n    return vec4<f32>(c * texel.rgb, a);\n}\n",
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window fill retail shows, from the texels and the band grey: the
    /// engine's 8-bit multiply drew each one above the 5-bit grid.
    #[test]
    fn modulation_truncates_to_the_5_bit_grid() {
        // A grey band tint of 0x54 / 128 over the marbled patch's texels.
        let c = 84.0 / 128.0;
        assert_eq!(modulate5(expand5(3), c), expand5(1));
        assert_eq!(modulate5(expand5(25), c), expand5(16));
        // The neutral colour word leaves a 5-bit texel exactly as it is.
        for t in 0..32u8 {
            assert_eq!(modulate5(expand5(t), 1.0), expand5(t));
        }
        // Over-bright words saturate at 31.
        assert_eq!(modulate5(255, 2.0), 255);
    }

    /// A whitewashed glyph under an ink stored as `T / 31` lands on `T`, and
    /// on retail's 8-bit reading of it.
    #[test]
    fn every_ink_draws_its_own_fill_and_shadow() {
        for e in INK_SHADOWS {
            let rgb = e.fill.map(ink_component);
            let fill = tint_texel([255, 255, 255, 255], rgb, true);
            assert_eq!(&fill[..3], &e.fill.map(expand5)[..], "ink {}", e.ink);
            let shadow = tint_texel([41, 41, 41, FONT_SHADOW_ALPHA], rgb, true);
            let s = e.shadow.map(expand5);
            assert_eq!(shadow, [s[0], s[1], s[2], 255], "ink {}", e.ink);
        }
        // The two the options screen shows side by side.
        let gold = tint_texel(
            [41, 41, 41, FONT_SHADOW_ALPHA],
            [28.0 / 31.0, 21.0 / 31.0, 0.0],
            true,
        );
        assert_eq!(gold, [66, 66, 33, 255]);
        let teal = tint_texel(
            [41, 41, 41, FONT_SHADOW_ALPHA],
            [8.0 / 31.0, 27.0 / 31.0, 27.0 / 31.0],
            true,
        );
        assert_eq!(teal, [33, 66, 66, 255]);
    }

    /// A tint that is no font ink leaves a shadow texel modulated like any
    /// other, opaque.
    #[test]
    fn an_unknown_tint_modulates_the_shadow() {
        let t = tint_texel([41, 41, 41, FONT_SHADOW_ALPHA], [1.0, 1.0, 1.0], true);
        assert_eq!(t, [41, 41, 41, 255]);
        let t = tint_texel([41, 41, 41, FONT_SHADOW_ALPHA], [1.0, 1.0, 1.0], false);
        assert_eq!(t, [41, 41, 41, 255]);
    }

    /// The smooth path is the plain multiply the overlay used before, and a
    /// partly transparent texel takes it whatever the mode.
    #[test]
    fn smooth_tint_is_the_8_bit_multiply() {
        let t = tint_texel([200, 100, 0, 255], [0.5, 0.5, 0.5], false);
        assert_eq!(t, [100, 50, 0, 255]);
        let t = tint_texel([200, 100, 0, 191], [0.5, 0.5, 0.5], true);
        assert_eq!(t, [100, 50, 0, 191]);
    }

    #[test]
    fn the_generated_wgsl_carries_every_ink() {
        let w = wgsl_ui_tint();
        for e in INK_SHADOWS {
            assert!(w.contains(&format!(
                "vec3<f32>({}.0, {}.0, {}.0)",
                e.fill[0], e.fill[1], e.fill[2]
            )));
        }
        assert!(w.contains("fn ui_tint("));
    }
}
