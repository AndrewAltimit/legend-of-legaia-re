//! What a table costs to draw on Mednafen's own GPU and DMA timing rules.
//!
//! Mednafen keeps a GPU time budget that grows two units a CPU cycle and
//! saturates at `256`; each GP0 command spends it, and a command waits in the
//! FIFO while it is negative. The linked-list DMA feeds the FIFO only while
//! the GPU can take a word, so a packet's drawing and its DMA run in series,
//! not side by side: a packet costs the longer of its DMA (a node header of
//! `15` cycles, a cycle a word) and its drawing. An empty bucket costs its
//! header, `10` cycles. Every figure here is in budget units (half CPU
//! cycles). The rules, with the source file and function each lives in, are
//! in `docs/tooling/emulator-timing.md`; `scripts/mednafen/gpu_cost.py`
//! (`med_ot_cost`) runs the same rules over a retail table and matches the
//! ring entry mednafen records on GPU-bound passes.
//!
//! What the engine cannot hand over: retail's own bucket order (the texture
//! cache below runs in draw order) and the empty buckets' interleaving (an
//! empty run lets the budget refill; the per-packet `max` ignores that, which
//! overprices a table by a few hblanks).
//!
//! REF: DrawOTag (the table walk this prices)

use super::PolyKind;

/// One NTSC line in CPU cycles on mednafen's clocks: `3412.5` GPU clocks of
/// `53.693182` MHz against a CPU of `44100 * 768` Hz.
pub const HBLANK_CPU_CYCLES: f64 = 3412.5 * 44_100.0 * 768.0 / 53_693_181.818;

/// The budget's ceiling: an idle GPU banks at most this much ahead.
pub const BUDGET_CAP: u64 = 256;
/// A non-empty list node's DMA header read, in CPU cycles.
pub const NODE_HEADER_CYCLES: u64 = 15;
/// An empty list node's header read, in CPU cycles.
pub const EMPTY_NODE_CYCLES: u64 = 10;
/// What every drawing command spends before its own work (the clip and
/// offset words spend nothing).
pub const COMMAND_UNITS: u64 = 2;

/// A triangle's fixed setup: `64 + 18` for a packet's first, `28 + 18` for
/// a quad's second half, plus the interpolants it walks.
pub const fn setup_units(kind: PolyKind, second_half: bool) -> u64 {
    let base = if second_half { 28 + 18 } else { 64 + 18 };
    let ext = match (kind.shaded, kind.textured) {
        (true, true) => 150 * 3,
        (true, false) => 96 * 3,
        (false, true) => 60 * 3,
        (false, false) => 0,
    };
    base + ext
}

/// One span's units: two a shaded or textured pixel, one and a half a flat
/// blended one, one a flat one.
pub const fn span_units(w: u64, kind: PolyKind) -> u64 {
    if kind.shaded || kind.textured {
        w * 2
    } else if kind.semi {
        w + w.div_ceil(2)
    } else {
        w
    }
}

/// The GPU's texture cache and CLUT cache as the rasteriser walks them: `256`
/// entries of a four-halfword VRAM block each, a miss costing `4` units; a
/// palette load costing `16` (4-bit) or `256` (8-bit) units when the CLUT
/// word changes. A texture-page change that moves the page or switches
/// between palette and direct colour empties the texture cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TexCache {
    tags: [u32; 256],
    page_x: u32,
    page_y: u32,
    mode: u32,
    clut: Option<u32>,
}

impl Default for TexCache {
    fn default() -> Self {
        Self {
            tags: [u32::MAX; 256],
            page_x: 0,
            page_y: 0,
            mode: 0,
            clut: None,
        }
    }
}

impl TexCache {
    /// Set the texture page from a TSB / `E1` word (page in bits `0..=4`,
    /// colour mode in bits `7..=8`).
    pub fn set_page(&mut self, tsb: u16) {
        let tsb = u32::from(tsb);
        let (x, y, m) = ((tsb & 0xF) * 64, (tsb & 0x10) * 16, (tsb >> 7) & 3);
        if (m == 0) != (self.mode == 0) || x != self.page_x || y != self.page_y {
            self.tags = [u32::MAX; 256];
        }
        (self.page_x, self.page_y, self.mode) = (x, y, m);
    }

