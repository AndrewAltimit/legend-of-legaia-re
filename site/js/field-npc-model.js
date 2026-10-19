/* Put one catalogued field NPC into a MeshView, posed by its own clip.
 *
 * Shared by the NPCs page and the shops page (the vendor window). The catalog
 * is `viewer.field_npc_catalog_json()` after `viewer.set_scene_npcs(label)`;
 * `n` is one of its `npcs`. The mesh is the field-hybrid build (textured prims
 * sampling the scene VRAM plus the untextured flat / gouraud prims), and a
 * multi-part model is posed by frame 0.. of its clip - drawn raw, its
 * object-local parts would collapse onto the origin (see js/mesh-view.js).
 *
 *   const r = FieldNpcModel.show(meshView, viewer, catalog, n);
 *   // r = { ok, why, posed, frameCount, boneCount }
 *
 * Requires js/mesh-view.js.
 */
(function () {
  'use strict';

  function show(meshView, viewer, catalog, n) {
    const out = { ok: false, why: null, posed: false, frameCount: 0, boneCount: 0 };
    try {
      viewer.field_npc_mesh(n.i);
    } catch (err) {
      out.why = `couldn't build that model: ${err.message || err}`;
      return out;
    }
    const positions = viewer.field_npc_mesh_positions();
    const indices = viewer.field_npc_mesh_indices();
    if (!positions.length || !indices.length) {
      out.why = 'this actor\'s model has no drawable primitives';
      return out;
    }
    /* Every NPC in a scene samples the same field VRAM the map does. */
    meshView.uploadVram(viewer.field_scene_vram_bytes());
    const flat = viewer.field_npc_mesh_flat_rgba();
    meshView.setMesh(
      positions,
      viewer.field_npc_mesh_uvs(),
      viewer.field_npc_mesh_cba_tsb(),
      indices,
      viewer.field_npc_mesh_bounds(),
      viewer.field_npc_mesh_object_ids(),
      flat.length ? flat : null,
    );
    out.ok = true;
    /* The catalog only lists actors it can pose, so a multi-part model always
     * resolves a clip here. Single-part models need none. */
    if (n.anim > 0 && catalog.anm_prot != null) {
      const rec = n.anim - 1;
      const dims = viewer.player_anm_record_dims(catalog.anm_prot, rec);
      const boneCount = dims[0] | 0;
      const frameCount = dims[1] | 0;
      if (frameCount > 0) {
        /* Pad the clip out to the TMD's object count: objects the clip doesn't
         * animate get an identity transform and stay at their model-local
         * origin, rather than being collapsed. */
        const partCount = Math.max(n.nobj, boneCount);
        const frames = viewer.player_anm_record_pose_frames(catalog.anm_prot, rec, n.nobj);
        if (frames.length >= frameCount * partCount * 6) {
          meshView.setAnimation({ partCount, frameCount, frames });
          out.posed = true;
          out.frameCount = frameCount;
          out.boneCount = boneCount;
        }
      }
    }
    if (!out.posed) meshView.setAnimation(null);
    return out;
  }

  window.FieldNpcModel = { show };
})();
