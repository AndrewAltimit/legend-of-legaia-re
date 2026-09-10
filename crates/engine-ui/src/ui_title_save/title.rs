use crate::*;

/// Build [`TextDraw`]s for the title screen.
///
/// Phase argument controls which UI is rendered:
/// - `phase` = 0: fade-in (no text - engines fade the screen to black);
/// - `phase` = 1: "Press START" prompt (centered roughly mid-screen);
/// - `phase` = 2: main menu (New Game / Continue / Options stacked).
///
/// `cursor` is ignored for phases 0/1 and selects the highlighted row
/// (0..=2) in phase 2. `continue_enabled = false` dims the Continue row.
/// `blink_on` toggles the prompt visibility on phase 1 every blink_period
/// frames; engines drive this from the title session's blink phase.
///
/// When the engine has uploaded the PROT 0888 title TIM atlas, pass
/// `atlas_present = true` to suppress the font-rendered "PRESS START"
/// prompt (phase 1) - the TIM's own "PRESS START BUTTON" band is drawn
/// in its place by the sprite layer. The menu rows (phase 2) are
/// still rendered via font because retail uses larger font glyphs
/// there too, not the tiny "NEW GAME CONTINUE" band at the bottom of
/// the TIM.
///
/// A natural anchor for a 320×240 surface is `pen = (96, 100)` - the
/// renderer offsets each line from this top-left.
pub fn title_draws_for(
    font: &legaia_font::Font,
    phase: u8,
    cursor: u8,
    continue_enabled: bool,
    blink_on: bool,
    atlas_present: bool,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 16;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let dim: [f32; 4] = [0.45, 0.45, 0.45, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];

    let mut out = Vec::new();

    match phase {
        0 => {}
        1 if blink_on && !atlas_present => {
            let l = font.layout_ascii("PRESS START");
            out.extend(text_draws_for(&l, pen, white));
        }
        1 => {}
        2 => {
            // Retail title menu carries only two rows; Options lives in
            // the in-game field menu. Color is the selection indicator
            // (selected = white, unselected = dim) - no arrow / cursor
            // mark in retail. The disabled-Continue row reads the same
            // as a non-highlighted row.
            let _ = (gold, continue_enabled);
            let rows = ["NEW GAME", "CONTINUE"];
            for (i, label) in rows.iter().enumerate() {
                let row_y = pen.1 + i as i32 * LINE_H;
                let selected = i as u8 == cursor;
                let color = if selected { white } else { dim };
                let l = font.layout_ascii(label);
                out.extend(text_draws_for(&l, (pen.0, row_y), color));
            }
        }
        _ => {}
    }
    out
}

/// Build [`SpriteDraw`]s for the title-screen main-menu rows ("NEW GAME"
/// / "CONTINUE") sampling the dedicated menu-glyph atlas from
/// `PROT.DAT` (see [`legaia_asset::menu_glyph_atlas`]).
///
/// Retail-faithful equivalent of phase 2 in [`title_draws_for`] - same
/// row labels and cursor / dim semantics, but each row is a horizontal
/// strip of sprite cells sampled from the menu-glyph atlas instead of
/// dialog-font glyphs. Selected row gets a gold tint; the Continue
/// row is dimmed when `continue_enabled = false`. Retail's title menu
/// only carries two rows (NEW GAME / CONTINUE); Options is reached via
/// the in-game field menu, not from the title.
///
/// `cell_scale` is an integer multiplier applied to source-pixel sizes
/// so engines can match the title-art's `play-window` stage scale
/// (mirrors the per-band SpriteDraw scaling). `pen` is the top-left
/// corner of the first row's first glyph in surface pixels.
///
/// Note: the menu-glyph atlas carries only uppercase letters and
/// digits - no cursor marks.
///
/// Returns an empty vec for any phase other than 2.
pub fn title_menu_draws_for(
    phase: u8,
    cursor: u8,
    continue_enabled: bool,
    pen: (i32, i32),
    cell_scale: u32,
) -> Vec<SpriteDraw> {
    if phase != 2 {
        return Vec::new();
    }
    // Retail uses color as the SELECTION INDICATOR: the highlighted row
    // is bright white and unselected rows are dim gray. There is no
    // arrow / cursor mark - the brightness IS the cursor. Disabled
    // (Continue with no save) reads the same as a non-highlighted row.
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];

    use legaia_asset::menu_glyph_atlas as mga;
    let cell_w = mga::GLYPH_W as i32;
    let cell_h = mga::ALPHABET_GLYPH_H as i32;
    let scale = cell_scale.max(1) as i32;
    // One blank row of padding between rows so the small-caps glyphs
    // sit clearly apart (matches the retail menu vertical pitch).
    let line_h = cell_h + 2;

    let rows = ["NEW GAME", "CONTINUE"];
    let mut out = Vec::new();
    for (i, label) in rows.iter().enumerate() {
        let row_y = pen.1 + i as i32 * line_h * scale;
        let selected = i as u8 == cursor;
        let row_disabled = i == 1 && !continue_enabled;
        let _ = row_disabled; // disabled rows render the same as unselected
        let color = if selected { white } else { dim };
        let mut x = pen.0;
        for c in label.chars() {
            if let Some((sx, sy, sw, sh)) = mga::glyph_rect(c) {
                out.push(SpriteDraw {
                    dst: (x, row_y, sw * scale as u32, sh * scale as u32),
                    src: (sx, sy, sw, sh),
                    color,
                });
            }
            x += cell_w * scale;
        }
    }
    out
}

