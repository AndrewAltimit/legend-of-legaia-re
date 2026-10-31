//! The detail layer of a map (`maps/<id>.toml`): decorations painted in bulk,
//! animated props, lights, animated materials and fog.
//!
//! A tile holds one placed prop ([`crate::map::PropDef`]), which is the
//! retail object-cell rule. Retail rooms are dense because each cell's mesh
//! is itself a dense piece of art; a pack map gets the same density by
//! **decorations**: any number of meshes a tile, baked when the map loads
//! into one mesh per tile that draws through the tile's own object cell. A
//! modder paints them three ways, all of which expand to the same list
//! ([`crate::map::MapDef::decorations`]):
//!
//! - `[[decor]]`, one mesh at a time ([`DecorDef`]);
//! - `[[scatter]]`, a seeded brush over a tile rectangle ([`ScatterBrush`]);
//! - `[[edges]]`, a mesh repeated along the floor's wall edges
//!   ([`EdgeDecor`]).
//!
//! The expansion is deterministic: the same file always yields the same
//! decorations in the same order, on every host, with or without a disc.
//!
//! Everything else in the layer is presentation: [`Animator`]s turn and move
//! a prop's draw, [`MapLighting`] lights the map under the port's enhanced
//! lighting, [`MaterialFx`] animates texture words in VRAM (palette cycling,
//! texel scrolling, hue cycling), and [`MapFog`] arms the retail field fog
//! pool. None of it changes collision, zones or anything a save carries.

use serde::{Deserialize, Serialize};

use crate::map::{PropMesh, TileRect};

/// The most decorations one map expands to. Each lands in its tile's baked
/// mesh; the cap keeps a runaway brush from building a map that cannot load.
pub const MAX_DECORATIONS: usize = 8192;

/// The most decorations one tile carries.
pub const MAX_DECORATIONS_PER_TILE: usize = 32;

/// One decoration: a mesh standing on a tile, with no collision of its own.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DecorDef {
    /// Optional id, for the workbench.
    pub id: String,
    /// The mesh.
    pub mesh: PropMesh,
    /// Map-local tile.
    pub tile: [u16; 2],
    /// Offset from the tile centre, world units `[x, y, z]` (`y` up).
    pub offset: [i16; 3],
    /// Rotation `[x, y, z]` in degrees; `y` is the yaw.
    pub rotation: [f32; 3],
    /// Uniform scale; `1` when left out.
    pub scale: Option<f32>,
    /// Block the ground under the mesh: the lower half of its drawn hull
    /// closes the collision sub-cells it covers, as a solid prop's does.
    pub solid: bool,
}

/// A seeded brush: scatter a weighted set of meshes over a tile rectangle.
///
/// Each tile of `rect` that passes the filters ([`Self::on`],
/// [`Self::keep_clear`]) draws a count from `density` and places that many
/// meshes, each picked by weight, jittered off the tile centre, turned and
/// scaled inside the brush's ranges. The draw is a pure function of `seed`,
/// the brush's index and the tile, so moving one brush never reshuffles
/// another.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScatterBrush {
    pub id: String,
    /// Inclusive map-local tile rectangle `[col0, row0, col1, row1]`.
    pub rect: TileRect,
    /// The meshes the brush picks from.
    pub meshes: Vec<ScatterMesh>,
    /// Expected meshes a tile. The integer part is placed on every tile; the
    /// fraction is the chance of one more.
    pub density: f32,
    pub seed: u64,
    /// Largest distance from the tile centre on each axis, world units
    /// (`0..=64` keeps a mesh's origin on its tile).
    pub jitter: u16,
    /// Yaw range in degrees, `[min, max]`.
    pub yaw: [f32; 2],
    /// Uniform scale range, `[min, max]`.
    pub scale: [f32; 2],
    /// Largest tilt off upright, degrees (pitch and roll each).
    pub tilt: f32,
    /// Legend characters the brush paints on. Empty paints every floor tile
    /// (not void, not fully walled).
    pub on: String,
    /// Leave the tiles a player needs clear: spawns, exit tiles, NPC tiles
    /// and patrol stops, and placed props.
    pub keep_clear: bool,
    /// Thin the brush away from a centre: dense in the middle, sparse at
    /// the rim ([`Cluster`]). `None` paints the rectangle evenly.
    pub cluster: Option<Cluster>,
    /// Thin the brush round the walkable floor so the player and the path
    /// read through it from the follow camera ([`Clearance`]). `None`
    /// paints up to the path's edge.
    pub clearance: Option<Clearance>,
    /// Every mesh the brush places blocks the ground under it
    /// ([`DecorDef::solid`]): an orchard the player walks round.
    pub solid: bool,
}

/// A [`ScatterBrush`]'s clearance round the walkable floor (every tile
/// that is not void and not fully walled). The follow camera stands on a
/// map's `-Z` side (rows shrinking, the top of the painter's grid) looking
/// toward `+Z`, so what stands on that side of the path is what covers it: the band is `radius` tiles
/// wide to the sides and behind a walkable tile and `front` tiles deep
/// toward the camera.
///
/// A tile's reach `e` is its least distance to a walkable tile centre,
/// each axis over the band's half-width on that side (`front` for a tile
/// on the camera side of the walkable one, `radius` otherwise). Up to
/// `e = 1` the brush keeps `keep` of its density; from there it ramps back
/// to full by `e = 1.5`, so the edge of a clearing is a thinning, not a
/// line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Clearance {
    /// Tiles to the sides of and behind the walkable floor.
    pub radius: f32,
    /// Tiles toward the camera (`-Z`, rows shrinking).
    pub front: f32,
    /// The fraction of the density kept inside the band (`0` = none).
    pub keep: f32,
}

impl Default for Clearance {
    fn default() -> Self {
        Self {
            radius: 1.0,
            front: 3.0,
            keep: 0.0,
        }
    }
}

