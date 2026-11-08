//! Disc-gated: the conc3 inn-stay arrival record behind the
//! `conc3_field_inn_night` entries in the field NPC-state oracle
//! (`parity` `field_npc_state_parity_disc.rs`).
//!
//! The conc innkeeper (`conc` `P1[9]`) sets `0x53C` and changes scene to
//! `CONC3` (`55 3C`, `3F DD 02 05 "CONC3" 0F 60` at `+0x2B0` / `+0x2C0`).
//! conc3's entry script `P1[0]` spawns `P2[9]` (global record `0x59`,
//! `44 59` at `+0x90`) only while `0x53C` is set and `0x5F7` is clear
//! (`75 3C` at `+0x85`, `75 F7` at `+0x8C`). `P2[9]` is the wake-up
//! cutscene: after its fade it parks Dr. Usha, the innkeeper, the
//! innkeeper's counter relay and the doctor's companion
//! (`A3 2A 7F 7F`, `A3 29 7F 7F`, `A3 39 7F 7F`, `A3 2B 7F 7F` at
//! `+0xAF..+0xBB` - `P1[8]`, `P1[7]`, `P1[23]`, `P1[9]`), sets `0x3F9`,
//! shows its "Wake up" page, parks three of them again, sets `0x5F7` and
//! idles. With `0x5F7` set the spawn arm is skipped and the four stand at
//! their authored seats.
//!
//! REF: FUN_801DE840 (op 0x44 spawn, op 0x70 test)
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
/// `P1[7]` innkeeper, `P1[8]` Dr. Usha, `P1[9]` companion, `P1[23]` relay.
const PARKED_BY_P2_9: [u8; 4] = [7, 8, 9, 23];

fn parked(host: &SceneHost, pi: u8) -> bool {
    host.world.npcs.positions.get(&pi).copied() == Some((HIDE, HIDE))
}

#[test]
fn conc3_inn_arrival_record_parks_the_inn_cohort() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(mut host) = open_host() else {
        eprintln!("[skip] no extracted/ tree and disc open failed");
        return;
    };
    // The inn stay: 0x53C set by the conc innkeeper, P2[9] not yet run.
    host.world.system_flag_set(0x53C);
    host.world.system_flag_clear(0x5F7);
    host.world.system_flag_clear(0x3F9);
    host.enter_field_scene("conc3", 0).expect("enter conc3");
    for pi in PARKED_BY_P2_9 {
        assert!(
            !parked(&host, pi),
            "P1[{pi}] stands at its authored seat before P2[9] runs"
        );
    }
    let mut parked_at = None;
    for t in 0..600 {
        let _ = host.tick();
        if PARKED_BY_P2_9.iter().all(|&pi| parked(&host, pi)) {
            parked_at = Some(t);
            break;
        }
    }
    eprintln!(
        "[conc3] inn cohort parked on tick {parked_at:?}, 0x3F9 = {}",
        host.world.system_flag_test(0x3F9)
    );
    assert!(
        parked_at.is_some(),
        "P2[9]'s A3 xx 7F 7F parks P1[7] / P1[8] / P1[9] / P1[23] after its fade"
    );
    assert!(
        host.world.system_flag_test(0x3F9),
        "P2[9] sets 0x3F9 straight after the parks (53 F9 at +0xCB)"
    );

    // The other arm: with 0x5F7 already set (P2[9] has run) the spawn is
    // skipped and the cohort keeps its seats - the arrangement a cold entry
    // against the post-stay bank builds.
    let Some(mut host) = open_host() else {
        return;
    };
    host.world.system_flag_set(0x53C);
    host.world.system_flag_set(0x5F7);
    host.enter_field_scene("conc3", 0).expect("enter conc3");
    for _ in 0..300 {
        let _ = host.tick();
    }
    for pi in PARKED_BY_P2_9 {
        assert!(
            !parked(&host, pi),
            "with 0x5F7 set P1[0] skips the 44 59 spawn and P1[{pi}] keeps its seat"
        );
    }
}
