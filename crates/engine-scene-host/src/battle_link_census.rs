//! What a battle frame links into its ordering table, counted from world
//! state alone.
//!
//! Retail's battle pass sends every primitive of every drawn object through
//! the dispatcher `FUN_80043390`, whose per-group handlers transform, cull and
//! link each one, so a frame's CPU work follows the polygons it links
//! (`docs/subsystems/battle-stage-camera.md`, "a frame's work is what it
//! links"). The frame-cost model prices from that count
//! (`World::note_linked_polys`). This module counts it with no renderer: the
//! geometry is the disc's (the stage TMD, each body's TMD), built once a
//! fight by the scene host ([`BattleLinkGeometry`]), and everything that
//! moves - the camera, each body's placement and pose, which objects it draws,
//! the grid's cell culls - is read off the [`World`] through the kernels the
//! play hosts draw with. Every host that ticks a `SceneHost` - the native
//! window, a headless session, the play page - counts the same number.
//!
//! The per-primitive rules are the dispatcher's:
//!
//! - **Near reject.** A primitive whose `OTZ` (mean corner `SZ` over the
//!   battle's ordering-table shift `2`) is under the floor `0x10` is not
//!   linked ([`BATTLE_NEAR_SZ`]); one with a corner behind the eye is not
//!   either.
//! - **Winding.** A single-sided draw word drops a back-facing primitive
//!   (`NCLIP` over corners `0..=2`), a double-sided one keeps both.
//! - **The ground grid** links four `POLY_GT4` a visible cell
//!   ([`legaia_engine_battle_vm::battle_ground_grid::visible_cell_count`]).
//!
//! What is not counted: the effect parts and billboards, and the 2D layers
//! (the HUD, the menus, the readouts). Those reach the frame cost through its
//! fitted base.
//!
//! REF: FUN_80043390, FUN_80048A08, FUN_8001ADA4, FUN_801D02C0

use std::sync::Arc;

use glam::{Mat3, Mat4, Vec3};
use legaia_engine_battle_loop::derived_cost::{DerivedUnits, PrimTally};
use legaia_engine_core::world::World;
use legaia_engine_system::gpu_draw_cost::Faces;

/// The `SZ` a primitive's corner mean must reach to be linked in battle: the
/// near floor `0x10` (`0x1F80037E`) on `OTZ = mean SZ >> 2`.
pub const BATTLE_NEAR_SZ: f32 = 64.0;

/// One primitive of a census mesh: its corners into the object's vertices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LinkPrim {
    v: [u16; 4],
    quad: bool,
    /// The dispatcher's primitive kind (the group flags `>> 1`), which picks
    /// the leaf and so its cull rules ([`RetailLeaf`]).
    kind: u8,
}

/// One TMD object's vertices and primitives.
#[derive(Debug, Clone, Default)]
pub struct LinkObject {
    verts: Vec<[f32; 3]>,
    prims: Vec<LinkPrim>,
}

/// A TMD's primitives, object by object, as the census walks them.
#[derive(Debug, Clone, Default)]
pub struct LinkMesh {
    objects: Vec<LinkObject>,
}

impl LinkMesh {
    /// Every object of `tmd`, textured and untextured primitives alike (the
    /// dispatcher links both kinds).
    pub fn from_tmd(tmd: &legaia_tmd::Tmd, raw: &[u8]) -> Self {
        Self::from_objects(tmd, raw, 0..tmd.objects.len())
    }

    /// The named objects of `tmd`, in that order.
    pub fn from_objects(
        tmd: &legaia_tmd::Tmd,
        raw: &[u8],
        objects: impl IntoIterator<Item = usize>,
    ) -> Self {
        let mut out = Self::default();
        for oi in objects {
            let Some(o) = tmd.objects.get(oi) else {
                continue;
            };
            let verts = o
                .vertices
                .iter()
                .map(|v| [f32::from(v.x), f32::from(v.y), f32::from(v.z)])
                .collect::<Vec<_>>();
            let n_vert = o.header.n_vert;
            let mut prims = Vec::new();
            for g in legaia_tmd::legaia_prims::iter_groups_lenient(
                raw,
                o.primitives_byte_offset,
                o.primitives_byte_size,
            ) {
                let kind = (g.header.flags >> 1) as u8;
                for p in &g.prims {
                    let idx = p.vertex_indices();
                    if idx.iter().any(|&i| u32::from(i) >= n_vert) {
                        continue;
                    }
                    match idx.len() {
                        3 => prims.push(LinkPrim {
                            v: [idx[0], idx[1], idx[2], 0],
                            quad: false,
                            kind,
                        }),
                        4 => prims.push(LinkPrim {
                            v: [idx[0], idx[1], idx[2], idx[3]],
                            quad: true,
                            kind,
                        }),
                        _ => {}
                    }
                }
            }
            out.objects.push(LinkObject { verts, prims });
        }
        out
    }

    /// The object count.
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// Every primitive, whatever is drawn.
    pub fn prim_count(&self) -> usize {
        self.objects.iter().map(|o| o.prims.len()).sum()
    }
}

/// The stage's census geometry: the backdrop shell's fixed objects and the
/// slot-1 object the backdrop draw turns, with the per-stage second copy.
#[derive(Debug, Clone)]
pub struct StageLink {
    pub fixed: LinkMesh,
    pub spun: Option<LinkMesh>,
    pub second: legaia_scene_formats::battle_backdrop::SecondCopy,
}

/// What a fight's census reads off the disc, built once a fight by the scene
/// host: the stage and each seat's body.
#[derive(Debug, Clone, Default)]
pub struct BattleLinkGeometry {
    /// The battle entry serial (`World::battle.entry_serial`) it was built for.
    pub serial: u32,
    pub stage: Option<StageLink>,
    /// Per actor slot: the body's identity key and its mesh.
    pub bodies: Vec<Option<(u32, Arc<LinkMesh>)>>,
    /// The effect-model library's meshes the parts have drawn, by pool index,
    /// each under the identity of the pool entry it was built from.
    pub parts: std::collections::HashMap<usize, (usize, Arc<LinkMesh>)>,
}

impl BattleLinkGeometry {
    /// The mesh seated at `slot` under `key`, if one is.
    pub fn body(&self, slot: usize, key: u32) -> Option<&LinkMesh> {
        match self.bodies.get(slot)? {
            Some((k, m)) if *k == key => Some(m),
            _ => None,
        }
    }
}

