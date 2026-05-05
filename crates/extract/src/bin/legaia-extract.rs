//! Unified Legaia extraction pipeline. Runs disc → PROT → categorize →
//! streaming-format extract → TIM-to-PNG in one shot.
//!
//! Wraps the per-crate library APIs; equivalent to running `disc-extract
//! extract`, `prot-extract extract`, `asset scan-stream`/`extract`, and
//! `tim convert-dir` in sequence, but with one CLI and one output tree.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Parser;
use legaia_asset::{AssetType, pack, parse_streaming};
use legaia_iso::{
    iso9660,
    raw::{RawDisc, USER_DATA_SIZE},
    region,
};
use legaia_prot::{archive::Archive, cdname};

#[derive(Parser)]
#[command(
    name = "legaia-extract",
    about = "Run the full Legaia extraction pipeline (disc → PROT → categorize → sub-assets → PNG)"
)]
struct Cli {
    /// Path to the Legend of Legaia (USA) disc image (.bin, Mode2/2352).
    bin: PathBuf,
    /// Output directory. Created if missing. Existing files are overwritten.
    #[arg(long, default_value = "extracted")]
    out: PathBuf,
    /// Skip the disc verification step (don't compute SHA-256).
    #[arg(long)]
    skip_verify: bool,
    /// Skip TIM → PNG conversion (the slowest step).
    #[arg(long)]
    skip_png: bool,
    /// Print one line per file written.
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if !cli.bin.exists() {
        bail!("disc image not found: {}", cli.bin.display());
    }
    std::fs::create_dir_all(&cli.out)
        .with_context(|| format!("creating output dir {}", cli.out.display()))?;

    let log = |msg: &str| println!("[legaia-extract] {}", msg);
    log(&format!("bin: {}", cli.bin.display()));
    log(&format!("out: {}", cli.out.display()));

    if !cli.skip_verify {
        verify(&cli.bin, log)?;
    } else {
        log("verify: skipped");
    }

    log("step 1/5: disc → ISO9660 files");
    let n = step_disc_extract(&cli.bin, &cli.out, cli.verbose)?;
    log(&format!("  {} files extracted", n));

    log("step 2/5: PROT.DAT → named entries");
    let prot_dir = cli.out.join("PROT");
    let cdname_path = cli.out.join("CDNAME.TXT");
    let cdname_arg = if cdname_path.exists() {
        Some(cdname_path.as_path())
    } else {
        None
    };
    let n_entries = step_prot_extract(
        &cli.out.join("PROT.DAT"),
        &prot_dir,
        cdname_arg,
        cli.verbose,
    )?;
    log(&format!(
        "  {} PROT entries written to {}",
        n_entries,
        prot_dir.display()
    ));

    log("step 3/5: categorize PROT entries");
    let cat_path = prot_dir.join("categorize.json");
    let report = step_categorize(&prot_dir, &cat_path)?;
    log(&format!(
        "  {} files classified → {}",
        report.n_files,
        cat_path.display()
    ));

    log("step 4/5: extract sub-assets from streaming-format entries");
    let stream_dir = cli.out.join("streaming");
    let n_streams = step_streaming_extract(&prot_dir, &stream_dir, cli.verbose)?;
    log(&format!("  {} streaming containers expanded", n_streams));

    if cli.skip_png {
        log("step 5/5: TIM → PNG (skipped via --skip-png)");
    } else {
        log("step 5/5: TIM → PNG");
        let n_png = step_tim_to_png(&stream_dir, cli.verbose)?;
        log(&format!(
            "  {} PNG images written under {}",
            n_png,
            stream_dir.display()
        ));
    }

    log("done");
    Ok(())
}

fn verify(bin: &Path, log: impl Fn(&str)) -> Result<()> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(bin)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let hash = format!("{:x}", hasher.finalize());
    log(&format!("verify: sha256 = {}", hash));
    // Region detection from SYSTEM.CNF
    if let Ok(mut disc) = RawDisc::open(bin)
        && let Ok(vol) = iso9660::read_volume(&mut disc)
        && let Ok(files) = iso9660::walk_files(&mut disc, &vol.root)
        && let Some((_, entry)) = files
            .iter()
            .find(|(p, _)| p.eq_ignore_ascii_case("SYSTEM.CNF"))
    {
        let mut sysbuf = Vec::new();
        let n = entry.size.div_ceil(USER_DATA_SIZE as u32);
        if disc.read_user_data(entry.lba, n, &mut sysbuf).is_ok() {
            sysbuf.truncate(entry.size as usize);
            if let Ok(d) = region::parse(&sysbuf) {
                log(&format!(
                    "verify: region = {} (executable={}, prefix={})",
                    d.region.name(),
                    d.executable,
                    d.prefix
                ));
            }
        }
    }
    Ok(())
}

fn step_disc_extract(bin: &Path, out: &Path, verbose: bool) -> Result<usize> {
    let mut disc = RawDisc::open(bin)?;
    let vol = iso9660::read_volume(&mut disc)?;
    let files = iso9660::walk_files(&mut disc, &vol.root)?;
    let mut buf = Vec::new();
    for (path, entry) in &files {
        let full = out.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let n = entry.size.div_ceil(USER_DATA_SIZE as u32);
        disc.read_user_data(entry.lba, n, &mut buf)?;
        buf.truncate(entry.size as usize);
        std::fs::write(&full, &buf)?;
        if verbose {
            println!("    [disc] {:>10}  {}", entry.size, path);
        }
    }
    Ok(files.len())
}

