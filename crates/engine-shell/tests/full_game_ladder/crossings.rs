//! Crossing a scene that is not connected on foot: the targets a crossing
//! may aim for, the beats that open the way, and the plan over them.

use super::*;

/// What a [`cross_over`] detour is for: a door toward another scene, or the
/// band tiles of a walk-on beat in another walk component of the scene.
#[derive(Debug, Clone)]
pub(super) enum CrossTarget {
    /// A door of the loaded scene toward this scene.
    Door(String),
    /// Any of these tiles (a walk-on band).
    Tiles(Vec<(i16, i16)>),
}

impl CrossTarget {
    /// The tiles the lattice plans toward.
    pub(super) fn tiles(&self, session: &NativeSession, graph: &DiscGraph) -> Vec<(i16, i16)> {
        match self {
            CrossTarget::Door(toward) => doors_to(session, graph, toward)
                .map(|d| {
                    d.iter()
                        .map(|d| (i16::from(d.tile.0), i16::from(d.tile.1)))
                        .collect()
                })
                .unwrap_or_default(),
            CrossTarget::Tiles(t) => t.clone(),
        }
    }

    /// Whether the pad walk can now reach the target from where it stands.
    pub(super) fn in_reach(&self, session: &NativeSession, graph: &DiscGraph) -> bool {
        match self {
            CrossTarget::Door(toward) => door_in_reach(session, graph, toward),
            CrossTarget::Tiles(t) => tiles_in_reach(session, t),
        }
    }
}

/// Can the pad walk reach one of `tiles` (or a tile beside it) from where
/// the player stands?
pub(super) fn tiles_in_reach(session: &NativeSession, tiles: &[(i16, i16)]) -> bool {
    let avoid = hazards(session, "");
    let (x, z) = player_xz(session);
    let from = cell_of(x, z);
    tiles.iter().any(|&g| {
        plan_path(session, from, g, &avoid)
            .and_then(|p| p.last().copied())
            .is_some_and(|c| {
                let t = tile_of(cell_center(c).0, cell_center(c).1);
                (t.0 - g.0).abs() + (t.1 - g.1).abs() <= 1
            })
    })
}

/// Whether the milestone beat in partition-2 record `rec` should wait: its
/// record sets one of the milestone's missing reach flags and clears a flag
/// the live state holds, and another
/// missing reach flag's band lies outside the player's walk component. The
/// pad hand plays the far beat first, by a crossing, the way the story does:
/// `chitei2` P2[11] (the `0x470` climax) clears the Rapid Transport switch
/// flags `0x4EF` / `0x4F0` that route `deroa`'s platform hub to the room
/// whose door re-enters by the junction band P2[3] (`0x3C8`).
pub(super) fn reach_beat_waits(
    session: &NativeSession,
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    rec: usize,
) -> bool {
    use legaia_engine_field::man_field_scripts::partition_record_span;
    use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker};
    let Some(target) = BEAT_TARGET.with(|t| t.borrow().clone()) else {
        return false;
    };
    let missing: Vec<u16> = target
        .reach_flags
        .iter()
        .copied()
        .filter(|&f| !session.host.world.system_flag_test(f))
        .collect();
    if missing.len() < 2 {
        return false;
    }
    let Some((start, pc0, len)) = partition_record_span(mf, man, 2, rec) else {
        return false;
    };
    let sets: Vec<u16> = LinearWalker::new(&man[start..start + len], pc0)
        .map_while(Result::ok)
        .filter_map(|i| match i.info {
            InsnInfo::SystemFlag {
                kind: FlagKind::Set,
                idx,
                ..
            } if missing.contains(&idx) => Some(idx),
            _ => None,
        })
        .collect();
    if sets.is_empty() {
        return false;
    }
    // Only a beat that takes something away waits: one that clears a flag
    // the live state holds (P2[11] clears the switch flags). A beat that only
    // adds state is played where it is met - P2[3] waiting on P2[11] in turn
    // would leave both unplayed.
    let w = &session.host.world;
    let takes = LinearWalker::new(&man[start..start + len], pc0)
        .map_while(Result::ok)
        .any(|i| {
            matches!(i.info, InsnInfo::SystemFlag { kind: FlagKind::Clear, idx, .. } if w.system_flag_test(idx))
        });
    if !takes {
        return false;
    }
    missing.iter().filter(|f| !sets.contains(f)).any(|&f| {
        let bands = reach_flag_bands(session, &[f]);
        !bands.is_empty() && !tiles_in_reach(session, &bands)
    })
}