/// One frame's linked polygons by class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LinkCount {
    pub stage: u32,
    pub grid: u32,
    pub bodies: u32,
    pub ghosts: u32,
    /// The drawn bodies' ground shadows, [`SHADOW_COLUMNS`] packets each.
    pub shadows: u32,
    /// The effects: the summon and move-FX parts, the effect models, the
    /// draw-kind-4 nodes.
    pub effects: u32,
    /// The effect-pool sprites, one `POLY_FT4` each.
    pub sprites: u32,
    /// The swept bands the hosts draw as screen primitives: the weapon
    /// trails, the move-FX after-image streak and the cast ribbons, one
    /// packet a quad.
    pub trails: u32,
    /// The primitives the floor-clipped bodies' meshes hold: a sinking body
    /// (`+0x42` non-zero, `0x800504AC`) draws through the full-TMD renderer
    /// `FUN_8002735C` behind `FUN_8001C204` (`FUN_80048A08`'s `+0x42` arm),
    /// which transforms every primitive of each drawn object, not the
    /// dispatcher's per-primitive leaves. Its work, not a link count: left
    /// out of [`Self::total`] and priced on its own
    /// (`World::note_floor_clip_walk`).
    pub floor_clip_walk: u32,
    /// The same frame by the units the derived pricing reads
    /// (`legaia_engine_battle_loop::derived_cost`): every primitive the
    /// dispatcher walks by topology and fate, the objects, bodies, ghosts,
    /// grid cells and kind-4 builds.
    pub units: legaia_engine_battle_loop::derived_cost::DerivedUnits,
    /// The stage's and the bodies' own tallies, for holding the census's
    /// fates against retail's (`retail-exec battle-cost --mode fates`).
    pub stage_tally: PrimTally,
    pub body_tally: PrimTally,
}

impl LinkCount {
    pub fn total(&self) -> u32 {
        self.stage
            + self.grid
            + self.bodies
            + self.ghosts
            + self.shadows
            + self.effects
            + self.sprites
            + self.trails
    }
}

/// The packets a body's ground shadow links: the 24-column disc
/// `FUN_80028158` builds in mode `1`
/// (`legaia_engine_effects::effect_default_arm::ground_shadow_mesh`).
pub const SHADOW_COLUMNS: u32 = 24;

/// Whether drawn body `i` casts its ground shadow this frame: the gate of
/// `legaia_engine_world_views::battle_draws::battle_ground_shadow`, without
/// building the disc.
fn casts_shadow(
    a: &legaia_engine_core::world::Actor,
    plan: &legaia_engine_core::world::BattleActorDrawPlan,
) -> bool {
    use legaia_engine_battle_vm::battle_actor_draw::{body_shadow_skip, shadow_plan};
    let m = &a.move_state;
    let pitch = if a.battle.render_pitch != 0 {
        a.battle.render_pitch as i16
    } else {
        m.render_24
    };
    let roll = if a.battle.render_roll != 0 {
        a.battle.render_roll as i16
    } else {
        m.render_28
    };
    shadow_plan(
        body_shadow_skip(plan.seat, false),
        plan.draw_colour,
        m.world_y,
        plan.radius,
        pitch,
        roll,
    )
    .is_some_and(|s| s.drawn)
}

/// The census's reusable buffers: one projected corner a vertex. Held by the
/// caller so a frame allocates nothing once it has grown.
#[derive(Debug, Default)]
pub struct LinkScratch {
    clip: Vec<[f32; 3]>,
    /// Decide each primitive by the leaves' own rules ([`tally_retail`]):
    /// under `WorldToggles::battle_derived_frame_cost` or
    /// `WorldToggles::battle_census_retail_culls`, set each count.
    retail_rules: bool,
    /// Each drawn body's seat and tally this count, for
    /// `LEGAIA_DIAG_CENSUS_FATES`.
    bodies: Vec<(usize, PrimTally)>,
}

/// The body key the summon creature's seat is counted under.
pub const SUMMON_BODY_KEY: u32 = 0x4000_0000;

/// The body key a seat's mesh is built under: the monster id, a party
/// member's roster slot with the high bit set, or [`SUMMON_BODY_KEY`] for
/// the summon creature's seat. `None` for a seat with no body.
pub fn body_key(world: &World, slot: usize) -> Option<u32> {
    let a = world.actors.get(slot)?;
    if world.casting.summon_actor_slot.map(usize::from) == Some(slot) {
        return Some(SUMMON_BODY_KEY);
    }
    if let Some(id) = a.battle_monster_id {
        return Some(u32::from(id));
    }
    (slot < usize::from(world.party.party_count))
        .then(|| 0x8000_0000 | world.party_roster_slot(slot) as u32)
}

