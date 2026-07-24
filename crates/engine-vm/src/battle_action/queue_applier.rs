//! Byte-level ports of the retail action-queue kernels that make up the tail
//! of the arts queue-builder `FUN_801EED1C` (battle overlay, PROT 0898).
//!
//! PORT: FUN_801EF9E4, FUN_801DA34C, FUN_801EFBFC, FUN_801E91E8
//!
//! These are the *byte-exact* forms operating on the raw 19-byte per-actor
//! queue at `actor[+0x1DF..+0x1F2]` and the resident trigger tables
//! (Miracle `0x801F64F4`, 16-byte stride; Super `find` `0x801F6524`, 13-byte
//! stride; Super `replace` `0x801F65E8`, 16-byte stride) - the shapes
//! `docs/subsystems/battle-action.md` ("The retail queue-builder and Super
//! applier") pins from the disassembly.
//!
//! [`resolve_action_queue`](super::resolve_action_queue) - the entry point the
//! engine's battle layer calls once per committed arts input - runs these
//! appliers in retail's own order, so the live path is byte-level, not
//! structural. The retail finish order, read off the `FUN_801EED1C`
//! disassembly, is:
//!
//! 1. the per-command build loop, which on a Miracle-armed slot
//!    (`ctx[+0x25F + slot] != 0`) overwrites `queue[0..16]` from the resident
//!    Miracle row ([`apply_miracle_replace`], block `0x801EF4E8`);
//! 2. the MSB-clear sweep over `queue[0..16]` ([`clear_queue_msb`], block
//!    `0x801EF85C`) - this is what strips the on-disc `0x8C..0x8F` quirk off
//!    the Miracle row's leading direction bytes;
//! 3. the marked-starter reorder at `0x801EF8A0` (**not** ported - see the
//!    module's `NOT PORTED` note below);
//! 4. `jal 0x801EF9E4` ([`apply_super_tail_replace`]) at `0x801EF9AC`,
//!    unconditionally, with `a0` = actor slot and `a1` = the roster char id
//!    minus one.
//!
//! `legaia_art`'s structural [`MiracleMatcher`] / [`SuperMatcher`] remain the
//! *table* source - [`miracle_row_for`] / [`super_rows_for`] project the
//! modeled tables into the resident row shapes the appliers index - but the
//! queue arithmetic on the live path is now retail's.
//!
//! [`MiracleMatcher`]: legaia_art::MiracleMatcher
//! [`SuperMatcher`]: legaia_art::SuperMatcher
//!
//! # NOT PORTED
//!
//! `FUN_801EED1C`'s marked-starter reorder (`0x801EF8A0..0x801EF968`) is not
//! ported. It walks `0x801F6990[i]`, and for every marked index `i > 0` whose
//! queue byte is a `SpecialStarter` (`0x1A`) it scans `j < i` and swaps
//! `queue[j]` with `queue[i]` wherever `queue[j + 1] == queue[i + 1]` (no
//! early exit, so the swap can run more than once per `i`). The marks it reads
//! are written by the *build* loop, which this module does not port - the
//! marks [`apply_super_tail_replace`] writes are consumed later, after this
//! pass has already run. Porting the reorder without the build loop would add
//! a second inert routine, so it is recorded here instead.

/// Capacity of the per-actor action-parameter byte stream
/// (`actor[+0x1DF..+0x1F2]`, 19 bytes). The queue-length scan and the
/// preseed only ever touch the first [`QUEUE_SCAN_LEN`] bytes; a Super
/// `replace` longer than its `find` can legally spill past 16 (e.g. a
/// 16-byte queue tail-matched by an 8-byte `find` with a 10-byte
/// `replace` writes through index 17).
pub const ACTION_QUEUE_CAP: usize = 0x13;

/// The queue-length zero-terminator scan bound (`slti v0,t4,0x10` at
/// `0x801EFA28`) and the preseed copy length.
pub const QUEUE_SCAN_LEN: usize = 0x10;

/// Fixed row shapes of the resident Super trigger tables.
pub const SUPER_FIND_STRIDE: usize = 13;
pub const SUPER_REPLACE_STRIDE: usize = 16;
/// Rows per character in both tables (`slti v0,a1,0x5` at `0x801EFBE0`).
pub const SUPER_ROWS: usize = 5;

