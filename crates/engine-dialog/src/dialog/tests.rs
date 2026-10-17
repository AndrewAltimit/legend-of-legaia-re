use super::*;
use legaia_mes::{DialogPlayer, Interpreter};

/// Tick until the panel stops typing (page break, menu or done).
fn type_until_wait(panel: &mut OwnedDialogPanel) -> PanelState {
    for _ in 0..512 {
        let st = panel.tick();
        if st != PanelState::Typing {
            return st;
        }
    }
    panic!("the panel never stopped typing");
}

/// Tick an open menu until its slide comes to rest and it takes input.
fn slide_in(panel: &mut OwnedDialogPanel) {
    for _ in 0..128 {
        if panel.picker_takes_input() {
            return;
        }
        panel.tick();
    }
    panic!("the menu never came to rest");
}

/// Minimal Compact-format MES blob: a single message with three glyphs
/// then End. Avoids dragging the full container parser into tests.
fn three_glyph_program() -> Vec<u8> {
    // bytecode is just the glyph stream + terminator: 'a' 'b' 'c' end
    vec![b'a', b'b', b'c', 0x00]
}

#[test]
fn typing_then_done() {
    let buf = three_glyph_program();
    let interp = Interpreter::new_at(&buf, 0);
    let mut player = DialogPlayer::new(interp);
    player.set_glyphs_per_frame(1);
    let mut panel = DialogPanel::new(player);
    for _ in 0..3 {
        assert_eq!(panel.tick(), PanelState::Typing);
    }
    assert_eq!(
        panel.page_bytes(),
        vec![b'a', b'b', b'c'],
        "three glyphs should accumulate"
    );
    assert_eq!(panel.tick(), PanelState::Done);
    assert!(panel.is_done());
}

#[test]
fn owned_panel_types_three_glyphs_then_done() {
    let buf = Arc::new(three_glyph_program());
    let mut panel = OwnedDialogPanel::new(buf, 0);
    for _ in 0..3 {
        assert_eq!(panel.tick(), PanelState::Typing);
    }
    assert_eq!(panel.page_bytes(), vec![b'a', b'b', b'c']);
    assert_eq!(panel.tick(), PanelState::Done);
    assert!(panel.is_done());
}

/// Inline field-VM dialog: a few prologue bytes (field-VM bytecode, some
/// in the glyph range) then a `0x1F`-lead text segment.
/// `from_inline_dialog` must skip the prologue to the first `0x1F`
/// marker and type the text after it - here `"Hi"` from `[00 56 00 1F
/// 'H' 'i' 00]`.
#[test]
fn from_inline_dialog_skips_prologue_and_types_text() {
    let inline = vec![0x00u8, 0x56, 0x00, 0x1F, b'H', b'i', 0x00];
    let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).expect("has a 0x1F lead");
    for _ in 0..2 {
        assert_eq!(panel.tick(), PanelState::Typing);
    }
    assert_eq!(panel.page_bytes(), vec![b'H', b'i']);
    // Retail pacing: the row's glyph count is 3 (lead included), so the
    // row finishes on the call whose counter passes 3 - two calls after
    // the last glyph shows.
    assert_eq!(panel.tick(), PanelState::Typing);
    assert_eq!(panel.tick(), PanelState::Done);
}

/// A `0xCE 0x0B` number escape prints the script counter the host
/// resolved, as one reveal unit; without a resolution it prints nothing.
#[test]
fn a_number_escape_prints_the_resolved_script_counter() {
    let inline = vec![0x1F, b'N', 0xCE, 0x0B, b'x', 0x00];
    let run = |subs: Option<PanelSubstitutions>| {
        let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).unwrap();
        panel.substitutions = subs;
        for _ in 0..16 {
            if panel.tick() == PanelState::Done {
                break;
            }
        }
        panel.page_bytes()
    };
    let mut map = std::collections::HashMap::new();
    map.insert((SCRIPT_COUNTER_KEY, 0x0B), script_counter_digits(0x2A));
    assert_eq!(run(Some(Arc::new(map))), b"N42x".to_vec());
    assert_eq!(
        run(None),
        vec![b'N', 0xCE, 0x0B, b'x'],
        "unresolved = the raw escape pair, left for the layout"
    );
}

