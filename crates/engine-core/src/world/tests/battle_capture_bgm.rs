use super::*;

#[test]
fn capture_banks_points_and_learns_on_finish_battle() {
    let mut world = capture_world(2);
    // Two monsters captured this battle: Killer Bee (Seru 1, learns) and
    // Wolf (no Seru, banks nothing).
    world.battle_captures = vec![7, 9];

    world.finish_battle();

    // battle_captures always drained.
    assert!(world.battle_captures.is_empty());
    // Both party slots learned Spark (id 0x20).
    assert!(world.seru_log.has_learned(0, 1));
    assert!(world.seru_log.has_learned(1, 1));
    assert_eq!(world.seru_log.learned_spells(0), &[0x20]);
    // One accepted outcome (the Wolf had no Seru), with two learn events.
    let outcomes = world.drain_last_capture_outcomes();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].learns.len(), 2);
    // Outcomes drained.
    assert!(world.drain_last_capture_outcomes().is_empty());
}

#[test]
fn capture_below_threshold_banks_points_without_learning() {
    let mut world = capture_world(1);
    world.battle_captures = vec![8]; // Slime -> Seru 2, 40 < 100

    world.finish_battle();

    assert!(!world.seru_log.has_learned(0, 2), "not learned yet");
    assert_eq!(world.seru_log.row(0, 2).points, 40, "points banked");
    let outcomes = world.drain_last_capture_outcomes();
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].learns.is_empty());
}

#[test]
fn capture_sets_the_banner_and_it_clears_on_tick() {
    use crate::seru_learning::CaptureState;

    let mut world = capture_world(1);
    world.set_spell_catalog(crate::spells::SpellCatalog::vanilla());
    world.battle_captures = vec![7]; // Killer Bee -> Seru 1 (Spark), learns
    world.finish_battle();

    // The banner opens on the capture phase naming the captured Seru.
    let banner = world
        .current_capture_banner
        .as_ref()
        .expect("capture banner set");
    assert_eq!(banner.seru_name(), "Spark");
    assert!(matches!(banner.state(), CaptureState::Capturing { .. }));
    assert_eq!(banner.current_banner().as_deref(), Some("Captured: Spark!"));
    // A learn event was recorded (party slot 0 crossed the threshold).
    assert_eq!(banner.learns().len(), 1);

    // Drive the banner to completion via World::tick (Field mode after the
    // battle). The default durations are 60 capture + 90 announce frames.
    for _ in 0..(60 + 90 + 4) {
        world.tick();
    }
    assert!(
        world.current_capture_banner.is_none(),
        "banner clears after its phases elapse"
    );
}

#[test]
fn sub_threshold_capture_banner_shows_no_learn_line() {
    let mut world = capture_world(1);
    world.battle_captures = vec![8]; // Slime -> Seru 2, 40 < 100, no learn
    world.finish_battle();

    let banner = world
        .current_capture_banner
        .as_ref()
        .expect("capture banner set even without a learn");
    assert_eq!(banner.seru_name(), "Slow");
    assert!(banner.learns().is_empty());
}

#[test]
fn battle_bgm_swaps_on_encounter_and_restores_on_finish() {
    use crate::monster_catalog::{FormationDef, FormationSlot};

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.current_bgm = Some(0x0A); // field track playing
    world.set_battle_bgm(Some(0x40)); // configured battle track
    let formation = FormationDef::new(7, vec![FormationSlot::new(1)]);

    world.enter_battle_from_formation(&formation);

    // Swapped to the battle track, with a start event queued for the host.
    assert_eq!(world.current_bgm, Some(0x40));
    assert!(world.battle_bgm_active);
    let evs = world.drain_field_events();
    assert!(
        evs.iter().any(|e| matches!(
            e,
            FieldEvent::Bgm {
                text_id: 0x40,
                sub_op: 1
            }
        )),
        "battle BGM start queued: {evs:?}"
    );

    // Finish (no formation/loot) restores the field track + queues its start.
    world.finish_battle();
    assert_eq!(world.current_bgm, Some(0x0A));
    assert!(!world.battle_bgm_active);
    let evs = world.drain_field_events();
    assert!(
        evs.iter().any(|e| matches!(
            e,
            FieldEvent::Bgm {
                text_id: 0x0A,
                sub_op: 1
            }
        )),
        "field BGM restore queued: {evs:?}"
    );
}

