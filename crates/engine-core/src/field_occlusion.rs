//! Player-visibility gate for the camera-occlusion fade (the see-through
//! walls enhancement): a CPU ray-cast of eye-to-player segments against the
//! assembled field scene's **static** geometry, in retail Y-down world
//! coordinates.
//!
//! The fade's per-fragment rule alone cannot know whether the character is
//! actually hidden - geometry that merely sits *near* the eye-to-player
//! corridor (an upper-tier floor beside a tower pit, a wall the player walks
//! past in the open) satisfies "nearer than the player, inside the circle"
//! while the character is plainly visible. This gate answers the global
//! question: the fade arms only when **every** sample point on the
//! character's body (head, centre, hips, both shoulders) is blocked by scene
//! geometry - "not completely occluded" means no fade at all.
//!
//! Per-TRIANGLE on purpose. The browser play page's abandoned occluder cull
//! tested the same segment against per-placement AABBs, and whole-tile boxes
//! made neighbouring bodies read as occluders (see
//! `engine-render::occlusion_fade` and `docs/subsystems/renderer.md`). Here
//! the AABB is only a broad phase; the verdict is Möller-Trumbore against
//! the instanced triangles - the geometry the renderers actually draw.
//!
//! Scope: terrain tiles + static placements + their untextured colour halves
//! (the same both-halves union `coplanar_draws::draw_plane_summaries` walks).
//! Posed/animated props (`EnvDraw::anim_id != 0`), NPCs and spawned actor
//! meshes are not occluders - a windmill blade or a bystander briefly
//! crossing the corridor should not pop the fade on.
//!
//! Both hosts run this one kernel: the native play-window directly (built at
//! `upload_assets`, queried per field frame with the analytically-derived
//! follow-camera eye), the browser play page through the wasm runtime's
//! `field_player_occluded` export (the page passes its orbit-camera eye,
//! converted to retail Y-down).

use crate::field_env::EnvDraw;
use crate::scene_resources::SceneResources;

/// Vertical half-extent of the sample cross (world units): head/hip samples
/// sit this far above/below the body centre. The character mesh is ~130
/// units tall; sampling slightly inside the extremes keeps a grazing floor
/// edge or low kerb from counting the whole character as visible.
pub const SAMPLE_HALF_HEIGHT: f32 = 55.0;

/// Lateral half-extent of the sample cross (world units), applied
/// perpendicular to the eye ray in the ground plane - the shoulder samples.
pub const SAMPLE_HALF_WIDTH: f32 = 25.0;

/// Segment-parameter trim: intersections within this fraction of either
/// endpoint are ignored, so the surface the player stands flush against
/// (or a near-plane sliver at the eye) cannot self-occlude the test.
const SEG_T_MIN: f32 = 0.002;
const SEG_T_MAX: f32 = 0.998;

/// Minimum opaque-texel fraction (over the prim's UV bbox - see
/// `Vram::prim_opaque_fraction`) for a textured prim to count as an
/// occluder. Below it the prim is a cutout the shaders discard per texel:
/// visually see-through, so it must not arm the gate.
pub const OCCLUDER_MIN_OPACITY: f32 = 0.55;

/// One instanced draw's triangle range + world AABB (the broad phase).
struct DrawRange {
    lo: [f32; 3],
    hi: [f32; 3],
    start: usize,
    end: usize,
    /// Source mesh identity (diagnostics - see [`FieldOccluders::first_hit`]).
    res_tmd: usize,
}

/// World-space static-occluder set for one assembled field scene.
#[derive(Default)]
pub struct FieldOccluders {
    /// Instanced triangles, retail Y-down world space.
    tris: Vec<[[f32; 3]; 3]>,
    draws: Vec<DrawRange>,
}