/// The walk-on bands of the loaded scene that are **lifts**: a gate-passing
/// partition-2 record that asks first (a two-way picker), arcs the player
/// (op `0x43` on the `0xF8` channel), and is laid on bands more than sixteen
/// tiles apart - one car serving several landings. `jouinb`'s P2[6] /
/// P2[7] / P2[8] are this class: each asks "ride?" and carries the party to
/// the landing its car flags (`0x450` / `0x461`) pick. The planner does not
/// route through them - where a car stands is story state the walk cannot
/// read ahead - so a party a ride set down in a pocket whose only exit is
/// the same lift's band is left there by every plan.
pub(super) fn ride_bands(session: &NativeSession) -> Vec<(i16, i16)> {
    use legaia_engine_field::man_field_scripts::{partition_record_span, partition2_record_gates};
    use legaia_scene_formats::field_disasm::{ActorCtrlKind, InsnInfo, LinearWalker};
    let Some((mf, man, triggers)) = scene_man_and_triggers(session) else {
        return Vec::new();
    };
    let w = &session.host.world;
    let mut by_record: BTreeMap<usize, Vec<(i16, i16)>> = BTreeMap::new();
    for t in triggers.iter().filter(|t| t.gate == 1) {
        by_record
            .entry(usize::from(t.record))
            .or_default()
            .push((i16::from(t.tile_x), i16::from(t.tile_z)));
    }
    let mut out = Vec::new();
    for (r, tiles) in by_record {
        let pass = partition2_record_gates(&mf, &man, r)
            .is_some_and(|(c1, c2)| w.p2_record_gates_pass(&c1, &c2));
        let Some((start, pc0, len)) = partition_record_span(&mf, &man, 2, r) else {
            continue;
        };
        if !pass {
            continue;
        }
        let spread = tiles.iter().any(|a| {
            tiles
                .iter()
                .any(|b| i32::from((a.0 - b.0).abs()) + i32::from((a.1 - b.1).abs()) > 16)
        });
        let body = &man[start..start + len];
        let insns: Vec<_> = LinearWalker::new(body, pc0).map_while(Result::ok).collect();
        let asks = insns
            .iter()
            .any(|i| matches!(i.info, InsnInfo::Picker { count: 2, .. }));
        let arcs = insns.iter().any(|i| {
            i.extended == Some(0xF8)
                && matches!(
                    i.info,
                    InsnInfo::ActorCtrl {
                        kind: ActorCtrlKind::ArcJump { .. },
                        ..
                    }
                )
        });
        if spread && asks && arcs {
            out.extend(tiles);
        }
    }
    out
}