#[test]
fn battle_bgm_unset_leaves_music_untouched() {
    use crate::monster_catalog::{FormationDef, FormationSlot};

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.current_bgm = Some(0x0A);
    // No battle_bgm configured (default None) -> no swap, no events.
    let formation = FormationDef::new(7, vec![FormationSlot::new(1)]);
    world.enter_battle_from_formation(&formation);
    assert_eq!(world.current_bgm, Some(0x0A));
    assert!(!world.battle_bgm_active);
    assert!(
        !world
            .drain_field_events()
            .iter()
            .any(|e| matches!(e, FieldEvent::Bgm { .. })),
        "no BGM events when battle_bgm is unset"
    );
    world.finish_battle();
    assert_eq!(world.current_bgm, Some(0x0A));
}

#[test]
fn battle_bgm_with_silent_field_stops_on_finish() {
    use crate::monster_catalog::{FormationDef, FormationSlot};

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.current_bgm = None; // no field music playing
    world.set_battle_bgm(Some(0x40));
    let formation = FormationDef::new(7, vec![FormationSlot::new(1)]);

    world.enter_battle_from_formation(&formation);
    assert_eq!(world.current_bgm, Some(0x40));
    let _ = world.drain_field_events();

    world.finish_battle();
    // Nothing to resume -> battle music stops (sub-op 4) and id clears.
    assert_eq!(world.current_bgm, None);
    let evs = world.drain_field_events();
    assert!(
        evs.iter()
            .any(|e| matches!(e, FieldEvent::Bgm { sub_op: 4, .. })),
        "BGM stop queued when no field track to resume: {evs:?}"
    );
}

#[test]
fn learned_spell_is_offered_in_the_battle_spell_session() {
    let mut world = capture_world(1);
    world.set_spell_catalog(crate::spells::SpellCatalog::vanilla());
    // Caster has an empty roster spell list; learning Spark via capture
    // should still surface it in the battle spell menu.
    world.battle_captures = vec![7];
    world.finish_battle();
    world.actors[0].battle.mp = 99;

    let session = world
        .build_battle_spell_session(0)
        .expect("session builds for slot 0");
    assert!(
        session.spells.iter().any(|s| s.id == 0x20),
        "captured Spark is castable: {:?}",
        session.spells.iter().map(|s| s.id).collect::<Vec<_>>()
    );
}

#[test]
fn capture_progress_round_trips_through_save_load() {
    // Bank a sub-threshold capture, save, reload into a fresh world that
    // has the registry installed, and confirm the points + learned state
    // survive.
    let mut world = capture_world(1);
    world.battle_captures = vec![7, 8]; // Seru 1 learns; Seru 2 banks 40
    world.finish_battle();
    assert!(world.seru_log.has_learned(0, 1));
    assert_eq!(world.seru_log.row(0, 2).points, 40);

    let save = world.save_full();

    let mut reloaded = capture_world(1);
    reloaded.load_full(save);
    assert!(
        reloaded.seru_log.has_learned(0, 1),
        "learned Spark restored"
    );
    assert_eq!(
        reloaded.seru_log.learned_spells(0),
        &[0x20],
        "spell list restored"
    );
    assert_eq!(
        reloaded.seru_log.row(0, 2).points,
        40,
        "sub-threshold progress restored"
    );
    assert!(
        !reloaded.seru_log.has_learned(0, 2),
        "still below threshold after reload"
    );
}

