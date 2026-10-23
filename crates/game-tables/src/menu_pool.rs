//! The menu overlay's leading string pool (PROT 0899, `0x801CE81C..0x801CEC78`)
//! as named labels: the pause menu's root rows and tab titles, the Items /
//! Equip / Status / Magic window labels, the shop picker and the Options
//! screen's rows and values.
//!
//! Coordinates only. Each label is the VA the menu overlay's renderers load
//! into the string primitive (`lui a0,0x801d` + `addiu a0,a0,-0x....`), and
//! the bytes are read from the image the user supplied - no text is
//! committed here. A host keeps a fallback only for a load without the
//! overlay. A label a language pack moved is read where the overlay's code
//! now finds it ([`crate::menu_string_refs`]).
//!
//! **The `@` lead is kept.** Most pool strings open with `0x40`, which
//! neither the expander `FUN_80036514` nor the renderer `FUN_80036888` tests
//! for: it is a blank glyph of width `0`, so retail draws it as a one-pixel
//! advance and every such label's first visible glyph sits one pixel past its
//! pen. The Options strings and the stat / Ra-Seru cells carry no `@`.

use crate::menu_windows::MENU_OVERLAY_BASE_VA;

/// One named pool label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuLabel {
    /// The root list's rows and the five sub-screen tab titles
    /// (`FUN_801CFD68` rows, `FUN_801DCA0C`..`FUN_801DCB1C` tabs, which load
    /// the same VAs).
    Items,
    Magic,
    Equip,
    Status,
    Options,
    Load,
    Save,
    /// The Items command window (`FUN_801D0D18`).
    Use,
    ThrowOut,
    Arrange,
    /// The Point Card's info line.
    PointsLeft,
    /// The Equip browse row 0.
    BestEquipment,
    /// The Magic info window's cost label (`FUN_801D2E74`).
    MpUsed,
    /// The Status pager / summary labels.
    Condition,
    Skills,
    Moves,
    Atr,
    NoSkills,
    NoMagic,
    ApUsed,
    Command,
    NoMoves,
    /// The shop picker and notes.
    Buy,
    Sell,
    Quit,
    Equipped,
    CannotEquip,
    Price,
    CannotSell,
    Exchange,
    /// The Options screen's rows and values (no `@`).
    DisplayOff,
    Gradual,
    Immediate,
    FieldHpDisplay,
    Encounters,
    Battles,
    VibrationOff,
    VibrationOn,
    Events,
    DualShock,
    VoicesOff,
    VoicesOn,
    BattleVoices,
    Run,
    Walk,
    FieldMove,
    DirectionalButtons,
    BattleCommand,
    CommandMode,
    Automatic,
    SelectMode,
    BattleSelectAttack,
    Far,
    Normal,
    Close,
    BattleCamera,
    Monaural,
    Stereo,
    Sound,
    /// The status page's stat cells (bare 4-byte cells, no `@`).
    Atk,
    Udf,
    Ldf,
    Spd,
    Int,
    Agl,
    Experience,
    NextLevel,
}

impl MenuLabel {
    /// Every label, in pool order.
    pub const ALL: [MenuLabel; 67] = [
        Self::DisplayOff,
        Self::Gradual,
        Self::Immediate,
        Self::FieldHpDisplay,
        Self::Encounters,
        Self::Battles,
        Self::VibrationOff,
        Self::VibrationOn,
        Self::Events,
        Self::DualShock,
        Self::VoicesOff,
        Self::VoicesOn,
        Self::BattleVoices,
        Self::Run,
        Self::Walk,
        Self::FieldMove,
        Self::DirectionalButtons,
        Self::BattleCommand,
        Self::CommandMode,
        Self::Automatic,
        Self::SelectMode,
        Self::BattleSelectAttack,
        Self::Far,
        Self::Normal,
        Self::Close,
        Self::BattleCamera,
        Self::Monaural,
        Self::Stereo,
        Self::Sound,
        Self::Atk,
        Self::Udf,
        Self::Ldf,
        Self::Spd,
        Self::Int,
        Self::Agl,
        Self::Experience,
        Self::NextLevel,
        Self::Items,
        Self::Magic,
        Self::Equip,
        Self::Status,
        Self::Options,
        Self::Load,
        Self::Save,
        Self::Use,
        Self::ThrowOut,
        Self::Arrange,
        Self::PointsLeft,
        Self::BestEquipment,
        Self::MpUsed,
        Self::Condition,
        Self::Skills,
        Self::Moves,
        Self::Atr,
        Self::NoSkills,
        Self::NoMagic,
        Self::ApUsed,
        Self::Command,
        Self::NoMoves,
        Self::Buy,
        Self::Sell,
        Self::Quit,
        Self::Equipped,
        Self::CannotEquip,
        Self::Price,
        Self::CannotSell,
        Self::Exchange,
    ];

