//! The links a planned walk may step through besides open floor: ledge
//! hops, teleports, object doors, prop rides, and the doors that open on
//! approach.

use super::*;

/// Kind-0 teleport trigger dispatch tile -> landing cell.
pub(super) type WarpMap = HashMap<(i32, i32), Cell>;

thread_local! {
    /// The loaded field scene's kind-0 intra-scene teleports, keyed by the
    /// scene they were read from: trigger dispatch tile -> landing cell.
    pub(super) static TELEPORTS: std::cell::RefCell<(String, WarpMap)> =
        std::cell::RefCell::new((String::new(), HashMap::new()));
    /// The loaded field scene's **ledge hops**, decoded once per scene: see
    /// [`ledge_hops`].
    pub(super) static LEDGE_HOPS: std::cell::RefCell<(String, Vec<LedgeHop>, Vec<ForkedHop>)> =
        const { std::cell::RefCell::new((String::new(), Vec::new(), Vec::new())) };
}

/// Axis-sign bits per compass index, the table at `0x80073F14` the compass
/// walk indexes with `d & 7` (`engine-motion-vm`'s `COMPASS_AXIS_SIGNS`):
/// bit 0 `+Z`, bit 1 `-Z`, bit 2 `+X`, bit 3 `-X`.
const COMPASS_AXIS_SIGNS: [u8; 8] = [0x2, 0xA, 0x8, 0x9, 0x1, 0x5, 0x4, 0x6];

/// One ledge hop: trigger tile, landing cell, and the record's C1 / C2
/// story gates (retail `FUN_8003BDE0`).
pub(super) type LedgeHop = ((i32, i32), Cell, Vec<u16>, Vec<u16>);

/// A walk-on record whose landing hangs on story flags: trigger tile, body,
/// first-opcode offset and C1 / C2 gates ([`flag_path_landing`]).
pub(super) type ForkedHop = ((i32, i32), Vec<u8>, usize, Vec<u16>, Vec<u16>);

