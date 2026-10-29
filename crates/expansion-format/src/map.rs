//! `maps/<id>.toml`: a custom map.
//!
//! A map is a walkable field scene the modder lays out tile by tile. It
//! borrows its look from a retail **base scene** - the scene's textures,
//! palettes and geometry pack stay on the player's disc and are named only by
//! [`DiscRef::Scene`] - and supplies everything that makes it a *place*:
//! which tiles are floor and which are wall, how high each floor stands,
//! where props stand, where the player arrives, and what each exit, trigger
//! and encounter zone does.
//!
//! The tile layer is a painter's grid. `rows` is a list of strings, one
//! character per tile, and `legend` names what each character means; a 2D
//! tile painter reads and writes exactly this pair, so a map round-trips
//! between a text editor and a painter without loss. Every coordinate a map
//! carries (spawns, exits, zones, props) is a **map-local tile**
//! `[column, row]`, counted from the top-left character of `rows`: column
//! grows to the right (world `+X`), row grows downward (world `+Z`).
//!
//! What a tile means to the engine is the retail collision byte of
//! `docs/formats/field-map.md`: four sub-cell wall bits (the tile's four
//! `64 x 64` quarters) and a floor tier indexing the map's
//! [`MapDef::floor_heights`] ladder. [`MapDef::tile_grid`] resolves the
//! characters into those fields; the engine bakes them into the scene's field
//! map.
//!
//! ```toml
//! name = "Tower of Rebirth, 1F"
//! base = { scene = { kind = "scene", name = "dolk" } }
//! floor_heights = [0, 64]
//!
//! [tiles]
//! rows = [
//!   "#####",
//!   "#...#",
//!   "#.^.#",
//!   "##=##",
//! ]
//! [tiles.legend]
//! "#" = { walls = "all" }
//! "." = { height = 0 }
//! "^" = { height = 1 }
//! "=" = { height = 0 }
//!
//! [[spawns]]
//! id = "start"
//! tile = [2, 1]
//!
//! [[exits]]
//! rect = [2, 3, 2, 3]
//! to = { kind = "retail", scene = { kind = "scene", name = "town01" }, tile = [37, 30] }
//! ```

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::refs::DiscRef;

/// The largest map, in tiles per axis. The retail field grid is `128 x 128`
/// tiles; the engine places a map at [`TileLayer::origin`] and keeps one
/// row of margin below it (the wall probe reads one row past a tile, see
/// `docs/subsystems/field-locomotion.md` "Wall probes").
pub const MAX_MAP_TILES: usize = 120;

/// The number of floor tiers a collision byte can name.
pub const FLOOR_TIERS: usize = 16;

/// One custom map.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapDef {
    /// Display name, shown as the place-name banner.
    pub name: String,
    /// Size in tiles, `[width, height]`. Optional: when given it must match
    /// `tiles.rows`, which is what a painter checks a hand edit against.
    pub size: Option<[u16; 2]>,
    /// The retail scene the map takes its look from.
    pub base: MapBase,
    /// The floor-tier ladder: world height of each tier, in world units with
    /// **up positive** (a tile one storey up is `128`). Tier `n` of a tile is
    /// `floor_heights[n]`; unlisted tiers are `0`. At most [`FLOOR_TIERS`]
    /// entries. Empty takes the base scene's own ladder.
    pub floor_heights: Vec<i16>,
    /// The tile layer.
    pub tiles: TileLayer,
    /// Placed props.
    pub props: Vec<PropDef>,
    /// Where the player can arrive. The first is the default.
    pub spawns: Vec<SpawnPoint>,
    /// Tile rectangles that leave the map.
    pub exits: Vec<ExitDef>,
    /// Tile rectangles that raise a pack event when the player steps in.
    pub triggers: Vec<TriggerZone>,
    /// Tile rectangles that roll random encounters from a pack table.
    pub encounters: Vec<EncounterZone>,
    /// The music cue the map plays ([`DiscRef::Bgm`]). `None` keeps silence.
    pub bgm: Option<DiscRef>,
    /// Follow-camera framing.
    pub camera: CameraPreset,
}

