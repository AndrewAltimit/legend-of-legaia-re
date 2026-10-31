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
//!   stamped, and the draw side reads the scene's image unrefreshed. Every
//!   ground cell the pack writes also carries the depth-sort bit `0x8000`
//!   ([`set_cell`]).
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
    CELL_GROUND_DEPTH_SORTED, CELL_WALK_VISIBLE, COLLISION_GRID_OFFSET, GRID_DIM,
    OBJECT_GRID_OFFSET, OBJECT_INDEX_MASK, OBJECT_RECORD_STRIDE, ObjectRecord, TILE_CENTER,
};

use legaia_scene_formats::man_section::{ActorPlacement, ManFile};

use crate::scene::{FIELD_MAP_LEN, ProtIndex, Scene};

pub mod ceiling;
pub mod detail;
pub mod footprint;
pub mod gltf;
pub mod surround;
pub mod terrain;
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
/// The first record a pack map allocates: past the empty record 0 and the
/// protagonist / NPC object ids `1..=3`.
const FIRST_FREE_RECORD: usize = 4;

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

/// Write cell word `value` at `o`, keeping the cell's elevation-override bit
/// (`0x800`, a kept retail stair or raised floor) so the floor sampler still
/// reads the tile's kind-2 record.
///
/// A ground cell (`0x1000`) also takes the depth-sort bit
/// [`CELL_GROUND_DEPTH_SORTED`]. Without it the field ground pass links the
/// cell into the far ordering-table bucket, under every other primitive
/// whatever its depth - right for a retail floor, which nothing is drawn
/// below, and wrong for a pack map with a [`surround`]: the drop and its
/// faces lie hundreds of units under the floor, and from the follow camera
/// the drop past a far edge projects onto the floor in front of it, so a far
/// bucket floor let it paint straight over the rows before that edge. Sorted
/// by its own depth, the floor covers what lies below it.
fn set_cell(img: &mut [u8], o: usize, value: u16) {
    let old = u16::from_le_bytes([img[o], img[o + 1]]);
    let sort = if value & CELL_WALK_VISIBLE != 0 {
        CELL_GROUND_DEPTH_SORTED
    } else {
        0
    };
    let v = value | sort | (old & CELL_ELEVATION_OVERRIDE);
    img[o..o + 2].copy_from_slice(&v.to_le_bytes());
}

use legaia_engine_field::field_elevation::{
    CELL_ELEVATION_OVERRIDE, ElevationOverride, lookup_elevation_override,
    parse_elevation_overrides,
};

/// A kept room's own floor steps: the base scene's kind-2 elevation
/// records ([`legaia_engine_field::field_elevation`]) on the cells that
/// carry the `0x800` bit. Retail raises a stair, a dais or a terrace this
/// way rather than through the tier grid, so a kept room's walkable floor
/// is the pack's tier height **plus** the retail step on those tiles: the
/// image carries the records (the player walks the retail floor) and props
/// and decorations stand on it (a prop's `offset[1]` is always from the
/// floor the player walks).
#[derive(Debug, Clone, Default)]
pub struct KeptFloor {
    records: Vec<ElevationOverride>,
    /// The base map's cell words (`+0x8000`), for the `0x800` bit.
    cells: Vec<u16>,
}

impl KeptFloor {
    /// The kept floor of `def` over `base_map` (the base scene's `.MAP`);
    /// empty unless the map keeps the base geometry.
    pub fn new(def: &MapDef, grid: &TileGrid, base_map: Option<&[u8]>) -> Self {
        let Some(map) = base_map.filter(|_| def.base.geometry == BaseGeometry::Keep) else {
            return Self::default();
        };
        let cells: Vec<u16> = map
            .get(OBJECT_GRID_OFFSET..OBJECT_GRID_OFFSET + GRID_DIM * GRID_DIM * 2)
            .map(|b| {
                b.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes(*c))
                    .collect()
            })
            .unwrap_or_default();
        let [ox, oy] = def.tiles.origin.map(usize::from);
        let inside = |r: &ElevationOverride| {
            let (x, z) = (usize::from(r.tile_x), usize::from(r.tile_z));
            (ox..ox + grid.width).contains(&x) && (oy..oy + grid.height).contains(&z)
        };
        let records = map
            .get(TRIGGER_BLOCK_OFFSET..)
            .map(parse_elevation_overrides)
            .unwrap_or_default()
            .into_iter()
            .filter(inside)
            .collect();
        Self { records, cells }
    }

    /// The kind-2 records the image carries.
    pub fn records(&self) -> &[ElevationOverride] {
        &self.records
    }

    /// The retail step at world `(x, z)`, world units up: the kind-2
    /// record's whole-tile and sub-cell steps on a flagged cell, `0`
    /// elsewhere (the tier grid alone).
    pub fn step_up_at(&self, x: i32, z: i32) -> i32 {
        if self.records.is_empty() || x < 0 || z < 0 {
            return 0;
        }
        let (tx, tz) = (x >> 7, z >> 7);
        let flagged = self
            .cells
            .get(tz as usize * GRID_DIM + tx as usize)
            .is_some_and(|c| c & CELL_ELEVATION_OVERRIDE != 0);
        if !flagged || tx > 0xFF || tz > 0xFF {
            return 0;
        }
        lookup_elevation_override(&self.records, tx as u8, tz as u8)
            .map_or(0, |r| -r.delta_at(x, z))
    }

    /// [`Self::step_up_at`] at map tile `(col, row)`'s centre plus `(dx, dz)`.
    pub fn step_up_on(&self, def: &MapDef, col: u16, row: u16, dx: i32, dz: i32) -> i32 {
        let (cx, cz) = def.tile_centre(col, row);
        self.step_up_at(cx + dx, cz + dz)
    }
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
    /// Models of townsfolk borrowed from another scene ([`detail::NpcBank`])
    /// as `(source PROT entry, TMD bytes)`: the resource build puts them in
    /// the model pool right after the base scene's own, at index
    /// [`Self::npc_model_base`] on.
    pub npc_tmds: Vec<(u32, Vec<u8>)>,
    /// The base scene's own model count, where [`Self::npc_tmds`] start.
    pub npc_model_base: usize,
    /// The borrowed scene's ANM bundle (decoded bytes): the clip bank the
    /// map's NPCs pose from in place of the base scene's
    /// (`npc_catalog::scene_anm_bundle`).
    pub npc_clips: Option<Vec<u8>>,
    /// The draw-record key of each animated or named prop, by the
    /// field-grid tile its placement anchors on ([`prop_records`]): what a
    /// host hands `World::object_draw_turn_matrices` /
    /// `object_draw_displacements` to find the prop's live transform, and
    /// `World::hidden_object_records` to find it hidden.
    pub prop_records: Vec<((u8, u8), usize)>,
}

