//! What the continuous hand buys: the merchant talk, and the shop screen
//! driven one pad edge at a time through the menu runtime both play hosts
//! own.
//!
//! A merchant is an ordinary talk whose record reaches op `0x49` sub-0 with a
//! priced stock list. The field VM parks on it and stages a shop session;
//! the host opens the screen, steps it on pad edges with the field frozen,
//! and the script resumes when the screen closes. `NativeSession` is the
//! session without a host, so the ladder plays the host's part here with the
//! same runtime calls (`open_field_overlay_requests`, `step_field_session`).

use super::*;

use legaia_engine_menu_runtime::menu_runtime::MenuRuntime;
use legaia_engine_menus::menu::MenuState;

thread_local! {
    /// The host half of the shop: the menu runtime the screen lives in.
    static SHOP_MENU: std::cell::RefCell<Option<MenuRuntime>> =
        const { std::cell::RefCell::new(None) };
    /// Set when a shop screen was opened and closed since last asked: a
    /// merchant's talk gains no flag, so the talk loop reads this instead.
    static SHOP_VISITED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// `(item, character)` buys the screen refused or that changed nothing,
    /// so one visit never asks for the same row twice.
    static REFUSED_BUYS: std::cell::RefCell<HashSet<(u8, u8)>> = Default::default();
    /// `(scene, gold, bag size)` at the last visit to a scene's merchants:
    /// the hand comes back only with a different purse or bag.
    static SHOPPED: std::cell::RefCell<HashMap<String, (i32, usize)>> = Default::default();
    /// Shop screens opened this run: how a merchant visit tells a counter
    /// that sold from one that only talked.
    static SHOP_OPENS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Per scene, the raised story flags its counters were last found
    /// closed on and that the scene can lower again ([`open_counters`]).
    static CLOSED_ON: std::cell::RefCell<HashMap<String, BTreeSet<u16>>> = Default::default();
}

/// Did a shop screen open and close since the last call?
pub(super) fn take_shop_visited() -> bool {
    SHOP_VISITED.with(|v| v.replace(false))
}

/// Healing items the hand keeps in the bag for each present member when a
/// merchant sells them: the road between two counters is a dungeon or two.
pub(super) const HEAL_STOCK: u8 = 15;

/// Healing items for each present member the hand buys before any
/// equipment ([`next_want`]).
pub(super) const HEAL_RESERVE: u32 = 3;

/// MP restoratives the hand keeps in the bag for each present member, bought
/// right after the [`HEAL_RESERVE`] when a counter sells one ([`mp_want`]).
pub(super) const MP_RESERVE: u32 = 1;

/// What an MP restorative restores, for ranking stock rows: a full restore
/// counts as a whole pool.
fn mp_restore(effect: &legaia_engine_menus::items::ItemEffect) -> Option<u32> {
    use legaia_engine_menus::items::ItemEffect;
    match *effect {
        ItemEffect::HealMp { amount } => Some(u32::from(amount)),
        ItemEffect::HealMpAll => Some(999),
        _ => None,
    }
}

/// The MP restoratives in the bag, an errand's item aside.
pub(super) fn mp_items_held(w: &legaia_engine_core::world::World) -> u32 {
    w.party
        .inventory
        .iter()
        .filter(|&(&id, _)| {
            !errand_item(w, id)
                && w.tables
                    .item_catalog
                    .get(id)
                    .is_some_and(|e| mp_restore(&e.effect).is_some())
        })
        .map(|(_, &n)| u32::from(n))
        .sum()
}

/// The buy that brings the bag's MP restoratives up to `target`: the stock
/// row that restores the most MP per gold, as many as the purse pays for
/// after the errands' price.
fn mp_want(
    session: &legaia_engine_menus::shop::ShopSession,
    w: &legaia_engine_core::world::World,
    target: u32,
) -> Option<Want> {
    let short = target.saturating_sub(mp_items_held(w));
    if short == 0 {
        return None;
    }
    let (row, id, price) = session
        .inventory
        .items
        .iter()
        .enumerate()
        .filter_map(|(row, it)| {
            let amount = mp_restore(&w.tables.item_catalog.get(it.item_id)?.effect)?;
            (it.price > 0).then_some((row, it.item_id, it.price, amount))
        })
        .max_by_key(|&(row, _, price, amount)| (amount * 1000 / price, std::cmp::Reverse(row)))
        .map(|(row, id, price, _)| (row, id, price))?;
    let held = w.party.inventory.get(&id).copied().unwrap_or(0);
    let room = legaia_engine_menus::shop::SHOP_HELD_CAP.saturating_sub(held);
    let afford =
        u32::try_from((i64::from(w.party.money) - errand_cost(w)).max(0)).unwrap_or(0) / price;
    let qty = short.min(afford).min(u32::from(room));
    (qty > 0).then_some(Want::Stack {
        row,
        qty: qty as u8,
    })
}

/// What the hand wants from the open buy list next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Want {
    /// Row `row`: a piece of equipment for the member at party-list position
    /// `member` (character `ch`).
    Gear {
        row: usize,
        item: u8,
        member: usize,
        ch: u8,
    },
    /// Row `row`: `qty` copies of a stackable item.
    Stack { row: usize, qty: u8 },
}

/// Does the bag hold fewer HP restoratives than half the [`HEAL_STOCK`] the
/// hand keeps for each present member? What a waypoint stocks up on: its
/// counter can be the last before the stretch's boss. `son` is the dock the
/// boat lands at on the way to `taiku`, and a run that passed through it
/// with eight Healing Flowers and 52000 gold fought Zora (F9, 22422 HP,
/// a party-wide breath every other round) out of an empty bag and lost
/// three times in four.
pub(super) fn bag_under_half_stock(session: &NativeSession) -> bool {
    let w = &session.host.world;
    heals_held(w) < u32::from(HEAL_STOCK) / 2 * w.present_party_list().len().clamp(1, 3) as u32
}

