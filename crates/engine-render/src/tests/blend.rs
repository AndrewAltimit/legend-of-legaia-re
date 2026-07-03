use super::*;

// ---- PSX dithering ----

#[test]
fn dither_matrix_is_balanced_4x4() {
    // The 16 offsets span the documented [-4, +3] range and (being a
    // balanced ordered-dither pattern) sum to a small bias near zero.
    let m = psx_dither::DITHER_MATRIX;
    assert_eq!(m.len(), 16);
    assert_eq!(*m.iter().min().unwrap(), -4);
    assert_eq!(*m.iter().max().unwrap(), 3);
    assert_eq!(m.iter().sum::<i32>(), -8);
}

#[test]
fn dither_component_quantizes_to_5bit_expanded() {
    // Every output is a 5-bit value re-expanded by bit-replication:
    // (c5 << 3) | (c5 >> 2). Check the endpoints and that all outputs
    // belong to that 32-value set regardless of pixel / input.
    let valid: std::collections::HashSet<u8> =
        (0..32).map(|c5| ((c5 << 3) | (c5 >> 2)) as u8).collect();
    for c8 in 0..=255i32 {
        for y in 0..4u32 {
            for x in 0..4u32 {
                let out = psx_dither::dither_component(c8, x, y);
                assert!(valid.contains(&out), "c8={c8} -> {out} not a 5-bit level");
            }
        }
    }
    // Black stays black, white stays white (no offset escapes the clamp).
    assert_eq!(psx_dither::dither_component(0, 1, 1), 0);
    assert_eq!(psx_dither::dither_component(255, 1, 1), 255);
}

#[test]
fn dither_varies_across_the_4x4_cell() {
    // A mid-grey that sits between two 5-bit levels must resolve to
    // different quantized values across the dither cell - that spatial
    // variation IS the dithering. Pick a value off the 5-bit grid.
    let c8 = 134; // straddles the 5-bit boundary at 136 (134-4=130, 134+3=137)
    let mut seen = std::collections::HashSet::new();
    for y in 0..4u32 {
        for x in 0..4u32 {
            seen.insert(psx_dither::dither_component(c8, x, y));
        }
    }
    assert!(seen.len() >= 2, "dither produced no spatial variation");
}

#[test]
fn dither_rgb_disabled_path_is_identity_in_shader_only() {
    // The CPU helper always dithers; the *shader* gates on enable. Here
    // we just confirm the CPU triple path stays in range and quantizes.
    let out = psx_dither::dither_rgb([0.5, 0.25, 1.0], 2, 3);
    for c in out {
        assert!((0.0..=1.0).contains(&c));
    }
}

/// Every shaded 3D shader (with the dither helper prepended) must parse
/// and pass naga validation - this is the GPU-free guard that the WGSL
/// edits are well-formed, since the render pipelines can't build in CI.
#[test]
fn psx_shaders_parse_and_validate() {
    use wgpu::naga;
    let sources = [
        ("mesh", compose_psx_shader(MESH_SHADER_SRC)),
        (
            "textured_mesh",
            compose_psx_shader(TEXTURED_MESH_SHADER_SRC),
        ),
        ("vram_mesh", compose_psx_shader(VRAM_MESH_SHADER_SRC)),
        ("color_mesh", compose_psx_shader(COLOR_MESH_SHADER_SRC)),
    ];
    for (name, src) in sources {
        let module = naga::front::wgsl::parse_str(&src)
            .unwrap_or_else(|e| panic!("{name} shader failed to parse: {e:?}"));
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name} shader failed to validate: {e:?}"));
    }
}

/// The VRAM-mesh and colour-mesh shaders must expose the blend-pass
/// entry points the semi-transparency pipelines compile against.
#[test]
fn vram_shader_has_blend_entry_points() {
    for entry in ["fs_main", "fs_blend", "fs_blend_quarter"] {
        assert!(
            VRAM_MESH_SHADER_SRC.contains(&format!("fn {entry}(")),
            "vram shader missing entry point {entry}"
        );
        assert!(
            COLOR_MESH_SHADER_SRC.contains(&format!("fn {entry}(")),
            "color mesh shader missing entry point {entry}"
        );
    }
}