/// The density factor [`Clearance`] gives tile `(col, row)`, with
/// `walkable` answering whether a map-local tile is walkable floor.
pub fn clearance_weight(
    k: &Clearance,
    col: u16,
    row: u16,
    walkable: impl Fn(i32, i32) -> bool,
) -> f32 {
    let radius = k.radius.max(0.01);
    let front = k.front.max(0.01);
    // Past `e = 1.5` nothing thins, so only that far needs looking at.
    let reach = (radius.max(front) * 1.5).ceil() as i32;
    let (c, r) = (i32::from(col), i32::from(row));
    let mut e = f32::INFINITY;
    for dr in -reach..=reach {
        for dc in -reach..=reach {
            if !walkable(c + dc, r + dr) {
                continue;
            }
            // `dr` runs from this tile to the walkable one: positive is
            // this tile on the walkable one's `-Z` side, the camera's.
            let dz = dr as f32;
            let along = if dz > 0.0 { front } else { radius };
            let d = ((dc as f32) / radius).hypot(dz / along);
            e = e.min(d);
        }
    }
    let keep = k.keep.clamp(0.0, 1.0);
    if e <= 1.0 {
        keep
    } else if e < 1.5 {
        keep + (1.0 - keep) * (e - 1.0) / 0.5
    } else {
        1.0
    }
}

/// A [`ScatterBrush`]'s falloff: the density at a tile `d` tiles from
/// `centre` is `density * (1 - d / radius) ^ falloff`, nothing past
/// `radius`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cluster {
    /// Map-local tile coordinates (fractional allowed); `None` is the
    /// rectangle's centre.
    pub centre: Option<[f32; 2]>,
    /// Tiles; `0` reaches the rectangle's farthest corner.
    pub radius: f32,
    pub falloff: f32,
}

impl Default for Cluster {
    fn default() -> Self {
        Self {
            centre: None,
            radius: 0.0,
            falloff: 2.0,
        }
    }
}

impl Default for ScatterBrush {
    fn default() -> Self {
        Self {
            id: String::new(),
            rect: [0; 4],
            meshes: Vec::new(),
            density: 1.0,
            seed: 0,
            jitter: 40,
            yaw: [0.0, 360.0],
            scale: [1.0, 1.0],
            tilt: 0.0,
            on: String::new(),
            keep_clear: true,
            cluster: None,
            clearance: None,
            solid: false,
        }
    }
}

/// One mesh of a [`ScatterBrush`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScatterMesh {
    pub mesh: PropMesh,
    /// Relative pick weight.
    pub weight: u32,
    /// This mesh's own scale range, overriding the brush's.
    pub scale: Option<[f32; 2]>,
    /// Height above the floor, world units (up positive).
    pub y: i16,
}

impl Default for ScatterMesh {
    fn default() -> Self {
        Self {
            mesh: PropMesh::default(),
            weight: 1,
            scale: None,
            y: 0,
        }
    }
}

/// A mesh repeated along the floor's wall edges: torches on every second
/// stretch of wall, a skirting of rubble, banners along a hall.
///
/// A floor tile ([`Self::on`]) that has a wall tile ([`Self::walls`]) as a
/// side neighbour gets one copy per walled side, pushed against that side
/// ([`Self::inset`] from the tile edge) and turned to face into the room.
/// [`Self::every`] thins the run: a copy is placed where
/// `(col + row + phase) % every == 0`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EdgeDecor {
    pub id: String,
    pub mesh: PropMesh,
    /// Legend characters that count as wall. Empty counts every fully
    /// walled or void tile, and the map's own border.
    pub walls: String,
    /// Legend characters of the floor the copies stand on. Empty is every
    /// floor tile.
    pub on: String,
    /// Only tiles inside this rectangle; `None` is the whole map.
    pub rect: Option<TileRect>,
    /// Place on every `every`th edge tile (at least 1).
    pub every: u16,
    pub phase: u16,
    /// Distance from the wall face toward the room, world units.
    pub inset: i16,
    /// Height above the floor, world units (up positive).
    pub y: i16,
    /// Extra yaw in degrees on top of facing into the room. A mesh whose
    /// front faces `+Z` needs none.
    pub yaw: f32,
    pub scale: Option<f32>,
    /// Organic placement: each copy slides up to `jitter` world units along
    /// its wall and a quarter of that toward the room, turns up to
    /// `yaw_jitter` degrees either way and scales inside `scale_range`
    /// (overriding `scale`), seeded by `seed` and the tile.
    pub jitter: u16,
    pub yaw_jitter: f32,
    pub scale_range: Option<[f32; 2]>,
    pub seed: u64,
    /// Each copy blocks the ground under it ([`DecorDef::solid`]).
    pub solid: bool,
}

impl Default for EdgeDecor {
    fn default() -> Self {
        Self {
            id: String::new(),
            mesh: PropMesh::default(),
            walls: String::new(),
            on: String::new(),
            rect: None,
            every: 1,
            phase: 0,
            inset: 8,
            y: 0,
            yaw: 0.0,
            scale: None,
            jitter: 0,
            yaw_jitter: 0.0,
            scale_range: None,
            seed: 0,
            solid: false,
        }
    }
}

/// A decal: a flat textured polygon lying on the floor - a crack, a moss
/// patch, a stain, a rune - cut from one `32 x 32` atlas cell of the base
/// scene's VRAM. A decal is a decoration (baked into its tile's cell mesh),
/// so it draws only.
///
/// With a `tile` the decal is placed once; with a `rect` it is a brush,
/// scattered like a [`ScatterBrush`] at `density` a tile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DecalDef {
    pub id: String,
    /// The texture (the three words a ground texture carries).
    pub cell: AtlasCell,
    /// Further cells a brush picks from at random, beside `cell`.
    pub cells: Vec<AtlasCell>,
    /// Atlas cells a side the texture spans, from `cell` right and down
    /// (`2` maps a `64 x 64` texel square - a rune circle drawn over four
    /// cells).
    pub span: u8,
    pub tile: Option<[u16; 2]>,
    pub rect: Option<TileRect>,
    pub density: f32,
    pub seed: u64,
    /// Legend characters a brush paints on; empty is every floor tile.
    pub on: String,
    /// Offset from the tile centre, world units `[x, y, z]` (`y` up); a
    /// brush jitters `x` / `z` by up to `jitter`.
    pub offset: [i16; 3],
    pub jitter: u16,
    /// Width and depth, world units.
    pub size: [u16; 2],
    /// A brush scales each decal by `1 +- size_jitter`.
    pub size_jitter: f32,
    /// Yaw range in degrees (a single decal takes `yaw[0]`).
    pub yaw: [f32; 2],
    /// `4` is a rectangle; `5..=16` an inscribed polygon (a round stain, an
    /// octagonal cap).
    pub sides: u8,
    /// Modulation colour: `[128, 128, 128]` draws the texel as it is.
    pub color: [u8; 3],
    /// Blend with the floor (the texture page's blend mode), for stains and
    /// glows.
    pub semi: bool,
}