/// Ride out of the walk component the party stands in: step onto the
/// nearest reachable [`ride_bands`] tile and let the lift carry the party.
/// `true` when a ride set the party down more than eight tiles off.
pub(super) fn ride_out(session: &mut NativeSession) -> bool {
    let (x, z) = player_xz(session);
    let here = dispatch_tile(x, z);
    let all = ride_bands(session);
    let mut bands: Vec<(i16, i16)> = all
        .iter()
        .copied()
        .filter(|&b| tiles_in_reach(session, &[b]))
        .collect();
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        eprintln!(
            "    [ride] {} from {here:?}: ride bands {all:?}, in reach {bands:?}",
            scene_name(session)
        );
    }
    bands.sort_by_key(|&(bx, bz)| (i32::from(bx) - here.0).abs() + (i32::from(bz) - here.1).abs());
    for (bx, bz) in bands {
        // Up to the band's side, then one press onto it and no further: a
        // walk that kept its goal after the car set the party down walked
        // straight onto the far landing's band and rode back.
        let avoid = pad_avoid(session, None);
        let Some(side) = beside(session, (bx, bz))
            .into_iter()
            .find(|&s| matches!(pad_walk(session, s, &avoid, 0), Ok(Walk::Arrived)))
        else {
            continue;
        };
        for _ in 0..60 {
            if on_tile(session, (bx, bz)) {
                break;
            }
            let p = pad_for_step(session, bx - side.0, bz - side.1);
            session.host.world.set_pad(p);
            let _ = session.tick();
        }
        session.host.world.set_pad(0);
        RIDING.with(|r| r.set(true));
        let _ = run_while_moving(session, DEEP_EXIT_TICKS);
        RIDING.with(|r| r.set(false));
        let (x, z) = player_xz(session);
        let now = dispatch_tile(x, z);
        let moved = (now.0 - here.0).abs() + (now.1 - here.1).abs() > 8;
        if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
            eprintln!(
                "    [ride] {} band ({bx}, {bz}) from {here:?}: now {now:?}",
                scene_name(session)
            );
        }
        if moved {
            return true;
        }
    }
    false
}

/// The band tiles of the live walk-on records of the loaded scene whose own
/// body sets one of `flags` - the beats a milestone's missing reach flags
/// wait on (`chitei2` P2[3], the junction's north-south band, sets `0x3C8`).
pub(super) fn reach_flag_bands(session: &NativeSession, flags: &[u16]) -> Vec<(i16, i16)> {
    use legaia_engine_field::man_field_scripts::{partition_record_span, partition2_record_gates};
    use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker};
    let Some((mf, man, triggers)) = scene_man_and_triggers(session) else {
        return Vec::new();
    };
    let w = &session.host.world;
    let mut out = Vec::new();
    for t in triggers.iter().filter(|t| t.gate == 1) {
        let r = usize::from(t.record);
        let pass = partition2_record_gates(&mf, &man, r)
            .is_some_and(|(c1, c2)| w.p2_record_gates_pass(&c1, &c2));
        if !pass {
            continue;
        }
        let Some((start, pc0, len)) = partition_record_span(&mf, &man, 2, r) else {
            continue;
        };
        let body = &man[start..start + len];
        let sets = LinearWalker::new(body, pc0).map_while(Result::ok).any(|i| {
            matches!(i.info, InsnInfo::SystemFlag { kind: FlagKind::Set, idx, .. } if flags.contains(&idx))
        });
        if sets {
            out.push((i16::from(t.tile_x), i16::from(t.tile_z)));
        }
    }
    out
}

/// A detour through a **crossing scene**: `cur`'s door toward `goal` lies in
/// another walk component of `cur` (`map01`'s north and south halves meet
/// only through `suimon`). A player crosses by entering a scene `X` that
/// `cur` has a door to and that has a door back, and leaving `X` by its
/// other door. Each candidate `X` is tried by pad hop in turn - an
/// unreachable door refuses before any walking - and `Ok(scene)` is where
/// the round trip landed.
pub(super) fn cross_over(
    session: &mut NativeSession,
    graph: &DiscGraph,
    cur: &str,
    goal: &str,
    target: &CrossTarget,
) -> Result<String, String> {
    // The doors a band crossing refused stay refused only for the crossing:
    // the segment's other hops still take them.
    let mut refused_here: Vec<(String, (i16, i16))> = Vec::new();
    let r = cross_over_inner(session, graph, cur, goal, target, &mut refused_here);
    REFUSED_DOORS.with(|r| {
        let mut r = r.borrow_mut();
        for k in refused_here {
            r.remove(&k);
        }
    });
    r
}

