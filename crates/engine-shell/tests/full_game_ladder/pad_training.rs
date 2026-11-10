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
///
/// Six, not five: the stretch into `cave01` is where Noa trains while
/// Terra's rest still works, and five milestones from it the anchors held
/// her at level 6, where the next stretch's held her at 9 - by then the rest
/// had gone dry and a bag-less Noa met `rikuroa`'s row 16 at level 5.
pub(super) const LEVEL_LOOKAHEAD: usize = 6;

thread_local! {
    /// The hand is out charging the gauge for a lost boss
    /// ([`pad_charge_for_race`]): a fight's turns go to the Spirit stance, and
    /// no art spends what they earn.
    pub(super) static CHARGING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the Spirit cell of every Miracle typist is up to the price.
fn typists_charged(session: &NativeSession) -> bool {
    const FULL: u16 = 99;
    let w = &session.host.world;
    w.present_party_list().into_iter().all(|m| {
        !race_typist(w, m)
            || w.party
                .roster
                .members
                .get(usize::from(m))
                .is_some_and(|r| r.hp_mp_sp().sp_cur >= FULL)
    })
}

/// Whether the party still holds half its MP. Charging pays for its fights out of
/// what would heal it, and a party that spends it all meets the boss hurt.
fn charge_affordable(session: &NativeSession) -> bool {
    let w = &session.host.world;
    let (mp, max) = w
        .present_party_list()
        .iter()
        .filter_map(|&m| w.party.roster.members.get(usize::from(m)))
        .map(|r| r.hp_mp_sp())
        .fold((0u32, 0u32), |(c, x), v| {
            (c + u32::from(v.mp_cur), x + u32::from(v.mp_max))
        });
    mp * 2 >= max
}

/// After a lost race against a timed finisher (Koru), walk the scene's
/// encounters until the typists' gauges are full. The gauge carries between
/// fights (the record's AP cell `+0x10E`,
/// [`arts-command-gauge.md`](../../../../docs/subsystems/arts-command-gauge.md#what-an-art-costs-in-ap)),
/// so a player who watched the finisher land walks into the next attempt with a
/// Miracle ready: `+0x20` a Spirit turn. Against any other boss the walk costs
/// more than it buys: the stance earns the same in the boss's own rounds, and
/// the first attempt from a `zora_castle` save took Zora to 4204 of 22422 on 22
/// heals where each charged attempt after it met him with an empty bag and left
/// him over 19000. Returns the scene, when the walk left this one.
pub(super) fn pad_charge_for_race(session: &mut NativeSession) -> Result<Option<String>, String> {
    /// Outings before the hand goes in with what it has.
    const CHARGE_OUTINGS: usize = 6;
    if !RACE_LOST.with(std::cell::Cell::get)
        || !at_lost_door()
        || session.host.world.party.party_count < 3
        || !walking(session)
        || !released(session)
        || !session.host.world.scene_can_roll_encounters()
    {
        return Ok(None);
    }
    if typists_charged(session) {
        return Ok(None);
    }
    let here = scene_name(session);
    // The walk is taken on its feet: the stance halves what it costs.
    pad_field_heal(session, 900);
    // A wipe out here unwinds to the reload: the flag must not outlive the walk.
    struct Charging;
    impl Drop for Charging {
        fn drop(&mut self) {
            CHARGING.with(|c| c.set(false));
        }
    }
    let charging = Charging;
    CHARGING.with(|c| c.set(true));
    let mut left = Ok(None);
    for _ in 0..CHARGE_OUTINGS {
        if typists_charged(session) || !charge_affordable(session) {
            break;
        }
        // A fled fight comes back hurt, and an outing below its floor walks nowhere.
        pad_field_heal(session, 900);
        left = outing(session, 400);
        if !matches!(&left, Ok(None)) || scene_name(session) != here || !walking(session) {
            break;
        }
    }
    drop(charging);
    // The last fight may have been fled: the boss is met healed.
    if matches!(&left, Ok(None)) && walking(session) {
        pad_field_heal(session, 900);
    }
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        let w = &session.host.world;
        let cells: Vec<u16> = w
            .present_party_list()
            .iter()
            .filter_map(|&m| w.party.roster.members.get(usize::from(m)))
            .map(|r| r.hp_mp_sp().sp_cur)
            .collect();
        eprintln!("    [charge] AP cells {cells:?}");
    }
    left.map(|s| s.filter(|s| *s != here))
}

