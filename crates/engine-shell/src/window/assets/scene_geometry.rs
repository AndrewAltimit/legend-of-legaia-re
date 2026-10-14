//! The scene's GPU geometry upload: the static env meshes, the posed
//! placed-object meshes and the ground heightfield, accumulated into one
//! [`SceneGeometry`] the later stages resolve draws against.

use super::*;
use legaia_engine_render::Renderer;
use legaia_engine_scene::scene_resources::ResolvedTmd;

/// What the geometry stages of [`PlayWindowApp::upload_assets`] accumulate:
/// the uploaded meshes of both pipelines, the bridges from an uploaded slot
/// back to its `res.tmds` index, and the per-mesh records the draw resolvers
/// and the lighting pass read.
pub(super) struct SceneGeometry {
    /// The textured (VRAM) pipeline's uploaded meshes.
    pub(super) meshes: Vec<UploadedVramMesh>,
    /// `LEGAIA_DIAG_DRAWS`: the census beside each pushed mesh.
    pub(super) census: Vec<Option<legaia_engine_system::draw_census::MeshCensus>>,
    census_on: bool,
    pub(super) tmd_data: Vec<(legaia_tmd::Tmd, Vec<u8>)>,
    /// `res.tmds` index for each pushed mesh (meshes skip empty-prim
    /// TMDs, so this is the bridge back to a `ResolvedTmd`).
    pub(super) tmd_src_index: Vec<usize>,
    /// Untextured (F*/G*) prop meshes + their res.tmds-index bridge,
    /// parallel to `meshes` / `tmd_src_index` but on the colour pipeline.
    pub(super) color_meshes: Vec<UploadedColorMesh>,
    pub(super) color_tmd_src_index: Vec<usize>,
    /// Model-space light-emitter samples per res.tmds slot (candle
    /// flames / wall-light glow prims), split by pipeline half so a
    /// mixed mesh appearing in both draw bridges never double-counts.
    /// Consumed after the placement lists resolve, where every
    /// *instance* of an emitting mesh contributes its own world-space
    /// samples (see `legaia_engine_render::scene_lights`).
    pub(super) tmd_vram_emitters: Vec<Vec<legaia_render_kernels::scene_lighting::EmitterSample>>,
    pub(super) tmd_color_emitters: Vec<Vec<legaia_render_kernels::scene_lighting::EmitterSample>>,
    /// `meshes` index -> the processed CPU mesh and its lit vertices
    /// ([`FieldLitMeshes::sources`]).
    pub(super) lit_sources: std::collections::HashMap<
        usize,
        (
            legaia_tmd::mesh::VramMesh,
            Vec<Option<legaia_tmd::mesh::LitVertex>>,
        ),
    >,
    pub(super) posed_placement_meshes: PosedPlacementMeshes,
    pub(super) posed_tmds: Vec<(legaia_tmd::Tmd, Vec<u8>)>,
    pub(super) world_map_hf: Option<UploadedVramMesh>,
    /// The CPU copy the visible-tile crop re-uploads a cropped index
    /// list from (`ground_crop` in `handle_redraw`).
    pub(super) ground_src: Option<GroundSource>,
    pub(super) ground_census: Option<legaia_engine_system::draw_census::MeshCensus>,
}

impl SceneGeometry {
    /// Empty accumulators for a scene of `tmd_count` resolved TMDs.
    pub(super) fn new(tmd_count: usize, census_on: bool) -> Self {
        Self {
            meshes: Vec::new(),
            census: Vec::new(),
            census_on,
            tmd_data: Vec::new(),
            tmd_src_index: Vec::new(),
            color_meshes: Vec::new(),
            color_tmd_src_index: Vec::new(),
            tmd_vram_emitters: vec![Vec::new(); tmd_count],
            tmd_color_emitters: vec![Vec::new(); tmd_count],
            lit_sources: std::collections::HashMap::new(),
            posed_placement_meshes: PosedPlacementMeshes::new(),
            posed_tmds: Vec::new(),
            world_map_hf: None,
            ground_src: None,
            ground_census: None,
        }
    }
}

/// The draw census of one mesh, when `LEGAIA_DIAG_DRAWS` is on.
fn mesh_census_of(
    census_on: bool,
    v: &legaia_tmd::mesh::VramMesh,
) -> Option<legaia_engine_system::draw_census::MeshCensus> {
    census_on.then(|| {
        legaia_engine_system::draw_census::MeshCensus::from_mesh(
            &v.positions,
            &v.cba_tsb,
            &v.colors,
            &v.indices,
        )
    })
}

