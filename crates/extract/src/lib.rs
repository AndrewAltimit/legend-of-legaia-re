//! The full Legaia extraction pipeline as a library: disc → ISO9660 files →
//! PROT entries → categorize → streaming sub-assets → PNG → CD-XA WAV → TIM
//! catalogs → dialog font.
//!
//! [`run`] drives every step in order and reports through a [`Reporter`],
//! which is how both front ends see the same pipeline: the `legaia-extract`
//! command line prints the messages, and `legaia-studio` turns them into a
//! progress bar and a log pane and can cancel between (and inside) steps.
//!
//! Each step is a thin loop over the per-format library crates; no format is
//! decoded here.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result, bail};
use legaia_container_formats::{AssetType, pack, parse_streaming};
use legaia_iso::{
    iso9660,
    raw::{RawDisc, USER_DATA_SIZE},
    region,
};
use legaia_prot::{archive::Archive, cdname};
use rayon::prelude::*;

/// Number of pipeline steps a [`Reporter`] can see: verification is step 0,
/// the eight standard steps are 1..=8, and the optional every-texture sweep
/// ([`Options::all_textures`]) is step 9.
pub const STEP_COUNT: usize = 9;

/// What to run. Mirrors the `legaia-extract` flags one for one.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Disc image: a raw Mode2/2352 `.bin`, or its `.cue` sheet.
    pub bin: PathBuf,
    /// Output directory; created if missing, existing files overwritten.
    pub out: PathBuf,
    pub skip_verify: bool,
    pub skip_png: bool,
    pub skip_xa: bool,
    pub skip_catalog: bool,
    pub skip_font: bool,
    /// Also sweep every PROT entry for TIMs (raw and inside LZS sections)
    /// and write each as `.tim` + `.png` under `tim_scan/` - what
    /// `asset tim-scan` followed by `tim convert-dir` produce.
    pub all_textures: bool,
    /// Report one [`Reporter::detail`] line per file written.
    pub verbose: bool,
}

/// Where the pipeline sends what it is doing. Every method has a default so
/// a front end implements only what it shows. `Sync` because the parallel
/// steps report from rayon workers.
pub trait Reporter: Sync {
    /// A pipeline-level status line.
    fn log(&self, msg: &str);
    /// A per-file line; only sent when [`Options::verbose`] is set.
    fn detail(&self, msg: &str) {
        self.log(msg);
    }
    /// A non-fatal problem (a skipped channel, a failed optional step).
    fn warn(&self, msg: &str) {
        self.log(msg);
    }
    /// Step `index` of [`STEP_COUNT`] is starting (`0` = verification).
    fn step(&self, _index: usize, _name: &str) {}
    /// `done` of `total` units of the current step are finished.
    fn progress(&self, _done: usize, _total: usize) {}
    /// Polled between units of work; `true` stops the run with [`Cancelled`].
    fn cancelled(&self) -> bool {
        false
    }
}

/// The error [`run`] returns when [`Reporter::cancelled`] asked it to stop.
/// Output written before the stop stays on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("extraction cancelled")
    }
}

impl std::error::Error for Cancelled {}

fn check(r: &dyn Reporter) -> Result<()> {
    if r.cancelled() {
        return Err(Cancelled.into());
    }
    Ok(())
}

/// Counts from a finished run, for a summary line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    pub iso_files: usize,
    pub prot_entries: usize,
    pub classified: usize,
    pub streams: usize,
    pub pngs: usize,
    pub wavs: usize,
    pub catalog_tims: (usize, usize),
    pub font_written: bool,
    /// TIMs written by the every-texture sweep (`(tims, pngs)`).
    pub all_textures: (usize, usize),
}

