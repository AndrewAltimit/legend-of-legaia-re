//! The play page's two JS / GLSL twins of engine kernels this lane added,
//! pinned against the kernels' constants:
//!
//! - `overworldDecorationCue` (`site/js/webgl-tmd.js`) against
//!   `legaia_engine_core::overworld_ground_cue::decoration_draw_cue`;
//! - `psxTextureBlend` (the screen-primitive fragment shader in
//!   `site/js/play-app.js`) against
//!   `legaia_engine_ui::screen_prim::psx_texture_blend`.
//!
//! Nothing runs the JS outside a browser; this reads the functions' text and
//! checks the literals their arithmetic uses, so a constant edited on one
//! side fails here.

use legaia_engine_core::overworld_ground_cue::{
    DECORATION_CUE_NEAR_Z, DECORATION_FAR_BYTE, DECORATION_IR0_MAX,
};

fn site(rel: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../site/js")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// The body of the function whose header starts with `head`.
fn body(src: &str, head: &str) -> String {
    let start = src
        .find(head)
        .unwrap_or_else(|| panic!("{head} is defined"));
    let open = start + src[start..].find('{').expect("function body");
    let mut depth = 0usize;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return src[open + 1..open + i].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("{head}: unbalanced body");
}

#[test]
fn decoration_cue_twin_uses_the_kernel_constants() {
    let b = body(&site("webgl-tmd.js"), "function overworldDecorationCue(");
    let flat: String = b.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flat.contains(&format!(
            "Math.min(Math.max(trz - {:#X}, 0) >> 3, {:#X})",
            DECORATION_CUE_NEAR_Z, DECORATION_IR0_MAX
        )),
        "IR0 law drifted: {flat}"
    );
    assert!(
        flat.contains(&format!("{:#X} / 255", DECORATION_FAR_BYTE)),
        "far colour drifted: {flat}"
    );
    assert!(
        flat.contains("maxIr0: ir0 / 4096"),
        "IR0 scale drifted: {flat}"
    );
    // The origin's clip w: row 3 of vp against model's translation column.
    assert!(
        flat.contains(
            "vp[3] * model[12] + vp[7] * model[13] + vp[11] * model[14] + vp[15] * model[15]"
        ),
        "origin depth drifted: {flat}"
    );
}

#[test]
fn texture_blend_twin_matches_the_kernel_shape() {
    let b = body(&site("play-app.js"), "vec3 psxTextureBlend(");
    let flat: String = b.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat.contains("round(texel * 31.0)"), "{flat}");
    assert!(flat.contains("* 128.0"), "{flat}");
    assert!(
        flat.contains("min((t5 * c8) >> 7u, uvec3(31u))) / 31.0"),
        "{flat}"
    );
    // The kernel the shader mirrors.
    assert_eq!(
        legaia_engine_ui::screen_prim::psx_texture_blend(14, 39),
        ((14u32 * 39) >> 7) as u8
    );
    assert_eq!(legaia_engine_ui::screen_prim::psx_texture_blend(9, 0x80), 9);
}
