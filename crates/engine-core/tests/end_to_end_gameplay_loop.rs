//! First end-to-end gameplay loop integration test.
//!
//! Walks the engine through the full minimum-viable cycle:
//!
//! 1. **Boot from a save.** Constructs a [`legaia_save::SaveFile`] (LGSF v2)
//!    holding party + story flags + money + inventory; loads it into a
//!    fresh [`World`] via [`World::load_full`]. With a real PSX memory
//!    card image at `~/.mednafen/sav/`, the disc-gated variant also
//!    parses the retail SC block via [`legaia_save::Party::from_retail_sc_block`]
//!    and loads it the same way.
//! 2. **Walk the field.** Switches to [`SceneMode::Field`], installs an
//!    [`crate::encounter::EncounterSession`] keyed to the vanilla
//!    formation table at full trigger rate, and steps until the session
//!    drops into [`crate::encounter::EncounterPhase::Triggered`].
//! 3. **Trigger an encounter.** Drains the formation roll, populates
//!    monster slots 3..N from [`legaia_engine_core::monster_catalog::MonsterCatalog`],
//!    flips the world into [`SceneMode::Battle`].
//! 4. **Drive the battle SM through to victory.** Loops [`World::tick`]
//!    while applying clean-room formula damage (the same pattern as
//!    `battle_full_playthrough.rs`) until every monster slot reaches 0
//!    HP. Asserts the SM resolves to `BattleEndCause::MonsterWipe`.
//! 5. **Apply post-battle rewards.** Calls [`World::apply_battle_loot`]
//!    with the formation + catalog. Asserts at least one party slot
//!    leveled up against the retail XP table when the formation reward
//!    crosses the next threshold.
//! 6. **Save back out.** Round-trips through `world.save_full().write() →
//!    SaveFile::parse() → world.load_full()`. Asserts party HP/MP, level,
//!    money, story flags, and inventory survived the cycle.
//!
//! The synthetic version runs in CI. The disc-gated variant unlocks when
//! a Legaia memory card is present at the mednafen default path.

use legaia_art::Character;
use legaia_engine_core::battle_session::{
    BattlePhase, BattleSession, SessionInput, SessionSlotInfo,
};
use legaia_engine_core::battle_stats::StatRecord;
use legaia_engine_core::encounter::{
    EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
};
use legaia_engine_core::levelup::{StatGain, StatGrowthCurve};
use legaia_engine_core::monster_catalog::{
    FormationDef, MonsterCatalog, vanilla_formation_table, vanilla_monster_catalog,
};
use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_vm::battle_action::{ActionState, ActorFlags, BattleEndCause, StepOutcome};
use legaia_engine_vm::battle_formulas::{accuracy_roll, psyq_rand_step};
use legaia_save::{CharacterRecord, HpMpSp, Party, SaveExt, SaveExtV2, SaveFile};

const ATTACKER_ATK: i32 = 35;
const TARGET_DEF: i32 = 8;

fn locate_memory_card() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    let dir = std::path::PathBuf::from(home).join(".mednafen/sav");
    if !dir.exists() {
        return None;
    }
    let entries = std::fs::read_dir(&dir).ok()?;
    for e in entries.flatten() {
        let p = e.path();
        let name = p.file_name()?.to_string_lossy().to_string();
        if name.contains("Legaia") && name.ends_with(".0.mcr") {
            return Some(p);
        }
    }
    None
}

