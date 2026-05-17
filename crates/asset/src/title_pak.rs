//! Title-screen TIM extractor.
//!
//! The "Legend of Legaia" title screen lives as a 256x256 8bpp TIM in
//! PROT entries `0888..=0890` (labelled `sound_data2` per CDNAME, but
//! the multi-bank sound-data cluster carries title art in the trailing
//! pool past the actual sound payload). The byte layout below is stable
//! across the NA retail build.
//!
//! ## Sources by PROT entry
//!
//! ```text
//!   PROT 0888 (sound_data2)    @ file offset 0x1AA28    — PRIMARY
//!   PROT 0889 (sound_data2)    @ file offset 0x19A28    — same content,
//!   PROT 0890 (sound_data2)    @ file offset 0x14228    — multi-bank duplicates
//! ```
//!
//! The TIM is 33312 bytes total (8-byte header + 12 + 512 CLUT block +
//! 12 + 65536 pixel block). Pixel-block VRAM target is `fb=(512,256)`,
//! CLUT block target is in-line with the pixel block's frame buffer.
//!
//! Pinned by `scripts/scan_tims_and_match_prot.py` against a full
//! 2 MiB main-RAM dump captured at the live title screen (sstate8),
//! cross-referenced against the in-RAM copy at vaddr `0x80170DF8`.
//! Renders to the complete title screen: wordmark, orb, "PRESS START
//! BUTTON", "NEW GAME" / "CONTINUE" menu, copyright lines.
//!
//! See [`crate::init_pak`] for the parallel publisher-logo case
//! (PROT 0895, `bat_back_dat` per CDNAME, actually init.pak).
//!
//! ## Related title-overlay TIMs
//!
//! The title overlay's save/load sub-menu draws two sprite-descriptor
//! TIMs embedded inside the overlay binary itself, in PROT entry
//! 0899's extended footprint (the trailing-gap title overlay landed
//! in commit `df4f1ce`):
//!
//! ```text
//!   PROT 0899 @ file offset 0x16908    — save-menu UI atlas
//!                                         (256x256 4bpp)
//!   PROT 0899 @ file offset 0x1F908    — animated memory-card icon
//!                                         (256x16 4bpp, 14 frames)
//! ```
//!
//! These are referenced as runtime sprite-descriptor templates at
//! vaddrs `0x801E5120` / `0x801EE120` by `FUN_801DD35C` (the title
//! tick); see [`crate::title_overlay`] in `engine-vm` for the
//! dispatcher port.
//!
//! ## Provenance
//!
//! Methodology: scan a PSX main-RAM dump for TIM-magic-headed records,
//! byte-grep the extracted PROT corpus for each candidate. The
//! runtime patches only `fb_x`/`fb_y` for CLUT relocation; the rest
//! of each TIM is byte-equal to the on-disc source. Full repro in
//! `scripts/scan_tims_and_match_prot.py --help`.

use anyhow::{Context, Result};

/// Primary PROT entry index for the main title TIM (NA retail build).
///
/// Entries 0889 and 0890 carry duplicate copies; see
/// [`TITLE_TIM_ALTERNATE_SOURCES`].
pub const PROT_INDEX_PRIMARY: u16 = 888;

/// Alternate PROT entry indices carrying the same title TIM at
/// different file offsets (multi-bank duplicates in the
/// `sound_data2` cluster).
pub const TITLE_TIM_ALTERNATE_SOURCES: &[(u16, usize)] = &[(889, 0x19A28), (890, 0x14228)];

/// File offset within PROT 0888 where the main title TIM begins.
pub const TITLE_TIM_OFFSET: usize = 0x1AA28;

/// Total byte length of the title TIM (header + CLUT block + pixel
/// block). The main title is 256x256 8bpp + 256-colour CLUT:
/// `8 + (12 + 256*2) + (12 + 256*256) = 66080` bytes.
pub const TITLE_TIM_SIZE: usize = 66080;

/// PROT entry index carrying the title-overlay's save-menu sprite TIMs
/// embedded in its trailing-overlay binary.
pub const PROT_INDEX_OVERLAY: u16 = 899;

