//! The field dialog pager's **picker slide** - how an option menu enters the
//! screen, and when it starts taking input.
//!
//! PORT: FUN_801D84D0 (the picker entry states `0x11` / `0x13` / `0x15` / `0x17` and the picker draw, `0x801D92F4..0x801D9418`, `0x801D9A08..0x801D9BE4`)
//!
//! Read off the pager's disassembly (field overlay, PROT 0897; jump table
//! `0x801CEBC0`):
//!
//! - **The press.** Every confirm in state `0x19` stores the sentinel
//!   `+0x54 = 0x309` on the pager actor (`0x801D90A0..0x801D90B8`). While the
//!   count is the sentinel the draw skips the picker box (`0x801D9A30`).
//! - **The first call** in the entry state sees the sentinel and initialises
//!   span `+0x50` and count `+0x54` to `0x18`, the start `(+0x3C, +0x3E)`,
//!   the target `(+0x14, +0x16)` and the size `(+0x24, +0x26)` - the
//!   geometry [`legaia_mes::picker_slide_start`] /
//!   [`legaia_mes::picker_box_rect`] carry. It does not count down.
//! - **Every later call** subtracts the frame step (`s2`) from the count; the
//!   call that takes it to zero or below stores 0, steps to the active state
//!   and branches to the draw (`0x801D93F4..0x801D9418`) - so the cursor
//!   handler (`0x801D941C`) first runs on the call after.
//! - **The draw** places the box at `target + (start - target) * count /
//!   span` per axis (`mult` + signed `div`, truncating), labels travelling
//!   with it; the option hand (`FUN_8002B994` kind 0) is drawn only at count
//!   0 (`0x801D9BB4..0x801D9BE4`).
//!
//! PCSX-Redux captures pin both slides at frame step 2 (`docs/formats/mes.md`,
//! "The picker slide"): the `0x2A` inn box runs x `336, 326, .. 226` then
//! `216` over thirteen pager calls, and Tetsu's 4-option `0x29` list runs y
//! `240, 232, 224, 217, .. 155` then `148`. Width and height never change.

/// The count value a press leaves behind: "slide not started, box hidden".
pub const SLIDE_SENTINEL: i16 = 0x309;

/// Span (and initial count) the entry arms write: `0x18` frame-step units.
pub const SLIDE_SPAN: i16 = 0x18;

/// One picker's slide state - the pager actor's `+0x14..+0x54` slide words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickerSlide {
    /// Remaining count (`+0x54`); [`SLIDE_SENTINEL`] before the first call.
    pub count: i16,
    /// Span (`+0x50`).
    pub span: i16,
    /// Start position (`+0x3C`, `+0x3E`).
    pub start: (i16, i16),
    /// Resting position (`+0x14`, `+0x16`).
    pub target: (i16, i16),
    /// Box size (`+0x24`, `+0x26`).
    pub size: (i16, i16),
    /// The count reached zero on an earlier pager call, so the active state's
    /// cursor handler runs - the call that zeroes the count only draws.
    settled: bool,
}

impl PickerSlide {
    /// The state a press in `0x19` leaves for a picker whose open byte is
    /// `open_byte`: sentinel count, box hidden. The geometry is filled here
    /// (retail writes it on the first call; nothing reads it before).
    pub fn pressed(open_byte: u8) -> Self {
        let (x, y, w, h) =
            legaia_mes::picker_box_rect(open_byte).unwrap_or((0x26, 0x94, 0xF4, 0x38));
        let start = legaia_mes::picker_slide_start(open_byte).unwrap_or((0x26, 0xF0));
        Self {
            count: SLIDE_SENTINEL,
            span: SLIDE_SPAN,
            start,
            target: (x, y),
            size: (w, h),
            settled: false,
        }
    }

    /// A slide already at rest - for a path that opens its menu without the
    /// press (the simplified panel), where no pager call animates it.
    pub fn at_rest(open_byte: u8) -> Self {
        let mut s = Self::pressed(open_byte);
        s.count = 0;
        s.settled = true;
        s
    }

    /// One pager call at frame step `dt` (`DAT_1F800393`).
    pub fn call(&mut self, dt: u8) {
        if self.count == SLIDE_SENTINEL {
            self.span = SLIDE_SPAN;
            self.count = SLIDE_SPAN;
            return;
        }
        if self.count == 0 {
            self.settled = true;
            return;
        }
        let next = self.count.wrapping_sub(i16::from(dt.max(1)));
        self.count = next.max(0);
    }

