//! Disc-gated: the two spans the slot-B parser used to decline, over all 64
//! band entries.
//!
//! [`legaia_asset::slot_b_module`] bounds a spawn record by the *next*
//! consumer pointer, which leaves two holes this file closes and then guards:
//!
//! 1. **The highest record.** Nothing above it computes an address, so the band
//!    cannot bound it. Its move-VM program can: `move_program_end` walks the
//!    opcode widths to a terminator - `0x08` HALT, or an armed `0x19` / `0x1B`
//!    idle loop whose `0x18` / `0x1A` counter carries bit `0x4000` - and rounds
//!    the end up to the 4-byte boundary the records are laid out on. The test
//!    that this is a rule about the format rather than a coincidence is that it
//!    reproduces the ends the band DOES bound: chaining `[header][program]`
//!    from every bounded record's start lands exactly on that record's measured
//!    end for the overwhelming majority of the band's bounded extents.
//!
//! 2. **Donor call sites.** A band image's inherited tail is a byte-identical,
//!    same-offset copy of a longer image's bytes, and a whole function of the
//!    donor can sit there, frame-match locally and issue the donor's spawn
//!    calls. The call-site filter ("inside a framed body of this image") cannot
//!    see the difference; the image's own content end can. `parse_with_tail`
//!    takes it and drops every call site and record target at or above it.
//!
//! Both invariants matter outside this crate: `asset account` credits the
//! record claims, and `scripts/ci/disc-coverage.py`'s `spawn_record_band` shape
//! takes them out of the code denominator, so a wrong claim retires real code
//! from the dump worklist.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_asset::slot_b_module::{
    self, ProgramEnd, SLOT_B_LINK_BASE, SLOT_B_PROT_FIRST, SLOT_B_PROT_LAST,
};
use std::path::PathBuf;

/// Bounded extents the chain must reproduce, as a share of all of them. The
/// floor sits below the measured figure on purpose: the assertion is that the
/// rule holds over the band, not that its residue is a particular size.
const CHAIN_EXACT_FLOOR_PERCENT: usize = 95;

/// Smallest byte-identical, same-offset suffix two images must share before
/// this test will call it residue. Matches `inherited_tail.MIN_TAIL_BYTES`.
const MIN_TAIL_BYTES: usize = 0x40;

/// Cap on the own-content/tail fixpoint below. The band settles in two rounds;
/// the cap exists because the two legs pull in opposite directions (cutting a
/// recipient makes it more recipient-shaped, cutting a donor less
/// donor-shaped), so a pathological pair could alternate forever.
const MAX_TAIL_ROUNDS: usize = 8;

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base);
        if p.join("PROT.DAT").is_file() {
            return Some(p);
        }
    }
    None
}

/// Every band image, keyed by PROT entry.
fn band_images(dir: &std::path::Path) -> Vec<(u32, Vec<u8>)> {
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let mut out = Vec::new();
    for idx in SLOT_B_PROT_FIRST..=SLOT_B_PROT_LAST {
        let Some(entry) = archive.entries.get(idx as usize).cloned() else {
            continue;
        };
        let mut bytes = Vec::new();
        if archive.read_entry(&entry, &mut bytes).is_ok() && bytes.len() >= 8 {
            out.push((idx, bytes));
        }
    }
    out
}

/// First offset from which `a` and `b` agree through the end of `a`.
fn suffix_start(a: &[u8], b: &[u8]) -> usize {
    let mut i = a.len();
    while i > 0 && a[i - 1] == b[i - 1] {
        i -= 1;
    }
    i
}

/// `{entry: (tail start, donor entry)}` under the rule
/// `scripts/ghidra-analysis/inherited_tail.py` states: any donor at any link
/// base, strictly longer, or equal-extent when its own content reaches above
/// the shared suffix and the recipient's does not.
///
/// `own` is the measurement that gates the equal-extent leg, and it is
/// mutually recursive with the cut: measuring it over the whole image lets a
/// record chain walk straight into the donor's residue. So it is iterated the
/// way `inherited_tail.tail_starts_fixpoint` iterates it - each round
/// re-measures `content_end` over the image cut at the previous round's tail.
fn tail_starts(imgs: &[(u32, Vec<u8>)]) -> std::collections::HashMap<u32, (usize, u32)> {
    let mut cuts: std::collections::HashMap<u32, (usize, u32)> = Default::default();
    for _ in 0..MAX_TAIL_ROUNDS {
        let own: Vec<usize> = imgs
            .iter()
            .map(|(idx, b)| {
                let end = cuts.get(idx).map_or(b.len(), |(c, _)| *c);
                slot_b_module::content_end(&b[..end], SLOT_B_LINK_BASE)
            })
            .collect();
        let next = tail_starts_round(imgs, &own);
        if next == cuts {
            return cuts;
        }
        cuts = next;
    }
    cuts
}

