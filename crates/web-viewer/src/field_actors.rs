//! The field **actor layer** both browser field surfaces draw: the scene's MAN
//! partition-1 placements (NPCs, chests, save crystals, story actors), posed
//! from the world's own clip cursors and placed at the world's live positions.
//!
//! One implementation, two owners. The play page's
//! [`crate::runtime::LegaiaRuntime`] holds one over its scene host and exports
//! it as the `play_npc_*` accessors; the map viewer's
//! [`crate::field_scene::FieldScenePack`] holds one over its headless
//! [`legaia_engine_core::scene_live::LiveScene`] and exports the same
//! accessors as `field_scene_npc_*`. Every answer is computed here from the
//! [`SceneHost`] passed in, so the two pages cannot pose, place or re-bind an
//! actor differently. `site/js/field-actors.js` is the one JS draw path both
//! pages run over those accessors.

use legaia_asset::player_anm::PlayerAnmBundle;
use legaia_engine_core::scene::SceneHost;
use std::collections::HashMap;

use crate::play::{NpcClip, NpcRender};

/// The pose banks an actor's clip ids index: the scene's own ANM bundle and
/// the PROT 0874 locomotion bundle (the party / global-pool specials).
#[derive(Clone, Copy)]
pub(crate) struct ActorBanks<'a> {
    pub scene_anm: Option<&'a PlayerAnmBundle>,
    pub locomotion_anm: Option<&'a PlayerAnmBundle>,
}

/// The scene's actor layer (see the module docs).
#[derive(Default)]
pub(crate) struct FieldActors {
    /// The placement catalog with its resolved meshes.
    pub npcs: Option<NpcRender>,
    /// One live clip player per catalogued slot that has a clip.
    pub clips: HashMap<u8, NpcClip>,
    /// Per-slot generation of the op-`0x4B` VDF morph, bumped whenever the
    /// world's staged deltas for the slot move.
    pub morph_gen: HashMap<u8, u32>,
    /// `(live model, mesh cut)` of the currently built mesh, so a re-bind or
    /// a re-cut rebuilds it.
    pub bound_model: Option<(i32, i32)>,
}

/// Decode ANM bundle record `rec_idx` into the flat pose stream the JS
/// animator consumes: `6` `i32` per bone per frame
/// (`[tx, ty, tz, rx, ry, rz]`, absolute), stride = the record's own bone
/// count. Empty when the record doesn't decode.
pub(crate) fn bundle_pose_frames(bundle: &PlayerAnmBundle, rec_idx: usize) -> Vec<i32> {
    let Ok(rec) = bundle.record(rec_idx) else {
        return Vec::new();
    };
    let bones = rec.bone_count as usize;
    let frames = rec.frame_count as usize;
    let mut out = Vec::with_capacity(frames * bones * 6);
    for f in 0..frames {
        for b in 0..bones {
            let Some(t) = bundle.bone_transform(rec_idx, f, b) else {
                return Vec::new();
            };
            out.extend_from_slice(&[t.t_x, t.t_y, t.t_z, t.r_x, t.r_y, t.r_z]);
        }
    }
    out
}

impl FieldActors {
    /// Drop the layer (a scene swap).
    pub fn clear(&mut self) {
        self.npcs = None;
        self.clips.clear();
        self.bound_model = None;
    }