/// File offset within PROT 0899 (extended footprint) of the save-menu
/// UI sprite atlas (256x256 4bpp; memory-card icons + Japanese strings).
/// Drawn via sprite-descriptor template `0x801E5120` at runtime.
pub const OVERLAY_SAVE_MENU_TIM_OFFSET: usize = 0x16908;

/// File offset within PROT 0899 (extended footprint) of the animated
/// memory-card icon strip (256x16 4bpp, 14 frames). Drawn via
/// sprite-descriptor template `0x801EE120` at runtime.
pub const OVERLAY_CARD_ICON_TIM_OFFSET: usize = 0x1F908;

/// Total byte length of [`OVERLAY_SAVE_MENU_TIM_OFFSET`]'s TIM.
pub const OVERLAY_SAVE_MENU_TIM_SIZE: usize = 33312;

/// Total byte length of [`OVERLAY_CARD_ICON_TIM_OFFSET`]'s TIM.
pub const OVERLAY_CARD_ICON_TIM_SIZE: usize = 2592;

/// Source sub-rect, in atlas pixels `(x, y, w, h)`, of a horizontal
/// **rounded-rectangle frame** sprite inside the 256x256 save-menu
/// TIM at PROT 0899. **WRONG SOURCE — kept only until the engine is
/// retargeted at [`OVERLAY_SYSTEM_UI_TIM_OFFSET`].** A VRAM dump from
/// PCSX-Redux sstate9 (gunzip the save state → extract the GPU.vram
/// protobuf field → decode 1024×512 BGR555) showed the load-screen
/// panel's CLUT lives at VRAM `(fb_x=32, fb_y=511)`, which is CLUT
/// row 2 of the system-UI sprite sheet TIM at `PROT.DAT[0x018E0]`.
/// That CLUT signature is unique to that TIM in the entire disc
/// corpus. The 9-slice tile geometry inside that sheet still needs
/// to be pinned (GPULog probe pending — sliding-window template
/// match on the captured panel peaks at 31.6% gold-pixel agreement,
/// so retail composites the panel from multiple smaller tiles, not
/// a single 81×29 sprite).
pub const OVERLAY_SAVE_MENU_BAND_PANEL: (u32, u32, u32, u32) = (1, 0, 103, 16);

/// Source sub-rect of the **SLOT 1 pill** (baked label "SLOT 1") inside
/// the save-menu TIM. Best rendered with CLUT 7 for the bright-blue
/// retail look.
pub const OVERLAY_SAVE_MENU_BAND_SLOT1: (u32, u32, u32, u32) = (33, 97, 45, 15);

/// Source sub-rect of the **SLOT 2 pill** (baked label "SLOT 2") inside
/// the save-menu TIM. Stacked directly below [`OVERLAY_SAVE_MENU_BAND_SLOT1`].
/// Best rendered with CLUT 7.
pub const OVERLAY_SAVE_MENU_BAND_SLOT2: (u32, u32, u32, u32) = (33, 113, 45, 15);

/// Source sub-rect of a **synthetic solid-blue 4×4 fill tile** the
/// engine writes into an otherwise-empty region of the decoded save-
/// menu atlas. **STOPGAP — to be deleted when the engine switches to
/// [`OVERLAY_SYSTEM_UI_TIM_OFFSET`].** A VRAM dump confirms retail
/// does NOT need a synthetic fill: it composites the panel by
/// drawing 4bpp tiles from the system-UI sprite sheet over the
/// dimmed title art, with the marbled interior emerging from
/// semi-transparent blending of CLUT-row-2 entries 1..6 (a blue
/// gradient) with the dimmed background — not from any solid fill
/// pre-baked into a sprite. Of 51 distinct panel-interior framebuffer
/// colors at parked sstate9, only 9 (the gold border) map directly
/// to CLUT entries; the rest are blend products.
///
/// Engines must NOT sample these atlas coordinates from the raw TIM —
/// the tile is filled in by [`crate::save_menu_atlas`] only.
pub const OVERLAY_SAVE_MENU_BAND_PANEL_FILL: (u32, u32, u32, u32) = (200, 200, 4, 4);