/// The HP restoratives in the bag, an errand's item aside.
pub(super) fn heals_held(w: &legaia_engine_core::world::World) -> u32 {
    use legaia_engine_menus::items::ItemEffect;
    w.party
        .inventory
        .iter()
        .filter(|&(&id, _)| {
            !errand_item(w, id)
                && w.tables.item_catalog.get(id).is_some_and(|e| {
                    matches!(e.effect, ItemEffect::Heal { .. } | ItemEffect::HealAll)
                })
        })
        .map(|(_, &n)| u32::from(n))
        .sum()
}

/// What the continuous hand sells at a counter whose stock sells a heal
/// while the bag holds fewer than its [`HEAL_STOCK`]: the armament no member
/// who has joined is better off wearing. A purse that cannot pay for the
/// heals the next stretch is fought out of is what a stack of outgrown
/// armour is for: the run reached Nivora's three solo duels with 26 000
/// gold and a bag of old gear, and the only counter left (`doman`, Healing
/// Berries at 3000) bought seven.
fn heal_sale_gear(session: &NativeSession, sells_heal: bool) -> BTreeSet<u8> {
    let w = &session.host.world;
    if !continuous::PLAYS_ON.with(std::cell::Cell::get)
        || !sells_heal
        || heals_held(w) >= u32::from(HEAL_STOCK) * w.present_party_list().len().clamp(1, 3) as u32
    {
        return BTreeSet::new();
    }
    outgrown_gear(session, true)
}

/// The worth of a piece of equipment to the hand: the sum of its stat
/// bonuses. Enough to tell a better weapon or armour from a worse one in
/// the same slot.
fn gear_worth(w: &legaia_engine_core::world::World, id: u8) -> i32 {
    w.tables.equipment_table.get(id).map_or(0, |m| {
        i32::from(m.atk) + i32::from(m.udf) + i32::from(m.ldf) + i32::from(m.spd) + i32::from(m.int)
    })
}

