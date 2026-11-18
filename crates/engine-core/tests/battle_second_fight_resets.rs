//! A fight opens the way the session's first fight opens.
//!
//! Battle init allocates the battle block afresh and zeroes all of it
//! (`FUN_80055B6C`, `0x80055F14..0x80055F24`: `0x1E8D` words from the block
//! base, the context at `base + 0x37B4`), and the frame driver's cost ring is
//! the one global history every field frame between two fights rewrites
//! (`FUN_80016B6C`). The engine keeps the context's mirrors and its battle
//! cost ring on `BattleState`, which outlives a fight. This ladder ends one
//! fight (by a flee, and by a win), walks the field, enters a second, and
//! holds every `BattleState` field at that entry to what a fresh session's
//! first entry shows, apart from the fields listed below with their reason.
//!
//! Disc-free: synthetic party + the vanilla monster/formation tables.

use legaia_engine_battle::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, World};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::input::{InputState, PadButton};

fn build_world() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 3;
    // The zeroed records seed HP 0 / no seat, so they go in FIRST: retail's
    // member walk (`FUN_801DB81C`) hands no ring to a member with no HP.
    w.load_party(legaia_save::Party::zeroed(3));
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.liveness = 1;
        w.battle.set_battle_attack(i as u8, 60);
    }
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());

    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.locomotion.camera_azimuth = 0;

    use legaia_engine_battle::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("flee_ladder_test");
    table.set_trigger_rate(0xFF);
    table.push(EncounterEntry::new(1, 1));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 2;
    session.grace_frames = 2;
    w.set_encounter_session(Some(session));

    w.mode = SceneMode::Field;
    w.toggles.live_gameplay_loop = true;
    w.battle.player_driven = true;
    w
}

fn enter_battle(w: &mut World) {
    let up = InputState::mask_of([PadButton::Up]);
    for _ in 0..6000 {
        w.set_pad(up);
        let _ = w.tick();
        if w.mode == SceneMode::Battle {
            return;
        }
    }
    panic!("no encounter triggered in 6000 field ticks");
}

/// Wait for the party command session (the round prompt) to open.
fn wait_for_prompt(w: &mut World) {
    for _ in 0..0x100 {
        if w.battle.command.is_some() {
            return;
        }
        w.set_pad(0);
        let _ = w.tick();
    }
    panic!("no command session opened");
}

fn press(w: &mut World, b: PadButton) {
    // The menu SM reads no pad while the prompt's glide is in flight
    // (`BattleState::battle_command_pad_held`): wait it out, as a player does.
    for _ in 0..64 {
        if !w.battle.battle_command_pad_held() {
            break;
        }
        w.set_pad(0);
        let _ = w.tick();
    }
    w.set_pad(InputState::mask_of([b]));
    let _ = w.tick();
    w.set_pad(0);
    let _ = w.tick();
}

/// Fields a second entry may legitimately show differently.
const MAY_DIFFER: &[(&str, &str)] = &[
    // Display-tick frame ids: they run on unbroken across fights, so a
    // last-ran latch of the first fight can never match a frame of this one.
    ("backdrop_spin_frame", "frame id"),
    ("ghost_pass_frame", "frame id"),
    ("separation_frame", "frame id"),
    ("frame_clock", "frame id origin"),
    ("pending_frame_step_from", "frame id"),
    ("pass_draw_frame", "frame id"),
    ("sm_pass_frame", "frame id"),
    ("frame_start_ticks", "frame id"),
    ("anim_pass_frame", "frame id"),
    ("defeat_sink_frame", "frame id"),
    // The world's one rand stream has moved on: the camera's draws, the
    // intro emitter's seed and the formation roll that sets the hold.
    ("camera", "rand stream"),
    ("intro_rand_state", "rand stream"),
    ("intro_names_frames", "formation roll"),
    // Per-pass records the next pass overwrites before anything reads them.
    ("sm_ran_state", "per pass"),
    ("sm_ran_actor", "per pass"),
    ("sm_ran_target_clip", "per pass"),
    // The last ring push, which every pass's own push overwrites before the
    // film seat that reads it can run.
    ("last_push", "per pass"),
    // Session counters and host hand-offs.
    ("entry_serial", "counts entries"),
    ("field_camera_reseat", "the host camera takes it"),
    ("last_rewards", "the last fight's spoils, for the hosts"),
];

