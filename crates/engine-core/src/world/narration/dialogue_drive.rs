//! Inline-dialogue driving: start / step / drive, and the talk-facing legs.

use super::*;

impl World {
    /// Begin running an inline interaction script through the field VM (the
    /// faithful dialogue path - see [`legaia_engine_field::inline_dialogue`]). `inline` is the
    /// actor's interaction-script bytes (e.g. [`DialogRequest::inline`]), which
    /// begin at the first `0x1F` text segment. Replaces any running script.
    pub fn start_inline_dialogue(&mut self, inline: Vec<u8>) {
        self.dialog.inline =
            Some(legaia_engine_field::inline_dialogue::InlineDialogue::from_inline(inline));
    }

    /// Start the inline-script runner on a full interaction record, executing the
    /// prologue from `entry_pc` (the record's `script_pc0`) before the first text
    /// segment at `first_segment`. The prologue's `SysFlag.Test`/`JmpRel` chain
    /// selects which segment the box opens at per story state; if it can't reach a
    /// segment the runner falls back to `first_segment`. See
    /// [`legaia_engine_field::inline_dialogue::InlineDialogue::with_prologue`].
    pub fn start_inline_dialogue_with_prologue(
        &mut self,
        body: Vec<u8>,
        entry_pc: usize,
        first_segment: usize,
    ) {
        self.dialog.inline = Some(
            legaia_engine_field::inline_dialogue::InlineDialogue::with_prologue(
                std::sync::Arc::new(body),
                entry_pc,
                first_segment,
            ),
        );
    }

