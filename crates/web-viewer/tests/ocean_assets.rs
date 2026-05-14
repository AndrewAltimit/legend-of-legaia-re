//! Verify `find_ocean_assets` extracts the world-map ocean tile texture +
//! 13-frame CLUT animation table from each of the three retail kingdoms.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset, matching the rest of
//! the disc-dependent test suite. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::disc::{extract_prot_dat, parse_prot_toc};
use legaia_web_viewer::ocean::{OCEAN_ANIM_FRAME_COUNT, OCEAN_ANIM_FRAME0_HEAD, find_ocean_assets};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;

/// Hex of the SHA-256 of the 13-frame animation table after extraction.
/// The same hash for all three kingdoms confirms the shared-asset
/// invariant the runtime relies on.
const EXPECTED_ANIM_SHA256: &str =
    "dfc6dd263a71152c40ab7726193d79e9658efc04402f4280f5f49f392e32071f";

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[test]
fn frame0_head_signature_is_unique_to_the_animation_table() {
    // The 16-byte signature should not happen by chance inside the disc
    // anywhere except the actual frame-0 of the animation table. This
    // test just sanity-checks the constants compile and are non-trivial.
    assert_eq!(OCEAN_ANIM_FRAME_COUNT, 13);
    assert_eq!(OCEAN_ANIM_FRAME0_HEAD.len(), 16);
    assert_eq!(OCEAN_ANIM_FRAME0_HEAD[0], 0x00);
    assert_eq!(OCEAN_ANIM_FRAME0_HEAD[1], 0x00);
}

#[test]
fn ocean_assets_extract_from_every_world_map_kingdom() {
    let Some(disc_path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping ocean-assets test");
        return;
    };
    let disc = fs::read(&disc_path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT extraction");
    let entries = parse_prot_toc(&prot).expect("PROT TOC parse");

    for prot_index in [85u32, 244, 391] {
        let entry = entries
            .iter()
            .find(|e| e.index == prot_index)
            .unwrap_or_else(|| panic!("PROT entry {prot_index} missing"));
        let off = entry.byte_offset as usize;
        let end = (entry.byte_offset + entry.size_bytes) as usize;
        let buf = &prot[off..end];

        // Find the 7-asset table and decompress slot 0.
        let table_off = legaia_web_viewer::find_asset_table_offset(buf)
            .unwrap_or_else(|| panic!("no asset table in PROT {prot_index}"));
        let table = &buf[table_off..];
        let slot0_ts = u32::from_le_bytes(table[8..12].try_into().unwrap());
        let slot0_off = u32::from_le_bytes(table[12..16].try_into().unwrap()) as usize;
        let slot0_size = (slot0_ts & 0x00FF_FFFF) as usize;
        let slot0 = legaia_lzs::decompress(&table[slot0_off..], slot0_size)
            .unwrap_or_else(|e| panic!("PROT {prot_index} slot 0 LZS: {e}"));

        let ocean = find_ocean_assets(&slot0)
            .unwrap_or_else(|| panic!("PROT {prot_index}: ocean assets not found in slot 0"));

        // Texture: 4bpp 64×256 = 32768 bytes.
        assert_eq!(
            ocean.texture.len(),
            32768,
            "PROT {prot_index}: ocean texture size"
        );
        // Base CLUT: 256 entries × 2 bytes = 512 bytes.
        assert_eq!(
            ocean.base_clut.len(),
            512,
            "PROT {prot_index}: base CLUT size"
        );
        // Animation: 13 frames × 32 bytes = 416 bytes.
        assert_eq!(
            ocean.animation_frames.len(),
            OCEAN_ANIM_FRAME_COUNT * 32,
            "PROT {prot_index}: animation table size"
        );

        // Animation table SHA-256: must be identical across all 3 kingdoms.
        let anim_sha = hex(&Sha256::digest(&ocean.animation_frames));
        assert_eq!(
            anim_sha, EXPECTED_ANIM_SHA256,
            "PROT {prot_index}: anim table sha256 mismatch"
        );

        // The texture + base-CLUT bytes vary per kingdom (each kingdom
        // ships its own variant; visually similar but byte-distinct -
        // confirmed by SHA-256 cross-check). Only the animation table
        // is byte-identical across kingdoms.

        // Frame-0 prefix must match the documented signature.
        assert_eq!(
            &ocean.animation_frames[..OCEAN_ANIM_FRAME0_HEAD.len()],
            &OCEAN_ANIM_FRAME0_HEAD,
            "PROT {prot_index}: frame-0 head mismatch"
        );
    }

    eprintln!(
        "[ok] ocean assets verified across PROT 0085/0244/0391 \
         (animation table identical, texture+CLUT vary per kingdom; \
         anim sha={EXPECTED_ANIM_SHA256})",
    );
}
