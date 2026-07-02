//! PROT-0900 **screen-effect widget family** - the 2D presentation layer the
//! field/event VM drives during cutscene-style sequences (iris mask, scripted
//! sprites, image panel, letterbox bands). Clean-room port from the resident
//! slot-B overlay PROT 0900 (link base `0x801F69D8`).
//!
//! ## What this is (and what it is not)
//!
//! This family was the long-open "quad-emit / matrix half of `FUN_801F811C`"
//! thread. Static decode of PROT 0900 at the correct link base resolves it:
//! **there is no per-part 3D matrix in this path.** `FUN_801F811C` is the
//! per-frame handler of a 2D *screen-mask widget* - its four tweened channels
//! (`+0x3c/3e/40/42` targets against `+0x14/16/18/1a` current) are the
//! **left / top / right / bottom edges of a screen rectangle**, and the four
//! emitted quads are the black border bands framing that rectangle (an iris /
//! letterbox wipe). The genuine `RotMatrixX/Y/Z` matrix code elsewhere in
//! PROT 0900 belongs to the top-view grid-instance renderer (`FUN_801F7088` +
//! its second-cluster sibling), a separate subsystem that a live trace already
//! showed does **not** run during a player summon.
//!
//! ## Structure (all pinned from PROT 0900 file bytes at base `0x801F69D8`)
//!
//! Widgets are actors on the generic effect-actor list (`_DAT_8007C34C`),
//! allocated by SCUS `FUN_80020DE0(descriptor, list)` which binds the per-frame
//! handler from `descriptor+8` at `actor+0xc`; `FUN_8003CF04(list, handler)`
//! finds the live widget by that handler. Four 0x18-byte handler-binding
//! descriptors live at `0x801F8FE4/8FFC/9014/902C` (each `[u32 0][u16 0]
//! [u16 0xFFFF][u32 handler][u32 0][...]`):
//!
//! | kind | handler | per-frame behaviour | spawn / control API |
//! |---|---|---|---|
//! | sprite | `FUN_801F7A9C` | script-driven tweened 2D sprite (GP0 `0x64` SPRT) | `FUN_801F8004(record)` |
//! | mask | `FUN_801F811C` | 4-edge rect tween + 4 black border quads | `FUN_801F8D4C(l,t,r,b,dur)` |
//! | panel | `FUN_801F849C` | 5-channel tween + 1–2 textured 15bpp quads | `FUN_801F88FC(rec)` spawn, `FUN_801F8E6C(x,y,scale,dur)` move/scale |
//! | letterbox | `FUN_801F8A34` | 2 solid bands + 2 gradient feather quads | `FUN_801F8F28(block)` |
//!
//! The sprite handler additionally interprets a byte-coded widget script
//! (opcode byte `0x40`, sub-op at `+2` dispatched through the 5-entry table at
//! the overlay head `0x801F69D8`: kill / wait-flag-set / wait-flag-clear /
//! tween-pos+colour / tween-colour). Tweens use the shared multi-mode
//! interpolator `FUN_801DE4C8` re-evaluated **from a captured start value**
//! each frame (not iteratively from the current value).
//!
//! ## Consumers
//!
//! The spawn/control APIs are called by **field-VM op `0x43` sub-ops** -
//! sub-`0x10` sprite / `0x11` mask / `0x13` panel / `0x14` panel-move /
//! `0x15` letterbox, via the 0x43 sub-op JT at `0x801CEDA8` (`jal` sites
//! inside `FUN_801DE840` at `0x801DF918`, `0x801DF974`, `0x801DFA70`,
//! `0x801DFABC`, `0x801DFACC`). On disc only the eight ending-sequence
//! scenes' partition-2 cutscene scripts invoke them.
//! The earlier reading that the summon stagers
//! (0910..0915) reference these functions was **VA aliasing**: those hits are
//! in-file `FUN_80021B04` part records whose addresses coincide with the 0900
//! handler VAs under the shared slot-B base.
//!
//! Capture evidence: PROT 0900 file `0x0640..0x2660` (covering this whole
//! family) is byte-resident at `0x801F7018..0x801F9038` in the fingerprinted
//! `battle_gimard_tail_fire_a` save; the function bodies are byte-identical to
//! the dance / baka-fighter overlay images (`overlay_dance_801f811c.txt` etc.).
//!
//! Retail screen parameters live in the render scratchpad block at
//! `0x1F800314`: `+0x74` (`0x1F800388`) = draw-area X origin, `+0x76`
//! (`0x1F80038A`) = Y offset, `+0x7A` (`0x1F80038E`) = screen height. The
//! engine models them as [`Screen`].
//!
//! Cross-referenced infrastructure (not ported here): the actor allocator /
//! finder pair, the field-VM caller, the part stager whose records VA-alias
//! the handlers, the grid renderer, the unaligned readers + story-flag test
//! the scripts use, and the texpage-packet builder.
// REF: FUN_80020DE0, FUN_8003CF04, FUN_801DE840, FUN_80021B04, FUN_801F7088
// REF: FUN_8003CE9C, FUN_8003CEB8, FUN_8003CE64, FUN_80059010

/// Interpolation modes of the shared tween helper `FUN_801DE4C8`.
///
/// Mode 1 is plain linear; 2/3 are quadratic ease-out / ease-in; 4 is the
/// two-segment ease-in-out (first half mode-3 toward the midpoint over `D/2`,
/// second half mode-2 from the midpoint). All arithmetic is integer with
/// truncating division, exactly as retail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpMode {
    /// `(a-b)*t/D + b`
    Linear,
    /// `e = (a-b)*t; (e + (e/D)*(D-t))/D + b` - quadratic ease-out.
    EaseOut,
    /// `((a-b)*t/D)*t/D + b` - quadratic ease-in.
    EaseIn,
    /// Ease-in to the midpoint over `D/2`, then ease-out to the target.
    EaseInOut,
}

impl InterpMode {
    /// Decode a script mode byte (`FUN_801DE4C8`'s `param_5`). Any value other
    /// than 2/3/4 takes the linear arm (retail's `default`).
    pub fn from_byte(b: u8) -> Self {
        match b {
            2 => InterpMode::EaseOut,
            3 => InterpMode::EaseIn,
            4 => InterpMode::EaseInOut,
            _ => InterpMode::Linear,
        }
    }
}

/// Multi-mode integer interpolator - full port of `FUN_801DE4C8(a, b, t, D,
/// mode)`: interpolate from `start` (`b`) toward `target` (`a`) at time `t` of
/// duration `dur`. Returns `target` exactly when `target == start` or
/// `t >= dur`. Division truncates toward zero (MIPS `div`), which is Rust `/`
/// on `i32`.
// PORT: FUN_801DE4C8
pub fn interp(target: i32, start: i32, t: i32, dur: i32, mode: InterpMode) -> i32 {
    if target == start || dur <= t {
        return target;
    }
    match mode {
        InterpMode::Linear => (target - start) * t / dur + start,
        InterpMode::EaseOut => {
            let e = (target - start) * t;
            (e + (e / dur) * (dur - t)) / dur + start
        }
        InterpMode::EaseIn => ((target - start) * t / dur) * t / dur + start,
        InterpMode::EaseInOut => {
            let half = dur >> 1;
            if half < t {
                // Second half: ease-out from the midpoint over the remainder.
                let mid = start + ((target - start) >> 1);
                interp(target, mid, t - half, half, InterpMode::EaseOut)
            } else {
                // First half: ease-in covering half the distance.
                let span = (target - start) >> 1;
                (span * t / half) * t / half + start
            }
        }
    }
}

