//! The meshes a pack map draws beyond its base scene's geometry pack, and the
//! VRAM they bring with them.
//!
//! A map's props and decorations name meshes three ways
//! ([`PropMesh`]): the base scene's own pack (`{ object = N }`), another
//! scene's pack (`{ scene = "town01", object = N }` or a PROT coordinate),
//! or a glTF model the pack carries (`{ asset = "..." }`). [`resolve_mesh_library`]
//! turns every one the map names into either a base-pack index or a mesh
//! **carried in**: a TMD whose texture words point at copies of what it
//! samples, written into free VRAM of the map's ([`VramPatch`]).
//!
//! # Texture relocation
//!
//! A mesh from another scene samples that scene's VRAM. For every texture
//! page its primitives read, the loader takes the bounding rectangle of the
//! UVs, and for every palette the palette's 16 or 256 entries; each that the
//! base scene's VRAM already holds word for word (the boot-resident system
//! UI, a page two scenes share) stays where it is, and the rest are copied
//! into rectangles [`VramAlloc`] hands out. The primitives' CLUT, page and UV
//! words are rewritten to the copies ([`super::tmd_ops::retexture`]). All the
//! meshes of one source scene relocate together, so a page two of them share
//! is copied once.
//!
//! The patches are applied by the scene's resource build after its own
//! uploads (`SceneResources::build_targeted_with_options`), so both hosts and
//! every rebuild - a battle's return included - get the same VRAM.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use legaia_expansion_format::map::{MapDef, PropMesh};
use legaia_expansion_format::refs::DiscRef;
use legaia_tim::Vram;

use super::tmd_ops;
use super::vram_alloc::{Rect, VramAlloc, cba_word, cba_xy, tsb_page, tsb_with_page};
use crate::scene::{ProtIndex, Scene};
use crate::scene_resources::{BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources};

/// A rectangle of VRAM halfwords a pack map writes over its base scene's
/// build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VramPatch {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    /// `w * h` halfwords, row-major.
    pub words: Vec<u16>,
}

impl VramPatch {
    /// Copy `r` out of `vram`.
    pub fn read(vram: &Vram, r: Rect) -> Self {
        let mut words = Vec::with_capacity(usize::from(r.w) * usize::from(r.h));
        for y in r.y..r.y + r.h {
            for x in r.x..r.x + r.w {
                words.push(vram.pixel(usize::from(x), usize::from(y)));
            }
        }
        Self {
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h,
            words,
        }
    }

    /// The same words at another position.
    pub fn moved(&self, to: Rect) -> Self {
        Self {
            x: to.x,
            y: to.y,
            ..self.clone()
        }
    }
}

/// Write every patch into `vram`, in order.
pub fn apply_vram_patches(vram: &mut Vram, patches: &[VramPatch]) {
    for p in patches {
        let bytes: Vec<u8> = p.words.iter().flat_map(|w| w.to_le_bytes()).collect();
        vram.write_block(p.x, p.y, p.w, p.h, &bytes);
    }
}

/// How one [`PropMesh`] resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibMesh {
    /// Mesh `N` of the base scene's geometry pack.
    Base(u16),
    /// [`MeshLibrary::added`]`[k]`: a mesh carried into the map.
    Added(usize),
}

/// Every mesh a map names, resolved ([`resolve_mesh_library`]).
#[derive(Debug, Clone, Default)]
pub struct MeshLibrary {
    entries: Vec<(PropMesh, LibMesh)>,
    /// Meshes carried in from other scenes and model assets, texture words
    /// already pointing at [`Self::vram_patches`].
    pub added: Vec<Vec<u8>>,
    /// The base scene's geometry pack, in index order: the parts a baked
    /// decoration cell starts from.
    pub base_raw: Vec<Vec<u8>>,
    /// The VRAM the carried-in meshes sample.
    pub vram_patches: Vec<VramPatch>,
    /// Where each source scene's textures went, by scene name: what a
    /// material naming that scene's VRAM (`from = "..."`) resolves through
    /// ([`resolve_materials`]).
    pub relocations: BTreeMap<String, Relocation>,
    /// The particle sprites a map with emitters draws from
    /// ([`particle_sheet`]), written through [`Self::vram_patches`].
    pub particle_sheet: Option<legaia_expansion_format::map_detail::ParticleSheet>,
    /// The shoreline foam's texture and band palettes a map whose terrain
    /// has foaming water draws from ([`super::terrain::foam_sheet`]).
    pub foam_sheet: Option<super::terrain::FoamSheet>,
    /// Each palette a terrain's soft boundaries blend through, keyed by
    /// its CBA word, to the CBA word of its copy with the STP bit on every
    /// non-zero entry ([`super::terrain::stp_palettes`]).
    pub stp_cluts: BTreeMap<u16, u16>,
    /// The map's `[terrain]` with every cell that names another scene's
    /// VRAM (`from`) rewritten to its copy ([`borrow_terrain_cells`]);
    /// `None` when nothing is borrowed. The terrain the map's meshes draw.
    pub terrain: Option<legaia_expansion_format::terrain::TerrainDef>,
    /// The VRAM the map will draw against (the base scene's build under
    /// [`Self::vram_patches`]) on a terrain map whose boundary quads
    /// subdivide (`detail`): a sub-quad samples only a few texels, and one
    /// whose texels the hosts' texture filter would read as missing (all
    /// zero words) is drawn whole instead ([`super::terrain::tile_meshes`]).
    pub terrain_vram: Option<TerrainVram>,
    /// The terrain resolved for drawing (heights, paint, colours) from
    /// [`Self::terrain`] (or the map's own `[terrain]`): built once here,
    /// where the blend palettes need it, and reused by the image build.
    pub terrain_surface: Option<std::sync::Arc<legaia_expansion_format::terrain::TerrainSurface>>,
    /// The townsfolk the map borrows from a scene other than its base
    /// ([`NpcBank`]), when it borrows any.
    pub npc_bank: Option<NpcBank>,
}