/// [`cross_over`]'s body; `refused_here` collects the doors it refused.
pub(super) fn cross_over_inner(
    session: &mut NativeSession,
    graph: &DiscGraph,
    cur: &str,
    goal: &str,
    target: &CrossTarget,
    refused_here: &mut Vec<(String, (i16, i16))>,
) -> Result<String, String> {
    let back = |x: &String| graph.edges.get(x).is_some_and(|e| e.contains(cur));
    let mut cands: Vec<String> = graph
        .edges
        .get(cur)
        .into_iter()
        .flatten()
        .filter(|x| x.as_str() != goal && x.as_str() != cur && back(x))
        .cloned()
        .collect();
    // The crossing the lattice names goes first ([`crossing_plan`]); the
    // rest keep their order behind it, so a chain the lattice cannot see is
    // still found by trial.
    let all = cands.clone();
    let plan = |session: &NativeSession, shut: &BTreeSet<String>| {
        let open: Vec<String> = all.iter().filter(|x| !shut.contains(*x)).cloned().collect();
        let p = crossing_plan(session, graph, &open, &target.tiles(session, graph));
        if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
            let (px, pz) = player_xz(session);
            eprintln!(
                "    [cross] {cur} toward {target:?} from {:?}: lattice plan {p:?} (shut {shut:?})",
                dispatch_tile(px, pz)
            );
        }
        p
    };
    // Crossings that brought the player back to the side it left, even with
    // their beats played: which side a crossing delivers to can be story
    // state (`suimon`'s water gate, `0x27B`, switches the `map01` entry to
    // the southern chamber), and the lattice cannot see it.
    let mut shut: BTreeSet<String> = BTreeSet::new();
    // Where the planned round trip should land, by crossing: leaving it,
    // the pad hand takes the door whose own record lands there.
    let mut want: HashMap<String, (i16, i16)> = HashMap::new();
    if let Some((x, landing)) = plan(session, &shut)
        && let Some(i) = cands.iter().position(|c| *c == x)
    {
        if let Some(l) = landing {
            want.insert(x.clone(), l);
        }
        let x = cands.remove(i);
        cands.insert(0, x);
    }
    let mut tried = Vec::new();
    let mut crossings = 0usize;
    let mut visited: BTreeSet<String> = BTreeSet::new();
    // A crossing that brings the player back to the side it left is taken
    // once more with its own beats played first.
    let mut queue: VecDeque<(String, bool)> = cands.into_iter().map(|x| (x, false)).collect();
    while let Some((x, again)) = queue.pop_front() {
        if crossings >= MAX_CROSSINGS {
            tried.push(format!("{MAX_CROSSINGS} crossings taken"));
            break;
        }
        let (fx, fz) = player_xz(session);
        let left_from = tile_of(fx, fz);
        LAST_HOP_DOOR.with(|l| *l.borrow_mut() = None);
        let r = pad_hop(session, graph, &x);
        let out_door = LAST_HOP_DOOR.with(|l| l.borrow().clone());
        if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
            let (px, pz) = player_xz(session);
            eprintln!(
                "    [cross] {cur} via {x}: {:?} at {:?}",
                r.as_ref()
                    .map_err(|e| e.chars().take(120).collect::<String>()),
                dispatch_tile(px, pz)
            );
        }
        match r {
            Ok(entered) if entered == x => {
                crossings += 1;
                visited.insert(x.clone());
                let r = match run_while_moving(session, SCRIPT_CEILING) {
                    // An arrival script that carries the party straight back
                    // out (`rikuroa` turns a party away before its story
                    // opens) is a crossing like any other: where it lands is
                    // the answer.
                    Run::Entered(s) => {
                        if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                            eprintln!("    [cross] {x}'s arrival carried the party to {s}");
                        }
                        // Let the landing scene's own arrival run before the
                        // position is read: a hand-off may land on the
                        // parked sentinel (127, 127) and leave the seat to
                        // the destination's script (`ropeway` P2[31] lands
                        // in `jiji`, whose entry spawns P2[10] and its
                        // `CC F8 51` puts the party at (67, 74)).
                        match run_while_moving(session, SCRIPT_CEILING) {
                            Run::Entered(s2) => Ok(s2),
                            _ => Ok(s),
                        }
                    }
                    Run::Released => {
                        // On the second visit, play the crossing's own beats
                        // first: a sluice gate or a lever is a story beat like
                        // any other.
                        let mut left = None;
                        if again {
                            let mut log = Vec::new();
                            left = play_beats(session, &mut log)
                                .map_err(|b| format!("in crossing {x}, playing beats: {b}"))?;
                        }
                        match left {
                            // A beat that changed scene may land on a
                            // cutscene that carries the party on (`suimon`'s
                            // water gate drains on `map01` at `(0, 0)` and
                            // returns to the drained chamber): let it run.
                            Some(s) => match run_while_moving(session, SCRIPT_CEILING) {
                                Run::Entered(s2) => Ok(s2),
                                _ => Ok(s),
                            },
                            None => {
                                FAR_DOOR.with(|f| f.set(true));
                                let wanted = want.get(&x).copied();
                                WANT_LANDING.with(|w| *w.borrow_mut() = wanted);
                                let mut r = pad_hop(session, graph, cur);
                                WANT_LANDING.with(|w| *w.borrow_mut() = None);
                                // The planned side's door is out of reach
                                // from where the crossing let the player in
                                // (its halves join on story state), or the
                                // walk there stalled: any door.
                                if wanted.is_some() && scene_name(session) == x && r.is_err() {
                                    r = pad_hop(session, graph, cur);
                                }
                                FAR_DOOR.with(|f| f.set(false));
                                // A door whose record runs a script before
                                // its scene change can report a release while
                                // the change is still to come (`dolk`'s south
                                // door): let the scene settle, and take the
                                // scene it lands in as the answer.
                                match r {
                                    Err(e) => {
                                        let settled = run(session, EXIT_TICKS, false);
                                        if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                                            eprintln!(
                                                "    [cross] leaving {x}: {e}; settled {settled:?} in {}",
                                                scene_name(session)
                                            );
                                        }
                                        match settled {
                                            Run::Entered(s) => Ok(s),
                                            _ if scene_name(session) == cur => Ok(cur.to_string()),
                                            _ => Err(e),
                                        }
                                    }
                                    r => r,
                                }
                            }
                        }
                    }
                    other => return Err(format!("entering crossing {x}: {other:?}")),
                };
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    let (px, pz) = player_xz(session);
                    eprintln!(
                        "    [cross] left {x}: {r:?}, now {} at {:?}",
                        scene_name(session),
                        dispatch_tile(px, pz)
                    );
                }
                match r {
                    // Back in `cur`: done when the goal's door is now in
                    // reach, else the next crossing from here.
                    Ok(s) if s == cur => {
                        let (px, pz) = player_xz(session);
                        if target.in_reach(session, graph) {
                            if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                                eprintln!(
                                    "    [cross] {cur}: {target:?} in reach from {:?}",
                                    dispatch_tile(px, pz)
                                );
                            }
                            return Ok(s);
                        }
                        let back =
                            plan_path(session, cell_of(px, pz), left_from, &hazards(session, ""))
                                .and_then(|p| p.last().copied())
                                .is_none_or(|c| {
                                    let t = tile_of(cell_center(c).0, cell_center(c).1);
                                    i32::from((t.0 - left_from.0).abs() + (t.1 - left_from.1).abs())
                                        <= DOOR_APPROACH_SLACK
                                });
                        if back && again {
                            shut.insert(x.clone());
                        }
                        // Back on the side it left: the door it went out by
                        // leads to the part of `x` that only returns here
                        // (`chitei2`'s P2[4] lands north of `deroa`'s
                        // platform hub, P2[5] south of it, where the hub ride
                        // starts). The next round trip leaves by another.
                        let mut new_door = false;
                        if back
                            && matches!(target, CrossTarget::Tiles(_))
                            && let Some((s0, d)) = out_door.clone()
                            && s0 == cur
                        {
                            new_door = REFUSED_DOORS.with(|r| {
                                r.borrow_mut()
                                    .insert((s0.clone(), d), flags_of_world(session))
                                    .is_none()
                            });
                            if new_door {
                                refused_here.push((s0, d));
                            }
                        }
                        // The lattice names the next crossing from where the
                        // player landed. A crossing taken before is taken
                        // again with its beats played.
                        match plan(session, &shut) {
                            Some((y, landing)) => {
                                match landing {
                                    Some(l) => want.insert(y.clone(), l),
                                    None => want.remove(&y),
                                };
                                // A round trip by a door not taken before
                                // is a first visit: its beats wait.
                                let seen = visited.contains(&y) && !new_door;
                                queue.retain(|(q, _)| *q != y);
                                queue.push_front((y, seen));
                            }
                            None if !again => queue.push_front((x.clone(), true)),
                            None => {}
                        }
                        tried.push(format!(
                            "{x}: landed {} short of the door",
                            if back {
                                "back on the same side"
                            } else {
                                "on a new side"
                            }
                        ));
                    }
                    Ok(s) => return Ok(s),
                    Err(e) => return Err(format!("crossing {x} back to {cur}: {e}")),
                }
            }
            Ok(other) => return Ok(other),
            Err(e) => tried.push(format!("{x}: {}", e.chars().take(80).collect::<String>())),
        }
    }
    Err(format!(
        "no crossing scene reachable ({})",
        tried.join("; ")
    ))
}

