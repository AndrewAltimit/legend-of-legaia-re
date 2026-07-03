//! Address / register / layout constants for the seru-trade overlay.
//!
//! Pure data: VA-address constants (loader / stub / mode-table / picker /
//! trade-screen cells), MIPS register numbers, and the trade box geometry. No
//! parent items are referenced, so this leaf module needs no `use super::*`.

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

pub(crate) const ZERO: u32 = 0;
pub(crate) const AT: u32 = 1;
pub(crate) const A0: u32 = 4;
pub(crate) const A1: u32 = 5;
pub(crate) const A2: u32 = 6;
pub(crate) const A3: u32 = 7;
pub(crate) const V0: u32 = 2;
pub(crate) const V1: u32 = 3;
pub(crate) const T0: u32 = 8;
pub(crate) const T1: u32 = 9;
pub(crate) const T2: u32 = 10;
pub(crate) const T3: u32 = 11;
pub(crate) const T4: u32 = 12;
pub(crate) const T5: u32 = 13;
pub(crate) const T6: u32 = 14;
pub(crate) const T7: u32 = 15;
pub(crate) const S0: u32 = 16;
pub(crate) const S1: u32 = 17;
pub(crate) const S2: u32 = 18;
pub(crate) const S3: u32 = 19;
pub(crate) const S4: u32 = 20;
pub(crate) const S5: u32 = 21;
pub(crate) const S6: u32 = 22;
pub(crate) const S7: u32 = 23;
pub(crate) const GP: u32 = 28;
pub(crate) const SP: u32 = 29;
pub(crate) const RA: u32 = 31;

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
/// `FUN_8003CE08(0xE)` - SysFlag set (field-VM 4th flag bank), called by the warp.
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
/// 0x40 above [`TRIGGER_VA`] - past the trigger, still in the gap window.
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
// it, and on exit `FUN_80026018` restores + RELOADS the scene (mode 2) - a clean
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
/// `FUN_80026018` - the mode-24 return warp (restore scene + mode 2 reload).
pub const MODE24_RETURN_FN: u32 = 0x8002_6018;
/// `FUN_80025980` stack-frame layout (so the redirect can return cleanly,
/// bypassing the function's mode-0x19 epilogue): `lw ra,0x14(sp)`, `lw s0,0x10(sp)`,
/// `addiu sp,sp,0x18`.
pub const WARP_INIT_FRAME: u16 = 0x18;
pub const WARP_INIT_RA_OFF: u16 = 0x14;
pub const WARP_INIT_S0_OFF: u16 = 0x10;

/// Gap-tail "warp already fired" flag (u16). With fire-once enabled, the trigger
/// warps a single time then lets the interrupted menu proceed - used for the
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
// handler calls `func_0x801ce850` = SLOT_A_BASE + 0x38 every frame - so the
// overlay's TICK lives at offset 0x38 and draws there. The general per-frame
// updater (`FUN_80016444`, also run by `FUN_80025f2c`) flushes the queued text.

/// Game-mode index 13 (MAPDSIP MODE): per-frame handler `FUN_80025f2c` calls the
/// slot-A overlay tick at `SLOT_A_BASE + 0x38` directly each frame - our draw loop.
pub const MAPDISP_MODE_INDEX: u16 = 13;
/// Native menu text drawer `FUN_80036888(a0=str, a1=0, a2=maxchars(0=all), a3=x,
/// [sp+0x10]=y)` - the game's own dialog-font renderer (decodes the ASCII string
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
/// even at name-entry - proves the live read) and spans through the seru count
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
/// (It cannot conjure a line on a *fresh* save - the per-owner render is empty when
/// nobody owns the want, which is correct behaviour, not a render bug.)
pub const SERU_DEMO_FORCE_WANT: bool = false;
/// The fixed want id used when [`SERU_DEMO_FORCE_WANT`]. `0x81` = Gimard, the first
/// player Seru-magic id; `+1` (`0x82`) is the forced give-back.
pub const SERU_DEMO_BASE_ID: u16 = 0x81;

