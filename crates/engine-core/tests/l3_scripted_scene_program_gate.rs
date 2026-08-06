//! Reach conversion: the **`MAN_LOAD_RESUME` gate** that arms the field
//! overlay's scripted-scene voice-over programs (`FUN_801D5A24` spawn,
//! `FUN_801D4A60` step).
//!
//! Both addresses were on the reach worklist as GATED-(b) rows behind "the
//! `MAN_LOAD_RESUME` story flags". Nothing seeds those flags: a new-game party
//! has an all-zero flag bank, and the only writers are the programs' own
//! openers, which the loader never spawns. So the gate is seeded here
//! directly (`World::system_flag_set` before the scene load), which is
//! precisely what the flag means in retail: *an opener ran in a previous scene
//! and its closer did not*.
//!
//! Two halves:
//!
//! * disc-free - `World::man_load_resume_programs` (the loader arm) under each
//!   of the two flags, and then the program each one seats stepped to its own
//!   terminal arm with the observables checked (voice cue, flag choreography,
//!   player release, story bit);
//! * disc-gated - the same flag seeded before `SceneHost::enter_field_scene`,
//!   so the spawn runs through the production call site
//!   (`load_scene` -> `man_load_actor_reset` -> `man_load_resume_programs`)
//!   rather than through a hand call.
//!
//! ## What driving the programs found
//!
//! The step function's own `NOT WIRED:` disclosure names the BGM
//! request/acknowledge pair as the blocker "for states `0x02`, `0x16` and
//! `0x19`". Only one of those three is in a program the loader can spawn:
//! `0x02` belongs to program **0**, an opener, and `0x19` gates on the CD-XA
//! in-flight counter the same disclosure says is *not* a blocker. Program 3 -
//! the flag-`0x0C` closer - reads no BGM field at all. That is asserted below
//! (`program_3_needs_no_bgm_input_at_all`), because it is the difference
//! between "this needs a BGM model" and "one of the two resumable programs
//! does".
//!
//! Structural assertions only; no Sony bytes. The disc half skip-passes
//! without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_core::actor_handler::ActorHandler;
use legaia_engine_core::field_actor_program::{
    FLAG_PLAYER_BUSY, FLAG_PROGRAM_1, FLAG_SCENE_ACTIVE, MAN_LOAD_RESUME, PLAYER_ENGAGED,
    PLAYER_LIFTING, PLAYER_MOTION_HELD, ProgramActor, ProgramEffect, ProgramEnv, ProgramPlayer,
    STORY_FLAG_BIT, VOICE_CHANNEL, VOICE_CLIP, VOICE_DURATION, step_scene_program,
};
use legaia_engine_core::world::World;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// A player the programs can actually park and release: non-zero speed (so
/// "the parked speed comes back" is falsifiable) and a non-zero height (so the
/// lift leg has something to wind down).
fn player() -> ProgramPlayer {
    ProgramPlayer {
        pos: (0x120, 0x40, -0x300),
        rot: (0, 0x800, 0),
        flags: 0,
        speed: 0x0C00,
        lift: 0,
    }
}

/// The env a live host could supply today: a real cadence byte, the BGM
/// request and acknowledge both reading the engine's single `current_bgm`
/// latch, no CD-XA stream in flight, guard flag clear.
fn env_with_bgm_ack(ack: bool) -> ProgramEnv {
    ProgramEnv {
        frame_delta: 2,
        bgm_request: 0x7F3,
        bgm_current: if ack { 0x7F3 } else { 0 },
        dev_flags: 0,
        xa_busy: 0,
        story_flags: STORY_FLAG_BIT,
        release_guard_set: false,
    }
}

/// Run a seated program to its terminal arm (or `None` if it parks).
struct Run {
    effects: Vec<ProgramEffect>,
    player: ProgramPlayer,
    story_flags: u32,
    retired: bool,
    frames: usize,
    parked_state: u16,
}

fn run_program(actor: ProgramActor, mut env: ProgramEnv, max_frames: usize) -> Run {
    let mut a = actor;
    let mut p = player();
    let mut effects = Vec::new();
    let mut frames = 0usize;
    let mut retired = false;
    while frames < max_frames {
        let step = step_scene_program(a, p, env);
        effects.extend(step.effects.iter().copied());
        a = step.actor;
        p = step.player;
        env.story_flags = step.story_flags;
        frames += 1;
        if step.retired {
            retired = true;
            break;
        }
    }
    Run {
        effects,
        player: p,
        story_flags: env.story_flags,
        retired,
        frames,
        parked_state: a.state,
    }
}

// ---------------------------------------------------------------------------
// The gate itself
// ---------------------------------------------------------------------------