/// Stride of the resident Miracle replacement table at `0x801F64F4`
/// (`sll v0,v0,0x4` at `0x801EF504` - one 16-byte row per character, indexed
/// by the roster char id minus one).
pub const MIRACLE_ROW_STRIDE: usize = 16;

/// PORT: FUN_801EED1C (the Miracle applier block `0x801EF4E8..0x801EF528`)
///
/// Overwrite the whole 16-byte queue window from the acting character's
/// resident Miracle row. Retail reaches this block when the slot's Miracle
/// marker `ctx[+0x25F + slot]` is non-zero and the build loop is on its first
/// pass; the copy is a flat 16-iteration `lbu`/`sb` pair over
/// `0x801F64F4 + (char_id - 1)*16`, so the row's zero padding is copied too
/// and everything the build had staged past the Miracle string is erased.
///
/// The bytes are copied **raw** - the leading direction bytes carry the
/// on-disc MSB-set quirk (`0x8C..0x8F`) at this point. [`clear_queue_msb`] is
/// what strips it, several blocks later; the two must run in that order.
///
/// Retail also latches `ctx[+0x28D] = 1` and the shared trigger flag
/// `0x801F696C = 1` here, and (when the character record's `+0x6C0` bit
/// `0x800` is set) moves 4 points from `actor[+0x170]` into `actor[+0x224]`.
/// Those are actor / ctx side effects rather than queue arithmetic, so they
/// stay with the caller.
pub fn apply_miracle_replace(queue: &mut [u8; ACTION_QUEUE_CAP], row: &[u8; MIRACLE_ROW_STRIDE]) {
    queue[..MIRACLE_ROW_STRIDE].copy_from_slice(row);
}

/// PORT: FUN_801EED1C (the MSB-clear sweep `0x801EF85C..0x801EF898`)
///
/// Clear bit 7 of every byte in the 16-byte queue window. Retail does it with
/// a signed load and an *add*, not an AND: `lb v0,0x1df(a0)` /
/// `lbu v1,0x1df(a0)` / `bgez v0, skip` / `addiu v0,v1,0x80` / `sb`. Adding
/// `0x80` to a byte that already has bit 7 set wraps it off, so the two forms
/// agree for every input - the add is transcribed here because it is what the
/// disassembly does.
///
/// This is the runtime half of the on-disc MSB quirk `docs/formats/art-data.md`
/// records for the Miracle strings: the resident Miracle row stores its leading
/// direction bytes as `0x8C..0x8F` and this sweep is what turns them back into
/// `0x0C..0x0F` in the live queue. Nothing else in the queue normally carries
/// bit 7, so the sweep is a no-op on a queue built from plain input.
pub fn clear_queue_msb(queue: &mut [u8; ACTION_QUEUE_CAP]) {
    for b in queue[..QUEUE_SCAN_LEN].iter_mut() {
        if *b & 0x80 != 0 {
            *b = b.wrapping_add(0x80);
        }
    }
}

/// Outcome of [`apply_super_tail_replace`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuperTailReplace {
    /// Matched table row (0..5), in resident-table order.
    pub row: usize,
    /// Queue index the replace copy started at (`len - find_len`).
    pub start: usize,
    /// Bytes written from the `replace` row (0 when the row's replace
    /// string is empty - retail still latches the trigger flag then).
    pub written: usize,
}

