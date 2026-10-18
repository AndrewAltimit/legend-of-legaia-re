//! Driving the engine's pause menu to the screen a retail state was saved
//! on, by scripted pad edges.

use super::*;

/// Ticks between two scripted pad edges of a menu drive: long enough for
/// the menu's open / hand-off beats, which swallow the edge that caused them.
pub const MENU_PRESS_GAP: u64 = 12;

/// Extra ticks the first press after `Start` waits: the field darkens for up
/// to fourteen ticks (`0x83 / 10` at a frame step of `1`) before the menu
/// exists and takes a press (`NativeSession::pause_wipe`, retail
/// `FUN_801ED308` phase 1), longer than one [`MENU_PRESS_GAP`].
pub const MENU_OPEN_WIPE_TICKS: u64 = MENU_PRESS_GAP;

/// The pad edges that reach `menu` from a settled field, as `(tick offset,
/// button)` pairs from the first press: `Start`, `Down` until the root
/// cursor (opening on row `0`) sits on the row, `Cross`. The Equip row opens
/// on its character picker (`0x12`); the slot browse (`0x13`) is one more
/// `Cross`, and the candidate list (`0x14`) a `Down` past Best Equipment and
/// a `Cross` after that.
pub fn pause_menu_presses(menu: &RetailMenu) -> Vec<(u64, legaia_engine_system::input::PadButton)> {
    use legaia_engine_system::input::PadButton;
    let mut out = vec![(0, PadButton::Start)];
    let mut t = MENU_OPEN_WIPE_TICKS;
    let mut press = |b: PadButton| {
        t += MENU_PRESS_GAP;
        out.push((t, b));
    };
    for _ in 0..menu.row.index() {
        press(PadButton::Down);
    }
    press(PadButton::Cross);
    if menu.equip_depth >= 1 {
        press(PadButton::Cross);
    }
    if menu.equip_depth >= 2 {
        press(PadButton::Down);
        press(PadButton::Cross);
    }
    out
}

/// Ticks a menu drive runs from its first press to the sample.
pub fn pause_menu_drive_ticks(menu: &RetailMenu) -> u64 {
    pause_menu_presses(menu).last().map_or(0, |p| p.0) + 2 * MENU_PRESS_GAP
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
            other => other.row().retail_subscreen(),
        }),
    })
}
