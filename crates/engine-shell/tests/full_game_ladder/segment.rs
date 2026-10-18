//! One segment of the ladder: its tier and report, the traversal from one
//! milestone toward the next, and `run_segment` which scores it.

use super::*;

// ---------------------------------------------------------------------------
// Segments
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Tier {
    None = 0,
    Loads = 1,
    Enters = 2,
    Progresses = 3,
    Pad = 4,
}

impl Tier {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Loads => "loads",
            Self::Enters => "enters",
            Self::Progresses => "progresses",
            Self::Pad => "pad",
        }
    }
}

pub(super) struct SegmentReport {
    pub(super) key: String,
    pub(super) from: String,
    pub(super) to: String,
    pub(super) route: String,
    pub(super) tier: Tier,
    /// Why the first tier that did not clear failed.
    pub(super) stall: Option<String>,
    /// Story flags the next anchor carries that the engine had not set where
    /// the seated run stopped.
    pub(super) missing_flags: Vec<u16>,
}

/// Is `target` reached in the session's current state?
pub(super) fn reached(session: &NativeSession, target: &Milestone) -> bool {
    if scene_name(session) != target.scene || !walking(session) {
        return false;
    }
    match target.reach.as_str() {
        "control" => released(session),
        "flags" => target
            .reach_flags
            .iter()
            .all(|&f| session.host.world.system_flag_test(f)),
        _ => true,
    }
}

/// A party wipe on a pad walk leaves the game-over screen up; nothing after
/// it is play.
pub(super) fn wiped(session: &NativeSession) -> Option<String> {
    let w = &session.host.world;
    (w.game_over_hold || w.game_over).then(|| format!("party wiped: {}", battle_snapshot(session)))
}

pub(super) fn flags_of_world(session: &NativeSession) -> BTreeSet<u16> {
    let mut out = BTreeSet::new();
    for (i, b) in session.host.world.flags.system_flags.iter().enumerate() {
        for k in 0..8 {
            if b & (0x80 >> k) != 0 {
                out.insert((i * 8 + k) as u16);
            }
        }
    }
    out
}