/// Screen geometry the widgets draw against. Retail reads these from the
/// render scratchpad (`0x1F800388` X origin, `0x1F80038A` Y offset,
/// `0x1F80038E` height); the right edge is the literal `0x140`.
#[derive(Debug, Clone, Copy)]
pub struct Screen {
    /// Draw-area X origin (scratch `+0x74`).
    pub x0: i16,
    /// Y offset subtracted by the panel / letterbox draws (scratch `+0x76`).
    pub y_off: i16,
    /// Screen height (scratch `+0x7a`).
    pub height: i16,
}

/// Hardcoded right edge of every widget draw (retail literal `0x140`).
pub const SCREEN_W: i16 = 0x140;

impl Default for Screen {
    fn default() -> Self {
        Screen {
            x0: 0,
            y_off: 0,
            height: 240,
        }
    }
}

/// Read a little-endian `i16` at `off` (the unaligned reader `FUN_8003CE9C`).
fn rs16(b: &[u8], off: usize) -> i16 {
    i16::from_le_bytes([b[off], b[off + 1]])
}

/// Read a little-endian `u24` at `off` (the unaligned reader `FUN_8003CEB8`).
/// The widget scripts use it for packed RGB.
fn ru24(b: &[u8], off: usize) -> [u8; 3] {
    [b[off], b[off + 1], b[off + 2]]
}

/// Per-byte tween of an RGB triple (the sprite handler's colour channels).
fn interp_rgb(target: [u8; 3], start: [u8; 3], t: i32, d: i32, mode: InterpMode) -> [u8; 3] {
    let mut out = [0u8; 3];
    for ((o, &tv), &sv) in out.iter_mut().zip(target.iter()).zip(start.iter()) {
        *o = interp(tv as i32, sv as i32, t, d, mode) as u8;
    }
    out
}

// ---------------------------------------------------------------------------
// Kind 1 - screen mask (iris) widget
// ---------------------------------------------------------------------------

/// One axis-aligned black border quad of the mask draw. Vertices are packed
/// exactly as the retail GP0 `0x28` (flat opaque quad, colour `0x000000`)
/// packet: `(left,top) (right,top) (left,bottom) (right,bottom)`, linked at
/// OT slot `+0x1c`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaskQuad {
    pub left: i16,
    pub top: i16,
    pub right: i16,
    pub bottom: i16,
}

/// The screen-mask widget (retail handler `FUN_801F811C`): a rectangle
/// `[L..R] x [T..B]` stays visible while four black quads cover everything
/// outside it. The four edges tween from their value at [`Self::set_rect`]
/// time toward the requested targets.
#[derive(Debug, Clone, Copy)]
pub struct MaskWidget {
    /// Current (latched) edges `[L, T, R, B]` - actor `+0x14/16/18/1a`. These
    /// only move on snap/latch; mid-tween display values are re-interpolated
    /// from here each frame.
    pub cur: [i16; 4],
    /// Target edges - actor `+0x3c/3e/40/42`.
    pub target: [i16; 4],
    /// Tween clock - actor `+0x9c`.
    pub t: i16,
    /// Tween duration - actor `+0x9e` (0 = no active tween, snap to target).
    pub dur: i16,
}

impl MaskWidget {
    /// Fresh-spawn state: fully open (the border quads are degenerate). Port
    /// of the spawn arm of `FUN_801F8D4C`: `cur = [x0, 0, 0x140, height-1]`.
    // PORT: FUN_801F8D4C
    pub fn spawn(screen: &Screen) -> Self {
        let open = [screen.x0, 0, SCREEN_W, screen.height - 1];
        MaskWidget {
            cur: open,
            target: open,
            t: 0,
            dur: 0,
        }
    }

    /// Start a tween of the visible rect toward `(l, t, r, b)` over `dur`
    /// frames - the `FUN_801F8D4C` control API. An edge passed as `-1` takes
    /// its full-open default (`x0` / `0` / `0x140` / `height-1`), so e.g.
    /// `set_rect(-1, -1, -1, -1, d)` opens the mask and `set_rect(160, 120,
    /// 160, 120, d)` irises shut on the screen centre.
    pub fn set_rect(&mut self, l: i16, t: i16, r: i16, b: i16, dur: i16, screen: &Screen) {
        let l = if l == -1 { screen.x0 } else { l };
        let t = if t == -1 { 0 } else { t };
        let r = if r == -1 { SCREEN_W } else { r };
        let b = if b == -1 { screen.height - 1 } else { b };
        self.target = [l, t, r, b];
        self.t = 0;
        self.dur = dur;
    }

    /// Advance the tween one frame and return the four border quads - the
    /// whole of `FUN_801F811C`. `frame_delta` is retail's per-frame byte
    /// `DAT_1F800393`.
    ///
    /// Faithful shape: when `dur == 0` the current edges snap to the targets;
    /// otherwise the clock advances (clamped to `dur`), each *display* edge is
    /// re-interpolated `interp(target, cur, t, dur, Linear)` from the latched
    /// `cur` (which does **not** move mid-tween), and on `t == dur` the edges
    /// latch to the targets and `dur` clears.
    // PORT: FUN_801F811C
    pub fn tick(&mut self, frame_delta: u8, screen: &Screen) -> [MaskQuad; 4] {
        // Locals seed from the targets; axes whose target == cur keep that.
        let mut e = self.target;
        if self.dur == 0 {
            self.cur = self.target;
        } else {
            self.t = self.t.wrapping_add(frame_delta as i16);
            if self.dur < self.t {
                self.t = self.dur;
            }
            for (i, edge) in e.iter_mut().enumerate() {
                if self.target[i] != self.cur[i] {
                    *edge = interp(
                        self.target[i] as i32,
                        self.cur[i] as i32,
                        self.t as i32,
                        self.dur as i32,
                        InterpMode::Linear,
                    ) as i16;
                }
            }
            if self.t == self.dur {
                self.dur = 0;
                self.cur = self.target;
                e = self.target;
            }
        }
        let [l, t, r, b] = e;
        [
            // Top band: rows above the rect.
            MaskQuad {
                left: screen.x0,
                top: 0,
                right: SCREEN_W,
                bottom: t,
            },
            // Bottom band: rows below the rect.
            MaskQuad {
                left: screen.x0,
                top: b,
                right: SCREEN_W,
                bottom: screen.height - 1,
            },
            // Left band (note the retail `L - 1` right edge).
            MaskQuad {
                left: screen.x0,
                top: t,
                right: l - 1,
                bottom: b,
            },
            // Right band.
            MaskQuad {
                left: r,
                top: t,
                right: SCREEN_W,
                bottom: b,
            },
        ]
    }
}

// ---------------------------------------------------------------------------
// Kind 0 - scripted sprite widget
// ---------------------------------------------------------------------------