    /// Advance the running inline interaction script one tick. Between text
    /// boxes the field VM executes the control bytecode (prologue story-flag
    /// tests, `SET`/`CLEAR` flag ops, scene changes) through the World host; at
    /// each `0x1F` segment it opens / ticks a dialog box. `confirm` dismisses the
    /// current box, or commits a menu choice - applying that option's relative
    /// jump (`FUN_80038050`) and handing the branch to the VM so its side
    /// effects run before the reply. `up`/`down` move a menu cursor. No-op when
    /// no inline dialogue is running.
    // PORT: FUN_80039B7C
    // REF: FUN_80038050 (the option-jump apply is delegated to OwnedDialogPanel::confirm_menu)
    // PORT: FUN_8003CF7C (the inline fast-forward loop below is retail's
    //      run-to-next-text helper: tick the field VM until `byte & 0x7F <
    //      0x20`, with the raw-`0x21` execute-then-stop and the stalled-PC
    //      stop mapped to the loop's end paths)
    pub fn step_inline_dialogue(&mut self, confirm: bool, up: bool, down: bool) {
        use legaia_engine_field::inline_dialogue::INLINE_DIALOGUE_STEP_BUDGET;
        let Some(mut id) = self.dialog.inline.take() else {
            return;
        };
        if id.done {
            self.dialog.inline = Some(id);
            return;
        }
        // The cross-context clip cursors advance once per frame. The field
        // frame does it in `tick_prop_interactions`; a host that drives only
        // the conversation (the disc oracles, the browser dialogue path) never
        // reaches that call, so the bank arbitrates and exactly one of the two
        // ticks. Runs before the slice below for the same reason retail's
        // actor tick runs before the dialog SM: the spin must see the latch the
        // clip earned on *this* frame, not last frame's.
        let grid = self.clock.script_frame_delta();
        self.props.bank.tick_actor_clips_for_frame_scaled(grid);
        // The halt window a talk's `CC F8 85` acquire opened: one walk-kernel
        // visit a frame on the player's face-the-speaker turn, closing the
        // window on its terminal frame. The acquire's own frame took its
        // first visit inside the slice below (retail's actor tick runs the
        // dialog SM and then, same visit, the walk kernel).
        self.step_talk_face_ramp(&mut id);
        // A player leg the record armed (`C7 F8 <tx> <tz> <mode>` walk,
        // `B8 F8 <op0> <op1>` budgeted turn): the walk kernel plays it on the
        // player one frame per visit, and the record stays parked on the op
        // until the leg lands. Only a player target parks the caller
        // (`0x801DF034..0x801DF044` for the walk, `0x801DEEFC` for the turn).
        // REF: FUN_801DE840 (0x801DEFC0..0x801DF054), FUN_8003774C (cases 0x38 / 0x47)
        if self.step_talk_player_legs(&mut id) {
            self.dialog.inline = Some(id);
            return;
        }
        // A box held for its press's own frame only (`HeldBox`) is gone by
        // the next pager call: the idle state after the dispatch clears it.
        if id.held_box.as_ref().is_some_and(|h| !h.until_next_line) {
            id.held_box = None;
        }

        // A box is open: tick the typewriter + route input.
        if let Some(panel) = id.panel.as_mut() {
            if panel.menu_active() {
                // The pager's cursor handler cues once when the cursor ends
                // the frame somewhere else (`0x801D9508..0x801D9514`).
                let mut moved = false;
                if up {
                    moved |= panel.move_picker_cursor(-1);
                }
                if down {
                    moved |= panel.move_picker_cursor(1);
                }
                if moved {
                    self.push_sfx_cue(legaia_engine_dialog::dialog::PICKER_MOVE_CUE);
                }
            }
            // Whether the menu was already open BEFORE this frame's typewriter
            // tick. A prompt whose last row finishes typing on a frame the
            // confirm button is down must not commit on that same frame: the
            // player has not seen the options yet, and a held or mashed
            // confirm would silently pick option 0 - which on Tetsu's Rim Elm
            // record ("I want to hear about Biron") re-enters the same speech
            // and reads as an inescapable loop. Retail opens the picker in one
            // dialog-SM state (`0x11` / `0x12`, the box geometry animates first)
            // and reads the choice in the next, so the commit is always at
            // least one frame behind the open.
            let menu_was_open = panel.menu_active();
            panel.tick_at_auto_drawn(
                self.clock.frame_step,
                &mut self.dialog.auto_press,
                &mut self.dialog.cursor_sprites,
            );
            // The pager's automatic press (`_DAT_80073F00`, op `4C 89`) is a
            // confirm the player did not make.
            let confirm = confirm || panel.take_auto_press();
            if confirm {
                if panel.menu_active() && (!menu_was_open || !panel.picker_takes_input()) {
                    // The menu opened this frame, or still slides in: show it,
                    // commit once the pager reads the choice (`dialog_picker_slide`).
                } else if panel.menu_active() {
                    // Commit the choice: apply the option's relative jump and
                    // resume the VM at the branch handler (its flag-sets /
                    // scene-change run before the reply box). A user choice is
                    // progress - clear the wrap map so menu records that
                    // re-emit their menu by jumping back still cycle.
                    self.push_sfx_cue(legaia_engine_dialog::dialog::PICKER_CONFIRM_CUE);
                    let choice = panel.picker_cursor();
                    let target = panel.picker().and_then(|pk| pk.jump_target(choice));
                    id.last_choice = Some(choice);
                    // A user choice is progress: clear the wrap map so a
                    // branch that jumps back over PCs this talk already ran
                    // is not read as a loop. A branch whose reply box is
                    // followed by the jump back (the izumi book-menu shape)
                    // ends the talk parked on that jump instead, and the next
                    // talk re-opens the menu - pinned by
                    // `a_menu_reply_parks_on_its_jump_back_and_the_next_talk_reopens_the_menu`.
                    id.visited.iter_mut().for_each(|v| *v = false);
                    match target {
                        Some(t) => id.pc = t,
                        None => id.done = true,
                    }
                    // The commit's own pager call goes on drawing the prompt's
                    // box (state `0` / `3` / `6` / `9`), and a `0x24`
                    // continuation keeps it up through the branch's opcodes.
                    // REF: FUN_801D84D0 (0x801D9530..0x801D95A4)
                    let continuation = panel.picker().map(|pk| pk.continuation);
                    id.held_box = Some(HeldBox::after_press(panel, continuation));
                    id.panel = None;
                } else if panel.is_done() {
                    // Plain box dismissed: the byte after the box decides
                    // whether the talk goes on (`FUN_80038050`).
                    id.pc = panel.pc;
                    id.held_box = Some(HeldBox::after_press(panel, post_page_byte(panel)));
                    id.panel = None;
                    end_talk_at_post_box_byte(&mut id);
                } else if panel.is_waiting_for_input() {
                    // Page break inside a multi-page conversation (`0x24` /
                    // `0x48` / implicit next lead): turn the page in the same
                    // panel instead of tearing the window down. Mark the new
                    // page's row leads in the wrap map so a tail that jumps
                    // back onto one of them still reads as a wrap.
                    panel.advance_page();
                    if panel.is_done() {
                        id.pc = panel.pc;
                        id.held_box = Some(HeldBox::after_press(panel, post_page_byte(panel)));
                        id.panel = None;
                        end_talk_at_post_box_byte(&mut id);
                    } else {
                        for lead in panel.row_leads() {
                            if lead < id.visited.len() {
                                id.visited[lead] = true;
                            }
                        }
                    }
                } else {
                    // Still typing or scrolling: the pager's skip latch
                    // completes the page (`legaia_engine_dialog::dialog_window`).
                    panel.confirm_while_typing();
                }
            }
            self.dialog.inline = Some(id);
            return;
        }

        // No box open: step the VM until the next text segment or an end.
        // Expose the record's NPC slot so the host's `0x4C 0x51` NPC-run hook
        // can route the prologue's walk ops to the interacted actor.
        self.dialog.stepping_inline_npc = id.npc_slot;
        // A talk runs on the touched actor's own context (retail's dialog SM
        // `FUN_80039B7C` dispatches the actor's record against its own
        // record), so its own-context `2B` / `2C` / `2D` ops read and write
        // the actor's `+0x62` anim-control word - the word its clip cursor
        // ticks under. That is what plays a treasure chest's lid once
        // (`2C 01` unhold, `2B 03` clamp, `2D 08` wait for the end latch)
        // before its item box opens. Bridged in for the slice, written back
        // after it.
        let npc_flag_slot = id
            .npc_slot
            .filter(|_| id.prop_anchor.is_none())
            .filter(|&slot| self.npc_clip_cursor_bound(slot));
        let npc_clip_bound = npc_flag_slot.is_some();
        if let Some(slot) = npc_flag_slot
            && let Some(ch) = self
                .field_vm
                .channels
                .iter()
                .find(|c| !c.object_bind && c.placement_index == usize::from(slot))
        {
            id.ctx.local_flags = ch.ctx.local_flags;
        }
        let mut host = FieldHostImpl { world: self };
        let mut budget = INLINE_DIALOGUE_STEP_BUDGET;
        while budget > 0 {
            budget -= 1;
            // The talker's own turn in flight (see `InlineDialogue::own_turn`):
            // the dispatcher prologue refuses every op of a context carrying
            // `0x400`, so the record holds at its PC - a text lead included -
            // until the walk kernel's terminal frame clears the bit.
            // REF: FUN_801DE840 (0x801DE90C..0x801DE944), FUN_8003774C (0x80038004)
            if let Some(slot) = id.own_turn {
                if host.world.npcs.face_legs.contains_key(&slot)
                    || host.world.npcs.glide_legs.contains_key(&slot)
                {
                    break;
                }
                id.own_turn = None;
                id.ctx.flags &= !0x400;
            }
            let b = id.bytecode.get(id.pc).copied().unwrap_or(0);
            // Retail SM transition test: a byte with `& 0x7F < 0x20` is a text
            // lead (`0x1F`) or a terminator (`0x00..0x1E`), not an opcode.
            if b & 0x7F < 0x20 {
                if b == 0x1F {
                    // Reached a text segment. A prologue (if any) selected it, so
                    // retire the fallback and open the box here.
                    id.fallback_segment_pc = None;
                    // Mark the segment's own PC as executed. Text segments used
                    // to be left unmarked - only opcode bytes were - and that
                    // made the pass-end detector below structurally unable to
                    // fire on the commonest retail shape: a conversation whose
                    // tail jumps **back onto a text segment**. Rim Elm alone has
                    // four such placements, and each one replayed its line
                    // without limit, which is what an unescapable looping NPC
                    // conversation is. The check needs "have I shown this box
                    // already", and a box is identified by its `0x1F` PC.
                    if id.pc < id.visited.len() {
                        id.visited[id.pc] = true;
                    }
                    // Same name-escape resolution as the prop-interaction and
                    // cutscene panels; this is the main NPC-talk path.
                    let mut panel = legaia_engine_dialog::dialog::OwnedDialogPanel::at_segment(
                        std::sync::Arc::clone(&id.bytecode),
                        id.pc,
                    );
                    panel.substitutions = host.world.dialog_substitutions(&id.bytecode);
                    host.world.apply_dialog_window_format(&mut panel);
                    // The panel types the whole packed box (up to 3 rows), so
                    // every row lead counts as shown for the wrap detector,
                    // not just the first.
                    for lead in panel.row_leads() {
                        if lead < id.visited.len() {
                            id.visited[lead] = true;
                        }
                    }
                    id.panel = Some(panel);
                    // The next line loads into the pager (state `4` -> `5`),
                    // which ends the held box.
                    id.held_box = None;
                    break;
                }
                // A non-`0x1F` terminator before any box opened: if a prologue
                // fallback is pending, resume at the first segment (so the box
                // still shows); otherwise the conversation ends.
                if let Some(fb) = id.fallback_segment_pc.take() {
                    id.pc = fb;
                    continue;
                }
                id.done = true;
                break;
            }
            if id.pc < id.visited.len() {
                id.visited[id.pc] = true;
            }
            // The `0x80`-prefix target byte, when this op carries one. Retail
            // resolves it through `FUN_8003C83C` to *another actor's* record -
            // `0xF8` to the live player (`_DAT_8007C364`), anything else by an
            // actor-list walk on `ctx[+0x50]` - and then runs the op against
            // that record's words, not the dispatching one's.
            let ext_target = if b & 0x80 != 0 {
                vm::field::peek_extended(&id.bytecode, id.pc)
            } else {
                None
            };
            // A record spawn (`44`) or a mode-24 door warp (`3E` with
            // `op0 >= 100`) is the pass's effect: the raw `0x21` after it is
            // the real end, never a prologue that failed to reach its box.
            // Each casino seat (`koin1` P1[51..=56]) warps with no box and
            // parks on the `21` at `+0x78`; the system script's loop then
            // re-engages it (`B1 3F..44 08` off flags `0x433..0x435` /
            // `0x4B6..0x4B8`), resuming at `+0x79`: the wash clear
            // (`34 01 FF FF FF 00 00`, `34 01 00 00 00 1E 00`) and the
            // stand-up. Falling back to the first segment there opened the
            // cabinet's refusal line over a field still under the
            // subtractive white wash - a black screen behind a "You don't
            // have any tokens" box after every exit.
            if b == 0x44 || (b == 0x3E && id.bytecode.get(id.pc + 1).is_some_and(|&op0| op0 >= 100))
            {
                id.spawned = true;
            }
            // Inside a halt window the player carries `0x400`, and the
            // dispatcher's halted-target early-out (`0x801DE90C..0x801DE940`)
            // returns an extended op aimed at it at its own PC: the dialog SM
            // sees a PC that did not move and retries next frame.
            if id.face_ramp.is_some()
                && ext_target == Some(legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET)
            {
                break;
            }
            // The talker's own face-at (`4C 85|8E|8F <lo> <hi> <bind>`, no
            // target byte): retail's acquire arm resolves the context itself
            // (`s5 = ctx`), raises the talker's own `0x400` and advances by
            // five (`0x801E2148..0x801E21DC`); the talker's actor tick then
            // runs the walk kernel's FaceTarget leg (`0x8003BD44..0x8003BD50`),
            // whose terminal frame clears the bit (`0x80038004`). The leg is
            // armed here so the talker turns, and the op runs on the stand-in
            // context below, which takes the bit; `InlineDialogue::own_turn`
            // holds the record until the leg lands (the hold at the top of
            // this loop). `rayman` `P1[6]` (Kina) turns this way before her
            // "Follow me!" beat.
            // REF: FUN_801DE840 (0x801E2148..0x801E21DC), FUN_8003774C, FUN_8003BC08
            if let Some(slot) = host.world.dialog.stepping_inline_npc
                && let Some(ramp) =
                    legaia_engine_field::inline_dialogue::TalkFaceRamp::from_own_acquire(
                        &id.bytecode,
                        id.pc,
                    )
            {
                host.world.face_leg_npc(slot, ramp);
                id.own_turn = Some(slot);
            }
            // A halt-acquire of a placement (`CC <id> 85|8E|8F <lo> <hi>
            // <bind>`): the target's walk kernel turns it toward the bind
            // while the talk runs on past the op, as in a cutscene record
            // (`World::face_leg_npc`). Run on the stand-in context it turned
            // nobody: the talk's "Noa looks at Vahn" beats stood still.
            // REF: FUN_801DE840 (0x801E2148..0x801E21DC), FUN_8003774C (the 0x4C arm)
            // A cross-context `B8 <id> ..` turn lands on `<id>` the same way;
            // stepped on the talk's context it turned the talker instead.
            // The player's own legs (`C7 F8 ..` walk-to-tile, `B8 F8 ..`
            // turn): `FUN_8003C83C` resolves `0xF8` to the player, so the
            // walk kernel moves and turns the player, and the record parks
            // on the leg. Stepped on the talker's context instead, the
            // player stayed wherever the touch found them and the casino
            // seats (`koin1` P1[51..=56]: walk to the stool tile, turn to
            // the cabinet, `A2 F8 05` sit) played the sit clip off the
            // stool.
            // REF: FUN_801DE840 (0x801DEE90..0x801DEF24, 0x801DEFC0..0x801DF054)
            if ext_target == Some(legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET)
                && let Some(next_pc) = host.world.arm_talk_player_leg(&mut id)
            {
                if id.pc < id.visited.len() {
                    id.visited[id.pc] = true;
                }
                id.pc = next_pc;
                id.park_frames = 0;
                if id.player_walk.is_some() || id.player_turn.is_some() {
                    // The kernel takes the leg's first step in the arming
                    // frame, after the talk's slice.
                    if host.world.clock.game_tick_live && host.world.clock.script_frame_delta() > 0
                    {
                        host.world.step_talk_player_legs(&mut id);
                    }
                    break;
                }
                continue;
            }
            if let Some(next_pc) = host.world.run_placement_facing_op(&id.bytecode, id.pc) {
                if id.pc < id.visited.len() {
                    id.visited[id.pc] = true;
                }
                id.pc = next_pc;
                id.park_frames = 0;
                continue;
            }
            // A placement the talk set walking carries `0x400` until the
            // kernel lands it, and the dispatcher prologue returns any
            // cross-context op aimed at it at its own PC: the talk retries
            // next frame. `town0c` `P1[21]` (the queen-bee beat) sends
            // `C1 3B 02 46` then `C1 3B 02 41`, and the second leg starts
            // where the first one landed. A turn the talk set going holds
            // its placement the same way: the halt-acquire arm of `B8 <id>`
            // and the `CC <id> 85` face-at raise the same `0x400`.
            // REF: FUN_801DE840 (0x801DE90C..0x801DE944)
            let walking_target = ext_target
                .and_then(|t| host.world.placement_slot_for_target(t))
                .filter(|slot| {
                    let n = &host.world.npcs;
                    n.glide_legs.contains_key(slot)
                        || n.rotate_legs.contains_key(slot)
                        || n.face_legs.contains_key(slot)
                        || host.world.leg_landed_this_frame(*slot)
                });
            if walking_target.is_some() {
                break;
            }
            // A compass walk aimed at a placement (`B7` / `C1 <id> b0 b1`):
            // the arm seats the op on the target's `+0x94`, raises its
            // `0x400` and advances the record past the op (`s7 = 3` plus the
            // target byte) - only a player target parks the caller. Run on
            // the talk's stand-in context it walked nobody.
            // REF: FUN_801DE840 (0x801DEE90..0x801DEF1C), FUN_8003774C (the 0x37 / 0x41 arm)
            if matches!(b, 0xB7 | 0xC1)
                && let Some(slot) = ext_target.and_then(|t| host.world.placement_slot_for_target(t))
                && let Some((body0, body1, rate)) =
                    super::field_channels::compass_walk_operands(&id.bytecode, id.pc)
            {
                host.world.npcs.motions.remove(&slot);
                let mut leg = legaia_engine_field::cutscene_timeline::TimelineNpcGlide {
                    slot,
                    state: legaia_engine_motion_vm::motion_vm::MotionState::default(),
                    body0,
                    body1,
                    rate,
                    frames: 0,
                };
                // The kernel takes the leg's first step in the arming frame,
                // after the talk's slice (the target ticks behind the talk).
                let grid = host.world.clock.script_frame_delta();
                if grid > 0 && host.world.clock.game_tick_live {
                    leg.state.speed = grid;
                    if !host.world.step_npc_glide(&mut leg) {
                        host.world.land_npc_leg(slot);
                    } else {
                        host.world.npcs.glide_legs.insert(slot, leg);
                    }
                } else {
                    host.world.npcs.glide_legs.insert(slot, leg);
                }
                if id.pc < id.visited.len() {
                    id.visited[id.pc] = true;
                }
                id.pc += 4;
                id.park_frames = 0;
                continue;
            }
            // `A2 <target> <clip>` - a cross-context ExecMove. Retail writes
            // the target's `+0x5C` and calls the anim tick, which re-points its
            // `+0x4C` clip pointer and zeroes its cursor; the port binds the
            // target's clip cursor in the bank, which is what the following
            // `AC <target> 08` / `AD <target> 08` end-latch spin then waits on.
            // For the player the windowed / browser hosts additionally draw the
            // gesture off `World::locomotion.player_move_cues` (moves 1/2 are the
            // locomotion clips their own controller already animates).
            if (b & 0x7F) == 0x22
                && let Some(target) = ext_target
                && let Some(&move_id) = id.bytecode.get(id.pc + 2)
            {
                if target == legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET {
                    host.world.bind_player_script_clip(move_id);
                } else {
                    let fallback = host.world.player_clip_frames_hint();
                    host.world
                        .props
                        .bank
                        .bind_actor_clip(target, move_id, fallback);
                }
            }
            // Bind the poked actor's `+0x62` into the executing context for the
            // clip-control ops, and mirror it back after - the same discipline
            // `World::step_prop_interaction` runs a prop's whole record under,
            // narrowed to the one word a cross-context op reaches. Without it a
            // conversation's `AC F8 08` would clear a latch on the NPC record's
            // own flag word and its `AD F8 08` would wait on a bit nothing
            // writes.
            //
            // A field NPC never poked with a clip gets its cursor on first use
            // ([`legaia_engine_scene::field_env::PropAnimBank::actor_clip_or_live`]). Any
            // other target the port has no cursor for falls through to the
            // record's own word - the op has to land somewhere, and the
            // spin's timeout net covers the rest.
            let saved_local_flags = id.ctx.local_flags;
            let bound = ext_target
                .filter(|_| matches!(b & 0x7F, 0x2B..=0x2D))
                .filter(|&target| {
                    let hint = host.world.player_clip_frames_hint();
                    let flags = if target == legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET {
                        Some(host.world.props.bank.player_clip(hint).flags)
                    } else if let Some(live) = host.world.npc_live_clip_for_target(target) {
                        // A field NPC never poked with a clip: its own
                        // looping cursor stands in, as the player's does.
                        Some(
                            host.world
                                .props
                                .bank
                                .actor_clip_or_live(target, live, hint)
                                .flags,
                        )
                    } else {
                        host.world.props.bank.actor_clip(target).map(|a| a.flags)
                    };
                    match flags {
                        Some(f) => {
                            id.ctx.local_flags = f;
                            true
                        }
                        None => false,
                    }
                });
            // A cross-context HALT-ACQUIRE (`4C 85` / `4C 8E` / `4C 8F` behind
            // an `0x80` target byte) suspends the TARGET, and for the player
            // target also the calling record, then advances the caller by its
            // width (the arm `0x801E2148..0x801E21DC`, jump-table `0x801CEF48`
            // entries `5` / `0xE` / `0xF`; `s7 = 0` is the refusal,
            // `beqz s7` at `0x801E21D0`, taken for a target already carrying
            // `0x400` while the scene word `*(_DAT_801C6EA4) + 8` is `0`,
            // `0x801E2168..0x801E218C`). The runner hands the op the record's
            // own context as a stand-in for the target, so the halt bit the
            // acquire sets lands on the stand-in, and the dispatcher's
            // halted-target early-out (`0x801DE90C..0x801DE940`: an extended
            // op whose target carries `+0x10 & 0x400` returns at its own PC
            // unless that scene word is non-zero or the caller's `+0x50` is
            // `0xFB`) would then turn every later cross-context op of the talk into a
            // `Halt` - `retock`'s innkeeper opens with `CC F8 85` and never
            // reached its gold gate. Retail's halt is a window, not a stall:
            // the acquire's bytes are also a walk-kernel FaceTarget leg on the
            // player, and the leg's terminal frame clears both `0x400` bits
            // (`FUN_8003774C`, `0x80038004` / `0x80038028`; captured 18 vsyncs
            // after the acquire). The runner therefore keeps the caller's own
            // halt state across the op and carries the window as
            // `InlineDialogue::face_ramp`. A talk's later re-acquire of the
            // same target is not an end: the capture shows the next talk's
            // acquire succeeding.
            let caller_halt =
                ext_target.map(|_| (id.ctx.flags & 0x400, id.ctx.saved_pc, id.ctx.wait_accum));
            // A player seat (`A3 F8 x z` MOVE_TO, `CC F8 51 x z ..` run) or a
            // player box test (`CD F8 ..` BBOX_TEST): `FUN_8003C83C` resolves
            // `0xF8` to the player object, so the op runs with the PLAYER as
            // its context - the seat takes the player arm (`0x801DEC7C`
            // compares the context pointer against `_DAT_8007C364`) and the
            // box test reads the player's `+0x14` / `+0x18`. Stepped on the
            // talker's own context, the seat moved nobody and the box tested
            // where the talker stands. `town0b` P1[37], the night-before
            // talk, closes with `A3 F8 20 63` - it puts Vahn on `(32, 99)`,
            // the walled-in tile of the P2[8] walk-on that stages the village
            // gathering, then ends the interaction (`21`), and the crossing
            // fires on the first unlocked frame. The elder's P1[55] then
            // opens every talk with `CD F8 1E 5B 21 5C`: only a player
            // standing in `[30..33, 91..92]` reaches the arm that sets
            // `0x141` and leaves for `map01`. The cutscene timeline and the
            // prop runner already take the seat arm.
            // REF: FUN_8003C83C, FUN_801DE840 (0x23 / 4C 51 / 4D arms)
            let op = b & 0x7F;
            let player_seat = ext_target
                == Some(legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET)
                && (op == 0x23
                    || op == 0x4D
                    || (op == 0x4C && id.bytecode.get(id.pc + 2) == Some(&0x51)));
            // A run aimed at ANOTHER actor (`CC <id> 51 x z ..`) walks that
            // actor, not the talker: `town0b` P1[55]'s first talk sends
            // placement `0x2A` home with `CC 2A 51 27 28`, and routing it to
            // the talker walked the village elder out of the room the next
            // beat needs him in. An id no placement channel carries moves
            // nobody.
            // REF: FUN_8003C83C
            let run_target =
                (op == 0x4C && id.bytecode.get(id.pc + 2) == Some(&0x51) && !player_seat)
                    .then_some(ext_target)
                    .flatten()
                    .filter(|&t| t != 0xFB);
            // A seat aimed at ANOTHER actor (`A3 <id> x z`) from a talker
            // whose own context carries the party-bank class bit (`31 18` in
            // its prologue): the port reads the player arm off that bit when
            // no channel is stepping, so run on the talker's context the
            // seat moved the PLAYER. Retail takes the player arm on an
            // identity compare of the resolved context against the player
            // (`bne s5,v0` at `0x801DEC7C`), which a companion's record
            // fails. `vozz` P1[1] parks its two helper actors with
            // `A3 06 7F 7F` / `A3 07 7F 7F`, and the player went to the
            // off-map hide tile with them.
            // REF: FUN_8003C83C, FUN_801DE840 (0x23 arm, 0x801DEC7C)
            let other_seat = (op == 0x23
                && !player_seat
                && id.ctx.flags & legaia_engine_field::field_player_clip::PARTY_BANK_FLAG != 0)
                .then_some(ext_target)
                .flatten();
            let talker = host.world.dialog.stepping_inline_npc;
            if let Some(t) = run_target {
                let view = host.world.channel_view();
                host.world.dialog.stepping_inline_npc =
                    legaia_engine_field::field_channels::resolve_target(view, t)
                        .map(|ci| &view[ci])
                        .filter(|ch| !ch.object_bind)
                        .and_then(|ch| u8::try_from(ch.placement_index).ok());
            }
            let step = if player_seat {
                let (px, pz) = host
                    .world
                    .player_actor_slot
                    .and_then(|slot| host.world.actors.get(usize::from(slot)))
                    .map_or((0, 0), |a| (a.move_state.world_x, a.move_state.world_z));
                let mut player_ctx = legaia_engine_vm::field::FieldCtx {
                    script_id: u16::from(legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET),
                    flags: 0x0100_0000,
                    world_x: px as u16,
                    world_z: pz as u16,
                    ..Default::default()
                };
                let r = vm::field::step(&mut host, &mut player_ctx, &id.bytecode, id.pc);
                if let Some(slot) = host.world.player_actor_slot
                    && let Some((x, z)) = host
                        .world
                        .actors
                        .get(usize::from(slot))
                        .map(|a| (a.move_state.world_x, a.move_state.world_z))
                {
                    let y = host
                        .world
                        .sample_field_floor_height(i32::from(x), i32::from(z))
                        as i16;
                    if let Some(a) = host.world.actors.get_mut(usize::from(slot)) {
                        a.move_state.world_y = y;
                    }
                }
                r
            } else if let Some(t) = other_seat {
                let mut other = id.ctx.clone();
                other.flags &= !legaia_engine_field::field_player_clip::PARTY_BANK_FLAG;
                let r = vm::field::step(&mut host, &mut other, &id.bytecode, id.pc);
                // The seat lands on the placement the id names, when it
                // names one; an id no channel carries moves nobody.
                let view = host.world.channel_view();
                let seated = legaia_engine_field::field_channels::resolve_target(view, t)
                    .map(|ci| &view[ci])
                    .filter(|ch| !ch.object_bind)
                    .and_then(|ch| u8::try_from(ch.placement_index).ok());
                if let Some(slot) = seated
                    && let Some(pos) = host.world.npcs.positions.get_mut(&slot)
                {
                    *pos = (other.world_x as i16, other.world_z as i16);
                    host.world.npcs.motions.remove(&slot);
                }
                r
            } else {
                field_step_routed(&mut host, &mut id.ctx, &id.bytecode, id.pc)
            };
            host.world.dialog.stepping_inline_npc = talker;
            if let Some((halt, saved_pc, wait_accum)) = caller_halt
                && halt == 0
                && id.ctx.flags & 0x400 != 0
            {
                id.ctx.flags &= !0x400;
                id.ctx.saved_pc = saved_pc;
                id.ctx.wait_accum = wait_accum;
                // The halt does not vanish, it moves: a `CC F8 85|8E|8F`
                // acquire hands the player the walk kernel's FaceTarget leg,
                // and that leg's terminal frame is what clears both bits
                // (`0x80038004` / `0x80038028`). The runner models the window
                // on `face_ramp` rather than on the stand-in context.
                if let Some(ramp) = legaia_engine_field::inline_dialogue::TalkFaceRamp::from_acquire(
                    &id.bytecode,
                    id.pc,
                ) {
                    id.face_ramp = Some(ramp);
                    host.world.step_talk_face_ramp(&mut id);
                }
            }
            if let Some(target) = bound
                && let Some(actor) = host.world.props.bank.actor_clip_mut(target)
            {
                actor.flags = id.ctx.local_flags;
                id.ctx.local_flags = saved_local_flags;
            }
            match step {
                // A backward Advance onto an already-executed PC with no box
                // parked in between is the record looping over its own ops.
                // Retail ends a talk earlier, at the dismissed box's parking
                // byte (`end_talk_at_post_box_byte`); this is the port's net
                // for a loop that shows no box. End the conversation like a
                // Halt would; the wrap map is cleared on picker commits.
                FieldStepResult::Advance { next_pc }
                    if next_pc <= id.pc && id.visited.get(next_pc).copied().unwrap_or(false) =>
                {
                    if let Some(fb) = id.fallback_segment_pc.take() {
                        id.pc = fb;
                        continue;
                    }
                    id.done = true;
                    break;
                }
                FieldStepResult::Advance { next_pc } => {
                    id.pc = next_pc;
                    // The record moved, so whatever spin it was parked on has
                    // fallen through: the park counter starts again from the
                    // next one.
                    id.park_frames = 0;
                    // Retail's run-to-next-text helper breaks after executing
                    // a raw `0x21` byte and returns it (`FUN_8003CF7C`
                    // `if (bVar1 == 0x21) break`); the dialog SM reads that
                    // as conversation end. Raw compare only - an extended
                    // `0xA1` NOP runs through like any other op.
                    if b == 0x21 {
                        if !id.spawned
                            && let Some(fb) = id.fallback_segment_pc.take()
                        {
                            id.pc = fb;
                            continue;
                        }
                        id.done = true;
                        break;
                    }
                }
                FieldStepResult::Yield { resume_pc } => {
                    let op_pc = id.pc;
                    id.pc = resume_pc;
                    // The talker's own glide-step (`37` / `41` / `47` with no
                    // target byte) parks the record (`+0x10 |= 0x400`) and
                    // hands the step to the walk kernel, whose terminal frame
                    // clears the bit again (`FUN_8003774C`, the 0x37 / 0x41
                    // arm). A compass walk is played on the talker as a
                    // placement leg, and the record holds on `own_turn` until
                    // it lands (the hold at the top of this loop). Left
                    // unwalked, the bit never cleared and every later
                    // cross-context op of the talk took the dispatcher's
                    // halted-target early-out and ended the conversation:
                    // Xain's second stage (`tunnelc` P1[4], `41 07 C1` then
                    // `AC 0C 08`) ended there, before the picker that raises
                    // `0x325` and the fight behind it. A step with no talker
                    // slot to walk (or a `47` walk-to-tile) is taken as done.
                    // REF: FUN_801DE840 (0x801DE90C..0x801DE944), FUN_8003774C (the 0x37 / 0x41 arm)
                    if matches!(b, 0x37 | 0x41 | 0x47) {
                        match (
                            host.world.dialog.stepping_inline_npc,
                            super::field_channels::compass_walk_operands(&id.bytecode, op_pc),
                        ) {
                            (Some(slot), Some((body0, body1, rate))) => {
                                host.world.npcs.glide_legs.insert(
                                    slot,
                                    legaia_engine_field::cutscene_timeline::TimelineNpcGlide {
                                        slot,
                                        state: legaia_engine_motion_vm::motion_vm::MotionState::default(),
                                        body0,
                                        body1,
                                        rate,
                                        frames: 0,
                                    },
                                );
                                id.own_turn = Some(slot);
                            }
                            _ => id.ctx.flags &= !0x400,
                        }
                    }
                }
                // op-0x4A WAIT_FRAMES halts at its own PC every tick until its
                // frame target elapses (`ctx.wait_accum` accumulates one
                // `frame_delta` per `step`, and `id.ctx` persists across ticks).
                // Persist the PC and resume next tick instead of ending, so
                // option effects scripted *behind* a wait still run - the Rim
                // Elm spar's `3E FF 04` battle install sits behind a
                // `WaitFrames 16`. A wait before the talk's first box parks
                // the same way: retail's runner has no notion of a prologue,
                // and falling back to the record's first segment there
                // replayed the wrong speech - `town0b` P1[55]'s gathering
                // arm (`+0xE14`) waits eight frames before its first box,
                // and the fallback re-ran the pre-gathering speech in its
                // place, so the arm that sets `0x141` never ran.
                FieldStepResult::Halt { final_pc } if (b & 0x7F) == 0x4A => {
                    id.pc = final_pc;
                    break;
                }
                // `4C CD` holds while the camera mover's glide is in flight:
                // the same timed park.
                FieldStepResult::Halt { final_pc }
                    if (b & 0x7F) == 0x4C && id.bytecode.get(id.pc + 1) == Some(&0xCD) =>
                {
                    id.pc = final_pc;
                    break;
                }
                // `4C C5` / `4C C6` hold while a system flag is clear / set,
                // and what flips it is a move-VM part the talk spawned
                // (`2F 1C` / `2F 1D` in a stager record, ticked by the frame
                // tail): park and re-test next frame. Ending the talk here
                // re-ran it from its first op - `town0b` P1[55]'s gathering
                // parks on `4C C6 56 05` until its fade part clears `0x556`.
                // REF: FUN_801DE840 (`0x801E2894` / `0x801E28B0`)
                FieldStepResult::Halt { final_pc }
                    if (b & 0x7F) == 0x4C
                        && matches!(id.bytecode.get(id.pc + 1), Some(&(0xC5 | 0xC6))) =>
                {
                    id.pc = final_pc;
                    break;
                }
                // A cross-context CFLAG_TST (`B3 <id> <bit>`) waits on
                // another actor's context word - `town01` P1[32] closes its
                // spar setup with `C1 44 02` (park actor `0x44`) and then
                // `B3 44 0A`, the halt-bit verify. The runner has no
                // resolved context word to read it from, and the engine's
                // cross-context pokes complete synchronously, so it steps
                // past by width as the cutscene timeline does; ending the
                // talk there left the player on the seat the talk had just
                // made, a walled-in tile, with the rest of the
                // choreography unrun.
                // REF: FUN_8003C83C
                FieldStepResult::Halt { final_pc }
                    if (b & 0x7F) == 0x33 && ext_target.is_some() && final_pc == id.pc =>
                {
                    id.pc = final_pc + 3;
                    id.park_frames = 0;
                }
                // op-0x2D LFLAG_TST is a **spin**, not an end. The bit it
                // tests is in the clip-control word `actor+0x62`, and bit 8
                // (`0x0100`) is the "end" flag the actor's anim tick
                // (`FUN_800204F8`) latches when the clip cursor reaches an
                // end - so the retail idiom `A2 F8 <clip>` / `AC F8 08` /
                // `AD F8 08` is "play it, clear the latch, wait for it". The
                // dialog SM returns and re-enters per frame until the latch
                // lands; ending the conversation here instead is what left
                // every scripted gesture beat unreachable, the innkeeper's
                // among them (`docs/subsystems/inn.md`).
                //
                // So: park, and re-test next frame. The runner writes nothing -
                // the bit it is waiting on belongs to the poked actor's clip
                // cursor, and `PropAnim::tick` is what sets it
                // (`World::step_inline_dialogue` runs that tick at the top of
                // the frame, exactly as retail's actor tick precedes the dialog
                // SM). `INLINE_SPIN_PARK_TIMEOUT` stays as a net for a spin
                // whose target the port cannot resolve to a cursor at all; a
                // resolved one drains in the clip's own frame count.
                //
                // REF: FUN_800204F8 (the anim tick that owns the latch)
                // REF: FUN_80039B7C (the dialog SM's per-frame re-entry)
                // A prologue run (a box not yet opened) parks on the same
                // spin when it waits on the end latch of the actor's own
                // bound clip: the chest's lid plays out before its box, it
                // does not fall through to the first segment. So does one
                // whose cross-context target resolved to a ticking cursor:
                // the Genesis Tree talk (`vozz` P1[7]) plays the player's
                // reach (`A2 F8 01` / `AD F8 08`) before its first box, and
                // falling through skipped the `0x00B` raise behind it.
                FieldStepResult::Halt { final_pc }
                    if (b & 0x7F) == 0x2D
                        && (id.fallback_segment_pc.is_none()
                            || bound.is_some()
                            || (npc_clip_bound
                                && ext_target.is_none()
                                && id.bytecode.get(final_pc + 1) == Some(&8))) =>
                {
                    id.park_frames = id.park_frames.saturating_add(1);
                    id.pc = final_pc;
                    if id.park_frames
                        > legaia_engine_field::inline_dialogue::INLINE_SPIN_PARK_TIMEOUT
                    {
                        id.done = true;
                    }
                    break;
                }
                // The same wait in a lead whose talker the port holds no clip
                // cursor for (a placement on the party clip bank, `31 18`):
                // nothing ticks toward the latch, but in retail it always
                // lands - the anim tick raises it when the bound clip runs
                // out - and the record goes on from the op behind the wait.
                // The lead's fallback to its first text segment skipped
                // everything between the two: `vozz` P1[1] gives item `0xAA`
                // and raises `0x079` there, after `2C 08` / `2D 08`. The
                // wait is taken as a clip of no length.
                // REF: FUN_800204F8
                FieldStepResult::Halt { final_pc }
                    if (b & 0x7F) == 0x2D
                        && ext_target.is_none()
                        && id.fallback_segment_pc.is_some()
                        && id.bytecode.get(final_pc + 1) == Some(&8) =>
                {
                    if final_pc < id.visited.len() {
                        id.visited[final_pc] = true;
                    }
                    id.pc = final_pc + 2;
                    id.park_frames = 0;
                }
                // A merchant's op `0x49` sub-0 under its open shop: the
                // record parks on the op, as every context that reaches it
                // does - retail re-runs the instruction each frame while the
                // park word `_DAT_8007B450` is up and takes the Done edge
                // (`sw zero,-0x4bb0(s0)`, then the advance) once the menu
                // overlay has closed. Ending the talk here instead left the
                // shop's arm standing with nothing to resume it, so the next
                // talk to reach a shop op took that stale Done edge and
                // walked past its own shop: every second visit to a counter
                // opened nothing.
                //
                // The op's Armed arm does not look at the sub-op: any park
                // word that is neither `0` nor `1` returns the unchanged PC
                // (`bne v0,zero` at `0x801E097C`). The casino prize counter
                // (sub-7, `koin1` / `balden`) is the same host-closed screen
                // as the gold shop and parks the same way.
                // REF: FUN_801DE840 (op `0x49`'s Armed / Done arms)
                FieldStepResult::Halt { final_pc }
                    if (b & 0x7F) == 0x49
                        && (host.world.shops.shop_armed
                            || host.world.shops.prize_exchange_armed) =>
                {
                    id.pc = final_pc;
                    break;
                }
                // Any other halt/hold, an unhandled op, or an end: stop.
                // (Unlike the cutscene timeline the runner does not force-
                // advance past these - an inline script that can't proceed
                // ends.) While a prologue is still running (no box opened yet),
                // a halt means the prologue can't proceed - fall back to the
                // first segment so the dialogue is never worse than truncation.
                FieldStepResult::Halt { .. }
                | FieldStepResult::Pending { .. }
                | FieldStepResult::Unknown { .. } => {
                    if let Some(fb) = id.fallback_segment_pc.take() {
                        id.pc = fb;
                        continue;
                    }
                    id.done = true;
                    break;
                }
            }
        }
        self.dialog.stepping_inline_npc = None;
        if let Some(slot) = npc_flag_slot
            && let Some(ch) = self
                .field_vm
                .channels
                .iter_mut()
                .find(|c| !c.object_bind && c.placement_index == usize::from(slot))
        {
            ch.ctx.local_flags = id.ctx.local_flags;
        }
        if id.done {
            self.restore_owed_player_scale(&id.bytecode, id.pc, &id.visited);
        }
        self.dialog.inline = Some(id);
    }

