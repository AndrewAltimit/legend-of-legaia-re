//! The pad tier's path planner: the sub-cell grid, the bodies that block
//! it, and the search that plans a walk to a goal tile.

use super::*;

// ---------------------------------------------------------------------------
// Pad walking (a condensed critical_path_replay follower)
// ---------------------------------------------------------------------------

pub(super) const SUBCELL: i16 = 32;
pub(super) const TILE: i16 = 128;
pub(super) const PAD_LEG_FRAMES: u32 = 12_000;
pub(super) const PAD_STALL_FRAMES: u32 = 300;
/// Times one walk-time script may fire on a single pad walk without changing
/// a flag before the walk gives up on that way.
pub(super) const WALK_SCRIPT_REFIRES: u32 = 24;
/// Times one walk-time script may carry the walker to the same landing,
/// leaving the same flags, before the walk gives up on that way.
pub(super) const WALK_SCRIPT_CYCLES: u32 = 3;
/// Cross-axis distance (world units) the pad walk does not correct while it
/// still has ground to cover on the other axis.
pub(super) const WALK_DEADBAND: i16 = 8;
pub(super) const MAX_PLAN_NODES: usize = 120_000;
/// Tiles short of a door the lattice may end before a pad hop is called
/// unwalkable rather than attempted.
pub(super) const DOOR_APPROACH_SLACK: i32 = 6;
/// What a route pays, in lattice steps, to wait on a clock-flag pad's edge
/// for the phase the flag is not in ([`phased_pads`]): a walk around is
/// taken when it is shorter than the wait.
pub(super) const PHASE_WAIT_COST: i32 = 40;
pub(super) type Cell = (i16, i16);

pub(super) fn cell_of(x: i16, z: i16) -> Cell {
    ((x + SUBCELL / 2) / SUBCELL, (z + SUBCELL / 2) / SUBCELL)
}
pub(super) fn cell_center(c: Cell) -> (i16, i16) {
    (c.0 * SUBCELL, c.1 * SUBCELL)
}
pub(super) fn tile_center(t: (i16, i16)) -> (i16, i16) {
    (t.0 * TILE + 0x40, t.1 * TILE + 0x40)
}
pub(super) fn dispatch_tile(x: i16, z: i16) -> (i32, i32) {
    (i32::from(x) >> 7, i32::from(z) >> 7)
}
pub(super) const STEPS: [((i16, i16), usize); 4] =
    [((0, -1), 0), ((-1, 0), 1), ((0, 1), 2), ((1, 0), 3)];

pub(super) fn pad_for_step(session: &NativeSession, dwx: i16, dwz: i16) -> u16 {
    let mut pad = 0u16;
    let (sx, sy) = if session.host.world.mode == SceneMode::WorldMap {
        let az = session
            .host
            .world
            .world_map
            .ctrl
            .as_ref()
            .map_or(0, |c| c.azimuth);
        if dwx == 0 && dwz == 0 {
            return 0;
        }
        // Invert the overworld remap exactly: of the eight pad directions,
        // take the one whose world bits are the wanted step's, else the one
        // sharing most of them with none opposed. A rounded rotation turns a
        // cardinal step into a diagonal whenever the camera sits off an
        // axis, and a diagonal cannot thread a one-tile cave mouth.
        let want = |b: u16| -> (i16, i16) {
            (
                i16::from(b & 0x2000 != 0) - i16::from(b & 0x8000 != 0),
                i16::from(b & 0x1000 != 0) - i16::from(b & 0x4000 != 0),
            )
        };
        let target = (dwx.signum(), dwz.signum());
        let mut best: Option<(i32, (i32, i32))> = None;
        for sx in -1..=1i32 {
            for sy in -1..=1i32 {
                if sx == 0 && sy == 0 {
                    continue;
                }
                let (wx, wz) = want(world_map_camera_relative_bits(az, sx, sy));
                let axis = |w: i16, t: i16| -> i32 {
                    if w == t {
                        2
                    } else if w == 0 || t == 0 {
                        0
                    } else {
                        -4
                    }
                };
                let score = axis(wx, target.0) + axis(wz, target.1);
                if best.is_none_or(|(b, _)| score > b) {
                    best = Some((score, (sx, sy)));
                }
            }
        }
        best.map_or((0, 0), |(_, (sx, sy))| (sx as i16, sy as i16))
    } else {
        if dwx == 0 && dwz == 0 {
            return 0;
        }
        field_step_press(session.host.world.locomotion.camera_azimuth, dwx, dwz)
    };
    if sy > 0 {
        pad |= PadButton::Up.mask();
    } else if sy < 0 {
        pad |= PadButton::Down.mask();
    }
    if sx > 0 {
        pad |= PadButton::Right.mask();
    } else if sx < 0 {
        pad |= PadButton::Left.mask();
    }
    pad
}

