//! Parser for the Mednafen `.cht` cheat-file format.
//!
//! ```text
//! cheats = N
//!
//! cheatI_desc = "Description"
//! cheatI_code = "AAAAAAAA VVVV[+AAAAAAAA VVVV ...]"
//! cheatI_enable = (true|false)
//! ```
//!
//! `+` separates multiple writes inside one effect (used for
//! multi-byte writes and conditional codes). The parser reads each
//! `cheatI_*` triplet into a single [`CheatEntry`](crate::CheatEntry).
//!
//! Comments (`#` to end-of-line) and blank lines are tolerated.
//! `cheatI_enable` is parsed but ignored at the data level - the
//! flag is purely a UI hint.

use crate::{CheatCode, CheatEntry, Database};
use std::collections::BTreeMap;

/// Parse a Mednafen `.cht` file into a [`Database`].
pub fn parse_mednafen_cht(input: &str) -> anyhow::Result<Database> {
    // Phase 1: bucket the per-cheat fields by index.
    let mut buckets: BTreeMap<usize, EntryBuilder> = BTreeMap::new();
    for raw in input.lines() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key == "cheats" {
            // Header line - we don't enforce it; some hand-edited
            // files have stale counts.
            continue;
        }
        let Some((idx, field)) = parse_keyed(key) else {
            continue;
        };
        let entry = buckets.entry(idx).or_default();
        match field {
            "desc" => {
                entry.desc = Some(strip_quotes(value).to_string());
            }
            "code" => {
                entry.code = Some(strip_quotes(value).to_string());
            }
            "enable" => {
                entry.enable = parse_bool(value);
            }
            _ => {}
        }
    }

    // Phase 2: build entries.
    let mut db = Database::new();
    for (idx, builder) in buckets {
        let desc = builder
            .desc
            .ok_or_else(|| anyhow::anyhow!("cheat{idx} missing `desc` field"))?;
        let code_str = builder
            .code
            .ok_or_else(|| anyhow::anyhow!("cheat{idx} (`{}`) missing `code` field", desc))?;
        let codes = parse_code_string(&code_str)
            .map_err(|e| anyhow::anyhow!("cheat{idx} (`{}`) code parse: {}", desc, e))?;
        db.entries.push(CheatEntry {
            description: desc,
            codes,
        });
    }
    Ok(db)
}

#[derive(Default)]
struct EntryBuilder {
    desc: Option<String>,
    code: Option<String>,
    #[allow(dead_code)]
    enable: Option<bool>,
}

fn strip_comment(line: &str) -> &str {
    if let Some(idx) = line.find('#') {
        &line[..idx]
    } else {
        line
    }
}

fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(s)
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Decompose `cheat42_desc` into `(42, "desc")`.
fn parse_keyed(key: &str) -> Option<(usize, &str)> {
    let rest = key.strip_prefix("cheat")?;
    let (digits, suffix) = rest.split_at(rest.find('_')?);
    let idx: usize = digits.parse().ok()?;
    let field = suffix.strip_prefix('_')?;
    Some((idx, field))
}

