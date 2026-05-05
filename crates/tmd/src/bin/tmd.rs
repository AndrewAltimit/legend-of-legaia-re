use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use legaia_tmd::{legaia_prim_probe, legaia_prims, parse};

#[derive(Parser)]
#[command(name = "tmd", about = "PSX TMD parser")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Parse a TMD file and print structural summary.
    Info { input: PathBuf },
    /// Parse all `.tmd` files under a directory and print one-line summaries.
    /// Reports any that fail to parse.
    ScanDir {
        dir: PathBuf,
        /// Print only files that fail to parse.
        #[arg(long, default_value_t = false)]
        only_failures: bool,
    },
    /// Dump vertices and faces of every object as a Wavefront OBJ file.
    /// Faces are decoded via the Legaia primitive iterator; pass --no-faces
    /// to emit only vertices (for sanity-checking vertex parsing).
    DumpObj {
        input: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
        #[arg(long, default_value_t = false)]
        no_faces: bool,
    },
    /// Probe the primitive section of a TMD by trying PsyQ standard sizes
    /// per mode byte. Reports per-object whether the walk consumes the
    /// section cleanly. Diagnostic only; Legaia uses a custom layout.
    Probe { input: PathBuf },
    /// Iterate the primitive section using the Legaia-specific 8-byte
    /// group header layout (decoded from FUN_8002735c). Reports per-group
    /// header + per-prim vertex indices.
    Prims {
        input: PathBuf,
        /// Limit per-group prim listing (default: print all).
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Validate the Legaia primitive iterator across every `.tmd` file
    /// under a directory. Reports per-file deltas (claimed vs walked prim
    /// count, bytes consumed vs section size, vertex-index range failures)
    /// and aggregate stats. Useful as ground truth for the iterator.
    ValidatePrims {
        dir: PathBuf,
        /// Print one line per file even when it validates cleanly (default
        /// is failures-only).
        #[arg(long, default_value_t = false)]
        verbose: bool,
        /// Treat group-walks that fall short by ≤ this many bytes as still
        /// clean (Legaia tends to leave one-prim-stride trailing footers).
        #[arg(long, default_value_t = 64)]
        slack_bytes: usize,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Info { input } => info(&input),
        Cmd::ScanDir { dir, only_failures } => scan_dir(&dir, only_failures),
        Cmd::DumpObj {
            input,
            out,
            no_faces,
        } => dump_obj(&input, &out, no_faces),
        Cmd::Probe { input } => probe(&input),
        Cmd::Prims { input, limit } => prims(&input, limit),
        Cmd::ValidatePrims {
            dir,
            verbose,
            slack_bytes,
        } => validate_prims(&dir, verbose, slack_bytes),
    }
}

#[derive(Default)]
struct CorpusStats {
    files_total: usize,
    files_ok: usize,
    files_fail_parse: usize,
    files_fail_iter: usize,
    files_count_mismatch: usize,
    files_bytes_short: usize,
    files_bad_vertex: usize,
    objects_total: usize,
    groups_total: usize,
    prims_total: usize,
    triangles: usize,
    quads: usize,
    section_bytes_total: usize,
    consumed_bytes_total: usize,
    flags_seen: std::collections::BTreeMap<u16, usize>,
    modes_seen: std::collections::BTreeMap<u8, usize>,
}

