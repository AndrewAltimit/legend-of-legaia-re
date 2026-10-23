//! The non-dialog lifts: UI pools in the menu overlays, the SCUS string pools,
//! place-name cells and monster names.

use super::super::pack::LanguagePack;
use super::super::{markup, monster_names, refpair, ui};
use super::chunk_align::{nul_chunks, pair_chunk_lists, pool_text, split_at_stride, table_stride};
use super::{LiftReport, PoolStat, key_scus_va};
use legaia_disc_patch::disc::DiscPatcher;
use legaia_game_tables::{item_names, worldmap_menu};
use std::collections::BTreeMap;

/// Fill the `ui:` entries: each pinned overlay pool is read on both discs,
/// the pool window on the source side widened (its strings are longer or
/// shorter, the pool moves), and the two NUL-chunk lists paired by anchors
/// ([`pair_chunk_lists`]).
pub(super) fn lift_ui_pools(
    target: &DiscPatcher,
    source: &DiscPatcher,
    by_ref: &BTreeMap<String, Vec<u8>>,
    pack: &mut LanguagePack,
    report: &mut LiftReport,
) {
    for pool in ui::UI_STRING_POOLS {
        let (Ok(usa_entry), Ok(src_entry)) = (
            target.read_entry(pool.prot_index),
            source.read_entry(pool.prot_index),
        ) else {
            continue;
        };
        let lo = (pool.va_start - pool.base_va) as usize;
        let hi = (pool.va_end - pool.base_va) as usize;
        let span = hi - lo;
        // A strict pool pairs through its references only: its windows mix
        // strings with debug formats and pointer tables, which a chunk list
        // would align against the wrong neighbour.
        let (usa_chunks, map) = if pool.strict {
            // A strict pool pairs through its references (and the source
            // build's pinned coordinates), or by offset where the source
            // lays the window out string for string like the target (a
            // patched USA disc): a PAL build lays these windows out
            // differently, and a chunk list aligns them against the wrong
            // neighbour.
            (Vec::new(), same_layout_pairs(&usa_entry, &src_entry, pool))
        } else {
            let usa_chunks = nul_chunks(&usa_entry, lo.saturating_sub(0x40), hi + 0x40);
            let mut src_chunks = nul_chunks(&src_entry, lo.saturating_sub(0x40), hi + span + 0x200);
            // A fixed-stride table's string that fills its slot has no NUL
            // before the next slot: cut it where the slot ends.
            let in_pool: Vec<usize> = usa_chunks
                .iter()
                .map(|c| c.0)
                .filter(|o| (lo..hi).contains(o))
                .collect();
            if let Some(stride) = table_stride(&in_pool) {
                src_chunks = split_at_stride(src_chunks, stride);
            }
            if std::env::var("LEGAIA_LIFT_DEBUG_POOL").ok().as_deref()
                == Some(&pool.prot_index.to_string())
            {
                for (o, b) in &src_chunks {
                    eprintln!("S 0x{o:05x} {:?}", markup::decode(b));
                }
            }
            let map = pair_chunk_lists(&usa_chunks, &src_chunks);
            (usa_chunks, map)
        };
        if std::env::var("LEGAIA_LIFT_DEBUG_POOL").ok().as_deref()
            == Some(&pool.prot_index.to_string())
        {
            for (o, b) in &usa_chunks {
                eprintln!(
                    "U 0x{o:05x} {:?} -> {:?}",
                    String::from_utf8_lossy(b),
                    map.get(o).map(|m| String::from_utf8_lossy(m).into_owned())
                );
            }
        }
        let mut stat = PoolStat {
            label: format!("{} (PROT {})", pool.label, pool.prot_index),
            paired: 0,
            total: 0,
        };
        let prefix = format!("ui:{}:0x", pool.prot_index);
        for e in pack.sections.ui_menu.iter_mut() {
            let Some(hex) = e.key.strip_prefix(&prefix) else {
                continue;
            };
            let Ok(va) = u32::from_str_radix(hex, 16) else {
                continue;
            };
            if va < pool.va_start || va >= pool.va_end {
                continue;
            }
            stat.total += 1;
            // A strict pool trusts its references first; an original pool
            // keeps the chunk pairing it was validated with and takes a
            // reference only where that pairs nothing.
            let by_ref_hit = by_ref.get(&e.key);
            if pool.strict
                && let Some(bytes) = by_ref_hit
            {
                e.translation = markup::decode(bytes);
                stat.paired += 1;
                report.pool_ref_paired += 1;
                continue;
            }
            let off = (va - pool.base_va) as usize;
            // A pool's first string can trail the code that addresses it with
            // no NUL between (the tutorial pool opens six code bytes ahead of
            // its first line): the chunk pairs as a whole, and the string is
            // its tail past the shared prefix.
            let hit = map.get(&off).cloned().or_else(|| {
                let (start, _) = usa_chunks
                    .iter()
                    .find(|(start, chunk)| *start < off && off < start + chunk.len())?;
                let prefix = off - start;
                map.get(start)
                    .filter(|src| src.len() > prefix)
                    .map(|src| src[prefix..].to_vec())
            });
            if let Some(bytes) = hit {
                e.translation = markup::decode(&bytes);
                stat.paired += 1;
            } else if let Some(bytes) = by_ref_hit {
                e.translation = markup::decode(bytes);
                stat.paired += 1;
                report.pool_ref_paired += 1;
            }
        }
        report.ui_paired += stat.paired;
        report.ui_total += stat.total;
        report.ui_pools.push(stat);
    }
}

