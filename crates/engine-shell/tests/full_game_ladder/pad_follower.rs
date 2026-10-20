//! The follower: walking a planned path with the pad, re-planning around
//! what it meets, the overworld's tile routes and the last step onto a tile.

use super::*;

/// Frames without progress after which a runner stuck on a corner walks it
/// (the follower's run-button hold).
const SLOW_WALK_AFTER: u32 = 20;
/// The d-pad bits of a pad word.
const DIRECTIONS: u16 = 0x00F0;

/// Every tile of the loaded scene a pad walk should not **enter** on its way
/// somewhere else: the walk-on door bands, the kind-0 intra-scene
/// teleports, and every gate-1 walk-on band whose partition-2 record the
/// live flags let spawn - stepping onto one runs its record (or arms a
/// warp) instead of the beat under test. A band whose record's story gates
/// shut it is inert and stays walkable: town gates and bridges are often
/// lined with them. `keep` is exempt - it is the goal.
/// Whether partition-2 record `rec` is an **inert band**: its body only
/// configures the camera or the view (`retona` P2[0], `4C 38` then a park,
/// laid across every pass off Mt. Letona's summit). Crossing one moves
/// nobody, writes no story flag and changes no scene, so the walk need not
/// step around it; avoiding it sealed the summit off from its way down.
pub(super) fn inert_band(
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    rec: usize,
) -> bool {
    use legaia_engine_field::man_field_scripts::partition_record_span;
    use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker};
    let Some((start, pc0, len)) = partition_record_span(mf, man, 2, rec) else {
        return false;
    };
    LinearWalker::new(&man[start..start + len], pc0).all(|i| {
        i.is_ok_and(|i| {
            i.extended.is_none()
                && match i.info {
                    InsnInfo::Nop
                    | InsnInfo::JmpRel { .. }
                    | InsnInfo::Camera { .. }
                    | InsnInfo::CamCfg { .. }
                    | InsnInfo::ViewWindow { .. }
                    | InsnInfo::Bgm { .. } => true,
                    InsnInfo::MenuCtrl { op0, .. } => (0x30..=0x3F).contains(&op0),
                    _ => false,
                }
        })
    })
}

pub(super) fn pad_avoid(session: &NativeSession, keep: Option<(i16, i16)>) -> HashSet<(i32, i32)> {
    use legaia_engine_field::man_field_scripts::partition2_record_gates;
    let mut out = hazards(session, "");
    if session.host.world.mode == SceneMode::Field
        && let Some((mf, man, triggers)) = scene_man_and_triggers(session)
    {
        let w = &session.host.world;
        // The continuous hand crosses a closer ([`band_is_closer`]), as a
        // player does: `korb3`'s collection bands wall a gate up behind the
        // party and stand on the only way through it.
        let continuous = continuous::PLAYS_ON.with(std::cell::Cell::get);
        for t in triggers.iter().filter(|t| t.gate == 1) {
            let live = partition2_record_gates(&mf, &man, usize::from(t.record))
                .is_none_or(|(c1, c2)| w.p2_record_gates_pass(&c1, &c2));
            if live
                && !inert_band(&mf, &man, usize::from(t.record))
                && !(continuous && band_is_closer(&mf, &man, usize::from(t.record)))
            {
                out.insert((i32::from(t.tile_x), i32::from(t.tile_z)));
            }
        }
        let index = &session.host.index;
        if let Ok(scene) = Scene::load(index, &scene_name(session))
            && let Ok((p, f)) = scene.field_intra_scene_teleports(index)
        {
            out.extend(
                p.iter()
                    .chain(f.iter())
                    .map(|t| (i32::from(t.tile_x), i32::from(t.tile_z))),
            );
        }
    }
    if let Some(k) = keep {
        out.remove(&(i32::from(k.0), i32::from(k.1)));
    }
    out
}

/// The gate-1 walk-on tiles of the loaded field scene whose partition-2
/// record cleanly clears a C1 latch of the walk-on record on `door` - the
/// bands that open the way to it.
pub(super) fn unlatching_bands(session: &NativeSession, door: (i16, i16)) -> Vec<(i32, i32)> {
    use legaia_engine_field::man_field_scripts::{
        FlagBank, partition2_record_gates, walk_partition_gflag_sites,
    };
    if session.host.world.mode != SceneMode::Field {
        return Vec::new();
    }
    let Some((mf, man, triggers)) = scene_man_and_triggers(session) else {
        return Vec::new();
    };
    let latches: Vec<u16> = triggers
        .iter()
        .filter(|t| t.gate == 1 && (i16::from(t.tile_x), i16::from(t.tile_z)) == door)
        .filter_map(|t| partition2_record_gates(&mf, &man, usize::from(t.record)))
        .flat_map(|(c1, _)| c1)
        .collect();
    if latches.is_empty() {
        return Vec::new();
    }
    let clears: HashSet<usize> = walk_partition_gflag_sites(&mf, &man, 2)
        .iter()
        .filter(|s| {
            s.bank == FlagBank::System
                && s.kind == FlagKind::Clear
                && s.clean
                && latches.contains(&s.flag)
        })
        .map(|s| s.record)
        .collect();
    triggers
        .iter()
        .filter(|t| t.gate == 1 && clears.contains(&usize::from(t.record)))
        .map(|t| (i32::from(t.tile_x), i32::from(t.tile_z)))
        .collect()
}

/// The loaded scene's minigame cabinets: the talk records (`P1`) that run
/// the minigame door-warp (`3E` with `op0 >= 100`), themselves or through a
/// record they spawn. `koin1`, Sol's casino, stands its slot machines
/// (P1[51..56], `3E 68`) along the floor as props the interact probe hits
/// but no NPC slot carries: the walk's idle tap beside one at (20, 43),
/// on the way to the Prize Counter, asked to play and put the session in
/// the slot machine.
pub(super) fn minigame_talks(session: &NativeSession) -> HashSet<u8> {
    let Some((mf, man, _)) = scene_man_and_triggers(session) else {
        return HashSet::new();
    };
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    (0..n1.min(256))
        .filter(|&r| {
            record_enters_minigame(&mf, &man, 1, r)
                || spawned_p2(&mf, &man, 1, r, n0 + n1, 3)
                    .iter()
                    .any(|&s| record_enters_minigame(&mf, &man, 2, s))
        })
        .map(|r| r as u8)
        .collect()
}

