//! Arrays an overlay's own code indexes at a **runtime** value, and arrays
//! a counted loop walks by bumping a pointer.
//!
//! [`super::loop_bounded_arrays`] claims an array only when a loop counts an
//! index from zero to a constant and scales it with one `sll`. Most of an
//! overlay's data segment is not reached that way: the index is a runtime
//! value (an actor field, a menu cursor, a script operand), the scaling is a
//! compiler strength-reduced multiply (`i * 28` as `((i << 3) - i) << 2`),
//! and the base is an `addiu` the element address is summed with. What still
//! pins such an array is read off the same instructions:
//!
//! * the **base** off its `lui` pair ([`super::reg_source`], following
//!   copies), or off the `lui` of the `lui at,hi; addu at,at,rX; lw y,lo(at)`
//!   form;
//! * the **stride** off the index arithmetic: the other `addu` operand is
//!   evaluated backwards as `k * leaf + c` through `sll` / `addu` / `subu` /
//!   `addiu` / copies, and `k` is the element size;
//! * the **field** each access reads, `c + displacement` reduced modulo `k`,
//!   which must fit inside one element at the access's width;
//! * the **count**, from a bound check on the same leaf register (`sltiu t,
//!   leaf, N` feeding a branch) when the consumer states one, and otherwise
//!   from the next address the image forms that is *not* a field of this
//!   array ([`claim_indexed_arrays`]).
//!
//! The pointer-bump form ([`pointer_bump_arrays`]) is a counted loop whose
//! body advances a pointer by a constant (`addiu p,p,S`) instead of scaling
//! an index. Its extent is `count * S` from each value the pointer starts at:
//! a formed base, or every word of a counted pointer table the loop is
//! handed an element of (PROT 0976's seventeen per-fighter blocks, each nine
//! `0x60`-byte records, walked by `FUN_801D553C`).

use super::{
    Flow, OWNER_CODE, OWNER_GLOBAL, OWNER_RECORD, RegSource, Sink, defines, loop_bounded_arrays,
    lui_forms, reg_source,
};
use std::collections::{BTreeMap, BTreeSet};

/// Longest backward walk the index evaluator takes, in words.
const AFFINE_WINDOW: usize = 16;
/// Recursion bound on the index evaluator.
const AFFINE_DEPTH: u32 = 8;
/// Largest element size either rule accepts.
const MAX_STRIDE: i64 = 0x2000;
/// Largest element count either rule accepts.
const MAX_COUNT: usize = 0x1000;

fn word(buf: &[u8], o: usize) -> Option<u32> {
    legaia_bytes::u32_le(buf, o)
}

fn simm(w: u32) -> i64 {
    (w & 0xFFFF) as i16 as i64
}

/// A register's value as `k * leaf + c`, where `leaf` names the register
/// and the offset of the instruction that last wrote it (or `None` when no
/// writer is in reach - a routine argument).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Affine {
    leaf: Option<(u32, Option<usize>)>,
    k: i64,
    c: i64,
}

impl Affine {
    fn constant(c: i64) -> Self {
        Self {
            leaf: None,
            k: 0,
            c,
        }
    }
    fn leaf(reg: u32, at: Option<usize>) -> Self {
        Self {
            leaf: Some((reg, at)),
            k: 1,
            c: 0,
        }
    }
    fn add(self, o: Self, sign: i64) -> Option<Self> {
        let leaf = match (self.k, o.k) {
            (0, 0) => None,
            (0, _) => o.leaf,
            (_, 0) => self.leaf,
            _ if self.leaf == o.leaf => self.leaf,
            _ => return None,
        };
        let k = self.k + sign * o.k;
        Some(Self {
            leaf: if k == 0 { None } else { leaf },
            k,
            c: self.c + sign * o.c,
        })
    }
}

