//! Disc-gated: entering a fight resolves each monster's four-slot event-list
//! beats from the battle heap placement and installs them with its clips.
//!
//! Retail reads a four-slot list at `entry[0x10 + ((entry + 0x13) & 0xFF)]`
//! in RAM, and where the block sits follows from the seated party, the
//! formation's ids and the block sizes
//! (`legaia_actor_formats::monster_archive::monster_block_bases`). The value
//! asserted is the one a retail battle state holds for this formation: Demon
//! Fly (id 3) beside Mad Ogre (id 108) with three members seated reads `209`
//! for its tag-`0x0C` entry 5.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_mode::mode::SceneMode;
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

#[test]
fn a_fight_installs_its_monsters_four_slot_beats() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("town01", 0).expect("enter town01");
    host.world.enter_battle(3, 2);
    host.world.actors[3].battle_monster_id = Some(3);
    host.world.actors[4].battle_monster_id = Some(108);
    host.tick().expect("tick");
    assert_eq!(host.world.mode, SceneMode::Battle);

    let fly = host.world.actors[3]
        .battle_four_slot_beats
        .as_ref()
        .expect("Demon Fly's beats installed with its clips");
    assert_eq!(
        fly.get(5).copied().flatten(),
        Some(209),
        "entry 5 reads 209"
    );
    assert!(
        fly.iter().enumerate().all(|(i, b)| i == 5 || b.is_none()),
        "only entry 5 carries a four-slot list"
    );
    let ogre = host.world.actors[4]
        .battle_four_slot_beats
        .as_ref()
        .expect("Mad Ogre's beats installed");
    assert!(
        ogre.iter().all(Option::is_none),
        "Mad Ogre has no four-slot list"
    );
}
