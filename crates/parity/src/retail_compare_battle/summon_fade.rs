//! The summon's screen fade as retail holds it in RAM: the fade actor found
//! in the actor pool and the template step it is on.

use super::*;

/// The summon band's live full-screen flash in a capture: which of the two
/// templates it is and how many vsyncs it has run. Read off the SCUS fade
/// actor's `+0x7C` block ([`legaia_engine_system::fade_ramp`]), whose
/// countdowns give the age back exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetailFade {
    /// `true` for the `0x33` flash-in (black to white), `false` for the
    /// `0x34` flash-out.
    pub to_white: bool,
    /// Vsyncs since the spawn, the start delay included.
    pub age: u16,
}

/// The SCUS fade actor's tick (`FUN_80025000`), stored at actor `+0x0C`.
pub(super) const FADE_ACTOR_TICK: u32 = 0x8002_5000;
/// The actor-list "done" bit (`actor[+0x10] |= 8`) a killed fade carries.
pub(super) const ACTOR_DONE: u32 = 0x8;
/// Where the effect-actor pool lives (`FUN_80020DE0`'s allocations).
pub(super) const ACTOR_POOL: std::ops::Range<u32> = 0x8007_0000..0x800A_0000;

/// The block's per-frame red delta for a template (`FUN_80020B00`).
pub(super) fn template_delta(
    t: &legaia_engine_battle_vm::battle_action::SummonFadeTemplate,
) -> i16 {
    (((i32::from(t.end_rgb[0]) - i32::from(t.start_rgb[0])) * 0x40) / i32::from(t.duration)) as i16
}

/// Find the live summon flash among the fade actors: tick word
/// `FUN_80025000`, not done, kind `1`, id `1`, and the delta of one of the
/// two summon templates (the creature's own fades share the actor and the
/// kind, never the delta and duration pair).
pub fn summon_fade(ram: &[u8]) -> Option<RetailFade> {
    use legaia_engine_battle_vm::battle_action::{
        SUMMON_FADE_ID, SUMMON_FLASH_IN, SUMMON_FLASH_OUT,
    };
    let s16 = |va: u32| game_anchors::u16_at(ram, va) as i16;
    let mut a = ACTOR_POOL.start;
    while a + 0xA0 < ACTOR_POOL.end {
        let base = a;
        a += 4;
        if game_anchors::u32_at(ram, base + 0x0C) != FADE_ACTOR_TICK
            || game_anchors::u32_at(ram, base + 0x10) & ACTOR_DONE != 0
        {
            continue;
        }
        let b = base + 0x7C;
        if s16(b + 0x18) != 1 || s16(b + 0x22) != SUMMON_FADE_ID {
            continue;
        }
        let (delta, delay, duration) = (s16(b + 0x10), s16(b + 0x1C), s16(b + 0x20));
        for (t, to_white) in [(&SUMMON_FLASH_IN, true), (&SUMMON_FLASH_OUT, false)] {
            if delta != template_delta(t) {
                continue;
            }
            // `FUN_80020C14` counts the delay down first; the frame it lands
            // also steps the duration, so a landed block's age is one short
            // of the two countdowns' sum.
            let age = if delay > 0 {
                i32::from(t.delay) - i32::from(delay)
            } else if t.delay > 0 {
                i32::from(t.delay) + i32::from(t.duration) - i32::from(duration) - 1
            } else {
                i32::from(t.duration) - i32::from(duration)
            };
            return Some(RetailFade {
                to_white,
                age: age.clamp(0, i32::from(u16::MAX)) as u16,
            });
        }
    }
    None
}