/// PORT: FUN_801EF9E4 - the Super Art find -> tail-replace applier the
/// queue-builder `FUN_801EED1C` invokes at its end (`jal` at `0x801EF9AC`).
///
/// `queue` is the actor's raw action-parameter stream; `starter_marks` is
/// the per-token side array at `0x801F6990` (u32 stride). `find_rows` /
/// `replace_rows` are the character's five resident trigger rows
/// (`0x801F6524 + row*13 + char*65` / `0x801F65E8 + row*16 + char*80`),
/// `find` as `[len u8][bytes...]`, `replace` zero-terminated.
///
/// Retail laws mirrored exactly (disassembly
/// `overlay_battle_action_801ef9e4.txt`):
/// - queue length = index of the first zero byte in `queue[0..16]`
///   (16 when none);
/// - rows are scanned **in table order** and the first full tail match
///   wins (`iVar1 = 5` terminates the row loop);
/// - a row is skipped when its `find` is longer than the queue;
/// - on a match the `replace` bytes overwrite the queue from
///   `len - find_len`, **without** re-zero-terminating - the copy stops at
///   the replace string's own terminator;
/// - every written `0x1A` (`SpecialStarter`) marks its queue position in
///   the side array with `4`;
/// - the shared trigger flag (`0x801F696C = 1` in retail) corresponds to
///   `Some(_)` here - it latches even for an empty replace row.
///
/// Returns `None` when no row matches.
pub fn apply_super_tail_replace(
    queue: &mut [u8; ACTION_QUEUE_CAP],
    starter_marks: &mut [u32; ACTION_QUEUE_CAP],
    find_rows: &[[u8; SUPER_FIND_STRIDE]; SUPER_ROWS],
    replace_rows: &[[u8; SUPER_REPLACE_STRIDE]; SUPER_ROWS],
) -> Option<SuperTailReplace> {
    // Queue-length scan: first zero byte in the 16-byte window.
    let len = queue[..QUEUE_SCAN_LEN]
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(QUEUE_SCAN_LEN);

    for row in 0..SUPER_ROWS {
        let find = &find_rows[row];
        let find_len = find[0] as usize;
        if find_len > len {
            continue;
        }
        // Tail compare: queue[len - find_len + j] vs find[1 + j].
        let start = len - find_len;
        let mismatches = (0..find_len)
            .filter(|&j| queue[start + j] != find[1 + j])
            .count();
        if mismatches != 0 {
            continue;
        }
        // Copy the zero-terminated replace string over the tail. The
        // terminator itself is NOT written (retail `do { write } while
        // (next != 0)` shape), and each written SpecialStarter (0x1A)
        // marks the side array.
        let replace = &replace_rows[row];
        let mut written = 0usize;
        if replace[0] != 0 {
            for (k, &b) in replace.iter().enumerate() {
                if b == 0 {
                    break;
                }
                queue[start + k] = b;
                if b == 0x1A {
                    starter_marks[start + k] = 4;
                }
                written = k + 1;
            }
        }
        return Some(SuperTailReplace {
            row,
            start,
            written,
        });
    }
    None
}

/// PORT: FUN_801DA34C - the round driver's arts-queue preseed (leaf; called
/// from `FUN_801D0748` at `0x801D15C8` / `0x801D1734`).
///
/// NOT WIRED: retail's caller `FUN_801D0748` (the battle command / menu SM) is
/// not ported, and the engine has no per-character saved arts-input string to
/// preseed from - `legaia_save`'s record model carries no `+0x76F` / `+0x77F`
/// chain slot. Wiring needs both.
///
/// Copies one of the character's two saved 16-byte arts-input strings
/// (char record `+0x76F` = first slot, `+0x77F` = second slot, off
/// `0x80084140 + (id-1)*0x414`) byte-for-byte into `queue[0..16]`, or
/// zero-fills the window.
///
/// Selection laws (disassembly `overlay_battle_action_801da34c.txt`):
/// - `staged == false` (the gate byte `DAT_8007BD04` is zero): zero-fill.
/// - `prefer_second == false` (`actor.u16[+0x156] < actor.u16[+0x154]`):
///   copy the **first** slot if its head byte is non-zero, else fall back
///   to the second slot, else zero-fill.
/// - `prefer_second == true` (`actor.u16[+0x156] >= actor.u16[+0x154]`):
///   copy the **second** slot if its head byte is non-zero, else
///   zero-fill. **There is no fallback to the first slot on this leg** -
///   the two branches are asymmetric in the retail body (the
///   `0x801DA4A8` block's empty-head exit at `0x801DA51C` is the
///   zero-fill loop, not the `+0x76F` copy).
pub fn preseed_action_queue(
    queue: &mut [u8; ACTION_QUEUE_CAP],
    staged: bool,
    prefer_second: bool,
    chain_first: &[u8; QUEUE_SCAN_LEN],
    chain_second: &[u8; QUEUE_SCAN_LEN],
) {
    let src: Option<&[u8; QUEUE_SCAN_LEN]> = if !staged {
        None
    } else if !prefer_second {
        if chain_first[0] != 0 {
            Some(chain_first)
        } else if chain_second[0] != 0 {
            Some(chain_second)
        } else {
            None
        }
    } else if chain_second[0] != 0 {
        Some(chain_second)
    } else {
        None
    };
    match src {
        Some(chain) => queue[..QUEUE_SCAN_LEN].copy_from_slice(chain),
        None => queue[..QUEUE_SCAN_LEN].fill(0),
    }
}

