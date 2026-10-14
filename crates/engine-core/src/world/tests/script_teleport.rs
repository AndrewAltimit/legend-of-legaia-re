//! Op-`0x4C 0xC3` script-table teleport: the **flat** record resolve
//! (`FUN_8003C8F0(ctx+0x50, 0)` + the `FUN_8003D0BC` name-field skip) and the
//! ctx write-set applied by the field-VM nibble-C dispatcher's case 3.
//!
//! The record a context re-seats itself at is its own **partition-1 actor
//! placement** (`+0x50 = N0 + placement_index`, flat-indexed), not a
//! partition-0 object record - see
//! [`legaia_engine_field::man_field_scripts::flat_record_span`].

use super::*;
use crate::world::vm_hosts::apply_script_table_teleport;

/// Byte-level minimal MAN: `headers.len()` partition-**1** placement records,
/// each `[N=1][2 locals][model, anim, bx, bz][0x21 halt]` (`pc0 = 7`),
/// followed by six zero-length sections so `man_section::parse` accepts the
/// buffer. Partition 0 is empty, so a context's flat `+0x50` id equals its
/// placement index.
fn man_bytes_with_placement_records(headers: &[[u8; 4]]) -> Vec<u8> {
    let records: Vec<([u8; 4], Vec<u8>)> = headers.iter().map(|h| (*h, vec![0x21])).collect();
    man_bytes_with_scripted_records(&records)
}

/// [`man_bytes_with_placement_records`] with each record's script body (the
/// bytes after the placement header) given.
fn man_bytes_with_scripted_records(records: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let headers: Vec<[u8; 4]> = records.iter().map(|r| r.0).collect();
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
        bodies.extend_from_slice(&records[i].1); // script body (pc0 = 1 + 2 + 4 = 7)
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
    let man_file = legaia_scene_formats::man_section::parse(&man).expect("fixture parses");
    let mut ctx = FieldCtx {
        script_id: 0,
        flags: 0xFFFF_FFFF,
        wait_accum: 77,
        field_8e: -5,
        field_8b: 0xEE,
        ..Default::default()
    };
    assert!(apply_script_table_teleport(&man_file, &man, &mut ctx).is_some());
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
    let man_file = legaia_scene_formats::man_section::parse(&man).expect("fixture parses");
    let mut ctx = FieldCtx::default();
    assert!(apply_script_table_teleport(&man_file, &man, &mut ctx).is_some());
    assert_eq!(ctx.world_x, 3 * 0x80 + 0x40 + 0x40);
    assert_eq!(ctx.world_z, 4 * 0x80 + 0x40);
    // The half-tile lands in the same grid column (0x200 - 0x40 >> 7 = 3).
    assert_eq!(ctx.npc_x, 3);
}

#[test]
fn unresolvable_script_id_leaves_ctx_untouched() {
    let man = man_bytes_with_placement_records(&[[0x00, 9, 0x03, 0x04]]);
    let man_file = legaia_scene_formats::man_section::parse(&man).expect("fixture parses");
    let mut ctx = FieldCtx {
        script_id: 5, // out of range: only record 0 exists
        world_x: 0x1234,
        flags: 0xFFFF_FFFF,
        ..Default::default()
    };
    assert!(apply_script_table_teleport(&man_file, &man, &mut ctx).is_none());
    assert_eq!(ctx.world_x, 0x1234);
    assert_eq!(ctx.flags, 0xFFFF_FFFF);
}

