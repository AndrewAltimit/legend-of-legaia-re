//! Disc-gated: the headless live scene preview ([`LiveScene`]) animates what
//! the play hosts animate.
//!
//! - `concnow` (Conkram, present): its entry script replaces the floor-height
//!   ladder, so the heights the scene is shown at are not the shipped ones.
//! - `jouina`: the travelling ladder wave keeps moving.
//! - `town01`: the windmill's sails (a placed prop's clip cursor) turn.
//!
//! `LEGAIA_SCENE_LIVE_SURVEY=1` also prints, for every CDNAME scene the
//! static map viewer offers, what a headless run of it does.
//!
//! Skips without `LEGAIA_DISC_BIN` / `extracted/` (disc-gated convention).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::scene::ProtIndex;
use legaia_engine_core::scene_assembly::assemble_field_scene;
use legaia_engine_core::scene_live::LiveScene;

fn open_index() -> Option<(Arc<ProtIndex>, PathBuf)> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            let ix = ProtIndex::open_extracted(&d).ok()?;
            return Some((Arc::new(ix), d));
        }
    }
    eprintln!("[skip] extracted/ missing");
    None
}

#[test]
fn concnow_is_shown_on_its_installed_ladder() {
    let Some((index, _)) = open_index() else {
        return;
    };
    eprintln!("[ran] concnow");
    let a = assemble_field_scene(&index, "concnow").expect("assemble");
    let mut live = LiveScene::enter(index, "concnow").expect("enter");
    let mut moved = 0usize;
    let mut first: Option<Vec<i32>> = None;
    let mut varied = false;
    for _ in 0..240 {
        live.tick();
        if let Some(o) = live.floor_wave_offsets(&a.terrain) {
            moved = moved.max(o.iter().filter(|&&v| v != 0).count());
            match &first {
                None => first = Some(o),
                Some(f) => varied |= *f != o,
            }
        }
    }
    assert!(live.is_live(), "concnow keeps ticking");
    assert!(
        moved > 0,
        "the entry script's ladder moves the terrain off its shipped heights"
    );
    assert!(varied, "and the ladder keeps animating");
    // The bound placed objects do not ride it: `FUN_8003A55C` stored their Y
    // at scene init, before the system script's install, and nothing
    // re-samples a placed actor's height. Several stand on the very rungs the
    // install raised (nibbles 1..3), so a lift here would be visible.
    let binds = legaia_engine_core::scene::Scene::load(&live.host.index, "concnow")
        .unwrap()
        .field_object_binds(&live.host.index)
        .unwrap();
    let keys: Vec<_> = a
        .placements
        .iter()
        .map(|d| legaia_engine_core::field_env::placed_window_key(d, binds.as_ref()))
        .collect();
    let bound_on_raised = a
        .placements
        .iter()
        .zip(&keys)
        .filter(|(d, k)| k.is_none() && matches!(d.floor.nibble, Some(1..=3)))
        .count();
    assert!(
        bound_on_raised > 0,
        "concnow binds objects on the raised rungs"
    );
    let placed = live.host.world.placed_floor_offsets(
        live.scene_floor_lut(),
        a.placements.iter().map(|d| &d.floor),
        &keys,
    );
    for ((d, k), o) in a.placements.iter().zip(&keys).zip(&placed) {
        if k.is_none() {
            assert_eq!(*o, 0, "bound object at {:?} keeps its spawn Y", d.cell);
        }
    }
    eprintln!(
        "concnow: {moved} of {} terrain draws off their shipped Y",
        a.terrain.len()
    );
}