/// One texture reference, decoded for the mesh diagnostics.
fn texture_ref_label(cba: u16, tsb: u16) -> String {
    format!(
        "cba={cba:#x} CLUT({},{}) tsb={tsb:#x} page({},{}) bpp{}",
        (cba & 0x3f) * 16,
        cba >> 6,
        (tsb & 0xf) * 64,
        ((tsb >> 4) & 1) * 256,
        4 << ((tsb >> 7) & 3)
    )
}

/// `LEGAIA_DIAG_PLACE`: why a mesh's textured half came back empty.
fn log_dropped_mesh(
    src_i: usize,
    rtmd: &ResolvedTmd,
    res: &SceneResources,
    color_mesh_empty: bool,
) {
    let (_, stats) = rtmd.build_filtered_vram_mesh_reasoned(&res.vram);
    let mut missing = std::collections::BTreeSet::new();
    let _ = legaia_tmd::mesh::tmd_to_vram_mesh_filtered(&rtmd.tmd, &rtmd.raw, |cba, tsb, uvs| {
        if !res.vram.prim_has_texture_data(cba, tsb, uvs) {
            missing.insert((cba, tsb));
        }
        false
    });
    let decoded: Vec<String> = missing
        .iter()
        .map(|&(cba, tsb)| texture_ref_label(cba, tsb))
        .collect();
    log::info!(
        "DIAG mesh drop: res {} (entry {} off {:#x}): textured filter empty \
         ({stats:?}, color mesh empty: {}) wants: {}",
        src_i,
        rtmd.entry_idx,
        rtmd.offset,
        color_mesh_empty,
        decoded.join(" | ")
    );
}

/// `LEGAIA_DIAG_MESHTEX=<res index>`: a KEPT mesh's surviving texture
/// references (distinct CBA/TSB pairs) + AABB, for chasing a mesh that
/// renders flat / mis-textured.
fn log_kept_mesh_textures(src_i: usize, rtmd: &ResolvedTmd, vmesh: &legaia_tmd::mesh::VramMesh) {
    let mut pairs = std::collections::BTreeSet::new();
    for ct in &vmesh.cba_tsb {
        pairs.insert((ct[0], ct[1]));
    }
    let decoded: Vec<String> = pairs
        .iter()
        .map(|&(cba, tsb)| texture_ref_label(cba, tsb))
        .collect();
    let mut color_hist = std::collections::BTreeMap::new();
    for c in &vmesh.colors {
        *color_hist.entry(*c).or_insert(0usize) += 1;
    }
    let (mlo, mhi) = vmesh.aabb();
    log::info!(
        "DIAG mesh tex: res {} (entry {} off {:#x}) verts {} tris {} \
         aabb {mlo:?}..{mhi:?} refs: {} colors: {:?}",
        src_i,
        rtmd.entry_idx,
        rtmd.offset,
        vmesh.positions.len(),
        vmesh.indices.len() / 3,
        decoded.join(" | "),
        color_hist
    );
}