impl SceneOverride {
    /// The draw-record key of the prop anchored on `anchor`.
    pub fn prop_record(&self, anchor: (u8, u8)) -> Option<usize> {
        self.prop_records
            .iter()
            .find(|(a, _)| *a == anchor)
            .map(|&(_, r)| r)
    }

    /// Give each placed draw of a keyed prop its draw-record key
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
    let anchors: Vec<(u16, u16)> = def.props.iter().map(|p| (p.tile[0], p.tile[1])).collect();
    prop_scales_at(def, &anchors)
}

/// [`prop_scales`] keyed by each prop's record anchor
/// ([`FieldImage::prop_anchors`]).
pub fn prop_scales_at(def: &MapDef, anchors: &[(u16, u16)]) -> Vec<((u8, u8), f32)> {
    def.props
        .iter()
        .zip(anchors)
        .filter_map(|(p, &(ac, ar))| {
            let s = p.scale.filter(|s| s.is_finite() && *s > 0.0 && *s != 1.0)?;
            let (c, r) = grid_tile(def, ac, ar);
            Some(((u8::try_from(c).ok()?, u8::try_from(r).ok()?), s))
        })
        .collect()
}

/// The draw-record key a pack map's prop `n` draws under: past every retail
/// bind record (a MAN record index is a byte), so the two never meet.
pub const PROP_RECORD_BASE: usize = 0x1_0000;

/// The draw-record key of every prop that animates or has an id (which a
/// mode table can hide, `World::hidden_object_records`), keyed by its
/// field-grid tile - [`SceneOverride::prop_records`].
pub fn prop_records(def: &MapDef) -> Vec<((u8, u8), usize)> {
    let anchors: Vec<(u16, u16)> = def.props.iter().map(|p| (p.tile[0], p.tile[1])).collect();
    prop_records_at(def, &anchors)
}

/// [`prop_records`] keyed by each prop's record anchor
/// ([`FieldImage::prop_anchors`]).
pub fn prop_records_at(def: &MapDef, anchors: &[(u16, u16)]) -> Vec<((u8, u8), usize)> {
    def.props
        .iter()
        .zip(anchors)
        .enumerate()
        .filter(|(_, (p, _))| !p.animate.is_empty() || !p.id.is_empty())
        .filter_map(|(i, (_, &(ac, ar)))| {
            let (c, r) = grid_tile(def, ac, ar);
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
    /// The map tile each prop's record anchors on, parallel to `def.props`:
    /// its own tile, or the nearest tile without a cell mesh when its own
    /// tile already draws one. The keys [`prop_scales_at`] and
    /// [`prop_records_at`] take.
    pub prop_anchors: Vec<(u16, u16)>,
    /// Ids of props that stand in the player's way
    /// ([`footprint::Footprint::is_large_obstacle`]) and close nothing
    /// (neither `solid` nor standable): drawn buildings the player walks
    /// through.
    pub open_large_props: Vec<String>,
    /// How many `solid` decorations closed their footprint.
    pub solid_decorations: usize,
    /// What the image's collision grid was built from, kept so an edit that
    /// moves a prop in place re-derives the grid as this build did
    /// ([`collision_grid`]).
    pub collision: CollisionInputs,
}

/// The inputs of a map's collision grid past its definition: the walls a
/// terrain adds per tile (`None` on a tier-floored map) and the mesh bytes
/// of every solid prop and decoration, whose footprints close the ground.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CollisionInputs {
    pub terrain_walls: Option<Vec<u8>>,
    pub solid_meshes: Vec<(PropMesh, Vec<u8>)>,
}

impl CollisionInputs {
    fn raw(&self, mesh: &PropMesh) -> Option<&[u8]> {
        self.solid_meshes
            .iter()
            .find(|(m, _)| m == mesh)
            .map(|(_, r)| r.as_slice())
    }
}

/// The `GRID_DIM x GRID_DIM` collision grid of `def` (the field image's
/// `+0x4000` region) from `inputs`: what [`build_field_image`] writes, by
/// the same code. A prop moved in place takes its footprint along by
/// rebuilding this region alone.
pub fn collision_grid(def: &MapDef, grid: &TileGrid, inputs: &CollisionInputs) -> Vec<u8> {
    let mut img = vec![0u8; COLLISION_GRID_OFFSET + GRID_DIM * GRID_DIM];
    write_solid_collision(&mut img, def, grid, inputs.terrain_walls.as_deref(), &|m| {
        inputs.raw(m)
    });
    img.split_off(COLLISION_GRID_OFFSET)
}

/// The tile walls ([`write_collision_grid`]), then every solid prop with no
/// standable top closing its own tile and its mesh's footprint, then every
/// solid decoration closing its footprint. `raw` gives a mesh's bytes.
/// Returns how many solid decorations closed their footprint.
fn write_solid_collision<'r>(
    img: &mut [u8],
    def: &MapDef,
    grid: &TileGrid,
    terrain_walls: Option<&[u8]>,
    raw: &dyn Fn(&PropMesh) -> Option<&'r [u8]>,
) -> usize {
    write_collision_grid(img, def, grid, terrain_walls);
    for prop in def.props.iter().filter(|p| p.solid && p.top.is_none()) {
        let [col, row] = prop.tile;
        if usize::from(col) >= grid.width || usize::from(row) >= grid.height {
            continue;
        }
        // Close the prop's tile: its wall quarters sit one byte row down.
        let (gc, gr) = grid_tile(def, col, row);
        img[COLLISION_GRID_OFFSET + (gr + 1) * GRID_DIM + gc] |= 0xF0;
        if let Some(f) = raw(&prop.mesh)
            .and_then(|r| footprint::footprint(r, prop.rotation, prop.scale.unwrap_or(1.0)))
        {
            let (wx, wz) = tile_centre_world(def, col, row);
            footprint::close_hull(
                img,
                &f.hull,
                f32::from(wx) + f32::from(prop.offset[0]),
                f32::from(wz) + f32::from(prop.offset[2]),
            );
        }
    }
    let mut solid_decorations = 0;
    for d in def
        .decorations()
        .unwrap_or_default()
        .iter()
        .filter(|d| d.solid)
    {
        let Some(h) = raw(&d.mesh).and_then(|r| footprint::base_hull(r, d.rotation, d.scale))
        else {
            continue;
        };
        let (x, z) = tile_centre_world(def, d.tile[0], d.tile[1]);
        footprint::close_hull(
            img,
            &h,
            f32::from(x) + d.offset[0],
            f32::from(z) + d.offset[2],
        );
        solid_decorations += 1;
    }
    solid_decorations
}

