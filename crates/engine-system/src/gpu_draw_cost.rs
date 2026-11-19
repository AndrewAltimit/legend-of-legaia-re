//! What one frame's ordering table costs the PSX GPU to draw, in hblanks.
//!
//! The engine half of the GPU draw-time model `scripts/mednafen/gpu_cost.py`
//! (and the probe library's `gpu_cost.lua`) runs over a retail table: each
//! polygon is priced on the v2 GPU's measured costs as DuckStation's
//! `src/core/gpu.cpp` records them (the polygon setup table by kind,
//! `AddDrawTriangleTicks`' pixel and row terms), and each linked-list node on
//! the DMA2 costs `src/core/dma.cpp` records (a header read of `8`, a block
//! setup of `5`, a cycle a word). Every figure is in CPU cycles; one NTSC
//! scanline is `3413` video cycles, `3413 * 7 / 11` CPU cycles. Nothing is
//! fitted to a capture.
//!
//! Retail's frame-cost entry is `max(cpu, dma + draw of the previous pass's
//! table)` from one vsync (`docs/subsystems/battle-stage-camera.md`, "What a
//! real console's frame costs"). The engine's CPU term is a table read off
//! PCSX-Redux, whose GPU is free; this module prices the GPU half off the
//! frame the host drew, for the opt-in frame-step source
//! `WorldToggles::battle_gpu_cost_step`.
//!
//! The host hands it triangles, not packets: a mesh's quads arrive as the
//! libgs split `(v0, v1, v2)` + `(v1, v3, v2)`, which [`add_mesh_draw`]
//! pairs back into one quad, so a quad takes the quad setup once. Rectangles (`SPRT`), lines and fills, which the
//! Python model also prices, have no engine source here.
//!
//! A second price rides along: the same table on mednafen's own rules
//! ([`mednafen`], `GpuTiming::Mednafen`), the rules the mednafen captures
//! measure. It is what [`TableCost::total_hblanks_for`] hands over when the
//! opt-in `WorldToggles::gpu_cost_timing` asks for it; the DuckStation figure
//! stays the default.
//!
//! REF: DrawOTag (the table walk this prices)

pub mod mednafen;

/// Whose GPU timing rules price a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuTiming {
    /// DuckStation's measured v2 GPU costs, a table's draw plus its DMA
    /// (`scripts/mednafen/gpu_cost.py`'s `ot_cost`).
    #[default]
    DuckStation,
    /// Mednafen's own rules ([`mednafen`]): the drawing and the DMA in
    /// series, texture and CLUT cache misses, mednafen's line length.
    Mednafen,
}

/// One NTSC scanline in CPU cycles (the video clock is the CPU's `* 11 / 7`).
pub const HBLANK_CPU_CYCLES: f64 = 3413.0 * 7.0 / 11.0;

/// The battle ordering table's length (`0x1F8003A6`, `4096` in every
/// battle state): every bucket is a list node the DMA walks, empty or not.
pub const BATTLE_OT_LEN: u32 = 4096;

/// The field ordering table's length (`0x1F8003A6`, `2048` in every field
/// state).
pub const FIELD_OT_LEN: u32 = 2048;

/// DMA2 linked-list costs: a node's header read, a non-empty block's setup.
const LL_HEADER: u64 = 8;
const LL_BLOCK: u64 = 5;

/// What kind of polygon packet a primitive is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PolyKind {
    /// A four-corner packet (`*4`).
    pub quad: bool,
    /// Gouraud (`G*`): one colour a corner.
    pub shaded: bool,
    /// Textured (`*T*`).
    pub textured: bool,
    /// Semi-transparent (command bit 1).
    pub semi: bool,
}

impl PolyKind {
    /// The setup cost DuckStation records for the packet kind
    /// (`gpu.cpp`, the polygon setup table).
    pub const fn setup_cycles(self) -> u64 {
        match (self.quad, self.shaded, self.textured) {
            (false, false, false) => 50,
            (false, false, true) => 130,
            (false, true, false) => 200,
            (false, true, true) => 295,
            (true, false, false) => 50,
            (true, false, true) => 215,
            (true, true, false) => 335,
            (true, true, true) => 495,
        }
    }

    /// The packet's GP0 words: the command and colour, then per corner its
    /// position, a UV word if textured and a colour word if shaded (the
    /// first corner's colour rides the command word).
    pub const fn words(self) -> u64 {
        let corners = if self.quad { 4 } else { 3 };
        let per = 1 + self.textured as u64 + self.shaded as u64;
        1 + corners * per - self.shaded as u64
    }
}

