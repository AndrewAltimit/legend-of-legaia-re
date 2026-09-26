//! Baka Fighter **round chrome** - the intro title card, the round banner and
//! the READY/FIGHT countdown that frame the duel proper.
//!
//! All three are frame-counter timelines over the same primitive: the HUD
//! textured-quad emitter `FUN_801D5ED0`, ported as
//! [`crate::baka_fighter::hud_widget_quad`] against the 51-record widget
//! descriptor table (`legaia_asset::baka_opponents::parse_baka_hud`). This
//! module keeps the *timelines* - which widget is drawn where, how bright and
//! how big on frame `t`, plus the CD-XA announcer lines and the screen fades
//! they fire - separate from the quad geometry, so a host can run them with no
//! renderer and no overlay image resident.
//!
//! Sourced from the Baka Fighter overlay (PROT 0976, link base `0x801CE818`);
//! see `docs/subsystems/minigame-baka-fighter.md`.
//!
//! ### The glyph strip
//!
//! Widget `5` of the descriptor table is a **digit/letter strip**: the drawing
//! wrappers select a cell by overwriting the widget's own `u` field (record 5
//! `+0x08`, runtime VA `0x801D71CC`) with `index * 24` before the emit. Two
//! call sites do it - `FUN_801D69A8` (this module) and the actor draw callback
//! `FUN_801D67F0` mode 2 - and both use the same 24-pixel cell pitch.

/// One resolved chrome draw: a widget id plus the four emitter arguments.
///
/// Retail passes these straight to `FUN_801D5ED0(x, y, widget, brightness,
/// size)`; feed them to [`crate::baka_fighter::hud_widget_quad`] once the
/// descriptor table is parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeDraw {
    /// Widget id into the HUD descriptor table.
    pub widget: u8,
    /// Screen centre of the quad.
    pub x: i16,
    pub y: i16,
    /// Colour scale, `0x80` = the descriptor's own RGB, `0xFF` = doubled.
    pub brightness: i32,
    /// Size scale, 20.12 fixed point (`0x1000` = pixel-exact cell size).
    pub size: i32,
    /// When `Some(n)`, the emitter first pages the [`GLYPH_WIDGET`] strip to
    /// cell `n` by writing `u = n * GLYPH_CELL_WIDTH` (`DAT_801D71CC`).
    pub glyph: Option<i32>,
}

impl ChromeDraw {
    const fn plain(widget: u8, x: i16, y: i16, brightness: i32, size: i32) -> Self {
        ChromeDraw {
            widget,
            x,
            y,
            brightness,
            size,
            glyph: None,
        }
    }
}

/// A CD-XA one-shot the chrome fires through `FUN_8003D53C(clip, chan, dur)`.
///
/// `clip` indexes the runtime clip table at `0x801C6ED8` (slot `i` = `XA<i+1>`),
/// so `0x20` = `XA33.XA` (the announcer bank) and `0x1F` = `XA32.XA`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XaCue {
    pub clip: u8,
    pub chan: u8,
    pub dur: u16,
}

/// Full-screen tint push (`FUN_80024EE4(1, 1, rgb)`), one 8-bit grey level
/// replicated across the three channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenTint {
    pub grey: u8,
}

/// What one chrome frame produces.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChromeFrame {
    /// Quads to emit, in retail submit order.
    pub draws: Vec<ChromeDraw>,
    /// Announcer line started this frame, if any.
    pub xa: Option<XaCue>,
    /// Full-screen tint pushed this frame, if any.
    pub tint: Option<ScreenTint>,
    /// When `Some`, the frame rewrites widget `0x22`'s sibling CLUT field
    /// (record 34 `+0x06`, runtime VA `0x801D740E`) before drawing.
    pub banner_clut: Option<u16>,
}

/// The widget whose `u` origin the glyph wrappers page.
pub const GLYPH_WIDGET: u8 = 5;
/// Pixel pitch of one glyph cell in that strip.
pub const GLYPH_CELL_WIDTH: i32 = 24;

/// Runtime VA of the glyph strip's `u` field (widget 5 `+0x08`).
pub const GLYPH_U_VA: u32 = 0x801D_71CC;
/// Runtime VA of the banner CLUT the intro timeline swaps (widget 34 `+0x06`).
pub const BANNER_CLUT_VA: u32 = 0x801D_740E;

/// The two CLUT ids the intro title alternates between on widget 34.
pub const BANNER_CLUT_IDLE: u16 = 0x7740;
pub const BANNER_CLUT_FLASH: u16 = 0x7742;

/// `u` origin the glyph wrappers stamp for cell `index` (a byte store, so it
/// wraps at 256 exactly as retail's `sb` does).
///
/// PORT: FUN_801D69A8 (the store half) / FUN_801D67F0 mode 2.
///
/// Wired: the duel's own [`BakaChrome`] produces the draws
/// ([`crate::baka_fighter::BakaFight::chrome_frame`]). The minigames page's
/// `baka_chrome_json` export stamps each glyph-carrying [`ChromeDraw`] through
/// this and the page samples widget 5's strip at the stamped column - the one
/// host that draws the chrome from the duel's own art. The play window
/// performs the same stamp against the overlay's parsed widget table
/// (`legaia_asset::baka_opponents::parse_baka_hud`, `window/minigames.rs`) and
/// carries the resolved rect beside each draw, but prints the chrome as
/// labels: its duel HUD has no textured-quad surface, so the rect is not yet
/// sampled there. The browser play page prints the same labels.
pub fn glyph_u(index: i32) -> u8 {
    (index.wrapping_mul(GLYPH_CELL_WIDTH) & 0xFF) as u8
}

/// PORT: FUN_801D69A8 - the glyph draw wrapper.
///
/// `FUN_801D69A8(x, y, index, brightness, size)` pages the [`GLYPH_WIDGET`]
/// strip to `index` and then emits that widget with the caller's brightness
/// and size unchanged. It is a pure re-spelling of `FUN_801D5ED0` with the
/// widget id pinned to 5.
pub fn glyph_draw(x: i16, y: i16, index: i32, brightness: i32, size: i32) -> ChromeDraw {
    ChromeDraw {
        widget: GLYPH_WIDGET,
        x,
        y,
        brightness,
        size,
        glyph: Some(index),
    }
}

/// The signed `>> 1` retail spells as `srl 31; addu; sra 1` (round toward
/// zero), used by both the banner and the countdown for their half-brightness.
fn half(v: i32) -> i32 {
    let v = v.wrapping_add(((v as u32) >> 31) as i32);
    v >> 1
}

/// The `mult`-by-magic divide-by-30 the banner ramp uses (`0x88888889`,
/// add, `sra 4`) - a plain signed division, restated.
fn div30(v: i32) -> i32 {
    v / 30
}

// ---------------------------------------------------------------------------
// Intro title card
// ---------------------------------------------------------------------------

/// Frame thresholds of the intro title timeline.
pub const INTRO_LOGO_IN: i32 = 30;
pub const INTRO_LOGO_HOLD: i32 = 100;
pub const INTRO_SUBTITLE_END: i32 = 140;

/// The intro title card's announcer latch (`DAT_801DBE8C`): `0` before the
/// first line, `1` after it, `2` after the second.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IntroTitle {
    pub announced: i32,
}

impl IntroTitle {
    /// PORT: FUN_801D59D4 - the Baka Fighter intro title card.
    ///
    /// One call per frame with the elapsed frame count `t`. Three segments,
    /// each an independent range test on the same counter (they do not chain -
    /// retail re-tests `t` at every stage):
    ///
    /// - `30 <= t < 100`: the logo (widget `0x28`) fades up at `(0xA0, 0x80)`,
    ///   brightness `(t - 30) * 8` clamped by holding the multiplier at `0x10`.
    ///   The first announcer line (`XA33` channel `0x0E`) fires once, latched
    ///   by [`IntroTitle::announced`].
    /// - `100 <= t < 140`: the logo holds at brightness `0x80`; the second
    ///   line (`XA33` channel `0x0F`) fires once; the first four frames push a
    ///   white screen tint; the subtitle (widget `0x22`) shrinks in from
    ///   `size = 0x1000 + (0x10 - k) << 11` where `k = min(t - 100, 0x10)`,
    ///   and the banner CLUT flips to [`BANNER_CLUT_FLASH`] until `k` reaches
    ///   its clamp.
    /// - `t >= 140`: the CLUT returns to [`BANNER_CLUT_IDLE`] and the full
    ///   card assembles - a four-cell caption ramp (widgets `0x24..=0x27`,
    ///   one per four frames from `t = 147`), the sweep bar (widget `0x32`)
    ///   whose brightness ramps `4` per frame from `-0x80`, a screen tint
    ///   fading white to black over the same 64 frames, then the logo, the
    ///   subtitle at `0x80 + ramp`, the two side ornaments (`0x2A` at
    ///   `x = 0x86`, `0x2B` at `x = 0xBB`) and the underline (`0x23`).
    ///
    /// The caption index is `(e - 8) >> 2` for `e >= 8` and `(e - 5) >> 2`
    /// below it (retail's round-toward-zero shift), tested **unsigned** so the
    /// negative early values fall through undrawn.
    pub fn frame(&mut self, t: i32) -> ChromeFrame {
        let mut out = ChromeFrame::default();

        if (INTRO_LOGO_IN..INTRO_LOGO_HOLD).contains(&t) {
            let step = if t - INTRO_LOGO_IN >= 0x11 {
                0x10
            } else {
                t - INTRO_LOGO_IN
            };
            out.draws
                .push(ChromeDraw::plain(0x28, 0xA0, 0x80, step * 8, 0x1000));
            if self.announced == 0 {
                self.announced = 1;
                out.xa = Some(XaCue {
                    clip: 0x20,
                    chan: 0x0E,
                    dur: 0x3F,
                });
            }
        }

        if (INTRO_LOGO_HOLD..INTRO_SUBTITLE_END).contains(&t) {
            let e = t - INTRO_LOGO_HOLD;
            if self.announced == 1 && e >= 0 {
                self.announced = 2;
                out.xa = Some(XaCue {
                    clip: 0x20,
                    chan: 0x0F,
                    dur: 0x76,
                });
            }
            out.draws
                .push(ChromeDraw::plain(0x28, 0xA0, 0x80, 0x80, 0x1000));
            if e < 4 {
                out.tint = Some(ScreenTint { grey: 0xFF });
            }
            let k = 0x10 - if e >= 0x11 { 0x10 } else { e };
            out.banner_clut = Some(if k != 0 {
                BANNER_CLUT_FLASH
            } else {
                BANNER_CLUT_IDLE
            });
            out.draws.push(ChromeDraw::plain(
                0x22,
                0xA0,
                0x98,
                0x80,
                (k << 11) + 0x1000,
            ));
        }

        if t >= INTRO_SUBTITLE_END {
            out.banner_clut = Some(BANNER_CLUT_IDLE);
            let e = t - INTRO_SUBTITLE_END;
            // Round-toward-zero `(e - 8) / 4`.
            let mut caption = if e - 8 >= 0 {
                (e - 8) >> 2
            } else {
                (e - 5) >> 2
            };
            if (caption as u32) < 4 {
                out.draws.push(ChromeDraw::plain(
                    0x24 + caption as u8,
                    0xA0,
                    0x98,
                    0x80,
                    0x1000,
                ));
            }
            // Sweep ramp: 4 per frame, saturating at 0xFF. Retail's saturated
            // arm also drops the caption ramp to zero.
            let mut ramp = e << 2;
            let mut sweep = ramp - 0x80;
            if ramp >= 0x100 {
                ramp = 0xFF;
                caption = 0;
                sweep = ramp - 0x80;
            }
            if sweep < 0 {
                sweep = 0;
            }
            out.draws
                .push(ChromeDraw::plain(0x32, 0xA0, 0xB8, sweep, 0x1000));
            out.tint = Some(ScreenTint {
                grey: (0xFF - ramp) as u8,
            });
            out.draws
                .push(ChromeDraw::plain(0x28, 0xA0, 0x80, 0x80, 0x1000));
            out.draws
                .push(ChromeDraw::plain(0x22, 0xA0, 0x98, caption + 0x80, 0x1000));
            out.draws
                .push(ChromeDraw::plain(0x2A, 0x86, 0x64, 0x80, 0x1000));
            out.draws
                .push(ChromeDraw::plain(0x2B, 0xBB, 0x64, 0x80, 0x1000));
            out.draws
                .push(ChromeDraw::plain(0x23, 0xA0, 0x80, 0x80, 0x1000));
        }

        out
    }
}

