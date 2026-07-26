//! Subcommands over two preservation modules that had no way to be invoked:
//! the boot-time overlay resolver and the slot machine's 3D scene maths.

use std::path::Path;

use anyhow::{Context, Result};
use legaia_asset::boot_overlay as boot;
use legaia_asset::minigame_art;
use legaia_asset::minigame_slot_scene as slot;

/// Print the boot path's overlay / side-band resolution table.
///
/// Every row is a *decision* the SCUS boot path makes about which `PROT.DAT`
/// entry to DMA where. When `prot_dir` is given, each resolved extraction index
/// is looked up in that directory so the row carries the entry it actually
/// names rather than a bare number.
pub(crate) fn boot_overlay_cmd(prot_dir: Option<&Path>) -> Result<()> {
    let names: Vec<String> = match prot_dir {
        None => Vec::new(),
        Some(dir) => {
            let mut v: Vec<String> = std::fs::read_dir(dir)
                .with_context(|| format!("read {}", dir.display()))?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".BIN"))
                .collect();
            v.sort();
            v
        }
    };
    let name_of = |idx: u32| -> String {
        names
            .iter()
            .find(|n| n.starts_with(&format!("{idx:04}_")))
            .cloned()
            .unwrap_or_else(|| "-".into())
    };

    println!(
        "index spaces: extraction = raw - {}, and both overlay loaders bias \
their parameter by {:#x} first",
        boot::RAW_TO_EXTRACTION,
        boot::OVERLAY_PARAM_BIAS
    );
    println!();
    println!("overlay parameter -> entry");
    for param in 0..=8u32 {
        println!(
            "  param {param:2}  raw {:5}  extraction {:5}  {}",
            boot::overlay_param_to_raw(param),
            boot::overlay_param_to_extraction(param),
            name_of(boot::overlay_param_to_extraction(param))
        );
    }

    println!();
    println!("slot-B default overlay (summon-render flag x suppression word)");
    for &suppressed in &[false, true] {
        for &flag in &[false, true] {
            // Ask twice: once with no resident overlay, once with the choice
            // already resident, so the skip-check shows up as a column.
            let fresh = boot::slot_b_default_overlay(flag, suppressed, None);
            let resident = fresh.map(|c| c.param);
            let again = boot::slot_b_default_overlay(flag, suppressed, resident);
            match (fresh, again) {
                (None, _) => {
                    println!("  flag {flag:5}  suppressed {suppressed:5}  -> no load this frame")
                }
                (Some(c), a) => println!(
                    "  flag {flag:5}  suppressed {suppressed:5}  -> param {} = extraction {} {}  \
needs_load {} (already resident: {})",
                    c.param,
                    c.extraction_index,
                    name_of(c.extraction_index),
                    c.needs_load,
                    a.map(|x| x.needs_load).unwrap_or(false),
                ),
            }
        }
    }

    println!();
    println!("effect-data side band (the dev-station flag picks the branch)");
    for &dev in &[false, true] {
        println!("  dev_flag {dev:5}  -> {:?}", boot::effect_data_source(dev));
    }
    println!(
        "  the disc branch is extraction {} {} at buffer offset {:#x}",
        boot::EFFECT_DATA_EXTRACTION_INDEX,
        name_of(boot::EFFECT_DATA_EXTRACTION_INDEX),
        boot::EFFECT_DATA_BUFFER_OFFSET
    );

    println!();
    println!(
        "CARD-mode TIM pack: extraction {} {}, scratch buffer {:#x} bytes = {} sectors",
        boot::CARD_TIM_EXTRACTION_INDEX,
        name_of(boot::CARD_TIM_EXTRACTION_INDEX),
        boot::CARD_TIM_BUFFER_LEN,
        boot::bytes_to_sectors(boot::CARD_TIM_BUFFER_LEN as i32)
    );
    println!(
        "sector rounding ({:#x} bytes/sector, round *toward zero* on negatives so an \
error return stays one):",
        boot::SECTOR_BYTES
    );
    for &b in &[0i32, 1, 0x7FF, 0x800, 0x801, -1, -0x800, -0x1000] {
        println!("  {b:>8} bytes -> {:>5} sectors", boot::bytes_to_sectors(b));
    }
    Ok(())
}

