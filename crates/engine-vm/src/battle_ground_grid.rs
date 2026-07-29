//! The battle backdrop's procedural ground grid.
//!
//! PORT: FUN_801D02C0
//!
//! The grass (or sand, or stone) under the combatants is not geometry from a
//! file - it is a flat tiled plane the battle overlay rasterises every frame,
//! and it is the sole draw call the mode-`0x15` render path makes for the
//! floor. See [`docs/subsystems/battle.md`](../../../docs/subsystems/battle.md)
//! for how it sits in the backdrop (grid + stage-TMD dome) and
//! [`docs/reference/functions/battle.md`](../../../docs/reference/functions/battle.md)
//! for the entry.
//!
//! Everything here is transcribed from the DISASSEMBLY in
//! `ghidra/scripts/funcs/overlay_battle_action_801d02c0.txt`. That dump's C is
//! unusable for this routine on its own - it renders the GTE `mtc2`/`cop2`/
//! `swc2` traffic as `setCopReg`/`getCopReg` with raw immediates, carries an
//! `Instruction at 0x801d06ec overlaps 0x801d06e8` warning where a branch
//! delay slot doubles as a jump target, and drops which shifted scratchpad
//! slot each store lands in.
//!
//! ## Shape
//!
//! A `width x height` cell grid on a flat plane, cell pitch `0x200`, centred on
//! the world origin, drawn in two passes:
//!
//! * **Pass 1** projects one probe point per cell (`RTPS`) and writes a
//!   three-valued visibility byte per cell into the `0x1000`-byte buffer at
//!   `_DAT_8007B814` - hence the ~64x64 ceiling on the grid.
//! * **Pass 2** walks the same cells, and for each *visible* one projects a
//!   **3x3 lattice** of its corners and midpoints (`RTPT`, three rows of three)
//!   and emits **four** `POLY_GT4` quads - the cell subdivided 2x2 at the
//!   `0x100` sub-step.
//!
//! ## Y is the sign bit of X, not a height
//!
//! Every vertex load is `mtc2 <x_word>, VXY<n>`, so the GTE's `VY` half takes
//! the **upper 16 bits of the same word** - which for a sign-extended `lh` of a
//! world X is `0x0000` when X is non-negative and `0xFFFF` (= -1) when it is.
//! The plane is therefore flat at `Y == 0` for the `X >= 0` half and `Y == -1`
//! for the other, which is the exact sense in which the grid is "Y ~ 0". There
//! is no height field and no per-cell Y at all.
//!
//! ## Sub-tiles
//!
//! The four sub-quads take the four `0x20 x 0x20` corners of the `(192..255)^2`
//! window in order, so a single 64x64 texture spans one whole `0x200` cell -
//! `tile = sub_row * 2 + sub_col`, u rising with the column and v with the row.
//! Nothing about the choice is random and no cell picks a single sub-tile.
//!
//! ## Depth cue
//!
//! The emitter runs **`DPCS`** (`cop2 0x780010`) once per projected lattice
//! vertex - four sites, `0x801d061c` / `0x801d063c` / `0x801d0654` /
//! `0x801d0688` - and immediately before each it loads `IR0` by hand:
//! `srl` of the vertex's `SZ` by 2, then `mtc2 .., IR0` (`0x801d0608..14`).
//! So the grid's blend factor is [`grid_ir0_raw`]`= SZ >> 2` in the GTE's
//! `0x1000 = 1.0` scale, **not** the `RTPS`-computed `IR0` the `DQA`/`DQB`
//! pair would produce - and `mtc2` does not saturate, so past `SZ = 0x4000`
//! the blend extrapolates beyond the far colour until the DPCS *output*
//! clamp catches it.
//!
//! The far colour (`RFC`/`GFC`/`BFC`, control regs 21-23) is **not written
//! here** - the function contains zero `ctc2` - so the grid draws with the
//! battle backdrop's staged far colour: the word at `0x8007BB48`, times 16
//! into the 28.4 control registers. Capture-pinned by exec breakpoints on
//! the emitter entry + the first DPCS site
//! (`scripts/pcsx-redux/autorun_grid_far_colour.lua`) across three battles:
//! the settled value is `(0x40, 0x40, 0x40)` on ordinary stages and
//! `(0xFE, 0xFE, 0xFE)` on the wide-open outdoor stages named by the SCUS
//! table at `DAT_80078C1C` - exactly [`grid_far_colour`] of the neutral
//! base `0x808080` through the two `FUN_80050120` derivation arms (`>> 1`
//! indoor at `0x800507fc`, `(c - 0x010101) * 2` outdoor at `0x80050834`).
//! The battle-intro fade ramps the staged word up from near-black over the
//! first ~28 frames before it settles. The field never runs the emitter
//! (zero entry hits on a field state - the negative control).
//!
//! # Wiring
//!
//! The drawn mesh is the shared builder `legaia_asset::battle_backdrop::
//! build_ground_grid` (re-exported by `engine-shell`'s `play-window` as
//! `build_battle_ground_grid` and drawn under the battle camera). This
//! module carries the emitter's *laws* the hosts consume: the play-window
//! battle draw fogs the grid with [`grid_cue_far_z`] / [`grid_cue_max_ir0`]
//! and the [`grid_far_colour`] resolved through [`OutdoorCueTable`]. The
//! visibility culls ([`classify_cell`] / [`quad_on_screen`]) stay reference
//! kernels: under a depth-buffered projection they are visually neutral,
//! and applying them to the once-uploaded mesh would wrongly freeze the
//! entry-pose cull (see `build_ground_grid`'s doc). The GTE half
//! (`RTPS`/`RTPT` and the ordering-table link) stays outside this crate:
//! `engine-vm` holds no projection matrix.

