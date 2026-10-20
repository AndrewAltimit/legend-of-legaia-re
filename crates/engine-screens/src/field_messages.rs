//! The **field message layer** both play hosts draw: the `4C E1` text
//! balloon, the overworld map display's place labels and compass letters,
//! the tile board's quit prompt, the Incense wear-off notice and the dialog
//! reading box - composed once, in one order, under one set of gates.
//!
//! Each host used to assemble this layer itself, and the two disagreed: the
//! native window stacked the map labels under the tile-board prompt where
//! the page stacked them over the Incense notice, its text pass ran in a
//! third order, and the window framed the Incense notice whenever the notice
//! was up while the page framed it only when its line resolved.
//!
//! **Order** (back to front, in both the sprite and the text half): balloon,
//! map labels + compass, tile-board prompt, Incense notice, reading box.
//! Retail's own stacking of these is not pinned: every one of them frames
//! through the SCUS window emitter `FUN_8002C69C` and prints through
//! `FUN_80036888`, and which OT depth each caller links at is not traced.
//! The order is chosen. The balloon is its own actor and outlives the dialog
//! engagement that spawned it, so the modal panels draw over it; the reading
//! box is the one panel that takes the pad mid-scene, so it draws last. The
//! map display, the tile board and the Incense notice belong to different
//! modes and do not share a frame in play.
//!
//! **Gates.** The balloon frames while `World::text_balloon_drawing` holds
//! (its startup band draws nothing) and prints once a host has committed its
//! measured width ([`commit_text_balloon_measure`]). The Incense notice
//! frames whenever `World::incense_notice_shown` holds - the panel install
//! `FUN_801F1E48` folds does not depend on the string - and prints its line
//! when it resolves from the field overlay.

use legaia_engine_core::world::World;
use legaia_engine_dialog::dialog::OwnedDialogPanel;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_ui::{self as ui, SaveMenuAtlasRects, SpriteDraw, TextDraw};

use crate::dialog_screens::{self, DialogSnapshot};

/// Everything the layer reads, borrowed from whichever host holds it.
#[derive(Clone, Copy)]
pub struct FieldMessageInputs<'a> {
    /// The live scene: its world, and the PROT index the Incense line and
    /// the tile-board prompt strings are read from.
    pub host: &'a SceneHost,
    /// The host's own typed-out dialog panel (the native window's
    /// pre-field-VM fallback; `None` on a host without one).
    pub host_panel: Option<&'a OwnedDialogPanel>,
    pub font: &'a legaia_font::Font,
    /// The system-UI chrome atlas rects. Without them no frame draws and the
    /// picker labels carry their own ASCII cursor.
    pub chrome: Option<&'a SaveMenuAtlasRects>,
}

/// One frame of the field message layer.
#[derive(Debug, Clone, Default)]
pub struct FieldMessageFrame {
    /// Surface-pixel chrome sprites, back to front.
    pub sprites: Vec<SpriteDraw>,
    /// **Stage**-pixel texts, back to front; the host scales them
    /// (`ui::scale_stage_text_draws`).
    pub stage_texts: Vec<TextDraw>,
    /// A dialog reading box is up this frame.
    pub reading_box: bool,
}

impl FieldMessageFrame {
    /// Nothing to draw this frame.
    pub fn is_empty(&self) -> bool {
        self.sprites.is_empty() && self.stage_texts.is_empty() && !self.reading_box
    }
}

/// Commit the live `4C E1` balloon's width, measured in the host's `font`
/// (`World::commit_text_balloon_width`). Retail measures inside the spawner
/// (`FUN_8003C764` via `FUN_80035F04`); the engine's font is host-side, so a
/// host runs this before it draws. Idempotent; a no-op with no live balloon
/// or one already measured.
pub fn commit_text_balloon_measure(world: &mut World, font: &legaia_font::Font) {
    let Some(b) = world.cutscene.text_balloon.as_ref() else {
        return;
    };
    if b.x.is_some() || b.killed {
        return;
    }
    let width = ui::text_balloon_text_width(font, &b.text);
    world.commit_text_balloon_width(width);
}

