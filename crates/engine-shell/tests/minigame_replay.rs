//! Minigame replay: the five minigames reached through **the disc's own door
//! bytecode**, then played by pad, and scored.
//!
//! ## Why this instrument exists
//!
//! `crates/engine-core/tests/{dance,fishing,slot,baka,muscle_dome}_minigame_real.rs`
//! already drive every one of these engines with real disc tables and a real
//! pad stream. They prove the *rules* work. They say nothing about whether a
//! player can get to them, because each one calls `World::enter_*` directly -
//! the same entry the native host's debug hotkeys and the browser's fishing
//! button use. Under that denominator "the minigames are covered" and "the
//! minigames are unreachable" are the same green suite.
//!
//! This ladder is denominated in the disc instead. Every rung starts from the
//! venue scene's real MAN, locates the real door record, and executes the real
//! field-VM bytecode; nothing calls `enter_*`. The retail chain it walks (see
//! [`legaia_engine_core::minigame_entry`], and `docs/subsystems/script-vm.md`
//! § `0x3E WARP`) is:
//!
//! ```text
//! field-VM op 0x3E, op0 >= 100   ->  sub_id = op0 - 100, game mode = 24
//!   -> FUN_80025980 loads PROT (sub_id + 0x4D + 0x37F)
//!   -> the overlay's init spawns the minigame's actor template
//!   -> FUN_80026018 returns to the departure scene
//! ```
//!
//! ## The ladder
//!
//! Rungs are ordered and cumulative - the run stops at the first one it cannot
//! clear, and the score is the count it cleared. Each rung is scored across
//! *all five* minigames, so a rung clears only when every one of them does; a
//! partial rung is reported per-minigame and the run stops there.
//!
//! | # | rung | what it proves |
//! |---|---|---|
//! | 1 | every venue's door record is located on the disc | the entry sites exist and the walk frames them |
//! | 2 | each door's bytecode publishes its `sub_id` | the ported op-`0x3E` arm routes to the door warp, not to a scene change |
//! | 3 | the host drain enters each minigame's scene mode | the mode-24 init is ported: overlay read, tables parsed, session installed |
//! | 4 | each minigame advances under a pad stream | the entered session is live, not a mode flag |
//! | 5 | each returns to the field | the round trip closes; no minigame is a one-way door |
//!
//! Rungs 2 and 3 are split because they fail for different reasons and the
//! difference is the whole point: a rung-2 failure is a **VM** defect (the arm
//! routed the sub-id somewhere else), a rung-3 failure is a **host** defect
//! (the overlay or its tables did not resolve). Collapsed into one rung, "the
//! script never armed" and "the script armed and the host dropped it" would be
//! the same number.
//!
//! ## A stall is the finding
//!
//! Following `critical_path_replay`'s contract: a rung that does not clear is
//! reported with the minigame and the reason it died on, not as a bare
//! assertion failure.
//!
//! ## What rung 2 does *not* claim
//!
//! The door records are executed by loading the record's own body into the
//! field VM ([`World::load_field_script_at`]) - the retail bytecode, stepped by
//! the ported VM. What is *not* exercised is the dispatch that runs that record
//! in retail (the interaction probe resuming the placement's script). The port
//! has no general "interact runs the placement's partition-1 record" path -
//! `World::trigger_field_interact` opens inline dialogue and runs a record only
//! for boss stagers - so the ladder supplies the trigger and the disc supplies
//! everything after it. Rung 2 therefore reads "the door's bytecode drives the
//! port into the minigame", not "walking up and pressing X does".
//!
//! The runner executes each record from its start and, when the prologue does
//! not reach the warp, retries from the warp instruction's own PC - still the
//! disc's bytes through the ported VM, minus the prologue. Which path each door
//! took is printed, because "cleared through its own prologue" and "cleared
//! only past it" are different facts.
//!
//! **Every door currently clears only past its prologue**, and the stall is
//! reported as `(pc, opcode)`. All five come to rest inside the attendant's
//! conversation - four on `0x1F` (a text-segment byte, not an opcode) and the
//! two `koin1` cabinets on `0xAB` (the `0x80` cross-context prefix on op
//! `0x2B`). Enabling `World::use_vm_dialogue` does not move any of them, which
//! is itself a measurement: the inline-script runner is reached through
//! `trigger_field_interact`'s dialogue install, not through a script loaded
//! with `load_field_script_at`, so on this path it never engages. Making a
//! door clear through its own prologue needs the placement-interaction
//! dispatch, not a deeper fix in the warp arm.
//!
//! ## Ratchet
//!
//! `scripts/replays/minigame_replay_baseline.toml` carries the highest rung
//! reached so far. The test asserts `score >= reached`; raising it is a
//! reviewed edit, never an auto-write.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset or
//! `extracted/` missing.

