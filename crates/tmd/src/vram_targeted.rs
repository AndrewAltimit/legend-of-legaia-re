//! Targeted VRAM upload + per-prim VRAM diagnostics.
//!
//! The naive "upload every TIM in this PROT entry into VRAM" approach
//! collides badly: a single PROT entry can carry hundreds of TIMs, and
//! the 1MB VRAM only fits ~64 distinct 256x256 pages. Most retail TIM
//! bundles assume the runtime asset chain has filtered the upload set
//! down to the textures the current scene actually samples; without
//! that filter, image data lands on top of CLUT rows another mesh
//! references and the paletted decode reads image pixels as palette
//! entries (rainbow noise).
//!
//! [`build_vram_targeted`] walks every TIM under one or more candidate
//! directories, then *per-block* decides whether to write the image
//! and / or the CLUT block based on whether the block intersects with
//! the rectangles a given TMD's primitives sample. A TIM can contribute
//! one block, both, or neither - skipping the half that would clobber
//! someone else's data.
//!
//! The same logic is used by both the `asset-viewer` window and the
//! `tmd prims --vram-dir` diagnostic, so the on-screen render and the
//! offline report agree on which prims are renderable.

use std::path::Path;

use legaia_tim::Vram;

use crate::Tmd;

/// VRAM rectangles a single primitive's CBA / TSB lookup will touch.
/// `clut` is the 1-row CLUT block; `page` is the (width, height) of
/// the sampled portion of the texture page. Coordinates are VRAM-word
/// units (= 16bpp pixels). Used by [`collect_prim_targets`] +
/// [`build_vram_targeted`] to skip TIM uploads that have nothing to do
/// with the current mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimTarget {
    /// `(x, y, w, h)` of the CLUT row the prim samples (in VRAM-word
    /// space). Width is 16 for a 4bpp prim, 256 for 8bpp, 0 for 15bpp
    /// direct (no CLUT).
    pub clut: (u16, u16, u16, u16),
    /// `(x, y, w, h)` of the texture-page region the prim's UV bbox
    /// hits, in VRAM-word space. Already accounts for 4bpp packing
    /// 4 pixels per word, 8bpp packing 2 per word, 15bpp 1:1.
    pub page: (u16, u16, u16, u16),
}

/// Collect the VRAM rectangles every textured primitive in `tmd` will
/// sample. Empty result means the mesh has no textured prims (e.g. a
/// stage prop with only Gouraud-shaded geometry).
pub fn collect_prim_targets(tmd: &Tmd, buf: &[u8]) -> Vec<PrimTarget> {
    let mut out = Vec::new();
    for o in &tmd.objects {
        let groups = crate::legaia_prims::iter_groups_lenient(
            buf,
            o.primitives_byte_offset,
            o.primitives_byte_size,
        );
        for g in &groups {
            for p in &g.prims {
                if p.uvs.is_empty() {
                    continue;
                }
                let (cx, cy) = p.cba_xy();
                let (px, py, depth, _abr) = p.tpage_xy();
                let clut_w: u16 = match depth {
                    4 => 16,
                    8 => 256,
                    _ => 0,
                };
                let mut umin = u8::MAX;
                let mut umax = 0u8;
                let mut vmin = u8::MAX;
                let mut vmax = 0u8;
                for &(u, v) in &p.uvs {
                    umin = umin.min(u);
                    umax = umax.max(u);
                    vmin = vmin.min(v);
                    vmax = vmax.max(v);
                }
                let (u_lo, u_hi) = match depth {
                    4 => (umin as u16 >> 2, umax as u16 >> 2),
                    8 => (umin as u16 >> 1, umax as u16 >> 1),
                    _ => (umin as u16, umax as u16),
                };
                let page_w = u_hi.saturating_sub(u_lo) + 1;
                let page_h = (vmax as u16).saturating_sub(vmin as u16) + 1;
                out.push(PrimTarget {
                    clut: (cx, cy, clut_w, 1),
                    page: (px + u_lo, py + vmin as u16, page_w, page_h),
                });
            }
        }
    }
    out
}

