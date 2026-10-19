//! Cutting the skin **out of** a kept primitive - the texel-level half of the
//! item-alone cut ([`equip_isolate`](super::equip_isolate)).
//!
//! The isolation decides per primitive, and a primitive is one textured
//! quad: when the quad's texture paints the item *and* the flesh beside it,
//! no per-primitive answer is right. That is how the cut's skin remnants
//! arise - Noa's Guardian Clip is eight quads whose texture carries the
//! forehead around the band, the top of her Fighting Robe paints the
//! shoulders and the collarbone above the neckline, Vahn's Seals paint the
//! brow under the circlet, a claw's quads carry the knuckles. Each of those
//! samples a 20 - 45 % skin share, under the drop threshold, so the whole
//! quad stays and the skin comes with it.
//!
//! The fix is to make the skin **transparent** rather than to drop the quad.
//! For every kept primitive with a skin share of at least [`MIXED_SKIN`],
//! the texels it samples are copied - through its CLUT - into an unused
//! 15-bit direct-colour page of the loadout's VRAM, with the skin texels
//! written as `0x0000` (the PSX transparent word every host's sampler
//! discards and the glTF baker exports as alpha 0), and the primitive's TSB
//! is pointed at the copy. UVs stay as they are; the item keeps its shape
//! and loses its flesh at texel resolution. A primitive left fully
//! transparent is dropped.
//!
//! "Skin texel" ([`skin_texel`]) is a flesh-tone band narrower than the
//! isolation's, or a colour within one 5-bit step of the character's own
//! face, filtered to **connected patches** of at least [`MIN_PATCH`] texels so
//! the odd warm highlight inside an item's texture is not punched out; a
//! patch then grows into the warm texels touching it (the skin's shadowed
//! edge) and swallows single opaque specks it surrounds. A primitive only
//! counts as mixed when at least [`ITEM_SHARE`] of it is clearly not flesh,
//! which keeps horns, claw tips and bone blades - one warm material shaded
//! light to dark - whole. The masked page
//! lives only in the loadout's VRAM: the full-character model never samples
//! it.

use std::collections::{BTreeMap, HashMap, HashSet};

use legaia_tim::Vram;
use legaia_tmd::mesh::VramMesh;

use super::equip_isolate::warm_like;

/// Skin share (of the primitive's sampled texels) from which a kept
/// primitive is re-pointed at a masked copy. Below it the skin is a stray
/// texel or two and the primitive is left alone.
pub const MIXED_SKIN: f32 = 0.04;
/// Share of a primitive's texels that must be clearly **not** flesh (neither
/// skin nor any warm tone) for it to count as mixed. A horn, a claw tip or a
/// bone blade is one warm material shaded light to dark - its light shades
/// match skin, its dark ones are warm too - and it is left whole; a quad that
/// paints a grey circlet over a forehead is mixed.
pub const ITEM_SHARE: f32 = 0.25;
/// Texels copied around the primitives' own footprint: a GPU sampler
/// rounding at a UV edge reads up to this far past it.
const COPY_MARGIN: i32 = 2;
/// Rounds of growing a masked patch into adjacent warm texels (the skin's
/// shadowed edge, darker than any face colour).
const GROW_ROUNDS: usize = 3;
/// Smallest 4-connected patch of skin texels (in one source page) that is
/// masked; smaller specks stay.
pub const MIN_PATCH: usize = 6;

/// TSB bits a re-pointed primitive keeps: the semi-transparency mode
/// (`5..=6`) and the mesh builder's semi-enable bit (`15`).
const TSB_KEEP: u16 = 0x8060;

/// Whether one texel word is the character's flesh: in the [`flesh_tone`]
/// band, or within one 5-bit step of a colour the character's own face
/// samples (`face`, see [`face_colours`]).
pub fn skin_texel(word: u16, face: &HashSet<u16>) -> bool {
    let w = word & 0x7FFF;
    w != 0 && (flesh_tone(w) || face.contains(&w))
}

