//! A small CPU rasteriser for the TMD preview: orbit camera, z-buffer,
//! textured triangles sampled straight out of a PSX VRAM image.
//!
//! Studio runs on egui's OpenGL backend, so it cannot reuse the wgpu
//! renderer; a preview at panel resolution is cheap enough on the CPU. The
//! mesh comes from `legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid`, and the
//! texel lookup is the page / CLUT addressing the site viewer's shader (and
//! `legaia_actor_formats::scene_gltf`) uses. UVs interpolate affinely, as the
//! PSX GPU does.

use legaia_tim::Vram;

/// A triangulated TMD ready to draw.
pub struct Model {
    /// Drawn positions: the rest pose, or the last [`Model::pose`].
    pub positions: Vec<[f32; 3]>,
    /// TMD-local rest positions.
    pub rest: Vec<[f32; 3]>,
    /// The TMD object (= animation part) each vertex belongs to.
    pub object_ids: Vec<u32>,
    pub uvs: Vec<[u8; 2]>,
    pub cba_tsb: Vec<[u16; 2]>,
    /// Texture-modulation colour (`0x80` = neutral) for textured vertices.
    pub modulation: Vec<[u8; 3]>,
    /// Per-vertex fill colour for untextured prims.
    pub fill: Vec<[u8; 3]>,
    pub textured: Vec<u8>,
    pub indices: Vec<u32>,
    pub center: [f32; 3],
    pub radius: f32,
}

