//! Disc-gated texture-replacement oracles.
//!
//! Four claims against the real disc:
//!
//! 1. **Encoder round trip** - for every raw-catalog TIM,
//!    `encode(decode(tim))` reproduces the original **bytes** exactly (the
//!    positional-reuse design makes this hold universally, not just
//!    pixel-wise).
//! 2. **Raw replacement** - replace a real texture with an edited image,
//!    re-decode it off the patched image pixel-exactly, keep every touched
//!    sector EDC/ECC-valid, keep the entry's format class, and stay
//!    byte-deterministic across runs.
//! 3. **LZS replacement** - same through a compressed section, including the
//!    recompress-fit gate.
//! 4. **Catalog sanity** - the raw catalog still reports the jPSXdec item
//!    count (1132) so the coordinates `tim-list` hands out stay stable.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips+passes when unset. Patched images live
//! only in memory.

use legaia_iso::iso9660::find_file_in_image;
use legaia_iso::raw::SECTOR_SIZE;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::texture::{TextureTarget, read_texture, replace_texture, texture_catalogs};
use legaia_tim::encode::{EncodeOptions, encode_replacement};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Every physical sector of `PROT.DAT` that differs between `a` and `b` must
/// still be EDC/ECC-valid in `b`.
fn assert_touched_sectors_valid(a: &[u8], b: &[u8]) -> usize {
    assert_eq!(a.len(), b.len());
    let mut touched = 0;
    for (i, (sa, sb)) in a
        .chunks_exact(SECTOR_SIZE)
        .zip(b.chunks_exact(SECTOR_SIZE))
        .enumerate()
    {
        if sa != sb {
            touched += 1;
            assert!(
                legaia_iso::write::mode2_form1_sector_is_valid(sb),
                "touched sector {i} fails EDC/ECC"
            );
        }
    }
    touched
}

#[test]
fn encoder_round_trips_every_raw_catalog_tim_byte_exactly() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let (raw, _deep) = texture_catalogs(&patcher).expect("catalogs");
    assert_eq!(raw.len(), 1132, "raw catalog is the jPSXdec item set");

    let prot = patcher.read_named_file("PROT.DAT").expect("PROT.DAT");
    let mut checked = 0usize;
    for t in &raw {
        let bytes = &prot[t.abs_offset as usize..t.abs_offset as usize + t.byte_len];
        let tim = legaia_tim::parse_strict(bytes).expect("catalog TIM strict-parses");
        let rgba = match legaia_tim::decode_rgba8(&tim, 0) {
            Ok(r) => r,
            // A handful of raw hits decode-fail (e.g. undecodable CLUT
            // geometry); the deep catalog gates on decodability, the raw one
            // does not. They are not replacement targets.
            Err(_) => continue,
        };
        let enc = encode_replacement(
            &tim,
            &rgba,
            tim.pixel_width(),
            tim.pixel_height(),
            &EncodeOptions::default(),
        )
        .expect("re-encode of own decode");
        assert_eq!(
            enc.bytes, bytes,
            "encode(decode(tim)) must be byte-identical (catalog id {})",
            t.id
        );
        assert_eq!(enc.new_palette_entries, 0);
        assert!(!enc.clut_rows_rewritten);
        checked += 1;
    }
    assert!(
        checked > 1000,
        "expected the byte-exact round trip to cover nearly the whole catalog, got {checked}"
    );
}