/// Drive from the seeded state toward `target`, hop by hop, by seated
/// traversal (with the beats pass) or by the pad. `Ok(())` when reached.
pub(super) fn traverse(
    session: &mut NativeSession,
    graph: &DiscGraph,
    target: &Milestone,
    pad: bool,
    trail: &mut Vec<String>,
) -> Result<(), String> {
    let mut beaten: BTreeSet<String> = BTreeSet::new();
    // Scenes whose training a stay carried the party out of (`dolk2`'s inn
    // plays Noa's dream mid-training and brings the party back): the
    // training goes on when the party is back, without the rest of the
    // arrival pass.
    let mut retrain: BTreeSet<String> = BTreeSet::new();
    // The flags each scene's beats pass left behind (pad tier): a scene
    // the story came back to with new flags has new beats to play.
    // `retona`: the Songi fight, the P2[17] hop to `map02` and back, and
    // the summit scene P2[18] all come before the party can work the
    // summit's wall switches (P2[4] / P2[5]) toward the way down.
    let mut beaten_flags: BTreeMap<String, BTreeSet<u16>> = BTreeMap::new();
    // Crossing detours taken per (scene, goal). A detour that ends in a
    // third scene (`rikuroa` turns the party away and the walk out lands
    // by `cave01`) leaves the player on another side of `cur`, from which
    // a further crossing is the way on (`keikoku`'s west mouth); a detour
    // that came back to `cur`, or found no crossing at all, is not retried.
    let mut crossed: BTreeMap<(String, String), usize> = BTreeMap::new();
    // Steps out onto an overworld for its clock, per scene
    // ([`clock_detour`]).
    let mut clock_detours: BTreeMap<String, usize> = BTreeMap::new();
    // Both tiers play the same beats and waypoints; the pad tier plays them
    // with pad input only ([`PAD_HAND`]).
    PAD_HAND.with(|h| h.set(pad));
    RESTED.with(|r| r.borrow_mut().clear());
    REFUSED_DOORS.with(|r| r.borrow_mut().clear());
    TURNBACK.with(|t| t.borrow_mut().clear());
    RAN_ON_WALK.with(|t| t.borrow_mut().clear());
    PLAYING_BEAT.with(|b| b.set(None));
    // Each pass answers pickers from a fresh rotation: a count carried over
    // from the seated pass (or an earlier segment) made the pad hand's
    // first answer depend on what else had run in the process (`nilboa`
    // P2[0]'s "are you ready?" opened on its second option, "No").
    PICKS.with(|p| *p.borrow_mut() = (HashMap::new(), None));
    let mut via = 0usize;
    let vias: &[String] = &target.via;
    // Edges whose hop failed even after the scene's beats ran: the ladder
    // routes around them (see the `dead` arm below).
    let mut dead: BTreeSet<(String, String)> = BTreeSet::new();
    let mut steps = 0usize;
    // The continuous hand's arrival pass (upkeep and the scene's story) is
    // one turn of this loop per scene it plays, not a hop: it does not come
    // out of the hop allowance.
    let mut upkept = 0usize;
    // Scenes whose only beats pass so far was the arrival pass's filtered
    // one (outside the milestone's scene it plays only the bands that raise
    // a flag the anchors ahead carry). A hop that then fails still gets the
    // full pass a refused door earns: the record that carries the party on
    // (`taiku`'s way back to `map03`) need not raise any such flag.
    let mut proactive_only: BTreeSet<String> = BTreeSet::new();
    // The scripted rides the continuous hand's rests and training outings
    // come back on (`retock`'s inn plays the dream and returns) are not hops
    // either; the frame budget ([`pad_budget`]) still bounds them.
    let upkeep = pad && continuous::PLAYS_ON.with(std::cell::Cell::get);
    // The scene the loop last stood in, and the one before it.
    let mut last_here = scene_name(session);
    let mut came_from = String::new();
    while steps < MAX_HOPS + vias.len() * 4 + dead.len() * 4 + upkept {
        steps += 1;
        pad_budget(session)?;
        // Let whatever the last landing started finish first.
        let here = scene_name(session);
        if here != last_here {
            came_from = std::mem::replace(&mut last_here, here.clone());
        }
        let f_arrival = flags_of_world(session);
        match run_while_moving(session, SCRIPT_CEILING) {
            Run::Entered(s) => {
                if upkeep {
                    steps -= 1;
                }
                // A waypoint whose own arrival script carries the party on
                // (`concend`'s P2[0] ends in the hop to `town0d`) was
                // visited: its beat is that script. One that carries it
                // straight back where it came from was a stay, not a visit:
                // `jiji`'s inn (P2[9]) sleeps the party over in `ropeway`
                // and its arrival cutscene returns it to `jiji` before the
                // station's own beats can play. A dream that plays and
                // wakes the party where it fell asleep is a visit all the
                // same: its arrival script is the beat, and it raises the
                // flag the waking reads (`urudre3` P2[0] raises `0x3F4`,
                // on which `uru`'s entry script spawns the waking P2[34]).
                if vias.get(via).is_some_and(|w| *w == here)
                    && (s != came_from || flags_of_world(session) != f_arrival)
                {
                    via += 1;
                    trail.push(format!("[via {here}: arrival script]"));
                }
                trail.push(format!("{s}(scripted)"));
                // A milestone scene a scripted chain passes through is
                // reached: the ending's `edteien` is one link of the credits
                // chain (`edteien > edbylon > ... > edlast`), and its anchor
                // is a state taken mid-cutscene there, not a walkable stop.
                if via >= vias.len() && reached(session, target) {
                    return Ok(());
                }
                continue;
            }
            Run::Released => {}
            Run::Parked(p) => {
                if via >= vias.len() && reached(session, target) {
                    return Ok(());
                }
                return Err(format!("in {}: parked - {p}", scene_name(session)));
            }
            other => return Err(format!("in {}: {other:?}", scene_name(session))),
        }
        if via >= vias.len() && reached(session, target) {
            return Ok(());
        }
        let cur = scene_name(session);
        // A waypoint reached: play its beats once, then head for the next.
        if let Some(w) = vias.get(via)
            && *w == cur
        {
            // A continuous hand that walks into a waypoint with a thin bag
            // stocks up before the waypoint's story, as a player does:
            // `retock`'s beats lead straight into the `jagaroom` fight (13245
            // HP), and a party that arrived with no heal and 36000 gold lost
            // it six times ([`bag_under_half_stock`]).
            if upkeep
                && session.host.world.mode == SceneMode::Field
                && bag_under_half_stock(session)
            {
                upkept += 1;
                if let Some(s) =
                    pad_shop(session).map_err(|e| format!("in {cur} (via), shopping: {e}"))?
                {
                    trail.push(format!("{s}(shop talk)"));
                    continue;
                }
                pad_equip_best(session);
                if let Some(s) =
                    pad_rest(session).map_err(|e| format!("in {cur} (via), resting: {e}"))?
                {
                    trail.push(format!("{s}(rest)"));
                    continue;
                }
            }
            via += 1;
            let mut log = Vec::new();
            let left = play_beats(session, &mut log)
                .map_err(|b| format!("in {cur} (via), playing beats: {b}"))?;
            trail.push(format!("[via {}]", log.join("; ")));
            if let Some(s) = left {
                trail.push(format!("{s}(beat)"));
                continue;
            }
            // The continuous hand finishes the waypoint's story before it
            // moves on: its talks in the scene's other walk components, and
            // the stretch's errand (an item to earn and buy).
            if upkeep {
                upkept += 1;
                if let Some(s) = pad_far_talks(session, graph, &cur, trail)
                    .map_err(|b| format!("in {cur} (via), crossing to a talk: {b}"))?
                {
                    trail.push(format!("{s}(talk crossing)"));
                    continue;
                }
                if let Some(s) = pad_far_bands(session, graph, &cur, trail)
                    .map_err(|b| format!("in {cur} (via), crossing to a band: {b}"))?
                {
                    trail.push(format!("{s}(band crossing)"));
                    continue;
                }
                if scene_name(session) == cur && walking(session) {
                    let goal = vias.get(via).unwrap_or(&target.scene).clone();
                    let out = graph
                        .route_avoiding(&cur, &goal, &dead)
                        .and_then(|r| r.get(1).cloned());
                    if let Some(s) = pad_errand(session, graph, &cur, out.as_deref(), trail)? {
                        trail.push(format!("{s}(errand)"));
                    }
                }
            }
            continue;
        }
        // The continuous hand plays a scene before it leaves it: the talks,
        // bands and props whose records raise a flag the next anchor
        // carries are what the retail run did here, and they are where a
        // party's rests, joins and grants come from. The segmented passes
        // play a scene's beats only when a door refuses.
        if pad
            && continuous::PLAYS_ON.with(std::cell::Cell::get)
            && session.host.world.mode == SceneMode::Field
            && (retrain.contains(&cur) || beaten.insert(cur.clone()))
        {
            upkept += 1;
            let train_only = retrain.remove(&cur);
            // Upkeep first, as a player arriving in a town: the shop, what
            // it sold, the bed.
            // A talk or a stay that carried the party off (`dolk2`'s inn
            // plays Noa's dream and brings it back) leaves the rest of the
            // scene for the return.
            if !train_only
                && let Some(s) =
                    pad_shop(session).map_err(|e| format!("in {cur}, shopping: {e}"))?
            {
                trail.push(format!("{s}(shop talk)"));
                beaten.remove(&cur);
                continue;
            }
            pad_equip_best(session);
            if let Some(s) = pad_rest(session).map_err(|e| format!("in {cur}, resting: {e}"))? {
                trail.push(format!("{s}(rest)"));
                beaten.remove(&cur);
                continue;
            }
            // A scene whose beats include a staged fight trains first: the
            // beats walk into the boss, and the levels come after them.
            // `noaru`'s Songi band (P2[30], 48000 HP) is on the way through
            // the valley, and the run met it at level 40 where the retail
            // run had grown to 50 by Biron's castle; `conc3` is the last bed
            // before Rogue's tower, Jette's fortress and Cort (`chitei2`,
            // 50000 HP), and a party that left it at level 35 lost to Cort
            // six times where one trained there to 40 beat him. `conc3`'s
            // boss band is down by then, so a scene with a bed trains too
            // while a member is [`FAR_SHORT`] levels or more below target.
            if !train_only
                && scene_name(session) == cur
                && walking(session)
                && (!fight_band_tiles(session).is_empty() || far_under_levelled(session))
                && !under_levelled(session).is_empty()
            {
                let goal = vias.get(via).unwrap_or(&target.scene).clone();
                let out = graph
                    .route_avoiding(&cur, &goal, &dead)
                    .and_then(|r| r.get(1).cloned());
                if let Some(s) = pad_train(session, graph, out.as_deref())
                    .map_err(|e| format!("in {cur}, training: {e}"))?
                {
                    trail.push(format!("{s}(training)"));
                    if s != cur {
                        retrain.insert(cur.clone());
                    }
                    beaten.remove(&cur);
                    continue;
                }
            }
            let mut log = Vec::new();
            let aim = (via >= vias.len()).then(|| target.clone());
            BEAT_TARGET.with(|t| *t.borrow_mut() = aim);
            let ahead = continuous::PLAYED_AHEAD
                .with(|a| a.borrow().clone())
                .filter(|(scene, _)| *scene == cur && cur != target.scene)
                .map(|(_, flags)| flags);
            let near = ahead.map(|a| NEXT_ANCHOR_FLAGS.with(|n| n.replace(Some(a))));
            // In the milestone's own scene the pass is the segmented one:
            // every live band, since the reach beat is among them.
            PROACTIVE.with(|p| p.set(cur != target.scene));
            if cur != target.scene {
                proactive_only.insert(cur.clone());
            }
            let left = if train_only {
                Ok(None)
            } else {
                play_beats(session, &mut log)
            };
            PROACTIVE.with(|p| p.set(false));
            if let Some(near) = near {
                NEXT_ANCHOR_FLAGS.with(|n| *n.borrow_mut() = near);
            }
            BEAT_TARGET.with(|t| *t.borrow_mut() = None);
            let left = left.map_err(|b| format!("in {cur}, playing beats: {b}"))?;
            beaten_flags.insert(cur.clone(), flags_of_world(session));
            trail.push(format!("[{}]", log.join("; ")));
            if let Some(s) = left {
                trail.push(format!("{s}(beat)"));
                continue;
            }
            // Then the levels: fight from this scene's bed until the party
            // stands near where the retail run did by the coming milestones.
            if scene_name(session) == cur && walking(session) {
                let goal = vias.get(via).unwrap_or(&target.scene).clone();
                let route = graph.route_avoiding(&cur, &goal, &dead);
                // Outings toward an overworld whose clock a talk ahead waits
                // on ([`clock_detour`]) rather than toward the talk itself:
                // `ropeway`'s outings to `station` came back with the cart
                // still east, and the hop on had nowhere to wait for it.
                let out = route
                    .as_ref()
                    .and_then(|r| clock_detour(session, graph, r).or_else(|| r.get(1).cloned()));
                if let Some(s) = pad_train(session, graph, out.as_deref())
                    .map_err(|e| format!("in {cur}, training: {e}"))?
                {
                    trail.push(format!("{s}(training)"));
                    if s != cur {
                        retrain.insert(cur.clone());
                    }
                } else if scene_name(session) == cur && walking(session) {
                    // What the fights paid goes over the counter, and the
                    // party sets out rested.
                    if let Some(s) =
                        pad_shop(session).map_err(|e| format!("in {cur}, shopping: {e}"))?
                    {
                        trail.push(format!("{s}(shop talk)"));
                        continue;
                    }
                    pad_equip_best(session);
                    if let Some(s) =
                        pad_rest(session).map_err(|e| format!("in {cur}, resting: {e}"))?
                    {
                        trail.push(format!("{s}(rest)"));
                    }
                }
            }
            continue;
        }
        // A counter the story had closed on arrival may have opened since
        // (`dolk2`'s dealers sell again once the festival is over): the
        // continuous hand looks in again before it leaves.
        if pad
            && continuous::PLAYS_ON.with(std::cell::Cell::get)
            && session.host.world.mode == SceneMode::Field
            && counters_reopened(session)
        {
            if let Some(s) = pad_shop(session).map_err(|e| format!("in {cur}, shopping: {e}"))? {
                trail.push(format!("{s}(shop talk)"));
                continue;
            }
            pad_equip_best(session);
        }
        let goal = vias.get(via).unwrap_or(&target.scene).clone();
        // In the target scene with its beat unplayed, or stuck on a door:
        // play the scene's walkable beats once, then look again.
        let stuck_here = cur == target.scene && via >= vias.len();
        let mut next = String::new();
        let hop = if stuck_here {
            Err(String::new())
        } else {
            let Some(route) = graph.route_avoiding(&cur, &goal, &dead) else {
                return Err(format!("no scene route from {cur} to {goal}"));
            };
            next = route[1].clone();
            // Leaving an overworld for a scene whose talk on waits on this
            // overworld's clock: stand until it shows
            // ([`wait_on_overworld_clock`]).
            // Short of the overworld, with its clock not showing what a talk
            // ahead wants: step out onto it first (the party trained from
            // `ropeway`, whose outings on `map02` flew the cart back east).
            if pad && continuous::PLAYS_ON.with(std::cell::Cell::get) {
                if session.host.world.mode == SceneMode::WorldMap {
                    wait_on_overworld_clock(session, &route[1..])?;
                } else if let Some(w) = clock_detour(session, graph, &route) {
                    let n = clock_detours.entry(cur.clone()).or_insert(0);
                    *n += 1;
                    if *n <= 3 {
                        if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                            eprintln!("    [clock] {cur}: out to {w} for its clock");
                        }
                        next = w;
                    }
                }
            }
            let hop = if pad {
                pad_hop(session, graph, &next)
            } else {
                seated_hop(session, graph, &next)
            };
            hop.map_err(|e| {
                let (x, z) = player_xz(session);
                format!("hop {cur} -> {next} at tile {:?}: {e}", tile_of(x, z))
            })
        };
        // A hop that fails while a scripted countdown runs (op `4C D3`) with
        // its expiry flag still clear: the story is waiting
        // on the clock, not on a door. Wait it out where the party stands,
        // then look at the scene again.
        if hop.is_err()
            && pad
            && continuous::PLAYS_ON.with(std::cell::Cell::get)
            && wait_out_story_timer(session)?
        {
            beaten.remove(&cur);
            crossed.retain(|(s, _), _| *s != cur);
            trail.push(format!("[{cur}: timer ran out]"));
            continue;
        }
        match hop {
            Ok(entered) => {
                if pad && std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!("    [hop] {cur} -> {entered} (frame {})", session.frames);
                }
                trail.push(entered)
            }
            Err(e)
                if pad && e.contains("no walkable path") && {
                    let n = crossed.entry((cur.clone(), goal.clone())).or_insert(0);
                    *n += 1;
                    *n <= MAX_CROSS_OVERS
                } =>
            {
                match cross_over(
                    session,
                    graph,
                    &cur,
                    &goal,
                    &CrossTarget::Door(next.clone()),
                ) {
                    Ok(s) => {
                        // Only a detour that ended in a third scene earns
                        // another: one that came back to `cur` already
                        // played every crossing its lattice named.
                        if s == cur {
                            crossed.insert((cur.clone(), goal.clone()), MAX_CROSS_OVERS);
                        }
                        trail.push(format!("{s}(crossing)"));
                    }
                    Err(why) => {
                        if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                            eprintln!("    [cross] {e}; {why}");
                        }
                        // A lift the planner does not route through may be
                        // the pocket's way out (`crossings::ride_out`).
                        let rides = crossed
                            .entry((cur.clone(), String::from("<ride>")))
                            .or_insert(0);
                        *rides += 1;
                        if *rides <= MAX_CROSS_OVERS && ride_out(session) {
                            trail.push(format!("{cur}(ride)"));
                            crossed.remove(&(cur.clone(), goal.clone()));
                            continue;
                        }
                        // Try the scene's beats next, as for any failed hop.
                        beaten.remove(&cur);
                        crossed.insert((cur.clone(), goal.clone()), MAX_CROSS_OVERS);
                        continue;
                    }
                }
            }
            Err(e) => {
                if pad
                    && beaten.contains(&cur)
                    && beaten_flags
                        .get(&cur)
                        .is_some_and(|f| *f != flags_of_world(session))
                {
                    beaten.remove(&cur);
                }
                let fresh = beaten.insert(cur.clone());
                if fresh || proactive_only.remove(&cur) {
                    if !e.is_empty() && std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                        eprintln!(
                            "    [hop] {e}; playing {cur}'s beats (frame {})",
                            session.frames
                        );
                    }
                    let mut log = Vec::new();
                    // Only the milestone's own beats pass may stop short of
                    // a fight it committed (see [`fight_committed`]).
                    let aim = (via >= vias.len()).then(|| target.clone());
                    BEAT_TARGET.with(|t| *t.borrow_mut() = aim);
                    let left = play_beats(session, &mut log);
                    BEAT_TARGET.with(|t| *t.borrow_mut() = None);
                    let left =
                        left.map_err(|b| format!("in {cur}, playing beats (after {e}): {b}"))?;
                    beaten_flags.insert(cur.clone(), flags_of_world(session));
                    trail.push(format!("[{}]", log.join("; ")));
                    if let Some(s) = left {
                        trail.push(format!("{s}(beat)"));
                    }
                    continue;
                }
                // The beats left the party in a pocket a lift serves: ride
                // it out and try the hop again (`crossings::ride_out`).
                if pad && e.contains("no walkable path") {
                    let rides = crossed
                        .entry((cur.clone(), String::from("<ride>")))
                        .or_insert(0);
                    *rides += 1;
                    if *rides <= MAX_CROSS_OVERS && ride_out(session) {
                        trail.push(format!("{cur}(ride)"));
                        crossed.remove(&(cur.clone(), goal.clone()));
                        continue;
                    }
                }
                // The beats did not open the hop either: the edge is one
                // the story no longer takes (a one-shot transport - `station`
                // P2[23], the chapter-3 cart crash into `map03`, spawns only
                // while `0x36C` is clear), or one it has not opened yet (the
                // `rikuroa` P2[57] warp to `uru` waits on `0x3BC`). Route
                // around it when another way exists.
                if !stuck_here
                    && dead.insert((cur.clone(), next.clone()))
                    && graph.route_avoiding(&cur, &goal, &dead).is_some()
                {
                    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                        eprintln!("    [hop] {cur} -> {next} is dead; rerouting: {e}");
                    }
                    trail.push(format!("[{cur}->{next} dead]"));
                    continue;
                }
                // The beat that sets a missing reach flag lies in another walk
                // component of the scene: a round trip through a neighbouring
                // scene that lands on its side (`chitei2`'s junction band
                // P2[3] is reached by re-entering from `deroa` at (93, 108)),
                // then the beats again.
                if stuck_here && pad {
                    let flags: Vec<u16> = target
                        .reach_flags
                        .iter()
                        .copied()
                        .filter(|&f| !session.host.world.system_flag_test(f))
                        .collect();
                    // The bands of the flags no reachable beat sets.
                    let bands: Vec<(i16, i16)> = flags
                        .iter()
                        .map(|&f| reach_flag_bands(session, &[f]))
                        .filter(|b| !tiles_in_reach(session, b))
                        .flatten()
                        .collect();
                    let n = crossed
                        .entry((cur.clone(), String::from("<band>")))
                        .or_insert(0);
                    *n += 1;
                    if !bands.is_empty() && *n <= MAX_CROSS_OVERS {
                        let t = CrossTarget::Tiles(bands);
                        match cross_over(session, graph, &cur, "", &t) {
                            Ok(s) => {
                                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                                    eprintln!("    [cross] {cur} to the reach band: landed in {s}");
                                }
                                trail.push(format!("{s}(band crossing)"));
                                beaten.remove(&s);
                                continue;
                            }
                            Err(why) => {
                                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                                    eprintln!("    [cross] {cur} to the reach band: {why}");
                                }
                            }
                        }
                    }
                }
                if stuck_here {
                    let missing: Vec<String> = target
                        .reach_flags
                        .iter()
                        .filter(|&&f| !session.host.world.system_flag_test(f))
                        .map(|f| format!("0x{f:03X}"))
                        .collect();
                    return Err(format!(
                        "in {cur} with control, but reach flag(s) {} never set",
                        missing.join(",")
                    ));
                }
                return Err(e);
            }
        }
    }
    // The last hop's landing is read like every other: the loop tests
    // `reached` at the top of a pass, and a hop that lands on the milestone
    // as the budget runs out (`kor5`, the Sol Tower summit, entered on the
    // sixteenth) would otherwise be called lost on arrival.
    if via >= vias.len() && reached(session, target) {
        return Ok(());
    }
    Err(format!("gave up after {MAX_HOPS} hops"))
}