/// The party's flesh as one hue / saturation band at any brightness: hue
/// `12..=36` degrees, saturation `0.30..=0.62`, red channel at least 12.
/// Narrower than the isolation's
/// [`skin_like`](super::equip_isolate::skin_like) on both sides, because here a
/// false match punches a hole in the item rather than dropping a quad: bone,
/// ivory and horn sit below the saturation floor or above the hue ceiling
/// (Vahn's Nail Glove spikes `(28, 24, 22)` at 0.21, the Chaos Breaker
/// blade `(23, 20, 13)` at 42 degrees), gold and wood above the saturation
/// ceiling; every character's skin ramp, Gala's dark one included, falls
/// inside.
pub fn flesh_tone(word: u16) -> bool {
    let (r, g, b) = (
        f32::from(word & 0x1F),
        f32::from((word >> 5) & 0x1F),
        f32::from((word >> 10) & 0x1F),
    );
    if !(r > g && g >= b) || r < 12.0 {
        return false;
    }
    let sat = (r - b) / r;
    let hue = 60.0 * (g - b) / (r - b);
    (0.30..=0.62).contains(&sat) && (12.0..=36.0).contains(&hue)
}

/// The face colours [`skin_texel`] matches: the character's own skin
/// ([`skin_colours`](super::equip_isolate::skin_colours)) dilated by one
/// 5-bit step per channel.
pub fn face_colours(own: &HashSet<u16>) -> HashSet<u16> {
    super::equip_isolate::dilate(own, 1)
}

/// Texel coordinates (in the primitive's page) a UV triangle covers: the
/// texel centres inside it, or its corner texels for a degenerate footprint.
pub fn triangle_texels(a: [u8; 2], b: [u8; 2], c: [u8; 2]) -> Vec<(usize, usize)> {
    let centre = |t: [u8; 2]| (f32::from(t[0]) + 0.5, f32::from(t[1]) + 0.5);
    let ((ax, ay), (bx, by), (cx, cy)) = (centre(a), centre(b), centre(c));
    let det = (bx - ax) * (cy - ay) - (cx - ax) * (by - ay);
    let minu = a[0].min(b[0]).min(c[0]) as usize;
    let maxu = a[0].max(b[0]).max(c[0]) as usize;
    let minv = a[1].min(b[1]).min(c[1]) as usize;
    let maxv = a[1].max(b[1]).max(c[1]) as usize;
    let mut out = Vec::new();
    if det.abs() >= 1e-6 {
        for v in minv..=maxv {
            for u in minu..=maxu {
                let (fx, fy) = (u as f32 + 0.5, v as f32 + 0.5);
                let w0 = ((bx - fx) * (cy - fy) - (cx - fx) * (by - fy)) / det;
                let w1 = ((cx - fx) * (ay - fy) - (ax - fx) * (cy - fy)) / det;
                let w2 = 1.0 - w0 - w1;
                if w0 >= -0.02 && w1 >= -0.02 && w2 >= -0.02 {
                    out.push((u, v));
                }
            }
        }
    }
    if out.is_empty() {
        for t in [a, b, c] {
            out.push((usize::from(t[0]), usize::from(t[1])));
        }
    }
    out
}

/// What the mask did to one item-alone mesh.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkinMaskReport {
    /// Primitives re-pointed at a masked copy.
    pub masked_primitives: usize,
    /// Skin texels made transparent (counted once per source page texel).
    pub masked_texels: usize,
    /// Primitives dropped because nothing opaque was left on them.
    pub dropped_primitives: usize,
    /// Primitives that needed a copy but found no free VRAM page (left
    /// unmasked).
    pub unplaced_primitives: usize,
}

/// One source page as a primitive samples it: `(cba, tsb)` with the
/// TSB's blend bits cleared - the key texels are read through.
type SourceKey = (u16, u16);

