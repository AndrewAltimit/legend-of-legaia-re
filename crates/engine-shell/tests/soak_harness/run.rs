//! One soak run: its spec, stats and tunables, the value detectors, the
//! host-side drains, and `run_one` which plays a scene under a policy with
//! every detector armed.

use super::*;

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
    pub(super) scenes_entered: BTreeSet<String>,
    pub(super) modes: BTreeSet<String>,
    pub(super) menu_opens: u32,
    pub(super) fmvs: u32,
    pub(super) save_commits: u32,
    /// Pause-menu sub-screens entered and shops opened - what the random
    /// browse actually reached.
    pub(super) menu_rows: BTreeSet<String>,
    pub(super) shop_opens: u32,
    pub(super) round_trips: u32,
}

pub(super) struct RunOutcome {
    pub(super) spec: RunSpec,
    pub(super) findings: Vec<Finding>,
    pub(super) pads: Vec<u16>,
    pub(super) stats: RunStats,
    pub(super) enter_ok: bool,
}

pub(super) struct Tunables {
    pub(super) softlock_frames: u64,
    pub(super) digest_every: u64,
    pub(super) menu_close_frames: u64,
    pub(super) battle_frames: u64,
    pub(super) queue_cap: usize,
    /// `LEGAIA_SOAK_TRACE=N`: print a state line every N frames (replay
    /// triage; 0 = off).
    pub(super) trace_every: u64,
    /// Distinct pad masks the softlock window must contain. `1` turns the
    /// detector into a neutral-pad control (`LEGAIA_SOAK_MIN_DISTINCT=1`).
    pub(super) min_distinct: usize,
}

impl Tunables {
    pub(super) fn from_env() -> Self {
        Self {
            softlock_frames: env_u64("LEGAIA_SOAK_SOFTLOCK_FRAMES").unwrap_or(1800),
            digest_every: 15,
            menu_close_frames: 900,
            battle_frames: env_u64("LEGAIA_SOAK_BATTLE_FRAMES").unwrap_or(18_000),
            queue_cap: 4096,
            trace_every: env_u64("LEGAIA_SOAK_TRACE").unwrap_or(0),
            min_distinct: env_u64("LEGAIA_SOAK_MIN_DISTINCT").unwrap_or(3) as usize,
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

pub(super) type StatusBoard = Arc<Mutex<Vec<WorkerStatus>>>;

pub(super) fn value_checks(
    s: &BootSession,
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
                format!("{}/{}", hms.hp_cur, hms.hp_max),
            ));
        }
        if hms.mp_max > 0 && hms.mp_cur > hms.mp_max {
            out.push((
                "value",
                format!("roster.mp>max(slot{i})"),
                format!("{}/{}", hms.mp_cur, hms.mp_max),
            ));
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
/// `BootSession::tick` routes only BGM events off the field queue and drains
/// none of the rest, so a headless driver that skipped this would measure its
/// own leak rather than the engine.
pub(super) fn host_drains(session: &mut BootSession) {
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
    let mut session = open_session(src);
    session.begin_new_game();
    // `<label>@<save>`: the run plays the scene with a library card save's
    // party, bag and story flags instead of the New Game party's.
    let (label, save) = split_save(&spec.scene);
    if let Some(save) = save {
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
        live_loop: !env_flag("LEGAIA_SOAK_NO_ENCOUNTERS"),
        player_battle: true,
        battle_bgm: None,
    };
    let mut findings: Vec<Finding> = Vec::new();
    let mut stats = RunStats::default();
    let mut pads = Vec::with_capacity(spec.frames as usize);

    let mk = |session: &BootSession,
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
    let (label, shop_visits) = split_shop(label);
    let (label, round_trip) = split_round_trip(label);
    let (scene, minigame) = split_minigame(label);
    let entered = catch_unwind(AssertUnwindSafe(|| {
        let r = session.enter_scene_live(scene, &opts);
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
        };
    }

    // A round-trip run starts with one save on the card, so the pause
    // menu's Load row (open in every scene; Save is per-scene) has a file
    // to resume - an empty card refuses before any commit.
    if round_trip {
        let sf = session.host.world.save_full();
        let resume = session.current_resume();
        card.insert((0, 0), (sf, resume));
        refresh_rack(&mut session, &card);
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
    let mut battle_sites: BTreeMap<String, u32> = BTreeMap::new();
    let mut prev_pad = 0u16;

    for frame in 0..spec.frames {
        let pad = match pads_override {
            Some(p) => p.get(frame as usize).copied().unwrap_or(0),
            None => policy.pad(&session, frame),
        };
        pads.push(pad);
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
        // redraw arm, the browser's `name_entry_input`). `BootSession::tick`
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
        // drain); `BootSession::tick` routes only the BGM events and restores
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
        // pick (`apply_save_commit`); `BootSession` only latches the pick.
        // The harness keeps the card in memory, so a later Load in the same
        // run reads back what an earlier Save wrote.
        if let Some(commit) = session.last_save_commit.take() {
            stats.save_commits += 1;
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

        let w = &session.host.world;
        stats.modes.insert(format!("{:?}", w.mode));
        if w.game_over || w.game_over_hold {
            stats.wiped = true;
            break;
        }
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
        if scene_changed {
            prev_scene = w.active_scene_label.clone();
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
        // Battle that never ends.
        if w.mode == SceneMode::Battle {
            let since = *battle_since.get_or_insert(frame);
            if frame - since > t.battle_frames {
                findings.push(mk(
                    &session,
                    "battle_endless",
                    frame,
                    held_by(&session),
                    format!("in Battle for {} frames", frame - since),
                ));
                break;
            }
        } else {
            battle_since = None;
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
                    if op == 0xC7 {
                        if let Some(a) = w.actors.get(player_slot(&session)) {
                            let m = &a.move_state;
                            let _ = write!(h, "p{},{}|", m.world_x, m.world_z);
                        }
                        for slot in w.npcs.motions.keys() {
                            let _ = write!(h, "{slot}{:?}|", w.npcs.positions.get(slot));
                        }
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
                .map(|c| &c.bytecode);
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
    }
}
