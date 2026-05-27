//! Disc-gated: the opening Rim Elm training fight is reachable end-to-end.
//!
//! Cold-boots into Rim Elm (`town01`), seeds the training opponent's real
//! stats from the disc monster archive (PROT 867), installs the scripted
//! training encounter through the engine's scripted-encounter seam, and drives
//! the live loop until it flips `Field -> Battle` with the genuine monster
//! (id `0x4F`, "Tetsu") in the enemy slot.
//!
//! This exercises the same path retail uses (a scripted single-monster
//! formation installed at battle entry), short of the per-scene field-VM
//! dialogue state machine that arms it automatically; the arm is driven via
//! the public API here. See `docs/formats/encounter.md`.
//!
//! Disc-gated per the `LEGAIA_DISC_BIN` skip-pass convention: skip-passes
//! without disc data / extracted assets so CI works without Sony bytes.

use std::path::PathBuf;

use legaia_engine_core::encounter_record::{
    RIM_ELM_TRAINING_FORMATION_ID, RIM_ELM_TRAINING_OPPONENT_ID,
};
use legaia_engine_core::monster_catalog::catalog_from_monster_archive;
use legaia_engine_core::world::{FieldCarrierConfig, SceneMode};
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};

const SCENE: &str = "town01";

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
fn training_encounter_reaches_battle_with_real_monster() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing — run `legaia-extract` first");
        return;
    };
    // The training opponent's stats live in the monster archive's extended
    // footprint (PROT 867). Use the extracted extended-footprint file the
    // pipeline writes.
    let archive_path = extracted.join("PROT").join("0867_battle_data.BIN");
    if !archive_path.exists() {
        eprintln!(
            "[skip] {} missing — run `legaia-extract`",
            archive_path.display()
        );
        return;
    }

    let cfg = BootConfig {
        scene: SCENE.to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open boot session");
    session
        .enter_field_live(
            SCENE,
            &FieldLiveOpts {
                live_loop: true,
                ..Default::default()
            },
        )
        .expect("enter field live");
    assert_eq!(
        session.host.world.mode,
        SceneMode::Field,
        "cold boot reaches the field"
    );

    // Seed the training opponent's real stats from the disc archive so the
    // battle spawns Tetsu (not a synthetic placeholder).
    let archive = std::fs::read(&archive_path).expect("read monster archive");
    let catalog = catalog_from_monster_archive(&archive, &[RIM_ELM_TRAINING_OPPONENT_ID as u16]);
    let tetsu = catalog
        .get(RIM_ELM_TRAINING_OPPONENT_ID as u16)
        .expect("archive carries the training opponent");
    assert_eq!(tetsu.hp, 999, "training opponent (Tetsu) HP is 999");
    session.host.world.set_monster_catalog(catalog);

    // Install the training encounter through the scripted-encounter seam: arm
    // the consumer, then forward the record window overlaying the arm opcode
    // (`[opcode][op1][op2][count=1][id]`).
    session.host.world.arm_scripted_encounter(true);
    let window = [0x37u8, 0x00, 0x00, 0x01, RIM_ELM_TRAINING_OPPONENT_ID];
    let formation_id = session
        .host
        .world
        .install_scripted_encounter(&window)
        .expect("non-empty record installs a formation");
    {
        let formation = session
            .host
            .world
            .formation_table
            .formation(formation_id)
            .expect("formation registered");
        assert_eq!(formation.slots.len(), 1, "lone training monster");
        assert_eq!(
            formation.slots[0].monster_id,
            RIM_ELM_TRAINING_OPPONENT_ID as u16
        );
    }

    // Force the encounter roll, then drive the live loop until it flips into
    // Battle (the forced-rate session triggers, the transition counts down,
    // and the world enters BattleMode).
    assert!(
        session.host.world.on_field_step(),
        "forced-rate roll triggers the encounter"
    );
    let mut reached_battle = false;
    for _ in 0..240 {
        let _ = session.tick().expect("tick");
        if session.host.world.mode == SceneMode::Battle {
            reached_battle = true;
            break;
        }
    }
    assert!(
        reached_battle,
        "training encounter flips Field -> Battle within the budget"
    );

    // The enemy slot carries the training opponent with its real HP.
    let world = &session.host.world;
    let monster_slot = world.party_count.clamp(1, 3) as usize;
    assert_eq!(
        world.actors[monster_slot].battle_monster_id,
        Some(RIM_ELM_TRAINING_OPPONENT_ID as u16),
        "enemy slot tagged with the training monster id"
    );
    assert_eq!(
        world.actors[monster_slot].battle.max_hp, 999,
        "enemy slot seeded with Tetsu's real HP"
    );
}

