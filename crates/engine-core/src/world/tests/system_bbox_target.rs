//! A system-script `BBOX_TEST` aimed at another actor (`CD <id> ..`) tests
//! that actor's position, not the player's. `nilboa`'s entry script raises
//! `0x4F2` (the boulders push) while the leader **or member `0x31`** stands
//! in `(53..55, 62..64)` (`+0x4D4` / `+0x4DF`).

use super::*;
use legaia_engine_mode::mode::SceneMode;

/// The box's middle in the test's own tiles, `(x - 0x40) >> 7` = `(54, 63)`.
const IN_BOX: (i16, i16) = (54 * 128 + 128, 63 * 128 + 128);
const FAR: (i16, i16) = (10 * 128 + 128, 10 * 128 + 128);

/// `CD 31 35 3E 37 40 04 00` (inside -> pc 8, outside -> pc 10), then
/// `54 F2` and a park.
fn world(player: (i16, i16), member: (i16, i16)) -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.install_field_player(0);
    w.actors[0].move_state.world_x = player.0;
    w.actors[0].move_state.world_z = player.1;
    w.sync_field_ctx_player_anchor();
    w.field_vm
        .channels
        .push(legaia_engine_field::field_channels::FieldChannel {
            placement_index: 7,
            ctx: FieldCtx {
                script_id: 0x31,
                ..FieldCtx::default()
            },
            record_offset: 0,
            pc: 0,
            done: false,
            object_bind: false,
        });
    w.npcs.positions.insert(7, member);
    w.field_bytecode = vec![
        0xCD, 0x31, 0x35, 0x3E, 0x37, 0x40, 0x04, 0x00, 0x54, 0xF2, 0x21,
    ];
    w.field_pc = 0;
    w
}

fn next_pc(mut w: World) -> usize {
    let _ = w.step_field();
    w.field_pc
}

#[test]
fn a_member_in_the_box_passes_while_the_leader_is_away() {
    assert_eq!(
        next_pc(world(FAR, IN_BOX)),
        8,
        "member 0x31 stands in the box"
    );
}

#[test]
fn the_leader_in_the_box_does_not_pass_a_test_on_the_member() {
    assert_eq!(
        next_pc(world(IN_BOX, FAR)),
        10,
        "the test reads member 0x31's position, not the player's"
    );
}
