use super::*;

/// A minimal `efect.dat` 2-pack: 1 atlas entry (a 24x24 sprite at texel
/// (5,7), tpage 0x88, clut 0x12), 1 anim batch (one frame -> atlas 0 with a
/// 4-frame hold so the seeded child persists under the retail walker), and
/// 1 effect script with 1 child referencing sprite_id 0.
fn minimal_efect_dat() -> Vec<u8> {
    let mut buf = vec![0u8; 8];
    // atlas[0]: u=5 v=7 w=24 h=24, CLUT@+4=0x88, tpage@+6=0x12, unk=0
    buf.extend_from_slice(&[5u8, 7, 24, 24]);
    buf.extend_from_slice(&0x88u16.to_le_bytes()); // CLUT (CBA)
    buf.extend_from_slice(&[0x12u8, 0]); // tpage byte, unk
    let pack0 = buf.len() as u32;
    // pack0: 1 anim batch, 1 frame (atlas_index 0, hold delay 4).
    buf.extend_from_slice(&1u32.to_le_bytes());
    let p0_tbl = buf.len();
    buf.extend_from_slice(&[0u8; 4]);
    let anim0 = buf.len() as u32;
    buf.extend_from_slice(&[1u8, 0]); // frame_count=1, flags
    buf.extend_from_slice(&[0u8, 4, 0, 0, 0, 0]); // frame 0 -> atlas 0, delay 4
    buf[p0_tbl..p0_tbl + 4].copy_from_slice(&anim0.to_le_bytes());
    let pack1 = buf.len() as u32;
    // pack1: 1 effect script, 1 child (sprite_id 0), flags 0 (no spread).
    buf.extend_from_slice(&1u32.to_le_bytes());
    let p1_tbl = buf.len();
    buf.extend_from_slice(&[0u8; 4]);
    let script0 = buf.len() as u32;
    buf.extend_from_slice(&[1u8, 0]); // child_count=1, flags=0
    buf.extend_from_slice(&0i16.to_le_bytes()); // spread
    buf.extend_from_slice(&0u16.to_le_bytes()); // child sprite_id=0
    buf.extend_from_slice(&0i16.to_le_bytes()); // width
    buf.extend_from_slice(&0u16.to_le_bytes()); // anim_flags
    buf.extend_from_slice(&0i16.to_le_bytes()); // depth
    buf.extend_from_slice(&[0u8; 6]); // tail
    buf[p1_tbl..p1_tbl + 4].copy_from_slice(&script0.to_le_bytes());
    buf[0..4].copy_from_slice(&pack0.to_le_bytes());
    buf[4..8].copy_from_slice(&pack1.to_le_bytes());
    buf
}

/// A spawned effect produces a faithful billboard sprite per child, sized
/// and UV-addressed from the real sprite atlas (the textured-quad seam).
#[test]
fn active_effect_sprites_carry_atlas_size_and_vram_coords() {
    use legaia_engine_vm::effect_vm::EffectCatalog;
    let mut world = World {
        effect_catalog: EffectCatalog::from_efect_dat_bytes(&minimal_efect_dat()),
        ..World::default()
    };
    assert_eq!(world.effect_catalog.len(), 1, "one effect script");

    // No effects yet -> no sprites.
    assert!(world.active_effect_sprites().is_empty());

    // Spawn effect 0 at world (10, 0, 20); the walker's first sweep seeds
    // the child from the spawn record (retail spawns are consumed by the
    // per-frame walker, not at spawn time).
    world.try_spawn_effect(0, [10, 0, 20], 0);
    assert!(world.active_effect_sprites().is_empty(), "no sweep yet");
    world.tick_effects();
    let sprites = world.active_effect_sprites();
    assert_eq!(sprites.len(), 1, "one child sprite");
    let s = sprites[0];
    assert_eq!(s.uv, [5, 7], "atlas texel origin");
    assert_eq!(s.uv_size, [24, 24], "atlas sprite size");
    // Pass-2 sizing: texels * sprite_scale (0xA00) >> 8 = x10.
    assert_eq!(s.size, [240.0, 240.0]);
    assert_eq!(s.page, 0x12, "tpage byte from atlas+6");
    assert_eq!(s.clut, 0x88, "CLUT (CBA) u16 from atlas+4");
    // Child seeded at the effect origin (zero offset legs, zero velocity).
    assert_eq!(s.world_pos, [10.0, 0.0, 20.0]);
    // Single-frame batch: n = 0 -> pure ramp-out from neutral.
    assert_eq!(s.brightness, 0x80);
}

