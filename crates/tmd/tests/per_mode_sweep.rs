//! Disc-gated regression test: run the Legaia primitive iterator + per-mode
//! descriptor table against every `.tmd` file under `extracted/tmd_scan/`
//! and assert 100% clean walk.
//!
//! Skips silently when `extracted/tmd_scan/` is missing or
//! `LEGAIA_DISC_BIN` is unset - same skip-pattern as the rest of the
//! disc-gated suite.
//!
//! What this catches:
//!  - The 6-entry mode descriptor table at `DAT_8007326c` covers every
//!    `flags` byte that real meshes emit.
//!  - Every TMD's primitive section walks cleanly (no count mismatch,
//!    no vertex-idx out-of-range, no group-walk truncation).
//!  - The histogram of mode bytes used across the corpus is reported
//!    so regressions in renderer coverage surface as test logs.
//!
//! Per the project memory's TMD walker note this should pass on all
//! 16830 TMDs without exception.

use legaia_tmd::legaia_prims::MODULATION_NEUTRAL;
use legaia_tmd::{legaia_prims, parse};
use std::path::{Path, PathBuf};

fn extracted_tmd_scan() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("extracted/tmd_scan"),
        PathBuf::from("../../extracted/tmd_scan"),
    ];
    candidates.into_iter().find(|p| p.is_dir())
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("tmd") {
            out.push(p);
        }
    }
}

#[test]
fn per_mode_descriptor_sweep_validates_every_tmd() {
    let Some(root) = extracted_tmd_scan() else {
        eprintln!("[skip] extracted/tmd_scan/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut paths = Vec::new();
    walk(&root, &mut paths);
    paths.sort();
    assert!(!paths.is_empty(), "no .tmd files under {}", root.display());

    let mut files_total = 0usize;
    let mut files_ok = 0usize;
    let mut prims_total = 0usize;
    let mut bad_vertex_idx = 0usize;
    let mut count_mismatch = 0usize;
    let mut iter_fail = 0usize;
    let mut parse_fail = 0usize;
    // Colour-decode invariants: EVERY prim exposes one colour per vertex -
    // that colour is the field's lighting signal (the PSX GPU modulates a
    // textured prim's texel by it, `texel * colour / 128`, and fills an
    // untextured prim with it outright). Untextured prims additionally expose
    // no UVs. Counters keep it non-vacuous.
    let mut color_invariant_fail = 0usize;
    let mut untextured_prims_with_colors = 0usize;
    let mut modes_seen: std::collections::BTreeMap<u8, usize> = std::collections::BTreeMap::new();
    // Track flag values that vertex_offset_bytes returns None for -
    // those would be missing entries in the 6-entry descriptor table.
    let mut unknown_flag_values: std::collections::BTreeSet<u16> =
        std::collections::BTreeSet::new();

    for p in &paths {
        files_total += 1;
        let Ok(raw) = std::fs::read(p) else {
            parse_fail += 1;
            continue;
        };
        let Ok(tmd) = parse(&raw) else {
            parse_fail += 1;
            continue;
        };

        let mut file_ok = true;
        for o in &tmd.objects {
            let groups = match legaia_prims::iter_groups(
                &raw,
                o.primitives_byte_offset,
                o.primitives_byte_size,
            ) {
                Ok(g) => g,
                Err(_) => {
                    iter_fail += 1;
                    file_ok = false;
                    break;
                }
            };
            let stats = legaia_prims::group_stats(o.primitives_byte_offset, &groups);
            prims_total += stats.total_prims;
            if stats.total_prims != o.claimed_n_primitive as usize {
                count_mismatch += 1;
                file_ok = false;
            }
            for g in &groups {
                *modes_seen.entry(g.header.mode).or_default() += g.header.count as usize;
                if legaia_prims::vertex_offset_bytes(g.header.flags).is_none() {
                    unknown_flag_values.insert(g.header.flags);
                }
                let desc = legaia_tmd::descriptor::Descriptor::for_flags(g.header.flags);
                let is_textured = desc.is_some_and(|d| d.packet_shape.is_textured());
                let texblock_off = desc.and_then(|d| d.texture_block_offset);
                let n_verts = g.header.n_vertices();
                for prim in &g.prims {
                    let idxs = prim.vertex_indices();
                    if !idxs.is_empty() && idxs.iter().any(|&v| (v as usize) >= o.vertices.len()) {
                        bad_vertex_idx += 1;
                        file_ok = false;
                        break;
                    }
                    // Colour decode: one colour per vertex, always. A textured
                    // prim on a baked-colour row reads its own colour block; one
                    // on a light-source-lit row (which has none) reads the
                    // neutral 0x80, so it draws at the raw texel. An untextured
                    // prim carries colours and no UVs.
                    if prim.colors.len() != n_verts {
                        color_invariant_fail += 1;
                        file_ok = false;
                    }
                    if is_textured {
                        // A lit prim's texture block sits at offset 0; reading a
                        // colour there would hand the renderer `[u0, v0, cba_lo]`
                        // as an RGB. Neutral is the only correct answer.
                        if texblock_off == Some(0)
                            && prim.colors.iter().any(|&c| c != [MODULATION_NEUTRAL; 3])
                        {
                            color_invariant_fail += 1;
                            file_ok = false;
                        }
                    } else {
                        if !prim.uvs.is_empty() {
                            color_invariant_fail += 1;
                            file_ok = false;
                        }
                        if prim.colors.iter().any(|&c| c != [0, 0, 0]) {
                            untextured_prims_with_colors += 1;
                        }
                    }
                }
            }
        }
        if file_ok {
            files_ok += 1;
        }
    }

    eprintln!("[per-mode] {files_total} TMDs, {files_ok} clean, {prims_total} prims walked");
    eprintln!("[per-mode] mode histogram (top 10):");
    let mut sorted: Vec<_> = modes_seen.iter().collect();
    sorted.sort_by_key(|&(_, c)| std::cmp::Reverse(*c));
    for (mode, c) in sorted.iter().take(10) {
        eprintln!("    mode=0x{:02X}  prims={c}", **mode);
    }

    assert_eq!(parse_fail, 0, "{parse_fail} files failed to parse");
    assert_eq!(iter_fail, 0, "{iter_fail} files failed primitive iteration");
    assert_eq!(
        count_mismatch, 0,
        "{count_mismatch} files have walked-count != claimed-count"
    );
    assert_eq!(
        bad_vertex_idx, 0,
        "{bad_vertex_idx} prims reference out-of-range vertex indices"
    );
    eprintln!("[per-mode] untextured prims with non-black colours: {untextured_prims_with_colors}");
    assert_eq!(
        color_invariant_fail, 0,
        "{color_invariant_fail} prims broke the colour-decode invariant \
         (textured carries colours, or untextured missing colours / carrying UVs)"
    );
    assert!(
        untextured_prims_with_colors > 0,
        "no untextured prim yielded a non-black colour - colour decode is vacuous"
    );
    assert!(
        unknown_flag_values.is_empty(),
        "vertex_offset_bytes returned None for flags {:?} - descriptor table missing entries",
        unknown_flag_values
    );
    assert_eq!(
        files_total,
        files_ok,
        "{} of {} TMDs failed validation",
        files_total - files_ok,
        files_total
    );
}
