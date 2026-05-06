use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use legaia_asset::{
    AssetType, DecodeMode, Descriptor, categorize, decode, effect_bundle, field_pack,
    parse_player_lzs, parse_streaming, stage_geom, tim_scan, tmd_scan, validate,
};
use legaia_prot::cdname;

#[derive(Parser)]
#[command(name = "asset", about = "Legaia asset descriptor + dispatcher")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Parse a buffer as a player.lzs-style container header and print.
    Describe {
        input: PathBuf,
        /// Number of descriptors to read after the 8-byte meta (default 3).
        #[arg(long, default_value_t = 3)]
        count: usize,
    },
    /// Decode one descriptor's payload (`--type-size 0xTTSSSSSS --offset 0xNN --mode lzs|raw`).
    Decode {
        input: PathBuf,
        /// `(type<<24) | size` packed into a single u32, hex-prefixed (e.g. 0x02001000).
        #[arg(long, value_parser = parse_hex_u32)]
        type_size: u32,
        /// Byte offset within the input buffer.
        #[arg(long, value_parser = parse_hex_u32)]
        offset: u32,
        #[arg(long, value_enum, default_value_t = ModeArg::Lzs)]
        mode: ModeArg,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Scan every PROT entry directory, treating each file as a player.lzs
    /// container and reporting any that fully decode.
    Scan {
        dir: PathBuf,
        #[arg(long, default_value_t = 3)]
        count: usize,
    },
    /// Parse a buffer as a DATA_FIELD-style streaming container and dump
    /// each chunk's header + magic.
    Stream {
        input: PathBuf,
        #[arg(long, default_value_t = 4096)]
        max_chunks: usize,
    },
    /// Scan PROT entries for the streaming format used by FUN_8002541c 0x14.
    /// Reports entries that parse cleanly (terminator + all known types +
    /// all known magics match).
    ScanStream {
        dir: PathBuf,
        #[arg(long, default_value_t = 4096)]
        max_chunks: usize,
        /// Print only entries that fully validate.
        #[arg(long, default_value_t = false)]
        only_hits: bool,
        /// Minimum chunk count to consider an entry "interesting".
        #[arg(long, default_value_t = 2)]
        min_chunks: usize,
    },
    /// Extract sub-assets from a streaming-format file. Each TIM_LIST and
    /// TMD chunk is unpacked using the [count, word_offsets, data] format.
    /// Each sub-asset is written to `<out>/chunk{i}_{TYPE}/{j}.{ext}`.
    Extract {
        input: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
        /// Also dump trailing data past the streaming terminator (if any)
        /// to `<out>/_trailer.bin` for later analysis.
        #[arg(long, default_value_t = true)]
        save_trailer: bool,
    },
    /// Bulk format classifier. Walks every file in `dir`, runs each known
    /// parser, and falls back to entropy/signature features. Emits a JSON
    /// report (default `<dir>/categorize.json`) plus a per-class summary.
    Categorize {
        dir: PathBuf,
        /// JSON output path. Defaults to `<dir>/categorize.json`.
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Print top-N first-u32 signatures across the whole directory.
        #[arg(long, default_value_t = 20)]
        top_signatures: usize,
        /// Print up to N example file names per class.
        #[arg(long, default_value_t = 5)]
        examples: usize,
    },
    /// Hunt for the 0x801C0000-overlay file. For each PROT entry, scan the
    /// raw bytes AND the result of LZS-decoding (at several plausible output
    /// sizes) for MIPS code-likelihood (`jr $ra` density, `addiu sp,sp,-N`
    /// prologue density, byte entropy in the code range). Reports the
    /// top-N candidates ranked by score.
    FindOverlay {
        dir: PathBuf,
        /// Number of top candidates to print.
        #[arg(long, default_value_t = 25)]
        top: usize,
        /// LZS output sizes to try, comma-separated. Default covers the
        /// plausible overlay code range (32 KB .. 256 KB).
        #[arg(long, default_value = "32768,65536,98304,131072,196608,262144")]
        lzs_sizes: String,
    },
    /// Scan PROT entries (raw + LZS-decoded) for embedded PSX TIMs.
    /// Reports per-entry hit counts; with `--out` extracts each TIM to
    /// `<out>/<entry>/raw_off<H>.tim` (or `lzs<i>_off<H>.tim`).
    TimScan {
        /// Directory of extracted PROT entries (e.g. `extracted/PROT`).
        dir: PathBuf,
        /// CDNAME.TXT for nicer names. Optional.
        #[arg(long)]
        cdname: Option<PathBuf>,
        /// Print only entries with at least one hit.
        #[arg(long, default_value_t = false)]
        only_hits: bool,
        /// Extract every found TIM into this directory.
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Scan PROT entries (raw + LZS-decoded) for embedded Legaia TMDs.
    /// Reports per-entry hit counts and total verts/prims; with `--out`
    /// extracts each found TMD to `<out>/<entry>/raw_off<H>.tmd` (or
    /// `lzs<i>_off<H>.tmd` for LZS-section hits).
    TmdScan {
        /// Directory of extracted PROT entries (e.g. `extracted/PROT`).
        dir: PathBuf,
        /// CDNAME.TXT for nicer names. Optional.
        #[arg(long)]
        cdname: Option<PathBuf>,
        /// Print only entries with at least one hit.
        #[arg(long, default_value_t = false)]
        only_hits: bool,
        /// Extract every found TMD into this directory.
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Walk `tim_scan/<entry>/*.tim` under `extracted/` and report every
    /// TIM that places its CLUT or image at the requested VRAM cell. Used
    /// to discover which PROT entry provides a missing CLUT row that a
    /// character mesh references — the runtime asset chain is partially
    /// undocumented (see `project_clut_scattering.md`), and this is the
    /// principled discovery step before adding the TIM dir to the viewer's
    /// `--vram-extra-dir` set.
    ClutFinder {
        /// `extracted/` root (must contain `tim_scan/`).
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// VRAM X coordinate (in 16-bit framebuffer units, 0..1024).
        x: u16,
        /// VRAM Y coordinate (0..512).
        y: u16,
        /// When set, only report TIMs whose CLUT covers the cell. Default
        /// reports BOTH CLUT and image cell hits, since a character mesh
        /// might reference either.
        #[arg(long, default_value_t = false)]
        clut_only: bool,
    },
    /// Inspect a stage-geometry PROT entry: detect the records table,
    /// pick the vertex pool side, print the first/last few records resolved
    /// to vertex indices, and a sample of the vertex pool.
    Stage {
        input: PathBuf,
        /// Number of records to print from the head of the table.
        #[arg(long, default_value_t = 8)]
        head: usize,
        /// Number of vertices to print from the head of the pool.
        #[arg(long, default_value_t = 8)]
        verts: usize,
        /// Optional output: write a wavefront-style OBJ of the wireframe
        /// quads (each record becomes 4 line segments — `l` directives).
        #[arg(short, long)]
        obj_out: Option<PathBuf>,
    },
    /// Bulk-scan a directory of PROT entries for stage-geometry tables.
    /// Reports per-entry records / pool size / how many records resolve to
    /// in-range vertex indices.
    StageScan {
        dir: PathBuf,
        /// CDNAME.TXT for nicer entry titles. Optional.
        #[arg(long)]
        cdname: Option<PathBuf>,
        /// Print only entries with at least one record table.
        #[arg(long, default_value_t = true)]
        only_hits: bool,
    },
    /// Inspect a single PROT entry for the field-pack container shape
    /// (97-entry schema after `0x01059B84` magic). Reports preamble size,
    /// schema slot summary, and bytes-after-table.
    FieldPack {
        input: PathBuf,
        /// Print all 97 slot offsets/sizes (otherwise only first/last 8).
        #[arg(long, default_value_t = false)]
        all_slots: bool,
        /// Group slots by size and print the buckets in size-descending
        /// order. Slots in the same bucket are the same kind of record —
        /// the schema is byte-identical across every field-pack instance,
        /// so the cluster output is a static index of slot semantics.
        #[arg(long, default_value_t = false)]
        groups: bool,
    },
    /// Bulk-scan a directory of PROT entries for the field-pack format.
    /// Reports per-entry preamble size, table offset, and bytes-after-table.
    FieldPackScan {
        dir: PathBuf,
        /// Print only entries that match.
        #[arg(long, default_value_t = false)]
        only_hits: bool,
    },
    /// Inspect a single PROT entry for the effect-bundle container shape
    /// (28-entry schema after `0x02018B0C` magic). Reports preamble size,
    /// constant header words, and the schema slot summary.
    EffectBundle {
        input: PathBuf,
        /// Print all 28 slot offsets/sizes (otherwise only first/last 8).
        #[arg(long, default_value_t = false)]
        all_slots: bool,
    },
    /// Bulk-scan a directory of PROT entries for the effect-bundle format.
    /// Reports per-entry preamble size, table offset, and bytes-after-table.
    EffectBundleScan {
        dir: PathBuf,
        /// Print only entries that match.
        #[arg(long, default_value_t = false)]
        only_hits: bool,
    },
    /// Targeted validation: walk PROT entries that correspond to the first
    /// entry of each named CDNAME block. Each is tested with strict layout
    /// and (when applicable) magic checks.
    Validate {
        /// Directory of extracted PROT entries (e.g. `extracted/PROT`).
        dir: PathBuf,
        /// CDNAME.TXT path (block boundaries). If absent, scan ALL entries.
        #[arg(long)]
        cdname: Option<PathBuf>,
        /// Try this many descriptor counts; report the best.
        #[arg(long, default_value = "1,2,3,4,8,16,32")]
        counts: String,
        /// Print only blocks whose first entry validates.
        #[arg(long, default_value_t = false)]
        only_hits: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ModeArg {
    Lzs,
    Raw,
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Describe { input, count } => describe(&input, count),
        Cmd::Decode {
            input,
            type_size,
            offset,
            mode,
            out,
        } => decode_one(&input, type_size, offset, mode, out.as_ref()),
        Cmd::Scan { dir, count } => scan(&dir, count),
        Cmd::Stream { input, max_chunks } => stream_one(&input, max_chunks),
        Cmd::ScanStream {
            dir,
            max_chunks,
            only_hits,
            min_chunks,
        } => scan_stream(&dir, max_chunks, only_hits, min_chunks),
        Cmd::Extract {
            input,
            out,
            save_trailer,
        } => extract_streaming(&input, &out, save_trailer),
        Cmd::TimScan {
            dir,
            cdname,
            only_hits,
            out,
        } => tim_scan_cmd(&dir, cdname.as_deref(), only_hits, out.as_deref()),
        Cmd::TmdScan {
            dir,
            cdname,
            only_hits,
            out,
        } => tmd_scan_cmd(&dir, cdname.as_deref(), only_hits, out.as_deref()),
        Cmd::ClutFinder {
            extracted_root,
            x,
            y,
            clut_only,
        } => clut_finder_cmd(&extracted_root, x, y, clut_only),
        Cmd::Stage {
            input,
            head,
            verts,
            obj_out,
        } => stage_one(&input, head, verts, obj_out.as_deref()),
        Cmd::StageScan {
            dir,
            cdname,
            only_hits,
        } => stage_scan_cmd(&dir, cdname.as_deref(), only_hits),
        Cmd::FieldPack {
            input,
            all_slots,
            groups,
        } => field_pack_one(&input, all_slots, groups),
        Cmd::FieldPackScan { dir, only_hits } => field_pack_scan(&dir, only_hits),
        Cmd::EffectBundle { input, all_slots } => effect_bundle_one(&input, all_slots),
        Cmd::EffectBundleScan { dir, only_hits } => effect_bundle_scan(&dir, only_hits),
        Cmd::Validate {
            dir,
            cdname,
            counts,
            only_hits,
        } => validate_blocks(&dir, cdname.as_ref(), &counts, only_hits),
        Cmd::Categorize {
            dir,
            out,
            top_signatures,
            examples,
        } => categorize_dir(&dir, out.as_ref(), top_signatures, examples),
        Cmd::FindOverlay {
            dir,
            top,
            lzs_sizes,
        } => find_overlay(&dir, top, &lzs_sizes),
    }
}

/// `jr $ra` opcode (0x03E00008) in little-endian byte order.
const MIPS_JR_RA_LE: [u8; 4] = [0x08, 0x00, 0xE0, 0x03];

/// Test whether a 4-byte instruction word is `addiu $sp, $sp, -N`.
/// Encoding: 0x27BD_FFXX (low byte = -imm). LE bytes: [XX, FF, BD, 27].
fn is_sp_prologue(word: u32) -> bool {
    (word & 0xFFFF_0000) == 0x27BD_0000 && (word & 0x8000) != 0
}

/// Count word-aligned occurrences of `jr $ra`.
fn count_jr_ra(buf: &[u8]) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i + 4 <= buf.len() {
        if buf[i..i + 4] == MIPS_JR_RA_LE {
            n += 1;
        }
        i += 4;
    }
    n
}

/// Count word-aligned `addiu $sp, $sp, -N` instructions.
fn count_sp_prologue(buf: &[u8]) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i + 4 <= buf.len() {
        let w = u32::from_le_bytes(buf[i..i + 4].try_into().unwrap());
        if is_sp_prologue(w) {
            n += 1;
        }
        i += 4;
    }
    n
}

/// Score a candidate buffer for "looks like MIPS code". Higher is better.
/// The signal is jr-ra and sp-prologue density per kilobyte, plus a soft
/// bonus when both are present.
fn code_score(buf: &[u8]) -> f32 {
    if buf.len() < 4096 {
        return 0.0;
    }
    let kb = buf.len() as f32 / 1024.0;
    let jr_ra = count_jr_ra(buf) as f32 / kb;
    let prologue = count_sp_prologue(buf) as f32 / kb;
    // Density caps prevent pathological repeated bytes from dominating.
    let s = jr_ra.min(5.0) + prologue.min(5.0);
    if jr_ra > 0.5 && prologue > 0.5 {
        s + 2.0
    } else {
        s
    }
}

fn find_overlay(dir: &PathBuf, top: usize, lzs_sizes: &str) -> Result<()> {
    let sizes: Vec<usize> = lzs_sizes
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<usize>())
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| anyhow::anyhow!("bad --lzs-sizes: {e}"))?;

    #[derive(Clone)]
    struct Hit {
        path: PathBuf,
        size: usize,
        mode: String,
        decoded_size: usize,
        jr_ra: usize,
        prologue: usize,
        score: f32,
    }

    let mut entries: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("BIN"))
        .collect();
    entries.sort();

    let mut hits: Vec<Hit> = Vec::new();
    let mut tried = 0usize;
    for path in &entries {
        let buf = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if buf.len() < 4096 {
            continue;
        }
        tried += 1;

        // Raw scan.
        let s = code_score(&buf);
        if s > 0.3 {
            hits.push(Hit {
                path: path.clone(),
                size: buf.len(),
                mode: "raw".to_string(),
                decoded_size: buf.len(),
                jr_ra: count_jr_ra(&buf),
                prologue: count_sp_prologue(&buf),
                score: s,
            });
        }

        // LZS pass at file-start.
        for &out_sz in &sizes {
            if let Ok((decoded, _consumed)) = legaia_lzs::decompress_tracked(&buf, out_sz) {
                let s = code_score(&decoded);
                if s > 0.3 {
                    hits.push(Hit {
                        path: path.clone(),
                        size: buf.len(),
                        mode: format!("lzs@0+{out_sz}"),
                        decoded_size: decoded.len(),
                        jr_ra: count_jr_ra(&decoded),
                        prologue: count_sp_prologue(&decoded),
                        score: s,
                    });
                }
            }
        }

        // Sub-entry sweep: walk container offsets if the first u32 looks like
        // a small entry-count (player.lzs-style or TIM-pack-style), and try
        // LZS at each pointed-to offset. The runtime treats these the same
        // way -- each (size, offset) pair is independently LZS-decoded.
        if buf.len() >= 16 {
            let first = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;
            // Heuristic count range covering every container we've seen so far.
            if (1..=64).contains(&first) {
                for i in 0..first {
                    let p = 4 + i * 4;
                    if p + 4 > buf.len() {
                        break;
                    }
                    let off = u32::from_le_bytes(buf[p..p + 4].try_into().unwrap()) as usize;
                    if off >= buf.len() || off + 32 > buf.len() {
                        continue;
                    }
                    let sub = &buf[off..];
                    for &out_sz in &sizes {
                        if let Ok((decoded, _)) = legaia_lzs::decompress_tracked(sub, out_sz) {
                            let s = code_score(&decoded);
                            if s > 0.3 {
                                hits.push(Hit {
                                    path: path.clone(),
                                    size: buf.len(),
                                    mode: format!("lzs@0x{off:X}+{out_sz}"),
                                    decoded_size: decoded.len(),
                                    jr_ra: count_jr_ra(&decoded),
                                    prologue: count_sp_prologue(&decoded),
                                    score: s,
                                });
                            }
                        }
                        // Also try raw at this offset (for stored-uncompressed code).
                        if sub.len() >= 4096 {
                            let s = code_score(sub);
                            if s > 0.3 {
                                hits.push(Hit {
                                    path: path.clone(),
                                    size: buf.len(),
                                    mode: format!("raw@0x{off:X}"),
                                    decoded_size: sub.len(),
                                    jr_ra: count_jr_ra(sub),
                                    prologue: count_sp_prologue(sub),
                                    score: s,
                                });
                                break; // raw scoring doesn't depend on out_sz
                            }
                        }
                    }
                }
            }
        }
    }

    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.truncate(top);

    println!(
        "scanned {} files; {} candidates with score > 0.3",
        tried,
        hits.len()
    );
    println!(
        "{:>5} {:>9} {:>14} {:>9} {:>5} {:>5} {:>6}  path",
        "rank", "size", "mode", "out_size", "jr_ra", "prol", "score"
    );
    for (rank, h) in hits.iter().enumerate() {
        let name = h.path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        println!(
            "{:>5} {:>9} {:>14} {:>9} {:>5} {:>5} {:>6.2}  {}",
            rank + 1,
            h.size,
            h.mode,
            h.decoded_size,
            h.jr_ra,
            h.prologue,
            h.score,
            name,
        );
    }
    Ok(())
}

