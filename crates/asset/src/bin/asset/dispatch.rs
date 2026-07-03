use std::path::PathBuf;

use crate::ModeArg;
use anyhow::Result;
use legaia_asset::{AssetType, DecodeMode, Descriptor, decode, parse_player_lzs, parse_streaming};

pub(crate) fn describe(input: &PathBuf, count: usize) -> Result<()> {
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

pub(crate) fn decode_one(
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

pub(crate) fn scan(dir: &PathBuf, count: usize) -> Result<()> {
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

pub(crate) fn stream_one(input: &PathBuf, max_chunks: usize) -> Result<()> {
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

pub(crate) fn scan_stream(
    dir: &PathBuf,
    max_chunks: usize,
    only_hits: bool,
    min_chunks: usize,
) -> Result<()> {
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

pub(crate) fn detect_extension(asset_type: AssetType, data: &[u8]) -> &'static str {
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

pub(crate) fn extract_streaming(input: &PathBuf, out: &PathBuf, save_trailer: bool) -> Result<()> {
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
        // TMD2 (case 9 in FUN_8001f05c) is a *bare* TMD - the dispatcher passes
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
