//! Custom-overlay loading on retail — the vertical slice that proves we can
//! stream hand-written code from an (overwritten) pochi PROT slot into RAM and
//! execute it on real hardware, the foundation the full retail seru-trade UI
//! needs (its UI driver is far too big for the SCUS rodata gap, so it must ship
//! as a loadable overlay the way the fishing / slot-machine minigames do).
//!
//! ## The mechanism
//!
//! 1. The randomizer overwrites a **pochi-filler PROT slot** (265 exist, the
//!    largest >1 MB — reserved dev fillers with real allocated disc sectors) with
//!    a small custom overlay. Because the randomizer placed it, it knows that
//!    slot's exact start LBA + sector count from the disc TOC.
//! 2. A tiny **loader stub** in the preserved SCUS rodata gap calls the
//!    game's own synchronous CD reader [`LOADER_FN`]
//!    (`FUN_8005E4D4(sector_count, lba, dest)` — verified sync: it issues the
//!    read then waits) with those values **baked as literals**, so there is no
//!    runtime PROT-index arithmetic (the recurring ±2 index-space trap can't
//!    bite). It then `jalr`s the loaded code at [`DEST`], and on return replays
//!    the displaced hook instructions and jumps back.
//! 3. A detour at the shop-open path (field-VM op `0x49`) routes into the stub.
//!
//! ## The slice payload
//!
//! For the slice the overlay is the simplest observable: it writes a 32-bit
//! [`SENTINEL`] to [`SENTINEL_ADDR`] (a reserved cell in the SCUS rodata gap,
//! resident RAM we own) and returns. If the sentinel appears after the hook
//! fires on an emulator, the load→exec→return mechanism works on hardware; the
//! real trade UI then replaces this payload. The overlay is a position-
//! independent leaf (absolute data store + `jr ra`), so it runs correctly at any
//! load address.
//!
//! Nothing here embeds Sony bytes: the overlay + stub are the randomizer's own
//! code, and the LBA/sectors come from the user's disc.

/// The game's synchronous LBA reader `FUN_8005E4D4(a0=sector_count, a1=lba,
/// a2=dest) -> bool`. SCUS-resident (always callable). Verified from
/// `ghidra/scripts/funcs/8005e4d4.txt`: sets read position from `a1`, reads
/// `a0` sectors to `a2`, then blocks on the read-sync before returning.
pub const LOADER_FN: u32 = 0x8005_E4D4;

/// Load VA of the loader stub, in the preserved rodata gap at `0x8007AB38`
/// (`0x8007AE00`, in the free window above the flee-EXP routine `0x8007AD00`+0x100
/// and below the seru-trade config blob `0x8007AF00`).
pub const STUB_VA: u32 = 0x8007_AE00;

/// Where the custom overlay is loaded + executed. Slot B (`0x801F69D8`, the
/// summon/effect overlay region) is idle during a field shop; the slice payload
/// is a one-shot leaf so a briefly-borrowed region is fine. (The full UI will
/// use the slot-A on-demand-overlay path instead, like the minigames.)
pub const DEST: u32 = 0x801F_69D8;

/// Reserved sentinel cell in the rodata gap tail (after the 0x18-byte seru-trade
/// config blob at `0x8007AF00`), resident writable RAM we own.
pub const SENTINEL_ADDR: u32 = 0x8007_AF20;

// --- Shop-open trigger (field-VM op 0x49 arm edge, overlay 0897) -------------
//
// The merchant-open path is the field-VM op-0x49 "arm edge" in the field overlay
// dispatcher `FUN_801DE840`. At `SHOP_HOOK_VA` it stashes the operand pointer
// (`sw s6,-0x4bb0(s0)` = `_DAT_8007b450 = operand`) right after suspending; `s6`
// is the operand pointer, whose first byte is the op-0x49 sub-op (`0` = shop,
// `1` = name-entry, others = inn/save). A detour here fires once per open.

/// PROT entry hosting the field overlay (raw; a VA maps to file offset
/// `va - SHOP_OVERLAY_BASE`).
pub const SHOP_OVERLAY_PROT_INDEX: usize = 897;
/// Field overlay load base.
pub const SHOP_OVERLAY_BASE: u32 = 0x801C_E818;
/// Detour site: the op-0x49 arm-edge operand stash (`sw s6,-0x4bb0(s0)`).
pub const SHOP_HOOK_VA: u32 = 0x801E_09A8;
/// Where the detour returns (after the two displaced instructions).
pub const SHOP_RETURN_VA: u32 = 0x801E_09B0;
/// The two displaced instructions: `sw s6,-0x4bb0(s0)` then `lbu v0,0(s6)`
/// (verified against the disc). Also the recognized-build fingerprint.
pub const SHOP_DISPLACED: [u32; 2] = [0xAE16_B450, 0x92C2_0000];

/// The value the slice overlay writes to [`SENTINEL_ADDR`] ("SERU" trade slice).
pub const SENTINEL: u32 = 0x5E_2D_7A_DE;

// --- MIPS R3000 encoders (little-endian words) ------------------------------

const ZERO: u32 = 0;
const AT: u32 = 1;
const A0: u32 = 4;
const A1: u32 = 5;
const A2: u32 = 6;
const A3: u32 = 7;
const V0: u32 = 2;
const V1: u32 = 3;
const T0: u32 = 8;
const T1: u32 = 9;
const T2: u32 = 10;
const T3: u32 = 11;
const T4: u32 = 12;
const T5: u32 = 13;
const T6: u32 = 14;
const T7: u32 = 15;
const S0: u32 = 16;
const S1: u32 = 17;
const S2: u32 = 18;
const S3: u32 = 19;
const S4: u32 = 20;
const S5: u32 = 21;
const S6: u32 = 22;
const S7: u32 = 23;
const GP: u32 = 28;
const SP: u32 = 29;
const RA: u32 = 31;

/// BIOS A-table dispatcher entry. Calling it with the function number in `$t1`
/// invokes that A-function (the game's own FlushCache wrapper at `0x8005BBE8`
/// does `li t2,0xA0; jr t2; li t1,0x44`).
pub const BIOS_DISPATCH_A: u16 = 0x00A0;
/// BIOS A-function number for `FlushCache()` (invalidate the I-cache).
pub const FLUSH_CACHE_FN: u16 = 0x0044;

// --- Option 1: the game's own loader via the mode-24 minigame round-trip -------
//
// The freeze in the raw-CD-read slice was reentrancy: a blocking FUN_8005E4D4
// issued INSIDE the field-VM tick. The safe path is the field-VM op-0x3E
// minigame door-warp (same dispatcher FUN_801DE840): it just REQUESTS mode 24,
// and the mode-INIT handler FUN_80025980 does the CD load between frames (CD
// idle), runs the slot-A overlay, then warps back to field. Our shop trigger
// MIRRORS the op-0x3E warp arm (0x801E078C) so the load happens in that safe
// mode context. Globals (all relative to `lui 0x8008` = 0x80080000):

/// Master game-mode index (u16). The main-loop dispatcher reads it each frame to
/// pick the mode-table (`0x8007078C`) handler. Writing it = requesting a mode.
pub const MODE_INDEX_VA: u32 = 0x8007_B83C;
/// Mode 24 = OTHER INIT (`FUN_80025980`, the minigame loader/round-trip handler).
pub const MODE_OTHER_INIT: u16 = 0x18;
/// Minigame sub-id consumed by `FUN_80025980` (`switch(_DAT_8007BA34)`); selects
/// which slot-A overlay it loads (`FUN_8003EBE4(sub_id + 0x4D)`) + which init.
pub const SUBID_VA: u32 = 0x8007_BA34;
/// Minigame session-winnings accumulator, zeroed on warp (op-0x3E housekeeping).
pub const WINNINGS_VA: u32 = 0x8008_4440;
/// Second housekeeping word zeroed on the op-0x3E warp.
pub const WARP_HOUSEKEEP_VA: u32 = 0x8007_BAC0;
/// `FUN_8003CE08(0xE)` — SysFlag set (field-VM 4th flag bank), called by the warp.
pub const SYSFLAG_SET_FN: u32 = 0x8003_CE08;
/// `a0` for [`SYSFLAG_SET_FN`] in the warp idiom.
pub const SYSFLAG_WARP_ARG: u16 = 0x000E;

// --- Dead dev-mode hosting (the chosen option-1 fork) --------------------------
//
// Rather than a minigame sub-id, the trade overlay is hosted by REPURPOSING an
// unused dev TEST game-mode: its mode-table INIT handler is repointed at our
// own mode-INIT loader in the gap. The op-0x49 shop detour just REQUESTS that
// mode (writes the master mode index); next frame the mode SM calls our loader
// in the SAFE between-frames context (CD idle), where the proven FUN_8005E4D4
// + FlushCache + overlay-exec runs without the mid-tick reentrancy that froze
// the raw slice. The loader then requests field mode again to return. Keeps all
// 7 minigames; the only SCUS edits are one mode-table handler word + the gap.

/// Mode-table base VA (`crate`-local mirror of `legaia_asset::mode_table`).
pub const MODE_TABLE_VA: u32 = 0x8007_078C;
/// Mode-table entry stride (bytes).
pub const MODE_ENTRY_STRIDE: u32 = 24;
/// Offset of the handler pointer within a mode-table entry.
pub const MODE_HANDLER_OFFSET: u32 = 0x10;
/// The dead dev-mode we repurpose: index 10 = "TEST TEST" (init), unreachable in
/// retail play. Its handler word is repointed at [`MODE_INIT_VA`].
pub const DEAD_MODE_INDEX: u16 = 10;
/// The recognized-build handler at [`DEAD_MODE_INDEX`] ("TEST TEST" init,
/// `0x8002B97C`). Guards the mode-table patch against an unexpected build.
pub const DEAD_MODE_HANDLER_ORIG: u32 = 0x8002_B97C;
/// Scratch cell (gap tail, after the sentinel) where the trigger stashes the
/// mode index it interrupted, so the loader restores it instead of hardcoding a
/// return mode. Forcing a fixed mode (3=field) froze when triggered from a
/// non-field context (cold-boot name-entry has no field scene loaded yet).
pub const ORIGIN_MODE_VA: u32 = 0x8007_AF24;

/// VA of the op-0x49 mode-request trigger (reached by the detour's `j STUB_VA`).
/// Shares the stub window base; the mode-INIT loader sits just above it.
pub const TRIGGER_VA: u32 = STUB_VA;
/// VA of the mode-INIT loader (what the repurposed mode-table handler points at).
/// 0x40 above [`TRIGGER_VA`] — past the trigger, still in the gap window.
pub const MODE_INIT_VA: u32 = STUB_VA + 0x40;

/// VA of the dead mode's handler word in `SCUS_942.54` (what we overwrite with
/// [`MODE_INIT_VA`] so the mode SM calls our loader).
pub const fn dead_mode_handler_va() -> u32 {
    MODE_TABLE_VA + (DEAD_MODE_INDEX as u32) * MODE_ENTRY_STRIDE + MODE_HANDLER_OFFSET
}

// --- Mode-24 WARP hosting (Fork A: new sub-id; the chosen robust path) ----------
//
// The robust path: the op-0x49 shop detour mirrors the op-0x3E minigame warp
// (set sub-id + mode 0x18; [`assemble_warp_trigger_stub`]). The game's mode-24
// INIT `FUN_80025980` then tears the field down, loads the slot-A overlay, runs
// it, and on exit `FUN_80026018` restores + RELOADS the scene (mode 2) — a clean
// teardown+reload, unlike resume-in-place. We host a NEW sub-id by detouring
// FUN_80025980's per-sub-id overlay-load call to a gap REDIRECT that, for our
// sub-id, baked-LBA-loads our pochi slot to slot A and runs it (then returns to
// field via FUN_80026018), else does the original load.

/// Our new mode-24 sub-id (past the 7 retail minigames, ids 0..=6).
pub const WARP_SUBID: u16 = 7;
/// `_DAT_8007BA34` minigame sub-id (alias of [`SUBID_VA`], for readability here).
pub const WARP_SUBID_VA: u32 = SUBID_VA;
/// Slot-A overlay load base (where `FUN_80025980` loads the per-mode overlay).
pub const SLOT_A_BASE: u32 = 0x801C_E818;
/// `FUN_80025980`'s per-sub-id overlay-load call site (`jal 0x8003EBE4`); we
/// detour it to [`WARP_REDIRECT_VA`].
pub const WARP_INIT_DETOUR_VA: u32 = 0x8002_5A30;
/// Where the redirect rejoins `FUN_80025980` for non-our sub-ids (`jal FUN_8003DE7C`).
pub const WARP_INIT_REJOIN_VA: u32 = 0x8002_5A38;
/// The two displaced instructions at [`WARP_INIT_DETOUR_VA`]: `jal 0x8003EBE4`
/// then `addu a1,zero,zero`. Recognized-build guard.
pub const WARP_INIT_DISPLACED: [u32; 2] = [0x0C00_FAF9, 0x0000_2821];
/// The game's slot-A overlay loader `FUN_8003EBE4` (replayed for non-our sub-ids).
pub const OVERLAY_LOADER_A_FN: u32 = 0x8003_EBE4;
/// `FUN_80026018` — the mode-24 return warp (restore scene + mode 2 reload).
pub const MODE24_RETURN_FN: u32 = 0x8002_6018;
/// `FUN_80025980` stack-frame layout (so the redirect can return cleanly,
/// bypassing the function's mode-0x19 epilogue): `lw ra,0x14(sp)`, `lw s0,0x10(sp)`,
/// `addiu sp,sp,0x18`.
pub const WARP_INIT_FRAME: u16 = 0x18;
pub const WARP_INIT_RA_OFF: u16 = 0x14;
pub const WARP_INIT_S0_OFF: u16 = 0x10;

/// Gap-tail "warp already fired" flag (u16). With fire-once enabled, the trigger
/// warps a single time then lets the interrupted menu proceed — used for the
/// name-entry sentinel test, where the mode-2 field reload re-runs the name-entry
/// op-0x49 and would otherwise re-trigger the warp forever (black-screen <->
/// Vahn oscillation). Not used by the real shop feature (shops don't auto-retrigger).
pub const WARP_FIRED_VA: u32 = 0x8007_AF28;

/// VA of the warp trigger (op-0x49 detour target). Reuses [`TRIGGER_VA`].
pub const WARP_TRIGGER_VA: u32 = TRIGGER_VA;
/// VA of the `FUN_80025980` redirect routine (gap; past the warp trigger; leaves
/// room for the larger fire-once trigger).
pub const WARP_REDIRECT_VA: u32 = STUB_VA + 0x60;

// --- Draw side: a self-drawing slot-A overlay -----------------------------------
//
// The overlay is loaded to SLOT_A_BASE. Its INIT (offset 0, jalr'd by the
// redirect) hands off to mode 13 (MAPDSIP MODE, `FUN_80025f2c`), whose per-frame
// handler calls `func_0x801ce850` = SLOT_A_BASE + 0x38 every frame — so the
// overlay's TICK lives at offset 0x38 and draws there. The general per-frame
// updater (`FUN_80016444`, also run by `FUN_80025f2c`) flushes the queued text.

/// Game-mode index 13 (MAPDSIP MODE): per-frame handler `FUN_80025f2c` calls the
/// slot-A overlay tick at `SLOT_A_BASE + 0x38` directly each frame — our draw loop.
pub const MAPDISP_MODE_INDEX: u16 = 13;
/// Native menu text drawer `FUN_80036888(a0=str, a1=0, a2=maxchars(0=all), a3=x,
/// [sp+0x10]=y)` — the game's own dialog-font renderer (decodes the ASCII string
/// via `FUN_80036514`, control codes `^`=icon / `0xFF`=color). SCUS-resident, so
/// callable from our slot-A overlay. Renders in the native font, unlike the debug
/// drawer `FUN_8001AA68`. NOTE the 5th arg (y) is passed on the stack at `sp+0x10`.
pub const TEXT_DRAW_FN: u32 = 0x8003_6888;
/// Offset of the per-frame tick within a slot-A overlay (what `FUN_80025f2c`
/// calls: `func_0x801ce850` = `SLOT_A_BASE + 0x38`).
pub const SLOT_A_TICK_OFFSET: u32 = 0x38;
/// Lead character record base (`0x80084708 + n*0x414`, n=0 = the party lead).
/// Seru-magic list: count at `+0x13C`, ids at `+0x13D[]`; displayed level `+0x130`.
pub const CHAR_RECORD_BASE: u32 = 0x8008_4708;
/// Dump start offset within the record. Starts at the level (`+0x130`, nonzero
/// even at name-entry — proves the live read) and spans through the seru count
/// (`+0x13C`) + first ids (`+0x13D..`), so the seru list shows once learned.
pub const SERU_DUMP_OFFSET: u16 = 0x130;
/// Bytes to hex-dump from the record (level + a few following bytes; kept short
/// so the line fits inside the window box). The real list will replace this.
pub const SERU_DUMP_LEN: u16 = 6;

/// Learnable-seru count in the character record (`+0x13C`, u8).
pub const SERU_COUNT_VA: u32 = CHAR_RECORD_BASE + 0x13C;
/// Learnable-seru id array in the character record (`+0x13D`, u8[count]).
pub const SERU_IDS_VA: u32 = CHAR_RECORD_BASE + 0x13D;
/// Spell display-name pointer table (`DAT_800754D0`, 12-byte stride): the name
/// pointer for spell id `n` is `*(SERU_NAME_PTRS + n*0xC)`. SCUS rodata; the names
/// are ASCII, drawn by [`TEXT_DRAW_FN`].
pub const SERU_NAME_PTRS: u32 = 0x8007_54D0;
/// Max seru rows drawn in the window (fits the box; scrolling comes later).
pub const SERU_MAX_ROWS: u16 = 6;

