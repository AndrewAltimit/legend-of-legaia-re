//! The banner slides, the strike splash, the catch-result plate and the banner timers.

use super::*;

/// Shared slide ramp of the two banner animators: `frame * 8` up to the
/// `0xa0` hold (reached at frame `0x14`), held until frame `0x8c`, then
/// `frame * 8 - 0x3c0` sliding off (the ramps join continuously at `0xa0`).
/// Retail leaves the value undefined for a negative frame (the timers only
/// ever pass `>= 1`); this clamps to the frame-0 value.
pub(super) fn banner_slide(frame: i32) -> i32 {
    let mut v = frame.max(0) * 8;
    if frame >= 0x14 {
        v = 0xa0;
    }
    if frame >= 0x8c {
        v = frame * 8 - 0x3c0;
    }
    v
}

/// One frame of the banner that slides in from the **left** (glyph `7` at
/// `y = 0x78`, x = the slide ramp). Its timer (`DAT_801d9160`) is seeded at
/// the moment the fish hooks (`FUN_801d26cc`, alongside the gauge-block
/// enable). Returns the draw while active, `None` once the frame count
/// reaches [`BANNER_FRAMES`] (retail returns the active flag).
// PORT: FUN_801d78ec (hook banner: left slide-in, hold, slide-off)
pub fn banner_from_left_draw(frame: i32) -> Option<HudDraw> {
    if frame >= BANNER_FRAMES {
        return None;
    }
    Some(HudDraw::Glyph {
        layer: 0,
        id: 7,
        x: banner_slide(frame),
        y: 0x78,
        brightness: HUD_BRIGHTNESS_FULL,
    })
}

/// One frame of the banner that slides in from the **right** (glyph `0xd` at
/// `y = 0x78`, x = `0x140 -` the slide ramp - the mirrored trajectory of
/// [`banner_from_left_draw`], holding at the same `x = 0xa0`). Its timer
/// (`DAT_801d915c`) is seeded on the reel-in-complete path of the hooked
/// fight (`FUN_801d26cc`: record below `0x136` while hooked); while it runs,
/// the driver tail forces the from-left timer back to zero.
// PORT: FUN_801d75dc (reel-in banner: right slide-in, hold, slide-off)
pub fn banner_from_right_draw(frame: i32) -> Option<HudDraw> {
    if frame >= BANNER_FRAMES {
        return None;
    }
    Some(HudDraw::Glyph {
        layer: 0,
        id: 0xd,
        x: 0x140 - banner_slide(frame),
        y: 0x78,
        brightness: HUD_BRIGHTNESS_FULL,
    })
}

/// One frame of the miss / retry banner (glyph `0x19` at `y = 0x78`), the
/// mirrored trajectory of [`banner_from_left_draw`] over the shared ramp. Its
/// timer (`DAT_801d9268`) is the retry countdown the driver runs in state
/// `0x2d` before returning to the cast state. `None` once the frame count
/// reaches [`BANNER_FRAMES`] (retail returns the active flag, which is what
/// keeps the state machine parked).
// PORT: FUN_801d6f10 (miss/retry banner: right slide-in, hold, slide-off)
pub fn banner_miss_draw(frame: i32) -> Option<HudDraw> {
    if frame >= BANNER_FRAMES {
        return None;
    }
    Some(HudDraw::Glyph {
        layer: 0,
        id: 0x19,
        x: 0x140 - banner_slide(frame),
        y: 0x78,
        brightness: HUD_BRIGHTNESS_FULL,
    })
}

/// One frame of the auxiliary two-sided banner: the *same* glyph (`0xc`)
/// emitted twice, once at the ramp and once mirrored at `0x140 -` the ramp, so
/// the pair converges on the `0xa0` hold from both edges and parts again on
/// the way out. Its timer (`DAT_801d9164`) is what the driver's state `0x28`
/// waits on before returning to the main loop.
// PORT: FUN_801d7528 (auxiliary banner: mirrored converging glyph pair)
pub fn banner_converge_draws(frame: i32) -> Option<[HudDraw; 2]> {
    if frame >= BANNER_FRAMES {
        return None;
    }
    let x = banner_slide(frame);
    let glyph = |x: i32| HudDraw::Glyph {
        layer: 0,
        id: 0xc,
        x,
        y: 0x78,
        brightness: HUD_BRIGHTNESS_FULL,
    };
    Some([glyph(x), glyph(0x140 - x)])
}

/// The strike-splash brightness ramp: `frame * 8` up to the `0x80` hold
/// (reached at frame `0x10`), held until frame `0x88`, then
/// `0x80 - (frame - 0x88) * 8` fading out (zero exactly at the `0x98`
/// lifetime end).
pub fn splash_brightness(frame: i32) -> i32 {
    let mut a = frame.max(0) * 8;
    if frame >= 0x10 {
        a = 0x80;
    }
    if frame >= 0x88 {
        a = 0x80 - (frame - 0x88) * 8;
    }
    a
}

