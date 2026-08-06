//! Per-clip **impact freeze + tint** - the battle-actor maintenance arms
//! that, on hand-picked frames of hand-picked party clips, freeze the
//! TARGET actor's pose and tint it with an impact-config colour.
//!
//! PORT: FUN_8004CE2C (pass 2 - the per-character clip-tag arms; pass 4,
//! the Stone CLUT recolour, is `legaia_engine_core::battle_status_clut`)
//! PORT: FUN_80050F30 (per-lane packed-colour ease - the tint's decay
//! primitive, driven by the presentation tick `FUN_80050120` arm 0)
//!
//! # The retail chain
//!
//! Every frame `FUN_8004CE2C` resolves the **acting** actor
//! (`ctx[+0x13]`), its committed action record (`+0x22C -> +0x4C`), the
//! record's `+0x77` clip-identity byte, the anim cursor `+0x68`
//! (12.4 fixed point - sixteenths of a keyframe), and the acting actor's
//! **target** (`+0x1DD`). It then dispatches on the roster character id
//! (`DAT_8007BD10[slot]`, 1 = Vahn / 2 = Noa / 3 = Gala) into per-clip
//! arms. The two clip-tag-`0x18` arms:
//!
//! * **Gala, tag `0x18`** (`0x8004D190..0x8004D1E4`): the acting actor's
//!   `+0x21F` impact selector is stamped `2` on the tag match alone; while
//!   the cursor sits in `0x40..=0x80` (keyframes 4..8) the TARGET gets
//!   `+0x21D = 0` (**pose freeze** - the rate-scaled cursor advance
//!   stops), `+0x04 = _DAT_801F53D8` (**tint** - impact-config entry 1),
//!   `+0x0C = 0x1000`, `+0x21F = 2`.
//! * **Vahn, tag `0x18`** (`0x8004D250..0x8004D29C`): cursor window
//!   `0x90..=0xA0`, tint only - `+0x04 = _DAT_801F53D4` (entry 0),
//!   `+0x0C = 0x1000`, `+0x21F = 1`. No freeze.
//!
//! Both writes re-apply every in-window frame. The freeze persists past
//! the window until the SM's Done arm reseeds every slot's `+0x21D = 8`
//! (`FUN_801E93C8`, ported as `battle_action::done`'s
//! `rearm_action_gauge`). The tint decays through the per-actor
//! presentation tick `FUN_80050120` arm 0 (`+0x21C == 0`): each frame the
//! packed word eases per-lane toward the neutral `0x20080200`
//! ([`ease_actor_state`] with target `(0x80, 0x80, 0x80)` and step 1);
//! once neutral, the `+0x0C` intensity word drains by `dt << 5` and the
//! `+0x21F` selector clears.
//!
//! The neighbouring arms of the same pass (Gala tags `0x16`/`0x17`/`0x67`,
//! Vahn `0x2B`, Noa `0x29`/`0x2D` status flags, monster `0x3B`) are
//! decoded in `ghidra/scripts/funcs/8004ce2c.txt` but not ported here -
//! the two `0x18` arms are the pair that freeze/tint a target.
//!
//! # The packed colour word
//!
//! Actor `+0x04` is a 10:10:10 packed colour (lane 0 = R, lane 1 = G,
//! lane 2 = B); the draw-time resolver `FUN_8004A908` shifts each lane
//! `>> 2` into the mesh colour byte, so lane `0x200` = `0x80` = neutral.
//! The impact tint values come from the disc's 5-entry impact-config
//! table at `0x801F53D4` (`legaia_asset::move_power::
//! parse_impact_effect_table`), 1-indexed by the `+0x21F` selector.

/// Neutral actor colour-state word (all lanes `0x200` = RGB `0x808080`) -
/// the value the tint decays back to. Identical to
/// [`crate::battle_cue_group::CUE_ACTOR_STATE_SKIP`].
pub const IMPACT_NEUTRAL_STATE: u32 = 0x2008_0200;

/// Per-frame ease step of the tint decay, in colour-lane units: the
/// presentation tick calls the ease with step byte `1`, and the primitive
/// scales it by `dt * 8` (`FUN_80050F30` `iVar2 * 8`; `dt = 1` per engine
/// tick).
pub const IMPACT_EASE_STEP: u32 = 8;

/// Host-side cue strength for the impact tint: the engine hosts render
/// the tint as a flat blend toward the unpacked RGB on the saturated
/// depth-cue seam (the same seam the target cursor and the hit flash
/// ride), rather than retail's multiplicative mesh-colour word - a
/// disclosed presentation approximation. Shared by both hosts so the
/// blend cannot drift.
pub const IMPACT_TINT_CUE_STRENGTH: f32 = 0.6;

/// One clip-impact arm's windowed target writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipImpactWrite {
    /// `+0x21D = 0` on the target - the pose freeze (Gala's arm only).
    pub freeze_target: bool,
    /// The 1-based impact-config selector written to the target's `+0x21F`
    /// (tint word = impact table entry `selector - 1`).
    pub impact_selector: u8,
}

/// Cursor windows, in the retail 12.4 sixteenths-of-a-keyframe unit
/// (`MonsterAnimPlayer::cursor_sixteenths` engine-side).
pub const GALA_FREEZE_WINDOW: core::ops::RangeInclusive<u16> = 0x40..=0x80;
/// Vahn's tint-only window.
pub const VAHN_TINT_WINDOW: core::ops::RangeInclusive<u16> = 0x90..=0xA0;

/// The clip-identity byte both `0x18` arms compare (`record[+0x77]`).
pub const IMPACT_CLIP_KEY: u8 = 0x18;

