//! Field-VM → dialog panel wiring: the **field-interact op** (`0x3E` with
//! `op0 < 100`) opens the interacted actor's inline interaction-script
//! dialogue into `World::dialog.current`; `OwnedDialogPanel::from_inline_dialog`
//! renders it; ticking the panel emits the glyphs.
//!
//! Field dialogue has no dedicated opcode - it is the actor's own inline MES
//! text (retail `actor[+0x90]`), keyed by the interact op's `slot`. (Opcode
//! `0x3F`, which an earlier model treated as the dialog opener, is the named
//! scene-change; see `docs/subsystems/script-vm.md` § Field dialogue.) This
//! drives the full field-VM-to-panel path with no disc, no asset loading, no
//! winit.

use legaia_engine_core::dialog::{OwnedDialogPanel, PanelState};
use legaia_engine_core::world::{SceneMode, World};

#[test]
fn field_interact_opens_dialog_and_panel_emits_glyphs() {
    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };

    // Seed actor slot 3's inline interaction-script dialogue: a single
    // `0x1F`-lead segment carrying "hi" (the MES glyph bytes for 'h' 'i'),
    // `0x00`-terminated.
    world.npcs.dialog.insert(3, vec![0x1F, b'h', b'i', 0x00]);

    // The interaction path (what the talk probe calls) on slot 3. No field-VM
    // opcode opens dialogue - op 0x3E with op0 < 100 is the scripted-battle
    // install.
    world.trigger_field_interact(0x05, 0x03);
    let req = world
        .dialog
        .current
        .as_ref()
        .expect("field-interact on an actor with inline text must open current_dialog");
    assert_eq!(req.inline, vec![0x1F, b'h', b'i', 0x00]);

    // Render the inline dialogue: from_inline_dialog re-finds the 0x1F lead and
    // types the segment after it.
    let mut panel = OwnedDialogPanel::from_inline_dialog(&req.inline)
        .expect("OwnedDialogPanel must resolve the inline 0x1F segment");
    panel.set_glyphs_per_frame(1);

    // Tick a few frames: 'h', 'i', End. The panel should be Done after the
    // terminator with the page byte buffer holding "hi".
    for _ in 0..4 {
        if matches!(panel.tick(), PanelState::Done) {
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
    // Open a dialog via the field-interact path, then clear it (the equivalent
    // of the user dismissing the box). The next `current_dialog` read is `None`.
    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    world.npcs.dialog.insert(3, vec![0x1F, b'h', b'i', 0x00]);
    world.trigger_field_interact(0x05, 0x03);
    assert!(world.dialog.current.is_some());
    world.dialog.current = None;
    assert!(world.dialog.current.is_none());
}