    /// The label's VA in the as-loaded menu overlay.
    pub fn va(self) -> u32 {
        match self {
            Self::DisplayOff => 0x801C_E81C,
            Self::Gradual => 0x801C_E828,
            Self::Immediate => 0x801C_E830,
            Self::FieldHpDisplay => 0x801C_E83C,
            Self::Encounters => 0x801C_E850,
            Self::Battles => 0x801C_E860,
            Self::VibrationOff => 0x801C_E86C,
            Self::VibrationOn => 0x801C_E87C,
            Self::Events => 0x801C_E88C,
            Self::DualShock => 0x801C_E898,
            Self::VoicesOff => 0x801C_E8A4,
            Self::VoicesOn => 0x801C_E8B0,
            Self::BattleVoices => 0x801C_E8BC,
            Self::Run => 0x801C_E8CC,
            Self::Walk => 0x801C_E8D0,
            Self::FieldMove => 0x801C_E8D8,
            Self::DirectionalButtons => 0x801C_E8F0,
            Self::BattleCommand => 0x801C_E904,
            Self::CommandMode => 0x801C_E914,
            Self::Automatic => 0x801C_E91C,
            Self::SelectMode => 0x801C_E928,
            Self::BattleSelectAttack => 0x801C_E930,
            Self::Far => 0x801C_E948,
            Self::Normal => 0x801C_E94C,
            Self::Close => 0x801C_E954,
            Self::BattleCamera => 0x801C_E95C,
            Self::Monaural => 0x801C_E96C,
            Self::Stereo => 0x801C_E978,
            Self::Sound => 0x801C_E980,
            Self::Atk => 0x801C_E9A0,
            Self::Udf => 0x801C_E9A4,
            Self::Ldf => 0x801C_E9A8,
            Self::Spd => 0x801C_E9AC,
            Self::Int => 0x801C_E9B0,
            Self::Agl => 0x801C_E9B4,
            Self::Experience => 0x801C_E9B8,
            Self::NextLevel => 0x801C_E9C4,
            Self::Items => 0x801C_E9D0,
            Self::Magic => 0x801C_E9D8,
            Self::Equip => 0x801C_E9E0,
            Self::Status => 0x801C_E9E8,
            Self::Options => 0x801C_E9F4,
            Self::Load => 0x801C_EA00,
            Self::Save => 0x801C_EA08,
            Self::Use => 0x801C_EA10,
            Self::ThrowOut => 0x801C_EA18,
            Self::Arrange => 0x801C_EA24,
            Self::PointsLeft => 0x801C_EA30,
            Self::BestEquipment => 0x801C_EAD8,
            Self::MpUsed => 0x801C_EAEC,
            Self::Condition => 0x801C_EAF8,
            Self::Skills => 0x801C_EB04,
            Self::Moves => 0x801C_EB10,
            Self::Atr => 0x801C_EB18,
            Self::NoSkills => 0x801C_EB20,
            Self::NoMagic => 0x801C_EB40,
            Self::ApUsed => 0x801C_EB58,
            Self::Command => 0x801C_EB64,
            Self::NoMoves => 0x801C_EB70,
            Self::Buy => 0x801C_EB94,
            Self::Sell => 0x801C_EB9C,
            Self::Quit => 0x801C_EBA4,
            Self::Equipped => 0x801C_EBAC,
            Self::CannotEquip => 0x801C_EBB8,
            Self::Price => 0x801C_EC54,
            Self::CannotSell => 0x801C_EC5C,
            Self::Exchange => 0x801C_EC6C,
        }
    }

    /// The root list's seven rows, top to bottom.
    pub const ROOT_ROWS: [MenuLabel; 7] = [
        Self::Items,
        Self::Magic,
        Self::Equip,
        Self::Status,
        Self::Options,
        Self::Load,
        Self::Save,
    ];
}

