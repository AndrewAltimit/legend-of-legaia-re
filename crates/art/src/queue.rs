//! Action queue, character ID, command directions, and the [`ActionConstant`]
//! enum.
//!
//! The action queue is the canonical sequence of values the battle action
//! state machine consumes per actor turn. Most action constants are either
//! menu choices (Item / Magic / Attack / Spirit / Escape), per-direction
//! input markers (Left / Right / Down / Up), starter sentinels (`0x19` /
//! `0x1A`) that introduce an art, or per-character art identifiers
//! (`0x1B`–`0x32`).

use serde::{Deserialize, Serialize};

/// Party member whose art tables / records are being addressed.
///
/// Only the three player-controllable Tactical Arts users have decoded
/// tables. Other party members (Songi, Noa-as-Kanon, etc.) share encodings
/// in some places but require additional capture work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Character {
    Vahn,
    Noa,
    Gala,
}

impl Character {
    pub fn name(self) -> &'static str {
        match self {
            Character::Vahn => "Vahn",
            Character::Noa => "Noa",
            Character::Gala => "Gala",
        }
    }

    pub fn all() -> [Character; 3] {
        [Character::Vahn, Character::Noa, Character::Gala]
    }
}

/// Directional input commands consumed by the player when chaining an art.
///
/// Encoded as `1`/`2`/`3`/`4` in retail RAM (`0` is the terminator). The
/// runtime translates these into action constants `0x0C–0x0F`
/// (Left/Right/Down/Up) when building the action queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    Left = 1,
    Right = 2,
    Down = 3,
    Up = 4,
}

impl Command {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(Command::Left),
            2 => Some(Command::Right),
            3 => Some(Command::Down),
            4 => Some(Command::Up),
            _ => None,
        }
    }

    pub fn as_byte(self) -> u8 {
        self as u8
    }

    /// Map a command to the directional [`ActionConstant`] the runtime
    /// emits while the player is buffering inputs.
    pub fn as_action(self) -> ActionConstant {
        match self {
            Command::Left => ActionConstant::Left,
            Command::Right => ActionConstant::Right,
            Command::Down => ActionConstant::Down,
            Command::Up => ActionConstant::Up,
        }
    }
}

/// Battle action queue values, full retail range `0x00–0x32`.
///
/// Values `0x1B–0x32` are character-dependent: the *constant* is the same
/// across Vahn / Noa / Gala but the art it names differs per character.
/// Use [`crate::tables::art_name`] for the canonical name.
///
/// Empty slots (`0x11–0x18`) are reserved for runtime placeholders the
/// game uses while building the queue and never appear in static data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum ActionConstant {
    Nothing = 0x00,
    Item = 0x01,
    Magic = 0x02,
    Attack = 0x03,
    Spirit = 0x04,
    Escape = 0x05,
    Unknown06 = 0x06,
    FaintAnim1 = 0x07,
    FaintAnim2 = 0x08,
    Unknown09 = 0x09,
    ItemMagicAnim = 0x0A,
    BlockAnim = 0x0B,
    Left = 0x0C,
    Right = 0x0D,
    Down = 0x0E,
    Up = 0x0F,
    SpiritAnim = 0x10,
    Empty1 = 0x11,
    Empty2 = 0x12,
    Empty3 = 0x13,
    Empty4 = 0x14,
    Empty5 = 0x15,
    Empty6 = 0x16,
    Empty7 = 0x17,
    Empty8 = 0x18,
    /// Regular Art Starter: precedes a per-character art constant in the
    /// queue. Pairs with the art's stored AP cost.
    RegularStarter = 0x19,
    /// Special Art Starter: introduces a Super or Miracle Art finisher.
    /// The Miracle/Super expansion logic appends one of these before the
    /// finisher action constant.
    SpecialStarter = 0x1A,
    /// Per-character arts. The constant is shared across characters but
    /// resolves to a different name (and ArtRecord) per character. Use
    /// [`crate::tables::art_name`].
    Art1B = 0x1B,
    Art1C = 0x1C,
    Art1D = 0x1D,
    Art1E = 0x1E,
    Art1F = 0x1F,
    Art20 = 0x20,
    Art21 = 0x21,
    Art22 = 0x22,
    Art23 = 0x23,
    Art24 = 0x24,
    Art25 = 0x25,
    Art26 = 0x26,
    Art27 = 0x27,
    Art28 = 0x28,
    Art29 = 0x29,
    Art2A = 0x2A,
    Art2B = 0x2B,
    Art2C = 0x2C,
    Art2D = 0x2D,
    Art2E = 0x2E,
    Art2F = 0x2F,
    Art30 = 0x30,
    Art31 = 0x31,
    Art32 = 0x32,
}