/// Outings one training session takes before the hand moves on regardless.
const TRAIN_ROUNDS: usize = 24;

/// Walk legs one outing takes.
const OUTING_LEGS: usize = 160;
/// Scenes an errand's gold outing hops through from its bed to the first
/// one that rolls encounters, and the hops [`pad_errand_from_rest`] walks
/// back to that bed.
pub(super) const ROAD_HOPS: usize = 4;

thread_local! {
    /// Set while the hand fights for an errand's gold from the last bed that
    /// worked ([`pad_errand_from_rest`]): the outing then follows the route
    /// past scenes that roll nothing.
    static ERRAND_ROAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The scene the last training outing fought in: where an errand's
    /// outing from a bed heads for.
    static LAST_OUTING: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Walk `way` (a scene route whose first entry is the loaded scene), a hop
/// at a time. `Ok(None)` at its end, `Ok(Some(scene))` where a hop or its
/// arrival script left the route.
fn hop_along(
    session: &mut NativeSession,
    graph: &DiscGraph,
    way: &[String],
) -> Result<Option<String>, String> {
    for next in way.iter().skip(1) {
        let landed = pad_hop(session, graph, next)?;
        if landed != *next {
            return Ok(Some(landed));
        }
        match run_while_moving(session, SCRIPT_CEILING) {
            Run::Released => {}
            Run::Entered(s) => return Ok(Some(s)),
            Run::Battle(b) => return Err(b),
            Run::Error(e) => return Err(e),
            Run::Parked(_) => return Ok(Some(scene_name(session))),
        }
    }
    Ok(None)
}

/// The errand's gold, fought for from the last rest that lifted the party
/// ([`LAST_REST`]) when the training on the way came up short: walk back to
/// that scene, and train from its bed with the outing following the route
/// as far as the scene the last outing fought in. `Ok(Some(scene))` when it
/// ended outside `cur`.
///
/// Sol's errand is the case. The hand fights for the bread's 48 000 on the
/// way down from `kor5`, out of the bag from `kor3` (the stairs between roll
/// nothing, so the training from `kor5`'s bed moves on at `kor4`), and stops
/// when the bag is down to its reserve. A stream on which that came first
/// reached `koin2` with 37 313 gold and 7 480 of outgrown gear, bought six
/// loaves of eight, could not train in the chest room it stood in, and
/// walked on to `kor`'s guard dogs with two chests unfed.
pub(super) fn pad_errand_from_rest(
    session: &mut NativeSession,
    graph: &DiscGraph,
    cur: &str,
) -> Result<Option<String>, String> {
    let trace = std::env::var_os("LEGAIA_FGL_TRACE").is_some();
    let last = LAST_REST.with(|l| l.borrow().clone());
    let way = last
        .as_ref()
        .filter(|b| *b != cur)
        .and_then(|b| graph.route(cur, b))
        .filter(|r| r.len() <= ROAD_HOPS + 1);
    let (Some(bed), Some(way)) = (last.clone(), way) else {
        if trace {
            eprintln!("    [errand] {cur}: no rest to fight from (last rest {last:?})");
        }
        return Ok(None);
    };
    let field = LAST_OUTING
        .with(|l| l.borrow().clone())
        .unwrap_or_else(|| cur.to_string());
    if trace {
        let w = &session.host.world;
        eprintln!(
            "    [errand] {cur}: gold {} short of {} and a reserve of {}; back to the rest in {bed} by {way:?}, to fight in {field}",
            w.party.money,
            errand_cost(w),
            errand_counter_reserve(w)
        );
    }
    if let Some(s) = hop_along(session, graph, &way)? {
        return Ok(Some(s));
    }
    let out = graph.route(&bed, &field).and_then(|r| r.get(1).cloned());
    ERRAND_ROAD.with(|r| r.set(true));
    let r = pad_train(session, graph, out.as_deref(), Some(&field));
    ERRAND_ROAD.with(|r| r.set(false));
    Ok(r?.or_else(|| Some(scene_name(session)).filter(|s| s != cur)))
}

/// The present members short of the target plus its margin
/// ([`LEVEL_MARGIN`], [`SOLO_LEVEL_MARGIN`]).
///
/// A member who stands alone takes a wider margin: every hit lands on the
/// one body, and nothing shares the damage. `rikuroa` is the case - Noa
/// alone against a 640-HP row 16 with a thin bag - where a level-8 Noa won
/// it only on streams whose drops had left her heals.
pub(super) fn under_levelled(session: &NativeSession) -> Vec<(u8, u8, u8)> {
    let w = &session.host.world;
    let party = w.present_party_list();
    let margin = if party.len() == 1 {
        SOLO_LEVEL_MARGIN
    } else {
        LEVEL_MARGIN
    };
    TARGET_LEVELS.with(|t| {
        let t = t.borrow();
        party
            .into_iter()
            .filter_map(|ch| {
                let want = t.get(usize::from(ch)).copied()?.saturating_add(margin);
                let have = w.party.roster.members.get(usize::from(ch))?.level();
                (have < want).then_some((ch, have, want))
            })
            .collect()
    })
}

/// Levels over the anchors' target a party trains to: the anchors are one
/// player's growth, and the hand's fights are dealt off other streams, so a
/// party a level under them met the bosses the anchors' player only just
/// beat (Zora, `taiku`, lost on two of fourteen streams).
pub(super) const LEVEL_MARGIN: u8 = 0;
/// The same for a member who stands alone: every hit lands on the one body.
pub(super) const SOLO_LEVEL_MARGIN: u8 = 2;

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
    // From a bed ([`pad_errand_from_rest`]) the fights cost the bag nothing:
    // the refill, or the reserve a counter sells before the item, is fought
    // for as well, and in coin - what the outgrown armament fetches is
    // counted on the way in and has mostly been sold by the way back.
    if ERRAND_ROAD.with(std::cell::Cell::get) {
        return cost > 0
            && i64::from(w.party.money)
                < cost + errand_heal_budget(w).max(errand_counter_reserve(w));
    }
    if purse >= cost && heals_held(w) <= reserve {
        return false;
    }
    cost > 0 && purse < cost + errand_heal_budget(w)
}