use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::man_field_scripts::{partition_record_span, scene_man_carriers};
use legaia_engine_core::minigame_entry::MinigameSubId;
use legaia_engine_core::scene::{MinigameWarpOutcome, ProtIndex, Scene, SceneHost};
use legaia_engine_core::world::SceneMode;
use legaia_engine_vm::field_disasm::{InsnInfo, LinearWalker};

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn extracted_dir() -> Option<PathBuf> {
    let d = repo_root().join("extracted");
    if d.is_dir() {
        Some(d)
    } else {
        eprintln!("[skip] extracted/ missing - run legaia-extract first");
        None
    }
}

fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir()?;
    Some(SceneHost::open_extracted(&extracted).expect("open SceneHost"))
}

fn baseline_path() -> PathBuf {
    repo_root().join("scripts/replays/minigame_replay_baseline.toml")
}

/// Parse the `reached = N` line out of the baseline. A missing file reads as
/// `0`, so a fresh clone starts from "no progress claimed".
fn read_baseline() -> usize {
    let Ok(text) = std::fs::read_to_string(baseline_path()) else {
        return 0;
    };
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("reached")
            && let Some(v) = rest.trim_start().strip_prefix('=')
            && let Ok(n) = v.trim().parse::<usize>()
        {
            return n;
        }
    }
    0
}

// ---------------------------------------------------------------------------
// Door discovery
// ---------------------------------------------------------------------------

/// One located mode-24 door-warp site: where its record lives and where in
/// that record's body the `0x3E` instruction sits.
#[derive(Debug, Clone)]
struct DoorSite {
    scene: String,
    slot: MinigameSubId,
    partition: usize,
    record: usize,
    /// MAN payload of the carrier the record lives in.
    man: std::sync::Arc<Vec<u8>>,
    /// Record body span, and the record-relative PC the walk starts at.
    script_start: usize,
    body_len: usize,
    pc0: usize,
    /// Record-relative PC of the `0x3E` warp instruction.
    warp_pc: usize,
}

/// The venue scenes the disc-wide census resolves door sites in. Scanning
/// only these keeps the ladder fast; the exhaustive corpus sweep is
/// `engine-core`'s `minigame_entry_census_disc`, which is what proves this
/// list is the complete set.
const VENUE_SCENES: &[&str] = &["koin1", "koin3", "balden", "map02", "map03"];

/// Locate every door site in the venue scenes, keyed by minigame slot (first
/// site per slot wins - the cabinets in one room are interchangeable doors).
fn find_doors(index: &ProtIndex) -> Vec<DoorSite> {
    let mut out: Vec<DoorSite> = Vec::new();
    for name in VENUE_SCENES {
        let Ok(scene) = Scene::load(index, name) else {
            continue;
        };
        for carrier in scene_man_carriers(index, &scene) {
            let man = std::sync::Arc::new(carrier.payload.clone());
            let Ok(man_file) = legaia_asset::man_section::parse(&man) else {
                continue;
            };
            for partition in 0..man_file.header.partition_counts.len() {
                let records = man_file
                    .header
                    .partition_counts
                    .get(partition)
                    .copied()
                    .unwrap_or(0)
                    .max(0) as usize;
                for record in 0..records {
                    let Some((script_start, pc0, body_len)) =
                        partition_record_span(&man_file, &man, partition, record)
                    else {
                        continue;
                    };
                    let body = &man[script_start..script_start + body_len];
                    for insn in LinearWalker::new(body, pc0).flatten() {
                        let InsnInfo::WarpOrInteract {
                            op0, is_warp: true, ..
                        } = insn.info
                        else {
                            continue;
                        };
                        if insn.extended.is_some() {
                            continue;
                        }
                        let Some(slot) = MinigameSubId::from_op0(op0) else {
                            continue;
                        };
                        if out.iter().any(|d| d.slot == slot) {
                            continue;
                        }
                        out.push(DoorSite {
                            scene: (*name).to_string(),
                            slot,
                            partition,
                            record,
                            man: man.clone(),
                            script_start,
                            body_len,
                            pc0,
                            warp_pc: insn.pc,
                        });
                    }
                }
            }
        }
    }
    out.sort_by_key(|d| d.slot);
    out
}