/// The continuous hand's way through a teleport maze: once a plan crosses
/// two or more jumps, a walk that leaves the route re-plans only to the
/// route's next jump, then rides the rest of the route as planned. A fresh
/// plan from every cell off the route chose between routes of nearly equal
/// cost from wherever the walker stood: `taiku`'s escape (the Floating
/// Castle, against its clock) set out west for the jump at (97, 101), was
/// carried a tile off it at (97, 100), re-planned east through the door at
/// (109, 78) - which carries the walker across both ways and re-walls its
/// tiles on each crossing - and spent the clock going back and forth. A
/// jump aimed at three times without landing on the route drops the
/// commitment, and the walk plans afresh.
#[allow(clippy::too_many_arguments)]
pub(super) fn committed_leg(
    session: &NativeSession,
    from: Cell,
    committed: &[Cell],
    pos: usize,
    tries: &mut HashMap<usize, u32>,
    avoid: &HashSet<(i32, i32)>,
    doors: &HashSet<(i32, i32)>,
) -> Option<Vec<Cell>> {
    if committed.is_empty() {
        return None;
    }
    if let Some(k) = committed.iter().skip(pos).position(|&c| c == from) {
        return Some(committed[pos + k + 1..].to_vec());
    }
    let entry = (pos..committed.len().saturating_sub(1))
        .find(|&i| !adjacent(committed[i], committed[i + 1]))?;
    let n = tries.entry(entry).or_insert(0);
    *n += 1;
    if *n > 3 {
        return None;
    }
    let (ex, ez) = cell_center(committed[entry]);
    let et = dispatch_tile(ex, ez);
    let sub = plan_around(session, from, (et.0 as i16, et.1 as i16), avoid, doors);
    let end = *sub.last()?;
    let (sx, sz) = cell_center(end);
    if dispatch_tile(sx, sz) != et {
        return None;
    }
    let mut out = sub;
    out.extend_from_slice(&committed[entry..]);
    Some(out)
}

/// [`plan_path`] around `avoid`; when that cannot reach `goal`, around the
/// door bands alone (a live walk-on band then gets crossed, as a player
/// crosses one that stands in the only way through).
pub(super) fn plan_around(
    session: &NativeSession,
    from: Cell,
    goal: (i16, i16),
    avoid: &HashSet<(i32, i32)>,
    doors: &HashSet<(i32, i32)>,
) -> Vec<Cell> {
    let gc = {
        let g = tile_center(goal);
        cell_of(g.0, g.1)
    };
    let miss = |p: &[Cell]| {
        p.last().map_or(i32::MAX, |c| {
            i32::from((c.0 - gc.0).abs() + (c.1 - gc.1).abs())
        })
    };
    let strict = plan_path(session, from, goal, avoid).unwrap_or_default();
    if miss(&strict) <= i32::from(TILE / SUBCELL) {
        return strict;
    }
    let loose = plan_path(session, from, goal, doors).unwrap_or_default();
    if miss(&loose) < miss(&strict) {
        loose
    } else {
        strict
    }
}

/// A route out of a wedge: the player can come to rest in the notch of a
/// stair-stepped diagonal wall, where every one of the lattice's four
/// probe-checked steps reads blocked although the free movement that got
/// it there can leave. Head for the nearest cell a step can leave; the
/// route is planned again from there.
///
/// The lattice cannot say which of those cells the free movement reaches:
/// the nearest can lie behind the wall the player is pressed against
/// (`retona`'s stair landing at (16, 99), where the cell 32 units west is
/// open and the slide toward it carries the player straight back). So each
/// call a walk makes without having left the wedge takes the next candidate
/// in turn, `nth` counting them.
pub(super) fn unwedge(session: &NativeSession, from: Cell, nth: usize) -> Vec<Cell> {
    let w = &session.host.world;
    let open = |c: Cell| {
        let (x, z) = cell_center(c);
        (0..4).any(|d| !w.field_dir_blocked(x, z, d))
    };
    // Only a real wedge: a start that can step somewhere but whose route
    // simply ends here (the goal is out of reach) is not one.
    if open(from) {
        return Vec::new();
    }
    let mut found = Vec::new();
    for r in 1..=3i16 {
        for dz in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dz.abs()) != r {
                    continue;
                }
                let c = (from.0 + dx, from.1 + dz);
                let (x, z) = cell_center(c);
                if !w.field_tile_is_wall(x, z) && open(c) {
                    found.push(c);
                }
            }
        }
        // The first call keeps to the nearest ring that holds a way out;
        // later ones may look a ring further.
        if found.len() > nth {
            break;
        }
    }
    if found.is_empty() {
        return Vec::new();
    }
    vec![found[nth % found.len()]]
}

/// The overworld's encounter step is a change of 128-unit tile, and a change
/// of one tile on **both** axes at once is one step, not two
/// (`FUN_801D9E1C` caches the tile and reads a region only when the new one
/// differs by at most one on each axis - `slti 0x2` at `0x801D9EF0` /
/// `0x801D9F08`; engine `region_encounter::is_region_step`). A player who
/// walks a diagonal through tile corners therefore drains the encounter
/// counter about half as fast as one who walks the staircase a four-way
/// lattice plans. On a crossing worn down to a lone member that is the
/// difference between two fights and three.
///
/// This is the tile route for that walk: eight-connected over tiles whose
/// four wall sub-cells are all open (a diagonal also needs both tiles it
/// cuts the corner of), every move costing one step, ending on the open
/// tile nearest `goal`. The lattice planner finishes from there.
pub(super) fn overworld_tile_route(
    session: &NativeSession,
    goal: (i16, i16),
    avoid: &HashSet<(i32, i32)>,
) -> Vec<(i32, i32)> {
    let w = &session.host.world;
    let open = |t: (i32, i32)| {
        (0..128).contains(&t.0)
            && (0..128).contains(&t.1)
            && !avoid.contains(&t)
            && [(32, 32), (96, 32), (32, 96), (96, 96)]
                .iter()
                .all(|&(dx, dz)| {
                    !w.field_tile_is_wall((t.0 * 128 + dx) as i16, (t.1 * 128 + dz) as i16)
                })
    };
    let (px, pz) = player_xz(session);
    let start = dispatch_tile(px, pz);
    if !open(start) {
        return Vec::new();
    }
    let g = (i32::from(goal.0), i32::from(goal.1));
    let far = |t: (i32, i32)| (t.0 - g.0).abs() + (t.1 - g.1).abs();
    let mut parent: HashMap<(i32, i32), (i32, i32)> = HashMap::from([(start, start)]);
    let mut q = VecDeque::from([start]);
    let mut best = start;
    while let Some(t) = q.pop_front() {
        if far(t) < far(best) {
            best = t;
        }
        for dz in -1..=1 {
            for dx in -1..=1 {
                let n = (t.0 + dx, t.1 + dz);
                if (dx, dz) == (0, 0) || parent.contains_key(&n) || !open(n) {
                    continue;
                }
                if dx != 0 && dz != 0 && !(open((t.0 + dx, t.1)) && open((t.0, t.1 + dz))) {
                    continue;
                }
                parent.insert(n, t);
                q.push_back(n);
            }
        }
    }
    let mut route = Vec::new();
    let mut t = best;
    while t != start {
        route.push(t);
        t = parent[&t];
    }
    route.reverse();
    route
}