/// The loaded field scene's **ledge hops** under the live flags: gate-1
/// walk-on tiles whose partition-2 record the flags let spawn and whose
/// body arcs the player to a landing tile (op `0x43` sub-0/1/A/B on the
/// `0xF8` channel, `FUN_801DE840` case `0x43`) - trigger tile -> landing
/// cell. A mountain joins its terraces with these the way a town joins its
/// rooms with kind-0 teleports (`rikuroa` P2[6..25]). A record that hops
/// more than once, or tests where the player stands first, is left out: its
/// landing is not one place.
pub(super) fn ledge_hops(session: &NativeSession) -> WarpMap {
    use legaia_engine_field::man_field_scripts::{partition_record_span, partition2_record_gates};
    use legaia_scene_formats::field_disasm::{ActorCtrlKind, InsnInfo, LinearWalker, MenuCtrlKind};
    if session.host.world.mode != SceneMode::Field {
        return HashMap::new();
    }
    let name = scene_name(session);
    LEDGE_HOPS.with(|t| {
        let mut t = t.borrow_mut();
        if t.0 != name {
            let mut hops = Vec::new();
            let mut forked = Vec::new();
            if let Some((mf, man, triggers)) = scene_man_and_triggers(session) {
                for tr in triggers.iter().filter(|t| t.gate == 1) {
                    let rec = usize::from(tr.record);
                    let Some((start, pc0, len)) = partition_record_span(&mf, &man, 2, rec) else {
                        continue;
                    };
                    let mut lands = Vec::new();
                    let mut branches = false;
                    let mut leaves = false;
                    let mut forks = false;
                    let body = &man[start..start + len];
                    // Op `0x47` walk-to-tile legs on the player (`C7 F8 x z
                    // mode`, the kernel `FUN_8003774C` case `0x47`): a
                    // transporter pad carries the walker across a wall
                    // (`deroa` P2[12] / P2[14], the platforms between its
                    // shaft rooms).
                    let mut walks = Vec::new();
                    // Compass legs on the player (`B7 F8 d n` / `C1 F8 d n`,
                    // ops `0x37` / `0x41`, the kernel `FUN_8003774C` arm
                    // ported as `engine_motion_vm::compass_walk`): each moves
                    // the player `rate * (n & 0x3F)` units along compass `d`,
                    // rate `0x80` for `0x37` and `0x40` for `0x41`. A lift
                    // band carries the walker this way (`dohaty` P2[3..8],
                    // the castle's stairs between its floors).
                    let mut compass: Option<(i32, i32)> = None;
                    // Story flags the record raises and lowers: a transport
                    // lowers every one it raises (its hold), a story beat
                    // leaves its mark.
                    let (mut sets, mut clears) = (BTreeSet::new(), BTreeSet::new());
                    for insn in LinearWalker::new(body, pc0).flatten() {
                        if let InsnInfo::SystemFlag { kind, idx, .. } = insn.info {
                            match kind {
                                legaia_scene_formats::field_disasm::FlagKind::Set => {
                                    sets.insert(idx);
                                }
                                legaia_scene_formats::field_disasm::FlagKind::Clear => {
                                    clears.insert(idx);
                                }
                                _ => {}
                            }
                        }
                        if insn.opcode == 0x47
                            && insn.extended == Some(0xF8)
                            && let (Some(&x), Some(&z)) =
                                (body.get(insn.pc + 2), body.get(insn.pc + 3))
                        {
                            walks.push((x, z));
                        }
                        if matches!(insn.opcode, 0x37 | 0x41)
                            && insn.extended == Some(0xF8)
                            && let (Some(&d), Some(&n)) =
                                (body.get(insn.pc + 2), body.get(insn.pc + 3))
                        {
                            let rate = if insn.opcode == 0x37 { 0x80 } else { 0x40 };
                            let len = rate * i32::from(n & 0x3F);
                            let signs = COMPASS_AXIS_SIGNS[usize::from(d & 7)];
                            let (mut dx, mut dz) = compass.unwrap_or((0, 0));
                            if signs & 1 != 0 {
                                dz += len;
                            }
                            if signs & 2 != 0 {
                                dz -= len;
                            }
                            if signs & 4 != 0 {
                                dx += len;
                            }
                            if signs & 8 != 0 {
                                dx -= len;
                            }
                            compass = Some((dx, dz));
                        }
                        match insn.info {
                            InsnInfo::ActorCtrl {
                                kind: ActorCtrlKind::ArcJump { tile_x, tile_z, .. },
                                ..
                            } if insn.extended == Some(0xF8) && (tile_x, tile_z) != (0, 0) => {
                                lands.push((tile_x, tile_z));
                            }
                            InsnInfo::BBoxTest { .. } => branches = true,
                            InsnInfo::CondJmp { .. }
                            | InsnInfo::Picker { .. }
                            | InsnInfo::SystemFlag {
                                kind: legaia_scene_formats::field_disasm::FlagKind::Test,
                                ..
                            }
                            | InsnInfo::GFlag {
                                kind: legaia_scene_formats::field_disasm::FlagKind::Test,
                                ..
                            } => forks = true,
                            InsnInfo::SceneChange { .. }
                            | InsnInfo::MenuCtrl {
                                kind: MenuCtrlKind::FmvTrigger { .. },
                                ..
                            } => leaves = true,
                            _ => {}
                        }
                    }
                    let at = |b: u8| {
                        i16::from(b & 0x7F) * 128 + 0x40 + if b & 0x80 != 0 { 0x40 } else { 0 }
                    };
                    // A warp pad: no arc, and every player-channel snap the
                    // record carries (`0x23` MOVE_TO / `4C 51`, on whichever
                    // story-flag branch) lands on one tile. Rogue Tower's
                    // floors join only through these (`rugi` P2[0..37]).
                    let mut snaps: Vec<(i16, i16)> =
                        legaia_engine_field::man_field_scripts::player_moves_in_region(body, pc0)
                            .into_iter()
                            .filter(|m| m.is_teleport())
                            .map(|m| (m.world_x, m.world_z))
                            .collect();
                    snaps.dedup();
                    let land = match (&lands[..], &snaps[..]) {
                        ([(x, z)], []) => Some(cell_of(at(*x), at(*z))),
                        // A chain of arcs with no fork in the record - the
                        // stepping-stone hops (`nilboa` P2[8] / P2[9] /
                        // P2[12] / P2[13], two or three arcs each) - lands
                        // where its last arc does.
                        ([.., (x, z)], []) if !forks => Some(cell_of(at(*x), at(*z))),
                        ([], [(x, z)]) if !leaves => Some(cell_of(*x, *z)),
                        // A fork-free pad that shows the player something on
                        // the way lands where its last snap does: `garmel`'s
                        // transport pads P2[0] / P2[1] / P2[8] / P2[9] snap
                        // the party to the tank room (8, 117) for the ride's
                        // cutscene, then to the far pad. Every flag such a
                        // pad raises it lowers again; a cutscene that leaves
                        // one up is a story beat, not a way across (`rayman`
                        // P2[19] seats the player three times and raises
                        // `0x1FC`).
                        ([], [.., (x, z)]) if !forks && !leaves && sets.is_subset(&clears) => {
                            Some(cell_of(*x, *z))
                        }
                        // A fork-free run of player walk legs ends where its
                        // last leg does.
                        ([], []) if !forks && !leaves && !walks.is_empty() => {
                            walks.last().map(|&(x, z)| cell_of(at(x), at(z)))
                        }
                        // A fork-free run of compass legs that lowers every
                        // flag it raises (its hold) lands its summed
                        // displacement from the band tile it was stepped on;
                        // one that ends inside a tile of where it began (a
                        // step back off a band) carries the walker nowhere.
                        ([], [])
                            if !forks
                                && !leaves
                                && sets.is_subset(&clears)
                                && compass
                                    .is_some_and(|(dx, dz)| dx.abs().max(dz.abs()) >= 128) =>
                        {
                            compass.map(|(dx, dz)| {
                                let (x, z) = tile_center((tr.tile_x.into(), tr.tile_z.into()));
                                open_landing(
                                    &session.host.world,
                                    cell_of(
                                        (i32::from(x) + dx).clamp(0, 0x3FFF) as i16,
                                        (i32::from(z) + dz).clamp(0, 0x3FFF) as i16,
                                    ),
                                )
                            })
                        }
                        _ => None,
                    };
                    if let Some(land) = land
                        && !branches
                    {
                        let (c1, c2) = partition2_record_gates(&mf, &man, rec).unwrap_or_default();
                        hops.push(((i32::from(tr.tile_x), i32::from(tr.tile_z)), land, c1, c2));
                    } else if !leaves
                        && ((land.is_none() && forks && !branches && !walks.is_empty())
                            || (branches && !lands.is_empty()))
                    {
                        // A record that tests where the player stands
                        // (`rikuroa` P2[60]: a box per landing, the arc out
                        // of the one the band tile sits in) is followed from
                        // the band tile it is stepped on.
                        let (c1, c2) = partition2_record_gates(&mf, &man, rec).unwrap_or_default();
                        forked.push((
                            (i32::from(tr.tile_x), i32::from(tr.tile_z)),
                            body.to_vec(),
                            pc0,
                            c1,
                            c2,
                        ));
                    }
                }
            }
            *t = (name, hops, forked);
        }
        let w = &session.host.world;
        let mut map = HashMap::new();
        for (tile, land, c1, c2) in &t.1 {
            if w.p2_record_gates_pass(c1, c2) {
                map.entry(*tile).or_insert(*land);
            }
        }
        for (tile, body, pc0, c1, c2) in &t.2 {
            if w.p2_record_gates_pass(c1, c2)
                && let Some(land) = flag_path_landing(w, body, *pc0, Some(*tile))
            {
                map.entry(*tile).or_insert(open_landing(w, land));
            }
        }
        map
    })
}

