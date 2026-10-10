//! The shared list-cursor navigation kernel both list windows page with.

use super::*;

/// Rows per list page (both retail list windows show 12 rows filling the
/// 182-px content height at the 0xE pitch).
pub const LIST_PAGE_ROWS: usize = 12;

/// The retail list-window pad decode - the SCUS kind-4 list kernel
/// `FUN_80032A44`'s navigation phase, in flat-cursor form (the kernel
/// keeps `scroll top` (`node+0x0`) and `selected` (`node+0x6`)
/// separately; page starts stay `LIST_PAGE_ROWS`-aligned under these
/// moves, so `top = cursor - cursor % ROWS` is an invariant):
///
/// - **Up** (held `0x1000`, `80032ae8..80032c74`): selection `-1` while
///   above the page top; at the page top it wraps to the page's last
///   row (`80032b28`: `sel = top + visible - 1`, clamped to the row
///   count at `80032c5c..80032c6c`).
/// - **Down** (`0x4000`, `80032b44..80032b84`): selection `+1`; stepping
///   past the page bottom (`sel+1 == top+visible`, `80032b68`) or past
///   the last row (`sel+1 == count`, `80032b78` fallthrough) wraps back
///   to the page top (`80032b80` restores `node+0x0`).
/// - **Left** (`0x8000`, `80032b90..80032c0c`): page up - only while
///   `top > 0`; both top and selection step back one page.
/// - **Right** (`0x2000`, `80032c1c..80032c50`): page down - only while
///   `top + visible < count`; selection clamps to the last row.
///
/// Up/Down never scroll - the only scrolling is the Left/Right page
/// flip, which is why the retail lists read as fixed 12-row pages.
///
/// PORT: FUN_80032A44 (kind-4 list kernel - navigation phase)
pub fn list_kernel_navigate(cursor: usize, n: usize, pressed: u16) -> usize {
    list_kernel_navigate_rows(cursor, n, pressed, LIST_PAGE_ROWS)
}

/// [`list_kernel_navigate`] for a list window whose page holds `rows` rows -
/// the kernel's `visible = (content_h - 4) / 0xE` for that window (12 for
/// the pause lists, 7 for the shop's buy list, 11 for its sell list).
///
/// PORT: FUN_80032A44 (kind-4 list kernel - navigation phase)
pub fn list_kernel_navigate_rows(cursor: usize, n: usize, pressed: u16, rows: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let rows = rows.max(1);
    let mut c = cursor.min(n - 1);
    let top = c - c % rows;
    if pressed & PadButton::Up.mask() != 0 {
        c = if c > top {
            c - 1
        } else {
            (top + rows).min(n) - 1
        };
    }
    if pressed & PadButton::Down.mask() != 0 {
        let top = c - c % rows;
        c = if (c + 1).is_multiple_of(rows) || c + 1 == n {
            top
        } else {
            c + 1
        };
    }
    if pressed & PadButton::Left.mask() != 0 {
        let top = c - c % rows;
        if top > 0 {
            c -= rows;
        }
    }
    if pressed & PadButton::Right.mask() != 0 {
        let top = c - c % rows;
        if top + rows < n {
            c = (c + rows).min(n - 1);
        }
    }
    c
}
