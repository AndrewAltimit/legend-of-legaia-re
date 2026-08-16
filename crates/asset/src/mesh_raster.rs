//! A small software rasteriser for posed [`VramMesh`]es - textured, depth
//! buffered, framed to fit - for thumbnails and contact sheets where a GPU
//! context is unavailable or unwanted (the browser's one WebGL context is the
//! main viewer; a grid of forty item cards cannot each own one), and for
//! disc-gated tests that want to *look* at a cut rather than count it.
//!
//! It draws what the character shader draws: each triangle samples PSX VRAM
//! through the prim's `(cba, tsb)` (4/8-bpp CLUT indirection), texel word `0`
//! is transparent, and the packet colour modulates the texel as
//! `texel * colour / 128`. On top of that a fixed head-on light shades by
//! `|n_z|` so a flat-lit shaft still reads as round; that shading is a
//! thumbnail aid, not retail (retail applies no light source to these
//! meshes - see the project's shading policy).

use legaia_tim::Vram;
use legaia_tmd::mesh::VramMesh;

/// One object's rigid placement: `v_world = rot · v + trans`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub rot: [[f32; 3]; 3],
    pub trans: [f32; 3],
}

impl Pose {
    /// Identity placement.
    pub const IDENTITY: Pose = Pose {
        rot: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        trans: [0.0; 3],
    };

    /// From an animation keyframe: PSX angle units (`4096` = one turn),
    /// composed `Rz · Ry · Rx` about the object origin, then translated -
    /// the retail per-object GTE pipeline (`FUN_8001BE80`).
    pub fn from_keyframe(t: [i16; 3], r: [u16; 3]) -> Pose {
        let a = |u: u16| f32::from(u) * std::f32::consts::TAU / 4096.0;
        let (sx, cx) = a(r[0]).sin_cos();
        let (sy, cy) = a(r[1]).sin_cos();
        let (sz, cz) = a(r[2]).sin_cos();
        let mx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
        let my = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
        let mz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
        Pose {
            rot: mul3(mz, mul3(my, mx)),
            trans: [f32::from(t[0]), f32::from(t[1]), f32::from(t[2])],
        }
    }

    pub fn apply(&self, v: [f32; 3]) -> [f32; 3] {
        let r = apply3(self.rot, v);
        [
            r[0] + self.trans[0],
            r[1] + self.trans[1],
            r[2] + self.trans[2],
        ]
    }
}

/// Framing + look of one render.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RasterOptions {
    pub width: usize,
    pub height: usize,
    /// Orbit angles in radians: yaw about the model's vertical axis, pitch
    /// about the screen horizontal. `(0, 0)` looks straight down `+Z` at the
    /// model's front (PSX `y` is down; the image is drawn `y`-down too).
    pub yaw: f32,
    pub pitch: f32,
    /// Fraction of the shorter image side left empty around the model.
    pub margin: f32,
    /// Background RGBA (`[0, 0, 0, 0]` = transparent).
    pub background: [u8; 4],
    /// Head-on shading strength: `0.0` = the raw texel (retail), `1.0` = full
    /// `|n_z|` falloff. Thumbnails want a little.
    pub shade: f32,
    /// Re-frame the posed model on its own principal axes before the orbit
    /// angles apply: longest extent up the image, second widest across it,
    /// thinnest toward the viewer - so a sword stands blade-up and flat-on
    /// no matter how the hand holds it. What a card grid wants; a single
    /// orbit view wants it off.
    pub auto_orient: bool,
}

impl Default for RasterOptions {
    fn default() -> Self {
        RasterOptions {
            width: 128,
            height: 128,
            yaw: 35f32.to_radians(),
            pitch: -20f32.to_radians(),
            margin: 0.06,
            background: [0, 0, 0, 0],
            shade: 0.45,
            auto_orient: false,
        }
    }
}