/// The townsfolk a map borrows from another scene than its base: their
/// models carried into the map's model pool with their textures, and the
/// scene's clip bank, which poses them.
///
/// A placement's mesh is its model byte's entry of the scene's model pool,
/// and its clips are records of the scene's ANM bundle, so a placement of
/// another scene brings both. The models join the pool right after the base
/// scene's own ([`Self::first`]), and the borrowed scene's bundle stands in
/// for the base scene's: the clip ids a pack writes for these NPCs are the
/// borrowed scene's own. That is why a map borrows from one scene at most,
/// and not beside townsfolk or animated props of its base scene's, which
/// pose from the base bundle.
#[derive(Debug, Clone, Default)]
pub struct NpcBank {
    /// The scene the NPCs come from.
    pub scene: String,
    /// The base scene's own model count: the index the first carried-in
    /// model takes.
    pub first: usize,
    /// Each borrowed placement's model byte to its index in the map's pool.
    pub models: BTreeMap<u8, u8>,
    /// The carried-in models in pool order, as `(source PROT entry, TMD
    /// bytes)`, texture words pointing at [`MeshLibrary::vram_patches`].
    pub tmds: Vec<(u32, Vec<u8>)>,
    /// The scene's ANM bundle (its decoded bytes), `None` when it ships none.
    pub clips: Option<Vec<u8>>,
}

/// The one scene other than `base` whose townsfolk `def` borrows, with that
/// scene's placements. Party-member placements (special models) draw in any
/// scene and do not count. Refused: townsfolk from two other scenes, or from
/// another scene beside the base scene's own townsfolk or animated props
/// (they pose from the base scene's clip bank, which the borrowed one
/// replaces).
pub fn borrowed_npc_scene(
    index: &ProtIndex,
    def: &MapDef,
    base: &str,
) -> Result<
    Option<(
        String,
        Vec<legaia_scene_formats::man_section::ActorPlacement>,
    )>,
> {
    let mut lists: BTreeMap<String, Vec<legaia_scene_formats::man_section::ActorPlacement>> =
        BTreeMap::new();
    let mut foreign: Option<String> = None;
    let mut own: Option<&str> = None;
    for npc in &def.npcs {
        let Some(DiscRef::Npc { scene, index: pi }) = &npc.model else {
            continue;
        };
        if !lists.contains_key(scene) {
            let s = Scene::load(index, scene)?;
            let man = s
                .field_man_payload(index)?
                .with_context(|| format!("scene `{scene}` has no MAN"))?;
            let mf = legaia_scene_formats::man_section::parse(&man)?;
            lists.insert(scene.clone(), mf.actor_placements(&man));
        }
        let special = lists[scene]
            .iter()
            .find(|p| p.index == usize::from(*pi))
            .is_some_and(|p| p.special_model);
        if special {
            continue;
        }
        if scene == base {
            own.get_or_insert(npc.id.as_str());
            continue;
        }
        match &foreign {
            Some(f) if f != scene => bail!(
                "npc {:?} is from `{scene}` and another from `{f}`: a map borrows townsfolk \
                 from one scene besides its base",
                npc.id
            ),
            _ => foreign = Some(scene.clone()),
        }
    }
    let Some(scene) = foreign else {
        return Ok(None);
    };
    if let Some(id) = own {
        bail!(
            "npc {id:?} is from the base scene `{base}` and others from `{scene}`: their clip \
             banks cannot both pose"
        );
    }
    if def.base.scenery {
        bail!(
            "the map borrows townsfolk from `{scene}`, so the base scene's animated props \
             (`base.scenery`) have no clip bank to pose from"
        );
    }
    let placements = lists.remove(&scene).unwrap_or_default();
    Ok(Some((scene, placements)))
}

/// [`MeshLibrary::terrain_vram`]: the map's VRAM, shared.
#[derive(Clone)]
pub struct TerrainVram(pub std::sync::Arc<Vram>);

impl std::fmt::Debug for TerrainVram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TerrainVram")
    }
}

/// One source scene's relocated pages and palettes.
#[derive(Debug, Clone, Default)]
pub struct Relocation {
    /// `(source rect, destination origin)` of each copied texture
    /// rectangle, in halfwords.
    pub rects: Vec<(Rect, (u16, u16))>,
    /// Source CLUT word to the copy's.
    pub cluts: BTreeMap<u16, u16>,
}

impl MeshLibrary {
    /// How `mesh` resolves, `None` for a mesh the library was not built
    /// with.
    pub fn lookup(&self, mesh: &PropMesh) -> Option<LibMesh> {
        self.entries
            .iter()
            .find(|(m, _)| m == mesh)
            .map(|&(_, l)| l)
    }

    /// The file bytes of `mesh`.
    pub fn raw(&self, mesh: &PropMesh) -> Option<&[u8]> {
        match self.lookup(mesh)? {
            LibMesh::Base(n) => self.base_raw.get(usize::from(n)).map(Vec::as_slice),
            LibMesh::Added(k) => self.added.get(k).map(Vec::as_slice),
        }
    }