/// The engine's live effect path IS the faithful walker: `World::tick_effects`
/// plus `active_effect_sprites` reproduce, frame by frame, a
/// `Pool::tick_retail` and `Pool::child_billboards` run driven directly with
/// the same catalog and the same RNG stream (including the randomized
/// spawn-offset path and the master/child lifecycle counts).
#[test]
fn world_effect_playback_matches_pool_walker_frame_by_frame() {
    use legaia_engine_vm::effect_vm::{
        AnimBatch, AnimFrame, ChildSprite, EffectCatalog, EffectHost, EffectScript, Pool,
        SpriteAtlasEntry,
    };

    /// Mirror of `World::next_rng` (the Numerical Recipes LCG the world-side
    /// `EffectHostImpl` feeds the pool), so the direct pool run consumes the
    /// identical RNG stream.
    struct LcgHost {
        state: u32,
    }
    impl EffectHost for LcgHost {
        fn next_random(&mut self) -> i32 {
            self.state = self
                .state
                .wrapping_mul(1_664_525)
                .wrapping_add(1_013_904_223);
            self.state as i32
        }
    }

    // A catalog exercising the whole algebra: randomized planar offsets
    // (flags bit 0, spread 24), staggered spawn delays, two multi-frame
    // batches with hold delays + motion speeds, and two atlas rects.
    let atlas = vec![
        SpriteAtlasEntry {
            u: 5,
            v: 7,
            w: 24,
            h: 24,
            clut: 0x88,
            page: 0x12,
            unk: 0,
        },
        SpriteAtlasEntry {
            u: 64,
            v: 0,
            w: 8,
            h: 16,
            clut: 0x99,
            page: 0x13,
            unk: 0,
        },
    ];
    let anims = vec![
        AnimBatch {
            flags: 0,
            frames: vec![
                AnimFrame {
                    atlas_index: 0,
                    timing: [2, 3, 0, 0, 0],
                },
                AnimFrame {
                    atlas_index: 1,
                    timing: [3, 1, 0, 0, 0],
                },
                AnimFrame {
                    atlas_index: 0,
                    timing: [1, 2, 0, 0, 0],
                },
            ],
        },
        AnimBatch {
            flags: 0,
            frames: vec![
                AnimFrame {
                    atlas_index: 1,
                    timing: [4, 2, 0, 0, 0],
                },
                AnimFrame {
                    atlas_index: 0,
                    timing: [2, 1, 0, 0, 0],
                },
            ],
        },
    ];
    let recs = vec![
        ChildSprite {
            sprite_id: 0,
            delay: 0,
            width: 12,
            height: 6,
            depth: -8,
            velocity: [3, -5, 2],
        },
        ChildSprite {
            sprite_id: 1,
            delay: 2,
            width: -4,
            height: 0,
            depth: 10,
            velocity: [0, 7, -1],
        },
        ChildSprite {
            sprite_id: 0,
            delay: 1,
            width: 0,
            height: 2,
            depth: 0,
            velocity: [-2, 0, 4],
        },
    ];
    let script = EffectScript {
        child_count: recs.len() as u8,
        flags: 0x01,
        spread: 24,
        body: vec![],
    };
    let catalog = EffectCatalog::from_parts(vec![(script, recs)], atlas, anims);

    let mut world = World {
        effect_catalog: catalog.clone(),
        ..World::default()
    };
    let mut host = LcgHost {
        state: world.rng_state,
    };
    let mut pool = Pool::new();

    world.try_spawn_effect(0, [10, -4, 20], 0x300);
    pool.spawn_by_ui_id(&mut host, 0, [10, -4, 20], 0x300, &catalog)
        .expect("direct pool spawn");

    let mut saw_sprites = false;
    for frame in 0..48 {
        world.tick_effects();
        pool.tick_retail(&mut host, &catalog, 1);

        let want = pool.child_billboards(&catalog);
        let got = world.active_effect_sprites();
        assert_eq!(got.len(), want.len(), "sprite count at frame {frame}");
        for (g, w) in got.iter().zip(&want) {
            assert_eq!(
                g.world_pos,
                [w.pos[0] as f32, w.pos[1] as f32, w.pos[2] as f32],
                "position at frame {frame}"
            );
            assert_eq!(g.uv, [w.entry.u as u16, w.entry.v as u16]);
            assert_eq!(g.uv_size, [w.entry.w as u16, w.entry.h as u16]);
            assert_eq!(g.page, w.entry.page);
            assert_eq!(g.clut, w.entry.clut);
            assert_eq!(g.size, [w.world_w.max(1) as f32, w.world_h.max(1) as f32]);
            assert_eq!(g.brightness, w.brightness, "brightness at frame {frame}");
            assert_eq!((g.flip_h, g.flip_v), (w.flip_h, w.flip_v));
        }
        saw_sprites |= !got.is_empty();
        assert_eq!(
            world.effect_pool.active_count(),
            pool.active_count(),
            "master lifecycle at frame {frame}"
        );
        assert_eq!(
            world.effect_pool.active_child_count(),
            pool.active_child_count(),
            "child lifecycle at frame {frame}"
        );
    }
    assert!(
        saw_sprites,
        "the playback produced billboards (non-vacuous)"
    );
    assert_eq!(world.effect_pool.active_child_count(), 0, "effect drained");
}

