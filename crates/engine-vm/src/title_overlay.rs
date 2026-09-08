//! Title-screen per-frame tick - the sub-mode dispatcher.
//!
//! PORT: FUN_801DD35C
//! REF: FUN_801E36A0
//! REF: FUN_801DD310
//!
//! **Ownership is settled, and this module is the one description of the
//! routine.** Its 48-byte prologue occurs exactly once in all of
//! `PROT.DAT`, at extraction entry **0899** file `+0xEB44`
//! (`0x801CE818 + 0xEB44` reproduces the printed VA), and is absent from
//! `SCUS_942.54`: one resident copy in the menu overlay's image, reached
//! every frame from the nine-instruction wrapper `FUN_801E36A0` (0899
//! `+0x14E88`, `jal 0x801dd35c` with both arguments zeroed) that master
//! mode 22 spawns. The many dumps that carry it - `overlay_menu_801dd35c`,
//! `overlay_title_801ddccc`, `overlay_save_ui_*`, `overlay_shop_save`, all
//! 12104 bytes / 3026 instructions - are that one copy under scenario
//! labels; the short `overlay_801dd35c.txt` that reads differently is a
//! 436-byte PROT 0897 routine `FUN_801DD310` at an aliased VA, not a
//! second copy.
//!
//! Hosting it in the menu overlay's image is what made
//! [`super::menu`] describe it as the *menu's* dispatcher. That reading is
//! falsified by the routine's own operands: it writes only `0x02..=0x18`
//! to its sub-mode word (inside the jump table's `sltiu v0,s2,0x19`
//! bound), never the pause-menu / shop screens `menu.rs` enumerates, and
//! all three of its master-mode stores into `0x8007B83C` are title
//! transitions - `0x1A` (attract -> STR) at `0x801DDCF0`, and `2` (-> the
//! field) twice: at `0x801DFAFC` on the load route and `0x801DFC00` on the
//! NEW GAME route. `menu.rs` now carries a `REF:` and says so. (The
//! two-store count came from following the NEW GAME route only.)
//!
//! The tick fans out via a 25-entry jump table at PSX virtual address
//! `0x801CF244`. The selector
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
//! The body at `0x801DFC3C` is the **shared epilogue**, not a no-op exit:
//! mode `0x01` jumps straight there and any out-of-range mode value falls
//! through to the same address, but every handler also ends there, and the
//! epilogue carries six of the function's 56 sub-mode stores plus the
//! cursor stepping the menu states rely on. The countdown decrement that
//! drives the attract loop is at `0x801DDCC8`
//! (`bgez v0, 0x801DFC3C`) - **inside sub-mode `0x10`'s handler body**, not
//! in the preamble. Nothing outside `0x801DDB0C..0x801DDD94` branches into
//! that block, so `AttractIdle` is the only mode that can fire the attract;
//! the earlier "observable from any mode whose handler doesn't re-route
//! past it" reading placed the decrement in the preamble and is falsified
//! by the branch targets.
//!
//! ## The 25 handlers
//!
//! Every sub-mode is a screen of the front-end: the title menu, the
//! memory-card manager behind CONTINUE / SAVE, and the launcher. The
//! variant names below are the roles the disassembly shows, and the full
//! guard-by-guard graph is [`STATE_204_WRITES`] (all 56 stores).
//!
//! - `0x00` `Init` - entry pass. Zeroes ~12 state fields, seeds the
//!   countdown with `0x5DC`, then writes `state[+0x204] = 0x02`. Three
//!   arms overwrite that: `0x11` when the entry word at `_DAT_8007BB00`
//!   is non-zero (`0x801DD97C`), and `0x14` when the tick's **second
//!   argument** is `1` or `2` (`0x801DDA58` / `0x801DDA80`), which also
//!   pre-selects the menu row in `state[+0x200]`. The production caller
//!   `FUN_801E36A0` passes `0`, so only the entry-word arm runs in the
//!   normal flow.
//!
//!   **The `0x02` arm is dead on retail, twice over.** The boot
//!   `init.pak` (`FUN_801CE9C0`) raises `_DAT_8007BB00` to `1`
//!   unconditionally at `0x801CEB84` (`li s2,0x1` /
//!   `sw s2,-0x4500(s0)`, `s0 = 0x80080000`) before it hands off, so
//!   `Init` always takes the `0x11` arm; and the shared epilogue tests
//!   the same word again at `0x801DFED8` and rewrites `0x02` to `0x10`
//!   whichever handler left it there (`0x801DFEF8`). A cold-boot capture
//!   (`scripts/pcsx-redux/autorun_boot_warning_screen.lua`) sees the
//!   write at that PC, then `AttractDelay` on the frame after the title
//!   mode is entered and `AttractIdle` ~75 vsyncs later; sub-mode `0x02`
//!   never appears.
//!
//!   **Address note.** The stores are at *instruction* addresses
//!   `0x801DD920` / `0x801DD97C`; the *data* word they write is
//!   `state[+0x204]` = `0x801F0204` (`lui a2,0x801f` four instructions
//!   ahead of the first store). Earlier prose quoted the instruction
//!   address as if it were the state word's address.
//! - `0x01` `Idle` - handler PC is the shared epilogue. No per-mode work.
//!   Nothing in the function ever stores `1` to the selector, so this is
//!   the out-of-range slot rather than a state the graph enters.
//! - `0x02` `TextMenu` - a two-row NEW GAME / CONTINUE menu drawn as two
//!   text lines at y `0x6B` / `0x78` with a cursor sprite, confirming on
//!   `pad & 0x44` into `0x14` with the picked row in `state[+0x200]`.
//!   Unreachable on retail for the two reasons above.
//! - `0x10` `AttractIdle` - the live title menu. Steps the row counter
//!   `_DAT_8007B820` on `Up | Down` (`0x801DDB9C`), wraps it with
//!   `andi v1,v1,0x1`, and confirms on `Start | L1 | Cross`
//!   (`pad & 0x844`, `0x801DDC04`): **row 0 goes straight to `0x16`**
//!   (`0x801DDC3C`, the launch white-out) and row 1 to `0x18`
//!   (`0x801DDC5C`, the CONTINUE fade-in). Its own pre-roll
//!   `state[-0xee0]` must drain before any of that runs.
//! - `0x11` `AttractDelay` - the wait state that precedes `AttractIdle`.
//!   Spends an `8 * frame_scalar` accumulator at `_DAT_8007BAB4`; when it
//!   runs out, writes `state[+0x204] = 0x10` (`0x801DDAC4`) and re-arms
//!   the countdown to `0x5DC`.
//! - `0x14` `MainMenu` - the two-row menu every card screen returns to.
//!   Its row counter is `state[-0xe9c]`, its confirm is `pad & 0x44`, and
//!   it hands to `0x07` (`0x801DE11C`).
//! - `0x07` `CardOpStage` -> `0x15` `CardCheck` -> `0x09` `ScanSetup` ->
//!   `0x0A` `BlockScan` -> `0x0B` `SlotGrid` -> `0x0E` `SlotConfirm` is
//!   the card path; `0x03` `SaveWrite` / `0x17` `SaveResult` and `0x04`
//!   `BlockTransfer` / `0x05` `LoadVerify` / `0x13` `CardOpResult` are its
//!   two commit legs, and `0x0F` `CardOpPrompt` / `0x12` `CardOpRun` the
//!   21-phase operation behind the second prompt (inner JT at
//!   `0x801CF2AC`, indexed by `state[+0x1C0]`).
//! - `0x16` `LaunchFade` and `0x06` `LaunchGame` are the two exits. Both
//!   write master game mode `2`: `0x16` at `0x801DFAFC` on the load route
//!   (`state[-0xea8] == 1`) and `0x06` at `0x801DFC00` on the new-game
//!   route, each clearing `_DAT_8007BB00` as it goes. The single-writer
//!   reading (only `0x801DFC00`) missed the load route.
//!
//! ## The mode graph
//!
//! [`STATE_204_WRITES`] carries all 56 `state[+0x204] = N` stores with the
//! handler each belongs to and the guard in front of it. Three of those
//! stores have a second source, because one handler jumps into the middle
//! of another's body (`0x15` into `0x0A` at `0x801DE838` and into `0x0F` at
//! `0x801DEF2C`; `0x0E` into `0x13` at `0x801DF47C`) - those carry the extra
//! mode in `also_from`, and without them the `0x04`/`0x05`/`0x13` cluster
//! looks unreachable.
//!
//! Six stores live in the shared epilogue and so apply after **any**
//! handler: the identity write-back at `0x801DFE88`, the fault arm to
//! `0x08`, the two `0x02` bypasses, the `0x03`/`0x04` re-routes, and the
//! per-handler cancel target `s3` at `0x801DFFE0`.
//!
//! [`TitleTickState::step`] executes that graph - one arm per sub-mode
//! followed by the epilogue, in retail's order.
//!
//! ## Provenance
//!
//! - JT read out of PROT entry 0899 at file `+0xA2C`
//!   (`0x801CF244 - 0x801CE818`), 25 words, and reproduced by the
//!   captured `overlay_title.bin` window at the same VA.
//! - Handler bodies, guards and state-struct field offsets read off the
//!   disassembly in `ghidra/scripts/funcs/overlay_title_801ddccc.txt`
//!   and re-derived from the entry's own bytes with
//!   `scripts/ghidra-analysis/disasm-overlay-fn.py --base 0x801CE818`.
//!
//! Both globals above are reached by a `lui 0x8008` paired with a
//! **negative** displacement, so the resolved address is one 64 KB page
//! *below* the `lui` immediate: `0x801DD968`'s `lw a0,-0x4500(v0)` is
//! `0x8007BB00`, and `0x801DDAEC`'s `sw v0,-0x454c(a0)` is
//! `0x8007BAB4`. Transcribing such a pair by concatenating its literals
//! yields `0x80084500` / `0x8008454C` - two unrelated live globals in
//! the save-block window, which is why the wrong name reads plausible.
//! Resolve the pair before naming it; the same slip produced the
//! `0x800846A8` / `0x8007B6A8` confusion in the pause-menu save gate.
//!
//! No Sony bytes are stored in this module - the JT entries are PSX
//! virtual addresses (numbers), not extracted overlay contents.
//! REF: FUN_801E38D0

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

/// Title-overlay state struct base. Passed as the first argument (`a0`)
/// to [`SUBMODE_TICK_FN_ENTRY_PC`].
pub const STATE_BASE_ADDR: u32 = 0x801F_0000;

/// `state[-0xeb4]` (= `0x801EF14C`): horizontal slider X position. Both
/// epilogue arms clamp at the **same** value, so it converges on `0x2C` from
/// either side (`slti 0x2c` floor at `0x801DFC88`, `slti 0x2d` ceiling at
/// `0x801DFCB4`) - it is not clamped to a `[0, 0x2C]` range.
pub const STATE_HORIZ_SLIDER_X_ADDR: u32 = 0x801E_F14C;

/// `state[-0xea0]` (= `0x801EF160`): fade / sweep accumulator,
/// clamped `[0, 0x1000]`.
pub const STATE_FADE_SWEEP_ADDR: u32 = 0x801E_F160;

/// `state[-0xe94]` (= `0x801EF16C`): attract countdown (u32). Reset to
/// [`COUNTDOWN_RESET_VALUE`] by [`TitleOverlaySubMode::Init`] and
/// [`TitleOverlaySubMode::AttractDelay`]; the CD-DMA overlay load
/// (FUN_8005D9A0, trigger instruction `0x8005DA4C`) deposits the
/// `0x8000` initial value as disc bytes before the first tick.
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
/// Distinct from the `0x8000` initial value that the CD-DMA overlay
/// load (`FUN_8005D9A0`, trigger `0x8005DA4C`) deposits before the
/// first tick.
pub const COUNTDOWN_RESET_VALUE: u32 = 0x0000_05DC;

/// Master game-mode word `_DAT_8007B83C` (the global mode the 28-mode
/// state machine at `0x8007078C` walks). Title overlay writes to this
/// from two places:
///
/// - The downstream attract-fire path (`SUBMODE_COUNTDOWN_DECR_PC`
///   fall-through, mode-write at `0x801DDCF0`) sets it to
///   [`MASTER_GAME_MODE_STR_INIT`] = `0x1A`.
/// - [`TitleOverlaySubMode::LaunchGame`] sets it to
///   [`MASTER_GAME_MODE_FIELD_LAUNCH`] = `0x02` - the "exit title /
///   launch game" transition (instruction `sh v0, -0x47C4(v1)` at
///   `0x801DFC00`).
pub const MASTER_GAME_MODE_ADDR: u32 = 0x8007_B83C;

/// Master-game-mode value [`crate::cutscene_trigger::STR_INIT_MODE`]
/// re-exported here for use alongside [`MASTER_GAME_MODE_ADDR`] in this
/// module.
pub const MASTER_GAME_MODE_STR_INIT: u8 = 0x1A;

