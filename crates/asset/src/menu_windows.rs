//! Menu-overlay **window descriptor table** - the caller-supplied window
//! rects + content-renderer dispatch behind every field pause-menu screen.
//!
//! The menu overlay (PROT 0899, base `0x801CE818`) holds a 52-entry table at
//! VA `0x801E4738` (file offset `0x15F20`): the overlay's window-script
//! runner `FUN_801D6628` computes `0x801E4738 + id*0x10` (`lui 0x801e` /
//! `addiu 0x4738` at `0x801D6658..0x801D665C`) and hands the record to the
//! SCUS window creator `FUN_800326AC`, whose field reads fix the layout.
//! Each 0x10-byte record describes one UI window:
//!
//! ```text
//! +0x0  u8   content_id  copied into live window `+0x1C` at create (`sb`
//!                        at `0x80032990`); selects the SCUS content-builder
//!                        case (`FUN_80030628` dispatches on it, accepting
//!                        `2..=0x22`; 0 = no built content)
//! +0x1  u8   park_edge   the `< 8` create switch at `0x800326E8`: which
//!                        screen edge the closed window parks against
//!                        (0 = bottom, 2 = left, 4 = top, 6 = right; odd
//!                        values are the corner combos, unused on disc)
//! +0x2  u16  kind        window class word: 2 = title tab, 3 = standard,
//!                        4 = list page. Low byte lands in live `+0x1D`
//!                        (staged into `gp+0x14C` for the frame drawer
//!                        `FUN_8002C69C` by the walker `FUN_80031D00`)
//! +0x4  i16  x           window-content origin (the `a0+0xa` rect the
//! +0x6  i16  y           content renderers receive - `WX`/`WY` in
//! +0x8  i16  w           docs/subsystems/field-menu.md)
//! +0xa  i16  h
//! +0xc  u32  renderer    content-renderer VA (menu-overlay function), or
//!                        0 = content-builder-driven list window
//! ```
//!
//! NB the decompiled C of `FUN_801D6628` renders the base as
//! `&DAT_801e473c + id*0x10` (folding the x-field offset into the symbol) -
//! reading that rendering instead of the disassembly yields a `+4`-skewed
//! table whose head fields belong to the *next* record. The x/y/w/h and
//! renderer bytes land on the same absolute file offsets either way.
//!
//! The live engine spawns windows as a doubly-linked list of 0x5C-stride
//! structs (rect at `+0xa`, descriptor id at `+0x8`); the live rect is the
//! window's *animated* position - closed windows slide toward the
//! [`MenuWindowDescriptor::park_edge`] screen edge and park offscreen
//! (x = 332 right, x = -124 left, y = 240 bottom in the captured states).
//! The descriptor holds the home position.
//!
//! The rect is the **content/pen** rect: the caller-drawn 9-slice frame art
//! extends past it by 8 px on every side (pinned by the RAM GPU-prim scan of
//! the `menu_status_town` capture - the frame corner tiles sit at
//! `content - 8` - and cross-checked against the captures' VRAM
//! framebuffers; [`MenuWindowDescriptor::frame_rect`]).
//!
//! Provenance: rect + renderer values byte-matched between the disc image
//! and the resident menu overlay across the `menu_status_field` /
//! `menu_equipment_field` / `menu_options_field` mednafen captures (only
//! id 23's `content_id` and id 49's `y` differ at runtime). Renderer
//! attribution: id 28 carries `0x801D33D8`, the Ghidra-traced status-panel
//! renderer of docs/subsystems/field-menu.md, and its rect `(90,16,218,188)`
//! reproduces every pinned content offset against the captured framebuffer.

use anyhow::{Result, anyhow, bail};
use legaia_bytes::{i16_le, u8_at, u16_le, u32_le};

/// PROT entry (extraction index) of the menu overlay hosting the table.
pub const MENU_OVERLAY_PROT_INDEX: usize = 899;
/// Menu-overlay link base.
pub const MENU_OVERLAY_BASE_VA: u32 = 0x801C_E818;
/// VA of the window-descriptor table inside the resident overlay (the base
/// `FUN_801D6628` indexes; see the module doc on the `+4`-skewed decompiler
/// rendering of the same address math).
pub const MENU_WINDOW_TABLE_VA: u32 = 0x801E_4738;
/// File offset of the table inside the as-loaded overlay image.
pub const MENU_WINDOW_TABLE_OFFSET: usize = (MENU_WINDOW_TABLE_VA - MENU_OVERLAY_BASE_VA) as usize;
/// Number of descriptor records (the record after the last one fails the
/// rect/renderer validity envelope - a structural bound, not a count word).
pub const MENU_WINDOW_COUNT: usize = 52;

