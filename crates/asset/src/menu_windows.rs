//! Menu-overlay **window descriptor table** - the caller-supplied window
//! rects + content-renderer dispatch behind every field pause-menu screen.
//!
//! The menu overlay (PROT 0899, base `0x801CE818`) holds a 52-entry table at
//! VA `0x801E473C` (file offset `0x15F24`). Each 0x10-byte record describes
//! one UI window:
//!
//! ```text
//! +0x0  i16  x        window-content origin X (the `a0+0xa` rect the
//! +0x2  i16  y        content renderers receive - `WX`/`WY` in
//! +0x4  i16  w        docs/subsystems/field-menu.md)
//! +0x6  i16  h
//! +0x8  u32  renderer content-renderer VA (menu-overlay function), or 0
//! +0xc  u16  f1       style/param word (low bits are per-renderer params;
//!                     runtime-mutated for some windows)
//! +0xe  u16  kind     window class (2 = title tab, 3 = standard window,
//!                     4 = list-page window, 0 seen once on id 51)
//! ```
//!
//! The live engine spawns windows as a doubly-linked list of 0x5C-stride
//! structs (rect at `+0xa`, descriptor id at `+0x8`); the live rect is the
//! window's *animated* position - closed windows slide toward the nearest
//! screen edge and park offscreen (x = 332 right, x = -124 left, y = 240
//! bottom in the captured states). The descriptor holds the home position.
//!
//! The rect is the **content/pen** rect: the caller-drawn 9-slice frame art
//! extends past it by 6 px left/right, 2 px above and 10 px below (measured
//! against the VRAM framebuffers of the six catalogued menu-open save
//! states; [`MenuWindowDescriptor::frame_rect`]).
//!
//! Provenance: rect + renderer values byte-matched between the disc image
//! and the resident menu overlay across the `menu_status_field` /
//! `menu_equipment_field` / `menu_options_field` mednafen captures (only
//! id 22's `f1` low bits and id 49's `y` differ at runtime). Renderer
//! attribution: id 28 carries `0x801D33D8`, the Ghidra-traced status-panel
//! renderer of docs/subsystems/field-menu.md, and its rect `(90,16,218,188)`
//! reproduces every pinned content offset against the captured framebuffer.

use anyhow::{Result, anyhow, bail};
use legaia_bytes::{i16_le, u16_le, u32_le};

/// PROT entry (extraction index) of the menu overlay hosting the table.
pub const MENU_OVERLAY_PROT_INDEX: usize = 899;
/// Menu-overlay link base.
pub const MENU_OVERLAY_BASE_VA: u32 = 0x801C_E818;
/// VA of the window-descriptor table inside the resident overlay.
pub const MENU_WINDOW_TABLE_VA: u32 = 0x801E_473C;
/// File offset of the table inside the as-loaded overlay image.
pub const MENU_WINDOW_TABLE_OFFSET: usize = (MENU_WINDOW_TABLE_VA - MENU_OVERLAY_BASE_VA) as usize;
/// Number of descriptor records (the record after the last one fails the
/// rect/renderer validity envelope - a structural bound, not a count word).
pub const MENU_WINDOW_COUNT: usize = 52;

/// One window descriptor: content rect + content-renderer dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuWindowDescriptor {
    /// Content-origin X (`WX` - the `a0+0xa` the renderer receives).
    pub x: i16,
    /// Content-origin Y (`WY` - `a0+0xc`).
    pub y: i16,
    /// Content width (`a0+0xe`; scroll-arrow / scrollbar geometry source).
    pub w: i16,
    /// Content height (`a0+0x10`).
    pub h: i16,
    /// Content-renderer VA in the menu overlay, or 0 (frame-only window).
    pub renderer_va: u32,
    /// Style/param word (low bits are per-renderer params).
    pub f1: u16,
    /// Window class: 2 = title tab, 3 = standard, 4 = list page.
    pub kind: u16,
}

impl MenuWindowDescriptor {
    /// Content rect as `(x, y, w, h)` in 320x240 display pixels.
    pub fn rect(&self) -> (i32, i32, i32, i32) {
        (self.x as i32, self.y as i32, self.w as i32, self.h as i32)
    }