/// Evaluate `reg` as it stands just before the word at `from`, walking the
/// straight-line code backwards.
///
/// The walk stops - and `reg` becomes a leaf - at its last writer when that
/// writer is anything but `sll`, `addu`, `subu`, `addiu` or a copy; at a
/// routine's prologue; past an unconditional jump (the code above it does
/// not fall through); and past a call when `reg` is caller-saved.
fn affine_before(buf: &[u8], base_va: u32, from: usize, reg: u32, depth: u32) -> Option<Affine> {
    if reg == 0 {
        return Some(Affine::constant(0));
    }
    if depth == 0 {
        return None;
    }
    let mut o = from;
    for _ in 0..AFFINE_WINDOW {
        let Some(prev) = o.checked_sub(4) else {
            return Some(Affine::leaf(reg, None));
        };
        o = prev;
        let w = word(buf, o)?;
        if w >> 16 == 0x27BD && w & 0x8000 != 0 {
            return Some(Affine::leaf(reg, None));
        }
        match Flow::of(w, o, base_va) {
            Flow::Jump(_) | Flow::Return => return Some(Affine::leaf(reg, Some(o))),
            Flow::Call if super::CALLER_SAVED.contains(&reg) => {
                return Some(Affine::leaf(reg, Some(o)));
            }
            _ => {}
        }
        if defines(w) != Some(reg) {
            continue;
        }
        let (op, rs, rt) = (w >> 26, (w >> 21) & 31, (w >> 16) & 31);
        return match (op, w & 0x3F) {
            (0x00, 0x00) => {
                let a = affine_before(buf, base_va, o, rt, depth - 1)?;
                let s = (w >> 6) & 31;
                Some(Affine {
                    k: a.k << s,
                    c: a.c << s,
                    ..a
                })
            }
            (0x00, 0x21) | (0x00, 0x25) if rs == 0 || rt == 0 => {
                affine_before(buf, base_va, o, rs | rt, depth - 1)
            }
            (0x00, 0x21) | (0x00, 0x23) => {
                let a = affine_before(buf, base_va, o, rs, depth - 1)?;
                let b = affine_before(buf, base_va, o, rt, depth - 1)?;
                a.add(b, if w & 0x3F == 0x23 { -1 } else { 1 })
            }
            (0x09, _) if rs == 0 => Some(Affine::constant(simm(w))),
            (0x09, _) => {
                let a = affine_before(buf, base_va, o, rs, depth - 1)?;
                Some(Affine {
                    c: a.c + simm(w),
                    ..a
                })
            }
            _ => Some(Affine::leaf(reg, Some(o))),
        };
    }
    Some(Affine::leaf(reg, None))
}

/// Width of a load / store opcode, or `None`.
fn access_width(op: u32) -> Option<i64> {
    match op {
        0x20 | 0x24 | 0x28 => Some(1),
        0x21 | 0x25 | 0x29 => Some(2),
        0x23 | 0x2B => Some(4),
        // `lwl` / `lwr` / `swl` / `swr`: half of an unaligned word access; each
        // touches at least the byte its displacement names.
        0x22 | 0x26 | 0x2A | 0x2E => Some(1),
        _ => None,
    }
}

/// One array the image's own code indexes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedArray {
    /// Array base VA.
    pub base: u32,
    /// Element size, off the index arithmetic.
    pub stride: u32,
    /// Byte offsets inside an element the accesses read or write, each with
    /// the access widths seen there.
    pub fields: BTreeMap<u32, BTreeSet<u32>>,
    /// Element count when a bound check states it (`sltiu t, leaf, N`, plus
    /// the highest constant element offset an access adds to the leaf).
    pub bound: Option<u32>,
    /// VA of the `addu` that forms one element address (the first found).
    pub site: u32,
    /// VA of the bound check, when there is one.
    pub bound_site: Option<u32>,
}

/// The bound check on `leaf`: `slti` / `sltiu t, leaf, N` within the 32 words
/// above `at`, below the leaf's own writer, with `t` tested by the next
/// branch (at most two words down).
fn bound_on(buf: &[u8], at: usize, leaf: (u32, Option<usize>)) -> Option<(u32, usize)> {
    let (reg, writer) = leaf;
    let floor = writer.map_or(0, |w| w + 4);
    let mut o = at;
    for _ in 0..32 {
        o = o.checked_sub(4)?;
        if o < floor {
            return None;
        }
        let w = word(buf, o)?;
        if matches!(w >> 26, 0x0A | 0x0B) && (w >> 21) & 31 == reg {
            let t = (w >> 16) & 31;
            let n = simm(w);
            let tested = (1..=2).any(|j| {
                word(buf, o + 4 * j).is_some_and(|b| {
                    matches!(b >> 26, 0x04 | 0x05) && (b >> 21) & 31 == t && (b >> 16) & 31 == 0
                })
            });
            return (tested && n >= 1).then_some((n as u32, o));
        }
    }
    None
}

