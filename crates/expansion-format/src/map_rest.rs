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
use crate::map_detail::DecorSource;
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
    let bottom = shapes(it.mesh, it.rotation, it.scale).map_or(0.0, |sh| visual_base(&sh));
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
    /// How many placements rest badly: `1` for a prop or a decoration;
    /// for a brush's mesh or an edge run, the placements `kind` is the
    /// worst of.
    pub count: usize,
}

impl RestIssue {
    /// Whether it is a brush's ([`DecorSource::Scatter`] or
    /// [`DecorSource::Edges`]): its placements have no `on`, only a height.
    pub fn is_brush(&self) -> bool {
        self.at.starts_with("scatter[") || self.at.starts_with("edges[")
    }
}

impl fmt::Display for RestIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut who = if self.id.is_empty() {
            self.at.clone()
        } else {
            format!("{} ({:?})", self.at, self.id)
        };
        if self.count > 1 {
            who = format!("{who}, {} placements, the worst", self.count);
        } else if self.is_brush() {
            who = format!("{who}, one placement,");
        }
        let (rest, hang) = if self.is_brush() {
            ("change its height", "mark it `hangs`")
        } else {
            ("rest it with `on`", "mark it `hangs`")
        };
        match &self.kind {
            RestKind::Floating { gap } => write!(
                f,
                "{who} floats {gap:.0} units over the ground with nothing under it \
                 ({rest}, or {hang})"
            ),
            RestKind::OffTop { support, by } if *by > 0.0 => write!(
                f,
                "{who} hovers {by:.0} units over the top of {support} ({rest})"
            ),
            RestKind::OffTop { support, by } => write!(
                f,
                "{who} sinks {:.0} units into the top of {support} ({rest})",
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
    /// The item it is (`usize::MAX` for a wall box).
    item: usize,
    name: String,
    /// Its top, world height.
    top: f32,
    /// The world-space convex area of the top, counter-clockwise.
    area: Vec<[f32; 2]>,
    /// Its bounding box `[x0, z0, x1, z1]`.
    bounds: [f32; 4],
}

/// One thing the check judges: a prop, a `[[decor]]`, or one placement of
/// a `[[scatter]]` mesh or an `[[edges]]` run.
struct Judged {
    /// Field path: `props[3]`, `scatter[1].meshes[0]`.
    at: String,
    id: String,
    /// Index into the prop and decor list, `None` for a brush placement
    /// (which is never a support).
    item: Option<usize>,
    xz: [f32; 2],
    /// The origin's world height.
    y: f32,
    shape: Shape,
    hangs: bool,
}

/// The items of `def` (its `on` resolved first, [`rest_on`]) that rest on
/// nothing they stand over, or overhang what they rest on. The props and
/// decorations are judged one by one; a scatter brush's mesh or an edge
/// run is judged over every placement it makes and reported once, at its
/// worst placement, with how many rest badly ([`RestIssue::count`]).
pub fn rest_issues(def: &MapDef, shapes: &ShapeLookup<'_>) -> Vec<RestIssue> {
    let (def, _) = rest_on(def, shapes);
    let Ok(grid) = def.tile_grid() else {
        return Vec::new();
    };
    let floors = Floors::new(&def, &grid);
    let (supports, unknown, judged) = gather(&def, &grid, &floors, shapes);
    // Per field path: the worst issue and how many placements rest badly.
    let mut found: BTreeMap<String, (RestIssue, f32)> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for j in &judged {
        if j.hangs {
            continue;
        }
        let Some((kind, worst)) = judge(j, &supports, &unknown, &floors) else {
            continue;
        };
        match found.get_mut(&j.at) {
            Some((issue, w)) => {
                issue.count += 1;
                if worst > *w {
                    issue.kind = kind;
                    *w = worst;
                }
            }
            None => {
                order.push(j.at.clone());
                found.insert(
                    j.at.clone(),
                    (
                        RestIssue {
                            at: j.at.clone(),
                            id: j.id.clone(),
                            kind,
                            count: 1,
                        },
                        worst,
                    ),
                );
            }
        }
    }
    order
        .into_iter()
        .filter_map(|at| found.remove(&at).map(|(i, _)| i))
        .collect()
}

/// Where one prop or `[[decor]]` rests, for an editor: its resolved height
/// and what stands under its footprint ([`rest_report`]).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ItemRest {
    /// Field path: `props[3]`, `decor[12]`.
    pub at: String,
    pub id: String,
    /// Its origin's world height, its `on` resolved.
    pub y: f32,
    /// Where it reads as standing ([`visual_base`]), world height; `None`
    /// when its mesh is not measured.
    pub base: Option<f32>,
    /// Its top, world height: a standable top, else its mesh's highest
    /// point; `None` when neither is known.
    pub top: Option<f32>,
    /// The highest ground under its footprint, world height.
    pub ground: Option<f32>,
    /// The surfaces under its footprint, the one holding the most of it
    /// first.
    pub under: Vec<Under>,
}

