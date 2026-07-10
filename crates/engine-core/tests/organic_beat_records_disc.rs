//! Disc-gated runtime oracle: the chapter-1 story-spine flags land from
//! **executing the P2 beat records' own script bytes**, not from engine
//! stand-ins.
//!
//! Two beats, one shared machinery (the `FUN_8003BDE0` gated partition-2
//! record dispatch):
//!
//! 1. **rikuroa `P2[50]`** (the post-Caruban-victory cutscene record in the
//!    streaming carrier MAN, PROT 0157): after the boss battle the field
//!    return re-runs the scene-entry system script `P1[0]`, whose
//!    `SysFlag.Test 0x289` arm issues the op-`0x44` spawn of global record
//!    `0x5C` = `P2[50]` (C1 gate `[0x142]`, the self-latching one-shot). The
//!    record's own `51 42` SET lands system flag `0x142` and its `62 89`
//!    clears the battle-staged marker. The engine's old victory latch (a
//!    direct `system_flag_set(0x142)` in `apply_battle_loot`) is retired.
//!
//! 2. **town01 `P2[3]`** (the Rim Elm opening record): its `52 25` SET at the
//!    record head lands system flag `0x225` (549) - the record SETs its own
//!    C1 gate, same self-latch shape - purely by timeline execution.
//!
//! Baseline passes keep both non-vacuous: the flags are asserted CLEAR before
//! the record executes.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::scene::SceneHost;
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

fn gated() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return None;
    };
    Some(extracted)
}

/// The Caruban first-visit gate flag (`map01` dolk->dolk2 entrance selector).
const CARUBAN_GATE_FLAG: u16 = 0x142;
/// The transient "boss battle staged" marker rikuroa's stager `P1[3]` sets
/// right before its battle-entry op; tested by `P1[0]` on the post-battle
/// scene re-entry.
const CARUBAN_STAGED_MARKER: u16 = 0x289;
/// The rikuroa first-arrival story flag: `P1[0]`'s intro arm spawns `P2[43]`
/// (op `44 55`) while it is clear, and that record's own `52 FB` SETs it -
/// the same self-latch shape one branch level up. Both `P1[0]`'s deeper
/// dispatch arms and the stager `P1[3]` require it set.
const RIKUROA_ARRIVAL_FLAG: u16 = 0x2FB;
/// The Rim Elm opening one-shot (flag 549), `town01` `P2[3]`'s own C1 gate.
const TOWN01_OPENING_FLAG: u16 = 0x225;

/// Full organic chapter-1 rikuroa slice, no flags pre-seeded:
///
/// 1. first entry runs `P1[0]`, whose intro arm spawns `P2[43]` - its script
///    bytes SET the arrival flag `0x2FB`;
/// 2. re-entering (the scene reload that follows the retail first-arrival
///    cutscene) re-runs `P1[0]` with `0x2FB` set and arms the Caruban fight;
/// 3. entering the battle stamps the staged marker `0x289`;
/// 4. winning returns to the field, the entry script re-runs, its `0x289` arm
///    spawns `P2[50]` through the C1-gated dispatch, and that record's own
///    `51 42` / `62 89` land `0x142` and clear the marker.
///
/// The old victory latch (a direct `system_flag_set(0x142)` in
/// `apply_battle_loot`) is deleted; every flag here lands from script bytes
/// except the battle-entry marker stamp (the stager-record stand-in).
#[test]
fn rikuroa_p2_50_sets_gate_flag_by_record_execution() {
    let Some(extracted) = gated() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.world.live_gameplay_loop = true;
    host.enter_field_scene("rikuroa", 0).expect("enter rikuroa");

    // Baseline (non-vacuous): every flag in the chain is clear.
    for (flag, what) in [
        (CARUBAN_GATE_FLAG, "gate flag 0x142"),
        (CARUBAN_STAGED_MARKER, "staged marker 0x289"),
        (RIKUROA_ARRIVAL_FLAG, "arrival flag 0x2FB"),
    ] {
        assert!(
            !host.world.system_flag_test(flag),
            "baseline: {what} clear on first visit"
        );
    }

    // Phase 1 - first arrival: P1[0]'s intro arm spawns P2[43], whose script
    // bytes SET 0x2FB (organic; the record then owns the scene like the
    // retail arrival cutscene).
    let mut ticks = 0u32;
    while !host.world.system_flag_test(RIKUROA_ARRIVAL_FLAG) && ticks < 2000 {
        host.tick().expect("tick");
        ticks += 1;
    }
    assert!(
        host.world.system_flag_test(RIKUROA_ARRIVAL_FLAG),
        "P2[43]'s `52 FB` SET lands the arrival flag 0x2FB organically (waited {ticks} ticks)"
    );

    // Phase 2 - re-enter (the scene reload after the arrival cutscene): the
    // entry script now takes the deeper dispatch arms, and the first-visit
    // boss arms (gate flag still clear).
    host.enter_field_scene("rikuroa", 0)
        .expect("re-enter rikuroa");
    assert!(
        host.world.scripted_formation_pending,
        "the Caruban fight is armed while the gate flag is clear"
    );

    // Walk across tiles so the armed formation triggers, then drive the
    // battle (the live loop's step detection fires on tile crossings).
    let slot = host.world.player_actor_slot.expect("player actor") as usize;
    let mut ticks = 0u32;
    while !matches!(
        host.world.mode,
        legaia_engine_core::world::SceneMode::Battle
    ) && ticks < 600
    {
        if let Some(actor) = host.world.actors.get_mut(slot) {
            actor.move_state.world_x = actor.move_state.world_x.wrapping_add(0x80);
        }
        host.tick().expect("tick");
        ticks += 1;
    }
    assert!(
        matches!(
            host.world.mode,
            legaia_engine_core::world::SceneMode::Battle
        ),
        "the armed boss formation entered battle within {ticks} ticks"
    );
    // The battle-entry stager marker is set (the engine's stand-in for
    // executing P1[3]'s `52 89` immediately before its battle-entry op).
    assert!(
        host.world.system_flag_test(CARUBAN_STAGED_MARKER),
        "entering the staged boss battle sets marker 0x289"
    );
    assert!(
        !host.world.system_flag_test(CARUBAN_GATE_FLAG),
        "gate flag 0x142 still clear during the battle"
    );

    // Win: wipe the monsters and let the live loop tear the battle down.
    let party = host.world.party_count as usize;
    for a in host.world.actors.iter_mut().skip(party) {
        if a.battle_monster_id.is_some() {
            a.battle.hp = 0;
            a.battle.liveness = 0;
        }
    }
    let mut ticks = 0u32;
    while matches!(
        host.world.mode,
        legaia_engine_core::world::SceneMode::Battle
    ) && ticks < 2000
    {
        host.tick().expect("tick");
        ticks += 1;
    }
    assert!(
        matches!(host.world.mode, legaia_engine_core::world::SceneMode::Field),
        "battle tears down to the field"
    );
    // The victory latch is deleted: the flag must NOT be set yet by the loot
    // path itself... it lands within the post-return ticks by P2[50]'s script.
    let mut ticks = 0u32;
    while !host.world.system_flag_test(CARUBAN_GATE_FLAG) && ticks < 2000 {
        host.tick().expect("tick");
        ticks += 1;
    }
    assert!(
        host.world.system_flag_test(CARUBAN_GATE_FLAG),
        "P2[50]'s `51 42` SET lands 0x142 on the post-battle field return (waited {ticks} ticks)"
    );
    // P2[50]'s `62 89` clears the staged marker as part of the same record.
    let mut ticks = 0u32;
    while host.world.system_flag_test(CARUBAN_STAGED_MARKER) && ticks < 2000 {
        host.tick().expect("tick");
        ticks += 1;
    }
    assert!(
        !host.world.system_flag_test(CARUBAN_STAGED_MARKER),
        "P2[50]'s `62 89` clears the staged marker 0x289"
    );
    eprintln!("[rikuroa] 0x142 landed organically from P2[50] execution");
}