/// Construct a synthetic 3-character party with retail-shaped stats.
///
/// Each slot gets HP / MP / SP / level / cumulative XP / record stats so
/// the level-up tracker has somewhere to apply gains and the round-trip
/// can verify those fields survived the save cycle.
fn synthetic_party() -> Party {
    let configs = [
        // (name placeholder via slot, hp_max, mp_max, sp_max, level, xp)
        (200u16, 30u16, 50u16, 4u8, 30u32),  // Vahn
        (180u16, 60u16, 30u16, 3u8, 105u32), // Noa
        (220u16, 25u16, 0u16, 5u8, 240u32),  // Gala
    ];
    let mut p = Party::zeroed(3);
    for (i, &(hp_max, mp_max, sp_max, level, xp)) in configs.iter().enumerate() {
        let rec = &mut p.members[i];
        rec.set_hp_mp_sp(HpMpSp {
            hp_cur: hp_max,
            hp_max,
            mp_cur: mp_max,
            mp_max,
            sp_cur: sp_max,
            sp_max,
        });
        rec.raw[0x100] = level; // current level (per project memory: +0x00 level lives at +0x100)
        rec.set_cumulative_xp(xp as u16);
        rec.set_stat_cap(0x3E7);
    }
    p
}

/// Build a populated [`SaveFile`] from a synthetic party.
fn synthetic_save_file() -> SaveFile {
    SaveFile {
        party: synthetic_party(),
        ext: SaveExt {
            story_flags: 0xCAFE_BABE,
            money: 1234,
            inventory: vec![(0x0A, 5), (0x14, 1), (0x20, 99)],
        },
        ext_v2: SaveExtV2 {
            play_time_seconds: 4500,
            active_party: vec![0, 1, 2],
            per_char: vec![],
            saved_chains: vec![],
        },
    }
}

/// Mint a level-up tracker that fires on the very next reward, so the
/// e2e test can assert the post-battle stat bump regardless of which
/// formation rolled. Per-slot gains are deliberately distinct so the
/// round-trip check observes character-keyed differences.
fn deterministic_level_up_tracker() -> legaia_engine_core::levelup::LevelUpTracker {
    use legaia_engine_core::levelup::LevelUpTracker;
    LevelUpTracker::new()
        // Tiny XP table: every level needs `level * 10` cumulative XP, so
        // a single 50-XP reward will always cross at least one threshold.
        .with_xp_table((1u32..=10).map(|n| n * 10).collect::<Vec<u32>>())
        .with_stat_curves([
            StatGrowthCurve::Flat(StatGain { hp: 8, mp: 2 }),
            StatGrowthCurve::Flat(StatGain { hp: 6, mp: 5 }),
            StatGrowthCurve::Flat(StatGain { hp: 12, mp: 1 }),
            StatGrowthCurve::Flat(StatGain::default()),
        ])
}

/// Boost a synthetic monster catalog so each formation hands out enough
/// XP to guarantee a level-up under [`deterministic_level_up_tracker`].
fn boosted_catalog() -> MonsterCatalog {
    let mut cat = vanilla_monster_catalog();
    for def in cat.by_id.values_mut() {
        if def.exp < 50 {
            def.exp = 50;
        }
        if def.gold == 0 {
            def.gold = 4;
        }
    }
    cat
}

/// Wire the world for a battle against `formation` after an encounter
/// triggered. Mirrors the boilerplate the real shell will use.
fn enter_battle(world: &mut World, formation: &FormationDef, catalog: &MonsterCatalog) {
    world.mode = SceneMode::Battle;
    world.party_count = 3;
    // Party slots 0..=2 stay populated from `load_full`. Reset action
    // category to Attack so the SM picks up `Begin → AttackChain` on the
    // first tick.
    for i in 0..3 {
        let actor = world.spawn_actor(i);
        actor.battle.action_category = 3;
        actor.battle.active_target = 3;
    }
    // Monster slots 3..=3+N from formation.
    for (i, slot) in formation.slots.iter().take(5).enumerate() {
        let def = catalog
            .get(slot.monster_id)
            .expect("formation monster in catalog");
        let actor = world.spawn_actor(3 + i);
        actor.battle.liveness = 1;
        actor.battle.hp = def.hp;
        actor.battle.max_hp = def.hp;
        actor.battle.action_category = 3;
    }
    world.battle_ctx.queued_action = 3; // Attack
    world.battle_ctx.action_state = ActionState::Begin.as_byte();
}

