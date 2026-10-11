//! Reference-anchored string pairing across two builds of the same program.
//!
//! A pooled UI string is found by the **code or data that points at it**, and
//! that reference survives a rebuild in another language even when the pool
//! itself is reshuffled: a localized build lays out its string pools by length
//! (the compiler puts a string of up to eight bytes in the `gp` small-data pool
//! and a longer one in read-only data), so the PAL `Automatico` sits in a
//! different pool from the USA `Auto` it replaces while the screen-element
//! placement record that points at it is the same record on both discs.
//!
//! Each reference is one word of an image:
//!
//! - **code** - an `addiu`/`ori` that completes a `lui` pair, or an `addiu`
//!   off `$gp`, whose resolved address is the string;
//! - **data** - a pointer word in a table (a placement record's payload
//!   pointer, a pointer array).
//!
//! A reference's **signature** is the window of words around it with every
//! address masked out: immediates of the I-type and J-type instructions for
//! code, pointer-shaped words for data. The rest - opcodes, registers, a
//! table's coordinates and ids - is the same program on both builds. A USA
//! reference pairs with the source reference whose signature differs in the
//! fewest words; a tie is broken by the displacement of the nearest reference
//! that paired uniquely. A USA string pairs with a source string when every
//! reference that paired agrees on it.

use std::collections::{BTreeMap, BTreeSet};

/// Words either side of a reference that make up its signature.
const HALF_WINDOW: usize = 8;
/// Most mismatching signature words a pairing may carry.
const MAX_MISMATCH: usize = 2;
/// Fewest informative words (not masked, not zero) a data reference's
/// signature needs: a run of bare pointers matches every other run.
const MIN_DATA_INFO: usize = 4;

/// Low / high bound of a word read as a pointer into a loaded image.
const PTR_LO: u32 = 0x8001_0000;
const PTR_HI: u32 = 0x8020_0000;

fn is_ptr(w: u32) -> bool {
    (PTR_LO..PTR_HI).contains(&w)
}

/// One loaded code/data image as a word array.
pub struct Image {
    words: Vec<u32>,
    base_va: u32,
    gp: Option<u32>,
}

impl Image {
    /// A PS-X EXE: text loaded at the header's `t_addr` from file `0x800`.
    pub fn from_exe(exe: &[u8]) -> Option<Self> {
        let t_addr = read_u32(exe, 0x18)?;
        let body = exe.get(0x800..)?;
        Some(Self {
            words: to_words(body),
            base_va: t_addr,
            gp: exe_gp(exe),
        })
    }

    /// A PROT overlay image loaded at `base_va`, sharing the executable's
    /// `$gp` (`gp`).
    pub fn from_overlay(bytes: &[u8], base_va: u32, gp: Option<u32>) -> Self {
        Self {
            words: to_words(bytes),
            base_va,
            gp,
        }
    }

    fn va(&self, idx: usize) -> u32 {
        self.base_va.wrapping_add((idx * 4) as u32)
    }
}

