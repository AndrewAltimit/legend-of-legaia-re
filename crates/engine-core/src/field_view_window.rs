//! The camera's **visible-tile crop**: which `.MAP` cells retail's field
//! render library draws this frame, given the camera focus, the visible tile
//! window `0x1F8003E8..EB` and the walk-region box `0x1F800384..87`.
//!
//! Retail never draws a field scene's whole map. The slot-B render library
//! (PROT 0900) walks only the cells around the camera, in two passes that share
//! one rectangle:
//!
//! - the **decoration pass** `FUN_801F7088` - the `0x2000`-gated per-cell pack
//!   meshes (the port's terrain [`EnvDraw`](crate::field_env::EnvDraw) list);
//! - the **ground emitters** `FUN_801F6D48` / `FUN_801F69EC` it calls at the
//!   end (one or the other on `_DAT_8007BB4C`, same loop) - the `0x1000`-gated
//!   textured ground quads (the port's walk-ground heightfield).
//!
//! [`view_cells`] is the prologue of `FUN_801F7088` that both passes key on;
//! [`ViewCells::decoration_visible`] and [`ViewCells::ground_visible`] are the
//! two passes' per-cell gates. Everything is read from the disassembly
//! (`overlay_dance_801f7088.txt`, `overlay_dance_801f6d48.txt`,
//! `overlay_dance_801f69ec.txt`; the dance dump is the same 0900 image).
//!
//! ```text
//! // 0x801F722C..0x801F72D8: first cell, focus stored negated
//! col = ceil(-(focus_x - (E8 << 7)) / 128)     // (0x7F - s) >> 7
//! row = ceil(-(focus_z - (E9 << 7)) / 128)
//! // 0x801F7304..0x801F7408: clamp against the region box, written back
//! if col < R0            { E8 += R0 - col; col = R0 }
//! if col - E8 + EA > R2  { EA -= (col - E8 + EA) - R2 }
//! zlo = min(R1 + 2, 0x7E); if row < zlo { E9 += zlo - row; row = zlo }
//! zhi = min(R3 + 1, 0x7E); if row - E9 + EB > zhi { EB -= ... }
//! ```
//!
//! The write-back is **scoped to the pass**: the prologue saves the six
//! scratchpad bytes (`0x384`, `0x385`, `E8..EB`) to `0x801F9064..78`
//! (`0x801F7178..0x801F71A4`) and the epilogue stores them back
//! (`0x801F7A00..0x801F7A5C`) after the ground emitter has run. So the clamped
//! window is what the ground emitter reads, and every reader after the pass -
//! the actor cull, the ambient emitter, the next frame's prologue - sees the
//! window the camera region or op `0x46` wrote. The port keeps that shape by
//! never writing [`ViewCells::window`] back to the camera.
//!
//! Hosts reach this through [`field_view_cells`], which adds the port's policy:
//! the crop only holds at retail's own framing (see its docs).

use crate::camera::{Camera, CameraDistance};
use crate::world::field_npc_cull::FieldCullView;
use crate::world::{SceneMode, World};

/// The cell rectangle one frame of `FUN_801F7088` walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ViewCells {
    /// The clamped first cell `(col, row)` - scratchpad `0x1F8002BC` /
    /// `0x1F8002C0` after the clamp, which the ground emitter takes as its
    /// start column and (minus one) start row.
    pub first: [i32; 2],
    /// The visible tile window as the pass writes it back for its own
    /// duration (`[E8, E9, EA, EB]`, signed), clipped to the region box.
    pub window: [i8; 4],
    /// The focus's sub-tile remainders `& 0x7F` (`0x1F80030C` / `0x1F800310`),
    /// the emit origin's fractional part. Carried for completeness; the
    /// per-cell gates do not read it.
    pub sub: [i32; 2],
    /// The decoration pass's loop extents: the **pre-clamp** `(EA - E8) + 10`
    /// columns and `(EB - E9) + 10` rows (`sp+0x18` / `sp+0x20` + `0x0A`).
    pub span: [i32; 2],
    /// The walk-region box `[x_lo, z_lo, x_hi, z_hi]` the clamp read.
    pub region: [u8; 4],
}