fn describe(input: &PathBuf, count: usize) -> Result<()> {
    let raw = std::fs::read(input)?;
    let c = parse_player_lzs(&raw, count)?;
    println!("meta: 0x{:08X}, 0x{:08X}", c.meta[0], c.meta[1]);
    println!(
        "{:>3}  {:>4}  {:>9}  {:>10}  {:>10}",
        "i", "type", "size", "offset", "type_name"
    );
    for (i, d) in c.descriptors.iter().enumerate() {
        let t = d.asset_type();
        println!(
            "{:>3}  0x{:02X}  {:>9}  0x{:08X}  {}",
            i,
            d.type_byte,
            d.size,
            d.data_offset,
            t.name()
        );
    }
    Ok(())
}

fn decode_one(
    input: &PathBuf,
    type_size: u32,
    offset: u32,
    mode: ModeArg,
    out: Option<&PathBuf>,
) -> Result<()> {
    let raw = std::fs::read(input)?;
    let desc = Descriptor::from_pair(type_size, offset);
    let mode = match mode {
        ModeArg::Lzs => DecodeMode::Lzs,
        ModeArg::Raw => DecodeMode::Raw,
    };
    let decoded = decode(&raw, &desc, mode)?;
    eprintln!(
        "[ok] type={} size={} offset=0x{:X} → {} bytes",
        desc.asset_type().name(),
        desc.size,
        desc.data_offset,
        decoded.len()
    );
    match out {
        Some(p) => std::fs::write(p, &decoded)?,
        None => {
            let preview: String = decoded
                .iter()
                .take(64)
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" ");
            println!("{}", preview);
        }
    }
    Ok(())
}

