//! CPU rasterizer scaffold - small enough to use as a regression target
//! against captured retail GTE traces without dragging in wgpu. Not
//! production-grade: it's a validation tool, not a renderer replacement.

use super::*;

/// Bounding box of a triangle in pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BBox {
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
}

impl BBox {
    pub fn from_triangle(a: ScreenXY, b: ScreenXY, c: ScreenXY) -> Self {
        Self {
            min_x: a.x.min(b.x).min(c.x),
            min_y: a.y.min(b.y).min(c.y),
            max_x: a.x.max(b.x).max(c.x),
            max_y: a.y.max(b.y).max(c.y),
        }
    }

    /// Clamp this bounding box to a render target. Returns `None` if the
    /// triangle is entirely off-screen.
    pub fn clamp(&self, w: i32, h: i32) -> Option<Self> {
        let r = Self {
            min_x: self.min_x.max(0),
            min_y: self.min_y.max(0),
            max_x: self.max_x.min(w - 1),
            max_y: self.max_y.min(h - 1),
        };
        if r.min_x > r.max_x || r.min_y > r.max_y {
            None
        } else {
            Some(r)
        }
    }
}

/// 2D edge function - positive when `p` is on the inside (right-hand
/// side) of the directed edge `a→b` under PSX winding. Sums of three
/// edge functions over a triangle's bbox give the barycentric weights
/// for an inside-triangle test (all-positive ⇒ inside).
pub fn edge(a: ScreenXY, b: ScreenXY, px: i32, py: i32) -> i64 {
    let ab_x = (b.x - a.x) as i64;
    let ab_y = (b.y - a.y) as i64;
    let ap_x = (px - a.x) as i64;
    let ap_y = (py - a.y) as i64;
    ab_x * ap_y - ab_y * ap_x
}

/// Whether `(px, py)` lies inside the triangle `(a, b, c)` under PSX
/// winding rules. Assumes the triangle is front-facing
/// ([`super::nclip`] returned negative); caller should reject
/// back-facing triangles before rasterising.
///
/// Edges on the bottom-right are counted as outside (top-left fill
/// rule), matching the PSX rasteriser's pixel-center convention.
pub fn contains(a: ScreenXY, b: ScreenXY, c: ScreenXY, px: i32, py: i32) -> bool {
    let w0 = edge(b, c, px, py);
    let w1 = edge(c, a, px, py);
    let w2 = edge(a, b, px, py);
    // Front-facing triangle: nclip < 0; the three edge functions then
    // share sign for inside points. Accept zero-area only on top-left
    // edges to avoid double-shading shared pixels.
    (w0 < 0 && w1 < 0 && w2 < 0)
        || (w0 == 0 && top_left(b, c))
        || (w1 == 0 && top_left(c, a))
        || (w2 == 0 && top_left(a, b))
}

/// PSX top-left fill rule: an edge counts as inside if it's exactly
/// horizontal pointing leftward, OR a non-horizontal edge pointing
/// upward.
fn top_left(a: ScreenXY, b: ScreenXY) -> bool {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    (dy == 0 && dx < 0) || dy < 0
}

/// Iterate every (px, py) inside `triangle`, calling `emit(px, py, w)`
/// where `w = (w0, w1, w2)` is the unnormalised edge-function triple
/// (caller can divide by triangle area to get barycentrics).
pub fn rasterize_triangle(
    a: ScreenXY,
    b: ScreenXY,
    c: ScreenXY,
    viewport_w: i32,
    viewport_h: i32,
    mut emit: impl FnMut(i32, i32, (i64, i64, i64)),
) {
    let bbox = match BBox::from_triangle(a, b, c).clamp(viewport_w, viewport_h) {
        Some(b) => b,
        None => return,
    };
    for py in bbox.min_y..=bbox.max_y {
        for px in bbox.min_x..=bbox.max_x {
            let w0 = edge(b, c, px, py);
            let w1 = edge(c, a, px, py);
            let w2 = edge(a, b, px, py);
            if w0 < 0 && w1 < 0 && w2 < 0 {
                emit(px, py, (w0, w1, w2));
            }
        }
    }
}
