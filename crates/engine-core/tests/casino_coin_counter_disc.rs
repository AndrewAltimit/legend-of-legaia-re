//! Disc-gated: the casino's field-side pieces read off the user's disc.
//!
//! - The coin counter (op-`0x49` sub-op 6, handler slot `0x25`): its entry
//!   panel's four labels come off the field overlay (PROT 0897) through
//!   `SceneHost::coin_counter_lines`, the builder both hosts draw.
//! - koin1, the scene whose attendant opens the Muscle Dome door (`3E 69`):
//!   loading it lists the dome hub's two announcer lines for prestage, so a
//!   host that decodes clips asynchronously has them before the hub asks.
//!
//! Only structural facts are asserted - counts, positions and non-emptiness,
//! never text. Skips and passes without `LEGAIA_DISC_BIN`.

use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::slot_machine::{COIN_ENTRY_LABEL_VAS, COIN_ENTRY_ORIGIN};

fn host() -> Option<SceneHost> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return None;
    };
    match SceneHost::open_disc(&disc) {
        Ok(h) => Some(h),
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            None
        }
    }
}

#[test]
fn the_coin_counter_labels_come_off_the_field_overlay() {
    let Some(mut host) = host() else {
        return;
    };
    let world = &mut host.world;
    let _ = world.man_load_actor_reset();
    world.party.money = 1_000;
    world.minigames.casino_coins = 7;
    world.open_coin_counter();
    world.tick_submode_screen(1);
    let lines = host.coin_counter_lines();
    let (x, y) = COIN_ENTRY_ORIGIN;
    let labels: Vec<_> = lines.iter().filter(|l| l.x == x).collect();
    assert_eq!(
        labels.len(),
        COIN_ENTRY_LABEL_VAS.len(),
        "four labels at the panel's left edge"
    );
    assert!(
        labels
            .iter()
            .all(|l| !l.text.is_empty() && l.text.iter().all(|c| (0x20..0x7F).contains(c))),
        "every label reads as printable text off PROT 0897"
    );
    assert_eq!(
        labels.iter().map(|l| l.y - y).collect::<Vec<_>>(),
        vec![2, 0x12, 0x30, 0x40]
    );
    eprintln!(
        "[ok] coin counter: {} lines, labels off the disc",
        lines.len()
    );
}

#[test]
fn koin1_lists_the_dome_hub_lines_on_load() {
    let Some(mut host) = host() else {
        return;
    };
    host.enter_field_scene("koin1", 0).expect("koin1 loads");
    let listed = host.world.drain_field_xa_prestage();
    let hub: Vec<legaia_engine_core::sfx_cue::XaVoiceClip> =
        legaia_engine_core::muscle_ringside::hub_xa_prestage()
            .into_iter()
            .map(Into::into)
            .collect();
    assert!(
        hub.iter().all(|c| listed.contains(c)),
        "koin1 carries the dome door, so its list holds both hub lines: {listed:?}"
    );
    eprintln!(
        "[ok] koin1 prestage: {} clips, both dome lines",
        listed.len()
    );

    // Contrast: a scene with no dome door does not list them.
    host.enter_field_scene("town01", 0).expect("town01 loads");
    let listed = host.world.drain_field_xa_prestage();
    assert!(
        !hub.iter().any(|c| listed.contains(c)),
        "town01 has no dome door: {listed:?}"
    );
}
