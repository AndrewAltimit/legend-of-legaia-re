use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use legaia_anm::{
    AnmPack, KeyframeReader, PREAMBLE_SIZE, Preamble, RecordHeader, pack_bytecode_histogram,
    pack_bytecode_top_bigrams, parse, peel_preamble, record_bytes, top_k,
};

#[derive(Parser)]
#[command(name = "anm", about = "Legaia ANM (asset type 0x06) inspector")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print the count, offset table summary, and per-record headers.
    Info {
        path: PathBuf,
        /// The input has the 16-byte allocator preamble (RAM-extracted blob).
        /// Strip it before parsing.
        #[arg(long, default_value_t = false)]
        with_preamble: bool,
        /// Print all records (default: first 8 + last 4).
        #[arg(long, default_value_t = false)]
        all: bool,
    },
    /// Extract every record to `<out>/rec_<NNN>.bin`.
    Extract {
        path: PathBuf,
        out: PathBuf,
        #[arg(long, default_value_t = false)]
        with_preamble: bool,
    },
    /// Emit a JSON dump of the parsed structure.
    Json {
        path: PathBuf,
        #[arg(long, default_value_t = false)]
        with_preamble: bool,
    },
    /// Build a byte histogram across every record's bytecode region (the
    /// bytes after the 8-byte common header). Surfaces likely opcode bytes
    /// without re-deriving the count loop in every consumer. The bytecode
    /// dispatcher is overlay-resident; this is the static-analysis stand-in.
    Histogram {
        path: PathBuf,
        #[arg(long, default_value_t = false)]
        with_preamble: bool,
        /// Number of top byte values to print (default 16).
        #[arg(long, default_value_t = 16)]
        top: usize,
    },
    /// Build an `(byte_n, byte_{n+1})` bigram histogram across every
    /// record's bytecode region. Bigrams concentrate when the bytecode is
    /// `[op, operand]` paired; they spread when it's variable-length.
    /// Useful for inferring the dispatcher's encoding shape before the
    /// overlay extraction lands.
    Bigrams {
        path: PathBuf,
        #[arg(long, default_value_t = false)]
        with_preamble: bool,
        /// Number of top bigrams to print (default 32).
        #[arg(long, default_value_t = 32)]
        top: usize,
    },
    /// Inspect a record as an animation-opcode-6 keyframe table. Without
    /// `--bones`, infer the bone count from the record size (must satisfy
    /// `size == 8 + 32*N`); with `--bones`, parse against the given count.
    /// Reports per-bone source / target poses + interpolation deltas.
    Keyframes {
        path: PathBuf,
        /// Record index to inspect (default 0).
        #[arg(long, default_value_t = 0)]
        record: usize,
        /// Override the bone count rather than inferring from record size.
        #[arg(long)]
        bones: Option<usize>,
        #[arg(long, default_value_t = false)]
        with_preamble: bool,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Info {
            path,
            with_preamble,
            all,
        } => info(&path, with_preamble, all),
        Cmd::Extract {
            path,
            out,
            with_preamble,
        } => extract(&path, &out, with_preamble),
        Cmd::Json {
            path,
            with_preamble,
        } => json(&path, with_preamble),
        Cmd::Histogram {
            path,
            with_preamble,
            top,
        } => histogram(&path, with_preamble, top),
        Cmd::Bigrams {
            path,
            with_preamble,
            top,
        } => bigrams(&path, with_preamble, top),
        Cmd::Keyframes {
            path,
            record,
            bones,
            with_preamble,
        } => keyframes(&path, record, bones, with_preamble),
    }
}

fn keyframes(path: &Path, record: usize, bones: Option<usize>, with_preamble: bool) -> Result<()> {
    let (payload, _preamble, pack) = load(path, with_preamble)?;
    let r = pack.records.get(record).ok_or_else(|| {
        anyhow::anyhow!("record index {} out of range (0..{})", record, pack.count)
    })?;
    let bytes = record_bytes(&payload, r);
    let bone_count = match bones {
        Some(n) => n,
        None => KeyframeReader::infer_bone_count(bytes.len()).ok_or_else(|| {
            anyhow::anyhow!(
                "record size {} doesn't fit `8 + 32*N` — pass --bones to override",
                bytes.len()
            )
        })?,
    };
    let reader = KeyframeReader::parse(bytes, bone_count)?;
    let header = RecordHeader::from_bytes(bytes)?;
    println!("file:    {}", path.display());
    println!("record:  {} ({} bytes)", record, bytes.len());
    println!(
        "header:  a=0x{:04X} b=0x{:04X} marker=0x{:04X} flag=0x{:04X}{}",
        header.a,
        header.b,
        header.marker_1,
        header.flag,
        if header.marker_ok {
            ""
        } else {
            " (BAD MARKER)"
        }
    );
    println!(
        "bones:   {} (source: {})",
        bone_count,
        if bones.is_some() {
            "explicit"
        } else {
            "inferred"
        }
    );
    println!(
        "bone | src_pos                 dst_pos                 src_rot                 dst_rot"
    );
    for (i, kf) in reader.iter().enumerate() {
        println!(
            "{:>4} | ({:>6}, {:>6}, {:>6}) ({:>6}, {:>6}, {:>6}) ({:>6}, {:>6}, {:>6}) ({:>6}, {:>6}, {:>6})",
            i,
            kf.src_pos[0],
            kf.src_pos[1],
            kf.src_pos[2],
            kf.dst_pos[0],
            kf.dst_pos[1],
            kf.dst_pos[2],
            kf.src_rot[0],
            kf.src_rot[1],
            kf.src_rot[2],
            kf.dst_rot[0],
            kf.dst_rot[1],
            kf.dst_rot[2]
        );
    }
    Ok(())
}