/// The retail scene a map borrows from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapBase {
    /// The base scene ([`DiscRef::Scene`]). Its VRAM - every texture page
    /// and palette - is loaded as-is, and its geometry pack is what
    /// [`PropMesh::Object`] and a [`Ground::Mesh`] tile index.
    pub scene: DiscRef,
    /// Whether the base scene's own environment geometry (its placed
    /// objects, decoration cells and ground) is drawn under the map.
    pub geometry: BaseGeometry,
}

impl Default for MapBase {
    fn default() -> Self {
        Self {
            scene: DiscRef::Scene {
                name: String::new(),
            },
            geometry: BaseGeometry::None,
        }
    }
}

impl MapBase {
    /// The base scene's CDNAME name, or `None` when [`Self::scene`] is not a
    /// [`DiscRef::Scene`].
    pub fn scene_name(&self) -> Option<&str> {
        match &self.scene {
            DiscRef::Scene { name } if !name.is_empty() => Some(name),
            _ => None,
        }
    }
}

/// What of the base scene's environment draws under a map.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaseGeometry {
    /// Nothing: the map draws only its own ground tiles and props.
    #[default]
    None,
    /// The base scene's whole environment layer, with the map's walls,
    /// heights and zones laid over it - a retail room re-purposed.
    Keep,
}

/// The painter's grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TileLayer {
    /// Where map tile `[0, 0]` sits on the engine's `128 x 128` field grid,
    /// `[column, row]`. With [`BaseGeometry::Keep`] this aligns the map with
    /// the base scene's own layout.
    pub origin: [u8; 2],
    /// One string per row, one character per tile. Rows shorter than the
    /// widest are padded with void.
    pub rows: Vec<String>,
    /// What each character means. A space is always void and needs no entry.
    pub legend: BTreeMap<String, TileStyle>,
}

impl Default for TileLayer {
    fn default() -> Self {
        Self {
            origin: [16, 16],
            rows: Vec::new(),
            legend: BTreeMap::new(),
        }
    }
}

/// One legend entry: what a tile character stands for.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TileStyle {
    /// Painter label (`"stone floor"`). Carried, not read.
    pub label: String,
    /// Which quarters of the tile are wall. Left out when open, which is
    /// also how it reads back (the default variant has no written form).
    #[serde(skip_serializing_if = "Walls::is_open")]
    pub walls: Walls,
    /// Floor tier, an index into [`MapDef::floor_heights`] (`0..16`).
    pub height: u8,
    /// What draws on the tile's floor. `None` draws nothing of the map's own
    /// (the base scene's geometry, when kept, still draws).
    pub ground: Option<Ground>,
}

/// The wall quarters of a tile. A tile is four `64 x 64` sub-cells; `tl` is
/// the low-X / low-Z quarter (top-left in the painter's view), `br` the
/// high-X / high-Z one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Walls {
    /// `"none"` (open floor) or `"all"` (a solid tile).
    Named(WallsNamed),
    /// An explicit set of quarters, e.g. `["tl", "tr"]` for a wall along the
    /// tile's top edge.
    Quarters(Vec<Quarter>),
}

impl Default for Walls {
    fn default() -> Self {
        Self::Named(WallsNamed::None)
    }
}

/// The two named wall sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WallsNamed {
    None,
    All,
}

/// One quarter of a tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quarter {
    /// Low X, low Z.
    Tl,
    /// High X, low Z.
    Tr,
    /// Low X, high Z.
    Bl,
    /// High X, high Z.
    Br,
}

impl Quarter {
    /// The quarter's bit in a tile's wall mask: `tl = 1`, `tr = 2`, `bl = 4`,
    /// `br = 8` - the sub-cell order of the retail collision byte's high
    /// nibble (`q = (z_cell & 1) << 1 | (x_cell & 1)`).
    pub fn bit(self) -> u8 {
        match self {
            Self::Tl => 1,
            Self::Tr => 2,
            Self::Bl => 4,
            Self::Br => 8,
        }
    }
}

