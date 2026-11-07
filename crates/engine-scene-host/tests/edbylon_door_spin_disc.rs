//! Disc-gated: `edbylon`'s credits record holds its shot on the dojo door.
//!
//! `edbylon` `P2[19]` swings placed object 4, the dojo door, the way a door
//! record swings its own (`AC 04 07`, `AC 04 01` drops the hold, `AB 04 03`
//! raises the clamp, `B1 04 00` at `+0x28A..+0x293`), clears the door's end
//! latch (`AC 04 08`) and spins on it (`AD 04 08` at `+0x2A0`) before the
//! pull-back glides. LFLAG_TST halts while `+0x62 & 0x100` is clear, and the
//! door's clip driver latches it at the end of the swing - 30 clip frames at
//! two vsyncs each. Stepping past the spin cut the door shot and ran the
//! vignette about 60 vsyncs short of the retail film.
//!
//! REF: FUN_801DE840 (op 0x2D), FUN_800204F8 (0x800206E4..0x8002072C)
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

/// `P2[19]`'s `AD 04 08` (record-relative PC).
const DOOR_SPIN_PC: usize = 0x2A0;

#[test]
fn edbylon_credits_record_waits_for_the_door_swing() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(mut host) = open_host() else {
        eprintln!("[skip] no extracted/ tree and disc open failed");
        return;
    };
    host.enter_field_scene("edbylon", 0).expect("enter edbylon");
    let (mut at, mut past) = (None, None);
    for t in 0..1500u32 {
        let _ = host.tick();
        let Some(pc) = host
            .world
            .field_vm
            .helper_contexts
            .iter()
            .map(|c| c.pc)
            .max()
        else {
            continue;
        };
        if at.is_none() && pc == DOOR_SPIN_PC {
            at = Some(t);
        }
        if past.is_none() && pc > DOOR_SPIN_PC {
            past = Some(t);
            break;
        }
    }
    eprintln!("[edbylon] door spin from tick {at:?} to {past:?}");
    let (Some(a), Some(p)) = (at, past) else {
        panic!("P2[19] reaches and leaves the door spin (at {at:?}, past {past:?})");
    };
    let held = p - a;
    assert!(
        (50..=70).contains(&held),
        "AD 04 08 holds for the door's 30-frame swing at two vsyncs a frame: held {held} ticks"
    );
}