/// Master-game-mode value the [`TitleOverlaySubMode::LaunchGame`]
/// (title-launch) handler writes to [`MASTER_GAME_MODE_ADDR`]. This is
/// the value the engine port consumes to transition out of the title
/// overlay into the main game (field / town).
///
/// `0x02` is the *init* mode ("MAIN INIT"): its handler `FUN_80025B64`
/// loads the field/town overlay and calls the per-scene initializer
/// [`FIELD_SCENE_INIT_PC`], which loads the map + MAN + camera + fog +
/// BGM, allocates the game-mode work buffer, and then hands off to the
/// field per-frame loop by writing [`MASTER_GAME_MODE_FIELD_RUN`] = `3`.
/// So `2` and `3` are the INIT/RUN pair of the field/town gameplay mode,
/// not an options screen.
pub const MASTER_GAME_MODE_FIELD_LAUNCH: u8 = 0x02;

/// Master-game-mode value the field scene initializer
/// ([`FIELD_SCENE_INIT_PC`]) writes once the map is resident, handing off
/// to the field per-frame loop ("MAIN MODE"). The retail write is
/// `_DAT_8007b83c = 3` at the tail of `FUN_801D6704`.
pub const MASTER_GAME_MODE_FIELD_RUN: u8 = 0x03;

/// SCUS handler for master game-mode `0x02` ("MAIN INIT"): loads the
/// field/town overlay via `FUN_8003EBE4(2)` and invokes
/// [`FIELD_SCENE_INIT_PC`].
// REF: FUN_80025B64
// REF: FUN_8003EBE4
pub const MODE2_INIT_HANDLER_PC: u32 = 0x8002_5B64;

/// The field/town scene initializer the NEW GAME launch lands in
/// (`FUN_801D6704`, in the field overlay). Reads the map id from the
/// resident globals, loads geometry + MAN + camera + fog + BGM, allocates
/// the game-mode work buffer, then writes [`MASTER_GAME_MODE_FIELD_RUN`].
/// Used for every field entry, not just NEW GAME; the title path simply
/// routes here via [`MASTER_GAME_MODE_FIELD_LAUNCH`].
// REF: FUN_801D6704
pub const FIELD_SCENE_INIT_PC: u32 = 0x801D_6704;

/// State offset holding the menu row the player confirmed on the title
/// screen (`state[+0x200]`). The confirm handler reads the live cursor
/// (`state[+0x1FC]`), stashes it here, and advances to sub-mode `0x14`;
/// the launch sub-phases then branch on this value.
pub const MENU_INDEX_STATE_OFFSET: u32 = 0x0000_0200;

/// Value of [`MENU_INDEX_STATE_OFFSET`] selecting NEW GAME (the top row).
/// The index-0 sub-phases reach the [`MASTER_GAME_MODE_FIELD_LAUNCH`]
/// write at [`PHASE06_LAUNCH_GAME_PC`]; a non-zero index (CONTINUE) routes
/// to the save/card load path instead.
pub const MENU_INDEX_NEW_GAME: u32 = 0;

/// One `state[+0x204] = value` write inside the tick function. Each row
/// pins the store's instruction address, the sub-mode handler whose body
/// contains it, the value stored, and the guard the disassembly puts in
/// front of it.
///
/// Three store sites sit inside one handler's body but are also reached
/// by another handler's unconditional `j` into the middle of it; those
/// carry the extra source in [`State204Write::also_from`]. The shared
/// epilogue's own stores use [`SUBMODE_TAIL_SOURCE`] as their `from`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct State204Write {
    /// PSX virtual address of the `sw` instruction.
    pub pc: u32,
    /// Sub-mode whose handler body contains this write, or
    /// [`SUBMODE_TAIL_SOURCE`] for the shared epilogue.
    pub from: u8,
    /// Extra sub-modes that reach this same store through a `j` into
    /// another handler's body. Empty for all but three sites.
    pub also_from: &'static [u8],
    /// Either a static value the dispatcher transitions to, or
    /// [`TransitionTarget::Register`] when the source is a register.
    pub target: TransitionTarget,
    /// The condition the disassembly guards this store with, in one line.
    pub guard: &'static str,
}