// -----------------------------------------------------------------------
// System-UI sprite sheet — byte-confirmed source of the load-screen panel.
//
// Lives in the unindexed pre-`init_data` gap of PROT.DAT (a 236 KiB region
// before entry 0; see [[project-title-tims-in-overlay]]). The TIM contains
// the entire in-game menu UI atlas decoded with CLUT row 2: stat panels,
// money/HP/MP displays, battle-menu chrome, equipment-slot frames, and the
// load-screen "Load" panel.
//
// VRAM pin (byte-confirmed via PCSX-Redux sstate9 GPU.vram protobuf field):
//   - CLUT block uploads to VRAM (fb_x=0,  fb_y=511) at 16x16 entries.
//   - Pixel block uploads to VRAM (fb_x=896, fb_y=256) at 64x192 halfwords
//     (= 256x192 4bpp source pixels).
//   - Load-screen panel uses CLUT **row 2** specifically.
// -----------------------------------------------------------------------

/// File offset within `PROT.DAT` (extended-footprint walk) of the
/// **system-UI sprite sheet** TIM. 4bpp + 16x16 CLUT block; 256x192
/// source pixels. Byte-confirmed as the source of the load-screen
/// "Load" panel via VRAM dump cross-reference (its CLUT row 2 is the
/// only CLUT in the disc corpus whose bytes match the retail panel
/// CLUT live at VRAM fb_y=511).
pub const OVERLAY_SYSTEM_UI_TIM_OFFSET: usize = 0x018E0;

/// Total byte length of [`OVERLAY_SYSTEM_UI_TIM_OFFSET`]'s TIM
/// (header + CLUT block + pixel block).
pub const OVERLAY_SYSTEM_UI_TIM_SIZE: usize = 0x07B00 - 0x018E0;

/// CLUT block row that decodes the load-screen panel chrome. Row 2
/// contains the 9-color gold-bronze gradient (entries 7..15) used by
/// the panel border, and the 6-color blue gradient (entries 1..6)
/// used by the marbled interior. Other CLUT rows in the same TIM
/// decode different menu-UI elements (HP/MP/money panels, battle
/// chrome, equipment frames).
pub const OVERLAY_SYSTEM_UI_PANEL_CLUT_ROW: u16 = 2;

// -----------------------------------------------------------------------
// Load-screen panel 9-slice source tiles
//
// Pinned via `scripts/pcsx-redux/scan_panel_prims.py` against
// `load_screen_ram.bin` captured at sstate9 — the retail engine had
// already queued 14 textured-sprite primitives (GP0 cmd 0x64) in the
// panel rect, each one carrying its source u/v + CLUT inline. All
// sample CLUT row 2 of `OVERLAY_SYSTEM_UI_TIM_OFFSET`.
//
// All rects are `(u, v, w, h)` in 256x192 source-page-pixel coords.
// Retail uses NO interior fill sprite — the "marbled blue" look on
// the load screen is the dimmed title art bleeding through the empty
// middle of the frame.
// -----------------------------------------------------------------------

/// Panel **top-left corner** tile.
pub const OVERLAY_SYSTEM_UI_PANEL_TL: (u32, u32, u32, u32) = (160, 0, 4, 4);
/// Panel **top-right corner** tile.
pub const OVERLAY_SYSTEM_UI_PANEL_TR: (u32, u32, u32, u32) = (188, 0, 4, 4);
/// Panel **bottom-left corner** tile.
pub const OVERLAY_SYSTEM_UI_PANEL_BL: (u32, u32, u32, u32) = (160, 28, 4, 4);
/// Panel **bottom-right corner** tile.
pub const OVERLAY_SYSTEM_UI_PANEL_BR: (u32, u32, u32, u32) = (188, 28, 4, 4);
/// Panel **top edge** tile — repeated horizontally between the top
/// corners. Stride = 24 in retail; the last instance is sampled at
/// a smaller `w` to cover the remainder when the panel width isn't a
/// multiple of 24 + 8 (the two corner widths).
pub const OVERLAY_SYSTEM_UI_PANEL_TOP: (u32, u32, u32, u32) = (164, 0, 24, 4);
/// Panel **bottom edge** tile — same dimensions as
/// [`OVERLAY_SYSTEM_UI_PANEL_TOP`], 24 pixels lower.
pub const OVERLAY_SYSTEM_UI_PANEL_BOT: (u32, u32, u32, u32) = (164, 28, 24, 4);
/// Panel **left edge** tile — height matches the panel content
/// area (retail's load-screen panel uses height=21).
pub const OVERLAY_SYSTEM_UI_PANEL_LEFT: (u32, u32, u32, u32) = (160, 4, 4, 21);
/// Panel **right edge** tile.
pub const OVERLAY_SYSTEM_UI_PANEL_RIGHT: (u32, u32, u32, u32) = (188, 4, 4, 21);