fn read_u32(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

fn to_words(b: &[u8]) -> Vec<u32> {
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect()
}

fn sext16(i: u32) -> u32 {
    (i as u16 as i16) as i32 as u32
}

/// The executable's `$gp`: the `lui $gp` / `addiu|ori $gp,$gp` pair the
/// startup code at the header's `pc0` sets it with.
pub fn exe_gp(exe: &[u8]) -> Option<u32> {
    let pc0 = read_u32(exe, 0x10)?;
    let t_addr = read_u32(exe, 0x18)?;
    let start = pc0.checked_sub(t_addr)? as usize + 0x800;
    let mut hi = None;
    for k in 0..256 {
        let w = read_u32(exe, start + k * 4)?;
        let (op, rs, rt, imm) = (w >> 26, (w >> 21) & 31, (w >> 16) & 31, w & 0xFFFF);
        if op == 0x0F && rt == 28 {
            hi = Some(imm << 16);
        } else if let Some(h) = hi
            && rt == 28
            && rs == 28
        {
            match op {
                0x09 => return Some(h.wrapping_add(sext16(imm))),
                0x0D => return Some(h | imm),
                _ => {}
            }
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Code,
    Data,
}

/// One reference: word index, the address it resolves to, and its kind.
#[derive(Clone, Copy)]
struct Ref {
    idx: usize,
    target: u32,
    kind: Kind,
}

/// Every code and data reference an image carries.
fn collect_refs(img: &Image) -> Vec<Ref> {
    let w = &img.words;
    let mut out = Vec::new();
    for (i, &x) in w.iter().enumerate() {
        let (op, rs, rt, imm) = (x >> 26, (x >> 21) & 31, (x >> 16) & 31, x & 0xFFFF);
        if op == 0x0F {
            // `lui r` then the first `addiu|ori rX, r, lo` within a short run.
            let r = rt;
            for (j, &y) in w.iter().enumerate().skip(i + 1).take(12) {
                let (yo, yrs, yrt) = (y >> 26, (y >> 21) & 31, (y >> 16) & 31);
                if (yo == 0x09 || yo == 0x0D) && yrs == r {
                    let lo = if yo == 0x09 {
                        sext16(y & 0xFFFF)
                    } else {
                        y & 0xFFFF
                    };
                    out.push(Ref {
                        idx: j,
                        target: (imm << 16).wrapping_add(lo),
                        kind: Kind::Code,
                    });
                    if yrt == r {
                        break;
                    }
                    continue;
                }
                // Another write of `r` ends the pair's reach.
                let writes_r = match yo {
                    0 => (y >> 11) & 31 == r && !matches!(y & 0x3F, 0x08 | 0x18..=0x1B),
                    0x01..=0x07 => false,
                    0x28..=0x2F => false,
                    _ => yrt == r,
                };
                if writes_r {
                    break;
                }
            }
        } else if op == 0x09
            && rs == 28
            && let Some(gp) = img.gp
        {
            out.push(Ref {
                idx: i,
                target: gp.wrapping_add(sext16(imm)),
                kind: Kind::Code,
            });
        } else if is_ptr(x) {
            out.push(Ref {
                idx: i,
                target: x,
                kind: Kind::Data,
            });
        }
    }
    out
}

fn mask(w: u32, kind: Kind) -> u32 {
    match kind {
        Kind::Data => {
            if is_ptr(w) {
                0
            } else {
                w
            }
        }
        Kind::Code => match w >> 26 {
            0 | 0x10..=0x13 => w,
            0x02 | 0x03 => w & 0xFC00_0000,
            _ => w & 0xFFFF_0000,
        },
    }
}

/// Masked words around `idx`, `None` outside the image.
fn signature(img: &Image, idx: usize, kind: Kind) -> Vec<Option<u32>> {
    (0..=2 * HALF_WINDOW)
        .map(|k| {
            let j = (idx + k).checked_sub(HALF_WINDOW)?;
            if j == idx {
                return Some(0);
            }
            img.words.get(j).map(|&w| mask(w, kind))
        })
        .collect()
}

fn mismatches(a: &[Option<u32>], b: &[Option<u32>]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

/// Pair the USA strings at `targets` with the source build's strings through
/// the references both images carry. Returns `usa_va -> source_va`.
///
/// `usa` and `src` are the same image on the two builds (the executable, or
/// one overlay); a string referenced from several images is paired by calling
/// this once per image and merging with [`merge_votes`].
pub fn pair_by_refs(
    usa: &Image,
    src: &Image,
    targets: &BTreeSet<u32>,
) -> BTreeMap<u32, BTreeSet<u32>> {
    let usa_refs: Vec<Ref> = collect_refs(usa)
        .into_iter()
        .filter(|r| targets.contains(&r.target))
        .collect();
    if usa_refs.is_empty() {
        return BTreeMap::new();
    }
    let src_refs = collect_refs(src);
    let src_sigs: Vec<Vec<Option<u32>>> = src_refs
        .iter()
        .map(|r| signature(src, r.idx, r.kind))
        .collect();
    // First pass: the best candidate per USA reference, unique or tied.
    let mut best: Vec<(Ref, Vec<usize>)> = Vec::new();
    for r in &usa_refs {
        let sig = signature(usa, r.idx, r.kind);
        if r.kind == Kind::Data
            && sig.iter().filter(|w| w.is_some_and(|w| w != 0)).count() < MIN_DATA_INFO
        {
            continue;
        }
        let site_word = mask(usa.words[r.idx], r.kind);
        let mut min = MAX_MISMATCH + 1;
        let mut cands: Vec<usize> = Vec::new();
        for (k, s) in src_refs.iter().enumerate() {
            if s.kind != r.kind || mask(src.words[s.idx], s.kind) != site_word {
                continue;
            }
            let m = mismatches(&sig, &src_sigs[k]);
            if m < min {
                min = m;
                cands.clear();
            }
            if m == min {
                cands.push(k);
            }
        }
        if min <= MAX_MISMATCH && !cands.is_empty() {
            best.push((*r, cands));
        }
    }
    // Displacements of the uniquely paired references, by USA address.
    let unique: BTreeMap<u32, i64> = best
        .iter()
        .filter(|(_, c)| c.len() == 1)
        .map(|(r, c)| {
            let s = &src_refs[c[0]];
            (usa.va(r.idx), src.va(s.idx) as i64 - usa.va(r.idx) as i64)
        })
        .collect();
    let mut votes: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for (r, cands) in best {
        let pick = if cands.len() == 1 {
            Some(cands[0])
        } else {
            // Tie: take the candidate whose displacement is the nearest
            // uniquely paired reference's, when exactly one matches it.
            let at = usa.va(r.idx);
            let near = unique
                .range(..at)
                .next_back()
                .into_iter()
                .chain(unique.range(at..).next())
                .min_by_key(|(va, _)| va.abs_diff(at))
                .map(|(_, d)| *d);
            near.and_then(|d| {
                let hits: Vec<usize> = cands
                    .iter()
                    .copied()
                    .filter(|&k| src.va(src_refs[k].idx) as i64 - at as i64 == d)
                    .collect();
                (hits.len() == 1).then(|| hits[0])
            })
        };
        if let Some(k) = pick {
            votes
                .entry(r.target)
                .or_default()
                .insert(src_refs[k].target);
        }
    }
    votes
}

/// Merge per-image votes and keep the strings every paired reference agrees
/// on: `usa_va -> source_va`.
pub fn merge_votes(maps: &[BTreeMap<u32, BTreeSet<u32>>]) -> BTreeMap<u32, u32> {
    let mut all: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for m in maps {
        for (k, v) in m {
            all.entry(*k).or_default().extend(v.iter().copied());
        }
    }
    all.into_iter()
        .filter_map(|(k, v)| (v.len() == 1).then(|| (k, *v.iter().next().unwrap())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(words: &[u32], base: u32) -> Image {
        Image {
            words: words.to_vec(),
            base_va: base,
            gp: None,
        }
    }

    #[test]
    fn data_reference_pairs_through_a_shifted_table() {
        // A three-word record [id, coords, ptr] on both builds; the source
        // table sits one word later and points at a different string.
        let usa = img(
            &[0, 7, 0x0012_0034, 0x8001_5000, 9, 0x0056_0078, 0x8001_5010],
            0x8002_0000,
        );
        let src = img(
            &[
                1,
                0,
                7,
                0x0012_0034,
                0x8001_6000,
                9,
                0x0056_0078,
                0x8001_6020,
            ],
            0x8002_0000,
        );
        let targets: BTreeSet<u32> = [0x8001_5000, 0x8001_5010].into();
        let m = merge_votes(&[pair_by_refs(&usa, &src, &targets)]);
        assert_eq!(m.get(&0x8001_5000), Some(&0x8001_6000));
        assert_eq!(m.get(&0x8001_5010), Some(&0x8001_6020));
    }

    #[test]
    fn code_reference_pairs_a_lui_addiu_pair() {
        // lui a0, 0x8001 ; addiu a0, a0, lo ; jal ; nop
        let lui = 0x3C04_8001;
        let usa = img(&[lui, 0x2484_5000, 0x0C00_1234, 0], 0x8003_0000);
        let src = img(&[0, lui, 0x2484_6000, 0x0C00_2234, 0], 0x8003_0000);
        let targets: BTreeSet<u32> = [0x8001_5000].into();
        let m = merge_votes(&[pair_by_refs(&usa, &src, &targets)]);
        assert_eq!(m.get(&0x8001_5000), Some(&0x8001_6000));
    }

    #[test]
    fn disagreeing_references_pair_nothing() {
        let mut a = BTreeMap::new();
        a.insert(1u32, BTreeSet::from([10u32]));
        let mut b = BTreeMap::new();
        b.insert(1u32, BTreeSet::from([11u32]));
        assert!(merge_votes(&[a, b]).is_empty());
    }
}
