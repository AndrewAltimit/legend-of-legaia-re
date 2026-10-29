//! Shared-world roaming: every player walks their own `World`, and each
//! sees the others as avatars.
//!
//! The model is host-authoritative for avatars only. A guest reports its
//! own avatar to the host a few times a second (an `Action` tagged
//! [`crate::mode_net::action_tag::AVATAR`]); the host gathers every report
//! plus its own into one `Snapshot` and broadcasts it at the same rate; each
//! peer, the host included, draws the snapshots through an
//! [`AvatarInterpolator`]. Nothing else is shared: each world runs its own
//! scripts, encounters and menus, so two players in one scene see each
//! other walk but not each other's battles, doors or dialogue.

use std::collections::BTreeMap;

use legaia_netplay::authority::{AvatarInterpolator, snapshot_interval_frames};
use legaia_netplay::protocol::{AvatarState, MAX_AVATARS, Message, PlayerId};

use crate::mode_net::action_tag;

/// Roaming snapshot rate.
pub const ROAM_HZ: u8 = 15;
/// The scene id of an avatar that is not in a field scene (in a menu, a
/// battle, a mode): reported but never drawn.
pub const NO_SCENE: u16 = u16::MAX;
/// Frames a guest's report stays in the host's snapshot without a fresh one.
const REPORT_TTL: u32 = 120;

/// The avatar body of an `AVATAR` action payload (the protocol's snapshot
/// layout for one avatar, without the player id the relay stamps).
pub fn encode_avatar(a: &AvatarState) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 2 + 12 + 6);
    out.push(action_tag::AVATAR);
    out.extend_from_slice(&a.scene.to_le_bytes());
    for c in a.pos {
        out.extend_from_slice(&c.to_le_bytes());
    }
    out.extend_from_slice(&a.facing.to_le_bytes());
    out.extend_from_slice(&a.anim.to_le_bytes());
    out.extend_from_slice(&a.anim_frame.to_le_bytes());
    out
}

/// The avatar in an `AVATAR` payload, owned by `player`.
pub fn decode_avatar(player: PlayerId, payload: &[u8]) -> Option<AvatarState> {
    let [action_tag::AVATAR, body @ ..] = payload else {
        return None;
    };
    if body.len() != 20 {
        return None;
    }
    let u16_at = |i: usize| u16::from_le_bytes([body[i], body[i + 1]]);
    let i32_at = |i: usize| i32::from_le_bytes([body[i], body[i + 1], body[i + 2], body[i + 3]]);
    Some(AvatarState {
        player,
        scene: u16_at(0),
        pos: [i32_at(2), i32_at(6), i32_at(10)],
        facing: u16_at(14),
        anim: u16_at(16),
        anim_frame: u16_at(18),
    })
}

/// What the roaming layer wants sent this frame.
#[derive(Debug, Clone, Default)]
pub struct Roam {
    interp: Option<AvatarInterpolator>,
    /// Host only: the latest report per guest and the frame it came.
    reports: BTreeMap<PlayerId, (u32, AvatarState)>,
    frame: u32,
}

impl Roam {
    pub fn new() -> Self {
        Self {
            interp: Some(AvatarInterpolator::for_rate(ROAM_HZ)),
            ..Self::default()
        }
    }

    /// A guest's report (host side).
    pub fn receive_report(&mut self, from: PlayerId, payload: &[u8]) -> bool {
        match decode_avatar(from, payload) {
            Some(a) => {
                self.reports.insert(from, (self.frame, a));
                true
            }
            None => false,
        }
    }

    /// A host snapshot (guest side).
    pub fn receive_snapshot(&mut self, tick: u32, avatars: &[AvatarState]) {
        if let Some(i) = self.interp.as_mut() {
            i.push_snapshot(tick, avatars);
        }
    }

    /// The latest report from `player` (host side).
    pub fn report_of(&self, player: PlayerId) -> Option<AvatarState> {
        self.reports.get(&player).map(|(_, a)| *a)
    }

    /// A player left: forget their report.
    pub fn forget(&mut self, player: PlayerId) {
        self.reports.remove(&player);
    }

    /// One frame. `own` is this player's avatar; returns the messages to
    /// send (a report from a guest, a snapshot from the host) on the
    /// snapshot cadence.
    pub fn frame(&mut self, own: AvatarState, hosting: bool) -> Vec<Message> {
        self.frame = self.frame.wrapping_add(1);
        if let Some(i) = self.interp.as_mut() {
            i.tick();
        }
        if !self.frame.is_multiple_of(snapshot_interval_frames(ROAM_HZ)) {
            return Vec::new();
        }
        if !hosting {
            return vec![Message::Action {
                from: own.player,
                payload: encode_avatar(&own),
            }];
        }
        let now = self.frame;
        self.reports
            .retain(|_, (at, _)| now.wrapping_sub(*at) <= REPORT_TTL);
        let mut avatars = vec![own];
        avatars.extend(self.reports.values().map(|(_, a)| *a).take(MAX_AVATARS - 1));
        self.receive_snapshot(now, &avatars);
        vec![Message::Snapshot { tick: now, avatars }]
    }

    /// The other players' avatars in `scene`, interpolated, for drawing.
    pub fn remote_avatars(&self, me: PlayerId, scene: u16) -> Vec<AvatarState> {
        if scene == NO_SCENE {
            return Vec::new();
        }
        self.interp
            .as_ref()
            .map(|i| i.sample())
            .unwrap_or_default()
            .into_iter()
            .filter(|a| a.player != me && a.scene == scene)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_avatar_report_round_trips() {
        let a = AvatarState {
            player: PlayerId(3),
            scene: 412,
            pos: [-1200, 64, 9000],
            facing: 3071,
            anim: 1,
            anim_frame: 17,
        };
        let bytes = encode_avatar(&a);
        assert_eq!(decode_avatar(PlayerId(3), &bytes), Some(a));
        assert_eq!(decode_avatar(PlayerId(3), &bytes[..10]), None);
        assert_eq!(decode_avatar(PlayerId(3), &[action_tag::PAD, 0, 0]), None);
    }
}
