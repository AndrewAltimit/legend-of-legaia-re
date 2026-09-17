//! `asset field-op-census` - the disc-wide field-VM opcode census.
//!
//! Walks every field-VM bytecode carrier on the disc and counts opcode
//! occurrences at decoded instruction boundaries, with the sub-dispatched
//! opcodes (`0x4C`, `0x43`, `0x45`, `0x49`, `0x34`) broken out by sub-arm.
//!
//! Three carrier kinds are walked, and all three are needed:
//!
//! * the **scene bundle's** MAN (the `type 0x03` descriptor, LZS-packed);
//! * every **streaming** MAN chunk (`type 0x03` inside a DATA_FIELD stream) -
//!   the per-scene *variant* MANs, which carry scripted cutscene records the
//!   bundle MAN does not;
//! * the raw **event-script** carriers (`scene_event_scripts`), which is where
//!   a `.PCH` scene's prescript records live (the prescript is the entry
//!   *after* the `.PCH` table, not a field inside it).
//!
//! Output is a per-entry x per-op CSV plus a summary the reach rows can cite.
//! See `docs/tooling/field-op-census.md`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use legaia_asset::field_disasm::{OpCensus, OpKey};
use legaia_prot::cdname;

/// One carrier's census plus enough provenance to reduce a hit to bytes.
struct CarrierCensus {
    entry_idx: u32,
    scene: String,
    kind: &'static str,
    census: OpCensus,
    /// `(label, body, pc0)` per record, kept only under `--context` so a rare
    /// op's hits can be re-walked and printed instead of trusted.
    bodies: Vec<(String, Vec<u8>, usize)>,
}

