use super::*;

#[test]
fn inline_dialogue_runs_branch_flag_set_through_field_vm() {
    // A menu box ("Hi" + A/B picker) whose option branches each SET a distinct
    // system flag before their reply. The faithful runner must (a) show the
    // menu, (b) on confirm apply the chosen option's relative jump, (c) run the
    // branch's `0x50` SET through the field VM, (d) show the reply. Choosing B
    // must set flag 6 and NOT flag 5.
    let mut b = vec![0x1F, b'H', b'i', 0x00]; // prompt, ends at pc 4
    let open = b.len(); // 4
    b.push(0x27); // 2-option picker
    let entries_at = b.len(); // 5
    b.extend_from_slice(&[0, 0, 0, 0]); // 2 jump entries, filled below
    b.push(0x24); // continuation
    b.extend_from_slice(&[0x1F, b'A', 0x00]); // label 0
    b.extend_from_slice(&[0x1F, b'B', 0x00]); // label 1
    let branch0 = b.len();
    b.extend_from_slice(&[0x50, 0x05]); // option A: SET system flag 5
    b.extend_from_slice(&[0x1F, b'a', 0x00]); // reply "a"
    b.push(0x00); // conversation end
    let branch1 = b.len();
    b.extend_from_slice(&[0x50, 0x06]); // option B: SET system flag 6
    b.extend_from_slice(&[0x1F, b'b', 0x00]); // reply "b"
    b.push(0x00); // conversation end
    let j0 = (branch0 as i32 - (open as i32 + 1)) as i16;
    let j1 = (branch1 as i32 - (open as i32 + 1 + 2)) as i16;
    b[entries_at..entries_at + 2].copy_from_slice(&j0.to_le_bytes());
    b[entries_at + 2..entries_at + 4].copy_from_slice(&j1.to_le_bytes());

    let mut world = World::new();
    world.start_inline_dialogue(b);

    // Tick until the menu box is awaiting a choice.
    let mut guard = 0;
    while !world.inline_dialogue.as_ref().unwrap().menu_active() {
        world.step_inline_dialogue(false, false, false);
        guard += 1;
        assert!(guard < 50, "menu never became active");
    }
    // Move the cursor to option B and confirm.
    world.step_inline_dialogue(false, false, true);
    assert_eq!(world.inline_dialogue.as_ref().unwrap().last_choice, None);
    world.step_inline_dialogue(true, false, false);
    assert_eq!(world.inline_dialogue.as_ref().unwrap().last_choice, Some(1));

    // The VM should run branch B (SET flag 6) and surface the "b" reply.
    let mut guard = 0;
    while world.inline_dialogue.as_ref().unwrap().page_bytes() != b"b" {
        world.step_inline_dialogue(false, false, false);
        guard += 1;
        assert!(guard < 50, "branch reply never typed");
    }
    assert!(
        world.system_flag_test(6),
        "option B branch SET flag 6 via the VM"
    );
    assert!(
        !world.system_flag_test(5),
        "option A branch must not have run"
    );

    // Confirming the reply ends the conversation.
    world.step_inline_dialogue(true, false, false);
    world.step_inline_dialogue(false, false, false);
    assert!(world.inline_dialogue.as_ref().unwrap().is_done());
}

/// Step the inline runner until a box is open and the typewriter has fully
/// revealed it (the glyph bytes stop growing), then return its page glyph bytes.
/// Panics if no stable box appears within a bounded number of ticks.
fn run_inline_until_box(world: &mut World) -> Vec<u8> {
    let mut last: Vec<u8> = Vec::new();
    let mut stable = 0;
    for _ in 0..400 {
        world.step_inline_dialogue(false, false, false);
        let pb = world.inline_dialogue.as_ref().unwrap().page_bytes();
        if pb.is_empty() {
            continue;
        }
        if pb == last {
            stable += 1;
            if stable >= 2 {
                return pb;
            }
        } else {
            stable = 0;
            last = pb;
        }
    }
    panic!("box never opened / finished typing");
}

