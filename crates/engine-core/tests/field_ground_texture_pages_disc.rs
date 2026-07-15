//! Disc-gated: the field VRAM pre-pass must leave every scene's **ground-tile
//! atlas** resident.
//!
//! A field/town scene's ground is a heightfield whose per-cell texture comes
//! from the `.MAP` object record (`+0x14` atlas tile, `+0x15` tpage, `+0x16`
//! CLUT - [`legaia_asset::field_objects::WalkHeightfield`]). The atlas pages
//! and their CLUT rows live in the scene's `scene_asset_table` entry, but the
//! scene's CDNAME block also reserves **pochi-filler** slots
//! (`pochipochi...`, `docs/formats/pochi.md`) whose bytes behind the fill
//! prefix are stale scratch - and that scratch is often a well-formed
//! `256 x 256` 4bpp TIM declaring fb `(768, 0)` / `(832, 0)`, i.e. tpages
//! `0x0C` / `0x1D`. Uploading one lands last and erases the terrain atlas, so
//! the ground quads sample character / backdrop texels (Jeremi's "tombstone"
//! lattice, Mt. Dhini's repeating vine texture).
//!
//! Two assertions:
//! 1. the hazard is real on the disc - `geremi`'s block *does* carry a
//!    pochi slot whose leftover TIM targets the ground page - and the built
//!    VRAM does **not** contain that leftover;
//! 2. across a spread of field scenes, every ground cell's `(tpage, clut)`
//!    resolves to a populated palette + page, and virtually every ground
//!    vertex finds texel data.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};
use std::path::Path;

/// Field scenes that own a ground heightfield, spread across all three
/// kingdoms + both ground-atlas page families (`0x0C` at fb `(768, 0)` -
/// the page the pochi leftovers collide with - and `0x1B`/`0x1C` at y 256).
const SCENES: &[&str] = &[
    "town01", // Rim Elm (the one scene that always worked: no pochi TIMs)
    "geremi", // Jeremi - the "tombstone lattice" report
    "deene",  // Mt. Dhini - the "repeating vine" report
    "izumi", "keikoku", "rikuroa", "garmel", "vell", "bylon", "rayman", "balden", "station",
    "bubu2", "uru",
];

/// Disc gate: the extracted `PROT.DAT` + `CDNAME.TXT` (either the crate-local
/// or the workspace-root copy) **and** `LEGAIA_DISC_BIN`. Absent either, the
/// test skips and passes - the repo-wide rule.
fn open_index() -> Option<ProtIndex> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for root in ["extracted", "../../extracted"] {
        let p = Path::new(root);
        if p.join("PROT.DAT").exists() && p.join("CDNAME.TXT").exists() {
            return ProtIndex::open_extracted(p).ok();
        }
    }
    None
}

fn build_field(index: &ProtIndex, scene: &Scene) -> SceneResources {
    let shared: Vec<Scene> = FIELD_SHARED_BLOCKS
        .iter()
        .filter_map(|n| Scene::load(index, n).ok())
        .collect();
    let shared_refs: Vec<&Scene> = shared.iter().collect();
    let system_ui = index.system_ui_bundle().ok();
    SceneResources::build_targeted_with_options(
        scene,
        &shared_refs,
        BuildOptions {
            kind: SceneLoadKind::Field,
            upload_all_tims: true,
            system_ui: system_ui.as_deref(),
        },
    )
    .expect("field scene resources")
    .0
}

/// `(fb_x, fb_y)` of a PSX `tpage` word (4bpp / 8bpp page base).
fn page_origin(tpage: u16) -> (usize, usize) {
    (
        ((tpage & 0xF) as usize) * 64,
        (((tpage >> 4) & 1) as usize) * 256,
    )
}

/// `(fb_x, fb_y)` of a PSX CBA (CLUT) word.
fn clut_origin(clut: u16) -> (usize, usize) {
    (
        ((clut & 0x3F) as usize) * 16,
        ((clut >> 6) & 0x1FF) as usize,
    )
}