/// Mask the skin out of `mesh` in place. `prim_of[v]` is the
/// `(object, ordinal)` primitive vertex `v` came from (triangles are grouped
/// by their first vertex's); `vram` is the loadout VRAM the mesh samples and
/// receives the masked direct-colour copies; `face` is [`face_colours`]. `highlight` writes the skin as
/// magenta instead of transparent (for the visual pass).
pub fn mask_skin(
    mesh: &mut VramMesh,
    object_ids: &mut Vec<u32>,
    prim_of: &[(u32, u32)],
    vram: &mut Vram,
    face: &HashSet<u16>,
    highlight: bool,
) -> SkinMaskReport {
    let mut report = SkinMaskReport::default();
    // Triangles per primitive.
    let mut tris_of: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new();
    for (t, tri) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
        let Some(&p) = prim_of.get(tri[0] as usize) else {
            continue;
        };
        tris_of.entry(p).or_default().push(t);
    }

    // Per primitive: source key, texel footprint, skin share.
    struct Prim {
        key: SourceKey,
        texels: Vec<(usize, usize)>,
        tris: Vec<usize>,
    }
    let mut mixed: Vec<Prim> = Vec::new();
    for tris in tris_of.values() {
        let v0 = mesh.indices[tris[0] * 3] as usize;
        let [cba, tsb] = mesh.cba_tsb[v0];
        // The mesh builder emits textured primitives only; a stream without
        // UVs has nothing to mask.
        if mesh.uvs.len() != mesh.positions.len() {
            continue;
        }
        let key = (cba, tsb & !TSB_KEEP);
        let mut seen: HashSet<(usize, usize)> = HashSet::new();
        let mut texels = Vec::new();
        for &t in tris {
            let i = &mesh.indices[t * 3..t * 3 + 3];
            let uv = |k: usize| mesh.uvs[i[k] as usize];
            for c in triangle_texels(uv(0), uv(1), uv(2)) {
                if seen.insert(c) {
                    texels.push(c);
                }
            }
        }
        let words: Vec<u16> = texels
            .iter()
            .map(|&(u, v)| crate::mesh_raster::texel_word(vram, key.0, key.1, u, v) & 0x7FFF)
            .filter(|&w| w != 0)
            .collect();
        if words.is_empty() {
            continue;
        }
        let n = words.len() as f32;
        let skin = words.iter().filter(|&&w| skin_texel(w, face)).count();
        let item = words
            .iter()
            .filter(|&&w| !skin_texel(w, face) && !warm_like(w))
            .count();
        if (skin as f32) / n < MIXED_SKIN || (item as f32) / n < ITEM_SHARE {
            continue;
        }
        mixed.push(Prim {
            key,
            texels,
            tris: tris.clone(),
        });
    }
    if mixed.is_empty() {
        return report;
    }

    // Per source page: the union footprint (dilated by `COPY_MARGIN` texels so a
    // sampler rounding past a UV edge still reads the copy) and the skin
    // patches in it.
    let mut by_key: BTreeMap<SourceKey, Vec<usize>> = BTreeMap::new();
    for (i, p) in mixed.iter().enumerate() {
        by_key.entry(p.key).or_default().push(i);
    }
    // Source key -> (page x, page y) of its masked copy.
    let mut placed: HashMap<SourceKey, (usize, usize)> = HashMap::new();
    // Source key -> masked texel set (page-local).
    let mut masked_of: HashMap<SourceKey, HashSet<(usize, usize)>> = HashMap::new();
    for (&key, members) in &by_key {
        let core: HashSet<(usize, usize)> = members
            .iter()
            .flat_map(|&i| mixed[i].texels.iter().copied())
            .collect();
        let mut area: HashSet<(usize, usize)> = core.clone();
        for &(u, v) in &core {
            for du in -COPY_MARGIN..=COPY_MARGIN {
                for dv in -COPY_MARGIN..=COPY_MARGIN {
                    let (nu, nv) = (u as i32 + du, v as i32 + dv);
                    if (0..256).contains(&nu) && (0..256).contains(&nv) {
                        area.insert((nu as usize, nv as usize));
                    }
                }
            }
        }
        let word_at = |(u, v): (usize, usize)| -> u16 {
            crate::mesh_raster::texel_word(vram, key.0, key.1, u, v)
        };
        // Skin patches over the primitives' own footprint.
        let skin: HashSet<(usize, usize)> = core
            .iter()
            .copied()
            .filter(|&c| skin_texel(word_at(c), face))
            .collect();
        let mut masked: HashSet<(usize, usize)> = HashSet::new();
        let mut visited: HashSet<(usize, usize)> = HashSet::new();
        for &start in &skin {
            if !visited.insert(start) {
                continue;
            }
            let mut patch = vec![start];
            let mut stack = vec![start];
            while let Some((u, v)) = stack.pop() {
                for (du, dv) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let (nu, nv) = (u as i32 + du, v as i32 + dv);
                    if nu < 0 || nv < 0 {
                        continue;
                    }
                    let n = (nu as usize, nv as usize);
                    if skin.contains(&n) && visited.insert(n) {
                        patch.push(n);
                        stack.push(n);
                    }
                }
            }
            if patch.len() >= MIN_PATCH {
                masked.extend(patch);
            }
        }
        if masked.is_empty() {
            continue;
        }
        let neighbours = |(u, v): (usize, usize)| {
            [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)]
                .into_iter()
                .filter_map(move |(du, dv)| {
                    let (nu, nv) = (u as i32 + du, v as i32 + dv);
                    (nu >= 0 && nv >= 0).then_some((nu as usize, nv as usize))
                })
        };
        // The shadowed edge of the skin: warm texels touching the patch -
        // over the copy margin too, which a sampler reads along every UV edge
        // (unmasked, it draws as a thin flesh-coloured seam).
        for _ in 0..GROW_ROUNDS {
            let grow: Vec<(usize, usize)> = area
                .iter()
                .copied()
                .filter(|c| !masked.contains(c))
                .filter(|&c| {
                    let w = word_at(c) & 0x7FFF;
                    w != 0 && (warm_like(w) || skin_texel(w, face))
                })
                .filter(|&c| neighbours(c).any(|n| masked.contains(&n)))
                .collect();
            if grow.is_empty() {
                break;
            }
            masked.extend(grow);
        }
        // Specks: an unmasked texel with three or four masked neighbours.
        let specks: Vec<(usize, usize)> = core
            .iter()
            .copied()
            .filter(|c| !masked.contains(c))
            .filter(|&c| neighbours(c).filter(|n| masked.contains(n)).count() >= 3)
            .collect();
        masked.extend(specks);
        let Some(page) = free_direct_page(vram, &area) else {
            report.unplaced_primitives += members.len();
            continue;
        };
        let copy: Vec<((usize, usize), u16)> = area
            .iter()
            .map(|&c| {
                let word = if masked.contains(&c) {
                    if highlight { 0x7C1F } else { 0 }
                } else {
                    word_at(c)
                };
                (c, word)
            })
            .collect();
        for ((u, v), word) in copy {
            vram.write_block(
                (page.0 + u) as u16,
                (page.1 + v) as u16,
                1,
                1,
                &word.to_le_bytes(),
            );
        }
        report.masked_texels += masked.len();
        placed.insert(key, page);
        masked_of.insert(key, masked);
    }

    // Re-point the mixed primitives; drop the ones with nothing opaque left.
    let mut drop_tris: HashSet<usize> = HashSet::new();
    for p in &mixed {
        let Some(&(px, py)) = placed.get(&p.key) else {
            continue;
        };
        let masked = &masked_of[&p.key];
        let opaque = p.texels.iter().any(|&c| {
            !masked.contains(&c)
                && crate::mesh_raster::texel_word(vram, p.key.0, p.key.1, c.0, c.1) != 0
        });
        if !opaque && !highlight {
            drop_tris.extend(p.tris.iter().copied());
            report.dropped_primitives += 1;
            continue;
        }
        let page_bits = ((px / 64) as u16) | (((py / 256) as u16) << 4) | (2 << 7);
        for &t in &p.tris {
            for k in 0..3 {
                let v = mesh.indices[t * 3 + k] as usize;
                let [cba, tsb] = mesh.cba_tsb[v];
                mesh.cba_tsb[v] = [cba, (tsb & TSB_KEEP) | page_bits];
            }
        }
        report.masked_primitives += 1;
    }
    if !drop_tris.is_empty() {
        compact(mesh, object_ids, &drop_tris);
    }
    report
}