/// `ceil(-s / 128)` exactly as `0x801F726C..0x801F72D4` computes it:
/// `(0x7F - s) >> 7` on both signs (the overflow fix-up for `-s + 0x7F < 0`
/// only matters at `i32::MIN`).
fn first_cell(s: i32) -> i32 {
    0x7Fi32.wrapping_sub(s) >> 7
}

/// The prologue of `FUN_801F7088`: the first cell, the region clamp and its
/// scoped write-back of the window, and the decoration loop's extents.
///
/// PORT: FUN_801F7088 (prologue `0x801F7088..0x801F74B0`: first cell, window clamp)
pub fn view_cells(view: &FieldCullView) -> ViewCells {
    let [e8, e9, ea, eb] = view.window;
    // 0x801F709C..0x801F70B4 / 0x801F7170: the loop extents come from the
    // bytes as they stand on entry, before the clamp below moves them.
    let span = [
        i32::from(ea) - i32::from(e8) + 10,
        i32::from(eb) - i32::from(e9) + 10,
    ];
    let [r0, r1, r2, r3] = view.attr_box.map(i32::from);
    let s_x = view.focus_stored[0].wrapping_sub(i32::from(e8) << 7);
    let s_z = view.focus_stored[1].wrapping_sub(i32::from(e9) << 7);
    let mut col = first_cell(s_x);
    let mut row = first_cell(s_z);
    let sub = [s_x & 0x7F, s_z & 0x7F];

    let mut w = view.window;
    // 0x801F7304: `lbu E8; addu; sb` - a byte store, re-read with `lb`.
    if col < r0 {
        w[0] = w[0].wrapping_add((r0 - col) as i8);
        col = r0;
    }
    let last = col - i32::from(w[0]) + i32::from(w[2]);
    if last > r2 {
        w[2] = w[2].wrapping_sub((last - r2) as i8);
    }
    let zlo = if r1 + 2 < 0x7F { r1 + 2 } else { 0x7E };
    if row < zlo {
        w[1] = w[1].wrapping_add((zlo - row) as i8);
        row = zlo;
    }
    let zhi = if r3 + 1 < 0x7F { r3 + 1 } else { 0x7E };
    let last = row - i32::from(w[1]) + i32::from(w[3]);
    if last > zhi {
        w[3] = w[3].wrapping_sub((last - zhi) as i8);
    }
    ViewCells {
        first: [col, row],
        window: w,
        sub,
        span,
        region: view.attr_box,
    }
}

impl ViewCells {
    /// The decoration pass's per-cell gate: whether `FUN_801F7088` reaches the
    /// cell `(col, row)` and lets a record with cull radius `radius`
    /// (`record[+0x1E]`) through to its draw.
    ///
    /// The walk starts one column left and three rows up of the clamped first
    /// cell (`0x801F7470` / `0x801F7484`) and runs the pre-clamp extents
    /// ([`Self::span`]). A cell must lie in the region box - `[R0, R2)` in X,
    /// `[R1 - 1, R3)` in Z (`0x801F74DC..0x801F7540`) - and then inside the
    /// clamped window widened by the radius (`0x801F7594..0x801F75D8`), with
    /// `i`, `j` the loop's column and row counters:
    ///
    /// ```text
    /// 1 - r < i < (EA - E8) + 1 + r
    ///   - r < j < (EB - E9) + 1 + r + 1
    /// ```
    ///
    /// The grid-word (`0x2000`) and placed-flag (`record[+0x12] & 4`) gates the
    /// same loop applies are properties of the draw list, not of the frame -
    /// the port's terrain list is built from exactly those cells.
    ///
    /// PORT: FUN_801F7088 (per-cell loop gates `0x801F74B4..0x801F75D8`, `0x801F78D4..0x801F7900`)
    pub fn decoration_visible(&self, col: i32, row: i32, radius: u8) -> bool {
        let i = col - (self.first[0] - 1);
        let j = row - (self.first[1] - 3);
        if !(0..self.span[0]).contains(&i) || !(0..self.span[1]).contains(&j) {
            return false;
        }
        let [r0, r1, r2, r3] = self.region.map(i32::from);
        if col < r0 || col >= r2 || row < r1 - 1 || row >= r3 {
            return false;
        }
        let r = i32::from(radius);
        let cols = i32::from(self.window[2]) - i32::from(self.window[0]) + 1;
        let rows = i32::from(self.window[3]) - i32::from(self.window[1]) + 1;
        1 - r < i && i < cols + r && -r < j && j < rows + r + 1
    }

