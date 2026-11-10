//! Driving the engine's pause menu to the screen a retail state was saved
//! on, by scripted pad edges.

use super::*;

/// Ticks between two scripted pad edges of a menu drive: long enough for
/// the window-slide lock a screen change raises
/// (`legaia_engine_menus::menu_slide_lock`). Retail's next sub-screen runs its
/// window program the frame after the press and reads no pad until the
/// twelve-frame slide is over, so the next press lands thirteen frames on at
/// the earliest; one more keeps the drive off the edge.
pub const MENU_PRESS_GAP: u64 = 14;

/// Extra ticks the first press after `Start` waits: the field darkens for up
/// to fourteen ticks (`0x83 / 10` at a frame step of `1`) before the menu
/// exists (`NativeSession::pause_wipe`, retail `FUN_801ED308` phase 1), and
/// the root picker's windows then slide in for twelve more before it reads
/// the pad. With one [`MENU_PRESS_GAP`] on top, the first press lands at
/// tick 28.
pub const MENU_OPEN_WIPE_TICKS: u64 = 14;

/// Ticks a card screen's grid takes to come up after its pill-row confirm:
/// the "Now checking" card read runs first (`FUN_801DD35C` sub-modes `0x04`
/// / `0x05`). Measured on `play-window`: the grid is up about `100` ticks
/// after the confirm.
pub const CARD_READ_SETTLE_TICKS: u64 = 160;

/// Ticks the prize counter's windows take to slide in after the arm.
pub const PRIZE_SETTLE_TICKS: u64 = 90;

/// Ticks the map display takes to fade up after `L1`.
pub const MAP_DISPLAY_SETTLE_TICKS: u64 = 120;

/// Ticks between two scripted presses inside a shop: the widget scripts of
/// the screen a confirm opens run before the next one takes the pad.
pub const SHOP_PRESS_GAP: u64 = 45;

/// The pad edges that reach `menu` from a settled field, as `(tick offset,
/// button)` pairs from the first press: `Start`, `Down` until the root
/// cursor (opening on row `0`) sits on the row, `Cross`. The Equip row opens
/// on its character picker (`0x12`); the slot browse (`0x13`) is one more
/// `Cross`, and the candidate list (`0x14`) a `Down` past Best Equipment and
/// a `Cross` after that.
pub fn pause_menu_presses(menu: &RetailMenu) -> Vec<(u64, legaia_engine_system::input::PadButton)> {
    use legaia_engine_system::input::PadButton;
    if menu.prize_block.is_some() {
        return Vec::new();
    }
    if menu.map_display {
        return vec![(0, PadButton::L1)];
    }
    if menu.shop_instr.is_some() {
        return (0u64..)
            .zip(&menu.path)
            .map(|(i, b)| (PRIZE_SETTLE_TICKS + i * SHOP_PRESS_GAP, *b))
            .collect();
    }
    let mut out = vec![(0, PadButton::Start)];
    let mut t = MENU_OPEN_WIPE_TICKS;
    let mut press = |b: PadButton| {
        t += MENU_PRESS_GAP;
        out.push((t, b));
    };
    // The root list is `Start` alone: the hand rests on its first row.
    if menu.subscreen == MENU_ROOT {
        return out;
    }
    for _ in 0..menu.row.index() {
        press(PadButton::Down);
    }
    press(PadButton::Cross);
    for b in &menu.path {
        press(*b);
    }
    if menu.equip_depth >= 1 {
        press(PadButton::Cross);
    }
    if menu.equip_depth >= 2 {
        press(PadButton::Down);
        press(PadButton::Cross);
    }
    // A Load / Save grid is one confirm past its pill row, on the port the
    // card in port 1 sits on (`SLOT 1`, the pill the screen opens on).
    if menu.card_grid {
        press(PadButton::Cross);
    }
    out
}

/// Ticks a menu drive runs from its first press to the sample.
pub fn pause_menu_drive_ticks(menu: &RetailMenu) -> u64 {
    if menu.prize_block.is_some() {
        return PRIZE_SETTLE_TICKS;
    }
    if menu.map_display {
        return MAP_DISPLAY_SETTLE_TICKS;
    }
    if menu.shop_instr.is_some() {
        return PRIZE_SETTLE_TICKS + menu.path.len() as u64 * SHOP_PRESS_GAP + PRIZE_SETTLE_TICKS;
    }
    let settle = if menu.card_grid {
        CARD_READ_SETTLE_TICKS
    } else if menu.subscreen == MENU_ROOT {
        MENU_OPEN_WIPE_TICKS + 2 * MENU_PRESS_GAP
    } else {
        2 * MENU_PRESS_GAP
    };
    pause_menu_presses(menu).last().map_or(0, |p| p.0) + settle
}