/// Walk `dir` (a directory of extracted PROT entries) and emit the census.
pub(crate) fn field_op_census(
    dir: &Path,
    cdname_path: Option<&Path>,
    csv_out: Option<&Path>,
    only: Option<&str>,
    context: bool,
) -> Result<()> {
    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

    let filter = only.map(parse_op_key).transpose()?;

    let mut carriers: Vec<CarrierCensus> = Vec::new();
    let mut total = OpCensus::default();
    let mut man_payloads_seen = 0usize;
    for path in &entries {
        let Ok(buf) = std::fs::read(path) else {
            continue;
        };
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let idx: u32 = stem
            .split_once('_')
            .and_then(|(n, _)| n.parse().ok())
            .unwrap_or(u32::MAX);
        let scene = names
            .as_ref()
            .and_then(|m| cdname::block_for_extraction_index(m, idx))
            .unwrap_or("-")
            .to_string();

        // Dedupe identical MAN payloads: a scene's bundle MAN and one of its
        // streaming chunks can be the same bytes.
        let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();
        for (kind, man) in man_payloads(&buf) {
            if !seen.insert(man.clone()) {
                continue;
            }
            let Ok(man_file) = legaia_asset::man_section::parse(&man) else {
                continue;
            };
            man_payloads_seen += 1;
            let mut c = OpCensus::default();
            legaia_asset::field_disasm::tally_man(&man_file, &man, &mut c, None);
            let bodies = if context {
                legaia_asset::field_disasm::man_script_spans(&man_file, &man)
                    .into_iter()
                    .map(|(p, r, start, pc0, len)| {
                        (format!("p{p}[{r}]"), man[start..start + len].to_vec(), pc0)
                    })
                    .collect()
            } else {
                Vec::new()
            };
            total.merge(&c);
            carriers.push(CarrierCensus {
                entry_idx: idx,
                scene: scene.clone(),
                kind,
                census: c,
                bodies,
            });
        }

        let is_event = legaia_asset::scene_event_scripts::detect(&buf).is_some()
            || legaia_asset::scene_event_scripts::detect_structural(&buf).is_some();
        if is_event && let Some(ranges) = legaia_asset::scene_event_scripts::record_ranges(&buf) {
            let mut c = OpCensus::default();
            for &(a, b) in &ranges {
                if let Some(body) = buf.get(a..b) {
                    c.tally_record(body, 0);
                }
            }
            let bodies = if context {
                ranges
                    .iter()
                    .enumerate()
                    .filter_map(|(i, &(a, b))| {
                        buf.get(a..b)
                            .map(|body| (format!("ev[{i}]"), body.to_vec(), 0))
                    })
                    .collect()
            } else {
                Vec::new()
            };
            total.merge(&c);
            carriers.push(CarrierCensus {
                entry_idx: idx,
                scene: scene.clone(),
                kind: "event",
                census: c,
                bodies,
            });
        }
    }

    if let Some(key) = filter {
        println!("# carriers with a CLEAN [{key}] occurrence");
        println!(
            "{:<6} {:<12} {:<8} {:>6} {:>6}",
            "entry", "scene", "kind", "clean", "total"
        );
        let mut hits = 0usize;
        let mut scenes: BTreeSet<String> = BTreeSet::new();
        for c in &carriers {
            let clean = c.census.clean_count(key);
            let tot = c.census.total.get(&key).copied().unwrap_or(0);
            if tot == 0 {
                continue;
            }
            hits += clean;
            if clean > 0 {
                scenes.insert(c.scene.clone());
            }
            println!(
                "{:<6} {:<12} {:<8} {:>6} {:>6}",
                c.entry_idx, c.scene, c.kind, clean, tot
            );
            if context && clean > 0 {
                print_hit_context(c, key);
            }
        }
        println!(
            "\n# [{key}]: {} clean occurrence(s) across {} scene(s); {} carrier(s) / {} MAN payload(s) walked",
            hits,
            scenes.len(),
            carriers.len(),
            man_payloads_seen
        );
        return Ok(());
    }

    // Per-op summary, clean-count descending.
    let mut rows: Vec<(OpKey, usize, usize, usize)> = Vec::new();
    for key in total.keys() {
        let clean = total.clean_count(key);
        let tot = total.total.get(&key).copied().unwrap_or(0);
        let scenes = carriers
            .iter()
            .filter(|c| c.census.clean_count(key) > 0)
            .map(|c| c.scene.clone())
            .collect::<BTreeSet<_>>()
            .len();
        rows.push((key, clean, tot, scenes));
    }
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    println!(
        "# {} carrier(s), {} MAN payload(s), {} record(s), {} desynced, {} decode error(s), {} script byte(s)",
        carriers.len(),
        man_payloads_seen,
        total.records,
        total.desynced_records,
        total.decode_errors,
        total.script_bytes
    );
    println!("{:<8} {:>9} {:>9} {:>8}", "op", "clean", "total", "scenes");
    for (key, clean, tot, scenes) in &rows {
        println!("{:<8} {:>9} {:>9} {:>8}", key.label(), clean, tot, scenes);
    }

    if let Some(out) = csv_out {
        let mut s = String::from("entry,scene,carrier,op,clean,total\n");
        for c in &carriers {
            for key in c.census.keys() {
                let clean = c.census.clean_count(key);
                let tot = c.census.total.get(&key).copied().unwrap_or(0);
                s.push_str(&format!(
                    "{},{},{},{},{},{}\n",
                    c.entry_idx,
                    c.scene,
                    c.kind,
                    key.label().replace(' ', ""),
                    clean,
                    tot
                ));
            }
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(out, s)?;
        eprintln!("wrote {}", out.display());
    }
    Ok(())
}

/// Every MAN payload a PROT entry carries: the scene bundle's `type 0x03`
/// descriptors (LZS-packed) and every `type 0x03` DATA_FIELD stream chunk.
fn man_payloads(buf: &[u8]) -> Vec<(&'static str, Vec<u8>)> {
    let mut out: Vec<(&'static str, Vec<u8>)> = Vec::new();
    if let Some(table) = legaia_asset::scene_asset_table::detect(buf) {
        for d in table.descriptors.iter().filter(|d| d.type_byte == 0x03) {
            let start = d.data_offset as usize;
            if start >= buf.len() {
                continue;
            }
            if let Ok((decoded, _)) = legaia_lzs::decompress_tracked(&buf[start..], d.size as usize)
            {
                out.push(("bundle", decoded));
            }
        }
    }
    if let Ok(report) = legaia_asset::parse_streaming(buf, 4096) {
        for chunk in &report.chunks {
            if chunk.type_byte != 0x03 {
                continue;
            }
            let start = chunk.header_offset + 4;
            if let Some(payload) = buf.get(start..start.saturating_add(chunk.size as usize))
                && legaia_asset::man_section::parse(payload).is_ok()
            {
                out.push(("stream", payload.to_vec()));
            }
        }
    }
    out
}

/// `"4CCF"` / `"4C CF"` / `"39"` -> an [`OpKey`].
fn parse_op_key(s: &str) -> Result<OpKey> {
    let hex: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    match hex.len() {
        2 => Ok(OpKey {
            opcode: u8::from_str_radix(&hex, 16)?,
            sub: None,
        }),
        4 => Ok(OpKey {
            opcode: u8::from_str_radix(&hex[0..2], 16)?,
            sub: Some(u8::from_str_radix(&hex[2..4], 16)?),
        }),
        _ => anyhow::bail!("--only wants `4C CF` or `39`, got {s:?}"),
    }
}

/// Print the decoded neighbourhood of every **clean** hit of `key` in one
/// carrier: two instructions either side, so a rare op's single occurrence is
/// read rather than trusted. A linear walk can stay error-free through message
/// text and re-sync on a byte that is no opcode; this is the check for that.
fn print_hit_context(c: &CarrierCensus, key: OpKey) {
    use legaia_asset::field_disasm::{LinearWalker, clean_hit_offsets, format_instruction};
    for (label, body, pc0) in &c.bodies {
        for hit in clean_hit_offsets(body, *pc0, key) {
            println!("    {label} @ 0x{hit:04X}");
            let lines: Vec<(usize, String)> = LinearWalker::new(body, *pc0)
                .map_while(|step| step.ok())
                .map(|insn| (insn.pc, format_instruction(&insn, body)))
                .collect();
            let Some(at) = lines.iter().position(|(pc, _)| *pc == hit) else {
                continue;
            };
            let lo = at.saturating_sub(2);
            let hi = (at + 3).min(lines.len());
            for (i, (_, line)) in lines[lo..hi].iter().enumerate() {
                let mark = if lo + i == at { ">>" } else { "  " };
                println!("    {mark}{line}");
            }
        }
    }
}
