//! GPU prim-pool tile-signature trace + per-primitive renderer dispatch-table
//! decode/survey subcommands for `mednafen-state`.

use anyhow::{Context, Result, bail};
use legaia_mednafen::{SaveState, extract::PSX_RAM_KSEG0};
use std::path::{Path, PathBuf};

/// Default RAM windows to scan for tile-signature matches. Both come from
/// the documented mc1↔mc2 diff regions that lack any known format marker
/// and are sized in the same ballpark as ~10k POLY_FT4 records.
const DEFAULT_WINDOWS: &[(u32, u32)] = &[
    (0x80190000, 0x801B6000), // 144 KB, contains 0x801913F5..0x801B5FD0
    (0x8016E000, 0x80185000), // 92 KB,  contains 0x8016E44C..0x80184BD0
];

pub fn cmd_prim_trace(
    save_path: &Path,
    pool_base: u32,
    pool_end: u32,
    extra_windows: &[(u32, u32)],
    top: usize,
    scan_all_ram: bool,
    json_out: Option<&Path>,
) -> Result<()> {
    use legaia_mednafen::{prim_pool, source_hunt};
    use serde::Serialize;

    if pool_end <= pool_base {
        bail!("pool_end <= pool_base");
    }
    let save = SaveState::from_path(save_path)?;
    let ram = save.main_ram()?;
    let pool_lo = (pool_base - PSX_RAM_KSEG0) as usize;
    let pool_hi = (pool_end - PSX_RAM_KSEG0) as usize;
    if pool_hi > ram.len() {
        bail!("pool end past end of main RAM");
    }
    let pool = &ram[pool_lo..pool_hi];

    // 1) Walk pool + topology check.
    let topology = prim_pool::chain_topology(pool, pool_base);
    let prims = prim_pool::decode(pool, pool_base);
    let mut counts: std::collections::HashMap<&'static str, usize> =
        std::collections::HashMap::new();
    for p in &prims {
        let key: &'static str = match p {
            prim_pool::Prim::PolyFt4 { .. } => "POLY_FT4",
            prim_pool::Prim::PolyGt4 { .. } => "POLY_GT4",
            prim_pool::Prim::PolyFt3 { .. } => "POLY_FT3",
            prim_pool::Prim::PolyGt3 { .. } => "POLY_GT3",
            prim_pool::Prim::Sprt16 { .. } => "SPRT_16",
            prim_pool::Prim::Sprt8 { .. } => "SPRT_8",
        };
        *counts.entry(key).or_insert(0) += 1;
    }

    println!("[prim-trace] save  {}", save_path.display());
    println!(
        "[prim-trace] pool  0x{pool_base:08X}..0x{pool_end:08X} ({} KB)",
        pool.len() / 1024
    );
    println!("[prim-trace] {} accepted prims", prims.len());
    let mut count_pairs: Vec<(&str, usize)> = counts.iter().map(|(k, v)| (*k, *v)).collect();
    count_pairs.sort_by_key(|b| std::cmp::Reverse(b.1));
    for (k, v) in &count_pairs {
        println!("    {k:<10} {v}");
    }
    println!(
        "[topology] {} tagged prims, {} chain head(s), {} terminator(s), {} linked",
        topology.total_tags,
        topology.heads.len(),
        topology.terminators,
        topology.linked
    );
    if !topology.heads.is_empty() {
        let first = topology.heads[0];
        let head_addr = pool_base + first as u32;
        println!(
            "[topology] first chain head at pool-offset 0x{first:X} (kuseg 0x{head_addr:08X})"
        );
    }

    // 2) Tile signature clustering.
    let sigs = prim_pool::tile_signatures(&prims);
    println!(
        "[tiles] {} unique POLY_FT4 (clut,tpage,uvs) clusters",
        sigs.len()
    );
    for (i, s) in sigs.iter().take(top).enumerate() {
        println!(
            "  #{:<3} clut=0x{:04X} tpage=0x{:04X} uvs={:?}  hits={}",
            i, s.clut, s.tpage, s.uvs, s.count
        );
    }

    // 3) Search the default + user windows.
    let mut all_windows: Vec<(u32, u32)> = DEFAULT_WINDOWS.to_vec();
    all_windows.extend(extra_windows);

    #[derive(Serialize)]
    struct ClusterReport {
        cluster_index: usize,
        clut: u16,
        tpage: u16,
        uvs: [(u8, u8); 4],
        count_in_pool: usize,
        windows: Vec<WindowHit>,
    }
    #[derive(Serialize)]
    struct WindowHit {
        window_start: u32,
        window_end: u32,
        match_count: usize,
        dominant_gap: Option<usize>,
        dominant_gap_share: f64,
        gap_histogram: Vec<(usize, usize)>,
        first_addrs: Vec<u32>,
    }
    #[derive(Serialize)]
    struct PooledReport {
        window_start: u32,
        window_end: u32,
        total_matches: usize,
        dominant_gap: Option<usize>,
        dominant_gap_share: f64,
        gap_histogram: Vec<(usize, usize)>,
    }
    #[derive(Serialize)]
    struct TraceReport {
        save: String,
        pool_base: u32,
        pool_end: u32,
        prim_count: usize,
        unique_signatures: usize,
        topology_heads: usize,
        topology_terminators: usize,
        top_clusters: Vec<ClusterReport>,
        pooled: Vec<PooledReport>,
    }

    let mut top_clusters = Vec::new();
    let mut pooled_reports = Vec::new();

    for &(ws, we) in &all_windows {
        let wlo = (ws - PSX_RAM_KSEG0) as usize;
        let whi = (we - PSX_RAM_KSEG0) as usize;
        if whi > ram.len() {
            eprintln!("[warn] window 0x{ws:08X}..0x{we:08X} extends past main RAM, clamping");
        }
        let win = &ram[wlo..whi.min(ram.len())];
        println!(
            "[search] window 0x{ws:08X}..0x{we:08X} ({} KB)",
            win.len() / 1024
        );
        let fp_labels = ["rich", "packet", "uv+clut", "uv+tpage", "clut+tpage"];
        // Pooled scan: union of every cluster's hits in this window, per
        // fingerprint shape.
        let mut union_offsets_by_fp: Vec<Vec<usize>> = vec![Vec::new(); fp_labels.len()];
        for (i, s) in sigs.iter().take(top).enumerate() {
            for (fp_idx, fp) in s.fingerprints.iter().enumerate() {
                let offs = source_hunt::search(win, fp);
                union_offsets_by_fp[fp_idx].extend(offs.iter().copied());
                let stride = source_hunt::stride(&offs);
                if stride.match_count == 0 {
                    continue;
                }
                let first_addrs: Vec<u32> = offs.iter().take(6).map(|o| ws + *o as u32).collect();
                let dom = stride
                    .dominant_gap
                    .map(|g| format!("stride {g}"))
                    .unwrap_or_else(|| {
                        format!("mixed (top {:.0}%)", stride.dominant_gap_share * 100.0)
                    });
                println!(
                    "    cluster #{i:<3} clut=0x{:04X} tpage=0x{:04X}  fp={:<8} matches={}  {dom}  first={:?}",
                    s.clut, s.tpage, fp_labels[fp_idx], stride.match_count, first_addrs
                );
                if (i == 0 || stride.dominant_gap.is_some())
                    && (fp_idx <= 1 || stride.match_count >= 10)
                {
                    let wh = WindowHit {
                        window_start: ws,
                        window_end: we,
                        match_count: stride.match_count,
                        dominant_gap: stride.dominant_gap,
                        dominant_gap_share: stride.dominant_gap_share,
                        gap_histogram: stride.gap_histogram.clone(),
                        first_addrs,
                    };
                    if let Some(existing) = top_clusters
                        .iter_mut()
                        .find(|c: &&mut ClusterReport| c.cluster_index == i)
                    {
                        existing.windows.push(wh);
                    } else {
                        top_clusters.push(ClusterReport {
                            cluster_index: i,
                            clut: s.clut,
                            tpage: s.tpage,
                            uvs: s.uvs,
                            count_in_pool: s.count,
                            windows: vec![wh],
                        });
                    }
                }
            }
        }
        // For pooled stride we use the richest fingerprint shape that had any hits.
        let union_offsets = union_offsets_by_fp
            .iter()
            .find(|v| !v.is_empty())
            .cloned()
            .unwrap_or_default();
        for (fp_idx, label) in fp_labels.iter().enumerate() {
            let n = union_offsets_by_fp[fp_idx].len();
            if n > 0 {
                println!("    pooled  fp={label:<8} union_matches={n}");
            }
        }
        let pooled = source_hunt::pooled_stride(std::slice::from_ref(&union_offsets));
        println!(
            "    pooled  matches={}  dominant_gap={:?}  share={:.0}%",
            pooled.match_count,
            pooled.dominant_gap,
            pooled.dominant_gap_share * 100.0
        );
        if !pooled.gap_histogram.is_empty() {
            let topgaps: Vec<String> = pooled
                .gap_histogram
                .iter()
                .take(4)
                .map(|(g, c)| format!("{g}({c})"))
                .collect();
            println!("    pooled  top gaps: {}", topgaps.join(" "));
        }
        pooled_reports.push(PooledReport {
            window_start: ws,
            window_end: we,
            total_matches: pooled.match_count,
            dominant_gap: pooled.dominant_gap,
            dominant_gap_share: pooled.dominant_gap_share,
            gap_histogram: pooled.gap_histogram,
        });
        // Per-window stride autocorrelation. The dominant record stride
        // of a structured region jumps out as a score significantly
        // above the ~1/256 noise floor.
        let strides = [4, 8, 12, 16, 20, 24, 28, 32, 40, 48, 56, 64];
        let auto = source_hunt::autocorr_strides(win, &strides);
        let top: Vec<String> = auto
            .iter()
            .take(4)
            .map(|s| format!("{}:{:.3}", s.stride, s.score))
            .collect();
        println!("    autocorr top: {}", top.join(" "));
    }

    if scan_all_ram {
        println!("[scan-all-ram] searching full 2 MiB main RAM for top-{top} cluster fingerprints");
        let fp_labels = ["rich", "packet", "uv+clut", "uv+tpage", "clut+tpage"];
        // Hide pool window matches - those are self-matches inside the
        // live prim packets, not source data.
        let pool_lo = (pool_base - PSX_RAM_KSEG0) as usize;
        let pool_hi = (pool_end - PSX_RAM_KSEG0) as usize;
        for (i, s) in sigs.iter().take(top).enumerate() {
            for (fp_idx, fp) in s.fingerprints.iter().enumerate() {
                if fp.len() < 4 {
                    continue; // skip too-noisy short fingerprints
                }
                let all_offs = source_hunt::search(ram, fp);
                let non_pool_offs: Vec<usize> = all_offs
                    .into_iter()
                    .filter(|o| !(pool_lo..pool_hi).contains(o))
                    .collect();
                if non_pool_offs.is_empty() {
                    continue;
                }
                let first: Vec<u32> = non_pool_offs
                    .iter()
                    .take(6)
                    .map(|o| PSX_RAM_KSEG0 + *o as u32)
                    .collect();
                let stride = source_hunt::stride(&non_pool_offs);
                let dom = stride
                    .dominant_gap
                    .map(|g| format!("stride {g}"))
                    .unwrap_or_else(|| {
                        format!("mixed (top {:.0}%)", stride.dominant_gap_share * 100.0)
                    });
                println!(
                    "  cluster #{i:<3} clut=0x{:04X} tpage=0x{:04X} fp={:<10} matches={} (excl pool) {dom}  first={:?}",
                    s.clut,
                    s.tpage,
                    fp_labels[fp_idx],
                    non_pool_offs.len(),
                    first
                );
            }
        }
    }

    if let Some(path) = json_out {
        let report = TraceReport {
            save: save_path.display().to_string(),
            pool_base,
            pool_end,
            prim_count: prims.len(),
            unique_signatures: sigs.len(),
            topology_heads: topology.heads.len(),
            topology_terminators: topology.terminators,
            top_clusters,
            pooled: pooled_reports,
        };
        let json = serde_json::to_string_pretty(&report)?;
        std::fs::write(path, json).with_context(|| format!("writing {}", path.display()))?;
        println!("[json] wrote {}", path.display());
    }
    Ok(())
}

