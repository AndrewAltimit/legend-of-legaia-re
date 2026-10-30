//! Disc-gated: on every map of the flagship pack, what draws as a building
//! blocks the player.
//!
//! For each prop the test measures the drawn mesh's footprint (the lower
//! half of its hull, `expansion_map::footprint`) off the disc, then enters
//! the map and asks the live world's wall probe (`World::field_tile_is_wall`,
//! the `FUN_801d56c4` port) about the ground under it:
//!
//! - a prop more than a tile and a half across is `solid` (or standable) -
//!   a house nobody marked is the walk-through-a-house bug;
//! - a solid prop's core (its hull shrunk toward its centre) reads as wall,
//!   so the collision covers the mesh and not only the prop's own tile;
//! - the player, seated outside a solid prop on each side and walked at it
//!   with the pad, stops outside that core.
//!
//! Skips when `LEGAIA_DISC_BIN` is unset.

use std::sync::Arc;

use legaia_engine_scene::expansion_map::detail::resolve_mesh_library;
use legaia_engine_scene::expansion_map::footprint::{footprint, hull_contains, hull_extent};
use legaia_engine_scene::expansion_map::tile_centre_world;
use legaia_engine_scene::scene::Scene;
use legaia_engine_scene_host::scene_host::{SceneHost, SceneTickEvent};
use legaia_engine_system::input::PadButton;
use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::map::PropMesh;

fn rebirth_pack() -> ExpansionPack {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
    let files = legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read");
    ExpansionPack::from_files(&files).expect("the pack parses")
}

/// A prop as the test measures it.
struct Measured {
    map: String,
    id: String,
    solid: bool,
    /// Stands in the player's way (`Footprint::is_large_obstacle`).
    large: bool,
    /// World-space hull.
    hull: Vec<[f32; 2]>,
    centre: [f32; 2],
}

fn measure(host: &SceneHost, pack: &ExpansionPack) -> Vec<Measured> {
    let mut out = Vec::new();
    for (id, def) in &pack.maps {
        let base_name = def.base.scene_name().expect("base scene");
        let base = Scene::load(&host.index, base_name).expect("base scene loads");
        let geometry = legaia_engine_scene::scene_resources::field_geometry_pack(&base);
        let lib = resolve_mesh_library(
            &host.index,
            def,
            &pack.assets,
            &base,
            geometry.map(|(e, _)| e),
        )
        .unwrap_or_else(|e| panic!("{id}: {e:#}"));
        for p in def.props.iter().filter(|p| p.top.is_none()) {
            let raw = lib.raw(&p.mesh).or_else(|| match p.mesh {
                PropMesh::Object { object } => {
                    lib.base_raw.get(usize::from(object)).map(|v| &v[..])
                }
                _ => None,
            });
            let Some(f) = raw.and_then(|r| footprint(r, p.rotation, p.scale.unwrap_or(1.0))) else {
                continue;
            };
            let large = f.is_large_obstacle(f32::from(p.offset[1]));
            let h = f.hull;
            let (x, z) = tile_centre_world(def, p.tile[0], p.tile[1]);
            let (x, z) = (
                f32::from(x) + f32::from(p.offset[0]),
                f32::from(z) + f32::from(p.offset[2]),
            );
            let hull: Vec<[f32; 2]> = h.iter().map(|q| [q[0] + x, q[1] + z]).collect();
            let n = hull.len() as f32;
            let centre = hull
                .iter()
                .fold([0.0, 0.0], |a, q| [a[0] + q[0] / n, a[1] + q[1] / n]);
            out.push(Measured {
                map: id.clone(),
                id: p.id.clone(),
                solid: p.solid,
                large,
                hull,
                centre,
            });
        }
    }
    out
}

/// How far inside a solid mesh's base hull the collision must hold.
const INSET: f32 = 40.0;

/// Is `p` inside the counter-clockwise convex `hull`, at least `d` from
/// every edge?
fn inset_inside(hull: &[[f32; 2]], p: [f32; 2], d: f32) -> bool {
    (0..hull.len()).all(|i| {
        let (a, b) = (hull[i], hull[(i + 1) % hull.len()]);
        let (ex, ez) = (b[0] - a[0], b[1] - a[1]);
        let len = ex.hypot(ez);
        len == 0.0 || (ex * (p[1] - a[1]) - ez * (p[0] - a[0])) / len >= d
    })
}

/// The hull shrunk toward its centre by `k`.
fn shrink(m: &Measured, k: f32) -> Vec<[f32; 2]> {
    m.hull
        .iter()
        .map(|q| {
            [
                m.centre[0] + (q[0] - m.centre[0]) * k,
                m.centre[1] + (q[1] - m.centre[1]) * k,
            ]
        })
        .collect()
}