#[test]
fn the_resume_flags_are_what_seat_a_scripted_scene_program() {
    // Baseline: an unseeded flag bank seats nothing. Without this the
    // assertions below would pass on a loader that spawns unconditionally.
    let mut w = World::default();
    assert_eq!(
        w.man_load_resume_programs(),
        Vec::<u16>::new(),
        "a zero flag bank must arm no program"
    );
    assert_eq!(w.find_actor_by_handler(ActorHandler::ScriptedScene), None);

    for (flag, program) in MAN_LOAD_RESUME {
        let mut w = World::default();
        w.system_flag_set(u16::from(flag));
        assert_eq!(
            w.man_load_resume_programs(),
            vec![program],
            "flag {flag:#x} must arm program {program}"
        );
        let slot = w
            .find_actor_by_handler(ActorHandler::ScriptedScene)
            .expect("the loader seated a scripted-scene actor");
        assert_eq!(
            w.actors[slot].state_50, program,
            "the spawn writes the program selector into +0x50"
        );
        assert_eq!(
            w.actors[slot].state_54, 0,
            "+0x54 starts at the entry arm, not at the program's block"
        );
    }

    // Both flags set: retail spawns one actor per set bit, in table order.
    let mut w = World::default();
    w.system_flag_set(u16::from(FLAG_SCENE_ACTIVE));
    w.system_flag_set(u16::from(FLAG_PROGRAM_1));
    assert_eq!(
        w.man_load_resume_programs(),
        vec![2, 3],
        "the loader spawns in MAN_LOAD_RESUME table order"
    );
    // Pool *slot* order is the allocator's, not the loader's: the engine's
    // free-slot scan is an `rposition`, so the second spawn lands in a lower
    // slot than the first. Compare the seated set, not the slot sequence.
    let mut seated: Vec<u16> = w
        .actors
        .iter()
        .filter(|a| a.active && a.handler == ActorHandler::ScriptedScene)
        .map(|a| a.state_50)
        .collect();
    seated.sort_unstable();
    assert_eq!(seated, vec![2, 3], "one actor per set resume flag");
}

// ---------------------------------------------------------------------------
// What the seated programs do once stepped
// ---------------------------------------------------------------------------

#[test]
fn program_2_parks_at_the_bgm_handshake_and_runs_once_it_lands() {
    // The flag-0x17 closer. Seat it exactly as the loader does, then step.
    let mut w = World::default();
    w.system_flag_set(u16::from(FLAG_SCENE_ACTIVE));
    assert_eq!(w.man_load_resume_programs(), vec![2]);
    let slot = w
        .find_actor_by_handler(ActorHandler::ScriptedScene)
        .expect("seated");
    let seated = ProgramActor {
        program: w.actors[slot].state_50,
        state: w.actors[slot].state_54,
        ..Default::default()
    };

    // (a) acknowledge pending: the program engages the player, then waits.
    let pending = run_program(seated, env_with_bgm_ack(false), 400);
    assert!(
        !pending.retired,
        "an un-acknowledged BGM request must hold the program open"
    );
    assert_eq!(
        pending.parked_state, 0x16,
        "it parks on the handshake state, not somewhere else"
    );
    assert_ne!(
        pending.player.flags & PLAYER_ENGAGED,
        0,
        "the player is locked while the program waits - this is the softlock \
         shape a host must not leave standing"
    );
    assert!(
        !pending
            .effects
            .iter()
            .any(|e| matches!(e, ProgramEffect::XaStream { .. })),
        "no voice line before the handshake lands"
    );

    // (b) acknowledge landed: the whole program runs, plays its line, and
    // gives the player back.
    let done = run_program(seated, env_with_bgm_ack(true), 4000);
    assert!(done.retired, "the closer must retire, not park");
    assert!(
        done.effects
            .contains(&ProgramEffect::XaStream { clip: VOICE_CLIP }),
        "state 0x16 starts the whole-clip voice stream"
    );
    assert!(
        done.effects.contains(&ProgramEffect::XaCue {
            clip: VOICE_CLIP,
            chan: VOICE_CHANNEL,
            dur: VOICE_DURATION,
        }),
        "state 0x17 fires the chunked cue of the same clip"
    );
    // The handshake is what the flag meant: the opener's flag is cleared by
    // the closer, which is what stops the loader re-spawning it next scene.
    assert!(
        done.effects
            .contains(&ProgramEffect::ClearFlag(FLAG_SCENE_ACTIVE)),
        "program 2 clears the flag that spawned it"
    );
    assert!(
        done.effects
            .contains(&ProgramEffect::SetFlag(FLAG_PLAYER_BUSY))
            && done
                .effects
                .contains(&ProgramEffect::ClearFlag(FLAG_PLAYER_BUSY)),
        "the busy flag is raised and dropped in the same run"
    );
    // The player comes out of it walking.
    assert_eq!(done.player.flags & PLAYER_ENGAGED, 0);
    assert_eq!(done.player.flags & PLAYER_MOTION_HELD, 0);
    assert_eq!(done.player.flags & PLAYER_LIFTING, 0);
    assert_eq!(
        done.player.speed,
        player().speed,
        "the parked speed is handed back"
    );
    assert_eq!(
        done.story_flags & STORY_FLAG_BIT,
        0,
        "and the scratchpad story bit is dropped"
    );
    // Part staging is the visible half; a run that emits none staged nothing.
    assert!(
        done.effects
            .iter()
            .any(|e| matches!(e, ProgramEffect::StagePart { .. })),
        "the program staged no effect parts at all"
    );
}

