//! Browser battle **effect** presentation: the browser twin of the native
//! window's per-frame FX block (`engine-shell`
//! `window/event_handler/redraw.rs` + `redraw_passes.rs` +
//! `window/geometry.rs`).
//!
//! [`crate::play_battle_render`] owns what stands *behind* the HUD (backdrop,
//! ground grid, actor meshes); this module owns what a cast, an art impact or
//! a summon puts *on top* of it. Four seams, each the same `World` API the
//! native window drains:
//!
//! * **effect-script spawns** (`BootSession::step_battle_presentation`, run
//!   from [`LegaiaRuntime::tick_battle_presentation`]):
//!   one request per effect record the per-actor effect-script walk consumed
//!   (the `FUN_801DEA50` port driven from the animation tick). The direct
//!   (`0x80`-flagged) form lands in the 2D effect pool
//!   (`World::try_spawn_effect`); the table form stages a `0x801F6324`
//!   prototype scene whose parts ride the move-VM part-draw seam
//!   (`World::spawn_action_table_effect`).
//! * **effect-pool billboards** ([`LegaiaRuntime::play_battle_fx_sync`]):
//!   camera-facing textured quads over `World::active_effect_sprites`, sized
//!   and UV-addressed from the effect bundle's inline atlas exactly like the
//!   native `effect_billboard_mesh` (retail `FUN_801E0088` pass 2). The
//!   native *diagnostic* outline has a twin here too - untextured hybrid-flat
//!   strips, because the page renderer has no Lines pipeline - behind the same
//!   default-off gate ([`FX_OUTLINE`] /
//!   [`LegaiaRuntime::set_battle_fx_outline`]).
//! * **3D FX models**: the `etmd.dat` effect meshes
//!   (`World::active_effect_models`) and the move-VM scene-graph parts
//!   (`World::active_summon_part_draws` + `active_move_fx_part_draws`), each
//!   resolved through `World::global_tmd` (the PROT 0871 effect-model
//!   library) and handed to the page as a ready model matrix.
//! * **the summon creature** ([`LegaiaRuntime::spawn_summon_creature_web`]):
//!   a player Seru-magic cast's namesake `battle_data` creature, seated in a
//!   high actor slot and drawn through the same enemy animation pipeline the
//!   monsters use - the browser twin of the native `spawn_summon_creature`.
//!
//! Everything the page receives is engine-composed: vertex arrays and 4x4
//! model matrices, never a `(position, rotation)` pair the page would have to
//! re-compose against a JS-side convention that can drift from the native
//! one.

use crate::runtime::LegaiaRuntime;
use legaia_engine_camera::psx_camera::{WORLD_FLIP, mat4_mul, mat4_scale};
use legaia_engine_mode::mode::SceneMode;
use wasm_bindgen::prelude::*;

/// Retail 4x uniform battle world scale - the shared value both hosts read
/// (`legaia_engine_session::battle_fx`), because every FX matrix folds it in
/// (the native `fx_cam = cam * scale(4.0)`).
use legaia_engine_session::battle_fx::BATTLE_WORLD_SCALE;

/// Outline strip width as a fraction of the billboard's larger side, with a
/// world-unit floor so a tiny sprite still reads. The native outline is a
/// `LineList`; the page has no line pipeline, so each edge draws as a thin
/// untextured quad through the hybrid-flat shader path.
const OUTLINE_FRAC: f32 = 0.03;
const OUTLINE_MIN: f32 = 1.0;

/// Whether the per-billboard debug outline is emitted. **Off by default**,
/// because retail draws no such rectangle.
///
/// This is the browser twin of the native window's `LEGAIA_DIAG_FX=1` gate on
/// `effect_sprite_line_geometry`. A WASM module has no process environment to
/// read, so the toggle is a module static the page (or a devtools console) can
/// flip through [`LegaiaRuntime::set_battle_fx_outline`]; the browser is
/// single-threaded, so an atomic is only for interior mutability.
///
/// # Why this needed a gate at all
///
/// The outline predates the battle-entry flame-atlas blit: it existed so a
/// spawn stayed readable when its texels were not resident. With the atlas
/// resident the textured quad draws on its own and the outline is purely in
/// the way - and it is not a faint one. Its vertices carry `flat.a = 0`, which
/// sends them down the page shader's untextured branch
/// (`site/js/webgl-shaders.js`, the `v_flat_rgba.a < 0.5` arm), and they carry
/// `cba_tsb = [0, 0]`, so `prim_semi` is clear and they rasterise in the
/// **opaque** pass at `alpha = 1.0`. The colour is R-dominant at every fade
/// (`(80 + 175f, 200f, 255f)` - dark red at the end of a sprite's animation,
/// rose in the middle), so what the page actually drew was an opaque red-ish
/// rectangle around every effect sprite in the fight.
///
/// The native window gated its twin off by default and this one was left on -
/// one edit, one host, exactly the drift shape `docs/tooling/host-drift.md`
/// describes.
static FX_OUTLINE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Flat RGBA an outline strip carries at animation fraction `age01`, matching
/// the native `effect_sprite_line_geometry` tint law. Alpha `0` is the page
/// shader's "untextured" flag, not an opacity.
fn outline_color(age01: f32) -> [u8; 4] {
    let fade = (1.0 - age01).clamp(0.0, 1.0);
    [
        (80.0 + 175.0 * fade) as u8,
        (200.0 * fade) as u8,
        (255.0 * fade) as u8,
        0,
    ]
}