/// One window descriptor: content rect + content-renderer dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuWindowDescriptor {
    /// Content id (`+0x0` -> live window `+0x1C`): the SCUS content-builder
    /// dispatch selector (`FUN_80030628` accepts `2..=0x22`; 0 = no built
    /// content). Non-zero exactly on the renderer-less list windows
    /// (e.g. id 15 Items Use list = `0x03`, id 16 Throw Out list = `0x22`,
    /// id 23 equip candidate list = `0x15`).
    pub content_id: u8,
    /// Park-edge class (`+0x1`): the `FUN_800326AC` create switch (0..7)
    /// picking which screen edge the closed window slides toward
    /// (0 = bottom, 2 = left, 4 = top, 6 = right on every disc record).
    pub park_edge: u8,
    /// Window class word (`+0x2`): 2 = title tab, 3 = standard, 4 = list
    /// page. Low byte lands in live window `+0x1D` (the frame drawer's
    /// style byte, staged into `gp+0x14C` by `FUN_80031D00`).
    pub kind: u16,
    /// Content-origin X (`WX` - the `a0+0xa` the renderer receives).
    pub x: i16,
    /// Content-origin Y (`WY` - `a0+0xc`).
    pub y: i16,
    /// Content width (`a0+0xe`; scroll-arrow / scrollbar geometry source).
    pub w: i16,
    /// Content height (`a0+0x10`).
    pub h: i16,
    /// Content-renderer VA in the menu overlay, or 0 (content-builder-driven
    /// list window; see [`MenuWindowDescriptor::content_id`]).
    pub renderer_va: u32,
}

impl MenuWindowDescriptor {
    /// Content rect as `(x, y, w, h)` in 320x240 display pixels.
    pub fn rect(&self) -> (i32, i32, i32, i32) {
        (self.x as i32, self.y as i32, self.w as i32, self.h as i32)
    }

    /// The 9-slice **frame** rect enclosing the content rect: the retail
    /// border art extends 8 px past the content rect on every side (the
    /// RAM prim scan of the `menu_status_town` capture places each
    /// window's 4x4 corner tiles at `content - 8`, e.g. window 26's
    /// content `(14, 38)` frames from `(6, 30)`; edge pixels
    /// cross-checked against the menu-open VRAM captures).
    pub fn frame_rect(&self) -> (i32, i32, i32, i32) {
        (
            self.x as i32 - 8,
            self.y as i32 - 8,
            self.w as i32 + 16,
            self.h as i32 + 16,
        )
    }
}