    /// One walk-kernel visit on a talk's halt window
    /// ([`legaia_engine_field::inline_dialogue::TalkFaceRamp`]): turn the player toward the
    /// actor the acquire names and, on the leg's terminal frame, close the
    /// window. No-op without a window.
    ///
    /// The face-at operand is an actor bind (`+0x50`), and the walk kernel
    /// resolves it the way every cross-context id resolves: the actor-list
    /// node whose `+0x50` equals it (`lhu v0,0x50(v1)` at `0x80037E88`), so the
    /// port resolves it through the scene's channel set, not through the
    /// conversation. The two differ often: of the disc's clean-decoded
    /// `CC F8 85|8E|8F` acquires, 40 of the 146 in placement records name an
    /// actor other than the record's own (often the neighbouring placement,
    /// sometimes a second actor the same talk turns to), and
    /// the 24 in object records and 861 in cutscene records have no own
    /// actor at all (`crates/engine-scene/tests/talk_face_acquire_bind_disc.rs`).
    /// A bind the channel set cannot resolve falls back to the conversation's
    /// own placement; a window with no player or nothing to face closes at
    /// once rather than holding the talk's player-targeted ops.
    ///
    /// REF: FUN_8003774C (the kernel visit), FUN_8003BC08 (visits it on `0x400`)
    /// One walk-kernel visit of a cutscene record's player face-at leg
    /// (`CutsceneTimeline::player_face`): turn the player toward the actor
    /// bind the acquire names. Returns `true` on the terminal frame, or when
    /// there is no player or nothing the bind resolves to (the leg then
    /// closes at once rather than holding the record).
    ///
    /// REF: FUN_8003774C (the 0x4C arm)
    pub fn step_player_face_leg(
        &mut self,
        ramp: &mut legaia_engine_field::inline_dialogue::TalkFaceRamp,
    ) -> bool {
        let speed = self.clock.display_frame_step.max(1);
        self.step_player_face_leg_by(ramp, speed)
    }

