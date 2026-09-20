//! The minigame **effect-part pool** - the sink the minigame overlays'
//! one-shot presentation spawns land in, shared by every host.
//!
//! Retail's minigame overlays spawn their one-shot parts through the same
//! shared part-spawn API (`FUN_80021B04` over the effect pool), age them on
//! the overlay's own tick and draw them through a sprite emit. The dance run
//! owns one of these pools inside [`crate::dance::DanceGame`] - its
//! sequence-clear banner and stars spawn into it from the judge, so they are
//! gameplay and belong to the session. The pools the *other* overlays fill -
//! the fishing venue's splash, its wander ripples and its catch bursts - have
//! no session to live inside, and that is what this module is.
//!
//! **Where it lives is the whole point.** Held by a host, a pool ages only on
//! that host, which is how the play window came to be the one surface on
//! which a fishing splash existed at all. Held on
//! [`crate::world::MinigameState`] it ages inside the world tick, and every
//! host that ticks the world drains the same parts through one draw builder.
//!
//! ## The coordinate space is the producer's, not this pool's
//!
//! A part's pair is **stage pixels** (the retail 320x240 framebuffer), already
//! through whatever shift its producer applies. The pool does not re-shift it,
//! and deliberately does not reuse the dance overlay's emit dispatch
//! ([`crate::dance::sprite_part_emit`], `FUN_801d387c`): that routine is the
//! *dance* overlay's, and running the fishing overlay's parts through it would
//! assert a shared draw dispatch the dump corpus does not show. The fade ramp
//! *is* shared, because it is the port's own decision either way
//! ([`crate::dance::PART_AGE_STEP`]) and one ramp beats two.
//!
//! The overlays' own sprite pages are not resident in engine VRAM, so a host
//! degrades each part to a placeholder cell keyed on its sprite id - the same
//! degradation the dance parts take.

use crate::dance::{PART_AGE_STEP, sprite_part_fade_weight};
use crate::minigame_actor::{BEAT_FADE_CEILING, FLAG_KILLED, MinigameActor, MinigameActorPool};

/// How many parts the pool holds before the oldest is dropped.
///
/// Retail's pool is a fixed array the spawn API walks for a free slot; the
/// port bounds it the same way round - a spawn into a full pool retires the
/// oldest part rather than failing silently.
pub const FX_POOL_CAPACITY: usize = 32;

/// One live part, resolved for a draw: a stage-pixel seat, the sprite id the
/// spawn stamped into `+0x50`, and this frame's fade weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FxPartFrame {
    /// Index in the pool, for a host that keys a cache on it.
    pub index: usize,
    /// Stage pixel (retail 320x240 framebuffer) position.
    pub x: i16,
    pub y: i16,
    /// The part's `+0x50` sprite id.
    pub sprite: u16,
    /// `0..=0xFF`, from the fade prologue's ramp.
    pub fade: u8,
}

/// The minigame effect-part pool: spawn, age, resolve.
#[derive(Default)]
pub struct MinigameFxPool {
    parts: MinigameActorPool,
}

impl MinigameFxPool {
    /// An empty pool.
    pub fn new() -> Self {
        Self {
            parts: MinigameActorPool::with_capacity(FX_POOL_CAPACITY),
        }
    }

    /// Spawn one part at a stage-pixel seat, in the field shape retail's part
    /// spawn stores: the pair into `+0x14` / `+0x16`, the sprite id into
    /// `+0x50`, the fixed `0x1000` scale, and the fade counter at the top of
    /// its ramp.
    // REF: FUN_801d3fd0 (the spawn's actor-field stores; the dance run's own
    //      pool carries the port tag for that routine)
    pub fn spawn(&mut self, x: i16, y: i16, sprite: u16) -> usize {
        if self.parts.len() >= FX_POOL_CAPACITY {
            // Oldest out. `retire_dead` is the pool's own compaction, so mark
            // rather than splice.
            if let Some(a) = self.parts.get_mut(0) {
                a.flags |= FLAG_KILLED;
            }
            self.parts.retire_dead();
        }
        let mut a = MinigameActor::at([x, y, 0], crate::dance::PART_DRAW_MODE);
        a.sprite = sprite;
        a.scale = crate::minigame_actor::SPAWN_SCALE;
        a.beat = BEAT_FADE_CEILING;
        self.parts.push(a)
    }

    /// Spawn the three-part fishing strike splash at its fanned-out seats.
    ///
    /// The geometry is [`crate::fishing_chrome::splash_burst`]'s
    /// (`FUN_801D7A5C`); the `>> 3` on the nudge is the host presentation glue
    /// the play window applied before this pool was shared, kept verbatim so
    /// the move changes no pixel on the host that already drew it.
    pub fn spawn_splash(&mut self, parts: &[crate::fishing_chrome::SplashPart]) {
        for p in parts {
            self.spawn(
                (p.x as i32 + (p.nudge.0 >> 3)) as i16,
                (p.y as i32 + (p.nudge.1 >> 3)) as i16,
                p.sprite_id,
            );
        }
    }

