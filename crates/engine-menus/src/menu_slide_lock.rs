//! The menu's **window-slide input lock**: every menu-overlay sub-screen
//! reads `_DAT_8007BB80` before it reads the pad, and takes no input while
//! it is non-zero.
//!
//! The protocol, from the sub-screen drivers in PROT 0899:
//!
//! - A handler requests a screen by writing the sub-screen word
//!   `DAT_801E46A4`. The master tick (`FUN_801DC6B4`, `0x801DC950..0x801DC994`)
//!   compares it with the settled copy on the **next** frame, zeroes the
//!   phase word `DAT_801E46AC` and calls the new handler.
//! - Phase 0 of every handler runs its window program through the
//!   window-script VM `FUN_801D6628` and steps to phase 1; it reads no pad.
//!   The root picker `FUN_801D6B20` is the shape: `0x801D6B54..0x801D6B8C`.
//! - Phase 1 loads `_DAT_8007BB80` and skips its input while it is non-zero
//!   (`0x801D6B90..0x801D6B98` in the root picker; `0x801DC334` /
//!   `0x801DC438` in the prize counter; the same load heads every other
//!   handler's input phase).
//! - Every handler ends in the window walker `FUN_80031D00`, whose motion
//!   pass `FUN_80031AE4` rewrites the latch with the number of windows it
//!   moved ([`MenuWidgetState::step_motion`]).
//!
//! A program that creates or closes a window starts a twelve-frame motion,
//! so the latch the next eleven frames read is up and the pad is first read
//! twelve frames after the program ran. Every phase-0 program in the table
//! below closes the open set or creates a window, so after a press that
//! changes the screen the pad is next read thirteen frames on (the request
//! frame, the phase-0 frame, eleven held); a step inside a screen that runs
//! its program on the press itself (the prize counter's Yes / No window) is
//! read twelve frames on.
//!
//! [`MenuSlideLock`] is that rule over the port's screens: the caller names
//! the screen the menu is on ([`MenuScreen`]) once per frame, before the
//! screen ticks; the lock runs the program retail runs on that change
//! against the window list, steps the walker, and answers whether the pad
//! reaches the screen this frame. Without the overlay's programs
//! ([`MenuWidgetScripts`] unresolved) there is no window list and the lock
//! is open.

use crate::menu_widget::{MenuWidgetScripts, MenuWidgetState};
use legaia_engine_vm as vm;

/// The walker's frame delta (`DAT_1F800393`) while the menu runs: the menu
/// family installs a per-mode floor of `1` (`FUN_801DC6B4` among the
/// installers of `DAT_8007B9D8`, docs/subsystems/actor-vm.md), and on
/// hardware that keeps up the adaptive term is `1` too. The field's own
/// cadence (`2` in towns, `3` on the overworld) does not reach the menu.
pub const MENU_FRAME_DELTA: u8 = 1;

/// Where the menu stands, as the lock keys it: the retail sub-screen id
/// (`DAT_801E46A4`) and a step inside it that ran a program of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MenuScreen {
    /// Retail sub-screen id.
    pub subscreen: u8,
    /// `0` for the screen's base step; a non-zero step names one of
    /// [`SUBSCREEN_STEP_PROGRAMS`].
    pub step: u8,
}

impl MenuScreen {
    /// The base step of sub-screen `subscreen`.
    pub const fn at(subscreen: u8) -> Self {
        Self { subscreen, step: 0 }
    }

    /// Step `step` of sub-screen `subscreen`.
    pub const fn step(subscreen: u8, step: u8) -> Self {
        Self { subscreen, step }
    }
}