#[test]
fn program_3_needs_no_bgm_input_at_all() {
    // The finding this fixture exists to pin: the flag-0x0C closer reads no
    // BGM field, so the disclosed blocker does not gate it. Run it under the
    // *worst* BGM env - request and acknowledge disagreeing forever - and it
    // must still reach its terminal arm.
    let mut w = World::default();
    w.system_flag_set(u16::from(FLAG_PROGRAM_1));
    assert_eq!(w.man_load_resume_programs(), vec![3]);
    let slot = w
        .find_actor_by_handler(ActorHandler::ScriptedScene)
        .expect("seated");
    let seated = ProgramActor {
        program: w.actors[slot].state_50,
        state: w.actors[slot].state_54,
        ..Default::default()
    };

    let run = run_program(seated, env_with_bgm_ack(false), 4000);
    assert!(
        run.retired,
        "program 3 parked at state {:#x} after {} frames - it has no BGM gate \
         to park on",
        run.parked_state, run.frames
    );
    assert!(
        run.effects
            .contains(&ProgramEffect::ClearFlag(FLAG_PROGRAM_1)),
        "program 3 clears the flag that spawned it"
    );
    assert!(
        run.effects.contains(&ProgramEffect::Sfx(
            legaia_engine_core::field_actor_program::SFX_BEAT
        )),
        "its state-0x20 beat cue"
    );
    assert_eq!(run.player.flags & PLAYER_ENGAGED, 0, "player released");
    assert_eq!(run.player.speed, player().speed, "speed handed back");
    // It never touches the story bit the other closer owns.
    assert_eq!(run.story_flags & STORY_FLAG_BIT, STORY_FLAG_BIT);
}

#[test]
fn a_resumed_program_does_not_survive_a_second_scene_load() {
    // The loader retires the previous load's program before re-reading the
    // flags, so a flag that stays set across three scene changes seats one
    // actor, not three. Without this the engine leaks a pool slot per change
    // (retail reclaims through the per-scene list teardown it has and the
    // engine does not).
    let mut w = World::default();
    w.system_flag_set(u16::from(FLAG_SCENE_ACTIVE));
    for _ in 0..3 {
        w.man_load_resume_programs();
    }
    let live = w
        .actors
        .iter()
        .filter(|a| a.active && a.handler == ActorHandler::ScriptedScene)
        .count();
    assert_eq!(live, 1, "one live program actor after three loads");
}

// ---------------------------------------------------------------------------
// The production call site
// ---------------------------------------------------------------------------

#[test]
fn a_real_scene_load_seats_the_program_the_seeded_flag_names() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost};

    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));

    // Seed the gate before the load: the flag survives a scene boundary in
    // retail, which is the whole reason the loader reads it.
    host.world.system_flag_set(u16::from(FLAG_SCENE_ACTIVE));
    host.enter_field_scene("town01", 0)
        .expect("enter_field_scene('town01')");

    let slot = host
        .world
        .find_actor_by_handler(ActorHandler::ScriptedScene)
        .expect(
            "a real scene load with the resume flag set must seat the closer \
             (SceneHost::load_scene -> man_load_actor_reset)",
        );
    assert_eq!(host.world.actors[slot].state_50, 2);
    eprintln!("[town01] resumed program {} at slot {slot}", 2);

    // And the negative: a fresh host with a clear bank seats nothing, so the
    // assertion above is about the flag and not about scene loading.
    let mut clean = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    clean.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&clean.index)));
    clean
        .enter_field_scene("town01", 0)
        .expect("enter_field_scene('town01')");
    assert_eq!(
        clean
            .world
            .find_actor_by_handler(ActorHandler::ScriptedScene),
        None,
        "an unseeded bank seats no scripted-scene program"
    );
}
