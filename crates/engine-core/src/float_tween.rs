//! The per-frame **screen-position tween** over the `gp+0x148` drawable list.
//!
//! PORT: FUN_80031AE4 - the tween pass.
//! PORT: FUN_800353E0 - the scene-entry reset that empties the list.
//! PORT: FUN_8003C110 - the one-line mode setter the reset calls.
//! REF: FUN_800355F0, FUN_80032434, FUN_80031D00, FUN_80030628
//!
//! `gp+0x148` (`0x8007B460`) is **one** sentinel-circular doubly-linked list of
//! `0x34`-byte nodes, not several. The text/label producer `FUN_80032434`
//! builds the head and inserts nodes into it, the per-frame walker
//! `FUN_80031D00` draws them, `FUN_800355F0` drains it, and `FUN_80031AE4` -
//! ported here - is the pass that moves them. It touches only the nodes that
//! carry a tween descriptor at `+0x24`; every other node walks past untouched.
//!
//! ## Node fields this pass owns
//!
//! | Offset | Type | Role |
//! |---|---|---|
//! | `+0x00` | ptr | next node (the walk terminates on the head sentinel) |
//! | `+0x0A` | i16 | screen X - the value being tweened |
//! | `+0x0C` | i16 | screen Y - the value being tweened |
//! | `+0x1E` | u16 | elapsed tween time, in the adaptive frame step `DAT_1F800393` |
//! | `+0x20` | i16 | tween **phase**, `-2..=1` (see below) |
//! | `+0x24` | ptr | tween descriptor, or null for "not tweening" |
//!
//! ## Descriptor fields
//!
//! | Offset | Type | Role |
//! |---|---|---|
//! | `+0x00` | i16 | duration; zero means the node is skipped entirely |
//! | `+0x02` / `+0x04` | i16 | endpoint **A** (x, y) - the parked / home position |
//! | `+0x06` / `+0x08` | i16 | the **moving start** (x, y); rewritten on arrival |
//! | `+0x0A` / `+0x0C` | i16 | endpoint **B** (x, y) - the fly-out position |
//!
//! ## The phase is not a repeat count
//!
//! `node+0x20` reads as a signed **phase index**, and the dispatch is
//! `sel = phase + 2` over `0..=3` (`addiu v1,v0,2` at `0x80031B98`, then the
//! `beq`/`slti` ladder). Anything outside that range skips the node.
//!
//! | Phase | `sel` | Behaviour |
//! |---|---|---|
//! | `-2` | 0 | parked at endpoint A: position stamped from descriptor `+0x02`/`+0x04` |
//! | `-1` | 1 | moving **towards A**, interpolating from `+0x06`/`+0x08` |
//! | `0` | 2 | parked at B: the descriptor's start `+0x06`/`+0x08` latches to the node's current position |
//! | `1` | 3 | moving **towards B**, interpolating from `+0x06`/`+0x08` towards `+0x0A`/`+0x0C` |
//!
//! The elapsed timer only advances in the two *moving* phases (`phase == 1`
//! or `phase == -1`, the pair of `beq`s at `0x80031B2C`/`0x80031B34`), and on
//! expiry the phase **decrements by one** and the timer resets. There is no
//! reload anywhere in the function - `node+0x20` is written exactly once, at
//! `0x80031B74`, as `phase - 1` - so nothing here loops. A phase of `1` runs
//! one fly-out and stops at `0`; getting back to `-1` takes an external
//! writer. Reading `1` / `-1` as "loop" gets the sign of the whole mechanism
//! wrong: they are the two *directions*, and the tween is one-shot per leg.
//!
//! One store on the expiry path is dead. `0x80031B78`/`0x80031B84` snap the
//! node to endpoint **B** (`+0x0A`/`+0x0C`) regardless of direction, but when
//! the decrement lands on `-2` the `sel == 0` arm overwrites both halfwords
//! with endpoint A in the same call. Only the `1 -> 0` transition keeps it.
//!
//! ## Interpolation
//!
//! Both moving arms compute, per axis,
//! `start + ((target - start) * elapsed) / duration`, with a **signed
//! integer divide** by the descriptor's duration (`div` at `0x80031C0C` /
//! `0x80031C8C`) - not a shift, so the result is truncated towards zero and
//! the tween is not symmetric about the midpoint for odd durations. The
//! duration is guaranteed non-zero by the `beqz` at `0x80031B1C`.
//!
//! Each node that reaches either moving arm bumps the active-tween counter
//! `gp+0x868`, which the pass zeroes on entry (`sw zero,0x868(gp)` at
//! `0x80031AE8`). Nodes in a parked phase do not count.

