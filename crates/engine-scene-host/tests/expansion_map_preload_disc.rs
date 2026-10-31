//! Disc-gated: pack maps prepared ahead of their entry
//! (`legaia_engine_scene_host::expansion_preload`) on the flagship pack.
//!
//! - A map entered from the cache, or prepared on a worker by the map whose
//!   exit leads to it, plays bit-identically to the same map built cold: the
//!   same field image, the same scene VRAM and meshes, the same world after
//!   a stretch of walking.
//! - Entering a map asks for exactly the maps its exits lead to, and the
//!   cache stays bounded.
//! - The timing report: every Rebirth map entered cold, then again through
//!   the cache (printed with `--nocapture`; no timing is asserted).
//!
//! Skips when `LEGAIA_DISC_BIN` is unset.

use std::sync::Arc;
use std::time::Instant;

use legaia_engine_save_state::WorldSaveState;
use legaia_engine_scene_host::expansion_preload::{
    PREPARED_MAP_BUDGET, PREPARED_MAP_CAPACITY, connected_maps,
};
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;
use legaia_expansion_format::ExpansionPack;

fn rebirth() -> Arc<ExpansionPack> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
    let files = legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read");
    Arc::new(ExpansionPack::from_files(&files).expect("the pack parses"))
}

fn host(pack: &Arc<ExpansionPack>) -> Option<SceneHost> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    };
    let mut host = SceneHost::open_disc(disc).expect("open the disc");
    host.activate_expansion_pack(pack.clone())
        .expect("Rebirth activates");
    Some(host)
}

/// Wait for every background preparation to land in the cache.
fn settle(host: &mut SceneHost) {
    let t = Instant::now();
    while !host.expansion_preload.pending_maps().is_empty() {
        host.step_expansion_preload();
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert!(t.elapsed().as_secs() < 120, "the preparation never landed");
    }
}

/// What the entry left on the host: the field image, the scene's VRAM and
/// meshes.
fn scene_snapshot(host: &SceneHost) -> (Vec<u8>, Vec<u16>, Vec<Vec<u8>>) {
    let scene = host.scene.as_ref().expect("a scene");
    let image = scene
        .overrides
        .as_ref()
        .expect("an override")
        .field_map
        .clone();
    let res = host.resources.as_ref().expect("resources");
    let vram: Vec<u16> = (0..512)
        .flat_map(|y| (0..1024).map(move |x| (x, y)))
        .map(|(x, y)| res.vram.pixel(x, y))
        .collect();
    let tmds = res.tmds.iter().map(|t| t.raw.clone()).collect();
    (image, vram, tmds)
}

fn walk(host: &mut SceneHost) {
    for (btn, n) in [
        (PadButton::Up, 40),
        (PadButton::Left, 30),
        (PadButton::Down, 30),
    ] {
        for _ in 0..n {
            host.world.set_pad(btn.mask());
            host.tick().expect("tick");
        }
    }
    host.world.set_pad(0);
}

#[test]
fn a_prepared_map_plays_as_one_built_cold() {
    let pack = rebirth();
    let Some(mut cold) = host(&pack) else {
        return;
    };
    let mut warm = host(&pack).expect("second host");
    warm.set_expansion_preload(true);
    // The warm host enters the beach, whose exit prepares the village on a
    // worker; the cold host enters the village with nothing prepared.
    warm.enter_expansion_map("isles_beach", "").expect("beach");
    assert!(
        warm.expansion_preload
            .pending_maps()
            .contains(&"isles_village")
            || warm
                .expansion_preload
                .ready_maps()
                .contains(&"isles_village"),
        "the beach's exit asks for the village"
    );
    settle(&mut warm);
    assert!(
        warm.expansion_preload
            .ready_maps()
            .contains(&"isles_village")
    );
    cold.enter_expansion_map("isles_beach", "").expect("beach");
    cold.expansion_preload.clear();
    cold.enter_expansion_map("isles_village", "")
        .expect("village cold");
    warm.enter_expansion_map("isles_village", "")
        .expect("village warm");
    assert!(
        warm.expansion_entry_build().is_some(),
        "the warm entry used the prepared build"
    );
    let (ci, cv, ct) = scene_snapshot(&cold);
    let (wi, wv, wt) = scene_snapshot(&warm);
    assert!(ci == wi, "the field images differ");
    assert!(cv == wv, "the scene VRAM differs");
    assert!(ct == wt, "the scene meshes differ");
    walk(&mut cold);
    walk(&mut warm);
    assert!(
        cold.world.save_full() == warm.world.save_full(),
        "the worlds part after the same walk"
    );
    let a = &cold.world.actors[0].move_state;
    let b = &warm.world.actors[0].move_state;
    assert_eq!(
        (a.world_x, a.world_y, a.world_z),
        (b.world_x, b.world_y, b.world_z)
    );
    // Re-entering a map the cache holds is the same entry again.
    cold.enter_expansion_map("isles_village", "")
        .expect("village again");
    let (ri, rv, rt) = scene_snapshot(&cold);
    assert!(
        ri == ci && rv == cv && rt == ct,
        "the cached re-entry differs"
    );
}

