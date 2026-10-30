//! An expansion-pack map as a scene's field layer.
//!
//! A pack map ([`legaia_expansion_format::map::MapDef`]) is entered **over a
//! retail base scene**: the base scene's block loads as usual - its VRAM, its
//! geometry pack, its sound - and a [`SceneOverride`] on the [`Scene`] stands
//! in for the two things that make the block a place: the `.MAP` field image
//! and the MAN. Every `.MAP` accessor on [`Scene`] reads through
//! [`Scene::field_map_bytes`], so the collision grid, the floor sampler, the
//! placed props, the decoration cells and the ground heightfield all come out
//! of the synthetic image through the same parsers a retail map goes through.
//! The MAN is suppressed outright (no entry script, NPCs, object binds or
//! encounter table of the base scene's).
//!
//! [`build_field_map_image`] writes that image from the map's tiles. It is a
//! `.MAP` in the byte layout of `docs/formats/field-map.md`:
//!
//! - `+0x4000` collision grid: tile `(c, r)`'s floor tier in byte `(c, r)`'s
//!   low nibble, its wall quarters in byte `(c, r + 1)`'s high nibble. The
//!   row shift is the wall probe's `+2` Z bias
//!   (`docs/subsystems/field-locomotion.md`, "Wall probes"): `FUN_801CFE4C`
//!   reads a point's wall bits one grid row below the row its floor nibble
//!   sits in, so a wall authored on tile row `r` lives in byte row `r + 1`.
//! - `+0x0000` / `+0x8000` object records and cells: one record per ground
//!   texture, decoration mesh and prop, each cell naming its record in its low
//!   nine bits and carrying the draw gates `0x1000` / `0x2000` the grid-mark
//!   refresh derives from the record's flags. A retail map ships them
//!   stamped, and the draw side reads the scene's image unrefreshed.
//! - `+0x10000` trigger block: the empty four-table chain. A pack map's exits
//!   and zones are the map runtime's, not `.MAP` triggers.
//!
//! The image is the modder's layout run through the documented format; it
//! carries no disc bytes of its own. With [`BaseGeometry::Keep`] it starts
//! from the base scene's map read off the player's disc at load time.

use anyhow::{Context, Result, bail};
use legaia_expansion_format::map::{
    BaseGeometry, FLOOR_TIERS, Ground, MapDef, PropMesh, TileGrid, WallMesh, npc_slot,
};
use legaia_expansion_format::refs::DiscRef;
use legaia_scene_formats::field_objects::{
    COLLISION_GRID_OFFSET, GRID_DIM, OBJECT_GRID_OFFSET, OBJECT_INDEX_MASK, OBJECT_RECORD_STRIDE,
    ObjectRecord, TILE_CENTER,
};

use legaia_scene_formats::man_section::ActorPlacement;

use crate::scene::{FIELD_MAP_LEN, ProtIndex, Scene};

pub mod detail;
pub mod gltf;
pub mod tmd_ops;
pub mod vram_alloc;

use detail::{LibMesh, MeshLibrary, VramPatch};

/// The length of a synthetic field image: the `0x12000`-byte map plus the
/// `0x800`-byte fallback window the scene loader stages behind it (here all
/// zero - no `.PCH` triggers).
pub const FIELD_IMAGE_LEN: usize = FIELD_MAP_LEN + 0x800;

/// Byte offset of the trigger block.
const TRIGGER_BLOCK_OFFSET: usize = 0x10000;

/// The number of object records a map has (`0x4000 / 0x20`).
const RECORD_SLOTS: usize = 0x4000 / OBJECT_RECORD_STRIDE;

/// Record flags of a ground-texture cell: bit `0x1`, which the grid-mark
/// refresh mirrors into the cell's walk-visible bit `0x1000` - the ground
/// heightfield's gate. The bit survives the refresh's countdown pass only
/// while `+0x16` is nonzero, and on a ground record `+0x16` is the palette
/// word, which is never zero for a real palette.
const FLAGS_GROUND: u16 = 0x0001;

/// Record flags of a decoration-mesh cell: the retail decoration family
/// `0x12` (mesh drawn, not placed).
const FLAGS_DECORATION: u16 = 0x0012;

/// Record flags of a placed prop: the retail placed-landmark family `0x16`
/// (placed, mesh drawn).
const FLAGS_PROP: u16 = 0x0016;

/// The object-cell draw gates the grid-mark refresh `FUN_80017BEC` derives
/// from a record's flags: bit `0x1` -> `0x1000`, bit `0x2` -> `0x2000`.
fn cell_gates(flags: u16) -> u16 {
    ((flags & 1) << 12) | ((flags & 2) << 12)
}