fn bigrams(path: &Path, with_preamble: bool, top: usize) -> Result<()> {
    let (payload, _preamble, pack) = load(path, with_preamble)?;
    let triples = pack_bytecode_top_bigrams(&payload, &pack, top);
    println!("file:    {}", path.display());
    println!("records: {}", pack.records.len());
    println!("top {} bigrams (descending count):", triples.len());
    let total: u32 = triples.iter().map(|(_, _, c)| c).sum();
    for (a, b, c) in &triples {
        let pct = if total > 0 {
            100.0 * (*c as f64) / (total as f64)
        } else {
            0.0
        };
        println!("  0x{:02X} 0x{:02X}  {:>6}  {:>5.1}%", a, b, c, pct);
    }
    Ok(())
}

fn histogram(path: &Path, with_preamble: bool, top: usize) -> Result<()> {
    let (payload, _preamble, pack) = load(path, with_preamble)?;
    let hist = pack_bytecode_histogram(&payload, &pack);
    let total: u32 = hist.iter().sum();
    println!("file:    {}", path.display());
    println!("records: {}", pack.records.len());
    println!("bytes:   {} (excludes 8-byte record headers)", total);
    if total == 0 {
        return Ok(());
    }
    let pairs = top_k(&hist, top);
    println!("top {} bytes (descending count):", pairs.len());
    for (b, c) in pairs {
        let pct = 100.0 * (c as f64) / (total as f64);
        let printable = if (0x20..=0x7E).contains(&b) {
            format!("'{}'", b as char)
        } else {
            "   ".to_string()
        };
        println!("  0x{:02X} {}  {:>6}  {:>5.1}%", b, printable, c, pct);
    }
    Ok(())
}

fn load(path: &Path, with_preamble: bool) -> Result<(Vec<u8>, Option<Preamble>, AnmPack)> {
    let raw = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let (payload, preamble) = if with_preamble {
        let pre = Preamble::from_bytes(&raw)?;
        let payload = peel_preamble(&raw)?.to_vec();
        (payload, Some(pre))
    } else {
        (raw, None)
    };
    let pack = parse(&payload).with_context(|| format!("parse {}", path.display()))?;
    Ok((payload, preamble, pack))
}

fn info(path: &Path, with_preamble: bool, all: bool) -> Result<()> {
    let (payload, preamble, pack) = load(path, with_preamble)?;
    println!("file:           {}", path.display());
    if let Some(pre) = preamble {
        println!("preamble:       {} bytes", PREAMBLE_SIZE);
        println!("  back_ptr      = 0x{:08X}", pre.back_ptr);
        println!("  forward_ptr   = 0x{:08X}", pre.forward_ptr);
        println!("  forward_ptr_2 = 0x{:08X}", pre.forward_ptr_2);
        println!(
            "  expanded_size = 0x{:X} ({})",
            pre.expanded_size, pre.expanded_size
        );
    }
    println!("payload:        {} bytes", pack.payload_size);
    println!("count:          {}", pack.count);
    if pack.count == 0 {
        return Ok(());
    }
    println!("records:");
    let n = pack.records.len();
    let show: Vec<usize> = if all {
        (0..n).collect()
    } else if n > 12 {
        let mut v: Vec<usize> = (0..8).collect();
        v.push(usize::MAX);
        v.extend((n - 4)..n);
        v
    } else {
        (0..n).collect()
    };
    for i in show {
        if i == usize::MAX {
            println!("  ...");
            continue;
        }
        let r = &pack.records[i];
        let bytes = record_bytes(&payload, r);
        let hdr = RecordHeader::from_bytes(bytes).ok();
        match hdr {
            Some(h) => println!(
                "  [{:>2}] off=0x{:>5X} size={:>5} (0x{:X})  a=0x{:04X} b=0x{:04X} flag=0x{:04X} marker={}{}",
                r.index,
                r.offset,
                r.size,
                r.size,
                h.a,
                h.b,
                h.flag,
                if h.marker_ok { "ok" } else { "BAD" },
                if h.flag_known { "" } else { " (UNKNOWN flag)" }
            ),
            None => println!(
                "  [{:>2}] off=0x{:>5X} size={:>5}  (header read failed)",
                r.index, r.offset, r.size
            ),
        }
    }
    Ok(())
}

fn extract(path: &Path, out: &Path, with_preamble: bool) -> Result<()> {
    let (payload, _preamble, pack) = load(path, with_preamble)?;
    std::fs::create_dir_all(out).with_context(|| format!("create out dir {}", out.display()))?;
    for r in &pack.records {
        let bytes = record_bytes(&payload, r);
        let p = out.join(format!("rec_{:03}.bin", r.index));
        std::fs::write(&p, bytes).with_context(|| format!("write {}", p.display()))?;
    }
    println!("wrote {} records to {}", pack.records.len(), out.display());
    Ok(())
}

fn json(path: &Path, with_preamble: bool) -> Result<()> {
    let (_payload, preamble, pack) = load(path, with_preamble)?;
    #[derive(serde::Serialize)]
    struct Out<'a> {
        preamble: Option<Preamble>,
        pack: &'a AnmPack,
    }
    let out = Out {
        preamble,
        pack: &pack,
    };
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}
