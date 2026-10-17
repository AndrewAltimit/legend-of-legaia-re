//! How the continuous hand grows its party: fighting near a rest until the
//! present members stand near the level the retail run had by the coming
//! milestones.
//!
//! A hand that only walks the route meets each boss with whatever the route
//! happened to pay. A player who means to finish the game fights beside an
//! inn until the party is ready: out where the encounters roll, back to the
//! bed when the party is worn, and on once the levels are there.

use super::*;

thread_local! {
    /// The level each character record holds in the anchors of the next
    /// few milestones, indexed by character: what the retail run had grown
    /// to by then. Empty outside the continuous run.
    pub(super) static TARGET_LEVELS: std::cell::RefCell<Vec<u8>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Milestones ahead whose anchors set the level target: a party is trained
/// before the stretch that needs it, where the rests are, not inside the
/// dungeon the anchor was saved in.
pub(super) const LEVEL_LOOKAHEAD: usize = 5;

/// Outings one training session takes before the hand moves on regardless.
const TRAIN_ROUNDS: usize = 24;

/// Walk legs one outing takes.
const OUTING_LEGS: usize = 160;

/// The present members still more than a level short of the target.
pub(super) fn under_levelled(session: &NativeSession) -> Vec<(u8, u8, u8)> {
    let w = &session.host.world;
    TARGET_LEVELS.with(|t| {
        let t = t.borrow();
        w.present_party_list()
            .into_iter()
            .filter_map(|ch| {
                let want = t.get(usize::from(ch)).copied()?.saturating_sub(1);
                let have = w.party.roster.members.get(usize::from(ch))?.level();
                (have < want).then_some((ch, have, want))
            })
            .collect()
    })
}

/// Levels below the target at which a scene with a bed trains before its
/// beats even with no boss band in it (`segment`'s arrival pass).
pub(super) const FAR_SHORT: u8 = 5;

/// Is a present member [`FAR_SHORT`] levels or more below the target?
pub(super) fn far_under_levelled(session: &NativeSession) -> bool {
    under_levelled(session)
        .iter()
        .any(|&(_, have, want)| want.saturating_sub(have) >= FAR_SHORT)
}

/// Is there a reason to train: a present member short of the target
/// level, or a purse short of what the stretch's errand items cost
/// ([`errand_gold_short`]) - the fights pay the gold as they pay the XP.
pub(super) fn wants_training(session: &NativeSession) -> bool {
    !under_levelled(session).is_empty() || errand_gold_short_after_sales(session)
}

/// Is the purse short of the errands' price even once the bag's outgrown
/// armament is sold ([`outgrown_value`])? What the fights still owe.
pub(super) fn errand_gold_short_after_sales(session: &NativeSession) -> bool {
    let w = &session.host.world;
    let cost = errand_cost(w);
    let purse = i64::from(w.party.money) + outgrown_value(session);
    // Once the errand itself is paid for and only the bag's heal refill
    // ([`errand_heal_budget`]) is still owed, the fighting stops at the bag's
    // three heals a member: training from the bag spends the heals it is
    // fighting to buy back, and the Sol run met Gaza (`korb3`) with two.
    let reserve = HEAL_RESERVE * w.present_party_list().len().clamp(1, 3) as u32;
    if purse >= cost && heals_held(w) <= reserve {
        return false;
    }
    cost > 0 && purse < cost + errand_heal_budget(w)
}

fn party_xp(session: &NativeSession) -> u64 {
    let w = &session.host.world;
    w.present_party_list()
        .iter()
        .filter_map(|&ch| w.party.roster.members.get(usize::from(ch)))
        .map(|m| u64::from(m.cumulative_xp()))
        .sum()
}

/// One outing in a scene that rolls encounters: walk out from where the
/// party stands and back, taking the fights the hand prices as winnable,
/// until a member is under `floor` per-mille, the levels are there, or the
/// legs run out. On the overworld a leg is a few tiles in one heading; in a
/// field scene it heads for a far landmark (a door band, a placed NPC) and
/// stops short of it. `Ok(Some(scene))` when a leg left the scene.
fn outing(session: &mut NativeSession, floor: u32) -> Result<Option<String>, String> {
    let (x0, z0) = player_xz(session);
    let home = tile_of(x0, z0);
    let mut goals: Vec<((i16, i16), i32)> = Vec::new();
    if session.host.world.mode == SceneMode::Field {
        let mut marks: Vec<(i16, i16)> = Vec::new();
        if let Some((_, _, triggers)) = scene_man_and_triggers(session) {
            marks.extend(
                triggers
                    .iter()
                    .map(|t| (i16::from(t.tile_x), i16::from(t.tile_z))),
            );
        }
        marks.extend(session.host.world.npcs.positions.values().map(|&(x, z)| {
            let t = dispatch_tile(x, z);
            (t.0 as i16, t.1 as i16)
        }));
        marks.retain(|t| (1..126).contains(&t.0) && (1..126).contains(&t.1));
        let far = |t: &(i16, i16)| {
            (i32::from(t.0) - i32::from(home.0)).abs() + (i32::from(t.1) - i32::from(home.1)).abs()
        };
        // Farthest first, ties by tile: the placements come out of a hash
        // map, and the outing must not depend on its order.
        marks.sort_by_key(|t| (std::cmp::Reverse(far(t)), *t));
        marks.dedup();
        // One landmark per neighbourhood, farthest first.
        for t in marks {
            if far(&t) >= 8
                && !goals.iter().any(|(g, _)| {
                    (i32::from(g.0) - i32::from(t.0)).abs()
                        + (i32::from(g.1) - i32::from(t.1)).abs()
                        < 12
                })
            {
                goals.push((t, 3));
            }
        }
        goals.truncate(8);
    }
    if goals.is_empty() {
        for (dx, dz) in [
            (5i16, 0i16),
            (-5, 0),
            (0, 5),
            (0, -5),
            (4, 4),
            (-4, -4),
            (4, -4),
            (-4, 4),
        ] {
            goals.push((
                ((home.0 + dx).clamp(1, 126), (home.1 + dz).clamp(1, 126)),
                1,
            ));
        }
    }
    let mut failed = 0usize;
    let mut pick = 0usize;
    for leg in 0..OUTING_LEGS {
        pad_budget(session)?;
        if party_hp_permille(session) < floor || !wants_training(session) {
            break;
        }
        if !walking(session) || !session.host.world.scene_can_roll_encounters() {
            break;
        }
        let out = leg % 2 == 0;
        let (goal, within) = if out {
            goals[pick % goals.len()]
        } else {
            (home, 1)
        };
        let avoid = pad_avoid(session, None);
        let before = player_xz(session);
        match pad_walk(session, goal, &avoid, within) {
            Ok(Walk::Entered(s)) => return Ok(Some(s)),
            Ok(Walk::Arrived) => {}
            Err(e) => {
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!(
                        "      [train] leg {leg} to {goal:?} from {:?}: {}",
                        tile_of(before.0, before.1),
                        e.chars().take(160).collect::<String>()
                    );
                }
                if let Some(w) = wiped(session) {
                    return Err(w);
                }
                if e.contains("party wiped") {
                    return Err(e);
                }
                // A leg that did not arrive is a wall on the way: the next
                // outing heads somewhere else. One that went nowhere at
                // all counts toward giving the outing up.
                if out {
                    pick += 1;
                    if player_xz(session) == before {
                        failed += 1;
                        if failed >= goals.len() {
                            break;
                        }
                    }
                }
            }
        }
        if let Some(w) = wiped(session) {
            return Err(w);
        }
    }
    Ok(None)
}

