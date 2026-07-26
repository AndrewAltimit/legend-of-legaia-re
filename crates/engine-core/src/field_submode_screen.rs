//! Host for the field overlay's **op-`0x49` submode screens**: the missing
//! caller that walks the submode driver actor and invokes its `+0x50` handler.
//!
//! [`legaia_engine_vm::baka_hub_actors`] carries the dispatcher `FUN_801F159C`,
//! its `PTR_FUN_801F33B4` state machines and the `PANEL_WINDOW_TABLE` painters.
//! None of them can run without three things this module supplies: an actor
//! with a `+0x50` handler id, the submode cursor context `DAT_801C6EA4` the
//! dispatcher polls, and a per-frame caller. The engine already had the first
//! ingredient and did not use it - [`World::man_load_actor_reset`] spawns an
//! [`ActorHandler::SubmodeDriver`] actor on every MAN load (retail's
//! `FUN_801D9C3C` at `0x8003B444`) and
//! [`crate::actor_handler::HandlerKernel`] classed it `Unported`, so the actor
//! sat in the pool doing nothing.
//!
//! ## Why this is load-bearing rather than decoration
//!
//! Field-VM opcode `0x49` (`STATE_RESUME`) is a **tristate park**: the script
//! arms `_DAT_8007B450` with its operand pointer and then re-enters the same PC
//! every frame until something writes `1` there. `docs/subsystems/script-vm.md`
//! names the writer: "the Done writer is field-overlay `FUN_801F159C`-class" -
//! that is [`hub_dispatch`]'s retire arm. The engine recognised exactly three
//! sub-ops (`0` inline shop, `3` name entry, `5` tile board) and had no Done
//! writer for the rest, so a script reaching any other sub-op re-armed and
//! halted on the same PC forever. Running the dispatcher supplies the writer.
//!
//! Slot `0` of `PTR_FUN_801F33B4` is `FUN_801F2134` (the close tick), and a
//! freshly spawned driver carries `+0x50 = 0`, so a sub-screen the engine
//! cannot draw yet still closes itself the retail way instead of parking.
//!
//! REF: FUN_8002519C (the `jalr node[+0x0C]` walk this hangs off),
//! FUN_801D9C3C (the spawn), FUN_801E9B3C (the panel install this records
//! rather than performs)

use legaia_engine_vm::baka_hub_actors::{
    self as hub, ACTOR_RETIRE, CoinCounter, HubAction, HubActor, HubDraw, HubEnv, HubFrame,
    HubGrid, HubPainter, slot,
};

use crate::actor_handler::ActorHandler;
use crate::world::World;

/// The live state of the one submode screen a field frame can have up.
///
/// Mirrors the retail globals rather than inventing a shape: [`Self::actor`]
/// is the driver actor's `+0x0A..+0x54` view, [`Self::cursor`] is
/// `DAT_801C6EA4`, [`Self::counter`] the coin counter's own cells.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubmodeScreen {
    /// The driver actor's fields the dispatcher reads and writes.
    pub actor: HubActor,
    /// `DAT_801C6EA4` - the submode cursor context.
    pub cursor: HubGrid,
    /// The coin counter's digit cells, cursors and hold timer.
    pub counter: CoinCounter,
    /// Panel-window record index whose painter runs this frame, if the state
    /// machine has installed a descriptor that names one.
    pub window: Option<usize>,
    /// A screen is up: the op-`0x49` park is Armed.
    pub open: bool,
    /// The dispatcher retired the actor - retail's `_DAT_8007B450 = 1`, which
    /// is the op-`0x49` Done signal.
    pub done: bool,
    /// Whatever the last tick drew and did, for a renderer to consume.
    pub frame: HubFrame,
    /// `FUN_801E9DC8`'s return for the confirm panel, supplied by the host
    /// that owns the two-option picker. `0` while nothing is picking.
    pub picker_result: i32,
}

