//! The sprite-actor draw callback, hold flags and animation bind.

use super::*;

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
