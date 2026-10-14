//! Script-spawned actors' meshes: the dynamic slot drain, the staged mesh
//! buffers and the live actor transforms.

use super::*;

/// Script-spawned actors (field-VM `0x4C 0xD8`, `FieldEvent::ActorSpawned`)
/// - the browser twin of the native window's `pending_dynamic_mesh_slots`
/// drain in its redraw pass. An actor a scene script spawns with a TMD
/// reference is not in the MAN placement catalog the NPC layer draws from,
/// so the page used to give it no geometry at all. The page drains the
/// slots once per frame, uploads each slot's mesh at its rest pose, and
/// draws them from the live actor transforms.
#[wasm_bindgen]
impl LegaiaRuntime {
    /// Drain the actor slots that gained a TMD reference since the last
    /// call. Upload each through [`Self::play_dynamic_actor_mesh`].
    pub fn play_take_dynamic_mesh_slots(&mut self) -> Vec<u32> {
        let pending = std::mem::take(&mut self.pending_dynamic_mesh_slots);
        pending
            .into_iter()
            .filter(|s| !self.dynamic_mesh_slots.contains(s))
            .map(u32::from)
            .collect()
    }

    /// Actor slots carrying a live morph-weight blend (the same `0x4C 0xD8`
    /// allocator, seated by `World::spawn_morph_weight_actor`). The page
    /// re-stages each one through [`Self::play_dynamic_actor_mesh`] every
    /// frame, exactly as the native window re-uploads it in its redraw pass:
    /// retail re-blends inside the handler call, and the envelope is a
    /// ping-pong ramp that moves on every frame, so no upload stays current.
    pub fn play_morph_weight_slots(&self) -> Vec<u32> {
        self.scene_host
            .host()
            .map(|h| {
                h.world
                    .morph_weight_actor_weights()
                    .into_iter()
                    .map(|(slot, _)| u32::from(slot))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Stage actor `slot`'s spawned mesh (its `tmd_ref` from the global
    /// pool) for the `play_dynamic_mesh_*` reads. `false` when the slot
    /// carries no drawable mesh.
    ///
    /// A morph-weight actor stages its **blended** mesh instead - through
    /// `World::morph_weight_posed_tmd`, the one engine-side kernel the
    /// native window poses from too, so the blend itself lives on neither
    /// host.
    pub fn play_dynamic_actor_mesh(&mut self, slot: u32) -> bool {
        let Ok(slot) = u8::try_from(slot) else {
            return false;
        };
        let posed = self
            .scene_host
            .host()
            .and_then(|h| h.world.morph_weight_posed_tmd(slot as usize));
        let Some(gtmd) = self
            .scene_host
            .host()
            .and_then(|h| h.world.actors.get(slot as usize))
            .and_then(|a| a.tmd_ref.as_ref().map(std::sync::Arc::clone))
        else {
            return false;
        };
        let (mesh, object_ids, shading) = match posed.as_ref() {
            Some((tmd, raw, _)) => legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(tmd, raw),
            None => legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&gtmd.tmd, &gtmd.raw),
        };
        if mesh.indices.is_empty() {
            return false;
        }
        let flat = crate::packet_color::hybrid(&mesh, &shading);
        self.dynamic_mesh_cur = Some(StagedActorMesh {
            mesh,
            object_ids,
            flat,
        });
        if !self.dynamic_mesh_slots.contains(&slot) {
            self.dynamic_mesh_slots.push(slot);
        }
        true
    }

    pub fn play_dynamic_mesh_positions(&self) -> Vec<f32> {
        self.dynamic_mesh_cur
            .as_ref()
            .map(|m| m.mesh.positions.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    pub fn play_dynamic_mesh_uvs(&self) -> Vec<u8> {
        self.dynamic_mesh_cur
            .as_ref()
            .map(|m| m.mesh.uvs.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    pub fn play_dynamic_mesh_cba_tsb(&self) -> Vec<u16> {
        self.dynamic_mesh_cur
            .as_ref()
            .map(|m| m.mesh.cba_tsb.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    pub fn play_dynamic_mesh_indices(&self) -> Vec<u32> {
        self.dynamic_mesh_cur
            .as_ref()
            .map(|m| m.mesh.indices.clone())
            .unwrap_or_default()
    }

    pub fn play_dynamic_mesh_flat_rgba(&self) -> Vec<u8> {
        self.dynamic_mesh_cur
            .as_ref()
            .map(|m| m.flat.clone())
            .unwrap_or_default()
    }

    /// Per-vertex TMD object index of the staged dynamic mesh.
    pub fn play_dynamic_mesh_object_ids(&self) -> Vec<u32> {
        self.dynamic_mesh_cur
            .as_ref()
            .map(|m| m.object_ids.clone())
            .unwrap_or_default()
    }

    /// Live transforms of every uploaded dynamic actor, `6 x f32` per entry:
    /// `[slot, x, y, z, facing_12bit, active]` in retail world units (the
    /// page negates Y like every other actor draw). `active` is `0` for a
    /// despawned / hidden slot - skip the draw. Heading follows the NPC
    /// heading map when the slot has one, else identity (`facing = 2048`,
    /// the native `None => Mat4::IDENTITY` arm).
    pub fn play_dynamic_actor_transforms(&self) -> Vec<f32> {
        let Some(h) = self.scene_host.host() else {
            return Vec::new();
        };
        let hide = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;
        let mut out = Vec::with_capacity(self.dynamic_mesh_slots.len() * 6);
        for &slot in &self.dynamic_mesh_slots {
            let Some(a) = h.world.actors.get(slot as usize) else {
                continue;
            };
            let (x, y, z) = (
                a.move_state.world_x,
                a.move_state.world_y,
                a.move_state.world_z,
            );
            let active = a.active && a.tmd_ref.is_some() && !(x == hide && z == hide);
            let facing = h.world.npcs.heading(slot) as f32;
            out.extend_from_slice(&[
                f32::from(slot),
                x as f32,
                y as f32,
                z as f32,
                facing,
                if active { 1.0 } else { 0.0 },
            ]);
        }
        out
    }
}
