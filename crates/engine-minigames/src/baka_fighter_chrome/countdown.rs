//! The READY / FIGHT countdown.

use super::*;

// ---------------------------------------------------------------------------
// READY / FIGHT countdown
// ---------------------------------------------------------------------------

/// The countdown's own gate: the banner brightness (`DAT_801DBEB4`) must reach
/// this before the timer starts decaying.
pub const COUNTDOWN_FADE_GATE: i32 = 0x11;
/// Timer seeded when the countdown enters state 2.
pub const COUNTDOWN_TIMER: i32 = 0x20;
/// The value of the round counter (`DAT_801DC110`) that marks the final round.
pub const COUNTDOWN_FINAL_ROUND: i32 = 0x0E;

/// READY/FIGHT countdown state (`DAT_801DC134` / `DAT_801DC138`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Countdown {
    /// `DAT_801DC134`: `0` fresh, `1` after the first line, `2` counting,
    /// `3` done.
    pub state: i32,
    /// `DAT_801DC138`: frames left before the final line.
    pub timer: i32,
}

/// PORT: FUN_801D21FC - the READY/FIGHT countdown.
///
/// `frame_step` is the per-frame tick (`DAT_1F800393`), `loading` the scene
/// load flag `_DAT_8007BC20`, `banner_level` / `title_level` the two banner
/// brightness globals (`DAT_801DBEB4` / `DAT_801DBEB0`), and `round_counter`
/// the round global `DAT_801DC110`.
///
/// While `loading` is clear the state advances: `0 -> 1` fires `XA33`
/// channel `0x0A`; `1 -> 2` fires channel `0x0B` and seeds
/// [`COUNTDOWN_TIMER`]; state `2` waits for `banner_level` to reach
/// [`COUNTDOWN_FADE_GATE`], then decays the timer by `frame_step` and, on
/// running out, fires channel `0x0D` on the final round (`round_counter ==
/// 0x0E`) or `0x0C` otherwise and settles in state `3`.
///
/// Every frame - loading or not - it draws the two banner widgets at half
/// their level: `0x1A` at `(0xA0, 0x60)` and, at `(0xA0, 0xA0)`, `0x1D` on
/// the final round or `0x1C` otherwise.
pub fn countdown_frame(
    st: &mut Countdown,
    frame_step: i32,
    loading: bool,
    banner_level: i32,
    title_level: i32,
    round_counter: i32,
) -> ChromeFrame {
    let mut out = ChromeFrame::default();
    let final_round = round_counter == COUNTDOWN_FINAL_ROUND;

    if st.state == 0 {
        st.state = 1;
        out.xa = Some(COUNTDOWN_ARM_XA);
    }
    if !loading {
        if st.state == 1 {
            st.state = 2;
            st.timer = COUNTDOWN_TIMER;
            out.xa = Some(COUNTDOWN_LOADED_XA);
        }
        if st.state == 2 && banner_level >= COUNTDOWN_FADE_GATE {
            st.timer -= frame_step;
            if st.timer < 0 {
                st.timer = 0;
                st.state = 3;
                out.xa = Some(if final_round {
                    COUNTDOWN_FINAL_XA
                } else {
                    COUNTDOWN_ROUND_XA
                });
            }
        }
    }

    out.draws.push(ChromeDraw::plain(
        0x1A,
        0xA0,
        0x60,
        half(title_level),
        0x1000,
    ));
    out.draws.push(ChromeDraw::plain(
        if final_round { 0x1D } else { 0x1C },
        0xA0,
        0xA0,
        half(banner_level),
        0x1000,
    ));
    out
}
