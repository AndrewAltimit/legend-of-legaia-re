//! The battle actors' exports: per-actor mesh buffers, transforms, poses and
//! the after-image ghosts.

use super::*;

/// A battle facing `+0x46` as a Y rotation in radians (glam's
/// `from_rotation_y` sense: `+Z` turns onto `(sin f, cos f)`, the direction
/// the root-motion drive moves the actor).
fn battle_facing_yaw(facing: u16) -> f32 {
    f32::from(facing & 0xFFF) / 4096.0 * std::f32::consts::TAU
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Number of bound battle actor meshes (monsters + party).
    pub fn play_battle_actor_count(&self) -> u32 {
        self.battle_render
            .as_ref()
            .map(|b| b.actors.len() as u32)
            .unwrap_or(0)
    }

    pub fn play_battle_actor_positions(&self, i: u32) -> Vec<f32> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.actors.get(i as usize))
            .map(|a| a.mesh.positions())
            .unwrap_or_default()
    }

    pub fn play_battle_actor_uvs(&self, i: u32) -> Vec<u8> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.actors.get(i as usize))
            .map(|a| a.mesh.uvs())
            .unwrap_or_default()
    }

    pub fn play_battle_actor_cba_tsb(&self, i: u32) -> Vec<u16> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.actors.get(i as usize))
            .map(|a| a.mesh.cba_tsb())
            .unwrap_or_default()
    }

    pub fn play_battle_actor_indices(&self, i: u32) -> Vec<u32> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.actors.get(i as usize))
            .map(|a| a.mesh.mesh.indices.clone())
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, 255]` packet colours of one bound actor mesh.
    /// Without this the combatants drew at the raw texel while the backdrop
    /// carried its modulation, so the party read brighter than the arena.
    pub fn play_battle_actor_flat_rgba(&self, i: u32) -> Vec<u8> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.actors.get(i as usize))
            .map(|a| a.mesh.flat.clone())
            .unwrap_or_default()
    }

    /// Per-vertex TMD object ids (the pose rig); empty = the upload is
    /// statically posed and must not be re-posed.
    pub fn play_battle_actor_object_ids(&self, i: u32) -> Vec<u32> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.actors.get(i as usize))
            .map(|a| a.object_ids.clone())
            .unwrap_or_default()
    }

    /// Live world transforms of every battle actor mesh, flattened
    /// `[x, y, z, yaw, active]` per actor in mesh order. Positions
    /// are RAW battle world units - the page multiplies by
    /// [`Self::play_battle_world_scale`] (retail composes the same 4x on
    /// the actor camera). `yaw` is the live battle facing `+0x46` in
    /// radians, `f / 4096 * TAU` about Y - the angle that turns a mesh's
    /// rest `+Z` onto the actor's travel direction `(sin f, cos f)`. Every
    /// mesh rests facing `+Z`, so a seated monster's `0x800` is the
    /// half-turn toward the party and a fleeing party's `0x800` turns its
    /// back on the fight.
    ///
    /// `active` is the draw gate: it also carries the summon band's hide
    /// (`+0x21C = 0xFF` over a zero colour word,
    /// `World::battle_body_hidden`): every party seat and living monster is
    /// off screen while the creature performs; and retail's
    /// per-body battle draw verdict ([`Self::battle_draw_plan`]: the
    /// `FUN_800480D8` colour-word / grey gate over the `FUN_8004A908` tint,
    /// plus the dispatcher's near-plane reject) - the browser twin of the
    /// native draw loop's skips.
    pub fn play_battle_actor_transforms(&self) -> Vec<f32> {
        let (Some(br), Some(host)) = (self.battle_render.as_ref(), self.scene_host.host()) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(br.actors.len() * 5);
        for a in &br.actors {
            match host.world.actors.get(a.actor_idx) {
                Some(actor) => out.extend_from_slice(&[
                    actor.move_state.world_x as f32,
                    actor.move_state.world_y as f32,
                    actor.move_state.world_z as f32,
                    battle_facing_yaw(actor.battle.facing_angle),
                    if actor.active
                        && !host.world.battle_body_hidden(a.actor_idx)
                        && self.battle_draw_plan(a.actor_idx).is_none_or(|p| p.drawn)
                    {
                        1.0
                    } else {
                        0.0
                    },
                ]),
                None => out.extend_from_slice(&[0.0; 5]),
            }
        }
        out
    }

    /// Battle actor `i`'s current pose: 6 `i32` per part
    /// (`[tx, ty, tz, rx, ry, rz]`, absolute PSX 4096-unit angles) - the
    /// live `pose_frame` the engine's battle-animation tick maintains, or
    /// the build-time rest pose until the first battle tick lands. Empty =
    /// draw the uploaded geometry as-is.
    pub fn play_battle_actor_pose(&self, i: u32) -> Vec<i32> {
        let Some(br) = self.battle_render.as_ref() else {
            return Vec::new();
        };
        let Some(a) = br.actors.get(i as usize) else {
            return Vec::new();
        };
        if a.object_ids.is_empty() {
            return Vec::new();
        }
        if let Some(pose) = self
            .scene_host
            .host()
            .and_then(|h| h.world.actors.get(a.actor_idx))
            .and_then(|actor| actor.pose_frame.as_ref())
            && !pose.bone_outputs.is_empty()
        {
            let mut out = Vec::with_capacity(pose.bone_outputs.len() * 6);
            for (t, r) in &pose.bone_outputs {
                out.extend_from_slice(&[
                    t[0] as i32,
                    t[1] as i32,
                    t[2] as i32,
                    r[0] as i32,
                    r[1] as i32,
                    r[2] as i32,
                ]);
            }
            return out;
        }
        a.rest_pose.clone()
    }

    /// Battle actor `i`'s **hidden objects** this frame, as TMD object
    /// indices: the objects its per-channel model table does not name for
    /// the playing stream's channels (`World::battle_live_drawn_objects` -
    /// the draw pass `FUN_80048A08` after the mesh-variant swap
    /// `FUN_8004CCD4`). The page collapses their vertices after posing.
    /// Empty for an actor with no model table (monsters, summons, a
    /// fallback form) - every object draws. The native window's twin masks
    /// the TMD in `build_posed_actor_overrides`.
    pub fn play_battle_actor_hidden(&self, i: u32) -> Vec<u32> {
        let (Some(br), Some(host)) = (self.battle_render.as_ref(), self.scene_host.host()) else {
            return Vec::new();
        };
        let Some(a) = br.actors.get(i as usize) else {
            return Vec::new();
        };
        if a.object_ids.is_empty() {
            return Vec::new();
        }
        host.world
            .battle_live_drawn_objects(a.actor_idx)
            .map(|d| legaia_engine_battle::battle_attach_swap::hidden_objects(&d))
            .unwrap_or_default()
    }

    /// Battle actor `i`'s object-local positions with the playing clip's
    /// morph lanes blended in (`World::battle_morphed_tmd`, the port of
    /// `FUN_800495C8`'s blend before the draw), index-aligned with
    /// [`Self::play_battle_actor_positions`]: the page poses these instead
    /// of the uploaded rest positions this frame. Empty while no lane is
    /// active (the rest positions stand), or for a body with no morph source.
    pub fn play_battle_actor_morph(&self, i: u32) -> Vec<f32> {
        let Some(a) = self
            .battle_render
            .as_ref()
            .and_then(|b| b.actors.get(i as usize))
        else {
            return Vec::new();
        };
        let Some(src) = a.morph_src.as_ref() else {
            return Vec::new();
        };
        let Some(host) = self.scene_host.host() else {
            return Vec::new();
        };
        let Some(tmd) = host.world.battle_morphed_tmd(a.actor_idx, &src.0) else {
            return Vec::new();
        };
        let positions: Vec<f32> = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &src.1)
            .positions
            .iter()
            .flatten()
            .copied()
            .collect();
        // The upload is `battle_render_mesh`'s, the same builder over the
        // same prims; a length mismatch would mis-pose every vertex.
        if positions.len() != a.mesh.mesh.positions.len() * 3 {
            return Vec::new();
        }
        positions
    }

    /// Battle actor `i`'s body broken by the shatter renderer this frame
    /// (its `+0x1BA` word, `BattleActorDrawPlan::shatter`; kernel
    /// `legaia_render_kernels::shatter`, the native window's shattered
    /// override too): one object-local position per **index** of the bound
    /// mesh ([`Self::play_battle_actor_indices`]), so corner `k` keeps vertex
    /// `indices[k]`'s UV, colour and bone. The page poses these with the
    /// live pose and draws them de-indexed, both sides. Empty while the body
    /// is whole, or for a statically-posed upload (no bone ids to pose by).
    pub fn play_battle_actor_shatter(&self, i: u32) -> Vec<f32> {
        let Some(a) = self
            .battle_render
            .as_ref()
            .and_then(|b| b.actors.get(i as usize))
        else {
            return Vec::new();
        };
        if a.object_ids.is_empty() {
            return Vec::new();
        }
        let Some(input) = self.battle_draw_plan(a.actor_idx).and_then(|p| p.shatter) else {
            return Vec::new();
        };
        // A sweep row measures each object in the pose the page draws it in
        // ([`Self::play_battle_actor_pose`]) through the body's view map, as
        // the native override does.
        let view = self.battle_shatter_view(a.actor_idx);
        let bones: Vec<legaia_render_kernels::shatter::BonePose> = self
            .play_battle_actor_pose(i)
            .as_chunks::<6>()
            .0
            .iter()
            .map(|p| {
                (
                    [p[0] as i16, p[1] as i16, p[2] as i16],
                    [p[3] as i16, p[4] as i16, p[5] as i16],
                )
            })
            .collect();
        legaia_render_kernels::shatter::shatter_local_triangles(
            &a.mesh.mesh.positions,
            &a.mesh.mesh.indices,
            &a.object_ids,
            &input,
            |o| legaia_render_kernels::shatter::bone_sweep_frame(&bones, o, true, &view),
        )
        .into_iter()
        .flatten()
        .collect()
    }

    /// Battle actor `i`'s **arts after-image ghosts** this frame - the
    /// browser seat of the retail walk `FUN_80049348`
    /// (`legaia_engine_battle::battle_afterimage`; the plan comes from the
    /// shared `World::battle_ghost_draws`). Seven `f32` per ghost:
    /// `[world_x, world_y, world_z, r, g, b, k]` - the position in the
    /// placement frame (`battle_afterimage::placement_pos`, height negated),
    /// which the page places as it stands - with RGB normalized `0..1`
    /// (the page draws each ghost flat-coloured additive via its cue
    /// override) and `k` the eye-push scale factor: the position is already
    /// pushed to `eye + k * (pos - eye)` and the page multiplies its draw
    /// scale by `k`, which together is retail's deeper-OT-bucket ordering
    /// under a depth buffer (see
    /// `legaia_engine_battle::battle_afterimage::ghost_eye_push_scale` - the
    /// screen silhouette is unchanged, the body's opaque depth wins where
    /// they overlap). The matching pose is
    /// [`Self::play_battle_actor_ghost_pose`]. Empty outside a
    /// SpecialStarter dash / non-idle monster clip.
    pub fn play_battle_actor_ghosts(&self, i: u32) -> Vec<f32> {
        use legaia_engine_battle::battle_afterimage as ai;
        let (Some(br), Some(host)) = (self.battle_render.as_ref(), self.scene_host.host()) else {
            return Vec::new();
        };
        let Some(a) = br.actors.get(i as usize) else {
            return Vec::new();
        };
        // The eye of the page's own vp (aspect only shapes the projection
        // rows, not the centre of projection). The vp's input space is the
        // page's scaled placement space, so the eye comes back scaled - the
        // ghost push runs in raw actor units.
        let pose = self.battle_cam_pose();
        let vp = legaia_engine_camera::projection::battle_vp(&pose, BATTLE_WORLD_SCALE, 4.0 / 3.0);
        let eye_raw = ai::camera_eye_from_vp(&vp).map(|e| e.map(|c| c / BATTLE_WORLD_SCALE));
        // Positions in the placement frame (`T(F pos)`,
        // `battle_afterimage::placement_pos`) - the frame the eye comes
        // back in and the page places them in.
        let body = host.world.actors.get(a.actor_idx).map(|w| {
            ai::placement_pos([
                f32::from(w.move_state.world_x),
                f32::from(w.move_state.world_y),
                f32::from(w.move_state.world_z),
            ])
        });
        let mut out = Vec::new();
        for g in host.world.battle_ghost_draws() {
            if g.actor_slot as usize != a.actor_idx {
                continue;
            }
            let gpos = ai::placement_pos([g.pos[0] as f32, g.pos[1] as f32, g.pos[2] as f32]);
            let (pos, k) = match (eye_raw, body) {
                (Some(e), Some(b)) => {
                    let k = ai::ghost_eye_push_scale(e, gpos, b, ai::GHOST_EYE_PUSH_MARGIN);
                    (
                        [
                            e[0] + k * (gpos[0] - e[0]),
                            e[1] + k * (gpos[1] - e[1]),
                            e[2] + k * (gpos[2] - e[2]),
                        ],
                        k,
                    )
                }
                _ => (gpos, 1.0),
            };
            out.extend_from_slice(&[
                pos[0],
                pos[1],
                pos[2],
                f32::from(g.color[0]) / 255.0,
                f32::from(g.color[1]) / 255.0,
                f32::from(g.color[2]) / 255.0,
                k,
            ]);
        }
        out
    }

    /// Ghost `g` of battle actor `i`'s hidden objects - the ghost's own
    /// re-run of the mesh-variant swap (`FUN_80049348` -> `FUN_8004CCD4`),
    /// in the [`Self::play_battle_actor_hidden`] shape.
    pub fn play_battle_actor_ghost_hidden(&self, i: u32, g: u32) -> Vec<u32> {
        let (Some(br), Some(host)) = (self.battle_render.as_ref(), self.scene_host.host()) else {
            return Vec::new();
        };
        let Some(a) = br.actors.get(i as usize) else {
            return Vec::new();
        };
        host.world
            .battle_ghost_draws()
            .into_iter()
            .filter(|d| d.actor_slot as usize == a.actor_idx)
            .nth(g as usize)
            .and_then(|d| d.drawn)
            .map(|d| legaia_engine_battle::battle_attach_swap::hidden_objects(&d))
            .unwrap_or_default()
    }

    /// Ghost `g` of battle actor `i`'s NCLIP word, in
    /// `TmdRenderer.setNclipCull`'s encoding (the page's placement
    /// `nclip`): the shared `BattleGhostDraw::nclip_mode`, single-sided as
    /// the walk's draw word leaves it - the native window stages the same
    /// word per draw. `0` (the frame's word) when the ghost is not active.
    pub fn play_battle_actor_ghost_nclip(&self, i: u32, g: u32) -> u32 {
        let (Some(br), Some(host)) = (self.battle_render.as_ref(), self.scene_host.host()) else {
            return 0;
        };
        let Some(a) = br.actors.get(i as usize) else {
            return 0;
        };
        host.world
            .battle_ghost_draws()
            .iter()
            .filter(|d| d.actor_slot as usize == a.actor_idx)
            .nth(g as usize)
            .map_or(0, |d| d.nclip_mode())
    }

    /// Ghost `g` of battle actor `i`'s pose, in the
    /// [`Self::play_battle_actor_pose`] shape (6 `i32` per part). Empty when
    /// the ghost is not active this frame.
    pub fn play_battle_actor_ghost_pose(&self, i: u32, g: u32) -> Vec<i32> {
        let (Some(br), Some(host)) = (self.battle_render.as_ref(), self.scene_host.host()) else {
            return Vec::new();
        };
        let Some(a) = br.actors.get(i as usize) else {
            return Vec::new();
        };
        let draws = host.world.battle_ghost_draws();
        let Some(d) = draws
            .iter()
            .filter(|d| d.actor_slot as usize == a.actor_idx)
            .nth(g as usize)
        else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(d.pose.bone_outputs.len() * 6);
        for (t, r) in &d.pose.bone_outputs {
            out.extend_from_slice(&[
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
}
