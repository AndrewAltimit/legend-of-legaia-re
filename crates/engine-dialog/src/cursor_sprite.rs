//! The animated cursor sprite primitive `FUN_8002B994(kind, mode, x, y)` -
//! its animation half: which frame of a kind's strip is up, and how far the
//! idle bob has pushed it.
//!
//! Every hand, pager triangle and page mark in the game goes through this one
//! SCUS routine. It reads a four-record table at `0x80073D18` (stride `0x18`:
//! `+0` frame count, `+1` CLUT index over `0x7FC0`, `+2` frame period in
//! vsyncs, `+4` / `+6` the position of the previous call, `+8 + f*4` frame
//! `f`'s texel origin on the system-UI page) and two per-kind words: the
//! frame index at `0x801C6000 + kind*4` and the timer at
//! `0x801C6010 + kind*4`. It draws one `16 x 16` `POLY_FT4` (`0x2E808080`,
//! tpage `0x1E`) at `(x, y)`.
//!
//! | kind | sprite | frames | period |
//! |---|---|---|---|
//! | 0 | pointing hand | 1 | 32 |
//! | 1 | page mark (the "press" icon of a waiting dialogue page) | 2 | 16 |
//! | 2 | left triangle | 1 | 32 |
//! | 3 | right triangle | 1 | 32 |
//!
//! `mode 1` is the animated call: a call at a new position resets the kind's
//! frame and timer (`0x8002BA00..0x8002BA38`); a call at the same position
//! adds the frame step `DAT_1F800393` to the timer and, once the timer
//! reaches the period, takes the period off and steps the frame, wrapping at
//! the count (`0x8002BA3C..0x8002BA98`). Any non-zero mode then bobs kinds
//! `0`, `2` and `3` by the eight-entry offset table at `0x80073D78`, indexed
//! `(timer * 2) & 0x38` - four vsyncs an entry - added to the position for
//! kinds `0` and `2`, subtracted in X for kind `3`
//! (`0x8002BADC..0x8002BB7C`). Kind `1` never bobs; its motion is the frame
//! flip. Mode `0` draws frame-current, unmoved.
//!
//! The state is global in retail - one frame / timer pair per kind, whoever
//! calls - so [`CursorSprites`] is held once by the owner of the screen and
//! lent to each caller.
//!
//! See `ghidra/scripts/funcs/8002b994.txt`.

/// One record of the `0x80073D18` table, less its mutable last-position pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorRecord {
    /// `+0`: frames in the strip.
    pub frames: u8,
    /// `+2`: vsyncs a frame holds.
    pub period: i16,
    /// `+8 + f*4`: each frame's texel origin on the system-UI page (tpage
    /// `0x1E`, CLUT row 7). Entries past [`Self::frames`] are unused.
    pub uv: [(u8, u8); 2],
}

/// The four records at `0x80073D18`.
pub const CURSOR_RECORDS: [CursorRecord; 4] = [
    CursorRecord {
        frames: 1,
        period: 32,
        uv: [(152, 64), (0, 0)],
    },
    CursorRecord {
        frames: 2,
        period: 16,
        uv: [(224, 64), (240, 64)],
    },
    CursorRecord {
        frames: 1,
        period: 32,
        uv: [(168, 8), (0, 0)],
    },
    CursorRecord {
        frames: 1,
        period: 32,
        uv: [(168, 40), (0, 0)],
    },
];

/// The idle-bob offsets at `0x80073D78`, `(dx, dy)` per four vsyncs of the
/// kind's timer.
pub const CURSOR_BOB: [(i32, i32); 8] = [
    (2, 0),
    (2, 0),
    (2, 0),
    (2, 0),
    (1, 0),
    (0, 0),
    (-1, 0),
    (-2, 0),
];

/// Kind `0`: the pointing hand.
pub const KIND_HAND: usize = 0;
/// Kind `1`: the page mark a waiting dialogue page shows.
pub const KIND_PAGE_MARK: usize = 1;

/// What one call draws: the strip frame and the bob added to the position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CursorPose {
    pub frame: u8,
    pub dx: i32,
    pub dy: i32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct KindState {
    /// `0x801C6000 + kind*4`.
    frame: i32,
    /// `0x801C6010 + kind*4`.
    timer: i32,
    /// Record `+4` / `+6`; `None` is the table's initial `(500, 500)`, a
    /// position no caller passes.
    last: Option<(i16, i16)>,
}