/// Well-known descriptor ids, mapped from the live window lists of the
/// catalogued menu-open save states (each screen's spawned windows carry
/// these ids at struct `+0x8`).
pub mod window_ids {
    /// "Items" title tab (renderer `FUN_801DCA0C`; live on the Items screen).
    pub const TAB_ITEMS: usize = 0;
    /// "Magic" title tab (renderer `FUN_801DCA50`; live on the Magic screen).
    pub const TAB_MAGIC: usize = 1;
    /// "Equip" title tab (live on the equipment screen).
    pub const TAB_EQUIP: usize = 2;
    /// "Status" title tab (live on the status screen).
    pub const TAB_STATUS: usize = 3;
    /// "Options" title tab (live on the options screen).
    pub const TAB_OPTIONS: usize = 4;
    /// Items screen: Use / Throw Out / Arrange command window
    /// (renderer `FUN_801D0D18`).
    pub const ITEMS_COMMAND: usize = 13;
    /// Items screen: right item-list page (renderer-less container; the
    /// items flow draws the page content directly).
    pub const ITEMS_LIST: usize = 15;
    /// Items screen: item info window (renderer `FUN_801DCB60` -> the
    /// shared item-info panel `FUN_801D0F1C`). The renderer also emits a
    /// second framed widget box below itself
    /// (`FUN_8002C69C(WX, WY+0x38, 0x90, 0x28)`).
    pub const ITEMS_INFO: usize = 17;
    /// Magic screen: right spell-list page (renderer-less container).
    pub const MAGIC_LIST: usize = 18;
    /// Magic screen: caster (party) window (renderer `FUN_801D2C98`).
    pub const MAGIC_CASTER: usize = 19;
    /// Magic screen: spell info window (renderer `FUN_801D2E74`).
    pub const MAGIC_INFO: usize = 20;
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
    /// (`FUN_801DA9F8`) stamps y (record `+0x6`, `sh 0x2F6(a0)` off the
    /// table base) and h (record `+0xA`, `sh 0x2FA(a0)`) per open -
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
/// Items screen window set (draw order of the pad-walked menu captures:
/// tab, command, list, info - see `docs/subsystems/field-menu.md`).
pub const ITEMS_SCREEN_WINDOWS: [usize; 4] = [
    window_ids::TAB_ITEMS,
    window_ids::ITEMS_COMMAND,
    window_ids::ITEMS_LIST,
    window_ids::ITEMS_INFO,
];
/// Magic screen window set (draw order: tab, list, caster, info).
pub const MAGIC_SCREEN_WINDOWS: [usize; 4] = [
    window_ids::TAB_MAGIC,
    window_ids::MAGIC_LIST,
    window_ids::MAGIC_CASTER,
    window_ids::MAGIC_INFO,
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
    // The create-time park switch dispatches on `< 8` only.
    renderer_ok && rect_ok && d.park_edge < 8
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
            content_id: u8_at(overlay, at).ok_or_else(short)?,
            park_edge: u8_at(overlay, at + 1).ok_or_else(short)?,
            kind: u16_le(overlay, at + 2).ok_or_else(short)?,
            x: i16_le(overlay, at + 4).ok_or_else(short)?,
            y: i16_le(overlay, at + 6).ok_or_else(short)?,
            w: i16_le(overlay, at + 8).ok_or_else(short)?,
            h: i16_le(overlay, at + 10).ok_or_else(short)?,
            renderer_va: u32_le(overlay, at + 12).ok_or_else(short)?,
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
            v[off] = 0x15; // content id
            v[off + 1] = 4; // park edge (top)
            v[off + 2..off + 4].copy_from_slice(&2u16.to_le_bytes());
            let (x, y, w, h): (i16, i16, i16, i16) = (16 + id as i16, 12, 60, 12);
            v[off + 4..off + 6].copy_from_slice(&x.to_le_bytes());
            v[off + 6..off + 8].copy_from_slice(&y.to_le_bytes());
            v[off + 8..off + 10].copy_from_slice(&w.to_le_bytes());
            v[off + 10..off + 12].copy_from_slice(&h.to_le_bytes());
            let va: u32 = 0x801D_0000 + id as u32 * 0x40;
            v[off + 12..off + 16].copy_from_slice(&va.to_le_bytes());
        }
        v
    }

    #[test]
    fn parses_synthetic_table() {
        let table = parse(&synthetic_overlay()).expect("parse");
        assert_eq!(table.windows.len(), MENU_WINDOW_COUNT);
        let d = table.window(3).unwrap();
        assert_eq!(d.rect(), (19, 12, 60, 12));
        assert_eq!(d.frame_rect(), (11, 4, 76, 28));
        assert_eq!(d.content_id, 0x15);
        assert_eq!(d.park_edge, 4);
        assert_eq!(d.kind, 2);
    }

    #[test]
    fn rejects_out_of_envelope_records() {
        let mut v = synthetic_overlay();
        // Corrupt record 5's width to an impossible span.
        let off = MENU_WINDOW_TABLE_OFFSET + 5 * 0x10 + 8;
        v[off..off + 2].copy_from_slice(&5000i16.to_le_bytes());
        assert!(parse(&v).is_err());
    }

    #[test]
    fn rejects_out_of_domain_park_edge() {
        let mut v = synthetic_overlay();
        // The create-time park switch only dispatches on values < 8.
        v[MENU_WINDOW_TABLE_OFFSET + 7 * 0x10 + 1] = 8;
        assert!(parse(&v).is_err());
    }

    #[test]
    fn rejects_short_input() {
        assert!(parse(&[0u8; 0x100]).is_err());
    }
}