/// The d-pad press, as `(right, up)` signs, whose field remap at camera
/// `azimuth` walks the world step `(dwx, dwz)` best.
pub(super) fn field_step_press(azimuth: u16, dwx: i16, dwz: i16) -> (i16, i16) {
    // Invert the field remap the engine applies: the held d-pad is
    // rotated by the camera azimuth in eighth turns around the
    // eight-direction ring (`World::remap_pad_direction`, rotation index
    // `((az + 0x100) >> 9) & 7`). Rounding to quarter turns agrees with
    // it only on an axis-aligned camera; on a diagonal one a diagonal
    // press came out a cardinal - `nilboa`'s way down to the Koru ledge
    // pressed into the wall at (98, 107) east of the walk.
    let az = u32::from(azimuth);
    let rot = (az.wrapping_add(0x100) >> 9) & 7;
    let target = (dwx.signum(), dwz.signum());
    let mut best: Option<(i32, (i16, i16))> = None;
    for sx in -1..=1i16 {
        for sy in -1..=1i16 {
            if sx == 0 && sy == 0 {
                continue;
            }
            let mut held = 0u16;
            if sy > 0 {
                held |= 0x1000;
            } else if sy < 0 {
                held |= 0x4000;
            }
            if sx > 0 {
                held |= 0x2000;
            } else if sx < 0 {
                held |= 0x8000;
            }
            let r = legaia_engine_core::world::World::remap_pad_direction(held, rot);
            let has = |b: u16| i16::from(r & b != 0);
            let (wx, wz) = (has(0x2000) - has(0x8000), has(0x1000) - has(0x4000));
            let axis = |w: i16, t: i16| -> i32 {
                if w == t {
                    2
                } else if w == 0 || t == 0 {
                    0
                } else {
                    -4
                }
            };
            let score = axis(wx, target.0) + axis(wz, target.1);
            if best.is_none_or(|(b, _)| score > b) {
                best = Some((score, (sx, sy)));
            }
        }
    }
    best.map_or((0, 0), |(_, s)| s)
}

/// The actor-collision boxes one plan tests against, bucketed by the
/// 128-unit tiles each overlaps: every field NPC at its live position
/// (±40) and every solid placed prop (±80 static, ±40 moving). The
/// planner's copy of `World::field_actor_dir_blocked`, built once per plan
/// so a whole-map search does not re-walk every body per probe.
/// A body box: centre `(x, z)` and half-extent.
pub(super) type BodyBox = (i32, i32, i32);

pub(super) struct Blockers(HashMap<(i32, i32), Vec<BodyBox>>);

impl Blockers {
    pub(super) fn build(
        w: &legaia_engine_core::world::World,
        pass: impl Fn(&legaia_engine_core::world::FieldPropCollider) -> bool,
    ) -> Self {
        const NPC_HALF: i32 = 0x40 - 0x18;
        let npcs_off = NPC_BODIES_OFF.with(std::cell::Cell::get);
        let mut boxes: Vec<(i32, i32, i32)> = w
            .npcs
            .positions
            .values()
            .filter(|_| !npcs_off)
            .map(|&(x, z)| (i32::from(x), i32::from(z), NPC_HALF))
            .collect();
        for c in &w.props.colliders {
            if !c.solid || pass(c) {
                continue;
            }
            boxes.push(if c.moving_box {
                (c.live.0, c.live.1, NPC_HALF)
            } else {
                (c.center.0, c.center.1, TOUCH_BOX_HALF)
            });
        }
        let mut map: HashMap<(i32, i32), Vec<BodyBox>> = HashMap::new();
        for b in boxes {
            let (x, z, h) = b;
            for tz in ((z - h) >> 7)..=((z + h) >> 7) {
                for tx in ((x - h) >> 7)..=((x + h) >> 7) {
                    map.entry((tx, tz)).or_default().push(b);
                }
            }
        }
        Self(map)
    }

    /// Whether a body box covers `(px, pz)`, ignoring the static boxes
    /// centred on `skip` (the platforms under the arrival bracket).
    pub(super) fn hit_except(&self, px: i32, pz: i32, skip: &[(i16, i16)]) -> bool {
        self.0.get(&(px >> 7, pz >> 7)).is_some_and(|v| {
            v.iter().any(|&(x, z, h)| {
                (px - x).abs() < h
                    && (pz - z).abs() < h
                    && !(h == TOUCH_BOX_HALF
                        && skip
                            .iter()
                            .any(|&(sx, sz)| (i32::from(sx), i32::from(sz)) == (x, z)))
            })
        })
    }

    pub(super) fn dir_blocked_except(
        &self,
        cx: i16,
        cz: i16,
        dir: usize,
        skip: &[(i16, i16)],
    ) -> bool {
        ACTOR_PROBES[dir].iter().any(|&(dx, dz)| {
            self.hit_except(
                i32::from(cx) + i32::from(dx),
                i32::from(cz) - i32::from(dz),
                skip,
            )
        })
    }
}

