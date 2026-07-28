//! Disc-free: the **runtime** ambient install and the halted-part free.
//!
//! Two properties of the retail chain, both read out of the disassembly:
//!
//!  - **There is one install chain, not two.** The field VM's op `0x34` sub-3
//!    arm is `0x801E00B0`'s `jal 0x800252EC`, and the scene-entry installer
//!    `FUN_8003A1E4` owns no install code - it calls `FUN_801DE840`, the same
//!    dispatcher, for one frame slice per just-spawned placement. So a runtime
//!    install and a load-slice install are the same op reached at two moments,
//!    and both must stage through the full `FUN_80021B04` port with its render
//!    tails. Routing the runtime arm at the older `SummonScene` pool left it
//!    running a stripped copy of the same tree.
//!  - **A halted part is freed.** `FUN_8002519C` walks the live actor list
//!    once per frame and tests `actor[+0x10] & 0x8` - move-VM op `0x08`
//!    HALT's bit - *before* dispatching the actor's tick word: when it is set
//!    the actor is torn down and its pool slot pushed back
//!    (`FUN_800204A4`). Without that, a tree that spawns on a loop grows a
//!    part per iteration forever.

use legaia_asset::summon_overlay::{RENDER_NODE_MODE_A, SummonPart};
use legaia_engine_core::world::World;
use legaia_engine_core::world::ambient::MAX_AMBIENT_PARTS;

