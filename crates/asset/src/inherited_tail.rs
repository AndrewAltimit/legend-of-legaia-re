//! Where an overlay image stops being its own content.
//!
//! A PROT entry's extent is sector-granular, and the packer that built the disc
//! wrote each overlay into a buffer it did **not** clear first. A module shorter
//! than the buffer therefore flushes its own bytes and then whatever the
//! previous, longer module left there - and the extraction hands that residue
//! back inside the entry.
//!
//! That residue is measurable rather than inferred. Two images are compared byte
//! for byte at the SAME file offset: where a sibling reproduces this image's
//! bytes from some offset all the way to the end of this image's content, the
//! run from that offset is this image's **inherited tail**. It is the sibling's
//! code, sitting at the same offset it sits at there.
//!
//! This is the Rust side of `scripts/ghidra-analysis/inherited_tail.py`, which
//! states the rule in full and records what each earlier restriction lost. The
//! two are held to each other cut for cut:
//! `rust_and_python_tails_agree` in `crates/asset/tests/inherited_tail_real.rs`
//! runs the Python module over the same corpus and diffs the result, and they
//! agree on every mapped image.
//!
//! ## One buffer, in TOC order
//!
//! The buffer is one buffer for the whole of `PROT.DAT`, filled in extraction
//! order, so the residue at file offset `k` is the byte the **nearest earlier
//! entry whose extent reaches `k`** holds there ([`buffer_run`]). That is a
//! prediction with no free parameter, and it reproduces every overlay cut the
//! sibling comparison below makes, offset for offset; where the two name
//! different donors, the comparison has named the image that first wrote the
//! bytes and the prediction the one the buffer last held them from. The
//! sibling comparison stays the overlays' cut because an overlay's own-content
//! end is itself a measurement the cut feeds back into; every other entry's
//! parser states its content end outright, and the byte account tests the
//! slack above it against [`buffer_run`] directly.
//!
//! ## Why the byte account needs it
//!
//! [`crate::byte_account`] measures what a parser here consumed against the
//! entry's own bytes. A tail is another module's code, so no parser of *this*
//! entry can ever consume it - counting it as residue puts work on the worklist
//! that no work can close, and gives the 0975-shaped runs ("PROT 0972's code,
//! byte-identical at the same file offset") the shape of un-dumped code.
//! `disc-coverage.py` has cut tails out of its denominator since the rule was
//! found; the byte account did not, and the asymmetry between the two
//! instruments was the gap.
//!
//! ## Where the images come from
//!
//! Every row of `crates/asset/data/static-overlays.toml` is `form = "raw"` with
//! `content_source = "prot_entry_extent"`, and each row's `content_bytes`
//! equals its extracted PROT entry's file length exactly, so the entry file IS
//! the as-loaded image and no `extracted/overlays/` tree is needed. That
//! equality is asserted rather than assumed
//! (`crates/asset/tests/inherited_tail_real.rs`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Shortest run that may be called a tail. At `0x40` the match is sixteen
/// instructions long AND runs to the end of the file, which no shared library
/// routine does unless it is the last thing linked.
pub const MIN_TAIL_BYTES: usize = 0x40;

/// Rounds the cut / own-content fixpoint is allowed before it gives up. The
/// retail band settles in two.
pub const MAX_ROUNDS: usize = 8;

/// Every mapped overlay's tail, shared between the callers that ask for it.
pub type TailMap = std::sync::Arc<BTreeMap<u32, Tail>>;

/// One image's inherited tail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tail {
    /// File offset at which this image stops being its own content.
    pub start: usize,
    /// PROT index of the image whose bytes those are.
    pub donor_prot_index: u32,
    /// `static-overlays.toml` label of that image.
    pub donor_label: String,
    /// Length of the image this cut was measured on. A caller holding a buffer
    /// of some other length (a decompressed payload, a nested pass) is not
    /// holding this image, and `start` means nothing there.
    pub image_bytes: usize,
}

/// First offset from which `a` and `b` agree through the end of `a`.
///
/// `b` must be at least as long as `a`; only `b[..a.len()]` is read.
fn suffix_start(a: &[u8], b: &[u8]) -> usize {
    let mut i = a.len();
    while i > 0 && a[i - 1] == b[i - 1] {
        i -= 1;
    }
    i
}

/// One image in the comparison set.
struct Image {
    prot_index: u32,
    label: String,
    base_va: u32,
    data: Vec<u8>,
}

