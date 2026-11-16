//! A scene entered over another binds its actor slots as the same scene
//! entered cold does.
//!
//! `World::init_scene_animations` binds slot `K` to scene TMD `K`; a slot past
//! the scene's pack has no model (`FUN_8001E890` registers the pack and
//! nothing else). It used to leave such a slot as the departing scene had
//! bound it, so `town01 -> rugi` kept slots `50..62` bound to TMDs `rugi`
//! does not have. Nothing drew them while they stayed unspawned, but the
//! host trace's binding line and the floor snap of bound actors read them,
//! and the play page lands a card save over its running scene where the
//! window lands it cold.
//!
//! Disc-gated: skips (and passes) without `LEGAIA_DISC_BIN`.

use legaia_engine_scene_host::scene_host::SceneHost;

fn bindings(host: &SceneHost) -> Vec<(usize, Option<usize>, bool, bool)> {
    let w = &host.world;
    (0..w.actors.len())
        .map(|i| {
            let a = &w.actors[i];
            (i, a.tmd_binding, a.active, a.active_animation.is_some())
        })
        .collect()
}

#[test]
fn a_scene_entered_over_another_binds_as_a_cold_entry() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|p| std::path::Path::new(p).exists())
    else {
        eprintln!("skip: LEGAIA_DISC_BIN unset");
        return;
    };
    let mut stale_before = 0;
    for (first, second) in [("town01", "rugi"), ("rugi", "map01"), ("uru", "town01")] {
        let mut cold = SceneHost::open_disc(&disc).expect("open disc");
        cold.enter_field_scene(second, 0).expect("enter cold");
        let mut warm = SceneHost::open_disc(&disc).expect("open disc");
        warm.enter_field_scene(first, 0).expect("enter first");
        for _ in 0..60 {
            let _ = warm.tick();
        }
        // Non-vacuity: the first scene binds slots the second's pack lacks.
        let second_pack = cold
            .world
            .actors
            .iter()
            .filter(|a| a.tmd_binding.is_some())
            .count();
        stale_before += warm
            .world
            .actors
            .iter()
            .enumerate()
            .filter(|(i, a)| *i >= second_pack && a.tmd_binding.is_some())
            .count();
        warm.enter_field_scene(second, 0).expect("enter second");
        let (c, w) = (bindings(&cold), bindings(&warm));
        let diffs: Vec<_> = c.iter().zip(w.iter()).filter(|(a, b)| a != b).collect();
        assert!(
            diffs.is_empty(),
            "{first} -> {second}: slots bound unlike a cold entry (cold, warm): {diffs:?}"
        );
    }
    assert!(
        stale_before > 10,
        "a longer pack bound slots past the next scene's ({stale_before})"
    );
}