/// Which of the front-end's screens a **retail title sub-mode** selects.
///
/// PORT: FUN_801DD35C (the sub-mode -> screen half of the dispatcher)
///
/// The front end is one function with a 25-entry jump table
/// ([`legaia_engine_vm::title_overlay::SUBMODE_TABLE`]) and one selector word
/// (`state[+0x204]`). The port's title, save-select and options screens are
/// separate sessions, so each host used to pick its screen off its own enum
/// and the sub-mode word was carried but never read. This is the one place
/// that maps the word to a screen, and every host calls it.
///
/// **The word does not carry the prompt / rows split.** Retail's `0x10`
/// `AttractIdle` is a single state that draws the title card, polls
/// `Start | L1 | Cross` and steps the menu cursor on `Up` / `Down`
/// ([`legaia_engine_vm::title_overlay::PADMASK_START_L1_CROSS`] and the
/// cursor masks beside it), and the port's title session stays on that word
/// from the prompt through the menu - only a confirm moves it, to `0x16` or
/// `0x18`. So "Press Start" versus "NEW GAME / CONTINUE" is a port-side
/// phase, not a sub-mode, and [`title_text_phase`] takes it as a second
/// argument rather than inventing a word retail does not write.
///
/// The rest of the mapping is the evidence line: the modes the port's title
/// reaches are named, every other in-range mode is the **memory-card
/// manager** band that the port draws through the save-select builders, and
/// `>= 0x19` is outside the dispatcher's own `sltiu 0x19` bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleDrawList {
    /// Nothing of the title's own - the entry pass, the idle tail, and every
    /// out-of-range word.
    Blank,
    /// The title card: the wordmark plus either the prompt or the menu rows
    /// (`0x10` `AttractIdle`, `0x11` `AttractDelay`, and the retail-
    /// unreachable `0x02` `TextMenu` / `0x14` `MainMenu`).
    TitleCard,
    /// A row has been confirmed and the card is on its way out (`0x06`
    /// `LaunchGame`, `0x16` `LaunchFade`, `0x18` `ContinueFadeIn`).
    Launching,
    /// The memory-card manager band (`0x03`..=`0x0F`, `0x12`, `0x13`, `0x15`,
    /// `0x17`): slot grids, block scans, card prompts and their notices. The
    /// port draws these from the save-select builders
    /// ([`save_select_draws_for`](super::save_select_draws_for) and
    /// siblings), so the title layer contributes nothing to them.
    CardManager,
}

/// Map a retail title sub-mode word to its [`TitleDrawList`].
pub fn title_draw_list(submode: u8) -> TitleDrawList {
    use legaia_engine_vm::title_overlay::TitleOverlaySubMode as S;
    let Some(m) = S::from_u8(submode) else {
        return TitleDrawList::Blank;
    };
    match m {
        S::Init | S::Idle => TitleDrawList::Blank,
        S::TextMenu | S::MainMenu | S::AttractIdle | S::AttractDelay => TitleDrawList::TitleCard,
        S::LaunchGame | S::LaunchFade | S::ContinueFadeIn => TitleDrawList::Launching,
        _ => TitleDrawList::CardManager,
    }
}

/// The `phase` argument [`title_draws_for`] / [`title_menu_draws_for`] take,
/// resolved from the retail sub-mode word instead of from a host's own enum.
///
/// `0` draws nothing, `1` is the prompt, `2` is the menu rows - the builders'
/// existing contract, with one selector behind it on every host.
/// `menu_open` is the port-side half of the split retail keeps outside the
/// sub-mode word (see [`TitleDrawList`]).
pub fn title_text_phase(submode: u8, menu_open: bool) -> u8 {
    match title_draw_list(submode) {
        TitleDrawList::TitleCard if menu_open => 2,
        TitleDrawList::TitleCard => 1,
        TitleDrawList::Blank | TitleDrawList::Launching | TitleDrawList::CardManager => 0,
    }
}

#[cfg(test)]
mod submode_selector_tests {
    use super::*;
    use legaia_engine_vm::title_overlay::TitleOverlaySubMode as S;

    #[test]
    fn every_in_range_submode_selects_one_list_and_the_bound_holds() {
        for b in 0..=0x18u8 {
            assert!(S::is_in_range(b), "{b:#x} should be in the table");
            let _ = title_draw_list(b);
        }
        // The dispatcher's own bound: 0x19 and up fall to the epilogue.
        for b in [0x19u8, 0x20, 0xFF] {
            assert_eq!(title_draw_list(b), TitleDrawList::Blank);
        }
    }

    #[test]
    fn the_modes_the_port_reaches_map_to_the_screens_it_draws() {
        // A cold boot sits on 0x10 from the prompt through the menu, so the
        // same word has to serve both phases.
        assert_eq!(
            title_draw_list(S::AttractIdle as u8),
            TitleDrawList::TitleCard
        );
        assert_eq!(title_text_phase(S::AttractIdle as u8, false), 1);
        assert_eq!(title_text_phase(S::AttractIdle as u8, true), 2);
        // Confirming a row leaves the card.
        assert_eq!(
            title_draw_list(S::LaunchFade as u8),
            TitleDrawList::Launching
        );
        assert_eq!(
            title_draw_list(S::ContinueFadeIn as u8),
            TitleDrawList::Launching
        );
        assert_eq!(title_text_phase(S::LaunchFade as u8, true), 0);
        // The card-manager band belongs to the save-select builders.
        assert_eq!(
            title_draw_list(S::SlotGrid as u8),
            TitleDrawList::CardManager
        );
        assert_eq!(title_text_phase(S::SlotGrid as u8, true), 0);
    }
}
