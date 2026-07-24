//! Dev-menu (overlay 0897) EVENT FLAG editor - the simulation half.
//!
//! The retail debug build ships a developer menu resident in overlay 0897
//! (the dev-menu toolset: warp appliers + this flag editor). Its EVENT
//! FLAG editor lets a developer step a raw flag index / value and poke
//! story-flag bits directly; the captured save states that once looked
//! like a mysterious `_DAT_8007BA78` writer came from exactly this tool
//! (see `docs/reference/re-settled-threads.md`, the "STR trigger
//! teleports + sets flags" row, which cites `FUN_801dbd04`).
//!
//! ## What lives here vs. what is a seam
//!
//! The worklist rows this module was asked to port are all VA-aliased
//! **slices of one giant dev-menu dispatcher**, not independent functions.
//! Three carry portable game-state logic:
//!
//! - `FUN_801dbd04` - the value-adjust arithmetic for the edited flag
//!   index/value `DAT_801f2aa0` ([`edit_flag_value`]).
//! - `FUN_801db8f4` - the flag-list cursor `DAT_801f2e90` decrement with
//!   the `'X'`-sentinel wrap-to-bottom search ([`flag_list_prev`]).
//! - `FUN_801db8b4` - the same cursor's increment with the `'X'`-sentinel
//!   wrap-to-top ([`flag_list_next`]).
//!
//! The other two are decompiler fragments of the same dispatcher and are
//! **not** standalone ports (documented as `// REF:` below):
//!
//! - `FUN_801d3444` is `addiu s8,s8,0x10; j 0x801e3624` - the PC-delta
//!   "advance the menu-entry pointer by one 0x10-byte stride, then fall to
//!   the shared redraw tail" exit idiom (a label, not a call; see
//!   CLAUDE.md's label-call note). No game state; nothing to port.
//! - `FUN_801d9bbc` is a 6-instruction interior slice
//!   `func(0x801cf1ec, s3, s5+0x10)` - a formatted print of one menu row
//!   (a text/draw concern). Row text emission belongs to `engine-ui`, so
//!   it is left as a documented draw seam, not ported into `engine-core`.
//!
//! ## Pad-bit layout
//!
//! The dev overlay reads the **packed** pad words `_DAT_8007bb84` (edge)
//! and `_DAT_8007b850` (held), built by the pad-mask packer `FUN_8001822C`
//! from pad-1's low 16 bits. That packing is **not** the raw-BIOS
//! [`crate::input::PadButton`] layout - it is the fishing/tile-board/dance
//! layout: `0x10` Triangle, `0x20` Circle, `0x40` Cross, `0x80` Square,
//! `0x1000` Up, `0x2000` Right, `0x4000` Down, `0x8000` Left. The
//! [`PACK_*`](PACK_UP) constants below are that layout; do not substitute
//! `PadButton::*.mask()` here.
//!
//! Wired: [`crate::dev_menu_host::DevMenuSession`]'s `EVENT_FLAG` page
//! drives all three kernels. The **flag-list table** is still absent - it is
//! an overlay-0897 debug asset (`DAT_801f2e94`, stride `0xA` with the `'X'`
//! sentinel) that the engine never loads, so `DevMenuSession::flag_tags`
//! starts empty and the list cursor has nothing to walk until a host
//! supplies it. The raw value editor works without it.

/// Packed-pad Triangle (`_DAT_8007b850 & 0x10` = the coarse-step modifier).
pub const PACK_TRIANGLE: u16 = 0x0010;
/// Packed-pad Circle.
pub const PACK_CIRCLE: u16 = 0x0020;
/// Packed-pad Cross.
pub const PACK_CROSS: u16 = 0x0040;
/// Packed-pad Square.
pub const PACK_SQUARE: u16 = 0x0080;
/// Packed-pad Up.
pub const PACK_UP: u16 = 0x1000;
/// Packed-pad Right.
pub const PACK_RIGHT: u16 = 0x2000;
/// Packed-pad Down.
pub const PACK_DOWN: u16 = 0x4000;
/// Packed-pad Left.
pub const PACK_LEFT: u16 = 0x8000;

