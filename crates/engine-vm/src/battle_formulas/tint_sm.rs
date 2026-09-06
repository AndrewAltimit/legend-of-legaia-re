//! The per-actor battle **presentation tint state machine** -
//! `FUN_80050120`'s per-actor arms, keyed on the actor state byte
//! `actor[+0x21C]` (11-entry jump table at `0x8001532C`).
//!
//! PORT: FUN_80050120 (the per-actor arms; the trailing backdrop far-colour /
//! depth-cue ramp block at `0x800505B0..0x800508B8` is the scene fog, not an
//! actor state, and stays with the renderer)
//!
//! Every frame the battle tick walks the eight actor slots (`DAT_801C9370`),
//! skips an actor with no `+0x22C` battle record, loads the packed 10:10:10
//! colour word `+0x04`, and dispatches on `+0x21C`. Read from the
//! disassembly (`ghidra/scripts/funcs/80050120.txt`):
//!
//! | `+0x21C` | Arm | Colour target | Ease step | `+0x0C` after |
//! |---|---|---|---|---|
//! | `0` | idle / decay | `0x80,0x80,0x80` (neutral) | `1` | drained once neutral (see below) |
//! | `1` | dim | `0x20,0x20,0x20` | `4` | `0x1000` |
//! | `2` | defeat fade | lanes step toward `0` (no ease call) | `8/dt`-lane | `0x1000` |
//! | `3` | red | `0xFF,0x00,0x00` | `2` | `0x1000` |
//! | `4` | blue | `0x00,0x00,0xFF` | `2` | `0x1000` |
//! | `5` | (hold) | untouched | - | untouched |
//! | `6` | magenta | `0xF0,0x20,0xF0` | `2` | `0x1000` |
//! | `7` | red (soft) | `0xF0,0x00,0x00` | `1` | `0x1000` |
//! | `8` | green | `0x00,0xF0,0x00` | `1` | `0x1000` |
//! | `9` | yellow | `0xFF,0xFF,0x00` | `1` | `0x1000` |
//! | `10` | white | `0xFF,0xFF,0xFF` | `1` | `0x1000` |
//! | `>= 11` | (hold) | untouched | - | untouched |
//!
//! The ease is [`super::actor_tween::packed3_approach_target`]
//! (`FUN_80050F30`): each lane moves at most `step * dt * 8` toward
//! `target << 2` per frame. The arm-0 tail (`0x800501C0..0x80050210`): once
//! the eased word **is** the neutral `0x20080200`, a non-zero `+0x0C`
//! tint-blend intensity drains by `dt * 0x20` (floored at `0`), and only a
//! zero `+0x0C` clears the `+0x21F` impact selector. So a hit's tint runs:
//! colour eases back to neutral, then the blend drains, then the selector
//! retires - three phases, in that order, one word at a time.
//!
//! Arm 2 (`0x80050230..0x80050360`) is the defeat / capture fade: `+0x0C =
//! 0x1000`, the mode word `+0x08 |= 0x81000000`, and - when the committed
//! anim is the idle (`+0x1D9 == 0`), the knockdown entry (`== +0x1F1`), the
//! party slot-8 clip, or the actor is captured (`+0x225 != 0`) - each lane
//! steps `dt * 8` toward zero. A party seat whose word reaches `0` then
//! resets `+0x21C = 0`, `+0x1DA = 0`, `+0x1DC = 1`, `+0x0C = 0x800` (the
//! "fade done" edge); monster seats `3..=6` with a non-zero record colour
//! run the capture / no-escape bookkeeping and the sink (`+0x36 +=
//! (monster[+0x1F] * dt) >> 2`). This kernel ports the fade and reports the
//! party fade-done edge; the monster bookkeeping is the action SM's
//! (`crate::battle_action`), not a colour law.
//!
//! The frame delta `dt` is the scratchpad byte `DAT_1F800393`; the engine
//! ticks one retail frame per tick, so callers pass `1`.

use super::actor_tween::packed3_approach_target;

/// The neutral packed word every arm converges on (all lanes `0x80`).
pub const TINT_NEUTRAL: u32 = 0x2008_0200;

/// `+0x0C` value the colour arms stamp (`s3` in `FUN_80050120`).
pub const TINT_BLEND_FULL: u32 = 0x1000;

/// `+0x0C` value the party defeat-fade completion stamps.
pub const TINT_BLEND_FADE_DONE: u32 = 0x800;

/// Per-frame drain of `+0x0C` in arm 0 once neutral (`dt << 5`).
pub const TINT_BLEND_DRAIN: u32 = 0x20;

/// Per-lane per-frame step of the arm-2 fade (`dt << 3`, in lane units).
pub const FADE_LANE_STEP: u32 = 8;

