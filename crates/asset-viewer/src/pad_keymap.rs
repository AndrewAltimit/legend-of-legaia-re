//! Keyboard to PSX pad mapping and the pad buttons' display labels.

use legaia_engine_system::input::PadButton;
use winit::keyboard::KeyCode;

/// Map winit physical keys to PSX pad button bits. Keyboard mapping mirrors
/// the conventional emulator default:
///
/// - Arrows → D-pad
/// - Z → Cross, X → Square, A → Triangle, S → Circle
/// - Enter → Start, Right Shift → Select
/// - Q / W → L1 / R1, 1 / 2 → L2 / R2
pub(crate) fn keymap_pad(code: KeyCode) -> Option<PadButton> {
    Some(match code {
        KeyCode::ArrowUp => PadButton::Up,
        KeyCode::ArrowDown => PadButton::Down,
        KeyCode::ArrowLeft => PadButton::Left,
        KeyCode::ArrowRight => PadButton::Right,
        KeyCode::KeyZ => PadButton::Cross,
        KeyCode::KeyX => PadButton::Square,
        KeyCode::KeyA => PadButton::Triangle,
        KeyCode::KeyS => PadButton::Circle,
        KeyCode::Enter => PadButton::Start,
        KeyCode::ShiftRight => PadButton::Select,
        KeyCode::KeyQ => PadButton::L1,
        KeyCode::KeyW => PadButton::R1,
        KeyCode::Digit1 => PadButton::L2,
        KeyCode::Digit2 => PadButton::R2,
        _ => return None,
    })
}

/// Friendly button-name string for HUD readouts. Returns `"_"` for
/// unset bits so the readout has a fixed grid shape.
pub(crate) fn pad_button_label(b: PadButton) -> &'static str {
    match b {
        PadButton::Up => "U",
        PadButton::Down => "D",
        PadButton::Left => "L",
        PadButton::Right => "R",
        PadButton::Cross => "X",
        PadButton::Circle => "O",
        PadButton::Square => "[]",
        PadButton::Triangle => "/\\",
        PadButton::Start => "ST",
        PadButton::Select => "SE",
        PadButton::L1 => "L1",
        PadButton::R1 => "R1",
        PadButton::L2 => "L2",
        PadButton::R2 => "R2",
        PadButton::L3 => "L3",
        PadButton::R3 => "R3",
    }
}
