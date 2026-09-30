//! The 25 title-overlay sub-modes and the sub-mode jump table.
//! Split out of `title_overlay.rs`.

/// The 25 sub-modes the title-overlay tick can be in.
///
/// `repr(u8)` so [`from_u8`] is a clamp + cast on the hot path.
///
/// [`from_u8`]: TitleOverlaySubMode::from_u8
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TitleOverlaySubMode {
    /// `0x00` - Init / entry pass. Zeroes UI state fields, sets the
    /// attract countdown to `0x5DC`, then routes to mode `0x02` (or
    /// `0x11` when the early-game sentinel is set).
    Init = 0x00,
    /// `0x01` - Idle. Handler PC is the post-dispatch body tail
    /// ([`SUBMODE_BODY_PC`]); the function just exits.
    Idle = 0x01,
    /// `0x02` - First post-init phase. Currently PC-only.
    TextMenu = 0x02,
    /// `0x03` - PC-only placeholder.
    SaveWrite = 0x03,
    /// `0x04` - PC-only placeholder.
    BlockTransfer = 0x04,
    /// `0x05` - PC-only placeholder.
    LoadVerify = 0x05,
    /// `0x06` - PC-only placeholder.
    LaunchGame = 0x06,
    /// `0x07` - PC-only placeholder.
    CardOpStage = 0x07,
    /// `0x08` - PC-only placeholder.
    CardFault = 0x08,
    /// `0x09` - PC-only placeholder.
    ScanSetup = 0x09,
    /// `0x0A` - PC-only placeholder.
    BlockScan = 0x0A,
    /// `0x0B` - PC-only placeholder.
    SlotGrid = 0x0B,
    /// `0x0C` - PC-only placeholder.
    LoadNotice = 0x0C,
    /// `0x0D` - PC-only placeholder.
    SaveNotice = 0x0D,
    /// `0x0E` - PC-only placeholder.
    SlotConfirm = 0x0E,
    /// `0x0F` - PC-only placeholder.
    CardOpPrompt = 0x0F,
    /// `0x10` - AttractIdle. The "Press Start" wait state.
    /// Polls for `Start | L1 | Cross` (`pad & 0x844`) and `Up | Down`
    /// (`pad & 0x4000` / `pad & 0x1000`) to drive the cursor.
    /// Countdown decrement at `0x801DDCCC` (the pinned watchpoint
    /// site) reaches the attract-fire transition from this state.
    AttractIdle = 0x10,
    /// `0x11` - AttractDelay. Decrements an `8 * frame_scalar`
    /// accumulator at `_DAT_8007BAB4`; on reach-zero transitions to
    /// [`AttractIdle`] with the countdown reset to `0x5DC`.
    ///
    /// [`AttractIdle`]: TitleOverlaySubMode::AttractIdle
    AttractDelay = 0x11,
    /// `0x12` - PC-only placeholder.
    CardOpRun = 0x12,
    /// `0x13` - PC-only placeholder.
    CardOpResult = 0x13,
    /// `0x14` - PC-only placeholder.
    MainMenu = 0x14,
    /// `0x15` - PC-only placeholder.
    CardCheck = 0x15,
    /// `0x16` - PC-only placeholder.
    LaunchFade = 0x16,
    /// `0x17` - PC-only placeholder.
    SaveResult = 0x17,
    /// `0x18` - PC-only placeholder.
    ContinueFadeIn = 0x18,
}

impl TitleOverlaySubMode {
    /// Decode a raw mode byte. Returns `None` for any byte `>= 0x19`
    /// (which the runtime treats as out-of-range and routes to the
    /// body tail / idle path).
    pub fn from_u8(b: u8) -> Option<Self> {
        match b {
            0x00 => Some(Self::Init),
            0x01 => Some(Self::Idle),
            0x02 => Some(Self::TextMenu),
            0x03 => Some(Self::SaveWrite),
            0x04 => Some(Self::BlockTransfer),
            0x05 => Some(Self::LoadVerify),
            0x06 => Some(Self::LaunchGame),
            0x07 => Some(Self::CardOpStage),
            0x08 => Some(Self::CardFault),
            0x09 => Some(Self::ScanSetup),
            0x0A => Some(Self::BlockScan),
            0x0B => Some(Self::SlotGrid),
            0x0C => Some(Self::LoadNotice),
            0x0D => Some(Self::SaveNotice),
            0x0E => Some(Self::SlotConfirm),
            0x0F => Some(Self::CardOpPrompt),
            0x10 => Some(Self::AttractIdle),
            0x11 => Some(Self::AttractDelay),
            0x12 => Some(Self::CardOpRun),
            0x13 => Some(Self::CardOpResult),
            0x14 => Some(Self::MainMenu),
            0x15 => Some(Self::CardCheck),
            0x16 => Some(Self::LaunchFade),
            0x17 => Some(Self::SaveResult),
            0x18 => Some(Self::ContinueFadeIn),
            _ => None,
        }
    }

    /// Whether the raw byte falls inside the dispatcher's in-range
    /// window. Out-of-range bytes are routed to [`SUBMODE_BODY_PC`].
    pub const fn is_in_range(b: u8) -> bool {
        b < SUBMODE_JT_ENTRY_COUNT as u8
    }

    /// PSX virtual address of this mode's handler block in the
    /// `overlay_title.bin` window.
    pub fn handler_pc(self) -> u32 {
        SUBMODE_TABLE[self as usize].handler_pc
    }

    /// The full row from [`SUBMODE_TABLE`] for this mode.
    pub fn row(self) -> SubModeRow {
        SUBMODE_TABLE[self as usize]
    }
}

