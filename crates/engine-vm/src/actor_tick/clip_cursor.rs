//! The clip cursor's step, its end timing and the per-tick advance.

/// `actor+0x62` bit `0x0002` - **hold**: the tick skips the cursor advance.
pub const CLIP_HOLD: u16 = 0x0002;
/// `actor+0x62` bit `0x0008` - **clamp**: the cursor stops at the end it
/// reaches (a one-shot); clear, it wraps (a loop).
pub const CLIP_CLAMP: u16 = 0x0008;
/// `actor+0x62` bit `0x0080` - **reverse**: the cursor counts down.
pub const CLIP_REVERSE: u16 = 0x0080;
/// `actor+0x62` bit `0x0100` - the **end latch**, raised by the tick on the
/// frame the cursor reaches either end; scripts spin on it (`AD <id> 08`).
pub const CLIP_END: u16 = 0x0100;
/// `actor+0x62` bit `0x0200` - **restart request**, consumed by the next tick.
pub const CLIP_RESTART: u16 = 0x0200;
/// The bits of `actor+0x62` a script sets to steer the cursor (everything the
/// tick reads but does not itself raise).
pub const CLIP_CONTROL_BITS: u16 = CLIP_HOLD | CLIP_CLAMP | CLIP_REVERSE | CLIP_RESTART;

/// The cursor step one retail clip tick adds, in 1/16-frame units, before the
/// frame-step scalar `DAT_1F800393` multiplies it (the port's tick is one
/// vsync, so that scalar is `1`).
///
/// A clip whose header carries the blend gate (clip byte `+1` bit 0 - the
/// same bit the frame blender tests) takes the **scaled** step
/// `(rate * 2 + div - 1) / div`, `div` being clip byte `+6`
/// (`0x800205C8..0x800205E0`); every other clip steps by the rate itself. At
/// the field rate `8` (`legaia_engine_field::field_anim::CLIP_RATE`) that is
/// `16 / 8 / 6 / 4 / 3` for divisors `1 / 2 / 3 / 4 / 6`, so the divisor-4 clips (the disc's most common gated shape) play at half
/// the ungated speed, their in-between ticks posed by the blender.
///
/// A gated clip with divisor `0` would divide by zero in retail; no record on
/// the disc has one, and the port falls back to the rate.
///
/// PORT: FUN_800204F8 (the step select at `0x800205B4..0x800205EC`)
pub fn clip_step(rate: u16, gated: bool, div: u8) -> u16 {
    if gated && div != 0 {
        // Retail's `(rate * 2 + div - 1) / div`: a ceiling division.
        (u32::from(rate) * 2).div_ceil(u32::from(div)) as u16
    } else {
        rate
    }
}

/// Ticks from a clip's bind (cursor `0`) to the tick that latches its end
/// flag `0x100`: the first tick whose cursor reaches `frames * 16 - 1`
/// (`0x800206E4..0x8002072C`), i.e. `ceil((frames * 16 - 1) / step)`. A
/// looping clip wraps to `0` on that same tick, so this is also its loop
/// period. `0` when `frames` is `0` (no clip to time).
///
/// REF: FUN_800204F8
pub fn clip_end_ticks(frames: u16, step: u16) -> u32 {
    if frames == 0 {
        return 0;
    }
    (u32::from(frames) * 16 - 1).div_ceil(u32::from(step.max(1)))
}

