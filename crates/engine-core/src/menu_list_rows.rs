//! SCUS list-kernel row model: the list-node allocator, the per-content-id
//! row builders, and the row name / description resolvers shared by every
//! pause-menu list window (see
//! [`docs/subsystems/field-menu.md`](../../../docs/subsystems/field-menu.md),
//! "Submenu state machines").
//!
//! The retail kind-4 list kernel (`FUN_80032A44`, navigation port at
//! [`crate::pause_screens::list_kernel_navigate`]) only *reads* row entries;
//! the entries themselves are built once, at window create / content refresh,
//! by the SCUS content builder `FUN_80030628` (a per-content-id switch, jump
//! table `0x80010D38`, index `content_id - 2`). This module ports the
//! documented item-list cases of that switch plus the small SCUS helpers
//! around it:
//!
//! - [`list_alloc`] - the list-node allocator `FUN_80030104` (scroll /
//!   selection persistence clamps);
//! - [`build_use_list_rows`] - content id 3, the Items **Use** list;
//! - [`build_throw_out_rows`] - content id `0x22`, the Items **Throw Out**
//!   list;
//! - [`build_price_gated_rows`] - content id 2, the price-gated bag list;
//! - [`build_shop_buy_rows`] + [`shop_buy_row_order`] - content id `0x0B`,
//!   the shop **buy** list (the one family here that is live);
//! - [`row_name_source`] - the per-class row-name resolver `FUN_8002FF8C`;
//! - [`row_description_source`] - the highlighted-row description
//!   dispatcher `FUN_80034250`;
//! - [`LiveWindowSet`] - the live-window upsert `FUN_80032434` (the
//!   `gp+0x148` = `0x8007B460` sorted window list the kernel iterates).
//!
//! All ports are derived from the SCUS disassembly
//! (`ghidra/scripts/funcs/<addr>.txt`); provenance notes sit on each item.
//!
//! The shop buy-list pair is the exception to everything below: its rows are
//! keyed by **item id**, not by a bag slot, and its order kernel is live -
//! `crate::shop_catalog::scene_shops` runs [`shop_buy_row_order`] over every
//! decoded stock list, so all three hosts draw the retail row order.
//!
//! Nothing else in the engine speaks retail's row-entry model - but the three
//! families below are in three *different* positions, and a note that names
//! only the first reads as one gap and is three. Two of them are
//! substitutions the port has already made; one is a real wiring gap. Each
//! item below carries its own class marker.
//!
//! 1. REPLACED-BY: the per-screen window models the engine's menu hosts keep.
//!    That covers [`list_alloc`], [`LiveWindowSet`] and
//!    [`record_string_glyph_count`]. No code path allocates the `gp+0x148`
//!    node whose header the allocator seeds, and no screen measures a record
//!    string to size a window - the rects are disc-parsed at boot from
//!    [`legaia_asset::menu_windows`]. Adopting the sorted window list would
//!    be a change of representation, not a call insertion.
//! 2. REPLACED-BY: the typed rows every pause-menu screen already carries -
//!    [`crate::pause_screens::PauseItemRow`],
//!    [`crate::spell_menu::SpellRowView`],
//!    [`crate::equip_session::EquipItem`]. That covers [`row_name_source`]
//!    and [`row_description_source`], which decode the class-tagged `u16`
//!    entry word ([`CLASS_BAG`]..[`CLASS_SHOP_ALT`]). Nothing produces such a
//!    word because the typed rows already carry the resolved name and
//!    description, so a resolver over them would re-derive what the caller
//!    holds.
//! 3. NOT WIRED - and this one is a real gap with a visible consequence. The
//!    three `FUN_80030628` builders want an **ordered bag-slot array** and a
//!    per-row ink bit, and the engine has neither. `World::inventory` is a
//!    `HashMap<u8, u8>` keyed by item id with no slot space at all, so the
//!    `slot | ink` payload the builders emit has no index to carry;
//!    `crate::field_menu_dispatch::build_pause_items_session` sorts the
//!    held ids and [`crate::pause_screens::PauseItemRow`] has no ink field,
//!    so retail's three-buffer order (in place, then equipment, then the
//!    flag-8 tail) and its dim gates have nowhere to land. Until they do,
//!    the port lists a player's items in an order retail would not, and dims
//!    none of them. The owner is `build_pause_items_session`, once the bag is
//!    slot-indexed.
//!
//! Which window each builder fills is not a guess: the menu-overlay
//! descriptor table ([`legaia_asset::menu_windows`]) carries the content id
//! in each record's `+0x0`, and on the retail disc content `2` is window 38
//! (the shop's **sell** list), content `3` is window 15 and content `0x22`
//! window 16 (the Items screen's Use / Throw Out lists). All three are
//! renderer-less containers the kind-4 list kernel draws from the entry
//! words - which is why the builders and the resolvers close together.
//!
//! `crate::pause_screens::list_kernel_navigate` - the *navigation* half of
//! the same kernel - is the one piece that survived the flat-cursor
//! translation and is live.

/// Row-entry class nibble mask (`entry & 0xF000`). The class routes the
/// per-row draw in `FUN_80032A44` and the name lookup in `FUN_8002FF8C`.
pub const ROW_CLASS_MASK: u16 = 0xF000;
/// Disabled bit: the row draws grey (ink 0) and confirming it buzzes
/// (`FUN_80032A44` branch at `0x80032d04`, cue `0x23`).
pub const ROW_DISABLED: u16 = 0x0800;
/// Alt-ink bit: the row draws with ink 1 instead of 7.
pub const ROW_ALT_INK: u16 = 0x0400;
/// Payload bits consumed by the name / description resolvers
/// (`FUN_8002FF8C` and `FUN_80034250` both mask `& 0x3FF`). The kernel
/// itself forwards the low 12 bits (`& 0xFFF`) into the selected-payload
/// global `0x8007BB88`.
pub const ROW_NAME_PAYLOAD_MASK: u16 = 0x03FF;