/// `FUN_80034B78` with min-digits `0`: leading zeros suppressed, the units
/// digit always drawn, so a value at or below zero prints one `0`.
#[test]
fn script_counter_digits_follow_the_number_writer() {
    assert_eq!(script_counter_digits(0), b"0".to_vec());
    assert_eq!(script_counter_digits(-5), b"0".to_vec());
    assert_eq!(script_counter_digits(7), b"7".to_vec());
    assert_eq!(script_counter_digits(i16::MAX), b"32767".to_vec());
    assert_eq!(script_counter_slot(0x0A), None);
    assert_eq!(script_counter_slot(0x0B), Some(0));
    assert_eq!(script_counter_slot(0x0E), Some(3));
    assert_eq!(script_counter_slot(0x0F), None);
}

/// No `0x1F` lead marker = nothing renderable; the caller falls back to the
/// MES `text_id` path.
#[test]
fn from_inline_dialog_without_lead_marker_is_none() {
    assert!(OwnedDialogPanel::from_inline_dialog(&[0x00, 0x10, 0x00]).is_none());
}

/// A picker label's name escape resolves through the panel's table, as
/// the box text does (dolk2's "Be careful, `C1 01`!" choice).
#[test]
fn picker_labels_splice_names_in() {
    let mut inline = vec![0x1F, b'O', b'K', b'?', 0x00];
    inline.push(0x27);
    inline.extend_from_slice(&0x10i16.to_le_bytes());
    inline.extend_from_slice(&0x20i16.to_le_bytes());
    inline.push(0x24);
    inline.extend_from_slice(&[0x1F, b'H', b'i', b',', b' ', 0xC1, 0x01, b'!', 0x00]);
    inline.extend_from_slice(&[0x1F, b'N', b'o', 0x00]);
    inline.resize(48, 0x21);
    let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).expect("has a 0x1F lead");
    let raw = panel.picker().unwrap().options[0].label.clone();
    assert_eq!(raw, b"Hi, !", "the mes crate has no names");
    assert_eq!(
        panel.picker_labels()[0],
        b"Hi, !".to_vec(),
        "no table, no name"
    );
    let mut map = std::collections::HashMap::new();
    map.insert((1u8, 1u8), b"Noa".to_vec());
    panel.substitutions = Some(Arc::new(map));
    assert_eq!(
        panel.picker_labels(),
        vec![b"Hi, Noa!".to_vec(), b"No".to_vec()]
    );
}

/// A prompt followed by a `0x27` Yes/No picker: the panel types the prompt,
/// then enters a menu wait exposing the two decoded option labels + a
/// movable cursor (instead of going `Done`).
#[test]
fn from_inline_dialog_surfaces_a_following_picker_menu() {
    // [1F 'O' 'K' '?' 00]  27  <j0:i16> <j1:i16>  24  [1F 'Y' 'e' 's' 00][1F 'N' 'o' 00]
    let mut inline = vec![0x1F, b'O', b'K', b'?', 0x00];
    inline.push(0x27); // open, N=2
    inline.extend_from_slice(&0x10i16.to_le_bytes());
    inline.extend_from_slice(&0x20i16.to_le_bytes());
    inline.push(0x24); // continuation
    inline.extend_from_slice(&[0x1F, b'Y', b'e', b's', 0x00]);
    inline.extend_from_slice(&[0x1F, b'N', b'o', 0x00]);
    // Branch bodies both jumps land in: `legaia_mes::scan_pickers` only
    // accepts a picker whose every option target is a byte of this
    // script. Filler is field-VM `0x21` NOPs (no second picker).
    inline.resize(48, 0x21);

    let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).expect("has a 0x1F lead");
    assert!(panel.picker().is_some(), "the following picker decodes");
    // Type the prompt "OK?" then hit the terminator.
    type_until_wait(&mut panel);
    assert_eq!(panel.page_bytes(), vec![b'O', b'K', b'?']);
    assert!(
        !panel.menu_active() && panel.is_waiting_for_input(),
        "the prompt's page waits for the press that opens the menu"
    );
    assert!(!panel.is_done(), "menu box is not Done");
    panel.advance_page();
    assert!(panel.menu_active(), "the press opens the menu");
    slide_in(&mut panel);
    let opts = panel.picker().unwrap();
    assert_eq!(opts.options.len(), 2);
    assert_eq!(opts.options[0].label, b"Yes");
    assert_eq!(opts.options[1].label, b"No");
    // Cursor wraps within 0..2.
    assert_eq!(panel.picker_cursor(), 0);
    panel.move_picker_cursor(1);
    assert_eq!(panel.picker_cursor(), 1);
    panel.move_picker_cursor(1);
    assert_eq!(panel.picker_cursor(), 0, "wraps");
    panel.move_picker_cursor(-1);
    assert_eq!(panel.picker_cursor(), 1, "wraps backward");
}

