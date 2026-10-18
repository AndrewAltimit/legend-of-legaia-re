//! `asset disc-map`: the disc's layout as JSON, for the site's disc-map page.
//!
//! Two levels. The ISO9660 level lists every file and directory extent by LBA
//! and fills the sectors no extent claims (the system area ahead of the volume
//! descriptors, inter-file padding) so the sectors sum to the whole image. The
//! PROT.DAT level lists each TOC entry with its CDNAME block, its sector range
//! and the [`categorize`] class of its bytes, mapped to a format family and the
//! site page that documents it.
//!
//! Everything written is layout metadata - names, LBAs, sizes, class names.
//! No entry bytes, no strings read out of the game data.

use std::path::Path;

use anyhow::{Context, Result};
use legaia_disc_audit::categorize::{self, Class};
use legaia_iso::iso9660::{self, DirectoryRecord};
use legaia_iso::raw::{RawDisc, USER_DATA_SIZE};
use legaia_overlay_images::static_overlay::overlay_map;
use legaia_prot::archive::Archive;
use legaia_prot::cdname;
use serde::Serialize;

/// Bumped when a consumer would misread the old shape.
const SCHEMA: &str = "legaia-disc-map-v1";

#[derive(Serialize)]
struct DiscMap {
    schema: &'static str,
    volume_id: String,
    /// Mode2/2352 sectors in the image.
    total_sectors: u64,
    /// ISO-level regions in LBA order, covering `0..total_sectors` exactly.
    regions: Vec<Region>,
    /// Archives whose TOC was walked, keyed by their region path.
    archives: Vec<ArchiveMap>,
}

#[derive(Serialize)]
struct Region {
    /// ISO path (`XA/XA1.XA`), or a bracketed name for an unclaimed run.
    path: String,
    /// `file`, `dir`, `system_area` or `gap`.
    kind: &'static str,
    lba: u32,
    sectors: u32,
    /// Byte size from the directory record (files and directories).
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes: Option<u32>,
    family: &'static str,
    /// What the region is, in a few words.
    what: &'static str,
    /// Site-relative page documenting it.
    doc: &'static str,
}

#[derive(Serialize)]
struct ArchiveMap {
    /// The region `path` this archive lives in.
    path: String,
    /// Sectors of header + TOC ahead of the first entry.
    header_sectors: u32,
    /// Sectors of the file (logical 2048-byte sectors).
    file_sectors: u32,
    /// CDNAME blocks, in raw-TOC order (`raw` = the `#define` number).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    blocks: Vec<Block>,
    /// The sectors ahead of entry 0: the header + TOC, then the raw TOC rows
    /// 0 and 1 that no extraction index addresses.
    head: Vec<HeadRow>,
    entries: Vec<EntryMap>,
    /// Sectors of the file past the last TOC entry, when there are any.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tail: Vec<HeadRow>,
}

#[derive(Serialize)]
struct HeadRow {
    name: &'static str,
    lba: u32,
    sectors: u32,
    class: &'static str,
    family: &'static str,
    doc: &'static str,
}

#[derive(Serialize)]
struct Block {
    label: String,
    raw: u32,
}

#[derive(Serialize)]
struct EntryMap {
    /// Extraction index (`extracted/PROT/NNNN_*.BIN`).
    i: u32,
    /// Start sector, relative to the archive file.
    lba: u32,
    sectors: u32,
    /// Index into `blocks` (retail-semantic: extraction + 2 against the
    /// `#define` numbers).
    #[serde(skip_serializing_if = "Option::is_none")]
    block: Option<usize>,
    /// Zero-based slot within the block.
    #[serde(skip_serializing_if = "Option::is_none")]
    slot: Option<u32>,
    class: &'static str,
    family: &'static str,
    doc: &'static str,
    /// Static-overlay identity (`label@base`) where the overlay map names one.
    #[serde(skip_serializing_if = "Option::is_none")]
    overlay: Option<String>,
    /// What the entry is, where a documented identity outranks the class.
    #[serde(skip_serializing_if = "Option::is_none")]
    known: Option<&'static str>,
}

