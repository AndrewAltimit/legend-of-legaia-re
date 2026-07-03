//! In-game menu UI draw builders: encounter banner, field/status/spell
//! panels, game-over, options + key-rebind, name entry, cutscene
//! narration, item use, equipment, and the tactical-arts editor.

use crate::*;

/// Build [`TextDraw`]s for the encounter transition banner.
///
/// Drawn during [`crate::EncounterPhase::Transition`] (where the engine
/// type is `legaia_engine_core::encounter::EncounterPhase`). Renders a
/// large centered "ENCOUNTER!" line plus the formation label below.
/// Engines fade the surface independently - this just produces the
/// glyph draws.
pub fn encounter_banner_draws_for(
    font: &legaia_font::Font,
    formation_label: &str,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 16;
    let yellow: [f32; 4] = [1.0, 0.9, 0.3, 1.0];
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

    let mut out = Vec::new();
    let head = font.layout_ascii("ENCOUNTER!");
    out.extend(text_draws_for(&head, pen, yellow));
    if !formation_label.is_empty() {
        let body = font.layout_ascii(formation_label);
        out.extend(text_draws_for(&body, (pen.0, pen.1 + LINE_H), white));
    }
    out
}

/// Plain-data row for the field-menu draw. Engines build these from
/// `engine_core::field_menu::FieldMenuView::rows` so this crate doesn't
/// depend on engine-core.
pub struct FieldMenuRowView<'a> {
    pub label: &'a str,
    pub enabled: bool,
}

/// Build [`TextDraw`]s for the field (pause) menu panel. `cursor` is the
/// row index; greyed-out rows render dim. The corner badges show money
/// and the H:MM:SS play-time.
pub fn field_menu_draws_for(
    font: &legaia_font::Font,
    rows: &[FieldMenuRowView<'_>],
    cursor: u8,
    money: u32,
    play_time_seconds: u32,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 16;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let dim: [f32; 4] = [0.45, 0.45, 0.45, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];

    let mut out = Vec::new();
    let title = font.layout_ascii("MENU");
    out.extend(text_draws_for(&title, pen, white));

    for (i, row) in rows.iter().enumerate() {
        let y = pen.1 + LINE_H + i as i32 * LINE_H;
        let selected = i as u8 == cursor;
        let color = if !row.enabled {
            dim
        } else if selected {
            gold
        } else {
            white
        };
        if selected && row.enabled {
            let cur = font.layout_ascii(">");
            out.extend(text_draws_for(&cur, (pen.0, y), color));
        }
        let l = font.layout_ascii(row.label);
        out.extend(text_draws_for(&l, (pen.0 + 14, y), color));
    }

    let foot_y = pen.1 + LINE_H + rows.len() as i32 * LINE_H + LINE_H;
    let g = format!("{}G", money);
    let g_l = font.layout_ascii(&g);
    out.extend(text_draws_for(&g_l, (pen.0, foot_y), white));
    let h = play_time_seconds / 3600;
    let m = (play_time_seconds % 3600) / 60;
    let s = play_time_seconds % 60;
    let t = format!("{h:02}:{m:02}:{s:02}");
    let t_l = font.layout_ascii(&t);
    out.extend(text_draws_for(&t_l, (pen.0 + 110, foot_y), white));

    out
}

/// One stat row for the status screen.
pub struct StatusStatRow<'a> {
    pub label: &'a str,
    pub value: u32,
}

/// Plain-data view of a single character's status panel.
pub struct StatusPanelView<'a> {
    pub name: &'a str,
    pub level: u8,
    pub xp: u32,
    pub xp_to_next: u32,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    pub ap: u8,
    pub ap_max: u8,
    pub stat_rows: &'a [StatusStatRow<'a>],
    pub equip_rows: &'a [(&'a str, &'a str)],
}