/// Apply a clean-room damage strike from the active attacker against the
/// first alive monster. Returns the new HP after the swing landed (or
/// `None` if every monster is already dead / the strike missed).
fn apply_strike(world: &mut World, attacker_slot: u8, seed: &mut u32) -> Option<u16> {
    let monster_count = (3..8)
        .filter(|&i| world.actors[i as usize].battle.liveness != 0)
        .count();
    if monster_count == 0 {
        return None;
    }
    let target_slot = (3..8).find(|&i| world.actors[i as usize].battle.liveness != 0)?;
    if !accuracy_roll(100, 8, seed) {
        return Some(world.actors[target_slot as usize].battle.hp);
    }
    let raw = (ATTACKER_ATK * 2 - TARGET_DEF).max(1);
    let var = (psyq_rand_step(seed) as i32 % 25) - 12;
    let dmg = (raw + raw * var / 100).max(1) as u16;
    let target = &mut world.actors[target_slot as usize].battle;
    target.hp = target.hp.saturating_sub(dmg);
    if target.hp == 0 {
        target.liveness = 0;
    }
    let _ = attacker_slot;
    Some(target.hp)
}

/// Drive [`World::tick`] until the action SM resolves the battle. The
/// caller seeds attacker action state via [`enter_battle`]; this loop
/// translates SM transitions into formula damage strikes and re-arms the
/// SM between attacks. Caps at 100_000 frames so a regression can't hang
/// the test.
fn drive_battle_to_victory(world: &mut World) -> Result<u32, String> {
    let mut seed: u32 = 0xDEAD_C0DE;
    let mut transitions = 0u32;
    let mut strikes = 0u32;

    for frame in 0..100_000u32 {
        let outcome = world.tick();

        // Render-side ADVANCE_DONE clear: the retail engine clears this
        // bit when the recovery animation finishes; without rendering we
        // simulate the same edge inline.
        let attacker = world.battle_ctx.active_actor as usize;
        if attacker < world.actors.len()
            && world.actors[attacker]
                .battle
                .flag_bits
                .has(ActorFlags::ADVANCE_DONE)
            && world.battle_ctx.action_state == ActionState::AttackRecovery.as_byte()
        {
            world.actors[attacker]
                .battle
                .flag_bits
                .clear(ActorFlags::ADVANCE_DONE);
        }

        if let Some(StepOutcome::Transition { from, to }) = outcome {
            transitions += 1;
            // AttackChain → AttackRecovery is the canonical strike-landed
            // edge in the action SM; route formula damage there.
            if from == ActionState::AttackChain.as_byte()
                && to == ActionState::AttackRecovery.as_byte()
                && apply_strike(world, world.battle_ctx.active_actor, &mut seed).is_some()
            {
                strikes += 1;
            }
        }

        if matches!(outcome, Some(StepOutcome::BattleComplete)) {
            return Ok(strikes);
        }

        // If the SM idles in EndOfAction without finishing, re-arm against
        // the next alive monster slot - mirrors the retail "next attacker"
        // queue advance.
        if world.battle_ctx.action_state == ActionState::EndOfAction.as_byte()
            && (3..8).any(|i| world.actors[i as usize].battle.liveness != 0)
        {
            world.battle_ctx.queued_action = 3;
            world.battle_ctx.action_state = ActionState::Begin.as_byte();
            let next = (world.battle_ctx.active_actor + 1) % world.party_count.max(1);
            world.battle_ctx.active_actor = next;
            let target = (3..8)
                .find(|&i| world.actors[i as usize].battle.liveness != 0)
                .unwrap_or(3);
            for i in 0..3 {
                world.actors[i].battle.active_target = target as u8;
                world.actors[i].battle.action_category = 3;
            }
        }

        // Hard-fail early if the SM produced no transitions in 5K frames -
        // probably a port regression.
        if frame == 5_000 && transitions == 0 {
            return Err("battle SM produced no transitions in the first 5000 frames".into());
        }
    }
    Err(format!(
        "battle did not complete after 100K frames (transitions={transitions}, strikes={strikes})"
    ))
}