/// One round of the tail rule at a fixed `own` measurement.
fn tail_starts_round(
    imgs: &[(u32, Vec<u8>)],
    own: &[usize],
) -> std::collections::HashMap<u32, (usize, u32)> {
    let mut out = std::collections::HashMap::new();
    for (i, (idx, data)) in imgs.iter().enumerate() {
        let mut best: Option<(usize, u32)> = None;
        for (j, (other_idx, other)) in imgs.iter().enumerate() {
            if i == j || other.len() < data.len() {
                continue;
            }
            let equal = other.len() == data.len();
            let start = suffix_start(data, &other[..data.len()]);
            if data.len() - start < MIN_TAIL_BYTES {
                continue;
            }
            if equal && !(own[j] > start && own[i] <= start) {
                continue;
            }
            if best.is_none_or(|(b, _)| start < b) {
                best = Some((start, *other_idx));
            }
        }
        if let Some(b) = best {
            out.insert(*idx, b);
        }
    }
    out
}

/// The set of `model_sel` values `FUN_80021B04` dispatches - a header outside it
/// is not a record. Re-derived here rather than taken from the parser.
fn dispatchable(sel: i16) -> bool {
    sel == -1 || (0..0x100).contains(&sel) || sel == 0x4000 || sel == 0x4001
}

/// The record-end rule, as a free function, so the chain assertion below is not
/// the parser checked against itself at the same entry point.
fn program_end(bytes: &[u8], record: usize) -> Option<usize> {
    match slot_b_module::move_program_end(bytes, record + 4) {
        ProgramEnd::Halt(e) | ProgramEnd::IdleLoop(e) => Some(e),
        ProgramEnd::Unterminated(_) => None,
    }
}

/// 1. The record-end rule reproduces the ends the band independently bounds.
///
/// For each extent the consumer pointers bound on BOTH sides, chain
/// `[header][program]` records from its start. The chain must land exactly on
/// the measured end - never past it - for at least
/// [`CHAIN_EXACT_FLOOR_PERCENT`] of them.
///
/// The image's **highest** record is excluded on purpose: its end comes from
/// this very walk, so including it would be the rule checked against itself
/// and would inflate the rate by one record per image.
#[test]
fn the_program_walk_reproduces_the_bounded_record_extents() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] slot-B record bounds: set LEGAIA_DISC_BIN + extracted/");
        return;
    };
    let imgs = band_images(&dir);
    assert!(imgs.len() >= 60, "band images: {}", imgs.len());

    // `over` - the chain stepped PAST the measured end. `stalled` - it stopped
    // below the end, on a halfword that is no opcode or on a non-terminating
    // instruction the walk cannot step over. The two shapes are different
    // residues and the page that quotes this figure has to say which it is.
    let (mut total, mut exact, mut over, mut stalled) = (0usize, 0usize, 0usize, 0usize);
    for (idx, bytes) in &imgs {
        let layout = slot_b_module::parse(bytes);
        let top = layout.record_offsets.last().copied();
        for r in &layout.records {
            if Some(r.start) == top {
                continue;
            }
            total += 1;
            let mut p = r.start;
            let mut landed = false;
            for _ in 0..64 {
                match program_end(bytes, p) {
                    Some(q) if q > p && q <= r.end => {
                        p = q;
                        if p == r.end {
                            landed = true;
                            break;
                        }
                        // The next record must open with a dispatchable
                        // selector, or this is not a chain.
                        let sel = i16::from_le_bytes([bytes[p], bytes[p + 1]]);
                        if p + 4 > r.end || !dispatchable(sel) {
                            break;
                        }
                    }
                    Some(_) => {
                        over += 1;
                        break;
                    }
                    None => break,
                }
            }
            if landed {
                exact += 1;
            } else if p < r.end {
                stalled += 1;
            }
        }
        assert!(
            layout.records.len() + layout.chained_records.len() < 4096,
            "PROT {idx}: runaway record count"
        );
    }
    assert!(total >= 900, "bounded record extents: {total}");
    let pct = exact * 100 / total;
    assert!(
        pct >= CHAIN_EXACT_FLOOR_PERCENT,
        "the program walk reproduces only {exact} of {total} bounded record \
         extents ({pct}%), {over} of them by overrunning the measured end"
    );
    eprintln!(
        "[ok] program walk lands exactly on {exact} of {total} bounded record \
         extents; {over} overran the measured end, {stalled} stalled below it"
    );
}