/// Parse a code string like `"80084708 FFFF+8008470A 0098"` into
/// individual [`CheatCode`]s. Each `+`-separated chunk is one
/// `<addr-hex> <value-hex>` pair.
fn parse_code_string(s: &str) -> anyhow::Result<Vec<CheatCode>> {
    let mut out = Vec::new();
    for chunk in s.split('+') {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }
        let mut it = chunk.split_whitespace();
        let addr_hex = it
            .next()
            .ok_or_else(|| anyhow::anyhow!("empty chunk `{chunk}`"))?;
        let val_hex = it
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing value in `{chunk}`"))?;
        if it.next().is_some() {
            anyhow::bail!("trailing tokens in chunk `{chunk}`");
        }
        let addr_packed = u32::from_str_radix(addr_hex, 16)
            .map_err(|_| anyhow::anyhow!("address `{addr_hex}` is not hex"))?;
        let value = u16::from_str_radix(val_hex, 16)
            .map_err(|_| anyhow::anyhow!("value `{val_hex}` is not hex"))?;
        out.push(CheatCode::from_packed(addr_packed, value));
    }
    if out.is_empty() {
        anyhow::bail!("no codes in `{s}`");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CheatOp;

    #[test]
    fn parses_simple_single_write_block() {
        let input = r#"
cheats = 1

cheat0_desc = "100 AP (Vahn)"
cheat0_code = "80084816 0064"
cheat0_enable = false
"#;
        let db = parse_mednafen_cht(input).unwrap();
        assert_eq!(db.entries.len(), 1);
        let e = &db.entries[0];
        assert_eq!(e.description, "100 AP (Vahn)");
        assert_eq!(e.codes.len(), 1);
        assert_eq!(e.codes[0].addr, 0x80084816);
        assert_eq!(e.codes[0].value, 0x0064);
    }

    #[test]
    fn parses_multi_write_with_plus_separators() {
        let input = r#"
cheat0_desc = "Max Exp (Vahn)"
cheat0_code = "80084708 FFFF+8008470A 0098"
cheat0_enable = false
"#;
        let db = parse_mednafen_cht(input).unwrap();
        let e = &db.entries[0];
        assert_eq!(e.codes.len(), 2);
        assert_eq!(e.codes[0].addr, 0x80084708);
        assert_eq!(e.codes[0].value, 0xFFFF);
        assert_eq!(e.codes[1].addr, 0x8008470A);
        assert_eq!(e.codes[1].value, 0x0098);
    }

    #[test]
    fn parses_conditional_codes() {
        let input = r#"
cheat0_desc = "Press R2 For Debug Menu"
cheat0_code = "D007B7C0 0002+D007B83C 0003+8007B83C 0000"
cheat0_enable = false
"#;
        let db = parse_mednafen_cht(input).unwrap();
        let e = &db.entries[0];
        assert_eq!(e.codes.len(), 3);
        assert_eq!(e.codes[0].op, CheatOp::IfEqU16);
        assert_eq!(e.codes[1].op, CheatOp::IfEqU16);
        assert_eq!(e.codes[2].op, CheatOp::WriteU16);
    }

    #[test]
    fn parses_write_u8_with_30_prefix() {
        let input = r#"
cheat0_desc = "Magic Slot Activator"
cheat0_code = "30084844 0024"
cheat0_enable = false
"#;
        let db = parse_mednafen_cht(input).unwrap();
        let e = &db.entries[0];
        assert_eq!(e.codes.len(), 1);
        assert_eq!(e.codes[0].op, CheatOp::WriteU8);
        assert_eq!(e.codes[0].addr, 0x80084844);
        assert_eq!(e.codes[0].value, 0x0024);
    }

    #[test]
    fn ignores_blank_lines_and_comments() {
        let input = r#"
# this is a comment
cheats = 1

# blank line below

cheat0_desc = "Test"
cheat0_code = "80084816 0064"
cheat0_enable = true
"#;
        let db = parse_mednafen_cht(input).unwrap();
        assert_eq!(db.entries.len(), 1);
    }

    #[test]
    fn rejects_entry_missing_code() {
        let input = r#"
cheat0_desc = "Test"
cheat0_enable = false
"#;
        assert!(parse_mednafen_cht(input).is_err());
    }

    #[test]
    fn parses_indices_in_any_order() {
        let input = r#"
cheat5_desc = "Five"
cheat5_code = "80000005 0005"
cheat0_desc = "Zero"
cheat0_code = "80000000 0000"
cheat3_desc = "Three"
cheat3_code = "80000003 0003"
"#;
        let db = parse_mednafen_cht(input).unwrap();
        // BTreeMap iteration → ordered by index.
        assert_eq!(db.entries.len(), 3);
        assert_eq!(db.entries[0].description, "Zero");
        assert_eq!(db.entries[1].description, "Three");
        assert_eq!(db.entries[2].description, "Five");
    }
}
