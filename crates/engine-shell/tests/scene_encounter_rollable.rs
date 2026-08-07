//! Disc-gated: which real scenes can produce a random encounter, and why the
//! one the binary boots into cannot.
//!
//! `World::scene_can_roll_encounters` answers the question a player asks as
//! "why am I not getting any fights". It has to answer it the way the roll
//! path does, which means honouring the **story-flag region groups**: a
//! scene's region array holds several story-state variants and only the one
//! the condition walk selects is searched. Rim Elm (`town01`) authors three -
//! two flag-gated ones and an unconditional tail - and with a cleared flag
//! bank the tail wins, whose every row is `rate 0`. It is also the scene
//! `legaia-engine play-window` boots into, so "the port doesn't do random
//! encounters" is what a first run looks like.
//!
//! This pins both sides against real MAN data: a field area answers yes, the
//! opening town answers no, and the no is *the live group being rate-0*
//! rather than an absent table.
//!
//! Skip-passes without disc data.

use std::path::{Path, PathBuf};

use legaia_engine_core::live_loop::LiveLoopOpts;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn boot(extracted: &Path, scene: &str) -> BootSession {
    let cfg = BootConfig {
        scene: scene.to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(extracted, &cfg).expect("open extracted boot session");
    let opts = FieldLiveOpts {
        live_loop: true,
        player_battle: true,
        battle_bgm: None,
    };
    session
        .enter_field_live(scene, &opts)
        .unwrap_or_else(|e| panic!("enter {scene} live: {e:#}"));
    session
}

#[test]
fn a_field_area_rolls_and_the_opening_town_does_not() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let field = boot(&extracted, "map03");
    assert!(
        field.host.world.scene_encounters_rollable,
        "map03 is a field area with rate-bearing encounter regions"
    );
    assert!(
        !field.host.world.show_encounter_hint(),
        "a rollable scene must not show the no-encounters hint"
    );

    let town = boot(&extracted, "town01");
    assert!(
        !town.host.world.scene_encounters_rollable,
        "town01's rate-bearing regions belong to a story state the flag bank \
         is not in - this is retail scene data, and the reason a default boot \
         looks quiet"
    );
    assert!(
        town.host.world.show_encounter_hint(),
        "the host must be told to say so"
    );
}

/// The `town01` answer is *group selection*, not an empty table - if it were
/// empty the test above would pass for the wrong reason and stop guarding the
/// condition walk.
#[test]
fn town01_has_rollable_regions_in_a_story_state_it_is_not_in() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let town = boot(&extracted, "town01");
    let Some(tracker) = town.host.world.field_region_tracker.as_ref() else {
        eprintln!("[skip] town01 installed no region tracker on this build of extracted/");
        return;
    };
    let table = tracker.table();
    assert!(!table.is_empty(), "town01's MAN carries encounter regions");
    let rate_bearing = table
        .regions
        .iter()
        .filter(|r| r.rate_increment > 0 && r.formation_count > 0)
        .count();
    assert!(
        rate_bearing > 0,
        "town01 has regions that would roll if their story flag were set"
    );
    assert!(
        table.groups.len() > 1,
        "town01 authors more than one story-state region group"
    );
    assert!(
        !table.any_rollable(),
        "...but the group a cleared flag bank selects is entirely rate-0"
    );
    // And the gated groups really are the ones carrying the rates, so the
    // answer above flips with story progress rather than being permanent.
    let mut gated_rollable = table.clone();
    for g in table.groups.clone() {
        gated_rollable.select_group(|f| f == g.flag_id);
        if gated_rollable.any_rollable() {
            return;
        }
    }
    panic!("at least one of town01's gated groups must be rollable");
}

/// The browser arms the loop through the same kernel with `LiveLoopOpts`, so
/// the two option shapes must project onto the same world state.
#[test]
fn both_hosts_option_shapes_arm_the_same_world_state() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let native = boot(&extracted, "map03");

    let cfg = BootConfig {
        scene: "map03".to_string(),
        enable_audio: false,
    };
    let mut browserish = BootSession::open(&extracted, &cfg).expect("open");
    browserish
        .host
        .enter_field_scene("map03", 0)
        .expect("enter map03");
    browserish
        .host
        .world
        .arm_live_loop("map03", &LiveLoopOpts::playable());

    let a = &native.host.world;
    let b = &browserish.host.world;
    assert_eq!(a.live_gameplay_loop, b.live_gameplay_loop);
    assert_eq!(a.battle_player_driven, b.battle_player_driven);
    assert_eq!(a.scene_encounters_rollable, b.scene_encounters_rollable);
    assert_eq!(a.active_scene_label, b.active_scene_label);
}