/// Run the whole pipeline. Fails fast (before any hashing) when `opts.bin`
/// is not a Mode2/2352 disc image.
pub fn run(opts: &Options, r: &dyn Reporter) -> Result<Summary> {
    if !opts.bin.exists() {
        bail!("disc image not found: {}", opts.bin.display());
    }
    RawDisc::open(&opts.bin)
        .with_context(|| format!("opening disc image {}", opts.bin.display()))?;
    std::fs::create_dir_all(&opts.out)
        .with_context(|| format!("creating output dir {}", opts.out.display()))?;

    let mut sum = Summary::default();
    let out = &opts.out;
    r.log(&format!("bin: {}", opts.bin.display()));
    r.log(&format!("out: {}", out.display()));

    r.step(0, "verify");
    if !opts.skip_verify {
        verify(&opts.bin, r)?;
    } else {
        r.log("verify: skipped");
    }

    check(r)?;
    r.step(1, "disc → ISO9660 files");
    r.log("step 1/8: disc → ISO9660 files");
    sum.iso_files = step_disc_extract(&opts.bin, out, opts.verbose, r)?;
    r.log(&format!("  {} files extracted", sum.iso_files));

    check(r)?;
    r.step(2, "PROT.DAT → named entries");
    r.log("step 2/8: PROT.DAT → named entries");
    let prot_dir = out.join("PROT");
    let cdname_path = out.join("CDNAME.TXT");
    let cdname_arg = cdname_path.exists().then_some(cdname_path.as_path());
    sum.prot_entries = step_prot_extract(
        &out.join("PROT.DAT"),
        &prot_dir,
        cdname_arg,
        opts.verbose,
        r,
    )?;
    r.log(&format!(
        "  {} PROT entries written to {}",
        sum.prot_entries,
        prot_dir.display()
    ));

    check(r)?;
    r.step(3, "categorize PROT entries");
    r.log("step 3/8: categorize PROT entries");
    let cat_path = prot_dir.join("categorize.json");
    sum.classified = step_categorize(&prot_dir, &cat_path, r)?;
    r.log(&format!(
        "  {} files classified → {}",
        sum.classified,
        cat_path.display()
    ));

    check(r)?;
    r.step(4, "streaming sub-assets");
    r.log("step 4/8: extract sub-assets from streaming-format entries");
    let stream_dir = out.join("streaming");
    sum.streams = step_streaming_extract(&prot_dir, &stream_dir, opts.verbose, r)?;
    r.log(&format!("  {} streaming containers expanded", sum.streams));

    check(r)?;
    r.step(5, "TIM → PNG");
    if opts.skip_png {
        r.log("step 5/8: streaming-container TIMs → PNG (skipped via --skip-png)");
    } else {
        r.log("step 5/8: streaming-container TIMs → PNG");
        sum.pngs = step_tim_to_png(&stream_dir, opts.verbose, r)?;
        r.log(&format!(
            "  {} PNG images written under {} (only the TIMs step 4 emitted; the \
             full texture inventory comes from the TIM catalogs in step 7)",
            sum.pngs,
            stream_dir.display()
        ));
    }

    check(r)?;
    r.step(6, "CD-XA → WAV");
    if opts.skip_xa {
        r.log("step 6/8: CD-XA demux → WAV (skipped via --skip-xa)");
    } else {
        r.log("step 6/8: CD-XA demux → per-channel WAV");
        let xa_dir = out.join("XA_WAV");
        sum.wavs = step_xa_demux(&opts.bin, &xa_dir, opts.verbose, r)?;
        r.log(&format!(
            "  {} WAVs written under {}",
            sum.wavs,
            xa_dir.display()
        ));
    }

    check(r)?;
    r.step(7, "TIM catalog → TSV");
    if opts.skip_catalog {
        r.log("step 7/8: TIM catalog → TSV (skipped via --skip-catalog)");
    } else {
        r.log("step 7/8: TIM catalog → TSV");
        sum.catalog_tims = step_tim_catalog(&out.join("PROT.DAT"), out)?;
        r.log(&format!(
            "  {} raw + {} compressed TIMs → prot_tim_catalog.tsv / prot_tim_deep_catalog.tsv",
            sum.catalog_tims.0, sum.catalog_tims.1
        ));
    }

    check(r)?;
    r.step(8, "dialog font");
    if opts.skip_font {
        r.log("step 8/8: dialog font → font/ (skipped via --skip-font)");
    } else {
        r.log("step 8/8: dialog font → font/");
        // The engine and asset-viewer load dialog text from these artifacts;
        // a failure here degrades text rendering but must not abort an
        // otherwise-good extraction, so it's a warning.
        match step_font_export(out) {
            Ok(font_dir) => {
                sum.font_written = true;
                r.log(&format!(
                    "  5 dialog-font artifacts written to {}",
                    font_dir.display()
                ))
            }
            Err(e) => r.warn(&format!(
                "  [warn] dialog-font build failed ({:#}); engine text will fall \
                 back to a placeholder font",
                e
            )),
        }
    }

    if opts.all_textures {
        check(r)?;
        r.step(9, "every texture → PNG");
        r.log("extra: every TIM in every PROT entry → tim_scan/ (.tim + .png)");
        let dir = out.join("tim_scan");
        sum.all_textures = step_all_textures(&prot_dir, &dir, opts.verbose, r)?;
        r.log(&format!(
            "  {} TIMs ({} PNGs) written under {}",
            sum.all_textures.0,
            sum.all_textures.1,
            dir.display()
        ));
    }

    r.log("done");
    let o = out.display();
    r.log(&format!(
        "textures: inventoried in {o}/prot_tim_catalog.tsv (+ prot_tim_deep_catalog.tsv); \
         export them with `asset tim-scan {o}/PROT --out {o}/tim_scan` and convert to \
         PNG with `tim convert-dir {o}/tim_scan`"
    ));
    Ok(sum)
}