/// Inward width of one outline strip for a billboard of pass-2 `size`.
fn outline_thickness(size: [f32; 2]) -> f32 {
    (size[0].max(size[1]) * OUTLINE_FRAC).max(OUTLINE_MIN)
}

/// Which mesh pool an [`FxModelDraw::tmd_index`] indexes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum FxModelSource {
    /// `World::global_tmd_pool` (PROT 0871 effect library / `etmd.dat`):
    /// the page decodes + caches the mesh per fight through the
    /// `play_battle_fx_mesh_*` accessors.
    #[default]
    GlobalPool,
    /// The scene's own TMD pack (a field move-VM stager part, native
    /// `field_stager_tmds`): the page already holds env pack slot `i` as
    /// scene mesh `i`, or uploads it on demand via `field_mesh_posed(i, 0)`.
    ScenePack,
}

/// One 3D FX draw, already composed into the page's model-matrix space.
pub(crate) struct FxModelDraw {
    /// Index into the pool [`Self::source`] names - the page's mesh cache key.
    pub tmd_index: usize,
    /// The pool `tmd_index` indexes.
    pub source: FxModelSource,
    /// `scale(4) * T(pos) * Ry * Rx * Rz * scale(1,-1,1)`, column-major. The
    /// page multiplies its battle VP by this, which reproduces the native
    /// `fx_cam * model` exactly.
    pub model: [f32; 16],
}

/// One frame's worth of built FX geometry, rebuilt by
/// [`LegaiaRuntime::play_battle_fx_sync`] and read back by the accessors.
#[derive(Default)]
pub(crate) struct BattleFxFrame {
    /// Billboard + outline vertex stream (world units, pre-scaled by the
    /// retail 4x so the page draws it under an identity model).
    pub positions: Vec<f32>,
    pub uvs: Vec<u8>,
    pub cba_tsb: Vec<u16>,
    pub indices: Vec<u32>,
    /// Per-vertex `[r, g, b, textured_flag]`: `255` on the billboard quads
    /// (sample VRAM, modulated by the retail brightness envelope), `0` on the
    /// outline strips (draw the flat colour).
    pub flat: Vec<u8>,
    pub models: Vec<FxModelDraw>,
}

/// Full 4x4 inverse (cofactor expansion), column-major. Returns `None` for a
/// singular matrix. Stands in for `glam::Mat4::inverse` - the native
/// billboard basis derivation inverts the *whole* FX camera (projection
/// included), and reproducing that exactly is what keeps the browser quads
/// facing the same way the native ones do.
fn mat4_inverse(m: &[f32; 16]) -> Option<[f32; 16]> {
    let mut inv = [0.0f32; 16];
    inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15]
        + m[9] * m[7] * m[14]
        + m[13] * m[6] * m[11]
        - m[13] * m[7] * m[10];
    inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15]
        - m[8] * m[7] * m[14]
        - m[12] * m[6] * m[11]
        + m[12] * m[7] * m[10];
    inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15]
        + m[8] * m[7] * m[13]
        + m[12] * m[5] * m[11]
        - m[12] * m[7] * m[9];
    inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14]
        - m[8] * m[6] * m[13]
        - m[12] * m[5] * m[10]
        + m[12] * m[6] * m[9];
    inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15]
        - m[9] * m[3] * m[14]
        - m[13] * m[2] * m[11]
        + m[13] * m[3] * m[10];
    inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15]
        + m[8] * m[3] * m[14]
        + m[12] * m[2] * m[11]
        - m[12] * m[3] * m[10];
    inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15]
        - m[8] * m[3] * m[13]
        - m[12] * m[1] * m[11]
        + m[12] * m[3] * m[9];
    inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14]
        + m[8] * m[2] * m[13]
        + m[12] * m[1] * m[10]
        - m[12] * m[2] * m[9];
    inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15]
        + m[5] * m[3] * m[14]
        + m[13] * m[2] * m[7]
        - m[13] * m[3] * m[6];
    inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15]
        - m[4] * m[3] * m[14]
        - m[12] * m[2] * m[7]
        + m[12] * m[3] * m[6];
    inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15]
        + m[4] * m[3] * m[13]
        + m[12] * m[1] * m[7]
        - m[12] * m[3] * m[5];
    inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14]
        - m[4] * m[2] * m[13]
        - m[12] * m[1] * m[6]
        + m[12] * m[2] * m[5];
    inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11]
        - m[5] * m[3] * m[10]
        - m[9] * m[2] * m[7]
        + m[9] * m[3] * m[6];
    inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11]
        + m[4] * m[3] * m[10]
        + m[8] * m[2] * m[7]
        - m[8] * m[3] * m[6];
    inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11]
        - m[4] * m[3] * m[9]
        - m[8] * m[1] * m[7]
        + m[8] * m[3] * m[5];
    inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10]
        + m[4] * m[2] * m[9]
        + m[8] * m[1] * m[6]
        - m[8] * m[2] * m[5];
    let det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
    if det.abs() < 1e-12 {
        return None;
    }
    let d = 1.0 / det;
    for v in inv.iter_mut() {
        *v *= d;
    }
    Some(inv)
}