/// Build [`TextDraw`]s for the status panel of one character. `nav_hint`
/// is rendered in the bottom-right corner ("L1/R1: Switch  Circle: Back")
/// and is `None` when the engine renders the hint elsewhere.
pub fn status_screen_draws_for(
    font: &legaia_font::Font,
    panel: &StatusPanelView<'_>,
    nav_hint: Option<&str>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 14;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.6, 0.6, 0.6, 1.0];

    let mut out = Vec::new();
    let head = format!("{}  Lv.{}", panel.name, panel.level);
    out.extend(text_draws_for(&font.layout_ascii(&head), pen, gold));

    let xp_line = format!("XP {} / {}", panel.xp, panel.xp_to_next);
    out.extend(text_draws_for(
        &font.layout_ascii(&xp_line),
        (pen.0, pen.1 + LINE_H),
        white,
    ));

    let hpmp = format!(
        "HP {:>4} / {:<4}   MP {:>3} / {:<3}   AP {:>2} / {:<2}",
        panel.hp, panel.hp_max, panel.mp, panel.mp_max, panel.ap, panel.ap_max
    );
    out.extend(text_draws_for(
        &font.layout_ascii(&hpmp),
        (pen.0, pen.1 + LINE_H * 2),
        white,
    ));

    for (i, sr) in panel.stat_rows.iter().enumerate() {
        let y = pen.1 + LINE_H * 4 + i as i32 * LINE_H;
        let line = format!("{:<8} {:>3}", sr.label, sr.value);
        out.extend(text_draws_for(&font.layout_ascii(&line), (pen.0, y), white));
    }
    let after_stats_y = pen.1 + LINE_H * 4 + panel.stat_rows.len() as i32 * LINE_H + LINE_H;
    out.extend(text_draws_for(
        &font.layout_ascii("Equipment"),
        (pen.0, after_stats_y),
        gold,
    ));
    for (i, (slot, item)) in panel.equip_rows.iter().enumerate() {
        let y = after_stats_y + LINE_H + i as i32 * LINE_H;
        let line = format!("{:<10} {}", slot, item);
        out.extend(text_draws_for(&font.layout_ascii(&line), (pen.0, y), white));
    }

    if let Some(hint) = nav_hint {
        let after_equip_y =
            after_stats_y + LINE_H + panel.equip_rows.len() as i32 * LINE_H + LINE_H;
        out.extend(text_draws_for(
            &font.layout_ascii(hint),
            (pen.0, after_equip_y),
            dim,
        ));
    }
    out
}

/// One row in the spell-menu list.
pub struct SpellRowView<'a> {
    pub name: &'a str,
    pub mp_cost: u8,
    pub admissible: bool,
}

/// One row in the spell-menu target picker.
pub struct SpellTargetView<'a> {
    pub name: &'a str,
    pub hp: u16,
    pub hp_max: u16,
    pub alive: bool,
}

/// Inputs for [`spell_menu_draws_for`]. Bundled so the function takes a
/// single payload struct instead of 12 positional arguments.
pub struct SpellMenuDrawArgs<'a> {
    pub party_names: &'a [&'a str],
    pub party_hp: &'a [(u16, u16)],
    pub party_mp: &'a [(u16, u16)],
    pub selected_caster: Option<u8>,
    pub spells: &'a [SpellRowView<'a>],
    pub selected_spell: Option<u8>,
    pub targets: &'a [SpellTargetView<'a>],
    pub selected_target: Option<u8>,
    /// Cursor row inside the active phase column.
    pub cursor: u8,
    /// `0` = caster column, `1` = spell column, `2` = target column.
    pub phase: u8,
}

/// Build [`TextDraw`]s for the field spell menu.
pub fn spell_menu_draws_for(
    font: &legaia_font::Font,
    args: SpellMenuDrawArgs<'_>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    let SpellMenuDrawArgs {
        party_names,
        party_hp,
        party_mp,
        selected_caster,
        spells,
        selected_spell,
        targets,
        selected_target,
        cursor,
        phase,
    } = args;
    const LINE_H: i32 = 14;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
    let red: [f32; 4] = [1.0, 0.55, 0.55, 1.0];

    let mut out = Vec::new();

    out.extend(text_draws_for(&font.layout_ascii("SPELLS"), pen, gold));

    // Caster column.
    for (i, name) in party_names.iter().enumerate() {
        let y = pen.1 + LINE_H + i as i32 * LINE_H;
        let selected_here = phase == 0 && i as u8 == cursor;
        let confirmed = selected_caster == Some(i as u8);
        let (cur_hp, _) = party_hp.get(i).copied().unwrap_or((0, 0));
        let (cur_mp, max_mp) = party_mp.get(i).copied().unwrap_or((0, 0));
        let alive = cur_hp > 0;
        let _ = confirmed;
        let color = if !alive {
            dim
        } else if selected_here {
            gold
        } else {
            white
        };
        if selected_here {
            out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
        }
        let line = format!("{:<8} MP {:>3}/{:<3}", name, cur_mp, max_mp);
        out.extend(text_draws_for(
            &font.layout_ascii(&line),
            (pen.0 + 14, y),
            color,
        ));
    }

    // Spell column (when entered).
    if let Some(_caster) = selected_caster {
        let col_x = pen.0 + 200;
        for (i, sp) in spells.iter().enumerate() {
            let y = pen.1 + LINE_H + i as i32 * LINE_H;
            let selected_here = phase == 1 && i as u8 == cursor;
            let _ = selected_spell;
            let color = if !sp.admissible {
                dim
            } else if selected_here {
                gold
            } else {
                white
            };
            if selected_here {
                out.extend(text_draws_for(&font.layout_ascii(">"), (col_x, y), color));
            }
            let line = format!("{:<14} {:>3}MP", sp.name, sp.mp_cost);
            out.extend(text_draws_for(
                &font.layout_ascii(&line),
                (col_x + 14, y),
                color,
            ));
        }
    }

    // Target column (when entered).
    if let Some(_spell) = selected_spell {
        let col_x = pen.0 + 380;
        for (i, t) in targets.iter().enumerate() {
            let y = pen.1 + LINE_H + i as i32 * LINE_H;
            let selected_here = phase == 2 && i as u8 == cursor;
            let _ = selected_target;
            let color = if !t.alive {
                red
            } else if selected_here {
                gold
            } else {
                white
            };
            if selected_here {
                out.extend(text_draws_for(&font.layout_ascii(">"), (col_x, y), color));
            }
            let line = format!("{:<8} {:>4}/{:<4}", t.name, t.hp, t.hp_max);
            out.extend(text_draws_for(
                &font.layout_ascii(&line),
                (col_x + 14, y),
                color,
            ));
        }
    }
    out
}