/// Validation aid for the per-owner handler. When `true` the handler forces the
/// bucket's wanted seru to [`SERU_DEMO_BASE_ID`] (and the give-back to
/// `SERU_DEMO_BASE_ID + 1`) instead of reading the precomputed [`BUCKET_TABLE_VA`],
/// so a save where any party member owns that fixed id lists a trade line without
/// having to align the play-time bucket. `false` = the live table-driven want.
/// (It cannot conjure a line on a *fresh* save — the per-owner render is empty when
/// nobody owns the want, which is correct behaviour, not a render bug.)
pub const SERU_DEMO_FORCE_WANT: bool = false;
/// The fixed want id used when [`SERU_DEMO_FORCE_WANT`]. `0x81` = Gimard, the first
/// player Seru-magic id; `+1` (`0x82`) is the forced give-back.
pub const SERU_DEMO_BASE_ID: u16 = 0x81;

/// Native window/box-frame draw `FUN_8002C69C(a0=x, a1=y, a2=w, a3=h)` (POLY_FT4
/// / SPRT emitter — the dialog/menu window). SCUS-resident; 4 register args.
pub const BOX_FN: u32 = 0x8002_C69C;
/// Window-skin selector global `gp[+`[`WINDOW_SKIN_OFF`]`]` (set by `FUN_80034b6c`,
/// read by [`BOX_FN`]): a skin index into the corner/edge table `DAT_800732a4`
/// (12-byte stride). The per-frame finalize `FUN_80031d00` rewrites it from each
/// drawn window's `+0x1d` skin byte, so it holds the *last* window's skin — which,
/// once the blue picker windows slide away, is the brown name-plate's special skin
/// (`0x31`), rendering our box as a brown name-plate. We force [`WINDOW_SKIN_STD`]
/// (index 0 = the standard box; the blue fill is hardcoded regardless of index)
/// right before our box draw via `sw zero, WINDOW_SKIN_OFF($gp)`.
pub const WINDOW_SKIN_OFF: u16 = 0x14C;
/// Standard menu-box skin index (`DAT_800732a4[0]`; indices 0/3/4 are identical).
pub const WINDOW_SKIN_STD: u16 = 0;

// --- Trade box geometry + slide-in animation ----------------------------------
//
// The box + its text are positioned below the gold/name boxes (which sit at the top)
// and animate in horizontally from the left, mirroring the shop's window slide. The
// slide is self-driven (the window manager only animates its own windows, not our
// raw box): a persistent signed x-offset in [`TRADE_SLIDE_DELTA_VA`] starts at
// [`SLIDE_START_OFF`] (box fully off the left edge) and steps toward 0 by
// [`SLIDE_STEP`] each frame; the offset is added to the box x and every text x.

/// Box top-left x / y and size. `y=0x40` sits just below the gold + vendor-name
/// boxes (no overlap, minimal black gap above); `0xC8 × 0x90` ends at y=0xD0 (208),
/// inside the NTSC safe area, with headroom below the current rows to grow.
pub const BOX_X: u16 = 0x28;
pub const BOX_Y: u16 = 0x40;
pub const BOX_W: u16 = 0xC8;
pub const BOX_H_PX: u16 = 0x90;
/// Text columns (relative to screen, before the slide offset): reward header /
/// per-owner want name / owner name / level number.
pub const COL_HEADER_X: u16 = 0x30;
pub const COL_WANT_X: u16 = 0x40;
pub const COL_OWNER_X: u16 = 0x80;
pub const COL_LEVEL_X: u16 = 0xB0;
/// Row baselines: reward header y, first per-owner row y, and per-row y advance.
/// Tucked just inside the box top (`BOX_Y + 0xC`).
pub const ROW_HEADER_Y: u16 = 0x4C;
pub const ROW_FIRST_Y: u16 = 0x5C;
pub const ROW_STEP_Y: u16 = 0x10;
/// Persistent slide x-offset cell (SCUS gap, resident). The dispatch stub resets it
/// to [`SLIDE_START_OFF`] on Trade confirm; the handler steps it toward 0 each frame.
pub const TRADE_SLIDE_DELTA_VA: u32 = 0x8007_AEC4;
/// Cursor index over the per-owner trade lines (0 = first line). SCUS gap, resident;
/// the handler nav-clamps it to `[0, line_count)` each frame.
pub const TRADE_CURSOR_VA: u32 = 0x8007_AEC8;
/// Previous-frame pad mask, for D-pad edge detection (one step per press, not per
/// held frame). SCUS gap, resident.
pub const TRADE_PAD_PREV_VA: u32 = 0x8007_AECC;
/// D-pad bits in [`PAD_CUR_VA`] (built by `FUN_8001822C`): Up / Down move the line
/// cursor; Left / Right pick Yes / No in the confirm sub-state.
pub const PAD_UP_MASK: u16 = 0x1000;
pub const PAD_DOWN_MASK: u16 = 0x4000;
pub const PAD_LEFT_MASK: u16 = 0x8000;
pub const PAD_RIGHT_MASK: u16 = 0x2000;
/// Face button ✕ (CONFIRM) in [`PAD_CUR_VA`] (the low byte; ○ = [`HANDLER_CANCEL_MASK`]).
pub const PAD_CONFIRM_MASK: u16 = 0x0040;

/// Confirm sub-state cell (0 = browsing the owner lines, 1 = the Yes/No prompt for
/// the selected line). SCUS gap, resident.
pub const TRADE_CONFIRM_VA: u32 = 0x8007_AED0;
/// Yes/No selection in the confirm sub-state (0 = Yes, 1 = No). SCUS gap, resident.
pub const TRADE_YESNO_VA: u32 = 0x8007_AED4;
/// The current offer's give-back id, stashed by the offer compute so the per-owner
/// loop can skip owners who already own it (a pointless trade). SCUS gap, resident.
pub const TRADE_GIVE_ID_VA: u32 = 0x8007_AED8;

/// Confirm-prompt strings, embedded in 0899 above the handler (resident in-shop).
/// Drawn only in the confirm sub-state; the selected line + reward header already
/// show *what* is being traded, so the prompt just needs the question + choices.
pub const CONFIRM_PROMPT_STR_VA: u32 = 0x801E_7C00;
pub const CONFIRM_PROMPT_STR: &[u8] = b"@Trade?\0";
pub const CONFIRM_YES_STR_VA: u32 = 0x801E_7C10;
pub const CONFIRM_YES_STR: &[u8] = b"@Yes\0";
pub const CONFIRM_NO_STR_VA: u32 = 0x801E_7C18;
pub const CONFIRM_NO_STR: &[u8] = b"@No\0";
/// Confirm-prompt layout (inside the box, near its bottom): question row y, choices
/// row y, and the Yes / No / cursor x columns.
pub const PROMPT_Y: u16 = 0xB4;
pub const CHOICE_Y: u16 = 0xC4;
pub const YES_X: u16 = 0x40;
pub const NO_X: u16 = 0x80;
/// Cursor x just left of a Yes/No choice (re-uses the line-cursor sprite).
pub const CHOICE_CURSOR_DX: u16 = 0x10;
/// Native animated cursor sprite `FUN_8002b994(slot, mode, x, y)` — a 16×16 bobbing
/// cursor (slot 0 = the standard menu cursor; `mode = 1` animates). Drawn at the
/// selected owner line. (Already used by the row-4 stub as [`HIGHLIGHT_FN`].)
pub const CURSOR_DRAW_FN: u32 = 0x8002_B994;
/// Cursor sprite x: just left of the per-owner want column.
pub const CURSOR_X: u16 = COL_WANT_X - 0x10;
/// Initial slide offset: box fully off the left edge (`-(BOX_X + BOX_W)` rounded).
pub const SLIDE_START_OFF: i16 = -0xF0;
/// Per-frame slide step toward 0. Divides `SLIDE_START_OFF` evenly so the box lands
/// exactly on 0 (no overshoot) — see the compile-time asserts below.
pub const SLIDE_STEP: u16 = 0x18;

// Compile-time invariants the hand-assembled handler relies on:
// - the slide step divides the start offset evenly (the box lands exactly on 0);
// - the row pitch is 0x10 so the cursor's line-count divide is a single `srl 4`.
const _: () = assert!((-(SLIDE_START_OFF as i32)) % (SLIDE_STEP as i32) == 0);
const _: () = assert!(ROW_STEP_Y == 0x10);

/// Per-frame button mask (1 = pressed), built by `FUN_8001822C`. Standard PSX
/// bits: UP 0x10, DOWN 0x40, START 0x08, TRIANGLE 0x1000, CIRCLE 0x2000,
/// CROSS 0x4000, SQUARE 0x8000. Read here to drive overlay input.
pub const PAD_CUR_VA: u32 = 0x8007_B850;
/// Previous-frame button mask (for edge detection later, when we add a cursor).
pub const PAD_PREV_VA: u32 = 0x8007_B7C0;
/// Button that exits the overlay back to the field (CROSS / ✕).
pub const PAD_EXIT_MASK: u16 = 0x4000;
/// Pad-poll `FUN_8001822C`: reads the BIOS pad (`0x800840F8`) and rebuilds the
/// active-high mask at [`PAD_CUR_VA`] (+ prev at [`PAD_PREV_VA`]). NOT called in
/// our hijacked mode 13, so the overlay calls it itself each tick to get a live
/// pad (otherwise [`PAD_CUR_VA`] reads stale -> dead input + spurious exit loop).
pub const PAD_POLL_FN: u32 = 0x8001_822C;

/// Frames to keep the draw overlay up before auto-returning to the field (this
/// first increment has no input yet). Held long (~30s at 60fps) so the draw
/// window can be observed + saved during; each tick also writes the live frame
/// counter to [`SENTINEL_ADDR`] as a heartbeat (proves the tick is running, even
/// if nothing is visible — distinguishes "tick ran, draw invisible" from "tick
/// never ran").
pub const DRAW_HOLD_FRAMES: u16 = 0x7FFF;

// --- Shop-menu (Buy/Sell/Quit picker) trigger ------------------------------
//
// The op-0x49 *arm* trigger fires in the same frame the shop spawns its menu
// actor, whose tick sets shop mode 0x17 (`_DAT_8007B83C`) AFTER our mode-0x18
// request, overriding it before the dispatcher reaches mode 24 -> the warp never
// runs at a merchant. The picker RENDERER `FUN_801d4868` (overlay 0899, mode
// 0x17) instead runs every frame the *settled* Buy/Sell/Quit choice is on
// screen, with no competing mode transition that frame -- the correct quiet-
// frame hook. This detour proves whether a mode-0x18 request issued from a
// settled shop frame STICKS (vs. the shop re-asserting 0x17 each frame): it arms
// the same mode-24 warp on a button the picker ignores (SQUARE).
//
/// Menu/shop overlay PROT entry (slot A, base [`SLOT_A_BASE`]).
pub const PICKER_MENU_PROT_INDEX: usize = 899;
/// VA of the Buy/Sell/Quit picker renderer `FUN_801d4868` (overlay 0899).
pub const PICKER_RENDER_VA: u32 = 0x801D_4868;
/// Return point: picker body resumes at `+8` (after the two displaced words).
pub const PICKER_RETURN_VA: u32 = PICKER_RENDER_VA + 8;
/// The two displaced instructions at [`PICKER_RENDER_VA`]: `addiu sp,sp,-0x28`
/// then `sw s1,0x1c(sp)` (the function prologue head), replayed by the stub.
pub const PICKER_DISPLACED: [u32; 2] = [0x27BD_FFD8, 0xAFB1_001C];
/// Gap VA of the picker trigger stub. Reuses the op-0x49 trigger slot
/// ([`STUB_VA`]) -- the two triggers never coexist (this build replaces the
/// op-0x49 detour). Sized to exactly 0x60 bytes so it abuts the redirect at
/// [`WARP_REDIRECT_VA`] (`STUB_VA + 0x60`) without overlap.
pub const PICKER_TRIGGER_VA: u32 = STUB_VA;
/// Pad mask the picker ignores, used to arm the trade warp (SQUARE / □).
pub const PICKER_TRIGGER_MASK: u16 = 0x8000;

/// Words planted at [`PICKER_RENDER_VA`]: `j PICKER_TRIGGER_VA` then `nop`
/// (replacing the prologue head, which the stub replays).
pub fn picker_detour_words() -> [u32; 2] {
    [j(PICKER_TRIGGER_VA), nop()]
}

// --- Native fourth "Trade" row (trigger-agnostic) --------------------------
//
// Two byte-verified edits to overlay 0899 make the Buy/Sell/Quit menu a
// Buy/Sell/Quit/Trade menu, in the game's own style: (1) bump the picker cursor
// clamp 3 -> 4 in the dispatcher `FUN_801dafd4` so the cursor can reach a fourth
// row; (2) detour the renderer `FUN_801d4868` epilogue to draw + highlight that
// row exactly as the other three (native dialog font + cursor sprite). Selecting
// it is a clean no-op for now -- index-3 confirm already falls through to the
// dispatcher's normal exit; the action (arm the trade screen) is the one piece
// deferred until the SQUARE test confirms how Trade should dispatch.

/// Picker cursor clamp arg site in `FUN_801dafd4`: `li a1,0x3` -> `li a1,0x4`.
pub const CLAMP_VA: u32 = 0x801D_B098;
/// The recognized instruction at [`CLAMP_VA`] (`addiu a1,zero,3`).
pub const CLAMP_OLD: u32 = 0x2405_0003;
/// Its replacement (`addiu a1,zero,4`).
pub const CLAMP_NEW: u32 = 0x2405_0004;

/// In-body detour site in `FUN_801d4868`, immediately AFTER the Quit text draw
/// (`jal FUN_80036888; sw s0,0x10(sp)` at 0x801d4a0c/10) and before the Quit
/// highlight logic. Drawing the 4th row here links its glyphs at the SAME OT
/// depth as Buy/Sell/Quit (in front of the box background) — an epilogue draw
/// instead links them behind the box and they never show.
pub const ROW4_DETOUR_VA: u32 = 0x801D_4A14;
/// Rejoin: the Quit-highlight logic (a `nop`, then `andi v0,a1,0x4000`).
pub const ROW4_RETURN_VA: u32 = 0x801D_4A1C;
/// The two displaced words at [`ROW4_DETOUR_VA`]: `lui v0,0x801e` then
/// `lw a1,0x46bc(v0)` (loads the cursor word for the Quit highlight), replayed by
/// the stub before it rejoins.
pub const ROW4_DISPLACED: [u32; 2] = [0x3C02_801E, 0x8C45_46BC];
/// Picker window-tile sprite definition (`DAT_801e4738 + 0x2a*0x10`): the
/// Buy/Sell/Quit box. Layout `[u16 flags][u16 kind][u16 x][u16 y][u16 w][u16 h]
/// [u32 draw-handler]` — here x=0x2a, y=0x2e, w=0x50, h=0x26, handler=FUN_801d4868
/// (this is how the renderer is invoked: as the window's content callback).
pub const PICKER_BOX_DEF_VA: u32 = 0x801E_49D8;
/// VA of the box height field (`+0xa`). Grown by one row so the 4th "Trade" row
/// sits inside the frame instead of below it.
pub const BOX_H_VA: u32 = PICKER_BOX_DEF_VA + 0xA;
/// The recognized 3-row box height.
pub const BOX_H_OLD: u16 = 0x26;
/// The 4-row box height (`+0xe`, one row taller).
pub const BOX_H_NEW: u16 = 0x34;

// --- Trade-confirm dispatch (FUN_801dafd4) + combined-build gap layout ---------
//
// On confirming the Trade row, arm the mode-24 warp from the dispatcher (a settled
// shop frame -- mode-stick is confirmed). The combined "trade" build needs three
// gap routines that must coexist: the row-4 draw stub (self-referential, stays at
// [`ROW4_STUB_VA`] = 0x8007AE00) plus the position-independent redirect + dispatch
// stub, placed in the LOWER gap (0x8007AB38..) so they don't collide. (Standalone
// seru-trade build; bonus-drop/flee-exp are off, so that lower window is free.)

// IN-SHOP design (no warp): the Trade screen runs as a picker SUB-MODE inside mode
// 0x17. `FUN_801dafd4` (the picker, menu-state table[0x1a]) dispatches its own
// sub-state var `DAT_801e46ac` (0=init/1=input/2=…); value 3 is an unused no-op we
// claim for the trade screen. Confirming the Trade row sets sub-mode 3; an entry
// detour routes sub-mode 3 to a SCUS handler that draws + swaps + returns to the
// picker (sub-mode 1). The shop is never torn down → clean return-to-shop.

/// Picker sub-state var (`DAT_801e46ac`), dispatched by `FUN_801dafd4`.
pub const SUBSTATE_VA: u32 = 0x801E_46AC;
/// Sub-mode value claimed for the trade screen (unused no-op in the retail build).
pub const TRADE_SUBMODE: u16 = 3;
/// Sub-mode the trade screen returns to on exit (the picker's input state).
pub const PICKER_INPUT_SUBMODE: u16 = 1;

/// Reorder: the body's row-2 text-load (`lui a0,0x801d; addiu a0,a0,-0x145c` =
/// "@Quit") is repointed to "@Trade", so row 2 shows Trade; the row-4 stub then
/// draws "@Quit" at row 3 → Buy/Sell/Trade/Quit.
pub const ROW2_STR_LOAD_VA: u32 = 0x801D_49F8;
/// The recognized two words at [`ROW2_STR_LOAD_VA`] (`lui a0,0x801d`/`addiu a0,…`).
pub const ROW2_STR_LOAD_OLD: [u32; 2] = [0x3C04_801D, 0x2484_EBA4];
/// "@Quit" overlay string (drawn at row 3 by the row-4 stub after the reorder).
pub const QUIT_STR_VA: u32 = 0x801C_EBA4;

