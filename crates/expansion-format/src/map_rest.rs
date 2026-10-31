//! Resting placement: what a raised prop or decoration stands on.
//!
//! A prop's `offset[1]` lifts it off its tile's floor, which is how a mug
//! goes on a table or a lantern on a post - and how an item ends up hovering
//! a hand's width over the table it was meant for, or hanging off its edge.
//! Two things here keep that honest:
//!
//! - [`rest_on`] resolves `on = "<id>"` ([`crate::map::PropDef::on`]): the
//!   item's lowest point is set on its support's top (a standable prop's
//!   `top`, else the support mesh's highest point), `offset[1]` lifting it
//!   from there, so the modder never has to know the table's height.
//! - [`rest_issues`] finds the items that rest on nothing: a raised base
//!   that is neither on the floor under it nor on the top of a prop, a
//!   decoration or a wall box whose area holds it (**floating**, or **off
//!   the top** of the support it is over), and an item resting on a support
//!   it is wider than (**overhang**). Items marked `hangs` are passed over.
//!
//! Both read a mesh's placed [`Shape`] through a lookup the caller supplies,
//! because a mesh's height and where its origin sits are in its vertices:
//! the engine measures them off the disc at map load, and the validator asks
//! its [`crate::refs::DiscResolver::mesh_shape`] (no disc, no judgement; an
//! `on` naming no item or resting on itself is an error either way). An
//! item or support whose shape the lookup does not know is not judged.

use std::collections::BTreeMap;
use std::fmt;

use crate::map::{MapDef, PropMesh, TileGrid, WallMesh};
use crate::terrain::TerrainSurface;

/// How far a base may sit from the surface under it and still rest on it,
/// world units.
pub const REST_TOLERANCE: f32 = 10.0;

/// How far an item may reach past its support's edge before it overhangs,
/// world units.
pub const OVERHANG_TOLERANCE: f32 = 12.0;

/// A mesh as placed: turned and scaled, relative to its origin, `y` up.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// The floor-plane convex hull `[x, z]`, counter-clockwise.
    pub hull: Vec<[f32; 2]>,
    /// The lowest and highest point above the origin.
    pub bottom: f32,
    pub top: f32,
}

/// The placed [`Shape`] of a mesh at a rotation (degrees) and a scale.
pub type ShapeLookup<'a> = dyn Fn(&PropMesh, [f32; 3], f32) -> Option<Shape> + 'a;

/// One prop or `[[decor]]` entry, as the checks read it.
#[derive(Debug, Clone)]
struct Item<'a> {
    /// Field path: `props[3]`, `decor[12]`.
    at: String,
    id: &'a str,
    mesh: &'a PropMesh,
    tile: [u16; 2],
    offset: [f32; 3],
    rotation: [f32; 3],
    scale: f32,
    on: &'a str,
    hangs: bool,
    /// A standable prop's top and footprint.
    top: Option<(i16, [u16; 2])>,
}

fn items(def: &MapDef) -> Vec<Item<'_>> {
    let props = def.props.iter().enumerate().map(|(i, p)| Item {
        at: format!("props[{i}]"),
        id: &p.id,
        mesh: &p.mesh,
        tile: p.tile,
        offset: p.offset.map(f32::from),
        rotation: p.rotation,
        scale: p.scale.unwrap_or(1.0),
        on: &p.on,
        hangs: p.hangs,
        top: p.top.map(|t| (t, p.footprint.unwrap_or([128, 128]))),
    });
    let decor = def.decor.iter().enumerate().map(|(i, d)| Item {
        at: format!("decor[{i}]"),
        id: &d.id,
        mesh: &d.mesh,
        tile: d.tile,
        offset: d.offset.map(f32::from),
        rotation: d.rotation,
        scale: d.scale.unwrap_or(1.0),
        on: &d.on,
        hangs: d.hangs,
        top: None,
    });
    props.chain(decor).collect()
}

