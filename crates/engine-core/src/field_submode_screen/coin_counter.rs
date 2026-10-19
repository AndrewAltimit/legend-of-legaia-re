//! The coin counter entry panel: its painter and the text lines it lays out.

use super::*;

impl World {
    /// Run the coin counter's entry panel ([`legaia_engine_minigames::slot_machine::coin_entry_panel`])
    /// when its record is installed, after the frame's commit has moved the
    /// bank and the gold - retail's window walk runs after the handler, so the
    /// painter reads the values the handler just wrote.
    pub(super) fn paint_coin_entry_panel(&mut self) {
        use legaia_engine_minigames::slot_machine::{
            COIN_ENTRY_ORIGIN, COIN_ENTRY_WINDOW, CoinPanelInput,
        };
        let s = &self.field_vm.submode_screen;
        let draws = if s.open && s.installed_windows.contains(&COIN_ENTRY_WINDOW) {
            legaia_engine_minigames::slot_machine::coin_entry_panel(&CoinPanelInput {
                origin: COIN_ENTRY_ORIGIN,
                digits: &s.counter.digits,
                cursor: s.counter.cursor,
                bank: self.minigames.casino_coins.min(i32::MAX as u32) as i32,
                gold: self.party.money,
                stock: s.counter.ceiling,
                clock: self.clock.display_frames as u32,
            })
        } else {
            Vec::new()
        };
        self.field_vm.submode_screen.coin_panel = draws;
    }
}

/// One positioned line of a submode screen, in retail 320x240 space: text in
/// the dialog-font encoding and the `_DAT_8007B454` pen it was staged in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmodeLine {
    pub text: Vec<u8>,
    pub x: i16,
    pub y: i16,
    pub pen: u8,
}

/// Cell pitch of the blank-padded number writer `FUN_80034B78`.
pub const SUBMODE_NUMBER_CELL_W: i16 = 8;

/// Stand-in glyph for the hand cursor sprite (`FUN_8002B994(0, 1, ..)`) until
/// the system-UI sprite page draws it.
pub const SUBMODE_CURSOR_STAND_IN: &[u8] = b">";
/// Stand-in glyph for the entry panel's caret cell `0x67`.
pub const SUBMODE_CARET_STAND_IN: &[u8] = b"^";
/// The pen both stand-ins take: the accent (`6`).
pub const SUBMODE_STAND_IN_PEN: u8 = 6;

/// A blank-padded decimal field (`FUN_80034B78`): the value's digits
/// right-aligned in `digits` 8-pixel cells, leading cells blank, one line per
/// digit so the cells keep their fixed pitch under a proportional font.
fn number_lines(value: i32, digits: u8, x: i16, y: i16, pen: u8, out: &mut Vec<SubmodeLine>) {
    let s = value.max(0).to_string();
    let len = s.len() as i16;
    for (i, ch) in s.bytes().enumerate() {
        let cell = (i16::from(digits) - len + i as i16).max(0);
        out.push(SubmodeLine {
            text: vec![ch],
            x: x.wrapping_add(cell * SUBMODE_NUMBER_CELL_W),
            y,
            pen,
        });
    }
}

/// Resolve the coin counter's draws to positioned lines: the entry panel
/// ([`SubmodeScreen::coin_panel`], record `10`) and whatever the installed
/// hub painters drew this frame (the confirm's three-line panel, record `11`,
/// with its cursor). `read` returns a field-overlay rodata string by VA (the
/// user's disc), or empty.
///
/// Empty unless the open screen is the coin counter (handler slot `0x25`):
/// the other hub screens' painters are not drawn by either host yet.
pub fn coin_counter_lines(
    screen: &SubmodeScreen,
    read: impl Fn(u32) -> Vec<u8>,
) -> Vec<SubmodeLine> {
    use legaia_engine_minigames::slot_machine::CoinPanelDraw;
    if !screen.is_open() || screen.actor.state != slot::COIN_COUNTER {
        return Vec::new();
    }
    let mut out = Vec::new();
    for d in &screen.coin_panel {
        match *d {
            CoinPanelDraw::Label { va, x, y, pen } => out.push(SubmodeLine {
                text: read(va),
                x,
                y,
                pen,
            }),
            CoinPanelDraw::Number {
                value,
                digits,
                x,
                y,
                pen,
            } => number_lines(value, digits, x, y, pen, &mut out),
            CoinPanelDraw::Cell { x, y, .. } => out.push(SubmodeLine {
                text: SUBMODE_CARET_STAND_IN.to_vec(),
                x,
                y,
                pen: SUBMODE_STAND_IN_PEN,
            }),
        }
    }
    // The hub painters stage the pen through their text draws; a number
    // draws in whatever the last one staged.
    let mut pen = hub::PALETTE_PANEL as u8;
    for d in screen.draws() {
        match *d {
            HubDraw::Text {
                text: hub::HubString::Literal(va),
                x,
                y,
                palette,
            }
            | HubDraw::ShortText {
                text: hub::HubString::Literal(va),
                x,
                y,
                palette,
            } => {
                pen = palette.clamp(0, 0xFF) as u8;
                out.push(SubmodeLine {
                    text: read(va),
                    x,
                    y,
                    pen,
                });
            }
            HubDraw::HeaderText {
                text: hub::HubString::Literal(va),
                x,
                y,
            } => out.push(SubmodeLine {
                text: read(va),
                x,
                y,
                pen,
            }),
            HubDraw::Number {
                value,
                digits,
                x,
                y,
            } => number_lines(value, digits.clamp(0, 0xFF) as u8, x, y, pen, &mut out),
            HubDraw::Sprite { x, y, .. } => out.push(SubmodeLine {
                text: SUBMODE_CURSOR_STAND_IN.to_vec(),
                x,
                y,
                pen: SUBMODE_STAND_IN_PEN,
            }),
            _ => {}
        }
    }
    out
}
