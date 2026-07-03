use super::*;

/// Field move-VM stager spawn (op 0x34 sub-3 / `FUN_800252EC` → `spawn_field_stager`):
/// a stager record spawns a one-part field scene-graph effect, and the
/// non-visual `0x4001` sound-emitter node is split off the mesh draw path into
/// the render-node channel (the `FUN_80021DF4` `+0x5A` classification).
#[test]
fn field_stager_spawn_splits_sound_node_off_the_mesh_draws() {
    use crate::summon::RenderMode;
    use legaia_asset::summon_overlay::{RENDER_NODE_MODE_B, SummonPart};

    let mut world = World::new();

    // Two prescript records back-to-back: a transform node then a 0x4001 sound
    // node. Each = [i16 model_sel][u16 flags][move-VM HALT].
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(-1i16).to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0x08u16.to_le_bytes()); // HALT
    bytes.extend_from_slice(&0u16.to_le_bytes()); // pad
    let r1 = bytes.len();
    bytes.extend_from_slice(&RENDER_NODE_MODE_B.to_le_bytes()); // 0x4001 sound
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0x08u16.to_le_bytes()); // HALT

    world.field_stager_bytes = bytes.clone();
    world.field_stagers = vec![
        SummonPart {
            record_off: 0,
            model_sel: -1,
            flags: 0,
            bytecode: 4..8,
        },
        SummonPart {
            record_off: r1,
            model_sel: RENDER_NODE_MODE_B,
            flags: 0,
            bytecode: (r1 + 4)..bytes.len(),
        },
    ];

    // Spawn the 0x4001 sound node (id 1) at a world position.
    assert!(world.spawn_field_stager(1, [5, 6, 7]));
    assert_eq!(world.active_field_fx.len(), 1);

    // It surfaces as a SoundEmitter render node, NOT a mesh draw.
    let nodes = world.active_field_fx_render_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].mode, RenderMode::SoundEmitter);
    assert_eq!(nodes[0].world_pos, [5.0, 6.0, 7.0]);
    assert!(
        world.active_field_fx_part_draws().is_empty(),
        "the sound node never mesh-draws"
    );

    // Out-of-range id no-ops (retail bounds behaviour).
    assert!(!world.spawn_field_stager(99, [0, 0, 0]));
    // After a tick the effect is KEPT (a finished part holds its final pose
    // rather than draining the same frame it halts).
    world.tick_field_fx(0x0400);
    assert_eq!(
        world.active_field_fx.len(),
        1,
        "a finished field effect is kept (held at its final pose), not drained"
    );
    // Scene entry (install) clears live effects.
    world.install_field_stagers(&bytes);
    assert!(
        world.active_field_fx.is_empty(),
        "scene entry clears live field effects"
    );
}

#[test]
fn vm_dialogue_drives_inline_runner_via_tick_and_tears_down() {
    // With `use_vm_dialogue` set (the shell's default; `--simple-dialogue`
    // opts out), a field dialogue that carries an inline buffer is driven
    // through the field VM by tick()'s `drive_inline_dialogue` wrapper: the
    // runner auto-starts, steps the prologue + `0x1F` text segment, and on the
    // dismissing confirm clears `current_dialog` and emits `DialogDismissed`.
    // Minimal buffer: a 3-byte prologue, one `0x1F` "Hi" segment, terminator
    // (same shape dialog.rs's `from_inline_dialog_skips_prologue` fixture uses).
    let inline = vec![0x00u8, 0x56, 0x00, 0x1F, b'H', b'i', 0x00];
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.use_vm_dialogue = true;
    world.current_dialog = Some(DialogRequest {
        text_id: 0,
        inline,
        world_x: 0,
        world_z: 0,
        depth_id: 0,
    });

    let mut dismissed = false;
    for i in 0..4000 {
        // Edge-triggered confirm: pulse Cross (press on even frames, release on
        // odd) so `just_pressed` fires repeatedly to finish the typewriter and
        // dismiss the box, instead of a held mask that only edges once.
        world.set_pad(if i % 2 == 0 {
            input::PadButton::Cross.mask()
        } else {
            0
        });
        let _ = world.tick();
        if world
            .drain_field_events()
            .iter()
            .any(|e| matches!(e, crate::field_events::FieldEvent::DialogDismissed))
        {
            dismissed = true;
        }
        if world.current_dialog.is_none() && world.inline_dialogue.is_none() {
            break;
        }
    }
    assert!(
        dismissed,
        "the VM-driven dialogue must emit DialogDismissed on completion"
    );
    assert!(
        world.current_dialog.is_none(),
        "current_dialog cleared after the VM dialogue ends"
    );
    assert!(
        world.inline_dialogue.is_none(),
        "inline runner torn down after completion"
    );
}