/// A strict pool's strings paired by offset when both builds lay the window
/// out string for string at the same offsets (a fan patch applied to the USA
/// disc keeps every string in place): `offset -> source bytes`, empty when the
/// layouts differ at all.
fn same_layout_pairs(usa: &[u8], src: &[u8], pool: &ui::UiStringPool) -> BTreeMap<usize, Vec<u8>> {
    let (u, s) = (ui::scan_pool(usa, pool), ui::scan_pool(src, pool));
    if u.len() != s.len() || u.iter().zip(&s).any(|(a, b)| a.va != b.va) {
        return BTreeMap::new();
    }
    s.into_iter()
        .map(|x| ((x.va - pool.base_va) as usize, x.bytes))
        .collect()
}

/// Pool strings paired through the code and data that reference them
/// ([`refpair`]): `pack key -> source bytes`. The SCUS pools are matched over
/// the executable and every UI overlay (overlay code reaches the small-data
/// strings through `$gp`), each overlay pool over its own overlay. A source
/// string is read the way its pool reads strings on the USA side.
pub(super) fn pool_ref_pairs(
    target: &DiscPatcher,
    source: &DiscPatcher,
    usa_exe: &[u8],
    src_exe: &[u8],
    src_exe_name: &str,
    pack: &LanguagePack,
) -> BTreeMap<String, Vec<u8>> {
    use std::collections::BTreeSet;
    let mut out = BTreeMap::new();
    let (Some(usa_img), Some(src_img)) = (
        refpair::Image::from_exe(usa_exe),
        refpair::Image::from_exe(src_exe),
    ) else {
        return out;
    };
    let (usa_gp, src_gp) = (refpair::exe_gp(usa_exe), refpair::exe_gp(src_exe));
    // USA VA -> (pool, key) for every pooled entry.
    let mut scus: BTreeMap<u32, (&ui::UiStringPool, &str)> = BTreeMap::new();
    for e in &pack.sections.system_text {
        if let Some(va) = key_scus_va(&e.key)
            && let Some(pool) = ui::pool_for(usize::MAX, va)
        {
            scus.insert(va, (pool, e.key.as_str()));
        }
    }
    let mut overlays: BTreeMap<usize, BTreeMap<u32, (&ui::UiStringPool, &str)>> = BTreeMap::new();
    for e in &pack.sections.ui_menu {
        let mut it = e.key.split(':').skip(1);
        let (Some(prot), Some(va)) = (
            it.next().and_then(|p| p.parse::<usize>().ok()),
            it.next()
                .and_then(|h| h.strip_prefix("0x"))
                .and_then(|h| u32::from_str_radix(h, 16).ok()),
        ) else {
            continue;
        };
        if let Some(pool) = ui::pool_for(prot, va) {
            overlays
                .entry(prot)
                .or_default()
                .insert(va, (pool, e.key.as_str()));
        }
    }
    let scus_targets: BTreeSet<u32> = scus.keys().copied().collect();
    let mut scus_votes = vec![refpair::pair_by_refs(&usa_img, &src_img, &scus_targets)];
    // A reference pairs a string, never the middle of one.
    let read = |buf: &[u8], off: usize, strict: bool| -> Option<Vec<u8>> {
        if off == 0 || buf.get(off - 1) != Some(&0) {
            return None;
        }
        let len = ui::pool_strlen(buf, off, strict)?;
        Some(buf[off..off + len].to_vec())
    };
    for (&prot, strings) in &overlays {
        let (Ok(u), Ok(s)) = (target.read_entry(prot), source.read_entry(prot)) else {
            continue;
        };
        let Some(base) = ui::overlay_base_va(prot) else {
            continue;
        };
        let (ui_img, si_img) = (
            refpair::Image::from_overlay(&u, base, usa_gp),
            refpair::Image::from_overlay(&s, base, src_gp),
        );
        let targets: BTreeSet<u32> = strings.keys().chain(scus.keys()).copied().collect();
        let votes = refpair::pair_by_refs(&ui_img, &si_img, &targets);
        let (mine, theirs): (BTreeMap<_, _>, BTreeMap<_, _>) = votes
            .into_iter()
            .partition(|(va, _)| strings.contains_key(va));
        scus_votes.push(theirs);
        for (usa_va, src_va) in refpair::merge_votes(&[mine]) {
            let (pool, key) = strings[&usa_va];
            let Some(off) = src_va.checked_sub(base).map(|d| d as usize) else {
                continue;
            };
            if let Some(bytes) = read(&s, off, pool.strict).filter(|b| pool_text(b)) {
                out.insert(key.to_string(), bytes);
            }
        }
    }
    for (usa_va, src_va) in refpair::merge_votes(&scus_votes) {
        let (pool, key) = scus[&usa_va];
        let Some(off) = item_names::file_offset_for_va(src_exe, src_va) else {
            continue;
        };
        if let Some(bytes) = read(src_exe, off, pool.strict).filter(|b| pool_text(b)) {
            out.insert(key.to_string(), bytes);
        }
    }
    // Pinned coordinates of the source build for the strict-pool strings no
    // reference pairs (the PAL build rewrote the code that draws them).
    for (prot, usa_off, src_off) in pinned_pool_pairs(src_exe_name) {
        if prot == usize::MAX {
            let Some((pool, key)) = scus.get(&usa_off).copied() else {
                continue;
            };
            if out.contains_key(key) {
                continue;
            }
            if let Some(off) = item_names::file_offset_for_va(src_exe, src_off)
                && let Some(len) = ui::pool_strlen(src_exe, off, pool.strict)
            {
                out.insert(key.to_string(), src_exe[off..off + len].to_vec());
            }
            continue;
        }
        let Some(base) = ui::overlay_base_va(prot) else {
            continue;
        };
        let Some((pool, key)) = overlays
            .get(&prot)
            .and_then(|m| m.get(&(base + usa_off)))
            .copied()
        else {
            continue;
        };
        if out.contains_key(key) {
            continue;
        }
        let Ok(s) = source.read_entry(prot) else {
            continue;
        };
        let off = src_off as usize;
        if let Some(bytes) = ui::pool_strlen(&s, off, pool.strict)
            .map(|len| s[off..off + len].to_vec())
            .filter(|b| !b.is_empty())
        {
            out.insert(key.to_string(), bytes);
        }
    }
    if std::env::var_os("LEGAIA_LIFT_DEBUG_REFS").is_some() {
        for (k, v) in &out {
            eprintln!("ref {k} -> {:?}", String::from_utf8_lossy(v));
        }
    }
    out
}