    /// The ground emitters' per-cell gate: whether `FUN_801F6D48` (or its
    /// `_DAT_8007BB4C` twin `FUN_801F69EC`) visits cell `(col, row)`.
    ///
    /// The caller hands it the clamped first column and the clamped first row
    /// minus one (`0x801F79E8..0x801F79F8`); it walks `EA - E8` columns by
    /// `EB - E9` rows of the window *as written back*. Both loops are
    /// `do`-`while` (`bgtz` after the decrement, `0x801F7038` / `0x801F7054`),
    /// so a non-positive count still runs once. The row wraps `& 0x7F`; the
    /// column does not. No region test: the clamp already confined the window.
    ///
    /// PORT: FUN_801F6D48 (cell loop `0x801F6D6C..0x801F6DD8`, `0x801F7028..0x801F7058`)
    // REF: FUN_801F69EC (the same loop, the `_DAT_8007BB4C != 0` emitter)
    pub fn ground_visible(&self, col: i32, row: i32) -> bool {
        let cols = (i32::from(self.window[2]) - i32::from(self.window[0])).max(1);
        let rows = (i32::from(self.window[3]) - i32::from(self.window[1])).max(1);
        let c0 = self.first[0];
        let r0 = (self.first[1] - 1) & 0x7F;
        (c0..c0 + cols).contains(&col) && (row - r0).rem_euclid(0x80) < rows
    }

    /// A value that changes whenever either gate's answer can - for a host
    /// that caches a cropped index buffer or a per-draw mask and rebuilds it
    /// only when the rectangle moves. Never `0` (a host's "no crop" sentinel).
    pub fn stamp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut eat = |v: i32| {
            for b in v.to_le_bytes() {
                h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
            }
        };
        eat(self.first[0]);
        eat(self.first[1]);
        eat(i32::from_le_bytes(self.window.map(|b| b as u8)));
        eat(self.span[0]);
        eat(self.span[1]);
        eat(i32::from_le_bytes(self.region));
        h.max(1)
    }
}

/// Whether a camera frames the field the way retail's own view does: the
/// retail distance preset and every user follow knob at identity. Retail sized
/// each scene's visible-tile window for that frustum, so the crop only makes
/// sense under it.
pub fn framing_is_retail(camera: &Camera) -> bool {
    camera.distance == CameraDistance::Retail
        && camera.manual_orbit == 0.0
        && camera.manual_tilt == 0.0
        && camera.manual_zoom == 1.0
}

/// This frame's cell rectangle, or `None` when the host should draw the whole
/// map - the one entry point both play hosts ask.
///
/// `None` when:
///
/// - the crop knob is off ([`crate::world::WorldToggles::view_window_crop`],
///   from [`crate::options::OptionsState::retail_view_window`]);
/// - `retail_framing` is false - the host passes [`framing_is_retail`] and
///   clears it under its own debug camera. The window is authored for
///   retail's frustum, so under a wider or re-aimed view it would open black
///   edges the player can see; the crop is the faithful mode's, not a
///   constraint on the enhanced views;
/// - the world is not in [`SceneMode::Field`]: the kingdom overworld is drawn
///   by the PROT 0901 library, not by the pair this module ports, and the
///   other modes draw no `.MAP` cells;
/// - a cutscene timeline owns the camera: the published view is the zone
///   follow camera's focus, while the scripted shot the hosts draw composes
///   from the op-`0x45` parameters, so a crop against the follow focus would
///   cut scenery out of the shot on screen;
/// - no camera view is published yet ([`crate::world::FieldNpcState::cull_view`]
///   is `None` until the zone follow camera composes);
/// - the focus tile lies outside the region box. Retail latches the box at
///   the camera's own re-centre tile (`FUN_80017DD4`), so the two agree by
///   construction; the port latches it from the player actor's tile, and a
///   scene entered without a seat can leave the follow focus somewhere the
///   actor is not. Cropping then would clamp the whole window away and draw
///   no ground at all, so the host draws the map whole instead.
pub fn field_view_cells(world: &World, retail_framing: bool) -> Option<ViewCells> {
    if !world.toggles.view_window_crop
        || !retail_framing
        || world.mode != SceneMode::Field
        || world.cutscene_timeline_active()
    {
        return None;
    }
    let view = world.npcs.cull_view.as_ref()?;
    let [r0, r1, r2, r3] = view.attr_box.map(i32::from);
    let (tx, tz) = (
        view.focus_stored[0].wrapping_neg() >> 7,
        view.focus_stored[1].wrapping_neg() >> 7,
    );
    if tx < r0 || tx >= r2 || tz < r1 || tz >= r3 {
        return None;
    }
    Some(view_cells(view))
}