/// Frames the hand stands on an overworld waiting for its clock
/// ([`wait_on_overworld_clock`]).
const OVERWORLD_CLOCK_FRAMES: u32 = 6000;

/// Does partition-1 record `rec` of `man` reach `dest` - a `0x3F` naming it,
/// in the record or a partition-2 record it spawns - when its tests of
/// `flag` read `on` and every other test reads the live bank? Picker,
/// compare and box branches are followed both ways.
fn talk_reaches_with(
    w: &legaia_engine_core::world::World,
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    rec: usize,
    dest: &str,
    flag: u16,
    on: bool,
) -> bool {
    use legaia_engine_field::man_field_scripts::partition_record_span;
    use legaia_scene_formats::field_disasm::{InsnInfo, decode, scene_change_name};
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let names = |part: usize, r: usize| -> bool {
        partition_record_span(mf, man, part, r).is_some_and(|(start, pc0, len)| {
            let body = &man[start..start + len];
            legaia_scene_formats::field_disasm::LinearWalker::new(body, pc0)
                .flatten()
                .any(|i| {
                    matches!(i.info, InsnInfo::SceneChange { .. })
                        && scene_change_name(body, &i).as_deref() == Some(dest)
                })
        })
    };
    let Some((start, pc0, len)) = partition_record_span(mf, man, 1, rec) else {
        return false;
    };
    let body = &man[start..start + len];
    let mut seen: HashSet<usize> = HashSet::new();
    let mut stack = vec![pc0];
    while let Some(mut pc) = stack.pop() {
        while seen.insert(pc) {
            let Ok(insn) = decode(body, pc) else { break };
            if insn.size == 0 {
                break;
            }
            let after = pc + insn.size;
            match insn.info {
                InsnInfo::SceneChange { .. }
                    if scene_change_name(body, &insn).as_deref() == Some(dest) =>
                {
                    return true;
                }
                InsnInfo::SpawnRecord { global_index } => {
                    if let Some(r2) = usize::from(global_index).checked_sub(n0 + n1)
                        && (names(2, r2)
                            || spawned_p2(mf, man, 2, r2, n0 + n1, 2)
                                .into_iter()
                                .any(|r3| names(2, r3)))
                    {
                        return true;
                    }
                }
                InsnInfo::SystemFlag {
                    kind: FlagKind::Test,
                    idx,
                    target: Some(target),
                    ..
                } => {
                    let set = if idx == flag {
                        on
                    } else {
                        w.system_flag_test(idx)
                    };
                    if set {
                        pc = target & 0xFFFF;
                        continue;
                    }
                }
                InsnInfo::JmpRel { target, .. } => {
                    pc = target & 0xFFFF;
                    continue;
                }
                InsnInfo::Picker { count, targets, .. } => {
                    for &t in targets.iter().take(usize::from(count)) {
                        stack.push(t & 0xFFFF);
                    }
                    break;
                }
                InsnInfo::CondJmp { target, .. } => stack.push(target & 0xFFFF),
                InsnInfo::InventoryCmp {
                    kind:
                        legaia_scene_formats::field_disasm::InventoryCmpKind::Compare {
                            skip_target,
                            ..
                        },
                    ..
                } => stack.push(skip_target & 0xFFFF),
                InsnInfo::BBoxTest { skip_target, .. } => stack.push(skip_target & 0xFFFF),
                _ => {}
            }
            pc = after;
        }
    }
    false
}

