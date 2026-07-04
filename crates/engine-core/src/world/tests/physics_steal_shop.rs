use super::*;

// ------------------------------------------------------------------
// tick_actor_physics + MoveBufferHost wiring
// ------------------------------------------------------------------

/// Build a 1-record MOVE pool: index `id` -> offset `record_off`,
/// record body `[0, flag, fc_lo, fc_hi, 0, 0, divisor, 0]`.
fn make_move_pool(id: u16, record_off: usize, frame_count: u16, divisor: u8) -> Vec<u8> {
    // Table size matches retail's hard-coded 1024-entry view.
    let table_entries = 1024usize;
    let table_bytes = table_entries * 4;
    let total = (record_off + 16).max(table_bytes);
    let mut pool = vec![0u8; total];
    let off = (id as usize) * 4;
    pool[off..off + 4].copy_from_slice(&(record_off as u32).to_le_bytes());
    let fc = frame_count.to_le_bytes();
    pool[record_off + 1] = 0; // flag
    pool[record_off + 2] = fc[0];
    pool[record_off + 3] = fc[1];
    pool[record_off + 6] = divisor;
    pool
}

#[test]
fn tick_actor_physics_skips_inactive_slots() {
    let mut world = World::new();
    // No actor active; should be a no-op (no panics, no events).
    world.tick_actor_physics();
    assert!(world.last_tick_events.is_empty());
}

#[test]
fn tick_actor_physics_records_keyframe_event_for_active_actor() {
    let mut world = World::new();
    // Activate slot 0 on the keyframe dispatch arm; populate the
    // record pointer so the keyframe writeback fires.
    world.actors[0].active = true;
    world.actors[0].set_physics_dispatch(0x06);
    world.actors[0].physics.set_record_ptr(0x80100000);
    world.actors[0].physics.set_bone_count(8);
    world.tick_actor_physics();
    // One slot fired; events vector non-empty.
    assert_eq!(world.last_tick_events.len(), 1);
    let (slot, res) = &world.last_tick_events[0];
    assert_eq!(*slot, 0);
    assert!(
        res.events
            .iter()
            .any(|e| matches!(e, TickEvent::KeyframePoseWritten { bone_count: 8 }))
    );
}

#[test]
fn move_vm_kick_drives_cursor_advance_against_installed_pool() {
    let mut world = World::new();
    // Install a MOVE pool with id 3 -> record at offset 0x1010,
    // frame_count = 8, divisor = 1.
    world.set_move_buffer_root(make_move_pool(3, 0x1010, 8, 1));
    // Activate slot 0; set the move_vm_kick flag so the physics
    // tick's late-update emits TickEvent::MoveVmKick.
    world.actors[0].active = true;
    world.actors[0].set_physics_dispatch(0x06);
    world.actors[0].physics.move_vm_kick = 1;
    // Request move id 3; phase rate of 8 steps per frame.
    world.actors[0].move_buffer.cursor_requested = 3;
    world.actors[0].move_buffer.phase_rate = 8;
    world.tick_actor_physics();
    // MoveVmKick emitted.
    let (_, res) = &world.last_tick_events[0];
    assert!(
        res.events
            .iter()
            .any(|e| matches!(e, TickEvent::MoveVmKick))
    );
    // Cursor latched the new id and stepped once.
    assert_eq!(world.actors[0].move_buffer.cursor_active, 3);
    // First frame after latch: cursor_active==3, phase started at
    // 0, advanced by phase_rate * frame_delta = 8 * 1 = 8.
    assert_eq!(world.actors[0].move_buffer.phase, 8);
    // Move VM kick flag set by the latch (cursor_advance writes
    // move_vm_kick = 1 whenever it latches a new record).
    assert_eq!(world.actors[0].move_buffer.move_vm_kick, 1);
}

#[test]
fn move_vm_kick_no_record_is_graceful_noop() {
    let mut world = World::new();
    // No pool installed; cursor_advance's resolver returns None.
    world.actors[0].active = true;
    world.actors[0].set_physics_dispatch(0x06);
    world.actors[0].physics.move_vm_kick = 1;
    world.actors[0].move_buffer.cursor_requested = 5;
    world.actors[0].move_buffer.phase_rate = 8;
    world.tick_actor_physics();
    // Kick emitted but cursor stays idle (no record source).
    assert_eq!(world.actors[0].move_buffer.cursor_active, 0);
    assert_eq!(world.actors[0].move_buffer.phase, 0);
    assert_eq!(world.actors[0].move_buffer.move_vm_kick, 0);
}

