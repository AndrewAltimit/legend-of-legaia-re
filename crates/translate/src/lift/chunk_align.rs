//! NUL-chunk framing and alignment of a source string pool against the USA pool
//! it replaces.

use super::super::{markup, segments};
use std::collections::BTreeMap;

/// Every `0x1F <text> 0x00` framing in `buf`, in order, with no quality gate:
/// `(text offset, text bytes)`.
pub(super) fn framings(buf: &[u8]) -> Vec<(usize, Vec<u8>)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < buf.len() {
        if buf[i] == 0x1F
            && let Some(t) = segments::walk_to_terminator(buf, i + 1)
            && buf[t] == 0x00
            && t > i + 1
        {
            out.push((i + 1, buf[i + 1..t].to_vec()));
            i = t + 1;
            continue;
        }
        i += 1;
    }
    out
}

/// NUL-to-NUL chunks of `buf[lo..hi)`: `(offset, bytes)` of every non-empty
/// run, junk included - the raw material the pool pairing anchors on.
pub(super) fn nul_chunks(buf: &[u8], lo: usize, hi: usize) -> Vec<(usize, Vec<u8>)> {
    let hi = hi.min(buf.len());
    let mut out = Vec::new();
    let mut pos = lo;
    while pos < hi {
        if buf[pos] == 0 {
            pos += 1;
            continue;
        }
        let mut e = pos;
        while e < hi && buf[e] != 0 {
            e += 1;
        }
        // A run the window's end cuts short is a fragment, not a string: it
        // can pair with nothing (and as a source it would lift a truncated
        // line), and a fragment's length disagrees with the whole string on
        // the other build, failing every alignment that reaches it.
        if e == hi && buf.get(e).is_some_and(|&b| b != 0) {
            break;
        }
        // A string pool's first string can trail the code that addresses it
        // with no NUL between: key the chunk on its first glyph, not on the
        // control bytes ahead of it, so the two builds' chunks compare.
        let start = pos + buf[pos..e].iter().take_while(|&&b| b < 0x20).count();
        // A run that is control bytes only is kept whole: the build-invariant
        // byte runs between two strings are what anchor the pairing.
        let start = if start < e { start } else { pos };
        out.push((start, buf[start..e].to_vec()));
        pos = e;
    }
    out
}

/// The stride of a fixed-stride string table: the one spacing every
/// consecutive pair of `offsets` shares, over at least three strings. `None`
/// for a pool laid out by length.
pub(super) fn table_stride(offsets: &[usize]) -> Option<usize> {
    if offsets.len() < 3 {
        return None;
    }
    let d = offsets[1].checked_sub(offsets[0])?;
    (d > 0
        && offsets
            .windows(2)
            .all(|w| w[1].checked_sub(w[0]) == Some(d)))
    .then_some(d)
}

/// Cut every chunk at least `stride` bytes long into `stride`-byte pieces.
///
/// In a fixed-stride table a string that fills its slot carries no NUL: the
/// game reads the next slot at `+stride` regardless, so the next string
/// follows with nothing between and a NUL scan reads the two as one chunk.
/// (The German fatal-decision table's recover line fills its `0x28`-byte
/// slot and runs straight into `MP Null`.) Cutting at the slot edge gives
/// back the strings the game's own indexing reads.
pub(super) fn split_at_stride(chunks: Vec<Chunk>, stride: usize) -> Vec<Chunk> {
    let mut out = Vec::with_capacity(chunks.len());
    for (off, b) in chunks {
        if b.len() < stride || !pool_text(&b) {
            out.push((off, b));
            continue;
        }
        for (k, piece) in b.chunks(stride).enumerate() {
            out.push((off + k * stride, piece.to_vec()));
        }
    }
    out
}