// ---------------------------------------------------------------------------
// Round banner
// ---------------------------------------------------------------------------

/// Frame thresholds of the round banner timeline.
pub const BANNER_SLIDE_IN: i32 = 30;
pub const BANNER_SLIDE_OUT: i32 = 90;
/// Screen x the two banner halves converge on.
pub const BANNER_CENTRE_X: i16 = 0x90;

/// PORT: FUN_801D5C7C - the "ROUND n" banner slide.
///
/// One call per frame with the elapsed frame count `t` and the round index
/// `round` (retail's `DAT_801DBF8C`, 0-based). The banner is two mirrored
/// halves that converge on [`BANNER_CENTRE_X`], hold, then part again:
///
/// - `t == 0` fires the round-announce voice line - clip `XA32`, and the
///   **channel is the round index itself**, duration `0x48`.
/// - `t < 30`: offset `0xB4 - 6t`, level held at `0x80`, halves drawn at
///   half brightness (the "parted" pose, both sprite flags set).
/// - `30 <= t < 90`: offset `0`, level `0x80 + (t - 30) * 127 / 30` (so it
///   reaches `0xFF` at `t = 60`), halves drawn at full level with the flags
///   cleared (the "joined" pose).
/// - `t >= 90`: offset `6 * (t - 90)`, level `0xC8 - (t - 90) * 127 / 30`,
///   back to the parted pose.
///
/// The level clamps to `0..=0xFF` after the ramps. Each pose draws the
/// caption widget `3` and the round digit (glyph cell `round + 1`) twice,
/// mirrored about [`BANNER_CENTRE_X`].
pub fn round_banner_frame(t: i32, round: i32) -> ChromeFrame {
    let mut out = ChromeFrame::default();
    if t == 0 {
        out.xa = Some(XaCue {
            clip: 0x1F,
            chan: (round & 0xFF) as u8,
            dur: 0x48,
        });
    }

    let mut level = 0x80;
    let mut offset;
    let mut parted = true;
    if t < BANNER_SLIDE_IN {
        offset = 0xB4 - t * 6;
    } else {
        level = 0x80 + div30((t - BANNER_SLIDE_IN) * 127);
        offset = 0;
        parted = false;
    }
    if t >= BANNER_SLIDE_OUT {
        offset = (t - BANNER_SLIDE_OUT) * 6;
        level = 0xC8 - div30((t - BANNER_SLIDE_OUT) * 127);
        parted = true;
    }
    level = level.clamp(0, 0xFF);

    let x = offset as i16;
    let mirror = BANNER_CENTRE_X - x;
    if parted {
        let lv = half(level);
        out.draws
            .push(ChromeDraw::plain(3, x + BANNER_CENTRE_X, 0x77, lv, 0x1000));
        out.draws
            .push(glyph_draw(x + 0xE8, 0x78, round + 1, lv, 0x1000));
        out.draws
            .push(ChromeDraw::plain(3, mirror, 0x77, lv, 0x1000));
        out.draws
            .push(glyph_draw(mirror + 0x58, 0x77, round + 1, lv, 0x1000));
    } else {
        out.draws
            .push(glyph_draw(x + 0xE8, 0x78, round + 1, level, 0x1000));
        out.draws
            .push(glyph_draw(x + 0xE8, 0x78, round + 1, level, 0x1000));
        out.draws
            .push(ChromeDraw::plain(3, mirror, 0x78, level, 0x1000));
        out.draws
            .push(glyph_draw(mirror + 0x58, 0x78, round + 1, level, 0x1000));
    }
    out
}

/// Whether the banner's two sprite-actor visibility flags are set this frame
/// (`DAT_801D71AB` / `DAT_801D71D3`, byte `+0x0F` of widget records 2 and 4).
pub fn round_banner_flags(t: i32) -> bool {
    !(BANNER_SLIDE_IN..BANNER_SLIDE_OUT).contains(&t)
}

// ---------------------------------------------------------------------------
// READY / FIGHT countdown
// ---------------------------------------------------------------------------

/// The countdown's own gate: the banner brightness (`DAT_801DBEB4`) must reach
/// this before the timer starts decaying.
pub const COUNTDOWN_FADE_GATE: i32 = 0x11;
/// Timer seeded when the countdown enters state 2.
pub const COUNTDOWN_TIMER: i32 = 0x20;
/// The value of the round counter (`DAT_801DC110`) that marks the final round.
pub const COUNTDOWN_FINAL_ROUND: i32 = 0x0E;

/// READY/FIGHT countdown state (`DAT_801DC134` / `DAT_801DC138`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Countdown {
    /// `DAT_801DC134`: `0` fresh, `1` after the first line, `2` counting,
    /// `3` done.
    pub state: i32,
    /// `DAT_801DC138`: frames left before the final line.
    pub timer: i32,
}

/// PORT: FUN_801D21FC - the READY/FIGHT countdown.
///
/// `frame_step` is the per-frame tick (`DAT_1F800393`), `loading` the scene
/// load flag `_DAT_8007BC20`, `banner_level` / `title_level` the two banner
/// brightness globals (`DAT_801DBEB4` / `DAT_801DBEB0`), and `round_counter`
/// the round global `DAT_801DC110`.
///
/// While `loading` is clear the state advances: `0 -> 1` fires `XA33`
/// channel `0x0A`; `1 -> 2` fires channel `0x0B` and seeds
/// [`COUNTDOWN_TIMER`]; state `2` waits for `banner_level` to reach
/// [`COUNTDOWN_FADE_GATE`], then decays the timer by `frame_step` and, on
/// running out, fires channel `0x0D` on the final round (`round_counter ==
/// 0x0E`) or `0x0C` otherwise and settles in state `3`.
///
/// Every frame - loading or not - it draws the two banner widgets at half
/// their level: `0x1A` at `(0xA0, 0x60)` and, at `(0xA0, 0xA0)`, `0x1D` on
/// the final round or `0x1C` otherwise.
pub fn countdown_frame(
    st: &mut Countdown,
    frame_step: i32,
    loading: bool,
    banner_level: i32,
    title_level: i32,
    round_counter: i32,
) -> ChromeFrame {
    let mut out = ChromeFrame::default();
    let final_round = round_counter == COUNTDOWN_FINAL_ROUND;

    if st.state == 0 {
        st.state = 1;
        out.xa = Some(XaCue {
            clip: 0x20,
            chan: 0x0A,
            dur: 0x46,
        });
    }
    if !loading {
        if st.state == 1 {
            st.state = 2;
            st.timer = COUNTDOWN_TIMER;
            out.xa = Some(XaCue {
                clip: 0x20,
                chan: 0x0B,
                dur: 0x4D,
            });
        }
        if st.state == 2 && banner_level >= COUNTDOWN_FADE_GATE {
            st.timer -= frame_step;
            if st.timer < 0 {
                st.timer = 0;
                st.state = 3;
                out.xa = Some(if final_round {
                    XaCue {
                        clip: 0x20,
                        chan: 0x0D,
                        dur: 0x66,
                    }
                } else {
                    XaCue {
                        clip: 0x20,
                        chan: 0x0C,
                        dur: 0x5A,
                    }
                });
            }
        }
    }

    out.draws.push(ChromeDraw::plain(
        0x1A,
        0xA0,
        0x60,
        half(title_level),
        0x1000,
    ));
    out.draws.push(ChromeDraw::plain(
        if final_round { 0x1D } else { 0x1C },
        0xA0,
        0xA0,
        half(banner_level),
        0x1000,
    ));
    out
}

// ---------------------------------------------------------------------------
// Sprite-actor draw callback
// ---------------------------------------------------------------------------

/// The per-actor fields the chrome draw callback reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChromeActor {
    /// `+0x14` / `+0x16` screen position.
    pub x: i16,
    pub y: i16,
    /// `+0x50` widget id (modes 0/1) or glyph cell (mode 2).
    pub id: u16,
    /// `+0x72` size scale (modes 0/1).
    pub size: u16,
    /// `+0x78` raw fade level, 20.4 fixed point.
    pub fade: u16,
}

/// What [`chrome_actor_draw`] resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeActorDraw {
    pub draw: Option<ChromeDraw>,
    /// Mode 1 raises the actor's retire bit (`+0x10 |= 8`) once the match
    /// phase global `DAT_801DBF78` is non-zero.
    pub retire: bool,
}

/// PORT: FUN_801D67F0 - the chrome sprite-actor draw callback.
///
/// `mode` selects the shape of the draw. Whatever the mode, the brightness is
/// the actor's `+0x78` fade level put through the same three-step conditioning
/// retail applies: values above `0x4000` are treated as zero, the level is
/// rounded toward zero by `>> 4` (`+0xF` first when positive), then clamped to
/// `0..=0xFF`.
///
/// - mode `0`: widget `id` at the actor position, size from `+0x72`.
/// - mode `1`: the same draw, then the retire bit once the match is live.
/// - mode `2`: the [`GLYPH_WIDGET`] strip paged to cell `id`, size `0x1000`.
/// - anything else: nothing at all.
pub fn chrome_actor_draw(actor: &ChromeActor, mode: i32, match_phase: i32) -> ChromeActorDraw {
    let mut level = actor.fade as i32;
    if level >= 0x4001 {
        level = 0;
    }
    let level = if level >= 0 { level + 0xF } else { level } >> 4;
    let level = level.clamp(0, 0xFF);

    match mode {
        0 => ChromeActorDraw {
            draw: Some(ChromeDraw::plain(
                actor.id as u8,
                actor.x,
                actor.y,
                level,
                actor.size as i32,
            )),
            retire: false,
        },
        1 => ChromeActorDraw {
            draw: Some(ChromeDraw::plain(
                actor.id as u8,
                actor.x,
                actor.y,
                level,
                actor.size as i32,
            )),
            retire: match_phase != 0,
        },
        2 => ChromeActorDraw {
            draw: Some(glyph_draw(actor.x, actor.y, actor.id as i32, level, 0x1000)),
            retire: false,
        },
        _ => ChromeActorDraw {
            draw: None,
            retire: false,
        },
    }
}

/// Actor flag bit the retire path raises (`+0x10 |= 8`).
pub const ACTOR_FLAG_RETIRE: u32 = 0x8;
/// Actor flag bit `FUN_801D6F18` raises (`+0x10 |= 0x200000`).
pub const ACTOR_FLAG_HOLD: u32 = 0x0020_0000;
/// Actor flag bit the bind path clears (`+0x10 &= ~2`).
pub const ACTOR_FLAG_TICK: u32 = 0x2;

/// PORT: FUN_801D6F18 - the chrome actor hold wrapper.
///
/// Raises [`ACTOR_FLAG_HOLD`] on the actor's flag word and re-enters the
/// shared actor dispatcher `FUN_800204F8`. The whole body is the `or` plus
/// the tail call.
pub fn chrome_actor_hold(flags: u32) -> u32 {
    flags | ACTOR_FLAG_HOLD
}

/// What [`chrome_actor_bind`] resolves for one sprite actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeActorBind {
    /// New flag word for the actor's `+0x10`.
    pub flags: u32,
    /// New `+0x78` fade level: `0` for the two focused fighters, `0x800`
    /// otherwise.
    pub fade: u16,
    /// New `+0x6A` frame count, byte `+0x07` of the resolved animation
    /// record. `None` when the match is over and the actor retires instead.
    pub frames: Option<u8>,
    /// Whether `+0x74` is cleared (the high-bank arm only).
    pub clear_accum: bool,
}