/// The drawing area the battle pass draws in: `[x0, y0, x1, y1]`, inclusive.
pub const DISPLAY_AREA: [i32; 4] = [0, 0, 319, 239];

/// One triangle's draw cycles (`AddDrawTriangleTicks`): its pixels after
/// the corners are clamped to the drawing area, a cycle a shaded or
/// textured pixel and half one for a flat one (`3/4` semi-transparent), plus
/// a per-row term.
pub fn tri_cycles(v: [[i32; 2]; 3], kind: PolyKind) -> u64 {
    let [x0, y0, x1, y1] = DISPLAY_AREA;
    let c = v.map(|[x, y]| [x.clamp(x0, x1 + 1), y.clamp(y0, y1 + 1)]);
    let twice = i64::from(c[0][0]) * i64::from(c[1][1])
        + i64::from(c[1][0]) * i64::from(c[2][1])
        + i64::from(c[2][0]) * i64::from(c[0][1])
        - i64::from(c[0][0]) * i64::from(c[2][1])
        - i64::from(c[1][0]) * i64::from(c[0][1])
        - i64::from(c[2][0]) * i64::from(c[1][1]);
    let pix = (twice.unsigned_abs()) / 2;
    let ys = [c[0][1], c[1][1], c[2][1]];
    let rows = (ys.iter().max().unwrap_or(&0) - ys.iter().min().unwrap_or(&0)).unsigned_abs();
    let flat = !kind.shaded && !kind.textured;
    let q: u64 = if flat {
        if kind.semi { 3 } else { 2 }
    } else {
        4
    };
    let rt: u64 = 2 + if kind.semi {
        if flat { 3 } else { 1 }
    } else {
        0
    };
    (pix * q).div_ceil(4) + u64::from(rows) * rt
}

/// Whether the GPU drops the polygon unrasterised: a span of `1024` or more
/// across or `512` or more down (DuckStation's `MAX_PRIMITIVE_WIDTH` /
/// `MAX_PRIMITIVE_HEIGHT`), which a corner projected from behind the eye
/// gives.
fn oversized(v: &[[i32; 2]]) -> bool {
    let (mut lo, mut hi) = ([i32::MAX; 2], [i32::MIN; 2]);
    for p in v {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    hi[0] - lo[0] >= 1024 || hi[1] - lo[1] >= 512
}

/// A table's running cost: draw and DMA cycles and the polygons priced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TableCost {
    pub draw: u64,
    pub dma: u64,
    pub polys: u32,
    /// The same table on mednafen's rules.
    pub med: mednafen::MedTable,
}

/// A textured packet's page and palette words: its TSB (texture page and
/// colour mode as the GPU's `E1` word lays them out) and CBA (CLUT).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TexWords {
    pub tsb: u16,
    pub cba: u16,
}

impl TableCost {
    /// An empty table of `ot_len` buckets: each is a node the DMA reads.
    pub fn new(ot_len: u32) -> Self {
        Self {
            draw: 0,
            dma: u64::from(ot_len) * LL_HEADER,
            polys: 0,
            med: mednafen::MedTable::new(ot_len),
        }
    }

    /// Mednafen's price of a packet's first triangle: its node, setup,
    /// palette load and raster.
    fn med_first(&mut self, c: [mednafen::Corner; 3], kind: PolyKind, tex: Option<TexWords>) {
        let mut units = mednafen::setup_units(kind, false);
        let cache = match tex {
            Some(t) if kind.textured => {
                self.med.cache.set_page(t.tsb);
                units += self.med.cache.load_clut(t.cba);
                Some(&mut self.med.cache)
            }
            _ => None,
        };
        units += mednafen::triangle_units(c, kind, DISPLAY_AREA, cache);
        self.med.open_packet(kind.words(), units);
    }

    /// Mednafen's price of a quad's second triangle.
    fn med_second(&mut self, c: [mednafen::Corner; 3], kind: PolyKind, tex: Option<TexWords>) {
        let cache = (kind.textured && tex.is_some()).then_some(&mut self.med.cache);
        let units = mednafen::setup_units(kind, true)
            + mednafen::triangle_units(c, kind, DISPLAY_AREA, cache);
        self.med.add_to_packet(units);
    }

