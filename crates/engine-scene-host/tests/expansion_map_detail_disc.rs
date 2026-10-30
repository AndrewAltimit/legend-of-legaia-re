//! Disc-gated: a pack map's detail layer entered as a live field scene over
//! the Sky Gardens Tower (`tower`).
//!
//! The fixture map names meshes from two other scenes (the Rogue's Tower's
//! torch, Rim Elm's barrel) and a glTF model the fixture carries (the
//! Rebirth pack's crystal, read from the repo), scatters a base-pack mesh,
//! animates one prop and places a lamp. Entering it relocates the foreign
//! textures into free VRAM without touching a word the base scene wrote,
//! bakes the decorations into cell meshes, and drives the animated prop's
//! draw through the world's turn and move tables. The Rebirth floors this
//! lane dresses (1F, 2F) enter too. Skips when `LEGAIA_DISC_BIN` is unset.

use std::sync::Arc;

use legaia_engine_scene::expansion_map::PROP_RECORD_BASE;
use legaia_engine_scene::expansion_map::detail::field_resources;
use legaia_engine_scene::scene::Scene;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_expansion_format::validate::{has_errors, validate};
use legaia_expansion_format::{ExpansionPack, PackFiles};

const MAP_ID: &str = "detail_hall";

const PACK: &str = r#"
schema = 1
id = "detail_fixture"
name = "Detail fixture"
version = "0.1.0"
description = "Test fixture."
authors = ["test"]
license = "CC0-1.0"
"#;

const MAP: &str = r##########"
name = "Detail Hall"
base = { scene = { kind = "scene", name = "tower" } }
floor_heights = [0]

[tiles]
origin = [20, 20]
rows = [
  "#########",
  "#.......#",
  "#.......#",
  "#.......#",
  "#.......#",
  "#########",
]
[tiles.legend]
"#" = { walls = "all", wall = { kind = "mesh", index = 10 } }
"." = { ground = { kind = "texture", tpage = 28, clut = 32576, tile = 0 } }

[[spawns]]
id = "start"
tile = [4, 3]

[[props]]
id = "crystal"
mesh = { asset = "models/ember_crystal.glb" }
tile = [2, 2]
offset = [0, 60, 0]
animate = [{ kind = "spin", period = 120 }, { kind = "bob", amplitude = 10, period = 60 }]
light = { color = [255, 150, 70], radius = 500 }

[[props]]
id = "barrel"
mesh = { scene = "town01", object = 34 }
tile = [6, 2]

[[edges]]
mesh = { scene = "rugi", object = 15 }
walls = "#"
every = 2
y = 100

[[scatter]]
rect = [1, 1, 7, 4]
density = 0.6
seed = 7
scale = [0.4, 0.6]
meshes = [{ mesh = { object = 27 } }]

[lighting]
mood = "cave"
lamps = [{ tile = [4, 1], color = [100, 150, 255], radius = 600 }]

[[materials]]
kind = "scroll"
cell = { tpage = 28, clut = 32576, tile = 0 }
dx = 1
period = 4
"##########;

fn host() -> Option<SceneHost> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    };
    Some(SceneHost::open_disc(disc).expect("open the disc"))
}

fn crystal() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../packs/rebirth/assets/models/ember_crystal.glb"
    );
    std::fs::read(path).expect("the Rebirth crystal model")
}

fn fixture() -> ExpansionPack {
    let mut files = PackFiles::default();
    files.insert("pack.toml".into(), PACK.as_bytes().to_vec());
    files.insert(format!("maps/{MAP_ID}.toml"), MAP.as_bytes().to_vec());
    files.insert("assets/models/ember_crystal.glb".into(), crystal());
    let pack = ExpansionPack::from_files(&files).expect("the fixture parses");
    let issues = validate(&pack);
    assert!(!has_errors(&issues), "{issues:#?}");
    pack
}

