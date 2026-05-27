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

/// The fully field-VM-driven path: no manual `engage_field_carrier`. A cold boot
/// auto-installs the town01 carriers **and** their interact-slot map, so a real
/// field-interact (`0x3E`, `op0 < 100`) on the sparring partner's placement
/// slot, driven through the field VM, opens its dialogue and arms the engage;
/// accepting the prompt (the `0x4C` n5 sub-4 dialog dismiss on a just-pressed
/// Cross) engages the carrier and the SM flips Field -> Battle against the real
/// per-scene MAN formation, with Tetsu (`0x4F`) in the enemy slot. This is the
/// dialogue-accept auto-arm end to end on disc data — the field-VM bytecode now
/// drives the engage the manual API stood in for.
#[test]
fn training_reaches_battle_via_field_vm_dialogue_accept() {
    use legaia_engine_core::input::PadButton;

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

    let world = &mut session.host.world;

    // Cold boot auto-installed the carrier slot map from town01's MAN: exactly
    // one scripted-encounter (sparring) carrier carries an interact-slot entry,
    // and that slot holds the sparring partner's inline dialogue.
    let slot = {
        let mut slots: Vec<u8> = world.field_carrier_slots.keys().copied().collect();
        slots.sort_unstable();
        assert_eq!(
            slots.len(),
            1,
            "town01 derives exactly one scripted-encounter carrier slot, got {slots:?}"
        );
        slots[0]
    };
    assert!(
        world.field_npc_dialog.contains_key(&slot),
        "the sparring carrier's slot carries inline dialogue"
    );

    // Drive a real field-interact on that slot, then a dialog-advance poll.
    world.load_field_script(vec![0x3E, 0x05, slot, 0x4C, 0x54]);
    world.input.set_pad(0);
    let _ = world.tick();
    assert!(
        world.current_dialog.is_some(),
        "the field-interact opens the sparring partner's dialogue"
    );
    assert!(
        world.pending_carrier_engage.is_some(),
        "the scripted carrier's engage is armed, waiting for the accept"
    );
    assert_eq!(
        world.mode,
        SceneMode::Field,
        "still in the field while the prompt is up"
    );

    // Accept: just-pressed Cross dismisses the dialogue and engages the carrier;
    // the SM runs the transition and flips Field -> Battle within a few ticks.
    world.input.set_pad(PadButton::Cross.mask());
    let mut reached_battle = false;
    for _ in 0..8 {
        let _ = world.tick();
        if world.mode == SceneMode::Battle {
            reached_battle = true;
            break;
        }
        // Release the button so a later tick doesn't re-trigger a dismiss.
        world.input.set_pad(0);
    }
    assert!(
        reached_battle,
        "the dialogue-accept auto-arm flips Field -> Battle (no manual engage)"
    );

    let monster_slot = world.party_count.clamp(1, 3) as usize;
    assert_eq!(
        world.actors[monster_slot].battle_monster_id,
        Some(RIM_ELM_TRAINING_OPPONENT_ID as u16),
        "enemy slot tagged with the training monster id"
    );
    assert_eq!(
        world.actors[monster_slot].battle.max_hp, 999,
        "Tetsu's real HP merged from the disc archive at scene entry"
    );
}

/// The fully input-driven path via the interaction probe (retail `FUN_801cf9f4`):
/// standing next to the sparring partner and pressing the action button talks to
/// it, and pressing again accepts — starting the fight with no script injection
/// and no manual engage. The runtime actor frame is the MAN placement frame
/// (`FUN_8003A1E4` spawns at `tile*128 + 0x40`, the placement's `world_x`), so
/// the probe box-tests the player against the carrier's stored placement
/// position. Positioning the player on the carrier's tile stands in for walking
/// there (the cold spawn is tile 20; the carrier is across the map).
#[test]
fn training_reaches_battle_via_interaction_probe() {
    use legaia_engine_core::input::PadButton;

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
    session.begin_new_game();
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

    // The sparring carrier's slot (the one scripted-encounter slot) and its
    // stored placement position — what the probe box-tests against.
    let (slot, cx, cz) = {
        let w = &session.host.world;
        let slot = *w
            .field_carrier_slots
            .keys()
            .next()
            .expect("town01 installs the scripted-encounter carrier slot");
        let &(cx, cz) = w
            .field_npc_positions
            .get(&slot)
            .expect("the carrier slot carries a placement position");
        (slot, cx, cz)
    };

    // Stand the player on the carrier's tile (stands in for walking across the
    // map from the tile-20 cold spawn).
    let pslot = session.host.world.player_actor_slot.expect("player actor") as usize;
    session.host.world.actors[pslot].move_state.world_x = cx;
    session.host.world.actors[pslot].move_state.world_z = cz;

    // Talk: the probe opens the carrier's dialogue and arms the engage.
    session.host.world.input.set_pad(PadButton::Cross.mask());
    let _ = session.host.world.tick();
    assert!(
        session.host.world.current_dialog.is_some(),
        "the interaction probe opens the sparring partner's dialogue (slot {slot})"
    );
    assert!(
        session.host.world.pending_carrier_engage.is_some(),
        "the scripted carrier's engage is armed"
    );

    // Accept: release, then press again -> dismiss -> engage -> Battle.
    session.host.world.input.set_pad(0);
    let _ = session.host.world.tick();
    session.host.world.input.set_pad(PadButton::Cross.mask());
    let mut reached_battle = false;
    for _ in 0..8 {
        let _ = session.host.world.tick();
        if session.host.world.mode == SceneMode::Battle {
            reached_battle = true;
            break;
        }
        session.host.world.input.set_pad(0);
    }
    assert!(
        reached_battle,
        "the interaction probe (talk + accept) flips Field -> Battle"
    );
    let world = &session.host.world;
    let monster_slot = world.party_count.clamp(1, 3) as usize;
    assert_eq!(
        world.actors[monster_slot].battle_monster_id,
        Some(RIM_ELM_TRAINING_OPPONENT_ID as u16),
        "the probe-driven fight is against Tetsu (0x4F)"
    );
}
