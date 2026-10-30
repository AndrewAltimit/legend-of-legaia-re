//! `[terrain]`: a sculpted ground under a pack map.
//!
//! The tile layer's floor is a corner grid of sixteen tiers, one height a
//! `128`-unit tile. A map with a `[terrain]` table draws and walks a finer
//! surface instead: a **heightfield** with `resolution` vertices a tile side
//! (`128 / resolution` world units apart), built from the tiers (smoothed),
//! then shaped by a list of sculpt strokes ([`TerrainShape`]). Its ground is
//! painted from **materials** - each a set of atlas cells of the base
//! scene's VRAM, named by the tile characters it covers - chosen quad by
//! quad with a noise-dithered boundary between neighbours, a per-tile
//! rotation and variant against visible repeats, and a vertex colour that
//! carries the material tints across the boundary, a macro brightness
//! variation and a baked sun. Steep faces take the cliff material; a water
//! table floods everything below its level.
//!
//! Everything here is a pure function of the file: the same heights,
//! paint, walls and colours on every host, with or without a disc. The
//! engine turns [`TerrainSurface`] into meshes
//! (`legaia_engine_scene::expansion_map::terrain`) and samples
//! [`Heightfield::sample_world`] as the floor, so the player walks exactly
//! the surface that draws.
//!
//! ```toml
//! [terrain]
//! resolution = 4
//! smooth = 2
//! max_step = 40
//! shapes = [
//!   { kind = "plateau", rect = [0, 0, 63, 3], height = 220, edge = 0.4, roughness = 0.6 },
//!   { kind = "ridge", from = [10.0, 7.0], to = [24.0, 9.0], radius = 2.5, height = 30 },
//!   { kind = "noise", amplitude = 6, scale = 2.0 },
//! ]
//! materials = [
//!   { name = "sand", chars = ".s", cells = [{ tpage = 12, clut = 32005, tile = 38 }] },
//! ]
//! cliff = { cells = [{ tpage = 12, clut = 32006, tile = 36 }], slope = 1.1 }
//! water = { level = -12, cells = [{ tpage = 9, clut = 31880, tile = 56 }] }
//! ```

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::map::{FLOOR_TIERS, Ground, MapDef, TileGrid};
use crate::map_detail::{AtlasCell, splitmix64};

/// The world units a tile spans.
pub const TILE_UNITS: i32 = 128;

/// The vertex densities a heightfield may take, vertices a tile side.
pub const RESOLUTIONS: [u8; 3] = [1, 2, 4];

/// The most tiles a terrain's skirt reaches past the map's edge.
pub const MAX_SKIRT: u8 = 12;

/// The highest a terrain vertex may stand, either way, in world units.
pub const MAX_HEIGHT: f32 = 4096.0;

/// A map's sculpted ground.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerrainDef {
    /// Vertices a tile side: `1`, `2` or `4` (a vertex every `128`, `64` or
    /// `32` world units).
    pub resolution: u8,
    /// Box-blur passes over the tier base before the shapes run: `0` keeps
    /// the tier steps as one-tile ramps, `2` rounds them into slopes.
    pub smooth: u8,
    /// The height range, world units, a `64 x 64` sub-cell may span and stay
    /// walkable. A steeper sub-cell is a wall quarter in the collision grid.
    pub max_step: f32,
    /// Seed of every noise the terrain draws (shapes carry their own).
    pub seed: u64,
    /// Tiles of terrain drawn past each edge of the map, continuing the
    /// edge's heights and paint (draw only: nothing walks there).
    pub skirt: u8,
    /// Macro brightness variation, `0..=1`: a low-frequency noise over the
    /// vertex colours that breaks up large single-material areas.
    pub variation: f32,
    /// Width of the dithered boundary between materials, `0..=1`; also how
    /// far, in tiles, the boundary meanders off the tile grid.
    pub blend: f32,
    /// How close, in score, the runner-up material must come for a quad to
    /// draw it half-blended over its own (`0` = never): the soft band of a
    /// boundary. Drawn through the PSX `0.5B + 0.5F` blend from palette
    /// copies carrying the STP bit, so it holds in both render modes.
    pub soften: f32,
    /// Subdivision of a quad on a material boundary: `1` (off), `2` or `4`
    /// sub-quads a side, each triangle choosing its material at its own
    /// centroid, so a boundary steps every `32 / detail` world units rather
    /// than every `32`. The sub-quads lie on the quad's own two triangles:
    /// the surface, and the floor the player walks, are unchanged.
    #[serde(default = "detail_off")]
    pub detail: u8,
    /// How far, in tiles, from the walkable floor (a tile neither void nor
    /// fully walled) a boundary is drawn in full: past it a quad keeps its
    /// own two triangles' materials, with no [`Self::detail`] subdivision
    /// and no [`Self::soften`] overlay. Ground a forest or a cliff band
    /// walls off is seen only from afar and through what stands on it, and
    /// there the finer boundary costs triangles nobody sees. `0` draws
    /// every boundary in full.
    pub detail_reach: u8,
    /// The baked sun: `[azimuth, elevation]` in degrees (azimuth `0` lights
    /// from `+Z`, `90` from `+X`).
    pub sun: [f32; 2],
    /// How strongly the sun shades the slopes, `0..=1` (`0` = unshaded).
    pub shading: f32,
    /// Sculpt strokes, applied in order.
    pub shapes: Vec<TerrainShape>,
    /// Ground materials. A tile whose character no material names paints
    /// with its style's ground texture.
    pub materials: Vec<TerrainMaterial>,
    /// The material steep faces take.
    pub cliff: Option<CliffMaterial>,
    /// A water table.
    pub water: Option<WaterDef>,
    /// A horizon ring around the map ([`SkyDef`]).
    pub sky: Option<SkyDef>,
}

impl Default for TerrainDef {
    fn default() -> Self {
        Self {
            resolution: 4,
            smooth: 2,
            max_step: 40.0,
            seed: 0,
            skirt: 0,
            variation: 0.15,
            blend: 0.5,
            soften: 0.3,
            detail: 1,
            detail_reach: 0,
            sun: [135.0, 50.0],
            shading: 0.5,
            shapes: Vec::new(),
            materials: Vec::new(),
            cliff: None,
            water: None,
            sky: None,
        }
    }
}