pub fn cmd_prim_dispatch_table(save: &Path, overlay_targets_only: bool) -> Result<()> {
    use legaia_mednafen::prim_dispatch::{
        HIGH_MODE_END, LOW_MODE_END, LOW_MODE_START, SLOT_BYTES, SlotKind, classify, decode_both,
    };

    let s = SaveState::from_path(save)?;
    let ram = s.main_ram()?;
    let (scus_table, overlay_table) = decode_both(ram)?;

    if overlay_targets_only {
        for tgt in overlay_table.high_mode_targets() {
            println!("0x{tgt:08X}");
        }
        return Ok(());
    }

    println!("[info] {}", save.display());
    println!(
        "[info] SCUS table @ 0x{:08X}  ({} alpha rows × {} slots)",
        scus_table.base,
        scus_table.rows.len(),
        scus_table.rows[0].slots.len()
    );
    let overlay_status = if overlay_table.is_empty() {
        "empty - world-map overlay not paged in"
    } else if overlay_table.looks_like_dispatch_table() {
        "populated (world-map overlay loaded)"
    } else {
        "leftover overlay code, NOT a dispatch table"
    };
    println!(
        "[info] overlay table @ 0x{:08X}  ({} alpha row(s); {})",
        overlay_table.base,
        overlay_table.rows.len(),
        overlay_status,
    );
    println!();

    let print_table = |label: &str, t: &legaia_mednafen::prim_dispatch::DispatchTable| {
        println!("=== {label} (base 0x{:08X}) ===", t.base);
        for (row_idx, row) in t.rows.iter().enumerate() {
            println!("  alpha row #{row_idx}  (+0x{:02X})", row.alpha_offset);
            for slot_idx in LOW_MODE_START..HIGH_MODE_END {
                let val = row.slots[slot_idx];
                let kind = classify(val);
                let kind_s = match kind {
                    SlotKind::Zero => "zero",
                    SlotKind::Scus => "SCUS",
                    SlotKind::Overlay => "OVERLAY",
                    SlotKind::Other => "OTHER",
                };
                let band = if slot_idx < LOW_MODE_END {
                    "low "
                } else if slot_idx < HIGH_MODE_END {
                    "high"
                } else {
                    "?"
                };
                let slot_addr = t.base + row.alpha_offset + slot_idx as u32 * SLOT_BYTES;
                println!(
                    "    [{band}] slot {slot_idx:>2}  @ 0x{slot_addr:08X}  ->  \
                     0x{val:08X}  {kind_s}"
                );
            }
        }
    };

    print_table("SCUS-resident dispatch table", &scus_table);
    println!();
    print_table("Overlay-resident dispatch table", &overlay_table);

    if overlay_table.looks_like_dispatch_table() {
        let scus_high = scus_table.high_mode_targets();
        let overlay_high = overlay_table.high_mode_targets();
        println!();
        println!(
            "=== high-mode targets (the per-prim emit leaves) ===\n\
             SCUS    : {} unique\n\
             overlay : {} unique\n\
             swap-in : the overlay-resident high-mode renderers are the\n\
                       bulk-continent emit leaves the world-map top-view\n\
                       routes its TMD prims through.",
            scus_high.len(),
            overlay_high.len()
        );
        for tgt in &overlay_high {
            let in_scus = scus_high.contains(tgt);
            let mark = if in_scus {
                "(shared with SCUS)"
            } else {
                "(overlay-only)"
            };
            println!("  0x{tgt:08X}  {mark}");
        }
        // Quick sanity check: any overlay-table slot whose pointer
        // lands outside the documented overlay window indicates the
        // world-map overlay actually extends past 0x801F9000 - flag it.
        let stragglers: Vec<u32> = overlay_high
            .iter()
            .copied()
            .filter(|p| classify(*p) == SlotKind::Other)
            .collect();
        if !stragglers.is_empty() {
            println!(
                "\nWARNING: {} overlay-table target(s) classified as OTHER \
                 (outside known overlay window); re-extract with a wider \
                 window:\n  {:?}",
                stragglers.len(),
                stragglers
                    .iter()
                    .map(|p| format!("0x{p:08X}"))
                    .collect::<Vec<_>>(),
            );
        }
    }
    Ok(())
}