impl Model {
    /// Build from a TMD's bytes; `None` when it doesn't parse or has no
    /// triangles.
    pub fn from_tmd(bytes: &[u8]) -> Option<Self> {
        let tmd = legaia_tmd::parse(bytes).ok()?;
        let (mesh, obj, shading) = legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd, bytes);
        Self::from_mesh(mesh, obj, Some(shading))
    }

    /// Build from a VRAM mesh the actor decoders produced (every prim
    /// textured unless `shading` says otherwise).
    pub fn from_mesh(
        mesh: legaia_tmd::mesh::VramMesh,
        object_ids: Vec<u32>,
        shading: Option<legaia_tmd::mesh::VertexShading>,
    ) -> Option<Self> {
        if mesh.indices.len() < 3 || mesh.positions.is_empty() {
            return None;
        }
        let n = mesh.positions.len();
        let pad = |v: Vec<[u8; 3]>, d: [u8; 3]| {
            let mut v = v;
            v.resize(n, d);
            v
        };
        let (fill, mut textured) = match shading {
            Some(s) => (s.colors, s.textured),
            None => (Vec::new(), vec![1; n]),
        };
        textured.resize(n, 1);
        let mut object_ids = object_ids;
        object_ids.resize(n, 0);
        let mut m = Self {
            modulation: pad(mesh.colors, [0x80; 3]),
            fill: pad(fill, [0xC0; 3]),
            textured,
            rest: mesh.positions.clone(),
            positions: mesh.positions,
            object_ids,
            uvs: mesh.uvs,
            cba_tsb: mesh.cba_tsb,
            indices: mesh.indices,
            center: [0.0; 3],
            radius: 1.0,
        };
        m.refit();
        Some(m)
    }

    /// Re-centre the camera target on the current positions.
    pub fn refit(&mut self) {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in &self.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let c = [
            (lo[0] + hi[0]) * 0.5,
            (lo[1] + hi[1]) * 0.5,
            (lo[2] + hi[2]) * 0.5,
        ];
        self.center = c;
        self.radius = self
            .positions
            .iter()
            .map(|p| {
                let d = [p[0] - c[0], p[1] - c[1], p[2] - c[2]];
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
            })
            .fold(1.0f32, f32::max);
    }

    /// Pose every object by its part's rigid transform: the PSX
    /// `Rz * Ry * Rx` rotation about the object origin, then the
    /// translation - the per-object assembly `FUN_8004998C` applies and
    /// `legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot` mirrors. Objects past
    /// the end of `parts` stay at rest.
    pub fn pose(&mut self, parts: &[legaia_actor_formats::monster_archive::PartPose]) {
        const A2R: f32 = std::f32::consts::TAU / 4096.0;
        let trig: Vec<([f32; 3], [f32; 6])> = parts
            .iter()
            .map(|p| {
                let (sx, cx) = (p.rx as f32 * A2R).sin_cos();
                let (sy, cy) = (p.ry as f32 * A2R).sin_cos();
                let (sz, cz) = (p.rz as f32 * A2R).sin_cos();
                (
                    [p.tx as f32, p.ty as f32, p.tz as f32],
                    [cx, sx, cy, sy, cz, sz],
                )
            })
            .collect();
        for (i, v) in self.rest.iter().enumerate() {
            let o = self.object_ids.get(i).copied().unwrap_or(0) as usize;
            self.positions[i] = match trig.get(o) {
                Some((t, [cx, sx, cy, sy, cz, sz])) => {
                    let r = legaia_tmd::mesh::rot_zyx(*v, *cx, *sx, *cy, *sy, *cz, *sz);
                    [r[0] + t[0], r[1] + t[1], r[2] + t[2]]
                }
                None => *v,
            };
        }
    }

    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    /// How many textured triangles `vram` can draw: their CLUT entries and
    /// the texels under their UV box are populated
    /// (`legaia_tim::Vram::prim_texture_status`, the test the asset viewer
    /// drops prims by). `(covered, textured)`.
    pub fn texture_coverage(&self, vram: &legaia_tim::Vram) -> (usize, usize) {
        let mut covered = 0;
        let mut textured = 0;
        for tri in self.indices.as_chunks::<3>().0 {
            let v = tri.map(|i| i as usize);
            if v.iter().any(|&i| self.textured.get(i) != Some(&1)) {
                continue;
            }
            textured += 1;
            let [cba, tsb] = self.cba_tsb[v[0]];
            let uvs = v.map(|i| (self.uvs[i][0], self.uvs[i][1]));
            if vram.prim_texture_status(cba, tsb, &uvs).ok() {
                covered += 1;
            }
        }
        (covered, textured)
    }

    /// Share of the mesh's surface (`0.0..=1.0`, by rest-pose triangle
    /// area) whose baked colour words are all near black - the modulation
    /// of a textured prim, the fill of an untextured one, every channel of
    /// every corner below [`DARK_BAKED_CHANNEL`]. Retail multiplies the
    /// texel by that word (`texel * colour / 128`), so such a triangle draws
    /// black however its texture resolves: the darkness is the disc's, not a
    /// missed page. The dark-sky shell of `town01` (PROT 6) is the shape this
    /// reads: its sky dome is untextured gouraud authored `(0, 0, 0)` at
    /// every corner - few triangles, most of the surface.
    pub fn dark_baked_share(&self) -> f32 {
        let (mut dark, mut total) = (0.0f64, 0.0f64);
        for tri in self.indices.as_chunks::<3>().0 {
            let v = tri.map(|i| i as usize);
            let Some(p) = v
                .iter()
                .map(|&i| self.rest.get(i).copied())
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let a = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
            let b = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
            let n = [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ];
            let area = f64::from((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt());
            total += area;
            let black = v.iter().all(|&i| {
                let c = if self.textured.get(i) == Some(&1) {
                    self.modulation.get(i)
                } else {
                    self.fill.get(i)
                };
                c.is_some_and(|c| c.iter().all(|&k| k < DARK_BAKED_CHANNEL))
            });
            if black {
                dark += area;
            }
        }
        if total > 0.0 {
            (dark / total) as f32
        } else {
            0.0
        }
    }
}

/// A baked colour channel below this reads as black once the texel is
/// scaled by it (`16 / 128` of the texel at most).
pub const DARK_BAKED_CHANNEL: u8 = 16;