/// Animation-id threshold that picks between the two clip banks.
pub const ANIM_BANK_SPLIT: i16 = 0x400;
/// Mask applied to the animation id before the bank offset lookup.
pub const ANIM_ID_MASK: u16 = 0x3FF;

/// PORT: FUN_801D3390 - the chrome sprite-actor animation bind.
///
/// Runs before the actor's draw. Once the match phase global
/// `DAT_801DBF78` is non-zero the whole body collapses to raising
/// [`ACTOR_FLAG_RETIRE`] and returning - the chrome tears itself down with
/// the match.
///
/// Otherwise it clears [`ACTOR_FLAG_TICK`], sets the actor's fade level from
/// whether its `+0x5A` owner id matches either focused fighter
/// (`DAT_801DBF70` / `DAT_801DBF74`) - `0` when it does, `0x800` when it does
/// not - and resolves the animation record: the `+0x5C` id picks the
/// scene's type-`0x05` clip bank `_DAT_8007B888` below [`ANIM_BANK_SPLIT`] and
/// the type-`0x0B` bank `_DAT_8007B840` at or above it (the same two banks the
/// clip selector `FUN_800204F8` resolves - ANM containers, not sprite sheets) (the at-or-above arm also clears `+0x74`), the id's
/// low [`ANIM_ID_MASK`] bits index a word offset table at the bank base, and
/// byte `+0x07` of the record that offset reaches becomes the actor's
/// `+0x6A` frame count.
pub fn chrome_actor_bind(
    flags: u32,
    match_phase: i32,
    owner: i16,
    focus: (i16, i16),
    anim_id: i16,
    bank_below_split: &[u8],
    bank_at_or_above: &[u8],
) -> ChromeActorBind {
    if match_phase != 0 {
        return ChromeActorBind {
            flags: flags | ACTOR_FLAG_RETIRE,
            fade: 0,
            frames: None,
            clear_accum: false,
        };
    }
    let flags = flags & !ACTOR_FLAG_TICK;
    let fade = if focus.0 == owner || focus.1 == owner {
        0
    } else {
        0x800
    };
    let below = anim_id < ANIM_BANK_SPLIT;
    let bank = if below {
        bank_below_split
    } else {
        bank_at_or_above
    };
    let idx = (anim_id as u16 & ANIM_ID_MASK) as usize;
    let frames = bank
        .get(idx * 4..idx * 4 + 4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) as usize)
        .and_then(|off| bank.get(off + 7))
        .copied();
    ChromeActorBind {
        flags,
        fade,
        frames,
        clear_accum: !below,
    }
}

// ---------------------------------------------------------------------------
// Knockdown effect slot table
// ---------------------------------------------------------------------------

/// Stride of one action-animation slot record in the per-fighter block.
pub const ANIM_SLOT_STRIDE: usize = 8;
/// Stride of one fighter's animation block (`t2 * 0x60` in the retail index
/// math: `(t2 * 3) << 5`).
pub const ANIM_FIGHTER_STRIDE: usize = 0x60;
/// Slot cap the allocator refuses to grow past.
pub const ANIM_SLOT_CAP: i32 = 8;

/// Result of [`anim_slot_install`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimSlotInstall {
    /// Index of the slot the key landed in.
    pub slot: usize,
    /// Whether the key was already resident (a re-trigger) rather than new.
    pub reused: bool,
    /// New live-slot count for the fighter.
    pub count: i32,
    /// Retail's return value: `-1` on install, `-1` on a full table too - the
    /// routine never reports success differently, which is why the caller
    /// treats it as void.
    pub full: bool,
}

// REPLACED-BY: nothing is owed a port - retail never reaches it. Its only
// caller is the keyframe-editor tick `FUN_801D4FC8` (see [`editor_tick`] for
// the gate evidence), a development screen behind `_DAT_8007B868`, which the
// shipped disc boots to zero and never sets.
/// PORT: FUN_801D57BC - the animation-slot installer.
///
/// `(bank, fighter, key)`: the routine resolves the fighter's slot block, then
/// linearly scans the block's live slots (`count` at `+0x1C`, key at slot
/// `+0x26`) for `key`. It rounds `key` toward zero by `>> 4` first (`+0xF`
/// when negative - note the sign test is on the *pre*-shift value).
///
/// A hit rewrites that slot in place; a miss appends a new one and bumps the
/// count. Either way the slot's three accumulators (`+0x20`, `+0x22`, `+0x24`)
/// are zeroed. When the block already holds [`ANIM_SLOT_CAP`] slots the
/// routine returns immediately and installs nothing.
pub fn anim_slot_install(keys: &mut Vec<i16>, key_raw: i32) -> AnimSlotInstall {
    let key = anim_slot_key(key_raw);
    let count = keys.len() as i32;
    if count >= ANIM_SLOT_CAP {
        return AnimSlotInstall {
            slot: 0,
            reused: false,
            count,
            full: true,
        };
    }
    if let Some(slot) = keys.iter().position(|&k| k == key) {
        AnimSlotInstall {
            slot,
            reused: true,
            count,
            full: false,
        }
    } else {
        keys.push(key);
        AnimSlotInstall {
            slot: keys.len() - 1,
            reused: false,
            count: keys.len() as i32,
            full: false,
        }
    }
}

/// The key both slot routines scan on: the caller's raw fixed-point frame
/// time divided by 16, truncated **toward zero** (`bgez` on the raw value,
/// `+0xF` when negative, then `sra 4`). Both `FUN_801D57BC` and
/// `FUN_801D58E0` open with this identical bias-and-shift, so a port that
/// rounds either one the other way stops finding its own installed slots.
fn anim_slot_key(key_raw: i32) -> i16 {
    (if key_raw >= 0 { key_raw } else { key_raw + 0xF } >> 4) as i16
}

/// Result of [`anim_slot_delete`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimSlotDelete {
    /// Index the key was found at, or `None` when the scan missed. Retail
    /// returns `-1` for the miss and the slot-block pointer otherwise, so the
    /// caller can only distinguish the two by sign.
    pub slot: Option<usize>,
    /// Live-slot count after the call. Unchanged on a miss.
    pub count: i32,
}

// REPLACED-BY: nothing is owed a port - retail never reaches it. Same caller
// and same gate as its installer sibling: the editor tick `FUN_801D4FC8`
// behind `_DAT_8007B868`, which the shipped disc never sets (evidence at
// [`editor_tick`]).
/// PORT: FUN_801D58E0 - the animation-slot **remover**, sibling of
/// [`anim_slot_install`].
///
/// Same block resolution (`fighter * 0x60`, count at `+0x1C`, 8-byte slots
/// from `+0x20` with the key half-word at slot `+0x06`) and the same
/// [`anim_slot_key`] bias-and-shift on the query. It scans the live slots for
/// the key; a miss returns `-1` and touches nothing.
///
/// A hit compacts: retail copies each following slot down one place with a
/// pair of unaligned `lwl`/`lwr` + `swl`/`swr` word moves covering the whole
/// 8-byte record (`+0x20` and `+0x24`), then decrements the count. It never
/// clears the slot it just vacated at the top of the block - that entry stays
/// as a stale duplicate above `count` and is simply never scanned again.
///
/// The port's slot model carries the key only, matching [`anim_slot_install`]:
/// the three accumulator half-words retail also moves are zeroed at install
/// and are not modelled anywhere.
pub fn anim_slot_delete(keys: &mut Vec<i16>, key_raw: i32) -> AnimSlotDelete {
    let key = anim_slot_key(key_raw);
    // Retail's scan is bounded by the live count, so an empty block never
    // enters the loop and falls straight through to the `-1` return.
    match keys.iter().position(|&k| k == key) {
        Some(slot) => {
            keys.remove(slot);
            AnimSlotDelete {
                slot: Some(slot),
                count: keys.len() as i32,
            }
        }
        None => AnimSlotDelete {
            slot: None,
            count: keys.len() as i32,
        },
    }
}

// ---------------------------------------------------------------------------
// The runner that drives the three timelines and the banner actor pool
// ---------------------------------------------------------------------------

/// Frame at which the intro title card has finished assembling and the
/// duel host stops ticking it. The card's last independent range test opens
/// at `140` and its ramps run 64 frames.
pub const INTRO_END: i32 = 204;

/// Frame at which the round banner's fly-out brightness has clamped to `0`
/// and the banner is done: `0xC8 - (t - 90) * 127 / 30 <= 0` first holds at
/// `t - 90 == 48`.
pub const BANNER_END: i32 = BANNER_SLIDE_OUT + 48;

/// Widget id the round banner spawns as a sprite actor
/// (`FUN_801D6E04`'s argument on the round-result path).
pub const ROUND_BANNER_SPRITE: u16 = 3;

/// One frame's worth of match state the runner needs from the duel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeTick {
    /// The global frame step (`DAT_1F800393`).
    pub frame_step: i32,
    /// `_DAT_8007BC20` - the scene-load flag that freezes the countdown walk.
    pub loading: bool,
    /// `DAT_801DBF78` - `0` while the match is torn down, non-zero while it
    /// runs. The actor bind retires every pooled banner once it is set.
    pub match_phase: i32,
    /// `DAT_801DBF8C` - the 0-based round index the banner announces.
    pub round: i32,
    /// `DAT_801DC110` - the round counter the countdown tests for its final
    /// announcer line.
    pub round_counter: i32,
    /// `DAT_801DBF70` / `DAT_801DBF74` - the two focused fighter ids the bind
    /// compares each actor's owner against.
    pub focus: (i16, i16),
}

impl Default for ChromeTick {
    fn default() -> Self {
        ChromeTick {
            frame_step: 1,
            loading: false,
            match_phase: 0,
            round: 0,
            round_counter: 0,
            focus: (0, 1),
        }
    }
}

/// One live banner sprite actor in the runner's pool, plus the mode its draw
/// callback runs under and the owner id the bind compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeSprite {
    pub actor: ChromeActor,
    /// The `mode` argument `FUN_801D67F0` is installed with.
    pub mode: i32,
    /// `+0x5A` - the fighter this banner belongs to.
    pub owner: i16,
    /// `+0x5C` - the animation id the bind resolves.
    pub anim_id: i16,
    /// `+0x10` flag word.
    pub flags: u32,
}

/// The Baka Fighter round chrome as one advancing object.
///
/// Retail runs all three timelines off the overlay's own per-frame tick, in
/// parallel with the resolution state machine, and spawns the round-result
/// banners as sprite actors that the `_DAT_8007BA2C` draw hook services.
/// [`BakaChrome`] is that arrangement: the duel
/// ([`crate::baka_fighter::BakaFight`]) steps it once per frame and reads the
/// [`ChromeFrame`] it produces.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BakaChrome {
    intro: IntroTitle,
    intro_t: Option<i32>,
    banner_t: Option<i32>,
    countdown: Countdown,
    /// `DAT_801DBEB4` / `DAT_801DBEB0` - the two banner brightness globals
    /// the countdown gates on and draws with.
    banner_level: i32,
    title_level: i32,
    sprites: Vec<ChromeSprite>,
    /// Whether the banner's two sprite-actor visibility flags are up.
    banner_flags: bool,
}

impl BakaChrome {
    /// A runner with the intro title card armed (the cabinet's attract
    /// sequence) rather than starting mid-duel.
    pub fn with_intro() -> Self {
        BakaChrome {
            intro_t: Some(0),
            ..BakaChrome::default()
        }
    }

    /// Drive the intro title card off an **externally owned clock** - the
    /// cabinet's `DAT_801DBE94`.
    ///
    /// The card is not the duel's: its two `jal` sites in PROT 0976
    /// (`0x801CF784`, `0x801CF840`) are inside the cabinet state machine's
    /// attract arms, and those are the arms that advance `DAT_801DBE94`. So
    /// the honest wiring runs the card while the cabinet occupies an attract
    /// state, with the cabinet's own counter as `t`, rather than arming a
    /// private timeline at duel start.
    ///
    /// Idempotent per frame; a clock past [`INTRO_END`] stops the card, which
    /// is what [`Self::step`]'s own arm does.
    pub fn set_intro_clock(&mut self, t: i32) {
        self.intro_t = (t < INTRO_END).then_some(t);
    }