    /// [`Self::step_player_face_leg`] spending `speed` vsyncs of the leg: a
    /// cutscene record's legs advance one game frame at a time, by the
    /// frame step (`DAT_1F800393`).
    pub fn step_player_face_leg_by(
        &mut self,
        ramp: &mut legaia_engine_field::inline_dialogue::TalkFaceRamp,
        speed: u16,
    ) -> bool {
        let target = self.talk_face_target(ramp.program[4]);
        let player = self
            .player_actor_slot
            .and_then(|slot| self.actors.get(usize::from(slot)))
            .map(|a| {
                (
                    a.move_state.world_x,
                    a.move_state.world_z,
                    a.move_state.render_26,
                )
            });
        let (Some((tx, tz)), Some((px, pz, yaw))) = (target, player) else {
            return true;
        };
        let (yaw, done) = ramp.step(px, pz, yaw as u16, tx, tz, speed);
        if let Some(slot) = self.player_actor_slot
            && let Some(actor) = self.actors.get_mut(usize::from(slot))
        {
            actor.move_state.render_26 = yaw as i16;
        }
        done
    }

    /// One walk-kernel visit of an NPC face-at leg
    /// ([`legaia_engine_field::cutscene_timeline::CutsceneTimeline::npc_faces`]): turn
    /// placement `slot` toward the actor the bind names - the player for
    /// `0xF8`. Returns `true` on the terminal frame, or when the NPC has no
    /// position or the bind resolves to nothing (the leg then ends at once).
    ///
    /// REF: FUN_8003774C (the 0x4C arm: `0xF8` resolves to `_DAT_8007C364`)
    pub fn step_npc_face_leg(
        &mut self,
        slot: u8,
        ramp: &mut legaia_engine_field::inline_dialogue::TalkFaceRamp,
    ) -> bool {
        let speed = self.clock.display_frame_step.max(1);
        self.step_npc_face_leg_by(slot, ramp, speed)
    }

