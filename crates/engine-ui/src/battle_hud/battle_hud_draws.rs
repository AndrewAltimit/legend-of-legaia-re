//! The battle HUD draw builder.

use super::*;
use crate::battle_hud_chrome::{
    STATUS_BADGE_PANEL_SEAT, STATUS_BADGE_SIZE, STATUS_BADGE_TAG_DY,
    message_banner_chrome_draws_for, message_banner_content, message_banner_text_draws_for,
};

/// Build the battle-HUD draw lists ([`BattleHudDraws`]).
///
/// The default (retail-shaped) surface:
///
/// * **The party strip** - ONE full-width strip across the bottom of the
///   320x240 stage, per the retail captures (see [`STRIP_STAGE_X`] for the
///   per-constant provenance): ornate frame + blue-gradient interior
///   (system-UI atlas chrome via [`BattleHudFrame::chrome`]), and per
///   member the name, the gold `HP` label sprite with right-aligned
///   `cur / max` numerals, and the green `MP` label sprite with numerals.
///   Numerals take the retail readout-tint law ([`hp_bar_color_index`] /
///   [`mp_bar_color_index`]); a dead member's name + numerals dim. Retail
///   draws **no HP/MP bars and no AP pips in the strip** - the captured
///   strip is numerals-only - so the engine draws none by default.
/// * **The plaque** - the acting actor's name as clean text at the
///   top-left plaque inset ([`BattleHudFrame::plaque`]).
/// * **Status badge** - when a slot's retail-selected status element
///   ([`HudSlotView::status_sprite`], `FUN_8002C2E4`'s ladder) is an
///   ailment, a labelled badge above the member's strip section (the
///   selection is retail; badge art + placement are engine approximations).
/// * Damage popups + the log column.
///
/// Retail draws **no monster gauge at all**
/// (`docs/subsystems/battle-action.md`), so monster rows, the per-slot
/// LV / AP readouts and the thin bars only draw under
/// [`BattleHudFrame::diag`] - the debug surface, off by default.
///
/// All filled rects ride [`BattleHudFrame::solid_src`]; without it the
/// builder emits text only. Chrome sprites ride
/// [`BattleHudFrame::chrome`]; without it the strip backdrop degrades to
/// solid-texel rects and the labels to tinted text.
pub fn battle_hud_draws_for(
    font: &legaia_font::Font,
    frame: &BattleHudFrame<'_>,
    pen: (i32, i32),
) -> BattleHudDraws {
    const LINE_H: i32 = 14;
    /// Diagnostic-row column origins (surface px, relative to `pen.x`): HP
    /// numerals, K.O. tag, status element, LV/AP tail. Sized from measured
    /// advances of the retail dialog font (longest monster name 69 px +
    /// gutter).
    const HP_X: i32 = 78;
    const KO_X: i32 = 150;
    const STATUS_X: i32 = 190;
    const DIAG_TAIL_X: i32 = 230;
    const POPUP_X: i32 = 80;
    /// Diagnostic HP bar width / height, surface px.
    const MBAR_W: i32 = 60;
    const MBAR_H: i32 = 3;

    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    // Glyph ink of every battle caption - the plaques, the tabs, the move
    // name and the message bar. Retail draws them all through the font's
    // tier-7 CLUT, whose entry 15 is `(206, 206, 206)` on screen, not white:
    // `white` stays the neutral modulation of the textured sprites.
    let ink: [f32; 4] = READOUT_NORMAL;
    let monster: [f32; 4] = [1.0, 0.7, 0.7, 1.0];
    let dim: [f32; 4] = [0.5, 0.5, 0.5, 1.0];
    let red: [f32; 4] = [1.0, 0.4, 0.4, 1.0];
    let green: [f32; 4] = [0.5, 1.0, 0.5, 1.0];
    let yellow: [f32; 4] = [1.0, 0.95, 0.4, 1.0];
    let cyan: [f32; 4] = [0.5, 0.85, 1.0, 1.0];
    // Fallback strip chrome (no system-UI atlas): interior + outline tints
    // approximating the captured strip's navy interior and light-blue rim.
    let strip_bg: [f32; 4] = [0.13, 0.13, 0.35, 0.85];
    let strip_frame: [f32; 4] = [0.58, 0.58, 0.84, 0.95];
    let bar_back: [f32; 4] = [0.10, 0.10, 0.12, 0.90];
    // Fallback label tints when the atlas sprites are unavailable: the
    // captured labels are gold "HP" / green "MP".
    let label_gold: [f32; 4] = [1.0, 0.78, 0.15, 1.0];
    let label_green: [f32; 4] = [0.25, 0.92, 0.55, 1.0];

    // The boot/menu stage transform: integer upscale of the 320x240 stage,
    // centred in the surface.
    let scale = (frame.surface.0 / BOOT_UI_STAGE_W)
        .min(frame.surface.1 / BOOT_UI_STAGE_H)
        .clamp(1, 4) as i32;
    let origin = (
        (frame.surface.0 as i32 - BOOT_UI_STAGE_W as i32 * scale) / 2,
        (frame.surface.1 as i32 - BOOT_UI_STAGE_H as i32 * scale) / 2,
    );

    let mut text: Vec<TextDraw> = Vec::new();
    let mut sprites: Vec<SpriteDraw> = Vec::new();
    // Solid rect in stage pixels (scaled into the surface).
    let stage_rect = |out: &mut Vec<TextDraw>, x: i32, y: i32, w: i32, h: i32, c: [f32; 4]| {
        if let Some(src) = frame.solid_src
            && w > 0
            && h > 0
        {
            out.push(TextDraw {
                dst: (
                    origin.0 + x * scale,
                    origin.1 + y * scale,
                    (w * scale) as u32,
                    (h * scale) as u32,
                ),
                src,
                color: c,
            });
        }
    };
    // Text laid out at a stage-pixel pen, upscaled with the stage transform
    // (same shape as `scale_stage_text_draws`, applied per burst).
    let stage_text = |out: &mut Vec<TextDraw>,
                      font: &legaia_font::Font,
                      s: &str,
                      x: i32,
                      y: i32,
                      c: [f32; 4]| {
        let layout = font.layout_ascii(s);
        let mut draws = text_draws_for(&layout, (x, y), c);
        scale_stage_text_draws(&mut draws, origin, scale as u32);
        out.extend(draws);
    };

    // Retail readout-tint law -> HUD palette. Every tier but "normal" takes
    // retail's own CLUT colour ([`gauge_fill_color`]); the normal tier keeps
    // the caller's base so the diagnostic monster rows stay row-tinted.
    let tint = |idx: u8, base: [f32; 4]| -> [f32; 4] {
        match idx {
            2 | 3 | 6 | 9 => gauge_fill_color(idx),
            _ => base,
        }
    };

    // Per-slot popup anchor, filled in as rows/panels are laid out. Surface px.
    let mut popup_anchor: Vec<(i32, i32)> = frame
        .slots
        .iter()
        .enumerate()
        .map(|(i, _)| (pen.0 + POPUP_X, pen.1 + i as i32 * LINE_H - 16))
        .collect();

    // ---- The retail party strip ----
    //
    // ONE full-width strip across the stage bottom (see [`STRIP_STAGE_X`]
    // for the per-constant provenance). Solo lays the single member's
    // name / HP / MP across the measured retail columns; a pair or trio
    // takes one section per member at the pinned `FUN_801D84C0` anchors
    // with the rows stacked (approximation - no multi-member retail
    // capture pins the section contents).
    let live_party: Vec<(usize, &HudSlotView<'_>)> = frame
        .slots
        .iter()
        .enumerate()
        .filter(|(_, s)| s.is_party && !s.name.is_empty())
        .collect();

    // Sprite blit in stage pixels.
    let stage_sprite = |out: &mut Vec<SpriteDraw>, src: (u32, u32, u32, u32), x: i32, y: i32| {
        out.push(SpriteDraw {
            dst: (
                origin.0 + x * scale,
                origin.1 + y * scale,
                src.2 * scale as u32,
                src.3 * scale as u32,
            ),
            src,
            color: white,
        });
    };
    // LV / HP / MP label: the resident system-UI atlas sprites
    // (`OVERLAY_SYSTEM_UI_LABEL_LV` / `_HP` / `_MP`, `legaia_ui_formats::title_pak`).
    // Retail packs all three into **one** sub-palette - the gold-vs-green
    // difference is baked into the texels, not a per-label CLUT - so they
    // draw untinted. Without an atlas they degrade to tinted text at the same
    // seats.
    let label =
        |sprites: &mut Vec<SpriteDraw>, text: &mut Vec<TextDraw>, which: u8, x: i32, y: i32| {
            match frame.chrome {
                Some(rects) => {
                    let src = match which {
                        0 => rects.label_hp,
                        1 => rects.label_mp,
                        _ => rects.label_lv,
                    };
                    stage_sprite(sprites, src, x, y);
                }
                None => {
                    let (s, c) = match which {
                        0 => ("HP", label_gold),
                        1 => ("MP", label_green),
                        _ => ("LV", label_gold),
                    };
                    stage_text(text, font, s, x, y, c);
                }
            }
        };
    // The battle screen's own cells, when the atlas carries them.
    let battle = frame.chrome.and_then(|r| r.battle);
    // The `/` between a current and a maximum: retail's 8x16 sheet sprite,
    // which seats four rows **above** its numerals. Without the atlas it
    // degrades to a font glyph on the numeral row.
    let separator =
        |sprites: &mut Vec<SpriteDraw>, text: &mut Vec<TextDraw>, x: i32, y: i32, c: [f32; 4]| {
            match battle {
                Some(b) => sprites.push(SpriteDraw {
                    dst: (
                        origin.0 + x * scale,
                        origin.1 + y * scale,
                        b.separator.2 * scale as u32,
                        b.separator.3 * scale as u32,
                    ),
                    src: b.separator,
                    color: numeral_cell_tint(c),
                }),
                None => stage_text(text, font, "/", x, y + 4, c),
            }
        };
    // A number, in retail's own numeral cells: 8x12 sprites off the
    // menu-glyph atlas strip, right-aligned so the field grows leftward one
    // cell per digit.
    //
    // This - not the dialog font - is how retail draws every HP, MP and
    // level on the battle screen, and it is what the seats above were
    // measured against. A proportional-font `9999` is wider than four cells
    // and overruns the roster panel's 102-px plate; four cells do not.
    //
    // Without the strip (an atlas built without the menu-glyph TIM, or no
    // atlas at all) the digits fall back to font glyphs **centred in the
    // same cells**, so the layout is identical and only the letterforms
    // differ.
    let numerals = |sprites: &mut Vec<SpriteDraw>,
                    text: &mut Vec<TextDraw>,
                    value: u32,
                    right: i32,
                    y: i32,
                    c: [f32; 4]| {
        let s = value.to_string();
        let left = right - s.len() as i32 * DIGIT_W;
        for (i, ch) in s.bytes().enumerate() {
            let cell_x = left + i as i32 * DIGIT_W;
            let d = u32::from(ch - b'0');
            match battle
                .and_then(|b| b.digits)
                .and_then(|strip| crate::hud_digit_rect(strip, d))
            {
                Some(src) => sprites.push(SpriteDraw {
                    dst: (
                        origin.0 + cell_x * scale,
                        origin.1 + y * scale,
                        src.2 * scale as u32,
                        src.3 * scale as u32,
                    ),
                    src,
                    color: numeral_cell_tint(c),
                }),
                None => {
                    let glyph = [ch];
                    let g = std::str::from_utf8(&glyph).unwrap_or("0");
                    let advance = font.layout_ascii(g).advance_x as i32;
                    stage_text(text, font, g, cell_x + (DIGIT_W - advance) / 2, y, c);
                }
            }
        }
    };
    // Fallback skin for a plate or panel with no atlas at all: a solid
    // interior plus a 1-px rim, which is what keeps a chrome-less host
    // readable.
    let flat_skin = |text: &mut Vec<TextDraw>, rect: (i32, i32, i32, i32)| {
        stage_rect(text, rect.0, rect.1, rect.2, rect.3, strip_bg);
        stage_rect(text, rect.0, rect.1, rect.2, 1, strip_frame);
        stage_rect(text, rect.0, rect.1 + rect.3 - 1, rect.2, 1, strip_frame);
        stage_rect(text, rect.0, rect.1, 1, rect.3, strip_frame);
        stage_rect(text, rect.0 + rect.2 - 1, rect.1, 1, rect.3, strip_frame);
    };
    // One plate run: retail's 3-slice out of the system-UI sheet - an 8-px
    // cap, body tiles filling the interior with the **last tile clipped** to
    // the remainder, and a closing cap. Total width is `interior + 16`, and
    // the clipped final tile is retail's behaviour rather than a rounding of
    // it (`battle_chrome::plate_run`).
    //
    // `art` picks the sheet row: the blue row (`v = 0`) for the bar and the
    // command chips, the carved-gold row (`v = 64`) for the actor plaque.
    // The gold row is the same tiles the field menu's tab banner samples,
    // which is why they are the atlas's `tab_*` rects.
    let plate_run = |text: &mut Vec<TextDraw>,
                     sprites: &mut Vec<SpriteDraw>,
                     x: i32,
                     y: i32,
                     interior_w: i32,
                     gold: bool| {
        let tiles = match (frame.chrome, battle) {
            (Some(r), Some(_)) if gold => Some((r.tab_cap_l, r.tab_body, r.tab_cap_r)),
            (Some(_), Some(b)) => Some((b.plate_cap_l, b.plate_body, b.plate_cap_r)),
            _ => {
                flat_skin(text, (x, y, interior_w + 2 * PLATE_CAP_W, PLATE_H));
                None
            }
        };
        let Some((cap_l, body, cap_r)) = tiles else {
            return;
        };
        let mut blit = |src: (u32, u32, u32, u32), bx: i32, w: u32| {
            sprites.push(SpriteDraw {
                dst: (
                    origin.0 + bx * scale,
                    origin.1 + y * scale,
                    w * scale as u32,
                    src.3 * scale as u32,
                ),
                src: (src.0, src.1, w, src.3),
                color: white,
            });
        };
        blit(cap_l, x, cap_l.2);
        let interior_x = x + PLATE_CAP_W;
        let mut done = 0;
        while done < interior_w {
            let w = (body.2 as i32).min(interior_w - done);
            blit(body, interior_x + done, w as u32);
            done += w;
        }
        blit(cap_r, interior_x + interior_w, cap_r.2);
    };
    // The roster panel's own background: retail blits the whole 102x48
    // marbled plate as one sprite, not as a 9-slice.
    let panel_plate =
        |text: &mut Vec<TextDraw>, sprites: &mut Vec<SpriteDraw>, rect: (i32, i32, i32, i32)| {
            match battle {
                Some(b) => sprites.push(SpriteDraw {
                    dst: (
                        origin.0 + rect.0 * scale,
                        origin.1 + rect.1 * scale,
                        b.panel_bg.2 * scale as u32,
                        b.panel_bg.3 * scale as u32,
                    ),
                    src: b.panel_bg,
                    color: white,
                }),
                None => flat_skin(text, rect),
            }
        };
    // The retail readout-tint law, per slot (`FUN_800349EC` / `FUN_80035EA8`).
    //
    // Two things the port used to get backwards, both read off a retail frame
    // whose third party member is at 0 HP:
    //
    // * a downed member's **name** is drawn in the ordinary readout colour,
    //   not dimmed - retail's name glyphs take CLUT `(208, 510)` on all three
    //   panels, the live pair and the downed one alike;
    // * a downed member's HP **and** MP readouts both take the dead tier,
    //   which is retail's brightest red, not a grey-out. The MP field takes it
    //   even when its own ratio would say "normal", so the death override sits
    //   above `mp_bar_color_index`.
    let tints = |slot: &HudSlotView<'_>| -> ([f32; 4], [f32; 4], [f32; 4]) {
        const DEAD: u8 = 2;
        let base = READOUT_NORMAL;
        let hp = if !slot.alive {
            gauge_fill_color(DEAD)
        } else {
            tint(
                hp_bar_color_index(slot.hp, slot.hp_max, slot.status_sprite != 0),
                READOUT_NORMAL,
            )
        };
        let mp = if !slot.alive {
            gauge_fill_color(DEAD)
        } else {
            tint(mp_bar_color_index(slot.mp, slot.mp_max), READOUT_NORMAL)
        };
        (base, hp, mp)
    };

    // The member whose readout takes the full-width bar, if any. Retail's two
    // party surfaces are **mutually exclusive**: while the bar owns the
    // screen the whole roster cluster is parked off-screen, so a frame shows
    // one or the other and never both.
    let message_bar = frame.message_bar.filter(|m| !m.is_empty());
    let bar_member = frame
        .active_slot
        .filter(|_| message_bar.is_none())
        .and_then(|a| live_party.iter().find(|(i, _)| *i == a as usize).copied());

    // Rows a host-drawn box claims this frame, skin included, and the row
    // test each party surface runs against them ([`BattleHudFrame::host_box`]).
    let host_box_rows = frame
        .host_box
        .map(|(_, y, _, h)| (y - HOST_BOX_SKIN, y + h + HOST_BOX_SKIN));
    let box_covers = |top: i32, height: i32| {
        host_box_rows.is_some_and(|(box_top, box_bot)| box_top < top + height && box_bot > top)
    };
    let bar_covered = box_covers(BAR_Y, PLATE_H);
    // The box's drawn frame in surface pixels: what it hides of a surface
    // drawn under it.
    let host_box_frame = frame.host_box.map(|(x, y, w, h)| {
        (
            origin.0 + (x - HOST_BOX_SKIN) * scale,
            origin.1 + (y - HOST_BOX_SKIN) * scale,
            (w + 2 * HOST_BOX_SKIN) * scale,
            (h + 2 * HOST_BOX_SKIN) * scale,
        )
    });

    // ---- Surface 1: the resting roster panels ----
    //
    // One 102x48 panel per live member at the packet-pinned seats, carrying
    // name + LV on the top cell, then an HP row and an MP row. Retail parks
    // the whole cluster at `y = 230` (under its 228-line display window)
    // while the bar or a command-entry session owns the frame; the engine
    // stage is 240 lines, so `y = 230` would still be visible here and the
    // port omits the draws instead.
    //
    // A host box over the panels does not park them. Retail keeps the
    // cluster's text actors on the list and the box, a later node, paints
    // over them: `v0_1_battle_command_menu` holds the panel at `(114, 168)`
    // under the waiting explainer, its name row showing above the box's
    // frame. The hosts composite every glyph over every sprite, so the
    // panels are cut to what the box leaves visible instead of layered.
    let seats = panel_seats(live_party.len().min(3));
    let (panel_text_from, panel_sprites_from) = (text.len(), sprites.len());
    // The panels and the bar are one seat's two tenants, except while the
    // ring opens: step 1 sinks the panels as it raises the bar, and both are
    // on screen for the glide (`ap_plate_dx` is non-zero exactly then).
    let ring_opening = frame.ap_plate_dx != 0;
    if !frame.panels_parked && (bar_member.is_none() || ring_opening) {
        for (ordinal, (i, slot)) in live_party.iter().take(3).enumerate() {
            let px = seats[ordinal];
            // The open glide ([`BattleHudFrame::panels_dy`]) moves the whole
            // panel, content and all.
            let py = PANEL_Y + frame.panels_dy;
            let (base, hp_tint, mp_tint) = tints(slot);
            panel_plate(&mut text, &mut sprites, (px, py, PANEL_W, PANEL_H));

            // The name pen is retail's per-party-size anchor
            // (`FUN_801D84C0` via `battle_party_panel::panel_anchors`),
            // which lands `+5` inside the panel seat ([`PANEL_NAME`]).
            stage_text(
                &mut text,
                font,
                slot.name,
                party_panel_stage_x(live_party.len().min(3), ordinal),
                py + PANEL_NAME.1,
                base,
            );
            // `FUN_8002C2E4` draws exactly **one** element per slot, and its
            // ladder is exclusive: the no-ailment arm is the base marker plus
            // the level, and any set bit (or zero HP) replaces both with a
            // single ailment sprite. Retail seats that element on the panel's
            // top cell, so the level and the ailment share one seat here too.
            //
            // The badge is retail's own 48x16 word cell off the system-UI
            // sheet, seated at the ladder caller's `pen + (0x33, -4)`
            // ([`battle_hud_chrome::STATUS_BADGE_PANEL_SEAT`]). Without the
            // cell (an atlas built from a slice that could not reach the
            // row-511 extension palettes) it degrades to the engine's
            // labelled tag on the level seat.
            if slot.status_sprite != 0 {
                match frame
                    .badges
                    .and_then(|b| b.status_badge(slot.status_sprite))
                {
                    Some(src) => stage_sprite(
                        &mut sprites,
                        src,
                        px + STATUS_BADGE_PANEL_SEAT.0,
                        py + STATUS_BADGE_PANEL_SEAT.1,
                    ),
                    // The fallback tag stands in for that 48x16 cell, so it
                    // belongs on the **badge's** seat, centred in the cell -
                    // not on the `LV` label seat the level would have used.
                    // The two are 8 px apart on x and 6 on y, which is what a
                    // host with no badge atlas (the browser play page) shows
                    // as a downed member's tag sitting off its own plate.
                    None => {
                        let label = status_element_label(slot.status_sprite);
                        let w = font.layout_ascii(label).advance_x as i32;
                        stage_text(
                            &mut text,
                            font,
                            label,
                            px + STATUS_BADGE_PANEL_SEAT.0 + (STATUS_BADGE_SIZE.0 - w) / 2,
                            py + STATUS_BADGE_PANEL_SEAT.1 + STATUS_BADGE_TAG_DY,
                            status_element_color(slot.status_sprite),
                        );
                    }
                }
            } else if slot.level > 0 {
                label(
                    &mut sprites,
                    &mut text,
                    2,
                    px + PANEL_LV_LABEL.0,
                    py + PANEL_LV_LABEL.1,
                );
                numerals(
                    &mut sprites,
                    &mut text,
                    u32::from(slot.level),
                    px + PANEL_LV_DIGITS_RIGHT,
                    py + PANEL_LV_DIGIT_Y,
                    base,
                );
            }

            label(
                &mut sprites,
                &mut text,
                0,
                px + PANEL_HP_LABEL.0,
                py + PANEL_HP_LABEL.1,
            );
            numerals(
                &mut sprites,
                &mut text,
                u32::from(slot.hp),
                px + PANEL_CUR_RIGHT,
                py + PANEL_HP_DIGIT_Y,
                hp_tint,
            );
            separator(
                &mut sprites,
                &mut text,
                px + PANEL_HP_SEPARATOR.0,
                py + PANEL_HP_SEPARATOR.1,
                hp_tint,
            );
            numerals(
                &mut sprites,
                &mut text,
                u32::from(slot.hp_max),
                px + PANEL_MAX_RIGHT,
                py + PANEL_HP_DIGIT_Y,
                hp_tint,
            );

            if slot.mp_max > 0 {
                label(
                    &mut sprites,
                    &mut text,
                    1,
                    px + PANEL_MP_LABEL.0,
                    py + PANEL_MP_LABEL.1,
                );
                numerals(
                    &mut sprites,
                    &mut text,
                    u32::from(slot.mp),
                    px + PANEL_CUR_RIGHT,
                    py + PANEL_MP_DIGIT_Y,
                    mp_tint,
                );
                separator(
                    &mut sprites,
                    &mut text,
                    px + PANEL_MP_SEPARATOR.0,
                    py + PANEL_MP_SEPARATOR.1,
                    mp_tint,
                );
                numerals(
                    &mut sprites,
                    &mut text,
                    u32::from(slot.mp_max),
                    px + PANEL_MAX_RIGHT,
                    py + PANEL_MP_DIGIT_Y,
                    mp_tint,
                );
            }

            popup_anchor[*i] = (
                origin.0 + (px + PANEL_W / 2) * scale,
                origin.1 + (py - 16) * scale,
            );
        }
    }
    if let Some(hole) = host_box_frame {
        cut_draws_under(&mut text, panel_text_from, hole, scale);
        cut_draws_under(&mut sprites, panel_sprites_from, hole, scale);
    }

    // ---- Surface 2: the active-actor bar ----
    //
    // The full-width plate the acting party member's readout takes over: name
    // at the left, then the HP label with a right-aligned current and a
    // right-aligned maximum either side of the `/`, then the same pair for
    // MP. Retail draws **no gauge bar** here - the packet run carries no bar
    // primitive in either readout, and neither reference frame shows one.
    if let Some((i, slot)) = bar_member.filter(|_| !bar_covered) {
        let (base, hp_tint, mp_tint) = tints(slot);
        // The raise glide ([`BattleHudFrame::bar_dy`]) moves the whole plate.
        let dy = frame.bar_dy;
        plate_run(
            &mut text,
            &mut sprites,
            BAR_X,
            BAR_Y + dy,
            BAR_INTERIOR_W,
            false,
        );
        stage_text(
            &mut text,
            font,
            slot.name,
            BAR_NAME.0,
            BAR_NAME.1 + dy,
            base,
        );
        label(
            &mut sprites,
            &mut text,
            0,
            BAR_HP_LABEL.0,
            BAR_HP_LABEL.1 + dy,
        );
        numerals(
            &mut sprites,
            &mut text,
            u32::from(slot.hp),
            BAR_HP_CUR_RIGHT,
            BAR_DIGIT_Y + dy,
            hp_tint,
        );
        separator(
            &mut sprites,
            &mut text,
            BAR_HP_SEPARATOR.0,
            BAR_HP_SEPARATOR.1 + dy,
            hp_tint,
        );
        numerals(
            &mut sprites,
            &mut text,
            u32::from(slot.hp_max),
            BAR_HP_MAX_RIGHT,
            BAR_DIGIT_Y + dy,
            hp_tint,
        );
        if slot.mp_max > 0 {
            label(
                &mut sprites,
                &mut text,
                1,
                BAR_MP_LABEL.0,
                BAR_MP_LABEL.1 + dy,
            );
            numerals(
                &mut sprites,
                &mut text,
                u32::from(slot.mp),
                BAR_MP_CUR_RIGHT,
                BAR_DIGIT_Y + dy,
                mp_tint,
            );
            separator(
                &mut sprites,
                &mut text,
                BAR_MP_SEPARATOR.0,
                BAR_MP_SEPARATOR.1 + dy,
                mp_tint,
            );
            numerals(
                &mut sprites,
                &mut text,
                u32::from(slot.mp_max),
                BAR_MP_MAX_RIGHT,
                BAR_DIGIT_Y + dy,
                mp_tint,
            );
        }
        popup_anchor[i] = (
            origin.0 + BAR_HP_LABEL.0 * scale,
            origin.1 + (BAR_Y + dy - 26) * scale,
        );
    }

    // ---- The top-left seat: banner OR plaque, never both ----
    //
    // Retail's message banner and the actor-name plaque share content pen
    // `(16, 12)`; they are alternatives on one seat. Drawing both is two
    // text runs on the same pixels, so the banner wins when a message is
    // up and the host can also claim the seat outright
    // (`plaque_seat_taken`) for a box it draws itself.
    if let Some(message) = frame.banner.filter(|m| !m.is_empty()) {
        // The class-0 nine-slice over its marbled fill, on the record's fixed
        // 280-wide content box (`battle_hud_chrome::BANNER_BOX_W`).
        if let Some(rects) = frame.chrome {
            let content = message_banner_content(font, message);
            sprites.extend(message_banner_chrome_draws_for(
                rects,
                content,
                origin,
                scale as u32,
            ));
        }
        let mut rows = message_banner_text_draws_for(font, message);
        scale_stage_text_draws(&mut rows, origin, scale as u32);
        text.extend(rows);
    } else if let Some(name) = frame.plaque
        && !name.is_empty()
        && !frame.plaque_seat_taken
    {
        // Retail's top-left plaque names the actor the frame belongs to -
        // the party member on his turn, the monster through its attack - on
        // a carved gold plate whose interior is sized to the measured name.
        // Its live seat is `(8, 8)` and its parked seat `(8, -24)`: the
        // plaque slides in from above over the action seed's raise glide
        // (`frame.plaque_dy`).
        //
        // An actor that carries an element wears its badge in front of the
        // name, and the interior grows by the badge plus a 5-px gap - the
        // second half of `battle_chrome::name_plaque`'s pinned law.
        let name_w = font.layout_ascii(name).advance_x as i32;
        let badge = frame
            .plaque_badge
            .and_then(|i| frame.badges.and_then(|b| b.element_badge(i)));
        let lead = if badge.is_some() {
            PLAQUE_BADGE_W + PLAQUE_BADGE_GAP
        } else {
            0
        };
        // From the ring on, the round prompt's `Begin` chip has become a
        // breadcrumb tab on the plaque's own seat (record 1 glides
        // `(104, 88)` to `(16, 14)` and takes the gold class), and the
        // plaque drops in beside it (record 26 at `(68, 14)`) - two gold
        // plates, `Begin | Vahn`, the trail the item window continues.
        let plaque_x = if frame.begin_tab {
            // The tab walks in from the prompt's `Begin` seat as the ring
            // opens (`frame.begin_tab_offset`).
            let (tab_dx, tab_dy) = frame.begin_tab_offset;
            plate_run(
                &mut text,
                &mut sprites,
                BEGIN_TAB_X + tab_dx,
                BEGIN_TAB_Y + tab_dy,
                BEGIN_TAB_INTERIOR_W,
                true,
            );
            stage_text(
                &mut text,
                font,
                BEGIN_TAB_LABEL,
                BEGIN_TAB_X + tab_dx + PLATE_CAP_W,
                BEGIN_TAB_Y + tab_dy + PLAQUE_CONTENT_DY,
                ink,
            );
            PLAQUE_BEHIND_TAB_X
        } else {
            PLAQUE_X
        };
        // The action plaque glides in from its parked seat above the top
        // edge, and so does the ring's (record `0x1A`) as the ring opens;
        // every later command-entry step snaps it, and the host's
        // `plaque_dy` is `0` there.
        let plaque_y = PLAQUE_Y + frame.plaque_dy;
        plate_run(
            &mut text,
            &mut sprites,
            plaque_x,
            plaque_y,
            lead + name_w,
            true,
        );
        let content_x = plaque_x + PLATE_CAP_W;
        let content_y = plaque_y + PLAQUE_CONTENT_DY;
        if let Some(src) = badge {
            stage_sprite(&mut sprites, src, content_x, content_y);
        }
        stage_text(&mut text, font, name, content_x + lead, content_y, ink);
        // The chosen arm's chip rests at `width(name) + 0x54`: content
        // `0x54 - 0x44 = 16` px past the plaque's own content run, i.e. its
        // plate starts where the plaque's right cap ends.
        if frame.begin_tab
            && let Some(label) = frame.third_tab.filter(|l| !l.is_empty())
        {
            let tab_x = content_x + lead + name_w + PLATE_CAP_W;
            let label_w = font.layout_ascii(label).advance_x as i32;
            plate_run(
                &mut text,
                &mut sprites,
                tab_x,
                PLAQUE_Y,
                THIRD_TAB_INTERIOR_W.max(label_w),
                true,
            );
            stage_text(
                &mut text,
                font,
                label,
                tab_x + PLATE_CAP_W,
                PLAQUE_Y + PLAQUE_CONTENT_DY,
                ink,
            );
        }
    } else if frame.begin_tab && !frame.plaque_seat_taken {
        // The commit-confirm screen `0x6E`: the tab stays on its seat and
        // the plaque has gone back up behind the top edge, so the tab
        // stands alone (`party_basic_attack_vs_gobu_gobu`).
        let (tab_dx, tab_dy) = frame.begin_tab_offset;
        plate_run(
            &mut text,
            &mut sprites,
            BEGIN_TAB_X + tab_dx,
            BEGIN_TAB_Y + tab_dy,
            BEGIN_TAB_INTERIOR_W,
            true,
        );
        stage_text(
            &mut text,
            font,
            BEGIN_TAB_LABEL,
            BEGIN_TAB_X + tab_dx + PLATE_CAP_W,
            BEGIN_TAB_Y + tab_dy + PLAQUE_CONTENT_DY,
            ink,
        );
    }

    // ---- The move-name label ----
    //
    // Placement records 76 / 77: `y = 150`, `w = 0`, and both seats' x
    // written to `0xA0 - width / 2` before the open (`FUN_8004C650` for an
    // art, the `0x28` / `0x3C` arms for a spell / item), so the label is
    // centred and does not glide. Plain glyphs, no plate - the display
    // list carries the run and nothing around it.
    if let Some(name) = frame.move_name.filter(|n| !n.is_empty()) {
        let name_w = font.layout_ascii(name).advance_x as i32;
        let lead = name.as_bytes()[0];
        let x = i32::from(crate::battle_name_banner::banner_x(name_w, lead, 0));
        stage_text(&mut text, font, name, x, MOVE_NAME_Y, ink);
    }

    // ---- The target plaque ----
    //
    // Placement record 81: the monster a party member's action is aimed
    // at, on the blue plate class, its `x` written so the right cap ends
    // at 312, rising to the bar's row; the name payload carries the
    // element-badge escape in front of the name, so the interior follows
    // the same badge law as the top-left plaque.
    //
    // Retail queues the plaque ahead of the message bar, so a bar that has
    // risen over it hides it; one still gliding up from below leaves it in
    // view (`zeto_call_wave_mid_cast`: the plaque on its row, the bar's top
    // edge under it). The plaque's name rides the text layer, which draws
    // over every sprite, so the port drops the plaque once the bar's frame
    // has reached its top.
    let plaque_hidden_by_bar = message_bar.is_some()
        && BAR_NAME.1
            - crate::battle_hud_chrome::BANNER_PEN_INSET
            - crate::battle_hud_chrome::BANNER_BORDER
            + frame.message_bar_dy
            <= BAR_Y + frame.target_plaque_dy;
    if let Some((name, badge_index)) = frame
        .target_plaque
        .filter(|(n, _)| !n.is_empty() && !plaque_hidden_by_bar)
    {
        let name_w = font.layout_ascii(name).advance_x as i32;
        let badge = badge_index.and_then(|i| frame.badges.and_then(|b| b.element_badge(i)));
        let lead = if badge.is_some() {
            PLAQUE_BADGE_W + PLAQUE_BADGE_GAP
        } else {
            0
        };
        let interior = lead + name_w;
        let plate_x = TARGET_PLAQUE_RIGHT - (interior + 2 * PLATE_CAP_W);
        // Rising from below the bottom edge while its raise glide runs.
        let plate_y = BAR_Y + frame.target_plaque_dy;
        plate_run(&mut text, &mut sprites, plate_x, plate_y, interior, false);
        let content_x = plate_x + PLATE_CAP_W;
        let content_y = plate_y + PLAQUE_CONTENT_DY;
        if let Some(src) = badge {
            stage_sprite(&mut sprites, src, content_x, content_y);
        }
        stage_text(&mut text, font, name, content_x + lead, content_y, ink);
    }

    // ---- The target-select plaque ----
    //
    // Placement record `0x29`: `FUN_801D5854`'s target-cursor arm measures
    // the target's name payload and writes the content box's rest seat
    // (`0x801D5B4C..0x801D5BAC`); the plate is the blue class (kind pair
    // `0x0101`), pen `(x, y - 2)`, plate `(x - 8, y - 6)` like every
    // content-box record. The slide in from `x + 0x80` is not modelled - the
    // plaque draws at rest.
    if let Some((name, badge_index)) = frame.target_select.filter(|(n, _)| !n.is_empty()) {
        use crate::battle_chrome as chrome;
        let name_w = font.layout_ascii(name).advance_x as i32;
        let badge = badge_index.and_then(|i| frame.badges.and_then(|b| b.element_badge(i)));
        let lead = if badge.is_some() {
            PLAQUE_BADGE_W + PLAQUE_BADGE_GAP
        } else {
            0
        };
        let interior = lead + name_w;
        let x = i32::from(chrome::target_select_plaque_x(interior as u16));
        let y = i32::from(chrome::TARGET_SELECT_Y);
        plate_run(
            &mut text,
            &mut sprites,
            x - PLATE_CAP_W,
            y - 6,
            interior,
            false,
        );
        if let Some(src) = badge {
            stage_sprite(&mut sprites, src, x, y - 2);
        }
        stage_text(&mut text, font, name, x + lead, y - 2, ink);
    }

    // ---- The commit log ----
    //
    // Placement records `0x2B + 3n`: each committed member's name, command
    // and target, landed by the commit arms of `FUN_801D388C` and laid out
    // by the shared kernel. Every element is a content box on the gold
    // plate class (kind `2`): pen `(x, y - 2)`, plate `(x - 8, y - 6)`.
    // An element whose content is empty (Spirit's target) draws nothing.
    if !frame.commit_log.is_empty() {
        use legaia_engine_battle_vm::battle_commit_log as log;
        let width = |t: &str| font.layout_ascii(t).advance_x as u16;
        let commits: Vec<log::LogCommit> =
            frame.commit_log.iter().map(|r| r.commit(width)).collect();
        for (row, elements) in frame
            .commit_log
            .iter()
            .zip(log::commit_log_layout(&commits))
        {
            let content = [row.name.as_str(), row.command.as_str(), row.target_text()];
            for (el, content) in elements.iter().zip(content) {
                if content.is_empty() {
                    continue;
                }
                let (x, y) = (i32::from(el.x) + i32::from(row.slide_x), i32::from(el.y));
                plate_run(
                    &mut text,
                    &mut sprites,
                    x - PLATE_CAP_W,
                    y - 6,
                    i32::from(el.width),
                    true,
                );
                stage_text(&mut text, font, content, x, y - 2, READOUT_NORMAL);
            }
        }
    }

    // ---- The battle-intro enemy-name banner ----
    //
    // One label per monster group, laid out by retail's composer and each
    // spawned on the message banner's class-0 frame (kind 3) at pen
    // `(x, 48)` over the same marbled fill. Frame origin is the pen less
    // `(8, 8)`, so `Moldy Worm` on `(86, 48)` frames `(78, 40)..(159, 67)`.
    for (label, x) in frame.intro_names.iter().filter(|(l, _)| !l.is_empty()) {
        let pen = (*x, BATTLE_INTRO_NAME_Y);
        let w = font.layout_ascii(label).advance_x as i32;
        if let Some(rects) = frame.chrome {
            sprites.extend(crate::battle_hud_chrome::class0_frame_draws_at(
                rects,
                pen,
                (w, crate::battle_hud_chrome::BANNER_INTERIOR_H),
                origin,
                scale as u32,
            ));
        }
        stage_text(&mut text, font, label, pen.0, pen.1, crate::MENU_TEXT_WHITE);
    }

    // ---- The battle-stage module's boss-name banner ----
    //
    // `FUN_8003541C(0, 0, name, x, y, 0, 0, 0)`: a text actor with no frame
    // rect, centred on the screen by its caller.
    if let Some((label, x, y)) = frame.stage_banner.filter(|(l, _, _)| !l.is_empty()) {
        stage_text(&mut text, font, label, x, y, crate::MENU_TEXT_WHITE);
    }

    // ---- The message bar ----
    //
    // Placement record 91: the active-actor bar's seat and content box
    // (`(16, 194)`, 288 wide), but a **framed window**, not a plate run. The
    // record's node kind `+0x10` is `13` where the bar's (record 7) is `0`,
    // and its frame style `+0x0E` is `0x44` - widget record `0x44`, the
    // class-0 frame the top message banner and the tutorial prompts wear
    // (`FUN_8002C69C` indexes the widget table by that byte, `0x8002C798`).
    // `player_steal_skeleton_banner` shows it: the blue marble fill inside
    // the gold border over `(8, 184)..(312, 212)`, the pen's 8-px inflation.
    // A host without the system-UI atlas keeps the plate run.
    //
    // `FUN_801D8DE8(0x5B, 0)` spawns it at seat A and glides it up to seat B
    // (`frame.message_bar_dy`).
    if let Some(message) = message_bar.filter(|_| !bar_covered) {
        let dy = frame.message_bar_dy;
        let pen = (BAR_NAME.0, BAR_NAME.1 + dy);
        match frame.chrome {
            Some(rects) => sprites.extend(crate::battle_hud_chrome::class0_frame_draws_at(
                rects,
                pen,
                (BAR_INTERIOR_W, crate::battle_hud_chrome::BANNER_INTERIOR_H),
                origin,
                scale as u32,
            )),
            None => plate_run(
                &mut text,
                &mut sprites,
                BAR_X,
                BAR_Y + dy,
                BAR_INTERIOR_W,
                false,
            ),
        }
        // The bar's text can carry a `0xCE` button escape (PROT 0954's stop
        // prompt), parked at `U+E000 + byte`, and the steal captions carry
        // their accented glyphs as Latin-1 characters: hand the font the
        // bytes back so each draws its own glyph.
        let bytes: Vec<u8> = message
            .chars()
            .map(|c| match c as u32 {
                u @ 0..=0xFF => u as u8,
                u @ 0xE000..=0xE0FF => (u - 0xE000) as u8,
                _ => b'?',
            })
            .collect();
        let mut draws = text_draws_for(&font.layout(&bytes), pen, ink);
        scale_stage_text_draws(&mut draws, origin, scale as u32);
        text.extend(draws);
    }

    // ---- The ring's AP plate ----
    //
    // Placement record 82 slides in to `(208, 174)` with the ring and
    // reads the acting member's Spirit gauge; it is the status screen's
    // own AP-gauge widget, so it samples the same atlas pieces the arts
    // input's copy does.
    if let (Some(value), Some(rects)) = (frame.ap_plate_value, frame.chrome) {
        sprites.extend(crate::arts_input::ap_plate_draws(
            &crate::arts_input::ApPlateRects {
                cap: rects.gauge_cap,
                trough: rects.gauge_trough,
                fill: rects.gauge_fill,
                box_: rects.gauge_box,
                digits: rects.gauge_digits,
                tip: rects.gauge_tip,
                full_mark: rects.gauge_100,
            },
            value,
            // The slide in with the ring ([`BattleHudFrame::ap_plate_dx`])
            // moves the whole plate.
            (origin.0 + frame.ap_plate_dx * scale, origin.1),
            scale as u32,
        ));
    }

    // ---- Diagnostic rows (LEGAIA_DIAG_HUD) ----
    //
    // The engine's debug readout: compact per-slot rows at `pen` in raw
    // surface pixels - monster HP numerals + thin gauge bars (retail draws
    // no monster gauge at all), K.O. tags, the selected status element, and
    // a per-party LV / AP tail (the base-marker + level element retail only
    // shows on the input panel).
    if frame.diag {
        for (i, slot) in frame.slots.iter().enumerate() {
            if slot.name.is_empty() {
                continue;
            }
            let row_y = pen.1 + i as i32 * LINE_H;
            let base = if !slot.alive {
                dim
            } else if slot.is_party {
                white
            } else {
                monster
            };

            let name_layout = font.layout_ascii(slot.name);
            text.extend(text_draws_for(&name_layout, (pen.0, row_y), base));

            let hp_color = if !slot.alive {
                dim
            } else {
                tint(
                    hp_bar_color_index(slot.hp, slot.hp_max, slot.status_sprite != 0),
                    base,
                )
            };
            let hp_layout = font.layout_ascii(&format!("{}/{}", slot.hp, slot.hp_max));
            text.extend(text_draws_for(&hp_layout, (pen.0 + HP_X, row_y), hp_color));

            // Thin HP bar under the numerals: fill colour = the retail gauge
            // index (`FUN_80046A20`) through `gauge_fill_color`; the dead arm
            // (index 2) greys the whole track, not just the fill.
            if let Some(src) = frame.solid_src {
                let frac = if slot.hp_max == 0 {
                    0.0
                } else {
                    (slot.hp as f32 / slot.hp_max as f32).clamp(0.0, 1.0)
                };
                let track = if slot.hp_fill == 2 {
                    gauge_fill_color(2)
                } else {
                    bar_back
                };
                text.push(TextDraw {
                    dst: (pen.0 + HP_X, row_y + 12, MBAR_W as u32, MBAR_H as u32),
                    src,
                    color: track,
                });
                let w = (MBAR_W as f32 * frac).round() as i32;
                let w = if slot.hp > 0 { w.max(1) } else { w };
                if w > 0 {
                    text.push(TextDraw {
                        dst: (pen.0 + HP_X, row_y + 12, w as u32, MBAR_H as u32),
                        src,
                        color: gauge_fill_color(slot.hp_fill),
                    });
                }
                // MP twin for slots that carry a ceiling.
                if slot.mp_max > 0 {
                    let mfrac = (slot.mp as f32 / slot.mp_max as f32).clamp(0.0, 1.0);
                    let mtrack = if slot.mp_fill == 2 {
                        gauge_fill_color(2)
                    } else {
                        bar_back
                    };
                    text.push(TextDraw {
                        dst: (
                            pen.0 + HP_X + MBAR_W + 4,
                            row_y + 12,
                            (MBAR_W / 2) as u32,
                            MBAR_H as u32,
                        ),
                        src,
                        color: mtrack,
                    });
                    let mw = ((MBAR_W / 2) as f32 * mfrac).round() as i32;
                    if mw > 0 {
                        text.push(TextDraw {
                            dst: (
                                pen.0 + HP_X + MBAR_W + 4,
                                row_y + 12,
                                mw as u32,
                                MBAR_H as u32,
                            ),
                            src,
                            color: gauge_fill_color(slot.mp_fill),
                        });
                    }
                }
            }

            if !slot.alive {
                let ko_layout = font.layout_ascii("K.O.");
                text.extend(text_draws_for(&ko_layout, (pen.0 + KO_X, row_y), red));
            }

            // The single retail-selected status element, as a letter tag.
            if slot.status_sprite != 0 {
                let layout = font.layout_ascii(status_element_label(slot.status_sprite));
                text.extend(text_draws_for(
                    &layout,
                    (pen.0 + STATUS_X, row_y),
                    status_element_color(slot.status_sprite),
                ));
            }

            // Party tail: the base-marker level (`FUN_8002C2E4`'s no-ailment
            // arm) + the AP gauge state.
            if slot.is_party {
                let mut tail = String::new();
                if slot.level > 0 {
                    tail.push_str(&format!("LV{}", slot.level));
                }
                if slot.ap_max > 0 {
                    if !tail.is_empty() {
                        tail.push(' ');
                    }
                    tail.push_str(&format!("AP{}/{}", slot.ap_filled, slot.ap_max));
                }
                if !tail.is_empty() {
                    let layout = font.layout_ascii(&tail);
                    text.extend(text_draws_for(&layout, (pen.0 + DIAG_TAIL_X, row_y), base));
                }
            }
        }
    }

    let log_x = pen.0;
    let log_y = pen.1 + frame.slots.len() as i32 * LINE_H + 4;
    for (i, line) in frame.log.iter().enumerate() {
        let layout = font.layout_ascii(line.text);
        text.extend(text_draws_for(
            &layout,
            (log_x, log_y + i as i32 * LINE_H),
            line.color,
        ));
    }

    // ---- Popups: the diagnostic seat only ----
    //
    // Retail's landed-hit numeral is not a HUD widget. It is a run of 24x24
    // cells off the battle effect atlas, thrown over the **struck actor** and
    // rising to a fixed screen row - laid out by
    // `engine-battle-vm::battle_value_readout::value_cells` and drawn by the host,
    // which is the only layer that knows where the actor projects. Anchoring a
    // number to a party panel (or, for a monster, to the diagnostic pen in the
    // top-left corner) put every damage figure somewhere retail never draws
    // one, so that pass is now the diagnostic readout's and nothing else's.
    for popup in frame.popups.iter().filter(|_| frame.diag) {
        if (popup.slot as usize) >= frame.slots.len() {
            continue;
        }
        let (ax, ay) = popup_anchor[popup.slot as usize];
        let popup_color = match (popup.is_heal, popup.is_crit) {
            (true, _) => apply_alpha(green, popup.alpha),
            (_, true) => apply_alpha(yellow, popup.alpha),
            _ => apply_alpha(cyan, popup.alpha),
        };
        let s = if let Some(letter) = popup.status_letter {
            format!("[{}]", letter as char)
        } else if popup.is_heal {
            format!("+{}", popup.amount)
        } else {
            format!("-{}", popup.amount)
        };
        let layout = font.layout_ascii(&s);
        text.extend(text_draws_for(&layout, (ax, ay), popup_color));
    }

    BattleHudDraws { text, sprites }
}