/// Retail framebuffer placement of the load-screen panel: dst
/// origin `(6, 4)` with overall size `81 x 29` pixels. Engines that
/// need to draw the panel elsewhere can use the 9-slice constants
/// above with a different anchor.
pub const OVERLAY_SAVE_PANEL_RETAIL_DST: (u32, u32, u32, u32) = (6, 4, 81, 29);

/// **Panel interior fill** — the 32x29 marbled-blue source region
/// retail samples to fill the 9-slice frame's interior. Byte-pinned
/// via `scripts/pcsx-redux/scan_panel_prims.py` (extended to
/// `0x3C..0x3F` gouraud-shaded textured-quad cmds): retail draws this
/// region as 3 gouraud-shaded textured quads with vertical gray
/// gradient `rgb(64,64,64) → rgb(136,136,136)` and CLUT row 2 — two
/// full-width 32x29 copies + one 17-wide-remainder copy, tiling
/// horizontally to fill the panel's 81-wide interior.
pub const OVERLAY_SYSTEM_UI_PANEL_INTERIOR: (u32, u32, u32, u32) = (128, 0, 32, 29);

/// Gouraud gradient applied to the interior: top vertex RGB.
pub const OVERLAY_SYSTEM_UI_PANEL_INTERIOR_TOP_RGB: (u8, u8, u8) = (64, 64, 64);
/// Gouraud gradient applied to the interior: bottom vertex RGB.
pub const OVERLAY_SYSTEM_UI_PANEL_INTERIOR_BOT_RGB: (u8, u8, u8) = (136, 136, 136);

/// **Pointing-finger cursor** sprite — the small white hand retail
/// renders to the left of the highlighted slot pill. Lives in the
/// same system-UI TIM as the panel chrome but uses a different CLUT
/// row (white-ink, not gold-bronze). Byte-pinned via the same
/// `scripts/pcsx-redux/scan_panel_prims.py` scan as the panel tiles
/// — retail dispatches it as a single textured-sprite primitive
/// with `dst=(114, 100)`, `src=(152, 64, 16, 16)`, CLUT at
/// VRAM `(112, 511)`.
pub const OVERLAY_SYSTEM_UI_CURSOR: (u32, u32, u32, u32) = (152, 64, 16, 16);

/// CLUT block row used to render the pointing-finger cursor (white-
/// ink with grey shading). Different from the panel's CLUT row 2 —
/// row 7 of the same 16x16 CLUT block. Maps to VRAM `(112, 511)`.
pub const OVERLAY_SYSTEM_UI_CURSOR_CLUT_ROW: u16 = 7;

/// Retail framebuffer placement of the cursor: dst origin `(114, 100)`
/// — directly left of the SLOT 1 pill at `(150, 100)`. The cursor's
/// y coord stays fixed (100); engines change x or y for SLOT 2 by
/// adjusting by `SAVE_SELECT_SLOT_PITCH_Y` (typically 16 stage pixels).
pub const OVERLAY_SAVE_CURSOR_RETAIL_DST: (u32, u32) = (114, 100);

/// Source sub-rect, in atlas pixels `(x, y, w, h)`, of the orb +
/// "Legend of Legaia" wordmark band inside the 256×256 title TIM.
/// Always drawn in PressStart and MainMenu phases - matches retail.
pub const TITLE_BAND_WORDMARK: (u32, u32, u32, u32) = (0, 17, 256, 124);

/// Source sub-rect of the `<DEMO>` band. **Demo-only** - retail builds
/// never draw this region, even though it sits in the same TIM. Kept
/// here as a reference; engines should NOT emit a draw for this rect.
pub const TITLE_BAND_DEMO: (u32, u32, u32, u32) = (96, 151, 64, 10);

