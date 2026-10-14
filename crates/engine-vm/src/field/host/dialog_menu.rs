//! The [`FieldDialogMenuHost`] supertrait of the field-VM host surface.

use crate::field::{FieldCtx, Op49State};

/// Dialogue boxes, text balloons, talk staging and the op `0x49` menu-request register.
///
/// One of the supertraits of [`FieldHost`](super::FieldHost), the surface the
/// field VM calls into the game through.
pub trait FieldDialogMenuHost {
    /// Open a dialog box. The text ID + inline buffer feed the MES bytecode that
    /// `crates/mes` parses. **Not** wired to a field-VM opcode: it is the host's
    /// dialogue-open primitive, invoked by the host's interaction path with the
    /// interacted actor's inline interaction-script text (the real field-dialogue
    /// source - retail `actor[+0x90]`). Field dialogue has no dedicated opcode;
    /// `0x3F` is the named scene-change, not a dialog op (see
    /// `docs/subsystems/script-vm.md` § Field dialogue). `world_x` / `world_z`
    /// are pre-decoded grid coordinates for the box position; `depth_id` is a raw
    /// depth selector. (`func_0x8001FD44` is the scene-change packet, not the
    /// original opener - an earlier mislabel.)
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

    /// Arm the op-49 state machine. `sub_op` is the operand's first byte,
    /// `pc` the bytecode offset of the opcode byte, and `field_90` the
    /// opaque actor-handle captured for sub-op 1 (`_DAT_8007B44C` in the
    /// original). Hosts hold this until [`Self::op49_state`] returns
    /// `Done`.
    ///
    /// `sub_op` is not decoration: retail arms the park with the **operand
    /// pointer**, and every downstream consumer dereferences that pointer's
    /// first byte, so the sub-op *is* the park's kind byte.
    ///
    /// ```text
    /// 801e0984  lbu   v0,0x0(s6)        ; sub_op = *operand
    /// 801e098c  sltiu v0,v0,0xe         ; sub_op < 0xE or no arm at all
    /// 801e09a0  jal   0x80020de0        ; spawn the driver
    /// 801e09a8  sw    s6,-0x4bb0(s0)    ; _DAT_8007B450 = operand
    /// ```
    ///
    /// The menu overlay's outer dispatcher then routes on exactly that byte
    /// (`FUN_801DC6B4` at `0x801dc88c..0x801dc8e4`): `0` opens sub-screen
    /// `0x1A`, `1` opens `0x19`, `7` opens `0x20` and `0x0D` opens `4`. A
    /// host that drops the sub-op here can never answer "which flow opened
    /// this menu", which is why the trait carries it rather than leaving
    /// hosts to re-read the bytecode.
    ///
    /// REF: FUN_801DE840 (op `0x49` Idle arm), FUN_801DC6B4 (the consumer)
    fn op49_arm(&mut self, sub_op: u8, pc: usize, field_90: u32) {
        let _ = (sub_op, pc, field_90);
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

    /// Op 0x43 sub-2 (3-actor talk setup).
    ///
    /// 8-byte instruction `[43, 2, a1, a2, a3, lo, hi, b6]`. The original
    /// resolves three actor IDs via `func_0x8003C83C`; if all three resolve,
    /// it reads `u16 = LE(operand[5..7])` and byte `operand[7]`, then calls
    /// `FUN_801D2D38(actor1, actor2, actor3, u16, byte)` - a 3-actor talk /
    /// face setup. If any ID fails to resolve, the call is skipped silently.
    ///
    /// PC += 8 in both branches.
    fn stage_three_actor_talk(&mut self, actor_ids: [u8; 3], arg_word: u16, arg_byte: u8) {
        let _ = (actor_ids, arg_word, arg_byte);
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
    fn set_dialog_auto_press(&mut self, value: i16) {
        let _ = value;
    }

    /// Op 0x4C outer-nibble-E sub-0 - three-way scene/menu state write.
    ///
    /// 3-byte instruction `[4C, 0xE0, b1]`. The original at line 7173:
    /// - `b1 == 0`: `DAT_801F2744 = 1`
    /// - `b1 < 100`: `DAT_801F2740 = b1`
    /// - else: `*(u16*)(_DAT_801C6EA4 + 0xE) = b1 - 100`
    ///
    /// PC += 3: the decompile's `goto LAB_801e00bc` hides the advance - the
    /// raw asm (0x801E306C) exits every path through the `addiu s8,s8,0x3`
    /// entry at 0x801E00B8, one instruction above that label.
    fn op4c_ne_sub0_state_write(&mut self, b1: u8) {
        let _ = b1;
    }

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
    /// [`field_helpers::packet_length`]: legaia_scene_formats::field_disasm::packet_length
    fn spawn_text_balloon(&mut self, text_buf: &[u8], script_id: u16) {
        let _ = (text_buf, script_id);
    }

    /// Dialog wait poll (op 0x4C n5 sub-3, 2 bytes).
    ///
    /// `[4C, 0x53]`. The original calls `FUN_801D65D8(1)` and falls through
    /// to `joined_r0x801E28C4`. `FUN_801D65D8(target)` is not a dialog query:
    /// it is a staged two-way mode switch. It returns 0 at once when the mode
    /// word `0x8007BA3C` already equals `target`; otherwise it steps
    /// `0x8007B9C0` - raise `0x8007B648 = 0x80`, wait out the CD read poll
    /// `FUN_8003DE7C(1)`, then for `target = 1` save `0x8007B9CC` to
    /// `0x8007BA38` and set its `0x28000` bits (and `0x8007B72C =
    /// 0x8007B85C`), for `target = 0` restore both - and finally stores
    /// `target` to `0x8007BA3C`, returning 1 while a step is in flight. The dispatch ends in either case - this is a
    /// halt-style instruction - but PC has been incremented to `pc + 2`
    /// before the joined block runs.
    ///
    /// The hook is purely a side-effect callback (the host pumps its dialog
    /// state machine). PC advances by 2 and the VM halts (yields to the
    /// host); the next pump reads from `pc + 2`.
    fn op4c_n5_sub3_dialog_wait(&mut self, ctx: &mut FieldCtx) {
        let _ = ctx;
    }

    /// Dialog advance poll (op 0x4C n5 sub-4, 2 bytes).
    ///
    /// `[4C, 0x54]`. The original calls `FUN_801D65D8(0)` - the restore
    /// direction of the staged mode switch described on
    /// [`Self::op4c_n5_sub3_dialog_wait`], not a dialog query - and:
    /// - Returns non-zero (switch still stepping) → halt at the standard
    ///   switch-default (PC unchanged for this pump cycle).
    /// - Returns zero (switch done) → clear `DAT_8007B648`, copy 6 bytes
    ///   from the directional-sound state globals, and advance PC by 2.
    ///
    /// The hook returns `true` when the dialog is still active (VM halts at
    /// PC), `false` when done (VM advances PC by 2). Default impl returns
    /// `false` - engines without a dialog renderer immediately complete.
    fn op4c_n5_sub4_dialog_advance(&mut self, ctx: &mut FieldCtx) -> bool {
        let _ = ctx;
        false
    }

    /// Op `0x4C` outer-nibble-D sub-0 - the **press-a-button prompt**.
    ///
    /// `[4C, 0xD0, x_lo, x_hi, y_lo, y_hi]`, arm at `0x801E2AE4`. The arm
    /// advances `s8 += 6`, then tests the new-press pad word `_DAT_8007B874`
    /// against the confirm and cancel masks `_DAT_800846D0 | _DAT_800846D4`:
    /// a press returns the advanced PC. Otherwise it calls
    /// `FUN_8002B994(1, 1, x, y)` - the menu selection-cursor emitter, the
    /// blinking cursor at `(x, y)` - and restores the entry PC
    /// (`j 0x801DEE50` / `move s8,s4`), so the op re-runs next frame.
    /// `town0c`'s art lessons and `balden`'s use it to hold a line until the
    /// player answers.
    ///
    /// Returns `true` once confirm or cancel is newly pressed. On `false` the
    /// host draws the cursor for this frame and the VM holds. The default
    /// reports a press, so a host without input advances at once.
    fn press_prompt(&mut self, x: u16, y: u16) -> bool {
        let _ = (x, y);
        true
    }
}