/// Every array the image's own code indexes at a runtime value, one entry per
/// base. `in_code` says whether a file offset is a dumped instruction of this
/// image: an `addu` elsewhere is data, not a consumer.
pub fn indexed_arrays(
    buf: &[u8],
    base_va: u32,
    in_code: &dyn Fn(usize) -> bool,
) -> Vec<IndexedArray> {
    let forms = lui_forms(buf, base_va);
    let mut by_base: BTreeMap<u32, Vec<IndexedArray>> = BTreeMap::new();
    let mut o = 0usize;
    while o + 4 <= buf.len() {
        let at = o;
        o += 4;
        let Some(w) = word(buf, at) else { break };
        if w >> 26 != 0 || w & 0x3F != 0x21 || !in_code(at) {
            continue;
        }
        let (rs, rt, rd) = ((w >> 21) & 31, (w >> 16) & 31, (w >> 11) & 31);
        if rd == 0 || rs == 0 || rt == 0 {
            continue;
        }
        // An `addu` in a call's delay slot sees the registers the call's own
        // word sees; evaluate its operands from above the call.
        let is_call_word = |w: u32| w >> 26 == 0x03 || (w >> 26 == 0 && w & 0x3F == 0x09);
        let eval_at = if at >= 4 && word(buf, at - 4).is_some_and(is_call_word) {
            at - 4
        } else {
            at
        };
        for (rb, ri) in [(rs, rt), (rt, rs)] {
            // The base: a formed `addiu`, or a bare `lui` whose low half the
            // access below supplies as its displacement.
            let formed = match eval_at
                .checked_sub(4)
                .and_then(|f| reg_source(buf, base_va, f, &forms, rb, None))
            {
                Some(RegSource::Formed(t)) => Some(t),
                _ => None,
            };
            let lui_hi = if formed.is_none() {
                bare_lui_before(buf, base_va, eval_at, rb)
            } else {
                None
            };
            if formed.is_none() && lui_hi.is_none() {
                continue;
            }
            let Some(a) = affine_before(buf, base_va, eval_at, ri, AFFINE_DEPTH) else {
                continue;
            };
            let Some(leaf) = a.leaf else { continue };
            if !(2..=MAX_STRIDE).contains(&a.k) {
                continue;
            }
            let k = a.k;
            let bound = bound_on(buf, eval_at, leaf);
            // Accesses through the element address, forward in straight line.
            // An element address handed to a call in an argument register
            // (the `addu` in the call's delay slot, or a call before `rd` is
            // rewritten) is an access to the element's start: the callee
            // reads the record, at a field the caller does not state.
            let arg_reg = (4..=7).contains(&rd);
            let in_delay_slot = eval_at != at;
            let mut q = at + 4;
            let mut after_flow = false;
            let mut handed = arg_reg && in_delay_slot;
            for step in 0..=32 {
                let z = if handed { 0 } else { word(buf, q).unwrap_or(0) };
                if !handed && step == 32 {
                    break;
                }
                if !handed && arg_reg && is_call_word(z) {
                    // The call's delay slot runs first; if it rewrites the
                    // register, the callee receives something else.
                    if word(buf, q + 4).is_some_and(|ds| defines(ds) == Some(rd)) {
                        break;
                    }
                    handed = true;
                }
                if handed && formed.is_none() {
                    // The bare-`lui` form takes its low half from the access.
                    break;
                }
                let access = if handed {
                    Some((1, 0))
                } else {
                    access_width(z >> 26)
                        .filter(|_| (z >> 21) & 31 == rd)
                        .map(|wd| (wd, simm(z)))
                };
                if let Some((wd, d)) = access {
                    let (arr_base, off) = match (formed, lui_hi) {
                        (Some(b), _) => (b as i64, a.c + d),
                        (None, Some(hi)) => (hi as i64 + d, a.c),
                        _ => unreachable!(),
                    };
                    let elem = off.div_euclid(k);
                    let field = off.rem_euclid(k);
                    if field + wd <= k && elem >= 0 {
                        let entry = IndexedArray {
                            base: arr_base as u32,
                            stride: k as u32,
                            fields: [(field as u32, [wd as u32].into_iter().collect())]
                                .into_iter()
                                .collect(),
                            bound: bound.map(|(n, _)| n + elem as u32),
                            site: base_va.wrapping_add(at as u32),
                            bound_site: bound.map(|(_, s)| base_va.wrapping_add(s as u32)),
                        };
                        by_base.entry(entry.base).or_default().push(entry);
                    }
                }
                if handed || defines(z) == Some(rd) || after_flow {
                    break;
                }
                after_flow = !matches!(Flow::of(z, q, base_va), Flow::None);
                q += 4;
            }
        }
    }
    // The bare-`lui` form puts the field in the base (`lo = base + field`):
    // fold same-stride bases closer together than one element into the
    // lowest, which is the array's.
    let mut out: Vec<IndexedArray> = Vec::new();
    for (base, v) in by_base {
        let strides: BTreeSet<u32> = v.iter().map(|a| a.stride).collect();
        if strides.len() != 1 {
            continue;
        }
        let stride = v[0].stride;
        let mut merged = v[0].clone();
        for a in &v[1..] {
            for (f, w) in &a.fields {
                merged.fields.entry(*f).or_default().extend(w);
            }
            merged.bound = merged.bound.max(a.bound);
            merged.bound_site = merged.bound_site.or(a.bound_site);
        }
        if let Some(prev) = out.last_mut()
            && prev.stride == stride
            && base - prev.base < stride
        {
            let shift = base - prev.base;
            for (f, w) in &merged.fields {
                prev.fields.entry(f + shift).or_default().extend(w);
            }
            prev.bound = prev.bound.max(merged.bound);
            prev.bound_site = prev.bound_site.or(merged.bound_site);
            continue;
        }
        out.push(merged);
    }
    out.retain(|a| a.fields.keys().all(|&f| f < a.stride));
    out
}

