//! Op `4C DC` / `4C DE`, the field VM's "does the party hold this item"
//! questions (`FUN_801DE840` arms `0x801E2EA8` / `0x801E2F7C`): the bag's
//! active window, then what the present members wear. `koin2`'s Sage's
//! Treasure Chests ask for a Soru Bread (`0xB2`) through `4C DC B2 27 00`;
//! answered no, every chest stays on its "(Silence)" line and `kor`'s Sage's
//! Gate never opens.

use super::*;
use crate::world::vm_hosts::{FieldHostImpl, field_step_routed};
use legaia_engine_mode::mode::SceneMode;

const SORU_BREAD: u8 = 0xB2;

/// `koin2` P1[8]'s question at `+0xCE`, a hit landing on `+0xF8`.
fn ask(w: &mut World, sub: u8) -> FieldStepResult {
    let mut bc = vec![0x21u8; 0x100];
    bc[0xCE..0xD3].copy_from_slice(&[0x4C, sub, SORU_BREAD, 0x27, 0x00]);
    let mut ctx = FieldCtx::default();
    let mut host = FieldHostImpl { world: w };
    field_step_routed(&mut host, &mut ctx, &bc, 0xCE)
}

fn party_world() -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    let blank = legaia_save::CharacterRecord::parse(&[0u8; 0x414]).expect("blank record");
    w.party.roster.members = vec![blank; 3];
    w.install_present_party_list(vec![0, 1, 2]);
    w
}

#[test]
fn a_loaf_in_the_bag_answers_the_chest() {
    let mut w = party_world();
    assert!(matches!(
        ask(&mut w, 0xDC),
        FieldStepResult::Advance { next_pc: 0xD3 }
    ));
    w.party.inventory.add(SORU_BREAD, 1);
    assert!(matches!(
        ask(&mut w, 0xDC),
        FieldStepResult::Advance { next_pc: 0xF8 }
    ));
    // `4C DE` reads only what is worn: a loaf in the bag is not an answer.
    assert!(matches!(
        ask(&mut w, 0xDE),
        FieldStepResult::Advance { next_pc: 0xD3 }
    ));
}

#[test]
fn a_worn_piece_answers_both_questions_for_present_members_only() {
    let mut w = party_world();
    let piece = 0x47;
    let mut eq = w.party.roster.members[2].equipment();
    eq.slots[3] = piece;
    w.party.roster.members[2].set_equipment(eq);
    let bc_hit = |w: &mut World, sub: u8| {
        let mut bc = vec![0x21u8; 0x10];
        bc[..5].copy_from_slice(&[0x4C, sub, piece, 0x05, 0x00]);
        let mut ctx = FieldCtx::default();
        let mut host = FieldHostImpl { world: w };
        field_step_routed(&mut host, &mut ctx, &bc, 0)
    };
    assert!(matches!(
        bc_hit(&mut w, 0xDC),
        FieldStepResult::Advance { next_pc: 8 }
    ));
    assert!(matches!(
        bc_hit(&mut w, 0xDE),
        FieldStepResult::Advance { next_pc: 8 }
    ));
    // The member who wears it leaves: the roster scan reads present members.
    w.install_present_party_list(vec![0, 1]);
    assert!(matches!(
        bc_hit(&mut w, 0xDC),
        FieldStepResult::Advance { next_pc: 5 }
    ));
}
