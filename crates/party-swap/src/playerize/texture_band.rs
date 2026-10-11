//! The texture side of a playerized file: the party band regions and face-stamp
//! windows, palette merging, and re-laying a monster's texels out into the
//! band.

use super::super::*;
use super::CLUT_BYTES;
use legaia_actor_formats::battle_char_assembly::SECTION_TEXTURE_RECTS;

/// One free texture region of the party band, in authoring-page space.
struct BandRegion {
    /// Authoring page: 0 = texpage 0x15, 1 = 0x16.
    page: usize,
    x0: usize,
    y0: usize,
    w: usize,
    h: usize,
    /// Which equipment section's pool uploads this region.
    section: usize,
}

/// Per-character live-face window inside section 1's rect, in band
/// texel coordinates `(x0, y0, x1, y1)` (x1/y1 exclusive): the union of
/// the eye + mouth stamp destination rects from the static
/// `SCUS_942.54` face-frame tables (`DAT_800768CC` / `DAT_800768E4`,
/// halfword coords x4 - mirror `legaia_actor_formats::face_anim`, values
/// asserted against the disc by `face_stamp_windows_match_the_scus_tables`).
///
/// The per-frame facial animator (`FUN_8004C7B4`) MoveImages the
/// character's eye / mouth frames onto these rows EVERY frame - the
/// neutral frame is re-stamped even when no track record is active - so
/// any sibling texels the relayout parks here are overwritten with the
/// host's face the moment a battle starts (the source strips live in
/// `RECORD0_TEXTURE_RECTS[0]`'s tile, which keeps its retail pixels).
/// The relayout must therefore keep this window empty and unreferenced.
///
/// Vahn: eyes (0,16) 15x17 + mouth (4,42) 7x16; Noa: eyes (2,72) 16x32 +
/// mouth (8,104) 4x24; Gala: eyes (0,24) 14x24 + mouth (4,48) 6x24.
pub const FACE_STAMP_WINDOWS: [(usize, usize, usize, usize); 3] = [
    (0, 16, 15 * 4, 58),      // Vahn: hw 0..15, rows 16..58
    (2 * 4, 72, 18 * 4, 128), // Noa: hw 2..18, rows 72..128
    (0, 24, 14 * 4, 72),      // Gala: hw 0..14, rows 24..72
];

/// The section tiles in authoring-page space (see
/// `SECTION_TEXTURE_RECTS`: band halfword x < 0x40 = page 0x15, else
/// 0x16; texels = halfwords * 4). Section 1's tile is split around the
/// character's live-face window ([`FACE_STAMP_WINDOWS`]) so no sibling
/// cluster is placed where the facial animator stamps - the emitted
/// pool still covers the full rect (window texels stay zero).
fn band_regions(char_slot: usize) -> Vec<BandRegion> {
    let mut out = Vec::new();
    for (section, r) in SECTION_TEXTURE_RECTS.iter().enumerate() {
        let hw_x = r.x0 as usize;
        let (page, page_hw_x) = if hw_x < 0x40 {
            (0, hw_x)
        } else {
            (1, hw_x - 0x40)
        };
        let base = BandRegion {
            page,
            x0: page_hw_x * 4,
            y0: r.y0 as usize,
            w: r.w as usize * 4,
            h: r.h as usize,
            section,
        };
        let window = FACE_STAMP_WINDOWS.get(char_slot).copied();
        if section != 1 || window.is_none() {
            out.push(base);
            continue;
        }
        // The face window is authored band-relative and section 1's rect
        // starts at band (0, 0), so window coords are region-local.
        let (wx0, wy0, wx1, wy1) = window.unwrap();
        let (wx1, wy1) = (wx1.min(base.w), wy1.min(base.h));
        let mut push = |x0: usize, y0: usize, w: usize, h: usize| {
            if w > 0 && h > 0 {
                out.push(BandRegion {
                    page: base.page,
                    x0: base.x0 + x0,
                    y0: base.y0 + y0,
                    w,
                    h,
                    section,
                });
            }
        };
        push(0, 0, base.w, wy0); // above the window
        push(0, wy0, wx0, wy1 - wy0); // left of the window
        push(wx1, wy0, base.w - wx1, wy1 - wy0); // right of the window
        push(0, wy1, base.w, base.h - wy1); // below the window
    }
    out
}