/// Source sub-rect of the "PRESS START BUTTON" prompt label. Drawn
/// only during the PressStart phase, matching retail.
pub const TITLE_BAND_PRESS_START: (u32, u32, u32, u32) = (60, 178, 196, 16);

/// Source sub-rect of the "TM of Sony Computer Entertainment America
/// Inc." copyright line. Drawn in all post-fade phases.
pub const TITLE_BAND_TM_COPYRIGHT: (u32, u32, u32, u32) = (4, 195, 244, 14);

/// Source sub-rect of the "© 1998,1999 Sony Computer Entertainment
/// Inc." copyright line. Drawn in all post-fade phases.
pub const TITLE_BAND_C_COPYRIGHT: (u32, u32, u32, u32) = (8, 209, 234, 14);

/// Source sub-rect of the **"NEW GAME"** menu row. Retail's two-row
/// main-menu strings sit in a single horizontal strip at `y=227..237`
/// inside the title TIM, in the same stylised small-caps font as the
/// "PRESS START BUTTON" and copyright bands. Drawn during the
/// `MainMenu` phase. Colour-based selection: bright/white when the
/// cursor is on this row, dim/gray otherwise.
pub const TITLE_BAND_MENU_NEW_GAME: (u32, u32, u32, u32) = (0, 227, 65, 10);

/// Source sub-rect of the **"CONTINUE"** menu row. Same band as
/// [`TITLE_BAND_MENU_NEW_GAME`]; sampled at a different `x` so retail
/// can stack the two rows vertically on screen.
pub const TITLE_BAND_MENU_CONTINUE: (u32, u32, u32, u32) = (65, 227, 62, 10);

/// PSX TIM magic word (`0x00000010` LE).
const TIM_MAGIC: u32 = 0x0000_0010;

/// A title-screen TIM extracted from one of the title-PROT entries.
#[derive(Debug, Clone)]
pub struct TitleTim<'a> {
    /// File offset within the source PROT entry.
    pub file_offset: usize,
    /// Total byte length (header + CLUT + pixel).
    pub byte_len: usize,
    /// Reference into the input buffer (no copy).
    pub bytes: &'a [u8],
    /// PSX VRAM target rect for the pixel block — `(fb_x, fb_y, w, h)`.
    pub pixel_rect: (u16, u16, u16, u16),
    /// PSX VRAM target rect for the CLUT block — `(fb_x, fb_y, w, h)`.
    pub clut_rect: (u16, u16, u16, u16),
    /// TIM colour mode (`0`=4bpp, `1`=8bpp, `2`=15bpp, `3`=24bpp).
    pub mode: u8,
}

/// Extract the main title TIM from PROT 0888 (or 889 / 890) bytes.
///
/// Validates the TIM header at [`TITLE_TIM_OFFSET`]. Pass the bytes of
/// PROT entry [`PROT_INDEX_PRIMARY`] (or an alternate from
/// [`TITLE_TIM_ALTERNATE_SOURCES`] - in which case pass the matching
/// offset as `at_offset`).
pub fn extract_title_tim(bytes: &[u8], at_offset: usize) -> Result<TitleTim<'_>> {
    parse_tim_at(bytes, at_offset)
}

/// Extract the save-menu UI sprite atlas from PROT 0899's extended
/// footprint. Returns the 256x256 4bpp TIM at
/// [`OVERLAY_SAVE_MENU_TIM_OFFSET`].
pub fn extract_overlay_save_menu_tim(bytes: &[u8]) -> Result<TitleTim<'_>> {
    parse_tim_at(bytes, OVERLAY_SAVE_MENU_TIM_OFFSET)
}

/// Extract the animated PSX-memory-card icon strip from PROT 0899's
/// extended footprint. Returns the 256x16 4bpp TIM at
/// [`OVERLAY_CARD_ICON_TIM_OFFSET`].
pub fn extract_overlay_card_icon_tim(bytes: &[u8]) -> Result<TitleTim<'_>> {
    parse_tim_at(bytes, OVERLAY_CARD_ICON_TIM_OFFSET)
}