/// What a pack map puts in place of its base scene's field layer.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneOverride {
    /// The pack map's id (`maps/<id>.toml`).
    pub map_id: String,
    /// The synthetic field image ([`FIELD_IMAGE_LEN`] bytes).
    pub field_map: Vec<u8>,
    /// The floor-height ladder in the MAN header's convention (positive =
    /// up); the field entry negates it into the runtime ladder as it does a
    /// retail MAN's.
    pub floor_lut: [i16; FLOOR_TIERS],
    /// Each scaled prop's uniform scale, keyed by the field-grid tile its
    /// placement anchors on (the prop's own tile).
    pub prop_scales: Vec<((u8, u8), f32)>,
    /// The map's NPCs as actor placements ([`resolve_map_npcs`]): the list a
    /// retail scene reads out of its MAN, which the hosts' NPC draw and the
    /// world's talk / collision install read in its place.
    pub npcs: Vec<ActorPlacement>,
    /// The meshes the map adds to its base scene's geometry pack, as TMD
    /// bytes: generated wall boxes ([`wall_box_tmd`]), meshes carried in from
    /// other scenes and model assets ([`detail`]), and baked decoration cells
    /// ([`tmd_ops::bake`]). The scene's resource build appends them to the
    /// pack, so a record reaches mesh `k` at pack index `geometry_len + k`
    /// ([`BaseInputs::geometry_len`]).
    pub added_tmds: Vec<Vec<u8>>,
    /// The PROT entry the geometry pack is registered under: the added
    /// meshes join it.
    pub geometry_entry: Option<u32>,
    /// VRAM the added meshes sample, written over the base scene's build
    /// ([`detail::apply_vram_patches`]).
    pub vram_patches: Vec<VramPatch>,
    /// The draw-record key of each animated prop, by the field-grid tile its
    /// placement anchors on ([`prop_records`]): what a host hands
    /// `World::object_draw_turn_matrices` / `object_draw_displacements` to
    /// find the prop's live transform.
    pub prop_records: Vec<((u8, u8), usize)>,
}

impl SceneOverride {
    /// The draw-record key of the animated prop anchored on `anchor`.
    pub fn prop_record(&self, anchor: (u8, u8)) -> Option<usize> {
        self.prop_records
            .iter()
            .find(|(a, _)| *a == anchor)
            .map(|&(_, r)| r)
    }

    /// Give each placed draw of an animated prop its draw-record key
    /// ([`Self::prop_records`]) where it has no bind record of its own.
    /// `anchors` and `records` are a host's parallel per-draw lists.
    pub fn fill_prop_records(
        &self,
        anchors: impl IntoIterator<Item = (u8, u8)>,
        records: &mut [Option<usize>],
    ) {
        if self.prop_records.is_empty() {
            return;
        }
        for (a, r) in anchors.into_iter().zip(records.iter_mut()) {
            if r.is_none() {
                *r = self.prop_record(a);
            }
        }
    }

    /// The render scale of the placed draw anchored on `anchor`: a scaled
    /// prop's, else `None` (the draw keeps whatever scale it had).
    pub fn prop_scale(&self, anchor: (u8, u8)) -> Option<f32> {
        self.prop_scales
            .iter()
            .find(|(a, _)| *a == anchor)
            .map(|&(_, s)| s)
    }
}

/// The `scale` of every prop that sets one, keyed by its field-grid tile -
/// [`SceneOverride::prop_scales`].
pub fn prop_scales(def: &MapDef) -> Vec<((u8, u8), f32)> {
    def.props
        .iter()
        .filter_map(|p| {
            let s = p.scale.filter(|s| s.is_finite() && *s > 0.0 && *s != 1.0)?;
            let (c, r) = grid_tile(def, p.tile[0], p.tile[1]);
            Some(((u8::try_from(c).ok()?, u8::try_from(r).ok()?), s))
        })
        .collect()
}

/// The draw-record key a pack map's animated prop `n` draws under: past every
/// retail bind record (a MAN record index is a byte), so the two never meet.
pub const PROP_RECORD_BASE: usize = 0x1_0000;

/// The draw-record key of every prop that animates, keyed by its field-grid
/// tile - [`SceneOverride::prop_records`].
pub fn prop_records(def: &MapDef) -> Vec<((u8, u8), usize)> {
    def.props
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.animate.is_empty())
        .filter_map(|(i, p)| {
            let (c, r) = grid_tile(def, p.tile[0], p.tile[1]);
            Some((
                (u8::try_from(c).ok()?, u8::try_from(r).ok()?),
                PROP_RECORD_BASE + i,
            ))
        })
        .collect()
}

/// Where a map tile lands on the `128 x 128` field grid.
pub fn grid_tile(def: &MapDef, col: u16, row: u16) -> (usize, usize) {
    (
        usize::from(def.tiles.origin[0]) + usize::from(col),
        usize::from(def.tiles.origin[1]) + usize::from(row),
    )
}

/// The world-space centre of map tile `(col, row)`.
pub fn tile_centre_world(def: &MapDef, col: u16, row: u16) -> (i16, i16) {
    let (gc, gr) = grid_tile(def, col, row);
    let centre = |t: usize| (t as i32 * 128 + TILE_CENTER) as i16;
    (centre(gc), centre(gr))
}

/// The map-local tile under world `(x, z)`, `None` off the map's rectangle
/// ([`MapDef::tile_at_world`]).
pub fn world_to_map_tile(def: &MapDef, grid: &TileGrid, x: i32, z: i32) -> Option<(u16, u16)> {
    def.tile_at_world(grid, x, z)
}

/// Inputs from the disc a map image may need: the base scene's own field map
/// (for [`BaseGeometry::Keep`] and [`Ground::BaseCell`]) and the PROT entry
/// that holds its geometry pack (for a [`PropMesh::Tmd`] prop).
#[derive(Debug, Clone, Copy, Default)]
pub struct BaseInputs<'a> {
    pub base_map: Option<&'a [u8]>,
    pub geometry_entry: Option<u32>,
    /// How many meshes the base scene's geometry pack holds
    /// ([`crate::scene_resources::field_geometry_pack`]): where the
    /// added meshes start in its index space.
    pub geometry_len: Option<usize>,
    /// The map's resolved meshes ([`detail::resolve_mesh_library`]): needed
    /// by a mesh from outside the base pack and by every decoration.
    pub library: Option<&'a MeshLibrary>,
}

