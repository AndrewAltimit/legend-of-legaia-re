use super::*;

#[test]
fn table_length_matches_jt_entry_count() {
    assert_eq!(SUBMODE_TABLE.len(), SUBMODE_JT_ENTRY_COUNT);
    assert_eq!(SUBMODE_JT_ENTRY_COUNT, 25);
}

#[test]
fn table_indices_are_dense_and_in_order() {
    for (i, row) in SUBMODE_TABLE.iter().enumerate() {
        assert_eq!(row.mode as usize, i, "row {i} mismatch");
    }
}

#[test]
fn from_u8_round_trips_in_range_bytes() {
    for b in 0..=0x18u8 {
        let mode = TitleOverlaySubMode::from_u8(b)
            .unwrap_or_else(|| panic!("byte 0x{b:02X} should decode"));
        assert_eq!(mode as u8, b);
    }
}

#[test]
fn from_u8_returns_none_for_out_of_range_bytes() {
    for b in 0x19..=0xFFu8 {
        assert!(
            TitleOverlaySubMode::from_u8(b).is_none(),
            "byte 0x{b:02X} should be out-of-range"
        );
        assert!(!TitleOverlaySubMode::is_in_range(b));
    }
}

#[test]
fn idle_handler_pc_equals_body_pc() {
    // The dispatcher's out-of-range path branches to SUBMODE_BODY_PC.
    // Mode 0x01 (Idle) shares that handler PC - it's a no-op exit.
    assert_eq!(
        TitleOverlaySubMode::Idle.handler_pc(),
        SUBMODE_BODY_PC,
        "Idle handler should equal the body tail PC"
    );
}

#[test]
fn well_known_modes_match_captured_pcs() {
    // Spot-check the four labelled modes against the JT entries
    // read out of `overlay_title.bin` at 0x801CF244.
    assert_eq!(TitleOverlaySubMode::Init.handler_pc(), 0x801D_D820);
    assert_eq!(TitleOverlaySubMode::Idle.handler_pc(), 0x801D_FC3C);
    assert_eq!(TitleOverlaySubMode::AttractIdle.handler_pc(), 0x801D_DB0C);
    assert_eq!(TitleOverlaySubMode::AttractDelay.handler_pc(), 0x801D_DA90);
}

#[test]
fn every_handler_pc_lives_inside_the_tick_function() {
    // Tick fn entry .. entry + size_bytes covers all handlers.
    let lo = SUBMODE_TICK_FN_ENTRY_PC;
    let hi = SUBMODE_TICK_FN_ENTRY_PC + SUBMODE_TICK_FN_SIZE_BYTES;
    for row in SUBMODE_TABLE {
        assert!(
            row.handler_pc >= lo && row.handler_pc < hi,
            "{} PC 0x{:08X} outside tick fn [{:08X}, {:08X})",
            row.label,
            row.handler_pc,
            lo,
            hi
        );
    }
}

#[test]
fn handler_pcs_are_unique_except_for_idle() {
    // Idle aliases the body tail; every other handler has its own
    // entry point. Build a histogram and check.
    let mut counts: std::collections::HashMap<u32, usize> = Default::default();
    for row in SUBMODE_TABLE {
        *counts.entry(row.handler_pc).or_insert(0) += 1;
    }
    // 24 unique handler PCs across 25 entries (only Idle's PC may
    // collide with something - and it doesn't collide with another
    // SUBMODE_TABLE entry in practice).
    assert_eq!(counts.len(), 25);
    for (pc, n) in counts {
        assert_eq!(n, 1, "PC 0x{pc:08X} appears {n} times in SUBMODE_TABLE");
    }
}

