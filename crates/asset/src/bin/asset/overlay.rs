use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// `jr $ra` opcode (0x03E00008) in little-endian byte order.
pub(crate) const MIPS_JR_RA_LE: [u8; 4] = [0x08, 0x00, 0xE0, 0x03];

/// Test whether a 4-byte instruction word is `addiu $sp, $sp, -N`.
/// Encoding: 0x27BD_FFXX (low byte = -imm). LE bytes: [XX, FF, BD, 27].
pub(crate) fn is_sp_prologue(word: u32) -> bool {
    (word & 0xFFFF_0000) == 0x27BD_0000 && (word & 0x8000) != 0
}

/// Count word-aligned occurrences of `jr $ra`.
pub(crate) fn count_jr_ra(buf: &[u8]) -> usize {
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
pub(crate) fn count_sp_prologue(buf: &[u8]) -> usize {
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
pub(crate) fn code_score(buf: &[u8]) -> f32 {
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

pub(crate) fn find_overlay(dir: &PathBuf, top: usize, lzs_sizes: &str) -> Result<()> {
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

use legaia_asset::static_overlay::{
    self, Eligibility, OverlayForm, OverlayRecord, ghidra_import_driver, ghidra_import_jython,
    overlay_map, recover_base, verify_fingerprint,
};

/// Read one overlay's as-loaded bytes from an already-open archive.
pub(crate) fn overlay_read_as_loaded(
    ar: &mut legaia_prot::archive::Archive,
    rec: &OverlayRecord,
) -> Result<Vec<u8>> {
    let entry = ar
        .entries
        .iter()
        .find(|e| e.index == rec.prot_index)
        .cloned()
        .with_context(|| format!("PROT entry {} not found in archive", rec.prot_index))?;
    let mut buf = Vec::new();
    ar.read_entry(&entry, &mut buf)?;
    static_overlay::as_loaded(&buf, rec)
}

pub(crate) fn overlay_list_cmd(json: bool) -> Result<()> {
    let map = overlay_map();
    if json {
        println!("{}", serde_json::to_string_pretty(&map.overlays)?);
        return Ok(());
    }
    println!(
        "{:>5}  {:<16}  {:<10}  {:<5}  {:<11}  clean_copy",
        "PROT", "label", "base", "form", "eligibility"
    );
    for o in &map.overlays {
        let form = match o.form {
            OverlayForm::Raw => "raw",
            OverlayForm::Lzs => "lzs",
        };
        let elig = match o.eligibility {
            Eligibility::Verified => "verified",
            Eligibility::Static => "static",
            Eligibility::Ineligible => "ineligible",
        };
        let cc = o
            .clean_copy_bytes
            .map(|n| format!("0x{n:x}"))
            .unwrap_or_else(|| "-".into());
        println!(
            "{:>5}  {:<16}  0x{:08X}  {:<5}  {:<11}  {}",
            o.prot_index, o.label, o.base_va, form, elig, cc
        );
    }
    Ok(())
}

pub(crate) fn overlay_extract_cmd(prot_dat: &Path, out: &Path, label: Option<&str>) -> Result<()> {
    let map = overlay_map();
    std::fs::create_dir_all(out)?;
    let mut ar = legaia_prot::archive::Archive::open(prot_dat)?;
    let mut wrote = 0usize;
    for rec in &map.overlays {
        if rec.eligibility == Eligibility::Ineligible {
            continue;
        }
        if label.is_some_and(|want| rec.label != want) {
            continue;
        }
        let bytes = overlay_read_as_loaded(&mut ar, rec)?;
        let path = out.join(rec.bin_filename());
        std::fs::write(&path, &bytes)?;
        println!(
            "[ok] {:<28} PROT {:>4} @ 0x{:08X}  {} bytes",
            rec.bin_filename(),
            rec.prot_index,
            rec.base_va,
            bytes.len()
        );
        wrote += 1;
    }
    println!(
        "[done] extracted {wrote} overlay blob(s) to {}",
        out.display()
    );
    Ok(())
}

pub(crate) fn overlay_verify_cmd(prot_dat: &Path) -> Result<()> {
    let map = overlay_map();
    let mut ar = legaia_prot::archive::Archive::open(prot_dat)?;
    let mut checked = 0usize;
    for rec in &map.overlays {
        if rec.fingerprint_sha256.is_none() {
            continue;
        }
        let bytes = overlay_read_as_loaded(&mut ar, rec)?;
        verify_fingerprint(rec, &bytes)?;
        println!(
            "[ok] {:<16} PROT {:>4} fingerprint reproduces ({} bytes)",
            rec.label,
            rec.prot_index,
            bytes.len()
        );
        checked += 1;
    }
    println!("[done] {checked} overlay fingerprint(s) reproduce from this disc");
    Ok(())
}

pub(crate) fn overlay_ghidra_cmd(out: &Path) -> Result<()> {
    let map = overlay_map();
    std::fs::create_dir_all(out)?;
    for rec in &map.overlays {
        if rec.eligibility == Eligibility::Ineligible {
            continue;
        }
        let script = ghidra_import_jython(rec);
        let path = out.join(format!("import_{}.py", rec.program_name()));
        std::fs::write(&path, script)?;
        println!("[ok] {}", path.display());
    }
    let driver = ghidra_import_driver(map);
    let driver_path = out.join("import_static_overlays.sh");
    std::fs::write(&driver_path, driver)?;
    println!("[ok] {}", driver_path.display());
    Ok(())
}

pub(crate) fn overlay_generate_cmd(prot_dat: &Path, indices: &[u32], min_votes: u32) -> Result<()> {
    let map = overlay_map();
    // Default to refreshing every index already in the committed map.
    let targets: Vec<u32> = if indices.is_empty() {
        map.overlays.iter().map(|o| o.prot_index).collect()
    } else {
        indices.to_vec()
    };
    let mut ar = legaia_prot::archive::Archive::open(prot_dat)?;
    println!("# Generated by `asset overlay generate`. Review before committing.");
    for idx in targets {
        let entry = match ar.entries.iter().find(|e| e.index == idx).cloned() {
            Some(e) => e,
            None => {
                eprintln!("[warn] PROT entry {idx} not found; skipping");
                continue;
            }
        };
        let mut buf = Vec::new();
        ar.read_entry(&entry, &mut buf)?;
        // Generation assumes the raw (uncompressed) as-loaded form; LZS overlays
        // must be filled in by hand with `form = "lzs"` + `decompressed_size`.
        let fp = static_overlay::fingerprint(&buf);
        let existing = map.by_prot_index(idx);
        let label = existing.map(|r| r.label.clone()).unwrap_or_default();
        let recovered = recover_base(&buf, min_votes);
        let base = recovered
            .map(|r| r.base_va)
            .or_else(|| existing.map(|r| r.base_va))
            .unwrap_or(0);
        let votes = recovered.map(|r| r.votes).unwrap_or(0);
        println!();
        println!("[[overlays]]");
        println!("prot_index = {idx}");
        println!("label = \"{label}\"");
        println!("base_va = 0x{base:08X}   # recovered votes={votes}");
        println!("form = \"raw\"");
        println!("eligibility = \"static\"");
        println!("fingerprint_sha256 = \"{fp}\"");
    }
    Ok(())
}

/// Reconnaissance sweep: for each PROT entry in `[from, to]`, recover its base
/// statically, count votes, and print the leading dev string. Not committed -
/// it's how the overlay corpus is triaged into slot-A / slot-B / non-overlay.
pub(crate) fn overlay_scan_cmd(
    prot_dat: &Path,
    from: u32,
    to: u32,
    min_votes: u32,
    base_filter: Option<u32>,
    json: bool,
) -> Result<()> {
    #[derive(serde::Serialize)]
    struct Row {
        prot_index: u32,
        size: usize,
        base_va: Option<u32>,
        votes: u32,
        jal_targets: u32,
        prologues: u32,
        head: Option<String>,
    }
    let mut ar = legaia_prot::archive::Archive::open(prot_dat)?;
    let mut rows: Vec<Row> = Vec::new();
    let mut buf = Vec::new();
    for idx in from..=to {
        let entry = match ar.entries.iter().find(|e| e.index == idx).cloned() {
            Some(e) => e,
            None => continue,
        };
        buf.clear();
        ar.read_entry(&entry, &mut buf)?;
        let rec = recover_base(&buf, min_votes);
        let base = rec.map(|r| r.base_va);
        if let Some(want) = base_filter
            && base != Some(want)
        {
            continue;
        }
        rows.push(Row {
            prot_index: idx,
            size: buf.len(),
            base_va: base,
            votes: rec.map(|r| r.votes).unwrap_or(0),
            jal_targets: rec.map(|r| r.jal_targets).unwrap_or(0),
            prologues: rec.map(|r| r.prologues).unwrap_or(0),
            head: static_overlay::head_string(&buf, 0x800, 5),
        });
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    println!(
        "{:>5}  {:>9}  {:<12}  {:>5}  {:>5}  {:>5}  head",
        "PROT", "size", "base", "votes", "jals", "prol"
    );
    for r in &rows {
        let base = r
            .base_va
            .map(|b| format!("0x{b:08X}"))
            .unwrap_or_else(|| "-".into());
        let head = r.head.as_deref().unwrap_or("");
        let head = if head.len() > 48 { &head[..48] } else { head };
        println!(
            "{:>5}  {:>9}  {:<12}  {:>5}  {:>5}  {:>5}  {}",
            r.prot_index, r.size, base, r.votes, r.jal_targets, r.prologues, head
        );
    }
    Ok(())
}

/// Locate a function-head signature across the corpus, printing the host PROT
/// entry + file offset (and, given the anchor VA, the implied load base). The
/// capture-free way to pin an overlay's entry - the menu-overlay method,
/// generalised into a CLI.
pub(crate) fn overlay_find_sig_cmd(
    prot_dat: &Path,
    sig_hex: &str,
    anchor_va: Option<u32>,
    from: u32,
    to: u32,
) -> Result<()> {
    let hex: String = sig_hex.chars().filter(|c| !c.is_whitespace()).collect();
    if !hex.len().is_multiple_of(2) {
        anyhow::bail!("signature hex must have an even number of nibbles");
    }
    let sig: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect::<std::result::Result<_, _>>()
        .context("parsing signature hex")?;
    if sig.is_empty() {
        anyhow::bail!("empty signature");
    }
    let mut ar = legaia_prot::archive::Archive::open(prot_dat)?;
    let mut buf = Vec::new();
    let mut hits = 0usize;
    println!(
        "# searching {} byte signature {} across PROT {from}..={to}",
        sig.len(),
        sig.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    for idx in from..=to {
        let entry = match ar.entries.iter().find(|e| e.index == idx).cloned() {
            Some(e) => e,
            None => continue,
        };
        buf.clear();
        ar.read_entry(&entry, &mut buf)?;
        if let Some(off) = static_overlay::find_signature(&buf, &sig) {
            match anchor_va {
                Some(va) => {
                    let base = va.wrapping_sub(off as u32);
                    println!("PROT {idx:>4}  file_off=0x{off:06X}  implied_base=0x{base:08X}");
                }
                None => println!("PROT {idx:>4}  file_off=0x{off:06X}"),
            }
            hits += 1;
        }
    }
    println!("# {hits} hit(s)");
    Ok(())
}