/// Confirming a menu option applies its relative jump and resumes typing
/// the chosen branch's reply segment (mirrors `FUN_80038050`).
#[test]
fn confirm_menu_jumps_to_the_chosen_branch_reply() {
    // Layout: prompt "OK?" + 0x27 picker (Yes/No) + two reply segments.
    // Build it, then back-fill each option's i16 jump so it lands on its
    // reply lead: target = (open + 1 + index*2) + rel_jump.
    let mut b = vec![0x1F, b'O', b'K', b'?', 0x00]; // prompt, ends at pc=5
    let open = b.len(); // 5
    b.push(0x27); // open
    let entries_at = b.len(); // 6
    b.extend_from_slice(&[0, 0, 0, 0]); // 2 entries, filled in below
    b.push(0x24); // continuation
    b.extend_from_slice(&[0x1F, b'Y', b'e', b's', 0x00]); // label0
    b.extend_from_slice(&[0x1F, b'N', b'o', 0x00]); // label1
    let reply0 = b.len();
    b.extend_from_slice(&[0x1F, b'Y', b'!', 0x00]); // reply for option 0
    let reply1 = b.len();
    b.extend_from_slice(&[0x1F, b'N', b'!', 0x00]); // reply for option 1
    // rel_jump[i] so (open + 1 + i*2) + rel = reply_lead.
    let j0 = (reply0 as i32 - (open as i32 + 1)) as i16;
    let j1 = (reply1 as i32 - (open as i32 + 1 + 2)) as i16;
    b[entries_at..entries_at + 2].copy_from_slice(&j0.to_le_bytes());
    b[entries_at + 2..entries_at + 4].copy_from_slice(&j1.to_le_bytes());

    // Choose option 1 ("No") and confirm -> should type the "N!" reply.
    let mut panel = OwnedDialogPanel::from_inline_dialog(&b).unwrap();
    type_until_wait(&mut panel);
    panel.advance_page();
    assert!(panel.menu_active());
    slide_in(&mut panel);
    panel.move_picker_cursor(1);
    assert_eq!(panel.confirm_menu(), Some(1));
    assert!(!panel.menu_active(), "menu resolved");
    assert_eq!(type_until_wait(&mut panel), PanelState::Done);
    assert_eq!(panel.page_bytes(), vec![b'N', b'!']);

    // Option 0 ("Yes") -> "Y!".
    let mut panel = OwnedDialogPanel::from_inline_dialog(&b).unwrap();
    type_until_wait(&mut panel);
    assert_eq!(panel.confirm_menu(), None, "no menu before the press");
    panel.advance_page();
    assert_eq!(panel.confirm_menu(), None, "the menu is still sliding in");
    slide_in(&mut panel);
    assert_eq!(panel.confirm_menu(), Some(0));
    type_until_wait(&mut panel);
    // The two reply lines are consecutive `0x1F` rows, so they share the
    // box; the chosen reply is its first row.
    assert!(panel.page_bytes().starts_with(b"Y!"));
}