/// Entries whose identity is pinned by a loader constant and documented, for
/// the ones the shape classifier leaves generic. `(index, what, family, doc)`
/// in the extraction frame.
const KNOWN_ENTRIES: &[(u32, &str, &str, &str)] = &[
    (867, "Monster archive", "model", "subsystems/battle.html"),
    (
        874,
        "Field character pack + locomotion ANM",
        "model",
        "formats/character-mesh.html",
    ),
    (
        1221,
        "Ringside still (16bpp)",
        "texture",
        "formats/ringside-still.html",
    ),
    (
        1222,
        "Ringside still (16bpp)",
        "texture",
        "formats/ringside-still.html",
    ),
];

/// `(family, site doc page)` for a categorizer class.
fn class_family(c: Class) -> (&'static str, &'static str) {
    use Class::*;
    match c {
        TimPassthrough => ("texture", "formats/tim.html"),
        Pack => ("texture", "formats/pack.html"),
        TimPack => ("texture", "formats/tim-pack.html"),
        DataFieldStreaming | DataFieldTruncated => ("scene", "formats/data-field.html"),
        SceneAssetTable | SceneScriptedAssetTable => ("scene", "formats/scene-bundles.html"),
        SceneEventScripts => ("scene", "formats/scene-bundles.html"),
        SceneV12Table => ("scene", "formats/scene-v12-table.html"),
        FieldMap => ("scene", "formats/field-map.html"),
        StageGeometry => ("scene", "formats/scene-bundles.html"),
        SceneTmdStream | TmdSizePrefix => ("model", "formats/tmd.html"),
        AnmContainer => ("model", "formats/anm.html"),
        BattleDataPack => ("model", "formats/battle-data-pack.html"),
        LzsContainer => ("model", "formats/lzs.html"),
        EffectBundle | EfectPack => ("effect", "formats/effect.html"),
        SummonReadef => ("effect", "formats/summon-readef.html"),
        SceneVabStream | VabMultiBank | MonsterSoundBank => ("audio", "formats/vab.html"),
        SeqContainer => ("audio", "formats/seq.html"),
        BseBank => ("audio", "formats/bse-dat.html"),
        MipsOverlay => ("code", "formats/mips-overlay.html"),
        OverlayPtrTable => ("code", "formats/overlay-ptr-table.html"),
        InitPak => ("code", "subsystems/boot.html"),
        PochiFiller => ("filler", "formats/pochi.html"),
        Empty | Tiny | AllZeros | MostlyZeros | ConstantByte => {
            ("filler", "tooling/byte-accounting.html")
        }
        ZeroSectorHighEntropy
        | OverlayDataBlob
        | UnknownHighEntropy
        | UnknownLowEntropy
        | UnknownOther => ("unknown", "tooling/byte-accounting.html"),
    }
}

/// `(family, what, doc)` for an ISO9660 file, by name.
fn file_family(path: &str) -> (&'static str, &'static str, &'static str) {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_uppercase();
    match name.as_str() {
        "SYSTEM.CNF" => ("system", "Boot configuration", "formats/disc.html"),
        "PROT.DAT" => ("archive", "Main asset archive", "formats/prot.html"),
        "DMY.DAT" => ("archive", "Dev-fixture archive", "formats/dmy.html"),
        "CDNAME.TXT" => ("system", "PROT block name map", "formats/cdname.html"),
        n if n.starts_with("SCUS_") || n.starts_with("SLUS_") => {
            ("code", "Main executable", "subsystems/boot.html")
        }
        n if n.ends_with(".XA") => ("audio", "CD-XA streamed audio", "formats/xa.html"),
        n if n.ends_with(".STR") => ("movie", "MDEC FMV movie", "subsystems/cutscene.html"),
        _ => ("unknown", "File", "formats/disc.html"),
    }
}

fn sectors_of(bytes: u32) -> u32 {
    bytes.div_ceil(USER_DATA_SIZE as u32).max(1)
}

/// Every file and directory under `root`, with their paths.
fn walk_all(disc: &mut RawDisc, root: &DirectoryRecord) -> Result<Vec<(String, DirectoryRecord)>> {
    let mut out = vec![(String::new(), root.clone())];
    let mut stack = vec![(String::new(), root.clone())];
    let mut seen = std::collections::HashSet::from([root.lba]);
    while let Some((prefix, dir)) = stack.pop() {
        for e in iso9660::list_directory(disc, &dir)? {
            let path = if prefix.is_empty() {
                e.name.clone()
            } else {
                format!("{prefix}/{}", e.name)
            };
            if e.is_dir {
                if seen.insert(e.lba) {
                    out.push((path.clone(), e.clone()));
                    stack.push((path, e));
                }
            } else {
                out.push((path, e));
            }
        }
    }
    Ok(out)
}

