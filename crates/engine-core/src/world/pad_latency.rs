//! The retail pad pipeline: when the game sees a press.
//!
//! Retail reads the pad once a game frame. The frame pacer `FUN_80016B6C`
//! calls the pad builder `FUN_8001822C` at the end of the frame body
//! (`jal 0x8001822C` at `0x80017034`), ahead of the frame-time sample
//! `FUN_800173BC` and the `VSync(step)` wait, and the builder copies the
//! libpad buffer (`0x800840F8`, refreshed at every vblank) into the held word
//! `_DAT_8007B850`. The frame after reads that word. So a frame opened at
//! vblank `B` samples the buffer as of the vblank its body ends after,
//! `B + cost / 263` (the frame time in hblanks), and the frame opened at
//! `B + step` is the first to act on it.
//!
//! PCSX-Redux films measure it (`kor_field_route_walk`, step 2, about 350
//! hblanks a frame; `map02_field_door_hop`, step 3, about 660): the held word
//! takes a press within the vblank after the body ends, the walk moves one
//! frame later, and a one-vsync tap that lands after a frame's sample is
//! never seen. On the port's tick numbering (tick `t` is the state retail's
//! RAM holds at vblank `t`, so a frame opened at vblank `B` is tick `B + 1`)
//! the frame opened at tick `T` takes the host pad of tick
//! `T_prev - 1 + k`, where `T_prev` opened the frame before and `k` is that
//! frame's whole vsyncs of body: two vsyncs late at the field's step 2 and
//! the overworld's step 3, and held for the frame.
//!
//! The port's default delivers the pad on the vsync it is pressed
//! (`WorldToggles::immediate_pad`): the same inputs act two vsyncs sooner and
//! no tap is dropped, the better experience. The retail-faithful mode runs
//! the pipeline.
//!
//! REF: FUN_80016B6C, FUN_8001822C

use super::*;
use legaia_engine_mode::mode::SceneMode;

/// History the pipeline keeps: enough host pads to reach the sample of the
/// frame before at any frame step the resolver picks.
const HISTORY: usize = 16;

/// The retail pad pipeline's state ([`World::retail_frame_pad`]).
#[derive(Debug, Clone, Default)]
pub struct PadLatency {
    /// Host pads by sim tick, `hist[tick % HISTORY]`.
    hist: [u16; HISTORY],
    /// The sim tick the newest entry belongs to.
    newest: Option<u32>,
    /// The word the game reads, held from one frame open to the next.
    game: u16,
    /// The sim tick whose host pad the next frame open takes: the current
    /// frame's sample point.
    sample_tick: Option<u32>,
}

impl World {
    /// Whether the pad the world's logic reads runs through the retail
    /// pipeline this tick.
    ///
    /// Only on the field and the overworld, the walks the films measure, and
    /// only while the world ticks on every pad it is handed: a tick a host
    /// screen owns (the pause menu) runs no world tick, and the pad goes
    /// straight through until the field runs again.
    pub fn retail_pad_pipeline_live(&self) -> bool {
        !self.toggles.immediate_pad
            && self.clock.game_tick_live
            && matches!(self.mode, SceneMode::Field | SceneMode::WorldMap)
            && self
                .pad_latency
                .newest
                .is_none_or(|n| n == self.clock.sim_ticks)
    }

    /// What [`Self::retail_frame_pad`] would hand game logic for the host's
    /// `mask` on the tick about to run, without moving the pipeline: a host
    /// that answers a press ahead of `set_pad` (the pause menu's accept)
    /// reads the same word the field controller will.
    pub fn preview_frame_pad(&self, mask: u16) -> u16 {
        self.frame_pad_step(mask).0
    }

    /// The game word for the tick about to run and the next sample point.
    fn frame_pad_step(&self, mask: u16) -> (u16, Option<u32>) {
        let p = &self.pad_latency;
        if !self.retail_pad_pipeline_live() {
            return (mask, None);
        }
        let tick = self.clock.sim_ticks.wrapping_add(1);
        // Whether the tick about to run opens a game frame: the actor pool's
        // accumulator test in `World::tick_modes`.
        let opens = self.clock.actor_vsync_accum.saturating_add(1) >= self.clock.game_frame_wait();
        if !opens {
            return (p.game, p.sample_tick);
        }
        let game = match p.sample_tick {
            // The sample the frame before took; the host pad of that tick.
            Some(s) if s == tick => mask,
            Some(s)
                if p.newest
                    .is_some_and(|n| tick.wrapping_sub(s) <= HISTORY as u32 && s <= n) =>
            {
                p.hist[s as usize % HISTORY]
            }
            _ => mask,
        };
        // This frame's body ends `k` whole vsyncs in; its sample is the
        // buffer at that vblank, the host pad of tick `tick - 1 + k`.
        let step = self.clock.frame_step.max(1);
        let k = match self.clock.frame_cost {
            Some(cost) => ((cost.max(0) / 263) as u8).min(step - 1),
            None => step - 1,
        };
        (game, Some(tick.wrapping_sub(1).wrapping_add(u32::from(k))))
    }

    /// Seat the pipeline's latched sample on `mask` (host-pad layout), as a
    /// state saved after a frame's pad build holds it: the next frame to
    /// open reads `mask`, and the frames after it sample the host pad again.
    /// A harness standing the world on a retail capture.
    pub fn seat_retail_frame_pad(&mut self, mask: u16) {
        let tick = self.clock.sim_ticks;
        let p = &mut self.pad_latency;
        p.hist[tick as usize % HISTORY] = mask;
        p.newest = Some(tick);
        p.game = mask;
        p.sample_tick = Some(tick);
    }

    /// The pad word game logic reads on the tick about to run, given the
    /// host's `mask` for it. With [`WorldToggles::immediate_pad`] up, or
    /// while no game-tick clock runs (a mode with no master frame driver),
    /// that is `mask` itself; otherwise it is the retail sample
    /// ([`self`](crate::world::pad_latency)).
    pub(crate) fn retail_frame_pad(&mut self, mask: u16) -> u16 {
        let tick = self.clock.sim_ticks.wrapping_add(1);
        let (game, sample) = self.frame_pad_step(mask);
        let p = &mut self.pad_latency;
        p.hist[tick as usize % HISTORY] = mask;
        p.newest = Some(tick);
        p.game = game;
        p.sample_tick = sample;
        game
    }
}