#[test]
fn jouina_ground_pulses() {
    let Some((index, _)) = open_index() else {
        return;
    };
    eprintln!("[ran] jouina");
    let a = assemble_field_scene(&index, "jouina").expect("assemble");
    let hf = a.ground.as_ref().expect("jouina has a walk ground");
    let mut live = LiveScene::enter(index, "jouina").expect("enter");
    let baked = legaia_engine_core::field_ground::render_positions(hf);
    let mut ground_moved = false;
    let mut prev = None;
    let mut frames_changed = 0;
    for _ in 0..180 {
        live.tick();
        let pos =
            legaia_engine_core::field_ground::live_render_positions(hf, &live.live_floor_lut());
        ground_moved |= pos != baked;
        if prev.as_ref() != Some(&pos) {
            frames_changed += 1;
        }
        prev = Some(pos);
    }
    assert!(ground_moved, "the walk ground follows the live ladder");
    assert!(frames_changed > 10, "and keeps moving ({frames_changed})");
}

#[test]
fn town01_windmill_turns() {
    let Some((index, _)) = open_index() else {
        return;
    };
    eprintln!("[ran] town01");
    let a = assemble_field_scene(&index, "town01").expect("assemble");
    let mut live = LiveScene::enter(index, "town01").expect("enter");
    let animated: Vec<_> = a.placements.iter().filter(|d| d.anim_id != 0).collect();
    assert!(!animated.is_empty(), "town01 places clip-bound props");
    let keys0: Vec<_> = animated.iter().map(|d| live.prop_pose_key(d)).collect();
    // 100 ticks, not 120: the windmill's 30-frame clip at 8 cursor units a
    // tick wraps exactly every 60 ticks.
    for _ in 0..100 {
        live.tick();
    }
    let keys1: Vec<_> = animated.iter().map(|d| live.prop_pose_key(d)).collect();
    assert!(
        keys0.iter().zip(&keys1).any(|(a, b)| a.is_some() && a != b),
        "a prop's clip cursor advances: {keys0:?} -> {keys1:?}"
    );
}

/// `concnow`'s system script re-installs the ladder per room (region-type
/// mask, op `0x42` mode 0 -> `4C 9F` / `4C 9E` / `4C 90`), so a full map
/// must not draw every room on the entry room's ladder: the probe finds the
/// other rooms' ladders, and at least one differs from the live one.
#[test]
fn concnow_rooms_keep_their_own_ladders() {
    let Some((index, _)) = open_index() else {
        return;
    };
    eprintln!("[ran] concnow rooms");
    let a = assemble_field_scene(&index, "concnow").expect("assemble");
    let mut live = LiveScene::enter(index, "concnow").expect("enter");
    for _ in 0..60 {
        live.tick();
    }
    let entry = live.live_floor_lut();
    for r in live.rooms() {
        eprintln!(
            "  room mask {:#010x}: lut {:?} bobs {}",
            r.mask,
            r.lut,
            r.bobs.len()
        );
    }
    eprintln!(
        "  entry mask {:#010x}: lut {entry:?}",
        live.host.world.flags.extra_flags
    );
    assert!(
        !live.rooms().is_empty(),
        "concnow has rooms besides the entry one"
    );
    let distinct = live.rooms().iter().filter(|r| r.lut != entry).count();
    assert!(
        distinct > 0,
        "some room installs a ladder unlike the entry room's"
    );
    // A terrain draw in such a room resolves through that room's ladder, not
    // the live one.
    let entry_wave = legaia_engine_core::field_env::FloorWave::from_scene_and_world(
        live.scene_floor_lut(),
        &entry,
    );
    let per_room = live
        .floor_wave_offsets(&a.terrain)
        .expect("concnow's ladder moved");
    let differs = a
        .terrain
        .iter()
        .zip(&per_room)
        .filter(|(d, o)| entry_wave.map_or(0, |w| w.offset(&d.floor)) != **o)
        .count();
    eprintln!(
        "  {differs} of {} terrain draws take a room ladder other than the entry room's",
        a.terrain.len()
    );
    assert!(
        differs > 0,
        "some terrain draw sits on its own room's ladder"
    );
    // The walk ground too, per cell.
    let hf = a.ground.as_ref().expect("concnow has a walk ground");
    let cells = legaia_engine_core::field_ground::vertex_cells(hf);
    let by_room = live.ground_positions(hf, &cells);
    let entry_only = legaia_engine_core::field_ground::live_render_positions(hf, &entry);
    assert_ne!(by_room, entry_only, "the ground takes each room's ladder");
    // And a room with oscillators keeps moving on its own.
    let before: Vec<_> = live.rooms().iter().map(|r| r.lut).collect();
    for _ in 0..30 {
        live.tick();
    }
    let after: Vec<_> = live.rooms().iter().map(|r| r.lut).collect();
    if live.rooms().iter().any(|r| !r.bobs.is_empty()) {
        assert_ne!(before, after, "a room with oscillators keeps moving");
    }
}

