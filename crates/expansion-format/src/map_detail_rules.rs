//! The validator's rules for a map's detail layer
//! ([`crate::map_detail`]): decorations, brushes, edge runs, lights,
//! animated materials and fog. Called from the map's own rules
//! ([`crate::map_rules`]).

use crate::map::{MapDef, TileGrid};
use crate::map_detail::{MAX_DECORATIONS, MaterialFx};
use crate::map_rules::{check_light, check_mesh};
use crate::validate::Checker;

/// The palette sizes a glTF texture quantises to.
pub const MODEL_PALETTE_SIZES: [u16; 2] = [16, 256];

pub(crate) fn check_detail(
    m: &MapDef,
    cx: &mut Checker<'_>,
    base: Option<&str>,
    grid: Option<&TileGrid>,
) {
    let on = |t: [u16; 2]| {
        grid.is_none_or(|g| usize::from(t[0]) < g.width && usize::from(t[1]) < g.height)
    };
    for (i, d) in m.decor.iter().enumerate() {
        let at = format!("decor[{i}]");
        check_mesh(cx, &format!("{at}.mesh"), &d.mesh, base);
        if !on(d.tile) {
            cx.error(
                &format!("{at}.tile"),
                format!("tile {:?} is off the map", d.tile),
            );
        }
        check_scale(cx, &format!("{at}.scale"), d.scale);
    }
    for (i, s) in m.scatter.iter().enumerate() {
        let at = format!("scatter[{i}]");
        if s.meshes.is_empty() {
            cx.error(&format!("{at}.meshes"), "the brush has no meshes");
        }
        for (k, sm) in s.meshes.iter().enumerate() {
            check_mesh(cx, &format!("{at}.meshes[{k}].mesh"), &sm.mesh, base);
            if let Some([a, b]) = sm.scale {
                check_scale(cx, &format!("{at}.meshes[{k}].scale"), Some(a.min(b)));
            }
        }
        if !(s.density.is_finite() && s.density >= 0.0) {
            cx.error(
                &format!("{at}.density"),
                format!("density {} is not a count", s.density),
            );
        }
        if !on([s.rect[0], s.rect[1]]) || !on([s.rect[2], s.rect[3]]) {
            cx.error(&format!("{at}.rect"), "the rectangle runs off the map");
        }
        check_scale(cx, &format!("{at}.scale"), Some(s.scale[0].min(s.scale[1])));
        if s.jitter > 64 {
            cx.warning(
                &format!("{at}.jitter"),
                "jitter past 64 lets a mesh's origin leave its tile; it draws while its own \
                 tile is in view",
            );
        }
    }
    for (i, e) in m.edges.iter().enumerate() {
        let at = format!("edges[{i}]");
        check_mesh(cx, &format!("{at}.mesh"), &e.mesh, base);
        check_scale(cx, &format!("{at}.scale"), e.scale);
        if e.every == 0 {
            cx.error(&format!("{at}.every"), "`every` counts from 1");
        }
    }
    if grid.is_some() {
        match m.decorations() {
            Ok(d) if d.is_empty() => {}
            Ok(d) if d.len() > MAX_DECORATIONS / 2 => cx.warning(
                "scatter",
                format!(
                    "{} decorations; the limit is {MAX_DECORATIONS}, and every one is baked \
                     when the map loads",
                    d.len()
                ),
            ),
            Ok(_) => {}
            Err(e) => cx.error("decor", format!("{e:#}")),
        }
        if let Ok(d) = m.decorations() {
            let props: std::collections::BTreeSet<[u16; 2]> =
                m.props.iter().map(|p| p.tile).collect();
            let on_props = d.iter().filter(|d| props.contains(&d.tile)).count();
            if on_props > 0 {
                cx.warning(
                    "decor",
                    format!(
                        "{on_props} decorations stand on placed props' tiles and are left out \
                         (a cell draws one record)"
                    ),
                );
            }
        }
    }
    for (i, l) in m.lighting.lamps.iter().enumerate() {
        let at = format!("lighting.lamps[{i}]");
        if !on(l.tile) {
            cx.error(
                &format!("{at}.tile"),
                format!("tile {:?} is off the map", l.tile),
            );
        }
        check_light(cx, &at, l.radius, l.flicker, l.period);
    }
    for (i, fx) in m.materials.iter().enumerate() {
        let at = format!("materials[{i}]");
        if fx.period() < 1 {
            cx.error(
                &format!("{at}.period"),
                format!("period {} frames; it must be at least 1", fx.period()),
            );
        }
        match fx {
            MaterialFx::Cycle { start, count, .. } | MaterialFx::Hsv { start, count, .. } => {
                if *count == 0 || usize::from(*start) + usize::from(*count) > 256 {
                    cx.error(
                        &format!("{at}.count"),
                        format!(
                            "entries {start}..{} are outside a 256-entry palette",
                            usize::from(*start) + usize::from(*count)
                        ),
                    );
                }
                if matches!(fx, MaterialFx::Cycle { .. }) && *count < 2 {
                    cx.warning(&format!("{at}.count"), "a cycle of one entry never moves");
                }
            }
            MaterialFx::Scroll { dx, dy, .. } => {
                if *dx == 0 && *dy == 0 {
                    cx.warning(&at, "a scroll with `dx = dy = 0` never moves");
                }
            }
        }
    }
    for (i, d) in m.decals.iter().enumerate() {
        let at = format!("decals[{i}]");
        if d.tile.is_none() && d.rect.is_none() {
            cx.warning(&at, "a decal with neither `tile` nor `rect` places nothing");
        }
        if let Some(t) = d.tile
            && !on(t)
        {
            cx.error(&format!("{at}.tile"), format!("tile {t:?} is off the map"));
        }
        if let Some(r) = d.rect
            && (!on([r[0], r[1]]) || !on([r[2], r[3]]))
        {
            cx.error(&format!("{at}.rect"), "the rectangle runs off the map");
        }
        if !(3..=16).contains(&d.sides) {
            cx.error(
                &format!("{at}.sides"),
                format!("sides {} is outside 3..=16", d.sides),
            );
        }
        if d.size[0] == 0 || d.size[1] == 0 || d.size[0] > 1024 || d.size[1] > 1024 {
            cx.error(
                &format!("{at}.size"),
                "a decal is 1..=1024 world units a side",
            );
        }
        if !(d.density.is_finite() && (0.0..=8.0).contains(&d.density)) {
            cx.error(
                &format!("{at}.density"),
                format!("density {} is outside 0..=8", d.density),
            );
        }
    }
    if grid.is_some()
        && let Err(e) = m.decals()
    {
        cx.error("decals", format!("{e:#}"));
    }
    for (i, e) in m.emitters.iter().enumerate() {
        let at = format!("emitters[{i}]");
        if !on([e.rect[0], e.rect[1]]) || !on([e.rect[2], e.rect[3]]) {
            cx.error(&format!("{at}.rect"), "the rectangle runs off the map");
        }
        if let Some(c) = e.count
            && c > crate::map_detail::MAX_PARTICLES
        {
            cx.error(
                &format!("{at}.count"),
                format!(
                    "{c} particles; an emitter keeps at most {}",
                    crate::map_detail::MAX_PARTICLES
                ),
            );
        }
        if e.life.is_some_and(|l| l < 1) {
            cx.error(&format!("{at}.life"), "a particle lives at least 1 frame");
        }
    }
    for (i, f) in m.fog.regions.iter().enumerate() {
        let at = format!("fog.regions[{i}]");
        if !on([f.rect[0], f.rect[1]]) || !on([f.rect[2], f.rect[3]]) {
            cx.error(&format!("{at}.rect"), "the rectangle runs off the map");
        }
    }
}