/// The high half a bare `lui rb, hi` left in `rb`, when it is `rb`'s last
/// writer in the straight-line code above `at` (copies followed).
fn bare_lui_before(buf: &[u8], base_va: u32, at: usize, mut rb: u32) -> Option<u32> {
    let mut o = at;
    for _ in 0..AFFINE_WINDOW {
        o = o.checked_sub(4)?;
        let w = word(buf, o)?;
        match Flow::of(w, o, base_va) {
            Flow::Jump(_) | Flow::Return => return None,
            Flow::Call if super::CALLER_SAVED.contains(&rb) => return None,
            _ => {}
        }
        if defines(w) != Some(rb) {
            continue;
        }
        let (op, rs, rt) = (w >> 26, (w >> 21) & 31, (w >> 16) & 31);
        if op == 0x0F {
            return Some((w & 0xFFFF) << 16);
        }
        if op == 0 && matches!(w & 0x3F, 0x21 | 0x25) && (rs == 0) != (rt == 0) {
            rb = rs | rt;
            continue;
        }
        return None;
    }
    None
}

/// A counted loop that walks an array by bumping a pointer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BumpArray {
    /// Every VA the pointer starts at: one formed base, or each word of the
    /// counted pointer table the loop is handed an element of.
    pub bases: Vec<u32>,
    /// Elements: the loop bound.
    pub count: u32,
    /// The pointer bump, bytes per element.
    pub stride: u32,
    /// VA of the bound check.
    pub bound_site: u32,
    /// VA of the pointer table the bases came from, when they did.
    pub table: Option<u32>,
}

