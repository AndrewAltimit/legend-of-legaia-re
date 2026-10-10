//! The field texture atlas: painting VRAM, the template fill and box, laying
//! out the head window, and patching the atlas and its TIM.

use super::super::*;
use super::slot::FieldSlot;
use super::{ATLAS_COLS_PER_CHAR, ATLAS_WINDOW_H, ATLAS_WINDOW_W, FIELD_CLUT_ROW, FIELD_TSB};
use crate::party_swap::playerize::{merge_palettes, nearest_color};
use legaia_tmd::descriptor::PacketShape;
use legaia_tmd::encode::ModelPrim;

/// Paint one TIM block (16-bit framebuffer units) into a 1024x512 VRAM
/// image.
pub(in super::super) fn paint_vram(
    vram: &mut [u16],
    fb_x: u16,
    fb_y: u16,
    w: u16,
    h: u16,
    data: &[u8],
) {
    for row in 0..h as usize {
        for col in 0..w as usize {
            let src = (row * w as usize + col) * 2;
            let (x, y) = (fb_x as usize + col, fb_y as usize + row);
            if src + 2 <= data.len() && x < 1024 && y < 512 {
                vram[y * 1024 + x] = u16::from_le_bytes(data[src..src + 2].try_into().unwrap());
            }
        }
    }
}