#[test]
fn every_building_on_the_rebirth_maps_blocks_the_player() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let pack = rebirth_pack();
    let mut host = SceneHost::open_disc(disc).expect("open the disc");
    host.activate_expansion_pack(Arc::new(pack.clone()))
        .expect("the Rebirth pack activates");
    let measured = measure(&host, &pack);
    assert!(!measured.is_empty());

    // Large props are solid. The Rebirth Isles maps are held to it; the
    // rest are reported.
    let open: Vec<(bool, String)> = measured
        .iter()
        .filter(|m| !m.solid && m.large)
        .map(|m| {
            (
                m.map.starts_with("isles_"),
                format!("{}:{} {:?}", m.map, m.id, hull_extent(&m.hull)),
            )
        })
        .collect();
    for (_, o) in open.iter().filter(|(held, _)| !held) {
        eprintln!("[note] a large prop closes no ground: {o}");
    }
    let open: Vec<&String> = open
        .iter()
        .filter(|(held, _)| *held)
        .map(|(_, o)| o)
        .collect();

    let mut leaks = Vec::new();
    let mut walked_in = Vec::new();
    // Walks that closed in on their prop: the pad directions are right.
    let mut approached = 0;
    for map in pack.maps.keys() {
        let props: Vec<&Measured> = measured
            .iter()
            .filter(|m| &m.map == map && m.solid)
            .collect();
        if props.is_empty() {
            continue;
        }
        host.enter_expansion_map(map, "")
            .unwrap_or_else(|e| panic!("{map}: {e:#}"));
        for m in &props {
            // The core reads as wall: a 16-unit lattice over the hull, each
            // point at least `INSET` inside every edge (the wall probe leads
            // the player by 47 units, and a sub-cell closes on its centre).
            let core = shrink(m, 0.6);
            let (mut n, mut wall) = (0, 0);
            let (x0, x1, z0, z1) = m.hull.iter().fold(
                (f32::MAX, f32::MIN, f32::MAX, f32::MIN),
                |(a, b, c, d), q| (a.min(q[0]), b.max(q[0]), c.min(q[1]), d.max(q[1])),
            );
            let mut z = z0;
            while z <= z1 {
                let mut x = x0;
                while x <= x1 {
                    if inset_inside(&m.hull, [x, z], INSET) {
                        n += 1;
                        if host.world.field_tile_is_wall(x as i16, z as i16) {
                            wall += 1;
                        }
                    }
                    x += 16.0;
                }
                z += 16.0;
            }
            if n > 0 && wall * 10 < n * 9 {
                leaks.push(format!("{map}:{} {wall}/{n} core points walled", m.id));
            }
            // Walk at it from each side.
            let reach = hull_extent(&m.hull).iter().cloned().fold(0.0, f32::max) / 2.0 + 140.0;
            for (btn, dx, dz) in [
                (PadButton::Up, 0.0, 1.0),
                (PadButton::Down, 0.0, -1.0),
                (PadButton::Left, 1.0, 0.0),
                (PadButton::Right, -1.0, 0.0),
            ] {
                let sx = m.centre[0] + dx * reach;
                let sz = m.centre[1] + dz * reach;
                if host.world.field_tile_is_wall(sx as i16, sz as i16) {
                    continue; // no ground to start from on this side
                }
                host.world.actors[0].move_state.world_x = sx as i16;
                host.world.actors[0].move_state.world_z = sz as i16;
                for _ in 0..90 {
                    host.world.set_pad(btn.mask());
                    match host.tick().expect("tick") {
                        SceneTickEvent::Stepped => {}
                        _ => break,
                    }
                }
                host.world.set_pad(0);
                if host.active_expansion_map() != Some(map.as_str()) {
                    host.enter_expansion_map(map, "").expect("re-enter");
                    continue;
                }
                let ms = &host.world.actors[0].move_state;
                let at = [f32::from(ms.world_x), f32::from(ms.world_z)];
                let d0 = (sx - m.centre[0]).hypot(sz - m.centre[1]);
                let d1 = (at[0] - m.centre[0]).hypot(at[1] - m.centre[1]);
                if d1 + 16.0 < d0 {
                    approached += 1;
                }
                if hull_contains(&core, at) {
                    walked_in.push(format!("{map}:{} from {btn:?} to {at:?}", m.id));
                }
            }
        }
    }
    assert!(approached > 0, "no walk approached its prop");
    eprintln!("{approached} walks approached their props");
    assert!(
        open.is_empty() && leaks.is_empty() && walked_in.is_empty(),
        "large props the player walks through: {open:#?}\n\
         solid props whose mesh is open ground: {leaks:#?}\n\
         the player walked into: {walked_in:#?}"
    );
}
