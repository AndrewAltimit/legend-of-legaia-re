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
//! * **effect-script spawns** ([`LegaiaRuntime::drain_battle_effect_spawns_web`]):
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
use legaia_engine_core::world::SceneMode;
use wasm_bindgen::prelude::*;

/// Retail 4x uniform battle world scale - the same constant
/// [`crate::play_battle_render`] exports to the page, repeated here because
/// every FX matrix folds it in (the native `fx_cam = cam * scale(4.0)`).
const BATTLE_WORLD_SCALE: f32 = 4.0;

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

/// One 3D FX draw, already composed into the page's model-matrix space.
pub(crate) struct FxModelDraw {
    /// Index into `World::global_tmd_pool` - the page's mesh cache key.
    pub tmd_index: usize,
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

/// Column-major 4x4 multiply, `out = a * b` (WebGL `mat4` layout - the same
/// convention [`legaia_engine_vm::battle_cam_script::battle_vp`] emits).
fn mat_mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for c in 0..4 {
        for r in 0..4 {
            let mut s = 0.0;
            for k in 0..4 {
                s += a[k * 4 + r] * b[c * 4 + k];
            }
            out[c * 4 + r] = s;
        }
    }
    out
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

/// The four world-space corners of a camera-facing billboard, TL / TR / BL /
/// BR - the browser copy of the native `effect_sprite_corners`. The native
/// `EFFECT_TEXEL_WORLD` extra scale is `1.0` (the sprite's `size` is already
/// the retail pass-2 world size), so it does not appear here.
///
/// The half-extents come through [`legaia_engine_vm::effect_billboard`]
/// because retail forms the corners in **view** space, after the camera
/// transform (`FUN_800195A8`), so they must not take the battle base matrix's
/// `BATTLE_WORLD_SCALE` a second time - the module carries the instruction
/// evidence. Both hosts share the kernel so their effects cannot differ in
/// size.
fn sprite_corners(
    sprite: &legaia_engine_core::world::EffectSprite,
    right: [f32; 3],
    up: [f32; 3],
) -> [[f32; 3]; 4] {
    let c = sprite.world_pos;
    let (hw, hh) =
        legaia_engine_vm::effect_billboard::world_half_extents(sprite.size, BATTLE_WORLD_SCALE);
    let mut out = [[0.0f32; 3]; 4];
    // TL, TR, BL, BR: (-r +u), (+r +u), (-r -u), (+r -u).
    for (i, (sr, su)) in [(-1.0f32, 1.0f32), (1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)]
        .into_iter()
        .enumerate()
    {
        for k in 0..3 {
            out[i][k] = c[k] + right[k] * hw * sr + up[k] * hh * su;
        }
    }
    out
}

impl LegaiaRuntime {
    /// Route this tick's battle effect-script spawn requests into the world's
    /// matching spawn path - the browser twin of the native window's
    /// `drain_and_log_battle_events` FX block. Without it every magic cast,
    /// art impact and enemy special in the browser produced a spawn request
    /// that nothing consumed, so the whole effect layer was inert.
    ///
    /// REF: FUN_801DEA50
    pub(crate) fn drain_battle_effect_spawns_web(&mut self) {
        let Some(host) = self.scene_host.as_mut() else {
            return;
        };
        let world = &mut host.world;
        let spawns = world.drain_battle_effect_spawns();
        for s in &spawns {
            let at = [
                s.at.0.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                s.at.1.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                s.at.2.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            ];
            if s.direct {
                world.try_spawn_effect(s.effect, at, s.facing);
                crate::play_battle_render::web_log(&format!(
                    "play battle FX: direct effect {:#04x} actor {} at {at:?}",
                    s.effect, s.actor_slot
                ));
            } else {
                let staged = world.spawn_action_table_effect(s.effect, at);
                crate::play_battle_render::web_log(&format!(
                    "play battle FX: table effect {:#04x} actor {} at {at:?} staged={staged}",
                    s.effect, s.actor_slot
                ));
            }
        }
    }

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
        Some(mat_mul(&m, &scale))
    }

