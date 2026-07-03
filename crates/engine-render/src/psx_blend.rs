//! PSX GPU semi-transparency (per-prim blend modes) for the VRAM-mesh path.
//!
//! A PSX primitive is semi-transparent when its packet's ABE bit is set; for
//! textured prims the texel's own BGR555 STP bit (bit 15) then decides *per
//! pixel*: STP=1 texels blend, STP=0 texels draw opaque even inside a
//! semi-transparent prim (texel `0x0000` is never drawn at all, and `0x8000`
//! - black with STP - blends). The blend equation comes from texpage (TSB)
//!   bits 5..=6 ("ABR"):
//!
//! | ABR | equation            | wgpu mapping                                  |
//! |-----|---------------------|-----------------------------------------------|
//! | 0   | `0.5*B + 0.5*F`     | src=Constant, dst=Constant, Add (constant 0.5)|
//! | 1   | `B + F`             | src=One, dst=One, Add                         |
//! | 2   | `B - F`             | src=One, dst=One, ReverseSubtract             |
//! | 3   | `B + 0.25*F`        | src=One, dst=One, Add; F pre-scaled 0.25      |
//!
//! (`B` = destination/background, `F` = source/foreground.) Mode 3's `0.25*F`
//! has no fixed-function factor, so the blend-pass fragment shader pre-scales
//! the output by [`src_shader_scale`] and the pipeline stays plain-additive.
//!
//! The mesh builders (`legaia_tmd::mesh`) pack the per-prim ABE bit into bit
//! 15 of the per-vertex TSB attribute ([`TSB_SEMI_TRANSPARENT_BIT`]), which
//! is unused by the TMD TSB encoding. With one fixed blend state per
//! pipeline, per-texel STP inside one prim is handled with **two passes**:
//! the opaque pass draws every triangle and discards STP texels of
//! semi-transparent prims in the shader, then a blend pass re-draws only the
//! semi-transparent triangles (grouped per ABR mode by
//! [`append_semi_tail`]) and discards everything *except* STP texels. Both
//! the shader discard and the blend pass are gated on the PSX-faithful mode
//! flag ([`Renderer::set_psx_mode`]), so the default path is unchanged.

/// Bit 15 of the per-vertex TSB attribute = "prim is semi-transparent"
/// (the TMD mode byte's ABE bit). Engine-side packing; kept in lockstep
/// with `legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT`.
pub const TSB_SEMI_TRANSPARENT_BIT: u16 = 0x8000;

/// Blend constant bound while drawing ABR mode 0 (`0.5*B + 0.5*F`):
/// both factors are `BlendFactor::Constant`.
pub const MODE0_BLEND_CONSTANT: f64 = 0.5;

/// True when the prim that produced this TSB attribute had its ABE
/// (semi-transparency) bit set.
pub fn prim_semi_transparent(tsb: u16) -> bool {
    tsb & TSB_SEMI_TRANSPARENT_BIT != 0
}

/// ABR blend mode from TSB bits 5..=6 (0..=3).
pub fn abr_mode(tsb: u16) -> u8 {
    ((tsb >> 5) & 0x3) as u8
}

/// Pack a prim's semi-transparency state into a blend word using the
/// same bit positions the textured path rides on the TSB attribute:
/// ABE → bit 15 ([`TSB_SEMI_TRANSPARENT_BIT`]), ABR → bits 5..=6.
/// The inverse of [`prim_semi_transparent`] + [`abr_mode`]. This is
/// the per-vertex word [`crate::Renderer::upload_color_mesh_blended`]
/// consumes for untextured prims (which carry no real TSB - their ABE
/// comes from the TMD group mode byte, and the blend mode from
/// whatever texpage/draw-env state the caller resolves; mode 0 is the
/// PSX draw-env default).
pub fn pack_blend_word(abe: bool, abr: u8) -> u16 {
    (if abe { TSB_SEMI_TRANSPARENT_BIT } else { 0 }) | (((abr & 0x3) as u16) << 5)
}

/// One semi-transparent prim's blend-pass metadata, recorded once at
/// mesh upload time by [`append_semi_tail`] /
/// [`append_semi_tail_words`]. The PSX-faithful blend pass re-keys
/// these per frame ([`prim_depth_key`]) to reproduce the retail
/// ordering table at per-primitive granularity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SemiPrim {
    /// Model-space centroid (average of the triangle's 3 corners).
    /// Because the MVP is linear, this point's clip-space `w` equals
    /// the average of the corners' clip-space `w` - the avg-Z the GTE
    /// `RTPT` + OT-insertion path bins on.
    pub centroid: [f32; 3],
    /// ABR blend mode 0..=3 (selects the blend pipeline).
    pub mode: u8,
    /// First index of this triangle inside the mesh's semi tail
    /// (absolute position in the extended index buffer; the triangle
    /// spans `first_index..first_index + 3`).
    pub first_index: u32,
}