/// The field message layer for one frame over a `surface_w x surface_h`
/// surface. See the module docs for the order and the gates.
pub fn field_message_frame(
    inputs: &FieldMessageInputs<'_>,
    surface_w: u32,
    surface_h: u32,
) -> FieldMessageFrame {
    let host = inputs.host;
    let world = &host.world;
    let font = inputs.font;
    let chrome = inputs.chrome;
    let (origin, scale) = ui::pause_menu::stage_transform(surface_w, surface_h);
    let mut out = FieldMessageFrame::default();

    // The `4C E1` balloon: the fixed `0x58 x 0x90` window
    // (`FUN_8002C69C(0x58, y, 0x90, 0xB)`) and the single line centred on the
    // full 320-px screen (`FUN_80036888(text, 0, 0, x, y)`).
    if let Some(text) = world.text_balloon_drawing()
        && let Some(balloon) = world.cutscene.text_balloon.as_ref()
    {
        if let Some(rects) = chrome {
            out.sprites.extend(ui::text_balloon_chrome_draws_for(
                rects,
                balloon.frame_rect(),
                origin,
                scale,
            ));
        }
        if let Some(pen) = balloon.pen() {
            out.stage_texts
                .extend(ui::text_balloon_text_draws_for(font, text, pen));
        }
    }

    // The overworld map display (retail game mode `0x0D`, PROT 0981's label
    // pass): one box per discovered place, then the compass letters.
    let labels = crate::world_map_labels::map_display_labels_now(world);
    let marks = crate::world_map_labels::map_display_compass_marks(world);
    if !labels.is_empty() || !marks.is_empty() {
        let rows: Vec<ui::MapLabelDraw<'_>> = labels
            .iter()
            .map(|l| (l.name.as_slice(), l.sx, l.sy))
            .collect();
        if let Some(rects) = chrome {
            out.sprites.extend(ui::map_label_chrome_draws_for(
                rects, font, &rows, origin, scale,
            ));
        }
        out.stage_texts
            .extend(ui::map_label_text_draws_for(font, &rows));
        out.stage_texts
            .extend(ui::map_compass_text_draws_for(font, &marks));
    }

    // The tile board's quit prompt (walk-SM state 5): the field overlay's own
    // title and two rows.
    if let Some(cursor) = world.tile_board_prompt_cursor() {
        let (frame, rows, cursor_x) = legaia_engine_minigames::tile_board::prompt_layout();
        let lay = ui::TileBoardPromptLayout {
            frame,
            rows,
            cursor_x,
        };
        if let Some(rects) = chrome {
            out.sprites.extend(ui::tile_board_prompt_sprites_for(
                rects, &lay, cursor, origin, scale,
            ));
        }
        if let Some(lines) = host.tile_board_prompt_lines() {
            out.stage_texts
                .extend(ui::tile_board_prompt_text_draws_for(font, &lay, &lines));
        }
    }

    // The Incense wear-off notice (`FUN_801F1E48`).
    if world.incense_notice_shown() {
        if let Some(rects) = chrome {
            out.sprites.extend(ui::incense_notice_sprites_for(
                rects,
                legaia_engine_core::incense_notice::notice_frame_rect(),
                origin,
                scale,
            ));
        }
        if let Some(line) = host.incense_notice_line() {
            out.stage_texts.extend(ui::incense_notice_text_draws_for(
                font,
                legaia_engine_core::incense_notice::notice_text_pen(),
                &line,
            ));
        }
    }

    // The dialog reading box (`FUN_801D84D0`) and its picker, on top.
    if let Some(snap) = field_dialog_snapshot(inputs) {
        out.reading_box = true;
        if let Some(rects) = chrome {
            out.sprites.extend(dialog_screens::dialog_box_chrome_draws(
                rects, &snap, origin, scale,
            ));
        }
        out.stage_texts
            .extend(dialog_screens::dialog_box_text_draws(
                font,
                &snap,
                chrome.is_some(),
            ));
    }
    out
}