#[test]
fn arts_editor_chain_round_trips_through_save_into_the_battle_menu() {
    use crate::tactical_arts_editor::{ChainEditor, EditInput, EditOutcome};

    // A field-side session: the player opens the Tactical Arts editor and
    // composes a brand-new chain for character slot 0 (Down, Up, Up).
    let mut field = World {
        party_count: 1,
        ..World::default()
    };
    let mut lib = field.chain_library();
    let mut ed = ChainEditor::new(0, &lib);
    // Cross: open the "+ New" editor.
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    for dir in [
        EditInput {
            down: true,
            ..Default::default()
        },
        EditInput {
            up: true,
            ..Default::default()
        },
        EditInput {
            up: true,
            ..Default::default()
        },
    ] {
        ed.tick(dir);
    }
    // Cross: commit to naming, then Cross again: confirm the default name.
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    assert!(
        matches!(ed.outcome(), Some(EditOutcome::Saved { slot: 0, .. })),
        "editor saved a new chain"
    );
    // Apply the edit to the library and store it back into the world -
    // the bridge under test (no direct `saved_chains` seeding).
    ed.apply_outcome(&mut lib).unwrap();
    field.store_chain_library(&lib);

    // The chain now serializes with the save block...
    let save = field.save_full();
    assert_eq!(save.ext_v2.saved_chains.len(), 1);

    // ...and a fresh boot that loads the save can offer it in battle.
    let mut battle = World {
        party_count: 1,
        ..World::default()
    };
    battle.load_full(save);
    let rows = battle.build_battle_arts_rows(0);
    assert_eq!(
        rows.len(),
        1,
        "the edited chain reaches the battle arts menu"
    );
    // Default new-chain name preset; 3 directional inputs => 3 synthetic hits.
    assert_eq!(rows[0].hits(), 3);
}

#[test]
fn battle_arts_synthetic_chain_runs_through_art_power_path_and_cycles_turn() {
    use crate::input::PadButton;
    use legaia_engine_vm::battle_action::ActionState;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.battle_player_driven = true;
    world.mode = SceneMode::Battle;
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.liveness = 1;
    world.set_battle_attack(0, 40);
    world.actors[1].battle.max_hp = 500;
    world.actors[1].battle.hp = 500;
    world.actors[1].battle.liveness = 1;
    world.set_battle_defense(1, 10);
    // One saved chain, 3 directional commands (Left, Right, Down) -> 3 hits.
    // No art record staged, so the row uses the synthetic ×12 profile.
    world.saved_chains.push(legaia_save::SavedChainRecord {
        char_slot: 0,
        name: "Combo".into(),
        sequence: vec![1, 2, 3],
    });

    world.battle_ctx.active_actor = 0;
    world.battle_arts_menu = Some(crate::battle_arts::BattleArtsSession::new(
        0,
        0,
        world.build_battle_arts_rows(0),
    ));
    assert_eq!(world.battle_arts_menu.as_ref().unwrap().arts[0].hits(), 3);

    // Frame 1: Cross opens the target cursor.
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_arts_menu();
    assert!(world.battle_arts_menu.is_some(), "still picking a target");

    // Frame 2: Cross confirms the monster; the art runs.
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_arts_menu();

    assert!(world.battle_arts_menu.is_none(), "arts menu closed");
    // Three synthetic ×12 hits: (40*12/16 - 10) = 20 each => 60 total.
    let per_hit = legaia_engine_vm::battle_formulas::art_strike_damage_default(40, 10, 12);
    assert_eq!(world.actors[1].battle.hp, 500 - per_hit * 3);
    assert_eq!(
        world.battle_ctx.action_state,
        ActionState::EndOfAction.as_byte(),
        "turn parked at EndOfAction so the loop cycles"
    );
    let fx = world.drain_battle_hit_fx();
    assert_eq!(fx.len(), 1, "one summed popup for the combo");
    assert!(!fx[0].is_heal);
    assert_eq!(fx[0].amount, per_hit * 3);
    assert_eq!(fx[0].target_slot, 1);
}

