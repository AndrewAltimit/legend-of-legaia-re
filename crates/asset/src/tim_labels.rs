//! Semantic role labels for cataloged TIMs.
//!
//! The TIM catalog ([`crate::tim_catalog`] for raw entries,
//! [`crate::tim_deep_catalog`] for TIMs inside LZS-compressed sections) records
//! *where* every texture lives and its dimensions, but not *what* it is. This
//! module cross-references the textures this project has independently pinned -
//! through runtime captures and Ghidra-traced loader sites - and attaches a
//! human-readable role to the catalog ids that match a pin.
//!
//! ## These labels are our own annotations
//!
//! A [`TimRole`] is a derived structural annotation - which pinned asset a
//! catalog id corresponds to - not a Sony string and not pixel data. Nothing
//! here decodes or stores asset bytes; the matcher works purely from a TIM's
//! catalog coordinates (owning entry + offset) and its header geometry (CLUT
//! load position). So labeling is safe to commit alongside the rest of the
//! derived-metadata catalog.
//!
//! ## Two kinds of pin
//!
//! - **Byte-exact, single-offset pins.** The boot/title/menu textures each sit
//!   at one fixed location, pinned to a specific `(owning entry, byte offset)`
//!   on the retail NA disc. These are listed in [`RAW_PINS`]. Note the offsets
//!   are matched against the catalog's *owning-entry* addressing: where a TIM's
//!   bytes are aliased by several overlapping TOC entries (the title sprite
//!   sheet is duplicated across PROT 888/889/890), the flat catalog resolves it
//!   to the innermost owning span, so the pin names that resolved entry.
//! - **Structural family pins.** The NPC palette rows are not one texture but a
//!   whole family sharing a fixed CLUT load signature
//!   (`fb=(0, 479)`, `256 x 1`, 4bpp - the documented "row-479" band). They are
//!   matched by that signature rather than by enumerated offsets, so the rule
//!   covers every scene's NPC CLUTs in both the raw and the deep tier.
//!
//! Pins are NA-retail addresses; on another build they simply don't match and
//! the affected ids stay unlabeled (the matcher never guesses).

use legaia_tim::{PixelMode, Tim};
use serde::Serialize;

/// A human-readable role attached to a cataloged TIM that matches a known pin.
///
/// [`TimRole::as_str`] is the short label the viewer shows beside a texture's
/// dimensions. It is our own annotation - never an asset string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TimRole {
    /// Boot-time small-caps glyph atlas used by the in-game menu UI
    /// (shop / inventory / status). Pinned to the pre-`init_data` gap.
    MenuGlyphAtlas,
    /// Main title-screen sprite sheet (orb + wordmark + copyright bands).
    MainTitle,
    /// One of the four publisher / warning logos in `init.pak`
    /// (PROKION / Contrail / SCEA / WARNING).
    PublisherLogo,
    /// System-UI sheet that carries the load/save panel 9-slice chrome and the
    /// "Load" glyphs (sampled at different CLUT rows).
    LoadScreenChrome,
    /// A party-member portrait shown in the load-screen slot grid (and baked
    /// into save-card icons).
    LoadScreenPortrait,
    /// The empty-slot frame drawn for unoccupied load-screen grid cells.
    LoadScreenEmptyFrame,
    /// NPC colour table in the documented row-479 band: a wide single-row CLUT
    /// at framebuffer `(0, 479)` paired with a 4bpp character page.
    NpcPaletteRow,
}

impl TimRole {
    /// Short human-readable label for the viewer / TSV. Our own annotation.
    pub fn as_str(self) -> &'static str {
        match self {
            TimRole::MenuGlyphAtlas => "menu glyph atlas",
            TimRole::MainTitle => "main-title sprite sheet",
            TimRole::PublisherLogo => "publisher logo",
            TimRole::LoadScreenChrome => "load-screen UI sheet",
            TimRole::LoadScreenPortrait => "load-screen portrait",
            TimRole::LoadScreenEmptyFrame => "load-screen empty-slot frame",
            TimRole::NpcPaletteRow => "NPC palette row",
        }
    }
}