#[test]
fn state_field_addresses_decode_to_known_offsets() {
    // The sibling region uses negative displacements off `lui 0x801f`,
    // so reachable addresses live below STATE_BASE_ADDR; each
    // displacement decodes to one of these literal addresses.
    // Sanity-check the table.
    assert_eq!(
        STATE_HORIZ_SLIDER_X_ADDR,
        0x801F_0000u32.wrapping_sub(0xEB4)
    );
    assert_eq!(STATE_FADE_SWEEP_ADDR, 0x801F_0000u32.wrapping_sub(0xEA0));
    assert_eq!(
        STATE_ATTRACT_COUNTDOWN_ADDR,
        0x801F_0000u32.wrapping_sub(0xE94)
    );
    assert_eq!(STATE_FRAME_COUNTER_ADDR, 0x801F_0000u32.wrapping_sub(0xE90));
    assert_eq!(STATE_ALPHA_A_ADDR, 0x801F_0000u32.wrapping_sub(0xE70));
    assert_eq!(STATE_ALPHA_B_ADDR, 0x801F_0000u32.wrapping_sub(0xE6C));
    assert_eq!(STATE_ALPHA_C_ADDR, 0x801F_0000u32.wrapping_sub(0xE60));
    // The +offset fields land above STATE_BASE_ADDR.
    assert_eq!(STATE_SUBMODE_OFFSET, 0x0204);
    assert_eq!(STATE_BASE_ADDR + STATE_SUBMODE_OFFSET, 0x801F_0204);
}

#[test]
fn jt_address_matches_lui_addiu_disassembly() {
    // The dispatcher resolves the JT base as:
    //   lui   v0, 0x801D            ; v0 = 0x801D_0000
    //   addiu v0, v0, -0xDBC        ; v0 = 0x801D_0000 + 0xFFFF_F244 = 0x801C_F244
    // (`addiu` sign-extends -0xDBC to 0xFFFFF244).
    let lui_hi: u32 = 0x801D_0000;
    let addiu_lo: i32 = -0xDBC;
    let resolved = (lui_hi as i64 + addiu_lo as i64) as u32;
    assert_eq!(resolved, SUBMODE_JT_ADDR);
}

#[test]
fn padmask_constants_match_disassembled_andi_immediates() {
    // The dispatcher uses these immediates verbatim - they're the
    // `andi v0, X` operands the title-overlay pad-poll path emits.
    assert_eq!(PADMASK_CONFIRM_L1_CROSS, 0x0044);
    assert_eq!(PADMASK_CANCEL_L2_CIRCLE, 0x0021);
    assert_eq!(PADMASK_ANY_FACE_OR_L, 0x00F5);
    assert_eq!(PADMASK_START_L1_CROSS, 0x0844);
}

#[test]
fn countdown_reset_value_is_disassembled_literal() {
    // `li v0, 0x5DC` appears at line 376 (Init) and line 482 (AttractDelay).
    assert_eq!(COUNTDOWN_RESET_VALUE, 0x5DC);
}

#[test]
fn state_204_writes_cover_all_well_known_transitions() {
    // Every labelled mode emits at least one observed transition.
    // (Idle has no body and AttractIdle's "transition" is to master
    // game mode, not state[+0x204] - covered separately.)
    let froms: std::collections::BTreeSet<u8> = STATE_204_WRITES.iter().map(|w| w.from).collect();
    assert!(froms.contains(&0x00), "Init missing");
    assert!(froms.contains(&0x06), "LaunchGame (LaunchGame) missing");
    assert!(froms.contains(&0x11), "AttractDelay missing");
}

#[test]
fn state_204_writes_are_ordered_by_pc_and_unique() {
    // Sorted + dedup invariant - keeps the table easy to extend.
    let pcs: Vec<u32> = STATE_204_WRITES.iter().map(|w| w.pc).collect();
    let mut sorted = pcs.clone();
    sorted.sort();
    assert_eq!(pcs, sorted, "STATE_204_WRITES not sorted by PC");
    let unique: std::collections::HashSet<u32> = pcs.iter().copied().collect();
    assert_eq!(unique.len(), pcs.len(), "duplicate PCs in STATE_204_WRITES");
}

#[test]
fn every_204_write_lives_inside_the_tick_function() {
    let lo = SUBMODE_TICK_FN_ENTRY_PC;
    let hi = SUBMODE_TICK_FN_ENTRY_PC + SUBMODE_TICK_FN_SIZE_BYTES;
    for w in STATE_204_WRITES {
        assert!(
            w.pc >= lo && w.pc < hi,
            "0x{:08X} (from mode 0x{:02X}) outside tick fn [{:08X}, {:08X})",
            w.pc,
            w.from,
            lo,
            hi
        );
    }
}

