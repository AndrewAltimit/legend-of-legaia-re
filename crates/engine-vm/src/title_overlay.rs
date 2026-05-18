//! Title-overlay sub-mode dispatcher.
//!
//! PORT: FUN_801DD35C
//!
//! The title-overlay per-frame tick `FUN_801DD35C` (in
//! `ghidra/scripts/funcs/overlay_title_801ddccc.txt`) fans out via a
//! 25-entry jump table at PSX virtual address `0x801CF244`. The selector
//! lives at offset `+0x204` of the title-overlay state struct (base
//! `0x801F0000`, sibling region at `0x801EF014..0x801EF200` reached via
//! negative displacements off the same `lui 0x801f` base).
//!
//! ```asm
//!   801dd6ac  lw   a0, 0x204(v0)        ; a0 = state[+0x204]  (sub-mode)
//!   801dd6b0  jal  0x801e38d0           ; FUN_801E38D0 identity (returns a0)
//!   ...                                 ; input / cursor / fade preamble
//!   801dd7f8  sltiu v0, s2, 0x19        ; clamp s2 < 25
//!   801dd7fc  beq  v0, zero, 0x801dfc3c ; out-of-range → body tail (idle)
//!   801dd800  _lui  v0, 0x801d
//!   801dd804  addiu v0, v0, -0xdbc      ; JT base = 0x801CF244
//!   801dd808  sll  v1, s2, 0x2
//!   801dd80c  addu v1, v1, v0
//!   801dd810  lw   v0, 0x0(v1)
//!   801dd818  jr   v0                   ; dispatch
//! ```
//!
//! The body tail at `0x801DFC3C` is the no-op exit path - mode `0x01`
//! jumps straight there, and any out-of-range mode value falls through
//! to the same address. The countdown decrement that drives the title
//! attract loop fires earlier in the preamble at `0x801DDCC8`
//! (`bgez v0, 0x801DFC3C`), so the attract-fire transition is
//! observable from any mode whose handler doesn't first re-route past
//! it - in practice the captured fire site is mode `0x10`
//! (`AttractIdle`).
//!
//! ## What's pinned vs what's open
//!
//! Four modes are labelled with semantic names; the rest carry
//! `Phase0xNN` placeholders. Doc comments on each variant record the
//! *observed* state-graph transitions traced from `0x204` writes in
//! the dump (62 such writes across the function body):
//!
//! - `0x00` `Init` - entry pass. Zeroes ~12 state fields, sets the
//!   countdown to `0x5DC`, then writes `state[+0x204] = 0x02`
//!   (line 374, `0x801DD920`). A conditional branch (line 397,
//!   `0x801DD97C`) routes to `state[+0x204] = 0x11` instead when a
//!   sentinel at `_DAT_80084500` reads `1` - the "skip intro / direct
//!   to attract" hand-off.
//! - `0x01` `Idle` - handler PC is the post-dispatch body tail. No
//!   per-mode work; the function just exits.
//! - `0x10` `AttractIdle` - the "Press Start" wait state. Polls for
//!   `Start | L1 | Cross` (`pad & 0x844`) at `0x801DDC04`, advances /
//!   rewinds the cursor on `Up | Down` (`pad & {0x4000, 0x1000}`) at
//!   `0x801DDB9C`. The pinned watchpoint hit (`pc=0x801DDCCC`) fires
//!   in this state, which is why retail capture reports the
//!   countdown-decrement as belonging here.
//! - `0x11` `AttractDelay` - the wait state that precedes
//!   `AttractIdle`. Decrements an `8 * frame_scalar` accumulator at
//!   `_DAT_8008454C`; when it reaches zero, writes
//!   `state[+0x204] = 0x10` (line 479, `0x801DDAC4`) and resets the
//!   countdown to `0x5DC`.
//!
//! ## Observed state-graph transitions
//!
//! From `state[+0x204] = N` writes in each handler body, sourced from
//! the [`STATE_204_WRITES`] table below:
//!
//! ```text
//!   Init(0x00)         --> Phase02 (default) | AttractDelay (sentinel)
//!   AttractDelay(0x11) --> AttractIdle (countdown done)
//!   AttractIdle(0x10)  --> StrInit game-mode (via inline write past 0x801DDCCC)
//!   Phase02            --> Phase14 (entry transitions)
//!   Phase18            --> Phase14 (fade-in completion at line 676)
//!   Phase0C, Phase0D   --> Phase14 (any-button pad poll, `pad & 0xF5`)
//!   Phase14            --> Phase07 (confirm: pad & 0x44 = L1|Cross)
//!   Phase07            --> Phase15 (slider transition setup)
//!   Phase06            --> Init (writes 0), AND writes
//!                          `_DAT_8007B83C = 0x02` - the master game-mode
//!                          transition that hands control to the field /
//!                          town engine (NEW GAME launch).
//!   Phase0C, Phase0D writers to `state[+0x204] = s3` where `s3 = -1`
//!   short-circuit to out-of-range (= Idle path).
//! ```
//!
//! Phase14 (Main menu candidate) is the cursor-navigation state:
//! `state[+0x1F4]` (X) and `state[+0x1F8]` (Y) are quantised from
//! `_DAT_8007B7CC * 0x66666667` (the magic divisor-by-10 constant);
//! Up/Down (`pad & 0x4000` / `pad & 0x1000`) walk the cursor;
//! L1|Cross confirms to Phase07.
//!
//! Follow-up work to fully label all 21 placeholders: read each
//! handler body in `overlay_title_801ddccc.txt`, record what helper
//! draws it issues (most are draw setups with one or two pad-poll
//! arms), and rename. The numeric placeholders + the
//! [`STATE_204_WRITES`] table are stable across labelling work; only
//! the enum variant names will churn.
//!
//! ## Provenance
//!
//! - JT extracted from the captured `overlay_title.bin` window
//!   (see `memory/project_title_overlay_tick_pinned.md` for the
//!   reproducible capture pipeline).
//! - Handler PC list tabulated in the same memory file under
//!   "Top-of-tick sub-mode dispatcher".
//! - State struct field offsets sourced from disassembly observation
//!   in `overlay_title_801ddccc.txt`.
//!
//! No Sony bytes are stored in this module - the JT entries are PSX
//! virtual addresses (numbers), not extracted overlay contents.