thread_local! {
    /// The loaded field scene's **clock flags**, keyed by scene: see
    /// [`clock_flags`].
    static CLOCK_FLAGS: std::cell::RefCell<(String, BTreeSet<u16>)> =
        const { std::cell::RefCell::new((String::new(), BTreeSet::new())) };
}

/// The story flags the loaded scene's ambient move-VM stagers write: the
/// move VM's overlay-extension set / clear (`2F 1C flag` / `2F 1D flag`,
/// u16 words, bank `DAT_80085758`) found in the scene's prescript stager
/// records. Such a flag is a clock, not story state: the stager flips it as
/// time passes, whatever the player does. Rogue Tower's pad cycler (`rugi`'s
/// prescript) waits on `0x594`, then raises `0x463` and `0x462` in turn for
/// a stretch of each lap and drops them again, and its upper pads
/// (P2[40..61]) test `0x462` to pick their landing.
pub(super) fn clock_flags(session: &NativeSession) -> BTreeSet<u16> {
    if session.host.world.mode != SceneMode::Field {
        return BTreeSet::new();
    }
    let name = scene_name(session);
    CLOCK_FLAGS.with(|t| {
        let mut t = t.borrow_mut();
        if t.0 != name {
            let mut flags = BTreeSet::new();
            if let Ok(scene) = Scene::load(&session.host.index, &name)
                && let Some(ev) = scene.find_event_scripts()
            {
                for &(start, end) in &ev.record_ranges {
                    let rec = &ev.bytes[start.min(ev.bytes.len())..end.min(ev.bytes.len())];
                    let words: Vec<u16> = rec
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| u16::from_le_bytes(*c))
                        .collect();
                    for w in words.windows(3) {
                        if w[0] == 0x2F && matches!(w[1], 0x1C | 0x1D) {
                            flags.insert(w[2]);
                        }
                    }
                }
            }
            *t = (name, flags);
        }
        t.1.clone()
    })
}

/// One landing of a pad that forks on a clock flag: where it lands, and the
/// flag and phase it lands there in.
pub(super) type PhasedLanding = (Cell, u16, bool);

/// The loaded scene's walk-on pads whose landing hangs on a [`clock_flags`]
/// flag, under the live story flags otherwise: trigger tile -> each phase's
/// landing. A player times the step onto such a pad; the planner offers both
/// landings and the follower waits on the pad's edge for the phase its route
/// wants ([`phase_ready`]).
pub(super) fn phased_pads(session: &NativeSession) -> HashMap<(i32, i32), Vec<PhasedLanding>> {
    let clocks = clock_flags(session);
    let mut out = HashMap::new();
    if clocks.is_empty() {
        return out;
    }
    let _ = ledge_hops(session);
    let w = &session.host.world;
    LEDGE_HOPS.with(|t| {
        let t = t.borrow();
        for (tile, body, pc0, c1, c2) in &t.2 {
            if !w.p2_record_gates_pass(c1, c2) {
                continue;
            }
            let tested: BTreeSet<u16> =
                legaia_scene_formats::field_disasm::LinearWalker::new(body, *pc0)
                    .flatten()
                    .filter_map(|insn| match insn.info {
                        legaia_scene_formats::field_disasm::InsnInfo::SystemFlag {
                            kind: legaia_scene_formats::field_disasm::FlagKind::Test,
                            idx,
                            ..
                        } if clocks.contains(&idx) => Some(idx),
                        _ => None,
                    })
                    .collect();
            // One clock per pad: two would make four phases the follower
            // cannot time from one flag's edges.
            let [flag] = tested.iter().copied().collect::<Vec<_>>()[..] else {
                continue;
            };
            let lands: Vec<PhasedLanding> = [false, true]
                .into_iter()
                .filter_map(|on| {
                    let test = |f: u16| if f == flag { on } else { w.system_flag_test(f) };
                    flag_path_landing_with(&test, body, *pc0, Some(*tile))
                        .map(|l| (open_landing(w, l), flag, on))
                })
                .collect();
            if lands.len() == 2 && lands[0].0 != lands[1].0 {
                out.insert(*tile, lands);
            }
        }
    });
    out
}

/// Frames from the step onto a clock pad to its flag test: the walk leg onto
/// the pad, the fade, and the `4A 0A 00` / `4A 3C 00` waits before `rugi`
/// P2[40]'s `74 62 69 00`. A phase that ends sooner than this after the
/// step sends the walker the other way.
pub(super) const PHASE_TEST_LEAD: u64 = 120;
/// A phase seen to start no more than this many frames ago is taken while
/// its length is still unknown.
pub(super) const PHASE_FRESH: u64 = 12;
/// A clock flag that has not flipped for this long is held: `rugi`'s cycler
/// parks on `0x46A` once Rogue is beaten. The follower then steps on and
/// lets the live landing stand.
pub(super) const PHASE_FROZEN: u64 = 4000;

/// One clock flag as the follower has watched it, in world frames: its
/// value, when it flipped into it (known only when the flip was seen to the
/// frame), the first frame it was seen holding it, the last frame it was
/// looked at, and the shortest run seen of each phase.
#[derive(Clone, Copy)]
struct ClockWatch {
    value: bool,
    flipped_at: Option<u64>,
    held_since: u64,
    looked: u64,
    shortest: [Option<u64>; 2],
}

thread_local! {
    static CLOCK_WATCH: std::cell::RefCell<(String, HashMap<u16, ClockWatch>)> =
        std::cell::RefCell::new((String::new(), HashMap::new()));
}

