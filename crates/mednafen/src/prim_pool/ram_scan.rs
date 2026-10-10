//! Whole-RAM scans: locate primitive pools and ordering-table arrays in a
//! main-RAM image without knowing their addresses.

use super::*;

/// One contiguous run of primitive packets found by [`find_pools`].
#[derive(Debug, Clone, Serialize)]
pub struct PoolRegion {
    /// KSEG0 address of the first accepted packet's tag word.
    pub start: u32,
    /// KSEG0 address one past the last accepted packet's payload.
    pub end: u32,
    /// How many packets were accepted inside the run.
    pub prims: usize,
}

impl PoolRegion {
    pub fn len(&self) -> usize {
        (self.end - self.start) as usize
    }
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// Locate the frame's primitive pool(s) inside a whole main-RAM image.
///
/// [`POOL_BASE_DEFAULT`] is a world-map constant, so anything that wants the
/// display list of an arbitrary scene has to *find* the pool rather than assume
/// it. libgpu builds each frame's packets contiguously in one work buffer, so
/// the pool shows up as a dense run of packets whose chain tags point at each
/// other. Two extra constraints keep the whole-RAM scan from drowning in false
/// positives (a scan bounded only by "somewhere in 2 MiB" accepts far too much):
///
/// - a tag's `next_addr` must be the terminator **or** land within `NEAR_WINDOW`
///   bytes of the tag itself - real OT links are short-range because the pool is
///   one buffer, whereas a random word that happens to look like a tag points
///   anywhere;
/// - a run must hold at least `min_prims` accepted packets to be reported.
///
/// Runs are separated when the gap between consecutive accepted packets exceeds
/// `MAX_GAP` bytes.
pub fn find_pools(ram: &[u8], ram_base: u32, min_prims: usize) -> Vec<PoolRegion> {
    /// How far a legitimate chain link may reach. The PSX work buffer for one
    /// frame is a few hundred KB at most.
    const NEAR_WINDOW: u32 = 512 * 1024;
    /// Bytes of non-primitive slack tolerated inside one run.
    const MAX_GAP: usize = 4096;

    let mut accepted: Vec<(usize, usize)> = Vec::new(); // (offset, total_bytes)
    let n_words = ram.len() / 4;
    let mut w = 0usize;
    while w < n_words {
        let i = w * 4;
        if i + 8 > ram.len() {
            break;
        }
        let tag = read_u32(ram, i);
        let length = ((tag >> 24) & 0xFF) as usize;
        let next_addr = tag & 0x00FF_FFFF;
        if !(1..=12).contains(&length) || i + 4 + length * 4 > ram.len() {
            w += 1;
            continue;
        }
        let here = (ram_base + i as u32) & 0x00FF_FFFF;
        let near = next_addr == 0x00FF_FFFF || here.abs_diff(next_addr) <= NEAR_WINDOW;
        if !near {
            w += 1;
            continue;
        }
        let cmd = ((read_u32(ram, i + 4) >> 24) & 0xFF) as u8;
        let (_, prim) = decode_packet(ram, i, cmd, length);
        if prim.is_some() {
            accepted.push((i, 4 + length * 4));
            w += length + 1;
        } else {
            w += 1;
        }
    }

    let mut out: Vec<PoolRegion> = Vec::new();
    let mut run_start: Option<usize> = None;
    let mut run_end = 0usize;
    let mut run_count = 0usize;
    for (off, size) in accepted {
        match run_start {
            Some(_) if off.saturating_sub(run_end) <= MAX_GAP => {
                run_end = off + size;
                run_count += 1;
            }
            Some(s) => {
                if run_count >= min_prims {
                    out.push(PoolRegion {
                        start: ram_base + s as u32,
                        end: ram_base + run_end as u32,
                        prims: run_count,
                    });
                }
                run_start = Some(off);
                run_end = off + size;
                run_count = 1;
            }
            None => {
                run_start = Some(off);
                run_end = off + size;
                run_count = 1;
            }
        }
    }
    if let Some(s) = run_start.filter(|_| run_count >= min_prims) {
        out.push(PoolRegion {
            start: ram_base + s as u32,
            end: ram_base + run_end as u32,
            prims: run_count,
        });
    }
    out.sort_by_key(|r| std::cmp::Reverse(r.prims));
    out
}

/// A located libgpu ordering table: the bucket array itself, not the packets.
#[derive(Debug, Clone, Serialize)]
pub struct OtArray {
    /// KSEG0 address of the lowest bucket (`ot[0]`).
    pub start: u32,
    /// KSEG0 address one past the highest bucket.
    pub end: u32,
    /// Number of `u32` buckets.
    pub buckets: usize,
    /// KSEG0 address of the entry `DrawOTag` is handed - the chain head. With
    /// `ClearOTagR` this is the **highest** bucket, because the reverse-cleared
    /// table threads `ot[N-1] -> ot[N-2] -> ... -> ot[0] -> terminator`.
    pub head: u32,
}

/// Find the frame's ordering-table array(s) in a main-RAM image.
///
/// The packet pool and the ordering table are different objects, and the chain
/// head lives in the **table**, not the pool - which is why walking a pool in
/// isolation reports hundreds of spurious "heads" (every packet whose
/// predecessor link sits outside the window looks like a head). To get real
/// draw order the walk has to start at the table.
///
/// `ClearOTagR` leaves a recognisable signature: an empty bucket at address `A`
/// holds `0x00` in its length byte and `A - 4` in its 24-bit next field, so a
/// cleared table is a run of words each pointing at its own predecessor. Buckets
/// that received primitives break the pattern by pointing into the pool instead,
/// so the run is detected on the empty ones and then extended across the
/// occupied ones.
pub fn find_ot_arrays(ram: &[u8], ram_base: u32, min_buckets: usize) -> Vec<OtArray> {
    let n_words = ram.len() / 4;
    let is_empty_bucket = |w: usize| -> bool {
        let i = w * 4;
        if i + 4 > ram.len() {
            return false;
        }
        let word = read_u32(ram, i);
        if (word >> 24) != 0 {
            return false;
        }
        let here = (ram_base + i as u32) & 0x00FF_FFFF;
        (word & 0x00FF_FFFF) == here.wrapping_sub(4) & 0x00FF_FFFF
    };
    // A bucket that received prims points somewhere else entirely; accept it as
    // part of the table so an occupied bucket does not split one array in two.
    //
    // The lower bound matters more than it looks. Accepting *any* word with a
    // zero length byte lets the run extend one word past the real table into
    // whatever data follows - and since the head is taken as the top of the
    // run, a single over-extension puts the head on a non-bucket and the whole
    // walk yields nothing. (Observed: a word `0x00002020` directly above a
    // 2048-bucket table, which silently cost 1600 packets.) A real link points
    // at a libgpu work buffer, which never lives in the first 64 KiB - that is
    // BIOS and kernel space.
    const MIN_LINK: u32 = 0x1_0000;
    let is_occupied_bucket = |w: usize| -> bool {
        let i = w * 4;
        if i + 4 > ram.len() {
            return false;
        }
        let word = read_u32(ram, i);
        if (word >> 24) != 0 {
            return false;
        }
        let next = word & 0x00FF_FFFF;
        next >= MIN_LINK
    };

    let mut out = Vec::new();
    let mut w = 0usize;
    while w < n_words {
        if !is_empty_bucket(w) {
            w += 1;
            continue;
        }
        // Walk back over any occupied buckets that precede this empty one.
        let mut lo = w;
        while lo > 0 && (is_empty_bucket(lo - 1) || is_occupied_bucket(lo - 1)) {
            lo -= 1;
        }
        // Walk forward likewise.
        let mut hi = w;
        while hi + 1 < n_words && (is_empty_bucket(hi + 1) || is_occupied_bucket(hi + 1)) {
            hi += 1;
        }
        let buckets = hi - lo + 1;
        if buckets >= min_buckets {
            out.push(OtArray {
                start: ram_base + (lo * 4) as u32,
                end: ram_base + ((hi + 1) * 4) as u32,
                buckets,
                head: ram_base + (hi * 4) as u32,
            });
        }
        w = hi + 1;
    }
    out.sort_by_key(|o| std::cmp::Reverse(o.buckets));
    out
}