/// Count one object's primitives under `m` (column-major clip transform),
/// its vertices first posed by `bone` (`R v + T`, the `Rz Ry Rx` Euler
/// triple of `tmd_to_vram_mesh_posed_rot`, or a translation alone), by
/// topology and fate: the linked primitives, those the off-screen reject
/// drops, and those the near floor or the winding test drops before it.
fn tally_object(
    obj: &LinkObject,
    bone: Option<&([i16; 3], [i16; 3])>,
    rotate: bool,
    m: &[f32; 16],
    faces: Faces,
    offscreen_reject: bool,
    scratch: &mut LinkScratch,
) -> PrimTally {
    const A2R: f32 = std::f32::consts::TAU / 4096.0;
    let (pos, trig) = match bone {
        Some((p, r)) if rotate => {
            let (sx, cx) = (f32::from(r[0]) * A2R).sin_cos();
            let (sy, cy) = (f32::from(r[1]) * A2R).sin_cos();
            let (sz, cz) = (f32::from(r[2]) * A2R).sin_cos();
            (p.map(f32::from), Some([cx, sx, cy, sy, cz, sz]))
        }
        Some((p, _)) => (p.map(f32::from), None),
        None => ([0.0; 3], None),
    };
    scratch.clip.clear();
    for v in &obj.verts {
        let r = match trig {
            Some([cx, sx, cy, sy, cz, sz]) => legaia_tmd::mesh::rot_zyx(*v, cx, sx, cy, sy, cz, sz),
            None => *v,
        };
        let p = [r[0] + pos[0], r[1] + pos[1], r[2] + pos[2]];
        let x = m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12];
        let y = m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13];
        let w = m[3] * p[0] + m[7] * p[1] + m[11] * p[2] + m[15];
        scratch.clip.push([x, y, w]);
    }
    if scratch.retail_rules {
        return tally_retail(obj, &scratch.clip, faces, offscreen_reject);
    }
    let clip = &scratch.clip;
    let mut t = PrimTally::default();
    for p in &obj.prims {
        let k = if p.quad { 4 } else { 3 };
        let mut sz = 0.0;
        let mut behind = false;
        for &i in &p.v[..k] {
            let w = clip[usize::from(i)][2];
            behind |= w <= 1e-3;
            sz += w.max(0.0);
        }
        let fate = if behind || sz < BATTLE_NEAR_SZ * k as f32 {
            t.near_culled += 1;
            Fate::Culled
        } else if offscreen_reject && {
            // The leaves' off-screen reject (`FUN_80044798` on the quad
            // leaves), called under a colour word carrying `0x40` in its top
            // byte, as the backdrop's is: every corner past one edge of the
            // frame. Retail's tables drop the backdrop's off-screen triangles
            // too.
            let mut edges = [true; 4];
            for &i in &p.v[..k] {
                let c = clip[usize::from(i)];
                let sx = (c[0] / c[2] * 0.5 + 0.5) * 320.0;
                let sy = (0.5 - c[1] / c[2] * 0.5) * 240.0;
                edges[0] &= sy < 0.0;
                edges[1] &= sy >= 240.0;
                edges[2] &= sx <= 0.0;
                edges[3] &= sx >= 320.0;
            }
            edges.iter().any(|&e| e)
        } {
            Fate::OffScreen
        } else {
            let kept = match faces {
                Faces::Both => true,
                single => {
                    let ndc = |i: u16| {
                        let c = clip[usize::from(i)];
                        [c[0] / c[2], c[1] / c[2]]
                    };
                    let side = |a: [f32; 2], b: [f32; 2], d: [f32; 2]| {
                        let cross = (b[0] - a[0]) * (d[1] - a[1]) - (d[0] - a[0]) * (b[1] - a[1]);
                        match single {
                            Faces::Ccw => cross > 0.0,
                            _ => cross < 0.0,
                        }
                    };
                    let (a, b, d) = (ndc(p.v[0]), ndc(p.v[1]), ndc(p.v[2]));
                    // A quad leaf tests both halves (`0x80045A38..0x80045A6C`).
                    side(a, b, d) || (p.quad && side(b, ndc(p.v[3]), d))
                }
            };
            if kept { Fate::Linked } else { Fate::Culled }
        };
        match (p.quad, fate) {
            (false, Fate::Linked) => t.tris_linked += 1,
            (false, Fate::OffScreen) => t.tris_off_screen += 1,
            (false, Fate::Culled) => t.tris_culled += 1,
            (true, Fate::Linked) => t.quads_linked += 1,
            (true, Fate::OffScreen) => t.quads_off_screen += 1,
            (true, Fate::Culled) => t.quads_culled += 1,
        }
    }
    t
}

/// The battle GTE's projection of a clip-space corner `[x, y, w]` (the
/// hosts' battle matrix, `psx_projection` at `H = 256`, `OFX` / `OFY`
/// `160` / `114`) the way `RTPS` / `RTPT` land it: the eye-space `IR1` /
/// `IR2` clamped to `i16`, `SZ3` clamped to `0..=0xFFFF`, the `H / SZ`
/// quotient capped at `0x1FFFF` (whenever `SZ * 2 <= H`, a corner at or
/// behind the eye among them), the screen pair truncated and clamped to
/// `-0x400..=0x3FF`.
fn gte_corner(c: [f32; 3]) -> (i32, i32, i32) {
    use legaia_engine_camera::projection::{GTE_H, GTE_OFY_NDC_BIAS};
    let ex = (c[0] * 160.0 / GTE_H).floor().clamp(-32768.0, 32767.0);
    let ey = (-(c[1] - GTE_OFY_NDC_BIAS * c[2]) * 120.0 / GTE_H)
        .floor()
        .clamp(-32768.0, 32767.0);
    let sz = c[2].floor().clamp(0.0, 65535.0);
    let q = if sz * 2.0 > GTE_H {
        (GTE_H * 65536.0 / sz).min(131071.0).floor()
    } else {
        131071.0
    };
    let px = |ofs: f32, ir: f32| -> i32 {
        let mac0 = ofs * 65536.0 + ir * q;
        ((mac0 / 65536.0).floor() as i32).clamp(-0x400, 0x3FF)
    };
    (px(160.0, ex), px(114.0, ey), sz as i32)
}

/// The leaf a primitive kind runs through on the battle's depth-cued banks,
/// by its cull rules (`docs/subsystems/battle-stage-camera.md`, "The
/// census").
#[derive(Clone, Copy, PartialEq, Eq)]
enum RetailLeaf {
    /// A triangle (`0x80044C14`, `0x80044FDC`, `0x800453BC`,
    /// `0x800457C4`): kept on `NCLIP >= 0`, the near test, then the
    /// off-screen test whatever the word. Kind `12` (`0x800448B0`) has no
    /// off-screen test.
    Tri { off_screen: bool },
    /// A quad of kinds `13` / `15` / `17` (`0x80044A3C`, `0x80045194`,
    /// `0x80044DC8`): kept on its first half's `NCLIP > 0` or, failing that,
    /// its second half's `< 0` (`blez` / `bgez`), the near test, then the
    /// off-screen test (none on kind `13`).
    QuadStrict { off_screen: bool },
    /// A kind-`19` quad (`0x80045584`, `0x80045988`): kept on `>= 0` or the
    /// second half's `<= 0` (`bltz` / `bgtz`), the off-screen test only under
    /// a word carrying `0x40` in its top byte, then the near test.
    QuadLoose,
}

impl RetailLeaf {
    fn of(kind: u8, quad: bool) -> Self {
        match (quad, kind) {
            (false, 12) => Self::Tri { off_screen: false },
            (false, _) => Self::Tri { off_screen: true },
            (true, 13) => Self::QuadStrict { off_screen: false },
            (true, 15 | 17) => Self::QuadStrict { off_screen: true },
            (true, _) => Self::QuadLoose,
        }
    }
}