/// Build [`TextDraw`]s for the game-over panel.
pub fn game_over_draws_for(
    font: &legaia_font::Font,
    cursor: u8,
    continue_enabled: bool,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 16;
    let red: [f32; 4] = [1.0, 0.4, 0.4, 1.0];
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let dim: [f32; 4] = [0.4, 0.4, 0.4, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];

    let mut out = Vec::new();
    out.extend(text_draws_for(&font.layout_ascii("GAME OVER"), pen, red));

    let rows = ["Continue", "Retry", "Quit"];
    for (i, row) in rows.iter().enumerate() {
        let y = pen.1 + LINE_H * 2 + i as i32 * LINE_H;
        let row_disabled = i == 0 && !continue_enabled;
        let color = if row_disabled {
            dim
        } else if i as u8 == cursor {
            gold
        } else {
            white
        };
        if i as u8 == cursor && !row_disabled {
            out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
        }
        out.extend(text_draws_for(
            &font.layout_ascii(row),
            (pen.0 + 14, y),
            color,
        ));
    }
    out
}

/// One row in the options panel.
pub struct OptionsRowView<'a> {
    pub label: &'a str,
    pub value: &'a str,
}

/// Build [`TextDraw`]s for the options screen.
pub fn options_draws_for(
    font: &legaia_font::Font,
    rows: &[OptionsRowView<'_>],
    cursor: u8,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 16;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.6, 0.6, 0.6, 1.0];

    let mut out = Vec::new();
    out.extend(text_draws_for(&font.layout_ascii("CONFIG"), pen, gold));

    for (i, row) in rows.iter().enumerate() {
        let y = pen.1 + LINE_H * 2 + i as i32 * LINE_H;
        let color = if i as u8 == cursor { gold } else { white };
        if i as u8 == cursor {
            out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
        }
        out.extend(text_draws_for(
            &font.layout_ascii(row.label),
            (pen.0 + 14, y),
            color,
        ));
        out.extend(text_draws_for(
            &font.layout_ascii(row.value),
            (pen.0 + 180, y),
            color,
        ));
    }
    out.extend(text_draws_for(
        &font.layout_ascii("Cross/Start: Save  Circle: Cancel"),
        (
            pen.0,
            pen.1 + LINE_H * 2 + rows.len() as i32 * LINE_H + LINE_H,
        ),
        dim,
    ));
    out
}

/// Build [`TextDraw`]s for the key-rebind panel. Each row shows a button
/// label paired with the currently-bound key string.
pub fn key_rebind_draws_for(
    font: &legaia_font::Font,
    rows: &[(&str, &str)],
    cursor: u8,
    awaiting: bool,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 14;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.6, 0.6, 0.6, 1.0];

    let mut out = Vec::new();
    out.extend(text_draws_for(&font.layout_ascii("KEY REBIND"), pen, gold));

    for (i, (button, key)) in rows.iter().enumerate() {
        let y = pen.1 + LINE_H * 2 + i as i32 * LINE_H;
        let selected = i as u8 == cursor;
        let color = if selected { gold } else { white };
        if selected {
            out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
        }
        out.extend(text_draws_for(
            &font.layout_ascii(button),
            (pen.0 + 14, y),
            color,
        ));
        let value = if selected && awaiting { "..." } else { *key };
        out.extend(text_draws_for(
            &font.layout_ascii(value),
            (pen.0 + 100, y),
            color,
        ));
    }
    out.extend(text_draws_for(
        &font.layout_ascii("Cross: Bind  Circle: Cancel  Start: Save"),
        (
            pen.0,
            pen.1 + LINE_H * 2 + rows.len() as i32 * LINE_H + LINE_H,
        ),
        dim,
    ));
    out
}

