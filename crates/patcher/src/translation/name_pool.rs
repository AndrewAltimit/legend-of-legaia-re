//! Name relocation: let a translated `SCUS_942.54` name outgrow its own span.
//!
//! Every name the executable's tables carry (items, item types, spells and
//! their info-window descriptions, Tactical Arts names, accessory passives) is
//! reached through a **pointer word** in a table record, never by its address
//! in code. So a translation longer than its in-place span (the string plus
//! the `0..=3` bytes of 4-byte alignment padding after its terminator) can
//! move: the text is written into free bytes elsewhere in the pools and every
//! table slot that pointed at the old string is repointed. The free bytes are
//! the pools' own - the tail a shorter translation leaves behind, the
//! alignment padding between strings, and the whole old span of every string
//! that moves - so relocation is a same-size edit of the executable (a PPF
//! carries it).
//!
//! A string moves only when that is provably safe on the disc being patched:
//!
//! - every slot the export walk reaches it through is a plain pointer - the
//!   arts-menu description is pinned, because the in-battle matcher finds the
//!   combo string in the bytes after its terminator
//!   ([`super::export::NameRefs::movable`]);
//! - the executable holds no *other* aligned word equal to its address (an
//!   unknown table would keep pointing at the old bytes);
//! - no `lui` + `addiu` / `ori` / load / store pair materialises its address
//!   in code.
//!
//! The statically based overlay images carry no reference to any of these
//! strings on the retail disc (swept with
//! `scripts/ghidra-analysis/find-address-word-refs.py`: every hit is one of
//! the walked table slots in the executable), so the executable is the only
//! place the import-time check has to look. Relocated strings start 4-byte aligned, as every retail name does.

use std::collections::{BTreeMap, HashMap, HashSet};

use legaia_asset::item_names;

use super::export::{NameRefs, name_table_refs};

/// Longest string the span measurement follows.
const MAX_STRLEN: usize = 512;

/// What [`NamePool::relocate`] did.
#[derive(Debug, Clone, Default)]
pub struct Relocation {
    /// Byte ranges written (`(file offset, len)`), to mirror onto the disc.
    pub written: Vec<(usize, usize)>,
    /// The growing strings that moved.
    pub moved: Vec<Moved>,
    /// The growing VAs that found no room (they keep their current text).
    pub no_room: Vec<u32>,
    /// The layout applied (with nothing to move, the layout of the pools as
    /// they read - nothing is written).
    pub layout: Layout,
}

/// A string the relocator placed: its old VA, its new VA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moved {
    /// The string's VA before the move (its pack key).
    pub from: u32,
    /// Where its text now lives.
    pub to: u32,
}

/// Why a name string stays where it is (see [`NamePool::pin_reason`]). The
/// order is the order the checks run in; a string reports the first that
/// holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinReason {
    /// Not a string the name-table walk reaches (a `system_text` pool
    /// string, or a VA no table slot points at).
    NotATableString,
    /// An arts-menu description: the in-battle matcher reads the combo
    /// string in the bytes after its terminator.
    ArtsDescription,
    /// Its text overlaps another pointed-to string (one starts inside the
    /// other), so neither may move.
    TailShared,
    /// No measurable span (an unmappable VA or an empty string).
    NoSpan,
    /// The executable holds more aligned words equal to its address than the
    /// table slots the walk found - an unknown table would keep pointing at
    /// the old bytes.
    WordCountMismatch,
    /// A `lui` + `addiu` / `ori` / load / store pair materialises its address
    /// in code.
    LuiMaterialised,
}

/// One compaction region: a maximal run of adjacent movable spans, with how
/// its bytes are used once its strings are re-laid end to end (see
/// [`NamePool::layout`]).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RegionUse {
    /// File offset of the region's first byte in `SCUS_942.54`.
    pub start_off: usize,
    /// One past its last byte.
    pub end_off: usize,
    /// VA of its first byte.
    pub start_va: u32,
    /// VA one past its last byte.
    pub end_va: u32,
    /// The string VAs whose retail spans make up the region, in file order.
    pub vas: Vec<u32>,
    /// Bytes the laid-out strings occupy (region size minus `free`): every
    /// kept string, 4-aligned, plus every moved-in string placed here.
    pub used: usize,
    /// Bytes of the region's free run left after the layout - the room a
    /// further longer name could still move into.
    pub free: usize,
}