#[test]
fn every_204_write_from_mode_is_in_range() {
    for w in STATE_204_WRITES {
        if w.from == SUBMODE_TAIL_SOURCE {
            continue;
        }
        assert!(
            TitleOverlaySubMode::is_in_range(w.from),
            "from-mode 0x{:02X} out of range",
            w.from
        );
        for extra in w.also_from {
            assert!(
                TitleOverlaySubMode::is_in_range(*extra),
                "also-from mode 0x{extra:02X} out of range"
            );
        }
    }
}

#[test]
fn every_static_target_mode_is_in_range() {
    for w in STATE_204_WRITES {
        for target in w.target.literals() {
            assert!(
                TitleOverlaySubMode::is_in_range(target),
                "from 0x{:02X} -> 0x{:02X} target out of range",
                w.from,
                target
            );
        }
    }
}

#[test]
fn master_game_mode_constants_align_with_cutscene_trigger() {
    use crate::cutscene_trigger;
    assert_eq!(MASTER_GAME_MODE_ADDR, cutscene_trigger::GAME_MODE_ADDR);
    assert_eq!(MASTER_GAME_MODE_STR_INIT, cutscene_trigger::STR_INIT_MODE);
}

#[test]
fn phase06_launch_game_pc_lives_inside_tick_function() {
    let lo = SUBMODE_TICK_FN_ENTRY_PC;
    let hi = SUBMODE_TICK_FN_ENTRY_PC + SUBMODE_TICK_FN_SIZE_BYTES;
    assert!(PHASE06_LAUNCH_GAME_PC >= lo && PHASE06_LAUNCH_GAME_PC < hi);
    // And the LaunchGame handler PC predates the launch-write PC (the
    // write happens inside LaunchGame's body).
    let phase06 = TitleOverlaySubMode::LaunchGame.handler_pc();
    assert!(
        phase06 < PHASE06_LAUNCH_GAME_PC,
        "LaunchGame handler 0x{phase06:08X} should precede launch write 0x{PHASE06_LAUNCH_GAME_PC:08X}"
    );
}

#[test]
fn new_game_boot_chain_constants() {
    // NEW GAME is the top menu row (index 0); the launch write sets the
    // field INIT mode (2), whose init handler reaches the field scene
    // initializer, which hands off to the field RUN mode (3).
    assert_eq!(MENU_INDEX_NEW_GAME, 0);
    assert_eq!(MENU_INDEX_STATE_OFFSET, 0x200);
    assert_eq!(MASTER_GAME_MODE_FIELD_LAUNCH, 0x02);
    assert_eq!(MASTER_GAME_MODE_FIELD_RUN, 0x03);
    // INIT precedes RUN, and both differ from the attract STR-FMV mode.
    const _: () = assert!(MASTER_GAME_MODE_FIELD_LAUNCH < MASTER_GAME_MODE_FIELD_RUN);
    assert_ne!(MASTER_GAME_MODE_FIELD_RUN, MASTER_GAME_MODE_STR_INIT);
    // The mode-2 init handler is SCUS-resident; the field scene
    // initializer it calls is overlay-resident (0x801C0000+).
    const _: () = assert!(MODE2_INIT_HANDLER_PC < 0x801C_0000);
    const _: () = assert!(FIELD_SCENE_INIT_PC >= 0x801C_0000);
}

// -- the executable half ------------------------------------------

fn menu() -> TitleMenuState {
    TitleMenuState::new()
}