/// Renderer-agnostic view of the name-entry overlay (engine-core's
/// `name_entry::NameEntry` projected to primitives, so this crate stays
/// decoupled from engine-core). Built per frame by the shell.
pub struct NameEntryView<'a> {
    /// The six selectable glyph rows (`|` marks a non-selectable separator).
    pub grid_rows: &'a [&'a str],
    /// Bottom control-bar button labels, left to right (e.g. Back / Space / End).
    pub control_labels: &'a [&'a str],
    /// Working name buffer.
    pub name: &'a str,
    /// Highlighted glyph cell `(row, col)` when the cursor is in the grid.
    pub grid_cursor: Option<(usize, usize)>,
    /// Highlighted control button index when the cursor is on the bar.
    pub control_cursor: Option<usize>,
    /// `true` while the "Is this name okay?" Yes/No prompt is showing.
    pub confirming: bool,
    /// Yes/No selection during the confirm prompt (`true` = Yes).
    pub confirm_yes: bool,
    /// Caret blink state (the trailing `_` after the name).
    pub caret_on: bool,
}

/// Build [`TextDraw`]s for the name-entry overlay - the screen the opening
/// `town01` field script opens so the player names the lead character.
///
/// Clean-room layout (the retail renderer is `FUN_801E6B34`): a heading +
/// working name with a blinking caret, the 6x17 character grid (separators
/// skipped), a control bar (Back / Space / End), and a Yes/No box while
/// confirming. Grid metrics: 14 px per column, 16 px per row.
///
/// REF: FUN_801E6B34
pub fn name_entry_draws_for(
    font: &legaia_font::Font,
    view: &NameEntryView<'_>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const COL_W: i32 = 14;
    const ROW_H: i32 = 16;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.6, 0.6, 0.6, 1.0];

    let mut out = Vec::new();
    out.extend(text_draws_for(
        &font.layout_ascii("Select your name."),
        pen,
        gold,
    ));

    // Working name + blinking caret.
    let name_y = pen.1 + ROW_H;
    out.extend(text_draws_for(
        &font.layout_ascii(view.name),
        (pen.0, name_y),
        white,
    ));
    if view.caret_on {
        let name_w = font.layout_ascii(view.name).advance_x as i32;
        out.extend(text_draws_for(
            &font.layout_ascii("_"),
            (pen.0 + name_w, name_y),
            white,
        ));
    }

    // Character grid.
    let grid_y0 = pen.1 + ROW_H * 3;
    for (r, row) in view.grid_rows.iter().enumerate() {
        for (c, ch) in row.bytes().enumerate() {
            if ch == b'|' || ch == b' ' {
                continue;
            }
            let selected = view.grid_cursor == Some((r, c));
            let color = if selected { gold } else { white };
            let x = pen.0 + c as i32 * COL_W;
            let y = grid_y0 + r as i32 * ROW_H;
            if selected {
                // Cursor bracket to the left of the highlighted glyph.
                out.extend(text_draws_for(
                    &font.layout_ascii(">"),
                    (x - COL_W / 2, y),
                    gold,
                ));
            }
            out.extend(text_draws_for(
                &font.layout_ascii(&(ch as char).to_string()),
                (x, y),
                color,
            ));
        }
    }

    // Control bar (Back / Space / End).
    let bar_y = grid_y0 + view.grid_rows.len() as i32 * ROW_H + ROW_H / 2;
    let mut bx = pen.0;
    for (i, label) in view.control_labels.iter().enumerate() {
        let selected = view.control_cursor == Some(i);
        let color = if selected { gold } else { white };
        if selected {
            out.extend(text_draws_for(
                &font.layout_ascii(">"),
                (bx - COL_W / 2, bar_y),
                gold,
            ));
        }
        out.extend(text_draws_for(
            &font.layout_ascii(label),
            (bx, bar_y),
            color,
        ));
        bx += font.layout_ascii(label).advance_x as i32 + COL_W;
    }

    // Confirm Yes/No box.
    if view.confirming {
        let cy = bar_y + ROW_H * 2;
        out.extend(text_draws_for(
            &font.layout_ascii("Is this name okay?"),
            (pen.0, cy),
            gold,
        ));
        let yes_color = if view.confirm_yes { gold } else { white };
        let no_color = if view.confirm_yes { white } else { gold };
        out.extend(text_draws_for(
            &font.layout_ascii("Yes"),
            (pen.0, cy + ROW_H),
            yes_color,
        ));
        out.extend(text_draws_for(
            &font.layout_ascii("No"),
            (pen.0 + 64, cy + ROW_H),
            no_color,
        ));
    } else {
        out.extend(text_draws_for(
            &font.layout_ascii("Cross: Select  Triangle: Back"),
            (
                pen.0,
                grid_y0 + view.grid_rows.len() as i32 * ROW_H + ROW_H * 2,
            ),
            dim,
        ));
    }

    out
}

