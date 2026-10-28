//! Disc-gated: the two `P1[0]` system-script writes behind the
//! `balden_field_route_walk` / `tunnelc_field_route_walk` entries in the
//! field NPC-state oracle (`parity` `field_npc_state_parity_disc.rs`).
//!
//! - `balden` (Vidna): with `0x1D5` set and `0x1D4` clear, `P1[0]` seats
//!   `P1[62]` at tile `(10, 105)` (`A3 55 0A 69` at `+0x1A8`). That op sits
//!   before the first `0x21` (`+0x21B`), so it belongs to the **install
//!   slice** retail's MAN loader runs (`FUN_8003AEB0` calls `FUN_8003AB2C`
//!   at `0x8003BAD8`, after the spawn loop and the player seat). The
//!   capture holds the actor at `(1344, 13504)`. The engine runs that pass
//!   on the first tick, not inside the load; this test pins the seat on the
//!   first frame and both arms of the flag gate.
//! - `tunnelc` (Fire Path): Xain's `P1[4]` spawn section sets `0x360` and
//!   writes `80` to script counter `1` (`53 60`, `4C CA 01 50 00`); the
//!   clear comes from `P1[0]`'s per-frame body (`+0x165` test, the
//!   `79 < slot` compare, `63 60` at `+0x1B0`, then `4C CA 01 00 00`), past
//!   the install pass's `0x21` at `+0x8E` - so a fresh entry holds the flag
//!   set and the first passes clear it. The capture holds the post-clear end
//!   state (`0x360` clear, counter `1` = 0).
//!
//! REF: FUN_8003AEB0, FUN_8003AB2C, FUN_8003A1E4
//!
//! Skip-passes without disc data (CLAUDE.md convention).

use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

fn open_host() -> Option<SceneHost> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return SceneHost::open_extracted(&d).ok();
        }
    }
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    SceneHost::open_disc(PathBuf::from(disc)).ok()
}

/// `P1[0]`'s `A3 55 0A 69` seat: tile `(10, 105)` at the tile centre.
const BALDEN_P62_SEAT: (i16, i16) = (10 * 128 + 0x40, 105 * 128 + 0x40);
const HIDE: i16 = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;

#[test]
fn balden_entry_script_seats_p1_62_on_the_first_frame() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(mut host) = open_host() else {
        eprintln!("[skip] no extracted/ tree and disc open failed");
        return;
    };
    host.world.system_flag_set(0x1D5);
    host.world.system_flag_clear(0x1D4);
    host.enter_field_scene("balden", 0).expect("enter balden");
    let _ = host.world.tick();
    assert_eq!(
        host.world.npcs.positions.get(&62).copied(),
        Some(BALDEN_P62_SEAT),
        "the install pass's A3 55 0A 69 seats P1[62] (retail \
         balden_field_route_walk holds it at (1344, 13504))"
    );

    // The other arm: with 0x1D5 clear the seat is skipped and the actor
    // keeps its header park.
    let Some(mut host) = open_host() else {
        return;
    };
    host.world.system_flag_clear(0x1D5);
    host.enter_field_scene("balden", 0).expect("enter balden");
    let _ = host.world.tick();
    assert_eq!(
        host.world.npcs.positions.get(&62).copied(),
        Some((HIDE, HIDE)),
        "with 0x1D5 clear P1[0] jumps past the seat and P1[62] stays parked"
    );
}

#[test]
fn tunnelc_hammer_tremor_flag_is_set_at_load_and_cleared_by_the_first_passes() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(mut host) = open_host() else {
        eprintln!("[skip] no extracted/ tree and disc open failed");
        return;
    };
    host.enter_field_scene("tunnelc", 0).expect("enter tunnelc");
    assert!(
        host.world.system_flag_test(0x360),
        "Xain's spawn section sets 0x360 during the load"
    );
    assert_eq!(
        host.world.field_vm.slot_table[1], 80,
        "Xain's spawn section writes 80 to counter 1 (4C CA 01 50 00)"
    );
    let mut cleared_at = None;
    for t in 0..30 {
        let _ = host.world.tick();
        if !host.world.system_flag_test(0x360) {
            cleared_at = Some(t);
            break;
        }
    }
    eprintln!("[tunnelc] 0x360 cleared on tick {cleared_at:?}");
    assert!(
        cleared_at.is_some(),
        "P1[0]'s per-frame body clears 0x360 once 79 < counter 1"
    );
    assert_eq!(
        host.world.field_vm.slot_table[1], 0,
        "the clear arm zeroes counter 1 (4C CA 01 00 00), the retail capture's value"
    );
}
