//! Names for the `0xCE` escape symbols - the `{ce:NN}` tokens of a pack.
//!
//! The byte after a `0xCE` indexes the 38-entry escape table at SCUS
//! `0x80074050` (`docs/formats/dialog-font.md`). Each entry either draws one
//! sprite of the system-UI sheet (a controller button, a target / gold
//! badge, an equip-slot or element icon) or, when its `string_id` is `0`,
//! prints one of the four script counters at `0x801C6460` as a number.
//!
//! [`SYMBOLS`] is the one name table: the workbench palette, the preview and
//! the docs read it. Each symbol also has a readable **alias** a pack may use
//! in place of the hex token - `{btn:x}` imports as the same two bytes as
//! `{ce:00}` ([`super::markup::encode`]). Export always writes the hex form,
//! so a pack that uses aliases re-imports byte-identically, and a re-export
//! of the patched disc reads `{ce:NN}` again.
//!
//! The labels were checked sprite by sprite against the disc (escape table ->
//! sprite record at `0x800732A4` -> system-UI sheet texels + CLUT); the list
//! a community translator contributed agreed on every drawn icon, and the
//! differences are recorded in `docs/formats/dialog-font.md`.

/// What an escape draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    /// A controller-button sprite (16x16, drawn 2 px above the text line).
    Button,
    /// A small sprite icon.
    Icon,
    /// A runtime number: script counter `index - 0x0B` at `0x801C6460`.
    Number,
    /// A sprite of pre-drawn text.
    Text,
}

impl SymbolKind {
    /// Lowercase name used in JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            SymbolKind::Button => "button",
            SymbolKind::Icon => "icon",
            SymbolKind::Number => "number",
            SymbolKind::Text => "text",
        }
    }
}

/// One `{ce:NN}` symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Symbol {
    /// The escape operand (`NN` in `{ce:NN}`).
    pub index: u8,
    /// Readable alias, written `{alias}` in a pack.
    pub alias: &'static str,
    /// What it draws.
    pub name: &'static str,
    pub kind: SymbolKind,
}

const fn s(index: u8, alias: &'static str, name: &'static str, kind: SymbolKind) -> Symbol {
    Symbol {
        index,
        alias,
        name,
        kind,
    }
}

use SymbolKind::{Button as B, Icon as I, Number as N, Text as T};

/// Every retail escape-table entry, `0x00..=0x25`. Operands past `0x25` read
/// whatever follows the table and have no name.
pub const SYMBOLS: [Symbol; 38] = [
    s(0x00, "btn:x", "X button", B),
    s(0x01, "btn:circle", "Circle button", B),
    s(0x02, "btn:square", "Square button", B),
    s(0x03, "btn:triangle", "Triangle button", B),
    s(0x04, "btn:r1", "R1 button", B),
    s(0x05, "btn:r2", "R2 button", B),
    s(0x06, "btn:l1", "L1 button", B),
    s(0x07, "btn:l2", "L2 button", B),
    s(0x08, "icon:gold", "G (gold) icon", I),
    s(0x09, "icon:single", "I (one target) icon", I),
    s(0x0A, "icon:all", "A (all targets) icon", I),
    s(0x0B, "num:0", "Number: script counter 0", N),
    s(0x0C, "num:1", "Number: script counter 1", N),
    s(0x0D, "num:2", "Number: script counter 2", N),
    s(0x0E, "num:3", "Number: script counter 3", N),
    s(
        0x0F,
        "text:ra-seru",
        "Ra-Seru in Japanese kana (text sprite)",
        T,
    ),
    s(0x10, "icon:arms", "Arms (fist) equip-slot icon", I),
    s(0x11, "icon:head", "Head (helmet) equip-slot icon", I),
    s(0x12, "icon:body", "Body (armor) equip-slot icon", I),
    s(0x13, "icon:legs", "Legs (boot) equip-slot icon", I),
    s(0x14, "icon:fire", "Fire element icon", I),
    s(0x15, "icon:thunder", "Thunder element icon", I),
    s(0x16, "icon:wind", "Wind element icon", I),
    s(0x17, "icon:water", "Water element icon", I),
    s(0x18, "icon:earth", "Earth element icon", I),
    s(0x19, "icon:light", "Light element icon", I),
    s(0x1A, "icon:dark", "Dark element icon", I),
    s(0x1B, "icon:monster", "Monster icon", I),
    s(
        0x1C,
        "icon:fire-cut",
        "Fire icon cut to 12 px (left edge only)",
        I,
    ),
    s(0x1D, "icon:fire2", "Winged fire icon", I),
    s(0x1E, "icon:thunder2", "Winged thunder icon", I),
    s(0x1F, "icon:wind2", "Winged wind icon", I),
    s(0x20, "icon:water2", "Winged water icon", I),
    s(0x21, "icon:earth2", "Winged earth icon", I),
    s(0x22, "icon:light2", "Winged light icon", I),
    s(0x23, "icon:dark2", "Winged dark icon", I),
    s(0x24, "icon:monster2", "Monster icon, wide plate", I),
    s(0x25, "icon:fire-wide", "Fire icon, 28 px wide", I),
];

/// The symbol for escape operand `index`.
pub fn by_index(index: u8) -> Option<&'static Symbol> {
    SYMBOLS.get(index as usize)
}

/// The escape operand an alias names (`"btn:x"` -> `0x00`), case-insensitive.
/// The alias is the text between the braces.
pub fn index_for_alias(alias: &str) -> Option<u8> {
    SYMBOLS
        .iter()
        .find(|s| s.alias.eq_ignore_ascii_case(alias))
        .map(|s| s.index)
}

/// `true` when `inner` (the text between a pair of braces) has the shape of
/// an alias - a known prefix and a name - so an unknown one can be reported
/// as an unknown symbol rather than as malformed hex.
pub fn looks_like_alias(inner: &str) -> bool {
    let lower = inner.to_ascii_lowercase();
    ["btn:", "icon:", "num:", "text:"]
        .iter()
        .any(|p| lower.starts_with(p) && lower.len() > p.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_dense_and_aliases_unique() {
        for (i, s) in SYMBOLS.iter().enumerate() {
            assert_eq!(s.index as usize, i);
            assert!(looks_like_alias(s.alias), "{}", s.alias);
            assert_eq!(index_for_alias(s.alias), Some(s.index));
            // No alias may be mistaken for a hex escape (`{xx}` / `{xx:yy}`).
            let hexish = |t: &str| t.len() == 2 && t.chars().all(|c| c.is_ascii_hexdigit());
            let parts: Vec<&str> = s.alias.split(':').collect();
            assert!(!parts.iter().all(|p| hexish(p)), "{}", s.alias);
            assert!(!s.alias.contains('{') && !s.alias.contains('}'));
        }
        let mut seen = std::collections::HashSet::new();
        for s in &SYMBOLS {
            assert!(seen.insert(s.alias.to_ascii_lowercase()), "dup {}", s.alias);
        }
        assert_eq!(index_for_alias("BTN:X"), Some(0));
        assert_eq!(index_for_alias("btn:start"), None);
    }
}
