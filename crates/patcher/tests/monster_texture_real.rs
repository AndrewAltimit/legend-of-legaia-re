//! Disc-gated oracle for [`legaia_patcher::monster_texture`] - the enemy and
//! boss battle skins inside the monster archive (PROT 867).
//!
//! What this proves on a scratch copy of the user's own disc: every
//! populated monster enumerates with its name, the anchor pages decode at
//! the geometry the loader gives them, an unedited round-trip writes not one
//! byte, a real repaint lands and reads back, an edit using a colour the
//! monster's palettes do not hold is refused **before** anything is written,
//! and the same edit twice is byte-identical.
//!
//! It also pins the finding that made the composite decode necessary:
//! decoding a page through palette 0 - the obvious convention - paints
//! Songi's transformed form (id 179) in colours nothing on the model
//! samples. No decoded pixels are written anywhere; only shapes, counts and
//! equality against what was read back. Skips + passes without
//! `LEGAIA_DISC_BIN`.

use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::monster_texture::{self as mt, MonsterTextureTarget};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Songi's transformed form. Three monsters are called "Songi" (76, 136,
/// 179), which is exactly why a row has to carry its id.
const SONGI_FINAL: MonsterTextureTarget = MonsterTextureTarget { id: 179 };
/// Songi as first fought.
const SONGI_FIRST: MonsterTextureTarget = MonsterTextureTarget { id: 76 };

/// Repaint every texel a primitive samples with the colour of the first
/// texel that shares its palette. A real edit that cannot introduce a colour
/// its own region does not already hold, so it exercises the pixel path
/// without dragging the palette question in.
fn flatten_regions(rgba: &[u8], owner: &[Option<u8>]) -> Vec<u8> {
    let mut first: std::collections::HashMap<u8, [u8; 4]> = std::collections::HashMap::new();
    for (i, o) in owner.iter().enumerate() {
        if let Some(p) = o {
            first.entry(*p).or_insert([
                rgba[i * 4],
                rgba[i * 4 + 1],
                rgba[i * 4 + 2],
                rgba[i * 4 + 3],
            ]);
        }
    }
    let mut out = rgba.to_vec();
    for (i, o) in owner.iter().enumerate() {
        if let Some(p) = o
            && let Some(c) = first.get(p)
        {
            out[i * 4..i * 4 + 4].copy_from_slice(c);
        }
    }
    out
}

#[test]
fn every_populated_monster_enumerates_with_its_name() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open");
    let pages = mt::catalog(&patcher).expect("catalog");

    assert_eq!(pages.len(), 186, "populated monster pages off the disc");
    assert!(
        pages.iter().all(|p| !p.name.is_empty()),
        "a row with no name is not searchable, which is the whole point"
    );
    assert!(
        pages.iter().all(|p| p.height() == 256),
        "the loader's StoreImage rect is always 256 rows tall"
    );
    assert!(
        pages.iter().all(|p| p.width() == 128 || p.width() == 256),
        "pages are 128 or 256 texels wide"
    );
    // Ids are 1-based and strictly increasing, so `section` addresses a row.
    assert!(pages.windows(2).all(|w| w[0].id < w[1].id));

    // The anchor the user searched for. A bare name is ambiguous here.
    let songi: Vec<u16> = pages
        .iter()
        .filter(|p| p.name == "Songi")
        .map(|p| p.id)
        .collect();
    assert_eq!(songi, vec![76, 136, 179], "three monsters are called Songi");
    for id in songi {
        let p = pages.iter().find(|p| p.id == id).unwrap();
        assert_eq!((p.width(), p.height()), (256, 256), "Songi #{id}");
    }
}

