use crate::*;

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