/// Counted loops over a bumped pointer.
///
/// The loop test is [`super::loop_bounded_arrays`]'s (a backward `bnez` /
/// `beqz` on `slti` / `sltiu i, N`, `i` zeroed within eight words above the
/// loop and bumped by one in it). The body must also hold `addiu p,p,S` with
/// `S > 0` and an access `off(p)` with `0 <= off < S`. The pointer's value on
/// entry is its last writer above the loop: a formed `addiu`, or - through a
/// copy - a `lw` of an element of a pointer table that
/// [`super::loop_bounded_arrays`] sized, in which case every in-image word of
/// that table is a base.
pub fn pointer_bump_arrays(buf: &[u8], base_va: u32) -> Vec<BumpArray> {
    let forms = lui_forms(buf, base_va);
    let tables: Vec<_> = loop_bounded_arrays(buf, base_va)
        .into_iter()
        .filter(|t| t.stride == 4)
        .collect();
    let in_image =
        |v: u32| v >= base_va && ((v - base_va) as usize) < buf.len() && v.is_multiple_of(4);
    let mut out: Vec<BumpArray> = Vec::new();
    let mut p = 0usize;
    while p + 8 <= buf.len() {
        let at = p;
        p += 4;
        let v = word(buf, at).unwrap_or(0);
        if !matches!(v >> 26, 0x04 | 0x05) || (v >> 16) & 0x1F != 0 {
            continue;
        }
        let l = at as i64 + 4 + (simm(v) << 2);
        if l < 0 || l as usize >= at || at - l as usize > 0x400 {
            continue;
        }
        let l = l as usize;
        let t = (v >> 21) & 0x1F;
        let mut bound = None;
        for k in 1..=3 {
            let Some(o) = at.checked_sub(4 * k) else {
                break;
            };
            let x = word(buf, o).unwrap_or(0);
            if matches!(x >> 26, 0x0A | 0x0B) && (x >> 16) & 0x1F == t {
                bound = Some((o, (x >> 21) & 0x1F, simm(x)));
                break;
            }
            if defines(x) == Some(t) {
                break;
            }
        }
        let Some((bound_off, i_reg, n)) = bound else {
            continue;
        };
        if !(2..=MAX_COUNT as i64).contains(&n) || i_reg == 0 {
            continue;
        }
        let body: Vec<usize> = (l..at + 8).step_by(4).collect();
        let bump = (0x09 << 26) | (i_reg << 21) | (i_reg << 16) | 1;
        if !body.iter().any(|&o| word(buf, o) == Some(bump)) {
            continue;
        }
        let mut zeroed = false;
        for k in 1..=8 {
            let Some(o) = l.checked_sub(4 * k) else { break };
            let x = word(buf, o).unwrap_or(0);
            if defines(x) == Some(i_reg) {
                let (rs, rt) = ((x >> 21) & 0x1F, (x >> 16) & 0x1F);
                zeroed = (x >> 26 == 0x09 && rs == 0 && x & 0xFFFF == 0)
                    || (x >> 26 == 0 && matches!(x & 0x3F, 0x21 | 0x25) && rs == 0 && rt == 0);
                break;
            }
        }
        if !zeroed {
            continue;
        }
        for &o in &body {
            let x = word(buf, o).unwrap_or(0);
            let (rs, rt) = ((x >> 21) & 31, (x >> 16) & 31);
            if x >> 26 != 0x09 || rs != rt || rs == i_reg || rs == 0 || simm(x) <= 0 {
                continue;
            }
            let (preg, stride) = (rs, simm(x));
            if stride > MAX_STRIDE {
                continue;
            }
            let accessed = body.iter().any(|&b| {
                word(buf, b).is_some_and(|z| {
                    access_width(z >> 26).is_some()
                        && (z >> 21) & 31 == preg
                        && (0..stride).contains(&simm(z))
                })
            });
            if !accessed {
                continue;
            }
            let (bases, table) = match pointer_origin(buf, base_va, l, preg, &forms, &tables) {
                Some(Origin::Formed(b)) => (vec![b], None),
                Some(Origin::Table(tb)) => {
                    let words: Vec<u32> = (0..tb.count as usize)
                        .filter_map(|j| {
                            let off = (tb.base - base_va) as usize + 4 * j;
                            word(buf, off)
                        })
                        .collect();
                    if !words.iter().all(|&w| in_image(w)) {
                        continue;
                    }
                    (words, Some(tb.base))
                }
                None => continue,
            };
            out.push(BumpArray {
                bases,
                count: n as u32,
                stride: stride as u32,
                bound_site: base_va.wrapping_add(bound_off as u32),
                table,
            });
        }
    }
    out
}

enum Origin {
    Formed(u32),
    Table(super::BoundedArray),
}

