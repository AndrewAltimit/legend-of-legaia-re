//! World-map actor particle-burst emitter, ported clean-room from
//! `FUN_801E5338` (field/world-map overlay band, `0x801E5338`, 201
//! instructions).
//!
//! PORT: FUN_801E5338
//!
//! ## What the retail function does
//!
//! It is a per-actor "sparkle burst" animator: a small state machine on the
//! actor's mode word (`actor + 0x54`) that repeatedly spawns up to eight
//! short-lived sprite particles at jittered offsets around the actor's screen
//! origin, ramps each one's brightness over its ten-frame life, then reports
//! completion once every particle has expired. It is reached from the
//! world-map object-effect dispatch and, like the rest of the world-map code,
//! is a mode hosted in the field overlay rather than an overlay of its own.
//!
//! The disassembly splits into two halves that both run every frame:
//!
//! 1. **The mode SM** (`0x801E535C..0x801E551C`) advances spawning:
//!    - mode `0`  - init: zero the spawn cursor (`+0x9E`), the spawn timer
//!      (`+0x6A`) and the eight per-particle active flags (`+0x80..+0x88`),
//!      then `mode += 1`.
//!    - mode `1`  - spawn: every [`SPAWN_INTERVAL`] frames, if the cursor's
//!      particle slot is free, activate it - palette byte from the caller's
//!      table indexed `slot + anim_row*8`, position jittered off the actor
//!      origin - and advance the cursor (wrapping at 8). Each frame after the
//!      spawn attempt, decrement the spawn-duration counter (`+0x9C`); when it
//!      reaches zero, `mode += 1`.
//!    - mode `2`  - drain: once every particle is inactive, set `mode = 0` and
//!      raise the actor's done bit (`actor+0x10 |= 8`, surfaced here as
//!      [`BurstFrame::finished`]).
//!
//! 2. **The emit tail** (`0x801E5520..0x801E5634`), which runs whenever
//!    `mode != 0`: for each active particle it appends one sprite packet
//!    (GP0 `0x66808080`, a semi-transparent textured sprite,
//!    [`SPRITE_SIZE`]x[`SPRITE_SIZE`]) whose brightness byte is
//!    `lifetime * `[`FADE_STEP`] taken **before** the per-frame increment,
//!    then increments the particle's lifetime and frees it once it reaches
//!    [`PARTICLE_LIFESPAN`]. A closing `SetDrawMode` packet ([`CLOSE_TPAGE`])
//!    is always appended after the loop.
//!
//! Because the emit tail is what ages and expires the particles, a burst in
//! mode `2` keeps draining for as long as any particle is still on screen; the
//! `finished` report lands the frame after the last one expires.
//!
//! ## Clean-room boundaries
//!
//! The palette source is the Sony table at `0x801F2960` (stride 8 bytes per
//! `anim_row`); per the project's no-baked-data rule it is **not** reproduced
//! here - the caller passes the relevant `palette_table` slice. The position
//! jitter draws from the game RNG (`FUN_80056798`); the caller supplies it as
//! a closure so the SM stays deterministic and disc-free under test. The
//! remainder used for the jitter is the C truncated remainder the MIPS
//! computes (`a - (a/N)*N`), which Rust's `%` on `i32` matches exactly,
//! including for negative RNG draws.

/// Number of particle slots per burst (`0x801E54A4`: cursor wraps at 8; the
/// active-flag array `+0x80..+0x88` is eight bytes).
pub const PARTICLE_COUNT: usize = 8;

/// Frames between spawn attempts. `0x801E53D4`: the spawn timer must reach 2
/// before a slot is filled, and it resets to 0 on each attempt.
pub const SPAWN_INTERVAL: i16 = 2;

/// Frames a particle lives before it is freed. `0x801E55DC`: the lifetime is
/// compared `< 0xA` after each draw.
pub const PARTICLE_LIFESPAN: u16 = 10;

/// Full width of the X jitter window (`0x801E543C`: masked to `% 0x20`).
pub const X_SPREAD: i32 = 0x20;