// ---------------------------------------------------------------------------
// Driving
// ---------------------------------------------------------------------------

/// Frames to let a door record run before calling it stalled. The koin1
/// cabinets fade and wait ~38 frames before the warp; this is comfortable
/// headroom over that.
const DOOR_FRAMES: usize = 240;

/// How the door's bytecode was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoorPath {
    /// The record ran from its start and reached the warp through its own gate.
    ThroughGate,
    /// The record's own prologue never reached the warp; re-run from the warp's
    /// own PC. `(pc, opcode)` is where the full-record run came to rest - the
    /// stall, reported with the instruction it died on.
    PastGate { stalled_at: (usize, u8) },
    /// Neither path armed the warp.
    Stalled,
}

/// Seed the world so a gated door has something to spend: casino coins, party
/// gold, and a broad inventory. Retail gates read an inventory item and debit
/// a coin; this is the "player who has been to the casino before" state.
fn seed_venue_state(host: &mut SceneHost) {
    host.world.casino_coins = 5_000;
    host.world.money = 50_000;
    for id in 0u8..=255 {
        host.world.inventory.insert(id, 20);
    }
}

/// Load `door`'s record body into the field VM at `pc` and tick until the warp
/// arms or `DOOR_FRAMES` elapse. Returns the published `sub_id`, if any.
///
/// The pad alternates release / `Cross`, because a venue door is a
/// **conversation**: the attendant's record opens a dialogue and a yes/no
/// picker before it fades and warps, and a run that only ever holds pad `0`
/// parks on the first page forever. Confirming is what a player does, so the
/// stream confirms.
fn run_door_at(host: &mut SceneHost, door: &DoorSite, pc: usize) -> Option<u8> {
    let body = door.man[door.script_start..door.script_start + door.body_len].to_vec();
    host.world.pending_minigame_warp = None;
    host.last_minigame_warp = None;
    host.world.load_field_script_at(body, pc);
    for frame in 0..DOOR_FRAMES {
        if let Some(sub) = host.world.pending_minigame_warp {
            return Some(sub);
        }
        // Two frames released, one pressed: an edge every third frame, which
        // advances a page without double-confirming a picker row.
        let mask = if frame % 3 == 2 {
            PadButton::Cross.mask()
        } else {
            0
        };
        host.world.set_pad(mask);
        let _ = host.tick();
        if let Some(sub) = host.world.pending_minigame_warp {
            return Some(sub);
        }
        // The drain runs inside `SceneHost::tick`, so on the frame the warp
        // fires `pending_minigame_warp` has already been consumed. Read the
        // *observed* slot back out of the drain's own outcome rather than
        // assuming the expected one - otherwise rung 2 would assert what it
        // was told instead of what ran.
        if let Some(outcome) = host.last_minigame_warp {
            return match outcome {
                MinigameWarpOutcome::Entered(s)
                | MinigameWarpOutcome::DevSlotSkipped(s)
                | MinigameWarpOutcome::LoadFailed(s) => Some(s.sub_id()),
                MinigameWarpOutcome::UnknownSubId(s) => Some(s),
            };
        }
    }
    None
}

