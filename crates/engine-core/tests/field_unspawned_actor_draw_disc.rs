//! The per-actor mesh pass draws spawned actors only.
//!
//! `World::init_scene_animations` binds every actor slot `K` to scene TMD `K`
//! so a later field-VM spawn finds its mesh. The never-spawned slots stay
//! bound, inactive and parked at the origin; the native window used to draw
//! them all, which painted uru's whole scene pack at world `(0, 0, 0)` and
//! smeared its sky / cliff geometry over the frame (`uru_field_run` in the
//! retail comparison corpus). Retail allocates no actor for a registered
//! TMD, so nothing is drawn there.
//!
//! Disc-gated: skips (and passes) without `LEGAIA_DISC_BIN`.

use legaia_engine_core::scene::SceneHost;

#[test]
fn uru_draws_no_unspawned_bound_actor() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|p| std::path::Path::new(p).exists())
    else {
        eprintln!("skip: LEGAIA_DISC_BIN unset");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");
    host.enter_field_scene("uru", 0).expect("enter uru");
    let w = &host.world;

    let bound = (0..w.actors.len())
        .filter(|&i| w.actors[i].tmd_binding.is_some())
        .count();
    let unspawned_bound = (0..w.actors.len())
        .filter(|&i| w.actors[i].tmd_binding.is_some() && !w.actors[i].active)
        .count();
    // Non-vacuity: the pre-binding really leaves inactive bound slots behind.
    assert!(
        unspawned_bound > 10,
        "uru pre-binds its scene pack onto actor slots ({unspawned_bound} inactive bound)"
    );
    let drawn: Vec<usize> = (0..w.actors.len())
        .filter(|&i| w.actor_slot_drawn(i, false))
        .collect();
    assert!(
        drawn.iter().all(|&i| w.actors[i].active),
        "only spawned actors draw ({drawn:?})"
    );
    assert!(drawn.contains(&0), "the player (slot 0) still draws");
    // The synthetic battle camera keeps drawing every bound body.
    let synthetic = (0..w.actors.len())
        .filter(|&i| w.actor_slot_drawn(i, true))
        .count();
    assert_eq!(synthetic, bound);
    println!(
        "uru: {bound} bound actor slots, {unspawned_bound} never spawned, {} drawn",
        drawn.len()
    );
}
