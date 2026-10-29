//! The desktop programs' icons: the game (`legend-of-legaia`) and Legaia
//! Explorer (`legaia-explorer`). Original artwork, drawn by
//! `icons/make_icons.py`, which writes every file beside it: the SVG source,
//! a 256 x 256 PNG (the runtime window icon), a `.ico` (embedded into the
//! Windows executables, [`crate::win_icon`]) and a `.icns` (the macOS app
//! bundles, `scripts/ci/release-build.sh`).

/// The game's icon, 256 x 256 RGBA PNG: the launcher and play window's
/// window icon.
pub const GAME_PNG: &[u8] = include_bytes!("../icons/legend-of-legaia.png");

/// Legaia Explorer's icon, 256 x 256 RGBA PNG: its window icon.
pub const EXPLORER_PNG: &[u8] = include_bytes!("../icons/legaia-explorer.png");

/// The game's `.ico`, as an absolute path a build script hands to
/// [`crate::win_icon::embed_in_bins`].
pub const GAME_ICO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/icons/legend-of-legaia.ico");

/// Legaia Explorer's `.ico`, as an absolute path.
pub const EXPLORER_ICO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/icons/legaia-explorer.ico");

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG's width and height, from its IHDR chunk.
    fn png_size(png: &[u8]) -> (u32, u32) {
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&png[12..16], b"IHDR");
        let be = |o: usize| u32::from_be_bytes([png[o], png[o + 1], png[o + 2], png[o + 3]]);
        (be(16), be(20))
    }

    #[test]
    fn window_icons_are_256_square_rgba() {
        for png in [GAME_PNG, EXPLORER_PNG] {
            assert_eq!(png_size(png), (256, 256));
            // Bit depth 8, colour type 6: RGBA.
            assert_eq!((png[24], png[25]), (8, 6));
        }
    }

    #[test]
    fn ico_files_hold_a_256_image() {
        for ico in [GAME_ICO, EXPLORER_ICO] {
            let b = std::fs::read(ico).unwrap();
            // ICONDIR: reserved 0, type 1 (icon), then the image count.
            assert_eq!(&b[..4], &[0, 0, 1, 0]);
            let count = u16::from_le_bytes([b[4], b[5]]) as usize;
            // A width byte of 0 in an ICONDIRENTRY means 256.
            assert!(
                (0..count).any(|i| b[6 + 16 * i] == 0),
                "{ico}: no 256 px image"
            );
        }
    }
}