impl Default for DecalDef {
    fn default() -> Self {
        Self {
            id: String::new(),
            cell: AtlasCell::default(),
            cells: Vec::new(),
            span: 1,
            tile: None,
            rect: None,
            density: 0.3,
            seed: 0,
            on: String::new(),
            offset: [0, 2, 0],
            jitter: 32,
            size: [96, 96],
            size_jitter: 0.0,
            yaw: [0.0, 360.0],
            sides: 4,
            color: [128, 128, 128],
            semi: false,
        }
    }
}

/// One decal as the engine bakes it ([`crate::map::MapDef::decals`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Decal {
    pub tile: [u16; 2],
    /// Offset from the tile centre (`y` up).
    pub offset: [f32; 3],
    pub yaw: f32,
    pub size: [f32; 2],
    pub sides: u8,
    pub cell: AtlasCell,
    /// Atlas cells a side ([`DecalDef::span`]).
    pub span: u8,
    pub color: [u8; 3],
    pub semi: bool,
}

/// What a particle emitter throws ([`EmitterDef::kind`]); each kind is a
/// preset of the emitter's other fields, any of which the file overrides.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParticleKind {
    /// Glowing sparks rising and fading.
    #[default]
    Embers,
    /// Twinkling points hanging in the air.
    Sparkles,
    /// Slow motes drifting in a beam of light.
    Dust,
    /// Tumbling leaves falling and swaying.
    Leaves,
    /// Drops falling straight down.
    Drips,
    /// Soft puffs rising and swelling.
    Smoke,
}

/// A particle emitter: `count` particles live at once over a tile
/// rectangle, each a camera-facing sprite drawn through the field
/// screen-effect pass the fog sheets use. A particle's whole life is a pure
/// function of `seed`, its index and the frame - no state, no random
/// stream - so it is the same on both hosts and in every run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmitterDef {
    pub id: String,
    pub kind: ParticleKind,
    /// Inclusive map-local tile rectangle particles spawn over.
    pub rect: TileRect,
    /// Spawn height range above the floor, world units.
    pub height: Option<[i16; 2]>,
    pub count: Option<u16>,
    /// Frames a particle lives.
    pub life: Option<i32>,
    /// World units a frame, `y` up.
    pub velocity: Option<[f32; 3]>,
    /// Random part of the velocity, either way, per axis.
    pub spread: Option<[f32; 3]>,
    /// Added to the `y` velocity each frame (negative falls).
    pub gravity: Option<f32>,
    /// Side-to-side drift amplitude, world units.
    pub sway: Option<f32>,
    /// Sprite size at birth and death, world units.
    pub size: Option<[u16; 2]>,
    pub color: Option<[u8; 3]>,
    /// Colour at death; `None` keeps `color`.
    pub color_end: Option<[u8; 3]>,
    /// Additive blend (glows); otherwise averaged with the scene.
    pub additive: Option<bool>,
    pub seed: u64,
}

/// An [`EmitterDef`] with its kind's preset filled in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Emitter {
    pub kind: ParticleKind,
    pub rect: TileRect,
    pub height: [i16; 2],
    pub count: u16,
    pub life: i32,
    pub velocity: [f32; 3],
    pub spread: [f32; 3],
    pub gravity: f32,
    pub sway: f32,
    pub size: [u16; 2],
    pub color: [u8; 3],
    pub color_end: [u8; 3],
    pub additive: bool,
    pub seed: u64,
}

/// Where a map's particle sprites sit in VRAM, written by the map's load
/// (`legaia_engine_scene::expansion_map::detail::particle_sheet`): four
/// `16 x 16` 4bpp sprites side by side from `(u0, v0)` of page `tpage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParticleSheet {
    /// TSB page bits (4bpp, blend mode clear).
    pub tpage: u16,
    /// The grey ramp with STP set: the sprite blends.
    pub clut_glow: u16,
    /// The grey ramp without STP: the sprite draws opaque.
    pub clut_solid: u16,
    pub u0: u8,
    pub v0: u8,
}

/// The sprite column of each kind in a [`ParticleSheet`].
pub const SPRITE_DOT: u8 = 0;
pub const SPRITE_SPARK: u8 = 1;
pub const SPRITE_LEAF: u8 = 2;
pub const SPRITE_DROP: u8 = 3;

impl ParticleKind {
    /// The sprite the kind draws.
    pub fn sprite(self) -> u8 {
        match self {
            Self::Embers | Self::Dust | Self::Smoke => SPRITE_DOT,
            Self::Sparkles => SPRITE_SPARK,
            Self::Leaves => SPRITE_LEAF,
            Self::Drips => SPRITE_DROP,
        }
    }
}

/// One particle on one frame: where it is (map frame, `y` up above the
/// spawn floor), how large, what colour and how bright.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleState {
    /// World `x` / `z` and the height above the floor at its spawn point.
    pub pos: [f32; 3],
    pub size: f32,
    pub color: [u8; 3],
}

