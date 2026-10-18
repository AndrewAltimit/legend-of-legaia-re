//! Gauge and readout inks, alpha, and the font's solid texel.

use super::*;
use legaia_engine_ui::font_solid_src;
use legaia_engine_ui::gauge_fill_color;

#[test]
fn hp_bar_color_index_tiers_match_retail() {
    // K.O. -> 2 regardless of max.
    assert_eq!(hp_bar_color_index(0, 100, false), 2);
    // cur <= max/4 -> 9 (danger). max>>2 = 25, so 25 is still danger.
    assert_eq!(hp_bar_color_index(25, 100, false), 9);
    assert_eq!(hp_bar_color_index(1, 100, false), 9);
    // max/4 < cur <= max/2 -> 6 (caution). 26..=50.
    assert_eq!(hp_bar_color_index(26, 100, false), 6);
    assert_eq!(hp_bar_color_index(50, 100, false), 6);
    // cur > max/2 -> 7 (normal).
    assert_eq!(hp_bar_color_index(51, 100, false), 7);
    assert_eq!(hp_bar_color_index(100, 100, false), 7);
    // The status flag forces caution (6) even at full HP.
    assert_eq!(hp_bar_color_index(100, 100, true), 6);
    // ...but never overrides K.O. or danger.
    assert_eq!(hp_bar_color_index(0, 100, true), 2);
    assert_eq!(hp_bar_color_index(10, 100, true), 9);
}

#[test]
fn mp_bar_color_index_tiers_match_retail() {
    // No K.O. tier: empty MP reads as danger (9), not 2.
    assert_eq!(mp_bar_color_index(0, 40), 9);
    assert_eq!(mp_bar_color_index(10, 40), 9); // cur <= max/4
    assert_eq!(mp_bar_color_index(11, 40), 6); // max/4 < cur <= max/2
    assert_eq!(mp_bar_color_index(20, 40), 6);
    assert_eq!(mp_bar_color_index(21, 40), 7); // cur > max/2
    assert_eq!(mp_bar_color_index(40, 40), 7);
}

#[test]
fn gauge_fill_color_distinguishes_every_retail_index() {
    let idxs = [2u8, 3, 6, 7, 9];
    for (i, a) in idxs.iter().enumerate() {
        for b in idxs.iter().skip(i + 1) {
            assert_ne!(
                gauge_fill_color(*a),
                gauge_fill_color(*b),
                "indices {a} and {b} map to one colour"
            );
        }
    }
}

#[test]
fn battle_hud_caution_mp_uses_the_caution_tint_not_the_row_color() {
    let font = legaia_font::synthetic_for_tests();
    // 15 MP of 40 is in (10, 20] -> caution -> the tier-6 tint.
    let slot = slot_view("Noa", true, true, 100, 100, 15, 40);
    let draws = hud_draws(&font, &[slot], &[], &[]);
    // Retail's caution tier is font CLUT byte 12, entry 15 `(230, 172, 0)` -
    // amber. The old magnitude line here (`G > 0.9`) encoded the port's
    // brighter approximation, not a measurement.
    let caution = gauge_fill_color(6);
    // On the 5-bit grid: entry 15 is `(28, 21, 0)`.
    assert_eq!(caution.map(|c| (c * 31.0).round() as u8), [28, 21, 0, 31]);
    assert!(
        draws.iter().any(|d| d.src != SOLID && d.color == caution),
        "caution MP should produce an amber-tinted glyph"
    );
    assert_ne!(
        caution,
        legaia_engine_ui::READOUT_NORMAL,
        "the caution tier must not collapse onto the row colour"
    );
}

#[test]
fn apply_alpha_scales_only_alpha_channel() {
    let c = [0.5, 0.6, 0.7, 1.0];
    let scaled = apply_alpha(c, 0.5);
    assert_eq!(scaled, [0.5, 0.6, 0.7, 0.5]);
}

#[test]
fn font_solid_src_finds_a_white_texel_in_the_placeholder_font() {
    let font = legaia_font::Font::placeholder();
    let src = font_solid_src(&font).expect("placeholder font has white strokes");
    assert_eq!((src.2, src.3), (1, 1), "solid src must be a 1x1 rect");
    let (w, _) = font.atlas_dimensions();
    let off = ((src.1 * w + src.0) * 4) as usize;
    let rgba = font.atlas_rgba();
    assert_eq!(&rgba[off..off + 4], &[255, 255, 255, 255]);
}