/// Per-monster-id scripted AI (the `FUN_801E9FD4` switch) end-to-end: a
/// wounded monster whose id has a low-HP self-heal case folds that heal onto
/// itself rather than striking the party. Monster id 6 (case `0x06`) casts
/// `0x52` at self when `HP <= maxHP/2` and its ability cooldown is clear.
#[test]
fn scripted_ai_monster_self_heals_when_wounded() {
    use crate::monster_catalog::vanilla_monster_catalog;
    use crate::spells::SpellCatalog;
    use legaia_engine_vm::battle_action::ActionState;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.set_spell_catalog(SpellCatalog::vanilla());
    world.monster_catalog = vanilla_monster_catalog();
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.liveness = 1;
    // Monster id 6 (Skeleton -> AI case 0x06) at slot 1, wounded to 20/100
    // with MP to spare for the heal.
    world.actors[1].battle.max_hp = 100;
    world.actors[1].battle.hp = 20;
    world.actors[1].battle.mp = 20;
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(6);
    world.rng_state = 7;

    world.take_monster_turn(1);

    assert_eq!(
        world.actors[1].battle.params[0], 0x52,
        "self-heal spell queued"
    );
    assert!(
        world.actors[1].battle.hp > 20,
        "the monster healed itself instead of striking the party"
    );
    assert_eq!(world.actors[0].battle.hp, 200, "party untouched");
    assert_eq!(
        world.battle_ctx.action_state,
        ActionState::EndOfAction.as_byte(),
        "cast is the whole turn"
    );
    assert_eq!(world.monster_ai_state.dat[4], 1, "ability cooldown armed");
    let fx = world.drain_battle_hit_fx();
    assert!(fx.iter().any(|f| f.is_heal && f.target_slot == 1));
}

