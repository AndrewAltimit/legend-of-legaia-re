//! The beats pass: the talks, door and prop touches a segment plays in a
//! scene before leaving it.

use super::*;

/// Walk-on beat records tried per scene visit.
pub(super) const MAX_BEATS: usize = 120;

/// The **beats** pass of the seated tier: in the loaded field scene, approach
/// every boss stager whose park gate is clear (the touch dispatch runs its
/// placement record) and step onto every gate-1 walk-on tile whose
/// partition-2 record the live flags let spawn and which is not a door. This
/// is how the seated tier plays a story beat a player triggers by walking
/// somewhere; beats that wait on a conversation, a menu or an item stay
/// unplayed. `Ok(Some(scene))` when a beat left the scene.
pub(super) fn play_beats(
    session: &mut NativeSession,
    log: &mut Vec<String>,
) -> Result<Option<String>, String> {
    use legaia_engine_field::man_field_scripts::{boss_stager_placements, partition2_record_gates};
    if session.host.world.mode != SceneMode::Field {
        return Ok(None);
    }
    let name = scene_name(session);
    let Some((mf, man, triggers)) = scene_man_and_triggers(session) else {
        return Ok(None);
    };
    let before: BTreeSet<u16> = flags_of_world(session);
    let mut ran = 0usize;
    let finish = |session: &NativeSession, log: &mut Vec<String>, ran: usize| {
        let gained = flags_of_world(session).difference(&before).count();
        log.push(format!("{name}: {ran} beat(s) run, +{gained} flag(s)"));
    };
    let doors: BTreeSet<u8> = overworld_portal_sites(&mf, &man, &triggers)
        .iter()
        .map(|s| s.record)
        .collect();
    // A tile reaches a record only when no earlier entry claims it: the
    // dispatch takes the first primary-then-fallback match
    // (`FUN_801D5630`), so a fallback band shadowed by a primary entry on
    // the same tile never fires its own record.
    let mut first_tile: BTreeMap<u8, (u8, u8)> = BTreeMap::new();
    // Every tile of each band, for a walk that cannot reach the first.
    let mut band: BTreeMap<u8, Vec<(u8, u8)>> = BTreeMap::new();
    for t in triggers.iter().filter(|t| t.gate == 1) {
        let owner = triggers
            .iter()
            .find(|u| (u.tile_x, u.tile_z) == (t.tile_x, t.tile_z));
        if owner.is_some_and(|u| u.record == t.record && u.gate == 1) {
            first_tile.entry(t.record).or_insert((t.tile_x, t.tile_z));
            band.entry(t.record).or_default().push((t.tile_x, t.tile_z));
        }
    }
    // Talk and walk-on beats unlock each other (a conversation sets the flag
    // a walk-on record's C2 gate needs, and a walk-on cutscene sets the flag
    // a conversation branches on), so both passes repeat while a round still
    // gains flags. Each beat plays at most once per scene visit.
    let mut walked: BTreeSet<u8> = BTreeSet::new();
    let mut touched: BTreeSet<u8> = BTreeSet::new();
    let mut examined: BTreeSet<(u8, u8)> = BTreeSet::new();
    let overreach = overreaching_records(session, &mf, &man, 2);
    // A stager's record is often a staged conversation (`tunnelc` P1[4],
    // Xain: `0x323`, `0x324`, `0x325`, then the fight), each contact
    // playing the next stage, so stagers are approached every round until
    // a contact gains nothing.
    let mut stager_spent: BTreeSet<usize> = BTreeSet::new();
    // A walk-on beat that ran and gained nothing ran on a gate its own
    // script tests and failed (`rayman` P2[18] wants `0x1FD..0x1FF`, which
    // the talks of the same pass raise): it plays again once the flags it
    // ran against have changed, as a player stepping back onto the band.
    // Only a change to a system flag the record itself tests rearms it: a
    // beat whose script reads none of the flags that moved would replay the
    // same run, and replaying a seat-and-release beat walks the player off
    // the route another beat set up.
    let tested: BTreeMap<usize, BTreeSet<u16>> = {
        use legaia_engine_field::man_field_scripts::{FlagBank, walk_partition_gflag_sites};
        let mut m: BTreeMap<usize, BTreeSet<u16>> = BTreeMap::new();
        for site in walk_partition_gflag_sites(&mf, &man, 2) {
            if site.clean && site.bank == FlagBank::System && site.kind == FlagKind::Test {
                m.entry(site.record).or_default().insert(site.flag);
            }
        }
        m
    };
    let mut idle: BTreeMap<u8, BTreeSet<u16>> = BTreeMap::new();
    let mut rearmed: BTreeSet<u8> = BTreeSet::new();
    for _round in 0..BEAT_ROUNDS {
        let round_start = flags_of_world(session);
        let stale: Vec<u8> = idle
            .iter()
            .filter(|(rec, f)| {
                tested.get(&usize::from(**rec)).is_some_and(|t| {
                    f.symmetric_difference(&round_start)
                        .any(|flag| t.contains(flag))
                })
            })
            .map(|(&rec, _)| rec)
            .collect();
        for rec in stale {
            idle.remove(&rec);
            walked.remove(&rec);
            rearmed.insert(rec);
        }
        for p in boss_stager_placements(&mf, &man) {
            if stager_spent.contains(&p.placement_index) {
                continue;
            }
            STAGED_FIGHTS.with(|s| {
                s.borrow_mut()
                    .insert((name.clone(), Some(u16::from(p.formation_row))))
            });
            if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                eprintln!(
                    "    [stager] {name} P1[{}] row {} park {:?} (set {:?}, next anchor {:?}) station {:?} spawn {:?} parked {}",
                    p.placement_index,
                    p.formation_row,
                    p.park_gate_flag,
                    p.park_gate_flag
                        .map(|f| session.host.world.system_flag_test(f)),
                    p.park_gate_flag.map(next_anchor_has),
                    p.station_world,
                    p.spawn_world,
                    p.spawn_parked
                );
            }
            if p.park_gate_flag
                .is_some_and(|f| session.host.world.system_flag_test(f))
            {
                continue;
            }
            if stager_overreaches(&mf, &man, p.placement_index) {
                continue;
            }
            let at = match (p.station_world, p.spawn_parked) {
                (Some(st), _) => st,
                (None, false) => p.spawn_world,
                (None, true) => continue,
            };
            let (tx, tz) = tile_of(at.0, at.1);
            ran += 1;
            if pad_hand() {
                // A player walks into a boss at full strength.
                pad_field_heal(session, 900);
                // Walk up to the stager; its touch dispatch runs off the
                // locomotion step, as it does for a player.
                let avoid = pad_avoid(session, None);
                let walk = pad_walk(session, (tx, tz), &avoid, 1);
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!("    [stager] walk -> {walk:?}");
                }
                match walk {
                    Ok(Walk::Entered(s)) => {
                        finish(session, log, ran);
                        return Ok(Some(s));
                    }
                    Ok(Walk::Arrived) => {
                        let _ = pad_lean(session, move |_| Some(at), 24, |s| !released(s));
                        if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                            eprintln!(
                                "    [stager] leaned: holder {} at {}; touch {:?}; player {:?}",
                                holder(session),
                                park_site(session),
                                session.host.world.props.active_walk_touch,
                                player_xz(session)
                            );
                        }
                    }
                    Err(_) => continue,
                }
            } else {
                session
                    .host
                    .world
                    .seat_player_at_tile(tx.clamp(0, 127) as u8, tz.clamp(0, 127) as u8);
            }
            let f0 = flags_of_world(session);
            let r = run_while_moving(session, DEEP_EXIT_TICKS);
            if flags_of_world(session) == f0 {
                stager_spent.insert(p.placement_index);
            }
            trace_beat(session, &f0, || {
                format!("{name} boss stager P1[{}] -> {r:?}", p.placement_index)
            });
            match r {
                Run::Entered(s) => {
                    finish(session, log, ran);
                    return Ok(Some(s));
                }
                Run::Battle(b) => {
                    return Err(format!("boss stager P1[{}]: {b}", p.placement_index));
                }
                Run::Error(e) => return Err(e),
                Run::Released | Run::Parked(_) => {}
            }
        }
        // A talk is re-tried each round while its flag stays clear: its
        // record may branch on a flag the previous round's beats set.
        for slot in talk_beats(session, &mf, &man) {
            if let Some(w) = wiped(session) {
                return Err(w);
            }
            pad_budget(session)?;
            if ran >= MAX_BEATS {
                continue;
            }
            ran += 1;
            let f0 = flags_of_world(session);
            if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                eprintln!("    [talk] {name} P1[{slot}] ...");
            }
            // A talk that stages a fight (Xain, `tunnelc` P1[4]: his
            // second stage's option 0 runs `3E FF 0A`) is a boss approach:
            // a player walks into it at full strength, as into a stager.
            if pad_hand() && record_stages_fight(&mf, &man, 1, usize::from(slot)) {
                pad_field_heal(session, 900);
            }
            let r = talk_to(session, slot);
            let r = fight_committed(session, r);
            trace_beat(session, &f0, || format!("{name} talk P1[{slot}] -> {r:?}"));
            match r {
                Run::Entered(s) => {
                    finish(session, log, ran);
                    return Ok(Some(s));
                }
                Run::Battle(b) => return Err(format!("talk P1[{slot}]: {b}")),
                Run::Error(e) => return Err(e),
                Run::Released | Run::Parked(_) => {}
            }
        }
        let boss_band_live = first_tile.keys().any(|&rec| {
            record_stages_fight(&mf, &man, 2, usize::from(rec))
                && partition2_record_gates(&mf, &man, usize::from(rec))
                    .is_some_and(|(c1, c2)| session.host.world.p2_record_gates_pass(&c1, &c2))
        });
        for (&rec, &tile) in &first_tile {
            if let Some(w) = wiped(session) {
                return Err(w);
            }
            pad_budget(session)?;
            if !rearmed.contains(&rec)
                && RAN_ON_WALK.with(|t| t.borrow().contains(&(name.clone(), rec)))
            {
                walked.insert(rec);
            }
            if doors.contains(&rec)
                || walked.contains(&rec)
                || overreach.contains(&usize::from(rec))
                || ran >= MAX_BEATS
            {
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() && !walked.contains(&rec) {
                    eprintln!(
                        "    [beat-skip] {name} P2[{rec}] door {} overreach {}",
                        doors.contains(&rec),
                        overreach.contains(&usize::from(rec))
                    );
                }
                continue;
            }
            if record_stages_fight(&mf, &man, 2, usize::from(rec))
                && record_overreaches(&mf, &man, 2, usize::from(rec), true)
            {
                continue;
            }
            let pass = partition2_record_gates(&mf, &man, usize::from(rec))
                .is_some_and(|(c1, c2)| session.host.world.p2_record_gates_pass(&c1, &c2));
            if !pass {
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!(
                        "    [beat-skip] {name} P2[{rec}] gates {:?}",
                        partition2_record_gates(&mf, &man, usize::from(rec))
                    );
                }
                continue;
            }
            if pad_hand() && reach_beat_waits(session, &mf, &man, usize::from(rec)) {
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!("    [beat-skip] {name} P2[{rec}] waits on the other reach beats");
                }
                continue;
            }
            walked.insert(rec);
            let f0 = flags_of_world(session);
            ran += 1;
            // A walk-on that stages a fight (`jagaroom` P2[9], Van Saryu) is
            // a boss approach like a stager's: a player tops the party up
            // before stepping on. So is any walk in a scene where such a band
            // is live, since the route to another beat can cross it: the
            // walk to `jagaroom` P2[4] runs over P2[9]'s tiles.
            if pad_hand() && boss_band_live {
                pad_field_heal(session, 900);
            }
            PLAYING_BEAT.with(|b| b.set(Some(rec)));
            let r = if pad_hand() {
                // A band is often several tiles wide and walled on some of
                // them (`retockin` P2[42], Lord Saryu's audience, spans
                // (104..107, 50) and (107, 50) is boxed in): a walk that
                // cannot reach its first tile tries the rest, nearest first.
                let (px, pz) = player_xz(session);
                let me = dispatch_tile(px, pz);
                let mut tiles: Vec<(u8, u8)> = band
                    .get(&rec)
                    .map(|v| v.iter().copied().filter(|&t| t != tile).collect())
                    .unwrap_or_default();
                tiles.sort_by_key(|&(x, z)| {
                    (i32::from(x) - me.0).abs() + (i32::from(z) - me.1).abs()
                });
                tiles.insert(0, tile);
                let mut r = Run::Parked("band has no tile".into());
                // A band out of the leader's walk component may be in another
                // party member's: inside an armed three-actor talk (`nilboa`'s
                // Three Tunnels, `43 02`) each member stands in its own tunnel
                // and Square hands control to the next ([`swap_leader`]).
                let mut swaps = 0;
                let mut queue: VecDeque<(u8, u8)> = tiles.iter().copied().collect();
                while let Some(t) = queue.pop_front() {
                    pad_budget(session)?;
                    r = match pad_step_onto(session, t) {
                        Ok(Walk::Entered(s)) => Run::Entered(s),
                        Ok(Walk::Arrived) => run_while_moving(session, DEEP_EXIT_TICKS),
                        Err(e) => Run::Parked(e),
                    };
                    // Retried only when the tile itself is the obstacle: no
                    // path to it at all, or a walk that stalled at its edge.
                    // A walk that stalled far off is the route's problem, and
                    // the next tile would only walk it again.
                    let short = |e: &str| {
                        e.split(" tiles short")
                            .next()
                            .and_then(|h| h.rsplit(' ').next().and_then(|n| n.parse::<u32>().ok()))
                    };
                    let unreached = matches!(&r, Run::Parked(e)
                        if e.contains("no walkable path")
                            || (e.contains("pad walk stalled") && short(e).is_some_and(|n| n <= 2)));
                    // A room-marker band crossed on the way may raise a flag
                    // (`0x52A`); only the record's own run ends the search -
                    // which may have come from another band on the way
                    // (`nilboa` P2[29] spawns the split, P2[0]).
                    if !unreached || RAN_ON_WALK.with(|t| t.borrow().contains(&(name.clone(), rec)))
                    {
                        break;
                    }
                    if queue.is_empty() && swaps < 2 && swap_leader(session) {
                        swaps += 1;
                        queue.extend(tiles.iter().copied());
                    }
                }
                r
            } else {
                // A band that stages a fight is a boss's door: the seated
                // run tops a party not near full up first, as the pad walk
                // does before such a band. Koru (`nilboa` P2[20], see
                // [`KORU_MONSTER_ID`]) is met straight off the tunnel beats
                // at 50..80% and is a race no heal turn can be spared in.
                if party_hp_permille(session) < 900
                    && fight_band_tiles(session).contains(&(i32::from(tile.0), i32::from(tile.1)))
                {
                    pad_field_heal(session, 1000);
                }
                step_onto(session, tile);
                run_while_moving(session, DEEP_EXIT_TICKS)
            };
            PLAYING_BEAT.with(|b| b.set(None));
            let r = fight_committed(session, r);
            // A band no member could walk to stays on the list: a later
            // beat may open the way (`nilboa` P2[39], the warp at the far
            // end of one tunnel, drops its member beside the P2[2] band).
            if matches!(&r, Run::Parked(e) if e.contains("no walkable path")) {
                walked.remove(&rec);
            }
            rearmed.remove(&rec);
            if matches!(r, Run::Released) && flags_of_world(session) == f0 {
                idle.insert(rec, f0.clone());
            }
            trace_beat(session, &f0, || {
                format!("{name} walk P2[{rec}] at {tile:?} -> {r:?}")
            });
            match r {
                Run::Entered(s) => {
                    finish(session, log, ran);
                    return Ok(Some(s));
                }
                Run::Battle(b) => return Err(format!("beat P2[{rec}]: {b}")),
                Run::Error(e) => return Err(e),
                Run::Released | Run::Parked(_) => {}
            }
        }
        // Object doors whose live arm spawns a story beat: a `.MAP`-bound
        // partition-0 door record branches on the story flags, and one arm
        // runs op `0x44` instead of the teleport (`town01` P0[29], Vahn's
        // front door, spawns the P2[5] night beat that sets `0x227` once
        // `0x226` is up and `0x227` is not).
        for (slot, contact) in object_door_beats(session, &mf, &man) {
            if let Some(w) = wiped(session) {
                return Err(w);
            }
            pad_budget(session)?;
            if !touched.insert(slot) || ran >= MAX_BEATS {
                continue;
            }
            ran += 1;
            let flat = session
                .host
                .world
                .props
                .walk_touch_records
                .get(&slot)
                .copied();
            let (tx, tz) = tile_of(contact.0, contact.1);
            session.host.world.set_pad(0);
            if pad_hand() {
                // Walk up to the door and lean on it: the locomotion's own
                // contact probe posts the touch.
                let f0 = flags_of_world(session);
                let avoid = pad_avoid(session, None);
                let mut r = Run::Parked(format!("no approach to door object {slot}"));
                // Up to the door tile itself first (its box may leave the
                // tile only partly solid), then from each side.
                let approaches: Vec<((i16, i16), i32)> = std::iter::once(((tx, tz), 1))
                    .chain(beside(session, (tx, tz)).into_iter().map(|t| (t, 0)))
                    .collect();
                for (side, within) in approaches {
                    match pad_walk(session, side, &avoid, within) {
                        Ok(Walk::Entered(s)) => {
                            r = Run::Entered(s);
                            break;
                        }
                        Ok(Walk::Arrived) => {}
                        Err(e) => {
                            r = Run::Parked(e);
                            continue;
                        }
                    }
                    let started = |s: &NativeSession| {
                        s.host.world.props.active_walk_touch == Some(slot) || !released(s)
                    };
                    if let Some(s) = pad_lean(session, move |_| Some(contact), 32, started) {
                        r = Run::Entered(s);
                        break;
                    }
                    if started(session) {
                        r = run_while_moving(session, DEEP_EXIT_TICKS);
                        break;
                    }
                    r = Run::Parked(format!("door object {slot} posted no touch"));
                }
                trace_beat(session, &f0, || {
                    format!("{name} door object {slot} (pad) -> {r:?}")
                });
                match r {
                    Run::Entered(s) => {
                        finish(session, log, ran);
                        return Ok(Some(s));
                    }
                    Run::Battle(b) => return Err(format!("door object {slot}: {b}")),
                    Run::Error(e) => return Err(e),
                    Run::Released | Run::Parked(_) => {}
                }
                continue;
            }
            session.host.world.props.active_walk_touch = None;
            session
                .host
                .world
                .seat_player_at_tile(tx.clamp(0, 127) as u8, tz.clamp(0, 127) as u8);
            let f0 = flags_of_world(session);
            // The touch dispatch runs from the locomotion step, so a seat
            // alone posts nothing: nudge the pad until the contact posts
            // (the stand-inside probe fires on the first stepped frame).
            let mut entered = None;
            for dir in [
                PadButton::Up,
                PadButton::Down,
                PadButton::Left,
                PadButton::Right,
            ] {
                if session.host.world.props.active_walk_touch == Some(slot) {
                    break;
                }
                session.host.world.set_pad(dir.mask());
                if let Ok(SceneTickEvent::SceneEntered { name }) = session.tick() {
                    entered = Some(name);
                    break;
                }
            }
            session.host.world.set_pad(0);
            let r = match entered {
                Some(s) => Run::Entered(s),
                None => run_while_moving(session, DEEP_EXIT_TICKS),
            };
            trace_beat(session, &f0, || {
                format!("{name} door object {slot} (flat record {flat:?}) at {contact:?} -> {r:?}")
            });
            match r {
                Run::Entered(s) => {
                    finish(session, log, ran);
                    return Ok(Some(s));
                }
                Run::Battle(b) => return Err(format!("door object {slot}: {b}")),
                Run::Error(e) => return Err(e),
                Run::Released | Run::Parked(_) => {}
            }
        }
        // Props whose own script writes a wanted flag. An interact-gated
        // one (a switch) is examined with Cross, never by contact (`chitei2`
        // P0[33] raises the `0x4F0` that opens the P2[11] walk-on setting
        // `0x470`); a touch-class one (a door) is walked into.
        for (anchor, at, gated) in prop_beats(session, &mf, &man) {
            if !examined.insert(anchor) || ran >= MAX_BEATS {
                continue;
            }
            ran += 1;
            let f0 = flags_of_world(session);
            let r = if gated {
                examine_prop(session, anchor, at)
            } else {
                touch_prop(session, anchor, at)
            };
            let r = fight_committed(session, r);
            trace_beat(session, &f0, || format!("{name} prop {anchor:?} -> {r:?}"));
            match r {
                Run::Entered(s) => {
                    finish(session, log, ran);
                    return Ok(Some(s));
                }
                Run::Battle(b) => return Err(format!("prop {anchor:?}: {b}")),
                Run::Error(e) => return Err(e),
                Run::Released | Run::Parked(_) => {}
            }
        }
        if flags_of_world(session) == round_start {
            break;
        }
    }
    finish(session, log, ran);
    Ok(None)
}