impl Walls {
    /// The unwritten default: no walls, no `walls` key.
    pub fn is_open(&self) -> bool {
        matches!(self, Self::Open)
    }

    /// The 4-bit wall mask ([`Quarter::bit`] order).
    pub fn mask(&self) -> u8 {
        match self {
            Self::Named(WallsNamed::All) => 0xF,
            Self::Named(WallsNamed::None) => 0,
            Self::Quarters(q) => q.iter().fold(0, |m, q| m | q.bit()),
        }
    }
}

/// What draws on a tile's floor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Ground {
    /// A flat textured ground quad: the `tile`th `32 x 32` texel cell of the
    /// `8 x 8` atlas on texture page `tpage`, through palette `clut` - the
    /// same three words a retail ground cell carries. All three index the
    /// base scene's VRAM.
    Texture { tpage: u8, clut: u16, tile: u8 },
    /// The ground texture of the base scene's own cell `[column, row]` (its
    /// field grid coordinates, not map-local).
    BaseCell { cell: [u8; 2] },
    /// The `index`th mesh of the base scene's geometry pack, stamped on the
    /// tile as a decoration cell.
    Mesh { index: u16 },
}

/// A placed prop.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PropDef {
    /// Optional id, for scripts and the workbench.
    pub id: String,
    /// The prop's mesh.
    pub mesh: PropMesh,
    /// Map-local tile the prop stands on.
    pub tile: [u16; 2],
    /// Offset from the tile centre, world units `[x, y, z]` (`y` up).
    pub offset: [i16; 3],
    /// Rotation `[x, y, z]` in degrees; `y` is the yaw.
    pub rotation: [f32; 3],
    /// Uniform scale. Carried for the workbench; the engine draws props at
    /// scale 1 (`docs/formats/expansion-pack-maps.md`, "Gaps").
    pub scale: Option<f32>,
    /// Whether the prop's tile is wall. A prop has no collision box of its
    /// own; a solid one closes its tile.
    pub solid: bool,
}

/// Where a prop's mesh comes from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropMesh {
    /// `{ object = N }`: the `N`th mesh of the base scene's geometry pack.
    Object { object: u16 },
    /// `{ tmd = { kind = "tmd", prot = P, index = N } }`: a TMD by disc
    /// coordinate. The engine resolves it only inside the base scene's
    /// geometry pack.
    Tmd { tmd: DiscRef },
}

impl Default for PropMesh {
    fn default() -> Self {
        Self::Object { object: 0 }
    }
}

/// A named arrival point.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpawnPoint {
    pub id: String,
    /// Map-local tile; the player stands at its centre.
    pub tile: [u16; 2],
    /// Facing, a compass sector `0..8` (the retail arrival-facing table:
    /// `n * 45` degrees).
    pub facing: u8,
}

/// An inclusive map-local tile rectangle `[col0, row0, col1, row1]`.
pub type TileRect = [u16; 4];

/// Whether map-local tile `(col, row)` lies in `rect`.
pub fn rect_contains(rect: &TileRect, col: u16, row: u16) -> bool {
    let (c0, c1) = (rect[0].min(rect[2]), rect[0].max(rect[2]));
    let (r0, r1) = (rect[1].min(rect[3]), rect[1].max(rect[3]));
    (c0..=c1).contains(&col) && (r0..=r1).contains(&row)
}

/// A way out of the map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExitDef {
    #[serde(default)]
    pub id: String,
    /// The tiles that take the exit when the player steps onto them.
    pub rect: TileRect,
    pub to: ExitTarget,
}

