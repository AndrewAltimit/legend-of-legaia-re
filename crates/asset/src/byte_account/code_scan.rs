//! Instruction scans over a code image: its uninitialised data region, `lui`-pair forms and loop-bounded arrays.
//! Split out of `byte_account.rs`.

use super::*;

// ---------------------------------------------------------------------------
// An image's own uninitialised data region
// ---------------------------------------------------------------------------

/// Shortest all-zero run considered as an image's uninitialised data region.
pub const BSS_RUN_MIN: usize = 256;

/// Sites that must form one single address inside a zero run for it to count
/// as addressed, when no second distinct address does.
pub const BSS_MIN_SITES: usize = 4;

/// How many instructions after a `lui` its half may be completed in.
///
/// A MIPS address materialises as `lui rt, hi` plus a second instruction that
/// uses `rt` as its base, and the assembler is free to put anything in
/// between - including the `jal` whose delay slot carries the pair's low half,
/// which is where the STR overlay hands the VLC unpacker its destination
/// (`0x801CF214` / `0x801CF218`). The window is walked forward and abandoned
/// the moment something redefines `rt`, so a stale high half can never be
/// paired with an unrelated low one; a backward-only scan from the second
/// instruction misses the delay-slot form entirely.
pub(super) const LUI_PAIR_WINDOW: usize = 16;

/// The register a MIPS word writes, or `None` for the forms that write none.
pub(super) fn defines(w: u32) -> Option<u32> {
    let op = w >> 26;
    let rt = (w >> 16) & 0x1F;
    match op {
        // SPECIAL: `rd`, except the two jump-register forms.
        0x00 => match w & 0x3F {
            0x08 => None,     // jr
            0x09 => Some(31), // jalr (retail always links to ra)
            _ => Some((w >> 11) & 0x1F),
        },
        0x01 | 0x04..=0x07 => None, // branches
        0x02 => None,               // j
        0x03 => Some(31),           // jal
        0x08..=0x0F => Some(rt),    // immediate ALU + lui
        0x20..=0x25 => Some(rt),    // loads
        0x28..=0x2B => None,        // stores
        0x10 | 0x12 => match (w >> 21) & 0x1F {
            0x00 | 0x02 => Some(rt), // mfc0 / mfc2
            _ => None,
        },
        _ => Some(rt),
    }
}

/// Registers a call may clobber under the o32 convention: `at`, `v0`-`v1`,
/// `a0`-`a3`, `t0`-`t9`, `ra`.
pub(super) const CALLER_SAVED: [u32; 18] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 24, 25, 31,
];

/// The control transfer a word makes, as far as a register walk cares.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Flow {
    None,
    /// `jal` / `jalr`: the walk goes on past the delay slot, less the
    /// caller-saved registers.
    Call,
    /// `jr ra`: the path ends.
    Return,
    /// `j` / `b`: the path continues at the target only (file offset).
    Jump(usize),
    /// A conditional branch: both the fall-through and the target.
    Branch(usize),
}

impl Flow {
    pub(super) fn of(w: u32, at: usize, base_va: u32) -> Self {
        let op = w >> 26;
        let rs = (w >> 21) & 0x1F;
        let rt = (w >> 16) & 0x1F;
        let rel = (at as i64 + 4 + (((w & 0xFFFF) as i16 as i64) << 2)).max(0) as usize;
        match (op, w & 0x3F) {
            (0x03, _) | (0x00, 0x09) => Flow::Call,
            (0x00, 0x08) if rs == 31 => Flow::Return,
            (0x02, _) => {
                let va = ((base_va.wrapping_add(at as u32 + 4)) & 0xF000_0000)
                    | ((w & 0x03FF_FFFF) << 2);
                Flow::Jump(va.wrapping_sub(base_va) as usize)
            }
            (0x04, _) if rs == 0 && rt == 0 => Flow::Jump(rel),
            (0x01, _) if rs == 0 && rt == 0x01 => Flow::Jump(rel),
            (0x04..=0x07, _) => Flow::Branch(rel),
            (0x01, _) if matches!(rt, 0x00 | 0x01 | 0x10 | 0x11) => Flow::Branch(rel),
            _ => Flow::None,
        }
    }
}

