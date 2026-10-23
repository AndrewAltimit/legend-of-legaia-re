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

/// One walk depth as the capture's displayed frame drew it: engine actor
/// slot, the clip the ring holds (a party seat's committed `+0x1D9`), the
/// walk's depth in battle frames, whether the walk drew a ghost there (the
/// ring id passed its `> 0x10` gate), and for a drawn one the ring's cursor
/// (12.4 sixteenths of a keyframe) and live pair at that depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GhostRingSeed {
    pub slot: u8,
    pub clip: u8,
    pub depth: u8,
    pub drawn: bool,
    pub cursor: u16,
    pub x: i32,
    pub z: i32,
}

/// Seat each depth on its slot's history ring: a drawn ghost re-posed
/// there (`World::seat_battle_ghost_ring`) when the slot plays the ring's
/// clip, an undrawn depth closed (`World::close_battle_ghost_ring`) - the
/// art had not committed that far back, which a replay whose art began
/// earlier on its own clock would otherwise ghost.
pub fn apply_ghost_ring(world: &mut legaia_engine_core::world::World, seeds: &[GhostRingSeed]) {
    for s in seeds {
        if !s.drawn {
            world.close_battle_ghost_ring(usize::from(s.slot), usize::from(s.depth));
            continue;
        }
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

/// `LEGAIA_SEAT_GHOST_RING`: `slot:clip:depth:drawn:cursor:x:z`
/// (`drawn` `0` / `1`), `,`-joined.
pub fn ghost_ring_to_env(seeds: &[GhostRingSeed]) -> String {
    seeds
        .iter()
        .map(|s| {
            format!(
                "{}:{}:{}:{}:{}:{}:{}",
                s.slot,
                s.clip,
                s.depth,
                u8::from(s.drawn),
                s.cursor,
                s.x,
                s.z
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
                drawn: f.next()?.parse::<u8>().ok()? != 0,
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
                drawn: true,
                cursor: 80,
                x: -21,
                z: 116,
            },
            GhostRingSeed {
                slot: 1,
                clip: 0x10,
                depth: 4,
                drawn: false,
                cursor: 0,
                x: 300,
                z: -1200,
            },
        ];
        assert_eq!(ghost_ring_from_env(&ghost_ring_to_env(&s)), s);
        assert!(ghost_ring_from_env("1:17:2:1:x:3:4,").is_empty());
    }
}