/// NOT WIRED: same prerequisite as [`preseed_action_queue`] - no engine-side
/// `+0x76F` / `+0x77F` chain slot to write back into.
///
/// PORT: FUN_801DA59C - the write-back twin of [`preseed_action_queue`]:
/// saves the actor's executed 16-byte arts-input string back into the
/// character record so the next battle's preseed can replay it.
///
/// Retail walks one actor slot (`&DAT_801C9370 + slot`): a dead/empty actor
/// (`+0x14C == 0`) or a non-arts action category (`+0x1DE != 3`) writes
/// nothing. Otherwise the 16 bytes at `actor[+0x1DF..+0x1EF]` are copied
/// into the char record's chain slot - the same `actor.u16[+0x156] <
/// u16[+0x154]` predicate the preseed uses picks the destination (`+0x76F`
/// first slot / `+0x77F` second slot, off `0x80084140 + (id-1)*0x414`;
/// `sb` loops at `0x801DA638` / `0x801DA69C`). Unlike the preseed there is
/// no head-byte fallback: exactly one slot is overwritten.
pub fn save_action_queue(
    queue: &[u8; QUEUE_SCAN_LEN],
    alive: bool,
    category_is_arts: bool,
    prefer_second: bool,
    chain_first: &mut [u8; QUEUE_SCAN_LEN],
    chain_second: &mut [u8; QUEUE_SCAN_LEN],
) -> bool {
    if !alive || !category_is_arts {
        return false;
    }
    if prefer_second {
        chain_second.copy_from_slice(queue);
    } else {
        chain_first.copy_from_slice(queue);
    }
    true
}

/// Result band of [`check_and_learn_art`] (the retail return register).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtUseCheck {
    /// `0` - not in the learned list and not learnable now.
    Unknown,
    /// `1` - already in the learned list.
    Known,
    /// `2` - was unknown and has just been inserted (learn-on-use).
    Learned,
}

/// PORT: FUN_801EFBFC - learned-art membership check **plus learn-on-use
/// insert** (called from the queue-builder at `0x801EF44C` per accepted
/// art).
///
/// NOT WIRED: the engine models learned arts as the save-ext
/// `learned_arts_mask` bitmask, not as the retail `+0x74D` count / `+0x74E`
/// ascending id list, and [`resolve_action_queue`](super::resolve_action_queue)
/// is handed already-recognized arts rather than the raw per-command stream
/// this check runs inside. Wiring it means the caller carrying the char
/// record's arts list plus the per-character innate cap at
/// `0x801F686C + char_id - 1`.
///
/// The character's learned-arts list lives in the char record: count at
/// `+0x74D`, ascending-sorted ids at `+0x74E..`. Laws (disassembly
/// `overlay_battle_action_801efbfc.txt`):
/// - membership scan first; a hit returns [`ArtUseCheck::Known`] and the
///   learn leg never runs;
/// - the learn leg runs only when `learn_gate_open` - retail:
///   `actor[+0x266] == 0` **or** `rand() & 0x1FF == 0` (a 1/512 roll)
///   **or** the debug byte `DAT_8007BD0C == 'O'`;
/// - an unknown art is inserted only when `art_id > innate_cap` (the
///   per-character byte at `0x801F686C + char_id - 1`) **or**
///   `art_id == 0` (a retail edge: the zero id passes the cap gate);
/// - the insert keeps the list ascending-sorted: entries greater than
///   `art_id` shift up one slot, then the id lands in the gap and the
///   count increments; returns [`ArtUseCheck::Learned`].
///
/// Defensive deviation: retail has no bound on the shift loop (a full
/// 16-entry list would spill into the equip bytes at `+0x75E`); this port
/// refuses the insert when the list is full and returns
/// [`ArtUseCheck::Unknown`].
pub fn check_and_learn_art(
    count: &mut u8,
    ids: &mut [u8; 16],
    art_id: u8,
    learn_gate_open: bool,
    innate_cap: u8,
) -> ArtUseCheck {
    let n = (*count as usize).min(ids.len());
    if ids[..n].contains(&art_id) {
        return ArtUseCheck::Known;
    }
    if !learn_gate_open {
        return ArtUseCheck::Unknown;
    }
    if art_id != 0 && art_id <= innate_cap {
        return ArtUseCheck::Unknown;
    }
    if n >= ids.len() {
        // Deviation from retail (which would overflow into +0x75E).
        return ArtUseCheck::Unknown;
    }
    // Ascending sorted insert: shift entries > art_id up by one.
    let mut slot = n;
    while slot > 0 && ids[slot - 1] > art_id {
        ids[slot] = ids[slot - 1];
        slot -= 1;
    }
    ids[slot] = art_id;
    *count += 1;
    ArtUseCheck::Learned
}