/// The lattice cells of dispatch tile `t` (four 32-unit cells a side).
pub(super) fn tile_cells(t: (i16, i16)) -> impl Iterator<Item = Cell> {
    (0..4).flat_map(move |i| (0..4).map(move |j| (t.0 * 4 + i, t.1 * 4 + j)))
}

/// The four tiles beside `t`.
pub(super) fn tile_nbrs(t: (i16, i16)) -> [(i16, i16); 4] {
    [
        (t.0, t.1 + 1),
        (t.0, t.1 - 1),
        (t.0 + 1, t.1),
        (t.0 - 1, t.1),
    ]
}

/// A tile, and the landing it was seeded from (when a round trip's own
/// entry tile seeded it).
pub(super) type Seed = ((i16, i16), Option<(i16, i16)>);

/// The first crossing of a chain, and the landing of its round trip.
pub(super) type FirstHop = Option<(String, Option<(i16, i16)>)>;

/// Most [`cross_over`] detours one [`traverse`] takes per (scene, goal).
pub(super) const MAX_CROSS_OVERS: usize = 3;

/// Most round trips one [`cross_over`] takes.
pub(super) const MAX_CROSSINGS: usize = 8;

/// Most walk components one [`crossing_plan`] floods.
pub(super) const MAX_SIDES: usize = 16;

