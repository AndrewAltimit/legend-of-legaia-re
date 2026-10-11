//! The banner glyph strip: U table, CLUT ids and the glyph draw.

use super::*;

/// The widget whose `u` origin the glyph wrappers page.
pub const GLYPH_WIDGET: u8 = 5;
/// Pixel pitch of one glyph cell in that strip.
pub const GLYPH_CELL_WIDTH: i32 = 24;

/// Runtime VA of the glyph strip's `u` field (widget 5 `+0x08`).
pub const GLYPH_U_VA: u32 = 0x801D_71CC;
/// Runtime VA of the banner CLUT the intro timeline swaps (widget 34 `+0x06`).
pub const BANNER_CLUT_VA: u32 = 0x801D_740E;

/// The two CLUT ids the intro title alternates between on widget 34.
pub const BANNER_CLUT_IDLE: u16 = 0x7740;
pub const BANNER_CLUT_FLASH: u16 = 0x7742;

/// `u` origin the glyph wrappers stamp for cell `index` (a byte store, so it
/// wraps at 256 exactly as retail's `sb` does).
///
/// PORT: FUN_801D69A8 (the store half) / FUN_801D67F0 mode 2.
///
/// Wired: the duel's own [`BakaChrome`] produces the draws
/// ([`crate::baka_fighter::BakaFight::chrome_frame`]). The minigames page's
/// `baka_chrome_json` export stamps each glyph-carrying [`ChromeDraw`] through
/// this and the page samples widget 5's strip at the stamped column - the one
/// host that draws the chrome from the duel's own art. The play window
/// performs the same stamp against the overlay's parsed widget table
/// (`legaia_minigame_formats::baka_opponents::parse_baka_hud`, `window/minigames.rs`) and
/// carries the resolved rect beside each draw, but prints the chrome as
/// labels: its duel HUD has no textured-quad surface, so the rect is not yet
/// sampled there. The browser play page prints the same labels.
pub fn glyph_u(index: i32) -> u8 {
    (index.wrapping_mul(GLYPH_CELL_WIDTH) & 0xFF) as u8
}

/// PORT: FUN_801D69A8 - the glyph draw wrapper.
///
/// `FUN_801D69A8(x, y, index, brightness, size)` pages the [`GLYPH_WIDGET`]
/// strip to `index` and then emits that widget with the caller's brightness
/// and size unchanged. It is a pure re-spelling of `FUN_801D5ED0` with the
/// widget id pinned to 5.
pub fn glyph_draw(x: i16, y: i16, index: i32, brightness: i32, size: i32) -> ChromeDraw {
    ChromeDraw {
        widget: GLYPH_WIDGET,
        x,
        y,
        brightness,
        size,
        glyph: Some(index),
    }
}

/// The signed `>> 1` retail spells as `srl 31; addu; sra 1` (round toward
/// zero), used by both the banner and the countdown for their half-brightness.
pub(super) fn half(v: i32) -> i32 {
    let v = v.wrapping_add(((v as u32) >> 31) as i32);
    v >> 1
}

/// The `mult`-by-magic divide-by-30 the banner ramp uses (`0x88888889`,
/// add, `sra 4`) - a plain signed division, restated.
pub(super) fn div30(v: i32) -> i32 {
    v / 30
}
