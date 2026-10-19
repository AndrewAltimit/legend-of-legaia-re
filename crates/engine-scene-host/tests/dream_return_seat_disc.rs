//! Disc-gated oracle for the **inn dream's return seat**.
//!
//! An inn that plays a dream (`retock` P1[26]: `55 88` then `3F .. DREAM
//! 40 40`) hands the party back through `dream` P2[5], whose `0x3F` to the
//! inn's scene carries the off-map entry tile `(0x7F, 0x7F)`. The
//! destination's entry script is what seats the party: `retock` P1[0] tests
//! `0x588`, sets `0x57F`, clears `0x588` and runs `A3 F8 F4 8B` - the
//! player to the inn bed, tile `(0x74, 0x0B)` with both far-half bits set,
//! world `(14976, 1536)`. The arrival must leave the player there, not on
//! the op's off-map tile. Skips without `extracted/` + `LEGAIA_DISC_BIN`.

use legaia_engine_scene_host::scene_host::{SceneHost, SceneTickEvent};
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    ["extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

fn player_xz(host: &SceneHost) -> (i16, i16) {
    let slot = host.world.player_actor_slot.expect("player installed") as usize;
    let ms = &host.world.actors[slot].move_state;
    (ms.world_x, ms.world_z)
}

#[test]
fn dream_return_lands_on_the_inn_bed() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("dream", 0).expect("enter dream");
    host.world.system_flag_set(0x588);
    host.world.pending_named_scene_transition = Some(("retock".to_string(), 0x7F, 0x7F, 0));
    let mut entered = false;
    for _ in 0..200 {
        match host.tick().expect("transition tick") {
            SceneTickEvent::SceneEntered { name } => {
                assert_eq!(name, "retock");
                entered = true;
                break;
            }
            SceneTickEvent::Stepped => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(entered, "the warp to retock never committed");
    for _ in 0..120 {
        host.tick().expect("arrival tick");
    }
    eprintln!(
        "player at {:?}, 0x588 {} 0x57F {}",
        player_xz(&host),
        host.world.system_flag_test(0x588),
        host.world.system_flag_test(0x57F)
    );
    assert!(
        !host.world.system_flag_test(0x588),
        "retock P1[0] clears the dream latch"
    );
    assert_eq!(
        player_xz(&host),
        (14976, 1536),
        "the entry script's A3 F8 F4 8B seats the player on the inn bed"
    );
}