/// Decode a monster pool into per-texel indices + CLUTs.
pub(in super::super) fn monster_pool_texels(
    pool: &[u8],
) -> Result<(Vec<[u16; 16]>, Vec<u8>, usize)> {
    if pool.len() <= CLUT_REGION_BYTES {
        bail!("monster pool too small");
    }
    let cluts: Vec<[u16; 16]> = (0..CLUT_COUNT)
        .map(|c| {
            let mut pal = [0u16; 16];
            for (i, slot) in pal.iter_mut().enumerate() {
                *slot = u16::from_le_bytes(
                    pool[c * CLUT_BYTES + i * 2..c * CLUT_BYTES + i * 2 + 2]
                        .try_into()
                        .unwrap(),
                );
            }
            pal
        })
        .collect();
    let pixels = &pool[CLUT_REGION_BYTES..];
    let bytes_per_row = pixels.len() / PAGE_HEIGHT;
    if bytes_per_row == 0 {
        bail!("monster pool has no pixel rows");
    }
    let width = bytes_per_row * 2;
    let mut indices = vec![0u8; width * PAGE_HEIGHT];
    for y in 0..PAGE_HEIGHT {
        for xb in 0..bytes_per_row {
            let b = pixels[y * bytes_per_row + xb];
            indices[y * width + xb * 2] = b & 0x0F;
            indices[y * width + xb * 2 + 1] = b >> 4;
        }
    }
    Ok((cluts, indices, width))
}

/// `(group_of_palette, group_colors)` - a texel's new index is its
/// colour's position in its group.
type PaletteGroups = (BTreeMap<u8, usize>, Vec<Vec<u16>>);

/// Union-merge the used palettes down to `budget` groups.
pub(in super::super) fn merge_palettes(
    used: &[u8],
    cluts: &[[u16; 16]],
    budget: usize,
    warnings: &mut Vec<String>,
) -> Result<PaletteGroups> {
    // Start with one group per used palette (distinct colours, index 0
    // reserved for transparent 0x0000).
    let mut groups: Vec<(Vec<u8>, Vec<u16>)> = used
        .iter()
        .map(|&p| {
            let mut colors: Vec<u16> = Vec::new();
            for &c in cluts[p as usize].iter().skip(1) {
                if c != 0 && !colors.contains(&c) {
                    colors.push(c);
                }
            }
            (vec![p], colors)
        })
        .collect();
    // Greedy: repeatedly merge the pair with the smallest colour union
    // while over budget; prefer lossless unions (<= 15 non-transparent
    // colours), fall back to dropping the least-frequent colours.
    while groups.len() > budget {
        let mut best: Option<(usize, usize, usize)> = None;
        for i in 0..groups.len() {
            for j in (i + 1)..groups.len() {
                let mut union = groups[i].1.clone();
                for &c in &groups[j].1 {
                    if !union.contains(&c) {
                        union.push(c);
                    }
                }
                let cost = union.len();
                if best.is_none_or(|(_, _, b)| cost < b) {
                    best = Some((i, j, cost));
                }
            }
        }
        let Some((i, j, cost)) = best else { break };
        if cost > 15 {
            warnings.push(format!(
                "palette merge exceeds 15 colours ({cost}); nearest colours collapse"
            ));
        }
        let (pals_j, colors_j) = groups.remove(j);
        let gi = &mut groups[i];
        gi.0.extend(pals_j);
        for c in colors_j {
            if !gi.1.contains(&c) {
                gi.1.push(c);
            }
        }
        gi.1.truncate(15);
    }
    let mut group_of = BTreeMap::new();
    let mut colors = Vec::new();
    for (gidx, (pals, cols)) in groups.iter().enumerate() {
        for &p in pals {
            group_of.insert(p, gidx);
        }
        colors.push(cols.clone());
    }
    Ok((group_of, colors))
}