/// [`tally_object`] by the leaves' own decisions on the GTE's integer
/// corners: the winding test on `NCLIP` (with a double-sided word's
/// `0x7FFFFFFF` mask, so a degenerate half still decides a strict quad),
/// the near floor on `OTZ = ZSF * sum SZ >> 12` against `0x10` (the
/// battle's shift `2`: `ZSF3 = 0x155`, `ZSF4 = 0x100`), and the off-screen
/// test `FUN_80044798` (every corner past one edge) where the leaf runs it,
/// in the leaf's order. `quad_off_screen` is the word's `0x40` bit, set on
/// the backdrop's draws. A corner at or behind the eye is not dropped as
/// such: it projects as the GTE saturates it.
fn tally_retail(
    obj: &LinkObject,
    clip: &[[f32; 3]],
    faces: Faces,
    quad_off_screen: bool,
) -> PrimTally {
    let mut t = PrimTally::default();
    let corners: Vec<(i32, i32, i32)> = clip.iter().map(|&c| gte_corner(c)).collect();
    // The census's winding sign against retail's: `Faces::Ccw` keeps a
    // counter-clockwise turn in NDC (y up), which is a negative `NCLIP` on
    // the screen (y down).
    let (sign, mask) = match faces {
        Faces::Both => (1i64, 0x7FFF_FFFFu32),
        Faces::Ccw => (-1, 0xFFFF_FFFF),
        Faces::Cw => (1, 0xFFFF_FFFF),
    };
    let nclip = |a: (i32, i32, i32), b: (i32, i32, i32), c: (i32, i32, i32)| -> i32 {
        let (ax, ay, bx, by, cx, cy) = (
            i64::from(a.0),
            i64::from(a.1),
            i64::from(b.0),
            i64::from(b.1),
            i64::from(c.0),
            i64::from(c.1),
        );
        let v = sign * (ax * by + bx * cy + cx * ay - ax * cy - bx * ay - cx * by);
        ((v as i32 as u32) & mask) as i32
    };
    for p in &obj.prims {
        let k = if p.quad { 4 } else { 3 };
        let c: [(i32, i32, i32); 4] =
            std::array::from_fn(|i| corners[usize::from(p.v[i.min(k - 1)])]);
        let leaf = RetailLeaf::of(p.kind, p.quad);
        let first = nclip(c[0], c[1], c[2]);
        let faced = match leaf {
            RetailLeaf::Tri { .. } => first >= 0,
            RetailLeaf::QuadStrict { .. } => first > 0 || nclip(c[1], c[2], c[3]) < 0,
            RetailLeaf::QuadLoose => first >= 0 || nclip(c[1], c[2], c[3]) <= 0,
        };
        let sum: i64 = c[..k].iter().map(|x| i64::from(x.2)).sum();
        let otz = if p.quad {
            (0x100 * sum) >> 12
        } else {
            (0x155 * sum) >> 12
        };
        let near = otz < 0x10;
        let off = || {
            let all = |f: &dyn Fn(&(i32, i32, i32)) -> bool| c[..k].iter().all(f);
            all(&|x| x.1 < 0)
                || all(&|x| x.1 >= 0xF0)
                || all(&|x| x.0 <= 0)
                || all(&|x| x.0 >= 0x140)
        };
        let fate = if !faced {
            Fate::Culled
        } else {
            match leaf {
                RetailLeaf::QuadLoose => {
                    if quad_off_screen && off() {
                        Fate::OffScreen
                    } else if near {
                        t.near_culled += 1;
                        Fate::Culled
                    } else {
                        Fate::Linked
                    }
                }
                RetailLeaf::Tri { off_screen } | RetailLeaf::QuadStrict { off_screen } => {
                    if near {
                        t.near_culled += 1;
                        Fate::Culled
                    } else if off_screen && off() {
                        Fate::OffScreen
                    } else {
                        Fate::Linked
                    }
                }
            }
        };
        match (p.quad, fate) {
            (false, Fate::Linked) => t.tris_linked += 1,
            (false, Fate::OffScreen) => t.tris_off_screen += 1,
            (false, Fate::Culled) => t.tris_culled += 1,
            (true, Fate::Linked) => t.quads_linked += 1,
            (true, Fate::OffScreen) => t.quads_off_screen += 1,
            (true, Fate::Culled) => t.quads_culled += 1,
        }
    }
    t
}

/// How a census primitive leaves its leaf.
#[derive(Clone, Copy)]
enum Fate {
    Linked,
    OffScreen,
    Culled,
}

/// Count a whole mesh (every object, unposed) under one transform, each
/// object one dispatcher call into `units`.
fn count_mesh(
    mesh: &LinkMesh,
    m: &Mat4,
    faces: Faces,
    offscreen_reject: bool,
    units: &mut DerivedUnits,
    scratch: &mut LinkScratch,
) -> u32 {
    let m = m.to_cols_array();
    let mut n = 0;
    for o in &mesh.objects {
        let t = tally_object(o, None, false, &m, faces, offscreen_reject, scratch);
        n += t.linked();
        units.prims.add(&t);
        units.objects += 1;
    }
    n
}

/// The packets a mesh links under one transform when its quads go to the
/// bank-3 leaf, sixteen `GT3`s each
/// (`legaia_engine_effects::summon::split_bank3_quads`): a kept quad counts
/// sixteen. Its units count the quad's sixteen links as triangles.
fn count_split_mesh(
    mesh: &LinkMesh,
    m: &Mat4,
    faces: Faces,
    units: &mut DerivedUnits,
    scratch: &mut LinkScratch,
) -> u32 {
    let m = m.to_cols_array();
    let mut n = 0;
    for o in &mesh.objects {
        let mut t = tally_object(o, None, false, &m, faces, false, scratch);
        n += t.tris_linked + t.quads_linked * 16;
        t.tris_linked += t.quads_linked * 16;
        t.quads_linked = 0;
        units.prims.add(&t);
        units.objects += 1;
    }
    n
}

/// A posed body under `mvp`: each drawn object through its bone. Returns the
/// links, the drawn objects and their tally.
fn count_body(
    mesh: &LinkMesh,
    bones: Option<&[([i16; 3], [i16; 3])]>,
    rotate: bool,
    drawn: Option<&[bool]>,
    mvp: &Mat4,
    faces: Faces,
    scratch: &mut LinkScratch,
) -> (u32, u32, PrimTally) {
    let m = mvp.to_cols_array();
    let (mut objects, mut tally) = (0, PrimTally::default());
    for (oi, o) in mesh.objects.iter().enumerate() {
        if drawn.is_some_and(|d| !d.get(oi).copied().unwrap_or(false)) {
            continue;
        }
        let bone = bones.and_then(|b| b.get(oi));
        tally.add(&tally_object(o, bone, rotate, &m, faces, false, scratch));
        objects += 1;
    }
    (tally.linked(), objects, tally)
}