/// NOT WIRED: `FUN_801E91E8` is **not** called by the queue-builder
/// `FUN_801EED1C` (whose only `jal`s are `0x80056798`, `0x801EFBFC` and
/// `0x801EF9E4`); it belongs to the per-keypress input recognizer that arms
/// the slot's Miracle marker `ctx[+0x25F + slot]`. The engine's Miracle gate is
/// the whole-string match in
/// [`resolve_action_queue`](super::resolve_action_queue), so the per-token
/// position has no consumer until that recognizer is ported.
///
/// PORT: FUN_801E91E8 - Miracle-command token position lookup.
///
/// Maps an input token to its 1-based position in the character's
/// MSB-masked Miracle command string (char record count `+0x704`, bytes
/// `+0x705..`, each stored as `value + 0x80` - the on-disc MSB-set quirk
/// `docs/formats/art-data.md` documents for the Miracle strings).
///
/// Laws (disassembly `overlay_battle_action_801e91e8.txt`):
/// - when the lookup is not applicable - retail: acting slot `>= 3`
///   (non-player), the slot's Miracle marker `ctx[+0x25F + slot]` clear,
///   or the global `_DAT_8007BAC0` non-zero - the function returns `1`
///   unconditionally (`miracle_pending == false` here);
/// - otherwise the first `i` with `token == cmds[i] - 0x80` returns
///   `i + 1` (truncated to u8);
/// - no match (or an empty string) returns `0`.
pub fn miracle_command_position(token: u8, miracle_pending: bool, cmds_msb: &[u8]) -> u8 {
    if !miracle_pending {
        return 1;
    }
    for (i, &c) in cmds_msb.iter().enumerate() {
        if token == c.wrapping_sub(0x80) {
            return (i as u8).wrapping_add(1);
        }
    }
    0
}

/// Build a character's resident-table-shaped Miracle row from the modeled
/// `legaia_art` table: the replacement action constants, zero-padded to
/// [`MIRACLE_ROW_STRIDE`], with the leading run of direction constants
/// (`0x0C..=0x0F`) MSB-set to mirror the on-disc `0x8C..0x8F` form the
/// resident row stores and [`clear_queue_msb`] strips.
///
/// This is the bridge that lets [`apply_miracle_replace`] run against the
/// modeled table; keeping the row in its on-disc form is what makes the
/// MSB-clear sweep observable instead of vacuous.
pub fn miracle_row_for(character: legaia_art::Character) -> [u8; MIRACLE_ROW_STRIDE] {
    let mut row = [0u8; MIRACLE_ROW_STRIDE];
    let Some(art) = legaia_art::MIRACLE_ARTS
        .iter()
        .find(|m| m.character == character)
    else {
        return row;
    };
    let mut leading = true;
    for (slot, action) in row.iter_mut().zip(art.replacement.iter()) {
        let b = action.as_byte();
        // The MSB quirk covers the row's leading direction bytes only; the
        // starter + art constants that follow are stored plain.
        if leading && (0x0C..=0x0F).contains(&b) {
            *slot = b | 0x80;
        } else {
            leading = false;
            *slot = b;
        }
    }
    row
}