/// The floors the checks stand things on.
struct Floors<'a> {
    def: &'a MapDef,
    grid: &'a TileGrid,
    surface: Option<TerrainSurface>,
    ladder: [i16; crate::map::FLOOR_TIERS],
}

impl<'a> Floors<'a> {
    fn new(def: &'a MapDef, grid: &'a TileGrid) -> Self {
        Self {
            def,
            grid,
            surface: def.terrain_surface().ok().flatten(),
            ladder: def.floor_ladder().unwrap_or_default(),
        }
    }

    fn tier(&self, col: u16, row: u16) -> f32 {
        self.grid
            .get(usize::from(col), usize::from(row))
            .map_or(0.0, |t| f32::from(self.ladder[usize::from(t.tier)]))
    }

    /// The world `(x, z)` an item's origin stands at.
    fn xz(&self, it: &Item<'_>) -> [f32; 2] {
        let (cx, cz) = self.def.tile_centre(it.tile[0], it.tile[1]);
        [cx as f32 + it.offset[0], cz as f32 + it.offset[2]]
    }

    /// The floor an item's `offset[1]` is measured from, as the field
    /// image's builder places it: its own tile's tier, or the terrain under
    /// its origin.
    fn item_floor(&self, it: &Item<'_>) -> f32 {
        match &self.surface {
            Some(s) => {
                let [x, z] = self.xz(it);
                s.field
                    .sample_world(x.round() as i32, z.round() as i32)
                    .unwrap_or(0) as f32
            }
            None => self.tier(it.tile[0], it.tile[1]),
        }
    }

    /// The ground under world `(x, z)`.
    fn ground(&self, x: f32, z: f32) -> Option<f32> {
        let (xi, zi) = (x.round() as i32, z.round() as i32);
        match &self.surface {
            Some(s) => s.field.sample_world(xi, zi).map(|h| h as f32),
            None => {
                let (c, r) = self.def.tile_at_world(self.grid, xi, zi)?;
                Some(self.tier(c, r))
            }
        }
    }
}

/// Resolve every `on` of `def`: each item naming a support has its
/// `offset[1]` rewritten so its lowest point sits on the support's top
/// (plus the `offset[1]` it had), and its `on` cleared, so the result places
/// as any map does and resolving it again changes nothing. Returns the map
/// and what could not be resolved (an unknown id, a cycle, a support whose
/// top the lookup cannot measure); those items keep their offset.
pub fn rest_on(def: &MapDef, shapes: &ShapeLookup<'_>) -> (MapDef, Vec<OnProblem>) {
    let mut out = def.clone();
    let mut problems = Vec::new();
    if !def.props.iter().any(|p| !p.on.is_empty()) && !def.decor.iter().any(|d| !d.on.is_empty()) {
        return (out, problems);
    }
    let Ok(grid) = def.tile_grid() else {
        return (out, problems);
    };
    let floors = Floors::new(def, &grid);
    let list = items(def);
    let by_id: BTreeMap<&str, usize> = list
        .iter()
        .enumerate()
        .filter(|(_, it)| !it.id.is_empty())
        .map(|(k, it)| (it.id, k))
        .collect();
    // The resolved origin height of each item, once known.
    let mut origin: Vec<Option<f32>> = vec![None; list.len()];
    for k in 0..list.len() {
        let mut chain = Vec::new();
        resolve(
            k,
            &list,
            &by_id,
            &floors,
            shapes,
            &mut origin,
            &mut chain,
            &mut problems,
        );
    }
    let n_props = def.props.len();
    for (k, it) in list.iter().enumerate() {
        if it.on.is_empty() {
            continue;
        }
        let Some(y) = origin[k] else {
            continue;
        };
        let lift = (y - floors.item_floor(it)).round().clamp(-32768.0, 32767.0) as i16;
        if k < n_props {
            out.props[k].offset[1] = lift;
            out.props[k].on.clear();
        } else {
            out.decor[k - n_props].offset[1] = lift;
            out.decor[k - n_props].on.clear();
        }
    }
    (out, problems)
}