/// The `.MAP` object doors of the loaded field scene (walk-touch slot and
/// contact centre) whose record, resolved against the live flags, spawns a
/// partition-2 record that - itself or through what it spawns - cleanly
/// SETs a still-clear flag the next anchor carries.
pub(super) fn object_door_beats(
    session: &NativeSession,
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
) -> Vec<(u8, (i16, i16))> {
    use legaia_engine_field::man_field_scripts::{
        FlagBank, WalkTouchEvent, resolve_walk_touch_event, walk_partition_gflag_sites,
    };
    let w = &session.host.world;
    let wanted = |part: usize| -> BTreeSet<usize> {
        walk_partition_gflag_sites(mf, man, part)
            .iter()
            .filter(|s| {
                s.bank == FlagBank::System
                    && s.kind == FlagKind::Set
                    && s.clean
                    && !s.text_alias
                    && !s.debug_menu
                    && !w.system_flag_test(s.flag)
                    && next_anchor_has(s.flag)
            })
            .map(|s| s.record)
            .collect()
    };
    let setters0 = wanted(0);
    let setters2 = wanted(2);
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let mut out = Vec::new();
    for (&slot, &flat) in &w.props.walk_touch_records {
        let Some(&(contact, _)) = w.props.walk_touch.get(&slot) else {
            continue;
        };
        let live = resolve_walk_touch_event(mf, man, flat, &|f| w.system_flag_test(f));
        let Some(WalkTouchEvent::SpawnRecord { flat_index }) = live else {
            continue;
        };
        let Some(r2) = flat_index.checked_sub(n0 + n1) else {
            continue;
        };
        // The door's own record may latch the beat as it spawns it
        // (`town0d` P0[1] sets `0x3B9` and spawns the P2[28] song-night
        // chain, whose own writes are all inside its dialogue).
        if (flat < n0 && setters0.contains(&flat))
            || setters2.contains(&r2)
            || spawned_p2(mf, man, 2, r2, n0 + n1, 3)
                .iter()
                .any(|r| setters2.contains(r))
        {
            out.push((slot, contact));
        }
    }
    out
}