/// Look at the loaded scene's clock flags: call once per walk tick.
pub(super) fn watch_clocks(session: &NativeSession) {
    let clocks = clock_flags(session);
    if clocks.is_empty() {
        return;
    }
    let name = scene_name(session);
    let w = &session.host.world;
    let now = w.frame;
    CLOCK_WATCH.with(|c| {
        let mut c = c.borrow_mut();
        if c.0 != name {
            *c = (name, HashMap::new());
        }
        for &f in &clocks {
            let v = w.system_flag_test(f);
            let e = c.1.entry(f).or_insert(ClockWatch {
                value: v,
                flipped_at: None,
                held_since: now,
                looked: now,
                shortest: [None; 2],
            });
            // A gap in the looking (a fight, a script) restarts the hold
            // count: the flag may have flipped and back unseen.
            if now.saturating_sub(e.looked) > 2 {
                e.held_since = now;
            }
            if e.value != v {
                // A flip seen within a frame or two of the last look is
                // timed; one that happened while nobody looked is not.
                let seen = now.saturating_sub(e.looked) <= 2;
                if seen && let Some(t) = e.flipped_at {
                    let k = usize::from(e.value);
                    let n = now - t;
                    e.shortest[k] = Some(e.shortest[k].map_or(n, |m| m.min(n)));
                }
                if std::env::var_os("LEGAIA_FGL_CLOCK_DEBUG").is_some() {
                    eprintln!(
                        "      [clock] {f:#05X} -> {v} at frame {now} (timed {seen}, shortest {:?})",
                        e.shortest
                    );
                }
                e.value = v;
                e.flipped_at = seen.then_some(now);
                e.held_since = now;
            }
            e.looked = now;
        }
    });
}

/// May the walker step onto a pad now and be sent by `flag` in phase `on`?
/// Yes when the flag is in that phase and the phase will still hold when the
/// pad tests it ([`PHASE_TEST_LEAD`]), judged from the shortest run of the
/// phase seen so far; a phase whose length is not known yet is taken only
/// fresh. A clock that has stopped ([`PHASE_FROZEN`]) is not waited on.
pub(super) fn phase_ready(session: &NativeSession, flag: u16, on: bool) -> bool {
    let w = &session.host.world;
    let now = w.frame;
    CLOCK_WATCH.with(|c| {
        let c = c.borrow();
        let Some(e) = c.1.get(&flag) else {
            return w.system_flag_test(flag) == on;
        };
        if now.saturating_sub(e.held_since) >= PHASE_FROZEN {
            return true;
        }
        if e.value != on {
            return false;
        }
        let Some(t) = e.flipped_at else {
            return false;
        };
        let since = now - t;
        match e.shortest[usize::from(on)] {
            Some(len) => since + PHASE_TEST_LEAD <= len,
            None => since <= PHASE_FRESH,
        }
    })
}

/// Can a step ever be timed into phase `on` of `flag`? Not when the phase
/// has been seen to run shorter than the pad takes to test it: `rugi`'s
/// `0x462` is up for under a hundred frames a lap, and a step taken as it
/// rises is tested after it has dropped. A phase not yet timed is assumed
/// usable.
pub(super) fn phase_usable(session: &NativeSession, flag: u16, on: bool) -> bool {
    let name = scene_name(session);
    CLOCK_WATCH.with(|c| {
        let c = c.borrow();
        if c.0 != name {
            return true;
        }
        c.1.get(&flag)
            .and_then(|e| e.shortest[usize::from(on)])
            .is_none_or(|len| len > PHASE_TEST_LEAD + PHASE_FRESH)
    })
}

/// The clock phase the route wants of the pad it steps onto next: `path[0]`
/// a pad tile, `path[1]` the landing (not a lattice step away).
pub(super) fn phase_wanted(
    session: &NativeSession,
    path: &[Cell],
    at: Cell,
) -> Option<(u16, bool)> {
    let (&next, &land) = (path.first()?, path.get(1)?);
    if adjacent(next, land) {
        return None;
    }
    // Either the route steps into the pad tile and lands from there, or it
    // jumps from the walker's own cell, pressing toward the pad tile.
    let (nx, nz) = if next == at {
        PRESS_AT.with(|m| m.borrow().get(&at).copied())?
    } else {
        cell_center(next)
    };
    let (ax, az) = cell_center(at);
    let tile = dispatch_tile(nx, nz);
    if dispatch_tile(ax, az) == tile {
        return None;
    }
    phased_pads(session)
        .get(&tile)?
        .iter()
        .find(|&&(l, _, _)| l == land)
        .map(|&(_, f, on)| (f, on))
}

/// `land`, or the nearest open lattice cell within a tile of it. A walk
/// leg's target carries the half-tile bit (`0x80`), and the leg's own
/// arrival stops short of it: `deroa`'s hub P2[11] walks its west leg to
/// `0xB6` (x 7040) and the platform parks the player at x 6912, beside the
/// pillar the encoded point sits in. A landing in a wall cell joins nothing.
pub(super) fn open_landing(w: &legaia_engine_core::world::World, land: Cell) -> Cell {
    let open = |c: Cell| {
        let (x, z) = cell_center(c);
        !w.field_tile_is_wall(x, z)
    };
    if open(land) {
        return land;
    }
    (1..=4)
        .flat_map(|r| {
            (-r..=r).flat_map(move |dx| (-r..=r).map(move |dz| (land.0 + dx, land.1 + dz)))
        })
        .find(|&c| open(c))
        .unwrap_or(land)
}