/// Monster `0x8A` reads its own spirit-art gauge (`actor+0x170`) as a charge
/// gate: once it passes `0x31` the AI fires the `0x4E` all-enemies cast and the
/// live loop clamps the caster's gauge to `0x32`. Below the threshold the
/// generic core stands (this monster has no castable magic in the catalog, so
/// that means a physical strike) and the gauge is left untouched.
#[test]
fn monster_8a_charge_gate_drives_cast_and_clamps_gauge() {
    use crate::monster_catalog::vanilla_monster_catalog;
    use crate::spells::SpellCatalog;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.set_spell_catalog(SpellCatalog::vanilla());
    world.monster_catalog = vanilla_monster_catalog();
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.max_hp = 300;
    world.actors[1].battle.hp = 300;
    world.actors[1].battle.mp = 200;
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(0x8a);

    // Charged: the gate fires the 0x4E cast and clamps the gauge to 0x32.
    world.actors[1].battle.spirit_gauge = 100;
    world.rng_state = 7;
    match world.pick_monster_action(1) {
        MonsterAction::Cast { spell_id, .. } => assert_eq!(spell_id, 0x4e),
        other => panic!("expected the 0x4E charge cast, got {other:?}"),
    }
    assert_eq!(
        world.actors[1].battle.spirit_gauge, 0x32,
        "the gauge is clamped to 0x32 as the cast commits"
    );

    // Below threshold: no override, gauge left exactly as-is.
    world.actors[1].battle.spirit_gauge = 0x31;
    world.rng_state = 7;
    assert!(matches!(
        world.pick_monster_action(1),
        MonsterAction::Physical { .. }
    ));
    assert_eq!(
        world.actors[1].battle.spirit_gauge, 0x31,
        "an uncharged 0x8A leaves its gauge untouched"
    );
}

/// Faithful `FUN_801E7320`: a targeting class in `3..=6` resolves to a
/// living PARTY slot; a class in `0..=2` resolves to a living MONSTER slot.
/// (Dead slots are skipped via the re-roll loop.)
#[test]
fn monster_target_resolver_expands_class_to_correct_side() {
    let mut world = World {
        party_count: 3,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    // Party slots 0..2: slot 0 dead, slots 1+2 alive.
    for i in 0..3u8 {
        let a = &mut world.actors[i as usize];
        a.battle.max_hp = 100;
        a.battle.hp = if i == 0 { 0 } else { 100 };
        a.battle.liveness = if i == 0 { 0 } else { 1 };
    }
    // Monster slots 3+4 alive.
    for i in 3..5u8 {
        let a = &mut world.actors[i as usize];
        a.battle.max_hp = 80;
        a.battle.hp = 80;
        a.battle.liveness = 1;
    }

    // Caster = monster slot 3, class 3 (party-targeting). Resolves to a
    // LIVING party slot (1 or 2, never the dead slot 0).
    world.actors[3].battle.active_target = 3; // class 3..6 -> party
    world.rng_state = 12345;
    world.resolve_monster_target(3);
    let t = world.actors[3].battle.active_target;
    assert!(
        (1..=2).contains(&t),
        "class 3 -> living party slot, got {t}"
    );

    // Class 1 (monster-band targeting). Resolves to a living monster slot.
    world.actors[3].battle.active_target = 1; // class 0..2 -> monster band
    world.rng_state = 999;
    world.resolve_monster_target(3);
    let t = world.actors[3].battle.active_target;
    assert!(
        (3..=4).contains(&t),
        "class 1 -> living monster slot, got {t}"
    );
}

/// `advance_battle_mode` (the SM `case 0xFF` writer for `ctx+0x28A`) flips a
/// multi-phase boss from its first-phase cast to its phased cast on the next
/// turn. Monster id `0xB6` always casts, picking its spell purely by mode.
#[test]
fn advancing_the_battle_mode_drives_a_boss_to_its_next_phase() {
    use crate::monster_catalog::MonsterDef;
    use crate::spells::SpellCatalog;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.set_spell_catalog(SpellCatalog::vanilla());
    // A clean-room boss at monster slot 1 with id 0xB6 (no own magic - it
    // casts purely off its scripted phase table).
    world
        .monster_catalog
        .insert(MonsterDef::new(0xb6, "Boss", 400, 50));
    world.actors[0].battle.max_hp = 300;
    world.actors[0].battle.hp = 300;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.max_hp = 400;
    world.actors[1].battle.hp = 400;
    world.actors[1].battle.mp = 250;
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(0xb6);
    world.rng_state = 1;

    assert_eq!(world.battle_mode(), 0, "fresh battle starts in phase 0");
    world.take_monster_turn(1);
    assert_eq!(world.actors[1].battle.params[0], 0xa2, "phase 0 cast");

    // A scripted phase transition advances the mode; next turn is phase I.
    world.advance_battle_mode();
    assert_eq!(world.battle_mode(), 1);
    world.take_monster_turn(1);
    assert_eq!(world.actors[1].battle.params[0], 0xa3, "phase 1 cast");
}