/// `normalize(linear3x3(m) * v)` - the `glam::Mat4::transform_vector3` +
/// `normalize_or_zero` pair the native basis derivation uses.
fn transform_dir(m: &[f32; 16], v: [f32; 3]) -> [f32; 3] {
    let x = m[0] * v[0] + m[4] * v[1] + m[8] * v[2];
    let y = m[1] * v[0] + m[5] * v[1] + m[9] * v[2];
    let z = m[2] * v[0] + m[6] * v[1] + m[10] * v[2];
    let len = (x * x + y * y + z * z).sqrt();
    if len < 1e-9 {
        [0.0, 0.0, 0.0]
    } else {
        [x / len, y / len, z / len]
    }
}

/// Push one effect-pool billboard into the page's stream: the textured quad
/// at `corners` (already in page space), then - behind the default-off
/// [`FX_OUTLINE`] diagnostic - four thin untextured strips along its edges,
/// faded by age like the native `effect_sprite_line_geometry`. `basis` is
/// the camera's world `[right, up]`; `scale` is the factor the corners took
/// on the way to page space, which the strip inset takes too.
fn push_billboard(
    frame: &mut BattleFxFrame,
    b: &legaia_engine_world_views::effect_draws::FxBillboard,
    corners: [[f32; 3]; 4],
    basis: [[f32; 3]; 2],
    scale: f32,
) {
    // The retail pass-2 brightness envelope writes `r = g = b = brightness`
    // on the GPU packet, and the page's textured branch applies retail's
    // `texel * colour / 128`, so the envelope rides the RGB lanes verbatim
    // (`0x80` neutral; see `effect_vm::pool::pass2_brightness`).
    let v = b.sprite.brightness;
    push_quad(frame, corners, b.uv, b.cba_tsb, [v, v, v, 255]);
    if !FX_OUTLINE.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let col = outline_color(b.sprite.age01);
    let t = outline_thickness(b.sprite.size) * scale;
    let [right, up] = basis;
    let inset =
        |a: [f32; 3], dir: [f32; 3]| [a[0] + dir[0] * t, a[1] + dir[1] * t, a[2] + dir[2] * t];
    let [tl, tr, bl, br] = corners;
    let down = [-up[0], -up[1], -up[2]];
    let left = [-right[0], -right[1], -right[2]];
    // (outer_a, outer_b, inward) per edge: top, bottom, left, right.
    for (a, b2, inward) in [
        (tl, tr, down),
        (bl, br, up),
        (tl, bl, right),
        (tr, br, left),
    ] {
        push_quad(
            frame,
            [a, b2, inset(a, inward), inset(b2, inward)],
            [[0, 0]; 4],
            [0, 0],
            col,
        );
    }
}

/// One quad (TL / TR / BL / BR) into the page's billboard stream.
fn push_quad(
    frame: &mut BattleFxFrame,
    corners: [[f32; 3]; 4],
    uv: [[u8; 2]; 4],
    cba_tsb: [u16; 2],
    flat: [u8; 4],
) {
    let base = (frame.positions.len() / 3) as u32;
    for (p, uv) in corners.iter().zip(uv) {
        frame.positions.extend_from_slice(p);
        frame.uvs.extend_from_slice(&uv);
        frame.cba_tsb.extend_from_slice(&cba_tsb);
        frame.flat.extend_from_slice(&flat);
    }
    frame
        .indices
        .extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
}

/// Bake a per-frame mesh into the billboard stream through `model` (the
/// page's model list addresses library meshes by index, so a mesh only this
/// frame has rides the stream instead).
fn bake_mesh(frame: &mut BattleFxFrame, model: &[f32; 16], m: &legaia_tmd::mesh::VramMesh) {
    let base = (frame.positions.len() / 3) as u32;
    for (i, p) in m.positions.iter().enumerate() {
        let w = [
            model[0] * p[0] + model[4] * p[1] + model[8] * p[2] + model[12],
            model[1] * p[0] + model[5] * p[1] + model[9] * p[2] + model[13],
            model[2] * p[0] + model[6] * p[1] + model[10] * p[2] + model[14],
        ];
        frame.positions.extend_from_slice(&w);
        frame.uvs.extend_from_slice(&m.uvs[i]);
        frame.cba_tsb.extend_from_slice(&m.cba_tsb[i]);
        let c = m.colors[i];
        frame.flat.extend_from_slice(&[c[0], c[1], c[2], 255]);
    }
    frame.indices.extend(m.indices.iter().map(|&ix| base + ix));
}

/// The effect models and the summon / move-FX parts of `lists` into the
/// page's model list, each behind the page's `outer` frame. A part whose
/// render state is not its rest mesh (armed VDF morph lanes, a render scale,
/// a colour word: the Spirit aura's cones grow, spin and fade) bakes
/// `World::part_draw_vram_mesh` - the mesh the native part pass uploads.
fn push_fx_models(
    frame: &mut BattleFxFrame,
    world: &legaia_engine_core::world::World,
    lists: &legaia_engine_world_views::effect_draws::FxModelDraws,
    outer: &[f32; 16],
) {
    for em in &lists.effect_models {
        frame.models.push(FxModelDraw {
            tmd_index: em.index,
            source: FxModelSource::GlobalPool,
            model: mat4_mul(outer, &em.model),
        });
    }
    for p in &lists.parts {
        if world.global_tmd(p.draw.model_index as i16).is_none() {
            continue;
        }
        let model = mat4_mul(outer, &p.model);
        if !p.draw.draws_rest_mesh()
            && let Some(m) = world.part_draw_vram_mesh(&p.draw)
        {
            bake_mesh(frame, &model, &m);
            continue;
        }
        frame.models.push(FxModelDraw {
            tmd_index: p.draw.model_index,
            source: FxModelSource::GlobalPool,
            model,
        });
    }
}

