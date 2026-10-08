//! The mode seat's `World` seam.
//!
//! The retail mode table, [`ModeDriver`], [`ModeSeat`] and [`SceneMode`]
//! live in [`legaia_engine_field::mode`] and are re-exported here at their old
//! paths. The seat reaches the world only through [`ModeWorld`]; this module
//! is where `World` implements it, plus the field-entry demo handler, which
//! drives the actor VM and so needs the whole world.

pub use legaia_engine_field::mode::*;

use crate::input::InputState;
use crate::world::World;
use legaia_engine_vm::Position as ActorVmPosition;

impl ModeWorld for World {
    fn scene_mode(&self) -> SceneMode {
        self.mode
    }

    fn set_scene_mode(&mut self, mode: SceneMode) {
        self.mode = mode;
    }

    fn take_frame_begin_skip(&mut self) -> bool {
        World::take_frame_begin_skip(self)
    }

    fn clear_frame_begin_skip(&mut self) {
        self.clock.frame_begin_skip = false;
    }

    fn detach_sound(&mut self) {
        World::detach_sound(self);
    }

    fn battle_mode_word_held(&self) -> bool {
        World::battle_mode_word_held(self)
    }

    fn clear_pad_edges(&mut self) {
        self.input.clear_edges();
    }

    fn tick_frame(&mut self) {
        World::tick(self);
    }
}

/// Reference [`ModeHandler`] that drives the field-entry mode pair
/// (`MainInit` → `MainMode`, the retail field/town init + per-frame
/// handlers) end-to-end without any GPU / scene-asset dependencies. Useful
/// as a smoke test for the World + ModeDriver wiring and as an example for
/// engines integrating real scene loaders.
///
/// Behaviour:
///
/// - `MainInit`: spawn `actor_count` actors in the world via the actor VM
///   `SpawnAt` opcode, with positions arranged on a horizontal line. Returns
///   `Done` so the driver advances to the table's next mode - mirroring the
///   retail mode-2 handler's "load the scene, hand off to mode 3" shape.
/// - `MainMode`: ticks the world (positions advance via the move VM). When
///   the host signals `Cross` (just-pressed), returns `GoTo(MapdispInit)` -
///   the field → world-map exit transition. Otherwise `Continue`.
/// - Other modes: no-op `Continue`.
///
/// This is the smallest concrete demonstration that the World + ModeDriver
/// stack ticks per-frame, advances actor state, and reacts to input.
#[derive(Debug, Clone, Copy)]
pub struct FieldDemoHandler {
    pub actor_count: u8,
    initialised: bool,
}

impl FieldDemoHandler {
    pub fn new(actor_count: u8) -> Self {
        Self {
            actor_count,
            initialised: false,
        }
    }
}