/// Build [`TextDraw`]s for one opening-cutscene narration page (a single
/// subtitle line), horizontally centered at `center_x` with its baseline at
/// `top_y`, both in surface pixels. The host calls this with the active
/// page text from [`legaia_engine_core::cutscene_narration::CutsceneNarration`]
/// each frame; an empty / completed narration draws nothing.
///
/// Centering is computed from the font metrics (`center_x - width / 2`), the
/// same scheme [`now_checking_text_draws_for`] uses for retail-style centered
/// dialog. Subtitles are bottom-anchored by the caller's `top_y`.
pub fn cutscene_narration_draws_for(
    font: &legaia_font::Font,
    text: &str,
    center_x: i32,
    top_y: i32,
    color: [f32; 4],
) -> Vec<TextDraw> {
    if text.is_empty() {
        return Vec::new();
    }
    let layout = font.layout_ascii(text);
    let left_x = center_x - (layout.advance_x as i32 / 2);
    text_draws_for(&layout, (left_x, top_y), color)
}

/// One row in the inventory item-use list. Plain-data view so the
/// renderer doesn't depend on `engine-core::inventory_use`.
pub struct InventoryItemRow<'a> {
    pub name: &'a str,
    pub count: u8,
    /// `true` when the item passes the active context's filter
    /// (battle/field). Failing items still appear, dimmed.
    pub admissible: bool,
}

/// One row in the inventory item-use target picker.
pub struct InventoryTargetRow<'a> {
    pub name: &'a str,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    pub alive: bool,
}

/// Bundle of arguments for [`inventory_use_draws_for`]. Bundled so the
/// function takes one payload struct rather than ten positional args.
pub struct InventoryUseDrawArgs<'a> {
    pub items: &'a [InventoryItemRow<'a>],
    pub targets: &'a [InventoryTargetRow<'a>],
    /// `true` for battle context (target column shows monsters too);
    /// `false` for field (party only). Drives the title.
    pub in_battle: bool,
    /// Cursor row inside the active phase column.
    pub cursor: u8,
    /// `0` = item column, `1` = target column.
    pub phase: u8,
    /// Selected item id when in target phase. `None` while browsing.
    pub selected_item_name: Option<&'a str>,
}

/// Build [`TextDraw`]s for the inventory item-use overlay shared by the
/// field menu's Items row and the battle command-menu's Items option.
///
/// Layout (anchored at `pen`):
/// ```text
/// ITEMS
/// > Healing Leaf            x 04         | Vahn        HP 250/300
///   Magic Leaf              x 02         | Noa         HP 180/220
///   Antidote Leaf           x 01         | Gala        HP  90/280
///   ...                                  |
/// ```
///
/// The right-hand target column is only drawn when `phase == 1` (target
/// select). Failing items (filtered out by the active context) render
/// dimmed but stay visible so the player understands why their item
/// disappeared.
pub fn inventory_use_draws_for(
    font: &legaia_font::Font,
    args: InventoryUseDrawArgs<'_>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 14;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
    let red: [f32; 4] = [1.0, 0.55, 0.55, 1.0];

    let mut out = Vec::new();

    let title = if args.in_battle { "ITEMS [B]" } else { "ITEMS" };
    out.extend(text_draws_for(&font.layout_ascii(title), pen, gold));

    if args.items.is_empty() {
        let l = font.layout_ascii("(no usable items)");
        out.extend(text_draws_for(&l, (pen.0, pen.1 + LINE_H), dim));
        return out;
    }

    // Item column.
    for (i, item) in args.items.iter().enumerate() {
        let y = pen.1 + LINE_H + i as i32 * LINE_H;
        let selected_here = args.phase == 0 && i as u8 == args.cursor;
        let color = if !item.admissible {
            dim
        } else if selected_here {
            gold
        } else {
            white
        };
        if selected_here {
            out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
        }
        let line = format!("{:<20} x{:>3}", item.name, item.count);
        out.extend(text_draws_for(
            &font.layout_ascii(&line),
            (pen.0 + 14, y),
            color,
        ));
    }

    // Target column when picking a target.
    if args.phase == 1 {
        let col_x = pen.0 + 240;
        if let Some(name) = args.selected_item_name {
            let head = format!("Use: {name}");
            out.extend(text_draws_for(
                &font.layout_ascii(&head),
                (col_x, pen.1),
                gold,
            ));
        }
        for (i, t) in args.targets.iter().enumerate() {
            let y = pen.1 + LINE_H + i as i32 * LINE_H;
            let selected_here = i as u8 == args.cursor;
            let color = if !t.alive {
                red
            } else if selected_here {
                gold
            } else {
                white
            };
            if selected_here {
                out.extend(text_draws_for(&font.layout_ascii(">"), (col_x, y), color));
            }
            let line = if t.mp_max > 0 {
                format!(
                    "{:<8} HP {:>3}/{:<3} MP {:>3}/{:<3}",
                    t.name, t.hp, t.hp_max, t.mp, t.mp_max
                )
            } else {
                format!("{:<8} HP {:>3}/{:<3}", t.name, t.hp, t.hp_max)
            };
            out.extend(text_draws_for(
                &font.layout_ascii(&line),
                (col_x + 14, y),
                color,
            ));
        }
        if args.targets.is_empty() {
            let l = font.layout_ascii("(no targets)");
            out.extend(text_draws_for(&l, (col_x, pen.1 + LINE_H), dim));
        }
    }
    out
}