fn step_prot_extract(
    prot: &Path,
    out: &Path,
    cdname_path: Option<&Path>,
    verbose: bool,
) -> Result<usize> {
    if !prot.exists() {
        bail!("PROT.DAT not found at {}", prot.display());
    }
    let mut archive = Archive::open(prot)?;
    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };
    std::fs::create_dir_all(out)?;
    let mut buf = Vec::new();
    let entries = archive.entries.clone();
    for entry in &entries {
        archive.read_entry(entry, &mut buf)?;
        let block = names
            .as_ref()
            .and_then(|m| cdname::block_for(m, entry.index));
        let stem = match block {
            Some(b) => format!("{:04}_{}", entry.index, b),
            None => format!("{:04}", entry.index),
        };
        let bin_name = format!("{}.BIN", stem);
        std::fs::write(out.join(&bin_name), &buf)?;
        if verbose {
            println!("    [prot] {:>10}  {}", buf.len(), bin_name);
        }
    }
    Ok(entries.len())
}

fn step_categorize(dir: &Path, out: &Path) -> Result<CategorizeSummary> {
    use legaia_asset::categorize::classify;
    let mut n_files = 0usize;
    let mut per_file = serde_json::Map::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if !name.ends_with(".BIN") {
            continue;
        }
        let buf = std::fs::read(&path)?;
        let report = classify(&buf);
        per_file.insert(name, serde_json::to_value(&report)?);
        n_files += 1;
    }
    let summary = serde_json::json!({
        "scan_root": dir.display().to_string(),
        "n_files": n_files,
        "per_file": per_file,
    });
    std::fs::write(out, serde_json::to_string_pretty(&summary)?)?;
    Ok(CategorizeSummary { n_files })
}

struct CategorizeSummary {
    n_files: usize,
}

fn step_streaming_extract(prot_dir: &Path, out: &Path, verbose: bool) -> Result<usize> {
    let mut hits = 0usize;
    for entry in std::fs::read_dir(prot_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) if n.ends_with(".BIN") => n.to_string(),
            _ => continue,
        };
        let buf = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let report = match parse_streaming(&buf, 4096) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if !(report.terminated
            && report.all_known_types
            && report.all_magic_ok
            && report.chunks.len() >= 2)
        {
            continue;
        }
        // Real streaming hit — extract sub-assets.
        let stem = name.trim_end_matches(".BIN").to_string();
        let dest = out.join(&stem);
        std::fs::create_dir_all(&dest)?;
        for (i, chunk) in report.chunks.iter().enumerate() {
            let t = AssetType::from_byte(chunk.type_byte);
            let chunk_dir = dest.join(format!("chunk{:02}_{}", i, t.name()));
            std::fs::create_dir_all(&chunk_dir)?;
            let data_start = chunk.header_offset + 4;
            let data_end = data_start + chunk.size as usize;
            if data_end > buf.len() {
                continue;
            }
            let chunk_data = &buf[data_start..data_end];
            // TIM_LIST and TMD use the inner pack format; everything else
            // gets written as a single blob.
            match t {
                AssetType::TimList | AssetType::Tmd | AssetType::Tmd2 => {
                    if let Ok(items) = pack::extract_pack(chunk_data) {
                        for (j, item) in items.iter().enumerate() {
                            let ext = match t {
                                AssetType::TimList => "tim",
                                _ => "tmd",
                            };
                            std::fs::write(chunk_dir.join(format!("{:04}.{}", j, ext)), item)?;
                        }
                    } else {
                        // fall back to single blob
                        std::fs::write(chunk_dir.join("blob.bin"), chunk_data)?;
                    }
                }
                _ => {
                    std::fs::write(chunk_dir.join("blob.bin"), chunk_data)?;
                }
            }
        }
        // Also dump the trailer (post-terminator bytes) for downstream analysis.
        if report.bytes_consumed < buf.len() {
            std::fs::write(dest.join("_trailer.bin"), &buf[report.bytes_consumed..])?;
        }
        if verbose {
            println!("    [stream] {} → {} chunks", name, report.chunks.len());
        }
        hits += 1;
    }
    Ok(hits)
}

fn step_tim_to_png(stream_dir: &Path, verbose: bool) -> Result<usize> {
    if !stream_dir.exists() {
        return Ok(0);
    }
    let mut count = 0usize;
    for entry in walk(stream_dir)? {
        if entry
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.eq_ignore_ascii_case("tim"))
            != Some(true)
        {
            continue;
        }
        let buf = match std::fs::read(&entry) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let tim = match legaia_tim::parse(&buf) {
            Ok(t) => t,
            Err(_) => continue,
        };
        // Use CLUT 0 for the default rendering; users can re-run the
        // standalone `tim` binary with --all-cluts for the full set.
        let rgba = match legaia_tim::decode_rgba8(&tim, 0) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let png_path = entry.with_extension("png");
        let w = tim.pixel_width();
        let h = tim.pixel_height();
        if w == 0 || h == 0 {
            continue;
        }
        if legaia_tim::write_png(&png_path, w, h, &rgba).is_ok() {
            if verbose {
                println!("    [png] {}", png_path.display());
            }
            count += 1;
        }
    }
    Ok(count)
}

fn walk(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    Ok(out)
}
