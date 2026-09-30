//! Moving `ui_menu` / `system_text` strings: room for labels longer than
//! English.
//!
//! These strings are reached straight from code - a `lui` pair, a
//! `$gp`-relative instruction, or a pointer word in a table - so a longer
//! translation cannot simply spill past its terminator. It can **move**, the
//! way a SCUS name does ([`super::name_pool`]), once every reference to it is
//! known and can be rewritten ([`super::code_refs`]):
//!
//! 1. the image's pool strings are compacted, each run of adjacent movable
//!    strings re-laid end to end in order, so the bytes shorter translations
//!    give up collect into one free run per run;
//! 2. the longer strings are placed into those runs, or into the spare
//!    regions the [space ledger](crate::space_ledger) reserves for
//!    translation in that image (the menu overlay's slack), or - for
//!    `SCUS_942.54` - into the free runs the name pools' own compaction left;
//! 3. every reference to a moved string is rewritten.
//!
//! A string moves only when that is provably safe on the disc being
//! patched ([`CodePin`] lists what pins one):
//!
//! - something in its own image forms its address - a string nothing
//!   references is reached some other way (an offset from a neighbour, an
//!   index into a table), so it stays, and so do its neighbours;
//! - nothing forms an address **inside** it (`@Items + 1` skips the marker);
//! - no other image that can be resident with it forms its address;
//! - it does not share bytes with another string, and it is not one of a run
//!   of strings laid at a fixed stride with an unreferenced member (a table
//!   indexed by arithmetic);
//! - every reference can form the new address: a pointer word always can, a
//!   private `lui` pair always can, a shared `lui` half only at the same
//!   `%hi`, a `$gp` form only within its displacement.
//!
//! Every move is a same-size edit of the image, so a PPF still carries it.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::code_refs::{self, RefSite};
use super::ui::{self, UiStringPool};

/// Why a code-referenced string stays where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodePin {
    /// Nothing in its image forms its address: it is reached by arithmetic.
    NoReference,
    /// An adjacent string has no reference, so it may be reached as an
    /// offset from this one.
    NeighbourUnreferenced,
    /// Something forms an address inside the string.
    InteriorReference,
    /// Another image that can be resident with it forms its address.
    ForeignReference,
    /// It shares bytes with another string.
    TailShared,
    /// One of a run of strings at a fixed stride, at least one of which
    /// nothing references: a table indexed by arithmetic.
    FixedStride,
    /// A reference no address other than its own can satisfy.
    Unmovable,
}

/// One string the importer can see.
#[derive(Debug, Clone, Copy)]
struct Str {
    off: usize,
    /// Span: text + terminator + zero alignment padding.
    span: usize,
    strict: bool,
}

/// One compaction run: adjacent movable strings, with how its bytes are
/// used after the layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRegion {
    /// VA of its first byte.
    pub start_va: u32,
    /// VA one past its last byte.
    pub end_va: u32,
    /// Its strings, in order.
    pub vas: Vec<u32>,
    /// Bytes its laid-out strings occupy.
    pub used: usize,
    /// Bytes left in its free run after the layout.
    pub free: usize,
}

/// A spare span the layout could place strings in, with its use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtraUse {
    /// First VA.
    pub start_va: u32,
    /// One past the last VA.
    pub end_va: u32,
    /// Bytes the placed strings take (with alignment).
    pub used: usize,
}

/// One planned layout ([`CodeStrings::layout`]).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodeLayout {
    /// The compaction runs, in image order.
    pub regions: Vec<CodeRegion>,
    /// The spare spans offered, with what the layout put in them.
    pub extra: Vec<ExtraUse>,
    /// Movable string VA -> VA its text starts at after the layout.
    pub placed: BTreeMap<u32, u32>,
    /// Growing VAs no run had room for.
    pub no_room: Vec<u32>,
    /// Final text (no terminator) of every placed string.
    #[serde(skip)]
    pub texts: BTreeMap<u32, Vec<u8>>,
}

