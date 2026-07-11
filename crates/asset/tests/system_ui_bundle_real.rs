//! Disc-gated regression for [`legaia_asset::system_ui_bundle`]: the
//! boot-resident system-UI TIM bundle at raw PROT TOC entries 0 and 1.
//!
//! Pins the retail bundle shape (20-member pack at raw entry 0 with 14
//! parseable TIMs, 1-member pack at raw entry 1), the pinned member
//! sub-assets (the menu-glyph / interior-page atlas at `PROT.DAT[0x11218]`
//! = entry-relative `0xFA18`; the UI sprite strip at `0x19438`; the system
//! sprite sheet at `0x018E0`), and the `FUN_800198E0` flat-strip upload
//! semantics (row-510/511 strips, raw-entry-1 row-480 strip).
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset or `PROT.DAT` is
//! missing.

use std::path::PathBuf;

use legaia_asset::{interior_page, system_ui_bundle};

fn prot_dat() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT.DAT", "../../extracted/PROT.DAT"] {
        let f = PathBuf::from(p);
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

#[test]
fn system_ui_bundle_parses_and_uploads_strips() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };

    let bundle = system_ui_bundle::read_from_prot_dat(&prot).expect("bundle parses");
    // Retail pack shape: raw entry 0 declares 20 members (6 of them the
    // non-TIM row-patch strips), raw entry 1 declares 1.
    assert_eq!(bundle.member_counts, [20, 1], "pack member counts");
    assert_eq!(bundle.tims.len(), 15, "parseable TIM members");
    // The six atlas row patches: single rows at (960, 456..=458 and
    // 460..=462), declared 256 words wide (VRAM-edge-clipped to 64 at
    // upload). Byte-verified against live captures: the patched rows are
    // exactly what retail VRAM holds over the disc atlas image.
    assert_eq!(bundle.row_patches.len(), 6, "atlas row-patch members");
    let patch_rows: Vec<u16> = bundle.row_patches.iter().map(|p| p.fb_y).collect();
    assert_eq!(patch_rows, [456, 457, 458, 460, 461, 462], "patched rows");
    for p in &bundle.row_patches {
        assert_eq!((p.fb_x, p.w_words, p.h), (960, 256, 1), "patch rect");
        assert_eq!(p.raw_entry, 0);
        assert!((10..=15).contains(&p.member), "patch member slot");
    }

    // The atlas (interior page): raw entry 0, entry-relative 0xFA18
    // (= absolute PROT.DAT 0x11218 with the entry starting at sector 3).
    let atlas = bundle
        .tims
        .iter()
        .find(|m| {
            (
                m.tim.image.fb_x,
                m.tim.image.fb_y,
                m.tim.image.fb_w,
                m.tim.image.h,
            ) == interior_page::IMAGE_RECT
        })
        .expect("atlas member present");
    assert_eq!(atlas.raw_entry, 0);
    assert_eq!(atlas.entry_offset, 0xFA18, "atlas entry-relative offset");
    assert_eq!(
        atlas.clut_strip_rect(),
        Some((0, 510, 256, 1)),
        "16x16 CLUT bank flattens to a 256-entry row-510 strip"
    );

    // The row-510 sibling (UI sprite strip at PROT.DAT[0x19438]) and the
    // row-511 sheet (PROT.DAT[0x018E0]) - the docs/formats/npc-palette.md
    // "Boot-resident strip band" table.
    let strips = bundle.clut_strip_rects();
    assert!(strips.contains(&(256, 510, 64, 1)), "0x19438 strip");
    assert!(strips.contains(&(0, 511, 256, 1)), "0x018E0 strip");
    assert!(strips.contains(&(256, 511, 48, 1)), "boot cursor strip");
    assert!(strips.contains(&(304, 511, 16, 1)), "0x07B00 strip");
    // Raw entry 1: the single row-480 CLUT TIM at image (640, 0).
    let raw1: Vec<_> = bundle.tims.iter().filter(|m| m.raw_entry == 1).collect();
    assert_eq!(raw1.len(), 1, "raw entry 1 carries one TIM");
    assert_eq!(raw1[0].clut_strip_rect(), Some((0, 480, 256, 1)));
    assert_eq!((raw1[0].tim.image.fb_x, raw1[0].tim.image.fb_y), (640, 0));

    // Upload: the strip band + the atlas page land where the env meshes
    // sample them (town01 slots 21/26/74 / rikuroa slots 50/51/63:
    // CBA (64,510), texpage (960,256)).
    let mut vram = legaia_tim::Vram::new();
    bundle.upload_to_vram(&mut vram);
    let lit_510 = (0..256).filter(|&x| vram.pixel(x, 510) != 0).count();
    assert!(lit_510 > 200, "row-510 strip populated (lit={lit_510})");
    let pal_lit = (65..80).filter(|&x| vram.pixel(x, 510) != 0).count();
    assert!(
        pal_lit >= 12,
        "CBA (64,510) palette populated ({pal_lit}/15)"
    );
    let lit_511 = (0..256).filter(|&x| vram.pixel(x, 511) != 0).count();
    assert!(lit_511 > 100, "row-511 strip populated (lit={lit_511})");
    assert!(vram.region_has_data(960, 256, 64, 256), "atlas image page");
    assert!(vram.region_has_data(896, 256, 64, 192), "sprite sheet page");

    // Pack-order overwrite: the sprite strip's image (960,400) 60x24 and
    // the cursor parts (976,256..) land INSIDE the atlas page after it -
    // the byte-identical reproduction of the retail "~70 overwritten
    // atlas rows". Assert the overlay members exist and follow the atlas
    // in upload order.
    let atlas_pos = bundle
        .tims
        .iter()
        .position(|m| m.entry_offset == 0xFA18)
        .unwrap();
    let overlay_pos = bundle
        .tims
        .iter()
        .position(|m| (m.tim.image.fb_x, m.tim.image.fb_y) == (960, 400))
        .expect("UI sprite strip member present");
    assert!(
        overlay_pos > atlas_pos,
        "sprite strip uploads after the atlas (pack order)"
    );
}

#[test]
fn system_ui_bundle_matches_interior_page_extractor() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    // The single-TIM extractor and the bundle member must be the same
    // bytes: same CLUT entries, same image data.
    let tim = interior_page::read_from_prot_dat(&prot).expect("interior page TIM");
    let bundle = system_ui_bundle::read_from_prot_dat(&prot).expect("bundle parses");
    let member = bundle
        .tims
        .iter()
        .find(|m| m.entry_offset == 0xFA18)
        .expect("atlas member");
    assert_eq!(
        tim.clut.as_ref().unwrap().entries,
        member.tim.clut.as_ref().unwrap().entries,
        "CLUT entries identical"
    );
    assert_eq!(
        tim.image.data, member.tim.image.data,
        "image bytes identical"
    );
}