impl LegaiaRuntime {
    /// The live FX camera: the page's battle view-projection with the retail
    /// 4x world scale composed on, i.e. the native `fx_cam`.
    fn fx_cam(&self, aspect: f32) -> Option<[f32; 16]> {
        let vp = self.play_battle_camera_vp(aspect);
        if vp.len() != 16 {
            return None;
        }
        let mut m = [0.0f32; 16];
        m.copy_from_slice(&vp);
        let scale: [f32; 16] = [
            BATTLE_WORLD_SCALE,
            0.0,
            0.0,
            0.0, //
            0.0,
            BATTLE_WORLD_SCALE,
            0.0,
            0.0, //
            0.0,
            0.0,
            BATTLE_WORLD_SCALE,
            0.0, //
            0.0,
            0.0,
            0.0,
            1.0,
        ];
        Some(mat4_mul(&m, &scale))
    }

    /// Rebuild [`Self::battle_fx`] for this frame at the page's viewport
    /// `aspect`. Returns the billboard vertex count (`0` = nothing to draw),
    /// so the page can skip the upload entirely on a quiet frame.
    ///
    /// The billboard basis comes from the inverse of the FX camera, exactly
    /// as the native `build_effect_billboards` derives it, so both hosts'
    /// quads face the camera that actually draws them.
    fn build_battle_fx(&mut self, aspect: f32) {
        use legaia_engine_world_views::effect_draws::{effect_billboards, effect_model_draws};
        let mut frame = BattleFxFrame::default();
        if !self.play_battle_active() {
            self.battle_fx = frame;
            return;
        }
        let Some(cam) = self.fx_cam(aspect) else {
            self.battle_fx = frame;
            return;
        };
        let Some(inv) = mat4_inverse(&cam) else {
            self.battle_fx = frame;
            return;
        };
        let right = transform_dir(&inv, [1.0, 0.0, 0.0]);
        let up = transform_dir(&inv, [0.0, 1.0, 0.0]);
        let Some(host) = self.scene_host.host() else {
            self.battle_fx = frame;
            return;
        };
        let world = &host.world;

        // --- 2D effect pool: one textured quad (+ diag outline) per child --
        // The list both hosts draw (`effect_draws::effect_billboards`): the
        // centre flipped into the battle camera's Y-up frame (a billboard has
        // no model matrix to carry the flip), the half-extents out of the 4x
        // view scale, the texel corners mirrored, the TSB semi bit set. The
        // page's VP is unscaled, so every corner takes the 4x here - the
        // battle mode's base matrix, the scale the native window takes from
        // the same kernel, stage or no stage.
        let s = legaia_engine_session::battle_fx::battle_world_scale(world);
        for b in effect_billboards(world, s) {
            let corners = b.corners(right, up).map(|c| [c[0] * s, c[1] * s, c[2] * s]);
            push_billboard(&mut frame, &b, corners, [right, up], s);
        }

        // --- 3D FX models ------------------------------------------------
        // The model list both hosts draw (`effect_draws::effect_model_draws`,
        // native frame), with the FX camera's 4x world scale folded in front
        // (the page multiplies its unscaled battle VP by these). Effect
        // models first, then the summon / move-FX parts, then the
        // draw-kind-4 nodes and the battle ground shadows - the native order.
        let part_cam =
            legaia_render_kernels::gte::PartCameraPose::from_battle(&self.battle_cam_pose());
        let lists = effect_model_draws(world, Some(&part_cam), |i| self.battle_draw_plan(i));
        let outer = mat4_scale(s);
        push_fx_models(&mut frame, world, &lists, &outer);
        for n in lists.kind4.iter().chain(&lists.shadows) {
            bake_mesh(&mut frame, &mat4_mul(&outer, &n.model), &n.mesh);
        }
        self.battle_fx = frame;
    }

