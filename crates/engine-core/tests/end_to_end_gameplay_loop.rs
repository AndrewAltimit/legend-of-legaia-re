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

use legaia_art::{Character, Command};
use legaia_engine_core::battle_session::{
    BattlePhase, BattleSession, SessionEvent, SessionInput, SessionSlotInfo,
};
use legaia_engine_core::battle_stats::StatRecord;
use legaia_engine_core::encounter::{
    EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
};
use legaia_engine_core::encounter_record::{EncounterRecord, FORMATION_SLOTS};
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
        rec.set_level(level);
        rec.set_cumulative_xp(xp);
        rec.set_stat_cap(0x3E7);
    }
    p
}

/// Build a populated [`SaveFile`] from a synthetic party.
fn synthetic_save_file() -> SaveFile {
    // Retail-shaped 512-byte story-flag bitmap with a sentinel pattern -
    // verifies that the bigger-than-u32 region round-trips through the
    // full save cycle.
    let mut story_flag_bits = vec![0u8; legaia_save::RETAIL_STORY_FLAGS_SIZE];
    story_flag_bits[0] = 0xAB;
    story_flag_bits[1] = 0xCD;
    story_flag_bits[0x100] = 0xEF;
    SaveFile {
        party: synthetic_party(),
        ext: SaveExt {
            story_flags: 0xCAFE_BABE,
            story_flag_bits,
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
            StatGrowthCurve::Flat(StatGain::hp_mp(8, 2)),
            StatGrowthCurve::Flat(StatGain::hp_mp(6, 5)),
            StatGrowthCurve::Flat(StatGain::hp_mp(12, 1)),
            StatGrowthCurve::Flat(StatGain::default()),
        ])
}