/// Static fields decoded from a sprite-widget spawn record (the `FUN_801F8004`
/// derivation). On-record layout: `[x:i16][y:i16][w:i16][h:i16][tex_x:i16]
/// [tex_y:i16][clut_x:i16][clut_y:i16][rgb:u24]` then the widget script at
/// `+0x13`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteRecord {
    pub x: i16,
    pub y: i16,
    pub w: i16,
    pub h: i16,
    /// GP0 texpage selector: `(tex_x >> 6) + ((tex_y & !0xff) >> 4)`.
    pub texpage: i16,
    /// Texel U within the page: `(tex_x & 0x3f) << 2` (4bpp page units).
    pub u: u8,
    /// Texel V within the page: `tex_y & 0xff`.
    pub v: u8,
    /// GP0 CLUT id: `(clut_y << 6) + (clut_x >> 4)`.
    pub clut: i16,
    /// Initial modulation colour (`actor+0x74` and the tween start `+0x7c`).
    pub rgb: [u8; 3],
    /// Byte offset of the widget script within the record (always `0x13`).
    pub script_off: usize,
}

/// Byte offset of a sprite record's widget script (`FUN_801F8004` seeds the
/// cursor at `record + 0x13`).
pub const SPRITE_SCRIPT_OFF: usize = 0x13;

impl SpriteRecord {
    /// Decode the spawn record's header. Returns `None` when `rec` is shorter
    /// than the fixed header.
    // PORT: FUN_801F8004
    pub fn parse(rec: &[u8]) -> Option<Self> {
        if rec.len() < SPRITE_SCRIPT_OFF {
            return None;
        }
        let tex_x = rs16(rec, 8);
        let tex_y = rs16(rec, 0xa);
        let clut_x = rs16(rec, 0xc);
        let clut_y = rs16(rec, 0xe);
        Some(SpriteRecord {
            x: rs16(rec, 0),
            y: rs16(rec, 2),
            w: rs16(rec, 4),
            h: rs16(rec, 6),
            texpage: (tex_x >> 6) + ((tex_y & !0xff) >> 4),
            u: ((tex_x & 0x3f) << 2) as u8,
            v: (tex_y & 0xff) as u8,
            clut: ((clut_y as i32) * 0x40 + ((clut_x as i32) >> 4)) as i16,
            rgb: ru24(rec, 0x10),
            script_off: SPRITE_SCRIPT_OFF,
        })
    }
}

/// One frame's sprite draw: a GP0 `0x64` textured SPRT (colour-modulated,
/// opaque) at OT slot `+0xc`, preceded by a texpage packet (`FUN_80059010`
/// with the widget's `texpage`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteDraw {
    pub x: i16,
    pub y: i16,
    pub w: i16,
    pub h: i16,
    pub u: u8,
    pub v: u8,
    pub clut: i16,
    pub texpage: i16,
    pub rgb: [u8; 3],
}

/// Live state of a scripted sprite widget (retail handler `FUN_801F7A9C`).
#[derive(Debug, Clone)]
pub struct SpriteWidget {
    /// Current screen position - actor `+0x14/+0x16`.
    pub x: i16,
    pub y: i16,
    /// Tween-start position captured when a sub-op-3 tween begins - actor
    /// `+0x3c/+0x3e`.
    pub start_x: i16,
    pub start_y: i16,
    /// Current modulation colour - actor `+0x74..76`.
    pub rgb: [u8; 3],
    /// Tween-start colour - actor `+0x7c..7e`.
    pub start_rgb: [u8; 3],
    /// Tween clock - actor `+0x9c`.
    pub t: i16,
    /// Script cursor - actor `+0x90` (byte offset into the record).
    pub cursor: usize,
    /// Kill flag - actor flag bit `0x8`; set by sub-op 0, suppresses the draw.
    pub killed: bool,
    /// Static draw fields from the spawn record.
    pub rec: SpriteRecord,
}

/// Widget-script opcode byte: every sprite sub-op record starts `0x40`.
pub const SPRITE_SCRIPT_OP: u8 = 0x40;

impl SpriteWidget {
    /// Spawn from a parsed record (mirrors `FUN_801F8004`'s actor seeding).
    pub fn spawn(rec: SpriteRecord) -> Self {
        SpriteWidget {
            x: rec.x,
            y: rec.y,
            start_x: rec.x,
            start_y: rec.y,
            rgb: rec.rgb,
            start_rgb: rec.rgb,
            t: 0,
            cursor: rec.script_off,
            killed: false,
            rec,
        }
    }

    /// Run one frame of the widget script then return the draw (or `None`
    /// once killed) - the whole of `FUN_801F7A9C`. `script` is the spawn
    /// record's byte buffer (the cursor indexes into it); `flag_test` is the
    /// story-flag probe `FUN_8003CE64` (bit `idx` of the `0x80085758` bank).
    ///
    /// Sub-ops (`script[cursor+2]`, dispatched via the 5-entry table at the
    /// overlay head):
    ///
    /// * `0` - kill: set the actor's bit-8 flag; never draws again.
    /// * `1` - wait until `flag_test(arg)` is **set**; then `cursor += 5` and
    ///   continue interpreting the same frame.
    /// * `2` - wait until the flag is **clear**; then `cursor += 5`.
    /// * `3` - tween position + colour to `(x:i16@3, y:i16@5, rgb:u24@7)` with
    ///   ease mode `@0xa` over `dur:i16@0xb` frames; `cursor += 0xd` on
    ///   completion.
    /// * `4` - tween colour only to `rgb:u24@3`, mode `@6`, `dur:i16@7`;
    ///   `cursor += 9` on completion.
    ///
    /// A cursor byte other than `0x40` (or a sub-op `>= 5`) leaves the script
    /// parked and just draws.
    // PORT: FUN_801F7A9C
    pub fn tick(
        &mut self,
        script: &[u8],
        frame_delta: u8,
        mut flag_test: impl FnMut(u16) -> bool,
    ) -> Option<SpriteDraw> {
        loop {
            if self.killed {
                break;
            }
            let Some(&op) = script.get(self.cursor) else {
                break;
            };
            if op != SPRITE_SCRIPT_OP || self.cursor + 3 > script.len() {
                break;
            }
            match script[self.cursor + 2] {
                0 => {
                    self.killed = true;
                    break;
                }
                1 => {
                    let f = rs16(script, self.cursor + 3) as u16;
                    if !flag_test(f) {
                        break;
                    }
                    self.cursor += 5;
                }
                2 => {
                    let f = rs16(script, self.cursor + 3) as u16;
                    if flag_test(f) {
                        break;
                    }
                    self.cursor += 5;
                }
                3 => {
                    if self.t == 0 {
                        self.start_x = self.x;
                        self.start_y = self.y;
                        self.start_rgb = self.rgb;
                    }
                    self.t = self.t.wrapping_add(frame_delta as i16);
                    let tx = rs16(script, self.cursor + 3);
                    let ty = rs16(script, self.cursor + 5);
                    let trgb = ru24(script, self.cursor + 7);
                    let dur = rs16(script, self.cursor + 0xb);
                    if dur < self.t {
                        self.t = dur;
                    }
                    if self.t == dur {
                        self.t = 0;
                        self.x = tx;
                        self.y = ty;
                        self.rgb = trgb;
                        self.cursor += 0xd;
                        break;
                    }
                    let mode = InterpMode::from_byte(script[self.cursor + 0xa]);
                    let (t, d) = (self.t as i32, dur as i32);
                    self.x = interp(tx as i32, self.start_x as i32, t, d, mode) as i16;
                    self.y = interp(ty as i32, self.start_y as i32, t, d, mode) as i16;
                    self.rgb = interp_rgb(trgb, self.start_rgb, t, d, mode);
                    break;
                }
                4 => {
                    if self.t == 0 {
                        self.start_rgb = self.rgb;
                    }
                    self.t = self.t.wrapping_add(frame_delta as i16);
                    let trgb = ru24(script, self.cursor + 3);
                    let dur = rs16(script, self.cursor + 7);
                    if dur < self.t {
                        self.t = dur;
                    }
                    if self.t == dur {
                        self.t = 0;
                        self.rgb = trgb;
                        self.cursor += 9;
                        break;
                    }
                    let mode = InterpMode::from_byte(script[self.cursor + 6]);
                    let (t, d) = (self.t as i32, dur as i32);
                    self.rgb = interp_rgb(trgb, self.start_rgb, t, d, mode);
                    break;
                }
                _ => break,
            }
        }
        if self.killed {
            return None;
        }
        Some(SpriteDraw {
            x: self.x,
            y: self.y,
            w: self.rec.w,
            h: self.rec.h,
            u: self.rec.u,
            v: self.rec.v,
            clut: self.rec.clut,
            texpage: self.rec.texpage,
            rgb: self.rgb,
        })
    }
}