#[test]
fn tick_does_not_advance_cursor_when_move_vm_kick_is_clear() {
    let mut world = World::new();
    world.set_move_buffer_root(make_move_pool(2, 0x1010, 4, 1));
    // Activate slot 0 but leave move_vm_kick = 0 in physics; the
    // late-update path does NOT emit MoveVmKick this frame, so
    // the cursor stays untouched even though a request is pending.
    world.actors[0].active = true;
    world.actors[0].set_physics_dispatch(0x06);
    world.actors[0].move_buffer.cursor_requested = 2;
    world.actors[0].move_buffer.phase_rate = 4;
    let before = world.actors[0].move_buffer.clone();
    world.tick_actor_physics();
    // Cursor unchanged (no kick).
    assert_eq!(world.actors[0].move_buffer, before);
}

#[test]
fn world_tick_runs_physics_pass_in_order() {
    // Smoke test: World::tick invokes tick_actor_physics. After
    // one tick with the kick flag set + a record installed, the
    // per-actor cursor should have advanced.
    let mut world = World::new();
    world.set_move_buffer_root(make_move_pool(1, 0x1010, 8, 1));
    world.actors[0].active = true;
    world.actors[0].set_physics_dispatch(0x06);
    world.actors[0].physics.move_vm_kick = 1;
    world.actors[0].move_buffer.cursor_requested = 1;
    world.actors[0].move_buffer.phase_rate = 4;
    // World::tick (no scene mode) returns None for Title; the
    // physics pass still runs unconditionally.
    world.tick();
    assert_eq!(world.actors[0].move_buffer.cursor_active, 1);
}

#[test]
fn apply_steal_grants_item_on_hit_and_respects_non_stealable() {
    use legaia_asset::steal_table::{StealEntry, StealTable};
    // ids: 0 sentinel, 1 = 30%/0x7e, 2 = 0% (no steal), 3 = 100%/0x8a.
    let table = StealTable::from_entries(vec![
        StealEntry {
            chance_pct: 0,
            item_id: 0xff,
        },
        StealEntry {
            chance_pct: 30,
            item_id: 0x7e,
        },
        StealEntry {
            chance_pct: 0,
            item_id: 0,
        },
        StealEntry {
            chance_pct: 100,
            item_id: 0x8a,
        },
    ]);

    // Seed so the first roll is 0 (lands for any chance >= 1).
    let mut world = World {
        rng_state: 32937,
        ..World::default()
    };
    let got = world.apply_steal(3, &table);
    assert_eq!(got, Some(0x8a), "100% steal lands and grants the item");
    assert_eq!(world.inventory.get(&0x8a).copied(), Some(1));

    // A non-stealable monster (0% chance) never grants and consumes no roll.
    let mut world = World::default();
    let rng_before = world.rng_state;
    assert_eq!(world.apply_steal(2, &table), None);
    assert!(world.inventory.is_empty());
    assert_eq!(
        world.rng_state, rng_before,
        "no roll for a non-stealable monster"
    );

    // An out-of-range / unknown monster id is also None.
    assert_eq!(World::default().apply_steal(999, &table), None);
}

// --- Live gold-shop trigger via field-VM op-0x49 (shop_catalog + try_arm_field_shop) ---

/// Build a field script that opens a 2-item shop: op `0x49` sub-0, length 0,
/// `[count=2][0x22][0x34]`, name `"Shop\0"`.
#[cfg(test)]
fn shop_op49_script() -> Vec<u8> {
    let mut code = vec![0x49, 0x00, 0x00, 0x02, 0x22, 0x34];
    code.extend_from_slice(b"Shop\0");
    code
}