/// The pad for one tick of an [`overworld_tile_route`] move from the tile the
/// player stands on to the adjacent `next`. A diagonal move is held only once
/// both axes are the same number of 2-unit sub-steps from their tile edge, so
/// both edges fall in one sub-step and the reader sees one step; until then
/// the axis further from its edge is walked alone - in single sub-steps when
/// close, by tapping (a released pad zeroes the overworld walk carry, so the
/// next pressed tick commits exactly one sub-step). `None` when the camera
/// gives no pad for the move's world direction.
pub(super) fn overworld_corner_pad(
    session: &NativeSession,
    next: (i32, i32),
    tapped: bool,
) -> Option<u16> {
    let (px, pz) = player_xz(session);
    let t = dispatch_tile(px, pz);
    let (dx, dz) = (next.0 - t.0, next.1 - t.1);
    let need = |p: i16, tile: i32, d: i32| -> i32 {
        if d > 0 {
            128 * (tile + 1) - i32::from(p)
        } else {
            i32::from(p) - 128 * tile + 1
        }
    };
    let pad_for = |sx: i32, sz: i32| -> Option<u16> {
        let pad = pad_for_step(session, sx as i16, sz as i16);
        let az = session
            .host
            .world
            .world_map
            .ctrl
            .as_ref()
            .map_or(0, |c| c.azimuth);
        let sx_pad = i32::from(pad & PadButton::Right.mask() != 0)
            - i32::from(pad & PadButton::Left.mask() != 0);
        let sy_pad = i32::from(pad & PadButton::Up.mask() != 0)
            - i32::from(pad & PadButton::Down.mask() != 0);
        let b = world_map_camera_relative_bits(az, sx_pad, sy_pad);
        let got = (
            i32::from(b & 0x2000 != 0) - i32::from(b & 0x8000 != 0),
            i32::from(b & 0x1000 != 0) - i32::from(b & 0x4000 != 0),
        );
        (got == (sx, sz)).then_some(pad)
    };
    if dx == 0 || dz == 0 {
        return pad_for(dx, dz);
    }
    let sx = (need(px, t.0, dx) + 1) / 2;
    let sz = (need(pz, t.1, dz) + 1) / 2;
    let (axis, gap) = match sx.cmp(&sz) {
        std::cmp::Ordering::Equal => return pad_for(dx, dz),
        std::cmp::Ordering::Greater => ((dx, 0), sx - sz),
        std::cmp::Ordering::Less => ((0, dz), sz - sx),
    };
    if gap < 3 && tapped {
        Some(0)
    } else {
        pad_for(axis.0, axis.1)
    }
}

