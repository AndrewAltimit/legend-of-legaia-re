//! `FUN_801E70BC`, the magic-level check a summon runs when it returns, against
//! `magic_xp::accrue_and_level`, the kernel `World::accrue_summon_spell_xp`
//! banks through. Every spell id a seat can cast, every level, the XP at either
//! side of each threshold.

use crate::common::{Rng, disc, scus, scus_cpu};
use legaia_engine_battle::magic_xp::accrue_and_level;
use legaia_save::CharacterRecord;
use psx_retail_exec::Stop;

const CHECK: u32 = 0x801E_70BC;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX: u32 = 0x8012_0000;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const SEATS: u32 = 0x8007_BD10;
const ACTOR: u32 = 0x8013_0000;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const TABLE: u32 = 0x8007_656C;

#[test]
fn the_magic_level_check_levels_what_retail_levels() {
    let (Some(mut cpu), Some(disc), Some(_)) = (scus_cpu(), disc(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.stub(0x801F_452C, 0);
    cpu.stub(0x801D_8DE8, 0);
    let table: Vec<u16> = (0..8u32).map(|i| cpu.mem.rd16(TABLE + i * 2)).collect();
    let mut rng = Rng(0x4D4C_564C);
    let (mut ups, mut stays, mut triple) = (0, 0, 0);
    for id in 0x80u32..=0xFF {
        let spell = id as u8;
        for level in 1..=10u8 {
            let thr = |mult: u32| u32::from(table[usize::from(level.min(8) - 1)]) * mult / 2;
            for xp in [
                0,
                1,
                thr(2).wrapping_sub(1),
                thr(2),
                thr(2) + 1,
                thr(3).wrapping_sub(1),
                thr(3),
                thr(3) + 1,
                0x7FFF_FFFF,
                0x8000_0000,
                0xFFFF_FFFF,
                rng.next(),
            ] {
                let member = rng.below(3);
                // The walk reads `0x20` of the list's bytes (`slti v0,a1,0x20`).
                let slot = rng.below(0x20) as usize;
                let mut raw = vec![0u8; STRIDE as usize];
                // The list holds other ids around it, and the spell only once.
                for s in 0..0x24 {
                    raw[0x13D + s] = 0x20 + s as u8 % 0x40;
                }
                raw[0x13C] = 0x20;
                raw[0x13D + slot] = spell;
                raw[0x161 + slot] = level;
                raw[0x08 + slot * 4..0x08 + slot * 4 + 4].copy_from_slice(&xp.to_le_bytes());

                cpu.mem.wr32(CTX_PTR, CTX);
                cpu.mem.wr8(CTX + 0x13, 0);
                cpu.mem.wr8(CTX + 0x26, 0);
                cpu.mem.wr32(ACTORS, ACTOR);
                cpu.mem.wr8(ACTOR + 0x1DF, spell);
                cpu.mem.wr8(SEATS, member as u8 + 1);
                let base = RECORDS + member * STRIDE;
                cpu.mem.load(base, &raw);
                assert_eq!(cpu.call(CHECK, &[]), Stop::Returned, "{spell:#x}");
                let retail_level = cpu.mem.rd8(base + 0x161 + slot as u32);
                let raised = cpu.mem.rd8(CTX + 0x26) == 0x65;

                let mut rec = CharacterRecord::parse(&raw).unwrap();
                let up = accrue_and_level(&mut rec, spell, 0, Some(&table));
                let port_level = rec.write()[0x161 + slot];
                assert_eq!(
                    (retail_level, raised),
                    (port_level, up.is_some()),
                    "spell {spell:#x} level {level} xp {xp:#x}"
                );
                if raised {
                    ups += 1;
                } else {
                    stays += 1;
                }
                if raised && xp >= thr(2) && xp <= thr(3) {
                    triple += 1;
                }
            }
        }
    }
    assert!(ups > 500 && stays > 500, "{ups} {stays} {triple}");
}

/// A spell the caster has not learned: the walk falls out at `0x20` and the
/// check reads the list's slot `0x20` (level byte `+0x181`, XP word `+0x88`),
/// a level `0` reading the halfword ahead of the table.
#[test]
fn an_unlearned_spell_is_checked_in_slot_0x20() {
    let (Some(mut cpu), Some(disc), Some(_)) = (scus_cpu(), disc(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.stub(0x801F_452C, 0);
    cpu.stub(0x801D_8DE8, 0);
    assert_eq!(
        u32::from(cpu.mem.rd16(TABLE - 2)),
        legaia_engine_battle_vm::battle_formulas::LEVEL_ZERO_THRESHOLD
    );
    let table: Vec<u16> = (0..8u32).map(|i| cpu.mem.rd16(TABLE + i * 2)).collect();
    let mut ups = 0;
    for spell in [0x81u8, 0x83, 0x86, 0x93, 0xA0] {
        for level in [0u8, 1, 2, 9] {
            for xp in [0u32, 1, 4, 17, 18, 0x40] {
                let mut raw = vec![0u8; STRIDE as usize];
                for s in 0..0x20 {
                    raw[0x13D + s] = 0x20 + s as u8;
                }
                raw[0x13C] = 0x20;
                raw[0x161 + 0x20] = level;
                raw[0x08 + 0x80..0x08 + 0x84].copy_from_slice(&xp.to_le_bytes());
                cpu.mem.wr32(CTX_PTR, CTX);
                cpu.mem.wr8(CTX + 0x13, 0);
                cpu.mem.wr8(CTX + 0x26, 0);
                cpu.mem.wr32(ACTORS, ACTOR);
                cpu.mem.wr8(ACTOR + 0x1DF, spell);
                cpu.mem.wr8(SEATS, 1);
                cpu.mem.load(RECORDS, &raw);
                assert_eq!(cpu.call(CHECK, &[]), Stop::Returned, "{spell:#x}");
                let retail_level = cpu.mem.rd8(RECORDS + 0x181);
                let raised = cpu.mem.rd8(CTX + 0x26) == 0x65;
                let mut rec = CharacterRecord::parse(&raw).unwrap();
                let up = accrue_and_level(&mut rec, spell, 0, Some(&table));
                assert_eq!(
                    (retail_level, raised),
                    (rec.write()[0x181], up.is_some()),
                    "spell {spell:#x} level {level} xp {xp:#x}"
                );
                ups += usize::from(raised);
            }
        }
    }
    assert!(ups > 10, "{ups}");
}