#[test]
fn field_vm_op49_opens_a_gold_shop_then_resumes() {
    use vm::field::{FieldHost, Op49State};
    let mut world = World::new();
    // Priced item data: 0x22 = 50g, 0x34 = 120g (both sellable).
    let mut prices = [0u16; 256];
    prices[0x22] = 50;
    prices[0x34] = 120;
    world.item_shop_data = Some(crate::shop_catalog::ShopItemData::from_prices(prices));

    let code = shop_op49_script();
    let mut ctx = FieldCtx::default();
    let pc = 0usize;

    // Frame 1: Idle -> the host recognises the inline shop, arms it, VM halts.
    {
        let mut host = FieldHostImpl { world: &mut world };
        assert_eq!(host.op49_state(), Op49State::Idle);
        let r = vm::field::step(&mut host, &mut ctx, &code, pc);
        assert!(
            matches!(r, FieldStepResult::Halt { .. }),
            "op-0x49 suspends the script while the shop is up"
        );
    }
    assert!(
        world.field_shop_armed && world.field_shop_open,
        "shop armed"
    );
    // The opened shop carries the priced inline stock.
    let sess = world
        .take_pending_field_shop()
        .expect("the field VM opened a shop");
    let items: Vec<(u8, u32)> = sess
        .inventory
        .items
        .iter()
        .map(|i| (i.item_id, i.price))
        .collect();
    assert_eq!(items, vec![(0x22, 50), (0x34, 120)]);

    // Frame 2: shop still up -> Armed, VM stays suspended at the same pc.
    {
        let mut host = FieldHostImpl { world: &mut world };
        assert_eq!(host.op49_state(), Op49State::Armed);
        let r = vm::field::step(&mut host, &mut ctx, &code, pc);
        assert!(matches!(r, FieldStepResult::Halt { .. }));
    }

    // Player closes the shop -> Done; the VM advances past the merchant op.
    world.finish_field_shop();
    {
        let mut host = FieldHostImpl { world: &mut world };
        assert_eq!(host.op49_state(), Op49State::Done);
        match vm::field::step(&mut host, &mut ctx, &code, pc) {
            FieldStepResult::Advance { next_pc } => {
                assert!(next_pc > pc, "advanced past the shop record")
            }
            other => panic!("expected Advance, got {other:?}"),
        }
    }
    assert!(
        !world.field_shop_armed,
        "the arm clears so a later op-0x49 can open the next merchant"
    );
}

// --- Tile-board runtime install via field-VM op-0x49 sub-5 ---

/// A field script carrying an op `0x49` sub-5 board install: 13-byte inline
/// header `[5][ox=0][oz=0][w=4][h=4][radius=2][mode=0][flags×4][player_tpl]
/// [tile_base]`, followed by a sentinel op the script resumes onto.
#[cfg(test)]
fn tile_board_op49_script() -> Vec<u8> {
    vec![
        0x49, 0x05, 0x00, 0x00, 0x04, 0x04, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x21, 0x30,
    ]
}

#[test]
fn field_vm_op49_sub5_installs_a_tile_board_then_resumes_on_exit() {
    use vm::field::{FieldHost, Op49State};
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.player_actor_slot = Some(0);
    world.actors[0].active = true;
    world.rng_state = 7;

    let code = tile_board_op49_script();
    let mut ctx = FieldCtx::default();
    let pc = 0usize;

    // Frame 1: Idle -> the host parses the inline header, installs the
    // board, and the VM suspends.
    {
        let mut host = FieldHostImpl { world: &mut world };
        assert_eq!(host.op49_state(), Op49State::Idle);
        let r = vm::field::step(&mut host, &mut ctx, &code, pc);
        assert!(
            matches!(r, FieldStepResult::Halt { .. }),
            "op-0x49 sub-5 suspends the script while the board mode runs"
        );
    }
    let board = world.tile_board.as_ref().expect("board installed");
    assert_eq!((board.width, board.height), (4, 4));
    assert_eq!(board.cells.len(), 16);
    // The retail fill only produces cells in the known value classes.
    assert!(board.cells.iter().all(|&c| (2..=0xE).contains(&c)));
    let header = world.tile_board_header.expect("header kept");
    assert_eq!(header.player_template, 0x21);
    assert_eq!(header.tile_template_base, 0x30);
    // The player actor was seated at the start-cell centre.
    let (px, pz) = world.tile_board.as_ref().unwrap().player_world();
    assert_eq!(world.actors[0].move_state.world_x as i32, px);
    assert_eq!(world.actors[0].move_state.world_z as i32, pz);

    // While the board is up the op stays Armed at the same pc.
    {
        let mut host = FieldHostImpl { world: &mut world };
        assert_eq!(host.op49_state(), Op49State::Armed);
        let r = vm::field::step(&mut host, &mut ctx, &code, pc);
        assert!(matches!(r, FieldStepResult::Halt { .. }));
    }

    // Simulate the walk reaching an event/transition cell: plant one under
    // the player and run the arrival pass (the interpolation-complete path).
    {
        let b = world.tile_board.as_mut().unwrap();
        let idx = b.player_row as usize * b.width as usize + b.player_col as usize;
        b.cells[idx] = crate::tile_board::CELL_EVENT_FIRST;
        let (tx, tz) = b.player_world();
        world.tile_board_target = Some((tx, tz));
        world.set_pad(0);
        let _ = world.tick();
    }
    assert!(
        world.tile_board.is_none(),
        "landing on an event cell exits the board mode"
    );

    // Exit flips the op to Done; the script resumes past the 14-byte install.
    {
        let mut host = FieldHostImpl { world: &mut world };
        assert_eq!(host.op49_state(), Op49State::Done);
        match vm::field::step(&mut host, &mut ctx, &code, pc) {
            FieldStepResult::Advance { next_pc } => {
                assert_eq!(next_pc, 14, "sub-5 Done advances opcode + 13 header bytes")
            }
            other => panic!("expected Advance, got {other:?}"),
        }
    }
    assert!(!world.tile_board_armed, "the arm clears on resume");
}

