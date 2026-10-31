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
use legaia_engine_expansion::party::{LocalNight, NetPodium, PodiumReturn};
use legaia_engine_expansion::podium_stage::{PodiumPlace, podium_frame};
use legaia_engine_field::field_player_clip::BASE_IDLE;
use legaia_engine_modes::map_frame::MapFrame;
use legaia_engine_modes::party::{PlayerKey, Standing};
use legaia_expansion_format::map::FloorTop;
use legaia_expansion_format::modes::PartyPodium;

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
        let standings = lp.night.standings();
        // The rivals are keys `1..`, in the podium's body order.
        let body = |k: PlayerKey| (k as usize).checked_sub(1).and_then(|i| pd.bodies.get(i));
        let stage = if up {
            self.raise_podium(
                &pd,
                &standings,
                PodiumWho {
                    me: 0,
                    body: &body,
                    shown: &[],
                    by_standing: false,
                },
                lp.frame,
                &mut lp.podium_since,
                &mut lp.podium_return,
            )
        } else {
            None
        };
        match stage {
            Some(s) => lp.podium_stage = Some(s),
            None => {
                if lp.podium_stage.take().is_some() {
                    self.take_down_party_podium(lp);
                }
            }
        }
    }

    /// The podium's frame on its map, `None` while the player is off it.
    /// Stands the blocks as tops, the player on its block (or watching
    /// from in front) and each figure's body NPC on its own.
    fn raise_podium(
        &mut self,
        pd: &PartyPodium,
        standings: &[Standing],
        who: PodiumWho<'_>,
        frame: u32,
        since: &mut Option<u32>,
        ret_slot: &mut Option<PodiumReturn>,
    ) -> Option<StageFrame> {
        let (cx, cz) = self
            .expansion
            .map
            .as_ref()
            .filter(|m| m.map_id == pd.map)
            .map(|m| m.def.tile_centre(pd.tile[0], pd.tile[1]))?;
        let since = *since.get_or_insert(frame);
        let place = PodiumPlace {
            centre: [cx as f32, cz as f32],
            facing: pd.facing,
        };
        let pod = podium_frame(place, standings, frame.wrapping_sub(since));
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
        let ret = ret_slot.get_or_insert_with(PodiumReturn::default);
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
        let (px, pz, heading) = match pod.figures.iter().find(|f| f.key == who.me) {
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
        // The others: each on its block, through its body NPC when the
        // podium names one, else a stand-in figure - unless this peer
        // already draws that player (a roaming avatar).
        let mut bodies = Vec::new();
        for f in pod
            .figures
            .iter()
            .filter(|f| f.key != who.me && !who.shown.contains(&f.key))
        {
            let npc = (who.body)(f.key).and_then(|id| {
                self.expansion
                    .map
                    .as_ref()?
                    .npcs()
                    .find(|(_, d)| &d.id == id)
                    .map(|(slot, _)| slot)
            });
            if let Some(slot) = npc {
                let ret = ret_slot.get_or_insert_with(PodiumReturn::default);
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
                // A stand-in's colour: the rival's seat alone, a networked
                // player's place in the standings (the same on every peer).
                seat: if who.by_standing {
                    standings.iter().position(|s| s.key == f.key).unwrap_or(0)
                } else {
                    f.key as usize
                },
                pos: f.pos,
                facing: f.facing,
                boosting: f.cheer,
                moving: false,
                actor: npc,
            });
        }
        Some(StageFrame {
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
            track: None,
            course: None,
            shot: pod.shot,
            banner: None,
            duel: None,
            poker: None,
            podium: Some(pod),
        })
    }

    /// Raise (or keep up) a networked party night's podium: `night_id`'s
    /// `modes.toml` `podium`, the room's final `standings`, this peer's
    /// player `me`, and the players `shown` as roaming avatars here (no
    /// stand-in figure for them). The night is the room's - host-directed
    /// and verified on every peer - and the anchor is the pack's, so every
    /// peer stands the same blocks in the same place; each stands its own
    /// leader on its block. `false` when the night names no podium. The
    /// world stays in its field: the room's party card holds the pad.
    pub fn show_net_podium(
        &mut self,
        night_id: &str,
        standings: Vec<Standing>,
        me: PlayerKey,
        shown: Vec<PlayerKey>,
    ) -> bool {
        let podium = self
            .pack_mode_sections()
            .0
            .party_night(night_id)
            .and_then(|d| d.podium.clone());
        let Some(podium) = podium else {
            self.clear_net_podium();
            return false;
        };
        let mut np = match self.expansion.net_podium.take() {
            Some(np) if np.night_id == night_id && np.me == me => np,
            other => {
                if let Some(mut old) = other {
                    self.lower_podium(old.ret.take());
                }
                Box::new(NetPodium {
                    night_id: night_id.to_string(),
                    podium,
                    standings: Vec::new(),
                    me,
                    shown: Vec::new(),
                    frame: 0,
                    idle: 0,
                    stage: None,
                    since: None,
                    ret: None,
                })
            }
        };
        np.standings = standings;
        np.shown = shown;
        np.frame = np.frame.wrapping_add(1);
        np.idle = 0;
        let free = self.expansion.mode_seat.is_none() && self.expansion.party.is_none();
        let stage = if free {
            let pd = np.podium.clone();
            let who = PodiumWho {
                me,
                body: &|_| None,
                shown: &np.shown,
                by_standing: true,
            };
            let standings = np.standings.clone();
            let mut since = np.since;
            let mut ret = np.ret.take();
            let s = self.raise_podium(&pd, &standings, who, np.frame, &mut since, &mut ret);
            np.since = since;
            np.ret = ret;
            s
        } else {
            None
        };
        if stage.is_none() && np.stage.is_some() {
            np.since = None;
            self.lower_podium(np.ret.take());
        }
        np.stage = stage;
        self.expansion.net_podium = Some(np);
        true
    }

    /// Take a networked night's podium down, putting the player back.
    /// No-op while none stands.
    pub fn clear_net_podium(&mut self) {
        if let Some(mut np) = self.expansion.net_podium.take() {
            self.lower_podium(np.ret.take());
        }
    }

    /// Frames a networked podium stands without the room raising it.
    const NET_PODIUM_IDLE: u32 = 4;

    /// The per-tick check: a networked podium the room has stopped raising
    /// comes down.
    pub(in crate::world) fn age_net_podium(&mut self) {
        let Some(np) = self.expansion.net_podium.as_deref_mut() else {
            return;
        };
        np.idle += 1;
        if np.idle > Self::NET_PODIUM_IDLE {
            self.clear_net_podium();
        }
    }

    /// The networked night's podium, while one is raised.
    pub fn net_podium(&self) -> Option<&NetPodium> {
        self.expansion.net_podium.as_deref()
    }

    /// The map a networked night's podium stands on, while the player is
    /// not on it yet (the scene host enters it).
    pub(in crate::world) fn net_podium_wants_map(&self) -> Option<(String, String)> {
        let np = self.expansion.net_podium.as_deref()?;
        if self.expansion.mode_seat.is_some() || self.expansion.party.is_some() {
            return None;
        }
        let here = self.expansion.map.as_ref().map(|m| m.map_id.as_str());
        (here != Some(np.podium.map.as_str())).then(|| (np.podium.map.clone(), String::new()))
    }

    /// Take the podium's blocks down and put the player and the bodies
    /// back where the podium found them.
    pub(in crate::world) fn take_down_party_podium(&mut self, lp: &mut LocalNight) {
        lp.podium_stage = None;
        lp.podium_since = None;
        self.lower_podium(lp.podium_return.take());
    }

    fn lower_podium(&mut self, ret: Option<PodiumReturn>) {
        if let Some(m) = self.expansion.map.as_mut() {
            m.set_stage_tops(Vec::new());
        }
        let Some(ret) = ret else {
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

/// Who the podium stands: this peer's player, the body NPC (by id) that
/// plays another figure, and the players drawn elsewhere (no stand-in).
struct PodiumWho<'a> {
    me: PlayerKey,
    body: &'a dyn Fn(PlayerKey) -> Option<&'a String>,
    shown: &'a [PlayerKey],
    /// Stand-in colours by standings order (networked) rather than key.
    by_standing: bool,
}
