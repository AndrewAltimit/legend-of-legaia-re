//! How retail places a battle-stage backdrop shell: **two** copies of one
//! registered TMD, and **not** every object in it.
//!
//! A `scene_tmd_stream` PROT entry holds one backdrop shell authored as a
//! half - object 0's vertex pool sits entirely on one side of `X = 0` or
//! `Z = 0` (see [`shell_shape`]). That half is what the file holds, and it
//! is complete; nothing is truncated on the way in. What completes the
//! circle is a **second draw of the same mesh under a second transform**,
//! and the transform is a per-stage decision the executable carries as a
//! table.
//!
//! [`shell_shape`]: crate::scene_tmd_stream::shell_shape
//!
//! # Battle init
//!
//! `FUN_800513F0` registers the backdrop TMD **once**
//! (`80051a60 jal 0x80026b4c`, handle stored at `DAT_80076810`) and then
//! allocates **two** actors from the same descriptor - `80051a7c` and
//! `80051aa8`, both `jal 0x80020de0` with `a0 = 0x8007680c` - parking their
//! pointers at `battle_ctx + 0x106C` (copy A) and `+0x1070` (copy B).
//! `FUN_80050120` drives the pair in lockstep: the depth-cue ramp at
//! `+0x78` and the draw-mode selector at `+0x56` are written to both on the
//! same path (`80050848..80050880`).
//!
//! Copy A is placed at raw coordinates. Copy B gets a transform:
//!
//! - **default** - `80051bc0 li v0,0x800` / `80051bc4 sh v0,0x26(v1)` writes
//!   `+0x26 = 0x800`. `+0x26` is the actor's world-Y angle (the second of
//!   the three half-words `FUN_80026988` reads at `actor + 0x24`; that
//!   kernel writes `sin` of it bare into matrix element `[0][2]` and `cos`
//!   into `[2][2]`, which only a Y rotation does). `0x800` of the `0x1000`
//!   full turn is exactly 180 degrees: [`SecondCopy::HalfTurn`].
//! - **per-stage exception** - `80051bc8..80051c18` walks the
//!   zero-terminated `u16` table at `DAT_80078B50`, comparing each entry
//!   against the backdrop id. On a hit (`80051cc4`) it writes `+0x5A = 2`
//!   and `+0x26 = 0` instead. `FUN_8001ADA4` case 3 turns `+0x5A & 2` into
//!   `_DAT_1F800348 = -0x1000` (`8001af28..8001af34`) and calls
//!   `FUN_8005B4E8` (`ScaleMatrix`) - X scale `-1.0` in 4.12, a reflection
//!   in the YZ plane: [`SecondCopy::MirrorX`]. The same predicate
//!   (`+0x5A & 0xE` at `8001afd8`) negates the per-object rotation argument
//!   and swaps the draw-call mode word from `0x40000000` to `0x48000000`,
//!   which is the winding compensation a negative-determinant transform
//!   needs.
//!
//! Both copies are drawn: `FUN_80050120` sets `+0x56 = 3` on each, which is
//! the `FUN_8001ADA4` dispatch selector (`8001ae60 lhu v0,0x56(s0)`, jump
//! table at `0x8001042C`).
//!
//! # Which objects draw
//!
//! Immediately after allocating the pair, `80051ad4..80051bac` decrements
//! each actor's object count at `**(actor + 0x44)` and left-shifts the
//! pointer array by one **from index 1**: `A[i] = B[i+1]` and
//! `B[i] = B[i+1]` for `i >= 1`. The surviving list is objects
//! `0, 2, 3, ...` - **object 1 is dropped**. It is still resident in the
//! relocated object table, just unreferenced by either actor.
//!
//! The whole block is gated on `DAT_8007B64B == 0` (`80051abc` /
//! `80051acc`). That byte is bit 5 of byte `+8` of the field scene's
//! encounter-region record (`801DA09C..801DA0AC` in the field battle-intro
//! overlay), the same byte whose low 5 bits pick which of a scene's stage
//! variants to use. Set, it keeps object 1.
//!
//! # Id space
//!
//! The value compared against the table is `word[0x80084540] +
//! byte[0x8007BD60] & 0x7F` (`80051a20..80051a40`), a small stage id - not
//! a pointer. It maps onto the PROT extraction index by
//! [`RUNTIME_ID_TO_PROT_OFFSET`]. Every one of the table's distinct ids
//! resolves to a `scene_tmd_stream` entry under that offset, which is what
//! pins it.
//!
//! # The sibling table
//!
//! `80051c1c..80051c6c` scans a **second** table at `DAT_80078C1C` the same
//! way and against the same id, but it touches neither actor - it sets a
//! byte at `0x8007BDA8`, which `FUN_80050120` reads to pick the backdrop's
//! depth-cue ceiling (`0x800` vs `0xC00`) and far-colour scaling. Its 13
//! ids are the wide-open outdoor stages. It is deliberately not modelled
//! here: it is a fog parameter, not a placement one.