/// What the overworld `map` must show before the party leaves it for `x`,
/// where a talk carries it on to `y`: each flag `map`'s own motion streams
/// write (and nothing in `x` does) that a talk leading to `y` tests, with the
/// value under which the talk leads there and the other does not. The
/// Karisto cart: `station` P1[15] sells the ride to `station3` only while
/// `0x467` is up, and only the cart's streams on `map02` (and `map03`) move
/// it, landing it at either station on the overworld's script counter once
/// `0x38F` is up (`map02` P1[0], P2[14] / P2[15]).
fn overworld_clock_wants(session: &NativeSession, map: &str, x: &str, y: &str) -> Vec<(u16, bool)> {
    let index = &session.host.index;
    let Ok(scene) = Scene::load(index, x) else {
        return Vec::new();
    };
    let Some(man) = scene.field_man_payload(index).ok().flatten() else {
        return Vec::new();
    };
    let Ok(mf) = legaia_scene_formats::man_section::parse(&man) else {
        return Vec::new();
    };
    let census = legaia_engine_scene::man_census::motion_flag_census(index, [map]);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let w = &session.host.world;
    let mut out = Vec::new();
    for &flag in census.keys() {
        let leads: Vec<(bool, bool)> = (0..n1)
            .map(|r| {
                (
                    talk_reaches_with(w, &mf, &man, r, y, flag, true),
                    talk_reaches_with(w, &mf, &man, r, y, flag, false),
                )
            })
            .collect();
        let on = leads.iter().any(|&(t, _)| t);
        let off = leads.iter().any(|&(_, f)| f);
        if on != off {
            out.push((flag, on));
        }
    }
    out
}