#[test]
fn cursor_wraps_over_the_two_row_space() {
    let mut m = menu();
    assert_eq!(m.row(TITLE_MENU_ROWS), 0);
    let ev = m.step(PADMASK_CURSOR_NEXT, 0, 1, TITLE_MENU_ROWS);
    assert_eq!(ev[0], TitleMenuEvent::CursorMoved { row: 1 });
    assert_eq!(m.sfx, Some(TITLE_SFX_CURSOR_MOVE));
    // Down again wraps back to 0 - retail's `andi v1,v1,0x1`.
    let ev = m.step(PADMASK_CURSOR_NEXT, 0, 1, TITLE_MENU_ROWS);
    assert_eq!(ev[0], TitleMenuEvent::CursorMoved { row: 0 });
    // Up from 0 wraps to the last row.
    let ev = m.step(PADMASK_CURSOR_PREV, 0, 1, TITLE_MENU_ROWS);
    assert_eq!(ev[0], TitleMenuEvent::CursorMoved { row: 1 });
    // The counter never runs away from the row space.
    assert!(m.row_counter >= 0 && m.row_counter < TITLE_MENU_ROWS as i32);
}

#[test]
fn confirm_takes_every_bit_of_the_0x844_mask() {
    for bit in [0x0800u16, 0x0040, 0x0004] {
        assert_ne!(PADMASK_START_L1_CROSS & bit, 0, "{bit:#06x} is in the mask");
        let mut m = menu();
        let ev = m.step(bit, 0, 1, TITLE_MENU_ROWS);
        assert_eq!(ev, vec![TitleMenuEvent::Confirmed { row: 0 }]);
        assert_eq!(m.sfx, Some(TITLE_SFX_CONFIRM));
        assert_eq!(m.chosen_row, TITLE_ROW_NEW_GAME);
    }
    // A bit outside the mask confirms nothing.
    let mut m = menu();
    assert!(m.step(0x0010, 0, 1, TITLE_MENU_ROWS).is_empty());
}

#[test]
fn a_confirm_on_row_one_stashes_continue() {
    let mut m = menu();
    m.step(PADMASK_CURSOR_NEXT, 0, 1, TITLE_MENU_ROWS);
    let ev = m.step(PADMASK_START_L1_CROSS, 0, 1, TITLE_MENU_ROWS);
    assert_eq!(
        ev,
        vec![TitleMenuEvent::Confirmed {
            row: TITLE_ROW_CONTINUE
        }]
    );
    assert_eq!(m.chosen_row, TITLE_ROW_CONTINUE);
}

#[test]
fn the_last_sixteen_frames_accept_no_input() {
    let mut m = menu();
    m.countdown = ATTRACT_INPUT_FREEZE_BELOW - 1;
    // Neither the cursor nor the confirm is read below the band; the
    // countdown still runs, and a held pad still re-arms it.
    let ev = m.step(
        PADMASK_CURSOR_NEXT | PADMASK_START_L1_CROSS,
        0,
        1,
        TITLE_MENU_ROWS,
    );
    assert!(ev.is_empty(), "input read below the freeze band: {ev:?}");
    assert_eq!(m.row_counter, 0);
    assert_eq!(m.sfx, None);
    // One frame above the band the same word is read.
    let mut m = menu();
    m.countdown = ATTRACT_INPUT_FREEZE_BELOW;
    let ev = m.step(PADMASK_CURSOR_NEXT, 0, 1, TITLE_MENU_ROWS);
    assert_eq!(ev[0], TitleMenuEvent::CursorMoved { row: 1 });
}

#[test]
fn any_held_bit_re_arms_the_countdown() {
    let mut m = menu();
    m.countdown = 3;
    m.step(0, 0x0010, 1, TITLE_MENU_ROWS);
    assert_eq!(m.countdown, COUNTDOWN_RESET_VALUE as i32 - 1);
}

#[test]
fn the_countdown_fires_on_underflow_at_the_frame_scalar() {
    let mut m = menu();
    m.countdown = 1;
    assert!(m.step(0, 0, 1, TITLE_MENU_ROWS).is_empty());
    assert_eq!(m.countdown, 0);
    let ev = m.step(0, 0, 1, TITLE_MENU_ROWS);
    assert_eq!(ev, vec![TitleMenuEvent::AttractFired]);
    // A doubled frame scalar spends the countdown twice as fast.
    let mut m = menu();
    m.countdown = 4;
    m.step(0, 0, 2, TITLE_MENU_ROWS);
    assert_eq!(m.countdown, 2);
}

// -- the whole dispatcher ------------------------------------------

