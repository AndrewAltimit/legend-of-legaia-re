//! The dialog reading box both play hosts draw: the plain-string snapshot of
//! the live pager, its stage layout, and the box's sprite and text halves.
//!
//! Each host used to carry its own `DialogSnapshot`, `from_panel` and
//! `dialog_stage_layout`, and the two layouts disagreed on the box height
//! (see [`dialog_stage_layout`]).

use legaia_engine_core::world::World;
use legaia_engine_dialog::dialog::OwnedDialogPanel;
use legaia_engine_ui::{SaveMenuAtlasRects, SpriteDraw, TextDraw};

/// A stage-pixel rect `(x, y, w, h)`.
pub type StageRect = (i32, i32, i32, i32);

/// Plain-string view of the live dialog panel, shared by the text and chrome
/// layers.
#[derive(Debug, Clone, Default)]
pub struct DialogSnapshot {
    /// Current typed-out page, `|` (0x7C) separating rows.
    pub page: String,
    /// Whole pixels the rows draw above their slots (the pager window's
    /// scroll, `<= 0`).
    pub scroll_px: i32,
    /// The pager window's height in rows (`None` for the plain-MES panel,
    /// whose box grows with its page); also the text clip band.
    pub box_rows: Option<usize>,
    /// Decoded option labels when a picker menu is open (empty otherwise).
    pub options: Vec<String>,
    /// Selected option row.
    pub cursor: usize,
    /// The picker box's rect this frame, from the engine's slide
    /// (`OwnedDialogPanel::picker_rect`); `None` before it starts.
    pub picker_rect: Option<StageRect>,
    /// The option hand is drawn (the slide rests).
    pub picker_hand: bool,
    /// The option hand's idle bob (X, stage pixels).
    pub hand_bob: i32,
    /// The page mark's strip frame, `None` when the pager draws none (no
    /// wait, or an automatic press counting down).
    pub advance_icon: Option<u8>,
}

/// Stage-pixel dialog box layout: the reading-box centre rect and the option
/// picker's rect when a menu is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogStageLayout {
    pub main: StageRect,
    pub picker: Option<StageRect>,
}

fn to_ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if (0x20..=0x7E).contains(&b) {
                b as char
            } else {
                '?'
            }
        })
        .collect()
}

/// Snapshot one panel. `require_text` drops a panel whose page is empty (the
/// script-driven sources, between pages); the page mark's frame is the
/// world's read (`World::page_mark_frame`), which carries the retail
/// comparison's seat when one is set.
pub fn dialog_snapshot_from_panel(
    world: &World,
    panel: &OwnedDialogPanel,
    require_text: bool,
) -> Option<DialogSnapshot> {
    let page = legaia_engine_ui::dialog_page_string(&panel.page_bytes());
    if require_text && page.is_empty() {
        return None;
    }
    let (options, cursor) = match (panel.menu_active(), panel.picker()) {
        (true, Some(_)) => (
            panel.picker_labels().iter().map(|l| to_ascii(l)).collect(),
            panel.picker_cursor(),
        ),
        _ => (Vec::new(), 0),
    };
    Some(DialogSnapshot {
        page,
        scroll_px: panel.scroll_px(),
        box_rows: panel.box_rows(),
        options,
        cursor,
        picker_rect: panel.picker_rect(),
        picker_hand: panel.picker_hand_drawn(),
        hand_bob: panel.picker_hand_bob(),
        advance_icon: world.page_mark_frame(panel),
    })
}

/// The live dialog source this frame, in precedence order: the host's own
/// typed-out panel (`host_panel`, the native window's pre-field-VM fallback;
/// `None` on a host without one), then the cutscene-timeline / script panel,
/// then the inline-script field-VM runner.
pub fn dialog_snapshot(
    world: &World,
    host_panel: Option<&OwnedDialogPanel>,
) -> Option<DialogSnapshot> {
    if let Some(panel) = host_panel {
        return dialog_snapshot_from_panel(world, panel, false);
    }
    if let Some(panel) = world.script_dialog_panel()
        && let Some(snap) = dialog_snapshot_from_panel(world, panel, true)
    {
        return Some(snap);
    }
    let inline = world.dialog.inline.as_ref()?;
    // Between boxes the pager still draws the box its last press dispatched
    // (`InlineDialogue::held_box`) - on the field: the battle overlay loads
    // over the pager's, so a talk that runs into its fight draws no box there.
    let held = (world.mode != legaia_engine_mode::mode::SceneMode::Battle)
        .then(|| inline.held_box.as_ref().map(|h| &h.panel))
        .flatten();
    let panel = inline.panel.as_ref().or(held)?;
    dialog_snapshot_from_panel(world, panel, true)
}

/// The reading box's centre rect for a box `lines` rows tall:
/// `(0x26, 0x10, 0xF4, lines*0xF - 3)`.
///
/// `FUN_801D84D0` frames the box with `FUN_8002C69C(ctx+0x12, ctx+0x14 + y,
/// 0xF4, h - 8)` where `h = rows*0xF + 5` (`0x801D9964..0x801D99D4`, `rows`
/// the word at `0x801F2740`, scaled by the open ramp while it grows), and the
/// live context holds `ctx+0x12 = 0x26`, `ctx+0x14 = 0x10`. Retail anchors the
/// reading box at the top of the stage, with or without a picker.
pub fn dialog_reading_box_rect(lines: i32) -> StageRect {
    (0x26, 0x10, 0xF4, lines * 0xF - 3)
}

