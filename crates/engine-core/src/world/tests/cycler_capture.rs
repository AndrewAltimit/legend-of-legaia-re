//! A CLUT-cell cycler captures its rect as the GPU holds it: a recolour
//! another part uploaded there is what op `0x2C`'s `StoreImage` reads.

use crate::world::World;
use crate::world::ambient::{AmbientPart, CyclerCapture};
use legaia_engine_field::clut_cell_fx::ClutCellFx;

const RECT: (u16, u16, u16, u16) = (0, 476, 320, 2);

fn cycler(fx: Option<ClutCellFx>, texels: Option<Vec<u16>>, due: bool) -> AmbientPart {
    let mut state = legaia_engine_vm::move_vm::ActorState::new();
    state.move_submode = 3;
    state.keyframe_desc = [RECT.0, RECT.1, RECT.2, RECT.3];
    AmbientPart {
        record_off: 0,
        model_sel: 0x4000,
        reserved: 0,
        buf: Vec::new(),
        state,
        finished: fx.is_some(),
        cell_fx: fx,
        cycler: CyclerCapture {
            lit: 0,
            texels,
            due,
        },
        scroll_fx: Vec::new(),
        prev_morph_weights: Vec::new(),
    }
}

fn grey() -> ClutCellFx {
    ClutCellFx {
        rect: RECT,
        h_add: 0,
        s_add: -1024,
        v_add: 0,
        mode: 0,
        white: 0,
    }
}

/// `opstati`: the parent greys the strip and halts, the child it spawns
/// captures the grey rows, not the scene's colours, and the strip a `320`
/// texels wide is written at all.
#[test]
fn a_child_captures_the_rows_its_parent_greyed() {
    let mut w = World::new();
    w.toggles.reduce_flashing = false;
    let mut vram = psx_tim::Vram::new();
    // A warm brown texel (r 13, g 10, b 9) across the strip.
    let brown = 13u16 | 10 << 5 | 9 << 10;
    let row: Vec<u8> = (0..640).flat_map(|_| brown.to_le_bytes()).collect();
    vram.write_block(RECT.0, RECT.1, RECT.2, RECT.3, &row);
    let src = vec![brown; 640];
    w.ambient
        .fx
        .push(cycler(Some(grey()), Some(src.clone()), false));
    w.ambient.fx.push(cycler(None, None, true));
    assert!(w.step_ambient_fx(&mut vram));
    let greyed = legaia_engine_field::clut_cell_fx::apply_hsv_cell(&src, &grey());
    assert_ne!(greyed, src);
    assert_eq!(w.ambient.fx[1].cycler.texels.as_ref(), Some(&greyed));
    assert_eq!(w.ambient.fx[1].cycler.lit, 640);
    assert_eq!(vram.pixel(319, 477), greyed[639]);
}
