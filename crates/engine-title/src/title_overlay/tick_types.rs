//! The whole dispatcher's inputs, effects and state (`TitleTickState`).

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
/// that writes [`MASTER_GAME_MODE_FIELD_LAUNCH`](super::MASTER_GAME_MODE_FIELD_LAUNCH) on the **load** route,
/// before `0x16` hands on to `0x06`. The tick therefore has two
/// master-mode-2 writers, not one: this and
/// [`PHASE06_LAUNCH_GAME_PC`](super::PHASE06_LAUNCH_GAME_PC).
pub const PHASE16_LOAD_LAUNCH_PC: u32 = 0x801D_FAFC;

/// The memory-card words the tick shares with the card driver
/// (`FUN_801E373C` and the routines it starts). Every field names the
/// retail address; a cold boot that never opens a card screen leaves all of
/// them at their [`Default`] zero, which is what the retail globals hold
/// there.
///
/// The tick reads these **and writes them**: mode `0x0E` stages
/// `op_status`, `0x13` / `0x17` count retries and clear the fault, the
/// shared epilogue raises `ready`, and so on. [`TitleTickState::step`]
/// therefore takes them by `&mut`, and a host that keeps them across frames
/// sees what retail's globals would hold.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TitleCardStatus {
    /// `state[-0xec0]` (`0x801EF140`): the last transfer's status (`1`
    /// failed and retryable, `2` a verify mismatch `0x05` raises).
    pub busy: i32,
    /// `state[-0xef4]` (`0x801EF10C`): the last operation errored.
    pub error: i32,
    /// `state[-0xec4]` (`0x801EF13C`): the last write's result code.
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
    /// `state[+0x329C]`: the operation the epilogue polls (`0` idle, `1` a
    /// save, `2` a load, `7` a format).
    pub op_status: i32,
    /// `state[+0x1D0]`: the block-transfer progress bar's target.
    pub draw_done: i32,
    /// `state[+0x228]`: the grace countdown `0x07` seeds with `0x78`.
    pub grace: i32,
    /// `state[+0x1C0]`: the phase of mode `0x12`'s format operation
    /// (21-entry inner JT at `0x801CF2AC`).
    pub op_phase: i32,
    /// `state[+0x3288]`: that operation's hold timer.
    pub op_timer: i32,
    /// `state[+0x1C4]`: format attempts (phase `0x0D` gives up at 3).
    pub op_attempts: i32,
    /// `state[+0x328C]`: the format's outcome (`2` = gave up).
    pub op_result: i32,
    /// `state[+0x220]`: the format request `0x15` clears and phase 1 tests
    /// for `3`.
    pub op_request: i32,
    /// `state[+0x3804]`: the card driver's last reply (`-2` = unformatted).
    pub op_reply: i32,
    /// `state[-0xe80]` (`0x801EF180`): the card probe has answered; `0x15`
    /// waits on it.
    pub probe: i32,
    /// What `FUN_801E3F74` says of the picked grid cell: `1` a readable
    /// save, `3` a block a save may take, `2` neither, `4` the cell past
    /// the grid. A Load takes `1`, a Save `1` or `3`.
    pub slot_status: i32,
    /// The loaded block's checksum matches its `slot[0x1FFC]` word (mode
    /// `0x05`, `FUN_801E38D8`).
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
    /// [`MASTER_GAME_MODE_STR_INIT`](super::MASTER_GAME_MODE_STR_INIT) to [`MASTER_GAME_MODE_ADDR`](super::MASTER_GAME_MODE_ADDR).
    FireAttract { fmv_id: i16 },
    /// A master-mode `0x02` hand-off. `from_load` distinguishes mode
    /// `0x16`'s load route ([`PHASE16_LOAD_LAUNCH_PC`]) from mode `0x06`'s
    /// new-game route ([`PHASE06_LAUNCH_GAME_PC`](super::PHASE06_LAUNCH_GAME_PC)).
    LaunchGame { from_load: bool },
    /// The tick called the card driver's reset `FUN_801E373C(arg)` (`1`
    /// re-probes the slot, `0` only clears the operation).
    CardReset { arg: u32 },
    /// The tail drew the title backdrop: the strip drawer `FUN_801E0418`
    /// and the dimmed art `FUN_801E02A4`, both handed `shade` as their one
    /// brightness byte ([`TitleTickState::backdrop_shade`]).
    Backdrop { shade: u8 },
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
    /// `param_2` (`a1`/`s8`). The title's caller `FUN_801E36A0` passes
    /// `0`; the pause menu's Save / Load pass `1` / `2`, the arms that
    /// pre-select a menu row.
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
    /// [`STATE_ATTRACT_COUNTDOWN_ADDR`](super::STATE_ATTRACT_COUNTDOWN_ADDR).
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
    /// [`STATE_HORIZ_SLIDER_X_ADDR`](super::STATE_HORIZ_SLIDER_X_ADDR).
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
    /// `0x8007B834` - the row the main menu (`0x14`) last confirmed, which
    /// `0x07` hands to the remap gate.
    pub menu_row: i32,
    /// `state[-0xe9c]` (`0x801EF164`) - the main menu's own row counter.
    pub main_row: i32,
    /// `state[-0xe98]` (`0x801EF168`) - the row the card was last probed
    /// for; `0x07` re-probes when [`Self::menu_row`] differs.
    pub probed_row: i32,
    /// `0x8007B7CC` - the slot index (`x + 5y`) the grid cursor is derived
    /// from.
    pub slot_index: i32,
    /// `state[-0xe70]` (`0x801EF190`) - the epilogue's free-running fade.
    pub alpha_a: i32,
    /// `state[-0xe6c]` (`0x801EF194`) - the card panel's fade.
    pub alpha_b: i32,
    /// `state[-0xe60]` (`0x801EF1A0`) - the card text's fade.
    pub alpha_c: i32,
    /// `state[-0xe68]` (`0x801EF198`) - raises the epilogue's ramp of
    /// [`Self::alpha_b`] / [`Self::alpha_c`].
    pub alpha_gate: i32,
    /// `state[-0xe58]` (`0x801EF1A8`) - the fault message's flash timer.
    pub fault_flash: i32,
    /// `state[-0xea4]` (`0x801EF15C`) - one frame's sweep direction (`0`
    /// down, `1` up), consumed by the next preamble.
    pub sweep_dir: i32,
    /// `state[-0xea0]` (`0x801EF160`) - the sweep the preamble steps.
    pub sweep: i32,
    /// `state[-0xe90]` (`0x801EF170`) - the preamble's frame counter.
    pub frame_counter: i32,
    /// `state[-0xed0]` (`0x801EF130`) - arms the main menu's cancel reset;
    /// raised by the fault message `0x08`.
    pub cancel_arm: i32,
    /// `state[-0xef8]` (`0x801EF108`) - the card has been probed once;
    /// `0x07` adds `0x96` to the grace while it is down.
    pub probed: i32,
    /// `state[-0xed4]` (`0x801EF12C`) - `0x08` raises it and `0x07` adds
    /// `0xB4` to the grace for it.
    pub fault_seen: i32,
    /// `state[-0xecc]` (`0x801EF134`) - the fault message's own timer;
    /// past `0xB4` it resets the card each frame.
    pub fault_timer: i32,
    /// `0x8007B910` - the music level the attract and launch fades write.
    pub music_level: i32,
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