/// What [`build_field_map_image`] produced: the image and the meshes its
/// records name past the base pack.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldImage {
    pub image: Vec<u8>,
    /// [`SceneOverride::added_tmds`].
    pub added_tmds: Vec<Vec<u8>>,
    /// Decorations left out: on a placed prop's tile (a cell draws one
    /// record, and the prop's is placed).
    pub dropped_decorations: usize,
    /// The decoration cell size the bake settled on, in tiles a side
    /// (`1` = one baked mesh a tile; see [`write_decorations`]).
    pub decoration_chunk: usize,
    /// Map tiles whose cell drew a mesh (a kept retail cell, a wall, a mesh
    /// ground) that a placed prop replaced: a cell names one record, so the
    /// earlier mesh no longer draws.
    pub covered_cells: Vec<[u16; 2]>,
}

/// Build the synthetic field image of `def` (whose resolved tiles are
/// `grid`). See the module docs for the layout.
pub fn build_field_map_image(
    def: &MapDef,
    grid: &TileGrid,
    base: BaseInputs<'_>,
) -> Result<Vec<u8>> {
    build_field_image(def, grid, base).map(|f| f.image)
}

/// [`build_field_map_image`] with the generated wall-box meshes the image's
/// wall records name.
pub fn build_field_image(
    def: &MapDef,
    grid: &TileGrid,
    base: BaseInputs<'_>,
) -> Result<FieldImage> {
    let [ox, oy] = def.tiles.origin.map(usize::from);
    // The last map row's walls land one byte row lower.
    if ox + grid.width + 1 > GRID_DIM || oy + grid.height + 1 > GRID_DIM {
        bail!(
            "a {} x {} map at origin ({ox}, {oy}) runs off the {GRID_DIM}-tile field grid",
            grid.width,
            grid.height
        );
    }
    let mut img = vec![0u8; FIELD_IMAGE_LEN];
    let keep = def.base.geometry == BaseGeometry::Keep;
    if keep {
        let Some(map) = base.base_map else {
            bail!("geometry = \"keep\" needs the base scene's field map");
        };
        // The object records and cells only: the collision grid and the
        // trigger block are the pack's.
        let n = map.len().min(TRIGGER_BLOCK_OFFSET);
        img[..COLLISION_GRID_OFFSET].copy_from_slice(&map[..COLLISION_GRID_OFFSET.min(n)]);
        if n > OBJECT_GRID_OFFSET {
            img[OBJECT_GRID_OFFSET..n].copy_from_slice(&map[OBJECT_GRID_OFFSET..n]);
        }
    }
    write_collision_grid(&mut img, def, grid);
    let mut added = Added {
        len: base.geometry_len,
        meshes: Vec::new(),
    };
    let (mut free, covered_cells) = write_objects(&mut img, def, grid, base, keep, &mut added)?;
    let (dropped, chunk) = write_decorations(&mut img, def, grid, base, &mut added, &mut free)?;
    write_empty_trigger_block(&mut img);
    Ok(FieldImage {
        image: img,
        added_tmds: added.meshes,
        dropped_decorations: dropped,
        decoration_chunk: chunk,
        covered_cells,
    })
}

/// The `+0x4000` grid. Everything off the map is a wall; a void tile takes
/// the tier of a floor neighbour above or left of it, so the ground's corner
/// heights stay level along the map's edge.
fn write_collision_grid(img: &mut [u8], def: &MapDef, grid: &TileGrid) {
    let [ox, oy] = def.tiles.origin.map(usize::from);
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
    let cg = &mut img[COLLISION_GRID_OFFSET..COLLISION_GRID_OFFSET + GRID_DIM * GRID_DIM];
    cg.fill(0xF0);
    for row in 0..grid.height {
        for col in 0..grid.width {
            let at = |c: usize, r: usize| (oy + r) * GRID_DIM + ox + c;
            cg[at(col, row)] = (cg[at(col, row)] & 0xF0) | tier(col, row);
            let walls = grid.get(col, row).expect("in range").walls;
            let below = at(col, row + 1);
            cg[below] = (walls << 4) | (cg[below] & 0x0F);
        }
    }
    // The ground's corner heights read one tile past the map's right and
    // bottom edges; those bytes take the edge tiles' tiers.
    let (w, h) = (grid.width, grid.height);
    if w > 0 && h > 0 {
        for row in 0..=h {
            let b = (oy + row) * GRID_DIM + ox + w;
            cg[b] = (cg[b] & 0xF0) | tier(w - 1, row.min(h - 1));
        }
        for col in 0..w {
            let b = (oy + h) * GRID_DIM + ox + col;
            cg[b] = (cg[b] & 0xF0) | tier(col, h - 1);
        }
    }
}

