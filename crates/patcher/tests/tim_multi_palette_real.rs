//! Disc-gated oracles for multi-palette texture editing.
//!
//! 1. **Every raw multi-palette TIM round-trips byte-exactly** through each
//!    download shape - image (through palette 0 and the last palette),
//!    composite, palette strip, indexed PNG - and back through the importer.
//! 2. **The menu / battle UI sheet** (`PROT.DAT` `0x18E0`) has a per-region
//!    palette map from the executable's widget table, its in-game composite
//!    round-trips byte-exactly, and a one-entry palette-strip edit changes
//!    exactly that entry's two bytes of the TIM.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips+passes when unset. Patched images live
//! only in memory.

use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::texture::{
    ExportFormat, TextureTarget, export_texture_png, read_texture, replace_texture_png,
    texture_catalogs,
};
use legaia_patcher::texture_palettes::texture_palettes;
use legaia_tim::encode::{EncodeOptions, decode_png_rgba};
use legaia_tim::multi_palette::{ImportKind, View, rgba_png};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

const SHEET: TextureTarget = TextureTarget {
    entry: None,
    lzs_section: None,
    offset: 0x18E0,
};

#[test]
fn every_raw_multi_palette_tim_round_trips_through_every_shape() {
    use legaia_tim::multi_palette::{
        PaletteContext, all_sets, composite_geom, import_png, indexed_png, own_palettes,
        render_composite, render_strip, standalone_strip_geom, uniform_map,
    };
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let (raw, _) = texture_catalogs(&patcher).expect("catalogs");
    let prot = patcher.read_named_file("PROT.DAT").expect("PROT.DAT");
    let ctx = PaletteContext::default();
    let opts = EncodeOptions::default();
    let mut checked = 0usize;
    for t in raw.iter().filter(|t| t.clut_count > 1) {
        let bytes = &prot[t.abs_offset as usize..t.abs_offset as usize + t.byte_len];
        let tim = legaia_tim::parse_strict(bytes).expect("catalog TIM strict-parses");
        if legaia_tim::decode_rgba8(&tim, 0).is_err() {
            continue;
        }
        let (w, h) = (tim.pixel_width(), tim.pixel_height());
        let own = own_palettes(&tim);
        let sets = all_sets(&tim, &ctx);
        let last = own.len() - 1;
        let mut shapes: Vec<(&str, Vec<u8>)> = vec![
            (
                "image pal 0",
                rgba_png(w, h, &legaia_tim::decode_rgba8(&tim, 0).unwrap()).unwrap(),
            ),
            (
                "image last pal",
                rgba_png(w, h, &legaia_tim::decode_rgba8(&tim, last).unwrap()).unwrap(),
            ),
            ("indexed", indexed_png(&tim, &own[last]).unwrap()),
        ];
        let g = standalone_strip_geom(&tim).expect("strip geometry");
        shapes.push((
            "strip",
            rgba_png(g.width(), g.height(), &render_strip(&own, g, g.width())).unwrap(),
        ));
        if composite_geom(&tim).is_some() {
            let (cw, ch, rgba) = render_composite(&tim, &sets, &uniform_map(&tim, 0)).unwrap();
            shapes.push(("composite", rgba_png(cw, ch, &rgba).unwrap()));
        }
        for (name, png) in shapes {
            let imp = import_png(&tim, &png, &ctx, &opts)
                .unwrap_or_else(|e| panic!("0x{:X} {name}: {e:#}", t.abs_offset));
            assert_eq!(
                imp.palette_entries_changed, 0,
                "0x{:X} {name}",
                t.abs_offset
            );
            assert_eq!(
                imp.encoded.bytes, bytes,
                "0x{:X} {name} is not byte-identical",
                t.abs_offset
            );
        }
        checked += 1;
    }
    eprintln!("[ran] {checked} multi-palette raw TIMs round-tripped in every shape");
    assert!(checked > 100, "only {checked} multi-palette TIMs checked");
}