/// State-byte values with a colour arm (the jump table's live rows).
pub const STATE_DECAY: u8 = 0;
pub const STATE_DIM: u8 = 1;
pub const STATE_DEFEAT_FADE: u8 = 2;
pub const STATE_RED: u8 = 3;
pub const STATE_BLUE: u8 = 4;
pub const STATE_HOLD: u8 = 5;
pub const STATE_MAGENTA: u8 = 6;
pub const STATE_RED_SOFT: u8 = 7;
pub const STATE_GREEN: u8 = 8;
pub const STATE_YELLOW: u8 = 9;
pub const STATE_WHITE: u8 = 10;

/// `(target rgb, ease step)` for the fixed-colour arms, `None` for the arms
/// that are not a plain ease (`0` decay, `2` fade) or hold the word.
pub fn colour_arm(state: u8) -> Option<([u8; 3], u8)> {
    Some(match state {
        STATE_DIM => ([0x20, 0x20, 0x20], 4),
        STATE_RED => ([0xFF, 0x00, 0x00], 2),
        STATE_BLUE => ([0x00, 0x00, 0xFF], 2),
        STATE_MAGENTA => ([0xF0, 0x20, 0xF0], 2),
        STATE_RED_SOFT => ([0xF0, 0x00, 0x00], 1),
        STATE_GREEN => ([0x00, 0xF0, 0x00], 1),
        STATE_YELLOW => ([0xFF, 0xFF, 0x00], 1),
        STATE_WHITE => ([0xFF, 0xFF, 0xFF], 1),
        _ => return None,
    })
}

/// The actor words one step reads and rewrites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TintWords {
    /// `+0x04` packed 10:10:10 colour.
    pub color: u32,
    /// `+0x0C` q12 tint-blend intensity.
    pub blend: u32,
    /// `+0x21F` impact selector.
    pub selector: u8,
}

/// Arm-2 predicate inputs the caller resolves from the actor.
#[derive(Debug, Clone, Copy, Default)]
pub struct FadeInputs {
    /// Seat `< 3` (party side).
    pub party: bool,
    /// Committed anim id `+0x1D9`.
    pub committed_anim: u8,
    /// Cached knockdown entry `+0x1F1`.
    pub knockdown_entry: u8,
    /// Capture state `+0x225` is non-zero.
    pub captured: bool,
}

impl FadeInputs {
    /// The arm-2 gate (`0x80050248..0x80050280`): the lanes step toward
    /// zero when the committed anim is `8` on a party seat, `0`, or the
    /// knockdown entry, or the actor is captured.
    pub fn fades(&self) -> bool {
        (self.committed_anim == 8 && self.party)
            || self.committed_anim == 0
            || self.committed_anim == self.knockdown_entry
            || self.captured
    }
}

/// What one step changed besides the words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TintStepEffects {
    /// Arm 2 on a party seat reached word `0`: retail resets `+0x21C = 0`,
    /// `+0x1DA = 0`, `+0x1DC = 1` alongside `+0x0C = 0x800`.
    pub party_fade_done: bool,
    /// Arm 2 ran: retail ORs `0x81000000` into the mode word `+0x08`.
    pub mode_semi_transparent: bool,
}

