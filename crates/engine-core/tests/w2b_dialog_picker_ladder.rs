//! The option-picker ladder: a real scene's multi-choice conversation driven
//! through the session path into `OwnedDialogPanel::confirm_menu` - the
//! inline-script option-jump handler `FUN_80038050`.
//!
//! ## What this converts
//!
//! The `80038050` reach row: `confirm_menu` applies a chosen option's
//! relative jump (`new_pc = (open + 1 + index*2) + i16(entry[index])`) and
//! resumes the conversation at the branch's reply. Its only production
//! caller is the native window's keyboard handler
//! (`window/event_handler/keyboard.rs`, on the `active_dialog` panel the HUD
//! opens via `SceneHost::open_pending_dialog`), so no pad-only ladder ever
//! entered it: the row needs a *live option-picker conversation* - scene
//! content, not a harness entry point.
//!
//! This ladder supplies the scene content: the `koin4` merchant's two-price
//! offer (a three-option picker - two gold prices and a refusal - whose
//! labels the disc carries and whose branches reply). The panel path arms a
//! menu only when the picker sits **immediately after the request's first
//! text segment** (`OwnedDialogPanel::from_inline_dialog`'s faithful "is
//! this box a menu?" test), which is what makes this record panel-reachable
//! where a menu buried behind story-flag prologue branches (the Tetsu spar
//! menu) is inline-runner content instead.
//!
//! The drive is the native host's own contract, stage by stage: a field
//! interact on the NPC raises `World::current_dialog` (session path),
//! `SceneHost::open_pending_dialog` builds the panel exactly as
//! `sync_dialog_panel` does, the typewriter ticks until the menu arms,
//! Up/Down move the cursor, and the confirm calls `confirm_menu` - the
//! anchor - asserting the branch reply actually types out.
//!
//! Disc-gated: skip-passes when `LEGAIA_DISC_BIN` is unset or `extracted/`
//! is absent, per the repo convention.

use std::path::PathBuf;

use legaia_engine_core::dialog::{OwnedDialogPanel, PanelState};
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::SceneMode;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn gate() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let d = extracted_dir();
    if d.is_none() {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    }
    d
}

/// The koin4 merchant: interact -> menu -> cursor -> confirm -> branch
/// reply, all on the pre-decoded dialog-panel path the native window drives.
#[test]
fn the_koin4_price_menu_confirms_through_the_option_jump() {
    let Some(extracted) = gate() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("koin4", 0).expect("enter koin4");
    assert_eq!(host.world.mode, SceneMode::Field);

    // Find the NPC whose record fronts the two-price offer - the
    // three-option picker with the "I don't want anything" refusal. The slot
    // number is disc data; keying by content keeps the ladder honest if the
    // placement table's order ever re-derives.
    let mut offer_slot = None;
    for (&slot, inline) in &host.world.field_npc_dialog {
        if let Some(panel) = OwnedDialogPanel::from_inline_dialog(inline)
            && let Some(pk) = panel.picker()
            && pk.n == 3
            && pk
                .options
                .iter()
                .any(|o| String::from_utf8_lossy(&o.label).contains("don't want anything"))
        {
            offer_slot = Some(slot);
        }
    }
    let slot = offer_slot.expect("koin4 carries the merchant's two-price offer");

    // Interact through the session path: the world raises the pre-decoded
    // dialog request (`World::current_dialog`) from the NPC's inline record.
    host.world.trigger_field_interact(0, slot);
    let mut ticks = 0u32;
    while host.world.current_dialog.is_none() && ticks < 240 {
        host.world.set_pad(0);
        let _ = host.world.tick();
        ticks += 1;
    }
    assert!(
        host.world.current_dialog.is_some(),
        "the interact raised a dialog request within {ticks} ticks"
    );

    // The native HUD's panel-open contract (`sync_dialog_panel`).
    let mut panel = host
        .open_pending_dialog()
        .expect("the request opens a typed panel");
    panel.set_glyphs_per_frame(2);

    // Type the offer out; page within the prompt until the menu arms.
    let mut frames = 0u32;
    while !panel.menu_active() && frames < 4_000 {
        let st = panel.tick();
        if matches!(st, PanelState::PageBreak) && !panel.menu_active() {
            panel.advance_page(); // the confirm-press page turn
        }
        assert!(
            !matches!(st, PanelState::Done),
            "the conversation ended without arming its menu"
        );
        frames += 1;
    }
    assert!(panel.menu_active(), "the price menu armed while typing");
    let picker = panel.picker().expect("an armed menu exposes its picker");
    assert_eq!(picker.n, 3, "the offer is the three-option picker");

    // Cursor routing, exactly as the keyboard handler's arrows do: wrap the
    // whole ring, then land on option 1 (the cheaper price).
    for _ in 0..picker.n {
        panel.move_picker_cursor(1);
    }
    assert_eq!(panel.picker_cursor(), 0, "a full ring wraps to the top");
    panel.move_picker_cursor(1);
    assert_eq!(panel.picker_cursor(), 1);

    // The confirm: `FUN_80038050` - apply option 1's relative jump and
    // resume at the branch reply.
    let chosen = panel.confirm_menu();
    assert_eq!(chosen, Some(1), "confirm reports the committed option");
    assert!(!panel.menu_active(), "the committed menu closes");

    // The branch reply types real glyphs - a jump into noise would type
    // nothing or end the panel immediately.
    let mut reply_glyphs = 0usize;
    for _ in 0..2_000 {
        let st = panel.tick();
        reply_glyphs = reply_glyphs.max(panel.page_bytes().len());
        if matches!(st, PanelState::PageBreak | PanelState::Done) {
            break;
        }
    }
    assert!(
        reply_glyphs > 0,
        "the chosen branch's reply typed no glyphs - the option jump landed in noise"
    );
    eprintln!(
        "[picker] koin4 slot {slot}: option 1 committed, reply page carries {reply_glyphs} bytes"
    );
}
