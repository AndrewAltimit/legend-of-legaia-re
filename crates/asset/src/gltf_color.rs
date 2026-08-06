//! The one `COLOR_0` convention every `.glb` exporter in this crate uses.
//!
//! Retail applies **no light source** to either TMD render path. All of its
//! shading is the prim's baked packet colour word, and the word means two
//! different things depending on whether the prim is textured:
//!
//! | prim | retail | glTF encoding |
//! |---|---|---|
//! | textured | `texel * colour / 128` (PSX GPU texture blend) | `COLOR_0 = colour / 128`, multiplied against `baseColorTexture` |
//! | untextured | filled with `colour` directly | `COLOR_0 = colour / 255`, over a white base |
//!
//! Getting the divisor wrong is not a subtle error in either direction: a
//! textured prim divided by 255 halves the model's brightness, and an
//! untextured prim divided by 128 doubles it. `0x80` is the neutral
//! modulation word, so a textured stream that is *missing* reads as white -
//! `texel * 255/128`, "too bright" - rather than as "unlit", which is why the
//! loss is easy to ship and hard to notice.
//!
//! ## Why the floats are allowed above 1.0
//!
//! `0xFF / 128 = 1.99`, so a faithful modulation factor exceeds 1.0 for every
//! colour component above neutral - the over-bright tail retail uses for
//! glowing / flame-tinted geometry (a summon's sword blade is a near-white
//! texture ramp tinted per vertex, with words as strong as `(248, 128, 0)`).
//! The normalized-integer `COLOR_n` encodings cannot represent that, so these
//! accessors use the float component type instead, as `VEC4`.
//!
//! Unclamped floats are never worse than clamping here: the PSX GPU clamps
//! the *product* at 255, which is exactly where a renderer clamps its own
//! LDR output, so a viewer that multiplies straight through reproduces the
//! canvas; and a viewer that clamps the attribute instead lands on precisely
//! the result a clamped encoding would have baked in. The cost is that the
//! attribute alone is out of the `[0, 1]` range some tools assume, so a
//! strict inspector may report values above 1.0 - by design.
//!
//! Two mismatches remain and are not fixable at this layer: a glTF viewer
//! multiplies in linear space after converting the base-colour texture from
//! sRGB, while the PSX multiplies raw 8-bit channels, and the GTE depth cue
//! is a per-frame term with no static equivalent.

/// The neutral modulation word: `texel * 128 / 128 == texel`.
pub const MODULATION_NEUTRAL: u8 = legaia_tmd::legaia_prims::MODULATION_NEUTRAL;

/// `COLOR_0` for a **textured** vertex - the packet word as the modulation
/// factor `colour / 128` (see the module docs on why this can exceed 1.0).
pub fn modulation_color(c: [u8; 3]) -> [f32; 4] {
    [
        f32::from(c[0]) / 128.0,
        f32::from(c[1]) / 128.0,
        f32::from(c[2]) / 128.0,
        1.0,
    ]
}

/// `COLOR_0` for an **untextured** vertex - the packet word is the prim's
/// fill, so it maps `0..=255` onto `0..=1`.
pub fn fill_color(c: [u8; 3]) -> [f32; 4] {
    [
        f32::from(c[0]) / 255.0,
        f32::from(c[1]) / 255.0,
        f32::from(c[2]) / 255.0,
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
    fn neutral_modulation_is_identity_and_the_top_word_nearly_doubles() {
        assert_eq!(modulation_color([MODULATION_NEUTRAL; 3])[0], 1.0);
        let hot = modulation_color([0xFF, 0x00, 0x80]);
        assert!((hot[0] - 1.9921875).abs() < 1e-6, "{hot:?}");
        assert_eq!(hot[1], 0.0);
        assert_eq!(hot[2], 1.0);
        assert_eq!(hot[3], 1.0);
    }

    /// The divisor is the whole difference between the two halves: the same
    /// word is 1.0 as a modulation and 0.502 as a fill.
    #[test]
    fn fill_and_modulation_disagree_by_the_divisor() {
        let w = [MODULATION_NEUTRAL; 3];
        assert_eq!(modulation_color(w)[0], 1.0);
        assert!((fill_color(w)[0] - 128.0 / 255.0).abs() < 1e-6);
        assert_eq!(fill_color([255, 255, 255])[0], 1.0);
    }
}