impl PlayWindowApp {
    /// Upload every resolved scene TMD: its untextured half to the colour
    /// pipeline, its VRAM-filtered textured half to the textured one, with
    /// the coplanar resolution, emissive tagging and light-emitter samples
    /// each mesh takes on the way.
    pub(super) fn upload_env_meshes(
        &self,
        r: &Renderer,
        res: &SceneResources,
        geo: &mut SceneGeometry,
    ) {
        let census_on = geo.census_on;
        let mesh_census = |v: &legaia_tmd::mesh::VramMesh| mesh_census_of(census_on, v);
        let SceneGeometry {
            meshes,
            census,
            tmd_data,
            tmd_src_index,
            color_meshes,
            color_tmd_src_index,
            tmd_vram_emitters,
            tmd_color_emitters,
            lit_sources,
            ..
        } = geo;
        for (src_i, rtmd) in res.tmds.iter().enumerate() {
            // Build this mesh's untextured (F*/G*) vertex-colour primitives
            // and upload them to the colour pipeline. `tmd_to_color_mesh`
            // skips textured groups, so these are DISJOINT from the VRAM-mesh
            // primitives below: a mesh carrying both textured and untextured
            // primitives now renders BOTH halves rather than only the
            // textured one (the old code built the colour mesh only when the
            // textured build came back empty, silently dropping the
            // untextured half of a mixed mesh). A mesh whose only textured
            // prims reference missing CLUTs yields an empty colour mesh AND
            // an empty filtered VRAM mesh, so it correctly stays dropped.
            let mut cmesh = legaia_tmd::mesh::tmd_to_color_mesh(&rtmd.tmd, &rtmd.raw);

            // Use the VRAM-aware filter so prims whose CBA / TSB sample
            // un-uploaded regions get dropped at mesh-build time. This
            // matches the asset-viewer's cleanup and avoids the "flat
            // green CLUT[0]" shells over correctly-textured geometry
            // that the unfiltered builder produces.
            let (mut vmesh, lit_vertices) = rtmd.build_filtered_vram_mesh_lit_vertices(&res.vram);
            // Coplanar z-fight resolution (`legaia_tmd::mesh::coplanar`):
            // flag double-sided prim pairs for the shaders' per-prim
            // facing discard (retail NCLIP keeps only the camera-facing
            // copy), then nudge distinct coplanar decal layers half a
            // unit toward their visible side so the depth buffer resolves
            // them deterministically instead of per-pixel fighting. Both
            // halves go through ONE stream (`resolve_hybrid`): the baked
            // floor shadows / wall paintings are untextured colour prims
            // lying on textured bases, pairs neither single-mesh pass
            // sees - running the passes on the textured half alone left
            // every such decal fighting (koin4 shop-floor shadows).
            {
                let (ds, nudged) = legaia_tmd::mesh::resolve_hybrid(&mut vmesh, &mut cmesh);
                if ds + nudged > 0 {
                    log::debug!(
                        "play-window: mesh res {src_i}: {ds} double-sided pairs flagged, \
                         {nudged} coplanar prims nudged"
                    );
                }
            }
            // Enhanced lighting: tag the emissive prims (TSB / blend bit
            // 13) - the shared `scene_lighting` rule the browser page's
            // mesh build runs too. Inert unless the enhancement is on.
            let curated = legaia_render_kernels::scene_lighting::tag_emissive_meshes(
                &rtmd.raw, &mut vmesh, &mut cmesh, &res.vram,
            );
            if !cmesh.is_empty() {
                tmd_color_emitters[src_i] =
                    legaia_render_kernels::scene_lighting::color_mesh_emitters(
                        &cmesh.positions,
                        &cmesh.colors,
                        &cmesh.blend,
                        &cmesh.indices,
                    );
                match r.upload_color_mesh_blended(
                    &cmesh.positions,
                    &cmesh.colors,
                    &cmesh.indices,
                    &cmesh.blend,
                ) {
                    Ok(m) => {
                        color_meshes.push(m);
                        color_tmd_src_index.push(src_i);
                    }
                    Err(e) => log::warn!("color mesh upload skipped: {e:#}"),
                }
            }
            if vmesh.indices.is_empty() {
                if std::env::var_os("LEGAIA_DIAG_PLACE").is_some() {
                    log_dropped_mesh(src_i, rtmd, res, cmesh.is_empty());
                }
                // No textured prims survived the VRAM filter; the colour prims
                // (if any) were already uploaded above.
                continue;
            }
            // Diag: `LEGAIA_DIAG_MESHTEX=<res index>` dumps a KEPT mesh's
            // surviving texture references (distinct CBA/TSB pairs) + AABB,
            // for chasing a mesh that renders flat / mis-textured.
            if std::env::var("LEGAIA_DIAG_MESHTEX")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                == Some(src_i)
            {
                log_kept_mesh_textures(src_i, rtmd, &vmesh);
            }
            tmd_vram_emitters[src_i] = legaia_render_kernels::scene_lighting::vram_mesh_emitters(
                &vmesh.positions,
                &vmesh.cba_tsb,
                &vmesh.colors,
                &vmesh.indices,
                Some((&res.vram, &vmesh.uvs)),
            );
            // A curated emissive mesh (the Genesis Tree) casts one strong
            // light from its centre.
            if let Some(hit) = curated {
                tmd_vram_emitters[src_i].push(
                    legaia_render_kernels::scene_lighting::curated_mesh_sample(&hit),
                );
            }
            match r.upload_vram_mesh(
                &vmesh.positions,
                &vmesh.uvs,
                &vmesh.cba_tsb,
                &vmesh.normals,
                &vmesh.colors,
                &vmesh.indices,
            ) {
                Ok(m) => {
                    // The light-source rows: keep the processed mesh so
                    // each env draw can take a copy shaded at its own
                    // rotation (`rebuild_field_lit_meshes`) - the prologue
                    // legs included: their dim ambient is the scene-load
                    // back colour the light kernel already carries, not a
                    // second staging.
                    if lit_vertices.len() == vmesh.colors.len()
                        && legaia_engine_field::field_lit_mesh::has_lit_rows(&lit_vertices)
                    {
                        lit_sources.insert(meshes.len(), (vmesh.clone(), lit_vertices));
                    }
                    tmd_data.push((rtmd.tmd.clone(), rtmd.raw.clone()));
                    census.resize(meshes.len(), None);
                    census.push(mesh_census(&vmesh));
                    meshes.push(m);
                    tmd_src_index.push(src_i);
                }
                Err(e) => log::warn!("TMD upload skipped: {e:#}"),
            }
        }
    }