    /// The figure `timing` gives for the GPU's whole wait, in hblanks,
    /// rounded, as the frame-cost ring stores a duration.
    pub fn total_hblanks_for(&self, timing: GpuTiming) -> u16 {
        let h = match timing {
            GpuTiming::DuckStation => {
                let (d, m) = self.hblanks();
                d + m
            }
            GpuTiming::Mednafen => self.med.hblanks().0,
        };
        h.round().clamp(0.0, f64::from(u16::MAX)) as u16
    }

    /// One packet's node: its header, block setup and words.
    fn add_node(&mut self, kind: PolyKind) {
        self.dma += LL_HEADER + LL_BLOCK + kind.words();
        self.polys += 1;
    }

    /// One three-corner packet.
    pub fn add_tri(&mut self, v: [[i32; 2]; 3], kind: PolyKind) {
        let kind = PolyKind {
            quad: false,
            ..kind
        };
        self.add_tri_tex(v.map(|p| (p, [0, 0])), kind, None);
    }

    /// One three-corner packet with its corners' UVs and, when textured,
    /// its page words (the mednafen price walks the texture cache with
    /// them; the DuckStation price reads positions only).
    pub fn add_tri_tex(&mut self, c: [mednafen::Corner; 3], kind: PolyKind, tex: Option<TexWords>) {
        let kind = PolyKind {
            quad: false,
            ..kind
        };
        // Mednafen sets an oversized triangle up and drops its raster
        // (`DrawTriangle`); DuckStation drops the packet.
        self.med_first(c, kind, tex);
        let v = c.map(|(p, _)| p);
        if oversized(&v) {
            return;
        }
        self.add_node(kind);
        self.draw += kind.setup_cycles() + tri_cycles(v, kind);
    }

    /// One four-corner packet, `v` in the GPU's corner order: drawn as
    /// `(v0, v1, v2)` and `(v1, v2, v3)` under one setup.
    pub fn add_quad(&mut self, v: [[i32; 2]; 4], kind: PolyKind) {
        let kind = PolyKind { quad: true, ..kind };
        let c = v.map(|p| (p, [0, 0]));
        self.med_first([c[0], c[1], c[2]], kind, None);
        self.med_second([c[1], c[2], c[3]], kind, None);
        if oversized(&v) {
            return;
        }
        self.add_node(kind);
        self.draw += kind.setup_cycles()
            + tri_cycles([v[0], v[1], v[2]], kind)
            + tri_cycles([v[1], v[2], v[3]], kind);
    }

    /// The second triangle of a quad already added by its first
    /// ([`Self::add_quad_first`]): its pixels only.
    pub fn add_quad_second(&mut self, v: [[i32; 2]; 3], kind: PolyKind) {
        self.add_quad_second_tex(v.map(|p| (p, [0, 0])), kind, None);
    }

    /// [`Self::add_quad_second`] with UVs and page words.
    pub fn add_quad_second_tex(
        &mut self,
        c: [mednafen::Corner; 3],
        kind: PolyKind,
        tex: Option<TexWords>,
    ) {
        let kind = PolyKind { quad: true, ..kind };
        self.med_second(c, kind, tex);
        let v = c.map(|(p, _)| p);
        if oversized(&v) {
            return;
        }
        self.draw += tri_cycles(v, kind);
    }

    /// The first triangle of a quad whose second arrives on its own
    /// ([`Self::add_quad_second`]): the quad's node and setup and this
    /// triangle's pixels.
    pub fn add_quad_first(&mut self, v: [[i32; 2]; 3], kind: PolyKind) {
        self.add_quad_first_tex(v.map(|p| (p, [0, 0])), kind, None);
    }

    /// [`Self::add_quad_first`] with UVs and page words.
    pub fn add_quad_first_tex(
        &mut self,
        c: [mednafen::Corner; 3],
        kind: PolyKind,
        tex: Option<TexWords>,
    ) {
        let kind = PolyKind { quad: true, ..kind };
        self.med_first(c, kind, tex);
        let v = c.map(|(p, _)| p);
        if oversized(&v) {
            return;
        }
        self.add_node(kind);
        self.draw += kind.setup_cycles() + tri_cycles(v, kind);
    }

    /// `(draw, dma)` in hblanks.
    pub fn hblanks(&self) -> (f64, f64) {
        (
            self.draw as f64 / HBLANK_CPU_CYCLES,
            self.dma as f64 / HBLANK_CPU_CYCLES,
        )
    }

