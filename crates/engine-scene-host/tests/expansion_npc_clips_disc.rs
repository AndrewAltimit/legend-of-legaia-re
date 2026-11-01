//! Disc-gated: a pack NPC borrowed from a retail placement rests and walks
//! in the clips the placement's own motion stream names
//! (`expansion_map::fill_borrowed_npc_clips`), and a patrolling NPC turns
//! back to its rest clip when it stands at a stop.
//!
//! Rim Elm (`town01`) is the evidence: placement 37 (the old man) is placed
//! on clip 12, which his stream's default-move record `[11, 12]` names as
//! his walk - he rests in 11. Placement 13's record is `[15, 17]`. Skips
//! when `LEGAIA_DISC_BIN` is unset.

use std::sync::Arc;

use legaia_engine_scene::expansion_map::fill_borrowed_npc_clips;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::map::{MapDef, NpcDef};
use legaia_expansion_format::refs::DiscRef;

fn npc(index: u16, patrol: bool) -> NpcDef {
    NpcDef {
        id: format!("n{index}"),
        model: Some(DiscRef::Npc {
            scene: "town01".into(),
            index,
        }),
        tile: [2, 2],
        patrol: if patrol { vec![[4, 2]] } else { Vec::new() },
        ..Default::default()
    }
}

#[test]
fn borrowed_npcs_take_their_streams_rest_and_walk_clips() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let host = SceneHost::open_disc(disc).expect("open the disc");
    let mut def = MapDef {
        npcs: vec![npc(37, false), npc(13, true), npc(13, true)],
        ..Default::default()
    };
    // The pack's own choice wins.
    def.npcs[2].clip = Some(14);
    def.npcs[2].walk_clip = Some(16);
    fill_borrowed_npc_clips(&host.index, &mut def).expect("fill");
    let clips: Vec<(Option<u8>, Option<u8>)> =
        def.npcs.iter().map(|n| (n.clip, n.walk_clip)).collect();
    assert_eq!(
        clips,
        vec![(Some(11), None), (Some(15), Some(17)), (Some(14), Some(16))]
    );
}

#[test]
fn a_patrolling_village_npc_walks_in_its_walk_clip_and_rests_in_its_idle() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
    let files = legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read");
    let pack = ExpansionPack::from_files(&files).expect("the pack parses");
    let def = pack.maps["isles_village"].clone();
    let (n, walker) = def
        .npcs
        .iter()
        .enumerate()
        .find(|(_, d)| !d.patrol.is_empty() && d.pause > 0 && !d.scenery)
        .expect("the village has a patrolling NPC that stops");
    let slot = legaia_expansion_format::map::npc_slot(n).unwrap();
    let mut host = SceneHost::open_disc(disc).expect("open the disc");
    host.activate_expansion_pack(Arc::new(pack.clone()))
        .expect("the pack activates");
    host.enter_expansion_map("isles_village", "")
        .expect("enter the village");
    let live = host.world.expansion.map.as_ref().unwrap().def.npcs[n].clone();
    let (idle, walk) = (
        live.clip.expect("an idle clip"),
        live.walk_clip.expect("a walk clip"),
    );
    assert_ne!(idle, walk, "{}", walker.id);
    let (mut walked, mut rested) = (false, false);
    for _ in 0..2000 {
        host.tick().expect("tick");
        let moving = host.world.npcs.motions.contains_key(&slot);
        let clip = host
            .world
            .npcs
            .anim_cues
            .get(&slot)
            .map(|c| c.1)
            .or_else(|| host.world.npcs.clip_current.get(&slot).copied());
        match (moving, clip) {
            (true, Some(c)) if c == walk => walked = true,
            (false, Some(c)) if c == idle && walked => rested = true,
            _ => {}
        }
        if walked && rested {
            break;
        }
    }
    assert!(walked, "{} never walked in clip {walk}", walker.id);
    assert!(
        rested,
        "{} never rested in clip {idle} after a leg",
        walker.id
    );
}
