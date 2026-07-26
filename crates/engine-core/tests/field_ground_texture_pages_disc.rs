//! Disc-gated: the field VRAM pre-pass must leave every scene's **ground-tile
//! atlas** resident.
//!
//! A field/town scene's ground is a heightfield whose per-cell texture comes
//! from the `.MAP` object record (`+0x14` atlas tile, `+0x15` tpage, `+0x16`
//! CLUT - [`legaia_asset::field_objects::WalkHeightfield`]). The atlas pages
//! and their CLUT rows live in the scene's `scene_asset_table` entry. The
//! observed failure this file guards is real: Jeremi's ground rendered as a
//! "tombstone" lattice and Mt. Dhini's as a repeating vine, because a full
//! `64 x 256` page landed on fb `(768, 0)` (tpage `0x0C`) - exactly where most
//! field scenes put their ground-tile atlas - after the atlas had been
//! uploaded, so the ground quads sampled character / backdrop texels.
//!
//! **What that page was is a corrected attribution.** It was read as stale
//! mastering scratch behind a **pochi-filler** slot's `pochipochi...` prefix
//! (`docs/formats/pochi.md`). It is not. Every one of the retail
//! `Class::PochiFiller` entries is exactly one 2048-byte sector of fill and
//! carries no parseable TIM at all; there is no scratch behind the prefix
//! because there is nothing behind the prefix. The page came from the pochi
//! entry's *neighbour* - the `scene_tmd_stream` entry that follows it, whose
//! `FUN_8001FE70` type-`0x01` chunks are the battle-character atlases at
//! `(768, 0)` and `(832, 0)` - reached through the historical PROT
//! entry-size expression, which spanned an entry into the next one. With the
//! entry sized as the sector gap to its successor
//! ([`docs/formats/prot.md`](../../../docs/formats/prot.md)) a pochi slot has
//! no reach, and the field loader's own `scene_tmd_stream` exclusion keeps the
//! neighbour's battle pages out.
//!
//! Three assertions:
//! 1. the corpus invariant that dissolves the hazard - every pochi-filler
//!    entry is one sector with no TIM in it;
//! 2. the real source, positively: `geremi`'s pochi slot is followed by a
//!    `scene_tmd_stream` that *does* carry the `(768, 0)` / `(832, 0)` pages,
//!    and the built field VRAM does **not** contain that page;
//! 3. across a spread of field scenes, every ground cell's `(tpage, clut)`
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

/// Every parseable TIM in `bytes`, raw and inside its LZS sections.
fn tims_in(bytes: &[u8]) -> Vec<legaia_tim::Tim> {
    let scan = legaia_asset::tim_scan::scan_entry(bytes);
    let mut out = Vec::new();
    for (source, hit) in &scan.hits {
        let src: &[u8] = match source {
            legaia_asset::tim_scan::Source::Raw => bytes,
            legaia_asset::tim_scan::Source::Lzs(i) => scan.lzs_sections[*i].as_slice(),
        };
        let Some(payload) = src.get(hit.offset..hit.offset + hit.byte_len) else {
            continue;
        };
        if let Ok(tim) = legaia_tim::parse(payload) {
            out.push(tim);
        }
    }
    out
}

#[test]
fn pochi_leftovers_never_reach_the_ground_atlas_page() {
    let Some(index) = open_index() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };

    // 1. The corpus invariant that dissolves the hazard. A pochi-filler slot
    //    is one reserved sector of `pochipochi...` + 0x1A fill. It has no
    //    second sector for scratch to live in, and nothing in it parses as a
    //    TIM - so no sweep of one can put a page anywhere. (The historical
    //    reading, that the bytes behind the fill prefix are stale scratch that
    //    often forms a complete valid TIM, was reading the *next* entry
    //    through the old PROT entry-size expression.)
    let mut pochi_slots = 0usize;
    let mut multi_sector = Vec::<u32>::new();
    let mut carrying_a_tim = Vec::<u32>::new();
    for idx in 0..index.entry_count() as u32 {
        if index.class_of(idx).ok() != Some(legaia_asset::categorize::Class::PochiFiller) {
            continue;
        }
        pochi_slots += 1;
        let Ok(bytes) = index.entry_bytes(idx) else {
            continue;
        };
        if bytes.len() != 2048 {
            multi_sector.push(idx);
        }
        if !tims_in(&bytes).is_empty() {
            carrying_a_tim.push(idx);
        }
    }
    assert!(
        pochi_slots >= 100,
        "expected the retail pochi-filler class to be populated, found {pochi_slots}"
    );
    assert!(
        multi_sector.is_empty(),
        "pochi-filler entries are single reserved sectors; these are not: {multi_sector:?}"
    );
    assert!(
        carrying_a_tim.is_empty(),
        "a pochi-filler entry parsed as carrying a TIM - the stale-scratch hazard would be \
         back, and the ground-atlas sweep needs re-auditing: {carrying_a_tim:?}"
    );

    // 2. The real source of the page that broke Jeremi's ground, positively:
    //    the `scene_tmd_stream` entry that follows the pochi slot carries the
    //    battle-character atlas at the ground page's own origin.
    let scene = Scene::load(&index, "geremi").expect("load geremi");
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

    let pochi_at = scene
        .entries
        .iter()
        .position(|e| e.class == legaia_asset::categorize::Class::PochiFiller)
        .expect("geremi's block reserves a pochi-filler slot");
    let neighbour = scene
        .entries
        .get(pochi_at + 1)
        .expect("the pochi slot has a successor inside the block");
    assert_eq!(
        neighbour.class,
        legaia_asset::categorize::Class::SceneTmdStream,
        "the entry the old over-read reached is geremi's scene_tmd_stream sibling"
    );
    let intruder = tims_in(&neighbour.bytes)
        .into_iter()
        .find(|t| (t.image.fb_x as usize, t.image.fb_y as usize) == (px, py))
        .expect("that sibling carries the battle-character page at the ground page's origin");

    // 3. The build must not contain it: compare the first image row. Field
    //    dispatch skips `scene_tmd_stream` TIM chunks, so the page that
    //    survives is the scene's own atlas.
    let res = build_field(&index, &scene);
    let row: Vec<u16> = (0..intruder.image.fb_w as usize)
        .map(|i| res.vram.pixel(px + i, py))
        .collect();
    let stale: Vec<u16> = (0..intruder.image.fb_w as usize)
        .map(|i| {
            let o = i * 2;
            u16::from_le_bytes([intruder.image.data[o], intruder.image.data[o + 1]])
        })
        .collect();
    assert_ne!(
        row, stale,
        "the scene_tmd_stream battle page was uploaded over geremi's ground atlas"
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