    /// Start the round banner timeline and spawn its sprite actor at the
    /// screen centre through the shared spawn wrapper.
    pub fn start_round_banner(&mut self, sprite_id: u16) {
        self.banner_t = Some(0);
        self.countdown = Countdown::default();
        let spec = crate::baka_fighter::center_effect_spawn(sprite_id);
        self.sprites.push(ChromeSprite {
            actor: ChromeActor {
                x: spec.x,
                y: spec.y,
                id: spec.sprite_id,
                size: spec.scale as u16,
                fade: 0x800,
            },
            mode: 1,
            owner: -1,
            anim_id: spec.sprite_id as i16,
            flags: chrome_actor_hold(0),
        });
    }

    /// `true` while any timeline is still running.
    pub fn busy(&self) -> bool {
        self.intro_t.is_some() || self.banner_t.is_some()
    }

    /// The live banner sprite pool.
    pub fn sprites(&self) -> &[ChromeSprite] {
        &self.sprites
    }

    /// The countdown's own state, for a host that wants to show it.
    pub fn countdown(&self) -> Countdown {
        self.countdown
    }

    /// Advance every armed timeline one frame and service the sprite pool.
    ///
    /// `banks` are the two runtime clip banks the bind resolves frame
    /// counts out of (`_DAT_8007B888` below [`ANIM_BANK_SPLIT`],
    /// `_DAT_8007B840` at or above). A host with neither staged passes empty
    /// slices, in which case the resolved frame count is `None` and every
    /// other bind output still applies.
    pub fn step(&mut self, tick: &ChromeTick, banks: (&[u8], &[u8])) -> ChromeFrame {
        let mut out = ChromeFrame::default();

        if let Some(t) = self.intro_t {
            let f = self.intro.frame(t);
            merge_frame(&mut out, f);
            self.intro_t = if t + 1 < INTRO_END { Some(t + 1) } else { None };
        }

        if let Some(t) = self.banner_t {
            let f = round_banner_frame(t, tick.round);
            self.banner_flags = round_banner_flags(t);
            merge_frame(&mut out, f);
            // The banner brightness the countdown gates on is the same ramp
            // the banner draws with, so it crosses COUNTDOWN_FADE_GATE a
            // couple of frames into the slide-in.
            self.banner_level = t.min(0xFF);
            self.title_level = self.banner_level;
            let cd = countdown_frame(
                &mut self.countdown,
                tick.frame_step,
                tick.loading,
                self.banner_level,
                self.title_level,
                tick.round_counter,
            );
            merge_frame(&mut out, cd);
            self.banner_t = if t + 1 < BANNER_END {
                Some(t + 1)
            } else {
                None
            };
        }

        let mut retired = Vec::new();
        for (i, s) in self.sprites.iter_mut().enumerate() {
            let bind = chrome_actor_bind(
                s.flags,
                tick.match_phase,
                s.owner,
                tick.focus,
                s.anim_id,
                banks.0,
                banks.1,
            );
            s.flags = bind.flags;
            s.actor.fade = bind.fade;
            if bind.flags & ACTOR_FLAG_RETIRE != 0 {
                retired.push(i);
                continue;
            }
            let d = chrome_actor_draw(&s.actor, s.mode, tick.match_phase);
            if let Some(draw) = d.draw {
                out.draws.push(draw);
            }
            if d.retire {
                s.flags |= ACTOR_FLAG_RETIRE;
                retired.push(i);
            }
        }
        for i in retired.into_iter().rev() {
            self.sprites.remove(i);
        }
        out
    }
}

/// Fold one timeline's frame into the accumulating frame: draws append, and
/// the three single-slot channels take the first value produced this frame -
/// retail's later writes land on the same globals, and the earlier timeline
/// is the one that owns them while it runs.
fn merge_frame(out: &mut ChromeFrame, f: ChromeFrame) {
    out.draws.extend(f.draws);
    out.xa = out.xa.or(f.xa);
    out.tint = out.tint.or(f.tint);
    out.banner_clut = out.banner_clut.or(f.banner_clut);
}

// ---------------------------------------------------------------------------
// Impact effect pair, positional cue, mirrored sprite pass
// ---------------------------------------------------------------------------

/// One effect-part spawn the impact pair emits: a world position, a Euler
/// rotation and the rodata VA of the template `FUN_80021B04` is handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImpactSpawn {
    pub pos: (i16, i16, i16),
    pub rot: (i16, i16, i16),
    /// Rodata VA of the spawn template.
    pub template: u32,
    /// Fixed-point scale (`0x1000` = 1.0).
    pub scale: i32,
}

/// Templates the impact pair picks by slot: `[slot 0, slot 1]`, first the
/// un-rotated pair then the yawed pair.
pub const IMPACT_TEMPLATE_A: [u32; 2] = [0x801D_B8FC, 0x801D_B960];
pub const IMPACT_TEMPLATE_B: [u32; 2] = [0x801D_BBA4, 0x801D_BBD4];
/// Yaw the second spawn takes, by slot - the two sides mirror.
pub const IMPACT_YAW: [i16; 2] = [-0x400, 0x400];
/// Z lift applied unless the special latch `DAT_801DBF50` is up.
pub const IMPACT_Z_LIFT: i16 = 0x32;

// NOT WIRED: the inputs are all in the port now - the resolution arms that
// call it are `BakaFight`'s booking (`0x801D36F0` winner 0, `0x801D3744`
// winner 1, `0x801D37B4` / `0x801D37C0` the draw with the keyframe reset), the
// keyframe TRS column is parsed (`legaia_asset::baka_opponents::
// BakaSubKeyframe::offset`) and the landed keyframe is the strike clock's
// (`crate::baka_fighter::StrikeClock::landed`). What no host has is the
// output: the two spawns are `FUN_80021B04` effect-part templates in the
// overlay's rodata (`0x801DB8FC` / `0x801DB960` / `0x801DBBA4` /
// `0x801DBBD4`), and no minigame host runs that effect runtime or knows the
// fighters' world positions the offsets are relative to - so a call from the
// booking would produce spawns nothing draws.
/// PORT: FUN_801d4df8 - the per-slot **impact effect pair**.
///
/// Retail calls it on a decided exchange. It first zeroes the fighter's
/// `+0x38` accumulator (`&DAT_801dbff4[slot * 0xa8]`), then places both
/// spawns at the fighter's world position offset by the current action
/// keyframe's TRS: X **added** when the facing flag `&DAT_801dbfe4[slot]` is
/// set and **subtracted** when it is clear, Y added, and Z added with a
/// further [`IMPACT_Z_LIFT`] taken off unless the special latch
/// `DAT_801DBF50` is up. `reset_keyframe` forces the keyframe index to `0`
/// rather than the fighter's live `&DAT_801dc054[slot]`
/// ([`impact_keyframe_index`]).
///
/// Both spawns go through the shared part-spawn API at scale `0x1000`; the
/// first uses [`IMPACT_TEMPLATE_A`] with no rotation, the second
/// [`IMPACT_TEMPLATE_B`] with the slot's [`IMPACT_YAW`]. The two slots take
/// **opposite** yaws, which is what mirrors the effect across the arena.
///
/// It is an effect spawn, not an animation play: nothing here touches the
/// fighter's action id or frame cursor.
pub fn impact_effect_pair(
    slot: usize,
    actor_pos: (i16, i16, i16),
    keyframe_trs: (i16, i16, i16),
    facing_flag: bool,
    special_latch: bool,
) -> [ImpactSpawn; 2] {
    let s = slot & 1;
    let x = if facing_flag {
        actor_pos.0.wrapping_add(keyframe_trs.0)
    } else {
        actor_pos.0.wrapping_sub(keyframe_trs.0)
    };
    let y = actor_pos.1.wrapping_add(keyframe_trs.1);
    let mut z = actor_pos.2.wrapping_add(keyframe_trs.2);
    if !special_latch {
        z = z.wrapping_sub(IMPACT_Z_LIFT);
    }
    let pos = (x, y, z);
    [
        ImpactSpawn {
            pos,
            rot: (0, 0, 0),
            template: IMPACT_TEMPLATE_A[s],
            scale: 0x1000,
        },
        ImpactSpawn {
            pos,
            rot: (0, IMPACT_YAW[s], 0),
            template: IMPACT_TEMPLATE_B[s],
            scale: 0x1000,
        },
    ]
}

/// Which keyframe index the impact pair reads - `0` when the caller asks for
/// a reset, otherwise the fighter's live cursor `&DAT_801dc054[slot]`.
///
/// REF: FUN_801d4df8 (`0x801D4E54..0x801D4E5C`)
pub fn impact_keyframe_index(live_cursor: i32, reset_keyframe: bool) -> i32 {
    if reset_keyframe { 0 } else { live_cursor }
}

/// One VRAM-to-VRAM sprite blit: the source `RECT` the helper stacks, plus
/// the destination it hands `MoveImage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteBlit {
    /// Source `RECT.x` (`0x801D6628`..`0x801D6638`).
    pub src_x: i16,
    /// Source `RECT.y` (`0x801D6644`..`0x801D664C`).
    pub src_y: i16,
    /// The rect's fixed `(w, h)` (`0x801D6610` / `0x801D6618`).
    pub size: (i16, i16),
    /// Destination `x`, the call's `a1` (`0x801D6624`..`0x801D6648`).
    pub dst_x: i32,
    /// Destination `y`, the call's `a2`.
    pub dst_y: i32,
}

/// Source-`x` base the table entry's first byte is added to.
pub const BLIT_SRC_X_BASE: i16 = 0x340;
/// Source-`y` base the table entry's second byte is added to.
pub const BLIT_SRC_Y_BASE: i16 = 0x80;
/// Destination `y` the blit always writes to.
pub const BLIT_DST_Y: i32 = 0x86;

// NOT WIRED: its one retail caller is the round-start cameo animator
// [`cameo_pose`] (`FUN_801D6310`, `jal` at `0x801D6354` / `0x801D63B0`), which
// is ported now and reports the table index each frame leaves showing
// (`CameoPose::blit_index`); the table itself is parsed
// (`legaia_asset::baka_opponents::parse_blit_rects`, disc-gated in
// `crates/asset/tests/baka_presentation_real.rs`). What no host has is the
// consumer: the move edits the texture the cameo's model samples, and nothing
// spawns or draws the cameo (see [`cameo_pose`]'s note).
/// PORT: FUN_801d65f8 - the **sprite-blit helper**.
///
/// Builds a VRAM source `RECT` out of the 4-byte record at
/// `&DAT_801dbe84 + index * 4`: byte `0` shifted right two and added to
/// [`BLIT_SRC_X_BASE`], byte `1` added to [`BLIT_SRC_Y_BASE`], with the
/// rect's `(w, h) = (6, 0x18)`, and hands it to
/// `FUN_80058490(&rect, 0x340, 0x86)`. That callee is **`MoveImage`** - it
/// validates the string `"MoveImage"` at `0x800156EC`, packs the destination
/// as `(y << 16) | x` and issues a VRAM-to-VRAM blit packet. An earlier
/// reading here made it a positional SFX play, so the two rect halves wore
/// `pitch` / `pan` names and the destination `y` wore `voice`; nothing in the
/// 49-instruction callee touches a sound path.
///
/// `mode` is the routine's first argument, and only `0` is a defined call:
/// the table pointer *and* the rect's `(w, h)` are written **only** inside
/// the `mode == 0` arm, so any other value reads the table through an
/// uninitialised register. Retail has no such call site; this port returns
/// `None` rather than inventing behaviour for it.
pub fn sprite_blit(mode: i32, entry: [u8; 2]) -> Option<SpriteBlit> {
    if mode != 0 {
        return None;
    }
    Some(SpriteBlit {
        src_x: (entry[0] >> 2) as i16 + BLIT_SRC_X_BASE,
        src_y: entry[1] as i16 + BLIT_SRC_Y_BASE,
        size: (6, 0x18),
        dst_x: BLIT_SRC_X_BASE as i32,
        dst_y: BLIT_DST_Y,
    })
}