impl Emitter {
    /// Particle `i` of emitter number `e` on `frame`, or `None` while it is
    /// not alive (the first life of each particle starts at a staggered
    /// frame so the emitter does not breathe in step). `centre` maps a
    /// map-local tile to its world centre.
    pub fn particle(
        &self,
        e: usize,
        i: u16,
        frame: u64,
        centre: impl Fn(u16, u16) -> (i32, i32),
    ) -> Option<ParticleState> {
        let life = self.life.max(1) as u64;
        let stagger = splitmix64(self.seed ^ ((e as u64) << 40) ^ u64::from(i)) % life;
        let t = frame + stagger;
        let cycle = t / life;
        let age = (t % life) as f32;
        let mut r = TileRng::new(
            self.seed ^ cycle.wrapping_mul(0x2545_F491_4F6C_DD1D),
            e,
            i,
            0x7A11,
        );
        let (c0, c1) = (
            self.rect[0].min(self.rect[2]),
            self.rect[0].max(self.rect[2]),
        );
        let (r0, r1) = (
            self.rect[1].min(self.rect[3]),
            self.rect[1].max(self.rect[3]),
        );
        let (x0, z0) = centre(c0, r0);
        let (x1, z1) = centre(c1, r1);
        let x = r.range(x0 as f32 - 64.0, x1 as f32 + 64.0);
        let z = r.range(z0 as f32 - 64.0, z1 as f32 + 64.0);
        let y = r.range(f32::from(self.height[0]), f32::from(self.height[1]));
        let v = [0, 1, 2].map(|k| self.velocity[k] + r.range(-self.spread[k], self.spread[k]));
        let ph = r.range(0.0, std::f32::consts::TAU);
        let sway = self.sway * (age * 0.045 + ph).sin();
        let sway_z = self.sway * (age * 0.037 + ph * 1.3).cos() * 0.6;
        let pos = [
            x + v[0] * age + sway,
            y + v[1] * age + 0.5 * self.gravity * age * age,
            z + v[2] * age + sway_z,
        ];
        let f = age / life as f32;
        let lerp = |a: f32, b: f32| a + (b - a) * f;
        // Glows fade in and out; solid particles hold their colour.
        let fade = if self.additive {
            (f * std::f32::consts::PI).sin()
        } else {
            1.0
        };
        let color = [0, 1, 2].map(|k| {
            (lerp(f32::from(self.color[k]), f32::from(self.color_end[k])) * fade).round() as u8
        });
        Some(ParticleState {
            pos,
            size: lerp(f32::from(self.size[0]), f32::from(self.size[1])),
            color,
        })
    }
}

/// The most particles one emitter keeps alive.
pub const MAX_PARTICLES: u16 = 512;

impl EmitterDef {
    /// The emitter with its kind's preset under the file's own values.
    pub fn resolved(&self) -> Emitter {
        use ParticleKind::*;
        // (height, count, life, velocity, spread, gravity, sway, size, color, end, additive)
        type Preset = (
            [i16; 2],
            u16,
            i32,
            [f32; 3],
            [f32; 3],
            f32,
            f32,
            [u16; 2],
            [u8; 3],
            [u8; 3],
            bool,
        );
        let p: Preset = match self.kind {
            Embers => (
                [0, 48],
                40,
                110,
                [0.0, 1.1, 0.0],
                [0.4, 0.5, 0.4],
                0.0,
                10.0,
                [16, 6],
                [255, 150, 60],
                [160, 30, 10],
                true,
            ),
            Sparkles => (
                [24, 180],
                32,
                70,
                [0.0, 0.15, 0.0],
                [0.15, 0.15, 0.15],
                0.0,
                4.0,
                [16, 16],
                [170, 200, 255],
                [170, 200, 255],
                true,
            ),
            Dust => (
                [16, 200],
                48,
                260,
                [0.0, 0.05, 0.0],
                [0.25, 0.1, 0.25],
                0.0,
                14.0,
                [9, 9],
                [190, 180, 150],
                [190, 180, 150],
                true,
            ),
            Leaves => (
                [220, 320],
                18,
                300,
                [0.2, -0.8, 0.1],
                [0.3, 0.2, 0.3],
                0.0,
                40.0,
                [14, 14],
                [150, 170, 70],
                [170, 120, 50],
                false,
            ),
            Drips => (
                [260, 320],
                16,
                70,
                [0.0, -2.5, 0.0],
                [0.0, 0.5, 0.0],
                -0.08,
                0.0,
                [6, 6],
                [140, 180, 220],
                [140, 180, 220],
                true,
            ),
            Smoke => (
                [0, 24],
                20,
                220,
                [0.0, 0.6, 0.0],
                [0.3, 0.2, 0.3],
                0.0,
                18.0,
                [40, 110],
                [60, 58, 54],
                [24, 23, 22],
                true,
            ),
        };
        let color = self.color.unwrap_or(p.8);
        Emitter {
            kind: self.kind,
            rect: self.rect,
            height: self.height.unwrap_or(p.0),
            count: self.count.unwrap_or(p.1).min(MAX_PARTICLES),
            life: self.life.unwrap_or(p.2).max(1),
            velocity: self.velocity.unwrap_or(p.3),
            spread: self.spread.unwrap_or(p.4),
            gravity: self.gravity.unwrap_or(p.5),
            sway: self.sway.unwrap_or(p.6),
            size: self.size.unwrap_or(p.7),
            color,
            color_end: self
                .color_end
                .unwrap_or(if self.color.is_some() { color } else { p.9 }),
            additive: self.additive.unwrap_or(p.10),
            seed: self.seed,
        }
    }
}

/// An axis of a prop's own frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    #[default]
    Y,
    Z,
}

/// A procedural animation of a prop's draw. Every animator is a pure
/// function of the world's frame counter (`World::frame`), so two hosts and
/// two runs draw the same pose on the same frame. A prop's animators compose
/// in list order.
///
/// `period` is in frames (world ticks) and must be at least 1; `phase` is a
/// fraction of the period (`0.25` starts a quarter cycle in), which is how a
/// row of torches avoids flickering in step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Animator {
    /// A full turn about `axis` every `period` frames; a negative period
    /// turns the other way.
    Spin {
        #[serde(default)]
        axis: Axis,
        period: i32,
        #[serde(default)]
        phase: f32,
    },
    /// Rock `degrees` either side of rest about `axis`.
    Sway {
        #[serde(default = "axis_z")]
        axis: Axis,
        degrees: f32,
        period: i32,
        #[serde(default)]
        phase: f32,
    },
    /// Rise and fall `amplitude` world units.
    Bob {
        amplitude: i16,
        period: i32,
        #[serde(default)]
        phase: f32,
    },
    /// Circle `radius` world units about the rest position, in the floor
    /// plane; a negative period circles the other way.
    Orbit {
        radius: i16,
        period: i32,
        #[serde(default)]
        phase: f32,
    },
    /// Breathe: scale between `1 - amount` and `1 + amount`.
    Pulse {
        amount: f32,
        period: i32,
        #[serde(default)]
        phase: f32,
    },
}