#[test]
fn tile_board_animated_cell_cycles_on_arrival() {
    let mut w = tile_board_world();
    // Plant an animated tile at the player's cell and run the arrival pass
    // via a completed interpolation.
    {
        let b = w.tile_board.as_mut().unwrap();
        b.cells[0] = crate::tile_board::CELL_ANIM_LAST; // 0xE wraps to 0xB
        let (tx, tz) = b.player_world();
        w.tile_board_target = Some((tx, tz));
    }
    w.set_pad(0);
    let _ = w.tick();
    assert_eq!(
        w.tile_board.as_ref().unwrap().cells[0],
        crate::tile_board::CELL_ANIM_FIRST,
        "0xE cycles back to 0xB on arrival"
    );
}

// --- Screen-effect widgets via field-VM op-0x43 sub-ops (PROT-0900 family) ---

#[test]
fn field_vm_op43_widget_subops_drive_screen_fx_frame() {
    let mut world = World::new();
    world.mode = SceneMode::Field;

    let mut ctx = FieldCtx::default();

    // Sub-0x11: mask rect tween to a centre iris over 0 frames (snap).
    // [43][11][l lo hi][t][r][b][dur]
    let mut mask_op = vec![0x43, 0x11];
    for w in [80i16, 60, 240, 180, 0] {
        mask_op.extend_from_slice(&w.to_le_bytes());
    }
    // Sub-0x15: letterbox config [x_left][x_right][y0][y1][y2][y3].
    let mut lb_op = vec![0x43, 0x15];
    for w in [0i16, 0x140, 40, 56, 184, 200] {
        lb_op.extend_from_slice(&w.to_le_bytes());
    }
    // Sub-0x13: panel spawn [x][y][w][h][tex_x][tex_y] past the sub-op byte.
    let mut panel_op = vec![0x43, 0x13];
    for w in [16i16, 32, 128, 96, 0, 0x100] {
        panel_op.extend_from_slice(&w.to_le_bytes());
    }
    // Sub-0x10: sprite spawn, 19-byte record
    // [x][y][w][h][tex_x][tex_y][clut_x][clut_y][rgb u24].
    let mut sprite_op = vec![0x43, 0x10];
    for w in [100i16, 50, 24, 24, 0x40, 0, 0, 480] {
        sprite_op.extend_from_slice(&w.to_le_bytes());
    }
    sprite_op.extend_from_slice(&[0x80, 0x80, 0x80]);

    for op in [&mask_op, &lb_op, &panel_op, &sprite_op] {
        let mut host = FieldHostImpl { world: &mut world };
        match vm::field::step(&mut host, &mut ctx, op, 0) {
            FieldStepResult::Advance { .. } => {}
            other => panic!("widget sub-op should advance, got {other:?}"),
        }
    }
    assert!(world.screen_fx.mask.is_some(), "mask widget spawned");
    assert!(world.screen_fx.letterbox.is_some(), "letterbox configured");
    assert!(world.screen_fx.panel.is_some(), "panel spawned");
    assert_eq!(world.screen_fx.sprites.len(), 1, "sprite widget spawned");

    // One world tick publishes the frame: 4 mask border quads + 2 letterbox
    // bands, 2 gradient strips, 1 panel quad (128px wide - no split), 1 sprite.
    let _ = world.tick();
    let frame = &world.screen_fx_frame;
    assert_eq!(
        frame.solid_quads.len(),
        6,
        "4 mask quads + 2 letterbox bands"
    );
    assert_eq!(frame.gradient_quads.len(), 2);
    assert_eq!(frame.panels.len(), 1);
    assert_eq!(frame.sprites.len(), 1);
    // The dur=0 mask snapped to the requested iris rect: the top border quad
    // ends at the rect's top edge.
    assert!(
        frame
            .solid_quads
            .iter()
            .any(|q| q.bottom == 60 || q.top == 60),
        "mask border reflects the snapped iris rect"
    );

    // Sub-0x14: panel move/scale to half size over 0 frames.
    let mut move_op = vec![0x43, 0x14];
    for w in [200i16, 100, 0x0800, 4] {
        move_op.extend_from_slice(&w.to_le_bytes());
    }
    {
        let mut host = FieldHostImpl { world: &mut world };
        let r = vm::field::step(&mut host, &mut ctx, &move_op, 0);
        assert!(matches!(r, FieldStepResult::Advance { .. }));
    }
    let p = world.screen_fx.panel.as_ref().unwrap();
    assert_eq!(p.target[0], 200);
    assert_eq!(p.target[2], 64, "0x0800 (4.12) halves the 128px base width");
}

