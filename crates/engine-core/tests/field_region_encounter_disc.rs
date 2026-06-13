//! Disc-gated: a FIELD scene's entry path routes per-region random encounters.
//!
//! `SceneHost::enter_field_scene` builds the per-region encounter table from the
//! scene's MAN (`region_encounter_table_from_man`) and installs it on
//! `World::field_region_tracker`, so a field step rolls against the player's
//! *active region* (per-region rate increment + formation-range pick,
//! `FUN_801D9E1C`) instead of the aggregated mean-rate `EncounterSession`. This
//! is the field counterpart to the world-map `set_world_map_regions` path, which
//! `region_encounter_disc.rs` already exercises at the tracker level.
//!
//! Three things are pinned here:
//!   1. *install* — at least one field (non-world-map) scene installs the field
//!      region tracker at entry, and the installed table is byte-equal to the
//!      one `region_encounter_table_from_man` decodes from the same MAN.
//!   2. *faithfulness* — at least one such scene carries regions with two or more
//!      distinct rate increments (or distinct formation ranges), i.e. the
//!      per-region routing is a genuine refinement over the single mean rate and
//!      not a cosmetic re-wrap.
//!   3. *flow* — driving `on_field_step` from inside a rollable region drives the
//!      shared `EncounterSession` transition SM (via `trigger_with`) to a real
//!      `Triggered` roll whose formation id lands inside the scene's range.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_core::encounter::EncounterPhase;
use legaia_engine_core::region_encounter::{
    EncounterRegion, RegionEncounterTable, region_encounter_table_from_man,
};
use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost, is_world_map_scene};

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// A region triggers only with both a non-zero rate increment and a non-empty
/// formation slice (`FUN_801D9E1C`'s gate).
fn rollable(r: &EncounterRegion) -> bool {
    r.formation_count > 0 && r.rate_increment > 0
}

/// A world position whose *resolved* region (first-AABB-match, the retail
/// walk's rule) is rollable. A region's own centre can be shadowed by an
/// earlier overlapping zero-rate region, so scan centres until one resolves to
/// a rollable region. `None` when every rollable region is shadowed.
fn unshadowed_rollable_center(table: &RegionEncounterTable) -> Option<(i16, i16)> {
    for r in &table.regions {
        if !rollable(r) {
            continue;
        }
        let cx = ((r.tile_x_min as i32 + r.tile_x_max as i32) / 2) * 128;
        let cz = ((r.tile_z_min as i32 + r.tile_z_max as i32) / 2) * 128;
        if let Some(resolved) = table.region_at_world(cx as i16, cz as i16)
            && rollable(resolved)
        {
            return Some((cx as i16, cz as i16));
        }
    }
    None
}

