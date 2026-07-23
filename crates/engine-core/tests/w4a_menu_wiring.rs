//! Wiring oracles for the pause-menu / shop / equip / save-select cluster.
//!
//! Each test drives a **host-reachable** entry point and asserts that the
//! ported kernel behind it actually ran, so the port stays on a live path
//! rather than being exercised only by its own unit tests.

use legaia_engine_core::battle_stats::{EquipmentTable, ItemModifier, StatRecord, StatusModifiers};
use legaia_engine_core::equip_session::{EquipInput, EquipSession};
use std::collections::HashMap;

fn press(cross: bool, down: bool) -> EquipInput {
    EquipInput {
        cross,
        down,
        ..Default::default()
    }
}

/// Two slot-1 candidates with different UDF bonuses, so the trial-equip
/// preview is distinguishable per row.
fn session() -> EquipSession {
    let record = StatRecord {
        base_attack: 10,
        base_udf: 20,
        base_ldf: 20,
        base_accuracy: 50,
        base_evasion: 10,
        base_spd: 10,
        base_int: 10,
        equip: [0; 8],
    };
    let mut inv = HashMap::new();
    // The legacy placeholder slot rule is `id >> 5`, so 0x20 / 0x21 are
    // both slot-1 candidates.
    inv.insert(0x20u8, 1u8);
    inv.insert(0x21u8, 1u8);
    let mut eq = EquipmentTable::new();
    eq.set(
        0x20,
        ItemModifier {
            udf: 5,
            ..Default::default()
        },
    );
    eq.set(
        0x21,
        ItemModifier {
            udf: 40,
            ..Default::default()
        },
    );
    EquipSession::new(record, inv, eq, StatusModifiers::default(), Vec::new())
}

/// `EquipSession::input` is the host-driven entry (`field_menu_dispatch`
/// ticks it every frame). Opening the candidate list must stage the retail
/// trial equip (`FUN_801d9c14`) for the top row, and moving the hand must
/// re-run it - the stat-compare block tracks the hovered row, not the
/// confirmed one.
#[test]
fn equip_item_picker_previews_the_hovered_candidate() {
    let mut s = session();
    let base_udf = s.preview_stats.udf;

    // Slot picker starts on slot 0; step to slot 1 and open its list.
    s.input(press(false, true));
    s.input(press(true, false));

    // Opening the list stages the first candidate (0x20, +5 UDF).
    let on_open = s.preview_stats.udf;
    assert_eq!(
        on_open,
        base_udf + 5,
        "opening the candidate list must stage the top row's trial equip"
    );

    // Moving the hand down re-runs it for 0x21 (+40 UDF).
    s.input(press(false, true));
    assert_eq!(
        s.preview_stats.udf,
        base_udf + 40,
        "the preview must follow the hand, not stay on the opening row"
    );
}

/// The confirm path routes through the same kernel: entering the Yes/No
/// prompt leaves the preview on the picked item.
#[test]
fn equip_confirm_preview_matches_the_picked_row() {
    let mut s = session();
    let base_udf = s.preview_stats.udf;
    s.input(press(false, true));
    s.input(press(true, false));
    s.input(press(false, true));
    s.input(press(true, false));
    assert_eq!(s.preview_stats.udf, base_udf + 40);
}

/// The Items screen's Use-list confirm routes through the ported
/// effect-class dispatch (`FUN_801D7E50`): the Door of Light / Incense
/// classes open their own confirm window, the Door of Wind class does not
/// (it is a destination list), and an ordinary item stays on the target
/// flow.
#[test]
fn use_list_confirm_routes_through_the_effect_class_dispatch() {
    use legaia_engine_core::pause_screens::{
        DOOR_OF_LIGHT_ITEM_ID, DOOR_OF_WIND_ITEM_ID, INCENSE_ITEM_ID, UseRoute,
        special_confirm_route_for_item, use_route_for_effect,
    };

    assert_eq!(
        special_confirm_route_for_item(DOOR_OF_LIGHT_ITEM_ID),
        Some(UseRoute::DoorOfLight)
    );
    assert_eq!(
        special_confirm_route_for_item(INCENSE_ITEM_ID),
        Some(UseRoute::Incense)
    );
    // Door of Wind has a route but not a confirm window.
    assert_eq!(special_confirm_route_for_item(DOOR_OF_WIND_ITEM_ID), None);
    assert_eq!(
        use_route_for_effect(0x81, 0),
        UseRoute::DoorOfWind,
        "the dispatch still knows the class even though the wrapper filters it"
    );
    // An ordinary bag id has no special class at all.
    assert_eq!(special_confirm_route_for_item(0x01), None);
}

/// Retail's `TestEvent` consumes the event it tests, so the session's
/// `NowChecking` beat must drain the latched card events after polling
/// them (`FUN_801E39A8`). Without the drain the latch is sticky and
/// re-fires on every later frame of every later beat.
#[test]
fn now_checking_beat_drains_the_latched_card_events() {
    use legaia_engine_core::save_select::{
        SaveSelectMode, SaveSelectSession, SelectInput, SelectPhase, SlotContent, SlotSnapshot,
    };

    let present = SlotSnapshot {
        present: true,
        content: SlotContent::LegaiaSave,
        ..SlotSnapshot::empty(0)
    };
    let mut s = SaveSelectSession::new(SaveSelectMode::Load, vec![present, SlotSnapshot::empty(1)]);
    s.set_now_checking_frames(120);
    // Confirm slot 0 -> the card-read beat.
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    assert!(matches!(
        s.phase(),
        SelectPhase::NowChecking { slot: 0, .. }
    ));

    // Latch handle 1 (Aborted): inconclusive, so the beat continues - and
    // the latch must not survive the frame that read it.
    s.set_card_events([false, true, false, false]);
    assert_eq!(s.card_events(), [false, true, false, false]);
    s.tick(SelectInput::default());
    assert_eq!(
        s.card_events(),
        [false; 4],
        "the poll must consume the events it tested (FUN_801E39A8)"
    );
    assert!(
        matches!(s.phase(), SelectPhase::NowChecking { slot: 0, .. }),
        "an aborted poll leaves the beat to the frame countdown"
    );
}
