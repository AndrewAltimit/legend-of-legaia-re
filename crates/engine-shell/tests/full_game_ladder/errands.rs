//! The continuous hand's errands: a story item a stretch consumes, which the
//! hand has to buy - and so first earn - before the beat that takes it can
//! play.
//!
//! `kor`'s Sage's Gate (P1[13]) spawns the `0x40A` beat (P2[10]) that moves
//! the guard dogs P1[9] / P1[10] off the `korb2` doorway only once all eight
//! of `koin2`'s Sage's Treasure Chests (P1[8..15]) have each taken a Soru
//! Bread (`4C 52 B2`) and raised their flag (`0x40B..0x412`). The one gold
//! counter that sells the bread is Soru's Bakery in `koin4`, at 6000 a loaf.
//! The retail anchors carry the chests fed with a 99 999 999 purse; a run of
//! its own pays the 48 000 the way a player does - it sells the armour and
//! weapons it has outgrown, and fights for the rest.

use super::*;

/// One record of a stretch scene that raises flags the next anchor carries
/// and takes an item from the bag on the way.
#[derive(Debug, Clone)]
pub(super) struct ErrandSite {
    pub(super) scene: String,
    pub(super) partition: usize,
    pub(super) record: usize,
    /// The wanted flags the record raises.
    pub(super) flags: BTreeSet<u16>,
}

/// A story item the stretch consumes: each site owes a copy while one of its
/// flags is clear.
#[derive(Debug, Clone)]
pub(super) struct Errand {
    pub(super) item: u8,
    pub(super) sites: Vec<ErrandSite>,
}

thread_local! {
    /// The stretch's errands ([`stretch_errands`]); empty outside the
    /// continuous run and on every stretch that has none.
    pub(super) static ERRANDS: std::cell::RefCell<Vec<Errand>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// Talks that ran and raised nothing, keyed by scene, slot and the
    /// errand items the bag held then: a chest that wants a loaf the bag
    /// does not hold is not worth a crossing until the bag changes.
    static DRY_TALKS: std::cell::RefCell<HashSet<(String, u8, u32)>> = Default::default();
}

/// The errands of a stretch whose waypoints and milestone scene are
/// `scenes`: for every flag of `wanted` (the next anchor's), the records of
/// those scenes that cleanly SET it (the disc-wide census), and of those,
/// each one that takes an item from the bag (`4C 52 id`). Other scenes' item
/// quests are the anchors' optional play.
pub(super) fn stretch_errands(
    index: &ProtIndex,
    census: &BTreeMap<u16, Vec<legaia_engine_field::man_field_scripts::FlagCensusSite>>,
    wanted: &BTreeSet<u16>,
    scenes: &BTreeSet<&str>,
) -> Vec<Errand> {
    use legaia_engine_field::man_field_scripts::partition_record_span;
    use legaia_scene_formats::field_disasm::LinearWalker;
    let mut by_record: BTreeMap<(String, usize, usize), BTreeSet<u16>> = BTreeMap::new();
    for &f in wanted {
        for s in census.get(&f).into_iter().flatten() {
            if s.kind == FlagKind::Set
                && s.clean
                && !s.variant
                && !s.debug_menu
                && scenes.contains(s.scene_name.as_str())
            {
                by_record
                    .entry((s.scene_name.clone(), s.partition, s.record))
                    .or_default()
                    .insert(f);
            }
        }
    }
    let mut mans: HashMap<String, Option<Vec<u8>>> = HashMap::new();
    let mut out: Vec<Errand> = Vec::new();
    for ((scene, partition, record), flags) in by_record {
        let man = mans
            .entry(scene.clone())
            .or_insert_with(|| {
                Scene::load(index, &scene)
                    .ok()
                    .and_then(|sc| sc.field_man_payload(index).ok().flatten())
            })
            .as_deref();
        let Some(man) = man else { continue };
        let Ok(mf) = legaia_scene_formats::man_section::parse(man) else {
            continue;
        };
        let Some((start, pc0, len)) = partition_record_span(&mf, man, partition, record) else {
            continue;
        };
        let body = &man[start..start + len];
        let taken = LinearWalker::new(body, pc0)
            .map_while(Result::ok)
            .find_map(|i| {
                (i.opcode == 0x4C && i.extended.is_none() && body.get(i.pc + 1) == Some(&0x52))
                    .then(|| body.get(i.pc + 2).copied())
                    .flatten()
            });
        let Some(item) = taken else { continue };
        let site = ErrandSite {
            scene,
            partition,
            record,
            flags,
        };
        match out.iter_mut().find(|e| e.item == item) {
            Some(e) => e.sites.push(site),
            None => out.push(Errand {
                item,
                sites: vec![site],
            }),
        }
    }
    out
}

/// Each errand item the bag is still short of, with a gold price:
/// `(item, copies, price)`.
pub(super) fn errands_short(w: &legaia_engine_core::world::World) -> Vec<(u8, u32, u32)> {
    let Some(prices) = w.shops.item_shop_data.as_ref() else {
        return Vec::new();
    };
    ERRANDS.with(|e| {
        e.borrow()
            .iter()
            .filter_map(|e| {
                let owed = e
                    .sites
                    .iter()
                    .filter(|s| s.flags.iter().any(|&f| !w.system_flag_test(f)))
                    .count() as u32;
                let held = u32::from(w.party.inventory.get(&e.item).copied().unwrap_or(0));
                let price = u32::from(prices.price(e.item));
                (owed > held && price > 0).then_some((e.item, owed - held, price))
            })
            .collect()
    })
}

/// The gold the errand items the bag is short of still cost.
pub(super) fn errand_cost(w: &legaia_engine_core::world::World) -> i64 {
    errands_short(w)
        .iter()
        .map(|&(_, n, p)| i64::from(n) * i64::from(p))
        .sum()
}

