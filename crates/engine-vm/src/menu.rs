//! Menu state-machine port - clean-room reimplementation of the menu
//! overlay's top-level dispatcher (`FUN_801DD35C` in the captured
//! `overlay_menu` program).
//!
//! PORT: FUN_801DD35C
//!
//! Shape: not an opcode VM (the menu doesn't run bytecode like the field
//! / move VMs), but a state machine driven by input + a frame counter,
//! with an outer `switch(state)` over ~28 numeric states. Mirrors the
//! battle-action state machine in [`super::battle_action`].
//!
//! State numbering follows the case labels in the captured dispatcher
//! (`overlay_menu_801de234.txt`): outer switch on `state` reads through
//! `FUN_801E38D0(_DAT_801F0204)` which is the active-menu-id resolver.
//! State bytes ≥ `0x70` are control / transition words (e.g. `0x70` is
//! the close-and-deactivate path the dispatcher takes when the player
//! cancels with Triangle).
//!
//! This module establishes the typed surface (state enum + host trait +
//! step entry point) plus the per-screen routing graph. [`commit_route`]
//! and [`back_route`] encode the dispatcher's `_DAT_801F0204 = N` writes:
//! the multi-step shop (browse -> quantity -> confirm -> back-to-list /
//! exit) and inn (confirm -> sleep) flows advance on Cross and back up one
//! screen on Triangle, on top of the host's commit kernels. Status
//! sub-screens back up to the status top-level. Per-screen specifics that
//! still need a side-effect kernel (item-use apply, the standalone
//! save/load progress states - the live save UI is driven separately by
//! the save-select session) stay as pass-through screens for now.
//!
//! See [`docs/subsystems/`] for the menu-VM doc page (TODO: add when
//! the second pass lands).
//! REF: FUN_801E38D0

// Menu is a state machine, not an opcode VM - no shared host dependency
// with the actor / move / field VMs.

/// Top-level menu state. The byte values match the case labels in
/// `overlay_menu_801de234.txt`'s outer `switch(uVar6)` block. Values not
/// listed here are still routed through [`step`] but treated as
/// pass-through transitions (the dispatcher's "default" arm).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MenuState {
    /// Closed - no menu open. Engine has not requested entry.
    Closed = 0x00,
    /// Idle / pre-init - menu requested but the open animation hasn't
    /// started yet.
    Idle = 0x01,
    /// Status menu top-level. Outer-switch case `0x0B`.
    StatusTop = 0x0B,
    /// Status - character submenu.
    StatusCharacter = 0x0C,
    /// Status - equipment submenu.
    StatusEquipment = 0x0D,
    /// Status - inventory submenu.
    StatusInventory = 0x0E,
    /// Status - magic submenu.
    StatusMagic = 0x0F,
    /// Status - Tactical Arts submenu.
    StatusTacticalArts = 0x10,
    /// Status - config submenu.
    StatusConfig = 0x11,
    /// Status - log / records submenu.
    StatusLog = 0x12,
    /// Save / load - pick slot.
    SavePickSlot = 0x13,
    /// Save / load - confirm overwrite.
    SaveConfirmOverwrite = 0x14,
    /// Save / load - write progress.
    SaveWriting = 0x15,
    /// Save / load - done.
    SaveDone = 0x16,
    /// Save / load - load slot.
    LoadSlot = 0x17,
    /// Save / load - load progress.
    LoadProgress = 0x18,
    /// Shop - buy.
    ShopBuy = 0x19,
    /// Shop - sell.
    ShopSell = 0x1A,
    /// Shop - quantity prompt.
    ShopQuantity = 0x1B,
    /// Shop - confirm.
    ShopConfirm = 0x1C,
    /// Shop - exit.
    ShopExit = 0x1D,
    /// Inn - confirm.
    InnConfirm = 0x1E,
    /// Inn - sleep transition.
    InnSleep = 0x1F,
    /// Item-use - pick target.
    ItemPickTarget = 0x20,
    /// Item-use - apply.
    ItemApply = 0x21,
    /// Item-use - done.
    ItemDone = 0x22,
    /// Generic confirm yes/no.
    Confirm = 0x6E,
    /// Closing - fade-out animation.
    Closing = 0x70,
    /// Deactivate - runs once at the very end of [`Closing`] to release
    /// the menu and resume the field VM.
    Deactivate = 0x71,
}