#[test]
fn inline_dialogue_prologue_selects_segment_by_story_flag() {
    // The interaction record's prologue is a single `SysFlag.Test` (op `0x70`)
    // on story flag 7: when the flag is set it jumps to segment B, otherwise it
    // falls through to segment A. This is the retail segment-selection mechanism
    // - the prologue's story-flag-gated jump chooses which line the box opens at.
    //
    //   pc 0: 70 07 06 00   SysFlag.Test flag 7 -> jump to pc (2 + 6) = 8
    //   pc 4: 1F 'A' 'A' 00  segment A (fall-through)
    //   pc 8: 1F 'B' 'B' 00  segment B (selected when flag 7 set)
    let body = vec![
        0x70, 0x07, 0x06, 0x00, // SysFlag.Test flag 7
        0x1F, b'A', b'A', 0x00, // segment A @ 4
        0x1F, b'B', b'B', 0x00, // segment B @ 8
    ];
    let entry_pc = 0;
    let first_segment = 4;

    // Flag clear: the test falls through to segment A.
    let mut world = World::new();
    assert!(!world.system_flag_test(7));
    world.start_inline_dialogue_with_prologue(body.clone(), entry_pc, first_segment);
    assert_eq!(run_inline_until_box(&mut world), b"AA");

    // Flag set: the prologue jumps to segment B.
    let mut world = World::new();
    world.system_flag_set(7);
    world.start_inline_dialogue_with_prologue(body, entry_pc, first_segment);
    assert_eq!(run_inline_until_box(&mut world), b"BB");
}

#[test]
fn inline_dialogue_prologue_falls_back_when_it_cannot_reach_a_segment() {
    // A prologue that can't proceed (here a `CFLAG_TST` on a clear ctx bit, which
    // halts) must not silently drop the dialogue: the runner falls back to the
    // first segment so the box still shows - never worse than the truncated path.
    //
    //   pc 0: 33 05         CFLAG_TST bit 5 (clear on a fresh ctx) -> Halt
    //   pc 2: 1F 'X' 'X' 00 first segment (fallback target)
    let body = vec![0x33, 0x05, 0x1F, b'X', b'X', 0x00];
    let mut world = World::new();
    world.start_inline_dialogue_with_prologue(body, 0, 2);
    assert_eq!(run_inline_until_box(&mut world), b"XX");
}

#[test]
fn inline_dialogue_resident_loop_ends_after_one_pass() {
    // Interaction records are resident conversation drivers: every branch
    // exits by jumping to a shared tail that loops back to the top selector
    // (town01's Val record: each talk plays ONE branch, then the context
    // parks until the next interaction). The runner must end the
    // conversation at that loop-back instead of replaying the branch
    // forever - the "infinite Val dialog" play-test regression.
    //
    //   pc 0: 50 09          SET system flag 9 (top selector body)
    //   pc 2: 1F 'V' 00      the branch box
    //   pc 5: 26 FA FF       JmpRel back to pc 0 (base = pc+1, delta -6)
    let body = vec![0x50, 0x09, 0x1F, b'V', 0x00, 0x26, 0xFA, 0xFF];
    let mut world = World::new();
    world.start_inline_dialogue_with_prologue(body, 0, 2);
    assert_eq!(run_inline_until_box(&mut world), b"V");
    // Dismiss the box: the VM resumes, takes the loop-back, and the wrap
    // rule ends the conversation (one pass, like retail's park).
    world.step_inline_dialogue(true, false, false);
    let mut guard = 0;
    while !world.inline_dialogue.as_ref().unwrap().is_done() {
        world.step_inline_dialogue(false, false, false);
        guard += 1;
        assert!(
            guard < 50,
            "the resident loop-back must end the conversation"
        );
    }
    assert!(world.system_flag_test(9), "the pass body executed");
}