/// `{prot_index -> (offset, donor prot_index)}` for every image with a tail.
///
/// `own_ends` is `{prot_index -> structural own-content end in bytes}`. It gates
/// the equal-extent leg only: without it, equal-extent donors are not considered
/// and the result is the "any base, strictly longer" figure. A donor need not
/// share this image's base - the mastering buffer is indexed by file offset.
fn tail_starts(
    images: &[Image],
    min_tail: usize,
    own_ends: Option<&BTreeMap<u32, usize>>,
) -> BTreeMap<u32, (usize, u32)> {
    let mut out = BTreeMap::new();
    for img in images {
        let mut best: Option<(usize, u32)> = None;
        for other in images {
            if other.prot_index == img.prot_index || other.data.len() < img.data.len() {
                continue;
            }
            let equal_extent = other.data.len() == img.data.len();
            let Some(own) = own_ends else {
                if equal_extent {
                    continue;
                }
                let start = suffix_start(&img.data, &other.data[..img.data.len()]);
                if img.data.len() - start >= min_tail && best.is_none_or(|(b, _)| start < b) {
                    best = Some((start, other.prot_index));
                }
                continue;
            };
            let start = suffix_start(&img.data, &other.data[..img.data.len()]);
            if img.data.len() - start < min_tail {
                continue;
            }
            // An equal-extent sibling is a donor only where the bytes can say
            // so: its own content must reach above the shared suffix and this
            // image's must not. Absent either measurement, decline. (A sector
            // extent is not a content length, so two modules that round to the
            // same sector count can still differ in what they hold.)
            if equal_extent
                && !(own.get(&other.prot_index).copied().unwrap_or(0) > start
                    && own.get(&img.prot_index).copied().unwrap_or(img.data.len()) <= start)
            {
                continue;
            }
            if best.is_none_or(|(b, _)| start < b) {
                best = Some((start, other.prot_index));
            }
        }
        if let Some(b) = best {
            out.insert(img.prot_index, b);
        }
    }
    out
}

/// [`tail_starts`] iterated until the cuts stop moving.
///
/// The cut and the own-content measurement are mutually recursive - the cut
/// needs the measurement, and the measurement is wrong until the cut is applied,
/// so this iterates the pair instead of taking the first estimate. A slot-B
/// module's spawn-record chain walks straight on into its donor's residue when
/// measured over the whole image, and an overshoot there is a claim about the
/// donor question the figure exists to answer.
///
/// Returns `(cuts, rounds)`. `rounds == MAX_ROUNDS` means it did not converge,
/// so a caller can report a non-convergence rather than print round eight of an
/// oscillation.
fn tail_starts_fixpoint(images: &[Image], min_tail: usize) -> (BTreeMap<u32, (usize, u32)>, usize) {
    let mut cuts: BTreeMap<u32, (usize, u32)> = BTreeMap::new();
    for round_no in 1..=MAX_ROUNDS {
        let mut own = BTreeMap::new();
        for img in images {
            let cut = cuts
                .get(&img.prot_index)
                .map(|c| c.0)
                .unwrap_or(img.data.len());
            own.insert(
                img.prot_index,
                crate::slot_b_module::content_end(&img.data[..cut], img.base_va),
            );
        }
        let next = tail_starts(images, min_tail, Some(&own));
        if next == cuts {
            return (cuts, round_no);
        }
        cuts = next;
    }
    (cuts, MAX_ROUNDS)
}

/// Every mapped overlay's tail, keyed by PROT extraction index.
///
/// `prot_dir` is a directory of extracted PROT entries (`extracted/PROT`). An
/// entry a row names but the directory does not hold is skipped, so a partial
/// extraction measures fewer donors rather than reporting a wrong cut.
pub fn tails_in(prot_dir: &Path) -> BTreeMap<u32, Tail> {
    let mut images: Vec<Image> = Vec::new();
    for row in &crate::static_overlay::overlay_map().overlays {
        let Some(path) = entry_path(prot_dir, row.prot_index) else {
            continue;
        };
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        images.push(Image {
            prot_index: row.prot_index,
            label: row.label.clone(),
            base_va: row.base_va,
            data,
        });
    }
    let labels: BTreeMap<u32, String> = images
        .iter()
        .map(|i| (i.prot_index, i.label.clone()))
        .collect();
    let lengths: BTreeMap<u32, usize> = images
        .iter()
        .map(|i| (i.prot_index, i.data.len()))
        .collect();
    let (cuts, _rounds) = tail_starts_fixpoint(&images, MIN_TAIL_BYTES);
    cuts.into_iter()
        .map(|(idx, (start, donor))| {
            (
                idx,
                Tail {
                    start,
                    donor_prot_index: donor,
                    donor_label: labels.get(&donor).cloned().unwrap_or_default(),
                    image_bytes: lengths.get(&idx).copied().unwrap_or(0),
                },
            )
        })
        .collect()
}