use crate::scene_tmd_stream;

/// Virtual address of the mirror-X stage table in `SCUS_942.54`.
pub const MIRROR_X_TABLE_VA: u32 = 0x8007_8B50;

/// Byte offset of [`MIRROR_X_TABLE_VA`] inside the `SCUS_942.54` file
/// image (`va - t_addr + 0x800`, the PS-X EXE header being `0x800` bytes).
pub const MIRROR_X_TABLE_SCUS_OFFSET: usize = 0x6_9350;

/// Upper bound on the zero-terminated table's length, so a mis-aimed
/// offset cannot walk the whole executable.
const MIRROR_X_TABLE_MAX_SLOTS: usize = 256;

/// Retail's stage id plus this is the entry's PROT extraction index.
pub const RUNTIME_ID_TO_PROT_OFFSET: u32 = 3;

/// The transform retail applies to the **second** copy of a backdrop shell.
///
/// Both alternatives complete a shell whose open side faces `-X` or `+X`.
/// Only [`HalfTurn`] completes one whose open side faces `-Z`, because a
/// `-Z`-open shell is symmetric about `X = 0` and so is mapped onto itself
/// by [`MirrorX`]. Retail's table respects that: no `-Z`-open stage is on
/// the mirror list.
///
/// [`HalfTurn`]: SecondCopy::HalfTurn
/// [`MirrorX`]: SecondCopy::MirrorX
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondCopy {
    /// `actor + 0x26 = 0x800` - a half turn about world Y.
    HalfTurn,
    /// `actor + 0x5A = 2` - X scale `-1`, a reflection in the YZ plane.
    MirrorX,
}

impl SecondCopy {
    /// Per-axis scale of the equivalent diagonal matrix. Both retail
    /// transforms are exact at the integer level, so no trigonometry is
    /// involved: a half turn about Y is `diag(-1, 1, -1)`.
    pub const fn scale(self) -> [f32; 3] {
        match self {
            SecondCopy::HalfTurn => [-1.0, 1.0, -1.0],
            SecondCopy::MirrorX => [-1.0, 1.0, 1.0],
        }
    }

    /// Whether the transform has negative determinant and so reverses
    /// triangle winding. Retail compensates for exactly this case by
    /// swapping the draw-call mode word (`0x40000000` -> `0x48000000`).
    pub const fn flips_winding(self) -> bool {
        matches!(self, SecondCopy::MirrorX)
    }

    /// Short label for a viewer status line.
    pub const fn label(self) -> &'static str {
        match self {
            SecondCopy::HalfTurn => "half-turn",
            SecondCopy::MirrorX => "mirrored",
        }
    }
}

/// The `SCUS_942.54` table of stages whose second backdrop copy is
/// mirrored rather than half-turned.
#[derive(Debug, Clone, Default)]
pub struct MirrorXTable {
    ids: Vec<u16>,
}