#[test]
fn battle_arts_uses_staged_art_record_power_tiers_and_status() {
    use crate::input::PadButton;
    use legaia_art::power::PowerByte;
    use legaia_art::queue::{ActionConstant, Command};
    use legaia_art::record::EnemyEffect;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.battle_player_driven = true;
    world.mode = SceneMode::Battle;
    world.actors[0].battle.liveness = 1;
    world.set_battle_attack(0, 64);
    world.actors[1].battle.max_hp = 4000;
    world.actors[1].battle.hp = 4000;
    world.actors[1].battle.liveness = 1;
    // UDF / LDF split so the record's per-strike target picks the right half.
    world.set_battle_defense_split(1, Some((10, 40)));

    // Stage a Vahn art: two damage strikes (UDF ×28, LDF ×28) that burns.
    let rec = legaia_art::ArtRecord {
        action: ActionConstant::Art1B,
        commands: vec![Command::Up, Command::Up],
        anim_index: 0,
        anim_extra: vec![],
        name: None,
        power: vec![PowerByte::from_byte(0x1A), PowerByte::from_byte(0x1F)],
        dmg_timing: vec![],
        effect_cues: Default::default(),
        hit_cues: vec![],
        identifier: 0,
        anim_speed: 0,
        enemy_effect: EnemyEffect::Toxic,
        repeat_frames: Default::default(),
        background: 0,
        runtime_address: None,
    };
    world.set_art_record(legaia_art::Character::Vahn, ActionConstant::Art1B, rec);

    // Saved chain ending in the art's command string (Up, Up).
    world.saved_chains.push(legaia_save::SavedChainRecord {
        char_slot: 0,
        name: "Burning Combo".into(),
        sequence: vec![1, 4, 4], // Left, Up, Up
    });

    let rows = world.build_battle_arts_rows(0);
    assert_eq!(rows[0].hits(), 2, "two damage strikes from the record");
    assert_eq!(rows[0].enemy_effect, EnemyEffect::Toxic);

    world.battle_ctx.active_actor = 0;
    world.battle_arts_menu = Some(crate::battle_arts::BattleArtsSession::new(0, 0, rows));

    // Open the target cursor, then confirm.
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_arts_menu();
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_arts_menu();

    // UDF ×28 vs udf=10: 64*28/16 - 10 = 112 - 10 = 102.
    // LDF ×28 vs ldf=40: 64*28/16 - 40 = 112 - 40 = 72.
    let expect = (102u16 + 72u16) as u32;
    assert_eq!(world.actors[1].battle.hp, 4000 - expect as u16);
    assert!(
        world.status_effects.is_afflicted(1),
        "the art's Toxic effect was applied to the target"
    );
    let fx = world.drain_battle_hit_fx();
    assert_eq!(fx.len(), 1);
    assert_eq!(fx[0].amount, expect as u16);
    assert!(fx[0].is_crit, "multi-hit art flagged as crit popup");
}

#[test]
fn build_battle_arts_rows_resolves_miracle_finisher_profile() {
    use legaia_art::power::PowerByte;
    use legaia_art::queue::{ActionConstant, Command};
    use legaia_art::record::EnemyEffect;

    // Vahn's Craze directional string: Right, Down, Left, Up, Left, Up,
    // Right, Down, Left (Left=1 Right=2 Down=3 Up=4).
    let craze_seq = vec![2u8, 3, 1, 4, 1, 4, 2, 3, 1];

    // No art records staged: each of Vahn's Craze's six component arts
    // (Art22/28/23/27/20/2A) degrades to one synthetic ×12 strike.
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.saved_chains.push(legaia_save::SavedChainRecord {
        char_slot: 0,
        name: "MyCraze".into(),
        sequence: craze_seq.clone(),
    });
    let rows = world.build_battle_arts_rows(0);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].miracle,
        Some("Vahn's Craze"),
        "chain flagged Miracle"
    );
    assert_eq!(
        rows[0].hits(),
        6,
        "six component arts -> six synthetic strikes with no records"
    );
    assert_eq!(rows[0].enemy_effect, EnemyEffect::None);

    // Stage the first component art (Art22) with two damage strikes that
    // burn: it contributes its real bytes; the other five stay synthetic.
    let rec = legaia_art::ArtRecord {
        action: ActionConstant::Art22,
        commands: vec![Command::Up],
        anim_index: 0,
        anim_extra: vec![],
        name: None,
        power: vec![PowerByte::from_byte(0x1A), PowerByte::from_byte(0x1A)],
        dmg_timing: vec![],
        effect_cues: Default::default(),
        hit_cues: vec![],
        identifier: 0,
        anim_speed: 0,
        enemy_effect: EnemyEffect::Toxic,
        repeat_frames: Default::default(),
        background: 0,
        runtime_address: None,
    };
    world.set_art_record(legaia_art::Character::Vahn, ActionConstant::Art22, rec);
    let rows = world.build_battle_arts_rows(0);
    assert_eq!(
        rows[0].hits(),
        7,
        "Art22 record (2 strikes) + 5 synthetic component arts"
    );
    assert_eq!(
        rows[0].enemy_effect,
        EnemyEffect::Toxic,
        "first staged component art's status effect is adopted"
    );
}