/// Render `mesh` posed by `poses[object_ids[v]]` (objects past the pose
/// list are drawn unposed at the origin) into a `width * height` RGBA8
/// buffer. Empty when nothing projects.
pub fn render_posed(
    mesh: &VramMesh,
    object_ids: &[u32],
    poses: &[Pose],
    vram: &Vram,
    opts: &RasterOptions,
) -> Vec<u8> {
    let (w, h) = (opts.width, opts.height);
    let mut out = vec![0u8; w * h * 4];
    for px in out.chunks_exact_mut(4) {
        px.copy_from_slice(&opts.background);
    }
    if w == 0 || h == 0 || mesh.indices.len() < 3 {
        return out;
    }
    // ---- Pose + view-rotate every vertex. ----
    let (sy, cy) = opts.yaw.sin_cos();
    let (sp, cp) = opts.pitch.sin_cos();
    let my = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let mx = [[1.0, 0.0, 0.0], [0.0, cp, -sp], [0.0, sp, cp]];
    let view = mul3(mx, my);
    let mut world: Vec<[f32; 3]> = mesh
        .positions
        .iter()
        .enumerate()
        .map(|(v, p)| {
            let obj = object_ids.get(v).copied().unwrap_or(u32::MAX) as usize;
            poses.get(obj).map(|ps| ps.apply(*p)).unwrap_or(*p)
        })
        .collect();
    if opts.auto_orient
        && let Some(basis) = principal_frame(&world, &mesh.indices)
    {
        for p in &mut world {
            *p = apply3(basis, *p);
        }
    }
    let posed: Vec<[f32; 3]> = world.iter().map(|p| apply3(view, *p)).collect();
    // ---- Frame: fit the projected XY bounds into the image. ----
    let mut lo = [f32::INFINITY; 2];
    let mut hi = [f32::NEG_INFINITY; 2];
    for &i in &mesh.indices {
        let p = posed[i as usize];
        lo[0] = lo[0].min(p[0]);
        lo[1] = lo[1].min(p[1]);
        hi[0] = hi[0].max(p[0]);
        hi[1] = hi[1].max(p[1]);
    }
    if !lo[0].is_finite() {
        return out;
    }
    let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0);
    let side = w.min(h) as f32;
    let scale = side * (1.0 - 2.0 * opts.margin) / span;
    let cx = (lo[0] + hi[0]) * 0.5;
    let cyy = (lo[1] + hi[1]) * 0.5;
    let to_screen = |p: [f32; 3]| -> [f32; 3] {
        [
            (p[0] - cx) * scale + w as f32 * 0.5,
            (p[1] - cyy) * scale + h as f32 * 0.5,
            p[2],
        ]
    };
    let mut zbuf = vec![f32::INFINITY; w * h];
    let tri_count = mesh.indices.len() / 3;
    for t in 0..tri_count {
        let ia = mesh.indices[t * 3] as usize;
        let ib = mesh.indices[t * 3 + 1] as usize;
        let ic = mesh.indices[t * 3 + 2] as usize;
        let sp3 = [
            to_screen(posed[ia]),
            to_screen(posed[ib]),
            to_screen(posed[ic]),
        ];
        let e1 = [
            sp3[1][0] - sp3[0][0],
            sp3[1][1] - sp3[0][1],
            sp3[1][2] - sp3[0][2],
        ];
        let e2 = [
            sp3[2][0] - sp3[0][0],
            sp3[2][1] - sp3[0][1],
            sp3[2][2] - sp3[0][2],
        ];
        // The screen-space normal's z, for the head-on shade. Depth is in
        // model units while xy are pixels, so scale z alike.
        let n = [
            e1[1] * e2[2] * scale - e1[2] * scale * e2[1],
            e1[2] * scale * e2[0] - e1[0] * e2[2] * scale,
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        let nl = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
        let shade = 1.0 - opts.shade * (1.0 - (n[2] / nl).abs());
        let det = e1[0] * e2[1] - e2[0] * e1[1];
        if det.abs() < 1e-6 {
            continue;
        }
        let minx = sp3
            .iter()
            .map(|p| p[0])
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as usize;
        let maxx = sp3
            .iter()
            .map(|p| p[0])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(w as f32 - 1.0) as usize;
        let miny = sp3
            .iter()
            .map(|p| p[1])
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as usize;
        let maxy = sp3
            .iter()
            .map(|p| p[1])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(h as f32 - 1.0) as usize;
        if minx > maxx || miny > maxy {
            continue;
        }
        let uv = [mesh.uvs[ia], mesh.uvs[ib], mesh.uvs[ic]];
        let col = [
            mesh.colors.get(ia).copied().unwrap_or([0x80; 3]),
            mesh.colors.get(ib).copied().unwrap_or([0x80; 3]),
            mesh.colors.get(ic).copied().unwrap_or([0x80; 3]),
        ];
        let [cba, tsb] = mesh.cba_tsb[ia];
        for py in miny..=maxy {
            for px in minx..=maxx {
                let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
                // Barycentric weights of (a, b, c) at the pixel centre.
                let w1 = ((fx - sp3[0][0]) * e2[1] - (fy - sp3[0][1]) * e2[0]) / det;
                let w2 = ((fy - sp3[0][1]) * e1[0] - (fx - sp3[0][0]) * e1[1]) / det;
                let w0 = 1.0 - w1 - w2;
                if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                    continue;
                }
                let depth = w0 * sp3[0][2] + w1 * sp3[1][2] + w2 * sp3[2][2];
                let zi = py * w + px;
                if depth >= zbuf[zi] {
                    continue;
                }
                let u =
                    w0 * f32::from(uv[0][0]) + w1 * f32::from(uv[1][0]) + w2 * f32::from(uv[2][0]);
                let v =
                    w0 * f32::from(uv[0][1]) + w1 * f32::from(uv[1][1]) + w2 * f32::from(uv[2][1]);
                let word = texel_word(
                    vram,
                    cba,
                    tsb,
                    u.round().clamp(0.0, 255.0) as usize,
                    v.round().clamp(0.0, 255.0) as usize,
                );
                if word == 0 {
                    continue;
                }
                let tex = [
                    ((word & 0x1F) as f32) * (255.0 / 31.0),
                    (((word >> 5) & 0x1F) as f32) * (255.0 / 31.0),
                    (((word >> 10) & 0x1F) as f32) * (255.0 / 31.0),
                ];
                let mut rgb = [0u8; 3];
                for k in 0..3 {
                    let m = w0 * f32::from(col[0][k])
                        + w1 * f32::from(col[1][k])
                        + w2 * f32::from(col[2][k]);
                    rgb[k] = (tex[k] * m / 128.0 * shade).round().clamp(0.0, 255.0) as u8;
                }
                zbuf[zi] = depth;
                let o = zi * 4;
                out[o] = rgb[0];
                out[o + 1] = rgb[1];
                out[o + 2] = rgb[2];
                out[o + 3] = 255;
            }
        }
    }
    out
}

