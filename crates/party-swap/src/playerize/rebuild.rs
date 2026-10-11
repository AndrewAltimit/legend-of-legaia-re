//! Rebuilding a player battle file around playerized sections, with and without
//! the retail size bound.

use super::super::*;

/// Re-pack a player file around a replacement record list: everything
/// below `data_base` verbatim except the descriptor table (rewritten from
/// `records`, `(id, decoded bytes)` in chain order, with an all-zero
/// terminator where the retail table had room for one), then the records
/// LZS-compressed into `0x800`-aligned slots from `data_base` - the
/// retail invariant the sector-streaming loader relies on. Greedy LZS
/// first; the optimal parse is paid per record only when the greedy total
/// misses the `entry_len` budget. Errors when the records cannot fit.
pub fn rebuild_player_file(
    player_file: &[u8],
    table_offset: usize,
    data_base: usize,
    records: Vec<(u32, Vec<u8>)>,
    entry_len: usize,
) -> Result<Vec<u8>> {
    rebuild_player_file_inner(
        player_file,
        table_offset,
        data_base,
        records,
        Some(entry_len),
    )
}

/// [`rebuild_player_file`] with no entry budget: greedy LZS throughout,
/// the file ends at the last record's sector boundary. The shape a
/// caller parks outside the PROT entry (`legaia_actor_formats::player_file_annex`)
/// wants - room is not the constraint there.
pub fn rebuild_player_file_unbounded(
    player_file: &[u8],
    table_offset: usize,
    data_base: usize,
    records: Vec<(u32, Vec<u8>)>,
) -> Result<Vec<u8>> {
    rebuild_player_file_inner(player_file, table_offset, data_base, records, None)
}

fn rebuild_player_file_inner(
    player_file: &[u8],
    table_offset: usize,
    data_base: usize,
    records: Vec<(u32, Vec<u8>)>,
    entry_len: Option<usize>,
) -> Result<Vec<u8>> {
    let mut new_records: Vec<(u32, Vec<u8>, Vec<u8>)> = records
        .into_iter()
        .map(|(id, rewritten)| {
            let stream = legaia_lzs::compress(&rewritten);
            let mut slot = Vec::with_capacity(4 + stream.len());
            slot.extend_from_slice(&(rewritten.len() as u32).to_le_bytes());
            slot.extend_from_slice(&stream);
            (id, slot, rewritten)
        })
        .collect();
    if table_offset + new_records.len() * 12 > data_base {
        bail!(
            "descriptor table of {} entries overruns the data base",
            new_records.len()
        );
    }

    // Greedy total first; pay for the optimal parse per record only when
    // the file misses its entry budget.
    let budget = entry_len.map(|e| e.saturating_sub(data_base));
    let total = |recs: &[(u32, Vec<u8>, Vec<u8>)]| -> usize {
        recs.iter()
            .map(|(_, s, _)| (s.len() + 0x7FF) & !0x7FF)
            .sum()
    };
    if let Some(budget) = budget {
        if total(&new_records) > budget {
            for (_, slot, decoded) in new_records.iter_mut() {
                let stream = legaia_lzs::compress_optimal(decoded);
                if 4 + stream.len() < slot.len() {
                    slot.truncate(4);
                    slot.extend_from_slice(&stream);
                }
            }
        }
        if total(&new_records) > budget {
            bail!(
                "rebuilt records need {} bytes, the PROT entry holds {}",
                total(&new_records),
                budget
            );
        }
    }

    // Rebuild the file: everything below data_base verbatim except the
    // descriptor entries' offset/size fields; records repacked from
    // data_base with 0x800-aligned slots (retail record offsets are all
    // sector-aligned - keep that invariant for the CD-read path).
    let mut file = player_file[..data_base.min(player_file.len())].to_vec();
    if file.len() < data_base {
        bail!("player file shorter than its data base");
    }
    let mut offset = 0u32;
    for (i, (id, slot, _)) in new_records.iter().enumerate() {
        let size = (slot.len() as u32 + 0x7FF) & !0x7FF;
        let p = table_offset + i * 12;
        file[p..p + 4].copy_from_slice(&id.to_le_bytes());
        file[p + 4..p + 8].copy_from_slice(&offset.to_le_bytes());
        file[p + 8..p + 12].copy_from_slice(&size.to_le_bytes());
        offset += size;
    }
    // Terminator entry (all-zero) right after ours, if the retail table
    // had room for one.
    let term = table_offset + new_records.len() * 12;
    if term + 12 <= data_base {
        file[term..term + 12].fill(0);
    }
    for (_, slot, _) in &new_records {
        let start = file.len();
        file.extend_from_slice(slot);
        let padded = (start + slot.len() + 0x7FF) & !0x7FF;
        file.resize(padded.max(file.len()), 0);
    }
    if let Some(entry_len) = entry_len {
        if file.len() > entry_len {
            bail!(
                "rebuilt player file {} bytes exceeds the PROT entry ({} bytes)",
                file.len(),
                entry_len
            );
        }
        file.resize(entry_len, 0);
    }
    Ok(file)
}