/// Every [`MenuLabel`] read off one as-loaded PROT 0899 image.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuPoolLabels {
    labels: std::collections::HashMap<MenuLabel, String>,
}

impl MenuPoolLabels {
    /// Read every label. A label whose VA lies outside the image, or whose
    /// bytes are not printable text, is left out ([`Self::get`] then answers
    /// `None` and the host falls back).
    pub fn from_menu_overlay(overlay: &[u8]) -> Self {
        let mut labels = std::collections::HashMap::new();
        for label in MenuLabel::ALL {
            if let Some(s) = pool_string(overlay, label.va()) {
                labels.insert(label, s);
            }
        }
        Self { labels }
    }

    /// The label's text, `@` lead included, or `None` without the overlay.
    pub fn get(&self, label: MenuLabel) -> Option<&str> {
        self.labels.get(&label).map(String::as_str)
    }

    /// `true` once real disc text was read.
    pub fn is_installed(&self) -> bool {
        !self.labels.is_empty()
    }
}

/// One NUL-terminated pool string at `va`, kept whole (`@` included), read
/// where the overlay's own code finds it
/// ([`crate::menu_string_refs::resolve_va`]: a translated label may have
/// moved). `None` outside the image, or when the string is empty, a lone
/// `@`, or carries a byte that is not a glyph (a substitution token belongs
/// to a different reader).
///
/// The text is the font's byte space as Latin-1: a byte `0x80..` (an accent
/// cell a language pack's accent font draws, or a PAL glyph) comes back as
/// the `char` of the same value, which is how the hosts hand bytes back to
/// the font.
fn pool_string(overlay: &[u8], va: u32) -> Option<String> {
    let va = crate::menu_string_refs::resolve_va(overlay, va);
    let off = va.checked_sub(MENU_OVERLAY_BASE_VA)? as usize;
    let rest = overlay.get(off..)?;
    let end = rest.iter().position(|&b| b == 0)?;
    let text = &rest[..end];
    if text.strip_prefix(b"@").unwrap_or(text).is_empty() || !text.iter().all(|&b| is_glyph(b)) {
        return None;
    }
    Some(text.iter().map(|&b| char::from(b)).collect())
}

/// A byte the string primitive draws as one glyph: printable ASCII or a
/// high cell, never a two-byte control token's lead (`0xC0..=0xCF`, `0xFF`).
pub fn is_glyph(b: u8) -> bool {
    (0x20..0x7F).contains(&b) || (0x80..0xC0).contains(&b) || (0xD0..0xFF).contains(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_read_at_their_vas_and_keep_the_lead() {
        let mut img = vec![0u8; 0x500];
        let put = |img: &mut Vec<u8>, va: u32, b: &[u8]| {
            let o = (va - MENU_OVERLAY_BASE_VA) as usize;
            img[o..o + b.len()].copy_from_slice(b);
        };
        put(&mut img, MenuLabel::Items.va(), b"@Bag\0");
        put(&mut img, MenuLabel::Far.va(), b"Far\0");
        put(&mut img, MenuLabel::Use.va(), b"@\0");
        put(&mut img, MenuLabel::Magic.va(), b"@Mag\x82\0");
        put(&mut img, MenuLabel::Equip.va(), b"@\xC2\x01\0");
        let l = MenuPoolLabels::from_menu_overlay(&img);
        assert!(l.is_installed());
        assert_eq!(l.get(MenuLabel::Items), Some("@Bag"));
        assert_eq!(l.get(MenuLabel::Far), Some("Far"));
        assert_eq!(l.get(MenuLabel::Use), None, "a lone @ is no label");
        // An accent cell survives as the Latin-1 char of its byte; a token
        // is not a label.
        assert_eq!(l.get(MenuLabel::Magic), Some("@Mag\u{82}"));
        assert_eq!(l.get(MenuLabel::Equip), None);
        assert_eq!(MenuPoolLabels::default().get(MenuLabel::Items), None);
    }

    #[test]
    fn every_label_has_a_distinct_va_in_the_pool() {
        let mut vas: Vec<u32> = MenuLabel::ALL.iter().map(|l| l.va()).collect();
        assert!(vas.iter().all(|&v| (0x801C_E81C..0x801C_EC78).contains(&v)));
        vas.sort_unstable();
        vas.dedup();
        assert_eq!(vas.len(), 67);
    }
}
