//! Disc-gated: a pack map's NPC, standable prop and visible wall, entered
//! as a live field scene over Rim Elm (`town01`).
//!
//! The pack is a fixture built here (no Rebirth content): one map whose NPC
//! borrows the model of a Rim Elm talk placement, a wall run drawn with a
//! generated box, and a standable crate. The NPC spawns as a field NPC,
//! resolves a non-empty mesh through the live-scene catalog both hosts
//! draw from, blocks the walking player, and talking to it runs its pack
//! line and raises the quest talk event. Skips when `LEGAIA_DISC_BIN` is
//! unset.

use std::sync::Arc;

use legaia_engine_scene::expansion_map::tile_centre_world;
use legaia_engine_scene::field_env::env_pack_tmd_indices;
use legaia_engine_scene::npc_catalog::{catalog_live_scene_npcs, scene_npc_picks};
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;
use legaia_expansion_format::map::{MapDef, npc_slot};
use legaia_expansion_format::validate::{has_errors, validate};
use legaia_expansion_format::{ExpansionPack, PackFiles};

const MAP_ID: &str = "npc_hall";

const PACK: &str = r#"
schema = 1
id = "npc_fixture"
name = "NPC fixture"
version = "0.1.0"
description = "Test fixture."
authors = ["test"]
license = "CC0-1.0"
"#;

const DIALOG: &str = r#"
[lines.elder_hello]
text = "Welcome to the hall."
"#;

const QUESTS: &str = r#"
[[quests]]
id = "meet_elder"
title = "Meet the elder"
auto_start = true
objectives = [{ kind = "talk", npc = "hall_elder" }]
"#;

/// The map: an open 9 x 7 hall walled by generated boxes, the elder two
/// tiles north of the spawn, a crate to stand on east of the spawn.
fn map_toml(placement: u16) -> String {
    format!(
        r##########"
name = "NPC Hall"
base = {{ scene = {{ kind = "scene", name = "town01" }} }}
floor_heights = [0]

[tiles]
origin = [20, 20]
rows = [
  "#########",
  "#.......#",
  "#.......#",
  "#.......#",
  "#.......#",
  "#.......#",
  "#########",
]
[tiles.legend]
"#" = {{ walls = "all", wall = {{ kind = "box", height = 160, tpage = 28, clut = 32576, tile = 2 }} }}
"." = {{}}

[[spawns]]
id = "start"
tile = [4, 4]
facing = 4

[[props]]
id = "crate"
mesh = {{ object = 0 }}
tile = [6, 4]
top = 64

[[npcs]]
id = "hall_elder"
model = {{ kind = "npc", scene = "town01", index = {placement} }}
tile = [4, 2]
facing = 0
line = "elder_hello"
"##########
    )
}

fn host() -> Option<SceneHost> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    };
    Some(SceneHost::open_disc(disc).expect("open the disc"))
}

/// A Rim Elm talk placement with a scene mesh.
fn talk_placement(host: &SceneHost) -> u16 {
    scene_npc_picks(&host.index, "town01")
        .expect("town01 placements")
        .into_iter()
        .find(|p| p.kind == "talk" && !p.special && p.anim_id != 0)
        .expect("a talk NPC")
        .index
}

fn activate(host: &mut SceneHost) -> MapDef {
    let placement = talk_placement(host);
    eprintln!("[{MAP_ID}] the elder wears town01 placement {placement}");
    let mut files = PackFiles::default();
    files.insert("pack.toml".into(), PACK.as_bytes().to_vec());
    files.insert("dialog.toml".into(), DIALOG.as_bytes().to_vec());
    files.insert("quests.toml".into(), QUESTS.as_bytes().to_vec());
    files.insert(
        format!("maps/{MAP_ID}.toml"),
        map_toml(placement).into_bytes(),
    );
    let pack = ExpansionPack::from_files(&files).expect("the fixture parses");
    let issues = validate(&pack);
    assert!(!has_errors(&issues), "{issues:#?}");
    let def = pack.maps[MAP_ID].clone();
    host.activate_expansion_pack(Arc::new(pack))
        .expect("the fixture activates");
    def
}

fn player_xz(host: &SceneHost) -> (i16, i16) {
    let ms = &host.world.actors[0].move_state;
    (ms.world_x, ms.world_z)
}

fn hold(host: &mut SceneHost, pad: u16, frames: usize) {
    for _ in 0..frames {
        host.world.set_pad(pad);
        host.tick().expect("tick");
    }
}