    /// [`Self::step_npc_face_leg`] spending `speed` vsyncs of the leg (see
    /// [`Self::step_player_face_leg_by`]).
    pub fn step_npc_face_leg_by(
        &mut self,
        slot: u8,
        ramp: &mut legaia_engine_field::inline_dialogue::TalkFaceRamp,
        speed: u16,
    ) -> bool {
        let bind = ramp.program[4];
        let target = if bind == 0xF8 {
            self.player_actor_slot
                .and_then(|s| self.actors.get(usize::from(s)))
                .map(|a| (a.move_state.world_x, a.move_state.world_z))
        } else {
            self.talk_face_target(bind)
        };
        let (Some((tx, tz)), Some(&(x, z))) = (target, self.npcs.positions.get(&slot)) else {
            return true;
        };
        // A never-posed NPC stands at the retail spawn default `0` (engine
        // `0x800`).
        let yaw = self.npcs.headings.get(&slot).copied().unwrap_or(0x800);
        let (yaw, done) = ramp.step(x, z, yaw as u16, tx, tz, speed);
        self.set_timeline_facing(Some(slot), yaw as i16);
        done
    }

    /// The placement slot a cross-context target byte names: the channel
    /// whose script id is `t` (`FUN_8003C83C`), when it is a placement - not
    /// the player (`0xF8`), the system channel (`0xFB`) or an object bind.
    pub(super) fn placement_slot_for_target(&self, t: u8) -> Option<u8> {
        if t == 0xF8 || t == 0xFB {
            return None;
        }
        let view = self.channel_view();
        legaia_engine_field::field_channels::resolve_target(view, t)
            .map(|ci| &view[ci])
            .filter(|ch| !ch.object_bind)
            .and_then(|ch| u8::try_from(ch.placement_index).ok())
    }