impl MirrorXTable {
    /// Parse the zero-terminated `u16` table out of a `SCUS_942.54` image.
    ///
    /// Returns `None` when the buffer is too short to hold the table's
    /// first slot, or when the table does not terminate within
    /// [`MIRROR_X_TABLE_MAX_SLOTS`] - either means this is not the retail
    /// USA executable and the caller should fall back to the default
    /// transform rather than trust a garbage list.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let mut ids = Vec::new();
        let mut off = MIRROR_X_TABLE_SCUS_OFFSET;
        for _ in 0..MIRROR_X_TABLE_MAX_SLOTS {
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

    /// Table slots in file order. Retail's list repeats one id, so this is
    /// longer than the set of distinct stages it names.
    pub fn ids(&self) -> &[u16] {
        &self.ids
    }

    /// Whether this runtime stage id takes the mirrored second copy.
    pub fn contains_runtime_id(&self, id: u16) -> bool {
        self.ids.contains(&id)
    }

    /// [`Self::contains_runtime_id`] keyed by PROT extraction index.
    pub fn contains_prot_index(&self, prot_index: u32) -> bool {
        runtime_stage_id(prot_index).is_some_and(|id| self.contains_runtime_id(id))
    }

    /// The second-copy transform for the entry at this PROT extraction
    /// index.
    pub fn second_copy_for_prot_index(&self, prot_index: u32) -> SecondCopy {
        if self.contains_prot_index(prot_index) {
            SecondCopy::MirrorX
        } else {
            SecondCopy::HalfTurn
        }
    }
}

/// Retail's stage id for a PROT extraction index, or `None` for the first
/// three entries, which are below the id space.
pub fn runtime_stage_id(prot_index: u32) -> Option<u16> {
    let id = prot_index.checked_sub(RUNTIME_ID_TO_PROT_OFFSET)?;
    u16::try_from(id).ok()
}

/// The PROT extraction index a runtime stage id names.
pub fn prot_index_for_runtime_id(id: u16) -> u32 {
    u32::from(id) + RUNTIME_ID_TO_PROT_OFFSET
}

/// The TMD object indices a backdrop actor draws, in draw order: object 0
/// then everything from index 2 up. Object 1 is dropped.
///
/// This is the `DAT_8007B64B == 0` arm, which is what every catalogued
/// battle takes. For the other arm see [`drawn_object_indices_gated`].
///
/// A one-object TMD would leave the actor with a zero count, so it draws
/// nothing; the retail corpus has no such entry (every `scene_tmd_stream`
/// backdrop carries either 2 or 4 objects).
pub fn drawn_object_indices(object_count: usize) -> Vec<usize> {
    drawn_object_indices_gated(object_count, false)
}

/// [`drawn_object_indices`] with retail's gate exposed.
///
/// `keep_object_1` is `DAT_8007B64B != 0` - bit 5 of byte `+8` of the field
/// scene's encounter-region record. Set, `80051abc` / `80051acc` branch
/// past the whole object edit, so every object stays in the draw list.
pub fn drawn_object_indices_gated(object_count: usize, keep_object_1: bool) -> Vec<usize> {
    if keep_object_1 {
        return (0..object_count).collect();
    }
    if object_count == 0 {
        return Vec::new();
    }
    std::iter::once(0).chain(2..object_count).collect()
}

/// The drawn-object subset of a backdrop TMD, as a standalone [`Tmd`] the
/// ordinary mesh builders can consume.
///
/// [`Tmd`]: legaia_tmd::Tmd
pub fn drawn_objects_tmd(tmd: &legaia_tmd::Tmd) -> legaia_tmd::Tmd {
    legaia_tmd::Tmd {
        header: tmd.header.clone(),
        objects: drawn_object_indices(tmd.objects.len())
            .into_iter()
            .filter_map(|i| tmd.objects.get(i).cloned())
            .collect(),
    }
}

/// One-line description of how retail places this backdrop, for a viewer
/// status line. Pass the second-copy transform when the caller could
/// resolve it from `SCUS_942.54`; `None` still names the completion,
/// without claiming which of the two it is.
pub fn describe_placement(
    shape: Option<&scene_tmd_stream::ShellShape>,
    second: Option<SecondCopy>,
) -> String {
    let open = shape
        .filter(|s| s.is_half_shell())
        .map(|s| format!(", authored half open toward {}", s.open.label()))
        .unwrap_or_default();
    match second {
        Some(c) => format!(
            "battle-stage backdrop - drawn twice, {} second copy{open}",
            c.label()
        ),
        None => format!("battle-stage backdrop - drawn twice, second copy transformed{open}"),
    }
}

// ---------------------------------------------------------------------------
// The procedural ground grid
// ---------------------------------------------------------------------------
//
// The shell above is only half the backdrop. The grass underfoot is not
// geometry from a file at all - it is emitted procedurally by
// `func_0x801d02c0`, the sole draw call the mode-`0x15` render makes.

/// Grid cell pitch in world units (`0x200`).
pub const GRID_CELL_PITCH: i32 = 0x200;

/// Sub-cell step (`0x100`) - half a cell, the spacing of the `3 x 3` corner
/// lattice each cell is projected from.
pub const GRID_SUB_STEP: i32 = 0x100;

/// Cells per side of the live grid (`_DAT_1f8003f8` / `_DAT_1f8003fa`).
pub const GRID_CELLS: i32 = 28;

/// Texture page attribute for the ground tile: 4bpp page at framebuffer
/// `(832, 0)` (`lui s1,0xd` at `0x801d030c`, the `tpage` half of the second
/// POLY_GT4 UV word).
pub const GROUND_TSB: u16 = 0x000D;

/// CLUT attribute for the ground tile: palette at framebuffer `(0, 479)`
/// (`lui t5,0x77c0` at `0x801d0304`, the `clut` half of the first UV word).
pub const GROUND_CBA: u16 = 0x77C0;

/// Low UV coordinate of the tile window (`0xC0`). The window is
/// `(192..=255)^2` and holds four `32 x 32` sub-tiles.
pub const GROUND_UV_BASE: u8 = 0xC0;

/// Side of one sub-tile in texels.
pub const GROUND_SUB_TILE: u8 = 0x20;

/// Sub-tiles per cell side - the emit loop runs `2 x 2` per visible cell.
pub const GRID_SUB_TILES_PER_SIDE: i32 = 2;

/// World `y` the grid quads are emitted at. Retail's plane is exactly `0`;
/// the port pushes it **one unit away from the viewer** (PSX up is `-y`, so
/// away is `+y`).
///
/// This is the standard cross-draw coplanar resolution described in
/// `docs/subsystems/renderer.md`, and it is needed because the backdrop
/// shell's own flat ground ring (object 3 of the four-object domes) also
/// lives on `y = 0`. Retail never sees the conflict: the grid links into the
/// **farthest** ordering-table bucket (`0x3fec >> shift`, a constant slot for
/// the whole grid), so it paints first and the backdrop actor paints over it.
/// A depth-tested port has no such order - two exactly coplanar surfaces
/// interpolate to depths differing only by float rounding and the winner
/// flickers per fragment.
///
/// One unit against the `0x200` cell pitch is far below visibility and sits
/// inside authored practice (retail art separates its own decals by about a
/// unit). Before the second backdrop copy landed this never showed, because
/// only the authored half of the ring existed and it never reached the near
/// foreground.
pub const GRID_PLANE_Y: f32 = 1.0;

/// One sub-tile's UV window as `(u_lo, v_lo, u_hi, v_hi)`, inclusive.
///
/// These are the emitter's own UV word table, built by the `lui`/`ori` block
/// at `0x801d0304..0x801d03a0` into scratchpad `0x1f800034` and read back one
/// group per quad at `0x801d0660` (the read cursor advances `0x10` per quad,
/// `0x801d06c8`). Decoding the POLY_GT4 UV words - word 0 `clut:u0:v0`, word 1
/// `tpage:u1:v1`, words 2/3 `u2:v2` / `u3:v3` - gives:
///
/// | Quad | Words | `u` | `v` |
/// |---:|---|---|---|
/// | 0 | `77c0c0c0 000dc0df 0000dfc0 0000dfdf` | `0xC0..=0xDF` | `0xC0..=0xDF` |
/// | 1 | `77c0c0e0 000dc0ff 0000dfe0 0000dfff` | `0xE0..=0xFF` | `0xC0..=0xDF` |
/// | 2 | `77c0e0c0 000de0df 0000ffc0 0000ffdf` | `0xC0..=0xDF` | `0xE0..=0xFF` |
/// | 3 | `77c0e0e0 000de0ff 0000ffe0 0000ffff` | `0xE0..=0xFF` | `0xE0..=0xFF` |
///
/// so quad `n` takes sub-tile `n`, and since the emit loop's inner axis is the
/// sub-column and its outer axis the sub-row, `n == sub_row * 2 + sub_col`.
/// **There is no RNG anywhere in the routine** and no corner mirroring: the
/// tiling is deterministic and the UVs are read verbatim from this table. The
/// random corner mirror belongs to the particle scatter `FUN_801E0080`.
pub const GROUND_SUB_TILE_UVS: [(u8, u8, u8, u8); 4] = [
    (0xC0, 0xC0, 0xDF, 0xDF),
    (0xE0, 0xC0, 0xFF, 0xDF),
    (0xC0, 0xE0, 0xDF, 0xFF),
    (0xE0, 0xE0, 0xFF, 0xFF),
];

/// The sub-tile a cell's `(sub_row, sub_col)` quad samples.
pub fn ground_sub_tile_uv(sub_row: i32, sub_col: i32) -> (u8, u8, u8, u8) {
    GROUND_SUB_TILE_UVS[((sub_row * GRID_SUB_TILES_PER_SIDE + sub_col) & 3) as usize]
}

/// World-space `(x, z)` of the grid's first cell corner for a `w x h` grid.
///
/// `0x801d03b4..0x801d03d8`: `x0 = -((w >> 1) << 9)` and
/// `z0 = -((h >> 1) << 9) - 0x200`. The `z` axis carries an extra `-0x200`
/// bias, so the grid is **not** symmetric about the origin - it sits one cell
/// further toward `-Z` than a naive centring would put it.
pub fn grid_origin(width: i32, height: i32) -> (i32, i32) {
    let x0 = -((width >> 1) << 9);
    let z0 = -((height >> 1) << 9) - GRID_CELL_PITCH;
    (x0, z0)
}

/// Pass-1 visibility verdict for one cell, matching the byte retail writes
/// into `_DAT_8007b814`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellVisibility {
    /// `-1`: the cell centre is at or behind the near limit.
    Behind,
    /// `0`: the cell centre is past the far limit.
    TooFar,
    /// `1`: emit this cell.
    Visible,
}