impl RegionUse {
    /// Region size in bytes.
    pub fn total(&self) -> usize {
        self.end_off - self.start_off
    }
}

/// One planned compaction of the name pools ([`NamePool::layout`]).
#[derive(Debug, Clone, Default)]
pub struct Layout {
    /// Every region, in file order.
    pub regions: Vec<RegionUse>,
    /// Movable string VA -> the file offset its text starts at after the
    /// layout (kept strings and moved-in ones alike).
    pub placed: BTreeMap<u32, usize>,
    /// Growing VAs that found no free run (they keep their current text).
    pub no_room: Vec<u32>,
    /// Every movable string's final text (no terminator): the placed growing
    /// ones' translation, the rest as `scus` reads.
    pub texts: BTreeMap<u32, Vec<u8>>,
}

impl Layout {
    /// Index of the region whose retail spans include `va`.
    pub fn region_of(&self, va: u32) -> Option<usize> {
        self.regions.iter().position(|r| r.vas.contains(&va))
    }

    /// Index of the region holding file offset `off` (where a string was
    /// placed).
    pub fn region_at(&self, off: usize) -> Option<usize> {
        self.regions
            .iter()
            .position(|r| (r.start_off..r.end_off).contains(&off))
    }
}

/// The pointer-addressed name strings of one executable, with what it takes to
/// move each one. Built from the executable **before** any translation write,
/// so a span is always measured on the retail layout.
pub struct NamePool {
    refs: BTreeMap<u32, NameRefs>,
    /// Aligned-word value -> occurrences anywhere in the executable.
    word_counts: HashMap<u32, usize>,
    /// Addresses a `lui` pair materialises somewhere in the executable.
    materialised: HashSet<u32>,
    /// String VA -> (file offset, span length incl. terminator + padding).
    spans: BTreeMap<u32, (usize, usize)>,
    /// VAs whose text overlaps another pointed-to string's.
    shared: HashSet<u32>,
    /// One `(va, file offset)` pair, to map offsets back to VAs.
    anchor: Option<(u32, usize)>,
}

impl NamePool {
    /// Walk the name tables of `scus` (see [`name_table_refs`]) and measure
    /// every string's span.
    pub fn build(scus: &[u8]) -> Self {
        let refs = name_table_refs(scus);
        let mut word_counts: HashMap<u32, usize> = HashMap::new();
        for w in scus.as_chunks::<4>().0 {
            let v = u32::from_le_bytes(*w);
            if (0x8000_0000..0x8020_0000).contains(&v) {
                *word_counts.entry(v).or_default() += 1;
            }
        }
        let materialised = materialised_addresses(scus);
        let mut spans = BTreeMap::new();
        let mut shared = HashSet::new();
        let vas: Vec<u32> = refs.keys().copied().collect();
        for (i, &va) in vas.iter().enumerate() {
            let Some(off) = item_names::file_offset_for_va(scus, va) else {
                continue;
            };
            let Some(len) = scus
                .get(off..)
                .and_then(|t| t.iter().take(MAX_STRLEN).position(|&b| b == 0))
            else {
                continue;
            };
            // Terminator, then the zero alignment padding (verified zeros).
            let nul = off + len;
            let aligned = (nul + 4) & !3;
            let mut end = nul + 1;
            while end < aligned && scus.get(end) == Some(&0) {
                end += 1;
            }
            // Never reach the next pointed-to string (an empty string - a
            // pointer at a bare NUL - can sit inside the padding). A string
            // that *starts* inside this one's text shares its tail: neither
            // may move, so neither gets a span.
            if let Some(&next) = vas.get(i + 1) {
                let next_off = off + (next - va) as usize;
                if next_off <= nul {
                    shared.insert(va);
                    shared.insert(next);
                } else if next_off < end {
                    end = next_off;
                }
            }
            if len > 0 {
                spans.insert(va, (off, end - off));
            }
        }
        for va in &shared {
            spans.remove(va);
        }
        let anchor = spans.iter().next().map(|(&va, &(off, _))| (va, off));
        Self {
            refs,
            word_counts,
            materialised,
            spans,
            shared,
            anchor,
        }
    }

