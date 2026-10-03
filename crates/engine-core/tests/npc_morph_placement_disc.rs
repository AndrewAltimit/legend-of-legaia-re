//! Disc-gated: a MAN **placement**'s op-`0x4B` morph reaches its mesh.
//!
//! Rim Elm's (`town01`) Genesis tree is placement `P1[50]`, a clip-less
//! actor whose prologue arms VDF sub-entries `3..=6` and holds them at peak
//! before story flag `0x142` is set: the withered tree the
//! `first_town_interactive` capture draws, where the authored mesh is the
//! revived one. `World::npc_morphed_tmd` is the staging kernel both hosts'
//! NPC draws go through (native re-pose / static re-stage, the browser
//! page's `play_npc_morph_base`); this pins that it hands back a displaced
//! copy of the placement's own model for the tree, and nothing for a slot
//! without a live lane.
//!
//! Skip-passes without `LEGAIA_DISC_BIN`.

use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::MorphOwner;

const TREE_SLOT: u8 = 50;

#[test]
fn rim_elm_withered_genesis_tree_stages_its_morph_onto_the_placement_mesh() {
    let disc = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|p| std::path::Path::new(p).exists());
    let Some(disc) = disc else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");
    host.enter_field_scene("town01", 0).expect("enter town01");
    for _ in 0..30 {
        let _ = host.world.tick();
    }
    assert_eq!(
        host.world
            .field_morph_lanes(MorphOwner::Placement(TREE_SLOT)),
        Some((3u8..=6).map(|i| (i, 0x1000)).collect()),
        "P1[50] holds sub-entries 3..=6 at peak"
    );
    assert!(host.world.npc_morph_live(TREE_SLOT));
    assert!(
        host.world.take_npc_morph_dirty().contains(&TREE_SLOT),
        "the arm marks the slot for both hosts' re-stage"
    );

    // The placement's own model, resolved the way both hosts resolve it.
    let man = host
        .scene
        .as_ref()
        .and_then(|s| s.field_man_payload(&host.index).ok().flatten())
        .expect("town01 MAN");
    let mf = legaia_asset::man_section::parse(&man).expect("parse MAN");
    let placement = legaia_engine_core::man_field_scripts::classify_placements(&mf, &man)
        .into_iter()
        .map(|(p, _)| p)
        .find(|p| p.index == usize::from(TREE_SLOT))
        .expect("P1[50] placement");
    assert!(!placement.special_model);
    let tmd = &host
        .resources
        .as_ref()
        .expect("scene resources")
        .tmds
        .get(placement.model_index as usize)
        .expect("tree model")
        .tmd;
    let morphed = host
        .world
        .npc_morphed_tmd(TREE_SLOT, tmd)
        .expect("live lanes stage a copy");
    assert_eq!(morphed.objects.len(), tmd.objects.len());
    let moved: usize = tmd
        .objects
        .iter()
        .zip(&morphed.objects)
        .map(|(a, b)| {
            a.vertices
                .iter()
                .zip(&b.vertices)
                .filter(|(u, v)| (u.x, u.y, u.z) != (v.x, v.y, v.z))
                .count()
        })
        .sum();
    assert!(
        moved > 0,
        "the withered pose displaces the authored vertices"
    );

    // A slot that never armed a lane draws the authored mesh.
    assert!(host.world.npc_morphed_tmd(0, tmd).is_none());
    println!("[ran] town01 P1[50] morph staged: {moved} vertices displaced");
}