#[test]
fn psx_blend_semi_bit_matches_tmd_packing() {
    // `legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT` packs the prim ABE
    // flag into TSB bit 15; the renderer-side mirror must agree (the
    // crates deliberately don't depend on each other).
    assert_eq!(psx_blend::TSB_SEMI_TRANSPARENT_BIT, 0x8000);
    assert!(psx_blend::prim_semi_transparent(0x8000));
    assert!(psx_blend::prim_semi_transparent(0x801A));
    assert!(!psx_blend::prim_semi_transparent(0x001A));
    assert!(!psx_blend::prim_semi_transparent(0x7FFF));
}

#[test]
fn psx_blend_abr_mode_extracts_tsb_bits_5_6() {
    for mode in 0u16..4 {
        // ABR sits in bits 5..=6, independent of page / depth bits.
        assert_eq!(psx_blend::abr_mode(mode << 5), mode as u8);
        assert_eq!(psx_blend::abr_mode(0x8F1F | (mode << 5)), mode as u8);
    }
}

#[test]
fn psx_blend_src_scale_only_quarters_mode_3() {
    assert_eq!(psx_blend::src_shader_scale(0), 1.0);
    assert_eq!(psx_blend::src_shader_scale(1), 1.0);
    assert_eq!(psx_blend::src_shader_scale(2), 1.0);
    assert_eq!(psx_blend::src_shader_scale(3), 0.25);
}

/// Evaluate one wgpu blend factor as used by [`psx_blend::blend_state`]
/// (none of the selected factors depend on the source/dest colour).
fn eval_factor(f: wgpu::BlendFactor) -> f32 {
    match f {
        wgpu::BlendFactor::One => 1.0,
        wgpu::BlendFactor::Zero => 0.0,
        wgpu::BlendFactor::Constant => psx_blend::MODE0_BLEND_CONSTANT as f32,
        other => panic!("unexpected blend factor {other:?}"),
    }
}

/// Fixed-function blend evaluator: `op(src*src_factor, dst*dst_factor)`
/// clamped to the normalized target range, exactly what the GPU ROP does.
fn eval_blend(comp: wgpu::BlendComponent, dst: f32, src: f32) -> f32 {
    let s = src * eval_factor(comp.src_factor);
    let d = dst * eval_factor(comp.dst_factor);
    let v = match comp.operation {
        wgpu::BlendOperation::Add => d + s,
        wgpu::BlendOperation::ReverseSubtract => d - s,
        other => panic!("unexpected blend op {other:?}"),
    };
    v.clamp(0.0, 1.0)
}

/// blend_state(mode) + the shader-side foreground pre-scale must
/// reproduce the PSX equations (0.5B+0.5F / B+F / B-F / B+0.25F)
/// for every ABR mode, including the clamped corners.
#[test]
fn psx_blend_states_reproduce_psx_equations() {
    let samples = [
        (0.0f32, 0.0f32),
        (0.25, 0.5),
        (0.5, 0.25),
        (1.0, 1.0), // clamps modes 1 and 0's unclamped sum
        (0.1, 0.9), // clamps mode 2 (B - F < 0)
        (0.75, 0.75),
    ];
    for mode in 0u8..4 {
        let state = psx_blend::blend_state(mode);
        // Alpha always replaces - the surface alpha is unused.
        assert_eq!(state.alpha.src_factor, wgpu::BlendFactor::One);
        assert_eq!(state.alpha.dst_factor, wgpu::BlendFactor::Zero);
        assert_eq!(state.alpha.operation, wgpu::BlendOperation::Add);
        for (b, f) in samples {
            // The blend-pass fragment shader outputs F * src_shader_scale.
            let shader_out = f * psx_blend::src_shader_scale(mode);
            let got = eval_blend(state.color, b, shader_out);
            let want = psx_blend::blend_apply(mode, b, f);
            assert!(
                (got - want).abs() < 1e-6,
                "mode {mode} B={b} F={f}: pipeline gives {got}, PSX wants {want}"
            );
        }
    }
}

/// Per-corner positions for `n` triangles: triangle `k` sits at
/// `z = zs[k]` with corners spread in x so its centroid is
/// `(1, 0, zs[k])`.
fn tri_positions(zs: &[f32]) -> Vec<[f32; 3]> {
    let mut out = Vec::new();
    for &z in zs {
        out.push([0.0, 0.0, z]);
        out.push([1.0, 0.0, z]);
        out.push([2.0, 0.0, z]);
    }
    out
}

