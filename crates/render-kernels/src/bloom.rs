//! The bloom enhancement's response (NOT retail): which pixels glow and how
//! strongly, shared by both play hosts.
//!
//! The passes themselves are host code - the native renderer's
//! `renderer/bloom.rs` and the play page's `site/js/webgl-bloom.js` - but
//! the numbers they run on come from here, so the two hosts bloom the same
//! frame the same way.
//!
//! The response follows enhanced lighting's mood: the darker the mood, the
//! lower the threshold and the stronger the glow, so at night the lamps,
//! halos, lit windows and emissive props carry it while a daylight frame
//! takes only a faint lift on its brightest surfaces. With enhanced lighting
//! off the daylight response applies.

use crate::scene_lighting::LightingMood;

/// Soft-knee half-width below the threshold.
pub const KNEE: f32 = 0.25;
/// Threshold at mood glow `0` (and its drop to glow `1`).
pub const THRESHOLD_BASE: f32 = 0.52;
/// How far the threshold falls from [`THRESHOLD_BASE`] at full mood glow.
pub const THRESHOLD_DROP: f32 = 0.17;
/// Glow gain at mood glow `0`.
pub const INTENSITY_BASE: f32 = 0.9;
/// Glow gain added at full mood glow.
pub const INTENSITY_RISE: f32 = 0.7;

/// One frame's bloom response.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BloomParams {
    /// Brightness (largest channel, 0..1) where the glow is in full.
    pub threshold: f32,
    /// Soft-knee half-width below the threshold.
    pub knee: f32,
    /// Gain of the blurred glow added back over the scene.
    pub intensity: f32,
}

impl BloomParams {
    /// The response for a mood's glow strength
    /// ([`LightingMood::glow`], `0..=1`).
    pub fn for_glow(glow: f32) -> Self {
        let g = glow.clamp(0.0, 1.0);
        Self {
            threshold: THRESHOLD_BASE - THRESHOLD_DROP * g,
            knee: KNEE,
            intensity: INTENSITY_BASE + INTENSITY_RISE * g,
        }
    }

    /// The response a host stages: the mood's while enhanced lighting is
    /// on, daylight's otherwise.
    pub fn for_frame(mood: &LightingMood, enhanced_lighting: bool) -> Self {
        if enhanced_lighting {
            Self::for_glow(mood.glow)
        } else {
            Self::for_glow(LightingMood::DAY.glow)
        }
    }

    /// As the three floats the play page's pass reads.
    pub fn page_triple(self) -> [f32; 3] {
        [self.threshold, self.knee, self.intensity]
    }
}

impl Default for BloomParams {
    fn default() -> Self {
        Self::for_glow(LightingMood::DAY.glow)
    }
}

/// CPU mirror of the shaders' `bloom_weight`: the share of a colour whose
/// largest channel is its brightness that feeds the glow - nothing below
/// `threshold - knee`, a quadratic ease across the knee, linear above.
pub fn bloom_weight(rgb: [f32; 3], threshold: f32, knee: f32) -> f32 {
    let br = rgb[0].max(rgb[1]).max(rgb[2]);
    let soft = (br - threshold + knee).clamp(0.0, 2.0 * knee);
    let soft = soft * soft / (4.0 * knee + 1e-5);
    soft.max(br - threshold) / br.max(1e-5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn darker_moods_bloom_lower_and_stronger() {
        let day = BloomParams::for_frame(&LightingMood::DAY, true);
        let dusk = BloomParams::for_frame(&LightingMood::DUSK, true);
        let night = BloomParams::for_frame(&LightingMood::NIGHT, true);
        assert!(day.threshold > dusk.threshold && dusk.threshold > night.threshold);
        assert!(day.intensity < dusk.intensity && dusk.intensity < night.intensity);
        assert_eq!(
            BloomParams::for_frame(&LightingMood::NIGHT, false),
            BloomParams::default()
        );
    }

    #[test]
    fn the_brightness_law_only_lets_bright_colours_glow() {
        let p = BloomParams::default();
        assert_eq!(bloom_weight([0.0; 3], p.threshold, p.knee), 0.0);
        assert_eq!(bloom_weight([0.2, 0.15, 0.1], p.threshold, p.knee), 0.0);
        let low = p.threshold - p.knee;
        assert_eq!(bloom_weight([low, 0.0, 0.0], p.threshold, p.knee), 0.0);
        let knee = bloom_weight([p.threshold, 0.1, 0.1], p.threshold, p.knee);
        assert!(knee > 0.0 && knee < 0.2, "{knee}");
        let a = bloom_weight([0.8; 3], p.threshold, p.knee);
        let b = bloom_weight([1.0; 3], p.threshold, p.knee);
        assert!(a > knee && b > a && b < 1.0, "{a} {b}");
    }
}