#[test]
fn the_table_holds_every_store_the_function_makes() {
    // A Capstone pass over PROT 0899 file +0xEB44 finds exactly 56
    // `sw <reg>,0x204(<base>)` in the 3026-instruction body.
    assert_eq!(STATE_204_WRITES.len(), 56);
}

#[test]
fn every_store_names_a_guard() {
    for w in STATE_204_WRITES {
        assert!(!w.guard.is_empty(), "0x{:08X} has no guard", w.pc);
    }
}

#[test]
fn the_three_cross_handler_stores_carry_their_second_source() {
    // Three handlers `j` into the middle of another handler's body,
    // so those stores fire from two sub-modes. Without the second
    // source the 0x04 / 0x05 / 0x13 cluster has no entry at all.
    let extra: std::collections::BTreeMap<u32, &[u8]> = STATE_204_WRITES
        .iter()
        .filter(|w| !w.also_from.is_empty())
        .map(|w| (w.pc, w.also_from))
        .collect();
    assert_eq!(extra.len(), 3, "expected exactly three aliased stores");
    assert_eq!(extra[&0x801D_E844], &[0x15][..]); // 0x15 -> 0x801DE838
    assert_eq!(extra[&0x801D_EF34], &[0x15][..]); // 0x15 -> 0x801DEF2C
    assert_eq!(extra[&0x801D_F484], &[0x0E][..]); // 0x0E -> 0x801DF47C
}

#[test]
fn no_handler_leaves_the_mode_graph() {
    // Every literal a store can leave in the selector is inside the
    // dispatcher's `sltiu v0,s2,0x19` window, so no handler can put
    // the tick into the out-of-range path by accident.
    for w in STATE_204_WRITES {
        for t in w.target.literals() {
            assert!(
                TitleOverlaySubMode::is_in_range(t),
                "0x{:08X} targets out-of-range 0x{t:02X}",
                w.pc
            );
        }
    }
}

#[test]
fn nothing_reaches_the_two_dead_modes_from_a_cold_boot() {
    let seen = cold_boot_reachable_modes();
    assert!(
        !seen[TitleOverlaySubMode::Idle as usize],
        "0x01 Idle is the out-of-range slot - no store writes 1"
    );
    assert!(
        !seen[TitleOverlaySubMode::TextMenu as usize],
        "0x02 is bypassed by the Init sentinel arm and again by the epilogue"
    );
    // And that is a property of the stores, not of the walk: no row
    // outside OVERWRITTEN_STORES targets either mode.
    for w in STATE_204_WRITES {
        if OVERWRITTEN_STORES.contains(&w.pc) {
            continue;
        }
        for t in w.target.literals() {
            assert_ne!(t, 0x01, "0x{:08X} writes Idle", w.pc);
            assert_ne!(t, 0x02, "0x{:08X} writes TextMenu", w.pc);
        }
    }
}

#[test]
fn every_store_fires_from_a_mode_a_cold_boot_can_be_in() {
    // Stronger than the mode-reachability test: not just "every mode is
    // reached" but "every one of the 56 stores is a live edge". A row
    // whose source mode a cold boot never enters is either mis-attributed
    // or evidence of a handler nothing dispatches.
    let seen = cold_boot_reachable_modes();
    for w in STATE_204_WRITES {
        if w.from == SUBMODE_TAIL_SOURCE {
            // The epilogue runs after every handler; its guarded rows
            // are covered by EPILOGUE_GUARD_MODES below.
            if let Some((_, m)) = EPILOGUE_GUARD_MODES.iter().find(|(pc, _)| *pc == w.pc) {
                // The two rows guarded on 0x02 are the mechanism that
                // makes 0x02 unreachable, so of course their guard mode
                // is not reachable - that is the point of them.
                assert!(
                    seen[*m as usize] || *m == 0x02,
                    "epilogue store 0x{:08X} is guarded on unreachable mode 0x{m:02X}",
                    w.pc
                );
            }
            continue;
        }
        let live = seen[w.from as usize] || w.also_from.iter().any(|m| seen[*m as usize]);
        if !live {
            // The only dead source is 0x02's own body: the entry word
            // keeps a retail boot out of that handler entirely, so its
            // two stores are edges nothing can take.
            assert_eq!(
                w.from, 0x02,
                "store 0x{:08X} fires only from unreachable mode 0x{:02X}",
                w.pc, w.from
            );
        }
    }
}