#[test]
fn a_pack_npc_spawns_draws_blocks_and_talks() {
    let Some(mut host) = host() else {
        return;
    };
    let def = activate(&mut host);
    host.world.npcs.solid = true;
    host.enter_expansion_map(MAP_ID, "start")
        .expect("enter the pack map");
    let slot = npc_slot(0).unwrap();

    // Spawned: a field NPC on its tile, and nothing of town01's own.
    let at = tile_centre_world(&def, 4, 2);
    assert_eq!(host.world.npcs.positions.get(&slot), Some(&at));
    assert_eq!(host.world.npcs.positions.len(), 1, "only the pack NPC");

    // Drawn: the live-scene catalog both hosts build their NPC draws from
    // lists it with a mesh that has triangles, and the world gives it a
    // visible draw pose.
    let scene = host.scene.as_ref().unwrap();
    let res = host.resources.as_ref().unwrap();
    let cat = catalog_live_scene_npcs(&host.index, scene, res, Some(&host.world.field_head_pool))
        .expect("catalog");
    assert_eq!(cat.entries.len(), 1);
    let e = &cat.entries[0];
    assert_eq!(e.placement.index, usize::from(slot));
    let t = &res.tmds[usize::from(e.placement.model_index)];
    let mesh = legaia_tmd::mesh::tmd_to_vram_mesh(&t.tmd, &t.raw);
    assert!(!mesh.indices.is_empty(), "the NPC mesh draws triangles");
    assert!(
        host.world.field_npc_draw_pose(slot, at).is_some(),
        "the NPC has a draw pose"
    );

    // Blocks: walking toward the NPC (-Z; which pad direction that is
    // depends on the camera) stops short of its tile.
    let z0 = player_xz(&host).1;
    hold(&mut host, PadButton::Up.mask(), 4);
    let toward = if player_xz(&host).1 < z0 {
        PadButton::Up
    } else {
        PadButton::Down
    };
    hold(&mut host, toward.mask(), 120);
    let (_, z) = player_xz(&host);
    eprintln!(
        "[{MAP_ID}] walked toward the npc to z = {z}, npc at z = {}",
        at.1
    );
    assert!(
        (i32::from(at.1) + 40..i32::from(at.1) + 128).contains(&i32::from(z)),
        "the NPC's body stopped the walk: player z {z}, npc z {}",
        at.1
    );

    // Talks: Cross facing the NPC runs its pack line and counts as the talk.
    hold(&mut host, 0, 2);
    host.world.set_pad(PadButton::Cross.mask());
    host.tick().expect("tick");
    hold(&mut host, 0, 2);
    assert_eq!(
        host.world
            .expansion
            .quests
            .dialog
            .pending
            .as_ref()
            .map(|p| p.line.as_str()),
        Some("elder_hello"),
        "the pack line runs"
    );
    let q = host.world.expansion.quests.quest("meet_elder");
    assert!(
        q.is_some_and(|s| s.completions > 0),
        "the talk objective completed: {q:?}"
    );
}

#[test]
fn walls_draw_generated_boxes_and_a_crate_is_a_floor_top() {
    let Some(mut host) = host() else {
        return;
    };
    let def = activate(&mut host);
    host.enter_expansion_map(MAP_ID, "start")
        .expect("enter the pack map");
    let scene = host.scene.as_ref().unwrap();
    let res = host.resources.as_ref().unwrap();
    let o = scene.overrides.as_ref().unwrap();
    assert_eq!(o.wall_tmds.len(), 1, "one distinct wall box");

    // The box joins the geometry pack after its own meshes, and the wall
    // cells' decoration records name it.
    let env = env_pack_tmd_indices(scene, res);
    let base_len = env.len() - 1;
    let wall = &res.tmds[*env.last().unwrap()];
    assert!(
        !legaia_tmd::mesh::tmd_to_vram_mesh(&wall.tmd, &wall.raw)
            .indices
            .is_empty()
    );
    let tiles = scene
        .field_terrain_tiles(&host.index)
        .unwrap()
        .expect("decoration cells");
    let walls = tiles
        .iter()
        .filter(|p| p.pack_index == Some(base_len as u16))
        .count();
    assert_eq!(walls, 9 * 2 + 5 * 2, "every wall tile draws the box");

    // The crate's tile samples its top; the retail walk is refused at its
    // edge.
    let (cx, cz) = tile_centre_world(&def, 6, 4);
    assert_eq!(
        host.world
            .sample_field_floor_height(i32::from(cx), i32::from(cz)),
        -64
    );
    let x0 = player_xz(&host).0;
    hold(&mut host, PadButton::Right.mask(), 4);
    let toward = if player_xz(&host).0 > x0 {
        PadButton::Right
    } else {
        PadButton::Left
    };
    hold(&mut host, toward.mask(), 120);
    let (x, _) = player_xz(&host);
    let edge = i32::from(cx) - 64;
    eprintln!("[{MAP_ID}] walked toward the crate to x = {x}, its edge at {edge}");
    assert!(
        (edge - 64..edge).contains(&i32::from(x)),
        "the walk stops at the crate: x = {x}, crate edge {edge}"
    );

    // Action movement stands on it: a hop onto the crate lands on its top,
    // 64 units up, inside its box.
    host.world.toggles.action_movement = true;
    let mut landed = None;
    for f in 0..90 {
        let jump = if f < 4 { PadButton::Square.mask() } else { 0 };
        // Steer until over the crate's middle, then drop onto it.
        let over = i32::from(host.world.actors[0].move_state.world_x) >= edge + 48;
        let steer = if over { 0 } else { toward.mask() };
        host.world.set_pad(steer | jump);
        host.tick().expect("tick");
        let ms = &host.world.actors[0].move_state;
        if std::env::var_os("NPC_TRACE").is_some() {
            eprintln!(
                "f{f} x{} y{} z{} drove {} body {:?}",
                ms.world_x,
                ms.world_y,
                ms.world_z,
                host.world.action_movement_drove(),
                host.world.action_movement_body().map(|b| b.grounded)
            );
        }
        if ms.world_y == -64
            && host
                .world
                .action_movement_body()
                .is_some_and(|b| b.grounded)
        {
            landed = Some((ms.world_x, ms.world_z));
            break;
        }
    }
    eprintln!("[{MAP_ID}] action movement landed at {landed:?}");
    let (lx, lz) = landed.expect("the jump lands on the crate's top");
    assert!(
        (edge..edge + 128).contains(&i32::from(lx)),
        "on the crate: x = {lx}"
    );
    assert!((i32::from(cz) - 64..i32::from(cz) + 64).contains(&i32::from(lz)));
}
