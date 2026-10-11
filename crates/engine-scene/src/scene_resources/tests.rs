use super::*;
use crate::scene::SceneEntry;
use legaia_disc_audit::categorize::Class;
use std::sync::Arc;

/// Synthetic 16bpp TIM at `(fb_x, fb_y)` (4 px × 1 row, no CLUT).
/// `bs_len` = 12 (block header) + 4 * 1 * 2 (pixel data) = 20.
fn synth_tim_16bpp(fb_x: u16, fb_y: u16) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&0x10u32.to_le_bytes());
    buf.extend_from_slice(&0x02u32.to_le_bytes()); // pmode 2 = 16bpp, no CLUT
    buf.extend_from_slice(&20u32.to_le_bytes()); // bs_len
    buf.extend_from_slice(&fb_x.to_le_bytes());
    buf.extend_from_slice(&fb_y.to_le_bytes());
    buf.extend_from_slice(&4u16.to_le_bytes()); // fb_w
    buf.extend_from_slice(&1u16.to_le_bytes()); // h
    for px in [0xAAAAu16, 0x5555, 0xF00F, 0x0FF0] {
        buf.extend_from_slice(&px.to_le_bytes());
    }
    buf
}

fn make_scene(entries: Vec<SceneEntry>) -> Scene {
    Scene {
        name: "test".into(),
        start: 0,
        end: entries.len() as u32,
        entries,
    }
}

#[test]
fn build_uploads_tim_into_vram() {
    let scene = make_scene(vec![SceneEntry {
        idx: 0,
        class: Class::UnknownOther,
        bytes: Arc::new(synth_tim_16bpp(64, 64)),
    }]);
    let res = SceneResources::build(&scene).unwrap();
    assert_eq!(res.tim_count, 1);
    assert_eq!(res.tim_parse_failures, 0);
    // The 4 image pixels should be visible in VRAM at (64..68, 64).
    assert_eq!(res.vram.pixel(64, 64), 0xAAAA);
    assert_eq!(res.vram.pixel(65, 64), 0x5555);
    assert_eq!(res.vram.pixel(66, 64), 0xF00F);
    assert_eq!(res.vram.pixel(67, 64), 0x0FF0);
}

#[test]
fn build_handles_empty_scene() {
    let scene = make_scene(vec![]);
    let res = SceneResources::build(&scene).unwrap();
    assert_eq!(res.tim_count, 0);
    assert_eq!(res.tim_parse_failures, 0);
    assert!(res.tmds.is_empty());
}

#[test]
fn build_filtered_vram_mesh_returns_empty_for_empty_tmd() {
    // ResolvedTmd around an empty TMD just to exercise the helper -
    // the filter only matters when prims exist, but we want to prove
    // the wiring compiles and runs without panicking on the no-prim
    // edge case (which is what most synthetic test fixtures hit).
    let rtmd = ResolvedTmd {
        entry_idx: 0,
        offset: 0,
        byte_len: 0,
        tmd: legaia_tmd::Tmd {
            header: legaia_tmd::Header {
                id: 0x80000002,
                flags: 0,
                nobj: 0,
                flist_bit_set: false,
            },
            objects: vec![],
        },
        raw: Vec::new(),
    };
    let vram = Vram::new();
    let mesh = rtmd.build_filtered_vram_mesh(&vram);
    assert!(mesh.indices.is_empty());
}

#[test]
fn build_uploads_multiple_tims_at_distinct_slots() {
    let scene = make_scene(vec![
        SceneEntry {
            idx: 0,
            class: Class::UnknownOther,
            bytes: Arc::new(synth_tim_16bpp(0, 0)),
        },
        SceneEntry {
            idx: 1,
            class: Class::UnknownOther,
            bytes: Arc::new(synth_tim_16bpp(128, 128)),
        },
    ]);
    let res = SceneResources::build(&scene).unwrap();
    assert_eq!(res.tim_count, 2);
    assert_eq!(res.shared_tim_count, 0);
    assert_eq!(res.vram.pixel(0, 0), 0xAAAA);
    assert_eq!(res.vram.pixel(128, 128), 0xAAAA);
}