/// The next purchase: the affordable piece of equipment that improves a
/// present member's slot the most, else healing items up to [`HEAL_STOCK`].
/// The first three heals come before any equipment - a player with an empty
/// bag buys those first.
fn next_want(menu: &MenuRuntime, w: &legaia_engine_core::world::World) -> Option<Want> {
    use legaia_engine_menus::equipment::EquipSlot;
    // Only the continuous hand buys. A segmented pass whose talk reaches a
    // counter (`doman` P1's merchant, on the way to Nivora) looks and
    // leaves: the screen still has to open and close for the merchant's
    // record to resume.
    if !continuous::PLAYS_ON.with(std::cell::Cell::get) {
        return None;
    }
    use legaia_engine_menus::items::ItemEffect;
    use legaia_game_tables::equip_stats::EquipSlot as Disc;
    let session = menu.shop_session.as_ref()?;
    let gold = w.party.money;
    let held = |id: u8| w.party.inventory.get(&id).copied().unwrap_or(0);
    let errands = errands_short(w);
    // The stretch's errand items first, as many as the purse pays for: the
    // story waits on them, and a loaf bought as a heal is the same loaf.
    for &(item, short, price) in &errands {
        let Some(row) = session
            .inventory
            .items
            .iter()
            .position(|it| it.item_id == item && it.price > 0)
        else {
            continue;
        };
        let room = legaia_engine_menus::shop::SHOP_HELD_CAP.saturating_sub(held(item));
        let qty = short
            .min(u32::try_from(gold).unwrap_or(0) / price)
            .min(u32::from(room));
        if qty > 0 {
            return Some(Want::Stack {
                row,
                qty: qty as u8,
            });
        }
    }
    // While an errand is short of gold, the purse is the errand's: nothing
    // else is bought until the items are paid for.
    if errand_gold_short(w) {
        return None;
    }
    // Healing: the stock row that restores the most HP per gold. An errand
    // item is not a heal to buy: it is bought above, for the story. A full
    // restore (`HealAll`: the Healing Berry, "Restores maximum HP") is worth
    // a present member's maximum HP; `doman`'s counter, the last before
    // Nivora's three solo duels, sells no other.
    let full_restore: u32 = {
        let party = w.present_party_list();
        let maxes: Vec<u32> = party
            .iter()
            .filter_map(|&ch| w.party.roster.members.get(usize::from(ch)))
            .map(|m| u32::from(m.hp_mp_sp().hp_max))
            .collect();
        (maxes.iter().sum::<u32>() / (maxes.len().max(1) as u32)).max(1)
    };
    let heal_row = session
        .inventory
        .items
        .iter()
        .enumerate()
        .filter(|(_, it)| !errands.iter().any(|e| e.0 == it.item_id))
        .filter_map(|(row, it)| {
            let e = w.tables.item_catalog.get(it.item_id)?;
            let amount = match e.effect {
                ItemEffect::Heal { amount } => u32::from(amount),
                ItemEffect::HealAll => full_restore,
                _ => return None,
            };
            (it.price > 0).then_some((row, it.item_id, it.price, amount))
        })
        .max_by_key(|&(row, _, price, amount)| (amount * 1000 / price, std::cmp::Reverse(row)));
    // The bag's heals in Healing Flowers' worth (800 HP each), not in items:
    // counted by the item, 38 Healing Leaves read as a full stock, and a run
    // that bought seven Flowers at `rayman2` met Van Saryu (`jagaroom`) with
    // none.
    let heals_held: u32 = w
        .party
        .inventory
        .iter()
        .filter(|&(&id, _)| !errand_item(w, id))
        .filter_map(|(&id, &n)| {
            let restore = match w.tables.item_catalog.get(id)?.effect {
                ItemEffect::Heal { amount } => u32::from(amount),
                ItemEffect::HealAll => full_restore,
                _ => return None,
            };
            Some(u32::from(n) * restore)
        })
        .sum::<u32>()
        / 800;
    let heal_want = |target: u32| -> Option<Want> {
        let (row, id, price, _) = heal_row?;
        let room = legaia_engine_menus::shop::SHOP_HELD_CAP.saturating_sub(held(id));
        // The errands' price is kept back: the counter that sells their
        // item may be the next one.
        let afford = u32::try_from((i64::from(gold) - errand_cost(w)).max(0)).unwrap_or(0) / price;
        let qty = target
            .saturating_sub(heals_held)
            .min(afford)
            .min(u32::from(room));
        (qty > 0).then_some(Want::Stack {
            row,
            qty: qty as u8,
        })
    };
    // Three heals for each present member come before any equipment: the
    // next boss is fought out of the bag the road leaves, and a party of
    // three that bought its armour first met Dohati (`dohaty` P2[10]) with
    // one Healing Flower.
    let members = w.present_party_list().len().clamp(1, 3) as u32;
    if let Some(want) = heal_want(HEAL_RESERVE * members) {
        return Some(want);
    }
    // Then an MP restorative for each present member: a boss's group heals
    // are cast, and a counter whose heals the hand bought alone left Zora's
    // party (`son`'s nineteen Healing Berries, its Magic Fruit row untouched)
    // with no way to pay for the Spoons its party-wide hits called for.
    if let Some(want) = mp_want(session, w, members * MP_RESERVE) {
        return Some(want);
    }
    // Equipment. At a counter with no healing row, a bag short of its
    // three heals keeps their price back for the counter that has one;
    // while an errand is pending, so does the bag's heal stock
    // ([`errand_heal_budget`]).
    let reserve: i64 = if heal_row.is_none() && heals_held < 3 {
        300
    } else {
        0
    } + errand_cost(w)
        + errand_heal_budget(w);
    let info = menu.equip_info.as_ref()?;
    let party = w.present_party_list();
    let mut best: Option<(i32, u32, Want)> = None;
    for (row, it) in session.inventory.items.iter().enumerate() {
        let Some(entry) = info.entry(it.item_id) else {
            continue;
        };
        if entry.is_ra_seru || i64::from(it.price) + reserve > i64::from(gold) {
            continue;
        }
        let slot = match entry.category {
            Disc::Weapon => EquipSlot::Weapon,
            Disc::Body => EquipSlot::BodyArmor,
            Disc::Head => EquipSlot::Helmet,
            Disc::Footwear => EquipSlot::Boot,
        };
        for (member, &ch) in party.iter().enumerate().take(3) {
            if !info.can_equip(it.item_id, ch)
                || REFUSED_BUYS.with(|r| r.borrow().contains(&(it.item_id, ch)))
            {
                continue;
            }
            let Some(rec) = w.party.roster.members.get(usize::from(ch)) else {
                continue;
            };
            let worn = rec.equipment().slots[slot.as_index() as usize];
            let gain = gear_worth(w, it.item_id) - gear_worth(w, worn);
            // A piece the bag already holds is worn through the Equip
            // screen, not bought again.
            if gain <= 0 || held(it.item_id) > 0 {
                continue;
            }
            if best.as_ref().is_none_or(|&(g, p, _)| {
                (gain, std::cmp::Reverse(it.price)) > (g, std::cmp::Reverse(p))
            }) {
                best = Some((
                    gain,
                    it.price,
                    Want::Gear {
                        row,
                        item: it.item_id,
                        member,
                        ch,
                    },
                ));
            }
        }
    }
    if let Some((_, _, want)) = best {
        return Some(want);
    }
    // A purse that pays for the stock many times over buys up to three
    // stocks: past the Jette fortress the run carried 650000 gold through
    // `ropeway` and `station3`, passed both with the stock full, met no
    // counter after them, and fought Cort (Bio castle's end) out of an empty
    // bag three times running.
    let stock = u32::from(HEAL_STOCK) * members;
    let wealth = heal_row.map_or(1, |(_, _, price, _)| {
        (u32::try_from(gold).unwrap_or(0) / (price * stock * 4).max(1)).clamp(1, 3)
    });
    // And MP for the casts the long road and the boss call for: Vahn met
    // Cort with 22 MP and three Magic Leaves in the bag.
    heal_want(stock * wealth).or_else(|| {
        (wealth > 1)
            .then(|| mp_want(session, w, members * 5 * wealth))
            .flatten()
    })
}

/// The armament pieces in the bag the hand sells while an errand is short
/// of gold ([`errand_gold_short`]): those no present member is better off
/// wearing. A piece is kept when it is worth more (summed stat bonuses) to a
/// member who can wear it than what that member wears in its slot, and a
/// weapon also when it swings cheaper for that member than the worn one
/// (`arm_cost`, the specialty swap's measure). A player who walks into a
/// counter with a stack of outgrown armour sells it.
pub(super) fn surplus_gear(session: &NativeSession) -> BTreeSet<u8> {
    let w = &session.host.world;
    if !continuous::PLAYS_ON.with(std::cell::Cell::get) || !errand_gold_short(w) {
        return BTreeSet::new();
    }
    outgrown_gear(session, false)
}

/// What the bag's outgrown armament ([`surplus_gear`]'s pieces, whatever the
/// purse holds) sells for: half the item table's price, a copy.
pub(super) fn outgrown_value(session: &NativeSession) -> i64 {
    let w = &session.host.world;
    let Some(prices) = w.shops.item_shop_data.as_ref() else {
        return 0;
    };
    outgrown_gear(session, false)
        .iter()
        .map(|&id| {
            i64::from(prices.price(id)) / 2
                * i64::from(w.party.inventory.get(&id).copied().unwrap_or(0))
        })
        .sum()
}

