//! VRAM parity/trace subcommands (`clut-trace`, `vram-oracle`) + coverage helpers.
//!
//! Mechanical split from `commands.rs` (behavior-preserving).

use super::*;

/// Walk a scene's TMD pool, locate every primitive that drops as
/// `MissingClut`, and report which PROT entries on the disc carry a TIM
/// whose CLUT block lands at the missing row. Optional runtime VRAM
/// cross-check distinguishes "row absent from the engine but present at
/// runtime" (engine loader gap) from "row absent from runtime too"
/// (mesh references unreachable CLUT - likely a parser issue).
pub(crate) fn cmd_clut_trace(
    scene_name: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    runtime_vram: Option<&Path>,
    max_sources: usize,
) -> Result<()> {
    use legaia_asset::tim_scan;
    use legaia_tim::vram::PrimTextureStatus;
    use std::collections::BTreeMap;

    let index = open_index_from_args(extracted_root, disc)?;
    let scene =
        Scene::load(&index, scene_name).with_context(|| format!("load scene '{scene_name}'"))?;

    let mut shared_scenes: Vec<Scene> = Vec::new();
    for name in FIELD_SHARED_BLOCKS {
        if let Ok(s) = Scene::load(&index, name) {
            shared_scenes.push(s);
        }
    }
    let shared_refs: Vec<&Scene> = shared_scenes.iter().collect();
    let (resources, _upload_stats) = SceneResources::build_targeted(&scene, &shared_refs)?;

    println!("scene '{}'", scene.name);
    println!(
        "  shared blocks loaded: {:?}",
        shared_scenes
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
    );
    println!(
        "  TMDs: {}  TIMs uploaded: {}",
        resources.tmds.len(),
        resources.tim_count
    );

    // Group dropped prims by (cba, depth). Multiple prims in multiple TMDs
    // often share the same CLUT row; we only need to find the supplier
    // once per unique row.
    let mut dropped: BTreeMap<(u16, u8), DroppedClut> = BTreeMap::new();
    for rtmd in &resources.tmds {
        for obj in &rtmd.tmd.objects {
            let groups = legaia_tmd::legaia_prims::iter_groups_lenient(
                &rtmd.raw,
                obj.primitives_byte_offset,
                obj.primitives_byte_size,
            );
            for g in &groups {
                for prim in &g.prims {
                    if prim.uvs.is_empty() {
                        continue;
                    }
                    let depth = match (prim.tsb >> 7) & 0x3 {
                        0 => 4u8,
                        1 => 8,
                        _ => continue, // 16bpp / direct: no CLUT to be missing
                    };
                    let status = resources
                        .vram
                        .prim_texture_status(prim.cba, prim.tsb, &prim.uvs);
                    if let PrimTextureStatus::MissingClut { .. } = status {
                        let entry = dropped.entry((prim.cba, depth)).or_default();
                        entry.prim_count += 1;
                        entry.tmd_locations.insert((rtmd.entry_idx, rtmd.offset));
                    }
                }
            }
        }
    }

    if dropped.is_empty() {
        println!("  no MissingClut drops detected in this scene");
        return Ok(());
    }

    let runtime_bytes = match runtime_vram {
        Some(p) => Some(
            std::fs::read(p).with_context(|| format!("read runtime VRAM blob {}", p.display()))?,
        ),
        None => None,
    };
    if let Some(b) = &runtime_bytes
        && b.len() != 1024 * 512 * 2
    {
        anyhow::bail!("runtime VRAM size {} != 1 MiB (1024*512*2)", b.len());
    }

    // Pre-scan every PROT entry once: collect (entry_idx, cba_fb_x,
    // cba_fb_y, depth). One pass through the disc; subsequent lookups
    // are cheap.
    println!("  scanning PROT corpus for CLUT suppliers ...");
    let mut suppliers: Vec<TimSupplier> = Vec::new();
    for idx in 0..index.entry_count() as u32 {
        let Ok(bytes) = index.entry_bytes(idx) else {
            continue;
        };
        for hit in tim_scan::scan_buffer(&bytes) {
            let Ok(tim) = legaia_tim::parse(&bytes[hit.offset..hit.offset + hit.byte_len]) else {
                continue;
            };
            let Some(clut) = tim.clut.as_ref() else {
                continue;
            };
            suppliers.push(TimSupplier {
                entry_idx: idx,
                offset: hit.offset,
                fb_x: clut.fb_x,
                fb_y: clut.fb_y,
                width: clut.w,
                bpp: hit.bpp,
            });
        }
    }
    println!(
        "  scanned {} PROT entries, found {} TIMs with CLUT blocks",
        index.entry_count(),
        suppliers.len()
    );

    // For each unique missing (cba, depth) report what we found.
    println!();
    println!(
        "  {} unique missing CLUT row(s) across the scene's TMDs:",
        dropped.len()
    );
    let mut supplier_entries: BTreeMap<u32, BTreeMap<&str, ()>> = BTreeMap::new();
    let mut shared_block_recommend: BTreeMap<&'static str, u32> = BTreeMap::new();
    for ((cba, depth), info) in &dropped {
        let cx = (cba & 0x3F) * 16;
        let cy = (cba >> 6) & 0x1FF;
        let clut_w: usize = match depth {
            4 => 16,
            8 => 256,
            _ => 0,
        };
        let in_runtime = match runtime_bytes.as_ref() {
            Some(b) => row_has_data(b, cx as usize, cy as usize, clut_w),
            None => false,
        };

        // Match by rectangle CONTAINMENT - a TIM CLUT block covers the
        // missing slot if its (fb_x, fb_y, width, 1) rect contains
        // (cx, cy). PSX games commonly pack 16 distinct 4bpp palettes
        // into one 256-wide CLUT block, so the CBA's 16-pixel slot
        // sits inside a wider supplier rect.
        let matching: Vec<&TimSupplier> = suppliers
            .iter()
            .filter(|s| s.fb_y == cy && s.fb_x <= cx && (cx + clut_w as u16) <= (s.fb_x + s.width))
            .collect();

        println!(
            "    cba=0x{:04X} depth={}bpp clut@({:4},{:3}) prims={:4} tmds={:2} runtime_has_row={}",
            cba,
            depth,
            cx,
            cy,
            info.prim_count,
            info.tmd_locations.len(),
            in_runtime
        );
        if matching.is_empty() {
            println!("      ! no PROT entry on disc supplies this row");
        } else {
            for s in matching.iter().take(max_sources) {
                let scene_name = index.scene_for_index(s.entry_idx).unwrap_or("?");
                supplier_entries
                    .entry(s.entry_idx)
                    .or_default()
                    .insert(scene_name, ());
                if let Some(static_name) = known_scene_block_for(scene_name) {
                    *shared_block_recommend.entry(static_name).or_default() += 1;
                }
                println!(
                    "      supplier: PROT {:4} ({}) off=0x{:06X} clut_w={} bpp={}",
                    s.entry_idx, scene_name, s.offset, s.width, s.bpp
                );
            }
            if matching.len() > max_sources {
                println!(
                    "      ... {} more supplier(s) suppressed",
                    matching.len() - max_sources
                );
            }
        }
    }

    println!();
    println!("  PROT entries the engine would need to keep resident:");
    for (entry, scenes) in &supplier_entries {
        let scene_list = scenes.keys().copied().collect::<Vec<_>>().join(", ");
        println!("    PROT {entry:4} (scene blocks: {scene_list})");
    }

    if !shared_block_recommend.is_empty() {
        println!();
        println!("  recommended FIELD_SHARED_BLOCKS additions (by supplier hit count):");
        for (name, hits) in &shared_block_recommend {
            println!("    \"{name}\"   (supplies {hits} missing row(s))");
        }
    }

    Ok(())
}