    /// A library over a base geometry pack alone: each `{ object = N }` the
    /// map names maps to its index, and nothing else resolves. What a
    /// disc-free build of a map image (a test, the workbench preview) uses.
    pub fn from_base_pack(base_raw: Vec<Vec<u8>>, def: &MapDef) -> Self {
        let mut lib = Self {
            base_raw,
            ..Default::default()
        };
        for m in def.mesh_refs() {
            if let PropMesh::Object { object } = m
                && usize::from(*object) < lib.base_raw.len()
                && lib.lookup(m).is_none()
            {
                lib.entries.push((m.clone(), LibMesh::Base(*object)));
            }
        }
        lib
    }

    /// Whether the library carries anything beyond the base pack's indices.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.base_raw.is_empty()
    }
}

/// Whether a map needs a [`MeshLibrary`]: it names a mesh outside the base
/// pack, or decorates (decorations bake from the pack's file bytes).
pub fn needs_library(def: &MapDef, base_entry: Option<u32>) -> bool {
    !def.decor.is_empty()
        || def.terrain.is_some()
        || !def.scatter.is_empty()
        || !def.edges.is_empty()
        || !def.decals.is_empty()
        || !def.emitters.is_empty()
        // A solid prop's footprint is measured off its mesh, and so are the
        // tops `on` rests things on.
        || def.props.iter().any(|p| (p.solid && p.top.is_none()) || !p.on.is_empty())
        // Townsfolk from another scene bring their models' textures.
        || def.npcs.iter().any(|n| {
            matches!(&n.model, Some(DiscRef::Npc { scene, .. })
                if def.base.scene_name() != Some(scene.as_str()))
        })
        || def.mesh_refs().iter().any(|m| match m {
            PropMesh::Object { .. } => false,
            PropMesh::Tmd {
                tmd: DiscRef::Tmd { prot, .. },
            } => Some(u32::from(*prot)) != base_entry,
            _ => true,
        })
}

/// A field scene's resources the way a field entry builds them: every TIM
/// uploaded, the shared blocks and the boot-resident system UI under it.
pub fn field_resources(index: &ProtIndex, scene: &Scene) -> Result<SceneResources> {
    let shared: Vec<Scene> = FIELD_SHARED_BLOCKS
        .iter()
        .filter_map(|n| Scene::load(index, n).ok())
        .collect();
    let shared_refs: Vec<&Scene> = shared.iter().collect();
    let system_ui = index.system_ui_bundle().ok();
    let (res, _) = SceneResources::build_targeted_with_options(
        scene,
        &shared_refs,
        BuildOptions {
            kind: SceneLoadKind::Field,
            upload_all_tims: true,
            system_ui: system_ui.as_deref(),
        },
    )?;
    Ok(res)
}

/// The VRAM a map's base scene runs with, for allocation: its field build,
/// the effect-texture pool and the field character atlas the hosts layer
/// with it.
fn occupancy_vram(index: &ProtIndex, res: &SceneResources) -> Vram {
    let mut vram = res.vram.clone();
    let mut pool = Vram::new();
    if crate::scene::upload_effect_textures_into_vram(index, &mut pool, true).is_ok() {
        vram.underlay(&pool);
    }
    if let Ok(mut pack) = index
        .entry_bytes(legaia_actor_formats::field_char_textures::PROT_ENTRY_INDEX)
        .and_then(|b| legaia_actor_formats::field_char_textures::parse(&b))
    {
        pack.textures.retain(|t| (1..=3).contains(&t.index));
        pack.upload_to_vram(&mut vram, false);
    }
    vram
}

/// The geometry pack of `scene` as raw TMDs in index order, out of its
/// resources.
fn geometry_pack(scene: &Scene, res: &SceneResources) -> Vec<Vec<u8>> {
    crate::field_env::env_pack_tmd_indices(scene, res)
        .into_iter()
        .map(|i| res.tmds[i].raw.clone())
        .collect()
}

/// The field scene whose block holds PROT extraction entry `entry`.
fn scene_of_entry(index: &ProtIndex, entry: u16) -> Option<String> {
    let map = index.cdname_map()?;
    legaia_prot::cdname::block_for_extraction_index(map, u32::from(entry)).map(str::to_string)
}