/// A node's tween phase. The retail value is the signed halfword at
/// `node+0x20`; [`TweenPhase::from_raw`] rejects everything outside the
/// dispatch range the way the retail `slti` ladder does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TweenPhase {
    /// `-2` - parked at endpoint A.
    ParkedAtA,
    /// `-1` - moving towards endpoint A.
    MovingToA,
    /// `0` - parked at endpoint B; the descriptor start latches to the node.
    ParkedAtB,
    /// `1` - moving towards endpoint B.
    MovingToB,
}

impl TweenPhase {
    /// Decode the raw `node+0x20` halfword. `None` for any value the retail
    /// `sel = phase + 2` dispatch would fall through on.
    pub fn from_raw(raw: i16) -> Option<Self> {
        match raw {
            -2 => Some(Self::ParkedAtA),
            -1 => Some(Self::MovingToA),
            0 => Some(Self::ParkedAtB),
            1 => Some(Self::MovingToB),
            _ => None,
        }
    }

    /// The raw halfword this phase stores back into `node+0x20`.
    pub fn as_raw(self) -> i16 {
        match self {
            Self::ParkedAtA => -2,
            Self::MovingToA => -1,
            Self::ParkedAtB => 0,
            Self::MovingToB => 1,
        }
    }

    /// Whether this phase advances the elapsed timer. Only the two moving
    /// phases do.
    pub fn is_moving(self) -> bool {
        matches!(self, Self::MovingToA | Self::MovingToB)
    }
}

/// The `+0x24` tween descriptor, as the pass reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TweenDescriptor {
    /// `+0x00` - duration in frame-step units. Zero skips the node.
    pub duration: i16,
    /// `+0x02` / `+0x04` - endpoint A.
    pub park_a: (i16, i16),
    /// `+0x06` / `+0x08` - the moving start, rewritten by the `ParkedAtB` arm.
    pub start: (i16, i16),
    /// `+0x0A` / `+0x0C` - endpoint B.
    pub park_b: (i16, i16),
}

/// The mutable per-node tween state (`+0x0A`, `+0x0C`, `+0x1E`, `+0x20`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TweenNode {
    /// `+0x0A` / `+0x0C` - the animated screen position.
    pub pos: (i16, i16),
    /// `+0x1E` - elapsed time.
    pub elapsed: u16,
    /// `+0x20` - the raw phase halfword.
    pub phase: i16,
}

/// Outcome of ticking one node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TweenTick {
    /// Whether the node counted towards `gp+0x868` this frame.
    pub counted_active: bool,
}

/// Advance one node by `frame_step` (`DAT_1F800393`).
///
/// PORT: FUN_80031AE4 - the per-node body, `0x80031B04..0x80031CE0`.
///
/// Returns whether the node bumped the active counter. `desc` is taken by
/// `&mut` because the `ParkedAtB` arm writes the node's current position back
/// into the descriptor's start pair - retail's `sh` pair at `0x80031C5C` /
/// `0x80031C68`, which is what makes the next fly-out begin where the last
/// one ended.
pub fn tick_node(node: &mut TweenNode, desc: &mut TweenDescriptor, frame_step: u8) -> TweenTick {
    let inactive = TweenTick {
        counted_active: false,
    };
    if desc.duration == 0 {
        return inactive;
    }
    // The timer only runs in a moving phase, and only its expiry rewrites
    // the phase.
    if matches!(node.phase, 1 | -1) {
        node.elapsed = node.elapsed.wrapping_add(frame_step as u16);
        if (node.elapsed as i32) >= desc.duration as i32 {
            node.elapsed = 0;
            node.phase = node.phase.wrapping_sub(1);
            // Snapped to endpoint B on both directions; the ParkedAtA arm
            // below overwrites it when the decrement landed on -2.
            node.pos = desc.park_b;
        }
    }
    let Some(phase) = TweenPhase::from_raw(node.phase) else {
        return inactive;
    };
    match phase {
        TweenPhase::ParkedAtA => {
            node.pos = desc.park_a;
            inactive
        }
        TweenPhase::ParkedAtB => {
            desc.start = node.pos;
            inactive
        }
        TweenPhase::MovingToA => {
            node.pos = lerp_towards(desc.start, desc.park_a, node.elapsed, desc.duration);
            TweenTick {
                counted_active: true,
            }
        }
        TweenPhase::MovingToB => {
            node.pos = lerp_towards(desc.start, desc.park_b, node.elapsed, desc.duration);
            TweenTick {
                counted_active: true,
            }
        }
    }
}

