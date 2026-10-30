//! A party night's podium **in the world**
//! ([`legaia_engine_expansion::podium_stage`]): once a night played alone
//! completes, and its `modes.toml` `podium` names the pack map the player
//! is on, the podium stands on that map instead of the party card's flat
//! one - three blocks raised as standable tops, the player's leader and
//! the rivals' bodies standing on them, confetti, and the camera's shot.
//!
//! The stage is presentation: the night's standings are its only input,
//! and it is read through [`World::pack_mode_stage`] like any in-world
//! stage, so both hosts frame the shot and draw the blocks and the
//! confetti through `engine-world-views`. Closing the night takes the
//! blocks down and puts the player and the bodies back.

use legaia_engine_expansion::mode_stage::{StageBody, StageFrame};
use legaia_engine_expansion::party::{LocalNight, PodiumReturn};
use legaia_engine_expansion::podium_stage::{PodiumPlace, podium_frame};
use legaia_engine_field::field_player_clip::BASE_IDLE;
use legaia_engine_modes::map_frame::MapFrame;
use legaia_expansion_format::map::FloorTop;

use super::World;
use super::expansion_stage::mode_angle_to_heading;

/// How far in front of the podium a player who placed off it watches,
/// world units.
const WATCH_DISTANCE: f32 = 260.0;

fn clamp16(v: f32) -> i16 {
    v.clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
}

impl World {
    /// The pack map a completed night's podium stands on, while the player
    /// is not on it yet (the scene host enters it).
    pub(in crate::world) fn party_podium_wants_map(&self) -> Option<(String, String)> {
        let lp = self.expansion.party.as_deref()?;
        let pd = lp.podium.as_ref()?;
        if !lp.night.completed() || self.expansion.mode_seat.is_some() {
            return None;
        }
        let here = self.expansion.map.as_ref().map(|m| m.map_id.as_str());
        (here != Some(pd.map.as_str())).then(|| (pd.map.clone(), String::new()))
    }