/// Bag-row class: payload = bag slot, name via the inventory byte array.
pub const CLASS_BAG: u16 = 0x1000;
/// Spell-row class: payload = spell id, name from the spell table.
pub const CLASS_SPELL: u16 = 0x2000;
/// Item-row class (id direct): payload = item id.
pub const CLASS_ITEM: u16 = 0x3000;
/// Menu-verb class: payload indexes the verb pointer table `0x8007329C`.
pub const CLASS_VERB: u16 = 0x4000;
/// Spell-row class (second ink variant in the kernel's row draw).
pub const CLASS_SPELL_ALT: u16 = 0x5000;
/// Bag-row class with an equip-slot pictogram (the Equip candidate list).
pub const CLASS_BAG_EQUIP: u16 = 0x6000;
/// Item-row class drawn with ICO `0x21` (payload = item id).
pub const CLASS_ITEM_ICON: u16 = 0x7000;
/// Landmark class: payload indexes the 6-byte placement records at
/// `0x80073A98`; names are fixed 32-byte cells at `0x80073B18`.
pub const CLASS_LANDMARK: u16 = 0x8000;
/// Passive ("Goods") row class: payload = bag slot, described via the
/// accessory-passive chain.
pub const CLASS_PASSIVE: u16 = 0x9000;
/// Shop-row class staging ink 5 (payload = item id).
pub const CLASS_SHOP_ALT: u16 = 0xA000;

/// The persisted scroll / selection pair the allocator clamps and re-seeds
/// each list from: the retail globals `0x8007BB90` (scroll top, `gp+0x878`)
/// and `0x8007BB98` (selected row, `gp+0x880`). Keeping them across window
/// rebuilds is what makes a refreshed list reopen on the same row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ListSelection {
    pub scroll_top: i32,
    pub selected: i32,
}

/// A freshly allocated list node's header fields (retail layout: `+0x0`
/// scroll top, `+0x2` visible rows, `+0x4` row count, `+0x6` selected row;
/// per-row `u16` entries follow from `+0x28`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListNodeInit {
    pub scroll_top: u16,
    pub visible_rows: u16,
    pub count: u16,
    pub selected: u16,
}

/// Result of the list-node allocator: a zero row count turns the window
/// into a plain-text panel instead of a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListAlloc {
    /// `count == 0`: the window content becomes the caller's fallback
    /// string (retail copies it into a fresh `0x80`-byte buffer at window
    /// `+0x18`, measures its pixel width via the text-width kernel
    /// `FUN_8003CC90` into `+0x12`, and marks `+0x14 = 0x80`).
    EmptyText,
    /// `count > 0`: a list node seeded from the clamped persisted
    /// selection.
    Node(ListNodeInit),
}

/// PORT: FUN_80030104 (list-node allocator; `see
/// ghidra/scripts/funcs/80030104.txt`).
///
/// REPLACED-BY: the per-screen window models the engine's menu hosts keep -
/// see family 1 in the module heading.
///
/// Allocates the `count*2 + 0x2A`-byte list node hung at live-window
/// `+0x18` and seeds its header from the persisted selection globals,
/// clamping them **in place** first (the stores at `0x80030204` /
/// `0x80030220` write the globals back, not locals):
///
/// - `selected >= count` drops the selection to the new last row;
/// - `scroll_top > selected` pulls the scroll top down to the selection.
///
/// `visible_rows = (content_h - 4) / 14` (signed, truncating - the
/// `0x92492493` magic-multiply sequence at `0x8003025C..0x80030284`), one
/// row per `0xE`-pixel pitch. Retail also mirrors `count` into the
/// row-count global `0x8007BBA0` and resets three more on the way out
/// (`0x8007BB9C` and `0x8007BB88` to zero, `0x8007BB60` to `-1`, plus
/// `node+0x28 = 0`); callers of this port own those mirrors. The third
/// was missing from this list while the routine was inert - an
/// enumeration that stops one store short reads exactly like a complete
/// one.
pub fn list_alloc(count: i32, content_h: i16, persisted: &mut ListSelection) -> ListAlloc {
    if count == 0 {
        return ListAlloc::EmptyText;
    }
    if persisted.selected >= count {
        persisted.selected = count - 1;
    }
    if persisted.selected < persisted.scroll_top {
        persisted.scroll_top = persisted.selected;
    }
    ListAlloc::Node(ListNodeInit {
        scroll_top: persisted.scroll_top as u16,
        visible_rows: ((content_h as i32 - 4) / 14) as u16,
        count: count as u16,
        selected: persisted.selected as u16,
    })
}

/// Where a row's display name comes from, per the class nibble.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowNameSource {
    /// Unknown / zero class: the shared empty string (`gp+0x168`).
    Empty,
    /// Spell-table name (record `+8` of the 12-byte `0x800754C8` record).
    Spell(u16),
    /// Item-table name, payload used as the item id directly (record `+4`
    /// of the 12-byte `0x80074368` record).
    Item(u16),
    /// Item-table name via the bag: payload is a bag slot, the id is the
    /// byte at `0x80085958 + slot*2`.
    BagSlot(u16),
    /// Fixed 32-byte landmark name cell: payload indexes the 6-byte
    /// placement records at `0x80073A98`, whose first byte picks the
    /// name cell at `0x80073B18 + code*0x20`.
    Landmark(u16),
    /// Menu-verb pointer table `0x8007329C[payload]`.
    Verb(u16),
}

/// PORT: FUN_8002FF8C (row-name resolver; `see
/// ghidra/scripts/funcs/8002ff8c.txt`).
///
/// REPLACED-BY: the typed pause-menu rows, which already carry the resolved
/// name - see family 2 in the module heading.
///
/// Maps a row entry's class nibble to its name source. Payloads are
/// masked `& 0x3FF`. Classes `0x2000`/`0x5000` read the spell table;
/// `0x3000`/`0x7000`/`0xA000` read the item table with the payload as
/// the id; `0x1000`/`0x6000`/`0x9000` first dereference the bag slot;
/// `0x8000` walks the landmark placement records; `0x4000` the verb
/// pointer table. Anything else (including class 0) resolves to the
/// shared empty string.
pub fn row_name_source(entry: u16) -> RowNameSource {
    let payload = entry & ROW_NAME_PAYLOAD_MASK;
    match entry & ROW_CLASS_MASK {
        CLASS_SPELL | CLASS_SPELL_ALT => RowNameSource::Spell(payload),
        CLASS_ITEM | CLASS_ITEM_ICON | CLASS_SHOP_ALT => RowNameSource::Item(payload),
        CLASS_BAG | CLASS_BAG_EQUIP | CLASS_PASSIVE => RowNameSource::BagSlot(payload),
        CLASS_LANDMARK => RowNameSource::Landmark(payload),
        CLASS_VERB => RowNameSource::Verb(payload),
        _ => RowNameSource::Empty,
    }
}