    /// The palette load a polygon's CLUT word costs.
    pub fn load_clut(&mut self, cba: u16) -> u64 {
        if self.mode >= 2 {
            return 0;
        }
        let key = u32::from(cba & 0x7FFF) | (self.mode << 16);
        if self.clut == Some(key) {
            return 0;
        }
        self.clut = Some(key);
        if self.mode == 1 { 256 } else { 16 }
    }

    /// One texel fetch: `4` units on a miss.
    pub fn fetch(&mut self, u: u32, v: u32) -> u64 {
        let m = self.mode.min(2);
        let x = (((u & 0xFF) + (self.page_x << (2 - m))) >> (2 - m)) & 1023;
        let y = ((v & 0xFF) + self.page_y) & 511;
        let gro = y * 1024 + x;
        let i = if m == 0 {
            ((gro >> 2) & 0x3) | ((gro >> 8) & 0xFC)
        } else {
            ((gro >> 2) & 0x7) | ((gro >> 7) & 0xF8)
        } as usize;
        let tag = gro & !3;
        if self.tags[i] == tag {
            0
        } else {
            self.tags[i] = tag;
            4
        }
    }
}

/// A triangle corner: screen position and, when textured, its UV.
pub type Corner = ([i32; 2], [u8; 2]);

/// The drawing area: `[x0, y0, x1, y1]`, inclusive.
pub type Clip = [i32; 4];

/// One triangle's raster units, after its setup: the rows inside `clip`
/// cost their spans, a row the walk passes outside it two units, and with
/// `tex` each textured pixel its texel fetch. The triangle is walked as
/// mednafen's `DrawTriangle` walks it: sorted by Y, split at the middle
/// corner, each half from the side its core corner says, a walk stopping at
/// the clip edge it heads for. Mednafen drops a triangle `512` or more high
/// or `1024` or more wide, and a degenerate one, unrasterised.
pub fn triangle_units(
    mut c: [Corner; 3],
    kind: PolyKind,
    clip: Clip,
    mut tex: Option<&mut TexCache>,
) -> u64 {
    let x = |c: &[Corner; 3], i: usize| c[i].0[0];
    let mut cv: u32 = if x(&c, 1) <= x(&c, 0) {
        if x(&c, 2) <= x(&c, 1) { 4 } else { 2 }
    } else if x(&c, 2) < x(&c, 0) {
        4
    } else {
        1
    };
    let swap12 = |cv: u32| ((cv >> 1) & 2) | ((cv << 1) & 4) | (cv & 1);
    let swap01 = |cv: u32| ((cv >> 1) & 1) | ((cv << 1) & 2) | (cv & 4);
    if c[2].0[1] < c[1].0[1] {
        c.swap(1, 2);
        cv = swap12(cv);
    }
    if c[1].0[1] < c[0].0[1] {
        c.swap(0, 1);
        cv = swap01(cv);
    }
    if c[2].0[1] < c[1].0[1] {
        c.swap(1, 2);
        cv = swap12(cv);
    }
    let core = (cv >> 1) as usize;
    let [x0, y0] = c[0].0;
    let [x1, y1] = c[1].0;
    let [x2, y2] = c[2].0;
    if y0 == y2
        || y2 - y0 >= 512
        || (x2 - x0).abs() >= 1024
        || (x2 - x1).abs() >= 1024
        || (x1 - x0).abs() >= 1024
    {
        return 0;
    }
    let den = f64::from((x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0));
    if den == 0.0 {
        return 0;
    }
    // The UV plane, for the texel walk.
    let uvf = |i: usize, k: usize| f64::from(c[i].1[k]);
    let grad = |k: usize| {
        let (a, b) = (uvf(1, k) - uvf(0, k), uvf(2, k) - uvf(0, k));
        (
            (a * f64::from(y2 - y0) - b * f64::from(y1 - y0)) / den,
            (b * f64::from(x1 - x0) - a * f64::from(x2 - x0)) / den,
        )
    };
    let ((dudx, dudy), (dvdx, dvdy)) = (grad(0), grad(1));
    let (cx, cy) = (f64::from(c[core].0[0]), f64::from(c[core].0[1]));
    let (cu, cvv) = (uvf(core, 0), uvf(core, 1));
    let edge = |xa: i32, ya: i32, xb: i32, yb: i32, y: i32| {
        f64::from(xa) + f64::from(xb - xa) * f64::from(y - ya) / f64::from(yb - ya)
    };
    let [cx0, cy0, cx1, cy1] = clip;
    let mut units = 0u64;
    for (ya, yb, dec) in [(y0, y1, core != 0), (y1, y2, core == 2)] {
        if ya == yb {
            continue;
        }
        let rows: Box<dyn Iterator<Item = i32>> = if dec {
            Box::new((ya..yb).rev())
        } else {
            Box::new(ya..yb)
        };
        for y in rows {
            let (stop, pass) = if dec {
                (y < cy0, y > cy1)
            } else {
                (y > cy1, y < cy0)
            };
            if stop {
                break;
            }
            if pass {
                units += 2;
                continue;
            }
            let xl = edge(x0, y0, x2, y2, y);
            let xr = if y < y1 {
                edge(x0, y0, x1, y1, y)
            } else {
                edge(x1, y1, x2, y2, y)
            };
            let (a, b) = if xl <= xr { (xl, xr) } else { (xr, xl) };
            let xs = ((a - 1e-9).ceil() as i32).max(cx0);
            let xe = ((b - 1e-9).ceil() as i32).min(cx1 + 1);
            if xe <= xs {
                continue;
            }
            let w = (xe - xs) as u64;
            units += span_units(w, kind);
            if kind.textured
                && let Some(t) = tex.as_deref_mut()
            {
                let (dx, dy) = (f64::from(xs) - cx, f64::from(y) - cy);
                let bu = cu + dudx * dx + dudy * dy + 0.5;
                let bv = cvv + dvdx * dx + dvdy * dy + 0.5;
                for k in 0..w {
                    let k = k as f64;
                    let u = (bu + dudx * k).max(0.0) as u32;
                    let v = (bv + dvdx * k).max(0.0) as u32;
                    units += t.fetch(u, v);
                }
            }
        }
    }
    units
}