/// Axis-aligned rectangle intersection test in VRAM-word space.
fn rects_overlap(a: (u16, u16, u16, u16), b: (u16, u16, u16, u16)) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

/// Per-TIM upload report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VramUploadStats {
    /// TIMs scanned across all dirs.
    pub total_tims: usize,
    /// TIMs that contributed at least one block.
    pub uploaded_tims: usize,
    /// Image and CLUT block both written.
    pub uploaded_both: usize,
    /// Only image block written (CLUT block would have collided with a
    /// prim's CLUT row, so it was suppressed).
    pub uploaded_image_only: usize,
    /// Only CLUT block written (image block would have collided with a
    /// prim's texture page).
    pub uploaded_clut_only: usize,
}

/// Walk every `*.tim` under each candidate `dirs` and write the blocks
/// useful for `needs` into a fresh `Vram`. Returns the populated VRAM
/// alongside per-block statistics.
///
/// For every TIM we decide *independently* whether to write its image
/// block and its CLUT block:
///
/// - **Image block** is written iff it overlaps at least one mesh
///   texture-page rectangle AND it does NOT overlap any mesh CLUT
///   rectangle. Skipping the image when it would land on a CLUT row
///   is what removes the rainbow-noise symptom - otherwise the
///   paletted decode reads image bytes as BGR555 palette entries.
///
/// - **CLUT block** is written iff it overlaps at least one mesh CLUT
///   rectangle AND it does NOT overlap any mesh page rectangle.
///
/// Falls back to "upload every block" when `needs` is empty (the mesh
/// has no textured prims, so collisions can't happen).
pub fn build_vram_targeted(dirs: &[&Path], needs: &[PrimTarget]) -> (Vram, VramUploadStats) {
    if needs.is_empty() {
        let (vram, count, total) = build_vram_unfiltered(dirs);
        return (
            vram,
            VramUploadStats {
                total_tims: total,
                uploaded_tims: count,
                uploaded_both: count,
                uploaded_image_only: 0,
                uploaded_clut_only: 0,
            },
        );
    }
    // Materialise once - we need two passes (images, then CLUTs) for
    // the same dual-use-row reason documented on
    // `build_vram_targeted_from_buffers`.
    let mut tims: Vec<legaia_tim::Tim> = Vec::new();
    let mut total = 0usize;
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("tim")) {
                continue;
            }
            let Ok(buf) = std::fs::read(&p) else {
                continue;
            };
            total += 1;
            if let Ok(tim) = legaia_tim::parse(&buf) {
                tims.push(tim);
            }
        }
    }

    let mut stats = VramUploadStats {
        total_tims: total,
        ..Default::default()
    };
    let decisions: Vec<(bool, bool)> = tims
        .iter()
        .map(|tim| {
            let img = &tim.image;
            let img_rect = (img.fb_x, img.fb_y, img.fb_w, img.h);
            let clut_rect = tim.clut.as_ref().map(|c| (c.fb_x, c.fb_y, c.w, c.h));

            let img_useful = needs.iter().any(|t| rects_overlap(img_rect, t.page));
            let img_collides_clut = needs.iter().any(|t| rects_overlap(img_rect, t.clut));
            let clut_useful =
                clut_rect.is_some_and(|r| needs.iter().any(|t| rects_overlap(r, t.clut)));

            (img_useful && !img_collides_clut, clut_useful)
        })
        .collect();

    let mut vram = Vram::new();
    for (tim, &(upload_image, _)) in tims.iter().zip(&decisions) {
        if upload_image {
            vram.upload_tim_partial(tim, true, false);
        }
    }
    for (tim, &(_, upload_clut)) in tims.iter().zip(&decisions) {
        if upload_clut {
            vram.upload_tim_partial(tim, false, true);
        }
    }

    for &(upload_image, upload_clut) in &decisions {
        if !upload_image && !upload_clut {
            continue;
        }
        stats.uploaded_tims += 1;
        match (upload_image, upload_clut) {
            (true, true) => stats.uploaded_both += 1,
            (true, false) => stats.uploaded_image_only += 1,
            (false, true) => stats.uploaded_clut_only += 1,
            _ => {}
        }
    }
    (vram, stats)
}