/// Drive the headless session's pause menu to `menu` through its pad path
/// ([`pause_menu_presses`], each a one-tick edge), settle, and read back the
/// sub-screen it holds.
pub(super) fn drive_pause_menu(
    session: &mut NativeSession,
    menu: &RetailMenu,
) -> Result<Option<u8>> {
    use legaia_engine_menu_runtime::field_menu_dispatch::FieldMenuSubsession;
    use legaia_engine_menus::equip_session::EquipState;
    use legaia_engine_menus::pause_screens::{PauseItemsFocus, UseRoute};
    use legaia_engine_menus::spell_menu::SpellMenuPhase;
    let other_row_id = |sub: &FieldMenuSubsession| sub.row().retail_subscreen();
    if let Some(block) = menu.prize_block {
        // A headless session reads no menu overlay on boot; its table is what
        // the arm stands on.
        let overlay = session.host.index.entry_bytes_extended(
            legaia_game_tables::menu_windows::MENU_OVERLAY_PROT_INDEX as u32,
        )?;
        session.host.world.install_menu_overlay_tables(&overlay);
        session
            .host
            .world
            .try_arm_prize_exchange(&[0x49, 0x07, block]);
        let mut screen = legaia_engine_menu_runtime::menu_runtime::MenuRuntime::new("");
        session.poll_field_overlay(&mut screen);
        return Ok(screen
            .prize_session
            .is_some()
            .then_some(MENU_PRIZE_EXCHANGE));
    }
    if let Some(instr) = &menu.shop_instr {
        let armed = session.host.world.try_arm_field_shop(instr);
        let mut screen = legaia_engine_menu_runtime::menu_runtime::MenuRuntime::new("");
        session.poll_field_overlay(&mut screen);
        return Ok(armed.then_some(menu.subscreen));
    }
    let presses = pause_menu_presses(menu);
    for t in 0..=pause_menu_drive_ticks(menu) {
        let mask = presses
            .iter()
            .filter(|p| p.0 == t)
            .fold(0u16, |m, p| m | p.1.mask());
        session.host.world.input.set_pad(mask);
        session.tick()?;
    }
    session.host.world.input.set_pad(0);
    if menu.map_display {
        return Ok(session.host.world.map_display_owns_pad().then_some(0));
    }
    Ok(match (&session.field_menu, &session.field_menu_sub) {
        (None, _) => None,
        (Some(_), None) => Some(0x01),
        // The Equip row's three steps: the character picker, the slot
        // browse and the candidate list.
        (Some(_), Some(sub)) => Some(match sub {
            FieldMenuSubsession::Equip { picking: true, .. } => MENU_EQUIP_PICK,
            FieldMenuSubsession::Equip { session, .. } => match session.state() {
                EquipState::SlotPicker { .. } => MENU_EQUIP_SLOTS,
                _ => MENU_EQUIP_CANDIDATES,
            },
            FieldMenuSubsession::Items(items) => match items.focus {
                PauseItemsFocus::Command => other_row_id(sub),
                PauseItemsFocus::ThrowOutList | PauseItemsFocus::ThrowOutConfirm => {
                    MENU_ITEMS_THROW
                }
                PauseItemsFocus::SpecialRoute => match items.special_use().map(|s| s.route) {
                    Some(UseRoute::DoorOfLight) => MENU_ITEMS_DOOR_LIGHT,
                    Some(UseRoute::DoorOfWind) => MENU_ITEMS_DOOR_WIND,
                    _ => MENU_ITEMS_INCENSE,
                },
                _ if matches!(
                    items.inner.state,
                    legaia_engine_menus::inventory_use::InventoryUseState::AllPartyConfirm { .. }
                ) =>
                {
                    MENU_ITEMS_APPLY_ALL
                }
                _ if items.target_select() => MENU_ITEMS_APPLY_ONE,
                _ => MENU_ITEMS_USE,
            },
            FieldMenuSubsession::Spells(spells) => match spells.phase() {
                SpellMenuPhase::CharSelect { .. } => other_row_id(sub),
                SpellMenuPhase::GroupConfirm { .. } => MENU_SPELL_GROUP,
                SpellMenuPhase::TargetSelect { .. } => MENU_SPELL_TARGET,
                _ => MENU_SPELL_LIST,
            },
            other => other.row().retail_subscreen(),
        }),
    })
}