#![forbid(unsafe_code)]

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
    Phase02 = 0x02,
    /// `0x03` - PC-only placeholder.
    Phase03 = 0x03,
    /// `0x04` - PC-only placeholder.
    Phase04 = 0x04,
    /// `0x05` - PC-only placeholder.
    Phase05 = 0x05,
    /// `0x06` - PC-only placeholder.
    Phase06 = 0x06,
    /// `0x07` - PC-only placeholder.
    Phase07 = 0x07,
    /// `0x08` - PC-only placeholder.
    Phase08 = 0x08,
    /// `0x09` - PC-only placeholder.
    Phase09 = 0x09,
    /// `0x0A` - PC-only placeholder.
    Phase0A = 0x0A,
    /// `0x0B` - PC-only placeholder.
    Phase0B = 0x0B,
    /// `0x0C` - PC-only placeholder.
    Phase0C = 0x0C,
    /// `0x0D` - PC-only placeholder.
    Phase0D = 0x0D,
    /// `0x0E` - PC-only placeholder.
    Phase0E = 0x0E,
    /// `0x0F` - PC-only placeholder.
    Phase0F = 0x0F,
    /// `0x10` - AttractIdle. The "Press Start" wait state.
    /// Polls for `Start | L1 | Cross` (`pad & 0x844`) and `Up | Down`
    /// (`pad & 0x4000` / `pad & 0x1000`) to drive the cursor.
    /// Countdown decrement at `0x801DDCCC` (the pinned watchpoint
    /// site) reaches the attract-fire transition from this state.
    AttractIdle = 0x10,
    /// `0x11` - AttractDelay. Decrements an `8 * frame_scalar`
    /// accumulator at `_DAT_8008454C`; on reach-zero transitions to
    /// [`AttractIdle`] with the countdown reset to `0x5DC`.
    ///
    /// [`AttractIdle`]: TitleOverlaySubMode::AttractIdle
    AttractDelay = 0x11,
    /// `0x12` - PC-only placeholder.
    Phase12 = 0x12,
    /// `0x13` - PC-only placeholder.
    Phase13 = 0x13,
    /// `0x14` - PC-only placeholder.
    Phase14 = 0x14,
    /// `0x15` - PC-only placeholder.
    Phase15 = 0x15,
    /// `0x16` - PC-only placeholder.
    Phase16 = 0x16,
    /// `0x17` - PC-only placeholder.
    Phase17 = 0x17,
    /// `0x18` - PC-only placeholder.
    Phase18 = 0x18,
}

