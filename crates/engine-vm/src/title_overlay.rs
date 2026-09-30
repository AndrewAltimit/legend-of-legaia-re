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
//! - `0x02` `TextMenu` - a two-row menu whose rows are the strings at
//!   `0x801CEF0C` and `0x801CEF14` (`lui a0,0x801d; addiu a0,a0,-0x10f4`
//!   / `-0x10ec` at `0x801DDE58` / `0x801DDE80`) - a save row and a load
//!   row, not NEW GAME / CONTINUE - drawn by `FUN_80036888` as two text
//!   lines at y `0x6B` / `0x78` with a cursor sprite (`FUN_8002C488`) and
//!   a `FUN_801E4140` panel, confirming on `pad & 0x44` into `0x14` with
//!   the picked row in `state[+0x200]`. Unreachable on retail for the two
//!   reasons above.
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

mod menu;
mod state_layout;
mod submode;
mod tick;
mod tick_types;
mod transitions;

pub use menu::*;
pub use state_layout::*;
pub use submode::*;
pub use tick::*;
pub use tick_types::*;
pub use transitions::*;

#[cfg(test)]
mod tests;
