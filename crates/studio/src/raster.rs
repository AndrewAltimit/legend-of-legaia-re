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
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[u8; 2]>,
    pub cba_tsb: Vec<[u16; 2]>,
    /// Texture-modulation colour (`0x80` = neutral) for textured vertices.
    pub modulation: Vec<[u8; 3]>,
    /// Per-vertex fill colour for untextured prims.
    pub fill: Vec<[u8; 3]>,
    pub textured: Vec<u8>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub center: [f32; 3],
    pub radius: f32,
}

impl Model {
    /// Build from a TMD's bytes; `None` when it doesn't parse or has no
    /// triangles.
    pub fn from_tmd(bytes: &[u8]) -> Option<Self> {
        let tmd = legaia_tmd::parse(bytes).ok()?;
        let (mesh, _obj, shading) = legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd, bytes);
        if mesh.indices.len() < 3 || mesh.positions.is_empty() {
            return None;
        }
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in &mesh.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let center = [
            (lo[0] + hi[0]) * 0.5,
            (lo[1] + hi[1]) * 0.5,
            (lo[2] + hi[2]) * 0.5,
        ];
        let radius = mesh
            .positions
            .iter()
            .map(|p| {
                let d = [p[0] - center[0], p[1] - center[1], p[2] - center[2]];
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
            })
            .fold(1.0f32, f32::max);
        let n = mesh.positions.len();
        let pad = |v: Vec<[u8; 3]>, d: [u8; 3]| {
            let mut v = v;
            v.resize(n, d);
            v
        };
        let mut textured = shading.textured;
        textured.resize(n, 1);
        Some(Self {
            modulation: pad(mesh.colors, [0x80; 3]),
            fill: pad(shading.colors, [0xC0; 3]),
            textured,
            positions: mesh.positions,
            uvs: mesh.uvs,
            cba_tsb: mesh.cba_tsb,
            normals: mesh.normals,
            indices: mesh.indices,
            center,
            radius,
        })
    }

    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }
}

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
    fn default() -> Self {
        Self {
            yaw: 0.6,
            pitch: -0.35,
            zoom: 1.0,
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
    /// A viewer aid, not a claim about retail.
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
            let n = model
                .normals
                .get(i[0])
                .copied()
                .map(view_normal)
                .unwrap_or([0.0, 0.0, -1.0]);
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
                    [0, 1, 2].map(|k| c[k] * lit * boost)
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
    fn bgr555_expands_to_full_range() {
        assert_eq!(bgr555(0x7FFF), [255, 255, 255]);
        assert_eq!(bgr555(0x001F), [255, 0, 0]);
        assert_eq!(bgr555(0x7C00), [0, 0, 255]);
    }
}