/// Where a loop's pointer register starts, read above the loop head `l`.
fn pointer_origin(
    buf: &[u8],
    base_va: u32,
    l: usize,
    mut reg: u32,
    forms: &[super::LuiForm],
    tables: &[super::BoundedArray],
) -> Option<Origin> {
    if let Some(RegSource::Formed(t)) =
        reg_source(buf, base_va, l.checked_sub(4)?, forms, reg, None)
    {
        return Some(Origin::Formed(t));
    }
    let mut o = l;
    for _ in 0..32 {
        o = o.checked_sub(4)?;
        let w = word(buf, o)?;
        if w >> 16 == 0x27BD && w & 0x8000 != 0 {
            return None;
        }
        match Flow::of(w, o, base_va) {
            Flow::Jump(_) | Flow::Return => return None,
            Flow::Call if super::CALLER_SAVED.contains(&reg) => return None,
            _ => {}
        }
        if defines(w) != Some(reg) {
            continue;
        }
        let (op, rs, rt) = (w >> 26, (w >> 21) & 31, (w >> 16) & 31);
        if op == 0 && matches!(w & 0x3F, 0x21 | 0x25) && (rs == 0) != (rt == 0) {
            reg = rs | rt;
            continue;
        }
        // `lw reg, 0(e)` with `e = table + i*4`.
        if op != 0x23 || w & 0xFFFF != 0 {
            return None;
        }
        let e = rs;
        let mut q = o;
        for _ in 0..8 {
            q = q.checked_sub(4)?;
            let x = word(buf, q)?;
            if defines(x) != Some(e) {
                continue;
            }
            if !(x >> 26 == 0 && x & 0x3F == 0x21) {
                return None;
            }
            for r in [(x >> 21) & 31, (x >> 16) & 31] {
                if let Some(RegSource::Formed(t)) =
                    reg_source(buf, base_va, q.checked_sub(4)?, forms, r, None)
                    && let Some(tb) = tables.iter().find(|tb| tb.base == t)
                {
                    return Some(Origin::Table(*tb));
                }
            }
            return None;
        }
        return None;
    }
    None
}

/// Claim [`pointer_bump_arrays`]: `count * stride` bytes at every base.
pub(super) fn claim_pointer_bump_arrays(
    buf: &[u8],
    sink: &mut Sink,
    base: u32,
    own_end: usize,
    in_code: &dyn Fn(usize) -> bool,
) {
    let mut n = 0usize;
    let mut bytes = 0usize;
    for a in pointer_bump_arrays(&buf[..own_end], base) {
        if !in_code((a.bound_site - base) as usize) {
            continue;
        }
        for &b in &a.bases {
            let off = (b - base) as usize;
            let end = off + (a.count * a.stride) as usize;
            if end > own_end || in_code(off) {
                continue;
            }
            let from = match a.table {
                Some(t) => format!(", start read from the pointer table at {t:#010x}"),
                None => String::new(),
            };
            sink.claim(
                off,
                end,
                OWNER_RECORD,
                format!(
                    "array of {} x {} B walked by a pointer bump, count from the loop bound \
                     at {:#010x}{from}",
                    a.count, a.stride, a.bound_site
                ),
            );
            n += 1;
            bytes += end - off;
        }
    }
    if n > 0 {
        sink.note(format!(
            "{n} array(s) walked by a pointer-bumping counted loop ({bytes} bytes)"
        ));
    }
}