fn scan(dir: &PathBuf, count: usize) -> Result<()> {
    let mut hits = 0usize;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let raw = match std::fs::read(&path) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let Ok(c) = parse_player_lzs(&raw, count) else {
            continue;
        };

        // Heuristic strictness: every descriptor must have a known type, a
        // sensible size (≥ 64 bytes, ≤ 4 MB), an offset past the header,
        // and decode cleanly under its declared mode (try LZS first, then raw).
        let header_end = (8 + count * 8) as u32;
        let valid_layout = c.descriptors.iter().all(|d| {
            !matches!(d.asset_type(), AssetType::Unknown(_))
                && (64..=4 * 1024 * 1024).contains(&d.size)
                && d.data_offset >= header_end
                && (d.data_offset as usize) < raw.len()
        });
        if !valid_layout {
            continue;
        }

        let mut all_ok = true;
        let mut total = 0usize;
        for d in &c.descriptors {
            let r = decode(&raw, d, DecodeMode::Lzs).or_else(|_| decode(&raw, d, DecodeMode::Raw));
            match r {
                Ok(v) => total += v.len(),
                Err(_) => {
                    all_ok = false;
                    break;
                }
            }
        }
        if all_ok {
            hits += 1;
            println!(
                "{}  meta=[0x{:X},0x{:X}]  descriptors={}  total_decoded={}b",
                path.file_name().unwrap_or_default().to_string_lossy(),
                c.meta[0],
                c.meta[1],
                c.descriptors.len(),
                total
            );
        }
    }
    eprintln!("scan done: {} hits", hits);
    Ok(())
}

fn parse_hex_u32(s: &str) -> std::result::Result<u32, String> {
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    u32::from_str_radix(s, 16).map_err(|e| e.to_string())
}