impl CodeLayout {
    /// Free bytes left: the compaction runs' tails plus the spare spans'
    /// remainder.
    pub fn free(&self) -> usize {
        self.regions.iter().map(|r| r.free).sum::<usize>()
            + self
                .extra
                .iter()
                .map(|e| {
                    (e.end_va - e.start_va) as usize - e.used.min((e.end_va - e.start_va) as usize)
                })
                .sum::<usize>()
    }
}

/// What [`CodeStrings::relocate`] did.
#[derive(Debug, Clone, Default)]
pub struct CodeRelocation {
    /// Image ranges written (`(offset, len)`).
    pub written: Vec<(usize, usize)>,
    /// Growing strings moved: `(old VA, new VA)`.
    pub moved: Vec<(u32, u32)>,
    /// Growing VAs that keep their current text (pinned, or no room).
    pub no_room: Vec<u32>,
    /// The layout applied.
    pub layout: CodeLayout,
}

/// The code-referenced strings of one image and what it takes to move each.
/// Built before any translation write, so every span is the retail one.
pub struct CodeStrings {
    base: u32,
    gp: u32,
    strs: BTreeMap<u32, Str>,
    refs: BTreeMap<u32, Vec<RefSite>>,
    pins: BTreeMap<u32, CodePin>,
}

fn align4(x: usize) -> usize {
    (x + 3) & !3
}

