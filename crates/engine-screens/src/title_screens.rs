//! The title card's two session-to-draw projections both play hosts make:
//! which title-TIM bands are up and how bright, and the menu-glyph rows the
//! no-title-art fallback draws. Each host keeps only its atlas residency
//! answers (is the title TIM resident, is the glyph atlas) and its surface.

use legaia_engine_title::title::{TitlePhase, TitleSession};
use legaia_engine_ui::{SpriteDraw, TitleBandState};

/// Which title-TIM bands the card draws this frame, and how bright: the
/// fade-in ramps the whole card's alpha, `PressStart` raises the prompt band,
/// `MainMenu` the two rows with the cursor's bright. `None` once the session
/// is `Done`.
pub fn title_band_state(session: &TitleSession) -> Option<TitleBandState> {
    let alpha = match session.phase() {
        TitlePhase::Done(_) => return None,
        TitlePhase::FadeIn { frames_remaining } => {
            let total = session.fade_in_frames.max(1) as f32;
            1.0 - (frames_remaining as f32 / total).clamp(0.0, 1.0)
        }
        _ => 1.0,
    };
    let mut st = TitleBandState::card(alpha);
    st.press_start = matches!(session.phase(), TitlePhase::PressStart { .. });
    if let TitlePhase::MainMenu { cursor } = session.phase() {
        st.menu = Some((cursor, true));
    }
    Some(st)
}

/// The NEW GAME / CONTINUE rows out of the menu-glyph atlas, for a host whose
/// title TIM did not resolve (with the TIM resident its own bands carry the
/// rows, and drawing both double-renders them - the caller makes that test).
///
/// The rows are drawn for whatever sub-mode word `title_text_phase` calls a
/// menu (phase `2`); the session's phase supplies only the cursor row. The
/// anchor is the centred, integer-scaled 256x256 title stage the art path
/// uses: the rows sit between the wordmark band (ends at source y 140) and
/// the copyright bands (start at source y 195), "NEW GAME" (8 cells x 8 px)
/// centred across the stage at source y 152, cells at the stage's scale.
pub fn title_menu_glyph_draws(
    session: &TitleSession,
    surface_w: u32,
    surface_h: u32,
) -> Vec<SpriteDraw> {
    let (menu_open, cursor) = match session.phase() {
        TitlePhase::MainMenu { cursor } => (true, cursor),
        _ => (false, 0),
    };
    let phase_id =
        legaia_engine_title::title_overlay::title_text_phase(session.retail_submode(), menu_open);
    if phase_id != 2 {
        return Vec::new();
    }
    const STAGE: u32 = 256;
    let title_scale = (surface_w / STAGE).min(surface_h / STAGE).clamp(1, 4);
    let ts = title_scale as i32;
    let stage_x0 = (surface_w as i32 - STAGE as i32 * ts) / 2;
    let stage_y0 = (surface_h as i32 - STAGE as i32 * ts) / 2;
    let menu_w_src = 8 * 8;
    let pen = (
        stage_x0 + (STAGE as i32 - menu_w_src) / 2 * ts,
        stage_y0 + 152 * ts,
    );
    legaia_engine_ui::title_menu_draws_for(
        phase_id,
        cursor,
        session.continue_enabled,
        pen,
        title_scale,
    )
}