#[test]
fn build_battle_arts_rows_fires_super_from_recognized_art_sequence() {
    use legaia_art::power::PowerByte;
    use legaia_art::queue::{ActionConstant, Command};
    use legaia_art::record::EnemyEffect;

    // Vahn's Tri-Somersault chains Somersault (Art27) -> Cyclone (Art1F) ->
    // Somersault (Art27); art_sequence = [0x27, 0x1F, 0x27]. Give each
    // component art a one-direction command so a flat chain recognizes them.
    fn stage_art(world: &mut World, action: ActionConstant, cmd: Command, strikes: usize) {
        let rec = legaia_art::ArtRecord {
            action,
            commands: vec![cmd],
            anim_index: 0,
            anim_extra: vec![],
            name: None,
            power: vec![PowerByte::from_byte(0x16); strikes],
            dmg_timing: vec![],
            effect_cues: Default::default(),
            hit_cues: vec![],
            identifier: 0,
            anim_speed: 0,
            enemy_effect: EnemyEffect::None,
            repeat_frames: Default::default(),
            background: 0,
            runtime_address: None,
        };
        world.set_art_record(legaia_art::Character::Vahn, action, rec);
    }

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    stage_art(&mut world, ActionConstant::Art27, Command::Up, 2);
    stage_art(&mut world, ActionConstant::Art1F, Command::Down, 1);

    // Chain Up Down Up -> Somersault Cyclone Somersault.
    // (Left=1 Right=2 Down=3 Up=4.)
    world.saved_chains.push(legaia_save::SavedChainRecord {
        char_slot: 0,
        name: "TriSom".into(),
        sequence: vec![4, 3, 4],
    });
    let rows = world.build_battle_arts_rows(0);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].super_art,
        Some("Tri-Somersault"),
        "recognized art sequence [27 1F 27] fires Vahn's Tri-Somersault"
    );
    assert_eq!(rows[0].miracle, None, "Super is not a Miracle");
    // Super replace art constants = [27, 1F, 2B, 2B, 2B]: Art27 (2 strikes) +
    // Art1F (1 strike) + three synthetic finisher (0x2B) strikes = 6.
    assert_eq!(rows[0].hits(), 6, "component-art strikes + 3 finisher hits");

    // Connector abstraction: a stray Left/Right between the arts (matching no
    // staged art) is skipped, so the same Super still fires.
    world.saved_chains.clear();
    world.saved_chains.push(legaia_save::SavedChainRecord {
        char_slot: 0,
        name: "TriSomLoose".into(),
        sequence: vec![4, 1, 3, 2, 4], // Up [Left] Down [Right] Up
    });
    let rows = world.build_battle_arts_rows(0);
    assert_eq!(
        rows[0].super_art,
        Some("Tri-Somersault"),
        "connector directions between arts are abstracted (skipped)"
    );

    // With no art catalog staged the recognizer can't run, so no Super is
    // detected and the chain falls back to a plain/synthetic row.
    let mut bare = World {
        party_count: 1,
        ..World::default()
    };
    bare.saved_chains.push(legaia_save::SavedChainRecord {
        char_slot: 0,
        name: "TriSom".into(),
        sequence: vec![4, 3, 4],
    });
    assert_eq!(
        bare.build_battle_arts_rows(0)[0].super_art,
        None,
        "no art catalog -> no Super detection (graceful degradation)"
    );
}