impl CodeStrings {
    /// Scan `image` (loaded at `base`) for the strings of `pools`, find every
    /// reference in it, and every reference in `foreign` (other images that
    /// can be resident at the same time, each `(bytes, base)`).
    pub fn build<'a>(
        image: &[u8],
        base: u32,
        pools: impl IntoIterator<Item = &'a UiStringPool>,
        foreign: &[(&[u8], u32)],
    ) -> Self {
        let gp = code_refs::RETAIL_GP;
        let mut found: BTreeMap<u32, (usize, bool)> = BTreeMap::new();
        for p in pools {
            for s in ui::scan_pool(image, p) {
                found.entry(s.va).or_insert((s.bytes.len(), p.strict));
            }
        }
        let vas: Vec<u32> = found.keys().copied().collect();
        let mut strs = BTreeMap::new();
        let mut pins = BTreeMap::new();
        for (i, &va) in vas.iter().enumerate() {
            let (len, strict) = found[&va];
            let off = (va - base) as usize;
            let nul = off + len;
            let mut end = nul + 1;
            let aligned = align4(nul + 1);
            let next_off = vas.get(i + 1).map(|&n| (n - base) as usize);
            while end < aligned && image.get(end) == Some(&0) && Some(end) != next_off {
                end += 1;
            }
            if let Some(n) = next_off
                && n <= nul
            {
                pins.insert(va, CodePin::TailShared);
                pins.insert(vas[i + 1], CodePin::TailShared);
            }
            strs.insert(
                va,
                Str {
                    off,
                    span: end - off,
                    strict,
                },
            );
        }
        let (lo, hi) = match (vas.first(), strs.iter().next_back()) {
            (Some(&lo), Some((&va, s))) => (lo, va + s.span as u32 + 1),
            _ => (0, 0),
        };
        let refs = code_refs::scan(image, base, Some(gp), lo, hi);
        let mut foreign_hits: BTreeSet<u32> = BTreeSet::new();
        for (img, fbase) in foreign {
            foreign_hits.extend(code_refs::scan(img, *fbase, Some(gp), lo, hi).into_keys());
        }
        let mut this = Self {
            base,
            gp,
            strs,
            refs,
            pins,
        };
        let text_len = |va: u32| found[&va].0 as u32;
        for &va in &vas {
            let end = va + text_len(va);
            let pin = if !this.refs.contains_key(&va) {
                Some(CodePin::NoReference)
            } else if this.refs.range(va + 1..=end).next().is_some() {
                Some(CodePin::InteriorReference)
            } else if foreign_hits.range(va..=end).next().is_some() {
                Some(CodePin::ForeignReference)
            } else {
                None
            };
            if let Some(p) = pin {
                this.pins.entry(va).or_insert(p);
            }
        }
        // A string beside an unreferenced one may be the base it is reached
        // from.
        for (i, &va) in vas.iter().enumerate() {
            let s = this.strs[&va];
            let adjacent_unref = |j: usize| {
                vas.get(j).is_some_and(|&n| {
                    let t = this.strs[&n];
                    let touches = t.off == s.off + s.span || t.off + t.span == s.off;
                    touches && this.pins.get(&n) == Some(&CodePin::NoReference)
                })
            };
            if (i > 0 && adjacent_unref(i - 1)) || adjacent_unref(i + 1) {
                this.pins
                    .entry(va)
                    .or_insert(CodePin::NeighbourUnreferenced);
            }
        }
        // Three or more strings at one stride, padded past their own
        // alignment, one of which nothing references: a table indexed by
        // arithmetic, so none of its members may move. (A run whose every
        // member is referenced on its own is an ordinary pool of similar
        // lengths.)
        let mut i = 0;
        while i + 2 < vas.len() {
            let d = vas[i + 1] - vas[i];
            let mut j = i + 1;
            while j + 1 < vas.len() && vas[j + 1] - vas[j] == d {
                j += 1;
            }
            let padded = (i..=j).any(|k| d as usize > align4(text_len(vas[k]) as usize + 1));
            let unreferenced = vas[i..=j]
                .iter()
                .any(|va| this.pins.get(va) == Some(&CodePin::NoReference));
            if j - i >= 2 && padded && unreferenced {
                for &va in &vas[i..=j] {
                    this.pins.entry(va).or_insert(CodePin::FixedStride);
                }
            }
            i = j;
        }
        this
    }

    /// Every string VA the pools hold.
    pub fn vas(&self) -> impl Iterator<Item = u32> + '_ {
        self.strs.keys().copied()
    }

    /// Why the string at `va` stays, or `None` when it may move.
    pub fn pin_reason(&self, va: u32) -> Option<CodePin> {
        if !self.strs.contains_key(&va) {
            return Some(CodePin::NoReference);
        }
        self.pins.get(&va).copied()
    }

    /// `true` when the string at `va` may move.
    pub fn is_movable(&self, va: u32) -> bool {
        self.pin_reason(va).is_none()
    }

    /// The string's span in bytes (text, terminator, zero padding).
    pub fn span(&self, va: u32) -> Option<usize> {
        self.strs.get(&va).map(|s| s.span)
    }

    /// Number of references to `va` in its own image.
    pub fn ref_count(&self, va: u32) -> usize {
        self.refs.get(&va).map_or(0, Vec::len)
    }

    /// Every site in the image that forms `va` (retail layout).
    pub fn sites(&self, va: u32) -> &[RefSite] {
        self.refs.get(&va).map_or(&[], Vec::as_slice)
    }

    /// `true` when every reference to `va` can form `new`.
    fn allowed(&self, va: u32, new: u32) -> bool {
        new == va
            || self.refs.get(&va).is_some_and(|sites| {
                sites
                    .iter()
                    .all(|s| code_refs::can_retarget(s, va, new, self.gp))
            })
    }

    /// The string's current text (no terminator) in `image`, the way its
    /// pool reads it, clamped to its span.
    fn current(&self, image: &[u8], va: u32) -> Vec<u8> {
        let s = self.strs[&va];
        let n = ui::pool_strlen(image, s.off, s.strict)
            .unwrap_or(0)
            .min(s.span.saturating_sub(1));
        image[s.off..s.off + n].to_vec()
    }

    /// Plan one compaction over `image` as it now reads, placing the strings
    /// in `growing` (`va -> text`) into the runs it frees or into `extra`
    /// (spare `(start_va, end_va)` spans the caller vouches are zero and
    /// resident whenever the image is). Nothing is written.
    pub fn layout(
        &self,
        image: &[u8],
        growing: &BTreeMap<u32, &[u8]>,
        extra: &[(u32, u32)],
    ) -> CodeLayout {
        let mut growing: BTreeMap<u32, &[u8]> = growing
            .iter()
            .filter(|(va, _)| self.is_movable(**va))
            .map(|(&va, &t)| (va, t))
            .collect();
        let mut regions: Vec<(usize, usize, Vec<u32>)> = Vec::new();
        for (&va, s) in &self.strs {
            if !self.is_movable(va) {
                continue;
            }
            match regions.last_mut() {
                Some((_, end, vas)) if *end == s.off => {
                    *end = s.off + s.span;
                    vas.push(va);
                }
                _ => regions.push((s.off, s.off + s.span, vec![va])),
            }
        }
        let current: BTreeMap<u32, Vec<u8>> = regions
            .iter()
            .flat_map(|(_, _, v)| v)
            .map(|&va| (va, self.current(image, va)))
            .collect();
        let extra_offs: Vec<(usize, usize)> = extra
            .iter()
            .filter_map(|&(s, e)| {
                let so = s.checked_sub(self.base)? as usize;
                let eo = (e.checked_sub(self.base)? as usize).min(image.len());
                (so < eo).then_some((so, eo))
            })
            .collect();
        let mut no_room = Vec::new();
        let (placed, free) = loop {
            match self.plan(&regions, &current, &growing, &extra_offs) {
                Ok(p) => break p,
                Err(failed) => {
                    for va in failed {
                        growing.remove(&va);
                        no_room.push(va);
                    }
                }
            }
        };
        let mut texts = current;
        for (va, t) in &growing {
            texts.insert(*va, t.to_vec());
        }
        let va_of = |off: usize| self.base + off as u32;
        let regions_out = regions
            .into_iter()
            .zip(free)
            .map(|((s, e, vas), free)| CodeRegion {
                start_va: va_of(s),
                end_va: va_of(e),
                vas,
                used: e - s - free,
                free,
            })
            .collect();
        let extra_out = extra_offs
            .iter()
            .map(|&(s, e)| ExtraUse {
                start_va: va_of(s),
                end_va: va_of(e),
                used: placed
                    .iter()
                    .filter(|(_, o)| (s..e).contains(*o))
                    .map(|(va, o)| (align4(o + texts[va].len() + 1).min(e)) - o)
                    .sum(),
            })
            .collect();
        CodeLayout {
            regions: regions_out,
            extra: extra_out,
            placed: placed.into_iter().map(|(va, o)| (va, va_of(o))).collect(),
            no_room,
            texts,
        }
    }

    /// `Ok((va -> new offset, free bytes per region))`, or `Err(growing VAs
    /// with no room)`.
    #[allow(clippy::type_complexity)]
    fn plan(
        &self,
        regions: &[(usize, usize, Vec<u32>)],
        current: &BTreeMap<u32, Vec<u8>>,
        growing: &BTreeMap<u32, &[u8]>,
        extra: &[(usize, usize)],
    ) -> Result<(BTreeMap<u32, usize>, Vec<usize>), Vec<u32>> {
        let mut placed = BTreeMap::new();
        let mut runs: Vec<(usize, usize, Option<usize>)> = Vec::new();
        for (ri, (start, end, vas)) in regions.iter().enumerate() {
            let mut cur = *start;
            for va in vas.iter().filter(|va| !growing.contains_key(va)) {
                let orig = self.strs[va].off;
                let cand = align4(cur);
                let at = if cand <= orig && self.allowed(*va, self.base + cand as u32) {
                    cand
                } else {
                    orig
                };
                placed.insert(*va, at);
                cur = at + current[va].len() + 1;
            }
            runs.push((align4(cur).min(*end), *end, Some(ri)));
        }
        for &(s, e) in extra {
            runs.push((align4(s), e, None));
        }
        let mut order: Vec<(&u32, &&[u8])> = growing.iter().collect();
        order.sort_by_key(|(va, t)| (std::cmp::Reverse(t.len()), **va));
        let mut no_room = Vec::new();
        for (va, text) in order {
            let need = text.len() + 1;
            // Tightest run with an aligned position every reference can form.
            let mut best: Option<(usize, usize)> = None;
            for (i, &(s, e, _)) in runs.iter().enumerate() {
                let mut p = align4(s);
                while p + need <= e {
                    if self.allowed(*va, self.base + p as u32) {
                        break;
                    }
                    p += 4;
                }
                if p + need <= e && best.is_none_or(|(bi, _)| e - s < runs[bi].1 - runs[bi].0) {
                    best = Some((i, p));
                }
            }
            match best {
                Some((i, p)) => {
                    let (s, e, r) = runs[i];
                    placed.insert(*va, p);
                    runs[i] = (align4(p + need).min(e), e, r);
                    if p > s {
                        runs.push((s, p, None));
                    }
                }
                None => no_room.push(*va),
            }
        }
        if !no_room.is_empty() {
            return Err(no_room);
        }
        let mut free = vec![0usize; regions.len()];
        for (s, e, r) in runs {
            if let Some(r) = r {
                free[r] = e - s;
            }
        }
        Ok((placed, free))
    }

    /// Give every string in `grow` (`(va, text)`, each longer than its span)
    /// room and rewrite `image`, which already holds the in-place writes.
    /// All or nothing: if any reference fails to rewrite, `image` is left
    /// untouched and every growing string is reported in `no_room`.
    pub fn relocate(
        &self,
        image: &mut [u8],
        grow: &[(u32, Vec<u8>)],
        extra: &[(u32, u32)],
    ) -> CodeRelocation {
        let mut growing: BTreeMap<u32, &[u8]> = BTreeMap::new();
        let mut failed = Vec::new();
        for (va, t) in grow {
            if self.is_movable(*va) {
                growing.insert(*va, t);
            } else {
                failed.push(*va);
            }
        }
        let layout = self.layout(image, &growing, extra);
        for va in &layout.no_room {
            growing.remove(va);
            failed.push(*va);
        }
        if growing.is_empty() {
            return CodeRelocation {
                no_room: failed,
                layout,
                ..Default::default()
            };
        }
        let mut out = image.to_vec();
        let mut written = Vec::new();
        for r in &layout.regions {
            let (s, e) = (
                (r.start_va - self.base) as usize,
                (r.end_va - self.base) as usize,
            );
            out[s..e].fill(0);
            written.push((s, e - s));
        }
        let mut moved = Vec::new();
        for (&va, &to) in &layout.placed {
            let text = &layout.texts[&va];
            let o = (to - self.base) as usize;
            out[o..o + text.len()].copy_from_slice(text);
            out[o + text.len()] = 0;
            written.push((o, text.len() + 1));
            if to != va {
                for site in self.refs.get(&va).into_iter().flatten() {
                    match code_refs::retarget(&mut out, site, va, to, self.gp) {
                        Some(w) => written.extend(w),
                        None => {
                            failed.extend(growing.keys());
                            return CodeRelocation {
                                no_room: failed,
                                layout,
                                ..Default::default()
                            };
                        }
                    }
                }
            }
            if growing.contains_key(&va) {
                moved.push((va, to));
            }
        }
        image.copy_from_slice(&out);
        CodeRelocation {
            written,
            moved,
            no_room: failed,
            layout,
        }
    }
}