fn axis_z() -> Axis {
    Axis::Z
}

impl Animator {
    /// The animator's period in frames.
    pub fn period(&self) -> i32 {
        match *self {
            Self::Spin { period, .. }
            | Self::Sway { period, .. }
            | Self::Bob { period, .. }
            | Self::Orbit { period, .. }
            | Self::Pulse { period, .. } => period,
        }
    }
}

/// The enhanced-lighting moods a map can ask for: the presets of
/// `legaia_render_kernels::scene_lighting::LightingMood`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoodPreset {
    Day,
    Dusk,
    Night,
    Cave,
}

/// How enhanced lighting lights the map. Retail has no light source, so
/// none of this draws in the faithful mode (enhanced lighting off): the map
/// then shows its baked texel colours, as a retail room does.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapLighting {
    /// The mood under the time-of-day setting `Auto`. `None` takes the base
    /// scene's. A time of day the player picks (`F8`) still wins.
    pub mood: Option<MoodPreset>,
    /// Point lights placed on the map.
    pub lamps: Vec<LampDef>,
}

/// A placed point light.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LampDef {
    pub id: String,
    /// Map-local tile.
    pub tile: [u16; 2],
    /// Offset from the tile centre on its floor, world units (`y` up).
    pub offset: [i16; 3],
    /// Light colour, 8 bits a channel.
    pub color: [u8; 3],
    /// Reach in world units.
    pub radius: u16,
    /// Flicker depth `0..=1`: the light's strength wavers by this much.
    pub flicker: f32,
    /// Flicker speed: frames per waver.
    pub period: i32,
}

impl Default for LampDef {
    fn default() -> Self {
        Self {
            id: String::new(),
            tile: [0, 0],
            offset: [0, 96, 0],
            color: [255, 176, 96],
            radius: 640,
            flicker: 0.0,
            period: 24,
        }
    }
}

/// A light carried by a prop: a [`LampDef`] that follows the prop's tile and
/// offset, `height` above it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PropLight {
    pub color: [u8; 3],
    pub radius: u16,
    /// Height above the prop's origin, world units (up positive).
    pub height: i16,
    pub flicker: f32,
    pub period: i32,
}

impl Default for PropLight {
    fn default() -> Self {
        let l = LampDef::default();
        Self {
            color: l.color,
            radius: l.radius,
            height: l.offset[1],
            flicker: 0.0,
            period: l.period,
        }
    }
}

/// One `32 x 32` cell of a texture page's `8 x 8` atlas through a palette:
/// the three words a [`crate::map::Ground::Texture`] carries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtlasCell {
    pub tpage: u8,
    pub clut: u16,
    pub tile: u8,
}

/// An animated material: a per-frame rewrite of texture words in the scene's
/// VRAM. Every prim that samples the words animates with them - a ground
/// texture, a wall box, a retail mesh of the base scene - on both hosts,
/// which re-upload VRAM when it changes. It is the mechanism the retail
/// field's own water, lava and lights run on (the CLUT-walk tables and the
/// mode-3 / mode-4 move-VM cyclers, `docs/subsystems/field-ambient-fx.md`).
///
/// `period` is frames per step (at least 1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MaterialFx {
    /// Palette cycling: rotate entries `start .. start + count` of palette
    /// `clut` one place every `period` frames (water ripples, lava churn,
    /// conveyor lights). `reverse` rotates the other way.
    Cycle {
        clut: u16,
        start: u8,
        count: u16,
        period: i32,
        #[serde(default)]
        reverse: bool,
        /// Name the palette in this scene's VRAM instead of the base
        /// scene's: a palette a mesh carried in from that scene samples,
        /// resolved to where the map's load copied it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
    },
    /// Hue and brightness cycling of palette entries, through the retail
    /// mode-3 HSV cell kernel (`FUN_80019D50`): the hue turns a full circle
    /// every `period` frames and the brightness swings `value` (`0..=255`)
    /// either side of the art.
    Hsv {
        clut: u16,
        /// As [`MaterialFx::Cycle`]'s `from`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
        #[serde(default)]
        start: u8,
        #[serde(default = "sixteen")]
        count: u16,
        period: i32,
        #[serde(default)]
        value: i16,
        /// Hue sweep in degrees per period; `360` is a full circle, `0`
        /// keeps the hue and pulses only the brightness.
        #[serde(default = "full_turn")]
        hue: f32,
    },
    /// A standing recolour of palette entries, applied once: a static hue
    /// turn and brightness shift through the same HSV kernel as
    /// [`MaterialFx::Hsv`], then a per-channel multiply `color` (`128` =
    /// unchanged, `255` about doubles). Re-colours a retail mesh's art to
    /// the map's mood - a kept room's torch flames, a carried-in crystal.
    Tint {
        clut: u16,
        /// As [`MaterialFx::Cycle`]'s `from`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
        #[serde(default)]
        start: u8,
        #[serde(default = "sixteen")]
        count: u16,
        /// Hue turn in degrees.
        #[serde(default)]
        hue: f32,
        /// Brightness shift, `-255..=255`.
        #[serde(default)]
        value: i16,
        /// Per-channel multiply `[r, g, b]`, `128` = unchanged.
        #[serde(default = "neutral_tint")]
        color: [u8; 3],
    },
    /// Texel scrolling: rotate an atlas cell's texels `dx` / `dy` whole
    /// texels every `period` frames, wrapping - flowing water, a moving
    /// energy floor. (Retail's mode-4 rotator `FUN_80021DF4` moves halfword
    /// strips; this moves texels, so a 4bpp cell scrolls one texel at a
    /// time.)
    Scroll {
        cell: AtlasCell,
        #[serde(default)]
        dx: i8,
        #[serde(default)]
        dy: i8,
        period: i32,
        /// Name the cell in this scene's VRAM instead of the base scene's
        /// (a texture a carried-in mesh samples, resolved to its copy).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
        /// An explicit VRAM rectangle `[x, y, w, h]` in halfwords, standing in
        /// for `cell`'s; the map's load writes it for a `from` cell. The page
        /// depth stays `cell.tpage`'s.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rect: Option<[u16; 4]>,
    },
}