/// Walk with the pad until the player's dispatch tile is within `within`
/// tiles of `goal`. Random encounters on the way are fled; a scripted
/// sequence the walk sets off (a band's record, a talk) is paged through.
pub(super) fn pad_walk(
    session: &mut NativeSession,
    goal: (i16, i16),
    avoid: &HashSet<(i32, i32)>,
    within: i32,
) -> Result<Walk, String> {
    let dist = |t: (i32, i32)| (t.0 - i32::from(goal.0)).abs() + (t.1 - i32::from(goal.1)).abs();
    let here = |s: &NativeSession| {
        let (x, z) = player_xz(s);
        dispatch_tile(x, z)
    };
    if dist(here(session)) <= within {
        return Ok(Walk::Arrived);
    }
    // A player low on HP heals before setting out, not after the next
    // encounter has already rolled - or, with nothing to heal with, burns an
    // Incense so the next encounter never rolls. A travel leg's encounter
    // can open on a back attack, which takes a round before the party acts
    // (`vozz`'s F4: ~120 to each member, then a caught Run), and a three-
    // monster group can catch Run two or three rounds running (`map01`'s F5
    // took ~240 from Noa over three caught rounds), so the walk sets out
    // above four fifths.
    pad_field_heal(session, 800);
    pad_field_repel(session, 500);
    if std::env::var_os("LEGAIA_FGL_WALK_DEBUG").is_some() {
        let w = &session.host.world;
        eprintln!(
            "      [walk] {} from {:?} to {goal:?}: teleports {:?}; object doors {:?}; walk-touch {:?}",
            scene_name(session),
            here(session),
            teleports(session),
            object_doors(session),
            w.props
                .walk_touch
                .iter()
                .map(|(s, (c, e))| format!("{s}@{c:?}:{e:?}"))
                .collect::<Vec<_>>()
        );
        // The scene's tile triggers: `(x,z)g<gate>r<record>`, gate 1 the
        // partition-2 walk-on bands, gate 0 the partition-0 object binds.
        if let Some((_, _, triggers)) = scene_man_and_triggers(session) {
            let list: Vec<String> = triggers
                .iter()
                .map(|t| format!("({},{})g{}r{}", t.tile_x, t.tile_z, t.gate, t.record))
                .collect();
            eprintln!("      [triggers] {}", list.join(" "));
        }
        for (s, ((x, z), _)) in &w.props.walk_touch {
            let near: Vec<String> = w
                .props
                .colliders
                .iter()
                .filter(|c| {
                    (c.center.0 - i32::from(*x)).abs() < 256
                        && (c.center.1 - i32::from(*z)).abs() < 256
                })
                .map(|c| {
                    format!(
                        "{:?} solid {} interact {} moving {} anchor {:?}",
                        c.center, c.solid, c.interact, c.moving_box, c.anchor
                    )
                })
                .collect();
            eprintln!("      [door] {s}@({x},{z}) colliders {near:?}");
        }
    }
    if std::env::var_os("LEGAIA_FGL_WALK_DEBUG").is_some() {
        let w = &session.host.world;
        for &(tx, tz) in teleports(session).keys() {
            // For each side: the cell just outside the tile's edge, stepping in.
            let c = |x: i32, z: i32| ((x * 128 + 64) as i16, (z * 128 + 64) as i16);
            let (mx, mz) = c(tx, tz);
            let sides = [
                ((mx, mz + 96), 0usize),
                ((mx + 96, mz), 1),
                ((mx, mz - 96), 2),
                ((mx - 96, mz), 3),
            ];
            let s: Vec<String> = sides
                .iter()
                .map(|&((x, z), d)| {
                    format!(
                        "{}{}",
                        if w.field_dir_blocked(x, z, d) {
                            "W"
                        } else {
                            "."
                        },
                        if w.field_actor_dir_blocked(x, z, d) {
                            "A"
                        } else {
                            "."
                        }
                    )
                })
                .collect();
            eprintln!("      [warp] ({tx},{tz}) entry from z+/x+/z-/x-: {s:?}");
        }
    }
    let walking_mode = session.host.world.mode;
    let mut doors = hazards(session, "");
    doors.remove(&(i32::from(goal.0), i32::from(goal.1)));
    // A goal the lattice cannot get near is not walked at all: wandering
    // toward it only rolls encounters (a placement parked off the map, a
    // room behind a door the flags keep shut).
    {
        let (x, z) = player_xz(session);
        let plan = plan_around(session, cell_of(x, z), goal, avoid, &doors);
        if let Some(&end) = plan.last() {
            let (ex, ez) = cell_center(end);
            let miss = dist(dispatch_tile(ex, ez));
            if miss > within + DOOR_APPROACH_SLACK {
                return Err(format!(
                    "no walkable path: the walk component ends {miss} tiles short of {goal:?} (closest {:?})",
                    dispatch_tile(ex, ez)
                ));
            }
        }
    }
    let mut best = dist(here(session));
    let mut visited: HashSet<Cell> = HashSet::new();
    let mut since = 0u32;
    let mut planned_from = None;
    // Wedge exits this walk has headed for ([`unwedge`]).
    let mut unwedged = 0usize;
    let mut scripted_next = session.host.world.encounters.scripted_formation_pending;
    let mut path = Vec::new();
    // A route through a teleport maze, committed to ([`committed_leg`]).
    let mut committed: Vec<Cell> = Vec::new();
    let mut committed_pos = 0usize;
    let mut committed_tries: HashMap<usize, u32> = HashMap::new();
    // The overworld's corner-crossing walk ([`overworld_tile_route`]).
    let mut corner_route: Vec<(i32, i32)> = Vec::new();
    let (mut corner_planned, mut corner_failed, mut corner_tapped) = (false, false, false);
    let mut corner_last: Option<(i16, i16)> = None;
    let mut corner_stuck = 0u32;
    let mut traced_tile = None;
    // Where the player stood when the previous walk frame was pressed.
    let mut pressed_at: Option<(i16, i16)> = None;
    let mut tap_owed = false;
    // Walk-time script firings per park site, with the flags they left.
    let mut refires: HashMap<String, (u32, BTreeSet<u16>)> = HashMap::new();
    // Landings of walk-time scripts that moved the walker, by site and the
    // flags they left.
    let mut landings: HashMap<(String, BTreeSet<u16>, Cell), u32> = HashMap::new();
    // Cells where the action tap opened a talk that changed nothing: the
    // stalled walker is leaning on an NPC, not on a door.
    let mut idle_taps: HashSet<Cell> = HashSet::new();
    // Talks an action tap opened that changed no flag, by park site; one
    // opened that way twice and this walk taps no more. The leaning cell
    // above misses a talk that nudges the walker a sub-cell each time
    // (`jiji`'s inn counter, P0[8..10]: the witch's "Want to stay?" on every
    // tap of the walk past it, two thousand frames each).
    let mut idle_talks: HashSet<String> = HashSet::new();
    let mut taps_off = false;
    let mut tapped_at: Option<usize> = None;
    // The idle tap is never pressed at a minigame cabinet
    // ([`minigame_talks`]): a walk that passes one does not sit down to play.
    let cabinets = if session.host.world.mode == SceneMode::Field {
        minigame_talks(session)
    } else {
        HashSet::new()
    };
    for step in 0..PAD_LEG_FRAMES as usize {
        pad_budget(session)?;
        watch_clocks(session);
        // Per overworld tile: the encounter step counter it left behind.
        if std::env::var_os("LEGAIA_FGL_WALK_DEBUG").is_some()
            && session.host.world.mode == SceneMode::WorldMap
        {
            let (x, z) = player_xz(session);
            let t = dispatch_tile(x, z);
            if traced_tile != Some(t) {
                traced_tile = Some(t);
                eprintln!(
                    "      [step] tile {t:?} counter {}",
                    session.host.world.encounters.step_counter
                );
            }
        }
        if session.host.world.mode == SceneMode::Battle {
            let trace = std::env::var_os("LEGAIA_FGL_TRACE").is_some();
            let f0 = flags_of_world(session);
            let formation = session
                .host
                .world
                .battle
                .active_formation
                .as_ref()
                .map(|f| f.formation_id);
            scripted_next |= formation.is_some()
                && STAGED_FIGHTS.with(|s| s.borrow().contains(&(scene_name(session), formation)));
            if trace {
                eprintln!(
                    "    [battle] {} ({}) at start: {}",
                    scene_name(session),
                    // The label names how the hand plays it. A fight that
                    // forbids running is fought whatever installed it (the
                    // Cort fight is entered from a band the staged-fight
                    // census does not list), and the continuous hand fights
                    // the random encounters it prices as winnable.
                    if scripted_next {
                        "scripted, fought"
                    } else if session.host.world.battle.no_escape {
                        "no escape, fought"
                    } else if continuous::PLAYS_ON.with(std::cell::Cell::get) {
                        "random, fought if winnable"
                    } else {
                        "random, fled"
                    },
                    battle_snapshot(session)
                );
            }
            // A random encounter is fled; a fight a script installed (a
            // boss stager's, which fires on the next field step) is fought.
            FLEE_ENCOUNTERS.with(|f| f.set(!scripted_next));
            let r = drain_battle(session);
            FLEE_ENCOUNTERS.with(|f| f.set(false));
            scripted_next = false;
            if trace {
                eprintln!(
                    "    [battle] ended: {r:?}; flags +{:?}",
                    flags_of_world(session)
                        .difference(&f0)
                        .map(|f| format!("0x{f:03X}"))
                        .collect::<Vec<_>>()
                );
            }
            if let Some(r) = r {
                return Err(format!("battle on the walk to {goal:?}: {r:?}"));
            }
            pad_field_heal(session, 500);
            pad_field_repel(session, 500);
            planned_from = None;
            path.clear();
            since = 0;
            continue;
        }
        if close_scripted_menu(session) {
            continue;
        }
        if session.host.world.mode != walking_mode {
            return Err(format!("mode changed to {:?}", session.host.world.mode));
        }
        scripted_next = session.host.world.encounters.scripted_formation_pending;
        let (wx, wz) = player_xz(session);
        let cell = cell_of(wx, wz);
        if planned_from != Some(cell) {
            // Follow the planned route while the player stays on it; plan
            // again only off it (a slide, a teleport, a script's move).
            if let Some(k) = committed
                .iter()
                .skip(committed_pos)
                .position(|&c| c == cell)
            {
                committed_pos += k;
            }
            if let Some(i) = path.iter().position(|&c| c == cell) {
                // On a teleport waypoint the landing is next: keep pressing
                // from the waypoint until the jump happens.
                let jump = path.get(i + 1).is_some_and(|&n| !adjacent(cell, n));
                path.drain(..if jump { i } else { i + 1 });
            } else if let Some(i) = path
                .iter()
                .take(8)
                .position(|&c| (c.0 - cell.0).abs() + (c.1 - cell.1).abs() <= 2)
            {
                // A slide a cell or two off the route rejoins it rather than
                // re-planning the whole map.
                path.drain(..i);
            } else if let Some(leg) = committed_leg(
                session,
                cell,
                &committed,
                committed_pos,
                &mut committed_tries,
                avoid,
                &doors,
            ) {
                path = leg;
            } else {
                path = plan_around(session, cell, goal, avoid, &doors);
                // The continuous hand commits to a route that crosses a
                // teleport maze ([`committed_leg`]).
                committed.clear();
                committed_pos = 0;
                committed_tries.clear();
                if continuous::PLAYS_ON.with(std::cell::Cell::get)
                    && path.windows(2).filter(|w| !adjacent(w[0], w[1])).count() >= 2
                {
                    committed = path.clone();
                }
                if std::env::var_os("LEGAIA_FGL_ROUTE_DEBUG").is_some() {
                    // The route's tiles, one per change of tile, and each
                    // jump (a teleport or a ride) marked.
                    let mut tiles: Vec<String> = Vec::new();
                    let mut last: Option<Cell> = None;
                    for &c in &path {
                        let (x, z) = cell_center(c);
                        let t = dispatch_tile(x, z);
                        let jump = last.is_some_and(|l| !adjacent(l, c));
                        let s = format!("{}{t:?}", if jump { "=>" } else { "" });
                        if jump || tiles.last().is_none_or(|p| !p.ends_with(&format!("{t:?}"))) {
                            tiles.push(s);
                        }
                        last = Some(c);
                    }
                    eprintln!(
                        "      [route] {} {:?} -> {goal:?}: {}",
                        scene_name(session),
                        here(session),
                        tiles.join(" ")
                    );
                }
                if path.is_empty() {
                    path = unwedge(session, cell, unwedged);
                    unwedged += usize::from(!path.is_empty());
                }
                // A route over a band that stages a fight walks into a boss:
                // a party not near full is topped up first, as before a
                // stager (`taiku` P2[27], Zora's F9 fight on the way out:
                // met at 56% after a fled encounter it wiped, and met at 75%
                // it still wiped, where a full party wins it).
                if !path.is_empty() && party_hp_permille(session) < 900 {
                    let fights = fight_band_tiles(session);
                    if path.iter().any(|&c| {
                        let (x, z) = cell_center(c);
                        fights.contains(&dispatch_tile(x, z))
                    }) && pad_field_heal(session, 1000) > 0
                    {
                        planned_from = None;
                        path.clear();
                        continue;
                    }
                }
            }
            planned_from = Some(cell);
        }
        // The next step is onto a pad that forks on a clock flag, and the
        // route wants the phase the clock is not in, or one that will end
        // before the pad tests it: stand at the pad's edge until it comes
        // round, as a player times the step ([`phase_ready`]).
        // A route whose first move is a jump from the walker's own cell (a
        // pad re-entered from beside it, [`plan_search`]) presses from here.
        // Only a clock pad's landing counts: `PRESS_AT` outlives the plans
        // that wrote it, and a route rejoined two cells off reads the same.
        if let Some(&first) = path.first()
            && first != cell
            && !adjacent(cell, first)
            && let Some((px, pz)) = PRESS_AT.with(|m| m.borrow().get(&cell).copied())
            && phased_pads(session)
                .get(&dispatch_tile(px, pz))
                .is_some_and(|lands| lands.iter().any(|&(l, _, _)| l == first))
        {
            path.insert(0, cell);
        }
        let wanted = phase_wanted(session, &path, cell);
        if let Some((flag, on)) = wanted
            && phase_ready(session, flag, on)
            && std::env::var_os("LEGAIA_FGL_CLOCK_DEBUG").is_some()
        {
            eprintln!(
                "      [clock] {} step toward {:?} for {flag:#05X}={on} at frame {}",
                scene_name(session),
                path.first().map(|&c| {
                    let (x, z) = cell_center(c);
                    dispatch_tile(x, z)
                }),
                session.host.world.frame
            );
        }
        if let Some((flag, on)) = wanted
            && !phase_ready(session, flag, on)
        {
            session.host.world.set_pad(0);
            match session.tick() {
                Ok(SceneTickEvent::SceneEntered { name }) => return Ok(Walk::Entered(name)),
                Ok(_) => {}
                Err(e) => return Err(format!("tick: {e:#}")),
            }
            pressed_at = None;
            continue;
        }
        let (tx, tz) = match path.first() {
            Some(&c) if c == cell => PRESS_AT
                .with(|m| m.borrow().get(&c).copied())
                .unwrap_or(cell_center(c)),
            Some(&c) => cell_center(c),
            None => tile_center(goal),
        };
        // A route that ran out short of the goal heads straight for it - but
        // not through a band the plan kept clear: a walk-on that leaves the
        // scene (`deene` P2[13] at (16..18, 23), the cutscene out to `map03`)
        // fires on the step, and the goal behind it is the next visit's, as a
        // player who walks into the cutscene finds.
        if path.is_empty() && walking_mode == SceneMode::Field {
            let t = dispatch_tile(wx, wz);
            let (dx, dz) = (tx - wx, tz - wz);
            let ahead = [
                (t.0 + i32::from(dx.signum()), t.1),
                (t.0, t.1 + i32::from(dz.signum())),
            ];
            if ahead
                .iter()
                .any(|a| *a != t && (avoid.contains(a) || doors.contains(a)))
            {
                return Err(format!(
                    "no walkable path: the way to {goal:?} from {t:?} crosses a band that leaves the scene"
                ));
            }
        }
        // A cross-axis offset of a few units is the walk's own overshoot,
        // not a step to take: chasing it flips the diagonal every frame, and
        // along a terrace edge the side-step toward the drop is a ledge hop
        // (`tunnela`'s corridor at (89, 79) hops the party down to (89, 80),
        // a one-way drop the walk then cannot climb back).
        let (dx, dz) = (tx - wx, tz - wz);
        // Unless that overshoot is what holds the walk: a body box the cell
        // centre clears and the player's own spot does not (`korb3` P0[3],
        // the gate stone, stops a walk eight units west of the corridor's
        // centre line). The continuous hand steps back onto the line.
        let squeezed = continuous::PLAYS_ON.with(std::cell::Cell::get) && {
            let dir = if dz.abs() >= dx.abs() {
                if dz < 0 { 0 } else { 2 }
            } else if dx < 0 {
                1
            } else {
                3
            };
            let w = &session.host.world;
            // A prop's box, not a person: an NPC walks off on its own, and
            // stepping round one changed the walk past `suimon`'s gate
            // keeper by a few units.
            let npc_near = w.npcs.positions.values().any(|&(x, z)| {
                (i32::from(x) - i32::from(wx)).abs() < 256
                    && (i32::from(z) - i32::from(wz)).abs() < 256
            });
            !npc_near
                && w.field_actor_dir_blocked(wx, wz, dir)
                && !w.field_actor_dir_blocked(tx, tz, dir)
        };
        let lean = |d: i16, other: i16| {
            if d.abs() <= WALK_DEADBAND && other.abs() > WALK_DEADBAND && !squeezed {
                0
            } else {
                d.signum()
            }
        };
        let mut pad = pad_for_step(session, lean(dx, dz), lean(dz, dx));
        if walking_mode == SceneMode::WorldMap && !corner_failed {
            let t = dispatch_tile(wx, wz);
            if corner_route.is_empty() && !corner_planned {
                corner_route = overworld_tile_route(session, goal, &doors);
                corner_planned = true;
            }
            if let Some(i) = corner_route.iter().position(|&r| r == t) {
                corner_route.drain(..=i);
            }
            match corner_route.first() {
                Some(&n) if (n.0 - t.0).abs() <= 1 && (n.1 - t.1).abs() <= 1 => {
                    match overworld_corner_pad(session, n, corner_tapped) {
                        Some(p) => {
                            corner_tapped = p != 0;
                            pad = p;
                        }
                        None => corner_failed = true,
                    }
                }
                // Off the route (a slide, a script's seat): plan it again
                // from here.
                Some(_) => {
                    corner_route.clear();
                    corner_planned = false;
                }
                None => {}
            }
            // Pressing without moving: the tile route met something the
            // wall bits do not show; the lattice takes over.
            if pad != 0 && corner_last == Some((wx, wz)) {
                corner_stuck += 1;
                if corner_stuck > 8 {
                    corner_failed = true;
                    corner_route.clear();
                }
            } else {
                corner_stuck = 0;
            }
            corner_last = Some((wx, wz));
        }
        // Held against something that will not give: a player tries the
        // action button (a door that opens on a press, not on contact), and
        // the route is planned afresh (an NPC walked into it).
        //
        // `since` also counts a frame that moved without improving (a run
        // step that stays inside one cell), and a tap there drops the held
        // direction for a frame - no player stops to press while still
        // walking. In a field the tap waits for a frame the player did not
        // move: standing still for one frame of every few cost a timed
        // script its window (`jouind`'s switch pair is 50 vsyncs of free walk
        // apart).
        let moved = pressed_at.is_some_and(|p| p != (wx, wz));
        if since == 0 {
            tap_owed = false;
        }
        if since > 0 && since.is_multiple_of(60) {
            path.clear();
            planned_from = None;
            pad = 0;
            tap_owed = false;
        } else {
            tap_owed |= since % 60 == 1;
            if tap_owed
                && !(moved && walking_mode == SceneMode::Field)
                && !idle_taps.contains(&cell_of(wx, wz))
                && !taps_off
                && !session
                    .host
                    .world
                    .field_interact_probe_slot()
                    .is_some_and(|s| cabinets.contains(&s))
            {
                pad = PadButton::Cross.mask();
                tap_owed = false;
                tapped_at = Some(step);
            }
        }
        // Running into a corner the wall slide deflects, the press and the
        // one after it can trade places forever: `ropeway`'s (24, 27), where
        // a Left press slid the runner 12 units south-west and the Down that
        // answers it slid it back. A player walks the corner instead - the
        // run button held against Field Move = Run walks
        // (`World::field_run_active`) - once a few frames have gone by
        // without progress.
        if since >= SLOW_WALK_AFTER
            && walking_mode == SceneMode::Field
            && pad & DIRECTIONS != 0
            && session.host.world.locomotion.run_default
        {
            pad |= PadButton::R1.mask();
        }
        pressed_at = Some((wx, wz));
        session.host.world.set_pad(pad);
        let jump_from = player_xz(session);
        let ticked = session.tick();
        note_caps(session);
        match ticked {
            Ok(SceneTickEvent::SceneEntered { name }) => return Ok(Walk::Entered(name)),
            Ok(_) => {}
            Err(e) => return Err(format!("tick: {e:#}")),
        }
        if session.host.world.mode == SceneMode::Battle {
            continue;
        }
        if std::env::var_os("LEGAIA_FGL_JUMP_DEBUG").is_some() {
            let to = player_xz(session);
            if (i32::from(to.0) - i32::from(jump_from.0)).abs()
                + (i32::from(to.1) - i32::from(jump_from.1)).abs()
                > 64
            {
                eprintln!(
                    "      [jump] {jump_from:?} -> {to:?} pad {pad:#06x} holder {} bracket {:?}",
                    holder(session),
                    session.host.world.props.arrival_exempt
                );
            }
        }
        let w = &session.host.world;
        if w.cutscene_timeline_active() || w.dialogue_owns_input() || w.active_fmv().is_some() {
            let site = format!("{} at {}", holder(session), park_site(session));
            let fired_record = timeline_p2_record(session);
            let rec_label = fired_record.map_or("-".to_string(), |r| format!("P2[{r}]"));
            let fired_at = here(session);
            // A script that changes nothing - an examined prop with nothing
            // to say (`ropeway`'s `21 26 FE FF` bind, which the stalled
            // follower's action tap keeps opening) - leaves the route as it
            // was: re-planning the whole walk component after each one costs
            // more than the walk.
            let before = (
                flags_of_world(session),
                cell_of(player_xz(session).0, player_xz(session).1),
            );
            // A band that polls the held pad once, on the tick it spawns
            // (`balden`'s elevator call bands P2[6] / P2[7], whose poll wants
            // Up toward the car), reads the pad the walk is holding: a hand
            // that walks across one going elsewhere fails the poll and walks
            // on. The scripted-sequence pad would answer the poll instead -
            // and its Up walks the player back off the band, so the walk
            // re-crosses it forever.
            let r = match poll_chain_pads(session) {
                Some(pads) if pad != 0 && !pads.contains(&pad) => {
                    session.host.world.set_pad(pad);
                    match session.tick() {
                        Ok(SceneTickEvent::SceneEntered { name }) => Run::Entered(name),
                        Ok(_) => run_while_moving(session, DEEP_EXIT_TICKS),
                        Err(e) => Run::Error(format!("{e:#}")),
                    }
                }
                _ => run_while_moving(session, DEEP_EXIT_TICKS),
            };
            if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                eprintln!(
                    "    [walk-script] tile {fired_at:?} (after: {:?}): {site} [{}] -> {r:?}; game_over {}",
                    here(session),
                    rec_label,
                    session.host.world.game_over
                );
            }
            match r {
                Run::Entered(s) => return Ok(Walk::Entered(s)),
                Run::Released => {
                    if let Some(rec) = fired_record
                        && session.host.world.dialog.three_actor_talk.is_some()
                    {
                        RAN_ON_WALK.with(|t| t.borrow_mut().insert((scene_name(session), rec)));
                    }
                }
                other => return Err(format!("scripted sequence on the walk: {other:?}")),
            }
            // A band whose record stages a fight (`3E FF row`) is a boss's:
            // `taiku` P2[27], Zora's "I will fight you", ends its talk on
            // the F9 fight. The battle opens after the walk that crossed the
            // band has returned, so the pending latch this walk polls never
            // shows it; the band's rows join the staged set the next walk
            // reads, which fights it instead of fleeing a fight it cannot
            // run from.
            if let Some(rec) = fired_record
                && let Some((mf, man, _)) = scene_man_and_triggers(session)
            {
                let here_name = scene_name(session);
                for row in staged_formation_rows(&mf, &man, 2, usize::from(rec)) {
                    STAGED_FIGHTS.with(|s| s.borrow_mut().insert((here_name.clone(), Some(row))));
                    scripted_next = true;
                }
            }
            let (px, pz) = player_xz(session);
            // The talk a tap opens owns the pad a tick or two after it.
            if tapped_at.is_some_and(|t| step.saturating_sub(t) <= 3)
                && site.starts_with("dialogue")
                && before.0 == flags_of_world(session)
                && !idle_talks.insert(site.clone())
            {
                taps_off = true;
            }
            if before != (flags_of_world(session), cell_of(px, pz)) {
                planned_from = None;
                path.clear();
            } else if pad == PadButton::Cross.mask() && site.starts_with("dialogue") {
                // The tap talked to whoever the walker is pressed against
                // and nothing came of it: tapping there again only pages
                // the same conversation.
                idle_taps.insert(before.1);
            }
            // A band that turns the walk back every time it is crossed
            // (`retock` P2[26], the Mt. Letona checkpoint: "Passage
            // forbidden" without Lord Saryu's key, then a step back) is a
            // wall to this walk; re-crossing it burns the segment budget.
            let flags_now = flags_of_world(session);
            // The script itself moved the walker and left the flags as
            // they were: a refusal's step back, not a band walked through
            // (`nilboa` P2[26] / P2[27] re-dress the boulder room in place).
            let turned_back = before.0 == flags_now && before.1 != cell_of(px, pz);
            // Keyed on the record as well as the park site: `jouinc`'s
            // camera bands P2[3..57], one every three tiles of the way to
            // `jouind`, all park at `pc=0x000C op=0x50`, and walking past
            // twenty-five of them read as one script firing 25 times.
            let key = format!("{site} {rec_label}");
            let n = refires.entry(key.clone()).or_insert((0, flags_now.clone()));
            if n.1 == flags_now {
                n.0 += 1;
            } else {
                *n = (1, flags_now.clone());
            }
            // On a split party, a band that fired twice on one walk and
            // changed nothing turned the walker back (`nilboa`'s tunnel
            // mouths: "go back outside?", No, a step back): the walk cannot
            // pass it, and the band is a wall to every later plan.
            if session.host.world.dialog.three_actor_talk.is_some()
                && n.0 >= 2
                && turned_back
                && let Some(rec) = fired_record
            {
                TURNBACK.with(|t| t.borrow_mut().insert((scene_name(session), rec)));
                session.host.world.set_pad(0);
                return Err(format!(
                    "no walkable path: band P2[{rec}] turned the walk to {goal:?} back at {:?}",
                    here(session)
                ));
            }
            if n.0 > WALK_SCRIPT_REFIRES {
                return Err(format!(
                    "pad walk stalled at tile {:?}: a script on the walk fired {} times and changed nothing",
                    here(session),
                    n.0
                ));
            }
            // A script that carries the walker somewhere it has already
            // carried it, leaving the same flags, is a loop rather than
            // progress, even when the flags it toggles keep the count above
            // from building: `chitei2`'s transport rides between (98, 102)
            // and (94, 22), each ride flipping the car's flags, and the
            // walk rode it back and forth for most of a segment's budget.
            if before.1 != cell_of(px, pz) {
                let c = landings
                    .entry((key, flags_now.clone(), cell_of(px, pz)))
                    .or_insert(0u32);
                *c += 1;
                if *c > WALK_SCRIPT_CYCLES {
                    return Err(format!(
                        "pad walk stalled at tile {:?}: a script on the walk carried it to the same spot {} times",
                        here(session),
                        *c
                    ));
                }
            }
        }
        let d = dist(here(session));
        if d <= within {
            session.host.world.set_pad(0);
            return Ok(Walk::Arrived);
        }
        let (px, pz) = player_xz(session);
        if d < best || visited.insert(cell_of(px, pz)) {
            best = best.min(d);
            since = 0;
        } else {
            since += 1;
            if since >= PAD_STALL_FRAMES {
                session.host.world.set_pad(0);
                let (px, pz) = player_xz(session);
                let next: Vec<(i16, i16)> = path.iter().take(4).map(|&c| cell_center(c)).collect();
                let w = &session.host.world;
                if std::env::var_os("LEGAIA_FGL_WALK_DEBUG").is_some() {
                    let lock = w
                        .player_actor_slot
                        .and_then(|sl| w.actors.get(usize::from(sl)))
                        .map(|a| a.move_state.flags);
                    eprintln!(
                        "      [stall] holder {} mode {:?} player flags {lock:x?} pad {:#06x} next {next:?} here-blocked {:?} actor {:?}",
                        holder(session),
                        w.mode,
                        w.input.pad(),
                        (0..4)
                            .map(|d| w.field_dir_blocked(px, pz, d))
                            .collect::<Vec<_>>(),
                        (0..4)
                            .map(|d| w.field_actor_dir_blocked(px, pz, d))
                            .collect::<Vec<_>>()
                    );
                    for c in &w.props.colliders {
                        if (c.center.0 - i32::from(px)).abs() < 400
                            && (c.center.1 - i32::from(pz)).abs() < 400
                        {
                            eprintln!(
                                "      [stall-prop] centre {:?} live {:?} solid {} moving {} interact {} anchor {:?}",
                                c.center, c.live, c.solid, c.moving_box, c.interact, c.anchor
                            );
                        }
                    }
                    if std::env::var_os("LEGAIA_FGL_STALL_PRESSES").is_some() {
                        // Debug only: hold each press for 20 ticks from here
                        // and print where it leaves the player.
                        let presses = [
                            ("U", PadButton::Up.mask()),
                            ("D", PadButton::Down.mask()),
                            ("L", PadButton::Left.mask()),
                            ("R", PadButton::Right.mask()),
                            ("UL", PadButton::Up.mask() | PadButton::Left.mask()),
                            ("UR", PadButton::Up.mask() | PadButton::Right.mask()),
                            ("DL", PadButton::Down.mask() | PadButton::Left.mask()),
                            ("DR", PadButton::Down.mask() | PadButton::Right.mask()),
                        ];
                        for (name, p) in presses {
                            let start = player_xz(session);
                            for _ in 0..20 {
                                session.host.world.set_pad(p);
                                let _ = session.tick();
                            }
                            session.host.world.set_pad(0);
                            let end = player_xz(session);
                            eprintln!(
                                "      [press] {name}: {start:?} -> {end:?} az {}",
                                session.host.world.locomotion.camera_azimuth
                            );
                            // Back to where the stall stood.
                            for _ in 0..20 {
                                let (x, z) = player_xz(session);
                                if (x, z) == start {
                                    break;
                                }
                                let back = pad_for_step(
                                    session,
                                    (start.0 - x).signum(),
                                    (start.1 - z).signum(),
                                );
                                session.host.world.set_pad(back);
                                let _ = session.tick();
                            }
                            session.host.world.set_pad(0);
                        }
                    }
                    let w = &session.host.world;
                    // 64-unit wall sub-cells around the player, Z rows. The
                    // goal tile reads `G` where it is wall, `g` where open.
                    for dz in -24i16..=24 {
                        let row: String = (-24i16..=24)
                            .map(|dx| {
                                let (x, z) = (px + dx * 64, pz + dz * 64);
                                if dx == 0 && dz == 0 {
                                    '@'
                                } else if dispatch_tile(x, z)
                                    == (i32::from(goal.0), i32::from(goal.1))
                                {
                                    if w.field_tile_is_wall(x, z) { 'G' } else { 'g' }
                                } else if w.field_tile_is_wall(x, z) {
                                    '#'
                                } else {
                                    '.'
                                }
                            })
                            .collect();
                        eprintln!("      [grid] {row}");
                    }
                }
                let w = &session.host.world;
                let (cx, cz) = cell_center(cell_of(px, pz));
                let block: String = (0..4)
                    .map(|dir| {
                        match (
                            w.field_dir_blocked(cx, cz, dir),
                            w.field_actor_dir_blocked(cx, cz, dir),
                        ) {
                            (true, _) => 'W',
                            (false, true) => 'A',
                            _ => '.',
                        }
                    })
                    .collect();
                return Err(format!(
                    "pad walk stalled at tile {:?} (world ({px},{pz})), {d} tiles short of {goal:?}; route head {next:?}; cell Z-/X-/Z+/X+ {block}",
                    here(session)
                ));
            }
        }
    }
    session.host.world.set_pad(0);
    Err(format!("pad walk to {goal:?} ran out of frames"))
}

