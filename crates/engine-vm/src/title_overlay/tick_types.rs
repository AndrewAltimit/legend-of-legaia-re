//! The whole dispatcher's inputs, effects and state (`TitleTickState`).
//! Split out of `title_overlay.rs`.

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
/// that writes [`MASTER_GAME_MODE_FIELD_LAUNCH`](super::MASTER_GAME_MODE_FIELD_LAUNCH) on the **load** route,
/// before `0x16` hands on to `0x06`. The tick therefore has two
/// master-mode-2 writers, not one: this and
/// [`PHASE06_LAUNCH_GAME_PC`](super::PHASE06_LAUNCH_GAME_PC).
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
    /// [`MASTER_GAME_MODE_STR_INIT`](super::MASTER_GAME_MODE_STR_INIT) to [`MASTER_GAME_MODE_ADDR`](super::MASTER_GAME_MODE_ADDR).
    FireAttract { fmv_id: i16 },
    /// A master-mode `0x02` hand-off. `from_load` distinguishes mode
    /// `0x16`'s load route ([`PHASE16_LOAD_LAUNCH_PC`]) from mode `0x06`'s
    /// new-game route ([`PHASE06_LAUNCH_GAME_PC`](super::PHASE06_LAUNCH_GAME_PC)).
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