/// Nearest colour in `colors` to `c` (RGB555 distance).
pub(in super::super) fn nearest_color(colors: &[u16], c: u16) -> usize {
    let split = |v: u16| {
        (
            (v & 0x1F) as i32,
            ((v >> 5) & 0x1F) as i32,
            ((v >> 10) & 0x1F) as i32,
        )
    };
    let (r, g, b) = split(c);
    let mut best = 0usize;
    let mut best_d = i32::MAX;
    for (i, &o) in colors.iter().enumerate() {
        let (orr, og, ob) = split(o);
        let d = (r - orr).pow(2) + (g - og).pow(2) + (b - ob).pow(2);
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    best
}

/// The re-laid band texture: per-section pool blocks + per-face rewrites.
pub(super) struct BandLayout {
    /// Per-section `[u16 clut_x][u16 clut_n][cluts][pixels]` pool bytes.
    pub(super) section_pools: Vec<Vec<u8>>,
}

/// Re-layout the Delilas page into the five section tiles, rewriting the
/// baked objects' UV/CBA/TSB in place (player authoring conventions:
/// CLUT row 480, texpages 0x15/0x16).
#[allow(clippy::too_many_arguments)]
pub(super) fn relayout_to_band(
    objects: &mut [ModelObject],
    cluts: &[[u16; 16]],
    indices: &[u8],
    src_width: usize,
    reserved_cols: &[u16],
    exclude_section: Option<usize>,
    base_scale: u32,
    char_slot: usize,
    warnings: &mut Vec<String>,
) -> Result<BandLayout> {
    // Collect textured faces (source page is single: the monster page).
    let mut faces: Vec<FaceRef> = Vec::new();
    for (oi, o) in objects.iter().enumerate() {
        for (gi, g) in o.groups.iter().enumerate() {
            if !g.shape.is_textured() {
                continue;
            }
            for (pi, p) in g.prims.iter().enumerate() {
                faces.push(FaceRef {
                    obj: oi,
                    group: gi,
                    prim: pi,
                    page: 0,
                    bbox: face_bbox(&p.uvs),
                });
            }
        }
    }

    // Cluster by inflated-bbox overlap (same idiom as the enemy side).
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    fn find(parent: &mut Vec<usize>, i: usize) -> usize {
        if parent[i] != i {
            let r = find(parent, parent[i]);
            parent[i] = r;
        }
        parent[i]
    }
    for i in 0..faces.len() {
        for j in (i + 1)..faces.len() {
            if boxes_touch(faces[i].bbox, faces[j].bbox) {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }
    let mut clusters: BTreeMap<usize, Cluster> = BTreeMap::new();
    #[allow(clippy::needless_range_loop)]
    for i in 0..faces.len() {
        let root = find(&mut parent, i);
        let e = clusters.entry(root).or_insert(Cluster {
            faces: Vec::new(),
            page: 0,
            bbox: faces[i].bbox,
            scale: base_scale,
            dst: (0, 0),
        });
        e.bbox = union_bbox(e.bbox, faces[i].bbox);
        e.faces.push(i);
    }
    let mut clusters: Vec<Cluster> = clusters.into_values().collect();

    // Multi-region shelf pack: place each cluster into one band region
    // (a face samples a single texpage, so a cluster cannot straddle a
    // region boundary). First-fit-decreasing per region; halve the
    // largest cluster and retry on overflow.
    let regions = band_regions(char_slot);
    // (region, x, y) per cluster.
    let placement: Vec<(usize, usize, usize)> = loop {
        let mut order: Vec<usize> = (0..clusters.len()).collect();
        order.sort_by_key(|&i| {
            let c = &clusters[i];
            let h = ((c.bbox.3 - c.bbox.1) as usize + 1).div_ceil(c.scale as usize);
            std::cmp::Reverse(h)
        });
        let mut cursors: Vec<(usize, usize, usize)> = regions.iter().map(|_| (0, 0, 0)).collect();
        let mut placed = vec![None; clusters.len()];
        'clusters: for &i in &order {
            let c = &clusters[i];
            let s = c.scale as usize;
            let w = ((c.bbox.2 - c.bbox.0) as usize + 1).div_ceil(s);
            let h = ((c.bbox.3 - c.bbox.1) as usize + 1).div_ceil(s);
            for (ri, r) in regions.iter().enumerate() {
                // The weapon section's tile belongs to the per-record
                // weapon texture (see `weapon_fuse`) - no sibling
                // islands there.
                if Some(r.section) == exclude_section {
                    continue;
                }
                let (ref mut x, ref mut y, ref mut shelf) = cursors[ri];
                if w > r.w || h > r.h {
                    continue;
                }
                if *x + w > r.w {
                    *y += *shelf;
                    *x = 0;
                    *shelf = 0;
                }
                if *y + h > r.h {
                    continue;
                }
                placed[i] = Some((ri, *x, *y));
                *x += w;
                *shelf = (*shelf).max(h);
                continue 'clusters;
            }
        }
        if placed.iter().all(|p| p.is_some()) {
            break placed.into_iter().map(|p| p.unwrap()).collect();
        }
        let cap = base_scale * 4;
        let Some(big) = clusters
            .iter_mut()
            .filter(|c| c.scale < cap)
            .max_by_key(|c| {
                let w = (c.bbox.2 - c.bbox.0) as usize + 1;
                let h = (c.bbox.3 - c.bbox.1) as usize + 1;
                (w / c.scale as usize) * (h / c.scale as usize)
            })
        else {
            bail!("texture islands exceed the party band tiles even at quarter resolution");
        };
        big.scale *= 2;
        warnings.push(format!(
            "player-side texture island {}x{} downscaled to 1/{} to fit the band",
            big.bbox.2 - big.bbox.0 + 1,
            big.bbox.3 - big.bbox.1 + 1,
            big.scale
        ));
    };

    // Palette groups within the free-column budget.
    let mut used: Vec<u8> = faces
        .iter()
        .map(|f| (objects[f.obj].groups[f.group].prims[f.prim].cba & 0x3F) as u8)
        .collect();
    used.sort_unstable();
    used.dedup();
    let free_cols: Vec<u16> = (0..16u16).filter(|c| !reserved_cols.contains(c)).collect();
    if free_cols.is_empty() {
        bail!("record[0] claims every CLUT column of the band row");
    }
    let (group_of, group_colors) = merge_palettes(&used, cluts, free_cols.len(), warnings)?;
    if group_colors.len() > free_cols.len() {
        bail!(
            "{} palette groups but only {} free CLUT columns",
            group_colors.len(),
            free_cols.len()
        );
    }
    let col_of_group: Vec<u16> = free_cols[..group_colors.len()].to_vec();

    // Paint the band tiles: per cluster, copy texel colours through the
    // source palette, then re-index through the face's group palette.
    // A texel shared by faces of two groups keeps the first writer
    // (bounded seam residual - same limitation the OBJ importer has).
    let mut tiles: Vec<Vec<u8>> = regions.iter().map(|r| vec![0u8; r.w * r.h]).collect();
    let mut painted: Vec<Vec<bool>> = regions.iter().map(|r| vec![false; r.w * r.h]).collect();
    let src_texel = |x: usize, y: usize| -> u8 {
        if x >= src_width || y >= PAGE_HEIGHT {
            return 0;
        }
        indices[y * src_width + x]
    };
    for (ci, c) in clusters.iter().enumerate() {
        let (ri, dx, dy) = placement[ci];
        let s = c.scale as usize;
        for &fi in &c.faces {
            let f = &faces[fi];
            let p = &objects[f.obj].groups[f.group].prims[f.prim];
            let src_pal = (p.cba & 0x3F) as usize;
            let group = group_of[&(src_pal as u8)];
            let colors = &group_colors[group];
            // Paint the face's bbox region (conservative cover).
            let fb = f.bbox;
            for sy in (fb.1 as usize..=fb.3 as usize).step_by(s) {
                for sx in (fb.0 as usize..=fb.2 as usize).step_by(s) {
                    let tx = dx + (sx - c.bbox.0 as usize) / s;
                    let ty = dy + (sy - c.bbox.1 as usize) / s;
                    let r = &regions[ri];
                    if tx >= r.w || ty >= r.h {
                        continue;
                    }
                    let at = ty * r.w + tx;
                    if painted[ri][at] {
                        continue;
                    }
                    let idx = src_texel(sx, sy) as usize;
                    let color = cluts[src_pal][idx];
                    let new_idx = if idx == 0 || color == 0 {
                        0
                    } else {
                        match colors.iter().position(|&cc| cc == color) {
                            Some(pos) => pos + 1,
                            None => nearest_color(colors, color) + 1,
                        }
                    };
                    tiles[ri][at] = new_idx as u8;
                    painted[ri][at] = true;
                }
            }
        }
    }
    // Rewrite the faces' UV / CBA / TSB.
    for (ci, c) in clusters.iter().enumerate() {
        let (ri, dx, dy) = placement[ci];
        let r = &regions[ri];
        let s = c.scale as usize;
        for &fi in &c.faces {
            let f = &faces[fi];
            let p = &mut objects[f.obj].groups[f.group].prims[f.prim];
            let src_pal = (p.cba & 0x3F) as u8;
            let group = group_of[&src_pal];
            let col = col_of_group[group];
            for uv in p.uvs.iter_mut() {
                let nx = r.x0 + dx + (uv.0 as usize - c.bbox.0 as usize) / s;
                let ny = r.y0 + dy + (uv.1 as usize - c.bbox.1 as usize) / s;
                *uv = (nx.min(UV_SPACE - 1) as u8, ny.min(PAGE_HEIGHT - 1) as u8);
            }
            // Authoring CBA: CLUT row 480, column * 16 halfwords.
            p.cba = (480u16 << 6) | col;
            // Authoring TSB: page 0x15/0x16, ABR bits preserved, 4bpp.
            let page = if r.page == 0 { 0x15u16 } else { 0x16 };
            p.tsb = (p.tsb & 0x0060) | page;
        }
    }

    // Build the per-section pool blocks. Section 0 carries the whole
    // palette run; the others upload pixels only. A section may be split
    // into several sub-regions (the live-face window of section 1) - the
    // emitted pool always covers the section's FULL rect, with each
    // sub-region's tile blitted at its offset and unplaced texels (the
    // face window) left zero.
    let mut section_pools: Vec<Vec<u8>> = Vec::with_capacity(SECTION_TEXTURE_RECTS.len());
    for (section, rect) in SECTION_TEXTURE_RECTS.iter().enumerate() {
        let hw_x = rect.x0 as usize;
        let sec_x0 = if hw_x < 0x40 { hw_x } else { hw_x - 0x40 } * 4;
        let sec_y0 = rect.y0 as usize;
        let (sec_w, sec_h) = (rect.w as usize * 4, rect.h as usize);
        let mut full = vec![0u8; sec_w * sec_h];
        for (ri, r) in regions.iter().enumerate() {
            if r.section != section {
                continue;
            }
            let (dx, dy) = (r.x0 - sec_x0, r.y0 - sec_y0);
            let tile = &tiles[ri];
            for y in 0..r.h {
                let dst = (dy + y) * sec_w + dx;
                full[dst..dst + r.w].copy_from_slice(&tile[y * r.w..(y + 1) * r.w]);
            }
        }
        let mut block = Vec::new();
        if section == 0 {
            // One contiguous run covering our columns is not guaranteed
            // (free columns may be scattered), so emit the run from the
            // lowest to the highest used column, holes zero-filled -
            // zero entries upload as transparent and nothing samples
            // the hole columns.
            let lo = *col_of_group.iter().min().unwrap();
            let hi = *col_of_group.iter().max().unwrap();
            let n = (hi - lo + 1) * 16;
            block.extend_from_slice(&(lo * 16).to_le_bytes());
            block.extend_from_slice(&n.to_le_bytes());
            let mut run = vec![0u16; n as usize];
            for (g, colors) in group_colors.iter().enumerate() {
                let base = ((col_of_group[g] - lo) * 16) as usize;
                for (i, &c) in colors.iter().enumerate() {
                    run[base + 1 + i] = c;
                }
            }
            for w in run {
                block.extend_from_slice(&w.to_le_bytes());
            }
        } else {
            block.extend_from_slice(&0u16.to_le_bytes());
            block.extend_from_slice(&0u16.to_le_bytes());
        }
        // Pixels: pack the tile's texel indices, low nibble first.
        for y in 0..sec_h {
            for xb in 0..sec_w / 2 {
                let lo = full[y * sec_w + xb * 2] & 0xF;
                let hi = full[y * sec_w + xb * 2 + 1] & 0xF;
                block.push(lo | (hi << 4));
            }
        }
        section_pools.push(block);
    }
    Ok(BandLayout { section_pools })
}
