//! Disc-gated: which placed records the minigame venues stand
//! (`venue_floor`), over the shipped `.MAP`s.
//!
//! * Fishing (`other1`): the floor pass over each venue's latched box keeps
//!   the map's three placed records - Buma's mountains and bank, rows
//!   `102..110` - for Buma and none for Vidna. The retail Vidna states
//!   (`minigame_fishing_retail_ready` / `_shore` / `_lure_cast`) hold an empty
//!   floor list and a zero spawn count at `0x801D90A4`.
//! * Dance (`other7`): the step-marker walk keeps the 35 triggered cells of
//!   its rect, which is the count and the cell set the retail dance states
//!   hold on `*0x8007C354`; the map's other 28 placed records (the unbound
//!   ones, and the bound ones north of the hall and east of it) stand nowhere.
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;

use legaia_engine_field::field_regions::parse_tile_triggers;
use legaia_engine_minigame_scenes::dance_venue::{DANCE_MARKER_WALK_RECT, DanceVenue};
use legaia_engine_minigame_scenes::fishing_venue::PARTY_ANCHOR_TILE;
use legaia_engine_minigame_scenes::venue_floor::{
    floor_pass_cells, keep_spawned, latched_region_box, marker_walk_cells,
};
use legaia_engine_scene::scene::{ProtIndex, Scene};

fn extracted_dir() -> Option<PathBuf> {
    let over = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from);
    over.into_iter()
        .chain(
            ["extracted", "../extracted", "../../extracted"]
                .into_iter()
                .map(PathBuf::from),
        )
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

fn gate() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing");
        None
    })
}

#[test]
fn each_fishing_venue_stands_its_own_half() {
    let Some(dir) = gate() else { return };
    let index = ProtIndex::open_extracted(&dir).expect("prot index");
    let scene = Scene::load(&index, "other1").expect("other1");
    let block = scene.field_map_region_block(&index).unwrap();
    let map = scene.field_map_bytes(&index).unwrap().expect("map");
    let all = scene
        .field_object_placements(&index)
        .unwrap()
        .expect("placements");
    assert_eq!(all.len(), 3, "other1's placed records");
    assert!(all.iter().all(|p| (102..=110).contains(&p.row)));
    let kept = |venue: usize| {
        let (tx, tz) = PARTY_ANCHOR_TILE[venue];
        let b = latched_region_box(block.as_deref(), (i32::from(tx), i32::from(tz)));
        keep_spawned(all.clone(), &floor_pass_cells(&map, b)).len()
    };
    assert_eq!(kept(0), 3, "Buma stands its mountains and bank");
    assert_eq!(kept(1), 0, "Vidna stands none of Buma's half");
}

#[test]
fn the_dance_hall_stands_the_marker_walks_cells() {
    let Some(dir) = gate() else { return };
    let index = ProtIndex::open_extracted(&dir).expect("prot index");
    let scene = Scene::load(&index, "other7").expect("other7");
    let map = scene.field_map_bytes(&index).unwrap().expect("map");
    let primary = parse_tile_triggers(&map[0x10000..]);
    let fallback = map
        .get(0x12000..)
        .map(parse_tile_triggers)
        .unwrap_or_default();
    let all = scene
        .field_object_placements(&index)
        .unwrap()
        .expect("placements");
    assert_eq!(all.len(), 63, "other7's placed records");
    let cells = marker_walk_cells(&map, DANCE_MARKER_WALK_RECT, &primary, &fallback);
    let kept = keep_spawned(all, &cells);
    assert_eq!(kept.len(), 35, "the retail states hold 35 on *0x8007C354");
    let (x0, z0, w, h) = DANCE_MARKER_WALK_RECT;
    for p in &kept {
        let (c, r) = (i32::from(p.col), i32::from(p.row));
        assert!((x0..x0 + w).contains(&c) && (z0..z0 + h).contains(&r));
        assert_ne!(
            p.anchor_cell & 0x400,
            0,
            "every triggered cell is bind-owned"
        );
    }

    // The venue's own draw list is built from that selection: no static draw
    // and no marker stands outside the walk's rect.
    let rec = legaia_overlay_images::static_overlay::overlay_map()
        .by_prot_index(legaia_minigame_formats::dance_chart::DANCE_OVERLAY_PROT_INDEX as u32)
        .expect("0980 in the static-overlay map");
    let bytes = index.entry_bytes_extended(rec.prot_index).expect("0980");
    let overlay = legaia_overlay_images::static_overlay::as_loaded(&bytes, rec).expect("0980");
    let venue = DanceVenue::build(&index, Some(&overlay)).expect("venue builds");
    assert!(!venue.markers.is_empty());
}