fn validate_blocks(
    dir: &PathBuf,
    cdname_path: Option<&PathBuf>,
    counts_str: &str,
    only_hits: bool,
) -> Result<()> {
    let counts: Vec<usize> = counts_str
        .split(',')
        .map(|s| s.trim().parse::<usize>())
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| anyhow::anyhow!("invalid --counts: {}", e))?;

    // Build a name lookup table from `<index>_<name>.BIN` filenames produced
    // by prot-extract. Index → full path.
    let mut index_to_path: std::collections::BTreeMap<u32, PathBuf> = Default::default();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some((idx_str, _)) = stem.split_once('_') else {
            continue;
        };
        let Ok(idx) = idx_str.parse::<u32>() else {
            continue;
        };
        index_to_path.insert(idx, p);
    }

    // Pick which entries to test: CDNAME block heads, or all entries.
    let test_indices: Vec<(u32, String)> = if let Some(p) = cdname_path {
        let map = cdname::parse(p)?;
        map.into_iter().collect()
    } else {
        index_to_path
            .keys()
            .map(|&i| (i, format!("entry_{:04}", i)))
            .collect()
    };

    let mut hits = 0usize;
    let mut tried = 0usize;
    for (start_idx, block_name) in &test_indices {
        let Some(path) = index_to_path.get(start_idx) else {
            continue;
        };
        tried += 1;
        let raw = match std::fs::read(path) {
            Ok(r) => r,
            Err(_) => continue,
        };

        // Pick the best count: highest one that yields layout_ok with at
        // least one descriptor decoding cleanly to a known magic OR all
        // descriptors decoding without error.
        let mut best: Option<(usize, legaia_asset::ContainerReport)> = None;
        for &n in &counts {
            if raw.len() < 8 + n * 8 {
                continue;
            }
            let report = match validate(&raw, n) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let any_magic_ok = report
                .descriptors
                .iter()
                .any(|d| d.magic_ok && d.decoded_as.is_some());
            let all_decoded = report
                .descriptors
                .iter()
                .all(|d| d.decoded_as.is_some() || d.error.is_some() && !report.layout_ok);
            // Prefer reports with layout_ok and a real magic hit.
            let score =
                (report.layout_ok as u8) * 4 + (any_magic_ok as u8) * 2 + (all_decoded as u8);
            let prev_score = best.as_ref().map(|(_, r)| {
                (r.layout_ok as u8) * 4
                    + (r.descriptors
                        .iter()
                        .any(|d| d.magic_ok && d.decoded_as.is_some()) as u8)
                        * 2
            });
            if prev_score.is_none_or(|ps| score > ps) {
                best = Some((n, report));
            }
        }

        let Some((count, report)) = best else {
            if !only_hits {
                println!(
                    "[skip] block={} idx={} {}: no count fits",
                    block_name,
                    start_idx,
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
            }
            continue;
        };

        let any_magic_ok = report
            .descriptors
            .iter()
            .any(|d| d.magic_ok && d.decoded_as.is_some());
        let is_hit = report.layout_ok && any_magic_ok;
        if !is_hit && only_hits {
            continue;
        }
        if is_hit {
            hits += 1;
        }
        let tag = if is_hit { "HIT " } else { "miss" };
        println!(
            "{}  block={:<16} idx={:>4}  count={}  layout_ok={}  file={}",
            tag,
            block_name,
            start_idx,
            count,
            report.layout_ok,
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        for d in &report.descriptors {
            let mode = d.decoded_as.unwrap_or("--");
            let mag = d.decoded_magic.as_deref().unwrap_or("        ");
            let magic_tag = if d.magic_ok { "OK " } else { "?? " };
            let len = d
                .decoded_len
                .map(|n| n.to_string())
                .unwrap_or_else(|| "-".into());
            let err = d.error.as_deref().unwrap_or("");
            println!(
                "    [{:>2}] type=0x{:02X} {:>8}  size={:>8}  off=0x{:08X}  mode={:<3}  magic={} {}  decoded={:>8}  {}",
                d.index,
                d.type_byte,
                d.type_name,
                d.size,
                d.data_offset,
                mode,
                mag,
                magic_tag,
                len,
                err
            );
        }
    }
    eprintln!("validate done: {} blocks tested, {} hits", tried, hits);
    Ok(())
}

fn categorize_dir(
    dir: &PathBuf,
    out: Option<&PathBuf>,
    top_signatures: usize,
    examples: usize,
) -> Result<()> {
    use std::collections::BTreeMap;

    #[derive(serde::Serialize)]
    struct PerFile<'a> {
        path: String,
        #[serde(flatten)]
        report: &'a categorize::FileReport,
    }

    #[derive(serde::Serialize)]
    struct ClassBucket<'a> {
        class: &'static str,
        count: usize,
        total_bytes: usize,
        examples: Vec<&'a String>,
    }

    #[derive(serde::Serialize)]
    struct SignatureBucket {
        first_u32_hex: String,
        count: usize,
        examples: Vec<String>,
    }

    #[derive(serde::Serialize)]
    struct Report<'a> {
        scan_root: String,
        n_files: usize,
        per_file: Vec<PerFile<'a>>,
        by_class: Vec<ClassBucket<'a>>,
        top_signatures: Vec<SignatureBucket>,
    }

    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    paths.sort();

    let mut reports: Vec<categorize::FileReport> = Vec::with_capacity(paths.len());
    let mut names: Vec<String> = Vec::with_capacity(paths.len());

    for p in &paths {
        let buf = match std::fs::read(p) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("read {}: {}", p.display(), e);
                continue;
            }
        };
        reports.push(categorize::classify(&buf));
        names.push(
            p.file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string()),
        );
    }

    let n_files = reports.len();

    // Group by class.
    let mut by_class: BTreeMap<&'static str, (usize, usize, Vec<&String>)> = BTreeMap::new();
    for (i, r) in reports.iter().enumerate() {
        let entry = by_class.entry(r.class.name()).or_insert((0, 0, Vec::new()));
        entry.0 += 1;
        entry.1 += r.size;
        if entry.2.len() < examples {
            entry.2.push(&names[i]);
        }
    }

    // Group by first-u32 signature.
    let mut by_sig: BTreeMap<u32, (usize, Vec<String>)> = BTreeMap::new();
    for (i, r) in reports.iter().enumerate() {
        let Some(sig) = r.first_u32 else { continue };
        let entry = by_sig.entry(sig).or_insert((0, Vec::new()));
        entry.0 += 1;
        if entry.1.len() < 3 {
            entry.1.push(names[i].clone());
        }
    }
    let mut sigs: Vec<SignatureBucket> = by_sig
        .into_iter()
        .map(|(s, (c, ex))| SignatureBucket {
            first_u32_hex: format!("0x{:08X}", s),
            count: c,
            examples: ex,
        })
        .collect();
    sigs.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then(a.first_u32_hex.cmp(&b.first_u32_hex))
    });
    sigs.truncate(top_signatures);

    let class_buckets: Vec<ClassBucket> = by_class
        .iter()
        .map(|(name, (c, b, ex))| ClassBucket {
            class: name,
            count: *c,
            total_bytes: *b,
            examples: ex.clone(),
        })
        .collect();

    // Console summary.
    println!("=== categorize: {} files ===", n_files);
    println!();
    println!(
        "{:>5}  {:>9}  class                      examples",
        "n", "MB"
    );
    let mut sorted_classes: Vec<_> = by_class.iter().collect();
    sorted_classes.sort_by_key(|b| std::cmp::Reverse(b.1.0));
    for (name, (count, total, ex)) in &sorted_classes {
        let mb = (*total as f64) / (1024.0 * 1024.0);
        let ex_str = ex
            .iter()
            .take(3)
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        println!("{:>5}  {:>9.2}  {:<26} {}", count, mb, name, ex_str);
    }
    println!();
    println!("=== top {} first-u32 signatures ===", sigs.len());
    println!("{:>5}  {:<12}  examples", "n", "signature");
    for sb in &sigs {
        let ex = sb
            .examples
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        println!("{:>5}  {:<12}  {}", sb.count, sb.first_u32_hex, ex);
    }

    let per_file: Vec<PerFile> = reports
        .iter()
        .zip(names.iter())
        .map(|(r, name)| PerFile {
            path: name.clone(),
            report: r,
        })
        .collect();

    let report = Report {
        scan_root: dir.display().to_string(),
        n_files,
        per_file,
        by_class: class_buckets,
        top_signatures: sigs,
    };

    let out_path: PathBuf = out.cloned().unwrap_or_else(|| dir.join("categorize.json"));
    let json = serde_json::to_string_pretty(&report)?;
    std::fs::write(&out_path, json)?;
    eprintln!("wrote {}", out_path.display());
    Ok(())
}

fn stream_one(input: &PathBuf, max_chunks: usize) -> Result<()> {
    let raw = std::fs::read(input)?;
    let r = parse_streaming(&raw, max_chunks)?;
    println!(
        "chunks={}  terminated={}  all_known_types={}  all_magic_ok={}  bytes_consumed={} / {}",
        r.chunks.len(),
        r.terminated,
        r.all_known_types,
        r.all_magic_ok,
        r.bytes_consumed,
        raw.len()
    );
    println!(
        "{:>3}  {:>4}  {:>9}  {:>10}  {:>9}  magic_ok",
        "i", "type", "size", "off", "name"
    );
    for (i, c) in r.chunks.iter().enumerate() {
        println!(
            "{:>3}  0x{:02X}  {:>9}  0x{:08X}  {:>9}  {} {}",
            i,
            c.type_byte,
            c.size,
            c.header_offset,
            c.type_name,
            c.magic,
            if c.magic_ok { "ok" } else { "MISMATCH" },
        );
    }
    Ok(())
}

