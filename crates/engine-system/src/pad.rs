//! The two PSX pad-word layouts every engine kernel reads: the **raw**
//! libpad word ([`PadButton`]) and the **packed** word the retail pump
//! publishes ([`PACK_UP`] and siblings, [`retail_packed`]).
//!
//! They live here, below `engine-core`, so the minigame rules engines can
//! take pad words without depending on the simulation crate. `engine-core`
//! re-exports both halves at their original paths (`input::PadButton`,
//! `dev_menu::PACK_*` / `dev_menu::retail_packed`).

/// Bit positions for the 16 pad buttons. Values match the PSX hardware
/// layout (0x0001 = Select … 0x8000 = Square) so engine-side code can
/// either use these typed constants or pack/unpack the raw word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum PadButton {
    Select = 0x0001,
    L3 = 0x0002,
    R3 = 0x0004,
    Start = 0x0008,
    Up = 0x0010,
    Right = 0x0020,
    Down = 0x0040,
    Left = 0x0080,
    L2 = 0x0100,
    R2 = 0x0200,
    L1 = 0x0400,
    R1 = 0x0800,
    Triangle = 0x1000,
    Circle = 0x2000,
    Cross = 0x4000,
    Square = 0x8000,
}

impl PadButton {
    /// Numeric mask, identical to `self as u16`. Convenience for code that
    /// works in raw u16 land.
    pub fn mask(self) -> u16 {
        self as u16
    }

    /// Human-readable name used in TOML config files and CLI output.
    pub fn name(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::L3 => "L3",
            Self::R3 => "R3",
            Self::Start => "Start",
            Self::Up => "Up",
            Self::Right => "Right",
            Self::Down => "Down",
            Self::Left => "Left",
            Self::L2 => "L2",
            Self::R2 => "R2",
            Self::L1 => "L1",
            Self::R1 => "R1",
            Self::Triangle => "Triangle",
            Self::Circle => "Circle",
            Self::Cross => "Cross",
            Self::Square => "Square",
        }
    }

    /// Parse a button from its [`Self::name`] string. Returns `None` for
    /// unknown names. Case-sensitive.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Select" => Some(Self::Select),
            "L3" => Some(Self::L3),
            "R3" => Some(Self::R3),
            "Start" => Some(Self::Start),
            "Up" => Some(Self::Up),
            "Right" => Some(Self::Right),
            "Down" => Some(Self::Down),
            "Left" => Some(Self::Left),
            "L2" => Some(Self::L2),
            "R2" => Some(Self::R2),
            "L1" => Some(Self::L1),
            "R1" => Some(Self::R1),
            "Triangle" => Some(Self::Triangle),
            "Circle" => Some(Self::Circle),
            "Cross" => Some(Self::Cross),
            "Square" => Some(Self::Square),
            _ => None,
        }
    }
}

/// Packed-pad Triangle (`_DAT_8007b850 & 0x10` = the coarse-step modifier).
pub const PACK_TRIANGLE: u16 = 0x0010;
/// Packed-pad Circle.
pub const PACK_CIRCLE: u16 = 0x0020;
/// Packed-pad Cross.
pub const PACK_CROSS: u16 = 0x0040;
/// Packed-pad Square.
pub const PACK_SQUARE: u16 = 0x0080;
/// Packed-pad Up.
pub const PACK_UP: u16 = 0x1000;
/// Packed-pad Right.
pub const PACK_RIGHT: u16 = 0x2000;
/// Packed-pad Down.
pub const PACK_DOWN: u16 = 0x4000;
/// Packed-pad Left.
pub const PACK_LEFT: u16 = 0x8000;

/// Convert a **raw** PSX pad word into the packed layout the retail pump
/// publishes and every [`PACK_*`](PACK_UP) constant names.
///
/// The two words hold the same 16 buttons with the byte halves swapped:
/// `FUN_8001822C` builds `~((b2 << 8) | b3)`, which puts the face/shoulder
/// libpad byte in bits 0-7 and the dpad/system byte in bits 8-15, while
/// [`PadButton`] keeps them the other way round. Two fixed
/// points documented on both sides pin the direction: Cross is `0x4000` raw
/// and `0x40` packed, Start is `0x0008` raw and `0x0800` packed.
///
/// Every host that drives `engine-core`'s `dev_menu_host::DevMenuSession` must run
/// its pad words through this before handing them over, because
/// `World::set_pad` forwards the raw argument straight into the retail pump,
/// so `InputState::retail_pad()` republishes the raw word under packed field
/// names for hosts that do not decode real libpad reports. Feeding the
/// session unconverted words cross-wires the whole dev menu - Up arrives as
/// `PACK_TRIANGLE`, Cross as `PACK_DOWN` - which is what the native window's
/// Records-page toggle first ran into. Shared here so the two hosts cannot
/// each answer the byte-swap question differently.
// REF: FUN_8001822C
pub fn retail_packed(raw: u16) -> u16 {
    raw.swap_bytes()
}
