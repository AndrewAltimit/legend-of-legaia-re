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
//! The whole channel - the box, the `4C E1` balloon, the map display's
//! labels, the tile-board prompt and the Incense notice - is composed by
//! `legaia_engine_screens::field_messages`, the native window's call too, so
//! the two hosts stack and gate it the same way. Geometry mirrors the traced pager (`FUN_801D84D0`):
//!
//! - Main (reading) box centre rect `(0x26, 0x10, 0xF4, lines*0xF - 3)`,
//!   anchored at the TOP of the 320x240 stage; `lines` is the pager's window
//!   word (`_DAT_801F2740`, 3 unless op `4C E0` set another), and only the
//!   plain-MES panel sizes off its page (3..=4 rows). The drawn skin extends 8 px beyond the
//!   centre rect on every side (the chrome builder's inflation).
//! - Picker box: the engine's slide rect
//!   ([`legaia_engine_dialog::dialog::OwnedDialogPanel::picker_rect`]), the
//!   same one the native window reads - the box slides in from off screen
//!   and rests at `(0xD8, 0x4A, 0x58, 0x1A)` for a `0x2A` menu, or at
//!   `(0x26, 0x94 + ((4-n)*0xF)/2, 0xF4, 0x38 - (4-n)*0xF)` for the N-option
//!   lists ([`legaia_engine_dialog::dialog_picker_slide`]).
//! - Text pen = box origin exactly (`FUN_80036888(line, 0, 0, ctx+0x12,
//!   ctx+0x14 + i*0xF)`), 15-px row pitch, body ink the staged CLUT-7
//!   (206,206,206) menu white; picker labels at `box_x + 0x10`.
//! - Advance hand at `x + w - 0x10`, `0x10` above the centre-rect bottom
//!   (`FUN_8002B994` kind 1); option hand on the selected row (kind 0), once
//!   the slide rests.
//!
//! REF: FUN_801D84D0, FUN_8002C69C, FUN_8002B994

use super::*;
use crate::runtime::LegaiaRuntime;
use legaia_engine_screens::field_messages as fm;
use legaia_engine_ui as ui;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Draw lists for the field message layer over a `surface_w` x
    /// `surface_h` canvas: the `4C E1` balloon, the map display's labels,
    /// the tile-board prompt, the Incense notice and the retail dialog
    /// reading box, composed by
    /// [`legaia_engine_screens::field_messages::field_message_frame`] (the
    /// native window's call too). Same shape as
    /// [`Self::play_menu_draws_json`]: `{ "open", "sprites", "texts" }` -
    /// `sprites` sample the chrome atlas, `texts` the font atlas (upload both
    /// via the `play_menu_*` atlas accessors; this call builds the shared
    /// assets on first use). `open` is `false` when nothing is up this frame.
    ///
    /// Unlike the pause menu the field keeps running underneath - retail
    /// draws the reading box over the live scene.
    pub fn play_dialog_draws_json(&mut self, surface_w: u32, surface_h: u32) -> String {
        const CLOSED: &str = r#"{"open":false,"sprites":[],"texts":[]}"#;
        if self.scene_host.host().is_none() || !self.ensure_menu_assets() {
            return CLOSED.to_string();
        }
        // The balloon's width is measured in the host font and handed back
        // to the engine before the draw (retail measures at spawn, inside
        // `FUN_8003C764`).
        if let (Some(assets), Some(host)) = (self.menu_assets.as_ref(), self.scene_host.host_mut())
        {
            fm::commit_text_balloon_measure(&mut host.world, assets.font_ref());
        }
        let (Some(assets), Some(host)) = (self.menu_assets.as_ref(), self.scene_host.host()) else {
            return CLOSED.to_string();
        };
        let (surface_w, surface_h) = (surface_w.max(1), surface_h.max(1));
        // The page runs with `use_vm_dialogue` armed and has no typed-out
        // panel of its own.
        let frame = fm::field_message_frame(
            &fm::FieldMessageInputs {
                host,
                host_panel: None,
                font: assets.font_ref(),
                chrome: assets.chrome_rects(),
            },
            surface_w,
            surface_h,
        );
        if frame.is_empty() {
            return CLOSED.to_string();
        }
        let (origin, scale) = crate::play_menu::stage_transform(surface_w, surface_h);
        let mut texts = frame.stage_texts;
        ui::scale_stage_text_draws(&mut texts, origin, scale);
        serde_json::json!({
            "open": true,
            "sprites": frame.sprites.iter().map(crate::play_menu::quad_json).collect::<Vec<_>>(),
            "texts": texts.iter().map(crate::play_menu::quad_json).collect::<Vec<_>>(),
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
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
        use legaia_engine_dialog::text_balloon::{BALLOON_TOTAL, TextBalloon, balloon_center_x};

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
            let host = rt.scene_host.host_mut().expect("scene host");
            host.world.cutscene.text_balloon = Some(TextBalloon::spawn(line));
            assert!(
                host.world
                    .cutscene
                    .text_balloon
                    .as_ref()
                    .unwrap()
                    .x
                    .is_none()
            );
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
        let world = &rt.scene_host.host().unwrap().world;
        let x = world
            .cutscene
            .text_balloon
            .as_ref()
            .unwrap()
            .x
            .expect("centred");
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
            rt.scene_host
                .host()
                .unwrap()
                .world
                .cutscene
                .text_balloon
                .is_none(),
            "the balloon must retire itself"
        );
        let after: serde_json::Value =
            serde_json::from_str(&rt.play_dialog_draws_json(960, 720)).unwrap();
        assert_eq!(after["open"], serde_json::json!(false));
    }
}