/// Write one `0x20`-byte object record.
#[allow(clippy::too_many_arguments)]
fn put_record(
    img: &mut [u8],
    slot: usize,
    offset: [i16; 3],
    rot: [u16; 3],
    mesh: u16,
    flags: u16,
    ground: Option<(u8, u8, u16)>,
) {
    let r = &mut img[slot * OBJECT_RECORD_STRIDE..(slot + 1) * OBJECT_RECORD_STRIDE];
    r.fill(0);
    r[0x00..0x02].copy_from_slice(&offset[0].to_le_bytes());
    // `+0x02` is added to the tile's floor height, which is Y-down.
    r[0x02..0x04].copy_from_slice(&offset[1].wrapping_neg().to_le_bytes());
    // `world_z = row * 128 - (z_off - 0x40)`: Z is subtractive.
    r[0x04..0x06].copy_from_slice(&offset[2].wrapping_neg().to_le_bytes());
    r[0x08..0x0A].copy_from_slice(&rot[0].to_le_bytes());
    r[0x0A..0x0C].copy_from_slice(&rot[1].to_le_bytes());
    r[0x0C..0x0E].copy_from_slice(&rot[2].to_le_bytes());
    r[0x10..0x12].copy_from_slice(&mesh.to_le_bytes());
    r[0x12..0x14].copy_from_slice(&flags.to_le_bytes());
    if let Some((tile, tpage, clut)) = ground {
        r[0x14] = tile;
        r[0x15] = tpage;
        r[0x16..0x18].copy_from_slice(&clut.to_le_bytes());
    }
}

/// A ground texture's record words: atlas cell, texture page, palette.
type GroundTexture = (u8, u8, u16);

/// Degrees to the PSX 12-bit angle (`4096` = a full turn).
fn psx_angle(deg: f32) -> u16 {
    ((deg / 360.0 * 4096.0).round() as i32).rem_euclid(4096) as u16
}

/// The meshes a map image adds past the base pack, deduplicated by bytes:
/// added mesh `k` is pack index `len + k`.
struct Added {
    len: Option<usize>,
    meshes: Vec<Vec<u8>>,
}

impl Added {
    /// The pack index of `raw`, adding it if it is new.
    fn index(&mut self, raw: Vec<u8>, what: &str) -> Result<u16> {
        let Some(len) = self.len else {
            bail!("{what} needs the base scene's geometry pack length");
        };
        let k = match self.meshes.iter().position(|m| *m == raw) {
            Some(k) => k,
            None => {
                self.meshes.push(raw);
                self.meshes.len() - 1
            }
        };
        u16::try_from(len + k).context("added mesh pack index")
    }

    /// The bytes of pack mesh `index`: the base pack's (from the library) or
    /// an added one.
    fn raw<'a>(&'a self, index: u16, lib: &'a MeshLibrary) -> Option<&'a [u8]> {
        let i = usize::from(index);
        match self.len {
            Some(len) if i >= len => self.meshes.get(i - len).map(Vec::as_slice),
            _ => lib.base_raw.get(i).map(Vec::as_slice),
        }
    }
}

/// A prop's (or decoration's) pack mesh index: a base-pack index as named, or
/// a mesh the library carried in, added to the pack.
fn mesh_index(
    mesh: &PropMesh,
    base: &BaseInputs<'_>,
    added: &mut Added,
    what: &str,
) -> Result<u16> {
    match mesh {
        PropMesh::Object { object } => return Ok(*object),
        PropMesh::Tmd {
            tmd: DiscRef::Tmd { prot, index },
        } if Some(u32::from(*prot)) == base.geometry_entry => return Ok(*index),
        _ => {}
    }
    let Some(lib) = base.library else {
        bail!(
            "{what}: mesh {mesh:?} is outside the base scene's pack and no mesh library is loaded"
        );
    };
    match lib.lookup(mesh) {
        Some(LibMesh::Base(n)) => Ok(n),
        Some(LibMesh::Added(k)) => added.index(lib.added[k].clone(), what),
        None => bail!("{what}: mesh {mesh:?} did not resolve"),
    }
}