// ---------------------------------------------------------------------------
// Kind 2 - image panel widget
// ---------------------------------------------------------------------------

/// One textured panel quad: GP0 `0x2C` (textured blended quad, colour
/// `0x888888`) over a **15bpp direct-colour** texpage (the spawn ORs `0x100`
/// into the page selector - depth bits = 15bpp, so no CLUT). OT slot `+0x10`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelQuad {
    pub left: i16,
    pub top: i16,
    pub right: i16,
    pub bottom: i16,
    pub u0: u8,
    pub v0: u8,
    pub u1: u8,
    pub v1: u8,
    pub texpage: u16,
}

/// The image-panel widget (retail handler `FUN_801F849C`): a full-screen-art
/// panel (e.g. an event cut-in image) that can move and scale. Five channels
/// tween together: x, y, width, height, and the first-page width (a panel
/// wider than 256px splits across two 15bpp texpages).
#[derive(Debug, Clone, Copy)]
pub struct PanelWidget {
    /// Current `[x, y, w, h, w_page0]` - actor `+0x14/16/18/1a` and `+0x24`.
    pub cur: [i16; 5],
    /// Targets - actor `+0x3c/3e/40/42` and `+0x26`.
    pub target: [i16; 5],
    /// Unscaled base `[w, h, w_page0]` - actor `+0xb8/ba/bc` (the `FUN_801F8E6C`
    /// scale reference).
    pub base: [i16; 3],
    /// Tween clock / duration - actor `+0x9c` / `+0x9e`.
    pub t: i16,
    pub dur: i16,
    /// Spawn-time pixel size - actor `+0xaa/+0xac` (drives the UV extents).
    pub w0: i16,
    pub h0: i16,
    /// Texel origin - actor `+0xa4` (`u`) / `+0xa8` (`v`).
    pub u: u8,
    pub v: u8,
    /// First / second 15bpp texpage - actor `+0xa0` / `+0xa2` (0 = no second
    /// page; set when the panel is wider than 256px).
    pub texpage: u16,
    pub texpage2: u16,
}

impl PanelWidget {
    /// Spawn from the on-script record `[x:i16@1][y:i16@3][w:i16@5][h:i16@7]
    /// [tex_x:i16@9][tex_y:i16@0xb]` (offsets are relative to the field-VM
    /// operand byte, matching the retail reader which is handed `operand`
    /// and reads from `+1`).
    // PORT: FUN_801F88FC
    pub fn spawn(rec: &[u8]) -> Option<Self> {
        if rec.len() < 0xd {
            return None;
        }
        let x = rs16(rec, 1);
        let y = rs16(rec, 3);
        let w = rs16(rec, 5);
        let h = rs16(rec, 7);
        let tex_x = rs16(rec, 9);
        let tex_y = rs16(rec, 0xb) as u16;
        let u = ((tex_x as i32 & 0x3f) << 2) as u8;
        let page_hi = ((tex_y & 0xff80) >> 4) as i16;
        let texpage = ((tex_x >> 6) + page_hi) as u16 | 0x100;
        let v = (tex_y & 0x7f) as u8;
        let mut w_page0 = w;
        let mut texpage2 = 0u16;
        if w as u16 > 0x100 {
            texpage2 = (((tex_x as i32 + 0x100) >> 6) as i16 + page_hi) as u16 | 0x100;
            w_page0 = 0x100;
        }
        Some(PanelWidget {
            cur: [x, y, w, h, w_page0],
            target: [x, y, w, h, w_page0],
            base: [w, h, w_page0],
            t: 0,
            dur: 0,
            w0: w,
            h0: h,
            u,
            v,
            texpage,
            texpage2,
        })
    }

    /// Start a move/scale tween - the `FUN_801F8E6C` control API. `scale` is
    /// 4.12 fixed point (`0x1000` = 1.0): the width / height / first-page
    /// width targets become `(base * scale) >> 12`.
    // PORT: FUN_801F8E6C
    pub fn move_scale(&mut self, x: i16, y: i16, scale: i32, dur: i16) {
        self.target[0] = x;
        self.target[1] = y;
        self.target[2] = ((self.base[0] as i32 * scale) >> 12) as i16;
        self.target[3] = ((self.base[1] as i32 * scale) >> 12) as i16;
        self.target[4] = ((self.base[2] as i32 * scale) >> 12) as i16;
        self.t = 0;
        self.dur = dur;
    }

    /// Advance the 5-channel tween one frame and return the 1–2 textured
    /// quads - the whole of `FUN_801F849C`. Unlike the mask handler this one
    /// has no `dur == 0` snap arm: with no active tween the latched `cur`
    /// values draw as-is.
    // PORT: FUN_801F849C
    pub fn tick(&mut self, frame_delta: u8, screen: &Screen) -> (PanelQuad, Option<PanelQuad>) {
        let mut e = self.cur;
        if self.dur != 0 {
            self.t = self.t.wrapping_add(frame_delta as i16);
            if self.dur < self.t {
                self.t = self.dur;
            }
            for (i, ch) in e.iter_mut().enumerate() {
                if self.target[i] != self.cur[i] {
                    *ch = interp(
                        self.target[i] as i32,
                        self.cur[i] as i32,
                        self.t as i32,
                        self.dur as i32,
                        InterpMode::Linear,
                    ) as i16;
                }
            }
            if self.t == self.dur {
                self.dur = 0;
                self.cur = self.target;
                e = self.target;
            }
        }
        let [x, y, _w, h, w0] = e;
        let y = y - screen.y_off;
        // Retail truncates the spawn width to a byte for the UV extent and
        // zeroes it when the panel needs a second page.
        let cw: u8 = if (self.w0 as u16) < 0x101 {
            self.w0 as u8
        } else {
            0
        };
        let q0 = PanelQuad {
            left: x,
            top: y,
            right: x + w0 - 1,
            bottom: y + h - 1,
            u0: self.u,
            v0: self.v,
            u1: self.u.wrapping_add(cw).wrapping_sub(1),
            v1: self.v.wrapping_add(self.h0 as u8).wrapping_sub(1),
            texpage: self.texpage,
        };
        let q1 = (self.texpage2 != 0).then(|| PanelQuad {
            left: x + w0 - 2,
            top: y,
            right: x + e[2],
            bottom: y + h - 1,
            u0: self.u.wrapping_add(cw).wrapping_add(0xe),
            v0: self.v,
            u1: self.u.wrapping_add(self.w0 as u8).wrapping_add(0x10),
            v1: self.v.wrapping_add(self.h0 as u8).wrapping_sub(1),
            texpage: self.texpage2,
        });
        (q0, q1)
    }
}