/// Extract the system-UI sprite sheet TIM from raw `PROT.DAT` bytes
/// (not a per-PROT entry — this TIM lives in the unindexed pre-
/// `init_data` gap; pass the whole disc-level PROT.DAT buffer).
/// Returns the 256x192 4bpp TIM at [`OVERLAY_SYSTEM_UI_TIM_OFFSET`].
/// Combine with [`OVERLAY_SYSTEM_UI_PANEL_CLUT_ROW`] to render the
/// load-screen panel chrome.
pub fn extract_overlay_system_ui_tim(prot_dat_bytes: &[u8]) -> Result<TitleTim<'_>> {
    parse_tim_at(prot_dat_bytes, OVERLAY_SYSTEM_UI_TIM_OFFSET)
}

/// Same as [`extract_overlay_system_ui_tim`] but accepts a slice that
/// **already starts at the TIM header** (i.e. the bytes from
/// `OVERLAY_SYSTEM_UI_TIM_OFFSET` for `OVERLAY_SYSTEM_UI_TIM_SIZE`
/// bytes). Useful when the caller has already used
/// `prot_dat_raw_bytes(OVERLAY_SYSTEM_UI_TIM_OFFSET, …)` to pull
/// just the TIM region into memory — avoids holding the full
/// ~115 MB PROT.DAT for one 25 KB parse.
pub fn extract_overlay_system_ui_tim_from_slice(tim_bytes: &[u8]) -> Result<TitleTim<'_>> {
    parse_tim_at(tim_bytes, 0)
}