/// The faithful by-index path: the Rim Elm Tetsu fight is town01 MAN
/// `formation_id` 4, which a cold boot already loads from the scene's MAN asset
/// (with the monster archive's real stats merged). The scripted carrier selects
/// that formation by index; `install_man_formation` models exactly that, with
/// no hand-built record and no manual catalog seeding. This is the same battle
/// the test above reaches, but driven through the actual per-scene formation
/// table rather than a re-encoded `[count][id]` window.
#[test]
fn training_reaches_battle_via_man_formation_index() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing — run `legaia-extract` first");
        return;
    };

    let cfg = BootConfig {
        scene: SCENE.to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open boot session");
    session
        .enter_field_live(
            SCENE,
            &FieldLiveOpts {
                live_loop: true,
                ..Default::default()
            },
        )
        .expect("enter field live");
    assert_eq!(session.host.world.mode, SceneMode::Field);

    // Cold boot loaded town01's MAN formation table. The Tetsu row is index 4,
    // a lone monster id 0x4F.
    {
        let formation = session
            .host
            .world
            .formation_table
            .formation(RIM_ELM_TRAINING_FORMATION_ID)
            .expect("town01 MAN carries formation_id 4");
        assert_eq!(
            formation.slots.len(),
            1,
            "Tetsu is a lone-monster formation"
        );
        assert_eq!(
            formation.slots[0].monster_id, RIM_ELM_TRAINING_OPPONENT_ID as u16,
            "town01 formation_id 4 = monster 0x4F (Tetsu)"
        );
    }

    // Install the formation by index (the carrier's mechanism) and drive to
    // Battle.
    assert_eq!(
        session
            .host
            .world
            .install_man_formation(RIM_ELM_TRAINING_FORMATION_ID),
        Some(RIM_ELM_TRAINING_FORMATION_ID),
    );
    assert!(
        session.host.world.on_field_step(),
        "forced-rate roll triggers the scripted formation"
    );
    let mut reached_battle = false;
    for _ in 0..240 {
        let _ = session.tick().expect("tick");
        if session.host.world.mode == SceneMode::Battle {
            reached_battle = true;
            break;
        }
    }
    assert!(
        reached_battle,
        "MAN-formation install flips Field -> Battle"
    );

    // The enemy slot is Tetsu with the real archive HP (999) merged at scene
    // entry — no manual catalog seeding needed on this path.
    let world = &session.host.world;
    let monster_slot = world.party_count.clamp(1, 3) as usize;
    assert_eq!(
        world.actors[monster_slot].battle_monster_id,
        Some(RIM_ELM_TRAINING_OPPONENT_ID as u16),
    );
    assert_eq!(
        world.actors[monster_slot].battle.max_hp, 999,
        "Tetsu's real HP merged from the disc archive at scene entry"
    );
}

/// The field-resident carrier SM path, driven from the **real MAN actor**:
/// entering town01 auto-installs the carrier set derived from its
/// actor-placement partition (`enter_field_scene` ->
/// `install_field_carriers_from_man`), so the Rim Elm sparring partner's
/// identity comes from the scene data rather than a hand-built config. Advance
/// the derived carrier via `engage_field_carrier` (the dialogue-accept
/// stand-in), and let the per-frame `tick_field_carriers` run the state-1
/// formation copy + the `case 2/3` fall-through battle handoff. This drives the
/// transition through the actual entity SM (the field-mode use of
/// `FUN_801DA51C`) rather than a manual `install_man_formation` +
/// `on_field_step`, against the real per-scene MAN formation table.
#[test]
fn training_reaches_battle_via_field_carrier_sm() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing — run `legaia-extract` first");
        return;
    };

    let cfg = BootConfig {
        scene: SCENE.to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open boot session");
    session
        .enter_field_live(
            SCENE,
            &FieldLiveOpts {
                live_loop: true,
                ..Default::default()
            },
        )
        .expect("enter field live");
    assert_eq!(session.host.world.mode, SceneMode::Field);

    // Entering the field auto-installed the carrier set derived from town01's
    // real MAN actor-placement partition (no hand-built config): the pinned Rim
    // Elm sparring partner is the scripted-encounter carrier for formation 4,
    // every other talk NPC a plain carrier. Find the sparring carrier's slot.
    let sparring_idx = session
        .host
        .world
        .field_carrier_configs
        .iter()
        .position(|c| {
            matches!(
                c,
                FieldCarrierConfig::ScriptedEncounter {
                    formation_id: RIM_ELM_TRAINING_FORMATION_ID
                }
            )
        })
        .expect("field entry auto-installs the Rim Elm sparring carrier from the MAN");

    // Idle carriers never self-fire (town01 is 0% random): several ticks stay
    // in the field.
    for _ in 0..16 {
        let _ = session.tick().expect("tick");
        assert_eq!(
            session.host.world.mode,
            SceneMode::Field,
            "idle carriers wait"
        );
    }

    // The dialogue-accept advances the sparring carrier; the SM runs the
    // transition and flips Field -> Battle within a couple of ticks.
    session.host.world.engage_field_carrier(sparring_idx);
    let mut reached_battle = false;
    for _ in 0..8 {
        let _ = session.tick().expect("tick");
        if session.host.world.mode == SceneMode::Battle {
            reached_battle = true;
            break;
        }
    }
    assert!(
        reached_battle,
        "engaging the field carrier flips Field -> Battle via the SM"
    );

    // The enemy slot is Tetsu with the real archive HP merged at scene entry.
    let world = &session.host.world;
    assert_eq!(world.battle_return_mode, SceneMode::Field);
    let monster_slot = world.party_count.clamp(1, 3) as usize;
    assert_eq!(
        world.actors[monster_slot].battle_monster_id,
        Some(RIM_ELM_TRAINING_OPPONENT_ID as u16),
    );
    assert_eq!(
        world.actors[monster_slot].battle.max_hp, 999,
        "Tetsu's real HP via the carrier SM path"
    );
}