/// [`tails_in`], memoised per directory.
///
/// The byte-account sweep runs one process per PROT entry, so the map is built
/// at most once per run; inside a process that accounts many entries (a test, a
/// nested walk) the cache keeps it to one.
pub fn tails_cached(prot_dir: &Path) -> TailMap {
    static CACHE: OnceLock<Mutex<BTreeMap<PathBuf, TailMap>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    let key = prot_dir.to_path_buf();
    if let Some(hit) = cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return hit;
    }
    let built = std::sync::Arc::new(tails_in(prot_dir));
    if let Ok(mut c) = cache.lock() {
        c.insert(key, built.clone());
    }
    built
}

/// Byte length of the sector-granular slack an entry can inherit: the part of
/// its **last sector** above its own content. A run reaching a whole sector
/// below the entry end is not residue of the buffer - the packer wrote those
/// sectors from this file.
pub const SECTOR_BYTES: usize = 0x800;

/// Every extracted entry's path and length, in extraction (= mastering) order.
type EntryIndex = std::sync::Arc<BTreeMap<u32, (PathBuf, usize)>>;

fn entry_index_cached(prot_dir: &Path) -> EntryIndex {
    static CACHE: OnceLock<Mutex<BTreeMap<PathBuf, EntryIndex>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    let key = prot_dir.to_path_buf();
    if let Some(hit) = cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return hit;
    }
    let mut map = BTreeMap::new();
    if let Ok(rd) = std::fs::read_dir(prot_dir) {
        for e in rd.flatten() {
            let path = e.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(idx) = crate::byte_account::prot_index_from_name(name) else {
                continue;
            };
            let Ok(meta) = e.metadata() else { continue };
            // First name wins, the same tie-break `entry_path` applies.
            map.entry(idx)
                .and_modify(|cur: &mut (PathBuf, usize)| {
                    if path < cur.0 {
                        *cur = (path.clone(), meta.len() as usize);
                    }
                })
                .or_insert((path.clone(), meta.len() as usize));
        }
    }
    let built = std::sync::Arc::new(map);
    if let Ok(mut c) = cache.lock() {
        c.insert(key, built.clone());
    }
    built
}

/// One contiguous piece of a [mastering-buffer run](buffer_run): `[start, end)`
/// of the recipient, reproduced by entry `donor` at the same file offsets
/// (`None` = no earlier entry reached that far, and the bytes are zero).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferPiece {
    pub start: usize,
    pub end: usize,
    pub donor: Option<u32>,
}

/// Is `buf[start..]` exactly what the packer's buffer held there before entry
/// `idx` was written?
///
/// The packer that built `PROT.DAT` used **one** buffer for every entry, in
/// extraction order, and never cleared it: an entry's last sector is its own
/// bytes and then whatever the buffer held above them. So the byte at file
/// offset `k` of that slack is the byte at offset `k` of the **nearest earlier
/// entry whose extent reaches `k`** - or zero where no earlier entry reached
/// that far, which is the buffer as first allocated. That is a prediction with
/// no free parameter: the donor is fixed by the TOC order and the entry
/// lengths, not chosen as the best-matching sibling.
///
/// Returns the run split by donor when every byte of `buf[start..]` matches the
/// prediction, and `None` when any byte does not, when `start` is a whole
/// sector or more below the end (a sector the packer wrote from this file is
/// not slack), or when an entry the prediction needs cannot be read. `buf` must
/// be entry `idx` itself - a buffer of another length is refused.
pub fn buffer_run(prot_dir: &Path, idx: u32, buf: &[u8], start: usize) -> Option<Vec<BufferPiece>> {
    if start >= buf.len() || buf.len() - start >= SECTOR_BYTES {
        return None;
    }
    let (pieces, predicted) = predict(prot_dir, idx, buf.len(), start)?;
    (predicted == buf[start..]).then_some(pieces)
}

/// Lowest offset from which [`buffer_run`] reproduces `buf` through its end,
/// searched inside the last sector only, or `None` when fewer than
/// [`MIN_TAIL_BYTES`] match.
///
/// This is the form for an entry whose own-content end no parser states - a
/// code image whose data segment runs past its last dumped function. The run is
/// a suffix match, so a coincidental agreement just below the true end (a few
/// zero bytes) can move the start down by that much; the caller rounds to its
/// own alignment if it has one.
pub fn buffer_suffix_start(prot_dir: &Path, idx: u32, buf: &[u8]) -> Option<usize> {
    let lo = buf.len().saturating_sub(SECTOR_BYTES - 1);
    let (_, predicted) = predict(prot_dir, idx, buf.len(), lo)?;
    let own = &buf[lo..];
    let mut i = own.len();
    while i > 0 && own[i - 1] == predicted[i - 1] {
        i -= 1;
    }
    (own.len() - i >= MIN_TAIL_BYTES).then_some(lo + i)
}