fn read_file(disc: &mut RawDisc, rec: &DirectoryRecord) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    disc.read_user_data(rec.lba, sectors_of(rec.size), &mut buf)?;
    buf.truncate(rec.size as usize);
    Ok(buf)
}

fn map_archive(
    path: &str,
    bytes: Vec<u8>,
    cdname_map: Option<&cdname::IndexMap>,
) -> Result<ArchiveMap> {
    let file_sectors = sectors_of(bytes.len() as u32);
    let mut ar = Archive::from_bytes(bytes)?;
    let blocks: Vec<Block> = cdname_map
        .map(|m| {
            m.iter()
                .map(|(&raw, label)| Block {
                    label: label.clone(),
                    raw,
                })
                .collect()
        })
        .unwrap_or_default();
    let mut head = vec![HeadRow {
        name: "header + TOC",
        lba: 0,
        sectors: ar.header.header_sectors,
        class: "toc",
        family: "archive",
        doc: "formats/prot.html",
    }];
    let first_lba = ar.entries.first().map(|e| e.start_lba).unwrap_or(0);
    let mut buf = Vec::new();
    for (row, name) in [(0usize, "raw TOC row 0"), (1, "raw TOC row 1")] {
        let (Some(&start), Some(&end)) = (ar.toc.get(row), ar.toc.get(row + 1)) else {
            continue;
        };
        if start < ar.header.header_sectors || end <= start || end > first_lba {
            continue;
        }
        let byte_offset = u64::from(start) * u64::from(legaia_prot::archive::SECTOR);
        let len = (end - start) as usize * legaia_prot::archive::SECTOR as usize;
        ar.read_raw(byte_offset, len, &mut buf)?;
        let class = categorize::classify(&buf).class;
        let (family, doc) = class_family(class);
        head.push(HeadRow {
            name,
            lba: start,
            sectors: end - start,
            class: class.name(),
            family,
            doc,
        });
    }
    let overlays = overlay_map();
    let mut entries = Vec::with_capacity(ar.entries.len());
    for e in ar.entries.clone() {
        ar.read_entry(&e, &mut buf)?;
        let class = categorize::classify(&buf).class;
        let (mut family, mut doc) = class_family(class);
        let rec = overlays.by_prot_index(e.index);
        // An entry the static-overlay map names is code whatever its head
        // looks like to the shape classifier.
        if rec.is_some() && family != "code" {
            family = "code";
            doc = "tooling/static-overlay-pipeline.html";
        }
        let known = KNOWN_ENTRIES.iter().find(|k| k.0 == e.index).map(|k| {
            family = k.2;
            doc = k.3;
            k.1
        });
        let raw = e.index + cdname::RAW_TOC_INDEX_OFFSET;
        let block = cdname_map.and_then(|_| blocks.iter().rposition(|b| b.raw <= raw));
        let slot = block.map(|b| raw - blocks[b].raw);
        let overlay = rec.map(|r| format!("{}@0x{:08X}", r.label, r.base_va));
        entries.push(EntryMap {
            i: e.index,
            lba: e.start_lba,
            sectors: e.size_sectors,
            block,
            slot,
            class: class.name(),
            family,
            doc,
            overlay,
            known,
        });
    }
    let mut tail = Vec::new();
    let covered = ar
        .entries
        .last()
        .map(|e| e.start_lba + e.size_sectors)
        .unwrap_or(first_lba);
    if covered < file_sectors {
        let byte_offset = u64::from(covered) * u64::from(legaia_prot::archive::SECTOR);
        let len = (ar.file_len() - byte_offset) as usize;
        ar.read_raw(byte_offset, len, &mut buf)?;
        let class = categorize::classify(&buf).class;
        let (family, doc) = class_family(class);
        tail.push(HeadRow {
            name: "past the last TOC entry",
            lba: covered,
            sectors: file_sectors - covered,
            class: class.name(),
            family,
            doc,
        });
    }
    Ok(ArchiveMap {
        path: path.to_string(),
        header_sectors: ar.header.header_sectors,
        file_sectors,
        blocks,
        head,
        entries,
        tail,
    })
}

