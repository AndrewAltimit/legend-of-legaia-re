//! A golf round or a race played **in the world**: the mode seat's in-world
//! stage ([`legaia_engine_expansion::mode_stage`]) applied to the field the
//! pack map built.
//!
//! An instance that names a pack map runs in 3D once that map is the one
//! the player is in (`World::expansion.map`): each frame the seat's session
//! ticks as it always does, then [`World::stage_pack_mode`] reads its view
//! into a [`StageFrame`] and moves the party leader onto it - position on
//! the floor, facing, clip and its animation. The hosts draw the field, the
//! leader and the stage's own pieces (balls, aim arrow, bodies, trails)
//! through `legaia_engine_world_views`, and frame it through the stage's
//! shot. Nothing here feeds the session: a world that draws the stage and
//! one that does not tick the same session state.
//!
//! The map itself is the scene host's to enter
//! (`SceneHost::sync_pack_mode_map`), off [`World::pack_mode_wants_map`];
//! until it is entered the instance draws as its top-down screen.

use legaia_engine_expansion::mode_stage::{LeaderClip, StageFrame};
use legaia_engine_expansion::modes::PackModeSeat;
use legaia_engine_field::field_player_clip::{BASE_IDLE, BASE_RUN, BASE_WALK};
use legaia_engine_mode::mode::SceneMode;

use super::World;

/// The clip base the swing plays: the hop phase machine's landing write
/// (`7`), the party bank's nearest thing to a putt.
pub const SWING_CLIP_BASE: u16 = legaia_engine_vm::field_ledge_hop_arc::hop_phase::LANDING as u16;

/// A mode angle (`0` = world `+X`, a quarter turn = `+Z`) as the player's
/// field heading (`+0x26` in the engine's space, `0` = `+Z`, a quarter turn
/// = `+X`).
pub fn mode_angle_to_heading(angle: u16) -> i16 {
    (1024i32 - i32::from(angle)).rem_euclid(4096) as i16
}

impl World {
    /// The in-world stage this frame, while a session played on a pack map
    /// runs on that map.
    pub fn pack_mode_stage(&self) -> Option<&StageFrame> {
        if self.mode != SceneMode::Expansion {
            return None;
        }
        self.expansion.mode_seat.as_deref()?.stage_frame.as_ref()
    }

    /// The pack map (and spawn) the running instance is played on, when the
    /// player is not on it yet - the scene host enters it.
    pub fn pack_mode_wants_map(&self) -> Option<(String, String)> {
        if self.mode != SceneMode::Expansion {
            return None;
        }
        let frame = self.expansion.mode_seat.as_deref()?.active()?.map_frame()?;
        let here = self.expansion.map.as_ref().map(|m| m.map_id.as_str());
        (here != Some(frame.map_id.as_str())).then(|| (frame.map_id.clone(), frame.spawn.clone()))
    }

    /// Note the player's pose on the open seat, to restore when it closes.
    pub fn remember_pack_mode_player(&mut self) {
        let pose = self
            .player_actor_slot
            .and_then(|s| self.actors.get(usize::from(s)))
            .map(|a| {
                let m = &a.move_state;
                [m.world_x, m.world_y, m.world_z, m.render_26]
            });
        if let Some(seat) = self.expansion.mode_seat.as_deref_mut() {
            seat.player_return = pose;
        }
    }

    /// Put the player back where the seat found it (the in-world stage moved
    /// it), and the idle clip back on.
    pub(in crate::world) fn restore_pack_mode_player_from(&mut self, pose: Option<[i16; 4]>) {
        let Some([x, y, z, facing]) = pose else {
            return;
        };
        let Some(a) = self
            .player_actor_slot
            .and_then(|s| self.actors.get_mut(usize::from(s)))
        else {
            return;
        };
        let m = &mut a.move_state;
        (m.world_x, m.world_y, m.world_z, m.render_26) = (x, y, z, facing);
        if self.locomotion.clip_base != BASE_IDLE {
            self.field_player_strided_clip(BASE_IDLE);
        }
    }