// ---------------------------------------------------------------------------
// Kind 3 - letterbox-band widget
// ---------------------------------------------------------------------------

/// The letterbox widget (retail handler `FUN_801F8A34`): two solid black
/// bands with gradient "feather" strips on their inner edges. No tween - the
/// `FUN_801F8F28` config writes the band edges directly.
///
/// Draw shape (all between `x_left..x_right`, OT slot `+0x4`):
/// * solid black `(x_left, -y_off) .. (x_right, y0)` and `(x_left, y3) ..
///   (x_right, height)` (GP0 `0x28`);
/// * gradient `y0` (white) → `y1` (black) and `y2` (black) → `y3` (white)
///   (GP0 `0x3B` shaded semi-transparent quads, preceded by a draw-mode
///   packet selecting **subtractive** blending - `FUN_80059010(.., 0x55, ..)`,
///   texpage blend bits mode 2), so the white edge subtracts to black.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Letterbox {
    /// Horizontal extent - actor `+0xa0` / `+0xa2`.
    pub x_left: i16,
    pub x_right: i16,
    /// Band edges top..bottom - actor `+0xa4/a6/a8/aa`.
    pub y0: i16,
    pub y1: i16,
    pub y2: i16,
    pub y3: i16,
}

impl Letterbox {
    /// Decode the 6-`i16` config block the field VM hands `FUN_801F8F28`:
    /// `[x_left][x_right][y0][y1][y2][y3]` (the spawn writes `y3` into both
    /// `+0xa8`'s pair slots).
    // PORT: FUN_801F8F28
    pub fn from_config(block: &[u8]) -> Option<Self> {
        if block.len() < 12 {
            return None;
        }
        Some(Letterbox {
            x_left: rs16(block, 0),
            x_right: rs16(block, 2),
            y0: rs16(block, 4),
            y1: rs16(block, 6),
            y2: rs16(block, 8),
            y3: rs16(block, 10),
        })
    }

    /// The two solid bands: `(left, top, right, bottom)` rects. Port of the
    /// `FUN_801F8A34` flat-quad pair (top band starts at `-y_off`; bottom band
    /// ends at `height`, **not** `height - 1` - retail's literal).
    // PORT: FUN_801F8A34
    pub fn solid_bands(&self, screen: &Screen) -> [MaskQuad; 2] {
        [
            MaskQuad {
                left: self.x_left,
                top: -screen.y_off,
                right: self.x_right,
                bottom: self.y0,
            },
            MaskQuad {
                left: self.x_left,
                top: self.y3,
                right: self.x_right,
                bottom: screen.height,
            },
        ]
    }

    /// The two gradient feather strips as `(rect, top_is_white)`: the first
    /// fades white at `y0` → black at `y1`, the second black at `y2` → white
    /// at `y3`. Drawn subtractively, so the white edge darkens fully.
    pub fn gradient_bands(&self) -> [(MaskQuad, bool); 2] {
        [
            (
                MaskQuad {
                    left: self.x_left,
                    top: self.y0,
                    right: self.x_right,
                    bottom: self.y1,
                },
                true,
            ),
            (
                MaskQuad {
                    left: self.x_left,
                    top: self.y2,
                    right: self.x_right,
                    bottom: self.y3,
                },
                false,
            ),
        ]
    }
}

// ---------------------------------------------------------------------------
// Aggregate widget host (the engine face of the PROT-0900 actor pool)
// ---------------------------------------------------------------------------

/// One frame of widget output, ready for a renderer: solid border/band
/// quads (drawn flat black), the letterbox feather strips, and the
/// textured sprite / panel draws (sampled from PSX VRAM by clut/texpage).
#[derive(Debug, Default, Clone)]
pub struct ScreenFxFrame {
    /// Flat black quads: the mask widget's four border quads plus the
    /// letterbox's two solid bands.
    pub solid_quads: Vec<MaskQuad>,
    /// Letterbox feather strips as `(rect, top_is_white)` (see
    /// [`Letterbox::gradient_bands`]).
    pub gradient_quads: Vec<(MaskQuad, bool)>,
    /// Live sprite-widget draws.
    pub sprites: Vec<SpriteDraw>,
    /// Image-panel quads (1 or 2 per panel - the >256px split).
    pub panels: Vec<PanelQuad>,
}

impl ScreenFxFrame {
    /// `true` when the frame draws nothing.
    pub fn is_empty(&self) -> bool {
        self.solid_quads.is_empty()
            && self.gradient_quads.is_empty()
            && self.sprites.is_empty()
            && self.panels.is_empty()
    }
}

/// Runtime host for the four widget kinds - the engine face of retail's
/// PROT-0900 actor pool, driven by the field-VM op `0x43` sub-ops
/// `0x10`/`0x11`/`0x13`/`0x14`/`0x15` (the ending-scene widget family).
/// The field host routes each sub-op here; [`Self::tick`] advances every
/// live widget one frame and returns the draw list.
#[derive(Debug, Default, Clone)]
pub struct ScreenFxHost {
    /// Screen geometry the widgets draw against.
    pub screen: Screen,
    /// The screen-mask (iris) widget; spawned on the first sub-0x11.
    pub mask: Option<MaskWidget>,
    /// Live scripted sprite widgets, each with its spawn-record bytes
    /// (the widget script cursor indexes into them).
    pub sprites: Vec<(SpriteWidget, Vec<u8>)>,
    /// The image-panel widget (sub-0x13 spawn / sub-0x14 move-scale).
    pub panel: Option<PanelWidget>,
    /// The letterbox widget (sub-0x15 config).
    pub letterbox: Option<Letterbox>,
}

impl ScreenFxHost {
    /// `true` when any widget is live (a renderer can skip the pass
    /// entirely otherwise).
    pub fn is_active(&self) -> bool {
        self.mask.is_some()
            || self.panel.is_some()
            || self.letterbox.is_some()
            || !self.sprites.is_empty()
    }

    /// Drop every widget (scene teardown).
    pub fn clear(&mut self) {
        self.mask = None;
        self.sprites.clear();
        self.panel = None;
        self.letterbox = None;
    }

