//! The two battle-overlay scans of a caster's learned-spell list (record
//! `+0x13C` count, `+0x13D` ids, `+0x161` levels): `FUN_801E91E8`, whether a
//! killing blow's Seru is already known, against `learned_seru_position` as
//! `World::roll_seru_absorb` calls it, and `FUN_801F3C34`, the "No effect."
//! pass of the summon return, against `queued_magic_message` over the
//! record's `spell_list`. Lists of every length, ids inside and outside the
//! Seru band, every gate.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_action::learned_seru_position;
use legaia_engine_battle_vm::move_no_effect_guard::queued_magic_message;
use legaia_save::CharacterRecord;
use psx_retail_exec::Stop;

const OVERLAY_BASE: u32 = 0x801C_E818;
const KNOWN: u32 = 0x801E_91E8;
const NO_EFFECT: u32 = 0x801F_3C34;
const CTX: u32 = 0x8012_0000;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const SEATS: u32 = 0x8007_BD10;
const ACTOR: u32 = 0x8013_0000;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const SPECIAL_BATTLE: u32 = 0x8007_BAC0;
const PENDING: u32 = 0x801F_6960;

fn record(rng: &mut Rng, spell: u8) -> Vec<u8> {
    let mut raw = vec![0u8; STRIDE as usize];
    let count = match rng.below(4) {
        0 => 0,
        1 => 0x24,
        _ => rng.below(0x25) as u8,
    };
    raw[0x13C] = count;
    for s in 0..0x24 {
        raw[0x13D + s] = match rng.below(6) {
            0 => rng.next() as u8,
            1 => rng.below(0x80) as u8,
            _ => 0x80 + rng.below(0x30) as u8,
        };
        raw[0x161 + s] = rng.below(10) as u8;
    }
    if rng.below(2) == 0 {
        let at = rng.below(0x24) as usize;
        raw[0x13D + at] = spell;
    }
    raw
}

#[test]
fn the_spell_list_scans_answer_as_retail() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.stub(0x801D_8DE8, 0);
    let clean = cpu.mem.clone();
    let mut rng = Rng(0x5345_5255);
    let (mut failures, mut known, mut messages) = (Vec::new(), 0, 0);
    for case in 0..60_000 {
        cpu.mem = clean.clone();
        let slot = rng.below(4) as u8;
        let member = rng.below(3);
        let armed = rng.below(4) != 0;
        let special = if rng.below(6) == 0 { 1 } else { 0 };
        let pending = rng.below(4) == 0;
        let action = match rng.below(4) {
            0 => rng.next() as u8,
            _ => 0x80 + rng.below(0x30) as u8,
        };
        let token = match rng.below(5) {
            0 => rng.next() as u8,
            _ => action.wrapping_sub(0x80),
        };
        let raw = record(&mut rng, action);
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.wr8(CTX + 0x13, slot);
        cpu.mem.wr8(CTX + 0x18, 0);
        cpu.mem.wr8(CTX + 0x25F + u32::from(slot), u8::from(armed));
        cpu.mem.wr32(SPECIAL_BATTLE, special);
        cpu.mem.wr32(PENDING, u32::from(pending));
        cpu.mem.wr32(ACTORS + 4 * u32::from(slot), ACTOR);
        cpu.mem.wr8(ACTOR + 0x1DF, action);
        cpu.mem.wr8(SEATS + u32::from(slot), member as u8 + 1);
        cpu.mem.load(RECORDS + member * STRIDE, &raw);

        assert_eq!(cpu.call(KNOWN, &[u32::from(token)]), Stop::Returned);
        let retail_known = cpu.regs.r[2] as u8;
        let rec = CharacterRecord::parse(&raw).unwrap();
        let list = rec.spell_list();
        let n = usize::from(list.count).min(list.ids.len());
        let port_known =
            learned_seru_position(token, slot < 3 && armed && special == 0, &list.ids[..n]);
        known += usize::from(retail_known > 1);

        // The summon return runs for a party caster.
        let retail_message = if slot < 3 {
            assert_eq!(cpu.call(NO_EFFECT, &[]), Stop::Returned);
            Some(cpu.mem.rd8(CTX + 0x18))
        } else {
            None
        };
        let port_message = (slot < 3)
            .then(|| queued_magic_message(action, &list.ids, &list.levels, pending).unwrap_or(0));
        messages += usize::from(retail_message == Some(0x66));

        if (retail_known, retail_message) != (port_known, port_message) && failures.len() < 12 {
            failures.push(format!(
                "case {case} slot {slot} token {token:#x} action {action:#x} count {} armed {armed} \
                 special {special} pending {pending}: retail {:?} port {:?}",
                raw[0x13C],
                (retail_known, retail_message),
                (port_known, port_message)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!("{known} known, {messages} messages");
    assert!(known > 2_000 && messages > 1_000, "{known} {messages}");
}
