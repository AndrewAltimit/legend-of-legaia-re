//! Unified Legaia extraction pipeline. Runs disc → PROT → categorize →
//! streaming-format extract → TIM-to-PNG in one shot.
//!
//! Wraps the per-crate library APIs; equivalent to running `disc-extract
//! extract`, `prot-extract extract`, `asset scan-stream`/`extract`, and
//! `tim convert-dir` in sequence, but with one CLI and one output tree.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result, bail};
use clap::Parser;
use legaia_asset::{AssetType, pack, parse_streaming};
use legaia_iso::{
    iso9660,
    raw::{RawDisc, USER_DATA_SIZE},
    region,
};
use legaia_prot::{archive::Archive, cdname};
use rayon::prelude::*;

#[derive(Parser)]
#[command(
    name = "legaia-extract",
    version,
    about = "Run the full Legaia extraction pipeline (disc → PROT → categorize → sub-assets → PNG → XA → font)"
)]
struct Cli {
    /// Legend of Legaia (USA) disc image: a raw Mode2/2352 `.bin` dump, or
    /// its `.cue` sheet (the referenced BINARY track is resolved
    /// automatically).
    bin: PathBuf,
    /// Output directory (resolved against the current directory when
    /// relative). Created if missing. Existing files are overwritten.
    #[arg(long, default_value = "extracted")]
    out: PathBuf,
    /// Skip the disc verification step (don't compute SHA-256).
    #[arg(long)]
    skip_verify: bool,
    /// Skip converting the streaming-container TIMs to PNG (a quick step;
    /// the bulk texture inventory is the TIM-catalog TSVs, see
    /// --skip-catalog).
    #[arg(long)]
    skip_png: bool,
    /// Skip CD-XA demux → per-channel WAV (the streamed-audio step).
    #[arg(long)]
    skip_xa: bool,
    /// Skip writing the TIM-catalog TSVs (the texture inventory step).
    #[arg(long)]
    skip_catalog: bool,
    /// Skip building the dialog-font artifacts (`font/` dir the engine and
    /// asset-viewer load text from).
    #[arg(long)]
    skip_font: bool,
    /// Print one line per file written.
    #[arg(short, long)]
    verbose: bool,
}

/// Restore default SIGPIPE behaviour so piping into `head` etc. exits
/// quietly instead of panicking on a broken-pipe write.
fn reset_sigpipe() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