/// One sculpt stroke. Positions are map-local tile coordinates as floats
/// (`[2.5, 3.5]` is the centre of tile `[2, 3]`); radii are in tiles;
/// heights in world units, up positive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TerrainShape {
    /// Add `height` (negative lowers) under a round brush, falling off as a
    /// cosine to nothing at `radius`.
    Raise {
        at: [f32; 2],
        radius: f32,
        height: f32,
    },
    /// [`Self::Raise`] along a segment: a dune, a ridge, a ditch.
    Ridge {
        from: [f32; 2],
        to: [f32; 2],
        radius: f32,
        height: f32,
    },
    /// Set the tiles of `rect` (inclusive, `[col0, row0, col1, row1]`) to
    /// `height`, with an `edge` tiles wide slope around it. `roughness`
    /// (tiles) wobbles the edge so a cliff line is not ruled.
    Plateau {
        rect: [u16; 4],
        height: f32,
        #[serde(default = "half")]
        edge: f32,
        #[serde(default)]
        roughness: f32,
        #[serde(default)]
        seed: u64,
    },
    /// Lay a straight slope from `height[0]` at `from` to `height[1]` at
    /// `to`, `radius` tiles either side of the line (soft at the sides).
    Ramp {
        from: [f32; 2],
        to: [f32; 2],
        radius: f32,
        height: [f32; 2],
    },
    /// Pull the ground under a round brush toward `height`, or, with no
    /// `height`, toward the mean height under the brush (a house's pad on a
    /// hillside).
    Flatten {
        at: [f32; 2],
        radius: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height: Option<f32>,
        #[serde(default = "one")]
        strength: f32,
    },
    /// Blur the ground under a round brush.
    Smooth {
        at: [f32; 2],
        radius: f32,
        #[serde(default = "one")]
        strength: f32,
    },
    /// A brush dragged along `path`: `op` under a round brush `radius`
    /// tiles wide, at every point of the polyline (the distance to the
    /// nearest segment, so the joints do not add up). `raise` adds `height`
    /// (negative digs); `flatten` pulls toward `height`, or with none toward
    /// the mean along the path; `smooth` blurs. What the 3D sculpt brush
    /// writes for a drag.
    Stroke {
        path: Vec<[f32; 2]>,
        radius: f32,
        #[serde(default)]
        op: StrokeOp,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height: Option<f32>,
        #[serde(default = "one")]
        strength: f32,
    },
    /// A grayscale PNG under the pack's `assets/` (`image`, relative to
    /// `assets/`) stretched over `rect` (the whole map when left out, image
    /// row 0 at the rectangle's low-Z edge): black is `height[0]`, white
    /// `height[1]`. The ground is pulled toward it by `strength`, or with
    /// `add` the image's height is added. Read at the pack's load
    /// ([`MapDef::resolve_terrain_images`]); 8- or 16-bit, grey or colour
    /// (luminance).
    Heightmap {
        image: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rect: Option<[u16; 4]>,
        height: [f32; 2],
        #[serde(default = "one")]
        strength: f32,
        #[serde(default)]
        add: bool,
        #[serde(skip)]
        data: HeightImage,
    },
    /// Fractal noise over `rect` (the whole map when left out), `amplitude`
    /// either way, features `scale` tiles across.
    Noise {
        #[serde(default)]
        rect: Option<[u16; 4]>,
        amplitude: f32,
        #[serde(default = "two")]
        scale: f32,
        #[serde(default = "three")]
        octaves: u8,
        #[serde(default)]
        seed: u64,
    },
}

/// What a [`TerrainShape::Stroke`] does under its brush.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrokeOp {
    #[default]
    Raise,
    Flatten,
    Smooth,
}

/// A decoded heightmap image: `w x h` samples, `0..=1`, row-major. Empty
/// until [`MapDef::resolve_terrain_images`] reads the file.
#[derive(Debug, Clone, Default)]
pub struct HeightImage(pub Option<std::sync::Arc<GrayImage>>);

/// One heightmap's samples.
#[derive(Debug, Clone, PartialEq)]
pub struct GrayImage {
    pub w: usize,
    pub h: usize,
    pub v: Vec<f32>,
}

impl PartialEq for HeightImage {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b) || a == b,
            (None, None) => true,
            _ => false,
        }
    }
}

impl GrayImage {
    /// Decode a PNG to luminance samples.
    pub fn from_png(bytes: &[u8]) -> Result<Self> {
        let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
        dec.set_transformations(png::Transformations::EXPAND);
        let mut reader = dec.read_info()?;
        let mut buf = vec![
            0u8;
            reader
                .output_buffer_size()
                .ok_or_else(|| anyhow::anyhow!("PNG frame too large"))?
        ];
        let info = reader.next_frame(&mut buf)?;
        let (w, h) = (info.width as usize, info.height as usize);
        if w == 0 || h == 0 || w * h > 1 << 22 {
            bail!("a heightmap is 1 to 4M pixels ({w} x {h})");
        }
        let chans = match info.color_type {
            png::ColorType::Grayscale => 1,
            png::ColorType::GrayscaleAlpha => 2,
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            png::ColorType::Indexed => bail!("an indexed PNG did not expand"),
        };
        let wide = info.bit_depth == png::BitDepth::Sixteen;
        let data = &buf[..info.buffer_size()];
        let sample = |k: usize| -> f32 {
            if wide {
                f32::from(u16::from_be_bytes([data[2 * k], data[2 * k + 1]])) / 65535.0
            } else {
                f32::from(data[k]) / 255.0
            }
        };
        let v = (0..w * h)
            .map(|p| {
                let b = p * chans;
                if chans >= 3 {
                    0.299 * sample(b) + 0.587 * sample(b + 1) + 0.114 * sample(b + 2)
                } else {
                    sample(b)
                }
            })
            .collect();
        Ok(Self { w, h, v })
    }

    /// Bilinear sample at `(u, v)`, `0..=1` across the image.
    pub fn sample(&self, u: f32, v: f32) -> f32 {
        let x = (u.clamp(0.0, 1.0) * (self.w - 1) as f32).max(0.0);
        let y = (v.clamp(0.0, 1.0) * (self.h - 1) as f32).max(0.0);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let p = |x: usize, y: usize| self.v[y * self.w + x];
        let top = p(x0, y0) + (p(x1, y0) - p(x0, y0)) * fx;
        let bot = p(x0, y1) + (p(x1, y1) - p(x0, y1)) * fx;
        top + (bot - top) * fy
    }
}

fn detail_off() -> u8 {
    1
}
fn half() -> f32 {
    0.5
}
fn one() -> f32 {
    1.0
}
fn two() -> f32 {
    2.0
}
fn three() -> u8 {
    3
}
fn neutral() -> [u8; 3] {
    [128, 128, 128]
}
fn yes() -> bool {
    true
}

/// A ground material.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerrainMaterial {
    /// Name, for the painter and messages.
    #[serde(default)]
    pub name: String,
    /// The tile characters it paints.
    #[serde(default)]
    pub chars: String,
    /// Atlas cells of the base scene's VRAM; each tile picks one.
    pub cells: Vec<AtlasCell>,
    /// Vertex colour (`128` = the texel as is) the material carries; it
    /// blends across a boundary with the neighbour's.
    #[serde(default = "neutral")]
    pub tint: [u8; 3],
    /// Turn and mirror the cell per tile (8 ways) against visible repeats.
    /// Off for a texture with a direction (planks, a paved path).
    #[serde(default = "yes")]
    pub rotate: bool,
    /// World units the material's tiles stand above (or, negative, sink
    /// below) their tier before the smoothing: a worn path, a raised bed.
    #[serde(default)]
    pub lift: f32,
    /// World units either way the material's tiles are jostled, tile by
    /// tile, before the smoothing: a lumpy cave wall, a rubbled slope.
    #[serde(default)]
    pub roughness: f32,
    /// A retail scene whose VRAM the cells name instead of the base
    /// scene's: the map's load copies each cell's texels and palette into
    /// free VRAM and paints with the copies ([`TerrainDef::borrowed`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

/// The material steep faces take.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CliffMaterial {
    pub cells: Vec<AtlasCell>,
    /// Rise over run past which a quad is cliff (`1.0` = 45 degrees).
    #[serde(default = "one")]
    pub slope: f32,
    #[serde(default = "neutral")]
    pub tint: [u8; 3],
    /// As [`TerrainMaterial::from`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

