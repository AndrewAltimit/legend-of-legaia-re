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
//! `restore_action_anim_rates`). The tint decays through the per-actor
//! presentation tick `FUN_80050120` arm 0 (`+0x21C == 0`): each frame the
//! packed word eases per-lane toward the neutral `0x20080200`
//! ([`ease_actor_state`] with target `(0x80, 0x80, 0x80)` and step 1);
//! once neutral, the `+0x0C` intensity word drains by `dt << 5` and the
//! `+0x21F` selector clears. The whole machine - that arm and the ten
//! others the jump table at `0x8001532C` reaches - is
//! `crate::battle_formulas::tint_sm_step`; [`ease_actor_state`] is the
//! same ease kept as the arms' shared primitive.
//!
//! # How the tint reaches the pixel
//!
//! The tint pass `FUN_8004A908` packs the `+0x04` lanes (`>> 2`) into the
//! render node's `+0x74` and copies `+0x0C` into `+0x78` whenever it is
//! non-zero (`0x8004AA24..0x8004AA70`); the draw pass `FUN_80048A08`
//! stages those two as the GTE far colour and `IR0` (`gp[0x9D8]` /
//! `gp[0x9DC]`, `0x80048BEC..0x80048C00`). So a hit does not paint a flat
//! silhouette: the prim's modulation colour becomes `baked + (tint -
//! baked) * blend / 0x1000` and the GPU still multiplies the texel through
//! it (`texel * colour / 128`). Retail capture: the Tail-Fire-struck Vahn
//! in `battle_gimard_tail_fire_a` (`+0x04 = (0xC7, 0x38, 0x38)`, `+0x0C =
//! 0x1000`) reads red 160..248 / green-blue 8..80 across his texture, not
//! one colour. Both hosts render exactly that: far = the unpacked lanes,
//! `IR0 = blend / 0x1000`, through the saturated per-draw depth-cue seam.
//!
//! # The rest of the clip-tag pass
//!
//! The same pass (`0x8004D01C..0x8004D32C`) carries more arms than the two
//! freezes, all keyed the same way (committed record `+0x77`, anim-player
//! node cursor `+0x68`, target = acting `+0x1DD`):
//!
//! * **Gala, tags `0x16` / `0x17`** (`0x8004D124..0x8004D18C`): the same
//!   entry-1 tint on the target (selector `2`, blend `0x1000`), no freeze,
//!   from cursor `0x20` on (tag `0x16`, `slti v0,v0,0x20` at `0x8004D14C`) or
//!   `0x40` on (tag `0x17`, `0x8004D168`), open-ended.
//! * **Gala, tag `0x67`** (`0x8004D1E8..0x8004D248`): the entry-1 tint in the
//!   window `0xB0..=0xF0`, plus a call `FUN_801E1D98(&target[+0x3C], 0xC)` on
//!   every in-window frame ([`ClipImpactWrite::effect_at_target`]).
//! * **Vahn, tag `0x2B`** (`0x8004D2A4..0x8004D2D8`): the **acting** actor's
//!   presentation-arm byte `+0x21C` is `3` below cursor `0x51` and `0`
//!   from there on.
//! * **Noa, tags `0x29` / `0x2D`** (`0x8004D0A4..0x8004D120`): once the clip
//!   has landed a hit (acting `+0x1F4 != 0`), outside a scripted fight
//!   (`ctx[+0x287] == 0`), on an even `rand()` and unless the formation's
//!   first monster is `0xA7` (`gp+0x9F4` = `0x8007BD0C`), the target's status
//!   word `+0x16E` takes `0x380` ([`noa_status_arm`]).
//! * **A monster, tag `0x3B`** (`0x8004D2DC..0x8004D32C`, acting slot `>= 3`):
//!   while the acting actor's `+0x21B` reads `0x13` its `+0x21C` is `3` and
//!   the target's `4`; when `+0x21B` reads `0` both go back to `0`
//!   ([`monster_render_arm`]).

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

/// Per-frame drain step of the actor `+0xC` tint-blend intensity
/// (`render_blend`): `FUN_80050120` arm 0 subtracts `dt * 0x20` once the
/// colour word has eased back to [`IMPACT_NEUTRAL_STATE`], clamping at
/// `0` (`dt = 1` per engine tick). This is what retires the item/spirit
/// cue-group flash (`+0x0C = 0x2000`, `FUN_801E22C8`).
pub const BLEND_DRAIN_STEP: u32 = 0x20;