/// Map a free-form CDNAME scene label to a stable shared-block name
/// the engine knows how to load. Conservative: only return a name if it
/// matches one of the well-known shared blocks we'd actually pin into
/// VRAM, not a per-scene town/field block.
fn known_scene_block_for(scene_name: &str) -> Option<&'static str> {
    match scene_name {
        "init_data" => Some("init_data"),
        "player_data" => Some("player_data"),
        "battle_data" => Some("battle_data"),
        "befect_data" => Some("befect_data"),
        "sound_data" => Some("sound_data"),
        "sound_data2" => Some("sound_data2"),
        "gameover_data" => Some("gameover_data"),
        "card_data" => Some("card_data"),
        _ => None,
    }
}

#[derive(Default)]
struct DroppedClut {
    prim_count: usize,
    tmd_locations: std::collections::BTreeSet<(u32, usize)>,
}

struct TimSupplier {
    entry_idx: u32,
    offset: usize,
    fb_x: u16,
    fb_y: u16,
    width: u16,
    bpp: u32,
}

/// True when any of the next `w` 16-bit words starting at `(x, y)` in
/// the 1 MiB BGR555 LE blob are non-zero. Used by `cmd_clut_trace` to
/// decide whether the runtime captured this CLUT row.
fn row_has_data(blob: &[u8], x: usize, y: usize, w: usize) -> bool {
    const VW: usize = 1024;
    const VH: usize = 512;
    if y >= VH {
        return false;
    }
    let row_start = (y * VW + x) * 2;
    let end = ((x + w).min(VW) * 2) + y * VW * 2;
    let end = end.min(blob.len());
    if row_start >= end {
        return false;
    }
    let mut i = row_start;
    while i + 1 < end {
        if blob[i] != 0 || blob[i + 1] != 0 {
            return true;
        }
        i += 2;
    }
    false
}