/// The kingdom overworld runs live: entered as the play page enters it
/// (`enter_world_map_scene`), it stays on the world map, ticks without
/// leaving, and its actors' clips run there (retail's overworld is a
/// field-run scene; `FUN_800204F8` steps every actor's clip).
#[test]
fn map01_runs_live() {
    let Some((index, _)) = open_index() else {
        return;
    };
    eprintln!("[ran] map01");
    let mut live = LiveScene::enter(index, "map01").expect("enter map01");
    assert!(live.is_world_map());
    assert_eq!(
        live.host.world.mode,
        legaia_engine_core::world::SceneMode::WorldMap
    );
    assert!(live.rooms().is_empty(), "the overworld has no ladder rooms");
    for _ in 0..240 {
        assert!(live.tick(), "map01 keeps ticking");
    }
    assert_eq!(live.restarts(), 0, "map01 never leaves the scene headless");
    assert!(live.host.world.field_npc_clips_advance());
}

#[test]
fn survey_every_viewer_scene() {
    if std::env::var_os("LEGAIA_SCENE_LIVE_SURVEY").is_none() {
        return;
    }
    let Some((index, dir)) = open_index() else {
        return;
    };
    let cdname = legaia_prot::cdname::parse(&dir.join("CDNAME.TXT")).expect("cdname");
    let mut names: Vec<String> = cdname.values().cloned().collect();
    names.sort();
    names.dedup();
    for name in names {
        let Ok(a) = assemble_field_scene(&index, &name) else {
            continue;
        };
        let t0 = std::time::Instant::now();
        let mut live = match LiveScene::enter(index.clone(), &name) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("{name:10} enter failed: {e}");
                continue;
            }
        };
        let enter_ms = t0.elapsed().as_millis();
        let t1 = std::time::Instant::now();
        let mut wave_draws = 0;
        let mut lut_changes = 0;
        let mut last = live.live_floor_lut();
        let anim: Vec<_> = a.placements.iter().filter(|d| d.anim_id != 0).collect();
        let k0: Vec<_> = anim.iter().map(|d| live.prop_pose_key(d)).collect();
        for _ in 0..600 {
            live.tick();
            let l = live.live_floor_lut();
            if l != last {
                lut_changes += 1;
                last = l;
            }
            if let Some(o) = live.floor_wave_offsets(&a.terrain) {
                wave_draws = wave_draws.max(o.iter().filter(|&&v| v != 0).count());
            }
        }
        let k1: Vec<_> = anim.iter().map(|d| live.prop_pose_key(d)).collect();
        let props_moving = k0.iter().zip(&k1).filter(|(a, b)| a != b).count();
        eprintln!(
            "{name:10} enter {enter_ms:4}ms tick600 {:5}ms live={} restarts={} lut_changes={lut_changes} wave_terrain={wave_draws}/{} props_moving={props_moving}/{} ambient={} rooms={} distinct_rooms={}",
            t1.elapsed().as_millis(),
            live.is_live(),
            live.restarts(),
            a.terrain.len(),
            anim.len(),
            live.host.world.ambient.fx.len(),
            live.rooms().len(),
            live.rooms()
                .iter()
                .filter(|r| r.lut != live.live_floor_lut())
                .count(),
        );
    }
}
