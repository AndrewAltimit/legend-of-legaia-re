//! Disc-gated: entering the fishing minigame through the shared entry gives
//! both play hosts the pond surface (`engine-core::fishing_scene`) - the
//! `other1` venue with three seated bodies, framed by the venue camera over
//! the lead on the captured floor - and the venue floor off the pond's own
//! `.MAP`, not the departure field's. Leaving drops the surface and keeps the
//! departure scene loaded.
//!
//! Only structural facts are asserted. Skips and passes without
//! `LEGAIA_DISC_BIN`.

use legaia_asset::static_overlay;
use legaia_engine_core::fishing_scene::FishingSurface;
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::SceneMode;

#[test]
fn the_pond_surface_seats_the_party_under_the_venue_camera() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");
    host.load_scene("town01").expect("town01");
    let rec = static_overlay::overlay_map()
        .by_prot_index(legaia_asset::fishing_species::FISHING_OVERLAY_PROT_INDEX as u32)
        .expect("fishing overlay in static map");
    let raw = host.index.entry_bytes_extended(rec.prot_index).unwrap();
    let loaded = static_overlay::as_loaded(&raw, rec).unwrap();
    assert!(host.enter_fishing_from_overlay(&loaded));
    assert_eq!(host.world.mode, SceneMode::Fishing);

    // The venue arms on the pond's own floor: the lead settles at -128.
    legaia_engine_core::fishing_venue::tick_fishing_venue_on_host(&mut host);
    let lead = host.world.minigames.fishing_venue.wander.clone().unwrap();
    assert_eq!((lead.x, lead.y, lead.z), (4736, -128, 10752));

    let mut surface = FishingSurface::default();
    assert!(
        surface
            .frame(&host.index, &host.world.minigames, true)
            .is_some(),
        "the pond decodes"
    );
    assert!(surface.vram().is_some());
    let scene = surface.scene().unwrap();
    assert_eq!(scene.bases.len(), 3, "three seated bodies");
    assert!(scene.textured_indices.len() > 3000);
    assert_eq!(scene.camera.h, 320.0);
    assert_eq!(scene.camera.focus, [4736.0, 0.0, 10752.0]);
    eprintln!(
        "[ran] pond surface: {} verts, {} textured tris",
        scene.positions.len(),
        scene.textured_indices.len() / 3
    );

    // Leaving drops the surface; the departure field is still the scene.
    host.world.exit_fishing();
    assert!(
        surface
            .frame(
                &host.index,
                &host.world.minigames,
                host.world.mode == SceneMode::Fishing
            )
            .is_none()
    );
    assert_eq!(host.scene.as_ref().map(|s| s.name.as_str()), Some("town01"));
}