impl SubmodeScreen {
    /// Is a screen up (the op-`0x49` park should read Armed)?
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Did the last tick hand the frame back (the park should read Done)?
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Every text/sprite draw the last tick produced.
    pub fn draws(&self) -> &[HubDraw] {
        &self.frame.draws
    }
}

impl World {
    /// Open one op-`0x49` sub-screen on the submode driver actor.
    ///
    /// `handler` is a `PTR_FUN_801F33B4` index (see
    /// [`legaia_engine_vm::baka_hub_actors::slot`]) and `window` the
    /// panel-window record whose painter draws it, if one is known.
    ///
    /// Retail's enter path also seeds the cursor context's completion gate to
    /// `1` (`FUN_801F1278` writes `_DAT_801C6EA4+0x3E = 1`); without it the
    /// dispatcher would retire the actor on its very first frame.
    pub fn open_field_submode_screen(&mut self, handler: u16, window: Option<usize>) {
        let s = &mut self.submode_screen;
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
        s.open = true;
        s.done = false;
        s.picker_result = 0;
        s.frame = HubFrame::default();
    }

    /// Open the casino **coin counter** - buy coins with party gold at
    /// [`legaia_engine_vm::baka_hub_actors::GOLD_PER_COIN`] each.
    ///
    /// Handler slot `0x25`; the two-option confirm panel draws through the
    /// panel-window record whose painter is `FUN_801F1950`.
    pub fn open_coin_counter(&mut self) {
        self.open_field_submode_screen(slot::COIN_COUNTER, Some(COIN_PANEL_WINDOW));
    }

