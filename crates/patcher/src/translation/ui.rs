//! Overlay-resident UI menu-string pools.
//!
//! The pause-menu / options / shop / equip / status command labels and the
//! in-battle system messages are **not** in the SCUS name tables or the
//! `0x1F`-segment dialog corpus - they are NUL-terminated C strings in the
//! menu / battle **overlay** data segments, loaded by pointer (a `lui`+`addiu`
//! pair, or via a small pointer table) from the overlay code. That is why they
//! stay English in every language pack until this section covers them.
//!
//! Each pool is a pinned **disc-coordinate range** (a PROT overlay entry + a
//! VA window inside the overlay-window load base), never the text itself, so no
//! Sony bytes are committed. Export scans NUL-to-NUL inside the window and
//! reads the strings off the user's own disc; import writes the translated
//! string back same-size in place at `file_offset = va - base_va`, exactly as
//! the SCUS-string path does, but into the PROT overlay entry.
//!
//! Provenance (`docs/subsystems/field-menu.md`,
//! `docs/tooling/static-overlay-pipeline.md`,
//! `crates/asset/data/static-overlays.toml`):
//!
//! - **Menu overlay** = PROT entry 0899, load base `0x801CE818`. Its leading
//!   rodata string pool (`0x801CE81C..`) holds the options-screen choices, the
//!   `@`-marked per-screen command labels (the command-list renderer
//!   `FUN_801CFD68` at base+0x1550 loads `@Items` = `0x801CE9D0` via
//!   `lui/addiu`, i.e. INCLUDING the leading `0x40` marker byte), the derived
//!   stat labels, and the shop / equip / status strings. The window ends before
//!   the Baka Fighter intro dialog and the jump tables that follow.
//! - **Battle overlay** = PROT entry 0898, load base `0x801CE818`. Its
//!   command / result string pool (`0x801F4B98..`) holds `Spirit` / `Defense`
//!   (the "Defend" command) / `Escape` / `Begin` plus the victory / defeat /
//!   escape / ambush messages. The `Attack` / `Arts` / `Magic` / `Item`
//!   command-ring labels are drawn as UI-icon sprites, not text, so there is no
//!   string to translate for those.
//!
//! The `@` (`0x40`) prefix on the menu command labels is a leading marker byte
//! the string primitive consumes; it is preserved verbatim (it decodes to a
//! literal `@` in the markup, like any other retail glyph) - a translator keeps
//! it exactly as the pipeline keeps `{xx}` control tokens.

use std::collections::BTreeSet;

/// One pinned overlay string pool: a PROT entry + a VA window inside it.
pub struct UiStringPool {
    /// PROT.DAT entry the overlay is extracted from (the write target).
    pub prot_index: usize,
    /// Overlay-window load base (see `static-overlays.toml`). File offset of a
    /// VA inside the entry is `va - base_va`.
    pub base_va: u32,
    /// First VA of the string pool (inclusive).
    pub va_start: u32,
    /// One past the last VA of the string pool (exclusive).
    pub va_end: u32,
    /// Human label for the pack `context` field.
    pub label: &'static str,
}

/// The committed overlay UI string pools. Coordinates only - the strings are
/// read from the user's disc at export time, never stored here.
///
/// Beyond the menu and battle overlays:
///
/// - **Field overlay** = PROT entry 0897, same load base. Two pools: the
///   equip / shop / inn / Genesis-Tree strings that follow the stat labels
///   (`SPD` .. `ATK`, `Cannot equip`, `Sell` / `Buy`, `Your Gold`,
///   `Total Cost`, `OK?` / `Yes` / `No`, the Genesis Tree heal prompt,
///   `Resume` / `End Game`) - the pool ends where the `PLAY1 X` debug labels
///   begin - and the new-game **name-select** prompts (`Cannot enter that
///   name.`, `Tell me my name.`, `Select your name.`).
/// - **Battle tutorial** = PROT entry 0967, slot-B base `0x801F69D8`: the
///   sparring-tutorial prompt strings (`docs/subsystems/battle.md`), the
///   first fight a new game reaches.
/// - **Cast modules** 0941 (steal) and 0954 (fatal decision), slot-B base:
///   the steal outcome lines (`Nothing was stolen.`, `You stole`) and the
///   `MP zero` / `Items lost` / `Gold lost` results.
pub const UI_STRING_POOLS: &[UiStringPool] = &[
    UiStringPool {
        prot_index: 899,
        base_va: 0x801C_E818,
        va_start: 0x801C_E81C,
        va_end: 0x801C_EC78,
        label: "menu",
    },
    UiStringPool {
        prot_index: 898,
        base_va: 0x801C_E818,
        va_start: 0x801F_4B98,
        va_end: 0x801F_4D2A,
        label: "battle",
    },
    UiStringPool {
        prot_index: 897,
        base_va: 0x801C_E818,
        va_start: 0x801C_F048,
        va_end: 0x801C_F1C8,
        label: "field (equip / shop / inn)",
    },
    UiStringPool {
        prot_index: 897,
        base_va: 0x801C_E818,
        va_start: 0x801C_F6AC,
        va_end: 0x801C_F71C,
        label: "field (name select)",
    },
    UiStringPool {
        prot_index: 967,
        base_va: 0x801F_69D8,
        va_start: 0x801F_7684,
        va_end: 0x801F_7D5C,
        label: "battle tutorial",
    },
    UiStringPool {
        prot_index: 941,
        base_va: 0x801F_69D8,
        va_start: 0x801F_83A0,
        va_end: 0x801F_83EC,
        label: "battle (steal)",
    },
    UiStringPool {
        prot_index: 954,
        base_va: 0x801F_69D8,
        va_start: 0x801F_8F30,
        va_end: 0x801F_8FB4,
        label: "battle (fatal decision)",
    },
];