fn parse_tim_at(bytes: &[u8], off: usize) -> Result<TitleTim<'_>> {
    let read_u32 = |p: usize| -> Result<u32> {
        bytes
            .get(p..p + 4)
            .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
            .with_context(|| format!("out-of-range read at 0x{:x}", p))
    };
    let read_u16 = |p: usize| -> Result<u16> {
        bytes
            .get(p..p + 2)
            .map(|s| u16::from_le_bytes(s.try_into().unwrap()))
            .with_context(|| format!("out-of-range read at 0x{:x}", p))
    };

    let magic = read_u32(off)?;
    if magic != TIM_MAGIC {
        anyhow::bail!(
            "bad TIM magic 0x{:08x} at 0x{:x} (expected 0x10)",
            magic,
            off
        );
    }
    let flags = read_u32(off + 4)?;
    let mode = (flags & 0x7) as u8;
    let has_clut = (flags & 0x8) != 0;
    if mode > 3 {
        anyhow::bail!("invalid TIM mode {}", mode);
    }
    if !has_clut {
        anyhow::bail!("title TIM at 0x{:x} expected CLUT (flags bit 3)", off);
    }

    let mut p = off + 8;
    let clut_size = read_u32(p)? as usize;
    let clut_fb_x = read_u16(p + 4)?;
    let clut_fb_y = read_u16(p + 6)?;
    let clut_w = read_u16(p + 8)?;
    let clut_h = read_u16(p + 10)?;
    p += clut_size;

    let pix_size = read_u32(p)? as usize;
    let pix_fb_x = read_u16(p + 4)?;
    let pix_fb_y = read_u16(p + 6)?;
    let pix_w = read_u16(p + 8)?;
    let pix_h = read_u16(p + 10)?;
    p += pix_size;

    let byte_len = p - off;
    let slice = bytes
        .get(off..off + byte_len)
        .with_context(|| format!("TIM at 0x{:x} overruns file", off))?;

    Ok(TitleTim {
        file_offset: off,
        byte_len,
        bytes: slice,
        pixel_rect: (pix_fb_x, pix_fb_y, pix_w, pix_h),
        clut_rect: (clut_fb_x, clut_fb_y, clut_w, clut_h),
        mode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Disc-gated: extract the main title TIM from a real PROT 0888.
    /// Skips when `extracted/` is missing (CI runs without disc data).
    #[test]
    fn extracts_real_title_tim_when_disc_extracted() {
        let path = "../../extracted/PROT/0888_sound_data2.BIN";
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("skip: extracted/PROT/0888_sound_data2.BIN missing");
                return;
            }
        };
        let tim =
            extract_title_tim(&bytes, TITLE_TIM_OFFSET).expect("extract main title TIM at 0x1AA28");

        // Canonical layout: 256x256 8bpp + 256-colour CLUT. The runtime
        // patches fb_x/fb_y for CLUT relocation; the dimensions + size
        // are stable.
        assert_eq!(tim.file_offset, TITLE_TIM_OFFSET);
        assert_eq!(tim.byte_len, TITLE_TIM_SIZE);
        assert_eq!(tim.mode, 1); // 8bpp
        assert_eq!(tim.pixel_rect.2, 128); // pw halfwords = 128 (= 256 8bpp pixels)
        assert_eq!(tim.pixel_rect.3, 256); // ph
        assert_eq!(tim.clut_rect.2, 256); // 256-colour CLUT
        assert_eq!(tim.clut_rect.3, 1); // 1 CLUT row
    }

    /// Disc-gated: extract the save-menu UI sprite atlas from PROT 0899.
    /// Requires the EXTENDED footprint (trailing-overlay sectors) so
    /// the extracted file must come from `Archive::read_entry`, not
    /// `read_entry_indexed`.
    #[test]
    fn extracts_overlay_save_menu_tim_when_disc_extracted() {
        let path = "../../extracted/PROT/0899_xxx_dat.BIN";
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("skip: extracted/PROT/0899_xxx_dat.BIN missing");
                return;
            }
        };
        let tim = extract_overlay_save_menu_tim(&bytes).expect("extract save-menu TIM at 0x16908");

        assert_eq!(tim.file_offset, OVERLAY_SAVE_MENU_TIM_OFFSET);
        assert_eq!(tim.byte_len, OVERLAY_SAVE_MENU_TIM_SIZE);
        assert_eq!(tim.mode, 0); // 4bpp
        assert_eq!(tim.pixel_rect.2, 64); // pw halfwords = 64 (= 256 4bpp pixels)
        assert_eq!(tim.pixel_rect.3, 256); // ph
    }

    /// Disc-gated: extract the animated memory-card icon strip.
    #[test]
    fn extracts_overlay_card_icon_tim_when_disc_extracted() {
        let path = "../../extracted/PROT/0899_xxx_dat.BIN";
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("skip: extracted/PROT/0899_xxx_dat.BIN missing");
                return;
            }
        };
        let tim = extract_overlay_card_icon_tim(&bytes).expect("extract card-icon TIM at 0x1F908");

        assert_eq!(tim.file_offset, OVERLAY_CARD_ICON_TIM_OFFSET);
        assert_eq!(tim.byte_len, OVERLAY_CARD_ICON_TIM_SIZE);
        assert_eq!(tim.mode, 0); // 4bpp
        assert_eq!(tim.pixel_rect.2, 64); // pw halfwords = 64
        assert_eq!(tim.pixel_rect.3, 16); // ph (14 frames + small gutter)
    }

    /// Disc-gated: each alternate source PROT entry should carry an
    /// identical (byte-equal) copy of the title TIM at its listed offset.
    #[test]
    fn alternate_sources_byte_equal_when_disc_extracted() {
        let primary_path = "../../extracted/PROT/0888_sound_data2.BIN";
        let primary_bytes = match std::fs::read(primary_path) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("skip: extracted/PROT/0888_sound_data2.BIN missing");
                return;
            }
        };
        let primary = extract_title_tim(&primary_bytes, TITLE_TIM_OFFSET).unwrap();

        for &(prot_idx, alt_offset) in TITLE_TIM_ALTERNATE_SOURCES {
            let alt_path = format!("../../extracted/PROT/{:04}_sound_data2.BIN", prot_idx);
            let alt_bytes = match std::fs::read(&alt_path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let alt = extract_title_tim(&alt_bytes, alt_offset).unwrap();
            assert_eq!(
                primary.bytes, alt.bytes,
                "PROT {} title TIM at 0x{:x} should byte-equal PROT 888",
                prot_idx, alt_offset
            );
        }
    }

    #[test]
    fn menu_band_constants_partition_the_packed_strip() {
        // The "NEW GAME CONTINUE" footer band at title-TIM y=227..237
        // is a single 128×10 strip. NEW_GAME samples the left half;
        // CONTINUE samples the right half. The two rects must abut
        // (NEW_GAME.x + NEW_GAME.w == CONTINUE.x) so engines can stack
        // them vertically without re-extracting bytes.
        let (ngx, ngy, ngw, ngh) = TITLE_BAND_MENU_NEW_GAME;
        let (cox, coy, _cow, coh) = TITLE_BAND_MENU_CONTINUE;
        assert_eq!(ngy, 227);
        assert_eq!(coy, 227);
        assert_eq!(ngh, 10);
        assert_eq!(coh, 10);
        assert_eq!(ngx + ngw, cox);
    }

    #[test]
    fn constants_are_internally_consistent() {
        // 256x256 8bpp + 256-colour CLUT.
        assert_eq!(TITLE_TIM_SIZE, 8 + (12 + 256 * 2) + (12 + 256 * 256));
        // 256x256 4bpp + 256-colour CLUT.
        assert_eq!(
            OVERLAY_SAVE_MENU_TIM_SIZE,
            8 + (12 + 256 * 2) + (12 + 128 * 256)
        );
        // 256x16 4bpp + 256-colour CLUT.
        assert_eq!(
            OVERLAY_CARD_ICON_TIM_SIZE,
            8 + (12 + 256 * 2) + (12 + 128 * 16)
        );
        // 256x192 4bpp + 16x16 CLUT block.
        assert_eq!(
            OVERLAY_SYSTEM_UI_TIM_SIZE,
            8 + (12 + 16 * 16 * 2) + (12 + 64 * 192 * 2)
        );
        // Alternate-source list shouldn't include the primary.
        for &(idx, _) in TITLE_TIM_ALTERNATE_SOURCES {
            assert_ne!(idx, PROT_INDEX_PRIMARY);
        }
    }

    #[test]
    fn panel_9slice_tiles_partition_the_retail_panel() {
        // The retail load-screen panel is 81x29 at dst (6, 4). It's
        // composed from four 4x4 corners, top + bottom edges (24x4
        // tiles repeated 3x with a 1x4 remainder), and left + right
        // edges (4x21). Verify the geometry math is internally
        // consistent: the corners + edges must exactly tile a 81x29
        // frame border.
        let (_, _, panel_w, panel_h) = OVERLAY_SAVE_PANEL_RETAIL_DST;
        assert_eq!((panel_w, panel_h), (81, 29));

        // Corner tile width / height.
        let (_, _, tl_w, tl_h) = OVERLAY_SYSTEM_UI_PANEL_TL;
        assert_eq!((tl_w, tl_h), (4, 4));
        // All four corners share width/height (sanity).
        for corner in [
            OVERLAY_SYSTEM_UI_PANEL_TL,
            OVERLAY_SYSTEM_UI_PANEL_TR,
            OVERLAY_SYSTEM_UI_PANEL_BL,
            OVERLAY_SYSTEM_UI_PANEL_BR,
        ] {
            assert_eq!((corner.2, corner.3), (tl_w, tl_h));
        }

        // Top + bottom edge tiles share dimensions: 24x4.
        for edge in [OVERLAY_SYSTEM_UI_PANEL_TOP, OVERLAY_SYSTEM_UI_PANEL_BOT] {
            assert_eq!((edge.2, edge.3), (24, 4));
        }
        // Left + right edge tiles share dimensions: 4x21.
        for edge in [OVERLAY_SYSTEM_UI_PANEL_LEFT, OVERLAY_SYSTEM_UI_PANEL_RIGHT] {
            assert_eq!((edge.2, edge.3), (4, 21));
        }

        // Top edge tiles between the two 4-wide corners must cover
        // `81 - 4 - 4 = 73` horizontal pixels. Retail does this with
        // 3x (24-wide) + 1x (1-wide remainder) = 72 + 1 = 73 ✓.
        assert_eq!(panel_w - 2 * tl_w, 73);
        assert_eq!(3 * OVERLAY_SYSTEM_UI_PANEL_TOP.2 + 1, 73);

        // Vertical content between top + bottom edges must be 21
        // (panel_h=29, minus two 4-tall edge bands = 21).
        assert_eq!(panel_h - 2 * OVERLAY_SYSTEM_UI_PANEL_TOP.3, 21);
        assert_eq!(OVERLAY_SYSTEM_UI_PANEL_LEFT.3, 21);
        assert_eq!(OVERLAY_SYSTEM_UI_PANEL_RIGHT.3, 21);
    }
}