/// Where a walk-on record whose only forks are story-flag tests walks the
/// player under the live flags: the run is followed from its first opcode
/// (a `SysFlag.Test` jumps when its flag is set - the same reading
/// [`legaia_engine_field::man_field_scripts::placement_spawn_relocation`]
/// makes), and the last player walk leg (`C7 F8 x z`), snap (`A3 F8 x z`)
/// or arc on it is the landing. `deroa`'s transporter hub P2[11] sends its platform to one of
/// three shaft rooms by the switch flags `0x4EF` / `0x4F0`.
///
/// Given the band `tile` the walker steps on, a box test on the player
/// (`CD F8 x0 z0 x1 z1`, `FUN_801DE840` case `0x4D`: inside falls through,
/// outside skips) is answered from that tile, and a random-chance branch
/// (`4E .. 04`, `Rand() & 0xFF` against the operand) is followed both ways
/// and lands only where both arms agree. `rikuroa` P2[60], the climb back
/// up the terrace P2[45] hops down, arcs the player out of whichever box the
/// band tile sits in, and on a die roll plays a stumble first, to the same
/// row.
pub(super) fn flag_path_landing(
    w: &legaia_engine_core::world::World,
    body: &[u8],
    pc0: usize,
    tile: Option<(i32, i32)>,
) -> Option<Cell> {
    flag_path_from(
        &|f| w.system_flag_test(f),
        body,
        pc0,
        tile,
        &mut HashSet::new(),
        None,
    )
}

/// [`flag_path_landing`] with the story flags answered by `test` rather than
/// the live world: how a pad that forks on a clock flag lands in each phase.
pub(super) fn flag_path_landing_with(
    test: &dyn Fn(u16) -> bool,
    body: &[u8],
    pc0: usize,
    tile: Option<(i32, i32)>,
) -> Option<Cell> {
    flag_path_from(test, body, pc0, tile, &mut HashSet::new(), None)
}

fn flag_path_from(
    test: &dyn Fn(u16) -> bool,
    body: &[u8],
    pc0: usize,
    tile: Option<(i32, i32)>,
    seen: &mut HashSet<usize>,
    mut last: Option<Cell>,
) -> Option<Cell> {
    use legaia_scene_formats::field_disasm::{
        ActorCtrlKind, FlagKind, InsnInfo, InventoryCmpKind, decode,
    };
    let at = |b: u8| i16::from(b & 0x7F) * 128 + 0x40 + if b & 0x80 != 0 { 0x40 } else { 0 };
    let mut pc = pc0;
    while seen.insert(pc) {
        let &op = body.get(pc)?;
        if op == 0x21 {
            break;
        }
        let insn = decode(body, pc).ok()?;
        // A walk leg (`C7 F8 x z`) or a snap (`A3 F8 x z`, op `0x23` on the
        // player): the snap is a warp pad's landing. Rogue Tower's upper
        // pads (`rugi` P2[40..61]) walk the player onto the pad and then
        // snap it to the partner pad or, under `0x462`, back to the hub.
        if matches!(insn.opcode, 0x47 | 0x23)
            && insn.extended == Some(0xF8)
            && let (Some(&x), Some(&z)) = (body.get(pc + 2), body.get(pc + 3))
            && (x & 0x7F, z & 0x7F) != (0x7F, 0x7F)
        {
            last = Some(cell_of(at(x), at(z)));
        }
        match insn.info {
            InsnInfo::SystemFlag {
                kind: FlagKind::Test,
                idx,
                target: Some(target),
                ..
            } => {
                if test(idx) {
                    pc = target & 0xFFFF;
                    continue;
                }
            }
            InsnInfo::JmpRel { target, .. } => {
                pc = target & 0xFFFF;
                continue;
            }
            InsnInfo::ActorCtrl {
                kind: ActorCtrlKind::ArcJump { tile_x, tile_z, .. },
                ..
            } if insn.extended == Some(0xF8) && (tile_x, tile_z) != (0, 0) => {
                last = Some(cell_of(at(tile_x), at(tile_z)));
            }
            InsnInfo::BBoxTest {
                x_min,
                z_min,
                x_max,
                z_max,
                skip_target,
                ..
            } if insn.extended == Some(0xF8) && tile.is_some() => {
                let (tx, tz) = tile?;
                let inside = (i32::from(x_min)..=i32::from(x_max)).contains(&tx)
                    && (i32::from(z_min)..=i32::from(z_max)).contains(&tz);
                if !inside {
                    pc = skip_target & 0xFFFF;
                    continue;
                }
            }
            InsnInfo::InventoryCmp {
                kind:
                    InventoryCmpKind::Compare {
                        sub_op: 4,
                        skip_target,
                        ..
                    },
                ..
            } => {
                let mut other = seen.clone();
                let skip = flag_path_from(test, body, skip_target & 0xFFFF, tile, &mut other, last);
                let on = flag_path_from(test, body, pc + insn.size.max(1), tile, seen, last);
                return (skip == on).then_some(on).flatten();
            }
            InsnInfo::BBoxTest { .. } | InsnInfo::CondJmp { .. } | InsnInfo::Picker { .. } => {
                return None;
            }
            _ => {}
        }
        pc += insn.size.max(1);
    }
    last
}

/// The loaded field scene's kind-0 intra-scene teleports (`.MAP` trigger
/// block, [`legaia_engine_field::field_regions::IntraSceneTeleport`]): the
/// tile that repositions the player outright, and the cell it lands in.
/// House interiors and dungeon rooms are laid out in the same field map as
/// the street outside, joined only by these, so a planner that does not
/// model them sees each room as a separate walk component.
pub(super) fn teleports(session: &NativeSession) -> WarpMap {
    if session.host.world.mode != SceneMode::Field {
        return HashMap::new();
    }
    let name = scene_name(session);
    TELEPORTS.with(|t| {
        let mut t = t.borrow_mut();
        if t.0 != name {
            let index = &session.host.index;
            let mut map = HashMap::new();
            if let Ok(scene) = Scene::load(index, &name)
                && let Ok((p, f)) = scene.field_intra_scene_teleports(index)
            {
                // The dispatch takes the first primary-then-fallback match.
                for tp in p.iter().chain(f.iter()) {
                    let (x, z) = tp.dest_world();
                    map.entry((i32::from(tp.tile_x), i32::from(tp.tile_z)))
                        .or_insert(cell_of(x, z));
                }
            }
            *t = (name, map);
        }
        let mut warps = t.1.clone();
        for (k, v) in ledge_hops(session) {
            warps.entry(k).or_insert(v);
        }
        warps
    })
}