/// An overworld one door from the route's start whose clock a talk further
/// along the route waits on, and does not show it now
/// ([`overworld_clock_wants`]): the scene to step out to before going on.
fn clock_detour(session: &NativeSession, graph: &DiscGraph, route: &[String]) -> Option<String> {
    let cur = route.first()?;
    // Overworlds within two doors, by their first step: `station`'s own
    // edge to `map02` is the scripted flight, so the way out is `ropeway`.
    let maps: Vec<(String, String)> = graph
        .edges
        .keys()
        .filter(|s| legaia_engine_scene::scene::is_world_map_scene(s))
        .filter_map(|m| {
            let r = graph.route_avoiding(cur, m, &BTreeSet::new())?;
            (r.len() <= 3 && r.len() >= 2).then(|| (m.clone(), r[1].clone()))
        })
        .collect();
    for (map, step) in &maps {
        let map = map.as_str();
        for pair in route.windows(2) {
            if legaia_engine_scene::scene::is_world_map_scene(&pair[0]) {
                break;
            }
            let wants = overworld_clock_wants(session, map, &pair[0], &pair[1]);
            if !wants.is_empty() && std::env::var_os("LEGAIA_FGL_CLOCK_DEBUG").is_some() {
                let now: Vec<bool> = wants
                    .iter()
                    .map(|&(f, _)| session.host.world.system_flag_test(f))
                    .collect();
                eprintln!(
                    "    [clock] {cur}: {} -> {} wants {wants:03X?} now {now:?}",
                    pair[0], pair[1]
                );
            }
            if !wants.is_empty()
                && !wants
                    .iter()
                    .all(|&(f, on)| session.host.world.system_flag_test(f) == on)
            {
                return Some(step.clone());
            }
        }
    }
    None
}

