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
                        ([], []) if !forks && !leaves => {
                            walks.last().map(|&(x, z)| cell_of(at(x), at(z)))
                        }
                        _ => None,
                    };
                    if let Some(land) = land
                        && !branches
                    {
                        let (c1, c2) = partition2_record_gates(&mf, &man, rec).unwrap_or_default();
                        hops.push(((i32::from(tr.tile_x), i32::from(tr.tile_z)), land, c1, c2));
                    } else if land.is_none() && forks && !branches && !leaves && !walks.is_empty() {
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
                && let Some(land) = flag_path_landing(w, body, *pc0)
            {
                map.entry(*tile).or_insert(open_landing(w, land));
            }
        }
        map
    })
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
/// makes), and the last player walk leg (`C7 F8 x z`) or arc on it is the
/// landing. `deroa`'s transporter hub P2[11] sends its platform to one of
/// three shaft rooms by the switch flags `0x4EF` / `0x4F0`.
pub(super) fn flag_path_landing(
    w: &legaia_engine_core::world::World,
    body: &[u8],
    pc0: usize,
) -> Option<Cell> {
    use legaia_scene_formats::field_disasm::{ActorCtrlKind, FlagKind, InsnInfo, decode};
    let at = |b: u8| i16::from(b & 0x7F) * 128 + 0x40 + if b & 0x80 != 0 { 0x40 } else { 0 };
    let mut pc = pc0;
    let mut seen = HashSet::new();
    let mut last = None;
    while seen.insert(pc) {
        let &op = body.get(pc)?;
        if op == 0x21 {
            break;
        }
        let insn = decode(body, pc).ok()?;
        if insn.opcode == 0x47
            && insn.extended == Some(0xF8)
            && let (Some(&x), Some(&z)) = (body.get(pc + 2), body.get(pc + 3))
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
                if w.system_flag_test(idx) {
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
            if insn.size == 0 || insn.opcode == 0x21 {
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
