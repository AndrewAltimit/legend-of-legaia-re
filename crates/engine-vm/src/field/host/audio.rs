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
    /// Every shipped carrier passes `b1` 0..2: the arts-book arm, which
    /// inserts art `b2` into roster slot `b1`'s displayed-skill list.
    fn apply_item_effect(&mut self, b1: u8, b2: u8) {
        let _ = (b1, b2);
    }
}
