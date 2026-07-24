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

/// Animation-id threshold that picks between the two sprite banks.
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
/// `_DAT_8007B888` bank below [`ANIM_BANK_SPLIT`] and the `_DAT_8007B840`
/// bank at or above it (the at-or-above arm also clears `+0x74`), the id's
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

// NOT WIRED: its only retail caller is the developer keyframe editor
// `FUN_801D4FC8`, which runs in the `DAT_801DBF44 == 400..500` editor
// sub-phase - a phase no shipping path enters and the port does not model.
// Reaching it needs that editor tick plus the per-fighter animation slot
// array it edits; `baka_fighter::FighterState` carries action ids, not slots.
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
    let key = (if key_raw >= 0 { key_raw } else { key_raw + 0xF } >> 4) as i16;
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
    /// `banks` are the two runtime sprite archives the bind resolves frame
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

#[cfg(test)]
mod tests {
    use super::*;

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