/// Where an exit leads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExitTarget {
    /// A retail scene ([`DiscRef::Scene`]), arriving on its field-grid tile
    /// `tile` facing `facing` - the same three operands a retail door
    /// carries. With no `tile` the engine seats the player where the
    /// scene's own doors land them.
    Retail {
        scene: DiscRef,
        #[serde(default)]
        tile: Option<[u8; 2]>,
        #[serde(default)]
        facing: u8,
    },
    /// Another map of the same pack, at one of its spawns (the first when
    /// `spawn` is empty).
    Map {
        map: String,
        #[serde(default)]
        spawn: String,
    },
}

/// A tile rectangle that raises a pack event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerZone {
    #[serde(default)]
    pub id: String,
    pub rect: TileRect,
    pub event: MapEvent,
    /// Fire only once, ever: the first entry raises the pack flag
    /// [`once_flag`] (saved with the pack's progress), and a set flag keeps
    /// the zone quiet on every later visit.
    #[serde(default)]
    pub once: bool,
}

/// The pack flag a `once` trigger raises when it fires: `map.<map>.<id>`,
/// or `map.<map>.<index>` for a trigger with no id.
pub fn once_flag(map_id: &str, trigger: &TriggerZone, index: usize) -> String {
    if trigger.id.is_empty() {
        format!("map.{map_id}.{index}")
    } else {
        format!("map.{map_id}.{}", trigger.id)
    }
}

/// What a trigger zone raises. Each names content of another pack section
/// by id; the battle, dialog and quest runtimes resolve it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MapEvent {
    /// Start a battle against a pack formation (`battle.toml` `formations`).
    Battle { formation: String },
    /// Start a pack boss fight (`battle.toml` `bosses`).
    Boss { boss: String },
    /// Show a pack dialogue line (`dialog.toml`).
    Dialog { line: String },
    /// Count as talking to the pack NPC `npc` (a quest's talk objective).
    Talk { npc: String },
    /// Start a pack quest (`quests.toml`).
    StartQuest { quest: String },
    /// Raise a pack flag.
    SetFlag { flag: String },
    /// Add `add` (default 1) to a pack counter.
    Counter {
        counter: String,
        #[serde(default = "one_i64")]
        add: i64,
    },
}

fn one_i64() -> i64 {
    1
}

/// A tile rectangle that rolls random encounters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncounterZone {
    #[serde(default)]
    pub id: String,
    pub rect: TileRect,
    /// The pack encounter table (`battle.toml`).
    pub table: String,
}

/// Follow-camera framing: the port's camera-distance presets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraPreset {
    /// The player's own setting.
    #[default]
    Default,
    /// The retail framing.
    Retail,
    /// Pulled back.
    Far,
    /// Pulled back further, for wide arenas.
    Farther,
}

/// One resolved tile of a map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tile {
    /// Wall quarters ([`Quarter::bit`] order).
    pub walls: u8,
    /// Floor tier (`0..16`).
    pub tier: u8,
    /// `true` for a void tile (a space, or past a short row): no floor,
    /// every quarter wall.
    pub void: bool,
}

/// A map's tile layer resolved to one [`Tile`] per cell, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct TileGrid {
    pub width: usize,
    pub height: usize,
    /// `width * height` tiles, row-major.
    pub tiles: Vec<Tile>,
    /// The legend character of each tile (space for void), parallel to
    /// [`Self::tiles`]; the key into [`TileLayer::legend`].
    pub chars: Vec<char>,
}

impl TileGrid {
    /// The tile at map-local `(col, row)`, `None` off the map.
    pub fn get(&self, col: usize, row: usize) -> Option<&Tile> {
        (col < self.width && row < self.height).then(|| &self.tiles[row * self.width + col])
    }

    /// The legend character at map-local `(col, row)`.
    pub fn char_at(&self, col: usize, row: usize) -> Option<char> {
        (col < self.width && row < self.height).then(|| self.chars[row * self.width + col])
    }
}

const VOID: Tile = Tile {
    walls: 0xF,
    tier: 0,
    void: true,
};

