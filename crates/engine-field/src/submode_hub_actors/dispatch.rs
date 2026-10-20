//! The state dispatcher and the menu-flow handlers it indexes: start menu, submenu, prompt, draw, close and deactivate.

use super::*;

// ---------------------------------------------------------------------------
// dispatcher
// ---------------------------------------------------------------------------

/// PORT: FUN_801f159c - the hub system-actor dispatcher.
///
/// Active only while the submode gate `DAT_801F2734` is one of
/// [`ACTIVE_SUBMODES`]. It forces the text layer byte `DAT_80073F20` to `0x0C`,
/// re-arms the submode through `FUN_801F1278` unless the actor's own gate
/// `+0x1A` is set (and retires the actor outright when the pad latch's
/// [`PAD_LATCH_SUSPEND`] bit is up), runs the `+0x50` handler, and then - once
/// the grid actor's completion gate `+0x3E` reads `0` - retires the actor,
/// releases the pad latch and drops the board busy state.
///
/// `handler` is the caller's view of `PTR_FUN_801F33B4[actor.state]`. It takes
/// the cursor context mutably because retail re-reads `+0x3E` **after** the
/// `jalr`, so a handler that clears the completion gate this frame is retired
/// this frame - which is exactly how [`draw_tick`] closes a screen.
pub fn hub_dispatch(
    actor: &mut HubActor,
    env: &HubEnv,
    grid: &mut HubGrid,
    handler: impl FnOnce(&mut HubActor, &mut HubGrid) -> HubFrame,
) -> HubFrame {
    let mut out = HubFrame::default();
    if !ACTIVE_SUBMODES.contains(&env.submode) {
        return out;
    }
    // `DAT_80073F20 = 0x0C` is a plain store with no engine-side reader.
    if actor.gate == 0 {
        if env.pad_latch & PAD_LATCH_SUSPEND != 0 {
            actor.flags |= ACTOR_RETIRE;
            return out;
        }
        out.act(HubAction::RearmSubmode);
    }
    let inner = handler(actor, grid);
    out.draws.extend(inner.draws);
    out.actions.extend(inner.actions);
    if grid.done_gate == 0 {
        actor.flags |= ACTOR_RETIRE;
        out.act(HubAction::ReleasePadLatch);
        if env.board_flag == 0 {
            out.act(HubAction::ClearBusyBit);
        } else {
            out.act(HubAction::SetBoardFlag);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// panel state machines
// ---------------------------------------------------------------------------

/// Panel descriptor VAs the state machines install through `FUN_801E9B3C`.
pub const PANEL_START: u32 = 0x801F_3370;

pub const PANEL_SUBMENU_IDLE: u32 = 0x801F_3294;

pub const PANEL_SUBMENU_CONFIRM: u32 = 0x801F_32A4;

pub const PANEL_PROMPT: u32 = 0x801F_3388;

pub const PANEL_DRAW_TICK: u32 = 0x801F_2A88;

/// Whether a confirm press lands this frame: the suppression flag must be
/// clear and the edge-triggered pad must intersect the confirm mask.
fn confirm_pressed(env: &HubEnv) -> bool {
    env.input_blocked == 0 && env.pad_edge & env.confirm_mask != 0
}

/// Hand the actor back to the hub: stash `+0x50` into the grid actor and
/// re-arm to [`HUB_RETURN_STATE`].
pub(super) fn hand_back(actor: &mut HubActor, grid: &mut HubGrid) {
    grid.handback = -1;
    grid.stashed_state = actor.state;
    actor.state = HUB_RETURN_STATE;
    actor.sub = 0;
}

/// PORT: FUN_801f1138 - the start / confirm menu tick.
///
/// State `0` counts the active entries in the three bytes at
/// `_DAT_8007B450 + 1..=3` (one plus however many are non-zero), sizes the
/// panel to `n * 14 - 2` pixels tall, centres it by writing the top edge as
/// `0x2C - (height >> 1)` (an arithmetic shift on the 16-bit height), installs
/// the panel and advances. State `1` waits for a confirm, plays
/// [`CUE_CONFIRM`] and hands the actor back.
pub fn start_menu(actor: &mut HubActor, env: &HubEnv, grid: &mut HubGrid) -> HubFrame {
    let mut out = HubFrame::default();
    match actor.sub {
        0 => {
            let mut n: i16 = 1;
            if env.board_entries[0] != 0 {
                n = 2;
            }
            if env.board_entries[1] != 0 {
                n += 1;
            }
            if env.board_entries[2] != 0 {
                n += 1;
            }
            let height = n.wrapping_mul(14).wrapping_sub(2);
            out.act(HubAction::SizePanel {
                height,
                top: 0x2C - (height >> 1),
            });
            out.act(HubAction::InstallPanel(PANEL_START));
            actor.sub = actor.sub.wrapping_add(1);
        }
        1 if confirm_pressed(env) => {
            out.act(HubAction::ConfirmCue(CUE_CONFIRM));
            hand_back(actor, grid);
        }
        _ => {}
    }
    out.act(HubAction::DrawPump);
    out
}

/// PORT: FUN_801f1e48 - the Incense wear-off notice.
///
/// Three states: `0` clears the cursor row and shows window record `16`
/// (whose painter [`single_label`] draws the field overlay's one line at
/// [`STR_SINGLE`] - the Incense's name and that its effect is gone), `1`
/// waits for a confirm and hides it, `2` clears the board flag and hands the
/// actor back. The walk tick `FUN_801D0B90` reaches it: its zero edge stores
/// the kind-`0x0B` record `0x801F2278` as the entry context, and the enter
/// half maps kind `0x0B` to this slot (`engine-core::incense_notice`). Also
/// op-`0x49` sub-op `0xB`'s screen.
pub fn submenu(actor: &mut HubActor, env: &HubEnv, grid: &mut HubGrid) -> HubFrame {
    let mut out = HubFrame::default();
    match actor.sub {
        0 => {
            out.act(HubAction::ClearCursorRow);
            out.act(HubAction::InstallPanel(PANEL_SUBMENU_IDLE));
            actor.sub = actor.sub.wrapping_add(1);
        }
        1 if confirm_pressed(env) => {
            out.act(HubAction::ConfirmCue(CUE_CONFIRM));
            out.act(HubAction::InstallPanel(PANEL_SUBMENU_CONFIRM));
            actor.sub = 2;
        }
        2 => {
            out.act(HubAction::ClearBoardFlag);
            hand_back(actor, grid);
        }
        _ => {}
    }
    out.act(HubAction::DrawPump);
    out
}

/// PORT: FUN_801f1fdc - the hub prompt state machine.
///
/// State `0` plays the entry sting [`CUE_ENTRY`] and installs the prompt
/// panel; state `1` waits for a confirm, plays [`CUE_CONFIRM`] and hands the
/// actor back. Unlike [`submenu`] it never clears the board flag.
pub fn hub_prompt(actor: &mut HubActor, env: &HubEnv, grid: &mut HubGrid) -> HubFrame {
    let mut out = HubFrame::default();
    match actor.sub {
        0 => {
            out.act(HubAction::EntryCue(CUE_ENTRY));
            out.act(HubAction::InstallPanel(PANEL_PROMPT));
            actor.sub = actor.sub.wrapping_add(1);
        }
        1 if confirm_pressed(env) => {
            out.act(HubAction::ConfirmCue(CUE_CONFIRM));
            hand_back(actor, grid);
        }
        _ => {}
    }
    out.act(HubAction::DrawPump);
    out
}

/// PORT: FUN_801f20b0 - the panel-install draw tick.
///
/// State `0` installs [`PANEL_DRAW_TICK`] and advances; any state above `1`
/// returns before the pump. Otherwise it pumps and, while the suppression
/// flag is clear, clears the grid actor's completion gate - which is what
/// lets [`hub_dispatch`] retire the actor on the following frame.
pub fn draw_tick(actor: &mut HubActor, env: &HubEnv, grid: &mut HubGrid) -> HubFrame {
    let mut out = HubFrame::default();
    match actor.sub {
        0 => {
            out.act(HubAction::InstallPanel(PANEL_DRAW_TICK));
            actor.sub = actor.sub.wrapping_add(1);
        }
        1 => {}
        _ => return out,
    }
    out.act(HubAction::DrawPump);
    if env.input_blocked == 0 {
        grid.done_gate = 0;
    }
    out
}

/// PORT: FUN_801f2134 - the close tick.
///
/// [`draw_tick`]'s twin: identical but for state `0`, which closes every
/// panel (`FUN_80035A4C`) instead of installing one.
///
/// Two hosts run it: `legaia_engine_core::field_submode_screen` (slot `0`
/// and `0x14..=0x18`) and `legaia_engine_world_views::world_map_panel_host`, which
/// follows the fill fade's case-4 hand-back to handler id `0` into it.
pub fn close_tick(actor: &mut HubActor, env: &HubEnv, grid: &mut HubGrid) -> HubFrame {
    let mut out = HubFrame::default();
    match actor.sub {
        0 => {
            out.act(HubAction::CloseAllPanels);
            actor.sub = actor.sub.wrapping_add(1);
        }
        1 => {}
        _ => return out,
    }
    out.act(HubAction::DrawPump);
    if env.input_blocked == 0 {
        grid.done_gate = 0;
    }
    out
}

/// PORT: FUN_801f1d90 - the actor deactivate with a chosen re-arm state.
///
/// Closes every panel, pumps, then hands the actor back with `+0x50` set
/// to [`HUB_SKIP_STATE`] when the first progress flag is set or the second is
/// set *and* the cancel button is held, and to [`HUB_DEACTIVATE_STATE`]
/// otherwise. The two arms are otherwise identical: retail writes the grid
/// hand-back fields in both.
pub fn deactivate(actor: &mut HubActor, env: &HubEnv, grid: &mut HubGrid) -> HubFrame {
    let mut out = HubFrame::default();
    out.act(HubAction::CloseAllPanels);
    out.act(HubAction::DrawPump);
    grid.handback = -1;
    grid.stashed_state = actor.state;
    let skip = env.progress_a != 0 || (env.progress_b != 0 && env.pad_held & env.cancel_mask != 0);
    actor.state = if skip {
        HUB_SKIP_STATE
    } else {
        HUB_DEACTIVATE_STATE
    };
    actor.sub = 0;
    out
}