/// Build a character's five resident-table-shaped Super rows from the
/// modeled `legaia_art` table: `find` as `[len][bytes][zero pad]` at the
/// 13-byte stride, `replace` zero-padded at the 16-byte stride, in
/// `SUPER_ARTS` order (the capture-validated resident order).
///
/// This is the bridge between the byte-exact applier and the structural
/// matcher - the shipped tables have exactly [`SUPER_ROWS`] entries per
/// character.
pub fn super_rows_for(
    character: legaia_art::Character,
) -> (
    [[u8; SUPER_FIND_STRIDE]; SUPER_ROWS],
    [[u8; SUPER_REPLACE_STRIDE]; SUPER_ROWS],
) {
    let mut find_rows = [[0u8; SUPER_FIND_STRIDE]; SUPER_ROWS];
    let mut replace_rows = [[0u8; SUPER_REPLACE_STRIDE]; SUPER_ROWS];
    let mut row = 0usize;
    for sa in legaia_art::SUPER_ARTS {
        if sa.character != character {
            continue;
        }
        assert!(
            row < SUPER_ROWS,
            "more than {SUPER_ROWS} supers per character"
        );
        assert!(sa.find.len() < SUPER_FIND_STRIDE);
        assert!(sa.replace.len() <= SUPER_REPLACE_STRIDE);
        find_rows[row][0] = sa.find.len() as u8;
        find_rows[row][1..1 + sa.find.len()].copy_from_slice(sa.find);
        replace_rows[row][..sa.replace.len()].copy_from_slice(sa.replace);
        row += 1;
    }
    assert_eq!(
        row, SUPER_ROWS,
        "expected {SUPER_ROWS} supers per character"
    );
    (find_rows, replace_rows)
}

#[cfg(test)]
mod queue_applier_tests {
    use super::*;
    use legaia_art::Character;

    fn queue_from(bytes: &[u8]) -> [u8; ACTION_QUEUE_CAP] {
        let mut q = [0u8; ACTION_QUEUE_CAP];
        q[..bytes.len()].copy_from_slice(bytes);
        q
    }

    #[test]
    fn super_applier_replaces_tail_for_every_shipped_super() {
        for ch in [Character::Vahn, Character::Noa, Character::Gala] {
            let (find_rows, replace_rows) = super_rows_for(ch);
            for (row, sa) in legaia_art::SUPER_ARTS
                .iter()
                .filter(|s| s.character == ch)
                .enumerate()
            {
                // Queue = exactly the find pattern.
                let mut q = queue_from(sa.find);
                let mut marks = [0u32; ACTION_QUEUE_CAP];
                let hit = apply_super_tail_replace(&mut q, &mut marks, &find_rows, &replace_rows)
                    .unwrap_or_else(|| panic!("{} did not match", sa.name));
                assert_eq!(hit.row, row, "{}", sa.name);
                assert_eq!(hit.start, 0, "{}", sa.name);
                assert_eq!(hit.written, sa.replace.len(), "{}", sa.name);
                assert_eq!(&q[..sa.replace.len()], sa.replace, "{}", sa.name);
                // Every written SpecialStarter is marked with 4.
                for (i, &b) in sa.replace.iter().enumerate() {
                    if b == 0x1A {
                        assert_eq!(marks[i], 4, "{} starter mark at {i}", sa.name);
                    }
                }
            }
        }
    }

    #[test]
    fn super_applier_matches_at_tail_with_prefix_preserved() {
        // Vahn's Tri-Somersault behind a two-byte prefix: the prefix
        // survives and the replace lands at the tail.
        let (find_rows, replace_rows) = super_rows_for(Character::Vahn);
        let tri = legaia_art::SUPER_ARTS
            .iter()
            .find(|s| s.name == "Tri-Somersault")
            .unwrap();
        let mut bytes = vec![0x1B, 0x1C];
        bytes.extend_from_slice(tri.find);
        let mut q = queue_from(&bytes);
        let mut marks = [0u32; ACTION_QUEUE_CAP];
        let hit = apply_super_tail_replace(&mut q, &mut marks, &find_rows, &replace_rows).unwrap();
        assert_eq!(hit.start, 2);
        assert_eq!(&q[..2], &[0x1B, 0x1C]);
        assert_eq!(&q[2..2 + tri.replace.len()], tri.replace);
    }

