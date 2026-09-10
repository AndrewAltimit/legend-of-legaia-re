//! The PROT-0900 **screen-effect widget family** end to end: the field VM's op
//! `0x43` spawn sub-ops, the widget tick, and the primitive list a renderer
//! draws.
//!
//! Four widget kinds live on the effect-actor list, each with its own per-frame
//! handler and its own ordering-table slot:
//!
//! | kind | handler | OT slot |
//! |---|---|---|
//! | letterbox | `FUN_801F8A34` | `+0x04` |
//! | sprite | `FUN_801F7A9C` | `+0x0c` |
//! | panel | `FUN_801F849C` | `+0x10` |
//! | mask | `FUN_801F811C` | `+0x1c` |
//!
//! The slots are read off the handlers' own `func_0x8003d2c4(_DAT_1F8003F4 + N,
//! packet)` calls (`ghidra/scripts/funcs/overlay_dance_801f8a34.txt` and
//! siblings), and a **larger** slot is farther, so the paint order back to front
//! is mask, panel, sprite, letterbox. That ordering is the load-bearing part:
//! the mask's borders and the letterbox's bands are both flat black quads, so a
//! renderer that batches them together draws the letterbox behind every sprite
//! the same scene spawns - at the opposite end of the table from where retail
//! links it.
//!
//! Where they are driven from, measured rather than assumed: decoding every
//! partition record of every MAN carrier of every CDNAME scene confines the
//! whole family to the **ending** scenes (`ed*`) - and to ten of them, not the
//! eight `screen_fx`'s module doc says. The disc-gated test below prints the
//! per-sub-op scene lists it measured.

use legaia_engine_core::screen_fx::{OT_LETTERBOX, OT_MASK, OT_PANEL, OT_SPRITE, ScreenFxQuad};
use legaia_engine_core::world::{SceneMode, World};

/// The five op-`0x43` sub-ops that spawn or drive a widget, with the
/// instruction length each one's decoder consumes.
const SPRITE: u8 = 0x10;
const MASK: u8 = 0x11;
const PANEL: u8 = 0x13;
const PANEL_MOVE: u8 = 0x14;
const LETTERBOX: u8 = 0x15;

fn field_world(script: Vec<u8>) -> World {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.field_frame_step = 1;
    world.load_field_script(script);
    world
}

/// `43 11 [l][t][r][b][dur]` - the 12-byte mask-rect instruction.
fn mask_rect(l: i16, t: i16, r: i16, b: i16, dur: i16) -> Vec<u8> {
    let mut v = vec![0x43, MASK];
    for w in [l, t, r, b, dur] {
        v.extend_from_slice(&w.to_le_bytes());
    }
    v
}

/// `43 15 [x_left][x_right][y0][y1][y2][y3]` - the 14-byte letterbox config.
fn letterbox(bands: [i16; 6]) -> Vec<u8> {
    let mut v = vec![0x43, LETTERBOX];
    for w in bands {
        v.extend_from_slice(&w.to_le_bytes());
    }
    v
}

fn ot_of(q: &ScreenFxQuad) -> u32 {
    match q {
        ScreenFxQuad::Flat { ot, .. } | ScreenFxQuad::Textured { ot, .. } => *ot,
    }
}

/// The mask widget reaches a draw list through the real VM, and its four
/// borders link at the mask slot.
#[test]
fn op_43_11_spawns_the_iris_mask_and_it_draws_four_borders() {
    // A rect well inside a 320x240 screen, snapped (`dur` 0).
    let mut world = field_world(mask_rect(80, 60, 240, 180, 0));
    for _ in 0..4 {
        let _ = world.tick();
    }
    assert!(world.screen_fx.mask.is_some(), "the sub-op must spawn it");
    let quads = world.screen_fx_frame.draw_quads();
    assert_eq!(quads.len(), 4, "four border bands: {quads:?}");
    assert!(quads.iter().all(|q| ot_of(q) == OT_MASK));
    // Every border is flat black, and none of them is degenerate.
    for q in &quads {
        let ScreenFxQuad::Flat { xy, rgba, .. } = q else {
            panic!("mask borders are untextured: {q:?}");
        };
        assert_eq!(*rgba, [0, 0, 0, 255]);
        assert!(xy[1].0 > xy[0].0 && xy[2].1 > xy[0].1, "degenerate {xy:?}");
    }
}

