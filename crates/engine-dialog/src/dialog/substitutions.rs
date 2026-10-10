//! Text substitutions a panel resolves at decode time: the substitute-kind key and the script counter escapes.

use super::*;

/// Host-installed name-substitution table for a dialog panel: `(kind key,
/// arg)` -> resolved glyph bytes. See [`OwnedDialogPanel::substitutions`].
pub type PanelSubstitutions = Arc<std::collections::HashMap<(u8, u8), Vec<u8>>>;

/// Stable map key for a [`legaia_mes::SubstituteKind`] (the `0xC1..0xC7`
/// escape families), used by [`OwnedDialogPanel::substitutions`].
pub fn substitute_kind_key(kind: legaia_mes::SubstituteKind) -> u8 {
    use legaia_mes::SubstituteKind as K;
    match kind {
        K::CharacterName => 1,
        K::ItemName => 2,
        K::MagicName => 3,
        K::SpellName => 5,
        K::QuestName => 7,
    }
}

/// Map key for the `0xCE` **number** escapes in [`PanelSubstitutions`]:
/// `(SCRIPT_COUNTER_KEY, operand)` holds the digits a `0xCE 0x0B..=0x0E`
/// pair prints. Distinct from every [`substitute_kind_key`] value.
pub const SCRIPT_COUNTER_KEY: u8 = 0xCE;

/// The `0xCE` operands that print a script counter instead of a sprite:
/// escape-table rows `0x0B..=0x0E` carry `string_id == 0` and `y_offset`
/// `0..=3`, and the renderer reads that `y_offset` as the counter index
/// (`docs/formats/dialog-font.md`).
pub const SCRIPT_COUNTER_ESCAPES: std::ops::RangeInclusive<u8> = 0x0B..=0x0E;

/// The counter a number escape prints: operand `0x0B + i` reads slot `i` of
/// the field VM's script-counter table `0x801C6460`. `None` for any other
/// operand.
pub fn script_counter_slot(operand: u8) -> Option<usize> {
    SCRIPT_COUNTER_ESCAPES
        .contains(&operand)
        .then(|| usize::from(operand - *SCRIPT_COUNTER_ESCAPES.start()))
}

/// The glyph bytes a number escape prints for counter `value`.
///
/// The dialog renderer's number arm (`FUN_80036888`, `0x80036A54..0x80036A94`)
/// loads the counter with `lh` and calls `FUN_80034B78(value, 0, x, y)`. That
/// writer peels each decimal digit by repeated subtraction of the paired
/// `4 * 10^k` / `10^k` powers at `0x80073DCC`, suppresses leading zeros, and
/// always draws the units digit - so a value that never reaches the first
/// power, which is every value `<= 0`, prints a single `0`. A positive value
/// prints its plain decimal form (a signed halfword tops out at five digits).
///
/// REF: FUN_80036888, FUN_80034B78
pub fn script_counter_digits(value: i16) -> Vec<u8> {
    if value <= 0 {
        b"0".to_vec()
    } else {
        value.to_string().into_bytes()
    }
}