/// MVP whose clip-space `w` row maps `w = z + d` - a minimal
/// perspective-like matrix that makes depth keys easy to predict.
fn z_to_w_mvp(d: f32) -> Mat4 {
    Mat4::from_cols(
        glam::Vec4::new(1.0, 0.0, 0.0, 0.0),
        glam::Vec4::new(0.0, 1.0, 0.0, 0.0),
        glam::Vec4::new(0.0, 0.0, 1.0, 1.0),
        glam::Vec4::new(0.0, 0.0, 0.0, d),
    )
}

#[test]
fn psx_blend_append_semi_tail_buckets_per_mode() {
    // 4 prims x 3 per-corner verts: opaque, semi ABR 0, semi ABR 2,
    // semi ABR 3 (in that order).
    let semi = psx_blend::TSB_SEMI_TRANSPARENT_BIT;
    let mut cba_tsb = Vec::new();
    for tsb in [0x001Au16, semi, semi | (2 << 5), semi | (3 << 5)] {
        cba_tsb.extend_from_slice(&[[0u16, tsb]; 3]);
    }
    let indices: Vec<u32> = (0..12).collect();
    let positions = tri_positions(&[1.0, 2.0, 3.0, 4.0]);
    let (out, ranges, prims) = psx_blend::append_semi_tail(&indices, &cba_tsb, &positions);
    // Original indices untouched at the front (the opaque pass range).
    assert_eq!(&out[..12], indices.as_slice());
    // Tail: 3 semi triangles bucketed per ABR mode, mode 1 empty.
    assert_eq!(ranges[0], (12, 3));
    assert_eq!(ranges[1], (15, 0));
    assert_eq!(ranges[2], (15, 3));
    assert_eq!(ranges[3], (18, 3));
    assert_eq!(&out[12..15], &[3, 4, 5]);
    assert_eq!(&out[15..18], &[6, 7, 8]);
    assert_eq!(&out[18..21], &[9, 10, 11]);
    assert_eq!(out.len(), 21);
    // Per-prim metadata: original mesh order (not bucket order), each
    // pointing at its slot in the mode tail, centroid = corner average.
    assert_eq!(
        prims,
        vec![
            psx_blend::SemiPrim {
                centroid: [1.0, 0.0, 2.0],
                mode: 0,
                first_index: 12,
            },
            psx_blend::SemiPrim {
                centroid: [1.0, 0.0, 3.0],
                mode: 2,
                first_index: 15,
            },
            psx_blend::SemiPrim {
                centroid: [1.0, 0.0, 4.0],
                mode: 3,
                first_index: 18,
            },
        ]
    );
}

#[test]
fn psx_blend_append_semi_tail_same_mode_prims_get_distinct_tail_slots() {
    // 3 semi prims all on ABR mode 0: their tail triangles must land at
    // consecutive first_index slots (12, 15, 18), in mesh order.
    let semi = psx_blend::TSB_SEMI_TRANSPARENT_BIT;
    let cba_tsb = vec![[0u16, semi]; 9];
    let indices: Vec<u32> = (0..9).collect();
    let positions = tri_positions(&[5.0, 6.0, 7.0]);
    let (out, ranges, prims) = psx_blend::append_semi_tail(&indices, &cba_tsb, &positions);
    assert_eq!(out.len(), 18);
    assert_eq!(ranges[0], (9, 9));
    let firsts: Vec<u32> = prims.iter().map(|p| p.first_index).collect();
    assert_eq!(firsts, vec![9, 12, 15]);
    let zs: Vec<f32> = prims.iter().map(|p| p.centroid[2]).collect();
    assert_eq!(zs, vec![5.0, 6.0, 7.0]);
    // Each prim's tail slot holds its own triangle's indices.
    assert_eq!(&out[9..12], &[0, 1, 2]);
    assert_eq!(&out[12..15], &[3, 4, 5]);
    assert_eq!(&out[15..18], &[6, 7, 8]);
}

