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
//! A fight reads the same word. Every battle reader takes the edge word
//! `_DAT_8007B874` the pad builder latched at the previous frame's end, or
//! the held words `_DAT_8007B850` / `_DAT_8007B938` beside it, from inside
//! the battle frame: the command flow `FUN_801D0748` (`0x801D0800`,
//! `0x801D0B14`, `0x801D21B8`), the action SM `FUN_801E295C` (`0x801E5988`,
//! `0x801E608C`, `0x801E63A8`), the side-band pass `FUN_80056208`
//! (`0x80056384`), the frame driver `FUN_80046A20` (`0x80046D80`,
//! `0x800472A4`), the list kernels the item and magic windows call
//! (`FUN_80031D00`, `FUN_80032A44`, `FUN_80035C24`) and the tutorial module
//! (`0x801F720C`). A battle frame spans its step, so the frame that opens on
//! tick `T` reads the host pad of `T_prev - 1 + k`, `k` the previous frame's
//! cost in whole vsyncs (its push on the frame-cost ring), capped one short
//! of its span. A PCSX-Redux film from `battle_menu_rugi_barra_retail`
//! (frames of about `650` hblanks at step `3`) pushes the round prompt's cue
//! three vsyncs after a Right and after a Left.
//!
//! The port's default delivers the pad on the vsync it is pressed
//! (`WorldToggles::immediate_pad`): the same inputs act two vsyncs sooner and
//! no tap is dropped, the better experience. The retail-faithful mode runs
//! the pipeline.
//!
//! REF: FUN_80016B6C, FUN_8001822C, FUN_801D0748, FUN_801E295C

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
    /// The sim tick the last battle frame opened on: a battle frame's sample
    /// point is read at the next open, once its cost is on the ring.
    battle_open: Option<u32>,
}