/// Per-prim ordering-table depth key: the clip-space `w` of the prim's
/// model-space centroid under `mvp` (for the standard projection,
/// clip `w` = view depth). By linearity of `mvp` this equals the
/// average of the prim's vertices' clip-space `w`, matching the PSX
/// OT semantics of binning a prim by its vertices' average Z. The
/// model origin's key is `mvp.w_axis.w` - the depth convention the
/// rest of the renderer (and the previous per-draw ordering) uses.
pub fn prim_depth_key(mvp: &glam::Mat4, centroid: [f32; 3]) -> f32 {
    mvp.x_axis.w * centroid[0]
        + mvp.y_axis.w * centroid[1]
        + mvp.z_axis.w * centroid[2]
        + mvp.w_axis.w
}

/// One entry of the per-frame blend-pass ordering list: a single semi
/// prim of a single draw, keyed for the global back-to-front sort.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlendListEntry {
    /// Depth key ([`prim_depth_key`]); larger = farther = drawn earlier.
    pub key: f32,
    /// Submission sequence number (list push order). Ties on `key`
    /// draw in *descending* `seq`: a retail OT bucket is LIFO
    /// (`AddPrim` prepends to the bucket list, `DrawOTag` walks it
    /// head-first), so prims inserted later draw first within a bucket.
    pub seq: u32,
    /// `true` when the prim belongs to an untextured colour-mesh draw
    /// (selects the colour-mesh blend pipelines / draw list).
    pub untextured: bool,
    /// Index of the owning draw inside its scene draw list.
    pub draw_index: u32,
    /// ABR blend mode 0..=3 (selects the blend pipeline).
    pub mode: u8,
    /// First index of the prim's triangle in its mesh's semi tail.
    pub first_index: u32,
}

/// Append one draw's semi prims to the per-frame blend ordering list,
/// keying each prim by [`prim_depth_key`] under the draw's `mvp`.
/// `prims` must be in original mesh order (as recorded by
/// [`append_semi_tail`]) so `seq` reflects submission order.
pub fn push_draw_prims(
    list: &mut Vec<BlendListEntry>,
    untextured: bool,
    draw_index: u32,
    mvp: &glam::Mat4,
    prims: &[SemiPrim],
) {
    for p in prims {
        list.push(BlendListEntry {
            key: prim_depth_key(mvp, p.centroid),
            seq: list.len() as u32,
            untextured,
            draw_index,
            mode: p.mode,
            first_index: p.first_index,
        });
    }
}

/// Far-to-near (descending key) sort of the per-prim blend list, the
/// per-primitive mirror of the retail ordering table's back-to-front
/// blend. Equal keys (one OT bucket) draw in descending `seq` - the
/// retail LIFO bucket order (later-submitted prims draw first). Uses
/// IEEE `total_cmp` so degenerate keys still give a deterministic
/// order (positive NaN sorts farthest).
pub fn sort_blend_list(list: &mut [BlendListEntry]) {
    list.sort_unstable_by(|a, b| b.key.total_cmp(&a.key).then(b.seq.cmp(&a.seq)));
}

/// Walk a sorted blend list and emit one GPU draw per *run*:
/// consecutive entries from the same draw + ABR mode whose tail
/// triangles are contiguous in the index buffer merge into a single
/// indexed range. `emit(head, first_index, index_count)` receives the
/// run's first entry (carrying the draw identity + mode) and the
/// merged index range.
pub fn coalesce_sorted(list: &[BlendListEntry], mut emit: impl FnMut(&BlendListEntry, u32, u32)) {
    let mut i = 0;
    while i < list.len() {
        let head = list[i];
        let start = head.first_index;
        let mut count = 3u32;
        let mut j = i + 1;
        while j < list.len() {
            let e = &list[j];
            if e.untextured == head.untextured
                && e.draw_index == head.draw_index
                && e.mode == head.mode
                && e.first_index == start + count
            {
                count += 3;
                j += 1;
            } else {
                break;
            }
        }
        emit(&head, start, count);
        i = j;
    }
}

/// Foreground pre-scale applied in the blend-pass fragment shader for
/// the given ABR mode. Only mode 3 (`B + 0.25*F`) scales; the other
/// modes get their factors from the fixed-function blend state.
pub fn src_shader_scale(mode: u8) -> f32 {
    if mode == 3 { 0.25 } else { 1.0 }
}