/// Near limit added to the view-space `z` before the sign test (`0x200`).
pub const GRID_NEAR_BIAS: i32 = 0x200;

/// Far limit the biased `z` is compared against (`0x6700`), i.e. a raw view
/// `z` of `0x6500`.
pub const GRID_FAR_LIMIT: i32 = 0x6700;

/// Classify one cell from the view-space `z` of its centre.
///
/// Pass 1 (`0x801d0420..0x801d0464`) transforms the cell centre
/// `(x + 0x100, ~0, z + 0x100)` by the view matrix with `MVMVA`
/// (`cop2 0x0480012`: rotation matrix, `V0`, `+TR`, `sf = 1`), reads `IR3`
/// back with `mfc2 t4, $11`, and then:
///
/// ```text
/// t = z + 0x200
/// if t <= 0              -> -1
/// else if t - 0x6700 > 0 ->  0
/// else                   ->  1
/// ```
///
/// Only `1` emits: pass 2 skips on both `bltz s0` (`0x801d04b0`) and
/// `beq s0, zero` (`0x801d04b8`).
///
/// Note what is *not* here - there is no screen-space test in pass 1. The
/// screen-space rejection is a separate, per-cell test in pass 2
/// ([`cell_offscreen`]).
pub fn classify_cell(view_z: i32) -> CellVisibility {
    let t = view_z + GRID_NEAR_BIAS;
    if t <= 0 {
        CellVisibility::Behind
    } else if t - GRID_FAR_LIMIT > 0 {
        CellVisibility::TooFar
    } else {
        CellVisibility::Visible
    }
}

