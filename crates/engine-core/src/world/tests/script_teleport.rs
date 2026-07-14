//! Op-`0x4C 0xC3` script-table teleport: the **flat** record resolve
//! (`FUN_8003C8F0(ctx+0x50, 0)` + the `FUN_8003D0BC` name-field skip) and the
//! ctx write-set applied by the field-VM nibble-C dispatcher's case 3.
//!
//! The record a context re-seats itself at is its own **partition-1 actor
//! placement** (`+0x50 = N0 + placement_index`, flat-indexed), not a
//! partition-0 object record - see
//! [`crate::man_field_scripts::flat_record_span`].

use super::*;
use crate::world::vm_hosts::apply_script_table_teleport;

/// Byte-level minimal MAN: `headers.len()` partition-**1** placement records,
/// each `[N=1][2 locals][model, anim, bx, bz][0x21 halt]` (`pc0 = 7`),
/// followed by six zero-length sections so `man_section::parse` accepts the
/// buffer. Partition 0 is empty, so a context's flat `+0x50` id equals its
/// placement index.
fn man_bytes_with_placement_records(headers: &[[u8; 4]]) -> Vec<u8> {
    let mut man = vec![0u8; 0x2B + headers.len() * 3];
    // Partition counts at +0x22: N0 = 0, N1 = record count, N2 = 0.
    man[0x24] = headers.len() as u8;
    // Record bodies in the data region (offsets are data-region-relative).
    let mut bodies = Vec::new();
    for (i, h) in headers.iter().enumerate() {
        let off = bodies.len() as u32;
        let p = 0x2B + i * 3;
        man[p..p + 3].copy_from_slice(&off.to_le_bytes()[..3]);
        bodies.push(0x01); // N = 1 local pair
        bodies.extend_from_slice(&[0xAA, 0xBB]); // the local pair
        bodies.extend_from_slice(h); // 4-byte placement header
        bodies.push(0x21); // halt opcode (pc0 = 1 + 2 + 4 = 7)
    }
    // u24_at_28: the section chain starts right after the record bodies.
    let sec_off = bodies.len() as u32;
    man[0x28..0x2B].copy_from_slice(&sec_off.to_le_bytes()[..3]);
    man.extend_from_slice(&bodies);
    // Six zero-length sections (each a 3-byte zero length prefix).
    man.extend_from_slice(&[0u8; 18]);
    man
}

#[test]
fn teleport_reseats_ctx_at_its_own_placement_record_header() {
    // Record 0: anim 9, tile (3, 4) with no half-tile bits.
    let man = man_bytes_with_placement_records(&[[0x00, 9, 0x03, 0x04]]);
    let man_file = legaia_asset::man_section::parse(&man).expect("fixture parses");
    let mut ctx = FieldCtx {
        script_id: 0,
        flags: 0xFFFF_FFFF,
        wait_accum: 77,
        field_8e: -5,
        field_8b: 0xEE,
        ..Default::default()
    };
    assert!(apply_script_table_teleport(&man_file, &man, &mut ctx));
    // Tile-centre coords: (b & 0x7F) * 0x80 + 0x40.
    assert_eq!(ctx.world_x, 3 * 0x80 + 0x40);
    assert_eq!(ctx.world_z, 4 * 0x80 + 0x40);
    // Header anim byte lands in the +0x5C move-id slot.
    assert_eq!(ctx.move_id, 9);
    // The fixed resets of the dispatcher's case 3.
    assert_eq!(ctx.field_6a, 8);
    assert_eq!(ctx.field_72, 0x1000);
    assert_eq!(ctx.wait_accum, 0);
    assert_eq!(ctx.field_8e, 0);
    assert_eq!(ctx.local_flags, 0x15);
    assert_eq!(ctx.flags, 0x9EBF_FAFE, "flags word masked by 0x9EBFFAFE");
    // Grid coords recomputed from the fresh world position.
    assert_eq!(ctx.npc_x, 3);
    assert_eq!(ctx.npc_facing, 4);
    assert_eq!(ctx.field_8b, 0);
}

#[test]
fn coord_high_bit_adds_the_half_tile_offset() {
    // bx = 0x83 -> tile 3 + half (0x40 extra); bz = 0x04 plain.
    let man = man_bytes_with_placement_records(&[[0x00, 0, 0x83, 0x04]]);
    let man_file = legaia_asset::man_section::parse(&man).expect("fixture parses");
    let mut ctx = FieldCtx::default();
    assert!(apply_script_table_teleport(&man_file, &man, &mut ctx));
    assert_eq!(ctx.world_x, 3 * 0x80 + 0x40 + 0x40);
    assert_eq!(ctx.world_z, 4 * 0x80 + 0x40);
    // The half-tile lands in the same grid column (0x200 - 0x40 >> 7 = 3).
    assert_eq!(ctx.npc_x, 3);
}

#[test]
fn unresolvable_script_id_leaves_ctx_untouched() {
    let man = man_bytes_with_placement_records(&[[0x00, 9, 0x03, 0x04]]);
    let man_file = legaia_asset::man_section::parse(&man).expect("fixture parses");
    let mut ctx = FieldCtx {
        script_id: 5, // out of range: only record 0 exists
        world_x: 0x1234,
        flags: 0xFFFF_FFFF,
        ..Default::default()
    };
    assert!(!apply_script_table_teleport(&man_file, &man, &mut ctx));
    assert_eq!(ctx.world_x, 0x1234);
    assert_eq!(ctx.flags, 0xFFFF_FFFF);
}

/// Full host path: `[0x4C, 0xC3]` stepped through [`FieldHostImpl`] resolves
/// the world's resident MAN ([`World::field_channels_man`]), applies the
/// teleport, and advances PC by 2.
#[test]
fn field_vm_op_4c_c3_teleports_through_the_host() {
    let man = man_bytes_with_placement_records(&[
        [0x00, 9, 0x03, 0x04],
        [0x00, 2, 0x10, 0x90], // record 1: bz high bit -> half tile
    ]);
    let mut world = World::new();
    world.field_channels_man = Some(std::sync::Arc::new(man));
    let mut ctx = FieldCtx {
        script_id: 1,
        ..Default::default()
    };
    let code = [0x4C, 0xC3, 0x00];
    let mut host = FieldHostImpl { world: &mut world };
    let r = vm::field::step(&mut host, &mut ctx, &code, 0);
    assert!(
        matches!(r, FieldStepResult::Advance { next_pc: 2 }),
        "sub-3 is a 2-byte op: {r:?}"
    );
    assert_eq!(ctx.world_x, 0x10 * 0x80 + 0x40);
    assert_eq!(
        ctx.world_z,
        0x10 * 0x80 + 0x40 + 0x40,
        "0x90 = tile 0x10 + half"
    );
    assert_eq!(ctx.move_id, 2);

    // Without a resident MAN the override leaves the ctx alone (default
    // no-op semantics) but the op still advances.
    let mut world = World::new();
    let mut ctx = FieldCtx {
        script_id: 1,
        world_x: 0x777,
        ..Default::default()
    };
    let mut host = FieldHostImpl { world: &mut world };
    let r = vm::field::step(&mut host, &mut ctx, &code, 0);
    assert!(matches!(r, FieldStepResult::Advance { next_pc: 2 }));
    assert_eq!(ctx.world_x, 0x777);
}