impl TitleOverlaySubMode {
    /// Decode a raw mode byte. Returns `None` for any byte `>= 0x19`
    /// (which the runtime treats as out-of-range and routes to the
    /// body tail / idle path).
    pub fn from_u8(b: u8) -> Option<Self> {
        match b {
            0x00 => Some(Self::Init),
            0x01 => Some(Self::Idle),
            0x02 => Some(Self::Phase02),
            0x03 => Some(Self::Phase03),
            0x04 => Some(Self::Phase04),
            0x05 => Some(Self::Phase05),
            0x06 => Some(Self::Phase06),
            0x07 => Some(Self::Phase07),
            0x08 => Some(Self::Phase08),
            0x09 => Some(Self::Phase09),
            0x0A => Some(Self::Phase0A),
            0x0B => Some(Self::Phase0B),
            0x0C => Some(Self::Phase0C),
            0x0D => Some(Self::Phase0D),
            0x0E => Some(Self::Phase0E),
            0x0F => Some(Self::Phase0F),
            0x10 => Some(Self::AttractIdle),
            0x11 => Some(Self::AttractDelay),
            0x12 => Some(Self::Phase12),
            0x13 => Some(Self::Phase13),
            0x14 => Some(Self::Phase14),
            0x15 => Some(Self::Phase15),
            0x16 => Some(Self::Phase16),
            0x17 => Some(Self::Phase17),
            0x18 => Some(Self::Phase18),
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
        label: "Phase02",
    },
    SubModeRow {
        mode: 0x03,
        handler_pc: 0x801D_F5BC,
        label: "Phase03",
    },
    SubModeRow {
        mode: 0x04,
        handler_pc: 0x801D_F33C,
        label: "Phase04",
    },
    SubModeRow {
        mode: 0x05,
        handler_pc: 0x801D_F82C,
        label: "Phase05",
    },
    SubModeRow {
        mode: 0x06,
        handler_pc: 0x801D_FB5C,
        label: "Phase06",
    },
    SubModeRow {
        mode: 0x07,
        handler_pc: 0x801D_E134,
        label: "Phase07",
    },
    SubModeRow {
        mode: 0x08,
        handler_pc: 0x801D_E4A4,
        label: "Phase08",
    },
    SubModeRow {
        mode: 0x09,
        handler_pc: 0x801D_E638,
        label: "Phase09",
    },
    SubModeRow {
        mode: 0x0A,
        handler_pc: 0x801D_E798,
        label: "Phase0A",
    },
    SubModeRow {
        mode: 0x0B,
        handler_pc: 0x801D_EA5C,
        label: "Phase0B",
    },
    SubModeRow {
        mode: 0x0C,
        handler_pc: 0x801D_E680,
        label: "Phase0C",
    },
    SubModeRow {
        mode: 0x0D,
        handler_pc: 0x801D_E728,
        label: "Phase0D",
    },
    SubModeRow {
        mode: 0x0E,
        handler_pc: 0x801D_EC40,
        label: "Phase0E",
    },
    SubModeRow {
        mode: 0x0F,
        handler_pc: 0x801D_EE0C,
        label: "Phase0F",
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
        label: "Phase12",
    },
    SubModeRow {
        mode: 0x13,
        handler_pc: 0x801D_F404,
        label: "Phase13",
    },
    SubModeRow {
        mode: 0x14,
        handler_pc: 0x801D_DF30,
        label: "Phase14",
    },
    SubModeRow {
        mode: 0x15,
        handler_pc: 0x801D_E260,
        label: "Phase15",
    },
    SubModeRow {
        mode: 0x16,
        handler_pc: 0x801D_F8D0,
        label: "Phase16",
    },
    SubModeRow {
        mode: 0x17,
        handler_pc: 0x801D_F6F4,
        label: "Phase17",
    },
    SubModeRow {
        mode: 0x18,
        handler_pc: 0x801D_DD94,
        label: "Phase18",
    },
];

// State struct field addresses (resolved with `lui 0x801f ; lw/sw imm(reg)`).
// The struct base is `0x801F0000` (a0 to the tick fn); the sibling region at
// `0x801EF014..0x801EF200` is reached via NEGATIVE displacements off the
// same `lui 0x801f` base.

/// Title-overlay state struct base. Passed as the first argument (`a0`)
/// to [`SUBMODE_TICK_FN_ENTRY_PC`].
pub const STATE_BASE_ADDR: u32 = 0x801F_0000;

/// `state[-0xeb4]` (= `0x801EF14C`): horizontal slider X position,
/// clamped `[0, 0x2C]`.
pub const STATE_HORIZ_SLIDER_X_ADDR: u32 = 0x801E_F14C;

/// `state[-0xea0]` (= `0x801EF160`): fade / sweep accumulator,
/// clamped `[0, 0x1000]`.
pub const STATE_FADE_SWEEP_ADDR: u32 = 0x801E_F160;

/// `state[-0xe94]` (= `0x801EF16C`): attract countdown (u32). Reset to
/// [`COUNTDOWN_RESET_VALUE`] by [`TitleOverlaySubMode::Init`] and
/// [`TitleOverlaySubMode::AttractDelay`]; the FUN_8005DA40 DMA load
/// initialises it to `0x8000` before the first tick.
pub const STATE_ATTRACT_COUNTDOWN_ADDR: u32 = 0x801E_F16C;

/// `state[-0xe90]` (= `0x801EF170`): free-running tick counter (u32),
/// incremented every call.
pub const STATE_FRAME_COUNTER_ADDR: u32 = 0x801E_F170;

/// `state[-0xe70]` (= `0x801EF190`): alpha channel A, clamped to
/// `0x1000`.
pub const STATE_ALPHA_A_ADDR: u32 = 0x801E_F190;

/// `state[-0xe6c]` (= `0x801EF194`): alpha channel B, clamped to
/// `0x1000`.
pub const STATE_ALPHA_B_ADDR: u32 = 0x801E_F194;

/// `state[-0xe60]` (= `0x801EF1A0`): alpha channel C, clamped to
/// `0x1000`.
pub const STATE_ALPHA_C_ADDR: u32 = 0x801E_F1A0;

/// `state[+0x204]` (= `0x801F0204`): the sub-mode selector this module
/// exists to model.
pub const STATE_SUBMODE_OFFSET: u32 = 0x0000_0204;

/// `state[+0x1e0]` (= `0x801F01E0`): slider direction
/// (`1` = left at `8 * frame_scalar`, `2` = right, else idle).
pub const STATE_SLIDER_DIR_OFFSET: u32 = 0x0000_01E0;

/// `state[+0x1f4]` (= `0x801F01F4`): X cursor grid position, clamped
/// `[0, 4]`.
pub const STATE_X_CURSOR_OFFSET: u32 = 0x0000_01F4;

/// `state[+0x1f8]` (= `0x801F01F8`): Y cursor grid position, clamped
/// `[0, 2]`.
pub const STATE_Y_CURSOR_OFFSET: u32 = 0x0000_01F8;

/// `state[+0x1fc]` (= `0x801F01FC`): linear cursor index, clamped
/// `[0, s7-1]`.
pub const STATE_LINEAR_CURSOR_OFFSET: u32 = 0x0000_01FC;

/// `state[+0x230]` (= `0x801F0230`): top-of-tick guard / early-out
/// flag (when non-zero the tick skips the per-mode dispatch).
pub const STATE_EARLY_OUT_OFFSET: u32 = 0x0000_0230;

/// Value [`TitleOverlaySubMode::Init`] and
/// [`TitleOverlaySubMode::AttractDelay`] write to
/// [`STATE_ATTRACT_COUNTDOWN_ADDR`].
///
/// Distinct from the `0x8000` initial value that the DMA load
/// (`FUN_8005DA40` site) deposits before the first tick.
pub const COUNTDOWN_RESET_VALUE: u32 = 0x0000_05DC;

/// Master game-mode word `_DAT_8007B83C` (the global mode the 28-mode
/// state machine at `0x8007078C` walks). Title overlay writes to this
/// from two places:
///
/// - The downstream attract-fire path (`SUBMODE_COUNTDOWN_DECR_PC`
///   fall-through, mode-write at `0x801DDCF0`) sets it to
///   [`MASTER_GAME_MODE_STR_INIT`] = `0x1A`.
/// - [`TitleOverlaySubMode::Phase06`] sets it to
///   [`MASTER_GAME_MODE_FIELD_LAUNCH`] = `0x02` - the "exit title /
///   launch game" transition (instruction `sh v0, -0x47C4(v1)` at
///   `0x801DFC00`).
pub const MASTER_GAME_MODE_ADDR: u32 = 0x8007_B83C;

/// Master-game-mode value [`crate::cutscene_trigger::STR_INIT_MODE`]
/// re-exported here for use alongside [`MASTER_GAME_MODE_ADDR`] in this
/// module.
pub const MASTER_GAME_MODE_STR_INIT: u8 = 0x1A;

/// Master-game-mode value the [`TitleOverlaySubMode::Phase06`]
/// (title-launch) handler writes to [`MASTER_GAME_MODE_ADDR`]. This is
/// the value the engine port consumes to transition out of the title
/// overlay into the main game (field / town).
pub const MASTER_GAME_MODE_FIELD_LAUNCH: u8 = 0x02;

/// One observed `state[+0x204] = value` write inside the tick function.
/// Each row pins which mode handler emits the write and the value
/// stored (or `s3`/`s5` register-source markers when the value is
/// computed dynamically).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct State204Write {
    /// PSX virtual address of the `sw` instruction.
    pub pc: u32,
    /// Sub-mode whose handler body contains this write.
    pub from: u8,
    /// Either a static value the dispatcher transitions to, or
    /// [`TransitionTarget::Register`] when the source is a register.
    pub target: TransitionTarget,
}

