use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use legaia_anm::{
    AnmPack, PREAMBLE_SIZE, Preamble, RecordHeader, pack_bytecode_histogram, parse, peel_preamble,
    record_bytes, top_k,
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
    }
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