/// What the decoration gate needs to know about one terrain draw: its grid
/// cell and its record's cull radius. Hosts that bake their draws into
/// `(mesh, matrix)` pairs keep one of these beside each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CellKey {
    /// [`crate::field_env::EnvDraw::cell`].
    pub cell: (u8, u8),
    /// [`crate::field_env::EnvDraw::cull_radius`].
    pub cull_radius: u8,
}

impl CellKey {
    /// The key of one resolved draw.
    pub fn of_draw(d: &crate::field_env::EnvDraw) -> Self {
        CellKey {
            cell: d.cell,
            cull_radius: d.cull_radius,
        }
    }
}

/// The terrain-list gate a host applies per draw: `true` = draw it. With no
/// crop this frame (`cells = None`) every draw is drawn.
pub fn terrain_draw_visible(cells: Option<&ViewCells>, key: CellKey) -> bool {
    cells.is_none_or(|c| {
        c.decoration_visible(
            i32::from(key.cell.0),
            i32::from(key.cell.1),
            key.cull_radius,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A town01-shaped view from the retail actor-cull capture: focus tile
    /// `(32, 92)`, region `[20, 77, 44, 108]`, window `(-8, -6, 8, 12)`.
    fn town01() -> FieldCullView {
        FieldCullView {
            focus_stored: [-4160, -11840],
            attr_box: [20, 77, 44, 108],
            window: [-8, -6, 8, 12],
        }
    }

    #[test]
    fn first_cell_rounds_up_on_both_signs() {
        assert_eq!(first_cell(0), 0);
        assert_eq!(first_cell(-1), 1);
        assert_eq!(first_cell(-128), 1);
        assert_eq!(first_cell(-129), 2);
        assert_eq!(first_cell(1), 0);
        assert_eq!(first_cell(128), -1);
        assert_eq!(first_cell(129), -1);
    }

    #[test]
    fn an_unclamped_window_starts_at_focus_plus_the_near_edge() {
        let c = view_cells(&town01());
        // -(-4160 - (-8 << 7)) = 3136 -> ceil(3136/128) = 25 = 32.5 - 8 rounded up.
        assert_eq!(c.first, [25, 87]);
        assert_eq!(c.window, [-8, -6, 8, 12]);
        assert_eq!(c.span, [26, 28]);
        assert_eq!(c.sub, [(-4160 + 1024) & 0x7F, (-11840 + 768) & 0x7F]);
    }

    #[test]
    fn the_window_is_clamped_to_the_region_and_written_back() {
        // Focus near the region's west + north edges: the near edges move in.
        let v = FieldCullView {
            focus_stored: [-(22 * 128), -(80 * 128)],
            attr_box: [20, 77, 44, 108],
            window: [-8, -6, 8, 12],
        };
        let c = view_cells(&v);
        // X: first = 22 - 8 = 14 < 20 -> E8 grows by 6 to -2, first = 20.
        assert_eq!(c.first[0], 20);
        assert_eq!(c.window[0], -2);
        // Z: first = 80 - 6 = 74 < 77 + 2 -> E9 grows by 5 to -1, first = 79.
        assert_eq!(c.first[1], 79);
        assert_eq!(c.window[1], -1);
        // Far edges inside the box: untouched.
        assert_eq!(c.window[2], 8);
        assert_eq!(c.window[3], 12);
        // The input the camera owns is not written: the write-back is scoped
        // to the pass (retail restores the bytes in the epilogue).
        assert_eq!(v.window, [-8, -6, 8, 12]);
        // The decoration loop still runs the PRE-clamp extents.
        assert_eq!(c.span, [26, 28]);
    }

    #[test]
    fn the_far_edges_shrink_to_the_region() {
        let v = FieldCullView {
            focus_stored: [-(40 * 128), -(104 * 128)],
            attr_box: [20, 77, 44, 108],
            window: [-8, -6, 8, 12],
        };
        let c = view_cells(&v);
        // X: last = 32 - (-8) + 8 = 48 > 44 -> EA -= 4.
        assert_eq!(c.first[0], 32);
        assert_eq!(c.window[2], 4);
        // Z: last = 98 + 6 + 12 = 116 > 108 + 1 -> EB -= 7.
        assert_eq!(c.first[1], 98);
        assert_eq!(c.window[3], 5);
    }

    #[test]
    fn the_z_bounds_cap_at_0x7e() {
        let v = FieldCullView {
            focus_stored: [-(64 * 128), -(126 * 128)],
            attr_box: [0, 125, 127, 127],
            window: [-8, -6, 8, 12],
        };
        let c = view_cells(&v);
        // zlo = min(125 + 2, 0x7E) = 126: first row 120 -> 126, E9 += 6.
        assert_eq!(c.first[1], 126);
        assert_eq!(c.window[1], 0);
        // zhi = min(127 + 1, 0x7E) = 126: last = 126 - 0 + 12 = 138 -> EB -= 12.
        assert_eq!(c.window[3], 0);
    }

    #[test]
    fn the_ground_walks_the_written_back_window_from_one_row_up() {
        let c = view_cells(&town01());
        // Columns first..first + (EA - E8), rows first - 1 ..+ (EB - E9).
        assert!(c.ground_visible(25, 86));
        assert!(c.ground_visible(25 + 15, 86 + 17));
        assert!(!c.ground_visible(24, 90));
        assert!(!c.ground_visible(25 + 16, 90));
        assert!(!c.ground_visible(30, 85));
        assert!(!c.ground_visible(30, 86 + 18));
    }

    #[test]
    fn a_collapsed_ground_window_still_draws_one_cell() {
        let mut c = view_cells(&town01());
        c.window = [3, 3, 3, 3];
        assert!(c.ground_visible(c.first[0], c.first[1] - 1));
        assert!(!c.ground_visible(c.first[0] + 1, c.first[1] - 1));
    }

    #[test]
    fn the_ground_row_wraps_but_the_column_does_not() {
        let mut c = view_cells(&town01());
        c.first = [10, 0];
        c.window = [0, 0, 4, 4];
        // Start row is (0 - 1) & 0x7F = 127; rows 127, 0, 1, 2.
        assert!(c.ground_visible(10, 127));
        assert!(c.ground_visible(10, 2));
        assert!(!c.ground_visible(10, 3));
        assert!(!c.ground_visible(9, 0));
    }

    #[test]
    fn decorations_widen_by_their_cull_radius() {
        let c = view_cells(&town01());
        // i = col - 24, j = row - 84; cols = 17, rows = 19.
        // r = 0: 1 < i < 17 -> cols 26..=40; 0 < j < 20 -> rows 85..=103.
        assert!(!c.decoration_visible(25, 90, 0));
        assert!(c.decoration_visible(26, 90, 0));
        assert!(c.decoration_visible(40, 90, 0));
        assert!(!c.decoration_visible(41, 90, 0));
        assert!(!c.decoration_visible(30, 84, 0));
        assert!(c.decoration_visible(30, 85, 0));
        assert!(c.decoration_visible(30, 103, 0));
        assert!(!c.decoration_visible(30, 104, 0));
        // r = 2 opens two cells on every side.
        assert!(c.decoration_visible(24, 84, 2));
        assert!(
            !c.decoration_visible(24, 83, 2),
            "the loop starts three rows up"
        );
        assert!(c.decoration_visible(42, 105, 2));
        assert!(!c.decoration_visible(43, 90, 2));
    }

    #[test]
    fn decorations_stay_inside_the_loop_and_the_region() {
        let c = view_cells(&town01());
        // A huge radius cannot reach past the pre-clamp loop extent.
        assert!(!c.decoration_visible(24 + 26, 90, 60));
        assert!(!c.decoration_visible(23, 90, 60));
        // Nor outside the region box.
        let v = FieldCullView {
            focus_stored: [-(22 * 128), -(80 * 128)],
            attr_box: [20, 77, 44, 108],
            window: [-8, -6, 8, 12],
        };
        let c = view_cells(&v);
        assert!(!c.decoration_visible(19, 85, 60));
        assert!(c.decoration_visible(20, 85, 60));
        // Z lower bound is R1 - 1.
        assert!(c.decoration_visible(25, 76, 60));
        assert!(!c.decoration_visible(25, 75, 60));
    }

    #[test]
    fn the_stamp_tracks_the_rectangle() {
        let a = view_cells(&town01());
        let mut v = town01();
        assert_eq!(view_cells(&v).stamp(), a.stamp());
        // A sub-tile move inside one cell keeps the rectangle.
        v.focus_stored[0] -= 1;
        assert_eq!(view_cells(&v).first, a.first);
        v.focus_stored[0] -= 128;
        assert_ne!(view_cells(&v).stamp(), a.stamp());
        assert_ne!(a.stamp(), 0);
    }

    #[test]
    fn the_policy_needs_a_view_the_knob_and_retail_framing() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        assert!(field_view_cells(&world, true).is_none(), "no view yet");
        world.npcs.cull_view = Some(town01());
        assert_eq!(field_view_cells(&world, true), Some(view_cells(&town01())));
        assert!(field_view_cells(&world, false).is_none());
        world.mode = SceneMode::WorldMap;
        assert!(field_view_cells(&world, true).is_none());
        world.mode = SceneMode::Field;
        // A focus the latched region box does not hold: no crop.
        world.npcs.cull_view = Some(FieldCullView {
            focus_stored: [-(20 * 128), -(20 * 128)],
            ..town01()
        });
        assert!(field_view_cells(&world, true).is_none());
        world.npcs.cull_view = Some(town01());
        world.toggles.view_window_crop = false;
        assert!(field_view_cells(&world, true).is_none());
    }

    #[test]
    fn framing_is_retail_only_at_the_retail_preset_and_identity_knobs() {
        let mut cam = Camera::new();
        cam.distance = CameraDistance::Retail;
        assert!(framing_is_retail(&cam));
        cam.distance = CameraDistance::Far;
        assert!(!framing_is_retail(&cam));
        cam.distance = CameraDistance::Retail;
        cam.manual_zoom = 1.2;
        assert!(!framing_is_retail(&cam));
    }

    /// A door does not clear the window: the crop always has a rectangle to
    /// work from across a scene boundary, and the incoming scene's entry
    /// primer (`FUN_801DE37C`) re-stamps it - a window scripted wide in the
    /// scene the player left does not leak into the next one's crop.
    #[test]
    fn a_door_keeps_a_window_and_the_entry_primer_restamps_it() {
        let mut cam = Camera::new();
        cam.zone.view_window = [-12, -10, 12, 16];
        let wide = FieldCullView {
            window: cam.zone.view_window,
            ..town01()
        };
        cam.reset_globals_for_scene_entry();
        let (x0, z0, x1, z1) = crate::mode_entry_init::FIELD_DEFAULT_VIEW_WINDOW;
        assert_eq!(cam.zone.view_window, [x0, z0, x1, z1]);
        let entry = FieldCullView {
            window: cam.zone.view_window,
            ..town01()
        };
        let (a, b) = (view_cells(&wide), view_cells(&entry));
        assert_ne!(a, b);
        // The entry window still spans cells - never an empty crop.
        assert!(b.ground_visible(b.first[0], b.first[1] - 1));
    }
}