    /// The FIELD twin of [`Self::build_battle_fx`]: the effect-pool
    /// billboards, the `etmd` effect models, the summon / move-FX parts, the
    /// field move-VM stager parts and the draw-kind-4 nodes, out of the same
    /// list the native redraw draws on every field frame
    /// (`effect_draws`; on the overworld only the draw-kind-4 list carries
    /// anything). The page used to simulate these (`tick_world_effects`)
    /// and draw none of them, because its only FX draw call sat inside the
    /// battle branch.
    ///
    /// `vp` is the page's field view-projection (column-major, the matrix
    /// `buildWorldOrbitVp` built for this frame), in **page space**: the
    /// retail world with Y negated, which is how every field draw on the
    /// page lands (`placementModelScaledY`'s `-sc` row). So each native
    /// model matrix takes `diag(1,-1,1)` as its LAST (outermost) factor,
    /// and each billboard centre is flipped the same way before the
    /// camera-basis corners are added. Field FX carry no world scale, so
    /// half-extents are the raw pass-2 size.
    fn build_field_fx(&mut self, vp: [f32; 16]) {
        use legaia_engine_world_views::effect_draws::{effect_billboards, effect_model_draws};
        let mut frame = BattleFxFrame::default();
        let Some(host) = self.scene_host.host() else {
            self.battle_fx = frame;
            return;
        };
        let world = &host.world;
        if world.mode == SceneMode::Battle {
            self.battle_fx = frame;
            return;
        }
        let Some(inv) = mat4_inverse(&vp) else {
            self.battle_fx = frame;
            return;
        };
        let right = transform_dir(&inv, [1.0, 0.0, 0.0]);
        let up = transform_dir(&inv, [0.0, 1.0, 0.0]);
        let flip = |p: [f32; 3]| [p[0], -p[1], p[2]];
        for b in effect_billboards(world, 1.0) {
            let corners = b.corners_at(flip(b.sprite.world_pos), right, up);
            push_billboard(&mut frame, &b, corners, [right, up], 1.0);
        }
        // The engine camera the page draws through this frame (the same
        // `resolve_field_camera` frame [`Self::play_camera_vp`] built `vp`
        // from), for the parts' `+0x52` camera-relative bits. `None` when the
        // page drew through its own orbit: nothing to undo.
        let part_cam = self
            .engine_camera
            .filter(|(m, _)| *m == vp)
            .and_then(|(_, frame)| frame.field_view())
            .map(|v| legaia_render_kernels::gte::PartCameraPose::from_field_view(&v));
        let lists = effect_model_draws(world, part_cam.as_ref(), |_| None);
        let outer = WORLD_FLIP;
        push_fx_models(&mut frame, world, &lists, &outer);
        // Field move-VM stager parts resolve against the SCENE's TMD pack
        // (`model_sel` relative to the spawn base = pack slot), not the
        // effect library - the page holds that pack as its env meshes.
        for fp in &lists.field_parts {
            frame.models.push(FxModelDraw {
                tmd_index: fp.index,
                source: FxModelSource::ScenePack,
                model: mat4_mul(&outer, &fp.model),
            });
        }
        for n in &lists.kind4 {
            bake_mesh(&mut frame, &mat4_mul(&outer, &n.model), &n.mesh);
        }
        self.battle_fx = frame;
    }

    /// Spawn a player Seru-magic cast's namesake `battle_data` creature into
    /// the live battle - the browser twin of the native window's
    /// `spawn_summon_creature`. The world half (the seat, its texture slot,
    /// idle and clip set) is the session step both hosts run
    /// (`battle_stage::seat_summon_creature`), whether or not this page holds
    /// a battle render; the render half injects the texture into the seat's
    /// slot (`battle_render_mesh`) and re-uploads the whole battle scene on
    /// the bumped generation instead of appending a single GPU mesh.
    pub(crate) fn spawn_summon_creature_web(&mut self, spell_id: u8) {
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        let Ok(archive) = host
            .index
            .entry_bytes_extended(legaia_engine_session::battle_stage::MONSTER_ARCHIVE_PROT_INDEX)
        else {
            return;
        };
        let Some(seat) = legaia_engine_session::battle_stage::seat_summon_creature(
            host,
            spell_id,
            &archive,
            self.summon_actor_slot,
        ) else {
            return;
        };
        self.summon_actor_slot = Some(seat.slot);
        let legaia_engine_session::battle_stage::SummonSeat {
            slot,
            tex_slot,
            asset,
        } = seat;
        let Some(br) = self.battle_render.as_ref() else {
            return;
        };
        let mut vram = br.vram.clone();
        let mesh = &asset.mesh;
        let Ok(tmd) = legaia_tmd::parse(mesh.tmd_bytes()) else {
            return;
        };
        let Some(vmesh) = mesh.battle_render_mesh(tex_slot, &mut vram) else {
            return;
        };
        if vmesh.indices.is_empty() {
            return;
        }
        let object_ids =
            legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, mesh.tmd_bytes()).1;
        let idle = asset.idle.clone();
        let rest_pose = idle
            .as_ref()
            .and_then(|a| a.frames.first())
            .map(|f| {
                f.iter()
                    .flat_map(|t| {
                        [
                            t.tx as i32,
                            t.ty as i32,
                            t.tz as i32,
                            t.rx as i32,
                            t.ry as i32,
                            t.rz as i32,
                        ]
                    })
                    .collect::<Vec<i32>>()
            })
            .unwrap_or_default();

        let Some(br) = self.battle_render.as_mut() else {
            return;
        };
        br.push_summon_actor(vram, tex_slot, slot, vmesh, object_ids, rest_pose);
        self.battle_render_generation = self.battle_render_generation.wrapping_add(1);
        if let Some(br) = self.battle_render.as_mut() {
            br.generation = self.battle_render_generation;
        }
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Rebuild this frame's battle FX geometry at the page's viewport
    /// `aspect` and return the billboard **vertex** count (`0` = no
    /// billboards this frame). Call once per battle frame before the
    /// `play_battle_fx_*` accessors; they read the cache this fills.
    pub fn play_battle_fx_sync(&mut self, aspect: f32) -> u32 {
        self.build_battle_fx(aspect);
        (self.battle_fx.positions.len() / 3) as u32
    }