/// Fine value step (Up/Down without the Triangle modifier).
pub const FLAG_STEP_FINE: i32 = 0x8;
/// Coarse value step (Up/Down while Triangle is held).
pub const FLAG_STEP_COARSE: i32 = 0x80;
/// The edited value clamps to `[0, 0xFFF]` - 4096 addressable flags.
pub const FLAG_VALUE_MAX: i32 = 0xFFF;

/// Per-entry stride of the flag-list table at `DAT_801f2e94` (0xA bytes).
pub const FLAG_ENTRY_STRIDE: usize = 0xA;
/// End-of-list sentinel: byte `+2` of a table entry equals `'X'` (0x58).
pub const FLAG_LIST_END_TAG: u8 = 0x58;

/// Step the edited flag index/value `DAT_801f2aa0` by one pad edge.
///
/// Faithful to the disassembly at `0x801dbd04`
/// (`ghidra/scripts/funcs/overlay_0897_801dbd04.txt`): Up/Down move by the
/// step size (fine `0x8`, or coarse `0x80` while Triangle is held), Left
/// nudges by `-1`, Right by `+1`, then the value clamps to `[0, 0xFFF]`.
///
/// The retail slice makes coarse-Down branch straight to a redraw tail
/// (`j 0x801ea5c4`) instead of also applying the Left/Right `+/-1` in the
/// same frame; that asymmetry is a slice/redraw-dispatch detail (only one
/// d-pad edge fires per frame in practice), so the port applies the four
/// deltas additively - the resulting game state is identical.
// PORT: FUN_801dbd04
pub fn edit_flag_value(value: i32, pad_edge: u16, pad_held: u16) -> i32 {
    let step = if pad_held & PACK_TRIANGLE != 0 {
        FLAG_STEP_COARSE
    } else {
        FLAG_STEP_FINE
    };
    let mut v = value;
    if pad_edge & PACK_UP != 0 {
        v -= step;
    }
    if pad_edge & PACK_DOWN != 0 {
        v += step;
    }
    if pad_edge & PACK_RIGHT != 0 {
        v += 1;
    }
    if pad_edge & PACK_LEFT != 0 {
        v -= 1;
    }
    v.clamp(0, FLAG_VALUE_MAX)
}

/// Move the flag-list cursor `DAT_801f2e90` to the previous entry.
///
/// Faithful to the disassembly at `0x801db8f4`
/// (`ghidra/scripts/funcs/overlay_0897_801db8f4.txt`). `tags` is the
/// per-entry projection of the stride-`0xA` table at `DAT_801f2e94`: one
/// byte per entry = that entry's `+2` byte, where [`FLAG_LIST_END_TAG`]
/// (`'X'`) marks the end of the list.
///
/// - `cursor > 0`: plain decrement (`cursor - 1`).
/// - `cursor <= 0`: wrap - scan forward from `cursor` for the first entry
///   carrying the `'X'` end tag and land one before it (the last real
///   entry). An empty list (`tags[0] == 'X'`) yields `-1`, matching the
///   retail pointer arithmetic.
// PORT: FUN_801db8f4
pub fn flag_list_prev(cursor: i32, tags: &[u8]) -> i32 {
    if cursor > 0 {
        return cursor - 1;
    }
    let mut i = cursor.max(0) as usize;
    while i < tags.len() && tags[i] != FLAG_LIST_END_TAG {
        i += 1;
    }
    i as i32 - 1
}