/// The `+0x0000` records and `+0x8000` cells for the map's ground, walls
/// and props. Adds the generated wall boxes and carried-in prop meshes to
/// `added`; returns the record slots still free and the tiles whose cell
/// mesh a prop replaced ([`FieldImage::covered_cells`]).
fn write_objects(
    img: &mut [u8],
    def: &MapDef,
    grid: &TileGrid,
    base: BaseInputs<'_>,
    keep: bool,
    added: &mut Added,
) -> Result<(Vec<usize>, Vec<[u16; 2]>)> {
    let [ox, oy] = def.tiles.origin.map(usize::from);
    let cell_off = |gc: usize, gr: usize| OBJECT_GRID_OFFSET + (gr * GRID_DIM + gc) * 2;
    // Record 0 is the empty slot every unnamed cell points at. In a kept
    // base map the free slots are the all-zero records past 0.
    let mut free: Vec<usize> = (1..RECORD_SLOTS)
        .filter(|&i| {
            !keep
                || img[i * OBJECT_RECORD_STRIDE..(i + 1) * OBJECT_RECORD_STRIDE]
                    .iter()
                    .all(|&b| b == 0)
        })
        .rev()
        .collect();
    let mut alloc = |what: &str| -> Result<usize> {
        free.pop()
            .ok_or_else(|| anyhow::anyhow!("out of object records placing {what}"))
    };
    // The texture triple of each ground style, resolved once.
    let ground_texture = |g: &Ground| -> Result<Option<(u8, u8, u16)>> {
        match *g {
            Ground::Texture { tpage, clut, tile } => Ok(Some((tile, tpage, clut))),
            Ground::BaseCell { cell: [c, r] } => {
                let Some(map) = base.base_map else {
                    bail!("ground base_cell needs the base scene's field map");
                };
                let o = cell_off(usize::from(c), usize::from(r));
                let Some(word) = map.get(o..o + 2) else {
                    bail!("base_cell ({c}, {r}) is off the base map");
                };
                let idx = u16::from_le_bytes([word[0], word[1]]) & OBJECT_INDEX_MASK;
                let rec = ObjectRecord::parse(map, usize::from(idx));
                match rec {
                    Some(rec) if rec.terrain_tpage != 0 => Ok(Some((
                        rec.terrain_tile,
                        rec.terrain_tpage as u8,
                        rec.terrain_clut,
                    ))),
                    _ => bail!("base_cell ({c}, {r}) carries no ground texture"),
                }
            }
            Ground::Mesh { .. } => Ok(None),
        }
    };
    // One record per distinct ground: texture triples and decoration meshes.
    let mut texture_records: Vec<((u8, u8, u16), usize)> = Vec::new();
    let mut mesh_records: Vec<(u16, usize)> = Vec::new();
    // The texture of each map tile, for a prop record standing on it.
    let mut tile_texture: Vec<Option<(u8, u8, u16)>> = vec![None; grid.width * grid.height];
    // Wall boxes: one generated mesh per distinct box, and one record per
    // distinct (wall mesh, ground texture) pair.
    let mut wall_records: Vec<((u16, Option<GroundTexture>), usize)> = Vec::new();
    for row in 0..grid.height {
        for col in 0..grid.width {
            let c = grid.char_at(col, row).expect("in range");
            let Some(style) = def.style(c) else {
                continue;
            };
            let tex = match &style.ground {
                Some(g @ (Ground::Texture { .. } | Ground::BaseCell { .. })) => ground_texture(g)?,
                _ => None,
            };
            if let Some(tex) = tex {
                tile_texture[row * grid.width + col] = Some(tex);
            }
            let (slot, flags) = if let Some(wall) = &style.wall {
                if matches!(style.ground, Some(Ground::Mesh { .. })) {
                    bail!("tile ({col}, {row}): a mesh ground and a wall cannot share a tile");
                }
                let mesh = match wall {
                    WallMesh::Mesh { index } => *index,
                    WallMesh::Box {
                        height,
                        tpage,
                        clut,
                        tile,
                    } => added.index(wall_box_tmd(*height, *tpage, *clut, *tile)?, "a wall box")?,
                };
                let flags = if tex.is_some() {
                    FLAGS_DECORATION | FLAGS_GROUND
                } else {
                    FLAGS_DECORATION
                };
                let slot = match wall_records.iter().find(|(k, _)| *k == (mesh, tex)) {
                    Some(&(_, s)) => s,
                    None => {
                        let s = alloc("a wall")?;
                        put_record(img, s, [0; 3], [0; 3], mesh, flags, tex);
                        wall_records.push(((mesh, tex), s));
                        s
                    }
                };
                (slot, flags)
            } else if let Some(Ground::Mesh { index }) = style.ground {
                let slot = match mesh_records.iter().find(|(m, _)| *m == index) {
                    Some(&(_, s)) => s,
                    None => {
                        let s = alloc("a decoration mesh")?;
                        put_record(img, s, [0; 3], [0; 3], index, FLAGS_DECORATION, None);
                        mesh_records.push((index, s));
                        s
                    }
                };
                (slot, FLAGS_DECORATION)
            } else if let Some(tex) = tex {
                let slot = match texture_records.iter().find(|(t, _)| *t == tex) {
                    Some(&(_, s)) => s,
                    None => {
                        let s = alloc("a ground texture")?;
                        put_record(img, s, [0; 3], [0; 3], 0, FLAGS_GROUND, Some(tex));
                        texture_records.push((tex, s));
                        s
                    }
                };
                (slot, FLAGS_GROUND)
            } else {
                continue;
            };
            let o = cell_off(ox + col, oy + row);
            img[o..o + 2].copy_from_slice(&(slot as u16 | cell_gates(flags)).to_le_bytes());
        }
    }
    // Props: one placed record each, standing on its tile's cell. The record
    // carries the tile's ground texture too, so the ground under it still
    // draws (the cell names one record).
    let mut prop_tiles: Vec<(u16, u16)> = Vec::new();
    let mut covered = Vec::new();
    for prop in &def.props {
        let [col, row] = prop.tile;
        if usize::from(col) >= grid.width || usize::from(row) >= grid.height {
            bail!("prop {:?} at ({col}, {row}) is off the map", prop.id);
        }
        if prop_tiles.contains(&(col, row)) {
            bail!(
                "prop {:?}: tile ({col}, {row}) already holds a prop (one per tile)",
                prop.id
            );
        }
        prop_tiles.push((col, row));
        let mesh = mesh_index(&prop.mesh, &base, added, &format!("prop {:?}", prop.id))?;
        let tex = tile_texture[usize::from(row) * grid.width + usize::from(col)];
        // The cell names one record: a mesh the cell drew before (a kept
        // retail cell, a wall, a mesh ground) gives way to the prop.
        {
            let (gc, gr) = grid_tile(def, col, row);
            let o = cell_off(gc, gr);
            let prev = usize::from(u16::from_le_bytes([img[o], img[o + 1]]) & OBJECT_INDEX_MASK);
            let r = &img[prev * OBJECT_RECORD_STRIDE..];
            if prev != 0 && u16::from_le_bytes([r[0x12], r[0x13]]) & 0x2 != 0 {
                covered.push([col, row]);
            }
        }
        let s = alloc("a prop")?;
        let flags = if tex.is_some() {
            FLAGS_PROP | FLAGS_GROUND
        } else {
            FLAGS_PROP
        };
        put_record(
            img,
            s,
            prop.offset,
            prop.rotation.map(psx_angle),
            mesh,
            flags,
            tex,
        );
        let (gc, gr) = grid_tile(def, col, row);
        let o = cell_off(gc, gr);
        img[o..o + 2].copy_from_slice(&(s as u16 | cell_gates(flags)).to_le_bytes());
        if prop.solid && prop.top.is_none() {
            // Close the prop's tile: its wall quarters sit one byte row down.
            let b = COLLISION_GRID_OFFSET + (gr + 1) * GRID_DIM + gc;
            img[b] |= 0xF0;
        }
    }
    Ok((free, covered))
}

