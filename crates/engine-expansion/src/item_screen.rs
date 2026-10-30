//! The pack item screen: a shop counter (Buy / Sell) and the pack bag
//! (Items / Gear), with the stat-compare panel of the equip step.
//!
//! The screen is `World`-free: [`ItemScreen::step`] reads and writes the
//! game through an [`ItemScreenHost`] (`engine-core` implements it over the
//! `World`), and [`ItemScreen::model`] projects what to draw as plain rows
//! the `engine-ui` draw builder takes. It owns the frame under
//! `SceneMode::Expansion`, as the card screen does, so both play hosts draw
//! it through the same seat.
//!
//! Retail items a pack shop stocks are bought into the retail bag at the
//! retail price; pack items into the pack bag. The Sell tab lists pack items
//! only - retail items sell at retail shops.

use std::sync::Arc;

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::input::PadButton;
use legaia_expansion_format::items::{ItemKind, ShopDef};
use legaia_expansion_format::quest::Prerequisites;

use crate::items::{CompiledItems, ItemKey, ItemProgress, STACK_CAP, equip_indices, slot_label};

/// A party member as the screen lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Member {
    /// Roster slot (`0` Vahn, `1` Noa, `2` Gala).
    pub roster: u8,
    pub name: String,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
}

/// What the screen knows about a retail item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetailInfo {
    pub name: String,
    pub description: String,
    /// The retail buy price.
    pub price: u32,
    pub held: u8,
}

/// A character's equipment-fed stats, for the compare panel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatLine {
    pub atk: u16,
    pub udf: u16,
    pub ldf: u16,
    pub spd: u16,
    pub int: u16,
}

impl StatLine {
    fn rows(&self) -> [(&'static str, u16); 5] {
        [
            ("ATK", self.atk),
            ("UDF", self.udf),
            ("LDF", self.ldf),
            ("SPD", self.spd),
            ("INT", self.int),
        ]
    }
}

/// The game the screen runs against.
pub trait ItemScreenHost {
    fn gold(&self) -> u32;
    /// The present party, in party order.
    fn members(&self) -> Vec<Member>;
    fn retail_info(&self, id: u8) -> RetailInfo;
    fn prerequisites_hold(&self, p: &Prerequisites) -> bool;
    fn compiled(&self) -> Option<Arc<CompiledItems>>;
    fn progress(&self) -> &ItemProgress;
    fn shop(&self, id: &str) -> Option<ShopDef>;
    /// Roster slot `roster`'s stats, with equipment byte `preview.0` holding
    /// the pack item `preview.1` (`None` empties it) when previewing.
    fn stats(&self, roster: u8, preview: Option<(u8, Option<&str>)>) -> StatLine;
    /// The name of what roster slot `roster` wears in byte `index` (a retail
    /// or a pack item), empty for nothing.
    fn worn_name(&self, roster: u8, index: u8) -> String;
    /// Spend `cost` gold and add `qty` of `item`. `false` changes nothing.
    fn buy(&mut self, item: &ItemKey, qty: u8, cost: u32) -> bool;
    /// Take `qty` pack `key` and add `gain` gold.
    fn sell(&mut self, key: &str, qty: u8, gain: u32) -> bool;
    /// Use one pack consumable `key` on roster slot `roster`: the outcome
    /// line, or `None` when it would do nothing (nothing is spent).
    fn use_item(&mut self, key: &str, roster: u8) -> Option<String>;
    /// Wear pack `key` on roster slot `roster` in byte `index`; whatever the
    /// slot held goes back to its bag.
    fn equip(&mut self, key: &str, roster: u8, index: u8) -> bool;
    /// Take the pack item off byte `index`.
    fn unequip(&mut self, roster: u8, index: u8) -> bool;
}

/// Which counter or list the screen opened as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenKind {
    /// A pack shop by id.
    Shop { shop: String },
    /// The pack bag.
    Bag,
}

/// The screen's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Buy,
    Sell,
    Items,
    Gear,
}

impl Tab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Buy => "Buy",
            Self::Sell => "Sell",
            Self::Items => "Items",
            Self::Gear => "Gear",
        }
    }
}