    /// Build the scene's catalog against `host`'s resources and bind one clip
    /// player per placement that names a clip - the native window's
    /// `npc_clip_players` rebuild. The live clip id (actor `+0x5C` after the
    /// spawn prologue) wins over the header byte, as in the native window: a
    /// save crystal ships header anim 0 and its prologue sets the savepoint
    /// clip. The playhead is the world's (`World::bind_npc_clip_cursor`): the
    /// actor's `+0x62` word may hold or one-shot the clip.
    pub fn rebuild(&mut self, host: &mut SceneHost, banks: ActorBanks<'_>) {
        self.clear();
        let (Some(scene), Some(res)) = (host.scene.as_ref(), host.resources.as_ref()) else {
            return;
        };
        let name = scene.name.clone();
        match crate::field_npc::build_npc_catalog_play(
            &host.index,
            &name,
            res,
            &host.world.field_head_pool,
        ) {
            Ok(mut pack) => {
                for e in &mut pack.entries {
                    if let Some(a) = host.world.field_npc_live_anim(e.placement.index) {
                        e.placement.anim_id = a;
                    }
                }
                self.npcs = Some(NpcRender { pack });
            }
            Err(e) => crate::console_log(&format!("field actors for {name}: {e}")),
        }
        let Some(n) = self.npcs.as_ref() else {
            return;
        };
        for e in &n.pack.entries {
            let slot = e.placement.index as u8;
            let bundle = Self::clip_bank(host, banks, slot, e.special);
            let (Some(b), Some(rec)) = (bundle, (e.placement.anim_id as usize).checked_sub(1))
            else {
                continue;
            };
            if let Some(player) =
                legaia_engine_core::field_anim::FieldClipPlayer::from_record(b, rec)
            {
                host.world
                    .bind_npc_clip_cursor(slot, e.placement.anim_id, &player);
                self.clips.insert(
                    slot,
                    NpcClip {
                        player,
                        generation: 0,
                    },
                );
            }
        }
    }

    /// One sim tick of clip playback: drain this tick's `A2` / `4C 51` clip
    /// cues through the shared frame-tail kernel
    /// (`World::drain_field_anim_cues`), re-target the cued slots' players,
    /// note op-`0x4B` morph moves, then advance every player by one tick off
    /// the world's cursor (or free-run one the world does not drive). The
    /// render side reads clip state without moving the playhead, so clip
    /// cadence is the 60 Hz sim clock, not the display refresh.
    pub fn drive(&mut self, host: &mut SceneHost, banks: ActorBanks<'_>) {
        // Every catalogued placement is a clip target, not only the ones that
        // spawned with a clip: a placement whose live anim id was still `0`
        // at the rebuild takes its first clip from a cue.
        let spawn_party: HashMap<u8, bool> = self
            .npcs
            .as_ref()
            .map(|n| {
                n.pack
                    .entries
                    .iter()
                    .map(|e| (e.placement.index as u8, e.special))
                    .collect()
            })
            .unwrap_or_default();
        let retargets =
            host.world
                .drain_field_anim_cues(banks.scene_anm, banks.locomotion_anm, |slot| {
                    spawn_party.get(&slot).copied()
                });
        for r in retargets {
            match self.clips.get_mut(&r.slot) {
                Some(clip) => {
                    clip.player = r.player;
                    clip.generation = clip.generation.wrapping_add(1);
                }
                None => {
                    self.clips.insert(
                        r.slot,
                        NpcClip {
                            player: r.player,
                            generation: 1,
                        },
                    );
                }
            }
        }
        for slot in host.world.take_npc_morph_dirty() {
            let g = self.morph_gen.entry(slot).or_insert(0);
            *g = g.wrapping_add(1);
        }
        // The playheads run only while the field owns the frame - the
        // world's decision, shared with the native draw pass.
        if host.world.field_npc_clips_advance() {
            for (slot, clip) in self.clips.iter_mut() {
                if !host.world.sync_npc_clip(*slot, &mut clip.player) {
                    clip.player.advance(1);
                }
            }
        }
    }