    /// The field-frame twin of [`Self::play_battle_fx_sync`]: rebuild this
    /// frame's FIELD effect geometry (effect-pool billboards, `etmd` models,
    /// summon / move-FX parts, field stager parts) against the page's field
    /// view-projection `vp` (16 floats, column-major, page space) and return
    /// the billboard vertex count. The same `play_battle_fx_*` accessors then
    /// read the cache; a draw whose
    /// [`Self::play_battle_fx_model_is_scene_pack`] is `true` indexes the
    /// scene's env pack (already uploaded as scene mesh `tmd`) rather than
    /// the battle FX mesh cache. Empty in battle; on the overworld only the
    /// draw-kind-4 nodes (the sprite-arm quads) draw.
    pub fn play_field_fx_sync(&mut self, vp: &[f32]) -> u32 {
        if vp.len() != 16 {
            self.battle_fx = BattleFxFrame::default();
            return 0;
        }
        let mut m = [0.0f32; 16];
        m.copy_from_slice(vp);
        self.build_field_fx(m);
        (self.battle_fx.positions.len() / 3) as u32
    }

    /// Whether FX model draw `i` indexes the scene's TMD pack (a field
    /// stager part) instead of the global effect pool. See
    /// [`Self::play_field_fx_sync`].
    pub fn play_battle_fx_model_is_scene_pack(&self, i: u32) -> bool {
        self.battle_fx
            .models
            .get(i as usize)
            .is_some_and(|m| m.source == FxModelSource::ScenePack)
    }

    /// Turn the per-billboard debug outline on or off (**off** by default).
    ///
    /// The browser twin of the native window's `LEGAIA_DIAG_FX=1` gate: it
    /// stamps a flat rectangle around every effect sprite so a spawn stays
    /// readable when its texels are not resident. Retail draws no such
    /// rectangle, so normal play leaves it off; the page never has to call
    /// this, and a devtools console can flip it for one session.
    pub fn set_battle_fx_outline(&mut self, on: bool) {
        FX_OUTLINE.store(on, std::sync::atomic::Ordering::Relaxed);
    }

    /// Whether [`Self::set_battle_fx_outline`] is currently on.
    pub fn battle_fx_outline(&self) -> bool {
        FX_OUTLINE.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Billboard + outline vertex positions, `3 x f32` per vertex, already in
    /// the page's battle-VP space (the retail 4x world scale is folded in, so
    /// the page draws them under an **identity** model matrix).
    pub fn play_battle_fx_positions(&self) -> Vec<f32> {
        self.battle_fx.positions.clone()
    }

    pub fn play_battle_fx_uvs(&self) -> Vec<u8> {
        self.battle_fx.uvs.clone()
    }

    pub fn play_battle_fx_cba_tsb(&self) -> Vec<u16> {
        self.battle_fx.cba_tsb.clone()
    }

    pub fn play_battle_fx_indices(&self) -> Vec<u32> {
        self.battle_fx.indices.clone()
    }

    /// Per-vertex `[r, g, b, textured_flag]`: `255` on the textured billboard
    /// quads, `0` on the outline strips (the page's hybrid-flat shader path).
    pub fn play_battle_fx_flat_rgba(&self) -> Vec<u8> {
        self.battle_fx.flat.clone()
    }

    /// Number of 3D FX model draws this frame (`etmd` effect meshes + summon
    /// / move-FX scene-graph parts).
    pub fn play_battle_fx_model_count(&self) -> u32 {
        self.battle_fx.models.len() as u32
    }

    /// The `World::global_tmd_pool` index draw `i` resolves to - the page's
    /// mesh cache key (one upload per distinct model, reused across frames).
    pub fn play_battle_fx_model_tmd(&self, i: u32) -> u32 {
        self.battle_fx
            .models
            .get(i as usize)
            .map(|m| m.tmd_index as u32)
            .unwrap_or(0)
    }

    /// Every FX model draw's model matrix, flattened `16 x f32` per draw in
    /// draw order (column-major, WebGL `mat4` layout). The page multiplies its
    /// battle view-projection by this - the composition is engine-side so the
    /// browser cannot drift from the native `fx_cam * model`.
    pub fn play_battle_fx_model_matrices(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.battle_fx.models.len() * 16);
        for m in &self.battle_fx.models {
            out.extend_from_slice(&m.model);
        }
        out
    }