/// World-space pitch of one grid cell (`0x200`).
pub const CELL_PITCH: i32 = 0x200;
/// World-space pitch of one sub-quad - the cell is subdivided 2x2 (`0x100`).
pub const SUB_STEP: i32 = 0x100;
/// Near bias added to the probe's projected depth before the sign test.
pub const NEAR_BIAS: i32 = 0x200;
/// Far cutoff on the biased depth: at or past this the cell is dropped.
pub const FAR_LIMIT: i32 = 0x6700;
/// Screen width the four-corner reject tests against (`0x140`).
pub const SCREEN_W: i32 = 0x140;
/// Screen height the four-corner reject tests against (`0xF0`).
pub const SCREEN_H: i32 = 0xF0;
/// CLUT base address word (`CBA`) the quads carry - CLUT at framebuffer
/// `(0, 479)`, i.e. `(479 << 6) | (0 >> 4)`.
pub const CLUT_ATTR: u16 = 0x77C0;
/// Texture-page attribute word the quads carry (4bpp page at fb `(832, 0)`).
pub const TPAGE_ATTR: u16 = 0x000D;
/// Low corner of the shared `64 x 64` UV window: `(192, 192)`.
pub const UV_WINDOW_ORIGIN: u8 = 0xC0;
/// GPU command byte the emit loop stamps while the quads are being built
/// (`0x3C`), restored to `0x2C` on the way out.
pub const CODE_WHILE_EMITTING: u8 = 0x3C;
/// GPU command byte the routine leaves behind (`0x2C`).
pub const CODE_AFTER_EMITTING: u8 = 0x2C;

/// Per-cell visibility class pass 1 writes into `_DAT_8007B814`.
///
/// Retail stores `-1` / `0` / `1`. Pass 2 emits for `Visible` only, so the
/// distinction between the two rejects is not consumed inside this routine -
/// it is preserved here because the buffer outlives the call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellVis {
    /// Behind the near plane (`depth + 0x200 <= 0`) - stored as `-1`.
    Behind,
    /// Past the far cutoff (`depth + 0x200 > 0x6700`) - stored as `0`.
    Far,
    /// In range - stored as `1`, the only class pass 2 draws.
    Visible,
}

impl CellVis {
    /// The signed byte retail writes for this class.
    pub const fn as_byte(self) -> i8 {
        match self {
            CellVis::Behind => -1,
            CellVis::Far => 0,
            CellVis::Visible => 1,
        }
    }
}

/// Pass 1's depth classifier.
///
/// `ir3` is the GTE `IR3` register after `RTPS` - the probe point's view-space
/// depth. Retail adds `NEAR_BIAS` first and then subtracts `FAR_LIMIT` from the
/// *biased* value inside a branch delay slot that both paths reach, so both
/// comparisons run against `ir3 + 0x200`.
pub fn classify_cell(ir3: i32) -> CellVis {
    let depth = ir3.wrapping_add(NEAR_BIAS);
    if depth <= 0 {
        CellVis::Behind
    } else if depth.wrapping_sub(FAR_LIMIT) > 0 {
        CellVis::Far
    } else {
        CellVis::Visible
    }
}

