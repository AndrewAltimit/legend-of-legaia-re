//! Disc-gated: the dance floor's bodies through the shared surface kernel
//! (`dance_cast_scene::DanceCastSurface`) every host draws, one run per mode.
//!
//! * Each mode's floor carries exactly the bodies the spawner seats - the
//!   mode's spawn-table records, plus the Disco King in the how-to mode - and
//!   every body resolves to a mesh and a clip.
//! * A mode change rebuilds the buffers (the generation moves), so a host
//!   holding the qualifier's three never draws them over a finals or
//!   free-play floor.
//! * Every body stands at its spawn position: the posed centroid of each
//!   body's range lies within a body's reach of it.
//! * The pose moves over time (the clip driver runs during the count-in).
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::dance::{DEMO_POS, DanceBodyModel, DanceGame, DanceMode};
use legaia_engine_core::dance_cast_scene::{DanceCastAssets, DanceCastSurface, resolve_clip};
use legaia_engine_core::dance_venue::DanceVenue;
use legaia_engine_core::scene::ProtIndex;

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
fn every_mode_draws_its_own_floor() {
    let Some(dir) = gate() else { return };
    eprintln!("[ran] dance cast surface");
    let index = ProtIndex::open_extracted(&dir).expect("prot index");
    let rec = legaia_asset::static_overlay::overlay_map()
        .by_prot_index(legaia_asset::dance_chart::DANCE_OVERLAY_PROT_INDEX as u32)
        .expect("0980 mapped");
    let raw = index.entry_bytes_extended(rec.prot_index).expect("0980");
    let overlay = legaia_asset::static_overlay::as_loaded(&raw, rec).expect("loaded form");
    let cast = legaia_asset::dance_cast::parse(&overlay).expect("cast");
    let venue = DanceVenue::build(&index, Some(&overlay)).expect("venue");
    let pack = index
        .entry_bytes(legaia_asset::character_pack::PROT_ENTRY_INDEX)
        .expect("0874");
    let assets = DanceCastAssets::from_venue(&venue, &cast, Some(pack.as_slice())).expect("assets");

    let mut surface = DanceCastSurface::default();
    surface.set_assets(Some(Arc::new(assets.clone())));
    let mut last_gen = surface.generation();
    for mode in [
        DanceMode::Qualifier,
        DanceMode::Finals,
        DanceMode::HowTo,
        DanceMode::FreePlay,
    ] {
        let mut game = DanceGame::from_overlay_for_mode(&overlay, mode, false).expect("run");
        let bodies = game.body_frames();
        let want = mode.cast_size() + usize::from(mode == DanceMode::HowTo);
        assert_eq!(bodies.len(), want, "{mode:?} body count");
        assert!(matches!(bodies[0].model, DanceBodyModel::Resident(1)));
        if mode == DanceMode::HowTo {
            let king = bodies.last().unwrap();
            assert_eq!(king.kind, None);
            assert_eq!(king.pos, DEMO_POS);
            assert_eq!(king.model, DanceBodyModel::Scene(0x3F));
        }
        for b in &bodies {
            assert!(
                resolve_clip(assets.anm(), b).is_some(),
                "{mode:?}: body {b:?} has no clip"
            );
        }

        let scene = surface.frame(Some(&game)).expect("scene");
        let first: Vec<[f32; 3]> = scene.positions.clone();
        for (i, b) in bodies.iter().enumerate() {
            let r = scene.body_range(i).unwrap();
            assert!(!r.is_empty(), "{mode:?}: body {i} has no mesh");
            let n = r.len() as f32;
            let c = r.clone().fold([0.0f32; 3], |mut a, v| {
                for (k, ak) in a.iter_mut().enumerate() {
                    *ak += scene.positions[v][k] / n;
                }
                a
            });
            let dx = c[0] - f32::from(b.pos[0]);
            let dz = c[2] - f32::from(b.pos[2]);
            assert!(
                dx.hypot(dz) < 400.0,
                "{mode:?}: body {i} centroid {c:?} far from spawn {:?}",
                b.pos
            );
        }
        assert_ne!(surface.generation(), last_gen, "{mode:?} rebuilt");
        last_gen = surface.generation();

        for _ in 0..24 {
            game.advance_body_clips(1);
        }
        let later = surface.frame(Some(&game)).expect("scene").positions.clone();
        assert_eq!(surface.generation(), last_gen, "a pose is not a rebuild");
        assert_ne!(first, later, "{mode:?}: the idle clip moves");
    }
    assert!(surface.frame(None).is_none());
}