/// A table's running Mednafen price: closed packets, the open one (a quad's
/// second half joins its first), the DMA's share and the packets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MedTable {
    /// Closed packets' cost, in units.
    closed: u64,
    /// The open packet's DMA cycles and drawing units.
    open: Option<(u64, u64)>,
    /// DMA cycles of every non-empty node.
    dma: u64,
    /// Packets placed.
    packets: u32,
    /// The table's bucket count (empty nodes are what the packets leave).
    ot_len: u32,
    pub cache: TexCache,
}

impl MedTable {
    /// An empty table of `ot_len` buckets.
    pub fn new(ot_len: u32) -> Self {
        Self {
            ot_len,
            ..Self::default()
        }
    }

    fn close(&mut self) {
        if let Some((dma, draw)) = self.open.take() {
            self.closed += (2 * dma).max(draw);
        }
    }

    /// Open a packet node of `words` GP0 words whose first command spends
    /// `units`.
    pub fn open_packet(&mut self, words: u64, units: u64) {
        self.close();
        let dma = NODE_HEADER_CYCLES + words;
        self.dma += dma;
        self.packets += 1;
        self.open = Some((dma, COMMAND_UNITS + units));
    }

    /// More drawing in the open packet (a quad's second triangle).
    pub fn add_to_packet(&mut self, units: u64) {
        if let Some((_, draw)) = self.open.as_mut() {
            *draw += units;
        }
    }

    /// The whole wait in CPU cycles: every packet, then every empty bucket.
    pub fn cycles(&self) -> f64 {
        let mut t = *self;
        t.close();
        let empty = u64::from(self.ot_len.saturating_sub(self.packets));
        (t.closed as f64) / 2.0 + (empty * EMPTY_NODE_CYCLES) as f64
    }

    /// `(total, dma)` in hblanks on mednafen's line length. The total is
    /// the whole wait, the DMA inside it.
    pub fn hblanks(&self) -> (f64, f64) {
        let empty = u64::from(self.ot_len.saturating_sub(self.packets));
        (
            self.cycles() / HBLANK_CPU_CYCLES,
            (self.dma + empty * EMPTY_NODE_CYCLES) as f64 / HBLANK_CPU_CYCLES,
        )
    }