/// Confirm-dispatch detour site in `FUN_801dafd4`: `bne a0,v0,0x801db0e8` (the
/// cursor==Quit check, v0=2), reached for every confirm. a0 = cursor index.
pub const TRADE_DISPATCH_VA: u32 = 0x801D_B0C8;
/// The two displaced words at [`TRADE_DISPATCH_VA`]: `bne a0,v0,0x801db0e8` (off 7)
/// then its `nop` delay slot.
pub const TRADE_DISPATCH_DISPLACED: [u32; 2] = [0x1482_0007, 0x0000_0000];
/// Buy/Sell branch target (cursor 0/1 fall here in the dispatcher).
pub const BUY_SELL_CHECK_VA: u32 = 0x801D_B0E8;
/// The original Quit action (sound + exit), reused for the reordered Quit row.
pub const QUIT_CODE_VA: u32 = 0x801D_B0D0;
/// Dispatcher exit (shared `jal 0x80031d00` tail).
pub const TRADE_EXIT_VA: u32 = 0x801D_B200;

// --- Native window-slide (reuse the shop's own widget scripts) -----------------
//
// The shop windows are actor-VM widget scripts: `FUN_801d6628(&script)` interprets
// 4-byte commands `[opcode, window_idx, p0, p1]` (terminator opcode `0`) over the
// window table at `0x801e4738`. Opcode 1 = open/slide-in, opcode 4 = close/slide-
// away. The Sell transition runs `DAT_801e4e54` = close {0x28, 0x2a (picker), 0x22},
// leaving the gold (0x20) + vendor-name (0x21) boxes — exactly the "slide the menus
// away, keep gold + name" effect. We reuse that on Trade entry, and the full open
// script `DAT_801e4e38` (opens 0x21/0x2a/0x20/0x28/0x22) to slide them back on exit.

/// Widget-script VM `FUN_801d6628(a0 = &script)` (overlay 0899, resident in-shop).
pub const WIDGET_VM_FN: u32 = 0x801D_6628;
/// The Sell slide-AWAY script `DAT_801e4e54`: close windows 0x28 / 0x2a / 0x22.
pub const SLIDE_AWAY_SCRIPT_VA: u32 = 0x801E_4E54;
/// The picker OPEN script `DAT_801e4e38`: (re)open windows 0x21/0x2a/0x20/0x28/0x22.
pub const SLIDE_OPEN_SCRIPT_VA: u32 = 0x801E_4E38;

/// `FUN_801dafd4` entry detour: route sub-mode 3 to the trade handler.
pub const ENTRY_VA: u32 = 0x801D_AFD4;
/// Rejoin after the replayed prologue (`lui s1,0x801e`).
pub const ENTRY_RETURN_VA: u32 = 0x801D_AFDC;
/// The two displaced prologue words at [`ENTRY_VA`]: `addiu sp,sp,-0x20`/`sw s1,0x14(sp)`.
pub const ENTRY_DISPLACED: [u32; 2] = [0x27BD_FFE0, 0xAFB1_0014];
/// Menu per-frame finalize (`FUN_801dafd4`'s tail call), replicated by the handler.
pub const FINALIZE_FN: u32 = 0x8003_1D00;
/// Trade-screen exit button = ○ / CANCEL. In `PAD_CUR` (0x8007b850, built by
/// `FUN_8001822C` as `~CONCAT11(rawbyte0,rawbyte1)`) the FACE buttons are the low
/// byte: △=0x10, ○=0x20, ✕=0x40, □=0x80 (the D-pad is the high byte: Up=0x1000,
/// Right=0x2000, Down=0x4000, Left=0x8000). So ○ = 0x20 — NOT 0x2000 (= Right) and
/// NOT ✕ (0x40 = CONFIRM, which opens Trade).
pub const HANDLER_CANCEL_MASK: u16 = 0x0020;
/// Private "trade screen active" flag in dead gap space. The dispatch stub sets it,
/// the entry detour gates on it, the handler clears it on exit. We DON'T reuse the
/// picker sub-state `DAT_801e46ac`: the menu owns it — `FUN_801dc6b4` case 2 resets
/// it (`DAT_801e46ac = 0`) whenever its draw-state shadow desyncs, wiping our value.
/// Lives in the upper row-4 gap tail (past `@Trade`, below the config blob at
/// `0x8007AF00`) so the handler body can grow downward from `0x8007ABB0` without
/// stepping on it.
pub const TRADE_ACTIVE_VA: u32 = 0x8007_AEC0;

/// Gap VAs (lower gap, free in the standalone build). Self-referential row-4 stub
/// stays at [`ROW4_STUB_VA`] (0x8007AE00); the rest are position-independent.
pub const ENTRY_STUB_VA: u32 = 0x8007_AB38;
/// Reorder dispatch stub (cursor 2 → Trade sub-mode, 3 → Quit, 0/1 → Buy/Sell).
pub const TRADE_DISPATCH_STUB_VA: u32 = 0x8007_AB68;
/// The in-shop trade-screen handler (draws + input + swap; runs in mode 0x17).
///
/// HOSTED IN THE MENU OVERLAY 0899, not the tiny SCUS rodata gap: the gap only had
/// ~116 words free, far too little for the full screen (slide + cursor + confirm +
/// swap). 0899 is the overlay that hosts the shop and is resident throughout it, and
/// it carries a large reference-free zero region (run-C, file `0x18CC7`, VA
/// `0x801E74DF`, ~0xF00 bytes) that is part of the loaded image (reloaded with the
/// overlay) and verified all-zero across the trade screen + both slide-transition
/// states — so the handler embeds there, resident during every shop, with ~960 words
/// of room and no runtime CD load. The entry detour (in the SCUS gap) `j`s here.
pub const TRADE_HANDLER_VA: u32 = 0x801E_74E0;
/// Upper bound the handler body must stay below (end of the 0899 run-C dead region,
/// `0x801E83E2`, rounded down with a small margin).
pub const TRADE_HANDLER_END: u32 = 0x801E_83E0;
/// PROT entry hosting the handler (the menu overlay — same entry the picker edits
/// target). Its load base is [`SLOT_A_BASE`]; the handler's file offset is
/// `TRADE_HANDLER_VA - SLOT_A_BASE`.
pub const HANDLER_OVL_PROT_INDEX: usize = PICKER_MENU_PROT_INDEX;

/// The randomizer's precomputed vendor schedule (one `[want_id, give_id]` pair per
/// time bucket; see [`legaia_asset::seru_trade::bucket_offers`] /
/// [`legaia_asset::seru_trade::bucket_table_to_bytes`]). `[want, give, give_level]`
/// per entry = [`legaia_asset::seru_trade::BUCKET_TABLE_LEN`] (192) bytes, placed in
/// the freed handler region of the SCUS rodata gap (the handler now lives in 0899),
/// above the dispatch stub and below the old handler end; resident always, so the
/// 0899-hosted handler reads it by absolute address. The handler indexes it by
/// `(play_time / `[`RESEED_PERIOD_FRAMES`]`) & `[`BUCKET_INDEX_MASK`]`, ×3`.
pub const BUCKET_TABLE_VA: u32 = 0x8007_AC00;
/// Byte length of the on-disc bucket schedule (mirrors the shared kernel).
pub const BUCKET_TABLE_LEN: usize = legaia_asset::seru_trade::BUCKET_TABLE_LEN;

/// Retail play-time counter `_DAT_80084570` (u32 game-time seconds). The handler
/// reads it to pick the current bucket; the engine mirrors it as
/// `World::play_time_seconds`.
pub const PLAY_TIME_VA: u32 = 0x8008_4570;
/// Reseed period in **play-time ticks** (the unit of `_DAT_80084570`). HW-pinned:
/// the counter advances ~per-frame (≈60/s), NOT per-second as the memory-map label
/// suggests — a maxed save read `0x80084570 ≈ 10.4M`, which is ~48 h at 60/s, not
/// the absurd ~2900 h it would be at 1/s. So the kernel's seconds-based
/// `SECONDS_PER_RESEED` (engine-facing) does NOT apply here; the handler divides the
/// frame counter by this. `32400` ticks ≈ 9 minutes at 60/s, and fits a single
/// `addiu` immediate (`0x7E90 < 0x8000`); the full 64-bucket cycle is then ~9.6 h.
pub const RESEED_PERIOD_FRAMES: u16 = 32400;
/// Mask folding the raw bucket into the precomputed schedule
/// (`BUCKET_COUNT - 1`; [`BUCKET_COUNT`](legaia_asset::seru_trade::BUCKET_COUNT) is a
/// power of two, so the runtime modulo is a single `andi`).
pub const BUCKET_INDEX_MASK: u16 = (legaia_asset::seru_trade::BUCKET_COUNT as u16) - 1;

/// Character-record stride (`0x80084708 + slot*0x414`); mirrors
/// `legaia_save::character`.
pub const CHAR_RECORD_STRIDE: u16 = 0x414;
/// Party roster slots the handler scans for owners of the wanted seru.
pub const PARTY_SLOT_COUNT: i16 = 4;
/// Seru-level array offset in the character record (`+0x161[36]`, parallel to the
/// id array at [`SERU_IDS_VA`]); the `LVL n` shown per trade line.
pub const SERU_LEVELS_OFFSET: u16 = 0x161;
/// Display-name offset in the character record (`+0x2A7`); the owner name per line.
pub const RECORD_NAME_OFFSET: u16 = 0x2A7;
/// Native monospaced base-10 number formatter `FUN_80034b78(value, min_digits, x, y)`
/// — draws the per-line `LVL` value. SCUS-resident; 4 register args.
pub const NUMBER_FN: u32 = 0x8003_4B78;
/// "SERU TRADE" title string for the handler. Relocated to the upper row-4 gap tail
/// (past `@Trade` at [`TRADE_STR_VA`], below the config blob) to free the
/// `0x8007ABB0..0x8007AD00` window for the grown handler body.
pub const TITLE_STR_VA: u32 = 0x8007_AEB0;
/// Title bytes ('@' format prefix like the menu strings + "SERU TRADE\0").
pub const TITLE_STR: &[u8] = b"@SERU TRADE\0";

/// Words planted at [`TRADE_DISPATCH_VA`]: `j TRADE_DISPATCH_STUB_VA` then `nop`.
pub fn trade_dispatch_detour_words() -> [u32; 2] {
    [j(TRADE_DISPATCH_STUB_VA), nop()]
}

/// Words planted at [`ENTRY_VA`]: `j ENTRY_STUB_VA` then `nop`.
pub fn trade_entry_detour_words() -> [u32; 2] {
    [j(ENTRY_STUB_VA), nop()]
}

/// Reorder: the two words that repoint the body's row-2 text load to "@Trade"
/// (`lui a0,hi(TRADE_STR_VA); addiu a0,a0,lo(TRADE_STR_VA)`), so row 2 shows Trade.
pub fn row2_str_load_new() -> [u32; 2] {
    [lui(A0, hi(TRADE_STR_VA)), addiu(A0, A0, lo(TRADE_STR_VA))]
}

/// Reorder dispatch stub ([`TRADE_DISPATCH_STUB_VA`]), reached from the confirm
/// detour at [`TRADE_DISPATCH_VA`] (a0 = cursor): cursor 2 → enter the Trade
/// sub-mode ([`SUBSTATE_VA`] = [`TRADE_SUBMODE`]) and exit the dispatcher; cursor 3
/// → the original Quit action ([`QUIT_CODE_VA`]); cursor 0/1 → the Buy/Sell checks
/// ([`BUY_SELL_CHECK_VA`]).
pub fn assemble_trade_dispatch_stub() -> Vec<u32> {
    let mut w: Vec<u32> = Vec::new();
    w.push(addiu(T0, ZERO, 2)); // cursor 2 = Trade (after reorder)
    let b_trade = w.len();
    w.push(0); // beq a0,t0,.trade (patched)
    w.push(nop());
    w.push(addiu(T0, ZERO, 3)); // cursor 3 = Quit
    let b_quit = w.len();
    w.push(0); // beq a0,t0,.quit (patched)
    w.push(nop());
    w.push(j(BUY_SELL_CHECK_VA)); // cursor 0/1 -> Buy/Sell
    w.push(nop());
    let trade = w.len();
    // Slide the picker windows away (reuse the Sell transition) so the trade screen
    // gets the cleared space. Preserve ra across the call — we exit via `j TRADE_EXIT`
    // whose tail `jr ra` must still return to the menu tick.
    w.push(addiu(SP, SP, 0xFFF8)); // sp -= 8
    w.push(sw(RA, SP, 0));
    w.push(lui(A0, hi(SLIDE_AWAY_SCRIPT_VA)));
    w.push(addiu(A0, A0, lo(SLIDE_AWAY_SCRIPT_VA)));
    w.push(jal(WIDGET_VM_FN)); // FUN_801d6628(&DAT_801e4e54) — slide away
    w.push(nop());
    w.push(lw(RA, SP, 0));
    w.push(addiu(SP, SP, 8));
    // Reset the trade-screen state for this entry. All cells live in the same
    // 0x8007AExx page, so one `lui at` covers them. pad-prev = all-ones so the ✕ held
    // from confirming "Trade" in the picker isn't seen as a fresh press on frame 1.
    w.push(lui(AT, hi(TRADE_ACTIVE_VA)));
    w.push(addiu(T1, ZERO, SLIDE_START_OFF as u16));
    w.push(sw(T1, AT, lo(TRADE_SLIDE_DELTA_VA))); // slide = off-screen start
    w.push(sw(ZERO, AT, lo(TRADE_CURSOR_VA))); // line cursor = 0
    w.push(sw(ZERO, AT, lo(TRADE_CONFIRM_VA))); // confirm sub-state = 0 (browsing)
    w.push(sw(ZERO, AT, lo(TRADE_YESNO_VA))); // yes/no = 0 (Yes)
    w.push(addiu(T1, ZERO, 0xFFFF));
    w.push(sw(T1, AT, lo(TRADE_PAD_PREV_VA))); // pad prev = all-held (no frame-1 edges)
    w.push(addiu(T1, ZERO, 1));
    w.push(sw(T1, AT, lo(TRADE_ACTIVE_VA))); // TRADE_ACTIVE = 1
    w.push(j(TRADE_EXIT_VA)); // exit dispatcher; the entry detour catches the flag
    w.push(nop());
    let quit = w.len();
    w.push(j(QUIT_CODE_VA)); // original Quit action (sound + exit)
    w.push(nop());
    w[b_trade] = beq(A0, T0, (trade as i32 - (b_trade as i32 + 1)) as i16);
    w[b_quit] = beq(A0, T0, (quit as i32 - (b_quit as i32 + 1)) as i16);
    debug_assert!(
        TRADE_DISPATCH_STUB_VA + (w.len() as u32) * 4 <= BUCKET_TABLE_VA,
        "dispatch stub overruns the SCUS gap (into the bucket table)"
    );
    w
}

/// `FUN_801dafd4` entry stub ([`ENTRY_STUB_VA`]): if the picker sub-state
/// ([`SUBSTATE_VA`]) is the Trade sub-mode, jump to the trade handler; otherwise
/// replay the displaced prologue and rejoin the function body at [`ENTRY_RETURN_VA`].
pub fn assemble_trade_entry_stub() -> Vec<u32> {
    let mut w: Vec<u32> = vec![
        lui(AT, hi(TRADE_ACTIVE_VA)),
        lw(V0, AT, lo(TRADE_ACTIVE_VA)), // v0 = TRADE_ACTIVE flag
        nop(),                           // R3000 load delay slot
    ];
    let b = w.len();
    w.push(0); // bne v0,zero,.trade (patched)
    w.push(nop());
    w.push(ENTRY_DISPLACED[0]); // addiu sp,sp,-0x20
    w.push(ENTRY_DISPLACED[1]); // sw s1,0x14(sp)
    w.push(j(ENTRY_RETURN_VA)); // back into FUN_801dafd4
    w.push(nop());
    let trade = w.len();
    w.push(j(TRADE_HANDLER_VA));
    w.push(nop());
    w[b] = bne(V0, ZERO, (trade as i32 - (b as i32 + 1)) as i16);
    w
}