/// The programs each sub-screen's phase 0 hands `FUN_801D6628`, in call
/// order, by sub-screen id. Read off each handler's phase-0 arm (the `a0`
/// materialised ahead of the `jal 0x801D6628`).
pub const SUBSCREEN_ENTRY_PROGRAMS: &[(u8, &[u32])] = &[
    // FUN_801D6B20 root picker, 0x801D6B54.
    (0x01, &[0x801E_4BC0]),
    // FUN_801D6D38 ready check, 0x801D6D7C.
    (0x03, &[0x801E_4BD4]),
    // FUN_801DD1B8 briefing notice, 0x801DD1F0.
    (0x04, &[0x801E_4BE0]),
    // FUN_801D7C00 Items command, 0x801D7C4C..0x801D7C78.
    (0x05, &[0x801E_4BEC]),
    // FUN_801D7E50 Use list, 0x801D7EC0.
    (0x06, &[0x801E_4C14]),
    // FUN_801D8734 Throw Out list, 0x801D87C0.
    (0x07, &[0x801E_4C70]),
    // FUN_801D7FF8 all-party apply, 0x801D8078.
    (0x09, &[0x801E_4C30]),
    // FUN_801D8308 single-target apply, 0x801D8380.
    (0x0A, &[0x801E_4C48]),
    // FUN_801D8A58 Door of Light, 0x801D8AB0.
    (0x0B, &[0x801E_4CBC]),
    // FUN_801D8B90 Door of Wind, 0x801D8C24.
    (0x0C, &[0x801E_4CD4]),
    // FUN_801D8D94 Incense, 0x801D8DE4 (the call at 0x801D8EBC).
    (0x0D, &[0x801E_4CE4]),
    // FUN_801D8F10 Magic caster, 0x801D8F44..0x801D8FB4: two programs.
    (0x0E, &[0x801E_4D10, 0x801E_4CF4]),
    // FUN_801D9110 spell list, 0x801D91A0.
    (0x0F, &[0x801E_4D18]),
    // FUN_801D9280 group cast, 0x801D92FC.
    (0x10, &[0x801E_4D38]),
    // FUN_801D9594 single-target cast, 0x801D960C.
    (0x11, &[0x801E_4D60]),
    // FUN_801D98F0 Equip character picker, 0x801D9924..0x801D995C.
    (0x12, &[0x801E_4D88]),
    // FUN_801D99F0 Equip slot browse, 0x801D9ACC.
    (0x13, &[0x801E_4DA0]),
    // FUN_801D9C14 Equip candidate list, 0x801D9D00..0x801D9D3C.
    (0x14, &[0x801E_4DC8]),
    // FUN_801DA2A0 Status, 0x801DA31C..0x801DA360.
    (0x15, &[0x801E_4DEC]),
    // FUN_801DD330 Options = FUN_801DA9F8(0, 9, 0x30, 1), 0x801DAA84.
    (0x17, &[0x801E_4E08]),
    // FUN_801DAE24 card load / FUN_801DAEF4 card save: phase 0 closes every
    // window (0x801DAE74..0x801DAE84 / 0x801DAF54) and phase 1 waits the
    // latch out before the card screen starts.
    (0x18, &[0x801E_4E28]),
    (0x19, &[0x801E_4E30]),
    // FUN_801DC1CC casino prize counter, 0x801DC320..0x801DC674.
    (0x20, &[0x801E_4F18]),
];

/// Steps inside a sub-screen that run a program on the press that enters
/// them, and the program the press that leaves them back to the base step
/// runs: `(sub-screen, step, enter, leave)`.
pub const SUBSCREEN_STEP_PROGRAMS: &[(u8, u8, u32, u32)] = &[
    // FUN_801D8734's Yes / No: window 9 slides in over the list
    // (0x801D8878, `[04 13][01 9]`) and back out (0x801D8978 / 0x801D8A24,
    // `[0A 16][01 13][04 9]`).
    (0x07, 1, 0x801E_4C88, 0x801E_4C94),
    // FUN_801DC1CC's Yes / No: window 46 slides in on an affordable row
    // (0x801DC408..0x801DC41C, `[01 46]`) and out on No, cancel or the
    // commit (0x801DC4A0..0x801DC4B0 / 0x801DC66C, `[0A 44][04 46]`).
    (0x20, 1, 0x801E_4F2C, 0x801E_4F34),
];

