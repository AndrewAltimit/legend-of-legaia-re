//! Disc-gated: every formation a scene can roll must resolve to monsters.
//!
//! A rolled encounter passes through two id spaces before it becomes a
//! fight. The **roll** produces a MAN formation-row index - either from the
//! aggregated mean-rate table (`encounter_man::encounter_table_from_man`) or,
//! on a scene whose MAN carries encounter regions, from the region's
//! `[formation_range_base, +formation_range_count)` slice (the faithful
//! `FUN_801D9E1C` model). The **battle** then looks that index up in
//! `World::formation_table`, which `install_man_encounter` populates from the
//! same MAN. If the two ever disagree the roll evaporates in
//! `begin_encounter_battle` and the player walks on with no fight and nothing
//! on screen to explain it.
//!
//! So this walks the whole CDNAME scene list, installs each scene's real MAN
//! encounter source through the real installer, and asserts every id either
//! space can produce resolves - the corpus-wide version of "rikuroa should
//! fight".
//!
//! The second test is the regression for the defect that produced this file:
//! a host that cleared `World::encounter` after scene entry (the New Game
//! reset does exactly that) used to leave the region tracker rolling into a
//! null sink, so every roll was consumed - RNG drawn, anti-repeat latched,
//! counter re-seeded - and thrown away.
//!
//! Skips silently when `extracted/` is missing.

use std::collections::BTreeSet;
use std::path::PathBuf;

use legaia_engine_core::live_loop::LiveLoopOpts;
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::SceneMode;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Every formation row a rate-bearing region of `man` can pick.
fn region_rollable_rows(man: &[u8]) -> BTreeSet<u16> {
    let mut ids = BTreeSet::new();
    let Some(table) =
        legaia_engine_core::region_encounter::region_encounter_table_from_man("corpus-probe", man)
    else {
        return ids;
    };
    for r in &table.regions {
        // A rate-0 region never advances the counter, so it never rolls -
        // that is how the scripted / boss rows stay out of the random pool
        // (see docs/formats/encounter.md). Only rate-bearing rows count.
        if r.rate_increment == 0 || r.formation_count == 0 {
            continue;
        }
        for i in 0..r.formation_count {
            ids.insert(u16::from(r.formation_base.wrapping_add(i)));
        }
    }
    ids
}

#[test]
fn every_rollable_formation_resolves_across_the_scene_corpus() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut scenes: Vec<String> = cdname.values().cloned().collect();
    scenes.sort();
    scenes.dedup();

    let mut checked = 0usize;
    let mut rows_checked = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for scene in &scenes {
        if host.load_scene(scene).is_err() {
            continue;
        }
        let Some(sc) = host.scene.as_ref() else {
            continue;
        };
        let Ok(Some(man)) = sc.field_man_payload(&host.index) else {
            continue;
        };
        let Some((table, defs)) =
            legaia_engine_core::encounter_man::scene_encounter_from_man(scene, &man)
        else {
            continue;
        };
        // Install through the real installer, not a re-implementation: the
        // pairing under test is the one the scene-entry path produces.
        let mean_rows: Vec<u16> = table.entries.iter().map(|e| e.formation_id).collect();
        host.world.set_active_scene_label(scene);
        host.world.install_man_encounter(table, defs);
        host.world.set_field_regions(
            legaia_engine_core::region_encounter::region_encounter_table_from_man(scene, &man),
        );

        let registered: BTreeSet<u16> = host.world.registered_formation_ids().into_iter().collect();
        let mut want: BTreeSet<u16> = mean_rows.into_iter().collect();
        want.extend(region_rollable_rows(&man));
        rows_checked += want.len();

        for id in &want {
            match host.world.formation_table.formation(*id) {
                None => failures.push(format!(
                    "{scene}: rollable formation row {id} is not registered (registered {registered:?})"
                )),
                Some(def) if def.slots.is_empty() => failures.push(format!(
                    "{scene}: rollable formation row {id} registered with zero monsters"
                )),
                Some(_) => {}
            }
        }
        checked += 1;
    }

    assert!(
        checked >= 40,
        "expected the CDNAME corpus to yield MAN encounter tables for many scenes, got {checked}"
    );
    assert!(
        rows_checked >= 200,
        "expected hundreds of rollable rows across the corpus, got {rows_checked}"
    );
    assert!(
        failures.is_empty(),
        "{} unresolvable rollable formation(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
    eprintln!("[ok] {checked} scenes, {rows_checked} rollable formation rows all resolve");
}