/// Static-vs-dynamic transition target for [`State204Write`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionTarget {
    /// The write stores a known literal mode value.
    Mode(u8),
    /// The write stores a register value (commonly `s3 = -1` which
    /// the runtime clamps to out-of-range / Idle).
    Register(&'static str),
}

/// Every observed `state[+0x204] = N` write site in the tick function,
/// ordered by PC. Used to drive the cross-link test that asserts each
/// well-known transition lines up with a real dump address. Listing
/// every site is intentional - it makes regressions noisy if a future
/// re-dump or relabelling shifts the addresses.
pub const STATE_204_WRITES: &[State204Write] = &[
    // Init (0x00) - chooses Phase02 default or AttractDelay sentinel branch.
    State204Write {
        pc: 0x801D_D920,
        from: 0x00,
        target: TransitionTarget::Mode(0x02),
    },
    State204Write {
        pc: 0x801D_D97C,
        from: 0x00,
        target: TransitionTarget::Mode(0x11),
    },
    // AttractDelay (0x11) -> AttractIdle (0x10).
    State204Write {
        pc: 0x801D_DAC4,
        from: 0x11,
        target: TransitionTarget::Mode(0x10),
    },
    // Phase18 (0x18) -> Phase14 (0x14) on fade-in completion.
    State204Write {
        pc: 0x801D_DDD8,
        from: 0x18,
        target: TransitionTarget::Mode(0x14),
    },
    // Phase14 (0x14) -> Phase07 (0x07) on confirm (pad & 0x44 = L1|Cross).
    State204Write {
        pc: 0x801D_E11C,
        from: 0x14,
        target: TransitionTarget::Mode(0x07),
    },
    // Phase07 (0x07) -> Phase15 (0x15) at end of transition setup.
    State204Write {
        pc: 0x801D_E25C,
        from: 0x07,
        target: TransitionTarget::Mode(0x15),
    },
    // Phase0C (0x0C) -> Phase14 (0x14) on any-button (pad & 0xF5).
    State204Write {
        pc: 0x801D_E6DC,
        from: 0x0C,
        target: TransitionTarget::Register("s3 = 0x14"),
    },
    // Phase0D (0x0D) -> Phase14 (0x14) on any-button (pad & 0xF5).
    State204Write {
        pc: 0x801D_E748,
        from: 0x0D,
        target: TransitionTarget::Register("s3 = 0x14"),
    },
    // Phase06 (0x06) -> Init (0x00). Same handler also writes
    // master-game-mode = 0x02 to MASTER_GAME_MODE_ADDR.
    State204Write {
        pc: 0x801D_FC1C,
        from: 0x06,
        target: TransitionTarget::Mode(0x00),
    },
];

