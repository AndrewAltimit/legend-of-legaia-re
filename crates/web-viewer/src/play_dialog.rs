//! Browser retail **dialog reading box**: the field NPC / event message box,
//! rendered from the same `legaia-engine-ui` draw builders the native
//! `play-window` uses (gradient fill + gold 9-slice frame via
//! [`legaia_engine_ui::dialog_window_chrome_draws_for`], text at the retail
//! reading-box geometry, the option / page-advance hand cursors).
//!
//! The page's original dialog surface was a DOM overlay `<div>` printing the
//! HUD JSON. This module serves the byte-pinned retail box instead, as the
//! same `{ sprites, texts }` quad lists the pause menu ships
//! ([`crate::play_menu`]) - the page blits them over the live (NOT frozen)
//! field, exactly as retail draws the box over the running scene.
//!
//! Geometry mirrors the traced pager (`FUN_801D84D0`, the native window's
//! `dialog_stage_layout`):
//!
//! - Main (reading) box centre rect `(0x26, 0x10, 0xF4, lines*0xF - 3)`,
//!   anchored at the TOP of the 320x240 stage; retail's standard box is
//!   always 3 rows tall (`_DAT_801F2740 = 3`), only over-long simplified
//!   pages grow it to a 4th row. The drawn skin extends 8 px beyond the
//!   centre rect on every side (the chrome builder's inflation).
//! - Picker box `x = 0x26`, `y = 0x94 + ((4-n)*0xF)/2`, `w = 0xF4`,
//!   `h = 0x38 - (4-n)*0xF` (the picker-init arms' literal geometry).
//! - Text pen = box origin exactly (`FUN_80036888(line, 0, 0, ctx+0x12,
//!   ctx+0x14 + i*0xF)`), 15-px row pitch, body ink the staged CLUT-7
//!   (206,206,206) menu white; picker labels at `box_x + 0x10`.
//! - Advance hand at `x + w - 0x10`, `0x10` above the centre-rect bottom
//!   (`FUN_8002B994` kind 1); option hand on the selected row (kind 0).
//!
//! REF: FUN_801D84D0, FUN_8002C69C, FUN_8002B994

use super::*;
use crate::runtime::LegaiaRuntime;
use legaia_engine_ui::{self as ui, SpriteDraw, TextDraw};

/// Plain-string view of the live dialog panel (the native window's
/// `DialogSnapshot` twin): the typed page, picker options, cursor, and
/// whether the pager waits for a confirm.
struct DialogSnapshot {
    /// Current typed-out page, `|` (0x7C) separating rows.
    page: String,
    options: Vec<String>,
    cursor: usize,
    waiting: bool,
}

/// A stage-pixel centre rect `(x, y, w, h)`.
pub type StageRect = (i32, i32, i32, i32);