fn scan_stream(dir: &PathBuf, max_chunks: usize, only_hits: bool, min_chunks: usize) -> Result<()> {
    let mut hits = 0usize;
    let mut tried = 0usize;
    // Sort by filename so output is stable.
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    paths.sort();

    for path in &paths {
        tried += 1;
        let raw = match std::fs::read(path) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let report = match parse_streaming(&raw, max_chunks) {
            Ok(r) => r,
            Err(_) => continue,
        };
        // A "hit" is: terminated cleanly, all types known, all magics ok,
        // and at least min_chunks chunks (so empty/junk doesn't count).
        let is_hit = report.terminated
            && report.all_known_types
            && report.all_magic_ok
            && report.chunks.len() >= min_chunks;
        if !is_hit && only_hits {
            continue;
        }
        if is_hit {
            hits += 1;
        }
        let tag = if is_hit { "HIT " } else { "miss" };
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        println!(
            "{}  {}  chunks={:<3} terminated={} types_ok={} magic_ok={} bytes={}/{}",
            tag,
            name,
            report.chunks.len(),
            report.terminated,
            report.all_known_types,
            report.all_magic_ok,
            report.bytes_consumed,
            raw.len()
        );
        if is_hit {
            for c in report.chunks.iter().take(8) {
                println!(
                    "      [{}] type=0x{:02X} {:<8}  size={:>8}  magic={} {}",
                    c.header_offset,
                    c.type_byte,
                    c.type_name,
                    c.size,
                    c.magic,
                    if c.magic_ok { "ok" } else { "??" },
                );
            }
            if report.chunks.len() > 8 {
                println!("      ... +{} more", report.chunks.len() - 8);
            }
        }
    }
    eprintln!("scan-stream done: {} entries tested, {} hits", tried, hits);
    Ok(())
}

fn detect_extension(asset_type: AssetType, data: &[u8]) -> &'static str {
    // Pre-empt by content first: a TIM always starts with 0x00000010.
    if data.len() >= 4 && u32::from_le_bytes(data[..4].try_into().unwrap()) == 0x0000_0010 {
        return "tim";
    }
    match asset_type {
        AssetType::Tim => "tim",
        AssetType::TimList => "tim",
        AssetType::Tmd | AssetType::Tmd2 => "tmd",
        _ => "bin",
    }
}

fn extract_streaming(input: &PathBuf, out: &PathBuf, save_trailer: bool) -> Result<()> {
    let raw = std::fs::read(input)?;
    let report = parse_streaming(&raw, 4096)?;
    if !report.terminated {
        eprintln!(
            "[warn] streaming parse did not hit terminator (consumed {}/{})",
            report.bytes_consumed,
            raw.len()
        );
    }

    std::fs::create_dir_all(out)?;
    let mut total_subassets = 0usize;
    for (chunk_idx, c) in report.chunks.iter().enumerate() {
        let chunk_data_start = c.header_offset + 4;
        let chunk_data_end = chunk_data_start + c.size as usize;
        let chunk_data = &raw[chunk_data_start..chunk_data_end];
        let t = AssetType::from_byte(c.type_byte);

        // Decide: pack-style (TIM_LIST/TMD) or single-asset.
        // TMD2 (case 9 in FUN_8001f05c) is a *bare* TMD — the dispatcher passes
        // the buffer directly to FUN_80026b4c without walking a pack header.
        // Case 2 (TMD), by contrast, walks `puVar1[i]` as pack offsets.
        let is_pack = matches!(t, AssetType::TimList | AssetType::Tmd);
        let chunk_dir = out.join(format!("chunk{:02}_{}", chunk_idx, t.name()));
        std::fs::create_dir_all(&chunk_dir)?;

        if is_pack {
            match legaia_asset::pack::extract_pack(chunk_data) {
                Ok(items) => {
                    println!(
                        "chunk @ 0x{:08X}  type={:<8}  size={:>9}  ->  {} sub-assets",
                        c.header_offset,
                        t.name(),
                        c.size,
                        items.len()
                    );
                    for (j, item) in items.iter().enumerate() {
                        let ext = detect_extension(t, item);
                        let path = chunk_dir.join(format!("{:04}.{}", j, ext));
                        std::fs::write(&path, item)?;
                        total_subassets += 1;
                    }
                }
                Err(e) => {
                    eprintln!(
                        "[warn] chunk @ 0x{:X} ({}, size={}) is not a valid pack: {}",
                        c.header_offset,
                        t.name(),
                        c.size,
                        e
                    );
                    let path = chunk_dir.join("raw.bin");
                    std::fs::write(&path, chunk_data)?;
                    total_subassets += 1;
                }
            }
        } else {
            let ext = detect_extension(t, chunk_data);
            let path = chunk_dir.join(format!("0000.{}", ext));
            std::fs::write(&path, chunk_data)?;
            total_subassets += 1;
            println!(
                "chunk @ 0x{:08X}  type={:<8}  size={:>9}  ->  raw.{}",
                c.header_offset,
                t.name(),
                c.size,
                ext
            );
        }
    }

    if save_trailer && report.bytes_consumed < raw.len() {
        let trailer_path = out.join("_trailer.bin");
        std::fs::write(&trailer_path, &raw[report.bytes_consumed..])?;
        println!(
            "trailer @ 0x{:08X}  size={:>9}  -> _trailer.bin",
            report.bytes_consumed,
            raw.len() - report.bytes_consumed
        );
    }

    eprintln!("extract done: {} sub-assets", total_subassets);
    Ok(())
}

fn tmd_scan_cmd(
    dir: &std::path::Path,
    cdname_path: Option<&std::path::Path>,
    only_hits: bool,
    out: Option<&std::path::Path>,
) -> Result<()> {
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };

    if let Some(out) = out {
        std::fs::create_dir_all(out)?;
    }

    println!(
        "{:<32}  {:>4}  {:>4}  {:>5}  {:>6}  notes",
        "entry", "raw", "lzs", "verts", "prims"
    );
    println!("{}", "-".repeat(80));

    let mut total_hits = 0usize;
    let mut total_verts = 0u32;
    let mut total_prims = 0u32;
    let mut entries_with_hits = 0usize;
    let mut tmds_written = 0usize;

    for path in &entries {
        let raw = std::fs::read(path)?;
        let scan = tmd_scan::scan_entry(&raw);
        if scan.hits.is_empty() && only_hits {
            continue;
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let display_name = display_name_for(&stem, names.as_ref());

        let raw_hits = scan
            .hits
            .iter()
            .filter(|(s, _)| matches!(s, tmd_scan::Source::Raw))
            .count();
        let lzs_hits = scan.hits.len() - raw_hits;
        let v: u32 = scan.hits.iter().map(|(_, h)| h.total_verts).sum();
        let p: u32 = scan.hits.iter().map(|(_, h)| h.total_prims).sum();
        let notes = if scan.lzs_ok { "" } else { "(lzs:no)" };
        if !scan.hits.is_empty() {
            println!(
                "{:<32}  {:>4}  {:>4}  {:>5}  {:>6}  {}",
                display_name, raw_hits, lzs_hits, v, p, notes
            );
            entries_with_hits += 1;
            total_hits += scan.hits.len();
            total_verts += v;
            total_prims += p;
        } else if !only_hits {
            println!(
                "{:<32}  {:>4}  {:>4}  {:>5}  {:>6}  {}",
                display_name, "-", "-", "-", "-", notes
            );
        }

        if let Some(out_root) = out {
            let entry_dir = out_root.join(&display_name);
            for (src, hit) in &scan.hits {
                let (buf, label) = match src {
                    tmd_scan::Source::Raw => (raw.as_slice(), "raw".to_string()),
                    tmd_scan::Source::Lzs(idx) => {
                        let Some(section) = scan.lzs_sections.get(*idx) else {
                            continue;
                        };
                        (section.as_slice(), format!("lzs{}", idx))
                    }
                };
                let end = (hit.offset + hit.byte_len).min(buf.len());
                let slab = &buf[hit.offset..end];
                std::fs::create_dir_all(&entry_dir)?;
                let fname = format!("{}_off{:06X}.tmd", label, hit.offset);
                std::fs::write(entry_dir.join(&fname), slab)?;
                tmds_written += 1;
            }
        }
    }

    println!();
    println!(
        "{} entries with TMDs, {} hits total ({} verts, {} prims)",
        entries_with_hits, total_hits, total_verts, total_prims
    );
    if out.is_some() {
        println!("wrote {} TMD files", tmds_written);
    }
    Ok(())
}