/// Resolve every mesh `def` names ([`MapDef::mesh_refs`]) over base scene
/// `base`, whose geometry pack lives in PROT entry `base_entry`. `assets`
/// are the pack's `assets/` files (model assets and their textures).
pub fn resolve_mesh_library(
    index: &ProtIndex,
    def: &MapDef,
    assets: &BTreeMap<String, Vec<u8>>,
    base: &Scene,
    base_entry: Option<u32>,
) -> Result<MeshLibrary> {
    let res = field_resources(index, base).context("the base scene's resources")?;
    let base_raw = geometry_pack(base, &res);
    let base_vram = occupancy_vram(index, &res);
    let mut alloc = VramAlloc::from_vram(&base_vram);
    let mut lib = MeshLibrary {
        base_raw,
        ..Default::default()
    };
    if !def.emitters.is_empty() {
        lib.particle_sheet = Some(
            particle_sheet(&mut alloc, &mut lib.vram_patches).context("the particle sprites")?,
        );
    }
    if def
        .terrain
        .as_ref()
        .and_then(|t| t.water.as_ref())
        .is_some_and(|w| w.foam > 0.0)
    {
        lib.foam_sheet = Some(
            super::terrain::foam_sheet(&mut alloc, &mut lib.vram_patches)
                .context("the shoreline foam")?,
        );
    }
    if let Some(t) = def.terrain.as_ref().filter(|t| !t.borrowed().is_empty()) {
        let mut t = t.clone();
        borrow_terrain_cells(index, &mut t, &mut alloc, &mut lib.vram_patches)
            .context("the terrain's borrowed textures")?;
        lib.terrain = Some(t);
    }
    let terrain_def = lib.terrain.as_ref().map(|t| MapDef {
        terrain: Some(t.clone()),
        ..def.clone()
    });
    if let Some(surface) = terrain_def
        .as_ref()
        .unwrap_or(def)
        .terrain_surface()
        .context("the terrain")?
        .map(std::sync::Arc::new)
    {
        lib.terrain_surface = Some(surface.clone());
        // The blend palettes copy what the map's VRAM will hold: the base
        // scene's words under the copies made so far.
        let mut vram = base_vram.clone();
        apply_vram_patches(&mut vram, &lib.vram_patches);
        lib.stp_cluts =
            super::terrain::stp_palettes(&surface, &vram, &mut alloc, &mut lib.vram_patches)
                .context("the terrain's blend palettes")?;
    }
    // Group the carried-in meshes by source scene so each scene relocates
    // once.
    let mut by_scene: BTreeMap<String, Vec<(PropMesh, u16)>> = BTreeMap::new();
    let borrowed = borrowed_npc_scene(index, def, &base.name).context("the borrowed townsfolk")?;
    if let Some((scene, _)) = &borrowed {
        by_scene.entry(scene.clone()).or_default();
    }
    for mesh in def.mesh_refs() {
        if lib.lookup(mesh).is_some() || by_scene.values().flatten().any(|(m, _)| m == mesh) {
            continue;
        }
        let base_index = |n: u16| -> Result<LibMesh> {
            if usize::from(n) >= lib.base_raw.len() {
                bail!(
                    "mesh {n} is past the base scene's geometry pack ({} meshes)",
                    lib.base_raw.len()
                );
            }
            Ok(LibMesh::Base(n))
        };
        match mesh {
            PropMesh::Object { object } => {
                let l = base_index(*object)?;
                lib.entries.push((mesh.clone(), l));
            }
            PropMesh::SceneObject { scene, object } if *scene == base.name => {
                let l = base_index(*object)?;
                lib.entries.push((mesh.clone(), l));
            }
            PropMesh::SceneObject { scene, object } => {
                by_scene
                    .entry(scene.clone())
                    .or_default()
                    .push((mesh.clone(), *object));
            }
            PropMesh::Tmd {
                tmd: DiscRef::Tmd { prot, index: n },
            } => {
                if Some(u32::from(*prot)) == base_entry {
                    let l = base_index(*n)?;
                    lib.entries.push((mesh.clone(), l));
                } else {
                    let scene = scene_of_entry(index, *prot)
                        .with_context(|| format!("PROT {prot} is in no CDNAME block"))?;
                    by_scene.entry(scene).or_default().push((mesh.clone(), *n));
                }
            }
            PropMesh::Tmd { tmd } => bail!("mesh {tmd} is not a tmd ref"),
            PropMesh::Asset { asset, colors } => {
                let raw = super::gltf::load_model(
                    assets,
                    asset,
                    colors.unwrap_or(256),
                    &mut alloc,
                    &mut lib.vram_patches,
                )
                .with_context(|| format!("model asset `{asset}`"))?;
                lib.added.push(raw);
                lib.entries
                    .push((mesh.clone(), LibMesh::Added(lib.added.len() - 1)));
            }
        }
    }
    for (scene_name, meshes) in by_scene {
        let src =
            Scene::load(index, &scene_name).with_context(|| format!("scene `{scene_name}`"))?;
        let src_res = field_resources(index, &src)
            .with_context(|| format!("scene `{scene_name}`'s resources"))?;
        let pack = geometry_pack(&src, &src_res);
        let mut raws = Vec::with_capacity(meshes.len());
        for (mesh, n) in &meshes {
            let raw = pack.get(usize::from(*n)).with_context(|| {
                format!(
                    "{mesh:?}: scene `{scene_name}`'s geometry pack has {} meshes",
                    pack.len()
                )
            })?;
            raws.push(raw.clone());
        }
        // The borrowed townsfolk's models relocate with the scene's meshes,
        // so a page they share is copied once.
        let mut bank = None;
        if let Some((_, placements)) = borrowed.as_ref().filter(|(s, _)| *s == scene_name) {
            let first = res.tmds.len() - res.shared_tmd_count;
            let scene_models = src_res.tmds.len() - src_res.shared_tmd_count;
            let mut b = NpcBank {
                scene: scene_name.clone(),
                first,
                clips: crate::npc_catalog::scene_anm_bundle(&src).map(|c| c.decoded),
                ..Default::default()
            };
            for npc in &def.npcs {
                let Some(DiscRef::Npc { scene, index: pi }) = &npc.model else {
                    continue;
                };
                let Some(p) = placements
                    .iter()
                    .find(|p| *scene == scene_name && p.index == usize::from(*pi))
                    .filter(|p| !p.special_model)
                else {
                    continue;
                };
                if b.models.contains_key(&p.model_index) {
                    continue;
                }
                let t = src_res
                    .tmds
                    .get(usize::from(p.model_index))
                    .filter(|_| usize::from(p.model_index) < scene_models)
                    .with_context(|| {
                        format!(
                            "npc {:?}: scene `{scene_name}` has no model {}",
                            npc.id, p.model_index
                        )
                    })?;
                let at = u8::try_from(first + b.tmds.len())
                    .ok()
                    .filter(|&i| i < 0xF0)
                    .with_context(|| format!("npc {:?}: the map's model pool is full", npc.id))?;
                b.models.insert(p.model_index, at);
                b.tmds.push((t.entry_idx, t.raw.clone()));
                raws.push(t.raw.clone());
            }
            bank = Some(b);
        }
        let moved = relocate(
            &mut raws,
            &src_res.vram,
            &base_vram,
            &mut alloc,
            &mut lib.vram_patches,
        )
        .with_context(|| format!("textures of scene `{scene_name}`"))?;
        lib.relocations.insert(scene_name.clone(), moved);
        if let Some(mut b) = bank {
            let models = raws.split_off(meshes.len());
            for ((_, raw), moved) in b.tmds.iter_mut().zip(models) {
                *raw = moved;
            }
            lib.npc_bank = Some(b);
        }
        for ((mesh, _), raw) in meshes.into_iter().zip(raws) {
            lib.added.push(raw);
            lib.entries
                .push((mesh, LibMesh::Added(lib.added.len() - 1)));
        }
    }
    if def.terrain.as_ref().is_some_and(|t| t.detail > 1) {
        let mut vram = res.vram.clone();
        apply_vram_patches(&mut vram, &lib.vram_patches);
        lib.terrain_vram = Some(TerrainVram(std::sync::Arc::new(vram)));
    }
    Ok(lib)
}