    /// How far the compass-walk leg (`B7` / `C1 <t> b0 b1`) a script armed on
    /// the placement target byte `t` names has run: the spent units the walk
    /// kernel keeps on the actor's `+0x54` (one per vsync, the frame step a
    /// tick). `None` while no such leg is live - before the arm, or after
    /// the landing clears the halt. Searches the talk runner's legs, the
    /// modal timeline's and every concurrent helper's.
    ///
    /// REF: FUN_8003774C (the 0x37 / 0x41 arm)
    pub fn npc_compass_walk_accum(&self, t: u8) -> Option<u16> {
        let slot = self.placement_slot_for_target(t)?;
        let in_timeline = |tl: &legaia_engine_field::cutscene_timeline::CutsceneTimeline| {
            tl.npc_glides
                .iter()
                .find(|g| g.slot == slot)
                .map(|g| g.state.op_accum)
        };
        self.npcs
            .glide_legs
            .get(&slot)
            .map(|g| g.state.op_accum)
            .or_else(|| self.cutscene.timeline.as_ref().and_then(in_timeline))
            .or_else(|| self.field_vm.helper_contexts.iter().find_map(in_timeline))
    }

    /// A facing op aimed at another placement from a context that is not a
    /// cutscene timeline (the scene's system script, a talk record):
    /// `B8 <id> <op0> <op1>` - the compass write, or with a budget the
    /// `0x38` RotateToAngle leg - and `CC <id> 85|8E|8F <lo> <hi> <bind>`,
    /// the FaceTarget leg. `FUN_8003C83C` resolves `<id>` to that actor, so
    /// the op turns it, not the context running the op, and for a
    /// placement target both arms advance the caller (`0x801DEEFC`,
    /// `0x801E21B8`). Returns the PC past the op, or `None` when the op is
    /// not one of these or the target is not a placement. `rikuroa`'s entry
    /// script stands two of its chests with `B8 21 84 00` / `B8 23 84 00`.
    ///
    /// REF: FUN_801DE840 (case 0x38, 0x801E2148..0x801E21DC), FUN_8003C83C
    pub fn run_placement_facing_op(&mut self, bc: &[u8], pc: usize) -> Option<usize> {
        let op = *bc.get(pc)?;
        if op & 0x80 == 0 {
            return None;
        }
        let slot = self.placement_slot_for_target(*bc.get(pc + 1)?)?;
        match op & 0x7F {
            0x38 => {
                let (op0, op1) = (*bc.get(pc + 2)?, *bc.get(pc + 3)?);
                self.npcs.face_legs.remove(&slot);
                self.npcs.rotate_legs.remove(&slot);
                if op1 & 0x7F == 0 {
                    if let Some(h) =
                        legaia_engine_field::man_field_scripts::facing_index_to_engine_heading(
                            op0 & 0xF,
                        )
                    {
                        self.npcs.headings.insert(slot, h);
                    }
                } else {
                    let cur = self.npcs.headings.get(&slot).copied().unwrap_or(0x800);
                    let grid = self.clock.script_frame_delta();
                    let mut leg = legaia_engine_field::cutscene_timeline::TimelineFacing {
                        slot: Some(slot),
                        state: legaia_engine_motion_vm::motion_vm::MotionState {
                            yaw: (cur as u16) & 0x0FFF,
                            speed: grid,
                            ..Default::default()
                        },
                        program: [0x38, op0, op1],
                        resume_pc: pc + 4,
                        frames: 0,
                    };
                    if grid == 0 || !self.step_npc_rotate_leg(&mut leg) {
                        self.npcs.rotate_legs.insert(slot, leg);
                    } else {
                        self.land_npc_leg(slot);
                    }
                }
                Some(pc + 4)
            }
            0x4C => {
                let (_, ramp) =
                    legaia_engine_field::inline_dialogue::TalkFaceRamp::from_npc_acquire(bc, pc)?;
                self.face_leg_npc(slot, ramp);
                Some(pc + 6)
            }
            _ => None,
        }
    }

