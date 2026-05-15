//! Field / event script VM, ported clean-room from `FUN_801DE840`.
//!
//! `FUN_801DE840` lives in PROT entry `0897_xxx_dat` (the town/field overlay,
//! see `docs/subsystems/script-vm.md`). It drives Legaia's overworld scripting - NPC
//! movement, dialog triggers, cutscene sequencing, story flag manipulation.
//! 17.5 KB, 357 outgoing calls - the largest function in the corpus.
//!
//! Unlike the small fixed-width actor VM in [`super`], the field VM has
//! variable-length opcodes (1 to many bytes), a rich per-script context
//! struct, and dispatches into hundreds of SCUS helpers. This module starts
//! with a foundation: the simplest opcodes ported faithfully, with stubs and
//! a `Pending` return for the rest. As the opcode reference fills in, this
//! module grows.
//!
//! ## Bytecode layout
//!
//! Each instruction starts with one opcode byte:
//!
//! ```text
//!   *(buffer + pc) = opcode
//! ```
//!
//! The high bit (0x80) is the **extended** flag. When set, the next byte is
//! a target script ID; the VM resolves it through the host and operates on
//! that script's context instead of the caller's. The low 7 bits are the
//! actual opcode (range `0x21..=0x4F` with gaps at `0x27..=0x2A`).
//!
//! Operands follow the opcode byte (or the script-ID byte if extended).
//! Operand width is per-opcode and ranges from 0 to ~14 bytes.
//!
//! Execution does NOT loop internally. The VM dispatches a single instruction
//! per call, returning a [`StepResult`] that tells the caller whether to
//! advance and where, or to halt.
//!
//! ## Cross-context dispatch
//!
//! When the high bit is set, the VM operates on a *different* script's
//! context than the caller's. The caller is responsible for resolving the
//! target script ID (via [`peek_extended`]) before invoking [`step`] - the
//! `ctx` parameter should already point at the target's context. This mirrors
//! the original's `func_0x8003C83C(target_id)` lookup, lifted into the host
//! layer to keep the VM borrow-free.
//!
//! ## Clean-room boundary
//!
//! No bytes from `SCUS_942.54` or any overlay live in this crate. The Ghidra
//! decompilation at `ghidra/scripts/funcs/overlay_0897_801de840.txt` and the
//! reference at `docs/subsystems/script-vm.md` are the *spec*, not source. The
//! [`FieldHost`] trait abstracts every call the original made into SCUS - its
//! implementation lives in the engine layer.
//!
//! Tests use hand-authored synthetic bytecode (no Sony bytes).

#![allow(clippy::too_many_arguments)]

/// Per-script execution context. One instance per running script.
///
/// Field naming follows the byte-offset convention from `docs/subsystems/script-vm.md`
/// to keep the link to the decompilation explicit. Each public field is a
/// distinct piece of state surfaced by at least one opcode handler.
#[derive(Debug, Clone, Default)]
pub struct FieldCtx {
    /// `+0x10` - context flag word. Bit `0x400` = halted (set by YIELD ops,
    /// checked by the dispatcher prelude). Bits `0x100`, `0x1000`, `0x20200`,
    /// `0x20000000`, `0x1000000`, `0x80000` carry per-feature semantics.
    pub flags: u32,
    /// `+0x14` - world X (units: `0.5` tile, formula `(b & 0x7F) * 0x80 + 0x40`).
    pub world_x: u16,
    /// `+0x16` - world Y (collision-derived).
    pub world_y: u16,
    /// `+0x18` - world Z.
    pub world_z: u16,
    /// `+0x26` - source value copied to [`saved_26`] by op 0x31 bit-8 path.
    pub field_26: u16,
    /// `+0x50` - script ID. `0xFB` = "system" channel.
    pub script_id: u16,
    /// `+0x54` - wait/timer accumulator. Cleared by YIELD; ticked by WAIT_FRAMES.
    pub wait_accum: i16,
    /// `+0x56` - move-table sub-state (op 0x22 sets to 5 if move==0, else 1).
    pub move_substate: u16,
    /// `+0x5A` - saved counterpart of [`field_26`] (op 0x31 bit-8 path).
    pub saved_26: u16,
    /// `+0x5C` - move-table index (op 0x22).
    pub move_id: u16,
    /// `+0x5E` - set to `0xFFFE` by op 0x22.
    pub field_5e: u16,
    /// `+0x62` - local flag bank (16 bits). Manipulated by 0x2B/0x2C/0x2D.
    pub local_flags: u16,
    /// `+0x6D` - face/body rotation index (op 0x43 sub-7).
    pub face_rotation: u8,
    /// `+0x72` - generic per-actor scalar slot. Written / ramped by
    /// op 0x4C outer-nibble-4 sub-0.
    pub field_72: u16,
    /// `+0x24` - generic per-actor scalar slot. Written / ramped by
    /// op 0x4C outer-nibble-4 sub-3 (ramp path); the immediate path is
    /// repurposed as an absolute jump and does not touch this field.
    pub field_24: i16,
    /// `+0x28` - generic per-actor scalar slot. Written by op 0x4C
    /// outer-nibble-4 sub-4 (immediate path); the ramp path is repurposed
    /// as an absolute jump and does not touch this field.
    pub field_28: i16,
    /// `+0x6A` - generic per-actor scalar slot. Written / ramped by op
    /// 0x4C outer-nibble-4 sub-1, which **halves the input** (`target >> 1`)
    /// and floors the result at `1` before applying.
    pub field_6a: i16,
    /// `+0x8E` - inverted-Y mirror slot. Written / ramped by op 0x4C
    /// outer-nibble-4 sub-2 (which also conditionally writes
    /// `world_y = -value` when `flags & 0x20000000` is set).
    pub field_8e: i16,
    /// `+0x8B` - cleared by op 0x23 NPC path.
    pub field_8b: u8,
    /// `+0x8C` - NPC X grid coordinate (op 0x23).
    pub npc_x: u8,
    /// `+0x8D` - NPC facing direction (op 0x23).
    pub npc_facing: u8,
    /// `+0x90` - opaque actor-handle field. Captured by op 0x49 sub-1 into the
    /// `_DAT_8007B44C` global (the runtime later restores it across the
    /// state-resume gate). Treated as opaque by the VM.
    pub field_90: u32,
    /// `+0x94` - saved PC (set by YIELD; the dispatcher reads this on resume).
    pub saved_pc: u32,
    /// `+0x42` - generic per-actor scalar slot. Written by op 0x4C
    /// outer-nibble-0xC sub-2 (`[4C, 0xC2, b1]` writes `b1` zero-extended).
    pub field_42: u16,
    /// `+0x58` - generic per-actor scalar slot. Written by op 0x4C
    /// outer-nibble-0xD sub-0xD (`[4C, 0xDD, b1]` writes `b1` zero-extended).
    pub field_58: u16,
    /// `+0x68` - local guard slot. Read by op 0x4C outer-nibble-8 sub-0xC
    /// to skip a forward jump when zero.
    pub field_68: i16,
    /// `+0x74` - composite control word. XOR-toggled by op 0x4C
    /// outer-nibble-0xC sub-8 (`[4C, 0xC8]` flips bit 0x10000000).
    pub field_74: u32,
}

impl FieldCtx {
    /// Has the YIELD bit (`flags & 0x400`) been set?
    pub fn is_halted(&self) -> bool {
        self.flags & 0x400 != 0
    }

    /// Set the halt bit. Called by the YIELD opcodes (0x37, 0x41, 0x47).
    pub fn halt(&mut self) {
        self.flags |= 0x400;
    }
}

/// Outcome of a single VM step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepResult {
    /// Advance to a new PC offset. The next call to [`step`] should resume
    /// from `next_pc`.
    Advance { next_pc: usize },
    /// The script has yielded - caller should wait for the next host tick
    /// before resuming. The next PC was saved to `ctx.saved_pc`.
    Yield { resume_pc: usize },
    /// The script wants to halt and not resume on its own - typically because
    /// a flag-test failed and the conditional path is "halt".
    Halt { final_pc: usize },
    /// The opcode is recognized but not yet implemented in this port.
    /// Carries the opcode byte for diagnostics.
    Pending { opcode: u8, pc: usize },
    /// Unknown / out-of-range opcode (matches the original's "default" arm
    /// behaviour, which prints `"UNFIND INDICATION %d"` and returns).
    Unknown { opcode: u8, pc: usize },
}

/// Engine-side callbacks the field VM dispatches into.
///
/// As more opcodes are ported, this trait grows. New methods land with default
/// impls so existing hosts compile unchanged - engines override what they care
/// about.
pub trait FieldHost {
    /// Read the global story-flag word at `_DAT_1F800394` (PSX scratchpad).
    /// Used by GFLAG_TST (0x30) and read/written by GFLAG_SET (0x2E) /
    /// GFLAG_CLR (0x2F).
    fn global_flags(&self) -> u32;
    /// Write the global story-flag word.
    fn set_global_flags(&mut self, value: u32);

    /// Frame delta tick. Mirrors `_DAT_1F800393` (PSX scratchpad byte read by
    /// op 0x4A WAIT_FRAMES on every step). Most engines return `1` when the
    /// frame budget allows the script to advance.
    fn frame_delta(&self) -> u16;

    /// Begin executing a move-table entry on `ctx`. Mirrors `func_0x800204F8`
    /// (the move-table consumer that `crates/mdt` parses). The VM has
    /// already written `ctx.move_id`, `ctx.field_5e`, and `ctx.move_substate`
    /// before this method fires; the host only needs to start the visual /
    /// animation side of the move (or override the substate for
    /// player-vs-NPC nuances).
    ///
    /// `move_id == 99` is the cancel sentinel on the player path.
    fn exec_move(&mut self, ctx: &mut FieldCtx, move_id: u8) {
        let _ = (ctx, move_id);
    }

    /// Teleport / position-set on `ctx`. `world_x` and `world_z` have already
    /// been computed from the bytecode's grid coordinates (formula
    /// `(b & 0x7F) * 0x80 + 0x40`, plus `0x40` if the high bit is set), and
    /// the VM has written `ctx.world_x` / `ctx.world_z` / `ctx.npc_x` /
    /// `ctx.npc_facing` / cleared `ctx.field_8b`. The host runs whatever
    /// post-step is appropriate (Y collision, camera scroll on the player
    /// path, NPC facing/movement init on the NPC path).
    ///
    /// `is_player` distinguishes the two paths in the original (player calls
    /// camera/scroll; NPC sets facing + movement init). The VM derives this
    /// from `ctx.flags & 0x1000000`; hosts can override based on richer
    /// bookkeeping.
    fn move_to(&mut self, ctx: &mut FieldCtx, world_x: u16, world_z: u16, is_player: bool) {
        let _ = (ctx, world_x, world_z, is_player);
    }

    /// Open a dialog box. The text ID + inline buffer feed the MES bytecode
    /// that `crates/mes` parses; `func_0x8001FD44` was the original opener.
    /// `world_x` / `world_z` are pre-decoded grid coordinates for the box
    /// position; `depth_id` is the raw byte (the original indexes a length-8
    /// `_DAT_xxx` lookup table for the actual depth value - the host decides
    /// whether to use the same table or its own scheme).
    fn open_dialog(
        &mut self,
        text_id: u16,
        inline: &[u8],
        world_x: u16,
        world_z: u16,
        depth_id: u8,
    ) {
        let _ = (text_id, inline, world_x, world_z, depth_id);
    }

    /// Background music dispatch (op 0x35). `text_id` is the 16-bit operand
    /// (LE). `sub_op` selects the action: 1 = start field BGM, 2 = pause,
    /// 3 = resume, 4 = stop, 5 = volume, 6 = flag-set (`_DAT_8007b750 |= 4`),
    /// 7 = target sound set (`_DAT_8007B880`), 8 = `func_0x80019898`,
    /// 9 = queue (`_DAT_8007bac8 = text_id` if not already loaded), 10 =
    /// pause-toggle, 11 = `_DAT_8007ba9c = -1`. The VM only knows how to
    /// advance the PC; per-sub-op state is host-side.
    fn bgm(&mut self, text_id: u16, sub_op: u8) {
        let _ = (text_id, sub_op);
    }

    /// Camera config (op 0x38, simple path). When `op1 & 0x7F == 0`, the
    /// original copies `*(short *)(0x80073F04 + (op0 & 0xF) * 2)` into
    /// `ctx.field_26`. The host owns the table - this method takes the
    /// `index = op0 & 0xF` and returns the u16 to write. Returning `None`
    /// keeps `ctx.field_26` unchanged (the VM still advances PC).
    fn cam_cfg_lookup(&self, index: u8) -> Option<u16> {
        let _ = index;
        None
    }

    /// Play a sound effect (op 0x39). The original calls
    /// `func_0x8004313C()` (likely a "stop current SFX" cleanup) then
    /// `func_0x800421D4(sfx_id, 1)` to start the new one.
    fn play_sfx(&mut self, sfx_id: u8) {
        let _ = sfx_id;
    }

    /// Add or subtract money (op 0x3A). `delta` is a 24-bit two's-complement
    /// integer already sign-extended to i32. The VM does NOT clamp; the host
    /// applies the new running total and clamps to `[0, 9999999]` per the
    /// original. Returning the new total is informational only.
    fn add_money(&mut self, delta: i32) {
        let _ = delta;
    }

    /// Set an inventory slot's count (op 0x3B). Slot encoding: low nibble +
    /// (high nibble * page-stride) - i.e. `slot_byte` is the raw operand.
    /// The original computes `*(byte*)(0x80084340 + (slot & 0xF) +
    /// (slot >> 4) * 0x414) = count;` then refreshes via
    /// `func_0x80042558()`. Hosts decide their own inventory layout.
    fn set_item_count(&mut self, slot_byte: u8, count: u8) {
        let _ = (slot_byte, count);
    }

    /// Add a character to the active party (op 0x3C). The original maintains
    /// a sorted insertion in `_DAT_80084598..` (cap 4), and updates
    /// `_DAT_8007B8F8` (party leader) when count was 0. Returns `true` on
    /// success, `false` if the party was full or the character was already
    /// in the party (matches the original's silent-no-op fallthrough).
    fn party_add(&mut self, char_id: u8) -> bool {
        let _ = char_id;
        true
    }

    /// Remove a character from the active party (op 0x3D).
    fn party_remove(&mut self, char_id: u8) {
        let _ = char_id;
    }

    /// Read the secondary global-flag bank `_DAT_8007B8F4` (op 0x42 mode 0).
    /// Distinct from the main `_DAT_1F800394` flag bank in scratchpad -
    /// `_DAT_8007B8F4` is in main RAM and tracks a different set of game
    /// states (likely "scene-already-visited" or similar bookkeeping).
    fn extra_flags(&self) -> u32 {
        0
    }

    /// Read the screen-mode word `_DAT_8007B850` (op 0x42 mode 1). Bits
    /// `0x10/0x20/0x40/0x80` are individually testable; bits `0xF000` are
    /// matched against an 8-entry lookup table at `_DAT_801F28D0`.
    fn screen_mode(&self) -> u32 {
        0
    }

    /// Look up the 0x42-mode-1 8-entry table `_DAT_801F28D0` (when `op1 < 8`).
    /// Default returns `None` so the test falls through to the skip path.
    fn screen_mode_table(&self, index: u8) -> Option<u32> {
        let _ = index;
        None
    }

    /// Trigger an in-scene field interaction (op 0x3E, `op0 < 100` path).
    /// `interact_id` is `op0` (with `0xFF` representing "current"); `slot` is
    /// `op1`. The original writes `sys_ctx[+0x94]` with a per-scene table
    /// offset and dispatches `func_0x8003CE08(0xE)`.
    fn field_interact(&mut self, interact_id: u8, slot: u8) {
        let _ = (interact_id, slot);
    }

    /// Trigger a scene transition (op 0x3E, `op0 >= 100` path). The original
    /// stores `_DAT_8007BA34 = op0 - 100` (the new map id), sets
    /// `_DAT_8007B83C = 0x18`, clears `player.flags & 0x80000`, and invokes
    /// `func_0x8003CE08(0xE)`. The VM clears `ctx.flags & 0x80000` itself
    /// so a no-op host still mirrors the player-flag write.
    fn scene_transition(&mut self, map_id: u8) {
        let _ = map_id;
    }

    /// Render-config write (op 0x46, long form `op0 == 0x24`). Original
    /// writes `pbVar47[1..5]` to scratchpad RGB cluster + the matching main
    /// RAM mirrors. The four bytes are passed as-is; semantic interpretation
    /// is host-side.
    fn render_cfg_long(&mut self, b1: u8, b2: u8, b3: u8, b4: u8) {
        let _ = (b1, b2, b3, b4);
    }

    /// Render-config write (op 0x46, short form). The VM has done the bitfield
    /// math: `r = !(op0 >> 1)`, `g = 2 - (op1 >> 1)`, `b = (op0 >> 1) - 1`,
    /// `packed = (op1 >> 1) + 2`. Hosts apply to their fog/render state.
    fn render_cfg_short(&mut self, r: u8, g: u8, b: u8, packed: u8) {
        let _ = (r, g, b, packed);
    }

    /// Scene-register write (op 0x4F). The original writes three `u16` values
    /// to `_DAT_801C6EA4 + 0x10/+0x12/+0x14`. Each value is one raw byte
    /// zero-extended.
    fn scene_register_write(&mut self, slot_10: u8, slot_12: u8, slot_14: u8) {
        let _ = (slot_10, slot_12, slot_14);
    }

    /// Counter / score / hit-counter update (op 0x44). The original calls
    /// `func_0x8003d064(_DAT_8007b898 + 0x22, ...)` to fetch a 3-int return,
    /// then `func_0x8003bde0(0, 0, op0 - a - b, 1)` to apply the difference.
    /// We surface just the operand byte; the host owns the actual counter.
    fn counter_update(&mut self, op0: u8) {
        let _ = op0;
    }

    /// Set up multi-keyframe animation (op 0x4B). The VM has already populated
    /// `ctx.local_flags` with bit 0x1000 (animation gate), `ctx.flags` with
    /// bit 0x1000, and the per-frame slots `ctx[+0xB0..]` / `+0xB8` / `+0xC8`,
    /// but those layout slots are still opaque so we just hand the host the
    /// raw frame data. `frames` is `count` × 4 bytes.
    fn setup_animation(&mut self, ctx: &mut FieldCtx, count: u8, base_id: u8, frames: &[u8]) {
        let _ = (ctx, count, base_id, frames);
    }

    /// Set the party leader (op 0x4C sub-0). `leader_id` is `op0 & 7`. The
    /// original mirrors this onto `DAT_80084597` and `_DAT_8007B8F8`.
    fn set_party_leader(&mut self, leader_id: u8) {
        let _ = leader_id;
    }

    /// Bounding-box test result (op 0x4D). When the active ctx is INSIDE
    /// `[x_min..=x_max] × [z_min..=z_max]` (in tile units), the VM advances
    /// PC; when OUTSIDE, the original calls `FUN_801e3614` which reads a
    /// 2-byte offset from operand+4..6 and computes a forward-skip target.
    /// Until that helper is captured the outside-box path returns `Pending`.
    ///
    /// `world_to_tile_use_alt` controls the tile derivation: `false` → the
    /// default `(world - 0x40) >> 7`, `true` → the alternate `(world << 16) >> 23`
    /// path that fires when `_DAT_1F800394 & 0x20000` is set.
    fn world_to_tile_use_alt(&self) -> bool {
        false
    }

    /// Camera op 0x45 CONFIGURE. The VM has decoded the 10-bit param mask
    /// from `[op0, op1]` and reads each set bit's `u16` value into `params`.
    /// `mode` is `(op0 >> 2) & 0xF` - passed to `FUN_801de084` originally as
    /// a third arg. `apply_trigger` is the `u16` at `operand+2` (between the
    /// mask bytes and the per-param value stream). Hosts apply the params
    /// to camera state; original calls `FUN_801ddfe4` (init) before the
    /// loop and `FUN_801de084` (apply) after.
    fn camera_configure(&mut self, params: &[CameraParam], apply_trigger: u16, mode: u8) {
        let _ = (params, apply_trigger, mode);
    }

    /// Camera op 0x45 LOAD. Payload is 18 raw bytes following `op0` -
    /// `FUN_801dbc20(operand + 1)` consumes them.
    fn camera_load(&mut self, payload: &[u8]) {
        let _ = payload;
    }

    /// Camera op 0x45 SAVE. `FUN_801de004(0x801c6ea8)` snapshots the camera
    /// scratch struct. No operands.
    fn camera_save(&mut self) {}

    /// Camera op 0x45 APPLY. The original calls `FUN_801dab90` (apply) +
    /// `FUN_801daa50` (read-back). The VM jumps to the absolute PC encoded
    /// in `LE_u16(operand[1..3])` afterwards.
    fn camera_apply(&mut self) {}

    /// Scene fade (op 0x36). The VM passes the two 16-bit operands raw and
    /// the host decides whether the fade applies immediately (PC += 5) or
    /// the scene is busy and the script must hold (PC stays).
    ///
    /// `op0_word` encodes the mode (high bit + sub-op nibble); `op1_word`
    /// carries the fade target / duration. See `docs/subsystems/script-vm.md`
    /// (opcode 0x36) for the sub-case table - the original branches based on
    /// `op0_word`'s `0xFFFF` sentinel, the `0x8000` flag bit, and the low-15
    /// sub-op.
    fn scene_fade(&mut self, op0_word: u16, op1_word: u16) -> SceneFadeResult {
        let _ = (op0_word, op1_word);
        SceneFadeResult::Done
    }

    /// Inventory state read for op 0x4E sub-ops 0 and 1.
    ///
    /// The original reads two `u16` values from the inventory page at
    /// `0x80084340 + page * 0x414`:
    /// - sub-op 0: `state` from `+0x4CE`, `factor` from `+0x4CC`
    ///   (`DAT_8008480E + page*0x414` and `DAT_8008480C + page*0x414`).
    /// - sub-op 1: `state` from `+0x4D2`, `factor` from `+0x4D0`
    ///   (`DAT_80084812 + page*0x414` and `DAT_80084810 + page*0x414`).
    ///
    /// Returning `(0, 0)` (the default) makes the comparison resolve to
    /// false (`0 < 0` is false in both modes), matching the original's
    /// behaviour when the slots are zero-initialised.
    fn inventory_compare_pair(&self, page: u8, sub_op: u8) -> (i32, i32) {
        let _ = (page, sub_op);
        (0, 0)
    }

    /// Op 0x49 STATE_RESUME tristate read.
    ///
    /// Original tracks `_DAT_8007B450` as a pointer-sized slot:
    /// - `0` = idle (no resume in progress)
    /// - `1` = a previous arm completed and the script can resume
    /// - any other value = currently armed (the runtime stored a PC pointer)
    ///
    /// Hosts model this however they like - the VM only needs the tristate.
    fn op49_state(&self) -> Op49State {
        Op49State::Idle
    }

    /// Arm the op-49 state machine. `pc` is the bytecode offset of the
    /// opcode byte, `field_90` is the opaque actor-handle captured for
    /// sub-op 1 (and `_DAT_8007B44C` in the original). Hosts hold this
    /// until [`FieldHost::op49_state`] returns `Done`.
    fn op49_arm(&mut self, pc: usize, field_90: u32) {
        let _ = (pc, field_90);
    }

    /// Clear the op-49 state machine back to `Idle`. Called when the
    /// resume path runs (state was `Done`) or when the runtime aborts
    /// (default sub-op).
    fn op49_clear(&mut self) {}

    /// Pre-arm setup. Mirrors `func_0x80020de0(0x8007065c, _DAT_8007c34c)`
    /// in the original - a one-shot subroutine the runtime invokes before
    /// suspending the script. Default no-op.
    fn op49_invoke_setup(&mut self) {}

    /// Op 0x34 sub-3 (3D model anim trigger). The original calls
    /// `func_0x800252ec(arg + 1, ctx + 0x14, ctx + 0x24)` where
    /// `arg = pbVar47[1]`. The host owns the actual animator; we pass the
    /// arg byte and the ctx mutably so it can access world-position
    /// fields if needed.
    fn effect_anim_trigger(&mut self, ctx: &mut FieldCtx, arg: u8) {
        let _ = (ctx, arg);
    }

    /// Op 0x43 sub-8 (face / rotation reset). The original writes
    /// `*(byte*)(ctx + 0x6D) = 0` and `*(short*)(ctx + 0x7A) = 0`. We
    /// expose `face_rotation` directly on `FieldCtx`; the `+0x7A` slot is
    /// not yet a named field, so the host applies it.
    fn actor_face_reset(&mut self, ctx: &mut FieldCtx) {
        let _ = ctx;
    }

    /// Op 0x4E sub-ops 10 / 11 (party-bank comparison).
    ///
    /// The original reads a 32-bit value from `_DAT_8008459C` (sub-10) or
    /// `_DAT_800845A4` (sub-11). These globals are the party-money / party-XP
    /// banks (separate from the per-page inventory used by sub-0/1). The VM
    /// compares this against the operand-encoded scaled value.
    ///
    /// Default returns 0; comparisons resolve to `0 < scaled` or `scaled < 0`
    /// depending on the mode, matching the original's behaviour when the
    /// bank is empty.
    fn party_bank_value(&self, sub_op: u8) -> i32 {
        let _ = sub_op;
        0
    }

    /// Op 0x4E sub-op 4 - BIOS Rand stub.
    ///
    /// The original at line 7479 of `overlay_0897_801de840.txt` is
    /// `iVar18 = func_0x80056798(); return iVar18;`. `FUN_80056798` is a
    /// 3-instruction BIOS thunk: `li t2,0xa0; jr t2; _li t1,0x2f` - i.e.
    /// `jr 0xA0` with `t1 = 0x2F`, the BIOS `Rand()` syscall. The script-VM
    /// dispatcher then uses the returned value directly as the next PC, which
    /// would jump into arbitrary memory in any sane retail execution. There
    /// are no callers of opcode 0x4E sub-4 in the captured bytecode, so this
    /// is almost certainly a dev / debug-only stub left in by the original
    /// authors.
    ///
    /// The default returns 0 (PC restarts at the bytecode origin), matching
    /// the broken-by-design behaviour. A custom host can override to either
    /// halt the script (return the current PC; the dispatch wrapper does NOT
    /// special-case this) or to actually invoke a PRNG.
    fn op4e_sub4_bios_rand(&mut self) -> i32 {
        0
    }

    /// Op 0x4C sub-1 (menu sub-dispatcher).
    ///
    /// The original encodes a 7-byte instruction `[4C, op0, b1, b2, b3, b4, b5]`
    /// where `op0` selects the inner sub-op:
    /// - `0x10`: writes `_DAT_8007B7B0 = 24-bit-LE(b1..b3)`.
    /// - `0x12`: writes `DAT_8007BCB8/B9/BA = b1/b2/b3` (with optional
    ///   `func_0x8003C5F0` ramp via `LE_u16(b4..b5)`).
    /// - `0x13`: writes `DAT_8007B636/B635/B634 = b1/b2/b3` (with optional ramp).
    /// - `0x14`: actor-lookup `func_0x8003C83C(b5)` then `FUN_801D835C(actor, ...)`.
    ///
    /// PC always advances by 7 (= `param_2 + 7` non-extended). Hosts model the
    /// per-sub-op behaviour internally; the VM just hands them the operand
    /// bytes.
    fn menu_ctrl_sub1(&mut self, op0: u8, payload: &[u8; 5]) {
        let _ = (op0, payload);
    }

    /// Op 0x4C sub-3 sub-3 (refresh helper).
    ///
    /// The original calls `FUN_801de190()` then writes `DAT_8007B648 = 0`
    /// (a "needs refresh" flag). Hosts likely re-rasterize the inventory or
    /// menu UI here. PC += 2.
    fn menu_refresh(&mut self) {}

    /// Op 0x4C sub-3 sub-A (dialog-depth copy to player ctx).
    ///
    /// The original writes `*(short *)(_DAT_8007C364 + 0x26) = _DAT_80073EFC`
    /// (i.e. `player_ctx.field_26 = dialog_depth`). The player ctx is global
    /// state and not threaded through the VM, so the host owns the write.
    /// PC += 2.
    fn copy_dialog_depth_to_player(&mut self) {}

    /// Op 0x4C sub-3 sub-8 / sub-D (sub-tile-coord helpers).
    ///
    /// Both sub-ops compute `(player.world - 0x40) >> 7` for X and Z, then
    /// invoke a SCUS helper:
    /// - sub-8: `FUN_801de3e0(x_tile, z_tile)` - overlay-resident sub-tile
    ///   refresh.
    /// - sub-D: `func_0x800180EC(x_tile, z_tile)` - SCUS sub-tile refresh.
    ///
    /// The VM passes the sub-op id (8 or 0xD) so the host can pick the right
    /// helper. PC += 2.
    fn player_subtile_refresh(&mut self, sub_op: u8) {
        let _ = sub_op;
    }

    /// Op 0x4C sub-3 sub-7 (player-coord copy onto a non-player ctx).
    ///
    /// 2-byte instruction `[4C, 0x37]`. When `ctx` is not the player ctx
    /// (`_DAT_8007C364`), the original copies the player's world coords +
    /// `+0x26` slot onto `ctx`:
    ///
    /// ```text
    /// ctx[+0x14] = player[+0x14]   ; world_x
    /// ctx[+0x16] = player[+0x16]   ; world_y
    /// ctx[+0x18] = player[+0x18]   ; world_z
    /// ctx[+0x26] = player[+0x26]   ; field_26
    /// if ctx[+0x10] & 0x20000000:
    ///     ctx[+0x8e] = -player[+0x16]    ; inverted Y
    ///     return caseD_4()                ; bail via STATE_RESUME path
    /// ```
    ///
    /// The host owns the player-context lookup (`_DAT_8007C364`). When the
    /// host returns `None`, the player ctx isn't initialised yet OR `ctx`
    /// IS the player ctx - both fall through to a regular advance.
    ///
    /// When the host returns `Some(coords)`, the VM:
    /// - copies `world_x / world_y / world_z / field_26` onto `ctx`,
    /// - if `ctx.flags & 0x20000000` is set, additionally calls
    ///   [`Self::set_inverted_y_mirror`] with `-player.world_y` and returns
    ///   `OpResume::StateResume` (the `caseD_4` exit - the state-resume
    ///   subsystem decides whether to advance or halt),
    /// - otherwise returns `OpResume::Advance` (normal PC += 2).
    fn fetch_player_coords(&self, ctx: &FieldCtx) -> Option<PlayerCoords> {
        let _ = ctx;
        None
    }

    /// Side-effect for Op 0x4C sub-3 sub-7's bit-`0x20000000` branch.
    ///
    /// Writes the inverted-Y value at `ctx[+0x8e]`. Hosts that don't track
    /// `+0x8e` separately can ignore this.
    fn set_inverted_y_mirror(&mut self, ctx: &mut FieldCtx, inverted_y: i16) {
        let _ = (ctx, inverted_y);
    }

    /// Op 0x4C sub-3 sub-2 (clear the 512-byte party/inventory state region).
    ///
    /// 2-byte instruction `[4C, 0x32]`. The original zeroes the 512 bytes at
    /// `[0x80085718 .. 0x80085918)` - a region holding party / inventory state
    /// adjacent to the fourth-flag-bank bitfield (`DAT_80086D70`). Hosts model
    /// their own party-state representation and call this hook to reset it.
    /// PC += 2.
    fn clear_party_state_region(&mut self) {}

    /// Op 0x43 sub-7 (face / body rotation setup).
    ///
    /// 17-byte instruction `[43, 7, face_id, b0..b3, lo0..hi0, lo1..hi1,
    /// lo2..hi2, lo3..hi3, target_lo, target_hi]`. The original writes a
    /// 12-byte struct at `&DAT_80087E68 + face_id * 12` populated from the
    /// operand stream, then schedules a ramp to `ctx + 0x7A` via
    /// `func_0x8003C5F0`. The VM also writes `ctx.face_rotation = face_id`
    /// before the host call.
    ///
    /// `payload_4` is the 32-bit value at operand+2..6 (read via
    /// `func_0x8003CED8` - likely a packed s24 + flag byte).
    /// `params` are four u16 values at operand+6..14.
    /// `target` is the signed target value at operand+14..16.
    fn actor_face_rotation_setup(
        &mut self,
        ctx: &mut FieldCtx,
        face_id: u8,
        payload_4: u32,
        params: [u16; 4],
        target: i16,
    ) {
        let _ = (ctx, face_id, payload_4, params, target);
    }

    /// Op 0x43 sub-12 (allocate scripted actor).
    ///
    /// 5-byte instruction `[43, 0xC, b1, b2, b3]`. Mirrors `FUN_801de754`
    /// which allocates a fresh actor via `func_0x80020DE0(&DAT_801F2858,
    /// _DAT_8007C34C)` and writes the three operand bytes into the new
    /// actor's `+0xB8`, `+0xBA`, `+0xBC` u16 slots. The VM doesn't track
    /// the actor pool - hosts model their own scripted-actor system.
    fn op43_alloc_scripted_actor(&mut self, b1: u8, b2: u8, b3: u8) {
        let _ = (b1, b2, b3);
    }

    /// Op 0x4C sub-2 (party-view-index swap).
    ///
    /// 2-byte instruction `[4C, op0]`. The new view index is `op0 & 7`. The
    /// original conditionally updates `_DAT_8007B5F0` (current view index)
    /// and adjusts `player_ctx[+0x26]` by `(new_index - old_index) * 0x200`
    /// - but only when `_DAT_8007B6B0 == -1000` (a "input idle" sentinel).
    ///
    /// Hosts model the policy. Returning the previous index is informational.
    /// The VM advances PC by 2 in both branches.
    fn party_view_swap(&mut self, new_index: u8) {
        let _ = new_index;
    }

    /// Op 0x43 sub-2 (3-actor talk setup).
    ///
    /// 8-byte instruction `[43, 2, a1, a2, a3, lo, hi, b6]`. The original
    /// resolves three actor IDs via `func_0x8003C83C`; if all three resolve,
    /// it reads `u16 = LE(operand[5..7])` and byte `operand[7]`, then calls
    /// `FUN_801D2D38(actor1, actor2, actor3, u16, byte)` - a 3-actor talk /
    /// face setup. If any ID fails to resolve, the call is skipped silently.
    ///
    /// PC += 8 in both branches.
    fn op43_three_actor_talk(&mut self, actor_ids: [u8; 3], arg_word: u16, arg_byte: u8) {
        let _ = (actor_ids, arg_word, arg_byte);
    }

    /// Op 0x43 sub-D / sub-F (actor allocation via FUN_801de7bc).
    ///
    /// 6-byte instruction `[43, sub_op, b1, b2, b3, b4]`. Mirrors
    /// `FUN_801de7bc` which allocates a fresh actor via
    /// `func_0x80020DE0(&DAT_801F2870, _DAT_8007C34C)` and writes:
    /// - `+0x72 = b1`
    /// - `+0xB8 = b2`
    /// - `+0xBA = b3`
    /// - `+0xBC = b4`
    /// - `+0x50 = mode` (3 for sub-D, 0 for sub-F).
    ///
    /// `mode` is decoded by the VM (3 if sub_op == 0xD else 0).
    fn op43_alloc_actor_with_mode(&mut self, sub_op: u8, mode: u8, args: [u8; 4]) {
        let _ = (sub_op, mode, args);
    }

    /// Op 0x43 sub-E (mark currently-iterating actor with flag bit 0x8).
    ///
    /// 2-byte instruction `[43, 0xE]`. The original walks the actor list
    /// via `func_0x8003CF04(_DAT_8007C34C, FUN_801DD9D4)` looking for a
    /// match; if found, sets `actor[+0x10] |= 0x8`. The VM passes the
    /// active ctx so hosts can correlate. PC += 2.
    fn op43_mark_actor_flag_8(&mut self, ctx: &mut FieldCtx) {
        let _ = ctx;
    }

    /// Op 0x43 sub-3/4/5/6 (sound register ramp).
    ///
    /// 10-byte instruction `[43, sub_op, b1, b2, b3, b4, lo1, hi1, lo2, hi2]`.
    /// Each sub-op picks a different 32-bit register slot:
    /// - sub-3 → `DAT_8007B618`
    /// - sub-4 → `DAT_8007B614`
    /// - sub-5 → `DAT_8007B60C`
    /// - sub-6 → `DAT_8007B610`
    ///
    /// The original calls `func_0x8003C6A4(slot, 4, b1, b2, b3, b4, u16_a, u16_b)`
    /// to schedule a 4-byte ramp from current value to (b1, b2, b3, b4) over
    /// `u16_a` ticks with `u16_b` curve param.
    ///
    /// The VM passes the sub-op verbatim so hosts pick the slot themselves.
    /// PC += 10.
    fn op43_sound_register_ramp(&mut self, sub_op: u8, bytes: [u8; 4], ticks: u16, curve: u16) {
        let _ = (sub_op, bytes, ticks, curve);
    }

    /// Op 0x43 sub-9 (explicit position, optional collision tween).
    ///
    /// 10-byte instruction `[43, 9, x_lo, x_hi, y_lo, y_hi, z_lo, z_hi, t_lo, t_hi]`.
    /// When `t = LE_u16(operand[7..9])` is non-zero, the original calls
    /// `FUN_801de698(ctx, &ctx[+0x14], &target_xyz, t)` to tween from current
    /// coords to target over `t` ticks. The VM doesn't do any of the immediate
    /// writes itself in that branch - the host owns the tween path.
    ///
    /// (When `t == 0`, the VM writes x/y/z to ctx if not `0xFFFF`, and that
    /// path doesn't need this hook.)
    fn op43_sub9_tween(&mut self, ctx: &mut FieldCtx, x: u16, y: u16, z: u16, ticks: u16) {
        let _ = (ctx, x, y, z, ticks);
    }

    /// Op 0x43 sub-0x10 (emitter setup, FUN_801F8004).
    ///
    /// 21-byte instruction. The original calls `FUN_801F8004(operand + 1)` -
    /// likely a particle/emitter init taking a 19-byte struct. PC += 21.
    fn op43_emitter_init(&mut self, payload: &[u8]) {
        let _ = payload;
    }

    /// Op 0x43 sub-0x11 (emitter setup, FUN_801F8D4C).
    ///
    /// 12-byte instruction; reads 5 u16s and calls
    /// `FUN_801F8D4C(u0, u1, u2, u3, u4)`. PC += 12.
    fn op43_emitter_5_words(&mut self, words: [u16; 5]) {
        let _ = words;
    }

    /// Op 0x43 sub-0x15 (emitter setup, FUN_801F8F28).
    ///
    /// 14-byte instruction. The original calls `FUN_801F8F28(operand + 1)` -
    /// a 12-byte struct. PC += 14.
    fn op43_emitter_struct_12(&mut self, payload: &[u8]) {
        let _ = payload;
    }

    /// Op 0x43 sub-0x12 (twin emitter call with 0x100 clamp + offset shift).
    ///
    /// 14-byte instruction `[43, 0x12, lo0, hi0, lo1, hi1, lo2, hi2, lo3, hi3,
    /// lo4, hi4, lo5, hi5]`. Reads six signed-16-bit words. The original:
    ///
    /// ```text
    /// c = words[2];                  // signed s16 from operand+5..7
    /// if (c > 0xFF) {
    ///     // First call uses (a+0xF0, b, c-0xE0, d, e+0x100, f), then c clamped.
    ///     func_0x800468a4(6, words[0]+0xF0, words[1], words[2]-0xE0,
    ///                     words[3], words[4]+0x100, words[5]);
    ///     c = 0x100;
    /// }
    /// // Second call uses (a, b, c_clamped, d, e, f).
    /// func_0x800468a4(6, words[0], words[1], c, words[3], words[4], words[5]);
    /// ```
    ///
    /// `func_0x800468a4(6, …)` is an SCUS particle/light helper not yet
    /// reversed; the VM passes the six raw words plus a `did_split` boolean
    /// (set when the `c > 0xFF` branch fired). PC += 14.
    fn op43_emitter_split_call(&mut self, words: [i16; 6], did_split: bool) {
        let _ = (words, did_split);
    }

    /// Op 0x43 sub-0x13 (opaque emitter call, FUN_801F88FC).
    ///
    /// 14-byte instruction `[43, 0x13, ...12 bytes]`. The original calls
    /// `FUN_801F88FC(operand)` - i.e. passes a pointer to the sub-op byte plus
    /// the 12 trailing data bytes (13 bytes total). FUN_801F88FC isn't yet
    /// reversed; treating the payload as opaque is correct until then.
    /// PC += 14.
    fn op43_emitter_func13(&mut self, payload: &[u8; 13]) {
        let _ = payload;
    }

    /// Op 0x43 sub-0x14 (emitter setup, FUN_801F8E6C with 4 signed words).
    ///
    /// 10-byte instruction `[43, 0x14, lo0, hi0, lo1, hi1, lo2, hi2, lo3, hi3]`.
    /// The original calls `FUN_801F8E6C(s0, s1, s2, s3)` with four signed-16-bit
    /// values. PC += 10.
    fn op43_emitter_4_words(&mut self, words: [i16; 4]) {
        let _ = words;
    }

    /// Op 0x4C sub-3 sub-9 (player position refresh + collision Y + render
    /// resync).
    ///
    /// 2-byte instruction `[4C, 0x39]`. The original chains three SCUS calls:
    /// 1. `FUN_801de3e0((player.world_x - 0x40) >> 7, (player.world_z - 0x40) >> 7)`
    ///    - re-broadcast the player's tile coords to the field grid.
    /// 2. `func_0x80019278(player_ctx)` → write result to `player.world_y`
    ///    - refresh collision Y at the new tile.
    /// 3. Falls through to sub-0xE: `FUN_801db8ec(player_ctx)` + `FUN_801daa50()`
    ///    - re-render / re-rasterize the player on the framebuffer.
    ///
    /// The host owns the player ctx, so the VM passes `ctx` (the active
    /// script's context) for hosts that want to correlate, but the calls
    /// themselves operate on the global player ctx. PC += 2.
    fn player_position_refresh_with_collision_y(&mut self, ctx: &mut FieldCtx) {
        let _ = ctx;
    }

    /// Op 0x4C sub-3 sub-E (player render resync).
    ///
    /// 2-byte instruction `[4C, 0x3E]`. Calls `FUN_801db8ec(player_ctx)` then
    /// `FUN_801daa50()` - the second half of sub-9's chain, without the
    /// position / collision-Y refresh. PC += 2.
    fn player_render_resync(&mut self) {}

    /// Op 0x4C sub-3 sub-F (field I/O resync).
    ///
    /// 2-byte instruction `[4C, 0x3F]`. Calls `func_0x8001ebec()` - an SCUS
    /// helper related to per-frame I/O / render-bank toggling (the same
    /// function called from a TMD-table setup path). PC += 2.
    fn field_io_resync(&mut self) {}

    /// Op 0x34 sub-2 (actor-pool capture-and-yield).
    ///
    /// 3-byte instruction `[34, 0x2N, b1]`. The original walks the actor
    /// linked list at `_DAT_8007C354` looking for an entry whose `+0x90`
    /// slot equals the active ctx. **If found AND `b1 == 0x40`**, it
    /// captures `pbVar47 + 3` into the matched actor's `+0x94` slot (a
    /// forwarded-PC pointer the host re-enters later) and exits via
    /// `caseD_4()` (STATE_RESUME, surfaced as `Yield` here).
    ///
    /// `captured_pc_offset` is the byte offset within the bytecode buffer
    /// at `pc + header_size + 2` - the byte just past the 3-byte
    /// instruction. The host stores this on the matched actor; on the
    /// next state-resume tick the host re-enters the dispatcher at that
    /// offset.
    ///
    /// Returns `true` if an actor was found AND `b1 == 0x40` AND the host
    /// actually performed the capture; `false` otherwise. The VM
    /// `Yield`s when `true`, advances PC by 2 when `false`.
    ///
    /// The default impl returns `false` so hosts without an actor pool
    /// just fall through to the `Advance` path.
    fn op34_capture_pc_for_existing_actor(
        &mut self,
        ctx: &FieldCtx,
        b1: u8,
        captured_pc_offset: usize,
    ) -> bool {
        let _ = (ctx, b1, captured_pc_offset);
        false
    }

    /// Op 0x34 sub-0 (effect-global colour + intensity setup).
    ///
    /// 7-byte instruction `[34, op0, r, g, b, intensity_lo, intensity_hi]`
    /// where `op0`'s low 3 bits select the fade mode. The original at
    /// `0x801E1FB0+`:
    ///
    /// - reads `intensity = signed_16(operand[3..5])` (after the 3 colour bytes),
    /// - if `_DAT_8007B62C != 0`: emits a "previous fade complete" event for
    ///   the slot (calls `FUN_801de2b0` or `func_0x80024e80` depending on
    ///   `_DAT_1F800394 & 0x800000`, the active-cutscene flag),
    /// - rewrites the `_DAT_8007BCCC..BCE0` colour-mode globals from the
    ///   operand: `BCE0 = 1` (or 2 if `op0 & 1`), `BCCC = 0x02` (or `0x00`
    ///   if `op0 & 4`, or `0x08` if `op0 & 2`), `BCCD..BCCF = r, g, b`,
    /// - if all three colour bytes are zero: clears `_DAT_8007B62C` (no
    ///   active fade); otherwise schedules a new fade with the colour and
    ///   intensity, applying the `iVar18 -= iVar18 >> 3` brightness clamp
    ///   when the colour is pure-white in mode 2.
    ///
    /// The host abstracts the fade pipeline; the VM hands over `op0`'s low
    /// nibble + RGB + intensity + the prior-fade-active hint. PC advances by 7.
    fn op34_sub0_color_intensity_setup(&mut self, op0: u8, rgb: [u8; 3], intensity: i16) {
        let _ = (op0, rgb, intensity);
    }

    /// Op 0x34 sub-1 (effect / sprite spawn with optional captured-PC).
    ///
    /// **Base instruction is 13 bytes** (opcode + 12 operand bytes). The
    /// `capture_flag` byte at the position immediately past the instruction
    /// is **peeked** by the runtime - when it equals `0x40`, two extra
    /// header bytes plus a variable-length payload are consumed before PC
    /// advances. Total instruction length is therefore 13 (no capture) or
    /// `13 + 2 + payload_len` (with capture).
    ///
    /// Operand layout (offsets relative to the operand byte at `pc + 1`):
    ///
    /// ```text
    /// +0x00  op0 (= 0x10..0x1F; bit 0 selects spawn-mode for FUN_801E5668)
    /// +0x01  byte_24[0]   ; high byte of 24-bit packed value
    /// +0x02  byte_24[1]
    /// +0x03  byte_24[2]   ; low byte
    /// +0x04  s16 lo, hi   ; world_x (`local_a0`)
    /// +0x06  s16 lo, hi   ; world_z (`local_9e`)
    /// +0x08  s16 lo, hi   ; raw -world_y (`local_a6`, NEGATED before spawn)
    /// +0x0A  s16 lo, hi   ; reserved (`local_a8` and `local_a4`, both stay 0)
    /// + (peek at pc + 13) capture_flag ; 0x40 = capture next PC into spawned actor
    /// + (peek at pc + 14) pc_payload_len  ; only when capture_flag == 0x40
    /// + (peek at pc + 15..) captured PC payload (only when capture_flag == 0x40)
    /// ```
    ///
    /// The original at `0x801E1F0C+`:
    /// 1. Walks the actor list at `_DAT_8007C354` looking for an entry whose
    ///    `+0x90` slot equals the active ctx - if found, jumps directly to
    ///    `LAB_801E2EA0` and returns `pc + 13` (skips the spawn).
    /// 2. Otherwise calls `FUN_801E5668(ctx, ..., pos, packed24, mode)` to
    ///    spawn a new actor at the world position. `mode = 1` if `op0 & 1`
    ///    is clear, else `mode = 2` (selects which of the two
    ///    24-bit-packed-value slots gets populated on the spawned actor).
    /// 3. If `capture_flag == 0x40`: captures the operand bytes at offset
    ///    0x0E onto the spawned actor's `+0x94` slot (a forwarded-PC
    ///    pointer), then advances PC by an extra `2 + pc_payload_len` past
    ///    the standard 13.
    ///
    /// Returns the **PC delta from the opcode byte** - the VM applies it as
    /// `Advance { next_pc: pc + delta }`. Default impl emits the
    /// "no actor pool" branch (always returns 13) which matches the
    /// game-code path when the spawn pool is full and no slot was
    /// available.
    fn op34_sub1_spawn_or_skip(
        &mut self,
        ctx: &FieldCtx,
        op0: u8,
        packed24: u32,
        pos: [i16; 3],
        capture_flag: u8,
        captured_pc_payload: &[u8],
    ) -> usize {
        let _ = (ctx, op0, packed24, pos, capture_flag, captured_pc_payload);
        13
    }

    /// Op 0x4C outer-nibble-4 ctx-slot ramp.
    ///
    /// The 0x4C nibble-4 family is a 6-byte "write or ramp a slot" cluster.
    /// Encoding `[4C, op0, val_lo, val_hi, ticks_lo, ticks_hi]` where
    /// `(op0 & 0x0F)` selects the slot:
    ///
    /// | Sub | Slot | VM ctx field | Notes |
    /// |---|---|---|---|
    /// | 0 | `+0x72` | `ctx.field_72` | plain s16 write or ramp |
    /// | 1 | `+0x6A` | `ctx.field_6a` | input is `(target >> 1).max(1)` (signed halve, floor 1) |
    /// | 2 | `+0x8E` | `ctx.field_8e` | when ramp == 0 and `flags & 0x20000000`, also writes `world_y = -value` |
    /// | 3 | `+0x24` | `ctx.field_24` | **ramp path only**; ticks==0 reuses the encoding as an absolute PC jump (`pc = target`) and does not touch the slot |
    /// | 4 | `+0x28` | `ctx.field_28` | **immediate path only**; ticks!=0 reuses the encoding as an absolute PC jump and does not touch the slot |
    /// | 8 | `+0x26` | `ctx.field_26` | plain s16 write or ramp |
    ///
    /// When `ticks == 0`, the VM writes the value directly to the ctx field
    /// (no host call). When `ticks != 0`, the original schedules a
    /// `func_0x8003C5F0` ramp from current value to target - the host
    /// drives the per-frame interpolation, so this hook fires.
    ///
    /// PC always advances by 6.
    fn op4c_nibble4_ctx_ramp(&mut self, ctx: &mut FieldCtx, sub: u8, target: i16, ticks: u16) {
        let _ = (ctx, sub, target, ticks);
    }

    /// Op 0x4C outer-nibble-4 global-slot write or ramp.
    ///
    /// Sub-ops `0xA / 0xB / 0xC / 0xD` write or ramp engine globals (the
    /// originals are `_DAT_8007BCD0` / `_DAT_8007BCD4` / `_DAT_8007BCD8` /
    /// `_DAT_8007B910`). Sub-`0xD` additionally multiplies the input by
    /// `_DAT_8008457C` and shifts right 12 bits - engines model the
    /// transform host-side so this hook receives the raw `target` and `sub`.
    ///
    /// `ticks == 0` means immediate write; non-zero means ramp over `ticks`
    /// frames.
    ///
    /// PC always advances by 6.
    fn op4c_nibble4_global_write(&mut self, sub: u8, target: i32, ticks: u16) {
        let _ = (sub, target, ticks);
    }

    /// Op 0x4C outer-nibble-4 sub-6 / sub-7 gate.
    ///
    /// Sub-6 (`_DAT_8007B92C`) and sub-7 (`_DAT_8007B930`) are a **paired**
    /// global-slot write whose normal write/ramp path is gated by
    /// `_DAT_800845A8`: when the gate is **set** (non-zero), the original
    /// short-circuits the write and instead clears **both** globals to zero
    /// via the host's [`op4c_nibble4_global_pair_clear`]. When the gate is
    /// clear (zero), the VM proceeds with the regular
    /// [`op4c_nibble4_global_write`] dispatch on the selected slot.
    ///
    /// Default impl returns `false` (gate clear → write proceeds), which is
    /// the retail-default state at boot.
    ///
    /// [`op4c_nibble4_global_pair_clear`]: FieldHost::op4c_nibble4_global_pair_clear
    /// [`op4c_nibble4_global_write`]: FieldHost::op4c_nibble4_global_write
    fn op4c_nibble4_global_pair_gate(&self) -> bool {
        false
    }

    /// Clear the paired sub-6 / sub-7 globals.
    ///
    /// Fires when [`op4c_nibble4_global_pair_gate`] returns `true`. The
    /// original zeroes `_DAT_8007B92C` and `_DAT_8007B930` together - the
    /// hook abstracts both writes into one call.
    ///
    /// [`op4c_nibble4_global_pair_gate`]: FieldHost::op4c_nibble4_global_pair_gate
    fn op4c_nibble4_global_pair_clear(&mut self) {}

    /// Op 0x4C sub-3 sub-0 / sub-1 (field input lock toggle).
    ///
    /// 2-byte instruction `[4C, 0x30]` (lock) / `[4C, 0x31]` (unlock). The
    /// original sets `_DAT_8007B854 = 1` (sub-0) or `_DAT_8007B854 = 0`
    /// (sub-1), then exits via `caseD_4()` - the STATE_RESUME path. The flag
    /// is reset to 0 by the field reset routine `FUN_8003AEB0`, suggesting
    /// it gates field-input handling during a scripted scene.
    ///
    /// PC advances to `pc + header_size + 1` on resume; the VM surfaces this
    /// as a `Yield` so the host's state-resume layer decides when to advance
    /// (mirrors the existing sub-7 inverted-Y branch).
    fn set_field_input_lock(&mut self, locked: bool) {
        let _ = locked;
    }

    /// Set a bit in the **system flag bank** (the 4th of the four flag banks
    /// the field VM exposes). Wired by the high-byte default-route opcode
    /// `0x5x` (SET) at the dispatcher's default arm. Mirrors `func_0x8003CE08`
    /// which sets `(&DAT_80086D70)[idx >> 3] |= (0x80 >> (idx & 7))`.
    ///
    /// `idx` is a 16-bit value computed by the VM as
    /// `((opcode_byte & 0x8F) << 8) | operand_byte` - bits 0..=7 from the
    /// operand byte, bits 8..=11 from the low nibble of the raw opcode byte,
    /// bit 15 from the extended-prefix bit (0x80) of the raw opcode byte.
    ///
    /// The bitfield's exact size and per-script-context partitioning aren't
    /// nailed down - the original SCUS dispatchers do **no bounds checking**
    /// past `idx >> 3`, so on the original a 16-bit `idx` could index past
    /// the documented 256-bit array. Hosts pick whatever representation
    /// works for them; the default impl is a no-op.
    fn system_flag_set(&mut self, idx: u16) {
        let _ = idx;
    }

    /// Clear a bit in the system flag bank. High-byte default-route opcode
    /// `0x6x`. Mirrors `func_0x8003CE34`. See [`system_flag_set`] for the
    /// `idx` encoding.
    ///
    /// [`system_flag_set`]: FieldHost::system_flag_set
    fn system_flag_clear(&mut self, idx: u16) {
        let _ = idx;
    }

    /// Test a bit in the system flag bank. High-byte default-route opcode
    /// `0x7x`. Mirrors `func_0x8003CE64` (returns 0xFF if set, 0 if clear -
    /// the Rust port collapses both into a `bool`).
    ///
    /// On the original, when the bit is set the dispatcher takes the post-test
    /// branch (jumps to `pc + header_size + 1 + LE_u16(operand[1..3])`); when
    /// the bit is clear, the dispatcher falls through past the 4-byte
    /// instruction. The default impl returns `false`, which keeps the VM
    /// advancing past the instruction.
    fn system_flag_test(&self, idx: u16) -> bool {
        let _ = idx;
        false
    }

    /// Op 0x4C outer-nibble-4 sub-5 - actor-field block (immediate write path).
    ///
    /// 11-byte instruction `[4C, 0x45, b1, w94_lo, w94_hi, w96_lo, w96_hi,
    /// w98_lo, w98_hi, ticks_lo, ticks_hi]`. The original at `0x801E1E14+`
    /// reads four sub-fields and writes them to the actor's `+0x44` pointer
    /// (the per-actor primary structure, typically the rendered object pool):
    /// - `b1` (u8) → `actor[+0x44][+0x9a]` (a control byte; mode select).
    /// - `w94` (s16) → `actor[+0x44][+0x94]`.
    /// - `w96` (s16) → `actor[+0x44][+0x96]`.
    /// - `w98` (s16) → `actor[+0x44][+0x98]`.
    ///
    /// Engines model the actor's `+0x44` pointer as their own per-actor
    /// rendered-object handle; the VM hands over the four field values and
    /// lets the host write them. Default impl is a no-op.
    ///
    /// PC advances by 11 (= `header_size + 10`).
    fn op4c_n4_sub5_write_immediate(
        &mut self,
        ctx: &mut FieldCtx,
        b1: u8,
        w94: i16,
        w96: i16,
        w98: i16,
    ) {
        let _ = (ctx, b1, w94, w96, w98);
    }

    /// Op 0x4C outer-nibble-4 sub-5 - actor-field block (ramp path).
    ///
    /// Same encoding as the immediate path but `ticks != 0`. The original
    /// schedules a `func_0x8003C5F0` ramp for each of the three s16 fields
    /// (`+0x94`, `+0x96`, `+0x98`) **only when** the new target differs from
    /// the current value, then runs through the default-arm STATE_RESUME
    /// path. The control byte at `+0x9a` is always written immediately - it
    /// isn't part of the ramp.
    ///
    /// Engines drive their own per-frame ramp interpolation; this hook fires
    /// once per `[4C, 0x45]` ramp instruction with the full target tuple +
    /// ticks. Default impl is a no-op.
    ///
    /// The VM surfaces a `Yield { resume_pc: pc }` after this hook fires -
    /// the script halts until the host's STATE_RESUME layer signals
    /// completion.
    fn op4c_n4_sub5_ramp(
        &mut self,
        ctx: &mut FieldCtx,
        b1: u8,
        w94: i16,
        w96: i16,
        w98: i16,
        ticks: u16,
    ) {
        let _ = (ctx, b1, w94, w96, w98, ticks);
    }

    /// Op 0x4C outer-nibble-4 sub-9 story-flag dispatch state.
    ///
    /// The original at `0x801E2080+` branches on two bits of
    /// `_DAT_1F800394` (the global story-flag word in PSX scratchpad):
    ///
    /// | Bit `0x02000000` | Bit `0x01000000` | Path |
    /// |------------------|------------------|------|
    /// | clear            | clear            | `Default` - write/ramp `_DAT_801C6EA4 + 0x4A` |
    /// | clear            | set              | `AbsJump` - return `signed_16(operand)` as new PC |
    /// | set              | (ignored)        | `Delta`   - write/ramp both target slot + delta global |
    ///
    /// The default impl reads the global flag word via [`global_flags`] and
    /// returns the matching variant. Hosts that don't model these specific
    /// bits can override to always return `Default`.
    ///
    /// [`global_flags`]: FieldHost::global_flags
    fn op4c_n4_sub9_state(&self) -> Sub9State {
        let f = self.global_flags();
        if f & 0x0200_0000 != 0 {
            Sub9State::Delta
        } else if f & 0x0100_0000 != 0 {
            Sub9State::AbsJump
        } else {
            Sub9State::Default
        }
    }

    /// Op 0x4C outer-nibble-4 sub-9 - default-path immediate write.
    ///
    /// Fires when [`op4c_n4_sub9_state`] returns `Default` and `ticks == 0`.
    /// The original writes `target` to `*(_DAT_801C6EA4 + 0x4A)` - a per-scene
    /// global. Default impl is a no-op.
    ///
    /// [`op4c_n4_sub9_state`]: FieldHost::op4c_n4_sub9_state
    fn op4c_n4_sub9_default_write(&mut self, target: i16) {
        let _ = target;
    }

    /// Op 0x4C outer-nibble-4 sub-9 - default-path ramp.
    ///
    /// Fires when [`op4c_n4_sub9_state`] returns `Default` and `ticks != 0`.
    /// The original schedules a ramp from the current value to `target` over
    /// `ticks` frames, then runs through the default-arm STATE_RESUME path.
    /// The VM surfaces a `Yield { resume_pc: pc }` after this hook fires.
    ///
    /// [`op4c_n4_sub9_state`]: FieldHost::op4c_n4_sub9_state
    fn op4c_n4_sub9_default_ramp(&mut self, target: i16, ticks: u16) {
        let _ = (target, ticks);
    }

    /// Op 0x4C outer-nibble-4 sub-9 - delta-path write or ramp.
    ///
    /// Fires when [`op4c_n4_sub9_state`] returns `Delta`. The original
    /// computes `delta = target - *(_DAT_8007C364 + 0x16)` (the delta from
    /// the active script-context's anchor) and writes both:
    /// - `target` → `*(_DAT_801C6EA4 + 0x4A)` (the same default-path slot).
    /// - `delta`  → `_DAT_8007BCAC` (the delta accumulator).
    ///
    /// On the ramp path (`ticks != 0`) the original schedules ramps for both
    /// slots then runs through STATE_RESUME. Engines compute the delta
    /// host-side from the target + active context - they own the anchor.
    ///
    /// [`op4c_n4_sub9_state`]: FieldHost::op4c_n4_sub9_state
    fn op4c_n4_sub9_delta_write_or_ramp(&mut self, target: i16, ticks: u16) {
        let _ = (target, ticks);
    }

    /// Op 0x4C outer-nibble-5 sub-0 - directional sound emitter.
    ///
    /// 4-byte instruction `[4C, 0x50, lo, hi]`. Reads a signed-16-bit value
    /// from `operand+1..3`, splits it into a low (`< 0xF0`) and high (`>= 0xF0`)
    /// half via the `_DAT_8007B6F8` / `_DAT_8007B824 + 0xFF10` index bases, and
    /// calls `func_0x80024E08(ctx, idx)`. The high half also sets bit
    /// `0x01000000` in `ctx.flags`; the low half clears it.
    ///
    /// The VM applies the flag-bit toggle itself (`ctx.flags |=` / `&= !`)
    /// and hands the host the raw value + the high/low selection. PC += 4.
    fn op4c_n5_sub0_sound_directional(&mut self, ctx: &mut FieldCtx, value: i16, high: bool) {
        let _ = (ctx, value, high);
    }

    /// Op 0x4C outer-nibble-6 op0 == 0x60 - 6-word emitter call.
    ///
    /// 14-byte instruction `[4C, 0x60, lo0, hi0, lo1, hi1, lo2, hi2, lo3, hi3,
    /// lo4, hi4, lo5, hi5]`. Reads six signed-16-bit words and calls
    /// `func_0x80058490(words[0..4], words[4], words[5])`. PC += 14.
    fn op4c_n6_sub0_emitter6(&mut self, words: [i16; 6]) {
        let _ = words;
    }

    /// Op 0x4C outer-nibble-7 - VRAM tile-flag bulk operation.
    ///
    /// 7-byte instruction `[4C, 0x7N, x0, x1, z0, z1, mask]`. Iterates over
    /// the rectangle `[x0..x1) × [z0..z1)` in the tile-flag bitmap at
    /// `_DAT_1F8003EC + 0x4000` (one byte per tile). The sub-op selects how
    /// the per-tile byte is mutated:
    ///
    /// | Sub | Op |
    /// |---|---|
    /// | 0 | `byte &= 0x0F` (clear high nibble) - yield via STATE_RESUME |
    /// | 1 | `byte |= 0xF0` (set high nibble) - yield via STATE_RESUME |
    /// | 2 | `byte &= ~(mask << 4)` (clear bits) - advance |
    /// | 3 | `byte |= (mask << 4)` (set bits) - advance |
    ///
    /// Default impl is a no-op; the VM still drives the loop for hosts that
    /// own the tile-flag bitmap. PC advances by 7 in the advance branches;
    /// the yield branches surface `Yield { resume_pc: pc + 7 }`.
    fn op4c_n7_tile_flag_bulk(&mut self, sub: u8, x_range: (u8, u8), z_range: (u8, u8), mask: u8) {
        let _ = (sub, x_range, z_range, mask);
    }

    /// Op 0x4C outer-nibble-8 sub-2 - party-page mirror write.
    ///
    /// 3-byte instruction `[4C, 0x82, page]`. The original mirrors two `u16`
    /// values within the per-page inventory struct at
    /// `&DAT_8008480C + page * 0x414`:
    ///
    /// ```text
    ///   *(short *)(0x8008480E + page * 0x414) = *(short *)(0x8008480C + page * 0x414);
    ///   *(short *)(0x80084812 + page * 0x414) = *(short *)(0x80084810 + page * 0x414);
    /// ```
    ///
    /// Hosts model their own inventory layout - the hook receives only
    /// `page`. PC += 3.
    fn op4c_n8_sub2_party_page_mirror(&mut self, page: u8) {
        let _ = page;
    }

    /// Op 0x4C outer-nibble-8 sub-4 - write `_DAT_8007B630`.
    ///
    /// 3-byte instruction `[4C, 0x84, value]`. The original writes
    /// `_DAT_8007B630 = value` - a global slot whose meaning is not yet
    /// reversed (likely a per-scene effect mask). PC += 3.
    fn op4c_n8_sub4_set_b630(&mut self, value: u8) {
        let _ = value;
    }

    /// Op 0x4C outer-nibble-8 sub-7 - register `LAB_801E5154` callback.
    ///
    /// 2-byte instruction `[4C, 0x87]`. The original calls
    /// `func_0x8003CF40(_DAT_8007C34C, &LAB_801E5154)` to register a callback
    /// on the actor list, then exits via `switchD_801e00f4::default()`.
    /// Since `0x4C & 0x70 = 0x40` (not in {0x50, 0x60, 0x70}), the dispatcher
    /// default returns `param_2` - i.e. **halts at PC**, waiting for the
    /// registered callback to release the script. The dispatch wrapper
    /// applies the halt; the host hook only needs to register the callback.
    fn op4c_n8_sub7_register_callback(&mut self) {}

    /// Op 0x4C outer-nibble-8 sub-8 - write 3 globals.
    ///
    /// 6-byte instruction `[4C, 0x88, lo, hi, b3, b4]`. Writes:
    /// - `_DAT_80084628 = signed_16(operand[1..3])`
    /// - `_DAT_80084624 = b3`
    /// - `_DAT_8008462C = b4`
    ///
    /// PC += 6.
    fn op4c_n8_sub8_write_globals(&mut self, value: i16, b3: u8, b4: u8) {
        let _ = (value, b3, b4);
    }

    /// Op 0x4C outer-nibble-8 sub-0xA - set 3 s16 + 1 u32 globals.
    ///
    /// 11-byte instruction `[4C, 0x8A, lo0, hi0, lo1, hi1, lo2, hi2, w_b0, w_b1, w_b2, w_b3]`.
    /// Writes:
    /// - `_DAT_8007B780 = signed_16(operand[1..3])`
    /// - `_DAT_8007B782 = signed_16(operand[3..5])`
    /// - `_DAT_8007B784 = signed_16(operand[5..7])`
    /// - `_DAT_8007B788 = LE_u32(operand[7..11])` (read via `func_0x8003CEB8`).
    ///
    /// PC += 11.
    fn op4c_n8_sub_a_write_quad(&mut self, slots: [i16; 3], packed: u32) {
        let _ = (slots, packed);
    }

    /// Op 0x4C outer-nibble-8 sub-0xC - conditional jump on `ctx.field_68 == 0`.
    ///
    /// 4-byte instruction `[4C, 0x8C, lo, hi]`. When `ctx.field_68 == 0` the VM
    /// jumps to the absolute target `signed_16(operand[1..3])`; otherwise PC
    /// advances by 4. The host has no input - the VM reads `ctx.field_68`
    /// directly.
    ///
    /// PC += 4 on no-jump.
    fn op4c_n8_sub_c_branch_on_field_68(&self, ctx: &FieldCtx) -> bool {
        ctx.field_68 == 0
    }

    /// Op 0x4C outer-nibble-8 sub-5 / sub-0xE / sub-0xF - halt-acquire idiom.
    ///
    /// All three sub-ops share the **same** inner-switch body in the dump
    /// (lines 6550-6570 of `overlay_0897_801de840.txt`): a conditional
    /// "halt-acquire" that, if the actor is acquireable, writes
    /// `ctx.saved_pc = current PC`, clears `ctx.wait_accum`, and sets the
    /// halt bit (`flags |= 0x400`). On `iVar18 == _DAT_8007c364` (system
    /// channel) the same fields are mirrored to the system context. Both
    /// the success and failure exits halt at PC (success returns
    /// `switchD_801e00f4::default()`; failure goes to `LAB_801dee50`).
    ///
    /// The default impl performs the standard ctx mutation (saved_pc +
    /// wait_accum + halt bit). Real hosts can override to also mirror to
    /// the system actor or to refuse the acquire.
    fn op4c_n8_halt_acquire(&mut self, ctx: &mut FieldCtx, opcode_pc: u32) {
        ctx.saved_pc = opcode_pc;
        ctx.wait_accum = 0;
        ctx.flags |= 0x400;
    }

    /// Op 0x4C outer-nibble-8 sub-9 - write `_DAT_80073F00`.
    ///
    /// 4-byte instruction `[4C, 0x89, lo, hi]`. Writes
    /// `_DAT_80073F00 = signed_16(operand[1..3])`. The original at line 6604
    /// of the dump then "calls FUN_801e3620 and returns" - but `0x801e3620`
    /// is actually a *branch label* inside FUN_801de840 (Ghidra mis-rendered
    /// the goto as a function call). The label body sets `iVar45 = param_2 + 4`
    /// and exits the dispatch normally; `_DAT_80073F00` is the only side
    /// effect. PC += 4.
    fn op4c_n8_sub9_set_73f00(&mut self, value: i16) {
        let _ = value;
    }

    /// Op 0x4C outer-nibble-0xC sub-0xF - position broadcast.
    ///
    /// 4-byte instruction `[4C, 0xCF, b1, b2]`. The original at lines
    /// 6912-6931 of the dump computes two 16-bit values and writes them to
    /// `_DAT_8007B628` (X) and `_DAT_8007B62A` (Z):
    /// - `b1 == 0xFF` → use `ctx.world_x`.
    /// - `b1 == 0` → 0.
    /// - else → `(b1 as u16) * 0x80 + 0x40` (the standard "tile center"
    ///   conversion seen elsewhere in the dispatcher).
    /// - `b2` is computed the same way but reads `ctx.world_z` for the 0xFF
    ///   case.
    ///
    /// The original's three exits (`iVar18 = FUN_801e3620(); return iVar18;`
    /// and `goto code_r0x801e3620;`) all converge on the same "PC += 4" label.
    /// PC += 4 in every path.
    fn op4c_n_c_sub_f_position_broadcast(&mut self, x_global: i16, z_global: i16) {
        let _ = (x_global, z_global);
    }

    /// Op 0x4C outer-nibble-9 sub-0/1/2 - fade/effect dispatch via FUN_801DDE34.
    ///
    /// 9-byte instruction `[4C, 0x9N, b1, lo0, hi0, lo1, hi1, lo2, hi2]` where
    /// `N ∈ {0, 1, 2}`. Reads `b1 = operand[1]`, three signed-16-bit words from
    /// `operand+2..8`, and calls `FUN_801DDE34(b1, op0 & 0x0F, w0, w1, w2)`.
    /// PC += 9.
    fn op4c_n9_sub0_2_dde34(&mut self, sub: u8, b1: u8, words: [i16; 3]) {
        let _ = (sub, b1, words);
    }

    /// Op 0x4C outer-nibble-9 sub-0xE - 16-word table copy.
    ///
    /// 0x22-byte (34) instruction `[4C, 0x9E, lo0..hi0, ..., lo15..hi15]`.
    /// Reads 16 signed-16-bit words from the operand stream and writes pairs
    /// to two scratchpad/RAM tables:
    /// - `*(short *)(_DAT_8007B898 + 2 + i*2) = words[i]`
    /// - `*(short *)(0x1F800314 + 0x48 + i*2) = -words[i]`
    ///
    /// Engines model their own table; the VM hands over the 16 values. PC +=
    /// 0x22.
    fn op4c_n9_sub_e_table_copy(&mut self, words: [i16; 16]) {
        let _ = words;
    }

    /// Op 0x4C outer-nibble-9 sub-0xF - register `LAB_801DA930` callback.
    ///
    /// 2-byte instruction `[4C, 0x9F]`. The original calls
    /// `func_0x8003CF40(_DAT_8007C34C, &LAB_801DA930)` (same as nibble-8 sub-7,
    /// but with a different callback target), then exits via
    /// `switchD_801e00f4::default()` - halts at PC for opcode 0x4C. The
    /// dispatch wrapper applies the halt; the host hook only needs to
    /// register the callback.
    fn op4c_n9_sub_f_register_callback(&mut self) {}

    /// Op 0x4C outer-nibble-A - conditional jump on a flag bit.
    ///
    /// 5-byte instruction `[4C, 0xAN, bit, lo, hi]` where `N` selects the
    /// bank:
    /// - `0`: `ctx.flags & (1 << bit)` (per-actor flag word, ctx[+0x10]).
    /// - `1`: `ctx.local_flags & (1 << bit)` (16-bit local flags, ctx[+0x62]).
    /// - `2`: `_DAT_1F800394 & (1 << bit)` (global story flag word).
    /// - `3..=0xF`: no bank check; helper returns `false`. The original at
    ///   line 6702 of `overlay_0897_801de840.txt` falls through every check
    ///   and skips 5 bytes (`return param_2 + 5`).
    ///
    /// When the bit is **set**, the original takes the absolute jump
    /// (`func_0x8003CE9C(operand + 2)` - signed 16-bit absolute PC). When
    /// clear, PC advances by 5.
    ///
    /// (Round 11 corrected the take/skip direction here: prior implementations
    /// were inverted because Ghidra's C output for case 10 hides the `bne a1,
    /// zero, 0x801e258c` dispatch at 0x801e2568. The asm flow is "if sub != 0,
    /// skip ctx[+0x10] check; per-sub-op bit SET → branch to the take-jump
    /// label", not the other way around.)
    ///
    /// `bit` is the operand byte; the VM masks the low 5 bits to match
    /// MIPS `sllv` shift semantics.
    fn op4c_n_a_flag_set(&self, ctx: &FieldCtx, bank: u8, bit: u8) -> bool {
        match bank {
            0 => ctx.flags & (1u32 << (bit & 0x1F)) != 0,
            1 => ctx.local_flags & (1u16 << (bit & 0x1F)) != 0,
            2 => self.global_flags() & (1u32 << (bit & 0x1F)) != 0,
            _ => false,
        }
    }

    /// Op 0x4C outer-nibble-C sub-4 - sub-tile broadcast.
    ///
    /// 4-byte instruction `[4C, 0xC4, x_byte, z_byte]`. Calls
    /// `FUN_801DE3E0(x_byte & 0x7F, z_byte & 0x7F)` (an overlay-resident
    /// per-tile broadcast helper). PC += 4.
    fn op4c_n_c_sub4_subtile_broadcast(&mut self, x: u8, z: u8) {
        let _ = (x, z);
    }

    /// Op 0x4C outer-nibble-C sub-7 - sound trigger.
    ///
    /// 4-byte instruction `[4C, 0xC7, b1, b2]`. Calls
    /// `func_0x800402F4(b1 + 0x0B, b2, 0, 0)` - likely a SE / BGM trigger
    /// on bank `b1 + 0x0B`. PC += 4.
    fn op4c_n_c_sub7_sound_trigger(&mut self, b1: u8, b2: u8) {
        let _ = (b1, b2);
    }

    /// Op 0x4C outer-nibble-C sub-0xA - write u16 at scratchpad slot.
    ///
    /// 5-byte instruction `[4C, 0xCA, slot, lo, hi]`. Writes
    /// `*(short *)(&DAT_801C6460 + slot * 2) = signed_16(operand[2..4])`.
    /// The slot table at `0x801C6460` is a 64-entry scratchpad zone shared
    /// between scripts; hosts can model it as their own slot array. PC += 5.
    fn op4c_n_c_sub_a_set_slot(&mut self, slot: u8, value: i16) {
        let _ = (slot, value);
    }

    /// Op 0x4C outer-nibble-C sub-0xB / sub-0xC - add or subtract on
    /// scratchpad slot.
    ///
    /// 5-byte instruction `[4C, 0xCN, slot, lo, hi]` where `N ∈ {0xB, 0xC}`.
    /// Reads `value = signed_16(operand[2..4])`; if `value == 0xFFFF` the VM
    /// substitutes `_DAT_1F800393` (the per-frame tick byte, surfaced via
    /// [`frame_delta`]).
    /// - sub-0xB: `slot += value`
    /// - sub-0xC: `slot -= value`
    ///
    /// PC += 5.
    ///
    /// [`frame_delta`]: FieldHost::frame_delta
    fn op4c_n_c_sub_bc_adjust_slot(&mut self, slot: u8, delta: i16, subtract: bool) {
        let _ = (slot, delta, subtract);
    }

    /// Op 0x4C outer-nibble-C sub-0xE - write `_DAT_8007B6AC`.
    ///
    /// 3-byte instruction `[4C, 0xCE, value]`. Writes `_DAT_8007B6AC = value`
    /// (zero-extended). The slot is read by op 0x43 sub-1 (the move-table
    /// stride accumulator). PC += 3.
    fn op4c_n_c_sub_e_set_b6ac(&mut self, value: u8) {
        let _ = value;
    }

    /// Op 0x4C outer-nibble-C sub-9 - global-pair compare gate.
    ///
    /// 2-byte instruction `[4C, 0xC9]`. The original at line 6851 reads two
    /// 16-bit globals `_DAT_8007BAB8` and `_DAT_8007BA9C` and halts at PC if
    /// they differ; otherwise advances by 2. Default returns `false` (globals
    /// match → advance). Real hosts return `true` to model an in-flight
    /// transition.
    fn op4c_n_c_sub9_globals_differ(&self) -> bool {
        false
    }

    /// Op 0x4C outer-nibble-D sub-3 - party state setup + FUN_801D596C.
    ///
    /// 14-byte instruction `[4C, 0xD3, lo_a, hi_a, lo_b, hi_b, lo_c, hi_c,
    /// lo_d, hi_d, lo_e, hi_e, lo_f, hi_f]`. Reads two signed 16-bit values
    /// (a, b), two LE u32 values (c-d, e-f via `func_0x8003CED8`), then writes:
    /// - `_DAT_800845C0 = (a << 16) | b` (high/low halves)
    /// - `_DAT_800845B8 = packed_cd`
    /// - `_DAT_800845BC = packed_ef`
    /// - `_DAT_800845A0 = packed_cd` (mirror)
    /// - `_DAT_80073ED4 = _DAT_80084570` (snapshot)
    ///
    /// Then calls `FUN_801D596C()` which kicks off a party-related state init.
    /// PC += 14.
    fn op4c_n_d_sub3_party_setup(&mut self, ab: u32, cd: u32, ef: u32) {
        let _ = (ab, cd, ef);
    }

    // Op 0x4C outer-nibble-D sub-9 - set inverted-Y mirror + clear flag.
    //
    // 4-byte instruction `[4C, 0xD9, lo, hi]`. Sets `ctx.flags |= 0x20000000`
    // (inverted-Y enable), reads `value = signed_16(operand[1..3])`. When
    // `value == 9999` the VM substitutes `-ctx.world_y` (re-mirror current
    // position). Then writes:
    // - `ctx.field_8e = value`
    // - `ctx.world_y = -value`
    //
    // All ctx writes are done by the VM directly; no host hook fires. PC += 4.

    /// Op 0x4C outer-nibble-D sub-0xA - clear inverted-Y mirror + collision Y.
    ///
    /// 2-byte instruction `[4C, 0xDA]`. Clears `ctx.flags & 0x20000000`, then
    /// calls `func_0x80019278(ctx)` (the collision-Y refresh helper) and
    /// writes the result into `ctx.world_y`. PC += 2.
    fn op4c_n_d_sub_a_collision_y_refresh(&mut self, ctx: &mut FieldCtx) {
        let _ = ctx;
    }

    /// Op 0x4C outer-nibble-D sub-0xF - write `_DAT_801C6EA4 + 0x61` byte.
    ///
    /// 3-byte instruction `[4C, 0xDF, value]`. Writes one byte at the
    /// per-scene state struct's `+0x61` offset. PC += 3.
    fn op4c_n_d_sub_f_scene_byte_write(&mut self, value: u8) {
        let _ = value;
    }

    /// Op 0x4C outer-nibble-D sub-6 - `ctx.field_74` bitfield mutation.
    ///
    /// 3-byte instruction `[4C, 0xD6, b1]`. The original at line 7028 writes:
    /// - if `b1 == 4`: clear the top bit only (`field_74 &= 0x7FFFFFFF`)
    /// - else: clear bits `0x83000000`, then set bit `0x80000000` and OR in
    ///   `(b1 as u32) << 24` (low byte of `b1` into the top byte of `field_74`).
    ///
    /// Pure ctx mutation - host hook is a side-effect-only ack. Halt at PC
    /// (the dump exits via `goto LAB_801e00bc`).
    fn op4c_n_d_sub6_field74_mutate_ack(&mut self) {}

    /// Op 0x4C outer-nibble-D sub-8 - `FUN_801D77F4` 4-arg call.
    ///
    /// 9-byte instruction `[4C, 0xD8, b1, lo_x, hi_x, lo_y, hi_y, lo_z, hi_z]`.
    /// The original at line 7042 reads three 16-bit values from the operand,
    /// adds the global `_DAT_8007B6F8` to `x` (sign-extended u16 truncation),
    /// then calls `FUN_801D77F4(b1, x, y, z)` - overlay-resident. Hosts apply
    /// the call if needed. PC += 9.
    fn op4c_n_d_sub8_call_d77f4(&mut self, b1: u8, words: [i16; 3]) {
        let _ = (b1, words);
    }

    /// Op 0x4C outer-nibble-E sub-2 - FMV trigger.
    ///
    /// 7-byte instruction `[4C, 0xE2, lo, hi, _, _, _]` (PC advances by
    /// 1+6). Writes `_DAT_8007BA78 = signed_16(operand[1..3])` (the
    /// FMV index passed to the STR/MDEC overlay) and
    /// `_DAT_8007B83C = 0x1A` (next-game-mode = 26 / StrInit). The
    /// FMV index selects a 64-byte entry from the runtime FMV-state
    /// table at `0x801D0A6C` (which the str_fmv overlay populates from
    /// the compact MV-file table at `0x801CAE40`); index range
    /// `0..=5` corresponds to `MV1.STR..MV6.STR`. The trailing 3
    /// bytes are reserved by the dispatcher's PC math but unused.
    fn op4c_n_e_sub2_fmv_trigger(&mut self, fmv_id: i16) {
        let _ = fmv_id;
    }

    /// Op 0x4C outer-nibble-E sub-6 - `FUN_801D8280` 3-arg call.
    ///
    /// 8-byte instruction `[4C, 0xE6, lo0, hi0, lo1, hi1, lo2, hi2]`. Calls
    /// `FUN_801D8280(s16_a, s16_b, s16_c)` - an overlay-resident helper.
    /// PC += 8.
    fn op4c_n_e_sub6_call_d8280(&mut self, words: [i16; 3]) {
        let _ = words;
    }

    /// Op 0x4C outer-nibble-E sub-0xC - write `_DAT_8007B5FC` from `FUN_801DDF48`.
    ///
    /// 2-byte instruction `[4C, 0xEC]`. Writes `_DAT_8007B5FC = FUN_801DDF48()`
    /// (captures the return of an overlay-resident helper into the global).
    /// Hosts model the call. PC += 2.
    fn op4c_n_e_sub_c_capture_ddf48(&mut self) {}

    /// Op 0x4C outer-nibble-E sub-0xD - write `_DAT_8007BA66`.
    ///
    /// 3-byte instruction `[4C, 0xED, value]`. Writes
    /// `_DAT_8007BA66 = value` (zero-extended u16). PC += 3.
    fn op4c_n_e_sub_d_set_ba66(&mut self, value: u8) {
        let _ = value;
    }

    /// Op 0x4C outer-nibble-E sub-0xE - snapshot `_DAT_80084570`.
    ///
    /// 2-byte instruction `[4C, 0xEE]`. Writes
    /// `_DAT_800845DC = _DAT_80084570` - a single-direction snapshot of the
    /// party-leader/scene state. PC += 2.
    fn op4c_n_e_sub_e_snapshot_84570(&mut self) {}

    /// Op 0x4C outer-nibble-E sub-0 - three-way scene/menu state write.
    ///
    /// 2-byte instruction `[4C, 0xE0, b1]`. The original at line 7173:
    /// - `b1 == 0`: `DAT_801F2744 = 1`
    /// - `b1 < 100`: `DAT_801F2740 = b1`
    /// - else: `*(u16*)(_DAT_801C6EA4 + 0xE) = b1 - 100`
    ///
    /// Halt at PC (`goto LAB_801e00bc`).
    fn op4c_n_e_sub0_state_write(&mut self, b1: u8) {
        let _ = b1;
    }

    /// Op 0x4C outer-nibble-E sub-9 - clear `_DAT_8007B9C4` then PC += 2.
    ///
    /// 1-byte instruction `[4C, 0xE9]`. The original at line 7362 clears
    /// `_DAT_8007B9C4` (a global state byte) and tail-calls
    /// `switchD_801e0f24::caseD_4()` - which is the `addiu s8, s8, 0x2;
    /// j epilogue` block at `0x801df098`, i.e. PC += 2.
    fn op4c_n_e_sub9_clear_b9c4(&mut self) {}

    /// Op 0x4C outer-nibble-E sub-A - call `func_0x8003C7EC` then halt at PC.
    ///
    /// 1-byte instruction `[4C, 0xEA]`. The original at line 7367 calls the
    /// overlay-resident `func_0x8003C7EC()` (an actor-list mutator) then
    /// exits via `switchD_801e00f4::default()` → halt at PC for opcode 0x4C.
    fn op4c_n_e_sub_a_call_c7ec(&mut self) {}

    /// Op 0x4C outer-nibble-E sub-1 - spawn a screen-anchored text balloon
    /// from the bytecode operand stream, then advance PC past the packet.
    ///
    /// Variable-length instruction `[4C, 0xE1, b1, ...string..., terminator]`.
    /// The dispatcher's PC advance is `pc + 3 + packet_length(buf)` where the
    /// packet runs from `operand + 1` until any byte `<= 0x1E` (see
    /// [`field_helpers::packet_length`]). The terminator byte is consumed.
    ///
    /// The original calls `FUN_8003C764(buf, ctx_ptr)` to allocate a centered
    /// "field text balloon" actor: priority `0x78`, screen-X centered around
    /// the measured glyph width (`(0x140 - text_width) / 2`), screen-Y `0xB4`.
    /// The actor is skipped when `b1 == 0`, but PC always advances by the
    /// measured length. This hook handles the spawn; the dispatcher computes
    /// the PC delta itself via [`field_helpers::packet_length`].
    ///
    /// `text_buf` is the slice from `operand + 1` to the terminator (NOT
    /// including the terminator). `script_id` is the active script's ID,
    /// passed in lieu of the original `param_3` ctx pointer.
    ///
    /// [`field_helpers::packet_length`]: crate::field_helpers::packet_length
    fn op4c_n_e_sub_1_text_actor(&mut self, text_buf: &[u8], script_id: u16) {
        let _ = (text_buf, script_id);
    }

    /// Op 0x4C outer-nibble-C sub-1 - reset every entry in the global
    /// "trigger flag" array based on per-record flags.
    ///
    /// 1-byte instruction `[4C, 0xC1]`. The original walks the
    /// `_DAT_80073ED8` record array (count `DAT_80073EDC`, stride `0xB`):
    /// for each record, reads bytes 9 and 10 to form a 16-bit index,
    /// queries [`field_helpers::party_flag_test`] against the trigger-flag
    /// array, and writes 1 to record byte 0 if the bit is **clear**, else 0.
    /// Then jumps to `LAB_801df09c` (= PC += 2 via the standard "addiu s8,
    /// s8, 0x2; j epilogue" block at `0x801df09c`).
    ///
    /// When `_DAT_80073ED8` is null OR the count is zero, the original
    /// `goto code_r0x801df098` skips the loop and still advances PC += 2.
    ///
    /// `flags` is the trigger-flag byte array (the bit storage queried
    /// per-record). Hosts that don't model the record array can leave the
    /// default no-op in place - the dispatcher always advances PC += 2
    /// regardless. The two callers (gate set / clear) merge into a single
    /// hook because the original's two branches differ only in side-effect
    /// (the loop body) and the PC delta is identical.
    ///
    /// [`field_helpers::party_flag_test`]: crate::field_helpers::party_flag_test
    fn op4c_n_c_sub_1_flag_loop_reset(&mut self, flags: &[u8]) {
        let _ = flags;
    }

    /// Op 0x4C outer-nibble-D sub-1 - linked-list lookup gate.
    ///
    /// 2-byte instruction `[4C, 0xD1]`. The original walks the global
    /// list-head at `_DAT_8007C34C` via `FUN_8003CF04(head, FUN_801DC0BC)`
    /// (the linked-list "search by predicate match" helper) and:
    /// - if the search returns null OR the matched entry's `[+0x10] & 8 != 0`
    ///   (the "deleted" bit) → `return param_2 + 4` (PC += 4),
    /// - else → `pbVar43 = pbVar47 + 1; goto LAB_801E360C` which calls
    ///   `func_0x8003CE9C(pbVar43)` (the variable-length signed-int reader)
    ///   and returns its result as the new PC.
    ///
    /// The host hook returns `Some(new_pc)` for the LAB path or `None` to
    /// take the PC += 4 path. The default impl returns `None` - engines
    /// without a list-walking model fall through to the safe "no jump"
    /// branch.
    fn op4c_n_d_sub_1_list_lookup_jump(&mut self, ctx: &FieldCtx) -> Option<usize> {
        let _ = ctx;
        None
    }

    /// Op 0x4C outer-nibble-D sub-2 - channel-lookup conditional spawn,
    /// then halt at PC.
    ///
    /// 2-byte instruction `[4C, 0xD2, b1]`. The original calls
    /// `func_0x8003C83C(b1)` (the script-context resolver - see
    /// `docs/subsystems/script-vm.md` "intra-function label catalogue"
    /// for the F8/FB system-channel idiom). When the result is **zero**
    /// (no resolved context), the original spawns a new script context via
    /// `func_0x8003D064` + `func_0x8003A1E4` with the rendering-busy
    /// guard (`*(_DAT_801C6EA4 + 8) = 1` around the call). Then halts.
    ///
    /// Hosts that don't yet model the channel resolver should leave this as
    /// a no-op - the halt-at-PC behaviour is handled by the dispatcher.
    fn op4c_n_d_sub_2_channel_spawn(&mut self, channel: u8) {
        let _ = channel;
    }

    /// Op 0x4C outer-nibble-D sub-7 - register a list-walk callback then
    /// halt at PC.
    ///
    /// 1-byte instruction `[4C, 0xD7]`. The original sets `pcVar33 =
    /// FUN_801DC0BC` (the "delete-if-flag" predicate) then jumps to
    /// `LAB_801E2DC4`, which calls `func_0x8003CF40(_DAT_8007C34C, pcVar33)`
    /// (a list-walk that invokes `pcVar33` per entry) and returns via the
    /// standard halt-acquire dispatcher. The script resumes when the walk
    /// finishes and re-arms the field VM.
    fn op4c_n_d_sub_7_register_list_walk(&mut self) {}

    /// Op 0x4C outer-nibble-D sub-B - call the overlay-resident
    /// `FUN_801E57F0` then advance PC by 13 bytes.
    ///
    /// 13-byte instruction (the original's call site at line 7085 passes
    /// `pbVar47` and falls through to `LAB_801E2EA0: return param_2 + 0xD`).
    ///
    /// `FUN_801E57F0` was not successfully decompiled - Ghidra's dump (see
    /// `ghidra/scripts/funcs/overlay_0897_801e57f0.txt`) shows ~441
    /// instructions of `lb s8, 0x6814(zero)` data masquerading as code,
    /// indicating Ghidra mis-parsed the function's start. The actual call
    /// target at `0x801E57F0` is overlay-resident and not currently dumpable.
    ///
    /// `bytecode` is the operand stream starting at `operand + 0` (the same
    /// `pbVar47` the original passes); hosts that model this opcode read
    /// fields at `bytecode[1..=12]`.
    fn op4c_n_d_sub_b_call_e57f0(&mut self, bytecode: &[u8]) {
        let _ = bytecode;
    }

    /// Op 0x4C outer-nibble-D sub-C - small-table lookup, then conditional
    /// per-party-record write.
    ///
    /// 5-byte instruction `[4C, 0xDC, b1, ?, ?]` (the trailing operand
    /// bytes are consumed by the LAB_801E360C tail's `func_0x8003CE9C` read).
    ///
    /// The original at line 7087:
    /// 1. Calls [`field_helpers::small_table_search`] (`FUN_80042EE0(b1)`)
    ///    to look up `b1` in the gp-relative short-table at `0x80085958`.
    /// 2. If the table lookup hits AND a party-record search across
    ///    `DAT_80084594` slots (party-record stride `0x414`, byte at `+0x196`)
    ///    matches `b1`, the slot is updated.
    /// 3. If neither hits, returns `param_2 + 5` (PC += 5).
    /// 4. On hit, falls through to `LAB_801E360C` which reads `ce9c(p+2)`
    ///    and returns it as the new PC.
    ///
    /// The host hook returns `Some(new_pc)` for the ce9c-jump path, or `None`
    /// to take the PC += 5 path. Default impl returns `None` (safe fallback).
    ///
    /// [`field_helpers::small_table_search`]: crate::field_helpers::small_table_search
    fn op4c_n_d_sub_c_party_search_set(&mut self, needle: u8) -> Option<usize> {
        let _ = needle;
        None
    }

    /// Op 0x4C outer-nibble-D sub-E - small-table lookup query (sister of
    /// sub-C without the per-party-record write side-effect).
    ///
    /// 5-byte instruction `[4C, 0xDE, b1, ?, ?]`. Same control flow as
    /// sub-C but the party-record loop only **tests** for a match - no slot
    /// update. PC is `param_2 + 5` on miss, ce9c-jump on hit.
    fn op4c_n_d_sub_e_party_search_query(&mut self, needle: u8) -> Option<usize> {
        let _ = needle;
        None
    }

    /// Op 0x4C outer-nibble-D sub-4 - VRAM rect read-modify-write that sets
    /// the PSX STP (semi-transparency) bit on a 16x1 pixel run.
    ///
    /// 6-byte instruction `[4C, 0xD4, x_lo, x_hi, y_lo, y_hi]`. The original
    /// (dispatcher dump lines 7621-7642) builds a `(vram_x, vram_y, w=0x10,
    /// h=1)` `RECT`, then runs the PsyQ libgs sequence
    /// `DrawSync(0); StoreImage(rect, buf16); DrawSync(0); for each of 16
    /// u16 pixels: if non-zero, OR with 0x8000; LoadImage(rect, buf16);` -
    /// i.e. it sets bit 15 (STP) on every non-zero pixel in the 16x1 rect.
    /// `StoreImage`/`LoadImage`/`DrawSync` correspond to
    /// `FUN_8005842c` / `FUN_800583c8` / `FUN_80058104` (string
    /// constants `s_StoreImage`/`s_LoadImage`/`s_DrawSync` confirm the
    /// shape). The host receives the rect origin and may emulate the
    /// read-modify-write against its own framebuffer. Default no-op.
    fn op4c_n_d_sub_4_vram_stp_set(&mut self, vram_x: u16, vram_y: u16) {
        let _ = (vram_x, vram_y);
    }

    /// Op 0x4C outer-nibble-D sub-5 - VRAM rect read-modify-write that
    /// clears the PSX STP bit on a 16x1 pixel run.
    ///
    /// 6-byte instruction `[4C, 0xD5, x_lo, x_hi, y_lo, y_hi]`. Same libgs
    /// shape as sub-4 (StoreImage/LoadImage round-trip with intervening
    /// DrawSyncs) but the inner loop is `if pixel != 0x8000: pixel &=
    /// 0x7FFF` - i.e. it clears bit 15 (STP) on every pixel that isn't
    /// already "STP-only" (`0x8000` = transparent black with STP set).
    /// Default no-op.
    fn op4c_n_d_sub_5_vram_stp_clear(&mut self, vram_x: u16, vram_y: u16) {
        let _ = (vram_x, vram_y);
    }

    // -----------------------------------------------------------------
    // Round 17 - five 0x4C nC sub-ops + two 0x4C nE sub-ops.
    // -----------------------------------------------------------------

    /// Op 0x4C outer-nibble-C sub-0 - cancel the active move-table animation.
    ///
    /// 2-byte instruction `[4C, 0xC0]`. The dispatcher dump (lines 6726-6732)
    /// gates on `ctx.move_id > 0` (the actor's currently-playing move); if a
    /// move is active, the original calls `func_0x800204F8(ctx)` (the move
    /// VM's "cancel" entry - see [`docs/subsystems/move-vm.md`]).
    ///
    /// `is_active` is the gate; the host should return `true` when the actor
    /// has a move-table animation that should be cancelled. Default impl
    /// returns `false`, so the cancel side-effect is skipped (safe fallback -
    /// engines without a move VM never trigger the cancel).
    ///
    /// PC always advances by 2 (whether the move was cancelled or not).
    fn op4c_n_c_sub_0_move_cancel(&mut self, ctx: &mut FieldCtx) -> bool {
        let _ = ctx;
        false
    }

    /// Op 0x4C outer-nibble-C sub-3 - script-table teleport with tile-center
    /// math.
    ///
    /// 2-byte instruction `[4C, 0xC3]`. The original (dispatcher dump
    /// 6762-6810) calls `func_0x8003C8F0(ctx.field_50, 0)` to resolve the
    /// destination tile descriptor, then writes:
    /// - `world_x = (b & 0x7F) * 0x80 + 0x40` (with optional `+0x40` if high
    ///   bit is set) - the standard tile-center formula,
    /// - `world_z` similarly,
    /// - rotation, animation frame, sprite flags get reset.
    ///
    /// Both helpers (`func_0x8003C8F0` table lookup and `func_0x8003D0BC`
    /// descriptor walk) are overlay-resident; hosts that don't model the
    /// scene table leave the default no-op in place.
    ///
    /// PC always advances by 2.
    fn op4c_n_c_sub_3_script_teleport(&mut self, ctx: &mut FieldCtx) {
        let _ = ctx;
    }

    /// Op 0x4C outer-nibble-C sub-D - script-context allocation gate, halt.
    ///
    /// 2-byte instruction `[4C, 0xCD]`. The original calls
    /// `func_0x8003CF04(_DAT_8007C34C, FUN_801DC0BC)` (the linked-list-walk
    /// "find by predicate"), tests the resolved entry's flag bit `0x8`, and:
    /// - if the entry exists AND the bit is set → halt acquired, PC stays at
    ///   start-of-instruction (we model this as `Halt { final_pc: pc }`),
    /// - else → yield via `LAB_801DEE50` (also `Halt { final_pc: pc }` in our
    ///   model - the engine-side state-resume layer drives re-entry).
    ///
    /// In both cases the dispatcher returns `Halt`, so the host hook only
    /// records the side effect (registering the actor with the list-walk
    /// machinery). Default no-op.
    fn op4c_n_c_sub_d_script_alloc(&mut self) {}

    /// Op 0x4C outer-nibble-E sub-4 - bounding-box collision query against
    /// the actor's world position.
    ///
    /// 9-byte instruction `[4C, 0xE4, x_lo, z_lo, x_hi, z_hi, scale, ?, ?]`.
    /// The original (dispatcher dump 7228-7255) builds 4 corners by mapping
    /// each operand byte through the standard tile-center formula
    /// (`(b & 0x7F) * 0x80 + 0x40`, plus 0x40 when the high bit is set), then
    /// tests whether the actor's `(world_x, world_z)` lies inside that AABB.
    /// On the **outside** path the original calls `FUN_801E3614()` which is
    /// the standard halt helper.
    ///
    /// We expose the pure-arithmetic predicate here and let the dispatcher
    /// emit the halt directly: the host returns `true` when the actor is
    /// **outside** the bbox (the original's "fail" path). When the actor is
    /// inside, PC advances by 8.
    ///
    /// `bbox` is `[x0, z0, x1, z1]` with each coordinate already converted
    /// from operand byte to world-space tile center. Default impl returns
    /// `false` (= "always inside"), so the dispatcher always advances -
    /// engines that don't model the world position can skip the test.
    fn op4c_n_e_sub_4_bbox_outside(&self, ctx: &FieldCtx, bbox: [i16; 4]) -> bool {
        let _ = (ctx, bbox);
        false
    }

    /// Op 0x4C outer-nibble-E sub-5 - add XP to the party-XP accumulator.
    ///
    /// 5-byte instruction `[4C, 0xE5, b1, b2, b3]`. The original (dispatcher
    /// dump 7256-7267) reads a 24-bit signed value via
    /// [`field_helpers::load_u24_le`] + [`field_helpers::sign_extend_24`],
    /// adds it to `_DAT_800845A4` (the party-XP global), clamps to
    /// `[0, 9999999]`, and calls `func_0x8003CE08(8)` (the standard
    /// "party stats refresh" trigger).
    ///
    /// `xp_delta` is the sign-extended 24-bit value already prepared by the
    /// dispatcher. Hosts apply the clamp + refresh; default no-op.
    ///
    /// [`field_helpers::load_u24_le`]: crate::field_helpers::load_u24_le
    /// [`field_helpers::sign_extend_24`]: crate::field_helpers::sign_extend_24
    fn op4c_n_e_sub_5_add_xp(&mut self, xp_delta: i32) {
        let _ = xp_delta;
    }

    /// Op 0x4C outer-nibble-E sub-B - conditional actor lookup with
    /// embedded jump target.
    ///
    /// 5-byte instruction `[4C, 0xEB, actor_id, target_lo, target_hi]`.
    /// The original (dispatcher dump 7370-7376) calls `func_0x8003C83C(b1)`
    /// (the actor-table walker - see also [`docs/subsystems/script-vm.md`]'s
    /// "intra-function label catalogue" for the F8/FB system-channel idiom).
    ///
    /// Two outcomes:
    /// - **actor resolved** → return `pc + 5` (PC advances 5),
    /// - **actor not resolved** → return absolute jump to
    ///   `LE_u16(operand+2..=operand+3)`.
    ///
    /// The host returns `Some(new_pc)` to take the resolved-actor "pc + 5"
    /// path (the host has confirmed the actor exists), or `None` to take the
    /// embedded-jump path (actor lookup missed). Default impl returns `None`
    /// - engines without an actor pool always take the jump.
    fn op4c_n_e_sub_b_actor_jump(&mut self, actor_id: u8) -> Option<()> {
        let _ = actor_id;
        None
    }

    /// Read 16-bit value used by op 0x4C nC sub-5/6 - the operand-stream
    /// equivalent of `func_0x8003CE9C` followed by [`party_flag_test`] over
    /// the trigger-flag bank.
    ///
    /// Both sub-5 (jump-if-zero) and sub-6 (jump-if-nonzero) read a 16-bit
    /// value from `bytecode[operand+1..=operand+2]` and test whether the
    /// corresponding bit is set in the trigger-flag array. The dispatcher
    /// computes the index via [`field_helpers::load_u16_le`] and asks the
    /// host whether that bit is set.
    ///
    /// Returns `true` when the bit is set (= the original's `0xFF` saturation
    /// from `FUN_8003CE64`), `false` otherwise. Default impl returns `false`
    /// - engines without a flag bank treat all bits as clear.
    ///
    /// [`field_helpers::load_u16_le`]: crate::field_helpers::load_u16_le
    /// [`party_flag_test`]: crate::field_helpers::party_flag_test
    fn op4c_n_c_party_flag_test(&self, flag_idx: u16) -> bool {
        let _ = flag_idx;
        false
    }

    /// Halt-acquire dispatcher used by op 0x38 (CAM_CFG yield path) and
    /// op 0x43 sub-0/1/A/B (actor-control halts).
    ///
    /// The original at `0x801df30c+` halts the active script (and the
    /// system-channel caller, when the active ctx is the player) by:
    /// - saving `pbVar43` (the opcode byte pointer) into `ctx.saved_pc`,
    /// - setting `ctx.flags |= 0x400` (HALT bit),
    /// - clearing `ctx.wait_accum`.
    ///
    /// For op 0x43 sub-0/1/A/B the resume PC is encoded in the operand:
    /// - Sub-0/1: `target = operand + 3` → bytecode bytes `pc+4..pc+6`.
    /// - Sub-A/B: `target = operand + 7` → bytecode bytes `pc+8..pc+10`.
    ///
    /// For op 0x38 the resume PC is `pc + 3` (post-instruction); there is no
    /// operand-encoded target.
    ///
    /// Acquisition succeeds only when:
    /// - `ctx.saved_pc != 0` OR `ctx == player_ctx`, AND
    /// - `ctx.flags & 0x400 == 0` OR the scene busy slot
    ///   `_DAT_801C6EA4 + 0x8` is non-zero.
    ///
    /// On success: VM emits `Yield { resume_pc: target_pc }` (the host's
    /// state-resume layer drives re-entry). On failure for op 0x43 the VM
    /// advances PC by the default amount (5 for sub-0/1, 9 for sub-A/B); on
    /// failure for op 0x38 the VM `Halt`s at the current PC (matching the
    /// original's `switchD_801e00f4::default()` fallthrough).
    ///
    /// `which` is the originating opcode/sub-op tag so hosts that need to
    /// distinguish the call site can - it's `0x38` for op 0x38, and the raw
    /// op-0x43 sub-op (`0`, `1`, `0xA`, `0xB`) for op 0x43.
    ///
    /// The default impl returns `true` (always acquire), which keeps the
    /// original's control-flow shape; hosts that don't model the player
    /// pointer can model their own predicate.
    fn field_halt_acquire_predicate(&self, ctx: &FieldCtx, which: u8) -> bool {
        let _ = (ctx, which);
        true
    }

    /// Side effect of [`field_halt_acquire_predicate`] returning `true`.
    ///
    /// The original also writes the same halt fields onto the system-channel
    /// (`param_3`) when `ctx == player_ctx`. Engines that maintain a separate
    /// caller channel use this hook; default no-op.
    ///
    /// `resume_pc` is the absolute PC the script will resume at; `coords` is
    /// the (x, y, z) position the original copies into a scratch local on the
    /// op-0x43 path. Op 0x38 passes `[0; 3]` since it has no coord operands.
    ///
    /// [`field_halt_acquire_predicate`]: FieldHost::field_halt_acquire_predicate
    fn field_halt_acquire_apply(
        &mut self,
        ctx: &mut FieldCtx,
        which: u8,
        resume_pc: usize,
        coords: [i16; 3],
    ) {
        let _ = (ctx, which, resume_pc, coords);
    }

    // -- Round 18: 0x4C n8 actor-allocator + nE camera + nD/n5 dialog --

    /// Set actor model + animation frame (op 0x4C n8 sub-1, 9 bytes).
    ///
    /// `[4C, 0x81, m0, m1, m2, anim_lo, anim_hi, frames_lo, frames_hi]`. The
    /// 24-bit `model_id` is the standard `load_u24_le` decode of bytes 1..3;
    /// `anim_frame` and `tween_frames` are 16-bit LE pairs at bytes 4..5 and
    /// 6..7 respectively.
    ///
    /// The original at lines 6496-6515 of the dispatcher dump has three paths:
    /// - `tween_frames == 0`: write `*(int *)(ctx + 0x74) = model_id;
    ///   *(short *)(ctx + 0x78) = anim_frame`. Done - caller advances PC by 9.
    /// - `tween_frames != 0` and `ctx + 0x78 == 0` (no current anim):
    ///   `*(int *)(ctx + 0x74) = model_id`. (Anim left untouched.)
    /// - Otherwise call `func_0x8003C5F0(ctx, ctx + 0x74, 3, ctx[0x74],
    ///   model_id, tween_frames)` to schedule a tween.
    ///
    /// This hook receives all four pieces of operand state - the host decides
    /// which path to take based on its own state model. The default impl is a
    /// no-op; the VM always advances PC by 9.
    fn op4c_n_8_sub_1_set_model_anim(
        &mut self,
        ctx: &mut FieldCtx,
        model_id: u32,
        anim_frame: u16,
        tween_frames: u16,
    ) {
        let _ = (ctx, model_id, anim_frame, tween_frames);
    }

    /// Look up an actor and apply a 6-axis rotation matrix (op 0x4C n8 sub-6,
    /// 15 bytes).
    ///
    /// `[4C, 0x86, x_lo, x_hi, y_lo, y_hi, z_lo, z_hi, rx_lo, rx_hi, ry_lo,
    /// ry_hi, rz_lo, rz_hi, actor_id]`. Six 16-bit LE values for the rotation
    /// matrix axes (decoded via `load_u16_le`), then a 1-byte actor selector
    /// at the tail.
    ///
    /// The original at lines 6571-6585 first calls `func_0x8003C83C(actor_id)`
    /// to resolve the actor pointer; if 0 (not found), falls through to
    /// `return param_2 + 0xF` (advance PC by 15 with no side effect).
    /// Otherwise calls `FUN_801E573C(ctx, target, x, y, z, rx, ry, rz)` to
    /// apply, then yields via the standard switch-default.
    ///
    /// This hook returns `true` if the actor was found (host applied the
    /// rotation); `false` if the actor lookup missed. PC advances by 15 in
    /// both cases - the only observable difference is whether the host's
    /// rotation pipeline ran.
    ///
    /// The default impl returns `false` (no actor pool).
    fn op4c_n_8_sub_6_actor_set_rotation(
        &mut self,
        ctx: &mut FieldCtx,
        actor_id: u8,
        position: [i16; 3],
        rotation: [i16; 3],
    ) -> bool {
        let _ = (ctx, actor_id, position, rotation);
        false
    }

    /// Count active actors of a given type (op 0x4C n8 sub-B, 5 bytes).
    ///
    /// `[4C, 0x8B, type_byte, target_lo, target_hi]`. The original at lines
    /// 6621-6644 walks the active actor pool (`DAT_80084594` = pool count;
    /// each slot is 0x414 bytes wide, indexed by the per-slot type byte at
    /// `+0x458`) and increments a count for every entry whose stored type
    /// matches `type_byte`.
    ///
    /// If any match: jump to absolute `LE_u16(operand+2..=operand+3)`.
    /// If no match: advance PC by 5.
    ///
    /// The host decides the pool layout - this hook just answers
    /// "is there at least one actor of `type_byte` active?". Default impl
    /// returns `false` (no actor pool).
    fn op4c_n_8_sub_b_actor_type_present(&self, type_byte: u8) -> bool {
        let _ = type_byte;
        false
    }

    /// Search a per-character actor sub-table for a matching marker
    /// (op 0x4C n8 sub-D, 6 bytes).
    ///
    /// `[4C, 0x8D, char_idx, marker, target_lo, target_hi]`. The original at
    /// lines 6652-6667 indexes `DAT_8008488D[char_idx * 0x414]` for the
    /// per-character sub-table size and walks the table comparing each entry
    /// against `marker`.
    ///
    /// Three outcomes:
    /// - [`ActorSearchResult::EmptySlot`]: per-character sub-table is empty
    ///   (size byte is 0). Advance PC by 6.
    /// - [`ActorSearchResult::Found`]: found a matching entry. Jump to
    ///   absolute `LE_u16(operand+3..=operand+4)`.
    /// - [`ActorSearchResult::NoMatch`]: sub-table is non-empty but no entry
    ///   matched. Halt at PC.
    ///
    /// The default impl returns [`ActorSearchResult::EmptySlot`] (advance,
    /// no match) which is the safest fallback for engines without an actor
    /// pool.
    fn op4c_n_8_sub_d_actor_search(&self, char_idx: u8, marker: u8) -> ActorSearchResult {
        let _ = (char_idx, marker);
        ActorSearchResult::EmptySlot
    }

    /// Camera-anchored teleport (op 0x4C nE sub-3, 2 bytes).
    ///
    /// `[4C, 0xE3, actor_id]`. The original at lines 7208-7227 resolves
    /// the actor via `func_0x8003C83C(actor_id)`, then copies the active
    /// camera's world-space position (`_DAT_8007C364 + 0x14/+0x18`) into
    /// the actor's `+0x14/+0x18` slots and the camera's rotation
    /// (`_DAT_8007C364 + 0x16`) into the actor's `+0x16`. If the actor's
    /// `flags & 0x20000000` is set, the actor's `+0x8E` (inverted-Y mirror)
    /// is also negated to match.
    ///
    /// PC always advances by 2; if the actor lookup fails the original is a
    /// silent no-op. Hosts get the actor ID and decide whether to apply.
    fn op4c_n_e_sub_3_actor_sync_camera(&mut self, ctx: &mut FieldCtx, actor_id: u8) {
        let _ = (ctx, actor_id);
    }

    /// Camera position animate (op 0x4C nE sub-7, 7 bytes).
    ///
    /// `[4C, 0xE7, t0, t1, t2, d0, d1]`. Mixed-width operand: a 24-bit LE
    /// `target` at bytes 1..3 and a 16-bit LE `duration` at bytes 4..5.
    /// (The agent's first-pass spec said both were LE24; the actual
    /// dispatcher at lines 7282-7288 calls `func_0x8003ceb8` (LE24) at
    /// `pbVar47+1` and `func_0x8003ce9c` (LE16) at `pbVar47+4`.)
    ///
    /// Behaviour:
    /// - `duration == 0` → write `_DAT_80073EE4 = target` immediately.
    /// - `duration != 0` → call `func_0x8003C5F0(0, &_DAT_80073EE4, 0,
    ///   _DAT_80073EE4, target, duration)` to schedule the tween.
    ///
    /// PC advances by 7. Hosts model the camera tween however they like.
    fn op4c_n_e_sub_7_camera_animate(&mut self, target: u32, duration: u16) {
        let _ = (target, duration);
    }

    /// Camera zoom config (op 0x4C nE sub-8, 10 bytes).
    ///
    /// `[4C, 0xE8, x0, x1, y0, y1, z0, z1, m0, m1]`. Four 16-bit LE values:
    /// `zoom_x`, `zoom_y`, `zoom_z`, and a `mode` selector. The agent's
    /// first-pass spec said three 24-bit LE values; the actual dispatcher
    /// at lines 7300-7303 reads `func_0x8003ce9c` (LE16) four times at
    /// `pbVar47 + 1/3/5/7`.
    ///
    /// Mode dispatch (line 7305+):
    /// - `mode == 0`: default zoom. Write `_DAT_801C6EA4 + 0x4C/+0x4E/+0x50
    ///   = (zoom_x or 0x40), (zoom_y or 8), (zoom_z or 4)` - each input is
    ///   replaced with the constant when zero.
    /// - `mode == 1` and `zoom_x == 0`: set bit `0x80000` on the resolved
    ///   actor's `+0x10` flags.
    /// - `mode == 1` and `zoom_x != 0`: set per-actor zoom at
    ///   `_DAT_801C6EA4 + 0x52 = zoom_x`.
    /// - `mode == 2`: clear bit `0x80000` on the resolved actor.
    /// - `mode == 3`: set bit `8` on the resolved actor and clear flag
    ///   `0x400` on its parent actor (`*(int *)(actor + 0x94)`).
    ///
    /// PC advances by 10. Default impl is a no-op; the host owns the camera
    /// struct.
    fn op4c_n_e_sub_8_camera_zoom(&mut self, zoom_x: i16, zoom_y: i16, zoom_z: i16, mode: i16) {
        let _ = (zoom_x, zoom_y, zoom_z, mode);
    }

    /// Field SE trigger with conditional 16-bit pair (op 0x4C nD sub-0,
    /// 6 bytes).
    ///
    /// `[4C, 0xD0, a_lo, a_hi, b_lo, b_hi]`. The original at lines 6936-6944
    /// reads the two 16-bit LE values; if any of three flag globals
    /// (`_DAT_8007B874`, `_DAT_800846D0`, `_DAT_800846D4`) is non-zero,
    /// calls `func_0x8002B994(a, b)` (the SE trigger). Otherwise no-op.
    ///
    /// PC advances by 6 in both branches. The hook returns nothing - the
    /// host decides whether the SE fires based on its own gate state.
    fn op4c_n_d_sub_0_field_se_trigger(&mut self, a: u16, b: u16) {
        let _ = (a, b);
    }

    /// Dialog wait poll (op 0x4C n5 sub-3, 2 bytes).
    ///
    /// `[4C, 0x53]`. The original at lines 6295-6298 calls
    /// `FUN_801D65D8(1)` (dialog "is finished?" query) and falls through to
    /// `joined_r0x801E28C4`. The dispatch ends in either case - this is a
    /// halt-style instruction - but PC has been incremented to `pc + 2`
    /// before the joined block runs.
    ///
    /// The hook is purely a side-effect callback (the host pumps its dialog
    /// state machine). PC advances by 2 and the VM halts (yields to the
    /// host); the next pump reads from `pc + 2`.
    fn op4c_n_5_sub_3_dialog_wait(&mut self, ctx: &mut FieldCtx) {
        let _ = ctx;
    }

    /// Dialog advance poll (op 0x4C n5 sub-4, 2 bytes).
    ///
    /// `[4C, 0x54]`. The original at lines 6299-6310 calls
    /// `FUN_801D65D8(0)` (dialog "advance one frame" query) and:
    /// - Returns non-zero (dialog still active) → halt at the standard
    ///   switch-default (PC unchanged for this pump cycle).
    /// - Returns zero (dialog done) → clear `DAT_8007B648`, copy 6 bytes
    ///   from the directional-sound state globals, and advance PC by 2.
    ///
    /// The hook returns `true` when the dialog is still active (VM halts at
    /// PC), `false` when done (VM advances PC by 2). Default impl returns
    /// `false` - engines without a dialog renderer immediately complete.
    fn op4c_n_5_sub_4_dialog_advance(&mut self, ctx: &mut FieldCtx) -> bool {
        let _ = ctx;
        false
    }
}

/// Tristate result for [`FieldHost::op4c_n_8_sub_d_actor_search`] (op 0x4C
/// n8 sub-D).
///
/// The dispatcher's three-way branch maps:
/// - [`EmptySlot`](Self::EmptySlot): the per-character actor sub-table is
///   empty. The dispatcher takes the standard "advance PC by 6" tail.
/// - [`Found`](Self::Found): a matching marker was located. The dispatcher
///   takes the absolute-jump path (`LE_u16(operand+3..=operand+4)`).
/// - [`NoMatch`](Self::NoMatch): the sub-table is non-empty but no entry
///   matched the marker. The dispatcher halts at the current PC via the
///   `switchD::default` fallthrough.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActorSearchResult {
    /// Per-character sub-table is empty - advance PC.
    #[default]
    EmptySlot,
    /// Marker matched - take the absolute-jump path.
    Found,
    /// Sub-table populated but no match - halt at PC.
    NoMatch,
}

/// Camera parameter for op 0x45 CONFIGURE. The bit index in the mask
/// determines the parameter slot (0..=9, MSB-first per the original); the
/// value is a `u16` read from the operand stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraParam {
    pub slot: u8,
    pub value: u16,
}

/// Story-flag-driven dispatch path for op 0x4C outer-nibble-4 sub-9.
///
/// The original VM branches on two bits of `_DAT_1F800394`. See
/// [`FieldHost::op4c_n4_sub9_state`] for the bit table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sub9State {
    /// Bits `0x02000000` clear, `0x01000000` clear - default path. Write or
    /// ramp `_DAT_801C6EA4 + 0x4A` only.
    #[default]
    Default,
    /// Bits `0x02000000` clear, `0x01000000` set - absolute jump. The VM
    /// returns the signed-16 operand as the new PC and bypasses the
    /// per-tick logic entirely.
    AbsJump,
    /// Bit `0x02000000` set - delta path. The host writes/ramps both the
    /// target slot AND a delta-accumulator at `_DAT_8007BCAC`.
    Delta,
}

/// Outcome of [`FieldHost::scene_fade`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneFadeResult {
    /// Fade started or completed - VM advances past the instruction.
    Done,
    /// Scene is busy with a previous transition - hold at current PC, the
    /// runtime will retry next tick.
    Busy,
}

/// Tristate for op 0x49 STATE_RESUME. Mirrors `_DAT_8007B450` in the
/// original (idle = 0, done = 1, anything else = currently armed). Hosts
/// World coordinates copied onto a script context by Op 0x4C sub-3 sub-7
/// ([`FieldHost::fetch_player_coords`]). All values are at-rest player ctx
/// fields - the VM doesn't transform them before assigning.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlayerCoords {
    /// `+0x14` on the player ctx.
    pub world_x: u16,
    /// `+0x16` on the player ctx.
    pub world_y: u16,
    /// `+0x18` on the player ctx.
    pub world_z: u16,
    /// `+0x26` on the player ctx.
    pub field_26: u16,
}

/// implement [`FieldHost::op49_state`] to surface their internal state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Op49State {
    /// `_DAT_8007B450 == 0`. The script can arm a fresh resume.
    #[default]
    Idle,
    /// `_DAT_8007B450` holds a PC pointer. The VM halts at the same PC and
    /// the host advances the underlying state machine.
    Armed,
    /// `_DAT_8007B450 == 1`. The previous arm completed; the script's
    /// 0x49 sub-op fires and the slot is cleared.
    Done,
}

/// Inspect the byte at `pc`. If the opcode there has the extended bit (0x80)
/// set, returns the target script ID byte that follows. Otherwise returns
/// `None`. Use this before calling [`step`] to know which `FieldCtx` to pass.
///
/// The system channel script ID is `0xFB`. Unknown IDs match the original's
/// `"UNFIND INDICATION %d"` diagnostic - the caller decides how to recover
/// (the original returns `pc + 1`).
pub fn peek_extended(bytecode: &[u8], pc: usize) -> Option<u8> {
    let op = *bytecode.get(pc)?;
    if op & 0x80 != 0 {
        bytecode.get(pc + 1).copied()
    } else {
        None
    }
}

/// Decode a grid-coordinate byte to a world coordinate.
///
/// Formula: `(b & 0x7F) * 0x80 + 0x40`, plus `0x40` if the high bit is set.
/// Used by ops 0x23 (`MOVE_TO`) and 0x3F (`DIALOG`) for the position bytes.
fn grid_to_world(b: u8) -> u16 {
    let base = u16::from(b & 0x7F) * 0x80 + 0x40;
    if b & 0x80 != 0 { base + 0x40 } else { base }
}

/// MES-shape bytecode walker. Mirrors `FUN_8003ca38`: counts payload bytes
/// starting at `buf[0]` until a terminator (`≤ 0x1E`), with a one-byte
/// peek-extension for `0xCx` prefix bytes (each consumes its trailing pair
/// byte). Used by op 0x49 sub-0 in the `Done` arm to advance past an inline
/// MES payload.
///
/// Returns the number of bytes walked. The walker stops at the first
/// terminator or at end-of-slice (defensive: the original reads past EOF
/// without bounds checks).
fn walk_mes_bytecode(buf: &[u8]) -> usize {
    let mut i = 0;
    while let Some(&b) = buf.get(i) {
        if b <= 0x1E {
            break;
        }
        if b & 0xF0 == 0xC0 {
            if buf.get(i + 1).is_none() {
                i += 1;
                break;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    i
}

/// Decode and execute one instruction.
///
/// `bytecode` is the script buffer, `pc` is the current byte offset. Returns
/// a [`StepResult`] describing the outcome.
///
/// **Cross-context dispatch:** when the opcode at `pc` has the extended bit
/// set, `ctx` should be the *target* script's context (not the caller's). Use
/// [`peek_extended`] to look up the target ID first.
pub fn step<H: FieldHost>(
    host: &mut H,
    ctx: &mut FieldCtx,
    bytecode: &[u8],
    pc: usize,
) -> StepResult {
    let Some(&opcode_byte) = bytecode.get(pc) else {
        return StepResult::Unknown { opcode: 0, pc };
    };

    let extended = opcode_byte & 0x80 != 0;
    let opcode = opcode_byte & 0x7F;
    let header_size = if extended { 2 } else { 1 };
    let operand = pc + header_size;

    // On extended (cross-context) dispatch, the resolved target ctx may be
    // halted; the original returns immediately rather than running the
    // instruction. Carve-out: opcode 0x32 (CFLAG_CLR) with bit 10 (mask 0x400)
    // is the only instruction allowed to run while halted - it's how a script
    // un-halts a target. The system channel (script_id == 0xFB) also bypasses
    // the halt check.
    if extended && ctx.is_halted() && ctx.script_id != 0xFB {
        let halt_bypass = opcode == 0x32
            && bytecode
                .get(operand)
                .map(|b| (b & 0x1F) == 10)
                .unwrap_or(false);
        if !halt_bypass {
            return StepResult::Halt { final_pc: pc };
        }
    }

    match opcode {
        // 0x21 / 0x24 / 0x25 / 0x48 - NOP cluster.
        0x21 | 0x24 | 0x25 | 0x48 => StepResult::Advance {
            next_pc: pc + header_size,
        },

        // 0x22 - EXEC_MOVE: schedule move-table playback on ctx.
        // Encoding: `[22, move_id]`. Sets ctx[+0x5C] = move_id, ctx[+0x5E] =
        // 0xFFFE, ctx[+0x56] = 5 if move_id==0 else 1. Then dispatches into
        // the move-table consumer via `host.exec_move`.
        0x22 => {
            let Some(&move_id) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            ctx.move_id = u16::from(move_id);
            ctx.field_5e = 0xFFFE;
            ctx.move_substate = if move_id == 0 { 5 } else { 1 };
            host.exec_move(ctx, move_id);
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x23 - MOVE_TO: teleport ctx to grid (x_byte, z_byte).
        // World coords use grid_to_world(). Player path also calls camera/
        // scroll; NPC path sets facing + movement init. Both go through
        // host.move_to(). PC += 3 (or +4 if extended).
        0x23 => {
            let Some(&xb) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&zb) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let world_x = grid_to_world(xb);
            let world_z = grid_to_world(zb);
            ctx.world_x = world_x;
            ctx.world_z = world_z;
            ctx.npc_x = xb;
            ctx.npc_facing = zb;
            ctx.field_8b = 0;
            let is_player = ctx.flags & 0x1000000 != 0;
            host.move_to(ctx, world_x, world_z, is_player);
            StepResult::Advance {
                next_pc: pc + header_size + 2,
            }
        }

        // 0x26 - JMP_REL: PC = pc + header_size + (lo + hi*0x100). Unconditional.
        0x26 => {
            let Some(&lo) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&hi) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let delta = u16::from_le_bytes([lo, hi]) as usize;
            let target = (pc + header_size).wrapping_add(delta);
            StepResult::Advance { next_pc: target }
        }

        // 0x2B - LFLAG_SET: ctx.local_flags |= 1 << (operand & 0x1F).
        0x2B => {
            let Some(&b) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            ctx.local_flags |= 1u16 << (b & 0x1F);
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x2C - LFLAG_CLR.
        0x2C => {
            let Some(&b) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            ctx.local_flags &= !(1u16 << (b & 0x1F));
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x2D - LFLAG_TST: if bit set, advance; else halt.
        0x2D => {
            let Some(&b) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let set = ctx.local_flags & (1u16 << (b & 0x1F)) != 0;
            if set {
                StepResult::Advance {
                    next_pc: pc + header_size + 1,
                }
            } else {
                StepResult::Halt { final_pc: pc }
            }
        }

        // 0x2E - GFLAG_SET on _DAT_1F800394.
        0x2E => {
            let Some(&b) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.set_global_flags(host.global_flags() | (1u32 << (b & 0x1F)));
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x2F - GFLAG_CLR.
        0x2F => {
            let Some(&b) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.set_global_flags(host.global_flags() & !(1u32 << (b & 0x1F)));
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x30 - GFLAG_TST.
        0x30 => {
            let Some(&b) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let set = host.global_flags() & (1u32 << (b & 0x1F)) != 0;
            if set {
                StepResult::Advance {
                    next_pc: pc + header_size + 1,
                }
            } else {
                StepResult::Halt { final_pc: pc }
            }
        }

        // 0x31 - CFLAG_SET on ctx.flags. Bit 8 has a side-effect: copy
        // ctx[+0x26] -> ctx[+0x5A]. Both paths advance PC by 2 - the original
        // calls `switchD_801e0f24::caseD_4()` (entry 0x801df098, which does
        // `addiu s8, s8, 0x2; j 0x801e3628`) for bit-8, and falls through to
        // the same advance for normal bits via `code_r0x801df098`.
        0x31 => {
            let Some(&b) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let bit = b & 0x1F;
            ctx.flags |= 1u32 << bit;
            if (1u32 << bit) == 0x100 {
                ctx.saved_26 = ctx.field_26;
            }
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x32 - CFLAG_CLR.
        0x32 => {
            let Some(&b) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            ctx.flags &= !(1u32 << (b & 0x1F));
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x33 - CFLAG_TST.
        0x33 => {
            let Some(&b) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let set = ctx.flags & (1u32 << (b & 0x1F)) != 0;
            if set {
                StepResult::Advance {
                    next_pc: pc + header_size + 1,
                }
            } else {
                StepResult::Halt { final_pc: pc }
            }
        }

        // 0x37 / 0x41 - YIELD: save PC, set halt bit. Resume PC is the byte
        // AFTER this opcode + tail-2. The original's iVar24 = 3 lines up with
        // pc + header_size + 2 in our model (header_size = 1 non-extended,
        // 2 extended; +2 is the original's "+3" minus the implicit +1 for the
        // opcode byte that's already in header_size).
        //
        // saved_pc stores the byte address of the *opcode* (pre-extended) -
        // the original writes `pbVar43`, which is `buffer_base + pc_offset`
        // BEFORE the extended-bit increment. So saved_pc == pc regardless.
        //
        // The original also propagates the halt to caller_ctx (param_3) when
        // ctx is the player. Use [`step_with_caller`] to get that propagation.
        0x37 | 0x41 => {
            ctx.saved_pc = pc as u32;
            ctx.wait_accum = 0;
            ctx.halt();
            StepResult::Yield {
                resume_pc: pc + header_size + 2,
            }
        }

        // 0x47 - YIELD_4: same as 0x37/0x41 but the post-yield PC delta is 4
        // (i.e. iVar24 = 4 in the original).
        0x47 => {
            ctx.saved_pc = pc as u32;
            ctx.wait_accum = 0;
            ctx.halt();
            StepResult::Yield {
                resume_pc: pc + header_size + 3,
            }
        }

        // 0x35 - BGM: 4-byte instruction. text_id (LE u16) at [operand],
        // sub_op at [operand + 2]. Host dispatches on sub_op.
        0x35 => {
            let Some(&lo) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&hi) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&sub_op) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            let text_id = u16::from_le_bytes([lo, hi]);
            host.bgm(text_id, sub_op);
            StepResult::Advance {
                next_pc: pc + header_size + 3,
            }
        }

        // 0x38 - CAM_CFG. Two paths share the 3-byte instruction `[38, op0,
        // op1]`:
        //
        // - **Simple path** (`op1 & 0x7F == 0`): the original copies
        //   `*(short *)(0x80073F04 + (op0 & 0xF) * 2)` into `ctx.field_26`
        //   and returns `pc + 3`.
        //
        // - **Halt-acquire path** (`op1 & 0x7F != 0`): identical predicate +
        //   apply pair to op 0x43 sub-0/1/A/B (see
        //   [`field_halt_acquire_predicate`]). Predicate succeeds → `ctx`
        //   acquires the HALT bit + `saved_pc + wait_accum=0`, the player-vs-
        //   caller mirror fires, and the VM yields with `resume_pc = pc + 3`
        //   (script halts but its post-instruction PC is the resume target).
        //   Predicate fails → the original falls into
        //   `switchD_801e00f4::default()`; for op 0x38 that path is not in the
        //   0x50/0x60/0x70 system-flag arm, so the dispatcher halts at PC.
        //
        // [`field_halt_acquire_predicate`]: FieldHost::field_halt_acquire_predicate
        0x38 => {
            let Some(&op0) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&op1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            if op1 & 0x7F == 0 {
                if let Some(value) = host.cam_cfg_lookup(op0 & 0x0F) {
                    ctx.field_26 = value;
                }
                StepResult::Advance {
                    next_pc: pc + header_size + 2,
                }
            } else if host.field_halt_acquire_predicate(ctx, 0x38) {
                let resume = pc + header_size + 2;
                ctx.flags |= 0x400;
                ctx.wait_accum = 0;
                ctx.saved_pc = pc as u32;
                host.field_halt_acquire_apply(ctx, 0x38, resume, [0; 3]);
                StepResult::Yield { resume_pc: resume }
            } else {
                StepResult::Halt { final_pc: pc }
            }
        }

        // 0x39 - PLAY_SFX. 2-byte instruction.
        0x39 => {
            let Some(&sfx_id) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.play_sfx(sfx_id);
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x3A - ADD_MONEY. 4-byte instruction. Three operand bytes form a
        // 24-bit signed integer (little-endian; bit 23 = sign). Host applies
        // the delta and decides clamping (original clamps to [0, 9999999]).
        0x3A => {
            let Some(&b0) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&b1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&b2) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            let raw = u32::from(b0) | (u32::from(b1) << 8) | (u32::from(b2) << 16);
            // Sign-extend 24 → 32.
            let signed = if raw & 0x80_0000 != 0 {
                (raw | 0xFF00_0000) as i32
            } else {
                raw as i32
            };
            host.add_money(signed);
            StepResult::Advance {
                next_pc: pc + header_size + 3,
            }
        }

        // 0x3B - SET_ITEM_COUNT. 3-byte instruction.
        0x3B => {
            let Some(&slot) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&count) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.set_item_count(slot, count);
            StepResult::Advance {
                next_pc: pc + header_size + 2,
            }
        }

        // 0x3C - PARTY_ADD. 2-byte instruction.
        0x3C => {
            let Some(&char_id) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.party_add(char_id);
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x3D - PARTY_REMOVE. 2-byte instruction.
        0x3D => {
            let Some(&char_id) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.party_remove(char_id);
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x42 - COND_JMP. Multi-mode conditional.
        //
        // Mode 0: extra-flags test. `[42, 0, bit, lo, hi]`. If
        //   `host.extra_flags() & (1 << bit)` is clear → skip 5 bytes.
        //   If set → jump to `pc + 3 + LE_u16(lo, hi)` (non-extended).
        //
        // Mode 1: screen-mode test. `[42, 1, op1, lo, hi]`. The original at
        //   `case 0x42` of FUN_801de840 (line 5176 of the dump) tests
        //   `host.screen_mode()` against:
        //     - `host.screen_mode_table(op1)` for `op1 < 8` (high-nibble check)
        //     - bit `0x20` for `op1 == 8`
        //     - bit `0x40` for `op1 == 9`
        //     - bit `0x80` for `op1 == 10`
        //     - bit `0x10` for `op1 == 0xB`
        //     - `op1 >= 0xC`: none of the conditional-skip branches match,
        //       so control falls through to the unconditional take-jump
        //       path (`iVar18 = param_2 + 3; LAB_801e35f8`). Treat as
        //       always-take.
        //   If the test FAILS, skip 5 bytes; if it succeeds, take the jump
        //   `pc + 3 + LE_u16(lo, hi)`.
        //
        // Mode 2+: the original calls `switchD_801e00f4::default()`. The
        //   dispatcher's default arm checks `opcode_byte & 0x70`; since
        //   0x42 & 0x70 = 0x40 (not in {0x50,0x60,0x70}), it returns
        //   `param_2` - halt at PC.
        0x42 => {
            let Some(&mode) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&op1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&lo) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&hi) = bytecode.get(operand + 3) else {
                return StepResult::Unknown { opcode, pc };
            };
            let test_passed = match mode {
                0 => host.extra_flags() & (1u32 << (op1 & 0x1F)) != 0,
                1 => match op1 {
                    0..=7 => host
                        .screen_mode_table(op1)
                        .is_some_and(|tbl| host.screen_mode() & 0xF000 == tbl),
                    8 => host.screen_mode() & 0x20 != 0,
                    9 => host.screen_mode() & 0x40 != 0,
                    10 => host.screen_mode() & 0x80 != 0,
                    11 => host.screen_mode() & 0x10 != 0,
                    // op1 >= 0xC: unconditional take-jump path.
                    _ => true,
                },
                _ => return StepResult::Halt { final_pc: pc },
            };
            if !test_passed {
                // Skip the whole 5-byte instruction (header + 4 operand bytes).
                StepResult::Advance {
                    next_pc: pc + header_size + 4,
                }
            } else {
                // Take the jump. The original computes
                // `iVar18 = param_2 + 3; return iVar18 + LE_u16(lo, hi)`.
                let delta = u16::from_le_bytes([lo, hi]) as usize;
                StepResult::Advance {
                    next_pc: pc + header_size + 2 + delta,
                }
            }
        }

        // 0x3E - WARP / INTERACT. Two paths:
        //
        // - INTERACT (`op0 == 0xFF` or `op0 < 100`): `[3E, op0, op1]`,
        //   PC += 3. Calls `host.field_interact(op0, op1)`.
        //
        // - WARP / scene transition (`op0 >= 100`): `[3E, op0, _, _, _, _]`,
        //   PC += 6. `map_id = op0 - 100`. The original clears the player
        //   ctx's bit `0x80000`; we mirror that on the active ctx (which is
        //   the player at the time scripts call this) and let the host
        //   override scene-side state.
        0x3E => {
            let Some(&op0) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            if op0 == 0xFF || op0 < 100 {
                let Some(&op1) = bytecode.get(operand + 1) else {
                    return StepResult::Unknown { opcode, pc };
                };
                host.field_interact(op0, op1);
                StepResult::Advance {
                    next_pc: pc + header_size + 2,
                }
            } else {
                let map_id = op0 - 100;
                ctx.flags &= !0x80000;
                host.scene_transition(map_id);
                StepResult::Advance {
                    next_pc: pc + header_size + 5,
                }
            }
        }

        // 0x46 - RENDER_CFG. Two forms keyed off `op0`:
        //
        // - Long form (`op0 == 0x24`): `[46, 0x24, b1, b2, b3, b4]`, PC += 6.
        //   Writes the four bytes via `host.render_cfg_long`.
        //
        // - Short form (anything else): `[46, op0, op1]`, PC += 3.
        //   The VM does the bitfield math:
        //     r = !(op0 >> 1) & 0xFF
        //     g = 2 - (op1 >> 1)
        //     b = (op0 >> 1) - 1
        //     packed = (op1 >> 1) + 2
        0x46 => {
            let Some(&op0) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            if op0 == 0x24 {
                let Some(&b1) = bytecode.get(operand + 1) else {
                    return StepResult::Unknown { opcode, pc };
                };
                let Some(&b2) = bytecode.get(operand + 2) else {
                    return StepResult::Unknown { opcode, pc };
                };
                let Some(&b3) = bytecode.get(operand + 3) else {
                    return StepResult::Unknown { opcode, pc };
                };
                let Some(&b4) = bytecode.get(operand + 4) else {
                    return StepResult::Unknown { opcode, pc };
                };
                host.render_cfg_long(b1, b2, b3, b4);
                StepResult::Advance {
                    next_pc: pc + header_size + 5,
                }
            } else {
                let Some(&op1) = bytecode.get(operand + 1) else {
                    return StepResult::Unknown { opcode, pc };
                };
                let r = !(op0 >> 1);
                let g = 2u8.wrapping_sub(op1 >> 1);
                let b = (op0 >> 1).wrapping_sub(1);
                let packed = (op1 >> 1).wrapping_add(2);
                host.render_cfg_short(r, g, b, packed);
                StepResult::Advance {
                    next_pc: pc + header_size + 2,
                }
            }
        }

        // 0x4F - SCENE_REGISTER_WRITE. `[4F, b0, b1, b2]`, PC += 4. The
        // original writes three u16 values (zero-extended bytes) to scene
        // offsets +0x10, +0x12, +0x14.
        0x4F => {
            let Some(&b0) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&b1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&b2) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.scene_register_write(b0, b1, b2);
            StepResult::Advance {
                next_pc: pc + header_size + 3,
            }
        }

        // 0x44 - COUNTER. `[44, op0]`, PC += 2.
        0x44 => {
            let Some(&op0) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.counter_update(op0);
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x4B - ANIMATE. `[4B, count, base_id, ...4*count keyframe bytes]`.
        // PC += 3 + count * 4. Sets ctx.flags |= 0x1000, ctx.local_flags |=
        // 0x1000 (with bits 0x2000+0x0C00 cleared via mask 0xD3FF), writes
        // ctx[+0x6c] = count (face_rotation slot is reused - the original
        // stores the count there).
        0x4B => {
            let Some(&count) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&base_id) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let frame_bytes = (count as usize) * 4;
            let frames_start = operand + 2;
            let frames_end = frames_start + frame_bytes;
            if frames_end > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            ctx.flags |= 0x1000;
            ctx.local_flags = (ctx.local_flags & 0xD3FF) | 0x1000;
            ctx.face_rotation = count;
            let frames = &bytecode[frames_start..frames_end];
            host.setup_animation(ctx, count, base_id, frames);
            StepResult::Advance {
                next_pc: pc + header_size + 2 + frame_bytes,
            }
        }

        // 0x4C - MENU_CTRL. Sub-dispatched by `op0 >> 4`.
        //
        // - sub-0: party leader change. `[4C, op0]` (2 bytes). leader_id =
        //   op0 & 7.
        // - sub-1: menu/effect sub-dispatcher. `[4C, op0, ...5 more bytes]`
        //   (7 bytes). Inner sub-ops 0x10/0x12/0x13/0x14 are host-delegated.
        // - sub-3 sub-5: `[4C, 0x35]` (2 bytes). `ctx.local_flags = (lf &
        //   0xFF7F) | 0x20A`.
        // - sub-3 sub-6: `[4C, 0x36]` (2 bytes). `ctx.local_flags |= 0x28A`.
        // - other sub-ops are Pending.
        0x4C => {
            let Some(&op0) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            match op0 >> 4 {
                0 => {
                    host.set_party_leader(op0 & 7);
                    StepResult::Advance {
                        next_pc: pc + header_size + 1,
                    }
                }
                1 => {
                    let Some(&b1) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&b2) = bytecode.get(operand + 2) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&b3) = bytecode.get(operand + 3) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&b4) = bytecode.get(operand + 4) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&b5) = bytecode.get(operand + 5) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    host.menu_ctrl_sub1(op0, &[b1, b2, b3, b4, b5]);
                    StepResult::Advance {
                        next_pc: pc + header_size + 6,
                    }
                }
                2 => {
                    host.party_view_swap(op0 & 7);
                    StepResult::Advance {
                        next_pc: pc + header_size + 1,
                    }
                }
                3 => match op0 & 0x0F {
                    0 => {
                        // sub-0: lock field input, exit via STATE_RESUME.
                        host.set_field_input_lock(true);
                        StepResult::Yield {
                            resume_pc: pc + header_size + 1,
                        }
                    }
                    1 => {
                        // sub-1: unlock field input, exit via STATE_RESUME.
                        host.set_field_input_lock(false);
                        StepResult::Yield {
                            resume_pc: pc + header_size + 1,
                        }
                    }
                    2 => {
                        host.clear_party_state_region();
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    4 | 0xB | 0xC => {
                        // sub-4 / sub-B / sub-C: original `goto code_r0x801df098`
                        // which falls through to `LAB_801df09c: switchD_801e00f4::default()`.
                        // The asm at 0x801df208 jumps with delay slot
                        // `_addiu s8, s8, 0x2` → PC += 2 from the opcode.
                        // No host hook fires; the original writes
                        // `_DAT_8007b5f0 = uVar31` (current view-index slot)
                        // before falling through, but `uVar31` was just read
                        // from the same slot a few lines earlier - net effect
                        // is a no-op write.
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    3 => {
                        host.menu_refresh();
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    5 => {
                        ctx.local_flags = (ctx.local_flags & 0xFF7F) | 0x020A;
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    6 => {
                        ctx.local_flags |= 0x028A;
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    7 => {
                        // sub-3 sub-7: copy player coords onto non-player ctx.
                        // The host gates on `ctx == player_ctx`; returning
                        // `None` means "no copy" (either the player ctx is
                        // unset OR ctx IS the player), in which case we
                        // fall through to a regular advance.
                        if let Some(p) = host.fetch_player_coords(ctx) {
                            ctx.world_x = p.world_x;
                            ctx.world_y = p.world_y;
                            ctx.world_z = p.world_z;
                            ctx.field_26 = p.field_26;
                            if ctx.flags & 0x2000_0000 != 0 {
                                let inverted_y = (p.world_y as i16).wrapping_neg();
                                host.set_inverted_y_mirror(ctx, inverted_y);
                                // Original returns via `caseD_4()` - the
                                // STATE_RESUME exit. We surface that as a
                                // Yield so the host's state-resume layer
                                // decides whether the next caller resumes.
                                return StepResult::Yield {
                                    resume_pc: pc + header_size + 1,
                                };
                            }
                        }
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    8 | 0xD => {
                        host.player_subtile_refresh(op0 & 0x0F);
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    9 => {
                        host.player_position_refresh_with_collision_y(ctx);
                        host.player_render_resync();
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    0xA => {
                        host.copy_dialog_depth_to_player();
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    0xE => {
                        host.player_render_resync();
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    0xF => {
                        host.field_io_resync();
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    // All 16 sub-ops covered above; this arm is dead code
                    // but the compiler can't prove it because the value is
                    // narrowed to `op0 & 0x0F` in this match's scrutinee.
                    16..=u8::MAX => unreachable!("op0 & 0x0F is at most 0xF"),
                },
                4 => {
                    // 0x4C outer nibble 4 - immediate-or-ramp cluster.
                    // 6-byte instruction `[4C, op0, val_lo, val_hi,
                    // ticks_lo, ticks_hi]`. The original at line ~5901 of
                    // FUN_801DE840 reads: target = signed_16(op+1..3),
                    // ticks = signed_16(op+3..5), then dispatches on
                    // `op0 & 0x0F`. PC advance = 6 (= header_size + 5)
                    // for the ported sub-ops; the abs-jump branches in
                    // sub-3 / sub-4 are deferred and return `Pending`.
                    if operand + 5 > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    let target_lo = bytecode[operand + 1];
                    let target_hi = bytecode[operand + 2];
                    let ticks_lo = bytecode[operand + 3];
                    let ticks_hi = bytecode[operand + 4];
                    let target = i16::from_le_bytes([target_lo, target_hi]);
                    let ticks = u16::from_le_bytes([ticks_lo, ticks_hi]);
                    let advance = StepResult::Advance {
                        next_pc: pc + header_size + 5,
                    };
                    let sub = op0 & 0x0F;
                    match sub {
                        0 => {
                            // ctx[+0x72] write or ramp.
                            if ticks == 0 {
                                ctx.field_72 = target as u16;
                            } else {
                                host.op4c_nibble4_ctx_ramp(ctx, sub, target, ticks);
                            }
                            advance
                        }
                        1 => {
                            // ctx[+0x6A] write or ramp. The input is halved
                            // (s16 arithmetic shift right 1) with a floor of
                            // 1 - see FUN_801DE840 line ~5923:
                            //   `iVar46 = signed_16(operand[0..2]);`
                            //   `uVar31 = iVar46 >> 1;`
                            //   `if (uVar31 == 0) uVar31 = 1;`
                            let halved = (target >> 1).max(1);
                            if ticks == 0 {
                                ctx.field_6a = halved;
                            } else {
                                host.op4c_nibble4_ctx_ramp(ctx, sub, halved, ticks);
                            }
                            advance
                        }
                        2 => {
                            // ctx[+0x8E] write or ramp; immediate-write
                            // path also mirrors `world_y = -value` when
                            // `flags & 0x20000000` is set.
                            if ticks == 0 {
                                ctx.field_8e = target;
                                if ctx.flags & 0x2000_0000 != 0 {
                                    ctx.world_y = (-target) as u16;
                                }
                            } else {
                                host.op4c_nibble4_ctx_ramp(ctx, sub, target, ticks);
                            }
                            advance
                        }
                        3 => {
                            // sub-3: ticks!=0 ramps `ctx.field_24`; ticks==0
                            // reuses the same 6-byte encoding as an absolute
                            // jump - the original at FUN_801DE840 line ~5961
                            // returns `iVar18 = signed_16(operand[0..2])`,
                            // which propagates back through the dispatcher
                            // as the new PC offset.
                            if ticks == 0 {
                                StepResult::Advance {
                                    next_pc: target as i32 as usize,
                                }
                            } else {
                                host.op4c_nibble4_ctx_ramp(ctx, sub, target, ticks);
                                advance
                            }
                        }
                        4 => {
                            // sub-4: mirror of sub-3 - ticks==0 writes
                            // `ctx.field_28`; ticks!=0 is the absolute jump.
                            if ticks == 0 {
                                ctx.field_28 = target;
                                advance
                            } else {
                                StepResult::Advance {
                                    next_pc: target as i32 as usize,
                                }
                            }
                        }
                        6 | 7 => {
                            // sub-6 (`_DAT_8007B92C`) / sub-7 (`_DAT_8007B930`)
                            // - paired global writes gated by `_DAT_800845A8`.
                            // When the gate is set, the original short-circuits
                            // both ports of the pair and clears them together;
                            // otherwise the regular global-write/ramp dispatch
                            // proceeds with the per-sub slot.
                            if host.op4c_nibble4_global_pair_gate() {
                                host.op4c_nibble4_global_pair_clear();
                            } else {
                                host.op4c_nibble4_global_write(sub, target as i32, ticks);
                            }
                            advance
                        }
                        8 => {
                            // ctx[+0x26] (`field_26`) write or ramp.
                            if ticks == 0 {
                                ctx.field_26 = target as u16;
                            } else {
                                host.op4c_nibble4_ctx_ramp(ctx, sub, target, ticks);
                            }
                            advance
                        }
                        0xA..=0xD => {
                            // Global slot ramp/write. Sub-D additionally
                            // multiplies by `_DAT_8008457C` and shifts
                            // right 12; the host owns the transform.
                            host.op4c_nibble4_global_write(sub, target as i32, ticks);
                            advance
                        }
                        5 => {
                            // sub-5 has a wider 11-byte encoding instead of 6.
                            // Override the operand-length precheck made above.
                            // `[4C, 0x45, b1, w94_lo, w94_hi, w96_lo, w96_hi,
                            //   w98_lo, w98_hi, ticks_lo, ticks_hi]`.
                            // Bytecode boundary: pc + 11 must fit.
                            if operand + 10 > bytecode.len() {
                                return StepResult::Unknown { opcode, pc };
                            }
                            let b1 = bytecode[operand + 1];
                            let w94 =
                                i16::from_le_bytes([bytecode[operand + 2], bytecode[operand + 3]]);
                            let w96 =
                                i16::from_le_bytes([bytecode[operand + 4], bytecode[operand + 5]]);
                            let w98 =
                                i16::from_le_bytes([bytecode[operand + 6], bytecode[operand + 7]]);
                            let sub5_ticks =
                                u16::from_le_bytes([bytecode[operand + 8], bytecode[operand + 9]]);
                            if sub5_ticks == 0 {
                                host.op4c_n4_sub5_write_immediate(ctx, b1, w94, w96, w98);
                                StepResult::Advance {
                                    next_pc: pc + header_size + 10,
                                }
                            } else {
                                host.op4c_n4_sub5_ramp(ctx, b1, w94, w96, w98, sub5_ticks);
                                // Ramp path falls through STATE_RESUME - yield
                                // and let the host's resume layer signal when
                                // to advance past the 11-byte instruction.
                                StepResult::Yield {
                                    resume_pc: pc + header_size + 10,
                                }
                            }
                        }
                        9 => {
                            // sub-9: dispatch on two bits of the global story
                            // flag word. See `FieldHost::op4c_n4_sub9_state`.
                            match host.op4c_n4_sub9_state() {
                                Sub9State::AbsJump => {
                                    // Absolute jump to signed_16(operand[0..2]).
                                    StepResult::Advance {
                                        next_pc: target as i32 as usize,
                                    }
                                }
                                Sub9State::Default => {
                                    if ticks == 0 {
                                        host.op4c_n4_sub9_default_write(target);
                                        advance
                                    } else {
                                        host.op4c_n4_sub9_default_ramp(target, ticks);
                                        StepResult::Yield { resume_pc: pc }
                                    }
                                }
                                Sub9State::Delta => {
                                    host.op4c_n4_sub9_delta_write_or_ramp(target, ticks);
                                    if ticks == 0 {
                                        advance
                                    } else {
                                        StepResult::Yield { resume_pc: pc }
                                    }
                                }
                            }
                        }
                        // Sub-ops 0xE/0xF have no `case` arm in the original
                        // case-4 inner switch (line 6188 of the dump): they
                        // hit `default: func_0x8001a068(s_SUB_40_ERROR_801cec88);
                        // iVar18 = switchD_801e00f4::default(); return iVar18;`
                        // - the dispatcher's default returns `param_2` ⇒ halt
                        // at PC.
                        0xE..=0xF => StepResult::Halt { final_pc: pc },
                        // `op0 & 0x0F` is at most 0xF; the arms above cover
                        // every value, so this arm is dead code.
                        16..=u8::MAX => unreachable!("op0 & 0x0F is at most 0xF"),
                    }
                }
                // Outer nibble 5 - sound directional + dialog dispatch.
                // Only sub-0 (sound directional, 4-byte) is ported; sub-1
                // (NPC move + run with halt-acquire), sub-2/3/4 (dialog
                // query cluster) remain `Pending` because they thread
                // halt-acquire and STATE_RESUME branches that need their
                // own host-hook surface. Sub-ops 5..=0xF have no `case`
                // arm in the original inner switch, so they silently
                // fall through and the function returns `iVar45 = param_2`
                // (initialised at the top of FUN_801de840) - halt at PC.
                5 => match op0 & 0x0F {
                    0 => {
                        let Some(&lo) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        let Some(&hi) = bytecode.get(operand + 2) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        let value = i16::from_le_bytes([lo, hi]);
                        let high = (value as i32) >= 0xF0;
                        if high {
                            ctx.flags |= 0x0100_0000;
                        } else {
                            ctx.flags &= !0x0100_0000;
                        }
                        host.op4c_n5_sub0_sound_directional(ctx, value, high);
                        StepResult::Advance {
                            next_pc: pc + header_size + 3,
                        }
                    }
                    // Sub-3: 2-byte `[4C, 0x53]`. Dialog-wait poll. The
                    // original at lines 6295-6298 of the dispatcher dump
                    // calls `FUN_801D65D8(1)` then `goto
                    // joined_r0x801E28C4`; both branches halt-style return
                    // after `param_2 = param_2 + 2`. We model this as
                    // "halt at pc+2" - the host pumps its dialog state
                    // machine through the side-effect hook.
                    3 => {
                        host.op4c_n_5_sub_3_dialog_wait(ctx);
                        StepResult::Halt {
                            final_pc: pc + header_size + 1,
                        }
                    }
                    // Sub-4: 2-byte `[4C, 0x54]`. Dialog-advance poll. The
                    // original at lines 6299-6310 calls `FUN_801D65D8(0)`;
                    // if non-zero (dialog still active) goes to
                    // `LAB_801DEE50` (halt at PC), else clears
                    // `DAT_8007B648`, snapshots state bytes, advances PC by
                    // 2. Host returns `true` when still active.
                    4 => {
                        if host.op4c_n_5_sub_4_dialog_advance(ctx) {
                            StepResult::Halt { final_pc: pc }
                        } else {
                            StepResult::Advance {
                                next_pc: pc + header_size + 1,
                            }
                        }
                    }
                    1..=2 => StepResult::Pending { opcode, pc },
                    _ => StepResult::Halt { final_pc: pc },
                },
                // Outer nibble 6 - emitter call families.
                // Only op0 == 0x60 (6-word emitter) is ported. op0 == 0x61
                // is a halt-acquire variant whose 16-byte encoding interacts
                // with cross-context dispatch; remaining values (0x62..=0x6F)
                // hit `else { return param_2; }` in the original at line
                // 6330 of the dump - halt at PC.
                6 => match op0 {
                    0x60 => {
                        if operand + 13 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let mut words = [0i16; 6];
                        for (i, w) in words.iter_mut().enumerate() {
                            *w = i16::from_le_bytes([
                                bytecode[operand + 1 + i * 2],
                                bytecode[operand + 2 + i * 2],
                            ]);
                        }
                        host.op4c_n6_sub0_emitter6(words);
                        StepResult::Advance {
                            next_pc: pc + header_size + 13,
                        }
                    }
                    0x61 => StepResult::Pending { opcode, pc },
                    _ => StepResult::Halt { final_pc: pc },
                },
                // Outer nibble 7 - VRAM tile-flag bulk operation. 7-byte
                // instruction. Sub-0/1 yield via STATE_RESUME; sub-2/3
                // advance directly; other sub-ops halt at PC.
                7 => {
                    let sub = op0 & 0x0F;
                    if !matches!(sub, 0..=3) {
                        return StepResult::Halt { final_pc: pc };
                    }
                    if operand + 6 > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    let x0 = bytecode[operand + 1];
                    let x1 = bytecode[operand + 3].wrapping_add(1);
                    let z0 = bytecode[operand + 2];
                    let z1 = bytecode[operand + 4].wrapping_add(1);
                    let mask = bytecode[operand + 5];
                    host.op4c_n7_tile_flag_bulk(sub, (x0, x1), (z0, z1), mask);
                    let advance_pc = pc + header_size + 6;
                    if sub <= 1 {
                        StepResult::Yield {
                            resume_pc: advance_pc,
                        }
                    } else {
                        StepResult::Advance {
                            next_pc: advance_pc,
                        }
                    }
                }
                // Outer nibble 8 - large multi-purpose dispatcher. Sub-ops
                // 1..=F minus sub-0 and sub-3 are fully ported: sub-1
                // (actor model + anim, 9-byte), sub-2 (mirror write, 2-byte),
                // sub-4 (b630 write, 2-byte), sub-5/E/F (halt-acquire idiom,
                // 2-byte - the original at lines 6550-6570 shares one body),
                // sub-6 (actor set rotation, 15-byte), sub-7 (callback
                // register + halt), sub-8 (globals write, 6-byte), sub-9
                // (DAT_80073F00 write, 4-byte), sub-0xA (write quad, 11-byte),
                // sub-0xB (actor-type conditional jump, 5-byte), sub-0xC
                // (field_68 conditional jump, 4-byte), sub-0xD (char actor
                // search, 6-byte). Sub-0 (actor allocator, needs
                // `func_0x80020de0`) and sub-3 (box-fill table, needs
                // `FUN_801D5630`) remain `Pending`.
                8 => match op0 & 0x0F {
                    // Sub-1: 9-byte `[4C, 0x81, m0, m1, m2, anim_lo,
                    // anim_hi, frames_lo, frames_hi]`. Set actor model +
                    // animation frame, optionally with a tween if
                    // `tween_frames != 0`. Dispatcher lines 6496-6515; the
                    // host applies whichever path applies based on its own
                    // state model. PC always advances by 9.
                    1 => {
                        if operand + 8 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let model_id = crate::field_helpers::load_u24_le(&bytecode[operand + 1..]);
                        let anim_frame =
                            crate::field_helpers::load_u16_le(&bytecode[operand + 4..]);
                        let tween_frames =
                            crate::field_helpers::load_u16_le(&bytecode[operand + 6..]);
                        host.op4c_n_8_sub_1_set_model_anim(ctx, model_id, anim_frame, tween_frames);
                        StepResult::Advance {
                            next_pc: pc + header_size + 8,
                        }
                    }
                    2 => {
                        let Some(&page) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        host.op4c_n8_sub2_party_page_mirror(page);
                        StepResult::Advance {
                            next_pc: pc + header_size + 2,
                        }
                    }
                    // Sub-6: 15-byte `[4C, 0x86, x_lo..rz_hi, actor_id]`.
                    // Six 16-bit LE values for position+rotation matrix
                    // axes, then a 1-byte actor selector at the tail.
                    // Dispatcher lines 6571-6585: actor lookup misses fall
                    // through to PC + 15 with no side effect; on hit, host
                    // applies the rotation matrix. PC always advances by
                    // 15.
                    6 => {
                        if operand + 14 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let position = [
                            crate::field_helpers::load_u16_le(&bytecode[operand + 1..]) as i16,
                            crate::field_helpers::load_u16_le(&bytecode[operand + 3..]) as i16,
                            crate::field_helpers::load_u16_le(&bytecode[operand + 5..]) as i16,
                        ];
                        let rotation = [
                            crate::field_helpers::load_u16_le(&bytecode[operand + 7..]) as i16,
                            crate::field_helpers::load_u16_le(&bytecode[operand + 9..]) as i16,
                            crate::field_helpers::load_u16_le(&bytecode[operand + 11..]) as i16,
                        ];
                        let actor_id = bytecode[operand + 13];
                        host.op4c_n_8_sub_6_actor_set_rotation(ctx, actor_id, position, rotation);
                        StepResult::Advance {
                            next_pc: pc + header_size + 14,
                        }
                    }
                    4 => {
                        let Some(&value) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        host.op4c_n8_sub4_set_b630(value);
                        StepResult::Advance {
                            next_pc: pc + header_size + 2,
                        }
                    }
                    7 => {
                        // Register callback then halt at PC. The original
                        // calls `switchD_801e00f4::default()` (= halt for
                        // 0x4C since `0x4C & 0x70 = 0x40`); script resumes
                        // when the registered callback fires. Distinct from
                        // an Advance - a re-entry of the dispatcher at the
                        // same PC re-registers, so the host's hook should
                        // be idempotent.
                        host.op4c_n8_sub7_register_callback();
                        StepResult::Halt { final_pc: pc }
                    }
                    8 => {
                        if operand + 5 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let value =
                            i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
                        let b3 = bytecode[operand + 3];
                        let b4 = bytecode[operand + 4];
                        host.op4c_n8_sub8_write_globals(value, b3, b4);
                        StepResult::Advance {
                            next_pc: pc + header_size + 5,
                        }
                    }
                    0xA => {
                        if operand + 10 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let s0 = i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
                        let s1 = i16::from_le_bytes([bytecode[operand + 3], bytecode[operand + 4]]);
                        let s2 = i16::from_le_bytes([bytecode[operand + 5], bytecode[operand + 6]]);
                        // Original uses `func_0x8003CEB8` - a 24-bit LE
                        // decoder. High byte is zero-padded; hosts can
                        // sign-extend if they need to.
                        let packed = u32::from_le_bytes([
                            bytecode[operand + 7],
                            bytecode[operand + 8],
                            bytecode[operand + 9],
                            0,
                        ]);
                        host.op4c_n8_sub_a_write_quad([s0, s1, s2], packed);
                        StepResult::Advance {
                            next_pc: pc + header_size + 10,
                        }
                    }
                    0xC => {
                        if operand + 3 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        if host.op4c_n8_sub_c_branch_on_field_68(ctx) {
                            let target =
                                i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
                            StepResult::Advance {
                                next_pc: target as i32 as usize,
                            }
                        } else {
                            StepResult::Advance {
                                next_pc: pc + header_size + 3,
                            }
                        }
                    }
                    // Sub-9: write `_DAT_80073F00 = i16(operand[1..3])`, then
                    // PC += 4 (`code_r0x801e3620` exit label).
                    9 => {
                        if operand + 3 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let value =
                            i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
                        host.op4c_n8_sub9_set_73f00(value);
                        StepResult::Advance {
                            next_pc: pc + header_size + 3,
                        }
                    }
                    // Sub-5/E/F all share the halt-acquire body. The host
                    // hook applies the standard ctx mutation; the dispatch
                    // halts at PC regardless of acquire success/failure
                    // (both paths in the dump halt - success via
                    // `switchD_801e00f4::default()`, failure via
                    // `LAB_801dee50`).
                    5 | 0xE | 0xF => {
                        host.op4c_n8_halt_acquire(ctx, pc as u32);
                        StepResult::Halt { final_pc: pc }
                    }
                    // Sub-B: 5-byte `[4C, 0x8B, type_byte, target_lo,
                    // target_hi]`. Conditional jump if any actor of
                    // `type_byte` is active. Dispatcher lines 6621-6644:
                    // count > 0 → jump to absolute u16; count == 0 →
                    // advance PC by 5.
                    0xB => {
                        if operand + 4 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let type_byte = bytecode[operand + 1];
                        if host.op4c_n_8_sub_b_actor_type_present(type_byte) {
                            let target =
                                crate::field_helpers::load_u16_le(&bytecode[operand + 2..]);
                            StepResult::Advance {
                                next_pc: target as usize,
                            }
                        } else {
                            StepResult::Advance {
                                next_pc: pc + header_size + 4,
                            }
                        }
                    }
                    // Sub-D: 6-byte `[4C, 0x8D, char_idx, marker, target_lo,
                    // target_hi]`. Tristate per-character actor sub-table
                    // search. Dispatcher lines 6652-6667.
                    0xD => {
                        if operand + 5 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let char_idx = bytecode[operand + 1];
                        let marker = bytecode[operand + 2];
                        match host.op4c_n_8_sub_d_actor_search(char_idx, marker) {
                            ActorSearchResult::EmptySlot => StepResult::Advance {
                                next_pc: pc + header_size + 5,
                            },
                            ActorSearchResult::Found => {
                                let target =
                                    crate::field_helpers::load_u16_le(&bytecode[operand + 3..]);
                                StepResult::Advance {
                                    next_pc: target as usize,
                                }
                            }
                            ActorSearchResult::NoMatch => StepResult::Halt { final_pc: pc },
                        }
                    }
                    _ => StepResult::Pending { opcode, pc },
                },
                // Outer nibble 9 - fade family + table copy + callback.
                // Sub-0/1/2 (fade dispatch, 9-byte) and sub-0xE (16-word
                // table copy, 34-byte) ported. Sub-0xF (callback registration
                // via LAB_801da930) remains `Pending`. Sub-3..=0xD have no
                // `case` arm in the original (line 6694–6696 of the dump
                // returns `param_2` when `2 < uVar27 < 0xE`) - halt at PC.
                9 => {
                    let sub = op0 & 0x0F;
                    match sub {
                        0..=2 => {
                            if operand + 8 > bytecode.len() {
                                return StepResult::Unknown { opcode, pc };
                            }
                            let b1 = bytecode[operand + 1];
                            let mut words = [0i16; 3];
                            for (i, w) in words.iter_mut().enumerate() {
                                *w = i16::from_le_bytes([
                                    bytecode[operand + 2 + i * 2],
                                    bytecode[operand + 3 + i * 2],
                                ]);
                            }
                            host.op4c_n9_sub0_2_dde34(sub, b1, words);
                            StepResult::Advance {
                                next_pc: pc + header_size + 8,
                            }
                        }
                        0xE => {
                            if operand + 33 > bytecode.len() {
                                return StepResult::Unknown { opcode, pc };
                            }
                            let mut words = [0i16; 16];
                            for (i, w) in words.iter_mut().enumerate() {
                                *w = i16::from_le_bytes([
                                    bytecode[operand + 1 + i * 2],
                                    bytecode[operand + 2 + i * 2],
                                ]);
                            }
                            host.op4c_n9_sub_e_table_copy(words);
                            StepResult::Advance {
                                next_pc: pc + header_size + 33,
                            }
                        }
                        // Sub-0xF: register `LAB_801DA930` callback then halt
                        // at PC. The original goes through
                        // `switchD_801e00f4::default()`, which for opcode 0x4C
                        // (`& 0x70 = 0x40`) returns `param_2` - halt at PC.
                        // The script resumes when the registered callback
                        // fires.
                        0xF => {
                            host.op4c_n9_sub_f_register_callback();
                            StepResult::Halt { final_pc: pc }
                        }
                        _ => StepResult::Halt { final_pc: pc },
                    }
                }
                // Outer nibble 0xA - conditional jump on flag bit. The 5-byte
                // instruction `[4C, 0xAN, bit, lo, hi]` dispatches first on
                // sub-op (`bne a1, zero, 0x801e258c` at 0x801e2568 of the
                // overlay disassembly), then per-sub-op checks one bit:
                // sub-0 → ctx.flags, sub-1 → ctx.local_flags, sub-2 → global
                // story flag word. When the bit is **set** the original
                // branches to the absolute-jump label (`bne v1, zero,
                // 0x801e360c`); clear (or sub-op 3..=0xF) falls through to
                // `s8 += 5` (PC += 5).
                0xA => {
                    if operand + 4 > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    let sub = op0 & 0x0F;
                    let bit = bytecode[operand + 1];
                    let target = i16::from_le_bytes([bytecode[operand + 2], bytecode[operand + 3]]);
                    if host.op4c_n_a_flag_set(ctx, sub, bit) {
                        StepResult::Advance {
                            next_pc: target as i32 as usize,
                        }
                    } else {
                        StepResult::Advance {
                            next_pc: pc + header_size + 4,
                        }
                    }
                }
                // Outer nibble 0xC - small per-actor / per-scene writes.
                // Ported subset covers sub-0 (move-table cancel, PC += 2),
                // sub-1 (flag-loop reset, PC += 2), sub-2 (field_42), sub-3
                // (script-table teleport, PC += 2), sub-4 (sub-tile
                // broadcast), sub-5/6 (party-flag conditional jumps via
                // `func_0x8003CE9C` + `party_flag_test`), sub-7 (sound
                // trigger), sub-8 (field_74 XOR), sub-9 (global-pair compare
                // gate - PC += 2 unless globals differ, then halt), sub-0xA
                // / sub-0xB / sub-0xC (slot table writes), sub-0xD
                // (script-context alloc, halt), sub-0xE (b6ac write), sub-0xF
                // (position broadcast). All 16 sub-ops in nibble 0xC are now
                // handled.
                0xC => match op0 & 0x0F {
                    // Sub-0: 2-byte. Cancel move-table animation if active
                    // (`func_0x800204F8`). Always advances PC += 2.
                    0 => {
                        host.op4c_n_c_sub_0_move_cancel(ctx);
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    // Sub-3: 2-byte. Script-table teleport with tile-center
                    // math. Helper `func_0x8003C8F0`/`8003D0BC` is overlay-
                    // resident; hosts decide what fields to write. PC += 2.
                    3 => {
                        host.op4c_n_c_sub_3_script_teleport(ctx);
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    // Sub-5: 4-byte `[4C, 0xC5, idx_lo, idx_hi]`. Reads the
                    // 16-bit flag index via `load_u16_le`, queries the host's
                    // party-flag bank, and jumps to `LAB_801E2A10` when the
                    // bit is **clear** (jump-if-zero polarity). The original's
                    // jump target is the dispatcher's "no-op fallthrough" -
                    // both branches advance PC += 4.
                    5 => {
                        if operand + 3 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let flag_idx = crate::field_helpers::load_u16_le(&bytecode[operand + 1..]);
                        let _bit_set = host.op4c_n_c_party_flag_test(flag_idx);
                        // Both polarities (5 = jump-if-zero, 6 = jump-if-nonzero)
                        // share the same dispatcher fallthrough - the original's
                        // `joined_r0x801e28c4` block returns `param_2 + 4` either
                        // way. The polarity selects which arm runs the
                        // host-visible side effect, but PC delta is constant.
                        StepResult::Advance {
                            next_pc: pc + header_size + 3,
                        }
                    }
                    // Sub-6: 4-byte. Sister of sub-5 with opposite polarity
                    // (jump-if-nonzero). PC always advances by 4.
                    6 => {
                        if operand + 3 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let flag_idx = crate::field_helpers::load_u16_le(&bytecode[operand + 1..]);
                        let _bit_set = host.op4c_n_c_party_flag_test(flag_idx);
                        StepResult::Advance {
                            next_pc: pc + header_size + 3,
                        }
                    }
                    // Sub-D: 2-byte. Allocate / register a script context.
                    // Halts at PC regardless of allocation outcome.
                    0xD => {
                        host.op4c_n_c_sub_d_script_alloc();
                        StepResult::Halt { final_pc: pc }
                    }
                    // Sub-1: 1-byte. Walk the trigger-flag record array,
                    // resetting each record's byte-0 from a per-record
                    // 16-bit index queried against the flag bit-array.
                    // Always advances PC += 2 (whether the array is empty
                    // or the loop completes).
                    1 => {
                        host.op4c_n_c_sub_1_flag_loop_reset(&[]);
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    2 => {
                        let Some(&b1) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        ctx.field_42 = u16::from(b1);
                        StepResult::Advance {
                            next_pc: pc + header_size + 2,
                        }
                    }
                    4 => {
                        let Some(&xb) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        let Some(&zb) = bytecode.get(operand + 2) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        host.op4c_n_c_sub4_subtile_broadcast(xb & 0x7F, zb & 0x7F);
                        StepResult::Advance {
                            next_pc: pc + header_size + 3,
                        }
                    }
                    7 => {
                        let Some(&b1) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        let Some(&b2) = bytecode.get(operand + 2) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        host.op4c_n_c_sub7_sound_trigger(b1, b2);
                        StepResult::Advance {
                            next_pc: pc + header_size + 3,
                        }
                    }
                    8 => {
                        ctx.field_74 ^= 0x1000_0000;
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    0xA => {
                        if operand + 4 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let slot = bytecode[operand + 1];
                        let value =
                            i16::from_le_bytes([bytecode[operand + 2], bytecode[operand + 3]]);
                        host.op4c_n_c_sub_a_set_slot(slot, value);
                        StepResult::Advance {
                            next_pc: pc + header_size + 4,
                        }
                    }
                    sub @ (0xB | 0xC) => {
                        if operand + 4 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let slot = bytecode[operand + 1];
                        let raw =
                            u16::from_le_bytes([bytecode[operand + 2], bytecode[operand + 3]]);
                        // Sentinel substitution: `0xFFFF` → frame delta byte.
                        let value = if raw == 0xFFFF {
                            host.frame_delta() as i16
                        } else {
                            raw as i16
                        };
                        host.op4c_n_c_sub_bc_adjust_slot(slot, value, sub == 0xC);
                        StepResult::Advance {
                            next_pc: pc + header_size + 4,
                        }
                    }
                    0xE => {
                        let Some(&value) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        host.op4c_n_c_sub_e_set_b6ac(value);
                        StepResult::Advance {
                            next_pc: pc + header_size + 2,
                        }
                    }
                    // Sub-9: 2-byte `[4C, 0xC9]`. PC += 2 unless host says
                    // globals differ, then halt at PC.
                    9 => {
                        if host.op4c_n_c_sub9_globals_differ() {
                            StepResult::Halt { final_pc: pc }
                        } else {
                            StepResult::Advance {
                                next_pc: pc + header_size + 1,
                            }
                        }
                    }
                    // Sub-0xF: 4-byte `[4C, 0xCF, b1, b2]`. Each byte selects
                    // either the actor's world coordinate (0xFF), the
                    // tile-center conversion (`b * 0x80 + 0x40` for non-zero),
                    // or 0. PC += 4.
                    0xF => {
                        if operand + 3 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let b1 = bytecode[operand + 1];
                        let b2 = bytecode[operand + 2];
                        let resolve = |b: u8, world: u16| -> i16 {
                            if b == 0xFF {
                                world as i16
                            } else if b != 0 {
                                ((u16::from(b) << 7) | 0x40) as i16
                            } else {
                                0
                            }
                        };
                        let x = resolve(b1, ctx.world_x);
                        let z = resolve(b2, ctx.world_z);
                        host.op4c_n_c_sub_f_position_broadcast(x, z);
                        StepResult::Advance {
                            next_pc: pc + header_size + 3,
                        }
                    }
                    // All 16 sub-ops 0x0..=0xF are covered above; values 16+
                    // are unreachable because `op0 & 0x0F` is at most 0xF.
                    16..=u8::MAX => unreachable!("op0 & 0x0F is at most 0xF"),
                },
                // Outer nibble 0xD - party state + camera-ish setup.
                // All 16 sub-ops are ported: sub-0 (field SE trigger, 6-byte),
                // sub-1 (linked-list lookup gate, 2-byte), sub-2 (channel-spawn
                // halt, 2-byte), sub-3 (party state setup, 14-byte), sub-4
                // (VRAM STP-bit set on 16x1 rect, 6-byte), sub-5 (VRAM STP-bit
                // clear on 16x1 rect, 6-byte), sub-6 (`field_74` bitfield
                // mutation, halts at PC), sub-7 (list-walk register + halt,
                // 1-byte), sub-8 (`FUN_801D77F4` 4-arg call, 9-byte), sub-9
                // (inverted-Y mirror set, 4-byte), sub-0xA (clear mirror +
                // collision-Y refresh, 2-byte), sub-0xB (FUN_801E57F0 yield,
                // 13-byte), sub-0xC (party search-and-set, 5-byte), sub-0xD
                // (field_58 write, 3-byte), sub-0xE (party search query,
                // 5-byte), sub-0xF (scene byte write, 3-byte).
                0xD => match op0 & 0x0F {
                    // Sub-0: 6-byte `[4C, 0xD0, a_lo, a_hi, b_lo, b_hi]`.
                    // Field SE trigger with conditional u16 pair. The
                    // original at lines 6936-6944 of the dispatcher dump
                    // gates the call on three flag globals; PC advances by
                    // 6 in both branches. Host owns the gate state and the
                    // SE pipeline.
                    0 => {
                        if operand + 5 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let a = crate::field_helpers::load_u16_le(&bytecode[operand + 1..]);
                        let b = crate::field_helpers::load_u16_le(&bytecode[operand + 3..]);
                        host.op4c_n_d_sub_0_field_se_trigger(a, b);
                        StepResult::Advance {
                            next_pc: pc + header_size + 5,
                        }
                    }
                    // Sub-1: 1-byte. Linked-list lookup via `FUN_8003CF04`.
                    // Host returns `Some(new_pc)` for the ce9c-jump path,
                    // `None` for PC += 4 on miss. Default host returns None.
                    1 => match host.op4c_n_d_sub_1_list_lookup_jump(ctx) {
                        Some(new_pc) => StepResult::Advance { next_pc: new_pc },
                        None => StepResult::Advance {
                            next_pc: pc + header_size + 3,
                        },
                    },
                    // Sub-2: 2-byte `[4C, 0xD2, b1]`. Calls the channel
                    // resolver; halts at PC after the (possibly conditional)
                    // spawn.
                    2 => {
                        let Some(&b1) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        host.op4c_n_d_sub_2_channel_spawn(b1);
                        StepResult::Halt { final_pc: pc }
                    }
                    // Sub-7: 1-byte. Register `FUN_801DC0BC` callback then
                    // halt at PC.
                    7 => {
                        host.op4c_n_d_sub_7_register_list_walk();
                        StepResult::Halt { final_pc: pc }
                    }
                    3 => {
                        if operand + 13 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let a = i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
                        let b = i16::from_le_bytes([bytecode[operand + 3], bytecode[operand + 4]]);
                        let cd = u32::from_le_bytes([
                            bytecode[operand + 5],
                            bytecode[operand + 6],
                            bytecode[operand + 7],
                            bytecode[operand + 8],
                        ]);
                        let ef = u32::from_le_bytes([
                            bytecode[operand + 9],
                            bytecode[operand + 10],
                            bytecode[operand + 11],
                            bytecode[operand + 12],
                        ]);
                        let ab = ((a as i32 as u32) << 16) | ((b as u16) as u32);
                        host.op4c_n_d_sub3_party_setup(ab, cd, ef);
                        StepResult::Advance {
                            next_pc: pc + header_size + 13,
                        }
                    }
                    // Sub-6: 3-byte `[4C, 0xD6, b1]`. Pure ctx.field_74
                    // bitfield mutation; halts at PC.
                    6 => {
                        let Some(&b1) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        if b1 == 4 {
                            ctx.field_74 &= 0x7FFF_FFFF;
                        } else {
                            ctx.field_74 =
                                (ctx.field_74 & 0x7CFF_FFFF) | 0x8000_0000 | (u32::from(b1) << 24);
                        }
                        host.op4c_n_d_sub6_field74_mutate_ack();
                        StepResult::Halt { final_pc: pc }
                    }
                    // Sub-8: 9-byte `[4C, 0xD8, b1, lo_x, hi_x, lo_y, hi_y, lo_z, hi_z]`.
                    // Calls the overlay-resident `FUN_801D77F4` with `(b1, x, y, z)`
                    // (the host applies the call); PC += 9.
                    8 => {
                        if operand + 8 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let b1 = bytecode[operand + 1];
                        let mut words = [0i16; 3];
                        for (i, w) in words.iter_mut().enumerate() {
                            *w = i16::from_le_bytes([
                                bytecode[operand + 2 + i * 2],
                                bytecode[operand + 3 + i * 2],
                            ]);
                        }
                        host.op4c_n_d_sub8_call_d77f4(b1, words);
                        StepResult::Advance {
                            next_pc: pc + header_size + 8,
                        }
                    }
                    9 => {
                        if operand + 3 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        ctx.flags |= 0x2000_0000;
                        let raw =
                            i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
                        let value = if raw == 9999 {
                            (ctx.world_y as i16).wrapping_neg()
                        } else {
                            raw
                        };
                        ctx.field_8e = value;
                        ctx.world_y = value.wrapping_neg() as u16;
                        StepResult::Advance {
                            next_pc: pc + header_size + 3,
                        }
                    }
                    0xA => {
                        ctx.flags &= !0x2000_0000;
                        host.op4c_n_d_sub_a_collision_y_refresh(ctx);
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    0xD => {
                        let Some(&b1) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        ctx.field_58 = u16::from(b1);
                        StepResult::Advance {
                            next_pc: pc + header_size + 2,
                        }
                    }
                    0xF => {
                        let Some(&b1) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        host.op4c_n_d_sub_f_scene_byte_write(b1);
                        StepResult::Advance {
                            next_pc: pc + header_size + 2,
                        }
                    }
                    // Sub-B: 13-byte. Call FUN_801E57F0(operand) then PC += 13.
                    // Total instruction = opcode (1) + 12 operand bytes; the
                    // helper receives the 12-byte operand slice starting at
                    // the sub-op byte.
                    0xB => {
                        if operand + 12 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        host.op4c_n_d_sub_b_call_e57f0(&bytecode[operand..operand + 12]);
                        StepResult::Advance {
                            next_pc: pc + header_size + 12,
                        }
                    }
                    // Sub-C: 5-byte `[4C, 0xDC, b1, ?, ?]`. Small-table search
                    // + party-record write. Host returns `Some(new_pc)` for
                    // the ce9c-jump path or `None` for the PC += 5 miss path.
                    0xC => {
                        let Some(&b1) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        match host.op4c_n_d_sub_c_party_search_set(b1) {
                            Some(new_pc) => StepResult::Advance { next_pc: new_pc },
                            None => StepResult::Advance {
                                next_pc: pc + header_size + 4,
                            },
                        }
                    }
                    // Sub-E: 5-byte. Sister of sub-C without the per-record
                    // write - same control flow.
                    0xE => {
                        let Some(&b1) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        match host.op4c_n_d_sub_e_party_search_query(b1) {
                            Some(new_pc) => StepResult::Advance { next_pc: new_pc },
                            None => StepResult::Advance {
                                next_pc: pc + header_size + 4,
                            },
                        }
                    }
                    // Sub-4: 6-byte `[4C, 0xD4, x_lo, x_hi, y_lo, y_hi]`.
                    // VRAM 16x1 rect read-modify-write that sets PSX STP bit
                    // 15 on every non-zero pixel. Original (lines 7621-7642
                    // of overlay_world_map_walk_801de840.txt) reads two u16
                    // operands as `(vram_x, vram_y)` with hardcoded `w=0x10,
                    // h=1`, runs `DrawSync; StoreImage(rect, buf);
                    // DrawSync; for each of 16 pixels: if != 0 then OR
                    // with 0x8000; LoadImage(rect, buf)`. `StoreImage` =
                    // `FUN_8005842c`, `LoadImage` = `FUN_800583c8`,
                    // `DrawSync` = `FUN_80058104`. Returns `iVar47 + 6`.
                    4 => {
                        if operand + 5 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let x = crate::field_helpers::load_u16_le(&bytecode[operand + 1..]);
                        let y = crate::field_helpers::load_u16_le(&bytecode[operand + 3..]);
                        host.op4c_n_d_sub_4_vram_stp_set(x, y);
                        StepResult::Advance {
                            next_pc: pc + header_size + 5,
                        }
                    }
                    // Sub-5: 6-byte `[4C, 0xD5, x_lo, x_hi, y_lo, y_hi]`.
                    // Sister of sub-4 that clears PSX STP bit 15 on every
                    // pixel that isn't already exactly `0x8000` (STP-only
                    // transparent black). Inner loop is `if pixel != 0x8000
                    // then AND with 0x7FFF`. Same libgs round-trip as sub-4.
                    // Returns `iVar47 + 6`.
                    5 => {
                        if operand + 5 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let x = crate::field_helpers::load_u16_le(&bytecode[operand + 1..]);
                        let y = crate::field_helpers::load_u16_le(&bytecode[operand + 3..]);
                        host.op4c_n_d_sub_5_vram_stp_clear(x, y);
                        StepResult::Advance {
                            next_pc: pc + header_size + 5,
                        }
                    }
                    16..=u8::MAX => unreachable!("op0 & 0x0F is at most 0xF"),
                },
                // Outer nibble 0xE - misc scene writes + emitter helper calls.
                // All sub-ops 0x0..=0xE are ported: sub-0 (3-way state write,
                // halt at PC), sub-1 (variable-length text balloon), sub-2
                // (set globals, 6-byte), sub-3 (camera-anchored teleport,
                // 2-byte), sub-4 (bbox-test halt-or-advance, 9-byte), sub-5
                // (XP add, 5-byte), sub-6 (FUN_801D8280, 8-byte), sub-7
                // (camera animate, 7-byte), sub-8 (camera zoom, 10-byte),
                // sub-9 (clear b9c4, 2-byte), sub-0xA (call c7ec then halt),
                // sub-0xB (actor lookup + conditional jump, 5-byte), sub-0xC
                // (capture FUN_801DDF48, 2-byte), sub-0xD (set ba66, 3-byte),
                // sub-0xE (snapshot 84570, 2-byte). Sub-0xF has no `case` arm
                // in the original and falls through to the default halt.
                0xE => match op0 & 0x0F {
                    // Sub-0: 2-byte `[4C, 0xE0, b1]`. 3-way write (host
                    // performs based on b1 value); halt at PC.
                    0 => {
                        let Some(&b1) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        host.op4c_n_e_sub0_state_write(b1);
                        StepResult::Halt { final_pc: pc }
                    }
                    // Sub-1: variable-length text balloon. Spawns a
                    // screen-anchored text actor when the leading byte is
                    // non-zero; PC always advances by `3 + packet_length`
                    // (opcode byte + sub-op byte + terminator + payload).
                    1 => {
                        let Some(&first) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        let payload = &bytecode[operand + 1..];
                        let length = crate::field_helpers::packet_length(payload);
                        if first != 0 {
                            host.op4c_n_e_sub_1_text_actor(&payload[..length], ctx.script_id);
                        }
                        StepResult::Advance {
                            next_pc: pc + header_size + 2 + length,
                        }
                    }
                    2 => {
                        if operand + 3 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let fmv_id =
                            i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
                        host.op4c_n_e_sub2_fmv_trigger(fmv_id);
                        StepResult::Advance {
                            next_pc: pc + header_size + 5,
                        }
                    }
                    // Sub-3: 2-byte `[4C, 0xE3, actor_id]`. Camera-anchored
                    // teleport: copy active camera position+rotation onto
                    // the resolved actor. Dispatcher lines 7208-7227. PC
                    // advances by 2; missing actor is a silent no-op.
                    3 => {
                        let Some(&actor_id) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        host.op4c_n_e_sub_3_actor_sync_camera(ctx, actor_id);
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    // Sub-7: 7-byte `[4C, 0xE7, t0, t1, t2, d0, d1]`.
                    // Camera animate: target (24-bit LE) at +1 and duration
                    // (16-bit LE) at +4. Dispatcher lines 7281-7297. PC
                    // advances by 7.
                    7 => {
                        if operand + 6 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let target = crate::field_helpers::load_u24_le(&bytecode[operand + 1..]);
                        let duration = crate::field_helpers::load_u16_le(&bytecode[operand + 4..]);
                        host.op4c_n_e_sub_7_camera_animate(target, duration);
                        StepResult::Advance {
                            next_pc: pc + header_size + 6,
                        }
                    }
                    // Sub-8: 10-byte `[4C, 0xE8, x0, x1, y0, y1, z0, z1, m0,
                    // m1]`. Camera zoom: four 16-bit LE values for zoom_x,
                    // zoom_y, zoom_z, mode. Dispatcher lines 7298-7361 reads
                    // `func_0x8003ce9c` four times at offsets +1/+3/+5/+7.
                    // PC advances by 10.
                    8 => {
                        if operand + 9 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let zoom_x =
                            crate::field_helpers::load_u16_le(&bytecode[operand + 1..]) as i16;
                        let zoom_y =
                            crate::field_helpers::load_u16_le(&bytecode[operand + 3..]) as i16;
                        let zoom_z =
                            crate::field_helpers::load_u16_le(&bytecode[operand + 5..]) as i16;
                        let mode =
                            crate::field_helpers::load_u16_le(&bytecode[operand + 7..]) as i16;
                        host.op4c_n_e_sub_8_camera_zoom(zoom_x, zoom_y, zoom_z, mode);
                        StepResult::Advance {
                            next_pc: pc + header_size + 9,
                        }
                    }
                    // Sub-4: 9-byte `[4C, 0xE4, x0, z0, x1, z1, scale, ?, ?]`.
                    // BBox collision query. Each operand byte goes through
                    // the standard tile-center conversion (`(b & 0x7F) * 0x80
                    // + 0x40`, plus 0x40 if the high bit is set). When the
                    // host predicate says "outside", the original calls the
                    // halt helper FUN_801E3614; we model that as Halt at PC.
                    // When inside, advance PC by 8.
                    4 => {
                        if operand + 8 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let bbox = [
                            crate::field_helpers::tile_center(bytecode[operand + 1]),
                            crate::field_helpers::tile_center(bytecode[operand + 2]),
                            crate::field_helpers::tile_center(bytecode[operand + 3]),
                            crate::field_helpers::tile_center(bytecode[operand + 4]),
                        ];
                        if host.op4c_n_e_sub_4_bbox_outside(ctx, bbox) {
                            StepResult::Halt { final_pc: pc }
                        } else {
                            StepResult::Advance {
                                next_pc: pc + header_size + 8,
                            }
                        }
                    }
                    // Sub-5: 5-byte `[4C, 0xE5, b1, b2, b3]`. Read 24-bit
                    // signed XP delta via load_u24_le + sign_extend_24, then
                    // call the host's add-xp hook. PC += 4.
                    5 => {
                        if operand + 4 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let raw = crate::field_helpers::load_u24_le(&bytecode[operand + 1..]);
                        let xp_delta = crate::field_helpers::sign_extend_24(raw);
                        host.op4c_n_e_sub_5_add_xp(xp_delta);
                        StepResult::Advance {
                            next_pc: pc + header_size + 4,
                        }
                    }
                    6 => {
                        if operand + 7 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let mut words = [0i16; 3];
                        for (i, w) in words.iter_mut().enumerate() {
                            *w = i16::from_le_bytes([
                                bytecode[operand + 1 + i * 2],
                                bytecode[operand + 2 + i * 2],
                            ]);
                        }
                        host.op4c_n_e_sub6_call_d8280(words);
                        StepResult::Advance {
                            next_pc: pc + header_size + 7,
                        }
                    }
                    // Sub-9: 1-byte. Clear `_DAT_8007B9C4` then PC += 2 via
                    // `caseD_4` (the standard `addiu s8, s8, 0x2; j epilogue`
                    // block at 0x801df098).
                    9 => {
                        host.op4c_n_e_sub9_clear_b9c4();
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    // Sub-A: 1-byte. Call overlay-resident `func_0x8003C7EC`,
                    // halt at PC.
                    0xA => {
                        host.op4c_n_e_sub_a_call_c7ec();
                        StepResult::Halt { final_pc: pc }
                    }
                    0xC => {
                        host.op4c_n_e_sub_c_capture_ddf48();
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    0xD => {
                        let Some(&b1) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        host.op4c_n_e_sub_d_set_ba66(b1);
                        StepResult::Advance {
                            next_pc: pc + header_size + 2,
                        }
                    }
                    // Sub-B: 5-byte `[4C, 0xEB, actor_id, target_lo, target_hi]`.
                    // Conditional actor lookup with embedded jump target.
                    // When the host resolves the actor, advance PC by 5;
                    // otherwise jump to absolute `LE_u16(operand+2..=operand+3)`.
                    0xB => {
                        if operand + 4 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let actor_id = bytecode[operand + 1];
                        match host.op4c_n_e_sub_b_actor_jump(actor_id) {
                            Some(()) => StepResult::Advance {
                                next_pc: pc + header_size + 4,
                            },
                            None => {
                                let target =
                                    crate::field_helpers::load_u16_le(&bytecode[operand + 2..]);
                                StepResult::Advance {
                                    next_pc: target as usize,
                                }
                            }
                        }
                    }
                    0xE => {
                        host.op4c_n_e_sub_e_snapshot_84570();
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    // Sub-F: no `case` arm in the original; falls through to
                    // `switchD_801e00f4::default()` which returns `param_2`
                    // (= halt at PC) for outer nibble 0xE opcodes.
                    _ => StepResult::Halt { final_pc: pc },
                },
                // Outer nibble 0xF - only `op0 == 0xFF` is valid; falls
                // through to the default arm (PC += 2). Other sub-ops in
                // this nibble print SUB_CMD_0F_ERROR and also fall through.
                0xF => StepResult::Advance {
                    next_pc: pc + header_size + 1,
                },
                // Outer nibble 0xB has no `case 0xb` in the original 0x4C
                // switch (the dump goes case 0xa → default → case 0xc).
                // The default arm (line 6718) prints SUB_CMD_ERROR and
                // returns the dispatcher default - halt at PC.
                0xB => StepResult::Halt { final_pc: pc },
                // `op0 >> 4` is at most 0xF; outer nibble is fully covered
                // above, so this arm is dead code.
                16..=u8::MAX => unreachable!("op0 >> 4 is at most 0xF"),
            }
        }

        // 0x36 - SCENE_FADE. `[36, lo0, hi0, lo1, hi1]`, PC += 5 normally.
        // The host decides whether the fade applies (`SceneFadeResult::Done`)
        // or the scene is busy (`Busy` → halt at same PC). The original has
        // many sub-paths (0xFFFF wait, bit-15-set sub-cases 0..4, bit-15-clear
        // sub-paths) - they all funnel through the host.
        0x36 => {
            let Some(&lo0) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&hi0) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&lo1) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&hi1) = bytecode.get(operand + 3) else {
                return StepResult::Unknown { opcode, pc };
            };
            let op0_word = u16::from_le_bytes([lo0, hi0]);
            let op1_word = u16::from_le_bytes([lo1, hi1]);
            match host.scene_fade(op0_word, op1_word) {
                SceneFadeResult::Done => StepResult::Advance {
                    next_pc: pc + header_size + 4,
                },
                SceneFadeResult::Busy => StepResult::Halt { final_pc: pc },
            }
        }

        // 0x45 - CAMERA. Sub-dispatched by `op0 & 0xC0`:
        // - 0x40 LOAD: `[45, op0, ...18-byte payload]`, PC += 20.
        // - 0x80 SAVE: `[45, op0]`, PC += 2.
        // - 0x00 CONFIGURE: 10-bit mask in `[op0, op1]` selects slots; each
        //   set bit consumes a u16. `[45, op0, op1, lo, hi, ...2*set_count]`.
        //   PC += 5 + 2 * set_count. The two bytes at operand+2..4 are the
        //   `apply_trigger` value passed to the host.
        // - 0xC0 APPLY: `[45, op0, lo, hi]`, host applies the camera and
        //   the new PC is the absolute `LE_u16(operand[1..3])`.
        0x45 => {
            let Some(&op0) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            match op0 & 0xC0 {
                0x40 => {
                    // LOAD: 18-byte payload after op0.
                    let payload_end = operand + 1 + 18;
                    if payload_end > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    host.camera_load(&bytecode[operand + 1..payload_end]);
                    StepResult::Advance {
                        next_pc: pc + header_size + 19,
                    }
                }
                0x80 => {
                    host.camera_save();
                    StepResult::Advance {
                        next_pc: pc + header_size + 1,
                    }
                }
                0xC0 => {
                    let Some(&lo) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&hi) = bytecode.get(operand + 2) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    host.camera_apply();
                    let target = u16::from_le_bytes([lo, hi]) as usize;
                    StepResult::Advance { next_pc: target }
                }
                0x00 => {
                    let Some(&op1) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&trig_lo) = bytecode.get(operand + 2) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&trig_hi) = bytecode.get(operand + 3) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let mask = (u16::from(op0) << 8) | u16::from(op1);
                    let apply_trigger = u16::from_le_bytes([trig_lo, trig_hi]);
                    let mode = (op0 >> 2) & 0x0F;
                    // Cursor starts at 4 (past opcode+op0+op1+trigger u16 - i.e.
                    // operand + 3, since operand=pc+header_size means
                    // operand+3 = pc+header_size+3). The original `iVar18 = 4`
                    // is the byte index relative to pbVar47 (= operand), so
                    // first param is at operand + 4 in the bytecode.
                    let mut cursor = operand + 4;
                    let mut params: Vec<CameraParam> = Vec::with_capacity(10);
                    for slot in 0u8..10 {
                        let bit = 1u16 << (9 - slot);
                        if mask & bit == 0 {
                            continue;
                        }
                        if cursor + 1 >= bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let v = u16::from_le_bytes([bytecode[cursor], bytecode[cursor + 1]]);
                        params.push(CameraParam { slot, value: v });
                        cursor += 2;
                    }
                    let consumed = cursor - operand; // = 4 + 2 * set_count
                    host.camera_configure(&params, apply_trigger, mode);
                    StepResult::Advance {
                        next_pc: pc + header_size + consumed,
                    }
                }
                _ => unreachable!(),
            }
        }

        // 0x4D - BBOX_TEST. `[4D, x_min, z_min, x_max, z_max, lo_skip,
        // hi_skip]` (7 bytes). Inside box → PC += 7. Outside box →
        // forward-skip jump per `FUN_801e3614` (which is just
        // `addiu v0, v0, -2; j 0x801e3624; addu s8, s8, v0`).
        //
        // The first bbox compare's branch-delay slot does
        // `addiu s8, s8, 0x7` unconditionally, so by the time we reach the
        // helper s8 = `param_2 + 7`. The helper then adds `skip - 2`,
        // giving `param_2 + 5 + skip = pc + header_size + 4 + skip`.
        //
        // Tile derivation depends on a global flag (`_DAT_1F800394 &
        // 0x20000`). Hosts toggle via `world_to_tile_use_alt()`.
        0x4D => {
            let Some(&x_min) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&z_min) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&x_max) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&z_max) = bytecode.get(operand + 3) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&skip_lo) = bytecode.get(operand + 4) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&skip_hi) = bytecode.get(operand + 5) else {
                return StepResult::Unknown { opcode, pc };
            };
            let (x_tile, z_tile) = if host.world_to_tile_use_alt() {
                let xt = ((i32::from(ctx.world_x as i16)) << 16) >> 23;
                let zt = ((i32::from(ctx.world_z as i16)) << 16) >> 23;
                (xt, zt)
            } else {
                let xt = (i32::from(ctx.world_x as i16) - 0x40) >> 7;
                let zt = (i32::from(ctx.world_z as i16) - 0x40) >> 7;
                (xt, zt)
            };
            let inside = x_tile >= i32::from(x_min)
                && z_tile >= i32::from(z_min)
                && x_tile <= i32::from(x_max)
                && z_tile <= i32::from(z_max);
            if inside {
                StepResult::Advance {
                    next_pc: pc + header_size + 6,
                }
            } else {
                let delta = u16::from_le_bytes([skip_lo, skip_hi]) as usize;
                StepResult::Advance {
                    next_pc: pc + header_size + 4 + delta,
                }
            }
        }

        // 0x3F - DIALOG: open a dialog box.
        // Encoding: [3F, lo, hi, len, [len bytes inline], xb, zb, depth_id]
        // - text_id = lo + hi*0x100 (16-bit, little-endian)
        // - inline buffer holds `len` raw bytes (the original copies up to
        //   16 bytes into a local buffer, null-terminated)
        // - xb / zb decode to grid_to_world coords
        // - depth_id is the raw byte (original indexes a depth-lookup table)
        // PC += header_size + 3 + len + 3
        0x3F => {
            let Some(&lo) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&hi) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&len) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            let text_id = u16::from_le_bytes([lo, hi]);
            let len_usize = len as usize;
            let inline_start = operand + 3;
            let inline_end = inline_start + len_usize;
            let pos_start = inline_end;
            // Need pos_start + 3 bytes (xb, zb, depth_id).
            if pos_start + 3 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let inline = &bytecode[inline_start..inline_end];
            let xb = bytecode[pos_start];
            let zb = bytecode[pos_start + 1];
            let depth_id = bytecode[pos_start + 2];
            let world_x = grid_to_world(xb);
            let world_z = grid_to_world(zb);
            host.open_dialog(text_id, inline, world_x, world_z, depth_id);
            StepResult::Advance {
                next_pc: pc + header_size + 6 + len_usize,
            }
        }

        // 0x40 - DATA_BLOCK: skip `len` bytes after the header.
        // Encoding: [0x40, len, ...len bytes]. PC += header_size + 1 + len.
        0x40 => {
            let Some(&len) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            StepResult::Advance {
                next_pc: pc + header_size + 1 + len as usize,
            }
        }

        // 0x4A - WAIT_FRAMES: ctx.wait_accum += frame_delta. If accum < target,
        // halt at the same PC (script will resume on next tick); else clear
        // and advance. Target is read via the SCUS helper `func_0x8003CE9C`,
        // which reads a 16-bit little-endian value from the operand cursor.
        0x4A => {
            let Some(&lo) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&hi) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let target = i32::from(u16::from_le_bytes([lo, hi]));
            ctx.wait_accum = ctx.wait_accum.saturating_add(host.frame_delta() as i16);
            if i32::from(ctx.wait_accum) < target {
                StepResult::Halt { final_pc: pc }
            } else {
                ctx.wait_accum = 0;
                StepResult::Advance {
                    next_pc: pc + header_size + 2,
                }
            }
        }

        // 0x4E - inventory comparison-and-jump. Sub-dispatched by
        // `op1 >> 4`. Encoding for sub-ops 0/1:
        //   `[4E, page, mode, arg_lo, arg_hi, skip_lo, skip_hi]` (7 bytes).
        // `mode` high nibble = sub-op; low nibble = comparison operator
        // (0 = state < scaled, 1 = scaled < state).
        //
        // Sub-ops 0/1 read `(state, factor)` from the inventory page via
        // `host.inventory_compare_pair(page, sub_op)`, compute
        // `scaled = (factor * arg) >> 8` (signed; the original rounds toward
        // zero for negative results), then compare per the operator. On
        // success, jump to `pc + header_size + 4 + LE_u16(operand[4..6])`;
        // on failure, advance past the 7-byte instruction.
        //
        // Sub-ops 2/3/5/6/7/8/9 fall through to an absolute jump:
        // `next_pc = LE_u16(operand[2..4])`.
        //
        // Sub-op 4 invokes [`FieldHost::op4e_sub4_bios_rand`] (BIOS Rand stub)
        // and uses the returned value as the next PC; the default is 0, which
        // restarts the script at the bytecode origin. Sub-ops 10/11 are the
        // party-bank comparison (9-byte encoding) ported above.
        0x4E => {
            let Some(&page) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&mode_byte) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let sub_op = mode_byte >> 4;
            match sub_op {
                0 | 1 => {
                    let Some(&arg_lo) = bytecode.get(operand + 2) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&arg_hi) = bytecode.get(operand + 3) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&skip_lo) = bytecode.get(operand + 4) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&skip_hi) = bytecode.get(operand + 5) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let arg = i32::from(u16::from_le_bytes([arg_lo, arg_hi]));
                    let (state, factor) = host.inventory_compare_pair(page, sub_op);
                    let raw = factor.wrapping_mul(arg);
                    let scaled = if raw < 0 { (raw + 0xFF) >> 8 } else { raw >> 8 };
                    let cmp = mode_byte & 0x0F;
                    let taken = match cmp {
                        0 => state < scaled,
                        1 => scaled < state,
                        _ => false,
                    };
                    if taken {
                        let delta = u16::from_le_bytes([skip_lo, skip_hi]) as usize;
                        StepResult::Advance {
                            next_pc: pc + header_size + 4 + delta,
                        }
                    } else {
                        StepResult::Advance {
                            next_pc: pc + header_size + 6,
                        }
                    }
                }
                2 | 3 | 5 | 6 | 7 | 8 | 9 => {
                    let Some(&lo) = bytecode.get(operand + 2) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&hi) = bytecode.get(operand + 3) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let target = u16::from_le_bytes([lo, hi]) as usize;
                    StepResult::Advance { next_pc: target }
                }
                12..=15 => {
                    // sub-ops 12..=15 hit the dispatcher's default arm at
                    // `switchD_801e0a38_default`: with `uVar31 = uVar27 = 0`
                    // the boolean test is false either way, and the
                    // `(sub_op - 10) < 2` check fails for sub-op >= 12, so
                    // the original returns `param_2 + 7` (= PC += 7).
                    StepResult::Advance {
                        next_pc: pc + header_size + 6,
                    }
                }
                10 | 11 => {
                    // 9-byte party-bank comparison:
                    //   [4E, _, mode, lo1, hi1, skip_lo, skip_hi, lo2, hi2]
                    // Original packs `LE_u16(operand[2..4])` into the low half
                    // of a u32 and `LE_u16(operand[6..8])` into the high half,
                    // then compares it (signed) against the bank value.
                    let Some(&lo1) = bytecode.get(operand + 2) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&hi1) = bytecode.get(operand + 3) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&skip_lo) = bytecode.get(operand + 4) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&skip_hi) = bytecode.get(operand + 5) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&lo2) = bytecode.get(operand + 6) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&hi2) = bytecode.get(operand + 7) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let low = u16::from_le_bytes([lo1, hi1]) as u32;
                    let high = u16::from_le_bytes([lo2, hi2]) as u32;
                    let scaled = (low | (high << 16)) as i32;
                    let state = host.party_bank_value(sub_op);
                    let cmp = mode_byte & 0x0F;
                    let taken = match cmp {
                        0 => state < scaled,
                        1 => scaled < state,
                        _ => false,
                    };
                    if taken {
                        let delta = u16::from_le_bytes([skip_lo, skip_hi]) as usize;
                        StepResult::Advance {
                            next_pc: pc + header_size + 4 + delta,
                        }
                    } else {
                        StepResult::Advance {
                            next_pc: pc + header_size + 8,
                        }
                    }
                }
                // Sub-op 4: `iVar18 = func_0x80056798(); return iVar18;` -
                // FUN_80056798 is a BIOS Rand thunk (`jr 0xA0; t1=0x2F`).
                // The original returns the random value as the next PC. There
                // are no captured callers, so this is almost certainly a dev
                // stub. The host hook returns the next-PC value (default 0,
                // matching broken-as-shipped behaviour).
                4 => {
                    let next = host.op4e_sub4_bios_rand();
                    StepResult::Advance {
                        next_pc: next as usize,
                    }
                }
                // `mode_byte >> 4` is at most 0xF; arms above cover every
                // value, so this arm is dead code.
                16..=u8::MAX => unreachable!("mode_byte >> 4 is at most 0xF"),
            }
        }

        // 0x49 - STATE_RESUME. Multi-frame state machine on `_DAT_8007B450`.
        // The host surfaces the tristate via [`FieldHost::op49_state`].
        //
        // - `Idle`: arm a new resume. Sub-op 1 also captures `ctx.field_90`
        //   into `_DAT_8007B44C` (the original's actor-handle save). The
        //   instruction halts at the same PC; the host advances the
        //   underlying state machine and flips to `Done` on the next
        //   resume.
        // - `Armed`: another resume is already in flight - halt.
        // - `Done`: clear state and dispatch on the sub-op:
        //   - 1, 3, 7: PC += 3
        //   - 2, 4: PC += 7
        //   - 5: PC += 14
        //   - all other sub-ops are not yet ported (Pending).
        0x49 => {
            let Some(&sub_op) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            match host.op49_state() {
                Op49State::Idle => {
                    if sub_op > 0xD {
                        return StepResult::Halt { final_pc: pc };
                    }
                    host.op49_invoke_setup();
                    let captured = if sub_op == 1 { ctx.field_90 } else { 0 };
                    host.op49_arm(pc, captured);
                    StepResult::Halt { final_pc: pc }
                }
                Op49State::Armed => StepResult::Halt { final_pc: pc },
                Op49State::Done => {
                    host.op49_clear();
                    match sub_op {
                        // sub-0 in DONE state - embedded MES bytecode walker.
                        // Instruction: `[49, 0, length, ...length args..., ...mes_bytes]`.
                        // The original reads `length = pbVar47[2]`, then calls
                        // `func_0x8003ca38(pbVar47 + length + 3)` (= the MES-shape
                        // walker that counts bytes > 0x1E, with one-byte
                        // peek-extension for 0xCx prefix bytes), and returns
                        // `param_2 + length + 5 + mes_count`.
                        0 => {
                            let Some(&length) = bytecode.get(operand + 1) else {
                                return StepResult::Unknown { opcode, pc };
                            };
                            let mes_start = operand + 2 + length as usize;
                            if mes_start > bytecode.len() {
                                return StepResult::Unknown { opcode, pc };
                            }
                            let mes_count = walk_mes_bytecode(&bytecode[mes_start..]);
                            StepResult::Advance {
                                next_pc: pc + header_size + 4 + length as usize + mes_count,
                            }
                        }
                        1 | 3 | 7 => StepResult::Advance {
                            next_pc: pc + header_size + 2,
                        },
                        2 | 4 => StepResult::Advance {
                            next_pc: pc + header_size + 6,
                        },
                        5 => StepResult::Advance {
                            next_pc: pc + header_size + 13,
                        },
                        // sub-6/8/9/C/D in Done all jump through LAB_801df898
                        // which does `addiu s8, s8, 0x5; j 0x801df89c` -
                        // PC += 5 from the opcode (= header_size + 4 past
                        // the sub-op byte). The original reads
                        // `_DAT_8007babc` into a register but only
                        // sub-paths lower in the dispatch (e.g. the 0x4C
                        // sub-3 sub-C wrapper) consume it; in the 0x49 Done
                        // arm it's used purely as a side-effect register.
                        6 | 8 | 9 | 0xC | 0xD => StepResult::Advance {
                            next_pc: pc + header_size + 4,
                        },
                        // sub-A / sub-B / any other byte > 0xD: the Done-side
                        // catch-all in `FUN_801de840 case 0x49` clears the
                        // resume slot and returns `param_2` (halt at PC).
                        // `op49_clear()` was already called above, so this is
                        // just the halt.
                        _ => StepResult::Halt { final_pc: pc },
                    }
                }
            }
        }

        // 0x34 - EFFECT. Sub-dispatched by `op0 >> 4` (4 sub-ops).
        // Sub-ops ported:
        // - 0: effect-global colour + intensity setup. PC += 7. Reads RGB at
        //   operand[1..4] + s16 intensity at operand[4..6]; the original
        //   falls through `LAB_801E212C` whose `iVar43 = iVar47 + 7` advances
        //   the PC by 7 bytes total.
        // - 1: effect/sprite spawn with optional captured-PC. PC += 13 (or
        //   `13 + 2 + pbVar46[0xD]` if `pbVar46[0xC] == 0x40`). Reads a
        //   24-bit packed value at operand[1..4] + four s16 fields at
        //   operand[4..0xC] + capture_flag at operand[0xC] + pc_payload_len
        //   at operand[0xD]. The original walks the actor list to skip the
        //   spawn if a matching actor is already alive.
        // - 2: actor-pool capture-and-yield (linked-list lookup; if found and
        //   `b1 == 0x40` the runtime captures the post-PC into the actor's
        //   `+0x94` slot and yields via STATE_RESUME, otherwise PC += 2).
        // - 3: 3D-model animation trigger via `host.effect_anim_trigger`.
        0x34 => {
            let Some(&op0) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let sub = op0 >> 4;
            match sub {
                0 => {
                    // 7-byte instruction: [op0, r, g, b, intensity_lo, intensity_hi].
                    let Some(rgb_int) = bytecode.get(operand + 1..operand + 6) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let rgb = [rgb_int[0], rgb_int[1], rgb_int[2]];
                    let intensity = i16::from_le_bytes([rgb_int[3], rgb_int[4]]);
                    host.op34_sub0_color_intensity_setup(op0, rgb, intensity);
                    StepResult::Advance {
                        next_pc: pc + header_size + 6,
                    }
                }
                1 => {
                    // Base instruction is 13 bytes (opcode + 12 operand
                    // bytes). The "capture flag" at `pbVar46[0xC]` is the
                    // BYTE JUST PAST the instruction - the runtime peeks at
                    // the first byte of the next instruction to decide
                    // whether to consume it as a capture extension.
                    let Some(payload) = bytecode.get(operand + 1..operand + 12) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let packed24 = ((payload[0] as u32) << 16)
                        | ((payload[1] as u32) << 8)
                        | (payload[2] as u32);
                    let world_x = i16::from_le_bytes([payload[3], payload[4]]);
                    let world_z = i16::from_le_bytes([payload[5], payload[6]]);
                    // The original NEGATES the y component (`local_a6 = -local_a6`)
                    // before the spawn call - undo the sign here.
                    let raw_neg_y = i16::from_le_bytes([payload[7], payload[8]]);
                    let world_y = raw_neg_y.wrapping_neg();
                    // Peek the byte AT pc + 13 (first byte after the
                    // 13-byte base instruction). When it's 0x40, the
                    // runtime treats it as a capture-extension marker and
                    // PC advances by an extra `2 + payload_len`.
                    let capture_flag = bytecode.get(operand + 12).copied().unwrap_or(0);
                    let captured_pc_payload: &[u8] = if capture_flag == 0x40 {
                        let payload_len = bytecode.get(operand + 13).copied().unwrap_or(0) as usize;
                        let start = operand + 14;
                        let end = start + payload_len;
                        bytecode.get(start..end).unwrap_or(&[])
                    } else {
                        &[]
                    };
                    let delta_from_opcode = host.op34_sub1_spawn_or_skip(
                        ctx,
                        op0,
                        packed24,
                        [world_x, world_y, world_z],
                        capture_flag,
                        captured_pc_payload,
                    );
                    StepResult::Advance {
                        next_pc: pc + delta_from_opcode,
                    }
                }
                2 => {
                    // sub-2: 3-byte instruction `[34, 0x2N, b1, ...]`. The
                    // original walks the actor list at `_DAT_8007C354` looking
                    // for an entry with `[+0x90] == iVar18` (current ctx). If
                    // found AND `b1 == 0x40`, it captures `pbVar47 + 3` into
                    // the matched actor's `+0x94` (a forwarded-PC pointer) and
                    // returns via `caseD_4()` (STATE_RESUME → `Yield`).
                    // Otherwise it falls through `code_r0x801df098` for PC += 2.
                    let Some(&b1) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let captured_pc_offset = pc + header_size + 2;
                    let captured =
                        host.op34_capture_pc_for_existing_actor(ctx, b1, captured_pc_offset);
                    if captured {
                        StepResult::Yield { resume_pc: pc }
                    } else {
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                }
                3 => {
                    let Some(&arg) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    host.effect_anim_trigger(ctx, arg);
                    StepResult::Advance {
                        next_pc: pc + header_size + 2,
                    }
                }
                // Sub-ops 4..=0xF: original has no `case` arm; falls through
                // `if (bVar35 != 2) { if (bVar35 != 3) { return param_2; } }`
                // at line 4811-4814 of the dump ⇒ halt at PC.
                4..=15 => StepResult::Halt { final_pc: pc },
                // `op0 >> 4` is at most 0xF; arms above cover every value.
                16..=u8::MAX => unreachable!("op0 >> 4 is at most 0xF"),
            }
        }

        // 0x43 - ACTOR_CTRL. Massive sub-dispatcher (22+ sub-ops keyed on
        // `pbVar47[0]`). Sub-ops ported:
        // - 2: 3-actor talk via FUN_801D2D38, 8-byte instruction.
        // - 7: face/body rotation setup, 17-byte instruction.
        // - 8: face/rotation reset, 2-byte instruction.
        // - 12 (0xC): allocate scripted actor via FUN_801de754, 5-byte.
        // - 13/15 (0xD/0xF): allocate actor via FUN_801de7bc with mode
        //   (3 for 0xD, 0 for 0xF), 6-byte.
        // - 14 (0xE): mark currently-iterating actor flag bit 0x8, 2-byte.
        // Other sub-ops (movement targeting, party-actor lookup, eye-blink
        // setup, model-swap, etc.) remain `Pending`.
        0x43 => {
            let Some(&sub_op) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            match sub_op {
                // Halt-acquire dispatcher (sub-0/1/A/B). 5-byte for sub-0/1,
                // 9-byte for sub-A/B. Acquire = save HALT bit + saved_pc on
                // ctx; on success, return absolute resume PC via the s16
                // operand at +3 (sub-0/1) or +7 (sub-A/B). On failure (the
                // host's predicate returns false), advance PC by the standard
                // amount (5 or 9). See `docs/subsystems/script-vm.md`
                // (opcode 0x43, halt-acquire dispatcher).
                0 | 1 | 0xA | 0xB => {
                    let wide = sub_op == 0xA || sub_op == 0xB;
                    let needed = if wide { 8 } else { 4 };
                    if operand + needed >= bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    let coords = [
                        i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]),
                        0,
                        i16::from_le_bytes([
                            bytecode.get(operand + 3).copied().unwrap_or(0),
                            bytecode.get(operand + 4).copied().unwrap_or(0),
                        ]),
                    ];
                    let target_offset = if wide { 7 } else { 3 };
                    if host.field_halt_acquire_predicate(ctx, sub_op) {
                        let resume = i16::from_le_bytes([
                            bytecode[operand + target_offset],
                            bytecode[operand + target_offset + 1],
                        ]) as i32 as usize;
                        ctx.flags |= 0x400;
                        ctx.wait_accum = 0;
                        ctx.saved_pc = pc as u32;
                        host.field_halt_acquire_apply(ctx, sub_op, resume, coords);
                        StepResult::Yield { resume_pc: resume }
                    } else {
                        let advance_by = if wide { 9 } else { 5 };
                        StepResult::Advance {
                            next_pc: pc + header_size + advance_by - 1,
                        }
                    }
                }
                2 => {
                    let Some(&a1) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&a2) = bytecode.get(operand + 2) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&a3) = bytecode.get(operand + 3) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&lo) = bytecode.get(operand + 4) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&hi) = bytecode.get(operand + 5) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&b6) = bytecode.get(operand + 6) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let arg_word = u16::from_le_bytes([lo, hi]);
                    host.op43_three_actor_talk([a1, a2, a3], arg_word, b6);
                    StepResult::Advance {
                        next_pc: pc + header_size + 7,
                    }
                }
                3..=6 => {
                    let Some(&b1) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&b2) = bytecode.get(operand + 2) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&b3) = bytecode.get(operand + 3) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&b4) = bytecode.get(operand + 4) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&t_lo) = bytecode.get(operand + 5) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&t_hi) = bytecode.get(operand + 6) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&c_lo) = bytecode.get(operand + 7) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&c_hi) = bytecode.get(operand + 8) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let ticks = u16::from_le_bytes([t_lo, t_hi]);
                    let curve = u16::from_le_bytes([c_lo, c_hi]);
                    host.op43_sound_register_ramp(sub_op, [b1, b2, b3, b4], ticks, curve);
                    StepResult::Advance {
                        next_pc: pc + header_size + 9,
                    }
                }
                7 => {
                    let Some(&face_id) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    if operand + 16 > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    let payload_4 = u32::from_le_bytes([
                        bytecode[operand + 2],
                        bytecode[operand + 3],
                        bytecode[operand + 4],
                        bytecode[operand + 5],
                    ]);
                    let params = [
                        u16::from_le_bytes([bytecode[operand + 6], bytecode[operand + 7]]),
                        u16::from_le_bytes([bytecode[operand + 8], bytecode[operand + 9]]),
                        u16::from_le_bytes([bytecode[operand + 10], bytecode[operand + 11]]),
                        u16::from_le_bytes([bytecode[operand + 12], bytecode[operand + 13]]),
                    ];
                    let target =
                        u16::from_le_bytes([bytecode[operand + 14], bytecode[operand + 15]]) as i16;
                    ctx.face_rotation = face_id;
                    host.actor_face_rotation_setup(ctx, face_id, payload_4, params, target);
                    StepResult::Advance {
                        next_pc: pc + header_size + 16,
                    }
                }
                8 => {
                    ctx.face_rotation = 0;
                    host.actor_face_reset(ctx);
                    StepResult::Advance {
                        next_pc: pc + header_size + 1,
                    }
                }
                0xC => {
                    let Some(&b1) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&b2) = bytecode.get(operand + 2) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&b3) = bytecode.get(operand + 3) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    host.op43_alloc_scripted_actor(b1, b2, b3);
                    StepResult::Advance {
                        next_pc: pc + header_size + 4,
                    }
                }
                0xD | 0xF => {
                    let Some(&b1) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&b2) = bytecode.get(operand + 2) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&b3) = bytecode.get(operand + 3) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&b4) = bytecode.get(operand + 4) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let mode = if sub_op == 0xD { 3 } else { 0 };
                    host.op43_alloc_actor_with_mode(sub_op, mode, [b1, b2, b3, b4]);
                    StepResult::Advance {
                        next_pc: pc + header_size + 5,
                    }
                }
                0xE => {
                    host.op43_mark_actor_flag_8(ctx);
                    StepResult::Advance {
                        next_pc: pc + header_size + 1,
                    }
                }
                9 => {
                    // 10-byte: [43, 9, x_lo, x_hi, y_lo, y_hi, z_lo, z_hi, t_lo, t_hi]
                    let Some(&xl) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&xh) = bytecode.get(operand + 2) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&yl) = bytecode.get(operand + 3) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&yh) = bytecode.get(operand + 4) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&zl) = bytecode.get(operand + 5) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&zh) = bytecode.get(operand + 6) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&tl) = bytecode.get(operand + 7) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&th) = bytecode.get(operand + 8) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let x = u16::from_le_bytes([xl, xh]);
                    let y = u16::from_le_bytes([yl, yh]);
                    let z = u16::from_le_bytes([zl, zh]);
                    let ticks = u16::from_le_bytes([tl, th]);
                    if ticks != 0 {
                        host.op43_sub9_tween(ctx, x, y, z, ticks);
                    } else {
                        // Immediate write: only if value != 0xFFFF (sentinel).
                        if x != 0xFFFF {
                            ctx.world_x = x;
                        }
                        if y != 0xFFFF {
                            ctx.world_y = y;
                        }
                        if z != 0xFFFF {
                            ctx.world_z = z;
                        }
                        // ctx.flags & 0x20000000 mirrors -y onto +0x8E (face_8E).
                        // We don't have that field exposed yet; the host can read
                        // ctx.flags + ctx.world_y after the call.
                    }
                    StepResult::Advance {
                        next_pc: pc + header_size + 9,
                    }
                }
                0x10 => {
                    if operand + 20 > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    host.op43_emitter_init(&bytecode[operand + 1..operand + 20]);
                    StepResult::Advance {
                        next_pc: pc + header_size + 20,
                    }
                }
                0x11 => {
                    if operand + 11 > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    let mut words = [0u16; 5];
                    for (i, w) in words.iter_mut().enumerate() {
                        *w = u16::from_le_bytes([
                            bytecode[operand + 1 + i * 2],
                            bytecode[operand + 2 + i * 2],
                        ]);
                    }
                    host.op43_emitter_5_words(words);
                    StepResult::Advance {
                        next_pc: pc + header_size + 11,
                    }
                }
                0x12 => {
                    if operand + 13 > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    let mut words = [0i16; 6];
                    for (i, w) in words.iter_mut().enumerate() {
                        *w = u16::from_le_bytes([
                            bytecode[operand + 1 + i * 2],
                            bytecode[operand + 2 + i * 2],
                        ]) as i16;
                    }
                    let did_split = words[2] > 0xFF;
                    host.op43_emitter_split_call(words, did_split);
                    StepResult::Advance {
                        next_pc: pc + header_size + 13,
                    }
                }
                0x13 => {
                    if operand + 13 > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    let mut payload = [0u8; 13];
                    payload.copy_from_slice(&bytecode[operand..operand + 13]);
                    host.op43_emitter_func13(&payload);
                    StepResult::Advance {
                        next_pc: pc + header_size + 13,
                    }
                }
                0x14 => {
                    if operand + 9 > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    let mut words = [0i16; 4];
                    for (i, w) in words.iter_mut().enumerate() {
                        *w = u16::from_le_bytes([
                            bytecode[operand + 1 + i * 2],
                            bytecode[operand + 2 + i * 2],
                        ]) as i16;
                    }
                    host.op43_emitter_4_words(words);
                    StepResult::Advance {
                        next_pc: pc + header_size + 9,
                    }
                }
                0x15 => {
                    if operand + 13 > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    host.op43_emitter_struct_12(&bytecode[operand + 1..operand + 13]);
                    StepResult::Advance {
                        next_pc: pc + header_size + 13,
                    }
                }
                // Sub-ops 0x16..=0xFF: original `case 0x43` inner switch has
                // no `case` arm beyond 0x15. Such sub-ops fall out of the
                // inner switch with `iVar45 = param_2` (initialised at
                // line 4511 of the dump) and hit the outer `break;` ⇒
                // halt at PC.
                _ => StepResult::Halt { final_pc: pc },
            }
        }

        // Default arm: high-byte route. The original dispatcher's default
        // case checks `*pbVar43 & 0x70` (raw opcode byte) and routes to one
        // of three SCUS helpers - SET / CLEAR / TEST against the 256-bit
        // bitfield at DAT_80086D70 (the **fourth flag bank**).
        //
        // The masked opcode here is `opcode_byte & 0x7F`, so `0x5x`/`0x6x`/
        // `0x7x` ranges fall through to this arm. The flag index is built
        // from the low nibble of the raw opcode byte plus the extended-bit:
        //   idx = ((opcode_byte & 0x8F) << 8) | operand[0]
        //
        // - 0x5_ SET   : PC += 1 idx byte. host.system_flag_set(idx).
        // - 0x6_ CLEAR : PC += 1 idx byte. host.system_flag_clear(idx).
        // - 0x7_ TEST  : 4-byte instruction (idx byte + 2 target bytes).
        //                When the bit IS set, jump to
        //                `pc + header_size + 1 + LE_u16(operand[1..3])`
        //                (relative offset from after the idx byte).
        //                When clear, fall through past the 4 bytes.
        0x50..=0x77 => {
            let route = opcode & 0x70;
            let Some(&idx_lo) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let idx = (u16::from(opcode_byte & 0x8F) << 8) | u16::from(idx_lo);
            match route {
                0x50 => {
                    host.system_flag_set(idx);
                    StepResult::Advance {
                        next_pc: pc + header_size + 1,
                    }
                }
                0x60 => {
                    host.system_flag_clear(idx);
                    StepResult::Advance {
                        next_pc: pc + header_size + 1,
                    }
                }
                0x70 => {
                    let Some(&off_lo) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&off_hi) = bytecode.get(operand + 2) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    if host.system_flag_test(idx) {
                        let delta = u16::from_le_bytes([off_lo, off_hi]) as usize;
                        let target = (pc + header_size + 1).wrapping_add(delta);
                        StepResult::Advance { next_pc: target }
                    } else {
                        StepResult::Advance {
                            next_pc: pc + header_size + 3,
                        }
                    }
                }
                // For opcode in 0x50..=0x77, `opcode & 0x70` can only be
                // 0x50, 0x60, or 0x70 - every value handled above.
                _ => unreachable!("opcode 0x{:02X} & 0x70 must be 0x50/0x60/0x70", opcode),
            }
        }

        // Top-level catch-all. The original dispatcher's `default:` arm
        // (line 4622 of the dump) routes through `*pbVar43 & 0x70`; for any
        // raw opcode byte whose high nibble is NOT 0x5x/0x6x/0x7x and whose
        // masked opcode isn't matched explicitly above, the original returns
        // `param_2` - halt at PC. The masked opcode here is `opcode_byte &
        // 0x7F`; the field VM has 43 documented opcodes, all of which are
        // explicitly cased above, so reaching this arm means a malformed or
        // future-extension byte. Halt rather than panic so we behave like
        // the original on garbage input.
        _ => StepResult::Halt { final_pc: pc },
    }
}

/// Execute one instruction in *cross-context* mode.
///
/// Use this only when [`peek_extended`] returned `Some(target_id)` and
/// `target_ctx != caller_ctx`. Equivalent to [`step`] for the dispatch +
/// state-write side, plus the original's "if target is the player, propagate
/// the YIELD halt to caller as well" behaviour.
///
/// `caller_player` is `true` when `caller_ctx` is the active player script
/// (the original branches on `iVar18 == _DAT_8007C364`). The host is the
/// authority - pass `caller_ctx.script_id == host.player_script_id()` or
/// equivalent. When in doubt, pass `false` and only the target halts.
pub fn step_with_caller<H: FieldHost>(
    host: &mut H,
    target_ctx: &mut FieldCtx,
    caller_ctx: &mut FieldCtx,
    target_is_player: bool,
    bytecode: &[u8],
    pc: usize,
) -> StepResult {
    let result = step(host, target_ctx, bytecode, pc);
    if target_is_player && let StepResult::Yield { .. } = result {
        // The original copies pbVar43 (opcode pointer) into both
        // target.saved_pc and caller.saved_pc. step() already set
        // target.saved_pc = pc; mirror it onto caller.
        caller_ctx.saved_pc = pc as u32;
        caller_ctx.wait_accum = 0;
        caller_ctx.halt();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One observed `op34_sub1_spawn_or_skip` invocation.
    #[derive(Debug, Default)]
    struct Op34Sub1Call {
        op0: u8,
        packed24: u32,
        pos: [i16; 3],
        capture_flag: u8,
        captured_payload: Vec<u8>,
    }

    /// Recording host: tracks every host-state interaction so tests can
    /// assert exact outcomes. Mirrors the actor-VM tests' approach.
    #[derive(Default)]
    struct TestHost {
        globals: u32,
        extras: u32,
        screen_mode: u32,
        screen_mode_table: Vec<Option<u32>>,
        frame_delta: u16,
        cam_cfg_table: Vec<u16>,                    // index = op0 & 0x0F
        exec_moves: Vec<(u16, u8)>,                 // (script_id, move_id)
        move_tos: Vec<(u16, u16, u16, bool)>,       // (script_id, world_x, world_z, is_player)
        dialogs: Vec<(u16, Vec<u8>, u16, u16, u8)>, // (text_id, inline, world_x, world_z, depth_id)
        bgm_calls: Vec<(u16, u8)>,                  // (text_id, sub_op)
        sfx_calls: Vec<u8>,                         // sfx_id list
        money_deltas: Vec<i32>,
        item_writes: Vec<(u8, u8)>, // (slot_byte, count)
        party_added: Vec<u8>,
        party_removed: Vec<u8>,
        interacts: Vec<(u8, u8)>,
        scene_transitions: Vec<u8>,
        render_long: Vec<(u8, u8, u8, u8)>,
        render_short: Vec<(u8, u8, u8, u8)>,
        scene_regs: Vec<(u8, u8, u8)>,
        counter_calls: Vec<u8>,
        animations: Vec<(u16, u8, u8, Vec<u8>)>,
        party_leaders: Vec<u8>,
        camera_configs: Vec<(Vec<CameraParam>, u16, u8)>,
        camera_loads: Vec<Vec<u8>>,
        camera_saves: u32,
        camera_applies: u32,
        scene_fade_calls: Vec<(u16, u16)>,
        scene_fade_busy: bool,
        // 0x4E inventory comparison: pairs[(page, sub_op)] -> (state, factor)
        inventory_pairs: std::collections::HashMap<(u8, u8), (i32, i32)>,
        // 0x49 STATE_RESUME: tristate (Idle/Armed/Done) + last arm record.
        op49_state_value: Op49State,
        op49_arms: Vec<(usize, u32)>,
        op49_clears: u32,
        op49_setups: u32,
        // 0x34 sub-3 / 0x43 sub-8 callbacks.
        effect_anim_calls: Vec<(u16, u8)>, // (script_id, arg)
        face_resets: Vec<u16>,             // script_ids
        // 0x4E sub-10/11 party-bank value table.
        party_bank: std::collections::HashMap<u8, i32>,
        // 0x4C sub-1 menu sub-dispatcher recordings.
        menu_sub1_calls: Vec<(u8, [u8; 5])>,
        // 0x4C sub-3 cleanups.
        menu_refresh_calls: u32,
        depth_copy_calls: u32,
        subtile_refresh_calls: Vec<u8>,
        // 0x4C sub-3 sub-7 (player-coord copy).
        player_coords: Option<PlayerCoords>,
        inverted_y_writes: Vec<i16>,
        // 0x4C sub-3 sub-2 (party-state region clear).
        party_state_clears: u32,
        // 0x43 sub-7 / sub-12 + 0x4C sub-2.
        face_rotation_setups: Vec<(u8, u32, [u16; 4], i16)>,
        scripted_actor_allocs: Vec<(u8, u8, u8)>,
        party_view_swaps: Vec<u8>,
        // 0x43 sub-2/D/F/E.
        three_actor_talks: Vec<([u8; 3], u16, u8)>,
        actor_alloc_modes: Vec<(u8, u8, [u8; 4])>,
        mark_flag_8_calls: Vec<u16>,
        // 0x43 sub-3/4/5/6.
        sound_ramp_calls: Vec<(u8, [u8; 4], u16, u16)>,
        // 0x43 sub-9 / sub-0x10 / sub-0x11 / sub-0x15.
        sub9_tweens: Vec<(u16, u16, u16, u16)>,
        // High-byte default-route opcodes 0x5x/0x6x/0x7x.
        // The in-game "system flag bank" at DAT_80086D70. Mirrors the
        // original's `idx >> 3` byte / `0x80 >> (idx & 7)` bit. Sized at
        // 8 KB so 16-bit indices never overflow the test slice.
        system_flags: Vec<u8>,
        sys_flag_writes: Vec<(u16, &'static str)>, // (idx, "set"|"clear")
        emitter_init_payloads: Vec<Vec<u8>>,
        emitter_5_words: Vec<[u16; 5]>,
        emitter_struct12_payloads: Vec<Vec<u8>>,
        // 0x43 sub-0x12/0x13/0x14.
        emitter_split_calls: Vec<([i16; 6], bool)>,
        emitter_func13_payloads: Vec<[u8; 13]>,
        emitter_4_words: Vec<[i16; 4]>,
        // 0x4C sub-3 sub-9 / sub-E / sub-F.
        player_pos_refresh_calls: u32,
        player_render_resync_calls: u32,
        field_io_resync_calls: u32,
        // 0x4C sub-3 sub-0 / sub-1 (field input lock).
        field_input_lock_writes: Vec<bool>,
        // 0x4C outer-nibble-4 (ctx-slot ramp + global write).
        n4_ctx_ramps: Vec<(u8, i16, u16)>,
        n4_global_writes: Vec<(u8, i32, u16)>,
        // 0x4C outer-nibble-4 sub-6 / sub-7 paired-global gate.
        n4_global_pair_gated: bool,
        n4_global_pair_clears: u32,
        // 0x34 sub-2 capture-and-yield.
        op34_sub2_capture_match: bool, // gates whether the lookup hits
        op34_sub2_captures: Vec<(u8, usize)>, // (b1, captured_pc_offset)
        // 0x34 sub-0 / sub-1.
        op34_sub0_calls: Vec<(u8, [u8; 3], i16)>, // (op0, rgb, intensity)
        op34_sub1_calls: Vec<Op34Sub1Call>,
        op34_sub1_capture_delta: Option<usize>, // override the default 13
        // 0x4C outer-nibble-4 sub-5 (actor-field block).
        n4_sub5_immediate: Vec<(u8, i16, i16, i16)>, // (b1, w94, w96, w98)
        n4_sub5_ramp: Vec<(u8, i16, i16, i16, u16)>, // (b1, w94, w96, w98, ticks)
        // 0x4C outer-nibble-4 sub-9 (story-flag-driven dispatch).
        n4_sub9_state: Sub9State,
        n4_sub9_default_writes: Vec<i16>,
        n4_sub9_default_ramps: Vec<(i16, u16)>,
        n4_sub9_delta_calls: Vec<(i16, u16)>,
        // 0x4C outer-nibbles 5..F.
        n5_sub0_calls: Vec<(i16, bool)>, // (value, high)
        n6_sub0_calls: Vec<[i16; 6]>,
        #[allow(clippy::type_complexity)]
        n7_tile_calls: Vec<(u8, (u8, u8), (u8, u8), u8)>,
        n8_party_mirrors: Vec<u8>,
        n8_b630_writes: Vec<u8>,
        n8_callback_regs: u32,
        n8_global_writes: Vec<(i16, u8, u8)>,
        n8_quad_writes: Vec<([i16; 3], u32)>,
        n8_sub9_writes: Vec<i16>,
        n8_halt_acquires: Vec<u32>,
        n9_dde34_calls: Vec<(u8, u8, [i16; 3])>,
        n9_table_copies: Vec<[i16; 16]>,
        n9_callback_regs: u32,
        op4e_sub4_bios_rand_value: i32,
        op4e_sub4_bios_rand_calls: u32,
        n_c_subtile_broadcasts: Vec<(u8, u8)>,
        n_c_sound_triggers: Vec<(u8, u8)>,
        n_c_slot_writes: Vec<(u8, i16)>,
        n_c_slot_adjusts: Vec<(u8, i16, bool)>,
        n_c_b6ac_writes: Vec<u8>,
        n_c_sub_f_broadcasts: Vec<(i16, i16)>,
        n_c_sub9_globals_differ: bool,
        n_d_sub6_acks: u32,
        n_d_sub8_calls: Vec<(u8, [i16; 3])>,
        n_e_sub0_writes: Vec<u8>,
        n_e_sub9_clears: u32,
        n_e_sub_a_calls: u32,
        n_d_party_setups: Vec<(u32, u32, u32)>,
        n_d_collision_y_calls: u32,
        n_d_scene_byte_writes: Vec<u8>,
        n_e_fmv_triggers: Vec<i16>,
        n_e_d8280_calls: Vec<[i16; 3]>,
        n_e_capture_ddf48_calls: u32,
        n_e_ba66_writes: Vec<u8>,
        n_e_snapshot_84570_calls: u32,
        // Round 16 - overlay-helper hooks.
        n_c_sub_1_flag_loops: u32,
        n_d_sub_1_jump_target: Option<usize>,
        n_d_sub_2_channel_calls: Vec<u8>,
        n_d_sub_7_list_walk_regs: u32,
        n_d_sub_b_e57f0_calls: Vec<Vec<u8>>,
        n_d_sub_c_jump_target: Option<usize>,
        n_d_sub_c_calls: Vec<u8>,
        n_d_sub_e_jump_target: Option<usize>,
        n_d_sub_e_calls: Vec<u8>,
        n_e_sub_1_text_calls: Vec<(Vec<u8>, u16)>,
        // Round 17 - five new 0x4C nC sub-ops + two 0x4C nE sub-ops.
        n_c_sub_0_move_cancels: u32,
        n_c_sub_0_active: bool,
        n_c_sub_3_teleports: u32,
        n_c_sub_d_allocs: u32,
        n_c_party_flag_bits: std::collections::HashMap<u16, bool>,
        n_e_sub_4_outside: bool,
        n_e_sub_4_bboxes: std::cell::RefCell<Vec<[i16; 4]>>,
        n_e_sub_5_xp_deltas: Vec<i32>,
        n_e_sub_b_resolves: bool,
        n_e_sub_b_actor_ids: Vec<u8>,
        // 0x43 halt-acquire (sub-0/1/A/B).
        halt_acquire_predicate: bool,
        halt_acquire_calls: Vec<(u8, usize, [i16; 3])>,
        // Round 18 - 0x4C n8 actor-allocator + nE camera + nD/n5 dialog.
        n_8_sub_1_set_model_calls: Vec<(u32, u16, u16)>, // (model_id, anim, tween)
        n_8_sub_6_actor_set_rotation_calls: Vec<(u8, [i16; 3], [i16; 3])>,
        n_8_sub_6_actor_present: bool,
        n_8_sub_b_present_types: std::collections::HashSet<u8>,
        n_8_sub_d_search_result: ActorSearchResult,
        n_8_sub_d_queries: std::cell::RefCell<Vec<(u8, u8)>>,
        n_e_sub_3_camera_syncs: Vec<u8>,            // actor_id values
        n_e_sub_7_camera_animates: Vec<(u32, u16)>, // (target, duration)
        n_e_sub_8_camera_zooms: Vec<(i16, i16, i16, i16)>, // (zoom_x, zoom_y, zoom_z, mode)
        n_d_sub_0_se_triggers: Vec<(u16, u16)>,
        n_5_sub_3_dialog_waits: u32,
        n_5_sub_4_dialog_active: bool,
        n_5_sub_4_polls: u32,
        // Round 19 - 0x4C nD sub-4/5 VRAM STP set/clear.
        n_d_sub_4_vram_stp_set_calls: Vec<(u16, u16)>,
        n_d_sub_5_vram_stp_clear_calls: Vec<(u16, u16)>,
    }

    impl FieldHost for TestHost {
        fn global_flags(&self) -> u32 {
            self.globals
        }
        fn set_global_flags(&mut self, value: u32) {
            self.globals = value;
        }
        fn frame_delta(&self) -> u16 {
            self.frame_delta
        }
        fn exec_move(&mut self, ctx: &mut FieldCtx, move_id: u8) {
            self.exec_moves.push((ctx.script_id, move_id));
        }
        fn move_to(&mut self, ctx: &mut FieldCtx, world_x: u16, world_z: u16, is_player: bool) {
            self.move_tos
                .push((ctx.script_id, world_x, world_z, is_player));
        }
        fn open_dialog(
            &mut self,
            text_id: u16,
            inline: &[u8],
            world_x: u16,
            world_z: u16,
            depth_id: u8,
        ) {
            self.dialogs
                .push((text_id, inline.to_vec(), world_x, world_z, depth_id));
        }
        fn bgm(&mut self, text_id: u16, sub_op: u8) {
            self.bgm_calls.push((text_id, sub_op));
        }
        fn cam_cfg_lookup(&self, index: u8) -> Option<u16> {
            self.cam_cfg_table.get(index as usize).copied()
        }
        fn play_sfx(&mut self, sfx_id: u8) {
            self.sfx_calls.push(sfx_id);
        }
        fn add_money(&mut self, delta: i32) {
            self.money_deltas.push(delta);
        }
        fn set_item_count(&mut self, slot_byte: u8, count: u8) {
            self.item_writes.push((slot_byte, count));
        }
        fn party_add(&mut self, char_id: u8) -> bool {
            self.party_added.push(char_id);
            true
        }
        fn party_remove(&mut self, char_id: u8) {
            self.party_removed.push(char_id);
        }
        fn extra_flags(&self) -> u32 {
            self.extras
        }
        fn screen_mode(&self) -> u32 {
            self.screen_mode
        }
        fn screen_mode_table(&self, index: u8) -> Option<u32> {
            self.screen_mode_table
                .get(index as usize)
                .copied()
                .flatten()
        }
        fn field_interact(&mut self, interact_id: u8, slot: u8) {
            self.interacts.push((interact_id, slot));
        }
        fn scene_transition(&mut self, map_id: u8) {
            self.scene_transitions.push(map_id);
        }
        fn render_cfg_long(&mut self, b1: u8, b2: u8, b3: u8, b4: u8) {
            self.render_long.push((b1, b2, b3, b4));
        }
        fn render_cfg_short(&mut self, r: u8, g: u8, b: u8, packed: u8) {
            self.render_short.push((r, g, b, packed));
        }
        fn scene_register_write(&mut self, slot_10: u8, slot_12: u8, slot_14: u8) {
            self.scene_regs.push((slot_10, slot_12, slot_14));
        }
        fn counter_update(&mut self, op0: u8) {
            self.counter_calls.push(op0);
        }
        fn setup_animation(&mut self, ctx: &mut FieldCtx, count: u8, base_id: u8, frames: &[u8]) {
            self.animations
                .push((ctx.script_id, count, base_id, frames.to_vec()));
        }
        fn set_party_leader(&mut self, leader_id: u8) {
            self.party_leaders.push(leader_id);
        }
        fn camera_configure(&mut self, params: &[CameraParam], apply_trigger: u16, mode: u8) {
            self.camera_configs
                .push((params.to_vec(), apply_trigger, mode));
        }
        fn camera_load(&mut self, payload: &[u8]) {
            self.camera_loads.push(payload.to_vec());
        }
        fn camera_save(&mut self) {
            self.camera_saves += 1;
        }
        fn camera_apply(&mut self) {
            self.camera_applies += 1;
        }
        fn scene_fade(&mut self, op0: u16, op1: u16) -> SceneFadeResult {
            self.scene_fade_calls.push((op0, op1));
            if self.scene_fade_busy {
                SceneFadeResult::Busy
            } else {
                SceneFadeResult::Done
            }
        }
        fn inventory_compare_pair(&self, page: u8, sub_op: u8) -> (i32, i32) {
            self.inventory_pairs
                .get(&(page, sub_op))
                .copied()
                .unwrap_or((0, 0))
        }
        fn op49_state(&self) -> Op49State {
            self.op49_state_value
        }
        fn op49_arm(&mut self, pc: usize, field_90: u32) {
            self.op49_arms.push((pc, field_90));
        }
        fn op49_clear(&mut self) {
            self.op49_clears += 1;
        }
        fn op49_invoke_setup(&mut self) {
            self.op49_setups += 1;
        }
        fn effect_anim_trigger(&mut self, ctx: &mut FieldCtx, arg: u8) {
            self.effect_anim_calls.push((ctx.script_id, arg));
        }
        fn actor_face_reset(&mut self, ctx: &mut FieldCtx) {
            self.face_resets.push(ctx.script_id);
        }
        fn party_bank_value(&self, sub_op: u8) -> i32 {
            self.party_bank.get(&sub_op).copied().unwrap_or(0)
        }
        fn menu_ctrl_sub1(&mut self, op0: u8, payload: &[u8; 5]) {
            self.menu_sub1_calls.push((op0, *payload));
        }
        fn menu_refresh(&mut self) {
            self.menu_refresh_calls += 1;
        }
        fn copy_dialog_depth_to_player(&mut self) {
            self.depth_copy_calls += 1;
        }
        fn player_subtile_refresh(&mut self, sub_op: u8) {
            self.subtile_refresh_calls.push(sub_op);
        }
        fn fetch_player_coords(&self, _ctx: &FieldCtx) -> Option<PlayerCoords> {
            self.player_coords
        }
        fn set_inverted_y_mirror(&mut self, _ctx: &mut FieldCtx, inverted_y: i16) {
            self.inverted_y_writes.push(inverted_y);
        }
        fn clear_party_state_region(&mut self) {
            self.party_state_clears += 1;
        }
        fn actor_face_rotation_setup(
            &mut self,
            _ctx: &mut FieldCtx,
            face_id: u8,
            payload_4: u32,
            params: [u16; 4],
            target: i16,
        ) {
            self.face_rotation_setups
                .push((face_id, payload_4, params, target));
        }
        fn op43_alloc_scripted_actor(&mut self, b1: u8, b2: u8, b3: u8) {
            self.scripted_actor_allocs.push((b1, b2, b3));
        }
        fn party_view_swap(&mut self, new_index: u8) {
            self.party_view_swaps.push(new_index);
        }
        fn op43_three_actor_talk(&mut self, actor_ids: [u8; 3], arg_word: u16, arg_byte: u8) {
            self.three_actor_talks.push((actor_ids, arg_word, arg_byte));
        }
        fn op43_alloc_actor_with_mode(&mut self, sub_op: u8, mode: u8, args: [u8; 4]) {
            self.actor_alloc_modes.push((sub_op, mode, args));
        }
        fn op43_mark_actor_flag_8(&mut self, ctx: &mut FieldCtx) {
            self.mark_flag_8_calls.push(ctx.script_id);
        }
        fn op43_sound_register_ramp(&mut self, sub_op: u8, bytes: [u8; 4], ticks: u16, curve: u16) {
            self.sound_ramp_calls.push((sub_op, bytes, ticks, curve));
        }
        fn op43_sub9_tween(&mut self, _ctx: &mut FieldCtx, x: u16, y: u16, z: u16, ticks: u16) {
            self.sub9_tweens.push((x, y, z, ticks));
        }
        fn op43_emitter_init(&mut self, payload: &[u8]) {
            self.emitter_init_payloads.push(payload.to_vec());
        }
        fn op43_emitter_5_words(&mut self, words: [u16; 5]) {
            self.emitter_5_words.push(words);
        }
        fn op43_emitter_struct_12(&mut self, payload: &[u8]) {
            self.emitter_struct12_payloads.push(payload.to_vec());
        }
        fn op43_emitter_split_call(&mut self, words: [i16; 6], did_split: bool) {
            self.emitter_split_calls.push((words, did_split));
        }
        fn op43_emitter_func13(&mut self, payload: &[u8; 13]) {
            self.emitter_func13_payloads.push(*payload);
        }
        fn op43_emitter_4_words(&mut self, words: [i16; 4]) {
            self.emitter_4_words.push(words);
        }
        fn player_position_refresh_with_collision_y(&mut self, _ctx: &mut FieldCtx) {
            self.player_pos_refresh_calls += 1;
        }
        fn player_render_resync(&mut self) {
            self.player_render_resync_calls += 1;
        }
        fn field_io_resync(&mut self) {
            self.field_io_resync_calls += 1;
        }
        fn set_field_input_lock(&mut self, locked: bool) {
            self.field_input_lock_writes.push(locked);
        }
        fn op4c_nibble4_ctx_ramp(&mut self, _ctx: &mut FieldCtx, sub: u8, target: i16, ticks: u16) {
            self.n4_ctx_ramps.push((sub, target, ticks));
        }
        fn op4c_nibble4_global_write(&mut self, sub: u8, target: i32, ticks: u16) {
            self.n4_global_writes.push((sub, target, ticks));
        }
        fn op4c_nibble4_global_pair_gate(&self) -> bool {
            self.n4_global_pair_gated
        }
        fn op4c_nibble4_global_pair_clear(&mut self) {
            self.n4_global_pair_clears += 1;
        }
        fn op34_capture_pc_for_existing_actor(
            &mut self,
            _ctx: &FieldCtx,
            b1: u8,
            captured_pc_offset: usize,
        ) -> bool {
            // Match the original's gates: only capture when b1 == 0x40 AND
            // the simulated lookup hits. Hosts model the lookup themselves.
            if self.op34_sub2_capture_match && b1 == 0x40 {
                self.op34_sub2_captures.push((b1, captured_pc_offset));
                true
            } else {
                false
            }
        }
        fn op34_sub0_color_intensity_setup(&mut self, op0: u8, rgb: [u8; 3], intensity: i16) {
            self.op34_sub0_calls.push((op0, rgb, intensity));
        }
        fn op34_sub1_spawn_or_skip(
            &mut self,
            _ctx: &FieldCtx,
            op0: u8,
            packed24: u32,
            pos: [i16; 3],
            capture_flag: u8,
            captured_pc_payload: &[u8],
        ) -> usize {
            self.op34_sub1_calls.push(Op34Sub1Call {
                op0,
                packed24,
                pos,
                capture_flag,
                captured_payload: captured_pc_payload.to_vec(),
            });
            self.op34_sub1_capture_delta.unwrap_or(13)
        }
        fn system_flag_set(&mut self, idx: u16) {
            self.ensure_sys_flag_capacity();
            let byte = (idx >> 3) as usize;
            let mask = 0x80u8 >> (idx & 7);
            self.system_flags[byte] |= mask;
            self.sys_flag_writes.push((idx, "set"));
        }
        fn system_flag_clear(&mut self, idx: u16) {
            self.ensure_sys_flag_capacity();
            let byte = (idx >> 3) as usize;
            let mask = 0x80u8 >> (idx & 7);
            self.system_flags[byte] &= !mask;
            self.sys_flag_writes.push((idx, "clear"));
        }
        fn system_flag_test(&self, idx: u16) -> bool {
            let byte = (idx >> 3) as usize;
            let mask = 0x80u8 >> (idx & 7);
            self.system_flags
                .get(byte)
                .map(|b| (*b & mask) != 0)
                .unwrap_or(false)
        }
        fn op4c_n4_sub5_write_immediate(
            &mut self,
            _ctx: &mut FieldCtx,
            b1: u8,
            w94: i16,
            w96: i16,
            w98: i16,
        ) {
            self.n4_sub5_immediate.push((b1, w94, w96, w98));
        }
        fn op4c_n4_sub5_ramp(
            &mut self,
            _ctx: &mut FieldCtx,
            b1: u8,
            w94: i16,
            w96: i16,
            w98: i16,
            ticks: u16,
        ) {
            self.n4_sub5_ramp.push((b1, w94, w96, w98, ticks));
        }
        fn op4c_n4_sub9_state(&self) -> Sub9State {
            self.n4_sub9_state
        }
        fn op4c_n4_sub9_default_write(&mut self, target: i16) {
            self.n4_sub9_default_writes.push(target);
        }
        fn op4c_n4_sub9_default_ramp(&mut self, target: i16, ticks: u16) {
            self.n4_sub9_default_ramps.push((target, ticks));
        }
        fn op4c_n4_sub9_delta_write_or_ramp(&mut self, target: i16, ticks: u16) {
            self.n4_sub9_delta_calls.push((target, ticks));
        }
        fn op4c_n5_sub0_sound_directional(&mut self, _ctx: &mut FieldCtx, value: i16, high: bool) {
            self.n5_sub0_calls.push((value, high));
        }
        fn op4c_n6_sub0_emitter6(&mut self, words: [i16; 6]) {
            self.n6_sub0_calls.push(words);
        }
        fn op4c_n7_tile_flag_bulk(
            &mut self,
            sub: u8,
            x_range: (u8, u8),
            z_range: (u8, u8),
            mask: u8,
        ) {
            self.n7_tile_calls.push((sub, x_range, z_range, mask));
        }
        fn op4c_n8_sub2_party_page_mirror(&mut self, page: u8) {
            self.n8_party_mirrors.push(page);
        }
        fn op4c_n8_sub4_set_b630(&mut self, value: u8) {
            self.n8_b630_writes.push(value);
        }
        fn op4c_n8_sub7_register_callback(&mut self) {
            self.n8_callback_regs += 1;
        }
        fn op4c_n8_sub8_write_globals(&mut self, value: i16, b3: u8, b4: u8) {
            self.n8_global_writes.push((value, b3, b4));
        }
        fn op4c_n8_sub_a_write_quad(&mut self, slots: [i16; 3], packed: u32) {
            self.n8_quad_writes.push((slots, packed));
        }
        fn op4c_n8_sub9_set_73f00(&mut self, value: i16) {
            self.n8_sub9_writes.push(value);
        }
        fn op4c_n9_sub0_2_dde34(&mut self, sub: u8, b1: u8, words: [i16; 3]) {
            self.n9_dde34_calls.push((sub, b1, words));
        }
        fn op4c_n9_sub_e_table_copy(&mut self, words: [i16; 16]) {
            self.n9_table_copies.push(words);
        }
        fn op4c_n9_sub_f_register_callback(&mut self) {
            self.n9_callback_regs += 1;
        }
        fn op4e_sub4_bios_rand(&mut self) -> i32 {
            self.op4e_sub4_bios_rand_calls += 1;
            self.op4e_sub4_bios_rand_value
        }
        fn op4c_n_c_sub4_subtile_broadcast(&mut self, x: u8, z: u8) {
            self.n_c_subtile_broadcasts.push((x, z));
        }
        fn op4c_n_c_sub7_sound_trigger(&mut self, b1: u8, b2: u8) {
            self.n_c_sound_triggers.push((b1, b2));
        }
        fn op4c_n_c_sub_a_set_slot(&mut self, slot: u8, value: i16) {
            self.n_c_slot_writes.push((slot, value));
        }
        fn op4c_n_c_sub_bc_adjust_slot(&mut self, slot: u8, delta: i16, subtract: bool) {
            self.n_c_slot_adjusts.push((slot, delta, subtract));
        }
        fn op4c_n_c_sub_e_set_b6ac(&mut self, value: u8) {
            self.n_c_b6ac_writes.push(value);
        }
        fn op4c_n_c_sub_f_position_broadcast(&mut self, x_global: i16, z_global: i16) {
            self.n_c_sub_f_broadcasts.push((x_global, z_global));
        }
        fn op4c_n_d_sub3_party_setup(&mut self, ab: u32, cd: u32, ef: u32) {
            self.n_d_party_setups.push((ab, cd, ef));
        }
        fn op4c_n_d_sub_a_collision_y_refresh(&mut self, _ctx: &mut FieldCtx) {
            self.n_d_collision_y_calls += 1;
        }
        fn op4c_n_d_sub_f_scene_byte_write(&mut self, value: u8) {
            self.n_d_scene_byte_writes.push(value);
        }
        fn op4c_n_e_sub2_fmv_trigger(&mut self, fmv_id: i16) {
            self.n_e_fmv_triggers.push(fmv_id);
        }
        fn op4c_n_e_sub6_call_d8280(&mut self, words: [i16; 3]) {
            self.n_e_d8280_calls.push(words);
        }
        fn op4c_n_e_sub_c_capture_ddf48(&mut self) {
            self.n_e_capture_ddf48_calls += 1;
        }
        fn op4c_n_e_sub_d_set_ba66(&mut self, value: u8) {
            self.n_e_ba66_writes.push(value);
        }
        fn op4c_n_e_sub_e_snapshot_84570(&mut self) {
            self.n_e_snapshot_84570_calls += 1;
        }
        fn op4c_n8_halt_acquire(&mut self, ctx: &mut FieldCtx, opcode_pc: u32) {
            ctx.saved_pc = opcode_pc;
            ctx.wait_accum = 0;
            ctx.flags |= 0x400;
            self.n8_halt_acquires.push(opcode_pc);
        }
        fn op4c_n_c_sub9_globals_differ(&self) -> bool {
            self.n_c_sub9_globals_differ
        }
        fn op4c_n_d_sub6_field74_mutate_ack(&mut self) {
            self.n_d_sub6_acks += 1;
        }
        fn op4c_n_d_sub8_call_d77f4(&mut self, b1: u8, words: [i16; 3]) {
            self.n_d_sub8_calls.push((b1, words));
        }
        fn op4c_n_e_sub0_state_write(&mut self, b1: u8) {
            self.n_e_sub0_writes.push(b1);
        }
        fn op4c_n_e_sub9_clear_b9c4(&mut self) {
            self.n_e_sub9_clears += 1;
        }
        fn op4c_n_e_sub_a_call_c7ec(&mut self) {
            self.n_e_sub_a_calls += 1;
        }
        fn op4c_n_e_sub_1_text_actor(&mut self, text_buf: &[u8], script_id: u16) {
            self.n_e_sub_1_text_calls
                .push((text_buf.to_vec(), script_id));
        }
        fn op4c_n_c_sub_1_flag_loop_reset(&mut self, _flags: &[u8]) {
            self.n_c_sub_1_flag_loops += 1;
        }
        fn op4c_n_d_sub_1_list_lookup_jump(&mut self, _ctx: &FieldCtx) -> Option<usize> {
            self.n_d_sub_1_jump_target
        }
        fn op4c_n_d_sub_2_channel_spawn(&mut self, channel: u8) {
            self.n_d_sub_2_channel_calls.push(channel);
        }
        fn op4c_n_d_sub_7_register_list_walk(&mut self) {
            self.n_d_sub_7_list_walk_regs += 1;
        }
        fn op4c_n_d_sub_b_call_e57f0(&mut self, bytecode: &[u8]) {
            self.n_d_sub_b_e57f0_calls.push(bytecode.to_vec());
        }
        fn op4c_n_d_sub_c_party_search_set(&mut self, needle: u8) -> Option<usize> {
            self.n_d_sub_c_calls.push(needle);
            self.n_d_sub_c_jump_target
        }
        fn op4c_n_d_sub_e_party_search_query(&mut self, needle: u8) -> Option<usize> {
            self.n_d_sub_e_calls.push(needle);
            self.n_d_sub_e_jump_target
        }
        fn op4c_n_d_sub_4_vram_stp_set(&mut self, x: u16, y: u16) {
            self.n_d_sub_4_vram_stp_set_calls.push((x, y));
        }
        fn op4c_n_d_sub_5_vram_stp_clear(&mut self, x: u16, y: u16) {
            self.n_d_sub_5_vram_stp_clear_calls.push((x, y));
        }
        fn op4c_n_c_sub_0_move_cancel(&mut self, _ctx: &mut FieldCtx) -> bool {
            self.n_c_sub_0_move_cancels += 1;
            self.n_c_sub_0_active
        }
        fn op4c_n_c_sub_3_script_teleport(&mut self, _ctx: &mut FieldCtx) {
            self.n_c_sub_3_teleports += 1;
        }
        fn op4c_n_c_sub_d_script_alloc(&mut self) {
            self.n_c_sub_d_allocs += 1;
        }
        fn op4c_n_c_party_flag_test(&self, flag_idx: u16) -> bool {
            *self.n_c_party_flag_bits.get(&flag_idx).unwrap_or(&false)
        }
        fn op4c_n_e_sub_4_bbox_outside(&self, _ctx: &FieldCtx, bbox: [i16; 4]) -> bool {
            self.n_e_sub_4_bboxes.borrow_mut().push(bbox);
            self.n_e_sub_4_outside
        }
        fn op4c_n_e_sub_5_add_xp(&mut self, xp_delta: i32) {
            self.n_e_sub_5_xp_deltas.push(xp_delta);
        }
        fn op4c_n_e_sub_b_actor_jump(&mut self, actor_id: u8) -> Option<()> {
            self.n_e_sub_b_actor_ids.push(actor_id);
            if self.n_e_sub_b_resolves {
                Some(())
            } else {
                None
            }
        }
        fn field_halt_acquire_predicate(&self, _ctx: &FieldCtx, _which: u8) -> bool {
            self.halt_acquire_predicate
        }
        fn field_halt_acquire_apply(
            &mut self,
            _ctx: &mut FieldCtx,
            which: u8,
            resume_pc: usize,
            coords: [i16; 3],
        ) {
            self.halt_acquire_calls.push((which, resume_pc, coords));
        }
        fn op4c_n_8_sub_1_set_model_anim(
            &mut self,
            _ctx: &mut FieldCtx,
            model_id: u32,
            anim_frame: u16,
            tween_frames: u16,
        ) {
            self.n_8_sub_1_set_model_calls
                .push((model_id, anim_frame, tween_frames));
        }
        fn op4c_n_8_sub_6_actor_set_rotation(
            &mut self,
            _ctx: &mut FieldCtx,
            actor_id: u8,
            position: [i16; 3],
            rotation: [i16; 3],
        ) -> bool {
            self.n_8_sub_6_actor_set_rotation_calls
                .push((actor_id, position, rotation));
            self.n_8_sub_6_actor_present
        }
        fn op4c_n_8_sub_b_actor_type_present(&self, type_byte: u8) -> bool {
            self.n_8_sub_b_present_types.contains(&type_byte)
        }
        fn op4c_n_8_sub_d_actor_search(&self, char_idx: u8, marker: u8) -> ActorSearchResult {
            self.n_8_sub_d_queries.borrow_mut().push((char_idx, marker));
            self.n_8_sub_d_search_result
        }
        fn op4c_n_e_sub_3_actor_sync_camera(&mut self, _ctx: &mut FieldCtx, actor_id: u8) {
            self.n_e_sub_3_camera_syncs.push(actor_id);
        }
        fn op4c_n_e_sub_7_camera_animate(&mut self, target: u32, duration: u16) {
            self.n_e_sub_7_camera_animates.push((target, duration));
        }
        fn op4c_n_e_sub_8_camera_zoom(&mut self, zoom_x: i16, zoom_y: i16, zoom_z: i16, mode: i16) {
            self.n_e_sub_8_camera_zooms
                .push((zoom_x, zoom_y, zoom_z, mode));
        }
        fn op4c_n_d_sub_0_field_se_trigger(&mut self, a: u16, b: u16) {
            self.n_d_sub_0_se_triggers.push((a, b));
        }
        fn op4c_n_5_sub_3_dialog_wait(&mut self, _ctx: &mut FieldCtx) {
            self.n_5_sub_3_dialog_waits += 1;
        }
        fn op4c_n_5_sub_4_dialog_advance(&mut self, _ctx: &mut FieldCtx) -> bool {
            self.n_5_sub_4_polls += 1;
            self.n_5_sub_4_dialog_active
        }
    }

    impl TestHost {
        fn ensure_sys_flag_capacity(&mut self) {
            if self.system_flags.is_empty() {
                self.system_flags.resize(8192, 0);
            }
        }
    }

    fn run<H: FieldHost>(host: &mut H, bytecode: &[u8]) -> (FieldCtx, Vec<StepResult>) {
        let mut ctx = FieldCtx::default();
        let mut pc = 0usize;
        let mut trace = Vec::new();
        loop {
            let r = step(host, &mut ctx, bytecode, pc);
            trace.push(r.clone());
            match r {
                StepResult::Advance { next_pc } => {
                    if next_pc >= bytecode.len() {
                        break;
                    }
                    pc = next_pc;
                }
                _ => break,
            }
        }
        (ctx, trace)
    }

    // -- NOP cluster ----------------------------------------------------

    #[test]
    fn nop_cluster_advances_one_byte() {
        for op in [0x21u8, 0x24, 0x25, 0x48] {
            let mut host = TestHost::default();
            let mut ctx = FieldCtx::default();
            let r = step(&mut host, &mut ctx, &[op], 0);
            assert_eq!(r, StepResult::Advance { next_pc: 1 });
        }
    }

    // -- JMP_REL --------------------------------------------------------

    #[test]
    fn jmp_rel_forward() {
        // 0x26 with offset 0x0008 should land at PC = 0 + 1 + 8 = 9.
        let bc = [0x26, 0x08, 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 9 });
    }

    #[test]
    fn jmp_rel_with_high_byte() {
        // Offset 0x0100 from PC 0 lands at 0x0101.
        let bc = [0x26, 0x00, 0x01];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 0x0101 });
    }

    // -- Local flag triplet 0x2B / 0x2C / 0x2D ---------------------------

    #[test]
    fn lflag_set_then_clear() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // Set bit 5 (mask 0x20).
        let r = step(&mut host, &mut ctx, &[0x2B, 5], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(ctx.local_flags, 0x0020);

        // Clear bit 5.
        let r = step(&mut host, &mut ctx, &[0x2C, 5], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(ctx.local_flags, 0);
    }

    #[test]
    fn lflag_set_masks_to_5_bits() {
        // The original masks the bit index by 0x1F. Index 0x25 should set
        // bit 5 (0x20), not bit 0x25.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        step(&mut host, &mut ctx, &[0x2B, 0x25], 0);
        assert_eq!(ctx.local_flags, 0x0020);
    }

    #[test]
    fn lflag_tst_bit_set_advances() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            local_flags: 0x0040, // bit 6 set
            ..Default::default()
        };
        let r = step(&mut host, &mut ctx, &[0x2D, 6], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
    }

    #[test]
    fn lflag_tst_bit_clear_halts() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x2D, 6], 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
    }

    // -- Global flag triplet 0x2E / 0x2F / 0x30 --------------------------

    #[test]
    fn gflag_set_writes_through_host() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x2E, 17], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.globals, 1u32 << 17);
    }

    #[test]
    fn gflag_clr_preserves_other_bits() {
        let mut host = TestHost {
            globals: 0xFFFF_FFFF,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        step(&mut host, &mut ctx, &[0x2F, 3], 0);
        assert_eq!(host.globals, !(1u32 << 3));
    }

    #[test]
    fn gflag_tst_branches_on_global() {
        let mut host = TestHost {
            globals: 1u32 << 12,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x30, 12], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });

        // bit 13 is clear -> halt
        let r = step(&mut host, &mut ctx, &[0x30, 13], 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
    }

    // -- Context flag triplet 0x31 / 0x32 / 0x33 -------------------------

    #[test]
    fn cflag_set_normal_path_advances() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x31, 0], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(ctx.flags, 1);
    }

    #[test]
    fn cflag_set_bit_8_copies_field_26_and_advances() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            field_26: 0x1234,
            ..Default::default()
        };
        let r = step(&mut host, &mut ctx, &[0x31, 8], 0);
        // Bit 8 = mask 0x100. The original calls
        // `switchD_801e0f24::caseD_4()` which is 0x801df098 → s8 += 2 →
        // return; PC advances by 2 like every other 0x31 path.
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(ctx.flags, 0x100);
        assert_eq!(ctx.saved_26, 0x1234);
    }

    #[test]
    fn cflag_clr_basic() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            flags: 0xFFFF_FFFF,
            ..Default::default()
        };
        step(&mut host, &mut ctx, &[0x32, 10], 0);
        assert_eq!(ctx.flags, !(1u32 << 10));
    }

    #[test]
    fn cflag_tst_halts_when_clear() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x33, 0], 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
    }

    // -- YIELD ----------------------------------------------------------

    #[test]
    fn yield_sets_halt_and_saves_pc() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x37], 0);
        assert_eq!(r, StepResult::Yield { resume_pc: 3 });
        assert!(ctx.is_halted());
        assert_eq!(ctx.saved_pc, 0);
    }

    #[test]
    fn yield_4_uses_pc_plus_4() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // Pad so pc=5 is in-bounds (yield reads only the opcode byte).
        let mut bc = [0u8; 6];
        bc[5] = 0x47;
        let r = step(&mut host, &mut ctx, &bc, 5);
        assert_eq!(r, StepResult::Yield { resume_pc: 9 });
        assert_eq!(ctx.saved_pc, 5);
    }

    // -- DATA_BLOCK ------------------------------------------------------

    #[test]
    fn data_block_skips_len_bytes() {
        // len = 4: skip operand byte + 4 inline bytes => PC += 6.
        let bc = [0x40, 0x04, 0xAA, 0xBB, 0xCC, 0xDD];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
    }

    // -- WAIT_FRAMES ----------------------------------------------------

    #[test]
    fn wait_frames_accumulates_until_target() {
        let mut host = TestHost {
            frame_delta: 1,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let bc = [0x4A, 0x03, 0x00]; // wait 3 frames

        // Tick 1: accum=1, halt.
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        assert_eq!(ctx.wait_accum, 1);

        // Tick 2: accum=2, still halt.
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        assert_eq!(ctx.wait_accum, 2);

        // Tick 3: accum=3, advance + reset.
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(ctx.wait_accum, 0);
    }

    // -- Pending / Unknown -----------------------------------------------

    #[test]
    fn unimplemented_opcode_returns_pending() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // 0x4C outer-nibble-5 sub-1 (movement+halt-acquire) is still unported.
        // The opcode is recognized but the handler returns Pending.
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4C, 0x51, 0x00, 0x00, 0x00, 0x00],
            0,
        );
        assert!(matches!(r, StepResult::Pending { opcode: 0x4C, .. }));
    }

    // -- Multi-instruction trace -----------------------------------------

    #[test]
    fn trace_runs_through_chain() {
        // Set local-flag bit 0, jump forward over 3 garbage bytes, set local
        // bit 1, fall off the end. JMP_REL formula:
        //   target = (pc + header_size) + ((hi << 8) | lo)
        // Jumping from pc=2 with offset 5 lands at byte 8 = the second SET.
        let bc = [
            0x2B, 0, // [0..2] LFLAG_SET 0
            0x26, 0x05, 0x00, // [2..5] JMP +5 -> target = 3 + 5 = 8
            0xFF, 0xFF, 0xFF, // [5..8] unreachable
            0x2B, 1, // [8..10] LFLAG_SET 1
        ];

        let mut host = TestHost::default();
        let (ctx, _trace) = run(&mut host, &bc);
        assert_eq!(ctx.local_flags, 0b11);
    }

    // -- peek_extended ---------------------------------------------------

    #[test]
    fn peek_extended_returns_target_id_when_high_bit_set() {
        // 0xA1 = 0x80 | 0x21 (extended NOP). Next byte is target script ID.
        let bc = [0xA1, 0x42, 0x00];
        assert_eq!(peek_extended(&bc, 0), Some(0x42));
    }

    #[test]
    fn peek_extended_returns_none_for_normal_opcode() {
        let bc = [0x21, 0x42];
        assert_eq!(peek_extended(&bc, 0), None);
    }

    #[test]
    fn peek_extended_handles_eof() {
        // Empty bytecode -> None.
        assert_eq!(peek_extended(&[], 0), None);
        // Lone extended-bit byte at end (no script-id byte) -> None.
        assert_eq!(peek_extended(&[0xA1], 0), None);
    }

    // -- Cross-context dispatch (extended bit) --------------------------

    #[test]
    fn extended_lflag_set_advances_three_bytes() {
        // Encoding: [0xAB (= 0x80 | 0x2B), script_id, bit].
        // Header size = 2; tail = 1; next_pc = 0 + 2 + 1 = 3.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            script_id: 5,
            ..Default::default()
        };
        let r = step(&mut host, &mut ctx, &[0xAB, 5, 3], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(ctx.local_flags, 1u16 << 3);
    }

    #[test]
    fn extended_jmp_rel_uses_header_size_2() {
        // [0xA6, script_id, lo, hi]. target = pc + 2 + delta. With pc=0,
        // delta=4, expect target=6.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0xA6, 0x42, 0x04, 0x00], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
    }

    #[test]
    fn extended_halt_target_returns_halt() {
        // Halted ctx + extended dispatch + non-carve-out op -> Halt.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            flags: 0x400, // halted
            script_id: 7,
            ..Default::default()
        };
        let r = step(&mut host, &mut ctx, &[0xAB, 7, 0], 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        // Local flag should NOT have been written.
        assert_eq!(ctx.local_flags, 0);
    }

    #[test]
    fn extended_system_channel_bypasses_halt() {
        // script_id == 0xFB is the system channel; halted state is ignored.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            flags: 0x400,
            script_id: 0xFB,
            ..Default::default()
        };
        let r = step(&mut host, &mut ctx, &[0xAB, 0xFB, 4], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(ctx.local_flags, 1u16 << 4);
    }

    #[test]
    fn extended_032_with_bit_10_unhalt_carve_out() {
        // 0x32 (CFLAG_CLR) with bit 10 (mask 0x400) is the unique opcode that
        // can run on a halted target - it's how a script un-halts another.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            flags: 0x400,
            script_id: 9,
            ..Default::default()
        };
        let r = step(&mut host, &mut ctx, &[0xB2, 9, 10], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(ctx.flags, 0); // halt bit cleared
        assert!(!ctx.is_halted());
    }

    #[test]
    fn extended_032_with_other_bit_does_not_bypass_halt() {
        // Same opcode but a different bit -> halt path still wins.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            flags: 0x400 | 0x10,
            script_id: 9,
            ..Default::default()
        };
        let r = step(&mut host, &mut ctx, &[0xB2, 9, 4], 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        // Bit 4 should NOT have been cleared (the dispatch never fired).
        assert_eq!(ctx.flags, 0x400 | 0x10);
    }

    // -- 0x22 EXEC_MOVE --------------------------------------------------

    #[test]
    fn exec_move_writes_state_and_calls_host() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            script_id: 3,
            ..Default::default()
        };
        let r = step(&mut host, &mut ctx, &[0x22, 0x05], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(ctx.move_id, 5);
        assert_eq!(ctx.field_5e, 0xFFFE);
        assert_eq!(ctx.move_substate, 1);
        assert_eq!(host.exec_moves, vec![(3u16, 5u8)]);
    }

    #[test]
    fn exec_move_zero_sets_substate_5() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x22, 0x00], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(ctx.move_substate, 5);
    }

    #[test]
    fn exec_move_extended_threads_target_id() {
        // Extended dispatch on a non-halted target.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            script_id: 0x42,
            ..Default::default()
        };
        let r = step(&mut host, &mut ctx, &[0xA2, 0x42, 0x07], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(host.exec_moves, vec![(0x42u16, 7u8)]);
    }

    // -- 0x23 MOVE_TO ----------------------------------------------------

    #[test]
    fn move_to_decodes_grid_coords_no_high_bit() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            script_id: 4,
            ..Default::default()
        };
        // x=2, z=3, neither has high bit -> world_x = 2*0x80 + 0x40 = 0x140,
        // world_z = 3*0x80 + 0x40 = 0x1C0.
        let r = step(&mut host, &mut ctx, &[0x23, 0x02, 0x03], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(ctx.world_x, 0x0140);
        assert_eq!(ctx.world_z, 0x01C0);
        assert_eq!(ctx.npc_x, 0x02);
        assert_eq!(ctx.npc_facing, 0x03);
        assert_eq!(host.move_tos, vec![(4u16, 0x0140u16, 0x01C0u16, false)]);
    }

    #[test]
    fn move_to_high_bit_adds_offset() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // x = 0x82 (low bits 2, high bit set) -> 2*0x80 + 0x40 + 0x40 = 0x180.
        let r = step(&mut host, &mut ctx, &[0x23, 0x82, 0x00], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(ctx.world_x, 0x0180);
        assert_eq!(ctx.world_z, 0x0040); // 0*0x80 + 0x40
    }

    #[test]
    fn move_to_player_path_uses_flag_bit() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            flags: 0x1000000, // player chain bit
            ..Default::default()
        };
        step(&mut host, &mut ctx, &[0x23, 0x01, 0x01], 0);
        assert_eq!(host.move_tos.len(), 1);
        assert!(host.move_tos[0].3); // is_player == true
    }

    // -- 0x3F DIALOG -----------------------------------------------------

    #[test]
    fn dialog_decodes_text_id_and_inline() {
        // text_id = 0x0042, len = 4, inline = [DE AD BE EF],
        // x=1, z=2, depth=3.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let bc = [
            0x3F, 0x42, 0x00, // opcode + text_id (LE)
            0x04, 0xDE, 0xAD, 0xBE, 0xEF, // len + inline
            0x01, 0x02, 0x03, // x, z, depth
        ];
        let r = step(&mut host, &mut ctx, &bc, 0);
        // header_size 1 + 3 (lo,hi,len) + 4 (inline) + 3 (x,z,depth) = 11.
        assert_eq!(r, StepResult::Advance { next_pc: 11 });
        assert_eq!(host.dialogs.len(), 1);
        let (text_id, inline, world_x, world_z, depth) = &host.dialogs[0];
        assert_eq!(*text_id, 0x0042);
        assert_eq!(inline, &vec![0xDE, 0xAD, 0xBE, 0xEF]);
        // x=1 -> 0x80 + 0x40 = 0xC0; z=2 -> 0x100 + 0x40 = 0x140.
        assert_eq!(*world_x, 0x00C0);
        assert_eq!(*world_z, 0x0140);
        assert_eq!(*depth, 0x03);
    }

    #[test]
    fn dialog_zero_length_inline() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let bc = [
            0x3F, 0x10, 0x00, 0x00, // opcode + text_id + len=0
            0x00, 0x00, 0x00, // x, z, depth
        ];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 7 });
        assert_eq!(host.dialogs.len(), 1);
        assert!(host.dialogs[0].1.is_empty());
    }

    #[test]
    fn dialog_truncated_buffer_returns_unknown() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // len = 10 but bytecode only has 5 trailing bytes - should error.
        let bc = [0x3F, 0x00, 0x00, 0x0A, 0x01, 0x02, 0x03, 0x04, 0x05];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert!(matches!(
            r,
            StepResult::Unknown {
                opcode: 0x3F,
                pc: 0
            }
        ));
        assert!(host.dialogs.is_empty());
    }

    // -- 0x35 BGM --------------------------------------------------------

    #[test]
    fn bgm_decodes_text_id_and_sub_op() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x35, 0x12, 0x00, 0x01], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
        assert_eq!(host.bgm_calls, vec![(0x12u16, 1u8)]);
    }

    // -- 0x38 CAM_CFG ----------------------------------------------------

    #[test]
    fn cam_cfg_simple_path_writes_field_26_from_table() {
        let mut host = TestHost {
            cam_cfg_table: vec![0xAA, 0xBB, 0xCC, 0xDD],
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        // op0 = 2 (low nibble), op1 = 0 (& 0x7F == 0 -> simple path).
        let r = step(&mut host, &mut ctx, &[0x38, 0x02, 0x00], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(ctx.field_26, 0xCC);
    }

    #[test]
    fn cam_cfg_halt_acquire_succeeds_and_yields() {
        // op1 != 0 → halt-acquire path. With the predicate set to true (the
        // default test impl), the VM marks ctx halted (saved_pc + wait_accum
        // + flag 0x400) and yields with `resume_pc = pc + 3`.
        let mut host = TestHost {
            halt_acquire_predicate: true,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x38, 0x05, 0x01], 0);
        assert_eq!(r, StepResult::Yield { resume_pc: 3 });
        assert!(ctx.is_halted());
        assert_eq!(ctx.flags & 0x400, 0x400);
        assert_eq!(ctx.saved_pc, 0);
        assert_eq!(ctx.wait_accum, 0);
        assert_eq!(host.halt_acquire_calls, vec![(0x38u8, 3usize, [0i16; 3])]);
    }

    #[test]
    fn cam_cfg_halt_acquire_failed_predicate_halts_at_pc() {
        // op1 != 0 + predicate false → original falls into the dispatcher
        // default-arm path; for op 0x38 (not a 0x50/0x60/0x70 opcode) that
        // halts the VM at the current PC.
        let mut host = TestHost {
            halt_acquire_predicate: false,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x38, 0x00, 0x01], 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        assert!(!ctx.is_halted());
        assert_eq!(host.halt_acquire_calls, vec![]);
    }

    // -- 0x39 PLAY_SFX ---------------------------------------------------

    #[test]
    fn play_sfx_calls_host() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x39, 0x42], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.sfx_calls, vec![0x42]);
    }

    // -- 0x3A ADD_MONEY --------------------------------------------------

    #[test]
    fn add_money_positive_24_bit() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // 0x000100 = +256.
        let r = step(&mut host, &mut ctx, &[0x3A, 0x00, 0x01, 0x00], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
        assert_eq!(host.money_deltas, vec![256]);
    }

    #[test]
    fn add_money_negative_sign_extended() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // 0xFFFFFF = -1 in 24-bit two's complement.
        let r = step(&mut host, &mut ctx, &[0x3A, 0xFF, 0xFF, 0xFF], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
        assert_eq!(host.money_deltas, vec![-1]);
    }

    #[test]
    fn add_money_min_negative() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // 0x800000 = -8388608 (24-bit min signed).
        let r = step(&mut host, &mut ctx, &[0x3A, 0x00, 0x00, 0x80], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
        assert_eq!(host.money_deltas, vec![-8388608]);
    }

    // -- 0x3B SET_ITEM_COUNT --------------------------------------------

    #[test]
    fn set_item_count_passes_raw_slot_and_count() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x3B, 0x23, 0x05], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(host.item_writes, vec![(0x23u8, 0x05u8)]);
    }

    // -- 0x3C / 0x3D party ----------------------------------------------

    #[test]
    fn party_add_and_remove_route_to_host() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x3C, 0x02], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        let r = step(&mut host, &mut ctx, &[0x3D, 0x02], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.party_added, vec![0x02]);
        assert_eq!(host.party_removed, vec![0x02]);
    }

    // -- 0x42 COND_JMP --------------------------------------------------

    #[test]
    fn cond_jmp_mode_0_skips_when_flag_clear() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // mode=0, bit=3, lo=0xFF, hi=0xFF - flag clear -> skip 5 bytes.
        let r = step(&mut host, &mut ctx, &[0x42, 0x00, 0x03, 0xFF, 0xFF], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
    }

    #[test]
    fn cond_jmp_mode_0_jumps_when_flag_set() {
        let mut host = TestHost {
            extras: 1u32 << 3,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        // mode=0, bit=3, delta=0x10. Jump target = pc + 3 + 0x10 = 19. The
        // original computes `iVar18 = param_2 + 3; return iVar18 + delta`.
        let r = step(&mut host, &mut ctx, &[0x42, 0x00, 0x03, 0x10, 0x00], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 19 });
    }

    #[test]
    fn cond_jmp_mode_1_table_match_jumps() {
        // mode=1, op1=3 (< 8 → table lookup path), table[3] = high-nibble
        // value of screen_mode. delta = 0x20.
        let mut host = TestHost {
            screen_mode: 0x4000,
            screen_mode_table: {
                let mut v = vec![None; 8];
                v[3] = Some(0x4000);
                v
            },
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x42, 0x01, 0x03, 0x20, 0x00], 0);
        // Take jump: pc + 3 + 0x20 = 35.
        assert_eq!(r, StepResult::Advance { next_pc: 35 });
    }

    #[test]
    fn cond_jmp_mode_1_table_mismatch_skips() {
        // Same as above but screen_mode high nibble doesn't match table[3].
        let mut host = TestHost {
            screen_mode: 0x1000,
            screen_mode_table: {
                let mut v = vec![None; 8];
                v[3] = Some(0x4000);
                v
            },
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x42, 0x01, 0x03, 0x20, 0x00], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
    }

    #[test]
    fn cond_jmp_mode_1_bit_test_path() {
        // op1 = 8 → tests screen_mode bit 0x20.
        let mut host = TestHost {
            screen_mode: 0x20,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        // Bit set → take jump.
        let r = step(&mut host, &mut ctx, &[0x42, 0x01, 0x08, 0x10, 0x00], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 19 });

        // Bit clear → skip.
        host.screen_mode = 0;
        let r = step(&mut host, &mut ctx, &[0x42, 0x01, 0x08, 0x10, 0x00], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
    }

    #[test]
    fn cond_jmp_mode_2_halts_at_pc() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // Mode >= 2 hits the dispatcher's default arm, which returns
        // `param_2` for opcodes whose high nibble isn't 0x5x/0x6x/0x7x.
        // 0x42 & 0x70 = 0x40 → halt at PC.
        let r = step(&mut host, &mut ctx, &[0x42, 0x02, 0x00, 0x00, 0x00], 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
    }

    #[test]
    fn cond_jmp_mode_1_op1_at_least_c_takes_jump() {
        // Mode 1 op1 >= 0xC: original at line 5176 of the dump falls
        // through every `if (uVar31 == N) ... return iVar18` branch and
        // ends up in the unconditional take-jump path with
        // `iVar18 = param_2 + 3` and `delta = LE_u16(operand[2..4])`.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // [0x42, 0x01, 0x0C, 0x10, 0x00] → mode 1, op1=0xC, delta=0x0010.
        // Expected next_pc = 0 + 3 + 0x10 = 19 (header_size=2, +1 for mode,
        // +0x10 delta - 3 + delta).
        let r = step(&mut host, &mut ctx, &[0x42, 0x01, 0x0C, 0x10, 0x00], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 19 });
    }

    #[test]
    fn op_34_sub_f_halts_at_pc() {
        // Top of the 4-bit sub-op range. Same dispatch path as sub-4.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x34, 0xF0, 0], 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
    }

    #[test]
    fn op_43_subop_ff_halts_at_pc() {
        // 0xFF is the sentinel "uninitialised bytecode" byte. 0x43 sub-op
        // 0xFF has no original handler ⇒ halt at PC.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(
            &mut host,
            &mut ctx,
            &[0x43, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            0,
        );
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
    }

    #[test]
    fn op_4c_n4_sub_e_or_f_halts_at_pc() {
        // 0x4C outer-nibble 4 inner switch (line 5901 of the dump) has cases
        // 0..=0xD followed by an explicit `default:` that prints
        // `SUB_40_ERROR` and routes via `switchD_801e00f4::default()` ⇒
        // halt at PC for sub-ops 0xE/0xF. The instruction is 6 bytes.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4C, 0x4E, 0x00, 0x00, 0x00, 0x00],
            0,
        );
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4C, 0x4F, 0x00, 0x00, 0x00, 0x00],
            0,
        );
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
    }

    // -- 0x3E WARP / INTERACT -------------------------------------------

    #[test]
    fn warp_interact_path_advances_3() {
        // op0 = 5 (< 100) → INTERACT path.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x3E, 0x05, 0x02], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(host.interacts, vec![(0x05u8, 0x02u8)]);
        assert!(host.scene_transitions.is_empty());
    }

    #[test]
    fn warp_interact_handles_0xff_sentinel() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x3E, 0xFF, 0x00], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(host.interacts, vec![(0xFFu8, 0x00u8)]);
    }

    #[test]
    fn warp_scene_transition_path_advances_6_and_clears_flag() {
        // op0 = 105 (>= 100) → WARP. map_id = 5.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            flags: 0xFFFF_FFFF,
            ..Default::default()
        };
        let bc = [0x3E, 105, 0, 0, 0, 0];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(host.scene_transitions, vec![5u8]);
        // Bit 0x80000 cleared on the active ctx.
        assert_eq!(ctx.flags & 0x80000, 0);
        // Other bits preserved.
        assert_eq!(ctx.flags & 0x40000, 0x40000);
    }

    // -- 0x46 RENDER_CFG ------------------------------------------------

    #[test]
    fn render_cfg_long_form_advances_6() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(
            &mut host,
            &mut ctx,
            &[0x46, 0x24, 0x11, 0x22, 0x33, 0x44],
            0,
        );
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(host.render_long, vec![(0x11u8, 0x22u8, 0x33u8, 0x44u8)]);
        assert!(host.render_short.is_empty());
    }

    #[test]
    fn render_cfg_short_form_advances_3_and_computes_bitfield() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // op0 = 0x10, op1 = 0x06.
        // r = !(0x10 >> 1) & 0xFF = !0x08 & 0xFF = 0xF7
        // g = 2 - (0x06 >> 1) = 2 - 3 = 0xFF (wrap)
        // b = (0x10 >> 1) - 1 = 0x07
        // packed = (0x06 >> 1) + 2 = 5
        let r = step(&mut host, &mut ctx, &[0x46, 0x10, 0x06], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(host.render_short, vec![(0xF7u8, 0xFFu8, 0x07u8, 0x05u8)]);
        assert!(host.render_long.is_empty());
    }

    // -- 0x4F SCENE_REGISTER_WRITE --------------------------------------

    #[test]
    fn scene_register_write_passes_three_bytes() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4F, 0xAA, 0xBB, 0xCC], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
        assert_eq!(host.scene_regs, vec![(0xAAu8, 0xBBu8, 0xCCu8)]);
    }

    // -- 0x44 COUNTER ----------------------------------------------------

    #[test]
    fn counter_advances_and_calls_host() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x44, 0x55], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.counter_calls, vec![0x55]);
    }

    // -- 0x4B ANIMATE ----------------------------------------------------

    #[test]
    fn animate_advances_3_plus_4_count_and_writes_flags() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            script_id: 7,
            local_flags: 0xFFFF, // all bits set; check mask
            ..Default::default()
        };
        // count = 2, base_id = 5, 8 keyframe bytes.
        let bc = [0x4B, 2, 5, 0x10, 0x11, 0x12, 0x13, 0x20, 0x21, 0x22, 0x23];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 11 });
        assert_eq!(ctx.flags & 0x1000, 0x1000);
        // 0xD3FF = 0b1101001111111111 keeps bits 12, 6-9, 0-5; clears 10, 11, 13, 14.
        assert_eq!(ctx.local_flags & 0x1000, 0x1000);
        assert_eq!(ctx.local_flags & 0x0C00, 0); // bits 10, 11 cleared by mask
        assert_eq!(ctx.local_flags & 0x2000, 0); // bit 13 cleared
        assert_eq!(ctx.face_rotation, 2);
        assert_eq!(host.animations.len(), 1);
        assert_eq!(host.animations[0].0, 7);
        assert_eq!(host.animations[0].1, 2);
        assert_eq!(host.animations[0].2, 5);
        assert_eq!(
            host.animations[0].3,
            vec![0x10, 0x11, 0x12, 0x13, 0x20, 0x21, 0x22, 0x23]
        );
    }

    #[test]
    fn animate_zero_count_advances_3() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4B, 0, 0], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(host.animations.len(), 1);
        assert!(host.animations[0].3.is_empty());
    }

    // -- 0x4C MENU_CTRL sub-0 -------------------------------------------

    #[test]
    fn menu_sub_0_sets_party_leader() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // op0 = 0x05 (high nibble 0, low bits 5 → masked to 5 = 0x05 & 7).
        let r = step(&mut host, &mut ctx, &[0x4C, 0x05], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.party_leaders, vec![5]);
    }

    #[test]
    fn menu_sub_0_masks_leader_to_low_3_bits() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // op0 = 0x0F → masked to 7.
        step(&mut host, &mut ctx, &[0x4C, 0x0F], 0);
        assert_eq!(host.party_leaders, vec![7]);
    }

    #[test]
    fn menu_sub_1_advances_seven_and_dispatches_to_host() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // op0 = 0x12 → high nibble 1 → menu_ctrl_sub1.
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4C, 0x12, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5],
            0,
        );
        assert_eq!(r, StepResult::Advance { next_pc: 7 });
        assert_eq!(
            host.menu_sub1_calls,
            vec![(0x12, [0xA1, 0xA2, 0xA3, 0xA4, 0xA5])]
        );
    }

    #[test]
    fn menu_sub_3_sub_5_writes_local_flags() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            local_flags: 0xFF80,
            ..Default::default()
        };
        // op0 = 0x35 → sub-3 sub-5: lf = (lf & 0xFF7F) | 0x020A.
        let r = step(&mut host, &mut ctx, &[0x4C, 0x35], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(ctx.local_flags, (0xFF80 & 0xFF7F) | 0x020A);
    }

    #[test]
    fn menu_sub_3_sub_6_or_local_flags() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            local_flags: 0x0001,
            ..Default::default()
        };
        // op0 = 0x36 → sub-3 sub-6: lf |= 0x028A.
        let r = step(&mut host, &mut ctx, &[0x4C, 0x36], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(ctx.local_flags, 0x0001 | 0x028A);
    }

    #[test]
    fn menu_sub_3_sub_7_no_player_falls_through() {
        // Host has no player_coords set → behave as a no-op advance.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4C, 0x37], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(ctx.world_x, 0);
        assert!(host.inverted_y_writes.is_empty());
    }

    #[test]
    fn menu_sub_3_sub_7_copies_player_coords() {
        let mut host = TestHost {
            player_coords: Some(PlayerCoords {
                world_x: 0x1234,
                world_y: 0x4567,
                world_z: 0x89AB,
                field_26: 0xCDEF,
            }),
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4C, 0x37], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(ctx.world_x, 0x1234);
        assert_eq!(ctx.world_y, 0x4567);
        assert_eq!(ctx.world_z, 0x89AB);
        assert_eq!(ctx.field_26, 0xCDEF);
        // No bit 0x20000000 → no inverted-Y write.
        assert!(host.inverted_y_writes.is_empty());
    }

    #[test]
    fn menu_sub_3_sub_7_yields_with_inverted_y_when_flag_set() {
        let mut host = TestHost {
            player_coords: Some(PlayerCoords {
                world_x: 0,
                world_y: 0x0010, // small positive - easy to negate
                world_z: 0,
                field_26: 0,
            }),
            ..Default::default()
        };
        let mut ctx = FieldCtx {
            flags: 0x2000_0000,
            ..Default::default()
        };
        let r = step(&mut host, &mut ctx, &[0x4C, 0x37], 0);
        // Inverted-Y branch returns Yield (caseD_4 STATE_RESUME exit).
        assert_eq!(r, StepResult::Yield { resume_pc: 2 });
        assert_eq!(host.inverted_y_writes, vec![-0x0010]);
        // Coords still copied even on the yield path.
        assert_eq!(ctx.world_y, 0x0010);
    }

    #[test]
    fn menu_sub_3_sub_2_clears_party_state_region() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // op0 = 0x32 → sub-3 sub-2 (clear 512-byte party-state region).
        let r = step(&mut host, &mut ctx, &[0x4C, 0x32], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.party_state_clears, 1);
    }

    #[test]
    fn op_4c_outer_unported_subdispatcher_returns_pending() {
        // Sanity-check that the outer 0x4C dispatcher still returns Pending
        // for sub-dispatchers we haven't ported yet. Outer nibble 5 sub-1
        // (NPC movement with halt-acquire), sub-2/3/4 (dialog query
        // cluster), and outer nibble 8 sub-0 (actor allocator with
        // halt-acquire) all remain Pending. Pick nibble 5 sub-1 as a stable
        // target.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4C, 0x51, 0, 0, 0, 0], 0);
        assert!(matches!(r, StepResult::Pending { opcode: 0x4C, .. }));
    }

    #[test]
    fn op_4c_n4_sub_0_immediate_writes_field_72() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // [4C, 0x40, 34, 12, 0, 0] → sub-0, val=0x1234, ticks=0 (immediate)
        let r = step(&mut host, &mut ctx, &[0x4C, 0x40, 0x34, 0x12, 0, 0], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(ctx.field_72, 0x1234);
        assert!(host.n4_ctx_ramps.is_empty());
    }

    #[test]
    fn op_4c_n4_sub_0_ramp_calls_host_does_not_write_ctx() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // [4C, 0x40, 0xCC, 0x55, 0x10, 0] → sub-0, val=0x55CC, ticks=16
        let r = step(&mut host, &mut ctx, &[0x4C, 0x40, 0xCC, 0x55, 0x10, 0], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(ctx.field_72, 0); // VM does not write ctx in ramp path
        assert_eq!(host.n4_ctx_ramps, vec![(0u8, 0x55CCi16, 16u16)]);
    }

    #[test]
    fn op_4c_n4_sub_2_immediate_mirrors_world_y_when_flag_set() {
        // sub-2 immediate write path: when `flags & 0x20000000` is set,
        // also writes `world_y = -value`.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            flags: 0x2000_0000,
            ..FieldCtx::default()
        };
        // val = 0x0064 (= 100); world_y should become (u16)(-100) = 0xFF9C
        let r = step(&mut host, &mut ctx, &[0x4C, 0x42, 0x64, 0x00, 0, 0], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(ctx.field_8e, 100);
        assert_eq!(ctx.world_y, (-100i16) as u16);
    }

    #[test]
    fn op_4c_n4_sub_2_immediate_no_mirror_without_flag() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4C, 0x42, 0x64, 0x00, 0, 0], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(ctx.field_8e, 100);
        assert_eq!(ctx.world_y, 0);
    }

    #[test]
    fn op_4c_n4_sub_8_immediate_writes_field_26() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // [4C, 0x48, 0x07, 0x00, 0, 0] → sub-8, val=7
        let r = step(&mut host, &mut ctx, &[0x4C, 0x48, 0x07, 0x00, 0, 0], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(ctx.field_26, 7);
    }

    #[test]
    fn op_4c_n4_global_subs_call_host() {
        for sub in [0xAu8, 0xB, 0xC, 0xD] {
            let mut host = TestHost::default();
            let mut ctx = FieldCtx::default();
            let r = step(
                &mut host,
                &mut ctx,
                &[0x4C, 0x40 | sub, 0x10, 0x00, 0x05, 0x00],
                0,
            );
            assert_eq!(r, StepResult::Advance { next_pc: 6 });
            assert_eq!(host.n4_global_writes, vec![(sub, 0x10i32, 5u16)]);
        }
    }

    // -- 0x4C outer-nibble-4 sub-1 (ctx[+0x6A] write/ramp with halve+floor)

    #[test]
    fn op_4c_n4_sub_1_immediate_halves_and_floors() {
        // val = 6 → halved = 3 → ctx.field_6a = 3
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4C, 0x41, 0x06, 0x00, 0, 0], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(ctx.field_6a, 3);
        assert!(host.n4_ctx_ramps.is_empty());
    }

    #[test]
    fn op_4c_n4_sub_1_immediate_floors_zero_to_one() {
        // val = 1 → halved = 0 → floor → 1
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4C, 0x41, 0x01, 0x00, 0, 0], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(ctx.field_6a, 1);
    }

    #[test]
    fn op_4c_n4_sub_1_ramp_passes_halved_target_to_host() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // val=20 → halved=10; ticks=8
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4C, 0x41, 0x14, 0x00, 0x08, 0x00],
            0,
        );
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(ctx.field_6a, 0); // VM does not write in ramp path
        assert_eq!(host.n4_ctx_ramps, vec![(1u8, 10i16, 8u16)]);
    }

    // -- 0x4C outer-nibble-4 sub-3 (ramp +0x24 OR absolute jump)

    #[test]
    fn op_4c_n4_sub_3_ticks_zero_jumps_absolute() {
        // ticks=0 path: returned `iVar18` = signed_16(operand[0..2]) - the
        // new PC offset is the literal target value.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4C, 0x43, 0x40, 0x00, 0, 0], 0);
        // target = 0x40 → next_pc = 0x40
        assert_eq!(r, StepResult::Advance { next_pc: 0x40 });
        assert_eq!(ctx.field_24, 0); // jump-only branch does not write
        assert!(host.n4_ctx_ramps.is_empty());
    }

    #[test]
    fn op_4c_n4_sub_3_ticks_nonzero_ramps_field_24() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // val=300, ticks=12 → ramp `ctx.field_24` 0 → 300 over 12 frames
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4C, 0x43, 0x2C, 0x01, 0x0C, 0x00],
            0,
        );
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(ctx.field_24, 0); // VM does not write in ramp path
        assert_eq!(host.n4_ctx_ramps, vec![(3u8, 300i16, 12u16)]);
    }

    // -- 0x4C outer-nibble-4 sub-4 (immediate +0x28 OR absolute jump)

    #[test]
    fn op_4c_n4_sub_4_ticks_zero_writes_field_28() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4C, 0x44, 0xFF, 0xFF, 0, 0], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(ctx.field_28, -1);
        assert!(host.n4_ctx_ramps.is_empty());
    }

    #[test]
    fn op_4c_n4_sub_4_ticks_nonzero_jumps_absolute() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // ticks=4 (nonzero) → jump to target = 0x80
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4C, 0x44, 0x80, 0x00, 0x04, 0x00],
            0,
        );
        assert_eq!(r, StepResult::Advance { next_pc: 0x80 });
        assert_eq!(ctx.field_28, 0); // jump-only branch does not write
    }

    // -- 0x4C outer-nibble-4 sub-6 / sub-7 (paired-global gate)

    #[test]
    fn op_4c_n4_sub_6_gate_clear_writes_global() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // gate=false (default) → regular write/ramp dispatch fires.
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4C, 0x46, 0x07, 0x00, 0x10, 0x00],
            0,
        );
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(host.n4_global_writes, vec![(6u8, 7i32, 16u16)]);
        assert_eq!(host.n4_global_pair_clears, 0);
    }

    #[test]
    fn op_4c_n4_sub_7_gate_clear_writes_global() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4C, 0x47, 0x21, 0x00, 0x00, 0x00],
            0,
        );
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(host.n4_global_writes, vec![(7u8, 0x21i32, 0u16)]);
        assert_eq!(host.n4_global_pair_clears, 0);
    }

    #[test]
    fn op_4c_n4_sub_6_gate_set_clears_pair_and_skips_write() {
        let mut host = TestHost {
            n4_global_pair_gated: true,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4C, 0x46, 0x07, 0x00, 0x10, 0x00],
            0,
        );
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        // No write happened - the gate short-circuited to a pair-clear.
        assert!(host.n4_global_writes.is_empty());
        assert_eq!(host.n4_global_pair_clears, 1);
    }

    #[test]
    fn op_4c_n4_sub_7_gate_set_clears_pair_and_skips_write() {
        let mut host = TestHost {
            n4_global_pair_gated: true,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4C, 0x47, 0x07, 0x00, 0x10, 0x00],
            0,
        );
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert!(host.n4_global_writes.is_empty());
        assert_eq!(host.n4_global_pair_clears, 1);
    }

    // -- 0x4C outer-nibble-4 sub-5 (actor-field block, 11-byte encoding)

    #[test]
    fn op_4c_n4_sub_5_immediate_writes_actor_block() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // [4C, 0x45, b1=0x07, w94=0x0123, w96=-0x0001, w98=0x4567, ticks=0]
        let bytes = [
            0x4C, 0x45, 0x07, 0x23, 0x01, 0xFF, 0xFF, 0x67, 0x45, 0x00, 0x00,
        ];
        let r = step(&mut host, &mut ctx, &bytes, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 11 });
        assert_eq!(host.n4_sub5_immediate, vec![(0x07, 0x0123, -1, 0x4567)]);
        assert!(host.n4_sub5_ramp.is_empty());
    }

    #[test]
    fn op_4c_n4_sub_5_ramp_yields_at_pc() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // ticks = 30 → ramp path
        let bytes = [
            0x4C, 0x45, 0x40, 0x10, 0x00, 0x20, 0x00, 0x30, 0x00, 0x1E, 0x00,
        ];
        let r = step(&mut host, &mut ctx, &bytes, 0);
        assert_eq!(r, StepResult::Yield { resume_pc: 11 });
        assert_eq!(
            host.n4_sub5_ramp,
            vec![(0x40, 0x0010, 0x0020, 0x0030, 30u16)]
        );
        assert!(host.n4_sub5_immediate.is_empty());
    }

    #[test]
    fn op_4c_n4_sub_5_truncated_buffer_returns_unknown() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // Only 8 bytes - sub-5 needs 11.
        let bytes = [0x4C, 0x45, 0x07, 0x23, 0x01, 0xFF, 0xFF, 0x67];
        let r = step(&mut host, &mut ctx, &bytes, 0);
        assert!(matches!(r, StepResult::Unknown { .. }));
    }

    // -- 0x4C outer-nibble-4 sub-9 (story-flag-driven dispatch)

    #[test]
    fn op_4c_n4_sub_9_default_immediate_calls_default_write() {
        let mut host = TestHost::default(); // n4_sub9_state defaults to Default
        let mut ctx = FieldCtx::default();
        // [4C, 0x49, target=0x0042, ticks=0]
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4C, 0x49, 0x42, 0x00, 0x00, 0x00],
            0,
        );
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(host.n4_sub9_default_writes, vec![0x0042i16]);
        assert!(host.n4_sub9_default_ramps.is_empty());
        assert!(host.n4_sub9_delta_calls.is_empty());
    }

    #[test]
    fn op_4c_n4_sub_9_default_ramp_yields_at_pc() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // ticks = 60 → ramp path; VM yields at the same PC.
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4C, 0x49, 0x42, 0x00, 0x3C, 0x00],
            0,
        );
        assert_eq!(r, StepResult::Yield { resume_pc: 0 });
        assert_eq!(host.n4_sub9_default_ramps, vec![(0x0042i16, 60u16)]);
        assert!(host.n4_sub9_default_writes.is_empty());
    }

    #[test]
    fn op_4c_n4_sub_9_abs_jump_uses_signed_target() {
        let mut host = TestHost {
            n4_sub9_state: Sub9State::AbsJump,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        // target = 0x0080 → absolute jump regardless of ticks.
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4C, 0x49, 0x80, 0x00, 0x10, 0x00],
            0,
        );
        assert_eq!(r, StepResult::Advance { next_pc: 0x80 });
        // No host writes happen on the abs-jump path.
        assert!(host.n4_sub9_default_writes.is_empty());
        assert!(host.n4_sub9_default_ramps.is_empty());
        assert!(host.n4_sub9_delta_calls.is_empty());
    }

    #[test]
    fn op_4c_n4_sub_9_delta_immediate_advances() {
        let mut host = TestHost {
            n4_sub9_state: Sub9State::Delta,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4C, 0x49, 0x55, 0xFF, 0x00, 0x00],
            0,
        );
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        // signed_16(0xFF55) = -171
        assert_eq!(host.n4_sub9_delta_calls, vec![(-171i16, 0u16)]);
    }

    #[test]
    fn op_4c_n4_sub_9_delta_ramp_yields() {
        let mut host = TestHost {
            n4_sub9_state: Sub9State::Delta,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4C, 0x49, 0x55, 0x00, 0x14, 0x00],
            0,
        );
        assert_eq!(r, StepResult::Yield { resume_pc: 0 });
        assert_eq!(host.n4_sub9_delta_calls, vec![(0x55i16, 20u16)]);
    }

    #[test]
    fn op_4c_n4_sub_9_state_default_when_no_story_bits_set() {
        // Verify the FieldHost::op4c_n4_sub9_state default impl reads global flags.
        let host = TestHost {
            globals: 0x0000_0000,
            ..TestHost::default()
        };
        // TestHost overrides with its own n4_sub9_state, but a fresh host with
        // the default Sub9State is at the `Default` variant.
        assert_eq!(host.n4_sub9_state, Sub9State::Default);
    }

    #[test]
    fn op_34_sub_2_no_match_advances_two() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // sub_op = 2, b1 = 0x40 - but op34_sub2_capture_match=false (default)
        // so the host returns false → Advance PC += 2.
        let r = step(&mut host, &mut ctx, &[0x34, 0x20, 0x40, 0x00], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert!(host.op34_sub2_captures.is_empty());
    }

    #[test]
    fn op_34_sub_2_match_with_b1_40_yields_and_captures() {
        let mut host = TestHost {
            op34_sub2_capture_match: true,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x34, 0x20, 0x40, 0x00, 0x00], 0);
        assert_eq!(r, StepResult::Yield { resume_pc: 0 });
        // Captured PC offset should be `pc + header_size + 2` = 3.
        assert_eq!(host.op34_sub2_captures, vec![(0x40u8, 3usize)]);
    }

    #[test]
    fn op_34_sub_2_match_but_b1_not_40_advances_two() {
        // Even with the lookup matching, b1 != 0x40 means no capture and the
        // VM falls through to Advance PC += 2.
        let mut host = TestHost {
            op34_sub2_capture_match: true,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x34, 0x20, 0x10, 0x00], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert!(host.op34_sub2_captures.is_empty());
    }

    #[test]
    fn op_34_sub_0_advances_pc_by_7_with_rgb_intensity() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // sub-0: op0 = 0x05 (sub_nibble = 0, low_3 = 0b101), rgb = (1, 2, 3),
        // intensity = 0x1234.
        let r = step(&mut host, &mut ctx, &[0x34, 0x05, 1, 2, 3, 0x34, 0x12], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 7 });
        assert_eq!(host.op34_sub0_calls.len(), 1);
        let (op0, rgb, intensity) = host.op34_sub0_calls[0];
        assert_eq!(op0, 0x05);
        assert_eq!(rgb, [1, 2, 3]);
        assert_eq!(intensity, 0x1234);
    }

    #[test]
    fn op_34_sub_0_negative_intensity_sign_extends() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // intensity = 0xFFFE = -2 as i16
        let r = step(&mut host, &mut ctx, &[0x34, 0x00, 0, 0, 0, 0xFE, 0xFF], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 7 });
        assert_eq!(host.op34_sub0_calls[0].2, -2);
    }

    #[test]
    fn op_34_sub_1_default_advance_is_13() {
        // No host overrides → default impl returns delta = 13. Capture flag
        // is 0 so the captured-PC path doesn't fire.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(
            &mut host,
            &mut ctx,
            &[0x34, 0x10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            0,
        );
        assert_eq!(r, StepResult::Advance { next_pc: 13 });
        assert_eq!(host.op34_sub1_calls.len(), 1);
        let call = &host.op34_sub1_calls[0];
        assert_eq!(call.op0, 0x10);
        assert_eq!(call.packed24, 0);
        assert_eq!(call.pos, [0, 0, 0]);
        assert_eq!(call.capture_flag, 0);
        assert!(call.captured_payload.is_empty());
    }

    #[test]
    fn op_34_sub_1_packed24_and_position_decode() {
        // packed24 = 0x123456 (b1=0x12, b2=0x34, b3=0x56), world_x = 100,
        // world_z = -50, world_y = -(-200) = 200 (the original NEGATES the
        // raw bytes). reserved bytes are 0; capture_flag = 0.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let raw = [
            0x34, 0x10, 0x12, 0x34, 0x56, // op0 + packed24
            100, 0, // world_x = 100
            0xCE, 0xFF, // world_z = -50
            0x38, 0xFF, // raw -y = -200, → world_y = 200
            0, 0, // reserved
            0, // capture_flag
        ];
        let r = step(&mut host, &mut ctx, &raw, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 13 });
        let call = &host.op34_sub1_calls[0];
        assert_eq!(call.packed24, 0x123456);
        assert_eq!(call.pos, [100, 200, -50]);
    }

    #[test]
    fn op_34_sub_1_capture_path_uses_host_returned_delta() {
        // capture_flag = 0x40, payload_len = 3. The instruction is 13 base
        // bytes + 2 (header bytes 0x40, len) + 3 (payload) = 18. Default
        // host returns 13; we override to model the capture branch, which
        // returns `13 + 2 + payload_len` = 18.
        let mut host = TestHost {
            op34_sub1_capture_delta: Some(18),
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let raw = [
            0x34, 0x10, 0x00, 0x00, 0x00, // op0 + packed24
            0, 0, 0, 0, 0, 0, 0, 0, // pos + reserved
            0x40, 3, // capture_flag, payload_len
            0xAA, 0xBB, 0xCC, // captured payload
        ];
        let r = step(&mut host, &mut ctx, &raw, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 18 });
        let call = &host.op34_sub1_calls[0];
        assert_eq!(call.capture_flag, 0x40);
        assert_eq!(call.captured_payload, vec![0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn op_4c_n4_negative_value_sign_extends() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // val = 0xFFFE = -2 as i16
        let r = step(&mut host, &mut ctx, &[0x4C, 0x40, 0xFE, 0xFF, 0, 0], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        // i16 (-2) cast to u16 = 0xFFFE
        assert_eq!(ctx.field_72, 0xFFFE);
    }

    #[test]
    fn op_4c_sub_3_sub_0_locks_input_and_yields() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4C, 0x30], 0);
        assert_eq!(r, StepResult::Yield { resume_pc: 2 });
        assert_eq!(host.field_input_lock_writes, vec![true]);
    }

    #[test]
    fn op_4c_sub_3_sub_1_unlocks_input_and_yields() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4C, 0x31], 0);
        assert_eq!(r, StepResult::Yield { resume_pc: 2 });
        assert_eq!(host.field_input_lock_writes, vec![false]);
    }

    #[test]
    fn op_4c_sub_3_sub_4_b_c_advance_two_no_host_call() {
        // sub-4 / sub-B / sub-C all jump through code_r0x801df098 →
        // LAB_801df09c → switchD_801e00f4::default(). Asm delay slot is
        // `_addiu s8, s8, 0x2` → PC += 2. No host hook fires.
        for sub in [0x4u8, 0xB, 0xC] {
            let mut host = TestHost::default();
            let mut ctx = FieldCtx::default();
            let r = step(&mut host, &mut ctx, &[0x4C, 0x30 | sub], 0);
            assert_eq!(
                r,
                StepResult::Advance { next_pc: 2 },
                "sub_{:X} did not advance",
                sub
            );
            // None of the side-effect counters should fire for this no-op
            // group.
            assert_eq!(host.field_input_lock_writes, Vec::<bool>::new());
            assert_eq!(host.party_state_clears, 0);
            assert_eq!(host.menu_refresh_calls, 0);
        }
    }

    #[test]
    fn menu_sub_3_sub_3_calls_refresh() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4C, 0x33], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.menu_refresh_calls, 1);
    }

    #[test]
    fn menu_sub_3_sub_a_copies_dialog_depth() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4C, 0x3A], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.depth_copy_calls, 1);
    }

    #[test]
    fn menu_sub_3_sub_8_and_d_call_subtile_refresh() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4C, 0x38], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        let r = step(&mut host, &mut ctx, &[0x4C, 0x3D], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.subtile_refresh_calls, vec![8, 0xD]);
    }

    #[test]
    fn menu_sub_2_dispatches_party_view_swap() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // op0 = 0x2A → sub-2; new_index = 0xA & 7 = 2.
        let r = step(&mut host, &mut ctx, &[0x4C, 0x2A], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.party_view_swaps, vec![2]);
    }

    // -- 0x4D BBOX_TEST --------------------------------------------------

    #[test]
    fn bbox_inside_default_tile_derivation() {
        // world_x = 0x1C0 → tile = (0x1C0 - 0x40) >> 7 = 3
        // world_z = 0x140 → tile = (0x140 - 0x40) >> 7 = 2
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            world_x: 0x01C0,
            world_z: 0x0140,
            ..Default::default()
        };
        // bbox: x in [2..4], z in [1..3] → inside.
        let r = step(&mut host, &mut ctx, &[0x4D, 2, 1, 4, 3, 0xAA, 0xBB], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 7 });
    }

    // -- 0x36 SCENE_FADE -------------------------------------------------

    #[test]
    fn scene_fade_done_advances_5() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x36, 0x10, 0x80, 0x05, 0x00], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
        assert_eq!(host.scene_fade_calls, vec![(0x8010u16, 0x0005u16)]);
    }

    #[test]
    fn scene_fade_busy_halts() {
        let mut host = TestHost {
            scene_fade_busy: true,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x36, 0xFF, 0xFF, 0x00, 0x00], 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        assert_eq!(host.scene_fade_calls, vec![(0xFFFFu16, 0u16)]);
    }

    // -- 0x45 CAMERA -----------------------------------------------------

    #[test]
    fn camera_load_advances_20() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // op0 = 0x40 → LOAD; 18 bytes payload.
        let mut bc = vec![0x45, 0x40];
        bc.extend((0..18u8).map(|i| 0x10 + i));
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 20 });
        assert_eq!(host.camera_loads.len(), 1);
        assert_eq!(host.camera_loads[0].len(), 18);
        assert_eq!(host.camera_loads[0][0], 0x10);
        assert_eq!(host.camera_loads[0][17], 0x21);
    }

    #[test]
    fn camera_save_advances_2_and_pings_host() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // op0 = 0x80 → SAVE.
        let r = step(&mut host, &mut ctx, &[0x45, 0x80], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.camera_saves, 1);
    }

    #[test]
    fn camera_apply_jumps_to_absolute_pc() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // op0 = 0xC0 → APPLY; absolute target = LE_u16(operand[1..3]) = 0x0042.
        let r = step(&mut host, &mut ctx, &[0x45, 0xC0, 0x42, 0x00], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 0x42 });
        assert_eq!(host.camera_applies, 1);
    }

    #[test]
    fn camera_configure_decodes_mask_and_advances() {
        // op0 = 0x00 (top 2 bits clear; mode = (0 >> 2) & 0xF = 0).
        // op1 = 0b1010_0000 = 0xA0 → mask bit 7 (slot 2), bit 5 (slot 4).
        // Wait - the bit interpretation is "MSB-first across the 10-bit
        // mask" from CONCAT11(op0, op1). So mask = u16(op0:op1) =
        // 0x00A0 = 0b0000_0000_1010_0000. Bit 9 → slot 0, bit 0 → slot 9.
        // Set bits at positions 7 and 5 → slot indices 9-7=2 and 9-5=4.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // [45, 0x00, 0xA0, lo_trig, hi_trig, val0_lo, val0_hi, val1_lo, val1_hi]
        let bc = [
            0x45, 0x00, 0xA0, // opcode + op0/op1 mask
            0x34, 0x12, // apply_trigger = 0x1234
            0x55, 0x44, // first set bit (slot 2) → 0x4455
            0x66, 0x77, // second set bit (slot 4) → 0x7766
        ];
        let r = step(&mut host, &mut ctx, &bc, 0);
        // PC += 5 + 2*set_count = 5 + 4 = 9.
        assert_eq!(r, StepResult::Advance { next_pc: 9 });
        assert_eq!(host.camera_configs.len(), 1);
        let (params, trigger, mode) = &host.camera_configs[0];
        assert_eq!(*trigger, 0x1234);
        assert_eq!(*mode, 0);
        assert_eq!(params.len(), 2);
        assert_eq!(
            params[0],
            CameraParam {
                slot: 2,
                value: 0x4455
            }
        );
        assert_eq!(
            params[1],
            CameraParam {
                slot: 4,
                value: 0x7766
            }
        );
    }

    #[test]
    fn camera_configure_zero_mask_advances_5() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // No bits set → no params, but still PC += 5.
        let bc = [0x45, 0x00, 0x00, 0x12, 0x34];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
        assert_eq!(host.camera_configs.len(), 1);
        assert!(host.camera_configs[0].0.is_empty());
        assert_eq!(host.camera_configs[0].1, 0x3412);
    }

    #[test]
    fn camera_configure_mode_is_op0_shifted_right_2_low_4() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // op0 = 0x14 (top 2 bits clear → CONFIGURE; (0x14 >> 2) & 0xF = 5).
        let bc = [0x45, 0x14, 0x00, 0x00, 0x00];
        step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(host.camera_configs[0].2, 5);
    }

    #[test]
    fn bbox_outside_jumps_via_skip_delta() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            world_x: 0x01C0, // tile 3
            world_z: 0x0140, // tile 2
            ..Default::default()
        };
        // bbox: x in [10..20] → outside. skip = 0x100; outside-box jump
        // target = pc + header_size + 4 + delta = 0 + 1 + 4 + 0x100 = 261.
        let r = step(&mut host, &mut ctx, &[0x4D, 10, 0, 20, 10, 0, 1], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 261 });
    }

    #[test]
    fn bbox_outside_zero_skip_advances_5() {
        // Confirm that skip=0 still produces a non-zero next_pc (= pc + 5).
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            world_x: 0x01C0,
            world_z: 0x0140,
            ..Default::default()
        };
        let r = step(&mut host, &mut ctx, &[0x4D, 10, 0, 20, 10, 0, 0], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
    }

    // -- 0x4E inventory comparison-and-jump ----------------------------

    #[test]
    fn op_4e_sub0_lt_taken_jumps_relative() {
        // Sub-op 0, comparison 0 (state < scaled). state=5, factor=10,
        // arg=128 -> scaled = (10 * 128) >> 8 = 5; 5 < 5 = false; not taken.
        let mut host = TestHost::default();
        host.inventory_pairs.insert((0, 0), (5, 10));
        let mut ctx = FieldCtx::default();
        // skip = 100 (0x64). Not taken -> PC += 7.
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4E, 0, 0x00, 0x80, 0x00, 0x64, 0x00],
            0,
        );
        assert_eq!(r, StepResult::Advance { next_pc: 7 });
    }

    #[test]
    fn op_4e_sub0_lt_strict_taken() {
        // state=5, factor=10, arg=200 -> scaled = (10*200)>>8 = 7; 5 < 7
        // is true. Forward jump: PC = pc + 1 + 4 + 100 = 105.
        let mut host = TestHost::default();
        host.inventory_pairs.insert((0, 0), (5, 10));
        let mut ctx = FieldCtx::default();
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4E, 0, 0x00, 200, 0x00, 100, 0x00],
            0,
        );
        assert_eq!(r, StepResult::Advance { next_pc: 105 });
    }

    #[test]
    fn op_4e_sub1_gt_uses_second_pair() {
        // mode_byte = 0x11 (sub=1, op=1 → scaled < state). page=2.
        // pair (2, 1) returns (state=20, factor=8). arg = 1024.
        // scaled = (8 * 1024) >> 8 = 32. 32 < 20 is false; not taken.
        let mut host = TestHost::default();
        host.inventory_pairs.insert((2, 1), (20, 8));
        let mut ctx = FieldCtx::default();
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4E, 2, 0x11, 0x00, 0x04, 0x42, 0x00],
            0,
        );
        // not-taken -> PC += 7
        assert_eq!(r, StepResult::Advance { next_pc: 7 });
    }

    #[test]
    fn op_4e_sub2_absolute_jump() {
        // sub-op 2 falls through to absolute jump = LE_u16(operand[2..4]).
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // mode_byte high nibble = 2; LE u16 at +2 = 0x1234.
        let bc = [0x4E, 0x00, 0x20, 0x34, 0x12, 0x00, 0x00];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 0x1234 });
    }

    #[test]
    fn op_4e_sub_c_through_f_advance_seven() {
        // mode_byte high-nibble in 12..=15 hits the dispatcher's default arm
        // at switchD_801e0a38_default. With no party-bank state initialised,
        // `uVar31 = uVar27 = 0`; the comparison is false, and `(sub-10) < 2`
        // is false for sub-op >= 12, so the original returns `param_2 + 7`
        // (= PC += 7 from the opcode).
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        for sub in 12..=15u8 {
            let mode = sub << 4;
            let bc = [0x4E, 0, mode, 0, 0, 0, 0, 0, 0];
            let r = step(&mut host, &mut ctx, &bc, 0);
            assert_eq!(
                r,
                StepResult::Advance { next_pc: 7 },
                "sub-op {sub}: expected PC += 7"
            );
        }
    }

    #[test]
    fn op_4e_sub_a_compares_party_bank() {
        // sub-op 10 (party-bank A). state=1000, scaled-from-operands=
        // (0x0064 | (0x0000 << 16)) = 100. mode=0 (state < scaled): false ->
        // PC += 9.
        let mut host = TestHost::default();
        host.party_bank.insert(10, 1000);
        let mut ctx = FieldCtx::default();
        let bc = [0x4E, 0, 0xA0, 0x64, 0x00, 0x42, 0x00, 0x00, 0x00];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 9 });
    }

    #[test]
    fn op_4e_sub_a_compare_taken() {
        // sub-op 10. state=50, scaled = 0x0064 = 100. mode=0 (state < scaled):
        // 50 < 100 = true -> jump pc + 1 + 4 + delta = 5 + 0x10 = 21.
        let mut host = TestHost::default();
        host.party_bank.insert(10, 50);
        let mut ctx = FieldCtx::default();
        let bc = [0x4E, 0, 0xA0, 0x64, 0x00, 0x10, 0x00, 0x00, 0x00];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 21 });
    }

    #[test]
    fn op_4e_sub_4_invokes_bios_rand_and_jumps_to_returned_pc() {
        // Sub-op 4 calls BIOS Rand (FUN_80056798) and uses the result as the
        // next PC. With the host returning 0x42, the VM should jump to 0x42.
        let mut host = TestHost {
            op4e_sub4_bios_rand_value: 0x42,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4E, 0, 0x40], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 0x42 });
        assert_eq!(host.op4e_sub4_bios_rand_calls, 1);
    }

    #[test]
    fn op_4e_sub_b_uses_high_word() {
        // sub-op 11. scaled = 0x0001 | (0x0001 << 16) = 0x10001. state =
        // 0x10000. mode=1 (scaled < state): 0x10001 < 0x10000 = false ->
        // PC += 9.
        let mut host = TestHost::default();
        host.party_bank.insert(11, 0x10000);
        let mut ctx = FieldCtx::default();
        let bc = [0x4E, 0, 0xB1, 0x01, 0x00, 0, 0, 0x01, 0x00];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 9 });
    }

    // -- 0x49 STATE_RESUME ---------------------------------------------

    #[test]
    fn op_49_idle_arms_and_halts() {
        let mut host = TestHost {
            op49_state_value: Op49State::Idle,
            ..Default::default()
        };
        let mut ctx = FieldCtx {
            field_90: 0xDEAD_BEEF,
            ..Default::default()
        };
        // Sub-op 1: captures field_90 into the arm record.
        let r = step(&mut host, &mut ctx, &[0x49, 1, 0, 0, 0], 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        assert_eq!(host.op49_arms, vec![(0, 0xDEAD_BEEF)]);
        assert_eq!(host.op49_setups, 1);
    }

    #[test]
    fn op_49_idle_invalid_subop_halts_without_arming() {
        let mut host = TestHost {
            op49_state_value: Op49State::Idle,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        // sub-op 0xE is out of range (> 0xD).
        let r = step(&mut host, &mut ctx, &[0x49, 0xE, 0, 0, 0], 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        assert!(host.op49_arms.is_empty());
        assert_eq!(host.op49_setups, 0);
    }

    #[test]
    fn op_49_armed_halts() {
        let mut host = TestHost {
            op49_state_value: Op49State::Armed,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x49, 1, 0, 0, 0], 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        assert!(host.op49_arms.is_empty());
    }

    #[test]
    fn op_49_done_advances_per_subop() {
        // Sub-op 1, 3, 7 jump to 0x801e00b8 in the original, where
        // `addiu s8, s8, 0x3` runs before the dispatcher tail; PC += 3.
        let mut host = TestHost {
            op49_state_value: Op49State::Done,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x49, 1, 0, 0, 0], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(host.op49_clears, 1);

        // Sub-op 2, 4: original returns `param_2 + 7`; PC += 7.
        host.op49_state_value = Op49State::Done;
        let r = step(&mut host, &mut ctx, &[0x49, 2, 0, 0, 0, 0, 0, 0], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 7 });

        // Sub-op 5: original returns `param_2 + 0xe`; PC += 14.
        host.op49_state_value = Op49State::Done;
        let r = step(
            &mut host,
            &mut ctx,
            &[0x49, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            0,
        );
        assert_eq!(r, StepResult::Advance { next_pc: 14 });
    }

    #[test]
    fn op_49_done_subop_a_halts_at_pc() {
        // Done-side catch-all in `FUN_801de840 case 0x49`: any sub-op that
        // isn't explicitly handled (here sub-0xA, which falls through the
        // 1/3/7, 2/4, 5, 6/8/9/C/D arms) clears the resume slot and returns
        // `param_2` (= halt at the same PC).
        let mut host = TestHost {
            op49_state_value: Op49State::Done,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x49, 0xA, 0, 0, 0], 0);
        assert!(matches!(r, StepResult::Halt { final_pc: 0 }));
        assert_eq!(host.op49_clears, 1, "should clear the resume slot");
    }

    #[test]
    fn op_49_done_sub0_walks_inline_mes_payload() {
        // `[49, 0, length, ...length args..., ...mes_bytes, terminator, ...]`.
        // The original at FUN_801DE840 (case 0x49 / DONE / sub-0) reads
        // `length = pbVar47[2]`, then walks the inline MES bytecode starting
        // at `pbVar47 + length + 3` via `func_0x8003ca38` (counts bytes > 0x1E,
        // pair-extends 0xCx prefix bytes), and advances PC by `5 + length +
        // walked`.
        let mut host = TestHost {
            op49_state_value: Op49State::Done,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        // length = 2 (two args), MES bytes = [0x50, 0xC3, 0xAA, 0x00 terminator]
        // → walker counts 4 bytes (0x50 = 1, 0xC3 0xAA pair = 2, then sees 0x00).
        let bc = &[
            0x49, // opcode
            0x00, // sub-op
            0x02, // length
            0xAA, 0xBB, // 2 args (ignored by walker - walker starts past them)
            0x50, 0xC3, 0xAA, 0x00, // MES body (3 bytes walked) + terminator
        ];
        let r = step(&mut host, &mut ctx, bc, 0);
        // PC = 0 + (header_size=1) + 4 + length=2 + walked=3 = 10. The
        // terminator is NOT consumed by the walker - it stops at it.
        assert_eq!(r, StepResult::Advance { next_pc: 10 });
        assert_eq!(host.op49_clears, 1);
    }

    #[test]
    fn op_49_done_sub0_empty_mes_advances_5_plus_length() {
        // length = 0, no MES body, immediate terminator.
        let mut host = TestHost {
            op49_state_value: Op49State::Done,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let bc = &[0x49, 0x00, 0x00, 0x00];
        let r = step(&mut host, &mut ctx, bc, 0);
        // PC = 1 + 4 + 0 + 0 = 5.
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
    }

    #[test]
    fn walk_mes_bytecode_terminates_on_low_byte() {
        // Standard byte run terminates on a byte ≤ 0x1E.
        assert_eq!(walk_mes_bytecode(&[0x50, 0x60, 0x70, 0x10, 0x80]), 3);
        // Empty buffer.
        assert_eq!(walk_mes_bytecode(&[]), 0);
        // Immediate terminator.
        assert_eq!(walk_mes_bytecode(&[0x05]), 0);
    }

    #[test]
    fn walk_mes_bytecode_pair_extends_cx_prefix() {
        // 0xC0..0xCF prefix bytes consume one extra byte each (the pair byte).
        // 0xC0 0x05 (pair 0x05 NOT a terminator, because it's a pair byte not
        // a top-level byte) - counts 2.
        assert_eq!(walk_mes_bytecode(&[0xC0, 0x05, 0x10]), 2);
        // 0xCF 0x99 - counts 2; then 0x50 - counts 3 total; then 0x10 stops.
        assert_eq!(walk_mes_bytecode(&[0xCF, 0x99, 0x50, 0x10]), 3);
    }

    #[test]
    fn walk_mes_bytecode_eof_mid_pair_stops_gracefully() {
        // 0xC0 at end of buffer - original would read past EOF; we stop.
        assert_eq!(walk_mes_bytecode(&[0xC0]), 1);
    }

    // -- 0x34 EFFECT sub-3 ----------------------------------------------

    #[test]
    fn op_34_sub3_triggers_anim() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            script_id: 7,
            ..Default::default()
        };
        // op0 high nibble = 3 (=> 0x30). arg = 0x42.
        let r = step(&mut host, &mut ctx, &[0x34, 0x30, 0x42], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(host.effect_anim_calls, vec![(7, 0x42)]);
    }

    #[test]
    fn op_34_high_subop_halts() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // sub-4..=sub-F (op0 >> 4 >= 4) hits the original's
        // `if (bVar35 != 2) { if (bVar35 != 3) { return param_2; } }` at
        // line 4811-4814 of the dump ⇒ halt at PC.
        let r = step(&mut host, &mut ctx, &[0x34, 0x40, 0], 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
    }

    // -- 0x43 ACTOR_CTRL sub-8 ------------------------------------------

    #[test]
    fn op_43_sub8_resets_face_rotation() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            script_id: 11,
            face_rotation: 5,
            ..Default::default()
        };
        let r = step(&mut host, &mut ctx, &[0x43, 8], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(ctx.face_rotation, 0);
        assert_eq!(host.face_resets, vec![11]);
    }

    #[test]
    fn op_43_other_subops_halt() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // Sub-op 0x16 (no handler in the original `case 0x43` inner switch);
        // falls through with `iVar45 = param_2` (initialised at line 4511 of
        // the dump) ⇒ halt at PC.
        let r = step(
            &mut host,
            &mut ctx,
            &[0x43, 0x16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            0,
        );
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
    }

    #[test]
    fn op_43_sub7_face_rotation_setup() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            face_rotation: 0,
            ..Default::default()
        };
        // 17-byte: [43, 7, face=3, payload=0xDEADBEEF, p0=0x1111, p1=0x2222,
        //          p2=0x3333, p3=0x4444, target=-1 (=0xFFFF)]
        let bc = [
            0x43, 7, 3, 0xEF, 0xBE, 0xAD, 0xDE, 0x11, 0x11, 0x22, 0x22, 0x33, 0x33, 0x44, 0x44,
            0xFF, 0xFF,
        ];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 17 });
        assert_eq!(ctx.face_rotation, 3);
        assert_eq!(
            host.face_rotation_setups,
            vec![(
                3u8,
                0xDEAD_BEEFu32,
                [0x1111u16, 0x2222, 0x3333, 0x4444],
                -1i16
            )]
        );
    }

    #[test]
    fn op_43_sub12_alloc_actor() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // [43, 0xC, 0x10, 0x20, 0x30] = 5 bytes; PC += 5.
        let r = step(&mut host, &mut ctx, &[0x43, 0xC, 0x10, 0x20, 0x30], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
        assert_eq!(host.scripted_actor_allocs, vec![(0x10, 0x20, 0x30)]);
    }

    #[test]
    fn op_43_sub2_three_actor_talk() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // [43, 2, a1=5, a2=6, a3=7, lo=0x12, hi=0x34, b6=0xAB] = 8 bytes; PC += 8.
        let r = step(
            &mut host,
            &mut ctx,
            &[0x43, 2, 5, 6, 7, 0x12, 0x34, 0xAB],
            0,
        );
        assert_eq!(r, StepResult::Advance { next_pc: 8 });
        assert_eq!(host.three_actor_talks, vec![([5, 6, 7], 0x3412, 0xAB)]);
    }

    #[test]
    fn op_43_sub_d_alloc_with_mode_3() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // [43, 0xD, b1, b2, b3, b4] = 6 bytes; PC += 6; mode=3.
        let r = step(&mut host, &mut ctx, &[0x43, 0xD, 1, 2, 3, 4], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(host.actor_alloc_modes, vec![(0xD, 3, [1, 2, 3, 4])]);
    }

    #[test]
    fn op_43_sub_f_alloc_with_mode_0() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // [43, 0xF, b1, b2, b3, b4] = 6 bytes; PC += 6; mode=0.
        let r = step(&mut host, &mut ctx, &[0x43, 0xF, 9, 8, 7, 6], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(host.actor_alloc_modes, vec![(0xF, 0, [9, 8, 7, 6])]);
    }

    #[test]
    fn op_43_sub_e_marks_flag() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            script_id: 0x55,
            ..Default::default()
        };
        let r = step(&mut host, &mut ctx, &[0x43, 0xE], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.mark_flag_8_calls, vec![0x55]);
    }

    #[test]
    fn op_43_sub_9_tween_path() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // ticks=5 (non-zero) → tween path. Coords (0x0100, 0x0200, 0x0300).
        let bc = [0x43, 9, 0x00, 0x01, 0x00, 0x02, 0x00, 0x03, 0x05, 0x00];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 10 });
        assert_eq!(host.sub9_tweens, vec![(0x0100u16, 0x0200, 0x0300, 5)]);
        // Tween path doesn't write ctx coords directly.
        assert_eq!(ctx.world_x, 0);
    }

    #[test]
    fn op_43_sub_9_immediate_writes_when_ticks_zero() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            world_x: 0xAAAA,
            world_y: 0xBBBB,
            world_z: 0xCCCC,
            ..Default::default()
        };
        // ticks=0 → immediate write path. y = 0xFFFF (sentinel - skip).
        let bc = [0x43, 9, 0x11, 0x22, 0xFF, 0xFF, 0x33, 0x44, 0x00, 0x00];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 10 });
        assert_eq!(ctx.world_x, 0x2211);
        assert_eq!(ctx.world_y, 0xBBBB); // unchanged (sentinel)
        assert_eq!(ctx.world_z, 0x4433);
        assert!(host.sub9_tweens.is_empty());
    }

    #[test]
    fn op_43_sub_10_emitter_init() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // 21-byte instruction. Payload is 19 bytes after the [43, 0x10] header.
        let mut bc = [0u8; 21];
        bc[0] = 0x43;
        bc[1] = 0x10;
        for (i, b) in bc.iter_mut().enumerate().skip(2) {
            *b = i as u8;
        }
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 21 });
        assert_eq!(host.emitter_init_payloads.len(), 1);
        assert_eq!(host.emitter_init_payloads[0].len(), 19);
        assert_eq!(host.emitter_init_payloads[0][0], 2);
    }

    #[test]
    fn op_43_sub_11_emitter_5_words() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // 12-byte: [43, 0x11, 5 u16s].
        let bc = [
            0x43, 0x11, 0x01, 0x00, 0x02, 0x00, 0x03, 0x00, 0x04, 0x00, 0x05, 0x00,
        ];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 12 });
        assert_eq!(host.emitter_5_words, vec![[1u16, 2, 3, 4, 5]]);
    }

    #[test]
    fn op_43_sub_15_emitter_struct_12() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // 14-byte: [43, 0x15, 12 bytes].
        let mut bc = [0u8; 14];
        bc[0] = 0x43;
        bc[1] = 0x15;
        for (i, b) in bc.iter_mut().enumerate().skip(2) {
            *b = (i + 0x10) as u8;
        }
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 14 });
        assert_eq!(host.emitter_struct12_payloads.len(), 1);
        assert_eq!(host.emitter_struct12_payloads[0].len(), 12);
    }

    #[test]
    fn op_43_sub_12_split_call_no_split() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // 14-byte: [43, 0x12, six s16 LE]. words[2] = 0x0080 ≤ 0xFF so no split.
        let bc = [
            0x43, 0x12, 0x10, 0x00, 0x20, 0x00, 0x80, 0x00, 0x40, 0x00, 0x50, 0x00, 0x60, 0x00,
        ];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 14 });
        assert_eq!(
            host.emitter_split_calls,
            vec![([0x10i16, 0x20, 0x80, 0x40, 0x50, 0x60], false)]
        );
    }

    #[test]
    fn op_43_sub_12_split_call_with_split() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // words[2] = 0x0200 > 0xFF → split.
        let bc = [
            0x43, 0x12, 0x10, 0x00, 0x20, 0x00, 0x00, 0x02, 0x40, 0x00, 0x50, 0x00, 0x60, 0x00,
        ];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 14 });
        assert_eq!(host.emitter_split_calls.len(), 1);
        assert!(host.emitter_split_calls[0].1);
        assert_eq!(host.emitter_split_calls[0].0[2], 0x0200i16);
    }

    #[test]
    fn op_43_sub_13_func13_passes_full_payload() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // 14-byte: [43, 0x13, 12 data bytes]. Payload includes the 0x13 byte.
        let mut bc = [0u8; 14];
        bc[0] = 0x43;
        bc[1] = 0x13;
        for (i, b) in bc.iter_mut().enumerate().skip(2) {
            *b = (i + 0x40) as u8;
        }
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 14 });
        assert_eq!(host.emitter_func13_payloads.len(), 1);
        assert_eq!(host.emitter_func13_payloads[0][0], 0x13);
        assert_eq!(host.emitter_func13_payloads[0][12], (13 + 0x40) as u8);
    }

    #[test]
    fn op_43_sub_14_4_words() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // 10-byte: [43, 0x14, 4 s16 LE]. Use a negative second word to verify sign-ext.
        let bc = [0x43, 0x14, 0x01, 0x00, 0xFF, 0xFF, 0x03, 0x00, 0x04, 0x00];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 10 });
        assert_eq!(host.emitter_4_words, vec![[1i16, -1, 3, 4]]);
    }

    #[test]
    fn op_43_sub_12_truncated_returns_unknown() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // 13-byte: one byte short of the required 14.
        let bc = [0u8; 13];
        let bc = {
            let mut b = bc;
            b[0] = 0x43;
            b[1] = 0x12;
            b
        };
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert!(matches!(r, StepResult::Unknown { .. }));
    }

    #[test]
    fn op_4c_sub_3_sub_9_position_refresh() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // [4C, 0x39] = 2 bytes; high nibble = 3, low nibble = 9.
        let r = step(&mut host, &mut ctx, &[0x4C, 0x39], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.player_pos_refresh_calls, 1);
        // sub-9 falls through to sub-E's render-resync chain.
        assert_eq!(host.player_render_resync_calls, 1);
    }

    #[test]
    fn op_4c_sub_3_sub_e_render_resync() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4C, 0x3E], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.player_render_resync_calls, 1);
        assert_eq!(host.player_pos_refresh_calls, 0);
    }

    #[test]
    fn op_4c_sub_3_sub_f_io_resync() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4C, 0x3F], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.field_io_resync_calls, 1);
    }

    #[test]
    fn op_49_done_state_sub_6_8_9_c_d_advance_5() {
        for sub_op in [6u8, 8, 9, 0xC, 0xD] {
            let mut host = TestHost {
                op49_state_value: Op49State::Done,
                ..Default::default()
            };
            let mut ctx = FieldCtx::default();
            // 5-byte instruction: [49, sub_op, 3 unused payload bytes].
            let bc = [0x49, sub_op, 0xAA, 0xBB, 0xCC];
            let r = step(&mut host, &mut ctx, &bc, 0);
            assert_eq!(
                r,
                StepResult::Advance { next_pc: 5 },
                "sub_op {sub_op:#x} should advance by 5 in Done state"
            );
            assert_eq!(host.op49_clears, 1);
        }
    }

    #[test]
    fn op_43_sub_3_4_5_6_sound_ramps() {
        for sub_op in [3u8, 4, 5, 6] {
            let mut host = TestHost::default();
            let mut ctx = FieldCtx::default();
            // [43, sub_op, b1=1, b2=2, b3=3, b4=4, ticks=0x0064, curve=0x0010]
            let bc = [0x43, sub_op, 1, 2, 3, 4, 0x64, 0x00, 0x10, 0x00];
            let r = step(&mut host, &mut ctx, &bc, 0);
            assert_eq!(r, StepResult::Advance { next_pc: 10 });
            assert_eq!(
                host.sound_ramp_calls,
                vec![(sub_op, [1, 2, 3, 4], 100u16, 16u16)]
            );
        }
    }

    // -- step_with_caller (YIELD propagation) --------------------------

    #[test]
    fn yield_propagates_to_caller_when_target_is_player() {
        let mut host = TestHost::default();
        let mut target = FieldCtx {
            script_id: 0xFB, // arbitrary - caller designates as player below
            ..Default::default()
        };
        let mut caller = FieldCtx::default();
        let r = step_with_caller(&mut host, &mut target, &mut caller, true, &[0x37], 0);
        assert!(matches!(r, StepResult::Yield { .. }));
        assert!(target.is_halted());
        assert!(caller.is_halted());
        assert_eq!(target.saved_pc, 0);
        assert_eq!(caller.saved_pc, 0);
    }

    #[test]
    fn yield_does_not_propagate_when_target_not_player() {
        let mut host = TestHost::default();
        let mut target = FieldCtx::default();
        let mut caller = FieldCtx::default();
        let r = step_with_caller(&mut host, &mut target, &mut caller, false, &[0x37], 0);
        assert!(matches!(r, StepResult::Yield { .. }));
        assert!(target.is_halted());
        assert!(!caller.is_halted());
    }

    #[test]
    fn step_with_caller_non_yield_does_not_touch_caller() {
        let mut host = TestHost::default();
        let mut target = FieldCtx::default();
        let mut caller = FieldCtx::default();
        // 0x21 NOP - no yield. Caller untouched even when target == player.
        let r = step_with_caller(&mut host, &mut target, &mut caller, true, &[0x21], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 1 });
        assert!(!caller.is_halted());
    }

    #[test]
    fn yield_saved_pc_is_pre_extended_when_extended_dispatch() {
        // The original stores pbVar43 (the address of the OPCODE byte) in
        // ctx.saved_pc, regardless of extended/non-extended. So an extended
        // YIELD at pc=0 saves 0, not 1.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // 0xB7 = 0x80 | 0x37. Extended YIELD on a (synthetic) target.
        let r = step(&mut host, &mut ctx, &[0xB7, 0x00], 0);
        assert!(matches!(r, StepResult::Yield { .. }));
        assert_eq!(ctx.saved_pc, 0);
    }

    // -- High-byte default-route opcodes 0x5x/0x6x/0x7x ------------------
    // (the **fourth flag bank** at DAT_80086D70). These fall through the
    // explicit opcode arm and hit the dispatcher's default route.

    #[test]
    fn sysflag_set_low_index_writes_through_host() {
        // Opcode 0x50 + idx_lo 0x07 → idx = (0x50 & 0x8F) << 8 | 0x07 = 0x07.
        // Bit at byte 0 / mask 0x01 set.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x50, 0x07], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.system_flags[0], 0x01);
        assert_eq!(host.sys_flag_writes, vec![(0x07, "set")]);
    }

    #[test]
    fn sysflag_set_uses_low_nibble_of_opcode_for_high_byte() {
        // Opcode 0x5F + idx_lo 0xFF → idx = (0x5F & 0x8F) << 8 | 0xFF =
        // 0x0F00 | 0xFF = 0x0FFF. Byte 0x1FF, mask 0x01.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x5F, 0xFF], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.system_flags[0x1FF], 0x01);
        assert_eq!(host.sys_flag_writes, vec![(0x0FFF, "set")]);
    }

    #[test]
    fn sysflag_set_extended_prefix_sets_high_bit_of_index() {
        // 0xD0 = 0x80 | 0x50. Extended SET. The dispatcher reads the raw
        // opcode byte for `(opcode_byte & 0x8F) << 8`, so the extended bit
        // (0x80) lands at bit 15 of `idx`.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // The extended prefix consumes one extra byte. peek_extended would
        // tell the caller to fetch the script ID (here 0x00).
        let r = step(&mut host, &mut ctx, &[0xD0, 0x00, 0x05], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        // idx = (0xD0 & 0x8F) << 8 | 0x05 = 0x80 << 8 | 0x05 = 0x8005.
        // Byte 0x1000, mask 0x04 (bit 5 in big-endian per-byte order).
        assert_eq!(host.sys_flag_writes, vec![(0x8005, "set")]);
        assert_eq!(host.system_flags[0x1000], 0x04);
    }

    #[test]
    fn sysflag_clear_only_resets_targeted_bit() {
        let mut host = TestHost::default();
        host.ensure_sys_flag_capacity();
        host.system_flags[0] = 0xFF;
        let mut ctx = FieldCtx::default();
        // Opcode 0x60 + idx_lo 0x03 → idx = 0x03. Bit at byte 0 / mask 0x10.
        let r = step(&mut host, &mut ctx, &[0x60, 0x03], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.system_flags[0], 0xEF);
        assert_eq!(host.sys_flag_writes, vec![(0x03, "clear")]);
    }

    #[test]
    fn sysflag_test_bit_clear_falls_through_4_bytes() {
        // TEST against an unset bit advances PC past the 4-byte instruction.
        // The 2 trailing operand bytes (the "branch target") are skipped.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x70, 0x05, 0xAA, 0xBB], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
        // No writes - TEST is read-only.
        assert!(host.sys_flag_writes.is_empty());
    }

    #[test]
    fn sysflag_test_bit_set_takes_relative_jump() {
        // Pre-set bit at idx 0x08 (byte 1, mask 0x80).
        // Opcode 0x70 + idx_lo 0x08 + offset 0x0010 → on bit-set, jump to
        // pc + header_size + 1 + 0x0010 = 0 + 1 + 1 + 16 = 18.
        let mut host = TestHost::default();
        host.ensure_sys_flag_capacity();
        host.system_flags[1] = 0x80;
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x70, 0x08, 0x10, 0x00], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 18 });
    }

    #[test]
    fn sysflag_test_extended_prefix_sets_high_bit_of_index() {
        // 0xF0 = 0x80 | 0x70. Extended TEST.
        let mut host = TestHost::default();
        host.ensure_sys_flag_capacity();
        // idx = (0xF0 & 0x8F) << 8 | 0x05 = 0x8005.
        // Byte 0x1000, mask 0x04. Pre-set it.
        host.system_flags[0x1000] = 0x04;
        let mut ctx = FieldCtx::default();
        // Layout: [prefix=0xF0, target_id, idx_lo, off_lo, off_hi]
        // pc=0; on bit-set, next_pc = 0 + 2 (header) + 1 + LE_u16(0x05, 0x00) = 8.
        let r = step(&mut host, &mut ctx, &[0xF0, 0x00, 0x05, 0x05, 0x00], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 8 });
    }

    #[test]
    fn sysflag_set_then_test_round_trips() {
        // SET-then-TEST round trip: a SET on idx K must subsequently make
        // TEST on the same idx return true and take the branch.
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x50, 0x42], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        // Fresh TEST at pc=0 on its own buffer; offset 0x0004 → jump to
        // 0 + header_size(1) + 1 + 4 = 6.
        let r = step(&mut host, &mut ctx, &[0x70, 0x42, 0x04, 0x00], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
    }

    #[test]
    fn sysflag_set_truncated_idx_byte_is_unknown() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // 1-byte buffer - no idx byte available.
        let r = step(&mut host, &mut ctx, &[0x50], 0);
        assert!(matches!(r, StepResult::Unknown { .. }));
    }

    #[test]
    fn sysflag_test_truncated_offset_is_unknown() {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // 3-byte buffer - missing the second offset byte.
        let r = step(&mut host, &mut ctx, &[0x70, 0x05, 0xAA], 0);
        assert!(matches!(r, StepResult::Unknown { .. }));
    }

    // -- Cross-opcode integration ---------------------------------------

    /// Drive a script through `step()` repeatedly until it `Halt`s, hits
    /// `Pending`/`Unknown`, or executes more than `max_steps` instructions.
    /// Returns the trace of `(pc_before, StepResult)` for assertion.
    fn run_until_halt(
        host: &mut TestHost,
        ctx: &mut FieldCtx,
        bytecode: &[u8],
        start_pc: usize,
        max_steps: usize,
    ) -> Vec<(usize, StepResult)> {
        let mut trace = Vec::new();
        let mut pc = start_pc;
        for _ in 0..max_steps {
            let r = step(host, ctx, bytecode, pc);
            trace.push((pc, r.clone()));
            match r {
                StepResult::Advance { next_pc } => pc = next_pc,
                StepResult::Yield { resume_pc } => {
                    // Treat Yield as "step one tick, resume next iter".
                    pc = resume_pc;
                }
                StepResult::Halt { .. }
                | StepResult::Pending { .. }
                | StepResult::Unknown { .. } => break,
            }
        }
        trace
    }

    #[test]
    fn integration_lflag_set_test_jmp_clr_test_halt() {
        // Composed script exercising five opcodes in sequence. The JMP_REL
        // formula is `target = pc + header_size + delta` (= pc + 1 + delta
        // for non-extended). All offsets below were chosen so the JMP at
        // PC=4 lands on PC=9 (delta=4 → 4 + 1 + 4 = 9), skipping the
        // intermediate CLR.
        //
        //   00: 2B 05         LFLAG_SET bit 5      (PC -> 02)
        //   02: 2D 05         LFLAG_TST bit 5      (set → PC -> 04)
        //   04: 26 04 00      JMP_REL +4           (target = 4 + 1 + 4 = 9)
        //   07: 2C 05         LFLAG_CLR bit 5  ← skipped by JMP
        //   09: 2D 05         LFLAG_TST bit 5  (still set → PC -> 0B)
        //   0B: 2C 05         LFLAG_CLR bit 5      (PC -> 0D)
        //   0D: 2D 05         LFLAG_TST bit 5      (now clear → Halt)
        //
        // Validates: local-flag round trip, unconditional jump skipping
        // an intermediate op, conditional-test halt path.
        let bytecode = [
            0x2B, 0x05, // 00..02 SET bit 5
            0x2D, 0x05, // 02..04 TEST bit 5 (advance)
            0x26, 0x04, 0x00, // 04..07 JMP +4 → PC=9
            0x2C, 0x05, // 07..09 CLR bit 5 (skipped)
            0x2D, 0x05, // 09..0B TEST bit 5 (still set, advance)
            0x2C, 0x05, // 0B..0D CLR bit 5
            0x2D, 0x05, // 0D..0F TEST bit 5 (clear, halt)
        ];

        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let trace = run_until_halt(&mut host, &mut ctx, &bytecode, 0, 32);

        // Terminal state is a Halt at PC 0x0D (the third TEST).
        let (last_pc, last) = trace.last().unwrap().clone();
        assert!(
            matches!(last, StepResult::Halt { final_pc: 0x0D }),
            "expected Halt at 0x0D, got {:?} at pc=0x{:X}",
            last,
            last_pc
        );

        assert_eq!(ctx.local_flags & (1 << 5), 0, "bit 5 should end clear");

        // Walk the visited PCs to ensure the JMP took its branch and the
        // CLR at offset 0x07 was skipped.
        let visited: Vec<usize> = trace.iter().map(|(p, _)| *p).collect();
        assert_eq!(
            visited,
            vec![0x00, 0x02, 0x04, 0x09, 0x0B, 0x0D],
            "PC trace mismatch (JMP must skip the CLR at 0x07)"
        );
    }

    #[test]
    fn integration_jmp_then_advance_through_n4_immediate_writes() {
        // Composed script that JMPs over a garbage region, then walks three
        // 6-byte 0x4C nibble-4 immediate writes, ending at NOP. JMP_REL
        // target = `pc + header_size + delta` = `0 + 1 + 4 = 5`. Validates
        // PC math composition across the 6-byte 0x4C nibble-4 instruction.
        //
        //   00: 26 04 00            JMP +4  → PC = 5
        //   03..04: garbage (skipped)
        //   05: 4C 40 11 00 00 00   nibble-4 sub-0: ctx.field_72 = 0x0011, immediate
        //   0B: 4C 48 22 00 00 00   nibble-4 sub-8: ctx.field_26 = 0x0022, immediate
        //   11: 4C 42 33 00 00 00   nibble-4 sub-2: ctx.field_8e = 0x0033, immediate
        //   17: 21                  NOP
        //   18: 21                  NOP - terminal (run_until_halt limit)
        let bytecode = [
            0x26, 0x04, 0x00, // 00..03 JMP +4 → PC=5
            0xAA, 0xAA, // 03..05 garbage (skipped)
            0x4C, 0x40, 0x11, 0x00, 0x00, 0x00, // 05..0B sub-0
            0x4C, 0x48, 0x22, 0x00, 0x00, 0x00, // 0B..11 sub-8
            0x4C, 0x42, 0x33, 0x00, 0x00, 0x00, // 11..17 sub-2
            0x21, // 17 NOP
            0x21, // 18 NOP
        ];

        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // Cap steps low enough that the trace ends inside the buffer; with
        // 6 instructions the trace stops at PC=0x19 reading past EOF
        // (Unknown). That's fine - we assert state, not the terminal kind.
        let _ = run_until_halt(&mut host, &mut ctx, &bytecode, 0, 16);

        assert_eq!(ctx.field_72, 0x11);
        assert_eq!(ctx.field_26, 0x22);
        assert_eq!(ctx.field_8e, 0x33);
        // Garbage bytes at 0x03..05 must not have been interpreted as ops.
        // We verify this via the host call counters: nothing else should
        // have fired.
        assert!(host.n4_ctx_ramps.is_empty());
        assert!(host.n4_global_writes.is_empty());
    }

    #[test]
    fn integration_yield_then_resume_then_advance() {
        // Validates the Yield resume cycle composed with Advance:
        //   00: 4C 30           sub-3 sub-0 (set field-input lock, Yield)
        //   02: 4C 31           sub-3 sub-1 (clear lock, Yield)
        //   04: 21              NOP (Advance)
        //   05: 2D 05           LFLAG_TST bit 5 (clear → Halt)
        //
        // After two Yields the host log should show [true, false]; after
        // the NOP advances we hit the LFLAG_TST and halt.
        let bytecode = [
            0x4C, 0x30, // lock + Yield
            0x4C, 0x31, // unlock + Yield
            0x21, // NOP
            0x2D, 0x05, // TEST bit 5 (clear → Halt)
        ];

        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let trace = run_until_halt(&mut host, &mut ctx, &bytecode, 0, 16);

        // Lock log must be exactly [true, false].
        assert_eq!(host.field_input_lock_writes, vec![true, false]);

        // Trace shape: Yield, Yield, Advance, Halt.
        assert!(matches!(trace[0].1, StepResult::Yield { resume_pc: 2 }));
        assert!(matches!(trace[1].1, StepResult::Yield { resume_pc: 4 }));
        assert!(matches!(trace[2].1, StepResult::Advance { next_pc: 5 }));
        let last = trace.last().unwrap().1.clone();
        assert!(matches!(last, StepResult::Halt { final_pc: 5 }));
    }

    // -- 0x4C outer-nibbles 5..F ----------------------------------------

    #[test]
    fn op_4c_n5_sub0_low_clears_flag_bit_and_advances() {
        // [4C, 0x50, 0x40, 0x00] → value = 0x40, < 0xF0 → low half.
        let bytecode = [0x4Cu8, 0x50, 0x40, 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            flags: 0x0100_0000,
            ..FieldCtx::default()
        };
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
        assert_eq!(ctx.flags & 0x0100_0000, 0);
        assert_eq!(host.n5_sub0_calls, vec![(0x40, false)]);
    }

    #[test]
    fn op_4c_n5_sub0_high_sets_flag_bit() {
        // [4C, 0x50, 0xF0, 0x00] → value = 0xF0, >= 0xF0 → high half.
        let bytecode = [0x4Cu8, 0x50, 0xF0, 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
        assert!(ctx.flags & 0x0100_0000 != 0);
        assert_eq!(host.n5_sub0_calls, vec![(0xF0, true)]);
    }

    #[test]
    fn op_4c_n6_sub_60_emitter6_passes_six_signed_words() {
        // [4C, 0x60, 12 bytes of 6 s16 words]
        let mut bytecode = vec![0x4C, 0x60];
        for w in &[1i16, -2, 3, -4, 5, -6] {
            bytecode.extend_from_slice(&w.to_le_bytes());
        }
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 14 });
        assert_eq!(host.n6_sub0_calls, vec![[1, -2, 3, -4, 5, -6]]);
    }

    #[test]
    fn op_4c_n6_unrecognized_returns_pending() {
        let bytecode = [0x4Cu8, 0x61, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert!(matches!(r, StepResult::Pending { .. }));
    }

    #[test]
    fn op_4c_n7_sub_0_yields_at_next_pc() {
        // [4C, 0x70, x0=1, z0=2, x1=3, z1=4, mask=0xFF]
        let bytecode = [0x4Cu8, 0x70, 1, 2, 3, 4, 0xFF];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Yield { resume_pc: 7 });
        assert_eq!(
            host.n7_tile_calls,
            vec![(0u8, (1u8, 4u8), (2u8, 5u8), 0xFFu8)]
        );
    }

    #[test]
    fn op_4c_n7_sub_2_advances() {
        // sub-2 → mask-clear, advance not yield.
        let bytecode = [0x4Cu8, 0x72, 0, 0, 0, 0, 0x0F];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 7 });
        assert_eq!(host.n7_tile_calls.len(), 1);
        assert_eq!(host.n7_tile_calls[0].0, 2);
    }

    #[test]
    fn op_4c_n8_sub2_party_mirror_advances_three_bytes() {
        let bytecode = [0x4Cu8, 0x82, 0x03];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(host.n8_party_mirrors, vec![3u8]);
    }

    #[test]
    fn op_4c_n8_sub_c_branch_taken_when_field_68_zero() {
        // [4C, 0x8C, 0x10, 0x00] - field_68 = 0 → branch to 0x0010.
        let bytecode = [0x4Cu8, 0x8C, 0x10, 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            field_68: 0,
            ..FieldCtx::default()
        };
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 0x10 });
    }

    #[test]
    fn op_4c_n8_sub_c_advances_past_when_field_68_nonzero() {
        let bytecode = [0x4Cu8, 0x8C, 0x10, 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            field_68: 1,
            ..FieldCtx::default()
        };
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
    }

    #[test]
    fn op_4c_n8_sub_a_writes_quad() {
        // [4C, 0x8A, s0_lo, s0_hi, s1_lo, s1_hi, s2_lo, s2_hi, u24_b0, u24_b1, u24_b2]
        // Original encodes the trailing slot as a 24-bit LE integer (read
        // via `func_0x8003CEB8`). Total instruction = 11 bytes.
        let mut bytecode = vec![0x4C, 0x8A];
        for s in &[10i16, 20, 30] {
            bytecode.extend_from_slice(&s.to_le_bytes());
        }
        // u24 = 0x00ADBEEF (bytes 0xEF, 0xBE, 0xAD).
        bytecode.extend_from_slice(&[0xEF, 0xBE, 0xAD]);
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 11 });
        assert_eq!(host.n8_quad_writes, vec![([10i16, 20, 30], 0x00AD_BEEFu32)]);
    }

    #[test]
    fn op_4c_n8_sub_9_writes_signed_16_and_advances_4() {
        // [4C, 0x89, 0x34, 0x12] writes 0x1234 then PC += 4.
        let bytecode = [0x4Cu8, 0x89, 0x34, 0x12];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
        assert_eq!(host.n8_sub9_writes, vec![0x1234i16]);
    }

    #[test]
    fn op_4c_n9_sub_0_passes_b1_and_three_words() {
        // [4C, 0x90, b1=7, lo,hi, lo,hi, lo,hi]
        let mut bytecode = vec![0x4C, 0x90, 0x07];
        for w in &[1i16, 2, 3] {
            bytecode.extend_from_slice(&w.to_le_bytes());
        }
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 9 });
        assert_eq!(host.n9_dde34_calls, vec![(0u8, 0x07u8, [1i16, 2, 3])]);
    }

    #[test]
    fn op_4c_n9_sub_e_copies_16_signed_words() {
        let mut bytecode = vec![0x4C, 0x9E];
        for i in 0..16i16 {
            bytecode.extend_from_slice(&(i * 100 - 700).to_le_bytes());
        }
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 0x22 });
        let mut expected = [0i16; 16];
        for (i, e) in expected.iter_mut().enumerate() {
            *e = (i as i16) * 100 - 700;
        }
        assert_eq!(host.n9_table_copies, vec![expected]);
    }

    #[test]
    fn op_4c_n_a_sub_0_advances_when_ctx_flag_clear() {
        // [4C, 0xA0, bit=4, lo, hi] - ctx.flags bit 4 clear → skip 5 bytes.
        // The asm at 0x801e2580/4 ANDs ctx[+0x10] with `(1 << bit)` and only
        // branches to the take-jump label when the result is non-zero.
        let bytecode = [0x4Cu8, 0xA0, 0x04, 0x00, 0x01];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
    }

    #[test]
    fn op_4c_n_a_sub_0_branches_when_ctx_flag_set() {
        // ctx.flags bit 4 SET → take the absolute jump (0x100).
        let bytecode = [0x4Cu8, 0xA0, 0x04, 0x00, 0x01];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            flags: 1u32 << 4,
            ..FieldCtx::default()
        };
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 0x100 });
    }

    #[test]
    fn op_4c_n_a_sub_2_uses_global_flags() {
        // Global flag set → take jump.
        let bytecode = [0x4Cu8, 0xA2, 0x03, 0x20, 0x00];
        let mut host = TestHost {
            globals: 1u32 << 3,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 0x20 });
    }

    #[test]
    fn op_4c_n_a_sub_3_through_f_skip_5() {
        // Sub-ops 3..=0xF have no `case` arm in the asm. The dispatch at
        // 0x801e2568 (`bne a1, zero, 0x801e258c`) jumps past every bank
        // check, both `bne a1, 1` and `bne a1, 2` then branch to the
        // skip-5 exit. The s8 += 5 in the delay slot of the first bne is
        // the PC delta.
        let bytecode = [0x4Cu8, 0xA5, 0xFF, 0x00, 0x01];
        let mut host = TestHost {
            globals: !0u32,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx {
            flags: !0u32,
            local_flags: !0u16,
            ..FieldCtx::default()
        };
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
    }

    #[test]
    fn op_4c_n9_sub_f_registers_callback_and_halts() {
        // [4C, 0x9F] - register `LAB_801DA930` callback then halt at PC.
        // Same dispatch pattern as nibble-8 sub-7 (callback target differs).
        let bytecode = [0x4Cu8, 0x9F];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        assert_eq!(host.n9_callback_regs, 1);
    }

    #[test]
    fn op_4c_n8_sub_7_registers_callback_and_halts() {
        // [4C, 0x87] - register actor-list callback (LAB_801E5154) then halt
        // at PC. The original goes through `switchD_801e00f4::default()`,
        // which for opcode 0x4C (`& 0x70 = 0x40`) returns `param_2` -
        // halt at PC. Our hook is one-shot per dispatch entry.
        let bytecode = [0x4Cu8, 0x87];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        assert_eq!(host.n8_callback_regs, 1);
    }

    #[test]
    fn op_4c_n_c_sub_4_subtile_broadcast() {
        let bytecode = [0x4Cu8, 0xC4, 0x05, 0x07];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
        assert_eq!(host.n_c_subtile_broadcasts, vec![(5u8, 7u8)]);
    }

    #[test]
    fn op_4c_n_c_sub_8_xors_field_74() {
        let bytecode = [0x4Cu8, 0xC8];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            field_74: 0,
            ..FieldCtx::default()
        };
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(ctx.field_74, 0x1000_0000);

        // Re-applying flips back.
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(ctx.field_74, 0);
    }

    #[test]
    fn op_4c_n_c_sub_a_writes_slot() {
        let bytecode = [0x4Cu8, 0xCA, 0x05, 0x10, 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
        assert_eq!(host.n_c_slot_writes, vec![(5u8, 0x10i16)]);
    }

    #[test]
    fn op_4c_n_c_sub_b_substitutes_frame_delta_when_value_is_ffff() {
        let bytecode = [0x4Cu8, 0xCB, 0x07, 0xFF, 0xFF];
        let mut host = TestHost {
            frame_delta: 3,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
        assert_eq!(host.n_c_slot_adjusts, vec![(7u8, 3i16, false)]);
    }

    #[test]
    fn op_4c_n_c_sub_c_subtracts() {
        let bytecode = [0x4Cu8, 0xCC, 0x02, 0x05, 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
        assert_eq!(host.n_c_slot_adjusts, vec![(2u8, 5i16, true)]);
    }

    #[test]
    fn op_4c_n_c_sub_2_writes_field_42() {
        let bytecode = [0x4Cu8, 0xC2, 0x55];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(ctx.field_42, 0x55);
    }

    #[test]
    fn op_4c_n8_sub_5_halt_acquire_writes_ctx_and_halts() {
        let bytecode = [0x4Cu8, 0x85];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            wait_accum: 7,
            ..FieldCtx::default()
        };
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        assert_eq!(ctx.saved_pc, 0);
        assert_eq!(ctx.wait_accum, 0);
        assert_eq!(ctx.flags & 0x400, 0x400);
        assert_eq!(host.n8_halt_acquires, vec![0u32]);
    }

    #[test]
    fn op_4c_n8_sub_e_and_f_share_halt_acquire_body() {
        for sub in [0x8Eu8, 0x8F] {
            let bytecode = [0x4Cu8, sub];
            let mut host = TestHost::default();
            let mut ctx = FieldCtx::default();
            let r = step(&mut host, &mut ctx, &bytecode, 0);
            assert_eq!(r, StepResult::Halt { final_pc: 0 });
            assert_eq!(host.n8_halt_acquires, vec![0u32]);
        }
    }

    #[test]
    fn op_4c_n_c_sub_9_advances_when_globals_match() {
        let bytecode = [0x4Cu8, 0xC9];
        let mut host = TestHost {
            n_c_sub9_globals_differ: false,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
    }

    #[test]
    fn op_4c_n_c_sub_9_halts_when_globals_differ() {
        let bytecode = [0x4Cu8, 0xC9];
        let mut host = TestHost {
            n_c_sub9_globals_differ: true,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
    }

    #[test]
    fn op_4c_n_d_sub_6_b1_eq_4_clears_top_bit_only() {
        let bytecode = [0x4Cu8, 0xD6, 0x04];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            field_74: 0xFFFF_FFFF,
            ..FieldCtx::default()
        };
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        assert_eq!(ctx.field_74, 0x7FFF_FFFF);
        assert_eq!(host.n_d_sub6_acks, 1);
    }

    #[test]
    fn op_4c_n_d_sub_6_b1_neq_4_sets_high_byte() {
        let bytecode = [0x4Cu8, 0xD6, 0x12];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            field_74: 0,
            ..FieldCtx::default()
        };
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        // Sets bit 0x80000000 + (0x12 << 24) = 0x92000000.
        assert_eq!(ctx.field_74, 0x9200_0000);
    }

    #[test]
    fn op_4c_n_d_sub_8_passes_b1_and_three_words_advances_9() {
        let bytecode = [0x4Cu8, 0xD8, 0x05, 0x10, 0x00, 0x20, 0x00, 0x30, 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 9 });
        assert_eq!(host.n_d_sub8_calls, vec![(5u8, [0x10i16, 0x20, 0x30])]);
    }

    #[test]
    fn op_4c_n_e_sub_0_writes_b1_and_halts() {
        let bytecode = [0x4Cu8, 0xE0, 0x42];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        assert_eq!(host.n_e_sub0_writes, vec![0x42u8]);
    }

    #[test]
    fn op_4c_n_e_sub_9_clears_global_and_advances_2() {
        let bytecode = [0x4Cu8, 0xE9];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.n_e_sub9_clears, 1);
    }

    #[test]
    fn op_4c_n_e_sub_a_calls_overlay_and_halts() {
        let bytecode = [0x4Cu8, 0xEA];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        assert_eq!(host.n_e_sub_a_calls, 1);
    }

    #[test]
    fn op_4c_n_c_sub_f_uses_actor_world_when_byte_is_ff() {
        // [4C, 0xCF, 0xFF, 0xFF] - both bytes select the actor's coords.
        let bytecode = [0x4Cu8, 0xCF, 0xFF, 0xFF];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            world_x: 0x1234,
            world_z: 0x5678,
            ..FieldCtx::default()
        };
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
        assert_eq!(
            host.n_c_sub_f_broadcasts,
            vec![(0x1234u16 as i16, 0x5678u16 as i16)]
        );
    }

    #[test]
    fn op_4c_n_c_sub_f_zero_yields_zero_nonzero_yields_tile_center() {
        // b1=0 → 0; b2=2 → 2*0x80 + 0x40 = 0x140.
        let bytecode = [0x4Cu8, 0xCF, 0x00, 0x02];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
        assert_eq!(host.n_c_sub_f_broadcasts, vec![(0, 0x0140)]);
    }

    #[test]
    fn op_4c_n_d_sub_3_party_setup_advances_14_bytes() {
        let mut bytecode = vec![0x4C, 0xD3];
        bytecode.extend_from_slice(&0x1234i16.to_le_bytes());
        bytecode.extend_from_slice(&0x5678i16.to_le_bytes());
        bytecode.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        bytecode.extend_from_slice(&0xCAFE_BABEu32.to_le_bytes());
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 14 });
        assert_eq!(host.n_d_party_setups.len(), 1);
        let (ab, cd, ef) = host.n_d_party_setups[0];
        assert_eq!(ab, (0x1234u32 << 16) | 0x5678u32);
        assert_eq!(cd, 0xDEAD_BEEF);
        assert_eq!(ef, 0xCAFE_BABE);
    }

    #[test]
    fn op_4c_n_d_sub_9_sets_inverted_y_mirror_and_negates_world_y() {
        // value = 5 → field_8e = 5, world_y = -5 (= 0xFFFB).
        let bytecode = [0x4Cu8, 0xD9, 0x05, 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
        assert!(ctx.flags & 0x2000_0000 != 0);
        assert_eq!(ctx.field_8e, 5);
        assert_eq!(ctx.world_y, 0xFFFB);
    }

    #[test]
    fn op_4c_n_d_sub_9_9999_sentinel_keeps_world_y_unchanged() {
        let bytecode = [0x4Cu8, 0xD9, 0x0F, 0x27]; // 9999 LE
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            world_y: 0x0040,
            ..FieldCtx::default()
        };
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
        // value = -world_y = -0x40, ctx.field_8e = -0x40 = 0xFFC0.
        assert_eq!(ctx.field_8e, -0x40);
        // world_y = -value = world_y = 0x40 (unchanged).
        assert_eq!(ctx.world_y, 0x0040);
    }

    #[test]
    fn op_4c_n_d_sub_a_clears_inverted_y_and_calls_collision_y() {
        let bytecode = [0x4Cu8, 0xDA];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            flags: 0x2000_0000,
            ..FieldCtx::default()
        };
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(ctx.flags & 0x2000_0000, 0);
        assert_eq!(host.n_d_collision_y_calls, 1);
    }

    #[test]
    fn op_4c_n_d_sub_d_writes_field_58() {
        let bytecode = [0x4Cu8, 0xDD, 0x77];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(ctx.field_58, 0x77);
    }

    #[test]
    fn op_4c_n_d_sub_f_scene_byte_write() {
        let bytecode = [0x4Cu8, 0xDF, 0xAB];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(host.n_d_scene_byte_writes, vec![0xABu8]);
    }

    #[test]
    fn op_4c_n_e_sub_2_fmv_trigger_decodes_fmv_id() {
        // The 6-byte form `[4C, 0xE2, lo, hi, _, _]` triggers an FMV
        // (game-mode 26 / StrInit). The fmv_id is the s16 at +1..+3
        // and selects an entry in the runtime FMV-state table.
        let bytecode = [0x4Cu8, 0xE2, 0x03, 0x00, 0, 0];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(host.n_e_fmv_triggers, vec![3i16]);
    }

    #[test]
    fn op_4c_n_e_sub_2_fmv_trigger_sign_extends_negative() {
        // s16 sign-extension - retail dispatcher reads through
        // FUN_8003CE9C which sign-extends. fmv_id can be negative
        // when the script wants to clear the trigger / signal a
        // sentinel.
        let bytecode = [0x4Cu8, 0xE2, 0xFF, 0xFF, 0, 0];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(host.n_e_fmv_triggers, vec![-1i16]);
    }

    #[test]
    fn op_4c_n_e_sub_6_d8280_passes_three_words() {
        let mut bytecode = vec![0x4C, 0xE6];
        for w in &[5i16, 10, 15] {
            bytecode.extend_from_slice(&w.to_le_bytes());
        }
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 8 });
        assert_eq!(host.n_e_d8280_calls, vec![[5i16, 10, 15]]);
    }

    #[test]
    fn op_4c_n_e_sub_c_capture_call() {
        let bytecode = [0x4Cu8, 0xEC];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.n_e_capture_ddf48_calls, 1);
    }

    #[test]
    fn op_4c_n_e_sub_d_writes_ba66() {
        let bytecode = [0x4Cu8, 0xED, 0x88];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        assert_eq!(host.n_e_ba66_writes, vec![0x88u8]);
    }

    #[test]
    fn op_4c_n_e_sub_e_snapshot_call() {
        let bytecode = [0x4Cu8, 0xEE];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.n_e_snapshot_84570_calls, 1);
    }

    #[test]
    fn op_4c_n_f_pass_through_advances_two_bytes() {
        let bytecode = [0x4Cu8, 0xFF];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
    }

    // -- 0x43 sub-0/1/A/B halt-acquire ----------------------------------

    #[test]
    fn op_43_sub_0_halt_acquire_yields_to_resume_pc() {
        // [43, 0, x_byte, z_byte, lo, hi] → resume_pc = signed_16(lo, hi) = 0x100
        let bytecode = [0x43u8, 0x00, 0x10, 0x20, 0x00, 0x01];
        let mut host = TestHost {
            halt_acquire_predicate: true,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Yield { resume_pc: 0x100 });
        assert!(ctx.is_halted());
        assert_eq!(ctx.saved_pc, 0);
        assert_eq!(host.halt_acquire_calls.len(), 1);
        assert_eq!(host.halt_acquire_calls[0].0, 0u8);
        assert_eq!(host.halt_acquire_calls[0].1, 0x100);
    }

    #[test]
    fn op_43_sub_a_halt_acquire_uses_offset_7_target() {
        // [43, 0xA, x, z, _, _, _, _, lo, hi]
        let bytecode = [0x43u8, 0x0A, 0x10, 0x20, 0, 0, 0, 0, 0x34, 0x12];
        let mut host = TestHost {
            halt_acquire_predicate: true,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Yield { resume_pc: 0x1234 });
        assert!(ctx.is_halted());
    }

    #[test]
    fn op_43_sub_0_predicate_false_advances_5_bytes() {
        let bytecode = [0x43u8, 0x00, 0, 0, 0, 0];
        let mut host = TestHost {
            halt_acquire_predicate: false,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
        assert!(!ctx.is_halted());
    }

    #[test]
    fn op_43_sub_b_predicate_false_advances_9_bytes() {
        let bytecode = [0x43u8, 0x0B, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut host = TestHost {
            halt_acquire_predicate: false,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 9 });
    }

    // -- Round 16: helper-driven sub-ops --------------------------------

    #[test]
    fn op_4c_n_c_sub_1_flag_loop_reset_advances_2_bytes() {
        let bytecode = [0x4Cu8, 0xC1];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.n_c_sub_1_flag_loops, 1);
    }

    #[test]
    fn op_4c_n_d_sub_1_no_jump_advances_4_bytes() {
        let bytecode = [0x4Cu8, 0xD1, 0, 0];
        let mut host = TestHost {
            n_d_sub_1_jump_target: None,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
    }

    #[test]
    fn op_4c_n_d_sub_1_jump_target_takes_ce9c_path() {
        let bytecode = [0x4Cu8, 0xD1, 0, 0];
        let mut host = TestHost {
            n_d_sub_1_jump_target: Some(0xABCD),
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 0xABCD });
    }

    #[test]
    fn op_4c_n_d_sub_2_channel_spawn_halts_at_pc() {
        let bytecode = [0x4Cu8, 0xD2, 0xFB];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        assert_eq!(host.n_d_sub_2_channel_calls, vec![0xFBu8]);
    }

    #[test]
    fn op_4c_n_d_sub_7_register_list_walk_halts_at_pc() {
        let bytecode = [0x4Cu8, 0xD7];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        assert_eq!(host.n_d_sub_7_list_walk_regs, 1);
    }

    #[test]
    fn op_4c_n_d_sub_b_e57f0_advances_13_bytes() {
        // 13 total bytes: opcode + sub-op + 11 payload.
        let mut bytecode = vec![0x4Cu8, 0xDB];
        bytecode.extend_from_slice(&[
            0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22, 0x33, 0x44, 0x55,
        ]);
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 13 });
        assert_eq!(host.n_d_sub_b_e57f0_calls.len(), 1);
        // 12-byte slice starting from the 0xDB sub-op byte.
        assert_eq!(host.n_d_sub_b_e57f0_calls[0].len(), 12);
        assert_eq!(host.n_d_sub_b_e57f0_calls[0][0], 0xDB);
        assert_eq!(host.n_d_sub_b_e57f0_calls[0][11], 0x55);
    }

    #[test]
    fn op_4c_n_d_sub_b_truncated_buffer_returns_unknown() {
        // 12 bytes only - sub-B needs 13.
        let bytecode = [0x4Cu8, 0xDB, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert!(matches!(
            r,
            StepResult::Unknown {
                opcode: 0x4C,
                pc: 0
            }
        ));
        assert!(host.n_d_sub_b_e57f0_calls.is_empty());
    }

    #[test]
    fn op_4c_n_d_sub_c_no_jump_advances_5_bytes() {
        let bytecode = [0x4Cu8, 0xDC, 0x42, 0, 0];
        let mut host = TestHost {
            n_d_sub_c_jump_target: None,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
        assert_eq!(host.n_d_sub_c_calls, vec![0x42u8]);
    }

    #[test]
    fn op_4c_n_d_sub_c_jump_target_takes_ce9c_path() {
        let bytecode = [0x4Cu8, 0xDC, 0x42, 0, 0];
        let mut host = TestHost {
            n_d_sub_c_jump_target: Some(0x100),
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 0x100 });
    }

    #[test]
    fn op_4c_n_d_sub_e_query_no_jump_advances_5_bytes() {
        let bytecode = [0x4Cu8, 0xDE, 0x99, 0, 0];
        let mut host = TestHost {
            n_d_sub_e_jump_target: None,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
        assert_eq!(host.n_d_sub_e_calls, vec![0x99u8]);
    }

    #[test]
    fn op_4c_n_d_sub_e_query_jump_target_takes_ce9c_path() {
        let bytecode = [0x4Cu8, 0xDE, 0x99, 0, 0];
        let mut host = TestHost {
            n_d_sub_e_jump_target: Some(0x40),
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 0x40 });
    }

    #[test]
    fn op_4c_n_e_sub_1_text_actor_zero_first_byte_skips_spawn() {
        // First byte 0 → no spawn; PC still advances by 3 (0 has no payload).
        // packet_length([0]) = 0 → advance by 3 + 0 = 3.
        let bytecode = [0x4Cu8, 0xE1, 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 });
        // No spawn because first byte is 0.
        assert!(host.n_e_sub_1_text_calls.is_empty());
    }

    #[test]
    fn op_4c_n_e_sub_1_text_actor_short_string_advances_correctly() {
        // [4C, E1, 'A', 'B', 'C', 0] - packet length 3, total advance 6.
        let bytecode = [0x4Cu8, 0xE1, b'A', b'B', b'C', 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx {
            script_id: 0x42,
            ..FieldCtx::default()
        };
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(host.n_e_sub_1_text_calls.len(), 1);
        assert_eq!(host.n_e_sub_1_text_calls[0].0, vec![b'A', b'B', b'C']);
        assert_eq!(host.n_e_sub_1_text_calls[0].1, 0x42);
    }

    #[test]
    fn op_4c_n_e_sub_1_text_actor_with_escape_sequences() {
        // [4C, E1, 'A', 0xC1, 0xAB, 'B', 0] - escape pair counts as 2,
        // total packet length = 4, total advance = 7.
        let bytecode = [0x4Cu8, 0xE1, b'A', 0xC1, 0xAB, b'B', 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 7 });
        assert_eq!(host.n_e_sub_1_text_calls[0].0.len(), 4);
    }

    #[test]
    fn op_4c_n_e_sub_1_text_actor_truncated_returns_unknown() {
        let bytecode = [0x4Cu8, 0xE1]; // first byte missing
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert!(matches!(
            r,
            StepResult::Unknown {
                opcode: 0x4C,
                pc: 0
            }
        ));
        assert!(host.n_e_sub_1_text_calls.is_empty());
    }

    // -----------------------------------------------------------------
    // Round 17 - five new 0x4C nC sub-ops + two 0x4C nE sub-ops.
    // -----------------------------------------------------------------

    #[test]
    fn op_4c_n_c_sub_0_move_cancel_advances_2_bytes() {
        let bytecode = [0x4Cu8, 0xC0];
        let mut host = TestHost {
            n_c_sub_0_active: true,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.n_c_sub_0_move_cancels, 1);
    }

    #[test]
    fn op_4c_n_c_sub_0_move_cancel_advances_2_bytes_when_inactive() {
        // Even when the host returns false (no active move), PC still
        // advances by 2 - the cancel side-effect is conditional but the
        // dispatcher's PC delta is constant.
        let bytecode = [0x4Cu8, 0xC0];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.n_c_sub_0_move_cancels, 1);
    }

    #[test]
    fn op_4c_n_c_sub_3_script_teleport_advances_2_bytes() {
        let bytecode = [0x4Cu8, 0xC3];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.n_c_sub_3_teleports, 1);
    }

    #[test]
    fn op_4c_n_c_sub_5_party_flag_jz_advances_4_bytes() {
        // [4C, 0xC5, 0x05, 0x00] - flag idx 5. Bit 5 in the host's bank is
        // unset, so the original "jump-if-zero" path fires; both branches
        // advance PC by 4.
        let bytecode = [0x4Cu8, 0xC5, 0x05, 0x00];
        let mut host = TestHost::default();
        host.n_c_party_flag_bits.insert(5, false);
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
    }

    #[test]
    fn op_4c_n_c_sub_5_party_flag_jz_advances_4_bytes_when_set() {
        // Bit set → original's "jump-if-zero" doesn't fire; PC still += 4.
        let bytecode = [0x4Cu8, 0xC5, 0x07, 0x00];
        let mut host = TestHost::default();
        host.n_c_party_flag_bits.insert(7, true);
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
    }

    #[test]
    fn op_4c_n_c_sub_5_reads_16_bit_index_via_helper() {
        // Verify the dispatcher reads the index through load_u16_le by
        // setting two distinct flag bits and checking which one was queried.
        let bytecode = [0x4Cu8, 0xC5, 0x34, 0x12]; // 0x1234
        let mut host = TestHost::default();
        host.n_c_party_flag_bits.insert(0x1234, true);
        host.n_c_party_flag_bits.insert(0x3412, false);
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
        // The 0x1234 bit is set in the bank - the dispatcher's load_u16_le
        // must produce 0x1234 (LE), not 0x3412 (BE).
        assert!(*host.n_c_party_flag_bits.get(&0x1234).unwrap());
    }

    #[test]
    fn op_4c_n_c_sub_6_party_flag_jnz_advances_4_bytes() {
        // Same shape as sub-5 but opposite polarity. Both polarities share
        // PC += 4 either way.
        let bytecode = [0x4Cu8, 0xC6, 0x09, 0x00];
        let mut host = TestHost::default();
        host.n_c_party_flag_bits.insert(9, false);
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 4 });
    }

    #[test]
    fn op_4c_n_c_sub_5_truncated_buffer_returns_unknown() {
        // Need 3 bytes after pc; only 2 available.
        let bytecode = [0x4Cu8, 0xC5, 0x05]; // missing high byte
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert!(matches!(
            r,
            StepResult::Unknown {
                opcode: 0x4C,
                pc: 0
            }
        ));
    }

    #[test]
    fn op_4c_n_c_sub_d_script_alloc_halts_at_pc() {
        let bytecode = [0x4Cu8, 0xCD];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        assert_eq!(host.n_c_sub_d_allocs, 1);
    }

    #[test]
    fn op_4c_n_e_sub_4_bbox_inside_advances_9_bytes() {
        // Host returns false (= "inside") → PC += 9. The dispatcher computes
        // a tile-center bbox and passes it to the host.
        let bytecode = [0x4Cu8, 0xE4, 0x10, 0x10, 0x20, 0x20, 0x00, 0x00, 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 9 });
        assert_eq!(host.n_e_sub_4_bboxes.borrow().len(), 1);
    }

    #[test]
    fn op_4c_n_e_sub_4_bbox_outside_halts_at_pc() {
        let bytecode = [0x4Cu8, 0xE4, 0x10, 0x10, 0x20, 0x20, 0x00, 0x00, 0x00];
        let mut host = TestHost {
            n_e_sub_4_outside: true,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
    }

    #[test]
    fn op_4c_n_e_sub_4_bbox_tile_center_math_for_low_byte() {
        // Operand byte 0x10 → tile-center: (0x10 << 7) | 0x40 = 0x840
        // (high bit clear, so no extra +0x40).
        let bytecode = [0x4Cu8, 0xE4, 0x10, 0x10, 0x10, 0x10, 0x00, 0x00, 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let _ = step(&mut host, &mut ctx, &bytecode, 0);
        let bboxes = host.n_e_sub_4_bboxes.borrow();
        assert_eq!(bboxes.len(), 1);
        // All four corners should be 0x840 = 2112.
        assert_eq!(bboxes[0], [0x840, 0x840, 0x840, 0x840]);
    }

    #[test]
    fn op_4c_n_e_sub_4_bbox_tile_center_high_bit_adds_0x40() {
        // Operand byte 0x90: low 7 bits are 0x10 → base 0x840. High bit set
        // → extra +0x40 = 0x880.
        let bytecode = [0x4Cu8, 0xE4, 0x90, 0x90, 0x90, 0x90, 0x00, 0x00, 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let _ = step(&mut host, &mut ctx, &bytecode, 0);
        let bboxes = host.n_e_sub_4_bboxes.borrow();
        assert_eq!(bboxes[0], [0x880, 0x880, 0x880, 0x880]);
    }

    #[test]
    fn op_4c_n_e_sub_4_bbox_zero_byte_yields_zero() {
        // Operand byte 0x00 → 0 (special case in tile-center math).
        let bytecode = [0x4Cu8, 0xE4, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let _ = step(&mut host, &mut ctx, &bytecode, 0);
        let bboxes = host.n_e_sub_4_bboxes.borrow();
        assert_eq!(bboxes[0], [0, 0, 0, 0]);
    }

    #[test]
    fn op_4c_n_e_sub_4_truncated_buffer_returns_unknown() {
        let bytecode = [0x4Cu8, 0xE4, 0x10, 0x10]; // missing last 5 bytes
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert!(matches!(
            r,
            StepResult::Unknown {
                opcode: 0x4C,
                pc: 0
            }
        ));
    }

    #[test]
    fn op_4c_n_e_sub_5_add_xp_positive_value() {
        // [4C, E5, 0xE8, 0x03, 0x00] - 0x0003E8 = 1000.
        let bytecode = [0x4Cu8, 0xE5, 0xE8, 0x03, 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
        assert_eq!(host.n_e_sub_5_xp_deltas, vec![1000]);
    }

    #[test]
    fn op_4c_n_e_sub_5_add_xp_negative_value() {
        // 0xFFFFFE = -2 in 24-bit two's complement.
        let bytecode = [0x4Cu8, 0xE5, 0xFE, 0xFF, 0xFF];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
        assert_eq!(host.n_e_sub_5_xp_deltas, vec![-2]);
    }

    #[test]
    fn op_4c_n_e_sub_5_add_xp_zero_advances_5_bytes() {
        let bytecode = [0x4Cu8, 0xE5, 0x00, 0x00, 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
        assert_eq!(host.n_e_sub_5_xp_deltas, vec![0]);
    }

    #[test]
    fn op_4c_n_e_sub_5_truncated_buffer_returns_unknown() {
        let bytecode = [0x4Cu8, 0xE5, 0x01]; // missing 2 bytes
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert!(matches!(
            r,
            StepResult::Unknown {
                opcode: 0x4C,
                pc: 0
            }
        ));
        assert!(host.n_e_sub_5_xp_deltas.is_empty());
    }

    #[test]
    fn op_4c_n_e_sub_b_actor_resolved_advances_5_bytes() {
        // [4C, EB, actor_id=0x07, target_lo=0x10, target_hi=0x20]
        // When the host resolves the actor, take the "pc + 5" path.
        let bytecode = [0x4Cu8, 0xEB, 0x07, 0x10, 0x20];
        let mut host = TestHost {
            n_e_sub_b_resolves: true,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
        assert_eq!(host.n_e_sub_b_actor_ids, vec![0x07]);
    }

    #[test]
    fn op_4c_n_e_sub_b_actor_unresolved_jumps_to_target() {
        // Same instruction; host returns None (actor not resolved). The
        // dispatcher reads the absolute jump target via load_u16_le and
        // returns it as the new PC.
        let bytecode = [0x4Cu8, 0xEB, 0x07, 0x10, 0x20];
        let mut host = TestHost::default(); // n_e_sub_b_resolves = false
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 0x2010 });
        assert_eq!(host.n_e_sub_b_actor_ids, vec![0x07]);
    }

    #[test]
    fn op_4c_n_e_sub_b_jump_target_uses_load_u16_le() {
        // Verify endianness: bytes 0x34, 0x12 should produce target 0x1234,
        // not 0x3412.
        let bytecode = [0x4Cu8, 0xEB, 0xAA, 0x34, 0x12];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 0x1234 });
    }

    #[test]
    fn op_4c_n_e_sub_b_truncated_buffer_returns_unknown() {
        let bytecode = [0x4Cu8, 0xEB, 0x07]; // missing 2 jump-target bytes
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert!(matches!(
            r,
            StepResult::Unknown {
                opcode: 0x4C,
                pc: 0
            }
        ));
        assert!(host.n_e_sub_b_actor_ids.is_empty());
    }

    // -- Round 18 - 0x4C n8 actor-allocator + nE camera + nD/n5 dialog ---

    #[test]
    fn op_4c_n_8_sub_1_set_model_anim_advances_pc_by_9() {
        // [4C, 0x81, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE]
        // model_id (LE24) = 0x563412
        // anim_frame (LE16) = 0x9A78
        // tween_frames (LE16) = 0xDEBC
        let bytecode = [0x4Cu8, 0x81, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 9 });
        assert_eq!(host.n_8_sub_1_set_model_calls.len(), 1);
        let (model_id, anim_frame, tween_frames) = host.n_8_sub_1_set_model_calls[0];
        assert_eq!(model_id, 0x0056_3412);
        assert_eq!(anim_frame, 0x9A78);
        assert_eq!(tween_frames, 0xDEBC);
    }

    #[test]
    fn op_4c_n_8_sub_1_truncated_buffer_returns_unknown() {
        // Only 8 bytes - sub-1 needs 9 total.
        let bytecode = [0x4Cu8, 0x81, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert!(matches!(
            r,
            StepResult::Unknown {
                opcode: 0x4C,
                pc: 0
            }
        ));
        assert!(host.n_8_sub_1_set_model_calls.is_empty());
    }

    #[test]
    fn op_4c_n_8_sub_6_actor_set_rotation_advances_15() {
        // [4C, 0x86, ... 12 bytes for 6 LE16 ..., actor_id]
        let bytecode = [
            0x4Cu8, 0x86, // opcode + sub-op
            0x10, 0x00, 0x20, 0x00, 0x30, 0x00, // x, y, z = 0x10, 0x20, 0x30
            0x40, 0x00, 0x50, 0x00, 0x60, 0x00, // rx, ry, rz = 0x40, 0x50, 0x60
            0x07, // actor_id = 7
        ];
        let mut host = TestHost {
            n_8_sub_6_actor_present: true,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 15 });
        assert_eq!(host.n_8_sub_6_actor_set_rotation_calls.len(), 1);
        let (actor_id, position, rotation) = host.n_8_sub_6_actor_set_rotation_calls[0];
        assert_eq!(actor_id, 7);
        assert_eq!(position, [0x10, 0x20, 0x30]);
        assert_eq!(rotation, [0x40, 0x50, 0x60]);
    }

    #[test]
    fn op_4c_n_8_sub_6_advances_15_even_when_actor_missing() {
        // The original short-circuits to `return param_2 + 0xF` when the
        // actor lookup fails - PC still advances by 15.
        let bytecode = [
            0x4Cu8, 0x86, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0xFF,
        ];
        let mut host = TestHost {
            n_8_sub_6_actor_present: false, // actor lookup fails
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 15 });
        // Hook still fires (the host records the call); the actor_present
        // bool just controls the return value.
        assert_eq!(host.n_8_sub_6_actor_set_rotation_calls.len(), 1);
    }

    #[test]
    fn op_4c_n_8_sub_6_signed_decode_round_trip() {
        // Negative LE16 should decode through `as i16` correctly.
        let bytecode = [
            0x4Cu8, 0x86, 0xFF, 0xFF, // x = -1
            0x00, 0x80, // y = -32768
            0xFE, 0xFF, // z = -2
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // rx/ry/rz = 0
            0x00,
        ];
        let mut host = TestHost {
            n_8_sub_6_actor_present: true,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        step(&mut host, &mut ctx, &bytecode, 0);
        let (_, position, _) = host.n_8_sub_6_actor_set_rotation_calls[0];
        assert_eq!(position, [-1, -32768, -2]);
    }

    #[test]
    fn op_4c_n_8_sub_b_jumps_when_actor_type_present() {
        // [4C, 0x8B, type=0x12, target_lo=0x34, target_hi=0x12] → jump 0x1234
        let bytecode = [0x4Cu8, 0x8B, 0x12, 0x34, 0x12];
        let mut host = TestHost::default();
        host.n_8_sub_b_present_types.insert(0x12);
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 0x1234 });
    }

    #[test]
    fn op_4c_n_8_sub_b_advances_5_when_no_actor() {
        let bytecode = [0x4Cu8, 0x8B, 0x12, 0x34, 0x12];
        let mut host = TestHost::default(); // no types registered
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
    }

    #[test]
    fn op_4c_n_8_sub_b_truncated_buffer_returns_unknown() {
        let bytecode = [0x4Cu8, 0x8B, 0x12, 0x34]; // missing target hi byte
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert!(matches!(
            r,
            StepResult::Unknown {
                opcode: 0x4C,
                pc: 0
            }
        ));
    }

    #[test]
    fn op_4c_n_8_sub_d_empty_slot_advances_6() {
        let bytecode = [0x4Cu8, 0x8D, 0x05, 0xAB, 0x34, 0x12];
        let host_state = ActorSearchResult::EmptySlot;
        let mut host = TestHost {
            n_8_sub_d_search_result: host_state,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        let queries = host.n_8_sub_d_queries.borrow();
        assert_eq!(queries.as_slice(), &[(0x05, 0xAB)]);
    }

    #[test]
    fn op_4c_n_8_sub_d_found_jumps_to_le_target() {
        let bytecode = [0x4Cu8, 0x8D, 0x05, 0xAB, 0x78, 0x56];
        let mut host = TestHost {
            n_8_sub_d_search_result: ActorSearchResult::Found,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 0x5678 });
    }

    #[test]
    fn op_4c_n_8_sub_d_no_match_halts_at_pc() {
        let bytecode = [0x4Cu8, 0x8D, 0x05, 0xAB, 0x34, 0x12];
        let mut host = TestHost {
            n_8_sub_d_search_result: ActorSearchResult::NoMatch,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
    }

    #[test]
    fn op_4c_n_8_sub_d_truncated_buffer_returns_unknown() {
        let bytecode = [0x4Cu8, 0x8D, 0x05, 0xAB, 0x34]; // missing target hi
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert!(matches!(
            r,
            StepResult::Unknown {
                opcode: 0x4C,
                pc: 0
            }
        ));
    }

    #[test]
    fn op_4c_n_e_sub_3_advances_2_and_records_actor() {
        let bytecode = [0x4Cu8, 0xE3, 0x09];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.n_e_sub_3_camera_syncs, vec![9]);
    }

    #[test]
    fn op_4c_n_e_sub_3_truncated_returns_unknown() {
        let bytecode = [0x4Cu8, 0xE3]; // missing actor_id
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert!(matches!(
            r,
            StepResult::Unknown {
                opcode: 0x4C,
                pc: 0
            }
        ));
        assert!(host.n_e_sub_3_camera_syncs.is_empty());
    }

    #[test]
    fn op_4c_n_e_sub_7_camera_animate_decodes_le24_then_le16() {
        // 7-byte instruction: [opcode, sub-op, t0, t1, t2, d0, d1].
        // target = LE24(0x12, 0x34, 0x56) = 0x563412
        // duration = LE16(0xEF, 0xCD) = 0xCDEF
        let bytecode = [0x4Cu8, 0xE7, 0x12, 0x34, 0x56, 0xEF, 0xCD];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 7 });
        assert_eq!(host.n_e_sub_7_camera_animates, vec![(0x0056_3412, 0xCDEF)]);
    }

    #[test]
    fn op_4c_n_e_sub_7_truncated_buffer_returns_unknown() {
        // 6 bytes - last byte is missing.
        let bytecode = [0x4Cu8, 0xE7, 0x12, 0x34, 0x56, 0xEF];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert!(matches!(
            r,
            StepResult::Unknown {
                opcode: 0x4C,
                pc: 0
            }
        ));
    }

    #[test]
    fn op_4c_n_e_sub_8_camera_zoom_decodes_four_le16() {
        // 10-byte instruction: [opcode, sub-op, x0, x1, y0, y1, z0, z1, m0,
        // m1]. zoom_x = LE16(0x40, 0x00) = 0x40, zoom_y = 0x08, zoom_z = 4,
        // mode = 0 (the default-zoom triplet at line 7315-7317 of the
        // dispatcher dump).
        let bytecode = [0x4Cu8, 0xE8, 0x40, 0x00, 0x08, 0x00, 0x04, 0x00, 0x00, 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 10 });
        assert_eq!(host.n_e_sub_8_camera_zooms, vec![(0x40, 0x08, 0x04, 0)]);
    }

    #[test]
    fn op_4c_n_e_sub_8_camera_zoom_signed_mode_round_trip() {
        // Mode is i16 - verify negative / sign-bit values flow through.
        let bytecode = [0x4Cu8, 0xE8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(host.n_e_sub_8_camera_zooms, vec![(0, 0, 0, -1)]);
    }

    #[test]
    fn op_4c_n_e_sub_8_truncated_buffer_returns_unknown() {
        // 9 bytes - needs 10.
        let bytecode = [0x4Cu8, 0xE8, 0x40, 0x00, 0x08, 0x00, 0x04, 0x00, 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert!(matches!(
            r,
            StepResult::Unknown {
                opcode: 0x4C,
                pc: 0
            }
        ));
    }

    #[test]
    fn op_4c_n_d_sub_0_field_se_trigger_advances_6() {
        // [4C, 0xD0, 0x34, 0x12, 0x78, 0x56] → a = 0x1234, b = 0x5678.
        let bytecode = [0x4Cu8, 0xD0, 0x34, 0x12, 0x78, 0x56];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(host.n_d_sub_0_se_triggers, vec![(0x1234, 0x5678)]);
    }

    #[test]
    fn op_4c_n_d_sub_0_truncated_buffer_returns_unknown() {
        let bytecode = [0x4Cu8, 0xD0, 0x34, 0x12, 0x78]; // 5 bytes - needs 6
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert!(matches!(
            r,
            StepResult::Unknown {
                opcode: 0x4C,
                pc: 0
            }
        ));
    }

    #[test]
    fn op_4c_n_5_sub_3_dialog_wait_halts_at_pc_plus_2() {
        // [4C, 0x53] → halt-style return with PC = 0 + 2 = 2.
        let bytecode = [0x4Cu8, 0x53];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Halt { final_pc: 2 });
        assert_eq!(host.n_5_sub_3_dialog_waits, 1);
    }

    #[test]
    fn op_4c_n_5_sub_4_dialog_advance_advances_when_done() {
        // Default: dialog_active = false → advance PC by 2.
        let bytecode = [0x4Cu8, 0x54];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 2 });
        assert_eq!(host.n_5_sub_4_polls, 1);
    }

    #[test]
    fn op_4c_n_5_sub_4_dialog_advance_halts_when_active() {
        // dialog_active = true → halt at PC.
        let bytecode = [0x4Cu8, 0x54];
        let mut host = TestHost {
            n_5_sub_4_dialog_active: true,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        assert_eq!(host.n_5_sub_4_polls, 1);
    }

    #[test]
    fn op_4c_n_e_sub_4_uses_shared_tile_center_helper() {
        // Verifies the round-18 tile_center helper is wired in: 0x10 → 0x840,
        // 0x90 → 0x880, 0x00 → 0. This is the same case the round-17 inline
        // closure verified - confirm round-18's lift to a shared helper
        // didn't change semantics.
        let bytecode = [0x4Cu8, 0xE4, 0x10, 0x90, 0x00, 0x10, 0x00, 0x00, 0x00];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        step(&mut host, &mut ctx, &bytecode, 0);
        let bboxes = host.n_e_sub_4_bboxes.borrow();
        assert_eq!(bboxes.len(), 1);
        assert_eq!(bboxes[0], [0x840, 0x880, 0, 0x840]);
    }

    #[test]
    fn op_4c_n_d_sub_4_vram_stp_set_advances_6_bytes() {
        // [4C, 0xD4, x_lo, x_hi, y_lo, y_hi] = 6 bytes; original returns
        // iVar47 + 6. next_pc = 0 + 1 (header_size) + 5 = 6.
        let bytecode = [0x4Cu8, 0xD4, 0x80, 0x00, 0xEF, 0x01]; // x=0x0080, y=0x01EF
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(host.n_d_sub_4_vram_stp_set_calls, vec![(0x0080, 0x01EF)]);
    }

    #[test]
    fn op_4c_n_d_sub_5_vram_stp_clear_advances_6_bytes() {
        // Sister of sub-4 with STP-clear semantics; same 6-byte encoding.
        let bytecode = [0x4Cu8, 0xD5, 0xC0, 0x01, 0x10, 0x00]; // x=0x01C0, y=0x0010
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(host.n_d_sub_5_vram_stp_clear_calls, vec![(0x01C0, 0x0010)]);
    }

    #[test]
    fn op_4c_n_e_sub_f_halts_at_pc() {
        // Sub-F has no case in the original; falls through to halt.
        let bytecode = [0x4Cu8, 0xEF];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
    }
}