/// Native window/box-frame draw `FUN_8002C69C(a0=x, a1=y, a2=w, a3=h)` (POLY_FT4
/// / SPRT emitter - the dialog/menu window). SCUS-resident; 4 register args.
pub const BOX_FN: u32 = 0x8002_C69C;
/// Window-skin selector global `gp[+`[`WINDOW_SKIN_OFF`]`]` (set by `FUN_80034b6c`,
/// read by [`BOX_FN`]): a skin index into the corner/edge table `DAT_800732a4`
/// (12-byte stride). The per-frame finalize `FUN_80031d00` rewrites it from each
/// drawn window's `+0x1d` skin byte, so it holds the *last* window's skin - which,
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
pub const TRADE_SLIDE_DELTA_VA: u32 = 0x801E_7E24;
/// Cursor index over the per-owner trade lines (0 = first line). SCUS gap, resident;
/// the handler nav-clamps it to `[0, line_count)` each frame.
pub const TRADE_CURSOR_VA: u32 = 0x801E_7E28;
/// Previous-frame pad mask, for D-pad edge detection (one step per press, not per
/// held frame). SCUS gap, resident.
pub const TRADE_PAD_PREV_VA: u32 = 0x801E_7E2C;
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
pub const TRADE_CONFIRM_VA: u32 = 0x801E_7E30;
/// Yes/No selection in the confirm sub-state (0 = Yes, 1 = No). SCUS gap, resident.
pub const TRADE_YESNO_VA: u32 = 0x801E_7E34;
/// The current offer's give-back id, stashed by the offer compute so the per-owner
/// loop can skip owners who already own it (a pointless trade). SCUS gap, resident.
pub const TRADE_GIVE_ID_VA: u32 = 0x801E_7E38;
/// Selected owner's record base + want-index, stashed by the render loop when it
/// draws the cursor's line, so the swap on ✕-Yes writes the right record without a
/// re-scan. SCUS gap, resident.
pub const TRADE_SEL_BASE_VA: u32 = 0x801E_7E3C;
pub const TRADE_SEL_J_VA: u32 = 0x801E_7E40;

/// Confirm-prompt strings, embedded in 0899 above the handler (resident in-shop).
/// Drawn only in the confirm sub-state; the selected line + reward header already
/// show *what* is being traded, so the prompt just needs the question + choices.
pub const CONFIRM_PROMPT_STR_VA: u32 = 0x801E_7D40;
pub const CONFIRM_PROMPT_STR: &[u8] = b"@Trade?\0";
pub const CONFIRM_YES_STR_VA: u32 = 0x801E_7D50;
pub const CONFIRM_YES_STR: &[u8] = b"@Yes\0";
pub const CONFIRM_NO_STR_VA: u32 = 0x801E_7D58;
pub const CONFIRM_NO_STR: &[u8] = b"@No\0";
/// Confirm-prompt layout (inside the box, near its bottom): question row y, choices
/// row y, and the Yes / No / cursor x columns.
pub const PROMPT_Y: u16 = 0xB4;
pub const CHOICE_Y: u16 = 0xC4;
pub const YES_X: u16 = 0x40;
pub const NO_X: u16 = 0x80;
/// Cursor x just left of a Yes/No choice (re-uses the line-cursor sprite).
pub const CHOICE_CURSOR_DX: u16 = 0x10;
/// Native animated cursor sprite `FUN_8002b994(slot, mode, x, y)` - a 16×16 bobbing
/// cursor (slot 0 = the standard menu cursor; `mode = 1` animates). Drawn at the
/// selected owner line. (Already used by the row-4 stub as [`HIGHLIGHT_FN`].)
pub const CURSOR_DRAW_FN: u32 = 0x8002_B994;
/// Cursor sprite x: just left of the per-owner want column.
pub const CURSOR_X: u16 = COL_WANT_X - 0x10;
/// Initial slide offset: box fully off the left edge (`-(BOX_X + BOX_W)` rounded).
pub const SLIDE_START_OFF: i16 = -0xF0;
/// Per-frame slide step toward 0. Divides `SLIDE_START_OFF` evenly so the box lands
/// exactly on 0 (no overshoot) - see the compile-time asserts below.
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
/// if nothing is visible - distinguishes "tick ran, draw invisible" from "tick
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
/// depth as Buy/Sell/Quit (in front of the box background) - an epilogue draw
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
/// [u32 draw-handler]` - here x=0x2a, y=0x2e, w=0x50, h=0x26, handler=FUN_801d4868
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
// leaving the gold (0x20) + vendor-name (0x21) boxes - exactly the "slide the menus
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
/// Right=0x2000, Down=0x4000, Left=0x8000). So ○ = 0x20 - NOT 0x2000 (= Right) and
/// NOT ✕ (0x40 = CONFIRM, which opens Trade).
pub const HANDLER_CANCEL_MASK: u16 = 0x0020;
/// Private "trade screen active" flag in dead gap space. The dispatch stub sets it,
/// the entry detour gates on it, the handler clears it on exit. We DON'T reuse the
/// picker sub-state `DAT_801e46ac`: the menu owns it - `FUN_801dc6b4` case 2 resets
/// it (`DAT_801e46ac = 0`) whenever its draw-state shadow desyncs, wiping our value.
///
/// ALL seru-trade pieces (this flag + the other cells, both stubs, the row-4 stub,
/// the strings, and the bucket table) live in the 0899 run-C dead region alongside
/// the handler - NOT the SCUS rodata gap. That gap is crowded by other randomizer
/// features (the Seru-Bell name at `0x8007AB40`, the bonus-equipment-drop routine at
/// `0x8007AB80`, the flee-EXP routine at `0x8007AD00`), so hosting in 0899 keeps
/// seru trading compatible with all of them. 0899 is resident throughout the shop
/// (the only time these run), and the region reloads with the overlay, so the cells
/// reset to 0 on each load and are re-initialised by the dispatch stub on entry.
pub const TRADE_ACTIVE_VA: u32 = 0x801E_7E20;