/// The grid's world-space minimum corner, as pass 1 computes it from the cell
/// counts at `_DAT_1F8003F8` / `_DAT_1F8003FA`.
///
/// Note the asymmetry, which is in the bytes and not a transcription slip: X is
/// `-(w/2) * 0x200` and Z is that minus a **whole extra cell**
/// (`addi t1, t1, -0x200`), so the plane is centred in X and biased one cell
/// toward the camera in Z.
pub fn grid_origin(width: i16, height: i16) -> (i32, i32) {
    let x_min = -(((width >> 1) as i32) << 9);
    let z_min = (-(((height >> 1) as i32) << 9)) - CELL_PITCH;
    (x_min, z_min)
}

/// Pass 1's probe point for cell `(col, row)` - the cell's centre in X and its
/// near edge plus a half-step in Z.
pub fn probe_point(width: i16, height: i16, col: i32, row: i32) -> (i32, i32) {
    let (x_min, z_min) = grid_origin(width, height);
    (
        x_min + SUB_STEP + col * CELL_PITCH,
        z_min + SUB_STEP + row * CELL_PITCH,
    )
}

/// The world-space `3 x 3` lattice pass 2 projects for cell `(col, row)`,
/// indexed `[z_index][x_index]` with both indices stepping by [`SUB_STEP`].
///
/// Retail feeds this to `RTPT` three vertices at a time (one lattice row per
/// `cop2 0x280030`), storing `SXY0`/`SZ1`/`SXY1`/`SZ2`/`SXY2`/`SZ3` per row.
pub fn cell_lattice(width: i16, height: i16, col: i32, row: i32) -> [[(i32, i32); 3]; 3] {
    let (x_min, z_min) = grid_origin(width, height);
    let x0 = x_min + col * CELL_PITCH;
    let z0 = z_min + row * CELL_PITCH;
    let mut out = [[(0, 0); 3]; 3];
    for (zi, lattice_row) in out.iter_mut().enumerate() {
        for (xi, slot) in lattice_row.iter_mut().enumerate() {
            *slot = (x0 + xi as i32 * SUB_STEP, z0 + zi as i32 * SUB_STEP);
        }
    }
    out
}

/// The GTE `VY` half a world X ends up supplying, given that retail packs the
/// X word straight into `VXY<n>` and never writes a separate Y.
pub fn implied_y(world_x: i32) -> i16 {
    ((world_x as u32) >> 16) as u16 as i16
}

/// Pass 2's four-corner screen reject, run on the outer corners of the
/// projected lattice - `[ (0,0), (0,2), (2,0), (2,2) ]` in `(x, y)` pairs.
///
/// Retail runs it as four "is any corner past this edge" scans, testing all
/// four Y values against the top edge, then the bottom, then all four X values
/// against the left edge, then the right; the conjunction is a screen-space AABB
/// overlap with the `0x140 x 0xF0` viewport under strict inequalities.
pub fn quad_on_screen(corners: [(i16, i16); 4]) -> bool {
    let any_y_below_top = corners.iter().any(|c| c.1 as i32 > 0);
    let any_y_above_bottom = corners.iter().any(|c| (c.1 as i32) - SCREEN_H < 0);
    let any_x_right_of_left = corners.iter().any(|c| c.0 as i32 > 0);
    let any_x_left_of_right = corners.iter().any(|c| (c.0 as i32) - SCREEN_W < 0);
    any_y_below_top && any_y_above_bottom && any_x_right_of_left && any_x_left_of_right
}

/// The four UV corners of sub-tile `index` (`0..4`), in the quad's vertex order
/// `uv0, uv1, uv2, uv3` = `(lo,lo), (hi,lo), (lo,hi), (hi,hi)`.
///
/// The tiles form a 2x2 layout inside the `(192..255)^2` window at a `0x20`
/// pitch: `index & 1` selects the U half, `index >> 1` the V half.
pub fn sub_tile_uv(index: usize) -> [(u8, u8); 4] {
    let u_lo = UV_WINDOW_ORIGIN + ((index as u8 & 1) << 5);
    let v_lo = UV_WINDOW_ORIGIN + ((index as u8 >> 1) << 5);
    let u_hi = u_lo + 0x1F;
    let v_hi = v_lo + 0x1F;
    [(u_lo, v_lo), (u_hi, v_lo), (u_lo, v_hi), (u_hi, v_hi)]
}

/// Which sub-tile a `(sub_row, sub_col)` sub-quad of a cell takes.
///
/// Retail walks the row pair in the outer loop and the column pair in the
/// inner, advancing the sub-tile row pointer by `0x10` per emit, so this is
/// just scan order.
pub const fn sub_tile_index(sub_row: usize, sub_col: usize) -> usize {
    sub_row * 2 + sub_col
}