/// Copy every terrain cell that names another scene's VRAM
/// ([`legaia_expansion_format::terrain::TerrainMaterial::from`]) into free
/// VRAM of the map's and rewrite the cell to the copy: the cell's `32 x 32`
/// texels to a free atlas-cell slot of a page of the same depth, its
/// palette (16 or 256 entries) to a free palette row. A cell or palette two
/// materials share is copied once. Clears every `from`.
pub fn borrow_terrain_cells(
    index: &ProtIndex,
    t: &mut legaia_expansion_format::terrain::TerrainDef,
    alloc: &mut VramAlloc,
    patches: &mut Vec<VramPatch>,
) -> Result<()> {
    use legaia_scene_formats::field_objects::{GROUND_ATLAS_AXIS, GROUND_ATLAS_TILE_PX};
    let scenes: Vec<String> = t.borrowed().into_iter().map(str::to_string).collect();
    let px = u16::from(GROUND_ATLAS_TILE_PX);
    let axis = GROUND_ATLAS_AXIS as u16;
    for name in scenes {
        let src = Scene::load(index, &name).with_context(|| format!("scene `{name}`"))?;
        let res =
            field_resources(index, &src).with_context(|| format!("scene `{name}`'s resources"))?;
        // (page bits, tile) -> (new page bits, new tile); clut -> clut.
        let mut cells: BTreeMap<(u16, u8), (u16, u8)> = BTreeMap::new();
        let mut cluts: BTreeMap<(u16, u16), u16> = BTreeMap::new();
        for cell in t.borrowed_cells_mut(&name) {
            let tsb = u16::from(cell.tpage);
            let (ppx, ppy, depth) = tsb_page(tsb);
            let per = 16 / u16::from(depth);
            let w = px / per;
            let key = (tsb & 0x19F, cell.tile);
            let (ntsb, ntile) = match cells.get(&key) {
                Some(&v) => v,
                None => {
                    let (cu, cv) = (
                        u16::from(cell.tile) % axis * px,
                        u16::from(cell.tile) / axis % axis * px,
                    );
                    let from = Rect {
                        x: ppx + cu / per,
                        y: ppy + cv,
                        w,
                        h: px,
                    };
                    let (to, (dpx, dpy)) = alloc_atlas_cell(alloc, depth, w, px)
                        .with_context(|| format!("no free VRAM for a cell of `{name}`"))?;
                    patches.push(VramPatch::read(&res.vram, from).moved(to));
                    let ntile = ((to.y - dpy) / px) * axis + (to.x - dpx) / w;
                    let v = (tsb_with_page(tsb, dpx, dpy), ntile as u8);
                    cells.insert(key, v);
                    v
                }
            };
            let entries: u16 = if depth == 8 { 256 } else { 16 };
            let nclut = if depth == 16 {
                cell.clut
            } else {
                match cluts.get(&(cell.clut, entries)) {
                    Some(&c) => c,
                    None => {
                        let (x, y) = cba_xy(cell.clut);
                        let to = alloc
                            .alloc_clut(entries)
                            .with_context(|| format!("no free VRAM for a palette of `{name}`"))?;
                        patches.push(
                            VramPatch::read(
                                &res.vram,
                                Rect {
                                    x,
                                    y,
                                    w: entries,
                                    h: 1,
                                },
                            )
                            .moved(to),
                        );
                        let c = cba_word(to.x, to.y);
                        cluts.insert((cell.clut, entries), c);
                        c
                    }
                }
            };
            cell.tpage = ntsb as u8;
            cell.tile = ntile;
            cell.clut = nclut;
        }
        for m in &mut t.materials {
            if m.from.as_deref() == Some(name.as_str()) {
                m.from = None;
            }
        }
        if let Some(c) = t
            .cliff
            .as_mut()
            .filter(|c| c.from.as_deref() == Some(name.as_str()))
        {
            c.from = None;
        }
        if let Some(w) = t
            .water
            .as_mut()
            .filter(|w| w.from.as_deref() == Some(name.as_str()))
        {
            w.from = None;
        }
    }
    Ok(())
}