/// A [`Model::dark_baked_share`] at or above this marks a mesh that is
/// mostly black on the disc, which the model tab says in words. Black
/// untextured prims (door voids, shadow blobs) are common in scene meshes,
/// so the bar is "most of the surface", not "some of it".
pub const MOSTLY_DARK_SHARE: f32 = 0.5;

/// The lowest colour [`Shading::Lit`] shades a prim from, per channel, so a
/// prim baked near black still shows its shape under the viewer light.
const LIT_FLOOR: f32 = 72.0;

/// Orbit camera around the model's bounding-sphere centre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Radians about the vertical axis.
    pub yaw: f32,
    /// Radians up/down, clamped to just short of the poles.
    pub pitch: f32,
    /// Distance multiplier (1 = the sphere fills the view).
    pub zoom: f32,
}

impl Default for Camera {
    /// A three-quarter view of the model's front with some margin.
    /// Character and monster meshes face `+Z` and the camera sits on the
    /// `-Z` side at yaw 0, so the half turn puts the face toward the viewer.
    fn default() -> Self {
        Self {
            yaw: std::f32::consts::PI + 0.6,
            pitch: -0.35,
            zoom: 1.3,
        }
    }
}

/// How pixels get their colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shading {
    /// Retail: `texel * colour / 128` for textured prims, the baked fill
    /// for untextured ones; no light source.
    Textured,
    /// No textures: every prim in its colour under a fixed viewer light,
    /// so geometry reads when the entry carries no textures of its own.
    /// Colours are floored at a mid-dark grey so a prim baked black (a
    /// dark-sky shell's dome) still shows its shape. A viewer aid, not a
    /// claim about retail.
    Lit,
}

fn bgr555(c: u16) -> [u8; 3] {
    let e = |v: u16| ((v & 0x1F) as u32 * 255 / 31) as u8;
    [e(c), e(c >> 5), e(c >> 10)]
}

/// The 16-bit VRAM word a textured prim samples at page-local `(u, v)`.
fn texel(vram: &Vram, cba: u16, tsb: u16, u: usize, v: usize) -> u16 {
    let tpage_x = ((tsb & 0xF) as usize) * 64;
    let tpage_y = (((tsb >> 4) & 1) as usize) * 256;
    let clut_x = ((cba & 0x3F) as usize) * 16;
    let clut_y = ((cba >> 6) & 0x1FF) as usize;
    let (u, v) = (u & 0xFF, v & 0xFF);
    match (tsb >> 7) & 3 {
        0 => {
            let w = vram.pixel((tpage_x + (u >> 2)) & 1023, (tpage_y + v) & 511);
            let idx = (w >> ((u & 3) * 4)) & 0xF;
            vram.pixel((clut_x + idx as usize) & 1023, clut_y & 511)
        }
        1 => {
            let w = vram.pixel((tpage_x + (u >> 1)) & 1023, (tpage_y + v) & 511);
            let idx = (w >> ((u & 1) * 8)) & 0xFF;
            vram.pixel((clut_x + idx as usize) & 1023, clut_y & 511)
        }
        _ => vram.pixel((tpage_x + u) & 1023, (tpage_y + v) & 511),
    }
}

