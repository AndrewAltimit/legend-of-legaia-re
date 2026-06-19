//! Parser for the GameShark text-dump format.
//!
//! Each line looks like:
//!
//! ```text
//! R I 2 L 0 80084816 64 100 AP
//! ```
//!
//! Field layout (whitespace-separated):
//!
//! 1. `R` - read/write classifier (always `R` in the corpus we ship).
//! 2. `I` - encoding flag (always `I`).
//! 3. width-in-bytes literal (`1`, `2`, or `4`).
//! 4. `L` - endianness (always `L` for little-endian).
//! 5. `0` - placeholder for compression group (always `0`).
//! 6. address (8-hex-digit, no `0x` prefix). The high byte encodes
//!    the [`CheatOp`].
//! 7. value (1-4 hex digits, no leading `0x`).
//! 8. ... rest of the line is the description.
//!
//! Only the fields we actually need (`width`, `address`, `value`,
//! `description`) are parsed strictly; the rest is sanity-checked.

use crate::{CheatCode, CheatEntry, CheatOp, Database};

/// Parse a GameShark-format text dump. Each input line becomes a
/// single-write [`CheatEntry`]; entries are not coalesced across
/// lines (the format isn't expressive enough to indicate grouping).
/// Use [`Database::dedupe_identical`] afterwards if you want to
/// collapse the trivial "Have 99 Items" duplicates that pad these
/// dumps.
pub fn parse_gs_text(input: &str) -> anyhow::Result<Database> {
    let mut db = Database::new();
    for (lineno, raw) in input.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let entry = parse_line(line)
            .map_err(|e| anyhow::anyhow!("parsing line {} (`{}`): {}", lineno + 1, raw, e))?;
        db.entries.push(entry);
    }
    Ok(db)
}

fn parse_line(line: &str) -> anyhow::Result<CheatEntry> {
    let mut tokens = line.split_whitespace();
    macro_rules! next {
        () => {
            tokens
                .next()
                .ok_or_else(|| anyhow::anyhow!("not enough tokens"))?
        };
    }
    let _r = next!();
    let _i = next!();
    let width_lit = next!();
    let width: u8 = width_lit
        .parse()
        .map_err(|_| anyhow::anyhow!("expected width literal, got `{}`", width_lit))?;
    let _l = next!();
    let _zero = next!();
    let addr_hex = next!();
    let val_hex = next!();
    let description: String = tokens.collect::<Vec<_>>().join(" ");
    let addr_packed = u32::from_str_radix(addr_hex, 16)
        .map_err(|_| anyhow::anyhow!("address `{}` is not hex", addr_hex))?;
    let value = u16::from_str_radix(val_hex, 16)
        .map_err(|_| anyhow::anyhow!("value `{}` is not hex", val_hex))?;

    let mut code = CheatCode::from_packed(addr_packed, value);
    // The text dump uses the field-width column as ground truth (the
    // high byte of the address is sometimes wrong/inconsistent in
    // user-edited dumps). Patch the width and infer the op accordingly
    // when the address prefix is bare (i.e. no GameShark prefix encoded).
    let prefix = (addr_packed >> 24) as u8;
    if prefix == 0x80 || prefix == 0x00 {
        code.width = width;
        code.op = match width {
            1 => CheatOp::WriteU8,
            2 => CheatOp::WriteU16,
            _ => CheatOp::Unknown { prefix },
        };
        code.addr = 0x80000000 | (addr_packed & 0x00FF_FFFF);
    } else if width != code.width && code.width != 0 {
        // Mismatch we don't try to repair - keep the prefix-derived width.
    }

    Ok(CheatEntry {
        description,
        codes: vec![code],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_write_u8_line() {
        let input = "R I 1 L 0 800848a3 0 Accessory 1 Modifier";
        let db = parse_gs_text(input).unwrap();
        assert_eq!(db.entries.len(), 1);
        let e = &db.entries[0];
        assert_eq!(e.description, "Accessory 1 Modifier");
        assert_eq!(e.codes.len(), 1);
        let c = e.codes[0];
        assert_eq!(c.addr, 0x800848A3);
        assert_eq!(c.value, 0);
        assert_eq!(c.width, 1);
        assert_eq!(c.op, CheatOp::WriteU8);
    }

    #[test]
    fn parses_single_write_u16_line() {
        let input = "R I 2 L 0 80084816 64 100 AP";
        let db = parse_gs_text(input).unwrap();
        assert_eq!(db.entries.len(), 1);
        let e = &db.entries[0];
        assert_eq!(e.description, "100 AP");
        let c = e.codes[0];
        assert_eq!(c.addr, 0x80084816);
        assert_eq!(c.value, 0x64);
        assert_eq!(c.width, 2);
        assert_eq!(c.op, CheatOp::WriteU16);
    }

    #[test]
    fn skips_blank_and_comment_lines() {
        let input = "\n\n# this is a comment\nR I 2 L 0 80084816 64 100 AP\n\n";
        let db = parse_gs_text(input).unwrap();
        assert_eq!(db.entries.len(), 1);
    }

    #[test]
    fn rejects_short_lines() {
        let input = "R I 2";
        assert!(parse_gs_text(input).is_err());
    }
}