    /// Close whatever screen is up without running its hand-back.
    pub fn close_field_submode_screen(&mut self) {
        self.submode_screen.open = false;
        self.submode_screen.window = None;
        self.submode_screen.frame = HubFrame::default();
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
        if !self.submode_screen.open {
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
        let env = self.submode_env(frame_delta);
        let mut screen = std::mem::take(&mut self.submode_screen);
        let window = screen.window;
        let SubmodeScreen {
            actor,
            cursor,
            counter,
            ..
        } = &mut screen;

        let frame = hub::hub_dispatch(actor, &env, cursor, |a, g| {
            let mut f = run_slot(a, &env, g, counter);
            if let Some(p) = window.and_then(HubPainter::for_window) {
                let painted = p.paint(a, &env, g);
                f.draws.extend(painted.draws);
                f.actions.extend(painted.actions);
            }
            f
        });

        let retired = screen.actor.flags & ACTOR_RETIRE != 0;
        screen.frame = frame;
        self.submode_screen = screen;
        self.apply_submode_actions();
        if retired {
            self.submode_screen.open = false;
            self.submode_screen.done = true;
            // Retail retires the node; the engine drops the pool slot the same
            // way every other kill-bit actor goes.
            if let Some(idx) = self.find_actor_by_handler(ActorHandler::SubmodeDriver) {
                self.actors[idx].physics.status_flags |=
                    crate::field_actor_kernels::ACTOR_FLAG_YIELD;
            }
        }
        retired
    }

    /// Project the world's own state onto the globals the family reads.
    fn submode_env(&self, frame_delta: u8) -> HubEnv {
        let pad = self.input.pad() as u32;
        let prev = self.input.pad_prev() as u32;
        let edge = pad & !prev;
        HubEnv {
            // `DAT_801F2734` is the submode context's state word, which
            // `open_submode` seeds and `World::submode_context` mirrors.
            submode: self.submode_context.first().copied().unwrap_or(0) as i32,
            pad_edge: edge,
            pad_held: pad,
            pad_repeat: edge,
            confirm_mask: SUBMODE_ACCEPT_MASK | SUBMODE_BACK_MASK,
            cancel_mask: SUBMODE_BACK_MASK,
            accept_mask: SUBMODE_ACCEPT_MASK,
            back_mask: SUBMODE_BACK_MASK,
            frame_delta: frame_delta.max(1) as i32,
            picker_result: self.submode_screen.picker_result,
            cursor_row: self.submode_screen.counter.cursor,
            cursor_row_alt: self.submode_screen.counter.yes_no,
            // Retail reads the op-0x49 operand pointer here; the engine has no
            // RAM pointer, so it carries the "a screen is armed" truth value
            // the dispatcher's release arm actually branches on.
            board_flag: i32::from(self.submode_screen.open),
            // `DAT_80084594` / `DAT_80084598..` are the present-party roster;
            // the engine's mirror of retail's `0x8007BD10` list is
            // `World::active_party`.
            entry_count: self.active_party.len().min(u8::MAX as usize) as u8,
            entry_codes: self.active_party.clone(),
            gold: self.money,
            coin_bank: self.casino_coins.min(i32::MAX as u32) as i32,
            ..HubEnv::default()
        }
    }

    /// Apply the side effects the last tick reported.
    fn apply_submode_actions(&mut self) {
        let actions = self.submode_screen.frame.actions.clone();
        for a in actions {
            match a {
                // The one action that moves persistent state: coins into the
                // casino bank `DAT_800845A4`, gold out of `DAT_8008459C`.
                HubAction::BuyCoins { coins, gold_cost } => {
                    if coins <= 0 {
                        continue;
                    }
                    self.casino_coins = self
                        .casino_coins
                        .saturating_add(coins as u32)
                        .min(hub::COIN_BANK_MAX as u32);
                    self.money = self.money.saturating_sub(gold_cost).max(0);
                }
                HubAction::ClearCursorRow => self.submode_screen.counter.cursor = 0,
                _ => {}
            }
        }
    }
}

/// Dispatch one `PTR_FUN_801F33B4` slot.
///
/// The slots without a ported body fall through to nothing, which is retail's
/// own behaviour for an index whose handler is a stub - the dispatcher still
/// runs its release arm afterwards, so the screen cannot wedge.
fn run_slot(
    actor: &mut HubActor,
    env: &HubEnv,
    cursor: &mut HubGrid,
    counter: &mut CoinCounter,
) -> HubFrame {
    match actor.state {
        slot::COIN_COUNTER => hub::coin_exchange(actor, env, counter, cursor),
        slot::START_MENU => hub::start_menu(actor, env, cursor),
        slot::PROMPT => hub::hub_prompt(actor, env, cursor),
        slot::SUBMENU => hub::submenu(actor, env, cursor),
        slot::DEACTIVATE => hub::deactivate(actor, env, cursor),
        slot::DRAW_TICK => hub::draw_tick(actor, env, cursor),
        slot::CLOSE_TICK | 0x14..=0x18 => hub::close_tick(actor, env, cursor),
        _ => HubFrame::default(),
    }
}

/// Panel width the driver actor carries (`+0x0E`), the anchor the right-edge
/// cursor of the single-label painters is measured from.
pub const SUBMODE_PANEL_WIDTH: i16 = 0x60;

/// Accept edge in the packed Legaia pad layout (Cross), standing in for
/// `_DAT_800846D0`.
pub const SUBMODE_ACCEPT_MASK: u32 = crate::dev_menu::PACK_CROSS as u32;
/// Back-out edge (Circle), standing in for `_DAT_800846D4`.
pub const SUBMODE_BACK_MASK: u32 = crate::dev_menu::PACK_CIRCLE as u32;

/// Panel-window record whose painter draws the coin counter's Yes/No panel
/// (`FUN_801F1950`).
pub const COIN_PANEL_WINDOW: usize = 1;

/// Op-`0x49` sub-ops the world handles through a dedicated path rather than
/// through a submode screen: `0` inline gold shop, `3` name entry, `5` tile
/// board.
pub const OP49_DEDICATED_SUB_OPS: [u8; 3] = [0, 3, 5];

/// The handler slot an op-`0x49` sub-op opens.
///
/// Only slot `0` is grounded: it is what the spawn descriptor leaves in
/// `+0x50`, so a sub-op whose screen the engine cannot yet name runs the close
/// tick and unparks the script. Which of the 52 slots each remaining sub-op
/// selects is decided by the operand payload retail reads through
/// `_DAT_8007B450`, which the engine does not carry, so this returns the
/// default rather than guessing a mapping.
pub fn slot_for_op49_sub_op(sub_op: u8) -> Option<u16> {
    if OP49_DEDICATED_SUB_OPS.contains(&sub_op) {
        return None;
    }
    Some(slot::CLOSE_TICK)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world_with_driver() -> World {
        let mut w = World::new();
        w.man_load_actor_reset();
        assert!(
            w.find_actor_by_handler(ActorHandler::SubmodeDriver)
                .is_some(),
            "the MAN-load reset spawns the driver actor"
        );
        w
    }

    #[test]
    fn a_screen_only_ticks_while_the_driver_actor_is_alive() {
        let mut w = world_with_driver();
        w.open_coin_counter();
        assert!(!w.tick_submode_screen(1));
        assert!(!w.submode_screen.frame.actions.is_empty());

        // Retire the driver: the dispatcher has nothing to run.
        w.retire_actors_by_handler(ActorHandler::SubmodeDriver);
        w.retire_yielded_actors();
        w.submode_screen.frame = HubFrame::default();
        assert!(!w.tick_submode_screen(1));
        assert!(w.submode_screen.frame.actions.is_empty());
    }

    #[test]
    fn the_coin_counter_moves_coins_and_gold_through_a_world_tick() {
        let mut w = world_with_driver();
        w.money = 5_000;
        w.casino_coins = 7;
        w.open_coin_counter();

        // Frame 1: state 0 seeds the screen.
        w.tick_submode_screen(1);
        // Type 12 coins.
        w.submode_screen.counter.set_entered(12);
        // Frame 2: accept.
        w.input.set_pad(SUBMODE_ACCEPT_MASK as u16);
        w.tick_submode_screen(1);
        assert_eq!(w.submode_screen.actor.sub, 2, "the confirm panel is up");
        // Frame 3: pick Yes on the confirm panel.
        w.input.set_pad(0);
        w.submode_screen.counter.yes_no = 0;
        w.submode_screen.picker_result = hub::PICK_ACCEPT;
        w.tick_submode_screen(1);
        // Frame 4: the commit.
        w.submode_screen.picker_result = 0;
        w.tick_submode_screen(1);

        assert_eq!(w.casino_coins, 7 + 12, "coins land in the casino bank");
        assert_eq!(w.money, 5_000 - 1_200, "gold pays 100 per coin");
    }

    #[test]
    fn the_close_tick_default_unparks_instead_of_hanging() {
        let mut w = world_with_driver();
        // Slot 0 is what a fresh driver carries.
        w.open_field_submode_screen(slot::CLOSE_TICK, None);
        assert!(w.submode_screen.is_open());
        let mut retired = false;
        for _ in 0..8 {
            if w.tick_submode_screen(1) {
                retired = true;
                break;
            }
        }
        assert!(retired, "the close tick clears the gate and retires");
        assert!(w.submode_screen.is_done());
        assert!(!w.submode_screen.is_open());
    }

    #[test]
    fn a_painted_screen_emits_draws() {
        let mut w = world_with_driver();
        w.money = 100_000;
        w.open_coin_counter();
        w.tick_submode_screen(1);
        assert!(
            !w.submode_screen.draws().is_empty(),
            "the panel-window painter runs alongside the state machine"
        );
    }

    #[test]
    fn dedicated_sub_ops_keep_their_own_paths() {
        for s in OP49_DEDICATED_SUB_OPS {
            assert_eq!(slot_for_op49_sub_op(s), None);
        }
        assert_eq!(slot_for_op49_sub_op(9), Some(slot::CLOSE_TICK));
    }
}