fn check_scale(cx: &mut Checker<'_>, at: &str, scale: Option<f32>) {
    if let Some(s) = scale
        && !(s.is_finite() && s > 0.0)
    {
        cx.error(at, format!("scale {s} is not positive"));
    }
}

/// A glTF model asset: carried by the pack, `.glb` with the binary glTF
/// header or `.gltf` JSON, quantised to a palette size the renderer has.
pub(crate) fn check_model_asset(cx: &mut Checker<'_>, at: &str, asset: &str, colors: Option<u16>) {
    let field = format!("{at}.asset");
    let Some(bytes) = cx.pack().assets.get(asset).cloned() else {
        cx.error(&field, format!("the pack carries no `assets/{asset}`"));
        return;
    };
    let lower = asset.to_ascii_lowercase();
    if lower.ends_with(".glb") {
        let ok = bytes.len() >= 12
            && &bytes[..4] == b"glTF"
            && u32::from_le_bytes(bytes[4..8].try_into().expect("4 bytes")) == 2;
        if !ok {
            cx.error(&field, "not a glTF 2.0 binary (`glTF` header, version 2)");
        }
    } else if lower.ends_with(".gltf") {
        if bytes.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{') {
            cx.error(&field, "not glTF JSON");
        }
    } else {
        cx.error(&field, "a model asset is a `.glb` or `.gltf` file");
    }
    if let Some(c) = colors
        && !MODEL_PALETTE_SIZES.contains(&c)
    {
        cx.error(
            &format!("{at}.colors"),
            format!("colors {c}: a model texture quantises to 16 or 256 colours"),
        );
    }
}
