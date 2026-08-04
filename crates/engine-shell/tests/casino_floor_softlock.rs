//! Disc-gated regression: **the Sol casino floor is walkable.**
//!
//! ## The defect this pins
//!
//! `koin1` is Sol's casino. Its seven genuine `0x3E` placements are the venue
//! cabinets (slot machine, Baka Fighter, Muscle Dome), and its casino NPCs
//! stand on the same floor. Walking into an NPC *or* a cabinet froze the
//! player mid-stride with the BGM still running - the classic "everything
//! stopped but the music" softlock.
//!
//! Three separate things had to be true for that, and this file measures all
//! three, because fixing any one alone leaves the softlock reachable by
//! another route:
//!
//! 1. **The trigger was wrong.** `check_field_walk_touch` executed the
//!    *structurally decoded* effect of the placement's script. For a
//!    `WalkTouchEvent::Warp` that effect is the script's terminal mode-24
//!    door warp - past a coin compare and a confirm dialogue the contact
//!    never cleared. Retail's contact kernel `FUN_801d5b5c` resumes the
//!    script; it does not run the script's last instruction.
//! 2. **The blast radius was the whole floor.** The contact box is
//!    +/-`FIELD_PROP_BOX_HALF` (`0x50`) about the placement, plus the
//!    directional probe offsets - and koin1's NPCs stand *inside* their
//!    neighbouring cabinets' boxes. So "walk up to an NPC to talk" and "use a
//!    cabinet" were the same input. That is why the bug report named both.
//! 3. **There was no way out.** No shipped host has a player-reachable exit
//!    from an entered minigame: the native window has developer hotkeys
//!    (`O` / `B` / `M`) and the browser play page does not draw four of the
//!    five modes at all. An entry was therefore terminal.
//!
//! ## What each test would catch
//!
//! - `casino_floor_is_walkable` fails if (1) or (2) regress - it walks the
//!   real player into every cabinet and every NPC from all four sides and
//!   asserts the scene mode never leaves `Field`.
//! - `every_minigame_can_be_left_by_pad` fails if (3) regresses - it enters
//!   each of the five modes the way the door warp does and asserts a pad
//!   press gets back out. Without it, the day a cabinet's script legitimately
//!   reaches its `0x3E` is the day the softlock returns.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md convention).

use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::minigame_entry::MinigameSubId;
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::SceneMode;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let d = repo_root().join("extracted");
    if d.is_dir() {
        Some(d)
    } else {
        eprintln!("[skip] extracted/ missing - run legaia-extract first");
        None
    }
}

fn open_koin1() -> Option<SceneHost> {
    let extracted = extracted_dir()?;
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("koin1", 0).expect("enter koin1");
    // Let the scene's entry script seat its NPCs before reading positions.
    for _ in 0..90 {
        let _ = host.tick();
    }
    Some(host)
}

fn player_pos(host: &SceneHost) -> (i16, i16) {
    host.world
        .player_actor_slot
        .and_then(|s| host.world.actors.get(s as usize))
        .map(|a| (a.move_state.world_x, a.move_state.world_z))
        .unwrap_or((0, 0))
}

fn seat(host: &mut SceneHost, x: i16, z: i16) {
    if host.world.player_actor_slot.is_none() {
        host.world.install_field_player(0);
    }
    let s = host.world.player_actor_slot.expect("player slot") as usize;
    host.world.actors[s].move_state.world_x = x;
    host.world.actors[s].move_state.world_z = z;
}

