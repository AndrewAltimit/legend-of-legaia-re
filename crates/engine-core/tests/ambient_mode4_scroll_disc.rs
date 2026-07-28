//! Render-mode 4 - the ambient VRAM-rect scroller - end to end.
//!
//! The first test is synthetic (runs everywhere, including disc-free CI): a
//! hand-built stager record seats the mode with move-VM op `0x1E` and
//! `World::step_ambient_fx` rotates a software VRAM rect.
//!
//! The rest are disc-gated. They pin, against the real prescript bundles:
//!  - jou's record 23 - the carrier the subsystem doc names - seats the mode
//!    with the operands the disc carries, and its rect really rotates;
//!  - the scene-entry census: which scenes' plain ambient trees put a live
//!    mode-4 part on screen, walked through the actual move VM (a *linear*
//!    scan for the op word mis-reports this - the move VM follows jumps).
//!
//! Skip-pass when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::ambient_effect_installs;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::World;

fn extracted_root() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

/// Paint `(x, y, w, h)` so every row carries a unique non-zero texel - a
/// rotation is then readable straight off the row values.
fn paint_rows(vram: &mut legaia_tim::Vram, x: u16, y: u16, w: u16, h: u16) {
    for row in 0..h {
        let texel = 0x8000u16 | (row + 1);
        let bytes: Vec<u8> = (0..w).flat_map(|_| texel.to_le_bytes()).collect();
        vram.write_block(x, y + row, w, 1, &bytes);
    }
}

/// Paint `(x, y, w, h)` so every *texel* is unique - both rotate axes are
/// then readable off the rect.
fn paint_cells(vram: &mut legaia_tim::Vram, x: u16, y: u16, w: u16, h: u16) {
    for row in 0..h {
        let bytes: Vec<u8> = (0..w)
            .flat_map(|col| (0x8000u16 | (row * w + col + 1)).to_le_bytes())
            .collect();
        vram.write_block(x, y + row, w, 1, &bytes);
    }
}

/// The rect's texels in row-major order.
fn rect_values(vram: &legaia_tim::Vram, x: u16, y: u16, w: u16, h: u16) -> Vec<u16> {
    (0..h)
        .flat_map(|row| (0..w).map(move |col| (usize::from(x + col), usize::from(y + row))))
        .map(|(px, py)| vram.pixel(px, py))
        .collect()
}

/// The per-row texel values currently in `(x, y, w, h)`, one entry per row.
/// Panics if a row is not uniform - which would mean the rotate smeared
/// columns instead of moving whole rows.
fn row_values(vram: &legaia_tim::Vram, x: u16, y: u16, w: u16, h: u16) -> Vec<u16> {
    (0..h)
        .map(|row| {
            let first = vram.pixel(usize::from(x), usize::from(y + row));
            for col in 1..w {
                assert_eq!(
                    vram.pixel(usize::from(x + col), usize::from(y + row)),
                    first,
                    "row {row} is not uniform - a vertical rotate must move whole rows"
                );
            }
            first
        })
        .collect()
}