/// Run `door` through the VM, first from the record start and then - if the
/// prologue never reached the warp - from the warp instruction itself.
fn arm_door(host: &mut SceneHost, door: &DoorSite) -> (DoorPath, Option<u8>) {
    seed_venue_state(host);
    if let Some(sub) = run_door_at(host, door, door.pc0) {
        return (DoorPath::ThroughGate, Some(sub));
    }
    // Where the full-record run came to rest, and the opcode sitting there.
    let rest_pc = host.world.field_pc;
    let rest_op = door
        .man
        .get(door.script_start + rest_pc)
        .copied()
        .unwrap_or(0);
    if let Some(sub) = run_door_at(host, door, door.warp_pc) {
        return (
            DoorPath::PastGate {
                stalled_at: (rest_pc, rest_op),
            },
            Some(sub),
        );
    }
    (DoorPath::Stalled, None)
}

/// One pad edge: a release frame then a pressed frame, because `InputState`
/// computes edges itself and a held mask is a single event.
fn tap(host: &mut SceneHost, button: PadButton) {
    host.world.set_pad(0);
    let _ = host.tick();
    host.world.set_pad(button.mask());
    let _ = host.tick();
}

/// Hold `mask` for `frames` frames (fishing's reel inputs are *held*, not
/// tapped - tapping them never raises tension).
fn hold(host: &mut SceneHost, mask: u16, frames: usize) {
    for _ in 0..frames {
        host.world.set_pad(mask);
        let _ = host.tick();
    }
}

/// Play the entered minigame for a bounded number of frames and report whether
/// it visibly advanced. "Advanced" is per-game: a scored note, a hooked fish, a
/// resolved spin, a landed exchange, a committed turn.
fn play_minigame(host: &mut SceneHost, slot: MinigameSubId) -> Result<String, String> {
    match slot {
        MinigameSubId::Dance => {
            let before = host
                .world
                .dance
                .as_ref()
                .map(|g| g.score())
                .ok_or("dance session absent")?;
            // Edge-triggered Triangle / Square / Circle; the song ends itself.
            for i in 0..600 {
                let b = match i % 3 {
                    0 => PadButton::Triangle,
                    1 => PadButton::Square,
                    _ => PadButton::Circle,
                };
                tap(host, b);
                if host.world.dance.as_ref().is_some_and(|g| g.song_over()) {
                    break;
                }
            }
            let g = host.world.dance.as_ref().ok_or("dance session vanished")?;
            let judged = host.world.dance_last_judge.is_some();
            if g.score() != before || judged || g.song_over() {
                Ok(format!(
                    "score {} -> {}, judged={judged}, song_over={}",
                    before,
                    g.score(),
                    g.song_over()
                ))
            } else {
                Err("dance ran 600 frames with no judged press and no score change".into())
            }
        }
        MinigameSubId::Fishing => {
            use legaia_engine_core::fishing::FishingPhase;
            // Casting auto-advances; Cross locks the cast and hooks a fish.
            for _ in 0..8 {
                host.world.set_pad(0);
                let _ = host.tick();
            }
            tap(host, PadButton::Cross);
            let phase = host
                .world
                .fishing
                .as_ref()
                .map(|s| s.phase())
                .ok_or("fishing session absent")?;
            if phase != FishingPhase::Fighting {
                return Err(format!("cast lock left phase {phase:?}, expected Fighting"));
            }
            // Reel A is a *held* input, not a tap.
            hold(host, PadButton::Cross.mask(), 400);
            let s = host
                .world
                .fishing
                .as_ref()
                .ok_or("fishing session vanished")?;
            Ok(format!(
                "hooked and reeled to phase {:?}, points {}",
                s.phase(),
                s.record().points
            ))
        }
        MinigameSubId::SlotMachine => {
            use legaia_engine_core::slot_machine::SlotPhase;
            let start = host
                .world
                .slot_machine
                .as_ref()
                .map(|m| m.balance())
                .ok_or("slot session absent")?;
            // Cross does everything: spin, stop each reel, collect.
            for _ in 0..400 {
                tap(host, PadButton::Cross);
                let Some(m) = host.world.slot_machine.as_ref() else {
                    return Err("slot session vanished mid-spin".into());
                };
                if m.balance() != start && m.phase() == SlotPhase::Idle {
                    break;
                }
            }
            let m = host.world.slot_machine.as_ref().ok_or("slot vanished")?;
            if m.balance() == start {
                Err(format!(
                    "400 Cross edges left the balance at {start} (phase {:?})",
                    m.phase()
                ))
            } else {
                Ok(format!("balance {start} -> {}", m.balance()))
            }
        }
        MinigameSubId::BakaFighter => {
            let before = host
                .world
                .baka_fighter
                .as_ref()
                .map(|f| f.round())
                .ok_or("baka session absent")?;
            // Left / Right / Up = attacks A/B/C, Down = special.
            let mut resolved = false;
            for i in 0..400 {
                let b = match i % 4 {
                    0 => PadButton::Left,
                    1 => PadButton::Right,
                    2 => PadButton::Up,
                    _ => PadButton::Down,
                };
                tap(host, b);
                let Some(f) = host.world.baka_fighter.as_ref() else {
                    break;
                };
                resolved |= f.last_exchange().is_some();
                if f.match_over() {
                    break;
                }
            }
            let f = host.world.baka_fighter.as_ref().ok_or("baka vanished")?;
            if !resolved && f.round() == before {
                Err("400 direction edges resolved no exchange".into())
            } else {
                Ok(format!(
                    "round {before} -> {}, exchange_resolved={resolved}, over={}",
                    f.round(),
                    f.match_over()
                ))
            }
        }
        MinigameSubId::MuscleDome => {
            let before = host
                .world
                .muscle_dome
                .as_ref()
                .map(|s| s.turn())
                .ok_or("dome session absent")?;
            // Left/Right/Up/Down commit hand slots 0..3; Cross confirms.
            for _ in 0..80 {
                for b in [
                    PadButton::Left,
                    PadButton::Right,
                    PadButton::Up,
                    PadButton::Down,
                ] {
                    tap(host, b);
                }
                tap(host, PadButton::Cross);
                let Some(s) = host.world.muscle_dome.as_ref() else {
                    break;
                };
                if s.turn() != before {
                    break;
                }
            }
            let s = host.world.muscle_dome.as_ref().ok_or("dome vanished")?;
            if s.turn() == before {
                Err(format!(
                    "80 commit rounds left the dome on turn {before} (phase {:?})",
                    s.phase()
                ))
            } else {
                Ok(format!("turn {before} -> {}", s.turn()))
            }
        }
        MinigameSubId::Other2 | MinigameSubId::Other3 => {
            Err("dev slot has no shipped gameplay".into())
        }
    }
}

