//! Disc-gated: a real town NPC conversation must **end**.
//!
//! The engine's inline field-VM runner ends one conversation pass when the
//! record's tail jumps backward onto a PC the pass already executed - retail's
//! records all finish that way, jumping to a shared tail that loops back to the
//! top selector, where the dialog SM parks until the next talk.
//!
//! That detector was blind to the commonest carrier of the loop-back: the
//! `visited` map was marked only for **opcode** bytes, and a record whose tail
//! jumps back onto a `0x1F` **text segment** therefore replayed its lines with
//! nothing able to stop it - the record's terminal `0x26` JmpRel lands on the
//! opening line, and the runner walked straight past a wrap it could not see.
//! An inescapable looping NPC conversation is what that is from the pad.
//!
//! This drives each placement's record through the real runner with an
//! auto-confirm and asserts it terminates inside a generous frame budget.
//!
//! Scope: records that open **no option picker**. A menu record is allowed to
//! cycle - retail's own book-menu shape re-emits its menu after each branch
//! reply, and the player leaves it by picking its exit option, which is a
//! choice no auto-player can be trusted to make. So a picker record's
//! non-termination under this driver says nothing, and it is not asserted on.
//! Every record without one must end on its own.
//!
//! Skip-passes without disc data.

use std::path::PathBuf;

use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Frames one conversation gets. A long Rim Elm record runs a few hundred
/// boxes' worth of typewriter at this cadence; a looping one runs forever, so
/// the budget only has to separate "long" from "unbounded".
const FRAME_BUDGET: usize = 20_000;

/// Play placement `slot`'s interaction record to its end, mashing confirm.
/// Returns `(boxes_shown, terminated, opened_a_picker)`.
fn play(session: &mut BootSession, slot: u8, pick_last_option: bool) -> (usize, bool, bool) {
    let w = &mut session.host.world;
    w.inline_dialogue = None;
    w.current_dialog = None;
    w.active_inline_prologue = None;
    w.trigger_field_interact(0, slot);
    let Some(pr) = w.active_inline_prologue.take() else {
        return (0, true, false);
    };
    w.start_inline_dialogue_with_prologue(pr.body, pr.entry_pc, pr.first_segment);

    let mut boxes = 0usize;
    let mut prev_panel = false;
    let mut saw_picker = false;
    for f in 0..FRAME_BUDGET {
        let w = &mut session.host.world;
        let Some(d) = w.inline_dialogue.as_ref() else {
            return (boxes, true, saw_picker);
        };
        let panel_up = d.panel.is_some();
        if panel_up && !prev_panel {
            boxes += 1;
        }
        prev_panel = panel_up;
        let confirm = f % 3 == 0;
        // Optionally walk the option cursor before committing, so a record
        // whose first option loops can still be escaped by another.
        let menu = d.menu_active();
        saw_picker |= menu;
        let down = pick_last_option && menu && !confirm;
        w.step_inline_dialogue(confirm, false, down);
        if w.inline_dialogue.as_ref().is_some_and(|d| d.is_done()) {
            w.inline_dialogue = None;
            return (boxes, true, saw_picker);
        }
    }
    (boxes, false, saw_picker)
}

#[test]
fn rim_elm_conversations_end() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open extracted boot session");
    session
        .enter_field_live("town01", &FieldLiveOpts::default())
        .expect("enter town01 live");
    session.host.world.use_vm_dialogue = true;

    let mut slots: Vec<u8> = session
        .host
        .world
        .field_npc_dialog_prologue
        .keys()
        .copied()
        .collect();
    slots.sort_unstable();
    assert!(
        slots.len() > 20,
        "town01 must surface its talkable placements (got {})",
        slots.len()
    );

    let mut looping: Vec<(u8, usize)> = Vec::new();
    let mut checked = 0usize;
    for slot in slots {
        for pick_last in [false, true] {
            let (boxes, ended, saw_picker) = play(&mut session, slot, pick_last);
            if saw_picker {
                // Out of scope - see the module docstring.
                continue;
            }
            checked += 1;
            if !ended {
                looping.push((slot, boxes));
            }
        }
    }
    assert!(
        checked > 20,
        "the driver must actually reach most placements (checked {checked})"
    );
    assert!(
        looping.is_empty(),
        "these town01 conversations never end - the player cannot escape them: \
         {looping:?}"
    );
}
