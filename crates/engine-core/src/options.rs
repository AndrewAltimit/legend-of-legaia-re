//! Options / config screen - the retail pause-menu settings set.
//!
//! The retail options screen (menu overlay PROT 0899) is three traced
//! pieces:
//!
//! - **Row renderer** `FUN_801d2910` (called by the window-id-48 content
//!   renderer `FUN_801dcef0` with row span `0..=9`): walks a 10-entry
//!   display-layout table (`[u16 row_id, u16 advance]` pairs at
//!   `0x801E4404`) and a row-descriptor list (8-byte nodes at
//!   `0x801E44B8`: config-word pointer, value count, label ink, row id,
//!   label string index). The value string is `strings[label + value + 1]`
//!   in the shared pointer table at `0x801E442C`.
//! - **Input SM** `FUN_801da9f8`: browse cursor `DAT_801e46c0` skips
//!   valueless rows; Cross opens a **value popup** (window descriptor
//!   id 47, y/h stamped at runtime), Cross inside commits the choice
//!   straight into the config word, Circle backs out.
//! - **Popup renderer** `FUN_801d2b44`: lists every candidate value at a
//!   13-px pitch with its own cursor `DAT_801e46d0`.
//!
//! The config words live in the `0x800845xx/0x800846xx` block (saved with
//! the game): Battle Camera `0x800846C0`, Battle Select Attack
//! `0x800846C4`, Battle Command `0x800846C8`, Field Move `0x800846CC`,
//! Field HP Display `0x800845C4`, Sound `0x800846BC`, Dual Shock
//! vibration Battles `0x800845C8` / Events `0x800845A8` / Encounters
//! `0x800845CC`. A tenth descriptor node ("Battle Voices",
//! `0x800845AC`) exists in the list but is absent from the display-layout
//! table - a hidden row the US build doesn't show.
//!
//! ## PROT 0896 is not the options overlay this port follows
//!
//! An older reading placed the options screen in PROT 0896 (`bat_back_dat`),
//! whose 0x9000-byte head carries a Shift-JIS options string table
//! (movement / battle command / battle camera / monaural-stereo) and about
//! 50 functions, `FUN_801C6C78` among them. That head is a **Japanese-build**
//! options menu, and it is not what the USA build runs:
//!
//! - a signature scan finds PROT 0896 resident in **0 of 140** save states;
//! - its widely-cited base `0x801C5818` is an over-read artifact - the entry's
//!   footprint runs into its neighbour, so everything from file offset
//!   `0x9000` on is the *field* overlay's bytes, which is what fixes the
//!   whole-file base recovery to `0x801CE818 - 0x9000` by construction. PROT
//!   0896's own link base is `0x801D4DF0`, recovered over the corrected entry
//!   (see `crates/asset/data/static-overlays.toml`), and it is not a runtime
//!   address here: none of the image's SCUS-range calls reaches a function
//!   entry of this disc's executable;
//! - the retail rows this module implements all trace to the **menu overlay
//!   PROT 0899** functions listed above, which *is* RAM-verified.
//!
//! So `FUN_801C6C78` and its 0896 siblings are deliberately **not ported**:
//! they are unreachable in the retail USA build, and porting them would put
//! behaviour in the engine that retail never executes. Any dump named
//! `overlay_0896_*` whose printed VA is at or above `0x801CE818 - 0x9000`
//! should be re-resolved against the extracted images before being cited -
//! most such dumps are field-overlay bytes shifted by `+0x5818`
//! (`docs/tooling/dump-corpus-integrity.md`).
//!
//! [`OptionsState`] additionally keeps the engine-only knobs (BGM / SFX
//! volume, message speed) that retail has no UI for; they round-trip
//! through the options config file but are **not** part of the
//! pause-menu row set.
//!
//! ## States
//!
//! `Browsing { cursor } -> Editing { cursor, choice } (value popup) ->
//! Browsing -> Done` - commits happen at popup confirm (retail writes the
//! config word immediately; backing out of the screen never reverts).

use crate::input::{Mapping, PadButton};
use crate::key_rebind::{KeyRebindEvent, KeyRebindInput, KeyRebindSession};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

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
/// [`World::field_run_active`](crate::world::World::field_run_active)). The
/// button itself is the separate config word `0x800846DC` = `0x48` =
/// Cross | R1, which retail seeds once and never exposes as an option row -
/// the port mirrors that pair as its default run mask
/// ([`FIELD_RUN_BUTTON_MASK_DEFAULT`](crate::world::config::FIELD_RUN_BUTTON_MASK_DEFAULT))
/// and rebinds it at the key level instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FieldMoveOpt {
    #[default]
    Walk,
    Run,
}

/// Field HP-restore display style (config word `0x800845C4`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HpDisplayOpt {
    #[default]
    Immediate,
    Gradual,
    DisplayOff,
}