    /// The box's centre rect `(x, y, w, h)` this frame, or `None` while the
    /// count is the sentinel (retail draws no box then).
    pub fn rect(&self) -> Option<(i32, i32, i32, i32)> {
        if self.count == SLIDE_SENTINEL {
            return None;
        }
        let axis = |start: i16, target: i16| -> i32 {
            let (start, target) = (i32::from(start), i32::from(target));
            if self.count == 0 || self.span == 0 {
                return target;
            }
            // Signed `div`, truncating toward zero - Rust's `/` on i32.
            target + (start - target) * i32::from(self.count) / i32::from(self.span)
        };
        Some((
            axis(self.start.0, self.target.0),
            axis(self.start.1, self.target.1),
            i32::from(self.size.0),
            i32::from(self.size.1),
        ))
    }

    /// The option hand is drawn (count 0).
    pub fn hand_drawn(&self) -> bool {
        self.count == 0
    }

    /// The active state's cursor handler runs: the count reached zero on an
    /// earlier pager call.
    pub fn takes_input(&self) -> bool {
        self.settled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run pager calls from the press until input opens, returning the box
    /// origin each call drew and the call on which input opened.
    fn run(open_byte: u8, dt: u8) -> (Vec<Option<(i32, i32)>>, usize) {
        let mut s = PickerSlide::pressed(open_byte);
        assert_eq!(s.rect(), None, "the press leaves the box hidden");
        let mut origins = Vec::new();
        for call in 1..=64 {
            s.call(dt);
            if s.takes_input() {
                return (origins, call);
            }
            origins.push(s.rect().map(|(x, y, _, _)| (x, y)));
        }
        panic!("slide never settled");
    }

    /// The captured `0x2A` inn slide (`retock_inn_stay_prompt`, frame step
    /// 2): x 336 -> 216 at 10 px a call over thirteen calls, y fixed at 74;
    /// the hand and the rest rect on call 13, input on the call after.
    #[test]
    fn inn_box_enters_from_the_right_as_captured() {
        let (origins, input_call) = run(0x2A, 2);
        let xs: Vec<i32> = origins.iter().map(|o| o.unwrap().0).collect();
        let want: Vec<i32> = (0..13).map(|i| 336 - 10 * i).collect();
        assert_eq!(xs, want);
        assert!(origins.iter().all(|o| o.unwrap().1 == 74));
        assert_eq!(input_call, 14);
        let mut s = PickerSlide::pressed(0x2A);
        for _ in 0..13 {
            s.call(2);
        }
        assert!(s.hand_drawn());
        assert!(!s.takes_input(), "the zeroing call only draws");
        assert_eq!(s.rect(), Some((0xD8, 0x4A, 0x58, 0x1A)));
    }

    /// The captured 4-option `0x29` rise (`town01_tetsu_topic_prompt`, frame
    /// step 2): x fixed at 38, y 240, 232, 224, 217, .. 155 then 148, the
    /// whole 244 x 56 box from the first drawn call.
    #[test]
    fn four_option_list_rises_as_captured() {
        let (origins, input_call) = run(0x29, 2);
        let ys: Vec<i32> = origins.iter().map(|o| o.unwrap().1).collect();
        assert_eq!(
            ys,
            vec![
                240, 232, 224, 217, 209, 201, 194, 186, 178, 171, 163, 155, 148
            ]
        );
        assert!(origins.iter().all(|o| o.unwrap().0 == 38));
        assert_eq!(input_call, 14);
        let s = PickerSlide::pressed(0x29);
        assert_eq!(s.size, (244, 56));
    }

    /// The 2- and 3-option lists share the start and the count sequence; only
    /// the target differs (`0x94 + ((4-N)*0xF)/2`).
    #[test]
    fn two_and_three_option_lists_rise_to_their_own_targets() {
        let (two, _) = run(0x27, 2);
        let (three, _) = run(0x28, 2);
        assert_eq!(two.first().unwrap().unwrap(), (38, 240));
        assert_eq!(three.first().unwrap().unwrap(), (38, 240));
        assert_eq!(two.last().unwrap().unwrap(), (38, 0xA3));
        assert_eq!(three.last().unwrap().unwrap(), (38, 0x9B));
        // 240 - 163 = 77 px over the span: 163 + 77*22/24 = 233 on call 2.
        assert_eq!(two[1].unwrap().1, 0xA3 + 77 * 22 / 24);
        assert_eq!(two.len(), 13);
        assert_eq!(three.len(), 13);
    }

    /// At 60 fps pacing (frame step 1) the same span takes twice the calls.
    #[test]
    fn frame_step_one_doubles_the_calls() {
        let (origins, input_call) = run(0x2A, 1);
        assert_eq!(origins.len(), 25);
        assert_eq!(input_call, 26);
        assert_eq!(origins[1].unwrap().0, 216 + 120 * 23 / 24);
    }

    /// The simplified panel's menu is at rest and takes input at once.
    #[test]
    fn at_rest_takes_input_immediately() {
        let s = PickerSlide::at_rest(0x27);
        assert!(s.takes_input() && s.hand_drawn());
        assert_eq!(s.rect(), Some((0x26, 0xA3, 0xF4, 0x1A)));
    }
}
