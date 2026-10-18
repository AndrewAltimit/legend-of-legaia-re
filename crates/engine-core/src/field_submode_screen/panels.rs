//! Panel-window geometry and slot tables: window origins, hub equip-panel slots, pad masks
//! and the op-`0x49` sub-op to handler-slot map.

use super::*;

/// Where a panel-window record's open places its window actor: the record's
/// geometry words at `+8` / `+0xA` of the field overlay's table
/// (`0x801F2B98`, stride `0x1C`), read from PROT 0897's bytes. Records `0..=3`
/// are the `0x2A` kind with no painter. Record `8`'s top is rewritten at run
/// time by the start menu (`HubAction::SizePanel`); this is its disc value.
pub const PANEL_WINDOW_ORIGINS: [(i16, i16); hub::PANEL_WINDOW_COUNT] = [
    (0x10, 0x90),
    (0x10, 0x90),
    (0x10, 0x90),
    (0x10, 0x90),
    (0x0E, 0x70),
    (0xB0, 0x28),
    (0x28, 0x6C),
    (0x10, 0x9A),
    (0x22, 0x54),
    (0x20, 0x60),
    (0x40, 0x26),
    (0x40, 0x84),
    (0x40, 0x90),
    (0x20, 0x63),
    (0x18, 0x90),
    (0x60, 0x58),
    (0x22, 0x20),
];

/// [`PANEL_WINDOW_ORIGINS`] for `idx`, `(0, 0)` past the table.
pub fn panel_window_origin(idx: usize) -> (i16, i16) {
    PANEL_WINDOW_ORIGINS.get(idx).copied().unwrap_or((0, 0))
}

/// Whether descriptor `desc`'s program begins with the installer's sub-op
/// `5`, which calls `FUN_80035A4C`: a walk of the live window list that
/// starts the close of every window still opening or open (state `0` / `1`
/// to `-1`). A program that opens with it replaces what was up; one that does
/// not **adds** to it.
///
/// The coin counter is the case that matters: its idle program is
/// `[5, 0] [6, 10] [1, 10]` and its confirm program is `[1, 11]` alone, so
/// retail keeps the entry panel (record `10`) up under the Yes/No panel
/// (record `11`). Read from the descriptors in PROT 0897; the sub-op table is
/// `0x801CF25C` (`FUN_801E9B3C`, `0x801E9BA4..0x801E9BC4`).
pub fn descriptor_closes_all(desc: u32) -> bool {
    !matches!(
        desc,
        hub::PANEL_COIN_CONFIRM | hub::PANEL_SUBMENU_IDLE | hub::PANEL_SUBMENU_CONFIRM
    )
}

/// Dispatch one `PTR_FUN_801F33B4` slot.
///
/// The slots without a ported body fall through to nothing, which is retail's
/// own behaviour for an index whose handler is a stub - the dispatcher still
/// runs its release arm afterwards, so the screen cannot wedge.
pub(super) fn run_slot(
    actor: &mut HubActor,
    env: &HubEnv,
    cursor: &mut HubGrid,
    counter: &mut CoinCounter,
) -> HubFrame {
    match actor.state {
        slot::COIN_COUNTER => hub::coin_exchange(actor, env, counter, cursor),
        slot::START_MENU => hub::start_menu(actor, env, cursor),
        slot::PROMPT => hub::hub_prompt(actor, env, cursor),
        slot::SUBMENU => hub::submenu(actor, env, cursor),
        slot::DEACTIVATE => hub::deactivate(actor, env, cursor),
        slot::DRAW_TICK => hub::draw_tick(actor, env, cursor),
        slot::CLOSE_TICK | 0x14..=0x18 => hub::close_tick(actor, env, cursor),
        _ => HubFrame::default(),
    }
}

/// Retail's per-character **weapon slot** table `DAT_8007B42C` (`SCUS_942.54`
/// file `0x6BC2C`, halfword per character), which `resolve_equip_slot` indexes
/// for a candidate whose `+7` byte says weapon.
///
/// The disc holds `2, 3, 2`: Vahn and Gala carry their weapon in equip byte
/// `2`, Noa in byte `3`. The halfword after them is `0`, and an index past the
/// pinned three resolves to `0` as well, so the two agree for every character.
/// Only entry codes below `3` draw a panel at all.
pub const RETAIL_WEAPON_SLOTS: [i16; 3] = [2, 3, 2];