    /// Arm the player leg a talk record's cross-context op on the player
    /// names, when it names one: `C7 F8 <tx> <tz> <mode>` (op `0x47`
    /// walk-to-tile) or `B8 F8 <op0> <op1>` (op `0x38`; a zero budget is the
    /// instant compass write, any other the RotateToAngle ramp). Returns the
    /// PC past the op, with [`InlineDialogue::player_walk`] /
    /// [`InlineDialogue::player_turn`] set when a leg is now in flight, or
    /// `None` when the op is neither. The leg's first frame runs on the
    /// next visit ([`Self::step_talk_player_legs`]), as the walk kernel
    /// runs after the dispatcher in retail's actor tick.
    ///
    /// The tile decode is the `0x47` arm's (`0x80037B84..0x80037C04`): tile
    /// `* 0x80 + 0x40`, a set high bit moving the target to the tile's far
    /// edge (`+ 0x80`). The parked sentinel tile `(0x7F, 0x7F)` arms nothing.
    ///
    /// [`InlineDialogue::player_walk`]: legaia_engine_field::inline_dialogue::InlineDialogue::player_walk
    /// [`InlineDialogue::player_turn`]: legaia_engine_field::inline_dialogue::InlineDialogue::player_turn
    // PORT: FUN_8003774C (case 0x47 target decode)
    // REF: FUN_801DE840 (0x801DEE90..0x801DEF24, 0x801DEFC0..0x801DF054)
    pub(super) fn arm_talk_player_leg(
        &mut self,
        id: &mut legaia_engine_field::inline_dialogue::InlineDialogue,
    ) -> Option<usize> {
        let bc = &id.bytecode;
        let pc = id.pc;
        match *bc.get(pc)? {
            0xC7 => {
                let (b0, b1, b2) = (*bc.get(pc + 2)?, *bc.get(pc + 3)?, *bc.get(pc + 4)?);
                if (b0 & 0x7F, b1 & 0x7F)
                    != legaia_engine_field::man_field_scripts::PARKED_SENTINEL_TILE
                {
                    let decode = |b: u8| -> i16 {
                        i16::from(b & 0x7F) * 0x80 + if b & 0x80 != 0 { 0x80 } else { 0x40 }
                    };
                    id.player_walk = Some(legaia_engine_field::inline_dialogue::PropPlayerWalk {
                        target: (decode(b0), decode(b1)),
                        speed: crate::world::field_npc_walk_step_speed(0x80, b2 & 7),
                        frames: 0,
                    });
                }
                Some(pc + 5)
            }
            0xB8 => {
                let (op0, op1) = (*bc.get(pc + 2)?, *bc.get(pc + 3)?);
                if op1 & 0x7F == 0 {
                    if let Some(h) =
                        legaia_engine_field::man_field_scripts::facing_index_to_engine_heading(
                            op0 & 0xF,
                        )
                    {
                        self.set_timeline_facing(None, h);
                    }
                } else {
                    let cur = self.timeline_facing(None).unwrap_or(0x800);
                    id.player_turn = Some(legaia_engine_field::cutscene_timeline::TimelineFacing {
                        slot: None,
                        state: legaia_engine_motion_vm::motion_vm::MotionState {
                            yaw: (cur as u16) & 0x0FFF,
                            speed: 1,
                            ..Default::default()
                        },
                        program: [0x38, op0, op1],
                        resume_pc: pc + 4,
                        frames: 0,
                    });
                }
                Some(pc + 4)
            }
            _ => None,
        }
    }

    /// One walk-kernel visit of the player legs a talk record armed
    /// ([`Self::arm_talk_player_leg`]): the walk first, then the turn the
    /// record reaches after it. `true` while a leg is still in flight - the
    /// record stays parked this frame.
    // REF: FUN_8003774C (cases 0x38 / 0x47)
    pub(super) fn step_talk_player_legs(
        &mut self,
        id: &mut legaia_engine_field::inline_dialogue::InlineDialogue,
    ) -> bool {
        // The talk's legs advance on the game-frame grid, as a record's do:
        // the walk kernel spends the frame step once a game frame and
        // nothing between (`FUN_8003774C`, `lbu s2,0x393` at `0x80037864`).
        let grid = self.clock.script_frame_delta();
        // A leg that landed holds the talk through the rest of its game
        // frame: the kernel ran after the talk's slice on that frame.
        if let Some(t) = self.npcs.talk_player_landed {
            if t == self.frame || grid == 0 {
                return true;
            }
            self.npcs.talk_player_landed = None;
        }
        if grid == 0 && (id.player_walk.is_some() || id.player_turn.is_some()) {
            return true;
        }
        let had_leg = id.player_walk.is_some() || id.player_turn.is_some();
        if let Some(mut walk) = id.player_walk.take() {
            walk.frames += 1;
            let arrived = self.step_player_walk_leg(walk.target, walk.speed.saturating_mul(grid));
            if !arrived && walk.frames < WALK_PARK_TIMEOUT {
                id.player_walk = Some(walk);
                return true;
            }
        }
        if let Some(mut leg) = id.player_turn.take()
            && {
                leg.state.speed = grid;
                !self.step_npc_rotate_leg(&mut leg)
            }
        {
            id.player_turn = Some(leg);
            return true;
        }
        if had_leg && self.clock.game_tick_live {
            self.npcs.talk_player_landed = Some(self.frame);
            return true;
        }
        false
    }

    /// Note that a free-standing leg on placement `slot` landed this tick
    /// ([`crate::world::FieldNpcState::landed_ticks`]); a no-op while the
    /// game-tick clock is down, where every tick is a frame.
    pub(crate) fn land_npc_leg(&mut self, slot: u8) {
        if self.clock.game_tick_live {
            self.npcs.landed_ticks.insert(slot, self.frame);
        }
    }

    /// Whether a leg on placement `slot` landed on the current game frame:
    /// this tick, or an earlier one with no game frame since. Retail's
    /// kernel lands it after the slice of the context that armed it, so the
    /// next op aimed at the placement runs on the following game frame.
    pub(crate) fn leg_landed_this_frame(&self, slot: u8) -> bool {
        self.npcs
            .landed_ticks
            .get(&slot)
            .is_some_and(|&t| t == self.frame || self.clock.script_frame_delta() == 0)
    }

    /// One frame of a free-standing `0x38` rotate leg; `true` once it snaps.
    pub(super) fn step_npc_rotate_leg(
        &mut self,
        leg: &mut legaia_engine_field::cutscene_timeline::TimelineFacing,
    ) -> bool {
        leg.frames += 1;
        let r = legaia_engine_motion_vm::motion_vm::step(
            &mut leg.state,
            legaia_engine_motion_vm::motion_vm::MotionTarget::default(),
            &leg.program,
        );
        if leg.state.yaw_written {
            self.set_timeline_facing(leg.slot, leg.state.yaw as i16);
        }
        r == legaia_engine_motion_vm::motion_vm::StepResult::Done || leg.frames >= WALK_PARK_TIMEOUT
    }

    /// Arm a free-standing NPC face-at leg (a talk record's
    /// `CC <id> 85|8E|8F ..`): its first frame now, the rest on the field
    /// tick ([`Self::tick_field_npc_face_legs`]). A new leg replaces one the
    /// actor had in flight - retail overwrites `+0x94`.
    pub fn face_leg_npc(
        &mut self,
        slot: u8,
        mut ramp: legaia_engine_field::inline_dialogue::TalkFaceRamp,
    ) {
        self.npcs.face_legs.remove(&slot);
        self.npcs.rotate_legs.remove(&slot);
        let grid = self.clock.script_frame_delta();
        if grid == 0 || !self.step_npc_face_leg_by(slot, &mut ramp, grid) {
            self.npcs.face_legs.insert(slot, ramp);
        } else {
            self.land_npc_leg(slot);
        }
    }