/// The lattice indices a sub-quad reads, in the quad's vertex order.
///
/// Vertex order matches the sub-tile UV order: `(z, x)` pairs
/// `(row, col), (row, col+1), (row+1, col), (row+1, col+1)`.
pub const fn sub_quad_lattice(sub_row: usize, sub_col: usize) -> [(usize, usize); 4] {
    [
        (sub_row, sub_col),
        (sub_row, sub_col + 1),
        (sub_row + 1, sub_col),
        (sub_row + 1, sub_col + 1),
    ]
}

/// The GTE `IR0` the emitter loads before each of its four `DPCS` sites:
/// the vertex's projected `SZ` shifted right by [`GRID_IR0_SZ_SHIFT`]
/// (`srl s5, s5, 0x2` + `mtc2 s5, IR0` at `0x801d0608..0x801d060c`).
///
/// The value is in the GTE's `0x1000 = 1.0` blend scale and is **not**
/// saturated - `mtc2` writes the raw register, so depths past `0x4000`
/// yield `IR0 > 0x1000` and the blend extrapolates past the far colour
/// until the DPCS colour-FIFO clamp bounds the output.
pub fn grid_ir0_raw(sz: i32) -> i32 {
    sz >> GRID_IR0_SZ_SHIFT
}

/// Shift applied to `SZ` to form the grid's `IR0`.
pub const GRID_IR0_SZ_SHIFT: u32 = 2;

/// One (full blend) on the GTE's `IR0` scale.
pub const GRID_IR0_ONE: i32 = 0x1000;

/// [`grid_ir0_raw`] as a `0..` fraction of full blend: `sz / 0x4000`.
/// Deliberately unclamped above 1 - see [`grid_ir0_raw`].
pub fn grid_ir0(sz: i32) -> f32 {
    grid_ir0_raw(sz) as f32 / GRID_IR0_ONE as f32
}

/// The view depth at which the grid's cue reaches exactly 1.0
/// (`GRID_IR0_ONE << GRID_IR0_SZ_SHIFT`).
pub const GRID_IR0_FULL_DEPTH: i32 = GRID_IR0_ONE << GRID_IR0_SZ_SHIFT;

/// The deepest unbiased view depth a drawn cell can have - the far cull is
/// on the biased depth ([`classify_cell`]), so the world-space cutoff is
/// `FAR_LIMIT - NEAR_BIAS`.
pub const GRID_CUE_FAR_Z: i32 = FAR_LIMIT - NEAR_BIAS;

/// End point of the equivalent linear ramp a host stages: the ramp's far
/// depth. With [`grid_cue_max_ir0`] at this depth, `ir0(z) = z / 0x4000`
/// holds across the whole drawable range `0..=GRID_CUE_FAR_Z` - i.e. the
/// per-vertex `SZ >> 2` law expressed as `(near_z, far_z, max_ir0) =
/// (0, GRID_CUE_FAR_Z, GRID_CUE_FAR_Z / 0x4000)`.
pub fn grid_cue_far_z() -> f32 {
    GRID_CUE_FAR_Z as f32
}

/// `IR0` at [`grid_cue_far_z`] - above 1.0, because retail's manual `mtc2`
/// load never saturates (the DPCS output clamp is what bounds the pixel).
pub fn grid_cue_max_ir0() -> f32 {
    GRID_CUE_FAR_Z as f32 / GRID_IR0_FULL_DEPTH as f32
}

/// The neutral far-colour *base* every capture shows an ordinary battle
/// settling on (`ctx + 0x890` after the intro fade): `0x808080`.
pub const GRID_FAR_BASE_NEUTRAL: [u8; 3] = [0x80; 3];

/// The far colour the grid (and backdrop) draw with, per stage class:
/// `FUN_80050120` derives the staged word at `0x8007BB48` from the base as
/// `c >> 1` per channel on ordinary stages (`0x800507fc`) and
/// `(c - 0x010101) * 2` per channel on the [`OutdoorCueTable`] stages
/// (`0x80050834`); the GTE control regs get that word times 16.
///
/// Channel arithmetic saturates here; retail's packed-word form can borrow
/// across channels when a channel is below the subtrahend, which the
/// observed neutral base never exercises.
pub fn grid_far_colour(base: [u8; 3], outdoor: bool) -> [u8; 3] {
    base.map(|c| {
        if outdoor {
            c.saturating_sub(1).saturating_mul(2)
        } else {
            c >> 1
        }
    })
}