/// `start + ((target - start) * elapsed) / duration`, per axis, with the
/// retail signed truncating divide.
fn lerp_towards(start: (i16, i16), target: (i16, i16), elapsed: u16, duration: i16) -> (i16, i16) {
    let axis = |s: i16, t: i16| -> i16 {
        let num = (t as i32 - s as i32) * elapsed as i32;
        (s as i32).wrapping_add(num / duration as i32) as i16
    };
    (axis(start.0, target.0), axis(start.1, target.1))
}

/// Tick a whole list in walk order and return the active-tween count the pass
/// leaves in `gp+0x868`.
///
/// PORT: FUN_80031AE4 - the list walk, `0x80031AE4..0x80031CF4`.
///
/// Retail walks a sentinel-circular list from `head->next` and stops when it
/// comes back to the head; the counter is zeroed before the walk, so an empty
/// list leaves `0`.
pub fn tick_list(nodes: &mut [(TweenNode, TweenDescriptor)], frame_step: u8) -> u32 {
    let mut active = 0;
    for (node, desc) in nodes.iter_mut() {
        if tick_node(node, desc, frame_step).counted_active {
            active += 1;
        }
    }
    active
}

/// State the scene-entry reset `FUN_800353E0` writes.
///
/// PORT: FUN_800353E0
/// PORT: FUN_8003C110
///
/// Eleven instructions, four stores: the drawable-list head `gp+0x148` and
/// `gp+0x138` are zeroed, the one-line setter `FUN_8003C110` stamps
/// `DAT_80073F20 = 0x0C`, and `gp+0x13C` is set to `7` - the last of those
/// lands *after* the call, in the epilogue's delay window, so a reader that
/// stops at the `jal` misses it.
///
/// Zeroing `gp+0x148` discards the list head outright rather than draining it;
/// the drain path is the separate `FUN_800355F0`, which frees each node. So a
/// scene entry that goes through this reset leaks whatever the previous scene
/// left linked, and the port models the reset as "the list is now empty".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSubsystemReset {
    /// `gp+0x148` - the drawable-list head, cleared to 0.
    pub list_head: u32,
    /// `gp+0x138` - cleared to 0.
    pub gp_138: u32,
    /// `DAT_80073F20` - stamped by `FUN_8003C110(0x0C)`.
    pub mode_byte: u8,
    /// `gp+0x13C` - set to 7.
    pub gp_13c: u32,
}

