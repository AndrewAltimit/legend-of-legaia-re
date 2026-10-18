//! Disc-gated: Snowdrift Cave's naming prompt names Noa, and the player body
//! on screen behind it is Noa's.
//!
//! `cave01`'s entry script (P1[0]) takes its `SysFlag.Test 0x156` arm on the
//! story beat that introduces Noa: `3D 00 3D 01 3D 02 3C 01` empties the party
//! and adds Noa alone, then spawns the cutscene (P2[2]) whose `49 03 01` opens
//! the name entry for character 1. The roster ops run on the first tick,
//! after a host has built the player's rig from the lead it entered with, so
//! the rig has to follow the lead change through
//! `World::take_player_rig_change` - the signal both play hosts drain to
//! rebuild the rig. It used to fire only on a `CC F8 50` re-stage, and the
//! prompt stood over Vahn's body.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::world::PlayerRigSource;
use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Noa's roster slot (character 1).
const NOA: usize = 1;

#[test]
fn cave01_naming_prompt_stands_over_noas_body() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    // The page's picker party (Vahn leading), and the story flag the scene
    // before raises for Noa's introduction.
    host.world.set_active_party(vec![0, 1, 2]);
    host.world.system_flag_set(0x156);
    host.enter_field_scene("cave01", 0).expect("enter cave01");
    assert_eq!(
        host.world.player_rig_source(),
        PlayerRigSource::PartySlot {
            slot: 0,
            model_id: None
        },
        "the rig a host builds at entry is Vahn's"
    );
    assert!(
        !host.world.take_player_rig_change(),
        "nothing to rebuild at entry - the host just built the rig"
    );

    let mut rebuilt = false;
    let mut ticks = 0u32;
    while !host.world.name_entry_active() && ticks < 2000 {
        host.tick().expect("tick");
        // The hosts drain the signal once per frame.
        rebuilt |= host.world.take_player_rig_change();
        ticks += 1;
    }
    assert!(host.world.name_entry_active(), "name entry never opened");
    assert_eq!(
        host.world.party.name_entry.as_ref().map(|e| e.char_index),
        Some(NOA),
        "the prompt names Noa"
    );
    assert_eq!(host.world.party.active_party, vec![NOA as u8]);
    assert!(rebuilt, "the lead change raised the rig-change signal");
    let rig = host.player_rig_mesh().expect("Noa's field mesh resolves");
    assert_eq!(rig.party_slot, Some(NOA), "the body is Noa's field form");
    assert_eq!(rig.model_id, None);
}