/// One slot row in the equipment screen.
pub struct EquipSlotRow<'a> {
    pub label: &'a str,
    /// Currently-equipped item display name. "(empty)" for an unfilled
    /// slot.
    pub current_name: &'a str,
}

/// One candidate row in the per-slot item picker.
pub struct EquipCandidateRow<'a> {
    pub name: &'a str,
    pub count: u8,
    /// Stat preview delta vs. the current equipped item: positive deltas
    /// are tinted green, negatives red. Engines compute these by running
    /// `compute_battle_stats` once with the candidate id installed.
    pub atk_delta: i16,
    pub udf_delta: i16,
}

/// Phase tag for [`equipment_session_draws_for`]. Mirrors
/// `engine-core::equip_session::EquipState` without naming the enum so
/// the renderer doesn't pull engine-core in as a dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipDrawPhase {
    /// Cursor on the slot grid.
    SlotPicker,
    /// Cursor on the candidate-item list for the active slot.
    ItemPicker,
    /// Yes/No confirmation prompt (`cursor` == 0 for Yes, 1 for No).
    Confirm,
}

/// Bundle for [`equipment_session_draws_for`].
pub struct EquipDrawArgs<'a> {
    /// Display name of the character being equipped.
    pub character_name: &'a str,
    pub slots: &'a [EquipSlotRow<'a>],
    /// Candidate items for the active slot. Empty in `SlotPicker` phase.
    pub candidates: &'a [EquipCandidateRow<'a>],
    pub phase: EquipDrawPhase,
    /// Cursor row inside the active phase column.
    pub cursor: u16,
    /// Active slot index (0..=7) when in `ItemPicker` / `Confirm`.
    pub active_slot: u8,
    /// Optional pending swap label rendered above the Yes/No prompt
    /// ("Equip Iron Sword?"). Only consumed in `Confirm`.
    pub confirm_label: Option<&'a str>,
}