/// A surface under an item ([`ItemRest::under`]).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Under {
    /// The support's field path (`props[2]`), empty for a wall box.
    pub at: String,
    /// Its id (what `on` names), empty when it has none.
    pub id: String,
    /// What the checks call it: `"table"`, `the wall box at [3, 1]`.
    pub name: String,
    /// Its top, world height.
    pub top: f32,
    /// The `offset[1]` that sets the item's base on that top with no `on`.
    pub lift: f32,
}

/// [`ItemRest`] for every prop and `[[decor]]` of `def`, in `props`, then
/// `decor` order. Without a measured mesh an item's `under` is what holds
/// its origin.
pub fn rest_report(def: &MapDef, shapes: &ShapeLookup<'_>) -> Vec<ItemRest> {
    let (resolved, _) = rest_on(def, shapes);
    let Ok(grid) = resolved.tile_grid() else {
        return Vec::new();
    };
    let floors = Floors::new(&resolved, &grid);
    let (supports, _, judged) = gather(&resolved, &grid, &floors, shapes);
    let list = items(&resolved);
    list.iter()
        .enumerate()
        .map(|(k, it)| {
            let j = judged.iter().find(|j| j.item == Some(k));
            let xz = floors.xz(it);
            let y = floors.item_floor(it) + it.offset[1];
            let floor = floors.item_floor(it);
            let bottom = j.map_or(0.0, |j| visual_base(&j.shape));
            let hull: Vec<[f32; 2]> = j.map_or_else(Vec::new, |j| {
                j.shape
                    .hull
                    .iter()
                    .map(|p| [xz[0] + p[0], xz[1] + p[1]])
                    .collect()
            });
            let flat = area(&hull) < MIN_AREA;
            let mut under: Vec<(&Support, f32)> = supports
                .iter()
                .filter(|s| s.item != k)
                .filter_map(|s| {
                    let held = if hull.is_empty() {
                        f32::from(u8::from(hull_contains(&s.area, xz)))
                    } else if flat {
                        held_points(&s.area, &hull, xz)
                    } else {
                        area(&clip(&hull, &s.area)) / area(&hull)
                    };
                    (held > 0.0).then_some((s, held))
                })
                .collect();
            under.sort_by(|a, b| {
                let r = |h: f32| (h * 100.0).round();
                r(b.1).total_cmp(&r(a.1)).then(b.0.top.total_cmp(&a.0.top))
            });
            ItemRest {
                at: it.at.clone(),
                id: it.id.to_string(),
                y,
                base: j.map(|j| j.y + visual_base(&j.shape)),
                top: match it.top {
                    Some((t, _)) => Some(f32::from(t)),
                    None => j.map(|j| j.y + j.shape.top),
                },
                ground: ground_under(&floors, &hull, xz),
                under: under
                    .into_iter()
                    .map(|(s, _)| {
                        let (at, id) = list
                            .get(s.item)
                            .map_or((String::new(), String::new()), |x| {
                                (x.at.clone(), x.id.to_string())
                            });
                        Under {
                            at,
                            id,
                            name: s.name.clone(),
                            top: s.top,
                            lift: (s.top - bottom - floor).round(),
                        }
                    })
                    .collect(),
            }
        })
        .collect()
}

/// What the checks read off a map: its supports, the items whose mesh is
/// not measured (by item and origin), and every judged item and placement.
type Gathered = (Vec<Support>, Vec<(usize, [f32; 2])>, Vec<Judged>);