/// Retail's walk-touch contact box half-extent (`FIELD_PROP_BOX_HALF`,
/// engine-core `world::config`): a movement probe point inside it posts the
/// placement's touch.
pub(super) const TOUCH_BOX_HALF: i32 = 0x50;

/// Retail's actor-collision probe points per step direction (Z-, X-, Z+,
/// X+; engine-core `world::config::FIELD_ACTOR_PROBES`, table
/// `DAT_801f21b4`), applied as `(x + dx, z - dz)`: the points whose contact
/// with a walk-touch box posts the touch.
pub(super) const ACTOR_PROBES: [[(i16, i16); 3]; 4] = [
    [(-32, 64), (0, 64), (32, 64)],
    [(-63, -32), (-63, 0), (-63, 32)],
    [(-32, -63), (0, -63), (32, -63)],
    [(64, -32), (64, 0), (64, 32)],
];

/// The loaded field scene's **object doors** under the live flags: each
/// `.MAP`-object walk-touch placement whose record, resolved against the
/// story flags as the contact dispatch resolves it, teleports the player
/// (`WalkTouchEvent::PlayerMoveTo`) - contact centre and landing cell.
/// Vahn's front door and every house door of a town are this class.
pub(super) fn object_doors(session: &NativeSession) -> Vec<((i16, i16), Cell)> {
    use legaia_engine_field::man_field_scripts::{WalkTouchEvent, resolve_walk_touch_arm};
    let w = &session.host.world;
    if w.mode != SceneMode::Field {
        return Vec::new();
    }
    let parsed = w.field_vm.channels_man.as_ref().and_then(|man| {
        Some((
            legaia_scene_formats::man_section::parse(man).ok()?,
            man.clone(),
        ))
    });
    let mut out = Vec::new();
    for (&slot, &(contact, event)) in &w.props.walk_touch {
        let live = w
            .props
            .walk_touch_records
            .get(&slot)
            .and_then(|&flat| {
                let (mf, man) = parsed.as_ref()?;
                resolve_walk_touch_arm(mf, man, flat, &|f| w.system_flag_test(f))
            })
            .unwrap_or(Some(event));
        if let Some(WalkTouchEvent::PlayerMoveTo {
            world_x, world_z, ..
        }) = live
        {
            out.push((contact, cell_of(world_x, world_z)));
        }
    }
    out.extend(prop_rides(session));
    out
}

/// The loaded field scene's **rides** under the live flags: each solid
/// touch-class prop whose bind record, resumed where it is parked and
/// followed against the story flags as the touch runs it, walks the player
/// with a walk-to-tile (`C7 F8 <tx> <tz> <mode>`) before its `21` - contact
/// centre and the cell of the last leg's tile. `taiku` P0[6], the lift at
/// (109, 78), walks the player aboard, rides and walks them off the far
/// side, each touch the other way (`0x38B`).
/// Where partition-2 record `global_index` (a spawn's flat index) leaves the
/// player when it carries them as a ride: its last player reposition (walk
/// leg, `0x23` MOVE_TO or `4C 51` seat), plus the compass legs after it
/// (`B7 F8 d n` / `C1 F8 d n`). `chitei2` P2[7] seats the party at (94, 24)
/// beside the car and walks it two tiles north (`C1 F8 00 84`) to (94, 22);
/// P2[8] seats it at (100, 102) and walks it two tiles west to (98, 102).
/// A record that leaves the scene or forks on a story flag or a picker is
/// not one ride.
fn spawned_ride_landing(session: &NativeSession, global_index: u8) -> Option<Cell> {
    use legaia_engine_field::man_field_scripts::{partition_record_span, player_moves_in_region};
    use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker};
    let (mf, man, _) = scene_man_and_triggers(session)?;
    let base = mf.partitions.first().map_or(0, Vec::len) + mf.partitions.get(1).map_or(0, Vec::len);
    let rec = usize::from(global_index).checked_sub(base)?;
    let (start, pc0, len) = partition_record_span(&mf, &man, 2, rec)?;
    let body = &man[start..start + len];
    let insns: Vec<_> = LinearWalker::new(body, pc0).map_while(Result::ok).collect();
    if insns.iter().any(|i| {
        i.opcode == 0x3F
            || matches!(
                i.info,
                InsnInfo::SystemFlag {
                    kind: FlagKind::Test,
                    ..
                } | InsnInfo::Picker { .. }
                    | InsnInfo::CondJmp { .. }
                    | InsnInfo::BBoxTest { .. }
            )
    }) {
        return None;
    }
    let last = player_moves_in_region(body, pc0).into_iter().next_back()?;
    let (mut dx, mut dz) = (0i32, 0i32);
    for i in insns.iter().filter(|i| i.pc > last.pc) {
        if matches!(i.opcode, 0x37 | 0x41)
            && i.extended == Some(0xF8)
            && let (Some(&d), Some(&n)) = (body.get(i.pc + 2), body.get(i.pc + 3))
        {
            let rate = if i.opcode == 0x37 { 0x80 } else { 0x40 };
            let step = rate * i32::from(n & 0x3F);
            let signs = COMPASS_AXIS_SIGNS[usize::from(d & 7)];
            if signs & 1 != 0 {
                dz += step;
            }
            if signs & 2 != 0 {
                dz -= step;
            }
            if signs & 4 != 0 {
                dx += step;
            }
            if signs & 8 != 0 {
                dx -= step;
            }
        }
    }
    let x = (i32::from(last.world_x) + dx).clamp(0, 0x3FFF) as i16;
    let z = (i32::from(last.world_z) + dz).clamp(0, 0x3FFF) as i16;
    Some(open_landing(&session.host.world, cell_of(x, z)))
}

