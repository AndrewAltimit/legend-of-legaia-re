//! Disc-gated: a **scene-picker** cold entry stands up the full Vahn / Noa /
//! Gala party (`World::seed_picker_party`, reached through
//! `NewGameDefaults::picker_party`), each member in a starter loadout picked
//! from the disc's equipment table, and all three fight with assembled
//! battle forms, swing clips and art banks. The cheats layered on top move
//! the records through the retail growth tables.
//!
//! The headless `BootSession` default stays retail's Vahn-alone roster; this
//! test raises the picker flag the way `play-window` and the browser play
//! page do.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / extracted data.

use std::path::PathBuf;

use legaia_engine_core::encounter_record::RIM_ELM_TRAINING_FORMATION_ID;
use legaia_engine_core::world::SceneMode;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};

const SCENE: &str = "town01";

fn extracted_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR") {
        candidates.push(PathBuf::from(d));
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    candidates.push(root.join("extracted"));
    candidates
        .into_iter()
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

fn open_picker_session() -> Option<BootSession> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return None;
    };
    let cfg = BootConfig {
        scene: SCENE.to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open boot session");
    let Some(defaults) = session.host.new_game_defaults.as_mut() else {
        eprintln!("[skip] no SCUS new-game template reachable from {extracted:?}");
        return None;
    };
    defaults.picker_party = true;
    session
        .enter_field_live(
            SCENE,
            &FieldLiveOpts {
                live_loop: true,
                ..Default::default()
            },
        )
        .expect("enter field live");
    Some(session)
}

#[test]
fn picker_entry_seeds_three_equipped_members_that_fight() {
    let Some(mut session) = open_picker_session() else {
        return;
    };
    {
        let w = &session.host.world;
        assert_eq!(w.party.party_count, 3, "Vahn, Noa and Gala");
        assert_eq!(w.party.active_party, vec![0, 1, 2]);
        let names: Vec<&str> = (0..3).map(|i| w.party_name(i)).collect();
        assert_eq!(names, ["Vahn", "Noa", "Gala"]);
        for (slot, rec) in w.party.roster.members.iter().enumerate().take(3) {
            let eq = rec.equipment().slots;
            let weapon = eq[legaia_engine_core::new_game::WEAPON_SLOT_BYTE[slot]];
            assert_ne!(weapon, 0, "member {slot} holds a starter weapon");
            assert_ne!(eq[0], 0, "member {slot} wears starter armour");
            eprintln!("[ran] member {slot} loadout {eq:?}");
        }
    }

    // A fight seats all three, each with an assembled battle form.
    assert_eq!(
        session
            .host
            .world
            .install_man_formation(RIM_ELM_TRAINING_FORMATION_ID),
        Some(RIM_ELM_TRAINING_FORMATION_ID),
    );
    assert!(session.host.world.on_field_step(), "forced roll triggers");
    for _ in 0..240 {
        let _ = session.tick().expect("tick");
        if session.host.world.mode == SceneMode::Battle {
            break;
        }
    }
    assert_eq!(session.host.world.mode, SceneMode::Battle);
    let forms = session
        .host
        .battle_party_forms()
        .expect("battle entry built the party forms");
    assert_eq!(forms.forms.len(), 3, "one form per member");
    for f in &forms.forms {
        assert!(
            f.assembled,
            "member {} (cslot {}) assembles from its player battle file with the starter loadout",
            f.member, f.cslot
        );
    }
    for m in 0..3 {
        let a = &session.host.world.actors[m];
        assert!(a.battle.max_hp > 0, "member {m} has a live HP mirror");
        assert!(
            a.battle_action_clips.is_some(),
            "member {m} has swing clips"
        );
        assert!(
            a.battle_art_bank
                .as_ref()
                .is_some_and(|b| b.iter().any(Option::is_some)),
            "member {m} has an art-animation bank"
        );
    }
    eprintln!("[ran] three assembled party forms in the training fight");
}

#[test]
fn level_cheat_applies_retail_growth() {
    let Some(mut session) = open_picker_session() else {
        return;
    };
    let w = &mut session.host.world;
    let before: Vec<_> = (0..3)
        .map(|i| w.party.roster.members[i].live_stats())
        .collect();
    let got = w.cheat_set_party_level(20);
    assert_eq!(got, vec![(0, 20), (1, 20), (2, 20)]);
    for (i, prev) in before.iter().enumerate() {
        let rec = &w.party.roster.members[i];
        let after = rec.live_stats();
        assert!(after.atk > prev.atk, "member {i} ATK grew");
        assert!(after.udf > prev.udf, "member {i} UDF grew");
        // The displayed level is `+0x130` (`+0x100` shares the ability
        // bitfield the stat fold rewrites).
        assert_eq!(rec.magic_rank(), 20);
        eprintln!(
            "[ran] member {i} Lv20: HP {} ATK {} -> {}",
            rec.hp_mp_sp().hp_max,
            prev.atk,
            after.atk
        );
    }
    let pairs = w.item_name_pairs();
    assert!(!pairs.is_empty(), "item names install from the disc");
    let id = legaia_engine_core::cheats::resolve_item(
        "healing leaf",
        pairs.iter().map(|(i, n)| (*i, n.as_str())),
    )
    .expect("Healing Leaf resolves by name");
    assert_eq!(id, 0x77, "Healing Leaf is item 0x77");
}