/// wgpu blend state for one ABR mode (see the module table). The alpha
/// component always replaces (the surface alpha is unused).
pub fn blend_state(mode: u8) -> wgpu::BlendState {
    use wgpu::{BlendComponent, BlendFactor, BlendOperation};
    let color = match mode {
        0 => BlendComponent {
            src_factor: BlendFactor::Constant,
            dst_factor: BlendFactor::Constant,
            operation: BlendOperation::Add,
        },
        2 => BlendComponent {
            // ReverseSubtract = dst - src = B - F.
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::ReverseSubtract,
        },
        // Modes 1 and 3 are both plain additive; mode 3's 0.25 factor
        // is pre-applied in the shader (`src_shader_scale`).
        _ => BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Add,
        },
    };
    wgpu::BlendState {
        color,
        alpha: wgpu::BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::Zero,
            operation: BlendOperation::Add,
        },
    }
}

/// Reference PSX blend arithmetic for one colour channel (`b` =
/// background, `f` = foreground, both `[0, 1]`). The GPU clamps the
/// 5-bit result; so does every wgpu blend op on a normalized target.
/// Unit tests evaluate [`blend_state`] + [`src_shader_scale`] against
/// this to keep the pipeline mapping honest.
pub fn blend_apply(mode: u8, b: f32, f: f32) -> f32 {
    let v = match mode {
        0 => 0.5 * b + 0.5 * f,
        1 => b + f,
        2 => b - f,
        _ => b + 0.25 * f,
    };
    v.clamp(0.0, 1.0)
}

/// Append a per-ABR-mode "semi tail" to a triangle index list: the
/// original indices stay untouched at the front (the opaque pass draws
/// `0..indices.len()` exactly as before), and every semi-transparent
/// triangle is *duplicated* into one of four contiguous tail buckets,
/// one per ABR mode. Returns the extended index list,
/// `[(first_index, index_count); 4]` tail ranges, and one [`SemiPrim`]
/// per semi triangle (in original mesh order) carrying its model-space
/// centroid + tail location for the per-frame ordering list.
///
/// A triangle's prim flags are read from its first vertex - the mesh
/// builders emit fresh per-corner vertices per prim, so all corners
/// share one `(cba, tsb)`.
pub fn append_semi_tail(
    indices: &[u32],
    cba_tsb: &[[u16; 2]],
    positions: &[[f32; 3]],
) -> (Vec<u32>, [(u32, u32); 4], Vec<SemiPrim>) {
    append_semi_tail_by(indices, positions, |v| cba_tsb[v][1])
}

/// [`append_semi_tail`] for the untextured colour-mesh path, whose
/// vertices carry a bare per-vertex blend word ([`pack_blend_word`])
/// instead of a `(cba, tsb)` pair. Same bucketing semantics.
pub fn append_semi_tail_words(
    indices: &[u32],
    blend: &[u16],
    positions: &[[f32; 3]],
) -> (Vec<u32>, [(u32, u32); 4], Vec<SemiPrim>) {
    append_semi_tail_by(indices, positions, |v| blend[v])
}

/// Shared bucketing core: `word_of` maps a vertex index to its packed
/// blend word (ABE bit 15, ABR bits 5..=6).
fn append_semi_tail_by(
    indices: &[u32],
    positions: &[[f32; 3]],
    word_of: impl Fn(usize) -> u16,
) -> (Vec<u32>, [(u32, u32); 4], Vec<SemiPrim>) {
    let mut buckets: [Vec<u32>; 4] = Default::default();
    // (mode, triangle slot within its bucket, centroid), kept in
    // original mesh order so the per-frame blend list sees retail's
    // prim submission order.
    let mut prims: Vec<(u8, u32, [f32; 3])> = Vec::new();
    for tri in indices.chunks_exact(3) {
        let word = word_of(tri[0] as usize);
        if prim_semi_transparent(word) {
            let mode = abr_mode(word);
            let slot = (buckets[mode as usize].len() / 3) as u32;
            buckets[mode as usize].extend_from_slice(tri);
            let mut c = [0.0f32; 3];
            for &v in tri {
                let p = positions[v as usize];
                c[0] += p[0];
                c[1] += p[1];
                c[2] += p[2];
            }
            prims.push((mode, slot, [c[0] / 3.0, c[1] / 3.0, c[2] / 3.0]));
        }
    }
    let mut out = indices.to_vec();
    let mut ranges = [(0u32, 0u32); 4];
    for (mode, bucket) in buckets.iter().enumerate() {
        ranges[mode] = (out.len() as u32, bucket.len() as u32);
        out.extend_from_slice(bucket);
    }
    let semi_prims = prims
        .into_iter()
        .map(|(mode, slot, centroid)| SemiPrim {
            centroid,
            mode,
            first_index: ranges[mode as usize].0 + slot * 3,
        })
        .collect();
    (out, ranges, semi_prims)
}
