//! The one `COLOR_0` convention every `.glb` exporter in this crate uses.
//!
//! Retail applies **no light source** to either TMD render path. All of its
//! shading is the prim's baked packet colour word, and the word means two
//! different things depending on whether the prim is textured:
//!
//! | prim | retail (display space) | glTF encoding (linear space) |
//! |---|---|---|
//! | textured | `texel * colour / 128` (PSX GPU texture blend) | `COLOR_0 = srgb_to_linear(colour / 128)`, multiplied against `baseColorTexture` |
//! | untextured | filled with `colour` directly | `COLOR_0 = srgb_to_linear(colour / 255)`, over a white base |
//!
//! Getting the divisor wrong is not a subtle error in either direction: a
//! textured prim divided by 255 halves the model's brightness, and an
//! untextured prim divided by 128 doubles it. `0x80` is the neutral
//! modulation word, so a textured stream that is *missing* reads as white -
//! `texel * 255/128`, "too bright" - rather than as "unlit", which is why the
//! loss is easy to ship and hard to notice.
//!
//! ## Why the ratio goes through the sRGB transfer
//!
//! Retail (and the site's WebGL pages, which upload raw texels and draw to a
//! non-linearized canvas) multiplies in **display space**: the on-screen
//! result of a textured prim is literally `texel * colour / 128` in 8-bit
//! display values. A glTF viewer does not: `baseColorTexture` is sRGB by
//! spec, `COLOR_0` is a *linear* multiplier, so the viewer decodes the
//! texture, multiplies in linear light, and re-encodes for display. Handing
//! it the raw display-space ratio therefore renders as
//! `texel * (colour/128)^(1/2.2)` on screen - dark packet words wash out
//! toward gray (a `0x4C` clothing word draws at ~x0.79 instead of x0.59; up
//! to ~50/255 of error), which is how exported NPCs lost their shading while
//! the on-site viewers kept it.
//!
//! Encoding the ratio through the sRGB EOTF ([`srgb_ratio_to_linear`])
//! cancels the viewer's round trip: `encode(decode(texel) * decode(r))`
//! equals `texel * r` exactly at a white texel and to within a few
//! 8-bit steps everywhere else (the residue is the EOTF's linear toe -
//! the transfer is not a pure power law, so no single factor is exact at
//! every texel). The untextured half is exact everywhere: its base colour
//! is white, so the screen shows `encode(COLOR_0)`, and
//! `decode(colour / 255)` reproduces the fill bit-perfectly.
//!
//! ## Why the floats are allowed above 1.0
//!
//! `0xFF / 128 = 1.99`, so a faithful modulation factor exceeds 1.0 for every
//! colour component above neutral - the over-bright tail retail uses for
//! glowing / flame-tinted geometry (a summon's sword blade is a near-white
//! texture ramp tinted per vertex, with words as strong as `(248, 128, 0)`).
//! The normalized-integer `COLOR_n` encodings cannot represent that, so these
//! accessors use the float component type instead, as `VEC4` (ratios above
//! 1.0 extend through the EOTF's power branch, up to ~4.9 linear).
//!
//! Unclamped floats are never worse than clamping here: the PSX GPU clamps
//! the *product* at 255, which is exactly where a renderer clamps its own
//! LDR output, so a viewer that multiplies straight through reproduces the
//! canvas; and a viewer that clamps the attribute instead lands on precisely
//! the result a clamped encoding would have baked in. The cost is that the
//! attribute alone is out of the `[0, 1]` range some tools assume, so a
//! strict inspector may report values above 1.0 - by design.
//!
//! One mismatch remains and is not fixable at this layer: the GTE depth cue
//! is a per-frame term with no static equivalent.

/// The neutral modulation word: `texel * 128 / 128 == texel`.
pub const MODULATION_NEUTRAL: u8 = legaia_tmd::legaia_prims::MODULATION_NEUTRAL;

/// The sRGB EOTF, extended above 1.0 through its power branch: the linear
/// factor whose sRGB encoding is the display-space ratio `r` (see the module
/// docs on why every `COLOR_0` component goes through this).
pub fn srgb_ratio_to_linear(r: f32) -> f32 {
    if r <= 0.04045 {
        r / 12.92
    } else {
        ((r + 0.055) / 1.055).powf(2.4)
    }
}

/// The inverse of [`srgb_ratio_to_linear`] (sRGB OETF, extended above 1.0):
/// recovers the display-space ratio a baked `COLOR_0` component encodes.
/// This is how a probe maps an exported float back onto its packet word.
pub fn linear_to_srgb_ratio(f: f32) -> f32 {
    if f <= 0.003_130_8 {
        f * 12.92
    } else {
        1.055 * f.powf(1.0 / 2.4) - 0.055
    }
}

/// `COLOR_0` for a **textured** vertex - the packet word's modulation ratio
/// `colour / 128`, encoded through the sRGB EOTF so the viewer's
/// linear-space multiply lands on retail's display-space product (see the
/// module docs on why this can exceed 1.0).
pub fn modulation_color(c: [u8; 3]) -> [f32; 4] {
    [
        srgb_ratio_to_linear(f32::from(c[0]) / 128.0),
        srgb_ratio_to_linear(f32::from(c[1]) / 128.0),
        srgb_ratio_to_linear(f32::from(c[2]) / 128.0),
        1.0,
    ]
}

