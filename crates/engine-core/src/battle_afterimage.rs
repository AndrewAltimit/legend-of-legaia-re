//! Battle **arts after-image ghosts** - the mesh trail a Super / Miracle Art
//! dash leaves behind the character.
//!
//! PORT: FUN_80049348 (the per-actor arts after-image walk)
//!
//! Retail redraws the actor's own mesh at poses from a few frames ago. The
//! SCUS anim tick `FUN_80047430` keeps a 32-deep per-actor history ring -
//! position (`actor[+0x4C]`, 8-byte stride), anim cursor (`+0x17A`),
//! committed clip record (`+0x234`) and committed anim id (`+0x1FB`), all
//! shifted down one slot per frame with slot 0 taking the live values
//! (`0x80047E58..0x80047F0C`). The after-image walk `FUN_80049348` (run from
//! the per-actor battle draw tick `FUN_800480D8`) then draws **two** ghosts
//! from that ring:
//!
//! * **Spacing**: `step = 8 / actor[+0x21D]` (floored at 1; doubled for a
//!   monster seat), ghosts at ring depths `step` and `2 * step`
//!   (`0x800493F0..0x8004943C`) - so the trail stretches exactly when the
//!   arts slow-motion drops the rate.
//! * **Gate**: a ghost draws only when the ring's anim id at that depth is
//!   `> 0x10` (`0x80049458..0x80049464`). For a party member the committed
//!   art-clip slot is `0x11` precisely when the staged id was `0x10` or
//!   `0x1A` (`anim_vm::resolve_staged_anim`) - i.e. the Super / Miracle
//!   **SpecialStarter** dash; an ordinary art materialises at slot `0x10`
//!   and leaves the 2D weapon-trail streak instead. For a monster the ring
//!   id is `clip_tag + 0x10` (`0x80048044..0x80048060`), so any non-idle
//!   clip (tag `>= 1`) ghosts.
//! * **Colour**: the ghost is drawn **flat-coloured and additive** - the
//!   draw wrapper `FUN_80043390` decodes the colour word's mode byte
//!   (`0x85`): bit `0x80` sets the GP0 ABE (semi-transparent) command bit,
//!   low bits `0x01` = ABR mode 1 (additive), bit `0x04` selects the
//!   flat-colour prim bank with the GTE far colour = the ghost RGB. The base
//!   RGB is per-character (SCUS table `0x80076908`, indexed by character id;
//!   monsters share `0x80076914`), and each drawn ghost then steps the word
//!   down by `0x101010` (`0x80049520..0x80049530`) so the older ghost is
//!   dimmer. The ghost's OT depth is pushed `+0x50` buckets deeper than the
//!   live body (`FUN_80048A08`, the `+0x10` bit-`0x800000` arms), so it
//!   draws behind it.
//!
//! This module is the renderer-free kernel: the schedule, gate and colour
//! law. `World::battle_ghost_draws` binds it to the live pose history; each
//! host draws the returned poses as flat-coloured additive copies of the
//! actor's mesh.

/// Depth of the retail history ring (`0x1F` shifts + slot 0).
pub const HISTORY_DEPTH: usize = 32;

/// Number of ghosts one walk draws (loop `i = step; i < 2*step + 1;
/// i += step`).
pub const GHOST_COUNT: usize = 2;

/// Per-character ghost base colours `[r, g, b]` - the SCUS word table at
/// `0x80076908`, indexed `character_id - 1` (1 = Vahn, 2 = Noa, 3 = Gala;
/// `FUN_80049348` `0x8004939C..0x800493C4`). Byte order per the render
/// node's colour word: R = byte 0.
pub const GHOST_COLOR_PARTY: [[u8; 3]; 3] = [
    [0x60, 0x30, 0x30], // Vahn - red
    [0x30, 0x60, 0x30], // Noa - green
    [0x30, 0x30, 0x60], // Gala - blue
];

/// Monster ghost base colour (`DAT_80076914`, `0x800493C8..0x800493D4`).
pub const GHOST_COLOR_MONSTER: [u8; 3] = [0x50, 0x50, 0x30];