/// Pseudo load base that maps a `SCUS_942.54` virtual address onto its file
/// offset (`0x800`-byte PS-X EXE header, text at `0x80010000`): the same
/// `va - base_va` arithmetic [`scan_pool`] uses for an overlay.
pub const SCUS_POOL_BASE_VA: u32 = 0x8000_F800;

/// `SCUS_942.54`-resident system strings outside the pointer-addressed name
/// tables: NUL-terminated C strings the export walks NUL-to-NUL inside a
/// pinned VA window and the importer rewrites in place like any other
/// `scus:str:` entry (span + alignment padding). Coordinates only.
///
/// - the pause menu's **empty-list messages** and equipment-slot names
///   (`You do not have|any items.`, `No magic skills.`, `No items to equip.`,
///   `Nowhere you can go.`, `{ce:13} Legs.` ...), the strings the menu
///   overlay draws from the executable when a list has nothing to show. The
///   window stops before the per-character `... For {c1:00}.` variants: a
///   NUL-to-NUL scan would cut those at the token's `0x00` argument;
/// - the battle **steal / spoils result** lines (`Took the stolen`,
///   `Recovered the stolen`, `Stole the`, `Recovered all stolen items.`);
/// - the sparring-tutorial opener the new-game template trails (`I will show
///   you how to fight ...`, `docs/formats/new-game-table.md`);
/// - the equip-screen `Remove` and the `Save` label near the config strings.
pub const SCUS_STRING_POOLS: &[UiStringPool] = &[
    UiStringPool {
        prot_index: usize::MAX,
        base_va: SCUS_POOL_BASE_VA,
        va_start: 0x8001_0C18,
        va_end: 0x8001_0CC8,
        label: "pause-menu empty-list messages",
    },
    UiStringPool {
        prot_index: usize::MAX,
        base_va: SCUS_POOL_BASE_VA,
        va_start: 0x8007_7A38,
        va_end: 0x8007_7A8C,
        label: "battle steal result",
    },
    UiStringPool {
        prot_index: usize::MAX,
        base_va: SCUS_POOL_BASE_VA,
        va_start: 0x8007_8CB4,
        va_end: 0x8007_8CF8,
        label: "sparring tutorial opener",
    },
    UiStringPool {
        prot_index: usize::MAX,
        base_va: SCUS_POOL_BASE_VA,
        va_start: 0x8007_B41C,
        va_end: 0x8007_B460,
        label: "equip / save label",
    },
];

/// The overlay load base for a PROT entry that hosts a UI string pool, or
/// `None` if the entry isn't one of the pinned overlays.
pub fn overlay_base_va(prot_index: usize) -> Option<u32> {
    UI_STRING_POOLS
        .iter()
        .find(|p| p.prot_index == prot_index)
        .map(|p| p.base_va)
}

/// One scanned UI string: its VA, raw bytes (no terminator), and the byte
/// budget a same-size in-place translation may occupy.
pub struct UiString {
    /// Virtual address of the string's first byte.
    pub va: u32,
    /// Raw bytes, up to (not including) the NUL terminator.
    pub bytes: Vec<u8>,
    /// Max encoded byte length: the string's own span plus the zero
    /// alignment padding after its terminator, clamped at any interior VA.
    pub budget: usize,
}

/// A chunk is a real UI string (not jump-table pointer bytes or a bare control
/// token) when it carries at least one ASCII letter and two printable bytes -
/// which keeps every menu / battle label and drops the degenerate one-byte
/// control fragments and the pointer bytes that flank the pool.
pub fn qualifies(chunk: &[u8]) -> bool {
    let letters = chunk.iter().filter(|b| b.is_ascii_alphabetic()).count();
    let printable = chunk.iter().filter(|&&b| (0x20..0x7f).contains(&b)).count();
    letters >= 1 && printable >= 2
}