/// Bake the map's decorations ([`MapDef::decorations`]) into its object
/// cells: every decorated tile's cell gets a record of its own whose mesh is
/// the tile's previous cell mesh (a wall box, a mesh ground, a kept retail
/// decoration) with every decoration of the tile baked in beside it
/// ([`tmd_ops::bake`]). The record keeps the tile's ground texture, so the
/// floor under the decorations still draws.
///
/// A map decorated on more tiles than it has free records bakes **cells of
/// `c x c` tiles** instead: each cell's decorations move onto one anchor tile
/// of the cell (offset by the tile distance, lifted by the floor tiers'
/// difference), which keeps the record count under the `0x200`-record table.
/// The draw is the same; a baked cell is culled with its anchor tile.
///
/// A placed prop's tile keeps its prop: its decorations are dropped (the
/// cell names one record, and a prop's is placed). Returns how many were
/// dropped and the cell size used.
fn write_decorations(
    img: &mut [u8],
    def: &MapDef,
    grid: &TileGrid,
    base: BaseInputs<'_>,
    added: &mut Added,
    free: &mut Vec<usize>,
) -> Result<(usize, usize)> {
    use legaia_scene_formats::field_objects::FLAG_PLACED;
    let decos = def.decorations()?;
    if decos.is_empty() {
        return Ok((0, 1));
    }
    let Some(lib) = base.library else {
        bail!("decorations need the map's mesh library");
    };
    let cell_off = |gc: usize, gr: usize| OBJECT_GRID_OFFSET + (gr * GRID_DIM + gc) * 2;
    let ladder = def.floor_ladder().unwrap_or([0; FLOOR_TIERS]);
    let tier_height = |c: u16, r: u16| -> f32 {
        let t = grid
            .get(usize::from(c), usize::from(r))
            .map_or(0, |t| usize::from(t.tier));
        f32::from(ladder[t])
    };
    let prop_tiles: std::collections::BTreeSet<[u16; 2]> =
        def.props.iter().map(|p| p.tile).collect();
    let mut dropped = 0;
    let decos: Vec<_> = decos
        .into_iter()
        .filter(|d| {
            let keep = !prop_tiles.contains(&d.tile)
                && usize::from(d.tile[0]) < grid.width
                && usize::from(d.tile[1]) < grid.height;
            dropped += usize::from(!keep);
            keep
        })
        .collect();
    // The smallest cell size whose decorated cells fit the free records.
    let cells_at = |c: u16| -> std::collections::BTreeSet<[u16; 2]> {
        decos
            .iter()
            .map(|d| [d.tile[0] / c, d.tile[1] / c])
            .collect()
    };
    let chunk = (1..=8u16)
        .find(|&c| cells_at(c).len() <= free.len())
        .context("the map's decorations need more object records than a map has")?;
    let mut groups: std::collections::BTreeMap<
        [u16; 2],
        Vec<&legaia_expansion_format::map_detail::Decoration>,
    > = Default::default();
    for d in &decos {
        groups
            .entry([d.tile[0] / chunk, d.tile[1] / chunk])
            .or_default()
            .push(d);
    }
    for list in groups.values() {
        // The anchor: the cell's decorated tile nearest the cell's centre.
        let anchor = if chunk == 1 {
            list[0].tile
        } else {
            let cx = f32::from(list[0].tile[0] / chunk * chunk) + f32::from(chunk) / 2.0 - 0.5;
            let cz = f32::from(list[0].tile[1] / chunk * chunk) + f32::from(chunk) / 2.0 - 0.5;
            list.iter()
                .map(|d| d.tile)
                .min_by(|a, b| {
                    let da = (f32::from(a[0]) - cx).powi(2) + (f32::from(a[1]) - cz).powi(2);
                    let db = (f32::from(b[0]) - cx).powi(2) + (f32::from(b[1]) - cz).powi(2);
                    da.total_cmp(&db).then(a.cmp(b))
                })
                .expect("a group has a decoration")
        };
        let (gc, gr) = grid_tile(def, anchor[0], anchor[1]);
        let o = cell_off(gc, gr);
        let cell = u16::from_le_bytes([img[o], img[o + 1]]);
        let rec_idx = usize::from(cell & OBJECT_INDEX_MASK);
        let mut record = [0u8; OBJECT_RECORD_STRIDE];
        if rec_idx != 0 {
            record.copy_from_slice(
                &img[rec_idx * OBJECT_RECORD_STRIDE..(rec_idx + 1) * OBJECT_RECORD_STRIDE],
            );
        }
        let flags = u16::from_le_bytes([record[0x12], record[0x13]]);
        if flags & FLAG_PLACED != 0 {
            dropped += list.len();
            continue;
        }
        let mut parts: Vec<tmd_ops::Placed<'_>> = Vec::new();
        // The tile's own cell mesh stays as the first part.
        if rec_idx != 0 && flags & 0x2 != 0 {
            let m = u16::from_le_bytes([record[0x10], record[0x11]]);
            let raw = added
                .raw(m, lib)
                .with_context(|| format!("tile {anchor:?}: cell mesh {m} has no bytes"))?;
            parts.push(tmd_ops::Placed {
                raw,
                offset: [0.0; 3],
                rotation: [0.0; 3],
                scale: 1.0,
                frame: None,
            });
        }
        // The record's own offset and angles move the whole cell mesh; the
        // decorations go through their inverse so they land where placed.
        let word = |o: usize| i16::from_le_bytes([record[o], record[o + 1]]);
        let rec_off = [f32::from(word(0)), f32::from(word(2)), -f32::from(word(4))];
        let rec_rot = [8, 10, 12].map(|o| f32::from(word(o) as u16 & 0x0FFF) * 360.0 / 4096.0);
        let frame = (rec_off != [0.0; 3] || rec_rot != [0.0; 3]).then(|| {
            let r = tmd_ops::rotation_deg(rec_rot);
            let inv: tmd_ops::Mat3 = std::array::from_fn(|i| std::array::from_fn(|j| r[j][i]));
            (inv, rec_off)
        });
        let h0 = tier_height(anchor[0], anchor[1]);
        // (bytes, offset, rotation, scale) of each decoration.
        type Part = (Vec<u8>, [f32; 3], [f32; 3], f32);
        let mut owned: Vec<Part> = Vec::new();
        for d in list {
            let raw = lib
                .raw(&d.mesh)
                .with_context(|| format!("decoration mesh {:?} did not resolve", d.mesh))?;
            let dx = (f32::from(d.tile[0]) - f32::from(anchor[0])) * 128.0;
            let dz = (f32::from(d.tile[1]) - f32::from(anchor[1])) * 128.0;
            let lift = tier_height(d.tile[0], d.tile[1]) - h0;
            // Map frame (y up) to the retail field frame (y down).
            owned.push((
                raw.to_vec(),
                [d.offset[0] + dx, -(d.offset[1] + lift), d.offset[2] + dz],
                d.rotation,
                d.scale,
            ));
        }
        parts.extend(
            owned
                .iter()
                .map(|(raw, offset, rotation, scale)| tmd_ops::Placed {
                    raw,
                    offset: *offset,
                    rotation: *rotation,
                    scale: *scale,
                    frame,
                }),
        );
        let baked =
            tmd_ops::bake(&parts).with_context(|| format!("decorations of tile {anchor:?}"))?;
        drop(parts);
        let mesh = added.index(baked, "a decoration")?;
        let slot = free
            .pop()
            .context("out of object records placing a decoration")?;
        let flags = flags | FLAGS_DECORATION;
        record[0x10..0x12].copy_from_slice(&mesh.to_le_bytes());
        record[0x12..0x14].copy_from_slice(&flags.to_le_bytes());
        img[slot * OBJECT_RECORD_STRIDE..(slot + 1) * OBJECT_RECORD_STRIDE]
            .copy_from_slice(&record);
        img[o..o + 2].copy_from_slice(&(slot as u16 | cell_gates(flags)).to_le_bytes());
    }
    Ok((dropped, usize::from(chunk)))
}