/// Static-vs-dynamic transition target for [`State204Write`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionTarget {
    /// The write stores a known literal mode value.
    Mode(u8),
    /// One store site whose source register carries one of two literals,
    /// picked by a branch a few instructions earlier.
    OneOf(&'static [u8]),
    /// The write stores a register value (commonly `s3`, the per-handler
    /// cancel target, which is `-1` in the handlers that take no cancel).
    Register(&'static str),
    /// The store writes back what it just read - the shared epilogue's
    /// `submode = FUN_801E1114(submode)`, whose callee returns its
    /// argument on every path. Not a transition.
    Identity,
}

/// `from` marker for the six stores that live in the shared epilogue at
/// [`SUBMODE_BODY_PC`] rather than in one sub-mode's handler body. The
/// epilogue runs after **every** handler, so these apply from any mode.
pub const SUBMODE_TAIL_SOURCE: u8 = 0xFF;

/// Every `state[+0x204] = N` store in the tick function, ordered by PC.
///
/// This is the complete set: the body carries exactly 56 `sw` to
/// `0x204(base)` and all 56 are here, each with the handler it belongs to
/// and its guard. Read off the disassembly of `FUN_801DD35C` (PROT entry
/// 0899 file `+0xEB44`; see `ghidra/scripts/funcs/overlay_title_801ddccc.txt`)
/// and cross-checked against a Capstone pass over the entry's own bytes.
pub const STATE_204_WRITES: &[State204Write] = &[
    State204Write {
        pc: 0x801D_D920,
        from: 0x00,
        also_from: &[],
        target: TransitionTarget::Mode(0x02),
        guard: "unconditional; every retail path overwrites it below",
    },
    State204Write {
        pc: 0x801D_D97C,
        from: 0x00,
        also_from: &[],
        target: TransitionTarget::Mode(0x11),
        guard: "entry word `_DAT_8007BB00 != 0` (lw a0,-0x4500(v0) @0x801DD968)",
    },
    State204Write {
        pc: 0x801D_DA58,
        from: 0x00,
        also_from: &[],
        target: TransitionTarget::Mode(0x14),
        guard: "`param_2 == 1`; also stashes `state[+0x200]=0`, `state[+0x1FC]=0`",
    },
    State204Write {
        pc: 0x801D_DA80,
        from: 0x00,
        also_from: &[],
        target: TransitionTarget::Mode(0x14),
        guard: "`param_2 == 2`; also stashes `state[+0x200]=1`",
    },
    State204Write {
        pc: 0x801D_DAC4,
        from: 0x11,
        also_from: &[],
        target: TransitionTarget::Mode(0x10),
        guard: "`_DAT_8007BAB4 <= 0`; re-arms the countdown to 0x5DC",
    },
    State204Write {
        pc: 0x801D_DC38,
        from: 0x10,
        also_from: &[],
        target: TransitionTarget::Mode(0x00),
        guard: "confirm on row 0; the next instruction stores 0x16 over it",
    },
    State204Write {
        pc: 0x801D_DC3C,
        from: 0x10,
        also_from: &[],
        target: TransitionTarget::Mode(0x16),
        guard: "confirm (`_DAT_8007B874 & 0x844`) on row 0 - NEW GAME",
    },
    State204Write {
        pc: 0x801D_DC5C,
        from: 0x10,
        also_from: &[],
        target: TransitionTarget::Mode(0x18),
        guard: "confirm on row 1 - CONTINUE; stashes `state[+0x200]=1`",
    },
    State204Write {
        pc: 0x801D_DDD8,
        from: 0x18,
        also_from: &[],
        target: TransitionTarget::Mode(0x14),
        guard: "`state[-0xee0]` ramps up by `0x80 * scalar` and reaches 0x1000",
    },
    State204Write {
        pc: 0x801D_DE4C,
        from: 0x02,
        also_from: &[],
        target: TransitionTarget::Mode(0x16),
        guard: "fade-out finished and `param_2 != 0`",
    },
    State204Write {
        pc: 0x801D_DF1C,
        from: 0x02,
        also_from: &[],
        target: TransitionTarget::Mode(0x14),
        guard: "confirm (`& 0x44`); stashes the picked row in `state[+0x200]`",
    },
    State204Write {
        pc: 0x801D_E11C,
        from: 0x14,
        also_from: &[],
        target: TransitionTarget::Mode(0x07),
        guard: "confirm (`& 0x44`) once both alpha ramps have drained",
    },
    State204Write {
        pc: 0x801D_E25C,
        from: 0x07,
        also_from: &[],
        target: TransitionTarget::Mode(0x15),
        guard: "`state[-0xec0] == 0 && state[-0xef4] == 0` (no card op in flight)",
    },
    State204Write {
        pc: 0x801D_E3E8,
        from: 0x15,
        also_from: &[],
        target: TransitionTarget::Mode(0x09),
        guard: "check timer >= 0x259, `state[+0x21C] == 3 && state[+0x224] == 0`",
    },
    State204Write {
        pc: 0x801D_E418,
        from: 0x15,
        also_from: &[],
        target: TransitionTarget::OneOf(&[0x0C, 0x0D]),
        guard: "same arm with `state[+0x1E4] == 0`: 0x0C when `state[+0x200] != 0`, \
                0x0D when it is 0 and `state[+0x1F0] == 0`",
    },
    State204Write {
        pc: 0x801D_E434,
        from: 0x15,
        also_from: &[],
        target: TransitionTarget::Mode(0x08),
        guard: "`state[+0x218] > 0` (a card fault); also sets `state[-0xe58] = 0xA`",
    },
    State204Write {
        pc: 0x801D_E484,
        from: 0x15,
        also_from: &[],
        target: TransitionTarget::Mode(0x0F),
        guard: "`state[+0x1BC] >= 2 && state[+0x200] == 0`",
    },
    State204Write {
        pc: 0x801D_E4A0,
        from: 0x15,
        also_from: &[],
        target: TransitionTarget::Mode(0x0C),
        guard: "`state[+0x1BC] >= 2 && state[+0x200] != 0`",
    },
    State204Write {
        pc: 0x801D_E5D0,
        from: 0x08,
        also_from: &[],
        target: TransitionTarget::Mode(0x07),
        guard: "`state[+0x218] == 0` (the fault cleared); re-arms `state[+0x228] = 0x78`",
    },
    State204Write {
        pc: 0x801D_E624,
        from: 0x08,
        also_from: &[],
        target: TransitionTarget::Mode(0x14),
        guard: "any button (`& 0xF5`); cue 0x20",
    },
    State204Write {
        pc: 0x801D_E654,
        from: 0x09,
        also_from: &[],
        target: TransitionTarget::Mode(0x0A),
        guard: "unconditional - 0x09 is a one-frame setup state",
    },
    State204Write {
        pc: 0x801D_E6DC,
        from: 0x0C,
        also_from: &[],
        target: TransitionTarget::Register("s3 = 0x14"),
        guard: "any button (`& 0xF5`); cue 0x20",
    },
    State204Write {
        pc: 0x801D_E748,
        from: 0x0D,
        also_from: &[],
        target: TransitionTarget::Register("s3 = 0x14"),
        guard: "any button (`& 0xF5`); cue 0x20",
    },
    State204Write {
        pc: 0x801D_E844,
        from: 0x0A,
        also_from: &[0x15],
        target: TransitionTarget::Mode(0x07),
        guard: "any of six abort flags set; also reached by mode 0x15's `j 0x801DE838`",
    },
    State204Write {
        pc: 0x801D_E888,
        from: 0x0A,
        also_from: &[],
        target: TransitionTarget::Mode(0x0C),
        guard: "`state[+0x200] != 0 && state[+0x1E4] == 0`",
    },
    State204Write {
        pc: 0x801D_E934,
        from: 0x0A,
        also_from: &[],
        target: TransitionTarget::Mode(0x0B),
        guard: "block scan complete (`state[+0x1D4] == state[+0x1E4]`)",
    },
    State204Write {
        pc: 0x801D_EB24,
        from: 0x0B,
        also_from: &[],
        target: TransitionTarget::Mode(0x14),
        guard: "chosen cell == 0x80; the 5x3 grid only yields 0..14, so this arm is dead",
    },
    State204Write {
        pc: 0x801D_EC00,
        from: 0x0B,
        also_from: &[],
        target: TransitionTarget::Mode(0x0E),
        guard: "a cell below 15 confirmed and `state[+0x200] == 0`",
    },
    State204Write {
        pc: 0x801D_EC34,
        from: 0x0B,
        also_from: &[],
        target: TransitionTarget::Mode(0x07),
        guard: "`state[-0xeb8] != 0` (the card went away)",
    },
    State204Write {
        pc: 0x801D_ECA4,
        from: 0x0E,
        also_from: &[],
        target: TransitionTarget::Mode(0x0B),
        guard: "cancel (`& 0x21`); cue 0x37",
    },
    State204Write {
        pc: 0x801D_EDF8,
        from: 0x0E,
        also_from: &[],
        target: TransitionTarget::Mode(0x03),
        guard: "confirm with `state[+0x1FC] == 1` and `state[+0x200] == 0` - the SAVE arm",
    },
    State204Write {
        pc: 0x801D_EE00,
        from: 0x0E,
        also_from: &[],
        target: TransitionTarget::Mode(0x0B),
        guard: "confirm with `state[+0x1FC] != 1`",
    },
    State204Write {
        pc: 0x801D_EF28,
        from: 0x0F,
        also_from: &[],
        target: TransitionTarget::Mode(0x12),
        guard: "confirm with `state[+0x1FC] == 1`",
    },
    State204Write {
        pc: 0x801D_EF34,
        from: 0x0F,
        also_from: &[0x15],
        target: TransitionTarget::Mode(0x14),
        guard: "confirm with `state[+0x1FC] != 1`; also reached by mode 0x15's `j 0x801DEF2C`",
    },
    State204Write {
        pc: 0x801D_EFE8,
        from: 0x12,
        also_from: &[],
        target: TransitionTarget::Mode(0x14),
        guard: "inner phase 0, any button (`& 0xF5`)",
    },
    State204Write {
        pc: 0x801D_F2FC,
        from: 0x12,
        also_from: &[],
        target: TransitionTarget::Mode(0x07),
        guard: "inner phase 0x0D, the `state[+0x3288]` hold timer underflows",
    },
    State204Write {
        pc: 0x801D_F364,
        from: 0x04,
        also_from: &[],
        target: TransitionTarget::Mode(0x13),
        guard: "`state[+0x218] > 0` (delay slot - taken on the whole fault arm)",
    },
    State204Write {
        pc: 0x801D_F400,
        from: 0x04,
        also_from: &[],
        target: TransitionTarget::Mode(0x05),
        guard: "`state[+0x1DC] != 0 && state[+0x1D0] == 0x1000`",
    },
    State204Write {
        pc: 0x801D_F484,
        from: 0x13,
        also_from: &[0x0E],
        target: TransitionTarget::Mode(0x04),
        guard: "retry with `state[-0xf08] < 5`; also reached by mode 0x0E's `j 0x801DF47C` \
                (the LOAD entry into the card-op state)",
    },
    State204Write {
        pc: 0x801D_F588,
        from: 0x13,
        also_from: &[],
        target: TransitionTarget::Mode(0x14),
        guard: "any button (`& 0xF5`); cue 0x20",
    },
    State204Write {
        pc: 0x801D_F5FC,
        from: 0x03,
        also_from: &[],
        target: TransitionTarget::Mode(0x17),
        guard: "the save hold timer `state[-0xee8]` reaches 0x4B1",
    },
    State204Write {
        pc: 0x801D_F638,
        from: 0x03,
        also_from: &[],
        target: TransitionTarget::Mode(0x17),
        guard: "`state[-0xef4] != 0` (the write errored)",
    },
    State204Write {
        pc: 0x801D_F68C,
        from: 0x03,
        also_from: &[],
        target: TransitionTarget::Mode(0x17),
        guard: "`state[+0x218] != 0` (a card fault during the write)",
    },
    State204Write {
        pc: 0x801D_F778,
        from: 0x17,
        also_from: &[],
        target: TransitionTarget::Mode(0x03),
        guard: "retry with `state[-0xf08] < 5`",
    },
    State204Write {
        pc: 0x801D_F7A8,
        from: 0x17,
        also_from: &[],
        target: TransitionTarget::Register("s3 = 0x14"),
        guard: "any button (`& 0xF5`); cue 0x20",
    },
    State204Write {
        pc: 0x801D_F8B0,
        from: 0x05,
        also_from: &[],
        target: TransitionTarget::Mode(0x13),
        guard: "the loaded block fails its `slot[0x1FFC] == 1` check",
    },
    State204Write {
        pc: 0x801D_F8BC,
        from: 0x05,
        also_from: &[],
        target: TransitionTarget::Mode(0x16),
        guard: "the block verifies; also sets `state[-0xea8] = 1` (the LOAD banner)",
    },
    State204Write {
        pc: 0x801D_FA88,
        from: 0x16,
        also_from: &[],
        target: TransitionTarget::Mode(0x06),
        guard: "the white-out `state[-0xeac]` reaches 0x1200",
    },
    State204Write {
        pc: 0x801D_FC1C,
        from: 0x06,
        also_from: &[],
        target: TransitionTarget::Mode(0x00),
        guard: "unconditional - the launcher re-arms Init behind the mode-2 hand-off",
    },
    State204Write {
        pc: 0x801D_FE88,
        from: SUBMODE_TAIL_SOURCE,
        also_from: &[],
        target: TransitionTarget::Identity,
        guard: "`state[-0xe74] >= 0`: `FUN_801E1114` returns its argument on every path",
    },
    State204Write {
        pc: 0x801D_FED0,
        from: SUBMODE_TAIL_SOURCE,
        also_from: &[],
        target: TransitionTarget::Mode(0x08),
        guard: "`s5 != 0 && state[+0x218] > 0 && state[+0x228] == 0`",
    },
    State204Write {
        pc: 0x801D_FEF8,
        from: SUBMODE_TAIL_SOURCE,
        also_from: &[],
        target: TransitionTarget::Mode(0x10),
        guard: "`_DAT_8007BB00 != 0 && submode == 2` - the second place 0x02 is bypassed",
    },
    State204Write {
        pc: 0x801D_FF28,
        from: SUBMODE_TAIL_SOURCE,
        also_from: &[],
        target: TransitionTarget::Mode(0x16),
        guard: "`param_2 != 0 && submode == 2`",
    },
    State204Write {
        pc: 0x801D_FF74,
        from: SUBMODE_TAIL_SOURCE,
        also_from: &[],
        target: TransitionTarget::OneOf(&[0x16, 0x17]),
        guard: "`state[+0x329C] == 0 && submode == 3`: 0x17 when `state[-0xec4] != 0`, else 0x16",
    },
    State204Write {
        pc: 0x801D_FF9C,
        from: SUBMODE_TAIL_SOURCE,
        also_from: &[],
        target: TransitionTarget::Mode(0x13),
        guard: "`state[+0x329C] == 0 && submode == 4 && state[-0xec0] != 0`",
    },
    State204Write {
        pc: 0x801D_FFE0,
        from: SUBMODE_TAIL_SOURCE,
        also_from: &[],
        target: TransitionTarget::Register("s3"),
        guard: "cancel (`& 0x21`) with `s3 >= 0` and submode not 3 or 4; cue 0x37",
    },
];

/// PSX virtual address of the `sh v0, -0x47C4(v1)` instruction inside
/// [`TitleOverlaySubMode::LaunchGame`] that writes
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

// ---------------------------------------------------------------------------
// The executable half: the title menu's input + attract law
// ---------------------------------------------------------------------------

/// D-pad bit that steps the title cursor **forward** one row - `Down` in the
/// repacked pad word (`andi v0,a2,0x4000` at `0x801DDBC0`).
pub const PADMASK_CURSOR_NEXT: u16 = 0x4000;

/// D-pad bit that steps it **back** one row - `Up` (`andi v0,a2,0x1000` at
/// `0x801DDB9C`).
pub const PADMASK_CURSOR_PREV: u16 = 0x1000;

/// SFX cue the tick stores on a cursor move (`li v1,0x21` /
/// `sh v1,-0x4928(a1)` at `0x801DDBB0`, i.e. `0x8007B6D8`).
pub const TITLE_SFX_CURSOR_MOVE: u16 = 0x21;

/// SFX cue it stores on a confirm (`li v1,0x20` at `0x801DDC20` /
/// `li v0,0x20` at `0x801DDC4C`, same halfword).
pub const TITLE_SFX_CONFIRM: u16 = 0x20;

/// Rows the title menu carries. Retail wraps the row counter with
/// `andi v1,v1,0x1` at `0x801DDC00`, which is a two-row space.
pub const TITLE_MENU_ROWS: u8 = 2;

/// Row `0` of the title menu - NEW GAME. Confirming it writes master game
/// mode [`MASTER_GAME_MODE_FIELD_LAUNCH`] further down the graph.
pub const TITLE_ROW_NEW_GAME: u8 = 0;

/// Row `1` - CONTINUE. Confirming it stashes `1` in `state[+0x200]`
/// (`0x801DDC64`) and routes to sub-mode `0x18`.
pub const TITLE_ROW_CONTINUE: u8 = 1;

/// Below this countdown value the tick stops reading the pad at all:
/// `slti v0,a0,0x11` / `bne v0,zero,0x801DDC94` at `0x801DDB84` jumps past
/// the whole cursor + confirm block. The last sixteen frames before the
/// attract fires accept no input.
pub const ATTRACT_INPUT_FREEZE_BELOW: i32 = 0x11;

/// What one [`TitleMenuState::step`] did, in the order retail does it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleMenuEvent {
    /// The cursor moved to `row`; retail cues [`TITLE_SFX_CURSOR_MOVE`].
    CursorMoved { row: u8 },
    /// `row` was confirmed off the `Start | L1 | Cross` mask
    /// ([`PADMASK_START_L1_CROSS`]); retail cues [`TITLE_SFX_CONFIRM`].
    Confirmed { row: u8 },
    /// The attract countdown underflowed. Retail's arm zeroes
    /// `0x8007BA78` and writes [`MASTER_GAME_MODE_STR_INIT`] to
    /// [`MASTER_GAME_MODE_ADDR`], i.e. hands the screen to the opening
    /// movie.
    AttractFired,
}

/// The mutable half of the title tick's `AttractIdle` (`0x10`) state - the
/// row counter, the attract countdown, and the row a confirm stashed.
///
/// The field names are retail's globals: `row_counter` is `_DAT_8007B820`
/// (`lw v0,-0x47e0(a0)` at `0x801DDBAC`), `countdown` is `0x801EF16C`
/// (`lw v0,-0xe94(a0)` at `0x801DDCBC`), `chosen_row` is `state[+0x200]`.
///
/// PORT: FUN_801DD35C (`0x801DDB74..0x801DDCF4`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TitleMenuState {
    /// Free-running row counter. The displayed row is this value wrapped
    /// into the row space; retail writes the wrapped value straight back.
    pub row_counter: i32,
    /// Frames left before the attract movie takes the screen.
    pub countdown: i32,
    /// The row the last confirm stashed (`state[+0x200]`).
    pub chosen_row: u8,
    /// The SFX cue the last step stored, if any (`0x8007B6D8`).
    pub sfx: Option<u16>,
}

impl Default for TitleMenuState {
    fn default() -> Self {
        Self::new()
    }
}

impl TitleMenuState {
    /// A freshly entered title menu: cursor on row 0, countdown at
    /// [`COUNTDOWN_RESET_VALUE`] (`li v0,0x5dc` at `0x801DDACC`).
    pub fn new() -> Self {
        Self {
            row_counter: 0,
            countdown: COUNTDOWN_RESET_VALUE as i32,
            chosen_row: 0,
            sfx: None,
        }
    }

    /// The row the cursor is on - retail's `andi v1,v1,0x1` at
    /// `0x801DDC00` generalised to `rows` (which is `2` in retail, where
    /// the mask and the modulo agree).
    pub fn row(&self, rows: u8) -> u8 {
        let n = rows.max(1) as i32;
        self.row_counter.rem_euclid(n) as u8
    }