#[test]
fn psx_blend_append_semi_tail_all_opaque_is_identity() {
    let cba_tsb = vec![[0u16, 0x001Au16]; 6];
    let indices: Vec<u32> = vec![0, 1, 2, 3, 4, 5];
    let positions = tri_positions(&[1.0, 2.0]);
    let (out, ranges, prims) = psx_blend::append_semi_tail(&indices, &cba_tsb, &positions);
    assert_eq!(out, indices);
    assert_eq!(ranges, [(6, 0); 4]);
    assert!(prims.is_empty());
}

/// The per-prim depth key must follow the renderer's existing depth
/// convention: the clip-space `w` of a point under the draw MVP. For
/// the model origin that is exactly `mvp.w_axis.w` (the old per-draw
/// key), and by linearity the centroid's key equals the average of the
/// corner keys (PSX OT avg-Z binning).
#[test]
fn psx_blend_prim_depth_key_matches_origin_and_vertex_average() {
    let mvp = Mat4::from_cols(
        glam::Vec4::new(0.5, 1.0, -2.0, 0.25),
        glam::Vec4::new(3.0, -1.5, 0.5, -1.0),
        glam::Vec4::new(0.0, 2.0, 1.0, 4.0),
        glam::Vec4::new(7.0, -3.0, 2.0, 11.0),
    );
    // Origin key = w_axis.w, the previous per-draw ordering key.
    assert_eq!(psx_blend::prim_depth_key(&mvp, [0.0; 3]), mvp.w_axis.w);
    let corners = [[1.0f32, 2.0, 3.0], [-4.0, 0.5, 2.0], [6.0, -1.0, 7.0]];
    let centroid = [
        (corners[0][0] + corners[1][0] + corners[2][0]) / 3.0,
        (corners[0][1] + corners[1][1] + corners[2][1]) / 3.0,
        (corners[0][2] + corners[1][2] + corners[2][2]) / 3.0,
    ];
    let avg_of_keys = corners
        .iter()
        .map(|&c| {
            let v = mvp * glam::Vec4::new(c[0], c[1], c[2], 1.0);
            v.w
        })
        .sum::<f32>()
        / 3.0;
    let key = psx_blend::prim_depth_key(&mvp, centroid);
    assert!(
        (key - avg_of_keys).abs() < 1e-4,
        "centroid key {key} != avg of corner keys {avg_of_keys}"
    );
    // And the key really is the clip w of the centroid.
    let clip = mvp * glam::Vec4::new(centroid[0], centroid[1], centroid[2], 1.0);
    assert!((key - clip.w).abs() < 1e-5);
}

/// Two overlapping draws whose semi prims interleave in depth: the
/// sorted blend list must be globally back-to-front per PRIM, an order
/// no per-draw sort can produce.
#[test]
fn psx_blend_list_orders_prims_back_to_front_across_draws() {
    let semi = psx_blend::TSB_SEMI_TRANSPARENT_BIT;
    // Draw A (textured): two mode-0 semi prims at view depths 5 and 25.
    let cba_tsb = vec![[0u16, semi]; 6];
    let idx: Vec<u32> = (0..6).collect();
    let (_, _, prims_a) = psx_blend::append_semi_tail(&idx, &cba_tsb, &tri_positions(&[5.0, 25.0]));
    // Draw B (untextured): two mode-0 semi prims at view depths 11
    // and 19 - interleaving with A's.
    let blend = vec![psx_blend::pack_blend_word(true, 0); 6];
    let (_, _, prims_b) =
        psx_blend::append_semi_tail_words(&idx, &blend, &tri_positions(&[11.0, 19.0]));
    let mvp = z_to_w_mvp(0.0);
    let mut list = Vec::new();
    psx_blend::push_draw_prims(&mut list, false, 0, &mvp, &prims_a);
    psx_blend::push_draw_prims(&mut list, true, 1, &mvp, &prims_b);
    psx_blend::sort_blend_list(&mut list);
    // Globally far-to-near: 25 (A), 19 (B), 11 (B), 5 (A).
    let got: Vec<(bool, u32, f32)> = list
        .iter()
        .map(|e| (e.untextured, e.draw_index, e.key))
        .collect();
    assert_eq!(
        got,
        vec![
            (false, 0, 25.0),
            (true, 1, 19.0),
            (true, 1, 11.0),
            (false, 0, 5.0),
        ]
    );
    // Keys are strictly non-increasing = back-to-front per prim.
    assert!(list.windows(2).all(|w| w[0].key >= w[1].key));
    // A per-draw key (the draw origin, w_axis.w = 0 for both) could
    // never interleave these - the per-prim keys are what separate them.
    // NaN keys still sort deterministically (farthest first).
    let mut with_nan = list.clone();
    with_nan.push(psx_blend::BlendListEntry {
        key: f32::NAN,
        seq: 99,
        untextured: false,
        draw_index: 7,
        mode: 0,
        first_index: 0,
    });
    psx_blend::sort_blend_list(&mut with_nan);
    assert_eq!(with_nan[0].draw_index, 7);
}

