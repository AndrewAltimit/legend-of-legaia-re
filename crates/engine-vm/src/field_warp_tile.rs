//! The field walk-on dispatcher's **timed warp**: `FUN_801D1EC4` (PROT 0897,
//! base `0x801CE818`, file offset `0x36AC`, `0x801D1EC4..0x801D2294`), and the
//! two gates the player's own tick `FUN_801D1344` puts in front of the pad.
//!
//! `FUN_801D1EC4` is reached once per frame from the settle
//! `FUN_801D1BA0` (`jal` at `0x801D1D80`), and it has two halves keyed on the
//! warp timer `_DAT_8007B6B0`:
//!
//! * **Timer running** (`> 0`, `0x801D1EF4..0x801D2064`): it tags for
//!   tear-down (`+0x10 |= 8`) the first live pool actor whose tick handler
//!   `+0x0C` is `0x801DA7F0` (`FUN_8003CF04` walks the list for it) - a
//!   singleton that reads its own `+0x94` encounter record - then subtracts
//!   the frame delta `DAT_1F800393` and returns while the result is still
//!   positive. The frame it reaches zero is the
//!   **landing**: the post-warp hold `_DAT_8007B6B4 = 0x28`, the player's
//!   movement lock `0x80000` cleared, the encounter step counter re-rolled
//!   when it is `<= 0` (`jal 0x801DDF48` at `0x801D1F6C`, guarded by the
//!   `bgtz` at `0x801D1F64`), the timer parked at `-1000`, the player seated
//!   at `(dest_x * 64 + 64, (dest_z + 1) * 64)`, the crossing tile re-stamped,
//!   the camera re-pinned (`FUN_80017EC8`, `FUN_801DE3E0`, `FUN_801DB8EC`,
//!   `FUN_801DAA50`), the floor re-sampled into `+0x16`, and the landing tile's
//!   kind-1 walk-on record run - queried at `(dest_x >> 1, dest_z >> 1)`.
//! * **Timer idle** (`<= 0`, `0x801D2068..0x801D227C`): the per-frame tile
//!   compare against the crossing tile `(_DAT_8007BDC8, _DAT_8007BDCC)`, the
//!   `cell & 0x600` object-index filter, the kind-1 walk-on arm (and, under
//!   `_DAT_8007B6A8`, the clip base `_DAT_8007BDD8 = 2` plus the player's
//!   party-bank bit), then the kind-0 arm, which **arms** the timer rather
//!   than teleporting: `_DAT_8007B6B0 = 0x26`, destination
//!   `(_DAT_8007BDD0, _DAT_8007BDD4) = (rec[2], rec[3])`, and two fades
//!   through `FUN_801D58F0` ([`WARP_FADE_OUT`] / [`WARP_FADE_IN`]).
//!
//! The kind-0 destination is therefore reached **`0x26` frames after** the
//! crossing, at the bottom of the fade, not on the crossing frame.
//!
//! The player tick `FUN_801D1344` is what makes the warp a pause rather than
//! a slide: it drains `_DAT_8007B6B4` by the frame delta and clamps it at zero
//! (`0x801D161C..0x801D1630`), and skips the pad controller `FUN_801D01B0`
//! entirely while `_DAT_8007B6B0 > 0` or `_DAT_8007B6B4 != 0`
//! (`0x801D16C8..0x801D16E4`). The settle's hop gate reads the same pair
//! (`0x801D1C6C..0x801D1C88`).
//!
//! `_DAT_8007B6B0` is not only this warp's word: `-1000` is the "landed"
//! sentinel other readers test, and the world-map controller writes it too.
//! It lives for the rest of the landing tick only: the field overlay's entity
//! tick `FUN_801DA51C`, which runs the system channel after the player, ends
//! by comparing the word with `-0x3e8` and storing `0` (`0x801DA7C8..0x801DA7D8`)
//! - see [`clear_landed_sentinel`].

/// `_DAT_8007B6B0` on the frame a kind-0 crossing arms the warp
/// (`addiu v0, zero, 0x26` at `0x801D2214`).
pub const WARP_TIMER_FRAMES: i32 = 0x26;

/// `_DAT_8007B6B0` after a landing (`addiu v0, zero, -0x3e8` at `0x801D1F78`).
pub const WARP_LANDED_SENTINEL: i32 = -1000;