    /// The GPU's whole wait in hblanks, `draw + dma`, rounded, as the
    /// frame-cost ring stores a duration.
    pub fn total_hblanks(&self) -> u16 {
        let (d, m) = self.hblanks();
        (d + m).round().clamp(0.0, f64::from(u16::MAX)) as u16
    }
}

/// Which faces of a draw reach the table: the renderer's per-draw NCLIP
/// word (`Renderer::set_backface_cull`'s encoding, its low two bits).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Faces {
    /// Both windings (`0`).
    Both,
    /// Counter-clockwise in NDC, the pipelines' front face (`1`).
    Ccw,
    /// Clockwise in NDC (`2`).
    Cw,
}

impl Faces {
    /// The faces a renderer NCLIP word keeps.
    pub const fn of_word(word: u32) -> Self {
        match word & 3 {
            1 => Self::Ccw,
            2 => Self::Cw,
            _ => Self::Both,
        }
    }
}

/// One mesh draw's streams, as the renderer uploaded them: corner positions,
/// per-corner words (`[cba, tsb]` on a textured mesh, `[0, blend]` on an
/// untextured one; the semi-transparency enable is bit 15 of the second
/// either way, the Gouraud tag bit 14 of a TSB), corner colours and the
/// triangle list.
#[derive(Debug, Clone, Copy)]
pub struct MeshStreams<'a> {
    pub positions: &'a [[f32; 3]],
    pub words: &'a [[u16; 2]],
    pub colors: &'a [[u8; 3]],
    pub indices: &'a [u32],
    pub textured: bool,
    /// Corner UVs, for the mednafen price's texture cache; empty when the
    /// host kept none (that price then leaves the cache misses out).
    pub uvs: &'a [[u8; 2]],
}

/// The Gouraud tag of a TSB word (`legaia_tmd::mesh::TSB_GOURAUD_BIT`).
const TSB_GOURAUD: u16 = 0x4000;
/// The semi-transparency enable of a TSB / blend word.
const WORD_SEMI: u16 = 0x8000;

