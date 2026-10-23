//! One soak run: its spec, stats and tunables, the value detectors, the
//! host-side drains, and `run_one` which plays a scene under a policy with
//! every detector armed.

use super::*;
use legaia_engine_save_state::WorldSaveState;

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub(super) struct RunSpec {
    pub(super) scene: String,
    pub(super) seed: u64,
    pub(super) frames: u64,
}

#[derive(Default, Clone, Debug)]
pub(super) struct RunStats {
    pub(super) frames_run: u64,
    pub(super) battles: u32,
    pub(super) battles_won: u32,
    pub(super) wiped: bool,
    /// Party wipes the run played on from: the return-to-title hold ran to
    /// its resolve and the card's latest save was resumed (Continue).
    pub(super) wipe_continues: u32,
    /// `+mg` runs: door warps back into the minigame after it was left.
    pub(super) minigame_revisits: u32,
    pub(super) scenes_entered: BTreeSet<String>,
    pub(super) modes: BTreeSet<String>,
    /// Frames ticked in each world mode: whether a run that reached a mode
    /// also stayed in it long enough to soak it.
    pub(super) mode_frames: BTreeMap<String, u64>,
    /// Battle surfaces a run reached: each command-session phase it saw,
    /// and the item / spell / arts pickers it opened.
    pub(super) battle_surfaces: BTreeSet<String>,
    pub(super) menu_opens: u32,
    pub(super) fmvs: u32,
    pub(super) save_commits: u32,
    /// Pause-menu sub-screens entered and shops opened - what the random
    /// browse actually reached.
    pub(super) menu_rows: BTreeSet<String>,
    pub(super) shop_opens: u32,
    pub(super) round_trips: u32,
    /// `+talk` runs: visits attempted, and those that found a seat the
    /// interact probe answers from.
    pub(super) talk_visits: u32,
    pub(super) talk_seats: u32,
    /// Seats taken at an examined prop rather than an actor.
    pub(super) prop_seats: u32,
    /// Seats whose action press took the frame off free roam.
    pub(super) talk_opens: u32,
}

pub(super) struct RunOutcome {
    pub(super) spec: RunSpec,
    pub(super) findings: Vec<Finding>,
    pub(super) pads: Vec<u16>,
    pub(super) stats: RunStats,
    pub(super) enter_ok: bool,
    /// The run's [`Tunables::arming`]: a confirmation or reduction
    /// replays in the same configuration.
    pub(super) arming: Arming,
}

pub(super) struct Tunables {
    pub(super) softlock_frames: u64,
    pub(super) digest_every: u64,
    pub(super) menu_close_frames: u64,
    pub(super) battle_frames: u64,
    /// `LEGAIA_SOAK_BATTLE_HP_POOL`: the monster HP left above which a fight
    /// still landing actions outlasts `battle_frames` without being endless.
    pub(super) battle_hp_pool: u32,
    pub(super) queue_cap: usize,
    /// `LEGAIA_SOAK_TRACE=N`: print a state line every N frames (replay
    /// triage; 0 = off).
    pub(super) trace_every: u64,
    /// Distinct pad masks the softlock window must contain. `1` turns the
    /// detector into a neutral-pad control (`LEGAIA_SOAK_MIN_DISTINCT=1`).
    pub(super) min_distinct: usize,
    /// Field frames without one free-roam frame before `control_starved`.
    pub(super) starve_frames: u64,
    /// Arm what both play hosts arm on a bare session
    /// ([`environment::arm_host_play_defaults`]). Off under
    /// `LEGAIA_SOAK_BARE=1`, and for a replay recorded before the soak armed
    /// it (no `# soak-arming` header): such a replay is judged in the
    /// configuration that recorded it. `LEGAIA_SOAK_RETAIL=1` adds the
    /// hosts' retail mode on top ([`Arming::retail`]).
    pub(super) arming: Arming,
}

/// Frames between two re-entries of a `+mg` run's minigame, counted on the
/// run clock; a re-entry waits for free roam in the venue.
pub(super) const MINIGAME_REVISIT_EVERY: u64 = 300;

/// Hold frames a party wipe may take before `wipe_continue` fires: the
/// hold's own length is a short fixed fade (`GameOverSession::new`).
const WIPE_HOLD_MAX_FRAMES: u32 = 600;

/// After a party wipe: run the return-to-title hold both hosts run
/// (`BootSession::take_party_wipe`, then `step_party_wipe_hold` until it
/// resolves), check the world it leaves is no longer a battle, and resume the
/// card save at `key` the way the title's Continue -> Load does. The title
/// screen itself is the hosts' (`boot_ui`) and is not driven.
fn continue_after_wipe(
    session: &mut NativeSession,
    card: &SoakCard,
    key: (u8, u8),
    opts: &FieldLiveOpts,
) -> Result<(), (String, String)> {
    let Some(mut hold) = session.take_party_wipe(false) else {
        return Err((
            "no-hold".into(),
            "game over raised but the session handed back no wipe hold".into(),
        ));
    };
    let resolved = (0..WIPE_HOLD_MAX_FRAMES).any(|_| session.step_party_wipe_hold(&mut hold));
    let w = &session.host.world;
    if !resolved {
        return Err((
            "hold-never-resolves".into(),
            format!("wipe hold still running after {WIPE_HOLD_MAX_FRAMES} frames"),
        ));
    }
    if w.game_over_hold || w.mode == SceneMode::Battle {
        return Err((
            "hold-left-battle".into(),
            format!(
                "after the hold resolved: mode {:?}, game_over_hold {}",
                w.mode, w.game_over_hold
            ),
        ));
    }
    let (sf, resume) = card
        .get(&key)
        .cloned()
        .expect("the latest save is on the card");
    let landing = session.resume_save(sf, &resume.scene, opts);
    if !matches!(
        landing,
        legaia_engine_scene_host::resume::ResumeLanding::SavedScene(_)
    ) {
        return Err((
            "resume-landing".into(),
            format!("Continue into {:?} landed {landing:?}", resume.scene),
        ));
    }
    let w = &session.host.world;
    if w.game_over || w.game_over_hold || w.mode == SceneMode::Battle {
        return Err((
            "resume-still-wiped".into(),
            format!(
                "after Continue: mode {:?}, game_over {}, game_over_hold {}",
                w.mode, w.game_over, w.game_over_hold
            ),
        ));
    }
    Ok(())
}

/// Confirm presses a `control_starved` window must contain.
const STARVE_MIN_PRESSES: usize = 60;

/// `door_loop`: this many scene changes inside this many frames.
const DOOR_LOOP_COUNT: usize = 6;
const DOOR_LOOP_SPAN: u64 = 600;

