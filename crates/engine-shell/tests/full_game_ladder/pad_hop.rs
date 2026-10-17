//! One pad-tier hop to the next scene on the route, and the frame budget a
//! pad segment is held to.

use super::*;

/// Walk to a door toward `dest` with the pad only. `Ok(entered)` on a scene
/// change (which may not be `dest`; the caller checks).
pub(super) fn pad_hop(
    session: &mut NativeSession,
    graph: &DiscGraph,
    dest: &str,
) -> Result<String, String> {
    if let Ok(v) = std::env::var("LEGAIA_FGL_PROBE_AT")
        && let Some((scene, rest)) = v.split_once(':')
        && scene == scene_name(session)
    {
        let home = player_xz(session);
        for spec in rest.split(';') {
            if let Some((x, z)) = spec.split_once(',')
                && let (Ok(x), Ok(z)) = (x.parse::<i16>(), z.parse::<i16>())
            {
                restore_player_xz(session, (x, z));
                continue;
            }
            // A direction or a diagonal pair (`UL`, `DR`, ...).
            let mask = spec
                .chars()
                .map(|c| match c {
                    'U' => PadButton::Up.mask(),
                    'L' => PadButton::Left.mask(),
                    'D' => PadButton::Down.mask(),
                    'R' => PadButton::Right.mask(),
                    _ => 0,
                })
                .fold(0, |a, b| a | b);
            let from = player_xz(session);
            let mut trail = Vec::new();
            for f in 0..40 {
                session.host.world.set_pad(mask);
                let _ = session.tick();
                if f % 8 == 7 {
                    trail.push(player_xz(session));
                }
            }
            session.host.world.set_pad(0);
            let _ = run_while_moving(session, 600);
            eprintln!(
                "      [probe] {} {spec} from {from:?}: {trail:?} now {:?} exempt {:?} walls {:?} npcs near {:?}",
                scene_name(session),
                player_xz(session),
                session.host.world.props.arrival_exempt,
                {
                    let (x, z) = player_xz(session);
                    (0..4)
                        .map(|d| session.host.world.field_dir_blocked(x, z, d))
                        .collect::<Vec<_>>()
                },
                {
                    let (x, z) = player_xz(session);
                    session
                        .host
                        .world
                        .npcs
                        .positions
                        .iter()
                        .filter(|(_, p)| {
                            (i32::from(p.0) - i32::from(x)).abs() < 384
                                && (i32::from(p.1) - i32::from(z)).abs() < 384
                        })
                        .map(|(s, p)| (*s, *p))
                        .collect::<Vec<_>>()
                },
            );
            let (x, z) = player_xz(session);
            let w = &session.host.world;
            for c in w.props.colliders.iter().filter(|c| {
                (c.center.0 - i32::from(x)).abs() < 384 && (c.center.1 - i32::from(z)).abs() < 384
            }) {
                eprintln!(
                    "        [probe-prop] {:?} solid {} interact {} moving {} P0[{:?}]",
                    c.center,
                    c.solid,
                    c.interact,
                    c.moving_box,
                    c.anchor
                        .and_then(|a| w.props.bank.props.get(&a))
                        .map(|p| p.record)
                );
            }
        }
        restore_player_xz(session, home);
    }
    // A party too worn for the road rests first, where the scene offers it.
    if let Some(s) = pad_rest(session)? {
        return Ok(s);
    }
    // A door the run has met a boss behind (`jouine`, whose arrival commits
    // Cort): the party goes through it ready, and full - the fight starts
    // the moment the door lands, with no prompt to heal at. Met at half HP
    // and next to no MP after the walk from the switch pair, Cort won six
    // times running; met at three fifths, its party-wide cast (about 2500 a
    // member) dropped two members in its first turn.
    if continuous::FIGHT_ON_ARRIVAL.with(|f| f.borrow().contains(dest)) {
        if let Some(s) = pad_ready(session)? {
            return Ok(s);
        }
        pad_field_restore(session, 1000, true);
        pad_field_heal(session, 1000);
    }
    let doors = match doors_to(session, graph, dest) {
        Ok(d) => d,
        // A hop no walk-on band carries is taken by talking, as a player does.
        Err(why) => return talk_hop(session, graph, dest).ok_or(why)?,
    };
    // Leaving a planned crossing: the doors whose record lands on the side
    // the plan needs, when any does.
    let doors = match WANT_LANDING.with(|w| *w.borrow()) {
        Some(want) => {
            let keep: Vec<Door> = doors
                .iter()
                .filter(|d| {
                    d.entry.is_some_and(|(ex, ez)| {
                        (i16::from(ex) - want.0).abs() + (i16::from(ez) - want.1).abs() <= 1
                    })
                })
                .cloned()
                .collect();
            if keep.is_empty() { doors } else { keep }
        }
        None => doors,
    };
    HOP_DEST.with(|d| *d.borrow_mut() = Some(dest.to_string()));
    let avoid = hazards(session, dest);
    if std::env::var_os("LEGAIA_FGL_WALK_DEBUG").is_some() {
        let tiles: Vec<(u8, u8)> = doors.iter().map(|d| d.tile).collect();
        eprintln!(
            "      [hop] {} -> {dest}: door tiles {tiles:?}",
            scene_name(session)
        );
        let w = &session.host.world;
        if let Some(t) = w.world_map.region_tracker.as_ref() {
            for r in t.table().active_regions() {
                eprintln!("      [region] {r:?}");
            }
        }
        let (px, pz) = player_xz(session);
        let me = dispatch_tile(px, pz);
        let warps = teleports(session);
        for tz in 0..128i32 {
            let row: String = (0..128i32)
                .map(|tx| {
                    if (tx, tz) == me {
                        return '@';
                    }
                    if tiles.contains(&(tx as u8, tz as u8)) {
                        return 'G';
                    }
                    if warps.contains_key(&(tx, tz)) {
                        return 'T';
                    }
                    let walls = [(32, 32), (96, 32), (32, 96), (96, 96)]
                        .iter()
                        .filter(|&&(dx, dz)| {
                            w.field_tile_is_wall((tx * 128 + dx) as i16, (tz * 128 + dz) as i16)
                        })
                        .count();
                    ['.', ',', '+', '*', '#'][walls]
                })
                .collect();
            if row.chars().any(|c| c != '#') {
                eprintln!("      [map {tz:3}] {row}");
            }
        }
    }
    let (sx, sz) = player_xz(session);
    let start = cell_of(sx, sz);
    // The door the lattice gets closest to.
    // One plan per door band, not per tile: the tiles of one band share a
    // reachability, and each plan can search the whole map.
    let me0 = tile_of(sx, sz);
    let mut tiles: Vec<(i16, i16)> = doors
        .iter()
        .map(|d| (i16::from(d.tile.0), i16::from(d.tile.1)))
        .collect();
    tiles.sort_by_key(|t| (t.0 - me0.0).abs() + (t.1 - me0.1).abs());
    // A band that turned the walk back under the live flags is tried last.
    let here_name = scene_name(session);
    let flags_now = flags_of_world(session);
    let refused = |t: &(i16, i16)| {
        REFUSED_DOORS.with(|r| {
            r.borrow().iter().any(|((s, d), f)| {
                *s == here_name && *f == flags_now && (d.0 - t.0).abs() + (d.1 - t.1).abs() <= 2
            })
        })
    };
    if tiles.iter().any(|t| !refused(t)) {
        tiles.retain(|t| !refused(t));
    }
    let mut picked: Vec<(i16, i16)> = Vec::new();
    for t in tiles {
        if picked.len() < 6
            && !picked
                .iter()
                .any(|p| (p.0 - t.0).abs() + (p.1 - t.1).abs() <= 2)
        {
            picked.push(t);
        }
    }
    let misses: Vec<((i16, i16), i32)> = picked
        .into_iter()
        .map(|g| {
            let miss = plan_path(session, start, g, &avoid)
                .and_then(|p| p.last().copied())
                .map_or(i32::MAX, |c| {
                    let t = tile_of(cell_center(c).0, cell_center(c).1);
                    i32::from((t.0 - g.0).abs() + (t.1 - g.1).abs())
                });
            (g, miss)
        })
        .collect();
    let me = tile_of(sx, sz);
    let far = |g: &(i16, i16)| i32::from((g.0 - me.0).abs() + (g.1 - me.1).abs());
    // On a crossing ([`cross_over`]) the way back is the reachable door
    // farthest from the one the player came in by.
    let goal = if FAR_DOOR.with(std::cell::Cell::get) {
        misses
            .iter()
            .filter(|(_, m)| *m <= DOOR_APPROACH_SLACK)
            .max_by_key(|(g, _)| far(g))
            .or_else(|| misses.iter().min_by_key(|(_, m)| *m))
    } else {
        misses.iter().min_by_key(|(_, m)| *m)
    }
    .map(|(g, _)| *g)
    .expect("doors_to is non-empty");
    LAST_HOP_DOOR.with(|l| *l.borrow_mut() = Some((scene_name(session), goal)));
    let dist = |a: (i16, i16)| i32::from((a.0 - goal.0).abs() + (a.1 - goal.1).abs());
    // A door the collision lattice cannot get near is a different finding
    // from a walk that stalls on the way: the scene is split into walk
    // components (map01's north / south halves meet only through `suimon`),
    // and this planner does not route through a crossing scene. A short gap
    // is not that: a door tile reads as a wall from inside, so the lattice
    // routinely ends a few tiles short and the follower presses on at the
    // band (the critical-path ladder's rule).
    if let Some(end) = plan_path(session, start, goal, &avoid).and_then(|p| p.last().copied()) {
        let t = tile_of(cell_center(end).0, cell_center(end).1);
        if !door_approachable(session, t, goal) {
            return Err(format!(
                "no walkable path: the start's walk component ends {} tiles short of door {goal:?} (closest tile {t:?})",
                dist(t)
            ));
        }
    }
    // On the walk, stay off the live walk-on bands as well as the wrong
    // doors: a band's record is a story beat (or a scripted game over) the
    // route does not want.
    let mut walk_avoid = pad_avoid(session, Some(goal));
    for d in &doors {
        walk_avoid.remove(&(i32::from(d.tile.0), i32::from(d.tile.1)));
    }
    // A band whose record clears the door record's own C1 latch is the way
    // in, not a beat to step around: `ropeway`'s gate to `ropeway2`
    // (P2[30], C1 `0x514`) is reached through the station door onto
    // (29, 35), whose P2[25] clears `0x514`; the corridor band P2[24] at
    // (27, 36) sets it, and stays avoided.
    let unlatch = unlatching_bands(session, goal);
    if std::env::var_os("LEGAIA_FGL_WALK_DEBUG").is_some() {
        let mut near: Vec<_> = walk_avoid
            .iter()
            .filter(|t| (t.0 - i32::from(goal.0)).abs() + (t.1 - i32::from(goal.1)).abs() <= 6)
            .collect();
        near.sort();
        eprintln!("      [hop] goal {goal:?} unlatching {unlatch:?}; avoided near {near:?}");
        let mut av = walk_avoid.clone();
        for t in &unlatch {
            av.remove(t);
        }
        let probe_from = tile_center((goal.0, goal.1 - 3));
        let p = plan_path(session, cell_of(probe_from.0, probe_from.1), goal, &av);
        eprintln!(
            "      [hop] plan from {:?}: {:?}",
            (goal.0, goal.1 - 3),
            p.map(|p| p
                .iter()
                .map(|&c| tile_of(cell_center(c).0, cell_center(c).1))
                .collect::<Vec<_>>())
        );
    }
    for t in &unlatch {
        walk_avoid.remove(t);
    }
    // The unlatching band is crossed first, as a player crossing it on the
    // way in does: P2[25] also re-opens the wall cells of the door frame at
    // (29, 35) (`4C 70`), so the lattice sees no way to the gate until it
    // has run.
    if let Some(&(bx, bz)) = unlatch.iter().min_by_key(|t| {
        let me = player_xz(session);
        let me = tile_of(me.0, me.1);
        (t.0 - i32::from(me.0)).abs() + (t.1 - i32::from(me.1)).abs()
    }) && let (Ok(bx), Ok(bz)) = (u8::try_from(bx), u8::try_from(bz))
        && let Ok(Walk::Entered(s)) = pad_step_onto(session, (bx, bz))
    {
        return Ok(s);
    }
    // A walk that stalls against the door's own frame is a door this side
    // cannot enter (`concnow`'s door to `conc2` at (15, 41) is entered from
    // the north; from (15, 42) its threshold is a wall): like a refusal, the
    // scene's other door to `dest` is tried.
    let at_frame = |e: &str| {
        e.contains("pad walk stalled")
            && e.split(" tiles short")
                .next()
                .and_then(|h| h.rsplit(' ').next())
                .and_then(|n| n.parse::<u32>().ok())
                .is_some_and(|n| n <= 2)
    };
    let walked = match pad_walk(session, goal, &walk_avoid, 0) {
        Err(e)
            if (e.contains("changed nothing") || at_frame(&e))
                && scene_name(session) == here_name =>
        {
            let first = REFUSED_DOORS.with(|r| {
                r.borrow_mut()
                    .insert((here_name.clone(), goal), flags_of_world(session))
                    .is_none()
            });
            // Another band of the scene leads to `dest`: walk to that one.
            if first
                && doors.iter().any(|d| {
                    let t = (i16::from(d.tile.0), i16::from(d.tile.1));
                    !refused(&t) && (t.0 - goal.0).abs() + (t.1 - goal.1).abs() > 2
                })
            {
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!("    [hop] door {goal:?} to {dest} refused ({e}); trying another");
                }
                return pad_hop(session, graph, dest);
            }
            return Err(e);
        }
        other => other?,
    };
    match walked {
        Walk::Entered(s) => Ok(s),
        Walk::Arrived => {
            // On the band: keep pressing into the door while its record
            // runs up to the scene change.
            let door = tile_center(goal);
            if let Some(s) = pad_lean(session, move |_| Some(door), 30, |s| !released(s)) {
                return Ok(s);
            }
            let first = match run_while_moving(session, DEEP_EXIT_TICKS) {
                Run::Entered(s) => return Ok(s),
                other => other,
            };
            // Standing on the band and nothing left: the crossing was
            // consumed. The dispatch drops a tile change made while a script
            // holds the player (`0x801D214C`), and the step in can be the
            // very press that touches a neighbouring prop: `ropeway`'s gate
            // to `ropeway2` at (29, 36) sits beside the P0[2] door, whose
            // touch from the west runs (and fails) its box test on the
            // frame the player crosses. A player steps off and back on;
            // so does the hand, from each side in turn.
            if matches!(first, Run::Released) {
                for side in beside(session, goal) {
                    let off = pad_avoid(session, Some(goal));
                    if let Ok(Walk::Entered(s)) = pad_walk(session, side, &off, 0) {
                        return Ok(s);
                    }
                    if tile_of(player_xz(session).0, player_xz(session).1) != side {
                        continue;
                    }
                    let onto = pad_avoid(session, Some(goal));
                    match pad_walk(session, goal, &onto, 0) {
                        Ok(Walk::Entered(s)) => return Ok(s),
                        Ok(Walk::Arrived) => {
                            if let Run::Entered(s) = run_while_moving(session, DEEP_EXIT_TICKS) {
                                return Ok(s);
                            }
                        }
                        Err(_) => {}
                    }
                }
            }
            Err(format!("on door {goal:?} to {dest}: {first:?}"))
        }
    }
}

