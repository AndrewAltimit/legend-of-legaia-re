//! The hub entry list's per-entry sub-draw, spliced where retail's `jal` sits.
//!
//! `FUN_801F16C0` publishes each entry's code byte to `DAT_8007B469`, prints
//! that entry's label, advances its pen `0x0D`, calls `FUN_801E5B4C` at
//! `0x801F1778`, and then advances a further `0x2A`. That `jal` is
//! `FUN_801E5B4C`'s **only** reference in the corpus, so the equipment stat
//! panel exists for this list and nothing else.
//!
//! The port used to stop at a marker. What this covers is the join: the panel
//! runs at the pen the caller left, once per drawn entry, over the character
//! the caller published - not the list position.

use legaia_engine_vm::baka_hub_actors::{
    HubActor, HubDraw, HubEnv, HubEquipEnv, HubEquipRecord, HubGrid, HubPainter,
};
use legaia_engine_vm::world_map_overlay::{
    EQUIP_COL_CURRENT, EQUIP_COL_LABEL, EQUIP_LABEL_VAS, EQUIP_ROW_PITCH, EquipPanelDraw,
    EquipProps, ItemProps,
};

/// Two party members with different loadouts, so a per-entry panel that keyed
/// on the wrong character would print the wrong numbers rather than none.
fn env() -> HubEnv {
    HubEnv {
        entry_count: 2,
        entry_codes: vec![0, 1],
        equip: HubEquipEnv {
            records: vec![
                HubEquipRecord {
                    code: 0,
                    slots: [1, 0, 0, 0, 0],
                    base_stats: [100, 200, 300],
                },
                HubEquipRecord {
                    code: 1,
                    slots: [0, 0, 0, 0, 0],
                    base_stats: [11, 22, 33],
                },
            ],
            item_props: vec![
                ItemProps::default(),
                ItemProps {
                    kind: 1,
                    stat_index: 1,
                },
            ],
            equip_props: vec![
                EquipProps::default(),
                EquipProps {
                    bonuses: [0, 7, 0, 0, 0],
                    char_mask: 7,
                    slot_bits: 0x40,
                },
            ],
            ..HubEquipEnv::default()
        },
        ..HubEnv::default()
    }
}

fn sub_panel(draws: &[HubDraw]) -> Vec<EquipPanelDraw> {
    draws
        .iter()
        .filter_map(|d| match d {
            HubDraw::EntrySubPanel(p) => Some(*p),
            _ => None,
        })
        .collect()
}

#[test]
fn every_drawn_entry_paints_a_sub_panel_at_the_pen_the_caller_left() {
    let mut actor = HubActor {
        x: 0x10,
        y: 0x40,
        ..HubActor::default()
    };
    let frame = HubPainter::EntryList.paint(&mut actor, &env(), &HubGrid::default());

    let labels: Vec<i16> = frame
        .draws
        .iter()
        .filter_map(|d| match d {
            HubDraw::Text { y, .. } => Some(*y),
            _ => None,
        })
        .collect();
    assert_eq!(labels, vec![0x40, 0x40 + 0x0D + 0x2A]);

    let panel = sub_panel(&frame.draws);
    // Three rows per entry, label + current value each, two entries.
    assert_eq!(panel.len(), 2 * 3 * 2);

    // The first entry's panel starts `0x0D` below its label, not at it.
    assert_eq!(
        panel[0],
        EquipPanelDraw::Label {
            label_va: EQUIP_LABEL_VAS[0],
            x: 0x10 + EQUIP_COL_LABEL,
            y: 0x40 + 0x0D,
            ink: 7,
        }
    );
    // And the second entry's starts a full entry pitch further down.
    assert_eq!(
        panel[6],
        EquipPanelDraw::Label {
            label_va: EQUIP_LABEL_VAS[0],
            x: 0x10 + EQUIP_COL_LABEL,
            y: 0x40 + 0x0D + 0x2A + 0x0D,
            ink: 7,
        }
    );
    // The pen is restored, which is what lets the caller be re-entered.
    assert_eq!(actor.y, 0x40);
}

#[test]
fn the_panel_reads_the_entry_code_not_the_list_position() {
    let mut actor = HubActor::default();
    let frame = HubPainter::EntryList.paint(&mut actor, &env(), &HubGrid::default());
    let values: Vec<i32> = sub_panel(&frame.draws)
        .iter()
        .filter_map(|d| match d {
            EquipPanelDraw::Value {
                value,
                x: EQUIP_COL_CURRENT,
                ..
            } => Some(*value),
            _ => None,
        })
        .collect();
    // Entry code 0 wears item 1 (+7 on the ATK accumulator); entry code 1
    // wears nothing. Reversing the two would swap these.
    assert_eq!(values, vec![100 + 7, 200, 300, 11, 22, 33]);
}

#[test]
fn a_skipped_entry_costs_a_loop_step_and_no_sub_panel() {
    let mut e = env();
    e.entry_count = 3;
    e.entry_codes = vec![0, 5, 1];
    let mut actor = HubActor::default();
    let frame = HubPainter::EntryList.paint(&mut actor, &e, &HubGrid::default());
    assert_eq!(
        sub_panel(&frame.draws).len(),
        2 * 3 * 2,
        "code 5 draws nothing"
    );
}

/// Non-vacuity in the other direction: the panel's row pitch is retail's, and
/// the op-`0x49` descriptor cell tightens it. If the sub-draw were a marker
/// again, there would be no rows to measure.
#[test]
fn the_board_flag_reaches_the_sub_panel_through_the_list() {
    let mut e = env();
    e.board_flag = 1;
    let mut actor = HubActor::default();
    let frame = HubPainter::EntryList.paint(&mut actor, &e, &HubGrid::default());
    let ys: Vec<i16> = sub_panel(&frame.draws)
        .iter()
        .filter_map(|d| match d {
            EquipPanelDraw::Label { y, .. } => Some(*y),
            _ => None,
        })
        .collect();
    let tight = EQUIP_ROW_PITCH[1];
    assert_eq!(ys[1] - ys[0], tight);
    assert_ne!(tight, EQUIP_ROW_PITCH[0]);
}