/// Where the info-panel description for the highlighted row comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescriptionSource {
    /// Nothing drawn (list parked, unknown class, or resolved id 0).
    None,
    /// Item description (record `+8` of the item table).
    ItemDesc(u8),
    /// Accessory-passive description: item record `+1` (subtype) ->
    /// item-effect record `+3` (passive index) -> `0x8007625C` passive
    /// record `+8`.
    PassiveDesc(u8),
}

/// The park mode value of the list-mode global `0x8007BB94`: the list is
/// behind the command window and suppresses cursor-tracking draws.
pub const LIST_MODE_PARKED: i32 = 4;

/// PORT: FUN_80034250 (highlighted-row description dispatcher; `see
/// ghidra/scripts/funcs/80034250.txt`).
///
/// REPLACED-BY: the typed pause-menu rows, which already carry the resolved
/// description - see family 2 in the module heading.
///
/// `list_mode` is the mode global `0x8007BB94` (`gp+0x87C`) - mode 4
/// (parked) suppresses the draw entirely. `screen_class` is the selected
/// row's class global `0x8007BB9C`, `cursor_payload` the payload global
/// `0x8007BB88`. Classes `0x1000`/`0x6000` (bag) and `0x9000` (passive)
/// resolve the item id through `bag_id_at(slot)`; class `0x7000` uses the
/// payload as the id directly. A resolved id of 0 draws nothing. Retail
/// then hands the string to the text drawer `FUN_800337B0` at the
/// window's `(+0xA, +0xC)` rect origin; the draw stays host-side here.
///
/// The name is deliberately not the bare `description_source`:
/// `legaia_engine_ui`'s `ui_menu_window_painters` declares a free function
/// of that name over a different id space, and the port catalog's call
/// graph links a bare call to **every** in-tree free function of the name
/// with no receiver to disambiguate. Sharing it would make this kernel read
/// live off the painter's first non-test call site and turn the disclosure
/// above into a false accusation - the `countdown_frame` / `cursor_step`
/// shape recorded in `docs/tooling/stale-not-wired-triage.md`.
pub fn row_description_source(
    list_mode: i32,
    screen_class: u16,
    cursor_payload: u16,
    bag_id_at: impl Fn(u16) -> u8,
) -> DescriptionSource {
    if list_mode == LIST_MODE_PARKED {
        return DescriptionSource::None;
    }
    let resolved = match screen_class {
        CLASS_BAG | CLASS_BAG_EQUIP => DescriptionSource::ItemDesc(bag_id_at(cursor_payload)),
        CLASS_PASSIVE => DescriptionSource::PassiveDesc(bag_id_at(cursor_payload)),
        CLASS_ITEM_ICON => DescriptionSource::ItemDesc(cursor_payload as u8),
        _ => return DescriptionSource::None,
    };
    match resolved {
        DescriptionSource::ItemDesc(0) | DescriptionSource::PassiveDesc(0) => {
            DescriptionSource::None
        }
        other => other,
    }
}

/// Item-table fields the row builders consume. The retail sources are the
/// 12-byte item record at `0x80074368` (`+0` kind, `+1` subtype, `+2`
/// price), the item-effect flags byte `0x800752C0[subtype*4 + 2]`
/// ([`legaia_asset::item_effect`]) and the equipment-record flags byte
/// `0x80074F68[subtype*8 + 7]` ([`legaia_asset::equip_stats`]).
pub trait ItemRowTables {
    /// Item record kind byte (`+0`): 1 = equipment / key gear, 2 =
    /// effect-bearing consumable.
    fn kind(&self, id: u8) -> u8;
    /// Item record subtype byte (`+1`): index into the effect table
    /// (kind 2) or the equipment stat table (kind 1).
    fn subtype(&self, id: u8) -> u8;
    /// Item record price halfword (`+2`).
    fn price(&self, id: u8) -> u16;
    /// Item-effect flags byte (`0x800752C0[subtype*4 + 2]`).
    fn effect_flags(&self, subtype: u8) -> u8;
    /// Equipment-record flags byte (`0x80074F68[subtype*8 + 7]`).
    fn equip_flags(&self, subtype: u8) -> u8;
}

/// Item-effect flag consumed by the Throw Out builder: set = the item is
/// not discardable (key items).
pub const EFFECT_FLAG_NOT_DISCARDABLE: u8 = 0x01;
/// Item-effect flag: usable from the field pause menu.
pub const EFFECT_FLAG_FIELD_USABLE: u8 = 0x02;
/// Item-effect flag: usable from the battle item menu.
pub const EFFECT_FLAG_BATTLE_USABLE: u8 = 0x04;
/// Item-effect flag routing kind-2 items into the tail alt-ink group.
pub const EFFECT_FLAG_TAIL_GROUP: u8 = 0x08;
/// Equipment-record flag consumed by the Throw Out builder: set = the
/// equip piece refuses discard.
pub const EQUIP_FLAG_NO_DISCARD: u8 = 0x01;

/// Door of Light item id (Use-list scene gate).
pub const ITEM_DOOR_OF_LIGHT: u8 = 0x88;
/// Door of Wind item id (Use-list scene gate).
pub const ITEM_DOOR_OF_WIND: u8 = 0x89;

/// Context for the Use-list build (content id 3).
pub struct UseListCtx<'a> {
    /// The menu context word `gp+0x85C` (`0x8007BB74`): `false` = field
    /// (context 0), `true` = battle (context 1).
    pub battle: bool,
    /// Scratchpad `0x1F800394` bit `0x100000`: set dims the Door of
    /// Light row in place (the branch at `0x8003094C` emits
    /// `slot | 0x1800` when the bit is set).
    pub door_light_blocked: bool,
    /// Scratchpad `0x1F800394` bit `0x200000`: same gate for the Door of
    /// Wind row (`0x8003096C`).
    pub door_wind_blocked: bool,
    /// The applicability probe `FUN_8003043C`: a party scan through the
    /// action validator `FUN_8003FB10` returning `false` when the item
    /// would affect nobody (everyone at full HP for a heal), which dims
    /// the row.
    pub applicable: &'a dyn Fn(u8) -> bool,
}