    /// Op `0x43` sub-0x10: spawn a scripted sprite widget from its inline
    /// 19-byte record (`FUN_801F8004`). The record bytes are kept so the
    /// widget script (cursor past the header) can run; the VM's fixed
    /// 19-byte slice carries no trailing script, which parks the widget
    /// on its static draw - the faithful outcome for a bare record.
    pub fn sprite_spawn(&mut self, payload: &[u8]) {
        if let Some(rec) = SpriteRecord::parse(payload) {
            self.sprites
                .push((SpriteWidget::spawn(rec), payload.to_vec()));
        }
    }

    /// Op `0x43` sub-0x11: mask rect tween (`FUN_801F8D4C`). Spawns the
    /// mask fully open on first use; `-1` edges take their full-open
    /// defaults.
    pub fn mask_rect(&mut self, words: [u16; 5]) {
        let screen = self.screen;
        let m = self.mask.get_or_insert_with(|| MaskWidget::spawn(&screen));
        m.set_rect(
            words[0] as i16,
            words[1] as i16,
            words[2] as i16,
            words[3] as i16,
            words[4] as i16,
            &screen,
        );
    }

    /// Op `0x43` sub-0x13: image-panel spawn (`FUN_801F88FC`); `payload`
    /// starts at the sub-op byte, matching the retail reader.
    pub fn panel_spawn(&mut self, payload: &[u8]) {
        self.panel = PanelWidget::spawn(payload);
    }

    /// Op `0x43` sub-0x14: panel move/scale (`FUN_801F8E6C`); no-op
    /// without a spawned panel, like retail's empty actor slot.
    pub fn panel_move(&mut self, words: [i16; 4]) {
        if let Some(p) = &mut self.panel {
            p.move_scale(words[0], words[1], words[2] as i32 & 0xFFFF, words[3]);
        }
    }

    /// Op `0x43` sub-0x15: letterbox config (`FUN_801F8F28`).
    pub fn letterbox_config(&mut self, payload: &[u8]) {
        self.letterbox = Letterbox::from_config(payload);
    }

    /// Advance every live widget one frame and collect the draw list.
    /// `frame_delta` is retail's per-frame byte (`DAT_1F800393`);
    /// `flag_test` is the story-flag probe the sprite scripts wait on
    /// (`FUN_8003CE64`, the `0x80085758` bank).
    pub fn tick(
        &mut self,
        frame_delta: u8,
        mut flag_test: impl FnMut(u16) -> bool,
    ) -> ScreenFxFrame {
        let mut frame = ScreenFxFrame::default();
        let screen = self.screen;
        if let Some(m) = &mut self.mask {
            frame.solid_quads.extend(m.tick(frame_delta, &screen));
        }
        for (w, rec) in &mut self.sprites {
            if let Some(draw) = w.tick(rec, frame_delta, &mut flag_test) {
                frame.sprites.push(draw);
            }
        }
        // Killed sprites never draw again; free their slots.
        self.sprites.retain(|(w, _)| !w.killed);
        if let Some(p) = &mut self.panel {
            let (q0, q1) = p.tick(frame_delta, &screen);
            frame.panels.push(q0);
            if let Some(q1) = q1 {
                frame.panels.push(q1);
            }
        }
        if let Some(lb) = &self.letterbox {
            frame.solid_quads.extend(lb.solid_bands(&screen));
            frame.gradient_quads.extend(lb.gradient_bands());
        }
        frame
    }
}

// ---------------------------------------------------------------------------
// PROT-0900 layout constants (for disc-gated validation + hosting)
// ---------------------------------------------------------------------------

/// Extraction PROT entry of the screen-effect / top-view render overlay.
pub const SCREEN_FX_OVERLAY_PROT: usize = 900;

/// Slot-B link base the overlay loads at.
pub const SCREEN_FX_LINK_BASE: u32 = 0x801F_69D8;

/// Per-frame handler entry points, in handler-table order
/// (sprite, mask, panel, letterbox).
pub const SCREEN_FX_HANDLER_VAS: [u32; 4] = [0x801F_7A9C, 0x801F_811C, 0x801F_849C, 0x801F_8A34];

/// The four 0x18-byte handler-binding descriptors (`FUN_80020DE0` input),
/// in the same order as [`SCREEN_FX_HANDLER_VAS`].
pub const SCREEN_FX_DESCRIPTOR_VAS: [u32; 4] = [0x801F_8FE4, 0x801F_8FFC, 0x801F_9014, 0x801F_902C];

/// The 5-entry sprite-script sub-op dispatch table at the overlay head
/// (`jr *(0x801F69D8 + sub*4)`): kill, wait-set, wait-clear, tween-pos+colour,
/// tween-colour.
pub const SPRITE_SUBOP_ENTRY_VAS: [u32; 5] = [
    0x801F_7B14,
    0x801F_7B28,
    0x801F_7B54,
    0x801F_7B8C,
    0x801F_7D90,
];

#[cfg(test)]
mod tests {
    use super::*;

    // -- interp (FUN_801DE4C8) ---------------------------------------------

    #[test]
    fn interp_linear_matches_mode1() {
        assert_eq!(interp(100, 0, 0, 10, InterpMode::Linear), 0);
        assert_eq!(interp(100, 0, 5, 10, InterpMode::Linear), 50);
        assert_eq!(interp(100, 0, 10, 10, InterpMode::Linear), 100);
        assert_eq!(
            interp(100, 0, 11, 10, InterpMode::Linear),
            100,
            "t>D clamps"
        );
        assert_eq!(interp(50, 50, 3, 10, InterpMode::Linear), 50, "a==b early");
        // Truncation toward zero both directions.
        assert_eq!(interp(10, 0, 3, 10, InterpMode::Linear), 3);
        assert_eq!(interp(0, 10, 3, 10, InterpMode::Linear), 7);
    }

    #[test]
    fn interp_ease_out_mode2() {
        // e = (a-b)*t; result = (e + (e/D)*(D-t))/D + b.
        // a=100,b=0,t=5,D=10: e=500, e/D=50, +50*5=750, /10=75.
        assert_eq!(interp(100, 0, 5, 10, InterpMode::EaseOut), 75);
        // Starts fast: t=1 -> e=100, e/D=10, +10*9=190, /10=19.
        assert_eq!(interp(100, 0, 1, 10, InterpMode::EaseOut), 19);
        assert_eq!(interp(100, 0, 10, 10, InterpMode::EaseOut), 100);
    }

    #[test]
    fn interp_ease_in_mode3() {
        // ((a-b)*t/D)*t/D + b: t=5,D=10 -> (100*5/10)*5/10 = 25.
        assert_eq!(interp(100, 0, 5, 10, InterpMode::EaseIn), 25);
        assert_eq!(interp(100, 0, 1, 10, InterpMode::EaseIn), 1);
        assert_eq!(interp(100, 0, 10, 10, InterpMode::EaseIn), 100);
    }