/// The sub-screen program addresses [`MenuWidgetScripts`] resolves for the
/// lock: every entry and step program above.
pub fn lock_program_vas() -> impl Iterator<Item = u32> {
    SUBSCREEN_ENTRY_PROGRAMS
        .iter()
        .flat_map(|(_, vas)| vas.iter().copied())
        .chain(
            SUBSCREEN_STEP_PROGRAMS
                .iter()
                .flat_map(|&(_, _, a, b)| [a, b]),
        )
}

/// The window-slide input lock over one menu front end (module docs).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuSlideLock {
    seen: Option<MenuScreen>,
}

impl MenuSlideLock {
    /// The screen the lock last saw.
    pub fn screen(&self) -> Option<MenuScreen> {
        self.seen
    }

    /// Forget the screen (the menu closed); the next screen named is a
    /// fresh open.
    pub fn reset(&mut self) {
        self.seen = None;
    }

    /// One menu frame, called before the screen ticks with the screen the
    /// menu is on. Returns whether the pad reaches the screen this frame.
    ///
    /// - A new sub-screen is its phase-0 frame: its entry programs run, the
    ///   pad is withheld, and the walker takes its first pass.
    /// - A new step inside the same sub-screen ran its program on the
    ///   previous frame's press: the program runs and the walker catches up
    ///   that frame's pass, then this frame reads the latch as usual.
    /// - Otherwise the pad reaches the screen when the latch the previous
    ///   pass left is zero; the walker then takes this frame's pass.
    ///
    /// `screen` `None` (no menu) drops the window list on the way out.
    pub fn frame(
        &mut self,
        widgets: &mut MenuWidgetState,
        scripts: Option<&MenuWidgetScripts>,
        screen: Option<MenuScreen>,
        frame_delta: u8,
    ) -> bool {
        let Some(scripts) = scripts else {
            self.seen = screen;
            return true;
        };
        let Some(now) = screen else {
            if self.seen.take().is_some() {
                widgets.reset();
            }
            return true;
        };
        let before = self.seen.replace(now);
        if before == Some(now) {
            let open = widgets.slide_latch() == 0;
            widgets.step_motion(frame_delta);
            return open;
        }
        match before {
            Some(b) if b.subscreen == now.subscreen => {
                // A step inside the screen: the press ran its program last
                // frame, before that frame's walker pass.
                let va = if now.step == 0 {
                    step_program(b, false)
                } else {
                    step_program(now, true)
                };
                if let Some(va) = va {
                    run(widgets, scripts, va);
                    widgets.step_motion(frame_delta);
                }
                let open = widgets.slide_latch() == 0;
                widgets.step_motion(frame_delta);
                open
            }
            _ => {
                if before.is_none() {
                    widgets.reset();
                }
                let programs = entry_programs(now.subscreen);
                if programs.is_empty() {
                    // A screen with no retail program (a port-only screen)
                    // has no phase 0 to wait out.
                    let open = widgets.slide_latch() == 0;
                    widgets.step_motion(frame_delta);
                    return open;
                }
                for &va in programs {
                    run(widgets, scripts, va);
                }
                widgets.step_motion(frame_delta);
                false
            }
        }
    }
}

fn entry_programs(subscreen: u8) -> &'static [u32] {
    SUBSCREEN_ENTRY_PROGRAMS
        .iter()
        .find(|(id, _)| *id == subscreen)
        .map(|(_, vas)| *vas)
        .unwrap_or(&[])
}

fn step_program(screen: MenuScreen, enter: bool) -> Option<u32> {
    SUBSCREEN_STEP_PROGRAMS
        .iter()
        .find(|&&(id, step, _, _)| id == screen.subscreen && step == screen.step)
        .map(|&(_, _, a, b)| if enter { a } else { b })
}