#[test]
fn foreign_meshes_relocate_without_touching_the_base_scenes_vram() {
    let Some(mut host) = host() else { return };
    host.activate_expansion_pack(Arc::new(fixture()))
        .expect("activates");
    host.enter_expansion_map(MAP_ID, "").expect("enters");
    let scene = host.scene.as_ref().expect("a scene");
    let o = scene.overrides.clone().expect("the map's override");
    assert!(
        !o.vram_patches.is_empty(),
        "the torch, barrel and model bring texture copies"
    );
    // Baked cells, the carried-in barrel and the model join the pack.
    assert!(
        o.added_tmds.len() >= 4,
        "{} added meshes",
        o.added_tmds.len()
    );
    let with = field_resources(&host.index, scene).expect("the map's resources");
    let base = Scene::load(&host.index, "tower").expect("tower");
    let without = field_resources(&host.index, &base).expect("tower's resources");
    let mut kept = 0usize;
    for y in 0..512 {
        for x in 0..1024 {
            if without.vram.is_written(x, y) {
                assert_eq!(
                    with.vram.pixel(x, y),
                    without.vram.pixel(x, y),
                    "base texel ({x}, {y}) changed"
                );
                kept += 1;
            }
        }
    }
    assert!(kept > 10_000, "the base scene writes its VRAM ({kept})");
    // Every patch is in the map's VRAM.
    for p in &o.vram_patches {
        for (i, &w) in p.words.iter().enumerate() {
            let (x, y) = (
                p.x as usize + i % p.w as usize,
                p.y as usize + i / p.w as usize,
            );
            assert_eq!(with.vram.pixel(x, y), w, "patch word ({x}, {y})");
        }
    }
    // The added meshes are in the scene's TMD pool under the geometry entry.
    let entry = o.geometry_entry.expect("a geometry entry");
    let pooled = with.tmds.iter().filter(|t| t.entry_idx == entry).count();
    assert!(pooled >= o.added_tmds.len());
    eprintln!(
        "[{MAP_ID}] {} VRAM patches, {} added meshes, {kept} base texels unchanged",
        o.vram_patches.len(),
        o.added_tmds.len()
    );
}

#[test]
fn an_animated_prop_turns_and_moves_over_frames() {
    let Some(mut host) = host() else { return };
    host.activate_expansion_pack(Arc::new(fixture()))
        .expect("activates");
    host.enter_expansion_map(MAP_ID, "").expect("enters");
    let key = PROP_RECORD_BASE;
    let o = host.scene.as_ref().unwrap().overrides.clone().unwrap();
    let mut vram = field_resources(&host.index, host.scene.as_ref().unwrap())
        .unwrap()
        .vram;
    assert_eq!(o.prop_records.len(), 1, "only the crystal animates");
    assert_eq!(o.prop_records[0].1, key);
    let sample = |host: &SceneHost| {
        let turns = host.world.object_draw_turn_matrices();
        let moves = host.world.object_draw_displacements();
        (turns.get(&key).copied(), moves.get(&key).copied())
    };
    let (t0, _) = sample(&host);
    assert!(t0.is_some(), "the crystal's turn is in the hosts' table");
    for _ in 0..15 {
        host.tick().expect("tick");
    }
    let (t1, m1) = sample(&host);
    assert_ne!(t0, t1, "the spin moved on");
    assert!(m1.is_some_and(|m| m[1] != 0), "the bob lifts it: {m1:?}");
    // The lamp and the prop's light reach the lighting kernel.
    assert_eq!(host.world.expansion_map_lights().len(), 2);
    assert!(host.world.expansion_map_mood().is_some());
    // The scroll material rewrites the dais cell between frames.
    let before = vram.clone();
    host.world.step_expansion_materials(&mut vram);
    for _ in 0..8 {
        host.tick().expect("tick");
    }
    assert!(host.world.step_expansion_materials(&mut vram));
    assert_ne!(vram.as_u16(), before.as_u16());
}