    /// One frame of the menu state, in retail's order:
    ///
    /// 1. Skip the whole input block while the countdown is below
    ///    [`ATTRACT_INPUT_FREEZE_BELOW`] (`0x801DDB84`).
    /// 2. `Down` / `Up` step the row counter and cue
    ///    [`TITLE_SFX_CURSOR_MOVE`] (`0x801DDB9C..0x801DDBE0`).
    /// 3. Wrap the counter and store it back (`0x801DDC00`).
    /// 4. `Start | L1 | Cross` confirms the wrapped row, cues
    ///    [`TITLE_SFX_CONFIRM`], and stashes it (`0x801DDC04..0x801DDC70`).
    /// 5. Any held pad bit re-arms the countdown (`0x801DDC74`), then the
    ///    countdown drops by the frame scalar and fires on underflow
    ///    (`0x801DDCB0..0x801DDCF4`).
    ///
    /// `pad_edge` is the just-pressed word the cursor and confirm read;
    /// `pad_held` is the held word retail re-arms the countdown from
    /// (`_DAT_8007B850`). `frame_scalar` is the scratchpad byte at
    /// `0x1F800393`, `1` on a normal frame.
    ///
    /// PORT: FUN_801DD35C (`0x801DDB74..0x801DDCF4`)
    pub fn step(
        &mut self,
        pad_edge: u16,
        pad_held: u16,
        frame_scalar: u8,
        rows: u8,
    ) -> Vec<TitleMenuEvent> {
        let mut events = Vec::new();
        self.sfx = None;
        let n = rows.max(1) as i32;
        if self.countdown >= ATTRACT_INPUT_FREEZE_BELOW {
            let before = self.row_counter.rem_euclid(n);
            if pad_edge & PADMASK_CURSOR_NEXT != 0 {
                self.row_counter = self.row_counter.wrapping_add(1);
                self.sfx = Some(TITLE_SFX_CURSOR_MOVE);
            }
            if pad_edge & PADMASK_CURSOR_PREV != 0 {
                self.row_counter = self.row_counter.wrapping_sub(1);
                self.sfx = Some(TITLE_SFX_CURSOR_MOVE);
            }
            // Retail wraps the counter and writes the wrapped value back,
            // so the counter never runs away from the row space.
            self.row_counter = self.row_counter.rem_euclid(n);
            if self.row_counter != before {
                events.push(TitleMenuEvent::CursorMoved {
                    row: self.row_counter as u8,
                });
            }
            if pad_edge & PADMASK_START_L1_CROSS != 0 {
                let row = self.row_counter as u8;
                self.chosen_row = row;
                self.sfx = Some(TITLE_SFX_CONFIRM);
                events.push(TitleMenuEvent::Confirmed { row });
                return events;
            }
        }
        if pad_held != 0 {
            self.countdown = COUNTDOWN_RESET_VALUE as i32;
        }
        self.countdown -= frame_scalar as i32;
        if self.countdown < 0 {
            events.push(TitleMenuEvent::AttractFired);
        }
        events
    }
}

// ---------------------------------------------------------------------------
// The whole dispatcher: one `step` per sub-mode over the tick's state
// ---------------------------------------------------------------------------

/// Instruction address of the `sw v0,0x204(a2)` in `Init` that stores the
/// sentinel sub-mode `0x11`. The **word** it writes is
/// `STATE_BASE_ADDR + STATE_SUBMODE_OFFSET`; this constant is the store,
/// not the datum.
pub const INIT_SENTINEL_STORE_PC: u32 = 0x801D_D97C;

/// Instruction address of the `sw v0,0x204(a2)` in `Init` that stores the
/// default sub-mode `0x02`, before the sentinel arm overwrites it.
pub const INIT_DEFAULT_STORE_PC: u32 = 0x801D_D920;

/// Address of the boot entry word retail's `init.pak` raises before the
/// title overlay's first tick (`FUN_801CE9C0`, `sw s2,-0x4500(s0)` at
/// `0x801CEB84` with `s0 = 0x80080000`). `Init` reads it at `0x801DD968`
/// and the shared epilogue reads it again at `0x801DFED8`; both send the
/// tick past sub-mode `0x02`.
pub const ENTRY_WORD_ADDR: u32 = 0x8007_BB00;

/// The value `init.pak` leaves in [`ENTRY_WORD_ADDR`] on a cold boot.
/// `Init` takes the `0x11` arm on any non-zero value, and additionally
/// streams the title assets when it reads exactly this.
pub const ENTRY_WORD_COLD_BOOT: u32 = 1;

/// The value the word carries when the title is re-entered from the
/// attract movie (capture; see `docs/subsystems/boot.md`). Still non-zero,
/// so the `0x11` arm holds, but the asset stream is skipped.
pub const ENTRY_WORD_FROM_ATTRACT: u32 = 2;

/// `_DAT_8007BAB4` - the pre-attract hold `AttractDelay` (`0x11`) spends
/// at `8 * frame_scalar` per frame before handing to `AttractIdle`.
///
/// The tick never seeds it; the SCUS routine that stages the title
/// overlay does, at `0x8002579C` (`addiu s0,zero,0x100` /
/// `sw s0,0x79c(gp)`, `gp = 0x8007B318`). That is its only writer outside
/// this function - `find-gp-relative-refs.py --va 0x8007BAB4` finds six
/// references across 84 images and five of them are the tick's own.
pub const ATTRACT_DELAY_ADDR: u32 = 0x8007_BAB4;

/// The value the SCUS title-overlay stager leaves in
/// [`ATTRACT_DELAY_ADDR`]. `AttractDelay` spends it at 8 a frame and the
/// hand-off fires on the frame that reads it already at zero (`bgtz v1`
/// at `0x801DDAB4`), so the hold is `0x100 / 8 + 1` = 33 frames at a
/// normal frame scalar.
pub const ATTRACT_DELAY_SEED: i32 = 0x100;

/// `_DAT_8007B820` - the free-running title row counter `AttractIdle`
/// steps and wraps with `andi v1,v1,0x1`.
pub const TITLE_ROW_COUNTER_ADDR: u32 = 0x8007_B820;

/// `state[-0xee0]` (= `0x801EF120`): the `AttractIdle` pre-roll. Counts
/// **down** by `0x80 * scalar` while `0x10` is still animating in; mode
/// `0x18` counts the same word **up** to `0x1000` and then hands to
/// `0x14`.
pub const STATE_PREROLL_ADDR: u32 = 0x801E_F120;

/// SFX cue the shared epilogue stores on a cancel (`li v0,0x37` at
/// `0x801DFFD8`).
pub const TITLE_SFX_CANCEL: u16 = 0x37;

/// Value `state[-0xea8]` (`0x801EF158`) carries on the CONTINUE / load
/// route; mode `0x16` reads it to pick the LOAD banner **and** to gate the
/// master-mode hand-off at [`PHASE16_LOAD_LAUNCH_PC`].
pub const OP_KIND_LOAD: u32 = 1;

/// PSX virtual address of the `sh s0,-0x47C4(v0)` inside sub-mode `0x16`
/// that writes [`MASTER_GAME_MODE_FIELD_LAUNCH`] on the **load** route,
/// before `0x16` hands on to `0x06`. The tick therefore has two
/// master-mode-2 writers, not one: this and
/// [`PHASE06_LAUNCH_GAME_PC`].
pub const PHASE16_LOAD_LAUNCH_PC: u32 = 0x801D_FAFC;

/// Reads the tick makes of state the memory-card / disc half of the
/// front-end owns. Every field names the retail address the guard reads;
/// a cold boot that never opens a card screen leaves all of them at their
/// [`Default`] zero, which is exactly what the retail globals hold there.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TitleCardStatus {
    /// `state[-0xec0]` (`0x801EF140`): a card operation is in flight.
    pub busy: i32,
    /// `state[-0xef4]` (`0x801EF10C`): the last operation errored.
    pub error: i32,
    /// `state[-0xec4]` (`0x801EF13C`): the last operation's result code.
    pub result: i32,
    /// `state[-0xebc]` (`0x801EF144`): the retry latch.
    pub retry_latch: i32,
    /// `state[-0xf08]` (`0x801EF0F8`): retries spent (retail gives up at 5).
    pub retries: i32,
    /// `state[-0xeb8]` (`0x801EF148`): the card was pulled.
    pub removed: i32,
    /// `state[+0x218]`: a card fault the screens surface.
    pub fault: i32,
    /// `state[+0x1E4]`: blocks the scan expects.
    pub scan_total: i32,
    /// `state[+0x1D4]`: blocks the scan has walked.
    pub scan_done: i32,
    /// `state[+0x21C]`: the mounted card's kind (`3` = a formatted card).
    pub slot_kind: i32,
    /// `state[+0x224]`: a scan request is still pending.
    pub scan_request: i32,
    /// `state[+0x1BC]`: block count the header reports.
    pub block_count: i32,
    /// `state[+0x1F0]`: the card holds at least one of our blocks.
    pub has_blocks: i32,
    /// `state[+0x1DC]`: the staged operation is ready to commit.
    pub ready: i32,
    /// `state[+0x329C]`: the operation-status word the epilogue reads.
    pub op_status: i32,
    /// `state[+0x1D0]`: the block-transfer progress bar's target.
    pub draw_done: i32,
    /// `state[+0x228]`: the grace countdown `0x07` seeds with `0x78`.
    pub grace: i32,
    /// `state[+0x1C0]`: the sub-dispatch phase of mode `0x12` (21-entry
    /// inner JT at `0x801CF2AC`).
    pub op_phase: i32,
    /// `state[+0x3288]`: mode `0x12`'s inner phase-`0x0D` hold timer.
    pub op_timer: i32,
    /// The picked grid cell passed its `FUN_801E3F74` status check.
    pub slot_ok: bool,
    /// The loaded block passed mode `0x05`'s `slot[0x1FFC] == 1` check.
    pub verify_ok: bool,
    /// `state[-0xf00]` (`0x801EF100`): a card read is still outstanding;
    /// one of the six flags mode `0x0A` aborts on.
    pub busy_gate: i32,
}

/// The pad + frame inputs one tick reads. Three different pad words feed
/// three different arms, and conflating them is the shape that made the
/// first pass of this port accept a cursor step from the wrong source:
///
/// - `edge` is `_DAT_8007B874`, what the **confirm** and **cancel** masks
///   test;
/// - `nav` is `state[-0xeec]` (`0x801EF114`), which the preamble computes
///   as `_DAT_8007B874 | _DAT_8007B93C` at `0x801DD408` and which the
///   **cursor** arms test;
/// - `held` is `_DAT_8007B850`, which re-arms the attract countdown and
///   suppresses cursor auto-repeat in the epilogue.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TitleTickPad {
    /// `_DAT_8007B874`.
    pub edge: u16,
    /// `state[-0xeec]` = `_DAT_8007B874 | _DAT_8007B93C`.
    pub nav: u16,
    /// `_DAT_8007B850`.
    pub held: u16,
    /// The scratchpad frame scalar at `0x1F800393` (`1` on a normal frame).
    pub frame_scalar: u8,
}

impl TitleTickPad {
    /// A frame with one pad word driving every arm, which is what a host
    /// with a single edge-triggered pad has.
    pub fn from_edge(edge: u16) -> Self {
        Self {
            edge,
            nav: edge,
            held: 0,
            frame_scalar: 1,
        }
    }
}

/// What one [`TitleTickState::step`] did that a host has to act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleTickEffect {
    /// A cue was stored to `0x8007B6D8`.
    Sfx(u16),
    /// `Init` read [`ENTRY_WORD_COLD_BOOT`] and streamed the title assets
    /// (`FUN_8003EB98(0x37C, ...)` / `FUN_8003E6BC`).
    LoadTitleAssets,
    /// The attract countdown underflowed inside `AttractIdle`: retail
    /// zeroes `_DAT_8007BA78` (fmv id) and writes
    /// [`MASTER_GAME_MODE_STR_INIT`] to [`MASTER_GAME_MODE_ADDR`].
    FireAttract { fmv_id: i16 },
    /// A master-mode `0x02` hand-off. `from_load` distinguishes mode
    /// `0x16`'s load route ([`PHASE16_LOAD_LAUNCH_PC`]) from mode `0x06`'s
    /// new-game route ([`PHASE06_LAUNCH_GAME_PC`]).
    LaunchGame { from_load: bool },
}

/// The tick's own state - the fields the 25 handlers and the shared
/// epilogue read and write to move the mode graph. Field names carry the
/// retail address in their doc line; the memory-card half of the graph
/// arrives through [`TitleCardStatus`] instead, because the card driver
/// owns those words and the tick only reads them.
///
/// PORT: FUN_801DD35C
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TitleTickState {
    /// `state[+0x204]` - the sub-mode selector.
    pub submode: u8,
    /// `state[+0x208]` - the sub-mode the previous frame ran, which the
    /// preamble compares against to log a mode change.
    pub prev_submode: u8,
    /// `param_2` (`a1`/`s8`). The production caller `FUN_801E36A0` passes
    /// `0`; `1` / `2` are the re-entry arms that pre-select a menu row.
    pub arg1: u32,
    /// [`ENTRY_WORD_ADDR`].
    pub entry_word: u32,
    /// `state[+0x200]` - the row a confirm stashed.
    pub menu_index: u32,
    /// `state[+0x1FC]` - the linear cursor the epilogue steps.
    pub linear_cursor: i32,
    /// `state[+0x1F4]` / `state[+0x1F8]` - the 5x3 grid cursor.
    pub cursor_x: i32,
    /// See [`TitleTickState::cursor_x`].
    pub cursor_y: i32,
    /// [`TITLE_ROW_COUNTER_ADDR`].
    pub row_counter: i32,
    /// [`STATE_ATTRACT_COUNTDOWN_ADDR`].
    pub countdown: i32,
    /// [`ATTRACT_DELAY_ADDR`].
    pub attract_delay: i32,
    /// [`STATE_PREROLL_ADDR`].
    pub preroll: i32,
    /// `state[-0xeac]` (`0x801EF154`) - the screen-level fade the launch
    /// path ramps to `0x1200` and the menu states drain to `0`.
    pub fade: i32,
    /// `state[-0xe5c]` (`0x801EF1A4`) - the slot-panel fade modes `0x0B`
    /// and `0x0E` cross between.
    pub panel_fade: i32,
    /// [`STATE_HORIZ_SLIDER_X_ADDR`].
    pub slider_x: i32,
    /// `state[+0x1E0]` - `1` slides the panel left, `2` right, else idle.
    pub slider_dir: u32,
    /// `state[-0xea8]` (`0x801EF158`) - which banner / route mode `0x16`
    /// is running ([`OP_KIND_LOAD`] on the CONTINUE route).
    pub op_kind: u32,
    /// `state[-0xee8]` (`0x801EF118`) - mode `0x03`'s save hold timer.
    pub save_timer: i32,
    /// `state[-0xedc]` (`0x801EF124`) - mode `0x15`'s card-check timer.
    pub check_timer: i32,
    /// `state[-0xefc]` (`0x801EF104`) - mode `0x16`'s banner timer.
    pub launch_timer: i32,
    /// `state[-0xe74]` (`0x801EF18C`) - when `>= 0` the epilogue runs the
    /// `FUN_801E1114` model pass. Negative on every handler that does not
    /// want it.
    pub remap_gate: i32,
    /// The per-frame cancel target register `s3`. `None` is retail's
    /// `-1` (no cancel arm this frame).
    pub cancel_target: Option<u8>,
    /// The per-frame `s5` flag the epilogue's fault arm tests.
    pub fault_arm: bool,
    /// The per-frame `s6` cursor-kind register: `1` = the epilogue steps
    /// the linear cursor, `2` = it steps the 5x3 grid, else neither.
    pub cursor_kind: u8,
    /// The per-frame `s7` row count the linear cursor wraps against.
    pub rows: i32,
}