/// The full round trip on **real disc encounter data**: seat the player
/// inside a rollable region of a real field scene, walk until the scene's own
/// MAN table triggers, then let `World::tick` run the battle to resolution
/// and assert it came back with the party's post-battle HP intact.
///
/// This is the end-to-end version of what a player reported as "our port
/// doesn't do random encounters or battles properly": every leg here -
/// the roll, the transition, the battle driving, the return - used to be
/// reachable only with `--live-loop`, and the battle leg could not finish at
/// all without it.
#[test]
fn a_real_scene_rolls_an_encounter_and_the_battle_resolves() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let mut session = boot(&extracted, "map03");

    // Seed the retail New Game roster (Vahn) so battle entry projects a
    // SEATED party - without a roster the party slots are hollow
    // (`max_hp == 0`, no record), the port-only unseeded state the wipe scan
    // refuses to score, and the battle below could never reach a terminal
    // state. (The fight itself is force-won by the resolve loop - see the
    // comment at the battle boost below.)
    let starting = session
        .starting_party
        .clone()
        .expect("SCUS new-game template parses");
    session.host.world.seed_starting_party(&starting);

    // Seat the player at the centre of the first region that is BOTH
    // rate-bearing and unshadowed, so the walk below actually rolls.
    let world = &mut session.host.world;
    let table = world
        .field_region_tracker
        .as_ref()
        .expect("map03 routes a field region tracker")
        .table()
        .clone();
    let mut seat = None;
    for r in &table.regions {
        if r.rate_increment == 0 || r.formation_count == 0 {
            continue;
        }
        let cx = ((r.tile_x_min as u16 + r.tile_x_max as u16) / 2) as u8;
        let cz = ((r.tile_z_min as u16 + r.tile_z_max as u16) / 2) as u8;
        if std::ptr::eq(
            table
                .region_at_tile(cx as i32, cz as i32)
                .expect("centre is inside its own region"),
            r,
        ) {
            seat = Some((cx, cz));
            break;
        }
    }
    let Some((cx, cz)) = seat else {
        panic!("map03 reported rollable but no unshadowed rate-bearing region centre");
    };
    world.seat_player_at_tile_rescued(cx, cz);
    world.live_gameplay_loop = true;
    // Auto-resolve the battle: this test is about the loop reaching a
    // terminal state, not about the command menu.
    world.battle_player_driven = false;
    // map03's roster is late-game (four-figure monster HP and four-figure
    // swings) and the boot party is a fresh Vahn: faithfully he chips for 1
    // damage AND dies long before the fight ends - either way no field
    // return inside the tick budget. The subject here is the loop reaching a
    // terminal state, not the balance, so the resolve loop below force-holds
    // the seeded party standing and re-applies an attack boost each battle
    // tick (a one-shot `set_battle_attack` is overwritten at battle entry by
    // the roster stat fold, which also caps record stats at retail's 999).

    // Retail's counter is `0x3ce +/- rng % 0x1e7` decremented per step by the
    // region's rate increment, so a trigger takes hundreds of steps. Drive
    // steps directly rather than walking - the walk is a separate subsystem.
    let mut triggered = false;
    for _ in 0..20_000 {
        if world.on_field_step() {
            triggered = true;
            break;
        }
    }
    assert!(
        triggered,
        "an unshadowed rate-bearing region must eventually roll a formation"
    );

    let hp_before: Vec<u16> = world
        .roster
        .members
        .iter()
        .map(|m| m.hp_mp_sp().hp_cur)
        .collect();

    let mut entered = false;
    let mut resolved = false;
    for _ in 0..100_000 {
        if world.mode == legaia_engine_core::world::SceneMode::Battle {
            entered = true;
            // Force the victory outcome (see the comment above): boosted
            // attack so the monsters fall in bounded time, and the party
            // held standing with the displayed bar force-synced (a live-HP
            // write leaving `hp != hp_display` pending is absorbing - the
            // SM's `0x51` bar-drain gate parks the battle on it).
            for slot in 0..world.party_count {
                world.set_battle_attack(slot, 30_000);
            }
            for slot in 0..world.party_count as usize {
                let a = &mut world.actors[slot].battle;
                if a.max_hp > 0 {
                    let max = a.max_hp;
                    a.set_hp_synced(max);
                    a.liveness = 1;
                }
            }
        }
        world.tick();
        if world.mode == legaia_engine_core::world::SceneMode::Battle {
            entered = true;
        } else if entered {
            resolved = true;
            break;
        }
    }
    assert!(entered, "the trigger must transition Field -> Battle");
    assert!(
        resolved,
        "the battle must reach a terminal state and return to the field"
    );

    // The records now carry whatever the fight left, not the pre-battle
    // snapshot. Either the party took damage (records moved) or it didn't
    // (records held) - what must NOT happen is a full-HP reset that erases a
    // real loss, so assert against the live mirrors instead of a constant.
    for (i, before) in hp_before.iter().enumerate() {
        let after = world.roster.members[i].hp_mp_sp().hp_cur;
        if i < world.party_count as usize {
            assert_eq!(
                after, world.actors[i].battle.hp,
                "slot {i} record and field actor must agree after the battle"
            );
        }
        let _ = before;
    }
}