/// A water table: a flat textured surface at `level` over every quad whose
/// ground dips below it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaterDef {
    /// World height of the surface, up positive.
    pub level: f32,
    /// The surface's atlas cells (animate them with `[[materials]]`).
    pub cells: Vec<AtlasCell>,
    /// Vertex colour where the water is shallow and where it is `depth` deep.
    #[serde(default = "shallow")]
    pub shallow: [u8; 3],
    #[serde(default = "deep")]
    pub deep: [u8; 3],
    /// The depth, world units, at which the colour reaches `deep`.
    #[serde(default = "sixty_four")]
    pub depth: f32,
    /// Shoreline foam: an additive band, brightest at the waterline,
    /// `foam` world units of depth wide (`0` = none).
    #[serde(default = "twelve")]
    pub foam: f32,
    /// Foam colour at the waterline (additive: black adds nothing).
    #[serde(default = "foam_color")]
    pub foam_color: [u8; 3],
    /// How deep the player may wade: ground deeper than this below the
    /// surface is a wall.
    #[serde(default = "four")]
    pub wade: f32,
    /// Frames between waves rolling in: the foam's depth bands brighten in
    /// turn, deepest first ([`surf_level`]). `0` holds the foam still.
    #[serde(default = "surf")]
    pub surf: u16,
    /// As [`TerrainMaterial::from`], for the surface's cells.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

fn surf() -> u16 {
    150
}

/// Depth bands of the shoreline foam, one palette each, shallowest first.
pub const FOAM_BANDS: usize = 4;

/// The texel index every foam texel holds: the band palettes' one live
/// entry.
pub const FOAM_INDEX: usize = 15;

/// A foam band palette at brightness `level` (`0..=31`): entry
/// [`FOAM_INDEX`] grey with the STP bit (the band blends additively), the
/// rest zero.
pub fn foam_palette(level: u16) -> Vec<u16> {
    let c = level.min(31);
    let mut words = vec![0u16; 16];
    words[FOAM_INDEX] = 0x8000 | c | (c << 5) | (c << 10);
    words
}

/// The brightness (`0..=31`) of foam band `band` (`0` = shallowest) at
/// `frame` under a surf of `period` frames: a crest that reaches the
/// deepest band first and rolls in to the waterline, with a low swell
/// between crests. Full brightness when `period` is `0`.
pub fn surf_level(frame: u64, period: u16, band: usize) -> u16 {
    if period == 0 {
        return 31;
    }
    let lag = (FOAM_BANDS - 1 - band.min(FOAM_BANDS - 1)) as f32 * 0.12;
    let p = (frame % u64::from(period)) as f32 / f32::from(period) - lag;
    let crest = 0.5 + 0.5 * (p * std::f32::consts::TAU).cos();
    (31.0 * (0.3 + 0.7 * crest.powi(3))).round() as u16
}

/// A horizon: a ring of wall around the map, far out, textured with a
/// texture page of the base scene's VRAM (a retail scene's sky page), the
/// texture repeated round it mirrored so its ends meet. It fills the void
/// past a map's edge the way a retail outdoor scene's backdrop mesh does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkyDef {
    pub tpage: u8,
    pub clut: u16,
    /// The texel rectangle `[u0, v0, u1, v1]` (inclusive) of the page it
    /// maps; the whole page when left out.
    #[serde(default = "full_page")]
    pub rect: [u8; 4],
    /// Tiles past the map's half-diagonal the ring stands.
    #[serde(default = "sky_radius")]
    pub radius: f32,
    /// World heights of the ring's foot and top, up positive.
    #[serde(default = "sky_span")]
    pub span: [f32; 2],
    /// Segments round the ring.
    #[serde(default = "sky_segments")]
    pub segments: u8,
    /// Mirrored copies of the texture round the ring.
    #[serde(default = "sky_repeat")]
    pub repeat: u8,
    #[serde(default = "neutral")]
    pub color: [u8; 3],
}

fn full_page() -> [u8; 4] {
    [0, 0, 255, 255]
}
fn sky_radius() -> f32 {
    8.0
}
fn sky_span() -> [f32; 2] {
    [-600.0, 2400.0]
}
fn sky_segments() -> u8 {
    32
}
fn sky_repeat() -> u8 {
    4
}

fn shallow() -> [u8; 3] {
    [150, 160, 150]
}
fn deep() -> [u8; 3] {
    [70, 90, 120]
}
fn sixty_four() -> f32 {
    64.0
}
fn twelve() -> f32 {
    12.0
}
fn four() -> f32 {
    4.0
}
fn foam_color() -> [u8; 3] {
    [96, 100, 104]
}

/// The heights of a terrain: `cols x rows` vertices, `step` world units
/// apart, vertex `(0, 0)` at world `origin` (the skirt's corner).
#[derive(Debug, Clone, PartialEq)]
pub struct Heightfield {
    /// Vertices a tile side.
    pub res: usize,
    /// Tiles of skirt each side.
    pub skirt: usize,
    pub cols: usize,
    pub rows: usize,
    /// World units between vertices.
    pub step: i32,
    /// World `(x, z)` of vertex `(0, 0)`.
    pub origin: [i32; 2],
    /// Heights, up positive, row-major.
    pub heights: Vec<i16>,
}

impl Heightfield {
    /// Height of vertex `(i, j)`, clamped onto the field.
    pub fn at(&self, i: isize, j: isize) -> i16 {
        let i = i.clamp(0, self.cols as isize - 1) as usize;
        let j = j.clamp(0, self.rows as isize - 1) as usize;
        self.heights[j * self.cols + i]
    }

    /// The surface height (up positive) at world `(x, z)`, interpolated over
    /// the triangle the mesh draws there: a quad's corners `a b / c d`
    /// split along `b`-`c`, the PSX quad's two triangles `(a, b, c)` and
    /// `(b, d, c)`. `None` off the field.
    pub fn sample_world(&self, x: i32, z: i32) -> Option<i32> {
        let lx = x - self.origin[0];
        let lz = z - self.origin[1];
        if lx < 0 || lz < 0 {
            return None;
        }
        let (i, j) = ((lx / self.step) as usize, (lz / self.step) as usize);
        if i + 1 >= self.cols || j + 1 >= self.rows {
            return None;
        }
        let fx = (lx % self.step) as f32 / self.step as f32;
        let fz = (lz % self.step) as f32 / self.step as f32;
        let (i, j) = (i as isize, j as isize);
        let h = |a: isize, b: isize| f32::from(self.at(a, b));
        let (a, b, c, d) = (h(i, j), h(i + 1, j), h(i, j + 1), h(i + 1, j + 1));
        let y = if fx + fz <= 1.0 {
            a + (b - a) * fx + (c - a) * fz
        } else {
            d + (c - d) * (1.0 - fx) + (b - d) * (1.0 - fz)
        };
        Some(y.round() as i32)
    }

    /// [`Self::sample_world`] at fractional vertex coordinates.
    fn sample_vertex(&self, fi: f32, fj: f32) -> f32 {
        let x = self.origin[0] as f32 + fi * self.step as f32;
        let z = self.origin[1] as f32 + fj * self.step as f32;
        let max_x = self.origin[0] + (self.cols as i32 - 1) * self.step - 1;
        let max_z = self.origin[1] + (self.rows as i32 - 1) * self.step - 1;
        let x = (x.round() as i32).clamp(self.origin[0], max_x);
        let z = (z.round() as i32).clamp(self.origin[1], max_z);
        self.sample_world(x, z).unwrap_or(0) as f32
    }

    /// The vertex index of map-local tile coordinate `t` (a tile edge) on
    /// either axis.
    pub fn vertex_of_tile(&self, t: isize) -> isize {
        (t + self.skirt as isize) * self.res as isize
    }
}

/// What paints one quad of the terrain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuadPaint {
    /// The atlas cell.
    pub cell: AtlasCell,
    /// The cell's turn and mirror, `0..8` ([`orient_uv`]).
    pub orient: u8,
    /// Index into [`TerrainSurface::materials`]; `CLIFF` for the cliff.
    pub material: u16,
    /// A second material's cell and orientation drawn half-blended over
    /// the quad: the runner-up near a boundary, or the ground over a face
    /// just steep enough to be cliff ([`TerrainDef::soften`]).
    pub overlay: Option<(AtlasCell, u8)>,
}