/// Move the flag-list cursor `DAT_801f2e90` to the next entry.
///
/// Faithful to the disassembly at `0x801db8b4`
/// (`ghidra/scripts/funcs/overlay_0897_801db8b4.txt`), the increment
/// sibling of [`flag_list_prev`] on the same cursor + stride-`0xA` table
/// `DAT_801f2e94`. The retail slice unconditionally stores `cursor + 1`,
/// then - if the newly-landed entry carries the `'X'` end tag
/// ([`FLAG_LIST_END_TAG`]) - resets the cursor to `0`:
///
/// ```text
/// 801db8bc  addiu v1,v1,0x1     ; next = cursor + 1
/// 801db8d0  sw    v1,0x2e90(a1) ; cursor = next
/// 801db8d4  lbu   v1,0x2(v0)    ; tag = table[next].byte[2]  (v0 = table + next*0xA)
/// 801db8dc  bne   v1,0x58,...   ; if tag != 'X' keep next
/// 801db8e4  sw    zero,0x2e90(a1) ; else cursor = 0  (wrap to top)
/// ```
///
/// The `'X'` sentinel always sits one past the last real entry, so `next`
/// stays in range for every real cursor; an out-of-range `next` (never
/// reached in retail flow) is treated as a non-sentinel and kept.
// PORT: FUN_801db8b4
pub fn flag_list_next(cursor: i32, tags: &[u8]) -> i32 {
    let next = cursor + 1;
    match tags.get(next as usize) {
        Some(&FLAG_LIST_END_TAG) => 0,
        _ => next,
    }
}

/// The two live editor cursors: the raw flag index/value `DAT_801f2aa0`
/// and the flag-list row cursor `DAT_801f2e90`. A thin state wrapper so
/// hosts can drive [`edit_flag_value`] / [`flag_list_prev`] as a unit; the
/// row-render + text emit (`FUN_801d9bbc`, and the `FUN_801d3444`
/// entry-pointer advance) stay a documented `engine-ui` draw seam.
// REF: FUN_801d3444 (entry-stride advance / PC-delta exit idiom)
// REF: FUN_801d9bbc (menu-row text emit -> engine-ui draw seam)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EventFlagEditor {
    /// Edited flag index/value (`DAT_801f2aa0`, `0..=0xFFF`).
    pub value: i32,
    /// Flag-list row cursor (`DAT_801f2e90`).
    pub list_cursor: i32,
}

impl EventFlagEditor {
    /// Apply one pad edge to the edited value (packed `_DAT_8007bb84` edge
    /// word + `_DAT_8007b850` held word).
    pub fn edit_value(&mut self, pad_edge: u16, pad_held: u16) {
        self.value = edit_flag_value(self.value, pad_edge, pad_held);
    }

    /// Move the list cursor to the previous entry (wrap on the `'X'`
    /// sentinel).
    pub fn list_prev(&mut self, tags: &[u8]) {
        self.list_cursor = flag_list_prev(self.list_cursor, tags);
    }