/// Full set of user-editable options. Engines round-trip via
/// [`OptionsState::load_or_default`] / [`OptionsState::save`] (TOML,
/// mirroring `input::Mapping`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OptionsState {
    // --- Retail rows (the pause-menu options screen) ---
    pub battle_camera: BattleCameraOpt,
    pub battle_select_attack: SelectAttackOpt,
    pub battle_command: BattleCommandOpt,
    pub field_move: FieldMoveOpt,
    pub field_hp_display: HpDisplayOpt,
    pub audio: AudioMode,
    /// Dual Shock "Battles" vibration (`true` = Vibration On).
    pub vibration_battles: bool,
    /// Dual Shock "Events" vibration. Retail also kills the live rumble
    /// motors when this commits to Off.
    pub vibration_events: bool,
    /// Dual Shock "Encounters" vibration.
    pub vibration_encounters: bool,
    // --- Engine-only knobs (config file only; retail shows no UI) ---
    /// 0..=10. Engines convert to their per-channel scalar.
    pub bgm_volume: u8,
    /// 0..=10. Engines convert to their per-channel scalar.
    pub sfx_volume: u8,
    /// 1..=8 (1 = slowest). Wired to dialog auto-advance interval.
    pub message_speed: u8,
    /// Master audio mute (`true` = silent). Engine-only: retail's options
    /// screen has Stereo/Monaural but no "off". Wired to the mixer's
    /// master gate (`AudioOut::set_muted`), which silences the output
    /// without pausing the sequencer / SPU, so unmuting stays in sync.
    pub muted: bool,
    /// Field follow-camera distance preset (engine-only framing knob;
    /// windowed hosts cycle it with a keybind). Defaults to
    /// [`CameraDistance::Far`] - the interactive default frames a bit more
    /// of the scene than retail. Pure render framing: never feeds the
    /// world simulation, so replays / oracles are unaffected (headless
    /// hosts don't read options and keep the engine-core `Retail` default).
    pub camera_distance: crate::camera::CameraDistance,
    /// Opt-in precise-movement toggle (engine-only, non-retail): mirrors
    /// into [`crate::world::FieldLocomotion::precise_movement`](crate::world::FieldLocomotion::precise_movement)
    /// by windowed hosts. Default off = retail's quantised 4/8-way remap.
    pub precise_movement: bool,
    /// Photosensitivity guard (engine-only, non-retail): slew-limits the
    /// luminance channels of the palette-space light animations so scenes
    /// like koin3's dance floor can't strobe at hazardous rates (retail
    /// itself full-swings bright<->black every game tick there - 15 Hz
    /// cycles, far past the 3-flashes-per-second guideline). Mirrors into
    /// [`crate::world::WorldToggles::reduce_flashing`](crate::world::WorldToggles::reduce_flashing).
    /// **Default ON**; turning it off restores the retail-exact palette
    /// steps. Purely presentational - the move-VM simulation state is
    /// identical either way.
    pub reduce_flashing: bool,
}

impl Default for OptionsState {
    fn default() -> Self {
        Self {
            battle_camera: BattleCameraOpt::Close,
            battle_select_attack: SelectAttackOpt::Select,
            battle_command: BattleCommandOpt::DirectionalButtons,
            field_move: FieldMoveOpt::Walk,
            field_hp_display: HpDisplayOpt::Immediate,
            audio: AudioMode::Stereo,
            vibration_battles: true,
            vibration_events: true,
            vibration_encounters: true,
            bgm_volume: 8,
            sfx_volume: 8,
            message_speed: 5,
            muted: false,
            camera_distance: crate::camera::CameraDistance::Far,
            precise_movement: false,
            reduce_flashing: true,
        }
    }
}

impl OptionsState {
    /// Load from a TOML file, falling back to [`Default`] if the file is
    /// absent or unparseable.
    pub fn load_or_default(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        toml::from_str(&text).unwrap_or_default()
    }

    /// Persist to a TOML file. Creates parent directories as needed.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string(self)?;
        std::fs::write(path, text)?;
        Ok(())
    }
}

/// One editable setting - the engine mirror of a retail row-descriptor
/// node's config-word pointer + value list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsSetting {
    BattleCamera,
    SelectAttack,
    BattleCommand,
    FieldMove,
    HpDisplay,
    Sound,
    VibrationBattles,
    VibrationEvents,
    VibrationEncounters,
}

