//! Texture cells, CLUTs and half-extents of the cabinet's billboards:
//! payline medallions, lamps, reel pedestals and the marquee frame.

// ---------------------------------------------------------------------------
// Sprite cells (the rects FUN_801d08e4 writes into its prims)
// ---------------------------------------------------------------------------

/// Art-pack page of the payline medallions (texpage `0x0C`).
pub const MEDALLION_PAGE: usize = 0;
/// Medallion cell `(u, v, w, h)`: `FUN_801d08e4`'s second loop draws 32x32 at
/// `uv (0xA8, 0x80)` with CLUT `0x7A80 + id`, `id` being the record's own `pad`
/// field - so the five medallions are one cell of artwork recoloured.
pub const MEDALLION_CELL: (u8, u8, u8, u8) = (0xA8, 0x80, 32, 32);
/// CLUT base of the medallions.
pub const MEDALLION_CLUT_BASE: u16 = 0x7A80;
/// View-space half-extent of a medallion billboard (`0x1A0 x 0xD0`).
pub const MEDALLION_HALF: (i32, i32) = (0x1A0, 0xD0);

/// Art-pack page of the payline lamps (texpage `0x1C`).
pub const LAMP_PAGE: usize = 2;
/// Unlit lamp cell.
pub const LAMP_CELL_UNLIT: (u8, u8, u8, u8) = (0x10, 0xE0, 16, 16);
/// Lit lamp cell (the winning payline's).
pub const LAMP_CELL_LIT: (u8, u8, u8, u8) = (0x00, 0xE0, 16, 16);
/// The lamps' fixed CLUT.
pub const LAMP_CLUT: u16 = 0x7B09;
/// View-space half-extent of a lamp billboard (`0xB4 x 0xA0`).
pub const LAMP_HALF: (i32, i32) = (0xB4, 0xA0);

/// Art-pack page of the reel-stop pedestals.
pub const PEDESTAL_PAGE: usize = 2;
/// Pedestal cell size (32x32).
pub const PEDESTAL_SIZE: u8 = 32;
/// `V` of pedestal `r`'s cell: `PEDESTAL_V0 + r * PEDESTAL_V_STEP`. Both
/// cells sit on this row - the open branch of `FUN_801d08e4` overrides **only
/// the `U`s**, so a pedestal slides along its own row rather than changing row.
pub const PEDESTAL_V0: u8 = 0x80;
/// Row step of the pedestal cell, per reel.
pub const PEDESTAL_V_STEP: u8 = 0x20;
/// `U` of the pedestal cell while its stop is **not** open: before a spin,
/// and once that reel's stop has been taken.
pub const PEDESTAL_U_IDLE: u8 = 0x60;
/// `U` of the pedestal cell while its stop is open.
pub const PEDESTAL_U_OPEN: u8 = 0x00;
/// CLUT base of the idle cell (`+ reel`).
pub const PEDESTAL_CLUT_IDLE: u16 = 0x7B03;
/// CLUT base of the open cell (`+ reel`).
pub const PEDESTAL_CLUT_OPEN: u16 = 0x7B06;

/// The cell pedestal `reel` draws, as `(u, v, w, h)`.
///
/// `open` is the reel's "stop still open" flag `DAT_801d3d00[reel]`: the bet
/// charge sets all three to `1` (`0x801CF4DC..0x801CF4E4`), a Stop press on
/// that reel clears it (`FUN_801d2114`: `DAT_801d3d00[reel] = 0`), and the
/// state-3 Stop tests accept a press only while it is set
/// (`0x801CF724`). So the swapped cell (`u = 0`, CLUT `0x7B06 + reel`) is the
/// "press now" pedestal of a still-spinning reel, and a taken stop returns to
/// the idle cell.
pub fn pedestal_cell(reel: usize, open: bool) -> (u8, u8, u8, u8) {
    let u = if open {
        PEDESTAL_U_OPEN
    } else {
        PEDESTAL_U_IDLE
    };
    (
        u,
        PEDESTAL_V0 + reel as u8 * PEDESTAL_V_STEP,
        PEDESTAL_SIZE,
        PEDESTAL_SIZE,
    )
}
/// View-space half-extent of a pedestal billboard (`0x230 x 0x120`).
pub const PEDESTAL_HALF: (i32, i32) = (0x230, 0x120);
/// Model-space `x` of pedestal 0.
pub const PEDESTAL_X0: i32 = -0x180;
/// Model-space `x` step between pedestals.
pub const PEDESTAL_X_STEP: i32 = 0x180;
/// Model-space `y` of the pedestal row.
pub const PEDESTAL_Y: i32 = 0x1E0;
/// The plane all the machine's glass furniture sits on.
pub const GLASS_Z: i32 = -800;

/// Art-pack page of the marquee panel + mascots.
pub const MARQUEE_PAGE: usize = 2;
/// Marquee CLUT base (`0x7B00 + record.clut_off`).
pub const MARQUEE_CLUT_BASE: u16 = 0x7B00;