/// Train the party from `home`, a scene with a rest: while a present member
/// is short of the target level, rest, go where the encounters roll (`home`
/// itself, or `out`, the next scene of the route), fight, and come back
/// worn. Ends where the levels are there, the fights stop paying, or the
/// hand can no longer get back to its bed. `Ok(Some(scene))` when it ended
/// outside `home`.
pub(super) fn pad_train(
    session: &mut NativeSession,
    graph: &DiscGraph,
    out: Option<&str>,
) -> Result<Option<String>, String> {
    if session.host.world.mode != SceneMode::Field || !walking(session) || !released(session) {
        return Ok(None);
    }
    if !wants_training(session) {
        return Ok(None);
    }
    let home = scene_name(session);
    let (beds, keepers) = rest_sites(session);
    let has_rest = !beds.is_empty() || !keepers.is_empty();
    let home_rolls = session.host.world.scene_can_roll_encounters();
    // Fighting for an errand's price, the bag stands in for a bed it cannot
    // walk to ([`pad_train_from`] heals from it at home): `koin2`'s inn
    // keeper (P1[23]) stands in one of the scene's five rooms.
    let for_gold = errand_gold_short_after_sales(session) && holds_field_heal(session);
    // The hand trains from a bed: a party worn down with nowhere to rest
    // walks into the next fight that way.
    if (!has_rest && !for_gold) || (home_rolls && !rest_close_by(session) && !for_gold) {
        return Ok(None);
    }
    // Training has its own time: the stretch's walking budget starts over
    // when it ends.
    PAD_DEADLINE.with(|d| d.set(session.frames + 4 * PAD_SEGMENT_FRAMES));
    PAD_BATTLE_LEFT.with(|l| l.set(4 * PAD_BATTLE_ALLOWANCE));
    TRAINING.with(|t| t.set(true));
    let r = pad_train_from(session, graph, out, &home, home_rolls);
    TRAINING.with(|t| t.set(false));
    // Training is the hand's own errand: a walk that stalled on the way
    // out ends it, and the stretch goes on from wherever the party stands.
    // Only a wipe is the stretch's.
    let r = match r {
        Err(e) if !e.contains("party wiped") && wiped(session).is_none() => {
            if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                eprintln!("    [train] ended: {e}");
            }
            let here = scene_name(session);
            Ok((here != home).then_some(here))
        }
        other => other,
    };
    PAD_DEADLINE.with(|d| d.set(session.frames + STRETCH_FRAMES.with(std::cell::Cell::get)));
    PAD_BATTLE_LEFT.with(|l| l.set(PAD_BATTLE_ALLOWANCE));
    r
}

