//! The overlay state block's addresses and the master game-mode values it hands off to.
//! Split out of `title_overlay.rs`.

/// Title-overlay state struct base. Passed as the first argument (`a0`)
/// to [`SUBMODE_TICK_FN_ENTRY_PC`](super::SUBMODE_TICK_FN_ENTRY_PC).
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
/// [`COUNTDOWN_RESET_VALUE`] by [`TitleOverlaySubMode::Init`](super::TitleOverlaySubMode::Init) and
/// [`TitleOverlaySubMode::AttractDelay`](super::TitleOverlaySubMode::AttractDelay); the CD-DMA overlay load
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

/// Value [`TitleOverlaySubMode::Init`](super::TitleOverlaySubMode::Init) and
/// [`TitleOverlaySubMode::AttractDelay`](super::TitleOverlaySubMode::AttractDelay) write to
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
/// - [`TitleOverlaySubMode::LaunchGame`](super::TitleOverlaySubMode::LaunchGame) sets it to
///   [`MASTER_GAME_MODE_FIELD_LAUNCH`] = `0x02` - the "exit title /
///   launch game" transition (instruction `sh v0, -0x47C4(v1)` at
///   `0x801DFC00`).
pub const MASTER_GAME_MODE_ADDR: u32 = 0x8007_B83C;

/// Master-game-mode value [`crate::cutscene_trigger::STR_INIT_MODE`]
/// re-exported here for use alongside [`MASTER_GAME_MODE_ADDR`] in this
/// module.
pub const MASTER_GAME_MODE_STR_INIT: u8 = 0x1A;

/// Master-game-mode value the [`TitleOverlaySubMode::LaunchGame`](super::TitleOverlaySubMode::LaunchGame)
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
/// write at [`PHASE06_LAUNCH_GAME_PC`](super::PHASE06_LAUNCH_GAME_PC); a non-zero index (CONTINUE) routes
/// to the save/card load path instead.
pub const MENU_INDEX_NEW_GAME: u32 = 0;