/// Synthetic: one stager record whose whole program is the op-`0x1E` seat
/// plus an infinite wait - jou record 23's exact shape - drives a cyclic
/// vertical scroll through `World::step_ambient_fx`.
#[test]
fn mode4_seat_rotates_a_vram_rect_through_step_ambient_fx() {
    use legaia_asset::summon_overlay::SummonPart;

    // [i16 model_sel = -1][u16 flags] then: 0x1E period dx dy x y w h,
    // 0x1A 0x4000 (infinite loop latch), 0x09 wait, 0x1B (jump back).
    let words: [u16; 16] = [
        0xFFFF, 0x0000, // transform node, no flags
        0x001E, 0x0003, 0x0002, 0x0001, // period 3, dx 2, dy 1
        0x0020, 0x0010, 0x0008, 0x0004, // rect (32, 16, 8, 4)
        0x001A, 0x4000, 0x0009, 0x0400, 0x001B, 0x0008,
    ];
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();

    let mut world = World {
        frame_step: 2,
        ..Default::default()
    };
    world.field_stager_bytes = bytes.clone();
    world.field_stagers = vec![SummonPart {
        record_off: 0,
        model_sel: -1,
        flags: 0,
        bytecode: 4..bytes.len(),
    }];
    assert!(world.spawn_ambient_record(0, [0, 0, 0]));

    // Op 0x1E seated the mode and the rect (`+0x5A = 4`, `+0xD0..+0xD6`).
    assert_eq!(
        world.active_ambient_scroll_rects(),
        vec![((0x20, 0x10, 8, 4), 2, 1)],
        "op 0x1E seat: rect + per-period steps"
    );

    // 4 rows of 8, every texel distinct, plus a guard row underneath.
    let mut vram = legaia_tim::Vram::new();
    paint_cells(&mut vram, 0x20, 0x10, 8, 4);
    paint_rows(&mut vram, 0x20, 0x14, 8, 1);
    let before = rect_values(&vram, 0x20, 0x10, 8, 4);

    // The seat itself fired one rotate (the spawn-time slice runs the render
    // tail, as the mode-3 sibling does). Period 3 at frame step 2 then fires
    // on ticks 2 and 4 of the next four: 3 fires x (dx*2, dy*2) = (4, 2).
    world.ambient_pending_game_ticks = 4;
    assert!(
        world.step_ambient_fx(&mut vram),
        "the scroller rewrites VRAM texels"
    );
    let after = rect_values(&vram, 0x20, 0x10, 8, 4);
    let mut expect = before.clone();
    for row in 0..4 {
        expect[row * 8..(row + 1) * 8].rotate_left(3 * 4 % 8); // 3 fires x dx*2
    }
    expect.rotate_left((3 * 2 % 4) * 8); // 3 fires x dy*2, in whole rows
    assert_eq!(after, expect, "cyclic left-then-up scroll, both axes");

    // Cyclic: nothing is lost or duplicated, and the row under the rect is
    // untouched (the strip is re-inserted inside the rect, not past it).
    let mut sorted_before = before.clone();
    let mut sorted_after = after.clone();
    sorted_before.sort_unstable();
    sorted_after.sort_unstable();
    assert_eq!(
        sorted_before, sorted_after,
        "rotation preserves every texel"
    );
    assert_eq!(
        vram.pixel(0x20, 0x14),
        0x8000 | 1,
        "the row below the rect is untouched"
    );

    // A part with no mode-4 seat never queues a rotate.
    let mut idle = World::new();
    idle.ambient_pending_game_ticks = 4;
    assert!(!idle.step_ambient_fx(&mut vram), "no parts, no writes");
}

/// Disc-gated: jou's record 23 - "the beam" - seats the scroller with the
/// operands the disc really carries, and stepping the ambient bank rotates
/// that VRAM rect.
#[test]
fn jou_record23_seats_the_scroller_and_rotates_its_rect_or_skip() {
    let Some(root) = extracted_root() else { return };
    let index = ProtIndex::open_extracted(&root).expect("prot index");
    let scene = Scene::load(&index, "jou").expect("load jou");
    let scripts = scene.find_event_scripts().expect("jou event scripts");

    let mut world = World {
        frame_step: 2, // town cadence
        ..Default::default()
    };
    world.install_field_stagers(scripts.bytes);
    // The entry install (arg 0 -> record 1) fans the whole tree out.
    assert!(world.spawn_ambient_record(1, [0, 0, 0]));

    // Exactly one part of jou's tree runs the scroller, with record 23's
    // authored rect and a one-per-period upward step.
    let rect = (0x220u16, 0x80u16, 0x0Eu16, 0x80u16);
    assert_eq!(
        world.active_ambient_scroll_rects(),
        vec![(rect, 0, 1)],
        "jou record 23: the mode-4 seat"
    );

    let (x, y, w, h) = rect;
    let mut vram = legaia_tim::Vram::new();
    paint_rows(&mut vram, x, y, w, h);
    let before = row_values(&vram, x, y, w, h);

    // Period 1 at frame step 2 underflows every tick, so the spawn slice plus
    // three banked ticks are four fires of `dy * frame_step` = 2 rows.
    world.ambient_pending_game_ticks = 3;
    assert!(
        world.step_ambient_fx(&mut vram),
        "jou's scroller rewrites VRAM texels"
    );
    let after = row_values(&vram, x, y, w, h);
    let mut expect = before.clone();
    expect.rotate_left(4 * 2);
    assert_eq!(after, expect, "8 rows of cyclic up-scroll");

    // The rect is animated in place: the column left of it and the row below
    // it stay as they were (untouched VRAM is still zero).
    assert_eq!(vram.pixel(usize::from(x) - 1, usize::from(y)), 0);
    assert_eq!(vram.pixel(usize::from(x), usize::from(y + h)), 0);

    // And it keeps running - the record parks in an infinite `0x1A`/`0x1B`
    // wait loop, so the render tail scrolls forever.
    for _ in 0..200 {
        world.ambient_pending_game_ticks = 1;
        world.step_ambient_fx(&mut vram);
    }
    assert_eq!(
        world.active_ambient_scroll_rects(),
        vec![(rect, 0, 1)],
        "the scroller never retires"
    );
    let late = row_values(&vram, x, y, w, h);
    let mut sorted_before = before.clone();
    let mut sorted_late = late.clone();
    sorted_before.sort_unstable();
    sorted_late.sort_unstable();
    assert_eq!(sorted_before, sorted_late, "still a pure cyclic rotation");
}