impl OptionsSetting {
    /// The retail value strings, in config-word order (value string =
    /// `strings[label + value + 1]` in the `0x801E442C` pointer table).
    /// Retail renders `BattleCommand`'s second choice as a cross-button
    /// glyph + " button"; the engine spells it out.
    pub fn choices(self) -> &'static [&'static str] {
        match self {
            Self::BattleCamera => &["Close", "Normal", "Far"],
            Self::SelectAttack => &["Select", "Automatic", "Command"],
            Self::BattleCommand => &["Directional Buttons", "Cross Button"],
            Self::FieldMove => &["Walk", "Run"],
            Self::HpDisplay => &["Immediate", "Gradual", "Display Off"],
            Self::Sound => &["Stereo", "Monaural"],
            Self::VibrationBattles | Self::VibrationEvents | Self::VibrationEncounters => {
                &["Vibration On", "Vibration Off"]
            }
        }
    }

    /// Current value index (the retail config-word value).
    pub fn get(self, s: &OptionsState) -> u8 {
        match self {
            Self::BattleCamera => s.battle_camera as u8,
            Self::SelectAttack => s.battle_select_attack as u8,
            Self::BattleCommand => s.battle_command as u8,
            Self::FieldMove => s.field_move as u8,
            Self::HpDisplay => s.field_hp_display as u8,
            Self::Sound => s.audio as u8,
            Self::VibrationBattles => !s.vibration_battles as u8,
            Self::VibrationEvents => !s.vibration_events as u8,
            Self::VibrationEncounters => !s.vibration_encounters as u8,
        }
    }

    /// Write value index `v` back (clamped to the choice count).
    pub fn set(self, s: &mut OptionsState, v: u8) {
        let v = v.min(self.choices().len() as u8 - 1);
        match self {
            Self::BattleCamera => {
                s.battle_camera = [
                    BattleCameraOpt::Close,
                    BattleCameraOpt::Normal,
                    BattleCameraOpt::Far,
                ][v as usize]
            }
            Self::SelectAttack => {
                s.battle_select_attack = [
                    SelectAttackOpt::Select,
                    SelectAttackOpt::Automatic,
                    SelectAttackOpt::Command,
                ][v as usize]
            }
            Self::BattleCommand => {
                s.battle_command = [
                    BattleCommandOpt::DirectionalButtons,
                    BattleCommandOpt::CrossButton,
                ][v as usize]
            }
            Self::FieldMove => s.field_move = [FieldMoveOpt::Walk, FieldMoveOpt::Run][v as usize],
            Self::HpDisplay => {
                s.field_hp_display = [
                    HpDisplayOpt::Immediate,
                    HpDisplayOpt::Gradual,
                    HpDisplayOpt::DisplayOff,
                ][v as usize]
            }
            Self::Sound => s.audio = [AudioMode::Stereo, AudioMode::Mono][v as usize],
            Self::VibrationBattles => s.vibration_battles = v == 0,
            Self::VibrationEvents => s.vibration_events = v == 0,
            Self::VibrationEncounters => s.vibration_encounters = v == 0,
        }
    }
}

/// One display row - the engine mirror of a `0x801E4404` layout entry
/// joined to its `0x801E44B8` descriptor node.
#[derive(Debug, Clone, Copy)]
pub struct OptionsRowDef {
    /// On-screen label (the Dual Shock sub-rows carry the retail
    /// two-space indent).
    pub label: &'static str,
    /// Label ink: `false` = white (retail ink 7), `true` = teal (ink 5,
    /// the Dual Shock sub-rows).
    pub teal: bool,
    /// Row pitch below this row in pixels (retail advance word: 14
    /// normally, 20 on the group-separator rows).
    pub advance: i32,
    /// The setting this row edits; `None` for the "Dual Shock" header.
    pub setting: Option<OptionsSetting>,
    /// A row that opens a sub-screen instead of a value popup. `None` for
    /// every retail row - this is the hook the engine-only
    /// [`OPTIONS_KEY_CONFIG_ROW`] hangs off, and it is what makes that row
    /// cursor-selectable without a config word behind it (the navigation
    /// skips a row with neither a setting nor an action, which is how the
    /// "Dual Shock" header stays unselectable).
    pub action: Option<OptionsRowAction>,
}

/// What a row with no config word does when Cross lands on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsRowAction {
    /// Open the key-rebind sub-screen
    /// ([`crate::key_rebind::KeyRebindSession`]).
    KeyConfig,
}

impl OptionsRowDef {
    /// Can the browse cursor land on this row? A retail value row can, the
    /// "Dual Shock" header cannot, and an action row can.
    pub fn selectable(&self) -> bool {
        self.setting.is_some() || self.action.is_some()
    }
}

/// The retail display order + pitch (layout table `0x801E4404`, row ids
/// `0,1,2,3,6,4,7,9,8,10`).
// PORT: FUN_801d2910
pub const OPTIONS_DISPLAY_ROWS: [OptionsRowDef; 10] = [
    OptionsRowDef {
        label: "Battle Camera",
        teal: false,
        advance: 14,
        setting: Some(OptionsSetting::BattleCamera),
        action: None,
    },
    OptionsRowDef {
        label: "Battle Select Attack",
        teal: false,
        advance: 14,
        setting: Some(OptionsSetting::SelectAttack),
        action: None,
    },
    OptionsRowDef {
        label: "Battle Command",
        teal: false,
        advance: 20,
        setting: Some(OptionsSetting::BattleCommand),
        action: None,
    },
    OptionsRowDef {
        label: "Field Move",
        teal: false,
        advance: 14,
        setting: Some(OptionsSetting::FieldMove),
        action: None,
    },
    OptionsRowDef {
        label: "Field HP Display",
        teal: false,
        advance: 20,
        setting: Some(OptionsSetting::HpDisplay),
        action: None,
    },
    OptionsRowDef {
        label: "Sound",
        teal: false,
        advance: 14,
        setting: Some(OptionsSetting::Sound),
        action: None,
    },
    OptionsRowDef {
        label: "Dual Shock",
        teal: false,
        advance: 14,
        setting: None,
        action: None,
    },
    OptionsRowDef {
        label: "  Battles",
        teal: true,
        advance: 14,
        setting: Some(OptionsSetting::VibrationBattles),
        action: None,
    },
    OptionsRowDef {
        label: "  Events",
        teal: true,
        advance: 14,
        setting: Some(OptionsSetting::VibrationEvents),
        action: None,
    },
    OptionsRowDef {
        label: "  Encounters",
        teal: true,
        advance: 14,
        setting: Some(OptionsSetting::VibrationEncounters),
        action: None,
    },
];