/// One texel word (BGR555, `0` = transparent) the way the character shader
/// samples it: page from `tsb`, 4/8-bpp CLUT indirection via `cba`. The
/// semi-transparency enable the mesh builder packs into `tsb` bit 15 is
/// ignored (it selects a blend mode, not a page).
pub fn texel_word(vram: &Vram, cba: u16, tsb: u16, u: usize, v: usize) -> u16 {
    let tpage_x = ((tsb & 0xF) as usize) * 64;
    let tpage_y = (((tsb >> 4) & 1) as usize) * 256;
    let depth = (tsb >> 7) & 3;
    let clut_x = ((cba & 0x3F) as usize) * 16;
    let clut_y = ((cba >> 6) & 0x1FF) as usize;
    match depth {
        0 => {
            let w = vram.pixel(tpage_x + (u >> 2), tpage_y + v);
            let idx = (w >> ((u & 3) * 4)) & 0xF;
            vram.pixel(clut_x + idx as usize, clut_y)
        }
        1 => {
            let w = vram.pixel(tpage_x + (u >> 1), tpage_y + v);
            let idx = (w >> ((u & 1) * 8)) & 0xFF;
            vram.pixel(clut_x + idx as usize, clut_y)
        }
        _ => vram.pixel(tpage_x + u, tpage_y + v),
    }
}

