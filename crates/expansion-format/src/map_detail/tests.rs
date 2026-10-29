use super::*;
use crate::map::MapDef;

fn parse(text: &str) -> MapDef {
    toml::from_str(text).expect("map parses")
}

const HALL: &str = r########"
name = "Hall"
base = { scene = { kind = "scene", name = "tower" } }

[tiles]
rows = [
  "#######",
  "#.....#",
  "#.,,,.#",
  "#.....#",
  "#######",
]
[tiles.legend]
"#" = { walls = "all" }
"." = {}
"," = {}

[[spawns]]
id = "start"
tile = [1, 1]

[[props]]
id = "torch"
mesh = { scene = "rugi", object = 12 }
tile = [3, 1]
animate = [{ kind = "spin", period = 120 }, { kind = "bob", amplitude = 6, period = 60, phase = 0.5 }]
light = { color = [255, 160, 80], radius = 500, flicker = 0.3 }

[[decor]]
mesh = { object = 3 }
tile = [2, 2]
rotation = [0.0, 45.0, 0.0]

[[scatter]]
id = "rubble"
rect = [1, 1, 5, 3]
meshes = [{ mesh = { object = 4 }, weight = 3 }, { mesh = { asset = "models/rock.glb" } }]
density = 1.5
seed = 99
scale = [0.5, 1.0]

[[edges]]
id = "sconces"
mesh = { tmd = { kind = "tmd", prot = 61, index = 2 } }
every = 2
inset = 4
y = 64

[lighting]
mood = "cave"
lamps = [{ tile = [3, 2], color = [80, 160, 255], radius = 800 }]

[[materials]]
kind = "cycle"
clut = 32576
start = 1
count = 6
period = 8

[[materials]]
kind = "scroll"
cell = { tpage = 28, clut = 32576, tile = 2 }
dx = 1
period = 2

[[materials]]
kind = "hsv"
clut = 32592
period = 240
value = 32

[fog]
regions = [{ rect = [1, 1, 5, 3], speed = 16 }]
"########;

#[test]
fn every_detail_section_parses() {
    let m = parse(HALL);
    assert_eq!(
        m.props[0].mesh,
        PropMesh::SceneObject {
            scene: "rugi".into(),
            object: 12
        }
    );
    assert_eq!(m.props[0].animate.len(), 2);
    assert!(matches!(
        m.props[0].animate[1],
        Animator::Bob { amplitude: 6, period: 60, phase } if phase == 0.5
    ));
    assert_eq!(m.props[0].light.as_ref().unwrap().radius, 500);
    assert_eq!(
        m.scatter[0].meshes[1].mesh,
        PropMesh::Asset {
            asset: "models/rock.glb".into(),
            colors: None
        }
    );
    assert_eq!(m.scatter[0].meshes[1].weight, 1, "weight defaults to 1");
    assert_eq!(m.lighting.mood, Some(MoodPreset::Cave));
    assert_eq!(
        m.lighting.lamps[0].offset,
        [0, 96, 0],
        "lamp default height"
    );
    assert_eq!(m.materials.len(), 3);
    assert!(matches!(
        m.materials[2],
        MaterialFx::Hsv { count: 16, hue, .. } if hue == 360.0
    ));
    assert_eq!(m.fog.regions[0].speed, 16);
    // Round trip.
    let text = toml::to_string(&m).unwrap();
    assert_eq!(parse(&text), m);
}

#[test]
fn decorations_are_deterministic() {
    let m = parse(HALL);
    let a = m.decorations().unwrap();
    let b = m.decorations().unwrap();
    assert_eq!(a, b);
    // The hand-placed one leads.
    assert_eq!(a[0].tile, [2, 2]);
    assert_eq!(a[0].rotation[1], 45.0);
}

#[test]
fn scatter_follows_density_and_keeps_paths_clear() {
    let mut m = parse(HALL);
    m.decor.clear();
    m.edges.clear();
    let d = m.decorations().unwrap();
    // 15 tiles of floor in the rect; the spawn and the prop's tile stay
    // clear, leaving 13; 1.5 a tile is 13 + about 6.
    assert!((13..=26).contains(&d.len()), "{} scattered", d.len());
    assert!(d.iter().all(|x| x.tile != [1, 1] && x.tile != [3, 1]));
    for x in &d {
        assert!(x.offset[0].abs() <= 40.0 && x.offset[2].abs() <= 40.0);
        assert!((0.5..=1.0).contains(&x.scale));
        assert!((0.0..=360.0).contains(&x.rotation[1]));
    }
    // A different seed paints a different floor.
    let mut other = m.clone();
    other.scatter[0].seed = 100;
    assert_ne!(other.decorations().unwrap(), d);
    // Only the `,` tiles.
    m.scatter[0].on = ",".into();
    m.scatter[0].density = 1.0;
    let d = m.decorations().unwrap();
    assert_eq!(d.len(), 3, "one on each `,` tile");
}

#[test]
fn edges_face_into_the_room() {
    let mut m = parse(HALL);
    m.decor.clear();
    m.scatter.clear();
    m.edges[0].every = 1;
    let d = m.decorations().unwrap();
    // The 5 x 3 room has 16 wall-facing sides: 5 + 5 along the long walls,
    // 3 + 3 along the short ones.
    assert_eq!(d.len(), 16);
    let corner: Vec<_> = d.iter().filter(|x| x.tile == [1, 1]).collect();
    assert_eq!(corner.len(), 2, "a corner tile faces two walls");
    // The north copy sits against the north wall and faces +Z (yaw 0).
    let north = corner.iter().find(|x| x.offset[2] < 0.0).unwrap();
    assert_eq!(north.offset, [0.0, 64.0, -60.0]);
    assert_eq!(north.rotation[1], 0.0);
    let west = corner.iter().find(|x| x.offset[0] < 0.0).unwrap();
    assert_eq!(west.rotation[1], 90.0);
    // `every = 2` keeps the tiles whose `col + row` is even.
    m.edges[0].every = 2;
    let d = m.decorations().unwrap();
    assert!(d.iter().all(|x| (x.tile[0] + x.tile[1]) % 2 == 0));
}

#[test]
fn runaway_brushes_are_refused() {
    let mut m = parse(HALL);
    m.scatter[0].density = 40.0;
    assert!(m.decorations().is_err(), "over the per-tile cap");
    let mut m = parse(HALL);
    m.scatter[0].meshes.clear();
    assert!(m.decorations().is_err(), "no meshes");
}

#[test]
fn mesh_refs_cover_props_and_every_decoration_source() {
    let m = parse(HALL);
    assert_eq!(m.mesh_refs().len(), 1 + 1 + 2 + 1);
}

#[test]
fn the_tile_stream_is_stable() {
    // The scatter of every published map rides on this stream: a change to
    // it reshuffles every pack's floors.
    let mut r = TileRng::new(7, 0, 3, 4);
    let first: Vec<u32> = (0..4).map(|_| (r.unit() * 1000.0) as u32).collect();
    let mut again = TileRng::new(7, 0, 3, 4);
    let second: Vec<u32> = (0..4).map(|_| (again.unit() * 1000.0) as u32).collect();
    assert_eq!(first, second);
    assert_eq!(splitmix64(0), 0xE220_A839_7B1D_CDAF);
}