impl Tunables {
    pub(super) fn from_env() -> Self {
        Self {
            softlock_frames: env_u64("LEGAIA_SOAK_SOFTLOCK_FRAMES").unwrap_or(1800),
            digest_every: 15,
            menu_close_frames: 900,
            battle_frames: env_u64("LEGAIA_SOAK_BATTLE_FRAMES").unwrap_or(18_000),
            battle_hp_pool: env_u64("LEGAIA_SOAK_BATTLE_HP_POOL").unwrap_or(20_000) as u32,
            queue_cap: 4096,
            trace_every: env_u64("LEGAIA_SOAK_TRACE").unwrap_or(0),
            min_distinct: env_u64("LEGAIA_SOAK_MIN_DISTINCT").unwrap_or(3) as usize,
            starve_frames: env_u64("LEGAIA_SOAK_STARVE_FRAMES").unwrap_or(5400),
            arming: Arming {
                host: std::env::var_os("LEGAIA_SOAK_BARE").is_none(),
                retail: env_flag("LEGAIA_SOAK_RETAIL"),
            },
        }
    }
}

/// Shared with the watchdog: what each worker is doing right now.
#[derive(Clone, Debug, Default)]
pub(super) struct WorkerStatus {
    pub(super) tick_started: Option<Instant>,
    pub(super) scene: String,
    pub(super) seed: u64,
    pub(super) frame: u64,
}

/// `confined`: the pocket's span in world units, and the direction-held
/// free-roam frames the player is given to leave it.
const CONFINE_SPAN: i16 = 48;
const CONFINE_WALK_FRAMES: u32 = 900;

/// Frames each d-pad direction must have been held before a `confined`
/// window may fire.
const CONFINE_DIR_MIN_FRAMES: u32 = 16;
/// The four d-pad bits.
const DIR_MASK: u16 = DIRS[0] | DIRS[1] | DIRS[2] | DIRS[3];

pub(super) type StatusBoard = Arc<Mutex<Vec<WorkerStatus>>>;