/// The stage layout for a snapshot.
///
/// The box is `rows` tall where `rows` is the pager's own window height word
/// (`_DAT_801F2740`: three unless op `4C E0` set another). The frame height
/// at `0x801D9964` and the clip band at `0x801D98D8` both read that word with
/// no clamp, so a four- or five-row pager window frames four or five rows;
/// only the plain-MES panel (no window word) sizes off its page, clamped to
/// `3..=4` ([`legaia_engine_ui::dialog_reading_box_lines`]). The picker rect
/// is the engine's slide rect, drawn only while options are up.
pub fn dialog_stage_layout(snap: &DialogSnapshot) -> DialogStageLayout {
    let lines = legaia_engine_ui::dialog_reading_box_lines(&snap.page, snap.box_rows);
    DialogStageLayout {
        main: dialog_reading_box_rect(lines),
        picker: if snap.options.is_empty() {
            None
        } else {
            snap.picker_rect
        },
    }
}

/// The reading box's sprite half: the window chrome around the reading box
/// and the picker, the option hand once the slide rests (`FUN_8002B994`
/// kind 0) or, with no picker, the two-frame page mark while the pager waits
/// (kind 1).
pub fn dialog_box_chrome_draws(
    rects: &SaveMenuAtlasRects,
    snap: &DialogSnapshot,
    origin: (i32, i32),
    scale: u32,
) -> Vec<SpriteDraw> {
    let lay = dialog_stage_layout(snap);
    let mut out = legaia_engine_ui::dialog_window_chrome_draws_for(rects, lay.main, origin, scale);
    if let Some(prect) = lay.picker {
        out.extend(legaia_engine_ui::dialog_window_chrome_draws_for(
            rects, prect, origin, scale,
        ));
        if snap.picker_hand {
            out.push(legaia_engine_ui::dialog_option_hand_sprite(
                rects,
                (prect.0 + snap.hand_bob, prect.1),
                snap.cursor,
                origin,
                scale,
            ));
        }
    } else if let Some(frame) = snap.advance_icon {
        out.push(legaia_engine_ui::dialog_page_mark_sprite(
            legaia_engine_menus::save_menu_atlas::ATLAS_RECT_ADVANCE_ICON[usize::from(frame) & 1],
            lay.main,
            origin,
            scale,
        ));
    }
    out
}

/// The reading box's text half, in **stage** pixels: the pager's row window
/// at the retail 15-px pitch from the box origin, offset by its scroll and
/// clipped to the rows band (`FUN_801D84D0` draws `FUN_80036888(row, 0, 0,
/// ctx+0x12, ctx+0x14 + (scroll >> 4) + i*0xF)`), then the picker labels.
pub fn dialog_box_text_draws(
    font: &legaia_font::Font,
    snap: &DialogSnapshot,
    has_chrome: bool,
) -> Vec<TextDraw> {
    let lay = dialog_stage_layout(snap);
    let (bx, by, _, _) = lay.main;
    let mut out = legaia_engine_ui::dialog_reading_box_text_draws_for(
        font,
        &snap.page,
        (bx, by),
        snap.scroll_px,
        snap.box_rows,
    );
    if let Some((px, py, _, _)) = lay.picker {
        out.extend(legaia_engine_ui::dialog_picker_label_draws_for(
            font,
            &snap.options,
            snap.cursor,
            (px, py),
            has_chrome,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The standard reading box: 3 rows, centre rect `(0x26, 0x10, 0xF4,
    /// 0x2A)` - the geometry the `v0_1_tetsu_dialogue_accept` capture pins.
    #[test]
    fn standard_reading_box_is_three_rows_at_the_top() {
        let snap = DialogSnapshot {
            box_rows: Some(3),
            ..Default::default()
        };
        assert_eq!(dialog_stage_layout(&snap).main, (0x26, 0x10, 0xF4, 0x2A));
    }

    /// A pager window word other than 3 frames that many rows: retail reads
    /// `_DAT_801F2740` unclamped for both the frame and the clip band.
    #[test]
    fn the_pager_window_word_sets_the_box_height() {
        let snap = DialogSnapshot {
            box_rows: Some(5),
            ..Default::default()
        };
        assert_eq!(dialog_stage_layout(&snap).main.3, 5 * 0xF - 3);
    }

    /// The plain-MES panel grows with its page, three to four rows.
    #[test]
    fn a_plain_page_sizes_three_to_four_rows() {
        for (page, rows) in [("a", 3), ("a|b|c|d", 4), ("a|b|c|d|e", 4)] {
            let snap = DialogSnapshot {
                page: page.to_string(),
                ..Default::default()
            };
            assert_eq!(dialog_stage_layout(&snap).main.3, rows * 0xF - 3);
        }
    }
}