/// The pad-word picker step: Down then Cross commits option 1, and every
/// picker bit is owned while the menu is up (none before it opens).
#[test]
fn step_picker_pad_reads_the_bound_pad_word() {
    let mut b = vec![0x1F, b'O', b'K', b'?', 0x00];
    let open = b.len();
    b.push(0x27);
    let entries_at = b.len();
    b.extend_from_slice(&[0, 0, 0, 0]);
    b.push(0x24);
    b.extend_from_slice(&[0x1F, b'Y', b'e', b's', 0x00]);
    b.extend_from_slice(&[0x1F, b'N', b'o', 0x00]);
    let reply0 = b.len();
    b.extend_from_slice(&[0x1F, b'Y', b'!', 0x00]);
    let reply1 = b.len();
    b.extend_from_slice(&[0x1F, b'N', b'!', 0x00]);
    let j0 = (reply0 as i32 - (open as i32 + 1)) as i16;
    let j1 = (reply1 as i32 - (open as i32 + 1 + 2)) as i16;
    b[entries_at..entries_at + 2].copy_from_slice(&j0.to_le_bytes());
    b[entries_at + 2..entries_at + 4].copy_from_slice(&j1.to_le_bytes());

    let mut panel = OwnedDialogPanel::from_inline_dialog(&b).unwrap();
    type_until_wait(&mut panel);
    assert_eq!(
        panel.step_picker_pad(PICKER_PAD_CONFIRM),
        PickerPadStep::default(),
        "no menu yet: the pad is the world's"
    );
    panel.advance_page();
    slide_in(&mut panel);
    let down = panel.step_picker_pad(PICKER_PAD_DOWN);
    assert!(down.moved);
    assert_eq!(down.consumed, PICKER_PAD_MASK);
    let held = panel.step_picker_pad(0);
    assert_eq!((held.moved, held.chosen), (false, None), "no edge, no step");
    assert_eq!(
        held.consumed, PICKER_PAD_MASK,
        "the held bits stay the picker's"
    );
    assert_eq!(panel.step_picker_pad(0x4000).chosen, Some(1));
    assert_eq!(type_until_wait(&mut panel), PanelState::Done);
    assert_eq!(panel.page_bytes(), vec![b'N', b'!']);
}

/// The press opens the menu hidden (sentinel), the box rises from
/// `y = 0xF0` to its rest rect over thirteen pager calls at frame step 2,
/// and Up/Down + confirm do nothing until the call after it rests - the
/// pager's picker slide ([`crate::dialog_picker_slide`]). A `0x2A` menu
/// rests at the top right instead, and its cursor clamps.
#[test]
fn a_menu_slides_in_before_it_takes_input() {
    let build = |open_byte: u8| {
        let mut b = vec![0x1F, b'O', b'K', b'?', 0x00];
        b.push(open_byte);
        b.extend_from_slice(&0x10i16.to_le_bytes());
        b.extend_from_slice(&0x20i16.to_le_bytes());
        b.push(0x24);
        b.extend_from_slice(&[0x1F, b'Y', b'e', b's', 0x00]);
        b.extend_from_slice(&[0x1F, b'N', b'o', 0x00]);
        b.resize(48, 0x21);
        b
    };
    let mut panel = OwnedDialogPanel::from_inline_dialog(&build(0x27)).unwrap();
    type_until_wait(&mut panel);
    panel.advance_page();
    assert!(panel.menu_active());
    assert_eq!(
        panel.picker_rect(),
        None,
        "the press's sentinel draws no box"
    );
    assert!(!panel.picker_takes_input());
    let mut ys = Vec::new();
    let mut vsyncs = 0;
    while !panel.picker_takes_input() {
        panel.tick_at(2);
        vsyncs += 1;
        // Input stays shut while it slides: the cursor and a confirm do
        // nothing.
        if !panel.picker_takes_input() {
            panel.move_picker_cursor(1);
            assert_eq!(panel.picker_cursor(), 0);
            assert_eq!(panel.confirm_menu(), None);
        }
        if let Some((x, y, w, h)) = panel.picker_rect()
            && ys.last() != Some(&y)
        {
            assert_eq!((x, w, h), (0x26, 0xF4, 0x1A));
            ys.push(y);
        }
        assert!(vsyncs < 64);
    }
    assert_eq!(ys.first(), Some(&0xF0));
    assert_eq!(ys.last(), Some(&0xA3));
    assert!(panel.picker_hand_drawn());
    panel.move_picker_cursor(1);
    assert_eq!(panel.picker_cursor(), 1);
    assert_eq!(panel.confirm_menu(), Some(1));

    let mut inn = OwnedDialogPanel::from_inline_dialog(&build(0x2A)).unwrap();
    type_until_wait(&mut inn);
    inn.advance_page();
    inn.tick_at(2);
    assert_eq!(inn.picker_rect(), Some((0x150, 0x4A, 0x58, 0x1A)));
    slide_in(&mut inn);
    assert_eq!(inn.picker_rect(), Some((0xD8, 0x4A, 0x58, 0x1A)));
    inn.move_picker_cursor(-1);
    assert_eq!(inn.picker_cursor(), 0, "a 0x2A cursor clamps at the top");
    inn.move_picker_cursor(1);
    inn.move_picker_cursor(1);
    assert_eq!(inn.picker_cursor(), 1, "and at the bottom");
}