/// A free `w x h` rectangle at an atlas-cell slot of a texture page of
/// `depth` bits: its offset in the page a whole number of cells, so a tile
/// index names it. Claims it; returns it and its page's origin.
fn alloc_atlas_cell(
    alloc: &mut VramAlloc,
    depth: u8,
    w: u16,
    h: u16,
) -> Option<(Rect, (u16, u16))> {
    let span: u16 = match depth {
        4 => 64,
        8 => 128,
        _ => 256,
    };
    for py in [256u16, 0] {
        for pxi in (0..16u16).rev() {
            let ppx = pxi * 64;
            if ppx + span > 1024 {
                continue;
            }
            for cy in 0..256 / h {
                for cx in 0..span / w {
                    let r = Rect {
                        x: ppx + cx * w,
                        y: py + cy * h,
                        w,
                        h,
                    };
                    if alloc.is_free(r) {
                        alloc.mark(r);
                        return Some((r, (ppx, py)));
                    }
                }
            }
        }
    }
    None
}

/// The particle sprite sheet: four `16 x 16` 4bpp sprites side by side -
/// a soft dot, a four-point spark, a leaf, a drop
/// ([`legaia_expansion_format::map_detail::SPRITE_DOT`] ..), each texel's
/// index its brightness - drawn here, so the sheet is the engine's own art,
/// and two 16-entry grey ramps: one with the STP bit on every entry (the
/// glowing kinds blend) and one without (leaves draw opaque).
pub fn particle_sheet(
    alloc: &mut VramAlloc,
    patches: &mut Vec<VramPatch>,
) -> Result<legaia_expansion_format::map_detail::ParticleSheet> {
    const S: usize = 16;
    let sprite = |k: usize, x: usize, y: usize| -> u16 {
        let (fx, fy) = (
            x as f32 + 0.5 - S as f32 / 2.0,
            y as f32 + 0.5 - S as f32 / 2.0,
        );
        let r = fx.hypot(fy) / (S as f32 / 2.0);
        let v = match k {
            // Soft dot.
            0 => (1.0 - r).max(0.0).powf(1.6),
            // Spark: a bright core and four rays.
            1 => {
                let ray = (1.0 - fx.abs().min(fy.abs()) / 1.2).max(0.0) * (1.0 - r).max(0.0);
                ((1.0 - r * 2.2).max(0.0) + ray).min(1.0)
            }
            // Leaf: an ellipse along the diagonal with a darker midrib.
            2 => {
                let (a, b) = ((fx + fy) / 1.414, (fx - fy) / 1.414);
                let e = (a / 7.0).powi(2) + (b / 3.4).powi(2);
                if e > 1.0 {
                    0.0
                } else if b.abs() < 0.6 {
                    0.55
                } else {
                    0.75 + 0.25 * (1.0 - e)
                }
            }
            // Drop: round below, pointed above.
            _ => {
                let w = if fy > 0.0 {
                    4.5
                } else {
                    4.5 * (1.0 + fy / 7.5).max(0.0)
                };
                let d = (fx.abs() / w.max(0.01)).powi(2) + (fy / 7.0).powi(2);
                if d > 1.0 { 0.0 } else { 1.0 - 0.5 * d }
            }
        };
        (v * 15.0).round().clamp(0.0, 15.0) as u16
    };
    let w_words = S * 4 / 4;
    let (rect, (px, py)) = alloc
        .alloc_texture(w_words as u16, S as u16, 4, 1)
        .context("no free VRAM for the particle sprites")?;
    let mut words = vec![0u16; w_words * S];
    for k in 0..4 {
        for y in 0..S {
            for x in 0..S {
                let u = k * S + x;
                words[y * w_words + u / 4] |= sprite(k, x, y) << ((u % 4) * 4);
            }
        }
    }
    patches.push(VramPatch {
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: rect.h,
        words,
    });
    let ramp = |stp: u16| -> Vec<u16> {
        (0..16u16)
            .map(|i| {
                if i == 0 {
                    0
                } else {
                    let c = (i * 31 / 15).max(1);
                    stp | c | (c << 5) | (c << 10)
                }
            })
            .collect()
    };
    let mut cluts = [0u16; 2];
    for (n, stp) in [0x8000u16, 0].into_iter().enumerate() {
        let c = alloc
            .alloc_clut(16)
            .context("no free VRAM for a particle palette")?;
        patches.push(VramPatch {
            x: c.x,
            y: c.y,
            w: 16,
            h: 1,
            words: ramp(stp),
        });
        cluts[n] = cba_word(c.x, c.y);
    }
    Ok(legaia_expansion_format::map_detail::ParticleSheet {
        tpage: (px / 64) | ((py / 256) << 4),
        clut_glow: cluts[0],
        clut_solid: cluts[1],
        u0: ((rect.x - px) * 4) as u8,
        v0: (rect.y - py) as u8,
    })
}

/// Texels a halfword holds at each page depth.
fn texels_per_word(depth: u8) -> u16 {
    match depth {
        4 => 4,
        8 => 2,
        _ => 1,
    }
}

/// Where one source texture page's used rectangle went.
#[derive(Debug, Clone, Copy)]
struct PageMove {
    /// The destination page's origin.
    page: (u16, u16),
    /// Added to every `u` / `v` of the page's primitives.
    du: i32,
    dv: i32,
}