/// In-shop trade-screen handler ([`TRADE_HANDLER_VA`]), invoked (via the entry
/// detour) in place of `FUN_801dafd4` while the picker sub-state is the Trade
/// sub-mode. Runs in mode 0x17 with the shop fully intact.
///
/// Renders the **want-a-type / offer-a-partner** offer (see
/// [`legaia_asset::seru_trade`]): it reads the current `(want, give)` pair from the
/// precomputed [`BUCKET_TABLE_VA`] indexed by `(play_time / `[`RESEED_PERIOD_FRAMES`]`)
/// & `[`BUCKET_INDEX_MASK`], draws the give-back seru as a reward header, then scans
/// the four party records — for each member that owns the wanted seru it draws one
/// selectable line `want_name  owner_name  LVL n` (so the same wanted type held by
/// two members lists once per owner, matching `expand_offers`). Finally the native
/// window box, and on ○ it clears the active flag to return to the picker.
///
/// DRAW ORDER MATTERS: text is emitted FIRST, the opaque window box LAST. The native
/// box (`FUN_8002C69C`) and the renderer's own pass both put a later-submitted prim
/// at a DEEPER OT slot, so a box drawn after the text lands *behind* it — exactly the
/// fix used for the in-body row-4 label. Drawing the box first buries every glyph
/// under the blue fill (verified: blank box in a VRAM dump).
///
/// Register budget (all callee-saved, restored on exit): s0 = party slot, s1 = seru
/// index within the owner, s2 = current row y, s3 = wanted id, s4 = current record
/// base, s5 = that record's seru count. The native draw callees preserve s-regs, so
/// loop state survives across them; the give id lives in a scratch reg only until the
/// header draw (no call between reading it and using it).
pub fn assemble_trade_handler() -> Vec<u32> {
    // Absolute VA of word index `i` (the loops `j` to fixed gap VAs, not PC-relative).
    let va = |i: usize| TRADE_HANDLER_VA + (i as u32) * 4;
    // Compute `id * 0xC` into T6 (the spell-name-table stride) from `id` in `src`.
    let id_times_12 = |w: &mut Vec<u32>, src: u32| {
        w.push(sll(T6, src, 2)); // id*4
        w.push(sll(T7, T6, 1)); // id*8
        w.push(addu(T6, T7, T6)); // id*12
    };

    // Prologue: 0x38 frame. sp+0x10 is the native draw 5th-arg (y) build slot; saves
    // ra + s0..s5 above it.
    let mut w: Vec<u32> = vec![
        addiu(SP, SP, 0xFFC8), // sp -= 0x38
        sw(RA, SP, 0x2C),
        sw(S0, SP, 0x14),
        sw(S1, SP, 0x18),
        sw(S2, SP, 0x1C),
        sw(S3, SP, 0x20),
        sw(S4, SP, 0x24),
        sw(S5, SP, 0x28),
        sw(S7, SP, 0x30), // s7 = slide x-offset (held across the frame)
        sw(S6, SP, 0x34), // s6 = give_level (header display + future swap)
        jal(PAD_POLL_FN), // refresh PAD_CUR
        nop(),
    ];

    // --- slide-in: s7 = the box/text x-offset, stepped from SLIDE_START_OFF -> 0 ---
    w.push(lui(AT, hi(TRADE_SLIDE_DELTA_VA)));
    w.push(lw(S7, AT, lo(TRADE_SLIDE_DELTA_VA)));
    w.push(nop()); // load-delay before the branch reads s7
    let slid_b = w.len();
    w.push(0); // bgez s7,.slid (settled at >=0 -> skip stepping) (patched)
    w.push(nop());
    w.push(addiu(S7, S7, SLIDE_STEP)); // step toward 0
    w.push(lui(AT, hi(TRADE_SLIDE_DELTA_VA)));
    w.push(sw(S7, AT, lo(TRADE_SLIDE_DELTA_VA)));
    let slid = w.len();
    w[slid_b] = bgez(S7, (slid as i32 - (slid_b as i32 + 1)) as i16);

    // --- current offer: want -> s3, give -> t5, give_level -> s6 ---
    if SERU_DEMO_FORCE_WANT {
        // DEV: force a fixed (want, give, level) so a save owning SERU_DEMO_BASE_ID lists.
        w.push(addiu(S3, ZERO, SERU_DEMO_BASE_ID)); // want
        w.push(addiu(T5, ZERO, SERU_DEMO_BASE_ID + 1)); // give
        w.push(addiu(S6, ZERO, 7)); // give_level (mid of 4..=9)
    } else {
        // bucket = (play_time / RESEED_PERIOD_FRAMES) & (BUCKET_COUNT-1); entry = bucket*3.
        w.push(lui(AT, hi(PLAY_TIME_VA)));
        w.push(lw(T0, AT, lo(PLAY_TIME_VA)));
        w.push(addiu(T1, ZERO, RESEED_PERIOD_FRAMES)); // (fills the lw load-delay)
        w.push(divu(T0, T1));
        w.push(mflo(T0)); // t0 = bucket
        w.push(andi(T0, T0, BUCKET_INDEX_MASK)); // % BUCKET_COUNT
        w.push(sll(T1, T0, 1)); // bucket*2
        w.push(addu(T0, T1, T0)); // bucket*3 (3-byte entries: want,give,give_level)
        w.push(lui(T1, hi(BUCKET_TABLE_VA)));
        w.push(addiu(T1, T1, lo(BUCKET_TABLE_VA)));
        w.push(addu(T1, T1, T0));
        w.push(lbu(S3, T1, 0)); // want
        w.push(lbu(T5, T1, 1)); // give
        w.push(lbu(S6, T1, 2)); // give_level (fills the t5 load-delay)
        w.push(nop()); // load-delay (s6 used by the header level draw)
    }
    // stash give id so the per-owner loop can skip owners who already own it
    w.push(lui(AT, hi(TRADE_GIVE_ID_VA)));
    w.push(sw(T5, AT, lo(TRADE_GIVE_ID_VA)));

    // --- reward header: the give-back seru name at (x=0x30, y=0x34) ---
    id_times_12(&mut w, T5);
    w.push(lui(T7, hi(SERU_NAME_PTRS))); // a0 = *(SERU_NAME_PTRS + give*0xC)
    w.push(addiu(T7, T7, lo(SERU_NAME_PTRS)));
    w.push(addu(T7, T7, T6));
    w.push(lw(A0, T7, 0));
    w.push(addiu(V0, ZERO, ROW_HEADER_Y)); // y (fills the lw load-delay before a0's use)
    w.push(sw(V0, SP, 0x10));
    w.push(addiu(A1, ZERO, 0));
    w.push(addiu(A2, ZERO, 0));
    w.push(addiu(A3, S7, COL_HEADER_X)); // x + slide offset
    w.push(jal(TEXT_DRAW_FN));
    w.push(nop());
    // reward level (the bucket's fixed give-back level, shown so the player sees the
    // trade's value): FUN_80034b78(s6, 1, COL_LEVEL_X + slide, ROW_HEADER_Y) — aligns
    // under the per-owner level column.
    w.push(addu(A0, ZERO, S6)); // value = give_level
    w.push(addiu(A1, ZERO, 1)); // min_digits
    w.push(addiu(A2, S7, COL_LEVEL_X)); // x + slide offset
    w.push(addiu(A3, ZERO, ROW_HEADER_Y)); // y
    w.push(jal(NUMBER_FN));
    w.push(nop());

    // --- per-owner lines: for slot in 0..4, for j in 0..count: if ids[j]==want ---
    w.push(addiu(S0, ZERO, 0)); // slot = 0
    w.push(addiu(S2, ZERO, ROW_FIRST_Y)); // first row y
    let slotloop = w.len();
    w.push(slti(T0, S0, PARTY_SLOT_COUNT)); // slot < 4 ?
    let done_b = w.len();
    w.push(0); // beq t0,zero,.done (patched)
    w.push(nop());
    // record base s4 = CHAR_RECORD_BASE + slot*0x414
    w.push(addiu(T1, ZERO, CHAR_RECORD_STRIDE));
    w.push(multu(S0, T1));
    w.push(mflo(T2));
    w.push(lui(T3, hi(CHAR_RECORD_BASE)));
    w.push(addiu(T3, T3, lo(CHAR_RECORD_BASE)));
    w.push(addu(S4, T3, T2));
    w.push(lbu(S5, S4, lo(SERU_COUNT_VA - CHAR_RECORD_BASE))); // s5 = count (+0x13C)
    w.push(addiu(S1, ZERO, 0)); // j = 0 (fills the lbu load-delay)
    let seruloop = w.len();
    w.push(slt(T0, S1, S5)); // j < count ?
    let nextslot_b = w.len();
    w.push(0); // beq t0,zero,.nextslot (patched)
    w.push(nop());
    // id = *(s4 + 0x13D + j)
    w.push(addu(T1, S4, S1));
    w.push(lbu(T4, T1, lo(SERU_IDS_VA - CHAR_RECORD_BASE)));
    w.push(sll(T6, S3, 2)); // (fills the lbu load-delay) want*4, reused by render (1)
    let skip_b = w.len();
    w.push(0); // bne t4,s3,.skip (patched)
    w.push(nop());
    // MATCH: skip this owner if they ALREADY own the give-back seru (pointless trade).
    // Scan k=0..count for the give id; if found, jump to .nextslot (no line drawn).
    // Uses t0/t1/t2 only — t6 still holds want*4 from the bne delay slot for the render.
    w.push(lui(AT, hi(TRADE_GIVE_ID_VA)));
    w.push(lw(T0, AT, lo(TRADE_GIVE_ID_VA))); // t0 = give id
    w.push(addiu(T1, ZERO, 0)); // t1 = k = 0 (fills the lw load-delay)
    let gloop = w.len();
    w.push(slt(T2, T1, S5)); // k < count ?
    let gdone_b = w.len();
    w.push(0); // beq t2,zero,.gnotfound (give not owned -> draw)
    w.push(nop());
    w.push(addu(T2, S4, T1));
    w.push(lbu(T2, T2, lo(SERU_IDS_VA - CHAR_RECORD_BASE))); // ids[k]
    w.push(nop()); // load-delay before the beq reads t2
    let gskip_b = w.len();
    w.push(0); // beq t2,t0,.gskip (owns give -> skip owner)
    w.push(nop());
    w.push(addiu(T1, T1, 1)); // k++
    w.push(j(va(gloop)));
    w.push(nop());
    let gskip = w.len();
    w.push(0); // j .nextslot (skip owner) — patched once nextslot is known
    w.push(nop());
    let gnotfound = w.len();
    w[gdone_b] = beq(T2, ZERO, (gnotfound as i32 - (gdone_b as i32 + 1)) as i16);
    w[gskip_b] = beq(T2, T0, (gskip as i32 - (gskip_b as i32 + 1)) as i16);
    // .gnotfound: render the line.
    // MATCH (1): want spell name at x=0x40 (t6 already = want*4 from the delay slot).
    w.push(sll(T7, T6, 1)); // want*8
    w.push(addu(T6, T7, T6)); // want*12
    w.push(lui(T7, hi(SERU_NAME_PTRS)));
    w.push(addiu(T7, T7, lo(SERU_NAME_PTRS)));
    w.push(addu(T7, T7, T6));
    w.push(lw(A0, T7, 0));
    w.push(sw(S2, SP, 0x10)); // y (fills the lw load-delay)
    w.push(addiu(A1, ZERO, 0));
    w.push(addiu(A2, ZERO, 0));
    w.push(addiu(A3, S7, COL_WANT_X)); // x + slide offset
    w.push(jal(TEXT_DRAW_FN));
    w.push(nop());
    // (2) owner name: a0 = s4 + record name offset (+0x2A7).
    w.push(addiu(A0, S4, RECORD_NAME_OFFSET));
    w.push(sw(S2, SP, 0x10));
    w.push(addiu(A1, ZERO, 0));
    w.push(addiu(A2, ZERO, 0));
    w.push(addiu(A3, S7, COL_OWNER_X)); // x + slide offset
    w.push(jal(TEXT_DRAW_FN));
    w.push(nop());
    // (3) level number: FUN_80034b78(*(s4+0x161+j), 1, COL_LEVEL_X + slide, y=s2).
    w.push(addu(T1, S4, S1));
    w.push(lbu(A0, T1, SERU_LEVELS_OFFSET)); // a0 = level value
    w.push(addiu(A1, ZERO, 1)); // min_digits (fills the lbu load-delay)
    w.push(addiu(A2, S7, COL_LEVEL_X)); // x + slide offset
    w.push(addu(A3, ZERO, S2)); // y
    w.push(jal(NUMBER_FN));
    w.push(nop());
    w.push(addiu(S2, S2, ROW_STEP_Y)); // advance the row
    let skip = w.len();
    w.push(addiu(S1, S1, 1)); // j++
    w.push(j(va(seruloop)));
    w.push(nop());
    let nextslot = w.len();
    w.push(addiu(S0, S0, 1)); // slot++
    w.push(j(va(slotloop)));
    w.push(nop());
    w[gskip] = j(va(nextslot)); // give-owned -> skip this owner
    let done = w.len();
    w[done_b] = beq(T0, ZERO, (done as i32 - (done_b as i32 + 1)) as i16);
    w[nextslot_b] = beq(T0, ZERO, (nextslot as i32 - (nextslot_b as i32 + 1)) as i16);
    w[skip_b] = bne(T4, S3, (skip as i32 - (skip_b as i32 + 1)) as i16);

    // --- input + cursor: browse the owner lines (NOT the header), or pick Yes/No ---
    // line count N = (s2 - ROW_FIRST_Y) >> 4 (kept in t0); line cursor in t1.
    w.push(addiu(T0, S2, (0u16).wrapping_sub(ROW_FIRST_Y)));
    w.push(srl(T0, T0, 4)); // t0 = N
    w.push(lui(AT, hi(TRADE_CURSOR_VA)));
    w.push(lw(T1, AT, lo(TRADE_CURSOR_VA))); // t1 = line cursor
    w.push(lui(AT, hi(PAD_CUR_VA)));
    w.push(lw(T2, AT, lo(PAD_CUR_VA))); // t2 = pad cur
    w.push(lui(AT, hi(TRADE_PAD_PREV_VA)));
    w.push(lw(T3, AT, lo(TRADE_PAD_PREV_VA))); // t3 = pad prev (last frame)
    w.push(lui(AT, hi(TRADE_PAD_PREV_VA)));
    w.push(sw(T2, AT, lo(TRADE_PAD_PREV_VA))); // prev = cur (fills the t3 load-delay)

    // Run `body` only on a fresh press of `mask` (held now in t2, not last frame in
    // t3). Uses t5/t6 as scratch (free for `body` after the guard branches).
    let edge = |w: &mut Vec<u32>, mask: u16, body: &dyn Fn(&mut Vec<u32>)| {
        w.push(andi(T5, T2, mask));
        let b1 = w.len();
        w.push(0);
        w.push(nop());
        w.push(andi(T6, T3, mask));
        let b2 = w.len();
        w.push(0);
        w.push(nop());
        body(w);
        let done = w.len();
        w[b1] = beq(T5, ZERO, (done as i32 - (b1 as i32 + 1)) as i16);
        w[b2] = bne(T6, ZERO, (done as i32 - (b2 as i32 + 1)) as i16);
    };

    // confirm sub-state -> t7; branch to .browse when 0.
    w.push(lui(AT, hi(TRADE_CONFIRM_VA)));
    w.push(lw(T7, AT, lo(TRADE_CONFIRM_VA)));
    w.push(nop());
    let browse_b = w.len();
    w.push(0); // beq t7,zero,.browse (patched)
    w.push(nop());

    // === CONFIRM input: Left=Yes(0) / Right=No(1); ✕ or ○ leaves the prompt ===
    edge(&mut w, PAD_LEFT_MASK, &|w| {
        w.push(lui(AT, hi(TRADE_YESNO_VA)));
        w.push(sw(ZERO, AT, lo(TRADE_YESNO_VA))); // yesno = 0 (Yes)
    });
    edge(&mut w, PAD_RIGHT_MASK, &|w| {
        w.push(addiu(T6, ZERO, 1));
        w.push(lui(AT, hi(TRADE_YESNO_VA)));
        w.push(sw(T6, AT, lo(TRADE_YESNO_VA))); // yesno = 1 (No)
    });
    // ✕ resolves (Yes = perform the swap — TODO; for now both just leave), ○ cancels.
    // Either way clear the confirm sub-state -> back to browsing.
    edge(&mut w, PAD_CONFIRM_MASK, &|w| {
        w.push(lui(AT, hi(TRADE_CONFIRM_VA)));
        w.push(sw(ZERO, AT, lo(TRADE_CONFIRM_VA)));
    });
    edge(&mut w, HANDLER_CANCEL_MASK, &|w| {
        w.push(lui(AT, hi(TRADE_CONFIRM_VA)));
        w.push(sw(ZERO, AT, lo(TRADE_CONFIRM_VA)));
    });
    let conf_done_b = w.len();
    w.push(0); // j .after_input (patched, absolute)
    w.push(nop());

    // === BROWSE input (.browse): Up/Down move the line cursor; ✕ enters confirm;
    // ○ exits the trade screen ===
    let browse = w.len();
    w[browse_b] = beq(T7, ZERO, (browse as i32 - (browse_b as i32 + 1)) as i16);
    edge(&mut w, PAD_UP_MASK, &|w| {
        w.push(addiu(T1, T1, 0xFFFF)); // cursor-- (-1)
    });
    edge(&mut w, PAD_DOWN_MASK, &|w| {
        w.push(addiu(T1, T1, 1)); // cursor++
    });
    edge(&mut w, PAD_CONFIRM_MASK, &|w| {
        w.push(addiu(T6, ZERO, 1));
        w.push(lui(AT, hi(TRADE_CONFIRM_VA)));
        w.push(sw(T6, AT, lo(TRADE_CONFIRM_VA))); // enter confirm
    });
    // ○ edge -> exit (jump to .do_exit, far ahead).
    w.push(andi(T5, T2, HANDLER_CANCEL_MASK));
    let ox_b = w.len();
    w.push(0); // beq t5,zero,.noox
    w.push(nop());
    w.push(andi(T6, T3, HANDLER_CANCEL_MASK));
    let ox_b2 = w.len();
    w.push(0); // bne t6,zero,.noox
    w.push(nop());
    let exit_jb = w.len();
    w.push(0); // j .do_exit (patched, absolute)
    w.push(nop());
    let noox = w.len();
    w[ox_b] = beq(T5, ZERO, (noox as i32 - (ox_b as i32 + 1)) as i16);
    w[ox_b2] = bne(T6, ZERO, (noox as i32 - (ox_b2 as i32 + 1)) as i16);

    // .after_input: clamp the line cursor to [0, N) and store it.
    let after_input = w.len();
    w[conf_done_b] = j(va(after_input));
    let low_b = w.len();
    w.push(0); // bgez t1,.nolow
    w.push(nop());
    w.push(addu(T1, ZERO, ZERO)); // cursor = 0
    let nolow = w.len();
    w[low_b] = bgez(T1, (nolow as i32 - (low_b as i32 + 1)) as i16);
    w.push(slt(T5, T1, T0)); // cursor < N ?
    let high_b = w.len();
    w.push(0); // bne t5,zero,.nohigh
    w.push(nop());
    w.push(addiu(T1, T0, 0xFFFF)); // cursor = N-1
    let nohigh = w.len();
    w[high_b] = bne(T5, ZERO, (nohigh as i32 - (high_b as i32 + 1)) as i16);
    w.push(lui(AT, hi(TRADE_CURSOR_VA)));
    w.push(sw(T1, AT, lo(TRADE_CURSOR_VA)));
    // line cursor sprite at the selected line (skip if no lines).
    let hl_b = w.len();
    w.push(0); // blez t0,.nohl (N <= 0)
    w.push(nop());
    w.push(sll(T5, T1, 4)); // cursor * ROW_STEP_Y
    w.push(addiu(T5, T5, ROW_FIRST_Y)); // + first row y
    w.push(addiu(A0, ZERO, 0)); // slot 0 (standard menu cursor)
    w.push(addiu(A1, ZERO, 1)); // mode 1 (animated)
    w.push(addiu(A2, S7, CURSOR_X)); // x + slide offset
    w.push(addu(A3, ZERO, T5)); // y
    w.push(jal(CURSOR_DRAW_FN));
    w.push(nop());
    let nohl = w.len();
    w[hl_b] = blez(T0, (nohl as i32 - (hl_b as i32 + 1)) as i16);

    // confirm prompt (only in the confirm sub-state): "@Trade?" + Yes / No + cursor.
    w.push(lui(AT, hi(TRADE_CONFIRM_VA)));
    w.push(lw(T7, AT, lo(TRADE_CONFIRM_VA))); // reload (it may have just changed)
    w.push(nop());
    let prompt_b = w.len();
    w.push(0); // beq t7,zero,.noprompt (patched)
    w.push(nop());
    // "@Trade?" at (COL_HEADER_X + slide, PROMPT_Y)
    w.push(lui(A0, hi(CONFIRM_PROMPT_STR_VA)));
    w.push(addiu(A0, A0, lo(CONFIRM_PROMPT_STR_VA)));
    w.push(addiu(V0, ZERO, PROMPT_Y));
    w.push(sw(V0, SP, 0x10));
    w.push(addiu(A1, ZERO, 0));
    w.push(addiu(A2, ZERO, 0));
    w.push(addiu(A3, S7, COL_HEADER_X));
    w.push(jal(TEXT_DRAW_FN));
    w.push(nop());
    // "@Yes" at (YES_X + slide, CHOICE_Y)
    w.push(lui(A0, hi(CONFIRM_YES_STR_VA)));
    w.push(addiu(A0, A0, lo(CONFIRM_YES_STR_VA)));
    w.push(addiu(V0, ZERO, CHOICE_Y));
    w.push(sw(V0, SP, 0x10));
    w.push(addiu(A1, ZERO, 0));
    w.push(addiu(A2, ZERO, 0));
    w.push(addiu(A3, S7, YES_X));
    w.push(jal(TEXT_DRAW_FN));
    w.push(nop());
    // "@No" at (NO_X + slide, CHOICE_Y)
    w.push(lui(A0, hi(CONFIRM_NO_STR_VA)));
    w.push(addiu(A0, A0, lo(CONFIRM_NO_STR_VA)));
    w.push(addiu(V0, ZERO, CHOICE_Y));
    w.push(sw(V0, SP, 0x10));
    w.push(addiu(A1, ZERO, 0));
    w.push(addiu(A2, ZERO, 0));
    w.push(addiu(A3, S7, NO_X));
    w.push(jal(TEXT_DRAW_FN));
    w.push(nop());
    // yes/no cursor: x = (yesno ? NO_X : YES_X) - CHOICE_CURSOR_DX + slide.
    w.push(lui(AT, hi(TRADE_YESNO_VA)));
    w.push(lw(T4, AT, lo(TRADE_YESNO_VA)));
    w.push(addiu(A2, S7, YES_X - CHOICE_CURSOR_DX)); // default Yes (fills load-delay)
    let ynx_b = w.len();
    w.push(0); // beq t4,zero,.yesx
    w.push(nop());
    w.push(addiu(A2, S7, NO_X - CHOICE_CURSOR_DX)); // No
    let yesx = w.len();
    w[ynx_b] = beq(T4, ZERO, (yesx as i32 - (ynx_b as i32 + 1)) as i16);
    w.push(addiu(A0, ZERO, 0));
    w.push(addiu(A1, ZERO, 1));
    w.push(addiu(A3, ZERO, CHOICE_Y));
    w.push(jal(CURSOR_DRAW_FN));
    w.push(nop());
    let noprompt = w.len();
    w[prompt_b] = beq(T7, ZERO, (noprompt as i32 - (prompt_b as i32 + 1)) as i16);

    // native window box LAST (behind the text). Force the standard skin first
    // (gp[+0x14c]) so it doesn't inherit the brown name-plate skin after the slide.
    w.push(addiu(T0, ZERO, WINDOW_SKIN_STD));
    w.push(sw(T0, GP, WINDOW_SKIN_OFF));
    w.push(addiu(A0, S7, BOX_X)); // x + slide
    w.push(addiu(A1, ZERO, BOX_Y));
    w.push(addiu(A2, ZERO, BOX_W));
    w.push(addiu(A3, ZERO, BOX_H_PX));
    w.push(jal(BOX_FN));
    w.push(nop());
    let fin_jb = w.len();
    w.push(0); // j .finalize (patched, absolute)
    w.push(nop());

    // .do_exit (○ in browse): clear TRADE_ACTIVE + slide the picker windows back in.
    let do_exit = w.len();
    w[exit_jb] = j(va(do_exit));
    w.push(lui(AT, hi(TRADE_ACTIVE_VA)));
    w.push(sw(ZERO, AT, lo(TRADE_ACTIVE_VA)));
    w.push(lui(A0, hi(SLIDE_OPEN_SCRIPT_VA)));
    w.push(addiu(A0, A0, lo(SLIDE_OPEN_SCRIPT_VA)));
    w.push(jal(WIDGET_VM_FN)); // FUN_801d6628(&DAT_801e4e38) — slide back in
    w.push(nop());

    // .finalize: per-frame finalize tail + epilogue.
    let finalize = w.len();
    w[fin_jb] = j(va(finalize));
    w.push(jal(FINALIZE_FN));
    w.push(nop());
    w.push(lw(RA, SP, 0x2C));
    w.push(lw(S0, SP, 0x14));
    w.push(lw(S1, SP, 0x18));
    w.push(lw(S2, SP, 0x1C));
    w.push(lw(S3, SP, 0x20));
    w.push(lw(S4, SP, 0x24));
    w.push(lw(S5, SP, 0x28));
    w.push(lw(S7, SP, 0x30));
    w.push(lw(S6, SP, 0x34));
    w.push(addiu(SP, SP, 0x38));
    w.push(jr(RA)); // return to the menu tick (FUN_801dc6b4)
    w.push(nop());
    debug_assert!(
        TRADE_HANDLER_VA + (w.len() as u32) * 4 <= TRADE_HANDLER_END,
        "trade handler overruns the 0899 run-C dead region"
    );
    debug_assert!(
        TRADE_HANDLER_VA + (w.len() as u32) * 4 <= CONFIRM_PROMPT_STR_VA,
        "trade handler collides with the confirm strings in 0899"
    );
    w
}