/// The reason the decode is a composite rather than "palette 0".
///
/// A monster page has no single colouring: a primitive picks a palette with
/// its CBA column. Decoding id 179 through palette 0 renders the 44% of its
/// page that lives on indices 14 and 15 as pure red and pure green - the
/// green/red checkerboard a whole-page export shows. Through the palettes
/// the model actually samples, those texels are not that colour, and the
/// texels no primitive samples are dead bytes rather than art.
#[test]
fn the_composite_decode_does_not_paint_dead_bytes_as_art() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open");
    let page = mt::read_page(&patcher, &SONGI_FINAL).expect("read page");
    let owner = page.ownership();

    let dead = owner.iter().filter(|o| o.is_none()).count();
    assert!(
        dead > 0,
        "this page has an unsampled region - that is what the fallback is about"
    );
    let composite = page.rgba(&owner);
    assert_eq!(composite.len(), 256 * 256 * 4);
    // Every dead texel is blank in the composite, whatever index it holds.
    assert!(
        owner
            .iter()
            .enumerate()
            .filter(|(_, o)| o.is_none())
            .all(|(i, _)| composite[i * 4 + 3] == 0),
        "a texel nothing samples must not be painted in some palette's colours"
    );
    // And through palette 0 those same texels are lurid: this is the
    // measurement, not a claim about how it looks.
    let flat = page.texture.to_rgba(0);
    let lurid = |px: &[u8]| {
        (px[0] > 200 && px[1] < 40 && px[2] < 40) || (px[1] > 200 && px[0] < 40 && px[2] < 40)
    };
    let flat_lurid = owner
        .iter()
        .enumerate()
        .filter(|(i, o)| o.is_none() && lurid(&flat[i * 4..i * 4 + 4]))
        .count();
    assert!(
        flat_lurid > 1000,
        "palette 0 paints {flat_lurid} dead texels pure red/green - the checkerboard"
    );
    // More than one palette is genuinely in use, which is why no single one
    // is "the" colouring.
    let used: std::collections::BTreeSet<u8> = owner.iter().flatten().copied().collect();
    assert!(used.len() > 1, "palettes in use: {used:?}");

    // And the cover has to be the polygon, not its bounding box. A page is
    // art islands with filler between them; a box around a diagonal face
    // swallows the filler beside it and paints it as skin. Measured on this
    // page: the polygon cover leaves ~2.8% of the page reading pure red or
    // green (retail's own filler, inside a face's UVs), where a box cover
    // left ~6.8%. The bound is the regression guard for that choice.
    let owned_lurid = owner
        .iter()
        .enumerate()
        .filter(|(i, o)| o.is_some() && lurid(&composite[i * 4..i * 4 + 4]))
        .count();
    assert!(
        owned_lurid * 100 < composite.len() / 4 * 4,
        "{owned_lurid} sampled texels render pure red/green - the cover is \
         swallowing the filler between the art islands again"
    );
}

#[test]
fn an_unedited_round_trip_writes_nothing() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let original = disc.clone();
    let mut patcher = DiscPatcher::open(disc).expect("open");
    let ex = mt::export_page(&patcher, &SONGI_FIRST).expect("export");
    assert_eq!((ex.width, ex.height), (256, 256));
    assert_eq!(ex.name, "Songi");

    let outcome = mt::replace_page(
        &mut patcher,
        &SONGI_FIRST,
        &ex.rgba,
        ex.width,
        ex.height,
        false,
        false,
    )
    .expect("replace");
    assert!(outcome.unchanged, "re-importing the export changes nothing");
    assert_eq!(outcome.texels_changed, 0);
    assert_eq!(outcome.quantized_texels, 0);
    // Not "no visible change" - no disc byte moved. Our LZS encoder is not
    // the mastering's, so this only holds because an unchanged block is
    // never re-emitted.
    assert_eq!(patcher.image(), &original[..], "zero-run patch");
}

