//! Player-side of the party swap: rebuild a `PLAYERn` battle file so the
//! character wears a Delilas sibling's model under their own animations,
//! whatever equipment is worn.
//!
//! Every equipment section record embeds its own slice of the skeleton
//! (`select_sections` picks by equipped id), so a swap that survives
//! equipment changes must rewrite **every** section record. The whole
//! PROT entry is rebuilt: header + `record[0]` (kept verbatim - it is
//! the character's own animation/palette/face source) + a rewritten
//! descriptor chain + one rewritten record per equipment id.
//!
//! Per record, everything except the mesh and the pool survives byte
//! for byte: the 0x14-byte header (offsets updated), the attach-object
//! offsets, the loader frame's attach list, and the swing/attach action
//! records that sit between the mesh end and `body_end` (shifted
//! whole). The embedded TMD keeps its retail object count - skeleton
//! objects get the baked Delilas part for their bone, surplus
//! (equipment-visual) objects go empty. The pool becomes the section's
//! fixed VRAM tile of the re-laid Delilas texture page, and every
//! record's upload flag is forced on so the full band uploads
//! regardless of equipment.
//!
//! Palette columns: all battle palette bands flow through the section
//! pools' trailing CLUT structs plus `record[0]`'s CLUT A/B structs
//! (`battle_char_palette`). The sections are ours, so the free columns
//! are `16 - record[0]'s claims`; the Delilas palettes union-merge down
//! to fit (two 16-colour palettes merge when their union still fits,
//! texel indices remapped during the island copy).

use super::*;
use crate::battle_char_assembly::{SECTION_TEXTURE_RECTS, record0_texture_uploads};
use crate::battle_data_pack::decode_record;
/// Bytes per stored monster CLUT (16 BGR555 entries).
const CLUT_BYTES: usize = 32;

/// The rebuilt player file.
#[derive(Debug, Clone)]
pub struct PlayerizedFile {
    /// The full PROT-entry bytes (same length as `entry_len`).
    pub file: Vec<u8>,
    pub warnings: Vec<String>,
}

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

/// The five section tiles in authoring-page space (see
/// `SECTION_TEXTURE_RECTS`: band halfword x < 0x40 = page 0x15, else
/// 0x16; texels = halfwords * 4).
fn band_regions() -> Vec<BandRegion> {
    SECTION_TEXTURE_RECTS
        .iter()
        .enumerate()
        .map(|(section, r)| {
            let hw_x = r.x0 as usize;
            let (page, page_hw_x) = if hw_x < 0x40 {
                (0, hw_x)
            } else {
                (1, hw_x - 0x40)
            };
            BandRegion {
                page,
                x0: page_hw_x * 4,
                y0: r.y0 as usize,
                w: r.w as usize * 4,
                h: r.h as usize,
                section,
            }
        })
        .collect()
}