/// Capture-pinned settled far colour on ordinary (indoor) stages.
pub const GRID_FAR_INDOOR: [u8; 3] = [0x40; 3];

/// Capture-pinned settled far colour on the outdoor-table stages.
pub const GRID_FAR_OUTDOOR: [u8; 3] = [0xFE; 3];

/// Virtual address of the outdoor depth-cue stage table in `SCUS_942.54` -
/// the sibling of the mirror-X table, scanned at `0x80051c1c..0x80051c6c`
/// into the flag byte `0x8007BDA8`.
pub const OUTDOOR_CUE_TABLE_VA: u32 = 0x8007_8C1C;

/// Byte offset of [`OUTDOOR_CUE_TABLE_VA`] inside the `SCUS_942.54` file
/// image (same `va - 0x8000F800` mapping as
/// `legaia_asset::battle_backdrop::MIRROR_X_TABLE_SCUS_OFFSET`).
pub const OUTDOOR_CUE_TABLE_SCUS_OFFSET: usize = 0x6_941C;

/// Upper bound on the zero-terminated table walk, so a mis-aimed offset
/// cannot scan the whole executable.
const OUTDOOR_CUE_TABLE_MAX_SLOTS: usize = 64;

/// The `SCUS_942.54` table of wide-open outdoor stages whose backdrop far
/// colour takes the brightening `(c - 0x010101) * 2` arm (and whose
/// backdrop cue ceiling is `0xC00` rather than `0x800`). 13 ids on the
/// retail disc: the nine kingdom-overworld variants plus `retona`,
/// `deene`, `kor5` and `rikuroa`.
#[derive(Debug, Clone, Default)]
pub struct OutdoorCueTable {
    ids: Vec<u16>,
}

impl OutdoorCueTable {
    /// Parse the zero-terminated `u16` table out of a `SCUS_942.54` image.
    /// `None` when the buffer is short or unterminated - not the retail
    /// USA executable; fall back to the indoor arm rather than trust a
    /// garbage list.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let mut ids = Vec::new();
        let mut off = OUTDOOR_CUE_TABLE_SCUS_OFFSET;
        for _ in 0..OUTDOOR_CUE_TABLE_MAX_SLOTS {
            let raw = scus.get(off..off + 2)?;
            let v = u16::from_le_bytes([raw[0], raw[1]]);
            if v == 0 {
                return Some(Self { ids });
            }
            ids.push(v);
            off += 2;
        }
        None
    }

    /// Table slots in file order.
    pub fn ids(&self) -> &[u16] {
        &self.ids
    }

    /// Whether this runtime stage id takes the outdoor (brightened) arm.
    pub fn contains_runtime_id(&self, id: u16) -> bool {
        self.ids.contains(&id)
    }

    /// [`Self::contains_runtime_id`] keyed by PROT extraction index
    /// (`runtime id + 3`, the `battle_backdrop` mapping).
    pub fn contains_prot_index(&self, prot_index: u32) -> bool {
        legaia_asset::battle_backdrop::runtime_stage_id(prot_index)
            .is_some_and(|id| self.contains_runtime_id(id))
    }

    /// The settled far colour for the stage at this PROT extraction index.
    pub fn far_colour_for_prot_index(&self, prot_index: u32) -> [u8; 3] {
        grid_far_colour(GRID_FAR_BASE_NEUTRAL, self.contains_prot_index(prot_index))
    }
}

/// Sub-quads emitted per visible cell (2x2).
pub const SUB_QUADS_PER_CELL: usize = 4;

/// Prim size in bytes retail advances the primitive cursor by per quad
/// (`0x34` = a 13-word `POLY_GT4`, tag plus twelve).
pub const QUAD_PRIM_BYTES: usize = 0x34;

/// Ordering-table tag word length field the emit loop ORs in (`0x0C000000`).
pub const QUAD_OT_TAG: u32 = 0x0C00_0000;

/// A whole grid's cell visibility buffer plus the emit count, as the two passes
/// produce them. `visible_cells * 4` is the quad count retail returns in `$v0`.
#[derive(Clone, Debug, Default)]
pub struct GroundGrid {
    /// Cell columns (`_DAT_1F8003F8`).
    pub width: i16,
    /// Cell rows (`_DAT_1F8003FA`).
    pub height: i16,
    /// Row-major visibility bytes, `width * height` long.
    pub vis: Vec<i8>,
}