/// `COLOR_0` for an **untextured** vertex - the packet word is the prim's
/// fill, so it maps `0..=255` onto `0..=1` display space, then through the
/// sRGB EOTF into the linear value the viewer will re-encode back to it.
pub fn fill_color(c: [u8; 3]) -> [f32; 4] {
    [
        srgb_ratio_to_linear(f32::from(c[0]) / 255.0),
        srgb_ratio_to_linear(f32::from(c[1]) / 255.0),
        srgb_ratio_to_linear(f32::from(c[2]) / 255.0),
        1.0,
    ]
}

/// The `KHR_materials_unlit` marker every exported material carries.
///
/// Retail issues no GTE light op on either mesh path, so the baked packet
/// colour *is* the shading. A plain metallic-roughness material would hand
/// the model to whatever lights the viewer happens to have, re-introducing
/// exactly the synthetic Lambert this project keeps deleting. Declared in
/// `extensionsUsed` only (never `extensionsRequired`), so a viewer that does
/// not implement it falls back to the same material's PBR fields.
pub const UNLIT_EXTENSION: &str = "KHR_materials_unlit";

/// Read an exported `.glb` back the way a viewer would.
///
/// A kernel that runs and writes nothing is indistinguishable from one that
/// is not wired, so every claim about an export is asserted here - on the
/// bytes that left the exporter - rather than on the call that produced them.
pub mod glb_probe {
    use serde_json::Value;

    /// Split a `.glb` into `(JSON root, BIN chunk)`. `None` if it is not a
    /// well-formed binary glTF container.
    pub fn split(glb: &[u8]) -> Option<(Value, &[u8])> {
        if glb.len() < 20 || &glb[0..4] != b"glTF" || &glb[16..20] != b"JSON" {
            return None;
        }
        let json_len = u32::from_le_bytes(glb[12..16].try_into().ok()?) as usize;
        let root: Value = serde_json::from_slice(glb.get(20..20 + json_len)?).ok()?;
        let bin_start = 20 + json_len;
        let bin_len = u32::from_le_bytes(glb.get(bin_start..bin_start + 4)?.try_into().ok()?);
        if glb.get(bin_start + 4..bin_start + 8)? != b"BIN\0" {
            return None;
        }
        let bin = glb.get(bin_start + 8..bin_start + 8 + bin_len as usize)?;
        Some((root, bin))
    }

    /// Every element of a float accessor, as fixed-width rows. `None` when
    /// the accessor is missing, not float, or its data is out of range.
    pub fn floats(root: &Value, bin: &[u8], accessor: usize) -> Option<Vec<Vec<f32>>> {
        let acc = root["accessors"].get(accessor)?;
        if acc["componentType"] != 5126 {
            return None;
        }
        let n = acc["count"].as_u64()? as usize;
        let width = match acc["type"].as_str()? {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            _ => return None,
        };
        let view = root["bufferViews"].get(acc["bufferView"].as_u64()? as usize)?;
        let base = view["byteOffset"].as_u64()? as usize;
        (0..n)
            .map(|i| {
                (0..width)
                    .map(|c| {
                        let o = base + (i * width + c) * 4;
                        Some(f32::from_le_bytes(bin.get(o..o + 4)?.try_into().ok()?))
                    })
                    .collect::<Option<Vec<f32>>>()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_modulation_is_identity_and_the_top_word_exceeds_one() {
        assert_eq!(modulation_color([MODULATION_NEUTRAL; 3])[0], 1.0);
        let hot = modulation_color([0xFF, 0x00, 0x80]);
        // 0xFF/128 = 1.992 display-space, ~4.89 linear through the extended
        // power branch; still >1.0 by design.
        assert!((hot[0] - 4.909).abs() < 1e-2, "{hot:?}");
        assert_eq!(hot[1], 0.0);
        assert_eq!(hot[2], 1.0);
        assert_eq!(hot[3], 1.0);
    }

    /// The transfer pair is an inverse: a viewer that re-encodes the baked
    /// float for display recovers the packet word's display-space ratio.
    #[test]
    fn srgb_transfer_round_trips_the_display_ratio() {
        for w in [0u8, 0x10, 0x4C, 0x80, 0xC0, 0xFF] {
            let r = f32::from(w) / 128.0;
            let back = linear_to_srgb_ratio(srgb_ratio_to_linear(r));
            assert!((back - r).abs() < 1e-5, "word {w:#x}: {r} -> {back}");
        }
    }

    /// The regression the sRGB encoding fixes: a dark clothing word (0x4C,
    /// display ratio 0.594) must NOT bake as its raw ratio - a linear-space
    /// viewer re-encodes that to a ~0.79 on-screen multiply, washing dark
    /// shading out to gray. The baked float has to be the ratio's EOTF.
    #[test]
    fn dark_words_bake_below_their_display_ratio() {
        let dark = modulation_color([0x4C; 3])[0];
        assert!((dark - 0.311).abs() < 1e-2, "{dark}");
        // On screen: encode(1.0-linear-texel * dark) == display ratio.
        assert!((linear_to_srgb_ratio(dark) - 0x4C as f32 / 128.0).abs() < 1e-5);
    }

    /// The divisor is the whole difference between the two halves: the same
    /// word is identity as a modulation but a mid-gray as a fill.
    #[test]
    fn fill_and_modulation_disagree_by_the_divisor() {
        let w = [MODULATION_NEUTRAL; 3];
        assert_eq!(modulation_color(w)[0], 1.0);
        // Fill bakes the EOTF of 128/255, so its re-encoded display value is
        // exactly the packet byte.
        assert!((linear_to_srgb_ratio(fill_color(w)[0]) - 128.0 / 255.0).abs() < 1e-5);
        assert_eq!(fill_color([255, 255, 255])[0], 1.0);
    }
}