/// Stage-pixel dialog box layout: main reading-box centre rect + the
/// option-picker rect when a menu is open.
struct DialogStageLayout {
    main: StageRect,
    picker: Option<StageRect>,
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

fn from_panel(
    panel: &legaia_engine_core::dialog::OwnedDialogPanel,
    require_text: bool,
) -> Option<DialogSnapshot> {
    let page = to_ascii(&panel.page_bytes());
    if require_text && page.is_empty() {
        return None;
    }
    let (options, cursor) = if panel.menu_active() {
        match panel.picker() {
            Some(p) => (
                p.options.iter().map(|o| to_ascii(&o.label)).collect(),
                panel.picker_cursor(),
            ),
            None => (Vec::new(), 0),
        }
    } else {
        (Vec::new(), 0)
    };
    Some(DialogSnapshot {
        page,
        options,
        cursor,
        // The advance hand shows at a page break AND on the final fully-typed
        // page (retail waits for a confirm on both).
        waiting: panel.is_waiting_for_input() || panel.is_done(),
    })
}

/// Retail reading-box + picker centre rects for a page of `page_lines` rows
/// and `options` picker entries (`0` = no picker) - the same literal geometry
/// as the native window's `dialog_stage_layout`:
///
/// - main box `(0x26, 0x10, 0xF4, lines*0xF - 3)`, `lines` clamped 3..=4
///   (retail's standard box is always 3 rows, `_DAT_801F2740 = 3`);
/// - picker `(0x26, 0x94 + ((4-n)*0xF)/2, 0xF4, 0x38 - (4-n)*0xF)`,
///   `n` clamped 2..=4.
///
/// REF: FUN_801D84D0
pub fn dialog_reading_box_layout(
    page_lines: usize,
    options: usize,
) -> (StageRect, Option<StageRect>) {
    let lines = page_lines.clamp(3, 4) as i32;
    let picker = if options == 0 {
        None
    } else {
        let n = options.clamp(2, 4) as i32;
        Some((0x26, 0x94 + ((4 - n) * 0xF) / 2, 0xF4, 0x38 - (4 - n) * 0xF))
    };
    ((0x26, 0x10, 0xF4, lines * 0xF - 3), picker)
}

fn dialog_stage_layout(snap: &DialogSnapshot) -> DialogStageLayout {
    let (main, picker) =
        dialog_reading_box_layout(snap.page.split('|').count(), snap.options.len());
    DialogStageLayout { main, picker }
}

impl LegaiaRuntime {
    /// Snapshot the live dialog source. The web host runs with
    /// `use_vm_dialogue` armed, so the sources are the cutscene-timeline
    /// segment (when a timeline plays) and the inline-script field-VM runner -
    /// the same precedence as the native window's `dialog_snapshot`.
    fn dialog_snapshot(&self) -> Option<DialogSnapshot> {
        let h = self.scene_host.as_ref()?;
        if let Some(panel) = h
            .world
            .cutscene_timeline
            .as_ref()
            .and_then(|tl| tl.dialog.as_ref())
            && let Some(snap) = from_panel(panel, true)
        {
            return Some(snap);
        }
        if let Some(id) = h.world.inline_dialogue.as_ref()
            && let Some(panel) = id.panel.as_ref()
        {
            return from_panel(panel, true);
        }
        None
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Draw lists for the retail dialog reading box over a `surface_w` x
    /// `surface_h` canvas. Same shape as
    /// [`Self::play_menu_draws_json`]: `{ "open", "sprites", "texts" }` -
    /// `sprites` sample the chrome atlas, `texts` the font atlas (upload both
    /// via the `play_menu_*` atlas accessors; this call builds the shared
    /// assets on first use). `open` is `false` when no box is up this frame.
    ///
    /// Unlike the pause menu the field keeps running underneath - retail
    /// draws the reading box over the live scene.
    pub fn play_dialog_draws_json(&mut self, surface_w: u32, surface_h: u32) -> String {
        const CLOSED: &str = r#"{"open":false,"sprites":[],"texts":[]}"#;
        if self.dialog_snapshot().is_none() || !self.ensure_menu_assets() {
            return CLOSED.to_string();
        }
        let Some(snap) = self.dialog_snapshot() else {
            return CLOSED.to_string();
        };
        let Some(assets) = self.menu_assets.as_ref() else {
            return CLOSED.to_string();
        };
        let (origin, scale) = crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
        let lay = dialog_stage_layout(&snap);
        let has_chrome = assets.chrome_rects().is_some();

        let mut sprites: Vec<SpriteDraw> = Vec::new();
        if let Some(rects) = assets.chrome_rects() {
            sprites.extend(ui::dialog_window_chrome_draws_for(
                rects, lay.main, origin, scale,
            ));
            if let Some(prect) = lay.picker {
                sprites.extend(ui::dialog_window_chrome_draws_for(
                    rects, prect, origin, scale,
                ));
                // Pointing-hand cursor on the selected option row
                // (FUN_8002B994 kind 0 at box_x-6, box_y + cursor*0xF).
                sprites.push(ui::dialog_option_hand_sprite(
                    rects,
                    (prect.0, prect.1),
                    snap.cursor,
                    origin,
                    scale,
                ));
            } else if snap.waiting {
                // Page-advance hand at the lower-right rim while the pager
                // waits for confirm (FUN_8002B994 kind 1).
                sprites.push(ui::dialog_advance_hand_sprite(
                    rects, lay.main, origin, scale,
                ));
            }
        }

        let font = assets.font_ref();
        let mut texts: Vec<TextDraw> = Vec::new();
        let (bx, by, _, _) = lay.main;
        // Main text: one row per 0x7C-separated line at the retail 15-px
        // pitch, pen at the box origin exactly, staged CLUT-7 white ink.
        for (i, line) in snap.page.split('|').enumerate() {
            texts.extend(ui::text_draws_for(
                &font.layout_ascii(line),
                (bx, by + i as i32 * 0xF),
                ui::MENU_TEXT_WHITE,
            ));
        }
        // Option-picker labels: CLUT-7 white at box_x + 0x10, 15-px pitch;
        // the hand sprite marks the selection. Keep a text `>` marker only
        // when the chrome atlas is missing (PROT.DAT-only load).
        if let Some((px, py, _, _)) = lay.picker {
            for (i, opt) in snap.options.iter().enumerate() {
                let selected = i == snap.cursor;
                let label = if has_chrome {
                    opt.clone()
                } else {
                    format!("{}{}", if selected { "> " } else { "  " }, opt)
                };
                texts.extend(ui::text_draws_for(
                    &font.layout_ascii(&label),
                    (px + 0x10, py + i as i32 * 0xF),
                    ui::MENU_TEXT_WHITE,
                ));
            }
        }
        ui::scale_stage_text_draws(&mut texts, origin, scale);

        serde_json::json!({
            "open": true,
            "sprites": sprites.iter().map(crate::play_menu::quad_json).collect::<Vec<_>>(),
            "texts": texts.iter().map(crate::play_menu::quad_json).collect::<Vec<_>>(),
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::dialog_reading_box_layout;

    /// The retail standard reading box: 3 rows regardless of typed lines
    /// below 3, centre rect `(0x26, 0x10, 0xF4, 0x2A)` - the geometry the
    /// `v0_1_tetsu_dialogue_accept` capture pins (drawn footprint = this
    /// rect inflated by the 8-px skin border).
    #[test]
    fn standard_reading_box_is_three_rows_at_the_top() {
        for lines in [1, 2, 3] {
            let (main, picker) = dialog_reading_box_layout(lines, 0);
            assert_eq!(main, (0x26, 0x10, 0xF4, 3 * 0xF - 3));
            assert!(picker.is_none());
        }
        let (tall, _) = dialog_reading_box_layout(4, 0);
        assert_eq!(tall, (0x26, 0x10, 0xF4, 4 * 0xF - 3));
    }

    /// Picker rects follow the picker-init arms' literal geometry: a 2-row
    /// picker sits at `y = 0x94 + 0xF`, height `0x38 - 2*0xF`.
    #[test]
    fn picker_rect_matches_the_init_arm_literals() {
        let (_, picker) = dialog_reading_box_layout(3, 2);
        assert_eq!(picker, Some((0x26, 0x94 + 0xF, 0xF4, 0x38 - 2 * 0xF)));
        let (_, four) = dialog_reading_box_layout(3, 4);
        assert_eq!(four, Some((0x26, 0x94, 0xF4, 0x38)));
    }
}