/// Where the cursor is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    List {
        cursor: usize,
    },
    /// Choosing how many of row `row` to buy or sell.
    Quantity {
        row: usize,
        qty: u8,
    },
    /// Choosing who uses or wears row `row`.
    Target {
        row: usize,
        cursor: usize,
    },
}

/// The open screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemScreen {
    pub kind: ScreenKind,
    pub tab: Tab,
    pub stage: Stage,
    /// The last action's line and the frames it stays up.
    pub message: Option<(String, u16)>,
    /// Buttons held when the screen opened, ignored until released.
    held_over: u16,
}

/// The screen while it owns the frame.
#[derive(Debug, Clone)]
pub struct ItemSeat {
    /// The mode the world returns to when the screen closes.
    pub return_mode: SceneMode,
    pub screen: ItemScreen,
}

/// Whether the screen stays up after a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenStep {
    Stay,
    Leave,
}

/// Frames a message line stays up.
const MESSAGE_FRAMES: u16 = 120;

/// One row of the current tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub item: ItemKey,
    pub name: String,
    pub description: String,
    /// The buy price (Buy) or sell price (Sell); `0` for the bag tabs.
    pub price: u32,
    pub held: u8,
    /// The pack item's kind; `None` for a retail item.
    pub kind: Option<ItemKind>,
    /// Bag tabs: the heroes wearing it.
    pub worn_by: Vec<String>,
}

impl ItemScreen {
    fn new(kind: ScreenKind, tab: Tab) -> Self {
        Self {
            kind,
            tab,
            stage: Stage::List { cursor: 0 },
            message: None,
            held_over: u16::MAX,
        }
    }

    /// A shop's counter, on the Buy tab.
    pub fn shop(id: &str) -> Self {
        Self::new(
            ScreenKind::Shop {
                shop: id.to_string(),
            },
            Tab::Buy,
        )
    }

    /// The pack bag, on the Gear tab when `gear`.
    pub fn bag(gear: bool) -> Self {
        Self::new(ScreenKind::Bag, if gear { Tab::Gear } else { Tab::Items })
    }

    fn tabs(&self) -> [Tab; 2] {
        match self.kind {
            ScreenKind::Shop { .. } => [Tab::Buy, Tab::Sell],
            ScreenKind::Bag => [Tab::Items, Tab::Gear],
        }
    }

    fn say(&mut self, line: impl Into<String>) {
        self.message = Some((line.into(), MESSAGE_FRAMES));
    }

    /// The current tab's rows.
    pub fn rows(&self, host: &dyn ItemScreenHost) -> Vec<Row> {
        let Some(c) = host.compiled() else {
            return Vec::new();
        };
        let progress = host.progress();
        let members = host.members();
        let worn_by = |key: &str| -> Vec<String> {
            progress
                .worn
                .iter()
                .filter(|(_, k)| k.as_str() == key)
                .map(|(&(r, _), _)| {
                    members
                        .iter()
                        .find(|m| m.roster == r)
                        .map_or_else(|| format!("#{r}"), |m| m.name.clone())
                })
                .collect()
        };
        let pack_row = |key: &str, price: u32| {
            c.get(key).map(|it| Row {
                item: ItemKey::Pack(key.to_string()),
                name: it.def.name.clone(),
                description: it.def.description.clone(),
                price,
                held: progress.count(key),
                kind: Some(it.def.kind),
                worn_by: worn_by(key),
            })
        };
        match self.tab {
            Tab::Buy => {
                let ScreenKind::Shop { shop } = &self.kind else {
                    return Vec::new();
                };
                let Some(shop) = host.shop(shop) else {
                    return Vec::new();
                };
                let pack_id = String::new();
                shop.stock
                    .iter()
                    .filter(|e| e.requires().is_none_or(|p| host.prerequisites_hold(p)))
                    .filter_map(|e| {
                        // Resolve against the pack's own keys: a bare key or
                        // one qualified with any id names the local item.
                        let key = match e.item().pack_key() {
                            Some(k) => ItemKey::Pack(k.rsplit(':').next().unwrap_or(k).to_string()),
                            None => ItemKey::resolve(&pack_id, e.item())?,
                        };
                        match key {
                            ItemKey::Retail(id) => {
                                let r = host.retail_info(id);
                                let price = e.price().unwrap_or(r.price);
                                (price > 0).then_some(Row {
                                    item: ItemKey::Retail(id),
                                    name: r.name,
                                    description: r.description,
                                    price,
                                    held: r.held,
                                    kind: None,
                                    worn_by: Vec::new(),
                                })
                            }
                            ItemKey::Pack(k) => {
                                let price = e
                                    .price()
                                    .unwrap_or_else(|| c.get(&k).map_or(0, |i| i.def.price));
                                pack_row(&k, price).filter(|r| r.price > 0)
                            }
                        }
                    })
                    .collect()
            }
            Tab::Sell => progress
                .bag
                .keys()
                .filter_map(|k| {
                    let it = c.get(k)?;
                    pack_row(k, it.def.sell_price()).filter(|r| r.price > 0)
                })
                .collect(),
            Tab::Items => progress
                .bag
                .keys()
                .filter(|k| c.get(k).is_some_and(|i| !i.def.kind.is_equipment()))
                .filter_map(|k| pack_row(k, 0))
                .collect(),
            Tab::Gear => {
                let mut keys: Vec<&str> = c
                    .items
                    .values()
                    .filter(|i| i.def.kind.is_equipment())
                    .filter(|i| progress.count(&i.key) > 0 || progress.worn_count(&i.key) > 0)
                    .map(|i| i.key.as_str())
                    .collect();
                keys.sort_by_key(|k| c.get(k).map(|i| i.def.kind as u8));
                keys.into_iter().filter_map(|k| pack_row(k, 0)).collect()
            }
        }
    }