/// A prop beat: anchor tile, contact centre, and whether it is the
/// interact-gated (examine) class rather than the touch (door) class.
pub(super) type PropBeat = ((u8, u8), (i32, i32), bool);

/// The props of the loaded field scene whose own bind record cleanly SETs a
/// still-clear flag the next anchor carries, or spawns a partition-2 record
/// that does, itself or through what it spawns (op `0x44`, three levels).
/// `rikuroa` P0[2], the Genesis Tree, is examined after Caruban: its body
/// spawns P2[53], which raises `0x28A` and `0x2C2` and carries the party
/// down to `map01`, where the latter plays the tree's revival. A prop whose
/// record overreaches (sets a latch the next anchor lacks) is left alone.
pub(super) fn prop_beats(
    session: &NativeSession,
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
) -> Vec<PropBeat> {
    use legaia_engine_field::man_field_scripts::{FlagBank, walk_partition_gflag_sites};
    let w = &session.host.world;
    let wanted = |part: usize| -> BTreeSet<usize> {
        walk_partition_gflag_sites(mf, man, part)
            .iter()
            .filter(|s| {
                s.bank == FlagBank::System
                    && s.kind == FlagKind::Set
                    && s.clean
                    && !s.text_alias
                    && !s.debug_menu
                    && !w.system_flag_test(s.flag)
                    && next_anchor_has(s.flag)
            })
            .map(|s| s.record)
            .collect()
    };
    let setters0 = wanted(0);
    let setters2 = wanted(2);
    let over0 = overreaching_records(session, mf, man, 0);
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let sets_wanted = |rec: usize| {
        setters0.contains(&rec)
            || (!over0.contains(&rec)
                && spawned_p2(mf, man, 0, rec, n0 + n1, 3)
                    .iter()
                    .any(|r| setters2.contains(r)))
    };
    w.props
        .bank
        .props
        .iter()
        .filter(|(_, p)| !p.collision_exempt())
        .filter(|(_, p)| sets_wanted(p.record))
        .map(|(&a, p)| {
            let at = if p.moving_box() { p.world } else { p.collider };
            (a, at, p.interact_gated())
        })
        .collect()
}