/// The three fixed slots of retail's `+0x196` array, as
/// `(retail index, the engine slot holding that item)`: byte `0` is **body
/// armour**, `1` the **head** slot, `4` **footwear**. Bytes `2` / `3` are the
/// weapon, per [`RETAIL_WEAPON_SLOTS`].
///
/// That is retail's own order, and it is not the engine's. Two disc tables pin
/// it independently: the panel's own slot resolution
/// (`(+7 & 0x60) >> 5` -> `0`, `1`, the weapon table, `4`) and the equip
/// screen's row map `DAT_801E43E8` = `00 01 00 04 05 06 07`, whose seven rows
/// are weapon (overridden by the per-character halfword), helmet `1`, body
/// armour `0`, footwear `4` and three Goods slots `5..7`. The engine's own
/// array is weapon-first with a hand-guard slot retail has no row for
/// ([`legaia_engine_menus::equip_session::ARMAMENT_ENGINE_SLOTS`]), so a record has to be
/// re-ordered before the ported kernel walks it - [`hub_panel_slots`].
pub const HUB_PANEL_FIXED_SLOTS: [(usize, EquipSlot); 3] = [
    (0, EquipSlot::BodyArmor),
    (1, EquipSlot::Helmet),
    (4, EquipSlot::Boot),
];

/// Re-order a character's engine equip array into the five slots the sub-panel
/// walks, in retail's own `+0x196` order.
///
/// The aggregation itself is order-blind (it sums all five), so this matters
/// for one thing: the **trial-equip destination**. `resolve_equip_slot` answers
/// in retail's index space, so handing it an engine-ordered array would displace
/// the wrong item - a body-armour candidate would replace the weapon.
///
/// The engine's hand-guard slot has no retail counterpart and is dropped, which
/// is retail's own five-slot sum; retail's three Goods slots (`5..7`) are
/// outside the walk for both.
pub fn hub_panel_slots(equip: &[u8; 8], char_index: usize) -> [u8; 5] {
    let mut out = [0u8; 5];
    for (retail, engine) in HUB_PANEL_FIXED_SLOTS {
        out[retail] = equip[engine.as_index() as usize];
    }
    let weapon = RETAIL_WEAPON_SLOTS
        .get(char_index)
        .copied()
        .unwrap_or(0)
        .clamp(0, 4) as usize;
    out[weapon] = equip[EquipSlot::Weapon.as_index() as usize];
    out
}

/// Rebuild an equipment record's `+7` **slot byte** from the disc restriction
/// view: the four `& 0x60` categories plus the `0x01` Ra-Seru bit.
///
/// The panel reads `+7 & 0x60` only; the Ra-Seru bit rides along because it is
/// the same byte and the view carries it.
pub fn disc_slot_bits(entry: DiscEquipEntry) -> u8 {
    use legaia_game_tables::equip_stats::EquipSlot as Disc;
    let category = match entry.category {
        Disc::Body => 0x00,
        Disc::Head => 0x20,
        Disc::Weapon => 0x40,
        Disc::Footwear => 0x60,
    };
    category | u8::from(entry.is_ra_seru)
}

/// Panel width the driver actor carries (`+0x0E`), the anchor the right-edge
/// cursor of the single-label painters is measured from.
pub const SUBMODE_PANEL_WIDTH: i16 = 0x60;

/// Accept edge in the packed Legaia pad layout (Cross), standing in for
/// `_DAT_800846D0`.
pub const SUBMODE_ACCEPT_MASK: u32 = legaia_engine_system::pad::PACK_CROSS as u32;
/// Back-out edge (Circle), standing in for `_DAT_800846D4`.
pub const SUBMODE_BACK_MASK: u32 = legaia_engine_system::pad::PACK_CIRCLE as u32;

/// Panel-window record whose painter draws the Yes/No panel
/// (`FUN_801F1950`).
///
/// Not the coin counter's: that screen installs
/// [`legaia_engine_field::submode_hub_actors::PANEL_COIN_CONFIRM`], which names the
/// three-line record. Kept because a host that wants the Yes/No panel by
/// itself has no descriptor to install.
pub const COIN_PANEL_WINDOW: usize = hub::window::TWO_OPTION;

