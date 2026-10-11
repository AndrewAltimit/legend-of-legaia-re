//! Tile and pad geometry, and the path planner over the walkability grid.

use super::*;

// ---------------------------------------------------------------------------
// Tile / pad geometry
// ---------------------------------------------------------------------------

/// A walkability sub-cell: world coordinates shifted down by
/// [`SUBCELL`], the granularity [`World::field_tile_is_wall`] indexes at.
pub(super) type Cell = (i16, i16);

/// World centre of a tile. Mirrors `seat_player_at_tile_inner`'s
/// `(b & 0x7F) * 0x80 + 0x40` for the near half.
pub(super) fn tile_center(t: (i16, i16)) -> (i16, i16) {
    (t.0 * TILE + 0x40, t.1 * TILE + 0x40)
}

/// Tile containing a world point - the inverse the spine oracle uses to
/// check the op-`0x3F` arrival seat.
pub(super) fn tile_of(x: i16, z: i16) -> (i16, i16) {
    ((x - 0x40) >> 7, (z - 0x40) >> 7)
}

/// Nearest lattice node to a world point.
///
/// The lattice sits **on** the 64-unit grid lines, not in the middle of the
/// cells between them, and that alignment is load-bearing: a tile centre is
/// `128t + 64 = 64(2t + 1)`, an exact multiple of `SUBCELL`, so a lattice of
/// node centres `64c` contains every tile centre while a lattice of `64c + 32`
/// contains none of them. Off-centre nodes cannot represent standing on a
/// doorway's centreline, and [`World::field_dir_blocked`] probes three
/// *laterally spread* leading-edge points - so from 32 units off-centre the
/// side probes catch the gate posts and a one-tile-wide opening reads as
/// sealed. Rim Elm's south gate did exactly that: the flood stopped two cells
/// short of it.
pub(super) fn cell_of(x: i16, z: i16) -> Cell {
    ((x + SUBCELL / 2) / SUBCELL, (z + SUBCELL / 2) / SUBCELL)
}

/// World position of a lattice node.
pub(super) fn cell_center(c: Cell) -> (i16, i16) {
    (c.0 * SUBCELL, c.1 * SUBCELL)
}

/// Tile a sub-cell belongs to, in the same frame as [`tile_of`].
pub(super) fn tile_of_cell(c: Cell) -> (i16, i16) {
    let (x, z) = cell_center(c);
    tile_of(x, z)
}

/// Neighbour offsets paired with the `dir` row
/// [`World::field_dir_blocked`] probes: `0` = Z-, `1` = X-, `2` = Z+,
/// `3` = X+.
pub(super) const STEPS: [((i16, i16), usize); 4] =
    [((0, -1), 0), ((-1, 0), 1), ((0, 1), 2), ((1, 0), 3)];