/// The crossing to take first toward `toward`'s door, read off the loaded
/// scene's lattice. Every door of every scene is a boundary (stepping on it
/// leaves), so the scene splits into **sides**: walk components. A
/// candidate `x` joins each side that touches one of its door tiles to each
/// other such side, since a round trip through `x` lands beside the door it
/// leaves by. A breadth-first search from the player's side to one touching
/// a door toward `toward` names the first crossing of the shortest chain:
/// once its water gate is drained, `map01`'s north half reaches the `vell`
/// door through `suimon` and then `bylon`, while `dolk` - the nearer
/// crossing - only opens a pocket south of the castle. `None` when the lattice shows no chain; the caller then
/// tries every candidate in turn.
pub(super) fn crossing_plan(
    session: &NativeSession,
    graph: &DiscGraph,
    cands: &[String],
    targets: &[(i16, i16)],
) -> FirstHop {
    if targets.is_empty() {
        return None;
    }
    let xdoors: Vec<(String, Vec<(i16, i16)>)> = cands
        .iter()
        .filter_map(|x| {
            let d = doors_to(session, graph, x).ok()?;
            Some((
                x.clone(),
                d.iter()
                    .map(|d| (i16::from(d.tile.0), i16::from(d.tile.1)))
                    .collect(),
            ))
        })
        .collect();
    let avoid = hazards(session, "");
    let w = &session.host.world;
    let flood = |from: Cell| -> HashSet<Cell> {
        plan_search(session, from, (-64, -64), None, &avoid)
            .map(|(_, seen, _)| seen)
            .unwrap_or_default()
    };
    let touches = |side: &HashSet<Cell>, t: (i16, i16)| {
        tile_nbrs(t)
            .into_iter()
            .any(|n| tile_cells(n).any(|c| side.contains(&c)))
    };
    let here = scene_name(session);
    let (px, pz) = player_xz(session);
    let mut sides: Vec<HashSet<Cell>> = vec![flood(cell_of(px, pz))];
    // The first crossing of the chain that reached each side, and the
    // landing of that first round trip (when the side was seeded by one).
    let mut first: Vec<FirstHop> = vec![None];
    let mut i = 0;
    while i < sides.len() {
        if i > 0 && targets.iter().any(|&t| touches(&sides[i], t)) {
            return first[i].clone();
        }
        for (x, tiles) in &xdoors {
            if !tiles.iter().any(|&t| touches(&sides[i], t)) {
                continue;
            }
            // Where the round trip lands: the entry tiles of `x`'s own
            // scene changes back to this scene, else beside its doors here.
            // A landing on a door tile (a portal the entry re-seats the
            // player on) counts by its neighbours.
            let seeds: Vec<Seed> = match graph.landings.get(&(x.clone(), here.clone())) {
                Some(l) if !l.is_empty() => l
                    .iter()
                    .map(|&(lx, lz)| (i16::from(lx), i16::from(lz)))
                    .flat_map(|t| {
                        std::iter::once(t)
                            .chain(tile_nbrs(t))
                            .map(move |n| (n, Some(t)))
                    })
                    .collect(),
                _ => tiles
                    .iter()
                    .flat_map(|&t| tile_nbrs(t))
                    .map(|n| (n, None))
                    .collect(),
            };
            for (n, landing) in seeds {
                if sides.len() >= MAX_SIDES {
                    break;
                }
                if avoid.contains(&(i32::from(n.0), i32::from(n.1)))
                    || sides.iter().any(|s| tile_cells(n).any(|c| s.contains(&c)))
                {
                    continue;
                }
                let Some(start) = tile_cells(n).find(|&c| {
                    let (cx, cz) = cell_center(c);
                    !w.field_tile_is_wall(cx, cz)
                }) else {
                    continue;
                };
                let side = flood(start);
                // A sliver of open ground inside a wall is not a side.
                if side.len() < 16 {
                    continue;
                }
                sides.push(side);
                first.push(first[i].clone().or_else(|| Some((x.clone(), landing))));
            }
        }
        i += 1;
    }
    None
}