/// Add every triangle of one draw - its streams, its column-major
/// view-projection onto the `320 x 240` display and the faces it keeps - to
/// `cost`. A mesh's quads arrive split as `(v0, v1, v2)` + `(v1, v3, v2)`
/// (`legaia_tmd::mesh`); such a pair with one page word links as one quad
/// packet. A triangle with a corner on or behind the eye (`w <= 0`) is not
/// linked, as retail's near reject drops it before the table; one whose
/// winding the draw culls is not linked either (GTE `NCLIP`).
pub fn add_mesh_draw(cost: &mut TableCost, mesh: MeshStreams<'_>, m: &[f32; 16], faces: Faces) {
    let tris = mesh.indices.as_chunks::<3>().0;
    let project = |i: u32| -> Option<([i32; 2], [f32; 2])> {
        let p = *mesh.positions.get(i as usize)?;
        let x = m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12];
        let y = m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13];
        let w = m[3] * p[0] + m[7] * p[1] + m[11] * p[2] + m[15];
        if w <= 1e-6 {
            return None;
        }
        let n = [x / w, y / w];
        let sx = (n[0] * 0.5 + 0.5) * 320.0;
        let sy = (0.5 - n[1] * 0.5) * 240.0;
        Some((
            [
                sx.round().clamp(-4096.0, 4096.0) as i32,
                sy.round().clamp(-4096.0, 4096.0) as i32,
            ],
            n,
        ))
    };
    let mut k = 0;
    while k < tris.len() {
        let t = tris[k];
        let word = mesh.words.get(t[0] as usize).copied().unwrap_or([0, 0]);
        let next = tris.get(k + 1).copied();
        let paired = next.is_some_and(|b| {
            b[0] == t[1]
                && b[2] == t[2]
                && b[1] != t[0]
                && mesh.words.get(b[1] as usize).copied() == Some(word)
        });
        let corners = |t: [u32; 3]| -> Option<[([i32; 2], [f32; 2]); 3]> {
            Some([project(t[0])?, project(t[1])?, project(t[2])?])
        };
        let kept = |c: &[([i32; 2], [f32; 2]); 3]| {
            let (a, b, d) = (c[0].1, c[1].1, c[2].1);
            let cross = (b[0] - a[0]) * (d[1] - a[1]) - (d[0] - a[0]) * (b[1] - a[1]);
            match faces {
                Faces::Both => true,
                Faces::Ccw => cross > 0.0,
                Faces::Cw => cross < 0.0,
            }
        };
        let colour = |i: u32| mesh.colors.get(i as usize).copied();
        let kind = PolyKind {
            quad: false,
            shaded: (mesh.textured && word[1] & TSB_GOURAUD != 0)
                || colour(t[0]) != colour(t[1])
                || colour(t[0]) != colour(t[2]),
            textured: mesh.textured,
            semi: word[1] & WORD_SEMI != 0,
        };
        let uv = |i: u32| mesh.uvs.get(i as usize).copied().unwrap_or([0, 0]);
        let screen = |c: [([i32; 2], [f32; 2]); 3], t: [u32; 3]| {
            [(c[0].0, uv(t[0])), (c[1].0, uv(t[1])), (c[2].0, uv(t[2]))]
        };
        let tex = (mesh.textured && !mesh.uvs.is_empty()).then_some(TexWords {
            tsb: word[1] & !(TSB_GOURAUD | WORD_SEMI),
            cba: word[0],
        });
        let first = corners(t).filter(kept);
        if paired && let Some(b) = next {
            let second = corners(b).filter(kept);
            match (first, second) {
                (Some(f), Some(s)) => {
                    cost.add_quad_first_tex(screen(f, t), kind, tex);
                    cost.add_quad_second_tex(screen(s, b), kind, tex);
                }
                (Some(c), None) => cost.add_tri_tex(screen(c, t), kind, tex),
                (None, Some(c)) => cost.add_tri_tex(screen(c, b), kind, tex),
                (None, None) => {}
            }
            k += 2;
        } else {
            if let Some(f) = first {
                cost.add_tri_tex(screen(f, t), kind, tex);
            }
            k += 1;
        }
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

    /// Figures `scripts/mednafen/gpu_cost.py`'s `tri_ticks` and `SETUP`
    /// give for the same corners.
    #[test]
    fn triangle_matches_the_python_model() {
        // tri_ticks(Area(), (10,10),(110,10),(10,60), True, True, False) = 2600
        assert_eq!(tri_cycles([[10, 10], [110, 10], [10, 60]], GT), 2600);
        // Flat semi: pix 2500, q 3 -> 1875, rows 50 * 5 = 250.
        let f = PolyKind {
            semi: true,
            ..Default::default()
        };
        assert_eq!(tri_cycles([[10, 10], [110, 10], [10, 60]], f), 2125);
        // Clamped to the area: a corner far off the right edge.
        assert_eq!(
            tri_cycles([[300, 0], [900, 0], [300, 10]], GT),
            tri_cycles([[300, 0], [320, 0], [300, 10]], GT)
        );
        assert_eq!(GT.setup_cycles(), 295);
        assert_eq!(PolyKind { quad: true, ..GT }.setup_cycles(), 495);
    }

    #[test]
    fn packet_words_by_kind() {
        assert_eq!(GT.words(), 9);
        assert_eq!(PolyKind { quad: true, ..GT }.words(), 12);
        assert_eq!(PolyKind::default().words(), 4);
        let ft4 = PolyKind {
            quad: true,
            textured: true,
            ..Default::default()
        };
        assert_eq!(ft4.words(), 9);
    }

    /// A quad split in two halves costs what the whole quad does.
    #[test]
    fn a_split_quad_costs_the_quad() {
        let v = [[0, 0], [40, 0], [0, 30], [40, 30]];
        let mut whole = TableCost::new(0);
        whole.add_quad(v, GT);
        let mut split = TableCost::new(0);
        split.add_quad_first([v[0], v[1], v[2]], GT);
        split.add_quad_second([v[1], v[3], v[2]], GT);
        assert_eq!(whole, split);
        assert_eq!(whole.polys, 1);
    }

    /// A mesh quad, split as the TMD mesh builder splits it, links as one
    /// quad packet; the winding the draw culls links nothing.
    #[test]
    fn a_mesh_quad_links_once() {
        let mut ident = [0.0f32; 16];
        ident[0] = 1.0;
        ident[5] = 1.0;
        ident[10] = 1.0;
        ident[15] = 1.0;
        // NDC corners: v0 top-left, v1 top-right, v2 bottom-left, v3 bottom-right.
        let positions = [
            [-0.5, 0.5, 0.5],
            [0.5, 0.5, 0.5],
            [-0.5, -0.5, 0.5],
            [0.5, -0.5, 0.5],
        ];
        let words = [[0x7EC1, 0x4000]; 4];
        let colors = [[0x80; 3]; 4];
        let mesh = MeshStreams {
            positions: &positions,
            words: &words,
            colors: &colors,
            indices: &[0, 1, 2, 1, 3, 2],
            textured: true,
            uvs: &[],
        };
        let mut both = TableCost::new(0);
        add_mesh_draw(&mut both, mesh, &ident, Faces::Both);
        let mut whole = TableCost::new(0);
        whole.add_quad([[80, 60], [240, 60], [80, 180], [240, 180]], GT);
        assert_eq!(both, whole);
        // (v0, v1, v2) runs clockwise in NDC.
        let mut ccw = TableCost::new(0);
        add_mesh_draw(&mut ccw, mesh, &ident, Faces::Ccw);
        assert_eq!(ccw, TableCost::new(0));
        let mut cw = TableCost::new(0);
        add_mesh_draw(&mut cw, mesh, &ident, Faces::Cw);
        assert_eq!(cw, whole);
    }

    /// The DuckStation figure is the default and what `total_hblanks`
    /// gives; the mednafen one rides along and differs.
    #[test]
    fn timing_picks_the_figure() {
        let mut t = TableCost::new(BATTLE_OT_LEN);
        t.add_quad([[80, 60], [240, 60], [80, 180], [240, 180]], GT);
        assert_eq!(GpuTiming::default(), GpuTiming::DuckStation);
        assert_eq!(
            t.total_hblanks_for(GpuTiming::DuckStation),
            t.total_hblanks()
        );
        // Mednafen: 4095 empty buckets at 10 cycles, the quad's packet at
        // (2 + 532 + 496 + 2 * 160 * 120) / 2 cycles, over a 2152.6-cycle line.
        let med = t.total_hblanks_for(GpuTiming::Mednafen);
        let want =
            (4095.0 * 10.0 + (2.0 + 532.0 + 496.0 + 38_400.0) / 2.0) / mednafen::HBLANK_CPU_CYCLES;
        assert!((f64::from(med) - want).abs() <= 1.0, "{med} vs {want}");
        assert_eq!(t.med.packets(), 1);
    }

    /// With UVs the mednafen price walks the texture cache: a 64 x 48-pixel
    /// quad over a 64 x 64 4-bit texture samples 48 texel rows of four
    /// sixteen-texel blocks each, a miss apiece.
    #[test]
    fn uvs_add_texture_misses() {
        let mut ident = [0.0f32; 16];
        ident[0] = 1.0;
        ident[5] = 1.0;
        ident[10] = 1.0;
        ident[15] = 1.0;
        let positions = [
            [-0.2, 0.2, 0.5],
            [0.2, 0.2, 0.5],
            [-0.2, -0.2, 0.5],
            [0.2, -0.2, 0.5],
        ];
        let words = [[0x7EC1, 0x4005]; 4];
        let colors = [[0x80; 3]; 4];
        let uvs = [[0, 0], [63, 0], [0, 63], [63, 63]];
        let mesh = |uvs| MeshStreams {
            positions: &positions,
            words: &words,
            colors: &colors,
            indices: &[0, 1, 2, 1, 3, 2],
            textured: true,
            uvs,
        };
        let mut bare = TableCost::new(0);
        add_mesh_draw(&mut bare, mesh(&[]), &ident, Faces::Both);
        let mut with = TableCost::new(0);
        add_mesh_draw(&mut with, mesh(&uvs), &ident, Faces::Both);
        assert_eq!(bare.draw, with.draw, "the DuckStation price reads no UVs");
        let extra = with.med.cycles() - bare.med.cycles();
        // 48 * 4 misses at 2 cycles, plus the 8-cycle palette load.
        assert_eq!(extra, 48.0 * 4.0 * 2.0 + 8.0);
    }

    /// The empty battle table alone is the DMA walk of its buckets, about
    /// fifteen hblanks.
    #[test]
    fn the_empty_table_is_its_bucket_walk() {
        let t = TableCost::new(BATTLE_OT_LEN);
        assert_eq!(t.total_hblanks(), 15);
        let mut t = t;
        t.add_tri([[0, 0], [2000, 0], [0, 10]], GT);
        assert_eq!(t.polys, 0, "an oversized polygon is dropped");
    }
}