/// Strict-pool strings of one source build that no reference pairs, as
/// `(PROT entry, USA file offset, source file offset)` - or, for an
/// executable string, `(usize::MAX, USA VA, source VA)` - coordinates only,
/// read off the two discs by hand. The Spanish build (`SCES_019.47`, the base
/// of the Brazilian Portuguese fan translation) redraws the battle help, the
/// record screen, the name-entry prompts and the whole memory-card dialog
/// with rewritten code, and moves the Hyper Arts list help to the end of the
/// overlay; its strings sit in the same order, so the Seru-magic effect lines
/// pair slot for slot at each build's own stride.
pub(super) fn pinned_pool_pairs(src_exe_name: &str) -> Vec<(usize, u32, u32)> {
    if src_exe_name != "SCES_019.47" {
        return Vec::new();
    }
    const FIXED: &[(usize, u32, u32)] = &[
        // Executable: the Seru-obtained line (its only reference is a pointer
        // word with no table around it).
        (usize::MAX, 0x8001_52E0, 0x8001_5F78),
        // Battle overlay: counters, Hyper Arts list help, counterattack,
        // points returned, no effect, ran away.
        (898, 0x0000, 0x0000),
        (898, 0x0028, 0x267DC),
        (898, 0x0048, 0x267FC),
        (898, 0x0500, 0x04D0),
        (898, 0x051C, 0x04EC),
        (898, 0x1208, 0x1388),
        (898, 0x28048, 0x282B4),
        // Field overlay: record screen, give up, name entry.
        (897, 0x0D04, 0x0D3C),
        (897, 0x0D14, 0x0D4C),
        (897, 0x0D24, 0x0D5C),
        (897, 0x0D34, 0x0D6C),
        (897, 0x0D44, 0x0D78),
        (897, 0x0D50, 0x0D80),
        (897, 0x0D64, 0x0D98),
        (897, 0x0D70, 0x0DA4),
        (897, 0x0D78, 0x0DAC),
        (897, 0x0E38, 0x0E68),
        (897, 0x0E80, 0x0EB0),
        (897, 0x0F30, 0x0F78),
        // Menu overlay: the Delilas bout preamble.
        (899, 0x0460, 0x0524),
        (899, 0x047C, 0x0540),
        (899, 0x0490, 0x0554),
        (899, 0x04BC, 0x057C),
        (899, 0x04E4, 0x05AC),
        (899, 0x0508, 0x05D0),
        (899, 0x0520, 0x05E8),
        (899, 0x0540, 0x060C),
        (899, 0x058C, 0x0654),
        // Menu overlay: memory-card messages.
        (899, 0x0710, 0x07D8),
        (899, 0x072C, 0x07F4),
        (899, 0x0774, 0x0844),
        (899, 0x0798, 0x0878),
        (899, 0x07C8, 0x08B0),
        (899, 0x0834, 0x0928),
        (899, 0x0850, 0x0948),
        (899, 0x0868, 0x0960),
        (899, 0x086C, 0x0964),
        (899, 0x0874, 0x0968),
        (899, 0x0894, 0x0994),
        (899, 0x08A8, 0x09A4),
        (899, 0x08D4, 0x09D0),
        (899, 0x091C, 0x0A14),
        (899, 0x0934, 0x0A38),
        (899, 0x0944, 0x0A48),
        (899, 0x09C8, 0x0AD4),
        (899, 0x09D8, 0x0AF0),
        (899, 0x09EC, 0x0B04),
        (899, 0x0B18, 0x0C30),
        (899, 0x0B28, 0x0C4C),
        (899, 0x0B34, 0x0C58),
        (899, 0x0B54, 0x0C7C),
        (899, 0x0B64, 0x0C88),
        (899, 0x0B6C, 0x0C98),
        (899, 0x0CD0, 0x0DF8),
        (899, 0x0CE8, 0x0E08),
        (899, 0x0D00, 0x0E1C),
        (899, 0x0D1C, 0x0E34),
        (899, 0x0D38, 0x0E44),
        (899, 0x0D50, 0x0E60),
    ];
    let mut out = FIXED.to_vec();
    // The Seru-magic effect lines: MP x4, recover, three cures, then INT /
    // SPD / ATK / AGL / DEF x4 - one table order on both builds.
    const USA_EFFECT: [u32; 24] = [
        0xE20, 0xE44, 0xE68, 0xE8C, 0xEB0, 0xED8, 0xEFC, 0xF1C, 0xF38, 0xF5C, 0xF80, 0xFA4, 0xFC8,
        0xFEC, 0x1010, 0x1034, 0x1058, 0x107C, 0x10A0, 0x10C4, 0x10E8, 0x110C, 0x1130, 0x1154,
    ];
    const ES_EFFECT: [u32; 24] = [
        0xDF8, 0xE2C, 0xE60, 0xE94, 0xEC8, 0xEFC, 0xF34, 0xF5C, 0xF78, 0xFAC, 0xFE0, 0x1014,
        0x1048, 0x107C, 0x10B0, 0x10E4, 0x1118, 0x114C, 0x1180, 0x11B4, 0x11E8, 0x121C, 0x1250,
        0x1284,
    ];
    out.extend(USA_EFFECT.iter().zip(ES_EFFECT).map(|(&u, s)| (898, u, s)));
    for k in 0..4u32 {
        out.push((898, 0x1178 + k * 0x24, 0x12B8 + k * 0x34));
    }
    out
}