#[test]
fn inline_dialogue_menu_reemission_survives_wrap_rule() {
    // A menu record that re-emits its menu by jumping BACK after a branch
    // reply (the izumi book-menu shape). The picker commit clears the wrap
    // map, so the backward jump re-opens the menu instead of ending the
    // conversation.
    let mut b = vec![0x1F, b'M', 0x00]; // menu prompt @ 0
    let open = b.len(); // 3
    b.push(0x27); // 2-option picker
    let entries_at = b.len();
    b.extend_from_slice(&[0, 0, 0, 0]);
    b.push(0x24); // continuation
    b.extend_from_slice(&[0x1F, b'A', 0x00]); // label 0
    b.extend_from_slice(&[0x1F, b'Q', 0x00]); // label 1 (quit)
    let branch0 = b.len();
    // Option A: reply "a", then jump back to the menu prompt (re-emit).
    b.extend_from_slice(&[0x1F, b'a', 0x00]);
    b.push(0x26);
    let back_at = b.len();
    b.extend_from_slice(&[0, 0]);
    // JmpRel target = (pc + 1) + delta; the op byte sits at back_at - 1, so
    // jumping to the menu prompt at pc 0 needs delta = -back_at.
    let delta0 = 0u16.wrapping_sub(back_at as u16);
    b[back_at..back_at + 2].copy_from_slice(&delta0.to_le_bytes());
    let branch1 = b.len();
    // Option Q: reply "q", then end.
    b.extend_from_slice(&[0x1F, b'q', 0x00, 0x00]);
    let j0 = (branch0 as i32 - (open as i32 + 1)) as i16;
    let j1 = (branch1 as i32 - (open as i32 + 1 + 2)) as i16;
    b[entries_at..entries_at + 2].copy_from_slice(&j0.to_le_bytes());
    b[entries_at + 2..entries_at + 4].copy_from_slice(&j1.to_le_bytes());

    let mut world = World::new();
    world.start_inline_dialogue(b);
    let mut guard = 0;
    while !world.inline_dialogue.as_ref().unwrap().menu_active() {
        world.step_inline_dialogue(false, false, false);
        guard += 1;
        assert!(guard < 50, "menu never became active");
    }
    // Pick option A: reply "a" plays, then the record jumps back to the menu.
    world.step_inline_dialogue(true, false, false);
    let mut guard = 0;
    while world.inline_dialogue.as_ref().unwrap().page_bytes() != b"a" {
        world.step_inline_dialogue(false, false, false);
        guard += 1;
        if guard >= 80 {
            let id = world.inline_dialogue.as_ref().unwrap();
            panic!(
                "branch reply never typed: pc={} done={} page={:?} menu={}",
                id.pc,
                id.is_done(),
                String::from_utf8_lossy(&id.page_bytes()),
                id.menu_active()
            );
        }
    }
    // Dismiss the reply: the backward jump must RE-OPEN the menu (the wrap
    // map was cleared by the picker commit), not end the conversation.
    world.step_inline_dialogue(true, false, false);
    let mut guard = 0;
    while !world.inline_dialogue.as_ref().unwrap().menu_active() {
        assert!(
            !world.inline_dialogue.as_ref().unwrap().is_done(),
            "menu re-emission must survive the wrap rule"
        );
        world.step_inline_dialogue(false, false, false);
        guard += 1;
        assert!(guard < 80, "menu never re-emitted");
    }
}

