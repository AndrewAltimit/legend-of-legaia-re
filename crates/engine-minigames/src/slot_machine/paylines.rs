//! The payline draw list: GP0 line primitives, their projection and ordering-table depth.

// --- payline draw list -----------------------------------------------------

/// GP0 command byte of a payline segment: `0x43` - a flat (non-gouraud),
/// **semi-transparent** two-point line.
pub const PAYLINE_GP0_CODE: u8 = 0x43;

/// Colour every unlit payline draws in - a neutral half-grey.
pub const PAYLINE_COLOR_IDLE: (u8, u8, u8) = (0x80, 0x80, 0x80);

/// Colour the lit payline draws in. Retail overwrites only the three
/// colour bytes of the already-assembled command word, so the `0x43` code
/// byte survives and the line stays semi-transparent.
pub const PAYLINE_COLOR_LIT: (u8, u8, u8) = (0xFF, 0xFF, 0x80);

/// One payline segment ready to draw: the two model-space endpoints the
/// caller projects, plus the resolved GPU packet fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaylinePrim {
    /// Payline index `0..5`. Doubles as the medallion / lamp index.
    pub index: usize,
    /// The segment's two endpoints, straight out of the geometry table.
    pub a: legaia_minigame_formats::minigame_slot_scene::Pos3,
    pub b: legaia_minigame_formats::minigame_slot_scene::Pos3,
    /// 24-bit modulation colour.
    pub color: (u8, u8, u8),
    /// Always [`PAYLINE_GP0_CODE`].
    pub code: u8,
    /// True for the line matching the winning-line index.
    pub lit: bool,
}

/// Build the five payline line-prims for one frame.
///
/// `winning_line` is retail's `DAT_801d3c8c`, compared for **equality**
/// against each line index - so a frame where that word still holds `0`
/// lights line 0, and only a value outside `0..5` leaves every line unlit.
/// The caller supplies the geometry table
/// ([`legaia_minigame_formats::minigame_slot_scene::SlotScene::paylines`], disc data
/// at `DAT_801d3680`); nothing here is hard-coded geometry.
///
/// Projection and ordering-table linkage stay caller-side, as they do for
/// the rest of the machine's 3D furniture: retail `RTPS`-projects each
/// endpoint on its own through `FUN_8003d368` and links the packet at
/// [`payline_ot_depth`] of the **second** endpoint's returned depth.
// REF: FUN_8003d368 (the SCUS RTPS wrapper each endpoint goes through; the
// browser pages run its equivalent caller-side, over the same projection)
// PORT: FUN_801d3380 (payline 3D line segments)
//
// Wired on all three hosts through one projection, [`projected_paylines`]:
// the minigames page (`slot_payline_prims_json`) and the play page
// (`play_mg_slot_payline_prims_json`) hand their page each prim with its
// projected endpoints, which the page strokes as a two-point line in the
// prim's colour, half-blended when the `0x43` code carries the
// semi-transparency bit; the native window draws the same segments as
// one-pixel flat quads (`legaia_engine_ui::slot_paylines`) off
// [`SlotMachine::payline_segments`]. The native window still has no model draw
// of the cabinet mesh (PROT 1200 descriptor 1) around them - see
// docs/subsystems/minigame-slot-machine.md, "The cabinet is a mesh".
pub fn payline_prims(
    paylines: &[legaia_minigame_formats::minigame_slot_scene::PayLine],
    winning_line: i32,
) -> Vec<PaylinePrim> {
    paylines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let lit = index as i32 == winning_line;
            PaylinePrim {
                index,
                a: line.a,
                b: line.b,
                color: if lit {
                    PAYLINE_COLOR_LIT
                } else {
                    PAYLINE_COLOR_IDLE
                },
                code: PAYLINE_GP0_CODE,
                lit,
            }
        })
        .collect()
}

/// One payline prim with both endpoints projected onto the retail 640x240
/// slot framebuffer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectedPayline {
    pub prim: PaylinePrim,
    /// Screen position of each endpoint.
    pub a: (f32, f32),
    pub b: (f32, f32),
}

/// The projection pass [`payline_prims`] leaves caller-side, run once for
/// every host: each endpoint goes through
/// [`legaia_minigame_formats::minigame_slot_scene::project`] - the machine's captured
/// projection (the stand-in for `FUN_8003D368`'s `RTPS` under the camera the
/// overlay installs) that the rest of the cabinet is drawn with.
///
/// REF: FUN_801d3380 (the two `FUN_8003D368` calls per segment)
pub fn projected_paylines(
    paylines: &[legaia_minigame_formats::minigame_slot_scene::PayLine],
    winning_line: i32,
) -> Vec<ProjectedPayline> {
    use legaia_minigame_formats::minigame_slot_scene::project;
    payline_prims(paylines, winning_line)
        .into_iter()
        .map(|prim| ProjectedPayline {
            prim,
            a: project(prim.a.x as i32, prim.a.y as i32, prim.a.z as i32),
            b: project(prim.b.x as i32, prim.b.y as i32, prim.b.z as i32),
        })
        .collect()
}

/// Ordering-table bucket for a payline packet: `(depth >> 2) >> ot_shift`,
/// with retail's round-toward-zero fixup (`depth + 3` before the shift when
/// negative) and `ot_shift` the frame context's `+0x90` byte.
pub fn payline_ot_depth(projected_depth: i32, ot_shift: u32) -> i32 {
    let biased = if projected_depth < 0 {
        projected_depth + 3
    } else {
        projected_depth
    };
    (biased >> 2) >> ot_shift
}
