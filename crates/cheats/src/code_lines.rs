//! Parser for a plain GameShark **code list** - the form codes are shared
//! and pasted in:
//!
//! ```text
//! 80084E28 FFFF   Max gold (low)
//! D007B850 0010
//! 80084E2A 0098
//! ```
//!
//! One `AAAAAAAA VVVV` pair per line, anything after the value is the
//! description. A conditional line (`D0` / `E0` / ...) gates the line after
//! it, so it opens the entry that line closes; every other line is an entry
//! of its own.

use crate::{CheatCode, CheatEntry, Database};

/// Whether `line` (trimmed) is an `AAAAAAAA VVVV` code line.
pub fn is_code_line(line: &str) -> bool {
    let mut it = line.split_whitespace();
    let (Some(a), Some(v)) = (it.next(), it.next()) else {
        return false;
    };
    a.len() == 8
        && v.len() == 4
        && a.chars().all(|c| c.is_ascii_hexdigit())
        && v.chars().all(|c| c.is_ascii_hexdigit())
}

/// Parse a code list. Blank lines and `#` comments are skipped; any other
/// line that is not a code line is an error.
pub fn parse_code_lines(input: &str) -> anyhow::Result<Database> {
    let mut db = Database::new();
    let mut pending: Vec<CheatCode> = Vec::new();
    for (lineno, raw) in input.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !is_code_line(line) {
            anyhow::bail!(
                "line {} (`{raw}`) is not an `AAAAAAAA VVVV` code",
                lineno + 1
            );
        }
        let mut it = line.split_whitespace();
        let addr = u32::from_str_radix(it.next().unwrap_or_default(), 16)?;
        let value = u16::from_str_radix(it.next().unwrap_or_default(), 16)?;
        let desc = it.collect::<Vec<_>>().join(" ");
        let code = CheatCode::from_packed(addr, value);
        pending.push(code);
        if code.is_conditional() {
            continue;
        }
        db.entries.push(CheatEntry {
            description: if desc.is_empty() {
                format!("{addr:08X} {value:04X}")
            } else {
                desc
            },
            codes: std::mem::take(&mut pending),
        });
    }
    Ok(db)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_conditional_opens_the_entry_of_the_line_it_gates() {
        let db =
            parse_code_lines("80084E28 FFFF Max gold\nD007B850 0010\n80084E2A 0098\n").unwrap();
        assert_eq!(db.entries.len(), 2);
        assert_eq!(db.entries[0].description, "Max gold");
        assert_eq!(db.entries[1].codes.len(), 2);
        assert!(db.entries[1].codes[0].is_conditional());
    }

    #[test]
    fn rejects_a_line_that_is_not_a_code() {
        assert!(parse_code_lines("R I 2 L 0 80084816 64 100 AP").is_err());
        assert!(is_code_line("80084816 0064"));
        assert!(!is_code_line("R I 2 L 0 80084816 64"));
    }
}