/// Render `model` into a `w * h` RGBA8 buffer.
pub fn render(
    model: &Model,
    vram: &Vram,
    cam: Camera,
    shading: Shading,
    w: usize,
    h: usize,
) -> Vec<u8> {
    let mut rgba = vec![0u8; w * h * 4];
    for px in rgba.as_chunks_mut::<4>().0 {
        px.copy_from_slice(&[30, 32, 36, 255]);
    }
    if w == 0 || h == 0 {
        return rgba;
    }
    let mut depth = vec![f32::INFINITY; w * h];

    let (sy, cy) = cam.yaw.sin_cos();
    let (sp, cp) = cam.pitch.sin_cos();
    let fov = 45f32.to_radians();
    let f = (h as f32 * 0.5) / (fov * 0.5).tan();
    let dist = model.radius / (fov * 0.5).sin() * cam.zoom.max(0.05);
    let near = model.radius * 0.01;

    let project = |p: [f32; 3]| -> [f32; 3] {
        let x = p[0] - model.center[0];
        let y = p[1] - model.center[1];
        let z = p[2] - model.center[2];
        // Yaw about Y, then pitch about X.
        let x1 = x * cy + z * sy;
        let z1 = -x * sy + z * cy;
        let y2 = y * cp - z1 * sp;
        let z2 = y * sp + z1 * cp + dist;
        [
            w as f32 * 0.5 + f * x1 / z2,
            h as f32 * 0.5 + f * y2 / z2,
            z2,
        ]
    };
    let screen: Vec<[f32; 3]> = model.positions.iter().map(|&p| project(p)).collect();
    // Viewer light, from the upper left in view space.
    let light = {
        let l = [-0.4f32, -0.7, -0.6];
        let n = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
        [l[0] / n, l[1] / n, l[2] / n]
    };
    let view_normal = |n: [f32; 3]| -> [f32; 3] {
        let x1 = n[0] * cy + n[2] * sy;
        let z1 = -n[0] * sy + n[2] * cy;
        [x1, n[1] * cp - z1 * sp, n[1] * sp + z1 * cp]
    };

    for tri in model.indices.as_chunks::<3>().0 {
        let i = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
        if i.iter().any(|&k| k >= screen.len()) {
            continue;
        }
        let s = [screen[i[0]], screen[i[1]], screen[i[2]]];
        if s.iter().any(|p| p[2] < near) {
            continue;
        }
        let area =
            (s[1][0] - s[0][0]) * (s[2][1] - s[0][1]) - (s[2][0] - s[0][0]) * (s[1][1] - s[0][1]);
        if area.abs() < 1e-6 {
            continue;
        }
        let min_x = s
            .iter()
            .map(|p| p[0])
            .fold(f32::MAX, f32::min)
            .floor()
            .max(0.0) as usize;
        let max_x = s
            .iter()
            .map(|p| p[0])
            .fold(f32::MIN, f32::max)
            .ceil()
            .min(w as f32 - 1.0);
        let min_y = s
            .iter()
            .map(|p| p[1])
            .fold(f32::MAX, f32::min)
            .floor()
            .max(0.0) as usize;
        let max_y = s
            .iter()
            .map(|p| p[1])
            .fold(f32::MIN, f32::max)
            .ceil()
            .min(h as f32 - 1.0);
        if max_x < 0.0 || max_y < 0.0 {
            continue;
        }
        let (max_x, max_y) = (max_x as usize, max_y as usize);

        let tex =
            model.textured.get(i[0]).copied().unwrap_or(0) != 0 && shading == Shading::Textured;
        let lit = if shading == Shading::Lit {
            // Face normal of the drawn (posed) triangle, in view space.
            let p = [
                model.positions[i[0]],
                model.positions[i[1]],
                model.positions[i[2]],
            ];
            let a = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
            let b = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
            let n = [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
            let n = view_normal([n[0] / len, n[1] / len, n[2] / len]);
            let d = (n[0] * light[0] + n[1] * light[1] + n[2] * light[2]).abs();
            0.35 + 0.65 * d
        } else {
            1.0
        };
        let [cba, tsb] = model.cba_tsb.get(i[0]).copied().unwrap_or([0, 0]);
        // Lit mode: a textured prim has no fill of its own, so it shows its
        // modulation colour (neutral 0x80, lifted to read as mid grey).
        let (lit_src, boost) = if model.textured.get(i[0]).copied().unwrap_or(0) != 0 {
            (&model.modulation, 1.5)
        } else {
            (&model.fill, 1.0)
        };

        for y in min_y..=max_y {
            let py = y as f32 + 0.5;
            for x in min_x..=max_x {
                let px = x as f32 + 0.5;
                let w0 = ((s[1][0] - px) * (s[2][1] - py) - (s[2][0] - px) * (s[1][1] - py)) / area;
                let w1 = ((s[2][0] - px) * (s[0][1] - py) - (s[0][0] - px) * (s[2][1] - py)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * s[0][2] + w1 * s[1][2] + w2 * s[2][2];
                let di = y * w + x;
                if z >= depth[di] {
                    continue;
                }
                let lerp3 = |c: &[[u8; 3]]| -> [f32; 3] {
                    let a = c.get(i[0]).copied().unwrap_or([128; 3]);
                    let b = c.get(i[1]).copied().unwrap_or([128; 3]);
                    let d = c.get(i[2]).copied().unwrap_or([128; 3]);
                    [0, 1, 2].map(|k| w0 * a[k] as f32 + w1 * b[k] as f32 + w2 * d[k] as f32)
                };
                let rgb: [f32; 3] = if tex {
                    let uv = |k: usize| model.uvs.get(i[k]).copied().unwrap_or([0, 0]);
                    let u = w0 * uv(0)[0] as f32 + w1 * uv(1)[0] as f32 + w2 * uv(2)[0] as f32;
                    let v = w0 * uv(0)[1] as f32 + w1 * uv(1)[1] as f32 + w2 * uv(2)[1] as f32;
                    let t = texel(vram, cba, tsb & 0x1FF, u as usize, v as usize);
                    if t == 0 {
                        // PSX transparent texel.
                        continue;
                    }
                    let c = bgr555(t);
                    let m = lerp3(&model.modulation);
                    [0, 1, 2].map(|k| c[k] as f32 * m[k] / 128.0)
                } else if shading == Shading::Lit {
                    let c = lerp3(lit_src);
                    [0, 1, 2].map(|k| (c[k] * boost).max(LIT_FLOOR) * lit)
                } else {
                    lerp3(&model.fill)
                };
                depth[di] = z;
                let o = di * 4;
                rgba[o] = rgb[0].clamp(0.0, 255.0) as u8;
                rgba[o + 1] = rgb[1].clamp(0.0, 255.0) as u8;
                rgba[o + 2] = rgb[2].clamp(0.0, 255.0) as u8;
                rgba[o + 3] = 255;
            }
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_model_input_is_rejected() {
        assert!(Model::from_tmd(&[]).is_none());
        assert!(Model::from_tmd(&[0u8; 64]).is_none());
    }

    #[test]
    fn texture_coverage_reads_the_clut_and_the_page_under_the_uvs() {
        // One 4bpp triangle: CLUT at (0, 480), texture page 5 (x 320),
        // UVs 0..=15 (4 VRAM words wide, 16 rows).
        let cba = 480 << 6;
        let tsb = 5;
        let m = Model {
            positions: vec![[0.0; 3]; 3],
            rest: vec![[0.0; 3]; 3],
            object_ids: vec![0; 3],
            uvs: vec![[0, 0], [15, 0], [0, 15]],
            cba_tsb: vec![[cba, tsb]; 3],
            modulation: vec![[0x80; 3]; 3],
            fill: vec![[0; 3]; 3],
            textured: vec![1; 3],
            indices: vec![0, 1, 2],
            center: [0.0; 3],
            radius: 1.0,
        };
        let mut vram = legaia_tim::Vram::new();
        assert_eq!(m.texture_coverage(&vram), (0, 1), "nothing uploaded");
        vram.write_clut_row(0, 480, &[0x11; 32]);
        assert_eq!(m.texture_coverage(&vram), (0, 1), "palette, no texels");
        vram.write_block(320, 0, 4, 16, &[0x22; 4 * 2 * 16]);
        assert_eq!(m.texture_coverage(&vram), (1, 1));
        // An untextured triangle is not counted either way.
        let flat = Model {
            textured: vec![0; 3],
            ..m
        };
        assert_eq!(flat.texture_coverage(&legaia_tim::Vram::new()), (0, 0));
    }

    #[test]
    fn bgr555_expands_to_full_range() {
        assert_eq!(bgr555(0x7FFF), [255, 255, 255]);
        assert_eq!(bgr555(0x001F), [255, 0, 0]);
        assert_eq!(bgr555(0x7C00), [0, 0, 255]);
    }
}