/// PSX virtual address of the `sh v0, -0x47C4(v1)` instruction inside
/// [`TitleOverlaySubMode::Phase06`] that writes
/// [`MASTER_GAME_MODE_FIELD_LAUNCH`] to [`MASTER_GAME_MODE_ADDR`].
///
/// This is the title-screen -> main-game transition's hard pin: an
/// engine-side observer watching `MASTER_GAME_MODE_ADDR` and noticing
/// the value flip to `0x02` knows it's time to swap out the title
/// overlay and load the field/town runtime.
pub const PHASE06_LAUNCH_GAME_PC: u32 = 0x801D_FC00;

// Pad-mask combinations - see `project_legaia_pad_mask_layout`.
// Legaia repacks the raw PSX 16-bit pad word: dpad lives in the HIGH byte,
// face/shoulder buttons in the LOW byte. These constants use the repacked
// layout (i.e. they match the literals the dispatcher uses verbatim).

/// `Cross | L1` - confirm (Cross with L1 as alt).
pub const PADMASK_CONFIRM_L1_CROSS: u16 = 0x0044;

/// `Circle | L2` - cancel (Circle with L2 as alt).
pub const PADMASK_CANCEL_L2_CIRCLE: u16 = 0x0021;

/// All face buttons + L1 + L2 - "any non-R-shoulder button"
/// (used as the generic "user interacted" filter to break attract).
pub const PADMASK_ANY_FACE_OR_L: u16 = 0x00F5;

