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
//! The full per-state body is not yet ported - this module establishes
//! the typed surface (state enum + host trait + step entry point) so
//! engine code can wire menu transitions without reaching into the
//! overlay's RAM scratchpad bytes directly. Per-state handler bodies
//! land in follow-up commits as each menu screen's behaviour is
//! reverse-engineered.
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
}

/// One frame of menu execution. Reads `ctx.state`, applies `input`,
/// possibly mutates `ctx`, possibly calls hooks on `host`. Returns the
/// post-step state byte for engines that want to drive UI off it.
///
/// The body is intentionally minimal in this first pass - it covers the
/// open / close transitions cleanly (the most common path) and folds
/// every per-screen state into a single "advance the cursor on input,
/// commit on Cross, cancel on Triangle" handler. Per-screen specifics
/// (shop quantity entry, save-slot prompts, etc.) land in follow-ups.
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
        // Per-screen: advance cursor on d-pad, commit on Cross, cancel
        // on Triangle. The `screen_item_count` hook clamps wrap-around
        // per-screen.
        Some(s) => {
            let count = host.screen_item_count(s).max(1);
            if input.up {
                ctx.cursor = (ctx.cursor + count - 1) % count;
            } else if input.down {
                ctx.cursor = (ctx.cursor + 1) % count;
            }
            if input.cross {
                ctx.selected_slot = ctx.cursor;
                host.commit(s, ctx.cursor);
            }
            if input.triangle {
                host.cancel();
                ctx.state = MenuState::Closing.as_byte();
                ctx.frame = 0;
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
    fn triangle_routes_to_closing_then_deactivate_then_closed() {
        let mut ctx = MenuCtx {
            state: MenuState::StatusInventory.as_byte(),
            ..Default::default()
        };
        let mut h = H::default();
        // Press triangle once - should switch to Closing.
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
