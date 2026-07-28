//! The hub entry list's equipment sub-panel, reached from the frame loop.
//!
//! `HubPainter::EntryList` is panel-window record `3` of the op-`0x49` submode
//! family, and `World::tick` reaches it through `tick_handler_actors` ->
//! `tick_submode_screen` -> `HubPainter::paint`. This asserts the sub-draw
//! `FUN_801E5B4C` is on that path rather than only on a unit-test one: calling
//! the painter directly would prove the kernel computes, not that a running
//! session ever asks it to.

use legaia_engine_core::actor_handler::ActorHandler;
use legaia_engine_core::world::World;
use legaia_engine_vm::baka_hub_actors::{HubDraw, HubPainter, slot};
use legaia_engine_vm::world_map_overlay::EquipPanelDraw;

/// Panel-window record the entry list paints.
const ENTRY_LIST_WINDOW: usize = 3;

fn field_world() -> World {
    let mut w = World::new();
    w.man_load_actor_reset();
    assert!(
        w.find_actor_by_handler(ActorHandler::SubmodeDriver)
            .is_some(),
        "every MAN load spawns the op-0x49 submode driver"
    );
    w
}

fn tick_until(w: &mut World, limit: usize, mut done: impl FnMut(&World) -> bool) -> bool {
    for _ in 0..limit {
        w.tick();
        if done(w) {
            return true;
        }
    }
    false
}

fn sub_panel(w: &World) -> Vec<EquipPanelDraw> {
    w.submode_screen
        .draws()
        .iter()
        .filter_map(|d| match d {
            HubDraw::EntrySubPanel(p) => Some(*p),
            _ => None,
        })
        .collect()
}

#[test]
fn the_window_record_three_painter_is_the_entry_list() {
    assert_eq!(
        HubPainter::for_window(ENTRY_LIST_WINDOW),
        Some(HubPainter::EntryList)
    );
}

#[test]
fn a_live_frame_loop_paints_the_per_entry_equipment_panel() {
    let mut w = field_world();
    w.active_party = vec![0, 1];
    w.open_field_submode_screen(slot::DRAW_TICK, Some(ENTRY_LIST_WINDOW));

    assert!(
        tick_until(&mut w, 16, |w| !sub_panel(w).is_empty()),
        "the frame loop never reached the entry list's sub-draw"
    );
    // Three rows per entry, label + current value each - the no-candidate arm,
    // which is what the hub's own list mode selects.
    assert_eq!(sub_panel(&w).len(), 2 * 3 * 2);
}

/// The rows the loop paints carry retail's own column geometry, which is what
/// a marker-only sub-draw could not produce.
///
/// This world has no roster and no static tables, so its rows print a base
/// stat of zero plus a zero aggregate. That is the empty case, not a gap:
/// `submode_env` fills `HubEnv::equip` from `World::roster`,
/// `World::item_effects` and `World::equipment_table`, and
/// `hub_entry_sub_panel_env` / `hub_entry_sub_panel_disc` assert the numbers a
/// populated world prints.
#[test]
fn the_painted_rows_carry_retails_column_geometry() {
    use legaia_engine_vm::world_map_overlay::{
        EQUIP_COL_CURRENT, EQUIP_COL_LABEL, EQUIP_LABEL_VAS, EQUIP_ROW_PITCH,
    };

    let mut w = field_world();
    w.active_party = vec![0];
    w.open_field_submode_screen(slot::DRAW_TICK, Some(ENTRY_LIST_WINDOW));
    assert!(tick_until(&mut w, 16, |w| !sub_panel(w).is_empty()));

    let panel = sub_panel(&w);
    let labels: Vec<(u32, i16, i16)> = panel
        .iter()
        .filter_map(|d| match d {
            EquipPanelDraw::Label { label_va, x, y, .. } => Some((*label_va, *x, *y)),
            _ => None,
        })
        .collect();
    assert_eq!(labels.len(), 3);
    for (row, (va, x, _)) in labels.iter().enumerate() {
        assert_eq!(*va, EQUIP_LABEL_VAS[row]);
        assert_eq!(*x - labels[0].1, 0, "one label column");
    }
    // The tight pitch, because the op-`0x49` descriptor cell is set while a
    // submode screen is armed - which is the state this screen is in.
    assert_eq!(labels[1].2 - labels[0].2, EQUIP_ROW_PITCH[1]);
    assert_eq!(labels[2].2 - labels[1].2, EQUIP_ROW_PITCH[1]);

    let value_x: Vec<i16> = panel
        .iter()
        .filter_map(|d| match d {
            EquipPanelDraw::Value { x, .. } => Some(*x),
            _ => None,
        })
        .collect();
    assert!(
        value_x
            .iter()
            .all(|x| *x - labels[0].1 == EQUIP_COL_CURRENT - EQUIP_COL_LABEL)
    );
}