/// The battle stage model: the 4x base matrix with the frame's Y flip.
fn stage_model() -> Mat4 {
    let s = legaia_engine_camera::projection::BATTLE_WORLD_SCALE;
    Mat4::from_scale(Vec3::new(s, -s, s))
}

/// A battle body's model, the play hosts' placement law (`actor_model`'s
/// battle arm): `T(F pos) Ry(facing) F`, the pitch about the origin, the roll
/// nearest the mesh, the render scale.
fn body_model(a: &legaia_engine_core::world::Actor) -> Mat4 {
    use legaia_engine_battle::battle_afterimage::placement_pos;
    let pos = placement_pos([
        a.move_state.world_x as f32,
        a.move_state.world_y as f32,
        a.move_state.world_z as f32,
    ]);
    let rot = Mat4::from_rotation_y(
        f32::from(a.battle.facing_angle & 0xFFF) / 4096.0 * std::f32::consts::TAU,
    );
    let t = Mat4::from_translation(Vec3::from(pos));
    let mut model = t * rot * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
    if a.battle.render_pitch & 0xFFF != 0 {
        model = t * Mat4::from_rotation_x(-a.battle.render_pitch_radians()) * t.inverse() * model;
    }
    if a.battle.render_roll & 0xFFF != 0 {
        model *= Mat4::from_rotation_z(a.battle.render_roll_radians());
    }
    model * Mat4::from_scale(Vec3::splat(a.battle.render_scale_q12()))
}

/// The polygons the battle frame `world` stands on links, by class. Zero
/// outside a battle.
pub fn battle_link_count(
    world: &World,
    geom: &BattleLinkGeometry,
    scratch: &mut LinkScratch,
) -> LinkCount {
    use legaia_engine_battle_vm::battle_ground_grid as grid;
    use legaia_engine_camera::projection::{BATTLE_WORLD_SCALE, battle_vp};
    let mut out = LinkCount::default();
    if world.mode != legaia_engine_mode::mode::SceneMode::Battle {
        return out;
    }
    let mut units = DerivedUnits::default();
    scratch.retail_rules =
        world.toggles.battle_derived_frame_cost || world.toggles.battle_census_retail_culls;
    scratch.bodies.clear();
    let pose = world.battle_cam_pose();
    // The camera the window composes, `cam` (the focus on the scaled stage);
    // the aspect does not move a cull.
    let cam = Mat4::from_cols_array(&battle_vp(&pose, BATTLE_WORLD_SCALE, 320.0 / 240.0));
    // The backdrop pair, while the backdrop draws at all.
    if world.battle_backdrop_cue().is_some()
        && let Some(st) = geom.stage.as_ref()
    {
        use crate::scene_host::{backdrop_copy_nclip, backdrop_slot_1_basis};
        let base = cam * stage_model();
        let second = Mat4::from_scale(Vec3::from(st.second.scale()));
        out.stage += count_mesh(
            &st.fixed,
            &base,
            Faces::of_word(backdrop_copy_nclip(st.second, false)),
            true,
            &mut units,
            scratch,
        );
        out.stage += count_mesh(
            &st.fixed,
            &(base * second),
            Faces::of_word(backdrop_copy_nclip(st.second, true)),
            true,
            &mut units,
            scratch,
        );
        if let Some(spun) = st.spun.as_ref() {
            let yaw = world.battle_backdrop_slot_1_yaw();
            for copy_b in [false, true] {
                let rows = backdrop_slot_1_basis(st.second, yaw, copy_b);
                let basis = Mat4::from_mat3(Mat3::from_cols_array_2d(&rows).transpose());
                out.stage += count_mesh(
                    spun,
                    &(base * basis),
                    Faces::of_word(backdrop_copy_nclip(st.second, copy_b)),
                    true,
                    &mut units,
                    scratch,
                );
            }
        }
    }
    out.stage_tally = units.prims;
    if world.battle_ground_grid_drawn() {
        let cells = legaia_scene_formats::battle_backdrop::GRID_CELLS as i16;
        let kept = grid::visible_cell_count(&pose, BATTLE_WORLD_SCALE, cells, cells);
        out.grid = (kept * grid::QUADS_PER_CELL) as u32;
        units.grid_cells = Some(kept as u32);
    }
    let actor_cam = cam * Mat4::from_scale(Vec3::splat(BATTLE_WORLD_SCALE));
    let outdoor = world.battle.stage_outdoor;
    for (i, a) in world.actors.iter().enumerate() {
        if a.tmd_binding.is_none() || !world.actor_slot_drawn(i) || world.battle_body_hidden(i) {
            continue;
        }
        let Some(key) = body_key(world, i) else {
            continue;
        };
        let Some(mesh) = geom.body(i, key) else {
            continue;
        };
        let Some(plan) = world.battle_actor_draw_plan(i, Some(&pose), BATTLE_WORLD_SCALE, outdoor)
        else {
            continue;
        };
        if !plan.drawn {
            continue;
        }
        if a.active && casts_shadow(a, &plan) {
            out.shadows += SHADOW_COLUMNS;
            // The disc is a shape-`0` build inside the body draw, linked
            // through the dispatcher as one object.
            units.builds += 1;
            units.build_segments += legaia_engine_battle_loop::derived_cost::SHADOW_SEGMENTS;
            units.prims.quads_linked += SHADOW_COLUMNS;
            units.objects += 1;
        }
        let drawn = legaia_engine_world_views::battle_draws::battle_drawn_objects(
            world,
            i,
            mesh.object_count(),
        );
        let rotate = a.battle_animation.is_some() || world.player_actor_slot == Some(i as u8);
        let bones = a.pose_frame.as_ref().map(|p| p.bone_outputs.as_slice());
        let (nb, objects, tally) = count_body(
            mesh,
            bones,
            rotate,
            drawn.as_deref(),
            &(actor_cam * body_model(a)),
            Faces::of_word(plan.nclip_mode()),
            scratch,
        );
        out.bodies += nb;
        units.bodies += 1;
        units.body_objects += objects;
        // A floor-clipped body's objects go through the full-TMD renderer,
        // not the dispatcher.
        out.body_tally.add(&tally);
        scratch.bodies.push((i, tally));
        if !a.battle.floor_clip {
            units.prims.add(&tally);
            units.objects += objects;
        }
        if a.battle.floor_clip {
            out.floor_clip_walk += mesh
                .objects
                .iter()
                .enumerate()
                .filter(|(oi, _)| {
                    drawn
                        .as_deref()
                        .is_none_or(|d| d.get(*oi).copied().unwrap_or(false))
                })
                .map(|(_, o)| o.prims.len() as u32)
                .sum::<u32>();
        }
    }
    for g in legaia_engine_world_views::battle_draws::battle_ghost_draws(world) {
        let i = usize::from(g.actor_slot);
        let Some(a) = world.actors.get(i) else {
            continue;
        };
        if a.tmd_binding.is_none() {
            continue;
        }
        let Some(mesh) = body_key(world, i).and_then(|k| geom.body(i, k)) else {
            continue;
        };
        let gpos = legaia_engine_battle::battle_afterimage::placement_pos(g.pos.map(|c| c as f32));
        let rot = Mat4::from_rotation_y(
            f32::from(a.battle.facing_angle & 0xFFF) / 4096.0 * std::f32::consts::TAU,
        );
        let model = Mat4::from_translation(Vec3::from(gpos))
            * rot
            * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
        let drawn = g.drawn_objects(mesh.object_count());
        let (n, objects, tally) = count_body(
            mesh,
            Some(&g.pose.bone_outputs),
            true,
            drawn.as_deref(),
            &(actor_cam * model),
            Faces::of_word(g.nclip_mode()),
            scratch,
        );
        out.ghosts += n;
        units.ghosts += 1;
        units.ghost_objects += objects;
        units.objects += objects;
        units.prims.add(&tally);
    }
    out.effects = count_effects(world, geom, &pose, &actor_cam, &mut units, scratch);
    out.sprites = world.active_effect_sprites().len() as u32;
    {
        use legaia_engine_world_views::screen_passes::{
            cast_ribbon_prims, move_fx_streak_quads, weapon_trail_prims,
        };
        let ot = legaia_render_kernels::streak_pass::MOVE_FX_STREAK_OT;
        out.trails = (weapon_trail_prims(world).len()
            + move_fx_streak_quads(world, &actor_cam).len()
            + cast_ribbon_prims(world, &actor_cam, ot).len()) as u32;
    }
    units.trail_packets = out.trails;
    units.floor_clip_prims = out.floor_clip_walk;
    out.units = units;
    out
}