/// Disc-gated census: which scenes' plain scene-entry ambient trees put a
/// live mode-4 scroller on screen. Walked through the move VM (spawn the
/// MAN's P1 effect installs, tick, read the seated parts back), which is the
/// only decode that survives the records' jump ops.
#[test]
fn mode4_scene_entry_carrier_census_or_skip() {
    let Some(root) = extracted_root() else { return };
    let index = ProtIndex::open_extracted(&root).expect("prot index");

    type Seat = legaia_engine_core::world::ambient::vram_scroll::ScrollSeat;
    let mut carriers: Vec<(String, Vec<Seat>)> = Vec::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        let Some(scripts) = scene.find_event_scripts() else {
            continue;
        };
        let Ok(Some(man_bytes)) = scene.field_man_payload(&index) else {
            continue;
        };
        let Ok(man) = legaia_asset::man_section::parse(&man_bytes) else {
            continue;
        };
        let installs = ambient_effect_installs(&man, &man_bytes);
        if installs.is_empty() {
            continue;
        }
        let mut world = World {
            frame_step: 2,
            ..Default::default()
        };
        world.install_field_stagers(scripts.bytes);
        for id in &installs {
            world.spawn_ambient_record(*id as usize + 1, [0, 0, 0]);
        }
        for _ in 0..4 {
            world.tick_ambient_fx();
        }
        let rects = world.active_ambient_scroll_rects();
        if !rects.is_empty() {
            carriers.push((name, rects));
        }
    }

    let names: Vec<&str> = carriers.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "korout", "koin3", "deroa", "jou", "jouinb", "jouine", "noaru", "other7"
        ],
        "scene-entry mode-4 carriers"
    );

    // Every authored carrier scrolls **vertically only**, upward, and every
    // rect sits in the upper texture band (`x >= 0x200`) - these are the
    // falling-water / energy-column texture strips, not CLUT rows.
    for (name, rects) in &carriers {
        for &((x, _, w, h), dx, dy) in rects {
            assert_eq!(dx, 0, "{name}: no horizontal carrier at scene entry");
            assert!(dy > 0, "{name}: upward step {dy}");
            assert!(x >= 0x200, "{name}: rect x {x:#x} in the texture band");
            // The authored steps always fit their rect at both frame steps -
            // the case retail's `w - strip` descriptor store would wrap.
            assert!(
                w > 0 && i32::from(dy) * 3 < i32::from(h),
                "{name}: strip fits"
            );
        }
    }
    // jouinb carries the most (three separate falls).
    assert_eq!(carriers.iter().map(|(_, r)| r.len()).max(), Some(3));
}
