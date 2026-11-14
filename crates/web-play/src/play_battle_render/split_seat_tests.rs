use super::*;
use legaia_engine_core::world::Actor;

fn render(actor_idx: usize) -> BattleActorRender {
    BattleActorRender {
        actor_idx,
        mesh: BattleMesh {
            mesh: legaia_tmd::mesh::VramMesh {
                positions: vec![[0.0; 3]; 3],
                uvs: vec![[0; 2]; 3],
                cba_tsb: vec![[0; 2]; 3],
                normals: vec![[0.0; 3]; 3],
                colors: vec![[0; 3]; 3],
                indices: vec![0, 1, 2],
            },
            flat: Vec::new(),
        },
        object_ids: vec![0; 3],
        rest_pose: vec![0; 6],
        morph_src: None,
    }
}

fn battle_render(actors: Vec<BattleActorRender>) -> BattleRender {
    BattleRender {
        vram: psx_tim::Vram::new(),
        backdrop: None,
        backdrop_spun: None,
        shell: None,
        ground: None,
        grid_far: None,
        outdoor: false,
        actors,
        tex_slots_used: 1,
        generation: 0,
        faces: Vec::new(),
    }
}

fn monster(id: u16) -> Actor {
    Actor {
        active: true,
        battle_monster_id: Some(id),
        ..Default::default()
    }
}

/// Issue #612: a Divide clone seated mid-fight (engine slot 2 here, the
/// caster's monster on slot 1) had no mesh on the page, so it could be
/// targeted but was never drawn. It takes the caster's bundle - the same
/// mesh out of the same texture band, as retail's clone shares its caster's
/// record and model word - once, and an unrelated monster is left alone.
#[test]
fn a_split_seat_draws_its_casters_mesh() {
    let mut br = battle_render(vec![render(1)]);
    let actors = vec![Actor::default(), monster(7), monster(7), monster(9)];
    assert_eq!(br.bind_split_seats(&actors), 1);
    assert_eq!(br.actor_slots(), vec![1, 2]);
    assert_eq!(
        br.actors[1].mesh.mesh.indices,
        br.actors[0].mesh.mesh.indices
    );
    // Bound once: the next tick finds nothing to do.
    assert_eq!(br.bind_split_seats(&actors), 0);
    assert_eq!(br.actor_slots(), vec![1, 2]);
}