impl MenuState {
    pub fn as_byte(self) -> u8 {
        self as u8
    }

    /// Narrow a state byte to a known [`MenuState`]. Returns `None` for
    /// bytes not in the documented case list - callers that need to
    /// dispatch unknown states fall through to the default "pass through"
    /// arm in [`step`].
    pub fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0x00 => Self::Closed,
            0x01 => Self::Idle,
            0x0B => Self::StatusTop,
            0x0C => Self::StatusCharacter,
            0x0D => Self::StatusEquipment,
            0x0E => Self::StatusInventory,
            0x0F => Self::StatusMagic,
            0x10 => Self::StatusTacticalArts,
            0x11 => Self::StatusConfig,
            0x12 => Self::StatusLog,
            0x13 => Self::SavePickSlot,
            0x14 => Self::SaveConfirmOverwrite,
            0x15 => Self::SaveWriting,
            0x16 => Self::SaveDone,
            0x17 => Self::LoadSlot,
            0x18 => Self::LoadProgress,
            0x19 => Self::ShopBuy,
            0x1A => Self::ShopSell,
            0x1B => Self::ShopQuantity,
            0x1C => Self::ShopConfirm,
            0x1D => Self::ShopExit,
            0x1E => Self::InnConfirm,
            0x1F => Self::InnSleep,
            0x20 => Self::ItemPickTarget,
            0x21 => Self::ItemApply,
            0x22 => Self::ItemDone,
            0x6E => Self::Confirm,
            0x70 => Self::Closing,
            0x71 => Self::Deactivate,
            _ => return None,
        })
    }
}

/// Where a Cross-commit routes the state machine next, as a function of
/// the current screen and the committed slot. `None` means "stay on the
/// current screen" - the screen's [`MenuHost::commit`] hook applied a
/// side effect in place (a status sub-screen mutation, a save-slot pick)
/// but the menu doesn't move.
///
/// PORT: the per-case `_DAT_801F0204 = N` writes in `FUN_801DD35C`. The
/// shop flow returns to the buy list after every confirm (the player buys
/// repeatedly and leaves with Triangle); the inn rest routes through the
/// sleep fade only on "yes".
pub fn commit_route(state: MenuState, slot: u8) -> Option<MenuState> {
    match state {
        // Shop: pick an item (buy or sell), choose a quantity, confirm,
        // then drop back to the buy list to shop again.
        MenuState::ShopBuy | MenuState::ShopSell => Some(MenuState::ShopQuantity),
        MenuState::ShopQuantity => Some(MenuState::ShopConfirm),
        MenuState::ShopConfirm => Some(MenuState::ShopBuy),
        // Inn: "yes" (slot 0) plays the rest fade; "no" closes the prompt.
        MenuState::InnConfirm if slot == 0 => Some(MenuState::InnSleep),
        MenuState::InnConfirm => Some(MenuState::Closing),
        _ => None,
    }
}

/// Where a Triangle (cancel / back) routes the state machine. Multi-step
/// flows back up one screen; status sub-screens return to the status
/// top-level; top-of-flow screens close the menu. Routing to
/// [`MenuState::Closing`] also fires [`MenuHost::cancel`] so the engine
/// can tear down the active session.
///
/// PORT: the Triangle-handling arms of `FUN_801DD35C`.
pub fn back_route(state: MenuState) -> MenuState {
    match state {
        // Shop: step back through the purchase flow.
        MenuState::ShopQuantity => MenuState::ShopBuy,
        MenuState::ShopConfirm => MenuState::ShopQuantity,
        // Leaving the shop list runs the canonical teardown screen so the
        // session always clears.
        MenuState::ShopBuy | MenuState::ShopSell => MenuState::ShopExit,
        // Status sub-screens return to the status top-level.
        MenuState::StatusCharacter
        | MenuState::StatusEquipment
        | MenuState::StatusInventory
        | MenuState::StatusMagic
        | MenuState::StatusTacticalArts
        | MenuState::StatusConfig
        | MenuState::StatusLog => MenuState::StatusTop,
        // Everything else (status top-level, slot pickers, confirms)
        // closes the menu.
        _ => MenuState::Closing,
    }
}