impl MapDef {
    /// Resolve the tile layer: one [`Tile`] per character of `tiles.rows`.
    ///
    /// Fails on a character the legend does not name, a legend key that is
    /// not exactly one character, a tier past [`FLOOR_TIERS`], a map wider
    /// or taller than [`MAX_MAP_TILES`], or a `size` that disagrees with the
    /// rows.
    pub fn tile_grid(&self) -> Result<TileGrid> {
        let mut legend: BTreeMap<char, &TileStyle> = BTreeMap::new();
        for (key, style) in &self.tiles.legend {
            let mut chars = key.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                bail!("legend key {key:?} is not one character");
            };
            if usize::from(style.height) >= FLOOR_TIERS {
                bail!(
                    "legend {key:?}: height {} is past the last tier ({})",
                    style.height,
                    FLOOR_TIERS - 1
                );
            }
            legend.insert(c, style);
        }
        let height = self.tiles.rows.len();
        let width = self
            .tiles
            .rows
            .iter()
            .map(|r| r.chars().count())
            .max()
            .unwrap_or(0);
        if width > MAX_MAP_TILES || height > MAX_MAP_TILES {
            bail!("map is {width} x {height} tiles; the limit is {MAX_MAP_TILES} per axis");
        }
        if let Some([w, h]) = self.size
            && (usize::from(w) != width || usize::from(h) != height)
        {
            bail!("size is {w} x {h} but the rows are {width} x {height}");
        }
        let mut tiles = Vec::with_capacity(width * height);
        let mut chars = Vec::with_capacity(width * height);
        for (row, text) in self.tiles.rows.iter().enumerate() {
            let mut n = 0;
            for (col, c) in text.chars().enumerate() {
                n += 1;
                let tile = if c == ' ' && !legend.contains_key(&' ') {
                    VOID
                } else {
                    let Some(style) = legend.get(&c) else {
                        bail!("tile ({col}, {row}) is {c:?}, which the legend does not name");
                    };
                    Tile {
                        walls: style.walls.mask(),
                        tier: style.height,
                        void: false,
                    }
                };
                tiles.push(tile);
                chars.push(c);
            }
            for _ in n..width {
                tiles.push(VOID);
                chars.push(' ');
            }
        }
        Ok(TileGrid {
            width,
            height,
            tiles,
            chars,
        })
    }

    /// The legend style of a tile character.
    pub fn style(&self, c: char) -> Option<&TileStyle> {
        let mut buf = [0u8; 4];
        self.tiles.legend.get(&*c.encode_utf8(&mut buf))
    }

    /// The map-local tile under world `(x, z)`, `None` off the map's
    /// rectangle. A tile is `128` world units; map tile `[0, 0]` is field
    /// grid tile [`TileLayer::origin`].
    pub fn tile_at_world(&self, grid: &TileGrid, x: i32, z: i32) -> Option<(u16, u16)> {
        if x < 0 || z < 0 {
            return None;
        }
        let col = (x >> 7) - i32::from(self.tiles.origin[0]);
        let row = (z >> 7) - i32::from(self.tiles.origin[1]);
        ((0..grid.width as i32).contains(&col) && (0..grid.height as i32).contains(&row))
            .then_some((col as u16, row as u16))
    }

    /// The spawn named `id`, or the first spawn when `id` is empty.
    pub fn spawn(&self, id: &str) -> Option<&SpawnPoint> {
        if id.is_empty() {
            self.spawns.first()
        } else {
            self.spawns.iter().find(|s| s.id == id)
        }
    }

    /// The floor-tier ladder as 16 heights (up positive), or `None` when the
    /// map takes the base scene's.
    pub fn floor_ladder(&self) -> Option<[i16; FLOOR_TIERS]> {
        if self.floor_heights.is_empty() {
            return None;
        }
        let mut out = [0i16; FLOOR_TIERS];
        for (slot, h) in out.iter_mut().zip(&self.floor_heights) {
            *slot = *h;
        }
        Some(out)
    }

    /// Every map-local tile coordinate the map names (spawns, exits, zones,
    /// props) that falls off the tile grid, as `(what, col, row)`. A
    /// validator reports these.
    pub fn off_grid_refs(&self, grid: &TileGrid) -> Vec<(String, u16, u16)> {
        let on = |c: u16, r: u16| usize::from(c) < grid.width && usize::from(r) < grid.height;
        let mut out = Vec::new();
        for s in &self.spawns {
            if !on(s.tile[0], s.tile[1]) {
                out.push((format!("spawn {:?}", s.id), s.tile[0], s.tile[1]));
            }
        }
        for p in &self.props {
            if !on(p.tile[0], p.tile[1]) {
                out.push((format!("prop {:?}", p.id), p.tile[0], p.tile[1]));
            }
        }
        let rects = self
            .exits
            .iter()
            .map(|e| ("exit", &e.id, &e.rect))
            .chain(self.triggers.iter().map(|t| ("trigger", &t.id, &t.rect)))
            .chain(
                self.encounters
                    .iter()
                    .map(|z| ("encounter", &z.id, &z.rect)),
            );
        for (what, id, rect) in rects {
            for (c, r) in [(rect[0], rect[1]), (rect[2], rect[3])] {
                if !on(c, r) {
                    out.push((format!("{what} {id:?}"), c, r));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> MapDef {
        toml::from_str(text).expect("map parses")
    }

    const ARENA: &str = r######"
name = "Test arena"
size = [5, 4]
floor_heights = [0, 64]
base = { scene = { kind = "scene", name = "dolk" } }

[tiles]
rows = [
  "#####",
  "#.^a#",
  "#...",
  "##=##",
]
[tiles.legend]
"#" = { walls = "all" }
"." = {}
"^" = { height = 1, ground = { kind = "texture", tpage = 12, clut = 31808, tile = 3 } }
"a" = { walls = ["tl", "br"] }
"=" = { label = "door" }

[[spawns]]
id = "start"
tile = [1, 1]
facing = 4

[[exits]]
rect = [2, 3, 2, 3]
to = { kind = "retail", scene = { kind = "scene", name = "town01" }, tile = [37, 30] }

[[exits]]
rect = [3, 2, 3, 2]
to = { kind = "map", map = "rebirth_tower_2f" }

[[triggers]]
rect = [1, 2, 2, 2]
event = { kind = "battle", formation = "tower_guard" }

[[triggers]]
rect = [3, 1, 3, 1]
event = { kind = "counter", counter = "steps" }
once = true

[[encounters]]
rect = [1, 1, 3, 2]
table = "tower_1f"

[[props]]
mesh = { object = 7 }
tile = [3, 1]
rotation = [0.0, 90.0, 0.0]
solid = true

[[props]]
mesh = { tmd = { kind = "tmd", prot = 61, index = 2 } }
tile = [1, 2]
"######;

    #[test]
    fn the_arena_parses_with_every_section() {
        let m = parse(ARENA);
        assert_eq!(m.name, "Test arena");
        assert_eq!(m.base.scene_name(), Some("dolk"));
        assert_eq!(m.base.geometry, BaseGeometry::None);
        assert_eq!(m.spawn("").map(|s| s.id.as_str()), Some("start"));
        assert_eq!(m.spawn("start").map(|s| s.facing), Some(4));
        assert!(m.spawn("nope").is_none());
        assert_eq!(m.exits.len(), 2);
        assert!(
            matches!(m.exits[1].to, ExitTarget::Map { ref map, .. } if map == "rebirth_tower_2f")
        );
        assert_eq!(
            m.triggers[0].event,
            MapEvent::Battle {
                formation: "tower_guard".into()
            }
        );
        assert_eq!(
            m.triggers[1].event,
            MapEvent::Counter {
                counter: "steps".into(),
                add: 1
            }
        );
        assert_eq!(once_flag("m", &m.triggers[0], 0), "map.m.0");
        assert_eq!(m.encounters[0].table, "tower_1f");
        assert_eq!(m.props[0].mesh, PropMesh::Object { object: 7 });
        assert_eq!(
            m.props[1].mesh,
            PropMesh::Tmd {
                tmd: DiscRef::Tmd { prot: 61, index: 2 }
            }
        );
        assert_eq!(m.floor_ladder().unwrap()[..3], [0, 64, 0]);
    }

    #[test]
    fn tiles_resolve_to_wall_masks_and_tiers() {
        let m = parse(ARENA);
        let g = m.tile_grid().unwrap();
        assert_eq!((g.width, g.height), (5, 4));
        let t = |c, r| *g.get(c, r).unwrap();
        assert_eq!(t(0, 0).walls, 0xF);
        assert!(!t(0, 0).void);
        assert_eq!((t(1, 1).walls, t(1, 1).tier), (0, 0));
        assert_eq!(t(2, 1).tier, 1);
        // `tl` is bit 0, `br` bit 3.
        assert_eq!(t(3, 1).walls, 0b1001);
        // Row 2 is one short: the missing tile is void.
        assert!(t(4, 2).void);
        assert_eq!(t(4, 2).walls, 0xF);
        assert_eq!(g.char_at(2, 3), Some('='));
        assert!(m.off_grid_refs(&g).is_empty());
    }

    #[test]
    fn a_space_is_void_unless_the_legend_names_it() {
        let mut m = parse(ARENA);
        m.tiles.rows[1] = "# . #".into();
        let g = m.tile_grid().unwrap();
        assert!(g.get(1, 1).unwrap().void);
        m.tiles.legend.insert(" ".into(), TileStyle::default());
        let g = m.tile_grid().unwrap();
        assert!(!g.get(1, 1).unwrap().void);
    }

    #[test]
    fn bad_layers_are_refused() {
        let mut m = parse(ARENA);
        m.tiles.rows[1] = "#.?.#".into();
        assert!(m.tile_grid().is_err(), "unnamed character");
        let mut m = parse(ARENA);
        m.size = Some([6, 4]);
        assert!(m.tile_grid().is_err(), "size mismatch");
        let mut m = parse(ARENA);
        m.tiles.legend.get_mut(".").unwrap().height = 16;
        assert!(m.tile_grid().is_err(), "tier 16");
        let mut m = parse(ARENA);
        m.tiles.legend.insert("ab".into(), TileStyle::default());
        assert!(m.tile_grid().is_err(), "two-character key");
        let mut m = parse(ARENA);
        m.tiles.rows = vec!["#".repeat(MAX_MAP_TILES + 1)];
        m.size = None;
        assert!(m.tile_grid().is_err(), "too wide");
    }

    #[test]
    fn off_grid_coordinates_are_reported() {
        let mut m = parse(ARENA);
        m.spawns[0].tile = [9, 1];
        m.exits[0].rect = [2, 3, 2, 7];
        let g = m.tile_grid().unwrap();
        let refs = m.off_grid_refs(&g);
        assert_eq!(refs.len(), 2, "{refs:?}");
    }

    #[test]
    fn rects_are_inclusive_and_order_free() {
        assert!(rect_contains(&[1, 1, 3, 2], 3, 2));
        assert!(rect_contains(&[3, 2, 1, 1], 1, 1));
        assert!(!rect_contains(&[1, 1, 3, 2], 4, 2));
    }

    #[test]
    fn a_partial_map_parses() {
        let m = parse("name = \"stub\"");
        assert_eq!(m.tiles.origin, [16, 16]);
        assert_eq!(m.tile_grid().unwrap().width, 0);
        assert_eq!(m.camera, CameraPreset::Default);
    }

    #[test]
    fn an_open_tile_style_writes_and_reads_back() {
        let m = parse("[tiles]\nrows = [\".\"]\n[tiles.legend]\n\".\" = { height = 1 }\n");
        let text = toml::to_string(&m).unwrap();
        assert!(!text.contains("walls"), "{text}");
        assert_eq!(parse(&text), m);
    }
}
