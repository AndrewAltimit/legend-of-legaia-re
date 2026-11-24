//! Disc-gated: the garmel post-Zeto re-entry record behind the
//! `garmel_zeto_defeat_pre_line` entries in the field NPC-state oracle
//! (`parity` `field_npc_state_parity_disc.rs`).
//!
//! The Zeto stager `P2[12]` enters the boss battle and arms `0x199`
//! (`3E FF 09 51 99`). Back on the field, garmel's entry script `P1[0]`
//! consumes it (`61 99`) and spawns `P2[14]` (`44 41`, partition-2 base
//! `0x33`). `P2[14]` is Zeto's defeat: it seats the player on tile
//! (23, 53) (`CC F8 51 17 35` at `+0x96`), the party channels `0x1B` /
//! `0x1C` (`P1[3]` / `P1[4]`) on tiles (25, 54) / (21, 54)
//! (`CC 1B 51 19 36`, `CC 1C 51 15 36` at `+0x9D` / `+0xA4`) and Zeto
//! (`0x1D`, `P1[5]`) on tile (23, 56) (`CC 1D 51 17 38 00 37` at
//! `+0x161`) before his first line. With `0x199` already clear the spawn
//! arm is skipped and the three keep their `(127, 127)` header parks.
//!
//! REF: FUN_801DE840 (op 0x44 spawn, op 0x61 clear-test)
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

const HIDE: i16 = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;

/// Tile centre in world units (`tile * 128 + 64`).
const fn tile(t: i16) -> i16 {
    t * 128 + 64
}

/// `(P1 index, seat)`: the party pair and Zeto as `P2[14]` seats them.
const SEATS: [(u8, (i16, i16)); 3] = [
    (3, (tile(25), tile(54))),
    (4, (tile(21), tile(54))),
    (5, (tile(23), tile(56))),
];

fn pos(host: &SceneHost, pi: u8) -> Option<(i16, i16)> {
    host.world.npcs.positions.get(&pi).copied()
}

fn enter(flag_199: bool) -> Option<SceneHost> {
    let mut host = open_host()?;
    // Inside the Zeto beat: P2[12]'s latch and its 0x363 are set.
    host.world.system_flag_set(0x198);
    host.world.system_flag_set(0x363);
    if flag_199 {
        host.world.system_flag_set(0x199);
    } else {
        host.world.system_flag_clear(0x199);
    }
    host.set_entry_seat(tile(23), tile(53));
    host.enter_field_scene("garmel", 0).expect("enter garmel");
    Some(host)
}

#[test]
fn garmel_post_zeto_record_seats_the_party_and_zeto() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(mut host) = enter(true) else {
        eprintln!("[skip] no extracted/ tree and disc open failed");
        return;
    };
    let mut seated_at = None;
    for t in 0..900 {
        let _ = host.tick();
        if SEATS.iter().all(|&(pi, seat)| pos(&host, pi) == Some(seat)) {
            seated_at = Some(t);
            break;
        }
    }
    eprintln!(
        "[garmel] P2[14] seats on tick {seated_at:?}: {:?}",
        SEATS.map(|(pi, _)| pos(&host, pi))
    );
    assert!(
        seated_at.is_some(),
        "P2[14] seats P1[3] / P1[4] / P1[5] on tiles (25,54) / (21,54) / (23,56)"
    );
    assert!(
        !host.world.system_flag_test(0x199),
        "P1[0]'s arm consumes 0x199 before spawning P2[14]"
    );

    // The other arm: with 0x199 already consumed the spawn is skipped and
    // the three hold their header parks - the arrangement a cold entry
    // against the mid-beat bank builds.
    let Some(mut host) = enter(false) else {
        return;
    };
    for _ in 0..300 {
        let _ = host.tick();
    }
    // No position override means the actor stands at its MAN placement,
    // which for all three is the `(127, 127)` header park.
    for (pi, _) in SEATS {
        let p = pos(&host, pi);
        assert!(
            p.is_none() || p == Some((HIDE, HIDE)),
            "with 0x199 clear P1[0] skips the 44 41 spawn and P1[{pi}] stays parked, got {p:?}"
        );
    }
}
