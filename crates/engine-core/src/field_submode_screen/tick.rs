//! Opening, closing and ticking the op-`0x49` submode screen on the `World`.

use super::*;

impl World {
    /// Open one op-`0x49` sub-screen on the submode driver actor.
    ///
    /// `handler` is a `PTR_FUN_801F33B4` index (see
    /// [`legaia_engine_field::submode_hub_actors::slot`]) and `window` the
    /// panel-window record whose painter draws it, if one is known.
    ///
    /// Retail's enter path also seeds the cursor context's completion gate to
    /// `1` (`FUN_801F1278` writes `_DAT_801C6EA4+0x3E = 1`); without it the
    /// dispatcher would retire the actor on its very first frame.
    ///
    /// It also **spawns the driver actor**, and that is not optional. The
    /// op-`0x49` Idle arm calls the allocator unconditionally on every arm,
    /// before it stores the operand pointer into the park:
    ///
    /// ```text
    /// 801e0984  lbu   v0,0x0(s6)        ; sub_op
    /// 801e098c  sltiu v0,v0,0xe         ; sub_op < 0xE ?
    /// 801e0990  beq   v0,zero,0x801e3624
    /// 801e09a0  jal   0x80020de0        ; spawn the driver - EVERY arm
    /// 801e09a4  _addiu a0,a0,0x65c      ;   from descriptor 0x8007065C
    /// 801e09a8  sw    s6,-0x4bb0(s0)    ; _DAT_8007B450 = operand ptr
    /// ```
    ///
    /// (`overlay_0897_801de840.txt`, `FUN_801DE840` case `0x49`.) The engine
    /// had only the one driver [`World::man_load_actor_reset`] spawns per MAN
    /// load, and [`World::tick_submode_screen`] *retires* that driver when a
    /// screen hands back - so the second screen of a scene had no dispatcher,
    /// never handed back, and left the park Armed for the rest of the scene.
    ///
    /// REF: FUN_80020DE0 (the allocator), FUN_801D9C3C (the MAN-load spawn)
    pub fn open_field_submode_screen(&mut self, handler: u16, window: Option<usize>) {
        let owner = self.op49_park_owner();
        if self
            .find_actor_by_handler(ActorHandler::SubmodeDriver)
            .is_none()
            && let Some(slot) = self.spawn_handler_actor(ActorHandler::SubmodeDriver)
        {
            // Retail clears the fresh driver's `+0x50` / `+0x54`; the handler
            // slot this screen wants goes on the screen's own actor view.
            self.actors[slot].state_54 = 0;
        }
        let s = &mut self.field_vm.submode_screen;
        s.owner = owner;
        s.actor = HubActor {
            width: SUBMODE_PANEL_WIDTH,
            state: handler,
            ..HubActor::default()
        };
        s.cursor = HubGrid {
            done_gate: 1,
            ..HubGrid::default()
        };
        s.counter = CoinCounter::default();
        s.window = window;
        s.installed_windows.clear();
        s.pad_edge_latch = 0;
        s.open = true;
        s.done = false;
        s.picker_result = 0;
        s.frame = HubFrame::default();
        s.flag_window = Default::default();
        s.code_lock = Default::default();
        s.coin_panel.clear();
        s.soft_reset = SoftResetScreen::default();
    }

    /// Open the casino **coin counter** - buy coins with party gold at
    /// [`legaia_engine_field::submode_hub_actors::GOLD_PER_COIN`] each.
    ///
    /// Handler slot `0x25`. No window is pinned: the counter's own arms
    /// install `PANEL_COIN_IDLE` and `PANEL_COIN_CONFIRM`, and those
    /// descriptors name the records that draw it - record `10` for the idle
    /// screen and the **three-line** panel `FUN_801F1890` for the confirm.
    pub fn open_coin_counter(&mut self) {
        self.open_field_submode_screen(slot::COIN_COUNTER, None);
    }

    /// Close whatever screen is up without running its hand-back.
    pub fn close_field_submode_screen(&mut self) {
        self.field_vm.submode_screen.open = false;
        self.field_vm.submode_screen.window = None;
        self.field_vm.submode_screen.frame = HubFrame::default();
        self.field_vm.submode_screen.coin_panel.clear();
    }