/// The engine-only **Key Config** row, appended below the retail ten.
///
/// Retail has no such row: its bindings are the pad, and there is nothing to
/// edit. The port runs on keyboards, so the binding table
/// ([`crate::input::Mapping`]) is a real setting - and until this row existed
/// it had no menu route on either host, which is why
/// [`crate::key_rebind::KeyRebindSession`] sat complete and unreached (the
/// native binary edited `legaia-input.toml` out of band and the browser page
/// could not rebind at all).
///
/// It carries no config word, so it is [`OptionsRowAction::KeyConfig`] rather
/// than an [`OptionsSetting`]: Cross opens the sub-screen, and the value
/// column reads `Edit` so the row does not look like the value-less "Dual
/// Shock" group header directly above it.
///
/// A host that has no binding table to edit (a replay driver, an oracle)
/// simply builds its [`OptionsSession`] with [`OptionsSession::new`] and never
/// sees the row - which is what keeps the retail ten the default display set.
pub const OPTIONS_KEY_CONFIG_ROW: OptionsRowDef = OptionsRowDef {
    label: "Key Config",
    teal: false,
    advance: 14,
    setting: None,
    action: Some(OptionsRowAction::KeyConfig),
};

/// [`OPTIONS_DISPLAY_ROWS`] plus [`OPTIONS_KEY_CONFIG_ROW`] - what a host
/// that armed the session with a binding table displays.
pub const OPTIONS_DISPLAY_ROWS_WITH_KEY_CONFIG: [OptionsRowDef; 11] = {
    let mut out = [OPTIONS_KEY_CONFIG_ROW; 11];
    let mut i = 0;
    while i < OPTIONS_DISPLAY_ROWS.len() {
        out[i] = OPTIONS_DISPLAY_ROWS[i];
        i += 1;
    }
    out
};

/// The display set for a session armed with (`true`) or without (`false`) a
/// binding table. One accessor so a host cannot pick the wrong array.
///
/// The retail set is the row span sub-screen `0x17` hands the generic picker
/// ([`OPTIONS_SUBSCREEN_ROW_SPAN`]), sliced out of the layout table - so the
/// span, not the table's length, is what bounds the browse cursor.
pub fn options_display_rows(key_config: bool) -> &'static [OptionsRowDef] {
    static RETAIL: [OptionsRowDef; 10] = OPTIONS_DISPLAY_ROWS;
    if key_config {
        &OPTIONS_DISPLAY_ROWS_WITH_KEY_CONFIG
    } else {
        let (start, end) = OPTIONS_SUBSCREEN_ROW_SPAN;
        &RETAIL[usize::from(start)..=usize::from(end)]
    }
}

/// The options screen's row span when it runs as menu sub-screen `0x17`:
/// retail's table slot is a thin wrapper handing the generic picker
/// `FUN_801DA9F8` the fixed argument tuple `(start = 0, end = 9,
/// window = 0x30, return_subscreen = 1)` - all ten display rows, the
/// window the picker raises, and the root command picker (sub-screen `1`)
/// as the exit destination.
///
/// The picker's state 0 stores the third argument into bytes `+5` and `+9`
/// of the window script at `0x801E4E08` (`0x801DAA78` / `0x801DAA7C`) - the
/// window-id bytes of its first two 4-byte instructions - so `0x30` is a
/// window id, not an init word. The fourth argument is what its exit arm
/// stores into the sub-screen id `DAT_801E46A4` (`sw s6,0x46a4(v0)` at
/// `0x801DAC24`).
///
/// PORT: FUN_801dd330 (see
/// `ghidra/scripts/funcs/overlay_menu_801dd330.txt` - nothing but the call)
///
/// Live: [`options_display_rows`] slices the retail display set out of the
/// layout table with this span, and every [`OptionsSession`] both play hosts
/// open browses that slice. The wrapper's other two arguments have engine
/// counterparts rather than readers: the exit destination `1` is the root
/// command picker (`FUN_801D6B20`) that `FieldMenuSession::resume` drops back
/// to, and window `0x30` (48) is the settings window the hosts draw off the
/// disc-parsed descriptor table.
pub const OPTIONS_SUBSCREEN_ROW_SPAN: (u8, u8) = (0, 9);

// The span must cover the layout table exactly - a longer span would read
// past it, a shorter one hide retail rows.
const _: () = assert!(
    OPTIONS_SUBSCREEN_ROW_SPAN.1 as usize + 1 - OPTIONS_SUBSCREEN_ROW_SPAN.0 as usize
        == OPTIONS_DISPLAY_ROWS.len()
);
/// The wrapper's third argument (`li a2, 0x30`): the window id the picker
/// patches into its window script.
pub const OPTIONS_SUBSCREEN_INIT: u32 = 0x30;
/// The wrapper's exit destination (`li a3, 0x1` - the root command picker).
pub const OPTIONS_SUBSCREEN_RETURN: u8 = 0x01;

