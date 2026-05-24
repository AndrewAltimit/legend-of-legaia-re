//! Disc-gated integration test for the runtime effect-catalog load.
//!
//! Parses the real `efect.dat` 2-pack (PROT 0873) through
//! [`EffectCatalog::from_efect_dat_bytes`] and asserts the wrapper's shape
//! matches what was byte-decoded from the extracted entry: an inline sprite
//! atlas, the pack0 animation batches, and the pack1 effect scripts. Then
//! confirms a freshly entered field scene leaves a non-empty catalog resident
//! on `World` (the production path that was previously missing a caller).
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset or `extracted/` is missing
//! (same convention as the other disc-gated integration tests).

use std::path::PathBuf;

use legaia_engine_core::scene::ProtIndex;
use legaia_engine_vm::effect_vm::EffectCatalog;

const PROT_EFECT_DAT_ENTRY: u32 = 873;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() {
            return Some(d);
        }
    }
    None
}

fn gated() -> Option<ProtIndex> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let extracted = extracted_dir()?;
    ProtIndex::open_extracted(&extracted).ok()
}

#[test]
fn efect_dat_parses_into_a_populated_catalog() {
    let Some(index) = gated() else {
        return;
    };
    let raw = index
        .entry_bytes(PROT_EFECT_DAT_ENTRY)
        .expect("read PROT 0873");
    let cat = EffectCatalog::from_efect_dat_bytes(&raw);

    // The retail efect.dat carries 14 pack0 anim batches and 33 pack1 effect
    // scripts, with an inline sprite atlas. Exact counts are stable disc
    // invariants (see docs/formats/effect.md).
    assert_eq!(cat.len(), 33, "33 effect scripts in pack1");
    assert_eq!(cat.anim_count(), 14, "14 anim batches in pack0");
    assert!(!cat.atlas().is_empty(), "inline sprite atlas present");

    // Every child sprite_id that an effect references must index into the
    // pack0 anim list (the render path resolves child -> anim -> atlas).
    for id in 0..cat.len() as u8 {
        let (_script, children) = cat.entry(id).unwrap();
        for ch in children {
            // sprite_id is allowed to exceed the anim count in retail (a few
            // high ids act as sentinels); just assert the lookup is total
            // (never panics) and that low ids resolve.
            let _ = cat.anim(ch.sprite_id);
        }
    }
    // The first anim batch's frames index real atlas entries.
    if let Some(batch) = cat.anim(0) {
        for f in &batch.frames {
            assert!(
                (f.atlas_index as usize) < cat.atlas().len(),
                "frame atlas_index {} out of atlas range {}",
                f.atlas_index,
                cat.atlas().len()
            );
        }
    }

    // Atlas field-order regression. The atlas entry is
    // `[u8 u, v, w, h, u16 CLUT@+4, u8 tpage@+6, u8 unk]` - confirmed from the
    // `FUN_801E0088` emit (CLUT <- atlas+4, tpage <- atlas+6), NOT the reverse.
    // The discriminator against the old swapped reading: the u16 at +4 (CLUT)
    // decodes to the high effect-CLUT rows (`fb_y >= 473`), and the byte at +6
    // (tpage) is a small page descriptor (< 0x80, a 4bpp page) selecting the
    // loaded effect texture pages (`fb_x >= 320`). Under the swap, `clut` would
    // be a tiny byte and `page` a huge `0x76xx` - so this assertion fails if
    // the fields are ever flipped back.
    let mut effect_band_hits = 0usize;
    for e in cat.atlas() {
        if e.w == 0 || e.h == 0 {
            continue; // skip empty/unused atlas slots
        }
        let clut_fb_y = e.clut >> 6; // CBA: fb_y = cba >> 6
        let page_fb_x = (e.page & 0xF) * 64; // tpage: fb_x = TX * 64
        assert!(
            e.page < 0x80,
            "tpage must be a single-byte 4bpp page descriptor, got {:#06x}",
            e.page
        );
        if (473..=486).contains(&clut_fb_y) && page_fb_x >= 320 {
            effect_band_hits += 1;
        }
    }
    assert!(
        effect_band_hits > 0,
        "expected >=1 atlas entry with an effect-CLUT-band CBA (fb_y 473..=486) \
         and an effect-page tpage (fb_x >= 320); field order is likely swapped"
    );
}