/// `_DAT_8007B6B4` after a landing (`addiu v0, zero, 0x28` at `0x801D1F4C`):
/// how many frames the player tick keeps the pad controller off.
pub const POST_WARP_HOLD_FRAMES: i32 = 0x28;

/// The crossed cell's object-index bits both walk-on arms need
/// (`andi v0, v0, 0x600` at `0x801D2140`).
pub const WALK_ON_CELL_MASK: u16 = 0x600;

/// The player's movement-lock bit, which the landing clears.
pub const MOVEMENT_LOCK: u32 = 0x0008_0000;

/// One `FUN_801D58F0` fade: its six arguments, which it repacks into the
/// 13-halfword template `FUN_80024E80` loads (`[0]` kind, `[1]` duration,
/// `[3..=5]` / `[7..=9]` the two RGB triples unpacked from `0x00BBGGRR`
/// words, `[10]` start delay, `[11]` hold).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarpFade {
    /// `a0` - the fade kind, which is also its blend (`2` = `B - F`).
    pub kind: i16,
    /// Start colour, `0x00BBGGRR`.
    pub from_rgb: u32,
    /// End colour, `0x00BBGGRR`.
    pub to_rgb: u32,
    /// `a3` - frames before the ramp starts.
    pub delay: i16,
    /// Fifth argument - the ramp's length.
    pub duration: i16,
    /// Sixth argument - frames the landed colour holds.
    pub hold: i16,
}

impl WarpFade {
    /// The three bytes of an RGB word, in the template's order.
    pub fn rgb(word: u32) -> [i16; 3] {
        [
            (word & 0xFF) as i16,
            ((word >> 8) & 0xFF) as i16,
            ((word >> 16) & 0xFF) as i16,
        ]
    }
}

/// The first fade (`0x801D21D8..0x801D222C`): a `0x1C`-frame black-to-white
/// ramp under the subtractive blend - a fade **to black** - held `0xE`
/// frames.
pub const WARP_FADE_OUT: WarpFade = WarpFade {
    kind: 2,
    from_rgb: 0,
    to_rgb: 0x00FF_FFFF,
    delay: 0,
    duration: 0x1C,
    hold: 0xE,
};

/// The second fade (`0x801D2234..0x801D224C`): the reverse ramp, delayed
/// `0x29` frames so it starts three frames after the landing.
pub const WARP_FADE_IN: WarpFade = WarpFade {
    kind: 2,
    from_rgb: 0x00FF_FFFF,
    to_rgb: 0,
    delay: 0x29,
    duration: 0x1C,
    hold: 0,
};

/// The warp's globals: `_DAT_8007B6B0`, `_DAT_8007B6B4` and the destination
/// pair `_DAT_8007BDD0` / `_DAT_8007BDD4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WarpTimer {
    /// `_DAT_8007B6B0`.
    pub timer: i32,
    /// `_DAT_8007B6B4` - the post-warp pad hold.
    pub hold: i32,
    /// `(rec[2], rec[3])` of the kind-0 record, in half-tiles.
    pub dest: (u8, u8),
}

/// Where the landing puts the player: `x = dest_x * 64 + 64`
/// (`sll v0, v0, 6; addiu v0, v0, 0x40`), `z = (dest_z + 1) * 64`
/// (`addiu v1, v1, 1; sll v1, v1, 6`). The two forms are the same number.
pub fn landing_world(dest: (u8, u8)) -> (i16, i16) {
    (
        ((i32::from(dest.0) << 6) + 0x40) as i16,
        ((i32::from(dest.1) + 1) << 6) as i16,
    )
}

/// The tile the landing queries its kind-1 record at: the destination halved
/// (`sll 0xf; sra 0x10` at `0x801D2024..0x801D2034`). For an odd destination
/// this is one tile short of the tile the player lands **on** - the crossing
/// tile [`landing_world`] re-stamps is `world >> 7`.
pub fn landing_query_tile(dest: (u8, u8)) -> (u8, u8) {
    (dest.0 >> 1, dest.1 >> 1)
}

