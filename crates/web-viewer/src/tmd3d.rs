//! 3D rendering for Legaia TMDs via Canvas 2D.
//!
//! All math runs in Rust. We transform + project vertices, cull back-faces,
//! sort triangles back-to-front (painter's algorithm), and emit a flat list
//! the JS side blits via `<canvas>.fillStyle = rgb(b,b,b); path; fill()`.
//!
//! Output format per visible triangle: `[x0, y0, x1, y1, x2, y2, brightness]`.
//! Brightness is `0.0..1.0` from a single directional light dot product.

use legaia_tmd::Tmd;
use legaia_tmd::mesh::{Mesh, tmd_to_mesh};

/// Triangle output: 7 f32s - three (x, y) screen-space pairs + 1 brightness.
const FLOATS_PER_TRI: usize = 7;

pub struct PreparedMesh {
    mesh: Mesh,
    /// Center of the AABB; used to translate the model to origin before rotation.
    center: [f32; 3],
    /// Half-extent of the bounding sphere; used to fit the model into the
    /// viewport regardless of TMD-native scale.
    radius: f32,
}

pub fn prepare(tmd: &Tmd, buf: &[u8]) -> Option<PreparedMesh> {
    let mesh = tmd_to_mesh(tmd, buf);
    if mesh.indices.is_empty() {
        return None;
    }
    let (lo, hi) = mesh.aabb();
    let center = [
        (lo[0] + hi[0]) * 0.5,
        (lo[1] + hi[1]) * 0.5,
        (lo[2] + hi[2]) * 0.5,
    ];
    // Bounding-sphere radius: max corner distance from center.
    let dx = (hi[0] - lo[0]) * 0.5;
    let dy = (hi[1] - lo[1]) * 0.5;
    let dz = (hi[2] - lo[2]) * 0.5;
    let radius = (dx * dx + dy * dy + dz * dz).sqrt();
    if radius < 1.0 {
        return None;
    }
    Some(PreparedMesh {
        mesh,
        center,
        radius,
    })
}

impl PreparedMesh {
    pub fn triangle_count(&self) -> usize {
        self.mesh.triangle_count()
    }
    pub fn vertex_count(&self) -> usize {
        self.mesh.vertex_count()
    }
}

