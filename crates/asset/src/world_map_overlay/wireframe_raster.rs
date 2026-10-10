//! Top-down PNG rasteriser for the slot-4 wireframe lines, and its per-body
//! palette.

use super::*;

/// One per-body color, RGBA8. Matches `site/js/webgl-tmd.js::wireframeBodyColor`
/// so the standalone PNG renderer agrees visually with the WebGL viewer.
const PALETTE: &[[u8; 4]] = &[
    [0xF2, 0x8C, 0x59, 0xFF], //  0  amber
    [0xA6, 0xD9, 0x73, 0xFF], //  1  lime
    [0x8C, 0xCC, 0xF2, 0xFF], //  2  sky
    [0xF2, 0x8C, 0xF2, 0xFF], //  3  magenta
    [0xF2, 0xCC, 0x73, 0xFF], //  4  gold
    [0xA6, 0x73, 0xF2, 0xFF], //  5  violet
    [0x73, 0xF2, 0xA6, 0xFF], //  6  mint
    [0xF2, 0x73, 0x8C, 0xFF], //  7  rose
    [0x73, 0xA6, 0xF2, 0xFF], //  8  azure
    [0xD9, 0xD9, 0x73, 0xFF], //  9  olive
    [0x73, 0xF2, 0xF2, 0xFF], // 10  aqua
    [0xF2, 0xA6, 0x73, 0xFF], // 11  apricot
    [0x8C, 0xF2, 0xF2, 0xFF], // 12
    [0xF2, 0xF2, 0xA6, 0xFF], // 13
    [0xA6, 0xF2, 0x8C, 0xFF], // 14  chartreuse
    [0xF2, 0x8C, 0xA6, 0xFF], // 15  blush
];

/// Color for body `i`. Wraps if `i >= PALETTE.len()`.
pub fn body_color(i: usize) -> [u8; 4] {
    PALETTE[i % PALETTE.len()]
}

/// Top-down PNG rasterizer for slot-4 wireframe lines.
///
/// Output coordinate system: `x` increases right, `z` increases down
/// (matches the in-game minimap orientation). The world bounds are
/// computed from `xz_bounds(slot)`; the line set is rasterized into a
/// canvas of `(width, height)` with a configurable margin so labels /
/// dots stay inside.
pub struct WireframeRaster {
    pub width: u32,
    pub height: u32,
    /// Margin in pixels around the world bbox (each edge).
    pub margin: u32,
    /// Background color (RGBA8). Default `#0A0A1A` to match the site.
    pub bg: [u8; 4],
    /// Pixel buffer (RGBA8, row-major, length = width * height * 4).
    pub buf: Vec<u8>,
    /// Cached world-bounds: (xmin, zmin, xmax, zmax). All-zero if the
    /// slot is empty (rendering is a no-op in that case).
    pub world_bounds: (i32, i32, i32, i32),
}

impl WireframeRaster {
    /// Create a new raster initialised to `bg`.
    pub fn new(width: u32, height: u32, margin: u32, bg: [u8; 4]) -> Self {
        let mut buf = Vec::with_capacity((width as usize) * (height as usize) * 4);
        for _ in 0..(width * height) {
            buf.extend_from_slice(&bg);
        }
        Self {
            width,
            height,
            margin,
            bg,
            buf,
            world_bounds: (0, 0, 0, 0),
        }
    }

    /// Compute world bounds from the slot's record set, using the X-Z
    /// projection. Use [`Self::set_bounds_from_axes`] for non-default
    /// projections.
    pub fn set_bounds_from(&mut self, slot: &KingdomSlot4) {
        self.set_bounds_from_axes(slot, Axis::X, Axis::Z);
    }

    /// Compute world bounds from the slot's record set on the given
    /// axis pair. Use when rendering a non-default projection so the
    /// raster's viewport matches the data being plotted.
    pub fn set_bounds_from_axes(&mut self, slot: &KingdomSlot4, ah: Axis, av: Axis) {
        match axis_bounds(slot, ah, av) {
            Some((amin, bmin, amax, bmax)) => {
                self.world_bounds = (amin as i32, bmin as i32, amax as i32, bmax as i32);
            }
            None => self.world_bounds = (0, 0, 1, 1),
        }
    }

    /// Override bounds (when rasterizing several kingdoms at once and you
    /// want them sharing a common camera).
    pub fn set_bounds(&mut self, xmin: i32, zmin: i32, xmax: i32, zmax: i32) {
        self.world_bounds = (xmin, zmin, xmax, zmax);
    }