impl Default for TitleTickState {
    fn default() -> Self {
        Self::cold_boot()
    }
}

impl TitleTickState {
    /// The state the title overlay's first tick sees on a **cold boot**.
    ///
    /// The entry word is raised, not the sub-mode: retail reaches sub-mode
    /// `0x10` by running `Init` with [`ENTRY_WORD_COLD_BOOT`] in
    /// [`ENTRY_WORD_ADDR`], which is what `init.pak` leaves there
    /// (`FUN_801CE9C0` at `0x801CEB84`). Hard-coding the mode instead
    /// skips the `0x11` `AttractDelay` leg the capture sees.
    pub fn cold_boot() -> Self {
        Self::with_entry_word(ENTRY_WORD_COLD_BOOT)
    }

    /// The same state with an explicit [`ENTRY_WORD_ADDR`] value - `0`
    /// reproduces the (retail-unreachable) `0x02` graph, `2` the return
    /// from the attract movie.
    pub fn with_entry_word(entry_word: u32) -> Self {
        Self {
            submode: TitleOverlaySubMode::Init as u8,
            prev_submode: TitleOverlaySubMode::Init as u8,
            arg1: 0,
            entry_word,
            menu_index: 0,
            linear_cursor: 0,
            cursor_x: 0,
            cursor_y: 0,
            row_counter: 0,
            countdown: COUNTDOWN_RESET_VALUE as i32,
            attract_delay: ATTRACT_DELAY_SEED,
            preroll: 0,
            fade: 0,
            panel_fade: 0,
            slider_x: 0x100,
            slider_dir: 0,
            op_kind: 0,
            save_timer: 0,
            check_timer: 0,
            launch_timer: 0,
            remap_gate: -1,
            cancel_target: None,
            fault_arm: false,
            cursor_kind: 0,
            rows: 0,
        }
    }

    /// The decoded sub-mode, or `None` when the selector is outside the
    /// dispatcher's `sltiu v0,s2,0x19` window (which routes to the tail).
    pub fn mode(&self) -> Option<TitleOverlaySubMode> {
        TitleOverlaySubMode::from_u8(self.submode)
    }

    /// One whole tick: the per-sub-mode handler followed by the shared
    /// epilogue, in retail's order.
    ///
    /// PORT: FUN_801DD35C
    pub fn step(&mut self, pad: TitleTickPad, card: TitleCardStatus) -> Vec<TitleTickEffect> {
        let mut fx = Vec::new();
        // Preamble: the mode-change edge the tick logs, then the per-frame
        // registers every handler re-seeds.
        self.prev_submode = self.submode;
        // The preamble's own register seeds, which a handler only changes
        // if it wants to: `li s3,0x14` at 0x801DD5BC (the default cancel
        // target is the main menu, not "no cancel"), `li s5,0x1` at
        // 0x801DD5E8 (the fault arm is ON unless a handler clears it),
        // `clear s6` at 0x801DD374 and `li s7,-0x1` at 0x801DD610.
        self.cancel_target = Some(0x14);
        self.fault_arm = true;
        self.cursor_kind = 0;
        self.rows = -1;
        let scalar = pad.frame_scalar.max(1) as i32;
        if self.handler(pad, card, scalar, &mut fx) {
            self.epilogue(pad, card, &mut fx);
        }
        fx
    }