/// Equal depth keys = one ordering-table bucket. Retail OT buckets are
/// LIFO (`AddPrim` prepends, `DrawOTag` walks head-first), so within a
/// bucket later-submitted prims draw FIRST.
#[test]
fn psx_blend_list_equal_depth_bucket_is_lifo() {
    let semi = psx_blend::TSB_SEMI_TRANSPARENT_BIT;
    let cba_tsb = vec![[0u16, semi]; 6];
    let idx: Vec<u32> = (0..6).collect();
    // Both draws' prims all sit at the same depth (z = 8).
    let (_, _, prims) = psx_blend::append_semi_tail(&idx, &cba_tsb, &tri_positions(&[8.0, 8.0]));
    let mvp = z_to_w_mvp(0.0);
    let mut list = Vec::new();
    psx_blend::push_draw_prims(&mut list, false, 0, &mvp, &prims);
    psx_blend::push_draw_prims(&mut list, false, 1, &mvp, &prims);
    psx_blend::sort_blend_list(&mut list);
    // 4 entries, all key 8: submission order was seq 0,1 (draw 0) then
    // seq 2,3 (draw 1); LIFO draws seq 3,2,1,0.
    let seqs: Vec<u32> = list.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, vec![3, 2, 1, 0]);
    let draws: Vec<u32> = list.iter().map(|e| e.draw_index).collect();
    assert_eq!(draws, vec![1, 1, 0, 0]);
}

/// Sorted-run coalescing: consecutive entries from the same draw +
/// mode with contiguous tail triangles merge into one indexed draw;
/// any change of draw, mode, or contiguity splits the run.
#[test]
fn psx_blend_coalesce_merges_contiguous_tail_runs() {
    let semi = psx_blend::TSB_SEMI_TRANSPARENT_BIT;
    // One draw, 3 mode-0 semi prims at strictly descending depth so the
    // sort keeps tail order and the runs stay contiguous, plus a
    // mode-2 prim in between depths that splits the run.
    let cba_tsb = vec![
        [0u16, semi],
        [0, semi],
        [0, semi],
        [0, semi],
        [0, semi],
        [0, semi],
        [0, semi | (2 << 5)],
        [0, semi | (2 << 5)],
        [0, semi | (2 << 5)],
        [0, semi],
        [0, semi],
        [0, semi],
    ];
    let idx: Vec<u32> = (0..12).collect();
    // Depths: 40, 30, (mode 2) 20, 10.
    let (_, ranges, prims) =
        psx_blend::append_semi_tail(&idx, &cba_tsb, &tri_positions(&[40.0, 30.0, 20.0, 10.0]));
    // Mode-0 tail holds prims 0,1,3; mode-2 tail holds prim 2.
    assert_eq!(ranges[0], (12, 9));
    assert_eq!(ranges[2], (21, 3));
    let mvp = z_to_w_mvp(0.0);
    let mut list = Vec::new();
    psx_blend::push_draw_prims(&mut list, false, 0, &mvp, &prims);
    psx_blend::sort_blend_list(&mut list);
    let mut runs: Vec<(u8, u32, u32)> = Vec::new();
    psx_blend::coalesce_sorted(&list, |head, start, count| {
        runs.push((head.mode, start, count));
    });
    // 40 + 30 are contiguous mode-0 tail slots (12, 15) -> one run of
    // 6 indices; then the mode-2 prim at 20 (tail 21); then the last
    // mode-0 prim at 10 (tail 18, not contiguous with the first run).
    assert_eq!(runs, vec![(0, 12, 6), (2, 21, 3), (0, 18, 3)]);
}