/// The literal state `FUN_800353E0` leaves behind.
pub const FIELD_SUBSYSTEM_RESET: FieldSubsystemReset = FieldSubsystemReset {
    list_head: 0,
    gp_138: 0,
    mode_byte: 0x0C,
    gp_13c: 7,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn desc() -> TweenDescriptor {
        TweenDescriptor {
            duration: 16,
            park_a: (10, 20),
            start: (10, 20),
            park_b: (90, 100),
        }
    }

    #[test]
    fn zero_duration_skips_the_node_entirely() {
        let mut n = TweenNode {
            pos: (1, 2),
            elapsed: 5,
            phase: 1,
        };
        let mut d = TweenDescriptor {
            duration: 0,
            ..desc()
        };
        assert!(!tick_node(&mut n, &mut d, 4).counted_active);
        assert_eq!(n.pos, (1, 2), "position untouched");
        assert_eq!(n.elapsed, 5, "timer not advanced");
    }

    #[test]
    fn only_the_moving_phases_advance_the_timer() {
        for (phase, moves) in [(-2i16, false), (-1, true), (0, false), (1, true)] {
            let mut n = TweenNode {
                pos: (0, 0),
                elapsed: 1,
                phase,
            };
            let mut d = desc();
            tick_node(&mut n, &mut d, 3);
            if moves {
                assert_ne!(n.elapsed, 1, "phase {phase} should have advanced");
            } else {
                assert_eq!(n.elapsed, 1, "phase {phase} must not advance");
            }
        }
    }

    #[test]
    fn phase_decrements_once_and_never_reloads() {
        // The one-shot claim: run a fly-out to completion, keep ticking, and
        // the phase must settle at 0 rather than cycling back to 1.
        let mut n = TweenNode {
            pos: (10, 20),
            elapsed: 0,
            phase: 1,
        };
        let mut d = desc();
        for _ in 0..64 {
            tick_node(&mut n, &mut d, 4);
        }
        assert_eq!(n.phase, 0, "settled at ParkedAtB, not looping");
        assert_eq!(n.pos, (90, 100), "parked on endpoint B");
    }

    #[test]
    fn arrival_at_b_latches_the_descriptor_start() {
        let mut n = TweenNode {
            pos: (10, 20),
            elapsed: 12,
            phase: 1,
        };
        let mut d = desc();
        tick_node(&mut n, &mut d, 8); // 12 + 8 >= 16 -> expiry
        assert_eq!(n.phase, 0);
        // Same call runs the ParkedAtB arm, which copies the snapped
        // position into the descriptor's start pair.
        assert_eq!(d.start, (90, 100));
        assert_eq!(n.elapsed, 0);
    }

    #[test]
    fn arrival_at_a_overwrites_the_dead_endpoint_b_snap() {
        // Retail snaps to endpoint B on BOTH expiry directions, then the
        // sel == 0 arm stamps endpoint A over it in the same call.
        let mut n = TweenNode {
            pos: (50, 60),
            elapsed: 15,
            phase: -1,
        };
        let mut d = desc();
        tick_node(&mut n, &mut d, 4);
        assert_eq!(n.phase, -2);
        assert_eq!(n.pos, d.park_a, "endpoint A wins, not the B snap");
    }

    #[test]
    fn interpolation_uses_a_truncating_signed_divide() {
        // duration 3, elapsed 1: (90 - 10) * 1 / 3 = 26 (not 26.67).
        let mut n = TweenNode {
            pos: (0, 0),
            elapsed: 0,
            phase: 1,
        };
        let mut d = TweenDescriptor {
            duration: 3,
            park_a: (10, 20),
            start: (10, 20),
            park_b: (90, 20),
        };
        tick_node(&mut n, &mut d, 1);
        assert_eq!(n.pos.0, 10 + 80 / 3);
        // And it truncates towards zero on a negative delta too.
        let mut n2 = TweenNode {
            pos: (0, 0),
            elapsed: 0,
            phase: 1,
        };
        let mut d2 = TweenDescriptor {
            duration: 3,
            park_a: (10, 20),
            start: (10, 20),
            park_b: (-70, 20),
        };
        tick_node(&mut n2, &mut d2, 1);
        assert_eq!(n2.pos.0, 10 + (-80) / 3);
    }

    #[test]
    fn only_moving_nodes_count_towards_the_active_counter() {
        let mut list = vec![
            (
                TweenNode {
                    pos: (0, 0),
                    elapsed: 0,
                    phase: 1,
                },
                desc(),
            ),
            (
                TweenNode {
                    pos: (0, 0),
                    elapsed: 0,
                    phase: -1,
                },
                desc(),
            ),
            (
                TweenNode {
                    pos: (0, 0),
                    elapsed: 0,
                    phase: 0,
                },
                desc(),
            ),
            (
                TweenNode {
                    pos: (0, 0),
                    elapsed: 0,
                    phase: -2,
                },
                desc(),
            ),
            (
                TweenNode {
                    pos: (0, 0),
                    elapsed: 0,
                    phase: 7,
                },
                desc(),
            ),
        ];
        assert_eq!(tick_list(&mut list, 1), 2);
        // An empty list leaves the counter at the zero the pass writes on
        // entry.
        assert_eq!(tick_list(&mut [], 1), 0);
    }

    #[test]
    fn out_of_range_phases_fall_through() {
        for raw in [-3i16, 2, 100, i16::MIN, i16::MAX] {
            assert!(TweenPhase::from_raw(raw).is_none(), "{raw} decoded");
            let mut n = TweenNode {
                pos: (5, 6),
                elapsed: 0,
                phase: raw,
            };
            let mut d = desc();
            assert!(!tick_node(&mut n, &mut d, 4).counted_active);
            assert_eq!(n.pos, (5, 6));
        }
    }

    #[test]
    fn phase_raw_round_trips() {
        for raw in -2i16..=1 {
            assert_eq!(TweenPhase::from_raw(raw).unwrap().as_raw(), raw);
        }
    }

    #[test]
    fn field_subsystem_reset_matches_the_four_retail_stores() {
        assert_eq!(FIELD_SUBSYSTEM_RESET.list_head, 0);
        assert_eq!(FIELD_SUBSYSTEM_RESET.gp_138, 0);
        assert_eq!(FIELD_SUBSYSTEM_RESET.mode_byte, 0x0C);
        // The gp+0x13C store is emitted after the jal, so it is easy to drop.
        assert_eq!(FIELD_SUBSYSTEM_RESET.gp_13c, 7);
    }
}