/// What one timer-running frame did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarpStep {
    /// The timer was idle: the tile compare runs this frame.
    Idle,
    /// The timer is still running.
    Running,
    /// The timer ran out this frame: seat the player, re-pin the camera and
    /// run the landing tile's walk-on record.
    Landed {
        /// [`landing_world`] of the stored destination.
        world: (i16, i16),
        /// [`landing_query_tile`] of the stored destination.
        query_tile: (u8, u8),
        /// The encounter step counter was `<= 0` and is re-rolled.
        reroll: bool,
    },
}

/// The timer half of `FUN_801D1EC4` (`0x801D1EEC..0x801D1F80`): count the
/// running warp down by `delta` and land it when it runs out.
///
/// `step_counter` is `_DAT_8007B5FC`; the landing re-rolls it only when it is
/// `<= 0` - a counter that still has steps left is carried through the warp.
/// The caller applies the landing's side effects, including clearing the
/// player's [`MOVEMENT_LOCK`].
///
/// PORT: FUN_801D1EC4 (the timer half and the landing, `0x801D1EEC..0x801D2064`)
pub fn tick_warp_timer(warp: &mut WarpTimer, delta: u8, step_counter: i32) -> WarpStep {
    if warp.timer <= 0 {
        return WarpStep::Idle;
    }
    warp.timer -= i32::from(delta);
    if warp.timer > 0 {
        return WarpStep::Running;
    }
    warp.hold = POST_WARP_HOLD_FRAMES;
    warp.timer = WARP_LANDED_SENTINEL;
    WarpStep::Landed {
        world: landing_world(warp.dest),
        query_tile: landing_query_tile(warp.dest),
        reroll: step_counter <= 0,
    }
}

/// The kind-0 arm (`0x801D21D8..0x801D2268`): store the destination and start
/// the timer. The two fades it spawns are [`WARP_FADE_OUT`] and
/// [`WARP_FADE_IN`], in that order; it also clears the player's
/// [`MOVEMENT_LOCK`] (`0x801D2254..0x801D2264`).
///
/// PORT: FUN_801D1EC4 (the kind-0 arm)
pub fn arm_warp(warp: &mut WarpTimer, dest: (u8, u8)) -> [WarpFade; 2] {
    warp.dest = dest;
    warp.timer = WARP_TIMER_FRAMES;
    [WARP_FADE_OUT, WARP_FADE_IN]
}

/// The player tick's drain of the post-warp hold (`0x801D1618..0x801D1630`):
/// subtract the frame delta and clamp at zero.
///
/// PORT: FUN_801D1344 (the `_DAT_8007B6B4` drain)
pub fn drain_post_warp_hold(warp: &mut WarpTimer, delta: u8) {
    warp.hold -= i32::from(delta);
    if warp.hold < 0 {
        warp.hold = 0;
    }
}

/// Whether the player tick skips the pad controller this frame: a running
/// warp or an undrained hold (`0x801D16C8..0x801D16E4`, the `bgtz` on
/// `_DAT_8007B6B0` and the `bnez` on `_DAT_8007B6B4`).
///
/// The settle's hop gate is the same test (`0x801D1C6C..0x801D1C88`).
///
/// PORT: FUN_801D1344 (the pad-controller gate)
pub fn pad_suppressed(warp: &WarpTimer) -> bool {
    warp.timer > 0 || warp.hold != 0
}

/// The tail of the entity tick `FUN_801DA51C` (`0x801DA7C4..0x801DA7D8`,
/// PROT 0897): `lw v1,-0x4950(a0)` / `li v0,-0x3e8` / `bne` / `sw zero,
/// -0x4950(a0)` - a `-1000` landed sentinel in `_DAT_8007B6B0` is cleared to
/// `0`. The system channel runs this tick after the player's settle, so the
/// sentinel a landing parks is gone before the next tick's readers look.
/// Retail reaches the tail only while the channel's `+0x8A` is `0`, the
/// scratchpad dialogue bit `0x1F800394 & 0x8000` is clear and the channel's
/// own `+0x10 & 0x80000` lock is clear (`0x801DA750..0x801DA784`). Returns
/// whether it cleared.
///
/// PORT: FUN_801DA51C (the `-1000` sentinel clear, `0x801DA7C4..0x801DA7D8`)
pub fn clear_landed_sentinel(warp: &mut WarpTimer) -> bool {
    if warp.timer == WARP_LANDED_SENTINEL {
        warp.timer = 0;
        return true;
    }
    false
}