#[test]
fn the_ui_sheet_has_a_region_map_and_edits_stay_surgical() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let orig = read_texture(&patcher, &SHEET).expect("the UI sheet");
    assert_eq!(
        (orig.tim.pixel_width(), orig.tim.pixel_height()),
        (256, 192)
    );
    assert_eq!(orig.tim_bytes.len(), 25120);
    assert_eq!(orig.tim.palette_count(), 16);

    let pals = texture_palettes(&patcher, &orig.tim).unwrap();
    assert!(pals.has_map(), "the widget table must give the sheet a map");
    assert_eq!(
        pals.context.external.len(),
        3,
        "sub-palettes 16..18 from 0x1858"
    );
    let map = pals.context.map.as_ref().unwrap();
    let at = |x: usize, y: usize| map[y * 256 + x];
    // Pinned in battle.md: blue plate sub-palette 4, gold plaque 12, the
    // marbled panel 0; status badges on their own sub-palettes.
    assert_eq!(at(200, 10), 4, "blue plate body");
    assert_eq!(at(200, 70), 12, "carved-gold plate body");
    assert_eq!(at(10, 10), 0, "marbled panel");
    assert_eq!(at(10, 50), 9, "Venom badge (widget 0x18)");
    assert_eq!(at(60, 90), 16, "Stone badge: sub-palette 16 = external 0");
    // Same kernel as the viewer composite (tim_palette_context_real): the
    // Curse badge keeps its own sub-palette 13 over the class-4 bar record
    // that also samples those texels, and the class-4 cap pair maps.
    assert_eq!(at(64, 64), 13, "Curse badge (widget 0x1F)");
    assert_eq!(at(193, 25), 5, "class-4 bar cap");
    // The button-glyph rectangle is another TIM's at runtime: named in a
    // note, not mapped, and no "unresolved sub-palette 19" left over.
    assert!(
        pals.notes.iter().any(|n| n.contains("0x7B00")),
        "{:?}",
        pals.notes
    );
    assert_eq!(pals.notes.len(), 1, "{:?}", pals.notes);
    assert!(
        pals.regions
            .iter()
            .all(|r| !(128..192).contains(&r.rect.0) || !(96..128).contains(&r.rect.1)),
        "a region inside the covered glyph rectangle"
    );

    // In-game composite: byte-identical round trip.
    let ex = export_texture_png(&patcher, &SHEET, ExportFormat::Composite, View::InGame).unwrap();
    assert_eq!((ex.width, ex.height), (256, 192 + 16 * 8));
    let out = replace_texture_png(
        &mut patcher,
        &SHEET,
        &ex.png,
        &EncodeOptions::default(),
        false,
    )
    .unwrap();
    assert_eq!(out.import, Some(ImportKind::Composite(View::InGame)));
    assert_eq!(
        read_texture(&patcher, &SHEET).unwrap().tim_bytes,
        orig.tim_bytes
    );

    // Strip edit: palette 12 entry 9 -> a new colour. Exactly one CLUT
    // entry (two bytes) of the TIM moves.
    let (w, h, mut rgba) = decode_png_rgba(&ex.png).unwrap();
    for y in 192 + 12 * 8..192 + 13 * 8 {
        for x in 9 * 16..10 * 16 {
            rgba[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[0, 200, 255, 255]);
        }
    }
    let png = rgba_png(w, h, &rgba).unwrap();
    let out =
        replace_texture_png(&mut patcher, &SHEET, &png, &EncodeOptions::default(), false).unwrap();
    assert_eq!(out.palette_entries_changed, 1);
    let after = read_texture(&patcher, &SHEET).unwrap();
    let diff: Vec<usize> = after
        .tim_bytes
        .iter()
        .zip(&orig.tim_bytes)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .collect();
    let entry = 20 + 2 * (12 * 16 + 9);
    assert!(
        !diff.is_empty() && diff.iter().all(|&i| i == entry || i == entry + 1),
        "{diff:?}"
    );

    // A new colour painted into the Venom badge lands in a slot palette 9
    // does not use; the other fifteen palettes stay byte-identical.
    let mut patcher = DiscPatcher::open(patcher.into_image()).unwrap();
    let before = read_texture(&patcher, &SHEET).unwrap();
    let ex = export_texture_png(&patcher, &SHEET, ExportFormat::Image, View::InGame).unwrap();
    let (w, h, mut rgba) = decode_png_rgba(&ex.png).unwrap();
    rgba[(50 * w + 5) * 4..(50 * w + 5) * 4 + 4].copy_from_slice(&[255, 0, 255, 255]);
    let png = rgba_png(w, h, &rgba).unwrap();
    let out =
        replace_texture_png(&mut patcher, &SHEET, &png, &EncodeOptions::default(), false).unwrap();
    assert_eq!(out.import, Some(ImportKind::Image(View::InGame)));
    assert_eq!(out.new_palette_entries, 1);
    let got = read_texture(&patcher, &SHEET).unwrap();
    let (a, b) = (
        &got.tim.clut.as_ref().unwrap().entries,
        &before.tim.clut.as_ref().unwrap().entries,
    );
    for p in (0..16).filter(|&p| p != 9) {
        assert_eq!(
            a[p * 16..p * 16 + 16],
            b[p * 16..p * 16 + 16],
            "palette {p}"
        );
    }
    eprintln!("[ran] UI sheet: map, composite round trip, strip edit, new colour");
}