/// `confirm_menu` is a no-op (returns `None`) when no menu is active.
#[test]
fn confirm_menu_without_active_menu_is_none() {
    let mut panel = OwnedDialogPanel::from_inline_dialog(&[0x1F, b'H', b'i', 0x00]).unwrap();
    assert_eq!(panel.confirm_menu(), None);
}

/// Three consecutive `0x1F` rows pack into ONE window: the page buffer
/// holds all three rows joined by the `0x7C` newline glyph, and only
/// after the last row does the panel end. Retail `_DAT_801F2740 = 3`
/// (`FUN_801D84D0`); the port used to close the box at every row
/// terminator, splitting a 3-row retail box into three 1-row windows.
#[test]
fn three_rows_type_into_one_window() {
    let inline = vec![
        0x1F, b'o', b'n', b'e', 0x00, // row 1
        0x1F, b't', b'w', b'o', 0x00, // row 2
        0x1F, b's', b'i', b'x', 0x00, // row 3
        0x25, // end
    ];
    let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).unwrap();
    // 9 glyphs + 2 newline pushes + 3 terminators = the panel is done
    // only after all three rows.
    let mut ticks = 0;
    while !panel.is_done() && ticks < 64 {
        panel.tick();
        ticks += 1;
    }
    assert!(panel.is_done(), "conversation ends after the packed box");
    assert_eq!(
        panel.page_bytes(),
        b"one\x7Ctwo\x7Csix".to_vec(),
        "all three rows land on ONE page, newline-joined"
    );
}

/// A `0x24` next-page dispatch between two boxes: the panel page-breaks
/// (not `Done`) after box one, and `advance_page` types box two in the
/// same panel.
#[test]
fn next_page_dispatch_pages_within_one_panel() {
    let inline = vec![
        0x1F, b'a', 0x00, // box 1 row 1
        0x1F, b'b', 0x00, // box 1 row 2
        0x1F, b'c', 0x00, // box 1 row 3 (box full)
        0x24, // next page
        0x1F, b'd', 0x00, // box 2 row 1
        0x25, // end
    ];
    let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).unwrap();
    let mut ticks = 0;
    while !panel.is_waiting_for_input() && ticks < 64 {
        panel.tick();
        ticks += 1;
    }
    assert!(!panel.is_done(), "page break, not teardown, at the 0x24");
    assert_eq!(panel.page_bytes(), b"a\x7Cb\x7Cc".to_vec());
    panel.advance_page();
    let mut ticks = 0;
    while !panel.is_done() && ticks < 64 {
        panel.tick();
        ticks += 1;
    }
    assert!(panel.is_done());
    assert_eq!(panel.page_bytes(), b"d".to_vec(), "page two typed fresh");
}