/// The `+0x10000` block's header chain with all four sub-tables empty:
/// `offset[0] = 0x12`, each next offset the previous end plus the 2-byte gap,
/// and the leading `u16` the last end plus 2 (`docs/formats/field-map.md`,
/// "Trigger block").
fn write_empty_trigger_block(img: &mut [u8]) {
    let b = &mut img[TRIGGER_BLOCK_OFFSET..TRIGGER_BLOCK_OFFSET + 0x12];
    let mut off: u16 = 0x12;
    for k in 0..4 {
        b[2 + k * 4..4 + k * 4].copy_from_slice(&off.to_le_bytes());
        b[4 + k * 4..6 + k * 4].copy_from_slice(&0u16.to_le_bytes());
        off += 2;
    }
    b[0..2].copy_from_slice(&off.to_le_bytes());
}

/// The tallest face a wall box textures with one atlas cell; a taller box
/// stacks faces.
const WALL_FACE_HEIGHT: i32 = 128;

/// A generated wall box: a `128 x 128` tile footprint `height` world units
/// tall, its four sides and its top each textured with atlas cell `tile` of
/// texture page `tpage` through palette `clut` (all base-scene VRAM, the
/// [`Ground::Texture`] words). Built when the map loads; the bytes are the
/// map's numbers run through the TMD encoder.
///
/// The mesh is one object in the decoration-cell frame - origin at the tile
/// centre on its floor, Y down - drawn at the baked texel colour `128`
/// (unlit, the retail field mesh path). Each face is emitted in both
/// windings, so the box reads from inside a wall run as well as outside.
pub fn wall_box_tmd(height: u16, tpage: u8, clut: u16, tile: u8) -> Result<Vec<u8>> {
    use legaia_tmd::descriptor::PacketShape;
    use legaia_tmd::encode::{LEGAIA_OBJECT_SCALE, ModelGroup, ModelObject, ModelPrim, encode};
    if height == 0 {
        bail!("a wall box needs a height");
    }
    let h = i32::from(height);
    let px = usize::from(legaia_scene_formats::field_objects::GROUND_ATLAS_TILE_PX);
    let axis = legaia_scene_formats::field_objects::GROUND_ATLAS_AXIS;
    let u0 = ((usize::from(tile) % axis) * px).min(224) as u8;
    let v0 = ((usize::from(tile) / axis % axis) * px).min(224) as u8;
    let (u1, v1) = (u0 + (px as u8 - 1), v0 + (px as u8 - 1));
    let mut vertices: Vec<[i16; 3]> = Vec::new();
    let mut prims = Vec::new();
    let mut vert = |p: [i32; 3]| -> u16 {
        let p = p.map(|c| c as i16);
        match vertices.iter().position(|v| *v == p) {
            Some(i) => i as u16,
            None => {
                vertices.push(p);
                (vertices.len() - 1) as u16
            }
        }
    };
    // `quad(a, b, c, d)` is a face in perimeter order with `a`-`b` its top
    // edge; the file stores Z-order `a, b, d, c`.
    let mut quad = |a: [i32; 3], b: [i32; 3], c: [i32; 3], d: [i32; 3]| {
        let (a, b, c, d) = (vert(a), vert(b), vert(c), vert(d));
        let uvs = vec![(u0, v0), (u1, v0), (u0, v1), (u1, v1)];
        for order in [[a, b, d, c], [b, a, c, d]] {
            prims.push(ModelPrim {
                vertices: order.to_vec(),
                uvs: uvs.clone(),
                cba: clut,
                tsb: u16::from(tpage),
                colors: vec![[128, 128, 128]],
            });
        }
    };
    let e = 64;
    let corners = [[-e, -e], [e, -e], [e, e], [-e, e]];
    let mut y = 0;
    while y < h {
        let top = (y + WALL_FACE_HEIGHT).min(h);
        for k in 0..4 {
            let [x0, z0] = corners[k];
            let [x1, z1] = corners[(k + 1) % 4];
            quad([x0, -top, z0], [x1, -top, z1], [x1, -y, z1], [x0, -y, z0]);
        }
        y = top;
    }
    quad([-e, -h, -e], [e, -h, -e], [e, -h, e], [-e, -h, e]);
    encode(&[ModelObject {
        vertices,
        groups: vec![ModelGroup {
            shape: PacketShape::Ft4,
            semi_transparent: false,
            prims,
        }],
        scale: LEGAIA_OBJECT_SCALE,
    }])
}

