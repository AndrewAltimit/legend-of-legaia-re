//! The photosensitivity guard over the ambient CLUT-cell cyclers
//! (`World::limit_flash` inside `World::step_ambient_fx`, gated by
//! `World::reduce_flashing`).
//!
//! koin3's dance-floor records full-swing their cells' `v_add`
//! `-256 <-> 0` on consecutive game ticks - a 15 Hz bright/black strobe.
//! With the guard ON (the default) the *applied* luminance channels slew
//! toward the simulated target instead of jumping; with it OFF the applied
//! values are the retail-exact targets. Hue passes through either way.

use super::*;

const RECT: (u16, u16, u16, u16) = (0x20, 0x1FB, 16, 1);

fn strobe_fx(v_add: i16) -> crate::clut_cell_fx::ClutCellFx {
    crate::clut_cell_fx::ClutCellFx {
        rect: RECT,
        h_add: 0,
        s_add: 0,
        v_add,
        mode: 0,
        white: 0,
    }
}

fn part_with(fx: crate::clut_cell_fx::ClutCellFx) -> crate::world::ambient::AmbientPart {
    crate::world::ambient::AmbientPart {
        record_off: 0,
        model_sel: 0x4000,
        flags: 0,
        buf: Vec::new(),
        state: Default::default(),
        // Finished part holding a live cell_fx: `step_ambient_fx` still
        // applies it (the `!finished || cell_fx` filter), and with no
        // banked ticks the tick/retire walk never runs, so the test owns
        // the target values.
        finished: true,
        cell_fx: Some(fx),
        scroll_fx: Vec::new(),
        prev_morph_weights: Default::default(),
    }
}

/// Seed a recognisable capture row so `apply_hsv_cell` output is non-trivial.
fn seeded_vram() -> legaia_tim::Vram {
    let mut vram = legaia_tim::Vram::new();
    // Mid-brightness distinct texels on the strobe row.
    let row: Vec<u8> = (0..16u16)
        .flat_map(|i| (0x3DEFu16 | (i & 1) << 15).to_le_bytes())
        .collect();
    vram.write_block(RECT.0, RECT.1, RECT.2, RECT.3, &row);
    vram
}

fn row_at(vram: &legaia_tim::Vram) -> Vec<u16> {
    (0..16)
        .map(|i| vram.as_u16()[RECT.1 as usize * 1024 + RECT.0 as usize + i])
        .collect()
}

#[test]
fn limiter_snaps_first_then_slews() {
    let mut w = World::new();
    assert!(w.reduce_flashing, "guard must default ON");
    // First application snaps to the simulated target (scene-entry state).
    let first = w.limit_flash(strobe_fx(-256), 1);
    assert_eq!(first.v_add, -256);
    // The target jumps back to 0 (the strobe's other phase, one tick
    // later): the applied value moves by at most the per-tick slew.
    let second = w.limit_flash(strobe_fx(0), 1);
    assert_eq!(second.v_add, -240, "one game tick moves v by the slew (16)");
    // Elapsed backlog steps proportionally: 15 more ticks close the rest.
    let third = w.limit_flash(strobe_fx(0), 15);
    assert_eq!(third.v_add, 0);
}

#[test]
fn hue_and_saturation_pass_through_unlimited() {
    let mut w = World::new();
    w.limit_flash(strobe_fx(0), 1);
    let mut jump = strobe_fx(0);
    jump.h_add = 0x5A; // quarter wheel in one tick
    jump.s_add = -200;
    let out = w.limit_flash(jump, 1);
    assert_eq!(out.h_add, 0x5A);
    assert_eq!(out.s_add, -200);
}

#[test]
fn guard_on_writes_the_limited_texels() {
    let mut w = World::new();
    let mut vram = seeded_vram();
    w.ambient_fx.push(part_with(strobe_fx(-256)));
    // No banked ticks: apply-only (the part survives; the walk never runs).
    assert!(w.step_ambient_fx(&mut vram));
    // First application snapped to the target.
    let src = w.ambient_cell_captures[&RECT].clone();
    let expect = crate::clut_cell_fx::apply_hsv_cell(&src, &strobe_fx(-256));
    assert_eq!(row_at(&vram), expect);
    assert_eq!(w.ambient_flash_applied[&RECT], (-256, 0));
}

#[test]
fn guard_off_applies_retail_exact_targets_and_keeps_no_state() {
    let mut w = World::new();
    w.reduce_flashing = false;
    let mut vram = seeded_vram();
    let pristine = row_at(&vram);
    w.ambient_fx.push(part_with(strobe_fx(-256)));
    assert!(w.step_ambient_fx(&mut vram));
    let src = w.ambient_cell_captures[&RECT].clone();
    assert_eq!(src, pristine);
    assert_eq!(
        row_at(&vram),
        crate::clut_cell_fx::apply_hsv_cell(&src, &strobe_fx(-256))
    );
    // Back to the other strobe phase: applied verbatim (pristine row).
    w.ambient_fx[0].cell_fx = Some(strobe_fx(0));
    assert!(w.step_ambient_fx(&mut vram));
    assert_eq!(row_at(&vram), pristine);
    assert!(
        w.ambient_flash_applied.is_empty(),
        "guard off keeps no state"
    );
}