fn tim_scan_cmd(
    dir: &std::path::Path,
    cdname_path: Option<&std::path::Path>,
    only_hits: bool,
    out: Option<&std::path::Path>,
) -> Result<()> {
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };

    if let Some(out) = out {
        std::fs::create_dir_all(out)?;
    }

    println!(
        "{:<32}  {:>4}  {:>4}  {:>5}  {:>5}  notes",
        "entry", "raw", "lzs", "tims", "px"
    );
    println!("{}", "-".repeat(80));

    let mut total_hits = 0usize;
    let mut entries_with_hits = 0usize;
    let mut tims_written = 0usize;

    for path in &entries {
        let raw = std::fs::read(path)?;
        let scan = tim_scan::scan_entry(&raw);
        if scan.hits.is_empty() && only_hits {
            continue;
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let display_name = display_name_for(&stem, names.as_ref());

        let raw_hits = scan
            .hits
            .iter()
            .filter(|(s, _)| matches!(s, tim_scan::Source::Raw))
            .count();
        let lzs_hits = scan.hits.len() - raw_hits;
        let total_px: u64 = scan
            .hits
            .iter()
            .map(|(_, h)| h.width as u64 * h.height as u64)
            .sum();
        let notes = if scan.lzs_ok { "" } else { "(lzs:no)" };
        if !scan.hits.is_empty() {
            println!(
                "{:<32}  {:>4}  {:>4}  {:>5}  {:>5}  {}",
                display_name,
                raw_hits,
                lzs_hits,
                scan.hits.len(),
                total_px,
                notes
            );
            entries_with_hits += 1;
            total_hits += scan.hits.len();
        } else if !only_hits {
            println!(
                "{:<32}  {:>4}  {:>4}  {:>5}  {:>5}  {}",
                display_name, "-", "-", "-", "-", notes
            );
        }

        if let Some(out_root) = out {
            let entry_dir = out_root.join(&display_name);
            for (src, hit) in &scan.hits {
                let (buf, label) = match src {
                    tim_scan::Source::Raw => (raw.as_slice(), "raw".to_string()),
                    tim_scan::Source::Lzs(idx) => {
                        let Some(section) = scan.lzs_sections.get(*idx) else {
                            continue;
                        };
                        (section.as_slice(), format!("lzs{}", idx))
                    }
                };
                let end = (hit.offset + hit.byte_len).min(buf.len());
                let slab = &buf[hit.offset..end];
                std::fs::create_dir_all(&entry_dir)?;
                let fname = format!(
                    "{}_off{:06X}_{}x{}_{}bpp.tim",
                    label, hit.offset, hit.width, hit.height, hit.bpp
                );
                std::fs::write(entry_dir.join(&fname), slab)?;
                tims_written += 1;
            }
        }
    }

    println!();
    println!(
        "{} entries with TIMs, {} hits total",
        entries_with_hits, total_hits
    );
    if out.is_some() {
        println!("wrote {} TIM files", tims_written);
    }
    Ok(())
}

/// `asset stage <PATH>` — dump one entry's stage-geometry layout. Useful
/// to confirm pool placement, sample resolved quad indices, and (with
/// `--obj-out`) export a wireframe mesh for any external viewer.
fn stage_one(input: &PathBuf, head: usize, verts: usize, obj_out: Option<&Path>) -> Result<()> {
    let raw = std::fs::read(input)?;
    let stage = stage_geom::parse(&raw)
        .ok_or_else(|| anyhow::anyhow!("no stage-geometry tables in {}", input.display()))?;
    println!(
        "file: {}  size={}  tables={}",
        input.display(),
        raw.len(),
        stage.tables.len()
    );
    for (i, t) in stage.tables.iter().enumerate() {
        println!(
            "  table[{}]: start=0x{:X} ({})  records={}  end=0x{:X}",
            i, t.start, t.start, t.records, t.end
        );
    }
    println!(
        "vertex pool: offset=0x{:X} ({})  bytes={}  verts={}",
        stage.pool_offset,
        stage.pool_offset,
        stage.pool_bytes,
        stage.vertex_count()
    );

    let largest = stage
        .tables
        .iter()
        .max_by_key(|t| t.records)
        .expect("at least one");
    println!("\nfirst {} records (resolved):", head.min(largest.records));
    let mut resolved = 0usize;
    let mut unresolved = 0usize;
    for (i, rec) in stage_geom::records(&raw, largest).enumerate().take(head) {
        let pl = rec.payload_u16s();
        match stage.quad_vertex_indices(&rec) {
            Some(idx) => {
                let kind = if idx[3] == idx[0] { "tri" } else { "quad" };
                println!(
                    "  rec {:>4}: bytes [{:>5} {:>5} {:>5} {:>5}]  -> {} verts {:?}",
                    i, pl[0], pl[1], pl[2], pl[3], kind, idx
                );
                resolved += 1;
            }
            None => {
                println!(
                    "  rec {:>4}: bytes [{:>5} {:>5} {:>5} {:>5}]  -> OUT OF RANGE",
                    i, pl[0], pl[1], pl[2], pl[3]
                );
                unresolved += 1;
            }
        }
    }
    // Tally for the whole table so the user knows the overall hit rate.
    let mut total_resolved = 0usize;
    for rec in stage_geom::records(&raw, largest) {
        if stage.quad_vertex_indices(&rec).is_some() {
            total_resolved += 1;
        }
    }
    println!(
        "\nresolved {}/{} records overall ({} shown above: {} ok, {} oor)",
        total_resolved,
        largest.records,
        head.min(largest.records),
        resolved,
        unresolved
    );

    println!("\nfirst {} vertices:", verts.min(stage.vertex_count()));
    for i in 0..verts.min(stage.vertex_count()) {
        let v = stage.vertex(&raw, i).expect("in range");
        println!("  v{:<4}: x={:>6} y={:>6} z={:>6}", i, v.x, v.y, v.z);
    }

    if let Some(out) = obj_out {
        write_stage_obj(&raw, &stage, largest, out)?;
        println!("\nwrote wireframe OBJ: {}", out.display());
    }
    Ok(())
}