thread_local! {
    /// `(scene, formation)` of every boss fight a stager beat armed this
    /// segment. A stager's battle fires on the player's next field step, so
    /// it interrupts whatever walk comes next; that walk must fight it, not
    /// flee it as it would a random encounter.
    pub(super) static STAGED_FIGHTS: std::cell::RefCell<BTreeSet<(String, Option<u16>)>> =
        const { std::cell::RefCell::new(BTreeSet::new()) };
}

thread_local! {
    /// Set while the pad tier drives: every beat is then played with pad
    /// input only - walked to, faced and pressed - instead of seated.
    pub(super) static PAD_HAND: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(super) fn pad_hand() -> bool {
    PAD_HAND.with(std::cell::Cell::get)
}

/// Frames the pad tier may spend on one segment. Walking every beat of a
/// scene is slow, and a segment the pad hand cannot finish should read as a
/// stall rather than hold the whole run; a real play of one milestone-to-
/// milestone stretch is well inside it.
pub(super) const PAD_SEGMENT_FRAMES: u64 = 216_000;

thread_local! {
    /// `NativeSession::frames` at which the current pad segment's budget runs
    /// out.
    pub(super) static PAD_DEADLINE: std::cell::Cell<u64> = const { std::cell::Cell::new(u64::MAX) };
    /// The current stretch's walking budget: [`PAD_SEGMENT_FRAMES`], and a
    /// quarter of it again for each of a continuous stretch's waypoints.
    /// The stretch from Dr. Usha's research to Nivora has eight - `doman`
    /// twice, Uru Mais three times and its three dreams - and spent the
    /// plain budget in the third dream.
    pub(super) static STRETCH_FRAMES: std::cell::Cell<u64> = const { std::cell::Cell::new(PAD_SEGMENT_FRAMES) };
}

/// Frames of battle a pad segment may spend beyond [`PAD_SEGMENT_FRAMES`].
/// The budget is for walking; a fight is already bounded by
/// [`BATTLE_TICKS_CAP`], and the pad hand's fights are slow - `nilboa`'s
/// Three Tunnels stage a lone member against a 9500..12000 HP boss in each
/// tunnel (`F29`..`F31`), and those three fights alone run past the walking
/// budget.
pub(super) const PAD_BATTLE_ALLOWANCE: u64 = 2 * PAD_SEGMENT_FRAMES;

thread_local! {
    /// What is left of [`PAD_BATTLE_ALLOWANCE`] in the current segment.
    pub(super) static PAD_BATTLE_LEFT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// `Err` once the pad segment's frame budget is spent.
pub(super) fn pad_budget(session: &NativeSession) -> Result<(), String> {
    if pad_hand() && session.frames >= PAD_DEADLINE.with(std::cell::Cell::get) {
        Err(format!(
            "pad frame budget ({}) spent",
            STRETCH_FRAMES.with(std::cell::Cell::get)
        ))
    } else {
        Ok(())
    }
}

/// Where a [`pad_walk`] ended.
#[derive(Debug)]
pub(super) enum Walk {
    /// The player stands within `within` tiles of the goal.
    Arrived,
    /// A scene change landed on the way (the goal tile's own record, or a
    /// scripted sequence the walk set off).
    Entered(String),
}