impl FieldOccluders {
    /// Build from the scene's resolved static draw lists (terrain +
    /// placements) and the parsed TMD pool. Per distinct `res_tmd`, the
    /// textured and untextured halves are concatenated into one triangle
    /// stream (the both-halves union `coplanar_draws` walks), then
    /// instanced per draw with retail's `T * Ry` (unit scale - no
    /// placement scales; see `field_env`). Animated draws are skipped.
    ///
    /// Both halves are filtered past the raw triangle walk, because "in
    /// the draw list" is not "hides the character":
    ///
    /// * **semi-transparent (ABE) prims are not occluders** - a blended
    ///   surface is see-through by construction (the keikoku canyon's mist
    ///   veils span the whole corridor and armed the gate while the
    ///   character was plainly on screen). Textured half: TSB bit 15
    ///   ([`legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT`]); colour half:
    ///   the same bit on the per-vertex blend word.
    /// * the VRAM-coverage filter (`Vram::prim_has_texture_data`, the same
    ///   predicate the renderers' mesh builds use) - a prim whose texture
    ///   data never loaded is never drawn, so it must never occlude;
    /// * an **opacity floor** (`Vram::prim_opaque_fraction` >=
    ///   [`OCCLUDER_MIN_OPACITY`]) - a prim that draws but whose texels are
    ///   mostly the transparent `0x0000` word is a cutout the shaders
    ///   `discard` per texel (foliage, grates, tile skirts); the character
    ///   reads straight through it on screen, so it must not arm the gate.
    pub fn build(draw_lists: &[&[EnvDraw]], res: &SceneResources) -> Self {
        use std::collections::HashMap;
        let mut cache: HashMap<usize, (Vec<[f32; 3]>, Vec<u32>)> = HashMap::new();
        let mut out = Self::default();
        for draws in draw_lists {
            for d in *draws {
                if d.anim_id != 0 {
                    continue;
                }
                let (positions, indices) = match cache.entry(d.res_tmd) {
                    std::collections::hash_map::Entry::Occupied(e) => {
                        let (p, i) = e.into_mut();
                        (&*p, &*i)
                    }
                    std::collections::hash_map::Entry::Vacant(e) => {
                        let Some(rt) = res.tmds.get(d.res_tmd) else {
                            continue;
                        };
                        let vram = &res.vram;
                        let semi = legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT;
                        let mesh = legaia_tmd::mesh::tmd_to_vram_mesh_filtered(
                            &rt.tmd,
                            &rt.raw,
                            |cba, tsb, uvs| {
                                vram.prim_has_texture_data(cba, tsb, uvs)
                                    && vram.prim_opaque_fraction(cba, tsb, uvs)
                                        >= OCCLUDER_MIN_OPACITY
                            },
                        );
                        let mut positions = mesh.positions;
                        let mut indices = Vec::with_capacity(mesh.indices.len());
                        // ABE gate on the BUILT stream: the filter closure
                        // above receives the prim's raw TSB, but the ABE
                        // enable is a mode-byte bit the builder packs into
                        // the per-vertex attribute afterwards - so the
                        // semi-transparency exclusion must read the packed
                        // cba_tsb, not the closure argument.
                        for tri in mesh.indices.chunks_exact(3) {
                            let abe = mesh
                                .cba_tsb
                                .get(tri[0] as usize)
                                .is_some_and(|ct| ct[1] & semi != 0);
                            if !abe {
                                indices.extend_from_slice(tri);
                            }
                        }
                        let cmesh = legaia_tmd::mesh::tmd_to_color_mesh(&rt.tmd, &rt.raw);
                        let base = positions.len() as u32;
                        positions.extend_from_slice(&cmesh.positions);
                        // Colour half: same ABE gate on the blend word (all
                        // corners of a prim share one word).
                        for tri in cmesh.indices.chunks_exact(3) {
                            let abe = cmesh
                                .blend
                                .get(tri[0] as usize)
                                .is_some_and(|w| w & semi != 0);
                            if !abe {
                                indices.extend(tri.iter().map(|i| i + base));
                            }
                        }
                        let (p, i) = e.insert((positions, indices));
                        (&*p, &*i)
                    }
                };
                out.add_instanced_mesh(positions, indices, d);
            }
        }
        out
    }

    /// Instance one mesh's triangles under an [`EnvDraw`] transform and add
    /// them to the occluder set. Public so tests (and future hosts holding
    /// raw geometry) can compose a set without a [`SceneResources`].
    pub fn add_instanced_mesh(&mut self, positions: &[[f32; 3]], indices: &[u32], d: &EnvDraw) {
        // Retail pure-Y rotation (FUN_80026988): local +Z -> (sin, 0, cos).
        let ang = f32::from(d.rot_y & 0x0FFF) * (std::f32::consts::TAU / 4096.0);
        let (s, c) = ang.sin_cos();
        let t = [d.world_x as f32, d.world_y as f32, d.world_z as f32];
        let xf = |v: [f32; 3]| -> [f32; 3] {
            [
                c * v[0] + s * v[2] + t[0],
                v[1] + t[1],
                -s * v[0] + c * v[2] + t[2],
            ]
        };
        let start = self.tris.len();
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for tri in indices.chunks_exact(3) {
            let (Some(a), Some(b), Some(cc)) = (
                positions.get(tri[0] as usize),
                positions.get(tri[1] as usize),
                positions.get(tri[2] as usize),
            ) else {
                continue;
            };
            let w = [xf(*a), xf(*b), xf(*cc)];
            for v in &w {
                for ax in 0..3 {
                    lo[ax] = lo[ax].min(v[ax]);
                    hi[ax] = hi[ax].max(v[ax]);
                }
            }
            self.tris.push(w);
        }
        if self.tris.len() > start {
            self.draws.push(DrawRange {
                lo,
                hi,
                start,
                end: self.tris.len(),
                res_tmd: d.res_tmd,
            });
        }
    }

