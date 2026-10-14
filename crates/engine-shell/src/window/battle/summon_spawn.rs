//! Spawning a summon's creature actor from the battle side-band streams.

use super::*;

impl PlayWindowApp {
    /// Spawn the player Seru-magic summon as a battle creature, the faithful
    /// render (the summon reuses its namesake `battle_data` enemy creature -
    /// see `summon::summon_creature_id`). The world half - the seat, its
    /// texture slot, idle and clip set - is the session step both hosts run
    /// (`battle_stage::seat_summon_creature`), renderer or not; this window
    /// then loads the creature's texture into that slot (`battle_render_mesh`)
    /// and binds its mesh to the seat, so the existing battle render animates
    /// + textures it exactly like an enemy. No-op outside battle.
    pub(in crate::window) fn spawn_summon_creature(&mut self, spell_id: u8) {
        let Some(archive) = self.monster_archive_bytes() else {
            return;
        };
        let Some(seat) = legaia_engine_session::battle_stage::seat_summon_creature(
            &mut self.session.host,
            spell_id,
            &archive,
            self.summon_actor_slot,
        ) else {
            return;
        };
        self.summon_actor_slot = Some(seat.slot);
        let legaia_engine_session::battle_stage::SummonSeat {
            slot,
            tex_slot,
            asset,
        } = seat;
        let Some(mut vram) = self.battle_vram.clone() else {
            return;
        };
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };
        let mesh = &asset.mesh;
        let Ok(tmd) = legaia_tmd::parse(mesh.tmd_bytes()) else {
            return;
        };
        // Inject the creature texture into the seat's battle slot.
        let Some(vmesh) = mesh.battle_render_mesh(tex_slot, &mut vram) else {
            return;
        };
        if vmesh.indices.is_empty() {
            return;
        }
        let uploaded = match r.upload_vram_mesh(
            &vmesh.positions,
            &vmesh.uvs,
            &vmesh.cba_tsb,
            &vmesh.normals,
            &vmesh.colors,
            &vmesh.indices,
        ) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("play-window: summon mesh upload: {e:#}");
                return;
            }
        };
        let idx = self.meshes.len();
        self.meshes.push(uploaded);
        self.battle_rest_vmesh.insert(idx, vmesh);
        self.scene_tmd_data.push((tmd, mesh.tmd_bytes().to_vec()));
        match r.upload_vram(&vram) {
            Ok(v) => {
                // A mid-battle summon spawn is a legitimate battle-VRAM
                // refresh: re-stamp the expected resident generation.
                self.battle_vram_generation = Some(v.generation());
                self.uploaded_vram = Some(v);
            }
            Err(e) => log::error!("play-window: summon VRAM re-upload: {e:#}"),
        }
        // Keep the stashed battle VRAM in sync with what's now resident, so
        // later battle-VRAM refreshes (the residency heal, the per-frame
        // face stamps) don't drop the injected summon texture.
        self.battle_vram = Some(vram);

        // The seat's mesh handle: this window's index into its mesh list.
        if let Some(a) = self.session.host.world.actors.get_mut(slot) {
            a.tmd_binding = Some(idx);
        }
        log::info!(
            "play-window: summon spell {spell_id:#04x} -> creature {:?} \
             (mesh slot {idx}, tex slot {tex_slot}, actor slot {slot})",
            asset.creature_id
        );
    }

    // -----------------------------------------------------------------------
    // The field-to-battle intro transition
    // -----------------------------------------------------------------------
}