/// [`CollisionInputs::solid_meshes`] of `def` from the mesh library.
fn solid_meshes(def: &MapDef, lib: Option<&MeshLibrary>) -> Result<Vec<(PropMesh, Vec<u8>)>> {
    let Some(lib) = lib else {
        return Ok(Vec::new());
    };
    let mut out: Vec<(PropMesh, Vec<u8>)> = Vec::new();
    let meshes = def
        .props
        .iter()
        .filter(|p| p.solid && p.top.is_none())
        .map(|p| p.mesh.clone())
        .chain(
            def.decorations()?
                .into_iter()
                .filter(|d| d.solid)
                .map(|d| d.mesh),
        );
    for m in meshes {
        if out.iter().any(|(k, _)| *k == m) {
            continue;
        }
        if let Some(r) = solid_mesh_raw(lib, &m) {
            out.push((m, r.to_vec()));
        }
    }
    Ok(out)
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
    // A terrain whose cells borrow another scene's VRAM paints with the
    // library's rewritten copy (the heights are the same either way).
    // The library built it already for the blend palettes.
    let computed;
    let surface = match base.library.and_then(|l| l.terrain_surface.as_deref()) {
        Some(s) => Some(s),
        None => {
            let borrowed = base
                .library
                .and_then(|l| l.terrain.as_ref())
                .map(|t| MapDef {
                    terrain: Some(t.clone()),
                    ..def.clone()
                });
            computed = borrowed.as_ref().unwrap_or(def).terrain_surface()?;
            computed.as_ref()
        }
    };
    let terrain_walls = match surface {
        Some(_) => Some(def.terrain_walls(grid)?),
        None => None,
    };
    // The decorations are expanded once more below; an error stops the
    // build there, as it did before.
    def.decorations()?;
    let solid_decorations =
        write_solid_collision(&mut img, def, grid, terrain_walls.as_deref(), &|m| {
            base.library.and_then(|lib| solid_mesh_raw(lib, m))
        });
    let collision = CollisionInputs {
        terrain_walls,
        solid_meshes: solid_meshes(def, base.library)?,
    };
    let mut added = Added {
        len: base.geometry_len,
        meshes: Vec::new(),
    };
    let mut cell_items = Vec::new();
    let (mut free, covered_cells, anchors, open_large_props) = write_objects(
        &mut img,
        def,
        grid,
        base,
        keep,
        &mut added,
        surface,
        &mut cell_items,
    )?;
    // Past the floor's edge: the map's surround (a terrain draws its own
    // skirt instead).
    if surface.is_none() {
        surround::write_surround(&mut img, def, grid, &mut added, &mut free)?;
    }
    // Over the floor: the ceiling's underside and the chains its hung items
    // hang from, baked with the decorations.
    cell_items.extend(ceiling::ceiling_items(
        &img,
        def,
        grid,
        base.library,
        surface,
    )?);
    if let Some(s) = surface {
        let foam = base.library.and_then(|l| l.foam_sheet.as_ref());
        let no_stp = Default::default();
        let stp = base.library.map_or(&no_stp, |l| &l.stp_cluts);
        let vram = base
            .library
            .and_then(|l| l.terrain_vram.as_ref())
            .map(|v| &*v.0);
        for (tile, raw) in terrain::tile_meshes(def, grid, s, foam, stp, vram)? {
            cell_items.push(Item {
                tile,
                src: ItemSrc::Raw(raw),
                offset: [0.0; 3],
                rotation: [0.0; 3],
                scale: 1.0,
                lift: Some(0.0),
            });
        }
        if let Some(sky) = def.terrain.as_ref().and_then(|t| t.sky.as_ref()) {
            let (tile, raw) = terrain::sky_mesh(def, grid, sky)?;
            cell_items.push(Item {
                tile,
                src: ItemSrc::Raw(raw),
                offset: [0.0; 3],
                rotation: [0.0; 3],
                scale: 1.0,
                lift: Some(0.0),
            });
        }
    }
    let (dropped, chunk) = write_decorations(
        &mut img, def, grid, base, &mut added, &mut free, surface, cell_items,
    )?;
    // A kept room keeps its own floor steps (`KeptFloor`).
    let kept = KeptFloor::new(def, grid, base.base_map);
    let records: Vec<ElevationOverride> = kept.records().iter().copied().take(0x7F0).collect();
    write_trigger_block(&mut img, &records);
    Ok(FieldImage {
        image: img,
        added_tmds: added.meshes,
        dropped_decorations: dropped,
        decoration_chunk: chunk,
        covered_cells,
        prop_anchors: anchors,
        open_large_props,
        solid_decorations,
        collision,
    })
}