/// The live dialog source for these inputs
/// ([`dialog_screens::dialog_snapshot`]).
pub fn field_dialog_snapshot(inputs: &FieldMessageInputs<'_>) -> Option<DialogSnapshot> {
    dialog_screens::dialog_snapshot(&inputs.host.world, inputs.host_panel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_dialog::text_balloon::TextBalloon;
    use legaia_engine_scene::scene::ProtIndex;

    /// A two-entry PROT.DAT with no scene in it: enough for a `SceneHost`,
    /// and nothing the Incense line or the tile-board strings resolve from.
    fn bare_host() -> SceneHost {
        let mut buf = vec![0u8; 0x3800];
        buf[4..8].copy_from_slice(&2u32.to_le_bytes());
        for (i, v) in [1u32, 0, 1, 5, 7, 6, 5].iter().enumerate() {
            let off = 0x08 + i * 4;
            buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
        }
        let index = ProtIndex::from_bytes(buf, None).expect("synthetic PROT.DAT");
        SceneHost::new(std::sync::Arc::new(index))
    }

    fn chrome() -> SaveMenuAtlasRects {
        let cell = (0, 0, 8, 8);
        SaveMenuAtlasRects {
            panel_tl: cell,
            panel_tr: cell,
            panel_bl: cell,
            panel_br: cell,
            panel_top: cell,
            panel_bot: cell,
            panel_left: cell,
            panel_right: cell,
            panel_interior: cell,
            dialog_fill: cell,
            ..Default::default()
        }
    }

    fn frame(
        host: &SceneHost,
        font: &legaia_font::Font,
        rects: &SaveMenuAtlasRects,
    ) -> FieldMessageFrame {
        field_message_frame(
            &FieldMessageInputs {
                host,
                host_panel: None,
                font,
                chrome: Some(rects),
            },
            960,
            720,
        )
    }

    fn show_incense(host: &mut SceneHost) {
        host.world.raise_incense_notice();
        host.world
            .locomotion
            .incense_notice
            .as_mut()
            .expect("raised")
            .shown = true;
    }

    /// Nothing up, nothing drawn.
    #[test]
    fn an_idle_field_draws_no_message() {
        let host = bare_host();
        let font = legaia_font::Font::placeholder();
        assert!(frame(&host, &font, &chrome()).is_empty());
    }

    /// The Incense notice frames whenever it is shown, with or without its
    /// line: the browser page used to frame it only once the line resolved.
    #[test]
    fn the_incense_notice_frames_without_its_line() {
        let mut host = bare_host();
        show_incense(&mut host);
        assert!(host.incense_notice_line().is_none(), "no field overlay");
        let font = legaia_font::Font::placeholder();
        let rects = chrome();
        let f = frame(&host, &font, &rects);
        let (origin, scale) = ui::pause_menu::stage_transform(960, 720);
        let want = ui::incense_notice_sprites_for(
            &rects,
            legaia_engine_core::incense_notice::notice_frame_rect(),
            origin,
            scale,
        );
        assert!(!want.is_empty());
        let got: Vec<_> = f.sprites.iter().map(|s| s.dst).collect();
        let want: Vec<_> = want.iter().map(|s| s.dst).collect();
        assert_eq!(got, want);
        assert!(f.stage_texts.is_empty());
    }

    /// The balloon draws under the Incense notice.
    #[test]
    fn the_balloon_stacks_under_the_notice() {
        let mut host = bare_host();
        let font = legaia_font::Font::placeholder();
        let rects = chrome();
        let mut balloon = TextBalloon::spawn(b"HELLO");
        balloon.timer = 2;
        host.world.cutscene.text_balloon = Some(balloon);
        commit_text_balloon_measure(&mut host.world, &font);
        let alone = frame(&host, &font, &rects);
        assert!(!alone.sprites.is_empty());
        assert!(!alone.stage_texts.is_empty(), "measured, so it prints");

        show_incense(&mut host);
        let both = frame(&host, &font, &rects);
        let n = alone.sprites.len();
        assert!(both.sprites.len() > n);
        let head: Vec<_> = both.sprites[..n].iter().map(|s| s.dst).collect();
        let balloon_dst: Vec<_> = alone.sprites.iter().map(|s| s.dst).collect();
        assert_eq!(head, balloon_dst, "the balloon's chrome is drawn first");
        assert_eq!(both.stage_texts.len(), alone.stage_texts.len());
    }
}