/// Full host path: `[0x4C, 0xC3]` stepped through [`FieldHostImpl`] resolves
/// the world's resident MAN ([`crate::world::FieldVmState::channels_man`]), applies the
/// teleport, and advances PC by 2.
#[test]
fn field_vm_op_4c_c3_teleports_through_the_host() {
    let man = man_bytes_with_placement_records(&[
        [0x00, 9, 0x03, 0x04],
        [0x00, 2, 0x10, 0x90], // record 1: bz high bit -> half tile
    ]);
    let mut world = World::new();
    world.field_vm.channels_man = Some(std::sync::Arc::new(man));
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

/// The `0x25` spawn section re-runs after the re-seat (`0x801E2800..
/// 0x801E282C`, `FUN_8003CF7C`): a section that seats its actor elsewhere
/// (here a bare `0x23` MoveTo) leaves it there, not on the header tile, and
/// the slice stops at its `0x21` - the move after it never runs.
#[test]
fn a_0x25_spawn_section_reruns_after_the_reseat() {
    let man = man_bytes_with_scripted_records(&[(
        [0x00, 0, 0x03, 0x04],
        vec![0x25, 0x23, 0x05, 0x06, 0x21, 0x23, 0x07, 0x07, 0x21],
    )]);
    let mut world = World::new();
    world.field_vm.channels_man = Some(std::sync::Arc::new(man));
    let mut ctx = FieldCtx::default();
    let code = [0x4C, 0xC3, 0x00];
    let mut host = FieldHostImpl { world: &mut world };
    let r = vm::field::step(&mut host, &mut ctx, &code, 0);
    assert!(matches!(r, FieldStepResult::Advance { next_pc: 2 }));
    assert_eq!(
        (ctx.world_x, ctx.world_z),
        (5 * 0x80 + 0x40, 6 * 0x80 + 0x40)
    );
    assert!(!world.field_vm.entry_prerun && !world.field_vm.respawn_rerun);
}

/// Only a `0x25` first opcode re-runs (`li v0,0x25; bne v1,v0` at
/// `0x801E2804..0x801E2808`); a `0x24` record keeps the header seat.
#[test]
fn a_record_opening_on_0x24_is_not_rerun() {
    let man = man_bytes_with_scripted_records(&[(
        [0x00, 0, 0x03, 0x04],
        vec![0x24, 0x23, 0x05, 0x06, 0x21],
    )]);
    let mut world = World::new();
    world.field_vm.channels_man = Some(std::sync::Arc::new(man));
    let mut ctx = FieldCtx::default();
    let code = [0x4C, 0xC3, 0x00];
    let mut host = FieldHostImpl { world: &mut world };
    let _ = vm::field::step(&mut host, &mut ctx, &code, 0);
    assert_eq!(
        (ctx.world_x, ctx.world_z),
        (3 * 0x80 + 0x40, 4 * 0x80 + 0x40)
    );
}

/// A partition-0 **object** record under the same read. The arm skips the
/// name field with `FUN_8003D0BC` and takes the next four bytes as the
/// placement header whatever partition the record is in
/// (`0x801E2700..0x801E27A4`); an object record has one attribute byte
/// there, so the anim is its first opcode, the tile is the two bytes after
/// it, and the rebased script offset lands three bytes into the script -
/// not on a `0x25`, so nothing re-runs.
#[test]
fn an_object_record_is_read_at_the_placement_header_offsets() {
    // One partition-0 record: `[n=1][name][attr][25][74 56 05 00][23 7F 7F][21]`.
    let body = [
        0x01, 0xAA, 0xBB, 0x07, 0x25, 0x74, 0x56, 0x05, 0x00, 0x23, 0x7F, 0x7F, 0x21,
    ];
    let mut man = vec![0u8; 0x2B + 3];
    man[0x22] = 1; // N0 = 1, N1 = N2 = 0
    man[0x28..0x2B].copy_from_slice(&(body.len() as u32).to_le_bytes()[..3]);
    man.extend_from_slice(&body);
    man.extend_from_slice(&[0u8; 18]);
    let man_file = legaia_scene_formats::man_section::parse(&man).expect("fixture parses");
    let mut ctx = FieldCtx::default();
    assert_eq!(
        apply_script_table_teleport(&man_file, &man, &mut ctx),
        Some(7),
        "script offset = name skip + 4"
    );
    assert_eq!(ctx.move_id, 0x25);
    assert_eq!(
        (ctx.world_x, ctx.world_z),
        (0x74 * 0x80 + 0x40, 0x56 * 0x80 + 0x40)
    );

    // Through the host: the byte at the rebased offset is `0x05`, so the
    // spawn section (whose flag arm would park the object at the hide box)
    // is not re-run and the header seat stands.
    let mut world = World::new();
    world.field_vm.channels_man = Some(std::sync::Arc::new(man));
    let mut ctx = FieldCtx::default();
    let code = [0x4C, 0xC3, 0x00];
    let mut host = FieldHostImpl { world: &mut world };
    let _ = vm::field::step(&mut host, &mut ctx, &code, 0);
    assert_eq!(
        (ctx.world_x, ctx.world_z),
        (0x74 * 0x80 + 0x40, 0x56 * 0x80 + 0x40)
    );
}