/// PORT: FUN_80030628 (content-id-3 case, `0x80030828..0x80030AA4` - the
/// Items **Use** list row build; `see ghidra/scripts/funcs/80030628.txt`
/// and `docs/subsystems/field-menu.md#use-list-row-build-content-id-3-fun_80030628`).
///
/// NOT WIRED: the owner is
/// `crate::field_menu_dispatch::build_pause_items_session`, and it cannot
/// call this until `World::inventory` is slot-indexed - see family 3 in the
/// module heading.
///
/// Walks the bag slots (`bag_ids[i]` = the item-id byte at
/// `0x80085958 + (slot_base + i)*2`; retail bounds the walk with the
/// window's slot range at `gp+0x2D2..gp+0x2D4`) and builds the row words
/// in the retail three-buffer order: in-place rows first, then the
/// kind-1 (equipment) rows, then the kind-2 tail-flag rows. Empty slots
/// (id 0) produce no row. Per-slot routing, in evaluation order:
///
/// - kind 2 with effect flag `0x8`: `slot | 0x1C00` (dim + alt-ink),
///   appended **last**;
/// - kind 1: `slot | 0x1800` (dim), appended after the in-place rows;
/// - field context: the Door of Light / Door of Wind scene gates, then
///   effect flag `0x2` (clear = dim in place), then the applicability
///   probe (`false` = dim in place);
/// - battle context: effect flag `0x4` set = white in place, clear =
///   dim **appended after** the in-place rows (unlike the field
///   context, battle-unusable rows sort to the tail - branch at
///   `0x800309E4` stores through the second buffer). A context word
///   other than 0/1 emits no row at all (`0x800309C0`).
pub fn build_use_list_rows(
    bag_ids: &[u8],
    slot_base: u16,
    tables: &impl ItemRowTables,
    ctx: &UseListCtx<'_>,
) -> Vec<u16> {
    let mut in_place = Vec::new();
    let mut tail_kind1 = Vec::new();
    let mut tail_flag8 = Vec::new();
    for (i, &id) in bag_ids.iter().enumerate() {
        if id == 0 {
            continue;
        }
        let slot = slot_base + i as u16;
        let kind = tables.kind(id);
        if kind == 2 && tables.effect_flags(tables.subtype(id)) & EFFECT_FLAG_TAIL_GROUP != 0 {
            tail_flag8.push(slot | 0x1C00);
            continue;
        }
        if kind == 1 {
            tail_kind1.push(slot | 0x1800);
            continue;
        }
        if !ctx.battle {
            if id == ITEM_DOOR_OF_LIGHT && ctx.door_light_blocked {
                in_place.push(slot | 0x1800);
                continue;
            }
            if id == ITEM_DOOR_OF_WIND && ctx.door_wind_blocked {
                in_place.push(slot | 0x1800);
                continue;
            }
            if tables.effect_flags(tables.subtype(id)) & EFFECT_FLAG_FIELD_USABLE == 0 {
                in_place.push(slot | 0x1800);
                continue;
            }
            if (ctx.applicable)(id) {
                in_place.push(slot | 0x1000);
            } else {
                in_place.push(slot | 0x1800);
            }
        } else if tables.effect_flags(tables.subtype(id)) & EFFECT_FLAG_BATTLE_USABLE != 0 {
            in_place.push(slot | 0x1000);
        } else {
            tail_kind1.push(slot | 0x1800);
        }
    }
    in_place.extend_from_slice(&tail_kind1);
    in_place.extend_from_slice(&tail_flag8);
    in_place
}

/// PORT: FUN_80030628 (content-id-0x22 case, `0x80030AF8..0x80030CF4` -
/// the Items **Throw Out** list row build; `see
/// ghidra/scripts/funcs/80030628.txt`).
///
/// NOT WIRED: same owner and same blocker as [`build_use_list_rows`] - see
/// family 3 in the module heading.
///
/// Same three-buffer shape as the Use list with a discardability gate
/// instead of the usability chain:
///
/// - kind 2 with effect flag `0x8`: `slot | 0x1C00`, appended last;
/// - kind 1: white unless equip-record flag `0x1` (no-discard) dims it;
///   appended after the in-place rows either way;
/// - otherwise in place: white unless effect flag `0x1` (key item) dims
///   it.
pub fn build_throw_out_rows(
    bag_ids: &[u8],
    slot_base: u16,
    tables: &impl ItemRowTables,
) -> Vec<u16> {
    let mut in_place = Vec::new();
    let mut tail_kind1 = Vec::new();
    let mut tail_flag8 = Vec::new();
    for (i, &id) in bag_ids.iter().enumerate() {
        if id == 0 {
            continue;
        }
        let slot = slot_base + i as u16;
        let kind = tables.kind(id);
        let subtype = tables.subtype(id);
        if kind == 2 && tables.effect_flags(subtype) & EFFECT_FLAG_TAIL_GROUP != 0 {
            tail_flag8.push(slot | 0x1C00);
            continue;
        }
        if kind == 1 {
            if tables.equip_flags(subtype) & EQUIP_FLAG_NO_DISCARD != 0 {
                tail_kind1.push(slot | 0x1800);
            } else {
                tail_kind1.push(slot | 0x1000);
            }
            continue;
        }
        if tables.effect_flags(subtype) & EFFECT_FLAG_NOT_DISCARDABLE != 0 {
            in_place.push(slot | 0x1800);
        } else {
            in_place.push(slot | 0x1000);
        }
    }
    in_place.extend_from_slice(&tail_kind1);
    in_place.extend_from_slice(&tail_flag8);
    in_place
}

/// PORT: FUN_80030628 (content-id-2 case, `0x80030694..0x80030824` - the
/// price-gated bag list; `see ghidra/scripts/funcs/80030628.txt`).
///
/// NOT WIRED: the owner is the shop session's sell list; same slot-indexing
/// blocker as [`build_use_list_rows`] - see family 3 in the module heading.
///
/// The shop-sell shape: rows with a non-zero item price stay white in
/// place; zero-price rows (unsellable) dim and sort last. No third
/// buffer in this case.
pub fn build_price_gated_rows(
    bag_ids: &[u8],
    slot_base: u16,
    tables: &impl ItemRowTables,
) -> Vec<u16> {
    let mut in_place = Vec::new();
    let mut tail = Vec::new();
    for (i, &id) in bag_ids.iter().enumerate() {
        if id == 0 {
            continue;
        }
        let slot = slot_base + i as u16;
        if tables.price(id) != 0 {
            in_place.push(slot | 0x1000);
        } else {
            tail.push(slot | 0x1800);
        }
    }
    in_place.extend_from_slice(&tail);
    in_place
}