/// Resolves the (scene_name, runtime_bytes, source_label) triple from
/// either explicit args or a scenario lookup. Scenario mode reads the
/// VRAM blob in-process via `legaia-mednafen`'s GPU section parser, so
/// no temp file is needed.
///
/// In scenario mode with `frames > 0`, additionally boots a
/// [`BootSession`] on the resolved scene and ticks it `frames` times
/// before returning the sampled engine VRAM. This catches dynamic
/// uploads (NPC palette swaps, fog ramps, per-frame CLUT mutations)
/// that the pure pre-pass doesn't see.
fn resolve_vram_inputs(args: &VramOracleArgs<'_>) -> Result<ResolvedVram> {
    use legaia_mednafen::ScenarioManifest;

    match (args.scenario, args.scene, args.runtime_vram) {
        (Some(label), _, _) => {
            let manifest = ScenarioManifest::from_path(args.manifest)?;
            let scn = manifest.by_label(label).with_context(|| {
                format!("scenario {label:?} not in {}", args.manifest.display())
            })?;
            let scene_name = scn.expected_active_scene.clone().with_context(|| {
                format!("scenario {label:?} has no `expected_active_scene`; cannot derive scene",)
            })?;
            let save_path = manifest.save_path(scn.slot)?;
            if !save_path.exists() {
                anyhow::bail!(
                    "scenario {label:?} slot {} save not found at {}",
                    scn.slot,
                    save_path.display()
                );
            }
            let runtime_bytes = load_runtime_vram_from_save(&save_path)?;
            let source_label = format!(
                "scenario {label:?} (slot {}, {})",
                scn.slot,
                save_path.display()
            );
            Ok(ResolvedVram {
                scene_name,
                runtime_bytes,
                source_label,
            })
        }
        (None, Some(scene_name), Some(runtime_path)) => {
            let runtime_bytes = std::fs::read(runtime_path)
                .with_context(|| format!("read runtime VRAM blob {}", runtime_path.display()))?;
            Ok(ResolvedVram {
                scene_name: scene_name.to_owned(),
                runtime_bytes,
                source_label: runtime_path.display().to_string(),
            })
        }
        _ => anyhow::bail!(
            "vram-oracle: provide either `--scenario <label>` or both `--scene` + `--runtime-vram`"
        ),
    }
}

struct ResolvedVram {
    scene_name: String,
    runtime_bytes: Vec<u8>,
    source_label: String,
}