/// One address the image's own code materialises from a `lui`, as the
/// instruction that completes it sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LuiForm {
    /// VA of the completing instruction (`addiu` / `ori` / load / store).
    pub site: u32,
    /// The address the instruction forms or touches - or, for an indexed
    /// form, the base of the array the index walks.
    pub target: u32,
    /// Primary opcode of the completing instruction.
    pub op: u32,
    /// The register holding the runtime index, when the `lui` register was
    /// summed with one before the low half was applied.
    pub index: Option<u32>,
}

/// A register a walk is carrying: `(register, value, index register)`.
pub(super) type Held = (u32, u32, Option<u32>);

/// The walk state after `v` runs.
pub(super) fn walk_apply(v: u32, st: &mut Vec<Held>) {
    let op = v >> 26;
    let rs = (v >> 21) & 0x1F;
    let rt = (v >> 16) & 0x1F;
    let low = v & 0xFFFF;
    let held = |st: &Vec<Held>, r: u32| st.iter().find(|x| x.0 == r).copied();
    // addiu / ori on a carried register: the register now holds more of
    // the address (the multi-step and base-plus-offset forms).
    if matches!(op, 0x09 | 0x0D)
        && let Some((_, value, index)) = held(st, rs)
    {
        st.retain(|x| x.0 != rt);
        if !(op == 0x0D && index.is_some()) && rt != 0 {
            let value = if op == 0x0D {
                value | low
            } else {
                value.wrapping_add(low as i16 as u32)
            };
            st.push((rt, value, index));
        }
        return;
    }
    // addu / or with one carried source: a copy, or one runtime index.
    if op == 0x00 && matches!(v & 0x3F, 0x21 | 0x25) {
        let rd = (v >> 11) & 0x1F;
        let (src, other) = match (held(st, rs), held(st, rt)) {
            (Some(s), None) => (Some(s), rt),
            (None, Some(s)) => (Some(s), rs),
            _ => (None, 0),
        };
        let next = match src {
            Some((_, value, index)) if other == 0 => Some((value, index)),
            Some((_, value, None)) if v & 0x3F == 0x21 => Some((value, Some(other))),
            _ => None,
        };
        if let (Some((value, index)), true) = (next, rd != 0) {
            st.retain(|x| x.0 != rd);
            st.push((rd, value, index));
            return;
        }
    }
    if let Some(d) = defines(v) {
        st.retain(|x| x.0 != d);
    }
}