fn fields(w: &World) -> std::collections::BTreeMap<String, String> {
    let s = format!("{:#?}", w.battle);
    let mut out = std::collections::BTreeMap::new();
    let mut cur: Option<String> = None;
    for line in s.lines().skip(1) {
        let top = line.strip_prefix("    ").filter(|r| !r.starts_with(' '));
        if let Some(name) = top.and_then(|r| r.split_once(": ")).map(|(n, _)| n) {
            cur = Some(name.to_string());
            out.insert(name.to_string(), line.to_string());
        } else if let Some(c) = &cur {
            out.get_mut(c).unwrap().push_str(line);
        }
    }
    out
}

fn assert_opens_fresh(second: &World, fresh: &World, how: &str) {
    let (a, b) = (fields(second), fields(fresh));
    assert!(a.len() > 100, "the dump parsed: {} fields", a.len());
    let stale: Vec<&String> = a
        .iter()
        .filter(|(k, v)| b.get(*k) != Some(*v) && !MAY_DIFFER.iter().any(|(m, _)| m == k))
        .map(|(k, _)| k)
        .collect();
    assert!(
        stale.is_empty(),
        "after a {how}, the second fight opened on {stale:?}"
    );
}

fn walk(w: &mut World, n: usize) {
    for _ in 0..n {
        w.set_pad(0);
        let _ = w.tick();
    }
}

#[test]
fn a_fight_after_a_flee_opens_as_the_first_did() {
    let mut fresh = build_world();
    enter_battle(&mut fresh);

    let mut w = build_world();
    enter_battle(&mut w);
    wait_for_prompt(&mut w);
    w.set_battle_formation_latched(
        legaia_engine_battle_vm::battle_formulas::FormationAdvantage::Preemptive,
    );
    press(&mut w, PadButton::Right);
    press(&mut w, PadButton::Left);
    walk(&mut w, 0x100);
    assert_eq!(w.mode, SceneMode::Field, "the flee left the battle");
    walk(&mut w, 200);
    enter_battle(&mut w);
    assert_opens_fresh(&w, &fresh, "flee");
}

#[test]
fn a_fight_after_a_win_opens_as_the_first_did() {
    let mut fresh = build_world();
    enter_battle(&mut fresh);

    let mut w = build_world();
    enter_battle(&mut w);
    wait_for_prompt(&mut w);
    // The party's first strike lands on a field already won.
    let pc = usize::from(w.party.party_count);
    for a in w.actors.iter_mut().skip(pc) {
        a.battle.hp = 0;
    }
    for k in 0..4000 {
        if w.mode == SceneMode::Field {
            break;
        }
        let pad = if k % 20 < 2 {
            InputState::mask_of([PadButton::Cross])
        } else {
            0
        };
        w.set_pad(pad);
        let _ = w.tick();
    }
    assert_eq!(w.mode, SceneMode::Field, "the win left the battle");
    walk(&mut w, 200);
    enter_battle(&mut w);
    assert_opens_fresh(&w, &fresh, "win");
}

/// The field a fight returns to comes up through the field initialiser, which
/// seeds the camera-offset accumulator `_DAT_8007BCAC = 0x3C` ahead of its
/// warp test (`0x801D67B8`); the ease had walked it toward the battle seat's
/// footing all fight.
#[test]
fn the_field_a_fight_returns_to_reseeds_the_camera_offset() {
    let mut w = build_world();
    enter_battle(&mut w);
    wait_for_prompt(&mut w);
    w.set_battle_formation_latched(
        legaia_engine_battle_vm::battle_formulas::FormationAdvantage::Preemptive,
    );
    press(&mut w, PadButton::Right);
    press(&mut w, PadButton::Left);
    for _ in 0..0x200 {
        if w.mode == SceneMode::Field {
            break;
        }
        w.set_pad(0);
        let _ = w.tick();
    }
    assert_eq!(w.mode, SceneMode::Field, "the flee left the battle");
    let seed = legaia_engine_field::camera_ease::CAMERA_OFFSET_EASE_SEED;
    let v = w.camera.offset_ease;
    // The return tick may already have taken one ease step off the seed.
    assert!(
        (v - seed).abs() <= 12,
        "the return seats the accumulator at {seed:#x}, not {v}"
    );
}