    /// No occluders collected (scene without static env geometry). The gate
    /// then never arms - with nothing to ray-cast, "fully occluded" is
    /// unprovable and the fade stays off rather than guessing.
    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    /// Total instanced triangle count (diagnostics / logs).
    pub fn triangle_count(&self) -> usize {
        self.tris.len()
    }

    /// Is the open segment `eye -> point` blocked by any occluder triangle?
    /// Both endpoints are trimmed by [`SEG_T_MIN`]/[`SEG_T_MAX`] so surfaces
    /// flush against either end don't self-occlude.
    pub fn segment_blocked(&self, eye: [f32; 3], point: [f32; 3]) -> bool {
        let dir = [point[0] - eye[0], point[1] - eye[1], point[2] - eye[2]];
        for dr in &self.draws {
            if !segment_hits_aabb(eye, dir, dr.lo, dr.hi) {
                continue;
            }
            for tri in &self.tris[dr.start..dr.end] {
                if segment_hits_triangle(eye, dir, tri) {
                    return true;
                }
            }
        }
        false
    }

    /// The 5-point body sample cross: centre, head, hips, and both
    /// shoulders (lateral = perpendicular to the eye ray in the ground
    /// plane). `centre` is the body centre in retail Y-down world
    /// coordinates (up = negative Y).
    pub fn sample_points(eye: [f32; 3], centre: [f32; 3]) -> [[f32; 3]; 5] {
        // Lateral axis: perpendicular to eye->centre in the XZ plane.
        let dx = centre[0] - eye[0];
        let dz = centre[2] - eye[2];
        let len = (dx * dx + dz * dz).sqrt().max(1e-3);
        let (px, pz) = (-dz / len, dx / len);
        [
            centre,
            // Y-down world: head is negative-Y of the centre.
            [centre[0], centre[1] - SAMPLE_HALF_HEIGHT, centre[2]],
            [centre[0], centre[1] + SAMPLE_HALF_HEIGHT * 0.7, centre[2]],
            [
                centre[0] + px * SAMPLE_HALF_WIDTH,
                centre[1],
                centre[2] + pz * SAMPLE_HALF_WIDTH,
            ],
            [
                centre[0] - px * SAMPLE_HALF_WIDTH,
                centre[1],
                centre[2] - pz * SAMPLE_HALF_WIDTH,
            ],
        ]
    }

    /// Per-sample verdicts of the 5-point cross (diagnostics; order matches
    /// [`Self::sample_points`]: centre, head, hips, shoulder+, shoulder-).
    pub fn sample_hits(&self, eye: [f32; 3], centre: [f32; 3]) -> [bool; 5] {
        let pts = Self::sample_points(eye, centre);
        std::array::from_fn(|i| self.segment_blocked(eye, pts[i]))
    }

    /// Diagnostics: the blocking triangle + its source `res_tmd` for the
    /// first hit on `eye -> point` (any draw order - not the nearest).
    pub fn first_hit(&self, eye: [f32; 3], point: [f32; 3]) -> Option<([[f32; 3]; 3], usize)> {
        let dir = [point[0] - eye[0], point[1] - eye[1], point[2] - eye[2]];
        for dr in &self.draws {
            if !segment_hits_aabb(eye, dir, dr.lo, dr.hi) {
                continue;
            }
            for tri in &self.tris[dr.start..dr.end] {
                if segment_hits_triangle(eye, dir, tri) {
                    return Some((*tri, dr.res_tmd));
                }
            }
        }
        None
    }

    /// The gate: is the character **completely** occluded from `eye`?
    /// Arms only when every sample of the 5-point cross is blocked.
    pub fn fully_occluded(&self, eye: [f32; 3], centre: [f32; 3]) -> bool {
        if self.is_empty() {
            return false;
        }
        Self::sample_points(eye, centre)
            .iter()
            .all(|p| self.segment_blocked(eye, *p))
    }
}

/// Slab test: does the segment `a + t*dir`, `t in [SEG_T_MIN, SEG_T_MAX]`,
/// pierce the AABB?
fn segment_hits_aabb(a: [f32; 3], dir: [f32; 3], lo: [f32; 3], hi: [f32; 3]) -> bool {
    let (mut t0, mut t1) = (SEG_T_MIN, SEG_T_MAX);
    for ax in 0..3 {
        if dir[ax].abs() < 1e-6 {
            if a[ax] < lo[ax] || a[ax] > hi[ax] {
                return false;
            }
            continue;
        }
        let inv = 1.0 / dir[ax];
        let (mut ta, mut tb) = ((lo[ax] - a[ax]) * inv, (hi[ax] - a[ax]) * inv);
        if ta > tb {
            std::mem::swap(&mut ta, &mut tb);
        }
        t0 = t0.max(ta);
        t1 = t1.min(tb);
        if t0 > t1 {
            return false;
        }
    }
    true
}