/// Pair two NUL-chunk lists of the *same* string pool on two builds:
/// `usa offset -> source bytes`.
///
/// Identical chunks that occur once on each side (a stat label, a proper
/// noun, a run of pointer bytes the two builds share) are **anchors**; they
/// partition both lists, and inside a partition the chunks pair by ordinal
/// when the two sides count the same. Where they do not - the source build
/// carries an extra string, or a window edge cut a run - the partition
/// retries on the prose-shaped chunks only (`segments::qualifies_ext`), and
/// an edge partition aligns on its anchor side. Nothing pairs across a
/// disagreement: an unpaired label stays vanilla on import, which is the
/// safe failure.
pub(super) fn pair_chunk_lists(
    usa: &[(usize, Vec<u8>)],
    src: &[(usize, Vec<u8>)],
) -> BTreeMap<usize, Vec<u8>> {
    use std::collections::HashMap;
    fn count(list: &[(usize, Vec<u8>)]) -> HashMap<&[u8], usize> {
        let mut m: HashMap<&[u8], usize> = HashMap::new();
        for (_, b) in list {
            *m.entry(b.as_slice()).or_default() += 1;
        }
        m
    }
    let (cu, cs) = (count(usa), count(src));
    let (class_u, class_s) = (cut_classes(usa), cut_classes(src));
    let cls_u = |c: &Chunk| class_u.get(&c.0).copied().unwrap_or(Cut::Whole);
    let cls_s = |c: &Chunk| class_s.get(&c.0).copied().unwrap_or(Cut::Whole);
    let src_index: HashMap<&[u8], usize> = src
        .iter()
        .enumerate()
        .map(|(i, (_, b))| (b.as_slice(), i))
        .collect();
    // Anchors, monotonic in both lists.
    let mut anchors: Vec<(usize, usize)> = Vec::new();
    let mut last = 0usize;
    for (i, (_, b)) in usa.iter().enumerate() {
        if cu.get(b.as_slice()) == Some(&1)
            && cs.get(b.as_slice()) == Some(&1)
            && cls_u(&usa[i]) == Cut::Whole
            && let Some(&j) = src_index.get(b.as_slice())
            && cls_s(&src[j]) == Cut::Whole
            && (anchors.is_empty() || j > last)
        {
            anchors.push((i, j));
            last = j;
        }
    }
    let mut out = BTreeMap::new();
    let mut bounds = vec![(0usize, 0usize)];
    bounds.extend(anchors.iter().map(|&(i, j)| (i + 1, j + 1)));
    let ends: Vec<(usize, usize)> = anchors
        .iter()
        .map(|&(i, j)| (i, j))
        .chain(std::iter::once((usa.len(), src.len())))
        .collect();
    for (p, (&(ui, sj), &(ue, se))) in bounds.iter().zip(&ends).enumerate() {
        let (u, s) = (&usa[ui..ue], &src[sj..se]);
        if u.len() == s.len() {
            for (a, b) in u.iter().zip(s) {
                out.insert(a.0, b.1.clone());
            }
            continue;
        }
        // The source window is widened past the pool on purpose (its strings
        // run longer), so a partition's surplus is run-in before or run-out
        // after the pool. A partition anchored on one side aligns on that
        // side; a window with no anchor tries the head first. Either way the
        // aligned pairs must agree pairwise in shape (prose against prose,
        // pointer bytes against pointer bytes, comparable length), and the
        // same is retried over the prose-shaped chunks only, so a run of
        // one-byte junk on one side cannot displace a line.
        let head_first = p + 1 == bounds.len();
        let prose = |c: &(usize, Vec<u8>)| pool_text(&c.1);
        let up: Vec<&(usize, Vec<u8>)> = u.iter().filter(|c| prose(c)).collect();
        let sp: Vec<&(usize, Vec<u8>)> = s.iter().filter(|c| prose(c)).collect();
        let u_all: Vec<&(usize, Vec<u8>)> = u.iter().collect();
        let s_all: Vec<&(usize, Vec<u8>)> = s.iter().collect();
        // The source window is the widened one, so its surplus is expected;
        // a surplus on the USA side means the gate ate source strings, and
        // aligning what survived would put a different line on a slot -
        // the one failure an unpaired label never is.
        let pairs = align_chunks_dp_classed(&u_all, &s_all, &cls_u, &cls_s)
            .or_else(|| {
                (s.len() >= u.len())
                    .then(|| align_chunks(&u_all, &s_all, head_first, &cls_u, &cls_s))
                    .flatten()
            })
            .or_else(|| {
                (sp.len() >= up.len())
                    .then(|| align_chunks(&up, &sp, head_first, &cls_u, &cls_s))
                    .flatten()
            });
        if let Some(pairs) = pairs {
            for (a, b) in pairs {
                out.insert(a.0, b.1.clone());
            }
        }
    }
    for &(i, j) in &anchors {
        out.insert(usa[i].0, src[j].1.clone());
    }
    out
}

/// Where a NUL-to-NUL chunk sits relative to a two-byte control token whose
/// argument byte is `0x00` (the `{c1:00}` name token): the NUL scan cuts
/// such a string in two, and the halves are fragments, not strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Cut {
    /// A string the scan read whole.
    Whole,
    /// The half ending in a token whose `0x00` argument ended the chunk.
    Head,
    /// The half that follows a [`Cut::Head`] with no gap.
    Tail,
}