/// The New-Game reset clears `World::encounter`. When a host runs it *after*
/// scene entry (which `play-window --seed-party` does), the per-region
/// tracker is left installed with nothing to trigger into - and every roll it
/// produced used to be discarded in `on_field_step`.
///
/// Walk a real region-bearing scene after that reset and require an actual
/// battle. Also asserts the pre-reset baseline so the test cannot pass by
/// the scene simply never rolling.
#[test]
fn a_region_scene_still_reaches_battle_after_the_new_game_reset() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    for (scene, reset) in [("rikuroa", false), ("rikuroa", true), ("map03", true)] {
        let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
        if host.enter_field_scene(scene, 0).is_err() {
            eprintln!("[skip] {scene}: enter_field_scene failed on this extraction");
            continue;
        }
        host.world.arm_live_loop(scene, &LiveLoopOpts::playable());
        if reset {
            // The exact call `BootSession::begin_new_game` makes.
            host.world.begin_new_game();
            assert!(
                host.world.encounter.is_none(),
                "the reset is expected to clear the session - that is the state under test"
            );
        }
        let world = &mut host.world;
        let Some(table) = world
            .field_region_tracker
            .as_ref()
            .map(|t| t.table().clone())
        else {
            panic!("{scene} must route a per-region field encounter tracker");
        };
        // Seat inside a rate-bearing region that is not shadowed by an
        // earlier row (region lookup stops at the first containing AABB).
        let seat = table.regions.iter().find_map(|r| {
            if r.rate_increment == 0 || r.formation_count == 0 {
                return None;
            }
            let cx = ((r.tile_x_min as u16 + r.tile_x_max as u16) / 2) as u8;
            let cz = ((r.tile_z_min as u16 + r.tile_z_max as u16) / 2) as u8;
            let first = table.region_at_tile(cx as i32, cz as i32)?;
            std::ptr::eq(first, r).then_some((cx, cz))
        });
        let Some((cx, cz)) = seat else {
            panic!("{scene} has no unshadowed rate-bearing region to stand in");
        };
        world.seat_player_at_tile_rescued(cx, cz);
        world.live_gameplay_loop = true;
        world.battle_player_driven = false;

        let mut triggered = false;
        for _ in 0..40_000 {
            if world.on_field_step() {
                triggered = true;
                break;
            }
        }
        assert!(
            triggered,
            "{scene} (new-game reset: {reset}) must roll a region encounter into the transition SM"
        );
        let mut entered = false;
        for _ in 0..2_000 {
            world.tick();
            if world.mode == SceneMode::Battle {
                entered = true;
                break;
            }
        }
        assert!(
            entered,
            "{scene} (new-game reset: {reset}) rolled but never entered battle - the formation \
             did not resolve"
        );
    }
}

/// `World::force_encounter` is the `play-window --battle <row>` harness's
/// engine side. It must reach battle **through the ordinary transition path**
/// (a harness that bypasses the path it verifies proves nothing), and it must
/// refuse an unregistered row rather than half-arming one.
#[test]
fn force_encounter_drives_a_named_row_through_the_normal_transition() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let scene = "rikuroa";
    if host.enter_field_scene(scene, 0).is_err() {
        eprintln!("[skip] {scene}: enter_field_scene failed on this extraction");
        return;
    }
    host.world.arm_live_loop(scene, &LiveLoopOpts::playable());
    host.world.live_gameplay_loop = true;
    host.world.battle_player_driven = false;
    let world = &mut host.world;

    // An unregistered row changes nothing.
    let bogus = world
        .registered_formation_ids()
        .last()
        .copied()
        .unwrap_or(0)
        + 1000;
    assert!(!world.force_encounter(bogus));
    assert!(matches!(
        world
            .encounter
            .as_ref()
            .map(|s| s.phase())
            .unwrap_or(legaia_engine_core::encounter::EncounterPhase::Idle),
        legaia_engine_core::encounter::EncounterPhase::Idle
    ));

    // rikuroa row 17 is the scripted lone-Caruban boss row (its formation
    // record's `record[+0]` is non-zero, which is what raises the per-battle
    // `0x80`). Force it and require the battle to open with that monster.
    let row = 17u16;
    let expect_ids: Vec<u16> = world
        .formation_table
        .formation(row)
        .map(|d| d.slots.iter().map(|s| s.monster_id).collect())
        .unwrap_or_default();
    assert!(
        !expect_ids.is_empty(),
        "rikuroa row {row} must be a registered formation"
    );
    assert_ne!(
        world
            .formation_table
            .formation(row)
            .map(|d| d.per_battle_flags())
            .unwrap_or(0),
        0,
        "rikuroa's scripted boss row must raise the per-battle 0x80 flag"
    );
    assert!(world.force_encounter(row));
    let mut entered = false;
    for _ in 0..2_000 {
        world.tick();
        if world.mode == SceneMode::Battle {
            entered = true;
            break;
        }
    }
    assert!(entered, "forced formation row {row} must enter battle");
    let seated: Vec<u16> = world
        .actors
        .iter()
        .filter_map(|a| a.battle_monster_id)
        .collect();
    assert_eq!(
        seated, expect_ids,
        "the battle must seat exactly the forced row's monsters"
    );
}