/// Phase of the SM. Mirrors the retail flow: browse over the display
/// rows, Cross opens the value popup (retail window id 47), Cross inside
/// commits, Circle exits (edits already committed - retail never
/// reverts).
// PORT: FUN_801da9f8
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsPhase {
    Browsing {
        cursor: u8,
    },
    /// Value popup open on display row `cursor`; `choice` is the popup
    /// cursor (`DAT_801e46d0`).
    Editing {
        cursor: u8,
        choice: u8,
    },
    /// The engine-only key-rebind sub-screen is open on display row
    /// `cursor` ([`OPTIONS_KEY_CONFIG_ROW`]). The live rows and the
    /// await-key beat belong to [`OptionsSession::key_rebind`].
    Rebinding {
        cursor: u8,
    },
    Done(OptionsOutcome),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsOutcome {
    /// Player left the screen. Value edits were committed at popup
    /// confirm time, so there is no separate cancelled/confirmed split
    /// (retail writes the config word inside the popup and never
    /// reverts).
    Closed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OptionsInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub cross: bool,
    pub circle: bool,
    pub start: bool,
}

impl OptionsInput {
    pub fn from_pad_edge(pressed: u16) -> Self {
        Self {
            up: pressed & PadButton::Up.mask() != 0,
            down: pressed & PadButton::Down.mask() != 0,
            left: pressed & PadButton::Left.mask() != 0,
            right: pressed & PadButton::Right.mask() != 0,
            cross: pressed & PadButton::Cross.mask() != 0,
            circle: pressed & PadButton::Circle.mask() != 0,
            start: pressed & PadButton::Start.mask() != 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsEvent {
    CursorMoved {
        row: u8,
    },
    /// Value popup opened on a row.
    EditOpened {
        row: u8,
    },
    /// A choice was committed into the state.
    ValueChanged {
        setting: OptionsSetting,
    },
    /// Popup closed without committing.
    EditCancelled,
    /// The key-rebind sub-screen opened on a row.
    KeyRebindOpened {
        row: u8,
    },
    /// A key was bound (or the sub-screen closed after one was). The
    /// session's [`OptionsSession::mapping`] carries the new table and
    /// [`OptionsSession::take_bindings_dirty`] reports it once, so a host
    /// persists exactly when the table changed.
    BindingsChanged,
    /// The key-rebind sub-screen closed.
    KeyRebindClosed,
    Closed,
}

/// Live popup view (row + choices + popup cursor) for renderers.
#[derive(Debug, Clone, Copy)]
pub struct OptionsPopup {
    /// Display-row index the popup hangs off.
    pub row: usize,
    pub setting: OptionsSetting,
    pub choices: &'static [&'static str],
    /// Popup cursor (index into `choices`).
    pub cursor: u8,
}

/// Compute the value popup's **content rect**, in 320x240 stage pixels.
///
/// Retail stamps window descriptor id 47's y/h at popup-open time:
/// `y = settings_y + 0x16 + sum(advances above the cursor row)`,
/// `h = count*13 - 4`, flipped above the anchor when the bottom would
/// pass y=0xB0. X/w stay the descriptor's static `(170, 128)`.
// PORT: FUN_801da9f8
pub fn options_popup_content_rect(
    settings_y: i32,
    popup_x: i32,
    popup_w: i32,
    row: usize,
    count: usize,
) -> (i32, i32, i32, i32) {
    let mut y = settings_y + 0x16;
    for def in OPTIONS_DISPLAY_ROWS.iter().take(row) {
        y += def.advance;
    }
    let h = count as i32 * 13 - 4;
    if y + h > 0xB0 {
        y -= count as i32 * 13 + 0x1c;
    }
    (popup_x, y, popup_w, h)
}

#[derive(Debug, Clone)]
pub struct OptionsSession {
    state: OptionsState,
    phase: OptionsPhase,
    /// The host's live keyboard binding table, when it armed the Key Config
    /// row. `None` keeps the display set at the retail ten.
    mapping: Option<Mapping>,
    /// Live rebind sub-screen, present only in [`OptionsPhase::Rebinding`].
    rebind: Option<KeyRebindSession>,
    /// Set whenever a bind lands; cleared by
    /// [`Self::take_bindings_dirty`]. The host's cue to persist -
    /// `legaia-input.toml` natively, the page's binding key in
    /// `localStorage` in the browser.
    bindings_dirty: bool,
}

impl OptionsSession {
    pub fn new(initial: OptionsState) -> Self {
        Self {
            state: initial,
            phase: OptionsPhase::Browsing { cursor: 0 },
            mapping: None,
            rebind: None,
            bindings_dirty: false,
        }
    }

    /// Build the session with the Key Config row armed against `mapping`.
    ///
    /// The row is the *menu route* the key-rebind screen always lacked; the
    /// session owns the edit state so both hosts drive one state machine and
    /// neither can grow its own.
    pub fn with_key_rebind(initial: OptionsState, mapping: Mapping) -> Self {
        Self {
            mapping: Some(mapping),
            ..Self::new(initial)
        }
    }

    /// Arm (or re-arm) the Key Config row on an already-built session - the
    /// hook a host uses when the sub-session was constructed by the shared
    /// dispatcher. No-op-safe to call twice.
    pub fn arm_key_rebind(&mut self, mapping: Mapping) {
        self.mapping = Some(mapping);
    }

    /// Is the Key Config row displayed?
    pub fn key_config_armed(&self) -> bool {
        self.mapping.is_some()
    }

    /// The display rows this session shows - the retail ten, plus
    /// [`OPTIONS_KEY_CONFIG_ROW`] when armed.
    pub fn display_rows(&self) -> &'static [OptionsRowDef] {
        options_display_rows(self.key_config_armed())
    }

    /// The live binding table, once armed. A host reads this back after
    /// [`Self::take_bindings_dirty`] answers `true`.
    pub fn mapping(&self) -> Option<&Mapping> {
        self.mapping.as_ref()
    }

    /// The open key-rebind sub-screen, for the renderer.
    pub fn key_rebind(&self) -> Option<&KeyRebindSession> {
        self.rebind.as_ref()
    }

    /// Report-and-clear: did a bind land since the last call?
    pub fn take_bindings_dirty(&mut self) -> bool {
        std::mem::take(&mut self.bindings_dirty)
    }

    pub fn state(&self) -> &OptionsState {
        &self.state
    }

    /// Current display-row cursor (valid in Browsing, Editing and
    /// Rebinding).
    pub fn cursor(&self) -> u8 {
        match self.phase {
            OptionsPhase::Browsing { cursor }
            | OptionsPhase::Editing { cursor, .. }
            | OptionsPhase::Rebinding { cursor } => cursor,
            _ => 0,
        }
    }

    pub fn phase(&self) -> OptionsPhase {
        self.phase
    }

    pub fn is_done(&self) -> bool {
        matches!(self.phase, OptionsPhase::Done(_))
    }

    pub fn outcome(&self) -> Option<OptionsOutcome> {
        match self.phase {
            OptionsPhase::Done(o) => Some(o),
            _ => None,
        }
    }

    /// Live value popup, when the session is editing a row.
    pub fn popup(&self) -> Option<OptionsPopup> {
        let OptionsPhase::Editing { cursor, choice } = self.phase else {
            return None;
        };
        let setting = OPTIONS_DISPLAY_ROWS.get(cursor as usize)?.setting?;
        Some(OptionsPopup {
            row: cursor as usize,
            setting,
            choices: setting.choices(),
            cursor: choice,
        })
    }

    /// Move `cursor` by `dir`, skipping rows the cursor cannot land on (the
    /// retail SM re-navigates off the Dual Shock header). Wraps at the ends.
    fn step(rows: &[OptionsRowDef], cursor: u8, dir: i8) -> u8 {
        let n = rows.len() as i8;
        let mut c = cursor as i8;
        loop {
            c = (c + dir).rem_euclid(n);
            if rows[c as usize].selectable() {
                return c as u8;
            }
        }
    }

    /// [`Self::tick_with_key`] with no key event - what a host that has not
    /// armed the Key Config row calls.
    pub fn tick(&mut self, input: OptionsInput) -> Vec<OptionsEvent> {
        self.tick_with_key(input, None)
    }

    /// Drive one frame, optionally carrying the host's most-recent keyboard
    /// key name.
    ///
    /// `key_pressed` is consumed only while the key-rebind sub-screen is
    /// awaiting a key, and it is the engine's own key-name vocabulary
    /// (`"Z"`, `"RShift"`, ... - [`crate::input::KEY_NAME_DOM_CODES`] maps it
    /// to the browser's `KeyboardEvent.code`), so both hosts hand over the
    /// same strings the TOML config and `legaia-engine config set --binding`
    /// speak.
    ///
    /// It rides beside [`OptionsInput`] rather than inside it because that
    /// bundle is `Copy` and every existing caller builds it from a pad word.
    pub fn tick_with_key(
        &mut self,
        input: OptionsInput,
        key_pressed: Option<&str>,
    ) -> Vec<OptionsEvent> {
        let rows = self.display_rows();
        let mut events = Vec::new();
        match self.phase {
            OptionsPhase::Browsing { cursor } => {
                if input.circle || input.start {
                    self.phase = OptionsPhase::Done(OptionsOutcome::Closed);
                    events.push(OptionsEvent::Closed);
                    return events;
                }
                if input.cross
                    && rows[cursor as usize].action == Some(OptionsRowAction::KeyConfig)
                    && let Some(mapping) = self.mapping.clone()
                {
                    self.rebind = Some(KeyRebindSession::new(mapping));
                    self.phase = OptionsPhase::Rebinding { cursor };
                    events.push(OptionsEvent::KeyRebindOpened { row: cursor });
                    return events;
                }
                if input.cross
                    && let Some(setting) = rows[cursor as usize].setting
                {
                    self.phase = OptionsPhase::Editing {
                        cursor,
                        choice: setting.get(&self.state),
                    };
                    events.push(OptionsEvent::EditOpened { row: cursor });
                    return events;
                }
                let mut new_cursor = cursor;
                if input.up {
                    new_cursor = Self::step(rows, cursor, -1);
                } else if input.down {
                    new_cursor = Self::step(rows, cursor, 1);
                }
                if new_cursor != cursor {
                    self.phase = OptionsPhase::Browsing { cursor: new_cursor };
                    events.push(OptionsEvent::CursorMoved { row: new_cursor });
                }
            }
            OptionsPhase::Rebinding { cursor } => {
                let Some(sub) = self.rebind.as_mut() else {
                    self.phase = OptionsPhase::Browsing { cursor };
                    return events;
                };
                let sub_events = sub.tick(KeyRebindInput {
                    up: input.up,
                    down: input.down,
                    cross: input.cross,
                    circle: input.circle,
                    start: input.start,
                    key_pressed: key_pressed.map(str::to_string),
                });
                let bound = sub_events
                    .iter()
                    .any(|e| matches!(e, KeyRebindEvent::Bound { .. }));
                if bound {
                    // Retail's options screen commits a value the moment the
                    // popup confirms and never reverts; the rebind screen
                    // follows that rule, so the table is published (and the
                    // host persists) per bind rather than at close.
                    self.mapping = Some(sub.mapping().clone());
                    self.bindings_dirty = true;
                    events.push(OptionsEvent::BindingsChanged);
                }
                if sub.is_done() {
                    self.mapping = Some(sub.mapping().clone());
                    self.rebind = None;
                    self.phase = OptionsPhase::Browsing { cursor };
                    events.push(OptionsEvent::KeyRebindClosed);
                }
            }
            OptionsPhase::Editing { cursor, choice } => {
                let Some(setting) = rows[cursor as usize].setting else {
                    self.phase = OptionsPhase::Browsing { cursor };
                    return events;
                };
                let n = setting.choices().len() as i8;
                if input.cross {
                    setting.set(&mut self.state, choice);
                    self.phase = OptionsPhase::Browsing { cursor };
                    events.push(OptionsEvent::ValueChanged { setting });
                    return events;
                }
                if input.circle {
                    self.phase = OptionsPhase::Browsing { cursor };
                    events.push(OptionsEvent::EditCancelled);
                    return events;
                }
                let mut new_choice = choice as i8;
                if input.up {
                    new_choice = (new_choice - 1).rem_euclid(n);
                } else if input.down {
                    new_choice = (new_choice + 1).rem_euclid(n);
                }
                if new_choice as u8 != choice {
                    self.phase = OptionsPhase::Editing {
                        cursor,
                        choice: new_choice as u8,
                    };
                }
            }
            OptionsPhase::Done(_) => {}
        }
        events
    }
}

/// Plain-data view of one display row for the renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionsRowView {
    pub label: &'static str,
    /// Current value string; `None` on the header row.
    pub value: Option<&'static str>,
    /// Teal label ink (the Dual Shock sub-rows).
    pub teal: bool,
    /// Pixel pitch below this row.
    pub advance: i32,
}

impl OptionsState {
    /// The retail display rows with their live value strings.
    pub fn rows(&self) -> Vec<OptionsRowView> {
        self.rows_for(false)
    }

    /// The display rows for a session with (`true`) or without (`false`) the
    /// engine-only Key Config row - the one that opens the key-rebind
    /// sub-screen. Its value column reads `Edit`: it has no config word, so
    /// there is no retail value string to show, and an empty column would
    /// make it read as a group header.
    pub fn rows_for(&self, key_config: bool) -> Vec<OptionsRowView> {
        options_display_rows(key_config)
            .iter()
            .map(|def| OptionsRowView {
                label: def.label,
                value: match (def.setting, def.action) {
                    (Some(s), _) => Some(s.choices()[s.get(self) as usize]),
                    (None, Some(OptionsRowAction::KeyConfig)) => Some("Edit"),
                    (None, None) => None,
                },
                teal: def.teal,
                advance: def.advance,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_match_retail_display_order() {
        let rows = OptionsState::default().rows();
        let labels: Vec<&str> = rows.iter().map(|r| r.label).collect();
        assert_eq!(
            labels,
            vec![
                "Battle Camera",
                "Battle Select Attack",
                "Battle Command",
                "Field Move",
                "Field HP Display",
                "Sound",
                "Dual Shock",
                "  Battles",
                "  Events",
                "  Encounters",
            ]
        );
        // Group separators: rows 2 (Battle Command) and 4 (HP Display)
        // carry the 20-px advance; everything else is 14.
        assert_eq!(rows[2].advance, 20);
        assert_eq!(rows[4].advance, 20);
        assert!(
            rows.iter()
                .enumerate()
                .filter(|(i, _)| *i != 2 && *i != 4)
                .all(|(_, r)| r.advance == 14)
        );
        // Dual Shock header has no value; sub-rows are teal.
        assert_eq!(rows[6].value, None);
        assert!(!rows[6].teal);
        assert!(rows[7].teal && rows[8].teal && rows[9].teal);
        assert_eq!(rows[7].value, Some("Vibration On"));
    }

    #[test]
    fn default_values_match_retail_capture() {
        let rows = OptionsState::default().rows();
        assert_eq!(rows[0].value, Some("Close"));
        assert_eq!(rows[1].value, Some("Select"));
        assert_eq!(rows[2].value, Some("Directional Buttons"));
        assert_eq!(rows[3].value, Some("Walk"));
        assert_eq!(rows[4].value, Some("Immediate"));
        assert_eq!(rows[5].value, Some("Stereo"));
    }

    #[test]
    fn cursor_moves_with_down() {
        let mut s = OptionsSession::new(OptionsState::default());
        let evs = s.tick(OptionsInput {
            down: true,
            ..Default::default()
        });
        assert_eq!(s.cursor(), 1);
        assert_eq!(evs, vec![OptionsEvent::CursorMoved { row: 1 }]);
    }

    #[test]
    fn cursor_skips_dual_shock_header() {
        let mut s = OptionsSession::new(OptionsState::default());
        // Walk down from row 0 past Sound (row 5); the header (row 6)
        // must be skipped straight to "  Battles" (row 7).
        for _ in 0..5 {
            let _ = s.tick(OptionsInput {
                down: true,
                ..Default::default()
            });
        }
        assert_eq!(s.cursor(), 5);
        let _ = s.tick(OptionsInput {
            down: true,
            ..Default::default()
        });
        assert_eq!(s.cursor(), 7);
        // ...and back up skips it too.
        let _ = s.tick(OptionsInput {
            up: true,
            ..Default::default()
        });
        assert_eq!(s.cursor(), 5);
    }

    #[test]
    fn cross_opens_popup_seeded_with_current_value() {
        let st = OptionsState {
            battle_camera: BattleCameraOpt::Far,
            ..Default::default()
        };
        let mut s = OptionsSession::new(st);
        let evs = s.tick(OptionsInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(evs, vec![OptionsEvent::EditOpened { row: 0 }]);
        let popup = s.popup().expect("popup open");
        assert_eq!(popup.choices, &["Close", "Normal", "Far"]);
        assert_eq!(popup.cursor, 2);
    }

    #[test]
    fn popup_commit_writes_value_and_survives_close() {
        let mut s = OptionsSession::new(OptionsState::default());
        // Open the Battle Camera popup, pick "Normal", commit.
        let _ = s.tick(OptionsInput {
            cross: true,
            ..Default::default()
        });
        let _ = s.tick(OptionsInput {
            down: true,
            ..Default::default()
        });
        let evs = s.tick(OptionsInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(
            evs,
            vec![OptionsEvent::ValueChanged {
                setting: OptionsSetting::BattleCamera
            }]
        );
        assert_eq!(s.state().battle_camera, BattleCameraOpt::Normal);
        assert!(s.popup().is_none());
        // Circle-exit keeps the committed value (retail never reverts).
        let _ = s.tick(OptionsInput {
            circle: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(OptionsOutcome::Closed));
        assert_eq!(s.state().battle_camera, BattleCameraOpt::Normal);
    }

    #[test]
    fn popup_cancel_leaves_value_untouched() {
        let mut s = OptionsSession::new(OptionsState::default());
        let _ = s.tick(OptionsInput {
            cross: true,
            ..Default::default()
        });
        let _ = s.tick(OptionsInput {
            down: true,
            ..Default::default()
        });
        let evs = s.tick(OptionsInput {
            circle: true,
            ..Default::default()
        });
        assert_eq!(evs, vec![OptionsEvent::EditCancelled]);
        assert_eq!(s.state().battle_camera, BattleCameraOpt::Close);
        assert!(!s.is_done());
    }

    #[test]
    fn vibration_row_toggles_through_popup() {
        let mut s = OptionsSession::new(OptionsState::default());
        // Down to "  Battles" (skipping the header), open, pick Off.
        for _ in 0..6 {
            let _ = s.tick(OptionsInput {
                down: true,
                ..Default::default()
            });
        }
        assert_eq!(s.cursor(), 7);
        let _ = s.tick(OptionsInput {
            cross: true,
            ..Default::default()
        });
        let _ = s.tick(OptionsInput {
            down: true,
            ..Default::default()
        });
        let _ = s.tick(OptionsInput {
            cross: true,
            ..Default::default()
        });
        assert!(!s.state().vibration_battles);
        assert!(s.state().vibration_events);
    }

    #[test]
    fn sound_row_maps_to_audio_mode() {
        let mut st = OptionsState::default();
        OptionsSetting::Sound.set(&mut st, 1);
        assert_eq!(st.audio, AudioMode::Mono);
        assert_eq!(st.rows()[5].value, Some("Monaural"));
    }

    #[test]
    fn popup_rect_matches_retail_math() {
        // Row 0 (Battle Camera, 3 choices) on the retail id-48 window
        // (y=40) with the id-47 descriptor x/w (170, 128):
        // y = 40 + 0x16 = 62, h = 3*13-4 = 35, bottom 97 < 0xB0.
        assert_eq!(
            options_popup_content_rect(40, 170, 128, 0, 3),
            (170, 62, 128, 35)
        );
        // Row 9 (Encounters, 2 choices): y = 62 + (14+14+20+14+20+14+14+14+14)
        // = 200, bottom 222 > 0xB0 -> flipped above: y -= 2*13+0x1c = 148.
        let (_, y, _, h) = options_popup_content_rect(40, 170, 128, 9, 2);
        assert_eq!(h, 2 * 13 - 4);
        assert_eq!(y, 200 - (2 * 13 + 0x1c));
        assert!(y + h <= 0xB0);
    }

    #[test]
    fn options_state_toml_round_trip() {
        let dir = std::env::temp_dir().join("legaia_options_test");
        let path = dir.join("options.toml");
        let st = OptionsState {
            field_move: FieldMoveOpt::Run,
            audio: AudioMode::Mono,
            bgm_volume: 3,
            ..Default::default()
        };
        st.save(&path).expect("save");
        let loaded = OptionsState::load_or_default(&path);
        assert_eq!(loaded, st);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