fn validate_prims(dir: &PathBuf, verbose: bool, slack_bytes: usize) -> Result<()> {
    let mut paths = Vec::new();
    walk(dir, &mut paths)?;
    paths.sort();

    let mut s = CorpusStats::default();

    for p in &paths {
        if p.extension().and_then(|s| s.to_str()) != Some("tmd") {
            continue;
        }
        s.files_total += 1;
        let raw = match std::fs::read(p) {
            Ok(r) => r,
            Err(_) => {
                s.files_fail_parse += 1;
                continue;
            }
        };
        let tmd = match parse(&raw) {
            Ok(t) => t,
            Err(e) => {
                s.files_fail_parse += 1;
                println!(
                    "PARSE-FAIL  {}: {}",
                    p.strip_prefix(dir).unwrap_or(p).display(),
                    e
                );
                continue;
            }
        };

        let mut file_ok = true;
        let mut count_mismatch = false;
        let mut bytes_short = false;
        let mut bad_vertex = false;
        let mut walked_prims_total: usize = 0;
        let mut walked_bytes_total: usize = 0;
        let mut claimed_prims_total: usize = 0;

        for (i, o) in tmd.objects.iter().enumerate() {
            s.objects_total += 1;
            claimed_prims_total += o.claimed_n_primitive as usize;
            let groups = match legaia_prims::iter_groups(
                &raw,
                o.primitives_byte_offset,
                o.primitives_byte_size,
            ) {
                Ok(g) => g,
                Err(e) => {
                    file_ok = false;
                    s.files_fail_iter += 1;
                    println!(
                        "ITER-FAIL   {} [obj {}]: {}",
                        p.strip_prefix(dir).unwrap_or(p).display(),
                        i,
                        e
                    );
                    break;
                }
            };
            let stats = legaia_prims::group_stats(o.primitives_byte_offset, &groups);
            s.groups_total += stats.group_count;
            s.prims_total += stats.total_prims;
            s.triangles += stats.triangles;
            s.quads += stats.quads;
            s.section_bytes_total += o.primitives_byte_size;
            s.consumed_bytes_total += stats.bytes_consumed;
            walked_prims_total += stats.total_prims;
            walked_bytes_total += stats.bytes_consumed;

            for g in &groups {
                *s.flags_seen.entry(g.header.flags).or_default() += g.header.count as usize;
                *s.modes_seen.entry(g.header.mode).or_default() += g.header.count as usize;
                for prim in &g.prims {
                    let idxs = prim.vertex_indices();
                    if !idxs.is_empty() && idxs.iter().any(|&v| (v as usize) >= o.vertices.len()) {
                        bad_vertex = true;
                    }
                }
            }

            if stats.total_prims != o.claimed_n_primitive as usize {
                count_mismatch = true;
            }
            if o.primitives_byte_size > stats.bytes_consumed
                && (o.primitives_byte_size - stats.bytes_consumed) > slack_bytes
            {
                bytes_short = true;
            }
        }

        if !file_ok {
            continue;
        }
        if count_mismatch {
            s.files_count_mismatch += 1;
        }
        if bytes_short {
            s.files_bytes_short += 1;
        }
        if bad_vertex {
            s.files_bad_vertex += 1;
        }
        let clean = !count_mismatch && !bytes_short && !bad_vertex;
        if clean {
            s.files_ok += 1;
        }
        if !clean || verbose {
            let tag = if clean { "OK" } else { "DELTA" };
            println!(
                "{:<6} {} (nobj={}) claimed={} walked={} bytes={}/{} count_diff={} bytes_short={} bad_vertex={}",
                tag,
                p.strip_prefix(dir).unwrap_or(p).display(),
                tmd.objects.len(),
                claimed_prims_total,
                walked_prims_total,
                walked_bytes_total,
                tmd.objects
                    .iter()
                    .map(|o| o.primitives_byte_size)
                    .sum::<usize>(),
                count_mismatch,
                bytes_short,
                bad_vertex,
            );
        }
    }

    println!();
    println!("=== corpus summary ===");
    println!(
        "files: {} total, {} clean, {} parse-fail, {} iter-fail",
        s.files_total, s.files_ok, s.files_fail_parse, s.files_fail_iter
    );
    println!(
        "       {} count-mismatch, {} bytes-short(>{}b), {} bad-vertex-idx",
        s.files_count_mismatch, s.files_bytes_short, slack_bytes, s.files_bad_vertex
    );
    println!(
        "objects: {}  groups: {}  prims: {} (tri {} / quad {})",
        s.objects_total, s.groups_total, s.prims_total, s.triangles, s.quads
    );
    println!(
        "bytes consumed: {} / {}  ({:.2}%)",
        s.consumed_bytes_total,
        s.section_bytes_total,
        100.0 * s.consumed_bytes_total as f64 / s.section_bytes_total.max(1) as f64
    );
    println!();
    println!("flags histogram (top 20):");
    let mut flags: Vec<(u16, usize)> = s.flags_seen.into_iter().collect();
    flags.sort_by_key(|b| std::cmp::Reverse(b.1));
    for (f, n) in flags.iter().take(20) {
        let off = legaia_prims::vertex_offset_bytes(*f);
        println!(
            "  flags=0x{:04X}  prims={:>6}  vertex_offset={}",
            f,
            n,
            off.map(|o| format!("{}b", o))
                .unwrap_or_else(|| "??".to_string())
        );
    }
    println!();
    println!("mode histogram (top 20):");
    let mut modes: Vec<(u8, usize)> = s.modes_seen.into_iter().collect();
    modes.sort_by_key(|b| std::cmp::Reverse(b.1));
    for (m, n) in modes.iter().take(20) {
        println!("  mode=0x{:02X}  prims={:>6}", m, n);
    }
    Ok(())
}

