//! The ambient part tick's mode-3 CLUT-cell arm against `koin1`'s spotlight
//! cone cycler, whose phase two retail captures pin.
//!
//! The record captures cell `(16, 507)` (op `0x2C`) and loops op `0x2E` (V
//! velocity `-2 << 3`), `WAIT_SET 5`, op `0x2E` (`+2 << 3`), `WAIT_SET 5`.
//! `FUN_80021DF4` drains the wait by `frame_step * DAT_1F80037D` (`3 * 8`),
//! then runs the mode-3 arm (`0x800226D8`: integrate the adds, upload the
//! cell), and only then the move VM (`0x80022B94`). Both captures hold
//! `v = -12`, velocity `+16` and wait `16`, with the two previous uploads still
//! in RAM at `v = -18` and `v = -12`.

use legaia_engine_core::world::*;
use legaia_overlay_images::summon_overlay::{RENDER_NODE_MODE_A, SummonPart};

fn cone_world() -> World {
    let mut world = World::new();
    let words: [u16; 18] = [
        0x2C, 16, 507, 16, 1, // capture the cell
        0x1A, 0x4000, // loop forever
        0x2E, 0, 0, 0xFFFE, 0x09, 5, // V velocity -16, wait 5
        0x2E, 0, 0, 2, 0x09,
    ];
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&RENDER_NODE_MODE_A.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    for w in words.iter().chain(&[5, 0x1B]) {
        bytes.extend_from_slice(&w.to_le_bytes());
    }
    world.props.stager_bytes = bytes.clone();
    world.props.stagers = vec![SummonPart {
        record_off: 0,
        model_sel: RENDER_NODE_MODE_A,
        reserved: 0,
        bytecode: 4..bytes.len(),
    }];
    world.clock.frame_step = 3;
    world
}

/// The adds each part tick uploads, from the spawn on.
fn uploads(world: &mut World, ticks: usize) -> Vec<Option<i16>> {
    (0..ticks)
        .map(|_| {
            world.tick_ambient_fx();
            world.ambient.fx[0].cell_fx.map(|f| f.v_add)
        })
        .collect()
}

#[test]
fn the_cone_cycler_integrates_before_its_move_vm_runs() {
    let mut world = cone_world();
    assert!(world.spawn_ambient_record(0, [0, 0, 0]));
    // The spawn-time run is the VM alone: the capture armed `+0x9C`, nothing
    // integrated or uploaded yet.
    assert_eq!(world.ambient.fx[0].state.field_9c, 1);
    assert_eq!(world.ambient.fx[0].state.tween_src_z, 0);
    assert!(world.ambient.fx[0].cell_fx.is_none());
    // Tick 1 integrates the spawn's velocity (`-16 * 3 * 8 >> 6 = -6`) but
    // does not upload (`+0x9C` was 1); each half of the loop then holds two
    // ticks, and the velocity a VM run sets moves the adds from the next tick.
    let seen = uploads(&mut world, 9);
    assert_eq!(
        seen,
        [
            None,
            Some(-12),
            Some(-6),
            Some(0),
            Some(-6),
            Some(-12),
            Some(-6),
            Some(0),
            Some(-6)
        ]
    );
}

/// The capture's own relation: on a tick that holds velocity `+16` and wait
/// `16` (the VM set both on the tick before), the upload one tick back is six
/// below this tick's and the one two ticks back equals it. Retail's newest
/// upload is `-12`, the older packet buffer's `-18`, and the displayed
/// framebuffer - drawn two ticks back - is lit like the newest. Running the VM
/// before the integrate would put the tick-before upload six below and the
/// one before that twelve below.
#[test]
fn the_capture_phase_relation_holds() {
    let mut world = cone_world();
    world.spawn_ambient_record(0, [0, 0, 0]);
    let mut hist: Vec<(Option<i16>, i16, i16)> = Vec::new();
    for _ in 0..24 {
        world.tick_ambient_fx();
        let part = &world.ambient.fx[0];
        hist.push((
            part.cell_fx.map(|f| f.v_add),
            part.state.tween_scale_z,
            part.state.wait_timer,
        ));
    }
    let mut matched = 0;
    // From tick 3 on: tick 1 integrates without uploading.
    for t in 3..hist.len() {
        if hist[t].1 == 16 && hist[t].2 == 16 {
            let (now, back1, back2) = (hist[t].0, hist[t - 1].0, hist[t - 2].0);
            assert_eq!(back1.zip(now).map(|(a, b)| b - a), Some(6));
            assert_eq!(back2, now);
            matched += 1;
        }
    }
    assert!(matched >= 3, "the capture phase recurs every four ticks");
}

/// Op `0x30` hands the capture back: `FUN_800583C8(actor + 0xA0, buf)`
/// (`jal` at `0x80023B04` in `FUN_80023070`) uploads the op-`0x2C` buffer
/// over the cell and the VM clears `+0x9C`. A part that recolours its cell,
/// runs `0x30` and lives on leaves the cell reading what it captured, not
/// its last recolour, through the VRAM pass both hosts run
/// (`World::step_ambient_fx`).
#[test]
fn op_0x30_writes_the_capture_back_over_the_recolour() {
    let mut world = World::new();
    let words: [u16; 16] = [
        0x2C, 16, 507, 16, 1, // capture the cell
        0x2E, 0, 0, 0xFFF8, // V velocity -64
        0x09, 12,   // wait a few ticks while the cell darkens
        0x30, // hand the capture back
        0x1A, 0x4000, 0x09, 0x7FF, // then idle, still alive
    ];
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&RENDER_NODE_MODE_A.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    for w in words.iter().chain(&[0x1B]) {
        bytes.extend_from_slice(&w.to_le_bytes());
    }
    world.props.stager_bytes = bytes.clone();
    world.props.stagers = vec![SummonPart {
        record_off: 0,
        model_sel: RENDER_NODE_MODE_A,
        reserved: 0,
        bytecode: 4..bytes.len(),
    }];
    world.clock.frame_step = 3;
    let mut vram = psx_tim::Vram::new();
    let cell: Vec<u16> = (0..16u16).map(|i| 0x4210 + i * 0x21).collect();
    let row: Vec<u8> = cell.iter().flat_map(|t| t.to_le_bytes()).collect();
    vram.write_block(16, 507, 16, 1, &row);
    let read =
        |vram: &psx_tim::Vram| -> Vec<u16> { (16..32).map(|x| vram.pixel(x, 507)).collect() };
    assert!(world.spawn_ambient_record(0, [0, 0, 0]));
    let mut recoloured = false;
    let mut restored_at = None;
    for t in 0..24 {
        world.ambient.pending_game_ticks = 1;
        world.step_ambient_fx(&mut vram);
        if read(&vram) != cell {
            recoloured = true;
        } else if recoloured && restored_at.is_none() {
            restored_at = Some(t);
        }
    }
    assert!(recoloured, "the cycler darkened the cell before op 0x30");
    assert!(
        restored_at.is_some(),
        "op 0x30 put the captured texels back"
    );
    assert_eq!(read(&vram), cell, "and the cell stays as captured");
    assert!(
        !world.ambient.fx[0].finished,
        "the part lives on past op 0x30"
    );
    assert_eq!(world.ambient.fx[0].state.field_9c, 0);
}
