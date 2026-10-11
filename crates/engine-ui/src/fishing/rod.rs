//! The fishing **rod** as screen-space primitives.
//!
//! Retail draws the first-person rod from its actor `FUN_801D1C5C`: object 0
//! of the rod model, bent by the VDF morph, posed in view space and handed to
//! the per-primitive dispatcher `FUN_80043390`, whose bank-0 handlers emit
//! untextured `POLY_F3` / `POLY_F4` / `POLY_G3` / `POLY_G4` packets into the
//! ordering table. The engine does the projection, the cull and the bucket
//! (`legaia_engine_core::fishing_actors::rod_faces`, reached through
//! `PondSession::rod_faces`); this module turns each face into the prim set's
//! flat kind with per-corner Gouraud colours.
//!
//! The native window and the browser play page both draw the rod through
//! [`fishing_rod_prim`], ahead of the line in the same pass; the minigames
//! page fills the same faces on its canvas, in the order
//! [`crate::screen_prim::order_primitives`] gives these prims.

use crate::screen_prim::{FlatQuad, ScreenPrim};

/// One rod face as an opaque Gouraud quad. `xy` / `rgb` are the four
/// corners in `v0..v3` order (a triangle repeats its third corner, which
/// the quad's `(v1, v2, v3)` half then degenerates), `ot` the face's
/// ordering-table bucket.
pub fn fishing_rod_prim(xy: [(i16, i16); 4], rgb: [[u8; 3]; 4], ot: u32) -> ScreenPrim {
    let g = rgb.map(|c| [c[0], c[1], c[2], 0xFF]);
    ScreenPrim::Flat(FlatQuad {
        xy,
        color: g[0],
        gouraud: Some(g),
        semi_transparent: false,
        abr_mode: 0,
        ot_index: ot,
        depth: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_face_is_an_opaque_gouraud_quad_at_its_bucket() {
        let rgb = [[128, 40, 40], [255, 80, 80], [128, 40, 40], [255, 80, 80]];
        let ScreenPrim::Flat(q) = fishing_rod_prim([(1, 2), (3, 4), (5, 6), (7, 8)], rgb, 19)
        else {
            panic!("a flat quad");
        };
        assert_eq!(q.xy, [(1, 2), (3, 4), (5, 6), (7, 8)]);
        assert_eq!(q.gouraud.unwrap()[1], [255, 80, 80, 0xFF]);
        assert!(!q.semi_transparent, "bank 0 is opaque");
        assert_eq!(q.ot_index, 19);
        assert!(q.depth.is_none(), "composites in OT order, like the line");
    }
}
