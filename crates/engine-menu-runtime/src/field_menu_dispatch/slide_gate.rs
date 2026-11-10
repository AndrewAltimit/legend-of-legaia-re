//! The pause menu's window-slide input lock
//! ([`legaia_engine_menus::menu_slide_lock`]) over the port's screens: which
//! retail sub-screen the root list and the open sub-session stand on, and the
//! one per-frame gate every host's pause-menu step calls before it routes the
//! pad.

use super::FieldMenuSubsession;
use legaia_engine_core::world::World;
use legaia_engine_menus::equip_session::EquipState;
use legaia_engine_menus::field_menu::{FieldMenuPhase, FieldMenuSession};
use legaia_engine_menus::menu_slide_lock::MenuScreen;
use legaia_engine_menus::pause_screens::{PauseItemsFocus, UseRoute};
use legaia_engine_menus::save_select::SaveSelectMode;
use legaia_engine_menus::spell_menu::SpellMenuPhase;

/// A port-only screen (the Arts editor, the expansion Journal): no retail
/// sub-screen, no program, no lock.
pub const PORT_ONLY_SUBSCREEN: u8 = 0xFF;

/// The retail sub-screen (`DAT_801E46A4`) the pause menu stands on, `None`
/// once the root session is done.
pub fn pause_screen(
    menu: &FieldMenuSession,
    sub: Option<&FieldMenuSubsession>,
) -> Option<MenuScreen> {
    match menu.phase() {
        FieldMenuPhase::Browsing { .. } => Some(MenuScreen::at(0x01)),
        FieldMenuPhase::ReadyConfirm { .. } => Some(MenuScreen::at(0x03)),
        FieldMenuPhase::Notice => Some(MenuScreen::at(0x04)),
        FieldMenuPhase::Suspended { row } => Some(
            sub.map(subsession_screen)
                .unwrap_or_else(|| MenuScreen::at(row.retail_subscreen())),
        ),
        FieldMenuPhase::Done(_) => None,
    }
}

/// The retail sub-screen an open sub-session stands on.
pub fn subsession_screen(sub: &FieldMenuSubsession) -> MenuScreen {
    match sub {
        FieldMenuSubsession::Items(s) => match s.focus {
            PauseItemsFocus::Command => MenuScreen::at(0x05),
            // The target pick is the apply screen (`0x09` all-party and
            // `0x0A` single-target run the same program).
            PauseItemsFocus::List if s.target_select() => MenuScreen::at(0x0A),
            PauseItemsFocus::List => MenuScreen::at(0x06),
            PauseItemsFocus::ThrowOutList => MenuScreen::at(0x07),
            PauseItemsFocus::ThrowOutConfirm => MenuScreen::step(0x07, 1),
            PauseItemsFocus::SpecialRoute => match s.special_use().map(|u| u.route) {
                Some(UseRoute::DoorOfLight) => MenuScreen::at(0x0B),
                Some(UseRoute::DoorOfWind) => MenuScreen::at(0x0C),
                Some(UseRoute::Incense) => MenuScreen::at(0x0D),
                _ => MenuScreen::at(0x06),
            },
        },
        FieldMenuSubsession::Equip { picking: true, .. } => MenuScreen::at(0x12),
        FieldMenuSubsession::Equip { session, .. } => match session.state() {
            EquipState::ItemPicker { .. } => MenuScreen::at(0x14),
            _ => MenuScreen::at(0x13),
        },
        FieldMenuSubsession::Spells(s) => MenuScreen::at(match s.phase() {
            SpellMenuPhase::CharSelect { .. } | SpellMenuPhase::Done(_) => 0x0E,
            SpellMenuPhase::SpellSelect { .. } => 0x0F,
            SpellMenuPhase::GroupConfirm { .. } => 0x10,
            SpellMenuPhase::TargetSelect { .. } => 0x11,
        }),
        FieldMenuSubsession::ListOrder(_) | FieldMenuSubsession::Status(_) => MenuScreen::at(0x15),
        FieldMenuSubsession::Config(_) => MenuScreen::at(0x17),
        FieldMenuSubsession::Save(s) => MenuScreen::at(match s.mode() {
            SaveSelectMode::Load => 0x18,
            SaveSelectMode::Save => 0x19,
        }),
        FieldMenuSubsession::Arts(_) | FieldMenuSubsession::Journal { .. } => {
            MenuScreen::at(PORT_ONLY_SUBSCREEN)
        }
    }
}

/// One pause-menu frame of the window-slide input lock: call once per menu
/// frame, before the root list or the sub-session ticks, and route the pad
/// only when it answers `true` (every host's pause-menu step does). Retail's
/// sub-screens read `_DAT_8007BB80` ahead of the pad and take nothing while
/// a window is sliding ([`legaia_engine_menus::menu_slide_lock`]).
///
/// `menu` `None` (no pause menu) drops the lock's window list.
///
/// REF: FUN_801D6B20 (`0x801D6B90`, the root picker's latch read)
pub fn pause_slide_gate(
    world: &mut World,
    menu: Option<&FieldMenuSession>,
    sub: Option<&FieldMenuSubsession>,
) -> bool {
    let screen = menu.and_then(|m| pause_screen(m, sub));
    let delta = legaia_engine_menus::menu_slide_lock::MENU_FRAME_DELTA;
    let state = &mut world.menu;
    state.pause_slide_lock.frame(
        &mut state.widgets,
        state.widget_scripts.as_ref(),
        screen,
        delta,
    )
}

/// Whether the next [`pause_slide_gate`] lets the pad through: the lock has
/// seen the screen the menu stands on and no window is moving. A driver that
/// presses into the menu (a test, a scripted hand) waits on this rather than
/// on a frame count. Always `true` without the overlay's programs, where the
/// lock is open.
pub fn pause_slide_settled(
    world: &World,
    menu: Option<&FieldMenuSession>,
    sub: Option<&FieldMenuSubsession>,
) -> bool {
    if world.menu.widget_scripts.is_none() {
        return true;
    }
    let screen = menu.and_then(|m| pause_screen(m, sub));
    world.menu.pause_slide_lock.screen() == screen && world.menu.widgets.slide_latch() == 0
}
