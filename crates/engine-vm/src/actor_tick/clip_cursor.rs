//! The clip cursor's step and end timing.

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
}
