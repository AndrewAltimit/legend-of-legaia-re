use std::path::Path;

use anyhow::Result;

pub(crate) fn worldmap_menu_cmd(scus: &Path, json: bool) -> Result<()> {
    let bytes = crate::common::read_input(scus)?;
    let menu = legaia_asset::worldmap_menu::parse_scus(&bytes)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&menu)?);
        return Ok(());
    }
    println!(
        "World-map quick-travel menu  ({} names, {} placement records)\n",
        menu.names.len(),
        menu.placements.len(),
    );
    println!("Names (DAT_80073B18, stride 0x20):");
    for (i, name) in menu.names.iter().enumerate() {
        let used = menu.placements.iter().any(|p| (p.name_idx as usize) == i);
        let tag = if used { "  " } else { "* " };
        println!("  {tag}[0x{i:02X}] {name:?}");
    }
    println!("* = not referenced by any placement record (cutscene-only).\n");
    println!(
        "Placements (DAT_80073A98, stride 6; terminator byte0=0xFF):\n  \
         idx flag scene_id  menu_xy   name"
    );
    for p in &menu.placements {
        let name = menu
            .names
            .get(p.name_idx as usize)
            .map(|s| s.as_str())
            .unwrap_or("<?>");
        println!(
            "   {:>2}  0x{:02X}  0x{:04X}   ({:3}, {:3})  {}",
            p.index, p.discovery_flag, p.scene_id, p.menu_x, p.menu_y, name
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn slot4_png_cmd(
    input: Option<&Path>,
    from_raw: Option<&Path>,
    out: &Path,
    placements_path: Option<&Path>,
    kingdom: &str,
    width: u32,
    height: u32,
    margin: u32,
    only_body: Option<usize>,
    frame_body: Option<usize>,
    close_polylines: bool,
    style: &str,
    axes: &str,
) -> Result<()> {
    use legaia_asset::{kingdom_bundle, world_map_overlay};

    if input.is_some() == from_raw.is_some() {
        anyhow::bail!("exactly one of --input or --from-raw is required");
    }
    let mode = match style {
        "row" => world_map_overlay::PolylineMode::RowMajor,
        "col" => world_map_overlay::PolylineMode::ColumnMajor,
        "pairs" => world_map_overlay::PolylineMode::PairWise,
        "grid" => world_map_overlay::PolylineMode::Grid,
        "points" => world_map_overlay::PolylineMode::RowMajor, // mode ignored in points-only path
        other => anyhow::bail!("--style must be row|col|pairs|grid|points (got {other})"),
    };
    let points_only = style == "points";
    let parse_axis = |c: char| match c {
        'x' | 'X' => Ok(world_map_overlay::Axis::X),
        'y' | 'Y' => Ok(world_map_overlay::Axis::Y),
        'z' | 'Z' => Ok(world_map_overlay::Axis::Z),
        other => anyhow::bail!("axis '{other}' must be one of x|y|z"),
    };
    let chars: Vec<char> = axes.chars().collect();
    if chars.len() != 2 {
        anyhow::bail!("--axes must be 2 chars from x|y|z (got '{axes}')");
    }
    let axis_pair = (parse_axis(chars[0])?, parse_axis(chars[1])?);

    // Source the decoded slot-4 bytes from either a kingdom PROT entry or
    // a previously-decoded .bin.
    let decoded: Vec<u8> = if let Some(p) = input {
        let buf = crate::common::read_input(p)?;
        kingdom_bundle::decode_slot(&buf, 4)
            .map_err(|e| anyhow::anyhow!("decode slot 4 from {p:?}: {e}"))?
    } else {
        crate::common::read_input(from_raw.unwrap())?
    };

    let parsed =
        world_map_overlay::parse(&decoded).map_err(|e| anyhow::anyhow!("parse slot 4: {e}"))?;
    println!(
        "Parsed slot 4: {} bodies, {} bytes decoded",
        parsed.bodies.len(),
        decoded.len()
    );

    let opts = world_map_overlay::WireframeOptions {
        close_polylines,
        mode,
        axes: axis_pair,
        ..world_map_overlay::WireframeOptions::default()
    };
    if points_only {
        let pts = world_map_overlay::record_points(&parsed, &opts);
        println!("Record points: {}", pts.len());
    } else {
        let lines = world_map_overlay::top_down_lines(&parsed, &opts);
        println!("Top-down line segments: {}", lines.len());
    }

    let mut raster =
        world_map_overlay::WireframeRaster::new(width, height, margin, [0x0A, 0x0A, 0x1A, 0xFF]);
    let (ah, av) = axis_pair;
    if let Some(b) = frame_body {
        let body = parsed
            .bodies
            .get(b)
            .ok_or_else(|| anyhow::anyhow!("--frame-body {b} out of range"))?;
        let mut amin = i16::MAX;
        let mut bmin = i16::MAX;
        let mut amax = i16::MIN;
        let mut bmax = i16::MIN;
        for r in &body.records {
            if r.x == 0 && r.y == 0 && r.z == 0 {
                continue;
            }
            let a = ah.pick(r);
            let v = av.pick(r);
            amin = amin.min(a);
            bmin = bmin.min(v);
            amax = amax.max(a);
            bmax = bmax.max(v);
        }
        if amin == i16::MAX {
            anyhow::bail!("--frame-body {b} has no non-zero records");
        }
        raster.set_bounds(amin as i32, bmin as i32, amax as i32, bmax as i32);
    } else {
        raster.set_bounds_from_axes(&parsed, ah, av);
    }
    let (amin, bmin, amax, bmax) = raster.world_bounds;
    println!("Camera bounds ({axes}): {amin}..{amax}, {bmin}..{bmax}");

    if points_only {
        raster.draw_points(&parsed, &opts, only_body, 1);
    } else {
        raster.draw_wireframe(&parsed, &opts, only_body);
    }

    if let Some(pp) = placements_path {
        match load_placements(pp, kingdom) {
            Ok(pts) => {
                println!(
                    "Overlaying {} placements for kingdom '{kingdom}'",
                    pts.len()
                );
                // Placement coords use a different scale than slot-4 (placements
                // are in `[0, world_extent]` while slot-4 is in centered ±32K).
                // We map placements into the current camera's bbox so a dot's
                // RELATIVE position within the kingdom carries over - imperfect
                // but enough for "does landmark N sit roughly inside the
                // is-this-anything?" eyeballing.
                let (xmin, zmin, xmax, zmax) = raster.world_bounds;
                let mut pmin_x = i32::MAX;
                let mut pmin_z = i32::MAX;
                let mut pmax_x = i32::MIN;
                let mut pmax_z = i32::MIN;
                for &(x, z) in &pts {
                    pmin_x = pmin_x.min(x);
                    pmin_z = pmin_z.min(z);
                    pmax_x = pmax_x.max(x);
                    pmax_z = pmax_z.max(z);
                }
                let dx_p = (pmax_x - pmin_x).max(1) as f32;
                let dz_p = (pmax_z - pmin_z).max(1) as f32;
                let dx_w = (xmax - xmin).max(1) as f32;
                let dz_w = (zmax - zmin).max(1) as f32;
                let mapped: Vec<(i32, i32)> = pts
                    .iter()
                    .map(|&(x, z)| {
                        let nx = (x - pmin_x) as f32 / dx_p;
                        let nz = (z - pmin_z) as f32 / dz_p;
                        let mx = (nx * dx_w) as i32 + xmin;
                        let mz = (nz * dz_w) as i32 + zmin;
                        (mx, mz)
                    })
                    .collect();
                raster.draw_placements(&mapped, [0xF4, 0xB4, 0x1A, 0xFF], 3);
            }
            Err(e) => {
                eprintln!("warn: skipping placement overlay ({e})");
            }
        }
    }

    let f = std::fs::File::create(out)?;
    raster
        .encode_png(std::io::BufWriter::new(f))
        .map_err(|e| anyhow::anyhow!("write PNG: {e}"))?;
    println!("Wrote {out:?}  ({width}x{height})");
    Ok(())
}

/// Tiny JSON-ish picker for the `world-overview.json` placement records.
/// Returns `Vec<(x, z)>` in world units, filtering out script-positioned
/// records (which carry no static world coordinate). We hand-roll the
/// extraction to avoid pulling a full serde_json model just for two ints.
pub(crate) fn load_placements(path: &Path, kingdom: &str) -> Result<Vec<(i32, i32)>> {
    let raw = std::fs::read_to_string(path)?;
    let value: serde_json::Value = serde_json::from_str(&raw)?;
    let king = value
        .get(kingdom)
        .ok_or_else(|| anyhow::anyhow!("kingdom '{kingdom}' not in placement JSON"))?;
    let arr = king
        .get("placements")
        .and_then(|p| p.as_array())
        .ok_or_else(|| anyhow::anyhow!("no `placements` array under '{kingdom}'"))?;
    let mut pts = Vec::new();
    for p in arr {
        if p.get("script_positioned").and_then(|v| v.as_bool()) == Some(true) {
            continue;
        }
        let pos = p.get("pos").and_then(|v| v.as_array());
        if let Some(a) = pos
            && a.len() >= 3
            && let (Some(x), Some(z)) = (a[0].as_i64(), a[2].as_i64())
        {
            pts.push((x as i32, z as i32));
        }
    }
    Ok(pts)
}

pub(crate) fn kingdom_slot_cmd(
    input: &Path,
    slot: u8,
    out: Option<&Path>,
    wireframe_obj: Option<&Path>,
) -> Result<()> {
    use legaia_asset::{kingdom_bundle, world_map_overlay};

    let buf = crate::common::read_input(input)?;
    let bundle = kingdom_bundle::parse(&buf).ok_or_else(|| {
        anyhow::anyhow!("no 7-asset table found at any 0x800-aligned offset in {input:?}")
    })?;
    println!("PROT entry: {} bytes", buf.len());
    println!("Asset table at 0x{:X}", bundle.table_offset);
    println!();
    println!(
        "{:<5}  {:<8}  {:>12}  {:>10}  {:>10}",
        "Slot", "Type", "Declared size", "Data off", "Decoded"
    );
    println!("{}", "-".repeat(64));
    for s in &bundle.slots {
        let decoded_n = match &s.decoded {
            Ok(b) => b.len() as i64,
            Err(_) => -1,
        };
        let decoded_str = if decoded_n >= 0 {
            format!("{decoded_n} OK")
        } else {
            "(LZS err)".to_string()
        };
        println!(
            "{:<5}  0x{:02X}    {:>12}   0x{:08X}  {:>10}",
            s.index, s.type_byte, s.declared_size, s.data_offset, decoded_str
        );
    }
    println!();

    let target = bundle
        .slots
        .iter()
        .find(|s| s.index == slot)
        .ok_or_else(|| anyhow::anyhow!("slot {slot} not present"))?;
    let bytes = match &target.decoded {
        Ok(b) => b.clone(),
        Err(e) => anyhow::bail!("slot {slot}: LZS decode failed: {e}"),
    };
    println!(
        "Selected slot {slot}: type 0x{:02X}, {} decoded bytes",
        target.type_byte,
        bytes.len()
    );

    if let Some(path) = out {
        std::fs::write(path, &bytes)?;
        println!("  wrote raw decoded bytes -> {path:?}");
    }

    if slot == 4 {
        match world_map_overlay::parse(&bytes) {
            Ok(parsed) => {
                println!();
                println!("World-map slot-4 container: {} bodies", parsed.bodies.len());
                println!(
                    "{:<6}  {:>6}  {:>6}  {:>5}  {:>4}  {:>6}  {:>9}",
                    "Body", "ca", "cb", "kind", "flag", "recs", "non-zero"
                );
                println!("{}", "-".repeat(60));
                for b in &parsed.bodies {
                    let nz = b
                        .records
                        .iter()
                        .filter(|r| !(r.x == 0 && r.y == 0 && r.z == 0))
                        .count();
                    println!(
                        "{:<6}  {:>6}  {:>6}  {:>5}  {:>2},{}  {:>6}  {:>9}",
                        b.index,
                        b.count_a,
                        b.count_b,
                        b.kind,
                        b.flag_a,
                        b.flag_b,
                        b.records.len(),
                        nz
                    );
                }
                if let Some((xmin, zmin, xmax, zmax)) = world_map_overlay::xz_bounds(&parsed) {
                    println!(
                        "\nTop-down (X-Z) bounds (non-zero records): \
                         x = {xmin}..{xmax}, z = {zmin}..{zmax}"
                    );
                }
                if let Some(obj_path) = wireframe_obj {
                    let opts = world_map_overlay::WireframeOptions::default();
                    let lines = world_map_overlay::top_down_lines(&parsed, &opts);
                    write_wireframe_obj(obj_path, &lines)?;
                    println!("  wrote {} line segments -> {obj_path:?}", lines.len());
                }
            }
            Err(e) => {
                println!("\nslot 4 parse failed: {e}");
            }
        }
    }
    Ok(())
}

/// Write a wireframe-only Wavefront OBJ (X-Z plane, vertices use Y=0).
/// Each line becomes two vertices + one `l` directive. OBJ indices start
/// at 1.
pub(crate) fn write_wireframe_obj(
    path: &Path,
    lines: &[legaia_asset::world_map_overlay::WireframeLine],
) -> Result<()> {
    let mut s = String::new();
    s.push_str("# slot-4 wireframe (top-down X-Z)\n");
    s.push_str(&format!("# {} line segments\n", lines.len()));
    for l in lines {
        s.push_str(&format!("v {} 0 {}\n", l.x0, l.z0));
        s.push_str(&format!("v {} 0 {}\n", l.x1, l.z1));
    }
    for (i, _) in lines.iter().enumerate() {
        let a = 2 * i + 1;
        let b = 2 * i + 2;
        s.push_str(&format!("l {a} {b}\n"));
    }
    std::fs::write(path, s)?;
    Ok(())
}