fn sixteen() -> u16 {
    16
}

fn full_turn() -> f32 {
    360.0
}

fn neutral_tint() -> [u8; 3] {
    [128; 3]
}

impl MaterialFx {
    /// The effect's period in frames; a standing [`MaterialFx::Tint`]
    /// has none and reports `1`.
    pub fn period(&self) -> i32 {
        match *self {
            Self::Cycle { period, .. } | Self::Hsv { period, .. } | Self::Scroll { period, .. } => {
                period
            }
            Self::Tint { .. } => 1,
        }
    }
}

/// The retail field fog pool, armed over the map: drifting wisps spawned in
/// tile regions, the system a retail scene's MAN section 4 drives
/// (`docs/subsystems/field-ambient-fx.md`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapFog {
    pub regions: Vec<FogRegionDef>,
}

/// One fog spawn region.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FogRegionDef {
    /// Inclusive map-local tile rectangle.
    pub rect: TileRect,
    /// Drift direction in degrees (`0` = `+Z`).
    pub angle: f32,
    /// Random spread of the drift direction, degrees either side.
    pub spread: f32,
    /// Drift speed (`0..=255`, the retail record's `+7`).
    pub speed: u8,
}

impl Default for FogRegionDef {
    fn default() -> Self {
        Self {
            rect: [0; 4],
            angle: 0.0,
            spread: 90.0,
            speed: 24,
        }
    }
}

/// The tiles past the floor's edge to which a surround reaches when the
/// map names none: enough to fill the follow camera's widest preset.
pub const SURROUND_DEFAULT_REACH: u8 = 8;

/// The most tiles a surround reaches past the map's edges.
pub const SURROUND_MAX_REACH: u8 = 16;

/// What a map built with no base geometry shows past its floor: a flat
/// ground `depth` world units below the floor's edge on every void tile and
/// every tile up to `reach` past the map's edges, and, with `side`, the
/// floor's own edge faces dropping to it. Without one the view past the
/// floor is the clear colour, which a follow camera at a map's edge frames
/// as a black void.
///
/// ```toml
/// [surround]
/// ground = { tpage = 12, clut = 32576, tile = 8 }
/// side = { tpage = 11, clut = 32523, tile = 38 }
/// depth = 384
/// reach = 8
/// shade = 72
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SurroundDef {
    /// The drop's ground: one atlas cell a tile.
    pub ground: AtlasCell,
    /// The floor edge's face from the floor down to the drop. `None` leaves
    /// the edge open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side: Option<AtlasCell>,
    /// How far below the floor's edge the drop lies, world units.
    pub depth: u16,
    /// Tiles past the map's edges the drop reaches.
    pub reach: u8,
    /// Vertex colour of the drop and its faces (`128` draws the texel as
    /// is; lower darkens it, the retail `texel * colour / 128` law).
    pub shade: u8,
}

impl Default for SurroundDef {
    fn default() -> Self {
        Self {
            ground: AtlasCell::default(),
            side: None,
            depth: 384,
            reach: SURROUND_DEFAULT_REACH,
            shade: 80,
        }
    }
}

/// One decoration as the engine bakes it: a mesh on a tile at a transform.
/// [`crate::map::MapDef::decorations`] expands every `[[decor]]`,
/// `[[scatter]]` and `[[edges]]` entry into these.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoration {
    pub mesh: PropMesh,
    /// Map-local tile the decoration's cell is.
    pub tile: [u16; 2],
    /// Offset from the tile centre, world units (`y` up).
    pub offset: [f32; 3],
    /// Degrees `[x, y, z]`.
    pub rotation: [f32; 3],
    pub scale: f32,
    /// Whether it blocks the ground under it ([`DecorDef::solid`]).
    pub solid: bool,
}

/// A [`Cluster`]'s density factor at map-local tile `(col, row)` of a
/// brush over `rect`.
pub fn cluster_weight(k: &Cluster, rect: &TileRect, col: u16, row: u16) -> f32 {
    let (c0, c1) = (
        f32::from(rect[0].min(rect[2])),
        f32::from(rect[0].max(rect[2])),
    );
    let (r0, r1) = (
        f32::from(rect[1].min(rect[3])),
        f32::from(rect[1].max(rect[3])),
    );
    let [cx, cz] = k.centre.unwrap_or([(c0 + c1) / 2.0, (r0 + r1) / 2.0]);
    let radius = if k.radius > 0.0 {
        k.radius
    } else {
        let far = |a: f32, b: f32, c: f32| (a - c).abs().max((b - c).abs());
        far(c0, c1, cx).hypot(far(r0, r1, cz)) + 0.5
    };
    let d = (f32::from(col) - cx).hypot(f32::from(row) - cz);
    (1.0 - d / radius).max(0.0).powf(k.falloff.max(0.0))
}

/// SplitMix64: the brush's deterministic stream. One step of it is a full
/// avalanche of its input, so seeding it per tile keeps tiles independent.
pub fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A brush's random stream for one tile.
pub struct TileRng(u64);

impl TileRng {
    pub fn new(seed: u64, brush: usize, col: u16, row: u16) -> Self {
        let key = seed
            ^ (brush as u64).wrapping_mul(0xA24B_AED4_963E_E407)
            ^ (u64::from(col) << 32 | u64::from(row)).wrapping_mul(0x9FB2_1C65_1E98_DF25);
        Self(splitmix64(key))
    }