fn prims(input: &PathBuf, limit: Option<usize>) -> Result<()> {
    let raw = std::fs::read(input)?;
    let tmd = parse(&raw)?;
    println!("file={}  nobj={}", input.display(), tmd.objects.len());
    for (i, o) in tmd.objects.iter().enumerate() {
        let groups =
            match legaia_prims::iter_groups(&raw, o.primitives_byte_offset, o.primitives_byte_size)
            {
                Ok(g) => g,
                Err(e) => {
                    println!("  [{:>3}] iter FAIL: {}", i, e);
                    continue;
                }
            };
        let stats = legaia_prims::group_stats(o.primitives_byte_offset, &groups);
        println!(
            "  [{:>3}] groups={} prims={} (tri={} quad={}) consumed={}b / {}b  claimed={}",
            i,
            stats.group_count,
            stats.total_prims,
            stats.triangles,
            stats.quads,
            stats.bytes_consumed,
            o.primitives_byte_size,
            o.claimed_n_primitive,
        );
        for (gi, g) in groups.iter().enumerate() {
            println!(
                "        group[{}] count={} flags=0x{:04X} olen={} ilen={} flag=0x{:02X} mode=0x{:02X} stride={}b",
                gi,
                g.header.count,
                g.header.flags,
                g.header.olen,
                g.header.ilen,
                g.header.flag,
                g.header.mode,
                g.header.prim_stride()
            );
            let n = g.prims.len();
            let take = limit.unwrap_or(n);
            for (pi, p) in g.prims.iter().take(take).enumerate() {
                let idxs: Vec<String> = p.vertex_indices().iter().map(|i| i.to_string()).collect();
                println!("          prim[{}] verts=[{}]", pi, idxs.join(", "));
            }
            if take < n {
                println!("          ... ({} more prims)", n - take);
            }
        }
    }
    Ok(())
}

fn probe(input: &PathBuf) -> Result<()> {
    let raw = std::fs::read(input)?;
    let tmd = parse(&raw)?;
    println!("file={}", input.display());
    for (i, o) in tmd.objects.iter().enumerate() {
        let section =
            &raw[o.primitives_byte_offset..o.primitives_byte_offset + o.primitives_byte_size];
        let result =
            legaia_prim_probe::walk_psx_stored_sizes(section, o.claimed_n_primitive as usize);
        match result {
            Ok(n) => println!(
                "  [{:>3}] PSX-walk OK: {} prims in {}b",
                i, n, o.primitives_byte_size
            ),
            Err(e) => println!(
                "  [{:>3}] PSX-walk FAIL ({} claimed, {}b): {}",
                i, o.claimed_n_primitive, o.primitives_byte_size, e
            ),
        }
    }
    Ok(())
}

fn info(input: &PathBuf) -> Result<()> {
    let raw = std::fs::read(input)?;
    let tmd = parse(&raw)?;
    let stats = tmd.stats();
    println!(
        "id=0x{:08X} flist_bit={} flags=0x{:08X} nobj={}",
        tmd.header.id, tmd.header.flist_bit_set, tmd.header.flags, tmd.header.nobj
    );
    println!(
        "totals: verts={} normals={} prims={} consumed={}b / {}b",
        stats.total_vertices,
        stats.total_normals,
        stats.total_primitives,
        stats.total_bytes_consumed,
        raw.len()
    );
    for (i, o) in tmd.objects.iter().enumerate() {
        println!(
            "  [{:>3}] vert={:>4}@0x{:04X}  norm={:>4}@0x{:04X}  prim={:>4}@0x{:04X}  prim_section={}b  scale=0x{:08X}",
            i,
            o.header.n_vert,
            o.header.vert_top,
            o.header.n_normal,
            o.header.normal_top,
            o.header.n_primitive,
            o.header.prim_top,
            o.primitives_byte_size,
            o.header.scale as u32,
        );
        if let Some(walk) = &o.primitives_psx_walk {
            match walk {
                Ok(prims) => {
                    let mut modes = std::collections::BTreeMap::<u8, usize>::new();
                    for p in prims {
                        *modes.entry(p.mode).or_default() += 1;
                    }
                    let s: Vec<String> = modes
                        .iter()
                        .map(|(m, n)| format!("0x{:02X}*{}", m, n))
                        .collect();
                    println!(
                        "        psx-walk: ok ({} prims), modes: {}",
                        prims.len(),
                        s.join(", ")
                    );
                }
                Err(e) => {
                    println!("        psx-walk: FAIL ({})", e);
                }
            }
        }
    }
    Ok(())
}

