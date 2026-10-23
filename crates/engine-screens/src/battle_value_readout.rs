//! The battle value readout's layout (damage / heal numerals and the combo
//! cluster) for one frame, shared by both play hosts.
//!
//! Each host keeps only its own camera and its answer to "is the battle
//! effect atlas resident"; the layout and both emits (retail-art prims, or
//! the font fallback before the atlas exists) are the functions here, so
//! the two cannot seat or draw the same number differently.

use legaia_engine_battle::battle_hud_state::{BattleHud, DamagePopup};
use legaia_engine_battle_vm::battle_value_readout as vr;
use legaia_engine_core::world::World;
use legaia_engine_mode::mode::SceneMode;

/// One frame's readout: the combo cluster, and one run of digit cells per
/// struck actor.
pub type ValueReadoutLayout = (Option<vr::ComboCluster>, Vec<Vec<vr::ValueCell>>);

/// The readout's layout this frame, seated through `vp` (the host's battle
/// camera, without the actor stage's world scale) at `world_scale`. `None`
/// outside battle and with nothing to say.
///
/// The combo counter cluster is the `HIT` / `TOTAL` / `DAMAGE` word cells and
/// the value digits, off the same sheet, on the seats the steal-banner and
/// tail-fire display lists pin, sliding in with placement record 80's glide
/// (`vr::combo_slide`).
///
/// Only the newest popup per actor contributes. Retail's readout is a
/// per-slot **value window** (`_DAT_801F6980`, four halfwords, one per slot),
/// so a second hit on the same actor replaces the figure rather than stacking
/// beside it - and stacking is not cosmetic here: two runs centred on the
/// same point interleave into an unreadable third number (a 9 landing inside
/// a 10 reads as "190"). Status applications have no numeral on the sheet.
///
/// Each run is seated the way retail's renderer `FUN_801DF6B8` seats it: a
/// view-space square over the struck actor's display trio, rising and growing
/// with the ring timer (`legaia_engine_ui::battle_numerals::popup_value_cells`
/// scales the anchor and keeps the square's half-extent in view units, as
/// retail's projection does). An actor with no display trio, or a popup past
/// its live window, contributes no run.
pub fn battle_value_readout_layout(
    world: &World,
    hud: &BattleHud,
    vp: &[f32; 16],
    world_scale: f32,
) -> Option<ValueReadoutLayout> {
    battle_value_readout_layout_with(world, hud, vp, world_scale, |slot| {
        world.battle_readout_trio(usize::from(slot))
    })
}

/// [`battle_value_readout_layout`] with the anchor trio of each struck slot
/// taken from `trio_of` - the capture harness's seam for a popup it rewinds.
pub fn battle_value_readout_layout_with(
    world: &World,
    hud: &BattleHud,
    vp: &[f32; 16],
    world_scale: f32,
    trio_of: impl Fn(u8) -> Option<[f32; 3]>,
) -> Option<ValueReadoutLayout> {
    if world.mode != SceneMode::Battle {
        return None;
    }
    if hud.popups.is_empty() && hud.combo.is_none() {
        return None;
    }
    let cluster = hud
        .combo
        .as_ref()
        .map(|c| vr::combo_cluster(c.style, c.hits, c.total, c.slide()));
    let mut newest: Vec<&DamagePopup> = Vec::new();
    for p in &hud.popups {
        if p.status.is_some() {
            continue;
        }
        match newest.iter_mut().find(|q| q.slot == p.slot) {
            Some(q) if q.frames_remaining >= p.frames_remaining => {}
            Some(q) => *q = p,
            None => newest.push(p),
        }
    }
    let mut runs = Vec::new();
    for p in newest {
        let Some(trio) = trio_of(p.slot) else {
            continue;
        };
        let age = p.frames_total.saturating_sub(p.frames_remaining);
        let cells = legaia_engine_ui::battle_numerals::popup_value_cells(
            vp,
            world_scale,
            trio,
            p.amount,
            age,
        );
        if !cells.is_empty() {
            runs.push(cells);
        }
    }
    Some((cluster, runs))
}

/// The layout as retail's own 24x24 numeral cells off the resident battle
/// effect atlas: the combo cluster, then every digit run, through
/// `legaia_engine_ui::battle_numerals` at the readout's ordering-table
/// index. A host calls this only while the battle VRAM holding the sheet is
/// resident, and [`value_readout_fallback_draws`] otherwise - never both, or
/// every number draws twice.
pub fn value_readout_prims(
    layout: &ValueReadoutLayout,
) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
    use legaia_engine_ui::battle_numerals as bn;
    let (cluster, runs) = layout;
    let mut out = Vec::new();
    if let Some(c) = cluster.as_ref() {
        out.extend(bn::combo_cluster_prims(c, bn::VALUE_READOUT_OT));
    }
    for cells in runs {
        out.extend(bn::digit_run_prims(cells, bn::VALUE_READOUT_OT));
    }
    out
}

/// The layout as **font text** for the frames before the battle VRAM
/// exists, on the stage transform `origin` / `scale` the rest of the battle
/// chrome uses: the combo cluster's words and digits, then each run's digits
/// in `VALUE_READOUT_FALLBACK_COLOR`.
pub fn value_readout_fallback_draws(
    font: &legaia_font::Font,
    layout: &ValueReadoutLayout,
    origin: (i32, i32),
    scale: u32,
) -> Vec<legaia_engine_ui::TextDraw> {
    use legaia_engine_ui as ui;
    let view = |k: &vr::ValueCell| ui::ValueCellView {
        digit: k.digit,
        x: k.x,
        y: k.y,
        w: k.w,
        h: k.h,
    };
    let (cluster, runs) = layout;
    let mut out = Vec::new();
    if let Some(c) = cluster.as_ref() {
        let labels: Vec<ui::ComboLabelView<'_>> = c
            .labels
            .iter()
            .map(|l| ui::ComboLabelView {
                word: l.word,
                x: l.x,
                y: l.y,
            })
            .collect();
        let cells: Vec<ui::ValueCellView> = c.cells.iter().map(view).collect();
        out.extend(ui::battle_combo_cluster_draws_for(
            font, &labels, &cells, origin, scale,
        ));
    }
    for run in runs {
        let cells: Vec<ui::ValueCellView> = run.iter().map(view).collect();
        out.extend(ui::battle_value_readout_draws_for(
            font,
            &cells,
            ui::VALUE_READOUT_FALLBACK_COLOR,
            origin,
            scale,
        ));
    }
    out
}