/// Serialize an `Option<TimRole>` as its human label string (or `null`), so
/// the catalog's serde output and the viewer's info JSON carry the readable
/// name rather than the Rust variant identifier.
pub fn serialize_role<S>(role: &Option<TimRole>, ser: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match role {
        Some(r) => ser.serialize_some(r.as_str()),
        None => ser.serialize_none(),
    }
}

/// Byte-exact `(owning entry, offset within that entry, role)` pins for the
/// boot / title / menu textures on the retail NA disc.
///
/// `entry` is `None` for TIMs in the unindexed system-UI gap that precedes the
/// first TOC entry (there the catalog's `offset_in_entry` equals the absolute
/// file offset); `Some(idx)` names the catalog's *owning* entry otherwise.
pub const RAW_PINS: &[(Option<u32>, u64, TimRole)] = &[
    // Pre-`init_data` unindexed gap (offset == absolute file offset).
    (None, 0x11218, TimRole::MenuGlyphAtlas),
    (None, 0x1AC90, TimRole::LoadScreenPortrait), // Vahn
    (None, 0x1AD50, TimRole::LoadScreenPortrait), // Noa
    (None, 0x1AE10, TimRole::LoadScreenPortrait), // Gala
    (None, 0x1AED0, TimRole::LoadScreenEmptyFrame),
    // System-UI sheet (load/save panel chrome + glyphs); owned by the TOC
    // pseudo-entry that wraps the whole header region.
    (Some(1234), 0x18E0, TimRole::LoadScreenChrome),
    // Main title sprite sheet. Aliased across PROT 888/889/890; the flat
    // catalog resolves it to the innermost owning span (890).
    (Some(890), 0x14228, TimRole::MainTitle),
    // init.pak publisher / warning logos (PROT 895).
    (Some(895), 0x021C4, TimRole::PublisherLogo), // PROKION
    (Some(895), 0x0D3E4, TimRole::PublisherLogo), // Contrail
    (Some(895), 0x18E04, TimRole::PublisherLogo), // SCEA
    (Some(895), 0x1CE44, TimRole::PublisherLogo), // WARNING
];

/// CLUT load row of the documented NPC palette band.
const NPC_PALETTE_FB_Y: u16 = 479;

/// Does this TIM carry the row-479 NPC palette CLUT signature?
///
/// The NPC colour tables share a fixed load signature regardless of which
/// scene hosts them: a 4bpp image whose CLUT block loads at framebuffer
/// `(0, 479)` and is `256` entries wide by a single row (16 stacked 16-colour
/// palettes). This is the same band documented in `docs/formats/npc-palette.md`
/// and is distinct from the many other textures that merely park a CLUT in the
/// bottom rows of VRAM (which use other `fb_x`/`fb_y`).
fn is_npc_palette_row(tim: &Tim) -> bool {
    tim.mode == PixelMode::Bpp4
        && matches!(
            &tim.clut,
            Some(c) if c.fb_x == 0 && c.fb_y == NPC_PALETTE_FB_Y && c.w == 256 && c.h == 1
        )
}

/// Look up the byte-exact pin for a raw-catalog hit, if any.
fn raw_pin(entry_index: Option<u32>, offset_in_entry: u64) -> Option<TimRole> {
    RAW_PINS
        .iter()
        .find(|&&(e, off, _)| e == entry_index && off == offset_in_entry)
        .map(|&(_, _, role)| role)
}

/// Classify a raw-catalog TIM. A byte-exact [`RAW_PINS`] match wins; otherwise
/// the structural NPC-palette signature applies. Returns `None` for unpinned
/// textures.
pub fn classify_raw(entry_index: Option<u32>, offset_in_entry: u64, tim: &Tim) -> Option<TimRole> {
    raw_pin(entry_index, offset_in_entry).or_else(|| {
        if is_npc_palette_row(tim) {
            Some(TimRole::NpcPaletteRow)
        } else {
            None
        }
    })
}