/// The ghost passes the special's afterimage draws.
pub const AFTERIMAGE_PASSES: usize = 2;
/// How far each pass sets its ghost back along the clip: `0x30` sixteenths,
/// three whole frames (`addiu v0, v0, -0x30` at `0x801D4B7C`, cumulative).
pub const AFTERIMAGE_LAG_STEP: i16 = 0x30;
/// The depth-cue level (`+0x78`, the `IR0` factor the mesh pass hands the
/// colour-blend call) the first ghost draws at - halfway to the colour word.
pub const AFTERIMAGE_CUE_START: u16 = 0x800;
/// Depth-cue step per pass: the second ghost draws three quarters of the way.
pub const AFTERIMAGE_CUE_STEP: u16 = 0x400;
/// Ordering-table push the ghosts draw under (`_DAT_1F8003F4 + 0x40`, the
/// same for both passes): they sort behind the fighter they trail.
pub const AFTERIMAGE_OT_PUSH: i32 = 0x40;
/// The colour word the spawn stores at `+0x74` (`0x801D4588`): the blend
/// target the depth cue pulls the ghost toward - black, with the
/// blend-enable byte `0x81` on top.
pub const AFTERIMAGE_COLOR_WORD: u32 = 0x8100_0000;
/// The frame-rate divisor the special's commit installs at `DAT_1F80037D`
/// (`0x801D4568`): half the round's `8`, so every clip that recomputes its
/// step from the divisor - both fighters' - runs at half speed for the rest of
/// the round.
pub const SPECIAL_RATE_DIVISOR: i32 = 4;

/// One ghost pass of the afterimage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AfterimagePass {
    /// Which live-mask bit this pass belongs to.
    pub bit: u32,
    /// `true` when the pass drew its ghost (`FUN_8001B964`).
    pub drawn: bool,
    /// `true` when the pass instead cleared its live-mask bit (its lagged
    /// cursor ran past the clip end).
    pub expired: bool,
    /// The lagged clip cursor the ghost is posed at (1/16 frame).
    pub cursor: i16,
    /// The depth-cue level it draws at.
    pub cue: u16,
}

/// What [`afterimage_pass`] resolved for one afterimage actor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AfterimageFrame {
    /// The actor retires this frame (its live mask was already empty).
    pub retire: bool,
    /// The live mask after the passes.
    pub live_mask: u16,
    /// The clip cursor the actor keeps (the advanced value; the passes'
    /// lags are scratch and are restored on the way out).
    pub cursor: i16,
    pub passes: Vec<AfterimagePass>,
}

/// PORT: FUN_801d49e8 - the special attack's **afterimage** (two lagged,
/// darkened ghosts of the attacker).
///
/// Wired: [`crate::baka_fighter::BakaFight`] spawns one [`AfterimageActor`]
/// on every special commit, exactly where the combat tick `FUN_801D3F44`
/// spawns the `0x801D7684` prototype (`0x801D4538..0x801D4634`), steps it
/// every tick, and exposes the ghosts through
/// [`crate::baka_fighter::BakaFight::afterimages`]; the minigames page draws
/// them as darkened copies of the attacker's mesh posed at each ghost's frame.
/// The native window and the play page draw the duel as labels with no fighter
/// meshes at all, so neither has a model to ghost - see
/// `docs/tooling/host-drift.md`.
///
/// This was read as a "mirrored two-pass sprite draw" whose second copy was
/// yawed `0x400` further. The field it steps is `+0x78`, which is not a yaw:
/// it is the depth-cue level `FUN_8001B964` hands the colour-blend call with
/// the colour word `+0x74`, and the spawn sets that word to
/// [`AFTERIMAGE_COLOR_WORD`]. The draw call is the animated mesh renderer, not
/// a sprite emitter. So the two copies are the same pose family drawn
/// [`AFTERIMAGE_LAG_STEP`] and twice that behind the cursor, pulled half and
/// three quarters of the way to black, and sorted [`AFTERIMAGE_OT_PUSH`]
/// deeper than the fighter.
///
/// Per frame: an empty live mask (`+0x5A == 0`) retires the actor outright.
/// Otherwise the clip record the id `+0x5C` reaches is cached, the owner's
/// position is copied in, and the cursor advances by `(step * 2 + n - 1) / n *
/// frame_step` for `n` = record byte `+6` - the clip selector's formula with
/// its `+1` flag test dropped, so it always applies. Then each of the
/// [`AFTERIMAGE_PASSES`] passes lags the cursor one more step and, for a set
/// bit, draws while the lagged cursor is in `0 ..< record[+2] * 0x10 - 1` and
/// clears the bit once it has run past that end.
///
/// The body reads `s5` without ever writing it - the transform call
/// `FUN_8003D344(s5 + 0x14, s5 + 0x2C)` runs against whatever the caller left
/// in the register. That is in the disassembly, not a decompiler artifact, so
/// the port takes no such argument rather than reproducing the read.
pub fn afterimage_pass(
    live_mask: u16,
    cursor: i16,
    frame_step: i32,
    step: i16,
    record_n: u8,
    clip_frames: u16,
) -> AfterimageFrame {
    if live_mask == 0 {
        return AfterimageFrame {
            retire: true,
            live_mask,
            cursor,
            passes: Vec::new(),
        };
    }
    let n = record_n.max(1) as i32;
    let advance = ((step as i32 * 2 + n - 1) / n) * frame_step;
    let kept = (cursor as i32 + advance) as i16;
    let mut running = kept;
    let end = (clip_frames as i32 * 0x10 - 1) as i16;
    let mut mask = live_mask;
    let mut passes = Vec::new();
    for i in 0..AFTERIMAGE_PASSES {
        running = running.wrapping_sub(AFTERIMAGE_LAG_STEP);
        let cue = AFTERIMAGE_CUE_START.wrapping_add(AFTERIMAGE_CUE_STEP * i as u16);
        if live_mask >> i & 1 == 0 {
            continue;
        }
        let (mut drawn, mut expired) = (false, false);
        if running >= 0 {
            if running < end {
                drawn = true;
            } else {
                mask &= !(1u16 << i);
                expired = true;
            }
        }
        passes.push(AfterimagePass {
            bit: i as u32,
            drawn,
            expired,
            cursor: running,
            cue,
        });
    }
    AfterimageFrame {
        retire: false,
        live_mask: mask,
        cursor: kept,
        passes,
    }
}

/// One live afterimage actor: the state [`afterimage_pass`] runs over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AfterimageActor {
    /// `+0x50` - the fighter slot it trails (its position is re-copied from
    /// that fighter every frame).
    pub owner: usize,
    /// `+0x5A` - live mask, `3` at spawn: both ghosts.
    pub live_mask: u16,
    /// `+0x68` - its own clip cursor, zeroed by the spawn.
    pub cursor: i16,
    /// `+0x6A` - the step the spawn computes:
    /// `record[+4] * DAT_1F80037D >> 3` against the divisor the special's
    /// commit has just lowered to [`SPECIAL_RATE_DIVISOR`].
    pub step: i16,
}

impl AfterimageActor {
    /// The spawn at a special commit: owner `slot`, the special record's
    /// speed `special_speed` (`+0x04`).
    pub fn spawn(slot: usize, special_speed: i32) -> Self {
        let raw = special_speed * SPECIAL_RATE_DIVISOR;
        // `bgez; addiu 7; sra 3` at `0x801D4624..0x801D4630`.
        let step = (if raw < 0 { raw + 7 } else { raw }) >> 3;
        AfterimageActor {
            owner: slot,
            live_mask: 3,
            cursor: 0,
            step: step as i16,
        }
    }

    /// One frame. `record_n` / `clip_frames` are the special clip's ANM
    /// record byte `+6` and frame count `+2`; `None` when the host did not
    /// stage the fighter's clip bank, in which case the ghosts carry their
    /// cursor but no clip end can expire them and they retire with the
    /// special's exchange instead.
    pub fn tick(&mut self, frame_step: i32, clip: Option<(u8, u16)>) -> AfterimageFrame {
        let (n, frames) = clip.unwrap_or((1, i16::MAX as u16 / 0x10));
        let f = afterimage_pass(
            self.live_mask,
            self.cursor,
            frame_step,
            self.step,
            n,
            frames,
        );
        self.live_mask = f.live_mask;
        self.cursor = f.cursor;
        f
    }
}

// ---------------------------------------------------------------------------
// The round-start cameo walk-on (FUN_801D6310)
// ---------------------------------------------------------------------------

/// Pad bit (`_DAT_8007B850` held mask, Legaia's packed layout) whose hold at
/// round setup spawns the cameo: `0x10` = Triangle. The round-setup arm of the
/// cabinet (`0x32`) tests it at `0x801D0190..0x801D01AC` and spawns the
/// `0x801D7624` prototype, whose callback is [`cameo_pose`]'s routine.
pub const CAMEO_HOLD_MASK: u16 = 0x10;
/// Clip the cameo walks on (`+0x5C = 0x1D`).
pub const CAMEO_CLIP_WALK: i16 = 0x1D;
/// Clip it strikes its pose on (`+0x5C = 0x1C`).
pub const CAMEO_CLIP_POSE: i16 = 0x1C;
/// Fixed `y` / `z` of the cameo (`+0x16 = 0x8C`, `+0x18 = 0x400`), in the
/// camera-relative frame `+0x52 |= 0x400` selects.
pub const CAMEO_Y: i16 = 0x8C;
pub const CAMEO_Z: i16 = 0x400;
/// Phase at which the cameo raises its retire bit.
pub const CAMEO_RETIRE_PHASE: i16 = 0xF0;

/// The cameo's pose for one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameoPose {
    /// `+0x14` - screen-relative `x`: slides in from `0x100`, holds at `0`,
    /// slides out to the negative side.
    pub x: i16,
    pub y: i16,
    pub z: i16,
    /// `+0x26` - yaw: side-on (`0x400`) while walking, turned to face the
    /// camera (`0`) for the pose.
    pub yaw: i16,
    /// `+0x5C` - [`CAMEO_CLIP_WALK`] or [`CAMEO_CLIP_POSE`].
    pub clip: i16,
    /// `+0x62` bit 3: the pose clip holds its last frame instead of looping.
    pub hold_last_frame: bool,
    /// The VRAM cell the frame leaves showing, through [`sprite_blit`]'s
    /// table index (`0` until the pose, `1` from it on: both blits run every
    /// frame past phase `0x40`, and the second lands last).
    pub blit_index: usize,
    /// The retire bit rose this frame.
    pub retire: bool,
}