#[test]
fn pochi_leftovers_never_reach_the_ground_atlas_page() {
    let Some(index) = open_index() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    let scene = Scene::load(&index, "geremi").expect("load geremi");

    // 1. The hazard: a pochi-filler slot in this block carries a stale TIM
    //    whose image block targets the very page the ground records name.
    let hf = scene
        .walk_heightfield(&index)
        .expect("geremi map")
        .expect("geremi heightfield");
    let mut by_page = std::collections::BTreeMap::<u16, usize>::new();
    for ct in &hf.cba_tsb {
        *by_page.entry(ct[1]).or_default() += 1;
    }
    let ground_tpage = *by_page
        .iter()
        .max_by_key(|(_, n)| **n)
        .expect("geremi ground pages")
        .0;
    let (px, py) = page_origin(ground_tpage);
    assert_eq!(
        (px, py),
        (768, 0),
        "geremi's ground atlas is the fb (768,0) page"
    );

    let mut hazard: Option<legaia_tim::Tim> = None;
    for entry in &scene.entries {
        if entry.class != legaia_asset::categorize::Class::PochiFiller {
            continue;
        }
        let scan = legaia_asset::tim_scan::scan_entry(&entry.bytes);
        for (source, hit) in &scan.hits {
            let src: &[u8] = match source {
                legaia_asset::tim_scan::Source::Raw => &entry.bytes,
                legaia_asset::tim_scan::Source::Lzs(i) => scan.lzs_sections[*i].as_slice(),
            };
            let Some(payload) = src.get(hit.offset..hit.offset + hit.byte_len) else {
                continue;
            };
            let Ok(tim) = legaia_tim::parse(payload) else {
                continue;
            };
            if (tim.image.fb_x as usize, tim.image.fb_y as usize) == (px, py) {
                hazard = Some(tim);
            }
        }
    }
    let hazard = hazard.expect(
        "geremi's block carries a pochi-filler slot whose leftover TIM targets the ground page",
    );

    // 2. The build must not contain it: compare the first image row.
    let res = build_field(&index, &scene);
    let row: Vec<u16> = (0..hazard.image.fb_w as usize)
        .map(|i| res.vram.pixel(px + i, py))
        .collect();
    let stale: Vec<u16> = (0..hazard.image.fb_w as usize)
        .map(|i| {
            let o = i * 2;
            u16::from_le_bytes([hazard.image.data[o], hazard.image.data[o + 1]])
        })
        .collect();
    assert_ne!(
        row, stale,
        "the pochi-filler leftover was uploaded over geremi's ground atlas"
    );
    // And the page is not simply empty - the scene's own atlas is there.
    assert!(
        row.iter().any(|w| *w != 0),
        "geremi's ground atlas page is unpopulated"
    );
}

#[test]
fn every_field_scene_ground_cell_resolves_to_a_resident_page() {
    let Some(index) = open_index() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    for name in SCENES {
        let Ok(scene) = Scene::load(&index, name) else {
            panic!("scene {name} does not load");
        };
        let Ok(Some(hf)) = scene.walk_heightfield(&index) else {
            panic!("scene {name} has no ground heightfield");
        };
        assert!(!hf.indices.is_empty(), "{name}: empty ground");
        let res = build_field(&index, &scene);

        // Weight the residency check by how many ground vertices name each
        // `(tpage, clut)`: the palette AND the page must be in VRAM. A handful
        // of cells in a couple of scenes (Biron's `0x1C`/`(0,501)` corner)
        // point at an atlas their own block never ships - retail would read
        // whatever the previous scene left there - so the bar is coverage, not
        // every last combo.
        let mut combos = std::collections::BTreeMap::<(u16, u16), usize>::new();
        for ct in &hf.cba_tsb {
            *combos.entry((ct[1], ct[0])).or_default() += 1;
        }
        let total_verts: usize = combos.values().sum();
        let mut resident_verts = 0usize;
        for ((tpage, clut), n) in &combos {
            let (cx, cy) = clut_origin(*clut);
            let palette_ok = (0..16).any(|i| res.vram.pixel(cx + i, cy) != 0);
            let (px, py) = page_origin(*tpage);
            let page_ok = res.vram.region_has_data(px, py, 64, 256);
            if palette_ok && page_ok {
                resident_verts += n;
            }
        }
        assert!(
            resident_verts * 100 >= total_verts * 90,
            "{name}: only {resident_verts}/{total_verts} ground vertices land on a \
             resident terrain page+palette - the scene's ground atlas never reached VRAM"
        );

        // And the sampled texels are actually there: a ground vertex whose
        // atlas tile lands on a blank page reads as a hole.
        let total = hf.cba_tsb.len();
        let missing = hf
            .cba_tsb
            .iter()
            .enumerate()
            .filter(|(i, ct)| {
                let uv = hf.uvs[*i];
                !res.vram
                    .prim_has_texture_data(ct[0], ct[1], &[(uv[0], uv[1])])
            })
            .count();
        assert!(
            missing * 100 <= total * 25,
            "{name}: {missing}/{total} ground vertices sample an empty texel"
        );
    }
}
