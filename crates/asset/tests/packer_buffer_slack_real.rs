//! Disc-gated: the last-sector slack of every scene bundle is the packer's
//! buffer, predicted byte for byte by TOC order.
//!
//! A `scene_asset_table` bundle's content ends where its last descriptor's LZS
//! stream stops being consumed (`legaia_lzs::decompress_tracked`), always
//! inside the entry's last sector. The bytes above are not this bundle's: the
//! packer wrote every PROT entry out of one buffer it never cleared, so file
//! offset `k` of that slack holds the byte the nearest earlier entry reaching
//! `k` holds there ([`legaia_asset::inherited_tail::buffer_run`]).
//!
//! The prediction has no free parameter - the donor is fixed by the TOC order
//! and the entry lengths - so a bundle whose slack it misses by one byte fails
//! here. Skips + passes without `extracted/PROT`.

use legaia_asset::byte_account::{AccountOptions, OWNER_INHERITED_TAIL, account};
use legaia_asset::inherited_tail::{self, SECTOR_BYTES};
use legaia_asset::scene_asset_table;
use std::path::PathBuf;

fn prot_dir() -> Option<PathBuf> {
    for c in [
        "extracted/PROT",
        "../extracted/PROT",
        "../../extracted/PROT",
    ] {
        let d = PathBuf::from(c);
        if d.is_dir() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/PROT missing - run `legaia-extract` first");
    None
}

fn entries(dir: &PathBuf) -> Vec<(u32, PathBuf)> {
    let mut v: Vec<(u32, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let n = p.file_name()?.to_str()?.to_string();
            let idx = legaia_asset::byte_account::prot_index_from_name(&n)?;
            Some((idx, p))
        })
        .collect();
    v.sort();
    v.dedup_by_key(|e| e.0);
    v
}

/// End of the last descriptor's compressed stream, as consumed.
fn bundle_content_end(buf: &[u8]) -> Option<usize> {
    let r = scene_asset_table::resolve(buf)?;
    if r.table_base != 0 {
        return None;
    }
    let mut end = 8 + r.table.count * 8;
    for d in r.table.used() {
        let off = d.data_offset as usize;
        let (_, consumed) =
            legaia_lzs::decompress_tracked(buf.get(off..)?, d.size as usize).ok()?;
        end = end.max(off + consumed);
    }
    Some(end)
}

#[test]
fn every_scene_bundles_slack_is_the_packer_buffer() {
    let Some(dir) = prot_dir() else { return };
    let mut bundles = 0usize;
    let mut slack_bytes = 0usize;
    let mut zero_only = 0usize;
    let mut misses = Vec::new();
    for (idx, path) in entries(&dir) {
        let buf = std::fs::read(&path).expect("read entry");
        let Some(end) = bundle_content_end(&buf) else {
            continue;
        };
        bundles += 1;
        assert!(
            buf.len() - end < SECTOR_BYTES,
            "PROT {idx:04}: content ends a whole sector below the entry end"
        );
        if end == buf.len() {
            continue;
        }
        slack_bytes += buf.len() - end;
        match inherited_tail::buffer_run(&dir, idx, &buf, end) {
            Some(pieces) => {
                if pieces.iter().all(|p| p.donor.is_none()) {
                    zero_only += 1;
                }
            }
            None => misses.push(idx),
        }
    }
    eprintln!(
        "[packer-buffer] {bundles} bundles, {slack_bytes} B of slack, {zero_only} zero-only \
         (no earlier entry reached), misses {misses:?}"
    );
    assert!(bundles >= 80, "found only {bundles} scene bundles");
    assert!(misses.is_empty(), "slack not reproduced: {misses:?}");
}

/// The byte account claims that slack, and names the donor.
#[test]
fn the_byte_account_claims_a_bundles_slack_as_an_inherited_tail() {
    let Some(dir) = prot_dir() else { return };
    // PROT 0013 (town0b): content ends at 0x3808C, the rest is PROT 0005's.
    let Some((_, path)) = entries(&dir).into_iter().find(|e| e.0 == 13) else {
        return;
    };
    let buf = std::fs::read(&path).expect("read 0013");
    let acc = account(
        &buf,
        &AccountOptions {
            prot_index: Some(13),
            prot_dir: Some(dir.clone()),
            depth: 0,
            ..Default::default()
        },
    );
    let tail = acc
        .by_owner
        .iter()
        .find(|o| o.owner == OWNER_INHERITED_TAIL)
        .expect("an inherited_tail claim");
    assert_eq!(tail.bytes, buf.len() - 0x3808C);
    assert_eq!(acc.residue_bytes, 0);

    // Without the sibling entries there is nothing to compare, and no claim.
    let bare = account(
        &buf,
        &AccountOptions {
            prot_index: Some(13),
            depth: 0,
            ..Default::default()
        },
    );
    assert!(
        bare.by_owner
            .iter()
            .all(|o| o.owner != OWNER_INHERITED_TAIL)
    );
}