/// The first 15-bit page origin (`x` a multiple of 64 up to 768, `y` 0 or
/// 256) under which every texel of `area` lands on a VRAM word no upload
/// (and no earlier copy) has written.
fn free_direct_page(vram: &Vram, area: &HashSet<(usize, usize)>) -> Option<(usize, usize)> {
    for py in [0usize, 256] {
        for px in (0..=768usize).step_by(64) {
            if area.iter().all(|&(u, v)| !vram.is_written(px + u, py + v)) {
                return Some((px, py));
            }
        }
    }
    None
}

/// Remove triangles `drop` and the vertices only they used.
fn compact(mesh: &mut VramMesh, object_ids: &mut Vec<u32>, drop: &HashSet<usize>) {
    let tris: Vec<[u32; 3]> = mesh
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .enumerate()
        .filter(|(t, _)| !drop.contains(t))
        .map(|(_, tri)| *tri)
        .collect();
    let mut remap = vec![u32::MAX; mesh.positions.len()];
    let mut out = VramMesh {
        positions: Vec::new(),
        uvs: Vec::new(),
        cba_tsb: Vec::new(),
        indices: Vec::with_capacity(tris.len() * 3),
        normals: Vec::new(),
        colors: Vec::new(),
    };
    let mut ids = Vec::new();
    for tri in &tris {
        for &i in tri {
            let i = i as usize;
            if remap[i] == u32::MAX {
                remap[i] = out.positions.len() as u32;
                out.positions.push(mesh.positions[i]);
                if let Some(&uv) = mesh.uvs.get(i) {
                    out.uvs.push(uv);
                }
                if let Some(&ct) = mesh.cba_tsb.get(i) {
                    out.cba_tsb.push(ct);
                }
                if let Some(&n) = mesh.normals.get(i) {
                    out.normals.push(n);
                }
                if let Some(&c) = mesh.colors.get(i) {
                    out.colors.push(c);
                }
                ids.push(object_ids.get(i).copied().unwrap_or(0));
            }
            out.indices.push(remap[i]);
        }
    }
    *mesh = out;
    *object_ids = ids;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangle_texels_cover_the_interior_and_fall_back_to_corners() {
        let t = triangle_texels([0, 0], [4, 0], [0, 4]);
        assert!(t.contains(&(0, 0)) && t.contains(&(1, 1)) && !t.contains(&(4, 4)));
        let line = triangle_texels([2, 2], [2, 2], [2, 2]);
        assert_eq!(line, vec![(2, 2), (2, 2), (2, 2)]);
    }

    /// A 16x16 15-bit source page half peach, half grey: the peach half is
    /// cut to transparent in a copy, the grey half survives, the primitive is
    /// re-pointed at the copy and keeps its UVs.
    #[test]
    fn a_half_skin_quad_is_repointed_at_a_masked_copy() {
        let peach: u16 = 28 | (20 << 5) | (15 << 10);
        let grey: u16 = 12 | (12 << 5) | (12 << 10);
        let mut vram = Vram::new();
        // Source: 15-bit page at (512, 256) - tsb page x 8, y 1, depth 2.
        let mut row = Vec::new();
        for u in 0..16u16 {
            row.extend_from_slice(&(if u < 8 { peach } else { grey }).to_le_bytes());
        }
        for v in 0..16u16 {
            vram.write_block(512, 256 + v, 16, 1, &row);
        }
        let tsb: u16 = 8 | (1 << 4) | (2 << 7);
        let mut mesh = VramMesh {
            positions: vec![[0.0; 3]; 3],
            uvs: vec![[0, 0], [15, 0], [0, 15]],
            cba_tsb: vec![[0, tsb]; 3],
            indices: vec![0, 1, 2],
            normals: vec![[0.0; 3]; 3],
            colors: vec![[0x80; 3]; 3],
        };
        let mut ids = vec![0u32; 3];
        let prim_of = vec![(0u32, 0u32); 3];
        let r = mask_skin(
            &mut mesh,
            &mut ids,
            &prim_of,
            &mut vram,
            &HashSet::new(),
            false,
        );
        assert_eq!(r.masked_primitives, 1);
        assert!(r.masked_texels > 0);
        let [cba, new_tsb] = mesh.cba_tsb[0];
        assert_ne!(new_tsb, tsb, "re-pointed");
        assert_eq!((new_tsb >> 7) & 3, 2, "direct colour");
        assert_eq!(mesh.uvs, vec![[0, 0], [15, 0], [0, 15]], "UVs unchanged");
        assert_eq!(crate::mesh_raster::texel_word(&vram, cba, new_tsb, 1, 1), 0);
        assert_eq!(
            crate::mesh_raster::texel_word(&vram, cba, new_tsb, 10, 1),
            grey
        );
        // The source page is untouched.
        assert_eq!(crate::mesh_raster::texel_word(&vram, cba, tsb, 1, 1), peach);
    }
}
