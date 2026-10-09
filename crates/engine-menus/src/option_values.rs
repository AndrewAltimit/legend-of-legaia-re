//! The options screen's value enums: one per config word the screen
//! edits (sound, battle camera, Select Attack, battle command entry, field
//! movement default, field HP display).
//!
//! Plain data the battle input and the field HUD key on as well as the
//! options session, so they sit below the session itself, which lives in
//! engine-core's `options` beside the `World` it pushes its knobs onto and
//! re-exports every enum here at its old path.

use serde::{Deserialize, Serialize};

/// Sound output mode (retail row "Sound", config word `0x800846BC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AudioMode {
    #[default]
    Stereo,
    Mono,
}

impl AudioMode {
    /// Retail value string ("Stereo" / "Monaural").
    pub fn label(self) -> &'static str {
        match self {
            Self::Stereo => "Stereo",
            Self::Mono => "Monaural",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Stereo => Self::Mono,
            Self::Mono => Self::Stereo,
        }
    }
}

/// Battle camera distance (config word `0x800846C0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BattleCameraOpt {
    #[default]
    Close,
    Normal,
    Far,
}

impl BattleCameraOpt {
    /// The option from its config word `0x800846C0` (`0` Close, `2` Far;
    /// the camera's arms test `!= 0` and `== 2`, so any other word reads
    /// as Normal).
    pub fn from_word(word: u8) -> Self {
        match word {
            0 => Self::Close,
            2 => Self::Far,
            _ => Self::Normal,
        }
    }
}

/// Battle attack-target picking (config word `0x800846C4`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SelectAttackOpt {
    #[default]
    Select,
    Automatic,
    Command,
}

/// Battle command entry style (config word `0x800846C8`). Retail's second
/// value string is a cross-button glyph (`0xCE` glyph escape) + " button".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BattleCommandOpt {
    #[default]
    DirectionalButtons,
    CrossButton,
}

/// Field movement default (config word `0x800846CC`).
///
/// This is the *default*, not the state: the run button inverts it, so with
/// `Run` selected the button walks (the XOR at `0x801D0370` / `0x801D0398`;
/// `World::field_run_active`). The
/// button itself is the separate config word `0x800846DC` = `0x48` =
/// Cross | R1, which retail seeds once and never exposes as an option row -
/// the port mirrors that pair as its default run mask
/// (`FIELD_RUN_BUTTON_MASK_DEFAULT`)
/// and rebinds it at the key level instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FieldMoveOpt {
    #[default]
    Walk,
    Run,
}

/// The options screen's **Field HP Display** row (config word `0x800845C4`):
/// how long the field party HUD waits after the player stops before it
/// appears (`FUN_801D0D38`), or that it never does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HpDisplayOpt {
    #[default]
    Immediate,
    Gradual,
    DisplayOff,
}