/// Can the pad walk reach a door of the loaded scene toward `dest`?
pub(super) fn door_in_reach(session: &NativeSession, graph: &DiscGraph, dest: &str) -> bool {
    let Ok(doors) = doors_to(session, graph, dest) else {
        return false;
    };
    let avoid = hazards(session, dest);
    let (x, z) = player_xz(session);
    let from = cell_of(x, z);
    doors.iter().any(|d| {
        let g = (i16::from(d.tile.0), i16::from(d.tile.1));
        plan_path(session, from, g, &avoid)
            .and_then(|p| p.last().copied())
            .is_some_and(|c| {
                let t = tile_of(cell_center(c).0, cell_center(c).1);
                door_approachable(session, t, g)
            })
    })
}

/// Can the follower press on from `end`, where the lattice stopped, to the
/// door band `goal`? A door tile reads as a wall from inside, so the lattice
/// routinely ends a few tiles short ([`DOOR_APPROACH_SLACK`]) - but not a
/// gap with a teleport tile in it, which carries the player off before the
/// band. `kor3`'s corridor from the west doorway (18..19, 39) lands at
/// (115, 13); the door to `kor` is (115, 11), and (115, 12) between them
/// teleports back to the doorway, so the door is reached from the north,
/// round the pillar over the spent P2[14] band at (113, 5..6).
pub(super) fn door_approachable(
    session: &NativeSession,
    end: (i16, i16),
    goal: (i16, i16),
) -> bool {
    let d = i32::from((end.0 - goal.0).abs() + (end.1 - goal.1).abs());
    if d > DOOR_APPROACH_SLACK {
        return false;
    }
    if d <= 1 {
        return true;
    }
    let (lo_x, hi_x) = (i32::from(end.0.min(goal.0)), i32::from(end.0.max(goal.0)));
    let (lo_z, hi_z) = (i32::from(end.1.min(goal.1)), i32::from(end.1.max(goal.1)));
    let ends = [
        (i32::from(end.0), i32::from(end.1)),
        (i32::from(goal.0), i32::from(goal.1)),
    ];
    !teleports(session).keys().any(|&w| {
        (lo_x..=hi_x).contains(&w.0) && (lo_z..=hi_z).contains(&w.1) && !ends.contains(&w)
    })
}

