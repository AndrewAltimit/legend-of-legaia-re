//! Field / event script VM, ported clean-room from `FUN_801DE840`.
//!
//! PORT: FUN_801DE840, FUN_8003CE08, FUN_8003CE34, FUN_8003CE64, FUN_8003C83C, FUN_8003CF04
//! PORT: FUN_801DAA50, FUN_801DAB90, FUN_801DBC20, FUN_801DE004, FUN_801DC0BC, FUN_801DDF48
//! PORT: FUN_801DE190, FUN_8003C5F0, FUN_801D77F4, FUN_801D8280, FUN_801E57F0, FUN_801E3614
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
//!
//! PORT: FUN_801D5630, FUN_801D596C, FUN_801D65D8, FUN_801D835C, FUN_801DB8EC
//! PORT: FUN_801DD9D4, FUN_801DDE34, FUN_801DDFE4, FUN_801DE084, FUN_801DE2B0
//! PORT: FUN_801DE3E0, FUN_801DE698, FUN_801DE754, FUN_801DE7BC, FUN_801E4C58
//! PORT: FUN_801E573C, FUN_801E5668, FUN_801F8004, FUN_801F88FC, FUN_801F8D4C
//! PORT: FUN_801F8E6C, FUN_801F8F28
//!
//! REF: FUN_8003AEB0, FUN_8003C764, FUN_8003CA38, FUN_8003CE9C, FUN_8003CF04
//! REF: FUN_80042EE0, FUN_80056798, FUN_80058104, FUN_800583C8, FUN_8005842C, FUN_801D2D38
//! REF: FUN_801E3620
//! REF: FUN_8001EBEC, FUN_80039B7C, FUN_801D84D0

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

    /// Open a dialog box. The text ID + inline buffer feed the MES bytecode that
    /// `crates/mes` parses. **Not** wired to a field-VM opcode: it is the host's
    /// dialogue-open primitive, invoked from [`Self::field_interact`] with the
    /// interacted actor's inline interaction-script text (the real field-dialogue
    /// source — retail `actor[+0x90]`). Field dialogue has no dedicated opcode;
    /// `0x3F` is the named scene-change, not a dialog op (see
    /// `docs/subsystems/script-vm.md` § Field dialogue). `world_x` / `world_z`
    /// are pre-decoded grid coordinates for the box position; `depth_id` is a raw
    /// depth selector. (`func_0x8001FD44` is the scene-change packet, not the
    /// original opener — an earlier mislabel.)
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

    /// Give the player one of inline item `item_id` (op 0x39 `GIVE_ITEM` — the
    /// treasure-chest / scripted-gift item-give). The original calls
    /// `func_0x8004313C()` (HUD/inventory window-bounds setup — writes the
    /// `gp+0x2D2/0x2D4/0x2D6` start/end/span triple; see
    /// `docs/reference/functions.md` `8004313C`) then the capacity-checked
    /// add-item-by-id primitive `func_0x800421D4(item_id, 1)`. (The earlier
    /// `play_sfx` name was wrong: SFX cues go through `FUN_80035B50`, not 0x39.)
    fn give_item(&mut self, item_id: u8) {
        let _ = item_id;
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
    /// `_DAT_8007B8F4` is the per-tile **region-type mask**: bit `n` set
    /// when the player tile sits inside a type-`n` region of the scene
    /// `.MAP` region table. Rebuilt by `FUN_800180EC` / `FUN_801DBA20`
    /// (ports: `legaia_engine_core::field_regions`).
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

    /// Trigger a *named* scene transition (op `0x3F`, the named scene-change /
    /// "warp by name"). Unlike [`Self::scene_transition`] (the `0x3E` door-warp,
    /// which carries only a 7-id scene-*type* `map_id`), this op carries the
    /// **destination scene name inline** in the bytecode, so the host loads
    /// `scene` directly with no map-id resolver. `entry_x` / `entry_z` are the
    /// destination entry-tile bytes the op also carries (`& 0x7F` tile,
    /// `& 0x80` half-tile). Default is a no-op so a bare host still advances past
    /// the op. The VM only passes a name that cleared the clean-CDNAME-label gate
    /// ([`crate::field_disasm::clean_scene_name`]); a desync-phantom `0x3F` inside
    /// message text is skipped (no transition) but still advances the PC.
    fn scene_transition_named(&mut self, scene: &str, entry_x: u8, entry_z: u8) {
        let _ = (scene, entry_x, entry_z);
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

    /// Op 0x49 **menu-request** edge (Idle->arm), handed the instruction bytes
    /// from the opcode onward (`[0x49][sub_op][len][...][payload]`). The op
    /// `0x49` (`STATE_RESUME`) is how a field script opens a menu overlay by
    /// driving the request register `_DAT_8007B450`; for `sub_op == 0` the
    /// payload can be an inline **shop** record (`[count][item_ids][name]`).
    /// Hosts inspect `instr` to recognise + open the right menu (a gold shop,
    /// say) before the op suspends. Default no-op (the op still arms + halts).
    /// Called once on the Idle->arm edge, before [`Self::op49_invoke_setup`].
    fn op49_menu_request(&mut self, sub_op: u8, instr: &[u8]) {
        let _ = (sub_op, instr);
    }

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
    /// adjacent to the fourth-flag-bank bitfield (`DAT_80085758`). Hosts model
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

    /// Op 0x43 sub-0x10 (sprite-widget spawn, FUN_801F8004).
    ///
    /// 21-byte instruction. The original calls `FUN_801F8004(operand + 1)` -
    /// the PROT-0900 sprite-widget spawner with its inline 19-byte record
    /// (`engine-core::screen_fx::SpriteRecord`). PC += 21.
    fn op43_widget_sprite_spawn(&mut self, payload: &[u8]) {
        let _ = payload;
    }

    /// Op 0x43 sub-0x11 (screen-mask rect tween, FUN_801F8D4C).
    ///
    /// 12-byte instruction; reads 5 u16s and calls
    /// `FUN_801F8D4C(l, t, r, b, dur)` - the PROT-0900 mask (iris) widget
    /// control API (`engine-core::screen_fx::MaskWidget`). PC += 12.
    fn op43_widget_mask_rect(&mut self, words: [u16; 5]) {
        let _ = words;
    }

    /// Op 0x43 sub-0x15 (letterbox config, FUN_801F8F28).
    ///
    /// 14-byte instruction. The original calls `FUN_801F8F28(operand + 1)` -
    /// the PROT-0900 letterbox widget's six-i16 config
    /// (`engine-core::screen_fx::Letterbox`). PC += 14.
    fn op43_widget_letterbox(&mut self, payload: &[u8]) {
        let _ = payload;
    }

    /// Op 0x43 sub-0x12 (VRAM rect copy with 0x100 clamp + offset shift).
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
    /// `func_0x800468a4(6, …)` enqueues a GP0 `0x80` VRAM->VRAM rectangle
    /// copy into OT slot 6 (packet builder `FUN_80057914`; `src_y += 0xF0`
    /// under the back-buffer flag) - the >256-wide dual call is the same
    /// two-page split the panel widget does. The VM passes the six raw words
    /// plus a `did_split` boolean (set when the `c > 0xFF` branch fired).
    /// No on-disc scene script uses this sub-op. PC += 14.
    fn op43_vram_rect_copy(&mut self, words: [i16; 6], did_split: bool) {
        let _ = (words, did_split);
    }

    /// Op 0x43 sub-0x13 (image-panel spawn, FUN_801F88FC).
    ///
    /// 14-byte instruction `[43, 0x13, ...12 bytes]`. The original calls
    /// `FUN_801F88FC(operand)` - the PROT-0900 image-panel widget spawner,
    /// reading its `[x][y][w][h][tex_x][tex_y]` record past the sub-op byte
    /// (`engine-core::screen_fx::PanelWidget`). PC += 14.
    fn op43_widget_panel_spawn(&mut self, payload: &[u8; 13]) {
        let _ = payload;
    }

    /// Op 0x43 sub-0x14 (panel move/scale, FUN_801F8E6C).
    ///
    /// 10-byte instruction `[43, 0x14, lo0, hi0, lo1, hi1, lo2, hi2, lo3, hi3]`.
    /// The original calls `FUN_801F8E6C(x, y, scale, dur)` - the PROT-0900
    /// panel widget's move/scale API (`scale` is 4.12 fixed). PC += 10.
    fn op43_widget_panel_move(&mut self, words: [i16; 4]) {
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

    /// Op 0x4C sub-3 sub-F (per-character TMD-pose copy).
    ///
    /// 2-byte instruction `[4C, 0x3F]`. Calls `FUN_8001ebec()`, which is a
    /// 3-iteration per-party-slot copier (slots 0..2, character-record stride
    /// `0x414`): for each slot, indexes the global TMD pool `DAT_8007C018`
    /// through the slot-byte at `_DAT_8007B824 + i` (the slot-4 freeze flag
    /// area), then writes 7 u32s of pose data (28 bytes) at TMD offset
    /// `+0xC + bytes[i]*0x1C` from either `+0x124..0x140` or `+0x140..0x158`,
    /// gated on a per-record flag byte at `record+0x75E`. (Earlier comments
    /// labelled `FUN_8001ebec` the "retail dialog-box renderer" — that's
    /// wrong; the disassembly shows the TMD-pose copier described above.
    /// The real dialog SM is `FUN_80039b7c`, pager `FUN_801D84D0`.) PC += 2.
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

    /// Is the active field entity an encounter carrier?
    ///
    /// This is the engine-side stand-in for retail's scripted-encounter
    /// **discriminator**: there is no dedicated "encounter" opcode - the
    /// arm-encounter opcodes (`0x37`/`0x41`, `0x38`, `0x43`, `0x47`, `0x4C`)
    /// are the field VM's generic halt-acquire family, and what turns a halt
    /// into an encounter arm is the *consumer*. Retail only reads `+0x94` as a
    /// formation record on entities ticked by the 5-state `FUN_801DA51C` SM,
    /// once that SM reaches the encounter-confirm state. The field VM cannot
    /// see that per-entity SM, so it asks the host whether the active entity
    /// is armed; only then does the bare arm-encounter op forward the record.
    ///
    /// The default impl returns `false`, so generic script yields never get
    /// mistaken for encounter arms.
    ///
    /// REF: FUN_801DA51C
    fn is_scripted_encounter_armed(&self) -> bool {
        false
    }

    /// Install a scripted encounter from the bytecode window at the bare
    /// arm-encounter op (`0x37`/`0x41`).
    ///
    /// The retail writer (`0x801DEEDC..0x801DEEEC`) sets `actor[+0x94] = s0`,
    /// where `s0 = bytecode_buffer + pc_offset` is the **current opcode
    /// pointer** - i.e. the encounter record overlays the install opcode
    /// itself: `record[+0] = opcode byte`, `[+1..+2] = its operands`,
    /// `[+3] = monster_count`, `[+4..] = ids`. The consumer (`FUN_801DA51C`)
    /// reads that pointer to fill the formation cell exactly once per arm.
    ///
    /// The VM hands the host the bounded window starting at the current PC
    /// (`[opcode][op1][op2][count][<=4 ids]`, at most 8 bytes). Called only
    /// when [`Self::is_scripted_encounter_armed`] returned `true`; the host is
    /// expected to install the formation and disarm (fire-once semantics). The
    /// default impl ignores the window.
    fn install_scripted_encounter(&mut self, window: &[u8]) {
        let _ = window;
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
    /// "no actor pool" branch (skips the spawn), but it must still consume the
    /// whole instruction: the base is 13 bytes, and a `0x40` capture marker
    /// appends a 1-byte payload-length field + the captured-PC payload, so the
    /// delta grows by `2 + payload_len` (matching the disassembler's width and
    /// step.rs's own slicing of `captured_pc_payload`). Returning a constant 13
    /// when the capture extension is present under-advances the PC into the
    /// middle of the payload and desyncs the rest of the script.
    fn op34_sub1_spawn_or_skip(
        &mut self,
        ctx: &FieldCtx,
        op0: u8,
        packed24: u32,
        pos: [i16; 3],
        capture_flag: u8,
        captured_pc_payload: &[u8],
    ) -> usize {
        let _ = (ctx, op0, packed24, pos);
        13 + if capture_flag == 0x40 {
            2 + captured_pc_payload.len()
        } else {
            0
        }
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
    /// which sets `(&DAT_80085758)[idx >> 3] |= (0x80 >> (idx & 7))`.
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

    /// Op 0x4C outer-nibble-5 sub-0 - set actor model (pool-select).
    ///
    /// 4-byte instruction `[4C, 0x50, lo, hi]`. Reads a signed-16-bit value
    /// from `operand+1..3` and resolves it to a model index against one of two
    /// model-pool bases: the low half (`< 0xF0`) indexes from `_DAT_8007B6F8`,
    /// the high half (`>= 0xF0`) from `_DAT_8007B824 + 0xFF10`. The resolved
    /// index is installed via `func_0x80024E08(actor, model_idx)` - the
    /// set-model primitive (writes `actor+0x64`, clears draw-flag bit `0x1000`,
    /// re-stages via `FUN_80020F88`; see `docs/reference/functions.md`). The
    /// high half also sets bit `0x01000000` in `ctx.flags` (recording which
    /// pool base was used); the low half clears it.
    ///
    /// The VM applies the flag-bit toggle itself (`ctx.flags |=` / `&= !`)
    /// and hands the host the raw value + the high/low selection. PC += 4.
    /// (Earlier mislabelled as a "directional sound emitter" - `FUN_80024E08`
    /// is a model-set, not an audio call.)
    fn op4c_n5_sub0_set_actor_model(&mut self, ctx: &mut FieldCtx, value: i16, high: bool) {
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

    /// Op 0x4C outer-nibble-8 sub-3 - rectangular tile fill.
    ///
    /// 7-byte instruction `[4C, 0x83, col_start, row_start, col_end,
    /// row_end, value]`. The original at lines 6447-6493 of the dispatcher
    /// dump (`overlay_0897_801de840.txt`) runs two nested loops over the
    /// inclusive rectangle `[col_start..=col_end] × [row_start..=row_end]`,
    /// calling `FUN_801D5630(col, row, ...)` for each tile. On non-null
    /// return the inner body writes `tile[+0x3] = 0; tile[+0x2] = value`.
    /// The post-loop trailer also writes `_DAT_8007B630 = col_start` and
    /// exits via the dispatcher's `j 0x801e3624` label.
    ///
    /// `FUN_801D5630` itself (`ghidra/scripts/funcs/overlay_0897_801d5630.txt`)
    /// is the tile-resolver helper: 9-instruction body that on hit returns
    /// a tile-record pointer and on miss sets `ctx.flags |= 0x8` and
    /// re-enters the dispatcher wait loop. The clean-room port exposes the
    /// rectangle via the host hook and lets the engine implement its tile
    /// pool however it wants.
    ///
    /// Default impl is a no-op: the tile pool is engine-owned, and there
    /// is no shared in-VM scratch the rect-fill writes to.
    fn op4c_n_8_sub_3_rect_tile_fill(
        &mut self,
        col_start: u8,
        row_start: u8,
        col_end: u8,
        row_end: u8,
        value: u8,
    ) {
        let _ = (col_start, row_start, col_end, row_end, value);
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

    /// Op 0x4C outer-nibble-D sub-8 - synchronous actor allocator,
    /// `FUN_801D77F4` (overlay-resident) 4-arg call.
    ///
    /// 9-byte instruction
    /// `[4C, 0xD8, vdf_idx, tmd_lo, tmd_hi, kind_lo, kind_hi, variant_lo, variant_hi]`.
    /// PC += 9.
    ///
    /// The retail dispatcher at `0x4C 0xD8` (case 8 of the outer-D dispatch,
    /// see `overlay_world_map_801de840.txt` line 7682-7687) reads:
    ///   - `vdf_idx`  (`b1` here): u8 packet-record index into the VDF
    ///     buffer at `_DAT_8007B7DC + 4 + vdf_idx*4`; the record contains
    ///     the per-actor bytecode that FUN_801D77F4 stores at `actor[+0x4C]`.
    ///   - `tmd_idx`  (`words[0]` here): i16 delta added to the global
    ///     `_DAT_8007B6F8` then truncated to i16; the sum indexes the global
    ///     TMD table at `DAT_8007C018 + tmd_idx*4` and is written to
    ///     `actor[+0x48]` (the spawned actor's TMD pointer).
    ///   - `kind`     (`words[1]` here): u16 stored at `actor[+0x3C]`.
    ///   - `variant`  (`words[2]` here): u16 stored at `actor[+0x3E]`.
    ///
    /// Unlike the halt-acquire-gated `0x4C 0x80` allocator path (which
    /// queues records and lets the engine materialize them on its own
    /// schedule via [`crate::field::FieldHost`]-side wiring), `0x4C 0xD8`
    /// spawns synchronously: FUN_801D77F4 returns with the slot already
    /// populated, and the parent script advances by 9 unconditionally.
    /// Hosts emit `FieldEvent::ActorSpawned` directly from this hook.
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

    // ----------------------------------------------------------------------
    // Round 19: 0x4C n5 / n6 / n8 sub-cases entangled with the halt-acquire
    // state machine. The standard halt-acquire predicate/apply pair gates
    // n6 sub-0x61 and n8 sub-0; n5 sub-1/sub-2 sit alongside but don't
    // route through the predicate.
    // ----------------------------------------------------------------------

    /// Op 0x4C n5 sub-1 - NPC / player move-to-tile with run dispatch.
    ///
    /// 6-byte instruction `[4C, 0x51, x_enc, z_enc, depth_or_flags, move_id]`.
    /// The VM has already decoded `world_x` / `world_z` via the standard tile
    /// formula (`(b & 0x7F) * 0x80 + 0x40 + 0x40 if (b & 0x80)`). `depth_byte`
    /// is the lower 4 bits of byte +3, indexing the 8-entry depth table at
    /// `_DAT_80073F04` in retail. `move_id` is the move-table id passed into
    /// the move-table consumer (`func_0x800204f8`); `99` is the cancel sentinel
    /// on the player path.
    ///
    /// `is_player` is derived from `ctx == _DAT_8007c364` in the original;
    /// the VM passes `true` when `ctx.flags & 0x0100_0000` is set (the
    /// player-vs-NPC bit). Default no-op - hosts that own the move-table
    /// consumer override.
    fn op4c_n5_sub1_npc_run(
        &mut self,
        ctx: &mut FieldCtx,
        world_x: u16,
        world_z: u16,
        depth_byte: u8,
        move_id: u8,
        is_player: bool,
    ) {
        let _ = (ctx, world_x, world_z, depth_byte, move_id, is_player);
    }

    /// Op 0x4C n5 sub-2 - menu / sub-screen activation poll.
    ///
    /// 3-byte instruction `[4C, 0x52, menu_id]`. The original at dump lines
    /// 6286-6294 calls `func_0x8004313c()` then
    /// `sVar9 = func_0x80042310(menu_id, 1)`. If `sVar9 == 0x100` (menu
    /// activation finished), the dispatcher calls `func_0x800430ac(menu_id)`
    /// and exits through `switchD_801e00f4::default()` - advance PC by 3.
    /// Otherwise the dispatcher routes through `LAB_801e00bc` which halts
    /// at PC - the script polls each tick until the menu transition lands.
    ///
    /// Returns `true` when the menu activation finalised (advance PC),
    /// `false` while still in transit (halt at PC). Default returns `false`
    /// (the menu never activates) so engines without a menu compositor
    /// halt indefinitely - they MUST override if they want this opcode to
    /// progress.
    fn op4c_n5_sub2_menu_activation(&mut self, menu_id: u8) -> bool {
        let _ = menu_id;
        false
    }

    /// Op 0x4C n6 sub-0x61 - 16-byte halt-acquire emitter (FUN_801E4C58 caller).
    ///
    /// `payload` is the 14-byte operand slice (bytes +1..+15 of the
    /// instruction, including the gating word at +0xD..=+0xE). The VM has
    /// already routed this through the standard halt-acquire predicate
    /// ([`Self::field_halt_acquire_predicate`] with `which = 0x61`) and
    /// performed the ctx mutation ((`saved_pc`, `wait_accum=0`,
    /// `flags |= 0x400`) plus optional system-channel mirror via
    /// [`Self::field_halt_acquire_apply`]).
    ///
    /// Default no-op. Hosts that model the emitter graph override.
    fn op4c_n6_sub_61_emitter(&mut self, ctx: &mut FieldCtx, payload: [u8; 14]) {
        let _ = (ctx, payload);
    }

    /// Op 0x4C n8 sub-0 - actor allocator with halt-acquire prelude.
    ///
    /// 3-byte header `[4C, 0x80, count]` followed by `count` variable-length
    /// child-actor records. The VM has routed this through the standard
    /// halt-acquire predicate ([`Self::field_halt_acquire_predicate`] with
    /// `which = 0x80`) and performed the ctx mutation. `count` is the byte
    /// at operand+1 (immediately after the `0x80` sub-byte); `tail` is the
    /// raw bytecode slice starting at operand+2 (the first record byte) up
    /// to the bytecode end. Hosts walk the records themselves - the
    /// original uses `func_0x8003ca38` to advance through variable-length
    /// entries.
    ///
    /// The VM advances PC by 3 regardless of how many records the host
    /// consumes - the records are owned by the spawned actor at offset
    /// `+0x90`, not by the parent script's PC.
    ///
    /// Default no-op. Hosts that model actor allocation override.
    fn op4c_n8_sub_0_actor_allocator(&mut self, ctx: &mut FieldCtx, count: u8, tail: &[u8]) {
        let _ = (ctx, count, tail);
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

/// Relative-jump target, faithful to retail's 16-bit `short` PC.
///
/// The original stores each script's PC as a signed 16-bit value at
/// `ctx[+0x9e]`, so every relative branch target wraps mod `0x10000`. A delta
/// with the high bit set is therefore a *backward* jump (e.g. `0xFFFE` = -2),
/// not a `+65534` forward one. Computing `base + delta` in `usize` (no wrap)
/// sends every backward jump off the end of the buffer (the classic
/// "PC runs away to 0x10102" symptom). `base` is the post-operand address the
/// delta is measured from.
fn rel_jump(base: usize, lo: u8, hi: u8) -> usize {
    let delta = u16::from_le_bytes([lo, hi]);
    usize::from((base as u16).wrapping_add(delta))
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

pub use step::{step, step_with_caller};

mod step;

#[cfg(test)]
mod tests;
