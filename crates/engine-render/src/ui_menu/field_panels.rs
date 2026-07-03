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