/// Build [`TextDraw`]s for the equipment overlay shared by the field
/// menu's Equip row and the shop's "buy then equip" flow.
///
/// Layout (anchored at `pen`):
/// ```text
/// EQUIP - Vahn
/// > Weapon       Iron Sword
///   Helmet       Leather Cap
///   Body Armor   (empty)
///   ...
///                                      | Iron Sword     ATK +10
///                                      | Wood Sword     ATK +5
///                                      | (empty)
///   Equip Iron Sword?  Yes  No
/// ```
pub fn equipment_session_draws_for(
    font: &legaia_font::Font,
    args: EquipDrawArgs<'_>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 14;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
    let green: [f32; 4] = [0.5, 1.0, 0.5, 1.0];
    let red: [f32; 4] = [1.0, 0.55, 0.55, 1.0];

    let mut out = Vec::new();

    let head = format!("EQUIP - {}", args.character_name);
    out.extend(text_draws_for(&font.layout_ascii(&head), pen, gold));

    // Slot column.
    for (i, slot) in args.slots.iter().enumerate() {
        let y = pen.1 + LINE_H + i as i32 * LINE_H;
        let cursor_here = args.phase == EquipDrawPhase::SlotPicker && i as u16 == args.cursor;
        let row_active = args.phase != EquipDrawPhase::SlotPicker && args.active_slot as usize == i;
        let color = if cursor_here || row_active {
            gold
        } else {
            white
        };
        if cursor_here {
            out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
        }
        out.extend(text_draws_for(
            &font.layout_ascii(slot.label),
            (pen.0 + 14, y),
            color,
        ));
        let item_color = if slot.current_name == "(empty)" {
            dim
        } else {
            color
        };
        out.extend(text_draws_for(
            &font.layout_ascii(slot.current_name),
            (pen.0 + 110, y),
            item_color,
        ));
    }

    // Candidate column.
    if args.phase != EquipDrawPhase::SlotPicker {
        let col_x = pen.0 + 250;
        let head = if let Some(slot) = args.slots.get(args.active_slot as usize) {
            format!("> {}", slot.label)
        } else {
            "Slot".to_string()
        };
        out.extend(text_draws_for(
            &font.layout_ascii(&head),
            (col_x, pen.1),
            gold,
        ));

        if args.candidates.is_empty() {
            out.extend(text_draws_for(
                &font.layout_ascii("(no items)"),
                (col_x, pen.1 + LINE_H),
                dim,
            ));
        }
        for (i, c) in args.candidates.iter().enumerate() {
            let y = pen.1 + LINE_H + i as i32 * LINE_H;
            let selected_here = args.phase == EquipDrawPhase::ItemPicker && i as u16 == args.cursor;
            let color = if selected_here { gold } else { white };
            if selected_here {
                out.extend(text_draws_for(&font.layout_ascii(">"), (col_x, y), color));
            }
            let line = format!("{:<14} x{:>2}", c.name, c.count);
            out.extend(text_draws_for(
                &font.layout_ascii(&line),
                (col_x + 14, y),
                color,
            ));
            let mut delta_x = col_x + 14 + 130;
            if c.atk_delta != 0 {
                let s = format!("ATK {:+}", c.atk_delta);
                let dc = if c.atk_delta > 0 { green } else { red };
                out.extend(text_draws_for(&font.layout_ascii(&s), (delta_x, y), dc));
                delta_x += 56;
            }
            if c.udf_delta != 0 {
                let s = format!("UDF {:+}", c.udf_delta);
                let dc = if c.udf_delta > 0 { green } else { red };
                out.extend(text_draws_for(&font.layout_ascii(&s), (delta_x, y), dc));
            }
        }
    }

    // Confirm prompt at the bottom.
    if args.phase == EquipDrawPhase::Confirm {
        let prompt_y = pen.1 + LINE_H + args.slots.len() as i32 * LINE_H + LINE_H;
        if let Some(label) = args.confirm_label {
            out.extend(text_draws_for(
                &font.layout_ascii(label),
                (pen.0, prompt_y),
                white,
            ));
        }
        for (i, opt) in ["Yes", "No"].iter().enumerate() {
            let x = pen.0 + 110 + i as i32 * 50;
            let selected = i as u16 == args.cursor;
            let color = if selected { gold } else { white };
            if selected {
                out.extend(text_draws_for(
                    &font.layout_ascii(">"),
                    (x - 10, prompt_y + LINE_H),
                    color,
                ));
            }
            out.extend(text_draws_for(
                &font.layout_ascii(opt),
                (x, prompt_y + LINE_H),
                color,
            ));
        }
    }
    out
}

/// One saved Tactical Arts chain row in the editor's browse list.
pub struct ArtsChainRow<'a> {
    pub name: &'a str,
    /// One-line stringification of the command sequence ("L R D U R").
    /// Engines build this with `SavedChain::pretty_sequence()`.
    pub pretty_sequence: &'a str,
}

/// Phase tag for [`tactical_arts_editor_draws_for`]. Mirrors
/// `engine-core::tactical_arts_editor::EditorPhase` without depending on
/// the enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtsEditorPhase {
    /// Cursor on the saved-chain list (browse).
    Browsing,
    /// Player editing a sequence of directions.
    Editing,
    /// Player picking a name for the new chain.
    Naming,
}

/// Bundle for [`tactical_arts_editor_draws_for`].
pub struct ArtsEditorDrawArgs<'a> {
    pub character_name: &'a str,
    pub phase: ArtsEditorPhase,
    pub saved: &'a [ArtsChainRow<'a>],
    /// Cursor row inside the saved list. Only consumed in `Browsing`.
    pub browse_cursor: u8,
    /// Live working-sequence pretty string, e.g. "L R D".
    pub editing_pretty: &'a str,
    /// Live working-sequence length (used to display "len 3 / 7" status).
    pub editing_len: usize,
    /// Min / max sequence length the editor enforces (3..=7 in retail).
    pub min_len: usize,
    pub max_len: usize,
    /// Currently-picked name in the naming phase ("Combo A", ...).
    pub naming_name: &'a str,
    /// `true` when there is room in the library for one more saved
    /// chain - the browse list shows a trailing "+ New" row only then.
    pub can_add_new: bool,
}

