//! [`ClipCursorSeed`]: where each combatant's playing clip stood in the
//! capture - the anim node's cursor `+0x68` - with its environment
//! hand-off.
//!
//! A replay reaches a capture's phase on its own clock: the pad path it
//! drives, the rounds it commits. A looping clip (every idle stance) is a
//! clock too, and nothing ties the replay's to retail's, so a seat that
//! landed the right phase still draws its combatants mid-sway at another
//! keyframe. The capture holds each node's cursor; the frame takes it.

/// One battle slot's playing clip as the capture shows it: engine actor
/// slot, the clip `+0x1D9`, and the node cursor `+0x68` of the displayed
/// frame (12.4 sixteenths of a keyframe; below zero, the clip wrapped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipCursorSeed {
    pub slot: u8,
    pub clip: u8,
    pub cursor: i32,
}

/// Land each seed on its slot's clip (`World::seat_battle_clip_cursor`);
/// a slot playing another clip than the capture's keeps its own cursor.
pub fn apply_clip_cursors(world: &mut legaia_engine_core::world::World, seeds: &[ClipCursorSeed]) {
    let mut any = false;
    for s in seeds {
        any |= world.seat_battle_clip_cursor(usize::from(s.slot), s.clip, s.cursor);
    }
    if any {
        world.settle_seated_clips();
    }
}

/// `LEGAIA_SEAT_BATTLE_CLIPS`: `slot:clip:cursor`, `,`-joined.
pub fn clip_cursors_to_env(seeds: &[ClipCursorSeed]) -> String {
    seeds
        .iter()
        .map(|s| format!("{}:{}:{}", s.slot, s.clip, s.cursor))
        .collect::<Vec<_>>()
        .join(",")
}

/// The inverse of [`clip_cursors_to_env`]; malformed entries are skipped.
pub fn clip_cursors_from_env(v: &str) -> Vec<ClipCursorSeed> {
    v.split(',')
        .filter_map(|e| {
            let mut f = e.trim().split(':');
            Some(ClipCursorSeed {
                slot: f.next()?.parse().ok()?,
                clip: f.next()?.parse().ok()?,
                cursor: f.next()?.parse().ok()?,
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
            ClipCursorSeed {
                slot: 1,
                clip: 0,
                cursor: 108,
            },
            ClipCursorSeed {
                slot: 3,
                clip: 2,
                cursor: 24,
            },
        ];
        assert_eq!(clip_cursors_from_env(&clip_cursors_to_env(&s)), s);
        assert!(clip_cursors_from_env("x:y").is_empty());
    }
}