/// 0899 run-C VAs (all above the handler, below the run-C end; non-overlapping -
/// asserted by `trade_0899_layout_is_disjoint`). Reached by `j` from the 0899
/// detours / handler, so no SCUS gap is used.
pub const ENTRY_STUB_VA: u32 = 0x801E_7B00;
/// Reorder dispatch stub (cursor 2 → Trade sub-mode, 3 → Quit, 0/1 → Buy/Sell).
pub const TRADE_DISPATCH_STUB_VA: u32 = 0x801E_7B60;
/// The in-shop trade-screen handler (draws + input + swap; runs in mode 0x17).
///
/// HOSTED IN THE MENU OVERLAY 0899, not the tiny SCUS rodata gap: the gap only had
/// ~116 words free, far too little for the full screen (slide + cursor + confirm +
/// swap). 0899 is the overlay that hosts the shop and is resident throughout it, and
/// it carries a large reference-free zero region (run-C, file `0x18CC7`, VA
/// `0x801E74DF`, ~0xF00 bytes) that is part of the loaded image (reloaded with the
/// overlay) and verified all-zero across the trade screen + both slide-transition
/// states - so the handler embeds there, resident during every shop, with ~960 words
/// of room and no runtime CD load. The entry detour (in the SCUS gap) `j`s here.
pub const TRADE_HANDLER_VA: u32 = 0x801E_74E0;
/// Upper bound the handler body must stay below (end of the 0899 run-C dead region,
/// `0x801E83E2`, rounded down with a small margin).
pub const TRADE_HANDLER_END: u32 = 0x801E_83E0;
/// PROT entry hosting the handler (the menu overlay - same entry the picker edits
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
pub const BUCKET_TABLE_VA: u32 = 0x801E_7D60;
/// Byte length of the on-disc bucket schedule (mirrors the shared kernel).
pub const BUCKET_TABLE_LEN: usize = legaia_asset::seru_trade::BUCKET_TABLE_LEN;

/// Retail play-time counter `_DAT_80084570` (u32 game-time seconds). The handler
/// reads it to pick the current bucket; the engine mirrors it as
/// `World::play_time_seconds`.
pub const PLAY_TIME_VA: u32 = 0x8008_4570;
/// Reseed period in **play-time ticks** (the unit of `_DAT_80084570`). HW-pinned:
/// the counter advances ~per-frame (≈60/s), NOT per-second as the memory-map label
/// suggests - a maxed save read `0x80084570 ≈ 10.4M`, which is ~48 h at 60/s, not
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
/// - draws the per-line `LVL` value. SCUS-resident; 4 register args.
pub const NUMBER_FN: u32 = 0x8003_4B78;
/// "SERU TRADE" title string for the handler. Relocated to the upper row-4 gap tail
/// (past `@Trade` at [`TRADE_STR_VA`], below the config blob) to free the
/// `0x8007ABB0..0x8007AD00` window for the grown handler body.
pub const TITLE_STR_VA: u32 = 0x801E_7D30;
/// Title bytes ('@' format prefix like the menu strings + "SERU TRADE\0").
pub const TITLE_STR: &[u8] = b"@SERU TRADE\0";

/// Cursor-state word `DAT_801e46bc` (low 12 bits = index; 0x2000 = ○; 0x4000 = ✕).
pub const CURSOR_VA: u32 = 0x801E_46BC;
/// Cursor-highlight sprite `FUN_8002b994(slot, mode, x, y)`.
pub const HIGHLIGHT_FN: u32 = 0x8002_B994;
/// 0899 run-C VA of the row-4 draw stub (the in-shop trade build hosts everything in
/// 0899; `<= 0x80` bytes reserved for code before the label). Reached by `j` from the
/// renderer's row-4 detour.
pub const ROW4_STUB_VA: u32 = 0x801E_7C20;
/// 0899 run-C VA of the "@Trade" label (just past the row-4 stub's reserved window).
pub const TRADE_STR_VA: u32 = 0x801E_7D20;
/// The label bytes: '@' format prefix (as on "@Buy"/"@Sell"/"@Quit") + "Trade\0".
pub const TRADE_STR: &[u8] = b"@Trade\0";