/// One frame of the strike splash: a two-glyph pair (`0x416` / `0x816`) at
/// `x = 0xa0` that rises one pixel every 32 frames from `y = 0x50` while
/// fading through [`splash_brightness`]. Its timer (`DAT_801d90f0`) is seeded
/// at the strike / hit event before the fish hooks (`FUN_801d26cc`, gated on
/// the gauge block not yet being up). `None` once the frame count reaches
/// [`SPLASH_FRAMES`].
// PORT: FUN_801d71d4 (strike splash: rising, fading two-glyph pair)
pub fn strike_splash_draws(frame: i32) -> Option<[HudDraw; 2]> {
    if frame >= SPLASH_FRAMES {
        return None;
    }
    let y = 0x50 - (frame.max(0) >> 5);
    let brightness = splash_brightness(frame);
    let glyph = |id: u32| HudDraw::Glyph {
        layer: 0,
        id,
        x: 0xa0,
        y,
        brightness,
    };
    Some([glyph(0x416), glyph(0x816)])
}

/// The result plate's rank glyph for a fight of accumulated `strength`
/// (`DAT_801D91B8`): `FUN_801D5298`'s threshold ladder
/// (`0x801D5448..0x801D54B4`) - `0x15` below `0xC9`, then `0x13`, `0x12` and
/// `0x11` from `0xC9`, `0x259` and `0x321`, and `0x14` from `0x4B1`.
pub fn catch_rank_glyph(strength: i32) -> u32 {
    match strength {
        i32::MIN..=0xC8 => 0x15,
        0xC9..=0x258 => 0x13,
        0x259..=0x320 => 0x12,
        0x321..=0x4B0 => 0x11,
        _ => 0x14,
    }
}

/// The landed catch's result plate for one frame - the draw half of the
/// result actor's tick `FUN_801D5298`.
///
/// `ramp` is the actor's `+0x1A` counter (`+4` per frame step from the
/// landing, held at `0x1000`); `lift = clamp(ramp - 0x180, 0, 0x100)` drives
/// everything, and every sprite draws at brightness `lift / 2`:
///
/// - the rank plate [`catch_rank_glyph`] at `(0xA0, 0x78)`, as ids `| 0x400`
///   then `| 0x800` (`0x801D5534..0x801D558C`);
/// - the points in the large digit style at `(0x20, 0x88)`
///   (`FUN_801D76E0(1, 0x20, 0x88, points, lift / 2)`, `0x801D5640`);
/// - the label glyph `0x17` at `(0xC0, 0x98)`, as `0x417` then `0x817`;
/// - the species name through `FUN_801D73B8(name, 0xA0, 0x1A0 - lift)`, which
///   centres it on `x = 0xA0` by `13 * (len - 1) / 4` (its length times a
///   13-unit pitch, not the glyph widths), draws it 7 px below the `y` it is
///   given, and skips it while that `y` is `>= 0xF1` - so the name rises
///   into view from below as the plate fades up. `name_len` is the name's
///   byte length; `0` draws no name.
// PORT: FUN_801d5298 (result plate draw: rank glyph, large points, label, rising name)
// REF: FUN_801d73b8 (centred caption at y + 7, skipped at y >= 0xF1)
pub fn catch_result_draws(
    ramp: i32,
    strength: i32,
    points: i32,
    species: u32,
    name_len: usize,
) -> Vec<HudDraw> {
    let lift = (ramp - 0x180).clamp(0, 0x100);
    let brightness = lift / 2;
    let rank = catch_rank_glyph(strength);
    let glyph = |id: u32, x: i32, y: i32| HudDraw::Glyph {
        layer: 0,
        id,
        x,
        y,
        brightness,
    };
    let mut d = vec![
        glyph(rank | 0x400, 0xA0, 0x78),
        glyph(rank | 0x800, 0xA0, 0x78),
        HudDraw::LargeNumber {
            x: 0x20,
            y: 0x88,
            value: points,
            brightness,
        },
        glyph(0x417, 0xC0, 0x98),
        glyph(0x817, 0xC0, 0x98),
    ];
    let name_y = 0x1A0 - lift;
    if name_len > 0 && name_y < 0xF1 {
        // `addiu s0,v0,-1` then `13 * s0`, `/ 4` rounding toward zero.
        let half = (13 * (name_len as i32 - 1)) / 4;
        d.push(HudDraw::Caption {
            text: HudCaption::FishName(species),
            x: 0xA0 - half,
            y: name_y + 7,
        });
    }
    d
}