    /// The map NPC ids instance `id` names as the other seats' bodies.
    fn pack_mode_body_ids(&self, id: &str) -> Vec<String> {
        let (modes, _) = self.pack_mode_sections();
        if let Some(c) = modes.golf.iter().find(|c| c.id == id) {
            return c.bodies.clone();
        }
        modes
            .races
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.bodies.clone())
            .unwrap_or_default()
    }

    /// Stand the stage's bodies on the map NPCs the instance names
    /// (`bodies`): the `k`th body (the seats in order, the leader's
    /// skipped) moves NPC `bodies[k]` to its pose - anchor, heading, its
    /// `walk_clip` while it moves and its own clip at rest - and is marked
    /// drawn by that actor. A body with no NPC stays a stand-in figure.
    fn stage_pack_mode_bodies(&mut self, seat: &mut PackModeSeat, mode_id: &str) {
        let ids = self.pack_mode_body_ids(mode_id);
        let Some(map) = self.expansion.map.as_ref() else {
            return;
        };
        let npcs: Vec<(u8, Option<u8>, Option<u8>)> = ids
            .iter()
            .map(|id| {
                map.npcs()
                    .find(|(_, d)| &d.id == id)
                    .map_or((0, None, None), |(slot, d)| (slot, d.walk_clip, d.clip))
            })
            .collect();
        let Some(frame) = seat.stage_frame.as_mut() else {
            return;
        };
        for (body, &(slot, walk_clip, own_clip)) in frame.bodies.iter_mut().zip(&npcs) {
            if slot == 0 {
                continue;
            }
            body.actor = Some(slot);
            let idle = match seat.body_idle.iter().find(|(s, _)| *s == slot) {
                Some(&(_, clip)) => clip,
                None => {
                    let current = self.npcs.clip_current.get(&slot).copied();
                    let clip = own_clip.or(current).unwrap_or(0);
                    seat.body_idle.push((slot, clip));
                    clip
                }
            };
            let clamp = |v: f32| v.clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
            self.npcs
                .positions
                .insert(slot, (clamp(body.pos[0]), clamp(body.pos[1])));
            self.npcs
                .headings
                .insert(slot, mode_angle_to_heading(body.facing));
            let clip = match walk_clip {
                Some(c) if body.moving => c,
                _ => idle,
            };
            self.carry_npc_run_anim(slot, clip);
        }
    }

    /// Put the bodies' NPCs back on their own tiles and clips.
    pub(in crate::world) fn restore_pack_mode_bodies(&mut self, body_idle: &[(u8, u8)]) {
        for &(slot, clip) in body_idle {
            if let Some(&at) = self.npcs.entry_positions.get(&slot) {
                self.npcs.positions.insert(slot, at);
            }
            self.carry_npc_run_anim(slot, clip);
        }
    }

    /// Step the seat's in-world stage and move the leader onto it. The stage
    /// runs only while the session's map is the one the player is in.
    pub(in crate::world) fn stage_pack_mode(&mut self, seat: &mut PackModeSeat) {
        let here = self.expansion.map.as_ref().map(|m| m.map_id.clone());
        let on_map = seat
            .active()
            .and_then(|m| m.map_frame())
            .is_some_and(|f| Some(&f.map_id) == here.as_ref());
        seat.stage_frame = None;
        if !on_map {
            return;
        }
        let legaia_engine_expansion::modes::ModeStage::Playing { mode, .. } = &seat.stage else {
            return;
        };
        seat.stage_frame = seat.world_stage.step(mode);
        let mode_id = mode.id.clone();
        self.stage_pack_mode_bodies(seat, &mode_id);
        let Some(leader) = seat.stage_frame.as_ref().and_then(|f| f.leader) else {
            return;
        };
        let Some(slot) = self.player_actor_slot.map(usize::from) else {
            return;
        };
        let (x, z) = (leader.pos[0] as i32, leader.pos[1] as i32);
        let y = self.sample_field_floor_height(x, z);
        if let Some(a) = self.actors.get_mut(slot) {
            let m = &mut a.move_state;
            m.world_x = x.clamp(i16::MIN.into(), i16::MAX.into()) as i16;
            m.world_z = z.clamp(i16::MIN.into(), i16::MAX.into()) as i16;
            m.world_y = y.clamp(i16::MIN.into(), i16::MAX.into()) as i16;
            m.render_26 = mode_angle_to_heading(leader.facing);
        }
        let base = match leader.clip {
            LeaderClip::Idle => BASE_IDLE,
            LeaderClip::Walk => BASE_WALK,
            LeaderClip::Run => BASE_RUN,
            LeaderClip::Swing => SWING_CLIP_BASE,
        };
        // The pad step's stamps: the party bank and the locomotion rate. A
        // leader the seat took over before any field frame ran still has the
        // scene bank selected, so the first frame rebinds.
        let rebind = self.locomotion.clip_base != base || !self.locomotion.player_party_bank;
        self.locomotion.player_party_bank = true;
        self.locomotion.player_clip_rate = legaia_engine_field::field_anim::CLIP_RATE as i16;
        if rebind {
            self.field_player_strided_clip(base);
        }
        if let Some(anim) = &mut self.locomotion.player_anim {
            anim.pad_drove_this_frame = true;
            anim.moved_this_frame |= matches!(leader.clip, LeaderClip::Walk | LeaderClip::Run);
        }
        self.tick_field_player_anim();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_angles_map_onto_field_headings() {
        // Mode 0 is world +X: engine heading a quarter turn.
        assert_eq!(mode_angle_to_heading(0), 1024);
        // Mode +Z (a quarter turn) is engine heading 0.
        assert_eq!(mode_angle_to_heading(1024), 0);
        assert_eq!(mode_angle_to_heading(2048), 3072);
    }
}