#[test]
fn the_dressed_rebirth_floors_enter() {
    let Some(mut host) = host() else { return };
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
    let pack = ExpansionPack::from_dir(std::path::Path::new(dir)).expect("Rebirth loads");
    let issues = validate(&pack);
    assert!(!has_errors(&issues), "{issues:#?}");
    host.activate_expansion_pack(Arc::new(pack))
        .expect("activates");
    for map in ["rebirth_tower_1f", "rebirth_tower_2f"] {
        host.enter_expansion_map(map, "")
            .unwrap_or_else(|e| panic!("{map}: {e:#}"));
        let o = host.scene.as_ref().unwrap().overrides.clone().unwrap();
        eprintln!(
            "[{map}] {} added meshes, {} VRAM patches, {} animated props",
            o.added_tmds.len(),
            o.vram_patches.len(),
            o.prop_records.len()
        );
        assert!(!o.added_tmds.is_empty());
        for _ in 0..30 {
            host.tick().expect("tick");
        }
        if map == "rebirth_tower_2f" {
            // The Ember Stair's torches: the violet ramp turns amber once
            // and then holds.
            let mut vram = field_resources(&host.index, host.scene.as_ref().unwrap())
                .unwrap()
                .vram;
            let row = |v: &legaia_tim::Vram| (0..16).map(|x| v.pixel(x, 506)).collect::<Vec<_>>();
            let before = row(&vram);
            let rgb = |w: u16| (w & 31, (w >> 5) & 31, (w >> 10) & 31);
            let (r, _, b) = rgb(before[14]);
            assert!(b > r, "the retail ramp is violet: {:?}", rgb(before[14]));
            assert!(host.world.step_expansion_materials(&mut vram));
            let after = row(&vram);
            let (r, _, b) = rgb(after[14]);
            assert!(r > b, "the tinted ramp is warm: {:?}", rgb(after[14]));
            assert!(
                !host.world.step_expansion_materials(&mut vram),
                "a tint holds"
            );
        }
    }
}

/// The first textured primitive's CLUT word of `scene`'s geometry-pack mesh
/// `n`.
fn mesh_clut(host: &SceneHost, scene: &str, n: usize) -> u16 {
    let s = Scene::load(&host.index, scene).unwrap();
    let res = field_resources(&host.index, &s).unwrap();
    let env = legaia_engine_scene::field_env::env_pack_tmd_indices(&s, &res);
    let r = &res.tmds[env[n]];
    for o in &r.tmd.objects {
        for g in legaia_tmd::legaia_prims::iter_groups(
            &r.raw,
            o.primitives_byte_offset,
            o.primitives_byte_size,
        )
        .unwrap()
        {
            if let Some(p) = g.prims.iter().find(|p| !p.uvs.is_empty()) {
                return p.cba;
            }
        }
    }
    panic!("no textured prim");
}

#[test]
fn round_two_layers_resolve_on_the_disc() {
    let Some(mut host) = host() else { return };
    let torch_clut = mesh_clut(&host, "rugi", 15);
    let extra = format!(
        r#"
[[decals]]
cell = {{ tpage = 28, clut = 32577, tile = 4 }}
rect = [1, 1, 7, 4]
density = 0.5
seed = 1
sides = 8

[[emitters]]
kind = "embers"
rect = [2, 2, 6, 3]
count = 20

[[materials]]
kind = "cycle"
from = "rugi"
clut = {torch_clut}
start = 1
count = 6
period = 4

[[npcs]]
id = "scenery_actor"
model = {{ kind = "npc", scene = "tower", index = 3 }}
tile = [6, 1]
scenery = true
"#
    );
    let mut files = PackFiles::default();
    files.insert("pack.toml".into(), PACK.as_bytes().to_vec());
    files.insert(
        format!("maps/{MAP_ID}.toml"),
        format!("{MAP}{extra}").into_bytes(),
    );
    files.insert("assets/models/ember_crystal.glb".into(), crystal());
    let pack = ExpansionPack::from_files(&files).expect("parses");
    let issues = validate(&pack);
    assert!(!has_errors(&issues), "{issues:#?}");
    host.activate_expansion_pack(Arc::new(pack))
        .expect("activates");
    host.enter_expansion_map(MAP_ID, "").expect("enters");
    let map = host.world.expansion.map.as_ref().unwrap();
    // The particle sprites have a home.
    assert!(map.particle_sheet.is_some());
    // The torch's palette resolved to the copy the load made.
    let resolved = match map.def.materials.last().unwrap() {
        legaia_expansion_format::map_detail::MaterialFx::Cycle { clut, .. } => *clut,
        _ => unreachable!(),
    };
    assert_ne!(resolved, torch_clut, "the cycle follows the copied palette");
    // The scenery actor draws (it is in the override's placements) but
    // takes no anchor: nothing collides with it and the talk probe skips it.
    let o = host.scene.as_ref().unwrap().overrides.clone().unwrap();
    let slot = legaia_expansion_format::map::npc_slot(0).unwrap();
    assert!(o.npcs.iter().any(|p| p.index == usize::from(slot)));
    assert!(!host.world.npcs.positions.contains_key(&slot));
    assert!(
        host.world
            .expansion
            .map
            .as_ref()
            .unwrap()
            .talk(slot)
            .is_none()
    );
}

