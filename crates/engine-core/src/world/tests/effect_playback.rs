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

    /// Mirror of `World::next_rand` (the BIOS `rand()` step and shape,, which the world-side `EffectHostImpl` feeds the
    /// pool), so the direct pool run consumes the identical RNG stream.
    struct LcgHost {
        state: u32,
    }
    impl EffectHost for LcgHost {
        fn next_random(&mut self) -> i32 {
            self.state = legaia_engine_system::rand::world_lcg_step(self.state);
            // The BIOS `rand()` shape the world host hands the walker.
            ((self.state >> 16) & 0x7FFF) as i32
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