/// An `on` that could not be resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct OnProblem {
    /// Field path: `props[3].on`.
    pub field: String,
    pub message: String,
    /// The reference itself is broken (no such id, a cycle), not merely a
    /// support the lookup cannot measure.
    pub broken: bool,
}

/// The world height of item `k`'s origin, resolving its support first.
#[allow(clippy::too_many_arguments)]
fn resolve(
    k: usize,
    list: &[Item<'_>],
    by_id: &BTreeMap<&str, usize>,
    floors: &Floors<'_>,
    shapes: &ShapeLookup<'_>,
    origin: &mut [Option<f32>],
    chain: &mut Vec<usize>,
    problems: &mut Vec<OnProblem>,
) -> Option<f32> {
    if let Some(y) = origin[k] {
        return Some(y);
    }
    let it = &list[k];
    let plain = floors.item_floor(it) + it.offset[1];
    if it.on.is_empty() {
        origin[k] = Some(plain);
        return origin[k];
    }
    let field = format!("{}.on", it.at);
    let Some(&s) = by_id.get(it.on) else {
        problems.push(OnProblem {
            field,
            message: format!("the map has no prop or decoration {:?}", it.on),
            broken: true,
        });
        origin[k] = Some(plain);
        return origin[k];
    };
    if s == k || chain.contains(&s) {
        problems.push(OnProblem {
            field,
            message: format!("{:?} rests on itself through `on`", it.on),
            broken: true,
        });
        origin[k] = Some(plain);
        return origin[k];
    }
    chain.push(k);
    let support_origin = resolve(s, list, by_id, floors, shapes, origin, chain, problems);
    chain.pop();
    let sup = &list[s];
    let top = match sup.top {
        Some((t, _)) => Some(f32::from(t)),
        None => support_origin
            .and_then(|y| shapes(sup.mesh, sup.rotation, sup.scale).map(|sh| y + sh.top)),
    };
    let Some(top) = top else {
        problems.push(OnProblem {
            field,
            message: format!(
                "the height of {:?} is unknown, so nothing rests on it",
                it.on
            ),
            broken: false,
        });
        origin[k] = Some(plain);
        return origin[k];
    };
    let bottom = shapes(it.mesh, it.rotation, it.scale).map_or(0.0, |sh| sh.bottom);
    origin[k] = Some(top - bottom + it.offset[1]);
    origin[k]
}

/// What is wrong with where an item rests.
#[derive(Debug, Clone, PartialEq)]
pub enum RestKind {
    /// Raised `gap` units over the ground with nothing under it.
    Floating { gap: f32 },
    /// Over `support` but `by` units off its top (positive above it,
    /// negative sunk into it).
    OffTop { support: String, by: f32 },
    /// Resting on `support` and reaching `by` units past its edge.
    Overhang { support: String, by: f32 },
}

/// One item that rests badly.
#[derive(Debug, Clone, PartialEq)]
pub struct RestIssue {
    /// Field path of the item: `props[3]`, `decor[12]`.
    pub at: String,
    /// Its id, empty when it has none.
    pub id: String,
    pub kind: RestKind,
}

impl fmt::Display for RestIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let who = if self.id.is_empty() {
            self.at.clone()
        } else {
            format!("{} ({:?})", self.at, self.id)
        };
        match &self.kind {
            RestKind::Floating { gap } => write!(
                f,
                "{who} floats {gap:.0} units over the ground with nothing under it \
                 (rest it with `on`, or mark it `hangs`)"
            ),
            RestKind::OffTop { support, by } if *by > 0.0 => write!(
                f,
                "{who} hovers {by:.0} units over the top of {support} (rest it with `on`)"
            ),
            RestKind::OffTop { support, by } => write!(
                f,
                "{who} sinks {:.0} units into the top of {support} (rest it with `on`)",
                -by
            ),
            RestKind::Overhang { support, by } => write!(
                f,
                "{who} reaches {by:.0} units past the edge of {support}, the thing it rests on"
            ),
        }
    }
}