    #[test]
    fn interp_ease_in_out_mode4() {
        // First half = ease-in over D/2 covering half the span:
        // t=2,D=20 (half=10, span=50): (50*2/10)*2/10 = 2.
        assert_eq!(interp(100, 0, 2, 20, InterpMode::EaseInOut), 2);
        // Exactly half: (50*10/10)*10/10 = 50.
        assert_eq!(interp(100, 0, 10, 20, InterpMode::EaseInOut), 50);
        // Second half = ease-out from the midpoint: t=15 -> mode-2 with
        // a=100,b=50,t=5,D=10: e=250, e/D=25, +25*5=375, /10=37 -> 87.
        assert_eq!(interp(100, 0, 15, 20, InterpMode::EaseInOut), 87);
        assert_eq!(interp(100, 0, 20, 20, InterpMode::EaseInOut), 100);
    }

    // -- mask widget (FUN_801F811C / FUN_801F8D4C) -------------------------

    fn screen() -> Screen {
        Screen {
            x0: 0,
            y_off: 0,
            height: 240,
        }
    }

    #[test]
    fn mask_spawns_fully_open() {
        let s = screen();
        let mut m = MaskWidget::spawn(&s);
        assert_eq!(m.cur, [0, 0, 0x140, 239]);
        let q = m.tick(10, &s);
        // All four border quads are degenerate or inverted (cover nothing).
        assert_eq!(q[0].bottom, 0, "top band has zero height");
        assert_eq!(q[1].top, 239, "bottom band starts at the last row");
        assert_eq!(q[2].right, -1, "left band right edge = L-1 < L");
        assert_eq!(q[3].left, 0x140, "right band starts at the right edge");
    }

    #[test]
    fn mask_set_rect_substitutes_defaults_for_minus_one() {
        let s = Screen {
            x0: 8,
            y_off: 0,
            height: 224,
        };
        let mut m = MaskWidget::spawn(&s);
        m.set_rect(-1, -1, -1, -1, 16, &s);
        assert_eq!(m.target, [8, 0, 0x140, 223]);
        m.set_rect(100, 50, 220, 190, 16, &s);
        assert_eq!(m.target, [100, 50, 220, 190]);
        assert_eq!((m.t, m.dur), (0, 16));
    }

    #[test]
    fn mask_tick_reinterpolates_from_fixed_start_and_latches() {
        let s = screen();
        let mut m = MaskWidget::spawn(&s);
        m.set_rect(100, 60, 240, 180, 40, &s);

        // Retail re-lerps from the latched start each frame: at t=10 of 40,
        // top = 0 + (60-0)*10/40 = 15 (NOT an iterative step).
        let q = m.tick(10, &s);
        assert_eq!(q[0].bottom, 15, "top edge at t=10");
        assert_eq!(m.cur[1], 0, "cur does not move mid-tween");

        let q = m.tick(10, &s);
        assert_eq!(q[0].bottom, 30, "top edge at t=20 from the same start");
        let q = m.tick(10, &s);
        assert_eq!(q[0].bottom, 45);

        // t reaches dur: latch, dur clears, edges exact.
        let q = m.tick(10, &s);
        assert_eq!(m.cur, [100, 60, 240, 180]);
        assert_eq!(m.dur, 0);
        assert_eq!(
            q,
            [
                MaskQuad {
                    left: 0,
                    top: 0,
                    right: 0x140,
                    bottom: 60
                },
                MaskQuad {
                    left: 0,
                    top: 180,
                    right: 0x140,
                    bottom: 239
                },
                MaskQuad {
                    left: 0,
                    top: 60,
                    right: 99,
                    bottom: 180
                },
                MaskQuad {
                    left: 240,
                    top: 60,
                    right: 0x140,
                    bottom: 180
                },
            ]
        );

        // Post-latch frames take the dur==0 snap arm and hold.
        let q2 = m.tick(10, &s);
        assert_eq!(q2, q);
    }

    #[test]
    fn mask_quads_frame_the_visible_rect() {
        let s = screen();
        let mut m = MaskWidget::spawn(&s);
        m.set_rect(80, 40, 260, 200, 0, &s);
        // dur == 0 -> snap arm.
        let q = m.tick(1, &s);
        // The union of the four bands covers everything outside [80..260]x[40..200].
        assert_eq!(
            q[0],
            MaskQuad {
                left: 0,
                top: 0,
                right: 0x140,
                bottom: 40
            }
        );
        assert_eq!(
            q[1],
            MaskQuad {
                left: 0,
                top: 200,
                right: 0x140,
                bottom: 239
            }
        );
        assert_eq!(
            q[2],
            MaskQuad {
                left: 0,
                top: 40,
                right: 79,
                bottom: 200
            }
        );
        assert_eq!(
            q[3],
            MaskQuad {
                left: 260,
                top: 40,
                right: 0x140,
                bottom: 200
            }
        );
    }

    // -- sprite widget (FUN_801F8004 / FUN_801F7A9C) -----------------------

    /// Build a sprite spawn record: header + script bytes.
    fn sprite_record(script: &[u8]) -> Vec<u8> {
        let mut r = Vec::new();
        for v in [50i16, 60, 32, 16] {
            r.extend_from_slice(&v.to_le_bytes()); // x y w h
        }
        for v in [0x143i16, 0x1A5, 0x320, 0x1E2] {
            r.extend_from_slice(&v.to_le_bytes()); // tex_x tex_y clut_x clut_y
        }
        r.extend_from_slice(&[0x80, 0x40, 0x20]); // rgb
        assert_eq!(r.len(), SPRITE_SCRIPT_OFF);
        r.extend_from_slice(script);
        r
    }

    #[test]
    fn sprite_record_field_derivation() {
        let rec = sprite_record(&[]);
        let p = SpriteRecord::parse(&rec).unwrap();
        assert_eq!((p.x, p.y, p.w, p.h), (50, 60, 32, 16));
        // texpage = (0x143 >> 6) + ((0x1A5 & !0xff) >> 4) = 5 + 0x10 = 0x15.
        assert_eq!(p.texpage, 0x15);
        // u = (0x143 & 0x3f) << 2 = 3 << 2 = 12 ; v = 0x1A5 & 0xff = 0xA5.
        assert_eq!((p.u, p.v), (12, 0xA5));
        // clut = (0x1E2 << 6) + (0x320 >> 4) = 0x7880 + 0x32 = 0x78B2.
        assert_eq!(p.clut as u16, 0x78B2);
        assert_eq!(p.rgb, [0x80, 0x40, 0x20]);
    }

    #[test]
    fn sprite_subop_0_kills_the_draw() {
        let rec = sprite_record(&[0x40, 0, 0]);
        let mut w = SpriteWidget::spawn(SpriteRecord::parse(&rec).unwrap());
        assert!(w.tick(&rec, 16, |_| false).is_none());
        assert!(w.killed);
        assert!(w.tick(&rec, 16, |_| false).is_none(), "stays dead");
    }

    #[test]
    fn sprite_subops_1_2_gate_on_story_flags() {
        // wait-until-set(flag 7) ; kill.
        let rec = sprite_record(&[0x40, 0, 1, 7, 0, 0x40, 0, 0]);
        let mut w = SpriteWidget::spawn(SpriteRecord::parse(&rec).unwrap());
        // Flag clear: parked, still draws.
        let d = w.tick(&rec, 16, |_| false).unwrap();
        assert_eq!((d.x, d.y), (50, 60));
        assert_eq!(w.cursor, SPRITE_SCRIPT_OFF, "cursor parked on the wait");
        // Flag set: advances AND interprets the next op (kill) the same frame.
        assert!(w.tick(&rec, 16, |f| f == 7).is_none());

        // wait-until-clear: inverted gate.
        let rec2 = sprite_record(&[0x40, 0, 2, 7, 0, 0x40, 0, 0]);
        let mut w2 = SpriteWidget::spawn(SpriteRecord::parse(&rec2).unwrap());
        assert!(w2.tick(&rec2, 16, |_| true).is_some(), "set -> parked");
        assert!(w2.tick(&rec2, 16, |_| false).is_none(), "clear -> advance");
    }