fn scan_dir(dir: &PathBuf, only_failures: bool) -> Result<()> {
    let mut paths = Vec::new();
    walk(dir, &mut paths)?;
    paths.sort();
    let mut ok = 0usize;
    let mut fail = 0usize;
    for p in &paths {
        if p.extension().and_then(|s| s.to_str()) != Some("tmd") {
            continue;
        }
        let raw = match std::fs::read(p) {
            Ok(r) => r,
            Err(_) => continue,
        };
        match parse(&raw) {
            Ok(tmd) => {
                ok += 1;
                if !only_failures {
                    let s = tmd.stats();
                    println!(
                        "OK   {}  nobj={} verts={} prims={} consumed={}/{}b",
                        p.strip_prefix(dir).unwrap_or(p).display(),
                        tmd.header.nobj,
                        s.total_vertices,
                        s.total_primitives,
                        s.total_bytes_consumed,
                        raw.len()
                    );
                }
            }
            Err(e) => {
                fail += 1;
                println!(
                    "FAIL {}  ({}b): {}",
                    p.strip_prefix(dir).unwrap_or(p).display(),
                    raw.len(),
                    e
                );
            }
        }
    }
    eprintln!("scan-dir: {} ok, {} fail", ok, fail);
    Ok(())
}

fn walk(dir: &PathBuf, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            walk(&p, out)?;
        } else {
            out.push(p);
        }
    }
    Ok(())
}

fn dump_obj(input: &PathBuf, out: &PathBuf, no_faces: bool) -> Result<()> {
    let raw = std::fs::read(input)?;
    let tmd = parse(&raw)?;
    let mut s = String::new();
    s.push_str(&format!(
        "# Generated from {}\n# nobj={}\n",
        input.display(),
        tmd.header.nobj
    ));
    // OBJ vertex indices are 1-based and span the whole file. Track the
    // running offset so per-object face lines reference the correct verts.
    let mut vert_base = 0usize;
    let mut total_faces = 0usize;
    for (i, o) in tmd.objects.iter().enumerate() {
        s.push_str(&format!(
            "o object_{:03}_v{}_n{}_p{}\n",
            i,
            o.vertices.len(),
            o.normals.len(),
            o.claimed_n_primitive
        ));
        // PSX uses i16 vertices with a per-object signed log2 scale.
        // For sanity checking, emit raw integer coordinates.
        for v in &o.vertices {
            s.push_str(&format!("v {} {} {}\n", v.x, v.y, v.z));
        }
        if !no_faces {
            match legaia_prims::iter_groups(&raw, o.primitives_byte_offset, o.primitives_byte_size)
            {
                Ok(groups) => {
                    for g in &groups {
                        for p in &g.prims {
                            let idxs = p.vertex_indices();
                            // Skip prims whose layout we couldn't decode.
                            if idxs.is_empty() {
                                continue;
                            }
                            // Range-check against the object's vertex array
                            // before emitting; out-of-range indices would
                            // produce a corrupt OBJ.
                            if idxs.iter().any(|&v| (v as usize) >= o.vertices.len()) {
                                continue;
                            }
                            let face: Vec<String> = idxs
                                .iter()
                                .map(|&v| (v as usize + 1 + vert_base).to_string())
                                .collect();
                            s.push_str(&format!("f {}\n", face.join(" ")));
                            total_faces += 1;
                        }
                    }
                }
                Err(e) => {
                    s.push_str(&format!("# object {}: prim iteration failed: {}\n", i, e));
                }
            }
        }
        vert_base += o.vertices.len();
    }
    std::fs::write(out, s)?;
    eprintln!(
        "wrote {} object(s), {} face(s)",
        tmd.objects.len(),
        total_faces
    );
    Ok(())
}