/// The primitive's per-kind frame / timer / last-position state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CursorSprites {
    kinds: [KindState; 4],
}

impl CursorSprites {
    /// One animated (`mode 1`) call for `kind` at `(x, y)` on a game frame
    /// `frame_step` vsyncs long. Returns the frame and bob this call draws.
    /// A `kind` past the table draws nothing in retail (`slti v0,t3,4`) and
    /// answers the default pose here.
    ///
    /// PORT: FUN_8002B994 (mode-1 frame timer, position reset and idle bob, `0x8002B9D4..0x8002BB7C`)
    pub fn call(&mut self, kind: usize, x: i16, y: i16, frame_step: u8) -> CursorPose {
        let Some(rec) = CURSOR_RECORDS.get(kind) else {
            return CursorPose::default();
        };
        let st = &mut self.kinds[kind];
        let frames = i32::from(rec.frames);
        if st.frame >= frames {
            st.frame = 0;
        }
        if st.last != Some((x, y)) {
            st.frame = 0;
            st.timer = 0;
            st.last = Some((x, y));
        } else {
            st.timer += i32::from(frame_step);
            let period = i32::from(rec.period);
            if st.timer >= period {
                st.timer -= period;
                st.frame += 1;
                if st.frame >= frames {
                    st.frame = 0;
                }
            }
        }
        let (bx, by) = CURSOR_BOB[((st.timer << 1) & 0x38) as usize >> 3];
        let (dx, dy) = match kind {
            0 | 2 => (bx, by),
            3 => (-bx, by),
            _ => (0, 0),
        };
        CursorPose {
            frame: st.frame as u8,
            dx,
            dy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page mark flips between its two frames every sixteen vsyncs and
    /// never bobs.
    #[test]
    fn the_page_mark_flips_every_period() {
        let mut c = CursorSprites::default();
        // The first call at a position is the reset.
        assert_eq!(
            c.call(KIND_PAGE_MARK, 0x10A, 0x1A, 2),
            CursorPose::default()
        );
        let mut frames = Vec::new();
        for _ in 0..24 {
            let p = c.call(KIND_PAGE_MARK, 0x10A, 0x1A, 2);
            assert_eq!((p.dx, p.dy), (0, 0));
            frames.push(p.frame);
        }
        // Timer 2, 4, .. 14 hold frame 0; the eighth call reaches 16.
        assert_eq!(&frames[..7], &[0; 7]);
        assert_eq!(&frames[7..15], &[1; 8]);
        assert_eq!(&frames[15..23], &[0; 8]);
        assert_eq!(frames[23], 1);
    }

    /// The hand has one frame; its motion is the bob - two pixels out for
    /// half the 32-vsync cycle, then a sweep back through `1, 0, -1, -2`.
    #[test]
    fn the_hand_bobs_over_its_period() {
        let mut c = CursorSprites::default();
        let mut dxs = vec![c.call(KIND_HAND, 0xD2, 0x4A, 1).dx];
        for _ in 0..32 {
            let p = c.call(KIND_HAND, 0xD2, 0x4A, 1);
            assert_eq!(p.frame, 0);
            dxs.push(p.dx);
        }
        let mut want = Vec::new();
        for d in [2, 2, 2, 2, 1, 0, -1, -2] {
            want.extend([d; 4]);
        }
        want.push(2); // the wrap
        assert_eq!(dxs, want);
    }

    /// A call at a new position restarts the kind, and the kinds do not
    /// share a timer.
    #[test]
    fn a_new_position_restarts_the_kind() {
        let mut c = CursorSprites::default();
        for _ in 0..12 {
            c.call(KIND_PAGE_MARK, 0x10A, 0x1A, 2);
        }
        assert_eq!(c.call(KIND_PAGE_MARK, 0x10A, 0x1A, 2).frame, 1);
        // The hand's own first call does not disturb it.
        c.call(KIND_HAND, 0, 0, 2);
        assert_eq!(c.call(KIND_PAGE_MARK, 0x10A, 0x1A, 2).frame, 1);
        // A taller box moves the mark: back to frame 0.
        assert_eq!(c.call(KIND_PAGE_MARK, 0x10A, 0x29, 2).frame, 0);
    }

    /// The right triangle mirrors the bob in X.
    #[test]
    fn the_right_triangle_mirrors_the_bob() {
        let mut c = CursorSprites::default();
        assert_eq!(c.call(3, 10, 10, 1).dx, -2);
        assert_eq!(c.call(2, 10, 10, 1).dx, 2);
    }
}
