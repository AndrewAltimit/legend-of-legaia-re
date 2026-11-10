//! The Triangle arts list's per-frame driver, `FUN_801D3444` (PROT 0898):
//! the list window's slide counter, its `x`, and which help caption the
//! entry screen puts up.
//!
//! PORT: FUN_801D3444
//!
//! The command flow `FUN_801D0748` calls it every frame after the frame's
//! input (`0x801D0B24`). It owns three bytes of overlay data:
//!
//! | address | field |
//! |---|---|
//! | `0x801F4E09` | [`ArtsListDriver::flags`]: `0x80` registered, `0x01` open |
//! | `0x801F4E0A` | [`ArtsListDriver::slide`], `0..=12` |
//! | `0x801F4E08` | [`ArtsListDriver::latch`], the closed caption's latch |
//!
//! With the registered bit clear it does nothing. Otherwise the counter
//! climbs by the frame step (`DAT_1F800393`) while the list is open in the
//! entry state `0x50` and seats at `12` (the climb is a byte add, so it wraps
//! before the clamp), and drains by the step otherwise, floored at `0`. The
//! window's `x` (`*(0x801F4E0C) + 0xA`) is `slide * 160 / 12 - 146`. Drained
//! outside the entry, the driver clears the whole flags byte and the latch
//! (`0x801D3528..0x801D3554`), which deregisters it.
//!
//! The caption. The latch is set the first frame the closed list sits in the
//! entry with a learned art (`record[+0x185]`, `0x801D3570..0x801D35E8`); the
//! pager clears it when it opens the list (`0x801D3868`) and the entry's
//! registration does (`0x801DBBA0`). A set latch draws `View Hyper Arts list`
//! whatever the flow byte - so a list closed shortly before the entry ends
//! keeps that caption up while it slides out. A clear latch tests the list's
//! row offset `0x8007B458` in the entry state: row `0` with six or more arts,
//! or row `5` with eleven or more, draws `View Next page`.

/// [`ArtsListDriver::flags`]: the list window is registered.
pub const ARTS_LIST_REGISTERED: u8 = 0x80;
/// [`ArtsListDriver::flags`]: the list is open.
pub const ARTS_LIST_OPEN: u8 = 0x01;
/// The slide counter seated (`sltiu v0,v0,0xd` / `li v0,0xc`).
pub const ARTS_LIST_SLIDE_SEATED: u8 = 12;
/// The window's `x` at slide `0`.
pub const ARTS_LIST_OFFSCREEN_X: i16 = -146;

/// The three overlay bytes the driver owns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArtsListDriver {
    /// `0x801F4E09`.
    pub flags: u8,
    /// `0x801F4E0A`.
    pub slide: u8,
    /// `0x801F4E08`.
    pub latch: u8,
}

/// What the driver reads besides its own bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArtsListDriverInputs {
    /// The flow byte `ctx[+6]` reads the entry state `0x50`.
    pub entering: bool,
    /// `DAT_1F800393`.
    pub frame_step: u8,
    /// The acting character's learned-arts count, `record[+0x185]`.
    pub learned: u8,
    /// The list's row offset `0x8007B458` (`0`, `5` or `10`).
    pub row_offset: u32,
}

/// Which help caption the driver draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtsListDriverCaption {
    /// `View Hyper Arts list`, the Triangle at `(12, 170)`.
    HyperArtsList,
    /// `View Next page`, the Triangle at `(162, 154)`.
    NextPage,
}

impl ArtsListDriverCaption {
    /// The Triangle sprite's stage seat (`FUN_8002C488`'s `x, y`); the text
    /// pen is `16, 2` further on.
    pub fn glyph_seat(self) -> (i16, i16) {
        match self {
            Self::HyperArtsList => (12, 170),
            Self::NextPage => (162, 154),
        }
    }
}

/// One frame's output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtsListDriverFrame {
    /// The window's `x`, written to `*(0x801F4E0C) + 0xA`.
    pub window_x: i16,
    pub caption: Option<ArtsListDriverCaption>,
}

impl ArtsListDriver {
    /// One frame. `None` (and nothing changed) while unregistered.
    pub fn tick(&mut self, i: &ArtsListDriverInputs) -> Option<ArtsListDriverFrame> {
        if self.flags & ARTS_LIST_REGISTERED == 0 {
            return None;
        }
        if self.flags & ARTS_LIST_OPEN != 0 && i.entering {
            self.slide = self.slide.wrapping_add(i.frame_step);
            if self.slide > ARTS_LIST_SLIDE_SEATED {
                self.slide = ARTS_LIST_SLIDE_SEATED;
            }
        } else {
            self.slide = self.slide.saturating_sub(i.frame_step);
        }
        let window_x = (i32::from(self.slide) * 160 / 12) as i16 + ARTS_LIST_OFFSCREEN_X;
        if self.slide == 0 && !i.entering {
            self.flags = 0;
            self.latch = 0;
        }
        if self.flags & ARTS_LIST_OPEN == 0 && self.latch == 0 && i.entering && i.learned != 0 {
            self.latch = 1;
        }
        let caption = if self.latch != 0 {
            Some(ArtsListDriverCaption::HyperArtsList)
        } else {
            let more = match i.row_offset {
                0 => i.learned >= 6,
                5 => i.learned >= 11,
                _ => false,
            };
            (more && i.entering).then_some(ArtsListDriverCaption::NextPage)
        };
        Some(ArtsListDriverFrame { window_x, caption })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(entering: bool, learned: u8, row_offset: u32) -> ArtsListDriverInputs {
        ArtsListDriverInputs {
            entering,
            frame_step: 2,
            learned,
            row_offset,
        }
    }

    #[test]
    fn unregistered_does_nothing() {
        let mut d = ArtsListDriver {
            flags: ARTS_LIST_OPEN,
            slide: 5,
            latch: 1,
        };
        assert_eq!(d.tick(&inputs(true, 3, 0)), None);
        assert_eq!(d.slide, 5);
    }

    #[test]
    fn the_open_list_slides_in_and_seats() {
        let mut d = ArtsListDriver {
            flags: ARTS_LIST_REGISTERED | ARTS_LIST_OPEN,
            ..Default::default()
        };
        for _ in 0..10 {
            d.tick(&inputs(true, 7, 0));
        }
        assert_eq!(d.slide, ARTS_LIST_SLIDE_SEATED);
        let f = d.tick(&inputs(true, 7, 0)).unwrap();
        assert_eq!(f.window_x, 14);
        assert_eq!(f.caption, Some(ArtsListDriverCaption::NextPage));
    }

    #[test]
    fn a_closed_caption_lingers_through_the_slide_out() {
        let mut d = ArtsListDriver {
            flags: ARTS_LIST_REGISTERED,
            slide: 6,
            latch: 0,
        };
        let f = d.tick(&inputs(true, 1, 0)).unwrap();
        assert_eq!(f.caption, Some(ArtsListDriverCaption::HyperArtsList));
        // The entry ends: the caption stays while the window drains.
        let f = d.tick(&inputs(false, 1, 0)).unwrap();
        assert_eq!(f.caption, Some(ArtsListDriverCaption::HyperArtsList));
        let f = d.tick(&inputs(false, 1, 0)).unwrap();
        assert_eq!((d.slide, d.flags, f.caption), (0, 0, None));
        assert_eq!(d.tick(&inputs(false, 1, 0)), None, "deregistered");
    }
}
