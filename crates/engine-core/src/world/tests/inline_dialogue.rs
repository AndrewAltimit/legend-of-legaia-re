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