/// A tiny flat-colored box spanning `part`'s local bbox: the low-poly
/// equipment-template stand-in (mimics retail's ~16-vert world-map
/// variants; see the template block in `fieldize_slot`).
/// Average colour of a part's flat prims; a fully-textured part (the
/// head) falls back to the source palettes' average - the dominant
/// cloth/hair tone.
pub(super) fn template_fill_color(part: &ModelObject, cluts: &[[u16; 16]]) -> [u8; 3] {
    let mut acc = [0u32; 3];
    let mut n = 0u32;
    for g in &part.groups {
        if g.shape.is_textured() {
            continue;
        }
        for p in &g.prims {
            for c in &p.colors {
                for k in 0..3 {
                    acc[k] += c[k] as u32;
                }
                n += 1;
            }
        }
    }
    if n == 0 {
        let s5 = |x: u16| ((x << 3) | (x >> 2)) as u32;
        for pal in cluts {
            for &c in pal.iter().skip(1) {
                if c != 0 {
                    acc[0] += s5(c & 0x1F);
                    acc[1] += s5((c >> 5) & 0x1F);
                    acc[2] += s5((c >> 10) & 0x1F);
                    n += 1;
                }
            }
        }
    }
    let n = n.max(1);
    [(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8]
}

pub(super) fn template_box(
    part: &ModelObject,
    cluts: &[[u16; 16]],
    indices: &[u8],
    width: usize,
) -> ModelObject {
    let _ = (indices, width);
    let color = template_fill_color(part, cluts);

    // Fixed-tiny geometry, tucked just BELOW the part's local origin
    // (y-down: positive y). Unposed in the field the box sits under the
    // ground plane at the actor origin; posed by the world-map toggle it
    // is a small blob at the joint. Never size it to the part's bbox - a
    // head-sized unposed box parks itself over the walker's legs.
    let _ = part;
    let (lo, hi) = ([-4i16, 1, -4], [4i16, 9, 4]);
    let vertices: Vec<[i16; 3]> = (0..8)
        .map(|i| {
            [
                if i & 1 == 0 { lo[0] } else { hi[0] },
                if i & 2 == 0 { lo[1] } else { hi[1] },
                if i & 4 == 0 { lo[2] } else { hi[2] },
            ]
        })
        .collect();
    let quads: [[u16; 4]; 6] = [
        [0, 1, 2, 3],
        [4, 5, 6, 7],
        [0, 1, 4, 5],
        [2, 3, 6, 7],
        [0, 2, 4, 6],
        [1, 3, 5, 7],
    ];
    let prims = quads
        .iter()
        .map(|q| ModelPrim {
            vertices: q.to_vec(),
            uvs: Vec::new(),
            cba: 0,
            tsb: 0,
            colors: vec![color],
        })
        .collect();
    ModelObject {
        vertices,
        groups: vec![ModelGroup {
            shape: PacketShape::F4,
            semi_transparent: false,
            prims,
        }],
        scale: legaia_tmd::encode::LEGAIA_OBJECT_SCALE,
    }
}

/// Pack the head part's texture islands into the character's atlas
/// window; rewrites the head prims' UV/CBA/TSB in place. Returns the
/// window texels + palettes.
#[allow(clippy::type_complexity)]
pub(super) fn layout_head_window(
    head: &mut ModelObject,
    cluts: &[[u16; 16]],
    indices: &[u8],
    width: usize,
    slot: usize,
    warnings: &mut Vec<String>,
) -> Result<(Vec<u8>, Vec<Vec<u16>>)> {
    let u_base = slot * ATLAS_WINDOW_W;
    let col_base = (slot * ATLAS_COLS_PER_CHAR) as u16;

    // Collect the head's textured faces + their palettes.
    let mut faces: Vec<(usize, usize, (u8, u8, u8, u8))> = Vec::new();
    let mut used: Vec<u8> = Vec::new();
    for (gi, g) in head.groups.iter().enumerate() {
        if !g.shape.is_textured() {
            continue;
        }
        for (pi, p) in g.prims.iter().enumerate() {
            faces.push((gi, pi, face_bbox(&p.uvs)));
            let pal = (p.cba & 0x3F) as u8;
            if !used.contains(&pal) {
                used.push(pal);
            }
        }
    }
    let mut window = vec![0u8; ATLAS_WINDOW_W * ATLAS_WINDOW_H];
    if faces.is_empty() {
        return Ok((window, Vec::new()));
    }
    used.sort_unstable();
    let (group_of, group_colors) = merge_palettes(&used, cluts, ATLAS_COLS_PER_CHAR, warnings)?;

    // Cluster faces by inflated-bbox overlap so shared texels pack once.
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
            if boxes_touch(faces[i].2, faces[j].2) {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }
    let mut clusters: BTreeMap<usize, (Vec<usize>, (u8, u8, u8, u8))> = BTreeMap::new();
    #[allow(clippy::needless_range_loop)]
    for i in 0..faces.len() {
        let root = find(&mut parent, i);
        let e = clusters.entry(root).or_insert((Vec::new(), faces[i].2));
        e.1 = union_bbox(e.1, faces[i].2);
        e.0.push(i);
    }
    let clusters: Vec<(Vec<usize>, (u8, u8, u8, u8))> = clusters.into_values().collect();

    // Shelf-pack the cluster bboxes into the window; halve on overflow.
    let mut placed: Vec<(usize, usize)> = Vec::with_capacity(clusters.len());
    let mut scale = 1usize;
    'retry: loop {
        placed.clear();
        let (mut x, mut y, mut shelf) = (0usize, 0usize, 0usize);
        for &(_, bb) in &clusters {
            let w = ((bb.2 - bb.0) as usize + 1).div_ceil(scale);
            let h = ((bb.3 - bb.1) as usize + 1).div_ceil(scale);
            if w > ATLAS_WINDOW_W || {
                if x + w > ATLAS_WINDOW_W {
                    y += shelf;
                    x = 0;
                    shelf = 0;
                }
                y + h > ATLAS_WINDOW_H
            } {
                if scale >= 8 {
                    bail!("head texture does not fit the field atlas window");
                }
                scale *= 2;
                warnings.push(format!(
                    "field head texture at 1/{scale} resolution (atlas window)"
                ));
                continue 'retry;
            }
            placed.push((x, y));
            x += w;
            shelf = shelf.max(h);
        }
        break;
    }

    for (ci, (members, bb)) in clusters.iter().enumerate() {
        let (dx, dy) = placed[ci];
        for &fi in members {
            let (gi, pi, _) = faces[fi];
            let p = &mut head.groups[gi].prims[pi];
            let src_pal = (p.cba & 0x3F) as usize;
            let group = group_of[&(src_pal as u8)];
            let colors = &group_colors[group];
            let fb = faces[fi].2;
            for sy in (fb.1 as usize..=fb.3 as usize).step_by(scale) {
                for sx in (fb.0 as usize..=fb.2 as usize).step_by(scale) {
                    let tx = dx + (sx - bb.0 as usize) / scale;
                    let ty = dy + (sy - bb.1 as usize) / scale;
                    if tx >= ATLAS_WINDOW_W || ty >= ATLAS_WINDOW_H {
                        continue;
                    }
                    let idx = if sx < width && sy < PAGE_HEIGHT {
                        indices[sy * width + sx] as usize
                    } else {
                        0
                    };
                    let color = cluts[src_pal][idx];
                    let new_idx = if idx == 0 || color == 0 {
                        0
                    } else {
                        match colors.iter().position(|&c| c == color) {
                            Some(pos) => pos + 1,
                            None => nearest_color(colors, color) + 1,
                        }
                    };
                    window[ty * ATLAS_WINDOW_W + tx] = new_idx as u8;
                }
            }
            for uv in p.uvs.iter_mut() {
                let nx = u_base + dx + (uv.0 as usize - bb.0 as usize) / scale;
                let ny = dy + (uv.1 as usize - bb.1 as usize) / scale;
                *uv = (nx.min(255) as u8, ny.min(255) as u8);
            }
            p.cba = (FIELD_CLUT_ROW << 6) | (col_base + group as u16);
            p.tsb = FIELD_TSB;
        }
    }
    Ok((window, group_colors))
}