/// Lowest item id the shop **buy** list will build a row for.
///
/// `slti v0,s0,0x1a` gates both of the builder's passes
/// (`0x80030E28` shrinks the allocation, `0x80030EA0` skips the emit). Every
/// item id below `0x1A` carries price `0` in the static item table, so this
/// id-range test and the port's price-`> 0` sellable mask agree over the
/// whole retail id space; a price-`0` id **at or above** `0x1A` (`0x1B`,
/// `0x1F`, `0x21`, ...) would build a row in retail and be dropped by the
/// price mask, which no shipped shop record exercises.
pub const SHOP_ROW_MIN_ITEM_ID: u8 = 0x1A;

/// Held count at which a shop buy row dims (`sltiu v0,v0,0x63` at
/// `0x80030F0C` - the row stays white while the held count is `< 99`).
pub const SHOP_ROW_HELD_CAP: u8 = 99;

/// Rows the builder splits off the **tail** of the stock record and emits
/// ahead of the rest (`_DAT_8007B450[2] - 3` is the split index, and the
/// same `3` is what the no-room arm subtracts).
pub const SHOP_TAIL_ROWS: usize = 3;

/// PORT: FUN_80030628 (case `0x0B` row order, `0x80030F1C..0x80030F90`).
///
/// The on-screen order of a shop buy list is **not** the record order.
/// Retail splits the walked rows at `record_count - 3`: rows below the split
/// stage into a scratch array at `0x801C6220`, rows at or above it are
/// written straight into the row buffer, and the staged group is appended
/// afterwards - so the record's last entries come out **first**.
///
/// `record_count` is the record's own `[+2]` id count (padding included);
/// `walk` is how many entries the emit loop covers, i.e. the count minus the
/// sub-[`SHOP_ROW_MIN_ITEM_ID`] template ids. The hoisted band is therefore
/// `walk - (record_count - 3)` rows wide - exactly `3 - padding_len`, which
/// is why a record always reserves three tail slots and pads the unused ones
/// with `Ra-Seru Meta $N`.
///
/// The hoisted rows are tagged [`CLASS_SHOP_ALT`], which the kind-4 list
/// kernel stages with ink 5 - the "new in this town" highlight the
/// walkthrough tables mark with `*`.
///
/// Returns a permutation of `0..walk`.
pub fn shop_buy_row_order(record_count: usize, walk: usize) -> Vec<usize> {
    let split = record_count as isize - SHOP_TAIL_ROWS as isize;
    let mut order: Vec<usize> = (0..walk).filter(|&i| (i as isize) >= split).collect();
    order.extend((0..walk).filter(|&i| (i as isize) < split));
    order
}

/// PORT: FUN_80030628 (content-id-`0x0B` case, `0x80030D48..0x80030F98` -
/// the shop **buy** list; `see ghidra/scripts/funcs/80030628.txt`).
///
/// This, not [`build_price_gated_rows`], is the shop's buy row layout.
/// Content id `2` is the price-gated *bag* list (the sell side); the buy
/// list is its own case and reads a different source - the field-VM
/// entry-context record `_DAT_8007B450` directly, `[+2]` = id count and
/// `[+3 + i]` = the item ids, i.e. exactly the op-`0x49` sub-`0` stock
/// record [`legaia_asset::shop_stock`] scans.
///
/// Three shapes the disassembly fixes:
///
/// * **The `< 0x1A` filter runs twice.** The first pass shrinks the
///   allocation by every low id among the first `n` entries
///   (`0x80030E10..0x80030E44`); the emit loop then walks only that shrunk
///   count (`slt v0,s1,s4` at `0x80030F5C`). So the builder *depends* on the
///   unsellable template ids being a trailing run - walking `n - low_count`
///   entries covers the sellable prefix exactly, and an interleaved record
///   would silently truncate. That is the same partition
///   `docs/subsystems/shop.md` measured disc-wide from the other side.
/// * **The last three entries emit first.** Rows below the split index go
///   to a staging array at `0x801C6220` tagged [`CLASS_ITEM`]; rows at or
///   above it are written straight into the row buffer tagged
///   [`CLASS_SHOP_ALT`], and the staged group is appended after
///   (`0x80030F68..0x80030F90`). The hoisted group is `3 - padding_len`
///   rows wide - empty on a record padded with three template ids, two or
///   three rows on the disc's shorter-padded records - and it is the
///   walkthrough tables' "new in this town" band.
/// * **The tail is conditional.** `tail_rows_allowed` is retail's
///   `s4 ∈ {0, 3}` probe pair at `0x80030D54..0x80030DE8`: the bag-slot
///   scan `FUN_80042F4C(0xFF)` and an eight-byte `0xFF` sweep of every
///   party member's equipment block (`char + 0x196..+0x19D`). Neither
///   probe touches the stock; both only decide whether the last three
///   entries are walked at all.
///
/// `price_of` / `held_of` are the two live reads the dim bit needs: the
/// item record's `+2` price halfword and the bag count for that id
/// (`0x80085959 + slot*2`, `0` when the id is not held).
pub fn build_shop_buy_rows(
    stock_ids: &[u8],
    tail_rows_allowed: bool,
    purse: u32,
    price_of: impl Fn(u8) -> u16,
    held_of: impl Fn(u8) -> u8,
) -> Vec<u16> {
    let count = stock_ids.len();
    let n = if tail_rows_allowed {
        count
    } else {
        count.saturating_sub(SHOP_TAIL_ROWS)
    };
    // Pass 1 (`0x80030E10..0x80030E44`): the allocation shrinks by every
    // low id, and that shrunk figure is what the emit loop walks.
    let low = stock_ids[..n]
        .iter()
        .filter(|&&id| id < SHOP_ROW_MIN_ITEM_ID)
        .count();
    let walk = n - low;
    // The split index re-reads the record's own count byte, not the
    // possibly-shrunk `n` (`lbu v0,2(v0); addiu v0,v0,-3` at `0x80030F24`).
    let split = count as isize - SHOP_TAIL_ROWS as isize;

    shop_buy_row_order(count, walk)
        .into_iter()
        .filter_map(|i| {
            let id = stock_ids[i];
            if id < SHOP_ROW_MIN_ITEM_ID {
                return None;
            }
            let mut word = u16::from(id);
            if purse < u32::from(price_of(id)) || held_of(id) >= SHOP_ROW_HELD_CAP {
                word |= ROW_DISABLED;
            }
            Some(
                word | if (i as isize) < split {
                    CLASS_ITEM
                } else {
                    CLASS_SHOP_ALT
                },
            )
        })
        .collect()
}