/// Cursor-state word `DAT_801e46bc` (low 12 bits = index; 0x2000 = ○; 0x4000 = ✕).
pub const CURSOR_VA: u32 = 0x801E_46BC;
/// Cursor-highlight sprite `FUN_8002b994(slot, mode, x, y)`.
pub const HIGHLIGHT_FN: u32 = 0x8002_B994;
/// Gap VA of the row-4 draw stub (reused [`STUB_VA`]; the native-row build uses no
/// trigger/redirect, so the gap is free).
pub const ROW4_STUB_VA: u32 = STUB_VA;
/// Gap VA of the "@Trade" label (past the stub; <= 0xA0 bytes reserved for code).
pub const TRADE_STR_VA: u32 = STUB_VA + 0xA0;
/// The label bytes: '@' format prefix (as on "@Buy"/"@Sell"/"@Quit") + "Trade\0".
pub const TRADE_STR: &[u8] = b"@Trade\0";

/// Words planted at [`ROW4_DETOUR_VA`]: `j ROW4_STUB_VA` then `nop`.
pub fn row4_detour_words() -> [u32; 2] {
    [j(ROW4_STUB_VA), nop()]
}

/// Words planted at [`ROW4_DETOUR_VA`]: `j ROW4_STUB_VA` then `nop`.
///
/// Draws + highlights the fourth "Trade" picker row from INSIDE the renderer body
/// (right after the Quit text draw), so the glyphs link at the same OT depth as
/// Buy/Sell/Quit — in front of the box background. At the detour site `s0` = the
/// Quit row's y, `s2` = the text x, `s1` = the picker context (`param_1`); the
/// native callees preserve them, and the function's `ra` was already stack-saved
/// at the prologue, so the stub may `jal` freely. The Trade row sits at `s0+0xe`,
/// highlighted (mirroring the Quit-row logic) when the cursor is on index 3. Then
/// it replays the two displaced words and rejoins the Quit-highlight at
/// [`ROW4_RETURN_VA`].
pub fn assemble_row4_draw_stub() -> Vec<u32> {
    assemble_row4_draw_stub_str(TRADE_STR_VA)
}

/// As [`assemble_row4_draw_stub`], with the 4th-row label address selectable. The
/// standalone native-row build draws "@Trade" ([`TRADE_STR_VA`]); the full in-shop
/// build reorders to Buy/Sell/Trade/Quit by swapping the body's row-2 string to
/// "@Trade" and drawing "@Quit" ([`QUIT_STR_VA`]) here at row 3.
pub fn assemble_row4_draw_stub_str(str_va: u32) -> Vec<u32> {
    // draw the row-4 label at (a3 = x = s2, y = s0 + 0xe), matching the body's call.
    let mut w: Vec<u32> = vec![
        addiu(T0, S0, 0x0e),       // t0 = 4th-row y = Quit y + 0xe
        sw(T0, SP, 0x10),          // 5th arg (y) on the stack
        addiu(A3, S2, 0),          // a3 = x (text x, = body s2)
        addiu(A1, ZERO, 0),        // a1 = 0
        addiu(A2, ZERO, 0),        // a2 = 0
        lui(A0, hi(str_va)),       //  \ a0 = &label
        addiu(A0, A0, lo(str_va)), //  /
        jal(TEXT_DRAW_FN),
        nop(),
        // highlight row 3 (mirror of the Quit-row highlight with index 3, y=s0+0xe)
        lui(V0, hi(CURSOR_VA)),
        lw(A1, V0, lo(CURSOR_VA)), // a1 = DAT_801e46bc
        nop(),                     // R3000 load-delay slot (a1 not ready until now)
        andi(V0, A1, 0x4000),      // ✕ cancel?
    ];
    let b_cancel = w.len();
    w.push(0); // bne v0,zero,.done (patched)
    w.push(nop());
    w.push(andi(V0, A1, 0x2000)); // ○ confirm?
    let b_nav = w.len();
    w.push(0); // beq v0,zero,.nav (patched)
    w.push(addiu(A0, ZERO, 0)); // (delay) a0 = 0 (slot)
    // confirm branch: a1 = ((DAT & 0x1000)==0) << 2
    w.push(andi(A1, A1, 0x1000));
    w.push(sltiu(A1, A1, 1));
    let j_hl = w.len();
    w.push(0); // j .hl (patched)
    w.push(sll(A1, A1, 2)); // (delay)
    // .nav: if cursor != 3 -> .done; else a1 = (DAT>>0xc ^ 1) & 1
    let nav = w.len();
    w.push(andi(V1, A1, 0xfff));
    w.push(addiu(V0, ZERO, 3));
    let b_skip = w.len();
    w.push(0); // bne v1,v0,.done (patched)
    w.push(srl(A1, A1, 0xc)); // (delay)
    w.push(xori(A1, A1, 1));
    w.push(andi(A1, A1, 1));
    // .hl: FUN_8002b994(0, a1=mode, a2=x, a3=y4)
    let hl = w.len();
    w.push(lh(A2, S1, 0x0a)); // a2 = cursor x (raw, like the Quit highlight)
    w.push(addiu(A3, S0, 0x0e)); // a3 = y4 = s0 + 0xe
    w.push(jal(HIGHLIGHT_FN));
    w.push(nop());
    // .done: replay the displaced words, rejoin the Quit-highlight at +8.
    let done = w.len();
    w.push(ROW4_DISPLACED[0]); // lui v0,0x801e
    w.push(ROW4_DISPLACED[1]); // lw a1,0x46bc(v0)
    w.push(j(ROW4_RETURN_VA));
    w.push(nop());
    // resolve branches / jump
    w[b_cancel] = bne(V0, ZERO, (done as i32 - (b_cancel as i32 + 1)) as i16);
    w[b_nav] = beq(V0, ZERO, (nav as i32 - (b_nav as i32 + 1)) as i16);
    w[b_skip] = bne(V1, V0, (done as i32 - (b_skip as i32 + 1)) as i16);
    w[j_hl] = j(ROW4_STUB_VA + (hl as u32) * 4);
    debug_assert!(
        (w.len() as u32) * 4 <= (TRADE_STR_VA - ROW4_STUB_VA),
        "row-4 stub overruns the reserved code window before TRADE_STR_VA"
    );
    w
}

/// Stub for the shop-picker quiet-frame trigger ([`PICKER_TRIGGER_VA`]).
///
/// Runs at the head of `FUN_801d4868` each frame the Buy/Sell/Quit choice is on
/// screen. On SQUARE it arms the mode-24 warp (sub-id + mode 0x18 + the
/// minigame-active sysflag), mirroring the op-0x3E door-warp minus the casino
/// housekeeping zeros; `ra`/`a0` (live at entry, not yet saved by the prologue)
/// are preserved across the sysflag `jal`. Then it replays the two displaced
/// prologue words and rejoins the picker at [`PICKER_RETURN_VA`]. Fixed at 24
/// words (0x60 bytes) to fit the gap slot before the redirect.
pub fn assemble_picker_trade_detour_stub(sub_id: u16) -> Vec<u32> {
    let mut w: Vec<u32> = Vec::new();
    // if ((pad & SQUARE) == 0) -> .replay
    w.push(lui(AT, hi(PAD_CUR_VA)));
    w.push(lw(T0, AT, lo(PAD_CUR_VA)));
    w.push(andi(T1, T0, PICKER_TRIGGER_MASK));
    let skipb = w.len();
    w.push(0); // beq T1,ZERO,.replay (patched)
    w.push(nop());
    // SQUARE held: arm the warp. Preserve ra + a0 across the sysflag jal (sp here
    // is the caller's; the prologue's own -0x28 happens later in .replay).
    w.push(addiu(SP, SP, 0xFFF8)); // sp -= 8
    w.push(sw(RA, SP, 0));
    w.push(sw(A0, SP, 4));
    w.push(addiu(V0, ZERO, sub_id)); //  \ _DAT_8007BA34 = sub_id
    w.push(lui(AT, hi(SUBID_VA))); //  |
    w.push(sh(V0, AT, lo(SUBID_VA))); //  /
    w.push(addiu(V0, ZERO, MODE_OTHER_INIT)); //  \ _DAT_8007B83C = 0x18 (mode 24)
    w.push(lui(AT, hi(MODE_INDEX_VA))); //  |
    w.push(sh(V0, AT, lo(MODE_INDEX_VA))); //  /
    w.push(addiu(A0, ZERO, SYSFLAG_WARP_ARG)); //  a0 = 0xE
    w.push(jal(SYSFLAG_SET_FN)); //  func_0x8003CE08(0xE)
    w.push(nop()); //  (delay)
    w.push(lw(RA, SP, 0));
    w.push(lw(A0, SP, 4));
    w.push(addiu(SP, SP, 8)); // sp += 8
    // .replay: the picker prologue head, then back into the body at +8.
    let replay = w.len();
    w.push(PICKER_DISPLACED[0]); // addiu sp,sp,-0x28
    w.push(PICKER_DISPLACED[1]); // sw s1,0x1c(sp)
    w.push(j(PICKER_RETURN_VA));
    w.push(nop()); // (delay)
    w[skipb] = beq(T1, ZERO, (replay as i32 - (skipb as i32 + 1)) as i16);
    debug_assert_eq!(w.len(), 24, "picker stub must be 24 words (0x60 bytes)");
    w
}