pub(super) fn prop_rides(session: &NativeSession) -> Vec<((i16, i16), Cell)> {
    use legaia_scene_formats::field_disasm::{InsnInfo, decode};
    let w = &session.host.world;
    if w.mode != SceneMode::Field {
        return Vec::new();
    }
    let tile =
        |b: u8| -> i16 { i16::from(b & 0x7F) * 0x80 + 0x40 + if b & 0x80 != 0 { 0x40 } else { 0 } };
    let mut out = Vec::new();
    for (&anchor, prop) in &w.props.bank.props {
        // A walk-touch door's landing is its decoded teleport
        // ([`object_doors`]); the engine plays no walk leg for it.
        if w.props
            .walk_touch_records
            .values()
            .any(|&r| r == prop.record)
        {
            continue;
        }
        let Some(c) = w
            .props
            .colliders
            .iter()
            .find(|c| c.anchor == Some(anchor) && c.solid && !c.interact && !c.moving_box)
        else {
            continue;
        };
        let body = &prop.record_body[..];
        let mut pc = prop.parked_pc;
        let mut seen = HashSet::new();
        let mut last = None;
        while seen.insert(pc) {
            let Ok(insn) = decode(body, pc) else { break };
            // A touch resumes past the yield it parked on.
            if insn.opcode == 0x21 && pc == prop.parked_pc {
                pc += 1;
                continue;
            }
            if insn.size == 0 || insn.opcode == 0x21 {
                break;
            }
            // A bind record that spawns the ride as a partition-2 record
            // (`chitei2`'s cars: P0[5] at (100, 102) spawns P2[7], P0[12]
            // at (94, 24) spawns P2[8]) lands where that record leaves the
            // player.
            if let InsnInfo::SpawnRecord { global_index } = insn.info {
                last = spawned_ride_landing(session, global_index);
                break;
            }
            // A record that leaves the scene or teleports the player after
            // its walk-in is a door, not a ride: `retockin` P0[0] walks the
            // player into the doorway and changes scene to `jagaroom`. Its
            // way out is the scene graph's, not a walk edge.
            if insn.opcode == 0x3F || (insn.opcode == 0x23 && insn.extended == Some(0xF8)) {
                last = None;
                break;
            }
            match insn.info {
                InsnInfo::SystemFlag {
                    kind: FlagKind::Test,
                    idx,
                    target: Some(target),
                    ..
                } if w.system_flag_test(idx) => {
                    pc = target;
                    continue;
                }
                InsnInfo::JmpRel { target, .. } => {
                    pc = target;
                    continue;
                }
                _ => {}
            }
            if insn.opcode == 0x47
                && insn.extended == Some(0xF8)
                && let (Some(&x), Some(&z)) = (body.get(pc + 2), body.get(pc + 3))
                && (x & 0x7F, z & 0x7F) != (0x7F, 0x7F)
            {
                last = Some(cell_of(tile(x), tile(z)));
            }
            pc += insn.size;
        }
        if let (Some(land), Ok(x), Ok(z)) =
            (last, i16::try_from(c.center.0), i16::try_from(c.center.1))
        {
            if std::env::var_os("LEGAIA_FGL_PLAN_DEBUG").is_some() {
                eprintln!(
                    "      [plan] ride {} P0[{}] at {anchor:?} contact ({x},{z}) lands {land:?} from pc {}",
                    scene_name(session),
                    prop.record,
                    prop.parked_pc
                );
            }
            out.push(((x, z), land));
        }
    }
    out
}

/// For each object door's contact centre, the contact centres of the doors
/// its record brackets for the arrival (`B1 <obj> 00`,
/// `World::arm_arrival_bracket`): the lift platform a ride lands the player
/// on. The engine exempts that platform from the touch dispatch and the
/// collision probe until the player is out of its reach, so the landing is
/// a pocket the player walks off, not a cell pinned against the platform.
pub(super) fn door_partners(session: &NativeSession) -> HashMap<(i16, i16), Vec<(i16, i16)>> {
    use legaia_engine_field::man_field_scripts::record_exempted_objects;
    let w = &session.host.world;
    let Some(man) = w.field_vm.channels_man.as_ref() else {
        return HashMap::new();
    };
    let Ok(mf) = legaia_scene_formats::man_section::parse(man) else {
        return HashMap::new();
    };
    let mut out: HashMap<(i16, i16), Vec<(i16, i16)>> = HashMap::new();
    for (&slot, &rec) in &w.props.walk_touch_records {
        let Some(&(contact, _)) = w.props.walk_touch.get(&slot) else {
            continue;
        };
        let targets = record_exempted_objects(&mf, man, rec);
        for (&s2, &r2) in &w.props.walk_touch_records {
            if s2 != slot
                && targets.iter().any(|&t| usize::from(t) == r2)
                && let Some(&(pos, _)) = w.props.walk_touch.get(&s2)
            {
                out.entry(contact).or_default().push(pos);
            }
        }
    }
    out
}

/// Whether a player standing at cell `c` reaches the contact box centred on
/// `at` from its centre or any of its four probe sets.
pub(super) fn in_reach(c: Cell, at: (i16, i16)) -> bool {
    let (cx, cz) = cell_center(c);
    std::iter::once((0, 0))
        .chain(ACTOR_PROBES.iter().flatten().copied())
        .any(|(dx, dz)| {
            let (px, pz) = (i32::from(cx) + i32::from(dx), i32::from(cz) - i32::from(dz));
            (px - i32::from(at.0)).abs() < TOUCH_BOX_HALF
                && (pz - i32::from(at.1)).abs() < TOUCH_BOX_HALF
        })
}