/// Op-`0x49` sub-ops the world handles through a dedicated path rather than
/// through a submode screen: `0` inline gold shop, `3` name entry, `5` tile
/// board, `7` casino prize exchange.
///
/// The retail table agrees about all four: sub-`0` and sub-`7` select no
/// handler at all (`-1` - sub-`7` routes through the menu-overlay outer
/// dispatcher `FUN_801DC6B4` into sub-screen `0x20` instead), sub-`3`
/// selects `FUN_801F03F0` (name entry) and sub-`5` selects `FUN_801EF2B0`
/// (the tile-board walk). The engine reaches all of them through its own
/// host paths ([`World::try_arm_field_shop`] /
/// [`World::try_arm_prize_exchange`] / name entry / tile board), so it
/// keeps them out of the dispatcher.
pub const OP49_DEDICATED_SUB_OPS: [u8; 4] = [0, 3, 5, 7];

use legaia_engine_field::field_subsystem_enter::{STATE_PICK_HANDLER, installer_entry};

/// The handler id the subsystem actor's enter half `FUN_801F1278` leaves in
/// `+0x50` for an op-`0x49` sub-op: [`STATE_PICK_HANDLER`] unless the sub-op's
/// byte in [`hub::OP49_SUBOP_SLOTS`] (`0x801F33A4`) names a screen. `None`
/// for a sub-op past the table, which the op's own range test refuses before
/// it arms.
///
/// This is the one place the engine makes that decision, and it makes it by
/// running the installer kernel itself
/// ([`legaia_engine_field::field_subsystem_enter::installer_entry`]) rather
/// than a second reading of the table. Only the handler is read: the caller's
/// saved handler, the cursor home and the roster cells the same call computes
/// land in the submode context `*_DAT_801C6EA4`, of which the engine mirrors
/// only the fields the dispatcher reads (`SubmodeScreen::cursor`, seeded by
/// `World::open_field_submode_screen`).
fn installed_handler(sub_op: u8) -> Option<u16> {
    let &remap = hub::OP49_SUBOP_SLOTS.get(usize::from(sub_op))?;
    Some(installer_entry(0, &[], Some(remap)).handler)
}

/// The handler slot an op-`0x49` sub-op opens.
///
/// This is retail's own selection: the enter half installs the state pick,
/// then overwrites it with the 14-byte table
/// [`legaia_engine_field::submode_hub_actors::OP49_SUBOP_SLOTS`] at
/// `0x801F33A4` unless the row is `-1` ([`installed_handler`]). The engine
/// previously had no mapping and opened the close tick for every sub-op, so
/// every screen but the three dedicated ones closed itself immediately
/// instead of running - the script unparked, but nothing drew.
///
/// `None` for the sub-ops the engine routes through dedicated host paths
/// ([`OP49_DEDICATED_SUB_OPS`]) and for a row whose state pick stands - a
/// scripted menu press ([`op49_sub_op_is_menu_press`]), which opens the
/// pause menu instead of a submode screen.
pub fn slot_for_op49_sub_op(sub_op: u8) -> Option<u16> {
    if OP49_DEDICATED_SUB_OPS.contains(&sub_op) {
        return None;
    }
    match installed_handler(sub_op) {
        // Past the table: the close tick unparks it.
        None => Some(slot::CLOSE_TICK),
        Some(STATE_PICK_HANDLER) => None,
        Some(handler) => Some(handler),
    }
}

/// Whether an op-`0x49` sub-op is a **scripted menu-button press**: no
/// dedicated host path, and the enter half leaves the state pick
/// `FUN_801F1F4C` installed, which with a park live picks the pause-menu
/// session. Retail's rows of that shape are sub-op `1` (a save point) and
/// `0x0D`.
pub fn op49_sub_op_is_menu_press(sub_op: u8) -> bool {
    !OP49_DEDICATED_SUB_OPS.contains(&sub_op)
        && installed_handler(sub_op) == Some(STATE_PICK_HANDLER)
}