fn main() -> Result<()> {
    reset_sigpipe();
    let cli = Cli::parse();
    if !cli.bin.exists() {
        bail!("disc image not found: {}", cli.bin.display());
    }
    // Fail fast (with the path and what was wrong) on inputs that aren't
    // Mode2/2352 disc images, before any hashing / extraction starts.
    RawDisc::open(&cli.bin).with_context(|| format!("opening disc image {}", cli.bin.display()))?;
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

    log("step 1/8: disc → ISO9660 files");
    let n = step_disc_extract(&cli.bin, &cli.out, cli.verbose)?;
    log(&format!("  {} files extracted", n));

    log("step 2/8: PROT.DAT → named entries");
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

    log("step 3/8: categorize PROT entries");
    let cat_path = prot_dir.join("categorize.json");
    let report = step_categorize(&prot_dir, &cat_path)?;
    log(&format!(
        "  {} files classified → {}",
        report.n_files,
        cat_path.display()
    ));

    log("step 4/8: extract sub-assets from streaming-format entries");
    let stream_dir = cli.out.join("streaming");
    let n_streams = step_streaming_extract(&prot_dir, &stream_dir, cli.verbose)?;
    log(&format!("  {} streaming containers expanded", n_streams));

    if cli.skip_png {
        log("step 5/8: streaming-container TIMs → PNG (skipped via --skip-png)");
    } else {
        log("step 5/8: streaming-container TIMs → PNG");
        let n_png = step_tim_to_png(&stream_dir, cli.verbose)?;
        log(&format!(
            "  {} PNG images written under {} (only the TIMs step 4 emitted; the \
             full texture inventory comes from the TIM catalogs in step 7)",
            n_png,
            stream_dir.display()
        ));
    }

    if cli.skip_xa {
        log("step 6/8: CD-XA demux → WAV (skipped via --skip-xa)");
    } else {
        log("step 6/8: CD-XA demux → per-channel WAV");
        let xa_dir = cli.out.join("XA_WAV");
        let n_wav = step_xa_demux(&cli.bin, &xa_dir, cli.verbose)?;
        log(&format!(
            "  {} WAVs written under {}",
            n_wav,
            xa_dir.display()
        ));
    }

    if cli.skip_catalog {
        log("step 7/8: TIM catalog → TSV (skipped via --skip-catalog)");
    } else {
        log("step 7/8: TIM catalog → TSV");
        let (raw_n, deep_n) = step_tim_catalog(&cli.out.join("PROT.DAT"), &cli.out)?;
        log(&format!(
            "  {} raw + {} compressed TIMs → prot_tim_catalog.tsv / prot_tim_deep_catalog.tsv",
            raw_n, deep_n
        ));
    }

    if cli.skip_font {
        log("step 8/8: dialog font → font/ (skipped via --skip-font)");
    } else {
        log("step 8/8: dialog font → font/");
        // The engine and asset-viewer load dialog text from these artifacts;
        // a failure here degrades text rendering but must not abort an
        // otherwise-good extraction, so it's a warning.
        match step_font_export(&cli.out) {
            Ok(font_dir) => log(&format!(
                "  5 dialog-font artifacts written to {}",
                font_dir.display()
            )),
            Err(e) => log(&format!(
                "  [warn] dialog-font build failed ({:#}); engine text will fall \
                 back to a placeholder font",
                e
            )),
        }
    }

    log("done");
    let out = cli.out.display();
    log(&format!(
        "textures: inventoried in {out}/prot_tim_catalog.tsv (+ prot_tim_deep_catalog.tsv); \
         export them with `asset tim-scan {out}/PROT --out {out}/tim_scan` and convert to \
         PNG with `tim convert-dir {out}/tim_scan`"
    ));
    Ok(())
}

/// Step 8: build the `font/` artifact set (atlas + sheet PNGs, widths CSV,
/// metadata JSON, raw 4bpp page) straight from the extracted `PROT.DAT` +
/// `SCUS_942.54` - no emulator save state needed. This is the directory
/// `legaia-engine` / `asset-viewer` load dialog text from
/// (`Font::load_from_extracted`). Returns the font dir path.
fn step_font_export(out: &Path) -> Result<PathBuf> {
    let prot = out.join("PROT.DAT");
    let scus = out.join("SCUS_942.54");
    let prot_bytes = std::fs::read(&prot).with_context(|| format!("reading {}", prot.display()))?;
    let scus_bytes = std::fs::read(&scus).with_context(|| format!("reading {}", scus.display()))?;
    let off = legaia_font::FONT_TIM_PROT_DAT_OFFSET as usize;
    let end = off + legaia_font::FONT_TIM_LEN;
    let tim = prot_bytes
        .get(off..end)
        .with_context(|| format!("PROT.DAT too short for font TIM at 0x{off:X}"))?;
    let font_dir = out.join("font");
    legaia_font::export_extracted_font_dir(tim, &scus_bytes, &font_dir)?;
    Ok(font_dir)
}