/// A surface something can rest on.
struct Support {
    name: String,
    /// Its top, world height.
    top: f32,
    /// The world-space convex area of the top, counter-clockwise.
    area: Vec<[f32; 2]>,
}

/// A support near an item whose shape is not known.
struct Unknown {
    xz: [f32; 2],
}

/// The items of `def` (its `on` resolved first, [`rest_on`]) that rest on
/// nothing they stand over, or overhang what they rest on.
pub fn rest_issues(def: &MapDef, shapes: &ShapeLookup<'_>) -> Vec<RestIssue> {
    let (def, _) = rest_on(def, shapes);
    let Ok(grid) = def.tile_grid() else {
        return Vec::new();
    };
    let floors = Floors::new(&def, &grid);
    let list = items(&def);
    let placed: Vec<([f32; 2], f32, Option<Shape>)> = list
        .iter()
        .map(|it| {
            (
                floors.xz(it),
                floors.item_floor(it) + it.offset[1],
                shapes(it.mesh, it.rotation, it.scale),
            )
        })
        .collect();
    let name = |it: &Item<'_>| {
        if it.id.is_empty() {
            it.at.clone()
        } else {
            format!("{:?}", it.id)
        }
    };
    let mut supports: Vec<(usize, Support)> = Vec::new();
    let mut unknown: Vec<(usize, Unknown)> = Vec::new();
    for (k, it) in list.iter().enumerate() {
        let (xz, y, shape) = &placed[k];
        if let Some((top, [w, d])) = it.top {
            let (hw, hd) = (f32::from(w) / 2.0, f32::from(d) / 2.0);
            supports.push((
                k,
                Support {
                    name: name(it),
                    top: f32::from(top),
                    area: rect(xz[0] - hw, xz[1] - hd, xz[0] + hw, xz[1] + hd),
                },
            ));
            continue;
        }
        match shape {
            Some(sh) => supports.push((
                k,
                Support {
                    name: name(it),
                    top: y + sh.top,
                    area: sh
                        .hull
                        .iter()
                        .map(|p| [xz[0] + p[0], xz[1] + p[1]])
                        .collect(),
                },
            )),
            None => unknown.push((k, Unknown { xz: *xz })),
        }
    }
    // A wall box closing its whole tile is a surface too: a counter, a
    // ledge.
    for row in 0..grid.height {
        for col in 0..grid.width {
            let (Some(t), Some(ch)) = (grid.get(col, row), grid.char_at(col, row)) else {
                continue;
            };
            if t.void || t.walls != 0xF {
                continue;
            }
            let Some(WallMesh::Box { height, .. }) = def.style(ch).and_then(|s| s.wall.as_ref())
            else {
                continue;
            };
            let (cx, cz) = def.tile_centre(col as u16, row as u16);
            let (cx, cz) = (cx as f32, cz as f32);
            supports.push((
                usize::MAX,
                Support {
                    name: format!("the wall box at [{col}, {row}]"),
                    top: floors.tier(col as u16, row as u16) + f32::from(*height),
                    area: rect(cx - 64.0, cz - 64.0, cx + 64.0, cz + 64.0),
                },
            ));
        }
    }
    let mut out = Vec::new();
    for (k, it) in list.iter().enumerate() {
        if it.hangs {
            continue;
        }
        // An item the lookup cannot measure is not judged: where its
        // lowest point is depends on its vertices.
        let (xz, y, Some(sh)) = &placed[k] else {
            continue;
        };
        let base = y + sh.bottom;
        let Some(ground) = floors.ground(xz[0], xz[1]) else {
            continue;
        };
        let gap = base - ground;
        if gap <= REST_TOLERANCE {
            continue;
        }
        let under: Vec<&Support> = supports
            .iter()
            .filter(|(s, sup)| *s != k && hull_contains(&sup.area, *xz))
            .map(|(_, sup)| sup)
            .collect();
        let issue = |kind| RestIssue {
            at: it.at.clone(),
            id: it.id.to_string(),
            kind,
        };
        match under
            .iter()
            .filter(|s| (base - s.top).abs() <= REST_TOLERANCE)
            .min_by(|a, b| (base - a.top).abs().total_cmp(&(base - b.top).abs()))
        {
            Some(sup) => {
                let by = sh
                    .hull
                    .iter()
                    .map(|p| outside(&sup.area, [xz[0] + p[0], xz[1] + p[1]]))
                    .fold(0.0, f32::max);
                if by > OVERHANG_TOLERANCE {
                    out.push(issue(RestKind::Overhang {
                        support: sup.name.clone(),
                        by,
                    }));
                }
            }
            None => {
                // An unmeasured mesh near the item may be what holds it.
                if unknown
                    .iter()
                    .any(|(u, n)| *u != k && (n.xz[0] - xz[0]).hypot(n.xz[1] - xz[1]) < 160.0)
                {
                    continue;
                }
                let nearest = under
                    .iter()
                    .min_by(|a, b| (base - a.top).abs().total_cmp(&(base - b.top).abs()));
                out.push(issue(match nearest {
                    Some(s) if s.top - ground > REST_TOLERANCE => RestKind::OffTop {
                        support: s.name.clone(),
                        by: base - s.top,
                    },
                    _ => RestKind::Floating { gap },
                }));
            }
        }
    }
    out
}

