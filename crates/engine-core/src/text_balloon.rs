//! The `4C E1` single-line **text balloon** - the field-VM's screen-anchored
//! one-caption presenter.
//!
//! Two retail halves, both ported here:
//!
//! - **Spawner** `FUN_8003C764` (SCUS): allocates a text actor from the
//!   effect-widget pool (`FUN_80020DE0(&DAT_8007431C, _DAT_8007C34C)`),
//!   stores the page text at `+0x90` and the parent-ctx link at `+0x94`,
//!   zeroes the timer `+0x54`, seeds the display total `+0x9C = 0x78`
//!   (120 frames), measures the line's pixel width (`FUN_80035F04`) and
//!   centers it: `X = (0x140 - width) >> 1`, `Y = 0xB4`.
//! - **Handler** `FUN_801DA7F0` (field overlay, PROT 0897): the per-tick
//!   actor body. Kills any live predecessor balloon (list-find via
//!   `FUN_8003CF04`, kill bit `+0x10 |= 8`), runs the parent-link /
//!   player-engaged handshake, advances the timer by the frame-delta byte
//!   `DAT_1F800393`, and while running draws the text
//!   (`FUN_80036888(text, 0, 0, x, y)`) over the balloon widget frame
//!   (`FUN_80034B6C(3)` + `FUN_8002C69C(0x58, y, 0x90, 0xB)`). At
//!   timer >= total it sets its own kill bit.
//!
//! **This is not the opening narration crawl** - the crawl is the `CC F8
//! 80 N` roller actor (`FUN_80037174`, see [`crate::cutscene_narration`]).
//! See `docs/subsystems/cutscene.md` and `docs/reference/functions.md`.
//!
//! ## The parent-link handshake
//!
//! The spawner stores the spawning script ctx pointer at `+0x94`. The
//! handler pairs it with the **player-engaged flag** (`_DAT_8007C364`
//! `+0x10` bit `0x80000` - the player context's engaged/walking gate):
//!
//! - link set + flag **clear** -> clear the link (the spawning engagement
//!   ended);
//! - link clear + flag **set** -> jump the timer to the total (the player
//!   engaged something new; the balloon ends early).
//!
//! So the balloon survives its spawning engagement, then any *new*
//! engagement dismisses it before the 120-frame timer expires.

/// Retail screen width the centering math uses (`li v1,0x140`).
pub const BALLOON_SCREEN_W: i16 = 0x140;
/// Fixed screen Y (`li v0,0xb4` = 180).
pub const BALLOON_Y: i16 = 0xB4;
/// Display total in timer units (`li v0,0x78` = 120 frames at cadence 1).
pub const BALLOON_TOTAL: i16 = 0x78;
/// Widget-frame emitter x argument (`li a0,0x58` at `0x801DA90C`).
pub const BALLOON_FRAME_X: i16 = 0x58;
/// Widget-frame emitter third argument (`li a2,0x90`).
pub const BALLOON_FRAME_ARG2: i16 = 0x90;
/// Widget-frame emitter fourth argument (`li a3,0xb`).
pub const BALLOON_FRAME_ARG3: i16 = 0xB;
/// Widget kind staged via `FUN_80034B6C(3)` before the frame emit.
pub const BALLOON_FRAME_KIND: u8 = 3;

/// Center a measured line: `X = (0x140 - width) >> 1`.
///
/// The `width` is the line's pixel width as retail's measurer
/// (`FUN_80035F04`) returns it; hosts measure through their font layout.
pub fn balloon_center_x(text_width_px: i16) -> i16 {
    (BALLOON_SCREEN_W - text_width_px) >> 1
}

/// One `4C E1` balloon actor. See the module docs for the retail split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBalloon {
    /// Raw page bytes (the dialog-markup-encoded line the field-VM operand
    /// stream carried; retail stores the pointer at `+0x90`).
    pub text: Vec<u8>,
    /// Screen X (`+0x14`). `None` until a width measurement arrives
    /// ([`Self::center_with_width`]) - retail measures at spawn, but the
    /// engine's font atlas lives host-side.
    pub x: Option<i16>,
    /// Screen Y (`+0x16`), always [`BALLOON_Y`].
    pub y: i16,
    /// Frame timer (`+0x54`), starts at 0.
    pub timer: i16,
    /// Display total (`+0x9C`), [`BALLOON_TOTAL`].
    pub total: i16,
    /// Parent-ctx link (`+0x94` non-null). See the module docs.
    pub parent_link: bool,
    /// Kill bit (`+0x10 & 8`). A killed balloon is dropped by its owner.
    pub killed: bool,
}

/// Outcome of one [`TextBalloon::tick`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BalloonTick {
    /// Timer still in the startup band (`< 1`) - nothing drawn.
    Startup,
    /// Balloon visible this frame: draw `text` at `(x, y)` over the
    /// widget frame (kind [`BALLOON_FRAME_KIND`], x [`BALLOON_FRAME_X`]).
    Draw,
    /// The balloon killed itself (timer expired). Owner drops it.
    Killed,
}

impl TextBalloon {
    /// PORT: FUN_8003C764
    ///
    /// Spawn a balloon for `text`. Retail kills any live predecessor via
    /// the handler's first lines - the engine models that by the owner
    /// **replacing** its current balloon with the spawned one. The X
    /// centering needs a font measurement; call
    /// [`Self::center_with_width`] once the host has one, or construct
    /// via [`Self::spawn_measured`] when the width is already known.
    pub fn spawn(text: &[u8]) -> Self {
        Self {
            text: text.to_vec(),
            x: None,
            y: BALLOON_Y,
            timer: 0,
            total: BALLOON_TOTAL,
            parent_link: true,
            killed: false,
        }
    }