/// Whether a crossed cell passes the object-index filter both walk-on arms
/// sit behind: the scene map word at `*(0x1F8003EC) + 0x8000 + x * 2 + z *
/// 256` ANDed with [`WALK_ON_CELL_MASK`] (`0x801D2104..0x801D2144`).
pub fn cell_admits_walk_on(cell: u16) -> bool {
    cell & WALK_ON_CELL_MASK != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crossing_lands_after_the_timer_not_on_the_frame() {
        let mut w = WarpTimer::default();
        let fades = arm_warp(&mut w, (21, 40));
        assert_eq!(fades, [WARP_FADE_OUT, WARP_FADE_IN]);
        assert!(pad_suppressed(&w));
        for _ in 0..WARP_TIMER_FRAMES - 1 {
            assert_eq!(tick_warp_timer(&mut w, 1, 500), WarpStep::Running);
        }
        let step = tick_warp_timer(&mut w, 1, 500);
        assert_eq!(
            step,
            WarpStep::Landed {
                world: (21 * 64 + 64, 41 * 64),
                query_tile: (10, 20),
                reroll: false,
            }
        );
        assert_eq!(w.timer, WARP_LANDED_SENTINEL);
        assert_eq!(w.hold, POST_WARP_HOLD_FRAMES);
        // Landed: the timer is idle, the hold still holds the pad.
        assert_eq!(tick_warp_timer(&mut w, 1, 500), WarpStep::Idle);
        assert!(pad_suppressed(&w));
        for _ in 0..POST_WARP_HOLD_FRAMES {
            drain_post_warp_hold(&mut w, 1);
        }
        assert!(!pad_suppressed(&w));
    }

    #[test]
    fn the_system_channel_clears_the_landed_sentinel() {
        let mut w = WarpTimer::default();
        arm_warp(&mut w, (4, 4));
        assert!(!clear_landed_sentinel(&mut w), "a running timer is kept");
        assert_eq!(w.timer, WARP_TIMER_FRAMES);
        w.timer = 1;
        assert!(matches!(
            tick_warp_timer(&mut w, 1, 1),
            WarpStep::Landed { .. }
        ));
        assert_eq!(w.timer, WARP_LANDED_SENTINEL);
        assert!(clear_landed_sentinel(&mut w));
        assert_eq!(w.timer, 0, "cleared on the landing tick");
        assert_eq!(w.hold, POST_WARP_HOLD_FRAMES, "the hold is untouched");
        assert!(pad_suppressed(&w), "the hold still keeps the pad off");
        assert!(!clear_landed_sentinel(&mut w));
    }

    #[test]
    fn the_landing_rerolls_only_a_spent_counter() {
        let mut w = WarpTimer::default();
        arm_warp(&mut w, (0, 0));
        w.timer = 1;
        assert!(matches!(
            tick_warp_timer(&mut w, 2, 0),
            WarpStep::Landed { reroll: true, .. }
        ));
        arm_warp(&mut w, (0, 0));
        w.timer = 1;
        assert!(matches!(
            tick_warp_timer(&mut w, 1, 1),
            WarpStep::Landed { reroll: false, .. }
        ));
    }

    #[test]
    fn the_hold_drain_clamps_at_zero() {
        let mut w = WarpTimer {
            hold: 3,
            ..WarpTimer::default()
        };
        drain_post_warp_hold(&mut w, 4);
        assert_eq!(w.hold, 0);
    }

    #[test]
    fn the_fades_bracket_the_landing() {
        // Out lands at 0x1C and holds to 0x2A; in starts at 0x29. The warp
        // lands at 0x26, inside the black.
        let out_end = WARP_FADE_OUT.delay + WARP_FADE_OUT.duration + WARP_FADE_OUT.hold;
        assert!(i32::from(WARP_FADE_OUT.duration) <= WARP_TIMER_FRAMES);
        assert!(i32::from(out_end) > WARP_TIMER_FRAMES);
        assert!(i32::from(WARP_FADE_IN.delay) > WARP_TIMER_FRAMES);
        assert_eq!(WarpFade::rgb(0x00FF_FFFF), [0xFF; 3]);
    }

    #[test]
    fn the_cell_filter() {
        assert!(cell_admits_walk_on(0x0200));
        assert!(cell_admits_walk_on(0x0400));
        assert!(!cell_admits_walk_on(0x01FF));
    }
}