/// Resolve the windowed target write for one acting frame. `character_id`
/// is the retail roster id space (1 = Vahn, 2 = Noa, 3 = Gala),
/// `attach_key` the committed record's `+0x77` byte, `cursor` the anim
/// cursor in sixteenths. `None` = no arm fires this frame.
pub fn clip_impact(character_id: u8, attach_key: u8, cursor: u16) -> Option<ClipImpactWrite> {
    if attach_key != IMPACT_CLIP_KEY {
        return None;
    }
    match character_id {
        3 if GALA_FREEZE_WINDOW.contains(&cursor) => Some(ClipImpactWrite {
            freeze_target: true,
            impact_selector: 2,
        }),
        1 if VAHN_TINT_WINDOW.contains(&cursor) => Some(ClipImpactWrite {
            freeze_target: false,
            impact_selector: 1,
        }),
        _ => None,
    }
}

/// The acting actor's own `+0x21F` stamp: Gala's arm writes `2` on the
/// tag match alone, before (and regardless of) the cursor window
/// (`0x8004D1B4`). `None` for every other character/tag pair.
pub fn clip_impact_acting_selector(character_id: u8, attach_key: u8) -> Option<u8> {
    (character_id == 3 && attach_key == IMPACT_CLIP_KEY).then_some(2)
}

/// Unpack a 10:10:10 actor colour-state word into RGB bytes - the
/// `FUN_8004A908` lane decode (`lane >> 2`, `0x8004AA24..0x8004AA50`),
/// saturating lanes above `0x3FC`.
pub fn unpack_actor_state_rgb(word: u32) -> [u8; 3] {
    let lane = |n: u32| (((word >> (10 * n)) & 0x3FF) >> 2).min(0xFF) as u8;
    [lane(0), lane(1), lane(2)]
}

/// Ease each 10-bit lane of `word` toward `target_rgb[lane] << 2` by
/// `step_lanes`, clamping at the target - the `FUN_80050F30` primitive.
/// The presentation tick's neutral decay is
/// `ease_actor_state(w, [0x80; 3], IMPACT_EASE_STEP)`.
pub fn ease_actor_state(word: u32, target_rgb: [u8; 3], step_lanes: u32) -> u32 {
    // The top two bits ride along untouched (`FUN_80050F30` masks each
    // lane back in and never clears them).
    let mut out = word & 0xC000_0000;
    for n in 0..3u32 {
        let cur = (word >> (10 * n)) & 0x3FF;
        let tgt = u32::from(target_rgb[n as usize]) << 2;
        let next = if cur < tgt {
            (cur + step_lanes).min(tgt)
        } else {
            cur.saturating_sub(step_lanes).max(tgt)
        };
        out |= next << (10 * n);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_two_retail_arms_fire() {
        // Gala in-window: freeze + entry 2.
        let g = clip_impact(3, 0x18, 0x60).expect("Gala window");
        assert!(g.freeze_target);
        assert_eq!(g.impact_selector, 2);
        // Window edges are inclusive (`addiu -0x40; sltiu 0x41`).
        assert!(clip_impact(3, 0x18, 0x40).is_some());
        assert!(clip_impact(3, 0x18, 0x80).is_some());
        assert!(clip_impact(3, 0x18, 0x3F).is_none());
        assert!(clip_impact(3, 0x18, 0x81).is_none());
        // Vahn in-window: tint only, entry 1.
        let v = clip_impact(1, 0x18, 0x98).expect("Vahn window");
        assert!(!v.freeze_target);
        assert_eq!(v.impact_selector, 1);
        assert!(clip_impact(1, 0x18, 0x8F).is_none());
        // Noa has no 0x18 arm; other tags fire nothing.
        assert!(clip_impact(2, 0x18, 0x60).is_none());
        assert!(clip_impact(3, 0x17, 0x60).is_none());
        // Gala's acting-side stamp is windowless.
        assert_eq!(clip_impact_acting_selector(3, 0x18), Some(2));
        assert_eq!(clip_impact_acting_selector(3, 0x18), Some(2));
        assert_eq!(clip_impact_acting_selector(1, 0x18), None);
        assert_eq!(clip_impact_acting_selector(3, 0x17), None);
    }

    #[test]
    fn the_lane_decode_is_the_draw_resolver_shift() {
        assert_eq!(unpack_actor_state_rgb(IMPACT_NEUTRAL_STATE), [0x80; 3]);
        // Lane order: 0 = R, 1 = G, 2 = B.
        assert_eq!(unpack_actor_state_rgb(0x3FF), [0xFF, 0, 0]);
        assert_eq!(unpack_actor_state_rgb(0x3FF << 10), [0, 0xFF, 0]);
        assert_eq!(unpack_actor_state_rgb(0x3FF << 20), [0, 0, 0xFF]);
    }

    #[test]
    fn the_ease_converges_to_neutral_and_clamps() {
        // A saturated red word eases down 8 lanes per step, clamping at
        // the neutral lane value on each channel independently.
        let mut w = 0x3FF;
        w = ease_actor_state(w, [0x80; 3], IMPACT_EASE_STEP);
        assert_eq!(w & 0x3FF, 0x3F7);
        // Below-neutral lanes climb.
        let up = ease_actor_state(0, [0x80; 3], IMPACT_EASE_STEP);
        assert_eq!(up & 0x3FF, 8);
        // Iterating reaches exactly neutral and stays there.
        let mut w = 0x3FF | (0x100 << 10);
        for _ in 0..0x100 {
            w = ease_actor_state(w, [0x80; 3], IMPACT_EASE_STEP);
        }
        assert_eq!(w, IMPACT_NEUTRAL_STATE);
        assert_eq!(
            ease_actor_state(w, [0x80; 3], IMPACT_EASE_STEP),
            w,
            "neutral is a fixed point"
        );
    }
}