    #[test]
    fn super_applier_no_match_leaves_queue_untouched() {
        let (find_rows, replace_rows) = super_rows_for(Character::Vahn);
        let mut q = queue_from(&[0x1B, 0x1C, 0x1D]);
        let before = q;
        let mut marks = [0u32; ACTION_QUEUE_CAP];
        assert_eq!(
            apply_super_tail_replace(&mut q, &mut marks, &find_rows, &replace_rows),
            None
        );
        assert_eq!(q, before);
        assert_eq!(marks, [0u32; ACTION_QUEUE_CAP]);
    }

    #[test]
    fn super_applier_agrees_with_structural_matcher() {
        // The byte applier and legaia_art's SuperMatcher must produce the
        // same queue for a matching tail.
        use legaia_art::{ActionConstant, ActionQueue, SuperMatcher};
        let matcher = SuperMatcher::with_default_table();
        for sa in legaia_art::SUPER_ARTS {
            let (find_rows, replace_rows) = super_rows_for(sa.character);
            let mut q = queue_from(sa.find);
            let mut marks = [0u32; ACTION_QUEUE_CAP];
            apply_super_tail_replace(&mut q, &mut marks, &find_rows, &replace_rows).unwrap();

            let mut structural = ActionQueue::new();
            for &b in sa.find {
                structural.push(ActionConstant::from_byte(b).unwrap());
            }
            matcher
                .try_trigger_at_tail(sa.character, &mut structural)
                .unwrap();
            let structural_bytes: Vec<u8> =
                structural.actions().iter().map(|a| a.as_byte()).collect();
            assert_eq!(
                &q[..structural_bytes.len()],
                structural_bytes.as_slice(),
                "{}",
                sa.name
            );
        }
    }

    #[test]
    fn preseed_copies_first_slot_when_preferred_and_nonempty() {
        let a = [7u8; QUEUE_SCAN_LEN];
        let b = [9u8; QUEUE_SCAN_LEN];
        let mut q = [0xFFu8; ACTION_QUEUE_CAP];
        preseed_action_queue(&mut q, true, false, &a, &b);
        assert_eq!(&q[..QUEUE_SCAN_LEN], &a);
        // Bytes past the 16-byte window are untouched.
        assert_eq!(q[QUEUE_SCAN_LEN], 0xFF);
    }

    #[test]
    fn preseed_first_slot_falls_back_to_second() {
        let a = [0u8; QUEUE_SCAN_LEN];
        let b = [9u8; QUEUE_SCAN_LEN];
        let mut q = [0xFFu8; ACTION_QUEUE_CAP];
        preseed_action_queue(&mut q, true, false, &a, &b);
        assert_eq!(&q[..QUEUE_SCAN_LEN], &b);
    }

    #[test]
    fn preseed_second_slot_leg_has_no_fallback() {
        // The retail asymmetry: prefer_second with an empty second slot
        // zero-fills even when the first slot is populated.
        let a = [7u8; QUEUE_SCAN_LEN];
        let b = [0u8; QUEUE_SCAN_LEN];
        let mut q = [0xFFu8; ACTION_QUEUE_CAP];
        preseed_action_queue(&mut q, true, true, &a, &b);
        assert_eq!(&q[..QUEUE_SCAN_LEN], &[0u8; QUEUE_SCAN_LEN]);
    }

    #[test]
    fn preseed_unstaged_zero_fills() {
        let a = [7u8; QUEUE_SCAN_LEN];
        let b = [9u8; QUEUE_SCAN_LEN];
        let mut q = [0xFFu8; ACTION_QUEUE_CAP];
        preseed_action_queue(&mut q, false, false, &a, &b);
        assert_eq!(&q[..QUEUE_SCAN_LEN], &[0u8; QUEUE_SCAN_LEN]);
    }