    /// One walk-kernel frame of every free-standing NPC face-at, rotate and
    /// compass-walk leg.
    /// REF: FUN_8003774C (the 0x4C arm), FUN_8003BC08
    pub(crate) fn tick_field_npc_face_legs(&mut self) {
        // On the game-frame grid: a vsync between two game frames moves no
        // leg, and a game frame spends the frame step.
        let grid = self.clock.script_frame_delta();
        if grid == 0 {
            return;
        }
        for (slot, mut ramp) in std::mem::take(&mut self.npcs.face_legs) {
            if !self.step_npc_face_leg_by(slot, &mut ramp, grid) {
                self.npcs.face_legs.insert(slot, ramp);
            } else {
                self.land_npc_leg(slot);
            }
        }
        for (slot, mut leg) in std::mem::take(&mut self.npcs.rotate_legs) {
            leg.state.speed = grid;
            if !self.step_npc_rotate_leg(&mut leg) {
                self.npcs.rotate_legs.insert(slot, leg);
            } else {
                self.land_npc_leg(slot);
            }
        }
        // A compass walk a placement's load-frame slice armed: the kernel
        // spends the tick's frame step (`DAT_1F800393`) against the leg's
        // budget and, on landing, clears the context's halt bit `0x400` and
        // its `+0x54` cursor (the shared epilogue), so the record can be
        // poked again.
        // REF: FUN_8003BC08 (the 0x400 kernel visit), FUN_8003774C (the 0x37 / 0x41 arm)
        for (slot, mut leg) in std::mem::take(&mut self.npcs.glide_legs) {
            leg.state.speed = grid;
            if self.step_npc_glide(&mut leg) {
                self.npcs.glide_legs.insert(slot, leg);
                continue;
            }
            self.land_npc_leg(slot);
            let pos = self.npcs.positions.get(&slot).copied();
            if let Some(c) = self
                .field_vm
                .channels
                .iter_mut()
                .find(|c| !c.object_bind && c.placement_index == usize::from(slot))
            {
                c.ctx.flags &= !0x400;
                if let Some((x, z)) = pos {
                    c.ctx.world_x = x as u16;
                    c.ctx.world_z = z as u16;
                }
            }
        }
    }

    pub fn step_talk_face_ramp(
        &mut self,
        id: &mut legaia_engine_field::inline_dialogue::InlineDialogue,
    ) {
        let Some(mut ramp) = id.face_ramp else {
            return;
        };
        let target = self.talk_face_target(ramp.program[4]).or_else(|| {
            id.npc_slot
                .and_then(|slot| self.npcs.positions.get(&slot).copied())
        });
        let player = self
            .player_actor_slot
            .and_then(|slot| self.actors.get(usize::from(slot)))
            .map(|a| {
                (
                    a.move_state.world_x,
                    a.move_state.world_z,
                    a.move_state.render_26,
                )
            });
        let (Some((tx, tz)), Some((px, pz, yaw))) = (target, player) else {
            id.face_ramp = None;
            return;
        };
        let speed = self.clock.script_frame_delta();
        if speed == 0 {
            return;
        }
        let (yaw, done) = ramp.step(px, pz, yaw as u16, tx, tz, speed);
        if let Some(slot) = self.player_actor_slot
            && let Some(actor) = self.actors.get_mut(usize::from(slot))
        {
            actor.move_state.render_26 = yaw as i16;
        }
        id.face_ramp = if done { None } else { Some(ramp) };
    }

    /// Where the actor a talk's face-at bind names stands: the channel whose
    /// script id (`+0x50`) equals `bind` (`FUN_8003C83C`'s list walk) - a
    /// placement's live position, or an object-bind context's own seat.
    /// `None` for the specials and for an id no channel carries.
    // REF: FUN_8003C83C (the id resolve the kernel's FaceTarget arm shares)
    pub(super) fn talk_face_target(&self, bind: u8) -> Option<(i16, i16)> {
        let view = self.channel_view();
        let ch = &view[legaia_engine_field::field_channels::resolve_target(view, bind)?];
        let own = (ch.ctx.world_x as i16, ch.ctx.world_z as i16);
        if ch.object_bind {
            return Some(own);
        }
        let slot = u8::try_from(ch.placement_index).ok()?;
        Some(self.npcs.positions.get(&slot).copied().unwrap_or(own))
    }

    /// Live-loop bridge for the inline-script runner: when [`crate::world::WorldToggles::use_vm_dialogue`]
    /// is set, this starts the runner the frame a field dialogue opens (from
    /// [`legaia_engine_field::dialog_state::DialogState::current`]'s inline buffer), steps it from the current pad
    /// edges (Cross/Circle = confirm, Up/Down = menu cursor), and tears it down
    /// (clearing `current_dialog`) when the conversation ends. No-op when the
    /// flag is off, so the default simplified path is untouched.
    pub fn drive_inline_dialogue(&mut self) {
        if !self.toggles.use_vm_dialogue {
            return;
        }
        // A prop-bound record run (door / cupboard) is stepped by its own
        // driver ([`Self::step_prop_interaction`]) with prop-actor bridging;
        // stepping it here too would double-run its VM slices.
        if self
            .dialog
            .inline
            .as_ref()
            .is_some_and(|id| id.prop_anchor.is_some())
        {
            return;
        }
        // Start the runner the frame a dialogue request appears. When the opened
        // NPC carries a prologue record, run it from the entry PC so the
        // interaction prologue (segment selection) executes; otherwise start at
        // the first segment from the request's inline buffer.
        if self.dialog.inline.is_none() {
            if let Some(prologue) = self.dialog.active_inline_prologue.take() {
                let mut runner =
                    legaia_engine_field::inline_dialogue::InlineDialogue::with_prologue(
                        std::sync::Arc::new(prologue.body),
                        prologue.entry_pc,
                        prologue.first_segment,
                    );
                runner.npc_slot = self.dialog.active_inline_slot.take();
                self.dialog.inline = Some(runner);
            } else if let Some(req) = self.dialog.current.as_ref() {
                if !req.inline.is_empty() {
                    let slot = self.dialog.active_inline_slot.take();
                    self.start_inline_dialogue(req.inline.clone());
                    if let Some(runner) = self.dialog.inline.as_mut() {
                        runner.npc_slot = slot;
                    }
                } else {
                    return;
                }
            } else {
                return;
            }
        }
        let confirm = self.input.just_pressed(input::PadButton::Cross)
            || self.input.just_pressed(input::PadButton::Circle);
        let up = self.input.just_pressed(input::PadButton::Up);
        let down = self.input.just_pressed(input::PadButton::Down);
        self.step_inline_dialogue(confirm, up, down);
        if self.dialog.inline.as_ref().is_some_and(|d| d.is_done()) {
            // A talk that ended on a parking post-box byte leaves the actor's
            // cursor there (retail `actor[+0x9E]`), and the next talk on the
            // same actor resumes from it - `retock`'s innkeeper re-enters
            // through its `26` loop-back and its acquire.
            if let Some(id) = self.dialog.inline.as_ref()
                && let (Some(pc), Some(slot)) = (id.parked_pc, id.npc_slot)
                && let Some(rec) = self.npcs.dialog_prologue.get_mut(&slot)
            {
                rec.entry_pc = pc;
            }
            // A talk that ended on an executed raw `0x21` leaves the actor's
            // own context there too: the talk and the placement are one
            // retail context (`actor[+0x9E]`), and an engagement of that
            // context (`B1 <id> 08`) resumes past the `0x21`. Xain's fight
            // (`tunnelc` P1[4]: `3E FF 0A`, `21`) is staged from a talk, and
            // the system script's post-battle engagement runs the scene after
            // it (`+0xC45`, which sets `0x1D5`).
            if let Some(id) = self.dialog.inline.as_ref()
                && id.parked_pc.is_none()
                && let Some(slot) = id.npc_slot
                && id.pc > 0
                && id.bytecode.get(id.pc - 1) == Some(&0x21)
                && id.visited.get(id.pc - 1).copied().unwrap_or(false)
                && let Some(c) = self
                    .field_vm
                    .channels
                    .iter_mut()
                    .find(|c| !c.object_bind && c.placement_index == usize::from(slot))
            {
                c.pc = id.pc;
            }
            self.dialog.inline = None;
            self.dialog.current = None;
            // Drop the interaction's staging slots with it. They are consumed
            // by `take()` when the runner starts, so a leftover is always a
            // *second* arm of the same interaction - and this function's own
            // start arm would then relaunch the conversation on the next
            // frame, with no input, forever. The probe no longer re-arms them
            // mid-conversation (see `tick_field_interaction_probe`); clearing
            // here makes the restart unrepresentable rather than merely
            // unreachable, because any future caller of
            // `trigger_field_interact` would otherwise re-open the same trap.
            self.dialog.active_inline_prologue = None;
            self.dialog.active_inline_slot = None;
            self.pending_field_events
                .push(legaia_engine_field::field_events::FieldEvent::DialogDismissed);
        }
    }
}