// NOT WIRED: nothing in the port spawns the cameo. Its spawn is the cabinet's
// round-setup arm (`0x32`) under a held Triangle, and none of the three hosts
// hands the duel a *held* pad word - they feed attack edges, and the port uses
// Triangle as its special. Past the spawn it needs a host that can draw scene
// model `0` (the prototype's `+0x04` half is `0`, copied to `+0x64` by
// `FUN_80020DE0`) posed by clips `0x1C` / `0x1D` of the scene's type-`0x05`
// clip bank in the camera-relative frame, and apply [`sprite_blit`]'s VRAM
// move to the texture it samples; no minigame host draws a model
// camera-relative or edits its VRAM copy after upload.
/// PORT: FUN_801D6310 - the round-start **cameo walk-on** animator.
///
/// Each frame it forces the actor's step `+0x6A = 8`, raises flag
/// `0x200000` and the camera-relative bit `+0x52 |= 0x400`, then poses the
/// actor from its phase `+0x22` (which it advances by the frame step after
/// the clip selector `FUN_800204F8` has run):
///
/// | phase | `x` | yaw | clip | cell |
/// |---|---|---|---|---|
/// | `0..0x20` | `0x100 - 8p` (walks in) | `0x400` | walk, looping | 0 |
/// | `0x20..0x40` | `0` | `0x400 - 32(p - 0x20)` (turns to camera) | walk | 0 |
/// | `0x40..0x90` | `0` | `0` | pose, held | 1 |
/// | `0x90..0xB0` | `0` | `32(p - 0x90)` (turns back) | walk, looping | 1 |
/// | `0xB0..` | `8(0xB0 - p)` (walks out) | `0x400` | walk | 1 |
///
/// and raises the retire bit from phase [`CAMEO_RETIRE_PHASE`] on. The cell
/// column is the blit helper [`sprite_blit`]: index `0` runs every frame from
/// phase `0`, index `1` every frame from `0x40`, both into the same VRAM cell.
///
/// A negative phase leaves `x` unwritten from an uninitialised register;
/// the spawn zeroes `+0x22` and the phase only ever grows, so no frame reaches
/// it, and the port returns `None` there.
pub fn cameo_pose(phase: i16) -> Option<CameoPose> {
    if phase < 0 {
        return None;
    }
    let p = phase as i32;
    let (mut x, mut yaw, mut clip, mut hold) = (0x100 - p * 8, 0x400, CAMEO_CLIP_WALK, false);
    let mut blit = 0;
    if p >= 0x20 {
        x = 0;
        yaw = 0x400 - (p - 0x20) * 32;
    }
    if p >= 0x40 {
        yaw = 0;
        clip = CAMEO_CLIP_POSE;
        hold = true;
        blit = 1;
    }
    if p >= 0x90 {
        yaw = (p - 0x90) * 32;
        clip = CAMEO_CLIP_WALK;
        hold = false;
    }
    if p >= 0xB0 {
        x = (0xB0 - p) * 8;
        yaw = 0x400;
    }
    Some(CameoPose {
        x: x as i16,
        y: CAMEO_Y,
        z: CAMEO_Z,
        yaw: yaw as i16,
        clip,
        hold_last_frame: hold,
        blit_index: blit,
        retire: phase >= CAMEO_RETIRE_PHASE,
    })
}

/// Whether the round setup spawns the cameo: the held pad word carries
/// [`CAMEO_HOLD_MASK`].
///
/// REF: FUN_801cf388 (`0x801D0190..0x801D01C4`)
pub fn cameo_spawns(held_pad: u16) -> bool {
    held_pad & CAMEO_HOLD_MASK != 0
}

// ---------------------------------------------------------------------------
// The developer action-table keyframe editor
// ---------------------------------------------------------------------------

/// First value of the match timer `DAT_801DBF44` that opens the editor band.
pub const EDITOR_PHASE_BASE: i32 = 400;
/// Width of that band - retail tests `DAT_801DBF44 - 400` **unsigned** against
/// this, so the editor runs in `400..=499` and nowhere else. The live-round
/// band is `DAT_801DBF44 == 100`, well below it.
pub const EDITOR_PHASE_SPAN: u32 = 100;
/// Number of entries in the per-character action table the editor cycles.
pub const EDITOR_ACTION_COUNT: u16 = 0x11;
/// Fixed-point shift the editor applies to a frame cursor before handing it to
/// the keyframe lookup.
pub const EDITOR_FRAME_SHIFT: u32 = 4;

/// What the editor decided to do with the selected keyframe this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorKeyEdit {
    /// The frame cursor did not move; nothing is installed or removed.
    None,
    /// `FUN_801D57BC` - install (or rewrite) the slot for this key.
    Install,
    /// `FUN_801D58E0` - remove the slot for this key.
    Remove,
}

/// One editor tick's decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorTick {
    /// The band gate failed: the editor actor and the fighter actor both
    /// take the retire bit and nothing else runs.
    pub retired: bool,
    /// The action cursor after wrapping.
    pub action: u16,
    /// The frame cursor after wrapping.
    pub frame: i16,
    /// The keyframe the current frame cursor lands on, if any.
    pub keyframe: Option<usize>,
    /// Whether the selected action changed this frame, which is what reloads
    /// the record's speed / power into the edit fields.
    pub action_changed: bool,
    pub edit: EditorKeyEdit,
}

/// Whether the editor band is open for a given match-timer value.
///
/// REF: FUN_801d4fc8 (`0x801D5018..0x801D5028`)
pub fn editor_band_open(match_timer: i32) -> bool {
    (match_timer.wrapping_sub(EDITOR_PHASE_BASE) as u32) < EDITOR_PHASE_SPAN
}

// REPLACED-BY: nothing is owed a port - retail never reaches it. The editor
// is *linked* rather than dead: nothing `jal`s `0x801D4FC8`, but its address
// is the callback word of the `0x18`-byte actor prototype at `0x801D766C`,
// which the `0x190` arm spawns (`lui`+`addiu` at `0x801D1D68`).
// The whole editor is a development screen behind a flag the shipped disc
// never raises. The band `0x190` / `0x191` is written in exactly one place,
// the developer menu arm (`0x801D19DC`, cabinet state `0xC8`), and the cabinet
// enters `0xC8` only when `_DAT_8007B868` is non-zero (`lw v1,-0x4798(v1)` at
// `0x801D08E8`, `beqz` to the pause menu `0xBF` otherwise). A disc-wide
// `find-gp-relative-refs.py --va 0x8007b868 --prot` sweep (SCUS, every based
// overlay image and every raw PROT entry) finds exactly two stores to that
// word: the boot store of `FUN_8002B92C`'s result (`0x80015F18`), a stub whose
// whole body is `jr ra; move v0, zero`, and a bit-clear (`and ~2` at
// `0x8001E008`). So retail boots it to zero and only ever clears it, and no
// shipped path reaches the editor or its two slot helpers.
/// PORT: FUN_801d4fc8 - the **developer action-table keyframe editor** tick.
///
/// It is not the fight's pose path. Outside the [`editor_band_open`] window it
/// retires both actors and returns; inside it, it draws the action / frame
/// cursors, reloads the record's speed (`+0x04`) and power (`+0x18`) into the
/// edit fields whenever the selected action changes, looks the current frame
/// cursor up through [`crate::baka_fighter::keyframe_in_range`], and - when
/// the frame cursor has moved - installs the keyframe through
/// [`anim_slot_install`] or removes it through its sibling `FUN_801D58E0`,
/// picked by the edit-mode flag. It then writes the edited values back into
/// the action record, wraps the action cursor at [`EDITOR_ACTION_COUNT`] and
/// the frame cursor at `record[+2] - 1`, spawns a marker effect at the
/// keyframe position and re-poses the fighter.
///
/// The frame cursor is handed to the lookup shifted left by
/// [`EDITOR_FRAME_SHIFT`], which is the fixed point the lookup expects on its
/// query rather than on the record.
#[allow(clippy::too_many_arguments)]
pub fn editor_tick(
    match_timer: i32,
    action: u16,
    prev_action: u16,
    frame: i16,
    prev_frame: i16,
    edit_mode: bool,
    keyframe_count: i16,
    frame_indices: &[i16],
    slots: &mut Vec<i16>,
) -> EditorTick {
    if !editor_band_open(match_timer) {
        return EditorTick {
            retired: true,
            action,
            frame,
            keyframe: None,
            action_changed: false,
            edit: EditorKeyEdit::None,
        };
    }
    let query = (frame as i32) << EDITOR_FRAME_SHIFT;
    let keyframe = crate::baka_fighter::keyframe_in_range(frame_indices, query, query);

    let mut edit = EditorKeyEdit::None;
    if frame != prev_frame {
        if edit_mode {
            anim_slot_install(slots, query);
            edit = EditorKeyEdit::Install;
        } else {
            anim_slot_delete(slots, query);
            edit = EditorKeyEdit::Remove;
        }
    }

    let action = if action >= EDITOR_ACTION_COUNT {
        0
    } else {
        action
    };
    let frame = if frame > keyframe_count.saturating_sub(1) {
        0
    } else {
        frame
    };

    EditorTick {
        retired: false,
        action,
        frame,
        keyframe,
        action_changed: action != prev_action,
        edit,
    }
}

// ---------------------------------------------------------------------------
// The duel HUD's three digit strips, placed
// ---------------------------------------------------------------------------

/// Stage pen of the one-glyph round digit.
pub const HUD_ROUND_PEN: (i32, i32) = (8, 98);
/// Stage pen of the 8 px right-aligned score field.
pub const HUD_SCORE_PEN: (i32, i32) = (40, 98);
/// Stage pen of the `0x10` px "GET COIN" numeral strip.
pub const HUD_COIN_PEN: (i32, i32) = (140, 98);
/// The round digit is one glyph, so a round past the row's ten cells has
/// nowhere to go; retail's widget row holds ten and the port clamps.
pub const HUD_ROUND_MAX: i32 = 9;

/// Where the duel HUD's three number drawers put their glyphs this frame, as
/// `(stage x, stage y, digit)`.
///
/// The three strips are `single_digit_cell` (the round), the right-aligned
/// score field and the coin strip, each at its own pen with its own cell
/// stride - and the two tally strips draw only while a match tally is up,
/// which is what `tally` carries: `(total, gold_remaining)`.
///
/// This is the *layout*. The glyph quads are
/// `legaia_engine_ui::ui_baka_strips::baka_digit_strip_draws_for`, which both
/// hosts call: a shared layout under two different emitters is exactly the
/// shape that let one host draw a summary line where the other drew the
/// retail cells.
pub fn hud_digit_placements(round: i32, tally: Option<(i32, i32)>) -> Vec<(i32, i32, u8)> {
    use crate::baka_fighter::{coin_digit_cells, right_aligned_number_cells, single_digit_cell};
    let mut out: Vec<(i32, i32, u8)> = Vec::new();
    let r = single_digit_cell((round + 1).clamp(0, HUD_ROUND_MAX) as u8);
    out.push((
        HUD_ROUND_PEN.0 + r.x_offset as i32,
        HUD_ROUND_PEN.1,
        r.digit,
    ));
    if let Some((total, gold_remaining)) = tally {
        for c in right_aligned_number_cells(total) {
            out.push((
                HUD_SCORE_PEN.0 + c.x_offset as i32,
                HUD_SCORE_PEN.1,
                c.digit,
            ));
        }
        for c in coin_digit_cells(gold_remaining) {
            out.push((HUD_COIN_PEN.0 + c.x_offset as i32, HUD_COIN_PEN.1, c.digit));
        }
    }
    out
}

/// The round chrome's draws as `(centre x, centre y, brightness, text)` rows
/// for a glyph-less host: a glyph draw shows its paged cell's digit, any
/// other widget its id. The HUD sprite page these widgets index is uploaded
/// by the standalone minigames page only, so the native window and the play
/// page print the draw where retail puts the quad - one label kernel, so the
/// two hosts cannot drift on what the chrome says.
pub fn chrome_labels(draws: &[ChromeDraw]) -> Vec<(i32, i32, i32, String)> {
    draws
        .iter()
        .map(|d| {
            let text = match d.glyph {
                Some(idx) => format!("{}", idx.rem_euclid(10)),
                None => format!("w{:02x}", d.widget),
            };
            (i32::from(d.x), i32::from(d.y), d.brightness, text)
        })
        .collect()
}

#[cfg(test)]
mod hud_strip_tests {
    use super::*;

    #[test]
    fn without_a_tally_only_the_round_digit_is_placed() {
        let out = hud_digit_placements(0, None);
        assert_eq!(out, vec![(HUD_ROUND_PEN.0, HUD_ROUND_PEN.1, 1)]);
    }

