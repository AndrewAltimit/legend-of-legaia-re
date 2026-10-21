//! Text as the dialog font draws it, for hosts that show a disc string in
//! a system font instead of the game's glyph atlas.
//!
//! Most printable ASCII cells of the font page draw their ASCII character,
//! but not all: cell `0x24` (`$`) draws the **level mark** - `Lv` on the
//! USA, German and Italian pages, `Nv` (niveau) on the French one - and
//! cell `0x2A` (`*`) draws a multiplication sign. The monster archive's
//! variant suffix is written with the first (`Gimard $2` reads
//! `Gimard Lv2` in battle). See `docs/formats/dialog-font.md`.
//!
//! A host that draws through the font atlas already gets the right glyph
//! from the byte; this is for everything else (web pages, desktop lists,
//! exported file names). The stored string stays the raw disc text, so a
//! re-pack or a translation round-trip never sees the display form.

/// What font cell `0x24` draws on build `exe`: `Nv` on the French PAL
/// executable, `Lv` on every other page (and when the build is unknown).
pub fn level_mark(exe: Option<&str>) -> &'static str {
    match exe {
        Some("SCES_019.44") => "Nv",
        _ => "Lv",
    }
}

/// `raw` (a disc string with its `^X` escapes already split off) as the
/// font draws it: `$` becomes `level_mark`, `*` becomes `×`.
pub fn display_name(raw: &str, level_mark: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 2);
    for c in raw.chars() {
        match c {
            '$' => out.push_str(level_mark),
            '*' => out.push('\u{00D7}'),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_read_the_way_the_font_draws_them() {
        let usa = level_mark(Some("SCUS_942.54"));
        assert_eq!(display_name("Gilium $3", usa), "Gilium Lv3");
        let fr = level_mark(Some("SCES_019.44"));
        assert_eq!(display_name("Gimard $2", fr), "Gimard Nv2");
        assert_eq!(level_mark(None), "Lv");
        assert_eq!(display_name("Killer Bee * 2", usa), "Killer Bee \u{d7} 2");
        assert_eq!(display_name("Gola Gola", usa), "Gola Gola");
    }
}