/// What a counter's heal reserve can take of an errand's purse: three heals
/// a member ([`HEAL_RESERVE`]) at the list price of the dearest heal a
/// counter's reserve is bought in - one that restores at least a hit point
/// a coin, which leaves out the party-wide ones (the Healing Flower, 800 HP
/// for 800). A counter sells the reserve before the errand's item, and what
/// the bag holds of it by then is the road's to say, so the whole of it is
/// counted.
fn errand_counter_reserve(w: &legaia_engine_core::world::World) -> i64 {
    use legaia_engine_menus::items::ItemEffect;
    let Some(prices) = w.shops.item_shop_data.as_ref() else {
        return 0;
    };
    let dearest = (0u8..=255)
        .filter_map(|id| {
            let ItemEffect::Heal { amount } = w.tables.item_catalog.get(id)?.effect else {
                return None;
            };
            let price = i64::from(prices.price(id));
            (price > 0 && price <= i64::from(amount)).then_some(price)
        })
        .max()
        .unwrap_or(0);
    let members = (w.present_party_list().len() + joining_members(w).len()).clamp(1, 3) as i64;
    i64::from(HEAL_RESERVE) * members * dearest
}

/// Is the purse, with the outgrown armament sold, short of what the
/// errands' counters will take: the items' price and the heal reserve a
/// counter sells first ([`errand_counter_reserve`])? Training out of the bag
/// stops once the items alone are paid for
/// ([`errand_gold_short_after_sales`]), on a bag at that reserve or under
/// it: `koin4`'s item counter then took 3 600 of a 48 393 purse for nine
/// heals, and the bakery sold seven loaves of eight.
pub(super) fn errand_purse_short(session: &NativeSession) -> bool {
    let w = &session.host.world;
    let cost = errand_cost(w);
    cost > 0
        && i64::from(w.party.money) + outgrown_value(session) < cost + errand_counter_reserve(w)
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
        let charging = CHARGING.with(std::cell::Cell::get);
        if party_hp_permille(session) < floor
            || !(wants_training(session) || charging && !typists_charged(session))
        {
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
    goal: Option<&str>,
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
    let r = pad_train_from(session, graph, out, goal, &home, home_rolls);
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
    goal: Option<&str>,
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
    // The scenes the outing hopped through from `home`, nearest first.
    let mut road: Vec<String> = Vec::new();
    let rest_at_home = !beds.is_empty() || !keepers.is_empty();
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
            road.clear();
            road.push(dest.to_string());
            // An errand's gold the bag could not earn is fought for from a
            // bed that works ([`pad_errand_from_rest`]): where the next
            // scene rolls nothing either, the outing goes on down the route
            // to the first scene that does, and comes back the way it went.
            // `kor5`'s keeper (P1[9]) rests the party three doors above
            // `kor`, the one floor of Sol's stairs that rolls.
            while !session.host.world.scene_can_roll_encounters()
                && road.len() < ROAD_HOPS
                && rest_at_home
                && ERRAND_ROAD.with(std::cell::Cell::get)
                && errand_gold_short_after_sales(session)
            {
                let at = scene_name(session);
                let Some(next) = goal
                    .and_then(|g| graph.route(&at, g))
                    .and_then(|r| r.get(1).cloned())
                    .filter(|n| *n != home && !road.contains(n))
                else {
                    break;
                };
                let landed = pad_hop(session, graph, &next)?;
                if landed != next {
                    return Ok(Some(landed));
                }
                match run_while_moving(session, SCRIPT_CEILING) {
                    Run::Released => {}
                    Run::Entered(s) => return Ok(Some(s)),
                    Run::Battle(b) => return Err(b),
                    Run::Error(e) => return Err(e),
                    Run::Parked(_) => return Ok(Some(scene_name(session))),
                }
                road.push(next);
            }
            if !session.host.world.scene_can_roll_encounters() {
                // The road rolls nothing: the hand is simply further along.
                return Ok(Some(scene_name(session)));
            }
        }
        let xp0 = party_xp(session);
        let gold0 = session.host.world.party.money;
        // A lone member with a working bed goes back to it early: nothing shares
        // the hits, and a level-1 Noa who walked on at 90 of 150 HP from `cave01`'s
        // Terra met three foes she could neither beat nor outrun.
        let lone = session.host.world.present_party_list().len() == 1;
        let floor = if lone && rest_works_here(session) {
            800
        } else if has_rest {
            550
        } else {
            650
        };
        LAST_OUTING.with(|l| *l.borrow_mut() = Some(scene_name(session)));
        let left = outing(session, floor)?;
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
        // Back to the bed, by the road the outing took.
        while scene_name(session) != home {
            let here = scene_name(session);
            let back = match road.iter().position(|s| *s == here) {
                Some(0) | None => home.clone(),
                Some(i) => road[i - 1].clone(),
            };
            let landed = pad_hop(session, graph, &back)?;
            if landed != back {
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
