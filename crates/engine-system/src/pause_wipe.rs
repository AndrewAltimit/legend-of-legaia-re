//! The field's fade to black around the pause menu - and so around every
//! save screen a save point or the world map's Save row opens.
//!
//! Retail opens the pause menu through the field overlay's subsystem actor,
//! whose session handler `FUN_801ED308` (handler `0x30`) owns a brightness
//! accumulator `_DAT_8007B440` and hands it to the field's wipe emitter
//! `FUN_8003479C` every frame:
//!
//! - phase `0` zeroes the level; phase `1` raises it by `10 * frame_step`
//!   (`DAT_1F800393`) a frame, and once `level + 0x70 > 0xF2` spawns the menu
//!   (`FUN_801D841C`) - so the field darkens for a few frames before any
//!   window appears, and the menu takes no input until it exists;
//! - phase `2` keeps raising it to `0xF2`, phase `3` holds `0xF2` while the
//!   menu runs;
//! - on the menu's close (exit code `> 5`) phase `4` lowers it by
//!   `10 * frame_step` a frame back to `0`, with the field running again
//!   under the lifting wipe.
//!
//! [`PauseWipe`] is that accumulator. Both play hosts own one through
//! `BootSession`, start it on a field menu press, release it on the close,
//! and draw [`PauseWipe::fade_level`] as the subtractive full-screen quad the
//! shop's opening fade already uses (`screen_prim::fade_prim(level * 0x010101,
//! 2, 0)`).
//!
//! REF: FUN_801ED308 (phases 0..4 of the pause-menu session)

/// The level the session holds while the menu is up (`0xF2`).
pub const PAUSE_WIPE_FULL: i32 = 0xF2;
/// The spawn test's offset: the menu appears once `level + 0x70 > 0xF2`.
pub const PAUSE_WIPE_SPAWN_OFFSET: i32 = 0x70;
/// Per-frame step, scaled by the frame-skip factor.
pub const PAUSE_WIPE_STEP: i32 = 10;

/// Where the wipe is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PauseWipe {
    /// No menu, no wipe.
    #[default]
    Idle,
    /// Phases `1..=3`: the field darkening, then held black under the menu.
    Opening { level: i32 },
    /// Phase `4`: the menu has closed and the field is brightening.
    Closing { level: i32 },
}

impl PauseWipe {
    /// A menu press: phase `0` zeroes the level.
    pub fn open(&mut self) {
        *self = PauseWipe::Opening { level: 0 };
    }

    /// The menu closed: phase `4` ramps down from wherever the level stands
    /// (`0xF2` once the menu has been up).
    pub fn close(&mut self) {
        if let PauseWipe::Opening { level } = *self {
            *self = PauseWipe::Closing { level };
        }
    }

    /// Drop the wipe outright (a scene change, a battle).
    pub fn reset(&mut self) {
        *self = PauseWipe::Idle;
    }

    /// One frame, `frame_step` vsyncs long.
    pub fn tick(&mut self, frame_step: u8) {
        let step = PAUSE_WIPE_STEP * i32::from(frame_step.max(1));
        *self = match *self {
            PauseWipe::Idle => PauseWipe::Idle,
            PauseWipe::Opening { level } => PauseWipe::Opening {
                level: (level + step).min(PAUSE_WIPE_FULL),
            },
            PauseWipe::Closing { level } if level - step < 1 => PauseWipe::Idle,
            PauseWipe::Closing { level } => PauseWipe::Closing {
                level: level - step,
            },
        };
    }

    /// Whether the menu exists yet: retail spawns it the frame the opening
    /// level passes `0xF2 - 0x70`. Before that the field is drawing (under
    /// the wipe) and the menu takes no input.
    pub fn menu_spawned(&self) -> bool {
        match *self {
            PauseWipe::Opening { level } => level + PAUSE_WIPE_SPAWN_OFFSET > PAUSE_WIPE_FULL,
            _ => true,
        }
    }

    /// The subtractive quad's grey level (`0..=255`) to draw over the field
    /// this frame, `None` when the field is not showing (the menu covers
    /// it) or no wipe runs. `0xF2` is full black: the emitter's own scale.
    pub fn fade_level(&self) -> Option<u8> {
        let level = match *self {
            PauseWipe::Opening { level } if !self.menu_spawned() => level,
            PauseWipe::Closing { level } => level,
            _ => return None,
        };
        Some(((level.clamp(0, PAUSE_WIPE_FULL) * 255) / PAUSE_WIPE_FULL) as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// At the 30 fps frame step (`2`) the menu spawns on the seventh frame
    /// (level `140 > 130`), the opening ramp reaches `0xF2` on the
    /// thirteenth, and the close lifts it in thirteen.
    #[test]
    fn the_ramp_matches_the_session_phases() {
        let mut w = PauseWipe::default();
        assert!(w.menu_spawned() && w.fade_level().is_none());
        w.open();
        assert_eq!(w.fade_level(), Some(0));
        let mut spawned_at = None;
        for f in 1..=20 {
            w.tick(2);
            if spawned_at.is_none() && w.menu_spawned() {
                spawned_at = Some(f);
            }
        }
        assert_eq!(spawned_at, Some(7));
        assert_eq!(w, PauseWipe::Opening { level: 0xF2 });
        assert!(w.fade_level().is_none(), "the menu covers the field");
        w.close();
        assert_eq!(w.fade_level(), Some(255));
        let mut frames = 0;
        while w != PauseWipe::Idle {
            w.tick(2);
            frames += 1;
        }
        assert_eq!(frames, 13);
    }

    /// A close before the menu spawned lifts from where the ramp stood.
    #[test]
    fn an_early_close_lifts_from_the_current_level() {
        let mut w = PauseWipe::default();
        w.open();
        w.tick(2);
        w.tick(2);
        w.close();
        assert_eq!(w, PauseWipe::Closing { level: 40 });
    }
}