/// Pad mask that walks the player one step along world direction
/// `(dwx, dwz)`.
///
/// `decode_field_direction` rotates the *screen* delta into world space by
/// the camera quadrant `((azimuth + 512) / 1024) & 3`; this inverts that
/// rotation so the caller can think in world axes. Quadrant 0 is identity
/// (screen-up -> Z+, screen-right -> X+).
pub(super) fn pad_for_world_step(azimuth: u16, dwx: i16, dwz: i16) -> u16 {
    let quadrant = ((azimuth as u32).wrapping_add(512) / 1024) & 3;
    let (sx, sy) = match quadrant {
        0 => (dwx, dwz),
        1 => (-dwz, dwx),
        2 => (-dwx, -dwz),
        _ => (dwz, -dwx),
    };
    let mut pad = 0u16;
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

/// The **overworld** pad inversion, which is a different space.
///
/// The two walks do not share a remap:
/// [`World::step_field_locomotion`] routes the pad through
/// `decode_field_direction` (world axes, quadrant rotation), while
/// `World::step_world_map_locomotion` routes it through
/// `world_map_camera_relative_bits` - screen-up -> world `(-sinθ, cosθ)`,
/// screen-right -> world `(cosθ, sinθ)` (the retail identity at azimuth 0).
/// Inverting the field remap on the overworld sends the player off at an
/// angle, and map01's collision grid leaves the sea open, so there is
/// nothing to stop them: the leg walks off the map instead of stalling
/// against a wall.
///
/// That 2x2 is a rotation, so its inverse is the transpose - the screen
/// delta for a desired world step applies the reverse rotation. The `T`
/// band matches the forward map's 8-direction quantisation.
pub(super) fn world_map_pad_for_world_step(azimuth: i32, dwx: i16, dwz: i16) -> u16 {
    let (dx, dz) = (f32::from(dwx), f32::from(dwz));
    let len = (dx * dx + dz * dz).sqrt();
    if len == 0.0 {
        return 0;
    }
    let theta = (azimuth as f32) / 4096.0 * std::f32::consts::TAU;
    let (sin, cos) = theta.sin_cos();
    let (dx, dz) = (dx / len, dz / len);
    let sx = dx * cos + dz * sin;
    let sy = -dx * sin + dz * cos;
    /// sin(22.5°) - the same cardinal band `world_map_camera_relative_bits` uses.
    const T: f32 = 0.382_683_43;
    let mut pad = 0u16;
    if sy > T {
        pad |= PadButton::Up.mask();
    } else if sy < -T {
        pad |= PadButton::Down.mask();
    }
    if sx > T {
        pad |= PadButton::Right.mask();
    } else if sx < -T {
        pad |= PadButton::Left.mask();
    }
    pad
}

/// Pick the pad for a desired world step in whichever space the world is
/// currently walking in.
pub(super) fn pad_for_step(host: &SceneHost, dwx: i16, dwz: i16) -> u16 {
    match host.world.mode {
        SceneMode::WorldMap => world_map_pad_for_world_step(
            host.world.world_map.ctrl.as_ref().map_or(0, |c| c.azimuth),
            dwx,
            dwz,
        ),
        _ => pad_for_world_step(host.world.locomotion.camera_azimuth, dwx, dwz),
    }
}

// ---------------------------------------------------------------------------
// Planner
// ---------------------------------------------------------------------------

/// BFS the reachable set from `from`, and return the route to whichever
/// reachable node sits closest to `goal_tile`'s centre.
///
/// Best-effort by design, because the exact goal is routinely **not** a
/// standable node: `seat_player_at_tile`'s own docs record that "a door tile
/// reads as a wall", and a scene exit is a door. Rim Elm's south gate is the
/// worked example - the gate cell `(3264, 5952)` is open only from the south,
/// and from inside the town `Z+` blocks 96 units short of it. A planner that
/// insisted on standing *on* the exit would report `NoPath` on a gate the
/// player walks through every playthrough.
///
/// What actually opens a scene exit is the walk-on trigger band on the
/// approach, so the follower's job is to get as close as the walls allow and
/// keep pressing; the leg succeeds on [`Leg::Transitioned`], not on arrival.
///
/// Edges are certified by [`World::field_dir_blocked`] - retail
/// `FUN_801cfe4c`'s static-wall arm, the same probe the locomotion step runs -
/// evaluated at the source node, with the lattice pitch held under the probe
/// reach (see [`SUBCELL`]).
///
/// `avoid` names **dispatch tiles** (`world >> 7`) the route must not enter.
/// On the overworld those are the other scenes' portals: standing on one is
/// how you enter it (`World::auto_engage_world_map_portals` matches the
/// player's `>> 7` tile against the entity's), so a route that crosses one
/// never arrives - it ends the leg in the wrong scene. Retail's own player has
/// the same constraint and routes around it; the planner has to as well.
///
/// `None` only when nothing at all is reachable from `from`.
pub(super) fn plan_path(
    host: &SceneHost,
    from: Cell,
    goal_tile: (i16, i16),
    avoid: &HashSet<(i32, i32)>,
) -> Option<Vec<Cell>> {
    let goal_w = tile_center(goal_tile);
    let goal_c = cell_of(goal_w.0, goal_w.1);
    let score = |c: Cell| (c.0 - goal_c.0).abs() + (c.1 - goal_c.1).abs();

    let mut seen: HashMap<Cell, Cell> = HashMap::new();
    let mut queue = VecDeque::new();
    seen.insert(from, from);
    queue.push_back(from);
    let mut best = from;

    while let Some(cur) = queue.pop_front() {
        if seen.len() > MAX_PLAN_NODES {
            break;
        }
        if score(cur) < score(best) {
            best = cur;
        }
        if cur == goal_c {
            break;
        }
        let (cx, cz) = cell_center(cur);
        for ((dx, dz), dir) in STEPS {
            let next = (cur.0 + dx, cur.1 + dz);
            if next.0 < 0 || next.1 < 0 || seen.contains_key(&next) {
                continue;
            }
            if host.world.field_dir_blocked(cx, cz, dir)
                || host.world.field_actor_dir_blocked(cx, cz, dir)
            {
                continue;
            }
            if !avoid.is_empty() {
                let (nx, nz) = cell_center(next);
                let (nt, ct) = (dispatch_tile(nx, nz), dispatch_tile(cx, cz));
                // A hazard tile is dangerous to **enter**, and a step that
                // stays inside one tile is not an entry: retail's dispatcher
                // fires on a tile *change*
                // (`SceneHost::dispatch_walk_on_trigger` compares against the
                // previous frame's tile), so the band under the player's own
                // feet has already fired and cannot fire again while it is
                // occupied. Testing the destination alone conflates "occupy"
                // with "enter", and that is a **self-block** whenever the walk
                // starts inside an avoided tile - which a dungeon arrival
                // always does, because the player is seated on the door they
                // came in through. See [`ExitRecord`].
                if nt != ct && avoid.contains(&nt) {
                    continue;
                }
            }
            seen.insert(next, cur);
            queue.push_back(next);
        }
    }
    if best == from {
        return None;
    }
    let mut path = vec![best];
    let mut step = best;
    while step != from {
        step = seen[&step];
        if step != from {
            path.push(step);
        }
    }
    path.reverse();
    Some(path)
}