/// Every address a `lui` forms, following its register through copies,
/// through one indexing `addu`, and across branches.
///
/// Three shapes complete a `lui rA, hi`:
///
/// * **pair** - `addiu`/`ori`/load/store with base `rA` (or with a register
///   that holds `rA` plus earlier `addiu`s: the multi-step and
///   base-plus-displacement forms);
/// * **copy** - `addu rB, rA, $zero` / `or rB, rA, $zero` first;
/// * **indexed** - `addu rB, rA, rX` with a runtime `rX`, then
///   `lw rY, lo(rB)`. The instruction stream carries `hi` and `lo` but never
///   `hi + lo`, and the `addu` redefines the register a pair-only walk is
///   following, so that walk stops one instruction short of the low half.
///   `hi + lo` is the base of the array `rX` indexes.
///
/// A register leaves the walk the moment anything else writes it, so a stale
/// high half is never paired with an unrelated low one. A branch forks the
/// walk (fall-through and target both inherit the state out of its delay
/// slot); `j` / `b` continue at the target only, because the word after them
/// belongs to another arm; `jr ra` ends the path; a call's delay slot still
/// sees every register - the STR overlay completes a pair there - but only
/// the callee-saved ones survive the call. A `lui` that itself sits in a
/// delay slot starts its walk where that jump goes. Every path is bounded by
/// [`LUI_PAIR_WINDOW`] instructions, and each word is walked at most once per
/// `lui`.
pub fn lui_forms(image: &[u8], base_va: u32) -> Vec<LuiForm> {
    let word = |off: usize| -> Option<u32> {
        image
            .get(off..off.checked_add(4)?)
            .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
    };
    let mut out: std::collections::BTreeSet<LuiForm> = std::collections::BTreeSet::new();
    let mut visited: Vec<usize> = Vec::new();
    let mut off = 0usize;
    while off + 4 <= image.len() {
        let Some(w) = word(off) else { break };
        let reg = (w >> 16) & 0x1F;
        if w >> 26 != 0x0F || reg == 0 {
            off += 4;
            continue;
        }
        let start: Vec<Held> = vec![(reg, (w & 0xFFFF) << 16, None)];
        // The jump whose delay slot this `lui` fills, if any, runs first.
        let prev = off
            .checked_sub(4)
            .and_then(word)
            .map(|p| Flow::of(p, off - 4, base_va))
            .unwrap_or(Flow::None);
        let mut stack: Vec<(usize, Vec<Held>, usize)> = match prev {
            Flow::Return => Vec::new(),
            Flow::Call => {
                let kept: Vec<Held> = start
                    .into_iter()
                    .filter(|x| !CALLER_SAVED.contains(&x.0))
                    .collect();
                vec![(off + 4, kept, LUI_PAIR_WINDOW)]
            }
            Flow::Jump(t) => vec![(t, start, LUI_PAIR_WINDOW)],
            Flow::Branch(t) => vec![
                (off + 4, start.clone(), LUI_PAIR_WINDOW),
                (t, start, LUI_PAIR_WINDOW),
            ],
            Flow::None => vec![(off + 4, start, LUI_PAIR_WINDOW)],
        };
        visited.clear();
        while let Some((mut at, mut st, mut budget)) = stack.pop() {
            let mut pending = Flow::None;
            while budget > 0 && !st.is_empty() {
                if pending == Flow::None && visited.contains(&at) {
                    break;
                }
                let Some(v) = word(at) else { break };
                visited.push(at);
                let op = v >> 26;
                let rs = (v >> 21) & 0x1F;
                if let Some(&(_, value, index)) = st.iter().find(|x| x.0 == rs) {
                    let low = v & 0xFFFF;
                    let target = match (op, index) {
                        // ori: the low half is zero-extended; meaningless on
                        // an indexed register.
                        (0x0D, None) => Some(value | low),
                        // addiu and every load/store form: sign-extended.
                        (0x09 | 0x20..=0x25 | 0x28..=0x2B, _) => {
                            Some(value.wrapping_add(low as i16 as u32))
                        }
                        _ => None,
                    };
                    if let Some(target) = target {
                        out.insert(LuiForm {
                            site: base_va.wrapping_add(at as u32),
                            target,
                            op,
                            index,
                        });
                    }
                }
                walk_apply(v, &mut st);
                budget -= 1;
                let here = Flow::of(v, at, base_va);
                match std::mem::replace(&mut pending, Flow::None) {
                    Flow::Return => break,
                    Flow::Call => st.retain(|x| !CALLER_SAVED.contains(&x.0)),
                    Flow::Jump(t) => {
                        at = t;
                        continue;
                    }
                    Flow::Branch(t) => stack.push((t, st.clone(), budget)),
                    Flow::None => pending = here,
                }
                at += 4;
            }
        }
        off += 4;
    }
    out.into_iter().collect()
}

/// Every `(site_va, target_va)` the image's own code forms from a `lui`
/// ([`lui_forms`] minus the indexed form).
///
/// This is the structural half of the uninitialised-data claim below: a zero
/// run is only that image's own declared buffer if the image's own code
/// computes an address inside it. Shape cannot say so - zero fill looks the
/// same whoever wrote it - which is why the test is a pointer-forming
/// instruction and not a byte statistic.
pub fn formed_addresses(image: &[u8], base_va: u32) -> Vec<(u32, u32)> {
    lui_forms(image, base_va)
        .into_iter()
        .filter(|f| f.index.is_none())
        .map(|f| (f.site, f.target))
        .collect()
}