/// Write a Wavefront OBJ with all in-range quads/tris from `table` as line
/// loops (`l` directives). Standard 3D viewers render these as wireframe.
fn write_stage_obj(
    buf: &[u8],
    stage: &stage_geom::Stage,
    table: &stage_geom::GeomTable,
    out: &Path,
) -> Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(out)?;
    writeln!(f, "# stage-geometry wireframe")?;
    writeln!(
        f,
        "# verts={}  records={}",
        stage.vertex_count(),
        table.records
    )?;
    for i in 0..stage.vertex_count() {
        let v = stage.vertex(buf, i).unwrap();
        // OBJ is right-handed Y-up; the source is PSX Y-down, so flip Y.
        writeln!(f, "v {} {} {}", v.x, -(v.y as i32), v.z)?;
    }
    for rec in stage_geom::records(buf, table) {
        let Some(idx) = stage.quad_vertex_indices(&rec) else {
            continue;
        };
        // OBJ indices are 1-based; degenerate 4th vert (idx[3] == idx[0])
        // collapses naturally in a 4-vertex line loop.
        let a = idx[0] + 1;
        let b = idx[1] + 1;
        let c = idx[2] + 1;
        let d = idx[3] + 1;
        writeln!(f, "l {} {} {} {} {}", a, b, c, d, a)?;
    }
    Ok(())
}

/// `asset clut-finder` — walk `extracted/tim_scan/<entry>/*.tim` and report
/// every TIM whose CLUT or image rect covers the requested VRAM cell.
///
/// Used to discover which PROT entry provides a specific CLUT row that a
/// character mesh references — see `project_clut_scattering.md`.
fn clut_finder_cmd(extracted_root: &Path, x: u16, y: u16, clut_only: bool) -> Result<()> {
    let tim_scan_root = extracted_root.join("tim_scan");
    if !tim_scan_root.is_dir() {
        anyhow::bail!(
            "no tim_scan/ under {} (run `asset tim-scan` first?)",
            extracted_root.display()
        );
    }
    let mut hits: Vec<(String, String, &'static str, u16, u16, u16, u16)> = Vec::new();

    let mut subdirs: Vec<PathBuf> = std::fs::read_dir(&tim_scan_root)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    subdirs.sort();

    for sub in &subdirs {
        let entry_name = sub
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let mut tims: Vec<PathBuf> = std::fs::read_dir(sub)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .map(|e| e == "tim" || e == "TIM")
                        .unwrap_or(false)
            })
            .collect();
        tims.sort();
        for tim_path in &tims {
            let bytes = match std::fs::read(tim_path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let tim = match legaia_tim::parse(&bytes) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let tim_name = tim_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            if let Some(c) = &tim.clut {
                let inside = x >= c.fb_x && x < c.fb_x + c.w && y >= c.fb_y && y < c.fb_y + c.h;
                if inside {
                    hits.push((
                        entry_name.clone(),
                        tim_name.clone(),
                        "clut",
                        c.fb_x,
                        c.fb_y,
                        c.w,
                        c.h,
                    ));
                }
            }
            if !clut_only {
                let img = &tim.image;
                let inside = x >= img.fb_x
                    && x < img.fb_x + img.fb_w
                    && y >= img.fb_y
                    && y < img.fb_y + img.h;
                if inside {
                    hits.push((
                        entry_name.clone(),
                        tim_name,
                        "image",
                        img.fb_x,
                        img.fb_y,
                        img.fb_w,
                        img.h,
                    ));
                }
            }
        }
    }
    println!(
        "VRAM cell ({x}, {y}): {} match(es) across {} entries",
        hits.len(),
        subdirs.len()
    );
    println!(
        "{:<28}  {:<24}  {:<6}  {:>4} {:>4} {:>4} {:>4}",
        "entry", "tim", "kind", "fbx", "fby", "w", "h"
    );
    println!("{}", "-".repeat(80));
    for (entry, tim, kind, fx, fy, w, h) in &hits {
        println!("{entry:<28}  {tim:<24}  {kind:<6}  {fx:>4} {fy:>4} {w:>4} {h:>4}");
    }
    Ok(())
}

/// `asset stage-scan <DIR>` — scan a directory of PROT entries for
/// stage-geometry tables and report per-entry stats.
fn stage_scan_cmd(dir: &Path, cdname_path: Option<&Path>, only_hits: bool) -> Result<()> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    paths.sort();
    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };

    println!(
        "{:<32}  {:>5}  {:>4}  {:>6}  {:>6}  {:>4}  pool",
        "entry", "size", "tabs", "recs", "verts", "ok%"
    );
    println!("{}", "-".repeat(80));

    let mut total_hits = 0usize;
    let mut total_resolved = 0usize;
    let mut total_records = 0usize;
    for path in &paths {
        let raw = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let Some(stage) = stage_geom::parse(&raw) else {
            if !only_hits {
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
                let display_name = display_name_for(stem, names.as_ref());
                println!(
                    "{:<32}  {:>5}  {:>4}  {:>6}  {:>6}  {:>4}  no table",
                    display_name,
                    raw.len(),
                    "-",
                    "-",
                    "-",
                    "-"
                );
            }
            continue;
        };
        total_hits += 1;
        let largest = stage
            .tables
            .iter()
            .max_by_key(|t| t.records)
            .expect("at least one");
        let mut resolved = 0usize;
        for rec in stage_geom::records(&raw, largest) {
            if stage.quad_vertex_indices(&rec).is_some() {
                resolved += 1;
            }
        }
        total_resolved += resolved;
        total_records += largest.records;
        let pct = (100 * resolved).checked_div(largest.records).unwrap_or(0);
        let pool_side = if stage.pool_offset == 0 {
            "before"
        } else {
            "after"
        };

        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
        let display_name = display_name_for(stem, names.as_ref());
        println!(
            "{:<32}  {:>5}  {:>4}  {:>6}  {:>6}  {:>3}%  {}",
            display_name,
            raw.len(),
            stage.tables.len(),
            largest.records,
            stage.vertex_count(),
            pct,
            pool_side
        );
    }
    println!();
    println!(
        "{} entries with stage tables; {}/{} records resolved overall ({:.1}%)",
        total_hits,
        total_resolved,
        total_records,
        if total_records > 0 {
            100.0 * total_resolved as f64 / total_records as f64
        } else {
            0.0
        }
    );
    Ok(())
}

fn field_pack_one(input: &PathBuf, all_slots: bool, groups: bool) -> Result<()> {
    let raw = std::fs::read(input)?;
    let Some(fp) = field_pack::detect(&raw) else {
        anyhow::bail!(
            "no field-pack signature in {} ({} bytes)",
            input.display(),
            raw.len()
        );
    };
    let (preamble_lo, preamble_hi) = fp.preamble_range();
    let (assets_lo, assets_hi) = fp.assets_range();
    println!("file:           {}", input.display());
    println!(
        "size:           {} bytes (0x{:X})",
        fp.file_size, fp.file_size
    );
    println!(
        "preamble:       0x{:X}..0x{:X} ({} bytes)",
        preamble_lo,
        preamble_hi,
        preamble_hi - preamble_lo
    );
    println!(
        "magic @         0x{:X} (= 0x{:08X})",
        fp.magic_offset,
        field_pack::MAGIC
    );
    println!(
        "schema table:   0x{:X}..0x{:X} ({} entries × 4 = {} bytes)",
        fp.table_offset,
        fp.table_offset + field_pack::SCHEMA_SIZE,
        field_pack::RECORD_COUNT,
        field_pack::SCHEMA_SIZE
    );
    println!(
        "assets region:  0x{:X}..0x{:X} ({} bytes)",
        assets_lo,
        assets_hi,
        assets_hi - assets_lo
    );
    println!();
    println!("schema slots:");
    let n = fp.slots.len();
    let show: Vec<usize> = if all_slots {
        (0..n).collect()
    } else {
        let mut v: Vec<usize> = (0..n.min(8)).collect();
        if n > 16 {
            v.push(usize::MAX); // sentinel for ellipsis
            v.extend((n - 8)..n);
        } else {
            v.extend(8..n);
        }
        v
    };
    for i in show {
        if i == usize::MAX {
            println!("  ...");
            continue;
        }
        let s = &fp.slots[i];
        match s.size {
            Some(sz) => println!(
                "  [{:>2}] off=0x{:>5X}  size={:>5} (0x{:X})",
                i, s.offset, sz, sz
            ),
            None => println!("  [{:>2}] off=0x{:>5X}  size=  ?", i, s.offset),
        }
    }
    if groups {
        println!();
        println!("slot size groups (slots sharing the same size = same record kind):");
        for (size, idxs) in fp.slot_size_groups() {
            let head: Vec<String> = idxs.iter().take(10).map(|i| i.to_string()).collect();
            let tail = if idxs.len() > 10 {
                format!(" … (+{} more)", idxs.len() - 10)
            } else {
                String::new()
            };
            println!(
                "  size={:>5} (0x{:X})  count={:>3}  slots={}{}",
                size,
                size,
                idxs.len(),
                head.join(","),
                tail
            );
        }
    }
    Ok(())
}

