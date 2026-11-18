//! The field's fade to black around the pause menu - and so around every
//! save screen a save point or the world map's Save row opens.
//!
//! Retail opens the pause menu through the field overlay's subsystem actor,
//! whose session handler `FUN_801ED308` (handler `0x30`) owns a brightness
//! accumulator `_DAT_8007B440` and hands it to the field's wipe emitter
//! `FUN_8003479C` every pass:
//!
//! - phase `0` zeroes the level and falls into phase `1`, which raises it by
//!   `10 * frame_step` (`DAT_1F800393`) a pass, and once `level + 0x70 > 0xF2`
//!   spawns the menu (`FUN_801D841C`) - so the field darkens for a few passes
//!   before any window appears, and the menu takes no input until it exists;
//! - phase `2` keeps raising it to `0xF2`, phase `3` holds `0xF2` while the
//!   menu runs;
//! - on the menu's close (exit code `> 5`) phase `3` draws one more pass at
//!   `0xF2`, then phase `4` lowers it by `10 * frame_step` a pass back to `0`,
//!   with the field running again under the lifting wipe.
//!
//! The session is a node of the field's actor lists, so it runs once a
//! **game frame**, not once a vsync: at the field's step of `2` the level
//! climbs `20` every second vsync. The town01 capture
//! (`autorun_pause_wipe_step.lua`) reads the emitter's level `20, 40, ..,
//! 140` on every second vsync from the first pass, the menu mode `0x17` two
//! passes after the spawn, and the close's ramp `242, 222, .., 2, 0` on every
//! second vsync once the field is back.
//!
//! [`PauseWipe`] is that accumulator, ticked once a vsync with its own count
//! of the vsyncs into the game frame. Both play hosts own one through
//! `BootSession`, start it on a field menu press, release it on the close,
//! and draw [`PauseWipe::curtain_level`] through the emitter's port
//! (`legaia_render_kernels::wipe_curtain`): two soft-edged curtains closing
//! from the top and the bottom of the screen, not an even fade.
//!
//! REF: FUN_801ED308 (phases 0..4 of the pause-menu session)

/// The level the session holds while the menu is up (`0xF2`).
pub const PAUSE_WIPE_FULL: i32 = 0xF2;
/// The spawn test's offset: the menu appears once `level + 0x70 > 0xF2`.
pub const PAUSE_WIPE_SPAWN_OFFSET: i32 = 0x70;
/// Per-pass step, scaled by the frame-skip factor.
pub const PAUSE_WIPE_STEP: i32 = 10;

/// Vsyncs from the pass that spawns the menu to the menu's first frame
/// while retail loads the menu overlay (`CARD INIT`, game mode `0x16`):
/// over `conc` (`autorun_pause_wipe_step.lua`) the spawning pass writes
/// level `140` at vsync `28` and the menu's first pass raises its counter at
/// vsync `88`. A CD wait, so the port runs it only in its retail-faithful
/// mode (`WorldToggles::skip_load_holds` off).
pub const LOAD_HOLD_OPEN_VSYNCS: u16 = 60;

/// Of those, the vsyncs the last field frame stays on the screen before
/// the display goes black: game mode `0x17` arrives at vsync `42`, `14`
/// after the spawning pass, and the filmed frame holds the level-`160`
/// curtain until then.
pub const LOAD_HOLD_LAST_FRAME_VSYNCS: u16 = 14;

/// Vsyncs from the closing press to the field's return while retail
/// reloads the field overlay: over `conc` Circle's edge reads at vsync `251`
/// and game mode `3` returns at `331`; the menu's windows slide out over the
/// first twelve of them and the screen is black for the rest. Retail-faithful
/// mode only, as [`LOAD_HOLD_OPEN_VSYNCS`].
pub const LOAD_HOLD_CLOSE_VSYNCS: u16 = 80;

/// Where the wipe is. `vsyncs` counts the vsyncs since the session's last
/// pass; a pass runs when it reaches the frame step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PauseWipe {
    /// No menu, no wipe.
    #[default]
    Idle,
    /// The press's first game frame: the subsystem actor the accept spawned
    /// runs its default handler, the state pick `FUN_801F1F4C` (slot `7`),
    /// which installs the session state `0x30` and raises nothing. The
    /// session's phase `0` runs on the next game frame. A capture over `conc`
    /// reads the edge at vsync `12` and the level `20` at vsync `16`: two
    /// game frames at step `2`.
    Picking { vsyncs: u8 },
    /// Phases `1..=3`: the field darkening, then held black under the menu.
    Opening { level: i32, vsyncs: u8 },
    /// Phase `4`: the menu has closed and the field is brightening.
    Closing { level: i32, vsyncs: u8 },
    /// The menu has spawned and its overlay is loading
    /// ([`LOAD_HOLD_OPEN_VSYNCS`]): the field is stopped, its last frame
    /// stands under the curtain and then the screen is black; the menu
    /// takes no input. `held` counts the vsyncs so far.
    Loading { level: i32, held: u16 },
    /// The menu has closed and the field overlay is loading
    /// ([`LOAD_HOLD_CLOSE_VSYNCS`]): black, the field stopped. `left`
    /// counts down to the lifting ramp.
    Reloading { left: u16 },
}

