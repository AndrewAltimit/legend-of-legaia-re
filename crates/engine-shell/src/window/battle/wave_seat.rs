//! Binding the battle meshes of monsters an expansion wave seats mid-battle.

use super::*;

impl PlayWindowApp {
    /// Bind the mesh of every monster an expansion pack's wave seated since
    /// the last frame. The world half - each seat's texture slot and idle -
    /// is the session step both hosts run
    /// (`battle_stage::stage_wave_monsters`); this window then injects each
    /// monster's texture pool into its slot of the battle VRAM and binds the
    /// uploaded mesh to its seat, the per-monster step
    /// [`Self::enter_battle_render`] runs for the opening monsters. Nothing
    /// arrives without a pack.
    pub(in crate::window) fn bind_wave_monsters(&mut self) {
        let archive = self.monster_archive_bytes();
        let seats = legaia_engine_session::battle_stage::stage_wave_monsters(
            &mut self.session.host,
            archive.as_deref().map(Vec::as_slice),
        );
        if seats.is_empty() {
            return;
        }
        let (Some(archive), Some(mut vram)) = (archive, self.battle_vram.clone()) else {
            return;
        };
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };
        let mut bound = 0usize;
        for seat in seats {
            let Ok(Some(mesh)) =
                legaia_actor_formats::monster_archive::mesh(&archive, seat.monster_id)
            else {
                continue;
            };
            let Ok(tmd) = legaia_tmd::parse(mesh.tmd_bytes()) else {
                continue;
            };
            let Some(vmesh) = mesh.battle_render_mesh(seat.tex_slot, &mut vram) else {
                continue;
            };
            legaia_actor_formats::monster_archive::write_boss_effect_page(
                &archive,
                seat.monster_id,
                &mut vram,
            );
            if vmesh.indices.is_empty() {
                continue;
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
                    let idx = self.meshes.len();
                    self.meshes.push(m);
                    self.battle_rest_vmesh.insert(idx, vmesh);
                    self.scene_tmd_data.push((tmd, mesh.tmd_bytes().to_vec()));
                    if let Some(a) = self.session.host.world.actors.get_mut(seat.actor) {
                        a.tmd_binding = Some(idx);
                    }
                    bound += 1;
                }
                Err(e) => log::warn!(
                    "play-window: wave monster {} mesh upload: {e:#}",
                    seat.monster_id
                ),
            }
        }
        if bound == 0 {
            return;
        }
        match r.upload_vram(&vram) {
            Ok(v) => {
                // A legitimate battle-VRAM refresh, like a summon's.
                self.battle_vram_generation = Some(v.generation());
                self.uploaded_vram = Some(v);
            }
            Err(e) => log::error!("play-window: wave VRAM re-upload: {e:#}"),
        }
        self.battle_vram = Some(vram);
        log::info!("play-window: bound {bound} wave monster mesh(es)");
    }
}