/// Full width of the Y jitter window (`0x801E546C`: masked to `% 0x10`).
pub const Y_SPREAD: i32 = 0x10;

/// Constant added to a particle's palette byte to form the sprite CLUT word
/// (`0x801E5578`: `+0x7F90`, stored at packet `+0xE`).
pub const CLUT_BASE: u16 = 0x7F90;

/// Sprite edge length in pixels (`0x801E553C`: `s5 = 0x18`, written to both
/// packet `+0x10` and `+0x12`).
pub const SPRITE_SIZE: u16 = 0x18;

/// The `u`/`v` byte written at packet `+0xD` (`0x801E559C`: `li v0, 0x90`).
pub const SPRITE_UV_BYTE: u8 = 0x90;

/// GP0 command word of each particle sprite (`0x801E5550`: `0x66808080`) - a
/// semi-transparent, textured sprite.
pub const SPRITE_GP0: u32 = 0x6680_8080;

/// OT tag word written at packet `+0x0` (`0x801E5564`: `lui 0x400`).
pub const SPRITE_TAG: u32 = 0x0400_0000;

/// Per-frame brightness step. `0x801E55AC`: the `+0xC` byte is
/// `lifetime * 3 << 3 == lifetime * 24`.
pub const FADE_STEP: u16 = 24;

/// `tpage` argument of the closing `SetDrawMode` packet (`0x801E5614`:
/// `a3 = 0x1F`).
pub const CLOSE_TPAGE: u16 = 0x1F;

/// One particle slot. Mirrors the actor's per-slot fields: the active flag
/// (`+0x80 + slot`), palette byte (`+0xB0 + slot`), and the halfword life /
/// position triple (`+0xA0` / `+0xB8` / `+0xC8`, each `+ slot*2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Particle {
    /// `actor[+0x80 + slot] != 0`.
    pub active: bool,
    /// `actor[+0xB0 + slot]` - the palette byte copied from the caller table.
    pub palette: u8,
    /// `actor[+0xA0 + slot*2]` - frames elapsed, `0..`[`PARTICLE_LIFESPAN`].
    pub lifetime: u16,
    /// `actor[+0xB8 + slot*2]` - jittered screen X.
    pub x: i16,
    /// `actor[+0xC8 + slot*2]` - jittered screen Y.
    pub y: i16,
}

/// One sprite the emit tail appends for an active particle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BurstSprite {
    /// Screen X (packet `+0x8`).
    pub x: i16,
    /// Screen Y (packet `+0xA`).
    pub y: i16,
    /// CLUT word (packet `+0xE`): `palette + `[`CLUT_BASE`].
    pub clut: u16,
    /// Edge length (packet `+0x10` / `+0x12`): [`SPRITE_SIZE`].
    pub size: u16,
    /// `u`/`v` byte (packet `+0xD`): [`SPRITE_UV_BYTE`].
    pub uv: u8,
    /// Brightness byte (packet `+0xC`): `lifetime * `[`FADE_STEP`], taken
    /// before the per-frame lifetime increment.
    pub fade: u8,
    /// GP0 command word (packet `+0x4`): [`SPRITE_GP0`].
    pub gp0: u32,
    /// OT tag word (packet `+0x0`): [`SPRITE_TAG`].
    pub tag: u32,
}

/// The draw list and status produced by one [`ParticleBurst::tick`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BurstFrame {
    /// One sprite per still-active particle, in slot order.
    pub sprites: Vec<BurstSprite>,
    /// `tpage` of the closing `SetDrawMode`, present exactly when the emit
    /// tail ran (`mode != 0` after the SM step). [`CLOSE_TPAGE`] when present.
    pub close_tpage: Option<u16>,
    /// `true` on the single frame the burst completes (mode `2` drains and
    /// the retail code raises `actor+0x10 |= 8`).
    pub finished: bool,
}