/// The pools of one image: overlay `prot`, or `SCUS_942.54` for `usize::MAX`.
pub fn pools_of(prot: usize) -> impl Iterator<Item = &'static UiStringPool> {
    ui::UI_STRING_POOLS
        .iter()
        .chain(ui::SCUS_STRING_POOLS)
        .filter(move |p| p.prot_index == prot)
}

/// The images that can be resident beside overlay `prot` in the other
/// overlay slot, among the ones the pools cover: the battle overlay (slot A)
/// beside the slot-B battle modules, and they beside it.
pub fn co_resident(prot: usize) -> &'static [usize] {
    match prot {
        898 => &[967, 941, 954],
        967 | 941 | 954 => &[898],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(base: u32, s: u32, e: u32) -> UiStringPool {
        UiStringPool {
            prot_index: 0,
            base_va: base,
            va_start: s,
            va_end: e,
            label: "t",
            strict: false,
        }
    }

    /// Code at 0x00..0x20, strings at 0x40.., a spare zero span at 0x100.
    fn image() -> (Vec<u8>, u32) {
        let base = 0x801C_0000u32;
        let mut img = vec![0u8; 0x200];
        let words: [u32; 8] = [
            // lui a0,0x801c ; jal 0 ; addiu a0,a0,0x40  -> "Items"
            0x3C04_801C,
            0x0C00_0000,
            0x2484_0040,
            // lui a0,0x801c ; jal 0 ; addiu a0,a0,0x48  -> "Magic"
            0x3C04_801C,
            0x0C00_0000,
            0x2484_0048,
            0x03E0_0008,
            0,
        ];
        for (i, w) in words.iter().enumerate() {
            img[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        // A pointer word to "Equip" at 0x30.
        img[0x30..0x34].copy_from_slice(&(base + 0x50).to_le_bytes());
        img[0x40..0x46].copy_from_slice(b"Items\0");
        img[0x48..0x4E].copy_from_slice(b"Magic\0");
        img[0x50..0x56].copy_from_slice(b"Equip\0");
        (img, base)
    }

    #[test]
    fn a_long_label_moves_into_the_spare_span_and_every_reference_follows() {
        let (mut img, base) = image();
        let p = pool(base, base + 0x40, base + 0x60);
        let cs = CodeStrings::build(&img, base, [&p], &[]);
        assert!(cs.is_movable(base + 0x40));
        assert!(cs.is_movable(base + 0x50));
        let grow = vec![(base + 0x50, b"Equipamento".to_vec())];
        let r = cs.relocate(&mut img, &grow, &[(base + 0x100, base + 0x140)]);
        assert_eq!(r.moved.len(), 1, "{r:?}");
        let (_, to) = r.moved[0];
        let o = (to - base) as usize;
        assert_eq!(&img[o..o + 12], b"Equipamento\0");
        assert_eq!(u32::from_le_bytes(img[0x30..0x34].try_into().unwrap()), to);
        // The other strings still resolve through their (untouched) pairs.
        let refs = code_refs::scan(&img, base, None, base, base + 0x200);
        assert!(refs.contains_key(&(base + 0x40)));
        assert_eq!(&img[0x40..0x46], b"Items\0");
    }

    #[test]
    fn an_unreferenced_string_pins_itself_and_its_neighbour() {
        let (mut img, base) = image();
        img[0x58..0x5E].copy_from_slice(b"Other\0");
        let p = pool(base, base + 0x40, base + 0x60);
        let cs = CodeStrings::build(&img, base, [&p], &[]);
        assert_eq!(cs.pin_reason(base + 0x58), Some(CodePin::NoReference));
        assert_eq!(
            cs.pin_reason(base + 0x50),
            Some(CodePin::NeighbourUnreferenced)
        );
    }

    #[test]
    fn a_foreign_reference_pins() {
        let (img, base) = image();
        let other = (base + 0x48).to_le_bytes().to_vec();
        let p = pool(base, base + 0x40, base + 0x60);
        let cs = CodeStrings::build(&img, base, [&p], &[(&other, 0x8000_0000)]);
        assert_eq!(cs.pin_reason(base + 0x48), Some(CodePin::ForeignReference));
    }

    #[test]
    fn nothing_moves_without_room() {
        let (mut img, base) = image();
        let before = img.clone();
        let p = pool(base, base + 0x40, base + 0x60);
        let cs = CodeStrings::build(&img, base, [&p], &[]);
        let r = cs.relocate(&mut img, &[(base + 0x50, vec![b'x'; 40])], &[]);
        assert!(r.moved.is_empty());
        assert_eq!(r.no_room, vec![base + 0x50]);
        assert_eq!(img, before);
    }
}