    #[test]
    fn sprite_subop_3_tweens_position_and_colour_from_captured_start() {
        // Tween to (90, 100), colour (0,0,0), linear, over 40.
        let mut script = vec![0x40, 0, 3];
        script.extend_from_slice(&90i16.to_le_bytes());
        script.extend_from_slice(&100i16.to_le_bytes());
        script.extend_from_slice(&[0, 0, 0]); // target rgb
        script.push(1); // mode = linear
        script.extend_from_slice(&40i16.to_le_bytes());
        let rec = sprite_record(&script);
        let mut w = SpriteWidget::spawn(SpriteRecord::parse(&rec).unwrap());

        let d = w.tick(&rec, 10, |_| false).unwrap();
        // x: 50 + (90-50)*10/40 = 60 ; y: 60 + (100-60)*10/40 = 70.
        assert_eq!((d.x, d.y), (60, 70));
        // r: 0x80 + (0-0x80)*10/40 = 0x60.
        assert_eq!(d.rgb, [0x60, 0x30, 0x18]);

        let d = w.tick(&rec, 10, |_| false).unwrap();
        assert_eq!((d.x, d.y), (70, 80), "re-lerped from the captured start");

        w.tick(&rec, 10, |_| false);
        let d = w.tick(&rec, 10, |_| false).unwrap();
        assert_eq!((d.x, d.y), (90, 100), "exact landing");
        assert_eq!(d.rgb, [0, 0, 0]);
        assert_eq!(w.t, 0, "clock reset for the next op");
        assert_eq!(w.cursor, SPRITE_SCRIPT_OFF + 0xd, "cursor advanced");
    }

    #[test]
    fn sprite_subop_4_tweens_colour_only() {
        let mut script = vec![0x40, 0, 4];
        script.extend_from_slice(&[0xFF, 0xFF, 0xFF]); // target rgb
        script.push(1); // mode
        script.extend_from_slice(&16i16.to_le_bytes());
        let rec = sprite_record(&script);
        let mut w = SpriteWidget::spawn(SpriteRecord::parse(&rec).unwrap());
        let d = w.tick(&rec, 8, |_| false).unwrap();
        assert_eq!((d.x, d.y), (50, 60), "position untouched");
        // r: 0x80 + (0xFF-0x80)*8/16 = 0x80 + 0x3F = 0xBF.
        assert_eq!(d.rgb[0], 0xBF);
        let d = w.tick(&rec, 8, |_| false).unwrap();
        assert_eq!(d.rgb, [0xFF, 0xFF, 0xFF]);
        assert_eq!(w.cursor, SPRITE_SCRIPT_OFF + 9);
    }

    // -- panel widget (FUN_801F88FC / FUN_801F8E6C / FUN_801F849C) ---------

    /// A panel spawn record (field-VM operand shape: data from byte 1).
    fn panel_record(w: i16) -> Vec<u8> {
        let mut r = vec![0u8]; // operand byte 0 (the opcode-adjacent byte)
        for v in [40i16, 30, w, 100, 0x140, 0x100] {
            r.extend_from_slice(&v.to_le_bytes());
        }
        r
    }

    #[test]
    fn panel_spawn_single_page() {
        let p = PanelWidget::spawn(&panel_record(0x100)).unwrap();
        assert_eq!(p.cur, [40, 30, 0x100, 100, 0x100]);
        // u = (0x140 & 0x3f) << 2 = 0 ; page = (0x140>>6) + ((0x100 & 0xff80)>>4) | 0x100.
        assert_eq!(p.u, 0);
        assert_eq!(p.texpage, (5 + 0x10) | 0x100);
        assert_eq!(p.texpage2, 0, "<=256 wide: one page");
        assert_eq!(p.v, 0, "v = tex_y & 0x7f");
    }

    #[test]
    fn panel_spawn_wide_splits_two_pages() {
        let p = PanelWidget::spawn(&panel_record(0x140)).unwrap();
        assert_eq!(p.cur[4], 0x100, "first-page width clamps to 256");
        assert_eq!(p.base, [0x140, 100, 0x100]);
        // texpage2 = ((0x140 + 0x100) >> 6) + 0x10 | 0x100 = (9 + 0x10) | 0x100.
        assert_eq!(p.texpage2, (9 + 0x10) | 0x100);
        let s = screen();
        let mut p2 = p;
        let (q0, q1) = p2.tick(1, &s);
        assert_eq!((q0.left, q0.right), (40, 40 + 0x100 - 1));
        let q1 = q1.expect("second page quad");
        assert_eq!((q1.left, q1.right), (40 + 0x100 - 2, 40 + 0x140));
        assert_eq!(q1.texpage, (9 + 0x10) | 0x100);
    }

    #[test]
    fn panel_move_scale_tweens_five_channels() {
        let mut p = PanelWidget::spawn(&panel_record(0x100)).unwrap();
        // Scale to half size while moving to (100, 80) over 20 frames.
        p.move_scale(100, 80, 0x800, 20);
        assert_eq!(p.target, [100, 80, 0x80, 50, 0x80]);
        let s = screen();
        let (q0, _) = p.tick(10, &s);
        // x: 40 + (100-40)*10/20 = 70 ; w0 channel: 0x100 + (0x80-0x100)*10/20 = 0xC0.
        assert_eq!(q0.left, 70);
        assert_eq!(q0.right, 70 + 0xC0 - 1);
        let _ = p.tick(10, &s);
        assert_eq!(p.cur, [100, 80, 0x80, 50, 0x80], "latched");
        assert_eq!(p.dur, 0);
    }

    // -- letterbox widget (FUN_801F8F28 / FUN_801F8A34) --------------------

    #[test]
    fn letterbox_bands_and_feathers() {
        let mut block = Vec::new();
        for v in [0i16, 0x140, 40, 56, 184, 200] {
            block.extend_from_slice(&v.to_le_bytes());
        }
        let lb = Letterbox::from_config(&block).unwrap();
        let s = screen();
        let solid = lb.solid_bands(&s);
        assert_eq!(
            solid[0],
            MaskQuad {
                left: 0,
                top: 0,
                right: 0x140,
                bottom: 40
            }
        );
        assert_eq!(
            solid[1],
            MaskQuad {
                left: 0,
                top: 200,
                right: 0x140,
                bottom: 240
            },
            "bottom band ends at height (retail literal, not height-1)"
        );
        let grad = lb.gradient_bands();
        assert_eq!((grad[0].0.top, grad[0].0.bottom, grad[0].1), (40, 56, true));
        assert_eq!(
            (grad[1].0.top, grad[1].0.bottom, grad[1].1),
            (184, 200, false)
        );
    }
}