/// The every-texture sweep: [`legaia_container_formats::tim_scan`] over each
/// PROT entry (raw bytes and LZS sections), each hit written as
/// `<entry>/<raw|lzsN>_off<HEX6>_<W>x<H>_<BPP>bpp.tim` - the layout and names
/// `asset tim-scan --out` uses - plus a `.png` next to it decoded with CLUT 0,
/// as `tim convert-dir` writes. Returns `(tims, pngs)`.
fn step_all_textures(
    prot_dir: &Path,
    out: &Path,
    verbose: bool,
    r: &dyn Reporter,
) -> Result<(usize, usize)> {
    use legaia_container_formats::tim_scan;
    let paths = bin_files(prot_dir)?;
    let total = paths.len();
    std::fs::create_dir_all(out)?;
    let done = AtomicUsize::new(0);
    let tims = AtomicUsize::new(0);
    let pngs = AtomicUsize::new(0);
    paths.par_iter().for_each(|(path, name)| {
        if r.cancelled() {
            return;
        }
        r.progress(done.fetch_add(1, Ordering::Relaxed) + 1, total);
        let Ok(raw) = std::fs::read(path) else {
            return;
        };
        let scan = tim_scan::scan_entry(&raw);
        if scan.hits.is_empty() {
            return;
        }
        let entry_dir = out.join(name.trim_end_matches(".BIN"));
        if std::fs::create_dir_all(&entry_dir).is_err() {
            return;
        }
        for (src, hit) in &scan.hits {
            let (buf, label) = match src {
                tim_scan::Source::Raw => (raw.as_slice(), "raw".to_string()),
                tim_scan::Source::Lzs(idx) => match scan.lzs_sections.get(*idx) {
                    Some(sec) => (sec.as_slice(), format!("lzs{idx}")),
                    None => continue,
                },
            };
            let end = (hit.offset + hit.byte_len).min(buf.len());
            let slab = &buf[hit.offset..end];
            let stem = format!(
                "{}_off{:06X}_{}x{}_{}bpp",
                label, hit.offset, hit.width, hit.height, hit.bpp
            );
            let tim_path = entry_dir.join(format!("{stem}.tim"));
            if std::fs::write(&tim_path, slab).is_err() {
                continue;
            }
            tims.fetch_add(1, Ordering::Relaxed);
            let Ok(tim) = legaia_tim::parse(slab) else {
                continue;
            };
            let (w, h) = (tim.pixel_width(), tim.pixel_height());
            if w == 0 || h == 0 {
                continue;
            }
            if let Ok(rgba) = legaia_tim::decode_rgba8(&tim, 0)
                && legaia_tim::write_png(&tim_path.with_extension("png"), w, h, &rgba).is_ok()
            {
                pngs.fetch_add(1, Ordering::Relaxed);
                if verbose {
                    r.detail(&format!("    [tim] {}", tim_path.display()));
                }
            }
        }
    });
    check(r)?;
    Ok((tims.load(Ordering::Relaxed), pngs.load(Ordering::Relaxed)))
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
    let raw = legaia_disc_audit::tim_catalog::build_from_path(prot)?;
    std::fs::write(
        out.join("prot_tim_catalog.tsv"),
        legaia_disc_audit::tim_catalog::to_tsv(&raw),
    )?;
    let deep = legaia_disc_audit::tim_deep_catalog::build_from_path(prot)?;
    std::fs::write(
        out.join("prot_tim_deep_catalog.tsv"),
        legaia_disc_audit::tim_deep_catalog::to_tsv(&deep),
    )?;
    Ok((raw.len(), deep.len()))
}