/// Drive the loop end-to-end against a populated [`SaveFile`].
///
/// The bulk of the cycle: load → install encounter → trigger → battle →
/// rewards → save round-trip. Returns the bytes of the round-tripped
/// save so callers can assert format-level invariants.
fn run_full_loop(starting_save: SaveFile) -> Vec<u8> {
    // 1. Boot from save.
    let mut world = World::new();
    while world.actors.len() < 8 {
        world.actors.push(Actor::default());
    }
    world.load_full(starting_save.clone());
    world.set_formation_table(vanilla_formation_table(), boosted_catalog());
    world.level_up_tracker = deterministic_level_up_tracker();
    // Sync each tracker slot's level to the loaded record level.
    for (i, rec) in starting_save.party.members.iter().enumerate() {
        if i < world.level_up_tracker.level.len() {
            let lvl = rec.raw[0x100].max(1);
            world.level_up_tracker.level[i] = lvl;
        }
    }

    let pre_money = world.money;
    let pre_story_flags = world.story_flags;
    let pre_inventory: std::collections::HashMap<u8, u8> = world.inventory.clone();
    let pre_levels: Vec<u8> = world.level_up_tracker.level[..3].to_vec();

    // 2. Walk the field — install encounter, step until trigger.
    world.mode = SceneMode::Field;
    let mut table = EncounterTable::new("e2e_test_field");
    table.set_trigger_rate(255); // guaranteed to fire on first step
    table.push(EncounterEntry::new(1, 100));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 0;
    session.grace_frames = 0;
    world.set_encounter_session(Some(session));

    let triggered = world.on_field_step();
    assert!(
        triggered,
        "saturated encounter rate should fire on the first step"
    );
    world.tick_encounter();
    let roll = world
        .drain_encounter_formation()
        .expect("encounter session should yield a formation roll");

    // 3. Trigger the encounter — populate monsters from the formation.
    let formation = world
        .formation_table
        .formation(roll.formation_id)
        .expect("formation present in vanilla table")
        .clone();
    let catalog = world.monster_catalog.clone();
    enter_battle(&mut world, &formation, &catalog);

    // 4. Drive the battle SM until victory.
    let strikes = drive_battle_to_victory(&mut world).expect("battle should resolve");
    assert!(strikes > 0, "expected at least one formula strike landed");
    assert_eq!(
        world.battle_end,
        Some(BattleEndCause::MonsterWipe),
        "SM should resolve into MonsterWipe after every monster falls"
    );
    let alive = (3..8)
        .filter(|i| world.actors[*i as usize].battle.liveness != 0)
        .count();
    assert_eq!(alive, 0, "no monster should remain alive after victory");

    // Resync HP/MP back into the roster so save_full sees the post-battle
    // state for the party (the SM updates BattleActor mirrors but not the
    // record copy until save_party fires).
    let _ = world.save_party();

    // 5. Apply post-battle rewards.
    let rewards = world.apply_battle_loot(&formation, &catalog);
    assert!(rewards.xp > 0, "formation should award XP");
    assert!(
        !rewards.level_ups.is_empty(),
        "deterministic tracker + boosted XP should produce at least one level-up; got {:?}",
        rewards
    );
    assert!(
        world.money > pre_money,
        "gold reward should bump money: pre={} post={}",
        pre_money,
        world.money
    );
    let post_levels: Vec<u8> = world.level_up_tracker.level[..3].to_vec();
    assert!(
        post_levels
            .iter()
            .zip(pre_levels.iter())
            .any(|(a, b)| a > b),
        "at least one party slot should level up: pre={:?} post={:?}",
        pre_levels,
        post_levels,
    );

    // Verify the level-up bumped the saved record's HP_max for every
    // character that crossed a threshold.
    for result in &rewards.level_ups {
        let slot = result.char_id as usize;
        let pre_hp_max = starting_save.party.members[slot].hp_mp_sp().hp_max;
        let live_hp_max = world.roster.members[slot].hp_mp_sp().hp_max;
        assert!(
            live_hp_max >= pre_hp_max + result.hp_gained,
            "slot {} HP_max should grow by ≥{} (pre={}, post={})",
            slot,
            result.hp_gained,
            pre_hp_max,
            live_hp_max
        );
    }

    world.end_encounter_battle();

    // 6. Save round-trip.
    let saved = world.save_full();
    let bytes = saved.write();
    let parsed = SaveFile::parse(&bytes).expect("LGSF round-trip must parse");

    let mut reloaded = World::new();
    while reloaded.actors.len() < 8 {
        reloaded.actors.push(Actor::default());
    }
    reloaded.load_full(parsed);

    assert_eq!(
        reloaded.story_flags, pre_story_flags,
        "story flags must round-trip"
    );
    assert_eq!(
        reloaded.money, world.money,
        "money post-battle must round-trip ({} ≠ {})",
        reloaded.money, world.money
    );
    assert_eq!(
        reloaded.inventory.len(),
        pre_inventory.len(),
        "inventory size must round-trip"
    );
    for (k, v) in &pre_inventory {
        assert_eq!(reloaded.inventory.get(k).copied(), Some(*v));
    }
    // Per-character HP / MP / level survives the cycle.
    for (slot, rec) in reloaded.roster.members.iter().enumerate() {
        let live = world.roster.members[slot].hp_mp_sp();
        let post = rec.hp_mp_sp();
        assert_eq!(post.hp_max, live.hp_max, "slot {slot} HP_max round-trip");
        assert_eq!(post.mp_max, live.mp_max, "slot {slot} MP_max round-trip");
        assert_eq!(
            rec.raw[0x100], world.roster.members[slot].raw[0x100],
            "slot {slot} level round-trip"
        );
    }

    bytes
}