/// The `+0x4000` grid. Everything off the map is a wall; a void tile takes
/// the tier of a floor neighbour above or left of it, so the ground's corner
/// heights stay level along the map's edge.
///
/// With a terrain (`terrain_walls`, the walls it adds per tile) every tier
/// is `0` - the terrain's heightfield is the floor - and each tile's wall
/// quarters take the terrain's too.
fn write_collision_grid(
    img: &mut [u8],
    def: &MapDef,
    grid: &TileGrid,
    terrain_walls: Option<&[u8]>,
) {
    let [ox, oy] = def.tiles.origin.map(usize::from);
    let tier = |c: usize, r: usize| -> u8 {
        if terrain_walls.is_some() {
            return 0;
        }
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
            let walls = grid.get(col, row).expect("in range").walls
                | terrain_walls.map_or(0, |w| w[row * grid.width + col]);
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

/// What [`write_objects`] leaves: the record slots still free, the tiles
/// whose cell mesh a prop replaced, and each prop's record anchor.
type WrittenObjects = (Vec<usize>, Vec<[u16; 2]>, Vec<(u16, u16)>, Vec<String>);

/// A prop whose footprint runs past this many world units on either axis
/// (one and a half tiles), standing in the player's way
/// ([`footprint::Footprint::is_large_obstacle`]), and neither `solid` nor
/// standable is listed in [`FieldImage::open_large_props`]: a house the
/// player walks through.
pub const OPEN_PROP_WARN_EXTENT: f32 = 192.0;

/// The file bytes a solid mesh's footprint is measured from: the library's
/// copy, or for a base-pack `{ object = N }` the base pack's.
/// A map mesh's placed shape off the library's bytes, for the resting check
/// ([`legaia_expansion_format::map_rest`]): `None` for a mesh the library
/// does not hold.
pub fn mesh_rest_shape(
    lib: &MeshLibrary,
    mesh: &PropMesh,
    rotation: [f32; 3],
    scale: f32,
) -> Option<legaia_expansion_format::map_rest::Shape> {
    footprint::rest_shape(solid_mesh_raw(lib, mesh)?, rotation, scale)
}

fn solid_mesh_raw<'a>(lib: &'a MeshLibrary, mesh: &PropMesh) -> Option<&'a [u8]> {
    lib.raw(mesh).or_else(|| match mesh {
        PropMesh::Object { object } => lib.base_raw.get(usize::from(*object)).map(Vec::as_slice),
        _ => None,
    })
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
///
/// With a terrain (`surface`) no tile carries a ground texture (the
/// terrain's meshes are the ground), and a tile's wall or mesh ground is
/// pushed to `cell_items` instead, lifted onto the terrain, for the
/// decoration bake to fold into the tile's cell.
#[allow(clippy::too_many_arguments)]
fn write_objects(
    img: &mut [u8],
    def: &MapDef,
    grid: &TileGrid,
    base: BaseInputs<'_>,
    keep: bool,
    added: &mut Added,
    surface: Option<&legaia_expansion_format::terrain::TerrainSurface>,
    cell_items: &mut Vec<Item>,
) -> Result<WrittenObjects> {
    let [ox, oy] = def.tiles.origin.map(usize::from);
    let kept = KeptFloor::new(def, grid, base.base_map.filter(|_| keep));
    let ladder = def.floor_ladder().unwrap_or([0; FLOOR_TIERS]);
    let cell_off = |gc: usize, gr: usize| OBJECT_GRID_OFFSET + (gr * GRID_DIM + gc) * 2;
    // Record 0 is the empty slot every unnamed cell points at, and object
    // ids 1..=3 name the protagonist / NPC meshes, which the field draw never
    // takes from the scene pack (`pack_mesh_index`), so a mesh in one of them
    // would not draw. In a kept base map the free slots are the all-zero
    // records past those.
    let mut free: Vec<usize> = (FIRST_FREE_RECORD..RECORD_SLOTS)
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
            if let Some(s) = surface {
                // On a terrain the wall or mesh ground stands on the
                // tile's lowest corner, baked into the tile's cell.
                let (cx, cz) = def.tile_centre(col as u16, row as u16);
                let low = [(-64, -64), (63, -64), (-64, 63), (63, 63)]
                    .iter()
                    .filter_map(|(dx, dz)| s.field.sample_world(cx + dx, cz + dz))
                    .min()
                    .unwrap_or(0);
                let src = match (&style.wall, &style.ground) {
                    (
                        Some(WallMesh::Box {
                            height,
                            tpage,
                            clut,
                            tile,
                        }),
                        _,
                    ) => Some(ItemSrc::Raw(wall_box_tmd(*height, *tpage, *clut, *tile)?)),
                    (Some(WallMesh::Mesh { index }), _) | (None, Some(Ground::Mesh { index })) => {
                        Some(ItemSrc::Mesh(PropMesh::Object { object: *index }))
                    }
                    _ => None,
                };
                if let Some(src) = src {
                    cell_items.push(Item {
                        tile: [col as u16, row as u16],
                        src,
                        offset: [0.0; 3],
                        rotation: [0.0; 3],
                        scale: 1.0,
                        lift: Some(low as f32),
                    });
                }
                continue;
            }
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
                    } => {
                        // The cell draws at its corners' mean height: the
                        // box reaches on down to the lowest corner, so a
                        // wall between two floors meets the lower one.
                        let foot = wall_foot(img, ox + col, oy + row, &ladder);
                        added.index(
                            wall_box_tmd_from(*height, foot, *tpage, *clut, *tile)?,
                            "a wall box",
                        )?
                    }
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
            set_cell(img, o, slot as u16 | cell_gates(flags));
        }
    }
    // Props: one placed record each, standing on its tile's cell. The record
    // carries the tile's ground texture too, so the ground under it still
    // draws (the cell names one record).
    let prop_tiles: Vec<(u16, u16)> = def.props.iter().map(|p| (p.tile[0], p.tile[1])).collect();
    let mut placed_tiles: Vec<(u16, u16)> = Vec::new();
    let mut anchors_taken: Vec<(u16, u16)> = Vec::new();
    let mut covered = Vec::new();
    let mut open_large = Vec::new();
    for prop in &def.props {
        let [col, row] = prop.tile;
        if usize::from(col) >= grid.width || usize::from(row) >= grid.height {
            bail!("prop {:?} at ({col}, {row}) is off the map", prop.id);
        }
        if placed_tiles.contains(&(col, row)) {
            bail!(
                "prop {:?}: tile ({col}, {row}) already holds a prop (one per tile)",
                prop.id
            );
        }
        placed_tiles.push((col, row));
        let mesh = mesh_index(&prop.mesh, &base, added, &format!("prop {:?}", prop.id))?;
        // The cell names one record. A cell that already draws a mesh (a
        // kept retail cell, a wall, a mesh ground) keeps it: the prop's
        // record moves to the nearest tile whose cell draws none, its offset
        // carrying it back to where it was placed. Only when no such tile
        // lies within three tiles does the prop replace the mesh.
        let draws_mesh = |img: &[u8], c: u16, r: u16| {
            let (gc, gr) = grid_tile(def, c, r);
            let o = cell_off(gc, gr);
            let prev = usize::from(u16::from_le_bytes([img[o], img[o + 1]]) & OBJECT_INDEX_MASK);
            let rec = &img[prev * OBJECT_RECORD_STRIDE..];
            prev != 0 && u16::from_le_bytes([rec[0x12], rec[0x13]]) & 0x2 != 0
        };
        let mut anchor = (col, row);
        if draws_mesh(img, col, row) {
            let mut best: Option<(i32, (u16, u16))> = None;
            for dr in -3i32..=3 {
                for dc in -3i32..=3 {
                    let (c, r) = (i32::from(col) + dc, i32::from(row) + dr);
                    if c < 0 || r < 0 || c as usize >= grid.width || r as usize >= grid.height {
                        continue;
                    }
                    let t = (c as u16, r as u16);
                    let d = dc * dc + dr * dr;
                    if prop_tiles.contains(&t)
                        || anchors_taken.contains(&t)
                        || draws_mesh(img, t.0, t.1)
                        || best.is_some_and(|(bd, _)| bd <= d)
                    {
                        continue;
                    }
                    best = Some((d, t));
                }
            }
            match best {
                Some((_, t)) => anchor = t,
                None => covered.push([col, row]),
            }
        }
        anchors_taken.push(anchor);
        // A tile whose style names no ground keeps the ground the cell
        // already draws (a kept retail floor), so the prop's record does not
        // punch a hole in it.
        let tex = tile_texture[usize::from(anchor.1) * grid.width + usize::from(anchor.0)].or_else(
            || {
                let (gc, gr) = grid_tile(def, anchor.0, anchor.1);
                let o = cell_off(gc, gr);
                let prev =
                    usize::from(u16::from_le_bytes([img[o], img[o + 1]]) & OBJECT_INDEX_MASK);
                let rec = &img[prev * OBJECT_RECORD_STRIDE..(prev + 1) * OBJECT_RECORD_STRIDE];
                let flags = u16::from_le_bytes([rec[0x12], rec[0x13]]);
                (prev != 0 && flags & FLAGS_GROUND != 0 && rec[0x15] != 0).then(|| {
                    (
                        rec[0x14],
                        rec[0x15],
                        u16::from_le_bytes([rec[0x16], rec[0x17]]),
                    )
                })
            },
        );
        // The offset that carries the record from its anchor tile back to the
        // prop's own; the floor tiers' difference with it.
        let ladder = def.floor_ladder().unwrap_or([0; FLOOR_TIERS]);
        let tier = |c: u16, r: u16| {
            grid.get(usize::from(c), usize::from(r))
                .map_or(0, |t| ladder[usize::from(t.tier)])
        };
        let shift = |v: i16, d: i32| (i32::from(v) + d).clamp(-32768, 32767) as i16;
        // On a terrain the prop stands on the surface under it.
        let lift = match surface {
            Some(s) => {
                let (cx, cz) = def.tile_centre(col, row);
                s.field
                    .sample_world(
                        cx + i32::from(prop.offset[0]),
                        cz + i32::from(prop.offset[2]),
                    )
                    .unwrap_or(0)
            }
            None => {
                i32::from(tier(col, row)) - i32::from(tier(anchor.0, anchor.1))
                    + kept.step_up_on(
                        def,
                        col,
                        row,
                        i32::from(prop.offset[0]),
                        i32::from(prop.offset[2]),
                    )
            }
        };
        let offset = [
            shift(prop.offset[0], (i32::from(col) - i32::from(anchor.0)) * 128),
            shift(prop.offset[1], lift),
            shift(prop.offset[2], (i32::from(row) - i32::from(anchor.1)) * 128),
        ];
        let s = alloc("a prop")?;
        let flags = if tex.is_some() {
            FLAGS_PROP | FLAGS_GROUND
        } else {
            FLAGS_PROP
        };
        put_record(
            img,
            s,
            offset,
            prop.rotation.map(psx_angle),
            mesh,
            flags,
            tex,
        );
        let (ac, ar) = grid_tile(def, anchor.0, anchor.1);
        let o = cell_off(ac, ar);
        set_cell(img, o, s as u16 | cell_gates(flags));
        // A solid prop's tile and footprint close in
        // [`write_solid_collision`]; a large one left open is flagged.
        if !prop.solid
            && prop.top.is_none()
            && base
                .library
                .and_then(|lib| solid_mesh_raw(lib, &prop.mesh))
                .and_then(|raw| footprint::footprint(raw, prop.rotation, prop.scale.unwrap_or(1.0)))
                .is_some_and(|f| f.is_large_obstacle(f32::from(prop.offset[1])))
        {
            open_large.push(prop.id.clone());
        }
    }
    Ok((free, covered, anchors_taken, open_large))
}