/// Build [`TextDraw`]s for the Tactical Arts editor overlay shared by
/// the field menu's Arts row.
///
/// Layout (anchored at `pen`) - varies per phase:
/// ```text
/// Browsing:
///   ARTS - Vahn
///   > Combo A     L R D U
///     Striker     U U L R D
///     + New
///
/// Editing:
///   ARTS - Vahn  (Editing)
///   Sequence: L R D     (3 / 7)
///   D-Pad: append   Triangle: pop   Cross: name
///
/// Naming:
///   ARTS - Vahn  (Naming)
///   Name: Combo B
///   Square: cycle    Cross: save    Circle: back
/// ```
pub fn tactical_arts_editor_draws_for(
    font: &legaia_font::Font,
    args: ArtsEditorDrawArgs<'_>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 14;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
    let green: [f32; 4] = [0.5, 1.0, 0.5, 1.0];

    let mut out = Vec::new();

    let head = match args.phase {
        ArtsEditorPhase::Browsing => format!("ARTS - {}", args.character_name),
        ArtsEditorPhase::Editing => format!("ARTS - {}  (Editing)", args.character_name),
        ArtsEditorPhase::Naming => format!("ARTS - {}  (Naming)", args.character_name),
    };
    out.extend(text_draws_for(&font.layout_ascii(&head), pen, gold));

    match args.phase {
        ArtsEditorPhase::Browsing => {
            // Saved chains.
            for (i, chain) in args.saved.iter().enumerate() {
                let y = pen.1 + LINE_H + i as i32 * LINE_H;
                let selected = i as u8 == args.browse_cursor;
                let color = if selected { gold } else { white };
                if selected {
                    out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
                }
                out.extend(text_draws_for(
                    &font.layout_ascii(chain.name),
                    (pen.0 + 14, y),
                    color,
                ));
                out.extend(text_draws_for(
                    &font.layout_ascii(chain.pretty_sequence),
                    (pen.0 + 110, y),
                    color,
                ));
            }
            // Trailing "+ New" row.
            if args.can_add_new {
                let i = args.saved.len();
                let y = pen.1 + LINE_H + i as i32 * LINE_H;
                let selected = i as u8 == args.browse_cursor;
                let color = if selected { gold } else { white };
                if selected {
                    out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
                }
                out.extend(text_draws_for(
                    &font.layout_ascii("+ New"),
                    (pen.0 + 14, y),
                    color,
                ));
            }
            let foot_y = pen.1
                + LINE_H
                + (args.saved.len() + if args.can_add_new { 1 } else { 0 }) as i32 * LINE_H
                + LINE_H;
            out.extend(text_draws_for(
                &font.layout_ascii("Cross: Edit  Triangle: Delete  Circle: Back"),
                (pen.0, foot_y),
                dim,
            ));
        }
        ArtsEditorPhase::Editing => {
            let line1 = format!(
                "Sequence: {}     ({} / {})",
                args.editing_pretty, args.editing_len, args.max_len
            );
            let len_ok = args.editing_len >= args.min_len;
            let color = if len_ok { green } else { white };
            out.extend(text_draws_for(
                &font.layout_ascii(&line1),
                (pen.0, pen.1 + LINE_H),
                color,
            ));
            out.extend(text_draws_for(
                &font.layout_ascii("D-Pad: append   Triangle: pop"),
                (pen.0, pen.1 + LINE_H * 3),
                dim,
            ));
            let cross_hint = if len_ok {
                "Cross: Name & Save"
            } else {
                "Cross: Name & Save  (need 3+ inputs)"
            };
            out.extend(text_draws_for(
                &font.layout_ascii(cross_hint),
                (pen.0, pen.1 + LINE_H * 4),
                if len_ok { gold } else { dim },
            ));
            out.extend(text_draws_for(
                &font.layout_ascii("Circle: Back"),
                (pen.0, pen.1 + LINE_H * 5),
                dim,
            ));
        }
        ArtsEditorPhase::Naming => {
            let l = format!("Name: {}", args.naming_name);
            out.extend(text_draws_for(
                &font.layout_ascii(&l),
                (pen.0, pen.1 + LINE_H),
                gold,
            ));
            let sequence = format!("Sequence: {}", args.editing_pretty);
            out.extend(text_draws_for(
                &font.layout_ascii(&sequence),
                (pen.0, pen.1 + LINE_H * 2),
                white,
            ));
            out.extend(text_draws_for(
                &font.layout_ascii("Square: cycle name   Cross: Save   Circle: Back"),
                (pen.0, pen.1 + LINE_H * 4),
                dim,
            ));
        }
    }

    out
}
