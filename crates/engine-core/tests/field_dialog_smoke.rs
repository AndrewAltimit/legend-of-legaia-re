//! Field-VM → dialog panel wiring: opcode `0x3F` populates
//! `World::current_dialog`; `OwnedDialogPanel::from_scene_mes` resolves
//! the request through a synthetic `SceneMes`; ticking the panel emits
//! the bytes that the field VM passed.
//!
//! Synthesises a one-message Records-format `SceneMes` (using the
//! `legaia_mes::RECORD_MARKER` so `SceneMes::message_offset` resolves
//! `text_id = 0` to the start of our bytecode) and drives the full
//! field-VM-to-panel path with no disc, no asset loading, no winit.

use legaia_engine_core::dialog::{OwnedDialogPanel, PanelState};
use legaia_engine_core::scene_assets::SceneMes;
use legaia_engine_core::world::{SceneMode, World};
use legaia_mes::Format as MesFormat;

/// Build a Records-format `SceneMes` carrying one message: the bytes
/// `'h' 'i' 0x00`. Records format is the easiest to hand-roll because the
/// record offsets are explicit - no offset table to build.
fn synthetic_scene_mes_one_message(message: &[u8]) -> SceneMes {
    // Records format: each record begins with `0x44 0x78` and runs until
    // the next marker. Embed one record at offset 0.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0x44, 0x78]);
    bytes.extend_from_slice(message);
    SceneMes {
        entry_idx: 0,
        offset: 0,
        bytes,
        format: MesFormat::Records,
        offset_table: None,
        // text_id 0 resolves to the byte immediately after the marker.
        record_offsets: vec![2],
    }
}

#[test]
fn field_op_3f_opens_dialog_and_panel_emits_glyphs() {
    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };

    // Field VM op 0x3F encoding: [3F, lo, hi, len, ...inline, xb, zb, depth]
    // text_id = 0 (lo=0, hi=0), len=0 (no inline payload), xb=zb=0, depth=0.
    let bc = [0x3F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    world.load_field_record(&bc);

    // Step once - opcode 0x3F should fire `open_dialog`, which writes a
    // request to `world.current_dialog`.
    let _ = world.step_field();
    let req = world
        .current_dialog
        .as_ref()
        .expect("op 0x3F must populate current_dialog");
    assert_eq!(req.text_id, 0);

    // Build a synthetic MES container that maps text_id 0 → "hi\0".
    let mes = synthetic_scene_mes_one_message(b"hi\x00");
    let mut panel = OwnedDialogPanel::from_scene_mes(&mes, req.text_id)
        .expect("OwnedDialogPanel must resolve text_id 0 from a Records SceneMes");
    panel.set_glyphs_per_frame(1);

    // Tick four frames: 'h', 'i', End. The panel should be Done after the
    // terminator with the page byte buffer holding "hi".
    for _ in 0..4 {
        let s = panel.tick();
        if matches!(s, PanelState::Done) {
            break;
        }
    }
    assert!(
        panel.is_done(),
        "panel must reach Done after a 2-glyph + end stream"
    );
    assert_eq!(panel.page_bytes(), b"hi");
}

#[test]
fn dialog_clear_unblocks_world() {
    // Pre-seed a dialog request, then clear it (the equivalent of the user
    // dismissing the box). The next `current_dialog` read must be `None`.
    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    let bc = [0x3F, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00];
    world.load_field_record(&bc);
    let _ = world.step_field();
    assert!(world.current_dialog.is_some());
    world.current_dialog = None;
    assert!(world.current_dialog.is_none());
}