#[test]
fn every_other_mode_is_reachable_from_the_cold_boot_entry() {
    let seen = cold_boot_reachable_modes();
    for (mode, reached) in seen.iter().enumerate() {
        if mode == TitleOverlaySubMode::Idle as usize
            || mode == TitleOverlaySubMode::TextMenu as usize
        {
            continue;
        }
        assert!(
            *reached,
            "sub-mode 0x{mode:02X} ({}) unreachable from Init",
            SUBMODE_TABLE[mode].label
        );
    }
}

#[test]
fn the_attract_arm_lives_only_in_attract_idle() {
    // The countdown decrement + the two attract stores are inside
    // 0x10's handler extent, not the preamble and not the epilogue.
    let lo = TitleOverlaySubMode::AttractIdle.handler_pc();
    let hi = TitleOverlaySubMode::ContinueFadeIn.handler_pc();
    assert!(lo < SUBMODE_COUNTDOWN_DECR_PC && SUBMODE_COUNTDOWN_DECR_PC < hi);
    assert!(lo < crate::cutscene_trigger::TITLE_TICK_INLINE.mode_write_addr);
    assert!(crate::cutscene_trigger::TITLE_TICK_INLINE.mode_write_addr < hi);
    assert_eq!(ATTRACT_FMV_ID, 0);
}

#[test]
fn both_master_mode_two_writers_are_pinned() {
    // The load route writes it inside 0x16, the new-game route inside
    // 0x06; each is inside its own handler's extent.
    assert!(TitleOverlaySubMode::LaunchFade.handler_pc() < PHASE16_LOAD_LAUNCH_PC);
    assert!(PHASE16_LOAD_LAUNCH_PC < TitleOverlaySubMode::LaunchGame.handler_pc());
    assert!(TitleOverlaySubMode::LaunchGame.handler_pc() < PHASE06_LAUNCH_GAME_PC);
    const _: () = assert!(PHASE06_LAUNCH_GAME_PC < SUBMODE_BODY_PC);
}

// -- the executable dispatcher -------------------------------------

fn tick(state: &mut TitleTickState, pad: TitleTickPad) -> Vec<TitleTickEffect> {
    state.step(pad, TitleCardStatus::default())
}

#[test]
fn a_cold_boot_enters_attract_idle_through_the_delay_state() {
    // The entry word is what routes it, not a hard-coded mode: Init
    // reads `_DAT_8007BB00`, writes 0x11, and 0x11 hands to 0x10 once
    // its 8-per-frame accumulator is spent.
    let mut s = TitleTickState::cold_boot();
    assert_eq!(s.submode, TitleOverlaySubMode::Init as u8);
    let fx = tick(&mut s, TitleTickPad::from_edge(0));
    assert_eq!(s.submode, TitleOverlaySubMode::AttractDelay as u8);
    assert!(fx.contains(&TitleTickEffect::LoadTitleAssets));
    // The SCUS stager seeds the hold with 0x100 and 0x11 spends it at
    // 8 per frame; the hand-off fires on the frame that reads it
    // already at zero, so the menu comes up 33 frames later.
    for _ in 0..=(ATTRACT_DELAY_SEED / 8) {
        assert_eq!(s.submode, TitleOverlaySubMode::AttractDelay as u8);
        tick(&mut s, TitleTickPad::from_edge(0));
    }
    assert_eq!(s.submode, TitleOverlaySubMode::AttractIdle as u8);
    assert_eq!(s.countdown, COUNTDOWN_RESET_VALUE as i32);
    // 0x02 is never entered on the way.
    assert_ne!(s.submode, TitleOverlaySubMode::TextMenu as u8);
}