/// SHA-256 the image and report the region from `SYSTEM.CNF`.
pub fn verify(bin: &Path, r: &dyn Reporter) -> Result<()> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(bin)?;
    let total = file.metadata().map(|m| m.len() as usize).unwrap_or(0);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    let mut done = 0usize;
    loop {
        check(r)?;
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        done += n;
        r.progress(done, total);
    }
    let hash = format!("{:x}", hasher.finalize());
    r.log(&format!("verify: sha256 = {}", hash));
    if let Some(d) = disc_region(bin) {
        r.log(&format!(
            "verify: region = {} (executable={}, prefix={})",
            d.region.name(),
            d.executable,
            d.prefix
        ));
    }
    Ok(())
}

/// Region data parsed from the image's `SYSTEM.CNF`, if it has one.
pub fn disc_region(bin: &Path) -> Option<region::DetectedRegion> {
    let mut disc = RawDisc::open(bin).ok()?;
    let vol = iso9660::read_volume(&mut disc).ok()?;
    let files = iso9660::walk_files(&mut disc, &vol.root).ok()?;
    let (_, entry) = files
        .iter()
        .find(|(p, _)| p.eq_ignore_ascii_case("SYSTEM.CNF"))?;
    let mut sysbuf = Vec::new();
    let n = entry.size.div_ceil(USER_DATA_SIZE as u32);
    disc.read_user_data(entry.lba, n, &mut sysbuf).ok()?;
    sysbuf.truncate(entry.size as usize);
    region::parse(&sysbuf).ok()
}

fn step_disc_extract(bin: &Path, out: &Path, verbose: bool, r: &dyn Reporter) -> Result<usize> {
    let mut disc = RawDisc::open(bin)?;
    let vol = iso9660::read_volume(&mut disc)?;
    let files = iso9660::walk_files(&mut disc, &vol.root)?;
    let mut buf = Vec::new();
    for (i, (path, entry)) in files.iter().enumerate() {
        check(r)?;
        let full = out.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let n = entry.size.div_ceil(USER_DATA_SIZE as u32);
        disc.read_user_data(entry.lba, n, &mut buf)?;
        buf.truncate(entry.size as usize);
        std::fs::write(&full, &buf)?;
        if verbose {
            r.detail(&format!("    [disc] {:>10}  {}", entry.size, path));
        }
        r.progress(i + 1, files.len());
    }
    Ok(files.len())
}

fn step_prot_extract(
    prot: &Path,
    out: &Path,
    cdname_path: Option<&Path>,
    verbose: bool,
    r: &dyn Reporter,
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
    for (i, entry) in entries.iter().enumerate() {
        check(r)?;
        archive.read_entry(entry, &mut buf)?;
        let bin_name = format!("{}.BIN", entry_stem(names.as_ref(), entry.index));
        std::fs::write(out.join(&bin_name), &buf)?;
        if verbose {
            r.detail(&format!("    [prot] {:>10}  {}", buf.len(), bin_name));
        }
        r.progress(i + 1, entries.len());
    }
    Ok(entries.len())
}

/// The file stem `prot-extract` / `legaia-extract` give PROT entry `index`:
/// `NNNN_<cdname block>`, or bare `NNNN` without a name map. The label is
/// the `#define` number applied as an extraction index (the long-standing
/// filename convention, shifted +2 from the retail reading - see
/// [`cdname::block_for_extraction_index`]).
pub fn entry_stem(names: Option<&cdname::IndexMap>, index: u32) -> String {
    match names.and_then(|m| cdname::block_for(m, index)) {
        Some(b) => format!("{:04}_{}", index, b),
        None => format!("{:04}", index),
    }
}

/// Collect `*.BIN` files in `dir`, sorted.
fn bin_files(dir: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut v: Vec<(PathBuf, String)> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?.to_string();
            (p.is_file() && name.ends_with(".BIN")).then_some((p, name))
        })
        .collect();
    v.sort();
    Ok(v)
}

