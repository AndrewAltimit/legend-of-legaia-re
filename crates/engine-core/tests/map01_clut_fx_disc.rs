//! Disc-gated: map01's scripted CLUT-cell effects (field-VM `4C 61`) decode
//! to the pinned row-498 strip-park set, and the engine's ported effect
//! family (`clut_fx` + `World::step_clut_fx`) drives them on the retail
//! vsync clock.
//!
//! What this catches:
//! - The `field_disasm` `4C 61` decode / `scene_clut_cell_fx` scanner
//!   regressing (map01's eight ops stop resolving, or phantom ops appear).
//! - `enter_field_scene` no longer pinning the frame-step factor
//!   (`DAT_1F800393`: overworld 3, field/town 2).
//! - The fade arithmetic drifting off the vsync denomination (a 128-frame
//!   fade must span ~128 vsyncs at any `dt`) or the completion write no
//!   longer landing cell B on the destination.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost};
use legaia_engine_core::world::World;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn disc_gate() -> Option<PathBuf> {
    let extracted = extracted_dir()?;
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    Some(extracted)
}

/// The eight `4C 61` ops in map01's field MAN: four one-shots (`frames = 0`)
/// copying cell `(112, 499)` onto the strip park row `(0/16/32/48, 498)`,
/// and four 128-vsync cross-fades fading those cells back toward
/// `(112, 499)`.
#[test]
fn map01_man_carries_the_eight_row_498_clut_ops() {
    let Some(extracted) = disc_gate() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
    host.load_scene("map01").expect("load map01");
    let man = host
        .scene
        .as_ref()
        .unwrap()
        .field_man_payload(&host.index)
        .expect("man payload")
        .expect("map01 has a field MAN");
    let mf = legaia_asset::man_section::parse(&man).expect("parse map01 MAN");
    let sites = legaia_engine_core::man_field_scripts::scene_clut_cell_fx(&mf, &man);
    assert_eq!(sites.len(), 8, "map01 carries exactly eight 4C 61 ops");
    let one_shots: Vec<_> = sites.iter().filter(|s| s.op.frames == 0).collect();
    let fades: Vec<_> = sites.iter().filter(|s| s.op.frames != 0).collect();
    assert_eq!(one_shots.len(), 4);
    assert_eq!(fades.len(), 4);
    for (k, s) in one_shots.iter().enumerate() {
        assert_eq!(s.op.b, (112, 499), "one-shot source is the strip cell");
        assert_eq!(s.op.dest, (16 * k as i16, 498), "park-row destination");
    }
    for (k, s) in fades.iter().enumerate() {
        assert_eq!(s.op.frames, 128, "fades are 128 vsyncs");
        assert_eq!(s.op.a, (16 * k as i16, 498), "fade A = the park cell");
        assert_eq!(s.op.b, (112, 499), "fade B = the strip cell");
        assert_eq!(s.op.dest, s.op.a, "fade writes back onto the park cell");
    }
}

/// Scene entry pins the frame-step factor: overworld kingdom scenes run at
/// `dt = 3`, field/town scenes at `dt = 2` (live poll baselines of the
/// adaptive `DAT_1F800393` written by `FUN_80016B6C`).
#[test]
fn scene_entry_pins_the_frame_step_factor() {
    let Some(extracted) = disc_gate() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
    host.enter_field_scene("map01", 0).expect("enter map01");
    assert_eq!(host.world.frame_step, 3, "overworld dt = 3");
    host.enter_field_scene("town01", 0).expect("enter town01");
    assert_eq!(host.world.frame_step, 2, "town dt = 2");
}

