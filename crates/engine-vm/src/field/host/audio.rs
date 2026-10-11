//! The [`FieldAudioHost`] supertrait of the field-VM host surface.

/// Music and sound-effect cues.
///
/// One of the supertraits of [`FieldHost`](super::FieldHost), the surface the
/// field VM calls into the game through.
pub trait FieldAudioHost {
    /// Background music dispatch (op 0x35). `text_id` is the 16-bit operand
    /// (LE). `sub_op` selects the action (arm table `0x801CEE00` in PROT
    /// 0897, indexed `sub_op - 1`): 1 = start field BGM, 2 = stop and rewind
    /// (raise pause bit 1, `FUN_800266E0`: notes killed, cursor back to the
    /// sequence start), 3 = key-off pause (raise bit 1, `FUN_80026740`:
    /// cursor kept), 4 = re-attach (clear bit 1, `FUN_80026478`: replays
    /// from the top), 5 = volume, 6 = flag-set (`_DAT_8007b750 |= 4`),
    /// 7 = target sound set (`_DAT_8007B880`), 8 = `func_0x80019898`,
    /// 9 = start behind a load barrier (`_DAT_8007bac8 = text_id`), 10 =
    /// unhalt-pause swap-commit (waits on `_DAT_8007B750` bit 3, releases
    /// the paused slot, sets ack bit 4, clears pause bit 1), 11 =
    /// `_DAT_8007ba9c = -1`. The VM only knows how to advance the PC;
    /// per-sub-op state is host-side.
    fn bgm(&mut self, text_id: u16, sub_op: u8) {
        let _ = (text_id, sub_op);
    }

    /// Op 0x4C outer-nibble-C sub-7 - run the item-effect applier.
    ///
    /// 4-byte instruction `[4C, 0xC7, b1, b2]`. Calls
    /// `func_0x800402F4(b1 + 0x0B, b2, 0, 0)` - the item / restore applier
    /// (`docs/subsystems/inventory.md`), which dispatches on an effect
    /// class (`b1 + 0x0B`) and tier (`b2`). Not a sound cue. PC += 4.
    fn apply_item_effect(&mut self, b1: u8, b2: u8) {
        let _ = (b1, b2);
    }

    /// Gated two-word call (op 0x4C nD sub-0, 6 bytes). The callee
    /// `func_0x8002B994` is the menu selection-cursor emitter
    /// (`docs/reference/functions/menus.md`), not a sound trigger; what the
    /// three gate words are is not established.
    ///
    /// `[4C, 0xD0, a_lo, a_hi, b_lo, b_hi]`. The original at lines 6936-6944
    /// reads the two 16-bit LE values; if any of three flag globals
    /// (`_DAT_8007B874`, `_DAT_800846D0`, `_DAT_800846D4`) is non-zero,
    /// calls `func_0x8002B994(a, b)` (the SE trigger). Otherwise no-op.
    ///
    /// PC advances by 6 in both branches. The hook returns nothing - the
    /// host decides whether the SE fires based on its own gate state.
    fn op4c_nd_sub0_gated_call(&mut self, a: u16, b: u16) {
        let _ = (a, b);
    }
}