const fn j(target: u32) -> u32 {
    (0x02 << 26) | ((target >> 2) & 0x03ff_ffff)
}
const fn jal(target: u32) -> u32 {
    (0x03 << 26) | ((target >> 2) & 0x03ff_ffff)
}
const fn nop() -> u32 {
    0
}
const fn lui(rt: u32, imm: u16) -> u32 {
    (0x0f << 26) | (rt << 16) | imm as u32
}
const fn ori(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0d << 26) | (rs << 21) | (rt << 16) | imm as u32
}
const fn addiu(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x09 << 26) | (rs << 21) | (rt << 16) | imm as u32
}
const fn sw(rt: u32, rs: u32, off: u16) -> u32 {
    (0x2b << 26) | (rs << 21) | (rt << 16) | off as u32
}
const fn sh(rt: u32, rs: u32, off: u16) -> u32 {
    (0x29 << 26) | (rs << 21) | (rt << 16) | off as u32
}
const fn jr(rs: u32) -> u32 {
    (rs << 21) | 0x08
}
const fn jalr(rs: u32) -> u32 {
    (rs << 21) | (RA << 11) | 0x09
}
const fn lbu(rt: u32, rs: u32, off: u16) -> u32 {
    (0x24 << 26) | (rs << 21) | (rt << 16) | off as u32
}
const fn andi(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0c << 26) | (rs << 21) | (rt << 16) | imm as u32
}
const fn sll(rd: u32, rt: u32, sa: u32) -> u32 {
    (rt << 16) | (rd << 11) | (sa << 6)
}
const fn addu(rd: u32, rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | (rd << 11) | 0x21
}
const fn slt(rd: u32, rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | (rd << 11) | 0x2a
}
const fn lw(rt: u32, rs: u32, off: u16) -> u32 {
    (0x23 << 26) | (rs << 21) | (rt << 16) | off as u32
}
const fn lhu(rt: u32, rs: u32, off: u16) -> u32 {
    (0x25 << 26) | (rs << 21) | (rt << 16) | off as u32
}
const fn bne(rs: u32, rt: u32, off: i16) -> u32 {
    (0x05 << 26) | (rs << 21) | (rt << 16) | (off as u16 as u32)
}
const fn beq(rs: u32, rt: u32, off: i16) -> u32 {
    (0x04 << 26) | (rs << 21) | (rt << 16) | (off as u16 as u32)
}
const fn slti(rt: u32, rs: u32, imm: i16) -> u32 {
    (0x0a << 26) | (rs << 21) | (rt << 16) | (imm as u16 as u32)
}
const fn lh(rt: u32, rs: u32, off: u16) -> u32 {
    (0x21 << 26) | (rs << 21) | (rt << 16) | off as u32
}
const fn sltiu(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0b << 26) | (rs << 21) | (rt << 16) | imm as u32
}
const fn srl(rd: u32, rt: u32, sa: u32) -> u32 {
    (rt << 16) | (rd << 11) | (sa << 6) | 0x02
}
const fn multu(rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | 0x19
}
const fn divu(rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | 0x1b
}
const fn mflo(rd: u32) -> u32 {
    (rd << 11) | 0x12
}
const fn bgez(rs: u32, off: i16) -> u32 {
    (0x01 << 26) | (rs << 21) | (0x01 << 16) | (off as u16 as u32)
}
const fn blez(rs: u32, off: i16) -> u32 {
    (0x06 << 26) | (rs << 21) | (off as u16 as u32)
}
const fn xori(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0e << 26) | (rs << 21) | (rt << 16) | imm as u32
}

/// High 16 bits to `lui` so a following signed-`lo` access reaches `va`.
const fn hi(va: u32) -> u16 {
    (va.wrapping_add(0x8000) >> 16) as u16
}
/// Low 16 bits of `va` (the signed offset half).
const fn lo(va: u32) -> u16 {
    (va & 0xffff) as u16
}
/// Plain high half of a 32-bit immediate (no sign correction — for `lui`+`ori`).
const fn imm_hi(v: u32) -> u16 {
    (v >> 16) as u16
}
const fn imm_lo(v: u32) -> u16 {
    (v & 0xffff) as u16
}

/// Assemble the slice overlay: write [`SENTINEL`] to [`SENTINEL_ADDR`], return.
/// Position-independent (absolute store + `jr ra`), so it executes at any load
/// address. 6 instructions / 24 bytes.
pub fn assemble_sentinel_overlay() -> Vec<u32> {
    vec![
        lui(V0, imm_hi(SENTINEL)),     // v0 = SENTINEL hi
        ori(V0, V0, imm_lo(SENTINEL)), // v0 |= SENTINEL lo
        lui(V1, hi(SENTINEL_ADDR)),    // v1 = &SENTINEL_ADDR hi
        sw(V0, V1, lo(SENTINEL_ADDR)), // *SENTINEL_ADDR = v0
        jr(RA),                        // return to the stub
        nop(),                         // (branch delay)
    ]
}

/// Assemble the loader stub for an overlay at disc `lba` spanning `sectors`
/// sectors, loaded to [`DEST`] and called. `displaced` are the two hook
/// instructions to replay; `return_va` is where to jump back. Lives at
/// [`STUB_VA`]. 18 instructions / 72 bytes (fits the gap free window).
///
/// After the CD read it calls the BIOS `FlushCache` (A-func [`FLUSH_CACHE_FN`]
/// via the [`BIOS_DISPATCH_A`] dispatcher) **before** executing the loaded code:
/// the PSX I-cache is not DMA-coherent, so freshly streamed code can otherwise
/// run stale on hardware.
pub fn assemble_loader_stub(
    lba: u32,
    sectors: u16,
    displaced: [u32; 2],
    return_va: u32,
) -> Vec<u32> {
    vec![
        addiu(A0, ZERO, sectors),  // 0:  a0 = sector_count
        lui(A1, imm_hi(lba)),      // 1:  \ a1 = lba
        ori(A1, A1, imm_lo(lba)),  // 2:  /
        lui(A2, imm_hi(DEST)),     // 3:  \ a2 = dest
        ori(A2, A2, imm_lo(DEST)), // 4:  /
        jal(LOADER_FN),            // 5:  FUN_8005E4D4(sectors, lba, dest)
        nop(),                     // 6:  (delay)
        // FlushCache() so the just-loaded code isn't executed from a stale
        // I-cache line (PSX I-cache is not DMA-coherent).
        addiu(T2, ZERO, BIOS_DISPATCH_A), // 7:  t2 = 0xA0 (A-table dispatcher)
        jalr(T2),                         // 8:  call it (returns to us)
        addiu(T1, ZERO, FLUSH_CACHE_FN),  // 9:  (delay) t1 = 0x44 = FlushCache
        lui(T0, imm_hi(DEST)),            // 10: \ t0 = dest
        ori(T0, T0, imm_lo(DEST)),        // 11: /
        jalr(T0),                         // 12: call the loaded overlay
        nop(),                            // 13: (delay)
        displaced[0],                     // 14: replay hook instr 0
        displaced[1],                     // 15: replay hook instr 1
        j(return_va),                     // 16: back to the hook join
        nop(),                            // 17: (delay)
    ]
}

/// Assemble the **shop-gated** loader stub for the op-0x49 arm-edge detour. It
/// gates on the sub-op (`*s6 == 0` = a merchant; skips name-entry / inn / save),
/// loads + FlushCaches + runs the overlay, then replays the two displaced
/// instructions ([`SHOP_DISPLACED`]) and jumps back to [`SHOP_RETURN_VA`]. `s6`
/// (the field VM's live operand pointer) and `s0`/`s1` are preserved by the
/// callees, so the dispatcher continues correctly. Lives at [`STUB_VA`].
pub fn assemble_shop_loader_stub(lba: u32, sectors: u16) -> Vec<u32> {
    assemble_shop_loader_stub_gated(lba, sectors, true)
}

/// As [`assemble_shop_loader_stub`], but `gated` selects whether the sub-op gate
/// is live. With `gated = false` the gate branch is neutered (`bne zero,zero` is
/// never taken) so the overlay loads on **every** op-`0x49` arm (shop / inn /
/// save / name-entry alike) — a diagnostic build that proves whether the detour
/// fires at all, independent of the sub-op value. The word layout is identical
/// either way (only the branch's `rs` register changes), so the sidecar + the
/// disc oracle stay consistent.
pub fn assemble_shop_loader_stub_gated(lba: u32, sectors: u16, gated: bool) -> Vec<u32> {
    // Indices: the load block is 3..16, the replay block starts at 17.
    const REPLAY: usize = 17;
    let skip_off = (REPLAY as i32 - (1 + 1)) as i16; // bne at idx 1 -> REPLAY
    let gate_rs = if gated { T3 } else { ZERO }; // bne zero,zero is never taken
    vec![
        lbu(T3, S6, 0),                   // 0:  t3 = *operand (op-0x49 sub-op)
        bne(gate_rs, ZERO, skip_off),     // 1:  if sub-op != 0 (not a shop) -> replay
        nop(),                            // 2:  (delay)
        addiu(A0, ZERO, sectors),         // 3:  a0 = sector_count
        lui(A1, imm_hi(lba)),             // 4:  \ a1 = lba
        ori(A1, A1, imm_lo(lba)),         // 5:  /
        lui(A2, imm_hi(DEST)),            // 6:  \ a2 = dest
        ori(A2, A2, imm_lo(DEST)),        // 7:  /
        jal(LOADER_FN),                   // 8:  FUN_8005E4D4(sectors, lba, dest)
        nop(),                            // 9:  (delay)
        addiu(T2, ZERO, BIOS_DISPATCH_A), // 10: t2 = 0xA0
        jalr(T2),                         // 11: FlushCache()
        addiu(T1, ZERO, FLUSH_CACHE_FN),  // 12: (delay) t1 = 0x44
        lui(T0, imm_hi(DEST)),            // 13: \ t0 = dest
        ori(T0, T0, imm_lo(DEST)),        // 14: /
        jalr(T0),                         // 15: run the loaded overlay
        nop(),                            // 16: (delay)
        // REPLAY (idx 17): the displaced arm-edge instructions, then return.
        SHOP_DISPLACED[0], // 17: sw s6,-0x4bb0(s0)
        SHOP_DISPLACED[1], // 18: lbu v0,0(s6)
        j(SHOP_RETURN_VA), // 19: back to the dispatcher
        nop(),             // 20: (delay)
    ]
}

/// Assemble the **option-1 warp trigger** stub (lives at [`STUB_VA`], reached by
/// the op-0x49 detour's `j STUB_VA`). Instead of a raw mid-tick CD read (which
/// reentrantly froze), it MIRRORS the field-VM op-0x3E minigame door-warp arm
/// (`0x801E078C`): zero the two housekeeping words, set the minigame `sub_id`,
/// request mode 24 (`MODE_OTHER_INIT`), and call the SysFlag setter — then replay
/// the op-0x49 displaced pair and return to the dispatcher. The current frame
/// finishes normally; next frame the mode SM enters mode 24 (`FUN_80025980`),
/// which loads the slot-A overlay for `sub_id` from the SAFE between-frames CD
/// context and warps back to field on exit. `sub_id` selects which overlay
/// `FUN_80025980` loads + dispatches — wiring our own overlay there is the
/// payload-hosting step (the FUN_80025980 switch fork), separate from this
/// trigger. Does NOT clear the op-0x3E `player[+0x10]&~0x80000` bit (that needs
/// the live VM-ctx player pointer, absent at this hook; it is session cleanup,
/// not part of the mode handoff).
pub fn assemble_warp_trigger_stub(sub_id: u16, fire_once: bool) -> Vec<u32> {
    assemble_warp_trigger_stub_opts(sub_id, fire_once, false)
}

/// As [`assemble_warp_trigger_stub`], but `gate_shop` adds a sub-op gate so the
/// warp only fires for a **merchant** (op-0x49 sub-op `0` — the shop record),
/// skipping name-entry / inn / save. `s6` is the live operand pointer at the
/// detour, so `*s6` is the sub-op. (Shops don't auto-retrigger, so the real
/// feature uses `gate_shop=true, fire_once=false`.) Both guards branch to the
/// shared `.replay` tail (offsets patched once it's placed).
pub fn assemble_warp_trigger_stub_opts(sub_id: u16, fire_once: bool, gate_shop: bool) -> Vec<u32> {
    let mut w: Vec<u32> = Vec::new();
    // bne placeholders (index, rs-register) that must branch to .replay.
    let mut to_replay: Vec<(usize, u32)> = Vec::new();
    if gate_shop {
        // if (*s6 != 0) -> .replay  (only sub-op 0 = a shop runs the warp)
        w.push(lbu(T3, S6, 0));
        to_replay.push((w.len(), T3));
        w.push(bne(T3, ZERO, 0));
        w.push(nop());
    }
    if fire_once {
        // Skip the warp (just replay) if we have already fired once.
        w.push(lui(AT, hi(WARP_FIRED_VA)));
        w.push(lhu(V0, AT, lo(WARP_FIRED_VA)));
        to_replay.push((w.len(), V0));
        w.push(bne(V0, ZERO, 0));
        w.push(nop());
        w.push(addiu(V0, ZERO, 1)); // flag = 1
        w.push(lui(AT, hi(WARP_FIRED_VA)));
        w.push(sh(V0, AT, lo(WARP_FIRED_VA)));
    }
    // The warp arm: mirror the op-0x3E minigame door-warp.
    w.push(lui(AT, hi(WARP_HOUSEKEEP_VA))); //  \ _DAT_8007BAC0 = 0
    w.push(sw(ZERO, AT, lo(WARP_HOUSEKEEP_VA))); //  /
    w.push(lui(AT, hi(WINNINGS_VA))); //  \ _DAT_80084440 = 0
    w.push(sw(ZERO, AT, lo(WINNINGS_VA))); //  /
    w.push(addiu(V0, ZERO, sub_id)); //  \ _DAT_8007BA34 = sub_id
    w.push(lui(AT, hi(SUBID_VA))); //  |
    w.push(sh(V0, AT, lo(SUBID_VA))); //  /
    w.push(addiu(V0, ZERO, MODE_OTHER_INIT)); //  \ _DAT_8007B83C = 0x18 (mode 24)
    w.push(lui(AT, hi(MODE_INDEX_VA))); //  |
    w.push(sh(V0, AT, lo(MODE_INDEX_VA))); //  /
    w.push(addiu(A0, ZERO, SYSFLAG_WARP_ARG)); //  a0 = 0xE
    w.push(jal(SYSFLAG_SET_FN)); //  func_0x8003CE08(0xE)
    w.push(nop()); //  (delay)
    // .replay: the op-0x49 displaced pair, then return to the dispatcher.
    let replay = w.len();
    w.push(SHOP_DISPLACED[0]); // replay sw s6,-0x4bb0(s0)
    w.push(SHOP_DISPLACED[1]); // replay lbu v0,0(s6)
    w.push(j(SHOP_RETURN_VA)); // back to the dispatcher
    w.push(nop()); // (delay)
    for (i, rs) in to_replay {
        w[i] = bne(rs, ZERO, (replay as i32 - (i as i32 + 1)) as i16);
    }
    w
}

/// Assemble the **dead-mode request trigger** at [`TRIGGER_VA`] (reached by the
/// op-0x49 detour). It just requests [`DEAD_MODE_INDEX`] (writes the master mode
/// index), replays the op-0x49 displaced pair, and returns to the dispatcher.
/// The current frame finishes; next frame the mode SM enters our dead mode and
/// calls [`assemble_mode_init_loader_stub`] in the safe between-frames context.
/// No CD read here — the load is deferred to the mode handler.
pub fn assemble_mode_request_trigger() -> Vec<u32> {
    vec![
        lui(AT, hi(MODE_INDEX_VA)),       // 0:  \ v0 = current mode index
        lhu(V0, AT, lo(MODE_INDEX_VA)),   // 1:  /  (the mode we are interrupting)
        lui(AT, hi(ORIGIN_MODE_VA)),      // 2:  \ stash it for the loader to restore
        sh(V0, AT, lo(ORIGIN_MODE_VA)),   // 3:  /
        addiu(V0, ZERO, DEAD_MODE_INDEX), // 4:  \ _DAT_8007B83C = dead mode
        lui(AT, hi(MODE_INDEX_VA)),       // 5:  |  (request the mode)
        sh(V0, AT, lo(MODE_INDEX_VA)),    // 6:  /
        SHOP_DISPLACED[0],                // 7:  replay sw s6,-0x4bb0(s0)
        SHOP_DISPLACED[1],                // 8:  replay lbu v0,0(s6)
        j(SHOP_RETURN_VA),                // 9:  back to the dispatcher
        nop(),                            // 10: (delay)
    ]
}

/// Assemble the **mode-INIT loader** at [`MODE_INIT_VA`] (what the repurposed
/// dead mode's handler points at). Called by the mode SM via `jal` in the safe
/// between-frames context, so the proven load sequence runs without mid-tick
/// reentrancy: CD-read the pochi overlay (baked `lba`/`sectors`) to [`DEST`],
/// FlushCache, run it, then request [`FIELD_MODE_INDEX`] to return to the field
/// (the field overlay stays resident in slot A — we load to slot B) and `jr ra`.
/// `ra` is saved across the inner calls on the stack.
pub fn assemble_mode_init_loader_stub(lba: u32, sectors: u16) -> Vec<u32> {
    vec![
        addiu(SP, SP, 0xFFF8),            // 0:  addiu sp,sp,-8
        sw(RA, SP, 4),                    // 1:  sw ra,4(sp)
        addiu(A0, ZERO, sectors),         // 2:  a0 = sector_count
        lui(A1, imm_hi(lba)),             // 3:  \ a1 = lba
        ori(A1, A1, imm_lo(lba)),         // 4:  /
        lui(A2, imm_hi(DEST)),            // 5:  \ a2 = dest
        ori(A2, A2, imm_lo(DEST)),        // 6:  /
        jal(LOADER_FN),                   // 7:  FUN_8005E4D4(sectors, lba, dest)
        nop(),                            // 8:  (delay)
        addiu(T2, ZERO, BIOS_DISPATCH_A), // 9:  t2 = 0xA0
        jalr(T2),                         // 10: FlushCache()
        addiu(T1, ZERO, FLUSH_CACHE_FN),  // 11: (delay) t1 = 0x44
        lui(T0, imm_hi(DEST)),            // 12: \ t0 = dest
        ori(T0, T0, imm_lo(DEST)),        // 13: /
        jalr(T0),                         // 14: run the loaded overlay
        nop(),                            // 15: (delay)
        lui(AT, hi(ORIGIN_MODE_VA)),      // 16: \ v0 = the stashed origin mode
        lhu(V0, AT, lo(ORIGIN_MODE_VA)),  // 17: /
        lui(AT, hi(MODE_INDEX_VA)),       // 18: \ _DAT_8007B83C = origin mode
        sh(V0, AT, lo(MODE_INDEX_VA)),    // 19: /  (resume what we interrupted)
        lw(RA, SP, 4),                    // 20: lw ra,4(sp)
        addiu(SP, SP, 8),                 // 21: addiu sp,sp,8
        jr(RA),                           // 22: return to the mode SM
        nop(),                            // 23: (delay)
    ]
}

