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
        || !def.scatter.is_empty()
        || !def.edges.is_empty()
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
    // Group the carried-in meshes by source scene so each scene relocates
    // once.
    let mut by_scene: BTreeMap<String, Vec<(PropMesh, u16)>> = BTreeMap::new();
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
        relocate(
            &mut raws,
            &src_res.vram,
            &base_vram,
            &mut alloc,
            &mut lib.vram_patches,
        )
        .with_context(|| format!("textures of scene `{scene_name}`"))?;
        for ((mesh, _), raw) in meshes.into_iter().zip(raws) {
            lib.added.push(raw);
            lib.entries
                .push((mesh, LibMesh::Added(lib.added.len() - 1)));
        }
    }
    Ok(lib)
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
) -> Result<()> {
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
    Ok(())
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