/// An RGBA8 image with its dimensions - the unit [`blit`] composes.
#[derive(Debug, Clone, PartialEq)]
pub struct Rgba<'a> {
    pub pixels: &'a [u8],
    pub width: usize,
    pub height: usize,
}

/// Composite `src` onto `dst` (`dw * dh` RGBA8) at `(x, y)`, opaque pixels
/// only. A contact-sheet helper.
pub fn blit(dst: &mut [u8], dw: usize, dh: usize, src: &Rgba<'_>, x: usize, y: usize) {
    let (sw, sh, src) = (src.width, src.height, src.pixels);
    for sy in 0..sh {
        let dy = y + sy;
        if dy >= dh {
            break;
        }
        for sx in 0..sw {
            let dx = x + sx;
            if dx >= dw {
                break;
            }
            let s = (sy * sw + sx) * 4;
            if src[s + 3] == 0 {
                continue;
            }
            let d = (dy * dw + dx) * 4;
            dst[d..d + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
}

/// The rotation that takes a vertex cloud onto its principal axes: rows are
/// (second axis, -first axis, third axis), so the longest extent runs up the
/// image (PSX `y` is down), the second across, the thinnest toward the
/// viewer. Only referenced vertices count. The first axis is signed so the
/// end farther from the centroid points up (a blade is long and sparse, a
/// hilt short and dense, so the tip is the far end). `None` for a
/// degenerate cloud.
pub fn principal_frame(points: &[[f32; 3]], indices: &[u32]) -> Option<[[f32; 3]; 3]> {
    let mut used = vec![false; points.len()];
    for &i in indices {
        if let Some(u) = used.get_mut(i as usize) {
            *u = true;
        }
    }
    let pts: Vec<[f32; 3]> = points
        .iter()
        .zip(&used)
        .filter(|(_, u)| **u)
        .map(|(p, _)| *p)
        .collect();
    if pts.len() < 3 {
        return None;
    }
    let n = pts.len() as f32;
    let mut c = [0.0f32; 3];
    for p in &pts {
        for k in 0..3 {
            c[k] += p[k] / n;
        }
    }
    let mut cov = [[0.0f32; 3]; 3];
    for p in &pts {
        let d = [p[0] - c[0], p[1] - c[1], p[2] - c[2]];
        for (i, row) in cov.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell += d[i] * d[j] / n;
            }
        }
    }
    // Power iteration for the leading eigenvector, deflate, repeat.
    let mut axes: Vec<[f32; 3]> = Vec::new();
    let mut m = cov;
    for _ in 0..2 {
        let mut v = [0.577f32, 0.577, 0.577];
        for _ in 0..64 {
            let w = apply3(m, v);
            let l = (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt();
            if l < 1e-9 {
                return None;
            }
            v = [w[0] / l, w[1] / l, w[2] / l];
        }
        let lambda = {
            let w = apply3(m, v);
            w[0] * v[0] + w[1] * v[1] + w[2] * v[2]
        };
        for (i, row) in m.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell -= lambda * v[i] * v[j];
            }
        }
        axes.push(v);
    }
    let (mut e1, e2) = (axes[0], axes[1]);
    // Sign: the far end up.
    let (mut hi, mut lo) = (f32::NEG_INFINITY, f32::INFINITY);
    for p in &pts {
        let d = (p[0] - c[0]) * e1[0] + (p[1] - c[1]) * e1[1] + (p[2] - c[2]) * e1[2];
        hi = hi.max(d);
        lo = lo.min(d);
    }
    if -lo > hi {
        e1 = [-e1[0], -e1[1], -e1[2]];
    }
    let e3 = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    Some([e2, [-e1[0], -e1[1], -e1[2]], e3])
}