#[test]
fn a_zeroed_entry_word_is_the_only_way_into_the_text_menu() {
    // With the word down, Init leaves 0x02 and the epilogue does not
    // rewrite it - the graph retail never takes because `init.pak`
    // raises the word at 0x801CEB84.
    let mut s = TitleTickState::with_entry_word(0);
    let fx = tick(&mut s, TitleTickPad::from_edge(0));
    assert_eq!(s.submode, TitleOverlaySubMode::TextMenu as u8);
    assert!(!fx.contains(&TitleTickEffect::LoadTitleAssets));
    // Raise the word and the epilogue takes it straight to 0x10.
    s.entry_word = ENTRY_WORD_COLD_BOOT;
    tick(&mut s, TitleTickPad::from_edge(0));
    assert_eq!(s.submode, TitleOverlaySubMode::AttractIdle as u8);
}

#[test]
fn the_return_from_the_attract_still_takes_the_sentinel_arm() {
    let mut s = TitleTickState::with_entry_word(ENTRY_WORD_FROM_ATTRACT);
    let fx = tick(&mut s, TitleTickPad::from_edge(0));
    assert_eq!(s.submode, TitleOverlaySubMode::AttractDelay as u8);
    // Only the exact cold-boot value streams the title assets again.
    assert!(!fx.contains(&TitleTickEffect::LoadTitleAssets));
}

fn at_attract_idle() -> TitleTickState {
    let mut s = TitleTickState::cold_boot();
    // Init, then the SCUS-seeded 0x100/8 hold, then one more frame
    // to spend the pre-roll so the menu is live.
    for _ in 0..(2 + ATTRACT_DELAY_SEED / 8) {
        tick(&mut s, TitleTickPad::from_edge(0));
    }
    // The hold transitions on the frame that reads the accumulator
    // already at zero (`bgtz v1` at 0x801DDAB4), i.e. one frame past
    // the last decrement.
    tick(&mut s, TitleTickPad::from_edge(0));
    assert_eq!(s.submode, TitleOverlaySubMode::AttractIdle as u8);
    s
}

#[test]
fn the_new_game_row_confirms_into_the_launch_fade() {
    let mut s = at_attract_idle();
    assert_eq!(s.submode, TitleOverlaySubMode::AttractIdle as u8);
    let fx = tick(&mut s, TitleTickPad::from_edge(PADMASK_START_L1_CROSS));
    assert_eq!(s.submode, TitleOverlaySubMode::LaunchFade as u8);
    assert!(fx.contains(&TitleTickEffect::Sfx(TITLE_SFX_CONFIRM)));
}

#[test]
fn the_continue_row_confirms_into_the_fade_in_then_the_main_menu() {
    let mut s = at_attract_idle();
    tick(&mut s, TitleTickPad::from_edge(PADMASK_CURSOR_NEXT));
    assert_eq!(s.row_counter, TITLE_ROW_CONTINUE as i32);
    tick(&mut s, TitleTickPad::from_edge(PADMASK_START_L1_CROSS));
    assert_eq!(s.submode, TitleOverlaySubMode::ContinueFadeIn as u8);
    assert_eq!(s.menu_index, TITLE_ROW_CONTINUE as u32);
    // The fade-in ramps 0x80 per frame to 0x1000, then hands over.
    for _ in 0..0x40 {
        tick(&mut s, TitleTickPad::from_edge(0));
    }
    assert_eq!(s.submode, TitleOverlaySubMode::MainMenu as u8);
}

#[test]
fn the_launch_fade_hands_to_the_launcher_and_the_launcher_writes_mode_two() {
    let mut s = at_attract_idle();
    tick(&mut s, TitleTickPad::from_edge(PADMASK_START_L1_CROSS));
    assert_eq!(s.submode, TitleOverlaySubMode::LaunchFade as u8);
    let mut launched = false;
    for _ in 0..0x100 {
        for fx in tick(&mut s, TitleTickPad::from_edge(0)) {
            if fx == (TitleTickEffect::LaunchGame { from_load: false }) {
                launched = true;
            }
        }
        if launched {
            break;
        }
    }
    assert!(launched, "the NEW GAME route never wrote master mode 2");
    // And the launcher re-arms Init behind it.
    assert_eq!(s.submode, TitleOverlaySubMode::Init as u8);
}