fn gather(
    def: &MapDef,
    grid: &TileGrid,
    floors: &Floors<'_>,
    shapes: &ShapeLookup<'_>,
) -> Gathered {
    let list = items(def);
    let name = |it: &Item<'_>| {
        if it.id.is_empty() {
            it.at.clone()
        } else {
            format!("{:?}", it.id)
        }
    };
    let mut supports: Vec<Support> = Vec::new();
    // Items near which an unmeasured mesh may be what holds a thing up.
    let mut unknown: Vec<(usize, [f32; 2])> = Vec::new();
    let mut judged: Vec<Judged> = Vec::new();
    for (k, it) in list.iter().enumerate() {
        let xz = floors.xz(it);
        let y = floors.item_floor(it) + it.offset[1];
        let shape = shapes(it.mesh, it.rotation, it.scale);
        if let Some((top, [w, d])) = it.top {
            let (hw, hd) = (f32::from(w) / 2.0, f32::from(d) / 2.0);
            let area = rect(xz[0] - hw, xz[1] - hd, xz[0] + hw, xz[1] + hd);
            supports.push(Support {
                item: k,
                name: name(it),
                top: f32::from(top),
                bounds: bounds(&area),
                area,
            });
        } else if let Some(sh) = &shape {
            let area: Vec<[f32; 2]> = sh
                .hull
                .iter()
                .map(|p| [xz[0] + p[0], xz[1] + p[1]])
                .collect();
            supports.push(Support {
                item: k,
                name: name(it),
                top: y + sh.top,
                bounds: bounds(&area),
                area,
            });
        }
        match shape {
            Some(shape) => judged.push(Judged {
                at: it.at.clone(),
                id: it.id.to_string(),
                item: Some(k),
                xz,
                y,
                shape,
                hangs: it.hangs,
            }),
            None => unknown.push((k, xz)),
        }
    }
    // Every placement the brushes make.
    if let Ok(placed) = def.decorations_sourced() {
        for (src, d) in placed {
            let (id, hangs) = match src {
                DecorSource::Decor(_) => continue,
                DecorSource::Scatter { brush, .. } => {
                    let b = &def.scatter[brush];
                    (b.id.clone(), b.hangs)
                }
                DecorSource::Edges(e) => {
                    let r = &def.edges[e];
                    (r.id.clone(), r.hangs)
                }
            };
            if hangs {
                continue;
            }
            let Some(shape) = shapes(&d.mesh, d.rotation, d.scale) else {
                continue;
            };
            let (cx, cz) = def.tile_centre(d.tile[0], d.tile[1]);
            let xz = [cx as f32 + d.offset[0], cz as f32 + d.offset[2]];
            let floor = match &floors.surface {
                Some(_) => floors.ground(xz[0], xz[1]).unwrap_or(0.0),
                None => floors.tier(d.tile[0], d.tile[1]),
            };
            judged.push(Judged {
                at: src.path(),
                id,
                item: None,
                xz,
                y: floor + d.offset[1],
                shape,
                hangs: false,
            });
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
            let area = rect(cx - 64.0, cz - 64.0, cx + 64.0, cz + 64.0);
            supports.push(Support {
                item: usize::MAX,
                name: format!("the wall box at [{col}, {row}]"),
                top: floors.tier(col as u16, row as u16) + f32::from(*height),
                bounds: bounds(&area),
                area,
            });
        }
    }
    (supports, unknown, judged)
}

/// The highest ground under a placed footprint (`hull`, world space; its
/// origin at `xz`): a mesh that touches the floor anywhere under it (a rock
/// on a slope) stands on it. A flat card reads the ground under its origin.
fn ground_under(floors: &Floors<'_>, hull: &[[f32; 2]], xz: [f32; 2]) -> Option<f32> {
    if area(hull) < MIN_AREA {
        return floors.ground(xz[0], xz[1]);
    }
    let c = centroid(hull);
    hull.iter()
        .chain([&c, &xz])
        .filter_map(|p| floors.ground(p[0], p[1]))
        .reduce(f32::max)
}