/// Transient screens auto-advance: they fire a one-shot
/// [`MenuHost::commit`] side effect on entry, hold for the render layer's
/// animation, then route forward along [`commit_route`] (falling back to
/// [`MenuState::Closing`]). They ignore input. Used for the shop teardown
/// and the inn rest fade.
fn is_transient(state: MenuState) -> bool {
    matches!(state, MenuState::ShopExit | MenuState::InnSleep)
}

/// Menu input - narrowed to the buttons the dispatcher actually reads.
/// The full PSX pad has more, but the menu only checks Cross / Circle /
/// Triangle / Square + the d-pad.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MenuInput {
    pub cross: bool,
    pub circle: bool,
    pub triangle: bool,
    pub square: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

/// Menu execution context - analogue of the menu overlay's RAM scratch
/// at `_DAT_801F0204`. Holds per-frame state the dispatcher reads and
/// writes (active state byte, cursor position within the current screen,
/// frame counter for animations, etc.).
#[derive(Debug, Clone, Copy, Default)]
pub struct MenuCtx {
    pub state: u8,
    /// Cursor index inside the current screen (0..=N depending on the
    /// active menu screen).
    pub cursor: u8,
    /// Frame counter - incremented every [`step`] until reset by a state
    /// transition. Drives open / close animations.
    pub frame: u16,
    /// Selected slot (e.g. save-slot index, shop-item index, party-member
    /// index - meaning depends on active state).
    pub selected_slot: u8,
}

/// Side-effect host the menu VM calls into. Engines impl this against
/// their save / shop / party state.
pub trait MenuHost {
    /// Number of menu items the active screen offers, used to clamp
    /// cursor wrap-around. Default 1 keeps the cursor pinned at 0 - fine
    /// for tests / engines that haven't wired real item lists.
    fn screen_item_count(&self, _state: MenuState) -> u8 {
        1
    }

    /// Called when [`step`] decides the active screen should commit
    /// (Cross button + cursor on a confirm row). The host applies any
    /// side effect (write save, deduct money, etc.) and the next [`step`]
    /// will route to the appropriate follow-up state.
    fn commit(&mut self, _state: MenuState, _selected_slot: u8) {}

    /// Called when [`step`] decides the menu should close (Triangle in
    /// most states). Default: no-op - the VM still transitions to
    /// [`MenuState::Closing`] regardless.
    fn cancel(&mut self) {}

    /// Number of frames the [`MenuState::Closing`] hold runs before the
    /// VM transitions to [`MenuState::Deactivate`]. The retail dispatcher
    /// at `_DAT_801f0204 = 0` is an immediate set; the menu render layer
    /// drives a separate per-frame fade buffer (alpha ramp on the panel
    /// background) and the SM here represents that fade as a hold timer.
    /// Default `16` matches the 0x10-frame fade that engine-render uses
    /// for `MenuState::Closing` panel alpha; engines that drive their own
    /// fade override this.
    fn close_hold_frames(&self) -> u16 {
        16
    }

    /// Number of frames a transient screen (shop teardown, inn rest fade)
    /// holds before auto-advancing along its forward route. The one-shot
    /// side effect fires on entry; the hold gives the render layer time to
    /// play the fade. Default `8`; engines that drive their own animation
    /// override per state.
    fn transient_hold_frames(&self, _state: MenuState) -> u16 {
        8
    }
}