    /// The dispatched half - one arm per JT entry. Returns whether the
    /// shared epilogue runs afterwards: two arms jump straight to the
    /// function's exit at `0x801E0274` instead of falling into it - the
    /// `AttractIdle` row-0 confirm (`0x801DDC44`) and the launcher's tail
    /// (`0x801DFB54`).
    fn handler(
        &mut self,
        pad: TitleTickPad,
        card: TitleCardStatus,
        scalar: i32,
        fx: &mut Vec<TitleTickEffect>,
    ) -> bool {
        let any = pad.edge & PADMASK_ANY_FACE_OR_L != 0;
        let confirm = pad.edge & PADMASK_CONFIRM_L1_CROSS != 0;
        let cancel = pad.edge & PADMASK_CANCEL_L2_CIRCLE != 0;
        match self.submode {
            // 0x00 Init - 0x801DD820.
            0x00 => {
                self.fault_arm = false;
                self.slider_x = 0x100;
                self.fade = 0x1000;
                self.slider_dir = 0;
                self.linear_cursor = 0;
                self.remap_gate = -1;
                self.submode = 0x02;
                self.countdown = COUNTDOWN_RESET_VALUE as i32;
                let mut arg1 = self.arg1;
                if self.entry_word != 0 {
                    self.submode = 0x11;
                    if self.entry_word == ENTRY_WORD_COLD_BOOT {
                        arg1 = 0;
                        fx.push(TitleTickEffect::LoadTitleAssets);
                    }
                }
                if arg1 == 1 {
                    self.menu_index = 0;
                    self.linear_cursor = 0;
                    self.submode = 0x14;
                } else if arg1 == 2 {
                    self.menu_index = 1;
                    self.linear_cursor = 0;
                    self.submode = 0x14;
                }
            }
            // 0x01 Idle - the JT entry is the epilogue itself.
            0x01 => {}
            // 0x02 - the (retail-unreachable) two-row text menu, 0x801DDDFC.
            0x02 => {
                self.fault_arm = false;
                self.remap_gate = -1;
                self.fade -= 0x100 * scalar;
                if self.fade > 0 {
                    self.cancel_target = None;
                    return true;
                }
                self.fade = 0;
                self.slider_x = 0x100;
                if self.arg1 != 0 {
                    self.submode = 0x16;
                    return true;
                }
                self.rows = 2;
                self.cursor_kind = 1;
                self.slider_dir = 0;
                self.cancel_target = Some(0x16);
                if confirm {
                    self.menu_index = self.linear_cursor as u32;
                    self.linear_cursor = 0;
                    self.submode = 0x14;
                }
            }
            // 0x03 - SAVE in progress, 0x801DF5BC.
            0x03 => {
                self.cancel_target = None;
                self.save_timer += scalar;
                if self.save_timer >= 0x4B1 || card.error != 0 || card.fault != 0 {
                    self.submode = 0x17;
                }
            }
            // 0x04 - block transfer, 0x801DF33C.
            0x04 => {
                self.cancel_target = None;
                if card.fault > 0 {
                    self.submode = 0x13;
                } else {
                    self.fault_arm = false;
                    if card.ready != 0 && card.draw_done == 0x1000 {
                        self.submode = 0x05;
                    }
                }
            }
            // 0x05 - post-load verify, 0x801DF82C.
            0x05 => {
                self.cancel_target = None;
                self.fault_arm = false;
                if card.verify_ok {
                    self.submode = 0x16;
                    self.op_kind = OP_KIND_LOAD;
                } else {
                    self.submode = 0x13;
                }
            }
            // 0x06 - the NEW GAME launcher, 0x801DFB5C.
            0x06 => {
                if self.entry_word != 0 {
                    fx.push(TitleTickEffect::LaunchGame { from_load: false });
                    self.entry_word = 0;
                }
                self.submode = 0x00;
                // `j 0x801E0274` at 0x801DFB54: the launcher returns 1 and
                // never reaches the epilogue.
                return false;
            }
            // 0x07 - card-op staging, 0x801DE134.
            0x07 => {
                self.cancel_target = None;
                if card.busy != 0 || card.error != 0 {
                    self.fault_arm = true;
                    return true;
                }
                self.fault_arm = false;
                self.slider_dir = 1;
                self.linear_cursor = 0;
                self.check_timer = 0;
                self.submode = 0x15;
            }
            // 0x08 - the card-fault message, 0x801DE4A4.
            0x08 => {
                self.cancel_target = None;
                if card.fault == 0 {
                    self.submode = 0x07;
                }
                // Sequential, not exclusive: the any-button arm at
                // 0x801DE624 runs after the fault-cleared arm above.
                if any {
                    self.fault_arm = false;
                    self.submode = 0x14;
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
                }
            }
            // 0x09 - one-frame scan setup, 0x801DE638.
            0x09 => {
                self.cancel_target = None;
                self.fault_arm = false;
                self.submode = 0x0A;
            }
            // 0x0A - the block scan, 0x801DE798.
            0x0A => {
                self.cancel_target = None;
                if card.busy != 0
                    || card.fault != 0
                    || card.busy_gate != 0
                    || card.block_count != 0
                    || card.error != 0
                    || card.removed != 0
                {
                    self.submode = 0x07;
                    return true;
                }
                // (`state[-0xea4] = 1` here is the one-shot fade-direction
                // request the tick's preamble consumes; it moves no mode.)
                if self.menu_index != 0 && card.scan_total == 0 {
                    self.submode = 0x0C;
                }
                // Falls through: the scan-complete arm at 0x801DE934 runs
                // after the 0x0C arm, not instead of it.
                if card.scan_done == card.scan_total {
                    self.submode = 0x0B;
                }
            }
            // 0x0B - the 5x3 slot grid, 0x801DEA5C.
            0x0B => {
                self.panel_fade -= 0x100 * scalar;
                if self.panel_fade > 0 {
                    self.cancel_target = None;
                    return true;
                }
                self.panel_fade = 0;
                self.cursor_kind = 2;
                self.slider_dir = 2;
                if confirm && card.slot_ok {
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
                    if self.menu_index == 0 {
                        self.linear_cursor = 0;
                        self.submode = 0x0E;
                    }
                }
                if card.removed != 0 {
                    self.submode = 0x07;
                }
            }
            // 0x0C / 0x0D - the two "no data" messages, 0x801DE680 / 0x801DE728.
            0x0C | 0x0D => {
                self.cancel_target = Some(0x14);
                if any {
                    self.submode = 0x14;
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
                }
            }
            // 0x0E - the slot confirm prompt, 0x801DEC40.
            0x0E => {
                self.slider_dir = 0;
                self.panel_fade_up(scalar);
                if self.panel_fade < 0x1000 {
                    self.cancel_target = None;
                    return true;
                }
                self.panel_fade = 0x1000;
                self.cancel_target = None;
                if cancel {
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CANCEL));
                    self.submode = 0x0B;
                    self.slider_dir = 2;
                    return true;
                }
                self.slider_x = -0x16;
                self.rows = 2;
                self.cursor_kind = 1;
                if !confirm {
                    return true;
                }
                if self.linear_cursor != 1 {
                    self.submode = 0x0B;
                    self.slider_dir = 2;
                } else if self.menu_index != 0 {
                    // The LOAD route jumps into 0x13's body at 0x801DF47C.
                    self.submode = 0x04;
                } else {
                    self.submode = 0x03;
                }
            }
            // 0x0F - the format / overwrite prompt, 0x801DEE0C.
            0x0F => {
                self.rows = 2;
                self.cursor_kind = 1;
                if confirm {
                    self.submode = if self.linear_cursor == 1 { 0x12 } else { 0x14 };
                }
            }
            // 0x10 AttractIdle - 0x801DDB0C.
            0x10 => {
                self.cancel_target = None;
                self.fault_arm = false;
                self.remap_gate = -1;
                self.fade = 0;
                self.preroll -= 0x80 * scalar;
                if self.preroll >= 0 {
                    return true;
                }
                self.preroll = 0;
                self.cursor_kind = 1;
                if self.countdown >= ATTRACT_INPUT_FREEZE_BELOW {
                    if pad.nav & PADMASK_CURSOR_NEXT != 0 {
                        self.row_counter = self.row_counter.wrapping_add(1);
                        fx.push(TitleTickEffect::Sfx(TITLE_SFX_CURSOR_MOVE));
                    }
                    if pad.nav & PADMASK_CURSOR_PREV != 0 {
                        self.row_counter = self.row_counter.wrapping_sub(1);
                        fx.push(TitleTickEffect::Sfx(TITLE_SFX_CURSOR_MOVE));
                    }
                    self.row_counter &= 1;
                    if pad.edge & PADMASK_START_L1_CROSS != 0 {
                        fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
                        if self.row_counter == TITLE_ROW_NEW_GAME as i32 {
                            // `j 0x801E0274` at 0x801DDC44 - straight to the
                            // function exit, past the shared epilogue.
                            self.submode = 0x16;
                            return false;
                        }
                        self.submode = 0x18;
                        self.menu_index = TITLE_ROW_CONTINUE as u32;
                        self.preroll = 0;
                        self.linear_cursor = 0;
                    }
                    if pad.held != 0 {
                        self.countdown = COUNTDOWN_RESET_VALUE as i32;
                    }
                }
                self.countdown -= scalar;
                if self.countdown < 0 {
                    fx.push(TitleTickEffect::FireAttract {
                        fmv_id: ATTRACT_FMV_ID,
                    });
                }
            }
            // 0x11 AttractDelay - 0x801DDA90.
            0x11 => {
                self.cancel_target = None;
                self.fault_arm = false;
                self.remap_gate = -1;
                self.fade = 0;
                if self.attract_delay > 0 {
                    self.attract_delay -= 8 * scalar;
                } else {
                    self.submode = 0x10;
                    self.countdown = COUNTDOWN_RESET_VALUE as i32;
                }
                if self.attract_delay < 0 {
                    self.attract_delay = 0;
                }
            }
            // 0x12 - the card-op sub-dispatcher, 0x801DEF38.
            0x12 => {
                self.cancel_target = None;
                self.fault_arm = false;
                if card.op_phase == 0 && any {
                    self.submode = 0x14;
                } else if card.op_phase == 0x0D && card.op_timer - scalar < 0 {
                    self.submode = 0x07;
                }
            }
            // 0x13 - the card-op result screen, 0x801DF404.
            0x13 => {
                self.cancel_target = None;
                self.fault_arm = false;
                if card.busy == 1 && card.retry_latch == 0 && card.retries + 1 < 5 {
                    self.submode = 0x04;
                } else if any {
                    self.submode = 0x14;
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
                }
            }
            // 0x14 - the second two-row menu, 0x801DDF30.
            0x14 => {
                self.fault_arm = false;
                self.preroll = 0x1000;
                self.remap_gate = -1;
                self.fade -= 0x100 * scalar;
                if self.fade > 0 {
                    self.cancel_target = None;
                    return true;
                }
                if cancel {
                    self.remap_gate = 0;
                }
                self.fade = 0;
                self.cancel_target = Some(if self.arg1 != 0 {
                    0x16
                } else if self.entry_word != 0 {
                    0x10
                } else {
                    0x02
                });
                self.cursor_kind = 1;
                self.slider_x = 0x100;
                self.slider_dir = 0;
                self.rows = 2;
                if pad.nav & PADMASK_CURSOR_PREV != 0 {
                    self.row_counter -= 1;
                }
                if pad.nav & PADMASK_CURSOR_NEXT != 0 {
                    self.row_counter += 1;
                }
                self.row_counter &= 1;
                if confirm {
                    self.submode = 0x07;
                    self.menu_index = self.row_counter as u32;
                }
            }
            // 0x15 - the card check, 0x801DE260.
            0x15 => {
                self.cancel_target = None;
                self.fault_arm = true;
                self.check_timer += scalar;
                if self.check_timer < 0x259 {
                    if any {
                        // Falls into 0x0F's body at 0x801DEF2C.
                        self.submode = 0x14;
                    }
                    return true;
                }
                self.check_timer = 0x258;
                if card.error != 0 {
                    // Falls into 0x0A's body at 0x801DE838.
                    self.submode = 0x07;
                    return true;
                }
                if card.grace != 0 {
                    return true;
                }
                if card.slot_kind == 3 && card.scan_request == 0 {
                    self.submode = 0x09;
                    if card.scan_total == 0 {
                        if self.menu_index != 0 {
                            self.submode = 0x0C;
                        } else if card.has_blocks == 0 {
                            self.submode = 0x0D;
                        }
                    }
                }
                if card.fault > 0 {
                    self.submode = 0x08;
                }
                if card.block_count >= 2 {
                    self.submode = if self.menu_index == 0 { 0x0F } else { 0x0C };
                }
            }
            // 0x16 - the launch white-out, 0x801DF8D0.
            0x16 => {
                self.cancel_target = None;
                self.fault_arm = false;
                self.launch_timer += scalar;
                // The `+= 0x5A` skip is the delay slot of the `op_kind`
                // test at 0x801DF90C, so it lands whatever the banner is;
                // only the cue is gated.
                if any && self.launch_timer < 0x5A {
                    self.launch_timer += 0x5A;
                    if self.op_kind != 0 {
                        fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
                    }
                }
                if self.launch_timer >= 0x5B {
                    self.fade += 0x100 * scalar;
                }
                if self.fade >= 0x1200 {
                    self.fade = 0x1200;
                    self.submode = 0x06;
                    if self.op_kind == OP_KIND_LOAD {
                        fx.push(TitleTickEffect::LaunchGame { from_load: true });
                        self.entry_word = 0;
                    }
                }
            }
            // 0x17 - the save result screen, 0x801DF6F4.
            0x17 => {
                self.fault_arm = false;
                if card.result == 1 && card.retry_latch == 0 && card.retries + 1 < 5 {
                    self.submode = 0x03;
                    self.cancel_target = None;
                    return true;
                }
                self.cancel_target = Some(0x14);
                self.op_kind = 0;
                if any {
                    self.submode = 0x14;
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
                }
            }
            // 0x18 - the CONTINUE fade-in, 0x801DDD94.
            0x18 => {
                self.cancel_target = None;
                self.fault_arm = false;
                self.remap_gate = -1;
                self.preroll += 0x80 * scalar;
                if self.preroll >= 0x1001 {
                    self.preroll = 0x1000;
                    self.submode = 0x14;
                }
            }
            // Out of range: `sltiu v0,s2,0x19` sends it to the epilogue.
            _ => {}
        }
        true
    }

    /// The shared epilogue at [`SUBMODE_BODY_PC`], which runs after every
    /// handler (and *is* the handler for sub-mode `0x01` and for any
    /// out-of-range selector).
    fn epilogue(
        &mut self,
        pad: TitleTickPad,
        card: TitleCardStatus,
        fx: &mut Vec<TitleTickEffect>,
    ) {
        let scalar = pad.frame_scalar.max(1) as i32;
        // The panel slider converges on 0x2C from whichever side it is on.
        match self.slider_dir {
            1 => {
                self.slider_x -= 8 * scalar;
                if self.slider_x < 0x2C {
                    self.slider_x = 0x2C;
                }
            }
            2 => {
                self.slider_x += 8 * scalar;
                if self.slider_x >= 0x2D {
                    self.slider_x = 0x2C;
                }
            }
            _ => {}
        }
        // `submode = FUN_801E1114(submode)` - the callee returns its
        // argument, so the store is an identity and only the model pass
        // it performs is a side effect.
        if self.remap_gate >= 0 {
            // identity
        }
        if self.fault_arm && card.fault > 0 && card.grace == 0 {
            self.submode = 0x08;
        }
        if self.entry_word != 0 {
            if self.submode == 0x02 {
                self.submode = 0x10;
            }
            if self.cancel_target == Some(0x02) {
                self.cancel_target = Some(0x10);
            }
        }
        if self.arg1 != 0 {
            if self.submode == 0x02 {
                self.submode = 0x16;
            }
            return;
        }
        if card.op_status == 0 {
            if self.submode == 0x03 {
                self.submode = if card.result != 0 { 0x17 } else { 0x16 };
            }
            if self.submode == 0x04 && card.busy != 0 {
                self.submode = 0x13;
            }
        }
        if self.submode != 0x03
            && self.submode != 0x04
            && pad.edge & PADMASK_CANCEL_L2_CIRCLE != 0
            && let Some(target) = self.cancel_target
        {
            fx.push(TitleTickEffect::Sfx(TITLE_SFX_CANCEL));
            self.submode = target;
        }
        if self.cursor_kind == 1 {
            if pad.edge & PADMASK_CONFIRM_L1_CROSS != 0 {
                fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
            }
            if pad.held & PADMASK_ANY_FACE_OR_L == 0 {
                if pad.nav & PADMASK_CURSOR_NEXT != 0 {
                    self.linear_cursor += 1;
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CURSOR_MOVE));
                }
                if pad.nav & PADMASK_CURSOR_PREV != 0 {
                    self.linear_cursor -= 1;
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CURSOR_MOVE));
                }
            }
            let last = self.rows - 1;
            if self.linear_cursor > last {
                self.linear_cursor = 0;
            }
            if self.linear_cursor < 0 {
                self.linear_cursor = last;
            }
        } else if self.cursor_kind == 2 && pad.held & PADMASK_ANY_FACE_OR_L == 0 {
            if pad.nav & PADMASK_CURSOR_NEXT != 0 {
                self.cursor_y += 1;
                fx.push(TitleTickEffect::Sfx(TITLE_SFX_CURSOR_MOVE));
            }
            if pad.nav & PADMASK_CURSOR_PREV != 0 {
                self.cursor_y -= 1;
                fx.push(TitleTickEffect::Sfx(TITLE_SFX_CURSOR_MOVE));
            }
            if pad.nav & PADMASK_GRID_LEFT != 0 {
                self.cursor_x -= 1;
                fx.push(TitleTickEffect::Sfx(TITLE_SFX_CURSOR_MOVE));
            }
            if pad.nav & PADMASK_GRID_RIGHT != 0 {
                self.cursor_x += 1;
                fx.push(TitleTickEffect::Sfx(TITLE_SFX_CURSOR_MOVE));
            }
            if self.cursor_x < 0 {
                self.cursor_x = GRID_COLUMNS - 1;
            }
            if self.cursor_x >= GRID_COLUMNS {
                self.cursor_x = 0;
            }
            if self.cursor_y < 0 {
                self.cursor_y = GRID_ROWS - 1;
            }
            if self.cursor_y >= GRID_ROWS {
                self.cursor_y = 0;
            }
        }
    }

    fn panel_fade_up(&mut self, scalar: i32) {
        self.panel_fade += 0x100 * scalar;
    }
}

impl TransitionTarget {
    /// The literal sub-modes this store can leave in the selector. Empty
    /// for the register-sourced and identity stores, whose target is a
    /// per-frame value rather than a constant.
    pub fn literals(self) -> Vec<u8> {
        match self {
            TransitionTarget::Mode(m) => vec![m],
            TransitionTarget::OneOf(v) => v.to_vec(),
            TransitionTarget::Register(_) | TransitionTarget::Identity => Vec::new(),
        }
    }
}

/// The two stores in [`STATE_204_WRITES`] that never survive the tick
/// that made them, and so carry no edge in the retail mode graph:
///
/// - `0x801DD920` writes `0x02` unconditionally in `Init`, and while the
///   entry word `_DAT_8007BB00` is up (which `init.pak` guarantees) the
///   sentinel arm at `0x801DD97C` replaces it in the same handler and the
///   shared epilogue would replace it again at `0x801DFEF8`;
/// - `0x801DDC38` writes `0` on the `AttractIdle` row-0 confirm and the
///   very next instruction (`0x801DDC3C`) writes `0x16` over it.
pub const OVERWRITTEN_STORES: &[u32] = &[0x801D_D920, 0x801D_DC38];

/// Four of the six shared-epilogue stores are guarded on the sub-mode the
/// handler left in the selector, so they are edges out of exactly that
/// mode rather than out of every mode. The remaining two - the identity
/// write-back at `0x801DFE88` and the fault arm at `0x801DFED0` - are not
/// (the fault arm only needs the per-frame `s5`), and carry no row here.
pub const EPILOGUE_GUARD_MODES: &[(u32, u8)] = &[
    (0x801D_FEF8, 0x02),
    (0x801D_FF28, 0x02),
    (0x801D_FF74, 0x03),
    (0x801D_FF9C, 0x04),
];