/// One live menu window of the SCUS window list (retail: a 0x34-byte
/// node in the sentinel-circular list at `gp+0x148` = `0x8007B460`,
/// sorted ascending by the window id at `+0x8`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LiveWindow {
    /// Window id (`+0x8`) - the sort key and upsert key.
    pub id: u16,
    /// Rect (`+0xA` x, `+0xC` y, `+0xE` w, `+0x10` h).
    pub x: i16,
    pub y: i16,
    pub w: i16,
    pub h: i16,
    /// Content id byte (`+0x1C`) - the `FUN_80030628` dispatch selector.
    pub content_id: u8,
    /// Sub-kind byte (`+0x1D`).
    pub sub_kind: u8,
    /// Glyph count (`+0x14`), summed from the length-prefixed record
    /// string (see [`record_string_glyph_count`]).
    pub glyph_count: u16,
    /// Slide-motion flag (`+0x20`): set to 1 on create; the window-script
    /// runner's op 6 zeroes it to snap the slide.
    pub slide_active: bool,
}

/// PORT: FUN_80032434 (glyph-count scan, `0x800324E8..0x80032528`).
///
/// REPLACED-BY: the disc-parsed window rects
/// ([`legaia_asset::menu_windows`]), which mean no screen measures a record
/// string to size a window - see family 1 in the module heading.
///
/// A record string is a sequence of `[len: u8][len * 2 bytes]` segments
/// terminated by a zero length byte; the glyph count is the sum of the
/// segment lengths. (The `* 2` stride is the two-byte-per-glyph record
/// encoding, not UTF-16.)
pub fn record_string_glyph_count(record: &[u8]) -> u16 {
    let mut total: u32 = 0;
    let mut idx = 0usize;
    while idx < record.len() && record[idx] != 0 {
        let len = record[idx] as usize;
        total += len as u32;
        idx += 1 + len * 2;
    }
    total as u16
}

/// The live-window list: ports the `gp+0x148` sentinel-circular list as a
/// vector kept sorted ascending by window id.
#[derive(Debug, Clone, Default)]
pub struct LiveWindowSet {
    windows: Vec<LiveWindow>,
}