/// Möller-Trumbore, double-sided (the corpus' winding is mixed - see the
/// renderers' no-cull default), over the trimmed segment parameter range.
fn segment_hits_triangle(a: [f32; 3], dir: [f32; 3], tri: &[[f32; 3]; 3]) -> bool {
    let e1 = sub(tri[1], tri[0]);
    let e2 = sub(tri[2], tri[0]);
    let p = cross(dir, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-8 {
        return false;
    }
    let inv = 1.0 / det;
    let s = sub(a, tri[0]);
    let u = dot(s, p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let q = cross(s, e1);
    let v = dot(dir, q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    let t = dot(e2, q) * inv;
    (SEG_T_MIN..=SEG_T_MAX).contains(&t)
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field_env::EnvDraw;

    fn draw_at(x: i32, y: i32, z: i32, rot_y: u16) -> EnvDraw {
        EnvDraw {
            env_slot: 0,
            res_tmd: 0,
            world_x: x,
            world_y: y,
            world_z: z,
            rot_y,
            anim_id: 0,
            anchor: (0, 0),
        }
    }

    /// A wall quad in the local XY plane (facing +/-Z), 400 wide x 800
    /// tall: high enough that the eye-to-player segment (which crosses the
    /// wall plane well above the player's own height) stays inside it.
    fn wall() -> (Vec<[f32; 3]>, Vec<u32>) {
        let p = vec![
            [-200.0, -400.0, 0.0],
            [200.0, -400.0, 0.0],
            [200.0, 400.0, 0.0],
            [-200.0, 400.0, 0.0],
        ];
        (p, vec![0, 1, 2, 0, 2, 3])
    }

    const EYE: [f32; 3] = [0.0, -500.0, -1000.0];
    const PLAYER: [f32; 3] = [0.0, -65.0, 500.0];

    fn set_with_wall_at(z: i32) -> FieldOccluders {
        let (p, i) = wall();
        let mut o = FieldOccluders::default();
        o.add_instanced_mesh(&p, &i, &draw_at(0, 0, z, 0));
        o
    }

    /// A wall crossing the corridor blocks every sample; the gate arms.
    #[test]
    fn wall_across_the_corridor_arms_the_gate() {
        let o = set_with_wall_at(0);
        assert!(o.segment_blocked(EYE, PLAYER));
        assert!(o.fully_occluded(EYE, PLAYER));
    }

    /// The same wall BEHIND the player does not block (the segment ends at
    /// the player), and neither does one behind the eye.
    #[test]
    fn walls_outside_the_segment_do_not_block() {
        assert!(!set_with_wall_at(800).fully_occluded(EYE, PLAYER));
        assert!(!set_with_wall_at(-1500).fully_occluded(EYE, PLAYER));
    }

    /// A wall covering only one side of the body: the centre ray is blocked
    /// but a shoulder sample clears it - partial cover must NOT arm the
    /// gate ("not completely occluded" = no fade at all).
    #[test]
    fn partial_cover_does_not_arm_the_gate() {
        let (p, i) = wall();
        let mut o = FieldOccluders::default();
        // Shift the wall so its edge splits the body: covers centre +
        // right shoulder, leaves the left shoulder sample visible.
        o.add_instanced_mesh(&p, &i, &draw_at(-190, 0, 0, 0));
        assert!(o.segment_blocked(EYE, PLAYER));
        assert!(!o.fully_occluded(EYE, PLAYER));
    }

    /// Yaw instancing: the wall rotated a quarter turn lies along the
    /// corridor instead of across it - nothing blocks.
    #[test]
    fn rotated_wall_clears_the_corridor() {
        let (p, i) = wall();
        let mut o = FieldOccluders::default();
        o.add_instanced_mesh(&p, &i, &draw_at(0, 0, 0, 1024));
        assert!(!o.fully_occluded(EYE, PLAYER));
    }

    /// Animated draws are not occluders; an empty set never arms.
    #[test]
    fn animated_and_empty_sets_never_arm() {
        let o = FieldOccluders::default();
        assert!(o.is_empty());
        assert!(!o.fully_occluded(EYE, PLAYER));
    }

    /// A surface flush against the player (their own floor tier / the wall
    /// they hug) is trimmed by the segment-parameter epsilon.
    #[test]
    fn surface_flush_with_the_player_does_not_self_occlude() {
        // Wall exactly at the player's Z: intersection t = 1.0, trimmed.
        let o = set_with_wall_at(PLAYER[2] as i32);
        assert!(!o.segment_blocked(EYE, PLAYER));
    }
}