/// Anchors of the placed props whose bind record runs `31 00` at a clean
/// decode boundary: the touch pass of a door. Bit 0 of the actor flag word
/// `+0x10` is the collision / touch exemption (`FUN_801CF754` /
/// `FUN_801CF9F4` skip `flags & 3`), so the leaf stops blocking once the
/// player's press into it has posted the touch - the press is the opening.
pub(super) fn self_opening_doors(session: &NativeSession) -> HashSet<(u8, u8)> {
    door_openings(session)
        .into_iter()
        .filter(|(_, b)| b.is_none())
        .map(|(a, _)| a)
        .collect()
}

/// A door that opens only for a player standing in a tile box (the
/// `BBoxTest` on the player channel ahead of its `31 00`): the box, with
/// the collider centres of the door's leaves. `ropeway` P0[2], the station
/// door, opens for a player inside on tiles (27..30, 33..34) and is a wall
/// from the corridor south of it.
pub(super) type TileBox = (u8, u8, u8, u8);
pub(super) type BoxedDoor = (TileBox, Vec<(i16, i16)>);

pub(super) fn boxed_doors(session: &NativeSession) -> Vec<BoxedDoor> {
    let w = &session.host.world;
    door_openings(session)
        .into_iter()
        .filter_map(|(a, b)| {
            let b = b?;
            let leaves = w
                .props
                .colliders
                .iter()
                .filter(|c| c.anchor == Some(a) && !c.moving_box)
                .filter_map(|c| {
                    Some((
                        i16::try_from(c.center.0).ok()?,
                        i16::try_from(c.center.1).ok()?,
                    ))
                })
                .collect();
            Some((b, leaves))
        })
        .collect()
}

/// Every placed prop whose touch opens it (`31 00` reached from the
/// resume point): `None` when nothing can branch around the opening,
/// `Some(box)` when only a player-channel box test (tiles `x_min, z_min,
/// x_max, z_max`) stands before it.
pub(super) fn door_openings(session: &NativeSession) -> HashMap<(u8, u8), Option<TileBox>> {
    use legaia_engine_field::man_field_scripts::partition_record_span;
    use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker};
    let w = &session.host.world;
    if w.mode != SceneMode::Field || w.props.bank.props.is_empty() {
        return HashMap::new();
    }
    let Some(man) = w.field_vm.channels_man.as_ref() else {
        return HashMap::new();
    };
    let Ok(mf) = legaia_scene_formats::man_section::parse(man) else {
        return HashMap::new();
    };
    // The touch resumes the record past its spawn prologue (the first `21`),
    // or at its first op when it has none (`FUN_8003A55C` runs a prologue
    // only behind the `24` / `25` marker: `chitei2`'s lift-cage doors,
    // partition-0 records 6..11, are a bare `31 00` the touch runs);
    // the door opens itself only when the `31 00` comes before anything
    // that can branch around it, or behind one box test of the player.
    // A story-flag test on the way is followed against the live flags, as
    // the touch runs it: `ropeway2` P0[16], the elevator door, opens only
    // once `0x1D5` (the power cut) is set.
    let opens = |body: &[u8], pc0: usize| -> Option<Option<TileBox>> {
        let mut pc = if matches!(body.get(pc0), Some(0x24 | 0x25)) {
            LinearWalker::new(body, pc0)
                .map_while(Result::ok)
                .find(|i| i.opcode == 0x21)
                .map(|i| i.pc + i.size)?
        } else {
            pc0
        };
        let mut boxed = None;
        let mut seen = HashSet::new();
        while seen.insert(pc) {
            let insn = legaia_scene_formats::field_disasm::decode(body, pc).ok()?;
            if insn.size == 0 {
                return None;
            }
            match insn.info {
                InsnInfo::CFlag {
                    kind: FlagKind::Set,
                    bit: 0,
                } if insn.extended.is_none() => return Some(boxed),
                // A prop that parks itself on the hide tile is out of the
                // way as surely: `vozz` P0[1], the thickets across East
                // Voz's paths, run `23 7F 7F` on the action button once
                // `0x079` is up.
                InsnInfo::MoveTo { xb: 0x7F, zb: 0x7F } if insn.extended.is_none() => {
                    return Some(boxed);
                }
                InsnInfo::BBoxTest {
                    x_min,
                    z_min,
                    x_max,
                    z_max,
                    ..
                } if boxed.is_none() && insn.extended == Some(0xF8) => {
                    boxed = Some((x_min, z_min, x_max, z_max));
                }
                InsnInfo::SystemFlag {
                    kind: FlagKind::Test,
                    idx,
                    target: Some(target),
                    ..
                } => {
                    if w.system_flag_test(idx) {
                        pc = target;
                        continue;
                    }
                }
                InsnInfo::JmpRel { target, .. } => {
                    pc = target;
                    continue;
                }
                InsnInfo::SystemFlag {
                    kind: FlagKind::Test,
                    ..
                }
                | InsnInfo::BBoxTest { .. }
                | InsnInfo::Picker { .. }
                | InsnInfo::SpawnRecord { .. } => return None,
                _ if insn.opcode == 0x21 => return None,
                _ => {}
            }
            pc += insn.size;
        }
        None
    };
    w.props
        .bank
        .props
        .iter()
        .filter_map(|(&a, p)| {
            let (start, pc0, len) = partition_record_span(&mf, man, 0, p.record)?;
            Some((a, opens(&man[start..start + len], pc0)?))
        })
        .collect()
}