impl PauseWipe {
    /// A menu press: the spawned actor's state pick takes the next game
    /// frame, then phase `0` zeroes the level and phase `1` starts raising it.
    pub fn open(&mut self) {
        *self = PauseWipe::Picking { vsyncs: 0 };
    }

    /// The menu closed: phase `4` ramps down from wherever the level stands
    /// (`0xF2` once the menu has been up). The level stands one game frame
    /// before the first step down, as phase `3`'s last pass draws it.
    pub fn close(&mut self) {
        match *self {
            PauseWipe::Opening { level, .. } => {
                *self = PauseWipe::Closing { level, vsyncs: 0 };
            }
            PauseWipe::Picking { .. } => *self = PauseWipe::Idle,
            _ => {}
        }
    }

    /// Retail-faithful mode: hold the spawned menu off while its overlay
    /// loads. A no-op unless the opening ramp has just spawned the menu.
    pub fn hold_for_load(&mut self) {
        if let PauseWipe::Opening { level, .. } = *self
            && self.menu_spawned()
        {
            *self = PauseWipe::Loading { level, held: 0 };
        }
    }

    /// Retail-faithful mode: the menu closed, hold black while the field
    /// overlay reloads, then lift.
    pub fn close_with_reload(&mut self) {
        *self = PauseWipe::Reloading {
            left: LOAD_HOLD_CLOSE_VSYNCS,
        };
    }

    /// Whether the closing hold is still running.
    pub fn reloading(&self) -> bool {
        matches!(self, PauseWipe::Reloading { .. })
    }

    /// Drop the wipe outright (a scene change, a battle).
    pub fn reset(&mut self) {
        *self = PauseWipe::Idle;
    }

    /// One vsync of a game frame `frame_step` vsyncs long: the session's
    /// pass runs on the vsync that completes the frame.
    pub fn tick(&mut self, frame_step: u8) {
        let wait = frame_step.max(1);
        let step = PAUSE_WIPE_STEP * i32::from(wait);
        *self = match *self {
            PauseWipe::Idle => PauseWipe::Idle,
            PauseWipe::Picking { vsyncs } if vsyncs + 1 < wait => {
                PauseWipe::Picking { vsyncs: vsyncs + 1 }
            }
            PauseWipe::Picking { .. } => PauseWipe::Opening {
                level: 0,
                vsyncs: 0,
            },
            PauseWipe::Opening { level, vsyncs } if vsyncs + 1 < wait => PauseWipe::Opening {
                level,
                vsyncs: vsyncs + 1,
            },
            PauseWipe::Opening { level, .. } => PauseWipe::Opening {
                level: (level + step).min(PAUSE_WIPE_FULL),
                vsyncs: 0,
            },
            PauseWipe::Closing { level, vsyncs } if vsyncs + 1 < wait => PauseWipe::Closing {
                level,
                vsyncs: vsyncs + 1,
            },
            PauseWipe::Closing { level, .. } if level - step < 1 => PauseWipe::Idle,
            PauseWipe::Closing { level, .. } => PauseWipe::Closing {
                level: level - step,
                vsyncs: 0,
            },
            PauseWipe::Loading { level, held } if held + 1 < LOAD_HOLD_OPEN_VSYNCS => {
                PauseWipe::Loading {
                    level,
                    held: held + 1,
                }
            }
            PauseWipe::Loading { .. } => PauseWipe::Opening {
                level: PAUSE_WIPE_FULL,
                vsyncs: 0,
            },
            PauseWipe::Reloading { left } if left > 1 => PauseWipe::Reloading { left: left - 1 },
            PauseWipe::Reloading { .. } => PauseWipe::Closing {
                level: PAUSE_WIPE_FULL,
                vsyncs: 0,
            },
        };
    }

    /// Whether the menu exists yet: retail spawns it the pass the opening
    /// level passes `0xF2 - 0x70`. Before that the field is drawing (under
    /// the wipe) and the menu takes no input.
    pub fn menu_spawned(&self) -> bool {
        match *self {
            PauseWipe::Opening { level, .. } => level + PAUSE_WIPE_SPAWN_OFFSET > PAUSE_WIPE_FULL,
            PauseWipe::Picking { .. } | PauseWipe::Loading { .. } => false,
            _ => true,
        }
    }