/// The buffer's bytes at `[start, len)` just before entry `idx` (of length
/// `len`) was written, split by donor.
fn predict(
    prot_dir: &Path,
    idx: u32,
    len: usize,
    start: usize,
) -> Option<(Vec<BufferPiece>, Vec<u8>)> {
    use std::io::{Read, Seek, SeekFrom};
    let entries = entry_index_cached(prot_dir);
    let (_, own_len) = entries.get(&idx)?;
    if *own_len != len || start >= len {
        return None;
    }
    let mut pieces: Vec<BufferPiece> = Vec::new();
    let mut bytes = Vec::with_capacity(len - start);
    let mut k = start;
    while k < len {
        // Nearest earlier entry whose extent covers offset `k`.
        let donor = entries
            .range(..idx)
            .rev()
            .find(|(_, (_, l))| *l > k)
            .map(|(i, (p, l))| (*i, p.clone(), *l));
        let end = match &donor {
            Some((_, _, l)) => (*l).min(len),
            None => len,
        };
        match &donor {
            Some((_, path, _)) => {
                let mut f = std::fs::File::open(path).ok()?;
                f.seek(SeekFrom::Start(k as u64)).ok()?;
                let mut want = vec![0u8; end - k];
                f.read_exact(&mut want).ok()?;
                bytes.extend_from_slice(&want);
            }
            None => bytes.resize(bytes.len() + (end - k), 0),
        }
        pieces.push(BufferPiece {
            start: k,
            end,
            donor: donor.map(|d| d.0),
        });
        k = end;
    }
    Some((pieces, bytes))
}

/// Path of extraction entry `idx` under `prot_dir`, by the `NNNN_` prefix the
/// extractor names every entry with.
fn entry_path(prot_dir: &Path, idx: u32) -> Option<PathBuf> {
    let prefix = format!("{idx:04}_");
    let mut hits: Vec<PathBuf> = std::fs::read_dir(prot_dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix))
        })
        .collect();
    hits.sort();
    hits.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_start_is_the_first_agreeing_offset() {
        assert_eq!(suffix_start(b"abcdef", b"zzzdef"), 3);
        assert_eq!(suffix_start(b"abcdef", b"abcdef"), 0);
        assert_eq!(suffix_start(b"abcdef", b"abcdeX"), 6);
    }

    #[test]
    fn a_strictly_longer_sibling_donates_without_an_own_end_measurement() {
        let images = vec![
            Image {
                prot_index: 1,
                label: "short".into(),
                base_va: 0x8000_0000,
                data: [vec![0x11; 0x40], vec![0xAB; MIN_TAIL_BYTES]].concat(),
            },
            Image {
                prot_index: 2,
                label: "long".into(),
                base_va: 0x8000_0000,
                data: [
                    vec![0x22; 0x40],
                    vec![0xAB; MIN_TAIL_BYTES],
                    vec![0x33; 0x40],
                ]
                .concat(),
            },
        ];
        let cuts = tail_starts(&images, MIN_TAIL_BYTES, None);
        assert_eq!(cuts.get(&1), Some(&(0x40, 2)));
        assert_eq!(cuts.get(&2), None);
    }

    #[test]
    fn an_equal_extent_sibling_is_declined_without_own_ends() {
        let images = vec![
            Image {
                prot_index: 1,
                label: "a".into(),
                base_va: 0x8000_0000,
                data: [vec![0x11; 0x40], vec![0xAB; MIN_TAIL_BYTES]].concat(),
            },
            Image {
                prot_index: 2,
                label: "b".into(),
                base_va: 0x8000_0000,
                data: [vec![0x22; 0x40], vec![0xAB; MIN_TAIL_BYTES]].concat(),
            },
        ];
        assert!(tail_starts(&images, MIN_TAIL_BYTES, None).is_empty());
    }

    #[test]
    fn a_run_under_the_minimum_is_not_a_tail() {
        let images = vec![
            Image {
                prot_index: 1,
                label: "short".into(),
                base_va: 0x8000_0000,
                data: [vec![0x11; 0x40], vec![0xAB; MIN_TAIL_BYTES - 1]].concat(),
            },
            Image {
                prot_index: 2,
                label: "long".into(),
                base_va: 0x8000_0000,
                data: [
                    vec![0x22; 0x40],
                    vec![0xAB; MIN_TAIL_BYTES - 1],
                    vec![0x33; 0x40],
                ]
                .concat(),
            },
        ];
        assert!(tail_starts(&images, MIN_TAIL_BYTES, None).is_empty());
    }
}