/// Shared CDNAME blocks (e.g. `init_data`, `player_data`) must
/// populate VRAM at slots the per-scene block doesn't touch - this
/// proves the order is shared-first then scene, so the scene wins
/// any slot collision, but non-overlapping shared slots survive.
#[test]
fn build_with_shared_uploads_shared_tims_into_distinct_slots() {
    let scene_only = make_scene(vec![SceneEntry {
        idx: 0,
        class: Class::UnknownOther,
        bytes: Arc::new(synth_tim_16bpp(64, 0)),
    }]);
    let shared = make_scene(vec![SceneEntry {
        idx: 100,
        class: Class::UnknownOther,
        bytes: Arc::new(synth_tim_16bpp(768, 0)),
    }]);
    let res = SceneResources::build_with_shared(&scene_only, &[&shared]).unwrap();
    // 1 shared TIM + 1 scene TIM = 2 total
    assert_eq!(res.tim_count, 2);
    assert_eq!(res.shared_tim_count, 1);
    // Shared TIM landed at (768, 0); scene TIM at (64, 0).
    assert_eq!(res.vram.pixel(768, 0), 0xAAAA);
    assert_eq!(res.vram.pixel(64, 0), 0xAAAA);
}

/// Scene-local TIMs win when the shared and scene blocks both target
/// the same VRAM slot. Mirrors the retail boot-then-scene ordering:
/// init_data lays down the boot atlas, then a scene-specific TIM at
/// the same slot overwrites it.
#[test]
fn build_with_shared_scene_wins_overlapping_slots() {
    // Shared writes 0xAAAA at (0, 0); scene writes 0x5555 at (0, 0).
    // Build two distinct 1px TIMs that target the same slot.
    fn pixel_at(x: u16, y: u16, color: u16) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&0x10u32.to_le_bytes());
        b.extend_from_slice(&0x02u32.to_le_bytes());
        b.extend_from_slice(&20u32.to_le_bytes());
        b.extend_from_slice(&x.to_le_bytes());
        b.extend_from_slice(&y.to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        for _ in 0..4 {
            b.extend_from_slice(&color.to_le_bytes());
        }
        b
    }
    let shared = make_scene(vec![SceneEntry {
        idx: 100,
        class: Class::UnknownOther,
        bytes: Arc::new(pixel_at(0, 0, 0xAAAA)),
    }]);
    let scene_only = make_scene(vec![SceneEntry {
        idx: 0,
        class: Class::UnknownOther,
        bytes: Arc::new(pixel_at(0, 0, 0x5555)),
    }]);
    let res = SceneResources::build_with_shared(&scene_only, &[&shared]).unwrap();
    assert_eq!(
        res.vram.pixel(0, 0),
        0x5555,
        "scene must win slot collision"
    );
}

#[test]
fn build_battle_boot_vram_empty_when_no_battle_data() {
    // Synthesize a scene that doesn't carry a battle_data pack -
    // the boot builder must return an empty stats and a fresh
    // VRAM, never crash.
    let scene = make_scene(vec![SceneEntry {
        idx: 0,
        class: Class::UnknownOther,
        bytes: Arc::new(synth_tim_16bpp(0, 0)),
    }]);
    let (vram, stats) = SceneResources::build_battle_boot_vram(&[&scene]);
    assert_eq!(stats.packs, 0);
    assert_eq!(stats.records, 0);
    assert_eq!(stats.tims_uploaded, 0);
    assert_eq!(stats.cluts_uploaded, 0);
    // VRAM is fresh / zero everywhere.
    assert_eq!(vram.pixel(0, 0), 0);
}

#[test]
fn build_battle_boot_vram_no_scenes() {
    let (vram, stats) = SceneResources::build_battle_boot_vram(&[]);
    assert_eq!(stats.packs, 0);
    assert_eq!(vram.pixel(0, 0), 0);
}