/// One of the driver tail's auxiliary one-shot animation timers
/// (`DAT_801d9160` / `DAT_801d915c` / `DAT_801d90f0`): zero = idle, seeded to
/// `1` to start, advanced by the frame step (`DAT_1f800393`) each frame its
/// animator reports active, and reset to zero when the animation expires.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BannerTimer(pub i32);

impl BannerTimer {
    /// Seed the timer to `1` (the retail start value).
    pub fn start(&mut self) {
        self.0 = 1;
    }

    /// Force the timer idle (retail zeroes a timer to cancel its banner -
    /// e.g. the from-right banner cancels the from-left one while it runs).
    pub fn cancel(&mut self) {
        self.0 = 0;
    }

    /// `true` while the timer is running.
    pub fn is_active(&self) -> bool {
        self.0 != 0
    }

    /// Service one frame: while active, run `animator` on the current frame
    /// count; advance by `frame_step` if it drew, reset to idle if it
    /// expired. Returns the animator's draw output.
    // PORT: FUN_801cf3bc shared tail LAB_801d01a4 (banner-timer service loop)
    pub fn service<T>(
        &mut self,
        frame_step: i32,
        animator: impl FnOnce(i32) -> Option<T>,
    ) -> Option<T> {
        if self.0 == 0 {
            return None;
        }
        let out = animator(self.0);
        if out.is_some() {
            self.0 += frame_step.max(1);
        } else {
            self.0 = 0;
        }
        out
    }
}

/// The fishing HUD's five one-shot banner animations, as the set of timers
/// retail keeps alongside the driver (`DAT_801d9160` hook / `DAT_801d915c`
/// reel-in / `DAT_801d9268` miss / `DAT_801d9164` auxiliary / `DAT_801d90f0`
/// splash).
///
/// Own one per fishing session on the host, seed it from the session's phase
/// edges, and call [`FishingBanners::service_frame`] once a frame; the returned
/// draws append to the HUD item list the rest of this module builds. The
/// per-frame service is the retail driver tail's own loop
/// ([`BannerTimer::service`]) - a timer advances only while its animator still
/// reports a draw, and resets itself the frame the animation expires.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FishingBanners {
    /// Hook banner, sliding in from the left ([`banner_from_left_draw`]).
    pub hook: BannerTimer,
    /// Reel-in banner, sliding in from the right ([`banner_from_right_draw`]).
    pub reel_in: BannerTimer,
    /// Miss / retry banner ([`banner_miss_draw`]).
    pub miss: BannerTimer,
    /// Auxiliary converging pair ([`banner_converge_draws`]).
    pub converge: BannerTimer,
    /// Strike splash ([`strike_splash_draws`]).
    pub splash: BannerTimer,
}

impl FishingBanners {
    /// The fish takes the lure. Retail seeds the splash at the strike and the
    /// hook banner at the hook a moment later; the engine's cast-lock is both
    /// events at once, so both timers start on the same edge.
    pub fn on_hook(&mut self) {
        self.splash.start();
        self.hook.start();
    }

    /// The fish was landed: the reel-in banner runs, and - as in the retail
    /// driver tail - it cancels the hook banner while it does.
    pub fn on_landed(&mut self) {
        self.hook.cancel();
        self.reel_in.start();
    }

    /// The line broke: the converging `LINE BREAK!` pair runs and the hook
    /// banner stops. The lure tick's line-break arm seeds exactly these two
    /// timers - `sw zero` to `DAT_801d9160` at `0x801D3C30` and `1` to
    /// `DAT_801d9164` at `0x801D3C54` - before parking the driver in state
    /// `0x28`, which waits on the pair. The miss banner (`DAT_801d9268`,
    /// state `0x2d`) is not this edge's.
    pub fn on_snapped(&mut self) {
        self.hook.cancel();
        self.converge.start();
    }

    /// Returning to the cast loop seeds nothing: the empty reel-in arm stores
    /// state `0x32` and no banner timer (`0x801D3C8C..0x801D3C94`), and the
    /// result screen's dismissal leaves through state `10` the same way.
    pub fn on_recast(&mut self) {}

    /// Service every timer for one frame and collect the draws still live.
    pub fn service_frame(&mut self, frame_step: i32) -> Vec<HudDraw> {
        let mut out = Vec::new();
        out.extend(self.hook.service(frame_step, banner_from_left_draw));
        out.extend(self.reel_in.service(frame_step, banner_from_right_draw));
        out.extend(self.miss.service(frame_step, banner_miss_draw));
        if let Some(pair) = self.converge.service(frame_step, banner_converge_draws) {
            out.extend(pair);
        }
        if let Some(pair) = self.splash.service(frame_step, strike_splash_draws) {
            out.extend(pair);
        }
        out
    }
}