/// Point `raws` (meshes of one source scene) at copies of what they sample
/// in `src`, allocated out of `alloc` and appended to `patches`. A page or
/// palette `base` already holds word for word stays where it is.
pub fn relocate(
    raws: &mut [Vec<u8>],
    src: &Vram,
    base: &Vram,
    alloc: &mut VramAlloc,
    patches: &mut Vec<VramPatch>,
) -> Result<Relocation> {
    let mut moved = Relocation::default();
    // Pass 1: the UV box of each page (keyed by its position and depth) and
    // the palette size each CLUT is read at.
    let mut pages: BTreeMap<u16, [u8; 4]> = BTreeMap::new();
    let mut cluts: BTreeMap<u16, u16> = BTreeMap::new();
    for raw in raws.iter_mut() {
        tmd_ops::retexture(raw, |t| {
            let key = t.tsb & 0x19F;
            let (_, _, depth) = tsb_page(t.tsb);
            let b = pages.entry(key).or_insert([255, 255, 0, 0]);
            for &(u, v) in t.uvs.iter() {
                b[0] = b[0].min(u);
                b[1] = b[1].min(v);
                b[2] = b[2].max(u);
                b[3] = b[3].max(v);
            }
            if depth < 16 {
                let n = if depth == 4 { 16 } else { 256 };
                let e = cluts.entry(t.cba).or_insert(n);
                *e = (*e).max(n);
            }
            (t.cba, t.tsb)
        })?;
    }
    let same = |r: Rect| -> bool {
        (r.y..r.y + r.h).all(|y| {
            (r.x..r.x + r.w).all(|x| {
                let (x, y) = (usize::from(x), usize::from(y));
                base.is_written(x, y) && base.pixel(x, y) == src.pixel(x, y)
            })
        })
    };
    let mut page_moves: BTreeMap<u16, PageMove> = BTreeMap::new();
    for (&key, &[u0, v0, u1, v1]) in &pages {
        let (px, py, depth) = tsb_page(key);
        let tpw = texels_per_word(depth);
        let hx0 = u16::from(u0) / tpw;
        let hx1 = u16::from(u1) / tpw;
        let r = Rect {
            x: px + hx0,
            y: py + u16::from(v0),
            w: hx1 - hx0 + 1,
            h: u16::from(v1) - u16::from(v0) + 1,
        };
        if r.x + r.w > 1024 || same(r) {
            continue;
        }
        let Some((dst, page)) = alloc.alloc_texture(r.w, r.h, depth, 1) else {
            bail!(
                "no free VRAM for a {} x {} texture rectangle ({depth}bpp)",
                r.w,
                r.h
            );
        };
        patches.push(VramPatch::read(src, r).moved(dst));
        moved.rects.push((r, (dst.x, dst.y)));
        page_moves.insert(
            key,
            PageMove {
                page,
                du: (i32::from(dst.x) - i32::from(page.0) - i32::from(hx0)) * i32::from(tpw),
                dv: i32::from(dst.y) - i32::from(page.1) - i32::from(v0),
            },
        );
    }
    let mut clut_moves: BTreeMap<u16, u16> = BTreeMap::new();
    for (&cba, &entries) in &cluts {
        let (x, y) = cba_xy(cba);
        let r = Rect {
            x,
            y,
            w: entries.min(1024 - x),
            h: 1,
        };
        if same(r) {
            continue;
        }
        let Some(dst) = alloc.alloc_clut(r.w.max(16).div_ceil(16) * 16) else {
            bail!("no free VRAM for a {entries}-entry palette");
        };
        patches.push(VramPatch::read(src, r).moved(dst));
        clut_moves.insert(cba, cba_word(dst.x, dst.y));
        moved.cluts.insert(cba, cba_word(dst.x, dst.y));
    }
    // Pass 2: point the primitives at the copies.
    for raw in raws.iter_mut() {
        tmd_ops::retexture(raw, |t| {
            let mut tsb = t.tsb;
            if let Some(m) = page_moves.get(&(t.tsb & 0x19F)) {
                tsb = tsb_with_page(t.tsb, m.page.0, m.page.1);
                for uv in t.uvs.iter_mut() {
                    uv.0 = (i32::from(uv.0) + m.du).clamp(0, 255) as u8;
                    uv.1 = (i32::from(uv.1) + m.dv).clamp(0, 255) as u8;
                }
            }
            let cba = clut_moves.get(&t.cba).copied().unwrap_or(t.cba);
            (cba, tsb)
        })?;
    }
    Ok(moved)
}

