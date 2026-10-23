//! Disc-gated: entering the fishing minigame through the shared entry gives
//! both play hosts the pond surface (`engine-core::fishing_scene`) - the
//! `other1` venue with three seated bodies, framed by the venue camera over
//! the lead on the captured floor - and the venue floor off the pond's own
//! `.MAP`, not the departure field's. Leaving drops the surface and keeps the
//! departure scene loaded.
//!
//! Only structural facts are asserted. Skips and passes without
//! `LEGAIA_DISC_BIN`.

use legaia_engine_minigame_scenes::fishing_scene::FishingSurface;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_overlay_images::static_overlay;

#[test]
fn the_pond_surface_seats_the_party_under_the_venue_camera() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");
    host.load_scene("town01").expect("town01");
    let rec = static_overlay::overlay_map()
        .by_prot_index(legaia_minigame_formats::fishing_species::FISHING_OVERLAY_PROT_INDEX as u32)
        .expect("fishing overlay in static map");
    let raw = host.index.entry_bytes_extended(rec.prot_index).unwrap();
    let loaded = static_overlay::as_loaded(&raw, rec).unwrap();
    assert!(host.enter_fishing_from_overlay(&loaded));
    assert_eq!(host.world.mode, SceneMode::Fishing);

    // The venue arms on the pond's own floor: the lead settles at -128.
    legaia_engine_scene_host::fishing_venue::tick_fishing_venue_on_host(&mut host);
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
    // The HUD's sprite table decoded on the same entry, and every record cuts
    // a page the pond VRAM holds: the 4bpp page at (832, 0) with a palette of
    // the (0, 503) strip, both non-empty in the surface's VRAM.
    let sprites = host
        .world
        .minigames
        .fishing_sprites
        .as_ref()
        .expect("sprite table");
    assert_eq!(
        sprites.len(),
        legaia_minigame_formats::fishing_sprites::FISHING_SPRITE_COUNT
    );
    for r in sprites {
        assert_eq!(r.tpage & 0x1F, 0x0D, "page (832, 0)");
        assert_eq!(r.clut >> 6, 503, "a palette of the 503 strip");
    }
    let vram = surface.vram().unwrap().as_bytes();
    let word = |x: usize, y: usize| {
        u16::from_le_bytes([vram[(y * 1024 + x) * 2], vram[(y * 1024 + x) * 2 + 1]])
    };
    assert!((0..256).any(|x| word(x, 503) != 0), "CLUT row 503 resident");
    let page_texels = (0..256)
        .flat_map(|y| (832..896).map(move |x| (x, y)))
        .filter(|&(x, y)| word(x, y) != 0)
        .count();
    assert!(
        page_texels > 4096,
        "HUD page resident ({page_texels} non-zero words)"
    );
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

/// The lead's ambient ripple runs the overlay's own part record through the
/// move VM: the record turns the part into a default-arm ring (op `0x13`),
/// seated out on the water ahead of the lead, and the ring retires when its
/// program ends.
#[test]
fn the_ambient_ripple_is_the_overlays_ring_part() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");
    host.load_scene("town01").expect("town01");
    let rec = static_overlay::overlay_map()
        .by_prot_index(legaia_minigame_formats::fishing_species::FISHING_OVERLAY_PROT_INDEX as u32)
        .expect("fishing overlay in static map");
    let raw = host.index.entry_bytes_extended(rec.prot_index).unwrap();
    let loaded = static_overlay::as_loaded(&raw, rec).unwrap();
    assert!(host.enter_fishing_from_overlay(&loaded));
    assert!(host.world.minigames.fishing_venue.ripple_records.is_some());
    let mut seen = 0usize;
    let mut retired = false;
    let mut live = 0usize;
    for _ in 0..600 {
        legaia_engine_scene_host::fishing_venue::tick_fishing_venue_on_host(&mut host);
        let draws = host.world.minigames.fishing_venue.ripple_draws();
        if let Some(d) = draws.first() {
            seen += 1;
            assert!(!d.mesh.positions.is_empty(), "the ring has geometry");
            // Out on the water ahead of the lead (`z + 0x400` give or take the
            // roll's jitter and the part's own drift), not on the shore.
            assert!(d.world_pos[2] > 10752.0 + 512.0, "pos {:?}", d.world_pos);
            assert!(
                (d.world_pos[0] - 4736.0).abs() < 1024.0,
                "pos {:?}",
                d.world_pos
            );
        }
        // A ring whose program ended leaves the list.
        retired |= draws.len() < live;
        live = draws.len();
        assert!(host.world.active_effect_kind4_draws().len() >= draws.len());
    }
    assert!(seen > 0, "no ripple ring drew in 600 frames");
    assert!(retired, "the ring never retired");
    eprintln!("[ran] ripple ring drew for {seen} frames");
}