    /// One frame of input. `Leave` closes the screen.
    pub fn step(&mut self, pad: u16, prev: u16, host: &mut dyn ItemScreenHost) -> ScreenStep {
        self.held_over &= pad;
        let live = pad & !self.held_over;
        let pressed = |b: PadButton| live & !prev & b.mask() != 0;
        if let Some((_, n)) = &mut self.message {
            *n = n.saturating_sub(1);
            if *n == 0 {
                self.message = None;
            }
        }
        if pressed(PadButton::Start) {
            return ScreenStep::Leave;
        }
        let rows = self.rows(host);
        match self.stage {
            Stage::List { cursor } => {
                let n = rows.len();
                let mut cursor = cursor.min(n.saturating_sub(1));
                if pressed(PadButton::Circle) {
                    return ScreenStep::Leave;
                }
                let tabs = self.tabs();
                if pressed(PadButton::Left)
                    || pressed(PadButton::Right)
                    || pressed(PadButton::L1)
                    || pressed(PadButton::R1)
                {
                    self.tab = if self.tab == tabs[0] {
                        tabs[1]
                    } else {
                        tabs[0]
                    };
                    self.stage = Stage::List { cursor: 0 };
                    return ScreenStep::Stay;
                }
                if n > 0 {
                    if pressed(PadButton::Up) {
                        cursor = (cursor + n - 1) % n;
                    }
                    if pressed(PadButton::Down) {
                        cursor = (cursor + 1) % n;
                    }
                }
                self.stage = Stage::List { cursor };
                if pressed(PadButton::Cross)
                    && let Some(row) = rows.get(cursor)
                {
                    self.confirm_row(cursor, row, host);
                }
            }
            Stage::Quantity { row, qty } => {
                let Some(r) = rows.get(row) else {
                    self.stage = Stage::List { cursor: 0 };
                    return ScreenStep::Stay;
                };
                let max = self.max_quantity(r, host).max(1);
                let mut qty = qty.clamp(1, max);
                if pressed(PadButton::Right) || pressed(PadButton::Up) {
                    qty = if pressed(PadButton::Up) {
                        (qty + 10).min(max)
                    } else {
                        qty % max + 1
                    };
                }
                if pressed(PadButton::Left) || pressed(PadButton::Down) {
                    qty = if pressed(PadButton::Down) {
                        qty.saturating_sub(10).max(1)
                    } else if qty == 1 {
                        max
                    } else {
                        qty - 1
                    };
                }
                self.stage = Stage::Quantity { row, qty };
                if pressed(PadButton::Circle) {
                    self.stage = Stage::List { cursor: row };
                } else if pressed(PadButton::Cross) {
                    self.trade(r, qty, host);
                    self.stage = Stage::List { cursor: row };
                }
            }
            Stage::Target { row, cursor } => {
                let members = host.members();
                let Some(r) = rows.get(row) else {
                    self.stage = Stage::List { cursor: 0 };
                    return ScreenStep::Stay;
                };
                let n = members.len();
                let mut cursor = cursor.min(n.saturating_sub(1));
                if n > 0 {
                    if pressed(PadButton::Up) || pressed(PadButton::Left) {
                        cursor = (cursor + n - 1) % n;
                    }
                    if pressed(PadButton::Down) || pressed(PadButton::Right) {
                        cursor = (cursor + 1) % n;
                    }
                }
                self.stage = Stage::Target { row, cursor };
                if pressed(PadButton::Circle) {
                    self.stage = Stage::List { cursor: row };
                } else if pressed(PadButton::Cross)
                    && let Some(m) = members.get(cursor)
                {
                    self.apply_to(r, m, host);
                }
            }
        }
        ScreenStep::Stay
    }