impl ActionConstant {
    /// Decode a raw queue byte. Returns `None` for values outside `0x00–0x32`.
    ///
    /// Note: Miracle Arts encode the first 4 replacement-string actions with
    /// the high nibble's MSB set (`0x8C`/`0x8D`/`0x8E`/`0x8F`). Mask with
    /// `& 0x7F` before calling this if you're decoding the raw replacement
    /// string. See [`crate::miracle`].
    pub fn from_byte(b: u8) -> Option<Self> {
        if b > 0x32 {
            return None;
        }
        // SAFETY: all values 0x00..=0x32 are listed in the enum.
        // Use a match here so the compiler validates exhaustiveness on
        // every change.
        Some(match b {
            0x00 => ActionConstant::Nothing,
            0x01 => ActionConstant::Item,
            0x02 => ActionConstant::Magic,
            0x03 => ActionConstant::Attack,
            0x04 => ActionConstant::Spirit,
            0x05 => ActionConstant::Escape,
            0x06 => ActionConstant::Unknown06,
            0x07 => ActionConstant::FaintAnim1,
            0x08 => ActionConstant::FaintAnim2,
            0x09 => ActionConstant::Unknown09,
            0x0A => ActionConstant::ItemMagicAnim,
            0x0B => ActionConstant::BlockAnim,
            0x0C => ActionConstant::Left,
            0x0D => ActionConstant::Right,
            0x0E => ActionConstant::Down,
            0x0F => ActionConstant::Up,
            0x10 => ActionConstant::SpiritAnim,
            0x11 => ActionConstant::Empty1,
            0x12 => ActionConstant::Empty2,
            0x13 => ActionConstant::Empty3,
            0x14 => ActionConstant::Empty4,
            0x15 => ActionConstant::Empty5,
            0x16 => ActionConstant::Empty6,
            0x17 => ActionConstant::Empty7,
            0x18 => ActionConstant::Empty8,
            0x19 => ActionConstant::RegularStarter,
            0x1A => ActionConstant::SpecialStarter,
            0x1B => ActionConstant::Art1B,
            0x1C => ActionConstant::Art1C,
            0x1D => ActionConstant::Art1D,
            0x1E => ActionConstant::Art1E,
            0x1F => ActionConstant::Art1F,
            0x20 => ActionConstant::Art20,
            0x21 => ActionConstant::Art21,
            0x22 => ActionConstant::Art22,
            0x23 => ActionConstant::Art23,
            0x24 => ActionConstant::Art24,
            0x25 => ActionConstant::Art25,
            0x26 => ActionConstant::Art26,
            0x27 => ActionConstant::Art27,
            0x28 => ActionConstant::Art28,
            0x29 => ActionConstant::Art29,
            0x2A => ActionConstant::Art2A,
            0x2B => ActionConstant::Art2B,
            0x2C => ActionConstant::Art2C,
            0x2D => ActionConstant::Art2D,
            0x2E => ActionConstant::Art2E,
            0x2F => ActionConstant::Art2F,
            0x30 => ActionConstant::Art30,
            0x31 => ActionConstant::Art31,
            0x32 => ActionConstant::Art32,
            _ => return None,
        })
    }

    pub fn as_byte(self) -> u8 {
        self as u8
    }