/// The armament pieces in the bag no present member is better off wearing
/// ([`surplus_gear`] without its errand gate). With `joined`, no member who
/// has joined the party either, present or not: what a heal sale may part
/// with ([`heal_sale_gear`]).
fn outgrown_gear(session: &NativeSession, joined: bool) -> BTreeSet<u8> {
    use legaia_engine_menus::equipment::EquipSlot;
    use legaia_game_tables::equip_stats::EquipSlot as Disc;
    let w = &session.host.world;
    let mut out = BTreeSet::new();
    let (Some(info), Some(prices)) = (
        session.equip_restrictions.as_ref(),
        w.shops.item_shop_data.as_ref(),
    ) else {
        return out;
    };
    let party: Vec<u8> = if joined {
        (0..w.party.roster.members.len())
            .filter(|&i| w.party.roster.members[i].hp_mp_sp().hp_max > 0)
            .map(|i| i as u8)
            .collect()
    } else {
        w.present_party_list()
    };
    for (&id, &n) in w.party.inventory.iter() {
        let Some(entry) = info.entry(id) else {
            continue;
        };
        if n == 0 || entry.is_ra_seru || prices.price(id) == 0 {
            continue;
        }
        let slot = match entry.category {
            Disc::Weapon => EquipSlot::Weapon,
            Disc::Body => EquipSlot::BodyArmor,
            Disc::Head => EquipSlot::Helmet,
            Disc::Footwear => EquipSlot::Boot,
        };
        let wanted = party.iter().any(|&ch| {
            if !info.can_equip(id, ch) {
                return false;
            }
            let Some(rec) = w.party.roster.members.get(usize::from(ch)) else {
                return false;
            };
            let worn = if slot == EquipSlot::Weapon {
                worn_weapon(session, ch)
            } else {
                rec.equipment().slots[slot.as_index() as usize]
            };
            if gear_worth(w, id) > gear_worth(w, worn) {
                return true;
            }
            slot == EquipSlot::Weapon
                && match (arm_cost(session, ch, id), arm_cost(session, ch, worn)) {
                    (Some(c), Some(cw)) => c < cw,
                    // A cost the battle file cannot price is kept.
                    _ => true,
                }
        });
        if !wanted {
            out.insert(id);
        }
    }
    out
}

/// The pad edge that moves a kind-4 list cursor (seven rows a page, Left /
/// Right flip the page, Up / Down wrap inside it) from `cursor` toward
/// `row`, or Cross on it.
fn list_edge(cursor: usize, row: usize) -> u16 {
    const PAGE: usize = 7;
    if cursor / PAGE < row / PAGE {
        PadButton::Right.mask()
    } else if cursor / PAGE > row / PAGE {
        PadButton::Left.mask()
    } else if cursor < row {
        PadButton::Down.mask()
    } else if cursor > row {
        PadButton::Up.mask()
    } else {
        PadButton::Cross.mask()
    }
}

/// The shop screens' cancel: the menu VM backs a screen out on Triangle.
const CANCEL: u16 = PadButton::Triangle as u16;

/// The sell list row of the first bag stack in `sell`, if one is left.
fn sale_row(w: &legaia_engine_core::world::World, sell: &BTreeSet<u8>) -> Option<usize> {
    MenuRuntime::sell_list_rows(w)
        .iter()
        .position(|r| !r.dim && r.count > 0 && sell.contains(&r.id))
}

