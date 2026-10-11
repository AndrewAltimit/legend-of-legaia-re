//! The sprite widget: its record, script and per-frame draw.

use super::*;

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
