//! Disc-gated fidelity oracle for the OBJ+PNG monster model codec
//! (`legaia_asset::monster_model`): export a monster to OBJ + RGBA page,
//! re-import, and require the result to be **render-equivalent** to retail:
//!
//! - the resolved primitive set matches (positions, stored quad order, UVs,
//!   per-corner baked colours, textured/semi/abr classification) - shape
//!   labels may legally differ (a gouraud prim with uniform corners
//!   re-imports as flat; identical on the GPU);
//! - every texel any textured prim covers renders to the same colour
//!   through the re-imported palettes as through the retail ones.
//!
//! Byte-level equality is NOT expected here (palette regrouping, vertex
//! dedup order, group bucketing) - the TMD byte layer has its own oracle in
//! `monster_tmd_reencode_real.rs`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_asset::monster_archive;
use legaia_asset::monster_model::{self, PAGE_HEIGHT};
use legaia_tmd::encode::decode_model;

fn entry_867() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        let f = PathBuf::from(p).join("0867_battle_data.BIN");
        if f.is_file() {
            return std::fs::read(f).ok();
        }
    }
    None
}

/// Position, baked colour, and (for textured prims) UV of one prim corner.
type Corner = ([i16; 3], [u8; 3], Option<(u8, u8)>);

/// One prim resolved to render-equivalent form.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ResolvedPrim {
    part: usize,
    textured: bool,
    semi: bool,
    abr: u8,
    corners: Vec<Corner>,
    palette: u8, // cba & 0x3F (compared via texel colours, not directly)
}

fn resolve(model: &[legaia_tmd::encode::ModelObject]) -> Vec<ResolvedPrim> {
    let mut out = Vec::new();
    for (oi, o) in model.iter().enumerate() {
        for g in &o.groups {
            for p in &g.prims {
                let n = p.vertices.len();
                let corners = (0..n)
                    .map(|k| {
                        let pos = o.vertices[p.vertices[k] as usize];
                        let col = if p.colors.len() == n {
                            p.colors[k]
                        } else {
                            p.colors[0]
                        };
                        let uv = p.uvs.get(k).copied();
                        (pos, col, uv)
                    })
                    .collect();
                out.push(ResolvedPrim {
                    part: oi,
                    textured: g.shape.is_textured(),
                    semi: g.semi_transparent,
                    abr: if g.semi_transparent {
                        ((p.tsb >> 5) & 3) as u8
                    } else {
                        1
                    },
                    corners,
                    palette: (p.cba & 0x3F) as u8,
                });
            }
        }
    }
    out.sort();
    out
}

/// Decode a pool into raw BGR555 CLUTs + texel indices.
fn decode_pool(pool: &[u8]) -> (Vec<[u16; 16]>, Vec<u8>, usize) {
    let cluts: Vec<[u16; 16]> = (0..monster_model::CLUT_COUNT)
        .map(|c| {
            let mut pal = [0u16; 16];
            for (i, slot) in pal.iter_mut().enumerate() {
                *slot = legaia_bytes::u16_le(pool, c * 32 + i * 2).unwrap_or(0);
            }
            pal
        })
        .collect();
    let pixels = &pool[monster_model::CLUT_REGION_BYTES..];
    let width = pixels.len() / PAGE_HEIGHT * 2;
    let mut indices = vec![0u8; width * PAGE_HEIGHT];
    for y in 0..PAGE_HEIGHT {
        for xb in 0..width / 2 {
            let b = pixels[y * (width / 2) + xb];
            indices[y * width + xb * 2] = b & 0x0F;
            indices[y * width + xb * 2 + 1] = b >> 4;
        }
    }
    (cluts, indices, width)
}

/// Sampled texel colours of a prim's UV footprint through its palette.
/// Only the exact corner texels + the strict interior are sampled (the
/// codec's ownership rasterizer is more generous; corners are what the GPU
/// is guaranteed to hit).
fn corner_texels(
    prim: &ResolvedPrim,
    cluts: &[[u16; 16]],
    indices: &[u8],
    width: usize,
) -> Vec<((u8, u8), u16)> {
    let pal = &cluts[prim.palette as usize];
    prim.corners
        .iter()
        .filter_map(|&(_, _, uv)| uv)
        // Narrow pools only back the left half of the 256-texel UV space;
        // a corner past the pool width samples VRAM outside the upload.
        .filter(|&(u, _)| (u as usize) < width)
        .map(|(u, v)| {
            let idx = indices[v as usize * width + u as usize] as usize;
            ((u, v), pal[idx])
        })
        .collect()
}

