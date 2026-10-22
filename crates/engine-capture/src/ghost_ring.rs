//! [`GhostRingSeed`]: where a capture's arts after-image ghosts stood - the
//! history-ring slots the ghost walk re-poses each one from - with its
//! environment hand-off.
//!
//! The walk `FUN_80049348` draws a body's ghosts from its per-battle-frame
//! history ring: the cursor `+0x17A`, the live pair `+0x4C` and the ring id
//! `+0x1FB` of the slots `step` and `2 * step` back. A replay's ring is its
//! own clock: it runs another frame step than the capture's passes did, so
//! the same depth reaches another keyframe of the swing, and its ground
//! history is the replay's. The capture holds the ring; the frame takes it.

/// One ghost as the capture's displayed frame drew it: engine actor slot,
/// the clip the ring holds (a party seat's committed `+0x1D9`), the walk's
/// depth in battle frames, the ring's cursor (12.4 sixteenths of a
/// keyframe) and live pair at that depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GhostRingSeed {
    pub slot: u8,
    pub clip: u8,
    pub depth: u8,
    pub cursor: u16,
    pub x: i32,
    pub z: i32,
}

/// Seat each ghost on its slot's history ring
/// (`World::seat_battle_ghost_ring`); a slot playing another clip than the
/// ring's keeps its own ghosts.
pub fn apply_ghost_ring(world: &mut legaia_engine_core::world::World, seeds: &[GhostRingSeed]) {
    for s in seeds {
        if world.battle_current_anim(usize::from(s.slot)) != s.clip {
            continue;
        }
        world.seat_battle_ghost_ring(
            usize::from(s.slot),
            usize::from(s.depth),
            s.cursor,
            [s.x, s.z],
        );
    }
}

/// `LEGAIA_SEAT_GHOST_RING`: `slot:clip:depth:cursor:x:z`, `,`-joined.
pub fn ghost_ring_to_env(seeds: &[GhostRingSeed]) -> String {
    seeds
        .iter()
        .map(|s| {
            format!(
                "{}:{}:{}:{}:{}:{}",
                s.slot, s.clip, s.depth, s.cursor, s.x, s.z
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// The inverse of [`ghost_ring_to_env`]; malformed entries are skipped.
pub fn ghost_ring_from_env(v: &str) -> Vec<GhostRingSeed> {
    v.split(',')
        .filter_map(|e| {
            let mut f = e.trim().split(':');
            Some(GhostRingSeed {
                slot: f.next()?.parse().ok()?,
                clip: f.next()?.parse().ok()?,
                depth: f.next()?.parse().ok()?,
                cursor: f.next()?.parse().ok()?,
                x: f.next()?.parse().ok()?,
                z: f.next()?.parse().ok()?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_round_trips() {
        let s = vec![
            GhostRingSeed {
                slot: 0,
                clip: 0x11,
                depth: 2,
                cursor: 80,
                x: -21,
                z: 116,
            },
            GhostRingSeed {
                slot: 1,
                clip: 0x10,
                depth: 4,
                cursor: 0,
                x: 300,
                z: -1200,
            },
        ];
        assert_eq!(ghost_ring_from_env(&ghost_ring_to_env(&s)), s);
        assert!(ghost_ring_from_env("1:17:2:x:3:4,").is_empty());
    }
}