/// Fill the `system_text` entries: the SCUS pools have no located base of
/// their own, so the source window is the USA window carried by the data
/// segment's `drift` (the located name tables' displacement) with a margin,
/// and the chunks pair as for an overlay pool.
pub(super) fn lift_scus_pools(
    usa_exe: &[u8],
    src_exe: &[u8],
    drift: i64,
    by_ref: &BTreeMap<String, Vec<u8>>,
    pack: &mut LanguagePack,
    report: &mut LiftReport,
) {
    for pool in ui::SCUS_STRING_POOLS {
        if pool.strict {
            let same = same_layout_pairs(usa_exe, src_exe, pool);
            for e in pack.sections.system_text.iter_mut() {
                let Some(va) = key_scus_va(&e.key) else {
                    continue;
                };
                if !(pool.va_start..pool.va_end).contains(&va) {
                    continue;
                }
                report.system_total += 1;
                if let Some(bytes) = by_ref.get(&e.key) {
                    e.translation = markup::decode(bytes);
                    report.system_paired += 1;
                    report.pool_ref_paired += 1;
                } else if let Some(bytes) = same.get(&((va - pool.base_va) as usize)) {
                    e.translation = markup::decode(bytes);
                    report.system_paired += 1;
                }
            }
            continue;
        }
        let lo = (pool.va_start - pool.base_va) as usize;
        let hi = (pool.va_end - pool.base_va) as usize;
        let span = hi - lo;
        // Both windows carry the same run-in and run-out (the debug labels
        // and pointer bytes around the pool are build-invariant), which is
        // what anchors the pairing where the pool's own strings all differ.
        let usa_chunks = nul_chunks(usa_exe, lo.saturating_sub(0x40), hi + 0x80);
        // The data segment does not drift uniformly: the pools ahead of the
        // name tables sit at the same offsets on every build, the ones
        // behind them carry the tables' displacement. Try both and keep the
        // window that shares more build-invariant chunks with the USA one
        // (the run-in and run-out bytes), then the one pairing more of the
        // pool's own strings: an in-order alignment pairs *some* text in any
        // window, so the pair count alone can crown the wrong one.
        let pool_offs: Vec<usize> = ui::scan_pool(usa_exe, pool)
            .iter()
            .map(|s| (s.va - pool.base_va) as usize)
            .collect();
        let map = [0i64, drift]
            .into_iter()
            .map(|d| {
                let s_lo = (lo as i64 + d - 0x40).max(0) as usize;
                let s_hi = (hi as i64 + d + span as i64 + 0x80).max(0) as usize;
                let src_chunks = nul_chunks(src_exe, s_lo, s_hi);
                let shared = usa_chunks
                    .iter()
                    .filter(|(_, b)| src_chunks.iter().any(|(_, c)| c == b))
                    .count();
                (shared, pair_chunk_lists(&usa_chunks, &src_chunks))
            })
            .max_by_key(|(shared, m)| {
                (
                    *shared,
                    pool_offs.iter().filter(|o| m.contains_key(o)).count(),
                )
            })
            .map(|(_, m)| m)
            .unwrap_or_default();
        for e in pack.sections.system_text.iter_mut() {
            let Some(va) = key_scus_va(&e.key) else {
                continue;
            };
            if va < pool.va_start || va >= pool.va_end {
                continue;
            }
            report.system_total += 1;
            let off = (va - pool.base_va) as usize;
            if !map.contains_key(&off)
                && let Some(bytes) = by_ref.get(&e.key)
            {
                e.translation = markup::decode(bytes);
                report.system_paired += 1;
                report.pool_ref_paired += 1;
                continue;
            }
            if let Some(bytes) = map.get(&off) {
                e.translation = markup::decode(bytes);
                report.system_paired += 1;
            }
        }
    }
}