/// Walk into the touch-class prop anchored at `anchor` (a door: contact
/// result bit `4`, posted by the movement probe with no button): from each
/// tile beside it, hold the pad toward it until its record starts, then let
/// the run play out. `town0d` P0[1], the house door, sets `0x3B9` and spawns
/// the P2[28] song-night chain the first time it is opened.
pub(super) fn touch_prop(session: &mut NativeSession, anchor: (u8, u8), at: (i32, i32)) -> Run {
    let (tx, tz) = tile_of(at.0 as i16, at.1 as i16);
    let prop_run = |s: &NativeSession| {
        s.host
            .world
            .dialog
            .inline
            .as_ref()
            .is_some_and(|id| id.prop_anchor == Some(anchor))
    };
    for (dx, dz) in [(-1i16, 0i16), (1, 0), (0, -1), (0, 1)] {
        let (sx, sz) = (tx + dx, tz + dz);
        if !(0..128).contains(&sx) || !(0..128).contains(&sz) {
            continue;
        }
        session.host.world.set_pad(0);
        session.host.world.seat_player_at_tile(sx as u8, sz as u8);
        let mut started = false;
        for _ in 0..40 {
            let pad = pad_for_step(session, -dx, -dz);
            session.host.world.set_pad(pad);
            match session.tick() {
                Ok(SceneTickEvent::SceneEntered { name }) => {
                    session.host.world.set_pad(0);
                    return Run::Entered(name);
                }
                Ok(_) => {}
                Err(e) => return Run::Error(format!("{e:#}")),
            }
            if prop_run(session) {
                started = true;
                break;
            }
        }
        session.host.world.set_pad(0);
        if started {
            return run_while_moving(session, DEEP_EXIT_TICKS);
        }
    }
    Run::Released
}