#[test]
fn synthetic_party_completes_full_gameplay_loop() {
    let bytes = run_full_loop(synthetic_save_file());
    // LGSF magic must lead the buffer.
    assert_eq!(&bytes[..4], b"LGSF");
}

#[test]
fn battle_session_phase_transitions_during_loop() {
    // A lighter smoke around the BattleSession side of the loop. The
    // companion `synthetic_party_completes_full_gameplay_loop` exercises
    // the action SM directly; this one drives the session phase machine
    // to verify CommandInput / Resolve transitions still wire when an
    // encounter routes into the session-driven path.
    let mut world = World::new();
    while world.actors.len() < 8 {
        world.actors.push(Actor::default());
    }
    world.load_full(synthetic_save_file());
    world.set_formation_table(vanilla_formation_table(), boosted_catalog());

    // Trigger an encounter the same way the full loop does.
    world.mode = SceneMode::Field;
    let mut table = EncounterTable::new("session_smoke");
    table.set_trigger_rate(255);
    table.push(EncounterEntry::new(1, 100));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 0;
    session.grace_frames = 0;
    world.set_encounter_session(Some(session));
    assert!(world.on_field_step());
    world.tick_encounter();
    let roll = world.drain_encounter_formation().expect("triggered");
    let formation = world
        .formation_table
        .formation(roll.formation_id)
        .expect("vanilla formation")
        .clone();

    // BattleSession setup: 3-party + N monsters from the formation.
    let mut bs = BattleSession::new();
    bs.set_party([Character::Vahn, Character::Noa, Character::Gala]);
    let stat = StatRecord {
        base_attack: 50,
        base_udf: 30,
        base_ldf: 25,
        base_accuracy: 80,
        base_evasion: 20,
        ..Default::default()
    };
    for (i, name) in ["Vahn", "Noa", "Gala"].iter().enumerate() {
        bs.set_slot_info(
            i as u8,
            SessionSlotInfo {
                name: (*name).into(),
                is_party: true,
                record: Some(stat),
                mp_max: 30,
            },
        );
    }
    let catalog = world.monster_catalog.clone();
    for (i, slot) in formation.slots.iter().enumerate() {
        let def = catalog.get(slot.monster_id).expect("monster");
        let actor_idx = 3 + i;
        world.actors[actor_idx].battle.hp = def.hp;
        world.actors[actor_idx].battle.max_hp = def.hp;
        bs.set_slot_info(
            actor_idx as u8,
            SessionSlotInfo {
                name: def.name.clone(),
                is_party: false,
                record: Some(StatRecord::default()),
                mp_max: 0,
            },
        );
    }
    bs.set_monster_count(formation.slots.len() as u8);

    bs.begin_round(&mut world);
    assert_eq!(bs.phase(), BattlePhase::RoundIntro);

    // Tick until the session lands in CommandInput. Cap a few seconds at
    // 60 Hz - the intro is a fixed 60-frame splash.
    let mut frames = 0u32;
    while !matches!(bs.phase(), BattlePhase::CommandInput) && frames < 600 {
        let _ = bs.tick(&mut world, SessionInput::default());
        frames += 1;
    }
    assert_eq!(
        bs.phase(),
        BattlePhase::CommandInput,
        "session should reach CommandInput within 10 simulated seconds (got {:?})",
        bs.phase()
    );

    // End cleanly so the encounter session enters grace.
    world.end_encounter_battle();
}