impl GroundGrid {
    /// Run pass 1 with a caller-supplied projector: `project(x, z) -> IR3`.
    ///
    /// The caller owns the GTE, so the projector is where retail's
    /// `mtc2`/`RTPS`/`mfc2 IR3` triple lands. The Y each vertex implicitly
    /// carries is [`implied_y`] of the X passed in.
    pub fn classify(width: i16, height: i16, mut project: impl FnMut(i32, i32) -> i32) -> Self {
        let mut vis = Vec::with_capacity((width.max(0) as usize) * (height.max(0) as usize));
        for row in 0..height.max(0) as i32 {
            for col in 0..width.max(0) as i32 {
                let (x, z) = probe_point(width, height, col, row);
                vis.push(classify_cell(project(x, z)).as_byte());
            }
        }
        Self { width, height, vis }
    }

    /// The visibility class stored for a cell, or `None` when out of range.
    pub fn cell(&self, col: i32, row: i32) -> Option<i8> {
        if col < 0 || row < 0 || col >= self.width as i32 || row >= self.height as i32 {
            return None;
        }
        self.vis
            .get(row as usize * self.width as usize + col as usize)
            .copied()
    }

    /// Cells pass 2 emits for - the `1` class only.
    pub fn visible_cells(&self) -> usize {
        self.vis.iter().filter(|&&b| b == 1).count()
    }