/// Write the flat and deep TIM catalogs as TSVs into the extract root, so a
/// headless extract carries the full texture inventory. These mirror the
/// committed reference catalogs (metadata + FNV fingerprints only - no pixel
/// bytes). Returns `(raw_count, deep_count)`.
fn step_tim_catalog(prot: &Path, out: &Path) -> Result<(usize, usize)> {
    if !prot.exists() {
        bail!("PROT.DAT not found at {}", prot.display());
    }
    let raw = legaia_asset::tim_catalog::build_from_path(prot)?;
    std::fs::write(
        out.join("prot_tim_catalog.tsv"),
        legaia_asset::tim_catalog::to_tsv(&raw),
    )?;
    let deep = legaia_asset::tim_deep_catalog::build_from_path(prot)?;
    std::fs::write(
        out.join("prot_tim_deep_catalog.tsv"),
        legaia_asset::tim_deep_catalog::to_tsv(&deep),
    )?;
    Ok((raw.len(), deep.len()))
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

    // Collect .BIN paths first so we can hand them to rayon.
    let paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.ends_with(".BIN"))
                    .unwrap_or(false)
        })
        .collect();

    // Classify in parallel; skip entries that fail to read or parse.
    let results: Vec<(String, serde_json::Value)> = paths
        .par_iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?.to_string();
            let buf = std::fs::read(path).ok()?;
            let report = classify(&buf);
            let val = serde_json::to_value(&report).ok()?;
            Some((name, val))
        })
        .collect();

    let n_files = results.len();
    let mut per_file = serde_json::Map::new();
    for (name, val) in results {
        per_file.insert(name, val);
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
    // Collect candidate .BIN paths up front.
    let paths: Vec<(PathBuf, String)> = std::fs::read_dir(prot_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?.to_string();
            if p.is_file() && name.ends_with(".BIN") {
                Some((p, name))
            } else {
                None
            }
        })
        .collect();

    // Ensure output root exists before spawning threads.
    std::fs::create_dir_all(out)?;

    let hits = AtomicUsize::new(0);

    paths.par_iter().for_each(|(path, name)| {
        let buf = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => return,
        };
        let report = match parse_streaming(&buf, 4096) {
            Ok(r) => r,
            Err(_) => return,
        };
        if !(report.terminated
            && report.all_known_types
            && report.all_magic_ok
            && report.chunks.len() >= 2)
        {
            return;
        }

        let stem = name.trim_end_matches(".BIN");
        let dest = out.join(stem);
        if std::fs::create_dir_all(&dest).is_err() {
            return;
        }

        for (i, chunk) in report.chunks.iter().enumerate() {
            let t = AssetType::from_byte(chunk.type_byte);
            let chunk_dir = dest.join(format!("chunk{:02}_{}", i, t.name()));
            if std::fs::create_dir_all(&chunk_dir).is_err() {
                continue;
            }
            let data_start = chunk.header_offset + 4;
            let data_end = data_start + chunk.size as usize;
            if data_end > buf.len() {
                continue;
            }
            let chunk_data = &buf[data_start..data_end];
            match t {
                AssetType::TimList | AssetType::Tmd | AssetType::Tmd2 => {
                    if let Ok(items) = pack::extract_pack(chunk_data) {
                        for (j, item) in items.iter().enumerate() {
                            let ext = match t {
                                AssetType::TimList => "tim",
                                _ => "tmd",
                            };
                            let _ =
                                std::fs::write(chunk_dir.join(format!("{:04}.{}", j, ext)), item);
                        }
                    } else {
                        let _ = std::fs::write(chunk_dir.join("blob.bin"), chunk_data);
                    }
                }
                _ => {
                    let _ = std::fs::write(chunk_dir.join("blob.bin"), chunk_data);
                }
            }
        }

        if report.bytes_consumed < buf.len() {
            let _ = std::fs::write(dest.join("_trailer.bin"), &buf[report.bytes_consumed..]);
        }
        if verbose {
            println!("    [stream] {} → {} chunks", name, report.chunks.len());
        }
        hits.fetch_add(1, Ordering::Relaxed);
    });

    Ok(hits.load(Ordering::Relaxed))
}