#[test]
fn field_shop_carries_a_stable_vendor_id_that_drives_trading() {
    // The op-0x49 shop arm captures a per-vendor id (from the shop's name +
    // stock) so seru trading reached through that shop keys on the right vendor.
    let mut prices = [0u16; 256];
    prices[0x22] = 50;
    prices[0x34] = 120;

    let mut world = World::new();
    world.item_shop_data = Some(crate::shop_catalog::ShopItemData::from_prices(prices));
    assert!(world.try_arm_field_shop(&shop_op49_script()));
    let sess = world.take_pending_field_shop().expect("shop opened");

    // The id is the stable derivation from the shop's identity ("Shop", stock).
    let expected = legaia_asset::seru_trade::vendor_id_from_shop("Shop", &[0x22, 0x34]);
    assert_eq!(sess.vendor_id, expected);
    assert_ne!(sess.vendor_id, 0, "a real vendor gets a concrete id");

    // With trading enabled and a party that owns seru, opening a trade for that
    // vendor yields offers (the through-the-shop path the host drives).
    world.seru_trade_config = Some(legaia_asset::seru_trade::SeruTradeConfig {
        enabled: true,
        seed: 0x1234,
        max_offers: 4,
    });
    let mut lead = legaia_save::CharacterRecord::zeroed();
    let mut list = legaia_save::SpellList::default();
    list.ids[0] = 0x81;
    list.ids[1] = 0x88;
    list.count = 2;
    lead.set_spell_list(list);
    world.roster = legaia_save::Party {
        members: vec![lead],
    };

    let session = world
        .open_seru_trade(sess.vendor_id)
        .expect("trading enabled -> session opens");
    assert!(
        !session.is_empty(),
        "the party owns seru, so the vendor offers trades"
    );
}

#[test]
fn field_vm_op49_non_shop_payload_does_not_open_a_shop() {
    let mut world = World::new();
    // Only 0x22 is priced. A genuine shop LEADS with a sellable item (a real
    // shop's unsellable template ids are only ever a trailing padding tail,
    // never the lead). This payload leads with an unpriced id, so the sellable
    // mask rejects it as not a gold shop.
    let mut prices = [0u16; 256];
    prices[0x22] = 50;
    world.item_shop_data = Some(crate::shop_catalog::ShopItemData::from_prices(prices));
    let mut code = vec![0x49, 0x00, 0x00, 0x02, 0x34, 0x22];
    code.extend_from_slice(b"Shop\0");
    let mut ctx = FieldCtx::default();
    {
        let mut host = FieldHostImpl { world: &mut world };
        let _ = vm::field::step(&mut host, &mut ctx, &code, 0);
    }
    assert!(
        !world.field_shop_armed,
        "a payload that doesn't lead with a sellable item is not a gold shop"
    );
    assert!(world.take_pending_field_shop().is_none());
}

