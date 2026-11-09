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

/// The hasher behind the planner's per-cell tables ([`FastMap`] /
/// [`FastSet`]): a multiply-rotate word hash (the Fx scheme). The A* probes
/// those tables several times per expanded cell, and with the default
/// SipHash the hashing outweighed the rest of the search. The tables are
/// only looked up and counted, never iterated in an order a decision
/// reads, so the hasher changes no route.
#[derive(Default, Clone, Copy)]
pub(super) struct FastHasher(u64);

impl FastHasher {
    #[inline]
    fn add(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

impl std::hash::Hasher for FastHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.add(u64::from(b));
        }
    }
    #[inline]
    fn write_u8(&mut self, v: u8) {
        self.add(u64::from(v));
    }
    #[inline]
    fn write_u16(&mut self, v: u16) {
        self.add(u64::from(v));
    }
    #[inline]
    fn write_u32(&mut self, v: u32) {
        self.add(u64::from(v));
    }
    #[inline]
    fn write_u64(&mut self, v: u64) {
        self.add(v);
    }
    #[inline]
    fn write_usize(&mut self, v: usize) {
        self.add(v as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

pub(super) type FastMap<K, V> = HashMap<K, V, std::hash::BuildHasherDefault<FastHasher>>;
pub(super) type FastSet<K> = HashSet<K, std::hash::BuildHasherDefault<FastHasher>>;

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

/// A body box: centre `(x, z)` and half-extent.
pub(super) type BodyBox = (i32, i32, i32);

/// The actor-collision boxes one plan tests against, bucketed by the
/// 128-unit tiles each overlaps: every field NPC at its live position
/// (±40) and every solid placed prop (±80 static, ±40 moving). The
/// planner's copy of `World::field_actor_dir_blocked`, built once per plan
/// so a whole-map search does not re-walk every body per probe. The
/// buckets are a dense tile grid over the boxes' bounding rectangle, so a
/// probe is an index, not a hash lookup.
pub(super) struct Blockers {
    /// Tile `(x0, z0)`, the grid's low corner.
    origin: (i32, i32),
    /// Tiles per grid row.
    width: i32,
    /// Grid rows.
    height: i32,
    /// Tile `i` (row-major) holds `boxes[starts[i]..starts[i + 1]]`.
    starts: Vec<u32>,
    /// The boxes overlapping each tile, tile by tile, in build order.
    boxes: Vec<BodyBox>,
    /// Row-major over the grid grown by one tile on every side: whether the
    /// tile or a neighbour holds a box. A lattice cell's probes reach under
    /// a tile from its centre, so a clear tile answers all of them at once.
    near: Vec<bool>,
}

impl Blockers {
    pub(super) fn build(
        w: &legaia_engine_core::world::World,
        goal: Option<(i16, i16)>,
        pass: impl Fn(&legaia_engine_core::world::FieldPropCollider) -> bool,
    ) -> Self {
        const NPC_HALF: i32 = 0x40 - 0x18;
        let npcs_off = NPC_BODIES_OFF.with(std::cell::Cell::get);
        let near = NPC_BODIES_NEAR.with(std::cell::Cell::get);
        // The bodies the walk collides with, not the talk map: a placement
        // the port tracks only by its script channel stops the player too,
        // and one hidden or culled does not.
        let mut boxes: Vec<(i32, i32, i32)> = w
            .field_moving_actor_centres()
            .filter(|_| !npcs_off)
            .filter(|&(x, z)| {
                near.is_none_or(|(nx, nz)| {
                    (x - nx).abs() <= NPC_NEAR_REACH && (z - nz).abs() <= NPC_NEAR_REACH
                })
            })
            // A body on the goal tile is what the walk is going to, not
            // something in its way: a talk's first walk aims at the NPC's own
            // tile. Left in, it makes the goal unreachable and sends the
            // route through every other body on the bodies-off search below.
            .filter(|&(x, z)| {
                goal.is_none_or(|(gx, gz)| (x >> 7, z >> 7) != (i32::from(gx), i32::from(gz)))
            })
            .map(|(x, z)| (x, z, NPC_HALF))
            .collect();
        // The centres come out of a `HashMap`; the plan memo hashes the boxes.
        boxes.sort_unstable();
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
        // A box's tile span: every tile its +-half square overlaps.
        let span = |b: &BodyBox| {
            let (x, z, h) = *b;
            ((x - h) >> 7, (z - h) >> 7, (x + h) >> 7, (z + h) >> 7)
        };
        let Some((x0, z0, x1, z1)) = boxes
            .iter()
            .map(span)
            .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))
        else {
            return Self {
                origin: (0, 0),
                width: 0,
                height: 0,
                starts: vec![0],
                boxes: Vec::new(),
                near: vec![false; 4],
            };
        };
        let (width, height) = (x1 - x0 + 1, z1 - z0 + 1);
        let tiles = |b: &BodyBox| {
            let (bx0, bz0, bx1, bz1) = span(b);
            (bz0..=bz1).flat_map(move |tz| {
                (bx0..=bx1).map(move |tx| ((tz - z0) * width + (tx - x0)) as usize)
            })
        };
        // Count per tile, prefix-sum the counts into starts, then place.
        let mut starts = vec![0u32; (width * height) as usize + 1];
        for b in &boxes {
            for i in tiles(b) {
                starts[i + 1] += 1;
            }
        }
        let mut run = 0;
        for s in &mut starts {
            run += *s;
            *s = run;
        }
        let (near_w, near_h) = (width as usize + 2, height as usize + 2);
        let mut near = vec![false; near_w * near_h];
        for z in 0..height as usize {
            for x in 0..width as usize {
                let i = z * width as usize + x;
                if starts[i] != starts[i + 1] {
                    for row in near.chunks_exact_mut(near_w).skip(z).take(3) {
                        row[x..x + 3].fill(true);
                    }
                }
            }
        }
        let mut fill = starts.clone();
        let mut placed = vec![(0, 0, 0); run as usize];
        for b in &boxes {
            for i in tiles(b) {
                placed[fill[i] as usize] = *b;
                fill[i] += 1;
            }
        }
        Self {
            origin: (x0, z0),
            width,
            height,
            starts,
            boxes: placed,
            near,
        }
    }

    /// Feeds the boxes into a [`PlanMemo`] key, in an order the live NPC
    /// table's iteration cannot change.
    fn hash_boxes(&self, h: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        let mut boxes = self.boxes.clone();
        boxes.sort_unstable();
        boxes.hash(h);
    }

    /// Whether tile `(tx, tz)` or a neighbour holds a box.
    #[inline]
    fn near_tile(&self, tx: i32, tz: i32) -> bool {
        let (x, z) = (tx - self.origin.0 + 1, tz - self.origin.1 + 1);
        if !(0..self.width + 2).contains(&x) || !(0..self.height + 2).contains(&z) {
            return false;
        }
        self.near[(z * (self.width + 2) + x) as usize]
    }

    /// The boxes overlapping tile `(tx, tz)`.
    #[inline]
    fn at(&self, tx: i32, tz: i32) -> &[BodyBox] {
        let (x, z) = (tx - self.origin.0, tz - self.origin.1);
        if !(0..self.width).contains(&x) || !(0..self.height).contains(&z) {
            return &[];
        }
        let i = (z * self.width + x) as usize;
        &self.boxes[self.starts[i] as usize..self.starts[i + 1] as usize]
    }

    /// Whether any box overlaps tile `(tx, tz)`.
    pub(super) fn covers_tile(&self, tx: i32, tz: i32) -> bool {
        !self.at(tx, tz).is_empty()
    }

    /// Whether a body box covers `(px, pz)`, ignoring the static boxes
    /// centred on `skip` (the platforms under the arrival bracket).
    #[inline]
    pub(super) fn hit_except(&self, px: i32, pz: i32, skip: &[(i16, i16)]) -> bool {
        self.at(px >> 7, pz >> 7).iter().any(|&(x, z, h)| {
            (px - x).abs() < h
                && (pz - z).abs() < h
                && !(h == TOUCH_BOX_HALF
                    && skip
                        .iter()
                        .any(|&(sx, sz)| (i32::from(sx), i32::from(sz)) == (x, z)))
        })
    }

    pub(super) fn dir_blocked_except(
        &self,
        cx: i16,
        cz: i16,
        dir: usize,
        skip: &[(i16, i16)],
    ) -> bool {
        // Every probe offset is under a tile, so its point lies in this tile
        // or a neighbour.
        if !self.near_tile(i32::from(cx) >> 7, i32::from(cz) >> 7) {
            return false;
        }
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

/// Side of the square of lattice cells [`Nodes`] holds densely: every cell
/// an `i16` world position falls in (`cell_of` of `0..=i16::MAX`). A cell
/// outside it (a step past the far edge) goes to the overflow map.
const NODE_SIDE: usize = 1025;

/// The search's per-cell record - parent link and path cost - for every
/// cell it has reached. A dense table over the lattice, stamped per search
/// so a new search starts empty without clearing it, and kept per thread
/// between searches: a hash map here cost more than the rest of the A*.
pub(super) struct Nodes {
    /// One record per cell: a slot's fields share a cache line, which is
    /// most of a probe's cost in a table this size.
    slots: Vec<NodeSlot>,
    now: u32,
    /// The cells reached, in first-reached order.
    reached: Vec<Cell>,
    /// Records for cells outside the dense square.
    overflow: FastMap<Cell, (Cell, i32)>,
    /// The frontier, kept here for its storage.
    open: OpenSet,
}

#[derive(Clone, Copy)]
struct NodeSlot {
    /// The search this record belongs to; a slot with an older stamp is
    /// empty.
    stamp: u32,
    parent: Cell,
    g: i32,
}

thread_local! {
    /// The node table between searches ([`Nodes::take`] / [`Nodes::put`]).
    static NODES: std::cell::RefCell<Option<Box<Nodes>>> = const { std::cell::RefCell::new(None) };
}

impl Nodes {
    /// An empty table: the thread's kept one when it is free, else a new
    /// one.
    pub(super) fn take() -> Box<Nodes> {
        let mut n = NODES.with(|t| t.borrow_mut().take()).unwrap_or_else(|| {
            Box::new(Nodes {
                slots: vec![
                    NodeSlot {
                        stamp: 0,
                        parent: (0, 0),
                        g: 0,
                    };
                    NODE_SIDE * NODE_SIDE
                ],
                now: 0,
                reached: Vec::new(),
                overflow: FastMap::default(),
                open: OpenSet::default(),
            })
        });
        n.now = n.now.wrapping_add(1);
        if n.now == 0 {
            n.slots.iter_mut().for_each(|s| s.stamp = 0);
            n.now = 1;
        }
        n.reached.clear();
        n.overflow.clear();
        n.open.clear();
        n
    }

    /// Hand the table back for the thread's next search.
    pub(super) fn put(n: Box<Nodes>) {
        NODES.with(|t| *t.borrow_mut() = Some(n));
    }

    #[inline]
    fn slot(c: Cell) -> Option<usize> {
        let (x, z) = (usize::try_from(c.0).ok()?, usize::try_from(c.1).ok()?);
        (x < NODE_SIDE && z < NODE_SIDE).then_some(z * NODE_SIDE + x)
    }

    /// `c`'s record, `(parent, g)`.
    #[inline]
    pub(super) fn get(&self, c: Cell) -> Option<(Cell, i32)> {
        match Self::slot(c) {
            Some(i) => {
                let s = &self.slots[i];
                (s.stamp == self.now).then_some((s.parent, s.g))
            }
            None => self.overflow.get(&c).copied(),
        }
    }

    #[inline]
    pub(super) fn parent(&self, c: Cell) -> Option<Cell> {
        self.get(c).map(|(p, _)| p)
    }

    #[inline]
    pub(super) fn g(&self, c: Cell) -> Option<i32> {
        self.get(c).map(|(_, g)| g)
    }

    #[inline]
    pub(super) fn contains(&self, c: Cell) -> bool {
        self.get(c).is_some()
    }

    /// Cells reached.
    pub(super) fn len(&self) -> usize {
        self.reached.len()
    }

    /// The cells reached, in first-reached order.
    pub(super) fn cells(&self) -> &[Cell] {
        &self.reached
    }

    #[inline]
    fn set(&mut self, c: Cell, parent: Cell, g: i32) {
        match Self::slot(c) {
            Some(i) => {
                let s = &mut self.slots[i];
                if s.stamp != self.now {
                    s.stamp = self.now;
                    self.reached.push(c);
                }
                s.parent = parent;
                s.g = g;
            }
            None => {
                if self.overflow.insert(c, (parent, g)).is_none() {
                    self.reached.push(c);
                }
            }
        }
    }
}

/// The search frontier: entries pop in exactly `(f, g, x, z)` tuple order.
/// One small heap per `f` value, holding `(g, x, z)` packed into one integer:
/// most of a pop in a single heap over the whole frontier is comparing and
/// moving entries with some other `f`.
#[derive(Default)]
struct OpenSet {
    by_f: Vec<std::collections::BinaryHeap<std::cmp::Reverse<u64>>>,
    /// No entry has an `f` below this.
    low: usize,
}

impl OpenSet {
    /// Empty, keeping the heaps' storage for the next search.
    fn clear(&mut self) {
        for heap in self.by_f.iter_mut().skip(self.low) {
            heap.clear();
        }
        self.low = self.by_f.len();
    }

    #[inline]
    fn push(&mut self, f: i32, g: i32, c: Cell) {
        let f = usize::try_from(f).expect("a path cost and a distance are never negative");
        if f >= self.by_f.len() {
            self.by_f.resize_with(f + 1, Default::default);
        }
        let g = u32::try_from(g).expect("a path cost is never negative");
        let cell = |v: i16| u64::from(v as u16 ^ 0x8000);
        let key = u64::from(g) << 32 | cell(c.0) << 16 | cell(c.1);
        self.by_f[f].push(std::cmp::Reverse(key));
        self.low = self.low.min(f);
    }

    /// The least entry's `(g, cell)`.
    #[inline]
    fn pop(&mut self) -> Option<(i32, Cell)> {
        while let Some(heap) = self.by_f.get_mut(self.low) {
            if let Some(std::cmp::Reverse(key)) = heap.pop() {
                let cell = |shift: u32| ((key >> shift) as u16 ^ 0x8000) as i16;
                return Some(((key >> 32) as i32, (cell(16), cell(0))));
            }
            self.low += 1;
        }
        None
    }
}

/// One step of [`plan_path`]'s search: reach `to` from `cur` at `cost`,
/// through the waypoint `via` when the step is a teleport.
pub(super) struct Search {
    pub(super) nodes: Box<Nodes>,
    pub(super) goal: Cell,
}

impl Search {
    /// A search from `from` toward `goal`, with `from` reached at cost 0
    /// (its own parent).
    pub(super) fn new(from: Cell, goal: Cell) -> Self {
        let mut nodes = Nodes::take();
        nodes.set(from, from, 0);
        Self { nodes, goal }
    }

    /// End the search, handing its table back for the next one.
    pub(super) fn finish(self) {
        Nodes::put(self.nodes);
    }

    #[inline]
    pub(super) fn h(&self, c: Cell) -> i32 {
        i32::from((c.0 - self.goal.0).abs() + (c.1 - self.goal.1).abs())
    }

    pub(super) fn step(&mut self, via: Option<Cell>, cur: Cell, to: Cell, cost: i32) {
        if self.nodes.g(to).is_some_and(|old| old <= cost) {
            return;
        }
        let parent = match via {
            Some(v) => {
                // The waypoint's cost is recorded too: every parent edge
                // must run from a cheaper cell to a dearer one, or a later
                // plain step into `v` (from a cell descended from `to`)
                // overwrites `parent[v]` and closes a cycle that the path
                // walk-back follows forever.
                self.nodes.set(v, cur, cost - 1);
                v
            }
            None => cur,
        };
        self.nodes.set(to, parent, cost);
        let f = cost + self.h(to);
        self.nodes.open.push(f, cost, to);
    }
}

thread_local! {
    /// Plans run and cells they expanded, for the trace's cost line.
    pub(super) static PLAN_STATS: std::cell::Cell<(u64, u64)> = const { std::cell::Cell::new((0, 0)) };
}

/// How far a step's door probes reach from the cell centre on either axis:
/// the largest [`ACTOR_PROBES`] offset plus the half-cell lean.
const DOOR_PROBE_REACH: i32 = 64 + SUBCELL as i32 / 2;

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
        // First with only the bodies around the walker: what seals the goal
        // is usually far off, and a route that also ignores the villager a
        // step away walks into them (`vell`'s square, a route to the map exit
        // straight through one standing in open ground).
        let (fx, fz) = cell_center(from);
        let local = NPC_BODIES_NEAR.with(|f| {
            f.set(Some((i32::from(fx), i32::from(fz))));
            let r = plan_search(session, from, goal, None, avoid);
            f.set(None);
            r
        });
        if let Some((p, _, true)) = local {
            return Some(p);
        }
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
        let bl = Blockers::build(&session.host.world, Some(goal), |_| false);
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
                    } else if bl.covers_tile(tx, tz) {
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
            u.insert(key, (seen.into_iter().collect(), best));
        });
    }
    (!path.is_empty()).then_some(path)
}