/// Cut the draws from index `from` on down to what the opaque rect `hole`
/// (surface pixels) leaves visible: a draw inside it goes, one crossing an
/// edge is split into the pieces outside. `scale` is surface pixels per
/// stage pixel - a draw's `src` is in stage texels - so a cut stays on a
/// texel boundary.
fn cut_draws_under(draws: &mut Vec<TextDraw>, from: usize, hole: (i32, i32, i32, i32), scale: i32) {
    let (hx0, hy0) = (hole.0, hole.1);
    let (hx1, hy1) = (hole.0 + hole.2, hole.1 + hole.3);
    let tail = draws.split_off(from.min(draws.len()));
    for d in tail {
        let (x0, y0) = (d.dst.0, d.dst.1);
        let (x1, y1) = (x0 + d.dst.2 as i32, y0 + d.dst.3 as i32);
        if x1 <= hx0 || x0 >= hx1 || y1 <= hy0 || y0 >= hy1 {
            draws.push(d);
            continue;
        }
        // The piece of the draw covering surface rect `(px0, py0)..(px1, py1)`.
        // A stretched draw (a solid-texel rect) keeps its source whole.
        let texel_mapped = d.dst.2 == d.src.2 * scale as u32 && d.dst.3 == d.src.3 * scale as u32;
        let piece = |px0: i32, py0: i32, px1: i32, py1: i32| {
            (px1 > px0 && py1 > py0).then(|| TextDraw {
                dst: (px0, py0, (px1 - px0) as u32, (py1 - py0) as u32),
                src: if texel_mapped {
                    (
                        d.src.0 + ((px0 - x0) / scale) as u32,
                        d.src.1 + ((py0 - y0) / scale) as u32,
                        ((px1 - px0) / scale) as u32,
                        ((py1 - py0) / scale) as u32,
                    )
                } else {
                    d.src
                },
                color: d.color,
            })
        };
        // Above and below span the draw's width; left and right fill the
        // rows between them.
        let (my0, my1) = (y0.max(hy0), y1.min(hy1));
        draws.extend(
            [
                piece(x0, y0, x1, hy0.min(y1)),
                piece(x0, hy1.max(y0), x1, y1),
                piece(x0, my0, hx0.min(x1), my1),
                piece(hx1.max(x0), my0, x1, my1),
            ]
            .into_iter()
            .flatten(),
        );
    }
}