/// 2. Every image's highest record is either bounded by its program or
///    disclosed as unbounded - and a bounded one never runs past the image.
#[test]
fn the_highest_record_is_bounded_by_its_own_program_or_disclosed() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] slot-B highest record: set LEGAIA_DISC_BIN + extracted/");
        return;
    };
    let imgs = band_images(&dir);
    let tails = tail_starts(&imgs);
    let (mut bounded, mut unbounded, mut with_records) = (0usize, 0usize, 0usize);
    for (idx, bytes) in &imgs {
        let tail = tails.get(idx).map(|&(s, _)| s);
        let layout = slot_b_module::parse_with_tail(bytes, SLOT_B_LINK_BASE, tail);
        let Some(&top) = layout.record_offsets.last() else {
            continue;
        };
        with_records += 1;
        // Exactly one of the two outcomes, never both and never neither.
        let claimed = layout.records.iter().any(|r| r.start == top);
        assert_ne!(
            claimed,
            layout.unbounded_record == Some(top),
            "PROT {idx}: the highest record at {top:#x} is both claimed and disclosed, or neither"
        );
        if claimed {
            bounded += 1;
        } else {
            unbounded += 1;
        }
        let limit = tail.unwrap_or(bytes.len());
        for r in layout.records.iter().chain(&layout.chained_records) {
            assert!(
                r.end <= limit && r.start < r.end,
                "PROT {idx}: record {:#x}..{:#x} leaves the image's own content (ends {limit:#x})",
                r.start,
                r.end
            );
        }
    }
    assert!(with_records >= 60, "images with records: {with_records}");
    assert!(
        bounded * 100 / with_records >= 90,
        "only {bounded} of {with_records} highest records are bounded ({unbounded} disclosed)"
    );
    eprintln!("[ok] {bounded} of {with_records} highest records bounded, {unbounded} disclosed");
}

/// 3. The donor cut: with the image's own content end in hand, no claim - and
///    no accepted spawn call site - lands in another image's residue.
#[test]
fn no_record_claim_sits_above_the_inherited_tail_start() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] slot-B donor cut: set LEGAIA_DISC_BIN + extracted/");
        return;
    };
    let imgs = band_images(&dir);
    let tails = tail_starts(&imgs);
    assert!(
        tails.len() >= 40,
        "images with a measured tail: {}",
        tails.len()
    );

    // The cut must actually do something: at least one image must lose a claim
    // it makes without the tail. Otherwise this test passes vacuously.
    let mut cut_images = Vec::new();
    for (idx, bytes) in &imgs {
        let Some(&(tail, _donor)) = tails.get(idx) else {
            continue;
        };
        let open = slot_b_module::parse(bytes);
        let cut = slot_b_module::parse_with_tail(bytes, SLOT_B_LINK_BASE, Some(tail));
        for r in cut.records.iter().chain(&cut.chained_records) {
            assert!(
                r.end <= tail,
                "PROT {idx}: cut parse still claims {:#x}..{:#x} above the tail start {tail:#x}",
                r.start,
                r.end
            );
        }
        for &o in &cut.record_offsets {
            assert!(
                o < tail,
                "PROT {idx}: cut parse still credits a record pointer at {o:#x} \
                 above the tail start {tail:#x}"
            );
        }
        if open.record_offsets.iter().any(|&o| o >= tail) {
            cut_images.push(*idx);
        }
    }
    assert!(
        !cut_images.is_empty(),
        "no image loses a donor-sourced record pointer to the cut - the test is vacuous"
    );
    eprintln!(
        "[ok] donor cut removes at least one credited pointer on {} images: {:?}",
        cut_images.len(),
        cut_images
    );
}