/// Assemble a stager bundle from `[model_sel, bytecode words]` records and
/// install it on `world`. Each record is laid out as retail's
/// `[i16 model_sel][u16 flags][u16 bytecode...]`; the parsed table is set
/// directly rather than round-tripped through a synthetic container, which is
/// what `install_field_stagers` is for on real bytes.
fn install_records(world: &mut World, records: &[(i16, Vec<u16>)]) {
    let mut bytes: Vec<u8> = Vec::new();
    let mut parts: Vec<SummonPart> = Vec::new();
    for (model_sel, code) in records {
        let record_off = bytes.len();
        bytes.extend_from_slice(&model_sel.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        let bc_start = bytes.len();
        for w in code {
            bytes.extend_from_slice(&w.to_le_bytes());
        }
        parts.push(SummonPart {
            record_off,
            model_sel: *model_sel,
            flags: 0,
            bytecode: bc_start..bytes.len(),
        });
    }
    world.field_stager_bytes = bytes;
    world.field_stagers = parts;
}

/// A mode-3 CLUT-cell record: capture the cell, arm nonzero HSV velocities,
/// then park on a long wait so the part stays live for the whole test.
fn mode3_cycler(x: u16, y: u16) -> (i16, Vec<u16>) {
    (
        RENDER_NODE_MODE_A,
        vec![
            0x2C, x, y, 0x10, 0x01, // KEY_BUFFER_ALLOC [x, y, w, h] - arms +0x9C
            0x2E, 4, 0xFFFB, 7, // TWEEN_SCALE_SET: the H / S / V velocities
            0x09, 0x0FFF, // WAIT_SET (~1000 game ticks at the town cadence)
            0x08,   // HALT
        ],
    )
}

/// Op `0x34` sub-3 executed by the **live field VM** stages the record into
/// the ambient pool, seated at the executing context's position, and its
/// mode-3 render tail runs - none of which the `SummonScene` field-stager pool
/// this arm used to call does.
#[test]
fn runtime_op34_sub3_stages_into_the_ambient_pool_with_its_render_tail() {
    let mut world = World {
        frame_step: 2,
        ..Default::default()
    };
    // Record 0 stands in for the per-scene SFX descriptor bank (never a
    // stager); record 1 is what `34 30 00` installs - the `arg + 1` id law.
    install_records(&mut world, &[(-1, vec![0x08]), mode3_cycler(0x10, 0x1F6)]);

    world.load_field_script(vec![0x34, 0x30, 0x00]);
    // The seat retail uses is the executing script's context (`s5 + 0x14` /
    // `s5 + 0x24`), which a placement channel carries from its MAN record.
    world.field_ctx.world_x = 0x0240;
    world.field_ctx.world_y = 0x0030;
    world.field_ctx.world_z = 0x01C0;
    world.field_ctx.field_26 = 0x0800;
    world.step_field();

    assert_eq!(
        world.ambient_fx.len(),
        1,
        "the install lands in the ambient pool"
    );
    assert!(
        world.active_field_fx.is_empty(),
        "and not in the SummonScene field-stager pool (the debug exerciser's)"
    );
    let part = &world.ambient_fx[0];
    assert_eq!(
        (
            part.state.world_x,
            part.state.world_y,
            part.state.world_z,
            part.state.render_26
        ),
        (0x0240, 0x0030, 0x01C0, 0x0800),
        "seated at the executing context, not at the player"
    );

    // The mode-3 arm emits from the tick after the capture arms (`+0x9C > 1`).
    for _ in 0..4 {
        world.tick_ambient_fx();
    }
    let fx = world.active_ambient_cell_fx();
    assert_eq!(
        fx.len(),
        1,
        "the runtime install runs the mode-3 render tail"
    );
    assert_eq!(fx[0].rect, (0x10, 0x1F6, 0x10, 0x01), "the authored cell");
    assert!(
        fx[0].h_add != 0 && fx[0].s_add != 0 && fx[0].v_add != 0,
        "the HSV adds integrate ({fx:?})"
    );

    // The contrast that makes the routing change non-vacuous: the same record
    // through the old pool produces no render-tail output at all, however long
    // it is ticked.
    let mut old = World {
        frame_step: 2,
        ..Default::default()
    };
    install_records(&mut old, &[(-1, vec![0x08]), mode3_cycler(0x10, 0x1F6)]);
    assert!(old.spawn_field_stager(1, [0x0240, 0x0030, 0x01C0]));
    for _ in 0..4 {
        old.tick_field_fx(0x20);
    }
    assert!(
        old.active_ambient_cell_fx().is_empty(),
        "the SummonScene pool carries no CLUT-cell arm"
    );
}

/// A tree that spawns on a loop stays bounded, because a child that halts is
/// freed on the next walk. The same fixture with a child that never halts
/// grows instead - which is both the contrast that proves the spawns are
/// really happening and the shape of the defect this ports away.
#[test]
fn halted_parts_are_freed_so_a_spawn_loop_stays_bounded() {
    /// Installer: infinite `0x18 0x4000` loop around a one-tick wait and an
    /// op-`0x25` spawn of record 2 - the retail emitter idiom.
    fn emitter() -> (i16, Vec<u16>) {
        (
            -1,
            vec![
                0x18, 0x4000, // latch PC, infinite loop
                0x09, 0x0004, // WAIT_SET: 0x20, exactly one town-cadence tick
                0x25, 0x0002, // spawn child record 2
                0x19,   // loop back to the wait
            ],
        )
    }

    // Child that halts on its own first run: the ordinary particle shape.
    let mut halting = World {
        frame_step: 2,
        ..Default::default()
    };
    install_records(
        &mut halting,
        &[(-1, vec![0x08]), emitter(), (-1, vec![0x08])],
    );
    assert!(halting.spawn_ambient_record(1, [0, 0, 0]));
    let mut peak = 0usize;
    for _ in 0..600 {
        halting.tick_ambient_fx();
        peak = peak.max(halting.ambient_fx.len());
    }
    assert!(
        peak <= 4,
        "a halting child is freed each walk, so the pool stays flat (peak {peak})"
    );
    assert!(
        !halting.ambient_pool_exhausted(),
        "and the pool is never exhausted"
    );

    // Same emitter, child parked forever: nothing halts, so nothing is freed
    // and the population climbs to the pool ceiling. This is the contrast -
    // the bound above comes from the free path, not from the emitter failing
    // to spawn.
    let mut parking = World {
        frame_step: 2,
        ..Default::default()
    };
    install_records(
        &mut parking,
        &[
            (-1, vec![0x08]),
            emitter(),
            (-1, vec![0x1A, 0x4000, 0x09, 0x0FFF, 0x1B]),
        ],
    );
    assert!(parking.spawn_ambient_record(1, [0, 0, 0]));
    for _ in 0..600 {
        parking.tick_ambient_fx();
    }
    assert_eq!(
        parking.ambient_fx.len(),
        MAX_AMBIENT_PARTS,
        "parts that never halt accumulate to the pool ceiling"
    );
    assert!(
        parking.ambient_pool_exhausted(),
        "and exhaustion is queryable rather than silent"
    );
}