#[test]
fn field_vm_op49_trims_unsellable_padding_to_the_sellable_stock() {
    let mut world = World::new();
    // 0x22/0x34 priced; 0x03 the trailing unsellable template-id padding the
    // record `count` over-counts. The shop opens with only the sellable stock.
    let mut prices = [0u16; 256];
    prices[0x22] = 50;
    prices[0x34] = 120;
    world.item_shop_data = Some(crate::shop_catalog::ShopItemData::from_prices(prices));
    let mut code = vec![0x49, 0x00, 0x00, 0x03, 0x22, 0x34, 0x03];
    code.extend_from_slice(b"Shop\0");
    let mut ctx = FieldCtx::default();
    {
        let mut host = FieldHostImpl { world: &mut world };
        let _ = vm::field::step(&mut host, &mut ctx, &code, 0);
    }
    let sess = world
        .take_pending_field_shop()
        .expect("the field VM opened the shop (padding doesn't reject it)");
    let items: Vec<(u8, u32)> = sess
        .inventory
        .items
        .iter()
        .map(|i| (i.item_id, i.price))
        .collect();
    assert_eq!(items, vec![(0x22, 50), (0x34, 120)], "0x03 padding trimmed");
}

#[test]
fn field_vm_op49_without_item_data_never_opens_a_shop() {
    // Disc-free build: no prices installed -> no sellable mask, so a stray
    // op-0x49 sub-0 can never be mistaken for a shop (and there'd be no prices).
    let mut world = World::new();
    let code = shop_op49_script();
    let mut ctx = FieldCtx::default();
    {
        let mut host = FieldHostImpl { world: &mut world };
        let _ = vm::field::step(&mut host, &mut ctx, &code, 0);
    }
    assert!(!world.field_shop_armed);
    assert!(world.take_pending_field_shop().is_none());
}

/// op-0x45 camera params MERGE per slot across beats (retail `FUN_801DE084`
/// writes each masked slot into a persistent camera struct). A later beat that
/// sets only slot 9 (H) - opdeene has exactly such a beat - must keep the
/// focus / pitch / eye-depth staged by the previous beat, not drop them.
#[test]
fn camera_configure_merges_params_across_beats() {
    use legaia_engine_vm::field::{CameraParam, FieldHost};
    let mut world = World::new();
    let val = |slots: &[u8], v: u16| -> Vec<CameraParam> {
        slots
            .iter()
            .map(|&slot| CameraParam { slot, value: v })
            .collect()
    };
    {
        let mut host = FieldHostImpl { world: &mut world };
        // Beat 1: a fully-staged shot (all slots but focus-Y / slot 7).
        host.camera_configure(&val(&[0, 1, 2, 3, 4, 5, 6, 8, 9], 111), 0, 0);
    }
    let get = |w: &World, slot: u8| {
        w.camera_state
            .params
            .iter()
            .find(|p| p.slot == slot)
            .map(|p| p.value)
    };
    assert_eq!(get(&world, 6), Some(111), "beat 1 stages focus X");
    {
        let mut host = FieldHostImpl { world: &mut world };
        // Beat 2: a H-only tweak, like opdeene's `[(9, 792)]`.
        host.camera_configure(
            &[CameraParam {
                slot: 9,
                value: 792,
            }],
            0,
            0,
        );
    }
    assert_eq!(get(&world, 9), Some(792), "beat 2 updates H");
    assert_eq!(
        get(&world, 6),
        Some(111),
        "focus X survives the H-only beat"
    );
    assert_eq!(get(&world, 0), Some(111), "pitch survives the H-only beat");
    assert_eq!(
        get(&world, 5),
        Some(111),
        "eye-depth survives the H-only beat"
    );
    assert_eq!(world.camera_state.params.len(), 9, "no slot dropped");
}
