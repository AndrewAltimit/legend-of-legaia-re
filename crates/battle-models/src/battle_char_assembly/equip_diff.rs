//! Equipment **diff highlight** - a presentation heuristic, not a retail
//! behaviour and not a claim about the data.
//!
//! Retail has no separable item mesh. Every equipment section splices whole
//! **skeleton bone objects** into the merged battle TMD, so equipping a
//! Survival Knife does not attach a knife: it replaces Vahn's right-hand
//! object with a re-authored one that happens to include a blade (40 -> 71
//! vertices, 52 -> 80 primitives). A full loadout re-sculpts most of the
//! body. There is nothing to extract, and this module must never be read as
//! extracting one.
//!
//! What it does instead is classify the *equipped* object's primitives
//! against the **bare** (`id = 0` default) object's own **radius envelope** -
//! centroid of the bare vertices plus the distance to the furthest of them.
//! A primitive whose corners all lie outside that envelope is what the
//! equipment adds *beyond the reach of the bare part*; one with corners on
//! both sides straddles the boundary and is genuinely shared between hand and
//! weapon.
//!
//! The envelope is the only test that works here. A positional set-difference
//! does not: an equipped hand shares as few as **one** vertex position with
//! the bare hand, because the section is re-authored rather than extended, so
//! a set-difference calls essentially everything "added" and says nothing.
//!
//! The boundary this draws is approximate by construction. It is a viewing
//! aid for "what did my gear change", not a separation of item from body.

use legaia_tmd::{Tmd, legaia_prims};

use super::assembly::AssembledCharacter;

/// Per-vertex class: geometry the bare and equipped parts have in common
/// (including the straddling boundary primitives).
pub const CLASS_SHARED: u8 = 0;
/// Per-vertex class: equipped geometry outside the bare part's envelope.
pub const CLASS_ADDED: u8 = 1;
/// Per-vertex class: geometry from the **bare** part of an object the
/// equipment replaced - present unequipped, absent equipped.
pub const CLASS_BARE_ONLY: u8 = 2;

/// Slack on the envelope radius, so vertices sitting exactly on the bare
/// part's outer shell read as shared rather than added.
const RADIUS_SLACK: f32 = 1.02;

/// A bone object's radius envelope: the centroid of its vertices plus the
/// distance to the furthest one.
#[derive(Debug, Clone, Copy)]
pub struct Envelope {
    /// Vertex centroid, object-local.
    pub center: [f32; 3],
    /// Distance from `center` to the furthest vertex.
    pub radius: f32,
}

impl Envelope {
    /// The envelope of `points` (an empty set gives a zero-radius envelope at
    /// the origin, which classifies everything as outside).
    pub fn of(points: &[[f32; 3]]) -> Self {
        if points.is_empty() {
            return Envelope {
                center: [0.0; 3],
                radius: 0.0,
            };
        }
        let n = points.len() as f32;
        let mut center = [0.0f32; 3];
        for p in points {
            for k in 0..3 {
                center[k] += p[k];
            }
        }
        for c in &mut center {
            *c /= n;
        }
        let radius = points
            .iter()
            .map(|p| dist(*p, center))
            .fold(0.0f32, f32::max);
        Envelope { center, radius }
    }