#[test]
fn the_attract_fires_from_attract_idle_with_fmv_zero() {
    let mut s = at_attract_idle();
    s.countdown = 1;
    assert!(tick(&mut s, TitleTickPad::from_edge(0)).is_empty());
    let fx = tick(&mut s, TitleTickPad::from_edge(0));
    assert!(fx.contains(&TitleTickEffect::FireAttract { fmv_id: 0 }));
    assert_eq!(s.submode, TitleOverlaySubMode::AttractIdle as u8);
}

#[test]
fn the_last_sixteen_frames_of_the_countdown_take_no_confirm() {
    let mut s = at_attract_idle();
    s.countdown = ATTRACT_INPUT_FREEZE_BELOW - 1;
    tick(&mut s, TitleTickPad::from_edge(PADMASK_START_L1_CROSS));
    assert_eq!(
        s.submode,
        TitleOverlaySubMode::AttractIdle as u8,
        "input was read below the freeze band"
    );
}

#[test]
fn the_panel_slider_converges_on_0x2c_from_both_sides() {
    // Both epilogue arms clamp to the same value, so the "clamped
    // [0, 0x2C]" reading of `state[-0xeb4]` is wrong in its low half:
    // the decreasing arm floors at 0x2C, it does not run to 0.
    let mut s = TitleTickState::cold_boot();
    s.submode = TitleOverlaySubMode::Idle as u8;
    s.slider_dir = 1;
    s.slider_x = 0x100;
    for _ in 0..0x100 {
        tick(&mut s, TitleTickPad::from_edge(0));
    }
    assert_eq!(s.slider_x, 0x2C);
    s.slider_dir = 2;
    s.slider_x = 0;
    for _ in 0..0x100 {
        tick(&mut s, TitleTickPad::from_edge(0));
    }
    assert_eq!(s.slider_x, 0x2C);
}

#[test]
fn the_epilogue_cancel_arm_uses_the_handlers_own_target() {
    // 0x0C / 0x0D set `s3 = 0x14` and the epilogue applies it on the
    // 0x21 mask with cue 0x37.
    let mut s = TitleTickState::cold_boot();
    s.entry_word = 0;
    s.submode = TitleOverlaySubMode::LoadNotice as u8;
    let fx = tick(&mut s, TitleTickPad::from_edge(PADMASK_CANCEL_L2_CIRCLE));
    assert_eq!(s.submode, TitleOverlaySubMode::MainMenu as u8);
    assert!(fx.contains(&TitleTickEffect::Sfx(TITLE_SFX_CANCEL)));
}

#[test]
fn the_grid_cursor_wraps_over_the_five_by_three_slot_grid() {
    let mut s = TitleTickState::cold_boot();
    s.submode = TitleOverlaySubMode::SlotGrid as u8;
    s.cursor_x = GRID_COLUMNS - 1;
    s.cursor_y = GRID_ROWS - 1;
    tick(&mut s, TitleTickPad::from_edge(PADMASK_GRID_RIGHT));
    assert_eq!(s.cursor_x, 0);
    tick(&mut s, TitleTickPad::from_edge(PADMASK_CURSOR_NEXT));
    assert_eq!(s.cursor_y, 0);
    tick(&mut s, TitleTickPad::from_edge(PADMASK_GRID_LEFT));
    assert_eq!(s.cursor_x, GRID_COLUMNS - 1);
    tick(&mut s, TitleTickPad::from_edge(PADMASK_CURSOR_PREV));
    assert_eq!(s.cursor_y, GRID_ROWS - 1);
}

#[test]
fn the_second_argument_pre_selects_a_menu_row() {
    // The two Init arms nothing in retail's production caller uses:
    // `FUN_801E36A0` passes 0, but 1 / 2 land straight on 0x14 with
    // the row already stashed.
    for (arg, row) in [(1u32, 0u32), (2, 1)] {
        let mut s = TitleTickState::with_entry_word(0);
        s.arg1 = arg;
        tick(&mut s, TitleTickPad::from_edge(0));
        assert_eq!(s.submode, TitleOverlaySubMode::MainMenu as u8);
        assert_eq!(s.menu_index, row);
    }
}