pub(crate) fn cmd_vram_oracle(args: VramOracleArgs<'_>) -> Result<()> {
    let resolved = resolve_vram_inputs(&args)?;
    let engine_bytes = build_engine_vram_bytes_with_frames(
        &resolved.scene_name,
        args.extracted_root,
        args.disc,
        args.frames,
    )?;
    let runtime_bytes = resolved.runtime_bytes;
    if runtime_bytes.len() != engine_bytes.len() {
        anyhow::bail!(
            "runtime VRAM size {} != expected {} (1 MiB BGR555)",
            runtime_bytes.len(),
            engine_bytes.len()
        );
    }

    let report = vram_coverage_report(&engine_bytes, &runtime_bytes);
    println!(
        "scene '{}'  vs runtime {}  (frames={})",
        resolved.scene_name, resolved.source_label, args.frames
    );
    print_vram_coverage(&report);
    let diff_png = args.diff_png;
    let tiles = args.tiles;
    let rows_csv = args.rows_csv;
    let clut_regions = args.clut_regions;

    if tiles {
        println!("  per-64x64-tile coverage (runtime non-zero / engine non-zero / overlap):");
        const W: usize = 1024;
        const H: usize = 512;
        for ty in 0..(H / 64) {
            for tx in 0..(W / 64) {
                let mut rt = 0u32;
                let mut en = 0u32;
                let mut ov = 0u32;
                for dy in 0..64 {
                    let y = ty * 64 + dy;
                    for dx in 0..64 {
                        let x = tx * 64 + dx;
                        let off = (y * W + x) * 2;
                        let rw = u16::from_le_bytes([runtime_bytes[off], runtime_bytes[off + 1]]);
                        let ew = u16::from_le_bytes([engine_bytes[off], engine_bytes[off + 1]]);
                        if rw != 0 {
                            rt += 1;
                        }
                        if ew != 0 {
                            en += 1;
                        }
                        if rw != 0 && ew != 0 {
                            ov += 1;
                        }
                    }
                }
                if rt > 0 || en > 0 {
                    println!(
                        "    tile ({:>3},{:>3})  rt={:5}  en={:5}  ov={:5}",
                        tx * 64,
                        ty * 64,
                        rt,
                        en,
                        ov
                    );
                }
            }
        }
    }

    if let Some(p) = diff_png {
        write_vram_diff_png(p, &engine_bytes, &runtime_bytes)?;
        println!("[ok] wrote VRAM diff PNG to {}", p.display());
    }

    if let Some(p) = rows_csv {
        write_vram_rows_csv(p, &engine_bytes, &runtime_bytes)?;
        println!("[ok] wrote per-row VRAM CSV to {}", p.display());
    }

    if clut_regions {
        print_vram_clut_region_report(&engine_bytes, &runtime_bytes);
    }

    if args.strict {
        match first_texpage_divergence(&engine_bytes, &runtime_bytes) {
            None => {
                println!("[strict] texpage region (y >= 256): byte-exact match");
            }
            Some(TexpageDivergence {
                y,
                x,
                engine_word,
                runtime_word,
            }) => {
                anyhow::bail!(
                    "[strict] texpage region diverged at row {y} col {x}: engine=0x{engine_word:04X} runtime=0x{runtime_word:04X}",
                );
            }
        }
    }

    Ok(())
}

/// VRAM regions known to carry CLUT (colour-lookup-table) data, by Y row
/// and approximate X span. The renderer treats CLUTs as 16- or 256-entry
/// rows of u16 BGR555 anywhere in VRAM; the project's RE has surfaced
/// specific bands that scene-pack uploads target.
///
/// Each entry is `(label, y, x_start, width)`; width is in pixels (not
/// bytes), and a CLUT row is one pixel tall by definition.
const VRAM_CLUT_BANDS: &[(&str, usize, usize, usize)] = &[
    // Row-479 NPC palette band (see docs/formats/npc-palette.md +
    // project_row479_global_hue_ramp memory). Scene-pack TIMs upload
    // 16- and 32-entry CLUTs into this row at fb_x=0..256.
    ("npc-clut row 479           x=  0..256", 479, 0, 256),
    // Common low-pages-area CLUT rows used by character / scene
    // textures. Most scenes touch at least one row in 480..512.
    ("char-clut row 480           x=  0..256", 480, 0, 256),
    ("char-clut row 481           x=  0..256", 481, 0, 256),
    ("char-clut row 496           x=  0..256", 496, 0, 256),
    // Display framebuffer scan rows. These are normally rewritten
    // every frame so any "engine populated this from the static
    // upload" content is suspect.
    ("framebuffer scanline y= 16  x=  0..640", 16, 0, 640),
    ("framebuffer scanline y=128  x=  0..640", 128, 0, 640),
];

fn write_vram_rows_csv(path: &Path, engine: &[u8], runtime: &[u8]) -> Result<()> {
    const W: usize = 1024;
    const H: usize = 512;
    let mut s = String::new();
    s.push_str("y,runtime_nz,engine_nz,overlap,runtime_only,engine_only\n");
    for y in 0..H {
        let mut rt = 0u32;
        let mut en = 0u32;
        let mut ov = 0u32;
        let mut rt_only = 0u32;
        let mut en_only = 0u32;
        let row_base = y * W * 2;
        for x in 0..W {
            let off = row_base + x * 2;
            let rw = u16::from_le_bytes([runtime[off], runtime[off + 1]]);
            let ew = u16::from_le_bytes([engine[off], engine[off + 1]]);
            let rnz = rw != 0;
            let enz = ew != 0;
            if rnz {
                rt += 1;
            }
            if enz {
                en += 1;
            }
            match (rnz, enz) {
                (true, true) => ov += 1,
                (true, false) => rt_only += 1,
                (false, true) => en_only += 1,
                _ => {}
            }
        }
        s.push_str(&format!("{y},{rt},{en},{ov},{rt_only},{en_only}\n"));
    }
    std::fs::write(path, s).with_context(|| format!("write VRAM rows CSV {}", path.display()))?;
    Ok(())
}

