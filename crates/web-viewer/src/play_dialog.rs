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

/// What one frame's `4C E1` balloon needs to draw: the page bytes, the centred
/// text pen, and the window rect - all in 320x240 stage pixels.
type TextBalloonPlacement = (Vec<u8>, (i32, i32), (i32, i32, i32, i32));

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

    /// Measure the live `4C E1` balloon in the host font, commit the width to
    /// the engine, and return `(page bytes, pen, frame rect)` for the draw -
    /// or `None` when no balloon is drawable this frame.
    ///
    /// The measurement round-trip is the whole reason the record's `x`
    /// existed as an `Option`: retail measures inside the spawner, the engine
    /// cannot, so the draw layer measures and hands the width back
    /// ([`legaia_engine_core::world::World::commit_text_balloon_width`]). The
    /// startup band is excluded by `text_balloon_drawing`, matching the
    /// handler's `timer < 1` arm, which draws nothing.
    fn take_text_balloon_placement(&mut self) -> Option<TextBalloonPlacement> {
        let text = self
            .scene_host
            .as_ref()?
            .world
            .text_balloon_drawing()?
            .to_vec();
        let width = ui::text_balloon_text_width(self.menu_assets.as_ref()?.font_ref(), &text);
        let host = self.scene_host.as_mut()?;
        let rect = host.world.text_balloon.as_ref()?.frame_rect();
        let pen = host.world.commit_text_balloon_width(width)?;
        Some((text, pen, rect))
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
        let has_balloon = self
            .scene_host
            .as_ref()
            .is_some_and(|h| h.world.text_balloon_drawing().is_some());
        if (self.dialog_snapshot().is_none() && !has_balloon) || !self.ensure_menu_assets() {
            return CLOSED.to_string();
        }
        // The `4C E1` balloon: measure the line in the host font, hand the
        // width back to the engine (retail measures at spawn, inside
        // `FUN_8003C764`), and keep the pen + frame rect the engine derives.
        let balloon = self.take_text_balloon_placement();
        let snap = self.dialog_snapshot();
        let Some(assets) = self.menu_assets.as_ref() else {
            return CLOSED.to_string();
        };
        let (origin, scale) = crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
        let has_chrome = assets.chrome_rects().is_some();
        let font = assets.font_ref();

        let mut sprites: Vec<SpriteDraw> = Vec::new();
        let mut texts: Vec<TextDraw> = Vec::new();
        if let Some((text, pen, rect)) = balloon.as_ref() {
            if let Some(rects) = assets.chrome_rects() {
                sprites.extend(ui::text_balloon_chrome_draws_for(
                    rects, *rect, origin, scale,
                ));
            }
            texts.extend(ui::text_balloon_text_draws_for(font, text, *pen));
        }

        let Some(snap) = snap else {
            // Balloon only - no reading box up this frame.
            ui::scale_stage_text_draws(&mut texts, origin, scale);
            return serde_json::json!({
                "open": !texts.is_empty() || !sprites.is_empty(),
                "sprites": sprites.iter().map(crate::play_menu::quad_json).collect::<Vec<_>>(),
                "texts": texts.iter().map(crate::play_menu::quad_json).collect::<Vec<_>>(),
            })
            .to_string();
        };
        let lay = dialog_stage_layout(&snap);

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

    /// The `4C E1` balloon reaches the page's draw channel, is centred from a
    /// real font measurement, and stops drawing when it dies.
    ///
    /// A crate-internal test because `scene_host` is `pub(crate)` and no
    /// public API spawns a balloon - the five on-disc `4C E1` sites sit
    /// mid-script, and reaching one by play is a ladder this file cannot
    /// build. What is exercised is everything downstream of the spawn: the
    /// measurement round-trip, the startup-band gate, the chrome + text
    /// emission and the teardown.
    ///
    /// Disc-gated: the font atlas + chrome rects come off the disc.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn text_balloon_reaches_the_page_draw_channel() {
        use legaia_engine_core::text_balloon::{BALLOON_TOTAL, TextBalloon, balloon_center_x};

        let Ok(disc) = std::env::var("LEGAIA_DISC_BIN") else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
            return;
        };
        let Ok(bytes) = std::fs::read(disc) else {
            eprintln!("[skip] disc image unreadable");
            return;
        };
        let mut rt = crate::runtime::LegaiaRuntime::new();
        if rt.load_disc(bytes, String::new()).is_err() {
            eprintln!("[skip] disc load failed");
            return;
        }
        if rt.enter_field("town01").is_err() {
            eprintln!("[skip] town01 unavailable");
            return;
        }

        // Contrast: no balloon, no dialog -> the channel is closed.
        let closed: serde_json::Value =
            serde_json::from_str(&rt.play_dialog_draws_json(960, 720)).unwrap();
        assert_eq!(closed["open"], serde_json::json!(false));

        // Spawn one exactly as the field-VM arm does (unmeasured, `x` unset).
        let line: &[u8] = b"BALLOON";
        {
            let host = rt.scene_host.as_mut().expect("scene host");
            host.world.text_balloon = Some(TextBalloon::spawn(line));
            assert!(host.world.text_balloon.as_ref().unwrap().x.is_none());
            // Startup band: retail's handler draws nothing on the first tick.
            assert!(host.world.text_balloon_drawing().is_none());
        }
        let startup: serde_json::Value =
            serde_json::from_str(&rt.play_dialog_draws_json(960, 720)).unwrap();
        assert_eq!(
            startup["open"],
            serde_json::json!(false),
            "the startup band must draw nothing"
        );

        // One tick leaves the startup band; now it draws.
        let _ = rt.tick_frame();
        let open: serde_json::Value =
            serde_json::from_str(&rt.play_dialog_draws_json(960, 720)).unwrap();
        assert_eq!(open["open"], serde_json::json!(true));
        let texts = open["texts"].as_array().expect("texts");
        assert!(!texts.is_empty(), "the balloon line must emit glyph quads");
        assert!(
            !open["sprites"].as_array().expect("sprites").is_empty(),
            "the balloon must be framed by the window skin"
        );

        // The measurement round-trip landed on the record, and it is the
        // font's own width - not a default.
        let world = &rt.scene_host.as_ref().unwrap().world;
        let x = world.text_balloon.as_ref().unwrap().x.expect("centred");
        let font_w = legaia_engine_ui::text_balloon_text_width(
            rt.menu_assets.as_ref().unwrap().font_ref(),
            line,
        );
        assert!(font_w > 0, "the disc font must measure the line");
        assert_eq!(x, balloon_center_x(font_w));
        assert_ne!(
            x,
            balloon_center_x(0),
            "an unmeasured balloon would centre a zero-width line"
        );

        // Run it out: the handler kills itself and the channel closes again.
        for _ in 0..(BALLOON_TOTAL as u32 + 8) {
            let _ = rt.tick_frame();
        }
        assert!(
            rt.scene_host.as_ref().unwrap().world.text_balloon.is_none(),
            "the balloon must retire itself"
        );
        let after: serde_json::Value =
            serde_json::from_str(&rt.play_dialog_draws_json(960, 720)).unwrap();
        assert_eq!(after["open"], serde_json::json!(false));
    }
}