/// The C1 one-shot: with `0x142` already set (the boss beaten), re-entering
/// rikuroa and returning from a battle must NOT replay `P2[50]` (its C1 gate
/// blocks the spawn) - and the boss does not re-arm.
#[test]
fn rikuroa_p2_50_is_blocked_by_its_own_c1_gate_once_set() {
    let Some(extracted) = gated() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.world.system_flag_set(CARUBAN_GATE_FLAG);
    host.enter_field_scene("rikuroa", 0).expect("enter rikuroa");
    assert!(
        !host.world.scripted_formation_pending,
        "the beaten boss does not re-arm"
    );
    // Simulate a stale staged marker (the state a fled/lost fight would leave):
    // the P1[0] arm fires, but P2[50]'s C1 gate `[0x142]` blocks the spawn.
    host.world.system_flag_set(CARUBAN_STAGED_MARKER);
    for _ in 0..600 {
        host.tick().expect("tick");
    }
    assert!(
        host.world.helper_contexts.is_empty(),
        "P2[50] does not spawn once its C1 gate flag is set"
    );
    // Non-vacuous: drive the gated dispatch directly against the real record
    // (P2[50] = global index 0x5C, partition counts [13, 29, ..]) in both
    // gate polarities.
    let man_bytes = host
        .scene
        .as_ref()
        .unwrap()
        .field_man_payload(&host.index)
        .expect("MAN payload read")
        .expect("rikuroa resolves its streaming carrier MAN");
    let man_file = legaia_asset::man_section::parse(&man_bytes).expect("MAN parses");
    assert!(
        !host
            .world
            .install_spawned_helper_record(&man_file, &man_bytes, 0x5C),
        "the dispatcher refuses P2[50] while its C1 flag 0x142 is set"
    );
    host.world.system_flag_clear(CARUBAN_GATE_FLAG);
    assert!(
        host.world
            .install_spawned_helper_record(&man_file, &man_bytes, 0x5C),
        "the dispatcher admits P2[50] once 0x142 is clear (same record, same call)"
    );
}

/// town01's opening timeline record `P2[3]` SETs its own C1 gate flag `0x225`
/// (549) from its script bytes when the opening executes.
#[test]
fn town01_p2_3_sets_flag_549_by_record_execution() {
    let Some(extracted) = gated() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    // New-game prologue hand-off into Rim Elm (the path that installs the
    // opening cutscene timeline record P2[3]).
    host.world.entering_town01_opening = true;
    host.enter_field_scene(legaia_asset::new_game::OPENING_SCENE, 0)
        .expect("enter town01");
    assert!(
        host.world.cutscene_timeline_active(),
        "the opening timeline installs on the prologue hand-off"
    );
    // Baseline (non-vacuous): 549 is clear until the record's own `52 25` runs.
    assert!(
        !host.world.system_flag_test(TOWN01_OPENING_FLAG),
        "baseline: flag 549 clear at scene entry"
    );
    let mut ticks = 0u32;
    while !host.world.system_flag_test(TOWN01_OPENING_FLAG) && ticks < 4000 {
        host.world.tick();
        ticks += 1;
    }
    assert!(
        host.world.system_flag_test(TOWN01_OPENING_FLAG),
        "P2[3]'s `52 25` SET lands flag 549 by timeline execution (ticked {ticks})"
    );
    eprintln!("[town01] flag 549 landed organically from P2[3] execution at tick {ticks}");
}
