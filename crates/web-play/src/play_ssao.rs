//! The play page's **screen-space ambient occlusion** and the contact
//! shadows folded into it - the browser twin of the native renderer's SSAO
//! (`engine-render`'s `renderer/ssao.rs`), fed from the same constants and
//! per-sample law (`legaia_render_kernels::ssao`).
//!
//! Not retail: the field path draws baked colour words with no light
//! source, so there is no ambient term to occlude. The persisted
//! `OptionsState::ssao` (`Home` on both hosts, the page's "Ambient
//! occlusion" box) turns it on; the retail pin turns it off. The passes are
//! `site/js/webgl-ssao.js`; this module hands them the numbers.

use crate::runtime::LegaiaRuntime;
use legaia_render_kernels::ssao::{self, SsaoParams};
use wasm_bindgen::prelude::*;

/// Floats in [`LegaiaRuntime::play_ssao_frame`]'s packet.
pub const SSAO_FRAME_FLOATS: usize = 24;

/// The page's SSAO packet for one frame drawn through `view_proj`
/// (column-major, the matrix the page's scene pass uses):
///
/// * `[0..4]` - `(kx, ky, kw, 0)`, [`ssao::view_scale`];
/// * `[4..20]` - [`ssao::shader_words`];
/// * `[20..24]` - [`ssao::contact_light`] (`xyz` the key light in view
///   space, `w` the contact-shadow strength, zero with enhanced lighting
///   off).
///
/// A battle camera is composed with the battle stage model first, as the
/// native window stages it, so the radius is in battle units on both hosts
/// and the key light is aimed in retail's Y-down frame. Empty for a
/// degenerate matrix.
pub fn ssao_frame_packet(
    params: &SsaoParams,
    view_proj: &[f32; 16],
    battle: bool,
    battle_scale: f32,
    key_dir: [f32; 3],
    key_rgb: [f32; 3],
    enhanced: bool,
) -> Vec<f32> {
    let mut cam = *view_proj;
    if battle {
        // `vp * diag(s, -s, s, 1)`: scale the first three columns.
        for (col, f) in [battle_scale, -battle_scale, battle_scale]
            .into_iter()
            .enumerate()
        {
            for row in 0..4 {
                cam[col * 4 + row] *= f;
            }
        }
    }
    let k = ssao::view_scale(&cam);
    if k.contains(&0.0) {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(SSAO_FRAME_FLOATS);
    out.extend_from_slice(&[k[0], k[1], k[2], 0.0]);
    for w in ssao::shader_words(params) {
        out.extend_from_slice(&w);
    }
    out.extend_from_slice(&ssao::contact_light(
        params, &cam, key_dir, key_rgb, battle, enhanced,
    ));
    out
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Turn ambient occlusion (and its contact shadows) on / off - the
    /// page's "Ambient occlusion" box, `Home` on both hosts - persisted with
    /// the other options (`OptionsState::ssao`).
    pub fn set_ssao(&mut self, on: bool) {
        if self.options_state.ssao != on {
            self.options_state.ssao = on;
            self.persist_and_apply_options();
        }
    }

    /// Whether ambient occlusion is on - the persisted option the page's
    /// box reflects (off under Retail-faithful's pin).
    pub fn ssao(&self) -> bool {
        self.options_state.ssao
    }

    /// This frame's SSAO packet for the page's passes
    /// ([`ssao_frame_packet`]), against the view-projection the scene pass
    /// draws with. Empty while the option is off or the matrix is
    /// degenerate - the page then draws the frame without the passes.
    pub fn play_ssao_frame(&self, view_proj: &[f32]) -> Vec<f32> {
        if !self.options_state.ssao {
            return Vec::new();
        }
        let Ok(vp) = <[f32; 16]>::try_from(view_proj) else {
            return Vec::new();
        };
        let mood = self.lighting_mood();
        ssao_frame_packet(
            &SsaoParams::default(),
            &vp,
            self.play_battle_active(),
            crate::play_battle_render::BATTLE_WORLD_SCALE,
            mood.key_dir,
            mood.key_rgb,
            self.options_state.enhanced_lighting,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> [f32; 16] {
        let proj = glam::camera::rh::proj::directx::perspective(0.9, 4.0 / 3.0, 1.0, 10_000.0);
        let view = glam::camera::rh::view::look_at_mat4(
            glam::Vec3::new(0.0, 800.0, 900.0),
            glam::Vec3::ZERO,
            glam::Vec3::Y,
        );
        (proj * view).to_cols_array()
    }

    /// The packet is the native frame block's words in the native order:
    /// view scale, the four parameter words, the contact light.
    #[test]
    fn the_packet_carries_the_shared_words() {
        let p = SsaoParams::default();
        let day = legaia_render_kernels::scene_lighting::LightingMood::DAY;
        let v = ssao_frame_packet(&p, &cam(), false, 4.0, day.key_dir, day.key_rgb, true);
        assert_eq!(v.len(), SSAO_FRAME_FLOATS);
        let k = ssao::view_scale(&cam());
        assert_eq!(&v[0..3], &k);
        let words: Vec<f32> = ssao::shader_words(&p).concat();
        assert_eq!(&v[4..20], &words[..]);
        let l = ssao::contact_light(&p, &cam(), day.key_dir, day.key_rgb, false, true);
        assert_eq!(&v[20..24], &l);
        // Enhanced lighting off: no key light, no march.
        let off = ssao_frame_packet(&p, &cam(), false, 4.0, day.key_dir, day.key_rgb, false);
        assert_eq!(off[23], 0.0);
        assert!(
            ssao_frame_packet(&p, &[0.0; 16], false, 4.0, day.key_dir, day.key_rgb, true)
                .is_empty()
        );
    }

    /// A battle frame measures in battle units, as the native window's
    /// `cam * battle_stage_model()`.
    #[test]
    fn a_battle_frame_is_in_battle_units() {
        let p = SsaoParams::default();
        let d = [0.0, -1.0, 0.0];
        let field = ssao_frame_packet(&p, &cam(), false, 4.0, d, [0.45; 3], true);
        let battle = ssao_frame_packet(&p, &cam(), true, 4.0, d, [0.45; 3], true);
        for i in 0..3 {
            assert!((battle[i] * 4.0 - field[i]).abs() < 1e-6 * field[i].abs().max(1.0));
        }
        // The stage model's Y flip and the Y-down aim cancel: the same sun.
        for i in 20..23 {
            assert!((battle[i] - field[i]).abs() < 1e-5);
        }
    }
}