/// One part a decorated cell bakes: a decoration's mesh or a decal's.
struct Item {
    tile: [u16; 2],
    src: ItemSrc,
    /// Map frame, `y` up.
    offset: [f32; 3],
    rotation: [f32; 3],
    scale: f32,
    /// The floor the part stands on, world units up from tier `0`: `None`
    /// takes its tile's tier (or, on a terrain, the surface under it).
    lift: Option<f32>,
}

enum ItemSrc {
    Mesh(PropMesh),
    Raw(Vec<u8>),
}

/// A decal's mesh ([`legaia_expansion_format::map_detail::Decal`]): a flat
/// polygon `size` wide in the floor plane at its origin, textured with its
/// atlas cell (a rectangle maps the whole cell, an `n`-gon the cell's
/// inscribed circle), both windings so it reads from any side.
pub fn decal_tmd(d: &legaia_expansion_format::map_detail::Decal) -> Result<Vec<u8>> {
    decal_tmd_draped(d, None)
}

/// [`decal_tmd`] laid over the ground: `drape` gives the mesh-frame `Y`
/// (retail, down) of the ground at mesh-frame `(x, z)`, and the polygon is
/// cut into pieces about `32` units across, each vertex at the ground under
/// it, so a decal on a slope follows it rather than cutting in or floating.
/// `None` draws it flat in one piece.
pub fn decal_tmd_draped(
    d: &legaia_expansion_format::map_detail::Decal,
    drape: Option<&dyn Fn(f32, f32) -> f32>,
) -> Result<Vec<u8>> {
    use legaia_tmd::descriptor::PacketShape;
    use legaia_tmd::encode::{LEGAIA_OBJECT_SCALE, ModelGroup, ModelObject, ModelPrim, encode};
    let px = f32::from(legaia_scene_formats::field_objects::GROUND_ATLAS_TILE_PX);
    let axis = legaia_scene_formats::field_objects::GROUND_ATLAS_AXIS;
    let u0 = ((usize::from(d.cell.tile) % axis) as f32 * px).min(224.0);
    let v0 = ((usize::from(d.cell.tile) / axis % axis) as f32 * px).min(224.0);
    let (hw, hd) = (d.size[0] / 2.0, d.size[1] / 2.0);
    // The texture spans `span` cells a side, clamped to the page.
    let side = f32::from(d.span.max(1)) * px;
    let (su, sv) = ((256.0 - u0).min(side), (256.0 - v0).min(side));
    let uv = |fu: f32, fv: f32| -> (u8, u8) {
        (
            (u0 + fu * (su - 1.0)).round() as u8,
            (v0 + fv * (sv - 1.0)).round() as u8,
        )
    };
    // Pieces a side: one flat, about 32 units each draped.
    let pieces = match drape {
        None => 1,
        Some(_) => ((hw.max(hd) * 2.0 / 32.0).ceil() as usize).clamp(1, 8),
    };
    let y_at = |x: f32, z: f32| drape.map_or(0.0, |f| f(x, z));
    let mut vertices: Vec<[i16; 3]> = Vec::new();
    let mut prims = Vec::new();
    let mut tri = |a: ([f32; 2], (u8, u8)), b: ([f32; 2], (u8, u8)), c: ([f32; 2], (u8, u8))| {
        let base = vertices.len() as u16;
        for p in [a.0, b.0, c.0] {
            vertices.push([
                p[0].round() as i16,
                y_at(p[0], p[1]).round().clamp(-32768.0, 32767.0) as i16,
                p[1].round() as i16,
            ]);
        }
        for order in [[0u16, 1, 2], [0, 2, 1]] {
            let uvs = [a.1, b.1, c.1];
            prims.push(ModelPrim {
                vertices: order.iter().map(|k| base + k).collect(),
                uvs: order.iter().map(|&k| uvs[usize::from(k)]).collect(),
                cba: d.cell.clut,
                tsb: u16::from(d.cell.tpage),
                colors: vec![d.color],
            });
        }
    };
    if d.sides == 4 {
        let n = pieces as f32;
        let at = |a: usize, b: usize| {
            let (fu, fv) = (a as f32 / n, b as f32 / n);
            ([-hw + fu * 2.0 * hw, -hd + fv * 2.0 * hd], uv(fu, fv))
        };
        for b in 0..pieces {
            for a in 0..pieces {
                tri(at(a, b), at(a + 1, b), at(a + 1, b + 1));
                tri(at(a, b), at(a + 1, b + 1), at(a, b + 1));
            }
        }
    } else {
        let n = usize::from(d.sides.clamp(3, 16));
        // Rings out from the centre, `pieces` of them.
        let ring = |k: usize, r: usize| {
            let a = k as f32 / n as f32 * std::f32::consts::TAU;
            let (s, c) = a.sin_cos();
            let f = r as f32 / pieces as f32;
            (
                [c * hw * f, s * hd * f],
                uv(0.5 + c * 0.5 * f, 0.5 + s * 0.5 * f),
            )
        };
        let centre = ([0.0, 0.0], uv(0.5, 0.5));
        for k in 0..n {
            let k1 = (k + 1) % n;
            tri(centre, ring(k, 1), ring(k1, 1));
            for r in 1..pieces {
                tri(ring(k, r), ring(k, r + 1), ring(k1, r + 1));
                tri(ring(k, r), ring(k1, r + 1), ring(k1, r));
            }
        }
    }
    encode(&[ModelObject {
        vertices,
        groups: vec![ModelGroup {
            shape: PacketShape::Ft3,
            semi_transparent: d.semi,
            prims,
        }],
        scale: LEGAIA_OBJECT_SCALE,
    }])
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
///
/// On a terrain map (`surface`) the terrain's own tile meshes and the
/// walls lifted onto it (`cell_items`) bake first, every decoration stands
/// on the surface under it, and a cell whose anchor tile holds a placed
/// prop anchors on another tile of the cell (or a neighbouring cell)
/// instead of dropping its parts: the ground must draw.
#[allow(clippy::too_many_arguments)]
fn write_decorations(
    img: &mut [u8],
    def: &MapDef,
    grid: &TileGrid,
    base: BaseInputs<'_>,
    added: &mut Added,
    free: &mut Vec<usize>,
    surface: Option<&legaia_expansion_format::terrain::TerrainSurface>,
    cell_items: Vec<Item>,
) -> Result<(usize, usize)> {
    use legaia_scene_formats::field_objects::FLAG_PLACED;
    let kept = KeptFloor::new(def, grid, base.base_map);
    // Decorations name their meshes; decals are meshes built here.
    let mut decos: Vec<Item> = cell_items;
    decos.extend(def.decorations()?.into_iter().map(|d| Item {
        tile: d.tile,
        src: ItemSrc::Mesh(d.mesh),
        offset: d.offset,
        rotation: d.rotation,
        scale: d.scale,
        lift: None,
    }));
    for d in def.decals()? {
        // On a terrain the decal drapes over the surface: each vertex at
        // the ground under it, relative to the ground at its centre (where
        // the bake stands it).
        let raw = match surface {
            Some(s) => {
                let (cx, cz) = def.tile_centre(d.tile[0], d.tile[1]);
                let (ox, oz) = (cx as f32 + d.offset[0], cz as f32 + d.offset[2]);
                let ground = |x: f32, z: f32| {
                    s.field
                        .sample_world(x.round() as i32, z.round() as i32)
                        .unwrap_or(0) as f32
                };
                let h0 = ground(ox, oz);
                let (sy, cy) = d.yaw.to_radians().sin_cos();
                let drape = move |x: f32, z: f32| {
                    // The bake turns the mesh by `Ry(yaw)`.
                    let (wx, wz) = (ox + cy * x + sy * z, oz - sy * x + cy * z);
                    -(ground(wx, wz) - h0)
                };
                decal_tmd_draped(&d, Some(&drape))?
            }
            None => decal_tmd(&d)?,
        };
        decos.push(Item {
            tile: d.tile,
            src: ItemSrc::Raw(raw),
            offset: d.offset,
            rotation: [0.0, d.yaw, 0.0],
            scale: 1.0,
            lift: None,
        });
    }
    if decos.is_empty() {
        return Ok((0, 1));
    }
    let Some(lib) = base.library else {
        bail!("decorations need the map's mesh library");
    };
    let cell_off = |gc: usize, gr: usize| OBJECT_GRID_OFFSET + (gr * GRID_DIM + gc) * 2;
    let ladder = def.floor_ladder().unwrap_or([0; FLOOR_TIERS]);
    let tier_height = |c: u16, r: u16| -> f32 {
        if surface.is_some() {
            return 0.0;
        }
        let t = grid
            .get(usize::from(c), usize::from(r))
            .map_or(0, |t| usize::from(t.tier));
        f32::from(ladder[t])
    };
    // The floor an item stands on: its own lift, the terrain under it, or
    // its tile's tier.
    let item_floor = |d: &Item| -> f32 {
        if let Some(l) = d.lift {
            return l;
        }
        match surface {
            Some(s) => {
                let (cx, cz) = def.tile_centre(d.tile[0], d.tile[1]);
                s.field
                    .sample_world(
                        cx + d.offset[0].round() as i32,
                        cz + d.offset[2].round() as i32,
                    )
                    .unwrap_or(0) as f32
            }
            None => {
                tier_height(d.tile[0], d.tile[1])
                    + kept.step_up_on(
                        def,
                        d.tile[0],
                        d.tile[1],
                        d.offset[0].round() as i32,
                        d.offset[2].round() as i32,
                    ) as f32
            }
        }
    };
    let placed_at = |img: &[u8], c: u16, r: u16| -> bool {
        let (gc, gr) = grid_tile(def, c, r);
        let o = cell_off(gc, gr);
        let idx = usize::from(u16::from_le_bytes([img[o], img[o + 1]]) & OBJECT_INDEX_MASK);
        idx != 0 && {
            let rec = &img[idx * OBJECT_RECORD_STRIDE..];
            u16::from_le_bytes([rec[0x12], rec[0x13]]) & FLAG_PLACED != 0
        }
    };
    // A placed prop's anchor cell names its placed record; decorations
    // there are left out below (`FLAG_PLACED`).
    let mut dropped = 0;
    let decos: Vec<_> = decos
        .into_iter()
        .filter(|d| {
            let keep = usize::from(d.tile[0]) < grid.width && usize::from(d.tile[1]) < grid.height;
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
    let mut groups: std::collections::BTreeMap<[u16; 2], Vec<&Item>> = Default::default();
    for d in &decos {
        groups
            .entry([d.tile[0] / chunk, d.tile[1] / chunk])
            .or_default()
            .push(d);
    }
    // On a terrain a cell anchors on any of its tiles but a placed prop's;
    // a cell with none joins the nearest cell that has one.
    let mut anchors: std::collections::BTreeMap<[u16; 2], [u16; 2]> = Default::default();
    if surface.is_some() {
        let free_tile = |key: [u16; 2]| -> Option<[u16; 2]> {
            let (c0, r0) = (key[0] * chunk, key[1] * chunk);
            let mid = (f32::from(chunk) - 1.0) / 2.0;
            (0..chunk)
                .flat_map(|dr| (0..chunk).map(move |dc| [c0 + dc, r0 + dr]))
                .filter(|t| usize::from(t[0]) < grid.width && usize::from(t[1]) < grid.height)
                .filter(|t| !placed_at(img, t[0], t[1]))
                .min_by(|a, b| {
                    let d = |t: &[u16; 2]| {
                        (f32::from(t[0] - c0) - mid).powi(2) + (f32::from(t[1] - r0) - mid).powi(2)
                    };
                    d(a).total_cmp(&d(b)).then(a.cmp(b))
                })
        };
        let keys: Vec<[u16; 2]> = groups.keys().copied().collect();
        for key in keys {
            if let Some(t) = free_tile(key) {
                anchors.insert(key, t);
                continue;
            }
            let list = groups.remove(&key).expect("a key of the map");
            let mut target = None;
            'search: for ring in 1..=3i32 {
                for dr in -ring..=ring {
                    for dc in -ring..=ring {
                        let (c, r) = (i32::from(key[0]) + dc, i32::from(key[1]) + dr);
                        if c < 0 || r < 0 {
                            continue;
                        }
                        let k = [c as u16, r as u16];
                        if let Some(t) = free_tile(k) {
                            target = Some((k, t));
                            break 'search;
                        }
                    }
                }
            }
            match target {
                Some((k, t)) => {
                    anchors.insert(k, t);
                    groups.entry(k).or_default().extend(list);
                }
                None => dropped += list.len(),
            }
        }
    }
    // The distinct meshes the decorations place, each read once.
    let mut bake_cache = tmd_ops::BakeCache::default();
    for (key, list) in &groups {
        // The anchor: the cell's decorated tile nearest the cell's centre.
        let anchor = if let Some(&t) = anchors.get(key) {
            t
        } else if chunk == 1 {
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
        let h0 = if surface.is_some() {
            0.0
        } else {
            tier_height(anchor[0], anchor[1])
        };
        for d in list.iter() {
            let raw: &[u8] = match &d.src {
                ItemSrc::Mesh(m) => lib
                    .raw(m)
                    .with_context(|| format!("decoration mesh {m:?} did not resolve"))?,
                ItemSrc::Raw(r) => r,
            };
            let dx = (f32::from(d.tile[0]) - f32::from(anchor[0])) * 128.0;
            let dz = (f32::from(d.tile[1]) - f32::from(anchor[1])) * 128.0;
            let lift = item_floor(d) - h0;
            // Map frame (y up) to the retail field frame (y down).
            parts.push(tmd_ops::Placed {
                raw,
                offset: [d.offset[0] + dx, -(d.offset[1] + lift), d.offset[2] + dz],
                rotation: d.rotation,
                scale: d.scale,
                frame,
            });
        }
        let baked = tmd_ops::bake_cached(&parts, &mut bake_cache)
            .with_context(|| format!("decorations of tile {anchor:?}"))?;
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
        set_cell(img, o, slot as u16 | cell_gates(flags));
    }
    Ok((dropped, usize::from(chunk)))
}

/// The `+0x10000` block's header chain: `offset[0] = 0x12`, each next
/// offset the previous body's end plus the 2-byte gap, and the leading `u16`
/// the last end plus 2 (`docs/formats/field-map.md`, "Trigger block"). The
/// kind-2 sub-table holds `elevation` (a kept room's floor steps); the other
/// three are empty.
fn write_trigger_block(img: &mut [u8], elevation: &[ElevationOverride]) {
    let b = &mut img[TRIGGER_BLOCK_OFFSET..TRIGGER_BLOCK_OFFSET + 0x2000];
    let mut off: u16 = 0x12;
    for k in 0..4 {
        let n = if k == 2 { elevation.len() } else { 0 };
        b[2 + k * 4..4 + k * 4].copy_from_slice(&off.to_le_bytes());
        b[4 + k * 4..6 + k * 4].copy_from_slice(&(n as u16).to_le_bytes());
        if k == 2 {
            for (i, r) in elevation.iter().enumerate() {
                let at = usize::from(off) + i * 4;
                b[at..at + 4].copy_from_slice(&[r.tile_x, r.tile_z, r.coarse as u8, r.quads]);
            }
        }
        off += (n * 4) as u16 + 2;
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
    wall_box_tmd_from(height, 0, tpage, clut, tile)
}

/// How far below the height its cell draws at (the mean of its four corner
/// tiles' floor heights, `Placement::world_y`) field-grid tile `(gc, gr)`'s
/// lowest corner lies: what a wall box there reaches down by.
fn wall_foot(img: &[u8], gc: usize, gr: usize, ladder: &[i16; FLOOR_TIERS]) -> u16 {
    let h = |c: usize, r: usize| -> i32 {
        let b = img
            .get(COLLISION_GRID_OFFSET + r.min(GRID_DIM - 1) * GRID_DIM + c.min(GRID_DIM - 1))
            .copied()
            .unwrap_or(0);
        i32::from(ladder[usize::from(b & 0x0F)])
    };
    let c = [h(gc, gr), h(gc + 1, gr), h(gc, gr + 1), h(gc + 1, gr + 1)];
    // Retail sums the corners' Y-down heights and divides toward zero.
    let mean = -((-c.iter().sum::<i32>()) / 4);
    let low = c.iter().copied().min().unwrap_or(0);
    (mean - low).clamp(0, i32::from(u16::MAX)) as u16
}

/// [`wall_box_tmd`] whose sides reach on `foot` world units below the
/// cell's floor: a wall on a tier edge, its cell drawn at the mean of
/// corners on two floors, meets the lower floor with no gap under it.
pub fn wall_box_tmd_from(
    height: u16,
    foot: u16,
    tpage: u8,
    clut: u16,
    tile: u8,
) -> Result<Vec<u8>> {
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
    let mut y = -i32::from(foot);
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

/// Fill each borrowed NPC's clips from its retail placement's motion stream
/// where the pack names none: the default-move record the stream installs
/// (`legaia_scene_formats::man_motion::placement_default_move`) gives the
/// clip the actor rests in, which becomes [`NpcDef::clip`], and the clip it
/// walks in, which a patrolling NPC takes as [`NpcDef::walk_clip`].
///
/// A retail placement's clip byte is not always its rest clip: Rim Elm's
/// old man (`town01` placement 37) is placed on his walk cycle and his
/// stream restamps the rest clip on its first tick. A pack map runs no
/// stream, so without this the NPC would mark time in place - and a patrol
/// would glide in its idle pose.
///
/// [`NpcDef::clip`]: legaia_expansion_format::map::NpcDef::clip
/// [`NpcDef::walk_clip`]: legaia_expansion_format::map::NpcDef::walk_clip
pub fn fill_borrowed_npc_clips(index: &ProtIndex, def: &mut MapDef) -> Result<()> {
    let mut moves: std::collections::BTreeMap<String, Option<(Vec<u8>, ManFile)>> =
        Default::default();
    for npc in &mut def.npcs {
        let Some(DiscRef::Npc { scene, index: pi }) = &npc.model else {
            continue;
        };
        if npc.clip.is_some() && (npc.walk_clip.is_some() || npc.patrol.is_empty()) {
            continue;
        }
        if !moves.contains_key(scene) {
            let s = Scene::load(index, scene)?;
            let parsed = s.field_man_payload(index)?.and_then(|man| {
                legaia_scene_formats::man_section::parse(&man)
                    .ok()
                    .map(|f| (man, f))
            });
            moves.insert(scene.clone(), parsed);
        }
        let Some((man, mf)) = moves[scene].as_ref() else {
            continue;
        };
        let Some([rest, walk]) =
            legaia_scene_formats::man_motion::placement_default_move(man, mf, usize::from(*pi))
        else {
            continue;
        };
        if rest != 0 {
            npc.clip.get_or_insert(rest);
        }
        if !npc.patrol.is_empty() && walk != 0 && walk != rest {
            npc.walk_clip.get_or_insert(walk);
        }
    }
    Ok(())
}

/// Resolve the map's NPCs into actor placements: each [`DiscRef::Npc`]'s
/// retail placement read off its scene's MAN for the model byte and idle
/// clip, seated on the NPC's tile in placement slot [`npc_slot`].
///
/// A placement of another scene than `base_scene` that is not a party
/// member takes the model index `bank` carried its model in at
/// ([`detail::NpcBank`]). Fails on an NPC with no model, a placement the
/// scene does not have, or another scene's townsperson `bank` does not
/// carry.
pub fn resolve_map_npcs(
    index: &ProtIndex,
    def: &MapDef,
    base_scene: &str,
    bank: Option<&detail::NpcBank>,
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
        let mut model_index = src.model_index;
        if scene != base_scene && !src.special_model {
            if let Some(&at) = bank
                .filter(|b| b.scene == *scene)
                .and_then(|b| b.models.get(&src.model_index))
            {
                model_index = at;
            } else {
                bail!(
                    "npc {:?}: placement {pi} of `{scene}` draws only in its own scene \
                 (the map's base scene is `{base_scene}`)",
                    npc.id
                );
            }
        }
        let slot = npc_slot(n).context("too many NPCs on one map")?;
        let (gc, gr) = grid_tile(def, npc.tile[0], npc.tile[1]);
        let (wx, wz) = tile_centre_world(def, npc.tile[0], npc.tile[1]);
        out.push(ActorPlacement {
            index: usize::from(slot),
            record_offset: 0,
            local_count: 0,
            model_index,
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

/// The base scene's animated scene props as scenery placements
/// ([`legaia_expansion_format::map::MapBase::scenery`]): every placement of
/// its MAN that is neither a talker nor a door, carries a clip and draws
/// from the scene's own pool, at the position its MAN header gives, in
/// placement slots from `first_slot` up (after the map's own NPCs). Empty
/// when the map does not ask for them.
pub fn base_scenery(
    index: &ProtIndex,
    def: &MapDef,
    base_scene: &str,
    first_slot: usize,
) -> Result<Vec<ActorPlacement>> {
    if !def.base.scenery {
        return Ok(Vec::new());
    }
    let picks =
        crate::npc_catalog::scene_npc_picks(index, base_scene).map_err(anyhow::Error::msg)?;
    let s = Scene::load(index, base_scene)?;
    let man = s
        .field_man_payload(index)?
        .with_context(|| format!("scene `{base_scene}` has no MAN"))?;
    let mf = legaia_scene_formats::man_section::parse(&man)?;
    let placements = mf.actor_placements(&man);
    let mut out = Vec::new();
    for pick in picks
        .iter()
        .filter(|p| p.kind == "prop" && !p.special && p.anim_id != 0)
    {
        let Some(src) = placements
            .iter()
            .find(|p| p.index == usize::from(pick.index))
        else {
            continue;
        };
        let slot = first_slot + out.len();
        if slot >= 0xF0 {
            break;
        }
        out.push(ActorPlacement {
            index: slot,
            record_offset: 0,
            local_count: 0,
            script_pc0: 0,
            ..src.clone()
        });
    }
    Ok(out)
}

/// The map's floor ladder in the MAN convention, falling back to `base_lut`
/// (the base scene's own) when the map names none.
///
/// A map with a terrain floors on its heightfield instead: every tier is
/// written `0`, and the ladder is all zero so the decoration cells and
/// placed props stand at the heights the terrain gives them.
pub fn floor_lut(def: &MapDef, base_lut: Option<[i16; FLOOR_TIERS]>) -> [i16; FLOOR_TIERS] {
    if def.terrain.is_some() {
        return [0; FLOOR_TIERS];
    }
    def.floor_ladder().or(base_lut).unwrap_or([0; FLOOR_TIERS])
}

#[cfg(test)]
mod tests;