fn print_vram_clut_region_report(engine: &[u8], runtime: &[u8]) {
    const W: usize = 1024;
    const H: usize = 512;
    println!();
    println!("VRAM CLUT-region health (engine vs runtime):");
    println!(
        "  {:<48} {:>5} {:>5} {:>5} {:>6} {:>6}",
        "band", "rt", "en", "ov", "rt-only", "en-only"
    );
    for &(label, y, x0, w) in VRAM_CLUT_BANDS {
        if y >= H {
            continue;
        }
        let row_base = y * W * 2;
        let mut rt = 0u32;
        let mut en = 0u32;
        let mut ov = 0u32;
        let mut rt_only = 0u32;
        let mut en_only = 0u32;
        let x_end = (x0 + w).min(W);
        for x in x0..x_end {
            let off = row_base + x * 2;
            let rw = u16::from_le_bytes([runtime[off], runtime[off + 1]]);
            let ew = u16::from_le_bytes([engine[off], engine[off + 1]]);
            let rnz = rw != 0;
            let enz = ew != 0;
            if rnz {
                rt += 1;
            }
            if enz {
                en += 1;
            }
            match (rnz, enz) {
                (true, true) => ov += 1,
                (true, false) => rt_only += 1,
                (false, true) => en_only += 1,
                _ => {}
            }
        }
        let pct = if rt > 0 {
            100.0 * (ov as f64) / (rt as f64)
        } else {
            0.0
        };
        let flag = if rt_only > 0 && rt > 0 {
            " <-- gap"
        } else {
            ""
        };
        println!(
            "  {label:<48} {rt:>5} {en:>5} {ov:>5} {rt_only:>6} {en_only:>6}  ({pct:5.1}%){flag}"
        );
    }
}

pub(crate) fn write_vram_png(path: &Path, bgr555_le: &[u8]) -> Result<()> {
    const W: u32 = 1024;
    const H: u32 = 512;
    let rgba = legaia_mednafen::vram_to_rgba8(bgr555_le);
    let f = std::fs::File::create(path)
        .with_context(|| format!("create VRAM PNG {}", path.display()))?;
    let bw = std::io::BufWriter::new(f);
    let mut enc = png::Encoder::new(bw, W, H);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&rgba)?;
    Ok(())
}