impl ModeHandler<World> for FieldDemoHandler {
    fn run(&mut self, mode: GameMode, world: &mut World, input: &InputState) -> HandlerResult {
        use crate::input::PadButton;
        match mode {
            GameMode::MainInit => {
                if !self.initialised {
                    // Set per-actor default positions before spawning so
                    // the actor VM SpawnDefault path lands them on a row.
                    for i in 0..self.actor_count {
                        let slot = i as usize;
                        if slot >= world.actors.len() {
                            break;
                        }
                        world.actors[slot].default_pos =
                            ActorVmPosition::new(32 + (slot as i16) * 24, 64);
                    }
                    // Synthesize bytecode: SpawnDefault for each actor, then End.
                    let mut bytecode = Vec::with_capacity((self.actor_count as usize + 1) * 4);
                    for i in 0..self.actor_count {
                        // 4-byte instruction: opcode=0x01 (SpawnDefault), operand_b=actor_id, w=0
                        bytecode.extend_from_slice(&[0x01, i, 0x00, 0x00]);
                    }
                    bytecode.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // End
                    let _ = world.run_actor_bytecode(&bytecode);
                    self.initialised = true;
                }
                HandlerResult::Done
            }
            GameMode::MainMode => {
                if input.just_pressed(PadButton::Cross) {
                    HandlerResult::GoTo(GameMode::MapdispInit)
                } else {
                    HandlerResult::Continue
                }
            }
            _ => HandlerResult::Continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entering_an_init_mode_returns_its_plan_and_leaves_the_run_sibling() {
        let mut world = World::new();
        let mut seat = ModeSeat::new_at_boot();
        assert_eq!(seat.game_mode(), GameMode::ReadInit);

        let plan = seat.enter(GameMode::MainInit, &mut world);
        match plan {
            Some(ModeInitPlan::Stage(st)) => {
                assert_eq!(st.overlay_a_param, 2);
                assert_eq!(st.overlay_entry, 0x801D_6704);
            }
            other => panic!("MAIN INIT should stage the field overlay, got {other:?}"),
        }
        assert_eq!(seat.game_mode(), GameMode::MainMode);
        assert_eq!(seat.scene_mode(), SceneMode::Field);
    }

    #[test]
    fn an_entered_mode_swallows_the_pad_edge_that_caused_it() {
        let mut world = World::new();
        let mut seat = ModeSeat::new(GameMode::MainMode);
        // A frame with Start newly pressed - the edge that opens the menu.
        world.set_pad(0);
        world.set_pad(crate::input::PadButton::Start.mask());
        assert!(world.input.just_pressed(crate::input::PadButton::Start));

        seat.enter(GameMode::CardInit, &mut world);
        assert_eq!(seat.game_mode(), GameMode::CardMode);
        // Held is untouched; only the edge is gone.
        assert!(!world.input.just_pressed(crate::input::PadButton::Start));
        assert!(world.input.pressed(crate::input::PadButton::Start));
        assert_eq!(seat.edges(), 1);
    }

    /// The other direction, and the one a host's frame loop depends on: an
    /// edge the seat *adopts* leaves the pad alone, because the host has
    /// already published this frame's word by the time the frame runs.
    #[test]
    fn an_adopted_mode_change_leaves_this_frames_input_alone() {
        let mut world = World::new();
        let mut seat = ModeSeat::new(GameMode::MainMode);
        seat.adopt_scene_mode(SceneMode::Battle);
        world.set_pad(0);
        world.set_pad(crate::input::PadButton::Circle.mask());

        let f = seat.frame(&mut world);
        let edge = f.edge.expect("the word moved, so the edge is taken");
        assert_eq!(edge.to, GameMode::BattleMode);
        assert!(!edge.swallowed_pad_edges);
        assert!(
            world.input.just_pressed(crate::input::PadButton::Circle),
            "the frame's own input survives an adopted transition"
        );
    }

    #[test]
    fn card_mode_is_the_one_mode_that_runs_no_master_driver() {
        let mut world = World::new();
        let mut seat = ModeSeat::new(GameMode::CardMode);
        let f = seat.frame(&mut world);
        assert!(!f.runs_master_driver);
        assert_eq!(f.stage.unwrap().body, FrameBody::CardDriver);

        let mut seat = ModeSeat::new(GameMode::MainMode);
        assert!(seat.frame(&mut world).runs_master_driver);
    }

    #[test]
    fn a_frame_begin_skip_abandons_the_frame_before_the_world_ticks() {
        struct Noop;
        impl ModeHandler<World> for Noop {
            fn run(&mut self, _m: GameMode, _w: &mut World, _i: &InputState) -> HandlerResult {
                HandlerResult::Continue
            }
        }
        let mut d = ModeDriver::new(GameMode::MainMode);
        let mut w = World::default();
        let input = InputState::default();

        let before = w.clock.sim_ticks;
        w.clock.frame_begin_skip = true;
        d.tick(&mut Noop, &mut w, &input);
        assert_eq!(
            w.clock.sim_ticks, before,
            "FUN_8001698C returned 1 - no frame ran"
        );
        assert!(!w.clock.frame_begin_skip, "the request is consumed");
        assert_eq!(d.last_stage().unwrap().body, FrameBody::Master { param: 1 });

        // Default (nothing set) is the ordinary every-frame tick.
        d.tick(&mut Noop, &mut w, &input);
        assert!(w.clock.sim_ticks != before);
    }

    #[test]
    fn init_modes_ignore_the_frame_begin_skip() {
        struct Noop;
        impl ModeHandler<World> for Noop {
            fn run(&mut self, _m: GameMode, _w: &mut World, _i: &InputState) -> HandlerResult {
                HandlerResult::Continue
            }
        }
        let mut d = ModeDriver::new(GameMode::CardInit);
        let mut w = World::default();
        let before = w.clock.sim_ticks;
        w.clock.frame_begin_skip = true;
        d.tick(&mut Noop, &mut w, &InputState::default());
        assert!(
            w.clock.sim_ticks != before,
            "only the per-frame handlers carry the early-out"
        );
        assert!(d.last_stage().is_none());
    }

    #[test]
    fn resolve_frame_step_installs_the_floor_when_frameskip_is_off() {
        let mut w = World {
            clock: crate::world::FrameClock {
                frame_step_floor: 3,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(w.resolve_frame_step(0x400, false), 3);
        assert_eq!(w.clock.frame_step, 3);
        // With the gate on, a spike raises past the floor for one frame.
        assert_eq!(w.resolve_frame_step(0x400, true), 4);
        assert_eq!(w.resolve_frame_step(0x10, true), 3, "then decays to it");
    }

    /// Mode 0 CONFIG INIT is the sound-detach caller, and the `gp+0x804`
    /// latch makes every frame after the first a no-op.
    #[test]
    fn config_init_runs_the_sound_detach_exactly_once() {
        struct Noop;
        impl ModeHandler<World> for Noop {
            fn run(&mut self, _m: GameMode, _w: &mut World, _i: &InputState) -> HandlerResult {
                HandlerResult::Continue
            }
        }
        let mut d = ModeDriver::new(GameMode::ConfigInit);
        let mut w = World::default();
        assert!(!w.audio.sound_detach.is_detached());
        d.tick(&mut Noop, &mut w, &InputState::default());
        assert!(w.audio.sound_detach.is_detached());
        // A second frame in the same mode must not re-run it.
        assert!(!w.detach_sound());

        // MAIN INIT does not (its stage plan has runs_core_reset = false).
        let mut d = ModeDriver::new(GameMode::MainInit);
        let mut w = World::default();
        d.tick(&mut Noop, &mut w, &InputState::default());
        assert!(!w.audio.sound_detach.is_detached());
    }

    /// The sound-release deadline is counted in vsyncs by `World::tick`, so
    /// it survives a cadence change unchanged.
    #[test]
    fn the_sound_release_timer_fires_through_the_world_tick() {
        let mut w = World::default();
        w.arm_sound_release(2);
        let mut fired = 0;
        for _ in 0..40 {
            w.tick();
            if w.take_pending_sound_release() {
                fired += 1;
            }
        }
        assert_eq!(fired, 1, "the deadline fires once and disarms");
        assert!(!w.audio.sound_release.armed);
    }

    /// The driver installs the PAIR into the World, not the mode word alone.
    #[test]
    fn the_driver_carries_the_sub_id_into_the_world() {
        let mut d = ModeDriver::new(GameMode::OtherMode);
        let mut h = NoopHandler;
        let mut w = World::default();
        let input = InputState::new();
        // With no staged sub-id the driver has nothing to resolve with.
        d.tick(&mut h, &mut w, &input);
        assert_eq!(w.mode, SceneMode::Title);
        // Stage the fishing door (sub-id 0) the way the `0x3E` arm does, with
        // a session installed - a minigame mode with no session self-heals
        // back to its return mode inside the world tick, which would mask the
        // install this test is about.
        w.enter_fishing(crate::fishing::PondSession::new(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            0,
            0,
            0,
            0,
            crate::fishing::FishingRecord::default(),
            0,
            0,
        ));
        w.mode = SceneMode::Title;
        d.set_warp_sub_id(Some(0));
        assert_eq!(d.warp_sub_id(), Some(0));
        d.tick(&mut h, &mut w, &input);
        assert_eq!(w.mode, SceneMode::Fishing);
        // Retail leaves the register standing across the init -> run handoff
        // (`FUN_80025980` writes only the mode word on its way out), so the
        // mode word moving does not lose the discriminator.
        d.jump_to(GameMode::OtherInit);
        assert_eq!(d.scene_mode(), SceneMode::Fishing);
        d.jump_to(GameMode::OtherMode);
        assert_eq!(d.scene_mode(), SceneMode::Fishing);
    }

    #[test]
    fn handler_continue_keeps_mode_and_increments_frames() {
        let mut d = ModeDriver::new(GameMode::MapdispMode);
        let mut h = NoopHandler;
        let mut w = World::default();
        let input = InputState::new();
        for _ in 0..3 {
            assert_eq!(d.tick(&mut h, &mut w, &input), HandlerResult::Continue);
        }
        assert_eq!(d.current(), GameMode::MapdispMode);
        assert_eq!(d.frames, 3);
        assert_eq!(d.frames_in_mode, 3);
        assert_eq!(w.mode, SceneMode::WorldMap);
    }

    #[test]
    fn handler_done_transitions_to_next_when_set() {
        struct DoneOnce {
            ticked: bool,
        }
        impl ModeHandler<World> for DoneOnce {
            fn run(&mut self, _: GameMode, _: &mut World, _: &InputState) -> HandlerResult {
                if self.ticked {
                    HandlerResult::Continue
                } else {
                    self.ticked = true;
                    HandlerResult::Done
                }
            }
        }
        // MainMode has next=ConfigInit - transition should land there.
        let mut d = ModeDriver::new(GameMode::MainMode);
        let mut h = DoneOnce { ticked: false };
        let mut w = World::default();
        let input = InputState::new();
        d.tick(&mut h, &mut w, &input);
        assert_eq!(d.current(), GameMode::ConfigInit);
        assert_eq!(d.frames_in_mode, 0);
    }

    #[test]
    fn handler_done_no_op_when_next_is_none() {
        // ConfigInit has next=None - Done should leave the mode unchanged.
        struct AlwaysDone;
        impl ModeHandler<World> for AlwaysDone {
            fn run(&mut self, _: GameMode, _: &mut World, _: &InputState) -> HandlerResult {
                HandlerResult::Done
            }
        }
        let mut d = ModeDriver::new(GameMode::ConfigInit);
        let mut h = AlwaysDone;
        let mut w = World::default();
        let input = InputState::new();
        d.tick(&mut h, &mut w, &input);
        assert_eq!(d.current(), GameMode::ConfigInit);
    }

    #[test]
    fn handler_goto_jumps_directly() {
        struct GoToBattle;
        impl ModeHandler<World> for GoToBattle {
            fn run(&mut self, _: GameMode, _: &mut World, _: &InputState) -> HandlerResult {
                HandlerResult::GoTo(GameMode::BattleInit)
            }
        }
        let mut d = ModeDriver::new(GameMode::MapdispMode);
        let mut h = GoToBattle;
        let mut w = World::default();
        let input = InputState::new();
        d.tick(&mut h, &mut w, &input);
        assert_eq!(d.current(), GameMode::BattleInit);
    }

    #[test]
    fn field_demo_handler_spawns_actors_then_advances() {
        let mut d = ModeDriver::new(GameMode::MainInit);
        let mut h = FieldDemoHandler::new(4);
        let mut w = World::default();
        let input = InputState::new();
        // First tick: MainInit spawns actors and reports Done - driver
        // advances to MainInit's next entry (which is None per the table,
        // so we stay in MainInit). The actors should still be live.
        let r = d.tick(&mut h, &mut w, &input);
        assert_eq!(r, HandlerResult::Done);
        // 4 actors spawned at the staggered positions.
        assert!(w.actors[0].active);
        assert!(w.actors[3].active);
        assert!(!w.actors[4].active);
        assert_eq!(w.actors[1].move_state.world_x, 32 + 24);
    }

    #[test]
    fn field_demo_handler_main_mode_transitions_on_cross() {
        let mut d = ModeDriver::new(GameMode::MainMode);
        let mut h = FieldDemoHandler::new(0);
        let mut w = World::default();
        let mut input = InputState::new();
        // No press: stays.
        let r = d.tick(&mut h, &mut w, &input);
        assert_eq!(r, HandlerResult::Continue);
        assert_eq!(d.current(), GameMode::MainMode);
        // Cross press: transitions to MapdispInit.
        input.set_pad(crate::input::PadButton::Cross.mask());
        let r = d.tick(&mut h, &mut w, &input);
        assert_eq!(r, HandlerResult::GoTo(GameMode::MapdispInit));
        assert_eq!(d.current(), GameMode::MapdispInit);
    }
}