thread_local! {
    /// Teleport waypoints of the routes [`plan_path`] returned: the cell a
    /// route steps into a teleport tile or against an object door from, and
    /// the world point the follower presses toward there (the teleport
    /// tile's centre, the door's contact centre). The route's next cell is
    /// the landing, which no press reaches.
    pub(super) static PRESS_AT: std::cell::RefCell<HashMap<Cell, (i16, i16)>> =
        std::cell::RefCell::new(HashMap::new());
}

thread_local! {
    /// Set while [`cross_over`] walks back out of a crossing scene: the
    /// pad hop then takes the reachable door farthest from where the player
    /// came in, instead of the nearest (which is the one it came in by).
    pub(super) static FAR_DOOR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Set while [`cross_over`] walks back out of a crossing scene whose
    /// round trip the lattice planned: the landing tile the plan needs. The
    /// pad hop then prefers the doors whose own record lands there.
    pub(super) static WANT_LANDING: std::cell::RefCell<Option<(i16, i16)>> =
        const { std::cell::RefCell::new(None) };
}

/// Is `b` one lattice step from `a`?
pub(super) fn adjacent(a: Cell, b: Cell) -> bool {
    (a.0 - b.0).abs() + (a.1 - b.1).abs() <= 1
}

/// One step of [`plan_path`]'s search: reach `to` from `cur` at `cost`,
/// through the waypoint `via` when the step is a teleport.
pub(super) struct Search {
    pub(super) parent: HashMap<Cell, Cell>,
    pub(super) g: HashMap<Cell, i32>,
    pub(super) open: std::collections::BinaryHeap<std::cmp::Reverse<(i32, i32, Cell)>>,
    pub(super) goal: Cell,
}

impl Search {
    pub(super) fn h(&self, c: Cell) -> i32 {
        i32::from((c.0 - self.goal.0).abs() + (c.1 - self.goal.1).abs())
    }

    pub(super) fn step(&mut self, via: Option<Cell>, cur: Cell, to: Cell, cost: i32) {
        if self.g.get(&to).is_some_and(|&old| old <= cost) {
            return;
        }
        match via {
            Some(v) => {
                // The waypoint's cost is recorded too: every parent edge
                // must run from a cheaper cell to a dearer one, or a later
                // plain step into `v` (from a cell descended from `to`)
                // overwrites `parent[v]` and closes a cycle that the path
                // walk-back follows forever.
                self.parent.insert(v, cur);
                self.g.insert(v, cost - 1);
                self.parent.insert(to, v);
            }
            None => {
                self.parent.insert(to, cur);
            }
        }
        self.g.insert(to, cost);
        let f = cost + self.h(to);
        self.open.push(std::cmp::Reverse((f, cost, to)));
    }
}

thread_local! {
    /// Plans run and cells they expanded, for the trace's cost line.
    pub(super) static PLAN_STATS: std::cell::Cell<(u64, u64)> = const { std::cell::Cell::new((0, 0)) };
}