/// Upload every TIM in every dir without any per-block filtering. Use
/// when the caller doesn't have a TMD to drive the targeted upload (a
/// scene-level pre-pass that just wants every TIM the disc carries
/// landed at its canonical fb_x/fb_y). Returns `(vram, uploaded_count)`;
/// bad / unparseable TIMs are skipped silently.
pub fn build_vram_from_dirs(dirs: &[&Path]) -> (Vram, usize) {
    let (vram, count, _total) = build_vram_unfiltered(dirs);
    (vram, count)
}

/// In-memory variant of [`build_vram_targeted`]. Takes an iterator over
/// already-loaded TIM byte slices (e.g. from `tim_scan::scan_buffer`
/// hits inside a PROT entry) instead of walking the filesystem, so an
/// engine driving the scene-asset chain doesn't need on-disk
/// intermediates.
///
/// Returns the populated VRAM + per-block statistics.
///
/// ## Upload ordering
///
/// Done in two passes to keep the dual-use lower rows of texture pages
/// (where PSX games place 16-packed 4bpp palettes - typically y=479,
/// y=480, y=500) coherent:
///
/// 1. **Image pass** uploads every TIM image block whose region
///    overlaps a mesh's sampled texture page. The `img_collides_clut`
///    check still suppresses image uploads that would land directly on
///    another mesh's CLUT row (preventing the rainbow-noise symptom).
/// 2. **CLUT pass** uploads every TIM CLUT block whose row overlaps a
///    mesh's referenced CLUT row, *unconditionally* with respect to
///    image-page collisions. CLUTs sit in the last 16-32 rows of VRAM
///    where PSX games leave the texture pages empty by design - the
///    bytes serve a dual purpose (image storage + palette storage). If
///    an image upload happened to land on those rows in pass 1, the
///    CLUT pass overwrites them with the canonical palette data.
///
/// Earlier versions did a single pass with a `clut_collides_page`
/// suppression check; that dropped legitimate CLUT uploads when *any*
/// mesh's UV bbox happened to brush a CLUT row (typical in town /
/// field scenes where one mesh samples deep into a 256x256 page while
/// another mesh stores its palette on the bottom row of the same
/// page). Cf. the `town01` regression: 4 character TMDs dropped 388
/// prims because their packed palettes at row 479 lost the upload
/// race to image rectangles that brushed the same row.
pub fn build_vram_targeted_from_buffers<'a, I>(
    tim_bufs: I,
    needs: &[PrimTarget],
) -> (Vram, VramUploadStats)
where
    I: IntoIterator<Item = &'a [u8]>,
{
    let mut vram = Vram::new();
    let mut stats = VramUploadStats::default();
    if needs.is_empty() {
        // Empty needs → no targeting to do, upload everything as-is. Same
        // semantics as `build_vram_from_dirs`'s fallback path.
        for buf in tim_bufs {
            stats.total_tims += 1;
            let Ok(tim) = legaia_tim::parse(buf) else {
                continue;
            };
            vram.upload_tim(&tim);
            stats.uploaded_tims += 1;
            stats.uploaded_both += 1;
        }
        return (vram, stats);
    }

    // Materialise once so we can do two passes over the same set.
    let parsed: Vec<legaia_tim::Tim> = tim_bufs
        .into_iter()
        .filter_map(|buf| {
            stats.total_tims += 1;
            legaia_tim::parse(buf).ok()
        })
        .collect();

    // Pre-compute decisions per TIM so the stats reflect what actually
    // happens after both passes, not the intermediate state.
    let decisions: Vec<(bool, bool)> = parsed
        .iter()
        .map(|tim| {
            let img = &tim.image;
            let img_rect = (img.fb_x, img.fb_y, img.fb_w, img.h);
            let clut_rect = tim.clut.as_ref().map(|c| (c.fb_x, c.fb_y, c.w, c.h));

            let img_useful = needs.iter().any(|t| rects_overlap(img_rect, t.page));
            let img_collides_clut = needs.iter().any(|t| rects_overlap(img_rect, t.clut));
            let clut_useful =
                clut_rect.is_some_and(|r| needs.iter().any(|t| rects_overlap(r, t.clut)));

            let upload_image = img_useful && !img_collides_clut;
            // CLUT uploads no longer suppressed by page collisions:
            // CLUT bytes always win the upload race in pass 2.
            let upload_clut = clut_useful;
            (upload_image, upload_clut)
        })
        .collect();

    // Pass 1: image blocks.
    for (tim, &(upload_image, _)) in parsed.iter().zip(&decisions) {
        if upload_image {
            vram.upload_tim_partial(tim, true, false);
        }
    }
    // Pass 2: CLUT blocks with merge-zeros semantics. Always last so
    // palette rows survive any image upload that brushed the bottom of
    // a texture page. Multiple scene-pack TIMs frequently target the
    // same CLUT row, each populating a different subset of the 16-color
    // slots (the remaining slots on disc are filler zeros); merge mode
    // preserves earlier non-zero entries when a later TIM's slot is
    // empty, producing the union of all per-TIM contributions. See the
    // `town01` regression: 7 row-479 TIMs in entries 6..9 split into
    // "full" (slots 0..14) and "partial" (slots 0..7) - the last-write-
    // wins ordering would drop slots 8..14 entirely.
    for (tim, &(_, upload_clut)) in parsed.iter().zip(&decisions) {
        if upload_clut {
            vram.upload_tim_partial_opts(tim, false, true, true);
        }
    }

    for &(upload_image, upload_clut) in &decisions {
        if !upload_image && !upload_clut {
            continue;
        }
        stats.uploaded_tims += 1;
        match (upload_image, upload_clut) {
            (true, true) => stats.uploaded_both += 1,
            (true, false) => stats.uploaded_image_only += 1,
            (false, true) => stats.uploaded_clut_only += 1,
            _ => {}
        }
    }

    (vram, stats)
}

