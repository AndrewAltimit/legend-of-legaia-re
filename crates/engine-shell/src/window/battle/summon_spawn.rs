//! Spawning a summon's creature actor from the battle side-band streams.

use super::*;

impl PlayWindowApp {
    /// Spawn the player Seru-magic summon as a battle creature, the faithful
    /// render (the summon reuses its namesake `battle_data` enemy creature -
    /// see `summon::summon_creature_id`). Loads that creature's mesh + texture
    /// (`battle_render_mesh`) into a free battle texture slot and binds it to a
    /// high actor slot with its idle [`MonsterAnimPlayer`], so the existing
    /// battle render animates + textures it exactly like an enemy. Replaces the
    /// move-VM `SummonScene` stand-in for the visual. No-op outside battle.
    pub(in crate::window) fn spawn_summon_creature(&mut self, spell_id: u8) {
        if self.session.host.world.mode != SceneMode::Battle {
            return;
        }
        let Some(archive) = self.monster_archive_bytes() else {
            return;
        };
        // The cast's own body: the archive twin for 0x81..=0x95, the cast's
        // `summon.dat` record (PROT 893) for the high block.
        let summon_dat = self
            .session
            .host
            .index
            .entry_bytes(u32::from(
                legaia_actor_formats::summon_readef::SUMMON_PROT_INDEX,
            ))
            .ok();
        let Some(asset) = legaia_engine_effects::summon::summon_spawn_asset(
            spell_id,
            &archive,
            summon_dat.as_ref().map(|b| b.as_slice()),
        ) else {
            return;
        };
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
        // Inject the creature texture into the next free battle slot.
        let tex_slot = self.battle_tex_slots_used.min(4);
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

        // Seat the summon in a free high actor slot (>= 8) so it never collides
        // with the party/monster battle slots. Place it on the party side
        // (`enter_battle` seats party at negative Z, enemies at positive Z),
        // in front of the party and clearly clear of the enemy cluster, so
        // the battle camera frames it distinct from the enemies it attacks.
        let slot = self
            .summon_actor_slot
            .unwrap_or_else(|| 8 + (self.session.host.world.party.party_count as usize));
        self.summon_actor_slot = Some(slot);
        if let Some(a) = self.session.host.world.actors.get_mut(slot) {
            a.active = true;
            a.tmd_binding = Some(idx);
            a.battle_tex_slot = Some(tex_slot);
            a.move_state.world_x = 0;
            a.move_state.world_y = 0;
            a.move_state.world_z = -350;
        }
        if let Some(idle) = asset.idle.as_ref()
            && let Some(player) = legaia_engine_battle::battle_anim::MonsterAnimPlayer::new(idle)
        {
            self.session
                .host
                .world
                .set_actor_battle_animation(slot, player);
        }
        // The creature's archive-order clip set, so the stager's staged ids
        // (the walk, clip 1) resolve through the same commit as a monster's.
        if !asset.clips.is_empty() {
            let clips: Vec<_> = asset.clips.iter().cloned().map(Some).collect();
            self.session
                .host
                .world
                .set_actor_battle_action_clips(slot, std::sync::Arc::new(clips));
        }
        // Hand the seat to the world: a cast in its summon band places the
        // creature at the stager's spawn point (behind the caster, facing
        // the enemy - the capture's slot-7 record) and retires it when the
        // choreography ends; a debug spawn keeps the placement above.
        self.session.host.world.seat_summon_actor(slot);
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