impl World {
    /// Whether the pad the world's logic reads runs through the retail
    /// pipeline this tick.
    ///
    /// On the field, the overworld and in a fight on retail's battle frames
    /// (`WorldToggles::battle_sm_frame_cadence`), and only while the world
    /// ticks on every pad it is handed: a tick a host screen owns (the pause
    /// menu) runs no world tick, and the pad goes straight through until the
    /// field runs again. A harness that authors one-tick battle presses
    /// (`InputState::battle_immediate`) keeps the fight's pad immediate.
    pub fn retail_pad_pipeline_live(&self) -> bool {
        let mode_live = match self.mode {
            SceneMode::Field | SceneMode::WorldMap => true,
            SceneMode::Battle => {
                self.toggles.battle_sm_frame_cadence && !self.input.battle_immediate()
            }
            _ => false,
        };
        !self.toggles.immediate_pad
            && self.clock.game_tick_live
            && mode_live
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
        if self.mode == SceneMode::Battle {
            return self.battle_frame_pad_step(mask, tick);
        }
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

    /// [`Self::frame_pad_step`] in a fight: the frame opens on the first tick
    /// of a battle frame ([`Self::battle_frame_opens_next_tick`]), and it reads the host
    /// pad of the previous frame's sample point, `k` whole vsyncs of that
    /// frame's cost past the vblank it opened on (the ring's newest push,
    /// [`legaia_engine_battle_loop::frame_clock::FrameCostRing::newest`]),
    /// capped one short of the vsyncs it spanned. The game word and the open
    /// tick are the state carried; no sample point is kept between opens.
    fn battle_frame_pad_step(&self, mask: u16, tick: u32) -> (u16, Option<u32>) {
        let p = &self.pad_latency;
        if !self.battle_frame_opens_next_tick() {
            return (p.game, None);
        }
        let Some(open) = p
            .battle_open
            .filter(|&o| tick.wrapping_sub(o) <= HISTORY as u32)
        else {
            return (mask, None);
        };
        let span = tick.wrapping_sub(open).max(1);
        let cost = u32::from(self.battle.frame_cost_ring.newest());
        let k = if cost == 0 {
            span - 1
        } else {
            (cost / 263).min(span - 1)
        };
        let sample = open.wrapping_sub(1).wrapping_add(k);
        // `k < span`, so `sample` is two or more ticks back; a pipeline that
        // went live since then has not recorded it.
        let game = if p.newest.is_some_and(|n| sample <= n) {
            p.hist[sample as usize % HISTORY]
        } else {
            mask
        };
        (game, None)
    }

    /// Whether the tick about to run is the first of a battle frame on the
    /// frame clock. Not the action SM's pass claim
    /// ([`Self::battle_sm_pass_due_next_tick`]): while a command surface is
    /// up the live loop returns ahead of the claim, and the frames still
    /// open on the clock.
    fn battle_frame_opens_next_tick(&self) -> bool {
        self.battle
            .frame_clock
            .ticks_into_frame(self.clock.display_frames + 1)
            == 0
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
        let battle_opens = self.mode == SceneMode::Battle
            && self.retail_pad_pipeline_live()
            && self.battle_frame_opens_next_tick();
        let p = &mut self.pad_latency;
        if battle_opens {
            p.battle_open = Some(tick);
        }
        p.hist[tick as usize % HISTORY] = mask;
        p.newest = Some(tick);
        p.game = game;
        p.sample_tick = sample;
        game
    }

    /// The edge word a host screen (the pause menu and its sub-screens, a
    /// shop, the prize exchange) reads this tick, the window kernel's cursor
    /// auto-repeat folded in. `edge` is the host's just-pressed word. With
    /// [`WorldToggles::immediate_pad`] up (the default) that is `edge` and
    /// this tick's repeat; in the retail-faithful mode the screen reads the
    /// retail pipeline ([`legaia_engine_system::input::InputState::retail_screen_edge`]):
    /// confirm and cancel two vsyncs after the press, a cursor step three.
    ///
    /// REF: FUN_801D688C, FUN_80031D00, FUN_8001822C
    pub fn screen_edge(&mut self, edge: u16) -> u16 {
        self.screen_edge_with(edge, true)
    }

    /// [`Self::screen_edge`] for a screen whose cursor reads the frame's own
    /// edge word rather than the window kernel's (`cursor_lags = false`):
    /// the Save / Load block grid steps on the press's second vsync, as its
    /// confirm does (a capture over `save_grid_endgame_card_conc`).
    pub fn screen_edge_with(&mut self, edge: u16, cursor_lags: bool) -> u16 {
        if self.toggles.immediate_pad || self.input.screen_immediate() {
            edge | self.input.take_menu_repeat()
        } else {
            self.input.retail_screen_edge(cursor_lags)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_system::input::PadButton;

    /// Vsyncs from a four-vsync press opening on host tick `press` to the
    /// first tick the game word holds it, on a step-2 field whose frames
    /// cost `cost` hblanks. The ticks advance as `World::tick` moves them:
    /// `set_pad` on the tick about to run, then the actor accumulator.
    fn latency(cost: i32, press: u32) -> u32 {
        let mut w = World::new();
        w.toggles.immediate_pad = false;
        w.mode = SceneMode::Field;
        w.clock.game_tick_live = true;
        w.clock.frame_step = 2;
        w.clock.frame_cost = Some(cost);
        let tri = PadButton::Triangle.mask();
        for _ in 0..200 {
            let tick = w.clock.sim_ticks + 1;
            let host = if (press..press + 4).contains(&tick) {
                tri
            } else {
                0
            };
            if w.retail_frame_pad(host) & tri != 0 {
                return tick - press;
            }
            w.clock.sim_ticks = tick;
            w.clock.actor_vsync_accum += 1;
            if w.clock.actor_vsync_accum >= w.clock.game_frame_wait() {
                w.clock.actor_vsync_accum = 0;
            }
        }
        panic!("press at {press} never reached the game word");
    }

    /// Vsyncs from a three-vsync press opening on host tick `press` to the
    /// first tick a fight's game word holds it, on step-`3` battle frames
    /// opening on ticks `== 1 (mod 3)` whose cost is `cost` hblanks.
    fn battle_latency(cost: u16, press: u32) -> u32 {
        let mut w = World::new();
        w.toggles.immediate_pad = false;
        w.toggles.battle_sm_frame_cadence = true;
        w.mode = SceneMode::Battle;
        w.clock.game_tick_live = true;
        w.clock.sim_ticks = 0;
        w.clock.display_frames = 0;
        w.seed_frame_cost_ring([cost; 16], 0);
        w.battle.frame_clock.set_step(1, 3);
        let right = PadButton::Right.mask();
        for _ in 0..200 {
            let tick = w.clock.sim_ticks + 1;
            let host = if (press..press + 3).contains(&tick) {
                right
            } else {
                0
            };
            if w.retail_frame_pad(host) & right != 0 {
                return tick - press;
            }
            w.clock.sim_ticks = tick;
            w.clock.display_frames = u64::from(tick);
        }
        panic!("press at {press} never reached the game word");
    }

    /// The battle command flow `FUN_801D0748` builds its pad word from the
    /// edge the pad builder latched at the previous frame's end
    /// (`0x801D0B0C..0x801D0B20`), so a press waits on the battle frame.
    /// PCSX-Redux films (frames of `530..650` hblanks at step `3`): the
    /// round prompt on `battle_menu_rugi_barra_retail` pushes its cue three
    /// vsyncs after a press three before a frame opens and two after one two
    /// before; a press one vsync before an open waits four (the item window
    /// on `battle_item_window_gaza2_retail`, the Auto / Command prompt and
    /// the arts entry on `battle_auto_prompt_gaza2_retail`, the target
    /// cursor on `battle_target_cursor_rugi_barra_retail`). A frame that
    /// ends a vsync early samples a vsync earlier.
    ///
    /// REF: FUN_801D0748, FUN_8001822C
    #[test]
    fn a_battle_press_waits_on_the_battle_frame() {
        // Frames open on ticks 1, 4, 7, ..., 61, 64.
        let at = |cost| [58u32, 59, 60].map(|p| battle_latency(cost, p));
        assert_eq!(at(650), [3, 2, 4]);
        assert_eq!(at(530), [3, 2, 4]);
        assert_eq!(at(300), [3, 5, 4]);
    }

    /// The default takes a battle press on its tick, and so does a harness
    /// that authors one-tick battle presses (`InputState::battle_immediate`).
    #[test]
    fn a_battle_press_is_immediate_by_default_and_under_the_harness_seat() {
        let mut w = World::new();
        w.mode = SceneMode::Battle;
        w.toggles.battle_sm_frame_cadence = true;
        w.clock.game_tick_live = true;
        assert!(!w.retail_pad_pipeline_live(), "the default is immediate");
        w.toggles.immediate_pad = false;
        assert!(w.retail_pad_pipeline_live());
        w.input.set_battle_immediate(true);
        assert!(!w.retail_pad_pipeline_live());
    }

    /// The field's menu accept (`0x801D0250`) reads the edge the pad builder
    /// latched at the frame before's end, so how long a Triangle press takes
    /// to open the menu is the frame's cost, not a constant. PCSX-Redux walk
    /// films: `town01_field_card_boot` and `conc_field_card_boot` (frames
    /// about 130 and 210 hblanks, under a vblank) push the open's cue `0x20`
    /// four vsyncs after a press on an even vsync, three after one on an odd
    /// one; `kor_field_route_walk` (about 350, past a vblank) raises the
    /// engaged bit two vsyncs after a press.
    ///
    /// REF: FUN_801D01B0, FUN_8001822C
    #[test]
    fn a_menu_press_waits_on_the_frames_cost() {
        let short: Vec<u32> = (40..44).map(|p| latency(130, p)).collect();
        assert_eq!(short.iter().min(), Some(&3));
        assert_eq!(short.iter().max(), Some(&4));
        let long: Vec<u32> = (40..44).map(|p| latency(350, p)).collect();
        assert_eq!(long.iter().min(), Some(&2));
        assert_eq!(long.iter().max(), Some(&3));
    }
}