/// Repaint each character's atlas TIM (entries 1..=3 of §2) with the new
/// head window + palettes, in place inside the decoded §2 pack bytes.
pub(super) fn patch_atlas(sec2: &mut [u8], slots: &[FieldSlot]) -> Result<()> {
    let entries = legaia_asset::pack::parse_pack(sec2)?;
    for (slot, fs) in slots.iter().enumerate() {
        let e = entries
            .get(slot + 1)
            .ok_or_else(|| anyhow::anyhow!("atlas entry {} missing", slot + 1))?;
        let (start, len) = (e.byte_offset, e.size);
        let tim = sec2
            .get_mut(start..start + len)
            .ok_or_else(|| anyhow::anyhow!("atlas entry {} out of range", slot + 1))?;
        patch_tim(tim, fs).with_context(|| format!("atlas entry {}", slot + 1))?;
    }
    Ok(())
}

/// Patch one 4bpp CLUT'd TIM in place: CLUT entries (4 x 16 colours) and
/// pixel data (20 halfwords x 128 rows) replaced, geometry untouched.
/// TIM layout: `[u32 0x10][u32 flags][clut block][image block]`, each
/// block `[u32 len][u16 x][u16 y][u16 w][u16 h][data]`.
fn patch_tim(tim: &mut [u8], fs: &FieldSlot) -> Result<()> {
    let magic = u32::from_le_bytes(tim.get(0..4).unwrap_or(&[0; 4]).try_into().unwrap());
    let flags = u32::from_le_bytes(tim.get(4..8).unwrap_or(&[0; 4]).try_into().unwrap());
    if magic != 0x10 || flags & 0x8 == 0 {
        bail!("not a CLUT'd TIM (magic {magic:#x}, flags {flags:#x})");
    }
    let clut_len = u32::from_le_bytes(tim[8..12].try_into().unwrap()) as usize;
    let clut_w = u16::from_le_bytes(tim[16..18].try_into().unwrap()) as usize;
    let clut_h = u16::from_le_bytes(tim[18..20].try_into().unwrap()) as usize;
    let clut_data = 20;
    let clut_n = clut_w * clut_h;
    if clut_n != ATLAS_COLS_PER_CHAR * 16 {
        bail!("atlas CLUT holds {clut_n} colours, expected 64");
    }
    let dst = tim
        .get_mut(clut_data..clut_data + clut_n * 2)
        .ok_or_else(|| anyhow::anyhow!("CLUT data out of range"))?;
    dst.fill(0);
    for (g, colors) in fs.palettes.iter().enumerate().take(ATLAS_COLS_PER_CHAR) {
        for (i, &c) in colors.iter().enumerate().take(15) {
            let at = (g * 16 + 1 + i) * 2;
            dst[at..at + 2].copy_from_slice(&c.to_le_bytes());
        }
    }
    // Image block follows the CLUT block (block len covers len..data end).
    let img = 8 + clut_len;
    let img_w = u16::from_le_bytes(
        tim.get(img + 8..img + 10)
            .ok_or_else(|| anyhow::anyhow!("image block out of range"))?
            .try_into()
            .unwrap(),
    ) as usize;
    let img_h = u16::from_le_bytes(tim[img + 10..img + 12].try_into().unwrap()) as usize;
    if img_w * 2 * 2 != ATLAS_WINDOW_W || img_h != ATLAS_WINDOW_H {
        bail!("atlas image is {img_w}hw x {img_h}, expected 20x128");
    }
    let data = img + 12;
    let dst = tim
        .get_mut(data..data + img_w * 2 * img_h)
        .ok_or_else(|| anyhow::anyhow!("image data out of range"))?;
    for yy in 0..ATLAS_WINDOW_H {
        for xb in 0..ATLAS_WINDOW_W / 2 {
            let lo = fs.window[yy * ATLAS_WINDOW_W + xb * 2] & 0xF;
            let hi = fs.window[yy * ATLAS_WINDOW_W + xb * 2 + 1] & 0xF;
            dst[yy * (ATLAS_WINDOW_W / 2) + xb] = lo | (hi << 4);
        }
    }
    Ok(())
}
