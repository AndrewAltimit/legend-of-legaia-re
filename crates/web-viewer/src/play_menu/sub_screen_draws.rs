//! The Load / Save sub-screen's builder. The other sub-screens are the shared
//! projection (`legaia_engine_screens::pause_screens`).

use super::*;

impl LegaiaRuntime {
    /// Load / Save sub-screen: the real retail save-select chrome, driven off
    /// the memory-card rack - the native window's `save_select_overlay`
    /// twin, with the pill row bound to the rack's card ports and the preview
    /// grid to the selected card's blocks.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_save_select(
        &self,
        assets: &PlayMenuAssets,
        s: &SaveSelectSession,
        menu: &PlayMenu,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) -> Option<(usize, u8)> {
        // The screen is the shared composition over the engine's overlay
        // sequence (`SaveScreenFlow::overlay_model` ->
        // `save_select_overlay_draws`), the native window's calls too. Its
        // text half draws with or without the chrome atlas: this page used
        // to return before every phase overlay when the atlas was absent,
        // leaving the title and nothing else.
        let out = legaia_engine_screens::pause_screens::save_select_overlay_draws(
            &menu.save_flow,
            s,
            &assets.font,
            assets.chrome.as_ref().map(|(_, rects)| rects),
            origin,
            scale,
        )?;
        let darken = out.darken.map(|d| (sprites.len() + d.sprite, d.level));
        sprites.extend(out.sprites);
        texts.extend(out.texts);
        darken
    }
}