    /// The 9-slice **frame** rect enclosing the content rect: the retail
    /// border art extends 6 px left/right, 2 px above and 10 px below the
    /// content rect (pixel-measured against the menu-open VRAM captures;
    /// the thicker bottom band is the frame's drop-shadow edge).
    pub fn frame_rect(&self) -> (i32, i32, i32, i32) {
        (
            self.x as i32 - 6,
            self.y as i32 - 2,
            self.w as i32 + 12,
            self.h as i32 + 12,
        )
    }
}

/// Well-known descriptor ids, mapped from the live window lists of the
/// catalogued menu-open save states (each screen's spawned windows carry
/// these ids at struct `+0x8`).
pub mod window_ids {
    /// "Equip" title tab (live on the equipment screen).
    pub const TAB_EQUIP: usize = 2;
    /// "Status" title tab (live on the status screen).
    pub const TAB_STATUS: usize = 3;
    /// "Options" title tab (live on the options screen).
    pub const TAB_OPTIONS: usize = 4;
    /// Equipment screen: party-member window (renderer shared with
    /// [`STATUS_PARTY_LIST`], wider rect).
    pub const EQUIP_PARTY: usize = 21;
    /// Equipment screen: main lower window ("Best Equipment" + slots).
    pub const EQUIP_MAIN: usize = 22;
    /// Equipment screen: right item-list window (renderer-less container;
    /// its lower span is occluded by [`EQUIP_MAIN`], drawn over it).
    pub const EQUIP_LIST: usize = 23;
    /// Status screen: party-member list window.
    pub const STATUS_PARTY_LIST: usize = 26;
    /// Status screen: "Condition" pager window.
    pub const STATUS_CONDITION: usize = 27;
    /// Status screen: the main per-character panel
    /// (renderer `FUN_801D33D8` - docs/subsystems/field-menu.md).
    pub const STATUS_MAIN: usize = 28;
    /// Status screen: lower-left character summary window (name/LV/ATR).
    pub const STATUS_SUMMARY: usize = 30;
    /// Options screen: the value-choice popup (renderer `FUN_801D2B44`).
    /// Its descriptor x/w are the home position; the options input SM
    /// (`FUN_801DA9F8`) stamps y (`+0x2`) and h (`+0x6`) per open -
    /// `y = id-48 y + 0x16 + the cursor row's layout offset`,
    /// `h = choices*13 - 4`, flipped above the anchor when the bottom
    /// would pass y = 0xB0.
    pub const OPTIONS_POPUP: usize = 47;
    /// Options screen: the single settings window.
    pub const OPTIONS_MAIN: usize = 48;
    /// Top-level pause menu: money + play-time corner box.
    pub const TOP_MONEY_TIME: usize = 49;
    /// Top-level pause menu: the command-list window
    /// (renderer `FUN_801CFD68`).
    pub const TOP_COMMAND_LIST: usize = 50;
    /// Top-level pause menu: right party-overview panel
    /// (renderer `FUN_801D030C`; slides in from x = 332).
    pub const TOP_INFO_PANEL: usize = 51;
}

/// Descriptor-id set per screen, in retail draw order (earlier windows are
/// drawn first, so a later window's opaque interior occludes them - the
/// equip screen relies on this: [`window_ids::EQUIP_MAIN`] covers the lower
/// span of [`window_ids::EQUIP_LIST`]).
pub const STATUS_SCREEN_WINDOWS: [usize; 5] = [
    window_ids::TAB_STATUS,
    window_ids::STATUS_PARTY_LIST,
    window_ids::STATUS_CONDITION,
    window_ids::STATUS_SUMMARY,
    window_ids::STATUS_MAIN,
];
/// Equipment screen window set (see [`STATUS_SCREEN_WINDOWS`] on order).
pub const EQUIP_SCREEN_WINDOWS: [usize; 4] = [
    window_ids::TAB_EQUIP,
    window_ids::EQUIP_PARTY,
    window_ids::EQUIP_LIST,
    window_ids::EQUIP_MAIN,
];
/// Options screen window set.
pub const OPTIONS_SCREEN_WINDOWS: [usize; 2] = [window_ids::TAB_OPTIONS, window_ids::OPTIONS_MAIN];
/// Top-level pause-menu window set.
pub const TOP_LEVEL_WINDOWS: [usize; 3] = [
    window_ids::TOP_COMMAND_LIST,
    window_ids::TOP_MONEY_TIME,
    window_ids::TOP_INFO_PANEL,
];

/// The parsed table.
#[derive(Debug, Clone)]
pub struct MenuWindowTable {
    pub windows: Vec<MenuWindowDescriptor>,
}