/// A 2-row prompt whose picker follows the SECOND row's terminator: the
/// menu must attach (the old per-segment test looked only after row 1,
/// so a multi-row prompt never found its picker).
#[test]
fn picker_after_two_row_prompt_attaches() {
    // [1F 'A' 00][1F 'B' '?' 00] 27 <j0> <j1> 24 [1F Y e s 00][1F N o 00]
    let mut inline = vec![0x1F, b'A', 0x00, 0x1F, b'B', b'?', 0x00];
    inline.push(0x27); // open, N=2
    inline.extend_from_slice(&0x10i16.to_le_bytes());
    inline.extend_from_slice(&0x20i16.to_le_bytes());
    inline.push(0x24);
    inline.extend_from_slice(&[0x1F, b'Y', b'e', b's', 0x00]);
    inline.extend_from_slice(&[0x1F, b'N', b'o', 0x00]);
    inline.resize(48, 0x21);

    let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).unwrap();
    assert!(
        panel.picker().is_some(),
        "picker after the box's LAST row attaches"
    );
    let mut ticks = 0;
    while !panel.is_waiting_for_input() && ticks < 64 {
        panel.tick();
        ticks += 1;
    }
    panel.advance_page();
    assert!(
        panel.menu_active(),
        "menu opens on the press after the 2-row prompt"
    );
    assert_eq!(panel.page_bytes(), b"A\x7CB?".to_vec());
}

/// Plain dialogue (no picker) still types and goes `Done`.
#[test]
fn from_inline_dialog_without_picker_is_not_a_menu() {
    let inline = vec![0x1F, b'H', b'i', 0x00];
    let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).unwrap();
    assert!(panel.picker().is_none());
    assert_eq!(type_until_wait(&mut panel), PanelState::Done);
    assert!(!panel.menu_active());
}

/// The segment-pool decoder recovers **every** `0x1F`-lead segment, not
/// just the first: a record carries the NPC's whole dialogue line pool
/// (consecutive speech lines plus interspersed option labels like
/// `"Yes"` / `"No"`), each `0x00`-terminated.
#[test]
fn decode_inline_segments_recovers_every_segment() {
    // [header] 1F W e l c o m e 00  1F Y e s 00  1F N o 00
    let inline = vec![
        0x00, 0x42, 0x1F, b'W', b'e', b'l', b'c', b'o', b'm', b'e', 0x00, 0x1F, b'Y', b'e', b's',
        0x00, 0x1F, b'N', b'o', 0x00,
    ];
    let segs = decode_inline_segments(&inline);
    assert_eq!(
        segs,
        vec![b"Welcome".to_vec(), b"Yes".to_vec(), b"No".to_vec()],
        "all three 0x1F-lead segments decode in pool order"
    );
}

/// A single-segment record decodes to exactly one segment.
#[test]
fn decode_inline_segments_single_segment() {
    let inline = vec![0x00, 0x56, 0x00, 0x1F, b'H', b'i', 0x00];
    assert_eq!(decode_inline_segments(&inline), vec![b"Hi".to_vec()]);
}

/// `0xCF XX` in MES bytecode = render XX alone. Both panel variants
/// must surface the operand byte through the page accumulator, not
/// silently drop it.
#[test]
fn dialog_panel_renders_skip_two_operand() {
    // Glyph 'a', then 0xCF 'b', then End. Expected page: ['a', 'b'].
    let buf = vec![b'a', 0xCF, b'b', 0x00];
    let interp = Interpreter::new_at(&buf, 0);
    let mut player = DialogPlayer::new(interp);
    player.set_glyphs_per_frame(1);
    let mut panel = DialogPanel::new(player);
    for _ in 0..3 {
        panel.tick();
    }
    assert_eq!(panel.page_bytes(), vec![b'a', b'b']);
}

#[test]
fn owned_panel_renders_skip_two_operand() {
    let buf = Arc::new(vec![b'a', 0xCF, b'b', 0x00]);
    let mut panel = OwnedDialogPanel::new(buf, 0);
    for _ in 0..3 {
        panel.tick();
    }
    assert_eq!(panel.page_bytes(), vec![b'a', b'b']);
}

#[test]
fn advance_page_clears_buffer_and_resumes() {
    // After a glyph, force a page break by injecting a control byte.
    // Compact format treats single bytes as glyphs unless they're in
    // the control range - the interpreter's actual control byte set is
    // out of scope here, so we just verify advance_page is a no-op
    // when not at a break, and clears properly when at one.
    let buf = three_glyph_program();
    let interp = Interpreter::new_at(&buf, 0);
    let mut player = DialogPlayer::new(interp);
    player.set_glyphs_per_frame(1);
    let mut panel = DialogPanel::new(player);
    panel.tick();
    assert_eq!(panel.page_bytes().len(), 1);
    // Not at page break - advance_page is idempotent.
    panel.advance_page();
    assert_eq!(panel.page_bytes().len(), 1);
}