/// Print the slot machine's reel cylinder + dot-matrix marquee, from the
/// **fixed-point** kernels.
///
/// The browser play page redoes the same maths in floating point off this
/// module's constants, so this is the reference that difference can be measured
/// against rather than assumed away.
pub(crate) fn slot_scene_cmd(overlay: &Path, art: Option<&Path>, frames: usize) -> Result<()> {
    let ov = std::fs::read(overlay).with_context(|| format!("read {}", overlay.display()))?;

    println!("overlay: {} ({} bytes)", overlay.display(), ov.len());
    println!(
        "reel cylinder: {} reels x {} faces, radius {:#x}, z >> {}, shade clamp {:#x}",
        slot::REEL_COUNT,
        slot::REEL_FACES,
        slot::REEL_Y_RADIUS,
        slot::REEL_Z_SHIFT,
        slot::REEL_SHADE_MAX
    );
    println!("  face   angle      y       z   shade");
    for face in 0..slot::REEL_FACES {
        let angle = slot::REEL_ANGLE_BASE + face as i32 * slot::REEL_ANGLE_STEP;
        let z = slot::reel_z(angle);
        println!(
            "  {face:4}   {angle:#06x}  {:5}  {z:6}   {:5}",
            slot::reel_y(angle),
            slot::reel_shade(z)
        );
    }

    let Some(art_path) = art else {
        println!("(pass --art <PROT 1200 entry> for the dot-matrix marquee)");
        return Ok(());
    };
    let art_raw =
        std::fs::read(art_path).with_context(|| format!("read {}", art_path.display()))?;
    let tims = minigame_art::parse_art_pack(&art_raw).context("parse the slot art pack")?;
    let (page, w, _h) = minigame_art::slot_page_indices(&tims, slot::DOT_PAGE)
        .context("locate the dot-matrix art page")?;
    let scene = slot::parse_scene(&ov, &page, w).context("parse the slot scene")?;

    println!();
    println!(
        "dot-matrix message bank: {} record(s)",
        scene.messages.len()
    );
    for (i, m) in scene.messages.iter().enumerate() {
        let lit = m.bitmap.iter().filter(|n| **n != 0).count();
        println!(
            "  msg {i:2}  {}x{} texels at page-3 ({}, {})  {lit} lit dot(s)",
            m.w, m.h, m.u, m.v
        );
    }

    // The per-frame composer, over a small tour of the six overlay globals it
    // reads. `clear_dots` is the empty buffer every frame starts from.
    let blank = slot::clear_dots();
    println!();
    println!("dot buffer: {} bytes, all clear", blank.len());
    println!("per-frame composition:");
    let tour = [
        slot::MarqueeFrame::default(),
        slot::MarqueeFrame {
            payout: 1234,
            payout_frame: 1,
            ..Default::default()
        },
        slot::MarqueeFrame {
            payout: 1234,
            payout_frame: 20,
            ..Default::default()
        },
        slot::MarqueeFrame {
            payout: 7,
            payout_frame: 20,
            ..Default::default()
        },
        slot::MarqueeFrame {
            feature_mode: 5,
            bonus_rounds: 2,
            claimed: [1, 0, 0],
            ..Default::default()
        },
    ];
    for f in tour.iter().take(frames.max(1)) {
        let placed = slot::compose_marquee_frame(f);
        println!(
            "  payout {:5} frame {:3} mode {} rounds {} claimed {:?} -> {} placement(s)",
            f.payout,
            f.payout_frame,
            f.feature_mode,
            f.bonus_rounds,
            f.claimed,
            placed.len()
        );
        for p in &placed {
            let Some(msg) = scene.messages.get(p.msg) else {
                println!(
                    "    msg {} col {} row {}  (out of bank)",
                    p.msg, p.col, p.row
                );
                continue;
            };
            let buf = slot::compose_marquee(msg, p.col, p.row);
            let lit = buf.iter().filter(|n| **n != 0).count();
            println!(
                "    msg {:2} col {:3} row {:4}  blits {lit} dot(s)",
                p.msg, p.col, p.row
            );
        }
        // The whole-strip rasterisation the placements feed, so the composed
        // and the placed views can be compared on one frame.
        let strip = slot::render_marquee(&placed, &scene.messages);
        println!(
            "    rasterised strip: {} of {} dots lit",
            strip.iter().filter(|n| **n != 0).count(),
            strip.len()
        );
    }

    // `place_message` is the in-place blit the composer wraps; exercise it on
    // its own so the two agree.
    if let Some(msg) = scene.messages.first() {
        let mut buf = slot::clear_dots();
        slot::place_message(&mut buf, msg, 0, 0);
        println!();
        println!(
            "place_message(msg 0, 0, 0) lights {} dot(s) in a {}-byte buffer",
            buf.iter().filter(|n| **n != 0).count(),
            buf.len()
        );
    }
    Ok(())
}