/// On an overworld whose route leaves for `x` and then `y` by a talk that
/// waits on the overworld's own clock ([`overworld_clock_wants`]): stand
/// still until the clock shows what the talk wants. A run that crossed
/// `map02` while the cart stood east met the station master's "no scheduled
/// return time" at the counter, out of reach of the overworld it had to
/// wait on. `Ok(true)` when it waited.
fn wait_on_overworld_clock(session: &mut NativeSession, ahead: &[String]) -> Result<bool, String> {
    let map = scene_name(session);
    if session.host.world.mode != SceneMode::WorldMap {
        return Ok(false);
    }
    // Every hop of the route up to the next overworld: the clock runs only
    // while the party stands on this one (`ropeway` lies between `map02`
    // and the station counter).
    let mut wants: Vec<(u16, bool)> = Vec::new();
    for pair in ahead.windows(2) {
        if legaia_engine_scene::scene::is_world_map_scene(&pair[0]) {
            break;
        }
        for want in overworld_clock_wants(session, &map, &pair[0], &pair[1]) {
            if !wants.contains(&want) {
                wants.push(want);
            }
        }
    }
    let (x, y) = (
        ahead.first().cloned().unwrap_or_default(),
        ahead.last().cloned().unwrap_or_default(),
    );
    let met = |s: &NativeSession| {
        wants
            .iter()
            .all(|&(f, on)| s.host.world.system_flag_test(f) == on)
    };
    if wants.is_empty() || met(session) {
        return Ok(false);
    }
    let trace = std::env::var_os("LEGAIA_FGL_TRACE").is_some();
    if trace {
        eprintln!("    [clock] {map}: {x} -> {y} waits on {wants:03X?}");
    }
    for n in 0..OVERWORLD_CLOCK_FRAMES {
        pad_budget(session)?;
        session.host.world.set_pad(0);
        match session.tick() {
            Ok(SceneTickEvent::SceneEntered { .. }) => break,
            Ok(_) => {}
            Err(e) => return Err(format!("tick: {e:#}")),
        }
        if session.host.world.mode != SceneMode::WorldMap {
            break;
        }
        if met(session) {
            if trace {
                eprintln!("    [clock] {map}: shown after {n} frames");
            }
            break;
        }
    }
    Ok(true)
}

/// Most round trips [`pad_far_bands`] takes in one waypoint.
const MAX_FAR_BANDS: usize = 3;

/// The continuous hand's story bands in another walk component of a
/// waypoint: live walk-on records whose body raises a flag the next anchor
/// carries and the world lacks, standing where the walk cannot reach. A
/// round trip through a neighbouring scene that lands on a band's side
/// ([`cross_over`]), then the waypoint's beats again, while such bands
/// remain. `chitei2`'s Jette fight (P2[13] at (18, 89), `0x4C4`) is reached
/// only from the side `deroa` P2[3] re-enters at (93, 108), by the car to
/// (94, 22); the `0x4C8` its aftermath raises opens `map03`'s portal to
/// `concend`. `Ok(Some(scene))` when it ended outside `cur`.
fn pad_far_bands(
    session: &mut NativeSession,
    graph: &DiscGraph,
    cur: &str,
    trail: &mut Vec<String>,
) -> Result<Option<String>, String> {
    let trace = std::env::var_os("LEGAIA_FGL_TRACE").is_some();
    for _ in 0..MAX_FAR_BANDS {
        if scene_name(session) != cur || !walking(session) {
            break;
        }
        let Some(wanted) = NEXT_ANCHOR_FLAGS.with(|a| a.borrow().clone()) else {
            break;
        };
        let missing: Vec<u16> = wanted
            .into_iter()
            .filter(|&f| !session.host.world.system_flag_test(f))
            .collect();
        // A boss's band only: the record (or one it spawns) stages a fight.
        // Crossing to every far band that raises an anchor flag replayed
        // `uru`'s dream bands out of order and walled off the `urudre1`
        // door.
        let Some((mf, man, triggers)) = scene_man_and_triggers(session) else {
            break;
        };
        let boss = |t: &(i16, i16)| {
            triggers.iter().any(|tr| {
                tr.gate == 1
                    && (i16::from(tr.tile_x), i16::from(tr.tile_z)) == *t
                    && !staged_formation_rows(&mf, &man, 2, usize::from(tr.record)).is_empty()
            })
        };
        let tiles: Vec<(i16, i16)> = reach_flag_bands(session, &missing)
            .into_iter()
            .filter(|t| boss(t) && !tiles_in_reach(session, &[*t]))
            .collect();
        if tiles.is_empty() {
            break;
        }
        pad_budget(session)?;
        match cross_over(session, graph, cur, "", &CrossTarget::Tiles(tiles.clone())) {
            Ok(s) if s == cur => {
                if trace {
                    eprintln!("    [far-band] {cur}: crossed toward {tiles:?}");
                }
                trail.push(format!("{s}(band crossing)"));
                let mut log = Vec::new();
                let left = play_beats(session, &mut log)?;
                trail.push(format!("[{}]", log.join("; ")));
                if let Some(s) = left {
                    return Ok(Some(s));
                }
            }
            Ok(s) => return Ok(Some(s)),
            Err(why) => {
                if trace {
                    eprintln!("    [far-band] {cur}: no way to {tiles:?}: {why}");
                }
                break;
            }
        }
    }
    Ok(None)
}