/// An array whose element count a loop in the image's own code states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundedArray {
    /// Array base, as the consumer's `lui` / `addiu` forms it.
    pub base: u32,
    /// Elements: the loop bound `N` of `i < N`, `i` counted up from zero.
    pub count: u32,
    /// Bytes per element: `1 << s` for an `sll i, s` index, else 1.
    pub stride: u32,
    /// VA of the `addiu` that completes the base.
    pub form_site: u32,
    /// VA of the `slti` / `sltiu` that states the bound.
    pub bound_site: u32,
}

impl BoundedArray {
    /// Extent in bytes.
    pub fn byte_len(&self) -> usize {
        (self.count * self.stride) as usize
    }
}

/// Arrays whose count is pinned by the loop that walks them.
///
/// The shape is the counted loop the compiler emits over a formed base:
///
/// ```text
///         move/addiu i, $zero, 0          ; i = 0, within eight words of L
///     L:  [sll  j, i, s]                  ; stride 1 << s (or 1: j = i)
///         addu e, j, B                    ; B = lui / addiu base, written in
///         l?/s?  x, k(e)                  ;     the body or just before it
///         ...
///         addiu i, i, 1
///         slti/sltiu t, i, N              ; within three words of the branch
///         bnez t, L                       ; backward
/// ```
///
/// Every element of the claim is read off an instruction: the base off its
/// `lui` pair, the stride off the `sll`, the count off the bound, and the
/// access through `e` must be no wider than the stride with its offset inside
/// one element. Anything else - a pointer bump, a runtime bound, an index that
/// starts elsewhere - is left alone, which is why this finds few arrays: most
/// of the retail data segment is indexed by runtime values.
pub fn loop_bounded_arrays(image: &[u8], base_va: u32) -> Vec<BoundedArray> {
    let word = |o: usize| legaia_bytes::u32_le(image, o);
    let formed: std::collections::BTreeMap<usize, u32> = lui_forms(image, base_va)
        .into_iter()
        .filter(|f| f.op == 0x09 && f.index.is_none())
        .map(|f| (f.site.wrapping_sub(base_va) as usize, f.target))
        .collect();
    let width = |op: u32| match op {
        0x20 | 0x24 | 0x28 => Some(1u32),
        0x21 | 0x25 | 0x29 => Some(2),
        0x23 | 0x2B => Some(4),
        _ => None,
    };
    let mut out: Vec<BoundedArray> = Vec::new();
    let mut p = 0usize;
    while p + 8 <= image.len() {
        let v = word(p).unwrap_or(0);
        p += 4;
        let at = p - 4;
        // bnez / beqz t, L with L behind the branch.
        if !matches!(v >> 26, 0x04 | 0x05) || (v >> 16) & 0x1F != 0 {
            continue;
        }
        let l = at as i64 + 4 + (((v & 0xFFFF) as i16 as i64) << 2);
        if l < 0 || l as usize >= at || at - l as usize > 0x200 {
            continue;
        }
        let l = l as usize;
        let t = (v >> 21) & 0x1F;
        // slti / sltiu t, i, N just before it.
        let mut bound = None;
        for k in 1..=3 {
            let Some(o) = at.checked_sub(4 * k) else {
                break;
            };
            let x = word(o).unwrap_or(0);
            if matches!(x >> 26, 0x0A | 0x0B) && (x >> 16) & 0x1F == t {
                bound = Some((o, (x >> 21) & 0x1F, (x & 0xFFFF) as i16));
                break;
            }
            if defines(x) == Some(t) {
                break;
            }
        }
        let Some((bound_off, i_reg, n)) = bound else {
            continue;
        };
        if !(2..=0x1000).contains(&n) || i_reg == 0 {
            continue;
        }
        let body = (l..at + 8).step_by(4);
        let bump = (0x09 << 26) | (i_reg << 21) | (i_reg << 16) | 1;
        if !body.clone().any(|o| word(o) == Some(bump)) {
            continue;
        }
        // i = 0 before the loop.
        let mut zeroed = false;
        for k in 1..=8 {
            let Some(o) = l.checked_sub(4 * k) else { break };
            let x = word(o).unwrap_or(0);
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
        // The index and its scaled copies.
        let mut index: Vec<(u32, u32)> = vec![(i_reg, 1)];
        for o in body.clone() {
            let x = word(o).unwrap_or(0);
            let rd = (x >> 11) & 0x1F;
            if x >> 26 == 0 && x & 0x3F == 0 && (x >> 16) & 0x1F == i_reg && rd != i_reg && rd != 0
            {
                index.push((rd, 1 << ((x >> 6) & 0x1F)));
            }
        }
        for o in body.clone() {
            let x = word(o).unwrap_or(0);
            if !(x >> 26 == 0 && x & 0x3F == 0x21) {
                continue;
            }
            let (rs, rt, rd) = ((x >> 21) & 0x1F, (x >> 16) & 0x1F, (x >> 11) & 0x1F);
            for (ri, rb) in [(rs, rt), (rt, rs)] {
                let Some(&(_, stride)) = index.iter().find(|e| e.0 == ri) else {
                    continue;
                };
                // The base register's last writer must be a formed `addiu`.
                let mut q = o;
                let mut base = None;
                while q >= 4 && q + 0x80 > l {
                    q -= 4;
                    let y = word(q).unwrap_or(0);
                    if defines(y) == Some(rb) {
                        if y >> 26 == 0x09 {
                            base = formed.get(&q).map(|&b| (q, b));
                        }
                        break;
                    }
                }
                let Some((form_off, b)) = base else { continue };
                // The access through the element address: below the `addu`,
                // or - when the `addu` sits at the loop's bottom (often the
                // branch's delay slot) - at the top of the next iteration.
                for o2 in (o + 4..at + 8).step_by(4).chain((l..o).step_by(4)) {
                    let z = word(o2).unwrap_or(0);
                    if let Some(w) = width(z >> 26)
                        && (z >> 21) & 0x1F == rd
                    {
                        let off = (z & 0xFFFF) as i16;
                        if stride >= w && off >= 0 && (off as u32) < stride {
                            out.push(BoundedArray {
                                base: b,
                                count: n as u32,
                                stride,
                                form_site: base_va.wrapping_add(form_off as u32),
                                bound_site: base_va.wrapping_add(bound_off as u32),
                            });
                        }
                        break;
                    }
                    if defines(z) == Some(rd) {
                        break;
                    }
                }
            }
        }
    }
    out.sort_by_key(|a| (a.base, std::cmp::Reverse(a.count * a.stride)));
    out.dedup_by_key(|a| a.base);
    out
}

/// Every `(site_va, array_base_va, index_reg)` the image's own code reaches
/// as `lui rA, hi; addu rB, rA, rX; <op> rY, lo(rB)` - the indexed form
/// [`formed_addresses`] leaves out, because its target is the base of an
/// array rather than the datum the instruction touches.
pub fn indexed_addresses(image: &[u8], base_va: u32) -> Vec<(u32, u32, u32)> {
    lui_forms(image, base_va)
        .into_iter()
        .filter_map(|f| f.index.map(|i| (f.site, f.target, i)))
        .collect()
}

/// Every `(site_va, target_va, width)` where the image's own code forms an
/// address with a `lui` pair whose second instruction is a **load or store**
/// (`lb`/`lbu`/`sb` = 1, `lh`/`lhu`/`sh` = 2, `lw`/`sw` = 4), so the access
/// itself states the datum's width. `lwl`/`lwr` are left out: they read an
/// unaligned word through two instructions and pin no width alone.
pub fn accessed_addresses(image: &[u8], base_va: u32) -> Vec<(u32, u32, u32)> {
    lui_forms(image, base_va)
        .into_iter()
        .filter(|f| f.index.is_none())
        .filter_map(|f| {
            let width = match f.op {
                0x20 | 0x24 | 0x28 => 1,
                0x21 | 0x25 | 0x29 => 2,
                0x23 | 0x2B => 4,
                _ => return None,
            };
            Some((f.site, f.target, width))
        })
        .collect()
}

/// Maximal all-zero runs of at least `min` bytes, as `(start, end)`.
pub fn zero_runs(buf: &[u8], min: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < buf.len() {
        if buf[i] != 0 {
            i += 1;
            continue;
        }
        let start = i;
        while i < buf.len() && buf[i] == 0 {
            i += 1;
        }
        if i - start >= min {
            out.push((start, i));
        }
    }
    out
}

/// Claim the zero runs an overlay image's own code addresses.
///
/// An overlay is streamed by a **fixed-length** transfer: `FUN_8003EBE4` asks
/// `FUN_8003E8A8` for the entry's sector count - `toc[i+3] - toc[i+2]`, the
/// gap to the next entry - and hands it straight to `FUN_8003E800`. So the
/// whole extent reaches RAM whatever is in it, and a linked image's
/// uninitialised data region travels with its code as zero fill. Those bytes
/// are not a format nobody has walked; they are the buffers the image's own
/// code writes at runtime, and the disc's largest single unclaimed run (PROT
/// `0970`, 131172 bytes) is one.
///
/// Two rules keep this from being a way to buy percentage points, and both are
/// asserted against the raw file rather than against the parser:
///
/// * the claim is exactly one maximal **all-zero** run - it can never grow
///   into live content, and a run interrupted by a single non-zero byte is two
///   runs;
/// * the image's own code must address the run, and once is not enough. A
///   single `lui` pair landing somewhere in a multi-kilobyte window is a
///   coincidence an image with thousands of pairs will produce; two distinct
///   addresses, or one formed at [`BSS_MIN_SITES`] separate sites, is a
///   structure. A zero region below that bar stays residue, which is what keeps
///   a donor's zero tail - and a zero hole inside a sparse data segment - out of
///   the figure. Entry `0970`'s post-blob slack and its 256-byte data-segment
///   hole are refused for having no site at all; the menu overlay's largest
///   data-segment hole is refused on the bar.
///
/// The claim's `detail` reports both counts, so the reader can weigh a run
/// addressed twice against one addressed hundreds of times.
pub(super) fn claim_uninitialised_data(buf: &[u8], sink: &mut Sink, base_va: u32) {
    // An indexed access addresses its array's base as surely as a pair
    // addresses a scalar, so both count as the image reaching the run.
    let formed: Vec<(u32, u32)> = lui_forms(buf, base_va)
        .into_iter()
        .map(|f| (f.site, f.target))
        .collect();
    let mut claimed = 0usize;
    let mut runs = 0usize;
    for (start, end) in zero_runs(buf, BSS_RUN_MIN) {
        let lo = base_va.wrapping_add(start as u32);
        let hi = base_va.wrapping_add(end as u32);
        let sites = formed.iter().filter(|(_, t)| *t >= lo && *t < hi).count();
        let distinct = formed
            .iter()
            .filter(|(_, t)| *t >= lo && *t < hi)
            .map(|(_, t)| *t)
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        if distinct < 2 && sites < BSS_MIN_SITES {
            continue;
        }
        sink.claim(
            start,
            end,
            OWNER_PAD,
            format!(
                "uninitialised data region {lo:#010x}..{hi:#010x}, \
                 {distinct} address(es) formed inside it at {sites} site(s)"
            ),
        );
        claimed += end - start;
        runs += 1;
    }
    if runs > 0 {
        sink.note(format!(
            "{runs} uninitialised data region(s), {claimed} bytes: \
             zero fill the loader transfers because the read length is the \
             entry's own sector extent, addressed by this image's own code"
        ));
    }
}
