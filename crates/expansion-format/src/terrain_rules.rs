//! The validator's rules for a map's `[terrain]`
//! ([`crate::terrain::TerrainDef`]). Called from the map's own rules
//! ([`crate::map_rules`]).

use crate::map::{MapDef, TileGrid};
use crate::terrain::{MAX_HEIGHT, MAX_SKIRT, RESOLUTIONS, TerrainShape};
use crate::validate::Checker;

pub(crate) fn check_terrain(m: &MapDef, cx: &mut Checker<'_>, grid: Option<&TileGrid>) {
    let Some(t) = &m.terrain else {
        return;
    };
    if !RESOLUTIONS.contains(&t.resolution) {
        cx.error(
            "terrain.resolution",
            format!("{} is not one of {RESOLUTIONS:?}", t.resolution),
        );
    }
    if t.skirt > MAX_SKIRT {
        cx.error(
            "terrain.skirt",
            format!("{} tiles; a skirt reaches at most {MAX_SKIRT}", t.skirt),
        );
    }
    if !(t.max_step.is_finite() && t.max_step > 0.0) {
        cx.error("terrain.max_step", "the walkable step must be positive");
    }
    for (what, v) in [
        ("variation", t.variation),
        ("blend", t.blend),
        ("soften", t.soften),
        ("shading", t.shading),
    ] {
        if !(0.0..=1.0).contains(&v) {
            cx.error(&format!("terrain.{what}"), format!("{v} is outside 0..=1"));
        }
    }
    if !(0.0..=90.0).contains(&t.sun[1]) {
        cx.error("terrain.sun", "the sun's elevation is degrees in 0..=90");
    }
    let height_ok = |h: f32| h.is_finite() && h.abs() <= MAX_HEIGHT;
    let on = |r: [u16; 4]| {
        grid.is_none_or(|g| {
            usize::from(r[0].max(r[2])) < g.width && usize::from(r[1].max(r[3])) < g.height
        })
    };
    for (i, s) in t.shapes.iter().enumerate() {
        let at = format!("terrain.shapes[{i}]");
        let (radius, heights): (Option<f32>, Vec<f32>) = match s {
            TerrainShape::Raise { radius, height, .. }
            | TerrainShape::Ridge { radius, height, .. } => (Some(*radius), vec![*height]),
            TerrainShape::Flatten { radius, height, .. } => {
                (Some(*radius), height.iter().copied().collect())
            }
            TerrainShape::Ramp { radius, height, .. } => (Some(*radius), height.to_vec()),
            TerrainShape::Smooth { radius, .. } => (Some(*radius), vec![]),
            TerrainShape::Stroke {
                path,
                radius,
                height,
                strength,
                ..
            } => {
                if path.is_empty() || path.len() > 4096 {
                    cx.error(&format!("{at}.path"), "a stroke has 1 to 4096 points");
                }
                if !(0.0..=1.0).contains(strength) {
                    cx.error(&format!("{at}.strength"), "strength is 0..=1");
                }
                (Some(*radius), height.iter().copied().collect())
            }
            TerrainShape::Heightmap {
                image,
                rect,
                height,
                ..
            } => {
                let key = image.strip_prefix("assets/").unwrap_or(image);
                match cx.pack().assets.get(key) {
                    None => cx.error(
                        &format!("{at}.image"),
                        format!("assets/{key} is not in the pack"),
                    ),
                    Some(bytes) => {
                        if let Err(e) = crate::terrain::GrayImage::from_png(bytes) {
                            cx.error(&format!("{at}.image"), format!("assets/{key}: {e}"));
                        }
                    }
                }
                if rect.is_some_and(|r| !on(r)) {
                    cx.error(&format!("{at}.rect"), "the rectangle runs off the map");
                }
                (None, height.to_vec())
            }
            TerrainShape::Plateau {
                rect, height, edge, ..
            } => {
                if !on(*rect) {
                    cx.error(&format!("{at}.rect"), "the rectangle runs off the map");
                }
                if *edge < 0.0 {
                    cx.error(&format!("{at}.edge"), "an edge width is not negative");
                }
                (None, vec![*height])
            }
            TerrainShape::Noise {
                rect,
                amplitude,
                scale,
                ..
            } => {
                if rect.is_some_and(|r| !on(r)) {
                    cx.error(&format!("{at}.rect"), "the rectangle runs off the map");
                }
                if *scale <= 0.0 {
                    cx.error(&format!("{at}.scale"), "a noise scale is positive");
                }
                (None, vec![*amplitude])
            }
        };
        if radius.is_some_and(|r| !(r.is_finite() && r > 0.0)) {
            cx.error(&format!("{at}.radius"), "a brush radius is positive");
        }
        if heights.iter().any(|h| !height_ok(*h)) {
            cx.error(
                &format!("{at}.height"),
                format!("heights stay within {MAX_HEIGHT} units either way"),
            );
        }
    }
    let mut claimed = std::collections::BTreeMap::new();
    for (i, mat) in t.materials.iter().enumerate() {
        let at = format!("terrain.materials[{i}]");
        if mat.cells.is_empty() {
            cx.error(&format!("{at}.cells"), "a material has at least one cell");
        }
        for ch in mat.chars.chars() {
            if let Some(prev) = claimed.insert(ch, i) {
                cx.error(
                    &format!("{at}.chars"),
                    format!("{ch:?} is already painted by materials[{prev}]"),
                );
            }
        }
    }
    if !matches!(t.detail, 1 | 2 | 4) {
        cx.error("terrain.detail", format!("{} is not 1, 2 or 4", t.detail));
    }
    for scene in t.borrowed() {
        if scene.is_empty() {
            cx.error("terrain", "a `from` names no scene");
        }
    }
    if t.cliff.as_ref().is_some_and(|c| c.cells.is_empty()) {
        cx.error("terrain.cliff.cells", "the cliff material has no cells");
    }
    if let Some(w) = &t.water {
        if w.cells.is_empty() {
            cx.error("terrain.water.cells", "the water has no cells");
        }
        if !height_ok(w.level) {
            cx.error("terrain.water.level", "the level is out of range");
        }
    }
    if let Some(sky) = &t.sky {
        if sky.segments < 3 || sky.repeat == 0 || sky.repeat > sky.segments {
            cx.error(
                "terrain.sky",
                "a ring has at least 3 segments and 1..=segments copies",
            );
        }
        if !(sky.radius.is_finite() && sky.radius >= 0.0 && sky.radius <= 64.0) {
            cx.error("terrain.sky.radius", "radius is 0..=64 tiles");
        }
        if !(sky.span[0] < sky.span[1] && sky.span.iter().all(|h| height_ok(*h))) {
            cx.error(
                "terrain.sky.span",
                "the foot stands below the top, within range",
            );
        }
    }
    // Every place the player is put or must reach stays walkable.
    let Some(g) = grid else {
        return;
    };
    let Ok(extra) = m.terrain_walls(g) else {
        cx.error("terrain", "the heightfield does not build");
        return;
    };
    let closed = |c: u16, r: u16| {
        let (c, r) = (usize::from(c), usize::from(r));
        g.get(c, r)
            .is_some_and(|t| (t.walls | extra[r * g.width + c]) & 0xF == 0xF)
    };
    let mut spots: Vec<(String, [u16; 2])> = Vec::new();
    for (i, s) in m.spawns.iter().enumerate() {
        spots.push((format!("spawns[{i}]"), s.tile));
    }
    for (i, n) in m.npcs.iter().enumerate() {
        spots.push((format!("npcs[{i}]"), n.tile));
    }
    for (i, e) in m.exits.iter().enumerate() {
        let r = e.rect;
        let any_open = (r[1]..=r[3]).any(|row| (r[0]..=r[2]).any(|col| !closed(col, row)));
        if !any_open {
            cx.warning(
                &format!("exits[{i}].rect"),
                "the terrain walls every tile of the exit (too steep or under water)",
            );
        }
    }
    for (at, [c, r]) in spots {
        if closed(c, r) {
            cx.warning(
                &format!("{at}.tile"),
                format!("tile ({c}, {r}) is too steep or too deep to stand on"),
            );
        }
    }
}