pub(super) fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic".into()
    }
}

/// `LEGAIA_FGL_RNG_SEED=<u32>` (decimal or `0x` hex): the world rand state
/// a pad pass starts from in place of the one it would have, so a route can
/// be played under another hand. Part B applies it to each pad segment's
/// seed, part C to its New Game (or to the checkpoint it resumes).
pub(super) fn rng_seed_override() -> Option<u32> {
    let s = std::env::var("LEGAIA_FGL_RNG_SEED").ok()?;
    let s = s.trim();
    s.strip_prefix("0x")
        .map_or_else(|| s.parse().ok(), |h| u32::from_str_radix(h, 16).ok())
}

/// Run segment `from -> to`.
pub(super) fn run_segment(
    inp: &Inputs,
    graph: &DiscGraph,
    from: &Milestone,
    from_anchor: Option<&Anchor>,
    to: &Milestone,
    to_anchor: Option<&Anchor>,
    with_pad: bool,
) -> SegmentReport {
    let route = graph
        .route(&from.scene, &to.scene)
        .map_or_else(|| "(no route)".to_string(), |r| graph.show(&r));
    let mut rep = SegmentReport {
        key: to.id.clone(),
        from: from.id.clone(),
        to: to.id.clone(),
        route,
        tier: Tier::None,
        stall: None,
        missing_flags: Vec::new(),
    };

    NEXT_ANCHOR_FLAGS.with(|a| *a.borrow_mut() = to_anchor.map(|a| a.flags.clone()));
    // -- seated pass: loads, enters, progresses -----------------------------
    // A seated pass that a boss wipes is played once more from the same seed
    // with the bag's guard against that boss's element equipped, the pad
    // pass's guard retry: the player's reload after a game over. Without it
    // a seated boss the unguarded hand wins about half the time (`chitei2`
    // P2[13], Jette) decided both the `progresses` and the `pad` tier on one
    // hand of the rand stream, so any change to how long a battle action
    // runs moved the segment.
    // A wipe after the guard retry (or with no element to guard) is reloaded
    // with a fresh hand, as the pad pass does, up to `PAD_WIPE_ATTEMPTS`
    // attempts in all: Koru (`nilboa` P2[20]) is a race the party wins or
    // loses by a few dozen HP on a given stream.
    let mut seated_prepare: Option<u8> = None;
    let mut seated_attempt = 0u32;
    LEARNED_CADENCE.with(|l| l.set(None));
    let seated = loop {
        seated_attempt += 1;
        let attempt = seated_attempt;
        LOST_TO_ELEMENT.with(|l| l.set(None));
        let prepare = seated_prepare;
        let seated = catch_unwind(AssertUnwindSafe(|| {
            let mut session = open_session(&inp.extracted);
            let opts = live_opts(false);
            let mut tier = Tier::None;
            let landed = match seed(&mut session, from, from_anchor, &opts) {
                Ok(s) => s,
                Err(e) => return (tier, Some(format!("seed: {e}")), None),
            };
            if landed != from.scene || !walking(&session) {
                return (
                    tier,
                    Some(format!(
                        "seed landed {landed} in {:?}, milestone scene is {}",
                        session.host.world.mode, from.scene
                    )),
                    None,
                );
            }
            tier = Tier::Loads;
            // `LEGAIA_FGL_SEATED_RNG_SEED=<u32>`: deal the seated pass another
            // hand, as `LEGAIA_FGL_RNG_SEED` does the pad pass - a seated boss
            // fight (`chitei2` P2[13], Jette) is checked across streams with it.
            if let Some(s) = std::env::var("LEGAIA_FGL_SEATED_RNG_SEED")
                .ok()
                .and_then(|s| s.trim().parse().ok())
            {
                session.host.world.rng_state = s;
            }
            // A reload other than the guard retry plays another hand (see
            // the pad pass's reload).
            if attempt > 1 && !(attempt == 2 && prepare.is_some()) {
                let w = &mut session.host.world;
                w.rng_state = w.rng_state.wrapping_add(attempt.wrapping_mul(0x9E37_79B9));
            }
            let mut trail = vec![landed];
            match run_while_moving(&mut session, SCRIPT_CEILING) {
                Run::Released => tier = Tier::Enters,
                Run::Entered(s) => {
                    trail.push(format!("{s}(scripted)"));
                    tier = Tier::Enters;
                }
                Run::Parked(p) => {
                    // A parked entry script can still be the segment's success
                    // (a milestone reached mid-cutscene), but the tier stops here.
                    let flags = session.host.world.flags.system_flags.clone();
                    return (
                        tier,
                        Some(format!("entry script never released: {p}")),
                        Some(flags),
                    );
                }
                other => {
                    let flags = session.host.world.flags.system_flags.clone();
                    return (tier, Some(format!("entry: {other:?}")), Some(flags));
                }
            }
            if let Some(element) = prepare {
                let n = pad_equip_guard(&mut session, element);
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!("    [seated] retry: equipped {n} element-{element} guard(s)");
                }
            }
            let res = traverse(&mut session, graph, to, false, &mut trail);
            let flags = session.host.world.flags.system_flags.clone();
            match res {
                Ok(()) => {
                    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                        eprintln!("    [seated] trail {}", trail.join(">"));
                    }
                    (Tier::Progresses, None, Some(flags))
                }
                Err(e) => (
                    tier,
                    Some(match prepare {
                        Some(el) => format!(
                            "seated (element-{el} guards): {e} [trail {}]",
                            trail.join(">")
                        ),
                        None => format!("{e} [trail {}]", trail.join(">")),
                    }),
                    Some(flags),
                ),
            }
        }));
        let wiped = matches!(&seated, Ok((_, Some(e), _)) if e.contains("party wiped"));
        match LOST_TO_ELEMENT.with(std::cell::Cell::get) {
            Some(el) if wiped && seated_prepare.is_none() => seated_prepare = Some(el),
            _ if wiped && seated_attempt < PAD_WIPE_ATTEMPTS => {}
            _ => break seated,
        }
    };
    let (tier, stall, flags_after) = match seated {
        Ok(r) => r,
        Err(p) => (
            Tier::None,
            Some(format!("PANIC: {}", panic_text(&*p))),
            None,
        ),
    };
    rep.tier = tier;
    rep.stall = stall;
    if let (Some(bank), Some(next)) = (flags_after, to_anchor)
        && rep.tier < Tier::Progresses
    {
        let have = |f: u16| {
            bank.get((f >> 3) as usize)
                .is_some_and(|b| b & (0x80 >> (f & 7)) != 0)
        };
        let prev: BTreeSet<u16> = from_anchor.map(|a| a.flags.clone()).unwrap_or_default();
        rep.missing_flags = next
            .flags
            .iter()
            .copied()
            .filter(|&f| !have(f) && !prev.contains(&f))
            .collect();
    }

    // -- pad pass ---------------------------------------------------------------
    // A party wiped by a boss whose element a Goods item in the bag guards
    // is played again from the same seed with that guard equipped: the
    // player's reload after a game over, preparing for the fight that ended
    // the last attempt (see "A boss the anchor's party cannot outlast" in
    // the ladder docs). A wipe after that - a boss with the guard on, or a
    // fight with no element to guard - is reloaded again with a fresh hand of
    // the rand stream, up to `PAD_WIPE_ATTEMPTS` attempts in all.
    if with_pad && rep.tier >= Tier::Progresses {
        let mut prepare: Option<u8> = None;
        let mut attempt = 0u32;
        LEARNED_CADENCE.with(|l| l.set(None));
        loop {
            attempt += 1;
            LOST_TO_ELEMENT.with(|l| l.set(None));
            let padded = catch_unwind(AssertUnwindSafe(|| {
                let mut session = open_session(&inp.extracted);
                let opts = live_opts(true);
                seed(&mut session, from, from_anchor, &opts)?;
                PAD_DEADLINE.with(|d| d.set(session.frames + PAD_SEGMENT_FRAMES));
                PAD_BATTLE_LEFT.with(|l| l.set(PAD_BATTLE_ALLOWANCE));
                // `LEGAIA_FGL_RNG_SEED=<u32>`: deal the pad tier another hand.
                // Every random draw it meets comes off the world rand stream,
                // so a re-seeded run is how a pad route is checked for
                // depending on one stream's luck (see "A pad wipe that moves
                // with an unrelated change" in the ladder docs).
                if let Some(s) = rng_seed_override() {
                    session.host.world.rng_state = s;
                }
                // A reload other than the guard retry plays another hand:
                // the rand stream advances with the time a player spends on
                // the game-over screen and the card menu, which the reseat
                // does not replay. (The guard retry keeps the stream, so it
                // measures the guard alone.)
                if attempt > 1 && !(attempt == 2 && prepare.is_some()) {
                    let w = &mut session.host.world;
                    w.rng_state = w.rng_state.wrapping_add(attempt.wrapping_mul(0x9E37_79B9));
                }
                if let Some(element) = prepare {
                    let n = pad_equip_guard(&mut session, element);
                    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                        eprintln!("    [pad] retry: equipped {n} element-{element} guard(s)");
                    }
                }
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!(
                        "    [pad] seeded {} at {:?} (anchor seat {:?})",
                        scene_name(&session),
                        player_xz(&session),
                        from_anchor.and_then(|a| a.seat)
                    );
                }
                let start = session.frames;
                let mut trail = vec![scene_name(&session)];
                let r = traverse(&mut session, graph, to, true, &mut trail)
                    .map_err(|e| format!("{e} [trail {}]", trail.join(">")));
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    let (n, cells) = PLAN_STATS.with(std::cell::Cell::take);
                    eprintln!(
                        "    [pad] {} frames, {n} plans over {cells} cells",
                        session.frames - start
                    );
                }
                r
            }));
            let wiped = matches!(&padded, Ok(Err(e)) if e.contains("party wiped"));
            let retry = prepare.is_none() && wiped;
            match padded {
                Ok(Ok(())) => {
                    rep.tier = Tier::Pad;
                    rep.stall = None;
                }
                Ok(Err(e)) => {
                    rep.stall = Some(match prepare {
                        Some(el) => {
                            format!("pad (attempt {attempt}, element-{el} guards): {e}")
                        }
                        None if attempt > 1 => format!("pad (attempt {attempt}): {e}"),
                        None => format!("pad: {e}"),
                    })
                }
                Err(p) => rep.stall = Some(format!("pad PANIC: {}", panic_text(&*p))),
            }
            match LOST_TO_ELEMENT.with(std::cell::Cell::get) {
                Some(el) if retry => prepare = Some(el),
                _ if wiped && attempt < PAD_WIPE_ATTEMPTS => {}
                _ => break,
            }
        }
    }
    rep
}