/// The `IR0` both hosts stage for an actor's tint: the `+0x0C` blend as a
/// `1.0 = 0x1000` factor, truncated to the halfword `FUN_8004A908` copies
/// (`lhu v0,0xc(s1)` / `sh v0,0x0(s4)` at `0x8004AA68..0x8004AA70`) and
/// **not** saturated - the cue-group flash's `0x2000` extrapolates past the
/// far colour until the DPCS output clamp bounds it, exactly as a bare
/// `mtc2` load does. `0` = no tint staged (the depth-dimming branch of the
/// tint pass, which the port does not model).
pub fn tint_ir0(render_blend: u32) -> f32 {
    (render_blend & 0xFFFF) as f32 / 4096.0
}

/// One clip-impact arm's windowed target writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipImpactWrite {
    /// `+0x21D = 0` on the target - the pose freeze (Gala's arm only).
    pub freeze_target: bool,
    /// The 1-based impact-config selector written to the target's `+0x21F`
    /// (tint word = impact table entry `selector - 1`).
    pub impact_selector: u8,
    /// The tag-`0x67` arm's `FUN_801E1D98(&target[+0x3C], 0xC)` call - an
    /// effect raised at the target's seat every in-window frame.
    pub effect_at_target: bool,
}

/// Gala's open-ended tint-only arms: tag `0x16` from cursor `0x20`, tag
/// `0x17` from cursor `0x40`.
pub const GALA_TINT_TAGS: [(u8, u16); 2] = [(0x16, 0x20), (0x17, 0x40)];
/// Gala's tag-`0x67` window (`slti 0xB0` / `slti 0xF1`).
pub const GALA_EFFECT_TAG: u8 = 0x67;
pub const GALA_EFFECT_WINDOW: core::ops::RangeInclusive<u16> = 0xB0..=0xF0;
/// The `FUN_801E1D98` argument the tag-`0x67` arm passes (`a1 = 0xC`).
pub const GALA_EFFECT_ARG: u8 = 0x0C;
/// Vahn's `+0x21C` arm: tag and the cursor it flips at.
pub const VAHN_RENDER_TAG: u8 = 0x2B;
pub const VAHN_RENDER_CURSOR_END: u16 = 0x51;
/// Noa's status arm: its two tags, the monster id that vetoes it, the bits.
pub const NOA_STATUS_TAGS: [u8; 2] = [0x29, 0x2D];
pub const NOA_STATUS_VETO_MONSTER: u8 = 0xA7;
pub const NOA_STATUS_BITS: u16 = 0x0380;
/// The monster arm's tag and its `+0x21B` trigger value.
pub const MONSTER_RENDER_TAG: u8 = 0x3B;
pub const MONSTER_RENDER_TRIGGER: u8 = 0x13;

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
    let tint = |selector: u8| ClipImpactWrite {
        freeze_target: false,
        impact_selector: selector,
        effect_at_target: false,
    };
    // The cursor is a signed halfword on the `lh` arms; a negative one never
    // reaches a window.
    let signed = cursor as i16;
    match (character_id, attach_key) {
        (3, IMPACT_CLIP_KEY) if GALA_FREEZE_WINDOW.contains(&cursor) => Some(ClipImpactWrite {
            freeze_target: true,
            ..tint(2)
        }),
        (1, IMPACT_CLIP_KEY) if VAHN_TINT_WINDOW.contains(&cursor) => Some(tint(1)),
        (3, k) => {
            if let Some(&(_, from)) = GALA_TINT_TAGS.iter().find(|(t, _)| *t == k) {
                return (signed >= from as i16).then(|| tint(2));
            }
            if k == GALA_EFFECT_TAG && signed >= 0 && GALA_EFFECT_WINDOW.contains(&cursor) {
                return Some(ClipImpactWrite {
                    effect_at_target: true,
                    ..tint(2)
                });
            }
            None
        }
        _ => None,
    }
}

/// Vahn's tag-`0x2B` arm: the value written to the **acting** actor's
/// `+0x21C` this frame, or `None` off the tag.
pub fn vahn_render_arm(character_id: u8, attach_key: u8, cursor: u16) -> Option<u8> {
    let flag = if (cursor as i16) < VAHN_RENDER_CURSOR_END as i16 {
        3
    } else {
        0
    };
    (character_id == 1 && attach_key == VAHN_RENDER_TAG).then_some(flag)
}

/// A monster's tag-`0x3B` arm: `(acting +0x21C, target +0x21C)` to write
/// this frame, or `None`. `hit_bound` is the acting actor's `+0x21B`.
pub fn monster_render_arm(attach_key: u8, hit_bound: u8) -> Option<(u8, u8)> {
    if attach_key != MONSTER_RENDER_TAG {
        return None;
    }
    match hit_bound {
        MONSTER_RENDER_TRIGGER => Some((3, 4)),
        0 => Some((0, 0)),
        _ => None,
    }
}