/// Upload **every** parseable TIM in `tim_bufs` to its header `(fb_x, fb_y)`
/// destination - the byte-faithful analogue of the retail field loader, which
/// DMAs every TIM in the scene to VRAM regardless of which prim samples it
/// (see [`docs/subsystems/asset-loader.md`]). Unlike
/// [`build_vram_targeted_from_buffers`], there is no render `needs` filter;
/// this is for the VRAM parity oracle, where the goal is to reproduce the live
/// VRAM rather than the minimal set a renderer samples.
///
/// Two passes preserve the same CLUT semantics as the targeted builder:
/// pass 1 writes all image blocks (last-write-wins in iteration order, like
/// sequential DMA), pass 2 writes all CLUT blocks with **merge-zeros** so the
/// row-479 multi-TIM palette split survives (each TIM populating a different
/// subset of the 16 colour slots - see the `town01` row-479 regression).
pub fn build_vram_full_from_buffers<'a, I>(tim_bufs: I) -> (Vram, VramUploadStats)
where
    I: IntoIterator<Item = &'a [u8]>,
{
    let mut vram = Vram::new();
    let mut stats = VramUploadStats::default();
    let parsed: Vec<legaia_tim::Tim> = tim_bufs
        .into_iter()
        .filter_map(|buf| {
            stats.total_tims += 1;
            legaia_tim::parse(buf).ok()
        })
        .collect();
    // Pass 1: image blocks.
    for tim in &parsed {
        vram.upload_tim_partial(tim, true, false);
    }
    // Pass 2: CLUT blocks last, with merge-zeros semantics.
    for tim in &parsed {
        if tim.clut.is_some() {
            vram.upload_tim_partial_opts(tim, false, true, true);
        }
    }
    for tim in &parsed {
        stats.uploaded_tims += 1;
        if tim.clut.is_some() {
            stats.uploaded_both += 1;
        } else {
            stats.uploaded_image_only += 1;
        }
    }
    (vram, stats)
}