/// Stand still until the field's scripted countdown expires, when its
/// expiry flag is still clear. An armed countdown is story the scene is
/// running: the flag it raises need not survive to the next anchor
/// (`chitei2`'s `0x4C7` does not), so the anchor is not consulted.
/// `true` once the countdown ran out (or the scene changed under it);
/// `false` when no such countdown is armed.
///
/// The countdown is retail's `4C D3` scheduler (`FUN_801D2EBC`), read from
/// the engine: `chitei2`'s collapse sets `0x4C7` on expiry, and only then
/// does the scene's entry script hand the party on (`0x4C8`).
fn wait_out_story_timer(session: &mut NativeSession) -> Result<bool, String> {
    let (armed, remaining, flag) = {
        let b = &session.host.world.battle;
        (
            b.escape_timer.armed,
            b.escape_timer.remaining,
            ((b.escape_timer_flag_word >> 16) & 0xFFF) as u16,
        )
    };
    if !armed || session.host.world.system_flag_test(flag) {
        return Ok(false);
    }
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        eprintln!(
            "    [timer] waiting out {remaining} frame(s) for 0x{flag:03X} (frame {})",
            session.frames
        );
    }
    let here = scene_name(session);
    let limit = session.frames + u64::try_from(remaining.max(0)).unwrap_or(0) + 600;
    while session.frames < limit
        && !session.host.world.system_flag_test(flag)
        && scene_name(session) == here
    {
        pad_budget(session)?;
        session.host.world.set_pad(0);
        session.tick().map_err(|e| format!("{e:#}"))?;
    }
    Ok(session.host.world.system_flag_test(flag) || scene_name(session) != here)
}