/// One frame of menu execution. Reads `ctx.state`, applies `input`,
/// possibly mutates `ctx`, possibly calls hooks on `host`. Returns the
/// post-step state byte for engines that want to drive UI off it.
///
/// Open / close transitions are handled directly. Transient screens
/// ([`is_transient`]) fire their one-shot side effect on entry, hold, then
/// auto-advance. Every other (interactive) screen advances the cursor on
/// the d-pad, commits on Cross - then routes forward via [`commit_route`] -
/// and on Triangle routes back via [`back_route`].
pub fn step<H: MenuHost + ?Sized>(host: &mut H, ctx: &mut MenuCtx, input: MenuInput) -> u8 {
    ctx.frame = ctx.frame.wrapping_add(1);
    let state = MenuState::from_byte(ctx.state);
    match state {
        // No menu open - nothing to do.
        Some(MenuState::Closed) => {}
        // Pre-init: roll forward into the status menu top-level after a
        // single tick. Engines that need a real open animation extend
        // this branch with a frame check.
        Some(MenuState::Idle) => {
            ctx.state = MenuState::StatusTop.as_byte();
            ctx.frame = 0;
        }
        // Closing: hold for `host.close_hold_frames()` ticks while the
        // render layer fades out the panel, then transition to
        // `Deactivate`. The retail dispatcher's `_DAT_801f0204 = 0` is
        // immediate; the visible fade lives in the panel renderer, which
        // we model here as an SM hold.
        Some(MenuState::Closing) if ctx.frame >= host.close_hold_frames() => {
            ctx.state = MenuState::Deactivate.as_byte();
            ctx.frame = 0;
        }
        Some(MenuState::Closing) => {}
        // Deactivate: one tick to release the menu, then back to
        // `Closed`. Mirrors the dispatcher's `_DAT_801F0204 = 0; return`
        // tail.
        Some(MenuState::Deactivate) => {
            ctx.state = MenuState::Closed.as_byte();
            ctx.frame = 0;
            ctx.cursor = 0;
            ctx.selected_slot = 0;
        }
        // Transient screens: fire a one-shot side effect on entry, hold for
        // the render layer's fade, then auto-advance forward. Input ignored.
        Some(s) if is_transient(s) => {
            if ctx.frame == 1 {
                host.commit(s, ctx.selected_slot);
            }
            if ctx.frame >= host.transient_hold_frames(s).max(1) {
                let next = commit_route(s, ctx.selected_slot).unwrap_or(MenuState::Closing);
                ctx.state = next.as_byte();
                ctx.frame = 0;
                ctx.cursor = 0;
            }
        }
        // Interactive screens: advance cursor on d-pad, commit on Cross
        // (then route forward), back up on Triangle. The `screen_item_count`
        // hook clamps wrap-around per-screen.
        Some(s) => {
            let count = host.screen_item_count(s).max(1);
            if input.up {
                // Widen the wrap arithmetic: `cursor + count` can exceed 255
                // (e.g. cursor 254, count 255) and overflow the u8 before the
                // `% count`, panicking in debug builds.
                ctx.cursor = ((ctx.cursor as u16 + count as u16 - 1) % count as u16) as u8;
            } else if input.down {
                ctx.cursor = (ctx.cursor + 1) % count;
            }
            if input.cross {
                ctx.selected_slot = ctx.cursor;
                host.commit(s, ctx.cursor);
                if let Some(next) = commit_route(s, ctx.cursor) {
                    ctx.state = next.as_byte();
                    ctx.frame = 0;
                    ctx.cursor = 0;
                }
            } else if input.triangle {
                let next = back_route(s);
                if next == MenuState::Closing {
                    host.cancel();
                }
                ctx.state = next.as_byte();
                ctx.frame = 0;
                ctx.cursor = 0;
            }
        }
        // Unknown state byte - fall through, leave ctx untouched.
        None => {}
    }
    ctx.state
}

