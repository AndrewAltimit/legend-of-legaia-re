//! The hall's animated props: the plain floor actors of `FUN_801D2A10` whose
//! object record names a clip.

/// The field actor template rate (`0x10`) the floor pass halves into `+0x6A`
/// (`sra v0,v0,0x11` at `0x801D2D0C`): eight cursor units, half a frame, per
/// vsync of a tick.
pub const HALL_PROP_RATE: u16 = 8;

/// The tick the floor pass's plain template (`0x801D42FC`) carries.
pub const HALL_PROP_TICK: u32 = 0x801D_4098;

/// One clip's cursor on the hall's plain floor actors.
///
/// The floor pass `FUN_801D2A10` runs once, from the hall init
/// `FUN_801CEF54` (`jal` at `0x801CF1B8`), and spawns every cell whose clip is
/// not a marker's from the plain template `0x801D42FC`. It stamps the object
/// record's class byte into `+0x5C` as the actor's clip
/// (`0x801D2D14..0x801D2D40`: `FUN_8003C8F0(record, 0)`, then
/// `FUN_8003D0BC` past the record's name) and halves the template rate. The
/// template's tick `FUN_801D4098` hands an actor with `+0x5C > 0` to the clip
/// driver `FUN_800204F8` every pass, under the template's looping word
/// `0x15`. Every such actor spawns on the same pass at the same rate, so one
/// cursor per clip is every actor's: `minigame_dance_retail_go_out` holds all
/// fourteen (clips `1`, `2` and `4`, thirty frames each) on `312`.
///
/// These are the hall's spotlight beams and their fixtures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HallPropClip {
    /// Clip id (`+0x5C`): scene ANM record `anim_id - 1`.
    pub anim_id: u8,
    /// The clip's frame count.
    pub frames: u16,
    /// The record's blend / scaled-step gate (clip byte `+1` bit 0).
    pub blends: bool,
    /// The record's step divisor (its `flag` low byte).
    pub step_div: u8,
    /// Frame cursor in 1/16-frame units (`+0x68`).
    pub cursor: i16,
    /// Clip control word (`+0x62`).
    pub flags: u16,
}

/// The template's clip control word: looping, forward, no hold.
const HALL_PROP_FLAGS: u16 = 0x0015;

/// Every clip of the hall's ANM bank on the plain actors' shared clock.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HallProps {
    clips: Vec<HallPropClip>,
}

impl HallProps {
    /// One cursor per record of the hall's ANM bank, each at frame `0`, as the
    /// floor pass leaves them.
    pub fn from_bank(bank: &legaia_actor_formats::player_anm::PlayerAnmBundle) -> Self {
        let clips = (0..bank.record_count as usize)
            .filter_map(|r| {
                let rec = bank.record_lenient(r).ok()?;
                let anim_id = u8::try_from(r + 1).ok()?;
                Some(HallPropClip {
                    anim_id,
                    frames: rec.frame_count.max(1),
                    blends: rec.blends(),
                    step_div: (rec.flag & 0xFF) as u8,
                    cursor: 0,
                    flags: HALL_PROP_FLAGS,
                })
            })
            .collect();
        Self { clips }
    }

    /// A clock over explicit clips (a host's own, or a test's).
    pub fn from_clips(clips: Vec<HallPropClip>) -> Self {
        Self { clips }
    }

    /// One pass of the plain actors' tick (`FUN_801D4098` -> `FUN_800204F8`)
    /// on a game frame `frame_step` vsyncs long.
    ///
    /// REF: FUN_801D4098 (the plain floor actor's tick: clip driver when
    /// `+0x5C > 0`)
    pub fn tick(&mut self, frame_step: u16) {
        for c in &mut self.clips {
            let step = legaia_engine_vm::actor_tick::clip_step(HALL_PROP_RATE, c.blends, c.step_div)
                .wrapping_mul(frame_step.max(1)) as i16;
            (c.cursor, c.flags) =
                legaia_engine_vm::actor_tick::clip_cursor_tick(c.cursor, c.flags, step, c.frames);
        }
    }

