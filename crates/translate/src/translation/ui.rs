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
//!   escape / ambush messages. The command chips beside the ring icons
//!   (`Attack` / `Item` / `Run` / `Begin`) are text too, in the executable's
//!   small-data pool - a strict [`SCUS_STRING_POOLS`] entry.
//!
//! The `@` (`0x40`) prefix on the menu command labels is a leading marker byte
//! the string primitive consumes; it is preserved verbatim (it decodes to a
//! literal `@` in the markup, like any other retail glyph) - a translator keeps
//! it exactly as the pipeline keeps `{xx}` control tokens.

use std::collections::BTreeSet;

use super::markup;

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
    /// Scan mode. `false` (the original pools) walks NUL to NUL and keeps
    /// every chunk [`qualifies`] admits. `true` walks each string with its
    /// two-byte control tokens stepped over - a `{c1:00}` name token carries a
    /// `0x00` argument that is not the terminator - and keeps only a chunk that
    /// reads as UI text ([`is_ui_text`]): the windows these pools cover
    /// interleave player-facing strings with debug `printf` formats and
    /// pointer tables.
    pub strict: bool,
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
///
/// The [`UiStringPool::strict`] pools cover windows that mix player-facing
/// strings with debug formats and pointer tables:
///
/// - **Battle overlay** 0898, beyond the command / result pool: the round
///   counters (`Turns Left:` / `HP Left:`) and the Hyper Arts list help
///   (`Button: View Hyper Arts list`) that open the image, `Counterattack
///   successful!` / `Points returned`, the Seru-magic effect lines (`Magic
///   effect: ...`, `No effect.`) and the post-battle `'s magic level
///   increased.` / ` ran away.`;
/// - **Field overlay** 0897: the record screen's stat labels (`No. of
///   Battles` .. `Treasure`), `Give up?`, the name-entry confirmation (`Is
///   this name okay?`) ahead of the name-select pool, and `[Nameless]`;
/// - **Menu overlay** 0899: the Delilas-bout preamble (`@The battle will
///   begin.` .. `@Forget all hyper arts moves.`) and the memory-card / save
///   messages (`Now checking MEMORY CARD` .. `Now Loading`).
pub const UI_STRING_POOLS: &[UiStringPool] = &[
    UiStringPool {
        prot_index: 899,
        base_va: 0x801C_E818,
        va_start: 0x801C_E81C,
        va_end: 0x801C_EC78,
        label: "menu",
        strict: false,
    },
    UiStringPool {
        prot_index: 898,
        base_va: 0x801C_E818,
        va_start: 0x801F_4B98,
        va_end: 0x801F_4D2A,
        label: "battle",
        strict: false,
    },
    UiStringPool {
        prot_index: 897,
        base_va: 0x801C_E818,
        va_start: 0x801C_F048,
        va_end: 0x801C_F1C8,
        label: "field (equip / shop / inn)",
        strict: false,
    },
    UiStringPool {
        prot_index: 897,
        base_va: 0x801C_E818,
        va_start: 0x801C_F6AC,
        va_end: 0x801C_F71C,
        label: "field (name select)",
        strict: false,
    },
    UiStringPool {
        prot_index: 967,
        base_va: 0x801F_69D8,
        va_start: 0x801F_7684,
        va_end: 0x801F_7D5C,
        label: "battle tutorial",
        strict: false,
    },
    UiStringPool {
        prot_index: 941,
        base_va: 0x801F_69D8,
        va_start: 0x801F_83A0,
        va_end: 0x801F_83EC,
        label: "battle (steal)",
        strict: false,
    },
    UiStringPool {
        prot_index: 954,
        base_va: 0x801F_69D8,
        va_start: 0x801F_8F30,
        va_end: 0x801F_8FB4,
        label: "battle (fatal decision)",
        strict: false,
    },
    UiStringPool {
        prot_index: 898,
        base_va: 0x801C_E818,
        va_start: 0x801C_E818,
        va_end: 0x801C_E878,
        label: "battle (turn / HP counters, Hyper Arts list help)",
        strict: true,
    },
    UiStringPool {
        prot_index: 898,
        base_va: 0x801C_E818,
        va_start: 0x801C_ED18,
        va_end: 0x801C_ED44,
        label: "battle (counterattack / points returned)",
        strict: true,
    },
    UiStringPool {
        prot_index: 898,
        base_va: 0x801C_E818,
        va_start: 0x801C_F638,
        va_end: 0x801C_FA2C,
        label: "battle (Seru magic effect lines)",
        strict: true,
    },
    UiStringPool {
        prot_index: 898,
        base_va: 0x801C_E818,
        va_start: 0x801F_6844,
        va_end: 0x801F_686C,
        label: "battle (magic level / ran away)",
        strict: true,
    },
    UiStringPool {
        prot_index: 897,
        base_va: 0x801C_E818,
        va_start: 0x801C_F51C,
        va_end: 0x801C_F59C,
        label: "field (record screen)",
        strict: true,
    },
    UiStringPool {
        prot_index: 897,
        base_va: 0x801C_E818,
        va_start: 0x801C_F650,
        va_end: 0x801C_F658,
        label: "field (give up)",
        strict: true,
    },
    UiStringPool {
        prot_index: 897,
        base_va: 0x801C_E818,
        va_start: 0x801C_F698,
        va_end: 0x801C_F6AC,
        label: "field (name confirm)",
        strict: true,
    },
    UiStringPool {
        prot_index: 897,
        base_va: 0x801C_E818,
        va_start: 0x801C_F748,
        va_end: 0x801C_F754,
        label: "field (nameless save)",
        strict: true,
    },
    UiStringPool {
        prot_index: 899,
        base_va: 0x801C_E818,
        va_start: 0x801C_EC78,
        va_end: 0x801C_EDC8,
        label: "menu (Delilas bout)",
        strict: true,
    },
    UiStringPool {
        prot_index: 899,
        base_va: 0x801C_E818,
        va_start: 0x801C_EF18,
        va_end: 0x801C_F218,
        label: "menu (memory card)",
        strict: true,
    },
    UiStringPool {
        prot_index: 899,
        base_va: 0x801C_E818,
        va_start: 0x801C_F244,
        va_end: 0x801C_F40C,
        label: "menu (memory card)",
        strict: true,
    },
    UiStringPool {
        prot_index: 899,
        base_va: 0x801C_E818,
        va_start: 0x801C_F444,
        va_end: 0x801C_F574,
        label: "menu (memory card)",
        strict: true,
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
///
/// Strict pools (token-aware scan, see [`UiStringPool::strict`]):
///
/// - the per-character equip-slot names (`{ce:13} Legs. For {c1:00}.`);
/// - the post-battle level-up lines (`Everyone's level increased!`,
///   `{c1:01}'s and {c1:02}'s level increased!` ...), the target-menu `All
///   Allies`, the round prompt's `Reselect` chip and the Seru-obtained
///   `|You now have the {c2:01}.`;
/// - the battle **command chips** in the small-data pool (`Auto`, `Command`,
///   `  All`, `MP`, `Attack`, `Item`, `Run`, `Begin`), each the payload of a
///   screen-element placement record (`legaia_asset::screen_elements`).
pub const SCUS_STRING_POOLS: &[UiStringPool] = &[
    UiStringPool {
        prot_index: usize::MAX,
        base_va: SCUS_POOL_BASE_VA,
        va_start: 0x8001_0C18,
        va_end: 0x8001_0CC8,
        label: "pause-menu empty-list messages",
        strict: false,
    },
    UiStringPool {
        prot_index: usize::MAX,
        base_va: SCUS_POOL_BASE_VA,
        va_start: 0x8007_7A38,
        va_end: 0x8007_7A8C,
        label: "battle steal result",
        strict: false,
    },
    UiStringPool {
        prot_index: usize::MAX,
        base_va: SCUS_POOL_BASE_VA,
        va_start: 0x8007_8CB4,
        va_end: 0x8007_8CF8,
        label: "sparring tutorial opener",
        strict: false,
    },
    UiStringPool {
        prot_index: usize::MAX,
        base_va: SCUS_POOL_BASE_VA,
        va_start: 0x8007_B41C,
        va_end: 0x8007_B460,
        label: "equip / save label",
        strict: false,
    },
    UiStringPool {
        prot_index: usize::MAX,
        base_va: SCUS_POOL_BASE_VA,
        va_start: 0x8001_0CC8,
        va_end: 0x8001_0D18,
        label: "pause-menu per-character equip-slot names",
        strict: true,
    },
    UiStringPool {
        prot_index: usize::MAX,
        base_va: SCUS_POOL_BASE_VA,
        va_start: 0x8001_5204,
        va_end: 0x8001_52FC,
        label: "battle level-up / target / reselect / Seru obtained",
        strict: true,
    },
    UiStringPool {
        prot_index: usize::MAX,
        base_va: SCUS_POOL_BASE_VA,
        va_start: 0x8007_B658,
        va_end: 0x8007_B690,
        label: "battle command chips",
        strict: true,
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

/// Offset of the terminator of the string starting at `off`, stepping over
/// every two-byte control token (whose argument byte may be `0x00` - the
/// `{c1:00}` name token), capped at `end`. `None` when the walk runs off
/// `end` without meeting a terminator.
pub fn token_terminator(buf: &[u8], off: usize, end: usize) -> Option<usize> {
    let end = end.min(buf.len());
    let mut i = off;
    while i < end {
        match buf[i] {
            0 => return Some(i),
            b if markup::is_two_byte_op(b) => i += 2,
            _ => i += 1,
        }
    }
    None
}

/// The token-aware string at `off` (no terminator), capped at [`MAX_UI_STRLEN`]
/// bytes. `None` for an empty or unterminated run.
pub fn token_cstr(buf: &[u8], off: usize) -> Option<&[u8]> {
    let t = token_terminator(buf, off, off.saturating_add(MAX_UI_STRLEN))?;
    (t > off).then(|| &buf[off..t])
}

/// Longest UI string the token-aware reader follows.
pub const MAX_UI_STRLEN: usize = 512;

/// A strict-pool chunk reads as player-facing UI text: every byte a printable
/// glyph or a two-byte control token (with its argument), at least two ASCII
/// letters, no `printf` conversion (`%d`) and no identifier underscore -
/// which leaves out the debug formats (they end in a `\n` too) the same
/// windows carry. The test never depends on letter case, so a translated
/// label re-exports at its key; the few bare debug words (`busy`, the scene
/// name `opdeene`) are kept out by the window bounds instead.
pub fn is_ui_text(chunk: &[u8]) -> bool {
    let (mut i, mut letters) = (0usize, 0usize);
    while i < chunk.len() {
        let b = chunk[i];
        if markup::is_two_byte_op(b) && b != b'^' {
            if i + 1 >= chunk.len() {
                return false;
            }
            i += 2;
            continue;
        }
        // A `printf` conversion (`%d`, `%s`, `%1d`) marks a debug format; a
        // bare percent sign (`down 20%`) is prose.
        let conversion = b == b'%' && chunk.get(i + 1).is_some_and(u8::is_ascii_alphanumeric);
        if !(0x20..0x7F).contains(&b) || conversion || b == b'_' {
            return false;
        }
        letters += usize::from(b.is_ascii_alphabetic());
        i += 1;
    }
    letters >= 2
}

/// Offset of the text that ends `chunk`: one past its last byte that is
/// neither a glyph (`0x20..0x7E`) nor part of a two-byte control token.
fn text_tail(chunk: &[u8]) -> usize {
    let (mut i, mut tail) = (0usize, 0usize);
    while i < chunk.len() {
        let b = chunk[i];
        if markup::is_two_byte_op(b) && b != b'^' && i + 1 < chunk.len() {
            i += 2;
            continue;
        }
        if !(0x20..0x7F).contains(&b) {
            tail = i + 1;
        }
        i += 1;
    }
    tail
}

/// The pool (overlay or SCUS) whose window holds `va` for PROT entry `prot`
/// (`usize::MAX` for `SCUS_942.54`).
pub fn pool_for(prot: usize, va: u32) -> Option<&'static UiStringPool> {
    UI_STRING_POOLS
        .iter()
        .chain(SCUS_STRING_POOLS)
        .find(|p| p.prot_index == prot && (p.va_start..p.va_end).contains(&va))
}

/// Length of the string at `off` the way the pool holding it reads strings:
/// token-aware in a strict pool, NUL-to-NUL otherwise. `None` for an empty or
/// unterminated run.
pub fn pool_strlen(buf: &[u8], off: usize, strict: bool) -> Option<usize> {
    if strict {
        return token_cstr(buf, off).map(<[u8]>::len);
    }
    buf.get(off..)?
        .iter()
        .take(MAX_UI_STRLEN)
        .position(|&b| b == 0)
        .filter(|&l| l > 0)
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
        let e = if pool.strict {
            // A string that starts inside the window belongs to the pool
            // wherever it ends.
            match token_terminator(entry, pos, pos + MAX_UI_STRLEN) {
                Some(t) => t,
                // A run the window's end cuts is not a string of this pool.
                None => break,
            }
        } else {
            let mut e = pos;
            while e < end && entry[e] != 0 {
                e += 1;
            }
            e
        };
        let mut start = pos;
        let keep = if pool.strict {
            // A string can follow a pointer table with no NUL between: key it
            // on the text after the last byte that is not a glyph.
            start = pos + text_tail(&entry[pos..e]);
            is_ui_text(&entry[start..e])
        } else {
            qualifies(&entry[pos..e])
        };
        if keep {
            raw.push((start, &entry[start..e]));
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
            strict: false,
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
    fn strict_scan_steps_over_token_arguments_and_debug_formats() {
        // `{c1:00}'s` (token with a 0x00 argument), two printf formats, a
        // pointer run glued to a label, then a percent line.
        let mut e = vec![0u8; 0x60];
        e[0x00..0x0A].copy_from_slice(&[0xC1, 0x00, b'\'', b's', b' ', b'l', b'v', b'l', b'!', 0]);
        e[0x0C..0x14].copy_from_slice(b"card %d\0");
        e[0x14..0x1C].copy_from_slice(b"no_%s\n\0\0");
        e[0x1C..0x2A].copy_from_slice(b"\x2c\x08\x1e\x80Old Name\0\0");
        e[0x2C..0x3A].copy_from_slice(b"MP down 20%\0\0\0");
        let pool = UiStringPool {
            prot_index: 0,
            base_va: 0x1000,
            va_start: 0x1000,
            va_end: 0x1040,
            label: "t",
            strict: true,
        };
        let got: Vec<(u32, Vec<u8>)> = scan_pool(&e, &pool)
            .into_iter()
            .map(|s| (s.va, s.bytes))
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    0x1000,
                    vec![0xC1, 0x00, b'\'', b's', b' ', b'l', b'v', b'l', b'!']
                ),
                (0x1020, b"Old Name".to_vec()),
                (0x102C, b"MP down 20%".to_vec()),
            ]
        );
        assert_eq!(pool_strlen(&e, 0, true), Some(9));
        assert_eq!(pool_strlen(&e, 0, false), Some(1));
    }

    #[test]
    fn writable_span_walks_zero_padding_only() {
        // "Hi\0" at 0, then a zero pad byte, then a non-zero at 0x04.
        let entry = [b'H', b'i', 0, 0, b'X', 0, 0, 0];
        assert_eq!(writable_span(&entry, 0, 2), 3);
    }
}