#[test]
fn raw_replacement_round_trips_and_keeps_sectors_valid() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let before = disc.clone();
    let mut patcher = DiscPatcher::open(disc).expect("open disc");

    // The main-title sprite sheet: raw, single-palette 8bpp in entry 890.
    let target = TextureTarget {
        entry: Some(890),
        lzs_section: None,
        offset: 0x14228,
    };
    let orig = read_texture(&patcher, &target).expect("read original");
    let (w, h) = (orig.tim.pixel_width(), orig.tim.pixel_height());

    // Edit: stamp a rectangle of an existing palette color (index-only edit,
    // exercised exactly as a user PNG edit would arrive).
    let mut rgba = legaia_tim::decode_rgba8(&orig.tim, 0).expect("decode original");
    let stamp: [u8; 4] = rgba[(200 * w + 200) * 4..(200 * w + 200) * 4 + 4]
        .try_into()
        .unwrap();
    for y in 8..40 {
        for x in 8..72 {
            rgba[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&stamp);
        }
    }

    let outcome = replace_texture(
        &mut patcher,
        &target,
        &rgba,
        w,
        h,
        &EncodeOptions::default(),
        false,
    )
    .expect("replace");
    assert_eq!(outcome.quantized_pixels, 0);

    // Pixel-exact readback off the patched image.
    let after = read_texture(&patcher, &target).expect("re-read");
    let got = legaia_tim::decode_rgba8(&after.tim, 0).expect("decode patched");
    assert_eq!(got, rgba, "patched texture must decode to the edited image");
    // VRAM placement fields survive.
    assert_eq!(after.tim.image.fb_x, orig.tim.image.fb_x);
    assert_eq!(after.tim.image.fb_y, orig.tim.image.fb_y);
    assert_eq!(
        after.tim.clut.as_ref().map(|c| (c.fb_x, c.fb_y)),
        orig.tim.clut.as_ref().map(|c| (c.fb_x, c.fb_y))
    );

    // Runtime-shaped assertion: the patched entry still classifies as the
    // same format class as before.
    let entry_after = patcher.read_entry(890).unwrap();
    let class_after = legaia_asset::categorize::classify(&entry_after).class;
    // (classify the pristine copy through a second patcher view)
    let pristine = DiscPatcher::open(before.clone()).unwrap();
    let class_before = legaia_asset::categorize::classify(&pristine.read_entry(890).unwrap()).class;
    assert_eq!(class_after, class_before, "entry format class unchanged");

    // Every touched physical sector is EDC/ECC-valid, and the touch is
    // confined to PROT.DAT.
    let patched = patcher.into_image();
    let touched = assert_touched_sectors_valid(&before, &patched);
    assert!(touched > 0, "the edit must touch at least one sector");
    let (prot_lba, prot_size) = find_file_in_image(&before, "PROT.DAT").unwrap();
    let prot_end = prot_lba as usize + (prot_size as usize).div_ceil(2048);
    for (i, (sa, sb)) in before
        .chunks_exact(SECTOR_SIZE)
        .zip(patched.chunks_exact(SECTOR_SIZE))
        .enumerate()
    {
        if sa != sb {
            assert!(
                (prot_lba as usize..prot_end).contains(&i),
                "sector {i} outside PROT.DAT was touched"
            );
        }
    }

    // Byte-determinism: the same edit on a fresh copy produces the same image.
    let mut patcher2 = DiscPatcher::open(before).unwrap();
    replace_texture(
        &mut patcher2,
        &target,
        &rgba,
        w,
        h,
        &EncodeOptions::default(),
        false,
    )
    .unwrap();
    assert_eq!(
        patcher2.into_image(),
        patched,
        "replacement must be byte-deterministic"
    );
}

#[test]
fn lzs_replacement_round_trips_through_a_compressed_section() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let before = disc.clone();
    let mut patcher = DiscPatcher::open(disc).expect("open disc");

    // First deep-catalog texture: entry 31 section 0 +0x74 (a 4bpp 256x256
    // environment page).
    let target = TextureTarget {
        entry: Some(31),
        lzs_section: Some(0),
        offset: 0x74,
    };
    let orig = read_texture(&patcher, &target).expect("read original");
    let (w, h) = (orig.tim.pixel_width(), orig.tim.pixel_height());
    let mut rgba = legaia_tim::decode_rgba8(&orig.tim, 0).expect("decode original");
    let stamp: [u8; 4] = rgba[(128 * w + 128) * 4..(128 * w + 128) * 4 + 4]
        .try_into()
        .unwrap();
    for y in 0..32 {
        for x in 0..32 {
            rgba[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&stamp);
        }
    }

    let outcome = replace_texture(
        &mut patcher,
        &target,
        &rgba,
        w,
        h,
        &EncodeOptions::default(),
        false,
    )
    .expect("replace through the LZS section");
    let fit = outcome.lzs.expect("lzs tier reports fit");
    assert!(
        fit.recompressed <= fit.capacity,
        "recompressed stream must fit the retail footprint"
    );

    // Pixel-exact readback through decompression.
    let after = read_texture(&patcher, &target).expect("re-read");
    let got = legaia_tim::decode_rgba8(&after.tim, 0).expect("decode patched");
    assert_eq!(got, rgba, "patched texture must decode to the edited image");

    // Touched sectors stay EDC/ECC-valid.
    let patched = patcher.into_image();
    let touched = assert_touched_sectors_valid(&before, &patched);
    assert!(touched > 0);
}

#[test]
fn no_op_replacement_leaves_the_image_byte_identical() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let before = disc.clone();
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let target = TextureTarget {
        entry: Some(890),
        lzs_section: None,
        offset: 0x14228,
    };
    let orig = read_texture(&patcher, &target).expect("read original");
    let rgba = legaia_tim::decode_rgba8(&orig.tim, 0).unwrap();
    replace_texture(
        &mut patcher,
        &target,
        &rgba,
        orig.tim.pixel_width(),
        orig.tim.pixel_height(),
        &EncodeOptions::default(),
        false,
    )
    .unwrap();
    assert_eq!(
        patcher.into_image(),
        before,
        "re-encoding a texture's own decode must not change a byte of the disc"
    );
}