/// One row in [`SUBMODE_TABLE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubModeRow {
    /// The dispatcher's selector value (`state[+0x204]`).
    pub mode: u8,
    /// Handler entry point PC inside the title overlay.
    pub handler_pc: u32,
    /// Short symbolic label, suitable for log messages and tests.
    pub label: &'static str,
}

/// Number of entries in the dispatcher JT (clamp is
/// `sltiu v0, s2, 0x19` at `0x801DD7F8`).
pub const SUBMODE_JT_ENTRY_COUNT: usize = 25;

/// Base address of the 25-entry JT (resolved as
/// `lui v0, 0x801d ; addiu v0, v0, -0xdbc`).
pub const SUBMODE_JT_ADDR: u32 = 0x801C_F244;

/// Entry point of the per-frame tick function the dispatcher lives in.
pub const SUBMODE_TICK_FN_ENTRY_PC: u32 = 0x801D_D35C;

/// Size of the tick function in bytes (3026 instructions).
pub const SUBMODE_TICK_FN_SIZE_BYTES: u32 = 12_104;

/// The post-dispatch body tail / out-of-range / `Idle` exit target.
pub const SUBMODE_BODY_PC: u32 = 0x801D_FC3C;

/// Address of the `sltiu v0, s2, 0x19` clamp - one instruction before
/// the JT base load.
pub const SUBMODE_DISPATCH_CLAMP_PC: u32 = 0x801D_D7F8;

/// Address of the watchpoint-pinned countdown decrement that triggers
/// the title attract loop (see [`cutscene_trigger::TITLE_TICK_INLINE`]).
///
/// [`cutscene_trigger::TITLE_TICK_INLINE`]: crate::cutscene_trigger::TITLE_TICK_INLINE
pub const SUBMODE_COUNTDOWN_DECR_PC: u32 = 0x801D_DCCC;

/// The 25-entry JT, indexed by mode byte.
///
/// Mode `0x01` is the no-op idle state - its handler PC is the same as
/// [`SUBMODE_BODY_PC`] (the post-dispatch body tail). Every other mode
/// has a distinct handler block earlier in the function body.
pub const SUBMODE_TABLE: [SubModeRow; SUBMODE_JT_ENTRY_COUNT] = [
    SubModeRow {
        mode: 0x00,
        handler_pc: 0x801D_D820,
        label: "Init",
    },
    SubModeRow {
        mode: 0x01,
        handler_pc: 0x801D_FC3C, // = SUBMODE_BODY_PC (idle)
        label: "Idle",
    },
    SubModeRow {
        mode: 0x02,
        handler_pc: 0x801D_DDFC,
        label: "TextMenu",
    },
    SubModeRow {
        mode: 0x03,
        handler_pc: 0x801D_F5BC,
        label: "SaveWrite",
    },
    SubModeRow {
        mode: 0x04,
        handler_pc: 0x801D_F33C,
        label: "BlockTransfer",
    },
    SubModeRow {
        mode: 0x05,
        handler_pc: 0x801D_F82C,
        label: "LoadVerify",
    },
    SubModeRow {
        mode: 0x06,
        handler_pc: 0x801D_FB5C,
        label: "LaunchGame",
    },
    SubModeRow {
        mode: 0x07,
        handler_pc: 0x801D_E134,
        label: "CardOpStage",
    },
    SubModeRow {
        mode: 0x08,
        handler_pc: 0x801D_E4A4,
        label: "CardFault",
    },
    SubModeRow {
        mode: 0x09,
        handler_pc: 0x801D_E638,
        label: "ScanSetup",
    },
    SubModeRow {
        mode: 0x0A,
        handler_pc: 0x801D_E798,
        label: "BlockScan",
    },
    SubModeRow {
        mode: 0x0B,
        handler_pc: 0x801D_EA5C,
        label: "SlotGrid",
    },
    SubModeRow {
        mode: 0x0C,
        handler_pc: 0x801D_E680,
        label: "LoadNotice",
    },
    SubModeRow {
        mode: 0x0D,
        handler_pc: 0x801D_E728,
        label: "SaveNotice",
    },
    SubModeRow {
        mode: 0x0E,
        handler_pc: 0x801D_EC40,
        label: "SlotConfirm",
    },
    SubModeRow {
        mode: 0x0F,
        handler_pc: 0x801D_EE0C,
        label: "CardOpPrompt",
    },
    SubModeRow {
        mode: 0x10,
        handler_pc: 0x801D_DB0C,
        label: "AttractIdle",
    },
    SubModeRow {
        mode: 0x11,
        handler_pc: 0x801D_DA90,
        label: "AttractDelay",
    },
    SubModeRow {
        mode: 0x12,
        handler_pc: 0x801D_EF38,
        label: "CardOpRun",
    },
    SubModeRow {
        mode: 0x13,
        handler_pc: 0x801D_F404,
        label: "CardOpResult",
    },
    SubModeRow {
        mode: 0x14,
        handler_pc: 0x801D_DF30,
        label: "MainMenu",
    },
    SubModeRow {
        mode: 0x15,
        handler_pc: 0x801D_E260,
        label: "CardCheck",
    },
    SubModeRow {
        mode: 0x16,
        handler_pc: 0x801D_F8D0,
        label: "LaunchFade",
    },
    SubModeRow {
        mode: 0x17,
        handler_pc: 0x801D_F6F4,
        label: "SaveResult",
    },
    SubModeRow {
        mode: 0x18,
        handler_pc: 0x801D_DD94,
        label: "ContinueFadeIn",
    },
];

// State struct field addresses (resolved with `lui 0x801f ; lw/sw imm(reg)`).
// The struct base is `0x801F0000` (a0 to the tick fn); the sibling region at
// `0x801EF014..0x801EF200` is reached via NEGATIVE displacements off the
// same `lui 0x801f` base.