fn pad_train_from(
    session: &mut NativeSession,
    graph: &DiscGraph,
    out: Option<&str>,
    home: &str,
    home_rolls: bool,
) -> Result<Option<String>, String> {
    let home = home.to_string();
    let short = under_levelled(session);
    let (beds, keepers) = rest_sites(session);
    let has_rest = true;
    let trace = std::env::var_os("LEGAIA_FGL_TRACE").is_some();
    if trace {
        eprintln!(
            "    [train] {home}: short {short:?} (character, level, wanted), gold {} for errands {}; beds {} keepers {keepers:?}; rolls {home_rolls}; out {out:?}",
            session.host.world.party.money,
            errand_cost(&session.host.world),
            beds.len()
        );
    }
    let mut idle = 0usize;
    for round in 0..TRAIN_ROUNDS {
        pad_budget(session)?;
        if !wants_training(session) {
            break;
        }
        let here = scene_name(session);
        // Rest up before going out.
        if here == home && has_rest && party_hp_permille(session) < 900 {
            RESTED.with(|r| r.borrow_mut().remove(&home));
            if let Some(s) = pad_rest_to(session, 900, true)? {
                return Ok(Some(s));
            }
        }
        // Home has no rest that still works (`cave01`'s Terra rests the
        // party only until the story sends Noa on, [`DRY_RESTS`]): the bag
        // stands in for the bed. A party short of the levels the coming
        // boss is beaten with is better served by them than by the
        // restoratives it would otherwise carry in, and the fights it
        // takes for them pay back gold and drops.
        if here == home && party_hp_permille(session) < 600 && !rest_works_here(session) {
            pad_field_heal(session, 900);
        }
        if party_hp_permille(session) < 600 {
            // The bed did not lift the party (an inn the purse cannot pay)
            // and the bag cannot either: the hand is done here.
            break;
        }
        if !walking(session) {
            break;
        }
        if !session.host.world.scene_can_roll_encounters() {
            // Out to the road.
            let Some(dest) = out.filter(|_| here == home) else {
                break;
            };
            let landed = pad_hop(session, graph, dest)?;
            if landed != dest {
                return Ok(Some(landed));
            }
            match run_while_moving(session, SCRIPT_CEILING) {
                Run::Released => {}
                Run::Entered(s) if s == home => continue,
                Run::Entered(s) => return Ok(Some(s)),
                Run::Battle(b) => return Err(b),
                Run::Error(e) => return Err(e),
                Run::Parked(_) => return Ok(Some(scene_name(session))),
            }
            if !session.host.world.scene_can_roll_encounters() {
                // The road rolls nothing: the hand is simply further along.
                return Ok(Some(scene_name(session)));
            }
        }
        let xp0 = party_xp(session);
        let gold0 = session.host.world.party.money;
        let left = outing(session, if has_rest { 550 } else { 650 })?;
        let gained = party_xp(session) - xp0.min(party_xp(session))
            + u64::try_from(session.host.world.party.money - gold0).unwrap_or(0);
        if trace {
            let w = &session.host.world;
            let levels: Vec<u8> = w
                .present_party_list()
                .iter()
                .filter_map(|&ch| w.party.roster.members.get(usize::from(ch)))
                .map(legaia_save::character::CharacterRecord::level)
                .collect();
            eprintln!(
                "    [train] round {round}: +{gained} xp, levels {levels:?}, weakest {} per-mille, in {} (frame {})",
                party_hp_permille(session),
                scene_name(session),
                session.frames
            );
        }
        if let Some(s) = left
            && s != home
        {
            return Ok(Some(s));
        }
        idle = if gained == 0 { idle + 1 } else { 0 };
        if idle >= 3 || !has_rest {
            // Fights the hand will not take pay nothing, and a party with
            // no bed trains only while it is fresh.
            break;
        }
        // Back to the bed.
        if scene_name(session) != home {
            let landed = pad_hop(session, graph, &home)?;
            if landed != home {
                return Ok(Some(landed));
            }
            match run_while_moving(session, SCRIPT_CEILING) {
                Run::Released => {}
                Run::Entered(s) if s == home => {}
                Run::Entered(s) => return Ok(Some(s)),
                Run::Battle(b) => return Err(b),
                Run::Error(e) => return Err(e),
                Run::Parked(_) => return Ok(Some(scene_name(session))),
            }
        }
    }
    let here = scene_name(session);
    if here == home && walking(session) && party_hp_permille(session) < 900 {
        RESTED.with(|r| r.borrow_mut().remove(&home));
        if let Some(s) = pad_rest_to(session, 900, true)? {
            return Ok(Some(s));
        }
    }
    Ok((here != home).then_some(here))
}
