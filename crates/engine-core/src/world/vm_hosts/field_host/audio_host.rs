//! `FieldAudioHost` for [`FieldHostImpl`].

use super::*;

impl<'a> FieldAudioHost for FieldHostImpl<'a> {
    fn bgm(&mut self, text_id: u16, sub_op: u8) {
        Self::bgm(self, text_id, sub_op)
    }
}