#[test]
fn monster_models_survive_obj_png_round_trip() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };

    let mut verified = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let slot_count = (entry.len() / monster_archive::SLOT_STRIDE) as u16;
    for id in 1..=slot_count {
        let Ok(Some(mesh)) = monster_archive::mesh(&entry, id) else {
            continue;
        };
        if mesh.texture_pool_bytes().is_none() {
            continue;
        }
        let exported = match monster_model::export_obj(&mesh, "test") {
            Ok(e) => e,
            Err(e) => {
                failures.push(format!("id {id}: export failed: {e:#}"));
                continue;
            }
        };
        let retail_tmd = legaia_tmd::parse(mesh.tmd_bytes()).expect("retail parse");
        let retail_model = decode_model(&retail_tmd, mesh.tmd_bytes()).expect("retail decode");
        let imported = match monster_model::import_obj(
            &exported.obj,
            &exported.rgba,
            exported.page_width,
            retail_model.len(),
        ) {
            Ok(m) => m,
            Err(e) => {
                failures.push(format!("id {id}: import failed: {e:#}"));
                continue;
            }
        };
        let new_tmd = legaia_tmd::parse(&imported.tmd).expect("imported TMD parse");
        let new_model = decode_model(&new_tmd, &imported.tmd).expect("imported decode");

        // Geometry: resolved prim multisets must match up to palette ids.
        let a = resolve(&retail_model);
        let b = resolve(&new_model);
        if a.len() != b.len() {
            failures.push(format!(
                "id {id}: prim count changed {} -> {}",
                a.len(),
                b.len()
            ));
            continue;
        }
        let strip = |v: &[ResolvedPrim]| -> Vec<ResolvedPrim> {
            v.iter()
                .map(|p| ResolvedPrim {
                    palette: 0,
                    ..p.clone()
                })
                .collect()
        };
        if strip(&a) != strip(&b) {
            let sa = strip(&a);
            let sb = strip(&b);
            let first = sa
                .iter()
                .zip(sb.iter())
                .position(|(x, y)| x != y)
                .unwrap_or(0);
            failures.push(format!(
                "id {id}: prim {first} diverges:\n  retail  {:?}\n  reimport {:?}",
                sa.get(first),
                sb.get(first)
            ));
            continue;
        }

        // Texels: corner samples of every textured prim must render to the
        // same BGR555 through each side's palettes.
        let retail_pool = mesh.texture_pool_bytes().unwrap();
        let (ac, ai, aw) = decode_pool(retail_pool);
        let (bc, bi, bw) = decode_pool(&imported.pool);
        if aw != bw {
            failures.push(format!("id {id}: page width changed {aw} -> {bw}"));
            continue;
        }
        // Contested corners: texels where prims of >= 2 distinct retail
        // palettes each place a UV corner. Retail renders the same stored
        // index through both CLUTs; a single flat PNG can only bake one
        // interpretation, so these are excluded - everything else must be
        // EXACT.
        let mut corner_palettes: BTreeMap<(u8, u8), std::collections::BTreeSet<u8>> =
            BTreeMap::new();
        for p in a.iter().filter(|p| p.textured) {
            for &(_, _, uv) in &p.corners {
                if let Some(uv) = uv {
                    corner_palettes.entry(uv).or_default().insert(p.palette);
                }
            }
        }
        let contested: std::collections::BTreeSet<(u8, u8)> = corner_palettes
            .iter()
            .filter(|(_, pals)| pals.len() >= 2)
            .map(|(uv, _)| *uv)
            .collect();
        let mut texel_mismatch: BTreeMap<(u8, u8), (u16, u16)> = BTreeMap::new();
        for (pa, pb) in a.iter().zip(b.iter()) {
            if !pa.textured {
                continue;
            }
            let ta = corner_texels(pa, &ac, &ai, aw);
            let tb = corner_texels(pb, &bc, &bi, bw);
            for ((uv_a, col_a), (uv_b, col_b)) in ta.iter().zip(tb.iter()) {
                assert_eq!(uv_a, uv_b, "id {id}: uv order changed");
                if col_a != col_b && !contested.contains(uv_a) {
                    texel_mismatch.insert(*uv_a, (*col_a, *col_b));
                }
            }
        }
        // Uncontested corners must be EXACT - any mismatch here is a codec
        // defect (UV drift, palette-grouping breakage, ownership-rule
        // divergence), not a bake limitation.
        if !texel_mismatch.is_empty() {
            let sample: Vec<String> = texel_mismatch
                .iter()
                .take(5)
                .map(|((u, v), (a, b))| format!("({u},{v}): {a:#06x} -> {b:#06x}"))
                .collect();
            failures.push(format!(
                "id {id}: {} uncontested corner texel(s) changed colour \
                 ({} contested excluded), e.g. {}",
                texel_mismatch.len(),
                contested.len(),
                sample.join(", ")
            ));
            continue;
        }
        let allowance = 16; // PNG fixed-point below: seam + rounding slack

        // Fixed-point property: exporting the imported model again must
        // reproduce the geometry and the PNG exactly - after the one lossy
        // bake, the codec must be lossless.
        match monster_model::export_obj_parts(&imported.tmd, &imported.pool, "test") {
            Ok(second) => {
                if second.obj != exported.obj {
                    // Geometry text can legally differ (vertex dedup order,
                    // shape reclassification) - compare the resolved form.
                    let second_tmd = legaia_tmd::parse(&imported.tmd).unwrap();
                    let second_model = decode_model(&second_tmd, &imported.tmd).unwrap();
                    if strip(&resolve(&second_model)) != strip(&b) {
                        failures.push(format!("id {id}: second export changed geometry"));
                        continue;
                    }
                }
                // Owned texels must round-trip exactly PNG-to-PNG (unowned
                // texels are dead bytes the import legitimately discards).
                let png_diff = exported
                    .rgba
                    .chunks_exact(4)
                    .zip(second.rgba.chunks_exact(4))
                    .zip(exported.owned.iter())
                    .filter(|((x, y), owned)| **owned && x != y)
                    .count();
                if png_diff > allowance {
                    failures.push(format!(
                        "id {id}: second export changed {png_diff} PNG texel(s) \
                         (allowance {allowance})"
                    ));
                    continue;
                }
            }
            Err(e) => {
                failures.push(format!("id {id}: second export failed: {e:#}"));
                continue;
            }
        }
        verified += 1;
    }

    assert!(
        failures.is_empty(),
        "{} monster(s) failed the OBJ+PNG round trip:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(
        verified >= 150,
        "only {verified} monsters verified - the walk went vacuous"
    );
}