    /// Clip `anim_id`'s cursor, when the bank carries it.
    pub fn clip(&self, anim_id: u8) -> Option<&HallPropClip> {
        self.clips.iter().find(|c| c.anim_id == anim_id)
    }

    /// Stand clip `anim_id` on a retail state's own cursor - capture alignment
    /// for the retail comparison's frame, never a game path: the clock counts
    /// the hall's game frames, whose lengths a seed does not replay.
    pub fn seat(&mut self, anim_id: u8, cursor: i16) {
        if let Some(c) = self.clips.iter_mut().find(|c| c.anim_id == anim_id) {
            c.cursor = cursor;
        }
    }

    /// [`Self::seat`] on the cursor a retail state's RAM holds, walked back
    /// `passes` passes of `frame_step` vsyncs: the frame on screen predates
    /// the RAM by the display lag. A step back from the loop's head lands on
    /// the last cursor the forward loop reached before its wrap.
    pub fn seat_passes_back(&mut self, anim_id: u8, cursor: i16, passes: u16, frame_step: u16) {
        let Some(c) = self.clips.iter_mut().find(|c| c.anim_id == anim_id) else {
            return;
        };
        let step = i32::from(
            legaia_engine_vm::actor_tick::clip_step(HALL_PROP_RATE, c.blends, c.step_div)
                .wrapping_mul(frame_step.max(1)),
        )
        .max(1);
        let span = i32::from(c.frames) * 16;
        let mut at = i32::from(cursor);
        for _ in 0..passes {
            at = if at >= step {
                at - step
            } else {
                ((span - 2).max(0) / step) * step
            };
        }
        c.cursor = at as i16;
    }

    /// Whether no bank is attached.
    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(frames: u16) -> HallProps {
        HallProps {
            clips: vec![HallPropClip {
                anim_id: 2,
                frames,
                blends: false,
                step_div: 0,
                cursor: 0,
                flags: HALL_PROP_FLAGS,
            }],
        }
    }

    /// At the hall's step of `3` a thirty-frame clip moves a frame and a half a
    /// pass and wraps to frame `0` on the pass that would reach `480`.
    #[test]
    fn a_hall_clip_loops_at_one_and_a_half_frames_a_pass() {
        let mut p = props(30);
        p.tick(3);
        assert_eq!(p.clip(2).unwrap().cursor, 24);
        for _ in 0..12 {
            p.tick(3);
        }
        assert_eq!(p.clip(2).unwrap().cursor, 312, "go_out's cursor");
        for _ in 0..6 {
            p.tick(3);
        }
        assert_eq!(p.clip(2).unwrap().cursor, 456);
        p.tick(3);
        assert_eq!(p.clip(2).unwrap().cursor, 0, "the overshoot is not carried");
        assert!(p.clip(1).is_none());
    }

    /// Walking back from the loop's head lands on the last pre-wrap cursor.
    #[test]
    fn a_displayed_seat_walks_back_across_the_wrap() {
        let mut p = props(30);
        p.seat_passes_back(2, 312, 2, 3);
        assert_eq!(p.clip(2).unwrap().cursor, 264);
        p.seat_passes_back(2, 24, 2, 3);
        assert_eq!(p.clip(2).unwrap().cursor, 456);
        // And forward again from there reaches the RAM's cursor.
        p.tick(3);
        p.tick(3);
        assert_eq!(p.clip(2).unwrap().cursor, 24);
    }

    #[test]
    fn a_seat_stands_the_clip_on_the_capture_s_cursor() {
        let mut p = props(30);
        p.seat(2, 312);
        assert_eq!(p.clip(2).unwrap().cursor, 312);
        p.seat(9, 1);
        assert_eq!(p.clip(2).unwrap().cursor, 312);
    }
}