/// Compact per-region VRAM coverage report.
pub(crate) struct VramCoverage {
    /// Per-tile counts. Each tile is 64x64 pixels (16 tiles wide, 8 rows tall).
    runtime_nonzero_pixels: u64,
    engine_nonzero_pixels: u64,
    overlap_pixels: u64,
    runtime_only_pixels: u64,
    engine_only_pixels: u64,
    /// `(y_range_label, runtime_nonzero, engine_nonzero, overlap)` for
    /// the common VRAM regions.
    bands: Vec<(&'static str, u64, u64, u64)>,
}

pub(crate) fn vram_coverage_report(engine: &[u8], runtime: &[u8]) -> VramCoverage {
    const W: usize = 1024;
    const H: usize = 512;
    let mut runtime_nz = 0u64;
    let mut engine_nz = 0u64;
    let mut overlap = 0u64;
    let mut runtime_only = 0u64;
    let mut engine_only = 0u64;
    for i in 0..(W * H) {
        let off = i * 2;
        let rw = u16::from_le_bytes([runtime[off], runtime[off + 1]]);
        let ew = u16::from_le_bytes([engine[off], engine[off + 1]]);
        let rnz = rw != 0;
        let enz = ew != 0;
        if rnz {
            runtime_nz += 1;
        }
        if enz {
            engine_nz += 1;
        }
        match (rnz, enz) {
            (true, true) => overlap += 1,
            (true, false) => runtime_only += 1,
            (false, true) => engine_only += 1,
            _ => {}
        }
    }
    // Band reports split VRAM into top half (display + scratch) and bottom
    // half (texture pages + CLUTs), then split the bottom into upper-256
    // (typical character / scene textures) and lower-256 (extra texture
    // pages, CLUT rows).
    let band = |y0: usize, y1: usize| -> (u64, u64, u64) {
        let mut rt = 0u64;
        let mut en = 0u64;
        let mut ov = 0u64;
        for y in y0..y1 {
            for x in 0..W {
                let off = (y * W + x) * 2;
                let rw = u16::from_le_bytes([runtime[off], runtime[off + 1]]);
                let ew = u16::from_le_bytes([engine[off], engine[off + 1]]);
                let rnz = rw != 0;
                let enz = ew != 0;
                if rnz {
                    rt += 1;
                }
                if enz {
                    en += 1;
                }
                if rnz && enz {
                    ov += 1;
                }
            }
        }
        (rt, en, ov)
    };
    let mut bands = Vec::new();
    let (rt, en, ov) = band(0, 256);
    bands.push(("top half y=  0..256 (display FB + scratch)", rt, en, ov));
    let (rt, en, ov) = band(256, 384);
    bands.push(("texpage rows y=256..384 (primary textures)", rt, en, ov));
    let (rt, en, ov) = band(384, 512);
    bands.push(("texpage rows y=384..512 (textures + CLUTs)", rt, en, ov));
    VramCoverage {
        runtime_nonzero_pixels: runtime_nz,
        engine_nonzero_pixels: engine_nz,
        overlap_pixels: overlap,
        runtime_only_pixels: runtime_only,
        engine_only_pixels: engine_only,
        bands,
    }
}

pub(crate) fn print_vram_coverage(c: &VramCoverage) {
    let total_runtime = c.runtime_nonzero_pixels.max(1);
    println!("VRAM coverage (engine vs runtime, BGR555 != 0 pixel mask)");
    println!(
        "  runtime non-zero pixels:  {:>8}   (= the loaded VRAM ground truth)",
        c.runtime_nonzero_pixels
    );
    println!("  engine  non-zero pixels:  {:>8}", c.engine_nonzero_pixels);
    println!(
        "  overlap (engine ∩ rt):    {:>8}   ({:.1}% of runtime)",
        c.overlap_pixels,
        100.0 * c.overlap_pixels as f64 / total_runtime as f64
    );
    println!(
        "  runtime-only (gap):       {:>8}   ({:.1}% missing in engine)",
        c.runtime_only_pixels,
        100.0 * c.runtime_only_pixels as f64 / total_runtime as f64
    );
    println!("  engine-only (extra):      {:>8}", c.engine_only_pixels);
    println!("  per-band breakdown:");
    for (label, rt, en, ov) in &c.bands {
        let pct = if *rt > 0 {
            100.0 * (*ov as f64) / (*rt as f64)
        } else {
            0.0
        };
        println!("    {label:<48} runtime={rt:>7} engine={en:>7} overlap={ov:>7} ({pct:5.1}%)");
    }
}

pub(crate) fn write_vram_diff_png(path: &Path, engine: &[u8], runtime: &[u8]) -> Result<()> {
    const W: u32 = 1024;
    const H: u32 = 512;
    let mut rgba = Vec::with_capacity((W * H * 4) as usize);
    for i in 0..(W as usize * H as usize) {
        let off = i * 2;
        let rw = u16::from_le_bytes([runtime[off], runtime[off + 1]]);
        let ew = u16::from_le_bytes([engine[off], engine[off + 1]]);
        let rnz = rw != 0;
        let enz = ew != 0;
        let color = match (rnz, enz) {
            (false, false) => [0u8, 0, 0, 0xFF],
            // Engine matches runtime exactly (same word) → grey
            (true, true) if rw == ew => [0x60, 0x60, 0x60, 0xFF],
            // Both non-zero but different content → blue
            (true, true) => [0x30, 0x80, 0xFF, 0xFF],
            // Runtime has content engine doesn't → red (the gap)
            (true, false) => [0xFF, 0x40, 0x40, 0xFF],
            // Engine has content runtime doesn't → green (extras / wrong slot)
            (false, true) => [0x40, 0xFF, 0x40, 0xFF],
        };
        rgba.extend_from_slice(&color);
    }
    let f = std::fs::File::create(path)
        .with_context(|| format!("create diff PNG {}", path.display()))?;
    let bw = std::io::BufWriter::new(f);
    let mut enc = png::Encoder::new(bw, W, H);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&rgba)?;
    Ok(())
}