fn mul3(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut o = [[0.0; 3]; 3];
    for (i, row) in o.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    o
}

fn apply3(m: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyframe_pose_rotates_about_y_and_translates() {
        // A quarter turn about Y takes +X to -Z (PSX handedness of the
        // matrix used by the site's poser: x' = x cos + z sin, z' = -x sin +
        // z cos), then the translation lands on top.
        let p = Pose::from_keyframe([10, 0, 0], [0, 1024, 0]);
        let v = p.apply([1.0, 0.0, 0.0]);
        assert!((v[0] - 10.0).abs() < 1e-4, "{v:?}");
        assert!((v[2] + 1.0).abs() < 1e-4, "{v:?}");
    }

    #[test]
    fn principal_frame_stands_the_long_axis_up_with_the_far_end_on_top() {
        // A rod along +X, dense at the -X end (the "hilt"), sparse toward +X
        // (the "tip"): the frame turns X into image-up (negative y) with the
        // tip end up.
        let mut pts: Vec<[f32; 3]> = Vec::new();
        for i in 0..40 {
            pts.push([i as f32 * 0.5, (i % 2) as f32, 0.0]);
        }
        pts.push([100.0, 0.5, 0.0]);
        pts.push([100.0, 0.0, 0.5]);
        let idx: Vec<u32> = (0..pts.len() as u32).collect();
        let f = principal_frame(&pts, &idx).unwrap();
        let tip = apply3(f, [100.0, 0.5, 0.0]);
        let hilt = apply3(f, [0.0, 0.0, 0.0]);
        assert!(tip[1] < hilt[1], "tip {tip:?} hilt {hilt:?}");
        assert!((tip[1] - hilt[1]).abs() > 90.0);
    }

    #[test]
    fn a_textured_quad_draws_and_frames_to_the_image() {
        // One 16-bit texel page: put a solid green word at (0..8, 0..8) of
        // page 0 (tsb depth 2 = direct 15-bit).
        let mut vram = Vram::new();
        let green: Vec<u8> = std::iter::repeat_n(0x03E0u16.to_le_bytes(), 64)
            .flatten()
            .collect();
        vram.write_block(0, 0, 8, 8, &green);
        let mesh = VramMesh {
            positions: vec![
                [-10.0, -10.0, 0.0],
                [10.0, -10.0, 0.0],
                [10.0, 10.0, 0.0],
                [-10.0, 10.0, 0.0],
            ],
            uvs: vec![[1, 1], [6, 1], [6, 6], [1, 6]],
            cba_tsb: vec![[0, 0x100]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            normals: vec![[0.0; 3]; 4],
            colors: vec![[0x80; 3]; 4],
        };
        let opts = RasterOptions {
            width: 32,
            height: 32,
            yaw: 0.0,
            pitch: 0.0,
            shade: 0.0,
            ..Default::default()
        };
        let img = render_posed(&mesh, &[0; 4], &[Pose::IDENTITY], &vram, &opts);
        assert_eq!(img.len(), 32 * 32 * 4);
        let centre = (16 * 32 + 16) * 4;
        assert_eq!(&img[centre..centre + 4], &[0, 255, 0, 255]);
        // Margin stays background.
        assert_eq!(img[3], 0);
        let opaque = img.chunks_exact(4).filter(|p| p[3] == 255).count();
        // ~88% of the side is covered: (32 * 0.88)^2 ≈ 793 pixels.
        assert!((700..=900).contains(&opaque), "{opaque}");
    }
}