/// One clip tick of the cursor `actor+0x68` (1/16-frame units) under the
/// control word `actor+0x62`, for a clip of `frames` frames stepping `step`
/// units this tick (the frame-step scalar already multiplied in). Returns the
/// new cursor and the new word.
///
/// In retail order (`0x800205F0..0x8002072C`): a restart request `0x200` is
/// consumed and seats the cursor at frame `0` (or the last frame when
/// reversed); the word is re-read and its end latch `0x100` cleared; unless
/// held (`0x02`) the cursor steps forward (or back under `0x80`); a cursor
/// below `0` wraps by the clip's span or, clamped (`0x08`), stops at `0`;
/// a cursor at or past `frames * 16 - 1` wraps to `0` (the overshoot is not
/// carried) or, clamped, stops there. Either end raises the latch. Under the
/// clamp the latch is raised again on every tick the cursor rests at the end,
/// which is what a script's end-latch spin reads.
///
/// PORT: FUN_800204F8 (the advance half, `0x800205F0..0x8002072C`)
pub fn clip_cursor_tick(cursor: i16, flags: u16, step: i16, frames: u16) -> (i16, u16) {
    let span = i32::from(frames) * 16;
    let mut cursor = cursor;
    let mut flags = flags;
    if flags & CLIP_RESTART != 0 {
        flags &= !CLIP_RESTART;
        cursor = if flags & CLIP_REVERSE == 0 {
            0
        } else {
            (span - 16).max(0) as i16
        };
    }
    // Retail re-reads the word here, so the restart bit it just cleared is not
    // what the hold test sees.
    let read = flags;
    flags &= !CLIP_END;
    if read & CLIP_HOLD == 0 {
        cursor = if read & CLIP_REVERSE == 0 {
            cursor.wrapping_add(step)
        } else {
            cursor.wrapping_sub(step)
        };
    }
    if cursor < 0 {
        cursor = if flags & CLIP_CLAMP == 0 {
            cursor.wrapping_add(span as i16)
        } else {
            0
        };
        flags |= CLIP_END;
    }
    let last = (span - 1) as i16;
    if last <= cursor {
        cursor = if flags & CLIP_CLAMP == 0 { 0 } else { last };
        flags |= CLIP_END;
    }
    (cursor, flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The field template rate (`legaia_engine_field::field_anim::CLIP_RATE`).
    const CLIP_RATE: u16 = 8;

    /// The step select of `FUN_800204F8` at the template rate: ungated
    /// clips step by the rate, gated ones by `(rate*2 + div - 1) / div`.
    #[test]
    fn clip_step_matches_the_retail_select() {
        assert_eq!(
            clip_step(CLIP_RATE, false, 4),
            8,
            "ungated ignores the divisor"
        );
        let gated: Vec<u16> = [1u8, 2, 3, 4, 6]
            .iter()
            .map(|&d| clip_step(CLIP_RATE, true, d))
            .collect();
        assert_eq!(gated, vec![16, 8, 6, 4, 3]);
        assert_eq!(clip_step(CLIP_RATE, true, 0), CLIP_RATE, "div 0 falls back");
        // A script's `4C 41` rate (`16` = a door's one frame a tick).
        assert_eq!(clip_step(16, true, 4), 8);
    }

    /// The end latch lands on the first tick whose cursor reaches the last
    /// position; at the ungated step that is the old two-ticks-a-frame length,
    /// at divisor 4 twice it, and the player's loop period agrees.
    #[test]
    fn clip_end_ticks_is_the_first_tick_at_the_last_position() {
        for frames in 1u16..=20 {
            for step in [16u16, 8, 6, 4, 3] {
                let last = u32::from(frames) * 16 - 1;
                let mut cursor = 0u32;
                let mut ticks = 0u32;
                loop {
                    cursor += u32::from(step);
                    ticks += 1;
                    if cursor >= last {
                        break;
                    }
                }
                assert_eq!(
                    clip_end_ticks(frames, step),
                    ticks,
                    "{frames} frames, step {step}"
                );
            }
            assert_eq!(clip_end_ticks(frames, 8), u32::from(frames) * 2);
            assert_eq!(clip_end_ticks(frames, 4), u32::from(frames) * 4);
        }
        assert_eq!(clip_end_ticks(0, 8), 0);
    }

    /// A clamped clip stops on its last position and keeps the latch up; a
    /// looping one wraps to `0` on the same tick and latches once.
    #[test]
    fn the_clamp_holds_the_last_position_and_the_loop_wraps() {
        let (frames, step) = (3u16, 8i16);
        let ticks = clip_end_ticks(frames, step as u16);
        let (mut c, mut f) = (0i16, CLIP_CLAMP);
        for t in 1..=ticks + 4 {
            (c, f) = clip_cursor_tick(c, f, step, frames);
            if t < ticks {
                assert_eq!(f & CLIP_END, 0, "tick {t}");
            } else {
                assert_eq!(c, 47, "held on the last position (tick {t})");
                assert_ne!(f & CLIP_END, 0, "latched (tick {t})");
            }
        }
        let (mut c, mut f) = (0i16, 0u16);
        for t in 1..=ticks {
            (c, f) = clip_cursor_tick(c, f, step, frames);
            if t == ticks {
                assert_eq!(c, 0, "wrapped");
                assert_ne!(f & CLIP_END, 0);
            }
        }
        (c, f) = clip_cursor_tick(c, f, step, frames);
        assert_eq!((c, f & CLIP_END), (step, 0), "the latch clears next tick");
    }

    /// Hold freezes the cursor; restart seats it at the start (or the last
    /// frame when reversed) before the step.
    #[test]
    fn hold_restart_and_reverse() {
        assert_eq!(clip_cursor_tick(20, CLIP_HOLD, 8, 4), (20, CLIP_HOLD));
        assert_eq!(clip_cursor_tick(20, CLIP_RESTART, 8, 4), (8, 0));
        assert_eq!(
            clip_cursor_tick(20, CLIP_RESTART | CLIP_REVERSE, 8, 4),
            (40, CLIP_REVERSE)
        );
        // Reversed past frame 0: wrap by the span, or stop at 0 when clamped.
        assert_eq!(
            clip_cursor_tick(4, CLIP_REVERSE, 8, 4),
            (60, CLIP_REVERSE | CLIP_END)
        );
        assert_eq!(
            clip_cursor_tick(4, CLIP_REVERSE | CLIP_CLAMP, 8, 4),
            (0, CLIP_REVERSE | CLIP_CLAMP | CLIP_END)
        );
    }
}