/// Per-drawn-ghost colour decay (`colour - 0x101010`, `0x80049520`).
pub const GHOST_COLOR_DECAY: u8 = 0x10;

/// Ring-id threshold: a ghost draws only for history ids **greater than**
/// `0x10` (`sltiu 0x11` at `0x80049460`).
pub const GHOST_RING_ID_MIN: u8 = 0x11;

/// The two ring depths the walk samples for an actor: `step` and `2 * step`
/// with `step = 8 / rate` (floored at 1 - retail floors the quotient's zero
/// case at `0x80049410`), doubled for a monster seat (`0x8004941C..
/// 0x80049428`). A frozen actor (`rate 0`) is clamped to the deepest pair
/// the ring can serve rather than reproducing retail's undefined
/// divide-by-zero.
pub fn ghost_depths(rate: u8, monster: bool) -> [usize; GHOST_COUNT] {
    let mut step = if rate == 0 {
        HISTORY_DEPTH / 2 - 1
    } else {
        ((8 / rate as usize).max(1)).min(HISTORY_DEPTH / 2 - 1)
    };
    if monster {
        step = (step * 2).min(HISTORY_DEPTH / 2 - 1);
    }
    [step, 2 * step]
}

/// One planned ghost draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GhostPlan {
    /// History-ring depth in frames (1 = last frame).
    pub depth: usize,
    /// Flat additive RGB for this ghost.
    pub color: [u8; 3],
}

/// Plan the walk for one actor: sample the two scheduled depths, keep the
/// ones whose history entry is ghost-eligible, and apply the per-drawn-ghost
/// colour decay (the decay steps only when a ghost is actually drawn -
/// retail decrements the live colour word inside the draw arm).
pub fn plan_ghosts(
    rate: u8,
    monster: bool,
    base: [u8; 3],
    mut eligible: impl FnMut(usize) -> bool,
) -> Vec<GhostPlan> {
    let mut out = Vec::with_capacity(GHOST_COUNT);
    let mut color = base;
    for depth in ghost_depths(rate, monster) {
        if !eligible(depth) {
            continue;
        }
        out.push(GhostPlan { depth, color });
        for c in color.iter_mut() {
            *c = c.saturating_sub(GHOST_COLOR_DECAY);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_rate_ghosts_trail_one_and_two_frames() {
        assert_eq!(ghost_depths(8, false), [1, 2]);
        // Monster seats double the spacing.
        assert_eq!(ghost_depths(8, true), [2, 4]);
    }

    #[test]
    fn slow_motion_stretches_the_trail() {
        assert_eq!(ghost_depths(4, false), [2, 4]);
        assert_eq!(ghost_depths(2, false), [4, 8]);
        // The starter's quarter-speed actor: depths 4 and 8 - the deep
        // dash trail.
        assert_eq!(ghost_depths(2, true), [8, 16]);
    }

    #[test]
    fn frozen_rate_clamps_inside_the_ring() {
        let [a, b] = ghost_depths(0, false);
        assert!(b > a && b < HISTORY_DEPTH);
        let [a, b] = ghost_depths(0, true);
        assert!(b > a && b < HISTORY_DEPTH);
    }

    #[test]
    fn plan_keeps_only_eligible_depths_and_decays_per_drawn_ghost() {
        // Both eligible: second ghost is one decay step dimmer.
        let plans = plan_ghosts(8, false, [0x60, 0x30, 0x30], |_| true);
        assert_eq!(
            plans,
            vec![
                GhostPlan {
                    depth: 1,
                    color: [0x60, 0x30, 0x30]
                },
                GhostPlan {
                    depth: 2,
                    color: [0x50, 0x20, 0x20]
                },
            ]
        );
        // First depth ineligible: the drawn ghost still gets the base
        // colour (the decay follows draws, not depths).
        let plans = plan_ghosts(8, false, [0x60, 0x30, 0x30], |d| d == 2);
        assert_eq!(
            plans,
            vec![GhostPlan {
                depth: 2,
                color: [0x60, 0x30, 0x30]
            }]
        );
        // None eligible: no ghosts.
        assert!(plan_ghosts(8, false, GHOST_COLOR_MONSTER, |_| false).is_empty());
    }
}