/// With `LEGAIA_FGL_TRACE` set, print one line per played beat: what ran,
/// how it ended, and the flags it set.
pub(super) fn trace_beat(
    session: &NativeSession,
    before: &BTreeSet<u16>,
    what: impl FnOnce() -> String,
) {
    if std::env::var_os("LEGAIA_FGL_TRACE").is_none() {
        return;
    }
    let gained: Vec<String> = flags_of_world(session)
        .difference(before)
        .map(|f| format!("0x{f:03X}"))
        .collect();
    let (px, pz) = player_xz(session);
    eprintln!(
        "    [beat] {} +{gained:?} (ends in {} {:?} at {:?} ({px},{pz}); frame {})",
        what(),
        scene_name(session),
        session.host.world.mode,
        tile_of(px, pz),
        session.frames
    );
}

thread_local! {
    /// The milestone a beats pass is played for, while that pass runs in
    /// the milestone's own scene with every waypoint behind it.
    pub(super) static BEAT_TARGET: std::cell::RefCell<Option<Milestone>> =
        const { std::cell::RefCell::new(None) };
}

thread_local! {
    /// The system flags the segment's next anchor carries, when it has one:
    /// what the retail run had set by the time it got there.
    pub(super) static NEXT_ANCHOR_FLAGS: std::cell::RefCell<Option<BTreeSet<u16>>> =
        const { std::cell::RefCell::new(None) };
}