    pub fn is_directional(self) -> bool {
        matches!(
            self,
            ActionConstant::Left
                | ActionConstant::Right
                | ActionConstant::Down
                | ActionConstant::Up
        )
    }

    pub fn is_starter(self) -> bool {
        matches!(
            self,
            ActionConstant::RegularStarter | ActionConstant::SpecialStarter
        )
    }

    /// True for `0x1B–0x32` art constants.
    pub fn is_art(self) -> bool {
        let b = self.as_byte();
        (0x1B..=0x32).contains(&b)
    }

    /// Convenience: list all valid bytes in canonical order.
    pub fn all() -> impl Iterator<Item = ActionConstant> {
        (0x00u8..=0x32u8).filter_map(ActionConstant::from_byte)
    }
}

/// Mutable action queue. Battle action SM owns one of these per actor turn.
///
/// Push directional commands or art constants in the order the player input
/// them; Miracle / Super Art expansion runs in-place via
/// [`MiracleMatcher`](crate::miracle::MiracleMatcher) and
/// [`SuperMatcher`](crate::super_art::SuperMatcher).
#[derive(Debug, Clone, Default)]
pub struct ActionQueue {
    actions: Vec<ActionConstant>,
}

impl ActionQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let mut q = Self::new();
        for &b in bytes {
            q.push(ActionConstant::from_byte(b)?);
        }
        Some(q)
    }

    pub fn push(&mut self, action: ActionConstant) {
        self.actions.push(action);
    }

    pub fn extend<I: IntoIterator<Item = ActionConstant>>(&mut self, iter: I) {
        self.actions.extend(iter);
    }

    pub fn clear(&mut self) {
        self.actions.clear();
    }

    pub fn replace_all(&mut self, iter: impl IntoIterator<Item = ActionConstant>) {
        self.actions.clear();
        self.actions.extend(iter);
    }

    pub fn actions(&self) -> &[ActionConstant] {
        &self.actions
    }

    pub fn len(&self) -> usize {
        self.actions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// Truncate to `n` actions. Used by Super Art trigger logic to drop
    /// the matched prefix before appending the replacement tail.
    pub fn truncate(&mut self, n: usize) {
        self.actions.truncate(n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_constant_round_trip() {
        for b in 0x00u8..=0x32 {
            let a = ActionConstant::from_byte(b).unwrap();
            assert_eq!(a.as_byte(), b);
        }
    }

    #[test]
    fn action_constant_rejects_out_of_range() {
        assert!(ActionConstant::from_byte(0x33).is_none());
        assert!(ActionConstant::from_byte(0xFF).is_none());
    }

    #[test]
    fn art_constants_classify_correctly() {
        assert!(ActionConstant::Art1B.is_art());
        assert!(ActionConstant::Art32.is_art());
        assert!(!ActionConstant::RegularStarter.is_art());
        assert!(!ActionConstant::Left.is_art());

        assert!(ActionConstant::RegularStarter.is_starter());
        assert!(ActionConstant::SpecialStarter.is_starter());
        assert!(!ActionConstant::Art1B.is_starter());

        assert!(ActionConstant::Left.is_directional());
        assert!(ActionConstant::Up.is_directional());
        assert!(!ActionConstant::Attack.is_directional());
    }

    #[test]
    fn command_to_action_mapping() {
        assert_eq!(Command::Left.as_action(), ActionConstant::Left);
        assert_eq!(Command::Right.as_action(), ActionConstant::Right);
        assert_eq!(Command::Down.as_action(), ActionConstant::Down);
        assert_eq!(Command::Up.as_action(), ActionConstant::Up);
    }

    #[test]
    fn action_queue_basic_ops() {
        let mut q = ActionQueue::new();
        q.push(ActionConstant::Attack);
        q.push(ActionConstant::RegularStarter);
        q.push(ActionConstant::Art1B);
        assert_eq!(q.len(), 3);
        assert_eq!(q.actions()[0], ActionConstant::Attack);
        q.truncate(1);
        assert_eq!(q.len(), 1);
        q.clear();
        assert!(q.is_empty());
    }
}