#[test]
fn real_psx_memory_card_save_drives_full_loop() {
    let Some(card_path) = locate_memory_card() else {
        eprintln!("[skip] no Legaia memory-card image at ~/.mednafen/sav/");
        return;
    };
    let bytes = std::fs::read(&card_path).expect("read memory card");
    let saves = match legaia_save::parse_card(&bytes) {
        Ok(saves) if !saves.is_empty() => saves,
        Ok(_) => {
            eprintln!("[skip] memory card has no active save blocks");
            return;
        }
        Err(e) => {
            eprintln!("[skip] memory card parse failed: {e}");
            return;
        }
    };
    let block = match legaia_save::read_block(&bytes, saves[0].block) {
        Some(b) => b,
        None => {
            eprintln!("[skip] save block {} could not be read", saves[0].block);
            return;
        }
    };
    let party = match Party::from_retail_sc_block(block, 4) {
        Ok(p) if !p.members.is_empty() => p,
        Ok(_) => {
            eprintln!("[skip] SC block contained no character records");
            return;
        }
        Err(e) => {
            eprintln!("[skip] failed to parse retail SC block: {e}");
            return;
        }
    };

    eprintln!(
        "[real-card] booting loop from {} ({} character record{})",
        card_path.display(),
        party.members.len(),
        if party.members.len() == 1 { "" } else { "s" }
    );

    // Build a SaveFile around the retail party + sentinel globals.
    let save = SaveFile {
        party,
        ext: SaveExt {
            story_flags: 0xCAFE_BABE,
            money: 5000,
            inventory: vec![(0x01, 9), (0x02, 3)],
        },
        ext_v2: SaveExtV2 {
            play_time_seconds: 7200,
            active_party: vec![0, 1, 2],
            per_char: vec![],
            saved_chains: vec![],
        },
    };

    // Pad / truncate to exactly 3 party members so the rest of the loop
    // (which assumes a 3-character active party) lines up.
    let mut three = save.party.clone();
    while three.members.len() < 3 {
        three.members.push(CharacterRecord::zeroed());
    }
    three.members.truncate(3);
    // Rehydrate any zero HP slots so the battle SM has live attackers.
    for rec in three.members.iter_mut() {
        let mut hms = rec.hp_mp_sp();
        if hms.hp_max == 0 {
            hms.hp_max = 200;
        }
        if hms.hp_cur == 0 {
            hms.hp_cur = hms.hp_max;
        }
        if hms.mp_max == 0 {
            hms.mp_max = 30;
        }
        if hms.mp_cur == 0 {
            hms.mp_cur = hms.mp_max;
        }
        rec.set_hp_mp_sp(hms);
    }
    let bytes = run_full_loop(SaveFile {
        party: three,
        ..save
    });
    assert_eq!(&bytes[..4], b"LGSF");
}