pub(super) fn value_checks(
    s: &NativeSession,
    t: &Tunables,
    out: &mut Vec<(&'static str, String, String)>,
) {
    let w = &s.host.world;
    let cam = &s.camera;
    let floats = [
        ("camera.eye", cam.eye.iter().all(|v| v.is_finite())),
        ("camera.look_at", cam.look_at.iter().all(|v| v.is_finite())),
        (
            "camera.angles",
            cam.yaw.is_finite() && cam.pitch.is_finite() && cam.roll.is_finite(),
        ),
        (
            "camera.follow",
            cam.follow_distance.is_finite() && cam.follow_height.is_finite(),
        ),
        (
            "cutscene.caption_alpha",
            w.cutscene.caption_alpha.is_finite(),
        ),
    ];
    for (field, ok) in floats {
        if !ok {
            out.push(("non_finite", field.to_string(), format!("{cam:?}")));
        }
    }
    for (i, m) in w.party.roster.members.iter().enumerate() {
        let hms = m.hp_mp_sp();
        if hms.hp_max > 0 && hms.hp_cur > hms.hp_max {
            out.push((
                "value",
                format!("roster.hp>max(slot{i})"),
                // The base window and the equipment ride along: a live max
                // the equipment's percent boosts no longer explain is the
                // first thing to rule in or out.
                format!(
                    "{}/{} (base max {}, level {}, equip {:?})",
                    hms.hp_cur,
                    hms.hp_max,
                    m.record_stats().hp_max,
                    m.level(),
                    m.equipment().slots
                ),
            ));
        }
        if hms.mp_max > 0 && hms.mp_cur > hms.mp_max {
            out.push((
                "value",
                format!("roster.mp>max(slot{i})"),
                format!("{}/{}", hms.mp_cur, hms.mp_max),
            ));
        }
        // The live maxima are the stat aggregator's rebuild of the base
        // window (`FUN_80042558`: base, plus a tenth for ability bit 0 / 2
        // and a quarter for bit 1 / 3, clamped to 9999 / 999). A live value
        // the base and the equipment do not explain was written by a path
        // that skipped the rebuild. Only checked with the disc's passive
        // table installed and on a record that carries a base window.
        let base = m.record_stats();
        if !w.tables.accessory_passives.is_empty() && base.hp_max != 0 {
            let bits = w
                .tables
                .accessory_passives
                .bits_for_equipment(&m.equipment().slots)[0];
            let rebuilt = |base: u16, tenth: u32, quarter: u32, cap: u16| {
                let mut v = base;
                if bits & tenth != 0 {
                    v = v.saturating_add(base / 10);
                }
                if bits & quarter != 0 {
                    v = v.saturating_add(base >> 2);
                }
                v.min(cap)
            };
            let (hp, mp) = (
                rebuilt(base.hp_max, 0x1, 0x2, 9999),
                rebuilt(base.mp_max, 0x4, 0x8, 999),
            );
            if hms.hp_max != hp {
                out.push((
                    "value",
                    format!("roster.hp_max!=rebuilt(slot{i})"),
                    format!(
                        "live {} against {hp} from base {} under ability bits {:#x}",
                        hms.hp_max,
                        base.hp_max,
                        bits & 0xF
                    ),
                ));
            }
            if hms.mp_max != mp {
                out.push((
                    "value",
                    format!("roster.mp_max!=rebuilt(slot{i})"),
                    format!(
                        "live {} against {mp} from base {} under ability bits {:#x}",
                        hms.mp_max,
                        base.mp_max,
                        bits & 0xF
                    ),
                ));
            }
        }
        // `+0x130` is the character level (`CharacterRecord::level`).
        let lv = m.level();
        if lv == 0 || lv > 99 {
            out.push(("value", format!("roster.level(slot{i})"), format!("{lv}")));
        }
    }
    if w.mode == SceneMode::Battle {
        for (i, a) in w.actors.iter().take(8).enumerate() {
            if a.battle.max_hp > 0 && a.battle.hp > a.battle.max_hp {
                out.push((
                    "value",
                    format!("battle.hp>max(actor{i})"),
                    format!("{}/{}", a.battle.hp, a.battle.max_hp),
                ));
            }
        }
    }
    if w.party.money < 0 || w.party.money > 99_999_999 {
        out.push(("value", "party.money".into(), format!("{}", w.party.money)));
    }
    let slots = w.party.inventory.slots();
    let (lo, hi) = w.party.inventory.window_bounds();
    let mut seen: HashMap<u8, usize> = HashMap::new();
    for (i, &(id, count)) in slots.iter().enumerate() {
        if count > 99 {
            out.push((
                "value",
                "bag.count>99".into(),
                format!("slot{i} id{id} x{count}"),
            ));
        }
        if id == 0 && count != 0 {
            out.push((
                "value",
                "bag.free-slot-count".into(),
                format!("slot{i} x{count}"),
            ));
        }
        if id != 0
            && (lo..hi).contains(&i)
            && let Some(prev) = seen.insert(id, i)
        {
            out.push((
                "value",
                "bag.duplicate-stack".into(),
                format!("id{id} slots {prev},{i}"),
            ));
        }
    }
    let queues = [
        ("pending_field_events", w.pending_field_events.len()),
        ("pending_battle_events", w.pending_battle_events.len()),
        ("pending_actor_spawns", w.pending_actor_spawns.len()),
        ("helper_contexts", w.field_vm.helper_contexts.len()),
        (
            "pending_record_spawns",
            w.field_vm.pending_record_spawns.len(),
        ),
        ("battle.hit_fx", w.battle.hit_fx.len()),
        ("battle.effect_spawns", w.battle.effect_spawns.len()),
        ("debug_effects", w.debug_effects.len()),
        ("actors", w.actors.len()),
    ];
    for (q, n) in queues {
        if n > t.queue_cap {
            let detail = if q == "pending_field_events" {
                let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
                for ev in &w.pending_field_events {
                    *kinds.entry(variant(&format!("{ev:?}"))).or_default() += 1;
                }
                format!("len {n}: {kinds:?}")
            } else {
                format!("len {n}")
            };
            out.push(("unbounded_growth", q.to_string(), detail));
        }
    }
}

/// The presentation queues both play hosts drain after every tick (the
/// window's `drain_and_route_field_events` / `drain_and_log_battle_events` /
/// CLUT-stage and effect-spawn routing, the browser runtime's twins).
/// `NativeSession::tick` routes only BGM events off the field queue and drains
/// none of the rest, so a headless driver that skipped this would measure its
/// own leak rather than the engine.
pub(super) fn host_drains(session: &mut NativeSession) {
    let w = &mut session.host.world;
    let _ = w.drain_field_events();
    let _ = w.drain_battle_events();
    let _ = w.drain_battle_hit_fx();
    let _ = w.drain_battle_hit_events();
    let _ = w.drain_battle_sfx_cues();
    let _ = w.drain_battle_shout_cues();
    let _ = w.drain_battle_xa_cues();
    let _ = w.drain_battle_xa_prestage();
    let _ = w.route_battle_effect_spawns();
    let _ = w.drain_battle_clut_stages();
    let _ = w.drain_minigame_sfx_cues();
}

pub(super) fn run_one(
    src: &Source,
    known_scenes: &BTreeSet<String>,
    spec: &RunSpec,
    pads_override: Option<&[u16]>,
    t: &Tunables,
    status: Option<(&StatusBoard, usize)>,
) -> RunOutcome {
    let mut session = open_session_armed(src, t.arming);
    session.begin_new_game();
    // `<label>@<save>`: the run plays the scene with a library card save's
    // party, bag and story flags instead of the New Game party's.
    // `title` / `title+continue[@save]`: the run enters through the boot
    // front end instead of a scene (`front_end.rs`); a Continue's save is
    // loaded off the card by the save-select, not seeded here.
    let front_end = split_front_end(&spec.scene);
    let (label, save) = split_save(&spec.scene);
    if let Some(save) = save.filter(|_| front_end.is_none()) {
        match card_save(save) {
            Ok(sf) => session.host.world.load_full(sf),
            Err(e) => {
                return RunOutcome {
                    spec: spec.clone(),
                    findings: vec![Finding {
                        detector: "tick_error",
                        start_scene: spec.scene.clone(),
                        scene: String::new(),
                        mode: String::new(),
                        frame: 0,
                        location: "card save".into(),
                        detail: e,
                    }],
                    pads: Vec::new(),
                    stats: RunStats::default(),
                    enter_ok: false,
                    arming: t.arming,
                };
            }
        }
    }
    session.set_save_rack(
        SaveRack::CardPorts(vec![
            card_port_snapshot(0, Some("MEMORY CARD")),
            card_port_snapshot(1, None),
        ]),
        vec![(0..15).map(SlotSnapshot::empty).collect(), Vec::new()],
    );
    let mut card = SoakCard::new();
    session.host.world.rng_state = (run_seed(&spec.scene, spec.seed) >> 16) as u32;
    let opts = FieldLiveOpts {
        // `LEGAIA_SOAK_NO_ENCOUNTERS=1` disarms the random-encounter roll - a
        // triage control that separates a scripted battle from a random one.
        // A `+talk` run leaves it disarmed too (`split_talk`).
        live_loop: !env_flag("LEGAIA_SOAK_NO_ENCOUNTERS") && !label.contains("+talk"),
        player_battle: true,
        battle_bgm: None,
    };
    let mut findings: Vec<Finding> = Vec::new();
    let mut stats = RunStats::default();
    let mut pads = Vec::with_capacity(spec.frames as usize);

    let mk = |session: &NativeSession,
              detector: &'static str,
              frame: u64,
              location: String,
              detail: String| Finding {
        detector,
        start_scene: spec.scene.clone(),
        scene: session.host.world.active_scene_label.clone(),
        mode: format!("{:?}", session.host.world.mode),
        frame,
        location,
        detail,
    };

    // `<venue>+mg<sub_id>`: enter the venue, then request the mode-24 door
    // warp the op-`0x3E` arm makes (`World::request_minigame_warp`), so the
    // host's next tick loads the minigame overlay through the retail path.
    let (label, rage) = split_rage(label);
    let (label, shop_visits) = split_shop(label);
    let (label, talk_visits) = split_talk(label);
    let (label, round_trip) = split_round_trip(label);
    let (scene, minigame) = split_minigame(label);
    // A label the scene set does not hold names no scene: entering it by
    // name loads an empty field (no MAN, no player) that then reads as a
    // softlock. Report it as the entry failure it is.
    if front_end.is_none() && !known_scenes.contains(scene) {
        findings.push(mk(
            &session,
            "unknown_scene",
            0,
            "enter".into(),
            format!("{scene:?} is not a scene of the soak's scene set"),
        ));
        return RunOutcome {
            spec: spec.clone(),
            findings,
            pads,
            stats,
            enter_ok: false,
            arming: t.arming,
        };
    }
    let entered = catch_unwind(AssertUnwindSafe(|| {
        if let Some(route) = &front_end {
            return enter_through_front_end(&mut session, route, &opts).map(|_| ());
        }
        let r = session
            .enter_scene_live(scene, &opts)
            .map(|_| ())
            .map_err(|e| format!("{e:#}"));
        if let Some(id) = minigame {
            // The casino doors refuse a player with no coins (the cabinet
            // record's coin compare), so a run arriving through the door
            // arrives with some: the one non-pad precondition these runs set.
            session.host.world.minigames.casino_coins = 500;
            session.host.world.request_minigame_warp(id);
        }
        r
    }));
    let enter_ok = match entered {
        Ok(Ok(_)) => true,
        Ok(Err(e)) => {
            findings.push(mk(
                &session,
                "tick_error",
                0,
                "enter_scene_live".into(),
                format!("{e:#}"),
            ));
            false
        }
        Err(_) => {
            let (loc, msg, frames) = LAST_PANIC
                .with(|p| p.borrow_mut().take())
                .unwrap_or_default();
            findings.push(mk(
                &session,
                "panic",
                0,
                loc,
                format!("[enter] {msg} | {frames}"),
            ));
            false
        }
    };
    if !enter_ok {
        return RunOutcome {
            spec: spec.clone(),
            findings,
            pads,
            stats,
            enter_ok,
            arming: t.arming,
        };
    }

    // A round-trip run starts with one save on the card, so the pause
    // menu's Load row (open in every scene; Save is per-scene) has a file
    // to resume - an empty card refuses before any commit.
    // The card cell the run last wrote: what a Continue after a party wipe
    // resumes.
    let mut latest_save: Option<(u8, u8)> = None;
    if round_trip {
        let sf = session.host.world.save_full();
        let resume = session.current_resume();
        card.insert((0, 0), (sf, resume));
        refresh_rack(&mut session, &card);
        latest_save = Some((0, 0));
    }
    let mut policy = Policy::new(run_seed(&spec.scene, spec.seed));
    let mut menu_rt = MenuRuntime::new(std::env::temp_dir().join("legaia-soak-menu"));
    let mut last_digest = progress_digest(&session) ^ menu_digest(&menu_rt);
    let mut last_change = 0u64;
    // Script stall: the modal timeline (or first helper) parked at one PC for
    // a whole window, even while something else moves.
    let mut park: Option<(String, u64, u64)> = None;
    let mut menu_open_since: Option<u64> = None;
    let mut battle_since: Option<u64> = None;
    // Endless battle: the frame a battle actor's HP / MP last moved, and what
    // it moved to.
    let mut battle_progress: Option<(u64, Vec<(u16, u16)>)> = None;
    let mut battle_pressed = 0u64;
    let mut prev_mode = session.host.world.mode;
    let mut prev_scene = session.host.world.active_scene_label.clone();
    let mut reported: BTreeSet<String> = BTreeSet::new();
    // A card save can carry values no play produces (the playthrough card's
    // bag holds a 255-count stack): whatever the seeded state already
    // violates is the save's, not the engine's, and is not reported.
    if save.is_some() {
        let mut v = Vec::new();
        value_checks(&session, t, &mut v);
        reported.extend(v.into_iter().map(|(det, loc, _)| format!("{det}|{loc}")));
    }
    let mut traced_scripts: BTreeSet<u64> = BTreeSet::new();
    let mut traced_stats: Vec<(u8, u16, u16, u16, u16)> = Vec::new();
    let mut starved_since: Option<u64> = None;
    type StarvedAt = (Option<usize>, Option<usize>, Option<usize>, Option<usize>);
    let mut starved_seen: BTreeSet<StarvedAt> = BTreeSet::new();
    let mut starved_scene = session.host.world.active_scene_label.clone();
    // Starts `true`: the cold entry seat is the harness's, not a door's.
    let mut was_free = true;
    let mut confine: Option<(i16, i16, i16, i16, u32)> = None;
    // Frames each d-pad direction (Up, Right, Down, Left) was held inside
    // the current confine window.
    let mut confine_dirs = [0u32; 4];
    let mut door_frames: std::collections::VecDeque<u64> = std::collections::VecDeque::new();
    let mut door_prev = String::new();
    let mut door_dir_held = false;
    let mut battle_sites: BTreeMap<String, u32> = BTreeMap::new();
    let mut prev_pad = 0u16;

    // `timeline_cap`: entries of `CutsceneState::capped` already reported.
    let mut capped_seen = 0usize;
    // Pads a run mode queued ahead of the policy (a talk visit's action
    // press). They land in the recorded stream like any other pad.
    let mut forced_pads: std::collections::VecDeque<u16> = std::collections::VecDeque::new();
    let mut talk_check: Option<u64> = None;

    for frame in 0..spec.frames {
        let pad = match pads_override {
            Some(p) => p.get(frame as usize).copied().unwrap_or(0),
            None => match forced_pads.pop_front() {
                Some(p) => p,
                None => policy.pad(&session, frame),
            },
        };
        pads.push(pad);
        if rage {
            give_party_rage(&mut session.host.world);
        }
        let was_menu_open = session.field_menu.is_some();
        let was_shop_open = session.host.world.shops.shop_open;
        // While a shop / prize counter is up the pad drives it, not the
        // field (the play window's `field_pad`).
        session
            .host
            .world
            .set_pad(if menu_rt.is_open() { 0 } else { pad });
        let menu_suspended = menu_rt.suspends_field();
        if let Some((board, i)) = status {
            let mut b = board.lock().unwrap();
            b[i].tick_started = Some(Instant::now());
            b[i].frame = frame;
        }
        // The name-entry overlay is host-modal on both play hosts: while it
        // is up the field tick is skipped, every pad edge routes into the
        // entry SM, and the frame counter keeps advancing (the window's
        // redraw arm, the browser's `name_entry_input`). `NativeSession::tick`
        // does not do this, so the harness does it for it.
        let edge = pad & !prev_pad;
        prev_pad = pad;
        let r = catch_unwind(AssertUnwindSafe(|| {
            if menu_suspended {
                // A shop / prize exchange freezes the field whole; only the
                // menu session steps (the play window's suspended arm).
                tick_menu_session(&mut menu_rt, &mut session.host.world, edge);
                Ok(SceneTickEvent::Stepped)
            } else if session.host.world.name_entry_active() {
                let input = legaia_engine_menus::name_entry::NameEntryInput::from_pad_edge(edge);
                session.host.world.step_name_entry(input);
                session.host.world.frame = session.host.world.frame.wrapping_add(1);
                Ok(SceneTickEvent::Stepped)
            } else {
                session.tick()
            }
        }));
        if let Some((board, i)) = status {
            board.lock().unwrap()[i].tick_started = None;
        }
        stats.frames_run = frame + 1;
        let event = match r {
            Ok(Ok(ev)) => ev,
            Ok(Err(e)) => {
                findings.push(mk(
                    &session,
                    "tick_error",
                    frame,
                    variant(&format!("{e}")),
                    format!("{e:#}"),
                ));
                break;
            }
            Err(_) => {
                let (loc, msg, frames) = LAST_PANIC
                    .with(|p| p.borrow_mut().take())
                    .unwrap_or_default();
                findings.push(mk(
                    &session,
                    "panic",
                    frame,
                    loc,
                    format!("{msg} | {frames}"),
                ));
                break;
            }
        };
        // Both play hosts drain the field-event queue after the tick (the
        // window's `drain_and_route_field_events`, the browser runtime's
        // drain); `NativeSession::tick` routes only the BGM events and restores
        // the rest, so a headless driver that skipped this would measure its
        // own leak. Checked before the drain, then drained.
        if session.host.world.pending_field_events.len() > t.queue_cap {
            let mut v = Vec::new();
            value_checks(&session, t, &mut v);
            for (det, loc, detail) in v {
                if det == "unbounded_growth" && reported.insert(format!("{det}|{loc}")) {
                    findings.push(mk(&session, det, frame, loc, detail));
                }
            }
        }
        host_drains(&mut session);
        // A shop or prize counter the tick opened goes to the menu runtime
        // and gets this frame's edge; an open session steps here every
        // unsuspended frame (the play window's frame tail).
        if !menu_suspended {
            let r = catch_unwind(AssertUnwindSafe(|| {
                let w = &mut session.host.world;
                if let Some(shop) = w.take_pending_field_shop() {
                    menu_rt.open_shop_menu(shop);
                }
                if let Some(ex) = w.take_pending_prize_exchange() {
                    menu_rt.open_prize_exchange(ex);
                }
                tick_menu_session(&mut menu_rt, w, edge);
            }));
            if r.is_err() {
                let (loc, msg, frames) = LAST_PANIC
                    .with(|p| p.borrow_mut().take())
                    .unwrap_or_default();
                findings.push(mk(
                    &session,
                    "panic",
                    frame,
                    loc,
                    format!("[menu runtime] {msg} | {frames}"),
                ));
                break;
            }
        }
        match &event {
            SceneTickEvent::SceneEntered { name } => {
                stats.scenes_entered.insert(name.clone());
                if !known_scenes.contains(name) {
                    findings.push(mk(
                        &session,
                        "unknown_scene",
                        frame,
                        format!("label:{name}"),
                        "entered a label outside the playable scene set".into(),
                    ));
                }
            }
            SceneTickEvent::UnknownMapId { map_id } => {
                findings.push(mk(
                    &session,
                    "unknown_scene",
                    frame,
                    format!("map_id:{map_id}"),
                    "scene transition to a map id with no scene".into(),
                ));
            }
            SceneTickEvent::Stepped => {}
        }
        if !was_menu_open && session.field_menu.is_some() {
            stats.menu_opens += 1;
        }
        if let Some(sub) = session.field_menu_sub.as_ref() {
            stats.menu_rows.insert(format!("{:?}", sub.row()));
        }
        if session.host.world.shops.shop_open && !was_shop_open {
            stats.shop_opens += 1;
        }
        // Save screen: both play hosts persist a Save pick and resume a Load
        // pick (`apply_save_commit`); `NativeSession` only latches the pick.
        // The harness keeps the card in memory, so a later Load in the same
        // run reads back what an earlier Save wrote.
        if let Some(commit) = session.last_save_commit.take() {
            stats.save_commits += 1;
            if matches!(
                commit.kind,
                legaia_engine_menus::save_screen::SaveCommitKind::Save
            ) {
                latest_save = Some((commit.port, commit.cell));
            }
            let r = catch_unwind(AssertUnwindSafe(|| {
                apply_save_commit(&mut session, &mut card, commit, &opts)
            }));
            if r.is_err() {
                let (loc, msg, frames) = LAST_PANIC
                    .with(|p| p.borrow_mut().take())
                    .unwrap_or_default();
                findings.push(mk(
                    &session,
                    "panic",
                    frame,
                    loc,
                    format!("[save commit] {msg} | {frames}"),
                ));
                break;
            }
        }
        // `<scene>+shop`: walk up to one of the scene's merchants every
        // `SHOP_VISIT_EVERY` free field frames - the priced session the
        // field VM's op `0x49` stages, handed to the menu runtime as the play
        // window hands it. Random walking almost never reaches a merchant and
        // picks Buy, so without this the shop UI is never soaked.
        if shop_visits
            && !menu_rt.is_open()
            && frame % SHOP_VISIT_EVERY == SHOP_VISIT_EVERY - 1
            && round_trip_ready(&session)
        {
            let w = &mut session.host.world;
            let n = w.shops.scene_shops.len();
            if n > 0
                && let Some(shop) = w.scene_shop_session(stats.shop_opens as usize % n)
            {
                w.shops.shop_open = true;
                menu_rt.open_shop_menu(shop);
                stats.shop_opens += 1;
            }
        }
        // `<venue>+mg<sub_id>`: back through the cabinet door every
        // `MINIGAME_REVISIT_EVERY` frames the party stands free in the venue.
        // The policy's random face buttons leave most minigames within a few
        // hundred frames (a cancel in a hub, a lost round), so without this a
        // minigame run spends most of its budget on the casino floor. Coins
        // are topped up for the door's compare, as at entry.
        if let Some(id) = minigame
            && frame % MINIGAME_REVISIT_EVERY == MINIGAME_REVISIT_EVERY - 1
            && session.host.world.mode == SceneMode::Field
            && session.host.world.active_scene_label == scene
            && round_trip_ready(&session)
            && !menu_rt.is_open()
        {
            let w = &mut session.host.world;
            w.minigames.casino_coins = w.minigames.casino_coins.max(500);
            w.request_minigame_warp(id);
            stats.minigame_revisits += 1;
        }
        // `<scene>+talk`: stand in front of the scene's next talk target
        // every `TALK_VISIT_EVERY` free field frames and press the action
        // button. The seat is a function of the frame and the world, so a
        // replay of the recorded pads takes the same seats.
        if talk_visits
            && !menu_rt.is_open()
            && frame % TALK_VISIT_EVERY == TALK_VISIT_EVERY - 1
            && round_trip_ready(&session)
        {
            let n = stats.talk_visits as usize;
            stats.talk_visits += 1;
            if let Some(target) = seat_at_talk_target(&mut session, n) {
                stats.talk_seats += 1;
                if matches!(target, TalkTarget::Prop(_)) {
                    stats.prop_seats += 1;
                }
                forced_pads.extend([0, b(PadButton::Cross), 0]);
                talk_check = Some(frame + 4);
            }
        }
        if talk_check == Some(frame) {
            talk_check = None;
            if !round_trip_ready(&session) || session.host.world.script_context_engages_player() {
                stats.talk_opens += 1;
            }
        }
        // `<scene>+rt`: a save / load round trip at a free field frame every
        // `ROUND_TRIP_EVERY` frames - `save_full`, then `resume_save` of that
        // file the way a Load resumes it, then `save_full` again. What the
        // second save says that the first did not is state the file does not
        // carry, or a resume that does not restore what it read.
        if round_trip
            && frame % ROUND_TRIP_EVERY == ROUND_TRIP_EVERY - 1
            && round_trip_ready(&session)
        {
            stats.round_trips += 1;
            let r = catch_unwind(AssertUnwindSafe(|| {
                let before = session.host.world.save_full();
                let resume = session.current_resume();
                let _ = session.resume_save(before.clone(), &resume.scene, &opts);
                let after = session.host.world.save_full();
                save_diff(&before, &after)
            }));
            match r {
                Ok(Some((field, detail))) => {
                    if reported.insert(format!("save_roundtrip|{field}")) {
                        findings.push(mk(&session, "save_roundtrip", frame, field, detail));
                    }
                }
                Ok(None) => {}
                Err(_) => {
                    let (loc, msg, frames) = LAST_PANIC
                        .with(|p| p.borrow_mut().take())
                        .unwrap_or_default();
                    findings.push(mk(
                        &session,
                        "panic",
                        frame,
                        loc,
                        format!("[round trip] {msg} | {frames}"),
                    ));
                    break;
                }
            }
        }
        // FMV: skip the movie, then run the shared post-play hand-off - the
        // headless `play` subcommand's order.
        let fmv = catch_unwind(AssertUnwindSafe(|| {
            if session.host.world.active_fmv().is_some() {
                session.host.world.finish_cutscene();
                let _ = session.apply_pending_fmv_handoff();
                true
            } else {
                false
            }
        }));
        match fmv {
            Ok(true) => stats.fmvs += 1,
            Ok(false) => {}
            Err(_) => {
                let (loc, msg, frames) = LAST_PANIC
                    .with(|p| p.borrow_mut().take())
                    .unwrap_or_default();
                findings.push(mk(
                    &session,
                    "panic",
                    frame,
                    loc,
                    format!("[fmv hand-off] {msg} | {frames}"),
                ));
                break;
            }
        }

        let mode = format!("{:?}", session.host.world.mode);
        *stats.mode_frames.entry(mode.clone()).or_default() += 1;
        if session.host.world.mode == SceneMode::Battle {
            let b = &session.host.world.battle;
            if let Some(c) = b.command.as_ref() {
                stats
                    .battle_surfaces
                    .insert(variant(&format!("{:?}", c.phase)));
            }
            for (open, name) in [
                (b.item_menu.is_some(), "item-menu"),
                (b.spell_menu.is_some(), "spell-menu"),
                (b.arts_menu.is_some(), "arts-menu"),
                (b.arts_input.is_some(), "arts-input"),
            ] {
                if open {
                    stats.battle_surfaces.insert(name.into());
                }
            }
        }
        stats.modes.insert(mode);
        if session.host.world.game_over || session.host.world.game_over_hold {
            stats.wiped = true;
            // A wipe with a save on the card plays on the way a player does:
            // the hosts' return-to-title hold, then Continue into that save.
            // With no save there is nothing to continue from and the run ends.
            let Some(key) = latest_save else { break };
            let r = catch_unwind(AssertUnwindSafe(|| {
                continue_after_wipe(&mut session, &card, key, &opts)
            }));
            match r {
                Ok(Ok(())) => {
                    stats.wipe_continues += 1;
                    // What the wipe left that the resume did not reset is
                    // checked on the next frame by the residue detector
                    // (`prev_mode` is still Battle).
                    continue;
                }
                Ok(Err((loc, detail))) => {
                    findings.push(mk(&session, "wipe_continue", frame, loc, detail));
                    break;
                }
                Err(_) => {
                    let (loc, msg, frames) = LAST_PANIC
                        .with(|p| p.borrow_mut().take())
                        .unwrap_or_default();
                    findings.push(mk(
                        &session,
                        "panic",
                        frame,
                        loc,
                        format!("[wipe continue] {msg} | {frames}"),
                    ));
                    break;
                }
            }
        }
        let w = &session.host.world;
        // Effect residue: a battle-scoped effect (the `efect.dat` pool, a
        // move-FX / effect-script / summon scene-graph, the streak block)
        // still live on the first frame past a battle exit or a scene load.
        // Retail drops every one of them with the actor-pool reset of its
        // per-stage init (`FUN_8001E1B4`); both play hosts draw them with no
        // mode test, so a survivor is on screen in the field.
        let left_battle = prev_mode == SceneMode::Battle
            && matches!(w.mode, SceneMode::Field | SceneMode::WorldMap);
        let scene_changed = w.active_scene_label != prev_scene;
        if (left_battle || scene_changed) && w.mode != SceneMode::Battle {
            let residue = w.battle_effect_residue();
            if !residue.is_empty() {
                findings.push(mk(
                    &session,
                    "effect_residue",
                    frame,
                    if left_battle {
                        "after:battle-exit".into()
                    } else {
                        "after:scene-change".into()
                    },
                    format!("battle effects still live: {}", residue.join(",")),
                ));
            }
        }
        // Timeline cap: the engine's anti-hang frame cap ended a script
        // context (`CutsceneState::capped`). Retail has no such cap, so each
        // entry is a record the port cut short - a wait that never cleared,
        // or a retail-length wait the cap stands below.
        if w.cutscene.capped.len() > capped_seen {
            for cap in &w.cutscene.capped[capped_seen..] {
                findings.push(mk(
                    &session,
                    "timeline_cap",
                    frame,
                    format!(
                        "{}@{:#x}:{:02X}",
                        if cap.modal { "timeline" } else { "helper" },
                        cap.pc,
                        cap.op
                    ),
                    format!("anti-hang cap ended a {}-byte record: {cap:?}", cap.len),
                ));
            }
        }
        capped_seen = w.cutscene.capped.len();
        let scene_changed_this_frame = scene_changed;
        if scene_changed {
            door_prev = std::mem::replace(&mut prev_scene, w.active_scene_label.clone());
        }
        // Mode edges.
        if w.mode != prev_mode {
            if w.mode == SceneMode::Battle {
                stats.battles += 1;
                // Battle loop: the same parked timeline PC opening battle
                // after battle. A scripted fight fires once per beat; a
                // record that re-arms it on every return never lets the
                // beat finish.
                if let Some(tl) = w.cutscene.timeline.as_ref() {
                    let site = format!("timeline@{:#06x}", tl.pc);
                    let n = battle_sites.entry(site.clone()).or_insert(0u32);
                    *n += 1;
                    if *n == 3 {
                        findings.push(mk(
                            &session,
                            "battle_loop",
                            frame,
                            site,
                            "three battles opened from one parked timeline PC".into(),
                        ));
                    }
                }
            }
            if prev_mode == SceneMode::Battle
                && matches!(w.mode, SceneMode::Field | SceneMode::WorldMap)
            {
                stats.battles_won += 1;
            }
            if w.mode == SceneMode::Title {
                findings.push(mk(
                    &session,
                    "dropped_to_title",
                    frame,
                    format!("from:{prev_mode:?}"),
                    "world mode became Title with no game over".into(),
                ));
                break;
            }
            prev_mode = w.mode;
        }
        // Battle that never ends: one battle outlasts the window. Two things
        // move that line. A fight in which no HP / MP moved for the whole
        // window is endless however it started. And a fight still landing
        // actions in the window's last quarter, against a monster side
        // holding more than the HP pool, is long, not endless: a random pad
        // chips about 200 a round off Cort's 50000, and a round of three
        // Gilium summons runs past 6000 frames. Below the pool a long fight
        // stays a finding - a summon hitting for 1 reads as progress, and
        // only the clock catches it.
        if w.mode == SceneMode::Battle {
            let since = *battle_since.get_or_insert(frame);
            let resources: Vec<(u16, u16)> = w
                .actors
                .iter()
                .take(8)
                .map(|a| (a.battle.hp, a.battle.mp))
                .collect();
            let (last, seen) = battle_progress.get_or_insert_with(|| (frame, resources.clone()));
            if resources != *seen {
                *last = frame;
                *seen = resources;
            }
            let idle = frame - *last;
            let pool: u32 = w
                .actors
                .iter()
                .take(8)
                .filter(|a| a.battle_monster_id.is_some())
                .map(|a| u32::from(a.battle.hp))
                .sum();
            if pad != 0 {
                battle_pressed = frame;
            }
            let long_fight = idle <= t.battle_frames / 4 && pool > t.battle_hp_pool;
            let due = idle > t.battle_frames || (frame - since > t.battle_frames && !long_fight);
            // A command prompt the pad has left alone for the window's last
            // quarter waits on the player, as retail's does.
            let at = if due {
                held_by(&session)
            } else {
                String::new()
            };
            let player_idle =
                at.starts_with("battle:command:") && frame - battle_pressed > t.battle_frames / 4;
            if due && !player_idle {
                findings.push(mk(
                    &session,
                    "battle_endless",
                    frame,
                    at,
                    format!(
                        "in Battle for {} frames, {idle} since an HP / MP moved, monster HP {pool}",
                        frame - since
                    ),
                ));
                break;
            }
        } else {
            battle_since = None;
            battle_progress = None;
            battle_pressed = frame;
        }
        // Pause menu that cannot be closed: the policy starts backing out
        // after at most 420 frames, so a menu open past the close budget
        // refused every Circle in between.
        if session.field_menu.is_some() {
            let since = *menu_open_since.get_or_insert(frame);
            if frame - since > 420 + t.menu_close_frames {
                findings.push(mk(
                    &session,
                    "menu_stuck",
                    frame,
                    held_by(&session),
                    format!("pause menu open {} frames under Circle", frame - since),
                ));
                break;
            }
        } else {
            menu_open_since = None;
        }
        // Script stall. Only field frames count: a battle, the pause menu or
        // a minigame suspends the field VM by design, so the clock pauses
        // (it restarts from zero on the next field frame).
        let field_frame = matches!(
            session.host.world.mode,
            SceneMode::Field | SceneMode::WorldMap
        ) && session.field_menu.is_none()
            // A script parked on a player-owned modal (the name-entry grid, a
            // shop) is waiting for the player, not stalled.
            && !session.host.world.name_entry_active()
            && !session.host.world.shops.shop_open;
        if !field_frame {
            park = None;
        } else if frame % t.digest_every == 0 {
            let w = &session.host.world;
            let site = w
                .cutscene
                .timeline
                .as_ref()
                .map(|tl| ("timeline", tl))
                .or_else(|| w.field_vm.helper_contexts.first().map(|h| ("helper", h)))
                .map(|(k, c)| {
                    let op = c.bytecode.get(c.pc).copied().unwrap_or(0xFF);
                    // The slice count rides along: a conversation a picker
                    // loops back to its own segment re-enters the same PC
                    // every pass, and each pass is a slice. A `0xC7` walk
                    // parks its PC for as long as the walk runs, so for it
                    // the walked bodies' positions ride along too: a long
                    // walk still stepping is the op progressing, not a stall.
                    // A `4C CD` parks on the camera glide the same way, so
                    // the glide countdown rides along for every op.
                    let mut h = HashWriter(std::collections::hash_map::DefaultHasher::new());
                    let _ = write!(h, "{}|{}|", c.frames, w.camera.state.glide_frames);
                    // A walk-on record that raised the re-poll bit (op
                    // `2E 13`) is re-run from its top every tick the player
                    // stays on its tile - Rim Elm's "stand here and press"
                    // polls - and the player walks freely meanwhile, so the
                    // player's own motion rides along for it too.
                    let repoll =
                        w.flags.story_flags & legaia_engine_core::world::WALK_ON_REPOLL_FLAG != 0;
                    if (op == 0xC7 || repoll)
                        && let Some(a) = w.actors.get(player_slot(&session))
                    {
                        let m = &a.move_state;
                        let _ = write!(h, "p{},{}|", m.world_x, m.world_z);
                    }
                    // A record parked on the walk it started - `bylon`'s
                    // Zopu talk issues `C7 47 ..` and waits on `B3 47 0A`
                    // across a walk of some 2400 frames - progresses for as
                    // long as the walker steps, whatever op it waits on.
                    for slot in w.npcs.motions.keys() {
                        let _ = write!(h, "{slot}{:?}|", w.npcs.positions.get(slot));
                    }
                    (format!("{k}@{:#06x}:op{op:02x}", c.pc), h.0.finish())
                });
            match (site, park.as_ref()) {
                (Some((site, slices)), Some((cur, cur_slices, since)))
                    if *cur == site && *cur_slices == slices =>
                {
                    if frame - since >= t.softlock_frames
                        && reported.insert(format!("script_stall|{site}"))
                    {
                        findings.push(mk(
                            &session,
                            "script_stall",
                            frame,
                            site,
                            format!("script parked at one PC for {} frames", frame - since),
                        ));
                    }
                }
                (Some((site, slices)), _) => park = Some((site, slices, frame)),
                (None, _) => park = None,
            }
        }
        // Control starved: the field has refused the pad (a timeline, a
        // dialogue, an engaging helper, a parked scene change) for a whole
        // window with no free-roam frame in it. The softlock and script-stall
        // clocks both restart whenever anything moves, so a cutscene that
        // cycles through its PCs without ever handing the player back passes
        // them. A battle, the pause menu or a minigame pauses this clock.
        if !field_frame {
            starved_since = None;
            starved_seen.clear();
        } else {
            let w = &session.host.world;
            let free = round_trip_ready(&session) && !w.script_context_engages_player();
            if free || w.active_scene_label != starved_scene {
                starved_since = None;
                starved_seen.clear();
                starved_scene = w.active_scene_label.clone();
            } else {
                let holder = held_by(&session);
                let kind = holder.split('@').next().unwrap_or("").to_string();
                // A script that reaches a PC this stretch has not been at is
                // a scene still playing - the ending's credits run their
                // record start to finish for minutes - and restarts the
                // clock. What is left is a script going round: every PC it
                // steps on, it has stepped on before.
                let at = (
                    w.cutscene.timeline.as_ref().map(|tl| tl.pc),
                    w.cutscene
                        .timeline
                        .as_ref()
                        .and_then(|tl| tl.dialog.as_ref().map(|d| d.pc)),
                    w.field_vm.helper_contexts.first().map(|h| h.pc),
                    w.dialog.inline.as_ref().map(|d| d.pc),
                );
                if starved_seen.insert(at) {
                    starved_since = Some(frame);
                }
                let since = *starved_since.get_or_insert(frame);
                // A text box waits for its confirm press for as long as it
                // takes, so the window must hold the presses that would have
                // paged a conversation through: without them the field is
                // waiting for the player, not refusing them.
                // They must still be coming at the end of the window: a
                // stream that pressed early and then went quiet is the same
                // wait.
                let cross = b(PadButton::Cross);
                let presses = |from: u64| {
                    pads[from as usize..]
                        .windows(2)
                        .filter(|p| p[1] & cross != 0 && p[0] & cross == 0)
                        .count()
                };
                if frame - since >= t.starve_frames
                    && presses(since) >= STARVE_MIN_PRESSES
                    && presses(frame.saturating_sub(t.starve_frames / 4)) >= STARVE_MIN_PRESSES / 4
                    && reported.insert(format!("control_starved|{}", w.active_scene_label))
                {
                    findings.push(mk(
                        &session,
                        "control_starved",
                        frame,
                        kind,
                        format!(
                            "no free-roam frame for {} frames; held by {holder}",
                            frame - since
                        ),
                    ));
                }
            }
            // Confined: from the frame the field hands the player back (a
            // scene arrival, a cutscene's end) the player is walked for a
            // whole budget of direction-held frames and never leaves a
            // pocket less than a sub-cell across. The softlock detector only
            // sees a seat no direction moves at all; a pocket that lets the
            // player shuffle keeps its digest ticking. Measured with the
            // walk controller itself rather than predicted from the grid.
            if w.mode != SceneMode::Field || !free {
                if !free {
                    confine = None;
                }
            } else if let Some(a) = w.actors.get(player_slot(&session)) {
                let (x, z) = (a.move_state.world_x, a.move_state.world_z);
                if !was_free {
                    confine = Some((x, x, z, z, 0));
                    confine_dirs = [0; 4];
                }
                if let Some((x0, x1, z0, z1, walked)) = confine.as_mut() {
                    *x0 = (*x0).min(x);
                    *x1 = (*x1).max(x);
                    *z0 = (*z0).min(z);
                    *z1 = (*z1).max(z);
                    if pad & DIR_MASK != 0 {
                        *walked += 1;
                    }
                    for (i, bit) in [0x10u16, 0x20, 0x40, 0x80].into_iter().enumerate() {
                        if pad & bit != 0 {
                            confine_dirs[i] += 1;
                        }
                    }
                    // Every direction has to have been tried: a corridor
                    // walled on three sides confines a stream that never
                    // holds the fourth (`uru2+talk`, a seat at (36, 64)
                    // walked Up, Down and Left for a thousand frames and
                    // left at the first Right).
                    let tried_all = confine_dirs.iter().all(|&n| n >= CONFINE_DIR_MIN_FRAMES);
                    if *x1 - *x0 >= CONFINE_SPAN || *z1 - *z0 >= CONFINE_SPAN {
                        confine = None;
                    } else if *walked >= CONFINE_WALK_FRAMES && tried_all {
                        confine = None;
                        if reported.insert(format!("confined|{}", w.active_scene_label)) {
                            findings.push(mk(
                                &session,
                                "confined",
                                frame,
                                format!("tile({},{})", (x - 0x40) >> 7, (z - 0x40) >> 7),
                                format!(
                                    "handed back at ({x},{z}) and held inside {CONFINE_SPAN} \
                                     units through {CONFINE_WALK_FRAMES} walked frames"
                                ),
                            ));
                        }
                    }
                }
            }
            was_free = free;
        }
        // Door loop: scene change after scene change inside a short span -
        // an arrival seat that stands on the door back.
        // A direction held across an arrival walks a two-way stair straight
        // back (`kor4` <-> `koin4`: each door sets the player down one tile
        // from the other's pad, facing away from it), so only changes with
        // no direction pressed since the last one count.
        if pad & DIR_MASK != 0 {
            door_dir_held = true;
        }
        if scene_changed_this_frame && std::mem::take(&mut door_dir_held) {
            door_frames.clear();
        } else if scene_changed_this_frame {
            door_frames.push_back(frame);
            while door_frames
                .front()
                .is_some_and(|&f| frame - f > DOOR_LOOP_SPAN)
            {
                door_frames.pop_front();
            }
            if door_frames.len() >= DOOR_LOOP_COUNT && reported.insert("door_loop".to_string()) {
                findings.push(mk(
                    &session,
                    "door_loop",
                    frame,
                    format!("from:{door_prev}"),
                    format!(
                        "{} scene changes inside {DOOR_LOOP_SPAN} frames",
                        door_frames.len()
                    ),
                ));
            }
        }
        // Softlock.
        if frame % t.digest_every == 0 {
            let d = progress_digest(&session) ^ menu_digest(&menu_rt);
            if d != last_digest {
                last_digest = d;
                last_change = frame;
            } else if frame - last_change >= t.softlock_frames {
                let window = &pads[last_change as usize..];
                let distinct: BTreeSet<u16> = window.iter().copied().collect();
                if distinct.len() >= t.min_distinct {
                    findings.push(mk(
                        &session,
                        "softlock",
                        frame,
                        held_by(&session),
                        format!(
                            "no progress for {} frames under {} distinct pad masks",
                            frame - last_change,
                            distinct.len()
                        ),
                    ));
                    break;
                }
            }
        }
        if t.trace_every != 0 {
            // A roster stat line whenever a member's level, base or live
            // maximum moves: which frame a level-up or a stat item landed
            // on, and whether the live window followed the base.
            let stats: Vec<(u8, u16, u16, u16, u16)> = session
                .host
                .world
                .party
                .roster
                .members
                .iter()
                .map(|m| {
                    let (h, r) = (m.hp_mp_sp(), m.record_stats());
                    (m.level(), r.hp_max, h.hp_max, r.mp_max, h.mp_max)
                })
                .collect();
            if stats != traced_stats {
                eprintln!(
                    "[trace {frame:>6}] roster (level, base hp, live hp, base mp, live mp): {stats:?} in {:?} held={}",
                    session.host.world.mode,
                    held_by(&session)
                );
                traced_stats = stats;
            }
            if frame % t.trace_every == 0 {
                eprintln!("[trace {frame:>6}] {}", trace_line(&session, pad));
            }
            // Each distinct script body a timeline / helper runs, once, as
            // hex - the bytes a triage needs to read the park in context.
            // Printed to the terminal only; never written to the report.
            let w = &session.host.world;
            let bodies = w
                .cutscene
                .timeline
                .iter()
                .chain(w.field_vm.helper_contexts.iter())
                .map(|c| &c.bytecode)
                // The inline runner's record too: an NPC talk or a prop
                // record is a script body like the others.
                .chain(w.dialog.inline.iter().map(|d| &d.bytecode));
            for bc in bodies {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                h.write(bc);
                if traced_scripts.insert(h.finish()) {
                    let hex: Vec<String> = bc
                        .iter()
                        .take(env_u64("LEGAIA_SOAK_TRACE_BYTES").unwrap_or(0x400) as usize)
                        .map(|b| format!("{b:02x}"))
                        .collect();
                    eprintln!(
                        "[trace {frame:>6}] script body ({} bytes): {}",
                        bc.len(),
                        hex.join(" ")
                    );
                }
            }
        }
        // Value checks.
        if frame % 8 == 0 {
            let mut v = Vec::new();
            value_checks(&session, t, &mut v);
            for (det, loc, detail) in v {
                let sig = format!("{det}|{loc}");
                if reported.insert(sig) {
                    findings.push(mk(&session, det, frame, loc, detail));
                }
            }
        }
    }
    RunOutcome {
        spec: spec.clone(),
        findings,
        pads,
        stats,
        enter_ok,
        arming: t.arming,
    }
}