/// Fill the `place_names` cells: the source exe's cell table is found by
/// its first cell (the home town's name, a proper noun every build shares)
/// near the drifted USA address, and the cells pair id-for-id.
pub(super) fn lift_place_cells(
    usa_exe: &[u8],
    src_exe: &[u8],
    drift: i64,
    pack: &mut LanguagePack,
    report: &mut LiftReport,
) {
    const STRIDE: usize = worldmap_menu::NAME_STRIDE;
    let usa_va = worldmap_menu::NAME_TABLE_ADDR;
    let Some(usa_off) = item_names::file_offset_for_va(usa_exe, usa_va) else {
        return;
    };
    let Some(first) = usa_exe.get(usa_off..usa_off + STRIDE) else {
        return;
    };
    let centre = (usa_off as i64 + drift).max(0) as usize;
    let lo = centre.saturating_sub(0x1000);
    let hi = (centre + 0x1000).min(src_exe.len().saturating_sub(STRIDE));
    let hit = (lo..=hi)
        .filter(|&o| o.is_multiple_of(4) && &src_exe[o..o + STRIDE] == first)
        .min_by_key(|&o| o.abs_diff(centre));
    let Some(src_off) = hit else {
        report.cells_total += pack.sections.place_names.len();
        return;
    };
    for e in pack.sections.place_names.iter_mut() {
        let Some(hex) = e.key.strip_prefix("scus:cell:0x") else {
            continue;
        };
        let Ok(va) = u32::from_str_radix(hex, 16) else {
            continue;
        };
        report.cells_total += 1;
        let Some(n) = va
            .checked_sub(usa_va)
            .map(|d| d as usize / STRIDE)
            .filter(|&n| n < worldmap_menu::NAME_COUNT)
        else {
            continue;
        };
        let Some(cell) = src_exe.get(src_off + n * STRIDE..src_off + (n + 1) * STRIDE) else {
            continue;
        };
        let len = cell.iter().position(|&b| b == 0).unwrap_or(cell.len());
        if len == 0 || cell[..len].iter().any(|&b| b < 0x20) {
            continue;
        }
        e.translation = markup::decode(&cell[..len]);
        report.cells_paired += 1;
    }
}

/// Fill the `monster_names` entries id-for-id from the source disc's own
/// archive: the monster ids are the same program on every build.
pub(super) fn lift_monster_names(
    target: &DiscPatcher,
    source: &DiscPatcher,
    pack: &mut LanguagePack,
    report: &mut LiftReport,
) {
    report.monsters_total = pack.sections.monster_names.len();
    let (Ok(_), Ok(src)) = (
        target.read_entry(legaia_disc_patch::disc::MONSTER_ARCHIVE_ENTRY),
        source.read_entry(legaia_disc_patch::disc::MONSTER_ARCHIVE_ENTRY),
    ) else {
        return;
    };
    let names: BTreeMap<u16, Vec<u8>> = monster_names::fields(&src)
        .into_iter()
        .map(|(id, f)| (id, f.bytes))
        .collect();
    for e in pack.sections.monster_names.iter_mut() {
        if let Some(bytes) = monster_names::key_id(&e.key).and_then(|id| names.get(&id)) {
            e.translation = markup::decode(bytes);
            report.monsters_paired += 1;
        }
    }
}