#[test]
fn an_entry_asks_for_its_exits_and_the_cache_stays_bounded() {
    let pack = rebirth();
    let Some(mut host) = host(&pack) else {
        return;
    };
    host.set_expansion_preload(true);
    let mut ids: Vec<String> = pack.maps.keys().cloned().collect();
    ids.sort();
    for id in &ids {
        host.enter_expansion_map(id, "").expect("enter");
        let mut asked: Vec<String> = host
            .expansion_preload
            .pending_maps()
            .iter()
            .chain(host.expansion_preload.ready_maps().iter())
            .map(|s| s.to_string())
            .collect();
        asked.sort();
        for next in connected_maps(&pack.maps[id]) {
            assert!(
                asked.contains(&next),
                "{id}: exit map {next} neither preparing nor ready"
            );
        }
        settle(&mut host);
        let ready = host.expansion_preload.ready_maps().len();
        assert!(ready <= PREPARED_MAP_CAPACITY);
        assert!(ready == 1 || host.expansion_preload.approx_bytes() <= PREPARED_MAP_BUDGET);
    }
}

/// A hot reload - the same pack activated again as a new value - keeps the
/// maps it prepared; a version that changes one map's definition drops that
/// map and keeps the rest, and the map built again plays as one built cold.
#[test]
fn a_reload_keeps_the_maps_it_left_unchanged() {
    let pack = rebirth();
    let Some(mut host) = host(&pack) else {
        return;
    };
    host.enter_expansion_map("rebirth_tower_1f", "")
        .expect("enter 1f");
    host.enter_expansion_map("rebirth_tower_2f", "")
        .expect("enter 2f");
    let cold = scene_snapshot(&host);
    // The same pack again.
    let same = Arc::new((*pack).clone());
    host.activate_expansion_pack(same.clone())
        .expect("reactivates");
    host.enter_expansion_map("rebirth_tower_2f", "")
        .expect("re-enter");
    let mut ready = host.expansion_preload.ready_maps();
    ready.sort();
    assert_eq!(ready, ["rebirth_tower_1f", "rebirth_tower_2f"]);
    assert!(scene_snapshot(&host) == cold, "the kept map differs");
    // 1f edited: it goes, 2f stays.
    let mut edited = (*same).clone();
    edited
        .maps
        .get_mut("rebirth_tower_1f")
        .expect("1f")
        .name
        .push_str(" (edited)");
    host.activate_expansion_pack(Arc::new(edited))
        .expect("the edit activates");
    host.enter_expansion_map("rebirth_tower_2f", "")
        .expect("enter 2f");
    assert_eq!(host.expansion_preload.ready_maps(), ["rebirth_tower_2f"]);
}

#[test]
fn time_rebirth_map_entries() {
    let pack = rebirth();
    let Some(mut host) = host(&pack) else {
        return;
    };
    let mut ids: Vec<String> = pack.maps.keys().cloned().collect();
    ids.sort();
    for id in &ids {
        host.expansion_preload.clear();
        let t = Instant::now();
        host.enter_expansion_map(id, "").expect("enter cold");
        let cold = t.elapsed();
        let bytes = host.expansion_preload.approx_bytes();
        let t = Instant::now();
        host.enter_expansion_map(id, "").expect("enter prepared");
        let warm = t.elapsed();
        eprintln!(
            "{id:18} cold {:7.1} ms  prepared {:6.1} ms  cached {:5.1} MiB",
            cold.as_secs_f64() * 1e3,
            warm.as_secs_f64() * 1e3,
            bytes as f64 / (1024.0 * 1024.0)
        );
    }
}

/// The cold preparation of every Rebirth map phase by phase (library, field
/// image, NPCs, resource build), then the entry from the cache: where a
/// map's entry time goes (printed with `--nocapture`; no timing is
/// asserted).
#[test]
fn time_rebirth_map_phases() {
    use legaia_engine_scene_host::expansion_preload::PrepareJob;
    let pack = rebirth();
    let Some(mut host) = host(&pack) else {
        return;
    };
    let index = host.index.clone();
    let mut ids: Vec<String> = pack.maps.keys().cloned().collect();
    ids.sort();
    for id in &ids {
        let mut job = PrepareJob::new(id);
        let mut phases = Vec::new();
        loop {
            let t = Instant::now();
            let done = job.step(&index, &pack).expect("phase");
            phases.push(t.elapsed().as_secs_f64() * 1e3);
            if done.is_some() {
                break;
            }
        }
        host.expansion_preload.clear();
        host.enter_expansion_map(id, "").expect("enter cold");
        let t = Instant::now();
        host.enter_expansion_map(id, "").expect("enter prepared");
        let warm = t.elapsed().as_secs_f64() * 1e3;
        eprintln!(
            "{id:18} library {:7.1}  image {:7.1}  npcs {:7.1}  build {:7.1}  | entry from cache {:7.1} ms",
            phases[0], phases[1], phases[2], phases[3], warm
        );
    }
}