#[test]
fn a_repaint_lands_reads_back_and_is_deterministic() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let original = disc.clone();
    let mut patcher = DiscPatcher::open(disc).expect("open");
    let page = mt::read_page(&patcher, &SONGI_FIRST).expect("read page");
    let owner = page.ownership();
    let ex = mt::export_page(&patcher, &SONGI_FIRST).expect("export");
    let edited = flatten_regions(&ex.rgba, &owner);
    assert_ne!(edited, ex.rgba, "the flatten really is a different image");

    // The preview is the write stopped before the patch.
    let preview = mt::preview_page(&patcher, &SONGI_FIRST, &edited, ex.width, ex.height, false)
        .expect("preview");
    assert!(!preview.unchanged);
    assert!(preview.texels_changed > 0);
    assert_eq!(
        preview.quantized_texels, 0,
        "every colour was already there"
    );
    assert!(
        preview.fit.recompressed <= preview.fit.capacity,
        "{:?}",
        preview.fit
    );
    assert_eq!(
        patcher.image(),
        &original[..],
        "a preview must never touch the image"
    );

    let outcome = mt::replace_page(
        &mut patcher,
        &SONGI_FIRST,
        &edited,
        ex.width,
        ex.height,
        false,
        false,
    )
    .expect("replace");
    assert_eq!(
        outcome.fit, preview.fit,
        "the preview measured the real write"
    );
    assert_eq!(outcome.texels_changed, preview.texels_changed);
    assert_ne!(patcher.image(), &original[..], "the write happened");

    // The disc now holds what the preview showed, and the monster still
    // parses as a monster.
    let after = mt::export_page(&patcher, &SONGI_FIRST).expect("re-export");
    assert_eq!(after.rgba, preview.rgba, "the disc holds the preview");
    assert_eq!(after.name, "Songi", "the record survived the re-pack");
    let reopened = DiscPatcher::open(patcher.image().to_vec()).expect("re-open patched image");
    let rows = mt::catalog(&reopened).expect("re-catalog");
    assert_eq!(rows.len(), 186, "no other monster moved");

    // Same edit, same bytes.
    let mut twin = DiscPatcher::open(original.clone()).expect("open");
    mt::replace_page(
        &mut twin,
        &SONGI_FIRST,
        &edited,
        ex.width,
        ex.height,
        false,
        false,
    )
    .expect("replace");
    assert_eq!(twin.image(), patcher.image(), "byte-deterministic");
}

/// A monster's CLUTs upload to VRAM verbatim, so this family may not rewrite
/// one - which means a colour the page's own palettes do not hold has no
/// slot to go in. That has to be an error *before* the patch, not a
/// half-written archive slot.
#[test]
fn a_colour_the_palettes_do_not_hold_is_refused_before_any_write() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let original = disc.clone();
    let mut patcher = DiscPatcher::open(disc).expect("open");
    let page = mt::read_page(&patcher, &SONGI_FIRST).expect("read page");
    let owner = page.ownership();
    let ex = mt::export_page(&patcher, &SONGI_FIRST).expect("export");

    // A colour no palette in this pool holds, painted over a texel the model
    // really samples.
    let stray = [8u8, 248, 8, 255];
    let held: std::collections::HashSet<[u8; 4]> = ex
        .rgba
        .chunks_exact(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect();
    assert!(!held.contains(&stray), "the probe colour must be new");
    let at = owner
        .iter()
        .position(|o| o.is_some())
        .expect("a live texel");
    let mut edited = ex.rgba.clone();
    edited[at * 4..at * 4 + 4].copy_from_slice(&stray);

    let err = mt::replace_page(
        &mut patcher,
        &SONGI_FIRST,
        &edited,
        ex.width,
        ex.height,
        false,
        false,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("palettes do not hold"), "{err}");
    assert_eq!(
        patcher.image(),
        &original[..],
        "a refused edit writes nothing at all"
    );

    // With quantize the same edit lands, folded onto the nearest colour that
    // texel's own palette holds.
    let outcome = mt::replace_page(
        &mut patcher,
        &SONGI_FIRST,
        &edited,
        ex.width,
        ex.height,
        true,
        false,
    )
    .expect("quantized replace");
    assert_eq!(outcome.quantized_texels, 1);
}

/// Wrong geometry is a caller error, and it must be caught before the encode
/// walks off the end of a page.
#[test]
fn a_replacement_of_the_wrong_size_is_refused() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open");
    let err = mt::replace_page(
        &mut patcher,
        &SONGI_FIRST,
        &vec![0u8; 64 * 64 * 4],
        64,
        64,
        false,
        false,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("256x256"), "{err}");
}