fn rect(x0: f32, z0: f32, x1: f32, z1: f32) -> Vec<[f32; 2]> {
    vec![[x0, z0], [x1, z0], [x1, z1], [x0, z1]]
}

/// The polygon's signed orientation: positive counter-clockwise in `(x, z)`.
fn orientation(hull: &[[f32; 2]]) -> f32 {
    (0..hull.len())
        .map(|i| {
            let (a, b) = (hull[i], hull[(i + 1) % hull.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

/// The least area a surface can hold something on: a flat card (a sprite
/// tree, a torch drawn on one plane) has none.
const MIN_AREA: f32 = 16.0;

/// Is `p` inside the convex polygon `hull` (either winding)? Never for a
/// polygon of no area.
pub fn hull_contains(hull: &[[f32; 2]], p: [f32; 2]) -> bool {
    orientation(hull).abs() / 2.0 >= MIN_AREA && outside(hull, p) <= 0.0
}

/// How far `p` lies outside the convex polygon `hull` (either winding):
/// `0` inside, else the largest distance past an edge's line.
fn outside(hull: &[[f32; 2]], p: [f32; 2]) -> f32 {
    if hull.len() < 3 {
        return f32::MAX;
    }
    let sign = if orientation(hull) >= 0.0 { 1.0 } else { -1.0 };
    (0..hull.len())
        .map(|i| {
            let (a, b) = (hull[i], hull[(i + 1) % hull.len()]);
            let (ex, ez) = (b[0] - a[0], b[1] - a[1]);
            let len = ex.hypot(ez);
            if len == 0.0 {
                return 0.0;
            }
            // Positive on the outer side of the edge.
            -sign * (ex * (p[1] - a[1]) - ez * (p[0] - a[0])) / len
        })
        .fold(0.0, f32::max)
}

/// The validator's disc-free half: an `on` that names no item, or that
/// rests an item on itself, is an error. The resting warnings need the
/// meshes' shapes, and run once a resolver supplies them.
pub(crate) fn check_rest(def: &MapDef, cx: &mut crate::validate::Checker<'_>) {
    let (_, problems) = rest_on(def, &|_, _, _| None);
    for p in problems.into_iter().filter(|p| p.broken) {
        cx.error(&p.field, p.message);
    }
}

#[cfg(test)]
mod tests;