thread_local! {
    /// Shop edges the hand spent dismissing the Point Card toast
    /// ([`shop_edge`]) over a run.
    pub(super) static POINT_TOASTS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// One pad edge for the open shop screen. `sell` holds the bag items the
/// hand sells first ([`surplus_gear`]): Sell, the row, the whole stack.
fn shop_edge(menu: &MenuRuntime, w: &legaia_engine_core::world::World, sell: &BTreeSet<u8>) -> u16 {
    // The casino prize counter: the hand looks and leaves.
    if menu.prize_session.is_some() {
        return CANCEL;
    }
    // A buy made holding the Point Card parks on the card's toast (window
    // 31, `FUN_801DB7F4` case 4) until a confirm or a cancel.
    if menu.point_card_toast().is_some() {
        POINT_TOASTS.with(|n| n.set(n.get() + 1));
        return PadButton::Cross.mask();
    }
    let sale = sale_row(w, sell);
    if let Some(q) = menu.quantity_view()
        && !q.buying
    {
        // The whole stack: Down adds ten, Right one.
        return if !sell.contains(&q.item_id) {
            CANCEL
        } else if q.max.saturating_sub(q.quantity) >= 10 {
            PadButton::Down.mask()
        } else if q.quantity < q.max {
            PadButton::Right.mask()
        } else {
            PadButton::Cross.mask()
        };
    }
    match MenuState::from_byte(menu.ctx.state) {
        // Sell is the picker's second row.
        Some(MenuState::ShopMenu) if sale.is_some() && menu.quantity_view().is_none() => {
            return match menu.ctx.cursor {
                1 => PadButton::Cross.mask(),
                0 => PadButton::Down.mask(),
                _ => PadButton::Up.mask(),
            };
        }
        Some(MenuState::ShopSell) if menu.quantity_view().is_none() => {
            return match sale {
                Some(row) => list_edge(usize::from(menu.ctx.cursor), row),
                None => CANCEL,
            };
        }
        _ => {}
    }
    let want = next_want(menu, w);
    if let Some(q) = menu.quantity_view() {
        return match want {
            Some(Want::Stack { qty, .. }) if q.buying && q.quantity < qty.min(q.max) => {
                if qty.min(q.max) - q.quantity >= 10 {
                    PadButton::Down.mask()
                } else {
                    PadButton::Right.mask()
                }
            }
            Some(Want::Stack { .. }) if q.buying => PadButton::Cross.mask(),
            _ => CANCEL,
        };
    }
    if let Some(r) = menu.recipient_session.as_ref() {
        return match want {
            Some(Want::Gear { item, member, .. }) if item == r.item_id => {
                let row = member as u32 + 1;
                match (r.cursor & 0xFFF).cmp(&row) {
                    std::cmp::Ordering::Less => PadButton::Down.mask(),
                    std::cmp::Ordering::Greater => PadButton::Up.mask(),
                    std::cmp::Ordering::Equal => PadButton::Cross.mask(),
                }
            }
            _ => CANCEL,
        };
    }
    match MenuState::from_byte(menu.ctx.state) {
        Some(MenuState::ShopMenu) => match want {
            // Buy is the picker's first row.
            Some(_) if menu.ctx.cursor == 0 => PadButton::Cross.mask(),
            Some(_) => PadButton::Up.mask(),
            None => CANCEL,
        },
        Some(MenuState::ShopBuy) => match want {
            Some(Want::Gear { row, .. } | Want::Stack { row, .. }) => {
                list_edge(usize::from(menu.ctx.cursor), row)
            }
            None => CANCEL,
        },
        Some(
            MenuState::ShopSell
            | MenuState::ShopQuantity
            | MenuState::ShopConfirm
            | MenuState::ShopTrade
            | MenuState::ShopTradeConfirm,
        ) => CANCEL,
        // The exit beat and the close run themselves.
        _ => 0,
    }
}

/// Play the host's part for a shop the field VM just staged: open the
/// screen, shop on pad edges with the field frozen, and let the merchant's
/// script resume once the screen has closed. Every pass does this - the
/// record is parked on its shop op until a host closes the screen - and
/// only the continuous hand buys. A no-op with no shop pending or open.
pub(super) fn service_shop(session: &mut NativeSession) {
    let w = &session.host.world;
    if w.shops.pending_shop.is_none()
        && !w.shops.shop_open
        && w.shops.pending_prize_exchange.is_none()
        && !w.shops.prize_exchange_open
    {
        return;
    }
    let trace = std::env::var_os("LEGAIA_FGL_TRACE").is_some();
    let restrictions = session.equip_restrictions.clone();
    let mut sell = surplus_gear(session);
    let heal_sale = heal_sale_gear(session, true);
    SHOP_MENU.with(|m| {
        let mut slot = m.borrow_mut();
        let menu = slot.get_or_insert_with(|| {
            let mut r = MenuRuntime::new(std::env::temp_dir().join("legaia-fgl-shop"));
            if let Some(info) = restrictions {
                r.install_equip_info(info);
                r.retail_equipment_buy = true;
            }
            r
        });
        let w = &mut session.host.world;
        let (gold0, bag0) = (w.party.money, w.party.inventory.clone());
        let worn0: Vec<[u8; 8]> = w
            .party
            .roster
            .members
            .iter()
            .map(|m| m.equipment().slots)
            .collect();
        REFUSED_BUYS.with(|r| r.borrow_mut().clear());
        // The opening tick steps the screen on no edge, as both hosts do.
        let opened = menu.open_field_overlay_requests(w);
        let _ = menu.step_field_session(w, MenuRuntime::session_edge(opened, 0));
        // A counter that sells a heal takes the outgrown armour of a bag
        // short of its heal stock ([`heal_sale_gear`]).
        let sells_heal = menu.shop_session.as_ref().is_some_and(|s| {
            use legaia_engine_menus::items::ItemEffect;
            s.inventory.items.iter().any(|it| {
                it.price > 0
                    && w.tables.item_catalog.get(it.item_id).is_some_and(|e| {
                        matches!(e.effect, ItemEffect::Heal { .. } | ItemEffect::HealAll)
                    })
            })
        });
        if sells_heal {
            sell.extend(heal_sale.iter().copied());
        }
        let stock: Vec<String> = menu
            .shop_session
            .as_ref()
            .map(|s| {
                s.inventory
                    .items
                    .iter()
                    .map(|i| format!("{:02X}@{}", i.item_id, i.price))
                    .collect()
            })
            .unwrap_or_default();
        // A want that stays the same for many edges is one the screen will
        // not grant (a refused row): a piece is struck off for this visit,
        // a stack ends it.
        let mut streak: (Option<Want>, Option<usize>, u32) = (None, None, 0);
        for _ in 0..4000 {
            if !menu.is_open() {
                break;
            }
            let want = next_want(menu, w);
            // A sale in hand is its own progress: the stack's row moves or
            // goes as each one is sold.
            let sale = sale_row(w, &sell);
            if (want, sale) == (streak.0, streak.1) {
                streak.2 += 1;
            } else {
                streak = (want, sale, 0);
            }
            if streak.2 > 120 {
                match want {
                    // A sale that does not go through is struck off this
                    // visit's list, not the visit: ending it there left a
                    // purse of 20000 at `rayman2`'s item counter with two
                    // Healing Flowers bought.
                    _ if sale.is_some() => {
                        let stuck = sale.and_then(|r| {
                            MenuRuntime::sell_list_rows(w).get(r).map(|row| row.id)
                        });
                        match stuck {
                            Some(id) if sell.remove(&id) => {
                                streak = (None, None, 0);
                                continue;
                            }
                            _ => break,
                        }
                    }
                    Some(Want::Gear { item, ch, .. }) => {
                        REFUSED_BUYS.with(|r| r.borrow_mut().insert((item, ch)));
                        streak = (None, None, 0);
                        continue;
                    }
                    _ => break,
                }
            }
            let edge = shop_edge(menu, w, &sell);
            if std::env::var_os("LEGAIA_FGL_SHOP_DEBUG").is_some() {
                eprintln!(
                    "      [shop-debug] want {want:?} sale {sale:?} edge {edge:#06x} state {:?} gold {} heals {}",
                    MenuState::from_byte(menu.ctx.state),
                    w.party.money,
                    heals_held(w)
                );
            }
            let _ = menu.step_field_session(w, edge);
            let _ = menu.step_field_session(w, 0);
        }
        // Whatever is left open closes on cancel.
        for k in 0..400 {
            if !menu.is_open() {
                break;
            }
            let edge = if k % 8 == 0 {
                shop_edge(menu, w, &BTreeSet::new())
            } else {
                0
            };
            let _ = menu.step_field_session(w, edge);
        }
        let _ = menu.step_field_session(w, 0);
        SHOP_VISITED.with(|v| v.set(true));
        SHOP_OPENS.with(|n| n.set(n.get() + 1));
        if trace {
            let bought: Vec<String> = w
                .party
                .inventory
                .iter()
                .filter(|&(id, &n)| n > bag0.get(id).copied().unwrap_or(0))
                .map(|(id, &n)| format!("{id:02X}x{}", n - bag0.get(id).copied().unwrap_or(0)))
                .collect();
            let worn: Vec<[u8; 8]> = w
                .party
                .roster
                .members
                .iter()
                .map(|m| m.equipment().slots)
                .collect();
            let sold: Vec<String> = bag0
                .iter()
                .filter(|&(id, &n)| n > w.party.inventory.get(id).copied().unwrap_or(0))
                .map(|(id, &n)| format!("{id:02X}x{}", n - w.party.inventory.get(id).copied().unwrap_or(0)))
                .collect();
            eprintln!(
                "    [shop] stock [{}]; gold {gold0} -> {}; bag +[{}] -[{}]; {} member(s) re-equipped; screen {}",
                stock.join(" "),
                w.party.money,
                bought.join(" "),
                sold.join(" "),
                worn.iter().zip(&worn0).filter(|(a, b)| a != b).count(),
                if menu.is_open() { "STILL OPEN" } else { "closed" }
            );
        }
    });
}

/// The talk NPCs of the loaded scene whose record, or a record it spawns,
/// carries a priced shop stock list (op `0x49` sub-0).
pub(super) fn merchants(session: &NativeSession) -> Vec<u8> {
    merchants_selling(session, |_| true)
}

/// [`merchants`] whose stock list carries an item `sells` picks.
fn merchants_selling(session: &NativeSession, sells: impl Fn(u8) -> bool) -> Vec<u8> {
    use legaia_engine_field::man_field_scripts::partition_record_span;
    let w = &session.host.world;
    let Some(data) = w.shops.item_shop_data.as_ref() else {
        return Vec::new();
    };
    let Some((mf, man, _)) = scene_man_and_triggers(session) else {
        return Vec::new();
    };
    let mask = data.sellable_mask();
    // Each stock list's offset, and whether it sells a healing item.
    let stock: Vec<(usize, bool)> = legaia_scene_formats::shop_stock::scan(&man, Some(&mask))
        .iter()
        .filter(|r| {
            r.id_offsets
                .iter()
                .any(|&o| man.get(o).is_some_and(|&id| sells(id)))
        })
        .map(|r| {
            let heals = r.id_offsets.iter().any(|&o| {
                man.get(o).is_some_and(|&id| {
                    w.tables.item_catalog.get(id).is_some_and(|e| {
                        matches!(
                            e.effect,
                            legaia_engine_menus::items::ItemEffect::Heal { .. }
                        )
                    })
                })
            });
            (r.count_off, heals)
        })
        .collect();
    if stock.is_empty() {
        return Vec::new();
    }
    let span_has = |part: usize, rec: usize, heals_only: bool| {
        partition_record_span(&mf, &man, part, rec).is_some_and(|(start, _, len)| {
            stock
                .iter()
                .any(|&(o, h)| o >= start && o < start + len && (h || !heals_only))
        })
    };
    let holds = |part: usize, rec: usize| span_has(part, rec, false);
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let mut out: Vec<u8> = w
        .npcs
        .positions
        .keys()
        .copied()
        .filter(|s| w.npcs.dialog.contains_key(s) || w.npcs.dialog_prologue.contains_key(s))
        // A placement parked off the map is not in the scene yet.
        .filter(|s| {
            w.npcs.positions.get(s).is_some_and(|&(x, z)| {
                let t = dispatch_tile(x, z);
                t.0 < 126 && t.1 < 126
            })
        })
        .filter(|&s| {
            holds(1, usize::from(s))
                || spawned_p2(&mf, &man, 1, usize::from(s), n0 + n1, 3)
                    .iter()
                    .any(|&r| holds(2, r))
        })
        .collect();
    // The counter that sells healing items first: what the next boss is
    // fought with is bought before the armour (`retock`'s arms dealer
    // P1[34] took 46800 of 49991 gold, and its item counter P1[36] then
    // sold seven Healing Flowers).
    let sells_heals = |s: u8| {
        span_has(1, usize::from(s), true)
            || spawned_p2(&mf, &man, 1, usize::from(s), n0 + n1, 3)
                .iter()
                .any(|&r| span_has(2, r, true))
    };
    out.sort_unstable_by_key(|&s| (!sells_heals(s), s));
    out
}

/// The tiles of the loaded scene's merchants who sell an item an errand is
/// still short of ([`errands_short`]).
pub(super) fn errand_merchant_tiles(session: &NativeSession) -> Vec<(i16, i16)> {
    let short: Vec<u8> = errands_short(&session.host.world)
        .iter()
        .map(|&(item, _, _)| item)
        .collect();
    if short.is_empty() {
        return Vec::new();
    }
    let w = &session.host.world;
    merchants_selling(session, |id| short.contains(&id))
        .iter()
        .filter_map(|s| w.npcs.positions.get(s))
        .map(|&(x, z)| {
            let t = dispatch_tile(x, z);
            (t.0 as i16, t.1 as i16)
        })
        .collect()
}

/// Does a merchant of the loaded scene sell an errand item the bag is short
/// of, and does the purse pay for a copy?
pub(super) fn errand_buyable_here(session: &NativeSession) -> bool {
    let w = &session.host.world;
    errands_short(w)
        .iter()
        .any(|&(_, _, price)| i64::from(w.party.money) >= i64::from(price))
        && !errand_merchant_tiles(session).is_empty()
}

/// Visit the scene's merchants with the pad, as a player passing a shop
/// with gold to spend does: walk up to each, talk, and buy
/// ([`service_shop`] runs when the talk reaches the shop op). Once per
/// purse and bag in a scene. `Ok(Some(scene))` when a talk left the scene.
pub(super) fn pad_shop(session: &mut NativeSession) -> Result<Option<String>, String> {
    if session.host.world.mode != SceneMode::Field || !walking(session) || !released(session) {
        return Ok(None);
    }
    let name = scene_name(session);
    let w = &session.host.world;
    let key = (w.party.money, w.party.inventory.iter().count());
    // A scene shopped in with this purse and bag is passed by, unless its
    // counters were closed on story flags that have since come down.
    let reopened = counters_reopened(session);
    // An errand short of gold sells what the bag has outgrown at any
    // counter, and buys its item wherever a counter sells it.
    let errand = !surplus_gear(session).is_empty() || errand_buyable_here(session);
    if (w.party.money <= 0 && !errand)
        || (!reopened && !errand && SHOPPED.with(|s| s.borrow().get(&name) == Some(&key)))
    {
        return Ok(None);
    }
    let keepers = merchants(session);
    if keepers.is_empty() {
        return Ok(None);
    }
    let trace = std::env::var_os("LEGAIA_FGL_TRACE").is_some();
    if trace {
        eprintln!(
            "    [shop] {name}: merchants {keepers:?}, gold {}",
            session.host.world.party.money
        );
    }
    let errand_before = errand_cost(&session.host.world);
    let mut closed = visit_merchants(session, &name, &keepers)?;
    if let Some(left) = closed.1.take() {
        return Ok(Some(left));
    }
    // The counter that sells heals is visited first, while the purse is
    // still the errand's; once a later counter has sold the errand its item,
    // the change goes on heals. The Sol run bought its bread at `koin4`'s
    // bakery after the item counter, left with 7000 gold, and met Gaza
    // (`korb3`) with a handful of Healing Flowers.
    let w = &session.host.world;
    if errand_cost(w) < errand_before
        && heals_held(w) < u32::from(HEAL_STOCK) * w.present_party_list().len().clamp(1, 3) as u32
    {
        let healers = merchants_selling(session, |id| {
            session
                .host
                .world
                .tables
                .item_catalog
                .get(id)
                .is_some_and(|e| {
                    matches!(
                        e.effect,
                        legaia_engine_menus::items::ItemEffect::Heal { .. }
                            | legaia_engine_menus::items::ItemEffect::HealAll
                    )
                })
        });
        if !healers.is_empty() {
            let again = visit_merchants(session, &name, &healers)?;
            if let Some(left) = again.1 {
                return Ok(Some(left));
            }
        }
    }
    // A counter that only talked is closed by the story (`dolk2`'s dealers
    // call out their sale and sell nothing while the festival flag `0x27D`
    // is up): the hand plays what ends that, then comes back.
    CLOSED_ON.with(|c| c.borrow_mut().remove(&name));
    if !closed.0.is_empty() {
        let before = flags_of_world(session);
        let (held, left) = open_counters(session, &closed.0)?;
        if let Some(s) = left {
            return Ok(Some(s));
        }
        if flags_of_world(session) != before && scene_name(session) == name {
            let again = merchants(session);
            closed = visit_merchants(session, &name, &again)?;
            if let Some(left) = closed.1.take() {
                return Ok(Some(left));
            }
        }
        // Still closed: the hand comes back once a flag it is closed on
        // has come down (the way out of `dolk2` ends the festival too).
        if !closed.0.is_empty() && !held.is_empty() {
            CLOSED_ON.with(|c| c.borrow_mut().insert(name.clone(), held));
        }
    }
    let w = &session.host.world;
    let key = (w.party.money, w.party.inventory.iter().count());
    SHOPPED.with(|s| s.borrow_mut().insert(name, key));
    Ok(None)
}

/// Were the loaded scene's counters last found closed on a story flag that
/// has since come down?
pub(super) fn counters_reopened(session: &NativeSession) -> bool {
    let w = &session.host.world;
    CLOSED_ON.with(|c| {
        c.borrow()
            .get(&scene_name(session))
            .is_some_and(|held| held.iter().any(|&f| !w.system_flag_test(f)))
    })
}

/// Talk to each merchant in turn. Returns the ones whose talk opened no
/// shop screen, and the scene a talk left for.
fn visit_merchants(
    session: &mut NativeSession,
    name: &str,
    keepers: &[u8],
) -> Result<(Vec<u8>, Option<String>), String> {
    let trace = std::env::var_os("LEGAIA_FGL_TRACE").is_some();
    let mut closed = Vec::new();
    for &slot in keepers {
        pad_budget(session)?;
        take_shop_visited();
        let opens = SHOP_OPENS.with(std::cell::Cell::get);
        SHOPPING.with(|s| s.set(true));
        let r = pad_talk_to(session, slot);
        SHOPPING.with(|s| s.set(false));
        if trace {
            eprintln!("    [shop] {name} talk P1[{slot}] -> {r:?}");
        }
        match r {
            Run::Entered(s) => return Ok((closed, Some(s))),
            Run::Battle(b) => return Err(format!("shop talk P1[{slot}]: {b}")),
            Run::Error(e) => return Err(e),
            Run::Released | Run::Parked(_) => {}
        }
        if let Some(w) = wiped(session) {
            return Err(w);
        }
        if SHOP_OPENS.with(std::cell::Cell::get) == opens {
            closed.push(slot);
        }
    }
    Ok((closed, None))
}

/// Play what re-opens the counters of `closed`, the merchants whose talk
/// sold nothing. A closed counter's record tests a story flag that is up
/// and branches round its shop op; the scene lowers that flag again in some
/// record. The hand talks to the NPC whose record, or a record it spawns,
/// does (a staged talk, one stage a contact), and failing that steps onto
/// the walk-on band whose record does. Returns the flags the counters are
/// closed on that the scene can lower, and the scene a beat left for.
fn open_counters(
    session: &mut NativeSession,
    closed: &[u8],
) -> Result<(BTreeSet<u16>, Option<String>), String> {
    use legaia_engine_field::man_field_scripts::{FlagBank, walk_partition_gflag_sites};
    let Some((mf, man, triggers)) = scene_man_and_triggers(session) else {
        return Ok((BTreeSet::new(), None));
    };
    let trace = std::env::var_os("LEGAIA_FGL_TRACE").is_some();
    let name = scene_name(session);
    let sites: Vec<_> = (1..3)
        .flat_map(|part| walk_partition_gflag_sites(&mf, &man, part))
        .filter(|s| s.clean && s.bank == FlagBank::System)
        .collect();
    let held: BTreeSet<u16> = {
        let w = &session.host.world;
        sites
            .iter()
            .filter(|s| {
                s.partition == 1
                    && s.kind == FlagKind::Test
                    && closed.iter().any(|&c| usize::from(c) == s.record)
                    && w.system_flag_test(s.flag)
            })
            .map(|s| s.flag)
            .collect()
    };
    let lowers = |part: usize, held: &BTreeSet<u16>| -> BTreeSet<usize> {
        sites
            .iter()
            .filter(|s| s.partition == part && s.kind == FlagKind::Clear && held.contains(&s.flag))
            .map(|s| s.record)
            .collect()
    };
    // Only the flags some record of the scene lowers again.
    let held: BTreeSet<u16> = held
        .into_iter()
        .filter(|f| {
            sites
                .iter()
                .any(|s| s.kind == FlagKind::Clear && s.flag == *f)
        })
        .collect();
    if held.is_empty() {
        return Ok((held, None));
    }
    let (low1, low2) = (lowers(1, &held), lowers(2, &held));
    let up = |s: &NativeSession| held.iter().any(|&f| s.host.world.system_flag_test(f));
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let reaches = |part: usize, rec: usize| {
        (part == 2 && low2.contains(&rec))
            || spawned_p2(&mf, &man, part, rec, n0 + n1, 3)
                .iter()
                .any(|r| low2.contains(r))
    };
    let mut talkers: Vec<u8> = {
        let w = &session.host.world;
        w.npcs
            .positions
            .keys()
            .copied()
            .filter(|s| !closed.contains(s))
            .filter(|s| w.npcs.dialog.contains_key(s) || w.npcs.dialog_prologue.contains_key(s))
            .filter(|s| {
                w.npcs.positions.get(s).is_some_and(|&(x, z)| {
                    let t = dispatch_tile(x, z);
                    t.0 < 126 && t.1 < 126
                })
            })
            .filter(|&s| low1.contains(&usize::from(s)) || reaches(1, usize::from(s)))
            .collect()
    };
    talkers.sort_unstable();
    if trace {
        eprintln!(
            "    [shop] {name}: counters {closed:?} closed on {held:03X?}; lowered by P1{low1:?} P2{low2:?}; talks {talkers:?}"
        );
    }
    for slot in talkers {
        // A staged talk plays one stage a contact.
        for _ in 0..6 {
            if !up(session) || !walking(session) || scene_name(session) != name {
                break;
            }
            pad_budget(session)?;
            let before = flags_of_world(session);
            let r = pad_talk_to(session, slot);
            if trace {
                eprintln!("    [shop] {name} talk P1[{slot}] (to open the counters) -> {r:?}");
            }
            match r {
                Run::Entered(s) => return Ok((held, Some(s))),
                Run::Battle(b) => return Err(format!("talk P1[{slot}]: {b}")),
                Run::Error(e) => return Err(e),
                Run::Released | Run::Parked(_) => {}
            }
            if let Some(w) = wiped(session) {
                return Err(w);
            }
            if flags_of_world(session) == before {
                break;
            }
        }
    }
    if !up(session) || !walking(session) || scene_name(session) != name {
        return Ok((held, None));
    }
    let mut bands: Vec<(u8, (u8, u8))> = Vec::new();
    for t in triggers.iter().filter(|t| t.gate == 1) {
        if reaches(2, usize::from(t.record)) && !bands.iter().any(|b| b.0 == t.record) {
            bands.push((t.record, (t.tile_x, t.tile_z)));
        }
    }
    for (rec, tile) in bands {
        if !up(session) {
            break;
        }
        pad_budget(session)?;
        let r = match pad_step_onto(session, tile) {
            Ok(Walk::Entered(s)) => Run::Entered(s),
            Ok(Walk::Arrived) => run_while_moving(session, DEEP_EXIT_TICKS),
            Err(e) => Run::Parked(e),
        };
        if trace {
            eprintln!(
                "    [shop] {name} band P2[{rec}] at {tile:?} (to open the counters) -> {r:?}"
            );
        }
        match r {
            Run::Entered(s) => return Ok((held, Some(s))),
            Run::Battle(b) => return Err(format!("band P2[{rec}]: {b}")),
            Run::Error(e) => return Err(e),
            Run::Released | Run::Parked(_) => {}
        }
        if let Some(w) = wiped(session) {
            return Err(w);
        }
    }
    Ok((held, None))
}

/// Forget which scenes were shopped in: a new run, or a reload.
pub(super) fn reset_shopping() {
    SHOPPED.with(|s| s.borrow_mut().clear());
    CLOSED_ON.with(|c| c.borrow_mut().clear());
    SHOP_MENU.with(|m| *m.borrow_mut() = None);
}
