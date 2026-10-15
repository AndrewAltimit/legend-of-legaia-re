//! `FieldAudioHost` for [`FieldHostImpl`].

use super::*;

impl<'a> FieldAudioHost for FieldHostImpl<'a> {
    // Op `4C C7 <char> <art>` (arm `0x801E28D4`): `FUN_800402F4(char +
    // 0x0B, art, 0, 0)`. Every shipped carrier passes `char` 0..2 - a master
    // teaching a Hyper Art (`town0c`'s Tetsu, behind a `4C 8D` "already
    // learned?" test) - so the applier's arts-book arm is the whole of it.
    // The "learned" window is the record's following `49 09` beat.
    fn apply_item_effect(&mut self, b1: u8, b2: u8) {
        self.world.teach_hyper_art(b1.wrapping_add(0x0B), b2);
    }

    fn bgm(&mut self, text_id: u16, sub_op: u8) {
        Self::bgm(self, text_id, sub_op)
    }
}

#[cfg(test)]
mod tests {
    use crate::world::World;
    use crate::world::vm_hosts::{FieldHostImpl, field_step_routed};
    use legaia_engine_vm::field::FieldCtx;

    /// `town0c`'s Tetsu teaches Noa art `0x0F` with `4C C7 01 0F`: the
    /// applier's arts-book arm inserts it into roster slot 1's displayed
    /// skills, which his own `4C 8D 01 0F` "already learned?" test reads.
    #[test]
    fn op_4c_c7_teaches_the_named_member_a_hyper_art() {
        let mut w = World::new();
        w.load_party(legaia_save::Party::zeroed(3));
        let before = w.party.roster.members[1].displayed_skills();
        assert!(!before.ids[..usize::from(before.count)].contains(&0x0F));
        let mut ctx = FieldCtx::default();
        let mut host = FieldHostImpl { world: &mut w };
        field_step_routed(&mut host, &mut ctx, &[0x4C, 0xC7, 0x01, 0x0F], 0);
        let after = w.party.roster.members[1].displayed_skills();
        assert_eq!(after.count, before.count + 1);
        assert!(after.ids[..usize::from(after.count)].contains(&0x0F));
    }
}