impl LiveWindowSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// PORT: FUN_80032434 (live-window upsert; `see
    /// ghidra/scripts/funcs/80032434.txt`).
    ///
    /// Walks the list for the first node whose id is `>= id`
    /// (`0x80032528..0x80032564`); a missing id inserts a fresh node
    /// there (keeping the ascending order), an existing id is reused in
    /// place. Either way the rect / content fields are overwritten from
    /// the config and the slide record re-seeded with current = target =
    /// the window rect origin (created in place, no slide travel) and
    /// the motion flag set. Retail then rebuilds the window content
    /// through `FUN_80030628`; content attachment stays caller-side here
    /// (see the row builders above).
    ///
    /// REPLACED-BY: the per-screen window models the engine's menu hosts
    /// keep - see family 1 in the module heading.
    pub fn upsert(&mut self, window: LiveWindow) -> &mut LiveWindow {
        let pos = self.windows.partition_point(|w| w.id < window.id);
        if pos < self.windows.len() && self.windows[pos].id == window.id {
            self.windows[pos] = window;
        } else {
            self.windows.insert(pos, window);
        }
        &mut self.windows[pos]
    }

    pub fn get(&self, id: u16) -> Option<&LiveWindow> {
        self.windows.iter().find(|w| w.id == id)
    }

    /// Windows in list order (ascending id) - the kernel's iteration
    /// order.
    pub fn iter(&self) -> impl Iterator<Item = &LiveWindow> {
        self.windows.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_alloc_clamps_persisted_selection() {
        // Selection past the new count drops to the last row and drags
        // the scroll top down with it.
        let mut sel = ListSelection {
            scroll_top: 20,
            selected: 25,
        };
        let ListAlloc::Node(node) = list_alloc(10, 172, &mut sel) else {
            panic!("expected node");
        };
        assert_eq!(sel.selected, 9);
        assert_eq!(sel.scroll_top, 9);
        assert_eq!(node.selected, 9);
        assert_eq!(node.scroll_top, 9);
        assert_eq!(node.count, 10);
        // (172 - 4) / 14 = 12 visible rows - the item-list rect.
        assert_eq!(node.visible_rows, 12);
    }

    #[test]
    fn list_alloc_keeps_valid_selection() {
        let mut sel = ListSelection {
            scroll_top: 12,
            selected: 14,
        };
        let ListAlloc::Node(node) = list_alloc(30, 172, &mut sel) else {
            panic!("expected node");
        };
        assert_eq!((node.scroll_top, node.selected), (12, 14));
        assert_eq!(sel.scroll_top, 12);
    }

    #[test]
    fn list_alloc_zero_count_is_text_panel() {
        let mut sel = ListSelection::default();
        assert_eq!(list_alloc(0, 172, &mut sel), ListAlloc::EmptyText);
    }

    #[test]
    fn row_name_source_class_routing() {
        assert_eq!(row_name_source(0x2005), RowNameSource::Spell(5));
        assert_eq!(row_name_source(0x5081), RowNameSource::Spell(0x81));
        assert_eq!(row_name_source(0x3077), RowNameSource::Item(0x77));
        assert_eq!(row_name_source(0x7010), RowNameSource::Item(0x10));
        assert_eq!(row_name_source(0xA020), RowNameSource::Item(0x20));
        assert_eq!(row_name_source(0x1C03), RowNameSource::BagSlot(3));
        assert_eq!(row_name_source(0x6002), RowNameSource::BagSlot(2));
        assert_eq!(row_name_source(0x9001), RowNameSource::BagSlot(1));
        assert_eq!(row_name_source(0x8004), RowNameSource::Landmark(4));
        assert_eq!(row_name_source(0x4002), RowNameSource::Verb(2));
        assert_eq!(row_name_source(0x0005), RowNameSource::Empty);
        // Payload masks 0x3FF: the dim / alt-ink bits never leak into it.
        assert_eq!(row_name_source(0x1800 | 7), RowNameSource::BagSlot(7));
    }

    #[test]
    fn row_description_source_laws() {
        let bag = [0u8, 0x77, 0x10, 0];
        let at = |slot: u16| bag[slot as usize];
        // Parked list draws nothing.
        assert_eq!(
            row_description_source(LIST_MODE_PARKED, CLASS_BAG, 1, at),
            DescriptionSource::None
        );
        // Bag classes dereference the slot.
        assert_eq!(
            row_description_source(1, CLASS_BAG, 1, at),
            DescriptionSource::ItemDesc(0x77)
        );
        assert_eq!(
            row_description_source(1, CLASS_BAG_EQUIP, 2, at),
            DescriptionSource::ItemDesc(0x10)
        );
        // Passive class routes to the accessory-passive chain.
        assert_eq!(
            row_description_source(1, CLASS_PASSIVE, 1, at),
            DescriptionSource::PassiveDesc(0x77)
        );
        // 0x7000 uses the payload as the id directly.
        assert_eq!(
            row_description_source(1, CLASS_ITEM_ICON, 0x42, at),
            DescriptionSource::ItemDesc(0x42)
        );
        // A resolved id of 0 draws nothing (empty bag slot).
        assert_eq!(
            row_description_source(1, CLASS_BAG, 0, at),
            DescriptionSource::None
        );
        // Other classes draw nothing.
        assert_eq!(
            row_description_source(1, CLASS_SPELL, 1, at),
            DescriptionSource::None
        );
    }

    /// Fixture tables: item ids pick fixed (kind, subtype, price) rows.
    struct FakeTables;
    impl ItemRowTables for FakeTables {
        fn kind(&self, id: u8) -> u8 {
            match id {
                0x30..=0x3F => 1,        // equipment band
                0x40..=0x4F | 0x88 => 2, // consumables band (+ Door of Light)
                _ => 0,
            }
        }
        fn subtype(&self, id: u8) -> u8 {
            id & 0x0F
        }
        fn price(&self, id: u8) -> u16 {
            if id & 1 == 0 { 100 } else { 0 }
        }
        fn effect_flags(&self, subtype: u8) -> u8 {
            match subtype {
                0 => EFFECT_FLAG_FIELD_USABLE | EFFECT_FLAG_BATTLE_USABLE,
                1 => EFFECT_FLAG_TAIL_GROUP,
                2 => EFFECT_FLAG_BATTLE_USABLE, // battle-only
                3 => EFFECT_FLAG_FIELD_USABLE,  // field-only
                4 => EFFECT_FLAG_NOT_DISCARDABLE | EFFECT_FLAG_FIELD_USABLE,
                _ => 0,
            }
        }
        fn equip_flags(&self, subtype: u8) -> u8 {
            if subtype == 2 {
                EQUIP_FLAG_NO_DISCARD
            } else {
                0
            }
        }
    }

    #[test]
    fn use_list_field_context_ordering_and_dims() {
        // Slots: [usable heal, equipment, tail-flag item, empty,
        //         battle-only item, field-usable-but-inapplicable]
        let bag = [0x40, 0x30, 0x41, 0x00, 0x42, 0x43];
        let applicable = |id: u8| id != 0x43;
        let ctx = UseListCtx {
            battle: false,
            door_light_blocked: false,
            door_wind_blocked: false,
            applicable: &applicable,
        };
        let rows = build_use_list_rows(&bag, 0, &FakeTables, &ctx);
        assert_eq!(
            rows,
            vec![
                0x1000, // slot 0: white in place
                0x1804, // slot 4: battle-only -> dim, in place (field ctx)
                0x1805, // slot 5: applicability probe failed -> dim in place
                0x1801, // slot 1: kind-1 equipment sorts after in-place rows
                0x1C02, // slot 2: kind-2 flag-8 tail group sorts last
            ]
        );
    }

    #[test]
    fn use_list_door_gates_dim_in_place() {
        let bag = [0x88];
        let yes = |_: u8| true;
        let blocked = UseListCtx {
            battle: false,
            door_light_blocked: true,
            door_wind_blocked: false,
            applicable: &yes,
        };
        // Door of Light is kind 2 subtype 8 (flags 0) - without the
        // scene gate it dims through the field-usable check anyway; the
        // gate short-circuits before the effect lookup.
        assert_eq!(
            build_use_list_rows(&bag, 0, &FakeTables, &blocked),
            vec![0x1800]
        );
    }

    #[test]
    fn use_list_battle_context_tails_unusable_rows() {
        // [battle-usable, field-only, equipment]
        let bag = [0x42, 0x43, 0x30];
        let yes = |_: u8| true;
        let ctx = UseListCtx {
            battle: true,
            door_light_blocked: false,
            door_wind_blocked: false,
            applicable: &yes,
        };
        let rows = build_use_list_rows(&bag, 0, &FakeTables, &ctx);
        // Battle context: unusable non-equipment rows join the tail
        // buffer (unlike the field context where they dim in place).
        assert_eq!(rows, vec![0x1000, 0x1801, 0x1802]);
    }

    #[test]
    fn throw_out_gates() {
        // [normal, key item (flag 1), equipment discardable,
        //  equipment no-discard, tail-flag]
        let bag = [0x40, 0x44, 0x31, 0x32, 0x41];
        let rows = build_throw_out_rows(&bag, 0, &FakeTables);
        assert_eq!(
            rows,
            vec![
                0x1000, // slot 0 white in place
                0x1801, // slot 1 key item dim in place
                0x1002, // slot 2 discardable equipment (white, tail)
                0x1803, // slot 3 no-discard equipment (dim, tail)
                0x1C04, // slot 4 tail-flag group last
            ]
        );
    }

    #[test]
    fn price_gated_rows_sort_unsellable_last() {
        // Even ids price 100, odd ids price 0.
        let bag = [0x40, 0x41, 0x00, 0x42];
        let rows = build_price_gated_rows(&bag, 0, &FakeTables);
        assert_eq!(rows, vec![0x1000, 0x1003, 0x1801]);
    }

    #[test]
    fn slot_base_offsets_payloads() {
        let bag = [0x40];
        let rows = build_price_gated_rows(&bag, 5, &FakeTables);
        assert_eq!(rows, vec![0x1005]);
    }

    #[test]
    fn record_string_glyph_count_sums_segments() {
        // [3][6 bytes][2][4 bytes][0]
        let rec = [3u8, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0];
        assert_eq!(record_string_glyph_count(&rec), 5);
        assert_eq!(record_string_glyph_count(&[0]), 0);
        assert_eq!(record_string_glyph_count(&[]), 0);
    }

    #[test]
    fn live_window_upsert_sorted_and_reused() {
        let mut set = LiveWindowSet::new();
        set.upsert(LiveWindow {
            id: 15,
            ..Default::default()
        });
        set.upsert(LiveWindow {
            id: 3,
            ..Default::default()
        });
        set.upsert(LiveWindow {
            id: 9,
            ..Default::default()
        });
        let ids: Vec<u16> = set.iter().map(|w| w.id).collect();
        assert_eq!(ids, vec![3, 9, 15]);
        // Upsert on an existing id reuses the node (no duplicate).
        set.upsert(LiveWindow {
            id: 9,
            x: 40,
            ..Default::default()
        });
        let ids: Vec<u16> = set.iter().map(|w| w.id).collect();
        assert_eq!(ids, vec![3, 9, 15]);
        assert_eq!(set.get(9).unwrap().x, 40);
    }
}