    fn max_quantity(&self, r: &Row, host: &dyn ItemScreenHost) -> u8 {
        match self.tab {
            Tab::Buy => {
                let room = STACK_CAP.saturating_sub(r.held);
                let afford = host.gold().checked_div(r.price).unwrap_or(0);
                room.min(afford.min(u32::from(STACK_CAP)) as u8)
            }
            Tab::Sell => r.held,
            _ => 1,
        }
    }

    fn confirm_row(&mut self, cursor: usize, row: &Row, host: &mut dyn ItemScreenHost) {
        match self.tab {
            Tab::Buy | Tab::Sell => {
                if self.max_quantity(row, host) == 0 {
                    self.say(match self.tab {
                        Tab::Buy if row.held >= STACK_CAP => "You can't carry any more.",
                        Tab::Buy => "Not enough gold.",
                        _ => "You have none to sell.",
                    });
                } else {
                    self.stage = Stage::Quantity {
                        row: cursor,
                        qty: 1,
                    };
                }
            }
            Tab::Items => match row.kind {
                Some(ItemKind::Consumable) => {
                    let usable = row
                        .item
                        .pack()
                        .and_then(|k| host.compiled()?.get(k).cloned());
                    if usable.is_some_and(|i| i.def.field_usable()) {
                        self.stage = Stage::Target {
                            row: cursor,
                            cursor: 0,
                        };
                    } else {
                        self.say("It can only be used in battle.");
                    }
                }
                _ => self.say("It can't be used here."),
            },
            Tab::Gear => {
                self.stage = Stage::Target {
                    row: cursor,
                    cursor: 0,
                };
            }
        }
    }

    fn trade(&mut self, r: &Row, qty: u8, host: &mut dyn ItemScreenHost) {
        let total = r.price.saturating_mul(u32::from(qty));
        match self.tab {
            Tab::Buy => {
                if host.buy(&r.item, qty, total) {
                    self.say(format!("Bought {qty} {} for {total} G.", r.name));
                } else {
                    self.say("Not enough gold.");
                }
            }
            Tab::Sell => {
                if let Some(k) = r.item.pack()
                    && host.sell(k, qty, total)
                {
                    self.say(format!("Sold {qty} {} for {total} G.", r.name));
                }
            }
            _ => {}
        }
    }