    /// Stand the podium for this frame, or take it down when the night has
    /// not reached it (or is off its map).
    pub(in crate::world) fn stage_party_podium(&mut self, lp: &mut LocalNight) {
        let Some(pd) = lp.podium.clone() else {
            return;
        };
        let up = lp.night.completed() && self.expansion.mode_seat.is_none();
        let centre = self
            .expansion
            .map
            .as_ref()
            .filter(|m| m.map_id == pd.map)
            .map(|m| m.def.tile_centre(pd.tile[0], pd.tile[1]));
        let (Some((cx, cz)), true) = (centre, up) else {
            if lp.podium_stage.take().is_some() {
                self.take_down_party_podium(lp);
            }
            return;
        };
        let since = *lp.podium_since.get_or_insert(lp.frame);
        let place = PodiumPlace {
            centre: [cx as f32, cz as f32],
            facing: pd.facing,
        };
        let pod = podium_frame(place, &lp.night.standings(), lp.frame.wrapping_sub(since));
        // The blocks stand as tops over the floor they rise from: sample
        // the floor with the last frame's blocks down.
        if let Some(m) = self.expansion.map.as_mut() {
            m.set_stage_tops(Vec::new());
        }
        let tops: Vec<FloorTop> = pod
            .blocks
            .iter()
            .filter(|b| b.height >= 1.0)
            .map(|b| {
                let (x, z) = (b.centre[0] as i32, b.centre[1] as i32);
                let floor = -self.sample_field_floor_height(x, z);
                let h = b.half as i32;
                FloorTop {
                    rect: [x - h, z - h, x + h, z + h],
                    top: clamp16(floor as f32 + b.height),
                }
            })
            .collect();
        if let Some(m) = self.expansion.map.as_mut() {
            m.set_stage_tops(tops);
        }
        let ret = lp.podium_return.get_or_insert_with(PodiumReturn::default);
        if ret.player.is_none() {
            ret.player = self
                .player_actor_slot
                .and_then(|s| self.actors.get(usize::from(s)))
                .map(|a| {
                    let m = &a.move_state;
                    [m.world_x, m.world_y, m.world_z, m.render_26]
                });
        }
        // The player: on its block, or watching from in front.
        let facing = pod.shot.yaw;
        let (px, pz, heading) = match pod.figures.iter().find(|f| f.key == 0) {
            Some(f) => (f.pos[0], f.pos[1], mode_angle_to_heading(f.facing)),
            None => {
                let a = (facing - 2048.0) / 4096.0 * std::f32::consts::TAU;
                (
                    cx as f32 + a.cos() * WATCH_DISTANCE,
                    cz as f32 + a.sin() * WATCH_DISTANCE,
                    mode_angle_to_heading(facing.rem_euclid(4096.0) as u16),
                )
            }
        };
        if let Some(slot) = self.player_actor_slot.map(usize::from) {
            let y = self.sample_field_floor_height(px as i32, pz as i32);
            if let Some(a) = self.actors.get_mut(slot) {
                let m = &mut a.move_state;
                m.world_x = clamp16(px);
                m.world_z = clamp16(pz);
                m.world_y = clamp16(y as f32);
                m.render_26 = heading;
            }
            if self.locomotion.clip_base != BASE_IDLE {
                self.field_player_strided_clip(BASE_IDLE);
            }
        }
        // The rivals: each on its block, through its body NPC when the
        // podium names one, else a stand-in figure.
        let mut bodies = Vec::new();
        for f in pod.figures.iter().filter(|f| f.key != 0) {
            let npc = pd
                .bodies
                .get((f.key as usize).saturating_sub(1))
                .and_then(|id| {
                    self.expansion
                        .map
                        .as_ref()?
                        .npcs()
                        .find(|(_, d)| &d.id == id)
                        .map(|(slot, _)| slot)
                });
            if let Some(slot) = npc {
                let ret = lp.podium_return.get_or_insert_with(PodiumReturn::default);
                if !ret.bodies.iter().any(|b| b.0 == slot) {
                    ret.bodies.push((
                        slot,
                        self.npcs.positions.get(&slot).copied(),
                        self.npcs.headings.get(&slot).copied(),
                        self.npcs.clip_current.get(&slot).copied(),
                    ));
                }
                self.npcs
                    .positions
                    .insert(slot, (clamp16(f.pos[0]), clamp16(f.pos[1])));
                self.npcs
                    .headings
                    .insert(slot, mode_angle_to_heading(f.facing));
            }
            bodies.push(StageBody {
                seat: f.key as usize,
                pos: f.pos,
                facing: f.facing,
                boosting: f.cheer,
                moving: false,
                actor: npc,
            });
        }
        lp.podium_stage = Some(StageFrame {
            frame: MapFrame {
                map_id: pd.map.clone(),
                spawn: String::new(),
                origin: [cx, cz],
                unit: 1,
            },
            leader: None,
            bodies,
            balls: Vec::new(),
            cup: None,
            aim: None,
            trails: Vec::new(),
            shot: pod.shot,
            banner: None,
            duel: None,
            poker: None,
            podium: Some(pod),
        });
    }

    /// Take the podium's blocks down and put the player and the bodies
    /// back where the podium found them.
    pub(in crate::world) fn take_down_party_podium(&mut self, lp: &mut LocalNight) {
        lp.podium_stage = None;
        lp.podium_since = None;
        if let Some(m) = self.expansion.map.as_mut() {
            m.set_stage_tops(Vec::new());
        }
        let Some(ret) = lp.podium_return.take() else {
            return;
        };
        self.restore_pack_mode_player_from(ret.player);
        for (slot, pos, heading, clip) in ret.bodies {
            match pos {
                Some(p) => {
                    self.npcs.positions.insert(slot, p);
                }
                None => {
                    self.npcs.positions.remove(&slot);
                }
            }
            if let Some(h) = heading {
                self.npcs.headings.insert(slot, h);
            }
            if let Some(c) = clip {
                self.carry_npc_run_anim(slot, c);
            }
        }
    }
}