/// A single actor's sparkle-burst state (the subset of the actor record the
/// function touches).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParticleBurst {
    /// `actor[+0x54]` - mode word: 0 init, 1 spawn, 2 drain.
    pub mode: i16,
    /// `actor[+0x6A]` - spawn timer.
    pub timer: i16,
    /// `actor[+0x9E]` - next spawn slot (wraps at [`PARTICLE_COUNT`]).
    pub cursor: i16,
    /// `actor[+0x9C]` - remaining spawn frames; when it hits zero the SM
    /// leaves the spawn mode. Seed it with the desired burst duration.
    pub spawn_frames: i16,
    /// The eight particle slots.
    pub particles: [Particle; PARTICLE_COUNT],
}

impl ParticleBurst {
    /// A fresh burst that will spawn for `spawn_frames` frames once ticked.
    /// Retail relies on the caller having seeded `+0x9C`; mode `0` clears
    /// everything else, so the remaining fields start zeroed.
    pub fn new(spawn_frames: i16) -> Self {
        Self {
            spawn_frames,
            ..Default::default()
        }
    }

    /// Advance one frame.
    ///
    /// - `anim_row` is the actor's `+0x50` selector; the palette byte for slot
    ///   `s` is `palette_table[s + anim_row as usize * 8]`, so the caller must
    ///   pass a slice at least `(anim_row + 1) * 8` bytes long.
    /// - `actor_x` / `actor_y` are the actor screen origin (`+0x14` / `+0x16`).
    /// - `rng` yields the game RNG (`FUN_80056798`) values; two are drawn per
    ///   spawn (X then Y).
    // PORT: FUN_801E5338
    pub fn tick(
        &mut self,
        anim_row: u16,
        actor_x: i16,
        actor_y: i16,
        palette_table: &[u8],
        rng: &mut dyn FnMut() -> i32,
    ) -> BurstFrame {
        let mut finished = false;

        // --- mode state machine (0x801E535C..0x801E551C) ---
        match self.mode {
            0 => {
                // 0x801E5394: init then fall through to `mode += 1`.
                self.cursor = 0;
                self.timer = 0;
                for p in &mut self.particles {
                    p.active = false;
                }
                self.mode += 1;
            }
            1 => {
                self.spawn_step(anim_row, actor_x, actor_y, palette_table, rng);
                // 0x801E54B4: decrement the spawn-frame counter, or leave the
                // spawn mode once it is exhausted.
                if self.spawn_frames != 0 {
                    self.spawn_frames -= 1;
                } else {
                    self.mode += 1;
                }
            }
            // 0x801E54E0: complete once every slot is inactive; while any
            // particle is still on screen the burst stays in mode 2 and the
            // emit tail keeps draining it.
            2 if self.particles.iter().all(|p| !p.active) => {
                self.mode = 0;
                finished = true;
            }
            _ => {}
        }

        // --- emit tail (0x801E5520..0x801E5634), runs while mode != 0 ---
        let mut sprites = Vec::new();
        let mut close_tpage = None;
        if self.mode != 0 {
            for p in &mut self.particles {
                if !p.active {
                    continue;
                }
                // Brightness uses the pre-increment lifetime (0x801E55AC).
                let fade = p.lifetime.wrapping_mul(FADE_STEP) as u8;
                sprites.push(BurstSprite {
                    x: p.x,
                    y: p.y,
                    clut: (p.palette as u16).wrapping_add(CLUT_BASE),
                    size: SPRITE_SIZE,
                    uv: SPRITE_UV_BYTE,
                    fade,
                    gp0: SPRITE_GP0,
                    tag: SPRITE_TAG,
                });
                // 0x801E55C8: age, and free once the life is spent.
                p.lifetime = p.lifetime.wrapping_add(1);
                if p.lifetime >= PARTICLE_LIFESPAN {
                    p.active = false;
                    p.lifetime = 0;
                }
            }
            close_tpage = Some(CLOSE_TPAGE);
        }

        BurstFrame {
            sprites,
            close_tpage,
            finished,
        }
    }

