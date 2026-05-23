//! Disc-gated: the region-keyed encounter model (`FUN_801D9E1C` port) builds
//! from real scene MAN bytes and the regions cover the same formation indices
//! the aggregated table exposes.
//!
//! This is the position-routed sibling of `field_man_encounter_disc.rs`: where
//! that test asserts the aggregated weighted table installs, this one asserts
//! the per-region geometry (`RegionEncounterTable`) decodes from the same MAN
//! and that picking the active region by an in-AABB tile yields a formation id
//! inside the scene's formation list.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_core::region_encounter::{
    EncounterRateSetting, RegionEncounterTracker, region_encounter_table_from_man,
};
use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost};

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn region_table_builds_from_real_scene_man() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));

    // map03 = a world-map (kingdom-bundle) scene; town01 / town0c = field
    // towns. All carry a MAN encounter section with a region table.
    let mut any_scene = false;
    for scene in ["map03", "town01", "town0c"] {
        host.enter_field_scene(scene, 0)
            .unwrap_or_else(|e| panic!("enter_field_scene('{scene}') failed: {e:#}"));

        let man = host
            .scene
            .as_ref()
            .and_then(|s| s.field_man_payload(&host.index).ok().flatten());
        let Some(man) = man else {
            eprintln!("[{scene}] no MAN payload (skip)");
            continue;
        };

        let Some(table) = region_encounter_table_from_man(scene, &man) else {
            eprintln!("[{scene}] no region table (skip)");
            continue;
        };
        any_scene = true;

        // Every region must have a well-ordered AABB and a non-degenerate
        // formation slice (count > 0 for at least one region).
        assert!(!table.regions.is_empty(), "[{scene}] regions non-empty");
        let mut max_formation = 0u16;
        let mut any_rollable = false;
        for r in &table.regions {
            assert!(
                r.tile_x_min <= r.tile_x_max && r.tile_z_min <= r.tile_z_max,
                "[{scene}] region AABB ordered: {r:?}"
            );
            if r.formation_count > 0 {
                any_rollable = true;
                max_formation =
                    max_formation.max(r.formation_base as u16 + r.formation_count as u16);
            }
        }
        assert!(
            any_rollable,
            "[{scene}] at least one region rolls a formation"
        );
        eprintln!(
            "[{scene}] {} regions, formation ids up to {}",
            table.regions.len(),
            max_formation
        );

        // Find a world position whose *resolved* region (first-AABB-match, the
        // way the retail walk picks) is rollable. The centre of a rollable
        // region can be shadowed by an earlier overlapping zero-count region,
        // so scan region centres until one resolves to a rollable region.
        // A region triggers only with both a non-zero rate increment and a
        // non-empty formation slice; scan for a region whose centre resolves
        // (first-AABB-match) to such a region.
        let rollable = |r: &legaia_engine_core::region_encounter::EncounterRegion| {
            r.formation_count > 0 && r.rate_increment > 0
        };
        let mut trigger_pos = None;
        for r in &table.regions {
            if !rollable(r) {
                continue;
            }
            let cx = ((r.tile_x_min as i32 + r.tile_x_max as i32) / 2) * 128;
            let cz = ((r.tile_z_min as i32 + r.tile_z_max as i32) / 2) * 128;
            if let Some(resolved) = table.region_at_world(cx as i16, cz as i16)
                && rollable(resolved)
            {
                trigger_pos = Some((cx as i16, cz as i16));
                break;
            }
        }
        let Some((cx, cz)) = trigger_pos else {
            eprintln!("[{scene}] no unshadowed rollable region centre; skip trigger leg");
            continue;
        };

        let mut tracker = RegionEncounterTracker::new(table.clone());
        tracker.set_setting(EncounterRateSetting::Normal);
        // Drive the counter negative so the next in-region step triggers.
        let mut fired = None;
        let mut seed = 0x1234_5678u32;
        for _ in 0..5000 {
            // Deterministic non-zero RNG stand-in (persists across steps).
            if let Some(roll) = tracker.on_step(cx, cz, || {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                seed
            }) {
                fired = Some(roll);
                break;
            }
        }
        let roll = fired.expect("an in-region trigger fires within the step budget");
        assert!(
            (roll.formation_id as u16) < max_formation.max(1) + 1,
            "[{scene}] picked formation {} within the scene's range",
            roll.formation_id
        );
    }

    assert!(
        any_scene,
        "at least one of map03/town01/town0c yielded a region table"
    );
}