    /// The level (`0..=0xF2`) to hand the curtain emitter
    /// (`legaia_render_kernels::wipe_curtain::curtain_prims`) this frame,
    /// `None` when the field is not showing (the menu covers it) or no wipe
    /// runs. `0` draws nothing.
    pub fn curtain_level(&self) -> Option<u8> {
        let level = match *self {
            PauseWipe::Opening { level, .. } if !self.menu_spawned() => level,
            PauseWipe::Closing { level, .. } => level,
            PauseWipe::Picking { .. } => 0,
            // The spawning pass's frame, one pass on, then black.
            PauseWipe::Loading { level, held } if held < LOAD_HOLD_LAST_FRAME_VSYNCS => {
                level + PAUSE_WIPE_STEP * 2
            }
            PauseWipe::Loading { .. } | PauseWipe::Reloading { .. } => PAUSE_WIPE_FULL,
            _ => return None,
        };
        Some(level.clamp(0, PAUSE_WIPE_FULL) as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pass's level after each vsync of a run of ticks.
    fn levels(w: &mut PauseWipe, step: u8, n: usize) -> Vec<Option<u8>> {
        (0..n)
            .map(|_| {
                w.tick(step);
                w.curtain_level()
            })
            .collect()
    }

    /// At the field's frame step of `2` the session runs every second vsync:
    /// the level climbs `20` a pass, the menu spawns on the seventh pass
    /// (level `140 > 130`, sixteen vsyncs after the press, the state pick's
    /// game frame first), the opening
    /// ramp reaches `0xF2` on the thirteenth pass, and the close stands at
    /// `0xF2` one pass and lifts in thirteen - the town01 capture's
    /// `242, 222, .., 2` then `0`.
    #[test]
    fn the_ramp_runs_once_a_game_frame() {
        let mut w = PauseWipe::default();
        assert!(w.menu_spawned() && w.curtain_level().is_none());
        w.open();
        assert_eq!(w.curtain_level(), Some(0));
        let opening = levels(&mut w, 2, 6);
        assert_eq!(
            opening,
            vec![Some(0), Some(0), Some(0), Some(20), Some(20), Some(40)]
        );
        let mut spawned_at = None;
        for v in 7..=42 {
            w.tick(2);
            if spawned_at.is_none() && w.menu_spawned() {
                spawned_at = Some(v);
            }
        }
        assert_eq!(spawned_at, Some(16));
        assert_eq!(
            w,
            PauseWipe::Opening {
                level: 0xF2,
                vsyncs: 0
            }
        );
        assert!(w.curtain_level().is_none(), "the menu covers the field");
        w.close();
        assert_eq!(w.curtain_level(), Some(0xF2));
        let mut drawn = vec![w.curtain_level()];
        while w != PauseWipe::Idle {
            w.tick(2);
            drawn.push(w.curtain_level());
        }
        let passes: Vec<u8> = drawn.iter().step_by(2).flatten().copied().collect();
        assert_eq!(
            passes,
            vec![242, 222, 202, 182, 162, 142, 122, 102, 82, 62, 42, 22, 2]
        );
        assert_eq!(drawn.len(), 27, "thirteen passes of two vsyncs, then idle");
    }

    /// At a step of `3` each pass spans three vsyncs and climbs `30`.
    #[test]
    fn a_step_of_three_climbs_thirty_every_third_vsync() {
        let mut w = PauseWipe::default();
        w.open();
        let l = levels(&mut w, 3, 9);
        assert_eq!(
            l,
            vec![
                Some(0),
                Some(0),
                Some(0),
                Some(0),
                Some(0),
                Some(30),
                Some(30),
                Some(30),
                Some(60)
            ]
        );
    }

    /// Retail-faithful holds: the spawned menu waits out the overlay load
    /// (the last field frame, then black), and a close holds black for the
    /// field's reload before the lifting ramp.
    #[test]
    fn the_load_holds_keep_the_menu_off_and_the_screen_dark() {
        let mut w = PauseWipe::default();
        w.open();
        while !w.menu_spawned() {
            w.tick(2);
        }
        w.hold_for_load();
        assert!(!w.menu_spawned());
        assert_eq!(w.curtain_level(), Some(160));
        let mut n = 0;
        while !w.menu_spawned() {
            w.tick(2);
            n += 1;
        }
        assert_eq!(n, LOAD_HOLD_OPEN_VSYNCS);
        w.close_with_reload();
        assert!(w.reloading());
        assert_eq!(w.curtain_level(), Some(0xF2));
        for _ in 0..LOAD_HOLD_CLOSE_VSYNCS {
            w.tick(2);
        }
        assert_eq!(
            w,
            PauseWipe::Closing {
                level: 0xF2,
                vsyncs: 0
            }
        );
    }

    /// A close before the menu spawned lifts from the current level.
    #[test]
    fn an_early_close_lifts_from_the_current_level() {
        let mut w = PauseWipe::default();
        w.open();
        for _ in 0..6 {
            w.tick(2);
        }
        w.close();
        assert_eq!(
            w,
            PauseWipe::Closing {
                level: 40,
                vsyncs: 0
            }
        );
    }
}