    fn apply_to(&mut self, r: &Row, m: &Member, host: &mut dyn ItemScreenHost) {
        let Some(key) = r.item.pack() else {
            return;
        };
        let Some(c) = host.compiled() else {
            return;
        };
        let Some(it) = c.get(key) else {
            return;
        };
        if self.tab == Tab::Items {
            match host.use_item(key, m.roster) {
                Some(line) => self.say(line),
                None => self.say("It would have no effect."),
            }
            if host.progress().count(key) == 0 {
                self.stage = Stage::List { cursor: 0 };
            }
            return;
        }
        if !it.wearable_by(m.roster) {
            self.say(format!("{} can't wear the {}.", m.name, it.def.name));
            return;
        }
        let progress = host.progress();
        let indices = equip_indices(it.def.kind, m.roster);
        if let Some(&i) = indices
            .iter()
            .find(|&&i| progress.worn_at(m.roster, i) == Some(key))
        {
            if host.unequip(m.roster, i) {
                self.say(format!("{} took off the {}.", m.name, it.def.name));
            }
            return;
        }
        if progress.count(key) == 0 {
            self.say(format!("No {} left to wear.", it.def.name));
            return;
        }
        let Some(index) = target_index(host, key, m.roster) else {
            return;
        };
        if host.equip(key, m.roster, index) {
            self.say(format!("{} equipped the {}.", m.name, it.def.name));
        }
    }

    /// What to draw.
    pub fn model(&self, host: &dyn ItemScreenHost) -> ScreenModel {
        let rows = self.rows(host);
        let title = match &self.kind {
            ScreenKind::Shop { shop } => host
                .shop(shop)
                .map(|s| s.name)
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "Shop".into()),
            ScreenKind::Bag => "Pack Items".into(),
        };
        let tabs = self.tabs();
        let cursor = match self.stage {
            Stage::List { cursor } => cursor,
            Stage::Quantity { row, .. } | Stage::Target { row, .. } => row,
        }
        .min(rows.len().saturating_sub(1));
        let is_shop = matches!(self.kind, ScreenKind::Shop { .. });
        let list: Vec<RowModel> = rows
            .iter()
            .map(|r| RowModel {
                name: r.name.clone(),
                right: if is_shop {
                    format!("{} G", r.price)
                } else {
                    format!("x{}", r.held)
                },
                sub: match (&r.kind, is_shop) {
                    (_, true) => format!("Have {}", r.held),
                    (Some(k), false) if k.is_equipment() && !r.worn_by.is_empty() => {
                        format!("E {}", r.worn_by.join(", "))
                    }
                    (Some(k), false) => k.label().to_string(),
                    (None, false) => String::new(),
                },
                dim: match self.tab {
                    Tab::Buy => r.price > host.gold() || r.held >= STACK_CAP,
                    Tab::Gear | Tab::Items => r.held == 0,
                    Tab::Sell => false,
                },
            })
            .collect();
        let mut detail = Vec::new();
        let mut compare = None;
        let mut members_model = None;
        if let Some(r) = rows.get(cursor) {
            detail.extend(r.description.split('|').map(str::to_string));
            if let Some(it) = r.item.pack().and_then(|k| host.compiled()?.get(k).cloned()) {
                let mut s = it.def.kind.label().to_string();
                let m = &it.modifier;
                for (label, v) in [
                    ("ATK", m.atk),
                    ("UDF", m.udf),
                    ("LDF", m.ldf),
                    ("SPD", m.spd),
                    ("INT", m.int),
                ] {
                    if v != 0 {
                        s.push_str(&format!("  {label} {v:+}"));
                    }
                }
                detail.push(s);
                if it.def.kind.is_equipment() {
                    let who: Vec<&str> = ["Vahn", "Noa", "Gala"]
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| it.def.equip_mask() & (1 << i) != 0)
                        .map(|(_, n)| *n)
                        .collect();
                    let mut line = format!("Worn by {}", who.join(" / "));
                    if let Some(set) = it
                        .def
                        .set
                        .as_ref()
                        .and_then(|s| host.compiled()?.sets.get(s).cloned())
                    {
                        line.push_str(&format!("   Set: {}", set.name));
                    }
                    detail.push(line);
                }
            }
            if let Stage::Target { cursor: mc, .. } = self.stage {
                let members = host.members();
                let gear = self.tab == Tab::Gear;
                let key = r.item.pack().unwrap_or_default();
                members_model = Some((
                    members
                        .iter()
                        .map(|m| MemberRow {
                            name: m.name.clone(),
                            line: if gear {
                                let idx = target_index(host, key, m.roster);
                                idx.map(|i| {
                                    let now = host.worn_name(m.roster, i);
                                    format!(
                                        "{}: {}",
                                        slot_label(i, m.roster),
                                        if now.is_empty() { "-".into() } else { now }
                                    )
                                })
                                .unwrap_or_else(|| "Can't wear it".into())
                            } else {
                                format!("HP {}/{}  MP {}/{}", m.hp, m.hp_max, m.mp, m.mp_max)
                            },
                            dim: gear
                                && !host
                                    .compiled()
                                    .and_then(|c| c.get(key).map(|i| i.wearable_by(m.roster)))
                                    .unwrap_or(false),
                        })
                        .collect(),
                    mc.min(members.len().saturating_sub(1)),
                ));
                if gear
                    && let Some(m) = members.get(mc)
                    && let Some(index) = target_index(host, key, m.roster)
                {
                    let wearing = host.progress().worn_at(m.roster, index) == Some(key);
                    let now = host.stats(m.roster, None);
                    let after = host.stats(
                        m.roster,
                        Some((index, if wearing { None } else { Some(key) })),
                    );
                    compare = Some(CompareModel {
                        heading: format!(
                            "{}: {}",
                            m.name,
                            if wearing { "take off" } else { "equip" }
                        ),
                        rows: now
                            .rows()
                            .iter()
                            .zip(after.rows())
                            .map(|(&(l, a), (_, b))| (l, a, b))
                            .collect(),
                    });
                }
            }
        }
        let quantity = match self.stage {
            Stage::Quantity { row, qty } => rows
                .get(row)
                .map(|r| (qty, r.price.saturating_mul(u32::from(qty)))),
            _ => None,
        };
        let footer = match self.stage {
            Stage::List { .. } if is_shop => "L/R: Buy/Sell  X: choose  O: leave",
            Stage::List { .. } => "L/R: Items/Gear  X: choose  O: leave",
            Stage::Quantity { .. } => "Left/Right: 1  Up/Down: 10  X: confirm  O: back",
            Stage::Target { .. } if self.tab == Tab::Gear => "X: equip / take off  O: back",
            Stage::Target { .. } => "X: use  O: back",
        };
        ScreenModel {
            title,
            gold: host.gold(),
            tabs: tabs.iter().map(|t| t.label()).collect(),
            tab: tabs.iter().position(|&t| t == self.tab).unwrap_or(0),
            rows: list,
            cursor,
            empty: match self.tab {
                Tab::Buy => "Nothing for sale yet.",
                Tab::Sell => "You have no pack items to sell.",
                Tab::Items => "No pack items.",
                Tab::Gear => "No pack equipment.",
            },
            detail,
            quantity,
            members: members_model,
            compare,
            message: self.message.as_ref().map(|(l, _)| l.clone()),
            footer,
        }
    }
}