fn step_categorize(dir: &Path, out: &Path, r: &dyn Reporter) -> Result<usize> {
    use legaia_disc_audit::categorize::classify;

    let paths = bin_files(dir)?;
    let total = paths.len();
    let done = AtomicUsize::new(0);

    // Classify in parallel; skip entries that fail to read or parse.
    let results: Vec<(String, serde_json::Value)> = paths
        .par_iter()
        .filter_map(|(path, name)| {
            if r.cancelled() {
                return None;
            }
            let buf = std::fs::read(path).ok()?;
            let report = classify(&buf);
            let val = serde_json::to_value(&report).ok()?;
            r.progress(done.fetch_add(1, Ordering::Relaxed) + 1, total);
            Some((name.clone(), val))
        })
        .collect();
    check(r)?;

    let n_files = results.len();
    // `per_file` is a LIST of records that each carry their own `path`, matching
    // what `asset categorize --out` writes. The two emitters used to disagree -
    // this one wrote a filename -> record MAP - so `categorize.json`'s schema
    // depended on which tool had written it last, and a consumer written against
    // one shape crashed on the other (`disc-coverage.py` iterating a map yields
    // its keys: `'str' object has no attribute 'get'`). That stayed hidden for
    // as long as nobody re-ran the extractor.
    let mut per_file: Vec<serde_json::Value> = results
        .into_iter()
        .map(|(name, mut val)| {
            if let Some(obj) = val.as_object_mut() {
                obj.insert("path".to_string(), serde_json::Value::String(name));
            }
            val
        })
        .collect();
    per_file.sort_by(|a, b| {
        a.get("path")
            .and_then(|v| v.as_str())
            .cmp(&b.get("path").and_then(|v| v.as_str()))
    });
    let summary = serde_json::json!({
        "scan_root": dir.display().to_string(),
        "n_files": n_files,
        "per_file": per_file,
    });
    std::fs::write(out, serde_json::to_string_pretty(&summary)?)?;
    Ok(n_files)
}

fn step_streaming_extract(
    prot_dir: &Path,
    out: &Path,
    verbose: bool,
    r: &dyn Reporter,
) -> Result<usize> {
    let paths = bin_files(prot_dir)?;
    let total = paths.len();

    // Ensure output root exists before spawning threads.
    std::fs::create_dir_all(out)?;

    let hits = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);

    paths.par_iter().for_each(|(path, name)| {
        if r.cancelled() {
            return;
        }
        r.progress(done.fetch_add(1, Ordering::Relaxed) + 1, total);
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
            r.detail(&format!(
                "    [stream] {} → {} chunks",
                name,
                report.chunks.len()
            ));
        }
        hits.fetch_add(1, Ordering::Relaxed);
    });
    check(r)?;

    Ok(hits.load(Ordering::Relaxed))
}

fn step_tim_to_png(stream_dir: &Path, verbose: bool, r: &dyn Reporter) -> Result<usize> {
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
    let total = tim_files.len();

    let count = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);

    // Decode + write PNGs in parallel. Each entry writes to its own path.
    tim_files.par_iter().for_each(|entry| {
        if r.cancelled() {
            return;
        }
        r.progress(done.fetch_add(1, Ordering::Relaxed) + 1, total);
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
                r.detail(&format!("    [png] {}", png_path.display()));
            }
            count.fetch_add(1, Ordering::Relaxed);
        }
    });
    check(r)?;

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
fn step_xa_demux(bin: &Path, out: &Path, verbose: bool, r: &dyn Reporter) -> Result<usize> {
    use legaia_xa::{Channels, DecodeOptions};

    let files = legaia_xa::demux::demux_disc_all(bin)
        .with_context(|| format!("demux all XA on {}", bin.display()))?;
    std::fs::create_dir_all(out)?;
    let total: usize = files.iter().map(|f| f.streams.len()).sum();
    let mut done = 0usize;
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
            check(r)?;
            done += 1;
            r.progress(done, total);
            let bits = match s.bits_per_sample {
                4 => legaia_xa::BitsPerSample::Four,
                8 => legaia_xa::BitsPerSample::Eight,
                other => {
                    r.warn(&format!(
                        "    [xa] {}_file{}_ch{}: SKIPPED ({other}-bit ADPCM unsupported, 4/8-bit only)",
                        stem, s.file_no, s.ch_no
                    ));
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
                r.detail(&format!(
                    "    [xa] {:>5}Hz {:<6} {:.2}s → {}",
                    s.sample_rate,
                    if s.stereo { "stereo" } else { "mono" },
                    dur,
                    path.display()
                ));
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