/// Leave the entered minigame and return to the field.
///
/// Baka is the exception to plain suspend/restore: its exit runs the mode-24
/// return warp from inside the tick and forces `SceneMode::Field` when the
/// session was entered from Field.
fn exit_minigame(host: &mut SceneHost, slot: MinigameSubId) -> Result<(), String> {
    match slot {
        MinigameSubId::Dance => {
            host.world.exit_dance();
        }
        MinigameSubId::Fishing => {
            host.world.exit_fishing();
        }
        MinigameSubId::SlotMachine => {
            host.world.exit_slot_machine();
        }
        MinigameSubId::BakaFighter => {
            // The tick's own exit: any face button fast-forwards a finished
            // match, then Cross leaves.
            for _ in 0..120 {
                tap(host, PadButton::Cross);
                if host.world.mode != SceneMode::BakaFighter {
                    break;
                }
            }
            if host.world.mode == SceneMode::BakaFighter {
                host.world.exit_baka_fighter();
            }
        }
        MinigameSubId::MuscleDome => {
            for _ in 0..120 {
                tap(host, PadButton::Cross);
                if host.world.mode != SceneMode::MuscleDome {
                    break;
                }
            }
            if host.world.mode == SceneMode::MuscleDome {
                host.world.exit_muscle_dome();
            }
        }
        MinigameSubId::Other2 | MinigameSubId::Other3 => {}
    }
    if host.world.mode == SceneMode::Field {
        Ok(())
    } else {
        Err(format!("exit left the world in {:?}", host.world.mode))
    }
}

// ---------------------------------------------------------------------------
// The ladder
// ---------------------------------------------------------------------------

/// Everything the ladder learned about one minigame.
#[derive(Debug)]
struct Leg {
    slot: MinigameSubId,
    door: Option<DoorSite>,
    door_path: DoorPath,
    armed_sub_id: Option<u8>,
    entered: Option<SceneMode>,
    played: Option<Result<String, String>>,
    returned: Option<Result<(), String>>,
}