/// PSX display width the pass-2 reject compares against.
pub const SCREEN_W: i32 = 0x140;
/// PSX display height the pass-2 reject compares against.
pub const SCREEN_H: i32 = 0xF0;

/// Pass-2 screen-space rejection for a cell, from the projected screen `(x, y)`
/// of its four **outer** corners.
///
/// `0x801d052c..0x801d05e8` reads the corner lattice's `(0,0)`, `(2,0)`,
/// `(0,2)` and `(2,2)` points and rejects the cell when all four fall off the
/// same edge: all `y <= 0`, or all `y >= 0xF0`, or all `x <= 0`, or all
/// `x >= 0x140`. It is a bounding-box reject against the display rect, so a
/// cell straddling an edge is kept.
pub fn cell_offscreen(corners: [(i32, i32); 4]) -> bool {
    corners.iter().all(|c| c.1 <= 0)
        || corners.iter().all(|c| c.1 >= SCREEN_H)
        || corners.iter().all(|c| c.0 <= 0)
        || corners.iter().all(|c| c.0 >= SCREEN_W)
}

/// Build the flat tiled battle ground grid - the port of `func_0x801d02c0`'s
/// pass-2 emit, as a [`VramMesh`] any host can upload.
///
/// A `GRID_CELLS` square field of `0x200`-pitch cells on the PSX `y = 0`
/// plane, textured from the constant [`GROUND_TSB`] / [`GROUND_CBA`] page.
/// Each cell is **four** quads, not one: retail projects a `3 x 3` corner
/// lattice per cell (`RTPT` three times over rows spaced `0x100` apart,
/// `0x801d04e4..0x801d0528`) and then emits `2 x 2` POLY_GT4s from it, taking
/// sub-tile `sub_row * 2 + sub_col` of the `(192..=255)^2` window for each.
///
/// The texture *address* is scene-independent - the scene's battle VRAM build
/// is what places that scene's own ground tile there (`town01` = warm sandy
/// pebbles). An engine heuristic that instead borrowed the dome's nearest
/// "grass vertex" sampled a blue texel region in `town01` and painted the
/// floor sky-blue.
///
/// # What this build leaves out, and why
///
/// Retail culls cells twice, every frame, because the grid is world-fixed and
/// the battle camera orbits over it: a view-space `z` bracket per cell
/// ([`classify_cell`]), then a screen-rect bounding-box reject
/// ([`cell_offscreen`]). Both kernels are ported and tested; neither is
/// applied here.
///
/// That is deliberate. Retail's culls exist to keep the ordering table and the
/// per-frame primitive budget inside a PSX's means: they remove only cells
/// that are already off-screen or behind the camera, so under a depth-buffered
/// projection with a real near plane they are **visually neutral**. Applying
/// them at build time would be worse than not applying them - the grid is
/// uploaded once and the camera then orbits, so cells culled against the entry
/// pose would stay culled once they swung into view.
///
/// [`VramMesh`]: legaia_tmd::mesh::VramMesh
pub fn build_ground_grid() -> legaia_tmd::mesh::VramMesh {
    let (x0, z0) = grid_origin(GRID_CELLS, GRID_CELLS);
    let sub = GRID_SUB_STEP as f32;

    let mut m = legaia_tmd::mesh::VramMesh {
        positions: Vec::new(),
        uvs: Vec::new(),
        cba_tsb: Vec::new(),
        indices: Vec::new(),
        normals: Vec::new(),
        colors: Vec::new(),
    };
    for iz in 0..GRID_CELLS {
        for ix in 0..GRID_CELLS {
            let cx = (x0 + ix * GRID_CELL_PITCH) as f32;
            let cz = (z0 + iz * GRID_CELL_PITCH) as f32;
            // The 2x2 sub-quads of this cell, each a `0x100` step of the
            // cell's own 3x3 corner lattice.
            for sr in 0..GRID_SUB_TILES_PER_SIDE {
                for sc in 0..GRID_SUB_TILES_PER_SIDE {
                    let (qx0, qz0) = (cx + sc as f32 * sub, cz + sr as f32 * sub);
                    let (qx1, qz1) = (qx0 + sub, qz0 + sub);
                    let (ua, va, ub, vb) = ground_sub_tile_uv(sr, sc);
                    let base = m.positions.len() as u32;
                    // Retail's POLY_GT4 corner order: v0 top-left, v1
                    // top-right, v2 bottom-left, v3 bottom-right - the same
                    // order the UV table's four words are written in.
                    for (x, z, u, v) in [
                        (qx0, qz0, ua, va),
                        (qx1, qz0, ub, va),
                        (qx0, qz1, ua, vb),
                        (qx1, qz1, ub, vb),
                    ] {
                        m.positions.push([x, GRID_PLANE_Y, z]);
                        m.uvs.push([u, v]);
                        m.cba_tsb.push([GROUND_CBA, GROUND_TSB]);
                        m.normals.push([0.0, -1.0, 0.0]); // PSX up = -y
                        // Neutral modulation: the grid quads draw the raw
                        // tile texel. (NB an earlier builder pushed NO
                        // colours at all, so its upload failed the
                        // attribute-length check and the grid never drew -
                        // the "sky-blue floor" was the bare battle clear
                        // colour showing through.)
                        m.colors
                            .push([legaia_tmd::legaia_prims::MODULATION_NEUTRAL; 3]);
                    }
                    m.indices
                        .extend([base, base + 2, base + 1, base + 1, base + 2, base + 3]);
                }
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_one_is_dropped_from_the_draw_list() {
        assert_eq!(drawn_object_indices(4), vec![0, 2, 3]);
        assert_eq!(drawn_object_indices(2), vec![0]);
        assert_eq!(drawn_object_indices(1), vec![0]);
        assert!(drawn_object_indices(0).is_empty());
    }

    #[test]
    fn the_gated_arm_keeps_every_object() {
        assert_eq!(drawn_object_indices_gated(4, true), vec![0, 1, 2, 3]);
        assert_eq!(drawn_object_indices_gated(2, true), vec![0, 1]);
        assert!(drawn_object_indices_gated(0, true).is_empty());
        // Default arm agrees with the convenience wrapper.
        for n in 0..6 {
            assert_eq!(
                drawn_object_indices_gated(n, false),
                drawn_object_indices(n)
            );
        }
    }

    #[test]
    fn half_turn_preserves_winding_and_mirror_reverses_it() {
        assert_eq!(SecondCopy::HalfTurn.scale(), [-1.0, 1.0, -1.0]);
        assert!(!SecondCopy::HalfTurn.flips_winding());
        assert_eq!(SecondCopy::MirrorX.scale(), [-1.0, 1.0, 1.0]);
        assert!(SecondCopy::MirrorX.flips_winding());
        // Determinant sign is what `flips_winding` reports.
        for c in [SecondCopy::HalfTurn, SecondCopy::MirrorX] {
            let s = c.scale();
            assert_eq!(c.flips_winding(), s[0] * s[1] * s[2] < 0.0);
        }
    }

    #[test]
    fn runtime_id_and_prot_index_round_trip() {
        assert_eq!(runtime_stage_id(88), Some(85));
        assert_eq!(runtime_stage_id(7), Some(4));
        assert_eq!(runtime_stage_id(2), None);
        assert_eq!(prot_index_for_runtime_id(85), 88);
    }

    #[test]
    fn table_parse_stops_at_the_terminator() {
        let mut scus = vec![0u8; MIRROR_X_TABLE_SCUS_OFFSET + 16];
        scus[MIRROR_X_TABLE_SCUS_OFFSET..MIRROR_X_TABLE_SCUS_OFFSET + 8]
            .copy_from_slice(&[0x60, 0x00, 0x04, 0x00, 0x00, 0x00, 0xFF, 0xFF]);
        let t = MirrorXTable::from_scus(&scus).expect("table");
        assert_eq!(t.ids(), &[96, 4]);
        assert!(t.contains_runtime_id(4));
        assert!(!t.contains_runtime_id(85));
        // 0007_town01 is on the list; 0088_map01 is not.
        assert_eq!(t.second_copy_for_prot_index(7), SecondCopy::MirrorX);
        assert_eq!(t.second_copy_for_prot_index(88), SecondCopy::HalfTurn);
    }

    #[test]
    fn a_truncated_or_unterminated_table_is_rejected() {
        assert!(MirrorXTable::from_scus(&[0u8; 16]).is_none());
        let scus = vec![0x11u8; MIRROR_X_TABLE_SCUS_OFFSET + 4096];
        assert!(MirrorXTable::from_scus(&scus).is_none());
    }
}

#[cfg(test)]
mod ground_grid_tests {
    use super::*;

    #[test]
    fn sub_tiles_walk_the_two_by_two_block_in_row_major_order() {
        assert_eq!(ground_sub_tile_uv(0, 0), (0xC0, 0xC0, 0xDF, 0xDF));
        assert_eq!(ground_sub_tile_uv(0, 1), (0xE0, 0xC0, 0xFF, 0xDF));
        assert_eq!(ground_sub_tile_uv(1, 0), (0xC0, 0xE0, 0xDF, 0xFF));
        assert_eq!(ground_sub_tile_uv(1, 1), (0xE0, 0xE0, 0xFF, 0xFF));
    }

    #[test]
    fn every_sub_tile_is_thirty_two_texels_square() {
        for (u0, v0, u1, v1) in GROUND_SUB_TILE_UVS {
            assert_eq!(u1 - u0, GROUND_SUB_TILE - 1);
            assert_eq!(v1 - v0, GROUND_SUB_TILE - 1);
        }
        // ...and together they tile the (192..=255)^2 window exactly.
        let lo = GROUND_SUB_TILE_UVS.iter().map(|t| t.0).min().unwrap();
        let hi = GROUND_SUB_TILE_UVS.iter().map(|t| t.2).max().unwrap();
        assert_eq!((lo, hi), (GROUND_UV_BASE, 0xFF));
    }

    #[test]
    fn grid_origin_is_biased_one_cell_toward_negative_z() {
        // 28 x 28 is the live grid: x spans [-7168, +7168] but z is pulled a
        // whole cell back, to [-7680, +6656].
        assert_eq!(grid_origin(28, 28), (-7168, -7680));
        let (x0, z0) = grid_origin(28, 28);
        assert_eq!(x0 - z0, GRID_CELL_PITCH);
    }

    #[test]
    fn visibility_brackets_the_view_z_range() {
        assert_eq!(classify_cell(-0x200), CellVisibility::Behind);
        assert_eq!(classify_cell(-0x201), CellVisibility::Behind);
        assert_eq!(classify_cell(-0x1FF), CellVisibility::Visible);
        assert_eq!(classify_cell(0), CellVisibility::Visible);
        assert_eq!(classify_cell(0x6500), CellVisibility::Visible);
        assert_eq!(classify_cell(0x6501), CellVisibility::TooFar);
    }

    #[test]
    fn offscreen_reject_needs_all_four_corners_past_one_edge() {
        assert!(cell_offscreen([(10, -5), (40, -5), (10, -30), (40, -30)]));
        assert!(cell_offscreen([(400, 10), (420, 10), (400, 40), (420, 40)]));
        // Straddling the top edge is kept - three corners off is not enough.
        assert!(!cell_offscreen([(10, -5), (40, -5), (10, 20), (40, -5)]));
        assert!(!cell_offscreen([(10, 10), (40, 10), (10, 40), (40, 40)]));
        // Spanning the whole display is kept: no single edge has all four.
        assert!(!cell_offscreen([
            (-10, -10),
            (400, -10),
            (-10, 300),
            (400, 300)
        ]));
    }

    /// Retail emits `2 x 2` quads per cell, so a 28x28 grid is 3136 quads -
    /// four times what a one-quad-per-cell builder produces.
    #[test]
    fn emits_four_quads_per_cell() {
        let m = build_ground_grid();
        let cells = (GRID_CELLS * GRID_CELLS) as usize;
        let quads = cells * 4;
        assert_eq!(m.positions.len(), quads * 4);
        assert_eq!(m.indices.len(), quads * 6);
    }

    /// Every vertex attribute stream has to stay the same length or the
    /// upload's attribute-length check rejects the mesh and the grid silently
    /// does not draw.
    #[test]
    fn attribute_streams_stay_parallel() {
        let m = build_ground_grid();
        let n = m.positions.len();
        assert_eq!(m.uvs.len(), n);
        assert_eq!(m.cba_tsb.len(), n);
        assert_eq!(m.normals.len(), n);
        assert_eq!(m.colors.len(), n);
        assert!(n > 0);
    }

    #[test]
    fn lies_flat_over_the_retail_extent() {
        let m = build_ground_grid();
        assert!(m.positions.iter().all(|p| p[1] == GRID_PLANE_Y));
        let (x0, z0) = grid_origin(GRID_CELLS, GRID_CELLS);
        let span = (GRID_CELLS * GRID_CELL_PITCH) as f32;
        let min_x = m.positions.iter().map(|p| p[0]).fold(f32::MAX, f32::min);
        let max_x = m.positions.iter().map(|p| p[0]).fold(f32::MIN, f32::max);
        let min_z = m.positions.iter().map(|p| p[2]).fold(f32::MAX, f32::min);
        let max_z = m.positions.iter().map(|p| p[2]).fold(f32::MIN, f32::max);
        assert_eq!((min_x, max_x), (x0 as f32, x0 as f32 + span));
        assert_eq!((min_z, max_z), (z0 as f32, z0 as f32 + span));
    }

    /// Every quad samples one of the four table sub-tiles with its corners in
    /// retail's order - no mirrored corner ever reaches the UVs, because the
    /// emitter has no mirror and no roll.
    #[test]
    fn uvs_are_unmirrored_table_sub_tiles() {
        let m = build_ground_grid();
        for q in m.uvs.chunks_exact(4) {
            let (u0, v0) = (q[0][0], q[0][1]);
            let (u1, v1) = (q[3][0], q[3][1]);
            assert!(
                GROUND_SUB_TILE_UVS.contains(&(u0, v0, u1, v1)),
                "quad UV block {:?} is not one of the four retail sub-tiles",
                (u0, v0, u1, v1)
            );
            assert_eq!((q[1][0], q[1][1]), (u1, v0));
            assert_eq!((q[2][0], q[2][1]), (u0, v1));
        }
    }

    /// All four sub-tiles are used, each exactly a quarter of the time - the
    /// deterministic `sub_row * 2 + sub_col` walk, not a hash.
    #[test]
    fn all_four_sub_tiles_appear_equally() {
        let m = build_ground_grid();
        let mut hits = [0usize; 4];
        for q in m.uvs.chunks_exact(4) {
            let key = (q[0][0], q[0][1], q[3][0], q[3][1]);
            let i = GROUND_SUB_TILE_UVS.iter().position(|t| *t == key).unwrap();
            hits[i] += 1;
        }
        let cells = (GRID_CELLS * GRID_CELLS) as usize;
        assert_eq!(hits, [cells; 4]);
    }

    #[test]
    fn every_quad_uses_the_pinned_page_and_clut() {
        let m = build_ground_grid();
        assert!(m.cba_tsb.iter().all(|c| *c == [GROUND_CBA, GROUND_TSB]));
    }
}

#[cfg(test)]
mod ground_plane_tests {
    use super::*;

    /// The grid sits one unit away from the viewer, not on the backdrop
    /// ring's own plane: PSX up is `-y`, so "away" is `+y`, and the sign
    /// matters - the wrong one puts the grid in FRONT of the ring and
    /// reverses retail's paint order.
    #[test]
    fn grid_plane_is_pushed_away_from_the_viewer() {
        let m = build_ground_grid();
        let y = m.positions[0][1];
        // PSX up is -y, so "away from the viewer" is +y. The wrong sign puts
        // the grid in FRONT of the backdrop's ground ring and reverses
        // retail's paint order.
        assert!(y > 0.0, "grid plane {y} should push to +y");
        // ...and far below visibility against the cell pitch.
        assert!(
            y < GRID_CELL_PITCH as f32 / 100.0,
            "grid plane {y} too large"
        );
        assert!(m.positions.iter().all(|p| p[1] == y), "the grid stays flat");
    }
}