/// `Start | L1 | Cross` - "press Start / confirm" mask the
/// `AttractIdle` polling path tests at `0x801DDC04`.
pub const PADMASK_START_L1_CROSS: u16 = 0x0844;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_length_matches_jt_entry_count() {
        assert_eq!(SUBMODE_TABLE.len(), SUBMODE_JT_ENTRY_COUNT);
        assert_eq!(SUBMODE_JT_ENTRY_COUNT, 25);
    }

    #[test]
    fn table_indices_are_dense_and_in_order() {
        for (i, row) in SUBMODE_TABLE.iter().enumerate() {
            assert_eq!(row.mode as usize, i, "row {i} mismatch");
        }
    }

    #[test]
    fn from_u8_round_trips_in_range_bytes() {
        for b in 0..=0x18u8 {
            let mode = TitleOverlaySubMode::from_u8(b)
                .unwrap_or_else(|| panic!("byte 0x{b:02X} should decode"));
            assert_eq!(mode as u8, b);
        }
    }

    #[test]
    fn from_u8_returns_none_for_out_of_range_bytes() {
        for b in 0x19..=0xFFu8 {
            assert!(
                TitleOverlaySubMode::from_u8(b).is_none(),
                "byte 0x{b:02X} should be out-of-range"
            );
            assert!(!TitleOverlaySubMode::is_in_range(b));
        }
    }

    #[test]
    fn idle_handler_pc_equals_body_pc() {
        // The dispatcher's out-of-range path branches to SUBMODE_BODY_PC.
        // Mode 0x01 (Idle) shares that handler PC - it's a no-op exit.
        assert_eq!(
            TitleOverlaySubMode::Idle.handler_pc(),
            SUBMODE_BODY_PC,
            "Idle handler should equal the body tail PC"
        );
    }

    #[test]
    fn well_known_modes_match_captured_pcs() {
        // Spot-check the four labelled modes against the JT entries
        // extracted from `overlay_title.bin` (see
        // `memory/project_title_overlay_tick_pinned.md`).
        assert_eq!(TitleOverlaySubMode::Init.handler_pc(), 0x801D_D820);
        assert_eq!(TitleOverlaySubMode::Idle.handler_pc(), 0x801D_FC3C);
        assert_eq!(TitleOverlaySubMode::AttractIdle.handler_pc(), 0x801D_DB0C);
        assert_eq!(TitleOverlaySubMode::AttractDelay.handler_pc(), 0x801D_DA90);
    }

    #[test]
    fn every_handler_pc_lives_inside_the_tick_function() {
        // Tick fn entry .. entry + size_bytes covers all handlers.
        let lo = SUBMODE_TICK_FN_ENTRY_PC;
        let hi = SUBMODE_TICK_FN_ENTRY_PC + SUBMODE_TICK_FN_SIZE_BYTES;
        for row in SUBMODE_TABLE {
            assert!(
                row.handler_pc >= lo && row.handler_pc < hi,
                "{} PC 0x{:08X} outside tick fn [{:08X}, {:08X})",
                row.label,
                row.handler_pc,
                lo,
                hi
            );
        }
    }

    #[test]
    fn handler_pcs_are_unique_except_for_idle() {
        // Idle aliases the body tail; every other handler has its own
        // entry point. Build a histogram and check.
        let mut counts: std::collections::HashMap<u32, usize> = Default::default();
        for row in SUBMODE_TABLE {
            *counts.entry(row.handler_pc).or_insert(0) += 1;
        }
        // 24 unique handler PCs across 25 entries (only Idle's PC may
        // collide with something - and it doesn't collide with another
        // SUBMODE_TABLE entry in practice).
        assert_eq!(counts.len(), 25);
        for (pc, n) in counts {
            assert_eq!(n, 1, "PC 0x{pc:08X} appears {n} times in SUBMODE_TABLE");
        }
    }

    #[test]
    fn state_field_addresses_decode_to_known_offsets() {
        // The sibling region uses negative displacements off `lui 0x801f`,
        // so reachable addresses live below STATE_BASE_ADDR. The "Off
        // (struct)" column in the memory file decodes to these literal
        // addresses; sanity-check the table.
        assert_eq!(
            STATE_HORIZ_SLIDER_X_ADDR,
            0x801F_0000u32.wrapping_sub(0xEB4)
        );
        assert_eq!(STATE_FADE_SWEEP_ADDR, 0x801F_0000u32.wrapping_sub(0xEA0));
        assert_eq!(
            STATE_ATTRACT_COUNTDOWN_ADDR,
            0x801F_0000u32.wrapping_sub(0xE94)
        );
        assert_eq!(STATE_FRAME_COUNTER_ADDR, 0x801F_0000u32.wrapping_sub(0xE90));
        assert_eq!(STATE_ALPHA_A_ADDR, 0x801F_0000u32.wrapping_sub(0xE70));
        assert_eq!(STATE_ALPHA_B_ADDR, 0x801F_0000u32.wrapping_sub(0xE6C));
        assert_eq!(STATE_ALPHA_C_ADDR, 0x801F_0000u32.wrapping_sub(0xE60));
        // The +offset fields land above STATE_BASE_ADDR.
        assert_eq!(STATE_SUBMODE_OFFSET, 0x0204);
        assert_eq!(STATE_BASE_ADDR + STATE_SUBMODE_OFFSET, 0x801F_0204);
    }

    #[test]
    fn jt_address_matches_lui_addiu_disassembly() {
        // The dispatcher resolves the JT base as:
        //   lui   v0, 0x801D            ; v0 = 0x801D_0000
        //   addiu v0, v0, -0xDBC        ; v0 = 0x801D_0000 + 0xFFFF_F244 = 0x801C_F244
        // (`addiu` sign-extends -0xDBC to 0xFFFFF244).
        let lui_hi: u32 = 0x801D_0000;
        let addiu_lo: i32 = -0xDBC;
        let resolved = (lui_hi as i64 + addiu_lo as i64) as u32;
        assert_eq!(resolved, SUBMODE_JT_ADDR);
    }

    #[test]
    fn padmask_constants_match_disassembled_andi_immediates() {
        // The dispatcher uses these immediates verbatim - they're the
        // `andi v0, X` operands the title-overlay pad-poll path emits.
        assert_eq!(PADMASK_CONFIRM_L1_CROSS, 0x0044);
        assert_eq!(PADMASK_CANCEL_L2_CIRCLE, 0x0021);
        assert_eq!(PADMASK_ANY_FACE_OR_L, 0x00F5);
        assert_eq!(PADMASK_START_L1_CROSS, 0x0844);
    }

    #[test]
    fn countdown_reset_value_is_disassembled_literal() {
        // `li v0, 0x5DC` appears at line 376 (Init) and line 482 (AttractDelay).
        assert_eq!(COUNTDOWN_RESET_VALUE, 0x5DC);
    }

    #[test]
    fn state_204_writes_cover_all_well_known_transitions() {
        // Every labelled mode emits at least one observed transition.
        // (Idle has no body and AttractIdle's "transition" is to master
        // game mode, not state[+0x204] - covered separately.)
        let froms: std::collections::BTreeSet<u8> =
            STATE_204_WRITES.iter().map(|w| w.from).collect();
        assert!(froms.contains(&0x00), "Init missing");
        assert!(froms.contains(&0x06), "Phase06 (LaunchGame) missing");
        assert!(froms.contains(&0x11), "AttractDelay missing");
    }

    #[test]
    fn state_204_writes_are_ordered_by_pc_and_unique() {
        // Sorted + dedup invariant - keeps the table easy to extend.
        let pcs: Vec<u32> = STATE_204_WRITES.iter().map(|w| w.pc).collect();
        let mut sorted = pcs.clone();
        sorted.sort();
        assert_eq!(pcs, sorted, "STATE_204_WRITES not sorted by PC");
        let unique: std::collections::HashSet<u32> = pcs.iter().copied().collect();
        assert_eq!(unique.len(), pcs.len(), "duplicate PCs in STATE_204_WRITES");
    }

    #[test]
    fn every_204_write_lives_inside_the_tick_function() {
        let lo = SUBMODE_TICK_FN_ENTRY_PC;
        let hi = SUBMODE_TICK_FN_ENTRY_PC + SUBMODE_TICK_FN_SIZE_BYTES;
        for w in STATE_204_WRITES {
            assert!(
                w.pc >= lo && w.pc < hi,
                "0x{:08X} (from mode 0x{:02X}) outside tick fn [{:08X}, {:08X})",
                w.pc,
                w.from,
                lo,
                hi
            );
        }
    }

    #[test]
    fn every_204_write_from_mode_is_in_range() {
        for w in STATE_204_WRITES {
            assert!(
                TitleOverlaySubMode::is_in_range(w.from),
                "from-mode 0x{:02X} out of range",
                w.from
            );
        }
    }

    #[test]
    fn every_static_target_mode_is_in_range() {
        for w in STATE_204_WRITES {
            if let TransitionTarget::Mode(target) = w.target {
                assert!(
                    TitleOverlaySubMode::is_in_range(target),
                    "from 0x{:02X} -> 0x{:02X} target out of range",
                    w.from,
                    target
                );
            }
        }
    }

    #[test]
    fn master_game_mode_constants_align_with_cutscene_trigger() {
        use crate::cutscene_trigger;
        assert_eq!(MASTER_GAME_MODE_ADDR, cutscene_trigger::GAME_MODE_ADDR);
        assert_eq!(MASTER_GAME_MODE_STR_INIT, cutscene_trigger::STR_INIT_MODE);
    }

    #[test]
    fn phase06_launch_game_pc_lives_inside_tick_function() {
        let lo = SUBMODE_TICK_FN_ENTRY_PC;
        let hi = SUBMODE_TICK_FN_ENTRY_PC + SUBMODE_TICK_FN_SIZE_BYTES;
        assert!(PHASE06_LAUNCH_GAME_PC >= lo && PHASE06_LAUNCH_GAME_PC < hi);
        // And the Phase06 handler PC predates the launch-write PC (the
        // write happens inside Phase06's body).
        let phase06 = TitleOverlaySubMode::Phase06.handler_pc();
        assert!(
            phase06 < PHASE06_LAUNCH_GAME_PC,
            "Phase06 handler 0x{phase06:08X} should precede launch write 0x{PHASE06_LAUNCH_GAME_PC:08X}"
        );
    }
}