/// The effects' packets: each part and effect model through the library mesh
/// it draws, under its own model and winding rule; each draw-kind-4 node's
/// quads.
fn count_effects(
    world: &World,
    geom: &BattleLinkGeometry,
    pose: &legaia_engine_camera::projection::BattleCamPose,
    fx_cam: &Mat4,
    units: &mut DerivedUnits,
    scratch: &mut LinkScratch,
) -> u32 {
    use legaia_engine_core::world::BATTLE_BODY_NCLIP;
    let part_cam = geom
        .stage
        .as_ref()
        .map(|_| legaia_render_kernels::gte::PartCameraPose::from_battle(pose));
    // No shadows here: the body pass counts them.
    let fx = legaia_engine_world_views::effect_draws::effect_model_draws(
        world,
        part_cam.as_ref(),
        |_| None,
    );
    let single = Faces::of_word(BATTLE_BODY_NCLIP);
    let mut n = 0;
    for p in &fx.parts {
        let Some((_, mesh)) = geom.parts.get(&p.draw.model_index) else {
            continue;
        };
        let m = *fx_cam * Mat4::from_cols_array(&p.model);
        let faces = if p.single_sided { single } else { Faces::Both };
        n += if p.draw.colour.split_quads {
            count_split_mesh(mesh, &m, faces, units, scratch)
        } else {
            count_mesh(mesh, &m, faces, false, units, scratch)
        };
    }
    for e in &fx.effect_models {
        if let Some((_, mesh)) = geom.parts.get(&e.index) {
            let m = *fx_cam * Mat4::from_cols_array(&e.model);
            n += count_mesh(mesh, &m, Faces::Both, false, units, scratch);
        }
    }
    for k in world.active_effect_kind4_draws() {
        // A node's mesh holds its quads as triangle pairs, or, under the
        // tessellating bank, each quad as the sixteen `GT3`s it links.
        let tris = (k.mesh.indices.len() / 3) as u32;
        let links = if k.tessellated { tris } else { tris / 2 };
        n += links;
        // Each node's mesh is built every frame (`FUN_80028158`), a segment
        // a quad of it (the ring's), and linked as one object.
        units.builds += 1;
        units.build_segments += if k.tessellated { tris / 32 } else { tris / 2 };
        units.objects += 1;
        if k.tessellated {
            units.prims.tris_linked += links;
        } else {
            units.prims.quads_linked += links;
        }
    }
    n
}

impl crate::scene_host::SceneHost {
    /// Record the second backdrop copy the battle edge resolved for this
    /// fight's stage (`DAT_80078B50`), which the census's stage draws.
    pub fn set_battle_link_second(
        &mut self,
        second: Option<legaia_scene_formats::battle_backdrop::SecondCopy>,
    ) {
        self.battle_link_second = second;
        self.battle_link = None;
    }

    /// Seat the summon creature's census body (`slot`, its mesh), dropping
    /// whatever body the seat was counted under before.
    pub fn seat_battle_link_creature(&mut self, slot: usize, mesh: LinkMesh) {
        self.battle_link_creature = Some((slot, Arc::new(mesh)));
        if let Some(Some(b)) = self
            .battle_link
            .as_mut()
            .and_then(|g| g.bodies.get_mut(slot))
        {
            *b = (u32::MAX, Arc::default());
        }
    }

