//! The target-cursor tint pass.

use super::*;

// --- target_cursor (FUN_801da6b4) -------------------------------------------

fn cursor_host() -> (RecHost, BattleActionCtx) {
    let mut host = RecHost::with_n_actors(ACTOR_SLOTS);
    for a in &mut host.actors {
        a.liveness = 100;
    }
    let mut ctx = BattleActionCtx::new();
    ctx.active_actor = 0;
    // Acting actor points at monster slot 4.
    host.actors[0].active_target = 4;
    (host, ctx)
}

#[test]
fn target_cursor_brightens_pointed_at_and_dims_rest() {
    let (mut host, ctx) = cursor_host();
    target_cursor_highlight(&mut host, &ctx, true);

    assert_eq!(host.actors[4].render_flag, CURSOR_FLAG_SELECTED);
    assert_eq!(host.actors[4].render_color, CURSOR_COLOR_BRIGHT);
    assert_eq!(host.actors[4].render_blend, CURSOR_BLEND_ON);

    for slot in [3usize, 5, 6] {
        assert_eq!(host.actors[slot].render_flag, CURSOR_FLAG_DIMMED);
        assert_eq!(host.actors[slot].render_color, CURSOR_COLOR_DIM);
        assert_eq!(host.actors[slot].render_blend, CURSOR_BLEND_ON);
    }
}

#[test]
fn target_cursor_window_is_slots_three_through_six() {
    let (mut host, ctx) = cursor_host();
    target_cursor_highlight(&mut host, &ctx, true);
    for slot in [0usize, 1, 2, 7] {
        assert_eq!(host.actors[slot].render_flag, 0);
        assert_eq!(host.actors[slot].render_blend, 0);
    }
}

#[test]
fn target_cursor_skips_dead_slots() {
    let (mut host, ctx) = cursor_host();
    host.actors[5].liveness = 0;
    host.actors[5].render_flag = 42;
    target_cursor_highlight(&mut host, &ctx, true);
    assert_eq!(host.actors[5].render_flag, 42);
    assert_eq!(host.actors[5].render_blend, 0);
}

#[test]
fn target_cursor_disable_clears_tint() {
    let (mut host, ctx) = cursor_host();
    target_cursor_highlight(&mut host, &ctx, true);
    target_cursor_highlight(&mut host, &ctx, false);
    for slot in 3usize..=6 {
        assert_eq!(host.actors[slot].render_flag, 0);
        assert_eq!(host.actors[slot].render_blend, 0);
        assert_eq!(host.actors[slot].render_color, CURSOR_COLOR_BRIGHT);
    }
}