    /// Spawn a wander-retarget ripple at the actor's world XZ pair.
    pub fn spawn_ripple(&mut self, r: &crate::fishing_chrome::RippleSpawn) {
        self.spawn(r.pos[0] >> 3, r.pos[2] >> 3, RIPPLE_SPRITE_ID);
    }

    /// Spawn one catch-celebration burst at its offset from the catch point.
    pub fn spawn_burst(&mut self, b: &crate::fishing_actors::CelebrationBurst, origin: (i16, i16)) {
        self.spawn(
            ((origin.0 as i32 + b.offset.0 as i32) >> 3) as i16,
            ((origin.1 as i32 + b.offset.2 as i32) >> 3) as i16,
            BURST_SPRITE_ID,
        );
    }

    /// Age every part by `frame_delta` frames and retire the faded ones - the
    /// same `+0x78` down-ramp [`crate::dance::DanceGame`] runs on its pool.
    pub fn tick(&mut self, frame_delta: u32) {
        let step = (frame_delta * PART_AGE_STEP).min(u32::from(u16::MAX)) as u16;
        for p in self.parts.actors_mut() {
            p.beat = p.beat.saturating_sub(step);
            if p.beat == 0 {
                p.flags |= FLAG_KILLED;
            }
        }
        self.parts.retire_dead();
    }

    /// Drop every live part (a minigame ending, or a scene leaving).
    pub fn clear(&mut self) {
        self.parts.clear();
    }

    /// Live parts.
    pub fn len(&self) -> usize {
        self.parts.len()
    }

    /// No live parts.
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// This frame's draw work: every live part's stage seat, sprite id and
    /// fade weight. A host draws these through
    /// `legaia_engine_ui::minigame_fx::fx_part_draws`.
    pub fn frames(&self) -> Vec<FxPartFrame> {
        self.parts
            .actors()
            .iter()
            .enumerate()
            .map(|(index, a)| FxPartFrame {
                index,
                x: a.pos[0],
                y: a.pos[1],
                sprite: a.sprite,
                fade: sprite_part_fade_weight(a.beat),
            })
            .collect()
    }
}

/// Sprite id the port spawns a fishing wander ripple with.
///
/// A **port choice**, not a reading: the venue overlay forms the ripple
/// spawn's `+0x50` in a register the backward scan does not reach, so the port
/// spawns a distinct id per producer and each host's placeholder table keys on
/// it. Stated rather than implied - the *geometry* is
/// [`crate::fishing_chrome::ripple_spawn`]'s.
pub const RIPPLE_SPRITE_ID: u16 = 0x101;

/// Sprite id the port spawns a catch-celebration burst with (same status as
/// [`RIPPLE_SPRITE_ID`]).
pub const BURST_SPRITE_ID: u16 = 0x102;

/// Sprite id the port spawns the fishing strike splash with when the producer
/// names none (same status as [`RIPPLE_SPRITE_ID`]).
pub const SPLASH_SPRITE_ID: u16 = 0x100;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spawned_part_ages_out() {
        let mut pool = MinigameFxPool::new();
        pool.spawn(160, 120, 0xb);
        assert_eq!(pool.len(), 1);
        let frames_to_zero = u32::from(BEAT_FADE_CEILING) / PART_AGE_STEP;
        pool.tick(frames_to_zero - 1);
        assert_eq!(pool.len(), 1, "still live one frame short of the ramp end");
        pool.tick(1);
        assert!(pool.is_empty(), "retired at the ramp end");
    }

    #[test]
    fn a_fresh_part_resolves_at_the_top_of_the_ramp() {
        let mut pool = MinigameFxPool::new();
        pool.spawn(160, 120, 0xb);
        let f = pool.frames();
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].x, f[0].y, f[0].sprite), (160, 120, 0xb));
        assert_eq!(f[0].fade, 0xFF);
    }

    #[test]
    fn the_pool_is_bounded_oldest_out() {
        let mut pool = MinigameFxPool::new();
        for i in 0..(FX_POOL_CAPACITY + 4) {
            pool.spawn(i as i16, 0, i as u16);
        }
        assert_eq!(pool.len(), FX_POOL_CAPACITY);
        // The four oldest are gone, so the first surviving part is #4.
        assert_eq!(pool.frames()[0].sprite, 4);
    }

    #[test]
    fn the_splash_keeps_the_play_windows_seats() {
        let mut pool = MinigameFxPool::new();
        let parts = crate::fishing_chrome::splash_burst(
            crate::fishing_actors::SCREEN_CENTRE.0,
            crate::fishing_actors::SCREEN_CENTRE.1,
            SPLASH_SPRITE_ID,
            0x40,
        );
        pool.spawn_splash(&parts);
        let f = pool.frames();
        assert_eq!(f.len(), 3);
        // Direct form: the first part moves `-spread >> 3` on both axes, the
        // middle one is the anchor, the third `+spread >> 3` on x.
        assert_eq!((f[1].x, f[1].y), (0xA0, 0x78));
        assert_eq!((f[0].x, f[0].y), (0xA0 - 8, 0x78 - 8));
        assert_eq!(f[2].x, 0xA0 + 8);
    }
}