/// Noa's tag-`0x29` / `0x2D` arm: `true` when the target's `+0x16E` takes
/// [`NOA_STATUS_BITS`] this frame. `rand` is drawn only once every other gate
/// has passed, matching the `jal 0x80056798` at `0x8004D0EC`; the draw's low
/// bit must be clear.
pub fn noa_status_arm(
    character_id: u8,
    attach_key: u8,
    hit_index: u8,
    scripted_fight: bool,
    first_monster: u8,
    rand: impl FnOnce() -> u32,
) -> bool {
    if character_id != 2 || !NOA_STATUS_TAGS.contains(&attach_key) {
        return false;
    }
    if hit_index == 0 || scripted_fight {
        return false;
    }
    rand() & 1 == 0 && first_monster != NOA_STATUS_VETO_MONSTER
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

    /// The staged IR0 is the halfword blend over `0x1000`, unsaturated:
    /// the hit triple's `0x1000` is exactly `1.0`, the cue-group flash's
    /// `0x2000` extrapolates to `2.0`, and a high-half bit the `lhu` never
    /// sees is dropped.
    #[test]
    fn tint_ir0_is_the_halfword_blend_over_0x1000() {
        assert_eq!(tint_ir0(0), 0.0);
        assert_eq!(tint_ir0(0x1000), 1.0);
        assert_eq!(tint_ir0(0x0800), 0.5);
        assert_eq!(tint_ir0(0x2000), 2.0);
        assert_eq!(tint_ir0(0x1_0800), 0.5);
    }

    #[test]
    fn the_two_freeze_arms_and_their_windows() {
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
        // Noa has no 0x18 arm.
        assert!(clip_impact(2, 0x18, 0x60).is_none());
        // Gala's acting-side stamp is windowless.
        assert_eq!(clip_impact_acting_selector(3, 0x18), Some(2));
        assert_eq!(clip_impact_acting_selector(3, 0x18), Some(2));
        assert_eq!(clip_impact_acting_selector(1, 0x18), None);
        assert_eq!(clip_impact_acting_selector(3, 0x17), None);
    }

    #[test]
    fn galas_tint_only_arms_open_at_their_cursor_and_never_close() {
        for (tag, from) in GALA_TINT_TAGS {
            assert!(clip_impact(3, tag, from - 1).is_none());
            let w = clip_impact(3, tag, from).expect("opens at its cursor");
            assert!(!w.freeze_target && !w.effect_at_target);
            assert_eq!(w.impact_selector, 2);
            assert!(clip_impact(3, tag, 0x7FFF).is_some(), "open-ended");
            assert!(clip_impact(1, tag, from).is_none(), "Gala only");
        }
        let e = clip_impact(3, GALA_EFFECT_TAG, 0xB0).expect("0x67 window");
        assert!(e.effect_at_target);
        assert!(clip_impact(3, GALA_EFFECT_TAG, 0xF0).is_some());
        assert!(clip_impact(3, GALA_EFFECT_TAG, 0xF1).is_none());
        assert!(clip_impact(3, GALA_EFFECT_TAG, 0xAF).is_none());
    }

    #[test]
    fn the_render_and_status_arms() {
        assert_eq!(vahn_render_arm(1, 0x2B, 0x50), Some(3));
        assert_eq!(vahn_render_arm(1, 0x2B, 0x51), Some(0));
        assert_eq!(vahn_render_arm(3, 0x2B, 0x10), None);
        assert_eq!(monster_render_arm(0x3B, 0x13), Some((3, 4)));
        assert_eq!(monster_render_arm(0x3B, 0), Some((0, 0)));
        assert_eq!(monster_render_arm(0x3B, 5), None);
        assert_eq!(monster_render_arm(0x3A, 0x13), None);
        assert!(noa_status_arm(2, 0x29, 1, false, 0x10, || 2));
        assert!(!noa_status_arm(2, 0x2D, 1, false, 0x10, || 1), "odd draw");
        assert!(!noa_status_arm(2, 0x29, 0, false, 0x10, || 0), "no hit yet");
        assert!(!noa_status_arm(2, 0x29, 1, true, 0x10, || 0), "scripted");
        assert!(!noa_status_arm(2, 0x29, 1, false, 0xA7, || 0), "vetoed");
        // No draw off the tag.
        assert!(!noa_status_arm(2, 0x18, 1, false, 0x10, || panic!("drew")));
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