    /// Next value in `0..1`.
    pub fn unit(&mut self) -> f32 {
        self.0 = splitmix64(self.0);
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Next value in `lo..=hi` (either order).
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

#[cfg(test)]
mod tests;

impl crate::map::MapDef {
    /// Every decoration the map draws: its `[[decor]]` entries, then each
    /// `[[scatter]]` brush's tiles, then each `[[edges]]` run, in that
    /// order. A pure function of the file.
    ///
    /// Fails when the tiles do not resolve, a brush has no meshes or a zero
    /// total weight, or the map expands past [`MAX_DECORATIONS`] (or a tile
    /// past [`MAX_DECORATIONS_PER_TILE`]).
    pub fn decorations(&self) -> anyhow::Result<Vec<Decoration>> {
        use anyhow::bail;
        let grid = self.tile_grid()?;
        let mut out: Vec<Decoration> = self
            .decor
            .iter()
            .map(|d| Decoration {
                mesh: d.mesh.clone(),
                tile: d.tile,
                offset: d.offset.map(f32::from),
                rotation: d.rotation,
                scale: d.scale.unwrap_or(1.0),
                solid: d.solid,
            })
            .collect();
        let floor = |c: usize, r: usize| grid.get(c, r).is_some_and(|t| !t.void && t.walls != 0xF);
        let char_ok = |set: &str, c: usize, r: usize| {
            set.is_empty() || grid.char_at(c, r).is_some_and(|ch| set.contains(ch))
        };
        let clear = self.clear_tiles();
        for (b, brush) in self.scatter.iter().enumerate() {
            if brush.meshes.is_empty() {
                bail!("scatter[{b}] has no meshes");
            }
            let total: u64 = brush.meshes.iter().map(|m| u64::from(m.weight)).sum();
            if total == 0 {
                bail!("scatter[{b}]'s meshes weigh nothing");
            }
            let (c0, c1) = (
                brush.rect[0].min(brush.rect[2]),
                brush.rect[0].max(brush.rect[2]),
            );
            let (r0, r1) = (
                brush.rect[1].min(brush.rect[3]),
                brush.rect[1].max(brush.rect[3]),
            );
            let density = brush.density.max(0.0);
            for row in r0..=r1 {
                for col in c0..=c1 {
                    let (c, r) = (usize::from(col), usize::from(row));
                    // A brush paints floor tiles; a walled tile only when its
                    // `on` names the tile's character outright (a forest's
                    // thicket, the trees standing on the tiles that wall the
                    // path in).
                    let named = !brush.on.is_empty() && char_ok(&brush.on, c, r);
                    let walled_ok = named && grid.get(c, r).is_some_and(|t| !t.void);
                    if !(floor(c, r) || walled_ok) || !char_ok(&brush.on, c, r) {
                        continue;
                    }
                    if brush.keep_clear && clear.contains(&[col, row]) {
                        continue;
                    }
                    let mut rng = TileRng::new(brush.seed, b, col, row);
                    let density = match &brush.cluster {
                        None => density,
                        Some(k) => density * cluster_weight(k, &brush.rect, col, row),
                    };
                    let density = match &brush.clearance {
                        None => density,
                        Some(k) => {
                            density
                                * clearance_weight(k, col, row, |c, r| {
                                    c >= 0 && r >= 0 && floor(c as usize, r as usize)
                                })
                        }
                    };
                    let n = density.trunc() as u32 + u32::from(rng.unit() < density.fract());
                    let jitter = f32::from(brush.jitter.min(256));
                    for _ in 0..n {
                        let mut pick = (rng.unit() * total as f32) as u64;
                        let m = brush
                            .meshes
                            .iter()
                            .find(|m| {
                                let w = u64::from(m.weight);
                                if pick < w {
                                    true
                                } else {
                                    pick -= w;
                                    false
                                }
                            })
                            .unwrap_or(&brush.meshes[brush.meshes.len() - 1]);
                        let [s0, s1] = m.scale.unwrap_or(brush.scale);
                        let dx = rng.range(-jitter, jitter);
                        let dz = rng.range(-jitter, jitter);
                        let yaw = rng.range(brush.yaw[0], brush.yaw[1]);
                        let pitch = rng.range(-brush.tilt, brush.tilt);
                        let roll = rng.range(-brush.tilt, brush.tilt);
                        let scale = rng.range(s0, s1);
                        out.push(Decoration {
                            mesh: m.mesh.clone(),
                            tile: [col, row],
                            offset: [dx, f32::from(m.y), dz],
                            rotation: [pitch, yaw, roll],
                            scale,
                            solid: brush.solid,
                        });
                    }
                }
            }
        }
        for (e, edge) in self.edges.iter().enumerate() {
            let every = u32::from(edge.every.max(1));
            let wall = |c: isize, r: isize| -> bool {
                if c < 0 || r < 0 {
                    return edge.walls.is_empty();
                }
                let (c, r) = (c as usize, r as usize);
                match grid.get(c, r) {
                    None => edge.walls.is_empty(),
                    Some(t) if edge.walls.is_empty() => t.void || t.walls == 0xF,
                    Some(_) => char_ok(&edge.walls, c, r),
                }
            };
            for row in 0..grid.height {
                for col in 0..grid.width {
                    if !floor(col, row) || !char_ok(&edge.on, col, row) {
                        continue;
                    }
                    let (mc, mr) = (col as u16, row as u16);
                    if let Some(rect) = &edge.rect
                        && !crate::map::rect_contains(rect, mc, mr)
                    {
                        continue;
                    }
                    if !(col as u32 + row as u32 + u32::from(edge.phase)).is_multiple_of(every) {
                        continue;
                    }
                    let (c, r) = (col as isize, row as isize);
                    // (neighbour, push toward it, yaw facing away from it)
                    let sides = [
                        ((c, r - 1), [0.0, -1.0], 0.0f32),
                        ((c, r + 1), [0.0, 1.0], 180.0),
                        ((c - 1, r), [-1.0, 0.0], 90.0),
                        ((c + 1, r), [1.0, 0.0], 270.0),
                    ];
                    let reach = 64.0 - f32::from(edge.inset);
                    let mut rng = TileRng::new(edge.seed, usize::MAX - e, mc, mr);
                    let j = f32::from(edge.jitter);
                    for ((nc, nr), [px, pz], yaw) in sides {
                        if !wall(nc, nr) {
                            continue;
                        }
                        // Along the wall is the axis the push is not on.
                        let along = rng.range(-j, j);
                        let inward = rng.range(0.0, j / 4.0);
                        let turn = rng.range(-edge.yaw_jitter, edge.yaw_jitter);
                        let scale = match edge.scale_range {
                            Some([a, b]) => rng.range(a, b),
                            None => edge.scale.unwrap_or(1.0),
                        };
                        out.push(Decoration {
                            mesh: edge.mesh.clone(),
                            tile: [mc, mr],
                            offset: [
                                px * (reach - inward) + pz.abs() * along,
                                f32::from(edge.y),
                                pz * (reach - inward) + px.abs() * along,
                            ],
                            rotation: [0.0, (yaw + edge.yaw + turn).rem_euclid(360.0), 0.0],
                            scale,
                            solid: edge.solid,
                        });
                    }
                }
            }
        }
        if out.len() > MAX_DECORATIONS {
            bail!(
                "the map expands to {} decorations; the limit is {MAX_DECORATIONS}",
                out.len()
            );
        }
        let mut per_tile: std::collections::BTreeMap<[u16; 2], usize> = Default::default();
        for d in &out {
            let n = per_tile.entry(d.tile).or_default();
            *n += 1;
            if *n > MAX_DECORATIONS_PER_TILE {
                bail!(
                    "tile {:?} carries more than {MAX_DECORATIONS_PER_TILE} decorations",
                    d.tile
                );
            }
        }
        Ok(out)
    }

    /// Every decal the map draws: single `[[decals]]` with a `tile`, then
    /// each brush's tiles. A pure function of the file, like
    /// [`Self::decorations`].
    pub fn decals(&self) -> anyhow::Result<Vec<Decal>> {
        let grid = self.tile_grid()?;
        let clear = self.clear_tiles();
        let mut out = Vec::new();
        for (i, d) in self.decals.iter().enumerate() {
            let sides = d.sides.clamp(3, 16);
            let base =
                |tile: [u16; 2], offset: [f32; 3], yaw: f32, k: f32, cell: AtlasCell| Decal {
                    tile,
                    offset,
                    yaw,
                    size: [f32::from(d.size[0]) * k, f32::from(d.size[1]) * k],
                    sides,
                    cell,
                    span: d.span.clamp(1, 8),
                    color: d.color,
                    semi: d.semi,
                };
            if let Some(tile) = d.tile {
                out.push(base(tile, d.offset.map(f32::from), d.yaw[0], 1.0, d.cell));
            }
            let Some(rect) = d.rect else {
                continue;
            };
            let (c0, c1) = (rect[0].min(rect[2]), rect[0].max(rect[2]));
            let (r0, r1) = (rect[1].min(rect[3]), rect[1].max(rect[3]));
            let cells: Vec<AtlasCell> = std::iter::once(d.cell)
                .chain(d.cells.iter().copied())
                .collect();
            for row in r0..=r1 {
                for col in c0..=c1 {
                    let (c, r) = (usize::from(col), usize::from(row));
                    let open = grid.get(c, r).is_some_and(|t| !t.void && t.walls != 0xF);
                    let on =
                        d.on.is_empty() || grid.char_at(c, r).is_some_and(|ch| d.on.contains(ch));
                    if !open || !on || clear.contains(&[col, row]) {
                        continue;
                    }
                    let mut rng = TileRng::new(d.seed, 0x4000 + i, col, row);
                    let density = d.density.max(0.0);
                    let n = density.trunc() as u32 + u32::from(rng.unit() < density.fract());
                    let j = f32::from(d.jitter);
                    for _ in 0..n {
                        let cell = cells
                            [((rng.unit() * cells.len() as f32) as usize).min(cells.len() - 1)];
                        let offset = [
                            f32::from(d.offset[0]) + rng.range(-j, j),
                            f32::from(d.offset[1]),
                            f32::from(d.offset[2]) + rng.range(-j, j),
                        ];
                        let yaw = rng.range(d.yaw[0], d.yaw[1]);
                        let k = 1.0 + rng.range(-d.size_jitter, d.size_jitter);
                        out.push(base([col, row], offset, yaw, k.max(0.1), cell));
                    }
                }
            }
        }
        Ok(out)
    }

    /// The tiles a [`ScatterBrush::keep_clear`] brush leaves empty: spawns,
    /// exits, NPC tiles and patrol stops, and placed props.
    pub fn clear_tiles(&self) -> std::collections::BTreeSet<[u16; 2]> {
        let mut out = std::collections::BTreeSet::new();
        out.extend(self.spawns.iter().map(|s| s.tile));
        out.extend(self.props.iter().map(|p| p.tile));
        for n in &self.npcs {
            out.insert(n.tile);
            out.extend(n.patrol.iter().copied());
        }
        for e in &self.exits {
            let (c0, c1) = (e.rect[0].min(e.rect[2]), e.rect[0].max(e.rect[2]));
            let (r0, r1) = (e.rect[1].min(e.rect[3]), e.rect[1].max(e.rect[3]));
            for r in r0..=r1 {
                for c in c0..=c1 {
                    out.insert([c, r]);
                }
            }
        }
        out
    }

    /// Every mesh reference the map draws: props, then decorations. What a
    /// loader resolves before it builds the map.
    pub fn mesh_refs(&self) -> Vec<&PropMesh> {
        let mut out: Vec<&PropMesh> = self.props.iter().map(|p| &p.mesh).collect();
        out.extend(self.decor.iter().map(|d| &d.mesh));
        for s in &self.scatter {
            out.extend(s.meshes.iter().map(|m| &m.mesh));
        }
        out.extend(self.edges.iter().map(|e| &e.mesh));
        out
    }
}
