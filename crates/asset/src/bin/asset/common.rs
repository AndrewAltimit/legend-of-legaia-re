use std::path::Path;

use anyhow::Result;
use legaia_prot::cdname;

/// LZS-decode the MAN sub-asset out of a scene_asset_table bundle entry.
///
/// Returns the decompressed MAN bytes plus the descriptor that pointed
/// at them. Bails when the buffer isn't a scene_asset_table or doesn't
/// have a type-0x03 (MAN) descriptor.
pub(crate) fn load_man_bytes(
    buf: &[u8],
) -> Result<(Vec<u8>, legaia_asset::scene_asset_table::DescriptorRecord)> {
    let table = legaia_asset::scene_asset_table::detect(buf)
        .ok_or_else(|| anyhow::anyhow!("not a scene_asset_table"))?;
    let man = table
        .descriptors
        .iter()
        .find(|d| d.type_byte == 0x03)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("bundle has no MAN (type 0x03) descriptor"))?;
    let start = man.data_offset as usize;
    if start >= buf.len() {
        anyhow::bail!(
            "MAN descriptor data_offset 0x{:X} past entry end ({})",
            start,
            buf.len()
        );
    }
    let (decoded, _) = legaia_lzs::decompress_tracked(&buf[start..], man.size as usize)?;
    Ok((decoded, man))
}

pub(crate) fn parse_cdname_text(text: &str) -> std::collections::HashMap<u32, String> {
    // CDNAME.TXT format: `#define <label> <PROT_index>` lines.
    // The label inherits forward until the next #define; we still only
    // emit the explicit (label, prot_index) pairs here.
    let mut out = std::collections::HashMap::new();
    for line in text.lines() {
        let l = line.trim();
        let Some(rest) = l.strip_prefix("#define ") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let Some(label) = parts.next() else { continue };
        let Some(idx_str) = parts.next() else {
            continue;
        };
        if let Ok(idx) = idx_str.parse::<u32>() {
            out.insert(idx, label.to_string());
        }
    }
    out
}

/// Encode an RGBA8 buffer (`width * height * 4` bytes) to a PNG file.
pub(crate) fn write_rgba_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    legaia_tim::write_png(path, width as usize, height as usize, rgba)
}

/// Build a display label for a PROT entry: `<index>_<cdname-block>` if we
/// have a name table, else just the file stem.
pub(crate) fn display_name_for(stem: &str, names: Option<&cdname::IndexMap>) -> String {
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