    /// Packets placed.
    pub fn packets(&self) -> u32 {
        self.packets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GT: PolyKind = PolyKind {
        quad: false,
        shaded: true,
        textured: true,
        semi: false,
    };
    const CLIP: Clip = [0, 0, 319, 239];

    fn tri(v: [[i32; 2]; 3]) -> [Corner; 3] {
        v.map(|p| (p, [0, 0]))
    }

    /// The figures `scripts/mednafen/gpu_cost.py`'s `med_triangle` gives
    /// for the same corners: a right triangle 100 wide, 50 high covers
    /// `2550` pixels on mednafen's walk (rows of `100, 98, .., 2`).
    #[test]
    fn triangle_matches_the_python_model() {
        let c = tri([[10, 10], [110, 10], [10, 60]]);
        assert_eq!(triangle_units(c, GT, CLIP, None), 5100);
        let flat = PolyKind::default();
        assert_eq!(triangle_units(c, flat, CLIP, None), 2550);
        let semi = PolyKind {
            semi: true,
            ..Default::default()
        };
        assert_eq!(triangle_units(c, semi, CLIP, None), 3825);
        // Partly above the area: twenty rows walked at two units each.
        let above = tri([[0, -20], [40, -20], [0, 20]]);
        assert_eq!(triangle_units(above, GT, CLIP, None), 460);
    }

    /// Rows above the drawing area cost two units each; rows below end the
    /// walk; columns outside are clipped.
    #[test]
    fn clipping() {
        let inside = triangle_units(tri([[0, 0], [40, 0], [0, 40]]), GT, CLIP, None);
        let above = triangle_units(tri([[0, -20], [40, -20], [0, 20]]), GT, CLIP, None);
        let lower = triangle_units(tri([[0, 0], [40, 0], [0, 40]]), GT, [0, 20, 319, 239], None);
        assert!(above < inside);
        assert_eq!(above % 2, 0);
        assert!(lower < inside);
        let wide = triangle_units(tri([[300, 0], [900, 0], [300, 10]]), GT, CLIP, None);
        let cut = triangle_units(tri([[300, 0], [320, 0], [300, 10]]), GT, CLIP, None);
        assert!(wide >= cut);
        assert_eq!(
            triangle_units(tri([[0, 0], [2000, 0], [0, 10]]), GT, CLIP, None),
            0
        );
        assert_eq!(
            triangle_units(tri([[0, 0], [10, 0], [20, 0]]), GT, CLIP, None),
            0
        );
    }

    #[test]
    fn setup_by_kind() {
        assert_eq!(setup_units(GT, false), 532);
        assert_eq!(setup_units(GT, true), 496);
        assert_eq!(setup_units(PolyKind::default(), false), 82);
        let ft = PolyKind {
            textured: true,
            ..Default::default()
        };
        assert_eq!(setup_units(ft, false), 262);
    }

    /// A 4-bit page caches sixteen texels a block: a 64-texel row costs
    /// four misses, and walking it again costs none.
    #[test]
    fn texture_cache_misses() {
        let mut t = TexCache::default();
        t.set_page(0x0005);
        assert_eq!(t.load_clut(0x7E00), 16);
        assert_eq!(t.load_clut(0x7E00), 0);
        let first: u64 = (0..64).map(|u| t.fetch(u, 0)).sum();
        assert_eq!(first, 4 * 4);
        let again: u64 = (0..64).map(|u| t.fetch(u, 0)).sum();
        assert_eq!(again, 0);
        t.set_page(0x0006);
        assert_eq!(t.fetch(0, 0), 4, "a new page empties the cache");
        t.set_page(0x0086);
        assert_eq!(t.load_clut(0x7E00), 256, "an 8-bit palette is 256 entries");
    }

    /// A packet costs the longer of its DMA and its drawing; empty buckets
    /// their header.
    #[test]
    fn packets_run_in_series() {
        let mut m = MedTable::new(10);
        m.open_packet(9, 1000);
        assert_eq!(m.cycles(), 501.0 + 9.0 * 10.0);
        let mut m = MedTable::new(1);
        m.open_packet(9, 10);
        assert_eq!(m.cycles(), 24.0);
        m.add_to_packet(1000);
        assert_eq!(m.cycles(), 506.0);
        assert_eq!(MedTable::new(4096).hblanks().0.round(), 19.0);
    }
}