/// A* over the collision lattice toward `goal`, never entering an `avoid`
/// dispatch tile (retail fires on a tile change, so occupying one is safe).
/// A step into a kind-0 teleport tile, or against an object door's contact
/// box, continues from its landing cell, so a route may pass through a door
/// into a room laid out elsewhere in the map. When the goal is out of
/// reach the route ends at the reachable cell nearest it.
pub(super) fn plan_path(
    session: &NativeSession,
    from: Cell,
    goal: (i16, i16),
    avoid: &HashSet<(i32, i32)>,
) -> Option<Vec<Cell>> {
    // A goal outside the start's walk component costs a whole-component
    // search every time it is asked for, and a stalled walk asks every
    // second. The component and its nearest cell do not move while the
    // scene, the flags and the avoid set stand, so a start inside a
    // remembered failure plans straight to that cell instead.
    let key = plan_key(session, goal, avoid);
    let hit = UNREACHED.with(|u| {
        u.borrow()
            .get(&key)
            .filter(|(seen, _)| seen.contains(&from))
            .map(|&(_, best)| best)
    });
    if let Some(best) = hit {
        if best == from {
            return None;
        }
        if let Some((path, _, _)) = plan_search(session, from, goal, Some(best), avoid)
            && path.last() == Some(&best)
        {
            return Some(path);
        }
    }
    let (path, seen, reached) = plan_search(session, from, goal, None, avoid)?;
    // The NPC boxes are the conservative half of the planner: a body
    // tests every probe of a lattice cell, where the player slides past
    // one at sub-cell offsets the lattice does not hold. Two NPCs standing
    // in a corridor mouth then read as a wall the player walks through:
    // `deene`'s entry corridor, sealed by P1[4] and P1[5] at (18, 46) /
    // (17, 46), which the player leaves between them at (17, 47). When the
    // bodies alone keep the goal out of reach, the route ignores them and
    // the follower's stall handling answers a body that does block.
    if !reached {
        let open = NPC_BODIES_OFF.with(|f| {
            f.set(true);
            let r = plan_search(session, from, goal, None, avoid);
            f.set(false);
            r
        });
        if let Some((p, _, true)) = open {
            return Some(p);
        }
    }
    if !reached && std::env::var_os("LEGAIA_FGL_COMP_DEBUG").is_some() {
        let tiles: HashSet<(i32, i32)> = seen
            .iter()
            .map(|&c| {
                let (x, z) = cell_center(c);
                dispatch_tile(x, z)
            })
            .collect();
        let bl = Blockers::build(&session.host.world, |_| false);
        eprintln!(
            "      [comp] {} goal {goal:?} cells {}",
            scene_name(session),
            seen.len()
        );
        {
            // The same question at wall-bit granularity: 64-unit sub-cells,
            // a sub-cell open when its centre reads no wall, four-connected.
            let w = &session.host.world;
            let open = |sx: i32, sz: i32| {
                (0..256).contains(&sx)
                    && (0..256).contains(&sz)
                    && !w.field_tile_is_wall((sx * 64 + 32) as i16, (sz * 64 + 32) as i16)
            };
            let (fx, fz) = cell_center(from);
            let s0 = (i32::from(fx) >> 6, i32::from(fz) >> 6);
            let mut seen2: HashSet<(i32, i32)> = HashSet::from([s0]);
            let mut q = VecDeque::from([s0]);
            while let Some((x, z)) = q.pop_front() {
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let n = (x + dx, z + dz);
                    if open(n.0, n.1) && !avoid.contains(&(n.0 >> 1, n.1 >> 1)) && seen2.insert(n) {
                        q.push_back(n);
                    }
                }
            }
            let g = (i32::from(goal.0), i32::from(goal.1));
            let near = seen2
                .iter()
                .map(|&(x, z)| ((x >> 1) - g.0).abs() + ((z >> 1) - g.1).abs())
                .min()
                .unwrap_or(i32::MAX);
            eprintln!(
                "      [comp] sub-cell flood: {} sub-cells, nearest tile to goal {near} away",
                seen2.len()
            );
        }
        let w = &session.host.world;
        for (cfg, &(x, z)) in w
            .world_map
            .entity_configs
            .iter()
            .zip(w.world_map.entity_positions.iter())
        {
            eprintln!("      [comp-ent] ({},{}) {cfg:?}", x >> 7, z >> 7);
        }
        for (slot, &(x, z)) in &w.npcs.positions {
            eprintln!("      [comp-npc] slot {slot:?} at ({},{})", x >> 7, z >> 7);
        }
        eprintln!("      [comp-open] {:?}", self_opening_doors(session));
        for c in w.props.colliders.iter().filter(|c| c.solid) {
            eprintln!(
                "      [comp-prop] ({},{}) anchor {:?} moving {} interact {} P0[{:?}]",
                c.center.0 >> 7,
                c.center.1 >> 7,
                c.anchor,
                c.moving_box,
                c.interact,
                c.anchor
                    .and_then(|a| w.props.bank.props.get(&a))
                    .map(|p| p.record)
            );
        }
        for tz in 0..128 {
            let row: String = (0..128)
                .map(|tx| {
                    if (tx, tz) == (i32::from(goal.0), i32::from(goal.1)) {
                        'G'
                    } else if avoid.contains(&(tx, tz)) {
                        'A'
                    } else if tiles.contains(&(tx, tz)) {
                        'o'
                    } else if bl.0.contains_key(&(tx, tz)) {
                        'b'
                    } else if session
                        .host
                        .world
                        .field_tile_is_wall((tx * 128 + 64) as i16, (tz * 128 + 64) as i16)
                    {
                        '#'
                    } else {
                        '.'
                    }
                })
                .collect();
            eprintln!("      [comp {tz:3}] {row}");
        }
    }
    if !reached && seen.len() > UNREACHED_MIN_CELLS {
        let best = path.last().copied().unwrap_or(from);
        UNREACHED.with(|u| {
            let mut u = u.borrow_mut();
            if u.len() > 64 {
                u.clear();
            }
            u.insert(key, (seen, best));
        });
    }
    (!path.is_empty()).then_some(path)
}

/// Searches smaller than this are cheap enough to repeat.
pub(super) const UNREACHED_MIN_CELLS: usize = 4_000;

/// What a failed plan's answer depends on: the scene, the goal, the avoid
/// set and the story flags (which open and shut doors and paint walls).
pub(super) type PlanKey = (String, (i16, i16), u64, u64);