/// One frame of the presentation tint SM for one actor.
///
/// `state` is `+0x21C`, `dt` the frame delta (`1` per engine tick).
/// Returns the rewritten words and the side edges; `state` itself is not
/// rewritten here (the only writer inside the SM is the arm-2 party
/// fade-done reset, reported as [`TintStepEffects::party_fade_done`]).
pub fn tint_sm_step(
    state: u8,
    words: TintWords,
    fade: FadeInputs,
    dt: u8,
) -> (TintWords, TintStepEffects) {
    let mut w = words;
    let mut fx = TintStepEffects::default();
    match state {
        STATE_DECAY => {
            w.color = packed3_approach_target(w.color, 0x80, 0x80, 0x80, 1, dt);
            if w.color == TINT_NEUTRAL {
                if w.blend != 0 {
                    let drain = u32::from(dt) * TINT_BLEND_DRAIN;
                    w.blend = w.blend.saturating_sub(drain);
                } else {
                    w.selector = 0;
                }
            }
        }
        STATE_DEFEAT_FADE => {
            w.blend = TINT_BLEND_FULL;
            fx.mode_semi_transparent = true;
            if fade.fades() {
                let step = u32::from(dt) * FADE_LANE_STEP;
                for lane in 0..3u32 {
                    let shift = 10 * lane;
                    let mask = 0x3FFu32 << shift;
                    let cur = w.color & mask;
                    if cur == 0 {
                        continue;
                    }
                    let dec = step << shift;
                    w.color = if dec < cur {
                        w.color - dec
                    } else {
                        w.color & !mask
                    };
                }
            }
            if fade.party && w.color == 0 {
                fx.party_fade_done = true;
                w.blend = TINT_BLEND_FADE_DONE;
            }
        }
        s => {
            if let Some((rgb, step)) = colour_arm(s) {
                w.color = packed3_approach_target(w.color, rgb[0], rgb[1], rgb[2], step, dt);
                w.blend = TINT_BLEND_FULL;
            }
        }
    }
    (w, fx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(r: u32, g: u32, b: u32) -> u32 {
        r | (g << 10) | (b << 20)
    }

    fn words(color: u32, blend: u32, selector: u8) -> TintWords {
        TintWords {
            color,
            blend,
            selector,
        }
    }

    /// Arm 0 runs the three phases in retail order: colour to neutral,
    /// then the blend drains, then the selector retires - and the selector
    /// clears on the frame the blend is already zero, not a frame later.
    #[test]
    fn decay_arm_eases_then_drains_then_clears() {
        let hit = words(pack(0x3FF, 0x100, 0x200), 0x1000, 2);
        let (w, _) = tint_sm_step(STATE_DECAY, hit, FadeInputs::default(), 1);
        // 8 lane units per frame on each off-target lane; blend + selector
        // untouched while the colour is still moving.
        assert_eq!(w.color, pack(0x3F7, 0x108, 0x200));
        assert_eq!(w.blend, 0x1000);
        assert_eq!(w.selector, 2);
        // Run to neutral.
        let mut w = hit;
        let mut frames = 0;
        while w.color != TINT_NEUTRAL {
            w = tint_sm_step(STATE_DECAY, w, FadeInputs::default(), 1).0;
            frames += 1;
            assert!(frames < 200, "never converged");
        }
        // The arrival frame already starts draining the blend.
        assert_eq!(w.blend, 0x1000 - 0x20);
        assert_eq!(w.selector, 2);
        // 0xFE0 left at 0x20 a frame: 127 more frames reach zero, and the
        // frame the blend hits zero leaves the selector alone (`sb zero,
        // 0x21f` sits only under `beq v1,zero` at `0x800501D8`).
        for _ in 0..(0x1000 / 0x20 - 1) {
            w = tint_sm_step(STATE_DECAY, w, FadeInputs::default(), 1).0;
        }
        assert_eq!(w.blend, 0);
        assert_eq!(w.selector, 2);
        // The frame after the blend hit zero clears the selector.
        w = tint_sm_step(STATE_DECAY, w, FadeInputs::default(), 1).0;
        assert_eq!(w.selector, 0);
        assert_eq!(w.color, TINT_NEUTRAL, "neutral is a fixed point");
    }

    /// A neutral word with a zero blend and a live selector retires the
    /// selector on the very first frame (`FUN_80050120` `0x8005020C`).
    #[test]
    fn decay_arm_clears_selector_at_once_when_already_neutral() {
        let (w, _) = tint_sm_step(
            STATE_DECAY,
            words(TINT_NEUTRAL, 0, 1),
            FadeInputs::default(),
            1,
        );
        assert_eq!(w.selector, 0);
    }

    /// The blend drain floors at zero rather than wrapping
    /// (`sltu v0,a0,v1` at `0x800501F0`).
    #[test]
    fn blend_drain_floors_at_zero() {
        let (w, _) = tint_sm_step(
            STATE_DECAY,
            words(TINT_NEUTRAL, 0x10, 3),
            FadeInputs::default(),
            1,
        );
        assert_eq!(w.blend, 0);
        assert_eq!(w.selector, 3, "the selector waits for the next frame");
        // A larger frame delta drains proportionally.
        let (w, _) = tint_sm_step(
            STATE_DECAY,
            words(TINT_NEUTRAL, 0x100, 3),
            FadeInputs::default(),
            2,
        );
        assert_eq!(w.blend, 0x100 - 0x40);
    }

    /// Every fixed-colour arm eases toward its own target at its own step
    /// and stamps the full blend.
    #[test]
    fn colour_arms_ease_toward_their_targets() {
        for (state, rgb, step) in [
            (STATE_DIM, [0x20u8, 0x20, 0x20], 4u32),
            (STATE_RED, [0xFF, 0x00, 0x00], 2),
            (STATE_BLUE, [0x00, 0x00, 0xFF], 2),
            (STATE_MAGENTA, [0xF0, 0x20, 0xF0], 2),
            (STATE_RED_SOFT, [0xF0, 0x00, 0x00], 1),
            (STATE_GREEN, [0x00, 0xF0, 0x00], 1),
            (STATE_YELLOW, [0xFF, 0xFF, 0x00], 1),
            (STATE_WHITE, [0xFF, 0xFF, 0xFF], 1),
        ] {
            let (w, _) = tint_sm_step(state, words(TINT_NEUTRAL, 0, 0), FadeInputs::default(), 1);
            assert_eq!(w.blend, TINT_BLEND_FULL, "state {state}");
            let max = step * 8;
            for lane in 0..3 {
                let cur = (w.color >> (10 * lane)) & 0x3FF;
                let tgt = u32::from(rgb[lane as usize]) << 2;
                let expect = if tgt > 0x200 {
                    (0x200 + max).min(tgt)
                } else if tgt < 0x200 {
                    (0x200 - max).max(tgt)
                } else {
                    0x200
                };
                assert_eq!(cur, expect, "state {state} lane {lane}");
            }
            // Converges to exactly the target.
            let mut w = w;
            for _ in 0..0x400 {
                w = tint_sm_step(state, w, FadeInputs::default(), 1).0;
            }
            assert_eq!(
                w.color & 0x3FFF_FFFF,
                pack(
                    u32::from(rgb[0]) << 2,
                    u32::from(rgb[1]) << 2,
                    u32::from(rgb[2]) << 2
                ),
                "state {state} target"
            );
        }
    }

    /// Arm 5 and the out-of-table values (the summon-hide `0xFF`, the cursor
    /// `200`) leave every word alone.
    #[test]
    fn hold_states_leave_the_words_alone() {
        for state in [STATE_HOLD, 11, 200, 0xFF] {
            let w0 = words(pack(0x3FF, 0x100, 0x010), 0x1234, 4);
            let (w, fx) = tint_sm_step(state, w0, FadeInputs::default(), 1);
            assert_eq!(w, w0, "state {state}");
            assert_eq!(fx, TintStepEffects::default());
        }
    }

    /// Arm 2: the lanes step 8 toward zero only under the fade gate, the
    /// blend is stamped either way, and a party seat reaching black raises
    /// the fade-done edge with the `0x800` blend.
    #[test]
    fn defeat_fade_steps_lanes_to_black_and_signals_party_done() {
        let gated = FadeInputs {
            party: true,
            committed_anim: 0,
            knockdown_entry: 4,
            captured: false,
        };
        let (w, fx) = tint_sm_step(
            STATE_DEFEAT_FADE,
            words(pack(0x10, 0x200, 0x004), 0, 0),
            gated,
            1,
        );
        assert_eq!(w.color, pack(0x8, 0x1F8, 0));
        assert_eq!(w.blend, TINT_BLEND_FULL);
        assert!(fx.mode_semi_transparent);
        assert!(!fx.party_fade_done);
        // Ungated (a party actor mid-swing): the word holds, the blend is
        // still stamped.
        let held = FadeInputs {
            party: true,
            committed_anim: 0xC,
            knockdown_entry: 4,
            captured: false,
        };
        let (w, fx) = tint_sm_step(
            STATE_DEFEAT_FADE,
            words(pack(0x10, 0x200, 0x004), 0, 0),
            held,
            1,
        );
        assert_eq!(w.color, pack(0x10, 0x200, 0x004));
        assert_eq!(w.blend, TINT_BLEND_FULL);
        assert!(!fx.party_fade_done);
        // The knockdown entry and the capture byte open the gate too.
        assert!(
            FadeInputs {
                committed_anim: 4,
                ..held
            }
            .fades()
        );
        assert!(
            FadeInputs {
                captured: true,
                ..held
            }
            .fades()
        );
        // Party seat at black: fade-done edge + 0x800 blend.
        let (w, fx) = tint_sm_step(STATE_DEFEAT_FADE, words(pack(0x4, 0, 0), 0, 0), gated, 1);
        assert_eq!(w.color, 0);
        assert!(fx.party_fade_done);
        assert_eq!(w.blend, TINT_BLEND_FADE_DONE);
        // A monster seat at black does not raise the party edge.
        let monster = FadeInputs {
            party: false,
            ..gated
        };
        let (w, fx) = tint_sm_step(STATE_DEFEAT_FADE, words(0, 0, 0), monster, 1);
        assert_eq!(w.color, 0);
        assert!(!fx.party_fade_done);
        assert_eq!(w.blend, TINT_BLEND_FULL);
    }

    /// The slot-8 clip fades only on a party seat (`bne v1,v0 ... sltiu
    /// v0,s2,0x3` at `0x8005024C..0x80050254`).
    #[test]
    fn party_slot_eight_gate_is_party_only() {
        let base = FadeInputs {
            party: true,
            committed_anim: 8,
            knockdown_entry: 4,
            captured: false,
        };
        assert!(base.fades());
        assert!(
            !FadeInputs {
                party: false,
                ..base
            }
            .fades()
        );
    }
}