impl MenuWindowTable {
    /// Descriptor by id, if in range.
    pub fn window(&self, id: usize) -> Option<&MenuWindowDescriptor> {
        self.windows.get(id)
    }
}

fn record_is_valid(d: &MenuWindowDescriptor) -> bool {
    let renderer_ok = d.renderer_va == 0
        || (MENU_OVERLAY_BASE_VA..crate::static_overlay::OVERLAY_WINDOW_HI)
            .contains(&d.renderer_va);
    // Home rects sit on (or just off) the 320x240 display; parked/offscreen
    // homes stay within one screen of it.
    let rect_ok = (-320..640).contains(&(d.x as i32))
        && (-240..480).contains(&(d.y as i32))
        && (1..=320).contains(&(d.w as i32))
        && (1..=240).contains(&(d.h as i32));
    renderer_ok && rect_ok
}

/// Parse the window-descriptor table out of the **as-loaded** menu-overlay
/// image (PROT 0899 in its `static_overlay::as_loaded` form).
pub fn parse(overlay: &[u8]) -> Result<MenuWindowTable> {
    let end = MENU_WINDOW_TABLE_OFFSET + MENU_WINDOW_COUNT * 0x10;
    if overlay.len() < end {
        bail!(
            "menu overlay too short for window table: {:#x} < {:#x}",
            overlay.len(),
            end
        );
    }
    let mut windows = Vec::with_capacity(MENU_WINDOW_COUNT);
    let short = || anyhow!("menu window table read out of range");
    for id in 0..MENU_WINDOW_COUNT {
        let at = MENU_WINDOW_TABLE_OFFSET + id * 0x10;
        let d = MenuWindowDescriptor {
            x: i16_le(overlay, at).ok_or_else(short)?,
            y: i16_le(overlay, at + 2).ok_or_else(short)?,
            w: i16_le(overlay, at + 4).ok_or_else(short)?,
            h: i16_le(overlay, at + 6).ok_or_else(short)?,
            renderer_va: u32_le(overlay, at + 8).ok_or_else(short)?,
            f1: u16_le(overlay, at + 12).ok_or_else(short)?,
            kind: u16_le(overlay, at + 14).ok_or_else(short)?,
        };
        if !record_is_valid(&d) {
            bail!("menu window descriptor {id} fails the validity envelope: {d:?}");
        }
        windows.push(d);
    }
    Ok(MenuWindowTable { windows })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_overlay() -> Vec<u8> {
        let mut v = vec![0u8; MENU_WINDOW_TABLE_OFFSET + MENU_WINDOW_COUNT * 0x10 + 4];
        for id in 0..MENU_WINDOW_COUNT {
            let off = MENU_WINDOW_TABLE_OFFSET + id * 0x10;
            let (x, y, w, h): (i16, i16, i16, i16) = (16 + id as i16, 12, 60, 12);
            v[off..off + 2].copy_from_slice(&x.to_le_bytes());
            v[off + 2..off + 4].copy_from_slice(&y.to_le_bytes());
            v[off + 4..off + 6].copy_from_slice(&w.to_le_bytes());
            v[off + 6..off + 8].copy_from_slice(&h.to_le_bytes());
            let va: u32 = 0x801D_0000 + id as u32 * 0x40;
            v[off + 8..off + 12].copy_from_slice(&va.to_le_bytes());
            v[off + 12..off + 14].copy_from_slice(&0x0400u16.to_le_bytes());
            v[off + 14..off + 16].copy_from_slice(&2u16.to_le_bytes());
        }
        v
    }

    #[test]
    fn parses_synthetic_table() {
        let table = parse(&synthetic_overlay()).expect("parse");
        assert_eq!(table.windows.len(), MENU_WINDOW_COUNT);
        let d = table.window(3).unwrap();
        assert_eq!(d.rect(), (19, 12, 60, 12));
        assert_eq!(d.frame_rect(), (13, 10, 72, 24));
        assert_eq!(d.kind, 2);
    }

    #[test]
    fn rejects_out_of_envelope_records() {
        let mut v = synthetic_overlay();
        // Corrupt record 5's width to an impossible span.
        let off = MENU_WINDOW_TABLE_OFFSET + 5 * 0x10 + 4;
        v[off..off + 2].copy_from_slice(&5000i16.to_le_bytes());
        assert!(parse(&v).is_err());
    }

    #[test]
    fn rejects_short_input() {
        assert!(parse(&[0u8; 0x100]).is_err());
    }
}