    /// Convert world (x, z) to pixel (px, py). Letter-boxes inside the
    /// margin so the shorter axis doesn't stretch.
    pub fn world_to_pix(&self, x: i32, z: i32) -> (i32, i32) {
        let (xmin, zmin, xmax, zmax) = self.world_bounds;
        let dx = (xmax - xmin).max(1) as f32;
        let dz = (zmax - zmin).max(1) as f32;
        let aw = (self.width as i32 - 2 * self.margin as i32).max(1) as f32;
        let ah = (self.height as i32 - 2 * self.margin as i32).max(1) as f32;
        // Letterbox: keep aspect ratio.
        let scale = (aw / dx).min(ah / dz);
        let pw = (dx * scale) as i32;
        let ph = (dz * scale) as i32;
        let ox = self.margin as i32 + (aw as i32 - pw) / 2;
        let oy = self.margin as i32 + (ah as i32 - ph) / 2;
        let px = ox + (((x - xmin) as f32) * scale) as i32;
        let py = oy + (((z - zmin) as f32) * scale) as i32;
        (px, py)
    }

    /// Plot a single pixel with alpha blending. Out-of-range coords are
    /// clipped.
    pub fn put_px(&mut self, x: i32, y: i32, color: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let off = ((y as u32 * self.width + x as u32) * 4) as usize;
        let sa = color[3] as u16;
        let inv = 255 - sa;
        for (c, src_c) in color.iter().enumerate().take(3) {
            let dst = self.buf[off + c] as u16;
            let src = *src_c as u16;
            self.buf[off + c] = ((dst * inv + src * sa) / 255) as u8;
        }
        // Keep alpha at 255 (opaque output).
    }

    /// Bresenham line in pixel space, blended.
    pub fn line_pix(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: [u8; 4]) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let mut x = x0;
        let mut y = y0;
        loop {
            self.put_px(x, y, color);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Filled circle of `radius` pixels around (cx, cy).
    pub fn dot_pix(&mut self, cx: i32, cy: i32, radius: i32, color: [u8; 4]) {
        let r2 = radius * radius;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= r2 {
                    self.put_px(cx + dx, cy + dy, color);
                }
            }
        }
    }

    /// Rasterize all wireframe lines, one body at a time. Body color
    /// comes from [`body_color`]; alpha is 220 so overlapping lines
    /// brighten the pixel without fully replacing it.
    ///
    /// When `only_body` is `Some(i)`, only body index `i` is rendered.
    /// Render order is body-major: body 0 first, body N-1 last; the
    /// the highest-index body (12 in Drake) therefore paints on top of the
    /// padded inner contours.
    pub fn draw_wireframe(
        &mut self,
        slot: &KingdomSlot4,
        opts: &WireframeOptions,
        only_body: Option<usize>,
    ) {
        let lines = top_down_lines(slot, opts);
        for l in &lines {
            if let Some(i) = only_body
                && l.body_index as usize != i
            {
                continue;
            }
            let mut c = body_color(l.body_index as usize);
            c[3] = 220;
            let (x0, y0) = self.world_to_pix(l.x0 as i32, l.z0 as i32);
            let (x1, y1) = self.world_to_pix(l.x1 as i32, l.z1 as i32);
            self.line_pix(x0, y0, x1, y1, c);
        }
    }

    /// Render every (non-zero) record as a small dot, colored by body.
    /// Polyline-topology-free; the truest one-shot view of the raw
    /// data. Optional `only_body` mirrors [`Self::draw_wireframe`].
    pub fn draw_points(
        &mut self,
        slot: &KingdomSlot4,
        opts: &WireframeOptions,
        only_body: Option<usize>,
        radius: i32,
    ) {
        let pts = record_points(slot, opts);
        for (body, _group, x, z) in &pts {
            if let Some(i) = only_body
                && (*body as usize) != i
            {
                continue;
            }
            let mut c = body_color(*body as usize);
            c[3] = 255;
            let (px, py) = self.world_to_pix(*x as i32, *z as i32);
            self.dot_pix(px, py, radius, c);
        }
    }

    /// Plot a placement scatter overlay. Each dot is a small filled
    /// circle with a halo so it stays visible against busy wireframes.
    pub fn draw_placements(&mut self, placements: &[(i32, i32)], color: [u8; 4], radius: i32) {
        for &(x, z) in placements {
            let (px, py) = self.world_to_pix(x, z);
            // Dark halo
            self.dot_pix(px, py, radius + 1, [0x00, 0x00, 0x00, 0xC0]);
            // Bright core
            self.dot_pix(px, py, radius, color);
        }
    }

    /// Encode the buffer as a PNG into `out`.
    pub fn encode_png<W: std::io::Write>(&self, out: W) -> std::io::Result<()> {
        let mut enc = png::Encoder::new(out, self.width, self.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc
            .write_header()
            .map_err(|e| std::io::Error::other(format!("png header: {e}")))?;
        writer
            .write_image_data(&self.buf)
            .map_err(|e| std::io::Error::other(format!("png data: {e}")))?;
        Ok(())
    }
}