    /// Keep the census geometry current and count the frame the world now
    /// stands on, handing the total to the world
    /// (`World::note_linked_polys`). Runs every battle tick on every host;
    /// counts only while the frame cost reads it
    /// (`WorldToggles::battle_poly_frame_cost`) or `LEGAIA_DIAG_GPU_COST`
    /// logs it.
    pub fn refresh_battle_link_census(&mut self) {
        if !self.world.toggles.battle_poly_frame_cost
            && !self.world.toggles.battle_derived_frame_cost
            && !census_diag()
            && std::env::var_os("LEGAIA_DIAG_CENSUS_FATES").is_none()
        {
            return;
        }
        let serial = self.world.battle.entry_serial;
        if self.battle_link.as_ref().is_none_or(|g| g.serial != serial) {
            let stage = self.battle_stage_resources().map(|res| {
                let (tmd, raw) = &res.dome;
                let layers = self.battle_stage_layers(tmd.objects.len());
                StageLink {
                    fixed: LinkMesh::from_objects(tmd, raw, layers.fixed.iter().copied()),
                    spun: (!layers.spun.is_empty())
                        .then(|| LinkMesh::from_objects(tmd, raw, layers.spun.iter().copied())),
                    second: self
                        .battle_link_second
                        .unwrap_or(legaia_scene_formats::battle_backdrop::SecondCopy::HalfTurn),
                }
            });
            self.battle_link = Some(BattleLinkGeometry {
                serial,
                stage,
                bodies: Vec::new(),
                parts: Default::default(),
            });
        }
        // Each seat's body, built the first frame it is seated under its key.
        let n = self.world.actors.len();
        for slot in 0..n {
            let Some(key) = body_key(&self.world, slot) else {
                continue;
            };
            if self
                .battle_link
                .as_ref()
                .is_some_and(|g| g.body(slot, key).is_some())
            {
                continue;
            }
            let mesh = self.build_body_link_mesh(slot);
            let Some(g) = self.battle_link.as_mut() else {
                return;
            };
            if g.bodies.len() < n {
                g.bodies.resize(n, None);
            }
            // A seat with no buildable body keeps an empty mesh, so it is
            // not tried again every frame.
            g.bodies[slot] = Some((key, Arc::new(mesh.unwrap_or_default())));
        }
        // The library meshes the live parts and effect models draw.
        let wanted = self
            .world
            .active_summon_part_draws()
            .into_iter()
            .chain(self.world.active_move_fx_part_draws())
            .map(|d| d.model_index)
            .chain(
                self.world
                    .active_effect_models()
                    .into_iter()
                    .map(|e| e.tmd_index),
            );
        for idx in wanted {
            let Some(g) = i16::try_from(idx)
                .ok()
                .and_then(|i| self.world.global_tmd(i))
            else {
                continue;
            };
            let id = Arc::as_ptr(g) as usize;
            let Some(geom) = self.battle_link.as_mut() else {
                return;
            };
            if geom.parts.get(&idx).is_none_or(|(k, _)| *k != id) {
                geom.parts
                    .insert(idx, (id, Arc::new(LinkMesh::from_tmd(&g.tmd, &g.raw))));
            }
        }
        let Some(geom) = self.battle_link.as_ref() else {
            return;
        };
        let count = battle_link_count(&self.world, geom, &mut self.battle_link_scratch);
        self.battle_link_last = count;
        self.world.note_linked_polys(count.total());
        self.world
            .note_link_classes(legaia_engine_battle_loop::frame_clock::LinkClasses {
                stage: count.stage,
                grid: count.grid,
                bodies: count.bodies,
                ghosts: count.ghosts,
                shadows: count.shadows,
                effects: count.effects,
                sprites: count.sprites,
                trails: count.trails,
            });
        self.world.note_floor_clip_walk(count.floor_clip_walk);
        self.world.note_derived_units(count.units);
        // `LEGAIA_DIAG_CENSUS_FATES=<file>`: each count's stage and body
        // tallies (linked, off-screen, culled, near), a line a refresh, for
        // holding the census against retail's own per-fate counts
        // (`retail-exec battle-cost --mode fates`).
        if let Some(path) = std::env::var_os("LEGAIA_DIAG_CENSUS_FATES") {
            use std::io::Write as _;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let row = |t: &PrimTally| {
                    format!(
                        "{},{},{},{}",
                        t.tris_linked + t.quads_linked,
                        t.tris_off_screen + t.quads_off_screen,
                        t.tris_culled + t.quads_culled,
                        t.near_culled
                    )
                };
                let bodies: Vec<String> = self
                    .battle_link_scratch
                    .bodies
                    .iter()
                    .map(|(i, t)| format!("{i}:{}", row(t).replace(',', "/")))
                    .collect();
                let _ = writeln!(
                    f,
                    "{},{},{},{}",
                    self.world.battle_ctx.action_state,
                    row(&count.stage_tally),
                    row(&count.body_tally),
                    bodies.join(" ")
                );
            }
        }
    }

    /// A seat's body mesh: a party member's battle form, a monster's archive
    /// mesh. `None` for anything else (a summon creature).
    fn build_body_link_mesh(&mut self, slot: usize) -> Option<LinkMesh> {
        if body_key(&self.world, slot) == Some(SUMMON_BODY_KEY) {
            return self
                .battle_link_creature
                .as_ref()
                .filter(|(s, _)| *s == slot)
                .map(|(_, m)| LinkMesh::clone(m));
        }
        let a = self.world.actors.get(slot)?;
        if let Some(id) = a.battle_monster_id {
            let archive = self.monster_archive_bytes()?;
            let mesh = legaia_actor_formats::monster_archive::mesh(&archive, id).ok()??;
            let tmd = legaia_tmd::parse(mesh.tmd_bytes()).ok()?;
            return Some(LinkMesh::from_tmd(&tmd, mesh.tmd_bytes()));
        }
        let form = self
            .battle_party_forms()?
            .forms
            .iter()
            .find(|f| f.member == slot)?;
        Some(LinkMesh::from_tmd(&form.tmd, &form.tmd_bytes))
    }
}

