//! The sprite-actor draw callback.

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
/// shifted down by four, then clamped to `0..=0xFF`.
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
    // `lhu` reads the level unsigned, so the `+0xF` of the round-toward-zero
    // idiom (`bgez a3` at `0x801D6818`) never runs: a plain `>> 4`.
    let level = (level >> 4).clamp(0, 0xFF);

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