/// Is the purse short of what the errands cost?
pub(super) fn errand_gold_short(w: &legaia_engine_core::world::World) -> bool {
    let cost = errand_cost(w);
    cost > 0 && i64::from(w.party.money) < cost
}

/// How many copies of the errand items the bag holds: the key a dry talk is
/// remembered under.
fn errand_holdings(w: &legaia_engine_core::world::World) -> u32 {
    ERRANDS.with(|e| {
        e.borrow()
            .iter()
            .map(|e| u32::from(w.party.inventory.get(&e.item).copied().unwrap_or(0)))
            .sum()
    })
}

/// Remember that talk `slot` of the loaded scene ran and raised nothing.
pub(super) fn note_dry_talk(session: &NativeSession, slot: u8) {
    let key = (
        scene_name(session),
        slot,
        errand_holdings(&session.host.world),
    );
    DRY_TALKS.with(|d| d.borrow_mut().insert(key));
}

/// Did talk `slot` of the loaded scene run dry with the bag as it is?
pub(super) fn talk_ran_dry(session: &NativeSession, slot: u8) -> bool {
    let key = (
        scene_name(session),
        slot,
        errand_holdings(&session.host.world),
    );
    DRY_TALKS.with(|d| d.borrow().contains(&key))
}

/// Is `id` an item one of the stretch's errands still owes to a record? The
/// hand keeps those out of every heal: a Soru Bread restores HP, and a loaf
/// eaten on the road is 6000 gold the chests still want.
pub(super) fn errand_item(w: &legaia_engine_core::world::World, id: u8) -> bool {
    ERRANDS.with(|e| {
        e.borrow().iter().any(|e| {
            e.item == id
                && e.sites
                    .iter()
                    .any(|s| s.flags.iter().any(|&f| !w.system_flag_test(f)))
        })
    })
}

/// The continuous hand's errand at a waypoint, while the bag is short of an
/// errand item: sell what the bag has outgrown and buy the item at the
/// scene's counters ([`pad_shop`]), crossing to the counter that sells it
/// when it opens off another door (`koin4`'s bakery and its two shops each
/// open off their own `kor4` door), and while the purse is still short,
/// fight for the gold from the scene's bed ([`pad_train`]) on the way to
/// `out`. `Ok(Some(scene))` when it ended outside `cur`.
pub(super) fn pad_errand(
    session: &mut NativeSession,
    graph: &DiscGraph,
    cur: &str,
    out: Option<&str>,
    trail: &mut Vec<String>,
) -> Result<Option<String>, String> {
    let trace = std::env::var_os("LEGAIA_FGL_TRACE").is_some();
    if errands_short(&session.host.world).is_empty() {
        return Ok(None);
    }
    if trace {
        let w = &session.host.world;
        eprintln!(
            "    [errand] {cur}: short {:?} (item, copies, price), gold {}, outgrown {:?} worth {}",
            errands_short(w),
            w.party.money,
            surplus_gear(session),
            outgrown_value(session)
        );
    }
    if let Some(s) = pad_shop(session).map_err(|e| format!("in {cur}, errand shopping: {e}"))? {
        trail.push(format!("{s}(shop talk)"));
        return Ok(Some(s));
    }
    // The counter that sells the item stands in another part of the scene.
    let tiles = errand_merchant_tiles(session);
    if !tiles.is_empty() && errand_buyable_here(session) && !tiles_in_reach(session, &tiles) {
        match cross_over(session, graph, cur, "", &CrossTarget::Tiles(tiles)) {
            Ok(s) if s == cur => {
                trail.push(format!("{s}(errand crossing)"));
                if let Some(s) =
                    pad_shop(session).map_err(|e| format!("in {cur}, errand shopping: {e}"))?
                {
                    trail.push(format!("{s}(shop talk)"));
                    return Ok(Some(s));
                }
            }
            Ok(s) => return Ok(Some(s)),
            Err(why) => {
                if trace {
                    eprintln!("    [errand] {cur}: no way to the counter: {why}");
                }
            }
        }
    }
    // Still short of the price once the outgrown armour is sold: the
    // fights pay the rest.
    if errand_gold_short_after_sales(session)
        && scene_name(session) == cur
        && let Some(s) = pad_train(session, graph, out)
            .map_err(|e| format!("in {cur}, training for gold: {e}"))?
    {
        trail.push(format!("{s}(training)"));
        return Ok(Some(s));
    }
    Ok(None)
}

/// Does the bag hold an HP restorative the field menu uses (an errand's
/// own item aside)?
pub(super) fn holds_field_heal(session: &NativeSession) -> bool {
    use legaia_engine_menus::items::ItemEffect;
    let w = &session.host.world;
    w.party.inventory.iter().any(|(&id, &n)| {
        n > 0
            && !errand_item(w, id)
            && w.tables.item_catalog.get(id).is_some_and(|e| {
                e.usable_in_field
                    && matches!(e.effect, ItemEffect::Heal { .. } | ItemEffect::HealAll)
            })
    })
}

/// Is partition-1 record `slot` of `scene` an errand site (a talk that
/// takes an errand item on its way to a wanted flag)?
pub(super) fn errand_talk(scene: &str, slot: u8) -> bool {
    ERRANDS.with(|e| {
        e.borrow().iter().any(|e| {
            e.sites
                .iter()
                .any(|s| s.scene == scene && s.partition == 1 && s.record == usize::from(slot))
        })
    })
}

/// Forget the errands and the dry talks: a new stretch, or a reload.
pub(super) fn reset_errands() {
    ERRANDS.with(|e| e.borrow_mut().clear());
    DRY_TALKS.with(|d| d.borrow_mut().clear());
}
