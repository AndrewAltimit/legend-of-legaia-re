//! Op `4C C1` - the fog-region enable reset (`0x801E2674..0x801E26EC`):
//! every region's byte 0 becomes "its story flag is clear".

use super::*;
use crate::fog_particles::FogRegion;

fn region(enabled: bool, flag_index: u16) -> FogRegion {
    FogRegion {
        enabled,
        x0: 0,
        z0: 0,
        x1: 0x7E,
        z1: 0x7E,
        angle_base: 0,
        angle_spread: 0xFF,
        speed: 0x20,
        byte_8: 0x10,
        flag_index,
    }
}

#[test]
fn op_4c_c1_derives_each_region_enable_from_its_flag() {
    let mut w = World::new();
    // `retock`'s inn regions all key on 0x51C; one more on a clear flag,
    // carrying a stale "off" the reset must turn back on.
    w.fog.regions = vec![
        region(true, 0x51C),
        region(true, 0x51C),
        region(false, 0x19A),
    ];
    w.system_flag_set(0x51C);
    let mut ctx = FieldCtx::default();
    let mut host = FieldHostImpl { world: &mut w };
    match vm::field::step(&mut host, &mut ctx, &[0x4C, 0xC1], 0) {
        FieldStepResult::Advance { next_pc } => assert_eq!(next_pc, 2),
        other => panic!("4C C1 should advance 2 bytes, got {other:?}"),
    }
    let on: Vec<bool> = w.fog.regions.iter().map(|r| r.enabled).collect();
    assert_eq!(on, [false, false, true]);
}