/// Build the A/B menu script used by the inline-dialogue tests: prompt "Hi",
/// a 2-option picker whose option A branch SETs system flag 5 and option B SETs
/// flag 6, each followed by a reply + conversation-end terminator.
fn ab_menu_inline_script() -> Vec<u8> {
    let mut b = vec![0x1F, b'H', b'i', 0x00];
    let open = b.len();
    b.push(0x27);
    let entries_at = b.len();
    b.extend_from_slice(&[0, 0, 0, 0]);
    b.push(0x24);
    b.extend_from_slice(&[0x1F, b'A', 0x00]);
    b.extend_from_slice(&[0x1F, b'B', 0x00]);
    let branch0 = b.len();
    b.extend_from_slice(&[0x50, 0x05, 0x1F, b'a', 0x00, 0x00]);
    let branch1 = b.len();
    b.extend_from_slice(&[0x50, 0x06, 0x1F, b'b', 0x00, 0x00]);
    let j0 = (branch0 as i32 - (open as i32 + 1)) as i16;
    let j1 = (branch1 as i32 - (open as i32 + 1 + 2)) as i16;
    b[entries_at..entries_at + 2].copy_from_slice(&j0.to_le_bytes());
    b[entries_at + 2..entries_at + 4].copy_from_slice(&j1.to_le_bytes());
    b
}

#[test]
fn vm_dialogue_tick_executes_branch_through_field_vm() {
    // Drive the inline-script runner through the LIVE `World::tick` field path:
    // `use_vm_dialogue` + a `current_dialog` request + pad edges. Selecting
    // option B must run its branch's SET (flag 6) through the field VM.
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.use_vm_dialogue = true;
    world.current_dialog = Some(DialogRequest {
        text_id: 0,
        inline: ab_menu_inline_script(),
        world_x: 0,
        world_z: 0,
        depth_id: 0,
    });

    // Tick (no input) until the menu is awaiting a choice.
    let mut guard = 0;
    while !world
        .inline_dialogue
        .as_ref()
        .is_some_and(|d| d.menu_active())
    {
        world.set_pad(0);
        let _ = world.tick();
        guard += 1;
        assert!(guard < 60, "menu never became active through tick");
    }
    // Down edge → option B; Cross edge → confirm.
    world.set_pad(input::PadButton::Down.mask());
    let _ = world.tick();
    world.set_pad(0);
    let _ = world.tick();
    world.set_pad(input::PadButton::Cross.mask());
    let _ = world.tick();
    // Tick out the branch + reply with no further input.
    let mut guard = 0;
    while !world.system_flag_test(6) {
        world.set_pad(0);
        let _ = world.tick();
        guard += 1;
        assert!(guard < 60, "branch SET flag 6 never landed through tick");
    }
    assert!(
        !world.system_flag_test(5),
        "option A branch must not have run"
    );
}

#[test]
fn inline_dialogue_raw_0x21_ends_the_conversation() {
    // Retail's run-to-next-text helper (`FUN_8003CF7C`) executes a raw
    // `0x21` byte and then stops, returning it to the dialog SM as
    // "conversation over" - even when more script (here another text
    // segment) follows. The engine loop must not run past it.
    let mut b = vec![0x50, 0x05]; // SET system flag 5 (prologue op runs)
    b.push(0x21); // raw NOP: executed, then the conversation ends
    b.extend_from_slice(&[0x1F, b'x', 0x00]); // unreachable text segment

    let mut world = World::new();
    world.start_inline_dialogue(b);
    world.step_inline_dialogue(false, false, false);

    let id = world.inline_dialogue.as_ref().unwrap();
    assert!(id.is_done(), "0x21 ends the conversation");
    assert!(id.panel.is_none(), "the trailing segment never opened");
    assert!(world.system_flag_test(5), "ops before the 0x21 still ran");
}

#[test]
fn inline_dialogue_extended_0xa1_nop_runs_through() {
    // The retail stop compares the RAW byte (`bVar1 == 0x21`): an extended
    // `0xA1` NOP is executed and the run continues to the next segment.
    let b = vec![0xA1, 0xF8, 0x1F, b'y', 0x00];

    let mut world = World::new();
    world.start_inline_dialogue(b);
    world.step_inline_dialogue(false, false, false);

    let id = world.inline_dialogue.as_ref().unwrap();
    assert!(!id.is_done(), "0xA1 is not a conversation end");
    assert!(id.panel.is_some(), "the segment after the 0xA1 opened");
}