thread_local! {
    /// Whether [`Blockers::build`] leaves the field NPCs' bodies out: the
    /// second search of a plan the bodies alone kept from its goal
    /// ([`plan_path`]).
    pub(super) static NPC_BODIES_OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Failed plans: the cells the search reached and the one nearest the
    /// goal, by [`PlanKey`].
    pub(super) static UNREACHED: std::cell::RefCell<HashMap<PlanKey, (HashSet<Cell>, Cell)>> =
        std::cell::RefCell::new(HashMap::new());
}

pub(super) fn plan_key(
    session: &NativeSession,
    goal: (i16, i16),
    avoid: &HashSet<(i32, i32)>,
) -> PlanKey {
    use std::hash::{Hash, Hasher};
    let mut a: Vec<&(i32, i32)> = avoid.iter().collect();
    a.sort();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    a.hash(&mut h);
    let mut f = std::collections::hash_map::DefaultHasher::new();
    session.host.world.flags.system_flags.hash(&mut f);
    (scene_name(session), goal, h.finish(), f.finish())
}

/// The A* behind [`plan_path`]: the route toward `goal`'s tile - or toward
/// `target` when one is given, a cell already known reachable - the cells
/// the search reached, and whether it reached its aim.
#[allow(clippy::type_complexity)]
pub(super) fn plan_search(
    session: &NativeSession,
    from: Cell,
    goal: (i16, i16),
    target: Option<Cell>,
    avoid: &HashSet<(i32, i32)>,
) -> Option<(Vec<Cell>, HashSet<Cell>, bool)> {
    let w = &session.host.world;
    let gw = tile_center(goal);
    // A goal tile whose centre is wall (a door band set in a wall strip)
    // is aimed at through its open part: the dispatch fires on entering the
    // tile, and the open corner says which side a player steps in from.
    // `teien`'s way down, band (42..43, 29), is wall but for its upper-left
    // sub-cell, so it is entered from the corridor above, never from below.
    let gc = target.unwrap_or_else(|| {
        let c = cell_of(gw.0, gw.1);
        let open = |c: Cell| {
            let (x, z) = cell_center(c);
            !w.field_tile_is_wall(x, z)
        };
        if open(c) {
            return c;
        }
        tile_cells(goal)
            .filter(|&t| open(t))
            .min_by_key(|t| (t.0 - c.0).abs() + (t.1 - c.1).abs())
            .unwrap_or(c)
    });
    let goal_tile = (i32::from(goal.0), i32::from(goal.1));
    let warps = teleports(session);
    let phased = phased_pads(session);
    let doors = object_doors(session);
    // The platforms under the engine's live arrival bracket: a plan that
    // starts on one (a re-plan just after a ride) may walk off it.
    let platforms: Vec<(i16, i16)> = w.props.arrival_exempt.iter().map(|&(_, p)| p).collect();
    let partners = door_partners(session);
    // Doors that open only for a player standing in their box: from a cell
    // in the box their leaves do not block (the press opens them).
    let boxed = boxed_doors(session);
    // Where a ride through the door at `contact` leaves the player able to
    // walk on: its landing, or - when the landing stands in reach of a
    // platform the record brackets - every cell just out of the platform's
    // reach that the landing's pocket opens onto, with the pocket distance.
    let arrivals = |contact: (i16, i16), land: Cell| -> Vec<(Cell, i32)> {
        let plats: Vec<(i16, i16)> = partners
            .get(&contact)
            .map(|v| v.iter().copied().filter(|&p| in_reach(land, p)).collect())
            .unwrap_or_default();
        if plats.is_empty() {
            if std::env::var_os("LEGAIA_FGL_PLAN_DEBUG").is_some() {
                eprintln!(
                    "      [plan] arrival {contact:?} at {land:?}: partners {:?}",
                    partners.get(&contact)
                );
            }
            return vec![(land, 0)];
        }
        let inside = |c: Cell| plats.iter().any(|&p| in_reach(c, p));
        // A ride's `MoveTo` sets the player on a tile centre, the low corner
        // of its lattice cell. A landing the walls seal there is carried
        // across the platform by the engine's walk-off
        // (`World::sealed_arrival_walk_off`), and the pocket starts where
        // that leaves the player.
        let corner = (land.0 * SUBCELL, land.1 * SUBCELL);
        let start = plats
            .iter()
            .find_map(|&p| w.sealed_arrival_walk_off(corner, p))
            .map_or(land, |(x, z)| cell_of(x, z));
        let mut dist: HashMap<Cell, i32> = HashMap::from([(start, 0)]);
        let mut q = VecDeque::from([start]);
        let mut exits = Vec::new();
        if !inside(start) {
            exits.push((start, 0));
            q.clear();
        }
        while let Some(c) = q.pop_front() {
            let d = dist[&c];
            if dist.len() > 400 {
                break;
            }
            let (ccx, ccz) = cell_center(c);
            for ((dx, dz), dir) in STEPS {
                let n = (c.0 + dx, c.1 + dz);
                if n.0 < 0 || n.1 < 0 || dist.contains_key(&n) || w.field_dir_blocked(ccx, ccz, dir)
                {
                    continue;
                }
                dist.insert(n, d + 1);
                if inside(n) {
                    q.push_back(n);
                } else {
                    exits.push((n, d + 1));
                }
            }
        }
        if std::env::var_os("LEGAIA_FGL_PLAN_DEBUG").is_some() {
            eprintln!(
                "      [plan] arrival {contact:?} at {land:?} platforms {plats:?}: pocket {} exits {exits:?}",
                dist.len()
            );
        }
        if exits.is_empty() {
            vec![(start, 0)]
        } else {
            exits
        }
    };
    // A closed door prop standing over a teleport tile, or beside an object
    // door's contact (the leaf of a two-part door), is solid until its touch
    // opens it; the route counts it open, and the follower's press into it
    // is the touch.
    // A touch-class door whose bind record runs `31 00` opens itself on the
    // touch wherever it stands, over a teleport or not (`jiji` P0[0], the
    // door across the corridor to the `map02` mouth at (66, 96)).
    // An interact-class one (`31 1E` in its spawn prologue) opens the same
    // way on the action button instead of the touch: the follower, held
    // against it, taps Cross (`chitei2` P0[2], the gate at (41, 122) in
    // front of the kind-0 teleport at (41, 124) into the Jette pocket).
    let self_opening = self_opening_doors(session);
    let blockers = Blockers::build(w, |c| {
        !c.moving_box
            && (c.anchor.is_some_and(|a| self_opening.contains(&a))
                || !c.interact
                    && (warps.keys().any(|&(tx, tz)| {
                        let (lx, lz) = (tx * 128 - 64, tz * 128 - 64);
                        (lx..lx + 256).contains(&c.center.0) && (lz..lz + 256).contains(&c.center.1)
                    // A leaf anchored on the tile in front of the teleport
                    // (`dolk`'s inn stair door: anchor (76, 121), box
                    // centre z 121.3 tiles, teleport tile (76, 122)).
                    || c.anchor.is_some_and(|(ax, az)| {
                        (i32::from(ax) - tx).abs() + (i32::from(az) - tz).abs() <= 1
                    })
                    }) || doors.iter().any(|&((x, z), _)| {
                        (c.center.0 - i32::from(x)).abs() <= 128
                            && (c.center.1 - i32::from(z)).abs() <= 128
                    })))
    });
    let mut s = Search {
        parent: HashMap::from([(from, from)]),
        g: HashMap::from([(from, 0)]),
        open: std::collections::BinaryHeap::new(),
        goal: gc,
    };
    s.open.push(std::cmp::Reverse((s.h(from), 0, from)));
    let mut best = from;
    while let Some(std::cmp::Reverse((_, gcur, cur))) = s.open.pop() {
        if s.g.get(&cur).is_some_and(|&v| v < gcur) {
            continue;
        }
        if s.parent.len() > MAX_PLAN_NODES {
            break;
        }
        if s.h(cur) < s.h(best) {
            best = cur;
        }
        if cur == gc {
            break;
        }
        let (cx, cz) = cell_center(cur);
        let here = dispatch_tile(cx, cz);
        let skip: Vec<(i16, i16)> = platforms
            .iter()
            .copied()
            .filter(|&p| in_reach(cur, p))
            .chain(
                boxed
                    .iter()
                    // One tile of slack: the leaf's own tile, which the
                    // player walks through once the press has opened it.
                    .filter(|((x0, z0, x1, z1), _)| {
                        (i32::from(*x0) - 1..=i32::from(*x1) + 1).contains(&here.0)
                            && (i32::from(*z0) - 1..=i32::from(*z1) + 1).contains(&here.1)
                    })
                    .flat_map(|(_, leaves)| leaves.iter().copied()),
            )
            .collect();
        for ((dx, dz), dir) in STEPS {
            let next = (cur.0 + dx, cur.1 + dz);
            if next.0 < 0 || next.1 < 0 {
                continue;
            }
            let (nx, nz) = cell_center(next);
            // Leaning into an object door's contact box posts its touch,
            // whose record teleports the player: the contact fires from the
            // same leading probe points that block the step, so a door set
            // in a wall is reached by pressing into the wall. The step
            // continues from the landing (the door cell is the waypoint the
            // follower presses toward).
            // The player comes to rest anywhere in its cell, not at its
            // centre, so the probes reach up to half a cell further than
            // the centre's (the lattice would otherwise miss a door whose
            // wall stops the player a few units short of the cell edge).
            let (sx, sz) = (dx * SUBCELL / 2, dz * SUBCELL / 2);
            let probes = ACTOR_PROBES[dir].map(|(px, pz)| {
                (
                    i32::from(cx) + i32::from(px) + i32::from(sx),
                    i32::from(cz) - i32::from(pz) + i32::from(sz),
                )
            });
            if let Some(&(contact, land)) =
                doors
                    .iter()
                    .filter(|(c, _)| !skip.contains(c))
                    .find(|((dx, dz), _)| {
                        probes.iter().any(|&(px, pz)| {
                            (px - i32::from(*dx)).abs() < TOUCH_BOX_HALF
                                && (pz - i32::from(*dz)).abs() < TOUCH_BOX_HALF
                        })
                    })
            {
                if land != next && !s.parent.contains_key(&next) {
                    for (to, d) in arrivals(contact, land) {
                        if to != next {
                            s.step(Some(next), cur, to, gcur + 2 + d);
                        }
                    }
                    PRESS_AT.with(|m| m.borrow_mut().insert(next, contact));
                    if std::env::var_os("LEGAIA_FGL_PLAN_DEBUG").is_some() {
                        eprintln!("      [plan] door {contact:?} from {cur:?} -> {land:?}");
                    }
                }
                continue;
            }
            if w.field_dir_blocked(cx, cz, dir) {
                continue;
            }
            let (nt, ct) = (dispatch_tile(nx, nz), dispatch_tile(cx, cz));
            // A teleport tile under a closed door prop reads actor-blocked
            // from every side: the prop is solid until the touch its body
            // posts runs the door's bind record (`31 00`), so pressing into
            // it opens the way. Only a static wall refuses the step.
            // A pad that forks on a clock flag lands in either phase's
            // place: both are edges, the phase the flag is not in now dearer
            // by the wait for it ([`phased_pads`]; the follower times the
            // step, [`phase_ready`]).
            let pad_tile = nt != ct
                && nt != goal_tile
                && (phased.contains_key(&nt) || warps.contains_key(&nt));
            let lands: Vec<(Cell, i32)> = if !pad_tile {
                Vec::new()
            } else if let Some(lands) = phased.get(&nt) {
                lands
                    .iter()
                    .filter(|&&(_, flag, on)| phase_usable(session, flag, on))
                    .map(|&(land, flag, on)| {
                        let wait = if w.system_flag_test(flag) == on {
                            0
                        } else {
                            PHASE_WAIT_COST
                        };
                        (land, wait)
                    })
                    .collect()
            } else if let Some(&land) = warps.get(&nt) {
                vec![(land, 0)]
            } else {
                Vec::new()
            };
            if pad_tile {
                // Entering the teleport tile lands the player elsewhere; the
                // tile itself is a waypoint the follower steps into, never a
                // cell the walk continues from.
                let press = tile_center((nt.0 as i16, nt.1 as i16));
                if !s.parent.contains_key(&next) {
                    let mut any = false;
                    for (land, wait) in lands {
                        if land != next {
                            s.step(Some(next), cur, land, gcur + 2 + wait);
                            any = true;
                        }
                    }
                    if any {
                        PRESS_AT.with(|m| m.borrow_mut().insert(next, press));
                    }
                } else if phased.contains_key(&nt) && s.parent.get(&cur) != Some(&next) {
                    // The tile's cells are already reached - the walker
                    // landed on the tile, as on a pad its partner pad sends
                    // it to (`rugi` P2[40] / P2[41]): crossing into it again
                    // still fires it. The jump then runs from this cell, the
                    // follower pressing toward the tile from here. Clock pads
                    // only: a plain teleport keeps its one waypoint, and a
                    // re-entry from a cell the follower cannot leave cleanly
                    // (`rugi`'s lower pad P2[16] at (92, 102), its south side
                    // walled) stalls a walk the waypoint route makes.
                    let mut any = false;
                    for (land, wait) in lands {
                        if land != next && land != cur {
                            s.step(None, cur, land, gcur + 2 + wait);
                            any = true;
                        }
                    }
                    if any {
                        PRESS_AT.with(|m| {
                            m.borrow_mut().insert(cur, press);
                        });
                    }
                }
                continue;
            }
            if blockers.dir_blocked_except(cx, cz, dir, &skip) {
                continue;
            }
            // The walk stops where the leading probes first touch a body, so
            // a cell whose centre the probes from the cell before clear may
            // still be one the player never reaches: the probes 64 units
            // ahead of the centre itself are already inside the body. The
            // player halts a few units short, and the next leg starts from
            // there, not from the centre the lattice assumed. `korb3`'s
            // switch P0[18] (box centre (4800, 4720)) stops a walk north at
            // Z 4862, while the cell (37, 75) it would turn east from is
            // centred on Z 4832. Only the goal cell is entered regardless:
            // a walk up to a body to face it ends pressed against it.
            if next != gc && blockers.dir_blocked_except(nx, nz, dir, &skip) {
                continue;
            }
            if nt != ct && avoid.contains(&nt) {
                continue;
            }
            s.step(None, cur, next, gcur + 1);
        }
    }
    PLAN_STATS.with(|p| {
        let (n, cells) = p.get();
        p.set((n + 1, cells + s.parent.len() as u64));
    });
    let reached = best == gc;
    let path = if best == from {
        Vec::new()
    } else {
        walk_back(&s.parent, from, best)?
    };
    Some((path, s.parent.into_keys().collect(), reached))
}