fn run_ladder(host: &mut SceneHost, index: &ProtIndex) -> (usize, Vec<Leg>) {
    let doors = find_doors(index);
    let playable: Vec<MinigameSubId> = MinigameSubId::ALL
        .into_iter()
        .filter(|s| s.is_playable())
        .collect();

    let mut legs: Vec<Leg> = playable
        .iter()
        .map(|&slot| Leg {
            slot,
            door: doors.iter().find(|d| d.slot == slot).cloned(),
            door_path: DoorPath::Stalled,
            armed_sub_id: None,
            entered: None,
            played: None,
            returned: None,
        })
        .collect();

    // Rung 1: every playable minigame has a door on the disc.
    if legs.iter().any(|l| l.door.is_none()) {
        return (0, legs);
    }

    // Rungs 2..5, per leg. Each leg re-enters its venue scene from scratch so
    // one leg's residue cannot carry into the next.
    for leg in &mut legs {
        let Some(door) = leg.door.clone() else {
            continue;
        };
        if host.enter_field_scene(&door.scene, 0).is_err() {
            continue;
        }
        host.world.mode = SceneMode::Field;
        let (path, armed) = arm_door(host, &door);
        leg.door_path = path;
        leg.armed_sub_id = armed;
        if armed != Some(leg.slot.sub_id()) {
            continue;
        }
        // The drain runs inside `SceneHost::tick`.
        for _ in 0..8 {
            if host.world.mode != SceneMode::Field {
                break;
            }
            host.world.set_pad(0);
            let _ = host.tick();
        }
        if host.world.mode == SceneMode::Field {
            continue;
        }
        leg.entered = Some(host.world.mode);
        if host.world.mode != leg.slot.scene_mode().unwrap() {
            continue;
        }
        leg.played = Some(play_minigame(host, leg.slot));
        leg.returned = Some(exit_minigame(host, leg.slot));
    }

    let rung2 = legs.iter().all(|l| l.armed_sub_id == Some(l.slot.sub_id()));
    let rung3 = rung2 && legs.iter().all(|l| l.entered == l.slot.scene_mode());
    let rung4 = rung3 && legs.iter().all(|l| matches!(&l.played, Some(Ok(_))));
    let rung5 = rung4 && legs.iter().all(|l| matches!(&l.returned, Some(Ok(()))));

    let score = 1 + [rung2, rung3, rung4, rung5]
        .iter()
        .take_while(|ok| **ok)
        .count();
    (score, legs)
}

#[test]
fn minigames_are_reachable_by_pad_through_the_disc_door_bytecode() {
    let Some(mut host) = open_host() else { return };
    let index = host.index.clone();
    let (score, legs) = run_ladder(&mut host, &index);

    eprintln!("\n[minigame replay] ladder score {score} / 5");
    for leg in &legs {
        match &leg.door {
            Some(d) => eprintln!(
                "  {:<13} door {}/P{}[{}] @+0x{:X} (warp @+0x{:X}) path={:?} armed={:?}",
                leg.slot.label(),
                d.scene,
                d.partition,
                d.record,
                d.pc0,
                d.warp_pc,
                leg.door_path,
                leg.armed_sub_id,
            ),
            None => eprintln!("  {:<13} NO DOOR FOUND", leg.slot.label()),
        }
        eprintln!(
            "  {:<13}   entered={:?} played={:?} returned={:?}",
            "", leg.entered, leg.played, leg.returned
        );
    }

    let baseline = read_baseline();
    if score > baseline {
        eprintln!(
            "\n[minigame replay] score rose {baseline} -> {score}. To ratchet, set \
             `reached = {score}` in {}",
            baseline_path().display()
        );
    }
    assert!(
        score >= baseline,
        "minigame replay regressed: reached rung {score}, baseline {baseline}. \
         Legs: {legs:#?}"
    );
    // Non-vacuity: a run that located no door at all would otherwise score 0
    // against a 0 baseline and pass silently.
    assert!(
        legs.iter().all(|l| l.door.is_some()),
        "a playable minigame has no door site in {VENUE_SCENES:?} - the ladder \
         cannot be denominated in what a player reaches. Legs: {legs:#?}"
    );
}
