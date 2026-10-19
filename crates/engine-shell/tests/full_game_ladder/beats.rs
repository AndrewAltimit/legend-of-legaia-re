//! The beats pass: the talks, door and prop touches a segment plays in a
//! scene before leaving it.

use super::*;

/// Walk-on beat records tried per scene visit.
pub(super) const MAX_BEATS: usize = 120;

/// Touches one prop beat may take in a row while it turns like a dial
/// ([`play_beats`]): a `stone` statue's four facings and two touches that
/// land while it is still turning.
const DIAL_TURNS: usize = 6;

/// Idle ticks between two turns of a dial.
const DIAL_SETTLE_TICKS: usize = 90;

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
    // A scene whose story waits behind a staged fight on a walk-on band
    // (`dohaty` P2[10], Dohati) is crossed for that fight: the continuous
    // hand runs from the scene's random encounters on the way, as a player
    // does who means to meet the boss with what the party has
    // ([`continuous::SAVING_FOR_BOSS`]).
    let _saving = continuous::SavingForBoss::arm(
        pad_hand()
            && first_tile.keys().any(|&rec| {
                record_stages_fight(&mf, &man, 2, usize::from(rec))
                    && partition2_record_gates(&mf, &man, usize::from(rec))
                        .is_some_and(|(c1, c2)| session.host.world.p2_record_gates_pass(&c1, &c2))
            }),
    );
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
    // Box-polled records stood in, with the flags they were stood in under.
    let mut boxes_stood: BTreeSet<(u8, BTreeSet<u16>)> = BTreeSet::new();
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
    // The continuous hand's own pass (a scene played before any door has
    // refused) walks only the bands that carry the story: a record that,
    // itself or through what it spawns, raises a still-clear flag the
    // anchors ahead carry and that nothing in the scene lowers again. A
    // room marker is raised on the way in and cleared on the way out, and
    // crossing a dungeon for one costs fights and buys nothing.
    let story_bands: Option<BTreeSet<usize>> = PROACTIVE.with(std::cell::Cell::get).then(|| {
        use legaia_engine_field::man_field_scripts::{FlagBank, walk_partition_gflag_sites};
        let w = &session.host.world;
        let sites: Vec<_> = (0..3)
            .flat_map(|part| walk_partition_gflag_sites(&mf, &man, part))
            .collect();
        let lowered: BTreeSet<u16> = sites
            .iter()
            .filter(|s| s.bank == FlagBank::System && s.kind == FlagKind::Clear && s.clean)
            .map(|s| s.flag)
            .collect();
        let setters: BTreeSet<usize> = walk_partition_gflag_sites(&mf, &man, 2)
            .iter()
            .filter(|s| {
                s.bank == FlagBank::System
                    && s.kind == FlagKind::Set
                    && s.clean
                    && !s.text_alias
                    && !s.debug_menu
                    && !w.system_flag_test(s.flag)
                    && next_anchor_has(s.flag)
                    && !lowered.contains(&s.flag)
            })
            .map(|s| s.record)
            .collect();
        let n0 = mf.partitions.first().map_or(0, Vec::len);
        let n1 = mf.partitions.get(1).map_or(0, Vec::len);
        first_tile
            .keys()
            .map(|&r| usize::from(r))
            .filter(|&r| {
                setters.contains(&r)
                    || spawned_p2(&mf, &man, 2, r, n0 + n1, 3)
                        .iter()
                        .any(|s| setters.contains(s))
            })
            .collect()
    });
    let mut idle: BTreeMap<u8, BTreeSet<u16>> = BTreeMap::new();
    let mut rearmed: BTreeSet<u8> = BTreeSet::new();
    // The continuous hand stays while a round still gains: a staged talk
    // (`cave01` P1[2], Terra's seven-stage `0x158`..`0x15E` chain) plays one
    // stage a contact.
    let rounds = if continuous::PLAYS_ON.with(std::cell::Cell::get) {
        BEAT_ROUNDS * 4
    } else {
        BEAT_ROUNDS
    };
    // Every flag the pass has seen up: the continuous hand's extra rounds
    // go on only while a round raises one it has not seen before. A room
    // marker toggles with every walk across the scene (`bylon`'s `0x528` /
    // `0x529`), and a round that only moved those gained nothing.
    let mut seen = flags_of_world(session);
    for round in 0..rounds {
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
                if let Some(s) = pad_ready(session)? {
                    finish(session, log, ran);
                    return Ok(Some(s));
                }
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
        // record may branch on a flag the previous round's beats set. The
        // continuous hand re-tries a dry talk at once when a later talk of
        // the same round raises a flag, before the round's walk-ons can
        // carry the party elsewhere: `doman`'s P1[3] has nothing to say
        // until Dr. Usha (P1[15]) has raised `0x379`, and the walk-on after
        // her talk rides the lift up to the street, a walk component and a
        // code lock away from P1[3].
        let mut talks: VecDeque<u8> = talk_beats(session, &mf, &man).into();
        let mut dry: Vec<u8> = Vec::new();
        let mut requeued = 0;
        while let Some(slot) = talks.pop_front() {
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
            if pad_hand()
                && record_stages_fight(&mf, &man, 1, usize::from(slot))
                && let Some(s) = pad_ready(session)?
            {
                finish(session, log, ran);
                return Ok(Some(s));
            }
            if pad_hand() && continuous::PLAYS_ON.with(std::cell::Cell::get) {
                hold_talk_box(session, &mf, &man, slot);
            }
            let r = talk_to(session, slot);
            let r = fight_committed(session, r);
            if matches!(r, Run::Released) && flags_of_world(session) == f0 {
                note_dry_talk(session, slot);
                dry.push(slot);
            } else if pad_hand()
                && continuous::PLAYS_ON.with(std::cell::Cell::get)
                && flags_of_world(session) != f0
                && requeued < 4
            {
                let live: Vec<u8> = dry
                    .iter()
                    .copied()
                    .filter(|&d| !talk_ran_dry(session, d) && !talks.contains(&d))
                    .collect();
                dry.retain(|d| !live.contains(d));
                for d in live.into_iter().rev() {
                    requeued += 1;
                    talks.push_front(d);
                }
            }
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
        // A partition-1 record that polls a box of the player's tiles is a
        // walk-on of its own: `doman` P1[18], Dr. Usha's intercom, runs
        // while the player stands on (40..41, 81), and the continuous hand
        // stands there for what it raises (the password's `0x36E`, the
        // lift's `0x371`).
        // Set when a band the box called carried the party off: the round's
        // walk-ons wait for the next round, after its talks. Walked straight
        // after the lift ride, `doman` P2[1] rides the party back up before
        // Dr. Usha is spoken to.
        let mut carried = false;
        if pad_hand() && continuous::PLAYS_ON.with(std::cell::Cell::get) {
            for (slot, tiles) in box_poll_beats(session, &mf, &man) {
                if let Some(w) = wiped(session) {
                    return Err(w);
                }
                pad_budget(session)?;
                if ran >= MAX_BEATS || !boxes_stood.insert((slot, flags_of_world(session))) {
                    continue;
                }
                ran += 1;
                let f0 = flags_of_world(session);
                let mut r = talk_to(session, slot);
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!("    [box] {name} P1[{slot}] talk -> {r:?}");
                }
                for &t in &tiles {
                    r = match pad_walk(
                        session,
                        (i16::from(t.0), i16::from(t.1)),
                        &pad_avoid(session, Some((i16::from(t.0), i16::from(t.1)))),
                        0,
                    ) {
                        Ok(Walk::Entered(s)) => Run::Entered(s),
                        Ok(Walk::Arrived) => {
                            // The box test reads the actor's own tile,
                            // `(x - 0x40) >> 7` (`FUN_8003A55C` `+0x8C`), half
                            // a tile off the dispatch tile the walk arrived
                            // in: `doman`'s intercom box (40..41, 81) holds a
                            // player at x 5126 outside it.
                            match center_on_box_tile(session, t) {
                                Some(s) => Run::Entered(s),
                                None => {
                                    let fa = flags_of_world(session);
                                    let r = run_while_moving(session, DEEP_EXIT_TICKS);
                                    // A record parked on its box test runs
                                    // when it is spoken to from inside the
                                    // box: `doman` P1[18] takes the password
                                    // from a player in (40..41, 81) facing
                                    // the intercom at (41, 83).
                                    if matches!(r, Run::Released) && flags_of_world(session) == fa {
                                        talk_from_box(session, slot, t).unwrap_or(r)
                                    } else {
                                        r
                                    }
                                }
                            }
                        }
                        Err(e) => Run::Parked(e),
                    };
                    if !matches!(&r, Run::Parked(e) if e.contains("no walkable path")) {
                        break;
                    }
                }
                trace_beat(session, &f0, || format!("{name} box P1[{slot}] -> {r:?}"));
                // What the box called is taken at once: the lift band P2[7]
                // waits on the `0x371` the intercom raises, and P2[3], met
                // first in record order, sends the lift back up (clears it).
                if matches!(r, Run::Released) {
                    let gained: BTreeSet<u16> =
                        flags_of_world(session).difference(&f0).copied().collect();
                    for rec in bands_testing(&mf, &man, &gained) {
                        let Some(mut tiles) = band.get(&rec).cloned() else {
                            continue;
                        };
                        let (px, pz) = player_xz(session);
                        let me = dispatch_tile(px, pz);
                        tiles.sort_by_key(|&(x, z)| {
                            (i32::from(x) - me.0).abs() + (i32::from(z) - me.1).abs()
                        });
                        let t = tiles[0];
                        let f1 = flags_of_world(session);
                        r = Run::Parked("no tile".into());
                        for &t in &tiles {
                            r = match pad_step_onto(session, t) {
                                Ok(Walk::Entered(s)) => Run::Entered(s),
                                Ok(Walk::Arrived) => run_while_moving(session, DEEP_EXIT_TICKS),
                                Err(e) => Run::Parked(e),
                            };
                            // A ride that carried the party off is the beat
                            // played, whatever the walk made of it.
                            let (px, pz) = player_xz(session);
                            let at = dispatch_tile(px, pz);
                            let near =
                                (at.0 - i32::from(t.0)).abs() + (at.1 - i32::from(t.1)).abs() <= 2;
                            if !near {
                                carried = true;
                            }
                            if !matches!(r, Run::Parked(_)) || !near {
                                break;
                            }
                        }
                        trace_beat(session, &f1, || {
                            format!(
                                "{name} walk P2[{rec}] at {t:?} (called by P1[{slot}]) -> {r:?}"
                            )
                        });
                        if !matches!(r, Run::Released | Run::Parked(_)) {
                            break;
                        }
                    }
                }
                match r {
                    Run::Entered(s) => {
                        finish(session, log, ran);
                        return Ok(Some(s));
                    }
                    Run::Battle(b) => return Err(format!("box P1[{slot}]: {b}")),
                    Run::Error(e) => return Err(e),
                    Run::Released | Run::Parked(_) => {}
                }
            }
        }
        // A split party's gate stones come before its walk-on bands: the
        // band at the end of a member's tunnel ends that member's part, and
        // the swap passes it by from then on. `nilboa`'s Shield Control
        // Panel (P0[6], `0x458`) and the snowball over the gap (P0[10],
        // `0x475`) stand in the Wind Ravine member's tunnel and each clear
        // the walls in another member's; walked after P2[2], that member's
        // tunnel end, neither is reached again.
        if continuous::PLAYS_ON.with(std::cell::Cell::get)
            && session.host.world.dialog.three_actor_talk.is_some()
            && let Some(s) =
                play_prop_beats(session, &mf, &man, &name, &mut examined, &mut ran, true)?
        {
            finish(session, log, ran);
            return Ok(Some(s));
        }
        let boss_band_live = first_tile.keys().any(|&rec| {
            record_stages_fight(&mf, &man, 2, usize::from(rec))
                && partition2_record_gates(&mf, &man, usize::from(rec))
                    .is_some_and(|(c1, c2)| session.host.world.p2_record_gates_pass(&c1, &c2))
        });
        for (&rec, &tile) in &first_tile {
            if carried {
                break;
            }
            if let Some(w) = wiped(session) {
                return Err(w);
            }
            pad_budget(session)?;
            if !rearmed.contains(&rec)
                && RAN_ON_WALK.with(|t| t.borrow().contains(&(name.clone(), rec)))
            {
                walked.insert(rec);
            }
            if story_bands
                .as_ref()
                .is_some_and(|b| !b.contains(&usize::from(rec)))
            {
                continue;
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
            // A band the planner already walks through as a link (a ledge
            // hop, a lift) whose record raises nothing the world lacks is a
            // way across, not a beat: `rikuroa` P2[45] hops the terrace down
            // once its `0x282` is up and P2[60] hops it back up, and a hand
            // that stepped onto one as a beat came back across the other,
            // ending the walk on whichever side the last hop left it.
            if pad_hand() && band_is_bare_link(session, &mf, &man, rec, &band) {
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!("    [beat-skip] {name} P2[{rec}] is a way across");
                }
                continue;
            }
            if continuous::PLAYS_ON.with(std::cell::Cell::get)
                && band_is_closer(&mf, &man, usize::from(rec))
            {
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!("    [beat-skip] {name} P2[{rec}] is a closer");
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
            if pad_hand()
                && boss_band_live
                && let Some(s) = pad_ready(session)?
            {
                finish(session, log, ran);
                return Ok(Some(s));
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
                let fight_band = record_stages_fight(&mf, &man, 2, usize::from(rec));
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
                    // A boss's band outside a split party is tried from its
                    // other tiles from a little further off: `nilboa`'s Koru
                    // ledge (P2[20]) is walled on the side the route comes
                    // from, and its walk stalls three tiles short at
                    // (98, 107); the band is entered from the south.
                    // The continuous hand alone: the segmented passes are
                    // scored on the walks they were tuned on.
                    let edge = if fight_band
                        && session.host.world.dialog.three_actor_talk.is_none()
                        && continuous::PLAYS_ON.with(std::cell::Cell::get)
                    {
                        3
                    } else {
                        2
                    };
                    let unreached = matches!(&r, Run::Parked(e)
                        if e.contains("no walkable path")
                            || (e.contains("pad walk stalled") && short(e).is_some_and(|n| n <= edge)));
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
        if let Some(s) = play_prop_beats(session, &mf, &man, &name, &mut examined, &mut ran, false)?
        {
            finish(session, log, ran);
            return Ok(Some(s));
        }
        let now = flags_of_world(session);
        let fresh = now.difference(&seen).next().is_some();
        seen.extend(now.iter().copied());
        if now == round_start || (round + 1 >= BEAT_ROUNDS && !fresh) {
            break;
        }
    }
    finish(session, log, ran);
    Ok(None)
}

/// One pass over the scene's prop beats ([`prop_beats`]). With
/// `stones_only` it plays only the gate stones (props whose touch clears
/// walls, `4C 70`) the continuous hand walks to: the pass a split party
/// takes ahead of its walk-on bands ([`play_beats`]). Returns the scene a
/// prop carried the party into.
fn play_prop_beats(
    session: &mut NativeSession,
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    name: &str,
    examined: &mut BTreeSet<(u8, u8)>,
    ran: &mut usize,
    stones_only: bool,
) -> Result<Option<String>, String> {
    for (anchor, at, gated) in prop_beats(session, mf, man) {
        // The continuous hand examines only what it can walk to, and
        // leaves the rest for a round that stands somewhere else.
        // A gate stone (its touch clears walls) is examined only from
        // where the party can walk, and not at all once its gate stands
        // open: the continuous hand plays `korb3`'s maze in order.
        let walked = gated
            && continuous::PLAYS_ON.with(std::cell::Cell::get)
            && prop_wall_clears(session, anchor).is_some();
        if stones_only && !walked {
            continue;
        }
        // Inside an armed three-actor talk the stone may stand in another
        // member's part of the map: Square hands control on until a member
        // can walk to it, as for a walk-on band.
        if walked
            && prop_wall_clears(session, anchor) == Some(false)
            && session.host.world.dialog.three_actor_talk.is_some()
        {
            let seat = (at.0 as i16, at.1 as i16);
            let mut swaps = 0;
            while interact_seats(session, seat, true).is_empty()
                && swaps < 2
                && swap_leader(session)
            {
                swaps += 1;
            }
        }
        if walked
            && (interact_seats(session, (at.0 as i16, at.1 as i16), true).is_empty()
                || prop_wall_clears(session, anchor) == Some(true))
        {
            continue;
        }
        if *ran >= MAX_BEATS || !examined.insert(anchor) {
            continue;
        }
        *ran += 1;
        // A prop whose touch moves its flags and still leaves the one
        // the next anchor holds clear is a dial: it is turned again
        // until it stands where the retail run left it. `stone`'s four
        // statues each step through four facings (`0x1D8..0x1DB`,
        // `0x1DC..0x1DF`, `0x1E0..0x1E3`, `0x1E4..0x1E7`) and the way on
        // opens on one combination; turned once a round, all four moved
        // in step and the pass ran out of fresh flags short of it.
        let mut dial: BTreeSet<u16> = BTreeSet::new();
        let mut stuck = false;
        for turn in 0..DIAL_TURNS {
            if turn > 0 {
                // A statue still turning from the last touch takes no
                // new one: let it settle first.
                session.host.world.set_pad(0);
                for _ in 0..DIAL_SETTLE_TICKS {
                    if session.tick().is_err() {
                        break;
                    }
                }
            }
            let f0 = flags_of_world(session);
            let r = if gated {
                examine_prop(session, anchor, at, walked)
            } else {
                touch_prop(session, anchor, at)
            };
            let r = fight_committed(session, r);
            trace_beat(session, &f0, || format!("{name} prop {anchor:?} -> {r:?}"));
            match r {
                Run::Entered(s) => return Ok(Some(s)),
                Run::Battle(b) => return Err(format!("prop {anchor:?}: {b}")),
                Run::Error(e) => return Err(e),
                Run::Parked(_) => break,
                Run::Released => {}
            }
            // The dial is the flags its turns have moved; it stands
            // right when each of them agrees with the next anchor.
            let f1 = flags_of_world(session);
            dial.extend(f0.symmetric_difference(&f1).copied());
            let anchored = NEXT_ANCHOR_FLAGS.with(|a| a.borrow().is_some());
            // One touch that turned nothing may be the statue still
            // settling; two in a row and it is not a dial.
            if f1 == f0 {
                if stuck || turn == 0 {
                    break;
                }
                stuck = true;
                continue;
            }
            stuck = false;
            if turn + 1 >= DIAL_TURNS
                || !anchored
                || dial.iter().all(|&f| f1.contains(&f) == next_anchor_has(f))
                || !prop_beats(session, mf, man)
                    .iter()
                    .any(|(a, _, _)| *a == anchor)
            {
                break;
            }
        }
    }
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
    // A placed chest (a partition-0 record that gives an item) the retail
    // run had opened a few milestones on is opened where the continuous
    // hand stands: the Point Card (`town0c` / `town0d` P0[17], `0x10F`)
    // lies in Rim Elm and is first held at Mt. Rikuroa, and without it no
    // shop visit ever credits the card's points.
    let gives_item = |part: usize, rec: usize| {
        use legaia_engine_field::man_field_scripts::partition_record_span;
        use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker};
        partition_record_span(mf, man, part, rec).is_some_and(|(start, pc0, len)| {
            LinearWalker::new(&man[start..start + len], pc0)
                .flatten()
                .any(|i| matches!(i.info, InsnInfo::GiveItem { .. }))
        })
    };
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
                    && (next_anchor_has(s.flag)
                        || part == 0
                            && continuous::chest_ahead(s.flag)
                            && gives_item(part, s.record))
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
    /// Set while the continuous hand plays a scene on arrival, before any
    /// door has refused: the pass then walks only the bands that carry the
    /// story.
    pub(super) static PROACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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

/// Step onto the middle of box-test tile `t`, the tile a script's `CD`
/// BBoxTest reads off the actor (`(x - 0x40) >> 7`): world `t * 128 + 128`
/// on each axis. `Some(scene)` if a scene change landed.
pub(super) fn center_on_box_tile(session: &mut NativeSession, t: (u8, u8)) -> Option<String> {
    let at = (i16::from(t.0) * 128 + 128, i16::from(t.1) * 128 + 128);
    pad_lean(
        session,
        |_| Some(at),
        40,
        |s| {
            let (px, pz) = player_xz(s);
            (px - at.0).abs() <= 16 && (pz - at.1).abs() <= 16
        },
    )
}

/// Talk to `slot` without leaving box-test tile `t`: lean toward its actor
/// while the player's own tile stays `t`, until a facing's probe reaches it,
/// then press Cross and page what opens. `None` when no facing reaches it
/// from inside the box.
pub(super) fn talk_from_box(session: &mut NativeSession, slot: u8, t: (u8, u8)) -> Option<Run> {
    let box_tile = |s: &NativeSession| {
        let (x, z) = player_xz(s);
        ((i32::from(x) - 0x40) >> 7, (i32::from(z) - 0x40) >> 7)
    };
    let inside = |s: &NativeSession| box_tile(s) == (i32::from(t.0), i32::from(t.1));
    let reaches = |s: &mut NativeSession| {
        (0..8u8).any(|d| {
            s.host.world.face_player_sector(d);
            s.host.world.field_interact_probe_slot() == Some(slot)
        })
    };
    let mut found = reaches(session);
    for _ in 0..40 {
        if found {
            break;
        }
        let &(nx, nz) = session.host.world.npcs.positions.get(&slot)?;
        let (px, pz) = player_xz(session);
        let back = player_xz(session);
        let pad = pad_for_step(session, (nx - px).signum(), (nz - pz).signum());
        session.host.world.set_pad(pad);
        if let Ok(SceneTickEvent::SceneEntered { name }) = session.tick() {
            return Some(Run::Entered(name));
        }
        session.host.world.set_pad(0);
        if !inside(session) {
            restore_player_xz(session, back);
            found = reaches(session);
            break;
        }
        found = reaches(session);
    }
    if !found {
        return None;
    }
    session.host.world.set_pad(PadButton::Cross.mask());
    match session.tick() {
        Ok(SceneTickEvent::SceneEntered { name }) => return Some(Run::Entered(name)),
        Ok(_) => {}
        Err(e) => return Some(Run::Error(format!("{e:#}"))),
    }
    for f in 0..DEEP_EXIT_TICKS {
        if session.host.world.mode == SceneMode::Battle {
            if let Some(r) = drain_battle(session) {
                return Some(r);
            }
            continue;
        }
        let w = &session.host.world;
        if f >= 2
            && w.dialog.inline.is_none()
            && !w.dialogue_owns_input()
            && !w.field_vm.submode_screen.is_open()
        {
            break;
        }
        let pad = script_pad(session, f);
        session.host.world.set_pad(pad);
        if let Ok(SceneTickEvent::SceneEntered { name }) = session.tick() {
            return Some(Run::Entered(name));
        }
    }
    Some(run_while_moving(session, DEEP_EXIT_TICKS))
}

/// The walls prop `anchor`'s touch clears (`4C 70`): `None` when it clears
/// none, else whether every one of them is clear already. `korb3`'s gates
/// each have a stone on either side, both clearing the same two tiles: the
/// far stone of a gate the near one has opened adds nothing but its flag,
/// and that flag arms the band on the near side, which walls the gate up in
/// front of a party walking through.
pub(super) fn prop_wall_clears(session: &NativeSession, anchor: (u8, u8)) -> Option<bool> {
    use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker, MenuCtrlKind};
    let w = &session.host.world;
    let p = w.props.bank.props.get(&anchor)?;
    let grid = &w.terrain.collision_grid;
    let mut paints = 0;
    let mut open = true;
    for i in LinearWalker::new(&p.record_body, p.parked_pc).flatten() {
        if i.extended.is_some() {
            continue;
        }
        if let InsnInfo::MenuCtrl {
            kind:
                MenuCtrlKind::Nibble7Tile {
                    sub: 0,
                    x0,
                    z0,
                    x1,
                    z1,
                    ..
                },
            ..
        } = i.info
        {
            paints += 1;
            // The painted rows carry the arm's `+1` (`0x801E1CB4`).
            for row in usize::from(z0) + 1..=usize::from(z1) + 1 {
                for col in usize::from(x0)..=usize::from(x1) {
                    if grid.get(col + row * 0x80).is_some_and(|b| b >> 4 != 0) {
                        open = false;
                    }
                }
            }
        }
    }
    (paints > 0).then_some(open)
}

/// Is walk-on record `rec` (with the records it spawns) a **closer**? It
/// raises no system flag, lowers one the next anchor carries, and walls
/// tiles up (`4C 71`). `korb3`'s collection bands P2[5..13] stand one step
/// past each gate: each clears its stone's flag (`0x43E`..) and walls the
/// gate up again behind the party, and P2[4] at the hall's door resets them
/// all. Walking one as a beat shut the gate a stone had just opened in front
/// of the party. A band that lowers a flag without walling anything is left
/// to the ordinary rules: `rikuroa`'s terrace hops are such bands.
pub(super) fn band_is_closer(
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    rec: usize,
) -> bool {
    use legaia_engine_field::man_field_scripts::{FlagBank, walk_partition_gflag_sites};
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let mut recs = spawned_p2(mf, man, 2, rec, n0 + n1, 3);
    recs.insert(rec);
    let sites: Vec<_> = walk_partition_gflag_sites(mf, man, 2)
        .into_iter()
        .filter(|s| recs.contains(&s.record) && s.bank == FlagBank::System && s.clean)
        .collect();
    let walls = recs.iter().any(|&r| {
        legaia_engine_field::man_field_scripts::partition_record_span(mf, man, 2, r).is_some_and(
            |(start, pc0, len)| {
                legaia_scene_formats::field_disasm::LinearWalker::new(&man[start..start + len], pc0)
                    .flatten()
                    .any(|i| {
                        i.extended.is_none()
                            && matches!(
                                i.info,
                                legaia_scene_formats::field_disasm::InsnInfo::MenuCtrl {
                                    kind: legaia_scene_formats::field_disasm::MenuCtrlKind::Nibble7Tile {
                                        sub: 1,
                                        ..
                                    },
                                    ..
                                }
                            )
                    })
            },
        )
    });
    walls
        && !sites.iter().any(|s| s.kind == FlagKind::Set)
        && sites
            .iter()
            .any(|s| s.kind == FlagKind::Clear && next_anchor_has(s.flag))
}

/// Is walk-on record `rec` only a way across under the live flags? It arcs
/// the player (a ledge hop), every tile of its band is a link the planner
/// walks through ([`ledge_hops`]), and no system flag it or a record it
/// spawns raises is still clear, outside the ones it lowers again itself (a
/// hop's hold).
pub(super) fn band_is_bare_link(
    session: &NativeSession,
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    rec: u8,
    band: &BTreeMap<u8, Vec<(u8, u8)>>,
) -> bool {
    use legaia_engine_field::man_field_scripts::{FlagBank, walk_partition_gflag_sites};
    let Some(tiles) = band.get(&rec).filter(|t| !t.is_empty()) else {
        return false;
    };
    let links = ledge_hops(session);
    if !tiles
        .iter()
        .all(|&(x, z)| links.contains_key(&(i32::from(x), i32::from(z))))
    {
        return false;
    }
    // Only a hop: a lift or a pad the lattice cannot step onto is still
    // walked as a beat, since the walk onto it is how the hand reaches the
    // far side (`station` P2[17], the lift from (119..120, 117) up to the
    // hall whose teleport leads to the ticket counter).
    let hops =
        legaia_engine_field::man_field_scripts::partition_record_span(mf, man, 2, usize::from(rec))
            .is_some_and(|(start, pc0, len)| {
                legaia_scene_formats::field_disasm::LinearWalker::new(&man[start..start + len], pc0)
            .flatten()
            .any(|i| {
                i.extended == Some(0xF8)
                    && matches!(
                        i.info,
                        legaia_scene_formats::field_disasm::InsnInfo::ActorCtrl {
                            kind: legaia_scene_formats::field_disasm::ActorCtrlKind::ArcJump { .. },
                            ..
                        }
                    )
            })
            });
    if !hops {
        return false;
    }
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let mut recs = spawned_p2(mf, man, 2, usize::from(rec), n0 + n1, 3);
    recs.insert(usize::from(rec));
    let w = &session.host.world;
    let sites: Vec<_> = walk_partition_gflag_sites(mf, man, 2)
        .into_iter()
        .filter(|s| recs.contains(&s.record) && s.bank == FlagBank::System)
        .collect();
    let lowered: BTreeSet<u16> = sites
        .iter()
        .filter(|s| s.kind == FlagKind::Clear)
        .map(|s| s.flag)
        .collect();
    !sites.iter().any(|s| {
        s.kind == FlagKind::Set && !w.system_flag_test(s.flag) && !lowered.contains(&s.flag)
    })
}

/// Before a talk whose branch waits on a flag a party member raises by
/// standing in a box ([`talk_box_gate`]): inside a three-actor talk, walk a
/// member into the box and hand control on (Square) until a leader stands
/// who can reach the talk while the one left behind holds the flag up.
///
/// When the entry script names the member it tests on the box (`nilboa`'s
/// `CD 31`, member 1), that member walks in, and control moves on until the
/// flag is up and the talk is in the new leader's walk component. Each swap
/// ends in `P2[17]` (`FUN_801D27E0`), which re-dresses the boulder room by
/// the leader's ravine - Fire Ravine (`0x010`) seats the boulders at
/// (49, 65) / (51, 65), Wind Ravine (`0x011`) parks them at (127, 127),
/// Thunder Ravine (`0x012`) leaves them - so the talk's position is read
/// again after every swap. Without a named member, each member in turn is
/// left in the box until the flag stays up.
fn hold_talk_box(
    session: &mut NativeSession,
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    slot: u8,
) {
    let Some((flag, tiles, holder)) = talk_box_gate(session, mf, man, slot) else {
        return;
    };
    if session.host.world.dialog.three_actor_talk.is_none() {
        return;
    }
    let trace = std::env::var_os("LEGAIA_FGL_TRACE").is_some();
    // The box's middle first: the entry script carves its edges back out
    // with half-tile `4C E4` tests (`nilboa` raises `0x4F2` only for a
    // member near the centre of (54, 63)).
    let mut tiles = tiles;
    let (cx, cz) = tiles.iter().fold((0i32, 0i32), |(x, z), t| {
        (x + i32::from(t.0), z + i32::from(t.1))
    });
    let n = tiles.len().max(1) as i32;
    tiles.sort_by_key(|t| (i32::from(t.0) * n - cx).abs() + (i32::from(t.1) * n - cz).abs());
    let settle = |session: &mut NativeSession| {
        for _ in 0..8 {
            if session.tick().is_err() {
                break;
            }
        }
    };
    let stand = |session: &mut NativeSession| -> bool {
        let avoid = pad_avoid(session, None);
        // The box is in the script's own tiles, `(x - 0x40) >> 7`: the walk
        // ends on the tile's middle.
        let stood = tiles.iter().any(|&t| {
            matches!(pad_walk(session, t, &avoid, 0), Ok(Walk::Arrived))
                && center_on_box_tile(session, (t.0 as u8, t.1 as u8)).is_none()
        });
        if stood {
            let _ = run_while_moving(session, 120);
            settle(session);
        }
        stood
    };
    if let Some(h) = holder {
        let members = usize::from(session.host.world.party.party_count.clamp(1, 3));
        // The named member takes control first.
        for _ in 0..members {
            if session.host.world.party.party_leader_slot == Some(h) {
                break;
            }
            if !swap_leader(session) {
                return;
            }
            settle(session);
        }
        if session.host.world.party.party_leader_slot != Some(h) {
            return;
        }
        let stood = stand(session);
        if trace {
            eprintln!(
                "    [box-hold] P1[{slot}] member {h} in the box at {:?}: 0x{flag:03X} up {}",
                player_xz(session),
                session.host.world.system_flag_test(flag)
            );
        }
        for _ in 1..members {
            if !swap_leader(session) {
                return;
            }
            settle(session);
            let up = session.host.world.system_flag_test(flag);
            let reach = talk_in_reach(session, slot);
            if trace {
                eprintln!(
                    "    [box-hold] P1[{slot}] member {h} stood {stood}, 0x{flag:03X} up {up}, leader {:?} reaches the talk {reach}, talk at {:?}",
                    session.host.world.party.party_leader_slot,
                    session.host.world.npcs.positions.get(&slot),
                );
            }
            if up && reach {
                return;
            }
        }
        return;
    }
    for _ in 0..3 {
        let stood = stand(session);
        if !swap_leader(session) {
            return;
        }
        settle(session);
        let up = session.host.world.system_flag_test(flag);
        if trace {
            eprintln!(
                "    [box-hold] P1[{slot}] stood {stood}, 0x{flag:03X} up {up}, talk at {:?}, leader at {:?}",
                session.host.world.npcs.positions.get(&slot),
                player_xz(session)
            );
        }
        if up {
            return;
        }
    }
}

/// Can the leader walk to within a tile of talk `slot`'s actor? A parked
/// actor (`(127, 127)`) or one in another walk component is out of reach.
fn talk_in_reach(session: &NativeSession, slot: u8) -> bool {
    let Some(&(nx, nz)) = session.host.world.npcs.positions.get(&slot) else {
        return false;
    };
    let npc = dispatch_tile(nx, nz);
    if npc.0 >= 127 && npc.1 >= 127 {
        return false;
    }
    let (px, pz) = player_xz(session);
    let avoid = pad_avoid(session, None);
    let goal = (npc.0 as i16, npc.1 as i16);
    plan_path(session, cell_of(px, pz), goal, &avoid)
        .and_then(|p| p.last().copied())
        .is_some_and(|c| {
            let (x, z) = cell_center(c);
            let t = dispatch_tile(x, z);
            (t.0 - npc.0).abs() <= 1 && (t.1 - npc.1).abs() <= 1
        })
}