    /// **Posed** static-object meshes. A placed `.MAP` object whose bind
    /// names an anim id (`field_env::ObjectBind`) is a multi-object prop
    /// whose TMD objects are the *bones* of that clip - Rim Elm's
    /// cupboard is the canonical one: object 0 is the cabinet, objects 1
    /// and 2 are its two doors, authored about their own hinges. Retail
    /// draws such an actor through `FUN_8001b964`, which applies the
    /// clip's per-bone rigid transform to each object; drawing them raw
    /// leaves the doors floating inside the cabinet, below the floor.
    ///
    /// The rest state is **frame 0** (doors closed); the clip's later
    /// frames are the door swinging open, which retail steps once the
    /// object's bind script is resumed by body contact. Frame 0 is baked
    /// once per (mesh, anim) pair and instanced - the cheap path every
    /// prop sits on until it is touched - while a prop whose clip has
    /// moved off frame 0 is re-posed per frame at the draw site (see
    /// `PlayWindowApp::field_posed_props` / `field_prop_anims`).
    pub(super) fn upload_posed_placement_meshes(
        &self,
        r: &Renderer,
        res: &SceneResources,
        scene_bundle: Option<&legaia_actor_formats::player_anm::PlayerAnmBundle>,
        geo: &mut SceneGeometry,
    ) {
        let census_on = geo.census_on;
        let mesh_census = |v: &legaia_tmd::mesh::VramMesh| mesh_census_of(census_on, v);
        let SceneGeometry {
            meshes,
            census,
            tmd_data,
            color_meshes,
            lit_sources,
            posed_placement_meshes,
            posed_tmds,
            ..
        } = geo;
        if let Some(bundle) = scene_bundle {
            for (res_tmd, anim_id) in self.posed_placement_keys(res) {
                let rtmd = &res.tmds[res_tmd];
                let rec_idx = (anim_id - 1) as usize;
                let Ok(rec) = bundle.record(rec_idx) else {
                    log::warn!(
                        "play-window: placed object wants ANM record {rec_idx} \
                         (anim {anim_id}); the scene bundle has no such record"
                    );
                    continue;
                };
                let bones = rec.bone_count as usize;
                // Retail's count-equality contract (`FUN_8001b964` refuses
                // to draw when the mesh chain and the clip disagree).
                if bones != rtmd.tmd.objects.len() {
                    log::warn!(
                        "play-window: ANM record {rec_idx} has {bones} bones but env mesh \
                         (res {res_tmd}) has {} objects - not posing",
                        rtmd.tmd.objects.len()
                    );
                    continue;
                }
                let offsets: Vec<([i16; 3], [i16; 3])> = (0..bones)
                    .map(|b| match bundle.bone_transform(rec_idx, 0, b) {
                        Some(t) => (
                            [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                            [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                        ),
                        None => ([0; 3], [0; 3]),
                    })
                    .collect();
                let (mut vmesh, posed_lit) = legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot_lit(
                    &rtmd.tmd, &rtmd.raw, &offsets,
                );
                let mut cmesh =
                    legaia_tmd::mesh::tmd_to_color_mesh_posed_rot(&rtmd.tmd, &rtmd.raw, &offsets);
                // Same hybrid coplanar resolution as the static env
                // meshes above (and the web `build_hybrid_env_mesh_posed`).
                legaia_tmd::mesh::resolve_hybrid(&mut vmesh, &mut cmesh);
                legaia_render_kernels::scene_lighting::tag_emissive_meshes(
                    &rtmd.raw, &mut vmesh, &mut cmesh, &res.vram,
                );
                // Retain the raw TMD so the draw pass can re-pose this prop
                // at whatever frame its clip is on.
                let mut slot = PosedMesh {
                    tmd: Some(posed_tmds.len()),
                    ..Default::default()
                };
                posed_tmds.push((rtmd.tmd.clone(), rtmd.raw.clone()));
                if !vmesh.indices.is_empty() {
                    match r.upload_vram_mesh(
                        &vmesh.positions,
                        &vmesh.uvs,
                        &vmesh.cba_tsb,
                        &vmesh.normals,
                        &vmesh.colors,
                        &vmesh.indices,
                    ) {
                        Ok(m) => {
                            // Light-source rows: each prop's draw takes a
                            // copy shaded at its own rotation
                            // (`rebuild_field_lit_meshes`).
                            if posed_lit.len() == vmesh.colors.len()
                                && legaia_engine_field::field_lit_mesh::has_lit_rows(&posed_lit)
                            {
                                lit_sources.insert(meshes.len(), (vmesh.clone(), posed_lit));
                            }
                            slot.vram = Some(meshes.len());
                            census.resize(meshes.len(), None);
                            census.push(mesh_census(&vmesh));
                            meshes.push(m);
                            tmd_data.push((rtmd.tmd.clone(), rtmd.raw.clone()));
                        }
                        Err(e) => log::warn!("posed placement mesh upload skipped: {e:#}"),
                    }
                }
                if !cmesh.is_empty() {
                    match r.upload_color_mesh_blended(
                        &cmesh.positions,
                        &cmesh.colors,
                        &cmesh.indices,
                        &cmesh.blend,
                    ) {
                        Ok(m) => {
                            slot.color = Some(color_meshes.len());
                            color_meshes.push(m);
                        }
                        Err(e) => log::warn!("posed placement colour upload skipped: {e:#}"),
                    }
                }
                posed_placement_meshes.insert((res_tmd, anim_id), slot);
            }
            if !posed_placement_meshes.is_empty() {
                log::info!(
                    "play-window: {} posed static-object meshes (frame-0 rest pose)",
                    posed_placement_meshes.len()
                );
            }
        }
    }

    /// Bulk **ground** heightfield: the surface built from the `.MAP`
    /// floor grid (the pack meshes are only the placed objects /
    /// landmarks, not a per-cell ground mesh), textured per cell from
    /// the terrain-type-keyed multi-page atlas - the heightfield
    /// bakes the per-cell tile UV (`+0x14`) and page+palette
    /// (`+0x15`/`+0x16..+0x18`) so grass / mountain / water cells sample
    /// their own VRAM page. See docs/subsystems/world-map.md
    /// "Ground texturing".
    ///
    /// This is NOT world-map-only: town `.MAP`s carry the same
    /// `0x1000`-gated ground layer (town01: ~1.9k ground cells vs the
    /// 208 `0x2000` decor tiles, most with `record[+0x10] = 0` = no
    /// pack mesh), and the retail town VRAM has the terrain atlas
    /// pages resident. Without this surface the town floor renders
    /// as holes wherever no `0x2000` tile covers a cell.
    pub(super) fn upload_ground_heightfield(&self, r: &Renderer, geo: &mut SceneGeometry) {
        let census_on = geo.census_on;
        let mesh_census = |v: &legaia_tmd::mesh::VramMesh| mesh_census_of(census_on, v);
        let SceneGeometry {
            world_map_hf,
            ground_src,
            ground_census,
            ..
        } = geo;
        if let Some(scene) = self.session.host.scene.as_ref()
            && let Ok(Some(hf)) = scene.walk_heightfield(&self.session.host.index)
            && !hf.indices.is_empty()
        {
            let vmesh = heightfield_to_vram_mesh(&hf);
            // Each vertex also carries its cell's four corners, so the
            // overworld draws the cell at its ordering-table bucket's
            // depth (`overworld_draw_order`; the browser play page
            // uploads the same refs through `field_ground_flat_refs`).
            // A sloped far-bucket field cell carries the marker that draws
            // it under everything, as retail's ground pass does
            // (`field_ground::flat_refs`).
            let flat_refs = legaia_engine_scene::field_ground::flat_refs(&hf, &vmesh.positions);
            match r.upload_vram_mesh_with_flat_refs(
                &vmesh.positions,
                &vmesh.uvs,
                &vmesh.cba_tsb,
                &vmesh.normals,
                &vmesh.colors,
                &vmesh.indices,
                &flat_refs,
            ) {
                Ok(m) => {
                    log::info!(
                        "play-window: ground heightfield {} quads ({} verts)",
                        hf.quad_count(),
                        hf.positions.len()
                    );
                    *world_map_hf = Some(m);
                    if let Some(c) = mesh_census(&vmesh) {
                        *ground_census = Some(c);
                    }
                    *ground_src = Some(GroundSource {
                        vmesh,
                        flat_refs,
                        hf,
                        lut_applied: None,
                    });
                }
                Err(e) => log::warn!("heightfield upload skipped: {e:#}"),
            }
        }
    }
}