/// Hold the d-pad toward world point `at` until `done` holds, for at most
/// `frames` ticks. A press toward a solid body turns the player to face it
/// and the collision stops the step, which is how a player lines up a talk
/// or leans on a door. `Some(scene)` if a scene change landed.
pub(super) fn pad_lean(
    session: &mut NativeSession,
    at: impl Fn(&NativeSession) -> Option<(i16, i16)>,
    frames: usize,
    done: impl Fn(&NativeSession) -> bool,
) -> Option<String> {
    for _ in 0..frames {
        if done(session) || !walking(session) {
            break;
        }
        let Some((ax, az)) = at(session) else { break };
        let (px, pz) = player_xz(session);
        let axis = |d: i16| if d.abs() > 16 { d.signum() } else { 0 };
        let pad = pad_for_step(session, axis(ax - px), axis(az - pz));
        session.host.world.set_pad(pad);
        if let Ok(SceneTickEvent::SceneEntered { name }) = session.tick() {
            session.host.world.set_pad(0);
            return Some(name);
        }
    }
    session.host.world.set_pad(0);
    None
}

/// The four tiles beside `tile`, nearest to the player first, skipping any
/// that carry a `.MAP` trigger (standing there would run its record).
pub(super) fn beside(session: &NativeSession, tile: (i16, i16)) -> Vec<(i16, i16)> {
    let claimed = claimed_tiles(session);
    let (px, pz) = player_xz(session);
    let me = dispatch_tile(px, pz);
    // A side whose shared edge with the band is a wall goes last: `taiku`'s
    // P2[15] (16, 28) lies in a corridor whose west wall the open collision
    // outside runs along, and the band is reached up the corridor, never
    // across the wall. The test is the locomotion's own leading-edge probe
    // (`FUN_801CFE4C`) swept over the half tile from the side's centre to the
    // shared edge. Its corner probes also catch the side walls of a one-tile
    // passage, so a walled side is demoted, not dropped.
    let w = &session.host.world;
    let walled = |from: (i16, i16)| {
        let (ax, az) = tile_center(from);
        let (sx, sz) = (tile.0 - from.0, tile.1 - from.1);
        let dir = match (sx, sz) {
            (0, -1) => 0,
            (-1, 0) => 1,
            (0, 1) => 2,
            _ => 3,
        };
        (0..=4).any(|k| w.field_dir_blocked(ax + sx * 16 * k, az + sz * 16 * k, dir))
    };
    let mut out: Vec<(i16, i16)> = [(-1i16, 0i16), (1, 0), (0, -1), (0, 1)]
        .iter()
        .map(|&(dx, dz)| (tile.0 + dx, tile.1 + dz))
        .filter(|&(x, z)| {
            (0..128).contains(&x) && (0..128).contains(&z) && !claimed.contains(&(x as u8, z as u8))
        })
        .collect();
    out.sort_by_key(|&t| {
        (
            w.mode == SceneMode::Field && walled(t),
            (i32::from(t.0) - me.0).abs() + (i32::from(t.1) - me.1).abs(),
        )
    });
    out
}

