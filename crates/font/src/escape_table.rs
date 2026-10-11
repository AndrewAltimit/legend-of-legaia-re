//! The `0xCE` inline-escape substitution table.

use anyhow::{Context, Result};

/// Runtime escape table at SCUS `0x80074050`.
///
/// The dialog renderer dispatches the `0xCE` (inline-escape) byte by
/// indexing this 38-entry table with the next byte. Each entry encodes a
/// substitution: a `string_id` referencing a runtime string, a horizontal
/// `advance_px` (how many pixels to move the pen after the substitution),
/// and a `y_offset` for the substitution's baseline.
///
/// Format on disc: 38 × 4-byte entries - `(i16 string_id, u8 advance_px,
/// i8 y_offset)`. Loaded from
/// `extracted/font/dialog_font_metadata.json` produced by `font-extract`.
#[derive(Debug, Clone)]
pub struct EscapeTable {
    pub entries: Vec<EscapeEntry>,
}

/// One escape-table entry. See [`EscapeTable`] for the format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EscapeEntry {
    pub string_id: i16,
    pub advance_px: u8,
    pub y_offset: i8,
}

impl EscapeTable {
    /// Parse a `dialog_font_metadata.json` file's `escape_table.entries`
    /// array.
    pub fn from_json(json: &str) -> Result<Self> {
        let v: serde_json::Value = serde_json::from_str(json).context("parse escape_table JSON")?;
        let arr = v
            .pointer("/escape_table/entries")
            .and_then(|x| x.as_array())
            .ok_or_else(|| anyhow::anyhow!("missing /escape_table/entries"))?;
        let mut entries = Vec::with_capacity(arr.len());
        for (i, e) in arr.iter().enumerate() {
            let string_id = e
                .get("string_id")
                .and_then(|x| x.as_i64())
                .ok_or_else(|| anyhow::anyhow!("entry {i}: missing string_id"))?
                as i16;
            let advance_px = e
                .get("advance_px")
                .and_then(|x| x.as_u64())
                .ok_or_else(|| anyhow::anyhow!("entry {i}: missing advance_px"))?
                as u8;
            let y_offset = e
                .get("y_offset")
                .and_then(|x| x.as_i64())
                .ok_or_else(|| anyhow::anyhow!("entry {i}: missing y_offset"))?
                as i8;
            entries.push(EscapeEntry {
                string_id,
                advance_px,
                y_offset,
            });
        }
        Ok(Self { entries })
    }

    /// Look up an entry by index - `byte` is the operand of `0xCE`.
    pub fn entry(&self, byte: u8) -> Option<&EscapeEntry> {
        self.entries.get(byte as usize)
    }
}
