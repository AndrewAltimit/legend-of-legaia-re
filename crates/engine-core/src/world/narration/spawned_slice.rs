//! The spawned-record slice runner: the field VM's per-frame walk over a
//! spawned P2 record's script slice (`World::run_spawned_record_slice`).
//! Split out of `narration.rs`; no logic change.

use super::*;

impl World {
    pub(super) fn run_spawned_record_slice(
        &mut self,
        tl: &mut crate::cutscene_timeline::CutsceneTimeline,
        modal: bool,
    ) -> bool {
        tl.frames = tl.frames.saturating_add(1);
        tl.stepped = true;
        // The player's poked scene-bank clip plays one engine tick per
        // slice, parked or not - retail's clip tick runs every frame.
        tl.player_clip_ticks = tl.player_clip_ticks.saturating_sub(1);
        self.cutscene.in_timeline = modal;
        self.field_vm.in_spawned_record_slice = true;
        let mut channels = std::mem::take(&mut self.field_vm.channels);
        // Host hooks resolve cross-context ids against the channel set while
        // one of these is executing; the live vector is moved out for the
        // borrow, so they read this copy (`World::channel_view`).
        self.field_vm.stepping_view = channels.clone();
        let channel_pre_pos: Vec<(u16, u16)> = channels
            .iter()
            .map(|c| (c.ctx.world_x, c.ctx.world_z))
            .collect();
        // Cross-context channel handshake (`B3 <id> <bit>` = CFLAG_TST
        // against a spawned per-actor channel): the timeline PARKED here on a
        // prior tick. Retail's op-`0x33` arm holds the PC while the target's
        // bit is SET and advances once it is clear (`0x801DEE44`), so re-test
        // it before stepping - rather than the pre-handshake behaviour of
        // advancing past the wait by instruction width.
        if let Some(mut wait) = tl.channel_wait.take() {
            let flag_set = crate::field_channels::resolve_target(&channels, wait.target_id)
                .map(|ci| channels[ci].ctx.flags & (1u32 << (wait.bit & 0x1F)) != 0);
            match flag_set {
                // Still waiting (the channel's bit is still up) and within the
                // park budget: hold the PC on the flag-test op another tick.
                Some(true) if wait.frames < CHANNEL_WAIT_PARK_TIMEOUT => {
                    wait.frames += 1;
                    tl.channel_wait = Some(wait);
                    self.field_vm.channels = channels;
                    self.field_vm.stepping_view.clear();
                    self.cutscene.in_timeline = false;
                    self.field_vm.in_spawned_record_slice = false;
                    return false;
                }
                // The channel dropped the flag (resume), the target is gone, or
                // the park timed out: step past the flag-test op by its encoded
                // width and let the timeline flow. (An extended flag-test is
                // `header 2 + 1 operand` = 3 bytes.)
                _ => {
                    let header_size = if tl.bytecode.get(tl.pc).copied().unwrap_or(0) & 0x80 != 0 {
                        2
                    } else {
                        1
                    };
                    tl.pc += header_size + 1;
                }
            }
        }
        // Player-channel (`0xF8`) arc park: the timeline is holding at a
        // `C3 F8` op while the player's scripted arc flies (retail halts the
        // caller with the player and the arc's watcher releases both on
        // landing). When no arc could start, the countdown armed by a
        // preceding `A2 F8 <move_id>` stands in for the playout instead. Either
        // way, once it clears, step PAST the op by its encoded width so the
        // record flows on to its trailing ops (the door records' terminal
        // `0x3F`).
        // REF: FUN_8003BDE0
        if let Some(width) = tl.player_wait.take() {
            tl.player_move_frames = tl.player_move_frames.saturating_sub(1);
            if tl.player_move_frames > 0 || self.player_script_arc_live() {
                tl.player_wait = Some(width);
                self.field_vm.channels = channels;
                self.field_vm.stepping_view.clear();
                self.cutscene.in_timeline = false;
                self.field_vm.in_spawned_record_slice = false;
                return false;
            }
            tl.pc += width;
        }
        // Cross-context walk-to-tile park (`C7 <id> <tx> <tz> <mode>` = op
        // 0x47 against an NPC channel / the player anchor): retail saves the
        // yield-op pointer into the TARGET actor's `+0x94` and the per-frame
        // walk kernel (`FUN_8003774C` case 0x47) moves it toward the tile at
        // `0x80 >> (2 + (mode & 7))` per frame, resuming the parked record on
        // arrival. The NPC leg glides through the motion VM
        // (`tick_field_npc_motions` removes the leg on arrival); the player
        // leg steps here directly. The town01 Mei walk-on beat is the pinned
        // case: `C7 46 11 1B 33` / `C7 46 11 1A 33` walk Mei from her seat at
        // the Vahn's-house door into the conversation frame, and
        // `C7 F8 12 1A 33` walks the player to the beat's camera focus -
        // dropping these left Mei OFFSCREEN (top corner of the shot) for the
        // whole conversation.
        // REF: FUN_8003774C (case 0x47), FUN_8003BC08 (0x400 walk-bit tick)
        if let Some(mut walk) = tl.walk_wait.take() {
            walk.frames += 1;
            let arrived = match walk.slot {
                Some(slot) => !self.npcs.motions.contains_key(&slot),
                None => self.step_player_walk_leg(walk.target, walk.speed),
            };
            if !arrived && walk.frames < WALK_PARK_TIMEOUT {
                tl.walk_wait = Some(walk);
                self.field_vm.channels = channels;
                self.field_vm.stepping_view.clear();
                self.cutscene.in_timeline = false;
                self.field_vm.in_spawned_record_slice = false;
                // A walk park is real playout progress, not a hang: don't let
                // it accumulate toward the anti-hang frame cap (a long leg -
                // tower P2[2]'s `C7 F8 0D 45` covers ~7500 units at 4/tick -
                // would otherwise burn the cap mid-walk and the forced
                // completion would skip the record's trailing flag latches).
                tl.frames = tl.frames.saturating_sub(1);
                return false;
            }
            // Arrived (or safety timeout: snap to the target so the
            // choreography stays coherent) - resume past the yield.
            if !arrived {
                match walk.slot {
                    Some(slot) => {
                        self.npcs.motions.remove(&slot);
                        self.npcs.positions.insert(slot, walk.target);
                    }
                    None => {
                        let y = self.sample_field_floor_height(
                            i32::from(walk.target.0),
                            i32::from(walk.target.1),
                        ) as i16;
                        if let Some(p) = self.player_actor_slot
                            && let Some(actor) = self.actors.get_mut(p as usize)
                        {
                            actor.move_state.world_x = walk.target.0;
                            actor.move_state.world_z = walk.target.1;
                            actor.move_state.world_y = y;
                        }
                    }
                }
            }
            tl.pc = walk.resume_pc;
        }
        // Player compass walk (`B7 F8 <b0> <b1>` / `C1 F8 ..`, ops `0x37` /
        // `0x41` against the player anchor): the walk kernel translates the
        // player in place, one speed unit per vsync, while the record runs on
        // past the op; the record's next cross-context op on the player
        // waits until the leg is spent (see the halted-target refusal below).
        //
        // Retail's kernel runs after the script in the record's actor tick
        // (`FUN_8003BC08`: runner at `0x8003BD34`, kernel at `0x8003BD50`),
        // so the leg spends its first unit on the frame the op arms it, and
        // a leg that lands frees the player for the script only on the next
        // frame. `glide_hold` carries that frame.
        // REF: FUN_8003774C (the 0x37 / 0x41 arm), FUN_8003BC08
        let mut glide_hold = false;
        if let Some(mut glide) = tl.player_glide.take() {
            if self.step_player_glide(&mut glide) {
                tl.player_glide = Some(glide);
            }
            glide_hold = true;
        }
        // NPC compass walks (`B7 <id> ..` / `C1 <id> ..`): the same in-place
        // kernel against a placement. A leg that lands this frame keeps its
        // actor held for the script until the next one, like `glide_hold`.
        // REF: FUN_8003774C (the 0x37 / 0x41 arm), FUN_8003BC08
        let mut npc_glide_hold: Vec<u8> = Vec::new();
        for mut glide in std::mem::take(&mut tl.npc_glides) {
            npc_glide_hold.push(glide.slot);
            if self.step_npc_glide(&mut glide) {
                tl.npc_glides.push(glide);
            }
        }
        // NPC walk-to-tile legs (`C7 <id> ..`) run on the motion VM
        // (`tick_field_npc_motions` drops the leg on arrival); the actor stays
        // held for the script while its leg is live and on the landing frame.
        // REF: FUN_801DE840 (0x801DEFC0..0x801DF054)
        for slot in std::mem::take(&mut tl.npc_walks) {
            npc_glide_hold.push(slot);
            if self.npcs.motions.contains_key(&slot) {
                tl.npc_walks.push(slot);
            }
        }
        // NPC rotate legs (`B8 <id> <dir> <budget>`): the same in-place
        // `0x38` RotateToAngle kernel the player park runs, stepped here
        // while the record runs on; the actor stays held for the script
        // until the ramp snaps, and on the snap frame.
        // REF: FUN_801DE840 (0x801DEE90..0x801DEF24), FUN_8003774C (case 0x38)
        for mut fw in std::mem::take(&mut tl.npc_facings) {
            if let Some(slot) = fw.slot {
                npc_glide_hold.push(slot);
            }
            fw.frames += 1;
            let r = vm::motion_vm::step(
                &mut fw.state,
                vm::motion_vm::MotionTarget::default(),
                &fw.program,
            );
            if fw.state.yaw_written {
                self.set_timeline_facing(fw.slot, fw.state.yaw as i16);
            }
            if r != vm::motion_vm::StepResult::Done && fw.frames < WALK_PARK_TIMEOUT {
                tl.npc_facings.push(fw);
            }
        }
        // NPC face-at legs (`CC <id> 85|8E|8F ..`): the target's walk kernel
        // turns it toward the bind while the record runs on; the actor stays
        // held for the script until the leg's terminal frame, and on it.
        // REF: FUN_8003774C (the 0x4C arm), FUN_8003BC08
        for mut face in std::mem::take(&mut tl.npc_faces) {
            npc_glide_hold.push(face.slot);
            face.frames += 1;
            if !self.step_npc_face_leg(face.slot, &mut face.ramp) && face.frames < WALK_PARK_TIMEOUT
            {
                tl.npc_faces.push(face);
            }
        }
        // Player end-latch spin (`AD F8 08`): held while the scene-bank clip
        // the record poked onto the player is still playing; retail's clip
        // tick latches `+0x62 & 0x100` on its last frame and the spin falls
        // through on the next visit.
        // REF: FUN_800204F8 (0x800206E4..0x8002072C)
        if let Some(width) = tl.player_clip_wait.take() {
            if tl.player_clip_ticks > 0 {
                tl.player_clip_wait = Some(width);
                self.field_vm.channels = channels;
                self.field_vm.stepping_view.clear();
                self.cutscene.in_timeline = false;
                self.field_vm.in_spawned_record_slice = false;
                tl.frames = tl.frames.saturating_sub(1);
                return false;
            }
            tl.pc += width;
        }
        // Cross-context rotate park (`B8 <id> <dir|flags> <budget|dir>` = op
        // 0x38 with a non-zero budget against an NPC channel): retail parks
        // the record and the `FUN_8003774C` 0x38 RotateToAngle leg ramps the
        // target actor's `+0x26` linearly over the operand budget - per-op
        // turn rates (`arc / budget`), raw pre-unwrap mid-ramp headings, and
        // an exact terminal snap onto the compass entry. Step the parked leg
        // once per tick, mirror the raw yaw into the render-heading map, and
        // resume the record past the yield when the ramp snaps.
        // REF: FUN_8003774C (case 0x38 interpreted in place)
        if let Some(mut fw) = tl.facing_wait.take() {
            fw.frames += 1;
            let r = vm::motion_vm::step(
                &mut fw.state,
                vm::motion_vm::MotionTarget::default(),
                &fw.program,
            );
            if fw.state.yaw_written {
                // Raw write-back (`yaw` may sit outside 0..0xFFF mid-ramp,
                // exactly as retail's `+0x26` does); render consumers mask.
                self.set_timeline_facing(fw.slot, fw.state.yaw as i16);
            }
            if r != vm::motion_vm::StepResult::Done && fw.frames < WALK_PARK_TIMEOUT {
                tl.facing_wait = Some(fw);
                self.field_vm.channels = channels;
                self.field_vm.stepping_view.clear();
                self.cutscene.in_timeline = false;
                self.field_vm.in_spawned_record_slice = false;
                // Like the walk park: a rotate park is real playout progress,
                // not a hang - keep it off the anti-hang frame cap.
                tl.frames = tl.frames.saturating_sub(1);
                return false;
            }
            tl.pc = fw.resume_pc;
        }
        // Player face-at park (`CC F8 85|8E|8F <lo> <hi> <id>`): the walk
        // kernel's FaceTarget leg turns the player toward the named actor,
        // and the record resumes past the acquire on the leg's terminal
        // frame - see `CutsceneTimeline::player_face`.
        // REF: FUN_8003774C (the 0x4C arm)
        if let Some((mut ramp, resume_pc, frames)) = tl.player_face.take() {
            let done = self.step_player_face_leg(&mut ramp);
            if !done && frames < WALK_PARK_TIMEOUT {
                tl.player_face = Some((ramp, resume_pc, frames + 1));
                self.field_vm.channels = channels;
                self.field_vm.stepping_view.clear();
                self.cutscene.in_timeline = false;
                self.field_vm.in_spawned_record_slice = false;
                tl.frames = tl.frames.saturating_sub(1);
                return false;
            }
            tl.pc = resume_pc;
        }
        {
            let mut host = FieldHostImpl { world: self };
            let mut budget = CUTSCENE_TIMELINE_STEP_BUDGET;
            while budget > 0 {
                budget -= 1;
                let pc = tl.pc;
                // Arrived at an inline narration block.
                //
                // Crawl (`op0 0x80`): retail spawns the roller as a CHILD
                // context (`FUN_80037174`) and keeps executing THIS parent
                // timeline, so the camera cuts / fades / waits authored after
                // the block play UNDER the scrolling text - for EVERY block,
                // the last included. (Pinned by the `map01` fly-in retail
                // capture: its last crawl's authored `4A` 600 + 330 tail runs
                // concurrent with the roller - the leg span only fits the
                // authored waits, leaving no room for a serialized roller.)
                // If a prior roller is still scrolling when a block is
                // reached, hold (don't stack rollers) until it drains, then
                // re-enter to open this one. Nothing holds the final pages:
                // the record's terminal SceneChange runs on under them.
                //
                // Title card (`op0 0x89`): the pages show simultaneously
                // while the parent CONTINUES; a card whose pages are blank
                // clears the overlay. Skip past the block either way.
                if let Some(site) = tl.narration_blocks.iter().find(|b| b.op_offset == pc) {
                    match site.kind {
                        legaia_asset::cutscene_text::NarrationKind::Crawl => {
                            if host.world.cutscene_narration_active() {
                                tl.narration_pc = Some(pc);
                                tl.narration_pending_open = true;
                                break;
                            }
                            let site_end = site.end;
                            let pages = site.pages.clone();
                            // The `CC F8 E8` geometry seed the field VM runs
                            // immediately before the block (retail stores it
                            // into `*0x801C6EA4 +0x4C..+0x50`; a block with no
                            // seed op of its own reads the one left there).
                            if let Some(seed) = host
                                .world
                                .cutscene
                                .narration_seed
                                .config_op_before(&tl.bytecode, pc)
                            {
                                host.world.cutscene.narration_seed = seed;
                            }
                            host.world.open_cutscene_narration(pages);
                            // Non-blocking: the roller scrolls on its own
                            // (`World::tick`); continue into the camera cuts.
                            tl.pc = site_end;
                            continue;
                        }
                        legaia_asset::cutscene_text::NarrationKind::Card => {
                            let blank = site.pages.iter().all(|p| p.trim().is_empty());
                            host.world.cutscene.card = if blank {
                                None
                            } else {
                                Some(site.pages.clone())
                            };
                            tl.pc = site.end;
                            continue;
                        }
                    }
                }
                // Retail dialog-SM transition test (`FUN_80039B7C`): an
                // in-bounds byte with `& 0x7F < 0x20` is a text-segment lead
                // (`0x1F`) or a terminator (`0x00..0x1E`), not an opcode. A
                // `0x1F` opens an inline dialog box over the record bytes and
                // parks the timeline at the segment (resumed by the pre-step
                // gate when the player dismisses it). A stray terminator the
                // flow lands on is consumed (skipped) - timeline records
                // continue with choreography ops after their conversation, so
                // ending here would drop the record's closing flag-sets.
                // Running OFF the record end falls through to the VM step
                // instead (its `Unknown` completes the timeline).
                if let Some(&text_byte) = tl.bytecode.get(pc)
                    && text_byte & 0x7F < 0x20
                {
                    if text_byte == 0x1F {
                        // Modal timeline or concurrent helper alike: the
                        // runner `FUN_80039B7C` parks ANY engaged context on
                        // its text segment and hands it to the shared dialog
                        // box (`+0x9C = 2`). A helper used to complete at its
                        // first segment instead, which dropped the rest of
                        // every op-`0x44` record with a line of text in it -
                        // town01 `P2[25]` (the FMV hand-off to town0b) and
                        // town0e `P2[5]` (the ending's hop to edteien).
                        //
                        // Resolve the record's `0xC1`/`0xC2`/`0xC4` name
                        // escapes, exactly as the prop-interaction panel
                        // does, or a name renders as an empty string.
                        let _ = modal;
                        let mut panel = crate::dialog::OwnedDialogPanel::at_segment(
                            std::sync::Arc::clone(&tl.bytecode),
                            pc,
                        );
                        panel.substitutions = host.world.dialog_substitutions(&tl.bytecode);
                        tl.dialog = Some(panel);
                        host.world.field_vm.dialog_claims += 1;
                        tl.dialog_claim = host.world.field_vm.dialog_claims;
                        break;
                    }
                    tl.pc = pc + 1;
                    continue;
                }
                let opcode_byte = tl.bytecode.get(pc).copied().unwrap_or(0);
                // A terminal SceneChange (`0x3F`) does NOT wait for a roller
                // still scrolling: the op's arm only builds the scene-change
                // packet (`FUN_8001FD44`), and a per-vsync capture of the
                // zero-input `opdeene` leg has the record execute its `3F`
                // with the 8-page Seru-history roller (`FUN_80037174`) three
                // pages short of retiring; the departing scene tears the
                // roller down with everything else.
                // A crawl's roller holds the player's halt bit for its whole
                // run: the `CC F8 80 N` spawn is a halt-acquire on its target
                // (`ori v0,v0,0x400` into the player's `+0x10` at
                // `0x801E1F24`), and the roller clears its parent's `0x400`
                // as it retires the last page. So `B3 F8 0A` - the halt-bit
                // test on the player - is how a record waits for its crawl:
                // `opurud`'s record sits on one before its `3F` until the
                // last page retires (per-vsync capture), while `opdeene`'s
                // carries none and changes scene under its roller.
                // REF: FUN_801DE840 (0x801E1ECC..0x801E1F58), FUN_80037174
                if opcode_byte == 0xB3
                    && tl.bytecode.get(pc + 1) == Some(&0xF8)
                    && tl.bytecode.get(pc + 2) == Some(&0x0A)
                    && host.world.cutscene_narration_active()
                {
                    tl.frames = tl.frames.saturating_sub(1);
                    break;
                }
                // Halted-target refusal: a cross-context op aimed at an actor
                // ANOTHER context holds in a walk / rotate / glide park waits
                // at the op and retries next frame. The park holds the
                // target's halt bit `0x400` until the kernel lands it, and
                // the dispatcher's prologue returns with the PC still on the
                // op for any target carrying `0x400` while the scene word
                // `*(_DAT_801C6EA4) + 8` is zero, unless the caller is the
                // system context `0xFB`. A spawned record's context is never
                // that one: `FUN_8003BDE0` stamps its global record index into
                // `+0x50` (`0x8003C094`); the `0xFB` this context carries is
                // the engine's stand-in. Without the refusal two records
                // walked the player at once: `dolk2` P2[15]'s
                // `C7 F8 46 4C 33` pulled against P2[12]'s `C7 F8 48 53 23`,
                // and the tug-of-war left the party inside the wall at tile
                // (66, 87).
                // REF: FUN_801DE840 (0x801DE90C..0x801DE944), FUN_8003BDE0
                let halted_target = opcode_byte & 0x80 != 0
                    && !host.world.field_vm.halted_elsewhere.is_empty()
                    && match vm::field::peek_extended(&tl.bytecode, pc) {
                        Some(0xF8) => host.world.field_vm.halted_elsewhere.contains(&None),
                        Some(t) => crate::field_channels::resolve_target(&channels, t)
                            .filter(|&ci| !channels[ci].object_bind)
                            .is_some_and(|ci| {
                                host.world
                                    .field_vm
                                    .halted_elsewhere
                                    .contains(&Some(channels[ci].placement_index as u8))
                            }),
                        None => false,
                    };
                // The player walk this context armed itself holds the player's
                // `0x400` the same way: the `0x37` / `0x41` arm advances the
                // record past the op (`s7 = 3` at `0x801DEEFC` for a player
                // target) and the next cross-context op on the player waits
                // for the leg to land - `map01`'s credits record sits on its
                // `B8 F8 82 08` at `+0x97` while the `C1 F8 03 C4` leg before
                // it walks Vahn. `32 <id> 0A` (the halt clear) is exempt
                // (`0x801DE8E0..0x801DE904`).
                // REF: FUN_801DE840 (0x801DEE90..0x801DEF1C)
                let own_glide_target = opcode_byte & 0x80 != 0
                    && (tl.player_glide.is_some() || glide_hold)
                    && vm::field::peek_extended(&tl.bytecode, pc) == Some(0xF8)
                    && !(opcode_byte & 0x7F == 0x32 && tl.bytecode.get(pc + 2) == Some(&0x0A));
                // The NPC legs this context armed hold their actors the
                // same way (the `s7 = 3` advance is taken for every target).
                // That includes a walk-to-tile leg armed earlier in this same
                // slice (`tl.npc_walks`): `dolk2` P2[11] runs `C7 1F 46 5B 32`
                // then `B3 1F 0A` within one tick, and the halt-bit verify
                // must hold until Noa lands - stepping past it opened the
                // "Noa: Vahn..." box with Noa still walking through Vahn.
                let own_npc_glide_target = opcode_byte & 0x80 != 0
                    && (!tl.npc_glides.is_empty()
                        || !tl.npc_walks.is_empty()
                        || !tl.npc_faces.is_empty()
                        || !npc_glide_hold.is_empty())
                    && !(opcode_byte & 0x7F == 0x32 && tl.bytecode.get(pc + 2) == Some(&0x0A))
                    && vm::field::peek_extended(&tl.bytecode, pc)
                        .filter(|&t| t != 0xF8 && t != 0xFB)
                        .and_then(|t| crate::field_channels::resolve_target(&channels, t))
                        .filter(|&ci| !channels[ci].object_bind)
                        .is_some_and(|ci| {
                            let slot = channels[ci].placement_index as u8;
                            npc_glide_hold.contains(&slot)
                                || tl.npc_glides.iter().any(|g| g.slot == slot)
                                || tl.npc_walks.contains(&slot)
                                || tl.npc_faces.iter().any(|f| f.slot == slot)
                        });
                if halted_target || own_glide_target || own_npc_glide_target {
                    tl.frames = tl.frames.saturating_sub(1);
                    break;
                }
                // Cross-context dispatch (`0x80`-bit ops): resolve the target
                // byte to a spawned per-actor channel (`ctx[+0x50] == target`,
                // retail `FUN_8003C83C`) and run the op against THAT context -
                // this is how the timeline cues the vignette actors. `0xF8`
                // (player anchor) / `0xFB` (system) keep the timeline's own
                // context (the player pokes route through host hooks).
                //
                // Partition-0 object contexts resolve too: the scene entry
                // spawns an object-bind channel per `.MAP` object script
                // (retail `FUN_8003A55C` writes the gate-0 trigger's flat
                // record index into `actor[+0x50]`), so the Mei beat's `0x01`
                // pokes land on the Vahn's-house door context. An id that
                // STILL matches no channel is skipped by its decoded width
                // instead: running it against the timeline's own ctx
                // corrupted the timeline (a `B1 <id> 00` set the timeline's
                // OWN busy bit, and the `CC <id> A0` busy-wait then hijacked
                // the caller PC into the record header).
                let target = vm::field::peek_extended(&tl.bytecode, pc).and_then(|t| {
                    crate::field_channels::resolve_target(&channels, t).map(|ci| (t, ci))
                });
                if target.is_none()
                    && let Some(t) = vm::field::peek_extended(&tl.bytecode, pc)
                    && t != 0xF8
                    && t != 0xFB
                    && let Ok(insn) = legaia_asset::field_disasm::decode(&tl.bytecode, pc)
                {
                    if pc < tl.visited.len() {
                        tl.visited[pc] = true;
                    }
                    tl.pc = pc + insn.size;
                    continue;
                }
                // Player-anchor channel (`0xF8`) ExecMove / halt-acquire
                // completion model. Retail resolves `0xF8` to the live player
                // object (`_DAT_8007C364`, the `FUN_8003C83C` special-target
                // arm) - not a spawned channel - so `resolve_target` keeps its
                // `None` contract, and the two ops the door-cutscene records
                // drive the player with are modelled here instead of falling
                // through to the timeline's own ctx:
                //
                // - `A2 F8 <move_id>` (op 0x22 ExecMove): retail pokes the
                //   move-table clip onto the player and lets it play out over
                //   the following frames. Emit the same `ExecMove` field
                //   event and arm a short completion countdown standing in
                //   for the playout.
                // - `C3 F8 <sub> …` (op 0x43 sub-0/1/A/B halt-acquire):
                //   retail halts the caller and state-resumes it at the
                //   operand s16 once the player move completes. That resume
                //   PC points BACKWARD into the poke loop (jou `P2[5]`:
                //   `C3 F8 00 5E E2 50` at `+0x60` resumes at `+0x50`), so
                //   taking the VM's yield here spins the timeline until the
                //   frame cap kills it WITHOUT the trailing `0x3F` scene
                //   change. Instead PARK at the op until the armed countdown
                //   drains (the pre-step gate above), then step PAST it by
                //   encoded width - the completion side of the handshake. A
                //   halt-acquire with no move in flight completes at once.
                //   (The op-0x38 halt-acquire variant resumes FORWARD at its
                //   post-instruction PC, so its yield is already
                //   completion-shaped and needs no special case.)
                // REF: FUN_8003C83C
                // REF: FUN_8003BDE0
                // Cross-context walk-to-tile yield (`C7 <id|F8> <tx> <tz>
                // <mode>`): retail parks the record and the walk kernel
                // (`FUN_8003774C` case 0x47) moves the TARGET toward the tile
                // in place. Arm the walk + park; the pre-step gate resumes
                // past the op on arrival. The despawn form (tile 127,127)
                // seats instantly - walking to the off-map box is invisible.
                // REF: FUN_8003774C (case 0x47)
                if opcode_byte & 0x7F == 0x47
                    && opcode_byte & 0x80 != 0
                    && let (Some(&b0), Some(&b1), Some(&b2)) = (
                        tl.bytecode.get(pc + 2),
                        tl.bytecode.get(pc + 3),
                        tl.bytecode.get(pc + 4),
                    )
                {
                    let ext = vm::field::peek_extended(&tl.bytecode, pc);
                    let decode = |b: u8| -> i16 {
                        i16::from(b & 0x7F) * 0x80 + 0x40 + if b & 0x80 != 0 { 0x40 } else { 0 }
                    };
                    let (tx, tz) = (decode(b0), decode(b1));
                    let speed = crate::world::field_npc_walk_step_speed(0x80, b2 & 7);
                    let parked_sentinel =
                        (b0 & 0x7F, b1 & 0x7F) == crate::man_field_scripts::PARKED_SENTINEL_TILE;
                    let walk_slot: Option<Option<u8>> = if ext == Some(0xF8) {
                        Some(None) // player anchor
                    } else if let Some((_, ci)) = target {
                        (!channels[ci].object_bind)
                            .then_some(Some(channels[ci].placement_index as u8))
                    } else {
                        None
                    };
                    if let Some(slot) = walk_slot {
                        if pc < tl.visited.len() {
                            tl.visited[pc] = true;
                        }
                        if let Some(s) = slot {
                            if let Some((_, ci)) = target {
                                channels[ci].ctx.world_x = tx as u16;
                                channels[ci].ctx.world_z = tz as u16;
                            }
                            if parked_sentinel {
                                // Despawn: seat at the hide box, no playout.
                                host.world.npcs.positions.insert(
                                    s,
                                    (
                                        crate::world::FIELD_OFFMAP_HIDE_XZ,
                                        crate::world::FIELD_OFFMAP_HIDE_XZ,
                                    ),
                                );
                                tl.pc = pc + 5;
                                continue;
                            }
                            if host.world.start_field_npc_motion(s, tx, tz) {
                                if let Some(m) = host.world.npcs.motions.get_mut(&s) {
                                    m.state.speed = speed;
                                    // The mode byte's high nibble picks the
                                    // walk kernel's approach (`srl a1,a1,0x4`
                                    // at `0x80037BEC`).
                                    m.state.approach = b2 >> 4;
                                }
                                // The record runs on past an NPC walk (`li
                                // s7,4` in the delay slot at `0x801DF030`);
                                // only a player target parks the caller.
                                tl.npc_walks.retain(|&w| w != s);
                                tl.npc_walks.push(s);
                                tl.pc = pc + 5;
                                continue;
                            } else {
                                // No surfaced live position to glide from:
                                // seat directly (the pre-park fallback).
                                host.world.npcs.positions.insert(s, (tx, tz));
                                tl.pc = pc + 5;
                                continue;
                            }
                        } else if parked_sentinel {
                            tl.pc = pc + 5;
                            continue;
                        }
                        tl.walk_wait = Some(crate::cutscene_timeline::TimelineWalk {
                            slot,
                            target: (tx, tz),
                            resume_pc: pc + 5,
                            speed,
                            frames: 0,
                        });
                        break;
                    }
                }
                // Halt clear on an NPC (`B2 <id> 0A`, op 0x32 bit 10): the
                // actor tick runs the walk kernel only while `+0x10 & 0x400`
                // is up (`FUN_8003BC08`), so clearing the bit ends whatever
                // leg this context armed on it, where it stands. `opdeene`
                // cuts the two Seru's `C1` legs this way before their next
                // beat; leaving them running held every later op on both
                // actors for the rest of the legs.
                // REF: FUN_8003BC08, FUN_8003774C
                if opcode_byte == 0xB2
                    && tl.bytecode.get(pc + 2) == Some(&0x0A)
                    && let Some((_, ci)) = target
                    && !channels[ci].object_bind
                {
                    let slot = channels[ci].placement_index as u8;
                    tl.npc_glides.retain(|g| g.slot != slot);
                    tl.npc_facings.retain(|f| f.slot != Some(slot));
                    tl.npc_faces.retain(|f| f.slot != slot);
                    if tl.npc_walks.contains(&slot) {
                        tl.npc_walks.retain(|&w| w != slot);
                        host.world.npcs.motions.remove(&slot);
                    }
                    npc_glide_hold.retain(|&s| s != slot);
                }
                // Cross-context compass walk on an NPC (`B7 <id> <b0> <b1>` /
                // `C1 <id> ..` = op 0x37 / 0x41 against a placement channel):
                // retail's arm seats the op on the target's `+0x94`, raises
                // its `0x400` and advances the record past the op - the same
                // `s7 = 3` exit the player target takes - so arm the leg and
                // run on; the record's next op on this actor waits for it
                // (the refusal above). The leg replaces any walk the actor
                // had in flight: retail overwrites `+0x94`. Dropping it left
                // bylon's Maya at the top of the shrine stairs, out of frame,
                // for her whole first conversation.
                // REF: FUN_801DE840 (0x801DEE90..0x801DEF1C), FUN_8003774C (the 0x37 / 0x41 arm)
                if matches!(opcode_byte & 0x7F, 0x37 | 0x41)
                    && opcode_byte & 0x80 != 0
                    && let (Some(&body0), Some(&body1)) =
                        (tl.bytecode.get(pc + 2), tl.bytecode.get(pc + 3))
                    && let Some((_, ci)) = target
                    && !channels[ci].object_bind
                {
                    let slot = channels[ci].placement_index as u8;
                    if pc < tl.visited.len() {
                        tl.visited[pc] = true;
                    }
                    host.world.npcs.motions.remove(&slot);
                    // An actor nothing has moved yet stands where its context
                    // was seated (retail's kernel walks the live `+0x14` /
                    // `+0x18`, which the spawn wrote). Without the seat the
                    // leg had no start and was dropped: `opdeene`'s vignette
                    // actor `0x05` never took its `C1 05 00 C4` step.
                    host.world.npcs.positions.entry(slot).or_insert((
                        channels[ci].ctx.world_x as i16,
                        channels[ci].ctx.world_z as i16,
                    ));
                    let mut glide = crate::cutscene_timeline::TimelineNpcGlide {
                        slot,
                        state: vm::motion_vm::MotionState {
                            speed: 1,
                            ..Default::default()
                        },
                        body0,
                        body1,
                        rate: if opcode_byte & 0x7F == 0x37 {
                            0x80
                        } else {
                            0x40
                        },
                        frames: 0,
                    };
                    // Retail's kernel runs after the script in the same actor
                    // tick, so the leg spends its first unit on the arming
                    // frame (the player arm does the same).
                    tl.npc_glides.retain(|g| g.slot != slot);
                    if host.world.step_npc_glide(&mut glide) {
                        tl.npc_glides.push(glide);
                    }
                    npc_glide_hold.push(slot);
                    tl.pc = pc + 4;
                    continue;
                }
                // Cross-context facing op (`B8 <id> <op0> <op1>` = op 0x38
                // CAM_CFG against a spawned NPC channel).
                //
                // - Simple path (`op1 & 0x7F == 0`): retail copies the
                //   compass-LUT entry `0x80073F04 + (op0 & 0xF) * 2` straight
                //   into the target's `+0x26` - an instant scripted pose.
                // - Budget path: the halt-acquire arm parks the record and
                //   the op bytes run in place as the walk kernel's `0x38`
                //   RotateToAngle leg - a linear ramp at the op's own
                //   `arc / budget` rate with an exact terminal compass snap
                //   (the town01 Mei dinner beat authors seven of these at
                //   budgets 0x12..0x20). Arm the rotate park; the pre-step
                //   gate plays it out and resumes past the yield.
                // REF: FUN_801DE840 (case 0x38), FUN_8003774C (case 0x38)
                //
                // The player (`B8 F8 ..`, `FUN_8003C83C` resolving `0xF8` to
                // the player object) takes both paths the same way, on its
                // own heading - every story beat turns the hero with these.
                let facing_target = match target {
                    Some((_, ci)) if !channels[ci].object_bind => {
                        Some(Some(channels[ci].placement_index as u8))
                    }
                    None if vm::field::peek_extended(&tl.bytecode, pc) == Some(0xF8) => Some(None),
                    _ => None,
                };
                if opcode_byte & 0x7F == 0x38
                    && opcode_byte & 0x80 != 0
                    && let (Some(&op0), Some(&op1)) =
                        (tl.bytecode.get(pc + 2), tl.bytecode.get(pc + 3))
                    && let Some(slot) = facing_target
                {
                    if pc < tl.visited.len() {
                        tl.visited[pc] = true;
                    }
                    if op1 & 0x7F == 0 {
                        if let Some(h) =
                            crate::man_field_scripts::facing_index_to_engine_heading(op0 & 0xF)
                        {
                            host.world.set_timeline_facing(slot, h);
                        }
                        tl.pc = pc + 4;
                        continue;
                    }
                    // Seed from the target's live heading (retail reads the
                    // live `+0x26`); a never-posed NPC stands at the retail
                    // spawn default 0 = engine 0x800.
                    let cur = host.world.timeline_facing(slot).unwrap_or(0x800);
                    let leg = crate::cutscene_timeline::TimelineFacing {
                        slot,
                        state: vm::motion_vm::MotionState {
                            yaw: (cur as u16) & 0x0FFF,
                            // The timeline ticks once per retail display
                            // frame, so speed 1 maps the operand budget 1:1
                            // to parked ticks (retail consumes the same
                            // budget at `_DAT_1F800393` per actor tick).
                            speed: 1,
                            ..Default::default()
                        },
                        program: [0x38, op0, op1],
                        resume_pc: pc + 4,
                        frames: 0,
                    };
                    // Only a player target parks the caller; an NPC's turn
                    // plays out while the record runs on (`li s7,3` in the
                    // delay slot at `0x801DEEFC`).
                    if slot.is_some() {
                        tl.npc_facings.retain(|f| f.slot != slot);
                        tl.npc_facings.push(leg);
                        tl.pc = pc + 4;
                        continue;
                    }
                    tl.facing_wait = Some(leg);
                    break;
                }
                // Halt-acquire of an NPC (`CC <id> 85|8E|8F <lo> <hi> <bind>`
                // against a placement channel): the target turns to face the
                // bind over the op's budget while the record runs on past the
                // op (`CutsceneTimeline::npc_faces`). Run as a plain
                // halt-acquire on the channel context, it set a flag and
                // nothing turned: Noa and Gala faced wherever their last
                // walk left them through every "turns to Vahn" beat.
                // REF: FUN_801DE840 (0x801E2148..0x801E21DC)
                if let Some((_, ci)) = target
                    && !channels[ci].object_bind
                    && let Some((_, ramp)) =
                        crate::inline_dialogue::TalkFaceRamp::from_npc_acquire(&tl.bytecode, pc)
                {
                    if pc < tl.visited.len() {
                        tl.visited[pc] = true;
                    }
                    let slot = channels[ci].placement_index as u8;
                    host.world.npcs.positions.entry(slot).or_insert((
                        channels[ci].ctx.world_x as i16,
                        channels[ci].ctx.world_z as i16,
                    ));
                    // The kernel runs in the same actor tick that armed it, so
                    // the leg takes its first frame now.
                    let mut face = crate::cutscene_timeline::TimelineNpcFace {
                        slot,
                        ramp,
                        frames: 1,
                    };
                    tl.npc_faces.retain(|f| f.slot != slot);
                    tl.npc_facings.retain(|f| f.slot != Some(slot));
                    if !host.world.step_npc_face_leg(slot, &mut face.ramp) {
                        tl.npc_faces.push(face);
                    }
                    npc_glide_hold.push(slot);
                    tl.pc = pc + 6;
                    continue;
                }
                if vm::field::peek_extended(&tl.bytecode, pc) == Some(0xF8) {
                    let op = opcode_byte & 0x7F;
                    // Halt-acquire of the player (`CC F8 85|8E|8F`): the
                    // player turns to face the op's actor bind and the record
                    // parks until the turn's terminal frame
                    // (`CutsceneTimeline::player_face`). `jouine` `P2[5]`
                    // turns Vahn toward Cort this way before the evolved-Cort
                    // fight; stepped as a plain halt on the record's own
                    // context, the player kept facing the camera.
                    // REF: FUN_801DE840 (0x801E2148..0x801E21DC)
                    if let Some(mut ramp) =
                        crate::inline_dialogue::TalkFaceRamp::from_acquire(&tl.bytecode, pc)
                    {
                        if pc < tl.visited.len() {
                            tl.visited[pc] = true;
                        }
                        let resume_pc = pc + 6;
                        if host.world.step_player_face_leg(&mut ramp) {
                            tl.pc = resume_pc;
                            continue;
                        }
                        tl.player_face = Some((ramp, resume_pc, 1));
                        break;
                    }
                    // Player seats (`A3 F8 x z` MOVE_TO, `CC F8 51 x z ..`
                    // NPC-run): `FUN_8003C83C` resolves `0xF8` to the player
                    // object, so the op runs with the PLAYER as its context
                    // and takes the player arm (`0x801DEC7C` compares the
                    // context pointer against `_DAT_8007C364`). Stepping
                    // them on the record's own context instead dropped the
                    // seat: `urudre1` `P2[1]` walks the player to (97,8) for
                    // the shot and closes with `A3 F8 60 0D`, and without it
                    // free roam resumed on a tile no direction leaves.
                    // REF: FUN_8003C83C, FUN_801DE840 (0x23 / 4C 51 arms)
                    if op == 0x23
                        || (op == 0x4C
                            && matches!(tl.bytecode.get(pc + 2), Some(&0x51) | Some(&0xE3)))
                    {
                        let mut player_ctx = legaia_engine_vm::field::FieldCtx {
                            script_id: 0xF8,
                            flags: 0x0100_0000,
                            ..Default::default()
                        };
                        let r = vm::field::step(&mut host, &mut player_ctx, &tl.bytecode, pc);
                        if let FieldStepResult::Advance { next_pc } = r {
                            if let Some(slot) = host.world.player_actor_slot
                                && let Some(actor) = host.world.actors.get(slot as usize)
                            {
                                let (x, z) = (actor.move_state.world_x, actor.move_state.world_z);
                                let y = host
                                    .world
                                    .sample_field_floor_height(i32::from(x), i32::from(z))
                                    as i16;
                                if let Some(a) = host.world.actors.get_mut(slot as usize) {
                                    a.move_state.world_y = y;
                                }
                            }
                            if pc < tl.visited.len() {
                                tl.visited[pc] = true;
                            }
                            tl.pc = next_pc;
                            continue;
                        }
                    }
                    if op == 0x22
                        && let Some(&move_id) = tl.bytecode.get(pc + 2)
                    {
                        host.world
                            .pending_field_events
                            .push(FieldEvent::ExecMove { move_id });
                        // Retail's player arm of op 0x22: the move id becomes
                        // the clip base and is picked + bound at once.
                        let pick = host.world.field_player_script_clip(move_id);
                        // The end latch a following `AD F8 08` waits on lands
                        // when a scene-bank clip has played its frames; a
                        // party-bank clip (the locomotion loops) is not timed.
                        tl.player_clip_ticks = match pick.bound() {
                            Some((vm::field_player_clip::ClipBank::Scene, record)) => host
                                .world
                                .locomotion
                                .scene_clip_ticks
                                .get(usize::from(record))
                                .copied()
                                .unwrap_or(0),
                            _ => 0,
                        };
                        // Cue the scripted player clip: the windowed host
                        // resolves scene-ANM record `move_id - 1` and plays
                        // it once over idle/walk (live-pinned: the town01
                        // post-naming `A2 F8 30`/`31` land the retail anim
                        // pointer on scene records 47/48 for one playthrough
                        // each). Only a pick that binds the scene bank plays
                        // a scene record: with the party-bank bit up the id
                        // strides into the leader's own bank, which the
                        // settle's slot pick already plays.
                        if let Some(id) = host.world.player_move_cue(&pick) {
                            host.world.locomotion.player_move_cues.push(id);
                        }
                        tl.player_move_frames = CHANNEL_WAIT_PARK_TIMEOUT;
                        if pc < tl.visited.len() {
                            tl.visited[pc] = true;
                        }
                        tl.pc = pc + 3;
                        continue;
                    }
                    // Compass walk on the player (`B7 F8 b0 b1` / `C1 F8
                    // b0 b1`): arm the leg and run on; the walk kernel plays
                    // it while the record's next op on the player waits.
                    //
                    // Modal timelines and concurrent helpers alike: both run
                    // under the player's engaged bit, so the pad is refused
                    // while the script walks the player
                    // (`World::script_context_engages_player`; `korout`'s
                    // first-visit walk is the helper case).
                    // REF: FUN_8003774C (the 0x37 / 0x41 arm)
                    if matches!(op, 0x37 | 0x41)
                        && let (Some(&body0), Some(&body1)) =
                            (tl.bytecode.get(pc + 2), tl.bytecode.get(pc + 3))
                    {
                        if pc < tl.visited.len() {
                            tl.visited[pc] = true;
                        }
                        let mut glide = crate::cutscene_timeline::TimelinePlayerGlide {
                            state: vm::motion_vm::MotionState {
                                speed: 1,
                                ..Default::default()
                            },
                            body0,
                            body1,
                            rate: if op == 0x37 { 0x80 } else { 0x40 },
                            resume_pc: pc + 4,
                            frames: 0,
                        };
                        if host.world.step_player_glide(&mut glide) {
                            tl.player_glide = Some(glide);
                        }
                        glide_hold = true;
                        tl.pc = pc + 4;
                        continue;
                    }
                    // End-latch spin on the player (`AD F8 08`): park while
                    // the poked scene-bank clip is still playing.
                    if op == 0x2D && tl.bytecode.get(pc + 2) == Some(&8) && tl.player_clip_ticks > 0
                    {
                        if pc < tl.visited.len() {
                            tl.visited[pc] = true;
                        }
                        tl.player_clip_wait = Some(3);
                        break;
                    }
                    if op == 0x43
                        && let Some(&sub) = tl.bytecode.get(pc + 2)
                        && matches!(sub, 0 | 1 | 0xA | 0xB)
                    {
                        // Encoded width: extended header (2) + sub-0/1
                        // operand (7) or sub-A/B operand (9) - the VM's own
                        // stride (`overlay_0897` `0x801DF5B8` `addiu s8,s8,8`
                        // plus the `sub >= 0xA` `+2` at `0x801DF534`, over an
                        // `s8` the prologue already advanced past the extended
                        // channel byte).
                        let width = if sub == 0xA || sub == 0xB { 11 } else { 9 };
                        if pc < tl.visited.len() {
                            tl.visited[pc] = true;
                        }
                        // The halt is the arc's: retail arcs the player
                        // (`FUN_801D25EC`, `0x801DF5AC`) and its watcher
                        // releases the halted caller on landing
                        // (`FUN_801D5D60`), so the park lasts exactly the
                        // clip. The move countdown is the fallback only when
                        // no arc could start.
                        // REF: FUN_801d25ec
                        if let Some(req) = tl
                            .bytecode
                            .get(pc + 2..)
                            .and_then(vm::field_ledge_hop_arc::ScriptArcRequest::decode)
                            && host.world.start_field_script_arc(
                                crate::world::ScriptActorRef::Player,
                                &req,
                                None,
                            )
                        {
                            tl.player_move_frames = 0;
                            tl.player_wait = Some(width);
                            break;
                        }
                        if tl.player_move_frames == 0 {
                            tl.pc = pc + width;
                            continue;
                        }
                        tl.player_wait = Some(width);
                        break;
                    }
                }
                let result = if let Some((_, ci)) = target {
                    // Object-bind channels are poke targets, but their
                    // `placement_index` is a flat record index - never
                    // attribute placement-keyed side effects (anim cues,
                    // seat write-throughs) to them.
                    host.world.field_vm.executing_channel =
                        (!channels[ci].object_bind).then_some(channels[ci].placement_index as u8);
                    host.world.field_vm.executing_object = channels[ci]
                        .object_bind
                        .then_some(channels[ci].ctx.script_id);
                    // The timeline is the acquirer: it halt-acquired these
                    // channels earlier (the `4C 85` freeze sweep) and now
                    // drives them beat by beat. A poke from the owner is the
                    // resume signal, so clear the target's halt bit before the
                    // op runs - otherwise the dispatcher prelude parks the
                    // caller on its own frozen actor and the camera beats
                    // after the sweep never play.
                    channels[ci].ctx.flags &= !0x400;
                    let before = (channels[ci].ctx.world_x, channels[ci].ctx.world_z);
                    let r = vm::field::step_with_caller(
                        &mut host,
                        &mut channels[ci].ctx,
                        &mut tl.ctx,
                        false,
                        &tl.bytecode,
                        pc,
                    );
                    host.world.field_vm.executing_channel = None;
                    host.world.field_vm.executing_object = None;
                    // A poke that moved the actor (`A3 <id>` seat, `CC <id> 37`
                    // copy-from-player, ...) lands on retail's `+0x14`/`+0x18`
                    // at once; surface it now rather than at the slice's end,
                    // so a walk later in the same slice starts from it.
                    let c = &channels[ci];
                    let after = (c.ctx.world_x, c.ctx.world_z);
                    if !c.object_bind
                        && after != before
                        && let Ok(slot) = u8::try_from(c.placement_index)
                    {
                        host.world
                            .npcs
                            .positions
                            .insert(slot, (after.0 as i16, after.1 as i16));
                        host.world.npcs.motions.remove(&slot);
                    }
                    r
                } else {
                    field_step_routed(&mut host, &mut tl.ctx, &tl.bytecode, pc)
                };
                let (mut next_pc, kind, mut stop) = match result {
                    FieldStepResult::Advance { next_pc } => (
                        next_pc,
                        crate::cutscene_timeline::TraceResult::Advance,
                        false,
                    ),
                    FieldStepResult::Yield { resume_pc } => (
                        resume_pc,
                        crate::cutscene_timeline::TraceResult::Yield,
                        true,
                    ),
                    // WAIT_FRAMES and conditional holds return `Halt` at the
                    // same PC: end the frame and resume there next tick.
                    FieldStepResult::Halt { final_pc } => {
                        (final_pc, crate::cutscene_timeline::TraceResult::Halt, true)
                    }
                    // An op this port can't advance past: stop and let the
                    // safety net below arm the hand-off.
                    FieldStepResult::Pending { pc, .. } => {
                        (pc, crate::cutscene_timeline::TraceResult::Pending, true)
                    }
                    FieldStepResult::Unknown { pc, .. } => {
                        (pc, crate::cutscene_timeline::TraceResult::Unknown, true)
                    }
                };
                // Step past the timeline's conditional-wait parks that are NOT
                // the modelled channel handshake. Retail Halts at PC on these -
                // a flag a spawned sub-context sets - so advancing by the op's
                // encoded width (these flag-tests read one operand byte,
                // `header_size + 1`) keeps the timeline flowing toward its
                // camera / move / STATE_RESUME ops. The step-past ops are the
                // flag-tests `0x2D` (LFLAG), `0x30` (GFLAG) and the `0x4C`
                // nibble-C `script_alloc` / globals-gate - all 2-byte (3
                // extended), so a fixed step-past is correct-width for them.
                // The cross-context CFLAG_TST `0x33` (`B3 <id> <bit>` = the
                // timeline waiting on a vignette channel's completion flag) is
                // now PARKED instead (handled just above): it holds the PC until
                // the channel raises the bit - the halt-acquire / state-resume
                // handshake - and only the `B3 <id> 0A` halt-bit *verify* form
                // (bit 10) still steps past here. A bare (non-cross-context)
                // `0x33` also steps past. Other cross-context ops (the `4C`/`23`
                // action pokes) are NOT stepped past - they run against the
                // target and advance by their real width. Two parks are kept:
                // `0x4A` WAIT_FRAMES (a real timed wait that plays out via the
                // wait accumulator) and `0x49` STATE_RESUME (the name-entry
                // suspend, driven by the op-49 host hooks).
                let op = opcode_byte & 0x7F;
                // Cross-context `4C A0` busy-wait (`CC <ch> A0 <bit> <s16>`):
                // "while the poked channel's ctx-flag bit is still set, jump".
                // Retail's channel clears its own busy bit as its move plays
                // out frame by frame; the timeline's channel pokes complete
                // synchronously, so the busy branch must always fall through -
                // and the s16 target is meaningless in the caller record's pc
                // space (taking it here derailed the Mei beat into its own
                // header + dialog text). Force the skip path (6-byte width).
                if op == 0x4C
                    && target.is_some()
                    && tl.bytecode.get(pc + 2).is_some_and(|b| b >> 4 == 0xA)
                {
                    next_pc = pc + 2 + 4;
                    stop = false;
                }
                // Cross-context channel wait (`B3 <id> <bit>`, CFLAG_TST against
                // a spawned channel): PARK the timeline while the awaited
                // channel's bit is set (`0x801DEE44`), rather than
                // stepping past by width. The park persists across ticks and is
                // resolved by the pre-step gate above. Bit 10 (0x400, the
                // halt/busy bit the acquire sweep toggles) is a suspension
                // *verify*, not a completion wait, so it falls through to the
                // width step-past below.
                if op == 0x33
                    && matches!(kind, crate::cutscene_timeline::TraceResult::Halt)
                    && next_pc == pc
                    && let Some((tid, _)) = target
                {
                    let bit = tl.bytecode.get(pc + 2).copied().unwrap_or(0) & 0x1F;
                    if bit != 10 {
                        tl.channel_wait = Some(crate::cutscene_timeline::ChannelWait {
                            target_id: tid,
                            bit,
                            frames: 0,
                        });
                        // Leave PC on the op; the pre-step gate resolves the park.
                        break;
                    }
                }
                // `4C CD` halts only while the camera mover's glide is in
                // flight (the VM advances it otherwise), so its park is a
                // timed wait to hold, not a handshake to step past.
                let glide_wait = op == 0x4C && tl.bytecode.get(pc + 1) == Some(&0xCD);
                // NPC end-latch spin (`AD <id> 08`): the record re-tests the
                // poked actor's `+0x62 & 0x100` every frame until its clip
                // tick latches it - the clip's remaining length for a
                // clamped clip, the time to the next wrap for a looping one.
                // Held only where the world owns that actor's clip cursor
                // (the `0x22` poke binds one); a held clip, which never
                // latches, falls back to the step-past after
                // [`NPC_CLIP_SPIN_TIMEOUT`] frames.
                // REF: FUN_800204F8 (0x800206E4..0x8002072C), FUN_801DE840 (op 0x2D)
                let npc_clip_spin = op == 0x2D
                    && opcode_byte & 0x80 != 0
                    && tl.bytecode.get(pc + 2) == Some(&8)
                    && matches!(kind, crate::cutscene_timeline::TraceResult::Halt)
                    && next_pc == pc
                    && target.is_some_and(|(_, ci)| {
                        !channels[ci].object_bind
                            && host
                                .world
                                .npc_clip_cursor_bound(channels[ci].placement_index as u8)
                    });
                if npc_clip_spin && tl.npc_clip_spin_frames < NPC_CLIP_SPIN_TIMEOUT {
                    tl.npc_clip_spin_frames += 1;
                    tl.frames = tl.frames.saturating_sub(1);
                    break;
                }
                tl.npc_clip_spin_frames = 0;
                let is_flag_test_handshake = matches!(op, 0x2D | 0x30 | 0x33)
                    || (op == 0x4C && target.is_none() && !glide_wait);
                if matches!(kind, crate::cutscene_timeline::TraceResult::Halt)
                    && next_pc == pc
                    && op != 0x4A
                    && op != 0x49
                    && !glide_wait
                    && (target.is_none() || is_flag_test_handshake)
                {
                    // By the op's own width: the flag tests are two bytes
                    // (three extended), but a `0x4C` park is not always -
                    // `4C D2 <ch>` is three, and stepping it by two read its
                    // channel byte as the next opcode (`rayman` `P2[19]`'s
                    // `4C D2 53 .. 4C D2 59` run then lost the `44 7A` that
                    // re-seats the village after the quake).
                    let header_size = if opcode_byte & 0x80 != 0 { 2 } else { 1 };
                    next_pc = legaia_asset::field_disasm::decode(&tl.bytecode, pc)
                        .map_or(pc + header_size + 1, |insn| pc + insn.size);
                    stop = false;
                }
                // Natural termination: the record's choreography **wrapped**.
                // On-disc partition-2 records have no end opcode - they finish
                // by parking in a tight `Nop`+`JmpRel`-to-self spin (the fog /
                // flag-reset ambients) or by looping back to their top as a
                // resident actor-driver (the Mei beat's op-`0x45` APPLY jump
                // back to its conversation loop). Retail leaves both spinning
                // as *parallel* contexts, invisible to the player; the modal
                // timeline completes instead so control returns. The signal is
                // an `Advance` jumping backward onto an already-executed PC -
                // real waits `Halt` at their own PC and never trip this.
                if pc < tl.visited.len() {
                    tl.visited[pc] = true;
                }
                // One backward jump is NOT a wrap: a loop that polls the held
                // pad (`42 01 <button>`) is the record waiting for the player.
                // `edlast`'s ending record closes that way - `4A 08 00` then
                // `42 01 08` / `42 01 09` (Circle / Cross held) and a `26`
                // back to the wait - and retail sits in it until the press;
                // reading it as a wrap dropped the record and handed the
                // player the pad in the ending's last scene.
                if matches!(kind, crate::cutscene_timeline::TraceResult::Advance)
                    && next_pc <= pc
                    && tl.visited.get(next_pc).copied().unwrap_or(false)
                    && !loop_polls_held_pad(&tl.bytecode, next_pc, pc)
                {
                    // There used to be a carve-out here for a `45 C0 <s16>`
                    // "camera-apply loop-back", on the reading that retail's
                    // sub-`0xC0` arm jumps to the operand `s16`. It does not:
                    // the arm is a four-byte fall-through and the `s16` is the
                    // apply trigger (`docs/subsystems/script-vm.md`,
                    // "0x45 CAMERA arm widths"). With the VM's arm corrected
                    // the op can no longer produce a backward `Advance` at all,
                    // so the carve-out was rescuing a loop the port invented.
                    // REF: FUN_801dab90
                    tl.done = true;
                    stop = true;
                }
                // A touch-resumed placement context ends its interaction at
                // the first raw `0x21` it executes (`FUN_80039B7C`: the loop
                // exits on `0x21` at `0x80039E20` and `0x80039E68..0x80039E7C`
                // clears the engaged bit). The Rim Elm bee beat (`town0c` /
                // `town0b` `P1[21]`) is `50 00` (the scripted-loss latch),
                // `3E FF 03`, `21`, then a jump back to its flag dispatch: the
                // `21` is what stops the fight re-firing until the next touch.
                if tl.interaction_slot.is_some()
                    && opcode_byte == 0x21
                    && matches!(kind, crate::cutscene_timeline::TraceResult::Advance)
                {
                    tl.done = true;
                    stop = true;
                }
                if tl.trace_enabled {
                    if std::env::var_os("LEGAIA_DIAG_TIMELINE").is_some()
                        && !(matches!(kind, crate::cutscene_timeline::TraceResult::Halt)
                            && next_pc == pc)
                    {
                        eprintln!(
                            "DIAG timeline: frame {} pc {pc:#06x} op {opcode_byte:#04x} \
                             ({:#04x}) -> {next_pc:#06x} {kind:?} bytes {:02x?}",
                            tl.frames,
                            opcode_byte & 0x7F,
                            &tl.bytecode[pc..(pc + 12).min(tl.bytecode.len())]
                        );
                    }
                    tl.trace.push(crate::cutscene_timeline::TraceEntry {
                        pc,
                        opcode_byte,
                        opcode: opcode_byte & 0x7F,
                        next_pc,
                        result: kind,
                    });
                }
                tl.pc = next_pc;
                if matches!(
                    kind,
                    crate::cutscene_timeline::TraceResult::Pending
                        | crate::cutscene_timeline::TraceResult::Unknown
                ) {
                    tl.done = true;
                }
                if stop {
                    // An authored `0x4A WAIT_FRAMES` hold is real playout, not
                    // a hang - the same carve-out the walk / rotate / narration
                    // parks above take. The op is bounded by construction
                    // (`ctx.wait_accum` grows by `frame_delta` every tick until
                    // it reaches the operand, then the op advances), so it
                    // cannot spin, and a record that spends its time in one is
                    // playing, not stuck. Counting those ticks made the
                    // anti-hang cap a *record-length* cap instead: `urudre3`
                    // P2[0] and `jouine` P2[16] need ~5400 and ~4900 stepping
                    // frames to reach their exits and were cut at 1200, so the
                    // forced completion dropped both records before their tail
                    // and those rooms read as one-way.
                    // A `4C CD` camera-glide wait is the same kind of hold:
                    // bounded by the glide's own frame count, which the
                    // mover spends one display frame at a time.
                    if ((opcode_byte & 0x7F) == 0x4A || glide_wait)
                        && matches!(kind, crate::cutscene_timeline::TraceResult::Halt)
                        && next_pc == pc
                    {
                        tl.frames = tl.frames.saturating_sub(1);
                    }
                    // Same carve-out for an op-`0x43` halt-acquire park
                    // (sub-0/1/A/B). Retail's arm raises the context's halt bit
                    // and hands the actor its walk target
                    // (`FUN_801D25EC`); the context then sits out however many
                    // frames the actor's leg takes. That is authored playout,
                    // and it cannot spin here - the port's arm advances the PC
                    // past the op, so the parks a record can spend are bounded
                    // by its own instruction count. Counting them turned the
                    // anti-hang cap back into a length cap on exactly the
                    // record the cap's own note names: `jouine` `P2[16]`
                    // stopped three bytes short of its `4C E2 08` FMV tail.
                    if (opcode_byte & 0x7F) == 0x43
                        && matches!(kind, crate::cutscene_timeline::TraceResult::Yield)
                        && tl
                            .bytecode
                            .get(pc + if opcode_byte & 0x80 != 0 { 2 } else { 1 })
                            .is_some_and(|s| matches!(s, 0 | 1 | 0xA | 0xB))
                    {
                        tl.frames = tl.frames.saturating_sub(1);
                    }
                    break;
                }
            }
        }
        // Timeline pokes that moved a channel context (cross-context MoveTo)
        // write through to the field NPC render/probe state. Object-bind
        // channels never write through: their `placement_index` is a FLAT
        // record index, not a placement slot, and the NPC surfaces are
        // placement-keyed.
        for (c, pre) in channels.iter().zip(channel_pre_pos) {
            if !c.object_bind && (c.ctx.world_x, c.ctx.world_z) != pre {
                self.npcs.positions.insert(
                    c.placement_index as u8,
                    (c.ctx.world_x as i16, c.ctx.world_z as i16),
                );
            }
        }
        self.field_vm.channels = channels;
        self.field_vm.stepping_view.clear();
        self.cutscene.in_timeline = false;
        self.field_vm.in_spawned_record_slice = false;
        true
    }
}