/// The letterbox reaches the same list, and - the regression this pins - it
/// links at the FRONT slot rather than sharing the mask's batch.
#[test]
fn op_43_15_letterbox_links_in_front_of_the_mask_not_beside_it() {
    let mut script = mask_rect(0, 0, 320, 224, 0);
    script.extend(letterbox([0, 320, 40, 56, 168, 184]));
    let mut world = field_world(script);
    for _ in 0..4 {
        let _ = world.tick();
    }
    assert!(world.screen_fx.letterbox.is_some());
    let quads = world.screen_fx_frame.draw_quads();
    let band_ots: Vec<u32> = quads.iter().map(ot_of).collect();
    assert!(
        band_ots.contains(&OT_MASK) && band_ots.contains(&OT_LETTERBOX),
        "both families must draw: {band_ots:?}"
    );
    const _: () = assert!(
        OT_LETTERBOX < OT_MASK,
        "a smaller slot is nearer, so the bands cover the mask, not vice versa"
    );
    // The emitter hands them back back-to-front, so every mask quad precedes
    // every letterbox quad.
    let last_mask = band_ots.iter().rposition(|&o| o == OT_MASK).unwrap();
    let first_lb = band_ots.iter().position(|&o| o == OT_LETTERBOX).unwrap();
    assert!(last_mask < first_lb, "emitted out of order: {band_ots:?}");
    // Two solid bands plus two gradient feathers, and the feathers are the
    // subtractive gouraud quads that were dropped entirely before.
    let feathers: Vec<&ScreenFxQuad> = quads
        .iter()
        .filter(|q| {
            matches!(
                q,
                ScreenFxQuad::Flat {
                    gouraud: Some(_),
                    semi_transparent: true,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(feathers.len(), 2, "both feather strips must draw");
    let ScreenFxQuad::Flat {
        gouraud: Some(g),
        abr_mode,
        ..
    } = feathers[0]
    else {
        unreachable!()
    };
    assert_eq!(
        *abr_mode,
        legaia_engine_core::screen_fx::ABR_SUBTRACTIVE,
        "the white edge subtracts to black"
    );
    assert_ne!(g[0], g[2], "top and bottom edges differing is the gradient");
}

/// The four kinds are ordered back-to-front by their retail slots, whatever
/// order the scripts spawned them in.
#[test]
fn the_four_kinds_paint_in_retail_ordering_table_order() {
    const _: () = assert!(OT_MASK > OT_PANEL);
    const _: () = assert!(OT_PANEL > OT_SPRITE);
    const _: () = assert!(OT_SPRITE > OT_LETTERBOX);
    // Spawn the letterbox FIRST and the mask last: the emitted order must
    // still be mask-then-letterbox, i.e. it follows the slot and not the
    // script.
    let mut script = letterbox([0, 320, 40, 56, 168, 184]);
    script.extend(mask_rect(0, 0, 320, 224, 0));
    let mut world = field_world(script);
    for _ in 0..6 {
        let _ = world.tick();
    }
    let ots: Vec<u32> = world
        .screen_fx_frame
        .draw_quads()
        .iter()
        .map(ot_of)
        .collect();
    assert!(!ots.is_empty());
    assert!(
        ots.windows(2).all(|w| w[0] >= w[1]),
        "the list must be sorted farthest-first: {ots:?}"
    );
}

/// A frame with no live widget produces no primitives at all - the
/// short-circuit both hosts skip their whole pass on.
#[test]
fn an_idle_scene_emits_nothing() {
    let mut world = field_world(vec![0x00]);
    for _ in 0..4 {
        let _ = world.tick();
    }
    assert!(!world.screen_fx.is_active());
    assert!(world.screen_fx_frame.draw_quads().is_empty());
}

// ---------------------------------------------------------------------------
// Disc-gated: which scenes actually carry these sub-ops
// ---------------------------------------------------------------------------

mod on_disc {
    use super::*;
    use legaia_asset::field_disasm::{InsnInfo, LinearWalker};
    use legaia_engine_core::man_field_scripts::{partition_record_span, scene_man_carriers};
    use legaia_engine_core::scene::{ProtIndex, Scene};
    use std::path::PathBuf;
    use std::sync::Arc;

    fn extracted_dir() -> Option<PathBuf> {
        for c in ["extracted", "../extracted", "../../extracted"] {
            let d = PathBuf::from(c);
            if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
                return Some(d);
            }
        }
        None
    }

    fn gate() -> Option<Arc<ProtIndex>> {
        if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
            eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
            return None;
        }
        let extracted = extracted_dir().or_else(|| {
            eprintln!("[skip] extracted/ missing");
            None
        })?;
        Some(Arc::new(
            ProtIndex::open_extracted(&extracted).expect("open prot index"),
        ))
    }

    /// Walk every MAN carrier of every CDNAME scene, decode every record of
    /// every partition as field-VM bytecode, and collect the `(scene, sub_op,
    /// script)` sites of op `0x43`'s widget sub-ops.
    ///
    /// This is a **decoded** census, not a byte-pair scan: an operand byte that
    /// happens to read `0x43` is not an instruction, and counting those is how
    /// a census invents scenes that carry nothing.
    #[allow(clippy::type_complexity)]
    fn widget_sites(index: &ProtIndex) -> Vec<(String, u8, Vec<u8>, usize, usize, usize)> {
        let mut out = Vec::new();
        for name in index.cdname_scene_names() {
            let Ok(scene) = Scene::load(index, &name) else {
                continue;
            };
            for carrier in scene_man_carriers(index, &scene) {
                let Ok(man_file) = legaia_asset::man_section::parse(&carrier.payload) else {
                    continue;
                };
                for (partition, count) in man_file.header.partition_counts.iter().enumerate() {
                    for record in 0..*count as usize {
                        let Some((start, pc0, len)) =
                            partition_record_span(&man_file, &carrier.payload, partition, record)
                        else {
                            continue;
                        };
                        let body = &carrier.payload[start..start + len];
                        for insn in LinearWalker::new(body, pc0).flatten() {
                            // The op-0x43 sub-op is the DECODER's, not
                            // `Insn::extended`: the lead byte's `0x80` is the
                            // cross-context target marker, so `extended` holds
                            // the target byte for a `0xC3 ..` form and reading
                            // it as the sub-op invents sites (`C3 11` is not
                            // `43 11`).
                            let InsnInfo::ActorCtrl { sub_op, .. } = insn.info else {
                                continue;
                            };
                            if matches!(sub_op, SPRITE | MASK | PANEL | PANEL_MOVE | LETTERBOX) {
                                out.push((
                                    name.clone(),
                                    sub_op,
                                    body.to_vec(),
                                    pc0,
                                    insn.pc,
                                    partition,
                                ));
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// Every widget kind is reachable from a real scene script, and running one
    /// such script through the field VM spawns the widget and draws it.
    #[test]
    fn every_widget_kind_has_a_scene_that_spawns_it() {
        let Some(index) = gate() else { return };
        let sites = widget_sites(&index);
        eprintln!("[ok] {} decoded op-0x43 widget sites on disc", sites.len());
        for sub in [SPRITE, MASK, PANEL, PANEL_MOVE, LETTERBOX] {
            let mut scenes: Vec<(&str, usize)> = sites
                .iter()
                .filter(|(_, s, _, _, _, _)| *s == sub)
                .map(|(n, _, _, _, _, p)| (n.as_str(), *p))
                .collect();
            scenes.sort_unstable();
            scenes.dedup();
            eprintln!(
                "[ok]   sub 0x{sub:02X}: {} scene(s) {scenes:?}",
                scenes.len()
            );
        }
        // Every spawn sub-op is carried by real scene scripts, so the family is
        // driven content and the port has something to draw.
        for sub in [SPRITE, MASK, PANEL, LETTERBOX] {
            assert!(
                sites.iter().any(|(_, s, _, _, _, _)| *s == sub),
                "no scene script spawns widget sub-op 0x{sub:02X}"
            );
        }

        // Now run one real script per spawn kind and assert the widget reaches
        // a draw list, from two entry points:
        //
        // - the record's own `pc0`, which is what the scene host enters at -
        //   the whole ladder, decoder and host hooks included;
        // - the widget instruction's own PC in the same real body, which
        //   isolates the instruction from whatever precedes it, so a record
        //   that later parks on a cross-context yield still proves its operands.
        let mut drew = 0usize;
        for sub in [SPRITE, MASK, PANEL, LETTERBOX] {
            let (scene, _, body, pc0, site_pc, _) = sites
                .iter()
                .find(|(_, s, _, _, _, _)| *s == sub)
                .expect("checked above");
            let run = |start: usize, ticks: usize| {
                let mut world = World::new();
                world.mode = SceneMode::Field;
                world.field_frame_step = 1;
                world.load_field_script_at(body.clone(), start);
                let mut spawned = false;
                for _ in 0..ticks {
                    let _ = world.tick();
                    if world.screen_fx.is_active() {
                        spawned = true;
                        break;
                    }
                }
                (world, spawned)
            };
            let (_, from_entry) = run(*pc0, 4096);
            let (mut world, from_site) = run(*site_pc, 8);
            eprintln!(
                "[ok] sub 0x{sub:02X} in '{scene}': reached from record entry={from_entry}, \
                 from its own pc 0x{site_pc:04X}={from_site}"
            );
            assert!(
                from_site,
                "sub 0x{sub:02X} at '{scene}' pc 0x{site_pc:04X} did not spawn its widget"
            );
            // A live widget must produce primitives within a frame or two (a
            // mask snaps at `dur` 0, a letterbox is static).
            for _ in 0..4 {
                let _ = world.tick();
            }
            assert!(
                !world.screen_fx_frame.draw_quads().is_empty(),
                "sub 0x{sub:02X} spawned in '{scene}' but drew nothing"
            );
            drew += 1;
        }
        assert_eq!(drew, 4, "all four spawn kinds must draw from disc operands");
    }
}