/// Resolve the map's NPCs into actor placements: each [`DiscRef::Npc`]'s
/// retail placement read off its scene's MAN for the model byte and idle
/// clip, seated on the NPC's tile in placement slot [`npc_slot`].
///
/// Fails on an NPC with no model, a placement the scene does not have, or a
/// model from another scene than `base_scene` that is not a party member:
/// that mesh indexes another scene's model pool and samples another scene's
/// VRAM, neither of which the map loads.
pub fn resolve_map_npcs(
    index: &ProtIndex,
    def: &MapDef,
    base_scene: &str,
) -> Result<Vec<ActorPlacement>> {
    let mut lists: std::collections::BTreeMap<String, Vec<ActorPlacement>> = Default::default();
    let mut out = Vec::with_capacity(def.npcs.len());
    for (n, npc) in def.npcs.iter().enumerate() {
        let Some(DiscRef::Npc { scene, index: pi }) = &npc.model else {
            bail!("npc {:?} names no npc model", npc.id);
        };
        if !lists.contains_key(scene) {
            let s = Scene::load(index, scene)?;
            let man = s
                .field_man_payload(index)?
                .with_context(|| format!("scene `{scene}` has no MAN"))?;
            let mf = legaia_scene_formats::man_section::parse(&man)?;
            lists.insert(scene.clone(), mf.actor_placements(&man));
        }
        let src = lists[scene]
            .iter()
            .find(|p| p.index == usize::from(*pi))
            .with_context(|| format!("npc {:?}: scene `{scene}` has no placement {pi}", npc.id))?;
        if scene != base_scene && !src.special_model {
            bail!(
                "npc {:?}: placement {pi} of `{scene}` draws only in its own scene \
                 (the map's base scene is `{base_scene}`)",
                npc.id
            );
        }
        let slot = npc_slot(n).context("too many NPCs on one map")?;
        let (gc, gr) = grid_tile(def, npc.tile[0], npc.tile[1]);
        let (wx, wz) = tile_centre_world(def, npc.tile[0], npc.tile[1]);
        out.push(ActorPlacement {
            index: usize::from(slot),
            record_offset: 0,
            local_count: 0,
            model_index: src.model_index,
            special_model: src.special_model,
            anim_id: npc.clip.unwrap_or(src.anim_id),
            tile_x: gc as u8,
            tile_z: gr as u8,
            world_x: wx,
            world_z: wz,
            script_pc0: 0,
        });
    }
    Ok(out)
}

/// The map's floor ladder in the MAN convention, falling back to `base_lut`
/// (the base scene's own) when the map names none.
pub fn floor_lut(def: &MapDef, base_lut: Option<[i16; FLOOR_TIERS]>) -> [i16; FLOOR_TIERS] {
    def.floor_ladder().or(base_lut).unwrap_or([0; FLOOR_TIERS])
}

#[cfg(test)]
mod tests;