    #[test]
    fn the_two_tally_strips_take_their_own_pens_and_strides() {
        let out = hud_digit_placements(2, Some((42, 7)));
        // Round 3, then "4" "2" 8 px apart, then the coin strip.
        assert_eq!(out[0], (HUD_ROUND_PEN.0, HUD_ROUND_PEN.1, 3));
        let score: Vec<&(i32, i32, u8)> = out
            .iter()
            .filter(|p| p.0 >= HUD_SCORE_PEN.0 && p.0 < HUD_COIN_PEN.0)
            .collect();
        assert_eq!(score.len(), 2);
        assert_eq!(score[1].0 - score[0].0, 8);
        let coin: Vec<&(i32, i32, u8)> = out.iter().filter(|p| p.0 >= HUD_COIN_PEN.0).collect();
        assert_eq!(coin.len(), 1);
        assert_eq!(coin[0].2, 7);
    }

    #[test]
    fn a_round_past_the_row_clamps_rather_than_indexing_off_it() {
        let out = hud_digit_placements(40, None);
        assert_eq!(out[0].2, HUD_ROUND_MAX as u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_editor_band_is_an_unsigned_window_above_the_round_band() {
        assert!(!editor_band_open(100));
        assert!(!editor_band_open(399));
        assert!(editor_band_open(400));
        assert!(editor_band_open(499));
        assert!(!editor_band_open(500));
        // The unsigned test also rejects everything below the base.
        assert!(!editor_band_open(-1));
    }

    #[test]
    fn outside_the_band_the_editor_retires_and_does_nothing() {
        let mut slots = Vec::new();
        let t = editor_tick(100, 5, 4, 2, 1, true, 8, &[0, 1, 2], &mut slots);
        assert!(t.retired);
        assert_eq!(t.edit, EditorKeyEdit::None);
        assert!(slots.is_empty());
    }

    #[test]
    fn a_moved_frame_cursor_installs_or_removes_by_the_edit_mode() {
        let mut slots = Vec::new();
        let t = editor_tick(400, 0, 0, 2, 1, true, 8, &[0, 1, 2, 3], &mut slots);
        assert_eq!(t.edit, EditorKeyEdit::Install);
        assert_eq!(slots, vec![2]);
        assert_eq!(t.keyframe, Some(2));

        let t = editor_tick(400, 0, 0, 2, 1, false, 8, &[0, 1, 2, 3], &mut slots);
        assert_eq!(t.edit, EditorKeyEdit::Remove);

        // A still cursor edits nothing.
        let t = editor_tick(400, 0, 0, 2, 2, true, 8, &[0, 1, 2, 3], &mut slots);
        assert_eq!(t.edit, EditorKeyEdit::None);
    }

    #[test]
    fn the_editor_wraps_both_cursors() {
        let mut slots = Vec::new();
        let t = editor_tick(
            400,
            EDITOR_ACTION_COUNT,
            0,
            9,
            9,
            false,
            4,
            &[0, 1],
            &mut slots,
        );
        assert_eq!(t.action, 0);
        assert_eq!(t.frame, 0);
    }

    #[test]
    fn impact_pair_mirrors_by_slot_and_lifts_by_the_special_latch() {
        let a = impact_effect_pair(0, (100, 20, 300), (5, 6, 7), true, false);
        assert_eq!(a[0].pos, (105, 26, 307 - IMPACT_Z_LIFT));
        assert_eq!(a[0].rot, (0, 0, 0));
        assert_eq!(a[0].template, IMPACT_TEMPLATE_A[0]);
        assert_eq!(a[1].rot, (0, IMPACT_YAW[0], 0));
        assert_eq!(a[1].template, IMPACT_TEMPLATE_B[0]);

        // A clear facing flag subtracts the keyframe X instead.
        let b = impact_effect_pair(1, (100, 20, 300), (5, 6, 7), false, true);
        assert_eq!(b[0].pos, (95, 26, 307));
        assert_eq!(b[1].rot, (0, IMPACT_YAW[1], 0));
        assert_eq!(IMPACT_YAW[0], -IMPACT_YAW[1]);
    }

    #[test]
    fn impact_keyframe_index_honours_the_reset_argument() {
        assert_eq!(impact_keyframe_index(4, false), 4);
        assert_eq!(impact_keyframe_index(4, true), 0);
    }

    #[test]
    fn sprite_blit_biases_the_source_rect_off_the_table_entry() {
        let c = sprite_blit(0, [0x40, 0x10]).unwrap();
        assert_eq!(c.src_x, 0x10 + BLIT_SRC_X_BASE);
        assert_eq!(c.src_y, 0x10 + BLIT_SRC_Y_BASE);
        assert_eq!(c.size, (6, 0x18));
        // The destination is fixed, not table-driven.
        assert_eq!((c.dst_x, c.dst_y), (0x340, BLIT_DST_Y));
        // Only mode 0 is a defined call.
        assert!(sprite_blit(1, [0x40, 0x10]).is_none());
    }

    #[test]
    fn an_empty_live_mask_retires_the_afterimage() {
        let f = afterimage_pass(0, 0, 1, 0, 1, 1);
        assert!(f.retire);
        assert!(f.passes.is_empty());
    }

    #[test]
    fn afterimage_ghosts_lag_three_frames_each_and_darken() {
        // Both bits live, one frame past the spawn: step 8 at n = 1 advances
        // 16 (one whole frame); the ghosts sit 3 and 6 frames behind it and
        // both are still before the clip start.
        let f = afterimage_pass(0b11, 0, 1, 8, 1, 30);
        assert_eq!(f.cursor, 16);
        assert_eq!(f.passes[0].cursor, 16 - 0x30);
        assert_eq!(f.passes[1].cursor, 16 - 0x60);
        assert_eq!(f.passes[0].cue, AFTERIMAGE_CUE_START);
        assert_eq!(f.passes[1].cue, AFTERIMAGE_CUE_START + AFTERIMAGE_CUE_STEP);
        assert!(f.passes.iter().all(|p| !p.drawn && !p.expired));
    }

    #[test]
    fn afterimage_ghosts_expire_on_the_clip_end() {
        // Cursor far past a one-frame clip: both expire, mask clears.
        let f = afterimage_pass(0b11, 0x400, 0, 0, 1, 1);
        assert!(f.passes.iter().all(|p| p.expired && !p.drawn));
        assert_eq!(f.live_mask, 0);
        assert_eq!(f.cursor, 0x400);
        // Inside the clip a ghost draws.
        let f = afterimage_pass(0b01, 0x100, 0, 0, 1, 0x40);
        assert!(f.passes[0].drawn);
        assert_eq!(f.live_mask, 0b01);
    }

    #[test]
    fn afterimage_spawn_halves_the_special_speed() {
        // `speed * 4 >> 3`, rounded toward zero.
        assert_eq!(AfterimageActor::spawn(1, 13).step, 6);
        assert_eq!(AfterimageActor::spawn(0, -3).step, -1);
        let mut a = AfterimageActor::spawn(0, 8);
        assert_eq!((a.live_mask, a.cursor, a.step), (3, 0, 4));
        // n = 1: the pass doubles the step, so the ghost clip runs at the
        // pre-special rate while the fighter plays at half.
        a.tick(1, Some((1, 30)));
        assert_eq!(a.cursor, 8);
    }

    #[test]
    fn cameo_walks_in_turns_poses_and_walks_out() {
        let p = cameo_pose(0).unwrap();
        assert_eq!(
            (p.x, p.yaw, p.clip, p.blit_index),
            (0x100, 0x400, CAMEO_CLIP_WALK, 0)
        );
        assert_eq!((p.y, p.z), (CAMEO_Y, CAMEO_Z));
        assert_eq!(cameo_pose(0x10).unwrap().x, 0x80);
        let p = cameo_pose(0x30).unwrap();
        assert_eq!((p.x, p.yaw), (0, 0x200));
        let p = cameo_pose(0x40).unwrap();
        assert_eq!(
            (p.yaw, p.clip, p.hold_last_frame, p.blit_index),
            (0, CAMEO_CLIP_POSE, true, 1)
        );
        let p = cameo_pose(0xA0).unwrap();
        assert_eq!(
            (p.yaw, p.clip, p.hold_last_frame),
            (0x200, CAMEO_CLIP_WALK, false)
        );
        let p = cameo_pose(0xC0).unwrap();
        assert_eq!(
            (p.x, p.yaw, p.blit_index, p.retire),
            (-0x80, 0x400, 1, false)
        );
        assert!(cameo_pose(CAMEO_RETIRE_PHASE).unwrap().retire);
        assert!(cameo_pose(-1).is_none());
        assert!(cameo_spawns(0x10) && cameo_spawns(0x110) && !cameo_spawns(0x40));
    }

    #[test]
    fn intro_fires_each_announcer_line_once() {
        let mut st = IntroTitle::default();
        let mut lines = Vec::new();
        for t in 0..200 {
            if let Some(c) = st.frame(t).xa {
                lines.push((c.clip, c.chan, c.dur));
            }
        }
        assert_eq!(lines, vec![(0x20, 0x0E, 0x3F), (0x20, 0x0F, 0x76)]);
        assert_eq!(st.announced, 2);
    }

    #[test]
    fn intro_logo_brightness_ramps_then_holds() {
        let mut st = IntroTitle::default();
        assert!(st.frame(29).draws.is_empty());
        let f = st.frame(30);
        assert_eq!(f.draws[0].brightness, 0);
        let mut st = IntroTitle::default();
        assert_eq!(st.frame(40).draws[0].brightness, 80);
        let mut st = IntroTitle::default();
        // The multiplier clamps at 0x10 once t - 30 reaches 0x11.
        assert_eq!(st.frame(60).draws[0].brightness, 0x80);
        let mut st = IntroTitle::default();
        assert_eq!(st.frame(99).draws[0].brightness, 0x80);
    }

    #[test]
    fn intro_subtitle_shrinks_and_flashes_the_clut() {
        let mut st = IntroTitle::default();
        let f = st.frame(100);
        assert_eq!(f.banner_clut, Some(BANNER_CLUT_FLASH));
        assert_eq!(f.tint, Some(ScreenTint { grey: 0xFF }));
        let sub = f.draws.iter().find(|d| d.widget == 0x22).unwrap();
        assert_eq!(sub.size, (0x10 << 11) + 0x1000);
        let mut st = IntroTitle::default();
        let f = st.frame(116);
        assert_eq!(f.banner_clut, Some(BANNER_CLUT_IDLE));
        let sub = f.draws.iter().find(|d| d.widget == 0x22).unwrap();
        assert_eq!(sub.size, 0x1000);
        assert!(f.tint.is_none());
    }

    #[test]
    fn intro_caption_ramp_walks_four_cells() {
        let cell = |t: i32| {
            let f = IntroTitle { announced: 2 }.frame(t);
            f.draws
                .iter()
                .find(|d| (0x24..=0x27).contains(&d.widget))
                .map(|d| d.widget)
        };
        assert_eq!(cell(140), None);
        assert_eq!(cell(147), Some(0x24));
        assert_eq!(cell(152), Some(0x25));
        assert_eq!(cell(156), Some(0x26));
        assert_eq!(cell(160), Some(0x27));
        assert_eq!(cell(164), None);
    }

    #[test]
    fn intro_tint_fades_white_to_black_over_64_frames() {
        let g = |t: i32| IntroTitle { announced: 2 }.frame(t).tint.unwrap().grey;
        assert_eq!(g(140), 0xFF);
        assert_eq!(g(150), 0xFF - 40);
        assert_eq!(g(204), 0x00);
        assert_eq!(g(400), 0x00);
    }

    #[test]
    fn banner_voice_channel_is_the_round_index() {
        let f = round_banner_frame(0, 2);
        assert_eq!(
            f.xa,
            Some(XaCue {
                clip: 0x1F,
                chan: 2,
                dur: 0x48
            })
        );
        assert!(round_banner_frame(1, 2).xa.is_none());
    }

    #[test]
    fn banner_converges_then_parts() {
        // Slide in: the offset walks 0xB4 -> 0 in 6px steps.
        assert_eq!(round_banner_frame(0, 0).draws[0].x, 0xB4 + 0x90);
        assert_eq!(round_banner_frame(29, 0).draws[0].x, 0xB4 - 29 * 6 + 0x90);
        // Joined pose: level ramps 0x80 -> 0xFF over 30 frames.
        assert_eq!(round_banner_frame(30, 0).draws[0].brightness, 0x80);
        assert_eq!(round_banner_frame(60, 0).draws[0].brightness, 0xFF);
        // Held at the clamp until the part starts.
        assert_eq!(round_banner_frame(89, 0).draws[0].brightness, 0xFF);
        // Parting: level falls from 0xC8, halved for the parted pose.
        assert_eq!(round_banner_frame(90, 0).draws[0].brightness, 0xC8 / 2);
    }

    #[test]
    fn banner_digit_is_round_plus_one() {
        let f = round_banner_frame(0, 3);
        let g = f.draws.iter().find(|d| d.widget == GLYPH_WIDGET).unwrap();
        assert_eq!(g.glyph, Some(4));
        assert_eq!(glyph_u(4), 96);
    }

    #[test]
    fn banner_flags_track_the_parted_pose() {
        assert!(round_banner_flags(0));
        assert!(!round_banner_flags(30));
        assert!(!round_banner_flags(89));
        assert!(round_banner_flags(90));
    }

    #[test]
    fn countdown_walks_its_four_states() {
        let mut st = Countdown::default();
        let f = countdown_frame(&mut st, 1, false, 0, 0, 0);
        assert_eq!(st.state, 2);
        // Both lines cannot fire on the same frame - the second wins the slot,
        // exactly as retail's single call does.
        assert_eq!(f.xa.unwrap().chan, 0x0B);
        assert_eq!(st.timer, COUNTDOWN_TIMER);
        // Below the fade gate nothing decays.
        countdown_frame(&mut st, 4, false, 0x10, 0, 0);
        assert_eq!(st.timer, COUNTDOWN_TIMER);
        for _ in 0..8 {
            countdown_frame(&mut st, 4, false, 0x11, 0, 0);
        }
        // Eight ticks land the timer exactly on zero - the transition needs
        // the ninth, because retail tests the post-decrement value for `< 0`.
        assert_eq!(st.timer, 0);
        assert_eq!(st.state, 2);
        countdown_frame(&mut st, 4, false, 0x11, 0, 0);
        assert_eq!(st.timer, 0);
        assert_eq!(st.state, 3);
    }

    #[test]
    fn countdown_final_round_swaps_line_and_widget() {
        let mut st = Countdown { state: 2, timer: 1 };
        let f = countdown_frame(&mut st, 4, false, 0x20, 0, COUNTDOWN_FINAL_ROUND);
        assert_eq!(f.xa.unwrap().chan, 0x0D);
        assert!(f.draws.iter().any(|d| d.widget == 0x1D));
        let mut st = Countdown { state: 2, timer: 1 };
        let f = countdown_frame(&mut st, 4, false, 0x20, 0, 0);
        assert_eq!(f.xa.unwrap().chan, 0x0C);
        assert!(f.draws.iter().any(|d| d.widget == 0x1C));
    }

    #[test]
    fn countdown_loading_freezes_the_sequence_but_still_draws() {
        let mut st = Countdown::default();
        let f = countdown_frame(&mut st, 4, true, 0x40, 0x40, 0);
        assert_eq!(st.state, 1);
        assert_eq!(f.draws.len(), 2);
        assert_eq!(f.draws[0].brightness, 0x20);
    }

    #[test]
    fn actor_draw_conditions_the_fade_level() {
        let a = ChromeActor {
            x: 10,
            y: 20,
            id: 7,
            size: 0x800,
            fade: 0x100,
        };
        let d = chrome_actor_draw(&a, 0, 0).draw.unwrap();
        assert_eq!(d.brightness, 0x10);
        assert_eq!(d.size, 0x800);
        // Above 0x4000 the level is discarded, not clamped.
        let a = ChromeActor { fade: 0x4001, ..a };
        assert_eq!(chrome_actor_draw(&a, 0, 0).draw.unwrap().brightness, 0);
        // Saturation at 0xFF.
        let a = ChromeActor { fade: 0x4000, ..a };
        assert_eq!(chrome_actor_draw(&a, 0, 0).draw.unwrap().brightness, 0xFF);
    }

    #[test]
    fn actor_draw_modes() {
        let a = ChromeActor {
            x: 1,
            y: 2,
            id: 3,
            size: 0x1000,
            fade: 0x800,
        };
        assert!(!chrome_actor_draw(&a, 0, 1).retire);
        assert!(chrome_actor_draw(&a, 1, 1).retire);
        assert!(!chrome_actor_draw(&a, 1, 0).retire);
        let g = chrome_actor_draw(&a, 2, 0).draw.unwrap();
        assert_eq!(g.widget, GLYPH_WIDGET);
        assert_eq!(g.glyph, Some(3));
        assert_eq!(g.size, 0x1000);
        assert!(chrome_actor_draw(&a, 3, 0).draw.is_none());
    }

    #[test]
    fn anim_slots_reuse_then_append_then_saturate() {
        let mut keys = Vec::new();
        let a = anim_slot_install(&mut keys, 0x30);
        assert_eq!((a.slot, a.reused, a.count), (0, false, 1));
        let b = anim_slot_install(&mut keys, 0x3F);
        // 0x3F >> 4 == 3 == 0x30 >> 4, so it lands back in slot 0.
        assert_eq!((b.slot, b.reused, b.count), (0, true, 1));
        // Key 3 is already resident, so seven fresh keys fill the table.
        for k in 4..11 {
            anim_slot_install(&mut keys, k * 0x10);
        }
        assert_eq!(keys.len(), 8);
        assert!(anim_slot_install(&mut keys, 0x900).full);
        assert_eq!(keys.len(), 8);
    }

    #[test]
    fn anim_slot_delete_compacts_and_reports_the_index() {
        let mut keys = Vec::new();
        for k in 0..4 {
            anim_slot_install(&mut keys, k * 0x10);
        }
        assert_eq!(keys, vec![0, 1, 2, 3]);

        // A miss changes nothing and reports `-1` (here: `None`).
        let miss = anim_slot_delete(&mut keys, 0x90);
        assert_eq!(miss.slot, None);
        assert_eq!(miss.count, 4);
        assert_eq!(keys, vec![0, 1, 2, 3]);

        // The query takes the same `>> 4` the installer used, so 0x1F finds
        // the slot 0x10 installed.
        let hit = anim_slot_delete(&mut keys, 0x1F);
        assert_eq!(hit.slot, Some(1));
        assert_eq!(hit.count, 3);
        // Everything above the hole moved down one place.
        assert_eq!(keys, vec![0, 2, 3]);

        // Deleting the last live slot needs no move at all.
        let tail = anim_slot_delete(&mut keys, 0x30);
        assert_eq!((tail.slot, tail.count), (Some(2), 2));
        assert_eq!(keys, vec![0, 2]);
    }

    #[test]
    fn anim_slot_key_truncates_toward_zero_on_both_sides() {
        // The `+0xF` bias is what makes the arithmetic shift truncate toward
        // zero instead of toward negative infinity.
        assert_eq!(anim_slot_key(0x1F), 1);
        assert_eq!(anim_slot_key(-0x1F), -1);
        assert_eq!(anim_slot_key(-0x20), -2);
        assert_eq!(anim_slot_key(-1), 0);
    }

    #[test]
    fn editor_removes_the_keyframe_when_edit_mode_is_off() {
        let mut slots = vec![0i16, 1, 2];
        let t = editor_tick(
            EDITOR_PHASE_BASE,
            0,
            0,
            1,
            0,
            false,
            8,
            &[0, 0x10, 0x20],
            &mut slots,
        );
        assert_eq!(t.edit, EditorKeyEdit::Remove);
        // frame 1 << EDITOR_FRAME_SHIFT, taken back down by `>> 4`, is key 1.
        assert_eq!(slots, vec![0, 2]);
    }

    #[test]
    fn actor_bind_retires_once_the_match_ends() {
        let b = chrome_actor_bind(0, 1, 3, (3, 4), 0, &[], &[]);
        assert_eq!(b.flags, ACTOR_FLAG_RETIRE);
        assert_eq!(b.frames, None);
    }

    #[test]
    fn actor_bind_picks_bank_and_frame_count() {
        // Offset-table word 1 -> record at byte 0x20; byte +7 of it is the
        // frame count.
        let mut bank = vec![0u8; 64];
        bank[4..8].copy_from_slice(&0x20u32.to_le_bytes());
        bank[0x20 + 7] = 0x1B;
        let b = chrome_actor_bind(0xFF, 0, 3, (3, 4), 1, &bank, &[]);
        assert_eq!(b.frames, Some(0x1B));
        assert_eq!(b.fade, 0);
        assert_eq!(b.flags, 0xFF & !ACTOR_FLAG_TICK);
        assert!(!b.clear_accum);
        // At or above the split it is the other bank, and `+0x74` is cleared.
        // The id masks down to the same slot, which is what makes the split a
        // bank select rather than an index offset.
        let b = chrome_actor_bind(0, 0, 9, (3, 4), ANIM_BANK_SPLIT + 1, &[], &bank);
        assert_eq!(b.frames, Some(0x1B));
        assert_eq!(b.fade, 0x800);
        assert!(b.clear_accum);
    }

    #[test]
    fn actor_hold_sets_one_bit() {
        assert_eq!(chrome_actor_hold(1), 1 | ACTOR_FLAG_HOLD);
    }

    #[test]
    fn the_runner_spawns_a_banner_actor_and_drives_the_timeline() {
        let mut c = BakaChrome::default();
        assert!(!c.busy());
        c.start_round_banner(ROUND_BANNER_SPRITE);
        assert!(c.busy());
        assert_eq!(c.sprites().len(), 1);
        // The spawn goes through the shared screen-centre wrapper.
        assert_eq!(c.sprites()[0].actor.x, 0xA0);
        assert_eq!(c.sprites()[0].actor.y, 0x78);
        assert_eq!(c.sprites()[0].flags & ACTOR_FLAG_HOLD, ACTOR_FLAG_HOLD);

        let tick = ChromeTick::default();
        let f = c.step(&tick, (&[], &[]));
        // Frame 0 fires the banner's announce line and draws the banner's
        // four quads plus the countdown's two.
        assert_eq!(f.xa.map(|x| x.clip), Some(0x1F));
        assert!(f.draws.len() >= 6);
    }

    #[test]
    fn the_runner_retires_its_banner_actors_once_the_match_ends() {
        let mut c = BakaChrome::default();
        c.start_round_banner(ROUND_BANNER_SPRITE);
        let tick = ChromeTick {
            match_phase: 1,
            ..ChromeTick::default()
        };
        c.step(&tick, (&[], &[]));
        assert!(c.sprites().is_empty());
    }

    #[test]
    fn the_banner_timeline_runs_out_and_disarms() {
        let mut c = BakaChrome::default();
        c.start_round_banner(ROUND_BANNER_SPRITE);
        let tick = ChromeTick::default();
        for _ in 0..BANNER_END {
            c.step(&tick, (&[], &[]));
        }
        assert!(!c.busy());
    }

    #[test]
    fn the_intro_card_runs_only_while_armed() {
        let mut plain = BakaChrome::default();
        let tick = ChromeTick::default();
        assert!(plain.step(&tick, (&[], &[])).draws.is_empty());

        let mut c = BakaChrome::with_intro();
        assert!(c.busy());
        // The card is silent before its first range opens.
        assert!(c.step(&tick, (&[], &[])).draws.is_empty());
        for _ in 0..INTRO_LOGO_IN {
            c.step(&tick, (&[], &[]));
        }
        assert!(!c.step(&tick, (&[], &[])).draws.is_empty());
        for _ in 0..INTRO_END {
            c.step(&tick, (&[], &[]));
        }
        assert!(!c.busy());
    }

    #[test]
    fn glyph_u_wraps_like_the_byte_store() {
        assert_eq!(glyph_u(0), 0);
        assert_eq!(glyph_u(1), 24);
        assert_eq!(glyph_u(10), 240);
        assert_eq!(glyph_u(11), 8);
    }
}