/// A triangle's material choice: the winner (its index, cell and turn) and
/// the runner-up close enough to blend.
type Choice = (Option<(u16, (AtlasCell, u8))>, Option<(AtlasCell, u8)>);

/// [`QuadPaint::material`] of a cliff quad.
pub const CLIFF: u16 = u16::MAX;

/// A material as the surface resolved it: an explicit one or a style's
/// ground texture.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedMaterial {
    pub name: String,
    pub cells: Vec<AtlasCell>,
    pub tint: [u8; 3],
    pub rotate: bool,
}

/// A terrain resolved for drawing and walking.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainSurface {
    pub field: Heightfield,
    pub materials: Vec<ResolvedMaterial>,
    /// Vertex colours, parallel to `field.heights`.
    pub colors: Vec<[u8; 3]>,
    /// One paint per triangle, two a quad, `(cols - 1) x (rows - 1)` quads
    /// row-major: the quad's `(a, c, b)` and `(b, c, d)` triangles (corners
    /// `a b / c d`). `None` where no material reaches.
    pub paint: Vec<[Option<QuadPaint>; 2]>,
    /// Sub-quads a side of a boundary quad ([`TerrainDef::detail`]).
    pub detail: usize,
    /// The boundary quads' finer paint, by quad index into `paint`:
    /// `detail x detail` sub-quads row-major, two triangles each, split as
    /// the quad is. A quad listed here draws its sub-quads instead.
    pub fine: std::collections::BTreeMap<usize, Vec<[Option<QuadPaint>; 2]>>,
    /// The water table, copied from the def.
    pub water: Option<WaterDef>,
}

impl TerrainSurface {
    /// The water colour of vertex `(i, j)` by its depth.
    pub fn water_color(&self, i: usize, j: usize) -> [u8; 3] {
        let Some(w) = &self.water else {
            return [128; 3];
        };
        let h = f32::from(self.field.heights[j * self.field.cols + i]);
        let t = ((w.level - h) / w.depth.max(1.0)).clamp(0.0, 1.0);
        std::array::from_fn(|k| {
            (f32::from(w.shallow[k]) + (f32::from(w.deep[k]) - f32::from(w.shallow[k])) * t).round()
                as u8
        })
    }

    /// The foam brightness of vertex `(i, j)`, `0..=1`: `1` at the
    /// waterline, `0` at `foam` depth or a quarter of it above the line.
    pub fn foam_at(&self, i: usize, j: usize) -> f32 {
        let Some(w) = &self.water else {
            return 0.0;
        };
        if w.foam <= 0.0 {
            return 0.0;
        }
        let d = w.level - f32::from(self.field.heights[j * self.field.cols + i]);
        if d >= 0.0 {
            (1.0 - d / w.foam).max(0.0)
        } else {
            (1.0 + d / (w.foam * 0.25)).max(0.0)
        }
    }
}

/// The texel corner `(u, v)` (`0..=1` across the cell) a quad corner maps
/// to under orientation `orient`: bit 0 mirrors U, bit 1 mirrors V, bit 2
/// swaps the axes.
pub fn orient_uv(orient: u8, u: f32, v: f32) -> (f32, f32) {
    let (mut u, mut v) = (u, v);
    if orient & 4 != 0 {
        std::mem::swap(&mut u, &mut v);
    }
    if orient & 1 != 0 {
        u = 1.0 - u;
    }
    if orient & 2 != 0 {
        v = 1.0 - v;
    }
    (u, v)
}

/// A hash of `(seed, a, b)` in `0..1`.
fn hash01(seed: u64, a: i64, b: i64) -> f32 {
    let k = splitmix64(
        seed ^ (a as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (b as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F),
    );
    (k >> 40) as f32 / (1u64 << 24) as f32
}

/// Smooth value noise in `0..1`, one lattice point a unit.
pub fn value_noise(seed: u64, x: f32, z: f32) -> f32 {
    let (x0, z0) = (x.floor(), z.floor());
    let (fx, fz) = (x - x0, z - z0);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sz) = (s(fx), s(fz));
    let (xi, zi) = (x0 as i64, z0 as i64);
    let h = |a, b| hash01(seed, a, b);
    let top = h(xi, zi) + (h(xi + 1, zi) - h(xi, zi)) * sx;
    let bot = h(xi, zi + 1) + (h(xi + 1, zi + 1) - h(xi, zi + 1)) * sx;
    top + (bot - top) * sz
}

/// Fractal value noise in `0..1`: `octaves` layers, each twice as fine and
/// half as strong.
pub fn fbm(seed: u64, x: f32, z: f32, octaves: u8) -> f32 {
    let (mut sum, mut norm, mut amp, mut f) = (0.0, 0.0, 1.0, 1.0);
    for o in 0..octaves.max(1) {
        sum += amp * value_noise(seed.wrapping_add(u64::from(o) * 7919), x * f, z * f);
        norm += amp;
        amp *= 0.5;
        f *= 2.0;
    }
    sum / norm
}

/// A cosine falloff: `1` at `d = 0`, `0` from `d = r` on.
fn cos_falloff(d: f32, r: f32) -> f32 {
    if r <= 0.0 || d >= r {
        0.0
    } else {
        0.5 * (1.0 + (std::f32::consts::PI * d / r).cos())
    }
}

