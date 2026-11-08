//! The two cast dispatchers `FUN_801F1ED4` (Seru id) and `FUN_801F2160`
//! (effect class) against `seru_spell_emitter` / `spell_class_emitter`: every
//! key through the jump tables, with the band's emitters stubbed and the call
//! compared; and `ctx[+0x27A]`'s gate on the shared tail.

use crate::common::{disc, scus_cpu};
use legaia_engine_battle_vm::battle_cast_dispatch::{
    CAST_DISPATCH_TAIL, seru_spell_emitter, spell_class_emitter,
};
use psx_retail_exec::Stop;

const SERU: u32 = 0x801F_1ED4;
const CLASS: u32 = 0x801F_2160;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const SPELLS: u32 = 0x8007_54C8;

fn run(cpu: &mut psx_retail_exec::Cpu, entry: u32) -> Vec<u32> {
    cpu.calls.clear();
    assert_eq!(cpu.call(entry, &[]), Stop::Returned, "{entry:#x}");
    cpu.calls.clone()
}

#[test]
fn every_cast_key_dispatches_as_retail_dispatches() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    // The band is not paged in: its emitters are the calls under test.
    cpu.stub_outside = Some((OVERLAY_BASE, 0x801F_69D8));
    cpu.stub(CAST_DISPATCH_TAIL, 0);
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr8(CTX + 0x13, 0);
    cpu.mem.wr32(ACTORS, ACTOR);
    let (mut seen, mut classes) = (0, 0);
    for tail in [0u8, 1] {
        cpu.mem.wr8(CTX + 0x27A, tail);
        for id in 0..=255u8 {
            cpu.mem.wr8(ACTOR + 0x1DF, id);
            let calls = run(&mut cpu, SERU);
            let port = seru_spell_emitter(id, tail);
            let emitter = calls
                .iter()
                .copied()
                .find(|c| (0x801F_69D8..0x8020_0000).contains(c));
            assert_eq!(emitter, port.emitter, "seru id {id:#x}");
            assert_eq!(
                calls.contains(&CAST_DISPATCH_TAIL),
                port.run_tail,
                "tail, id {id:#x} ctx27a {tail}"
            );
            seen += u32::from(emitter.is_some());
        }
        for spell in 0..=255u32 {
            // The class byte the dispatcher reads from the live table.
            cpu.mem.wr8(ACTOR + 0x1DF, spell as u8);
            let class = cpu.mem.rd8(SPELLS + spell * 0xC + 1);
            let calls = run(&mut cpu, CLASS);
            let port = spell_class_emitter(class, tail);
            let emitter = calls
                .iter()
                .copied()
                .find(|c| (0x801F_69D8..0x8020_0000).contains(c));
            assert_eq!(emitter, port.emitter, "spell {spell:#x} class {class:#x}");
            assert_eq!(calls.contains(&CAST_DISPATCH_TAIL), port.run_tail);
            classes += u32::from(emitter.is_some());
        }
    }
    assert!(seen > 40 && classes > 100, "{seen} {classes}");
}