#[test]
fn a_kept_room_seats_its_animated_scenery() {
    let Some(mut host) = host() else { return };
    // A kept Rogue's Tower hall (the Rebirth floors are laid out from
    // scratch, so the fixture keeps the retail room itself).
    let map = r#"
name = "Kept Hall"
base = { scene = { kind = "scene", name = "rugi" }, geometry = "keep", scenery = true }

[tiles]
origin = [22, 41]
rows = [
  ".....",
  ".....",
  ".....",
]
[tiles.legend]
"." = { label = "floor" }

[[spawns]]
id = "start"
tile = [2, 1]
"#;
    let mut files = PackFiles::default();
    files.insert("pack.toml".into(), PACK.as_bytes().to_vec());
    files.insert("maps/kept_hall.toml".into(), map.as_bytes().to_vec());
    let pack = ExpansionPack::from_files(&files).expect("parses");
    host.activate_expansion_pack(Arc::new(pack))
        .expect("activates");
    host.enter_expansion_map("kept_hall", "")
        .expect("the kept hall enters");
    let o = host.scene.as_ref().unwrap().overrides.clone().unwrap();
    // rugi's animated scene props, after the map's own NPCs, each with a
    // clip and none anchored.
    let n_npcs = host.world.expansion.map.as_ref().unwrap().def.npcs.len();
    let scenery: Vec<_> = o.npcs.iter().filter(|p| p.index > n_npcs).collect();
    assert!(!scenery.is_empty(), "rugi has animated props");
    for p in scenery {
        assert_ne!(p.anim_id, 0);
        assert!(!host.world.npcs.positions.contains_key(&(p.index as u8)));
    }
}

/// Every Rebirth map leaves its base scene's texels as the base scene wrote
/// them: the foreign meshes, models, decals and the particle sheet all land
/// in VRAM the base left free. Pins the 4F Verdant Vault's pink-veined green
/// stone (`concnow`'s own 4bpp camouflage texture, CLUT row 509) as the
/// retail texel, not a relocation collision.
#[test]
fn no_rebirth_map_overwrites_its_base_scenes_texels() {
    let Some(mut host) = host() else { return };
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
    let pack = ExpansionPack::from_dir(std::path::Path::new(dir)).expect("Rebirth loads");
    let maps: Vec<(String, String)> = pack
        .maps
        .iter()
        .filter_map(|(id, m)| m.base.scene_name().map(|s| (id.clone(), s.to_string())))
        .collect();
    host.activate_expansion_pack(Arc::new(pack))
        .expect("activates");
    assert!(maps.len() >= 8, "{} maps", maps.len());
    for (map, base) in maps {
        host.enter_expansion_map(&map, "")
            .unwrap_or_else(|e| panic!("{map}: {e:#}"));
        let scene = host.scene.as_ref().unwrap();
        let with = field_resources(&host.index, scene).expect("the map's resources");
        let b = Scene::load(&host.index, &base).expect("the base scene");
        let without = field_resources(&host.index, &b).expect("the base's resources");
        let mut kept = 0usize;
        for y in 0..512 {
            for x in 0..1024 {
                if without.vram.is_written(x, y) {
                    assert_eq!(
                        with.vram.pixel(x, y),
                        without.vram.pixel(x, y),
                        "{map}: base texel ({x}, {y}) of {base} changed"
                    );
                    kept += 1;
                }
            }
        }
        eprintln!("[{map}] {kept} texels of {base} unchanged");
        assert!(kept > 10_000, "{map}: {kept} base texels");
    }
}