    /// [`Self::spawn`] with the width measurement already in hand -
    /// matches the retail spawner exactly (measure + center inline).
    pub fn spawn_measured(text: &[u8], text_width_px: i16) -> Self {
        let mut b = Self::spawn(text);
        b.center_with_width(text_width_px);
        b
    }

    /// Apply the retail centering (`X = (0x140 - width) >> 1`).
    pub fn center_with_width(&mut self, text_width_px: i16) {
        self.x = Some(balloon_center_x(text_width_px));
    }

    /// PORT: FUN_801DA7F0
    ///
    /// One handler tick. `player_engaged` is the `_DAT_8007C364 +0x10`
    /// bit `0x80000` (the engine's `+0x10 & 0x80000` engaged/walking
    /// gate); `cadence` is the frame-delta byte `DAT_1F800393` (1 at 60
    /// FPS field cadence). The predecessor-kill runs at the owner
    /// (spawn-replaces); everything else follows the retail body:
    ///
    /// 1. Parent-link handshake (see module docs).
    /// 2. `timer < 1` -> `timer += 1`, [`BalloonTick::Startup`].
    /// 3. `timer < total` -> `timer += cadence`, [`BalloonTick::Draw`].
    /// 4. else -> kill bit set, [`BalloonTick::Killed`].
    pub fn tick(&mut self, player_engaged: bool, cadence: i16) -> BalloonTick {
        if self.killed {
            return BalloonTick::Killed;
        }
        // Parent-link handshake (0x801DA838..0x801DA88C).
        if self.parent_link {
            if !player_engaged {
                self.parent_link = false;
            }
        } else if player_engaged {
            // Fast-forward: +0x54 = +0x9C.
            self.timer = self.total;
        }
        // Timer state machine (0x801DA890..).
        if self.timer < 1 {
            self.timer += 1;
            BalloonTick::Startup
        } else if self.timer < self.total {
            self.timer += cadence;
            BalloonTick::Draw
        } else {
            self.killed = true;
            BalloonTick::Killed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centering_matches_retail_arithmetic() {
        // (0x140 - w) >> 1.
        assert_eq!(balloon_center_x(0), 0xA0);
        assert_eq!(balloon_center_x(0x90), 0x58); // frame-width case
        assert_eq!(balloon_center_x(0x140), 0);
        // Wider than the screen: arithmetic shift keeps the sign
        // (sra in the original).
        assert_eq!(balloon_center_x(0x142), -1);
    }

    #[test]
    fn spawn_seeds_retail_fields() {
        let b = TextBalloon::spawn_measured(b"hi", 0x20);
        assert_eq!(b.y, 0xB4);
        assert_eq!(b.timer, 0);
        assert_eq!(b.total, 0x78);
        assert!(b.parent_link);
        assert!(!b.killed);
        assert_eq!(b.x, Some((0x140 - 0x20) >> 1));
    }

    #[test]
    fn startup_tick_then_draw_until_total() {
        let mut b = TextBalloon::spawn_measured(b"x", 0x10);
        // Timer 0 -> startup band, +1 per tick.
        assert_eq!(b.tick(true, 1), BalloonTick::Startup);
        assert_eq!(b.timer, 1);
        // Draw band: timer advances by cadence until total.
        let mut draws = 0;
        loop {
            match b.tick(true, 1) {
                BalloonTick::Draw => draws += 1,
                BalloonTick::Killed => break,
                BalloonTick::Startup => panic!("startup after leaving band"),
            }
        }
        // Timer went 1..0x78 by 1 per draw tick = 0x77 draws.
        assert_eq!(draws, 0x77);
        assert!(b.killed);
    }

    #[test]
    fn cadence_two_halves_the_display_time() {
        let mut b = TextBalloon::spawn_measured(b"x", 0x10);
        b.tick(true, 2); // startup: still +1
        assert_eq!(b.timer, 1);
        let mut draws = 0;
        while b.tick(true, 2) == BalloonTick::Draw {
            draws += 1;
        }
        // 1 -> 0x78 by 2 per tick: ceil(0x77 / 2) = 60 draws.
        assert_eq!(draws, 60);
    }

    #[test]
    fn parent_link_clears_when_engagement_ends() {
        let mut b = TextBalloon::spawn_measured(b"x", 0x10);
        assert!(b.parent_link);
        // Engaged: link survives.
        b.tick(true, 1);
        assert!(b.parent_link);
        // Engagement drops: link clears, balloon keeps running.
        assert_eq!(b.tick(false, 1), BalloonTick::Draw);
        assert!(!b.parent_link);
        assert!(!b.killed);
    }

    #[test]
    fn re_engagement_after_link_clear_ends_balloon_early() {
        let mut b = TextBalloon::spawn_measured(b"x", 0x10);
        b.tick(false, 1); // startup; link clears (not engaged)
        assert!(!b.parent_link);
        b.tick(false, 1); // draw
        // Player engages something new: timer jumps to total, and this
        // same tick falls into the kill arm.
        assert_eq!(b.tick(true, 1), BalloonTick::Killed);
        assert!(b.killed);
    }

    #[test]
    fn killed_balloon_stays_killed() {
        let mut b = TextBalloon::spawn_measured(b"x", 0x10);
        b.killed = true;
        assert_eq!(b.tick(true, 1), BalloonTick::Killed);
    }
}