/// The route from `from` to `to` along `parent` links. A chain longer than
/// the map holds is a cycle: `None`, never an unbounded walk.
pub(super) fn walk_back(parent: &HashMap<Cell, Cell>, from: Cell, to: Cell) -> Option<Vec<Cell>> {
    let mut path = vec![to];
    let mut c = to;
    while c != from {
        c = *parent.get(&c)?;
        if c != from {
            path.push(c);
        }
        if path.len() > parent.len() {
            return None;
        }
    }
    path.reverse();
    Some(path)
}

/// Every door tile in the scene that does NOT lead to `dest` - stepping on one
/// would leave for the wrong scene.
pub(super) fn hazards(session: &NativeSession, dest: &str) -> HashSet<(i32, i32)> {
    let w = &session.host.world;
    let mut out = HashSet::new();
    if w.mode == SceneMode::WorldMap {
        for (cfg, &(x, z)) in w
            .world_map
            .entity_configs
            .iter()
            .zip(w.world_map.entity_positions.iter())
        {
            if let WorldMapEntityConfig::OverworldPortal { scene_name, .. } = cfg
                && scene_name != dest
            {
                out.insert((i32::from(x) >> 7, i32::from(z) >> 7));
            }
        }
        return out;
    }
    let name = scene_name(session);
    let index = &session.host.index;
    if let Ok(scene) = Scene::load(index, &name)
        && let Ok(Some(man)) = scene.field_man_payload(index)
        && let Ok(mf) = legaia_scene_formats::man_section::parse(&man)
        && let Ok((p, f)) = scene.field_tile_triggers(index)
    {
        let triggers: Vec<TileTrigger> = p.into_iter().chain(f).collect();
        // A band whose record's spawn gates fail under the live flags is
        // floor: `kor3`'s P2[14] at (113, 5..6), the Cara and Grantes beat
        // into `koin1b`, spawns only while `0x426` is clear, and once it is
        // set the corridor to the door to `kor` runs over that band.
        let live = |rec: u8| {
            legaia_engine_field::man_field_scripts::partition2_record_gates(
                &mf,
                &man,
                usize::from(rec),
            )
            .is_none_or(|(c1, c2)| w.p2_record_gates_pass(&c1, &c2))
        };
        for s in overworld_portal_sites(&mf, &man, &triggers) {
            if s.scene_name != dest && live(s.record) {
                out.insert((i32::from(s.overworld_x), i32::from(s.overworld_z)));
            }
        }
        // A band that turned a split party's walker back is a wall to it.
        let turned: HashSet<u8> = TURNBACK.with(|t| {
            t.borrow()
                .iter()
                .filter(|(sc, _)| *sc == name)
                .map(|&(_, r)| r)
                .collect()
        });
        if w.dialog.three_actor_talk.is_some() {
            out.extend(
                triggers
                    .iter()
                    .filter(|t| t.gate == 1 && turned.contains(&t.record))
                    .map(|t| (i32::from(t.tile_x), i32::from(t.tile_z))),
            );
        }
    }
    out
}

thread_local! {
    /// `(scene, partition-2 record)` bands whose record ran to its release
    /// on a walk to somewhere else and left a three-actor split armed. The
    /// beat pass does not walk them again: `nilboa`'s split cutscene P2[0]
    /// runs from the entrance band P2[29] (`44 4E`), and walking its own
    /// band afterwards sent every member of the split across the map toward
    /// a beat already played - the Fire Ravine member re-dressing the
    /// boulder room (P2[26]) across the other member's way.
    pub(super) static RAN_ON_WALK: std::cell::RefCell<HashSet<(String, u8)>> =
        std::cell::RefCell::new(HashSet::new());
    /// `(scene, partition-2 record)` bands that fired on a split party's
    /// walk and changed nothing - `nilboa`'s tunnel mouths, whose "go back
    /// outside?" No steps the walker back. While the split holds, the plan
    /// treats them as walls: the members' tunnels join only outside, and a
    /// route through a mouth loops on its refusal.
    pub(super) static TURNBACK: std::cell::RefCell<HashSet<(String, u8)>> =
        std::cell::RefCell::new(HashSet::new());
}