/// Resolve the materials of `def` that name another scene's VRAM
/// (`from = "scene"`) to the copies the map's load made
/// ([`MeshLibrary::relocations`]): a palette to its copy's CLUT word, a
/// scrolled cell to the VRAM rectangle its texels now sit at. A material
/// naming the base scene, or a palette / page the load did not move (the
/// base scene already held it), keeps its words. Returns what did not
/// resolve - a scene none of the map's meshes came from, a cell outside
/// every copied rectangle - for the host to log.
pub fn resolve_materials(def: &mut MapDef, lib: &MeshLibrary, base_scene: &str) -> Vec<String> {
    use legaia_expansion_format::map_detail::MaterialFx;
    let mut problems = Vec::new();
    for (i, fx) in def.materials.iter_mut().enumerate() {
        let from = match fx {
            MaterialFx::Cycle { from, .. }
            | MaterialFx::Hsv { from, .. }
            | MaterialFx::Tint { from, .. }
            | MaterialFx::Scroll { from, .. } => from.clone(),
        };
        let Some(scene) = from.filter(|s| s != base_scene) else {
            continue;
        };
        let Some(r) = lib.relocations.get(&scene) else {
            problems.push(format!(
                "materials[{i}]: no mesh of the map comes from `{scene}`"
            ));
            continue;
        };
        match fx {
            MaterialFx::Cycle { clut, .. }
            | MaterialFx::Hsv { clut, .. }
            | MaterialFx::Tint { clut, .. } => {
                if let Some(&c) = r.cluts.get(clut) {
                    *clut = c;
                }
            }
            MaterialFx::Scroll { cell, rect, .. } => {
                let (px, py, depth) = tsb_page(u16::from(cell.tpage));
                let tpw = texels_per_word(depth);
                let side = legaia_scene_formats::field_objects::GROUND_ATLAS_TILE_PX as u16;
                let axis = legaia_scene_formats::field_objects::GROUND_ATLAS_AXIS as u16;
                let (col, row) = (
                    u16::from(cell.tile) % axis,
                    u16::from(cell.tile) / axis % axis,
                );
                let src = Rect {
                    x: px + col * side / tpw,
                    y: py + row * side,
                    w: side / tpw,
                    h: side,
                };
                // The copied rectangle holding the cell, clipped to it.
                let hit = r.rects.iter().find_map(|(from, (dx, dy))| {
                    let x0 = src.x.max(from.x);
                    let y0 = src.y.max(from.y);
                    let x1 = (src.x + src.w).min(from.x + from.w);
                    let y1 = (src.y + src.h).min(from.y + from.h);
                    (x0 < x1 && y0 < y1)
                        .then(|| [dx + (x0 - from.x), dy + (y0 - from.y), x1 - x0, y1 - y0])
                });
                match hit {
                    Some(h) => *rect = Some(h),
                    None => problems.push(format!(
                        "materials[{i}]: no copied texture of `{scene}` covers the cell"
                    )),
                }
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_tmd::descriptor::PacketShape;
    use legaia_tmd::encode::{LEGAIA_OBJECT_SCALE, ModelGroup, ModelObject, ModelPrim, encode};

    fn quad(cba: u16, tsb: u16, uv0: (u8, u8), uv1: (u8, u8)) -> Vec<u8> {
        encode(&[ModelObject {
            vertices: vec![[-10, 0, -10], [10, 0, -10], [-10, 0, 10], [10, 0, 10]],
            groups: vec![ModelGroup {
                shape: PacketShape::Ft4,
                semi_transparent: false,
                prims: vec![ModelPrim {
                    vertices: vec![0, 1, 2, 3],
                    uvs: vec![uv0, (uv1.0, uv0.1), (uv0.0, uv1.1), uv1],
                    cba,
                    tsb,
                    colors: vec![[128, 128, 128]],
                }],
            }],
            scale: LEGAIA_OBJECT_SCALE,
        }])
        .unwrap()
    }

    fn texel(vram: &Vram, cba: u16, tsb: u16, u: u8, v: u8) -> u16 {
        // 4bpp: one halfword holds four indices; the palette maps them.
        let (px, py, _) = tsb_page(tsb);
        let word = vram.pixel(
            usize::from(px + u16::from(u) / 4),
            usize::from(py + u16::from(v)),
        );
        let idx = (word >> ((u % 4) * 4)) & 0xF;
        let (cx, cy) = cba_xy(cba);
        vram.pixel(usize::from(cx + idx), usize::from(cy))
    }

    #[test]
    fn a_foreign_mesh_samples_the_same_texels_after_relocation() {
        // Source scene: a 4bpp page at (704, 0) and a palette on row 480.
        let mut src = Vram::new();
        let words: Vec<u8> = (0..64u16 * 32)
            .flat_map(|i| (i.wrapping_mul(0x1357)).to_le_bytes())
            .collect();
        src.write_block(704, 0, 64, 32, &words);
        let pal: Vec<u8> = (0..16u16)
            .flat_map(|i| (0x4000 | (i * 3)).to_le_bytes())
            .collect();
        src.write_block(0, 480, 16, 1, &pal);
        let tsb = 0x000B; // 4bpp page (704, 0)
        let cba = cba_word(0, 480);
        // The base scene holds other words there.
        let mut base = Vram::new();
        base.write_block(704, 0, 64, 32, &vec![0xAAu8; 64 * 32 * 2]);
        let before = base.clone();
        let mut alloc = VramAlloc::from_vram(&base);
        let mut raws = vec![quad(cba, tsb, (8, 4), (39, 19))];
        let mut patches = Vec::new();
        relocate(&mut raws, &src, &base, &mut alloc, &mut patches).unwrap();
        assert_eq!(patches.len(), 2, "one page rectangle, one palette");
        let mut out = base.clone();
        apply_vram_patches(&mut out, &patches);
        // The base scene's own words are untouched.
        for y in 0..32 {
            for x in 704..768 {
                assert_eq!(out.pixel(x, y), before.pixel(x, y));
            }
        }
        // The rewritten prim reads the source's texels.
        let tmd = legaia_tmd::parse(&raws[0]).unwrap();
        let o = &tmd.objects[0];
        let g = legaia_tmd::legaia_prims::iter_groups(
            &raws[0],
            o.primitives_byte_offset,
            o.primitives_byte_size,
        )
        .unwrap();
        let p = &g[0].prims[0];
        for (k, &(u, v)) in p.uvs.iter().enumerate() {
            let (su, sv) = [(8, 4), (39, 4), (8, 19), (39, 19)][k];
            assert_eq!(
                texel(&out, p.cba, p.tsb, u, v),
                texel(&src, cba, tsb, su, sv),
                "corner {k}"
            );
        }
    }

    #[test]
    fn shared_vram_is_not_copied() {
        let mut src = Vram::new();
        src.write_block(960, 256, 16, 16, &vec![7u8; 16 * 16 * 2]);
        src.write_block(0, 510, 16, 1, &[9u8; 32]);
        let base = src.clone();
        let mut alloc = VramAlloc::from_vram(&base);
        let tsb = 0x001F; // 4bpp page (960, 256)
        let mut raws = vec![quad(cba_word(0, 510), tsb, (0, 0), (15, 15))];
        let orig = raws[0].clone();
        let mut patches = Vec::new();
        relocate(&mut raws, &src, &base, &mut alloc, &mut patches).unwrap();
        assert!(patches.is_empty());
        assert_eq!(raws[0], orig);
    }
}