    /// `true` when the string at `va` may move (see the module docs).
    pub fn is_movable(&self, va: u32) -> bool {
        self.pin_reason(va).is_none()
    }

    /// Why the string at `va` may not move, or `None` when it may: the one
    /// rule behind [`Self::is_movable`], and so behind every move the
    /// importer makes.
    pub fn pin_reason(&self, va: u32) -> Option<PinReason> {
        let Some(r) = self.refs.get(&va).filter(|r| !r.slots.is_empty()) else {
            return Some(PinReason::NotATableString);
        };
        if !r.movable {
            return Some(PinReason::ArtsDescription);
        }
        if self.shared.contains(&va) {
            return Some(PinReason::TailShared);
        }
        if !self.spans.contains_key(&va) {
            return Some(PinReason::NoSpan);
        }
        if self.word_counts.get(&va).copied().unwrap_or(0) != r.slots.len() {
            return Some(PinReason::WordCountMismatch);
        }
        if self.materialised.contains(&va) {
            return Some(PinReason::LuiMaterialised);
        }
        None
    }

    /// `(file offset, span length)` of the string at `va`: its text, its
    /// terminator and the zero alignment padding after it.
    pub fn span(&self, va: u32) -> Option<(usize, usize)> {
        self.spans.get(&va).copied()
    }