/// Assemble the **mode-24 overlay-load redirect** at [`WARP_REDIRECT_VA`]. It is
/// reached by a detour planted at [`WARP_INIT_DETOUR_VA`] inside `FUN_80025980`
/// (the mode-24 INIT), in place of the game's per-sub-id `jal FUN_8003EBE4`. For
/// **our** sub-id ([`WARP_SUBID`]) it baked-LBA-loads our pochi overlay to slot A
/// ([`SLOT_A_BASE`]), FlushCaches, runs the overlay's init, then calls the mode-24
/// return warp [`MODE24_RETURN_FN`] (restore scene + request mode 2 reload) and
/// runs `FUN_80025980`'s epilogue itself — bypassing the function's mode-0x19
/// hand-off (we go straight back to the field, not into a mode-25 minigame loop).
/// For any other sub-id it replays the original loader and rejoins at
/// [`WARP_INIT_REJOIN_VA`], so all 7 retail minigames are unaffected. `a0` still
/// holds `sub_id+0x4D` from the code preceding the detour (for the replayed load).
pub fn assemble_warp_init_redirect(lba: u32, sectors: u16) -> Vec<u32> {
    assemble_warp_init_redirect_opts(lba, sectors, true)
}

/// As [`assemble_warp_init_redirect`], but `call_return_warp` selects what happens
/// after our overlay's INIT returns. `true` (sentinel slice): call
/// [`MODE24_RETURN_FN`] for an immediate field reload. `false` (draw side): skip it
/// — the overlay's INIT itself requests the persistent draw mode (mode 13), so the
/// game keeps calling the overlay's TICK each frame until the TICK returns to field.
pub fn assemble_warp_init_redirect_opts(
    lba: u32,
    sectors: u16,
    call_return_warp: bool,
) -> Vec<u32> {
    // .ours is index 9; beq is index 3 -> offset = 9 - (3 + 1) = 5.
    const OURS: i16 = 5;
    let (ret0, ret1) = if call_return_warp {
        (jal(MODE24_RETURN_FN), nop())
    } else {
        (nop(), nop())
    };
    vec![
        lui(AT, hi(WARP_SUBID_VA)),     // 0:  \ v0 = current sub-id
        lhu(V0, AT, lo(WARP_SUBID_VA)), // 1:  /
        addiu(T0, ZERO, WARP_SUBID),    // 2:  t0 = our sub-id
        beq(V0, T0, OURS),              // 3:  if ours -> .ours (idx 9)
        nop(),                          // 4:  (delay)
        // .default: original loader (a0 = sub_id+0x4D intact), then rejoin.
        jal(OVERLAY_LOADER_A_FN), // 5:  FUN_8003EBE4(a0)
        nop(),                    // 6:  (delay)
        j(WARP_INIT_REJOIN_VA),   // 7:  back into FUN_80025980
        nop(),                    // 8:  (delay)
        // .ours: baked-LBA load our overlay to slot A, run it, return to field.
        addiu(A0, ZERO, sectors),         // 9:  a0 = sector_count
        lui(A1, imm_hi(lba)),             // 10: \ a1 = lba
        ori(A1, A1, imm_lo(lba)),         // 11: /
        lui(A2, imm_hi(SLOT_A_BASE)),     // 12: \ a2 = slot A base
        ori(A2, A2, imm_lo(SLOT_A_BASE)), // 13: /
        jal(LOADER_FN),                   // 14: FUN_8005E4D4(sectors, lba, slotA)
        nop(),                            // 15: (delay)
        addiu(T2, ZERO, BIOS_DISPATCH_A), // 16: t2 = 0xA0
        jalr(T2),                         // 17: FlushCache()
        addiu(T1, ZERO, FLUSH_CACHE_FN),  // 18: (delay) t1 = 0x44
        lui(T3, imm_hi(SLOT_A_BASE)),     // 19: \ t3 = slot-A overlay entry
        ori(T3, T3, imm_lo(SLOT_A_BASE)), // 20: /
        jalr(T3),                         // 21: run our overlay's init
        nop(),                            // 22: (delay)
        ret0,                             // 23: FUN_80026018 (sentinel) or nop (draw)
        ret1,                             // 24: (delay / nop)
        lw(RA, SP, WARP_INIT_RA_OFF),     // 25: \ FUN_80025980 epilogue, bypassing
        lw(S0, SP, WARP_INIT_S0_OFF),     // 26: |  its mode-0x19 hand-off
        addiu(SP, SP, WARP_INIT_FRAME),   // 27: /
        jr(RA),                           // 28: return from FUN_80025980
        nop(),                            // 29: (delay)
    ]
}

/// Assemble the **draw-side slot-A overlay** (loaded to [`SLOT_A_BASE`]). INIT
/// (offset 0) hands off to mode 13 so the game calls the TICK (offset
/// [`SLOT_A_TICK_OFFSET`]) each frame. The TICK draws a native window box
/// ([`BOX_FN`]) with a `"SERU TRADE"` title and the party lead's **learnable-seru
/// list** — it reads the count ([`SERU_COUNT_VA`]) + ids ([`SERU_IDS_VA`]) live
/// from the character record, looks each id up in the spell display-name table
/// ([`SERU_NAME_PTRS`]), and draws the name with [`TEXT_DRAW_FN`] (native font).
/// CROSS returns to the field; heartbeats the frame counter to [`SENTINEL_ADDR`].
pub fn assemble_draw_overlay() -> Vec<u32> {
    const STR_WORD: usize = 5;
    const COUNTER_WORD: usize = 13;
    let tick_word = (SLOT_A_TICK_OFFSET / 4) as usize; // 14
    let va = |word: usize| SLOT_A_BASE + (word as u32) * 4;
    let (str_va, counter_va) = (va(STR_WORD), va(COUNTER_WORD));

    let s = b"SERU TRADE\0";
    let sw_word = |i: usize| -> u32 {
        let b = |k: usize| -> u8 { s.get(i + k).copied().unwrap_or(0) };
        u32::from_le_bytes([b(0), b(1), b(2), b(3)])
    };

    let mut w = vec![0u32; tick_word];
    // INIT: request mode 13 (MAPDSIP MODE), whose per-frame handler ticks us.
    w[0] = addiu(V0, ZERO, MAPDISP_MODE_INDEX);
    w[1] = lui(AT, hi(MODE_INDEX_VA));
    w[2] = sh(V0, AT, lo(MODE_INDEX_VA));
    w[3] = jr(RA);
    w[4] = nop();
    w[STR_WORD] = sw_word(0);
    w[STR_WORD + 1] = sw_word(4);
    w[STR_WORD + 2] = sw_word(8);

    // Native FUN_80036888(str, 0, 0, x, y): y is the 5th arg -> stack at sp+0x10.
    let draw = |t: &mut Vec<u32>, ptr: u32, x: u16, y: u16| {
        t.push(addiu(V0, ZERO, y));
        t.push(sw(V0, SP, 0x10));
        t.push(lui(A0, hi(ptr)));
        t.push(addiu(A0, A0, lo(ptr)));
        t.push(addiu(A1, ZERO, 0));
        t.push(addiu(A2, ZERO, 0));
        t.push(addiu(A3, ZERO, x));
        t.push(jal(TEXT_DRAW_FN));
        t.push(nop());
    };
    // Native window/box frame: FUN_8002C69C(x, y, w, h) — 4 register args.
    let box_frame = |t: &mut Vec<u32>, x: u16, y: u16, bw: u16, bh: u16| {
        t.push(addiu(A0, ZERO, x));
        t.push(addiu(A1, ZERO, y));
        t.push(addiu(A2, ZERO, bw));
        t.push(addiu(A3, ZERO, bh));
        t.push(jal(BOX_FN));
        t.push(nop());
    };

    // TICK. Frame 0x28: sp+0x10 = stacked text arg (y), sp+0x14/0x18/0x1c = saved
    // s0/s1/s2 (loop vars survive the FUN_80036888 calls), sp+0x20 = saved ra.
    // Text is drawn first (lands in front); the window box last (behind the text).
    let mut t: Vec<u32> = vec![
        addiu(SP, SP, 0xFFD8), // sp -= 0x28
        sw(RA, SP, 0x20),
        sw(S0, SP, 0x14),
        sw(S1, SP, 0x18),
        sw(S2, SP, 0x1C),
    ];
    // Refresh the pad ourselves (mode 13 doesn't poll it) so PAD_CUR_VA is live.
    t.push(jal(PAD_POLL_FN));
    t.push(nop());
    draw(&mut t, str_va, 0x40, 0x30); // title: "SERU TRADE"

    // --- learnable-seru list: for i in 0..min(count, MAX): draw name(ids[i]) ---
    t.push(addiu(S0, ZERO, 0)); // s0 = i = 0
    t.push(lui(AT, hi(SERU_COUNT_VA))); // s1 = count
    t.push(lbu(S1, AT, lo(SERU_COUNT_VA)));
    t.push(slti(T0, S1, (SERU_MAX_ROWS + 1) as i16)); // count <= MAX ?
    let capb = t.len();
    t.push(0); // bne -> .capok (placeholder)
    t.push(nop());
    t.push(addiu(S1, ZERO, SERU_MAX_ROWS)); // else cap
    let capok = t.len();
    t[capb] = bne(T0, ZERO, (capok as i32 - (capb as i32 + 1)) as i16);
    t.push(addiu(S2, ZERO, 0x44)); // s2 = row y
    let lloop = t.len();
    t.push(slt(T0, S0, S1)); // i < count ?
    let endb = t.len();
    t.push(0); // beq -> .ldone (placeholder)
    t.push(nop());
    t.push(lui(T1, hi(SERU_IDS_VA))); // id = ids[i]
    t.push(addiu(T1, T1, lo(SERU_IDS_VA)));
    t.push(addu(T1, T1, S0));
    t.push(lbu(T4, T1, 0));
    t.push(sll(T6, T4, 2)); // id*0xC
    t.push(sll(T7, T6, 1));
    t.push(addu(T6, T7, T6));
    t.push(lui(T7, hi(SERU_NAME_PTRS))); // a0 = *(SERU_NAME_PTRS + id*0xC)
    t.push(addiu(T7, T7, lo(SERU_NAME_PTRS)));
    t.push(addu(T7, T7, T6));
    t.push(lw(A0, T7, 0));
    t.push(sw(S2, SP, 0x10)); // FUN_80036888(name, 0, 0, 0x48, y=s2)
    t.push(addiu(A1, ZERO, 0));
    t.push(addiu(A2, ZERO, 0));
    t.push(addiu(A3, ZERO, 0x48));
    t.push(jal(TEXT_DRAW_FN));
    t.push(nop());
    t.push(addiu(S2, S2, 0xE)); // y += 0xe
    t.push(addiu(S0, S0, 1)); // i++
    t.push(j(va(tick_word + lloop))); // j .lloop (absolute)
    t.push(nop());
    let ldone = t.len();
    t[endb] = beq(T0, ZERO, (ldone as i32 - (endb as i32 + 1)) as i16);

    box_frame(&mut t, 0x28, 0x28, 0xB0, 0x80); // window box (behind the text)

    // Exit on CROSS (held): if (pad & PAD_EXIT_MASK) -> FUN_80026018 (return).
    t.push(lui(AT, hi(PAD_CUR_VA)));
    t.push(lw(T0, AT, lo(PAD_CUR_VA)));
    t.push(andi(T1, T0, PAD_EXIT_MASK));
    let xb = t.len();
    t.push(0); // beq -> .noexit (placeholder)
    t.push(nop());
    t.push(jal(MODE24_RETURN_FN));
    t.push(nop());
    let noexit = t.len();
    t[xb] = beq(T1, ZERO, (noexit as i32 - (xb as i32 + 1)) as i16);
    // Heartbeat: counter++ -> SENTINEL_ADDR (proves the tick is live).
    t.push(lui(AT, hi(counter_va)));
    t.push(lw(V0, AT, lo(counter_va)));
    t.push(addiu(V0, V0, 1));
    t.push(sw(V0, AT, lo(counter_va)));
    t.push(lui(AT, hi(SENTINEL_ADDR)));
    t.push(sw(V0, AT, lo(SENTINEL_ADDR)));
    // Epilogue: restore s0/s1/s2 + ra.
    t.push(lw(RA, SP, 0x20));
    t.push(lw(S0, SP, 0x14));
    t.push(lw(S1, SP, 0x18));
    t.push(lw(S2, SP, 0x1C));
    t.push(addiu(SP, SP, 0x28));
    t.push(jr(RA));
    t.push(nop());
    w.extend(t);
    w
}

/// The two detour words written at the hook: `j STUB_VA` then `nop`.
pub fn detour_words() -> [u32; 2] {
    [j(STUB_VA), nop()]
}

/// The two words planted at [`WARP_INIT_DETOUR_VA`] inside `FUN_80025980`:
/// `j WARP_REDIRECT_VA` then `nop` (replacing its `jal FUN_8003EBE4` + delay).
pub fn warp_init_detour_words() -> [u32; 2] {
    [j(WARP_REDIRECT_VA), nop()]
}

/// Number of disc sectors needed to hold `byte_len` bytes (2048-byte sectors).
pub fn sectors_for(byte_len: usize) -> u16 {
    byte_len.div_ceil(2048) as u16
}