    /// Run one dispatcher frame over the submode driver actor.
    ///
    /// This is the `jalr node[+0x0C]` arm of `FUN_8002519C` for
    /// [`ActorHandler::SubmodeDriver`], so it is called from
    /// [`World::tick_handler_actors`] on the same cadence as the colour tween.
    ///
    /// Returns `true` when the dispatcher retired the actor this frame, which
    /// is retail's `_DAT_8007B450 = 1` and unparks the field VM's op `0x49`.
    pub fn tick_submode_screen(&mut self, frame_delta: u8) -> bool {
        if !self.field_vm.submode_screen.open {
            return false;
        }
        // Retail runs this off an actor on the pool; with no live driver there
        // is nothing to dispatch, exactly as before the spawn.
        if self
            .find_actor_by_handler(ActorHandler::SubmodeDriver)
            .is_none()
        {
            return false;
        }
        // The coin counter's Yes/No confirm runs over the shared two-row
        // picker (`FUN_801E9DC8`): retail's picker maintains the row cursor
        // (`counter.yes_no`, seeded to 1 = No) and its return is what the
        // handler reads as `env.picker_result`. The engine drives that
        // picker here, at the model layer, so every host gets a live
        // confirm - `picker_result` used to have no production writer at
        // all, which parked the counter at state 2 forever with the field
        // script still Armed (a softlock at every koin1/balden coin
        // cabinet's accept). A host/test that pre-loaded `picker_result`
        // keeps its value for this pass (the derive only fills an empty
        // cell); either way the dispatch below consumes it one-shot.
        if self.field_vm.submode_screen.actor.state == hub::slot::COIN_COUNTER
            && self.field_vm.submode_screen.actor.sub == 2
            && self.field_vm.submode_screen.picker_result == 0
        {
            let (pad, prev) = self.submode_pad_words();
            let edge = (pad & !prev) | self.field_vm.submode_screen.pad_edge_latch;
            let toggle = (legaia_engine_system::pad::PACK_UP
                | legaia_engine_system::pad::PACK_DOWN
                | legaia_engine_system::pad::PACK_LEFT
                | legaia_engine_system::pad::PACK_RIGHT) as u32;
            if edge & toggle != 0 {
                self.field_vm.submode_screen.counter.yes_no ^= 1;
            }
            if edge & SUBMODE_ACCEPT_MASK != 0 {
                self.field_vm.submode_screen.picker_result = hub::PICK_ACCEPT;
            } else if edge & SUBMODE_BACK_MASK != 0 {
                self.field_vm.submode_screen.picker_result = hub::PICK_CANCEL;
            }
        }
        let env = self.submode_env(frame_delta);
        // The latch is one-shot: a pass that ran has consumed every edge that
        // reached it, exactly as retail's per-game-tick pad sample is.
        self.field_vm.submode_screen.pad_edge_latch = 0;
        // Slot 0x23 (the flag-window picker) reads its descriptor off the
        // parked operand and the system flag bank; its writes land after the
        // dispatch, once the bank is no longer borrowed for the scan.
        let flag_desc = self.flag_window_descriptor();
        // Slot 0x21 (the code lock) compares against the parked operand's
        // five code bytes and reads the edge word as symbols.
        let lock_code = self.code_lock_code();
        let lock_edge = env.pad_edge;
        let lock_blocked = env.input_blocked != 0;
        let lock_delta = env.frame_delta;
        let flag_pad = legaia_engine_field::world_map_panel::CursorPad {
            held: env.pad_edge,
            pressed: env.pad_repeat,
            action_a_mask: SUBMODE_ACCEPT_MASK,
            action_b_mask: SUBMODE_BACK_MASK,
        };
        let mut flag_writes = Vec::new();
        let mut screen = std::mem::take(&mut self.field_vm.submode_screen);
        let window = screen.window;
        let mut installed = std::mem::take(&mut screen.installed_windows);
        let mut reset_fx = Vec::new();
        let SubmodeScreen {
            actor,
            cursor,
            counter,
            flag_window,
            code_lock,
            soft_reset,
            ..
        } = &mut screen;
        let flag_bank: &[u8] = &self.flags.system_flags;
        let flag_test = |id: i32| {
            u16::try_from(id).is_ok_and(|i| {
                flag_bank
                    .get(usize::from(i >> 3))
                    .is_some_and(|b| b & (0x80u8 >> (i & 7)) != 0)
            })
        };

        let frame = hub::hub_dispatch(actor, &env, cursor, |a, g| {
            let mut f = if a.state == crate::field_submode_flag_window::FLAG_WINDOW_SLOT {
                legaia_engine_field::field_submode_flag_window::flag_window_slot(
                    a,
                    g,
                    flag_window,
                    flag_desc,
                    flag_pad,
                    flag_test,
                    &mut flag_writes,
                )
            } else if a.state == legaia_engine_field::field_submode_code_lock::CODE_LOCK_SLOT {
                legaia_engine_field::field_submode_code_lock::code_lock_slot(
                    a,
                    g,
                    code_lock,
                    &lock_code,
                    lock_edge,
                    lock_blocked,
                    lock_delta,
                    &mut flag_writes,
                )
            } else if a.state == SOFT_RESET_SLOT {
                use legaia_engine_field::world_map_panel_actors::{
                    SoftResetInput, soft_reset_tick,
                };
                let (phase, slide, fx) = soft_reset_tick(
                    soft_reset.phase,
                    SoftResetInput {
                        frame_delta: env.frame_delta,
                        slide: soft_reset.slide,
                        // Retail samples the held word once per game tick;
                        // the latched edge keeps a tap that fell between two
                        // ticks (see `pad_edge_latch`).
                        pad: env.pad_held | env.pad_edge,
                    },
                );
                soft_reset.phase = phase;
                soft_reset.slide = slide;
                reset_fx = fx;
                HubFrame::default()
            } else {
                run_slot(a, &env, g, counter)
            };
            // An install replaces whatever panel was up, and takes effect on
            // the frame that issued it: retail's window walk runs after the
            // handler in the same frame.
            if let Some(desc) = f.actions.iter().rev().find_map(|act| match act {
                HubAction::InstallPanel(va) => Some(*va),
                _ => None,
            }) {
                let named = legaia_engine_field::field_submode_flag_window::installed_windows(desc);
                if descriptor_closes_all(desc) {
                    installed = named;
                } else {
                    for idx in named {
                        if !installed.contains(&idx) {
                            installed.push(idx);
                        }
                    }
                }
            }
            // The host-pinned window (if any) draws alongside the installed
            // one; retail has no such override, so it is additive rather than
            // a replacement.
            // A descriptor may name the same record twice (the coin idle
            // program does); the window system installs it once.
            let mut to_paint: Vec<usize> = Vec::new();
            for idx in installed.iter().copied().chain(window) {
                if !to_paint.contains(&idx) {
                    to_paint.push(idx);
                }
            }
            for idx in to_paint {
                if let Some(p) = HubPainter::for_window(idx) {
                    // A painter's `a0` is the **window's** actor, which the
                    // descriptor's open placed at the record's geometry
                    // (`FUN_800357FC` stores its x / y into the actor's
                    // `+0x0A` / `+0x0C`), not the submode driver.
                    let (ox, oy) = panel_window_origin(idx);
                    let (dx, dy) = (a.x, a.y);
                    a.x = ox;
                    a.y = oy;
                    let painted = p.paint(a, &env, g);
                    a.x = dx;
                    a.y = dy;
                    f.draws.extend(painted.draws);
                    f.actions.extend(painted.actions);
                }
            }
            f
        });
        screen.installed_windows = installed;

        let retired = screen.actor.flags & ACTOR_RETIRE != 0;
        screen.frame = frame;
        self.field_vm.submode_screen = screen;
        self.apply_flag_window_writes(&flag_writes);
        // The picker return is one-shot: the pass that ran has consumed it
        // (retail's `FUN_801E9DC8` returns a fresh value per call).
        self.field_vm.submode_screen.picker_result = 0;
        self.apply_submode_actions();
        self.paint_coin_entry_panel();
        self.apply_soft_reset_effects(&reset_fx);
        if retired {
            self.field_vm.submode_screen.open = false;
            self.field_vm.submode_screen.done = true;
            // Retail retires the node; the engine drops the pool slot the same
            // way every other kill-bit actor goes.
            if let Some(idx) = self.find_actor_by_handler(ActorHandler::SubmodeDriver) {
                self.actors[idx].physics.status_flags |=
                    legaia_engine_field::scene_transition_sweep::ACTOR_FLAG_YIELD;
            }
        }
        retired
    }
}