/// The byte a wear of `key` by `roster` lands in: the byte it already
/// occupies, else the first empty Goods slot for an accessory (the first
/// Goods slot when all three are full), else the kind's own slot.
fn target_index(host: &dyn ItemScreenHost, key: &str, roster: u8) -> Option<u8> {
    let c = host.compiled()?;
    let it = c.get(key)?;
    let indices = equip_indices(it.def.kind, roster);
    let progress = host.progress();
    if let Some(&i) = indices
        .iter()
        .find(|&&i| progress.worn_at(roster, i) == Some(key))
    {
        return Some(i);
    }
    indices
        .iter()
        .copied()
        .find(|&i| host.worn_name(roster, i).is_empty())
        .or_else(|| indices.first().copied())
}

/// One list row to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowModel {
    pub name: String,
    /// The right-aligned column: a price or a count.
    pub right: String,
    /// The small line under the name column (`Weapon`, `E Vahn`, `Have 2`).
    pub sub: String,
    pub dim: bool,
}

/// One member row of the target step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRow {
    pub name: String,
    pub line: String,
    pub dim: bool,
}

/// The equip step's stat compare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompareModel {
    pub heading: String,
    /// `(label, now, after)`.
    pub rows: Vec<(&'static str, u16, u16)>,
}

/// Everything the item screen draws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenModel {
    pub title: String,
    pub gold: u32,
    pub tabs: Vec<&'static str>,
    pub tab: usize,
    pub rows: Vec<RowModel>,
    pub cursor: usize,
    pub empty: &'static str,
    /// The selected row's description and stat lines.
    pub detail: Vec<String>,
    /// `(quantity, total price)` while choosing a quantity.
    pub quantity: Option<(u8, u32)>,
    /// `(rows, cursor)` while choosing a member.
    pub members: Option<(Vec<MemberRow>, usize)>,
    pub compare: Option<CompareModel>,
    pub message: Option<String>,
    pub footer: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_expansion_format::ExpansionPack;

    struct Host {
        gold: u32,
        compiled: Arc<CompiledItems>,
        progress: ItemProgress,
        shop: ShopDef,
        retail_held: u8,
        used: Vec<(String, u8)>,
    }

    impl ItemScreenHost for Host {
        fn gold(&self) -> u32 {
            self.gold
        }
        fn members(&self) -> Vec<Member> {
            ["Vahn", "Noa", "Gala"]
                .iter()
                .enumerate()
                .map(|(i, n)| Member {
                    roster: i as u8,
                    name: n.to_string(),
                    hp: 10,
                    hp_max: 100,
                    ..Default::default()
                })
                .collect()
        }
        fn retail_info(&self, id: u8) -> RetailInfo {
            RetailInfo {
                name: format!("Retail {id:02X}"),
                description: String::new(),
                price: 100,
                held: self.retail_held,
            }
        }
        fn prerequisites_hold(&self, p: &Prerequisites) -> bool {
            p.flags.is_empty()
        }
        fn compiled(&self) -> Option<Arc<CompiledItems>> {
            Some(self.compiled.clone())
        }
        fn progress(&self) -> &ItemProgress {
            &self.progress
        }
        fn shop(&self, _: &str) -> Option<ShopDef> {
            Some(self.shop.clone())
        }
        fn stats(&self, roster: u8, preview: Option<(u8, Option<&str>)>) -> StatLine {
            let b = self.compiled.worn_bonus(&self.progress, roster, preview);
            StatLine {
                atk: (50 + b.total.atk) as u16,
                ..Default::default()
            }
        }
        fn worn_name(&self, roster: u8, index: u8) -> String {
            self.progress
                .worn_at(roster, index)
                .and_then(|k| self.compiled.get(k))
                .map(|i| i.def.name.clone())
                .unwrap_or_default()
        }
        fn buy(&mut self, item: &ItemKey, qty: u8, cost: u32) -> bool {
            if cost > self.gold {
                return false;
            }
            self.gold -= cost;
            match item {
                ItemKey::Pack(k) => {
                    self.progress.add(k, qty);
                }
                ItemKey::Retail(_) => self.retail_held += qty,
            }
            true
        }
        fn sell(&mut self, key: &str, qty: u8, gain: u32) -> bool {
            self.progress.take(key, qty);
            self.gold += gain;
            true
        }
        fn use_item(&mut self, key: &str, roster: u8) -> Option<String> {
            self.progress.take(key, 1);
            self.used.push((key.to_string(), roster));
            Some("Healed.".into())
        }
        fn equip(&mut self, key: &str, roster: u8, index: u8) -> bool {
            self.progress.equip(key, roster, index)
        }
        fn unequip(&mut self, roster: u8, index: u8) -> bool {
            self.progress.unequip(roster, index).is_some()
        }
    }

    fn host() -> Host {
        let pack = ExpansionPack {
            items: toml::from_str(
                r#"
[items.blade]
name = "Blade"
kind = "weapon"
price = 1000
attack = 40
characters = ["vahn"]
[items.tonic]
name = "Tonic"
kind = "consumable"
price = 50
effect = { kind = "heal_hp", amount = 500 }
[[shops]]
id = "s"
name = "Armory"
stock = ["blade", 0x77, { item = "tonic", requires = { flags = ["shut"] } }]
"#,
            )
            .unwrap(),
            ..Default::default()
        };
        Host {
            gold: 2500,
            compiled: Arc::new(CompiledItems::compile(&pack)),
            progress: ItemProgress::default(),
            shop: pack.items.shops[0].clone(),
            retail_held: 0,
            used: Vec::new(),
        }
    }

    fn press(s: &mut ItemScreen, h: &mut Host, b: PadButton) -> ScreenStep {
        let step = s.step(b.mask(), 0, h);
        s.step(0, b.mask(), h);
        step
    }

    #[test]
    fn a_shop_lists_open_rows_and_buys_into_both_bags() {
        let mut h = host();
        let mut s = ItemScreen::shop("s");
        s.step(0, 0, &mut h);
        let rows = s.rows(&h);
        assert_eq!(rows.len(), 2, "the gated tonic is hidden");
        assert_eq!((rows[0].name.as_str(), rows[0].price), ("Blade", 1000));
        assert_eq!(rows[1].item, ItemKey::Retail(0x77));
        press(&mut s, &mut h, PadButton::Cross);
        assert_eq!(s.stage, Stage::Quantity { row: 0, qty: 1 });
        press(&mut s, &mut h, PadButton::Right);
        assert_eq!(s.stage, Stage::Quantity { row: 0, qty: 2 });
        press(&mut s, &mut h, PadButton::Right);
        assert_eq!(
            s.stage,
            Stage::Quantity { row: 0, qty: 1 },
            "2500 G buys two"
        );
        press(&mut s, &mut h, PadButton::Cross);
        assert_eq!((h.progress.count("blade"), h.gold), (1, 1500));
        press(&mut s, &mut h, PadButton::Down);
        press(&mut s, &mut h, PadButton::Cross);
        press(&mut s, &mut h, PadButton::Cross);
        assert_eq!((h.retail_held, h.gold), (1, 1400));
        // Sell the blade back at half.
        press(&mut s, &mut h, PadButton::R1);
        assert_eq!(s.tab, Tab::Sell);
        press(&mut s, &mut h, PadButton::Cross);
        press(&mut s, &mut h, PadButton::Cross);
        assert_eq!((h.progress.count("blade"), h.gold), (0, 1900));
        assert_eq!(press(&mut s, &mut h, PadButton::Circle), ScreenStep::Leave);
    }

    #[test]
    fn the_gear_tab_equips_with_a_compare_and_takes_off() {
        let mut h = host();
        h.progress.add("blade", 1);
        let mut s = ItemScreen::bag(true);
        s.step(0, 0, &mut h);
        press(&mut s, &mut h, PadButton::Cross);
        let m = s.model(&h);
        let c = m.compare.expect("compare panel");
        assert_eq!(c.rows[0], ("ATK", 50, 90));
        let (members, _) = m.members.unwrap();
        assert_eq!(members[0].line, "Weapon: -");
        assert!(members[1].dim, "Noa can't wear it");
        press(&mut s, &mut h, PadButton::Cross);
        assert_eq!(h.progress.worn_at(0, 2), Some("blade"));
        assert_eq!(s.model(&h).compare.unwrap().rows[0], ("ATK", 90, 50));
        press(&mut s, &mut h, PadButton::Down);
        press(&mut s, &mut h, PadButton::Cross);
        assert!(s.model(&h).message.unwrap().contains("can't wear"));
        press(&mut s, &mut h, PadButton::Up);
        press(&mut s, &mut h, PadButton::Cross);
        assert_eq!(h.progress.worn_at(0, 2), None, "took it off");
        assert_eq!(h.progress.count("blade"), 1);
    }

    #[test]
    fn the_items_tab_uses_a_consumable_on_a_member() {
        let mut h = host();
        h.progress.add("tonic", 2);
        let mut s = ItemScreen::bag(false);
        s.step(0, 0, &mut h);
        press(&mut s, &mut h, PadButton::Cross);
        press(&mut s, &mut h, PadButton::Down);
        press(&mut s, &mut h, PadButton::Cross);
        assert_eq!(h.used, [("tonic".to_string(), 1)]);
        assert_eq!(h.progress.count("tonic"), 1);
    }

    #[test]
    fn a_button_held_at_open_is_ignored() {
        let mut h = host();
        let mut s = ItemScreen::shop("s");
        let cross = PadButton::Cross.mask();
        s.step(cross, cross, &mut h);
        assert_eq!(s.stage, Stage::List { cursor: 0 });
        s.step(0, cross, &mut h);
        s.step(cross, 0, &mut h);
        assert!(matches!(s.stage, Stage::Quantity { .. }));
    }
}