    /// Move the list cursor to the next entry (wrap to top on the `'X'`
    /// sentinel).
    pub fn list_next(&mut self, tags: &[u8]) {
        self.list_cursor = flag_list_next(self.list_cursor, tags);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fine_step_is_eight() {
        // No Triangle held -> Up/Down move by 0x8.
        assert_eq!(edit_flag_value(0x40, PACK_DOWN, 0), 0x48);
        assert_eq!(edit_flag_value(0x40, PACK_UP, 0), 0x38);
    }

    #[test]
    fn coarse_step_is_0x80_with_triangle() {
        assert_eq!(edit_flag_value(0x200, PACK_DOWN, PACK_TRIANGLE), 0x280);
        assert_eq!(edit_flag_value(0x200, PACK_UP, PACK_TRIANGLE), 0x180);
    }

    #[test]
    fn left_right_nudge_by_one() {
        assert_eq!(edit_flag_value(0x10, PACK_RIGHT, 0), 0x11);
        assert_eq!(edit_flag_value(0x10, PACK_LEFT, 0), 0xF);
        // The +/-1 nudge is unaffected by the Triangle coarse modifier.
        assert_eq!(edit_flag_value(0x10, PACK_RIGHT, PACK_TRIANGLE), 0x11);
    }

    #[test]
    fn value_clamps_to_zero_and_0xfff() {
        assert_eq!(edit_flag_value(0x4, PACK_UP, 0), 0); // 4 - 8 = -4 -> 0
        assert_eq!(edit_flag_value(0, PACK_LEFT, 0), 0); // 0 - 1 -> 0
        assert_eq!(
            edit_flag_value(FLAG_VALUE_MAX, PACK_DOWN, PACK_TRIANGLE),
            FLAG_VALUE_MAX
        );
        assert_eq!(
            edit_flag_value(FLAG_VALUE_MAX, PACK_RIGHT, 0),
            FLAG_VALUE_MAX
        );
    }

    #[test]
    fn combined_edges_apply_additively() {
        // Up (-8) and Right (+1) in one edge -> -7.
        assert_eq!(edit_flag_value(0x20, PACK_UP | PACK_RIGHT, 0), 0x19);
    }

    #[test]
    fn no_edge_is_identity() {
        assert_eq!(edit_flag_value(0x123, 0, 0), 0x123);
        assert_eq!(edit_flag_value(0x123, PACK_CROSS | PACK_CIRCLE, 0), 0x123);
    }

    #[test]
    fn list_prev_plain_decrement() {
        // 'A' = 0x41 entries, 'X' = end. Cursor 3 -> 2.
        let tags = [0x41, 0x41, 0x41, 0x41, FLAG_LIST_END_TAG];
        assert_eq!(flag_list_prev(3, &tags), 2);
        assert_eq!(flag_list_prev(1, &tags), 0);
    }

    #[test]
    fn list_prev_wraps_to_last_real_entry() {
        // Four real entries then the 'X' sentinel at index 4; wrapping from
        // the top lands on the last real entry (index 3).
        let tags = [0x41, 0x41, 0x41, 0x41, FLAG_LIST_END_TAG];
        assert_eq!(flag_list_prev(0, &tags), 3);
    }

    #[test]
    fn list_prev_empty_list_is_minus_one() {
        // Sentinel at index 0 -> the retail pointer arithmetic yields -1.
        let tags = [FLAG_LIST_END_TAG];
        assert_eq!(flag_list_prev(0, &tags), -1);
    }

    #[test]
    fn list_next_plain_increment() {
        // 'A' = 0x41 entries, 'X' = end at index 4. Cursor 1 -> 2.
        let tags = [0x41, 0x41, 0x41, 0x41, FLAG_LIST_END_TAG];
        assert_eq!(flag_list_next(1, &tags), 2);
        assert_eq!(flag_list_next(0, &tags), 1);
    }

    #[test]
    fn list_next_wraps_to_top_on_sentinel() {
        // From the last real entry (index 3), the next slot (index 4) is
        // the 'X' sentinel, so the cursor wraps back to 0.
        let tags = [0x41, 0x41, 0x41, 0x41, FLAG_LIST_END_TAG];
        assert_eq!(flag_list_next(3, &tags), 0);
    }

    #[test]
    fn list_next_single_entry_wraps() {
        // One real entry then the sentinel: next always wraps to 0.
        let tags = [0x41, FLAG_LIST_END_TAG];
        assert_eq!(flag_list_next(0, &tags), 0);
    }

    #[test]
    fn list_prev_next_round_trip() {
        // prev then next (and vice-versa) returns to the start away from
        // the wrap edges.
        let tags = [0x41, 0x41, 0x41, 0x41, FLAG_LIST_END_TAG];
        assert_eq!(flag_list_next(flag_list_prev(2, &tags), &tags), 2);
        assert_eq!(flag_list_prev(flag_list_next(1, &tags), &tags), 1);
    }

    #[test]
    fn editor_state_wrapper_drives_both_cursors() {
        let mut ed = EventFlagEditor {
            value: 0x100,
            list_cursor: 2,
        };
        ed.edit_value(PACK_DOWN, PACK_TRIANGLE);
        assert_eq!(ed.value, 0x180);
        let tags = [0x41, 0x41, 0x41, FLAG_LIST_END_TAG];
        ed.list_prev(&tags);
        assert_eq!(ed.list_cursor, 1);
        // Wrap from the top.
        ed.list_cursor = 0;
        ed.list_prev(&tags);
        assert_eq!(ed.list_cursor, 2);
        // list_next steps forward and wraps to top off the last entry.
        ed.list_next(&tags);
        assert_eq!(ed.list_cursor, 0);
    }
}