/// Decode a monster pool into per-texel indices + CLUTs.
pub(super) fn monster_pool_texels(pool: &[u8]) -> Result<(Vec<[u16; 16]>, Vec<u8>, usize)> {
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
pub(super) fn merge_palettes(
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
pub(super) fn nearest_color(colors: &[u16], c: u16) -> usize {
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
struct BandLayout {
    /// Per-section `[u16 clut_x][u16 clut_n][cluts][pixels]` pool bytes.
    section_pools: Vec<Vec<u8>>,
}

/// Re-layout the Delilas page into the five section tiles, rewriting the
/// baked objects' UV/CBA/TSB in place (player authoring conventions:
/// CLUT row 480, texpages 0x15/0x16).
fn relayout_to_band(
    objects: &mut [ModelObject],
    cluts: &[[u16; 16]],
    indices: &[u8],
    src_width: usize,
    reserved_cols: &[u16],
    base_scale: u32,
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
    let regions = band_regions();
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
    // palette run; the others upload pixels only.
    let mut section_pools: Vec<Vec<u8>> = Vec::with_capacity(regions.len());
    for (ri, r) in regions.iter().enumerate() {
        let mut block = Vec::new();
        if r.section == 0 {
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
        let tile = &tiles[ri];
        for y in 0..r.h {
            for xb in 0..r.w / 2 {
                let lo = tile[y * r.w + xb * 2] & 0xF;
                let hi = tile[y * r.w + xb * 2 + 1] & 0xF;
                block.push(lo | (hi << 4));
            }
        }
        section_pools.push(block);
    }
    // Regions are listed per section 0..4 in order.
    Ok(BandLayout { section_pools })
}

/// Object index of a section's **equipment-variant** object, given its
/// attach count and object count - `None` when the section has no surplus.
///
/// A section's object list is `[attach_count bone objects][surplus]`, and
/// the splice `FUN_800536BC` tags the surplus `0xFF` (the **first**) /
/// `0xFE` (the rest). The two tags are different animals:
///
/// * **`0xFF` - the equipment VARIANT.** The post-pass `FUN_80053898`
///   sorts it past every drawn channel and records the preceding object's
///   bone as its attach channel; `FUN_800513F0` then snapshots the
///   `(default, variant)` object-pointer pair, and the per-frame pass
///   `FUN_8004CCD4` installs one or the other **into the attach bone's own
///   channel** of the render node's model table - the variant while a
///   window of the playing entry's `+0xA4..+0xAB` track is open (or
///   unconditionally, on its extra-channel escape), the default
///   otherwise. So the variant is not a separate part hanging off the
///   hand: it IS the hand, for those frames.
/// * **`0xFE` - an extra animated part**, seated directly after the
///   skeleton bones and posed by a pose channel of its own (the
///   extra-channel swings). It draws alongside the hand, not instead
///   of it.
///
/// Leaving both empty therefore reads very differently. An empty `0xFE`
/// simply drops the equipment visual, which is what the swap wants. An
/// empty `0xFF` **deletes the attach bone's hand** for every frame a
/// window is open - the channel's model pointer is replaced, so the baked
/// part it was drawing is gone, not merely un-decorated.
///
/// Retail's own variant is the same mesh as its attach bone (byte-equal
/// vertices and prims on Vahn / Gala / Terra; same topology, alternate
/// vertices on Noa), which is why the swap never wants a hole here. Live
/// windows exist only in **Noa's** file on the USA disc, and the entry the
/// **Spirit** command plays - art-bank record 0, staged as anim id `0x10`
/// by the command dispatcher `FUN_801D0748` at `0x801D16B0` - is one of
/// them (`+0xA8 = [1, 53]` over a 58-frame clip, pair 1 = the armB hand).
pub(crate) fn variant_object(attach_count: usize, nobj: usize) -> Option<usize> {
    (attach_count > 0 && nobj > attach_count).then_some(attach_count)
}

/// Point the attach bone's object-table entry at the variant's, so the two
/// share one copy of the geometry.
///
/// The variant has to carry the same mesh as its attach bone, and the
/// budget will not pay for a second copy: the sections repeat per
/// equipment id (Vahn's arm sections alone hold 30 records), and duplicating
/// a hand into every one of them costs Gala's PROT entry enough to force
/// the whole band down to half texture resolution. Two object-table
/// entries reading one data region cost nothing, and nothing can tell:
/// both the retail splice `FUN_800536BC` and the port's
/// `battle_char_assembly` relocate every entry by one per-section delta and
/// then address the data purely through it, and the retail draw pass reads
/// `prim_top` / `n_primitive` off the entry it was handed.
///
/// The direction matters. The geometry is emitted in the **variant's**
/// slot and the bone's entry is aliased FORWARD onto it, never the other
/// way round: the variant is the section's last object whenever there is
/// exactly one surplus, and `rewrite_section_record` reads a retail TMD's
/// length off its last object's `normal_top`, so an entry aliased backwards
/// would leave that reading short of the real end.
fn alias_variant_onto_bone(tmd: &mut [u8], variant: usize) -> Result<()> {
    let entry = |i: usize| legaia_tmd::HEADER_SIZE + i * legaia_tmd::OBJECT_SIZE;
    let (src, dst) = (entry(variant), entry(variant - 1));
    if src + legaia_tmd::OBJECT_SIZE > tmd.len() {
        bail!("variant object {variant} past the encoded object table");
    }
    let copy: Vec<u8> = tmd[src..src + legaia_tmd::OBJECT_SIZE].to_vec();
    tmd[dst..dst + legaia_tmd::OBJECT_SIZE].copy_from_slice(&copy);
    Ok(())
}

/// Rebuild one section record around the swapped geometry + pool.
fn rewrite_section_record(
    decoded: &[u8],
    channel_geom: &BTreeMap<u8, ModelObject>,
    pool_block: &[u8],
) -> Result<Vec<u8>> {
    let u32at = |o: usize| -> Result<u32> {
        legaia_bytes::u32_le(decoded, o).ok_or_else(|| anyhow::anyhow!("short record at +{o:#x}"))
    };
    let frame_off = u32at(0)? as usize;
    let swing_a = u32at(4)? as usize;
    let swing_b = u32at(8)? as usize;
    let body_end = u32at(0xC)? as usize;
    let attach_obj_count = i16::from_le_bytes(
        decoded
            .get(0x10..0x12)
            .ok_or_else(|| anyhow::anyhow!("short record"))?
            .try_into()
            .unwrap(),
    ) as usize;
    if body_end > decoded.len() || frame_off < 0x14 || frame_off >= body_end {
        bail!("section record header out of range");
    }
    // Loader frame.
    let attach_count = decoded[frame_off] as usize;
    let tmd_off = frame_off + 0xC;
    let old_data_size = u32at(frame_off + 8)? as usize;
    let tmd = legaia_tmd::parse(&decoded[tmd_off..]).context("section TMD")?;
    let nobj = tmd.objects.len();
    if attach_count > nobj {
        bail!("attach_count {attach_count} > nobj {nobj}");
    }
    let bone_ids: Vec<u8> = decoded[frame_off + 1..frame_off + 1 + attach_count].to_vec();

    // New object list. A section's objects have three roles (see
    // [`variant_object`]): the first `attach_count` bind to skeleton bones
    // and wear the baked Delilas part for theirs; the FIRST surplus is the
    // section's `0xFF` equipment VARIANT of the last attached bone and has
    // to wear that same part, because the render pass swaps it INTO that
    // bone's channel; any further surplus is a `0xFE` extra animated part
    // with a pose channel of its own and stays empty.
    //
    // The attach bone and its variant share one copy of the geometry: it
    // is emitted in the VARIANT's slot and the bone's own slot goes out
    // empty, its object-table entry aliased onto the variant's below.
    let variant = variant_object(attach_count, nobj);
    let mut objects: Vec<ModelObject> = Vec::with_capacity(nobj);
    for k in 0..nobj {
        let bone = match variant {
            Some(v) if k + 1 == v => None,
            Some(v) if k == v => bone_ids.last().copied(),
            _ if k < attach_count => bone_ids.get(k).copied(),
            _ => None,
        };
        objects.push(
            bone.and_then(|b| channel_geom.get(&b))
                .cloned()
                .unwrap_or_else(|| ModelObject {
                    vertices: Vec::new(),
                    groups: Vec::new(),
                    scale: legaia_tmd::encode::LEGAIA_OBJECT_SCALE,
                }),
        );
    }
    let mut new_tmd = encode(&objects).context("encode section TMD")?;
    if let Some(v) = variant {
        alias_variant_onto_bone(&mut new_tmd, v).context("alias the equipment-variant object")?;
    }

    // Old TMD byte extent (up to the swing region / body_end).
    let old_last = tmd
        .objects
        .last()
        .ok_or_else(|| anyhow::anyhow!("section TMD has no objects"))?;
    let old_tmd_len = legaia_tmd::HEADER_SIZE + old_last.header.normal_top as usize;
    let old_tail_start = tmd_off + old_tmd_len;
    if old_tail_start > body_end {
        bail!("section TMD runs past body_end");
    }

    // Assemble: [0x00..tmd_off verbatim][new TMD][tail shifted][pool].
    let mut out = Vec::with_capacity(decoded.len());
    out.extend_from_slice(&decoded[..tmd_off]);
    out.extend_from_slice(&new_tmd);
    while out.len() % 4 != 0 {
        out.push(0);
    }
    let delta = out.len() as i64 - old_tail_start as i64;
    out.extend_from_slice(&decoded[old_tail_start..body_end]);
    let new_body_end = out.len();
    out.extend_from_slice(pool_block);

    // Header fixups.
    let put_u32 =
        |out: &mut [u8], o: usize, v: u32| out[o..o + 4].copy_from_slice(&v.to_le_bytes());
    if swing_a != 0 {
        put_u32(&mut out, 4, (swing_a as i64 + delta) as u32);
    }
    if swing_b != 0 {
        put_u32(&mut out, 8, (swing_b as i64 + delta) as u32);
    }
    put_u32(&mut out, 0xC, new_body_end as u32);
    // Upload flag on: the pool must land even on retail flag-0 records
    // (bare-fist / no-Ra-Seru defaults), or the band tile stays stale.
    out[0x12] = 1;
    out[0x13] = 0;
    for i in 0..attach_obj_count {
        let off = u32at(0x14 + i * 4)? as usize;
        if off >= old_tail_start && off < body_end {
            put_u32(&mut out, 0x14 + i * 4, (off as i64 + delta) as u32);
        } else if off >= body_end {
            bail!("attach-object record beyond body_end");
        }
    }
    // Frame data_size shifts with the TMD size change.
    let new_data_size = (old_data_size as i64 + (new_tmd.len() as i64 - old_tmd_len as i64)) as u32;
    put_u32(&mut out, frame_off + 8, new_data_size);
    Ok(out)
}

/// One rig's own body frame - `[up, lateral, forward]` - from landmarks
/// all six rigs carry: up = pelvis pivot -> head pivot, lateral = the two
/// shoulder pivots, forward = their cross. Unlike a joint's bend plane it
/// never degenerates and never depends on how one rig happens to author a
/// spine.
///
/// `pivots` is indexed in [`CANONICAL_PARTS`] order (0 head, 2 pelvis,
/// 3/6 the two shoulders) - the landmarks are those slots, not a search.
fn body_axes(pivots: &[[f32; 3]]) -> [[f32; 3]; 3] {
    let unit = |v: [f32; 3], fallback: [f32; 3]| {
        let l = vnorm(v);
        if l < 1e-3 {
            fallback
        } else {
            [v[0] / l, v[1] / l, v[2] / l]
        }
    };
    let up = unit(vsub(pivots[0], pivots[2]), [0.0, -1.0, 0.0]);
    let lat = vsub(pivots[6], pivots[3]);
    let d = vdot(lat, up);
    let lat = unit(
        [lat[0] - up[0] * d, lat[1] - up[1] * d, lat[2] - up[2] * d],
        [0.0, 0.0, 1.0],
    );
    [up, lat, unit(vcross(up, lat), [1.0, 0.0, 0.0])]
}

/// The rotation taking unit `a` onto unit `b` about their common
/// perpendicular - the swing that adds no twist of its own (Rodrigues).
/// `None` when the two are antiparallel and that axis is undefined.
fn swing_rotation(a: [f32; 3], b: [f32; 3]) -> Option<[[f32; 3]; 3]> {
    let ident = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let v = vcross(a, b);
    let c = vdot(a, b).clamp(-1.0, 1.0);
    let s = vnorm(v);
    if s < 1e-6 {
        return (c > 0.0).then_some(ident);
    }
    let k = [v[0] / s, v[1] / s, v[2] / s];
    let kx = [[0.0, -k[2], k[1]], [k[2], 0.0, -k[0]], [-k[1], k[0], 0.0]];
    let (st, ct) = s.atan2(c).sin_cos();
    let mut r = ident;
    for (i, row) in r.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            let kk: f32 = (0..3).map(|q| kx[i][q] * kx[q][j]).sum();
            *cell = ident[i][j] + st * kx[i][j] + (1.0 - ct) * kk;
        }
    }
    Some(r)
}

/// The rest bone frames the bake aligns through, with each part's TWIST
/// about its bone referenced to the rig's own body instead of to the
/// joint's bend plane.
///
/// [`bone_frames`] pins the roll with the adjacent chain bone (the elbow /
/// knee bend plane), falling through to a world axis when that chain is
/// straight. Across two independently authored rigs that reference is not
/// comparable, in two ways that both roll the baked part about its own
/// axis:
///
/// * **Reference kinds disagree.** Che is the one sibling whose
///   pelvis->torso bone is measurable (20 units), so HIS torso takes its
///   bend plane from real anatomy while every player torso - pelvis pivot
///   sitting on the torso pivot - falls through to world Z. The two y axes
///   come out at `y . y = -0.98`, so the alignment rolled Che's chest
///   167.9 degrees about his own spine: he wore his torso backwards, and
///   the shoulder tuck, which seats the arms where the SIBLING's sockets
///   land through the torso bake, then dragged both arms round with it.
/// * **Bend planes diverge.** Even with real anatomy on both sides two
///   rigs flex their joints in unrelated directions - Gi's armA elbow
///   plane sits 122 degrees from Noa's - so his upper arm baked rolled 166
///   degrees about its own axis.
///
/// The replacement carries no per-part reference at all: turn the WHOLE
/// rig onto the host's facing (`R_body`, the one rotation taking the
/// sibling's body frame onto the host's), then swing each part minimally
/// onto its own host bone. Writing the destination frame as `R_ideal *
/// F_src` makes [`frame_align`] return exactly that composite. Nothing is
/// left free for a joint plane to get wrong, and a part's roll relative to
/// the body it hangs on is carried across the swap unchanged - which is
/// what the enemy-table preview shows and what the report compared against.
///
/// Terminal parts (head, hands, feet) keep the frame [`bone_frames`]
/// inherited for them: [`normalize_battle_rest_feet`] already pins their
/// world orientation by pre-cancelling *that* frame's alignment, and the
/// two have to describe the same rotation or the cancellation stops
/// cancelling.
///
/// Measured as excess roll - what the bake rotates a part by beyond that
/// re-facing and swing - worst non-terminal part per cast: Gi 152.5 -> 0,
/// Che 114.0 -> 0, Lu (the slot that already read right) 44.3 -> 0. The
/// rotation is right by construction, so the numbers that carry weight are
/// the geometric ones the correction was not fitted to. The per-part
/// affine fit of the bake loses the shear the old roll dragged through the
/// socket tucks - Che's armA upper went from principal scales
/// 1.01/0.75/0.24 (squashed to a quarter of its width in one direction)
/// to 0.81/0.75/0.65, its non-affine residual 0.069 -> 0.020 - and no
/// chain edge opens past BOTH the retail host's own idle envelope and the
/// sibling's, before or after.
///
/// NB [`winpose::retarget_clip`](super::winpose) cancels this bake by
/// rebuilding the SAME alignment out of [`bone_frames`]; it has to build
/// its frames here too, or its conjugation stops cancelling by exactly the
/// correction this applies (measured: up to 152.5 degrees on Gi, 114.0 on
/// Che, 44.3 on Lu).
///
/// Both pivot arrays are in [`CANONICAL_PARTS`] order - [`body_axes`] reads
/// its landmarks off fixed slots - so a caller on another rig topology gets
/// the plain [`bone_frames`] pair back by way of the length guard only if
/// that topology is shorter, not if it merely orders its parts differently.
pub(crate) fn bake_frames(
    src_pivots: &[[f32; 3]],
    dst_pivots: &[[f32; 3]],
    child: &[Option<usize>],
    parent: &[Option<usize>],
) -> (Vec<BoneFrame>, Vec<BoneFrame>) {
    let mut src = bone_frames(src_pivots, child, parent);
    let mut dst = bone_frames(dst_pivots, child, parent);
    if src_pivots.len() < CANONICAL_PARTS || dst_pivots.len() < CANONICAL_PARTS {
        return (src, dst);
    }
    let (bs, bd) = (body_axes(src_pivots), body_axes(dst_pivots));
    // Turn the whole rig onto the host's facing: read the vector in the
    // sibling's body coordinates, rebuild it in the host's.
    let reface = |v: [f32; 3]| -> [f32; 3] {
        let c = [vdot(v, bs[0]), vdot(v, bs[1]), vdot(v, bs[2])];
        [
            bd[0][0] * c[0] + bd[1][0] * c[1] + bd[2][0] * c[2],
            bd[0][1] * c[0] + bd[1][1] * c[1] + bd[2][1] * c[2],
            bd[0][2] * c[0] + bd[1][2] * c[1] + bd[2][2] * c[2],
        ]
    };
    for (s, d) in src.iter_mut().zip(dst.iter_mut()) {
        // Only parts with a bone of their own: a terminal's frame is
        // inherited, and is the one the feet normalisation cancels.
        if s.len.is_none() || d.len.is_none() || !s.real || !d.real {
            continue;
        }
        let xd = d.axes[0];
        // The re-faced source bone. Antiparallel to the host's leaves the
        // swing axis undefined; such a part keeps the bend-plane frame.
        let Some(swing) = swing_rotation(reface(s.axes[0]), xd) else {
            continue;
        };
        let yd = apply(&swing, reface(s.axes[1]));
        d.axes = [xd, yd, vcross(xd, yd)];
    }
    (src, dst)
}

/// Rebuild a `PLAYERn` battle file so the character wears the Delilas
/// model of `source_id` (162/163/164). `entry_len` is the PROT entry's
/// exact byte length (the output is padded to it).
pub fn playerize_player_file(
    player_file: &[u8],
    entry_len: usize,
    rig: &PlayerRig,
    archive_entry: &[u8],
    source_id: u16,
) -> Result<PlayerizedFile> {
    let mut last_err = None;
    for downscale in [1u32, 2, 4] {
        match playerize_scaled(
            player_file,
            entry_len,
            rig,
            archive_entry,
            source_id,
            downscale,
        ) {
            Ok(out) => return Ok(out),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("playerize failed")))
        .context("rebuilt player file does not fit its PROT entry at any texture resolution")
}

fn playerize_scaled(
    player_file: &[u8],
    entry_len: usize,
    rig: &PlayerRig,
    archive_entry: &[u8],
    source_id: u16,
    texture_downscale: u32,
) -> Result<PlayerizedFile> {
    let mut warnings = Vec::new();
    if texture_downscale > 1 {
        warnings.push(format!(
            "texture at 1/{texture_downscale} resolution to fit the player file"
        ));
    }
    let pack = battle_data_pack::parse(player_file).context("parse player battle file")?;

    // Source model: the Delilas mesh + pool + rest pose.
    let mesh = monster_archive::mesh(archive_entry, source_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: empty slot"))?;
    let src_tmd = legaia_tmd::parse(mesh.tmd_bytes()).context("delilas TMD")?;
    let src_model = decode_model(&src_tmd, mesh.tmd_bytes())?;
    if src_model.len() != CANONICAL_PARTS {
        bail!("monster id {source_id} has {} parts", src_model.len());
    }
    let pool = mesh
        .texture_pool_bytes()
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: no texture pool"))?;
    let (cluts, indices, src_width) = monster_pool_texels(pool)?;
    let src_idle = monster_archive::idle_animation(archive_entry, source_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: no idle"))?;
    let mut src_rest = src_idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("monster idle empty"))?
        .clone();

    // Player rest pose + the retail per-channel part anchors.
    let idle = battle_char_assembly::idle_battle_animation(player_file)?
        .ok_or_else(|| anyhow::anyhow!("player file has no idle"))?;
    let dst_rest = idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("player idle empty"))?
        .clone();
    // Player-shaped ankles (mirrored in `winpose` - the two must agree).
    normalize_battle_rest_feet(&mut src_rest, &dst_rest, rig);
    let asm = battle_char_assembly::assemble_character(player_file, &pack, &[0; SECTION_COUNT])?;
    let dst_tmd = legaia_tmd::parse(&asm.tmd)?;
    let dst_model = decode_model(&dst_tmd, &asm.tmd)?;

    // Bake each canonical part into its player channel's rest frame,
    // pivot-anchored (`bake_object_pivot`): the pivot is the joint the
    // engine rotates the part about, the bone frames re-aim the part
    // from the sibling's rest stance onto the player's (twist pinned by
    // the bend plane), and the axial scale puts the part's far end on
    // the player's child joint - so the player's OWN clips (run / arts
    // / block / spirit) move each baked part exactly like the retail
    // geometry it replaced, joints staying closed. The radial scale is
    // UNIFORM (whole-rig height ratio) so the sibling's shapes survive.
    let skeleton = dst_rest.len();
    let dst_stats: Vec<PartStats> = (0..CANONICAL_PARTS)
        .map(|c| {
            let ch = rig.channel_for_canonical[c] as usize;
            let dst_core = dst_model
                .get(ch)
                .filter(|_| ch < skeleton)
                .ok_or_else(|| anyhow::anyhow!("player model missing channel {ch}"))?;
            let dst_pose = dst_rest
                .get(ch)
                .ok_or_else(|| anyhow::anyhow!("player rest pose missing channel {ch}"))?;
            Ok(part_world_stats(dst_core, dst_pose))
        })
        .collect::<Result<_>>()?;
    let src_stats: Vec<PartStats> = src_model
        .iter()
        .enumerate()
        .map(|(c, o)| part_world_stats(o, &src_rest[c]))
        .collect();
    let radial = global_height_scale(&src_stats, &dst_stats)[0];
    let pivot_of = |p: &PartPose| [p.tx as f32, p.ty as f32, p.tz as f32];
    let src_pivots: Vec<[f32; 3]> = src_rest
        .iter()
        .take(CANONICAL_PARTS)
        .map(pivot_of)
        .collect();
    let dst_pivots: Vec<[f32; 3]> = (0..CANONICAL_PARTS)
        .map(|c| pivot_of(&dst_rest[rig.channel_for_canonical[c] as usize]))
        .collect();
    let (src_frames, dst_frames) = bake_frames(
        &src_pivots,
        &dst_pivots,
        &CANONICAL_CHILD,
        &CANONICAL_PARENT,
    );
    let mut baked: Vec<ModelObject> = Vec::with_capacity(CANONICAL_PARTS);
    for (c, src_obj) in src_model.iter().enumerate() {
        let ch = rig.channel_for_canonical[c] as usize;
        let mut o = src_obj.clone();
        // The torso keeps the AXIAL fit like every chained part: its
        // bone spans torso->head pivot, and the host's head + shoulder
        // pivots are authored at that span's end - a uniform-scaled
        // long torso (Che: 164 vs Gala's 90) towers past them, so the
        // head pokes out mid-hump and the shoulder pads ride above the
        // head (the in-game screenshot). The axial crush is the price
        // of attachment; width stays radial.
        let pb = pivot_bake_params(&src_frames[c], &dst_frames[c], radial);
        bake_object_pivot(&mut o, &src_rest[c], src_pivots[c], &dst_rest[ch], &pb)
            .with_context(|| format!("bake canonical part {c}"))?;
        if c == 0 {
            // Seat the head on the neck: its near edge along the bone
            // axis lands where the replaced head's sat.
            if let Some(dst_head) = dst_model.get(ch) {
                seat_terminal_axial(&mut o, dst_head, &dst_rest[ch], pb.x_dst)?;
            }
        }
        if c == 5 || c == 8 {
            // Seat each HAND by centroid match: translate the baked fist
            // so its local centroid lands where the replaced hand's was.
            // A sibling's hand can be authored far from its own pivot
            // (Gi's armA fist sits 60 units out - his rig swings it from
            // up the arm; Che's hammer-fist 150), and the player channels
            // anchor the pivot AT the wrist, so an unseated fist floats a
            // forearm-length off the arm (the move-input floater in the
            // `delilas_gi_move_input_hand_gap` catalogue state, pinned to
            // the armA hand object by live bisection). The head's axial
            // near-edge seat is the wrong instrument here (measured: it
            // slides Gi's fist further out) - a hand is a closed blob
            // whose only seam is the wrist, and matching centroids puts
            // it exactly in the local region the retail hand occupied
            // under every clip. Feet stay unseated: their ankles go
            // through `normalize_battle_rest_feet` and re-seating would
            // fight that alignment.
            if let Some(dst_part) = dst_model.get(ch) {
                let cen = |obj: &ModelObject| -> Option<[f32; 3]> {
                    if obj.vertices.is_empty() {
                        return None;
                    }
                    let n = obj.vertices.len() as f32;
                    let s = obj.vertices.iter().fold([0f32; 3], |a, v| {
                        [a[0] + v[0] as f32, a[1] + v[1] as f32, a[2] + v[2] as f32]
                    });
                    Some([s[0] / n, s[1] / n, s[2] / n])
                };
                if let (Some(cb), Some(cd)) = (cen(&o), cen(dst_part)) {
                    let shift = [cd[0] - cb[0], cd[1] - cb[1], cd[2] - cb[2]];
                    for v in o.vertices.iter_mut() {
                        *v = [
                            round_coord(v[0] as f32 + shift[0])?,
                            round_coord(v[1] as f32 + shift[1])?,
                            round_coord(v[2] as f32 + shift[2])?,
                        ];
                    }
                }
            }
        }
        compact_object(&mut o);
        baked.push(o);
    }

    // NB a "buried-head lift" (raise the head when its rest CENTROID
    // sat below the torso top) was tried here and REVERTED: a tall
    // helmet biases the centroid low even when the face is seated
    // correctly, so the lift hoisted Gi's whole head ~25 units off the
    // neck - in-game (where head channels animate) that read as a
    // DETACHED head with sky through the gap. `seat_terminal_axial`
    // alone owns the neck seam.

    // Hip-clearance taper: shrink each thigh's radial extent near the
    // hip (12% at the pivot, fading to zero 60% down the bone) so the
    // thigh's upper rim stops cutting through the pelvis skirt when the
    // host's clips swing the leg (Lu's thigh clipped through her rear).
    for c in [9usize, 12usize] {
        let Some(len) = dst_frames[c].len.filter(|l| *l >= 2.0) else {
            continue;
        };
        let ch = rig.channel_for_canonical[c] as usize;
        let md = rot_matrix(&dst_rest[ch]);
        let ax = apply_transposed(&md, dst_frames[c].axes[0]);
        for v in baked[c].vertices.iter_mut() {
            let p = [v[0] as f32, v[1] as f32, v[2] as f32];
            let t = p[0] * ax[0] + p[1] * ax[1] + p[2] * ax[2];
            let w = (1.0 - t / (0.6 * len)).clamp(0.0, 1.0) * 0.12;
            let perp = [p[0] - ax[0] * t, p[1] - ax[1] * t, p[2] - ax[2] * t];
            *v = [
                round_coord(p[0] - perp[0] * w)?,
                round_coord(p[1] - perp[1] * w)?,
                round_coord(p[2] - perp[2] * w)?,
            ];
        }
    }

    // Seat the upper arms on the sibling's OWN shoulder sockets. The
    // arms hang at the PLAYER's shoulder pivots (the clips' channel
    // translations dictate that), but the baked torso's socket surface
    // lands where the SIBLING's shoulder sits after the torso bake -
    // on a narrower rig the arm floats outboard of the body (Lu's
    // shoulder gap: her torso wraps her sockets at ~4 units, the baked
    // pair left ~26). The shoulder END of each upper arm tucks in by
    // the socket delta, TAPERING to zero at the elbow: a rigid
    // whole-chain shift instead makes the per-part offsets rotate
    // apart mid-swing and opens the elbow/wrist, so the elbow end (and
    // the forearm + hand parts) must stay exactly where the clips
    // expect them.
    {
        let pb_torso = pivot_bake_params(&src_frames[1], &dst_frames[1], radial);
        for c in [3usize, 6usize] {
            let socket = bake_point_pivot(src_pivots[c], src_pivots[1], dst_pivots[1], &pb_torso);
            let delta = vsub(socket, dst_pivots[c]);
            let Some(len) = dst_frames[c].len.filter(|l| *l >= 2.0) else {
                continue;
            };
            let ch = rig.channel_for_canonical[c] as usize;
            let md = rot_matrix(&dst_rest[ch]);
            let ld = apply_transposed(&md, delta);
            let ax = apply_transposed(&md, dst_frames[c].axes[0]);
            for v in baked[c].vertices.iter_mut() {
                let t = v[0] as f32 * ax[0] + v[1] as f32 * ax[1] + v[2] as f32 * ax[2];
                let w = (1.0 - t / len).clamp(0.0, 1.0);
                *v = [
                    round_coord(v[0] as f32 + ld[0] * w)?,
                    round_coord(v[1] as f32 + ld[1] * w)?,
                    round_coord(v[2] as f32 + ld[2] * w)?,
                ];
            }
        }
    }

    // Hip-socket tuck - the shoulder tuck's mirror for the legs: the
    // thigh tops ride the HOST's hip pivots, which sit wider than a
    // slimmer sibling's authored hips, so the thigh's upper rim pokes
    // through the pelvis skirt from OUTSIDE (Lu's in-game hip overlap).
    // Each thigh's hip end tucks to where the sibling's own hip socket
    // lands through the pelvis bake, tapering to zero at the knee (the
    // knee/shin/foot stay exactly where the clips expect them).
    {
        let pb_pelvis = pivot_bake_params(&src_frames[2], &dst_frames[2], radial);
        for c in [9usize, 12usize] {
            let socket = bake_point_pivot(src_pivots[c], src_pivots[2], dst_pivots[2], &pb_pelvis);
            let delta = vsub(socket, dst_pivots[c]);
            let Some(len) = dst_frames[c].len.filter(|l| *l >= 2.0) else {
                continue;
            };
            let ch = rig.channel_for_canonical[c] as usize;
            let md = rot_matrix(&dst_rest[ch]);
            let ld = apply_transposed(&md, delta);
            let ax = apply_transposed(&md, dst_frames[c].axes[0]);
            for v in baked[c].vertices.iter_mut() {
                let t = v[0] as f32 * ax[0] + v[1] as f32 * ax[1] + v[2] as f32 * ax[2];
                let w = (1.0 - t / len).clamp(0.0, 1.0);
                *v = [
                    round_coord(v[0] as f32 + ld[0] * w)?,
                    round_coord(v[1] as f32 + ld[1] * w)?,
                    round_coord(v[2] as f32 + ld[2] * w)?,
                ];
            }
        }
    }

    // Texture re-layout (rewrites the baked objects' UV/CBA/TSB).
    let reserved: Vec<u16> = record0_texture_uploads(player_file, 0)?
        .iter()
        .filter(|u| !u.clut.is_empty())
        .flat_map(|u| {
            let first = u.clut_x / 16;
            let n_cols = (u.clut.len() as u16).div_ceil(16);
            (first..first + n_cols).collect::<Vec<u16>>()
        })
        .collect();
    let layout = relayout_to_band(
        &mut baked,
        &cluts,
        &indices,
        src_width,
        &reserved,
        texture_downscale,
        &mut warnings,
    )?;

    // Per-channel geometry map.
    let mut channel_geom: BTreeMap<u8, ModelObject> = BTreeMap::new();
    for (c, o) in baked.into_iter().enumerate() {
        channel_geom.insert(rig.channel_for_canonical[c], o);
    }
    // The hair channel (Noa) intentionally has no entry - it goes empty.

    // Rewrite every record except record[0].
    let table_offset = pack.table_offset;
    // (id, slot bytes, decoded bytes - kept for the optimal-LZS retry)
    let mut new_records: Vec<(u32, Vec<u8>, Vec<u8>)> = Vec::new();
    let mut section = 0usize;
    for (idx, rec) in pack.records.iter().enumerate() {
        let decoded = decode_record(player_file, &pack, idx)
            .with_context(|| format!("decode record {idx}"))?;
        let pool_block = layout
            .section_pools
            .get(section)
            .ok_or_else(|| anyhow::anyhow!("record {idx}: section {section} out of range"))?;
        let rewritten = rewrite_section_record(&decoded.bytes, &channel_geom, pool_block)
            .with_context(|| format!("rewrite record {idx} (id {:#x})", rec.id))?;
        let stream = legaia_lzs::compress(&rewritten);
        let mut slot = Vec::with_capacity(4 + stream.len());
        slot.extend_from_slice(&(rewritten.len() as u32).to_le_bytes());
        slot.extend_from_slice(&stream);
        new_records.push((rec.id, slot, rewritten));
        if rec.id == 0 {
            section += 1;
        }
    }
    if section != SECTION_COUNT {
        bail!("descriptor chain closed after {section} sections");
    }

    // Greedy total first; pay for the optimal parse per record only when
    // the file misses its entry budget.
    let data_base = pack.data_base;
    let budget = entry_len - data_base;
    let total = |recs: &[(u32, Vec<u8>, Vec<u8>)]| -> usize {
        recs.iter()
            .map(|(_, s, _)| (s.len() + 0x7FF) & !0x7FF)
            .sum()
    };
    if total(&new_records) > budget {
        for (_, slot, decoded) in new_records.iter_mut() {
            let stream = legaia_lzs::compress_optimal(decoded);
            if 4 + stream.len() < slot.len() {
                slot.truncate(4);
                slot.extend_from_slice(&stream);
            }
        }
    }
    if total(&new_records) > budget {
        bail!(
            "rebuilt records need {} bytes, the PROT entry holds {}",
            total(&new_records),
            budget
        );
    }

    // Rebuild the file: everything below data_base verbatim except the
    // descriptor entries' offset/size fields; records repacked from
    // data_base with 0x800-aligned slots (retail record offsets are all
    // sector-aligned - keep that invariant for the CD-read path).
    let mut file = player_file[..data_base.min(player_file.len())].to_vec();
    if file.len() < data_base {
        bail!("player file shorter than its data base");
    }
    let mut offset = 0u32;
    for (i, (id, slot, _)) in new_records.iter().enumerate() {
        let size = (slot.len() as u32 + 0x7FF) & !0x7FF;
        let p = table_offset + i * 12;
        file[p..p + 4].copy_from_slice(&id.to_le_bytes());
        file[p + 4..p + 8].copy_from_slice(&offset.to_le_bytes());
        file[p + 8..p + 12].copy_from_slice(&size.to_le_bytes());
        offset += size;
    }
    // Terminator entry (all-zero) right after ours, if the retail table
    // had room for one.
    let term = table_offset + new_records.len() * 12;
    if term + 12 <= data_base {
        file[term..term + 12].fill(0);
    }
    for (_, slot, _) in &new_records {
        let start = file.len();
        file.extend_from_slice(slot);
        let padded = (start + slot.len() + 0x7FF) & !0x7FF;
        file.resize(padded.max(file.len()), 0);
    }
    if file.len() > entry_len {
        bail!(
            "rebuilt player file {} bytes exceeds the PROT entry ({} bytes)",
            file.len(),
            entry_len
        );
    }
    file.resize(entry_len, 0);
    Ok(PlayerizedFile { file, warnings })
}