/// Rounds of the talk + walk-on beat passes per scene visit.
pub(super) const BEAT_ROUNDS: usize = 3;

/// The **story talks** of the loaded field scene: the talk-NPC placements
/// (partition-1 record index = the slot the interact probe addresses) whose
/// own record carries a clean, non-debug SET of a system flag the live bank
/// still has clear. A conversation that writes no story flag is scenery and
/// is not played.
pub(super) fn talk_beats(
    session: &NativeSession,
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
) -> Vec<u8> {
    use legaia_engine_field::man_field_scripts::{FlagBank, walk_partition_gflag_sites};
    let w = &session.host.world;
    let wanted = |s: &legaia_engine_field::man_field_scripts::GFlagSite| {
        s.bank == FlagBank::System
            && s.kind == FlagKind::Set
            && s.clean
            && !s.text_alias
            && !s.debug_menu
            && !w.system_flag_test(s.flag)
            && next_anchor_has(s.flag)
    };
    let setters1: BTreeSet<usize> = walk_partition_gflag_sites(mf, man, 1)
        .iter()
        .filter(|s| wanted(s))
        .map(|s| s.record)
        .collect();
    let setters2: BTreeSet<usize> = walk_partition_gflag_sites(mf, man, 2)
        .iter()
        .filter(|s| wanted(s))
        .map(|s| s.record)
        .collect();
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let mut slots: Vec<u8> = w
        .npcs
        .positions
        .keys()
        .copied()
        .filter(|s| w.npcs.dialog.contains_key(s) || w.npcs.dialog_prologue.contains_key(s))
        // A venue cabinet's record runs the minigame door-warp (`3E` with
        // `op0 >= 100`): `balden` P1[24], Vidna's slot machine, raises
        // `0x5D2` on the way in. Its flags are the minigame's, not the
        // story's, and the hand does not play minigames.
        .filter(|&s| !record_enters_minigame(mf, man, 1, usize::from(s)))
        .filter(|&s| {
            // The talk's own record writes a wanted flag, or a partition-2
            // record it spawns does (`station`'s ticket seller spawns the
            // P2[19] departure that latches `0x36B`), or it hands a flag to
            // another scene's entry script (`suimon`'s water gate).
            setters1.contains(&usize::from(s))
                || spawned_p2(mf, man, 1, usize::from(s), n0 + n1, 3)
                    .iter()
                    .any(|r| setters2.contains(r))
                || hands_off_to_entry_script(session, mf, man, usize::from(s))
        })
        .collect();
    slots.sort_unstable();
    if pad_hand() {
        // A walking player talks to the nearest first: every tile walked is
        // an encounter roll.
        let (px, pz) = player_xz(session);
        let me = dispatch_tile(px, pz);
        slots.sort_by_key(|s| {
            w.npcs.positions.get(s).map_or(i32::MAX, |&(x, z)| {
                let t = dispatch_tile(x, z);
                (t.0 - me.0).abs() + (t.1 - me.1).abs()
            })
        });
    }
    slots
}