/// Claim [`indexed_arrays`].
///
/// A bound check states the count outright. Without one the array runs from
/// its base to the next address this image forms that is **not** one of its
/// own elements' fields, or to the next claim that is not, whichever is lower -
/// and it is kept only when that distance is a whole number of elements, give
/// or take word-alignment padding. An address counts as this array's when it is
/// a field of element 0 (the consumer's own base-plus-field form), when it is a
/// fixed load or store of a nonzero field at a width an indexed access reads
/// there, or when it is an element start whose nonzero fields are so accessed
/// (the consumer addressing element `m` by constant). A bare element start with
/// no field evidence stops the array: a word array is indistinguishable from
/// the scalar after it. The reason line says which evidence sized it.
pub(super) fn claim_indexed_arrays(
    buf: &[u8],
    sink: &mut Sink,
    base: u32,
    own_end: usize,
    in_code: &dyn Fn(usize) -> bool,
) {
    let arrays = indexed_arrays(&buf[..own_end], base, in_code);
    // Every address the image forms, with the width of the fixed access that
    // forms it (`None` for an `addiu` / `ori` or an indexed base).
    let mut fixed: BTreeMap<u32, BTreeSet<Option<u32>>> = BTreeMap::new();
    // Targets of the indexed `lui at,hi; addu at,at,rX; op y,lo(at)` form:
    // `hi + lo` is an array base plus a field, never a scalar of its own.
    let mut indexed: BTreeSet<u32> = BTreeSet::new();
    for f in lui_forms(&buf[..own_end], base) {
        if f.index.is_some() {
            indexed.insert(f.target);
        }
        let w = if f.index.is_none() {
            access_width(f.op).map(|w| w as u32)
        } else {
            None
        };
        fixed.entry(f.target).or_default().insert(w);
    }
    for a in &arrays {
        fixed.entry(a.base).or_default().insert(None);
    }
    let barriers: Vec<usize> = sink
        .claims
        .iter()
        .filter(|c| c.owner != OWNER_GLOBAL)
        .map(|c| c.start)
        .collect();
    let code_ends: Vec<(usize, usize)> = sink
        .claims
        .iter()
        .filter(|c| c.owner == OWNER_CODE)
        .map(|c| (c.start, c.end))
        .collect();
    let mut claimed = 0usize;
    let mut bytes = 0usize;
    for a in arrays {
        let Some(off) = a.base.checked_sub(base).map(|o| o as usize) else {
            continue;
        };
        if off >= own_end || in_code(off) {
            continue;
        }
        let stride = a.stride as usize;
        let (end, why) = if let Some(nb) = a.bound {
            (
                off + nb as usize * stride,
                format!(
                    "count {nb} from the bound check at {:#010x}",
                    a.bound_site.unwrap_or(0)
                ),
            )
        } else {
            let accessed_as_field = |t: u32, f: u32| {
                a.fields.get(&f).is_some_and(|ws| {
                    fixed
                        .get(&t)
                        .is_some_and(|seen| seen.iter().any(|w| w.is_some_and(|w| ws.contains(&w))))
                })
            };
            let own = |t: u32| {
                if t < a.base {
                    return false;
                }
                let rel = t - a.base;
                let (e, f) = (rel / a.stride, rel % a.stride);
                if f != 0 {
                    (e == 0 && indexed.contains(&t))
                        || (a.fields.contains_key(&f) && (e == 0 || accessed_as_field(t, f)))
                } else {
                    e == 0
                        || a.fields
                            .keys()
                            .any(|&ff| ff != 0 && accessed_as_field(t + ff, ff))
                }
            };
            let next_anchor = fixed
                .range(a.base + 1..)
                .map(|(&t, _)| t)
                .find(|&t| !own(t))
                .map(|t| (t - base) as usize);
            let next_claim = barriers
                .iter()
                .copied()
                .filter(|&s| s > off && !own(base.wrapping_add(s as u32)))
                .min();
            let next_code = code_ends.iter().map(|c| c.0).filter(|&s| s > off).min();
            let end = [next_anchor, next_claim, next_code, Some(own_end)]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(own_end);
            let mut count = (end - off) / stride;
            // A table of code or data pointers ends at its last in-image word.
            let is_ptr = |j: usize| {
                legaia_bytes::u32_le(buf, off + 4 * j).is_some_and(|w| {
                    w.is_multiple_of(4) && w >= base && ((w - base) as usize) < own_end
                })
            };
            if stride == 4 && is_ptr(0) {
                count = (0..count).take_while(|&j| is_ptr(j)).count();
            }
            let end = if stride == 4 && is_ptr(0) {
                off + count * 4
            } else {
                end
            };
            if (end - off) % stride >= 4 || count < 2 {
                continue;
            }
            (
                off + count * stride,
                "count to the next address the image forms outside the array".to_string(),
            )
        };
        let count = (end - off) / stride;
        if end > own_end || count > MAX_COUNT || (off + 1..end).any(&in_code) {
            continue;
        }
        sink.claim(
            off,
            end,
            OWNER_RECORD,
            format!(
                "array of {count} x {stride} B indexed at a runtime value (element address \
                 formed at {:#010x}, stride off the index arithmetic), {why}",
                a.site
            ),
        );
        claimed += 1;
        bytes += end - off;
    }
    if claimed > 0 {
        sink.note(format!(
            "{claimed} runtime-indexed array(s) sized off their consumers ({bytes} bytes)"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(words: &[u32], len: usize) -> Vec<u8> {
        let mut v: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        v.resize(len, 0);
        v
    }
    const fn lui(rt: u32, imm: u32) -> u32 {
        0x3C00_0000 | rt << 16 | imm
    }
    const fn addiu(rt: u32, rs: u32, imm: u32) -> u32 {
        0x2400_0000 | rs << 21 | rt << 16 | (imm & 0xFFFF)
    }
    const fn sll(rd: u32, rt: u32, sa: u32) -> u32 {
        rt << 16 | rd << 11 | sa << 6
    }
    const fn r3(rd: u32, rs: u32, rt: u32, funct: u32) -> u32 {
        rs << 21 | rt << 16 | rd << 11 | funct
    }
    const fn mem(op: u32, rt: u32, base: u32, off: u32) -> u32 {
        op << 26 | base << 21 | rt << 16 | (off & 0xFFFF)
    }

    #[test]
    fn a_strength_reduced_index_states_the_stride() {
        // PROT 0897 at 0x801ECA24: base 0x801F2B98 in v0, `((a3 << 3) - a3)
        // << 2` = a3 * 28, element address in a0, `lh s6, 8(a0)`.
        let w = [
            lui(2, 0x8000),
            addiu(2, 2, 0x40),
            sll(4, 7, 3),
            r3(4, 4, 7, 0x23),
            sll(4, 4, 2),
            r3(4, 4, 2, 0x21),
            mem(0x21, 22, 4, 8),
            mem(0x21, 23, 4, 0xA),
        ];
        let b = img(&w, 0x200);
        let a = indexed_arrays(&b, 0x8000_0000, &|o| o < 0x20);
        assert_eq!(a.len(), 1);
        assert_eq!((a[0].base, a[0].stride), (0x8000_0040, 28));
        assert_eq!(
            a[0].fields.keys().copied().collect::<Vec<_>>(),
            vec![8, 0xA]
        );
        assert_eq!(a[0].bound, None);
    }

    #[test]
    fn a_bound_check_on_the_leaf_states_the_count() {
        // sltiu v1, a1, 5; beqz v1, out; ... a1 * 12 + base; lw
        let w = [
            mem(0x0B, 3, 5, 5), // sltiu v1,a1,5
            0x1060_0010,        // beqz v1
            0,
            lui(2, 0x8000),
            addiu(2, 2, 0x80),
            sll(3, 5, 1),
            r3(3, 3, 5, 0x21), // a1*3
            sll(3, 3, 2),      // *12
            r3(3, 3, 2, 0x21),
            mem(0x23, 4, 3, 4),
        ];
        let b = img(&w, 0x200);
        let a = indexed_arrays(&b, 0x8000_0000, &|o| o < 0x28);
        assert_eq!(a.len(), 1);
        assert_eq!(
            (a[0].base, a[0].stride, a[0].bound),
            (0x8000_0080, 12, Some(5))
        );
    }

    #[test]
    fn two_leaves_summed_state_no_stride() {
        // v0 = a1*13 + t5*65: a 2-D index, which one stride cannot describe.
        let w = [
            lui(2, 0x8000),
            addiu(14, 2, 0x80), // t6 = base
            sll(17, 13, 6),
            r3(16, 17, 13, 0x21), // s0 = t5*65
            sll(10, 5, 1),
            r3(2, 10, 5, 0x21),
            sll(2, 2, 2),
            r3(2, 2, 5, 0x21), // a1*13
            r3(2, 2, 16, 0x21),
            r3(2, 2, 14, 0x21),
            mem(0x24, 3, 2, 0),
        ];
        let b = img(&w, 0x200);
        assert!(indexed_arrays(&b, 0x8000_0000, &|o| o < 0x2C).is_empty());
    }

    #[test]
    fn a_pointer_bump_loop_sizes_its_array() {
        let w = [
            lui(18, 0x8000),
            addiu(18, 18, 0x40), // s2 = base
            addiu(21, 0, 0),     // s5 = 0
            mem(0x23, 2, 18, 8), // L: lw v0, 8(s2)
            addiu(21, 21, 1),
            mem(0x0A, 2, 21, 3), // slti v0, s5, 3
            0x1440_FFFC,         // bnez v0, L
            addiu(18, 18, 0x10), // s2 += 0x10
        ];
        let b = img(&w, 0x100);
        let a = pointer_bump_arrays(&b, 0x8000_0000);
        assert_eq!(a.len(), 1);
        assert_eq!(
            (a[0].bases.clone(), a[0].count, a[0].stride, a[0].table),
            (vec![0x8000_0040], 3, 0x10, None)
        );
    }
}
