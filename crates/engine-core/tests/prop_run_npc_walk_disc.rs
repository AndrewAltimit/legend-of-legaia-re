//! A prop record that walks another actor runs on past the leg.
//!
//! `jiji`'s inn counter (P0[5]) opens on a box test of the player, walks
//! its keeper (channel `0x0C`) to the counter with `C7 0C 58 1E 32`, and
//! then pokes the keeper (`B2 0C 18`, `A2 0C 2D`, ...) before the greeting.
//! In retail the keeper's walk kernel clears the leg's park bit on its
//! terminal frame (`FUN_8003774C`, the `0x47` arm) and the record's next
//! op on the keeper runs then. The prop runner steps the leg on the
//! record's own context, which stands in for the keeper, and plays no leg:
//! the park bit stayed up, the dispatcher's halted-target early-out held
//! the record on `B2 0C 18` until the park timeout, and the greeting never
//! opened. Every touch then froze the player for the timeout.
//!
//! Disc-gated: skips and passes without `LEGAIA_DISC_BIN`.

use legaia_engine_core::scene_host::SceneHost;
use std::path::PathBuf;

fn disc_path() -> Option<PathBuf> {
    let path = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)?;
    path.is_file().then_some(path)
}

/// The counter's opening box test on the player: `CD F8 58 1B 59 1C`,
/// tiles (88..89, 27..28).
const COUNTER_BOX: [u8; 6] = [0xCD, 0xF8, 0x58, 0x1B, 0x59, 0x1C];

#[test]
fn the_inn_counter_greets_after_walking_its_keeper() {
    let Some(path) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    let mut host = SceneHost::open_disc(&path).expect("open disc");
    host.enter_field_scene("jiji", 0).expect("enter jiji");
    let mut anchors: Vec<(u8, u8)> = host
        .world
        .props
        .bank
        .props
        .iter()
        .filter(|(_, p)| {
            p.record_body
                .windows(COUNTER_BOX.len())
                .any(|w| w == COUNTER_BOX)
        })
        .map(|(&a, _)| a)
        .collect();
    anchors.sort_unstable();
    let anchor = *anchors.first().expect("jiji binds the inn counter");
    // The player stands in the counter's box, as one who walked up to it.
    host.world.seat_player_at_tile(88, 28);
    assert!(
        host.world.start_prop_interaction(anchor),
        "the counter's record starts"
    );
    // The keeper's walk and the turns before the greeting take well under
    // two seconds; the park timeout the stalled run fell to is thirty.
    let mut greeted = false;
    for _ in 0..240 {
        let _ = host.world.tick();
        if host
            .world
            .dialog
            .inline
            .as_ref()
            .is_some_and(|id| id.prop_anchor == Some(anchor) && id.panel.is_some())
        {
            greeted = true;
            break;
        }
    }
    let pc = host.world.dialog.inline.as_ref().map(|id| id.pc);
    assert!(
        greeted,
        "the greeting's box opened within 240 ticks (run parked at {pc:?})"
    );
}