/// Render the prepared mesh at rotation `(yaw, pitch)` (radians) into a flat
/// `Vec<f32>` of triangle data for Canvas 2D consumption. `viewport_w/h` is
/// the canvas size in pixels.
#[allow(clippy::too_many_arguments)]
pub fn render(
    pm: &PreparedMesh,
    yaw: f32,
    pitch: f32,
    distance: f32,
    pan_x: f32,
    pan_y: f32,
    viewport_w: f32,
    viewport_h: f32,
) -> Vec<f32> {
    // Camera setup: looking down -Z, with the model centered at origin and
    // pushed back so its bounding sphere fits the viewport. `distance` is in
    // unit-radius units (default 2.5 = camera 2.5 model-radii away). At
    // distance < 1.0 the camera is inside the bounding sphere - that's a
    // "fly through" view, intentional. `pan` is a view-space translation in
    // unit-radius units.
    let model_scale = 1.0 / pm.radius;
    // Clamp only above zero so matrix math doesn't divide by 0 when fully
    // zoomed in.
    let camera_z: f32 = distance.max(0.001);
    let fov: f32 = 1.2; // radians (~70°)
    let focal = (viewport_h * 0.5) / (fov * 0.5).tan();

    let cy = yaw.cos();
    let sy = yaw.sin();
    let cp = pitch.cos();
    let sp = pitch.sin();

    // Combined rotation: Yaw (Y axis) then Pitch (X axis). Matrix rows are
    // (right, up, forward) basis vectors:
    //   right   = [ cy,        0,        sy        ]
    //   up      = [ sy*sp,     cp,      -cy*sp     ]
    //   forward = [-sy*cp,     sp,       cy*cp     ]
    // Apply to model-space vertex (x_m, y_m, z_m) → camera-space
    //   x_c = right · v
    //   y_c = up · v
    //   z_c = forward · v + camera_z
    let mut cam: Vec<[f32; 3]> = Vec::with_capacity(pm.vertex_count());
    for p in &pm.mesh.positions {
        let x = (p[0] - pm.center[0]) * model_scale;
        let y = (p[1] - pm.center[1]) * model_scale;
        let z = (p[2] - pm.center[2]) * model_scale;

        let x_c = cy * x + 0.0 * y + sy * z + pan_x;
        let y_c = sy * sp * x + cp * y + (-cy * sp) * z + pan_y;
        let z_c = -sy * cp * x + sp * y + cy * cp * z + camera_z;

        cam.push([x_c, y_c, z_c]);
    }

    // Project to screen + accumulate per-tri data.
    let cx = viewport_w * 0.5;
    let cy_screen = viewport_h * 0.5;

    let n_tris = pm.mesh.indices.len() / 3;
    let mut tris: Vec<TriData> = Vec::with_capacity(n_tris);

    // Light direction (in camera space). Coming from +X +Y -Z (over-shoulder).
    let light = normalize([0.5, 0.7, -0.5]);

    let indices = &pm.mesh.indices;
    for i in 0..n_tris {
        let i0 = indices[i * 3] as usize;
        let i1 = indices[i * 3 + 1] as usize;
        let i2 = indices[i * 3 + 2] as usize;
        let v0 = cam[i0];
        let v1 = cam[i1];
        let v2 = cam[i2];

        // Reject triangles partly or fully behind the near plane. Tiny near
        // plane so fly-through views (distance < 1.0) still render the model
        // walls instead of clipping out everything close to the camera.
        let near = 0.001;
        if v0[2] < near || v1[2] < near || v2[2] < near {
            continue;
        }

        // Project (right-handed: looking down +Z away from camera, so flip Y).
        let p0 = project(v0, focal, cx, cy_screen);
        let p1 = project(v1, focal, cx, cy_screen);
        let p2 = project(v2, focal, cx, cy_screen);

        // Face normal (camera-space) for back-face cull + flat shading.
        let e1 = sub(v1, v0);
        let e2 = sub(v2, v0);
        let normal = cross(e1, e2);
        // Back-face cull: if normal points away from camera (+Z), skip.
        // (Camera looks down +Z; visible faces have normal · viewDir < 0
        // where viewDir from triangle to camera = -v0. Equivalently:
        // normal · v0 < 0 because we want triangles facing the camera.)
        // Some Legaia TMDs have inconsistent winding; render both sides.
        // For shading we flip the normal so brightness is always positive.
        let n = normalize(normal);
        let dot = n[0] * light[0] + n[1] * light[1] + n[2] * light[2];
        let brightness = (dot.abs() * 0.7 + 0.25).clamp(0.0, 1.0);

        // Average Z for painter's-algorithm depth sort.
        let avg_z = (v0[2] + v1[2] + v2[2]) / 3.0;
        tris.push(TriData {
            avg_z,
            x: [p0.0, p1.0, p2.0],
            y: [p0.1, p1.1, p2.1],
            brightness,
        });
    }

    // Painter's: sort back-to-front (largest avg_z first).
    tris.sort_by(|a, b| {
        b.avg_z
            .partial_cmp(&a.avg_z)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut out = Vec::with_capacity(tris.len() * FLOATS_PER_TRI);
    for t in tris {
        out.push(t.x[0]);
        out.push(t.y[0]);
        out.push(t.x[1]);
        out.push(t.y[1]);
        out.push(t.x[2]);
        out.push(t.y[2]);
        out.push(t.brightness);
    }
    out
}

struct TriData {
    avg_z: f32,
    x: [f32; 3],
    y: [f32; 3],
    brightness: f32,
}

fn project(v: [f32; 3], focal: f32, cx: f32, cy: f32) -> (f32, f32) {
    let inv_z = 1.0 / v[2];
    let sx = v[0] * focal * inv_z + cx;
    // Flip Y so "up" in TMD space becomes "up" on screen.
    let sy = -v[1] * focal * inv_z + cy;
    (sx, sy)
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

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let m = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if m < 1e-6 {
        return [0.0, 0.0, 1.0];
    }
    [v[0] / m, v[1] / m, v[2] / m]
}