/// Distance from `p` to segment `a`-`b`, and the segment parameter `0..=1`
/// of the nearest point.
fn segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> (f32, f32) {
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dz * dz;
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (qx, qz) = (a[0] + dx * t, a[1] + dz * t);
    ((p[0] - qx).hypot(p[1] - qz), t)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// One 3x3 box-blur pass over `h` (`cols x rows`), edges clamped.
fn blur(h: &[f32], cols: usize, rows: usize) -> Vec<f32> {
    let mut out = vec![0.0; h.len()];
    for j in 0..rows {
        for i in 0..cols {
            let mut s = 0.0;
            for dj in -1isize..=1 {
                for di in -1isize..=1 {
                    let ii = (i as isize + di).clamp(0, cols as isize - 1) as usize;
                    let jj = (j as isize + dj).clamp(0, rows as isize - 1) as usize;
                    s += h[jj * cols + ii];
                }
            }
            out[j * cols + i] = s / 9.0;
        }
    }
    out
}

impl TerrainDef {
    /// The vertex density, checked.
    pub fn res(&self) -> Result<usize> {
        if !RESOLUTIONS.contains(&self.resolution) {
            bail!(
                "terrain resolution {} is not one of {RESOLUTIONS:?}",
                self.resolution
            );
        }
        Ok(usize::from(self.resolution))
    }
}

impl TerrainDef {
    /// The retail scenes whose VRAM the terrain's cells name (`from`), each
    /// once, in order.
    pub fn borrowed(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        let froms = self.materials.iter().map(|m| m.from.as_deref()).chain([
            self.cliff.as_ref().and_then(|c| c.from.as_deref()),
            self.water.as_ref().and_then(|w| w.from.as_deref()),
        ]);
        for f in froms.flatten() {
            if !out.contains(&f) {
                out.push(f);
            }
        }
        out
    }

    /// Every atlas cell the terrain paints with that names scene `from`'s
    /// VRAM, mutably: what the map's load rewrites to its copies.
    pub fn borrowed_cells_mut(&mut self, from: &str) -> Vec<&mut AtlasCell> {
        let mut out: Vec<&mut AtlasCell> = Vec::new();
        for m in &mut self.materials {
            if m.from.as_deref() == Some(from) {
                out.extend(m.cells.iter_mut());
            }
        }
        if let Some(c) = self
            .cliff
            .as_mut()
            .filter(|c| c.from.as_deref() == Some(from))
        {
            out.extend(c.cells.iter_mut());
        }
        if let Some(w) = self
            .water
            .as_mut()
            .filter(|w| w.from.as_deref() == Some(from))
        {
            out.extend(w.cells.iter_mut());
        }
        out
    }
}

impl MapDef {
    /// Read every [`TerrainShape::Heightmap`]'s image out of the pack's
    /// `assets/` (keyed relative to `assets/`). Returns a message per image
    /// that is missing or does not decode; such a shape shapes nothing.
    pub fn resolve_terrain_images(
        &mut self,
        assets: &std::collections::BTreeMap<String, Vec<u8>>,
    ) -> Vec<String> {
        let mut problems = Vec::new();
        let Some(t) = self.terrain.as_mut() else {
            return problems;
        };
        for s in &mut t.shapes {
            if let TerrainShape::Heightmap { image, data, .. } = s {
                let key = image.strip_prefix("assets/").unwrap_or(image);
                match assets.get(key) {
                    None => {
                        data.0 = None;
                        problems.push(format!("heightmap assets/{key} is not in the pack"));
                    }
                    Some(bytes) => match GrayImage::from_png(bytes) {
                        Ok(img) => data.0 = Some(std::sync::Arc::new(img)),
                        Err(e) => {
                            data.0 = None;
                            problems.push(format!("heightmap assets/{key}: {e}"));
                        }
                    },
                }
            }
        }
        problems
    }

    /// The map's terrain heights, `None` with no `[terrain]`.
    pub fn terrain_heightfield(&self) -> Result<Option<Heightfield>> {
        let Some(t) = &self.terrain else {
            return Ok(None);
        };
        let grid = self.tile_grid()?;
        heightfield(self, t, &grid).map(Some)
    }

    /// The map's terrain resolved for drawing ([`TerrainSurface`]), `None`
    /// with no `[terrain]`.
    pub fn terrain_surface(&self) -> Result<Option<TerrainSurface>> {
        let Some(t) = &self.terrain else {
            return Ok(None);
        };
        let grid = self.tile_grid()?;
        let field = heightfield(self, t, &grid)?;
        Ok(Some(surface(self, t, &grid, field)))
    }

    /// The wall quarters the terrain adds to each tile (row-major, the
    /// [`crate::map::Tile::walls`] bits): every `64 x 64` sub-cell steeper
    /// than `max_step`, or under more than `wade` of water. All zero with no
    /// `[terrain]`.
    pub fn terrain_walls(&self, grid: &TileGrid) -> Result<Vec<u8>> {
        let mut out = vec![0u8; grid.width * grid.height];
        let Some(t) = &self.terrain else {
            return Ok(out);
        };
        let field = heightfield(self, t, grid)?;
        let res = field.res as f32;
        for row in 0..grid.height {
            for col in 0..grid.width {
                let mut walls = 0u8;
                for q in 0..4u8 {
                    let (qx, qz) = (f32::from(q & 1), f32::from(q >> 1));
                    let i0 = (col + field.skirt) as f32 * res + qx * res / 2.0;
                    let j0 = (row + field.skirt) as f32 * res + qz * res / 2.0;
                    let mut lo = f32::MAX;
                    let mut hi = f32::MIN;
                    for a in 0..=2 {
                        for b in 0..=2 {
                            let h = field.sample_vertex(
                                i0 + a as f32 * res / 4.0,
                                j0 + b as f32 * res / 4.0,
                            );
                            lo = lo.min(h);
                            hi = hi.max(h);
                        }
                    }
                    let centre = field.sample_vertex(i0 + res / 4.0, j0 + res / 4.0);
                    let steep = hi - lo > t.max_step;
                    let drowned = t.water.as_ref().is_some_and(|w| centre < w.level - w.wade);
                    if steep || drowned {
                        walls |= 1 << q;
                    }
                }
                out[row * grid.width + col] = walls;
            }
        }
        Ok(out)
    }
}

/// The tier height of every tile (void tiles take a floor neighbour's, as
/// the collision grid writes them).
fn tile_heights(def: &MapDef, t: &TerrainDef, grid: &TileGrid) -> Vec<f32> {
    let ladder = def.floor_ladder().unwrap_or([0; FLOOR_TIERS]);
    let lift = |c: usize, r: usize| -> f32 {
        let ch = grid.char_at(c, r).unwrap_or(' ');
        t.materials
            .iter()
            .find(|m| m.chars.contains(ch))
            .map_or(0.0, |m| {
                let jostle = (hash01(t.seed ^ 0x524F_5547, c as i64, r as i64) - 0.5) * 2.0;
                m.lift + m.roughness * jostle
            })
    };
    let tier = |c: usize, r: usize| -> u8 {
        let t = grid.get(c, r).expect("in range");
        if !t.void {
            return t.tier;
        }
        let floor = |c: Option<usize>, r: Option<usize>| {
            grid.get(c?, r?).filter(|t| !t.void).map(|t| t.tier)
        };
        floor(c.checked_sub(1), Some(r))
            .or_else(|| floor(Some(c), r.checked_sub(1)))
            .or_else(|| floor(c.checked_sub(1), r.checked_sub(1)))
            .unwrap_or(0)
    };
    let mut out = Vec::with_capacity(grid.width * grid.height);
    for r in 0..grid.height {
        for c in 0..grid.width {
            out.push(f32::from(ladder[usize::from(tier(c, r))]) + lift(c, r));
        }
    }
    out
}

fn heightfield(def: &MapDef, t: &TerrainDef, grid: &TileGrid) -> Result<Heightfield> {
    let res = t.res()?;
    if t.skirt > MAX_SKIRT {
        bail!("terrain skirt {} is past {MAX_SKIRT} tiles", t.skirt);
    }
    if grid.width == 0 || grid.height == 0 {
        bail!("terrain on an empty map");
    }
    let skirt = usize::from(t.skirt);
    let cols = (grid.width + 2 * skirt) * res + 1;
    let rows = (grid.height + 2 * skirt) * res + 1;
    let tiles = tile_heights(def, t, grid);
    let tile_h = |c: isize, r: isize| -> f32 {
        let c = c.clamp(0, grid.width as isize - 1) as usize;
        let r = r.clamp(0, grid.height as isize - 1) as usize;
        tiles[r * grid.width + c]
    };
    // Map-local tile coordinate of vertex (i, j).
    let uv = |i: usize, j: usize| -> [f32; 2] {
        [
            i as f32 / res as f32 - skirt as f32,
            j as f32 / res as f32 - skirt as f32,
        ]
    };
    // The base: tile-centre heights, bilinear between centres.
    let mut h = Vec::with_capacity(cols * rows);
    for j in 0..rows {
        for i in 0..cols {
            let [u, v] = uv(i, j);
            let (cx, cz) = (u - 0.5, v - 0.5);
            let (c0, r0) = (cx.floor(), cz.floor());
            let (tx, tz) = (cx - c0, cz - r0);
            let (c0, r0) = (c0 as isize, r0 as isize);
            let top = tile_h(c0, r0) + (tile_h(c0 + 1, r0) - tile_h(c0, r0)) * tx;
            let bot = tile_h(c0, r0 + 1) + (tile_h(c0 + 1, r0 + 1) - tile_h(c0, r0 + 1)) * tx;
            h.push(top + (bot - top) * tz);
        }
    }
    for _ in 0..t.smooth.min(16) {
        h = blur(&h, cols, rows);
    }
    for shape in &t.shapes {
        let map_rect = [0.0, 0.0, grid.width as f32, grid.height as f32];
        apply_shape(&mut h, cols, rows, &uv, shape, t.seed, map_rect);
    }
    let heights = h
        .iter()
        .map(|v| v.clamp(-MAX_HEIGHT, MAX_HEIGHT).round() as i16)
        .collect();
    let step = TILE_UNITS / res as i32;
    let origin = [
        (i32::from(def.tiles.origin[0]) - skirt as i32) * TILE_UNITS,
        (i32::from(def.tiles.origin[1]) - skirt as i32) * TILE_UNITS,
    ];
    Ok(Heightfield {
        res,
        skirt,
        cols,
        rows,
        step,
        origin,
        heights,
    })
}

fn apply_shape(
    h: &mut [f32],
    cols: usize,
    rows: usize,
    uv: &impl Fn(usize, usize) -> [f32; 2],
    shape: &TerrainShape,
    terrain_seed: u64,
    map_rect: [f32; 4],
) {
    let each = |h: &mut [f32], f: &mut dyn FnMut([f32; 2], f32) -> f32| {
        for j in 0..rows {
            for i in 0..cols {
                let k = j * cols + i;
                h[k] = f(uv(i, j), h[k]);
            }
        }
    };
    match *shape {
        TerrainShape::Raise { at, radius, height } => each(h, &mut |p, v| {
            v + height * cos_falloff((p[0] - at[0]).hypot(p[1] - at[1]), radius)
        }),
        TerrainShape::Ridge {
            from,
            to,
            radius,
            height,
        } => each(h, &mut |p, v| {
            v + height * cos_falloff(segment(p, from, to).0, radius)
        }),
        TerrainShape::Plateau {
            rect,
            height,
            edge,
            roughness,
            seed,
        } => {
            let [c0, r0, c1, r1] = rect.map(f32::from);
            let (x0, z0, x1, z1) = (c0.min(c1), r0.min(r1), c0.max(c1) + 1.0, r0.max(r1) + 1.0);
            let seed = seed ^ terrain_seed ^ 0x504C_4154;
            each(h, &mut |p, v| {
                // Distance outside the rectangle (0 inside).
                let dx = (x0 - p[0]).max(p[0] - x1).max(0.0);
                let dz = (z0 - p[1]).max(p[1] - z1).max(0.0);
                let mut d = dx.hypot(dz);
                if roughness > 0.0 {
                    d += roughness * (fbm(seed, p[0] * 0.6, p[1] * 0.6, 3) - 0.5) * 2.0;
                }
                let m = 1.0 - smoothstep(0.0, edge.max(0.01), d);
                v + (height - v) * m
            })
        }
        TerrainShape::Ramp {
            from,
            to,
            radius,
            height,
        } => each(h, &mut |p, v| {
            let (d, s) = segment(p, from, to);
            let m = 1.0 - smoothstep(radius * 0.6, radius.max(0.01), d);
            let target = height[0] + (height[1] - height[0]) * s;
            v + (target - v) * m
        }),
        TerrainShape::Flatten {
            at,
            radius,
            height,
            strength,
        } => {
            let height = height.unwrap_or_else(|| {
                let (mut sum, mut n) = (0.0, 0.0);
                for j in 0..rows {
                    for i in 0..cols {
                        let p = uv(i, j);
                        let w = cos_falloff((p[0] - at[0]).hypot(p[1] - at[1]), radius);
                        sum += h[j * cols + i] * w;
                        n += w;
                    }
                }
                if n > 0.0 { sum / n } else { 0.0 }
            });
            each(h, &mut |p, v| {
                let m = cos_falloff((p[0] - at[0]).hypot(p[1] - at[1]), radius) * strength;
                v + (height - v) * m.clamp(0.0, 1.0)
            })
        }
        TerrainShape::Smooth {
            at,
            radius,
            strength,
        } => {
            let mut b = h.to_vec();
            for _ in 0..3 {
                b = blur(&b, cols, rows);
            }
            for j in 0..rows {
                for i in 0..cols {
                    let p = uv(i, j);
                    let k = j * cols + i;
                    let m = (cos_falloff((p[0] - at[0]).hypot(p[1] - at[1]), radius) * strength)
                        .clamp(0.0, 1.0);
                    h[k] += (b[k] - h[k]) * m;
                }
            }
        }
        TerrainShape::Stroke {
            ref path,
            radius,
            op,
            height,
            strength,
        } => {
            if path.is_empty() {
                return;
            }
            // Distance from `p` to the polyline.
            let dist = |p: [f32; 2]| -> f32 {
                if path.len() == 1 {
                    return (p[0] - path[0][0]).hypot(p[1] - path[0][1]);
                }
                path.windows(2)
                    .map(|w| segment(p, w[0], w[1]).0)
                    .fold(f32::MAX, f32::min)
            };
            match op {
                StrokeOp::Raise => {
                    let height = height.unwrap_or(0.0);
                    each(h, &mut |p, v| {
                        v + height * strength * cos_falloff(dist(p), radius)
                    })
                }
                StrokeOp::Flatten => {
                    let level = height.unwrap_or_else(|| {
                        let (mut sum, mut n) = (0.0, 0.0);
                        for j in 0..rows {
                            for i in 0..cols {
                                let w = cos_falloff(dist(uv(i, j)), radius);
                                sum += h[j * cols + i] * w;
                                n += w;
                            }
                        }
                        if n > 0.0 { sum / n } else { 0.0 }
                    });
                    each(h, &mut |p, v| {
                        let m = (cos_falloff(dist(p), radius) * strength).clamp(0.0, 1.0);
                        v + (level - v) * m
                    })
                }
                StrokeOp::Smooth => {
                    let mut b = h.to_vec();
                    for _ in 0..3 {
                        b = blur(&b, cols, rows);
                    }
                    for j in 0..rows {
                        for i in 0..cols {
                            let k = j * cols + i;
                            let m =
                                (cos_falloff(dist(uv(i, j)), radius) * strength).clamp(0.0, 1.0);
                            h[k] += (b[k] - h[k]) * m;
                        }
                    }
                }
            }
        }
        TerrainShape::Heightmap {
            rect,
            height,
            strength,
            add,
            ref data,
            ..
        } => {
            let Some(img) = data.0.as_ref() else {
                return;
            };
            let [x0, z0, x1, z1] = match rect {
                Some([c0, r0, c1, r1]) => [
                    f32::from(c0.min(c1)),
                    f32::from(r0.min(r1)),
                    f32::from(c0.max(c1)) + 1.0,
                    f32::from(r0.max(r1)) + 1.0,
                ],
                None => map_rect,
            };
            each(h, &mut |p, v| {
                if p[0] < x0 || p[0] > x1 || p[1] < z0 || p[1] > z1 {
                    return v;
                }
                let s = img.sample(
                    (p[0] - x0) / (x1 - x0).max(1e-3),
                    (p[1] - z0) / (z1 - z0).max(1e-3),
                );
                let target = height[0] + (height[1] - height[0]) * s;
                if add {
                    v + target * strength
                } else {
                    v + (target - v) * strength.clamp(0.0, 1.0)
                }
            })
        }
        TerrainShape::Noise {
            rect,
            amplitude,
            scale,
            octaves,
            seed,
        } => {
            let seed = seed ^ terrain_seed ^ 0x4E4F_4953;
            let inv = 1.0 / scale.max(0.05);
            each(h, &mut |p, v| {
                let m = match rect {
                    None => 1.0,
                    Some([c0, r0, c1, r1]) => {
                        let [c0, r0, c1, r1] = [c0, r0, c1, r1].map(f32::from);
                        let dx = (c0 - p[0]).max(p[0] - (c1 + 1.0)).max(0.0);
                        let dz = (r0 - p[1]).max(p[1] - (r1 + 1.0)).max(0.0);
                        1.0 - smoothstep(0.0, 1.0, dx.hypot(dz))
                    }
                };
                v + amplitude * (fbm(seed, p[0] * inv, p[1] * inv, octaves) - 0.5) * 2.0 * m
            })
        }
    }
}

/// The materials a terrain paints with: the explicit ones, then one per
/// distinct style ground texture no explicit material claims. Returns them
/// with each tile's material index.
fn tile_materials(
    t: &TerrainDef,
    def: &MapDef,
    grid: &TileGrid,
) -> (Vec<ResolvedMaterial>, Vec<Option<u16>>) {
    let mut mats: Vec<ResolvedMaterial> = t
        .materials
        .iter()
        .map(|m| ResolvedMaterial {
            name: m.name.clone(),
            cells: m.cells.clone(),
            tint: m.tint,
            rotate: m.rotate,
        })
        .collect();
    let mut per_tile = vec![None; grid.width * grid.height];
    for r in 0..grid.height {
        for c in 0..grid.width {
            let ch = grid.char_at(c, r).unwrap_or(' ');
            let explicit = t.materials.iter().position(|m| m.chars.contains(ch));
            let idx = match explicit {
                Some(k) => Some(k),
                None => {
                    let ground = def
                        .tiles
                        .legend
                        .iter()
                        .find(|(k, _)| k.starts_with(ch))
                        .and_then(|(_, s)| s.ground.as_ref());
                    match ground {
                        Some(Ground::Texture { tpage, clut, tile }) => {
                            let cell = AtlasCell {
                                tpage: *tpage,
                                clut: *clut,
                                tile: *tile,
                            };
                            Some(
                                mats.iter()
                                    .position(|m| m.cells == [cell] && m.name.starts_with('\''))
                                    .unwrap_or_else(|| {
                                        mats.push(ResolvedMaterial {
                                            name: format!("'{ch}'"),
                                            cells: vec![cell],
                                            tint: [128; 3],
                                            rotate: false,
                                        });
                                        mats.len() - 1
                                    }),
                            )
                        }
                        _ => None,
                    }
                }
            };
            per_tile[r * grid.width + c] = idx.map(|k| k as u16);
        }
    }
    // A tile with no material takes its nearest painted neighbour's.
    let painted: Vec<(usize, usize, u16)> = (0..grid.height)
        .flat_map(|r| (0..grid.width).map(move |c| (c, r)))
        .filter_map(|(c, r)| per_tile[r * grid.width + c].map(|m| (c, r, m)))
        .collect();
    if !painted.is_empty() {
        let filled: Vec<Option<u16>> = (0..grid.height)
            .flat_map(|r| (0..grid.width).map(move |c| (c, r)))
            .map(|(c, r)| {
                per_tile[r * grid.width + c].or_else(|| {
                    painted
                        .iter()
                        .min_by_key(|&&(pc, pr, _)| {
                            (pc as isize - c as isize).pow(2) + (pr as isize - r as isize).pow(2)
                        })
                        .map(|&(_, _, m)| m)
                })
            })
            .collect();
        per_tile = filled;
    }
    (mats, per_tile)
}

/// Each map tile's Chebyshev distance in tiles from the nearest walkable
/// tile (neither void nor fully walled), capped at `reach + 1`, row-major;
/// `None` when `reach` is `0` (no limit).
fn walkable_distance(grid: &TileGrid, reach: u8) -> Option<Vec<u8>> {
    if reach == 0 {
        return None;
    }
    let cap = reach.saturating_add(1);
    let (w, h) = (grid.width, grid.height);
    let mut d = vec![cap; w * h];
    let mut queue = std::collections::VecDeque::new();
    for r in 0..h {
        for c in 0..w {
            if grid.get(c, r).is_some_and(|t| !t.void && t.walls != 0xF) {
                d[r * w + c] = 0;
                queue.push_back((c, r));
            }
        }
    }
    while let Some((c, r)) = queue.pop_front() {
        let next = d[r * w + c] + 1;
        if next >= cap {
            continue;
        }
        for dr in -1..=1isize {
            for dc in -1..=1isize {
                let (nc, nr) = (c as isize + dc, r as isize + dr);
                if nc < 0 || nr < 0 || nc >= w as isize || nr >= h as isize {
                    continue;
                }
                let k = nr as usize * w + nc as usize;
                if d[k] > next {
                    d[k] = next;
                    queue.push_back((nc as usize, nr as usize));
                }
            }
        }
    }
    Some(d)
}

fn surface(def: &MapDef, t: &TerrainDef, grid: &TileGrid, field: Heightfield) -> TerrainSurface {
    let (mats, per_tile) = tile_materials(t, def, grid);
    let (cols, rows, res, skirt) = (field.cols, field.rows, field.res, field.skirt);
    let tile_mat = |c: isize, r: isize| -> Option<u16> {
        let c = c.clamp(0, grid.width as isize - 1) as usize;
        let r = r.clamp(0, grid.height as isize - 1) as usize;
        per_tile[r * grid.width + c]
    };
    // Bilinear material weights at map-local tile coordinate (u, v).
    let weights = |u: f32, v: f32| -> [(Option<u16>, f32); 4] {
        let (cx, cz) = (u - 0.5, v - 0.5);
        let (c0, r0) = (cx.floor(), cz.floor());
        let (tx, tz) = (cx - c0, cz - r0);
        let (c0, r0) = (c0 as isize, r0 as isize);
        [
            (tile_mat(c0, r0), (1.0 - tx) * (1.0 - tz)),
            (tile_mat(c0 + 1, r0), tx * (1.0 - tz)),
            (tile_mat(c0, r0 + 1), (1.0 - tx) * tz),
            (tile_mat(c0 + 1, r0 + 1), tx * tz),
        ]
    };
    let to_uv = |i: f32, j: f32| (i / res as f32 - skirt as f32, j / res as f32 - skirt as f32);
    let step = field.step as f32;
    // Sun direction (y up).
    let (az, el) = (
        t.sun[0].to_radians(),
        t.sun[1].clamp(1.0, 90.0).to_radians(),
    );
    let sun = [az.sin() * el.cos(), el.sin(), az.cos() * el.cos()];
    let mut colors = Vec::with_capacity(cols * rows);
    for j in 0..rows {
        for i in 0..cols {
            let (u, v) = to_uv(i as f32, j as f32);
            let mut tint = [0.0f32; 3];
            let mut wsum = 0.0;
            for (m, w) in weights(u, v) {
                let c = m.map_or([128; 3], |m| mats[usize::from(m)].tint);
                for k in 0..3 {
                    tint[k] += f32::from(c[k]) * w;
                }
                wsum += w;
            }
            let tint = tint.map(|c| c / wsum.max(1e-6));
            let (ii, jj) = (i as isize, j as isize);
            let hx =
                (f32::from(field.at(ii + 1, jj)) - f32::from(field.at(ii - 1, jj))) / (2.0 * step);
            let hz =
                (f32::from(field.at(ii, jj + 1)) - f32::from(field.at(ii, jj - 1))) / (2.0 * step);
            let n = [-hx, 1.0, -hz];
            let nl = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            let ndl = (n[0] * sun[0] + n[1] * sun[1] + n[2] * sun[2]) / nl;
            let shade = (1.0 + t.shading * (ndl - sun[1]) / sun[1].max(0.2)).clamp(0.45, 1.45);
            let macro_v =
                1.0 + t.variation * (fbm(t.seed ^ 0x4D41_4352, u / 3.0, v / 3.0, 3) - 0.5) * 2.0;
            let k = shade * macro_v;
            colors.push(tint.map(|c| (c * k).round().clamp(0.0, 255.0) as u8));
        }
    }
    // Score each candidate material at `(u, v)`: its summed weight at a
    // noise-warped point (so a boundary meanders rather than following the
    // tile grid) plus a dithering noise of its own. Best first.
    let rank = |u: f32, v: f32| -> Vec<(u16, f32)> {
        let warp = t.blend;
        let (wu, wv) = (
            u + warp * (fbm(t.seed ^ 0x5741_5250, u * 0.45, v * 0.45, 3) - 0.5) * 2.0,
            v + warp * (fbm(t.seed ^ 0x5741_5251, u * 0.45, v * 0.45, 3) - 0.5) * 2.0,
        );
        let mut cand: Vec<(u16, f32)> = Vec::with_capacity(4);
        for (m, w) in weights(wu, wv) {
            let Some(m) = m else { continue };
            match cand.iter_mut().find(|(cm, _)| *cm == m) {
                Some((_, cw)) => *cw += w,
                None => cand.push((m, w)),
            }
        }
        let mut scored: Vec<(u16, f32)> = cand
            .into_iter()
            .map(|(m, w)| {
                let n = fbm(
                    t.seed ^ (u64::from(m) * 0x1F3D + 0x5A5A),
                    u * 1.7,
                    v * 1.7,
                    2,
                );
                (m, w + t.blend * 0.6 * (n - 0.5))
            })
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        scored
    };
    let detail = usize::from(t.detail.clamp(1, 4));
    // Each tile's distance from the walkable floor, in tiles (Chebyshev),
    // when `detail_reach` limits where a boundary draws in full; a skirt
    // quad counts its overshoot past the map's edge.
    let reach_of = walkable_distance(grid, t.detail_reach);
    let in_reach = |tc: isize, tr: isize| -> bool {
        let Some(d) = &reach_of else { return true };
        let c = tc.clamp(0, grid.width as isize - 1);
        let r = tr.clamp(0, grid.height as isize - 1);
        let over = (tc - c).abs().max((tr - r).abs()) as u32;
        u32::from(d[r as usize * grid.width + c as usize]).saturating_add(over)
            <= u32::from(t.detail_reach)
    };
    // The winning material at each vertex: a quad whose corners or
    // triangles disagree lies on a boundary and is subdivided.
    let vertex_win: Vec<Option<u16>> = if detail > 1 {
        (0..rows)
            .flat_map(|j| (0..cols).map(move |i| (i, j)))
            .map(|(i, j)| {
                let (u, v) = to_uv(i as f32, j as f32);
                rank(u, v).first().map(|&(m, _)| m)
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut fine = std::collections::BTreeMap::new();
    let cliff_slope = t.cliff.as_ref().map(|c| c.slope);
    let mut paint = Vec::with_capacity((cols - 1) * (rows - 1));
    for j in 0..rows - 1 {
        for i in 0..cols - 1 {
            let (u, v) = to_uv(i as f32 + 0.5, j as f32 + 0.5);
            let (ii, jj) = (i as isize, j as isize);
            let hs = [
                field.at(ii, jj),
                field.at(ii + 1, jj),
                field.at(ii, jj + 1),
                field.at(ii + 1, jj + 1),
            ]
            .map(f32::from);
            let rise = (hs[0] - hs[1])
                .abs()
                .max((hs[2] - hs[3]).abs())
                .max((hs[0] - hs[2]).abs())
                .max((hs[1] - hs[3]).abs())
                .max((hs[1] - hs[2]).abs() * std::f32::consts::FRAC_1_SQRT_2);
            let tile_c = (i / res) as isize - skirt as isize;
            let tile_r = (j / res) as isize - skirt as isize;
            let full = in_reach(tile_c, tile_r);
            let tile_hash = |salt: u64| {
                splitmix64(
                    t.seed
                        ^ salt
                        ^ (tile_c as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                        ^ (tile_r as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F),
                )
            };
            let paint_of = |m: u16| -> Option<(AtlasCell, u8)> {
                let mat = &mats[usize::from(m)];
                if mat.cells.is_empty() {
                    return None;
                }
                let hsh = tile_hash(u64::from(m) << 20);
                Some((
                    mat.cells[(hsh >> 8) as usize % mat.cells.len()],
                    if mat.rotate { (hsh & 7) as u8 } else { 0 },
                ))
            };
            // The winner at `(u, v)` and the runner-up close enough to blend.
            let choose = |u: f32, v: f32| {
                let scored = rank(u, v);
                let ground = scored
                    .first()
                    .and_then(|&(m, _)| paint_of(m).map(|p| (m, p)));
                let runner = match (scored.first(), scored.get(1)) {
                    (Some(&(_, a)), Some(&(m, b))) if full && a - b < t.soften => paint_of(m),
                    _ => None,
                };
                (ground, runner)
            };
            // Each triangle chooses at its own centroid, so a boundary steps
            // along the diagonals as well as the quad edges.
            let third = 1.0 / (3.0 * res as f32);
            let tris = [
                choose(u - 0.5 / res as f32 + third, v - 0.5 / res as f32 + third),
                choose(u + 0.5 / res as f32 - third, v + 0.5 / res as f32 - third),
            ];
            let (ground, _) = choose(u, v);
            if let (Some(c), Some(slope)) = (&t.cliff, cliff_slope) {
                let s = rise / step;
                let jitter = (value_noise(t.seed ^ 0x434C_4946, u * 2.0, v * 2.0) - 0.5) * 0.3;
                let edge = slope * (1.0 + jitter);
                if s > edge && !c.cells.is_empty() {
                    let hsh = tile_hash(0x434C);
                    let q = Some(QuadPaint {
                        cell: c.cells[(hsh >> 8) as usize % c.cells.len()],
                        orient: (hsh & 7) as u8,
                        material: CLIFF,
                        // A face just past the threshold keeps a veil of the
                        // ground above it.
                        overlay: if t.soften > 0.0 && s < edge * 1.35 {
                            ground.map(|(_, p)| p)
                        } else {
                            None
                        },
                    });
                    paint.push([q, q]);
                    continue;
                }
            }
            let to_paint = |(ground, runner): Choice| {
                ground.map(|(m, (cell, orient))| QuadPaint {
                    cell,
                    orient,
                    material: m,
                    overlay: runner,
                })
            };
            if detail > 1 && full {
                let win = |a: usize, b: usize| vertex_win[b * cols + a];
                let mats = [
                    tris[0].0.map(|g| g.0),
                    tris[1].0.map(|g| g.0),
                    win(i, j),
                    win(i + 1, j),
                    win(i, j + 1),
                    win(i + 1, j + 1),
                ];
                if mats.iter().any(|m| *m != mats[0]) {
                    // Sub-quad (si, sj) of `detail` a side, its triangles
                    // at their own centroids, split as the quad is.
                    let d = detail as f32;
                    let q = 1.0 / (res as f32 * d);
                    let (u0, v0) = (u - 0.5 / res as f32, v - 0.5 / res as f32);
                    let mut subs = Vec::with_capacity(detail * detail);
                    for sj in 0..detail {
                        for si in 0..detail {
                            let (su, sv) = (u0 + si as f32 * q, v0 + sj as f32 * q);
                            let third = q / 3.0;
                            subs.push([
                                to_paint(choose(su + third, sv + third)),
                                to_paint(choose(su + q - third, sv + q - third)),
                            ]);
                        }
                    }
                    fine.insert(paint.len(), subs);
                }
            }
            paint.push(tris.map(to_paint));
        }
    }
    TerrainSurface {
        field,
        materials: mats,
        colors,
        paint,
        detail,
        fine,
        water: t.water.clone(),
    }
}

#[cfg(test)]
mod tests;