    /// The mode-1 spawn attempt (`0x801E53BC..0x801E54B0`).
    fn spawn_step(
        &mut self,
        anim_row: u16,
        actor_x: i16,
        actor_y: i16,
        palette_table: &[u8],
        rng: &mut dyn FnMut() -> i32,
    ) {
        // 0x801E53C4: timer++; only attempt once it has reached the interval.
        self.timer = self.timer.wrapping_add(1);
        if self.timer < SPAWN_INTERVAL {
            return;
        }
        self.timer = 0;

        let slot = self.cursor as usize;
        // 0x801E53F4: a busy slot blocks the spawn (the cursor does NOT
        // advance in that case).
        if self.particles[slot].active {
            return;
        }

        // 0x801E5400: palette byte from the caller's table, row-major by
        // anim_row, stride 8.
        let palette = palette_table[slot + anim_row as usize * 8];

        // 0x801E5424: X jitter = actor_x + (rand % 0x20) - 0x10.
        let rx = rng();
        let x = (actor_x as i32 + (rx % X_SPREAD) - X_SPREAD / 2) as i16;
        // 0x801E545C: Y jitter = actor_y + (rand % 0x10) - 0x8.
        let ry = rng();
        let y = (actor_y as i32 + (ry % Y_SPREAD) - Y_SPREAD / 2) as i16;

        self.particles[slot] = Particle {
            active: true,
            palette,
            lifetime: 0,
            x,
            y,
        };

        // 0x801E548C: advance the spawn cursor, wrapping at 8.
        self.cursor += 1;
        if self.cursor >= PARTICLE_COUNT as i16 {
            self.cursor = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic RNG stub returning a fixed script of values.
    fn scripted(values: Vec<i32>) -> impl FnMut() -> i32 {
        let mut it = values.into_iter().cycle();
        move || it.next().unwrap()
    }

    /// 64-byte palette table (8 rows x 8 slots) with a recognisable pattern.
    fn palette() -> Vec<u8> {
        (0..64u8).collect()
    }

    #[test]
    fn init_advances_to_spawn_and_clears_state() {
        let mut b = ParticleBurst {
            mode: 0,
            timer: 99,
            cursor: 5,
            spawn_frames: 3,
            particles: [Particle {
                active: true,
                ..Default::default()
            }; PARTICLE_COUNT],
        };
        let mut rng = scripted(vec![0]);
        let frame = b.tick(0, 100, 50, &palette(), &mut rng);

        assert_eq!(b.mode, 1);
        assert_eq!(b.timer, 0);
        assert_eq!(b.cursor, 0);
        assert!(b.particles.iter().all(|p| !p.active));
        // Emit tail ran (mode became 1) but no particle is active yet.
        assert!(frame.sprites.is_empty());
        assert_eq!(frame.close_tpage, Some(CLOSE_TPAGE));
        assert!(!frame.finished);
    }

    #[test]
    fn spawns_one_particle_every_interval() {
        let mut b = ParticleBurst::new(100);
        b.mode = 1;
        let pal = palette();
        // rand X, rand Y per spawn.
        let mut rng = scripted(vec![0, 0]);

        // Frame 1: timer 0->1, below interval, no spawn.
        let f1 = b.tick(0, 0, 0, &pal, &mut rng);
        assert!(f1.sprites.is_empty());
        assert_eq!(b.particles.iter().filter(|p| p.active).count(), 0);

        // Frame 2: timer 1->2, spawn slot 0, cursor -> 1.
        let f2 = b.tick(0, 0, 0, &pal, &mut rng);
        assert_eq!(b.cursor, 1);
        assert!(b.particles[0].active);
        // The just-spawned particle draws this frame (spawn precedes emit).
        assert_eq!(f2.sprites.len(), 1);
        assert_eq!(b.timer, 0);
    }

    #[test]
    fn palette_index_is_row_major_stride_eight() {
        let mut b = ParticleBurst::new(100);
        b.mode = 1;
        b.timer = 1; // spawn on this tick
        let pal = palette();
        let mut rng = scripted(vec![0, 0]);
        b.tick(3, 0, 0, &pal, &mut rng);
        // slot 0, anim_row 3 -> index 0 + 3*8 = 24.
        assert_eq!(b.particles[0].palette, 24);
        assert!(b.particles[0].active);
    }

    #[test]
    fn jitter_matches_truncated_remainder_including_negatives() {
        let mut b = ParticleBurst::new(100);
        b.mode = 1;
        b.timer = 1;
        let pal = palette();
        // X rand = -33 -> (-33 % 32) - 16 = -1 - 16 = -17; actor_x 200 -> 183.
        // Y rand = 5   -> (5 % 16) - 8 = 5 - 8 = -3;      actor_y 100 -> 97.
        let mut rng = scripted(vec![-33, 5]);
        b.tick(0, 200, 100, &pal, &mut rng);
        assert_eq!(b.particles[0].x, 183);
        assert_eq!(b.particles[0].y, 97);
    }

    #[test]
    fn sprite_carries_the_fixed_packet_constants_and_fade_ramp() {
        let mut b = ParticleBurst::new(100);
        b.mode = 1;
        b.timer = 1;
        let pal = palette();
        let mut rng = scripted(vec![0, 0]);
        let f = b.tick(0, 0, 0, &pal, &mut rng);
        let s = f.sprites[0];
        assert_eq!(s.gp0, 0x6680_8080);
        assert_eq!(s.tag, 0x0400_0000);
        assert_eq!(s.size, 0x18);
        assert_eq!(s.uv, 0x90);
        assert_eq!(s.clut, CLUT_BASE); // palette byte 0
        // First draw uses lifetime 0 -> fade 0.
        assert_eq!(s.fade, 0);
        // After the draw the particle aged to 1.
        assert_eq!(b.particles[0].lifetime, 1);
    }

    #[test]
    fn particle_lives_exactly_lifespan_frames_with_ramping_fade() {
        // Drive the drain path directly with a single active particle so the
        // emit tail is the only thing touching its lifetime.
        let mut b = ParticleBurst::new(0);
        b.mode = 2;
        b.particles[0] = Particle {
            active: true,
            palette: 0,
            lifetime: 0,
            x: 0,
            y: 0,
        };
        let pal = palette();
        let mut rng = scripted(vec![0, 0]);

        let mut fades = Vec::new();
        for _ in 0..PARTICLE_LIFESPAN {
            let f = b.tick(0, 0, 0, &pal, &mut rng);
            if let Some(s) = f.sprites.first() {
                fades.push(s.fade);
            }
        }
        // Ten draws with lifetime 0..9 -> fade 0,24,48,...,216.
        assert_eq!(fades.len(), PARTICLE_LIFESPAN as usize);
        assert_eq!(fades[0], 0);
        assert_eq!(fades[1], 24);
        assert_eq!(fades[9], (9 * FADE_STEP) as u8);
        // Now expired.
        assert!(!b.particles[0].active);
    }

    #[test]
    fn drain_reports_finished_the_frame_after_the_last_particle_expires() {
        let mut b = ParticleBurst::new(0);
        b.mode = 2;
        // One particle already at the edge of its life.
        b.particles[0] = Particle {
            active: true,
            lifetime: PARTICLE_LIFESPAN - 1,
            ..Default::default()
        };
        let pal = palette();
        let mut rng = scripted(vec![0, 0]);

        // Frame A: still active at tick start -> draws, then expires. Not
        // finished yet.
        let a = b.tick(0, 0, 0, &pal, &mut rng);
        assert_eq!(a.sprites.len(), 1);
        assert!(!a.finished);
        assert!(!b.particles[0].active);

        // Frame B: all inactive -> completes, mode resets to 0, no draw.
        let bf = b.tick(0, 0, 0, &pal, &mut rng);
        assert!(bf.finished);
        assert_eq!(b.mode, 0);
        assert!(bf.sprites.is_empty());
        assert_eq!(bf.close_tpage, None);
    }

    #[test]
    fn busy_slot_blocks_spawn_without_advancing_cursor() {
        let mut b = ParticleBurst::new(100);
        b.mode = 1;
        b.timer = 1;
        b.particles[0].active = true; // slot 0 busy
        let pal = palette();
        let mut rng = scripted(vec![7, 7]);
        b.tick(0, 0, 0, &pal, &mut rng);
        // Cursor stayed at 0 (no spawn happened); timer still reset.
        assert_eq!(b.cursor, 0);
        assert_eq!(b.timer, 0);
    }
}