/// Zero alignment padding usable past a string's terminator. The overlay pools
/// are 4-byte aligned, so a string of length `n` occupies `align4(n + 1)` bytes
/// and the 0..3 bytes after its NUL are zero filler. Those bytes are dead
/// (nothing reads past a terminator), so a translation may spill into them as
/// long as it re-terminates. Measured, not assumed - the run must actually be
/// zeros, and it never crosses another scanned string's offset.
fn padding_slack(entry: &[u8], off: usize, strlen: usize, all_offs: &BTreeSet<usize>) -> usize {
    let end = off + strlen; // the NUL
    let aligned = (end + 4) & !3;
    let mut u = end + 1;
    while u < aligned && entry.get(u) == Some(&0) && !all_offs.contains(&u) {
        u += 1;
    }
    u - 1 - end
}

/// Bytes writable at `off` on this disc: the string's own length plus the zero
/// alignment padding after its terminator. Import's same-size guard - the write
/// can never leave this span, so a bad budget can't reach a neighbour.
pub fn writable_span(entry: &[u8], off: usize, cur_len: usize) -> usize {
    let end = off + cur_len;
    let aligned = (end + 4) & !3;
    let mut u = end + 1;
    while u < aligned && entry.get(u) == Some(&0) {
        u += 1;
    }
    (u - 1) - off
}

/// Scan one pool's VA window for NUL-terminated UI strings.
pub fn scan_pool(entry: &[u8], pool: &UiStringPool) -> Vec<UiString> {
    let start = (pool.va_start - pool.base_va) as usize;
    let end = ((pool.va_end - pool.base_va) as usize).min(entry.len());
    if start >= end {
        return Vec::new();
    }
    // NUL-to-NUL chunks that pass the quality gate.
    let mut raw: Vec<(usize, &[u8])> = Vec::new();
    let mut pos = start;
    while pos < end {
        if entry[pos] == 0 {
            pos += 1;
            continue;
        }
        let mut e = pos;
        while e < end && entry[e] != 0 {
            e += 1;
        }
        let chunk = &entry[pos..e];
        if qualifies(chunk) {
            raw.push((pos, chunk));
        }
        pos = e;
    }
    let all_offs: BTreeSet<usize> = raw.iter().map(|(o, _)| *o).collect();
    raw.iter()
        .map(|&(off, bytes)| {
            let strlen = bytes.len();
            // Clamp at the next scanned string that lands inside this span (a
            // pointer-shared interior string); otherwise widen across padding.
            let interior = all_offs
                .range(off + 1..off + strlen + 1)
                .next()
                .map(|&o| o - off);
            let budget = match interior {
                Some(clamped) => clamped,
                None => strlen + padding_slack(entry, off, strlen, &all_offs),
            };
            UiString {
                va: pool.base_va + off as u32,
                bytes: bytes.to_vec(),
                budget,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualifies_keeps_labels_drops_junk() {
        assert!(qualifies(b"Items"));
        assert!(qualifies(b"@Save"));
        assert!(qualifies(b"AGL"));
        // Bare control byte / one-printable fragments are junk.
        assert!(!qualifies(&[0xce]));
        assert!(!qualifies(&[0x40, 0xc1]));
        assert!(!qualifies(b"."));
    }

    #[test]
    fn scan_finds_nul_terminated_strings_with_budget() {
        // Two 4-aligned strings, "Run\0" (no slack) then "Options\0" (1 pad).
        let mut entry = vec![0u8; 0x40];
        entry[0x10..0x14].copy_from_slice(b"Run\0");
        entry[0x14..0x1c].copy_from_slice(b"Options\0");
        let pool = UiStringPool {
            prot_index: 0,
            base_va: 0x1000,
            va_start: 0x1010,
            va_end: 0x1020,
            label: "t",
        };
        let out = scan_pool(&entry, &pool);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].va, 0x1010);
        assert_eq!(out[0].bytes, b"Run");
        assert_eq!(out[0].budget, 3); // "Run" + NUL fills its 4-aligned cell
        assert_eq!(out[1].va, 0x1014);
        assert_eq!(out[1].bytes, b"Options");
    }

    #[test]
    fn writable_span_walks_zero_padding_only() {
        // "Hi\0" at 0, then a zero pad byte, then a non-zero at 0x04.
        let entry = [b'H', b'i', 0, 0, b'X', 0, 0, 0];
        assert_eq!(writable_span(&entry, 0, 2), 3);
    }
}