    /// The bank NPC `slot`'s clip ids name: the locomotion bundle when the
    /// slot's live party-bank bit is up, else the scene's own
    /// (`World::npc_clip_party_bank`; `special`, the spawn class, only for a
    /// slot no channel carries) - the native window's bundle split.
    pub fn clip_bank<'a>(
        host: &SceneHost,
        banks: ActorBanks<'a>,
        slot: u8,
        special: bool,
    ) -> Option<&'a PlayerAnmBundle> {
        if host.world.npc_clip_party_bank(slot, special) {
            banks.locomotion_anm
        } else {
            banks.scene_anm
        }
    }

    fn clip_bone_count(
        host: &SceneHost,
        banks: ActorBanks<'_>,
        slot: u8,
        anim_id: u8,
        special: bool,
    ) -> Option<usize> {
        let bundle = Self::clip_bank(host, banks, slot, special)?;
        let rec = bundle.record((anim_id as usize).checked_sub(1)?).ok()?;
        (rec.bone_count > 0).then_some(rec.bone_count as usize)
    }

    fn entry(&self, i: u32) -> Option<&legaia_engine_core::npc_catalog::NpcEntry> {
        self.npcs.as_ref()?.pack.entries.get(i as usize)
    }

    /// The scene's actor catalog as JSON:
    /// `{"anm_prot", "npcs": [{"i", "slot", "model", "anim", "nobj", "kind",
    /// "target_map", "dialog", "conditional", "special", "x", "z"}, ...]}`;
    /// `null` before a scene is entered.
    pub fn catalog_json(&self) -> String {
        let Some(n) = self.npcs.as_ref() else {
            return "null".to_string();
        };
        let npcs: Vec<serde_json::Value> = n
            .pack
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                serde_json::json!({
                    "i": i,
                    "slot": e.placement.index,
                    "model": e.placement.model_index,
                    "anim": e.placement.anim_id,
                    "nobj": e.nobj,
                    "kind": e.kind,
                    "target_map": e.target_map,
                    "dialog": e.dialog,
                    "conditional": e.conditional,
                    "special": e.special,
                    "x": e.placement.world_x,
                    "z": e.placement.world_z,
                })
            })
            .collect();
        serde_json::json!({
            "anm_prot": n.pack.anm_prot,
            "npcs": npcs,
        })
        .to_string()
    }

    /// The live model id the scripted-motion VM's op `0x0E` re-bound catalog
    /// entry `i`'s actor to, or `-1` while it still draws its spawn mesh
    /// (`World::field_npc_live_model`).
    pub fn live_model(&self, host: &SceneHost, i: u32) -> i32 {
        let Some(e) = self.entry(i) else {
            return -1;
        };
        host.world
            .field_npc_live_model(e.placement.index as u8)
            .map_or(-1, i32::from)
    }

    /// The object count catalog entry `e`'s mesh is cut to: the bone count of
    /// the slot's live clip when it has one (including a clip its first
    /// ANIMATE cue bound after the spawn), else of the placement's spawn clip.
    /// `None` = no clip, keep the whole object table.
    fn mesh_cut_of(
        &self,
        host: &SceneHost,
        banks: ActorBanks<'_>,
        e: &legaia_engine_core::npc_catalog::NpcEntry,
    ) -> Option<usize> {
        let slot = e.placement.index as u8;
        match self.clips.get(&slot) {
            Some(c) => Some(c.player.bone_count()).filter(|&b| b > 0),
            None => Self::clip_bone_count(host, banks, slot, e.placement.anim_id, e.special),
        }
    }

    /// [`Self::mesh_cut_of`] for catalog entry `i`, `-1` for an uncut mesh.
    pub fn mesh_cut(&self, host: &SceneHost, banks: ActorBanks<'_>, i: u32) -> i32 {
        self.entry(i)
            .and_then(|e| self.mesh_cut_of(host, banks, e))
            .map_or(-1, |b| b as i32)
    }

    /// Catalog entry `i`'s mesh source: the TMD (object table cut to the
    /// clip's bone count) and its raw bytes. A scripted mesh re-bind replaces
    /// the placement's own model; a special (`model >= 0xF0`) resolves out of
    /// the world's global pool.
    fn entry_tmd(
        &self,
        host: &SceneHost,
        banks: ActorBanks<'_>,
        i: u32,
    ) -> Result<(legaia_tmd::Tmd, Vec<u8>), String> {
        let live = self.live_model(host, i);
        let live_src = (live >= 0)
            .then(|| {
                let scene = host.scene.as_ref()?;
                let raw = host.model_bank.tmd_bytes(scene, live as i16)?;
                let tmd = legaia_tmd::parse(&raw).ok()?;
                Some((tmd, raw))
            })
            .flatten();
        let e = self
            .entry(i)
            .ok_or_else(|| format!("actor mesh: no entry {i}"))?;
        let (mut tmd, raw) = if let Some(m) = live_src {
            m
        } else if e.special {
            let slot = (e.placement.model_index - 0xF0) as usize;
            let g = host
                .world
                .field_head_pool
                .get(slot)
                .and_then(|s| s.as_ref())
                .ok_or("actor mesh: no global-pool mesh")?;
            (g.tmd.clone(), g.raw.clone())
        } else {
            let res = host.resources.as_ref().ok_or("actor mesh: no resources")?;
            let t = res
                .tmds
                .get(e.placement.model_index as usize)
                .ok_or("actor mesh: model out of range")?;
            (t.tmd.clone(), t.raw.clone())
        };
        if let Some(bones) = self.mesh_cut_of(host, banks, e) {
            tmd.objects.truncate(bones);
        }
        Ok((tmd, raw))
    }

    /// Enhanced lighting's light sets of the catalog's actors, each clustered
    /// at its spawn anchor (the floor under its placement) from its rest
    /// mesh - the native window's MAN-prop light pass. Painted lamplight is
    /// not read here (on a character it is skin), so a set comes only from
    /// glow prims and the curated emissive table.
    pub fn prop_light_sets(
        &self,
        host: &SceneHost,
        banks: ActorBanks<'_>,
    ) -> Vec<legaia_engine_ui::scene_lighting::PropLights> {
        use legaia_engine_ui::scene_lighting as sl;
        let (Some(n), Some(res)) = (self.npcs.as_ref(), host.resources.as_ref()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (i, e) in n.pack.entries.iter().enumerate() {
            let Ok((tmd, raw)) = self.entry_tmd(host, banks, i as u32) else {
                continue;
            };
            let mut vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &raw);
            let mut cmesh = legaia_tmd::mesh::tmd_to_color_mesh(&tmd, &raw);
            let hit = sl::tag_emissive_meshes(&raw, &mut vmesh, &mut cmesh, &res.vram);
            let mut samples = sl::vram_mesh_emitters(
                &vmesh.positions,
                &vmesh.cba_tsb,
                &vmesh.colors,
                &vmesh.indices,
                Some((&res.vram, &vmesh.uvs)),
            );
            samples.extend(sl::color_mesh_emitters(
                &cmesh.positions,
                &cmesh.colors,
                &cmesh.blend,
                &cmesh.indices,
            ));
            if let Some(h) = hit {
                samples.push(sl::curated_mesh_sample(&h));
            }
            if samples.is_empty() {
                continue;
            }
            let (x, z) = (e.placement.world_x, e.placement.world_z);
            let y = host
                .world
                .sample_field_floor_height(i32::from(x), i32::from(z));
            let spawn = [f32::from(x), y as f32, f32::from(z)];
            let model = glam::Mat4::from_translation(glam::Vec3::from(spawn));
            let lights = sl::cluster_all_scene_lights(&sl::transform_samples(&samples, &model));
            if !lights.is_empty() {
                out.push(sl::PropLights {
                    slot: e.placement.index as u8,
                    spawn,
                    lights,
                });
            }
        }
        out
    }

    /// Per catalog entry, its op-`0x4B` morph generation (`-1` = never armed).
    pub fn morph_states(&self) -> Vec<i32> {
        let Some(n) = self.npcs.as_ref() else {
            return Vec::new();
        };
        n.pack
            .entries
            .iter()
            .map(|e| {
                self.morph_gen
                    .get(&(e.placement.index as u8))
                    .map_or(-1, |&g| g as i32)
            })
            .collect()
    }

    /// Catalog entry `i`'s object-local base positions with its live morph
    /// staged (`World::npc_morphed_tmd`), in the mesh's vertex order.
    pub fn morph_base(&self, host: &SceneHost, banks: ActorBanks<'_>, i: u32) -> Vec<f32> {
        let Ok((tmd, raw)) = self.entry_tmd(host, banks, i) else {
            return Vec::new();
        };
        let Some(slot) = self.entry(i).map(|e| e.placement.index as u8) else {
            return Vec::new();
        };
        let morphed = host.world.npc_morphed_tmd(slot, &tmd);
        let tmd = morphed.as_ref().unwrap_or(&tmd);
        let (mesh, _, _) = legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(tmd, &raw);
        mesh.positions.iter().flatten().copied().collect()
    }

    /// Build catalog entry `i`'s mesh (hybrid: textured + vertex-colour
    /// prims, with per-vertex bone ids); the `mesh_*` readers then serve it.
    /// Cached on `(entry, live model, cut)`, since a re-bind or re-cut leaves
    /// the entry index where it was.
    pub fn build_mesh(
        &mut self,
        host: &SceneHost,
        banks: ActorBanks<'_>,
        i: u32,
    ) -> Result<(), String> {
        let idx = i as usize;
        let key = (self.live_model(host, i), self.mesh_cut(host, banks, i));
        if let Some(n) = self.npcs.as_ref()
            && n.pack.cur.as_ref().map(|c| c.0) == Some(idx)
            && self.bound_model == Some(key)
        {
            return Ok(());
        }
        let (tmd, raw) = self.entry_tmd(host, banks, i)?;
        let (mut mesh, object_ids, shading) =
            legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd, &raw);
        let flat = crate::packet_color::hybrid(&mesh, &shading);
        // Enhanced lighting's emissive tags - the same rule the native
        // window's actor build runs (inert unless the enhancement is on).
        if let Some(res) = host.resources.as_ref() {
            legaia_engine_ui::scene_lighting::tag_emissive_hybrid(
                &raw, &mut mesh, &flat, &res.vram,
            );
        }
        if let Some(n) = self.npcs.as_mut() {
            n.pack.cur = Some((idx, mesh, object_ids, flat));
        }
        self.bound_model = Some(key);
        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn cur(&self) -> Option<&(usize, legaia_tmd::mesh::VramMesh, Vec<u32>, Vec<u8>)> {
        self.npcs.as_ref()?.pack.cur.as_ref()
    }

    pub fn mesh_positions(&self) -> Vec<f32> {
        self.cur()
            .map(|(_, m, _, _)| m.positions.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    pub fn mesh_uvs(&self) -> Vec<u8> {
        self.cur()
            .map(|(_, m, _, _)| m.uvs.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    pub fn mesh_cba_tsb(&self) -> Vec<u16> {
        self.cur()
            .map(|(_, m, _, _)| m.cba_tsb.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    pub fn mesh_indices(&self) -> Vec<u32> {
        self.cur()
            .map(|(_, m, _, _)| m.indices.clone())
            .unwrap_or_default()
    }

    pub fn mesh_object_ids(&self) -> Vec<u32> {
        self.cur().map(|(_, _, o, _)| o.clone()).unwrap_or_default()
    }

    pub fn mesh_flat_rgba(&self) -> Vec<u8> {
        self.cur().map(|(_, _, _, f)| f.clone()).unwrap_or_default()
    }

    /// Catalog entry `i`'s spawn clip decoded to the JS animator's pose
    /// stream ([`bundle_pose_frames`]); empty when it has none.
    pub fn pose_frames(&self, host: &SceneHost, banks: ActorBanks<'_>, i: u32) -> Vec<i32> {
        let Some(e) = self.entry(i) else {
            return Vec::new();
        };
        let bundle = Self::clip_bank(host, banks, e.placement.index as u8, e.special);
        let (Some(b), Some(rec)) = (bundle, (e.placement.anim_id as usize).checked_sub(1)) else {
            return Vec::new();
        };
        bundle_pose_frames(b, rec)
    }

    /// `[frame_count, bone_count]` of catalog entry `i`'s spawn clip; `[0, 0]`
    /// when it has none.
    pub fn pose_dims(&self, host: &SceneHost, banks: ActorBanks<'_>, i: u32) -> Vec<u32> {
        let Some(e) = self.entry(i) else {
            return vec![0, 0];
        };
        let bundle = Self::clip_bank(host, banks, e.placement.index as u8, e.special);
        let (Some(b), Some(rec)) = (bundle, (e.placement.anim_id as usize).checked_sub(1)) else {
            return vec![0, 0];
        };
        match b.record(rec) {
            Ok(r) if r.bone_count > 0 => vec![r.frame_count as u32, r.bone_count as u32],
            _ => vec![0, 0],
        }
    }

    /// `[pose, generation, ...]` per catalog entry (`[-1, -1]` with no live
    /// clip): the pose key this render should show and the re-target count.
    pub fn clip_states(&self) -> Vec<i32> {
        let Some(n) = self.npcs.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(n.pack.entries.len() * 2);
        for e in &n.pack.entries {
            match self.clips.get(&(e.placement.index as u8)) {
                Some(c) => {
                    out.push(c.player.pose_key() as i32);
                    out.push(c.generation as i32);
                }
                None => out.extend([-1, -1]),
            }
        }
        out
    }

    /// Current pose of catalog entry `i`'s live clip, 6 `i32` per bone, read
    /// without advancing the playhead. Empty with no live clip.
    pub fn live_bones(&self, i: u32) -> Vec<i32> {
        let Some(e) = self.entry(i) else {
            return Vec::new();
        };
        let Some(c) = self.clips.get(&(e.placement.index as u8)) else {
            return Vec::new();
        };
        let pose = c.player.current_pose();
        let mut out = Vec::with_capacity(pose.bone_outputs.len() * 6);
        for (t, r) in pose.bone_outputs {
            out.extend([
                t[0] as i32,
                t[1] as i32,
                t[2] as i32,
                r[0] as i32,
                r[1] as i32,
                r[2] as i32,
            ]);
        }
        out
    }

    /// `[x, y, z, facing, ...]` per catalog entry from the world: live
    /// positions (the placement anchor for one that never moved), the
    /// render-scale-zero hide (surfaced as the off-map hide box), the seeded
    /// heading (identity = `2048` in the page's convention) and the floor /
    /// scripted-arc height.
    pub fn transforms(&self, host: &SceneHost) -> Vec<f32> {
        let Some(n) = self.npcs.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(n.pack.entries.len() * 4);
        let hide = legaia_engine_core::world::FIELD_OFFMAP_HIDE_XZ;
        for e in &n.pack.entries {
            let slot = e.placement.index as u8;
            let (mut x, mut z) = host
                .world
                .npcs
                .positions
                .get(&slot)
                .copied()
                .unwrap_or((e.placement.world_x, e.placement.world_z));
            if host.world.field_npc_render_scale(e.placement.index) == Some(0) {
                (x, z) = (hide, hide);
            }
            let facing = host.world.npcs.headings.get(&slot).copied().unwrap_or(2048) as f32;
            let y = host.world.field_npc_render_y(slot, x, z) as f32;
            out.extend_from_slice(&[x as f32, y, z as f32, facing]);
        }
        out
    }

    /// `[r, g, b, ir0, ...]` per catalog entry: the actor's op-`4C 81` draw
    /// tint as a constant cue (`World::field_npc_draw_tint`), zeros for an
    /// untinted actor. Empty while no entry is tinted.
    pub fn tints(&self, host: &SceneHost) -> Vec<f32> {
        let Some(n) = self.npcs.as_ref() else {
            return Vec::new();
        };
        let mut any = false;
        let mut out = Vec::with_capacity(n.pack.entries.len() * 4);
        for e in &n.pack.entries {
            match host.world.field_npc_draw_tint(e.placement.index) {
                Some((colour, blend)) => {
                    let (far, ir0) = legaia_engine_core::world::tint_cue(colour, blend);
                    out.extend_from_slice(&[far[0], far[1], far[2], ir0]);
                    any = true;
                }
                None => out.extend_from_slice(&[0.0; 4]),
            }
        }
        if any { out } else { Vec::new() }
    }

    /// `[pitch, roll, ...]` per catalog entry (`World::field_npc_tilt`).
    pub fn tilts(&self, host: &SceneHost) -> Vec<f32> {
        let Some(n) = self.npcs.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(n.pack.entries.len() * 2);
        for e in &n.pack.entries {
            let (pitch, roll) = host
                .world
                .field_npc_tilt(e.placement.index as u8)
                .unwrap_or((0, 0));
            out.extend_from_slice(&[pitch as f32, roll as f32]);
        }
        out
    }
}