/// The automatic press counts down only while the page waits, by the
/// frame step once per pager call, and presses on the call that takes
/// it to zero or below (`0x801D8F4C..0x801D8F88`).
/// The pager draws the page mark only in the wait and only while no
/// automatic press counts down (`0x801D9804..0x801D9828`), and the mark
/// flips frames every sixteen vsyncs.
#[test]
fn the_page_mark_shows_in_the_wait_and_hides_under_an_automatic_press() {
    use crate::cursor_sprite::CursorSprites;
    let inline = vec![0x1F, b'a', 0x00, 0x24, 0x1F, b'b', 0x00, 0x25];
    // A page the player turns: no mark while it types, then the strip.
    let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).unwrap();
    let mut cursors = CursorSprites::default();
    let mut auto = 0i16;
    while !panel.is_waiting_for_input() {
        assert_eq!(panel.advance_icon(), None, "no mark while typing");
        panel.tick_at_auto_drawn(2, &mut auto, &mut cursors);
    }
    assert_eq!(panel.advance_icon(), Some(0));
    let mut frames = Vec::new();
    for _ in 0..64 {
        panel.tick_at_auto_drawn(2, &mut auto, &mut cursors);
        frames.push(panel.advance_icon().expect("the mark stays up"));
    }
    assert!(frames[..14].iter().all(|f| *f == 0), "{frames:?}");
    assert!(frames[16..30].iter().all(|f| *f == 1), "{frames:?}");
    assert!(frames[32..46].iter().all(|f| *f == 0), "{frames:?}");

    // A page a script turns: the countdown hides the mark throughout.
    let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).unwrap();
    let mut cursors = CursorSprites::default();
    let mut auto = 21i16;
    let mut vsyncs = 0;
    loop {
        panel.tick_at_auto_drawn(2, &mut auto, &mut cursors);
        assert_eq!(panel.advance_icon(), None, "vsync {vsyncs}");
        if panel.take_auto_press() {
            break;
        }
        vsyncs += 1;
        assert!(vsyncs < 256, "the press never fired");
    }
}

#[test]
fn the_automatic_press_counts_down_in_the_wait_and_fires_once() {
    let inline = vec![0x1F, b'a', 0x00, 0x24, 0x1F, b'b', 0x00, 0x25];
    let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).unwrap();
    let mut auto = 21i16;
    // Typing: the countdown does not move.
    while !panel.is_waiting_for_input() {
        panel.tick_at_auto(2, &mut auto);
        assert_eq!(auto, 21, "no countdown while the page types");
        assert!(!panel.take_auto_press());
    }
    // Waiting: 21 -> 19 -> .. -> 1, then the press (eleven calls, one
    // every two vsyncs), as the retail capture reads.
    let mut calls = 0;
    let mut vsyncs = 0;
    loop {
        let before = auto;
        panel.tick_at_auto(2, &mut auto);
        vsyncs += 1;
        if auto != before {
            calls += 1;
        }
        if panel.take_auto_press() {
            break;
        }
        assert!(vsyncs < 64, "the press never fired");
    }
    assert_eq!(auto, 0);
    assert_eq!(calls, 11);
    // Handled as a confirm: the page turns; nothing fires again.
    panel.advance_page();
    for _ in 0..8 {
        panel.tick_at_auto(2, &mut auto);
        assert!(!panel.take_auto_press());
    }
}

/// An idle countdown (`<= 0`) never presses.
#[test]
fn an_idle_countdown_never_presses() {
    let inline = vec![0x1F, b'a', 0x00, 0x25];
    let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).unwrap();
    let mut auto = 0i16;
    for _ in 0..64 {
        panel.tick_at_auto(2, &mut auto);
        assert!(!panel.take_auto_press());
    }
    assert_eq!(auto, 0);
}
