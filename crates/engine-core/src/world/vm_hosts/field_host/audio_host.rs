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

    fn bgm_ready(&mut self, sub_op: u8) -> bool {
        Self::bgm_ready(self, sub_op)
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

    /// Issue #597: a second grant of an art the record already lists (a
    /// second copy of a book, or a book after the art was learned) leaves
    /// the list as it was - retail's insert has no membership check and
    /// listed the art twice in the battle Arts list.
    #[test]
    fn a_repeated_grant_does_not_list_the_art_twice() {
        let mut w = World::new();
        w.load_party(legaia_save::Party::zeroed(3));
        let first = w.teach_hyper_art(0x0B, 3);
        let again = w.teach_hyper_art(0x0B, 3);
        assert!(matches!(
            again,
            legaia_engine_menus::items::ItemOutcome::ArtLearned {
                character: 0,
                art_id: 3,
                ..
            }
        ));
        assert_eq!(first, again, "the repeat reports the art where it sits");
        let list = w.party.roster.members[0].displayed_skills();
        assert_eq!(&list.ids[..usize::from(list.count)], &[3]);
    }

    /// A save whose record already lists an art twice loads with the repeat
    /// dropped and the rest in order.
    #[test]
    fn a_loaded_record_with_a_repeated_art_is_healed() {
        let mut party = legaia_save::Party::zeroed(3);
        let mut list = party.members[0].displayed_skills();
        list.count = 4;
        list.ids[..4].copy_from_slice(&[0x03, 0x03, 0x0A, 0x0D]);
        party.members[0].set_displayed_skills(list);
        let mut w = World::new();
        w.load_party(party);
        let list = w.party.roster.members[0].displayed_skills();
        assert_eq!(list.count, 3);
        assert_eq!(&list.ids[..4], &[0x03, 0x0A, 0x0D, 0x00]);
    }

    fn op35(w: &mut World, id: u16, sub_op: u8) -> legaia_engine_vm::field::StepResult {
        let [lo, hi] = id.to_le_bytes();
        let mut ctx = FieldCtx::default();
        let mut host = FieldHostImpl { world: w };
        field_step_routed(&mut host, &mut ctx, &[0x35, lo, hi, sub_op], 0)
    }

    /// A mid-scene sub-op 1 runs the swap poller's walk: the track attaches
    /// the read (longer than the settle) plus stages 4..6 (two frames at the
    /// field step) later, and the same word again is no change.
    #[test]
    fn a_mid_scene_start_installs_after_the_poller_walk() {
        let mut w = World::new();
        w.clock.frame_step = 2;
        assert!(matches!(
            op35(&mut w, 2001, 1),
            legaia_engine_vm::field::StepResult::Advance { .. }
        ));
        assert!(w.audio.bgm_swap_in_flight());
        let due = (1..=100u16)
            .find(|_| {
                w.tick_bgm_swap(1);
                w.take_bgm_install_due()
            })
            .unwrap();
        // Retail's measured walk: the attach 66 vsyncs after the store.
        assert_eq!(due, 66);
        op35(&mut w, 2001, 1);
        assert!(!w.audio.bgm_swap_in_flight(), "the same word is no change");
    }

    /// The commit re-runs its PC until bit 3 (`0x801E0288`), and a second
    /// select waits out the walk in flight (`0x801E0238`).
    #[test]
    fn the_commit_waits_for_the_load_to_settle() {
        use legaia_engine_vm::field::StepResult;
        let mut w = World::new();
        w.clock.frame_step = 2;
        assert!(matches!(op35(&mut w, 2033, 9), StepResult::Advance { .. }));
        assert!(matches!(op35(&mut w, 2040, 9), StepResult::Halt { .. }));
        let mut passes = 0;
        while matches!(op35(&mut w, 0, 0xA), StepResult::Halt { .. }) {
            w.tick_bgm_swap(1);
            passes += 1;
            assert!(
                !w.take_bgm_install_due(),
                "nothing installs before the commit"
            );
        }
        assert_eq!(passes, World::BGM_SWAP_READ_VSYNCS);
        let due = (1..=10u16)
            .find(|_| {
                w.tick_bgm_swap(1);
                w.take_bgm_install_due()
            })
            .unwrap();
        assert_eq!(due, 5);
    }

    /// The park word ends the walk: requested and loaded agree on the next
    /// call, so a select right after it does not wait (`0x801E0238`).
    #[test]
    fn the_park_word_ends_a_walk_in_flight() {
        use legaia_engine_vm::field::StepResult;
        let mut w = World::new();
        op35(&mut w, 2001, 1);
        assert!(w.audio.bgm_swap_in_flight());
        op35(&mut w, crate::world::BGM_PARK_SENTINEL, 1);
        assert!(!w.audio.bgm_swap_in_flight());
        assert!(matches!(op35(&mut w, 2033, 9), StepResult::Advance { .. }));
    }

    /// The field init's install slice loads with the init: no walk.
    #[test]
    fn an_entry_start_does_not_run_the_walk() {
        let mut w = World::new();
        w.release_field_init_voices();
        op35(&mut w, 2001, 1);
        assert!(!w.audio.bgm_swap_in_flight());
    }
}
