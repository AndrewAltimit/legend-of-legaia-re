//! The move-VM spawn `FUN_80021B04` against the port's reading of it: the
//! init-word classification `SpawnSubmode::classify` and its two predicates,
//! the spawn descriptor's model index `template_model_index`, and the
//! summon part render mode `RenderMode::from_model_sel`.
//!
//! The allocator `FUN_80020DE0` hands back a record filled with noise (with
//! an OBJECT-table pointer and a model slot that resolve), the GTE transform
//! and the move-VM kick return at once. Compared per init word: the staged
//! model index `DAT_80070630`, the render mode `+0x5A`, whether the
//! OBJECT-table rebuild ran (`actor[+0x44]` filled from `DAT_8007C018`) with
//! the `0x08000000` flag it sets, and whether the render-scratch clear ran
//! (`+0x96 = rot[1] & 0xFFF`).

use crate::common::{Rng, scus_cpu};
use legaia_engine_effects::summon::RenderMode;
use legaia_engine_vm::move_vm::{SpawnSubmode, template_model_index};
use psx_retail_exec::{Stop, V0};

const SPAWN: u32 = 0x8002_1B04;
const ALLOC: u32 = 0x8002_0DE0;
const TRANSFORM: u32 = 0x8003_D344;
const MOVE_VM: u32 = 0x8002_3070;
const STAGED: u32 = 0x8007_0630;
/// `gp[0x754]`, the scene's model base.
const MODEL_BASE: u32 = 0x8007_BA6C;
const OBJECTS: u32 = 0x8007_C018;
const ACTOR: u32 = 0x8013_0000;
const OBJECT_TABLE: u32 = 0x8013_1000;
const MODEL: u32 = 0x8013_2000;
const POS: u32 = 0x8014_0000;
const ROT: u32 = 0x8014_0010;
const BUFFER: u32 = 0x8014_0020;

#[test]
fn the_spawn_classifies_as_the_port_does() {
    differential!(scus(SPAWN));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(ALLOC, ACTOR);
    cpu.stub(TRANSFORM, 0);
    cpu.stub(MOVE_VM, 0);
    let mut rng = Rng(0x5350_574E);
    let mut seen = [0u32; 4];
    for round in 0..20_000 {
        let init: u16 = match rng.below(6) {
            0 => 0x4000,
            1 => 0x4001,
            2 => 0x8000 | rng.next() as u16,
            3 => [0x3FFF, 0x4002, 0x7FFF, 0][rng.below(4) as usize],
            _ => rng.below(0x100) as u16,
        };
        let base = rng.next() as u16;
        let rot = [rng.next() as u16, rng.next() as u16, rng.next() as u16];
        for o in (0..0x100).step_by(2) {
            cpu.mem.wr16(ACTOR + o, rng.next() as u16);
        }
        let model_slot = rng.below(8) as u16;
        cpu.mem.wr16(ACTOR + 0x64, model_slot);
        cpu.mem.wr32(ACTOR + 0x44, OBJECT_TABLE);
        cpu.mem.wr32(OBJECTS + u32::from(model_slot) * 4, MODEL);
        let parts = 1 + rng.below(6);
        cpu.mem.wr32(MODEL + 8, parts);
        for i in 0..8 {
            cpu.mem.wr32(OBJECT_TABLE + i * 4, 0xDEAD_0000 | i);
        }
        cpu.mem.wr16(MODEL_BASE, base);
        cpu.mem.wr16(STAGED, rng.next() as u16);
        for (i, r) in rot.iter().enumerate() {
            cpu.mem.wr16(ROT + 2 * i as u32, *r);
        }
        cpu.mem.wr16(BUFFER, init);
        assert_eq!(
            cpu.call(SPAWN, &[POS, ROT, BUFFER, 0x1000]),
            Stop::Returned,
            "round {round}"
        );
        assert_eq!(cpu.regs.r[V0], ACTOR);
        let c = format!("round {round} init {init:#06x}");
        let sub = SpawnSubmode::classify(init);
        seen[sub as usize] += 1;
        assert_eq!(
            cpu.mem.rd16(STAGED),
            template_model_index(init, base),
            "{c}: model index"
        );
        let mode = match sub {
            SpawnSubmode::Negative => 0,
            SpawnSubmode::Default => 1,
            _ => RenderMode::from_model_sel(init as i16)
                .expect("a render-mode node")
                .raw(),
        };
        assert_eq!(cpu.mem.rd16(ACTOR + 0x5A) as i16, mode, "{c}: +0x5A");
        if !matches!(sub, SpawnSubmode::Keyframe | SpawnSubmode::Tween) {
            assert_eq!(RenderMode::from_model_sel(init as i16), None, "{c}");
        }
        // The OBJECT-table rebuild: the part count, then a pointer per part
        // (the first slot is overwritten with `1` by the spawn's tail).
        let rebuilt = cpu.mem.rd32(OBJECT_TABLE + 4) == MODEL + 0xC;
        assert_eq!(rebuilt, sub.rebuilds_object_table(), "{c}: OBJECT table");
        let flags = cpu.mem.rd32(ACTOR + 0x10);
        assert!(
            flags & 0x0800_0000 != 0 || !sub.rebuilds_object_table(),
            "{c}: flags {flags:#x}"
        );
        let scratch = cpu.mem.rd16(ACTOR + 0x96) == rot[1] & 0xFFF
            && cpu.mem.rd16(ACTOR + 0xC0) == 0
            && cpu.mem.rd16(ACTOR + 0x80) == 0;
        assert_eq!(scratch, sub.clears_render_scratch(), "{c}: render scratch");
    }
    assert!(seen.iter().all(|&n| n > 1_000), "{seen:?}");
}