/// Step onto walk-on `tile` with the pad: walk to an inert tile beside it,
/// then onto it, so the dispatch sees a genuine tile change.
pub(super) fn pad_step_onto(session: &mut NativeSession, tile: (u8, u8)) -> Result<Walk, String> {
    let goal = (i16::from(tile.0), i16::from(tile.1));
    let avoid = pad_avoid(session, None);
    let mut last = String::from("no inert tile beside it");
    for side in beside(session, goal) {
        match pad_walk(session, side, &avoid, 0) {
            Ok(Walk::Entered(s)) => return Ok(Walk::Entered(s)),
            Ok(Walk::Arrived) => {}
            Err(e) => {
                last = e;
                continue;
            }
        }
        let onto = pad_avoid(session, Some(goal));
        match pad_walk(session, goal, &onto, 0) {
            Err(e) if !on_tile(session, goal) => {
                last = e;
                break;
            }
            r => return r,
        }
    }
    // No side reaches it: a teleport whose landing is the band is a tile
    // change onto it too. `taiku`'s exit band P2[15] (16, 28) sits in a
    // walled corridor whose only way in is the pad at (84, 59).
    let mut sources: Vec<(i16, i16)> = teleports(session)
        .iter()
        .filter(|&(_, &c)| tile_of(cell_center(c).0, cell_center(c).1) == goal)
        .map(|(&(x, z), _)| (x as i16, z as i16))
        .collect();
    sources.sort_unstable();
    for src in sources {
        let onto = pad_avoid(session, Some(src));
        match pad_walk(session, src, &onto, 0) {
            Ok(Walk::Entered(s)) => return Ok(Walk::Entered(s)),
            Ok(Walk::Arrived) => return Ok(Walk::Arrived),
            Err(_) if on_tile(session, goal) => return Ok(Walk::Arrived),
            Err(e) => last = format!("{last}; by the teleport at {src:?}: {e}"),
        }
    }
    Err(format!("pad step onto {tile:?}: {last}"))
}

/// Whether the player stands on `tile`'s dispatch tile.
pub(super) fn on_tile(session: &NativeSession, tile: (i16, i16)) -> bool {
    let (x, z) = player_xz(session);
    dispatch_tile(x, z) == (i32::from(tile.0), i32::from(tile.1))
}