pub(crate) fn disc_map_cmd(disc_path: &Path, out: Option<&Path>) -> Result<()> {
    let mut disc =
        RawDisc::open(disc_path).with_context(|| format!("opening {}", disc_path.display()))?;
    let total_sectors = disc.sector_count();
    let vol = iso9660::read_volume(&mut disc)?;
    let mut recs = walk_all(&mut disc, &vol.root)?;
    recs.sort_by_key(|(_, r)| r.lba);

    let mut regions = Vec::new();
    let mut cursor: u32 = 0;
    let push_gap = |regions: &mut Vec<Region>, from: u32, to: u32| {
        if to > from {
            let system = from < 16;
            regions.push(Region {
                path: if system {
                    "[system area]".into()
                } else {
                    "[unclaimed]".into()
                },
                kind: if system { "system_area" } else { "gap" },
                lba: from,
                sectors: to - from,
                bytes: None,
                family: "iso",
                what: if system {
                    "PSX licence sectors, ISO9660 volume descriptors and path tables"
                } else {
                    "Sectors no directory record claims (padding)"
                },
                doc: "formats/disc.html",
            });
        }
    };
    let mut prot = None;
    let mut dmy = None;
    let mut cdname_rec = None;
    for (path, r) in &recs {
        if r.lba < cursor {
            continue;
        }
        push_gap(&mut regions, cursor, r.lba);
        let sectors = sectors_of(r.size);
        let (family, what, doc) = if r.is_dir {
            ("iso", "ISO9660 directory extent", "formats/disc.html")
        } else {
            file_family(path)
        };
        let shown = if path.is_empty() { "/" } else { path.as_str() };
        regions.push(Region {
            path: shown.to_string(),
            kind: if r.is_dir { "dir" } else { "file" },
            lba: r.lba,
            sectors,
            bytes: Some(r.size),
            family,
            what,
            doc,
        });
        match path.to_ascii_uppercase().as_str() {
            "PROT.DAT" => prot = Some((path.clone(), r.clone())),
            "DMY.DAT" => dmy = Some((path.clone(), r.clone())),
            "CDNAME.TXT" => cdname_rec = Some(r.clone()),
            _ => {}
        }
        cursor = r.lba + sectors;
    }
    let end = u32::try_from(total_sectors).unwrap_or(u32::MAX);
    push_gap(&mut regions, cursor, end);

    let cdname_map = match &cdname_rec {
        Some(r) => {
            let text = read_file(&mut disc, r)?;
            Some(cdname::parse_str(&String::from_utf8_lossy(&text))?)
        }
        None => None,
    };

    let mut archives = Vec::new();
    if let Some((path, r)) = prot {
        let bytes = read_file(&mut disc, &r)?;
        archives.push(map_archive(&path, bytes, cdname_map.as_ref())?);
    }
    if let Some((path, r)) = dmy {
        let bytes = read_file(&mut disc, &r)?;
        // DMY.DAT shares the PROT TOC shape; a disc whose copy does not parse
        // just keeps it as one file-level box.
        if let Ok(m) = map_archive(&path, bytes, None) {
            archives.push(m);
        }
    }

    let map = DiscMap {
        schema: SCHEMA,
        volume_id: vol.volume_id,
        total_sectors,
        regions,
        archives,
    };
    let json = serde_json::to_string(&map)?;
    match out {
        Some(p) => {
            std::fs::write(p, json + "\n").with_context(|| format!("writing {}", p.display()))?;
            eprintln!(
                "wrote {} ({} regions, {} archive entries)",
                p.display(),
                map.regions.len(),
                map.archives.iter().map(|a| a.entries.len()).sum::<usize>()
            );
        }
        None => println!("{json}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_disc_file_kind_has_a_doc_page() {
        for name in [
            "SYSTEM.CNF",
            "PROT.DAT",
            "XA/XA1.XA",
            "MOV/MV1.STR",
            "SCUS_942.54",
        ] {
            let (family, _, doc) = file_family(name);
            assert_ne!(family, "unknown", "{name}");
            assert!(doc.ends_with(".html"));
        }
    }
}