/// Every sub-mode a retail cold boot can put in the selector, walked from
/// [`TitleOverlaySubMode::Init`] over [`STATE_204_WRITES`]. Indexed by
/// mode byte.
///
/// [`OVERWRITTEN_STORES`] contribute no edge; the shared-epilogue rows
/// contribute one out of the mode [`EPILOGUE_GUARD_MODES`] names, or out
/// of every reached mode when they name none.
pub fn cold_boot_reachable_modes() -> [bool; SUBMODE_JT_ENTRY_COUNT] {
    let mut seen = [false; SUBMODE_JT_ENTRY_COUNT];
    seen[TitleOverlaySubMode::Init as usize] = true;
    let mut changed = true;
    while changed {
        changed = false;
        for w in STATE_204_WRITES {
            if OVERWRITTEN_STORES.contains(&w.pc) {
                continue;
            }
            let live = if w.from == SUBMODE_TAIL_SOURCE {
                match EPILOGUE_GUARD_MODES.iter().find(|(pc, _)| *pc == w.pc) {
                    Some((_, m)) => seen[*m as usize],
                    // Unguarded epilogue store: reachable once anything is.
                    None => true,
                }
            } else {
                seen[w.from as usize] || w.also_from.iter().any(|m| seen[*m as usize])
            };
            if !live {
                continue;
            }
            for t in w.target.literals() {
                if (t as usize) < SUBMODE_JT_ENTRY_COUNT && !seen[t as usize] {
                    seen[t as usize] = true;
                    changed = true;
                }
            }
        }
    }
    seen
}

/// The fmv id retail's attract arm hardcodes: `sh zero,-0x4588(v0)` at
/// `0x801DDCE8` zeroes `_DAT_8007BA78` before the mode write, so the
/// attract always plays `fmv_id 0` (`MV1.STR`, the intro).
pub const ATTRACT_FMV_ID: i16 = 0;

/// D-pad bit that steps the 5x3 grid cursor **left** (`andi v0,a2,0x8000`
/// at `0x801E0124`).
pub const PADMASK_GRID_LEFT: u16 = 0x8000;

/// D-pad bit that steps it **right** (`andi v0,a3,0x2000` at `0x801E0148`).
pub const PADMASK_GRID_RIGHT: u16 = 0x2000;

/// Columns in the slot grid the epilogue clamps `state[+0x1F4]` to
/// (`slti v0,v0,0x5` at `0x801E017C`).
pub const GRID_COLUMNS: i32 = 5;