fn run(widgets: &mut MenuWidgetState, scripts: &MenuWidgetScripts, va: u32) {
    if let Some(bytes) = scripts.program(va) {
        let _ = vm::run(widgets, bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_widget::WidgetDescriptor;

    /// Hand-authored programs (not disc bytes) at the root picker's and the
    /// prize counter's addresses: close all, then open one window.
    fn scripts() -> MenuWidgetScripts {
        let open = |id: u8| vec![0x05, 0, 0, 0, 0x01, id, 0, 0, 0, 0, 0, 0];
        MenuWidgetScripts {
            shop_open: Vec::new(),
            shop_sell_away: Vec::new(),
            programs: Vec::new(),
            lock_programs: vec![
                (0x801E_4BC0, open(50)),
                (0x801E_4F18, open(43)),
                (0x801E_4F2C, vec![0x01, 46, 0, 0, 0, 0, 0, 0]),
                (0x801E_4F34, vec![0x04, 46, 0, 0, 0, 0, 0, 0]),
            ],
        }
    }

    fn widgets() -> MenuWidgetState {
        let mut w = MenuWidgetState::default();
        w.set_descriptors(
            (0..52)
                .map(|i| WidgetDescriptor {
                    home: vm::Position::new(40, 40 + i),
                    size: (64, 32),
                    park_edge: (i % 8) as u8,
                    kind: 0,
                })
                .collect(),
        );
        w
    }

    fn frames(
        lock: &mut MenuSlideLock,
        w: &mut MenuWidgetState,
        s: &MenuWidgetScripts,
        screen: MenuScreen,
        n: usize,
    ) -> Vec<bool> {
        (0..n)
            .map(|_| lock.frame(w, Some(s), Some(screen), 1))
            .collect()
    }

    #[test]
    fn a_new_screen_takes_the_pad_twelve_frames_after_its_first() {
        let (mut lock, mut w, s) = (MenuSlideLock::default(), widgets(), scripts());
        let open = frames(&mut lock, &mut w, &s, MenuScreen::at(0x01), 14);
        // Phase 0, then eleven frames reading a raised latch.
        assert_eq!(open.iter().position(|&o| o), Some(12), "{open:?}");
        assert!(open[12..].iter().all(|&o| o));
    }

    #[test]
    fn a_step_inside_a_screen_reads_the_pad_twelve_frames_after_its_press() {
        let (mut lock, mut w, s) = (MenuSlideLock::default(), widgets(), scripts());
        frames(&mut lock, &mut w, &s, MenuScreen::at(0x20), 20);
        let open = frames(&mut lock, &mut w, &s, MenuScreen::step(0x20, 1), 13);
        assert_eq!(open.iter().position(|&o| o), Some(11), "{open:?}");
        // Leaving the step closes window 46: the same wait.
        let open = frames(&mut lock, &mut w, &s, MenuScreen::at(0x20), 13);
        assert_eq!(open.iter().position(|&o| o), Some(11), "{open:?}");
    }

    #[test]
    fn without_the_overlay_the_lock_is_open() {
        let (mut lock, mut w) = (MenuSlideLock::default(), widgets());
        assert!(lock.frame(&mut w, None, Some(MenuScreen::at(0x01)), 1));
        assert!(lock.frame(&mut w, None, Some(MenuScreen::at(0x05)), 1));
    }

    #[test]
    fn closing_the_menu_drops_the_window_list() {
        let (mut lock, mut w, s) = (MenuSlideLock::default(), widgets(), scripts());
        frames(&mut lock, &mut w, &s, MenuScreen::at(0x01), 3);
        assert!(w.any_open());
        assert!(lock.frame(&mut w, Some(&s), None, 1));
        assert!(!w.any_open());
        assert_eq!(lock.screen(), None);
    }
}
