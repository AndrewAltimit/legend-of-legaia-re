use crate::*;

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