/// Serialize a word list to a little-endian byte blob.
pub fn words_to_bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinel_overlay_writes_the_sentinel() {
        let w = assemble_sentinel_overlay();
        assert_eq!(w.len(), 6);
        // v0 = 0x5E2D7ADE via lui+ori
        assert_eq!(w[0], lui(V0, 0x5E2D));
        assert_eq!(w[1], ori(V0, V0, 0x7ADE));
        // store to SENTINEL_ADDR (0x8007AF20): hi corrects for the +0x20 lo.
        assert_eq!(w[2], lui(V1, hi(SENTINEL_ADDR)));
        assert_eq!(w[3], sw(V0, V1, lo(SENTINEL_ADDR)));
        assert_eq!(w[4], jr(RA));
        assert_eq!(w[5], 0);
    }

    #[test]
    fn loader_stub_calls_the_reader_then_the_overlay() {
        let lba = 0x0004_2A17u32;
        let sectors = 1u16;
        let displaced = [0x3c03_801du32, 0x2464_9070u32];
        let return_va = 0x801E_5A18u32;
        let s = assemble_loader_stub(lba, sectors, displaced, return_va);
        assert_eq!(s.len(), 18);

        // a0 = sectors, a1 = lba (lui+ori), a2 = DEST (lui+ori).
        assert_eq!(s[0], addiu(A0, ZERO, sectors));
        assert_eq!(s[1], lui(A1, 0x0004));
        assert_eq!(s[2], ori(A1, A1, 0x2A17));
        assert_eq!(s[3], lui(A2, imm_hi(DEST)));
        assert_eq!(s[4], ori(A2, A2, imm_lo(DEST)));

        // jal lands on the loader function.
        assert_eq!((s[5] & 0x03ff_ffff) << 2, LOADER_FN & 0x0fff_ffff);
        // FlushCache: li t2,0xA0 ; jalr t2 ; (delay) li t1,0x44.
        assert_eq!(s[7], addiu(T2, ZERO, BIOS_DISPATCH_A));
        assert_eq!(s[8], jalr(T2));
        assert_eq!(s[9], addiu(T1, ZERO, FLUSH_CACHE_FN));
        // jalr t0 calls the loaded overlay at DEST.
        assert_eq!(s[10], lui(T0, imm_hi(DEST)));
        assert_eq!(s[12], jalr(T0));
        // displaced pair replayed, then j back to the hook join.
        assert_eq!(s[14], displaced[0]);
        assert_eq!(s[15], displaced[1]);
        assert_eq!((s[16] & 0x03ff_ffff) << 2, return_va & 0x0fff_ffff);
    }

    #[test]
    fn shop_stub_gates_on_sub_op_and_replays() {
        let s = assemble_shop_loader_stub(0x0004_2A17, 1);
        assert_eq!(s.len(), 21);
        // Gate: lbu t3,0(s6) ; bne t3,zero,->replay.
        assert_eq!(s[0], lbu(T3, S6, 0));
        assert_eq!(s[1] >> 26, 0x05, "bne opcode");
        // bne target = replay block (idx 17): off (words) = 17 - (1+1) = 15.
        let off = (s[1] & 0xffff) as i16;
        let target = (1 + 1) + off as i32; // branch idx+1 + off
        assert_eq!(target, 17, "bne skips to the replay block");
        // loader call + FlushCache + overlay call present.
        assert_eq!((s[8] & 0x03ff_ffff) << 2, LOADER_FN & 0x0fff_ffff);
        assert_eq!(s[10], addiu(T2, ZERO, BIOS_DISPATCH_A));
        assert_eq!(s[11], jalr(T2));
        assert_eq!(s[15], jalr(T0));
        // Replay the exact displaced pair, then jump back.
        assert_eq!(s[17], SHOP_DISPLACED[0]);
        assert_eq!(s[18], SHOP_DISPLACED[1]);
        assert_eq!((s[19] & 0x03ff_ffff) << 2, SHOP_RETURN_VA & 0x0fff_ffff);
        // Fits the gap window below the config blob.
        assert!(STUB_VA + (s.len() as u32) * 4 <= 0x8007_AF00);
    }

    #[test]
    fn ungated_stub_neuters_the_gate_branch_only() {
        let g = assemble_shop_loader_stub_gated(0x0004_2A17, 1, true);
        let u = assemble_shop_loader_stub_gated(0x0004_2A17, 1, false);
        // Same length + layout; only the gate branch (idx 1) differs.
        assert_eq!(g.len(), u.len());
        for i in 0..g.len() {
            if i == 1 {
                continue;
            }
            assert_eq!(g[i], u[i], "word {i} must be identical");
        }
        // Gated: bne t3,zero (rs = T3). Ungated: bne zero,zero (rs = ZERO, never taken).
        assert_eq!((g[1] >> 21) & 0x1f, T3);
        assert_eq!((u[1] >> 21) & 0x1f, ZERO);
        assert_eq!(
            u[1] >> 26,
            0x05,
            "still a bne (never-taken), so layout holds"
        );
        // Same branch displacement either way.
        assert_eq!(g[1] & 0xffff, u[1] & 0xffff);
    }

    #[test]
    fn warp_trigger_mirrors_the_op_0x3e_idiom() {
        let sub_id = 7u16;
        let s = assemble_warp_trigger_stub(sub_id, false);
        assert_eq!(s.len(), 17);
        // Two housekeeping zeroing stores.
        assert_eq!(s[0], lui(AT, hi(WARP_HOUSEKEEP_VA)));
        assert_eq!(s[1], sw(ZERO, AT, lo(WARP_HOUSEKEEP_VA)));
        assert_eq!(s[2], lui(AT, hi(WINNINGS_VA)));
        assert_eq!(s[3], sw(ZERO, AT, lo(WINNINGS_VA)));
        // sub-id written before the mode (mirrors op-0x3E ordering).
        assert_eq!(s[4], addiu(V0, ZERO, sub_id));
        assert_eq!(s[6], sh(V0, AT, lo(SUBID_VA)));
        // mode index <- 0x18 (mode 24 OTHER INIT).
        assert_eq!(s[7], addiu(V0, ZERO, MODE_OTHER_INIT));
        assert_eq!(s[9], sh(V0, AT, lo(MODE_INDEX_VA)));
        // SysFlag setter call with a0 = 0xE.
        assert_eq!(s[10], addiu(A0, ZERO, SYSFLAG_WARP_ARG));
        assert_eq!((s[11] & 0x03ff_ffff) << 2, SYSFLAG_SET_FN & 0x0fff_ffff);
        // Replay the op-0x49 displaced pair, then j back to the dispatcher.
        assert_eq!(s[13], SHOP_DISPLACED[0]);
        assert_eq!(s[14], SHOP_DISPLACED[1]);
        assert_eq!((s[15] & 0x03ff_ffff) << 2, SHOP_RETURN_VA & 0x0fff_ffff);
        // Mode 24 (not 25): we request INIT, which loads + hands off to RUN itself.
        assert_eq!(MODE_OTHER_INIT, 0x18);
        // Fits the gap free window below the config blob at 0x8007AF00.
        assert!(STUB_VA + (s.len() as u32) * 4 <= 0x8007_AF00);
        // The warp globals resolve to the addresses pinned from overlay_0897.
        assert_eq!(MODE_INDEX_VA, 0x8007_B83C);
        assert_eq!(SUBID_VA, 0x8007_BA34);
        assert_eq!(WINNINGS_VA, 0x8008_4440);
        assert_eq!(WARP_HOUSEKEEP_VA, 0x8007_BAC0);
    }

    #[test]
    fn warp_trigger_fire_once_guard() {
        let s = assemble_warp_trigger_stub(WARP_SUBID, true);
        // Prefix: read the fired flag, branch past the warp if already set.
        assert_eq!(s[0], lui(AT, hi(WARP_FIRED_VA)));
        assert_eq!(s[1], lhu(V0, AT, lo(WARP_FIRED_VA)));
        assert_eq!(s[2] >> 26, 0x05, "bne (skip warp if fired)");
        assert_eq!(s[4], addiu(V0, ZERO, 1), "set the fired flag");
        assert_eq!(s[6], sh(V0, AT, lo(WARP_FIRED_VA)));
        // The bne targets .replay: replay is the SHOP_DISPLACED pair near the end.
        let off = s[2] as u16 as i16 as i32;
        let target = 2 + 1 + off;
        assert_eq!(
            s[target as usize], SHOP_DISPLACED[0],
            "bne -> .replay (skip warp)"
        );
        // The warp arm still sets mode 0x18 and the sub-id.
        assert!(s.contains(&addiu(V0, ZERO, MODE_OTHER_INIT)));
        assert!(s.contains(&addiu(V0, ZERO, WARP_SUBID)));
        // Fire-once trigger + redirect both fit the gap window, no overlap.
        assert!(WARP_TRIGGER_VA + (s.len() as u32) * 4 <= WARP_REDIRECT_VA);
    }

    #[test]
    fn dead_mode_trigger_and_loader_layout() {
        let trig = assemble_mode_request_trigger();
        let load = assemble_mode_init_loader_stub(0x0004_2A17, 1);
        // Trigger: stash the interrupted mode, request the dead mode, replay, return.
        assert_eq!(trig[1], lhu(V0, AT, lo(MODE_INDEX_VA)));
        assert_eq!(trig[3], sh(V0, AT, lo(ORIGIN_MODE_VA)));
        assert_eq!(trig[4], addiu(V0, ZERO, DEAD_MODE_INDEX));
        assert_eq!(trig[6], sh(V0, AT, lo(MODE_INDEX_VA)));
        assert_eq!(trig[7], SHOP_DISPLACED[0]);
        assert_eq!(trig[8], SHOP_DISPLACED[1]);
        assert_eq!((trig[9] & 0x03ff_ffff) << 2, SHOP_RETURN_VA & 0x0fff_ffff);
        // The trigger fits below the loader (no overlap at MODE_INIT_VA).
        assert!(TRIGGER_VA + (trig.len() as u32) * 4 <= MODE_INIT_VA);
        // Loader: sp frame saves ra; loads via FUN_8005E4D4; FlushCache; runs the
        // overlay; restores the stashed origin mode; restores ra; jr ra.
        assert_eq!(load[0], addiu(SP, SP, 0xFFF8));
        assert_eq!(load[1], sw(RA, SP, 4));
        assert_eq!((load[7] & 0x03ff_ffff) << 2, LOADER_FN & 0x0fff_ffff);
        assert_eq!(load[10], jalr(T2));
        assert_eq!(load[14], jalr(T0));
        assert_eq!(load[17], lhu(V0, AT, lo(ORIGIN_MODE_VA)));
        assert_eq!(load[19], sh(V0, AT, lo(MODE_INDEX_VA)));
        assert_eq!(load[20], lw(RA, SP, 4));
        assert_eq!(load[22], jr(RA));
        // Loader fits the gap window below the config blob at 0x8007AF00.
        assert!(MODE_INIT_VA + (load.len() as u32) * 4 <= 0x8007_AF00);
        // The dead mode's handler word lands inside the mode table.
        assert_eq!(dead_mode_handler_va(), MODE_TABLE_VA + 10 * 24 + 0x10);
    }

    #[test]
    fn draw_overlay_layout() {
        let o = assemble_draw_overlay();
        // INIT (offset 0): hand off to mode 13, then return.
        assert_eq!(o[0], addiu(V0, ZERO, MAPDISP_MODE_INDEX));
        assert_eq!(o[2], sh(V0, AT, lo(MODE_INDEX_VA)));
        assert_eq!(o[3], jr(RA));
        // The string lives at word 5 and reads "SERU" in the first word.
        assert_eq!(o[5], u32::from_le_bytes(*b"SERU"));
        // TICK starts at offset 0x38 (word 14).
        let tick = (SLOT_A_TICK_OFFSET / 4) as usize;
        assert_eq!(tick, 14);
        assert_eq!(
            o[tick],
            addiu(SP, SP, 0xFFD8),
            "tick begins with the 0x28 sp frame (saves s0-s2 + ra)"
        );
        // The tick refreshes the pad, draws the box + native text + the seru list,
        // and exits via the return warp. (Checked by presence to stay robust to
        // layout shifts as the UI grows.)
        let body = &o[tick..];
        assert!(body.contains(&jal(PAD_POLL_FN)), "refreshes the pad");
        assert!(body.contains(&jal(BOX_FN)), "draws the native window box");
        assert!(body.contains(&jal(TEXT_DRAW_FN)), "draws native text");
        assert!(body.contains(&jal(MODE24_RETURN_FN)), "exit -> return warp");
        // Native text passes its y arg on the stack at sp+0x10 (o32 5th arg).
        assert!(body.contains(&sw(V0, SP, 0x10)), "y passed on the stack");
        // The title string ptr (SLOT_A_BASE + 5*4) is loaded for the title draw.
        let str_va = SLOT_A_BASE + 5 * 4;
        assert!(body.contains(&lui(A0, hi(str_va))) && body.contains(&addiu(A0, A0, lo(str_va))));
        // The seru list reads the count + indexes the spell-name pointer table.
        assert!(
            body.contains(&lbu(S1, AT, lo(SERU_COUNT_VA))),
            "reads seru count"
        );
        assert!(
            body.contains(&lui(T7, hi(SERU_NAME_PTRS))),
            "indexes the name table"
        );
        assert!(body.contains(&slt(T0, S0, S1)), "loops i < count");
        // The draw redirect (call_return_warp=false) does NOT call the return warp;
        // the overlay INIT requests the persistent mode instead.
        let r_draw = assemble_warp_init_redirect_opts(0x0004_2A17, 1, false);
        assert!(
            !r_draw.contains(&jal(MODE24_RETURN_FN)),
            "draw redirect skips return-warp"
        );
        let r_sent = assemble_warp_init_redirect_opts(0x0004_2A17, 1, true);
        assert!(
            r_sent.contains(&jal(MODE24_RETURN_FN)),
            "sentinel redirect keeps return-warp"
        );
    }

    #[test]
    fn trade_handler_renders_per_owner_offer() {
        let h = assemble_trade_handler();
        // Fits the 0899 run-C dead region it's embedded in, and is hosted in that
        // overlay (VA inside the 0899 image window), not the SCUS gap.
        let end = TRADE_HANDLER_VA + (h.len() as u32) * 4;
        assert!(end <= TRADE_HANDLER_END && TRADE_HANDLER_VA >= SLOT_A_BASE);
        // Refreshes the pad, draws native text + the level number + the window box.
        assert!(h.contains(&jal(PAD_POLL_FN)), "refreshes the pad");
        assert!(h.contains(&jal(TEXT_DRAW_FN)), "draws native text");
        assert!(h.contains(&jal(NUMBER_FN)), "draws the LVL number");
        assert!(h.contains(&jal(BOX_FN)), "draws the window box (last)");
        assert!(
            h.contains(&jal(FINALIZE_FN)),
            "replays the menu finalize tail"
        );
        // Indexes the precomputed bucket schedule (live build, demo off) and the
        // spell-name pointer table.
        if !SERU_DEMO_FORCE_WANT {
            assert!(h.contains(&lw(T0, AT, lo(PLAY_TIME_VA))), "reads play-time");
            assert!(h.contains(&divu(T0, T1)), "divides into a bucket");
            assert!(
                h.contains(&andi(T0, T0, BUCKET_INDEX_MASK)),
                "wraps to BUCKET_COUNT"
            );
            assert!(
                h.contains(&addiu(T1, T1, lo(BUCKET_TABLE_VA))),
                "indexes the bucket table"
            );
        }
        assert!(
            h.contains(&lui(T7, hi(SERU_NAME_PTRS))),
            "indexes the spell-name table"
        );
        // Scans the four party records: slot < 4, record stride 0x414, per-record
        // seru count + id array.
        assert!(
            h.contains(&slti(T0, S0, PARTY_SLOT_COUNT)),
            "loops party slots"
        );
        assert!(
            h.contains(&addiu(T1, ZERO, CHAR_RECORD_STRIDE)),
            "uses the record stride"
        );
        assert!(h.contains(&slt(T0, S1, S5)), "loops each owner's seru list");
        // A bne on (rs=id reg T4, rt=want reg S3) skips non-matching ids (the branch
        // displacement is patched, so match on opcode + register fields).
        assert!(
            h.iter()
                .any(|&x| x >> 26 == 0x05 && (x >> 21) & 0x1f == T4 && (x >> 16) & 0x1f == S3),
            "compares each owned id against the want"
        );
        // Owner name comes from the record name field (+0x2A7).
        assert!(
            h.contains(&addiu(A0, S4, RECORD_NAME_OFFSET)),
            "draws the owner name from the record"
        );
        // ○ (CANCEL) in browse exits: an andi against the cancel mask + the flag clear.
        assert!(
            h.iter()
                .any(|&x| x >> 26 == 0x0c && (x & 0xffff) == HANDLER_CANCEL_MASK as u32),
            "○ exit tests the cancel mask"
        );
        assert!(
            h.contains(&sw(ZERO, AT, lo(TRADE_ACTIVE_VA))),
            "clears the active flag"
        );
        // Confirm sub-state: enters on ✕, draws the prompt + Yes/No, navigates yes/no.
        assert!(
            h.iter()
                .any(|&x| x >> 26 == 0x0c && (x & 0xffff) == PAD_CONFIRM_MASK as u32),
            "✕ edge tested (enter/confirm)"
        );
        assert!(
            h.contains(&addiu(A0, A0, lo(CONFIRM_PROMPT_STR_VA))),
            "draws the @Trade? prompt"
        );
        assert!(
            h.contains(&addiu(A0, A0, lo(CONFIRM_YES_STR_VA)))
                && h.contains(&addiu(A0, A0, lo(CONFIRM_NO_STR_VA))),
            "draws Yes + No"
        );
        assert!(
            h.contains(&sw(ZERO, AT, lo(TRADE_CONFIRM_VA))),
            "✕/○ in confirm clears the sub-state"
        );
        // On exit it slides the picker windows back in via the widget VM.
        assert!(
            h.contains(&jal(WIDGET_VM_FN)),
            "exit slides the picker back in"
        );
        assert!(
            h.contains(&addiu(A0, A0, lo(SLIDE_OPEN_SCRIPT_VA))),
            "exit runs the open script"
        );
        // The dispatch stub slides the picker away (Sell script) on Trade confirm.
        let d = assemble_trade_dispatch_stub();
        assert!(d.contains(&jal(WIDGET_VM_FN)), "Trade confirm slides away");
        assert!(
            d.contains(&addiu(A0, A0, lo(SLIDE_AWAY_SCRIPT_VA))),
            "confirm runs the Sell slide-away script"
        );
    }

    #[test]
    fn warp_init_redirect_layout_and_branch() {
        let r = assemble_warp_init_redirect(0x0004_2A17, 1);
        assert_eq!(r.len(), 30);
        // sub-id read + compare against our sub-id.
        assert_eq!(r[1], lhu(V0, AT, lo(WARP_SUBID_VA)));
        assert_eq!(r[2], addiu(T0, ZERO, WARP_SUBID));
        // beq to the .ours block (index 9): offset words = 9 - (3+1) = 5.
        assert_eq!(r[3] >> 26, 0x04, "beq opcode");
        assert_eq!(
            (1 + 3 + (r[3] as u16 as i16) as i32),
            9,
            "beq targets .ours"
        );
        // .default: original loader, then rejoin into FUN_80025980.
        assert_eq!((r[5] & 0x03ff_ffff) << 2, OVERLAY_LOADER_A_FN & 0x0fff_ffff);
        assert_eq!((r[7] & 0x03ff_ffff) << 2, WARP_INIT_REJOIN_VA & 0x0fff_ffff);
        // .ours: load to slot A, FlushCache, run overlay, return-warp, epilogue.
        assert_eq!((r[14] & 0x03ff_ffff) << 2, LOADER_FN & 0x0fff_ffff);
        assert_eq!(r[12], lui(A2, imm_hi(SLOT_A_BASE)));
        assert_eq!(r[17], jalr(T2));
        assert_eq!(r[21], jalr(T3));
        assert_eq!((r[23] & 0x03ff_ffff) << 2, MODE24_RETURN_FN & 0x0fff_ffff);
        assert_eq!(r[25], lw(RA, SP, WARP_INIT_RA_OFF));
        assert_eq!(r[28], jr(RA));
        // Fits the gap window below the config blob.
        assert!(WARP_REDIRECT_VA + (r.len() as u32) * 4 <= 0x8007_AF00);
        // Detour words jump to the redirect; displaced guard matches the build.
        assert_eq!(
            (warp_init_detour_words()[0] & 0x03ff_ffff) << 2,
            WARP_REDIRECT_VA & 0x0fff_ffff
        );
        assert_eq!(WARP_INIT_REJOIN_VA, WARP_INIT_DETOUR_VA + 8);
    }

    #[test]
    fn shop_hook_file_offset_is_in_the_field_overlay() {
        // The hook VA maps linearly from the overlay base.
        assert_eq!(SHOP_HOOK_VA - SHOP_OVERLAY_BASE, 0x12190);
        assert_eq!(SHOP_RETURN_VA, SHOP_HOOK_VA + 8);
    }

    #[test]
    fn detour_jumps_to_the_stub() {
        let d = detour_words();
        assert_eq!((d[0] & 0x03ff_ffff) << 2, STUB_VA & 0x0fff_ffff);
        assert_eq!(d[1], 0);
    }

    #[test]
    fn stub_fits_the_gap_free_window() {
        // The stub at 0x8007AE00 must stay below the config blob at 0x8007AF00.
        let s = assemble_loader_stub(0, 1, [0, 0], 0);
        let end = STUB_VA + (s.len() as u32) * 4;
        assert!(end <= 0x8007_AF00, "stub overruns into the config blob");
        // ...and the sentinel cell sits in the reserved tail after the blob.
        assert!((0x8007_AF18..0x8007_AF40).contains(&SENTINEL_ADDR));
    }

    #[test]
    fn sectors_for_rounds_up() {
        assert_eq!(sectors_for(1), 1);
        assert_eq!(sectors_for(2048), 1);
        assert_eq!(sectors_for(2049), 2);
        assert_eq!(sectors_for(0), 0);
    }
}