pub fn cmd_prim_dispatch_survey(saves: &[PathBuf]) -> Result<()> {
    use legaia_mednafen::prim_dispatch::{
        HIGH_MODE_END, HIGH_MODE_START, SCUS_ALPHA_ROWS, SCUS_TABLE_BASE, SLOT_BYTES, classify,
        decode, decode_both,
    };

    if saves.len() < 2 {
        anyhow::bail!("prim-dispatch-survey requires at least 2 save states");
    }

    println!("[info] surveying {} save state(s)", saves.len());

    let mut entries: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    for path in saves {
        let s = SaveState::from_path(path)?;
        let ram = s.main_ram()?;
        entries.push((path.clone(), ram.to_vec()));
    }

    // SCUS invariant. Use the first save as anchor; compare the
    // populated slot range (12..20) on every alpha row to every other
    // save.
    let (anchor_path, anchor_ram) = &entries[0];
    let anchor = decode(anchor_ram, SCUS_TABLE_BASE, SCUS_ALPHA_ROWS)?;
    let mut drift_count = 0;
    for (path, ram) in &entries[1..] {
        let here = decode(ram, SCUS_TABLE_BASE, SCUS_ALPHA_ROWS)?;
        for (row_idx, (ra, rh)) in anchor.rows.iter().zip(here.rows.iter()).enumerate() {
            for slot_idx in HIGH_MODE_START..HIGH_MODE_END {
                if ra.slots[slot_idx] != rh.slots[slot_idx] {
                    drift_count += 1;
                    if drift_count <= 8 {
                        println!(
                            "WARN: SCUS table drift {}:row{row_idx}:slot{slot_idx} \
                             vs {}: 0x{:08X} != 0x{:08X}",
                            path.display(),
                            anchor_path.display(),
                            rh.slots[slot_idx],
                            ra.slots[slot_idx]
                        );
                    }
                }
            }
        }
    }
    if drift_count == 0 {
        println!(
            "[ok]   SCUS dispatch table @ 0x{:08X} is byte-identical across all \
             surveyed saves (high-mode slots {}..{}).",
            SCUS_TABLE_BASE,
            HIGH_MODE_START,
            HIGH_MODE_END - 1
        );
    } else {
        println!(
            "ERROR: SCUS dispatch table drifted in {drift_count} slot(s) - the SCUS \
             code region should be immutable. Re-extract or re-import the saves."
        );
    }

    println!();
    println!(
        "{:<48}  {:>6}  {:<40}  high-mode targets",
        "save", "status", "summary"
    );
    println!("{}", "-".repeat(140));
    for (path, ram) in &entries {
        let (_scus, overlay) = decode_both(ram)?;
        let (status, summary) = if overlay.is_empty() {
            ("empty", "world-map overlay NOT paged in".to_string())
        } else if overlay.looks_like_dispatch_table() {
            (
                "POP",
                format!(
                    "world-map overlay loaded ({} high-mode targets)",
                    overlay.high_mode_targets().len()
                ),
            )
        } else {
            (
                "stale",
                "leftover overlay code, not a dispatch table".to_string(),
            )
        };
        let targets = overlay
            .high_mode_targets()
            .iter()
            .map(|t| {
                use legaia_mednafen::prim_dispatch::SlotKind;
                let mark = match classify(*t) {
                    SlotKind::Overlay => "",
                    SlotKind::Scus => "(SCUS!)",
                    SlotKind::Zero => "(zero!)",
                    SlotKind::Other => "(OTHER!)",
                };
                format!("0x{t:08X}{mark}")
            })
            .collect::<Vec<_>>()
            .join(" ");
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let truncated = if name.len() > 48 {
            format!("…{}", &name[name.len() - 47..])
        } else {
            name
        };
        println!("{truncated:<48}  {status:>6}  {summary:<40}  {targets}");
    }
    let row_bytes = SLOT_BYTES * legaia_mednafen::prim_dispatch::SLOTS_PER_ROW as u32;
    println!(
        "\n[info] row stride = 0x{row_bytes:X} bytes; high-mode slots = {}..{}",
        HIGH_MODE_START,
        HIGH_MODE_END - 1
    );
    if drift_count > 0 {
        anyhow::bail!("SCUS dispatch table drift detected; see warnings above");
    }
    Ok(())
}
