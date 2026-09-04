//! Subcommands over two preservation modules that had no way to be invoked:
//! the boot-time overlay resolver and the slot machine's 3D scene maths.

use std::path::Path;

use anyhow::{Context, Result};
use legaia_asset::boot_overlay as boot;
use legaia_asset::minigame_art;
use legaia_asset::minigame_slot_scene as slot;

use crate::common::write_rgba_png;

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

/// Crop a rect out of a decoded page sprite (row-major RGBA8).
fn crop_rgba(page: &minigame_art::Sprite, x: usize, y: usize, w: usize, h: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(w * h * 4);
    for row in 0..h {
        for col in 0..w {
            let (px, py) = (x + col, y + row);
            if px < page.width && py < page.height {
                let o = (py * page.width + px) * 4;
                out.extend_from_slice(&page.rgba[o..o + 4]);
            } else {
                out.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    out
}

/// Export the slot machine's art + data as engine-external files (PNG + JSON) -
/// the feed for the VRChat world kit's cabinet builder
/// (`scripts/vrchat-world/`), or any other host that wants the machine's
/// sprites as plain images.
///
/// Everything written is decoded from the *user's* extracted disc data
/// (PROT 0975 overlay + PROT 1200 art pack) - the output directory is
/// Sony-derived and must stay out of the repo, like `extracted/` itself.
pub(crate) fn slot_art_cmd(overlay: &Path, art: &Path, out: &Path) -> Result<()> {
    use legaia_asset::slot_payout;

    let ov = std::fs::read(overlay).with_context(|| format!("read {}", overlay.display()))?;
    let art_raw = std::fs::read(art).with_context(|| format!("read {}", art.display()))?;
    let tims = minigame_art::parse_art_pack(&art_raw).context("parse the slot art pack")?;

    let payouts = slot_payout::parse(&ov).context("parse the slot payout table")?;
    let (page3_idx, page3_w, _) = minigame_art::slot_page_indices(&tims, slot::DOT_PAGE)
        .context("locate the dot-matrix art page")?;
    let scene = slot::parse_scene(&ov, &page3_idx, page3_w).context("parse the slot scene")?;

    let dir = |sub: &str| -> Result<std::path::PathBuf> {
        let d = out.join(sub);
        std::fs::create_dir_all(&d).with_context(|| format!("create {}", d.display()))?;
        Ok(d)
    };
    let save = |path: &Path, sprite: &minigame_art::Sprite| -> Result<()> {
        write_rgba_png(
            path,
            sprite.width as u32,
            sprite.height as u32,
            &sprite.rgba,
        )
    };
    let mut written = 0usize;

    // Reel faces: the ten symbols and the ten bonus numerals, each through its
    // own CLUT column (the palette is load-bearing - see minigame_art).
    let symbols_dir = dir("symbols")?;
    for sym in 0..minigame_art::SLOT_SYMBOL_COUNT {
        let s = minigame_art::slot_symbol(&tims, sym)?;
        save(&symbols_dir.join(format!("symbol_{sym}.png")), &s)?;
        written += 1;
    }
    for n in 1..=minigame_art::SLOT_BONUS_NUMBER_COUNT {
        let s = minigame_art::slot_bonus_number(&tims, n)?;
        save(&symbols_dir.join(format!("numeral_{n}.png")), &s)?;
        written += 1;
    }

    // The glass furniture: payline lamps (lit/unlit), the five medallions
    // (one cell of art, each record's own CLUT column), the reel-stop
    // pedestals (per-reel spinning/stopped palette + cell), and the marquee
    // panel + mascots (per-record cell + CLUT).
    let furn_dir = dir("furniture")?;
    let lamp_page = minigame_art::slot_page(
        &tims,
        slot::LAMP_PAGE,
        minigame_art::ClutId(slot::LAMP_CLUT).palette_index(),
    )?;
    for (name, cell) in [
        ("lamp_lit", slot::LAMP_CELL_LIT),
        ("lamp_unlit", slot::LAMP_CELL_UNLIT),
    ] {
        let (u, v, w, h) = (cell.0 as usize, cell.1 as usize, cell.2, cell.3);
        let rgba = crop_rgba(&lamp_page, u, v, w as usize, h as usize);
        write_rgba_png(
            &furn_dir.join(format!("{name}.png")),
            w as u32,
            h as u32,
            &rgba,
        )?;
        written += 1;
    }
    for (i, m) in scene.medallions.iter().enumerate() {
        let page = minigame_art::slot_page(
            &tims,
            slot::MEDALLION_PAGE,
            slot::medallion_clut(m.art).palette_index(),
        )?;
        let (u, v, w, h) = (
            slot::MEDALLION_CELL.0 as usize,
            slot::MEDALLION_CELL.1 as usize,
            slot::MEDALLION_CELL.2 as usize,
            slot::MEDALLION_CELL.3 as usize,
        );
        let rgba = crop_rgba(&page, u, v, w, h);
        write_rgba_png(
            &furn_dir.join(format!("medallion_{i}.png")),
            w as u32,
            h as u32,
            &rgba,
        )?;
        written += 1;
    }
    for reel in 0..slot::REEL_COUNT {
        for (tag, stopped) in [("spin", false), ("stop", true)] {
            let clut = if stopped {
                slot::PEDESTAL_CLUT_STOPPED
            } else {
                slot::PEDESTAL_CLUT_SPINNING
            } + reel as u16;
            let page = minigame_art::slot_page(
                &tims,
                slot::PEDESTAL_PAGE,
                minigame_art::ClutId(clut).palette_index(),
            )?;
            let (u, v, w, h) = slot::pedestal_cell(reel, stopped);
            let rgba = crop_rgba(&page, u as usize, v as usize, w as usize, h as usize);
            write_rgba_png(
                &furn_dir.join(format!("pedestal_{reel}_{tag}.png")),
                w as u32,
                h as u32,
                &rgba,
            )?;
            written += 1;
        }
    }
    for (i, b) in scene.marquee.iter().enumerate() {
        let page = minigame_art::slot_page(
            &tims,
            slot::MARQUEE_PAGE,
            minigame_art::ClutId(slot::MARQUEE_CLUT_BASE.wrapping_add(b.clut_off as u16))
                .palette_index(),
        )?;
        let rgba = crop_rgba(
            &page,
            b.u as usize,
            b.v as usize,
            b.w as usize,
            b.h as usize,
        );
        write_rgba_png(
            &furn_dir.join(format!("marquee_{i}.png")),
            b.w as u32,
            b.h as u32,
            &rgba,
        )?;
        written += 1;
    }

    // The dot-matrix message bank, rendered through the two blink palettes'
    // lamp swatches: a nibble `n` lights a dot with the swatch at page-3
    // `u = n * 4` (nibble 0 is an unlit dot -> transparent).
    let msg_dir = dir("marquee")?;
    for (tag, &pal) in ["a", "b"].iter().zip(slot::DOT_BLINK_PALETTES.iter()) {
        let swatch_page = minigame_art::slot_page(&tims, slot::DOT_PAGE, pal)?;
        let swatch = |n: u8| -> [u8; 4] {
            let x = (n as usize) * slot::DOT_U_PER_NIBBLE as usize;
            if n == 0 || x >= swatch_page.width {
                return [0, 0, 0, 0];
            }
            let o = x * 4;
            [
                swatch_page.rgba[o],
                swatch_page.rgba[o + 1],
                swatch_page.rgba[o + 2],
                255,
            ]
        };
        for (i, m) in scene.messages.iter().enumerate() {
            let mut rgba = Vec::with_capacity(m.bitmap.len() * 4);
            for &n in &m.bitmap {
                rgba.extend_from_slice(&swatch(n));
            }
            write_rgba_png(
                &msg_dir.join(format!("msg_{i:02}_{tag}.png")),
                m.w as u32,
                m.h as u32,
                &rgba,
            )?;
            written += 1;
        }
    }

    // The screen-space HUD pieces: the 8bpp paytable board, the coin digit
    // strip and the cash-out cursor (HUD record 2).
    let hud_dir = dir("hud")?;
    save(
        &hud_dir.join("paytable.png"),
        &minigame_art::slot_info_panel(&tims)?,
    )?;
    save(
        &hud_dir.join("digits.png"),
        &minigame_art::slot_digit_strip(&tims)?,
    )?;
    let hud = minigame_art::parse_slot_hud(&ov)?;
    if let Some(cursor) = hud.get(2) {
        save(
            &hud_dir.join("cursor.png"),
            &minigame_art::slot_hud_sprite(&tims, cursor)?,
        )?;
    }
    written += 3;

    // The manifest: the payout table plus the geometry / layout constants a
    // cabinet builder needs, in the disc's own units.
    let pos = |p: &slot::Pos3| serde_json::json!({ "x": p.x, "y": p.y, "z": p.z });
    let manifest = serde_json::json!({
        "payouts": (0..slot_payout::SLOT_SYMBOL_COUNT as u8)
            .map(|s| payouts.payout(s).unwrap_or(0))
            .collect::<Vec<u8>>(),
        "kick_symbol": slot_payout::KICK_SYMBOL_ID,
        "kick_rounds": slot_payout::KICK_BONUS_ROUNDS,
        "punch_symbol": slot_payout::PUNCH_SYMBOL_ID,
        "punch_rounds": slot_payout::PUNCH_BONUS_ROUNDS,
        "strip_len": slot::STRIP_LEN,
        "reel_x0": slot::REEL_X0,
        "reel_x_step": slot::REEL_X_STEP,
        "reel_width": slot::REEL_WIDTH,
        "reel_y_radius": slot::REEL_Y_RADIUS,
        "reel_z_radius": slot::ANGLE_FULL >> slot::REEL_Z_SHIFT,
        "reel_faces": slot::REEL_FACES,
        "reel_angle_base": slot::REEL_ANGLE_BASE,
        "reel_angle_step": slot::REEL_ANGLE_STEP,
        "angle_full": slot::ANGLE_FULL,
        "reel_shade_max": slot::REEL_SHADE_MAX,
        "reel_shade_bias": slot::REEL_SHADE_Z_BIAS,
        "reel_shade_gain": slot::REEL_SHADE_Z_GAIN,
        "glass_z": slot::GLASS_Z,
        "pedestal_x0": slot::PEDESTAL_X0,
        "pedestal_x_step": slot::PEDESTAL_X_STEP,
        "pedestal_y": slot::PEDESTAL_Y,
        "pedestal_half_w": slot::PEDESTAL_HALF.0,
        "pedestal_half_h": slot::PEDESTAL_HALF.1,
        "lamp_half_w": slot::LAMP_HALF.0,
        "lamp_half_h": slot::LAMP_HALF.1,
        "medallion_half_w": slot::MEDALLION_HALF.0,
        "medallion_half_h": slot::MEDALLION_HALF.1,
        "dot_cols": slot::DOT_COLS,
        "dot_rows": slot::DOT_ROWS,
        "dot_x0": slot::DOT_X0,
        "dot_y0": slot::DOT_Y0,
        "dot_x_step": slot::DOT_X_STEP,
        "dot_y_step": slot::DOT_Y_STEP,
        "msg_number_base": slot::MSG_NUMBER_BASE,
        "msg_times": slot::MSG_TIMES,
        "msg_pip_on": slot::MSG_ROUND_PIP_ON,
        "msg_pip_off": slot::MSG_ROUND_PIP_OFF,
        "msg_coins": slot::MSG_COINS,
        "tally_number_cols": slot::TALLY_NUMBER_COLS,
        "tally_times_cols": slot::TALLY_TIMES_COLS,
        "payout_digit_cols": slot::PAYOUT_DIGIT_COLS,
        "payout_coins_col": slot::PAYOUT_COINS_COL,
        "round_pip_cols": slot::ROUND_PIP_COLS,
        "messages": scene.messages.iter().enumerate()
            .map(|(i, m)| serde_json::json!({ "id": i, "w": m.w, "h": m.h }))
            .collect::<Vec<_>>(),
        "paylines": scene.paylines.iter()
            .map(|l| serde_json::json!({ "a": pos(&l.a), "b": pos(&l.b) }))
            .collect::<Vec<_>>(),
        "lamps": scene.lamps.iter().map(|l| pos(&l.pos)).collect::<Vec<_>>(),
        "medallions": scene.medallions.iter()
            .map(|m| serde_json::json!({ "pos": pos(&m.pos), "art": m.art }))
            .collect::<Vec<_>>(),
        "marquee": scene.marquee.iter()
            .map(|b| serde_json::json!({
                "pos": pos(&b.pos), "half_w": b.half_w, "half_h": b.half_h,
            }))
            .collect::<Vec<_>>(),
    });
    let manifest_path = out.join("slot-machine.json");
    std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)
        .with_context(|| format!("write {}", manifest_path.display()))?;

    println!(
        "slot art: {written} PNGs + slot-machine.json -> {}",
        out.display()
    );
    println!("(decoded from your disc's data - keep the output out of any repo)");
    Ok(())
}