/// Coalescing never merges across draw boundaries even when tail
/// indices happen to line up.
#[test]
fn psx_blend_coalesce_splits_on_draw_change() {
    let entries = [
        psx_blend::BlendListEntry {
            key: 9.0,
            seq: 0,
            untextured: false,
            draw_index: 0,
            mode: 0,
            first_index: 12,
        },
        psx_blend::BlendListEntry {
            key: 8.0,
            seq: 1,
            untextured: false,
            draw_index: 1,
            mode: 0,
            first_index: 15,
        },
        psx_blend::BlendListEntry {
            key: 7.0,
            seq: 2,
            untextured: true,
            draw_index: 1,
            mode: 0,
            first_index: 18,
        },
    ];
    let mut runs = Vec::new();
    psx_blend::coalesce_sorted(&entries, |head, start, count| {
        runs.push((head.untextured, head.draw_index, start, count));
    });
    assert_eq!(
        runs,
        vec![(false, 0, 12, 3), (false, 1, 15, 3), (true, 1, 18, 3)]
    );
}

/// `pack_blend_word` must round-trip through the extractors the blend
/// pass uses, and must agree bit-for-bit with the TSB packing the
/// textured path rides (ABE bit 15, ABR bits 5..=6).
#[test]
fn psx_blend_pack_blend_word_round_trips() {
    for abr in 0u8..4 {
        let semi = psx_blend::pack_blend_word(true, abr);
        assert!(psx_blend::prim_semi_transparent(semi));
        assert_eq!(psx_blend::abr_mode(semi), abr);
        assert_eq!(semi, 0x8000 | ((abr as u16) << 5));
        let opaque = psx_blend::pack_blend_word(false, abr);
        assert!(!psx_blend::prim_semi_transparent(opaque));
        assert_eq!(psx_blend::abr_mode(opaque), abr);
    }
    // Out-of-range ABR is masked to 2 bits.
    assert_eq!(psx_blend::abr_mode(psx_blend::pack_blend_word(true, 7)), 3);
}

/// The word-slice variant (untextured colour-mesh path) must bucket
/// identically to `append_semi_tail` given equivalent per-vertex words.
#[test]
fn psx_blend_append_semi_tail_words_buckets_per_mode() {
    // 4 prims x 3 per-corner verts: opaque, semi ABR 0, semi ABR 2,
    // semi ABR 3 (in that order) - the colour-mesh packing of the
    // textured test's TSB values.
    let mut blend = Vec::new();
    for (abe, abr) in [(false, 0u8), (true, 0), (true, 2), (true, 3)] {
        blend.extend_from_slice(&[psx_blend::pack_blend_word(abe, abr); 3]);
    }
    let indices: Vec<u32> = (0..12).collect();
    let positions = tri_positions(&[1.0, 2.0, 3.0, 4.0]);
    let (out, ranges, prims) = psx_blend::append_semi_tail_words(&indices, &blend, &positions);
    // Original indices untouched at the front (the opaque pass range).
    assert_eq!(&out[..12], indices.as_slice());
    assert_eq!(ranges[0], (12, 3));
    assert_eq!(ranges[1], (15, 0));
    assert_eq!(ranges[2], (15, 3));
    assert_eq!(ranges[3], (18, 3));
    assert_eq!(&out[12..15], &[3, 4, 5]);
    assert_eq!(&out[15..18], &[6, 7, 8]);
    assert_eq!(&out[18..21], &[9, 10, 11]);

    // Cross-check against the textured-path partitioner on the same data.
    let cba_tsb: Vec<[u16; 2]> = blend.iter().map(|&w| [0u16, w]).collect();
    let (out_tsb, ranges_tsb, prims_tsb) =
        psx_blend::append_semi_tail(&indices, &cba_tsb, &positions);
    assert_eq!(out, out_tsb);
    assert_eq!(ranges, ranges_tsb);
    assert_eq!(prims, prims_tsb);
}

#[test]
fn psx_blend_append_semi_tail_words_all_opaque_is_identity() {
    let blend = vec![psx_blend::pack_blend_word(false, 1); 6];
    let indices: Vec<u32> = vec![0, 1, 2, 3, 4, 5];
    let positions = tri_positions(&[1.0, 2.0]);
    let (out, ranges, prims) = psx_blend::append_semi_tail_words(&indices, &blend, &positions);
    assert_eq!(out, indices);
    assert_eq!(ranges, [(6, 0); 4]);
    assert!(prims.is_empty());
}