/// Internal helper used by both [`build_vram_from_dirs`] and the
/// targeted-upload empty-needs fallback.
fn build_vram_unfiltered(dirs: &[&Path]) -> (Vram, usize, usize) {
    let mut vram = Vram::new();
    let mut count = 0;
    let mut total = 0;
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("tim")) {
                continue;
            }
            let Ok(buf) = std::fs::read(&p) else {
                continue;
            };
            let Ok(tim) = legaia_tim::parse(&buf) else {
                continue;
            };
            total += 1;
            vram.upload_tim(&tim);
            count += 1;
        }
    }
    (vram, count, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rects_overlap_basic_cases() {
        // Touching edges don't overlap.
        assert!(!rects_overlap((0, 0, 4, 4), (4, 0, 4, 4)));
        // True overlap.
        assert!(rects_overlap((0, 0, 4, 4), (2, 2, 4, 4)));
        // Disjoint.
        assert!(!rects_overlap((0, 0, 4, 4), (10, 10, 4, 4)));
    }

    #[test]
    fn build_vram_targeted_empty_needs_falls_back_to_full_upload() {
        let dirs: Vec<&Path> = Vec::new();
        let (vram, stats) = build_vram_targeted(&dirs, &[]);
        assert_eq!(stats.total_tims, 0);
        assert_eq!(stats.uploaded_tims, 0);
        // Empty VRAM, no uploads attempted.
        assert_eq!(vram.pixel(0, 0), 0);
    }

    /// Synthetic 4bpp TIM: image at `(img_x, img_y)` 1x1 (just enough
    /// for the upload to register), CLUT at `(clut_x, clut_y)` 16-wide.
    fn tim_4bpp_at(img_x: u16, img_y: u16, clut_x: u16, clut_y: u16) -> Vec<u8> {
        let mut buf = Vec::new();
        // Header: id=0x10, flags=0x08 (4bpp with CLUT)
        buf.extend_from_slice(&0x10u32.to_le_bytes());
        buf.extend_from_slice(&0x08u32.to_le_bytes());
        // CLUT block: len = 12 (header) + 16 entries * 2 = 44
        buf.extend_from_slice(&44u32.to_le_bytes());
        buf.extend_from_slice(&clut_x.to_le_bytes());
        buf.extend_from_slice(&clut_y.to_le_bytes());
        buf.extend_from_slice(&16u16.to_le_bytes()); // w
        buf.extend_from_slice(&1u16.to_le_bytes()); // h
        // 16 distinct non-zero entries
        for i in 0..16u16 {
            buf.extend_from_slice(&(0x1000 | i).to_le_bytes());
        }
        // Image block: 1x1, fb_w=1 (one 16-bit word holds 4 4bpp pixels)
        buf.extend_from_slice(&14u32.to_le_bytes()); // bs_len = 8 + 1*1*2 = 10... wait header is 12 then 1 word data so 14
        buf.extend_from_slice(&img_x.to_le_bytes());
        buf.extend_from_slice(&img_y.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes()); // fb_w
        buf.extend_from_slice(&1u16.to_le_bytes()); // h
        buf.extend_from_slice(&0xAAAAu16.to_le_bytes());
        buf
    }

    /// Two TIMs collide: TIM A's image at (16, 0); TIM B's CLUT at (16, 0).
    /// The mesh wants TIM B's CLUT for paletted decode. The targeted
    /// builder must suppress TIM A's image upload to keep TIM B's CLUT
    /// row intact - otherwise the paletted decode reads TIM A's image
    /// bytes as palette entries (rainbow noise).
    #[test]
    fn from_buffers_suppresses_image_on_clut_collision() {
        let tim_a = tim_4bpp_at(/*img*/ 16, 0, /*clut*/ 0, 100);
        let tim_b = tim_4bpp_at(/*img*/ 64, 64, /*clut*/ 16, 0);
        // Mesh references CLUT at (16, 0) with 4bpp width 16, page at (64, 64).
        let needs = vec![PrimTarget {
            clut: (16, 0, 16, 1),
            page: (64, 64, 1, 1),
        }];
        let (vram, stats) =
            build_vram_targeted_from_buffers([tim_a.as_slice(), tim_b.as_slice()], &needs);
        assert_eq!(stats.total_tims, 2);
        // TIM A's image (16, 0) would collide with TIM B's CLUT - suppressed.
        // But TIM A's CLUT (0, 100) is useful for no prim - suppressed.
        // TIM B's image (64, 64) is useful + no collision - written.
        // TIM B's CLUT (16, 0) is useful + no collision - written.
        // So TIM A contributes nothing; TIM B contributes both blocks.
        assert_eq!(stats.uploaded_tims, 1);
        assert_eq!(stats.uploaded_both, 1);
        assert_eq!(stats.uploaded_image_only, 0);
        assert_eq!(stats.uploaded_clut_only, 0);
        // TIM B's CLUT row entry [0] is 0x1000 (not TIM A's image bytes).
        assert_eq!(vram.pixel(16, 0), 0x1000);
    }

    #[test]
    fn from_buffers_empty_needs_uploads_everything() {
        let tim_a = tim_4bpp_at(16, 0, 0, 100);
        let (vram, stats) = build_vram_targeted_from_buffers([tim_a.as_slice()], &[]);
        assert_eq!(stats.total_tims, 1);
        assert_eq!(stats.uploaded_tims, 1);
        assert_eq!(stats.uploaded_both, 1);
        // CLUT entry [0] visible.
        assert_eq!(vram.pixel(0, 100), 0x1000);
    }

    /// Regression for the town01 case. TIM A's image at (0, 0) covers
    /// a 256x256 region; TIM B's CLUT lives at (16, 100) (dual-use
    /// "palette row at the bottom of a texture page" layout common in
    /// PSX games). A different mesh references a texture page rect
    /// that crosses y=100 (so collides with TIM B's CLUT row coordinate
    /// in rect terms), AND another mesh's prim references the CLUT
    /// row TIM B supplies. Pre-fix, this combination dropped TIM B's
    /// CLUT upload entirely because of the spurious `clut_collides_page`
    /// check. Post-fix, the 2-pass upload writes images first then
    /// CLUTs unconditionally, so TIM B's palette survives.
    #[test]
    fn from_buffers_clut_survives_when_another_prims_page_crosses_it() {
        let tim_b = tim_4bpp_at(/*img*/ 64, 64, /*clut*/ 16, 100);
        let needs = vec![
            // Mesh X samples TIM B's palette.
            PrimTarget {
                clut: (16, 100, 16, 1),
                page: (64, 64, 1, 1),
            },
            // Mesh Y samples some other tex page whose vertical
            // extent crosses y=100. Pre-fix, the engine would refuse
            // to upload TIM B's CLUT block because `(16, 100, 16, 1)`
            // overlaps `(0, 0, 256, 200)`.
            PrimTarget {
                clut: (0, 200, 16, 1),
                page: (0, 0, 256, 200),
            },
        ];
        let (vram, stats) = build_vram_targeted_from_buffers([tim_b.as_slice()], &needs);
        assert_eq!(stats.total_tims, 1);
        assert_eq!(stats.uploaded_tims, 1);
        // TIM B's CLUT entry [0] must survive at (16, 100).
        assert_eq!(
            vram.pixel(16, 100),
            0x1000,
            "CLUT row must not be suppressed by an unrelated page collision"
        );
    }
}