/// Classify a deep-catalog (LZS-embedded) TIM. The boot/title/menu textures are
/// all stored raw, so only the structural NPC-palette family reaches the deep
/// tier (e.g. town scenes whose CLUTs sit inside compressed sections).
pub fn classify_deep(tim: &Tim) -> Option<TimRole> {
    if is_npc_palette_row(tim) {
        Some(TimRole::NpcPaletteRow)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_tim::Clut;

    fn tim_with_clut(mode: PixelMode, fb_x: u16, fb_y: u16, w: u16, h: u16) -> Tim {
        Tim {
            flags: 0,
            mode,
            clut: Some(Clut {
                fb_x,
                fb_y,
                w,
                h,
                entries: vec![0u16; (w as usize) * (h as usize)],
            }),
            image: legaia_tim::Image {
                fb_x: 832,
                fb_y: 0,
                fb_w: 64,
                h: 256,
                data: Vec::new(),
            },
        }
    }

    #[test]
    fn raw_pins_resolve_each_family() {
        let dummy = tim_with_clut(PixelMode::Bpp8, 0, 0, 256, 1);
        assert_eq!(
            classify_raw(None, 0x11218, &dummy),
            Some(TimRole::MenuGlyphAtlas)
        );
        assert_eq!(
            classify_raw(None, 0x1AC90, &dummy),
            Some(TimRole::LoadScreenPortrait)
        );
        assert_eq!(
            classify_raw(None, 0x1AED0, &dummy),
            Some(TimRole::LoadScreenEmptyFrame)
        );
        assert_eq!(
            classify_raw(Some(1234), 0x18E0, &dummy),
            Some(TimRole::LoadScreenChrome)
        );
        assert_eq!(
            classify_raw(Some(890), 0x14228, &dummy),
            Some(TimRole::MainTitle)
        );
        assert_eq!(
            classify_raw(Some(895), 0x021C4, &dummy),
            Some(TimRole::PublisherLogo)
        );
    }

    #[test]
    fn npc_signature_matches_only_the_documented_band() {
        // 4bpp, CLUT at (0,479), 256x1 -> NPC palette row.
        let npc = tim_with_clut(PixelMode::Bpp4, 0, NPC_PALETTE_FB_Y, 256, 1);
        assert!(is_npc_palette_row(&npc));
        assert_eq!(classify_deep(&npc), Some(TimRole::NpcPaletteRow));

        // Same band but a single 16-colour palette (cw=16) is NOT the NPC
        // signature - that is the common "park a CLUT in the bottom rows" case.
        let other = tim_with_clut(PixelMode::Bpp4, 0, NPC_PALETTE_FB_Y, 16, 1);
        assert!(!is_npc_palette_row(&other));

        // Wrong row, wrong fb_x, and 8bpp all fail.
        assert!(!is_npc_palette_row(&tim_with_clut(
            PixelMode::Bpp4,
            0,
            480,
            256,
            1
        )));
        assert!(!is_npc_palette_row(&tim_with_clut(
            PixelMode::Bpp4,
            16,
            NPC_PALETTE_FB_Y,
            256,
            1
        )));
        assert!(!is_npc_palette_row(&tim_with_clut(
            PixelMode::Bpp8,
            0,
            NPC_PALETTE_FB_Y,
            256,
            1
        )));
    }

    #[test]
    fn byte_exact_pin_wins_over_structural_rule() {
        // A TIM that both sits at a pinned offset AND carries the NPC signature
        // is labeled by the specific pin, not the family rule.
        let npc = tim_with_clut(PixelMode::Bpp4, 0, NPC_PALETTE_FB_Y, 256, 1);
        assert_eq!(
            classify_raw(Some(895), 0x021C4, &npc),
            Some(TimRole::PublisherLogo)
        );
    }

    #[test]
    fn unpinned_returns_none() {
        let plain = tim_with_clut(PixelMode::Bpp16, 100, 100, 0, 0);
        assert_eq!(classify_raw(Some(5), 0x1234, &plain), None);
        assert_eq!(classify_deep(&plain), None);
    }
}