/// What is wrong with where `j` rests, and how badly (for picking a
/// brush's worst placement); `None` when it rests well.
fn judge(
    j: &Judged,
    supports: &[Support],
    unknown: &[(usize, [f32; 2])],
    floors: &Floors<'_>,
) -> Option<(RestKind, f32)> {
    let sh = &j.shape;
    let hull: Vec<[f32; 2]> = sh
        .hull
        .iter()
        .map(|p| [j.xz[0] + p[0], j.xz[1] + p[1]])
        .collect();
    let flat = area(&hull) < MIN_AREA;
    let base = j.y + visual_base(sh);
    let ground = ground_under(floors, &hull, j.xz)?;
    let gap = base - ground;
    if gap <= REST_TOLERANCE {
        return None;
    }
    let bb = if hull.is_empty() {
        [j.xz[0], j.xz[1], j.xz[0], j.xz[1]]
    } else {
        bounds(&hull)
    };
    // The supports under the item's footprint, by how much of it they
    // hold.
    let under: Vec<(&Support, f32)> = supports
        .iter()
        .filter(|s| Some(s.item) != j.item && boxes_meet(&s.bounds, &bb))
        .filter_map(|s| {
            let held = if flat {
                held_points(&s.area, &hull, j.xz)
            } else {
                area(&clip(&hull, &s.area)) / area(&hull)
            };
            (held > 0.0).then_some((s, held))
        })
        .collect();
    // The support holding the most of it; of those holding as much, the
    // one whose top is nearest its base.
    let best = |list: &mut dyn Iterator<Item = (&Support, f32)>| -> Option<usize> {
        list.map(|(s, held)| {
            let k = supports
                .iter()
                .position(|x| std::ptr::eq(x, s))
                .unwrap_or(0);
            (k, (held * 100.0).round(), -(base - s.top).abs())
        })
        .max_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)))
        .map(|(k, _, _)| k)
    };
    let on = best(
        &mut under
            .iter()
            .copied()
            .filter(|(s, _)| (base - s.top).abs() <= REST_TOLERANCE),
    );
    if let Some(sup) = on.map(|k| &supports[k]) {
        let by = hull
            .iter()
            .map(|&p| outside(&sup.area, p))
            .fold(0.0, f32::max);
        return (by > OVERHANG_TOLERANCE).then(|| {
            (
                RestKind::Overhang {
                    support: sup.name.clone(),
                    by,
                },
                by,
            )
        });
    }
    // An unmeasured mesh near the item may be what holds it.
    if unknown
        .iter()
        .any(|(u, n)| Some(*u) != j.item && (n[0] - j.xz[0]).hypot(n[1] - j.xz[1]) < 160.0)
    {
        return None;
    }
    let raised = best(
        &mut under
            .iter()
            .copied()
            // Sunk into a support, it is that support's only when the
            // support holds most of it: a rock leaning into the next one
            // of a pile is a pile, not a mug sunk into a table.
            .filter(|(s, held)| s.top - ground > REST_TOLERANCE && (base > s.top || *held >= 0.5)),
    );
    Some(match raised.map(|k| &supports[k]) {
        Some(s) => (
            RestKind::OffTop {
                support: s.name.clone(),
                by: base - s.top,
            },
            (base - s.top).abs(),
        ),
        None => (RestKind::Floating { gap }, gap),
    })
}

/// Where a placed mesh reads as standing, above its origin. A mesh with a
/// floor area stands on its lowest point. A flat card (a torch's flame, a
/// sprite tree: a picture drawn on one upright plane, [`MIN_AREA`]) is
/// read from its origin instead: its lowest vertex says little about where
/// the picture stands (`rugi`'s torch card, object 15, reaches 64 units
/// below its origin, under the floor of any tile it is set on), so its base
/// is the origin when the card spans it, else the card's nearer end.
pub fn visual_base(sh: &Shape) -> f32 {
    if area(&sh.hull) < MIN_AREA {
        sh.bottom.max(sh.top.min(0.0))
    } else {
        sh.bottom
    }
}

/// The polygon's unsigned area.
fn area(hull: &[[f32; 2]]) -> f32 {
    if hull.len() < 3 {
        0.0
    } else {
        orientation(hull).abs() / 2.0
    }
}

/// The mean of the polygon's corners.
fn centroid(hull: &[[f32; 2]]) -> [f32; 2] {
    let n = hull.len().max(1) as f32;
    let (x, z) = hull
        .iter()
        .fold((0.0, 0.0), |(x, z), p| (x + p[0], z + p[1]));
    [x / n, z / n]
}

/// The polygon's bounding box `[x0, z0, x1, z1]`.
fn bounds(hull: &[[f32; 2]]) -> [f32; 4] {
    hull.iter().fold(
        [f32::MAX, f32::MAX, f32::MIN, f32::MIN],
        |[x0, z0, x1, z1], p| [x0.min(p[0]), z0.min(p[1]), x1.max(p[0]), z1.max(p[1])],
    )
}

fn boxes_meet(a: &[f32; 4], b: &[f32; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

/// The share of a flat item's points (its ends and its origin) the convex
/// area `hull` holds.
fn held_points(hull: &[[f32; 2]], item: &[[f32; 2]], origin: [f32; 2]) -> f32 {
    let pts: Vec<[f32; 2]> = item.iter().copied().chain([origin]).collect();
    let held = pts.iter().filter(|&&p| hull_contains(hull, p)).count();
    held as f32 / pts.len() as f32
}

/// The convex polygon `subject` clipped to the convex polygon `window`
/// (Sutherland-Hodgman; either winding).
fn clip(subject: &[[f32; 2]], window: &[[f32; 2]]) -> Vec<[f32; 2]> {
    if window.len() < 3 || subject.len() < 3 {
        return Vec::new();
    }
    let sign = if orientation(window) >= 0.0 {
        1.0
    } else {
        -1.0
    };
    let mut out = subject.to_vec();
    for i in 0..window.len() {
        let (a, b) = (window[i], window[(i + 1) % window.len()]);
        // Positive inside (on the left of a counter-clockwise edge).
        let side =
            |p: [f32; 2]| sign * ((b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]));
        let input = std::mem::take(&mut out);
        for k in 0..input.len() {
            let (p, q) = (input[k], input[(k + 1) % input.len()]);
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
        if out.is_empty() {
            break;
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