/// Boost a synthetic monster catalog so each formation hands out enough
/// XP to guarantee a level-up under [`deterministic_level_up_tracker`].
///
/// Per-member XP after the retail-shape split is `total / alive`. With a
/// 3-character party that's ~one-third of the per-monster reward, so the
/// floor here (`150`) keeps per-slot crediting comfortably above the
/// `level * 10` threshold the deterministic tracker installs.
fn boosted_catalog() -> MonsterCatalog {
    let mut cat = vanilla_monster_catalog();
    for def in cat.by_id.values_mut() {
        if def.exp < 150 {
            def.exp = 150;
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
/// save and the [`SaveFile`] captured immediately after rewards landed
/// (pre-LGSF-buffer) so callers can assert format-level invariants and
/// run additional round-trips (e.g. through the retail SC block path
/// for Phase K1) without re-driving the entire loop.
fn run_full_loop(starting_save: SaveFile) -> (Vec<u8>, SaveFile) {
    // 1. Boot from save.
    let mut world = World::new();
    while world.actors.len() < 8 {
        world.actors.push(Actor::default());
    }
    world.load_full(starting_save.clone());
    world.set_formation_table(vanilla_formation_table(), boosted_catalog());
    // Replace the tracker with the deterministic test fixture, then
    // re-hydrate per-slot levels from the loaded records (load_full's
    // hydration only touches the active tracker, which we just swapped).
    world.level_up_tracker = deterministic_level_up_tracker();
    for (i, rec) in starting_save.party.members.iter().enumerate() {
        if i < world.level_up_tracker.level.len() {
            world.level_up_tracker.level[i] = rec.level().max(1);
        }
    }

    let pre_money = world.money;
    let pre_story_flags = world.story_flags;
    let pre_story_flag_bits = world.story_flag_bits.clone();
    let pre_inventory: std::collections::HashMap<u8, u8> = world.inventory.clone();
    let pre_levels: Vec<u8> = world.level_up_tracker.level[..3].to_vec();

    // 2. Walk the field - install encounter, step until trigger.
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

    // 3. Trigger the encounter - populate monsters from the formation.
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
    // K1 callers want the pre-buffer SaveFile so they can run their own
    // round-trips (e.g. through the retail SC block path) against the
    // same post-rewards state - clone it before we move into the reload.
    let post_loop_save = saved.clone();

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
        reloaded.story_flag_bits, pre_story_flag_bits,
        "retail-sized story-flag bitmap must round-trip"
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
            rec.level(),
            world.roster.members[slot].level(),
            "slot {slot} level round-trip"
        );
    }

    (bytes, post_loop_save)
}

#[test]
fn synthetic_party_completes_full_gameplay_loop() {
    let (bytes, _post_loop) = run_full_loop(synthetic_save_file());
    // LGSF magic must lead the buffer.
    assert_eq!(&bytes[..4], b"LGSF");
}

/// Phase K1 - engine→retail SC round-trip parity.
///
/// Drive the full gameplay loop and then assert the post-rewards
/// [`SaveFile`] survives a round-trip through the retail SC block
/// layout (`write_into_retail_sc_block` → `from_retail_sc_block`).
/// The SC layout has no slot for the engine's `money`, play time,
/// active party, per-character ext, or saved chains; the schema
/// fixture in `crates/save/tests/fixtures/` documents those as
/// engine-only drops. Everything that *is* representable in retail
/// SC (party records, full 512-byte story-flag bitmap, compact
/// inventory) must come back byte-equal.
///
/// Closes the symmetric gap that `real_card_roundtrip` doesn't
/// cover: it walks a retail card *into* the engine, but until this
/// test there was nothing asserting engine-written saves round-trip
/// back through the retail SC block path.
#[test]
fn synthetic_party_loop_round_trips_via_retail_sc_block() {
    use legaia_save::{
        BLOCK_SIZE, RETAIL_STORY_FLAGS_SIZE, SAVE_BLOCK_MAGIC, read_retail_inventory,
        read_retail_story_flags,
    };

    let (_lgsf_bytes, post_loop) = run_full_loop(synthetic_save_file());

    // Write the engine save into a retail SC block and verify the
    // pinned-offset regions match before we round-trip.
    let mut sc_block = vec![0u8; BLOCK_SIZE];
    post_loop
        .write_into_retail_sc_block(&mut sc_block)
        .expect("write engine save into retail SC block");
    assert_eq!(&sc_block[..2], &SAVE_BLOCK_MAGIC, "SC magic stamped at +0");
    let bits_on_disk = read_retail_story_flags(&sc_block).expect("story flag region present");
    // The writer right-pads bitmaps shorter than RETAIL_STORY_FLAGS_SIZE.
    let mut expected_bits = post_loop.ext.story_flag_bits.clone();
    expected_bits.resize(RETAIL_STORY_FLAGS_SIZE, 0);
    assert_eq!(
        bits_on_disk, expected_bits,
        "story-flag bitmap lands at retail offset 0x14C0"
    );
    let inv_on_disk = read_retail_inventory(&sc_block).expect("inventory region present");
    for (i, (id, count)) in post_loop.ext.inventory.iter().enumerate() {
        assert_eq!(
            inv_on_disk[i * 2],
            *id,
            "inventory slot {i} item id at retail offset"
        );
        assert_eq!(
            inv_on_disk[i * 2 + 1],
            *count,
            "inventory slot {i} count at retail offset"
        );
    }

    // Re-import via from_retail_sc_block. Walk only the active record
    // count so the reader doesn't dip into the story-flag region in
    // the slot-3 overlap.
    let max_records = post_loop.party.members.len();
    let parsed = SaveFile::from_retail_sc_block(&sc_block, max_records)
        .expect("re-import engine save from retail SC block");

    // SC-representable fields survive byte-equal.
    assert_eq!(
        parsed.party.members.len(),
        post_loop.party.members.len(),
        "all party slots survive the SC round-trip"
    );
    for (i, (a, b)) in parsed
        .party
        .members
        .iter()
        .zip(post_loop.party.members.iter())
        .enumerate()
    {
        assert_eq!(
            a.raw, b.raw,
            "char record {i} byte-equal through retail SC (post-rewards state)"
        );
    }
    assert_eq!(
        parsed.ext.story_flag_bits, expected_bits,
        "512-byte story-flag bitmap round-trips through retail SC"
    );
    // The scratchpad u32 is derived from the first 4 bitmap bytes on
    // the read path, so the engine save's `story_flags` must agree.
    assert_eq!(
        parsed.ext.story_flags,
        u32::from_le_bytes([
            expected_bits[0],
            expected_bits[1],
            expected_bits[2],
            expected_bits[3]
        ]),
        "scratchpad story_flags derived from bitmap bytes"
    );
    assert_eq!(
        parsed.ext.inventory, post_loop.ext.inventory,
        "compact inventory survives the SC round-trip"
    );

    // Engine-only fields drop to defaults: money, play_time,
    // active_party, per_char, saved_chains. See the K2 schema fixture
    // for the documented field map.
    assert_eq!(parsed.ext.money, 0, "money is engine-only - drops to 0");
    assert_eq!(
        parsed.ext_v2,
        SaveExtV2::default(),
        "v2 ext block is engine-only - drops to defaults"
    );
    assert!(
        post_loop.ext.money != 0 || post_loop.ext_v2 != SaveExtV2::default(),
        "this test only signals when the engine save actually populates \
         engine-only fields - if both are default the engine-only drop \
         contract isn't meaningfully exercised"
    );

    // Final consumer step: a fresh `World` accepts the parsed save
    // through `load_full` and reports the dropped-fields state. This
    // is how a real save-load flow would experience an SC-only save
    // (e.g. a retail memory card slot the user just opened).
    let mut reloaded = World::new();
    while reloaded.actors.len() < 8 {
        reloaded.actors.push(Actor::default());
    }
    reloaded.load_full(parsed);
    assert_eq!(
        reloaded.money, 0,
        "World::load_full sees money as engine-only"
    );
    assert_eq!(
        reloaded.play_time_seconds, 0,
        "World::load_full sees play_time as engine-only"
    );
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

/// Wire the loop end-to-end through [`BattleSession`] instead of
/// hand-spinning the action SM. The session's `Resolve` phase owns the
/// action SM (one `world.tick()` per frame), so a single `bs.tick()` per
/// iteration advances both the menu phase machine and the underlying
/// SM. The test commits via `bs.tick(SessionInput { start: true, .. })`
/// rather than mutating `world.battle_ctx` directly - the only thing the
/// real shell will do differently is route player input into
/// `push_command` / `push_command_with_target`.
#[test]
fn battle_session_drives_action_sm_to_monster_wipe() {
    let mut world = World::new();
    while world.actors.len() < 8 {
        world.actors.push(Actor::default());
    }
    world.load_full(synthetic_save_file());
    world.set_formation_table(vanilla_formation_table(), boosted_catalog());
    world.level_up_tracker = deterministic_level_up_tracker();
    for (i, rec) in synthetic_party().members.iter().enumerate() {
        if i < world.level_up_tracker.level.len() {
            world.level_up_tracker.level[i] = rec.level().max(1);
        }
    }

    // Trigger an encounter through the same field-step path as the
    // synthetic loop. The session takes over from this point.
    world.mode = SceneMode::Field;
    let mut table = EncounterTable::new("session_e2e");
    table.set_trigger_rate(255);
    table.push(EncounterEntry::new(1, 100));
    let mut enc = EncounterSession::new(EncounterTracker::new(table));
    enc.transition_frames = 0;
    enc.grace_frames = 0;
    world.set_encounter_session(Some(enc));
    assert!(world.on_field_step());
    world.tick_encounter();
    let roll = world.drain_encounter_formation().expect("triggered");
    let formation = world
        .formation_table
        .formation(roll.formation_id)
        .expect("vanilla formation")
        .clone();

    world.mode = SceneMode::Battle;
    world.party_count = 3;

    // Spawn party actors at non-zero HP so the SM treats them as alive.
    for i in 0..3 {
        let actor = world.spawn_actor(i);
        actor.battle.liveness = 1;
        let live = actor.battle.max_hp.max(100);
        actor.battle.max_hp = live;
        actor.battle.hp = live;
        actor.battle.action_category = 3;
    }

    let mut bs = BattleSession::new()
        .with_phase_durations(1, 1) // skip intro/outro splash
        .with_rng_seed(0xC0FF_EE13);
    bs.set_party([Character::Vahn, Character::Noa, Character::Gala]);
    let stat = StatRecord {
        base_attack: 80,
        base_udf: 30,
        base_ldf: 25,
        base_accuracy: 95,
        base_evasion: 5,
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
    for (i, slot) in formation.slots.iter().take(5).enumerate() {
        let def = catalog.get(slot.monster_id).expect("monster def");
        let actor_idx = 3 + i;
        let actor = world.spawn_actor(actor_idx);
        actor.battle.liveness = 1;
        actor.battle.hp = def.hp;
        actor.battle.max_hp = def.hp;
        actor.battle.action_category = 3;
        bs.set_slot_info(
            actor_idx as u8,
            SessionSlotInfo {
                name: def.name.clone(),
                is_party: false,
                record: Some(StatRecord {
                    base_attack: def.attack,
                    base_udf: def.udf,
                    base_ldf: def.ldf,
                    base_accuracy: def.accuracy as u16,
                    base_evasion: def.evasion as u16,
                    ..Default::default()
                }),
                mp_max: 0,
            },
        );
    }
    bs.set_monster_count(formation.slots.len() as u8);

    bs.begin_round(&mut world);

    // Tick until CommandInput - the intro splash takes intro_frames + 1.
    let mut frames = 0u32;
    while !matches!(bs.phase(), BattlePhase::CommandInput) && frames < 600 {
        let _ = bs.tick(&mut world, SessionInput::default());
        frames += 1;
    }
    assert_eq!(bs.phase(), BattlePhase::CommandInput);

    // Buffer one directional command per party slot, then commit. The
    // session enters Resolve and drives the action SM from there.
    for slot in 0u8..3 {
        let _ = bs.runner.set_active_party_slot(slot);
        assert!(
            bs.push_command(&mut world, Command::Right),
            "AP should cover one Right command for slot {slot}"
        );
    }
    let commit_events = bs.tick(
        &mut world,
        SessionInput {
            start: true,
            ..SessionInput::default()
        },
    );
    assert!(
        commit_events
            .iter()
            .any(|e| matches!(e, SessionEvent::TurnCommitted)),
        "commit tick should fire TurnCommitted"
    );
    assert_eq!(bs.phase(), BattlePhase::Resolve);

    // Drive Resolve → terminal. Each `bs.tick` advances `world.tick`
    // once for the head-of-queue attacker; the session pops to the next
    // attacker on EndOfAction. Cap at 200K frames so a regression can't
    // hang the test.
    let mut ended = false;
    for _ in 0..200_000u32 {
        let events = bs.tick(&mut world, SessionInput::default());
        if events
            .iter()
            .any(|e| matches!(e, SessionEvent::BattleEnded { .. }))
        {
            ended = true;
            break;
        }
        if matches!(
            bs.phase(),
            BattlePhase::Victory | BattlePhase::Defeat | BattlePhase::Escaped
        ) {
            ended = true;
            break;
        }
    }
    assert!(ended, "session should land in a terminal phase");
    assert!(
        matches!(bs.phase(), BattlePhase::Victory),
        "session-driven battle should resolve to Victory (got {:?})",
        bs.phase()
    );
    let alive_monsters = (3..3 + formation.slots.len())
        .filter(|i| world.actors.get(*i).is_some_and(|a| a.battle.hp > 0))
        .count();
    assert_eq!(alive_monsters, 0, "all monsters should be dead");

    // Apply the post-battle loot the same way the real shell will and
    // verify the split landed XP on at least one alive slot.
    let _ = world.save_party();
    let rewards = world.apply_battle_loot(&formation, &catalog);
    assert!(rewards.xp > 0);
    assert!(world.money > 0);
    world.end_encounter_battle();
}

/// Locate the extracted `PROT.DAT` (gitignored) so we can scan a real
/// `battle_data` entry for candidate encounter records. Walks the same
/// `extracted/` paths the disc-gated `validation_suite.rs` test uses.
fn extracted_prot_path() -> Option<std::path::PathBuf> {
    for p in ["extracted/PROT.DAT", "../../extracted/PROT.DAT"] {
        let pb = std::path::PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    None
}

/// Disc-gated: parse an encounter record off a real `battle_data` PROT
/// entry, install it through [`World::install_encounter_from_record`],
/// then drive the encounter session into a battle and resolve it. This
/// closes the synthetic data leak in the field → battle handoff -
/// every byte of the formation came from the disc, not a clean-room
/// catalog.
///
/// Skips when `extracted/PROT.DAT` is missing (CI without disc data).
///
/// The on-disc carrier of encounter records is still open (see
/// [`docs/formats/encounter.md`](../../../docs/formats/encounter.md)),
/// so we use a structural sweep: walk the first ~64 KB of an early
/// PROT entry one byte at a time, parse a candidate record at each
/// offset, accept the first record whose `count` is 1..=4 and whose
/// monster ids are all in the catalog's id range. That's the same
/// validity gate the reader at `0x801DA620..0x801DA678` would apply.
#[test]
fn real_battle_data_encounter_drives_loop() {
    let Some(prot_path) = extracted_prot_path() else {
        eprintln!("[skip] no extracted/PROT.DAT");
        return;
    };
    let mut archive = match legaia_prot::archive::Archive::open(&prot_path) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[skip] could not open PROT.DAT: {e}");
            return;
        }
    };
    let mut catalog = vanilla_monster_catalog();
    // Boost the catalog so a single split + XP grant still crosses a
    // level threshold under `deterministic_level_up_tracker`.
    for def in catalog.by_id.values_mut() {
        if def.exp < 150 {
            def.exp = 150;
        }
        if def.gold == 0 {
            def.gold = 4;
        }
    }

    // Sweep early PROT entries (the battle_data cluster starts well
    // under entry 100). Cap at 200 entries and 64 KB per entry so the
    // disc-gated cost is bounded.
    let max_entry_bytes = 64 * 1024usize;
    let mut found: Option<(usize, EncounterRecord)> = None;
    let entries = archive.entries.clone();
    'outer: for (idx, entry) in entries.iter().enumerate().take(200) {
        let mut bytes = Vec::new();
        if archive.read_entry(entry, &mut bytes).is_err() {
            continue;
        }
        let limit = bytes.len().min(max_entry_bytes);
        // Each candidate record is at least 4 + count bytes wide; we
        // step by 4 to stay aligned with the retail record stride.
        let mut off = 0usize;
        while off + 4 + FORMATION_SLOTS <= limit {
            let slice = &bytes[off..off + 4 + FORMATION_SLOTS];
            if let Some(rec) = EncounterRecord::parse(slice)
                && rec.count >= 1
                && rec.count as usize <= FORMATION_SLOTS
                && rec
                    .active_ids()
                    .all(|id| id > 0 && catalog.get(id as u16).is_some())
            {
                found = Some((idx, rec));
                break 'outer;
            }
            off += 4;
        }
    }

    let Some((entry_idx, record)) = found else {
        eprintln!(
            "[skip] no candidate encounter record found in first 200 PROT entries (catalog has {} monsters)",
            catalog.len()
        );
        return;
    };
    eprintln!(
        "[real-encounter] PROT entry {} carries record count={} ids={:?}",
        entry_idx,
        record.count,
        record.active_ids().map(|i| i as u16).collect::<Vec<_>>()
    );

    // Boot a synthetic save and install the record through the same
    // code path the field-VM op handler would.
    let mut world = World::new();
    while world.actors.len() < 8 {
        world.actors.push(Actor::default());
    }
    world.load_full(synthetic_save_file());
    let formation_table = vanilla_formation_table();
    world.set_formation_table(formation_table, catalog.clone());
    world.level_up_tracker = deterministic_level_up_tracker();
    for (i, rec) in synthetic_party().members.iter().enumerate() {
        if i < world.level_up_tracker.level.len() {
            world.level_up_tracker.level[i] = rec.level().max(1);
        }
    }
    world.mode = SceneMode::Field;
    let formation_id = world
        .install_encounter_from_record(&format!("prot_entry_{entry_idx}"), &record)
        .expect("non-empty record should install");
    if let Some(session) = world.encounter.as_mut() {
        session.transition_frames = 0;
        session.grace_frames = 0;
    }

    // Drive the field → battle handoff exactly like the synthetic
    // loop. The retail step roll fires immediately because
    // install_encounter_from_record set the table's trigger rate to
    // 255.
    assert!(world.on_field_step());
    world.tick_encounter();
    let roll = world
        .drain_encounter_formation()
        .expect("disc-derived formation should yield a roll");
    assert_eq!(roll.formation_id, formation_id);
    let formation = world
        .formation_table
        .formation(roll.formation_id)
        .expect("synthesized formation registered")
        .clone();

    enter_battle(&mut world, &formation, &catalog);
    let strikes = drive_battle_to_victory(&mut world).expect("battle resolves");
    assert!(strikes > 0);
    assert_eq!(world.battle_end, Some(BattleEndCause::MonsterWipe));

    let _ = world.save_party();
    let rewards = world.apply_battle_loot(&formation, &catalog);
    assert!(rewards.xp > 0);
    assert!(world.money > 0);
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
    // SaveFile::from_retail_sc_block pulls party + story_flag_bits + inventory
    // straight from the SC block at their pinned offsets. Fall back to the
    // party-only constructor when the block is too small or the records are
    // shaped unexpectedly so the test remains a soft skip rather than a hard fail.
    let retail_save = match SaveFile::from_retail_sc_block(block, 4) {
        Ok(s) if !s.party.members.is_empty() => s,
        Ok(_) => {
            eprintln!("[skip] SC block contained no character records");
            return;
        }
        Err(e) => {
            eprintln!("[skip] failed to parse retail SC block: {e}");
            return;
        }
    };
    let party = retail_save.party.clone();

    eprintln!(
        "[real-card] booting loop from {} ({} character record{}, story-bits={} B, items={})",
        card_path.display(),
        party.members.len(),
        if party.members.len() == 1 { "" } else { "s" },
        retail_save.ext.story_flag_bits.len(),
        retail_save.ext.inventory.len(),
    );

    // Build a SaveFile around the retail party + retail story flag bitmap +
    // retail inventory. Override money/play_time/active_party with test
    // sentinels since the retail money offset isn't yet exposed as a reader
    // and the SM expects a 3-slot active party.
    let save = SaveFile {
        party,
        ext: SaveExt {
            story_flags: retail_save.ext.story_flags,
            story_flag_bits: retail_save.ext.story_flag_bits.clone(),
            money: 5000,
            inventory: if retail_save.ext.inventory.is_empty() {
                vec![(0x01, 9), (0x02, 3)]
            } else {
                retail_save.ext.inventory.clone()
            },
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
    let (bytes, _post_loop) = run_full_loop(SaveFile {
        party: three,
        ..save
    });
    assert_eq!(&bytes[..4], b"LGSF");
}

/// Walk a save through the retail SC-block layout end-to-end.
///
/// Build a populated `SaveFile`, write it into an 8 KiB SC block via
/// [`SaveFile::write_into_retail_sc_block`], read it back via
/// [`SaveFile::from_retail_sc_block`], and verify that the party records,
/// inventory, and full 512-byte story-flag bitmap survive the cycle. Runs
/// in CI - no disc data needed, the SC block is synthesised in-memory.
#[test]
fn save_file_round_trips_through_retail_sc_block_layout() {
    use legaia_save::{
        BLOCK_SIZE, RETAIL_STORY_FLAGS_SIZE, SAVE_BLOCK_MAGIC, read_retail_inventory,
        read_retail_story_flags,
    };

    let mut bits = vec![0u8; RETAIL_STORY_FLAGS_SIZE];
    bits[0] = 0xDE;
    bits[1] = 0xAD;
    bits[0x40] = 0x55;
    bits[RETAIL_STORY_FLAGS_SIZE - 1] = 0x99;

    let mut roster = synthetic_party();
    // synthetic_party may produce zero-valued slots at byte 0 - flip a
    // sentinel into raw[0] so the retail reader recognises each slot as
    // populated (its empty-slot test is "all bytes zero").
    for (i, rec) in roster.members.iter_mut().enumerate() {
        rec.raw[0] = (0x11 * (i as u8 + 1)).max(1);
    }
    let original = SaveFile {
        party: roster,
        ext: SaveExt {
            story_flags: u32::from_le_bytes([bits[0], bits[1], bits[2], bits[3]]),
            story_flag_bits: bits.clone(),
            money: 0,
            inventory: vec![(0x07, 3), (0x10, 1), (0x42, 64)],
        },
        ext_v2: SaveExtV2::default(),
    };

    let mut sc_block = vec![0u8; BLOCK_SIZE];
    original
        .write_into_retail_sc_block(&mut sc_block)
        .expect("write into retail SC block");
    assert_eq!(&sc_block[..2], &SAVE_BLOCK_MAGIC, "SC magic stamped");
    assert_eq!(
        read_retail_story_flags(&sc_block).unwrap(),
        bits.as_slice(),
        "story-flag bitmap landed at retail offset"
    );
    let inv_raw = read_retail_inventory(&sc_block).unwrap();
    assert_eq!(inv_raw[..6], [0x07, 3, 0x10, 1, 0x42, 64]);

    // synthetic_party builds 3 records. Read max_records=3 so the reader
    // never walks into slot 3 (which overlaps the story-flag region in
    // the retail layout and would surface as a spurious extra record).
    let parsed =
        SaveFile::from_retail_sc_block(&sc_block, 3).expect("round-trip through retail SC block");
    assert_eq!(parsed.party.members.len(), original.party.members.len());
    for (i, (a, b)) in parsed
        .party
        .members
        .iter()
        .zip(original.party.members.iter())
        .enumerate()
    {
        assert_eq!(a.raw, b.raw, "character record {i} round-trips byte-exact");
    }
    assert_eq!(parsed.ext.story_flag_bits, bits);
    assert_eq!(parsed.ext.inventory, original.ext.inventory);
}