#[test]
fn simple_dialogue_opt_out_leaves_runner_untouched() {
    // The `--simple-dialogue` path (`use_vm_dialogue == false`) must NOT start
    // the inline runner: `drive_inline_dialogue` returns early, so the request
    // stays owned by the simplified typewriter panel. This pins that the
    // default-on flip is opt-out-able and non-destructive.
    let inline = vec![0x00u8, 0x56, 0x00, 0x1F, b'H', b'i', 0x00];
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.use_vm_dialogue = false;
    world.current_dialog = Some(DialogRequest {
        text_id: 0,
        inline,
        world_x: 0,
        world_z: 0,
        depth_id: 0,
    });

    // No confirm press: with the VM runner disabled, dismissal is the
    // simplified path's job (host panel / interaction probe on a confirm),
    // not tick()'s. Ticking without input must leave the request open AND
    // never spin up the inline runner.
    for _ in 0..64 {
        world.set_pad(0);
        let _ = world.tick();
        assert!(
            world.inline_dialogue.is_none(),
            "opt-out must never start the VM runner"
        );
    }
    assert!(
        world.current_dialog.is_some(),
        "the request stays with the simplified panel when VM dialogue is off"
    );
}

/// A non-summon battle move whose move-power record carries a spawnable effect
/// entry requests a move-FX spawn (`World::pending_move_fx_spawn`) at the
/// target's battle position when it resolves through the shared cast path
/// (`cast_spell_on_slots`) - the engine-side wiring the host drains to call
/// `spawn_move_fx`. A move with NO effect entry requests nothing (the gate).
/// Disc-free: uses a synthetic PROT-0898 overlay; the actual scene-graph render
/// is proven on real data by `move_fx_render_disc`.
#[test]
fn battle_special_attack_requests_move_fx_spawn() {
    use crate::monster_catalog::vanilla_monster_catalog;
    use crate::move_power::MovePowerCatalog;
    use crate::spells::SpellCatalog;
    use legaia_asset::move_power::{
        MOVE_ID_INDEX_MAP_FILE_OFFSET, MOVE_POWER_RECORD_STRIDE, MOVE_POWER_TABLE_FILE_OFFSET,
        MOVE_POWER_TABLE_LEN,
    };

    // Flame (move id 0x20) -> power record 1; `with_fx` adds a `+0x12` Spawn(1)
    // on-contact entry so `move_has_spawn_fx(0x20)` is true.
    fn overlay(with_fx: bool) -> Vec<u8> {
        let mut buf = vec![
            0u8;
            MOVE_POWER_TABLE_FILE_OFFSET
                + MOVE_POWER_RECORD_STRIDE * MOVE_POWER_TABLE_LEN
        ];
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1; // structural guard
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 0x20] = 1; // Flame -> record 1
        let rec = MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE;
        buf[rec] = 0xB8; // +0 power 0x0BB8
        buf[rec + 1] = 0x0B;
        if with_fx {
            buf[rec + 0x12] = 0x01; // +0x12 on-contact list: Spawn(1)
        }
        buf
    }

    fn run(with_fx: bool) -> Option<(u8, [i16; 3])> {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        world.set_spell_catalog(SpellCatalog::vanilla());
        world.monster_catalog = vanilla_monster_catalog();
        // Party target at slot 0 with a distinct battle position so the request
        // origin is provably the target's position, not a constant.
        world.actors[0].battle.max_hp = 4000;
        world.actors[0].battle.hp = 4000;
        world.actors[0].battle.liveness = 1;
        world.actors[0].move_state.world_x = 100;
        world.actors[0].move_state.world_y = -50;
        world.actors[0].move_state.world_z = 200;
        world.battle_accuracy[0] = 30;
        world.battle_defense[0] = 40;
        // Bandit Boss (id 5) at slot 1 casts Flame (0x20) on seed 0.
        world.actors[1].battle.max_hp = 120;
        world.actors[1].battle.hp = 120;
        world.actors[1].battle.mp = 10;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(5);
        world.battle_accuracy[1] = 25;
        world.set_battle_magic(1, 40);
        world.move_power = MovePowerCatalog::from_overlay_0898(&overlay(with_fx));
        world.rng_state = 0;

        world.take_monster_turn(1);
        assert_eq!(world.actors[1].battle.params[0], 0x20, "picker chose Flame");
        // A non-summon move never requests a summon-creature spawn.
        assert!(world.pending_summon_spawn.is_none());
        world.pending_move_fx_spawn
    }

    // FX record -> a move-FX spawn request at the target's position.
    assert_eq!(
        run(true),
        Some((0x20, [100, -50, 200])),
        "Flame with a spawnable effect entry requests move-FX at the target"
    );
    // No FX entry -> no request (the move_has_spawn_fx gate).
    assert_eq!(
        run(false),
        None,
        "a move with no effect entry requests no move-FX spawn"
    );
}