/// Walk into `target` from each of the four sides. Returns the modes the world
/// left `Field` for, one per approach that produced one.
///
/// A seat inside a wall produces no movement and is not a run; it is skipped
/// rather than counted as a pass, so the test cannot go vacuous by seating
/// every probe somewhere the player cannot walk.
fn approach_from_all_sides(target: (i16, i16)) -> (usize, Vec<(&'static str, SceneMode)>) {
    const OFF: i16 = 260;
    let mut runs = 0usize;
    let mut left = Vec::new();
    for (name, dx, dz, pad) in [
        ("from S", 0, -OFF, PadButton::Up),
        ("from N", 0, OFF, PadButton::Down),
        ("from W", -OFF, 0, PadButton::Right),
        ("from E", OFF, 0, PadButton::Left),
    ] {
        let Some(mut host) = open_koin1() else {
            return (0, Vec::new());
        };
        seat(&mut host, target.0 + dx, target.1 + dz);
        let start = player_pos(&host);
        host.world.set_pad(pad.mask());
        let mut departed = None;
        for f in 0..200 {
            let _ = host.tick();
            if f == 140 {
                host.world.set_pad(0);
            }
            if departed.is_none() && host.world.mode != SceneMode::Field {
                departed = Some(host.world.mode);
            }
        }
        let end = player_pos(&host);
        let moved = (end.0 - start.0).abs() as i32 + (end.1 - start.1).abs() as i32;
        if moved < 16 && departed.is_none() {
            continue; // seated in a wall - not a run
        }
        runs += 1;
        if let Some(m) = departed {
            left.push((name, m));
        }
    }
    (runs, left)
}

#[test]
fn casino_floor_is_walkable() {
    let Some(host) = open_koin1() else { return };

    let cabinets: Vec<(u8, (i16, i16))> = host
        .world
        .field_walk_touch
        .iter()
        .map(|(&s, &(p, _))| (s, p))
        .collect();
    // NPCs parked at the off-map stow coordinate are not on the floor.
    let npcs: Vec<(u8, (i16, i16))> = host
        .world
        .field_npc_positions
        .iter()
        .map(|(&s, &p)| (s, p))
        .filter(|(_, p)| p.0 < 16000 && p.1 < 16000)
        .collect();
    assert!(
        cabinets.len() >= 7,
        "koin1 installs its casino cabinets as walk-touch placements (got {})",
        cabinets.len()
    );
    assert!(
        npcs.len() >= 10,
        "koin1 seats its casino NPCs on the floor (got {})",
        npcs.len()
    );

    let mut runs = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for (kind, list) in [("cabinet", &cabinets), ("npc", &npcs)] {
        for (slot, pos) in list.iter() {
            let (n, left) = approach_from_all_sides(*pos);
            runs += n;
            for (side, mode) in left {
                failures.push(format!("{kind} {slot} at {pos:?} {side} -> {mode:?}"));
            }
        }
    }

    // Non-vacuity: the probe must actually have walked. A run count of zero
    // would mean every seat landed in a wall and the assertion below proved
    // nothing.
    assert!(
        runs >= 20,
        "the probe never got moving ({runs} usable approaches) - the walk \
         itself regressed, so the softlock assertion below is vacuous"
    );
    eprintln!(
        "[koin1] {runs} approaches across {} cabinets + {} NPCs",
        cabinets.len(),
        npcs.len()
    );
    assert!(
        failures.is_empty(),
        "walking into something on the casino floor left the field - each of \
         these is a softlock (no host has a player-reachable exit at the time \
         of writing except the pad escape pinned by the sibling test):\n  {}",
        failures.join("\n  ")
    );
}

#[test]
fn every_minigame_can_be_left_by_pad() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let mut entered = 0usize;
    for slot in MinigameSubId::ALL {
        if !slot.is_playable() {
            continue; // the two dev modules install no session
        }
        let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
        host.enter_field_scene("koin1", 0).expect("enter koin1");

        // Enter exactly the way a cabinet's script does: publish the sub-id
        // and let the host's mode-24 init drain it. Nothing calls `enter_*`,
        // so this measures the entry a player would actually reach.
        host.world.arm_minigame_warp();
        host.world.pending_minigame_warp = Some(slot.sub_id());
        let _ = host.tick();
        let mode = host.world.mode;
        if mode == SceneMode::Field {
            // The overlay or its tables did not resolve - the drain's
            // LoadFailed arm already completed the round trip, which is the
            // documented behaviour. Nothing to escape from.
            eprintln!("[{slot:?}] overlay did not install a session - skipped");
            continue;
        }
        entered += 1;

        // Idling must not leave on its own, or the escape below would be
        // measuring the session ending rather than the pad.
        for _ in 0..30 {
            let _ = host.tick();
        }
        assert_eq!(
            host.world.mode, mode,
            "{slot:?}: still inside before the escape press"
        );

        host.world.set_pad(PadButton::Start.mask());
        let _ = host.tick();
        host.world.set_pad(0);
        for _ in 0..3 {
            let _ = host.tick();
        }
        assert_eq!(
            host.world.mode,
            SceneMode::Field,
            "{slot:?} ({mode:?}): a pad press must get the player back out - a \
             minigame the player can enter and cannot leave is a softlock"
        );
        assert_eq!(
            host.world.active_scene_label, "koin1",
            "{slot:?}: the mode-24 round trip restored the departure scene"
        );
        eprintln!("[{slot:?}] entered {mode:?} and left it by pad");
    }
    assert!(
        entered >= 4,
        "only {entered} minigames installed a session - the escape assertion \
         above is close to vacuous"
    );
}