/// Drive one of map01's real fade ops through the engine kernel on a scratch
/// VRAM: the fade completes in ~128 vsyncs at both dt = 3 (43 game ticks)
/// and dt = 2 (64 game ticks), passes through genuinely interpolated rows,
/// and lands the exact cell-B content on the destination.
#[test]
fn map01_fade_completes_in_128_vsyncs_at_either_frame_step() {
    let Some(extracted) = disc_gate() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
    host.load_scene("map01").expect("load map01");
    let man = host
        .scene
        .as_ref()
        .unwrap()
        .field_man_payload(&host.index)
        .expect("man payload")
        .expect("map01 has a field MAN");
    let mf = legaia_asset::man_section::parse(&man).expect("parse map01 MAN");
    let sites = legaia_engine_core::man_field_scripts::scene_clut_cell_fx(&mf, &man);
    let fade = sites
        .iter()
        .find(|s| s.op.frames == 128 && s.op.dest == (0, 498))
        .expect("the (0, 498) fade op");
    // Reconstruct the 14-byte operand payload the VM hook would carry.
    let mut payload = [0u8; 14];
    for (i, v) in [
        fade.op.a.0,
        fade.op.a.1,
        fade.op.b.0,
        fade.op.b.1,
        fade.op.dest.0,
        fade.op.dest.1,
        fade.op.frames,
    ]
    .into_iter()
    .enumerate()
    {
        payload[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
    }

    for (dt, expect_game_ticks) in [(3u8, 43u32), (2, 64)] {
        let mut world = World::new();
        world.frame_step = dt;
        let mut vram = legaia_tim::Vram::new();
        // Seed distinct A / B rows: park cell = a red ramp, strip cell = a
        // blue ramp.
        let seed_row = |vram: &mut legaia_tim::Vram, x: u16, y: u16, f: &dyn Fn(u16) -> u16| {
            let mut bytes = [0u8; 32];
            for i in 0..16u16 {
                bytes[i as usize * 2..i as usize * 2 + 2].copy_from_slice(&f(i).to_le_bytes());
            }
            vram.write_clut_row(x, y, &bytes);
        };
        seed_row(&mut vram, 0, 498, &|i| i + 1); // red ramp 1..=16
        seed_row(&mut vram, 112, 499, &|i| (i + 1) << 10); // blue ramp
        let b_row: Vec<u16> = (0..16).map(|i| vram.pixel(112 + i, 499)).collect();

        world.spawn_clut_cell_fx(&payload);
        assert_eq!(world.clut_fx.len(), 1);

        let mut vsyncs = 0u32;
        let mut game_ticks = 0u32;
        let mut saw_mid_row = false;
        let mut sim_ticks = 0u32;
        while !world.clut_fx.is_empty() {
            sim_ticks += 1;
            assert!(sim_ticks < 1000, "fade never completed (dt={dt})");
            world.tick();
            if world.field_frame_step == 1 {
                vsyncs += 1;
            }
            game_ticks += world.clut_pending_game_ticks;
            if world.step_clut_fx(&mut vram) && !world.clut_fx.is_empty() {
                // Mid-fade write: the destination must be neither pure A
                // nor pure B (a genuinely interpolated row).
                let cur: Vec<u16> = (0..16).map(|i| vram.pixel(i, 498)).collect();
                let a: Vec<u16> = (1..=16).collect();
                if cur != a && cur != b_row {
                    saw_mid_row = true;
                }
            }
        }
        assert_eq!(game_ticks, expect_game_ticks, "dt={dt}");
        // Game tick N lands on vsync N*dt; completion on tick 43/64 =
        // vsync 129/128 (the >= compare overshoots at dt=3).
        assert_eq!(vsyncs, game_ticks * u32::from(dt), "dt={dt}");
        assert!(
            saw_mid_row,
            "fade passed through interpolated rows (dt={dt})"
        );
        let final_row: Vec<u16> = (0..16).map(|i| vram.pixel(i, 498)).collect();
        assert_eq!(
            final_row, b_row,
            "completion lands cell B on the destination (dt={dt})"
        );
    }
}

/// The four one-shot ops apply as immediate cell copies: after one
/// `step_clut_fx` the park cells hold the strip cell's content.
#[test]
fn map01_one_shots_copy_the_strip_cell_immediately() {
    let Some(extracted) = disc_gate() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
    host.load_scene("map01").expect("load map01");
    let man = host
        .scene
        .as_ref()
        .unwrap()
        .field_man_payload(&host.index)
        .expect("man payload")
        .expect("map01 has a field MAN");
    let mf = legaia_asset::man_section::parse(&man).expect("parse map01 MAN");
    let sites = legaia_engine_core::man_field_scripts::scene_clut_cell_fx(&mf, &man);

    let mut world = World::new();
    world.frame_step = 3;
    let mut vram = legaia_tim::Vram::new();
    let mut bytes = [0u8; 32];
    for i in 0..16u16 {
        bytes[i as usize * 2..i as usize * 2 + 2].copy_from_slice(&(0x7C00 | i).to_le_bytes());
    }
    vram.write_clut_row(112, 499, &bytes);
    let strip: Vec<u16> = (0..16).map(|i| vram.pixel(112 + i, 499)).collect();

    for s in sites.iter().filter(|s| s.op.frames == 0) {
        let mut payload = [0u8; 14];
        for (i, v) in [
            s.op.a.0,
            s.op.a.1,
            s.op.b.0,
            s.op.b.1,
            s.op.dest.0,
            s.op.dest.1,
            s.op.frames,
        ]
        .into_iter()
        .enumerate()
        {
            payload[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
        }
        world.spawn_clut_cell_fx(&payload);
    }
    assert_eq!(world.clut_fx.len(), 4);
    assert!(world.step_clut_fx(&mut vram), "one-shots wrote VRAM");
    assert!(world.clut_fx.is_empty(), "one-shots retire immediately");
    for x0 in [0usize, 16, 32, 48] {
        let row: Vec<u16> = (0..16).map(|i| vram.pixel(x0 + i, 498)).collect();
        assert_eq!(row, strip, "park cell ({x0}, 498) holds the strip cell");
    }
}