#[cfg(test)]
mod shop_buy_row_tests {
    use super::*;

    /// The order kernel reproduces the three record shapes the disc carries:
    /// a three-id template tail (no hoist), a one-id tail (two hoisted) and
    /// no tail at all (three hoisted).
    #[test]
    fn buy_row_order_hoists_three_minus_padding() {
        // 10 declared ids, 3 of them template padding -> walk 7, split 7.
        assert_eq!(shop_buy_row_order(10, 7), vec![0, 1, 2, 3, 4, 5, 6]);
        // 15 declared ids, 1 template id -> walk 14, split 12.
        assert_eq!(
            shop_buy_row_order(15, 14),
            vec![12, 13, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
        );
        // 10 declared ids, no padding -> walk 10, split 7.
        assert_eq!(
            shop_buy_row_order(10, 10),
            vec![7, 8, 9, 0, 1, 2, 3, 4, 5, 6]
        );
    }

    /// Rim Elm's Variety Shop: ten ids, no template padding. Retail hoists
    /// the last three (Hunter Clothes / Scarlet Jewel / Azure Jewel) to the
    /// top tagged [`CLASS_SHOP_ALT`], which is exactly the order and the
    /// "new in this town" marking the curated walkthrough table carries.
    #[test]
    fn buy_rows_hoist_the_featured_band() {
        let ids = [0x22, 0x34, 0x59, 0xD6, 0x77, 0x7E, 0x88, 0x43, 0xC7, 0xC8];
        let rows = build_shop_buy_rows(&ids, true, 1_000_000, |_| 100, |_| 0);
        let payload: Vec<u8> = rows.iter().map(|w| (w & 0xFF) as u8).collect();
        assert_eq!(
            payload,
            vec![0x43, 0xC7, 0xC8, 0x22, 0x34, 0x59, 0xD6, 0x77, 0x7E, 0x88]
        );
        // The hoisted band is the alt class; the rest is the plain item class.
        let classes: Vec<u16> = rows.iter().map(|w| w & ROW_CLASS_MASK).collect();
        assert_eq!(&classes[..3], &[CLASS_SHOP_ALT; 3]);
        assert!(classes[3..].iter().all(|&c| c == CLASS_ITEM));
        assert!(rows.iter().all(|w| w & ROW_DISABLED == 0));
    }

    /// A three-id template tail keeps record order and drops the padding:
    /// the sub-`0x1A` ids shrink the walk, so the emit loop never reaches
    /// the split.
    #[test]
    fn buy_rows_drop_the_template_tail_and_keep_order() {
        let ids = [0xD3, 0xD4, 0x77, 0x78, 0x7C, 0x7F, 0x88, 0x01, 0x02, 0x03];
        let rows = build_shop_buy_rows(&ids, true, 1_000_000, |_| 100, |_| 0);
        let payload: Vec<u8> = rows.iter().map(|w| (w & 0xFF) as u8).collect();
        assert_eq!(payload, vec![0xD3, 0xD4, 0x77, 0x78, 0x7C, 0x7F, 0x88]);
        assert!(rows.iter().all(|w| w & ROW_CLASS_MASK == CLASS_ITEM));
    }

    /// The dim bit is an OR of the two gates: purse below price, or a bag
    /// already holding the cap. Nothing else sets it - there is no alt-ink
    /// tier on this list.
    #[test]
    fn buy_rows_dim_on_purse_or_full_stack() {
        let ids = [0x22, 0x34, 0x59];
        let rows = build_shop_buy_rows(
            &ids,
            true,
            250,
            |id| if id == 0x22 { 180 } else { 400 },
            |id| if id == 0x59 { SHOP_ROW_HELD_CAP } else { 0 },
        );
        // Order: split = 0, so all three are the hoisted class in record
        // order; only the affordable, non-full row stays white.
        let dim: Vec<bool> = rows.iter().map(|w| w & ROW_DISABLED != 0).collect();
        assert_eq!(dim, vec![false, true, true]);
        assert!(rows.iter().all(|w| w & ROW_ALT_INK == 0));
    }

    /// The no-room probe drops the three reserved tail slots outright.
    #[test]
    fn buy_rows_without_room_lose_the_reserved_tail() {
        let ids = [0x22, 0x34, 0x59, 0xD6, 0x77, 0x7E, 0x88, 0x43, 0xC7, 0xC8];
        let rows = build_shop_buy_rows(&ids, false, 1_000_000, |_| 100, |_| 0);
        let payload: Vec<u8> = rows.iter().map(|w| (w & 0xFF) as u8).collect();
        assert_eq!(payload, vec![0x22, 0x34, 0x59, 0xD6, 0x77, 0x7E, 0x88]);
    }
}