/// `true` when `b` ends in a two-byte control token's lead byte: its
/// argument was the `0x00` the chunk ended at.
fn ends_in_cut_token(b: &[u8]) -> bool {
    let mut i = 0;
    while i < b.len() {
        if markup::is_two_byte_op(b[i]) {
            if i + 1 >= b.len() {
                return true;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    false
}

/// The [`Cut`] class of every chunk that is not [`Cut::Whole`], by offset.
/// Two builds put a name token in different places (`@{c1:00} will equip`
/// against `@Equiper {c1:00}`), so a fragment pairs only with a fragment of
/// the same class and never anchors a partition: a two-byte `@{c1}` head can
/// be unique on one build and match an unrelated line's head on the other.
pub(super) fn cut_classes(list: &[Chunk]) -> std::collections::HashMap<usize, Cut> {
    let mut out = std::collections::HashMap::new();
    for (k, (off, b)) in list.iter().enumerate() {
        if ends_in_cut_token(b) {
            out.insert(*off, Cut::Head);
            if let Some((next, _)) = list.get(k + 1)
                && *next == off + b.len() + 1
            {
                out.entry(*next).or_insert(Cut::Tail);
            }
        }
    }
    out
}

/// Whether a pool chunk is a string rather than pointer / code bytes: every
/// glyph (control tokens stepped over) printable or an accent tile, with at
/// least two letters. The dialog gate is too strict for UI strings - it
/// refuses the `/` of an abbreviation (`p/ equipar`) and a punctuated word
/// with no space (`{ce:13}Pernas.`) - and one refused string on one build
/// fails every alignment of the pool it sits in.
pub(super) fn pool_text(b: &[u8]) -> bool {
    let (mut i, mut letters) = (0usize, 0usize);
    while i < b.len() {
        let c = b[i];
        if markup::is_two_byte_op(c) {
            i += 2;
            continue;
        }
        if c < 0x20 {
            return false;
        }
        if c == 0x7F || (0xB0..=0xB4).contains(&c) || (0xDB..=0xDF).contains(&c) {
            // The CP850 shade / box / block tiles: a string never mixes
            // them with prose, pointer bytes land on them all the time.
            return false;
        }
        if c.is_ascii_alphabetic() || c >= 0x80 {
            letters += 1;
        }
        i += 1;
    }
    letters >= 2
}

/// Two chunks of the same pool slot on two builds look alike: both prose or
/// both not, and when prose, within a length ratio a translation stays inside.
fn chunks_agree(a: &[u8], b: &[u8]) -> bool {
    let (pa, pb) = (pool_text(a), pool_text(b));
    if pa != pb {
        return false;
    }
    if !pa {
        return true;
    }
    let (la, lb) = (a.len().max(1), b.len().max(1));
    la * 2 <= lb * 5 && lb * 2 <= la * 5
}

/// A NUL-delimited chunk of a string pool: `(offset, bytes)`.
pub(super) type Chunk = (usize, Vec<u8>);

/// Align two chunk lists of unequal length by dynamic programming, in order:
/// the source may carry extra chunks anywhere (a build that splits one
/// string in two, or adds a label variant, puts its surplus in the middle of
/// the pool, where a head or tail alignment slides every later pair by one),
/// and a USA chunk may stay unpaired at a higher cost (the widened source
/// window can still end before the USA one does). A pair must
/// [`chunks_agree`]; an identical pair scores highest, then a pair whose
/// leading glyph class agrees (the `@` of a menu label) and whose lengths are
/// closest. `None` when no USA prose chunk pairs at all.
#[cfg(test)]
pub(super) fn align_chunks_dp<'a>(
    u: &[&'a Chunk],
    s: &[&'a Chunk],
) -> Option<Vec<(&'a Chunk, &'a Chunk)>> {
    align_chunks_dp_classed(u, s, &|_| Cut::Whole, &|_| Cut::Whole)
}

/// Whether a USA chunk of class `cu` can pair with a source chunk of class
/// `cs`: only within one class, and a fragment only when it carries prose of
/// its own - a bare `@{c1}` head is the same two bytes on every line that
/// opens with a name, so it identifies nothing.
fn fragments_pair(cu: Cut, a: &Chunk, cs: Cut) -> bool {
    cu == cs && (cu == Cut::Whole || pool_text(&a.1))
}

/// [`align_chunks_dp`] with each side's [`Cut`] classes ([`fragments_pair`]).
///
/// A source chunk skipped after the last pair costs less than one skipped
/// before or between pairs: the source window is widened past the pool's
/// end, so run-out there is expected, while a surplus ahead of or inside the
/// pool is a build's extra string. Without the difference every
/// order-preserving alignment of comparable lengths scores alike, and one
/// that slides the pool's tail by a string (`@Price` against the next label)
/// can win the tie.
pub(super) fn align_chunks_dp_classed<'a>(
    u: &[&'a Chunk],
    s: &[&'a Chunk],
    cls_u: &dyn Fn(&Chunk) -> Cut,
    cls_s: &dyn Fn(&Chunk) -> Cut,
) -> Option<Vec<(&'a Chunk, &'a Chunk)>> {
    let (n, m) = (u.len(), s.len());
    if n == 0 || m == 0 {
        return None;
    }
    let pair = |a: &Chunk, b: &Chunk| -> Option<i64> {
        if !fragments_pair(cls_u(a), a, cls_s(b)) {
            return None;
        }
        if a.1 == b.1 {
            return Some(60);
        }
        if !chunks_agree(&a.1, &b.1) {
            return None;
        }
        let lead = |c: &Chunk| c.1.first().map(|&x| (x == b'@', x.is_ascii_alphanumeric()));
        let (la, lb) = (a.1.len() as i64, b.1.len() as i64);
        // A translation runs about a fifth longer; score the distance from
        // that, in fifths of the USA length.
        let dev = ((lb * 5 - la * 6).abs() * 4 / (la * 5).max(1)).min(12);
        Some(24 + if lead(a) == lead(b) { 12 } else { 0 } - dev)
    };
    const SKIP_SRC_END: i64 = 2;
    const SKIP_SRC_MID: i64 = 12;
    const SKIP_USA: i64 = 20;
    let skip_src = |i: usize| {
        if i == n { SKIP_SRC_END } else { SKIP_SRC_MID }
    };
    // dp[i][j]: best score over u[..i] and s[..j].
    let mut dp = vec![vec![0i64; m + 1]; n + 1];
    for (j, cell) in dp[0].iter_mut().enumerate() {
        *cell = -(j as i64) * SKIP_SRC_MID;
    }
    for i in 1..=n {
        dp[i][0] = -(i as i64) * SKIP_USA;
        for j in 1..=m {
            let mut best = (dp[i][j - 1] - skip_src(i)).max(dp[i - 1][j] - SKIP_USA);
            if let Some(sc) = pair(u[i - 1], s[j - 1]) {
                best = best.max(dp[i - 1][j - 1] + sc);
            }
            dp[i][j] = best;
        }
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (n, m);
    while i > 0 && j > 0 {
        let here = dp[i][j];
        if pair(u[i - 1], s[j - 1]).is_some_and(|sc| dp[i - 1][j - 1] + sc == here) {
            out.push((u[i - 1], s[j - 1]));
            i -= 1;
            j -= 1;
        } else if dp[i][j - 1] - skip_src(i) == here {
            j -= 1;
        } else {
            i -= 1;
        }
    }
    out.reverse();
    (!out.is_empty()).then_some(out)
}

/// Align two chunk lists of unequal length by their head or their tail,
/// whichever the caller prefers, accepting an alignment only when every
/// pair [`chunks_agree`]s. `None` when neither does.
fn align_chunks<'a>(
    u: &[&'a Chunk],
    s: &[&'a Chunk],
    head_first: bool,
    cls_u: &dyn Fn(&Chunk) -> Cut,
    cls_s: &dyn Fn(&Chunk) -> Cut,
) -> Option<Vec<(&'a Chunk, &'a Chunk)>> {
    let n = u.len().min(s.len());
    if n == 0 {
        return None;
    }
    let head: Vec<_> = u[..n].iter().zip(&s[..n]).map(|(a, b)| (*a, *b)).collect();
    let tail: Vec<_> = u[u.len() - n..]
        .iter()
        .zip(&s[s.len() - n..])
        .map(|(a, b)| (*a, *b))
        .collect();
    let order = if head_first {
        [head, tail]
    } else {
        [tail, head]
    };
    order.into_iter().find(|pairs| {
        pairs
            .iter()
            .all(|(a, b)| fragments_pair(cls_u(a), a, cls_s(b)) && chunks_agree(&a.1, &b.1))
    })
}