    #[test]
    fn save_action_queue_writes_exactly_one_slot() {
        let q = [3u8; QUEUE_SCAN_LEN];
        let mut a = [0u8; QUEUE_SCAN_LEN];
        let mut b = [0u8; QUEUE_SCAN_LEN];
        // First-slot leg (+0x156 < +0x154).
        assert!(save_action_queue(&q, true, true, false, &mut a, &mut b));
        assert_eq!(a, q);
        assert_eq!(b, [0u8; QUEUE_SCAN_LEN]);
        // Second-slot leg.
        let mut a2 = [0u8; QUEUE_SCAN_LEN];
        assert!(save_action_queue(&q, true, true, true, &mut a2, &mut b));
        assert_eq!(b, q);
        assert_eq!(a2, [0u8; QUEUE_SCAN_LEN]);
        // Guards: dead actor / non-arts category write nothing.
        let mut c = [0u8; QUEUE_SCAN_LEN];
        assert!(!save_action_queue(&q, false, true, false, &mut c, &mut b));
        assert!(!save_action_queue(&q, true, false, false, &mut c, &mut b));
        assert_eq!(c, [0u8; QUEUE_SCAN_LEN]);
    }

    #[test]
    fn learn_art_known_id_short_circuits() {
        let mut count = 3u8;
        let mut ids = [0u8; 16];
        ids[..3].copy_from_slice(&[0x22, 0x28, 0x30]);
        assert_eq!(
            check_and_learn_art(&mut count, &mut ids, 0x28, true, 0x20),
            ArtUseCheck::Known
        );
        assert_eq!(count, 3);
    }

    #[test]
    fn learn_art_inserts_sorted_above_cap() {
        let mut count = 3u8;
        let mut ids = [0u8; 16];
        ids[..3].copy_from_slice(&[0x22, 0x28, 0x30]);
        assert_eq!(
            check_and_learn_art(&mut count, &mut ids, 0x2B, true, 0x20),
            ArtUseCheck::Learned
        );
        assert_eq!(count, 4);
        assert_eq!(&ids[..4], &[0x22, 0x28, 0x2B, 0x30]);
    }

    #[test]
    fn learn_art_gate_closed_returns_unknown() {
        let mut count = 1u8;
        let mut ids = [0u8; 16];
        ids[0] = 0x22;
        assert_eq!(
            check_and_learn_art(&mut count, &mut ids, 0x2B, false, 0x20),
            ArtUseCheck::Unknown
        );
        assert_eq!(count, 1);
    }

    #[test]
    fn learn_art_at_or_below_cap_is_not_learnable() {
        let mut count = 0u8;
        let mut ids = [0u8; 16];
        assert_eq!(
            check_and_learn_art(&mut count, &mut ids, 0x1B, true, 0x20),
            ArtUseCheck::Unknown
        );
        // The retail zero-id edge passes the cap gate.
        assert_eq!(
            check_and_learn_art(&mut count, &mut ids, 0, true, 0x20),
            ArtUseCheck::Learned
        );
        assert_eq!(count, 1);
    }

    #[test]
    fn learn_art_full_list_refuses_insert() {
        let mut count = 16u8;
        let mut ids = [0u8; 16];
        for (i, id) in ids.iter_mut().enumerate() {
            *id = 0x20 + i as u8;
        }
        assert_eq!(
            check_and_learn_art(&mut count, &mut ids, 0x60, true, 0x10),
            ArtUseCheck::Unknown
        );
        assert_eq!(count, 16);
    }

    #[test]
    fn miracle_position_bypass_and_lookup() {
        // Not applicable -> unconditional 1.
        assert_eq!(miracle_command_position(0x0C, false, &[0x8C, 0x8D]), 1);
        // Match -> 1-based position of the masked byte.
        assert_eq!(miracle_command_position(0x0D, true, &[0x8C, 0x8D, 0x8E]), 2);
        // Absent (or empty string) -> 0.
        assert_eq!(miracle_command_position(0x0F, true, &[0x8C, 0x8D, 0x8E]), 0);
        assert_eq!(miracle_command_position(0x0F, true, &[]), 0);
    }
}
