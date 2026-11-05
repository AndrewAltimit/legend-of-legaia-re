//! An inn stay's script ops through the field VM `FUN_801DE840`: the gold gate
//! the debit `3A` and the restore `4C 82 slot`.
use crate::common::{disc, scus_cpu};
use legaia_retail_exec::Stop;

const VM: u32 = 0x801D_E840;
const SCRIPT: u32 = 0x8011_0000;
const CTX: u32 = 0x8012_0000;
const RECORDS: u32 = 0x8008_4708;

/// `(HP, MP, SP)` currents of a record.
fn currents(cpu: &mut legaia_retail_exec::Cpu, slot: u32) -> [u16; 3] {
    let r = RECORDS + slot * 0x414;
    [
        cpu.mem.rd16(r + 0x106),
        cpu.mem.rd16(r + 0x10A),
        cpu.mem.rd16(r + 0x10E),
    ]
}

#[test]
fn a_stay_restores_and_charges_as_retail_does() {
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(0x801C_E818, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub_outside = Some((0x801C_E818, 0x8020_0000));
    cpu.mem.wr8(0x1F80_0393, 1);
    cpu.mem.wr32(CTX + 0x94, SCRIPT);
    // The party order is not consulted: the operand names the record.
    cpu.mem.wr8(0x8008_4598, 2);
    cpu.mem.wr8(0x8008_4599, 0);
    for slot in 0..3u8 {
        for s in 0..3u32 {
            let r = RECORDS + s * 0x414;
            for (o, v) in [
                (0x104, 1000),
                (0x106, 10),
                (0x108, 500),
                (0x10A, 5),
                (0x10C, 300),
                (0x10E, 3),
            ] {
                cpu.mem.wr16(r + o, v);
            }
        }
        cpu.mem.load(SCRIPT, &[0x4C, 0x82, slot, 0, 0, 0, 0, 0]);
        let before = cpu.mem.read(0x8008_4140, 0x2000);
        assert_eq!(cpu.call(VM, &[SCRIPT, 0, CTX]), Stop::Returned);
        let after = cpu.mem.read(0x8008_4140, 0x2000);
        let changed = before.iter().zip(&after).filter(|(a, b)| a != b).count();
        assert_eq!(changed, 4, "two halfwords and nothing else");
        for s in 0..3u32 {
            let want = if s == u32::from(slot) {
                [1000, 500, 3]
            } else {
                [10, 5, 3]
            };
            assert_eq!(currents(&mut cpu, s), want, "slot {slot} record {s}");
        }
    }
    // The debit, across the clamps `add_money` spells in the field host.
    const GOLD: u32 = 0x8008_459C;
    for (gold, delta) in [
        (500i32, -200i32),
        (100, -200),
        (0, -1),
        (9_999_990, 100),
        (9_999_999, 1),
    ] {
        cpu.mem.wr32(GOLD, gold as u32);
        let raw = (delta as u32).to_le_bytes();
        cpu.mem
            .load(SCRIPT, &[0x3A, raw[0], raw[1], raw[2], 0, 0, 0, 0]);
        assert_eq!(cpu.call(VM, &[SCRIPT, 0, CTX]), Stop::Returned);
        assert_eq!(
            cpu.mem.rd32(GOLD) as i32,
            (gold + delta).clamp(0, 9_999_999),
            "gold {gold} delta {delta}"
        );
    }
}