#[test]
fn field_entry_routes_per_region_encounters() {
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

    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut scene_names: Vec<String> = cdname.values().cloned().collect();
    scene_names.sort();
    scene_names.dedup();

    let mut installed = 0usize;
    let mut multi_rate_scene: Option<String> = None;
    // A scene where the region tracker AND the mean session are both live and a
    // reachable (unshadowed) rollable region exists, so the full
    // region -> trigger_with -> session-SM flow can be driven. Stored with the
    // trigger position (the table is identical on re-entry, so it stays valid).
    let mut drivable: Option<(String, (i16, i16))> = None;

    for scene in &scene_names {
        if is_world_map_scene(scene) {
            continue; // the overworld path is `set_world_map_regions`, tested elsewhere.
        }
        if host.enter_field_scene(scene, 0).is_err() {
            continue; // battle / menu / cutscene labels that aren't field scenes.
        }
        if host.world.field_region_tracker.is_none() {
            continue; // towns etc. — no encounter-region section; mean path stays.
        }
        installed += 1;

        // (1) The installed tracker's table must match a fresh decode of the
        // same MAN — proving the scene-entry path wires the real disc table.
        let man = host
            .scene
            .as_ref()
            .and_then(|s| s.field_man_payload(&host.index).ok().flatten())
            .expect("field MAN payload for a region-tracked scene");
        let fresh = region_encounter_table_from_man(scene, &man)
            .expect("region table re-decodes for a region-tracked scene");
        let installed_regions = &host
            .world
            .field_region_tracker
            .as_ref()
            .expect("tracker present")
            .table()
            .regions;
        assert_eq!(
            installed_regions, &fresh.regions,
            "[{scene}] installed field region table == fresh MAN decode"
        );

        // (2) Faithfulness: does this scene's per-region routing actually differ
        // from a single mean rate? Record the first scene with >=2 distinct rate
        // increments among its rollable regions.
        if multi_rate_scene.is_none() {
            let mut rates: Vec<u8> = installed_regions
                .iter()
                .filter(|r| rollable(r))
                .map(|r| r.rate_increment)
                .collect();
            rates.sort_unstable();
            rates.dedup();
            if rates.len() >= 2 {
                multi_rate_scene = Some(scene.clone());
            }
        }

        // Drivable = a live mean session (the SM the region roll feeds) plus a
        // reachable rollable region centre.
        if drivable.is_none() && host.world.encounter.is_some() {
            let table = host
                .world
                .field_region_tracker
                .as_ref()
                .expect("tracker present")
                .table();
            if let Some(pos) = unshadowed_rollable_center(table) {
                drivable = Some((scene.clone(), pos));
            }
        }

        // Bound the scan: enough evidence for all three assertions.
        if installed >= 4 && multi_rate_scene.is_some() && drivable.is_some() {
            break;
        }
    }

    assert!(
        installed > 0,
        "at least one field scene installs a per-region encounter tracker at entry"
    );
    eprintln!("[field-region] {installed} field scenes routed per-region (scan bounded)");

    let multi = multi_rate_scene
        .expect("at least one field scene has >=2 distinct per-region rate increments");
    eprintln!("[field-region] distinct per-region rates confirmed on '{multi}'");

    // (3) Drive the full flow on a drivable scene: re-enter it, drop the player
    // into the reachable rollable region found during the scan, and step until
    // the shared session SM reports a trigger sourced from the region tracker.
    let (scene, (cx, cz)) =
        drivable.expect("a field scene with a reachable rollable region and a mean session");
    host.enter_field_scene(&scene, 0)
        .unwrap_or_else(|e| panic!("re-enter '{scene}' failed: {e:#}"));

    let max_formation = host
        .world
        .field_region_tracker
        .as_ref()
        .expect("tracker present on re-entry")
        .table()
        .regions
        .iter()
        .filter(|r| r.formation_count > 0)
        .map(|r| r.formation_base as u16 + r.formation_count as u16)
        .max()
        .unwrap_or(0);

    let slot = host
        .world
        .player_actor_slot
        .expect("field scene has a player actor");
    let actor = host
        .world
        .actors
        .get_mut(slot as usize)
        .expect("player actor slot populated");
    actor.move_state.world_x = cx;
    actor.move_state.world_z = cz;

    // Step until the session SM leaves Idle (a region roll fired `trigger_with`),
    // then advance the transition to `Triggered` and drain the formation.
    let mut fired = false;
    for _ in 0..20_000 {
        if host.world.on_field_step() {
            fired = true;
            break;
        }
    }
    assert!(
        fired,
        "[{scene}] an in-region field step triggers within the step budget"
    );
    assert!(
        matches!(
            host.world.encounter.as_ref().map(|s| s.phase()),
            Some(EncounterPhase::Transition { .. })
        ),
        "[{scene}] region trigger drove the mean session into its Transition SM"
    );

    // Advance the transition timer to completion, then drain the roll.
    let mut roll = None;
    for _ in 0..1024 {
        host.world.tick_encounter();
        if let Some(r) = host.world.drain_encounter_formation() {
            roll = Some(r);
            break;
        }
        assert!(
            !matches!(
                host.world.encounter.as_ref().map(|s| s.phase()),
                Some(EncounterPhase::Idle)
            ),
            "[{scene}] transition must reach Triggered, not fall back to Idle"
        );
    }
    let roll = roll.expect("transition resolves to a Triggered roll within the frame budget");
    assert!(
        roll.formation_id < max_formation.max(1),
        "[{scene}] region-routed formation {} is inside the scene's range (< {max_formation})",
        roll.formation_id
    );
    eprintln!(
        "[field-region] '{scene}': region step -> session Transition -> formation {}",
        roll.formation_id
    );
}