/// Searches smaller than this are cheap enough to repeat.
pub(super) const UNREACHED_MIN_CELLS: usize = 4_000;

/// How far from the walker a body still counts on the second search of a
/// plan the bodies kept from its goal: eight tiles.
const NPC_NEAR_REACH: i32 = 8 * 128;

/// What a failed plan's answer depends on: the scene, the goal, the avoid
/// set and the story flags (which open and shut doors and paint walls).
pub(super) type PlanKey = (String, (i16, i16), u64, u64);

thread_local! {
    /// Whether [`Blockers::build`] leaves the field NPCs' bodies out: the
    /// second search of a plan the bodies alone kept from its goal
    /// ([`plan_path`]).
    pub(super) static NPC_BODIES_OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// When set, [`Blockers::build`] keeps only the NPC bodies within
    /// [`NPC_NEAR_REACH`] of this world point.
    pub(super) static NPC_BODIES_NEAR: std::cell::Cell<Option<(i32, i32)>> =
        const { std::cell::Cell::new(None) };
    /// Failed plans: the cells the search reached and the one nearest the
    /// goal, by [`PlanKey`].
    pub(super) static UNREACHED: std::cell::RefCell<HashMap<PlanKey, (FastSet<Cell>, Cell)>> =
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

/// What one search left behind: its answer and the press waypoints it
/// recorded, replayed whole when the same search comes up again.
struct MemoPlan {
    answer: Option<PlanAnswer>,
    presses: Vec<(Cell, (i16, i16))>,
    cells: u64,
}

/// A route, the cells the search reached, and whether it reached its aim.
type PlanAnswer = (Vec<Cell>, Vec<Cell>, bool);

/// Searches kept for reuse, by a hash of everything a search reads. A walk
/// that keeps asking from where it stands, in a world that has not moved, is
/// most of the planner's calls.
#[derive(Default)]
struct PlanMemo {
    plans: HashMap<u64, std::rc::Rc<MemoPlan>>,
    cells: usize,
}

/// The cells [`PlanMemo`] may hold before it starts over.
const PLAN_MEMO_CELLS: usize = 4_000_000;

/// Lattice cells a side the wall table holds: the whole `0x4000`-unit grid.
const WALL_TABLE_SIDE: usize = 512;

/// [`World::field_dir_blocked`] per lattice cell and direction, for one wall
/// grid: a search asks it twelve probes a cell, and its answer is a function of
/// the grid alone. A byte holds four answers and which of them are known.
#[derive(Default)]
struct WallTable {
    grid: u64,
    cells: Vec<u8>,
}

impl WallTable {
    fn for_grid(mut self, grid: u64) -> Self {
        if self.grid != grid || self.cells.is_empty() {
            self.grid = grid;
            self.cells.clear();
            self.cells.resize(WALL_TABLE_SIDE * WALL_TABLE_SIDE, 0);
        }
        self
    }

    fn blocked(&mut self, w: &legaia_engine_core::world::World, cell: Cell, dir: usize) -> bool {
        let (cx, cz) = cell_center(cell);
        let (x, z) = (cell.0 as usize, cell.1 as usize);
        if x >= WALL_TABLE_SIDE || z >= WALL_TABLE_SIDE {
            return w.field_dir_blocked(cx, cz, dir);
        }
        let byte = &mut self.cells[z * WALL_TABLE_SIDE + x];
        let bit = 1u8 << dir;
        if *byte & (bit << 4) == 0 {
            *byte |= bit << 4;
            if w.field_dir_blocked(cx, cz, dir) {
                *byte |= bit;
            }
        }
        *byte & bit != 0
    }
}

/// Forget the remembered unreachable components at a new hop. [`UNREACHED`] is
/// keyed on the scene, flags and avoid set, not on the NPC bodies a search
/// stood against, so a failure remembered on one walk can steer a later
/// one: `tunnelb`'s crossing planned straight to a corner another run's
/// bodies had left unreached and stalled on a wall.
pub(super) fn forget_unreached() {
    UNREACHED.with(|u| u.borrow_mut().clear());
}

thread_local! {
    static PLAN_MEMO: std::cell::RefCell<PlanMemo> = std::cell::RefCell::new(PlanMemo::default());
    static WALL_TABLE: std::cell::Cell<Option<WallTable>> = const { std::cell::Cell::new(None) };
    /// [`overworld_portal_sites`] per scene: a function of the scene's script
    /// and triggers, asked afresh at every walk.
    static PORTAL_SITES: std::cell::RefCell<
        HashMap<String, std::rc::Rc<Vec<legaia_engine_field::man_field_scripts::OverworldPortalSite>>>,
    > = std::cell::RefCell::new(HashMap::new());
}

/// The A* behind [`plan_path`]: the route toward `goal`'s tile - or toward
/// `target` when one is given, a cell already known reachable - the cells
/// the search reached, and whether it reached its aim.
pub(super) fn plan_search(
    session: &NativeSession,
    from: Cell,
    goal: (i16, i16),
    target: Option<Cell>,
    avoid: &HashSet<(i32, i32)>,
) -> Option<PlanAnswer> {
    let w = &session.host.world;
    let plan_debug = std::env::var_os("LEGAIA_FGL_PLAN_DEBUG").is_some();
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
    // The per-step membership tests, against fast-hashed copies (the
    // tables themselves stay keyed the callers' way).
    let pad_tiles: FastSet<(i32, i32)> = phased.keys().chain(warps.keys()).copied().collect();
    let avoid_tiles: FastSet<(i32, i32)> = avoid.iter().copied().collect();
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
            if plan_debug {
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
        let mut dist: FastMap<Cell, i32> = FastMap::from_iter([(start, 0)]);
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
        if plan_debug {
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
    let blockers = Blockers::build(w, Some(goal), |c| {
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
    // Everything below reads the world only through what has been gathered
    // above and the wall grid, so these inputs name the search's result. A
    // debug run prints as it searches, so it always searches.
    let grid_key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        w.terrain.collision_grid.hash(&mut h);
        h.finish()
    };
    let memo_key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (from, goal, gc).hash(&mut h);
        grid_key.hash(&mut h);
        blockers.hash_boxes(&mut h);
        doors.hash(&mut h);
        platforms.hash(&mut h);
        boxed.hash(&mut h);
        let mut sorted: Vec<_> = avoid.iter().collect();
        sorted.sort_unstable();
        sorted.hash(&mut h);
        let mut sorted: Vec<_> = warps.iter().collect();
        sorted.sort_unstable();
        sorted.hash(&mut h);
        let mut sorted: Vec<_> = partners.iter().collect();
        sorted.sort_unstable();
        sorted.hash(&mut h);
        let mut sorted: Vec<_> = phased.iter().collect();
        sorted.sort_unstable();
        for (tile, lands) in sorted {
            tile.hash(&mut h);
            for &(land, flag, on) in lands {
                (land, flag, on).hash(&mut h);
                phase_usable(session, flag, on).hash(&mut h);
                w.system_flag_test(flag).hash(&mut h);
            }
        }
        h.finish()
    };
    if !plan_debug && let Some(plan) = PLAN_MEMO.with(|m| m.borrow().plans.get(&memo_key).cloned())
    {
        PRESS_AT.with(|m| m.borrow_mut().extend(plan.presses.iter().copied()));
        PLAN_STATS.with(|p| {
            let (n, cells) = p.get();
            p.set((n + 1, cells + plan.cells));
        });
        return plan.answer.clone();
    }
    let mut presses: Vec<(Cell, (i16, i16))> = Vec::new();
    let mut s = Search::new(from, gc);
    let start_f = s.h(from);
    s.nodes.open.push(start_f, 0, from);
    let mut best = from;
    let mut walls = WALL_TABLE
        .with(std::cell::Cell::take)
        .unwrap_or_default()
        .for_grid(grid_key);
    // Reused per expanded cell: the platforms and door leaves this cell may
    // pass, and the doors its step probes can touch.
    let mut skip: Vec<(i16, i16)> = Vec::new();
    let mut near_doors: Vec<((i16, i16), Cell)> = Vec::new();
    while let Some((gcur, cur)) = s.nodes.open.pop() {
        if s.nodes.g(cur).is_some_and(|v| v < gcur) {
            continue;
        }
        if s.nodes.len() > MAX_PLAN_NODES {
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
        skip.clear();
        skip.extend(
            platforms
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
                ),
        );
        // A step's probes lie within `DOOR_PROBE_REACH` of the centre on each
        // axis, so only these doors can answer the contact test below; the
        // list keeps `doors`' order, and the test's first match with it.
        near_doors.clear();
        near_doors.extend(doors.iter().copied().filter(|((dx, dz), _)| {
            (i32::from(cx) - i32::from(*dx)).abs() < TOUCH_BOX_HALF + DOOR_PROBE_REACH
                && (i32::from(cz) - i32::from(*dz)).abs() < TOUCH_BOX_HALF + DOOR_PROBE_REACH
                && !skip.contains(&(*dx, *dz))
        }));
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
            if let Some(&(contact, land)) = near_doors.iter().find(|((dx, dz), _)| {
                probes.iter().any(|&(px, pz)| {
                    (px - i32::from(*dx)).abs() < TOUCH_BOX_HALF
                        && (pz - i32::from(*dz)).abs() < TOUCH_BOX_HALF
                })
            }) {
                if land != next && !s.nodes.contains(next) {
                    for (to, d) in arrivals(contact, land) {
                        if to != next {
                            s.step(Some(next), cur, to, gcur + 2 + d);
                        }
                    }
                    presses.push((next, contact));
                    if plan_debug {
                        eprintln!("      [plan] door {contact:?} from {cur:?} -> {land:?}");
                    }
                }
                continue;
            }
            if walls.blocked(w, cur, dir) {
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
            let pad_tile = nt != ct && nt != goal_tile && pad_tiles.contains(&nt);
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
                if !s.nodes.contains(next) {
                    let mut any = false;
                    for (land, wait) in lands {
                        if land != next {
                            s.step(Some(next), cur, land, gcur + 2 + wait);
                            any = true;
                        }
                    }
                    if any {
                        presses.push((next, press));
                    }
                } else if phased.contains_key(&nt) && s.nodes.parent(cur) != Some(next) {
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
                        presses.push((cur, press));
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
            if nt != ct && avoid_tiles.contains(&nt) {
                continue;
            }
            s.step(None, cur, next, gcur + 1);
        }
    }
    WALL_TABLE.with(|t| t.set(Some(walls)));
    let cells = s.nodes.len();
    PLAN_STATS.with(|p| {
        let (n, total) = p.get();
        p.set((n + 1, total + cells as u64));
    });
    PRESS_AT.with(|m| m.borrow_mut().extend(presses.iter().copied()));
    let reached = best == gc;
    let path = if best == from {
        Some(Vec::new())
    } else {
        walk_back(|c| s.nodes.parent(c), s.nodes.len(), from, best)
    };
    let seen = s.nodes.cells().to_vec();
    s.finish();
    let answer = path.map(|path| (path, seen, reached));
    PLAN_MEMO.with(|m| {
        let mut m = m.borrow_mut();
        if m.cells + cells > PLAN_MEMO_CELLS {
            *m = PlanMemo::default();
        }
        m.cells += cells;
        m.plans.insert(
            memo_key,
            std::rc::Rc::new(MemoPlan {
                answer: answer.clone(),
                presses,
                cells: cells as u64,
            }),
        );
    });
    answer
}

/// The route from `from` to `to` along `parent` links. A chain longer than
/// the map holds (`links` cells) is a cycle: `None`, never an unbounded
/// walk.
pub(super) fn walk_back(
    parent: impl Fn(Cell) -> Option<Cell>,
    links: usize,
    from: Cell,
    to: Cell,
) -> Option<Vec<Cell>> {
    let mut path = vec![to];
    let mut c = to;
    while c != from {
        c = parent(c)?;
        if c != from {
            path.push(c);
        }
        if path.len() > links {
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
        let sites = PORTAL_SITES.with(|m| {
            m.borrow_mut()
                .entry(name.clone())
                .or_insert_with(|| std::rc::Rc::new(overworld_portal_sites(&mf, &man, &triggers)))
                .clone()
        });
        for s in sites.iter() {
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