    /// Every string VA the name-table walk reaches.
    pub fn table_vas(&self) -> impl Iterator<Item = u32> + '_ {
        self.refs.keys().copied()
    }

    /// `(va, file offset, span length)` of every string that may move - the
    /// only executable bytes a relocation rewrites besides the table slots.
    pub fn movable_spans(&self) -> Vec<(u32, usize, usize)> {
        self.spans
            .iter()
            .filter(|(va, _)| self.is_movable(**va))
            .map(|(&va, &(off, len))| (va, off, len))
            .collect()
    }

    /// Plan one compaction of the pools over `scus` as it now reads: each
    /// maximal run of adjacent movable spans is one region, its strings are
    /// re-laid end to end (4-byte aligned, original order), and the strings in
    /// `growing` (`va -> text without terminator`) are placed into the free
    /// runs that leaves, largest first, each into the tightest run that holds
    /// it - a run in any region. One that finds no room is dropped and the
    /// layout is re-planned without it.
    ///
    /// With `growing` empty this is the compaction of the current text: on a
    /// retail executable, the bytes English uses per region.
    pub fn layout(&self, scus: &[u8], growing: &BTreeMap<u32, &[u8]>) -> Layout {
        let mut growing: BTreeMap<u32, &[u8]> = growing
            .iter()
            .filter(|(va, _)| self.is_movable(**va))
            .map(|(&va, &t)| (va, t))
            .collect();
        // Regions: maximal runs of adjacent movable spans, in file order.
        let mut regions: Vec<(usize, usize, Vec<u32>)> = Vec::new();
        for (&va, &(off, len)) in &self.spans {
            if !self.is_movable(va) {
                continue;
            }
            match regions.last_mut() {
                Some((_, end, vas)) if *end == off => {
                    *end = off + len;
                    vas.push(va);
                }
                _ => regions.push((off, off + len, vec![va])),
            }
        }
        // Every movable string's current text (in-place translation or
        // retail).
        let current: BTreeMap<u32, Vec<u8>> = regions
            .iter()
            .flat_map(|(_, _, vas)| vas)
            .map(|&va| {
                let (off, len) = self.spans[&va];
                let n = scus[off..off + len]
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(len);
                (va, scus[off..off + n].to_vec())
            })
            .collect();
        let mut no_room = Vec::new();
        let (placed, free) = loop {
            match plan_compaction(&self.spans, &regions, &current, &growing) {
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
        let regions = regions
            .into_iter()
            .zip(free)
            .map(|((start, end, vas), free)| RegionUse {
                start_off: start,
                end_off: end,
                start_va: self.va_of(start),
                end_va: self.va_of(end),
                vas,
                used: end - start - free,
                free,
            })
            .collect();
        Layout {
            regions,
            placed,
            no_room,
            texts,
        }
    }

    /// VA of executable file offset `off` (the name pools sit in one segment).
    fn va_of(&self, off: usize) -> u32 {
        self.anchor.map_or(0, |(va, va_off)| va_at(va, va_off, off))
    }

    /// Give every string in `grow` (`(va, text without terminator)`, each a
    /// translation that overflows its span) room, and rewrite `scus`
    /// accordingly. `scus` already holds the in-place writes, so every other
    /// name reads as its final text.
    ///
    /// The freed bytes a shorter translation leaves are scattered - a few
    /// bytes after each name - so the pools are **compacted**
    /// ([`Self::layout`]): in original order a string never starts later than
    /// it did, so the rest always fits and each region's spare bytes collect
    /// into one run at its end; the growing strings are placed into those
    /// runs. One that still finds no room keeps its original text (it is
    /// reported, never truncated).
    ///
    /// See [`Relocation`] for what comes back.
    pub fn relocate(&self, scus: &mut [u8], grow: &[(u32, Vec<u8>)]) -> Relocation {
        let mut growing: BTreeMap<u32, &[u8]> = BTreeMap::new();
        let mut failed = Vec::new();
        for (va, text) in grow {
            if self.is_movable(*va) {
                growing.insert(*va, text);
            } else {
                failed.push(*va);
            }
        }
        let layout = self.layout(scus, &growing);
        if growing.is_empty() {
            return Relocation {
                no_room: failed,
                layout,
                ..Default::default()
            };
        }
        for va in &layout.no_room {
            growing.remove(va);
            failed.push(*va);
        }

        let mut written = Vec::new();
        for r in &layout.regions {
            scus[r.start_off..r.end_off].fill(0);
            written.push((r.start_off, r.total()));
        }
        let mut moved = Vec::new();
        for (&va, &new_off) in &layout.placed {
            let text = &layout.texts[&va];
            scus[new_off..new_off + text.len()].copy_from_slice(text);
            let (old_off, _) = self.spans[&va];
            let to = va_at(va, old_off, new_off);
            if to != va {
                for &slot in &self.refs[&va].slots {
                    if let Some(s) = item_names::file_offset_for_va(scus, slot) {
                        scus[s..s + 4].copy_from_slice(&to.to_le_bytes());
                        written.push((s, 4));
                    }
                }
            }
            if growing.contains_key(&va) {
                moved.push(Moved { from: va, to });
            }
        }
        Relocation {
            written,
            moved,
            no_room: failed,
            layout,
        }
    }
}

/// Lay out one compaction: `Ok((va -> new file offset, free bytes left per
/// region))` for every movable string, or `Err(growing vas that found no
/// room)`.
#[allow(clippy::type_complexity)]
fn plan_compaction(
    spans: &BTreeMap<u32, (usize, usize)>,
    regions: &[(usize, usize, Vec<u32>)],
    current: &BTreeMap<u32, Vec<u8>>,
    growing: &BTreeMap<u32, &[u8]>,
) -> Result<(BTreeMap<u32, usize>, Vec<usize>), Vec<u32>> {
    let mut placed = BTreeMap::new();
    // Free run per region after the in-order compaction of the rest (empty
    // when the region is full), one per region in region order.
    let mut free: Vec<(usize, usize)> = Vec::new();
    for (start, end, vas) in regions {
        let mut cur = *start;
        for va in vas.iter().filter(|va| !growing.contains_key(va)) {
            // 4-aligned, unless that would start it past where it started in
            // retail (an unaligned retail string): never starting later than
            // before, and never longer than before, the rest always fits.
            let orig = spans[va].0;
            let at = if align4(cur) <= orig {
                align4(cur)
            } else {
                orig
            };
            placed.insert(*va, at);
            cur = at + current[va].len() + 1;
        }
        free.push((align4(cur).min(*end), *end));
    }
    let mut order: Vec<(&u32, &&[u8])> = growing.iter().collect();
    order.sort_by_key(|(va, t)| (std::cmp::Reverse(t.len()), **va));
    let mut no_room = Vec::new();
    for (va, text) in order {
        let need = text.len() + 1;
        // Tightest run that holds it (runs start 4-aligned).
        let best = free
            .iter()
            .enumerate()
            .filter(|(_, (s, e))| s + need <= *e)
            .min_by_key(|(_, (s, e))| e - s)
            .map(|(i, _)| i);
        match best {
            Some(i) => {
                let (s, e) = free[i];
                placed.insert(*va, s);
                free[i] = (align4(s + need).min(e), e);
            }
            None => no_room.push(*va),
        }
    }
    if no_room.is_empty() {
        Ok((placed, free.iter().map(|(s, e)| e - s).collect()))
    } else {
        Err(no_room)
    }
}

fn align4(x: usize) -> usize {
    (x + 3) & !3
}

/// VA of file offset `off`, given one `(va, va_off)` pair in the same segment.
fn va_at(va: u32, va_off: usize, off: usize) -> u32 {
    (va as i64 + off as i64 - va_off as i64) as u32
}

/// Every address a `lui rX, hi` materialises through a following `addiu` /
/// `ori` / load / store on `rX` (the pair Ghidra's reference manager does not
/// resolve). Over-approximates - a data word that decodes as `lui` adds
/// harmless extra addresses, which only pins more strings in place.
fn materialised_addresses(scus: &[u8]) -> HashSet<u32> {
    let words: Vec<u32> = scus
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .collect();
    let mut out = HashSet::new();
    for (i, &w) in words.iter().enumerate() {
        if w >> 26 != 0x0F {
            continue;
        }
        let reg = (w >> 16) & 31;
        let hi = (w & 0xFFFF) << 16;
        for &u in words.iter().skip(i + 1).take(32) {
            let op = u >> 26;
            let rs = (u >> 21) & 31;
            let rt = (u >> 16) & 31;
            let imm = u & 0xFFFF;
            if rs == reg {
                match op {
                    0x0D => {
                        out.insert(hi | imm);
                    }
                    0x09 | 0x20..=0x26 | 0x28..=0x2B => {
                        out.insert(hi.wrapping_add(imm as u16 as i16 as i32 as u32));
                    }
                    _ => {}
                }
            }
            // The pair ends once the register is overwritten (an I-type ALU op
            // or load into it, an R-type op with it as `rd`) or control leaves
            // the straight line (a jump; its delay slot was already seen).
            let rd = (u >> 11) & 31;
            let writes = match op {
                0x08..=0x0F | 0x20..=0x26 => rt == reg,
                0x00 => rd == reg || matches!(u & 0x3F, 0x08 | 0x09),
                0x02 | 0x03 => true,
                _ => false,
            };
            if writes {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lui_pairs_materialise_their_address() {
        // lui v0,0x8007 ; addiu a0,v0,0x7a38  -> 0x80077a38
        // lui v1,0x8008 ; lw t0,-0x4960(v1)   -> 0x8007b6a0
        let code = [0x3C02_8007u32, 0x2444_7A38, 0x3C03_8008, 0x8C68_B6A0];
        let bytes: Vec<u8> = code.iter().flat_map(|w| w.to_le_bytes()).collect();
        let got = materialised_addresses(&bytes);
        assert!(got.contains(&0x8007_7A38));
        assert!(got.contains(&0x8007_B6A0));
    }

    #[test]
    fn va_at_is_segment_relative() {
        assert_eq!(va_at(0x8001_1230, 0x1A30, 0x1A40), 0x8001_1240);
        assert_eq!(va_at(0x8001_1230, 0x1A30, 0x1A20), 0x8001_1220);
    }
}