fn field_pack_scan(dir: &Path, only_hits: bool) -> Result<()> {
    let mut files: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    println!(
        "{:<32}  {:>9}  {:>10}  {:>9}  {:>9}",
        "entry", "size", "table_off", "preamble", "assets"
    );
    println!("{}", "-".repeat(76));
    let mut hits = 0usize;
    let mut total = 0usize;
    for path in &files {
        total += 1;
        let raw = std::fs::read(path)?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
        match field_pack::detect(&raw) {
            Some(fp) => {
                hits += 1;
                let (assets_lo, assets_hi) = fp.assets_range();
                println!(
                    "{:<32}  {:>9}  0x{:>8X}  {:>9}  {:>9}",
                    stem,
                    fp.file_size,
                    fp.table_offset,
                    fp.magic_offset,
                    assets_hi - assets_lo,
                );
            }
            None => {
                if !only_hits {
                    println!(
                        "{:<32}  {:>9}  {:>10}  {:>9}  {:>9}",
                        stem,
                        raw.len(),
                        "-",
                        "-",
                        "-"
                    );
                }
            }
        }
    }
    println!();
    println!(
        "{} of {} entries match the field-pack signature",
        hits, total
    );
    Ok(())
}

fn effect_bundle_one(input: &PathBuf, all_slots: bool) -> Result<()> {
    let raw = std::fs::read(input)?;
    let Some(eb) = effect_bundle::detect(&raw) else {
        anyhow::bail!(
            "no effect-bundle signature in {} ({} bytes)",
            input.display(),
            raw.len()
        );
    };
    let (preamble_lo, preamble_hi) = eb.preamble_range();
    let (assets_lo, assets_hi) = eb.assets_range();
    println!("file:           {}", input.display());
    println!(
        "size:           {} bytes (0x{:X})",
        eb.file_size, eb.file_size
    );
    println!(
        "preamble:       0x{:X}..0x{:X} ({} bytes)",
        preamble_lo,
        preamble_hi,
        preamble_hi - preamble_lo
    );
    println!(
        "magic @         0x{:X} (= 0x{:08X})",
        eb.magic_offset,
        effect_bundle::MAGIC
    );
    println!(
        "header_a:       0x{:08X}{}",
        eb.header_a,
        if eb.header_a == effect_bundle::HEADER_A {
            " (= constant)"
        } else {
            " (UNEXPECTED)"
        }
    );
    println!(
        "header_b:       0x{:08X}{}",
        eb.header_b,
        if eb.header_b == effect_bundle::HEADER_B {
            " (= constant)"
        } else {
            " (UNEXPECTED)"
        }
    );
    println!(
        "schema table:   0x{:X}..0x{:X} ({} entries × 4 = {} bytes)",
        eb.table_offset,
        eb.table_offset + effect_bundle::TABLE_SIZE,
        effect_bundle::RECORD_COUNT,
        effect_bundle::TABLE_SIZE
    );
    println!(
        "assets region:  0x{:X}..0x{:X} ({} bytes)",
        assets_lo,
        assets_hi,
        assets_hi - assets_lo
    );
    println!();
    println!("asset region content:");
    let n_tmds = eb.assets.tmds.len();
    let n_tims = eb.assets.tims.len();
    println!(
        "  {} TMD(s) — {} master + {} sub (HEADER_A reserves 1 master + 28 sub = 29 slots)",
        n_tmds,
        n_tmds.min(1),
        n_tmds.saturating_sub(1),
    );
    if let Some(&master) = eb.assets.tmds.first() {
        println!("    master TMD @ 0x{:X} (= assets_start)", master);
    }
    if eb.assets.tmds.len() > 1 {
        let preview: Vec<String> = eb.assets.tmds[1..]
            .iter()
            .take(4)
            .map(|o| format!("0x{:X}", o))
            .collect();
        let suffix = if eb.assets.tmds.len() > 5 {
            ", …"
        } else {
            ""
        };
        println!("    sub-TMDs   @ {}{}", preview.join(", "), suffix);
    }
    println!("  {} TIM(s)", n_tims);
    if !eb.assets.tims.is_empty() {
        let preview: Vec<String> = eb
            .assets
            .tims
            .iter()
            .take(4)
            .map(|o| format!("0x{:X}", o))
            .collect();
        let suffix = if eb.assets.tims.len() > 4 {
            ", …"
        } else {
            ""
        };
        println!("    @ {}{}", preview.join(", "), suffix);
    }
    println!();
    println!("schema slots:");
    let n = eb.slots.len();
    let show: Vec<usize> = if all_slots {
        (0..n).collect()
    } else {
        let mut v: Vec<usize> = (0..n.min(8)).collect();
        if n > 16 {
            v.push(usize::MAX); // sentinel for ellipsis
            v.extend((n - 8)..n);
        } else {
            v.extend(8..n);
        }
        v
    };
    for i in show {
        if i == usize::MAX {
            println!("  ...");
            continue;
        }
        let s = &eb.slots[i];
        match s.size {
            Some(sz) => println!(
                "  [{:>2}] off=0x{:>5X}  size={:>5} (0x{:X})",
                i, s.offset, sz, sz
            ),
            None => println!("  [{:>2}] off=0x{:>5X}  size=  ?", i, s.offset),
        }
    }
    Ok(())
}

fn effect_bundle_scan(dir: &Path, only_hits: bool) -> Result<()> {
    let mut files: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    println!(
        "{:<32}  {:>9}  {:>10}  {:>9}  {:>9}",
        "entry", "size", "table_off", "preamble", "assets"
    );
    println!("{}", "-".repeat(76));
    let mut hits = 0usize;
    let mut total = 0usize;
    for path in &files {
        total += 1;
        let raw = std::fs::read(path)?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
        match effect_bundle::detect(&raw) {
            Some(eb) => {
                hits += 1;
                let (assets_lo, assets_hi) = eb.assets_range();
                println!(
                    "{:<32}  {:>9}  0x{:>8X}  {:>9}  {:>9}",
                    stem,
                    eb.file_size,
                    eb.table_offset,
                    eb.magic_offset,
                    assets_hi - assets_lo,
                );
            }
            None => {
                if !only_hits {
                    println!(
                        "{:<32}  {:>9}  {:>10}  {:>9}  {:>9}",
                        stem,
                        raw.len(),
                        "-",
                        "-",
                        "-"
                    );
                }
            }
        }
    }
    println!();
    println!(
        "{} of {} entries match the effect-bundle signature",
        hits, total
    );
    Ok(())
}

/// Build a display label for a PROT entry: `<index>_<cdname-block>` if we
/// have a name table, else just the file stem.
fn display_name_for(stem: &str, names: Option<&cdname::IndexMap>) -> String {
    if let Some(names) = names {
        // The PROT file stem looks like "0028_town0c". The numeric prefix
        // before the first underscore is the entry index.
        if let Some((num_str, _)) = stem.split_once('_')
            && let Ok(idx) = num_str.parse::<u32>()
            && let Some(block) = cdname::block_for(names, idx)
        {
            return format!("{:04}_{}", idx, block);
        }
    }
    stem.to_string()
}