/// Convenience: open the menu by setting the entry state. Engines call
/// this when the field VM raises a "menu requested" event.
pub fn open(ctx: &mut MenuCtx) {
    ctx.state = MenuState::Idle.as_byte();
    ctx.frame = 0;
    ctx.cursor = 0;
    ctx.selected_slot = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test host that records `commit` / `cancel` calls.
    #[derive(Default)]
    struct H {
        commits: Vec<(u8, u8)>,
        cancels: usize,
        item_counts: std::collections::HashMap<u8, u8>,
    }

    impl MenuHost for H {
        fn screen_item_count(&self, state: MenuState) -> u8 {
            self.item_counts.get(&state.as_byte()).copied().unwrap_or(1)
        }
        fn commit(&mut self, state: MenuState, slot: u8) {
            self.commits.push((state.as_byte(), slot));
        }
        fn cancel(&mut self) {
            self.cancels += 1;
        }
    }

    #[test]
    fn open_seeds_idle_state() {
        let mut ctx = MenuCtx::default();
        open(&mut ctx);
        assert_eq!(ctx.state, MenuState::Idle.as_byte());
    }

    #[test]
    fn idle_transitions_to_status_top_on_first_tick() {
        let mut ctx = MenuCtx::default();
        open(&mut ctx);
        let mut h = H::default();
        step(&mut h, &mut ctx, MenuInput::default());
        assert_eq!(ctx.state, MenuState::StatusTop.as_byte());
        assert_eq!(ctx.frame, 0);
    }

    #[test]
    fn cursor_wraps_within_screen_item_count() {
        let mut ctx = MenuCtx {
            state: MenuState::StatusTop.as_byte(),
            cursor: 0,
            frame: 0,
            selected_slot: 0,
        };
        let mut h = H::default();
        h.item_counts.insert(MenuState::StatusTop.as_byte(), 3);
        // Down 4 times → wraps back to 1.
        for _ in 0..4 {
            step(
                &mut h,
                &mut ctx,
                MenuInput {
                    down: true,
                    ..Default::default()
                },
            );
        }
        assert_eq!(ctx.cursor, 1);
    }

    #[test]
    fn cross_commits_with_current_cursor() {
        let mut ctx = MenuCtx {
            state: MenuState::SavePickSlot.as_byte(),
            cursor: 2,
            frame: 0,
            selected_slot: 0,
        };
        let mut h = H::default();
        h.item_counts.insert(MenuState::SavePickSlot.as_byte(), 4);
        step(
            &mut h,
            &mut ctx,
            MenuInput {
                cross: true,
                ..Default::default()
            },
        );
        assert_eq!(h.commits, vec![(MenuState::SavePickSlot.as_byte(), 2)]);
        assert_eq!(ctx.selected_slot, 2);
    }

    #[test]
    fn triangle_backs_status_subscreen_to_status_top() {
        let mut ctx = MenuCtx {
            state: MenuState::StatusInventory.as_byte(),
            ..Default::default()
        };
        let mut h = H::default();
        // Triangle from a status sub-screen backs up to the status top-level,
        // it does not close the menu (so no `cancel`).
        step(
            &mut h,
            &mut ctx,
            MenuInput {
                triangle: true,
                ..Default::default()
            },
        );
        assert_eq!(ctx.state, MenuState::StatusTop.as_byte());
        assert_eq!(h.cancels, 0);
    }

    #[test]
    fn triangle_from_status_top_closes_then_deactivates_then_closed() {
        let mut ctx = MenuCtx {
            state: MenuState::StatusTop.as_byte(),
            ..Default::default()
        };
        let mut h = H::default();
        // Triangle from the top-level closes the menu (and fires cancel).
        step(
            &mut h,
            &mut ctx,
            MenuInput {
                triangle: true,
                ..Default::default()
            },
        );
        assert_eq!(ctx.state, MenuState::Closing.as_byte());
        assert_eq!(h.cancels, 1);
        // Tick 16 idle frames - should switch to Deactivate.
        for _ in 0..0x10 {
            step(&mut h, &mut ctx, MenuInput::default());
        }
        assert_eq!(ctx.state, MenuState::Deactivate.as_byte());
        // One more tick - closes.
        step(&mut h, &mut ctx, MenuInput::default());
        assert_eq!(ctx.state, MenuState::Closed.as_byte());
    }

    #[test]
    fn shop_buy_flow_routes_browse_quantity_confirm_back_to_list() {
        // ShopBuy -> (Cross) ShopQuantity -> (Cross) ShopConfirm -> (Cross)
        // back to ShopBuy, with a commit recorded at each interactive step.
        let mut ctx = MenuCtx {
            state: MenuState::ShopBuy.as_byte(),
            ..Default::default()
        };
        let mut h = H::default();
        h.item_counts.insert(MenuState::ShopBuy.as_byte(), 4);
        h.item_counts.insert(MenuState::ShopQuantity.as_byte(), 9);
        h.item_counts.insert(MenuState::ShopConfirm.as_byte(), 2);

        let cross = MenuInput {
            cross: true,
            ..Default::default()
        };
        step(&mut h, &mut ctx, cross);
        assert_eq!(ctx.state, MenuState::ShopQuantity.as_byte());
        step(&mut h, &mut ctx, cross);
        assert_eq!(ctx.state, MenuState::ShopConfirm.as_byte());
        step(&mut h, &mut ctx, cross);
        assert_eq!(ctx.state, MenuState::ShopBuy.as_byte());
        assert_eq!(
            h.commits,
            vec![
                (MenuState::ShopBuy.as_byte(), 0),
                (MenuState::ShopQuantity.as_byte(), 0),
                (MenuState::ShopConfirm.as_byte(), 0),
            ]
        );
    }

    #[test]
    fn shop_triangle_backs_one_screen_then_tears_down_via_exit() {
        // From ShopConfirm: Triangle backs to ShopQuantity, then ShopBuy,
        // then the ShopExit teardown screen, which auto-advances to Closing
        // after firing its one-shot commit.
        let mut ctx = MenuCtx {
            state: MenuState::ShopConfirm.as_byte(),
            ..Default::default()
        };
        let mut h = H::default();
        let tri = MenuInput {
            triangle: true,
            ..Default::default()
        };
        step(&mut h, &mut ctx, tri);
        assert_eq!(ctx.state, MenuState::ShopQuantity.as_byte());
        step(&mut h, &mut ctx, tri);
        assert_eq!(ctx.state, MenuState::ShopBuy.as_byte());
        step(&mut h, &mut ctx, tri);
        assert_eq!(ctx.state, MenuState::ShopExit.as_byte());
        // ShopExit is transient: fires its one-shot commit on entry, holds,
        // then routes to Closing.
        step(&mut h, &mut ctx, MenuInput::default());
        assert_eq!(h.commits, vec![(MenuState::ShopExit.as_byte(), 0)]);
        for _ in 0..8 {
            step(&mut h, &mut ctx, MenuInput::default());
        }
        assert_eq!(ctx.state, MenuState::Closing.as_byte());
    }

    #[test]
    fn inn_yes_routes_through_sleep_fade_then_closes() {
        let mut ctx = MenuCtx {
            state: MenuState::InnConfirm.as_byte(),
            ..Default::default()
        };
        let mut h = H::default();
        h.item_counts.insert(MenuState::InnConfirm.as_byte(), 2);
        // Cursor on slot 0 (yes) -> InnSleep fade.
        step(
            &mut h,
            &mut ctx,
            MenuInput {
                cross: true,
                ..Default::default()
            },
        );
        assert_eq!(ctx.state, MenuState::InnSleep.as_byte());
        assert_eq!(h.commits, vec![(MenuState::InnConfirm.as_byte(), 0)]);
        // Sleep fade holds, then closes.
        for _ in 0..8 {
            step(&mut h, &mut ctx, MenuInput::default());
        }
        assert_eq!(ctx.state, MenuState::Closing.as_byte());
    }

    #[test]
    fn inn_no_closes_without_sleeping() {
        let mut ctx = MenuCtx {
            state: MenuState::InnConfirm.as_byte(),
            cursor: 1, // slot 1 = no
            ..Default::default()
        };
        let mut h = H::default();
        step(
            &mut h,
            &mut ctx,
            MenuInput {
                cross: true,
                ..Default::default()
            },
        );
        assert_eq!(ctx.state, MenuState::Closing.as_byte());
    }

    #[test]
    fn host_close_hold_frames_overrides_default() {
        // Host overriding `close_hold_frames()` shortens the Closing → Deactivate
        // hold accordingly. Verifies the hook is consulted, not just the default.
        struct FastClose;
        impl MenuHost for FastClose {
            fn close_hold_frames(&self) -> u16 {
                4
            }
        }
        let mut ctx = MenuCtx {
            state: MenuState::StatusTop.as_byte(),
            ..Default::default()
        };
        let mut h = FastClose;
        step(
            &mut h,
            &mut ctx,
            MenuInput {
                triangle: true,
                ..Default::default()
            },
        );
        assert_eq!(ctx.state, MenuState::Closing.as_byte());
        for _ in 0..4 {
            step(&mut h, &mut ctx, MenuInput::default());
        }
        assert_eq!(ctx.state, MenuState::Deactivate.as_byte());
    }

    #[test]
    fn closed_state_does_nothing() {
        let mut ctx = MenuCtx::default();
        let mut h = H::default();
        // Even with input, closed stays closed.
        for _ in 0..10 {
            step(
                &mut h,
                &mut ctx,
                MenuInput {
                    cross: true,
                    ..Default::default()
                },
            );
        }
        assert_eq!(ctx.state, MenuState::Closed.as_byte());
        assert_eq!(h.commits, Vec::<(u8, u8)>::new());
    }
}