/// `LEGAIA_DIAG_GPU_COST` set: the census runs for the log whatever the
/// frame cost reads. Read once.
fn census_diag() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("LEGAIA_DIAG_GPU_COST").is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clip transform that maps `(x, y, z)` to clip `(x, y, _, z)`: the
    /// corner's `w` is its `z`, so `SZ` reads straight off the vertex.
    const EYE: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 0.0, 1.0, //
        0.0, 0.0, 0.0, 0.0,
    ];

    fn quad_object(z: f32, half: f32, centre: [f32; 2]) -> LinkObject {
        let [cx, cy] = centre;
        LinkObject {
            // Counter-clockwise in NDC as (v0, v1, v2).
            verts: vec![
                [(cx - half) * z, (cy - half) * z, z],
                [(cx + half) * z, (cy - half) * z, z],
                [(cx - half) * z, (cy + half) * z, z],
                [(cx + half) * z, (cy + half) * z, z],
            ],
            prims: vec![LinkPrim {
                v: [0, 1, 2, 3],
                quad: true,
                kind: 19,
            }],
        }
    }

    fn count(o: &LinkObject, faces: Faces, offscreen: bool) -> u32 {
        tally_object(
            o,
            None,
            false,
            &EYE,
            faces,
            offscreen,
            &mut LinkScratch::default(),
        )
        .linked()
    }

    /// The units count the primitives the links leave out: the off-screen
    /// ones and the culled ones apart.
    #[test]
    fn the_retail_rules_follow_the_leaves() {
        let retail = || LinkScratch {
            retail_rules: true,
            ..Default::default()
        };
        let tri = |z: f32, half: f32, centre: [f32; 2]| {
            let mut o = quad_object(z, half, centre);
            o.prims[0] = LinkPrim {
                v: [0, 1, 2, 0],
                quad: false,
                kind: 18,
            };
            o
        };
        // A triangle wholly off the right edge: a body's draw tests it too.
        let past = tri(1000.0, 0.2, [1.5, 0.0]);
        let t = tally_object(&past, None, false, &EYE, Faces::Both, false, &mut retail());
        assert_eq!((t.tris_linked, t.tris_off_screen), (0, 1));
        // A kind-19 quad off screen is tested only under the word's `0x40`.
        let q = quad_object(1000.0, 0.2, [1.5, 0.0]);
        let t = tally_object(&q, None, false, &EYE, Faces::Both, false, &mut retail());
        assert_eq!((t.quads_linked, t.quads_off_screen), (1, 0));
        let t = tally_object(&q, None, false, &EYE, Faces::Both, true, &mut retail());
        assert_eq!((t.quads_linked, t.quads_off_screen), (0, 1));
        // A triangle whose corners land on one pixel has `NCLIP = 0`, which
        // the leaf keeps on either winding.
        let dot = tri(1000.0, 1e-6, [0.0, 0.0]);
        for faces in [Faces::Ccw, Faces::Cw] {
            let t = tally_object(&dot, None, false, &EYE, faces, false, &mut retail());
            assert_eq!(t.tris_linked, 1, "{faces:?}");
            let t = tally_object(
                &dot,
                None,
                false,
                &EYE,
                faces,
                false,
                &mut LinkScratch::default(),
            );
            assert_eq!(t.linked(), u32::from(faces == Faces::Ccw), "the float rule");
        }
        // Under the near floor (mean `SZ` under `64`), culled either way.
        let near = tri(40.0, 0.2, [0.0, 0.0]);
        let t = tally_object(&near, None, false, &EYE, Faces::Both, false, &mut retail());
        assert_eq!((t.tris_culled, t.near_culled), (1, 1));
    }

    #[test]
    fn the_units_tally_each_fate() {
        let past = quad_object(1000.0, 0.2, [1.5, 0.0]);
        let t = tally_object(
            &past,
            None,
            false,
            &EYE,
            Faces::Both,
            true,
            &mut LinkScratch::default(),
        );
        assert_eq!(
            (t.quads_linked, t.quads_off_screen, t.quads_culled),
            (0, 1, 0)
        );
        let q = quad_object(1000.0, 0.2, [0.0, 0.0]);
        let t = tally_object(
            &q,
            None,
            false,
            &EYE,
            Faces::Cw,
            false,
            &mut LinkScratch::default(),
        );
        assert_eq!(
            (t.quads_linked, t.quads_off_screen, t.quads_culled),
            (0, 0, 1)
        );
    }

    #[test]
    fn a_front_facing_quad_links_once() {
        let q = quad_object(1000.0, 0.2, [0.0, 0.0]);
        assert_eq!(count(&q, Faces::Both, false), 1);
        assert_eq!(count(&q, Faces::Ccw, false), 1);
        assert_eq!(count(&q, Faces::Cw, false), 0, "its back face is culled");
    }

    /// `OTZ` is the mean corner `SZ >> 2`; under the floor `0x10` the
    /// primitive is not linked.
    #[test]
    fn the_near_floor_drops_a_primitive_under_sz_64() {
        assert_eq!(
            count(&quad_object(63.0, 0.2, [0.0, 0.0]), Faces::Both, false),
            0
        );
        assert_eq!(
            count(&quad_object(64.0, 0.2, [0.0, 0.0]), Faces::Both, false),
            1
        );
        assert_eq!(
            count(&quad_object(-500.0, 0.2, [0.0, 0.0]), Faces::Both, false),
            0
        );
    }

    /// The backdrop's off-screen reject drops a primitive wholly past one
    /// edge and keeps one that straddles it.
    #[test]
    fn the_off_screen_reject_needs_every_corner_past_one_edge() {
        let past = quad_object(1000.0, 0.2, [1.5, 0.0]);
        assert_eq!(count(&past, Faces::Both, false), 1, "a body links it");
        assert_eq!(count(&past, Faces::Both, true), 0, "the backdrop does not");
        let straddle = quad_object(1000.0, 0.4, [1.0, 0.0]);
        assert_eq!(count(&straddle, Faces::Both, true), 1);
    }

    /// A bone's rotation and offset move the vertices before the transform:
    /// a half turn about Y reverses the quad's winding.
    #[test]
    fn a_bone_poses_the_object() {
        let q = quad_object(1.0, 0.2, [0.0, 0.0]);
        // Rest at z = 1, posed by a half turn about Y to z = -1, then offset
        // 1000 out: the mirrored quad faces the other way.
        let bone = ([0i16, 0, 1000], [0i16, 2048, 0]);
        let n = |faces| {
            tally_object(
                &q,
                Some(&bone),
                true,
                &EYE,
                faces,
                false,
                &mut LinkScratch::default(),
            )
            .linked()
        };
        assert_eq!(n(Faces::Both), 1);
        assert_eq!(n(Faces::Ccw), 0);
        assert_eq!(n(Faces::Cw), 1);
    }

    #[test]
    fn outside_a_battle_nothing_is_counted() {
        let w = World::new();
        let n = battle_link_count(
            &w,
            &BattleLinkGeometry::default(),
            &mut LinkScratch::default(),
        );
        assert_eq!(n, LinkCount::default());
    }
}