    /// Quads pass 2 emits, before the screen-space reject: four per visible
    /// cell. This is the upper bound on retail's `$v0`.
    pub fn max_quads(&self) -> usize {
        self.visible_cells() * SUB_QUADS_PER_CELL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_is_x_centred_and_z_biased_one_cell() {
        // The 28x28 grid the live battle capture reports.
        assert_eq!(grid_origin(28, 28), (-7168, -7680));
        // X is symmetric about zero; Z is a whole cell further out.
        let (x, z) = grid_origin(28, 28);
        assert_eq!(z, x - CELL_PITCH);
    }

    #[test]
    fn odd_cell_counts_round_toward_zero_like_an_sra() {
        // `sra` of a positive halfword truncates, so 29 and 28 share an origin.
        assert_eq!(grid_origin(29, 29), grid_origin(28, 28));
    }

    #[test]
    fn probe_points_sit_a_half_step_inside_the_cell() {
        let (x0, z0) = grid_origin(4, 4);
        assert_eq!(probe_point(4, 4, 0, 0), (x0 + 0x100, z0 + 0x100));
        assert_eq!(probe_point(4, 4, 3, 2), (x0 + 0x700, z0 + 0x500));
    }

    #[test]
    fn depth_classes_split_at_the_two_documented_bounds() {
        assert_eq!(classify_cell(-NEAR_BIAS), CellVis::Behind);
        assert_eq!(classify_cell(-NEAR_BIAS - 1), CellVis::Behind);
        assert_eq!(classify_cell(-NEAR_BIAS + 1), CellVis::Visible);
        // The far test is on the biased depth, so the world-space cutoff is
        // FAR_LIMIT - NEAR_BIAS = 0x6500.
        assert_eq!(classify_cell(0x6500), CellVis::Visible);
        assert_eq!(classify_cell(0x6501), CellVis::Far);
        assert_eq!(CellVis::Behind.as_byte(), -1);
        assert_eq!(CellVis::Far.as_byte(), 0);
        assert_eq!(CellVis::Visible.as_byte(), 1);
    }

    #[test]
    fn implied_y_is_zero_for_the_positive_half_and_minus_one_for_the_other() {
        assert_eq!(implied_y(0), 0);
        assert_eq!(implied_y(0x7FFF), 0);
        assert_eq!(implied_y(-1), -1);
        assert_eq!(implied_y(-0x7000), -1);
    }

    #[test]
    fn lattice_is_three_by_three_at_the_sub_step() {
        let l = cell_lattice(4, 4, 1, 1);
        let (x0, z0) = grid_origin(4, 4);
        assert_eq!(l[0][0], (x0 + CELL_PITCH, z0 + CELL_PITCH));
        assert_eq!(l[0][2], (x0 + CELL_PITCH + 0x200, z0 + CELL_PITCH));
        assert_eq!(l[2][2], (x0 + CELL_PITCH + 0x200, z0 + CELL_PITCH + 0x200));
        // Middle samples are the sub-step, i.e. the shared edge of the 2x2.
        assert_eq!(l[1][1], (x0 + CELL_PITCH + 0x100, z0 + CELL_PITCH + 0x100));
    }

    #[test]
    fn screen_reject_is_an_aabb_overlap_with_the_viewport() {
        let on = [(10, 10), (60, 10), (10, 60), (60, 60)];
        assert!(quad_on_screen(on));
        // Entirely off the left edge.
        let left = [(-40, 10), (-1, 10), (-40, 60), (-1, 60)];
        assert!(!quad_on_screen(left));
        // Entirely below the bottom edge.
        let below = [(10, 240), (60, 240), (10, 300), (60, 300)];
        assert!(!quad_on_screen(below));
        // Entirely off the right edge.
        let right = [(320, 10), (400, 10), (320, 60), (400, 60)];
        assert!(!quad_on_screen(right));
        // Straddling: one corner inside is enough.
        let straddle = [(-40, -40), (10, -40), (-40, 10), (10, 10)];
        assert!(quad_on_screen(straddle));
    }

    #[test]
    fn sub_tiles_tile_the_window_without_gaps_or_overlap() {
        let mut covered = std::collections::HashSet::new();
        for i in 0..SUB_QUADS_PER_CELL {
            let uv = sub_tile_uv(i);
            // Each tile is 0x20 wide and tall, expressed as inclusive bounds.
            assert_eq!(uv[1].0 - uv[0].0, 0x1F);
            assert_eq!(uv[2].1 - uv[0].1, 0x1F);
            for u in uv[0].0..=uv[1].0 {
                for v in uv[0].1..=uv[2].1 {
                    assert!(covered.insert((u, v)), "texel ({u},{v}) claimed twice");
                }
            }
        }
        assert_eq!(covered.len(), 64 * 64, "the 64x64 window is fully covered");
        assert!(covered.iter().all(|&(u, v)| u >= 0xC0 && v >= 0xC0));
    }

    #[test]
    fn sub_tile_order_follows_the_sub_quad_scan_order() {
        assert_eq!(sub_tile_index(0, 0), 0);
        assert_eq!(sub_tile_index(0, 1), 1);
        assert_eq!(sub_tile_index(1, 0), 2);
        assert_eq!(sub_tile_index(1, 1), 3);
        // u rises with the column, v with the row.
        assert!(sub_tile_uv(1)[0].0 > sub_tile_uv(0)[0].0);
        assert_eq!(sub_tile_uv(1)[0].1, sub_tile_uv(0)[0].1);
        assert!(sub_tile_uv(2)[0].1 > sub_tile_uv(0)[0].1);
        assert_eq!(sub_tile_uv(2)[0].0, sub_tile_uv(0)[0].0);
    }

    #[test]
    fn sub_quads_partition_the_lattice() {
        let mut seen = std::collections::HashSet::new();
        for r in 0..2 {
            for c in 0..2 {
                for idx in sub_quad_lattice(r, c) {
                    assert!(idx.0 < 3 && idx.1 < 3);
                    seen.insert(idx);
                }
            }
        }
        assert_eq!(seen.len(), 9, "all nine lattice points are used");
    }

    /// The `DPCS` kernel, as this crate's test-local mirror of
    /// `engine_render::psx_light::depth_cue` (extrapolate, then clamp -
    /// the GTE's colour-FIFO saturation).
    fn dpcs(c: f32, fc: f32, ir0: f32) -> f32 {
        (c + (fc - c) * ir0).clamp(0.0, 255.0)
    }

    #[test]
    fn ir0_is_sz_over_four_on_the_gte_scale() {
        assert_eq!(grid_ir0_raw(0), 0);
        assert_eq!(grid_ir0_raw(0x1000), 0x400);
        // Full blend at SZ = 0x4000...
        assert_eq!(grid_ir0_raw(GRID_IR0_FULL_DEPTH), GRID_IR0_ONE);
        assert!((grid_ir0(GRID_IR0_FULL_DEPTH) - 1.0).abs() < 1e-6);
        // ...and deliberately unsaturated past it, like the mtc2 load.
        assert!(grid_ir0_raw(0x6500) > GRID_IR0_ONE);
        assert!((grid_ir0(0x6500) - (0x6500 as f32 / 0x4000 as f32)).abs() < 1e-3);
    }

    #[test]
    fn cue_ramp_constants_reproduce_the_per_vertex_law() {
        // The linear ramp a host stages - ir0(z) = clamp(z / far_z, 0, 1)
        // * max_ir0 - must equal SZ >> 2 (as a fraction of 0x1000) across
        // the whole drawable depth range.
        let far_z = grid_cue_far_z();
        let max = grid_cue_max_ir0();
        assert!(max > 1.0, "the grid's cue extrapolates past the far colour");
        for z in (0..=GRID_CUE_FAR_Z).step_by(0x100) {
            let ramp = (z as f32 / far_z).clamp(0.0, 1.0) * max;
            let law = grid_ir0(z & !0x3); // the srl truncates the low bits
            assert!(
                (ramp - law).abs() < 1.5e-3,
                "z={z:#x}: ramp {ramp} != law {law}"
            );
        }
    }

    #[test]
    fn far_colour_arms_reproduce_the_captured_values() {
        // Settled captures: indoor (Queen Bee / Gimard, town01 stages) and
        // outdoor (vs Gobu Gobu, stage id 0x55 = map01).
        assert_eq!(
            grid_far_colour(GRID_FAR_BASE_NEUTRAL, false),
            GRID_FAR_INDOOR
        );
        assert_eq!(
            grid_far_colour(GRID_FAR_BASE_NEUTRAL, true),
            GRID_FAR_OUTDOOR
        );
        // The battle-intro fade sample: the same indoor arm over the
        // ramping base (captured (6,6,6) at base 0x0C0C0C, frame 1).
        assert_eq!(grid_far_colour([0x0C; 3], false), [0x06; 3]);
    }

    #[test]
    fn dpcs_at_the_captured_far_colours_pins_the_drawn_packet_colour() {
        let neutral = 0x80 as f32; // the grid quads' packet colour
        // Indoor: full blend lands exactly on the far colour...
        assert_eq!(dpcs(neutral, 0x40 as f32, 1.0), 0x40 as f32);
        // ...and the far cull edge extrapolates darker (SZ = 0x6500).
        let edge = dpcs(neutral, 0x40 as f32, grid_ir0(0x6500));
        assert!((edge - 27.0).abs() < 1.0, "edge = {edge}");
        // Outdoor: brightens toward 0xFE and saturates just past full
        // blend rather than overshooting.
        assert_eq!(dpcs(neutral, 0xFE as f32, 1.0), 0xFE as f32);
        assert_eq!(dpcs(neutral, 0xFE as f32, grid_ir0(0x6500)), 255.0);
        // ir0 = 0 is the identity - the near edge draws unfogged.
        assert_eq!(dpcs(neutral, 0x40 as f32, 0.0), neutral);
    }

    #[test]
    fn outdoor_table_parses_and_classifies() {
        let mut scus = vec![0u8; OUTDOOR_CUE_TABLE_SCUS_OFFSET + 16];
        scus[OUTDOOR_CUE_TABLE_SCUS_OFFSET..OUTDOOR_CUE_TABLE_SCUS_OFFSET + 6]
            .copy_from_slice(&[0x55, 0x00, 0x9E, 0x00, 0x00, 0x00]);
        let t = OutdoorCueTable::from_scus(&scus).expect("table");
        assert_eq!(t.ids(), &[0x55, 0x9E]);
        assert!(t.contains_runtime_id(0x55));
        assert!(!t.contains_runtime_id(0x15));
        // PROT keying: runtime id + 3 (0x55 -> PROT 88 = map01's stage).
        assert!(t.contains_prot_index(88));
        assert!(!t.contains_prot_index(24)); // 0x15 + 3, the Queen Bee stage
        assert_eq!(t.far_colour_for_prot_index(88), GRID_FAR_OUTDOOR);
        assert_eq!(t.far_colour_for_prot_index(24), GRID_FAR_INDOOR);
        // Unterminated / short buffers refuse rather than fabricate.
        assert!(OutdoorCueTable::from_scus(&[0u8; 16]).is_none());
        let junk = vec![0x11u8; OUTDOOR_CUE_TABLE_SCUS_OFFSET + 4096];
        assert!(OutdoorCueTable::from_scus(&junk).is_none());
    }

    #[test]
    fn classify_walks_row_major_and_counts_the_emitted_quads() {
        // A projector that only keeps the second row in range.
        let (_, z_min) = grid_origin(4, 3);
        let grid = GroundGrid::classify(4, 3, |_x, z| {
            if z == z_min + SUB_STEP + CELL_PITCH {
                0x1000
            } else {
                -0x8000
            }
        });
        assert_eq!(grid.vis.len(), 12);
        assert_eq!(grid.visible_cells(), 4);
        assert_eq!(grid.max_quads(), 16);
        for col in 0..4 {
            assert_eq!(grid.cell(col, 1), Some(1));
            assert_eq!(grid.cell(col, 0), Some(-1));
        }
        assert_eq!(grid.cell(4, 0), None);
    }
}
