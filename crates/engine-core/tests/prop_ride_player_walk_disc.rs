//! Disc-gated: a placed prop's record that **walks the player** carries them.
//!
//! `taiku` partition-0 record 6 is the lift at tile `(109, 78)` in Zora
//! Castle's east corridor. Its touch pass (resumed when the player presses
//! into its box) walks the player onto the platform with a cross-context
//! walk-to-tile `C7 F8 6D 4E 32`, ramps the lift and the player's height,
//! walks them off the far side with `C7 F8 6D CF 32`, and repaints the wall
//! row behind it. Retail parks the calling record on a player-target walk
//! until the walk kernel lands the player (`FUN_801DE840`
//! `0x801DF034..0x801DF044`, `FUN_8003774C` case `0x47`).
//!
//! The pass runs only while `0x38C` is set, and `0x38B` (cleared by the
//! ride up, set by the ride down) picks which way it goes.
//!
//! Stepping that op on a throwaway context, as the prop runner did, left
//! the player where the touch found them, north of the lift, and the corridor
//! south of it - the only way to Zora Castle's west wing after the Zora
//! fight - unreachable.
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` are missing (disc-gated).

use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from)
        && d.join("PROT.DAT").exists()
    {
        return Some(d);
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("extracted");
    if root.join("PROT.DAT").exists() {
        Some(root)
    } else {
        eprintln!("[skip] extracted/ missing");
        None
    }
}

fn player_pos(host: &SceneHost) -> (i16, i16) {
    host.world
        .player_actor_slot
        .and_then(|s| host.world.actors.get(s as usize))
        .map(|a| (a.move_state.world_x, a.move_state.world_z))
        .unwrap_or((0, 0))
}

#[test]
fn taiku_lift_carries_the_player_across() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("taiku", 0).expect("enter taiku");
    for _ in 0..120 {
        let _ = host.tick();
    }
    if host.world.player_actor_slot.is_none() {
        host.world.install_field_player(0);
    }
    let s = host.world.player_actor_slot.expect("player slot") as usize;
    // Tile (109, 76), two tiles north of the lift, facing it.
    host.world.actors[s].move_state.world_x = 109 * 128 + 64;
    host.world.actors[s].move_state.world_z = 76 * 128 + 64;
    let start = player_pos(&host);
    // The touch pass runs the lift only once `0x38C` is up (otherwise it
    // jumps straight to its `21`), and `0x38B` picks the direction: set, as
    // in the Zora Castle card save the full-game ladder seeds from, the ride
    // goes up and leaves the player south of the lift.
    host.world.system_flag_set(0x38C);
    host.world.system_flag_set(0x38B);
    let mut furthest = start.1;
    // Up walks +Z here; the touch runs the ride, which owns the frames until
    // the player is walked off the far side.
    for f in 0..900 {
        host.world
            .set_pad(if f < 60 { PadButton::Up.mask() } else { 0 });
        let _ = host.tick();
        furthest = furthest.max(player_pos(&host).1);
    }
    let end = player_pos(&host);
    eprintln!("[ran] taiku lift: start {start:?} end {end:?} furthest z {furthest}");
    assert_eq!(end.0, start.0, "the ride stays on the lift's column");
    // `C7 F8 6D CF 32`: tile z 0x4F with the half-tile bit, the far edge.
    assert!(
        end.1 >= 79 * 128 + 128,
        "the lift walked the player off its far side (z {} < {})",
        end.1,
        79 * 128 + 128
    );
}