/// Rows in that grid (`slti v0,v0,0x3` at `0x801E01B0`).
pub const GRID_ROWS: i32 = 3;

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
        // read out of `overlay_title.bin` at 0x801CF244.
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
        // so reachable addresses live below STATE_BASE_ADDR; each
        // displacement decodes to one of these literal addresses.
        // Sanity-check the table.
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
        assert!(froms.contains(&0x06), "LaunchGame (LaunchGame) missing");
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
            if w.from == SUBMODE_TAIL_SOURCE {
                continue;
            }
            assert!(
                TitleOverlaySubMode::is_in_range(w.from),
                "from-mode 0x{:02X} out of range",
                w.from
            );
            for extra in w.also_from {
                assert!(
                    TitleOverlaySubMode::is_in_range(*extra),
                    "also-from mode 0x{extra:02X} out of range"
                );
            }
        }
    }

    #[test]
    fn every_static_target_mode_is_in_range() {
        for w in STATE_204_WRITES {
            for target in w.target.literals() {
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
        // And the LaunchGame handler PC predates the launch-write PC (the
        // write happens inside LaunchGame's body).
        let phase06 = TitleOverlaySubMode::LaunchGame.handler_pc();
        assert!(
            phase06 < PHASE06_LAUNCH_GAME_PC,
            "LaunchGame handler 0x{phase06:08X} should precede launch write 0x{PHASE06_LAUNCH_GAME_PC:08X}"
        );
    }

    #[test]
    fn new_game_boot_chain_constants() {
        // NEW GAME is the top menu row (index 0); the launch write sets the
        // field INIT mode (2), whose init handler reaches the field scene
        // initializer, which hands off to the field RUN mode (3).
        assert_eq!(MENU_INDEX_NEW_GAME, 0);
        assert_eq!(MENU_INDEX_STATE_OFFSET, 0x200);
        assert_eq!(MASTER_GAME_MODE_FIELD_LAUNCH, 0x02);
        assert_eq!(MASTER_GAME_MODE_FIELD_RUN, 0x03);
        // INIT precedes RUN, and both differ from the attract STR-FMV mode.
        const _: () = assert!(MASTER_GAME_MODE_FIELD_LAUNCH < MASTER_GAME_MODE_FIELD_RUN);
        assert_ne!(MASTER_GAME_MODE_FIELD_RUN, MASTER_GAME_MODE_STR_INIT);
        // The mode-2 init handler is SCUS-resident; the field scene
        // initializer it calls is overlay-resident (0x801C0000+).
        const _: () = assert!(MODE2_INIT_HANDLER_PC < 0x801C_0000);
        const _: () = assert!(FIELD_SCENE_INIT_PC >= 0x801C_0000);
    }

    // -- the executable half ------------------------------------------

    fn menu() -> TitleMenuState {
        TitleMenuState::new()
    }

    #[test]
    fn cursor_wraps_over_the_two_row_space() {
        let mut m = menu();
        assert_eq!(m.row(TITLE_MENU_ROWS), 0);
        let ev = m.step(PADMASK_CURSOR_NEXT, 0, 1, TITLE_MENU_ROWS);
        assert_eq!(ev[0], TitleMenuEvent::CursorMoved { row: 1 });
        assert_eq!(m.sfx, Some(TITLE_SFX_CURSOR_MOVE));
        // Down again wraps back to 0 - retail's `andi v1,v1,0x1`.
        let ev = m.step(PADMASK_CURSOR_NEXT, 0, 1, TITLE_MENU_ROWS);
        assert_eq!(ev[0], TitleMenuEvent::CursorMoved { row: 0 });
        // Up from 0 wraps to the last row.
        let ev = m.step(PADMASK_CURSOR_PREV, 0, 1, TITLE_MENU_ROWS);
        assert_eq!(ev[0], TitleMenuEvent::CursorMoved { row: 1 });
        // The counter never runs away from the row space.
        assert!(m.row_counter >= 0 && m.row_counter < TITLE_MENU_ROWS as i32);
    }

    #[test]
    fn confirm_takes_every_bit_of_the_0x844_mask() {
        for bit in [0x0800u16, 0x0040, 0x0004] {
            assert_ne!(PADMASK_START_L1_CROSS & bit, 0, "{bit:#06x} is in the mask");
            let mut m = menu();
            let ev = m.step(bit, 0, 1, TITLE_MENU_ROWS);
            assert_eq!(ev, vec![TitleMenuEvent::Confirmed { row: 0 }]);
            assert_eq!(m.sfx, Some(TITLE_SFX_CONFIRM));
            assert_eq!(m.chosen_row, TITLE_ROW_NEW_GAME);
        }
        // A bit outside the mask confirms nothing.
        let mut m = menu();
        assert!(m.step(0x0010, 0, 1, TITLE_MENU_ROWS).is_empty());
    }

    #[test]
    fn a_confirm_on_row_one_stashes_continue() {
        let mut m = menu();
        m.step(PADMASK_CURSOR_NEXT, 0, 1, TITLE_MENU_ROWS);
        let ev = m.step(PADMASK_START_L1_CROSS, 0, 1, TITLE_MENU_ROWS);
        assert_eq!(
            ev,
            vec![TitleMenuEvent::Confirmed {
                row: TITLE_ROW_CONTINUE
            }]
        );
        assert_eq!(m.chosen_row, TITLE_ROW_CONTINUE);
    }

    #[test]
    fn the_last_sixteen_frames_accept_no_input() {
        let mut m = menu();
        m.countdown = ATTRACT_INPUT_FREEZE_BELOW - 1;
        // Neither the cursor nor the confirm is read below the band; the
        // countdown still runs, and a held pad still re-arms it.
        let ev = m.step(
            PADMASK_CURSOR_NEXT | PADMASK_START_L1_CROSS,
            0,
            1,
            TITLE_MENU_ROWS,
        );
        assert!(ev.is_empty(), "input read below the freeze band: {ev:?}");
        assert_eq!(m.row_counter, 0);
        assert_eq!(m.sfx, None);
        // One frame above the band the same word is read.
        let mut m = menu();
        m.countdown = ATTRACT_INPUT_FREEZE_BELOW;
        let ev = m.step(PADMASK_CURSOR_NEXT, 0, 1, TITLE_MENU_ROWS);
        assert_eq!(ev[0], TitleMenuEvent::CursorMoved { row: 1 });
    }

    #[test]
    fn any_held_bit_re_arms_the_countdown() {
        let mut m = menu();
        m.countdown = 3;
        m.step(0, 0x0010, 1, TITLE_MENU_ROWS);
        assert_eq!(m.countdown, COUNTDOWN_RESET_VALUE as i32 - 1);
    }

    #[test]
    fn the_countdown_fires_on_underflow_at_the_frame_scalar() {
        let mut m = menu();
        m.countdown = 1;
        assert!(m.step(0, 0, 1, TITLE_MENU_ROWS).is_empty());
        assert_eq!(m.countdown, 0);
        let ev = m.step(0, 0, 1, TITLE_MENU_ROWS);
        assert_eq!(ev, vec![TitleMenuEvent::AttractFired]);
        // A doubled frame scalar spends the countdown twice as fast.
        let mut m = menu();
        m.countdown = 4;
        m.step(0, 0, 2, TITLE_MENU_ROWS);
        assert_eq!(m.countdown, 2);
    }

    // -- the whole dispatcher ------------------------------------------

    #[test]
    fn the_table_holds_every_store_the_function_makes() {
        // A Capstone pass over PROT 0899 file +0xEB44 finds exactly 56
        // `sw <reg>,0x204(<base>)` in the 3026-instruction body.
        assert_eq!(STATE_204_WRITES.len(), 56);
    }

    #[test]
    fn every_store_names_a_guard() {
        for w in STATE_204_WRITES {
            assert!(!w.guard.is_empty(), "0x{:08X} has no guard", w.pc);
        }
    }

    #[test]
    fn the_three_cross_handler_stores_carry_their_second_source() {
        // Three handlers `j` into the middle of another handler's body,
        // so those stores fire from two sub-modes. Without the second
        // source the 0x04 / 0x05 / 0x13 cluster has no entry at all.
        let extra: std::collections::BTreeMap<u32, &[u8]> = STATE_204_WRITES
            .iter()
            .filter(|w| !w.also_from.is_empty())
            .map(|w| (w.pc, w.also_from))
            .collect();
        assert_eq!(extra.len(), 3, "expected exactly three aliased stores");
        assert_eq!(extra[&0x801D_E844], &[0x15][..]); // 0x15 -> 0x801DE838
        assert_eq!(extra[&0x801D_EF34], &[0x15][..]); // 0x15 -> 0x801DEF2C
        assert_eq!(extra[&0x801D_F484], &[0x0E][..]); // 0x0E -> 0x801DF47C
    }

    #[test]
    fn no_handler_leaves_the_mode_graph() {
        // Every literal a store can leave in the selector is inside the
        // dispatcher's `sltiu v0,s2,0x19` window, so no handler can put
        // the tick into the out-of-range path by accident.
        for w in STATE_204_WRITES {
            for t in w.target.literals() {
                assert!(
                    TitleOverlaySubMode::is_in_range(t),
                    "0x{:08X} targets out-of-range 0x{t:02X}",
                    w.pc
                );
            }
        }
    }

    #[test]
    fn nothing_reaches_the_two_dead_modes_from_a_cold_boot() {
        let seen = cold_boot_reachable_modes();
        assert!(
            !seen[TitleOverlaySubMode::Idle as usize],
            "0x01 Idle is the out-of-range slot - no store writes 1"
        );
        assert!(
            !seen[TitleOverlaySubMode::TextMenu as usize],
            "0x02 is bypassed by the Init sentinel arm and again by the epilogue"
        );
        // And that is a property of the stores, not of the walk: no row
        // outside OVERWRITTEN_STORES targets either mode.
        for w in STATE_204_WRITES {
            if OVERWRITTEN_STORES.contains(&w.pc) {
                continue;
            }
            for t in w.target.literals() {
                assert_ne!(t, 0x01, "0x{:08X} writes Idle", w.pc);
                assert_ne!(t, 0x02, "0x{:08X} writes TextMenu", w.pc);
            }
        }
    }

    #[test]
    fn every_store_fires_from_a_mode_a_cold_boot_can_be_in() {
        // Stronger than the mode-reachability test: not just "every mode is
        // reached" but "every one of the 56 stores is a live edge". A row
        // whose source mode a cold boot never enters is either mis-attributed
        // or evidence of a handler nothing dispatches.
        let seen = cold_boot_reachable_modes();
        for w in STATE_204_WRITES {
            if w.from == SUBMODE_TAIL_SOURCE {
                // The epilogue runs after every handler; its guarded rows
                // are covered by EPILOGUE_GUARD_MODES below.
                if let Some((_, m)) = EPILOGUE_GUARD_MODES.iter().find(|(pc, _)| *pc == w.pc) {
                    // The two rows guarded on 0x02 are the mechanism that
                    // makes 0x02 unreachable, so of course their guard mode
                    // is not reachable - that is the point of them.
                    assert!(
                        seen[*m as usize] || *m == 0x02,
                        "epilogue store 0x{:08X} is guarded on unreachable mode 0x{m:02X}",
                        w.pc
                    );
                }
                continue;
            }
            let live = seen[w.from as usize] || w.also_from.iter().any(|m| seen[*m as usize]);
            if !live {
                // The only dead source is 0x02's own body: the entry word
                // keeps a retail boot out of that handler entirely, so its
                // two stores are edges nothing can take.
                assert_eq!(
                    w.from, 0x02,
                    "store 0x{:08X} fires only from unreachable mode 0x{:02X}",
                    w.pc, w.from
                );
            }
        }
    }

    #[test]
    fn every_other_mode_is_reachable_from_the_cold_boot_entry() {
        let seen = cold_boot_reachable_modes();
        for (mode, reached) in seen.iter().enumerate() {
            if mode == TitleOverlaySubMode::Idle as usize
                || mode == TitleOverlaySubMode::TextMenu as usize
            {
                continue;
            }
            assert!(
                *reached,
                "sub-mode 0x{mode:02X} ({}) unreachable from Init",
                SUBMODE_TABLE[mode].label
            );
        }
    }

    #[test]
    fn the_attract_arm_lives_only_in_attract_idle() {
        // The countdown decrement + the two attract stores are inside
        // 0x10's handler extent, not the preamble and not the epilogue.
        let lo = TitleOverlaySubMode::AttractIdle.handler_pc();
        let hi = TitleOverlaySubMode::ContinueFadeIn.handler_pc();
        assert!(lo < SUBMODE_COUNTDOWN_DECR_PC && SUBMODE_COUNTDOWN_DECR_PC < hi);
        assert!(lo < crate::cutscene_trigger::TITLE_TICK_INLINE.mode_write_addr);
        assert!(crate::cutscene_trigger::TITLE_TICK_INLINE.mode_write_addr < hi);
        assert_eq!(ATTRACT_FMV_ID, 0);
    }

    #[test]
    fn both_master_mode_two_writers_are_pinned() {
        // The load route writes it inside 0x16, the new-game route inside
        // 0x06; each is inside its own handler's extent.
        assert!(TitleOverlaySubMode::LaunchFade.handler_pc() < PHASE16_LOAD_LAUNCH_PC);
        assert!(PHASE16_LOAD_LAUNCH_PC < TitleOverlaySubMode::LaunchGame.handler_pc());
        assert!(TitleOverlaySubMode::LaunchGame.handler_pc() < PHASE06_LAUNCH_GAME_PC);
        const _: () = assert!(PHASE06_LAUNCH_GAME_PC < SUBMODE_BODY_PC);
    }

    // -- the executable dispatcher -------------------------------------

    fn tick(state: &mut TitleTickState, pad: TitleTickPad) -> Vec<TitleTickEffect> {
        state.step(pad, TitleCardStatus::default())
    }

    #[test]
    fn a_cold_boot_enters_attract_idle_through_the_delay_state() {
        // The entry word is what routes it, not a hard-coded mode: Init
        // reads `_DAT_8007BB00`, writes 0x11, and 0x11 hands to 0x10 once
        // its 8-per-frame accumulator is spent.
        let mut s = TitleTickState::cold_boot();
        assert_eq!(s.submode, TitleOverlaySubMode::Init as u8);
        let fx = tick(&mut s, TitleTickPad::from_edge(0));
        assert_eq!(s.submode, TitleOverlaySubMode::AttractDelay as u8);
        assert!(fx.contains(&TitleTickEffect::LoadTitleAssets));
        // The SCUS stager seeds the hold with 0x100 and 0x11 spends it at
        // 8 per frame; the hand-off fires on the frame that reads it
        // already at zero, so the menu comes up 33 frames later.
        for _ in 0..=(ATTRACT_DELAY_SEED / 8) {
            assert_eq!(s.submode, TitleOverlaySubMode::AttractDelay as u8);
            tick(&mut s, TitleTickPad::from_edge(0));
        }
        assert_eq!(s.submode, TitleOverlaySubMode::AttractIdle as u8);
        assert_eq!(s.countdown, COUNTDOWN_RESET_VALUE as i32);
        // 0x02 is never entered on the way.
        assert_ne!(s.submode, TitleOverlaySubMode::TextMenu as u8);
    }

    #[test]
    fn a_zeroed_entry_word_is_the_only_way_into_the_text_menu() {
        // With the word down, Init leaves 0x02 and the epilogue does not
        // rewrite it - the graph retail never takes because `init.pak`
        // raises the word at 0x801CEB84.
        let mut s = TitleTickState::with_entry_word(0);
        let fx = tick(&mut s, TitleTickPad::from_edge(0));
        assert_eq!(s.submode, TitleOverlaySubMode::TextMenu as u8);
        assert!(!fx.contains(&TitleTickEffect::LoadTitleAssets));
        // Raise the word and the epilogue takes it straight to 0x10.
        s.entry_word = ENTRY_WORD_COLD_BOOT;
        tick(&mut s, TitleTickPad::from_edge(0));
        assert_eq!(s.submode, TitleOverlaySubMode::AttractIdle as u8);
    }

    #[test]
    fn the_return_from_the_attract_still_takes_the_sentinel_arm() {
        let mut s = TitleTickState::with_entry_word(ENTRY_WORD_FROM_ATTRACT);
        let fx = tick(&mut s, TitleTickPad::from_edge(0));
        assert_eq!(s.submode, TitleOverlaySubMode::AttractDelay as u8);
        // Only the exact cold-boot value streams the title assets again.
        assert!(!fx.contains(&TitleTickEffect::LoadTitleAssets));
    }

    fn at_attract_idle() -> TitleTickState {
        let mut s = TitleTickState::cold_boot();
        // Init, then the SCUS-seeded 0x100/8 hold, then one more frame
        // to spend the pre-roll so the menu is live.
        for _ in 0..(2 + ATTRACT_DELAY_SEED / 8) {
            tick(&mut s, TitleTickPad::from_edge(0));
        }
        // The hold transitions on the frame that reads the accumulator
        // already at zero (`bgtz v1` at 0x801DDAB4), i.e. one frame past
        // the last decrement.
        tick(&mut s, TitleTickPad::from_edge(0));
        assert_eq!(s.submode, TitleOverlaySubMode::AttractIdle as u8);
        s
    }

    #[test]
    fn the_new_game_row_confirms_into_the_launch_fade() {
        let mut s = at_attract_idle();
        assert_eq!(s.submode, TitleOverlaySubMode::AttractIdle as u8);
        let fx = tick(&mut s, TitleTickPad::from_edge(PADMASK_START_L1_CROSS));
        assert_eq!(s.submode, TitleOverlaySubMode::LaunchFade as u8);
        assert!(fx.contains(&TitleTickEffect::Sfx(TITLE_SFX_CONFIRM)));
    }

    #[test]
    fn the_continue_row_confirms_into_the_fade_in_then_the_main_menu() {
        let mut s = at_attract_idle();
        tick(&mut s, TitleTickPad::from_edge(PADMASK_CURSOR_NEXT));
        assert_eq!(s.row_counter, TITLE_ROW_CONTINUE as i32);
        tick(&mut s, TitleTickPad::from_edge(PADMASK_START_L1_CROSS));
        assert_eq!(s.submode, TitleOverlaySubMode::ContinueFadeIn as u8);
        assert_eq!(s.menu_index, TITLE_ROW_CONTINUE as u32);
        // The fade-in ramps 0x80 per frame to 0x1000, then hands over.
        for _ in 0..0x40 {
            tick(&mut s, TitleTickPad::from_edge(0));
        }
        assert_eq!(s.submode, TitleOverlaySubMode::MainMenu as u8);
    }

    #[test]
    fn the_launch_fade_hands_to_the_launcher_and_the_launcher_writes_mode_two() {
        let mut s = at_attract_idle();
        tick(&mut s, TitleTickPad::from_edge(PADMASK_START_L1_CROSS));
        assert_eq!(s.submode, TitleOverlaySubMode::LaunchFade as u8);
        let mut launched = false;
        for _ in 0..0x100 {
            for fx in tick(&mut s, TitleTickPad::from_edge(0)) {
                if fx == (TitleTickEffect::LaunchGame { from_load: false }) {
                    launched = true;
                }
            }
            if launched {
                break;
            }
        }
        assert!(launched, "the NEW GAME route never wrote master mode 2");
        // And the launcher re-arms Init behind it.
        assert_eq!(s.submode, TitleOverlaySubMode::Init as u8);
    }

    #[test]
    fn the_attract_fires_from_attract_idle_with_fmv_zero() {
        let mut s = at_attract_idle();
        s.countdown = 1;
        assert!(tick(&mut s, TitleTickPad::from_edge(0)).is_empty());
        let fx = tick(&mut s, TitleTickPad::from_edge(0));
        assert!(fx.contains(&TitleTickEffect::FireAttract { fmv_id: 0 }));
        assert_eq!(s.submode, TitleOverlaySubMode::AttractIdle as u8);
    }

    #[test]
    fn the_last_sixteen_frames_of_the_countdown_take_no_confirm() {
        let mut s = at_attract_idle();
        s.countdown = ATTRACT_INPUT_FREEZE_BELOW - 1;
        tick(&mut s, TitleTickPad::from_edge(PADMASK_START_L1_CROSS));
        assert_eq!(
            s.submode,
            TitleOverlaySubMode::AttractIdle as u8,
            "input was read below the freeze band"
        );
    }

    #[test]
    fn the_panel_slider_converges_on_0x2c_from_both_sides() {
        // Both epilogue arms clamp to the same value, so the "clamped
        // [0, 0x2C]" reading of `state[-0xeb4]` is wrong in its low half:
        // the decreasing arm floors at 0x2C, it does not run to 0.
        let mut s = TitleTickState::cold_boot();
        s.submode = TitleOverlaySubMode::Idle as u8;
        s.slider_dir = 1;
        s.slider_x = 0x100;
        for _ in 0..0x100 {
            tick(&mut s, TitleTickPad::from_edge(0));
        }
        assert_eq!(s.slider_x, 0x2C);
        s.slider_dir = 2;
        s.slider_x = 0;
        for _ in 0..0x100 {
            tick(&mut s, TitleTickPad::from_edge(0));
        }
        assert_eq!(s.slider_x, 0x2C);
    }

    #[test]
    fn the_epilogue_cancel_arm_uses_the_handlers_own_target() {
        // 0x0C / 0x0D set `s3 = 0x14` and the epilogue applies it on the
        // 0x21 mask with cue 0x37.
        let mut s = TitleTickState::cold_boot();
        s.entry_word = 0;
        s.submode = TitleOverlaySubMode::LoadNotice as u8;
        let fx = tick(&mut s, TitleTickPad::from_edge(PADMASK_CANCEL_L2_CIRCLE));
        assert_eq!(s.submode, TitleOverlaySubMode::MainMenu as u8);
        assert!(fx.contains(&TitleTickEffect::Sfx(TITLE_SFX_CANCEL)));
    }

    #[test]
    fn the_grid_cursor_wraps_over_the_five_by_three_slot_grid() {
        let mut s = TitleTickState::cold_boot();
        s.submode = TitleOverlaySubMode::SlotGrid as u8;
        s.cursor_x = GRID_COLUMNS - 1;
        s.cursor_y = GRID_ROWS - 1;
        tick(&mut s, TitleTickPad::from_edge(PADMASK_GRID_RIGHT));
        assert_eq!(s.cursor_x, 0);
        tick(&mut s, TitleTickPad::from_edge(PADMASK_CURSOR_NEXT));
        assert_eq!(s.cursor_y, 0);
        tick(&mut s, TitleTickPad::from_edge(PADMASK_GRID_LEFT));
        assert_eq!(s.cursor_x, GRID_COLUMNS - 1);
        tick(&mut s, TitleTickPad::from_edge(PADMASK_CURSOR_PREV));
        assert_eq!(s.cursor_y, GRID_ROWS - 1);
    }

    #[test]
    fn the_second_argument_pre_selects_a_menu_row() {
        // The two Init arms nothing in retail's production caller uses:
        // `FUN_801E36A0` passes 0, but 1 / 2 land straight on 0x14 with
        // the row already stashed.
        for (arg, row) in [(1u32, 0u32), (2, 1)] {
            let mut s = TitleTickState::with_entry_word(0);
            s.arg1 = arg;
            tick(&mut s, TitleTickPad::from_edge(0));
            assert_eq!(s.submode, TitleOverlaySubMode::MainMenu as u8);
            assert_eq!(s.menu_index, row);
        }
    }
}