    /// Vertex stream of the FX model in `World::global_tmd_pool[tmd]`, in the
    /// page's scene-mesh shape. Fetched once per distinct model per battle.
    pub fn play_battle_fx_mesh_positions(&self, tmd: u32) -> Vec<f32> {
        self.fx_mesh(tmd)
            .map(|m| m.positions.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    pub fn play_battle_fx_mesh_uvs(&self, tmd: u32) -> Vec<u8> {
        self.fx_mesh(tmd)
            .map(|m| m.uvs.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    pub fn play_battle_fx_mesh_cba_tsb(&self, tmd: u32) -> Vec<u16> {
        self.fx_mesh(tmd)
            .map(|m| m.cba_tsb.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    pub fn play_battle_fx_mesh_indices(&self, tmd: u32) -> Vec<u32> {
        self.fx_mesh(tmd).map(|m| m.indices).unwrap_or_default()
    }

    /// Per-battle-actor target-select cursor state, flattened `6 x f32` per
    /// actor **in mesh order** (index-parallel with
    /// `play_battle_actor_transforms`): `[enable, far_r, far_g, far_b,
    /// max_ir0, model_scale]`.
    ///
    /// The ported `FUN_801DA6B4` (`target_cursor_highlight`) stamps three
    /// render words across the monster slots while the command picker points
    /// at an enemy row; this is the native redraw's *consumption* of them,
    /// resolved engine-side so both hosts saturate the same depth-cue ramp:
    /// the pointed-at monster pulses bright (`far = white`, phase from the
    /// world's own frame counter), the rest sit dim (`far = black`,
    /// `max_ir0 = 0.55`). The `model_scale` channel is retained for layout
    /// compatibility and is always `1.0`: the actor `+0xC` word is the
    /// render packet's `+0x78` tint-blend weight (`FUN_8004A908`), not a
    /// mesh scale - it weights the cue-group flash strength below instead.
    ///
    /// REF: FUN_801DA6B4
    pub fn play_battle_actor_cursor(&self) -> Vec<f32> {
        let (Some(br), Some(host)) = (self.battle_render.as_ref(), self.scene_host.host()) else {
            return Vec::new();
        };
        let frame = host.world.clock.display_frames;
        let mut out = Vec::with_capacity(br.actor_slots().len() * 6);
        for actor_idx in br.actor_slots() {
            let b = host.world.actors.get(actor_idx).map(|a| &a.battle);
            // Retail's tint pass (the native window's twin in `redraw.rs`
            // carries the instruction cites): the whole `FUN_8004A908`
            // word - the lanes as the GTE far colour, the blend or the
            // view-depth weight as the IR0, the depth-cue arm that darkens
            // (or, outdoors, brightens) a far body, the status colours - so
            // the mesh's modulation colour is pushed toward the far colour
            // and the texel still multiplies through. Resolved engine-side
            // by `World::battle_actor_draw_plan` so both hosts stage the
            // same word. `render_flag == 2` (the capture / defeat fade)
            // draws additive: its word's ABE + ABR ride the body's TSB
            // stream (`play_battle_body_blend`), so it takes its cue only
            // while that word raises ABE - un-cued it is never an opaque
            // black silhouette; its zero-lanes end is the draw gate in
            // `play_battle_actor_transforms`.
            //
            // Tint word first, else the cursor's pulse: the one precedence
            // (`BattleActorDrawPlan::body_cue`) the native draw pass stages.
            let plan = self.battle_draw_plan(actor_idx);
            match b.and_then(|b| {
                legaia_engine_core::world::BattleActorDrawPlan::body_cue(
                    plan.as_ref(),
                    b.render_flag,
                    frame,
                )
            }) {
                Some((far, max_ir0)) => {
                    out.extend_from_slice(&[1.0, far[0], far[1], far[2], max_ir0, 1.0]);
                }
                None => out.extend_from_slice(&[0.0, 0.0, 0.0, 0.0, 0.0, 1.0]),
            }
        }
        out
    }

    /// Per-battle-actor NCLIP word **in mesh order** (index-parallel with
    /// `play_battle_actor_transforms`), in `TmdRenderer.setNclipCull`'s
    /// encoding: the page's placement `nclip`, which overrides the frame's
    /// both-sided battle word for that body. Retail draws a battle body
    /// single-sided unless its colour word carries the double-sided bit
    /// (`BattleActorDrawPlan::nclip_mode`); the native window stages the
    /// same word per draw (`Renderer::set_draw_nclip`). `0` for a slot with
    /// no draw plan.
    pub fn play_battle_actor_nclip(&self) -> Vec<u32> {
        let Some(br) = self.battle_render.as_ref() else {
            return Vec::new();
        };
        br.actor_slots()
            .into_iter()
            .map(|actor_idx| {
                self.battle_draw_plan(actor_idx)
                    .map_or(0, |p| p.nclip_mode())
            })
            .collect()
    }

    /// Per battle actor (mesh order, as `play_battle_actor_transforms`), the
    /// floor clip a body the defeat fade is sinking draws under, in the
    /// object-effect clip encoding `[m0, m1, m2, lo, hi, enable]` (keep
    /// `lo <= m . p <= hi` over the posed mesh-space position): its node's
    /// `+0x42` routes it through the clipping renderer under object-effect
    /// row 0, the stage floor as battle setup seeds it. All six zero for an
    /// unclipped body. The native window stages the same kernel,
    /// `World::battle_actor_floor_clip`, on its draw.
    pub fn play_battle_actor_floor_clips(&self) -> Vec<f32> {
        let (Some(br), Some(host)) = (self.battle_render.as_ref(), self.scene_host.host()) else {
            return Vec::new();
        };
        br.actor_slots()
            .into_iter()
            .flat_map(|actor_idx| {
                host.world
                    .battle_actor_floor_clip(actor_idx)
                    .map_or([0.0; 6], |c| c.shader_floats())
            })
            .collect()
    }
}

impl LegaiaRuntime {
    /// Build the VRAM mesh for `World::global_tmd_pool[tmd]`, or `None` when
    /// the slot is empty / the battle render is down.
    fn fx_mesh(&self, tmd: u32) -> Option<legaia_tmd::mesh::VramMesh> {
        let host = self.scene_host.host()?;
        let gtmd = host.world.global_tmd(tmd as i16)?;
        Some(legaia_tmd::mesh::tmd_to_vram_mesh(&gtmd.tmd, &gtmd.raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hand-written 4x4 inverse is the one piece of linear algebra this
    /// module does not inherit from the engine, and the billboard basis is
    /// wrong in a way no screenshot names if it drifts. Pin it against the
    /// real battle projection: `battle_vp * scale(4)` inverted and
    /// re-multiplied must land on the identity.
    #[test]
    fn fx_camera_inverse_round_trips() {
        let pose = legaia_engine_battle_vm::battle_cam_script::BOOT_POSE;
        let vp = legaia_engine_camera::projection::battle_vp(&pose, BATTLE_WORLD_SCALE, 4.0 / 3.0);
        let scale: [f32; 16] = [
            4.0, 0.0, 0.0, 0.0, //
            0.0, 4.0, 0.0, 0.0, //
            0.0, 0.0, 4.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ];
        let cam = mat4_mul(&vp, &scale);
        let inv = mat4_inverse(&cam).expect("invertible");
        let id = mat4_mul(&cam, &inv);
        for (i, v) in id.iter().enumerate() {
            let want = if i % 5 == 0 { 1.0 } else { 0.0 };
            assert!(
                (v - want).abs() < 1e-3,
                "identity element {i}: {v} != {want}"
            );
        }
    }

    /// A camera-facing quad has to be *flat* and *square to the basis*: the
    /// two diagonals must cross at the sprite centre and the edges must be
    /// parallel. Cheap invariant, catches a corner-order transposition.
    #[test]
    fn billboard_corners_are_a_centred_parallelogram() {
        let sprite = legaia_engine_core::world::EffectSprite {
            world_pos: [10.0, -20.0, 30.0],
            size: [8.0, 4.0],
            uv: [0, 0],
            uv_size: [16, 16],
            page: 0,
            clut: 0,
            brightness: 0x80,
            flip_h: false,
            flip_v: false,
            age01: 0.0,
        };
        let right = [1.0, 0.0, 0.0];
        let up = [0.0, 1.0, 0.0];
        // The shared billboard (`effect_draws::FxBillboard`), sized by the
        // same view-scale kernel the list builder uses.
        let b = legaia_engine_world_views::effect_draws::FxBillboard {
            sprite,
            half: legaia_render_kernels::effect_billboard::world_half_extents(
                sprite.size,
                BATTLE_WORLD_SCALE,
            ),
            uv: legaia_engine_world_views::effect_draws::sprite_corner_uvs(&sprite),
            cba_tsb: [sprite.clut, sprite.packet_tsb()],
        };
        let [tl, tr, bl, br] = b.corners(right, up);
        for k in 0..3 {
            assert!(((tl[k] + br[k]) * 0.5 - sprite.world_pos[k]).abs() < 1e-4);
            assert!(((tr[k] + bl[k]) * 0.5 - sprite.world_pos[k]).abs() < 1e-4);
            assert!(((tr[k] - tl[k]) - (br[k] - bl[k])).abs() < 1e-4);
        }
        // The quad's WORLD extent is the pass-2 size divided by the camera's
        // own scale, because retail forms the corners in view space after the
        // camera transform (`FUN_800195A8`) - drawing them under a `4x` MVP
        // would otherwise apply that scale twice. This assertion used to read
        // `== size[0]`, which is what the oversize looked like from inside the
        // browser host.
        let (hw, hh) = legaia_render_kernels::effect_billboard::world_half_extents(
            sprite.size,
            BATTLE_WORLD_SCALE,
        );
        assert!((tr[0] - tl[0] - 2.0 * hw).abs() < 1e-4, "width = 2 * hw");
        assert!((tl[1] - bl[1] - 2.0 * hh).abs() < 1e-4, "height = 2 * hh");
        // Non-vacuity: the kernel really does shrink, so a host that skipped
        // it would fail the two assertions above rather than pass them.
        assert!(
            2.0 * hw < sprite.size[0],
            "the shared kernel must divide by the camera scale"
        );
    }

    /// Retail draws no rectangle around an effect sprite, and the native
    /// window's twin outline is off unless `LEGAIA_DIAG_FX=1`. This host had
    /// no gate at all, so every fight on the play page carried an opaque
    /// red-ish box around every billboard. The default is the whole fix.
    #[test]
    fn the_debug_outline_is_off_by_default() {
        assert!(!FX_OUTLINE.load(std::sync::atomic::Ordering::Relaxed));
    }

    /// The tint law, pinned so the "what colour was the box" question has an
    /// answer in the tree rather than in a screenshot. R dominates G and B at
    /// every fade, and the end of a sprite's animation is a dark red - which
    /// is what the box read as. Alpha is the page shader's untextured flag,
    /// not an opacity, so it stays `0` throughout.
    #[test]
    fn outline_tint_is_red_dominant_across_the_animation() {
        // age01 = 1 (animation done): the darkest, reddest end.
        assert_eq!(outline_color(1.0), [80, 0, 0, 0]);
        // age01 = 0 (just spawned): the pale end.
        assert_eq!(outline_color(0.0), [255, 200, 255, 0]);
        for step in 0..=10 {
            let c = outline_color(step as f32 / 10.0);
            assert!(c[0] >= c[1], "R >= G at age {step}");
            assert_eq!(c[3], 0, "alpha is the untextured flag at age {step}");
        }
    }

    /// The strip width has a world-unit floor, so a tiny sprite's outline does
    /// not collapse to nothing - which is why the box was visible even around
    /// the small dust puffs the Rim Elm spar fires.
    #[test]
    fn outline_thickness_has_a_floor() {
        assert_eq!(outline_thickness([4.0, 4.0]), OUTLINE_MIN);
        assert_eq!(outline_thickness([100.0, 200.0]), 200.0 * OUTLINE_FRAC);
    }
}