    /// Whether `p` lies beyond the envelope (with [`RADIUS_SLACK`]).
    pub fn outside(&self, p: [f32; 3]) -> bool {
        dist(p, self.center) > self.radius * RADIUS_SLACK
    }
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// One bone object's vertices in object-local space.
pub fn object_points(tmd: &Tmd, obj: usize) -> Vec<[f32; 3]> {
    tmd.objects
        .get(obj)
        .map(|o| {
            o.vertices
                .iter()
                .map(|v| [f32::from(v.x), f32::from(v.y), f32::from(v.z)])
                .collect()
        })
        .unwrap_or_default()
}

/// How one bone object differs between the bare and equipped assemblies.
#[derive(Debug, Clone)]
pub struct ObjectDiff {
    /// The skeleton bone tag both objects carry.
    pub bone_tag: u8,
    /// Object index in the equipped assembly's TMD.
    pub equipped_object: usize,
    /// Object index in the bare assembly's TMD.
    pub bare_object: usize,
    /// Vertex count of the bare object.
    pub bare_vertices: usize,
    /// Vertex count of the equipped object.
    pub equipped_vertices: usize,
    /// Walkable primitive count of the bare object.
    pub bare_primitives: usize,
    /// Walkable primitive count of the equipped object.
    pub equipped_primitives: usize,
    /// Equipped primitives whose every corner lies outside the bare
    /// envelope.
    pub added_primitives: usize,
    /// Equipped primitives with corners on both sides of the bare envelope -
    /// the shared boundary between the bare part and what equipment added.
    pub straddling_primitives: usize,
    /// Vertex positions the two objects have in common. Near zero for a
    /// weapon: the section is re-authored, not extended.
    pub shared_vertex_positions: usize,
}

impl ObjectDiff {
    /// Whether the equipment changed this object's geometry at all.
    pub fn changed(&self) -> bool {
        self.bare_vertices != self.equipped_vertices
            || self.bare_primitives != self.equipped_primitives
            || self.shared_vertex_positions < self.bare_vertices
    }
}

/// Classify every skeleton bone object of `equipped` against its `bare`
/// counterpart (matched by bone tag). Equipment-extra objects (tags `100+` /
/// `200+`) are not skeleton bones and are left out.
pub fn diff_objects(
    bare: &AssembledCharacter,
    bare_tmd: &Tmd,
    equipped: &AssembledCharacter,
    equipped_tmd: &Tmd,
) -> Vec<ObjectDiff> {
    let mut out = Vec::new();
    for (ei, &tag) in equipped.bone_tags.iter().enumerate() {
        if tag >= 100 {
            continue;
        }
        let Some(bi) = bare.bone_tags.iter().position(|&t| t == tag) else {
            continue;
        };
        let bare_pts = object_points(bare_tmd, bi);
        let eq_pts = object_points(equipped_tmd, ei);
        let env = Envelope::of(&bare_pts);
        let mut added = 0usize;
        let mut straddling = 0usize;
        let mut total = 0usize;
        for prim in prim_corner_indices(equipped_tmd, &equipped.tmd, ei) {
            total += 1;
            let outside = prim
                .iter()
                .filter(|&&vi| eq_pts.get(vi).is_some_and(|p| env.outside(*p)))
                .count();
            if outside == prim.len() {
                added += 1;
            } else if outside > 0 {
                straddling += 1;
            }
        }
        let shared_vertex_positions = eq_pts
            .iter()
            .filter(|p| bare_pts.iter().any(|b| b == *p))
            .count();
        out.push(ObjectDiff {
            bone_tag: tag,
            equipped_object: ei,
            bare_object: bi,
            bare_vertices: bare_pts.len(),
            equipped_vertices: eq_pts.len(),
            bare_primitives: prim_corner_indices(bare_tmd, &bare.tmd, bi).count(),
            equipped_primitives: total,
            added_primitives: added,
            straddling_primitives: straddling,
            shared_vertex_positions,
        });
    }
    out
}

/// Corner vertex indices of every walkable primitive of object `obj`.
fn prim_corner_indices<'a>(
    tmd: &'a Tmd,
    blob: &'a [u8],
    obj: usize,
) -> impl Iterator<Item = Vec<usize>> + 'a {
    let (off, size) = tmd
        .objects
        .get(obj)
        .map(|o| (o.primitives_byte_offset, o.primitives_byte_size))
        .unwrap_or((0, 0));
    legaia_prims::iter_groups_lenient(blob, off, size)
        .into_iter()
        .flat_map(|g| {
            g.prims
                .iter()
                .map(|p| p.vertex_indices().iter().map(|&i| i as usize).collect())
                .collect::<Vec<Vec<usize>>>()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_of_a_point_cloud_is_centroid_plus_reach() {
        let e = Envelope::of(&[[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
        assert_eq!(e.center, [0.0, 0.0, 0.0]);
        assert!((e.radius - 1.0).abs() < 1e-6);
        // Slack keeps the shell itself inside.
        assert!(!e.outside([1.0, 0.0, 0.0]));
        assert!(e.outside([2.0, 0.0, 0.0]));
    }

    #[test]
    fn an_empty_object_classifies_everything_as_outside() {
        let e = Envelope::of(&[]);
        assert!(e.outside([1.0, 0.0, 0.0]));
        assert!(!e.outside([0.0, 0.0, 0.0]));
    }
}