/// Round trips one scene visit takes toward story talks in another walk
/// component ([`pad_far_talks`]).
const MAX_FAR_TALKS: usize = 8;

/// The tiles of the loaded scene's errand talks ([`errand_talk`], among its
/// story talks, [`talk_beats`]) - or all its story talks, in a scene the
/// stretch's `continuous_via` names - that the pad walk cannot reach from
/// where the player stands and that have not run dry ([`talk_ran_dry`]).
pub(super) fn far_talk_tiles(session: &NativeSession) -> Vec<(i16, i16)> {
    let Some((mf, man, _)) = scene_man_and_triggers(session) else {
        return Vec::new();
    };
    let w = &session.host.world;
    let scene = scene_name(session);
    // An errand's talks, and any story talk of a scene the stretch's
    // `continuous_via` names: `koin2` P1[19], Emperor Etora, gives the
    // password (`0x36D`) from the throne room at (20, 9), a room of its own
    // off another `kor` door. Every waypoint's story talks was too wide:
    // `balden`'s carried the party into `balden2` and no way back.
    let story = continuous::STORY_WAYPOINTS.with(|w| w.borrow().contains(&scene));
    talk_beats(session, &mf, &man)
        .into_iter()
        .filter(|&s| (story || errand_talk(&scene, s)) && !talk_ran_dry(session, s))
        .filter_map(|s| {
            w.npcs.positions.get(&s).map(|&(x, z)| {
                let t = dispatch_tile(x, z);
                (t.0 as i16, t.1 as i16)
            })
        })
        .filter(|t| (1..126).contains(&t.0) && (1..126).contains(&t.1))
        .filter(|t| !tiles_in_reach(session, &[*t]))
        .collect()
}

/// The continuous hand's story talks in another walk component of the
/// scene: `koin2`'s eight Sage's Treasure Chests (P1[8..15]) stand in five
/// rooms, each entered by its own `kor` door (P2[1..5]). A round trip
/// through a neighbouring scene that lands on a talk's side ([`cross_over`]),
/// then the scene's beats again, while such talks remain. `Ok(Some(scene))`
/// when it ended outside `cur`.
pub(super) fn pad_far_talks(
    session: &mut NativeSession,
    graph: &DiscGraph,
    cur: &str,
    trail: &mut Vec<String>,
) -> Result<Option<String>, String> {
    let trace = std::env::var_os("LEGAIA_FGL_TRACE").is_some();
    for _ in 0..MAX_FAR_TALKS {
        if scene_name(session) != cur || !walking(session) {
            break;
        }
        let tiles = far_talk_tiles(session);
        if tiles.is_empty() {
            break;
        }
        pad_budget(session)?;
        match cross_over(session, graph, cur, "", &CrossTarget::Tiles(tiles.clone())) {
            Ok(s) if s == cur => {
                if trace {
                    eprintln!("    [far-talk] {cur}: crossed toward {tiles:?}");
                }
                trail.push(format!("{s}(talk crossing)"));
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
                    eprintln!("    [far-talk] {cur}: no way to {tiles:?}: {why}");
                }
                break;
            }
        }
    }
    Ok(None)
}
