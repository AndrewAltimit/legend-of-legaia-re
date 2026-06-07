//! Resolve the world-map ocean tile assets out of the real kingdom bundles.
//! The 13-frame animation table is the shared global asset (byte-identical
//! across the three kingdoms); the tile texture + base CLUT are per-kingdom.
//! Skips and passes when the extracted PROT entries aren't on disk (the
//! disc-gated skip pattern).

use legaia_asset::kingdom_bundle;
use legaia_asset::ocean::{OCEAN_ANIM_FRAME_COUNT, find_ocean_assets};
use std::path::PathBuf;

fn workspace() -> Option<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()
        .map(PathBuf::from)
}

#[test]
fn ocean_assets_resolve_and_match_across_kingdoms_or_skip() {
    let Some(ws) = workspace() else { return };
    // Drake / Sebucus / Karisto world-map kingdom bundles (PROT 0085 / 0244 / 0391).
    let kingdoms = ["0085_map01.BIN", "0244_map02.BIN", "0391_map03.BIN"];
    let prot = ws.join("extracted").join("PROT");
    if !kingdoms.iter().all(|k| prot.join(k).is_file()) {
        eprintln!("extracted kingdom bundles not present - skipping");
        return;
    }

    let mut shared_anim: Option<Vec<u8>> = None;
    let mut prev_texture: Option<Vec<u8>> = None;
    for k in kingdoms {
        let buf = std::fs::read(prot.join(k)).expect("read kingdom bundle");
        let slot0 = kingdom_bundle::decode_slot(&buf, 0).expect("decode kingdom slot 0");
        let ocean = find_ocean_assets(&slot0).unwrap_or_else(|| panic!("no ocean assets in {k}"));

        // Texture: 64 halfwords * 256 rows * 2 bytes = 32768 bytes (4bpp tile).
        assert_eq!(ocean.texture.len(), 64 * 256 * 2, "{k}: texture size");
        // Base CLUT: 256 BGR555 entries * 2 bytes.
        assert_eq!(ocean.base_clut.len(), 256 * 2, "{k}: base CLUT size");
        // Animation table: 13 frames * 16 entries * 2 bytes.
        assert_eq!(
            ocean.animation_frames.len(),
            OCEAN_ANIM_FRAME_COUNT * 32,
            "{k}: animation frame bytes"
        );

        // The animation table is the shared global asset - byte-identical
        // across kingdoms.
        match &shared_anim {
            None => shared_anim = Some(ocean.animation_frames.clone()),
            Some(a) => assert_eq!(
                &ocean.animation_frames, a,
                "{k}: animation table differs - it should be the shared global table"
            ),
        }
        // The tile texture is per-kingdom: each kingdom's coastline/water tile
        // differs, so consecutive kingdoms must NOT have an identical texture.
        if let Some(prev) = &prev_texture {
            assert_ne!(
                &ocean.texture, prev,
                "{k}: texture matched the previous kingdom - expected a per-kingdom tile"
            );
        }
        prev_texture = Some(ocean.texture);
    }
}
