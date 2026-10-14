//! The intro quad descriptor table read out of the overlay.

use crate::battle_intro_styles as styles;
use crate::battle_intro_transition::{INTRO_QUAD_DESC_STRIDE, IntroQuadDesc};

/// Overlay VA of the transition sprite descriptor table, inside PROT 0979
/// `field_battle_intro`.
pub const INTRO_QUAD_TABLE_VA: u32 = 0x801D_1EC4;

/// Records of [`INTRO_QUAD_TABLE_VA`] the curtain actually indexes.
///
/// The style patches indices `2` and `3` and draws them; `0` and `1` are the
/// full-screen halves (`0xC0`- and `0x80`-wide, `0xF0` tall) the mid-pass
/// rectangles use. Past `3` the bytes stop being descriptor-shaped, which is
/// what bounds the table - it carries no count.
pub const INTRO_QUAD_TABLE_LEN: usize = 4;

/// The parsed descriptor table.
///
/// Held by value because [`crate::battle_intro_styles::tick_curtain`]
/// **patches records in place** before every quad - that is retail's own
/// mechanism, not a port shortcut - so the table is per-transition mutable
/// state rather than a shared read-only asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntroQuadTable(pub Vec<IntroQuadDesc>);

impl IntroQuadTable {
    /// Parse the table out of a PROT 0979 image already relocated to its load
    /// base (`legaia_overlay_images::static_overlay::as_loaded`).
    ///
    /// Returns `None` when the image is too short to hold the table, which is
    /// the only failure that can be detected structurally - the records carry
    /// no magic.
    pub fn parse_overlay(as_loaded: &[u8], base_va: u32) -> Option<Self> {
        let off = INTRO_QUAD_TABLE_VA.checked_sub(base_va)? as usize;
        let mut out = Vec::with_capacity(INTRO_QUAD_TABLE_LEN);
        for i in 0..INTRO_QUAD_TABLE_LEN {
            let at = off.checked_add(i * INTRO_QUAD_DESC_STRIDE)?;
            out.push(IntroQuadDesc::parse(as_loaded.get(at..)?)?);
        }
        Some(Self(out))
    }

    /// A neutral stand-in with the shape the curtain needs, for hosts that
    /// have no disc image to hand. Unity scale, white on both edges - so the
    /// strips carry the captured frame unmodulated.
    ///
    /// The four extents are **not** interchangeable, and a uniform `1 x 1`
    /// stand-in silently broke the column pass: `tick_curtain` patches `w` and
    /// `v0` on the row record but only `u0` and `tpage` on the column one, so
    /// the column strip's height comes from the table. The values here are the
    /// ones `battle_intro_table_real.rs` asserts of the disc records: records
    /// `0` / `1` the two full-screen halves, `2` a full-height column, `3` a
    /// single scanline.
    pub fn neutral() -> Self {
        let desc = |w: u8, h: u8| IntroQuadDesc {
            size_q12: 0x1000,
            w,
            h,
            top: [0xFF; 3],
            bottom: [0xFF; 3],
            ..Default::default()
        };
        Self(vec![
            desc(styles::CURTAIN_LEFT_W, styles::CURTAIN_ROWS as u8),
            desc(styles::CURTAIN_RIGHT_W, styles::CURTAIN_ROWS as u8),
            desc(1, styles::CURTAIN_ROWS as u8),
            desc(1, 1),
        ])
    }
}