fn step_tim_to_png(stream_dir: &Path, verbose: bool) -> Result<usize> {
    if !stream_dir.exists() {
        return Ok(0);
    }

    // Collect .tim files first; filtering is cheap and sequential.
    let tim_files: Vec<PathBuf> = walk(stream_dir)?
        .into_iter()
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("tim"))
        })
        .collect();

    let count = AtomicUsize::new(0);

    // Decode + write PNGs in parallel. Each entry writes to its own path.
    tim_files.par_iter().for_each(|entry| {
        let buf = match std::fs::read(entry) {
            Ok(b) => b,
            Err(_) => return,
        };
        let tim = match legaia_tim::parse(&buf) {
            Ok(t) => t,
            Err(_) => return,
        };
        // CLUT 0 for default rendering; --all-cluts via the standalone `tim`
        // binary covers the full palette set.
        let rgba = match legaia_tim::decode_rgba8(&tim, 0) {
            Ok(r) => r,
            Err(_) => return,
        };
        let w = tim.pixel_width();
        let h = tim.pixel_height();
        if w == 0 || h == 0 {
            return;
        }
        let png_path = entry.with_extension("png");
        if legaia_tim::write_png(&png_path, w, h, &rgba).is_ok() {
            if verbose {
                println!("    [png] {}", png_path.display());
            }
            count.fetch_add(1, Ordering::Relaxed);
        }
    });

    Ok(count.load(Ordering::Relaxed))
}

/// Demux every `*.XA` file on the disc into correctly-paced per-channel WAVs.
///
/// The disc-extract step copies each `XA/*.XA` file as Form-1 user data (2048
/// B/sector), which truncates the Form-2 audio sectors and collapses the file's
/// multiplexed channels into one shuffled byte stream - those raw dumps are not
/// listenable. This step instead reads the raw 2352-byte sectors straight off
/// the disc image, splits them by `(file_no, ch_no)` via
/// [`legaia_xa::demux::demux_disc_all`], and decodes each channel at its true
/// per-sector rate / stereo mode, so the WAVs are reference-quality. Non-4-bit
/// channels are skipped with a warning (the group decoder is 4-bit only) rather
/// than mis-decoded.
fn step_xa_demux(bin: &Path, out: &Path, verbose: bool) -> Result<usize> {
    use legaia_xa::{Channels, DecodeOptions};

    let files = legaia_xa::demux::demux_disc_all(bin)
        .with_context(|| format!("demux all XA on {}", bin.display()))?;
    std::fs::create_dir_all(out)?;
    let mut written = 0usize;
    for f in &files {
        // Output stem from the on-disc filename (e.g. `XA/XA1.XA` -> `XA1`).
        let base = f.path.rsplit('/').next().unwrap_or(&f.path);
        let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(base);
        let stem = if stem.is_empty() {
            format!("lba{}", f.start_lba)
        } else {
            stem.to_string()
        };
        for s in &f.streams {
            let bits = match s.bits_per_sample {
                4 => legaia_xa::BitsPerSample::Four,
                8 => legaia_xa::BitsPerSample::Eight,
                other => {
                    eprintln!(
                        "    [xa] {}_file{}_ch{}: SKIPPED ({other}-bit ADPCM unsupported, 4/8-bit only)",
                        stem, s.file_no, s.ch_no
                    );
                    continue;
                }
            };
            let opts = DecodeOptions {
                channels: if s.stereo {
                    Channels::Stereo
                } else {
                    Channels::Mono
                },
                sample_rate: s.sample_rate,
                bits,
            };
            let (samples, _report) = legaia_xa::decode(&s.audio, opts)?;
            let path = out.join(format!("{stem}_file{}_ch{}.wav", s.file_no, s.ch_no));
            legaia_xa::write_wav(&path, &samples, opts.channels, opts.sample_rate)?;
            if verbose {
                let dur = samples.len() as f64 / opts.sample_rate as f64 / opts.channels.n() as f64;
                println!(
                    "    [xa] {:>5}Hz {:<6} {:.2}s → {}",
                    s.sample_rate,
                    if s.stereo { "stereo" } else { "mono" },
                    dur,
                    path.display()
                );
            }
            written += 1;
        }
    }
    Ok(written)
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