    /// Rebuild [`Self::battle_fx`] for this frame at the page's viewport
    /// `aspect`. Returns the billboard vertex count (`0` = nothing to draw),
    /// so the page can skip the upload entirely on a quiet frame.
    ///
    /// The billboard basis comes from the inverse of the FX camera, exactly
    /// as the native `build_effect_billboards` derives it, so both hosts'
    /// quads face the camera that actually draws them.
    fn build_battle_fx(&mut self, aspect: f32) {
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
        let Some(host) = self.scene_host.as_ref() else {
            self.battle_fx = frame;
            return;
        };
        let world = &host.world;

        // --- 2D effect pool: one textured quad + one outline per child ----
        let s = BATTLE_WORLD_SCALE;
        let push_vertex =
            |f: &mut BattleFxFrame, p: [f32; 3], uv: [u8; 2], ct: [u16; 2], flat: [u8; 4]| {
                f.positions
                    .extend_from_slice(&[p[0] * s, p[1] * s, p[2] * s]);
                f.uvs.extend_from_slice(&uv);
                f.cba_tsb.extend_from_slice(&ct);
                f.flat.extend_from_slice(&flat);
            };
        for sprite in world.active_effect_sprites() {
            let [u0, v0] = sprite.uv;
            let u1 = u0
                .saturating_add(sprite.uv_size[0].saturating_sub(1))
                .min(255) as u8;
            let v1 = v0
                .saturating_add(sprite.uv_size[1].saturating_sub(1))
                .min(255) as u8;
            let (mut u0, mut u1) = ((u0 & 0xFF) as u8, u1);
            let (mut v0, mut v1) = ((v0 & 0xFF) as u8, v1);
            // Random UV-mirror corner order (retail pass 2).
            if sprite.flip_h {
                std::mem::swap(&mut u0, &mut u1);
            }
            if sprite.flip_v {
                std::mem::swap(&mut v0, &mut v1);
            }
            let corners = sprite_corners(&sprite, right, up);
            let corner_uv = [[u0, v0], [u1, v0], [u0, v1], [u1, v1]];
            let ct = [sprite.clut, sprite.page];
            // The retail pass-2 brightness envelope writes `r = g = b =
            // brightness` on the GPU packet, and the page's textured branch
            // now applies retail's `texel * colour / 128`, so the envelope
            // rides the RGB lanes verbatim. Writing 255 here would both
            // discard the envelope and brighten every sprite by `255/128`.
            // `0x80` is neutral; see `effect_vm::pool::pass2_brightness`.
            let base = (frame.positions.len() / 3) as u32;
            let b = sprite.brightness;
            for (corner, uv) in corners.iter().zip(corner_uv) {
                push_vertex(&mut frame, *corner, uv, ct, [b, b, b, 255]);
            }
            frame.indices.extend_from_slice(&[
                base,
                base + 1,
                base + 2,
                base + 2,
                base + 1,
                base + 3,
            ]);

            // Outline: four thin untextured strips along the quad edges,
            // faded by age like the native `effect_sprite_line_geometry`.
            // A **diagnostic**, off unless the page asked for it - see
            // [`FX_OUTLINE`] for what leaving it on looked like on screen.
            if !FX_OUTLINE.load(std::sync::atomic::Ordering::Relaxed) {
                continue;
            }
            let col = outline_color(sprite.age01);
            let t = outline_thickness(sprite.size);
            let inset = |a: [f32; 3], dir: [f32; 3], amount: f32| {
                [
                    a[0] + dir[0] * amount,
                    a[1] + dir[1] * amount,
                    a[2] + dir[2] * amount,
                ]
            };
            let [tl, tr, bl, br] = corners;
            let down = [-up[0], -up[1], -up[2]];
            let left = [-right[0], -right[1], -right[2]];
            // (outer_a, outer_b, inward) per edge: top, bottom, left, right.
            let edges = [
                (tl, tr, down),
                (bl, br, up),
                (tl, bl, right),
                (tr, br, left),
            ];
            for (a, b, inward) in edges {
                let base = (frame.positions.len() / 3) as u32;
                for p in [a, b, inset(a, inward, t), inset(b, inward, t)] {
                    push_vertex(&mut frame, p, [0, 0], [0, 0], col);
                }
                frame.indices.extend_from_slice(&[
                    base,
                    base + 1,
                    base + 2,
                    base + 2,
                    base + 1,
                    base + 3,
                ]);
            }
        }

        // --- 3D FX models ------------------------------------------------
        // `etmd.dat` effect meshes (the model-spawn seam) followed by the
        // move-VM scene-graph parts (summon + battle move-FX), the same two
        // sources and the same order the native redraw pushes them in.
        let flip: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, //
            0.0, -1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ];
        let world_scale: [f32; 16] = [
            s, 0.0, 0.0, 0.0, //
            0.0, s, 0.0, 0.0, //
            0.0, 0.0, s, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ];
        let fx_translate = |p: [f32; 3]| -> [f32; 16] {
            [
                1.0, 0.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                p[0], p[1], p[2], 1.0,
            ]
        };
        let fx_rot_x = |a: f32| -> [f32; 16] {
            let (sa, ca) = a.sin_cos();
            [
                1.0, 0.0, 0.0, 0.0, //
                0.0, ca, sa, 0.0, //
                0.0, -sa, ca, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ]
        };
        let fx_rot_y = |a: f32| -> [f32; 16] {
            let (sa, ca) = a.sin_cos();
            [
                ca, 0.0, -sa, 0.0, //
                0.0, 1.0, 0.0, 0.0, //
                sa, 0.0, ca, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ]
        };
        let fx_rot_z = |a: f32| -> [f32; 16] {
            let (sa, ca) = a.sin_cos();
            [
                ca, sa, 0.0, 0.0, //
                -sa, ca, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ]
        };
        for em in world.active_effect_models() {
            if world.global_tmd(em.tmd_index as i16).is_none() {
                continue;
            }
            let model = mat_mul(&world_scale, &mat_mul(&fx_translate(em.world_pos), &flip));
            frame.models.push(FxModelDraw {
                tmd_index: em.tmd_index,
                model,
            });
        }
        let parts = world
            .active_summon_part_draws()
            .into_iter()
            .chain(world.active_move_fx_part_draws());
        for sp in parts {
            if world.global_tmd(sp.model_index as i16).is_none() {
                continue;
            }
            // `T * Ry * Rx * Rz * flip`, the native part composition, with
            // the FX camera's 4x world scale folded in front of it (the page
            // multiplies its unscaled battle VP by this).
            let model = mat_mul(
                &world_scale,
                &mat_mul(
                    &fx_translate(sp.world_pos),
                    &mat_mul(
                        &fx_rot_y(sp.rot[1]),
                        &mat_mul(&fx_rot_x(sp.rot[0]), &mat_mul(&fx_rot_z(sp.rot[2]), &flip)),
                    ),
                ),
            );
            frame.models.push(FxModelDraw {
                tmd_index: sp.model_index,
                model,
            });
        }
        self.battle_fx = frame;
    }

    /// Spawn a player Seru-magic cast's namesake `battle_data` creature into
    /// the live battle - the browser twin of the native window's
    /// `spawn_summon_creature`. Same resolution chain (`summon_creature_id` ->
    /// PROT 867 mesh -> `battle_render_mesh` texture injection -> a high actor
    /// slot on the party side with its idle clip installed); the one host
    /// difference is that the page re-uploads the whole battle scene on the
    /// bumped generation instead of appending a single GPU mesh.
    pub(crate) fn spawn_summon_creature_web(&mut self, spell_id: u8) {
        let Some(host) = self.scene_host.as_ref() else {
            return;
        };
        if host.world.mode != SceneMode::Battle {
            return;
        }
        let Ok(archive) = host.index.entry_bytes_extended(867) else {
            return;
        };
        let Some(creature) = legaia_engine_core::summon::summon_creature_id(spell_id, &archive)
        else {
            return;
        };
        let Some(br) = self.battle_render.as_ref() else {
            return;
        };
        let mut vram = br.vram.clone();
        let tex_slot = br.tex_slots_used.min(4);
        let mesh = match legaia_asset::monster_archive::mesh(&archive, creature) {
            Ok(Some(m)) => m,
            _ => return,
        };
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
        let idle = legaia_asset::monster_archive::idle_animation(&archive, creature)
            .ok()
            .flatten();
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

        // Seat the summon in a free high actor slot (>= 8) so it never
        // collides with the party / monster battle slots, on the party side
        // in front of the party - the native seating law.
        let party_count = host.world.party_count as usize;
        let slot = self.summon_actor_slot.unwrap_or(8 + party_count);
        self.summon_actor_slot = Some(slot);

        let Some(host) = self.scene_host.as_mut() else {
            return;
        };
        if let Some(a) = host.world.actors.get_mut(slot) {
            a.active = true;
            a.battle_tex_slot = Some(tex_slot);
            a.move_state.world_x = 0;
            a.move_state.world_y = 0;
            a.move_state.world_z = -350;
        }
        if let Some(idle) = idle.as_ref()
            && let Some(player) = legaia_engine_core::battle_anim::MonsterAnimPlayer::new(idle)
        {
            host.world.set_actor_battle_animation(slot, player);
        }

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
        use legaia_engine_vm::battle_action as ba;
        let (Some(br), Some(host)) = (self.battle_render.as_ref(), self.scene_host.as_ref()) else {
            return Vec::new();
        };
        let frame = host.world.field_frames as f32;
        let mut out = Vec::with_capacity(br.actor_slots().len() * 6);
        for actor_idx in br.actor_slots() {
            let b = host.world.actors.get(actor_idx).map(|a| &a.battle);
            // Per-clip impact tint (`FUN_8004CE2C` pass 2 via
            // `engine-vm::battle_impact_fx`): while the +0x21F selector is
            // armed the packed +0x04 word rides the same cue row the
            // cursor uses - the browser twin of the native window's arm.
            // The same arm renders the item/spirit cue-group flash
            // (`FUN_801E22C8` stamps colour + a `0x2000` blend the world
            // tick drains), strength weighted by the live blend so the
            // flash fades out.
            {
                use legaia_engine_vm::battle_impact_fx as ifx;
                if let Some(b) = b {
                    let blend_armed = b.render_flag == 0 && b.render_blend != 0;
                    if (b.impact_state != 0 || blend_armed)
                        && b.render_color != ifx::IMPACT_NEUTRAL_STATE
                        && b.render_color != 0
                    {
                        let strength = if b.impact_state != 0 {
                            ifx::IMPACT_TINT_CUE_STRENGTH
                        } else {
                            ifx::IMPACT_TINT_CUE_STRENGTH
                                * (b.render_blend.min(0x1000) as f32 / 4096.0)
                        };
                        let c = ifx::unpack_actor_state_rgb(b.render_color);
                        out.extend_from_slice(&[
                            1.0,
                            f32::from(c[0]) / 255.0,
                            f32::from(c[1]) / 255.0,
                            f32::from(c[2]) / 255.0,
                            strength,
                            1.0,
                        ]);
                        continue;
                    }
                }
            }
            match b.map(|b| b.render_flag) {
                Some(ba::CURSOR_FLAG_SELECTED) => {
                    let pulse = 0.30 + 0.20 * (frame * 0.25).sin();
                    out.extend_from_slice(&[1.0, 1.0, 1.0, 1.0, pulse, 1.0]);
                }
                Some(ba::CURSOR_FLAG_DIMMED) => {
                    out.extend_from_slice(&[1.0, 0.0, 0.0, 0.0, 0.55, 1.0]);
                }
                _ => out.extend_from_slice(&[0.0, 0.0, 0.0, 0.0, 0.0, 1.0]),
            }
        }
        out
    }
}

impl LegaiaRuntime {
    /// Build the VRAM mesh for `World::global_tmd_pool[tmd]`, or `None` when
    /// the slot is empty / the battle render is down.
    fn fx_mesh(&self, tmd: u32) -> Option<legaia_tmd::mesh::VramMesh> {
        let host = self.scene_host.as_ref()?;
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
        let pose = legaia_engine_vm::battle_cam_script::BOOT_POSE;
        let vp =
            legaia_engine_vm::battle_cam_script::battle_vp(&pose, BATTLE_WORLD_SCALE, 4.0 / 3.0);
        let scale: [f32; 16] = [
            4.0, 0.0, 0.0, 0.0, //
            0.0, 4.0, 0.0, 0.0, //
            0.0, 0.0, 4.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ];
        let cam = mat_mul(&vp, &scale);
        let inv = mat4_inverse(&cam).expect("invertible");
        let id = mat_mul(&cam, &inv);
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
        let [tl, tr, bl, br] = sprite_corners(&sprite, right, up);
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
        let (hw, hh) =
            legaia_engine_vm::effect_billboard::world_half_extents(sprite.size, BATTLE_WORLD_SCALE);
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
