//! Placed-prop touch / interact interactions: the door swing, the searchable
//! cupboard, and their collision-side effects.
//!
//! A placed field object's bind record is its field-VM script, and retail
//! runs it on the touched actor through the per-actor dialog SM
//! (`FUN_80039B7C` state 0 loops the dispatcher `FUN_801DE840` on
//! `actor[+0x90]`/`+0x9E`). The engine mirrors that by running the record
//! through the inline field-VM runner ([`legaia_engine_field::inline_dialogue`]) with a
//! **prop binding**: the executing context's `local_flags`/`flags`/`field_6a`
//! are the prop actor's `+0x62` / `+0x10` / `+0x6A`, synced into the
//! [`legaia_engine_scene::field_env::PropAnimBank`] entry around every VM slice - so
//!
//! - the record's `2B`/`2C` LFlag ops drive the clip (the swing),
//! - its `2D 08` end-latch spin **parks** the run until the per-frame anim
//!   tick latches the clip's end,
//! - its `31 00` CFLAG_SET drops the prop from the collision candidate list
//!   ([`FieldPropCollider::solid`] - retail `FUN_801CF754`'s `flags & 3`
//!   filter), which is how an opened door stops blocking,
//! - its `0x1F` dialog segments open the real dialog panel (the cupboard's
//!   "There's a ... in the cupboard!"), its `39` GIVE_ITEM grants through the
//!   host, and its `50 xx` SysFlag.Set latches the searched-once flag,
//! - the trailing `21` ends the interaction, re-parking the record where the
//!   next touch resumes (the cupboard's post-close backward jump replays the
//!   pass, whose `70 xx` guard then selects the "empty!" arm).
//!
//! Two dispatch classes start a run (the retail `FUN_801CFC40` result-bit
//! split on the actor's `+0x10`):
//!
//! - **auto-touch** (bit `4`, doors): the movement probe both refuses the
//!   step and posts the touch - [`crate::world::FieldPropState::pending_touch`] set by
//!   [`World::advance_with_collision`];
//! - **interact-gated** (bit `1`, `+0x10 & 0x40020000`, cupboards): only the
//!   just-pressed-confirm facing probe posts it
//!   ([`World::field_interact_prop_anchor`], wired into
//!   [`World::tick_field_interaction_probe`]).
//!
//! While a run is live the player's movement-disabled flag (`+0x10 &
//! 0x80000`) is held, exactly as `FUN_801D5B5C` raises it and the dialog SM
//! teardown clears it - the player stands until the door finishes opening or
//! the message is dismissed.
//!
//! REF: FUN_801D5B5C, FUN_80039B7C, FUN_801DE840, FUN_801CFC40, FUN_801CF754

use crate::world::narration::post_page_byte;
use legaia_engine_field::inline_dialogue::HeldBox;

use super::*;

/// Frames a prop run may stay parked on a waitable op (`2D 08`, `4A`, ...)
/// before it is force-completed - a safety net against a decode drift
/// soft-locking the player (the engaged flag suppresses locomotion while a
/// run is live). Generous: the longest retail door swing is under 40 frames.
const PROP_RUN_PARK_TIMEOUT: u32 = 1800;

impl World {
    /// One frame of the per-actor anim tick over every placed prop and MAN
    /// actor. It runs unconditionally (`FUN_800204F8` from the actor tick) -
    /// the windmill turns during dialogs too - and on the overworld as in a
    /// town: a kingdom map is a field-run scene whose actors play the
    /// bundle's slot-4 clips through the same tick
    /// (`docs/formats/world-map-overlay.md`).
    ///
    /// REF: FUN_800204F8
    pub fn tick_actor_anims(&mut self) {
        // The clip cursors move on the game-frame grid: the actor tick runs
        // once a game frame and multiplies each step by the frame step
        // (`FUN_800204F8`, `mult a0,v0` at `0x80020660`), so a vsync between
        // two game frames moves none of them.
        let grid = self.clock.script_frame_delta();
        if grid > 0 {
            self.props.bank.tick_anims_scaled(grid);
        }
        // The same tick over the NPC actors: the morph envelope first
        // (`FUN_800204F8` runs `FUN_80020740` before its cursor step), then
        // the clip cursors (a treasure chest's lid, every NPC's idle loop).
        self.tick_npc_morphs();
        if grid > 0 {
            self.tick_npc_clips_scaled(grid);
        }
        // The height arm of the same actor tick over the placed objects: a
        // `4C 42` tween's `+0x8E` step and the `0x20000000` Y pin.
        self.tick_object_actor_heights();
    }

    /// The prop object `key`'s actor draws: the bank entry at the bind's key
    /// tile ([`Self::object_key_anchor`]).
    fn object_prop_mut(
        &mut self,
        key: u16,
    ) -> Option<&mut legaia_engine_scene::field_env::PropAnimState> {
        let anchor = self.object_key_anchor(usize::from(key))?;
        self.props.bank.props.get_mut(&anchor)
    }

    /// A cross-context `A2 <record> <clip>` ExecMove landing on a placed
    /// prop's actor: bind `clip` to the prop object `key`'s actor draws
    /// (a key of [`legaia_engine_field::field_channels::object_channel_keys`]),
    /// cursor at zero (the anim tick's forced re-point), under the actor's
    /// live `+0x62` control word (`flags`) and `+0x6A` rate (`rate`, kept
    /// when zero). The windowed and browser hosts pose posed props from the
    /// bank, so both draw the clip.
    ///
    /// REF: FUN_800204F8 (the re-point at `0x8002057C..0x800205A8`)
    pub(crate) fn play_object_prop_clip(&mut self, key: u16, clip: u8, flags: u16, rate: i16) {
        let meta = self.props.bank.clip_meta(clip);
        if let Some(p) = self.object_prop_mut(key) {
            if p.anim.anim_id != clip {
                let Some((frames, scaled, div)) = meta else {
                    return;
                };
                p.anim.anim_id = clip;
                p.anim.frames = frames.max(1);
                p.anim.scaled_step = scaled;
                p.anim.step_div = div;
            }
            p.anim.cursor = 0;
            p.anim.flags = flags;
            if rate != 0 {
                p.anim.rate = rate;
            }
        }
    }

    /// Write a placed object's clip-control word (`+0x62`) - a script's
    /// `LFLAG_SET` / `LFLAG_CLR` or `4C 35` / `4C 36` landing on the object's
    /// actor. The anim tick reads the word every frame (a restart request
    /// snaps the cursor, hold freezes it), so the clip takes the bits at once
    /// without a clip poke.
    ///
    /// REF: FUN_800204F8
    pub(crate) fn set_object_prop_flags(&mut self, key: u16, flags: u16) {
        if let Some(p) = self.object_prop_mut(key) {
            p.anim.flags = flags;
        }
    }

    /// A cross-context op from a spawned record changed a placed object's
    /// clip words - the control word `+0x62` (`AB` / `AC <id> <bit>`,
    /// `CC <id> 35` / `36`) or the cursor step `+0x6A` (`CC <id> 41`) - with
    /// no `A2` after it. They are the words the object's anim tick
    /// `FUN_800204F8` reads every frame, so the clip it already has bound
    /// takes them at once: `town01`'s opening swings Vahn's front door open
    /// with `CC 00 41 10`, `AC 00 07`, `AC 00 01`, `AB 00 03` alone (P2[3]
    /// `+0xFF`: step 8, forward, release the spawn hold, clamp at the end).
    ///
    /// Only the bits the op changed are written (`before` / `after` are the
    /// object channel's context words either side of the op), so the prop
    /// keeps whatever its spawn program left in the others. A zero step is
    /// not taken, as [`Self::play_object_prop_clip`] does not take one.
    ///
    /// REF: FUN_800204F8, FUN_801DE840
    pub(crate) fn poke_object_prop_words(
        &mut self,
        key: usize,
        flags: (u16, u16),
        rate: (i16, i16),
    ) {
        let changed = flags.0 ^ flags.1;
        let new_rate = (rate.0 != rate.1 && rate.1 != 0).then_some(rate.1);
        if changed == 0 && new_rate.is_none() {
            return;
        }
        if let Some(p) = self.object_prop_mut(key as u16) {
            p.anim.flags = (p.anim.flags & !changed) | (flags.1 & changed);
            if let Some(r) = new_rate {
                p.anim.rate = r;
            }
        }
    }

    /// Carry the object's clip end latch onto its channel's copy of the
    /// control word before a script op runs on that channel. The clip tick
    /// `FUN_800204F8` latches `+0x62 & 0x100` on the actor's one word; the
    /// port's tick writes the prop's copy and the context copy never saw it,
    /// so `AC <id> 08` found nothing to clear and the prop's latch survived.
    /// `edbylon`'s second door swing clears the latch left by the first and
    /// spins on it, and the spin then passed at once.
    ///
    /// REF: FUN_800204F8 (0x800206E4..0x8002072C), FUN_801DE840 (ops 0x2B / 0x2C)
    pub(crate) fn mirror_object_end_latch(&self, key: usize, word: &mut u16) {
        use legaia_engine_scene::field_env::ANIM_END;
        let Some(anchor) = self.object_key_anchor(key) else {
            return;
        };
        if let Some(p) = self.props.bank.props.get(&anchor) {
            *word = (*word & !ANIM_END) | (p.anim.flags & ANIM_END);
        }
    }

    /// `true` while placed object `record`'s prop plays a clip that has not
    /// latched its end (`+0x62 & 0x100`): not held (`0x2`), end latch down.
    /// What a record's `AD <id> 08` spin on a door's swing waits for.
    // REF: FUN_800204F8 (0x800206E4..0x8002072C)
    pub fn object_prop_clip_running(&self, record: u16) -> bool {
        use legaia_engine_scene::field_env::{ANIM_END, ANIM_HOLD};
        self.props
            .bank
            .props
            .values()
            .any(|p| p.record == usize::from(record) && p.anim.flags & (ANIM_END | ANIM_HOLD) == 0)
    }

    /// Capture alignment: write a placed object's clip cursor and control
    /// word over its prop's own, where the prop has that clip bound. Which
    /// frame of a door's swing is up is touch history a seat does not
    /// replay; a prop on another clip is left alone, so the seed never
    /// re-binds what the engine's own spawn or script chose. The retail
    /// comparison's image child only.
    pub fn seed_object_prop_clip(
        &mut self,
        record: usize,
        clip: u8,
        cursor: i16,
        flags: u16,
        rate: i16,
    ) {
        for p in self
            .props
            .bank
            .props
            .values_mut()
            .filter(|p| p.record == record && p.anim.anim_id == clip)
        {
            p.anim.cursor = cursor;
            p.anim.flags = flags;
            if rate != 0 {
                p.anim.rate = rate;
            }
        }
        // A placement's actor (an NPC, a party member a cutscene poses)
        // keeps its clip in the world's per-slot cursor, which the record's
        // `A2 <id> <clip>` binds; the record's `+0x50` is the channel's
        // script id, as a cross-context target byte resolves it.
        let slot = self
            .field_vm
            .channels
            .iter()
            .find(|c| !c.object_bind && usize::from(c.ctx.script_id) == record)
            .and_then(|c| u8::try_from(c.placement_index).ok());
        if let Some(c) = slot
            .and_then(|s| self.npcs.clip_cursors.get_mut(&s))
            .filter(|c| c.anim_id == clip)
        {
            c.cursor = cursor;
            c.flags = flags;
            if rate != 0 {
                c.rate = rate;
            }
        }
    }

    /// Advance the placed-prop layer one field tick: step the clips, step an
    /// in-flight prop record run, and start a run for a movement touch posted
    /// by this tick's locomotion.
    pub fn tick_prop_interactions(&mut self) {
        self.sync_prop_colliders_to_object_actors();
        self.tick_actor_anims();
        self.step_prop_interaction();
        if let Some(anchor) = self.props.pending_touch.take() {
            self.start_prop_interaction(anchor);
        }
    }

    /// Start running prop `anchor`'s bind record from its parked cursor - the
    /// engine's `FUN_801D5B5C` touch post. No-op when another interaction or
    /// dialog owns the frame, when the prop has no bank entry, or when it has
    /// gone collision-exempt (retail's probes skip `flags & 3` actors, so an
    /// opened door can never re-fire).
    ///
    /// PORT: FUN_801D5B5C (the touch-event post + engaged-flag raise)
    pub fn start_prop_interaction(&mut self, anchor: (u8, u8)) -> bool {
        if self.dialogue_owns_input()
            || self.script_context_engages_player()
            || self.field_scripts_held_for_battle()
        {
            return false;
        }
        let Some(prop) = self.props.bank.props.get(&anchor) else {
            return false;
        };
        if prop.collision_exempt() {
            return false;
        }
        let mut runner = legaia_engine_field::inline_dialogue::InlineDialogue::new(
            std::sync::Arc::clone(&prop.record_body),
            prop.parked_pc,
        );
        runner.prop_anchor = Some(anchor);
        // The executing context IS the prop actor: seed its `+0x62` anim
        // word, `+0x10` flag word and `+0x6A` rate from the live prop.
        runner.ctx.local_flags = prop.anim.flags;
        runner.ctx.flags = prop.cflags;
        runner.ctx.field_6a = prop.anim.rate;
        self.dialog.inline = Some(runner);
        // Engaged: locomotion input is suppressed until the run completes
        // (retail: `FUN_801D5B5C` raises `player+0x10 |= 0x80000`, the dialog
        // SM teardown clears it).
        self.set_player_engaged(true);
        true
    }

    /// The interact-gated prop (cupboard class) whose contact box contains the
    /// facing compass point 64 units ahead of the player, if any - the prop
    /// arm of retail's button-press probe (`FUN_801CF9F4` over the same actor
    /// list the NPC arm walks). Solid interact-class props only: an exempt
    /// (`flags & 3`) actor is skipped, an auto-touch prop fires from movement
    /// contact instead.
    ///
    /// Box classes mirror the collision arm: static ±80 around the footprint
    /// centre (the probe's moving-arm extents widen only moving-box props:
    /// ±(0x40 + 0x20 − 0x18) = 72 around the live position).
    ///
    /// PORT: FUN_801CF9F4
    pub fn field_interact_prop_anchor(&self) -> Option<(u8, u8)> {
        let slot = self.player_actor_slot? as usize;
        let actor = self.actors.get(slot).filter(|a| a.active)?;
        let ms = &actor.move_state;
        let sector = (((ms.render_26 as i32 + 0x800) & 0xfff) >> 9) as usize;
        let (dx, dz) = FIELD_FACING_PROBES[sector];
        let px = ms.world_x.saturating_add(dx) as i32;
        let pz = ms.world_z.saturating_sub(dz) as i32;
        let mut best: Option<(i32, (u8, u8))> = None;
        for c in &self.props.colliders {
            if !c.solid || !c.interact || self.prop_collider_exempt(c) {
                continue;
            }
            let Some(anchor) = c.anchor else { continue };
            if !self.props.bank.props.contains_key(&anchor) {
                continue;
            }
            let ((cx, cz), half) = if c.moving_box {
                (c.live, FIELD_INTERACT_BOX_HALF)
            } else {
                (c.center, FIELD_PROP_BOX_HALF)
            };
            let (ex, ez) = ((px - cx).abs(), (pz - cz).abs());
            if ex < half && ez < half {
                let d = ex * ex + ez * ez;
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, anchor));
                }
            }
        }
        best.map(|(_, a)| a)
    }

    /// Step an in-flight prop record run one frame: route the dialog panel's
    /// input, or run a VM slice until the next waitable op / text segment /
    /// end. The retail counterpart is the dialog SM (`FUN_80039B7C`) driving
    /// the dispatcher on the touched actor each frame.
    pub(crate) fn step_prop_interaction(&mut self) {
        use legaia_engine_system::input::PadButton;
        let is_prop_run = self
            .dialog
            .inline
            .as_ref()
            .is_some_and(|id| id.prop_anchor.is_some());
        if !is_prop_run {
            return;
        }
        let confirm =
            self.input.just_pressed(PadButton::Cross) || self.input.just_pressed(PadButton::Circle);
        let up = self.input.just_pressed(PadButton::Up);
        let down = self.input.just_pressed(PadButton::Down);

        let Some(mut id) = self.dialog.inline.take() else {
            return;
        };
        let anchor = id.prop_anchor.expect("checked above");

        // A player compass leg the record armed plays every frame, box or no
        // box: the walk kernel steps it off the player's own `0x400`, not off
        // the record. A run whose record has already ended stays engaged
        // until the leg lands, then releases the player.
        // The leg spends on the game-frame grid, as a cutscene timeline's
        // does: nothing between game frames, the frame step's worth on one
        // (`speed = DAT_1F800393`). Stepped every vsync at speed `1`, the
        // player moved half a pass a vsync and the follow focus, which trails
        // the player a pass, trailed it half as far (`tower_field_walk`).
        // REF: FUN_8003774C (the 0x37 / 0x41 arm), FUN_801DE840 (0x801DEE90..0x801DEF1C)
        let grid = self.clock.script_frame_delta();
        if let Some(mut glide) = id.player_glide.take() {
            if grid == 0 {
                id.player_glide = Some(glide);
            } else {
                glide.state.speed = grid;
                if self.step_player_glide(&mut glide) {
                    id.player_glide = Some(glide);
                }
            }
        }
        if id.done {
            if id.player_glide.is_none() {
                self.finish_prop_interaction(&mut id, anchor);
            } else {
                self.dialog.inline = Some(id);
            }
            return;
        }

        // A box held for its press's own frame only is gone by the next pager
        // call, as in the talk runner (`HeldBox`).
        if id.held_box.as_ref().is_some_and(|h| !h.until_next_line) {
            id.held_box = None;
        }

        // A box is open: tick the typewriter + route input. The prop's clip
        // keeps ticking underneath (the cupboard holds open at its clamp).
        if let Some(panel) = id.panel.as_mut() {
            if panel.menu_active() {
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
            // Same one-frame rule as the inline runner: a menu that opened
            // on this frame's tick is shown first and commits on a later
            // confirm, so a mashed confirm never picks option 0 unseen.
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
                self.dialog.input_consumed = true;
                if panel.menu_active() && (!menu_was_open || !panel.picker_takes_input()) {
                    // Opened this frame, or still sliding in (the pager reads the
                    // choice only once the slide rests): nothing to commit yet.
                } else if panel.menu_active() {
                    self.push_sfx_cue(legaia_engine_dialog::dialog::PICKER_CONFIRM_CUE);
                    let choice = panel.picker_cursor();
                    let target = panel.picker().and_then(|pk| pk.jump_target(choice));
                    id.last_choice = Some(choice);
                    id.visited.iter_mut().for_each(|v| *v = false);
                    match target {
                        Some(t) => id.pc = t,
                        None => id.done = true,
                    }
                    let continuation = panel.picker().map(|pk| pk.continuation);
                    id.held_box = Some(HeldBox::after_press(panel, continuation));
                    id.panel = None;
                } else if panel.is_done() {
                    // Dismissed: resume the record just past the segment -
                    // which for the cupboard is `48`, a `26` to the shared
                    // tail and the close pass (`2B 07` set reverse, ...), so
                    // the doors swing shut as the window drops. Retail sequences it identically: the
                    // pager returns to SM state 0 and the script continues -
                    // unless the byte after the box parks the talk.
                    id.pc = panel.pc;
                    id.held_box = Some(HeldBox::after_press(panel, post_page_byte(panel)));
                    id.panel = None;
                    end_prop_talk_at_post_box_byte(&mut id);
                } else if panel.is_waiting_for_input() {
                    // Multi-page box: turn the page in the same window.
                    panel.advance_page();
                    if panel.is_done() {
                        id.pc = panel.pc;
                        id.held_box = Some(HeldBox::after_press(panel, post_page_byte(panel)));
                        id.panel = None;
                        end_prop_talk_at_post_box_byte(&mut id);
                    }
                } else {
                    // Still typing or scrolling: the pager's skip latch
                    // completes the page (`legaia_engine_dialog::dialog_window`).
                    panel.confirm_while_typing();
                }
            }
            if id.done && id.player_glide.is_none() {
                self.finish_prop_interaction(&mut id, anchor);
                return;
            }
            self.dialog.inline = Some(id);
            return;
        }

        // A player walk the record armed: the record stays parked on its
        // `C7 F8` until the player lands, then resumes past the op.
        // REF: FUN_801DE840 (0x801DF034..0x801DF044), FUN_8003774C (case 0x47)
        if let Some(mut walk) = id.player_walk.take() {
            walk.frames += 1;
            let arrived = self.step_player_walk_leg(walk.target, walk.speed, 1, walk.approach);
            if !arrived && walk.frames < WALK_PARK_TIMEOUT {
                id.player_walk = Some(walk);
                self.dialog.inline = Some(id);
                return;
            }
        }

        // The record's ops run on game frames only: the object's actor
        // steps its context from the actor-list walk
        // (`FrameClock::script_slice_due`). The box above and the player's
        // legs read the pad and spend on every vsync.
        // REF: FUN_8002519C, FUN_80039B7C
        if !self.clock.script_slice_due() {
            self.dialog.inline = Some(id);
            return;
        }

        // No box: sync the prop's live state into the context (the per-frame
        // anim tick may have latched the clip's end since the last slice),
        // then run a VM slice.
        if let Some(prop) = self.props.bank.props.get(&anchor) {
            id.ctx.local_flags = prop.anim.flags;
        }
        let mut parked = false;
        let own_record = self.object_key_at_anchor(anchor);
        // Where the record's own `23 <x> <z>` seated the prop this slice.
        let mut own_seat: Option<(i32, i32)> = None;
        {
            let mut host = FieldHostImpl { world: self };
            let mut budget = legaia_engine_field::inline_dialogue::INLINE_DIALOGUE_STEP_BUDGET;
            while budget > 0 {
                budget -= 1;
                let b = id.bytecode.get(id.pc).copied().unwrap_or(0);
                if b & 0x7F < 0x20 {
                    if b == 0x1F {
                        // A text segment: open the real dialog panel here,
                        // with the record's name escapes resolved.
                        let mut panel =
                            HeldBox::next_line_panel(id.held_box.take(), &id.bytecode, id.pc);
                        panel.substitutions = host.world.dialog_substitutions(&id.bytecode);
                        host.world.apply_dialog_window_format(&mut panel);
                        id.panel = Some(panel);
                        break;
                    }
                    // A stray terminator byte in the flow is consumed
                    // (retail's dispatcher skips `0x00..0x1E` bytes it lands
                    // on between segments).
                    id.pc += 1;
                    continue;
                }
                if id.pc < id.visited.len() {
                    id.visited[id.pc] = true;
                }
                // A `0x80`-prefix op aimed at the player (`0xF8`) runs with the
                // PLAYER as its context: `FUN_8003C83C` resolves `0xF8` to
                // `_DAT_8007C364` and the dispatcher applies the op to that
                // record's words. Stepping it on the prop's own context put
                // the player's halt-acquire on the prop - rayman's gondola
                // (P0[4]) walks the player aboard with `B7 F8 04 81`, a YIELD
                // whose halt bit (`+0x10 & 0x400`) then made the dispatcher's
                // halted-target early-out (`0x801DE90C..0x801DE940`) refuse
                // the prop's own next op, `AB 04 07`, for good.
                //
                // REF: FUN_8003C83C, FUN_801DE840 (the halted-target early-out)
                let player_target = b & 0x80 != 0
                    && vm::field::peek_extended(&id.bytecode, id.pc)
                        == Some(legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET);
                let mut player_ctx = legaia_engine_vm::field::FieldCtx {
                    script_id: u16::from(legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET),
                    flags: 0x0100_0000,
                    ..Default::default()
                };
                // The player's clip is the one cursor such an op reaches: an
                // `A2 F8 <clip>` ExecMove binds it (retail writes the player's
                // `+0x5C` and calls the anim tick `FUN_800204F8`), and the
                // clip-control ops `2B`/`2C`/`2D` read and write its `+0x62`.
                // A fresh context word in its place left the switch pull's
                // `AC F8 08` / `AD F8 08` end-latch spin (`chitei2` P0[33])
                // waiting on a bit nothing latches, until the park timeout
                // abandoned the record before its flag write.
                // REF: FUN_800204F8, FUN_8003C83C
                if player_target {
                    // The player record's `+0x14` / `+0x16` / `+0x18` are
                    // what an op on it reads: a `CD F8 ..` box test compares
                    // the player's tile. A context seeded at the origin
                    // failed every such test - `ropeway` P0[2], the station
                    // door, opens only for a player inside tiles
                    // (27..30, 33..34) and stayed shut for all of them.
                    // REF: FUN_8003C83C, FUN_801DE840 (0x4D arm)
                    if let Some(p) = host.world.player_actor_slot
                        && let Some(a) = host.world.actors.get(p as usize)
                    {
                        player_ctx.world_x = a.move_state.world_x as u16;
                        player_ctx.world_y = a.move_state.world_y as u16;
                        player_ctx.world_z = a.move_state.world_z as u16;
                    }
                    if b & 0x7F == 0x22
                        && let Some(&move_id) = id.bytecode.get(id.pc + 2)
                    {
                        host.world.bind_player_script_clip(move_id);
                    }
                    let hint = host.world.player_clip_frames_hint();
                    player_ctx.local_flags = host.world.props.bank.player_clip(hint).flags;
                }
                // A walk-to-tile on the player (`C7 F8 <tx> <tz> <mode>`)
                // hands the leg to the walk kernel and parks the record until
                // the player lands - the lift at `taiku` P0[6] walks the
                // player onto its platform, rides, and walks them off the
                // far side (`C7 F8 6D 4E 32`, then `C7 F8 6D CF 32`) before
                // its wall paint reopens the corridor. Stepping the op on a
                // throwaway context left the player where the touch found
                // them, on the near side of the lift.
                // REF: FUN_801DE840 (0x801DEFC0..0x801DF054), FUN_8003774C (case 0x47)
                // A door bind whose record also teleports the player plays its
                // walk-in leg like any other: the contact dispatch leaves the
                // record's `A3 F8` to the record (`check_field_walk_touch`),
                // so `tower`'s floor doors walk the player into the doorway,
                // close behind them, then jump to the next floor.
                // The record's next cross-context op on the player waits while
                // a compass leg it armed is still walking the player; the
                // halt clear `32 F8 0A` is exempt.
                // REF: FUN_801DE840 (0x801DE90C..0x801DE944, 0x801DE8E0..0x801DE904)
                if player_target
                    && id.player_glide.is_some()
                    && !(b == 0xB2 && id.bytecode.get(id.pc + 2) == Some(&0x0A))
                {
                    break;
                }
                // A compass walk on the player (`B7 F8 b0 b1` / `C1 F8 b0
                // b1`): arm the leg and run on past the op.
                // REF: FUN_8003774C (the 0x37 / 0x41 arm), FUN_801DE840 (0x801DEEDC..0x801DEF00)
                if player_target
                    && matches!(b, 0xB7 | 0xC1)
                    && let (Some(&body0), Some(&body1)) =
                        (id.bytecode.get(id.pc + 2), id.bytecode.get(id.pc + 3))
                {
                    let mut glide = legaia_engine_field::cutscene_timeline::TimelinePlayerGlide {
                        state: legaia_engine_motion_vm::motion_vm::MotionState {
                            speed: 1,
                            ..Default::default()
                        },
                        body0,
                        body1,
                        rate: if b == 0xB7 { 0x80 } else { 0x40 },
                        resume_pc: id.pc + 4,
                        frames: 0,
                    };
                    id.pc += 4;
                    let grid = host.world.clock.script_frame_delta();
                    glide.state.speed = grid;
                    if grid == 0 || host.world.step_player_glide(&mut glide) {
                        id.player_glide = Some(glide);
                    }
                    continue;
                }
                if player_target
                    && b == 0xC7
                    && let (Some(&b0), Some(&b1), Some(&b2)) = (
                        id.bytecode.get(id.pc + 2),
                        id.bytecode.get(id.pc + 3),
                        id.bytecode.get(id.pc + 4),
                    )
                {
                    id.pc += 5;
                    if (b0 & 0x7F, b1 & 0x7F)
                        != legaia_engine_field::man_field_scripts::PARKED_SENTINEL_TILE
                    {
                        let decode = |b: u8| -> i16 {
                            i16::from(b & 0x7F) * 0x80 + 0x40 + if b & 0x80 != 0 { 0x40 } else { 0 }
                        };
                        id.player_walk =
                            Some(legaia_engine_field::inline_dialogue::PropPlayerWalk {
                                target: (decode(b0), decode(b1)),
                                speed: crate::world::field_npc_walk_step_speed(0x80, b2 & 7),
                                approach: b2 >> 4,
                                frames: 0,
                            });
                        break;
                    }
                    continue;
                }
                // An op aimed at another actor that no arm below routes to
                // that actor (an id no placement or object answers, an op on
                // an object with no word routing) runs on the record's own
                // context, which stands in for the target, so a park the op
                // raises (`+0x10 |= 0x400`) lands on it. In retail the
                // target's walk kernel clears that bit on the leg's terminal
                // frame (`FUN_8003774C`), so the runner keeps the record's own
                // halt state across the op, as the talk runner does. Left
                // up, the bit held the record's
                // next op on any actor at the dispatcher's halted-target
                // early-out (`0x801DE90C..0x801DE944`) until the park
                // timeout, and the prop's flag word carried it into every
                // later touch: `jiji`'s counters (P0[5], P0[7], P0[8]) walk
                // and turn their keeper before the greeting, and each touch
                // froze the player for the timeout and greeted no one.
                // REF: FUN_8003774C, FUN_801DE840 (0x801DE90C..0x801DE944)
                let other_target = b & 0x80 != 0 && !player_target;
                let own_halt = (id.ctx.flags & 0x400, id.ctx.saved_pc, id.ctx.wait_accum);
                // A motion op aimed at a **placement** moves that placement:
                // `FUN_8003C83C` resolves the id to the NPC, the arm raises
                // its halt bit and hands the leg to its walk kernel, and the
                // record runs on past the op. The record's next op on the
                // same NPC waits at the dispatcher's halted-target early-out
                // until the kernel lands the leg (the halt clear `B2 <id> 0A`
                // is exempt). `koin3`'s dance-hall door turns its doorman
                // with `B8 26 80 14`, `jiji`'s counters walk and turn their
                // keeper, `stone`'s and `vell`'s records walk and face their
                // cast. Stepped on the record's own context, nobody moved.
                // REF: FUN_8003C83C, FUN_801DE840 (0x801DE90C..0x801DE944), FUN_8003774C
                let npc_slot = other_target
                    .then(|| vm::field::peek_extended(&id.bytecode, id.pc))
                    .flatten()
                    .and_then(|t| host.world.placement_slot_for_target(t));
                if let Some(slot) = npc_slot {
                    let halt_clear = b == 0xB2 && id.bytecode.get(id.pc + 2) == Some(&0x0A);
                    if !halt_clear && host.world.npc_leg_in_flight(slot) {
                        parked = true;
                        break;
                    }
                    if let Some(next_pc) = host.world.run_placement_motion_op(&id.bytecode, id.pc) {
                        id.pc = next_pc;
                        continue;
                    }
                }
                // An actor-word op aimed at another **placed object** lands on
                // that object's words, not on the record's own: `tower`'s floor
                // doors open the partner door one floor away with `AC <id> 07`,
                // `AC <id> 01`, `AB <id> 03`, drop its collision with `B1 <id>
                // 00`, wait on its end latch with `AC <id> 08` / `AD <id> 08`
                // and close it with the mirror run. Stepped on the record's own
                // context, the partner door never moved and the record's own
                // door swung again in its place, out of view.
                // A record naming its own object (`rayman` P0[4]'s `AB 04 07`)
                // is the executing context already.
                let object_target = if other_target {
                    vm::field::peek_extended(&id.bytecode, id.pc)
                        .filter(|_| is_object_word_op(&id.bytecode, id.pc))
                        .and_then(|t| host.world.cross_context_object_prop(t))
                        .filter(|&r| Some(r) != own_record)
                } else {
                    None
                };
                // An actor-word op aimed at a **placement** (a partition-1 NPC)
                // lands on that NPC's context: its `+0x62` clip-control word,
                // its `+0x10` bank bit and its clip. `koin3`'s dance-hall door
                // (P0[3]) has its doorman (P1[2], id `0x26`) step across the
                // doorway - `AC 26 07`, `AB 26 03`, `AC 26 01`, `B2 26 18`,
                // `A2 26 35`, `AC 26 08` / `AD 26 08` - and the door itself
                // never moves unless the Gold Card flags let the record reach
                // its own `2B 03`. Stepped on the door's own context, the
                // doorman's clip bits drove the door's clip, which then looped
                // its swing for the rest of the visit, and the doorman stood
                // still.
                // REF: FUN_8003C83C, FUN_800204F8
                let npc_target = (other_target && object_target.is_none())
                    .then(|| npc_word_op_slot(host.world, &id.bytecode, id.pc))
                    .flatten();
                let step = if let Some(slot) = npc_target {
                    step_npc_word_op(&mut host, &mut id, slot)
                } else if let Some(key) = object_target {
                    let mut target_ctx = host.world.object_prop_ctx(key);
                    let r = vm::field::step_with_caller(
                        &mut host,
                        &mut target_ctx,
                        &mut id.ctx,
                        false,
                        &id.bytecode,
                        id.pc,
                    );
                    host.world.store_object_prop_ctx(key, &target_ctx);
                    r
                } else {
                    let ctx = if player_target {
                        &mut player_ctx
                    } else {
                        &mut id.ctx
                    };
                    field_step_routed(&mut host, ctx, &id.bytecode, id.pc)
                };
                if other_target && own_halt.0 == 0 && id.ctx.flags & 0x400 != 0 {
                    id.ctx.flags &= !0x400;
                    id.ctx.saved_pc = own_halt.1;
                    id.ctx.wait_accum = own_halt.2;
                }
                // The record's own MOVE_TO seats the prop actor itself
                // (`0x801DECAC`, the non-player arm: the tile goes into the
                // executing actor's `+0x14` / `+0x18`). `vozz` P0[1], a
                // thicket across East Voz's paths, parks itself on the hide
                // tile with `23 7F 7F` once the player examines it holding
                // `0x079`.
                // REF: FUN_801DE840 (0x23 arm)
                if b == 0x23
                    && matches!(step, FieldStepResult::Advance { .. })
                    && let (Some(&xb), Some(&zb)) =
                        (id.bytecode.get(id.pc + 1), id.bytecode.get(id.pc + 2))
                {
                    let decode = |b: u8| -> i32 {
                        i32::from(b & 0x7F) * 0x80 + 0x40 + if b & 0x80 != 0 { 0x40 } else { 0 }
                    };
                    own_seat = Some((decode(xb), decode(zb)));
                }
                if player_target && matches!(b & 0x7F, 0x2B..=0x2D) {
                    let hint = host.world.player_clip_frames_hint();
                    host.world.props.bank.player_clip(hint).flags = player_ctx.local_flags;
                    host.world
                        .mirror_player_clip_control(player_ctx.local_flags);
                }
                match step {
                    FieldStepResult::Advance { next_pc }
                        if next_pc <= id.pc
                            && id.visited.get(next_pc).copied().unwrap_or(false) =>
                    {
                        // Backward wrap onto an executed PC: the record's
                        // trailing park loop. The interaction is over.
                        id.done = true;
                        break;
                    }
                    FieldStepResult::Advance { next_pc } => {
                        id.pc = next_pc;
                        if b == 0x21 {
                            // The raw `21` ends the interaction pass (retail
                            // `FUN_8003CF7C` breaks on it); the PC past it is
                            // where the next touch resumes.
                            id.done = true;
                            break;
                        }
                    }
                    FieldStepResult::Yield { resume_pc } => id.pc = resume_pc,
                    // Waitable ops park the run and re-test next frame - the
                    // `2D 08` end-latch spin while the swing plays, a `4A`
                    // frame wait, a busy fade. Retail's SM does exactly this
                    // (the dispatcher "returns" and is re-entered per frame).
                    FieldStepResult::Halt { .. } => {
                        parked = true;
                        break;
                    }
                    // An op the port can't advance past: end rather than
                    // soft-lock the engaged player.
                    FieldStepResult::Pending { .. } | FieldStepResult::Unknown { .. } => {
                        id.done = true;
                        break;
                    }
                }
            }
            if budget == 0 {
                // Step-budget exhausted without a yield: treat as parked (the
                // next tick continues).
                parked = true;
            }
        }
        // Mirror the context back onto the live prop: anim word, rate, and
        // the `+0x10` class word (whose bit `0` is the door's collision-off).
        self.sync_prop_from_ctx(anchor, &id.ctx);
        if let Some(seat) = own_seat {
            self.seat_prop(anchor, seat);
        }
        if parked {
            id.park_frames = id.park_frames.saturating_add(1);
            if id.park_frames > PROP_RUN_PARK_TIMEOUT {
                id.done = true;
            }
        } else {
            id.park_frames = 0;
        }
        if id.done && id.player_glide.is_none() {
            self.restore_owed_player_scale(&id.bytecode, id.pc, &id.visited);
            self.finish_prop_interaction(&mut id, anchor);
        } else {
            self.dialog.inline = Some(id);
        }
    }

    /// Interaction teardown: write the resume cursor back onto the prop
    /// (retail parks `actor+0x9E` where the dispatcher stopped) and release
    /// the player (the dialog SM teardown clears the `0x80000` engaged flag).
    fn finish_prop_interaction(
        &mut self,
        id: &mut legaia_engine_field::inline_dialogue::InlineDialogue,
        anchor: (u8, u8),
    ) {
        self.sync_prop_from_ctx(anchor, &id.ctx);
        if let Some(prop) = self.props.bank.props.get_mut(&anchor)
            && id.pc < prop.record_body.len()
        {
            prop.parked_pc = id.pc;
        }
        self.set_player_engaged(false);
        self.dialog.inline = None;
    }

    /// Move prop `anchor`'s actor to `seat`: its position, its contact-box
    /// centre and its collider rows travel together, so a prop that parks
    /// itself on the hide tile stops blocking where it stood - retail's
    /// candidate walk `FUN_801CF754` culls an actor that far from the player.
    fn seat_prop(&mut self, anchor: (u8, u8), seat: (i32, i32)) {
        let Some(prop) = self.props.bank.props.get_mut(&anchor) else {
            return;
        };
        let (dx, dz) = (seat.0 - prop.world.0, seat.1 - prop.world.1);
        prop.world = seat;
        prop.collider = (prop.collider.0 + dx, prop.collider.1 + dz);
        for c in &mut self.props.colliders {
            if c.anchor == Some(anchor) {
                c.center = (c.center.0 + dx, c.center.1 + dz);
                c.live = (c.live.0 + dx, c.live.1 + dz);
            }
        }
    }

    /// Carry every placed object's collider rows to where a script has moved
    /// the object's actor.
    ///
    /// Retail has no separate collision row to move: the actor box test
    /// `FUN_801CFC40` centres a static-class box on the actor's live
    /// `+0x14` / `+0x18` plus the record's footprint offset
    /// (`0x801CFCF4..0x801CFD6C`, `0x801CFDA0..0x801CFDAC`), and a
    /// moving-class box on the live position alone. So a cross-context seat
    /// (`A3 <obj> <x> <z>`), a `4C C3` re-seat or a park at the hide box moves
    /// what the player collides with on the same frame it moves what is
    /// drawn. `nilboa`'s ice gate (`P0[4]`) is the case that needs it: the
    /// beat that raises `0x456` re-seats the gate off the walkable map
    /// (`CC 04 C3` in `P2[11]`), and a row left at the `.MAP` seat kept the
    /// way west shut until the scene was loaded again.
    ///
    /// The displacement is the object-bind channel's, measured from its bind
    /// seat, as [`Self::object_draw_displacements`] measures it for the draw;
    /// like the draw it carries that object alone.
    ///
    /// REF: FUN_801CFC40
    pub(crate) fn sync_prop_colliders_to_object_actors(&mut self) {
        if self.props.colliders.is_empty() {
            return;
        }
        let moves: Vec<(usize, (i32, i32))> = self
            .field_vm
            .channels
            .iter()
            .filter(|c| c.object_bind)
            .filter_map(|c| {
                let (sx, sz) = self
                    .field_vm
                    .object_binds_by_key
                    .get(&c.placement_index)?
                    .seat;
                let d = (
                    i32::from(c.ctx.world_x as i16) - i32::from(sx),
                    i32::from(c.ctx.world_z as i16) - i32::from(sz),
                );
                let applied = self
                    .props
                    .object_shifts
                    .get(&c.placement_index)
                    .copied()
                    .unwrap_or((0, 0));
                (d != applied).then_some((c.placement_index, (d.0 - applied.0, d.1 - applied.1)))
            })
            .collect();
        for (key, (dx, dz)) in moves {
            let shift = self.props.object_shifts.entry(key).or_insert((0, 0));
            *shift = (shift.0 + dx, shift.1 + dz);
            let Some(anchor) = self.object_key_anchor(key) else {
                continue;
            };
            let Some(world) = self.props.bank.props.get(&anchor).map(|p| p.world) else {
                continue;
            };
            self.seat_prop(anchor, (world.0 + dx, world.1 + dz));
        }
    }

    /// Copy the executing context's actor words back onto the live prop and
    /// its collider row: `local_flags` -> `+0x62` (anim control), `field_6a`
    /// -> `+0x6A` (rate), `flags` -> `+0x10` - and when the script has set
    /// `+0x10 & 3` (`31 00`), drop the prop from the collision layer, exactly
    /// as `FUN_801CF754` / `FUN_801CF9F4` skip `flags & 3` actors from then
    /// on.
    fn sync_prop_from_ctx(&mut self, anchor: (u8, u8), ctx: &FieldCtx) {
        let Some(prop) = self.props.bank.props.get_mut(&anchor) else {
            return;
        };
        prop.anim.flags = ctx.local_flags;
        if ctx.field_6a != 0 {
            prop.anim.rate = ctx.field_6a;
        }
        // The collision layer reads `+0x10 & 3` live
        // ([`Self::prop_collider_exempt`]): an opened door stops blocking, and
        // a door its record closes again (`32 00`) blocks again.
        prop.cflags = ctx.flags;
        // A record's own `31 03` retires the object: its draw leaves with the
        // hidden-record set, which reads the channel.
        if ctx.flags & legaia_engine_field::field_channels::CONTEXT_RETIRE_BIT != 0
            && let Some(key) = self.object_key_at_anchor(anchor)
            && let Some(ch) = self
                .field_vm
                .channels
                .iter_mut()
                .find(|c| c.object_bind && c.placement_index == key)
        {
            ch.ctx.flags |= legaia_engine_field::field_channels::CONTEXT_RETIRE_BIT;
        }
    }

    /// The object key of the placed object a cross-context target byte
    /// names, when that object has a prop in the bank: `FUN_8003C83C` walks
    /// the actor list for `+0x50 == target`, and a bound object's `+0x50` is
    /// its gate-0 trigger's flat record index (`FUN_8003A55C`) - the walk
    /// answers the first object the record binds. A target the placement
    /// channels claim first is not an object; with no channel list (a prop
    /// run before the channels spawn) the id is the record itself.
    ///
    /// REF: FUN_8003C83C, FUN_8003A55C
    pub(crate) fn cross_context_object_prop(&self, target: u8) -> Option<usize> {
        if target == legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET || target == 0xFB {
            return None;
        }
        let record = match legaia_engine_field::field_channels::resolve_target(
            &self.field_vm.channels,
            target,
        ) {
            Some(ci) => {
                let c = &self.field_vm.channels[ci];
                if !c.object_bind {
                    return None;
                }
                c.placement_index
            }
            None => usize::from(target),
        };
        self.object_key_anchor(record)
            .is_some_and(|a| self.props.bank.props.contains_key(&a))
            .then_some(record)
    }

    /// The key of the object whose prop stands at `anchor` (the first, when
    /// several objects share the key tile); with no binds installed, the
    /// prop's record.
    pub(crate) fn object_key_at_anchor(&self, anchor: (u8, u8)) -> Option<usize> {
        if !self.field_vm.object_binds_by_key.is_empty() {
            return self
                .field_vm
                .channels
                .iter()
                .filter(|c| c.object_bind)
                .map(|c| c.placement_index)
                .find(|k| {
                    self.field_vm
                        .object_binds_by_key
                        .get(k)
                        .is_some_and(|b| b.key_tile == anchor)
                });
        }
        self.props.bank.props.get(&anchor).map(|p| p.record)
    }

    /// A context standing for placed object `key`'s actor: its `+0x62`
    /// clip-control word, `+0x10` flag word and `+0x6A` rate, read from the
    /// object's prop.
    pub(crate) fn object_prop_ctx(&self, key: usize) -> FieldCtx {
        let p = self
            .object_key_anchor(key)
            .and_then(|a| self.props.bank.props.get(&a));
        FieldCtx {
            script_id: self.object_key_record(key as u16),
            local_flags: p.map_or(0, |p| p.anim.flags),
            flags: p.map_or(0, |p| p.cflags),
            field_6a: p.map_or(0, |p| p.anim.rate),
            saved_pc: 0xFFFF_FFFF,
            field_72: 0x1000,
            ..FieldCtx::default()
        }
    }

    /// Write an op's result on [`Self::object_prop_ctx`] back onto the prop
    /// the object draws and onto its object-bind channel, so the anim tick and
    /// the collision layer read the new words on the next frame.
    pub(crate) fn store_object_prop_ctx(&mut self, key: usize, ctx: &FieldCtx) {
        if let Some(p) = self
            .object_key_anchor(key)
            .and_then(|a| self.props.bank.props.get_mut(&a))
        {
            p.anim.flags = ctx.local_flags;
            p.cflags = ctx.flags;
            if ctx.field_6a != 0 {
                p.anim.rate = ctx.field_6a;
            }
        }
        if let Some(c) = self
            .field_vm
            .channels
            .iter_mut()
            .find(|c| c.object_bind && c.placement_index == key)
        {
            c.ctx.local_flags = ctx.local_flags;
            c.ctx.flags = ctx.flags;
            if ctx.field_6a != 0 {
                c.ctx.field_6a = ctx.field_6a;
            }
        }
    }

    /// `FUN_801CF754` / `FUN_801CF9F4`'s live `flags & 3` filter for a prop
    /// collider: the bound prop's `+0x10` as its record last left it.
    pub(crate) fn prop_collider_exempt(&self, c: &FieldPropCollider) -> bool {
        c.anchor
            .and_then(|a| self.props.bank.props.get(&a))
            .is_some_and(|p| p.collision_exempt())
    }
}

/// The `0x4C` sub-ops whose arm reads or writes the **resolved actor**, the
/// context the dispatcher prologue swaps into `s5` for a `0x80`-prefix op
/// (`jal 0x8003C83C` / `move s5,v0` at `0x801DE8A8..0x801DE8B0`): the
/// clip-control pair `35` / `36` (`+0x62`), the player copy `37` (`+0x14` ..
/// `+0x18`, `+0x26`), the nibble-4 slots `40` .. `45` and `48` (`+0x72`,
/// `+0x6A`, `+0x8E`, `+0x24`, `+0x28`, the look turn, `+0x26`), the model
/// set `50`, the seat `51`, the tint `81` (`+0x74` / `+0x78`), the clip
/// cursor test `8C` (`+0x68`), the clip phase `C0`, the height-law bit
/// `D9` / `DA` (`+0x10`), the position copy `E3` and the box test `E4`
/// (`+0x14` / `+0x18`). Every other sub-op works on globals, the player or
/// the script's own flow, and runs wherever it is stepped.
///
/// REF: FUN_801DE840 (0x801DE894..0x801DE8B4), FUN_8003C83C
pub(in crate::world) fn is_resolved_actor_4c(sub: u8) -> bool {
    matches!(
        sub,
        0x35..=0x37
            | 0x40..=0x45
            | 0x48
            | 0x50
            | 0x51
            | 0x81
            | 0x8C
            | 0xC0
            | 0xD9
            | 0xDA
            | 0xE3
            | 0xE4
    )
}

/// The placement slot a prop record's cross-context op at `pc` reaches, when
/// the op is one of the actor-word ops the prop runner routes onto the NPC's
/// own context ([`step_npc_word_op`]): an `A2 <id> <clip>` ExecMove, an
/// `A3 <id> x z` seat, a `CC <id> C3` re-seat at the placement's own
/// record, a clip-control `AB` / `AC` / `AD`, a class-bit `B1` / `B2` - but
/// not the retire (`3`), engage (`8`) or halt (`0xA`)
/// bits, whose side effects the runner models elsewhere or not at all - or
/// a `CC <id> ..` sub-op on the resolved actor ([`is_resolved_actor_4c`]).
/// `None` for any other op, and for a target no placement channel carries.
pub(in crate::world) fn npc_word_op_slot(world: &World, bytecode: &[u8], pc: usize) -> Option<u8> {
    let b = *bytecode.get(pc)?;
    let routed = match b & 0x7F {
        0x22 | 0x23 | 0x2B..=0x2D => true,
        0x4C => bytecode
            .get(pc + 2)
            .is_some_and(|&sub| sub == 0xC3 || is_resolved_actor_4c(sub)),
        0x31 | 0x32 => bytecode
            .get(pc + 2)
            .is_some_and(|bit| !matches!(bit & 0x1F, 3 | 8 | 0xA)),
        // `B4 <id> 1x` / `2x` (the attached light and its re-script) and
        // `3x` (the ambient-tree install at `s5 + 0x14`).
        0x34 => bytecode
            .get(pc + 2)
            .is_some_and(|sub| matches!(sub >> 4, 1..=3)),
        _ => false,
    };
    if !routed {
        return None;
    }
    let t = vm::field::peek_extended(bytecode, pc)?;
    if t == legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET || t == 0xFB {
        return None;
    }
    let ci = legaia_engine_field::field_channels::resolve_target(&world.field_vm.channels, t)?;
    let ch = &world.field_vm.channels[ci];
    if ch.object_bind {
        return None;
    }
    u8::try_from(ch.placement_index).ok()
}

/// Step one actor-word op of a prop record on placement `slot`'s own context
/// (the channel retail's `FUN_8003C83C` resolves the target byte to), with
/// the channel marked executing so an `A2` binds the NPC's clip. The NPC
/// anim tick moves that context's `+0x62` with the clip
/// ([`World::tick_npc_clips`]), so an `AD <id> 08` end-latch spin waits on
/// the NPC's own clip; with no clip cursor bound to the NPC nothing would
/// latch the bit, and the spin is taken as a clip of no length.
///
/// A re-seat (`CC <id> C3`) always publishes the context's tile: the
/// teleport puts the actor back on its record's seat, which is where its
/// context already stands when an earlier seat parked only the drawn
/// placement.
///
/// REF: FUN_8003C83C, FUN_800204F8
pub(in crate::world) fn step_npc_word_op(
    host: &mut FieldHostImpl<'_>,
    id: &mut legaia_engine_field::inline_dialogue::InlineDialogue,
    slot: u8,
) -> FieldStepResult {
    let b = id.bytecode.get(id.pc).copied().unwrap_or(0);
    if b & 0x7F == 0x2D && !host.world.npc_clip_cursor_bound(slot) {
        return FieldStepResult::Advance { next_pc: id.pc + 3 };
    }
    let Some(ci) = host
        .world
        .field_vm
        .channels
        .iter()
        .position(|c| !c.object_bind && c.placement_index == usize::from(slot))
    else {
        let width = legaia_scene_formats::field_disasm::decode(&id.bytecode, id.pc)
            .map_or(3, |insn| insn.size);
        return FieldStepResult::Advance {
            next_pc: id.pc + width,
        };
    };
    let mut target_ctx = host.world.field_vm.channels[ci].ctx.clone();
    // Op `34 3x` reads the actor's live `+0x14` / `+0x18`; a leg moves the
    // drawn placement, not this context.
    if b & 0x7F == 0x34
        && let Some(&(x, z)) = host.world.npcs.positions.get(&slot)
    {
        target_ctx.world_x = x as u16;
        target_ctx.world_z = z as u16;
    }
    // The port keeps a placement's legs in its own leg state, which the
    // caller has already tested (`World::npc_leg_in_flight`); a halt bit the
    // channel's context still carries from an earlier leg or acquire is not
    // the dispatcher's to refuse on here. It is put back after the op.
    let held = target_ctx.flags & 0x400;
    target_ctx.flags &= !0x400;
    host.world.field_vm.executing_channel = Some(slot);
    let r = vm::field::step_with_caller(
        host,
        &mut target_ctx,
        &mut id.ctx,
        false,
        &id.bytecode,
        id.pc,
    );
    host.world.field_vm.executing_channel = None;
    // A seat (`A3 <id> x z`, `CC <id> 51 x z ..`, `CC <id> C3`) lands on the
    // NPC's `+0x14` / `+0x18` at once, ending any leg it had in flight.
    let before = host
        .world
        .field_vm
        .channels
        .get(ci)
        .map(|c| (c.ctx.world_x, c.ctx.world_z));
    let reseat = b & 0x7F == 0x4C && id.bytecode.get(id.pc + 2) == Some(&0xC3);
    let effect_op = b & 0x7F == 0x34;
    if reseat || (!effect_op && before != Some((target_ctx.world_x, target_ctx.world_z))) {
        let pos = (target_ctx.world_x as i16, target_ctx.world_z as i16);
        host.world.npcs.positions.insert(slot, pos);
        host.world.npcs.motions.remove(&slot);
        host.world.npcs.glide_legs.remove(&slot);
    }
    target_ctx.flags |= held;
    if let Some(c) = host.world.field_vm.channels.get_mut(ci) {
        c.ctx = target_ctx;
    }
    r
}

/// The actor-word ops a prop run routes onto another placed object
/// ([`World::cross_context_object_prop`]): the clip-control word `+0x62`
/// (`2B` / `2C` / `2D`, `4C 35` / `4C 36`), the flag word `+0x10` (`31` /
/// `32` / `33`) and the rate `+0x6A` (`4C 41`). Any other op on an object
/// keeps the stand-in context.
fn is_object_word_op(bytecode: &[u8], pc: usize) -> bool {
    match bytecode.get(pc).map(|b| b & 0x7F) {
        Some(0x2B..=0x2D | 0x31..=0x33) => true,
        Some(0x4C) => matches!(bytecode.get(pc + 2), Some(0x35 | 0x36 | 0x41)),
        _ => false,
    }
}

impl World {
    /// Step the player one frame of a script walk-to-tile leg (op `0x47`
    /// on the player, `C7 F8 <tx> <tz> <mode>`), on the floor under every
    /// step: `iterations` steps of `speed`, the frame step's count. Returns
    /// whether the leg is done (or there is no player to walk), which, as the
    /// kernel has it, is the frame after the one that lands on the target
    /// unless an earlier iteration of the same frame landed. Shared by the
    /// cutscene timeline's player leg and a prop run's.
    ///
    /// The player runs the same walk kernel an NPC leg does
    /// ([`legaia_engine_motion_vm::motion_vm::step`]): `approach` is the mode
    /// byte's high nibble, so a default-mode leg closes the dominant axis
    /// first and only then cuts the diagonal, and the facing is the
    /// eight-point compass entry of the step's axis signs (the `0x47` tail,
    /// `0x80037D4C..0x80037DDC`), written on every moving frame as the tail
    /// writes it - never an in-between angle.
    ///
    /// REF: FUN_8003774C (case 0x47)
    pub(crate) fn step_player_walk_leg(
        &mut self,
        target: (i16, i16),
        speed: u16,
        iterations: u16,
        approach: u8,
    ) -> bool {
        use legaia_engine_motion_vm::motion_vm::{MotionState, MotionTarget, StepResult, step};
        let Some(p) = self.player_actor_slot else {
            return true;
        };
        let Some(actor) = self.actors.get_mut(p as usize) else {
            return true;
        };
        let ms = &mut actor.move_state;
        let mut st = MotionState {
            world_x: ms.world_x,
            world_z: ms.world_z,
            speed: speed.max(1),
            approach,
            walk_iterations: iterations,
            ..MotionState::default()
        };
        let goal = MotionTarget {
            x: target.0,
            z: target.1,
            ..MotionTarget::default()
        };
        let done = step(&mut st, goal, &[0x47]) == StepResult::Done;
        ms.world_x = st.world_x;
        ms.world_z = st.world_z;
        if st.yaw_written {
            ms.render_26 = (st.yaw & 0x0FFF) as i16;
        }
        // The kernel writes `+0x14` / `+0x18` alone (no store to `+0x16` in
        // `FUN_8003774C`); the player's height is the settle's.
        self.snap_player_y_unless_settling();
        done
    }

    /// Land the player's Y on the floor under it after a script moved it,
    /// unless the retail settle ([`crate::world::FieldLocomotion::vertical_settle`])
    /// owns the footing. Retail's walk kernel `FUN_8003774C` (op `0x47`'s
    /// walk-to-tile, the `0x37` / `0x41` compass legs) writes `+0x14` /
    /// `+0x18` only, and
    /// `FUN_801D1BA0`'s glide carries `+0x16` onto the floor at `12` a vsync
    /// on every frame, movement lock or not: a carried player climbs a stair
    /// run `24` a pass (`tower_field_walk`'s film). Under the step snap alone
    /// nothing else would, so the floor under every step is sampled here: a
    /// leg that crosses a stair run (chitei2's escape beats) otherwise
    /// carries the start height through the steps.
    pub(crate) fn snap_player_y_unless_settling(&mut self) {
        if self.locomotion.vertical_settle {
            return;
        }
        if let Some(slot) = self.player_actor_slot
            && let Some(a) = self.actors.get(usize::from(slot))
        {
            let (x, z) = (a.move_state.world_x, a.move_state.world_z);
            let y = self.sample_field_floor_height(i32::from(x), i32::from(z));
            if let Some(a) = self.actors.get_mut(usize::from(slot)) {
                a.move_state.world_y = y.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
            }
        }
    }

    /// Set / clear the player's movement-disabled flag (`+0x10 & 0x80000`) -
    /// the engaged bit `FUN_801D5B5C` raises on the touch post and the dialog
    /// SM teardown clears.
    pub(crate) fn set_player_engaged(&mut self, engaged: bool) {
        if let Some(slot) = self.player_actor_slot
            && let Some(actor) = self.actors.get_mut(slot as usize)
        {
            if engaged {
                actor.move_state.flags |= 0x0008_0000;
            } else {
                actor.move_state.flags &= !0x0008_0000;
            }
        }
    }

    /// Bind a cross-context `A2 F8 <clip>` ExecMove onto the player: the
    /// bank's player cursor (what a following `AD F8 08` end-latch spin
    /// waits on), the player's script clip, and the gesture cue the windowed
    /// and browser hosts draw when the pick binds a scene-bank record
    /// ([`Self::player_move_cue`]).
    ///
    /// REF: FUN_800204F8
    /// The cursor (`+0x68`) of the clip a script poked on the player
    /// ([`Self::bind_player_script_clip`]), which the record's `AD F8 08`
    /// end-latch spin waits on. `None` before any poke.
    pub fn player_script_clip_cursor(&self) -> Option<i16> {
        self.props
            .bank
            .actor_clip(legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET)
            .map(|a| a.cursor)
    }

    pub(crate) fn bind_player_script_clip(&mut self, move_id: u8) {
        use legaia_engine_field::field_player_clip::ClipBank;
        let fallback = self.player_clip_frames_hint();
        let pick = self.field_player_script_clip(move_id);
        // A party-bank clip (`B1 F8 18` before the poke) is timed by the
        // party locomotion bundle; the id means something else in the scene
        // bank the cursor would otherwise read. `FUN_800204F8` takes the clip
        // through the bank's pointer either way (`0x80020534..0x8002056C`).
        let meta = match pick.bound() {
            Some((ClipBank::Party, record)) => self
                .locomotion
                .party_clip_meta
                .get(usize::from(record))
                .copied(),
            _ => self.props.bank.clip_meta(move_id),
        };
        self.props.bank.bind_actor_clip_meta(
            legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET,
            move_id,
            meta,
            fallback,
        );
        if let Some(id) = self.player_move_cue(&pick) {
            self.locomotion.player_move_cues.push(id);
        }
    }

    /// Name-substitution table for a record's dialog escapes: every
    /// `0xC1`/`0xC2`/`0xC4` escape pair in `record` resolved against the
    /// engine's live tables (`0xC1 63` = the party leader, `0xC2 xx` = item
    /// name - the same tables retail's dialog renderer consults), plus every
    /// `0xCE 0x0B..=0x0E` number escape resolved against the script-counter
    /// table. `None` when the record carries no resolvable escape.
    ///
    /// The counters are read when the box opens. Retail re-reads them each
    /// frame it draws, which differs only if a script rewrites a counter
    /// while its own box is still on screen.
    /// Hand a reading box opening now the window format the field VM set
    /// (op `4C E0`) and reset it, as the pager's reset stores `3` when the
    /// box closes: a format lasts one box.
    ///
    /// REF: FUN_801D84D0 (`0x801D8798`, `0x801D90EC`)
    pub(crate) fn apply_dialog_window_format(
        &mut self,
        panel: &mut legaia_engine_dialog::dialog::OwnedDialogPanel,
    ) {
        let rows = std::mem::replace(
            &mut self.dialog.window_rows,
            legaia_engine_dialog::dialog_window::WINDOW_ROWS as u8,
        );
        let instant = std::mem::take(&mut self.dialog.window_instant);
        // No `4C E0` since the last box: a window carried over a `0x24` keeps
        // its count (`_DAT_801F2740` is reset by states `0`, `6` and `9`, not
        // by the `3` -> `4` path).
        let rows = if usize::from(rows) == legaia_engine_dialog::dialog_window::WINDOW_ROWS {
            panel.box_rows().unwrap_or(usize::from(rows))
        } else {
            usize::from(rows)
        };
        panel.set_window_format(rows, instant);
    }

    pub fn dialog_substitutions(
        &self,
        record: &[u8],
    ) -> Option<legaia_engine_dialog::dialog::PanelSubstitutions> {
        let mut map: std::collections::HashMap<(u8, u8), Vec<u8>> = Default::default();
        for w in record.windows(2) {
            let (esc, arg) = (w[0], w[1]);
            match esc {
                0xC1 => {
                    // `0xC1 99` = current party leader; other args index the
                    // roster order.
                    let name = if arg == 99 {
                        self.party.party_names.get(usize::from(self.field_leader()))
                    } else {
                        self.party.party_names.get(arg as usize)
                    };
                    if let Some(name) = name {
                        map.entry((1, arg))
                            .or_insert_with(|| name.clone().into_bytes());
                    }
                }
                // `0xC2` / `0xC4` - an item name off the static item-name
                // table `0x8007436C + id*0xC`, which names every id. The
                // item catalog names only the usable items, so resolving
                // through it alone left a weapon or key item blank ("There
                // is a  in the treasure chest!", issue #556); it stays the
                // fallback for a disc-free host.
                0xC2 | 0xC4 => {
                    let name = self.item_display_name(arg).or_else(|| {
                        self.tables
                            .item_catalog
                            .get(arg)
                            .map(|e| e.name.to_string())
                    });
                    if let Some(name) = name {
                        map.entry((2, arg)).or_insert_with(|| name.into_bytes());
                    }
                }
                // `0xC7 xx` - an inline 8-byte SCUS name (the Ra-Seru
                // names), `FUN_80036044`'s `0x80073F24 + xx * 8`.
                0xC7 => {
                    if let Some(name) = self.tables.inline_names.get(usize::from(arg)) {
                        map.entry((7, arg)).or_insert_with(|| name.clone());
                    }
                }
                // `0xCE 0x0B..=0x0E` - the number escapes. Retail's renderer
                // reads the field VM's script-counter table `0x801C6460`
                // (`lh` at `0x80036A8C`) - the same table `4C CA/CB/CC`
                // write and op `0x4E` sub-`5..=8` compares.
                0xCE => {
                    if let Some(slot) = legaia_engine_dialog::dialog::script_counter_slot(arg) {
                        let value = self.field_vm.slot_table[slot];
                        map.entry((legaia_engine_dialog::dialog::SCRIPT_COUNTER_KEY, arg))
                            .or_insert_with(|| {
                                legaia_engine_dialog::dialog::script_counter_digits(value)
                            });
                    }
                }
                _ => {}
            }
        }
        if map.is_empty() {
            None
        } else {
            Some(std::sync::Arc::new(map))
        }
    }
}

/// `FUN_80038050`'s verdict on the byte after a dismissed box, for a prop
/// record: a parking byte ends the interaction with the cursor left **on**
/// it, and the next touch runs it.
///
/// The dialog SM that runs a touched prop's record is the one that runs an
/// NPC's (`FUN_80039B7C`; the call is `jal 0x80038050` at `0x80039C84`, and
/// the `0` return clears the context's `+0x10 & 0x100` with no test of what
/// kind of actor it is), so the rule is the same. The disc's prop records
/// are written to it: a box is followed by `0x48`, `0x24` or `0x25` where
/// the record goes on (a cupboard's close pass), by a picker's option bytes,
/// and by a bare `0x26` where the interaction is over - nearly always the
/// jump back to the record's first question.
///
/// Running that jump at once re-opened the question for good. A spring
/// (`izumi`'s: "Do you wish to drink this water?", a Yes / No picker, each
/// arm's last box followed by `26` back to the question) could not be left
/// by either answer; the runner's revisit guard does not stop it, because a
/// picker commit clears the visited map.
///
/// REF: FUN_80039B7C (`0x80039C84..0x80039CA8`), FUN_80038050
fn end_prop_talk_at_post_box_byte(id: &mut legaia_engine_field::inline_dialogue::InlineDialogue) {
    if let legaia_engine_field::inline_dialogue::TalkDispatch::EndParked(pc) =
        legaia_engine_field::inline_dialogue::talk_dispatch(&id.bytecode, id.pc)
    {
        id.pc = pc;
        id.done = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_scene::field_env::{PropAnim, PropAnimState, PropProgram};

    /// A prop whose run makes the **player** play a clip and waits for it -
    /// the shape of `chitei2` P0[33], the transport switch: `A2 F8 04`
    /// (player ExecMove), `AC F8 08` / `AD F8 08` (clear, then spin on the
    /// player's end latch), then the record's story-flag write and `21`.
    fn world_with_switch() -> (World, (u8, u8)) {
        let mut w = World::new();
        w.install_field_player(0);
        // `[u8 name_len = 0][u8 anim = 0]` header, then the script at pc 2.
        let record = vec![
            0x00, 0x00, // header
            0xA2, 0xF8, 0x04, // ExecMove player move 4
            0xAC, 0xF8, 0x08, // LFlag.Clear player bit 8
            0xAD, 0xF8, 0x08, // LFlag.Test player bit 8 (end-latch spin)
            0x54, 0xF0, // SysFlag.Set 0x4F0
            0x21, // end of interaction
        ];
        let anchor = (16, 19);
        w.props.bank.props.insert(
            anchor,
            PropAnimState {
                anim: PropAnim::spawned(1, 4, false, 0),
                program: PropProgram::default(),
                world: (2240, 2496),
                collider: (2240, 2496),
                record: 33,
                record_body: std::sync::Arc::new(record),
                parked_pc: 2,
                cflags: 0x4002_0000,
            },
        );
        (w, anchor)
    }

    /// A script that moves a placed object's actor from another context
    /// moves what the player collides with: the collider rows follow the
    /// object-bind channel's displacement from its bind seat, once.
    #[test]
    fn a_script_moved_object_takes_its_collider_along() {
        let mut w = World::new();
        let anchor = (16, 19);
        w.props.bank.props.insert(
            anchor,
            PropAnimState {
                anim: PropAnim::spawned(1, 4, false, 0),
                program: PropProgram::default(),
                world: (2112, 2496),
                collider: (2240, 2496),
                record: 4,
                record_body: std::sync::Arc::new(vec![0x00, 0x00, 0x21]),
                parked_pc: 2,
                cflags: 0,
            },
        );
        w.props.colliders.push(crate::world::FieldPropCollider {
            anchor: Some(anchor),
            center: (2240, 2496),
            live: (2112, 2496),
            moving_box: false,
            interact: false,
            solid: true,
        });
        w.install_object_binds(&[legaia_engine_field::man_field_scripts::ObjectScriptBind {
            key_tile: anchor,
            ..legaia_engine_field::man_field_scripts::ObjectScriptBind::at(4, (2112, 2496))
        }]);
        w.field_vm
            .channels
            .push(legaia_engine_field::field_channels::FieldChannel {
                placement_index: 4,
                ctx: FieldCtx {
                    script_id: 4,
                    world_x: 2112,
                    world_z: 2496,
                    ..FieldCtx::default()
                },
                record_offset: 0,
                pc: 0,
                done: true,
                object_bind: true,
            });
        // Unmoved: the rows stay on the `.MAP` seat.
        w.sync_prop_colliders_to_object_actors();
        assert_eq!(w.props.colliders[0].center, (2240, 2496));
        assert!(w.field_actor_point_blocked(2240, 2496));
        // A cross-context seat three tiles east and one south.
        w.field_vm.channels[0].ctx.world_x = 2112 + 384;
        w.field_vm.channels[0].ctx.world_z = 2496 + 128;
        w.sync_prop_colliders_to_object_actors();
        w.sync_prop_colliders_to_object_actors();
        assert_eq!(w.props.colliders[0].center, (2240 + 384, 2496 + 128));
        assert_eq!(w.props.colliders[0].live, (2112 + 384, 2496 + 128));
        assert_eq!(w.props.bank.props[&anchor].world, (2112 + 384, 2496 + 128));
        assert!(!w.field_actor_point_blocked(2240, 2496));
        assert!(w.field_actor_point_blocked(2240 + 384, 2496 + 128));
    }

    /// A prop whose record parks itself (`23 7F 7F`) leaves the collision
    /// layer where it stood: its collider rows move with the actor.
    #[test]
    fn a_prop_that_parks_itself_takes_its_collider_along() {
        let mut w = World::new();
        w.install_field_player(0);
        let record = vec![
            0x00, 0x00, // header
            0x23, 0x7F, 0x7F, // MoveTo the hide tile
            0x21, // end of interaction
        ];
        let anchor = (16, 19);
        w.props.bank.props.insert(
            anchor,
            PropAnimState {
                anim: PropAnim::spawned(1, 4, false, 0),
                program: PropProgram::default(),
                world: (2240, 2496),
                collider: (2240, 2496),
                record: 1,
                record_body: std::sync::Arc::new(record),
                parked_pc: 2,
                cflags: 0x4000_0000,
            },
        );
        w.props.colliders.push(crate::world::FieldPropCollider {
            anchor: Some(anchor),
            center: (2240, 2496),
            live: (2240, 2496),
            moving_box: false,
            interact: true,
            solid: true,
        });
        assert!(w.start_prop_interaction(anchor));
        for _ in 0..4 {
            w.step_prop_interaction();
        }
        let hide = 0x7F * 0x80 + 0x40;
        assert_eq!(w.props.colliders[0].center, (hide, hide));
        assert_eq!(w.props.bank.props[&anchor].world, (hide, hide));
    }

    /// A prop record whose box is followed by a bare `0x26` back to its own
    /// question - the shape of a spring's answer arm. The interaction ends
    /// when the box is dismissed, parked on the jump; the next touch runs the
    /// jump and asks again. A box followed by `0x48` runs on.
    #[test]
    fn a_prop_talk_ends_parked_on_the_jump_after_its_box() {
        use legaia_engine_system::input::PadButton;
        let mut w = World::new();
        w.install_field_player(0);
        let record = vec![
            0x00, 0x00, // header
            0x1F, b'O', b'n', b'e', 0x00, // pc 2: first box
            0x48, // pc 7: box closes, the record runs on
            0x54, 0xF0, // pc 8: SysFlag.Set 0x4F0
            0x1F, b'T', b'w', b'o', 0x00, // pc 10: second box
            0x26, 0xF1, 0xFF, // pc 15: back to pc 2 (15 + 1 - 15)
            0x21,
        ];
        let anchor = (16, 19);
        w.props.bank.props.insert(
            anchor,
            PropAnimState {
                anim: PropAnim::spawned(1, 4, false, 0),
                program: PropProgram::default(),
                world: (2240, 2496),
                collider: (2240, 2496),
                record: 33,
                record_body: std::sync::Arc::new(record),
                parked_pc: 2,
                cflags: 0x4002_0000,
            },
        );
        // Tap the confirm button every other frame until the run ends.
        let run = |w: &mut World| {
            let mut boxes = 0;
            let mut was_open = false;
            for f in 0..2_000 {
                if w.dialog.inline.is_none() {
                    break;
                }
                let open = w.dialog.inline.as_ref().is_some_and(|d| d.panel.is_some());
                if open && !was_open {
                    boxes += 1;
                }
                was_open = open;
                w.set_pad(if f % 2 == 0 {
                    PadButton::Cross.mask()
                } else {
                    0
                });
                w.dialog.input_consumed = false;
                w.tick_prop_interactions();
            }
            boxes
        };
        assert!(w.start_prop_interaction(anchor));
        assert_eq!(run(&mut w), 2, "both boxes show, once each");
        assert!(
            w.dialog.inline.is_none(),
            "the talk ended after its last box"
        );
        assert!(
            w.system_flag_test(0x4F0),
            "the 0x48 box ran on into the flag write"
        );
        assert_eq!(
            w.props.bank.props[&anchor].parked_pc, 15,
            "parked on the jump, not past it"
        );
        // The next touch runs the jump and the record asks again.
        assert!(w.start_prop_interaction(anchor));
        assert_eq!(run(&mut w), 2);
        assert_eq!(w.props.bank.props[&anchor].parked_pc, 15);
    }

    /// A spawned record's clip-word pokes at a placed object swing the clip
    /// it already has bound, with no `A2` after them - `town01` P2[3]'s door
    /// (`CC 00 41 10`, `AC 00 07`, `AC 00 01`, `AB 00 03`). Only the changed
    /// bits land, a zero step is not taken, and another record's prop is not
    /// touched.
    #[test]
    fn clip_word_pokes_swing_a_placed_objects_bound_clip() {
        use legaia_engine_scene::field_env::{ANIM_CLAMP, ANIM_END, ANIM_HOLD, ANIM_REVERSE};
        let mut w = World::new();
        let prop = |record: usize| {
            let mut anim = PropAnim::spawned(2, 6, false, 0);
            // The spawn program's park: held on frame 0, plus a bit the
            // object channel's context never saw.
            anim.flags = ANIM_HOLD | ANIM_REVERSE | 0x0004;
            PropAnimState {
                anim,
                program: PropProgram::default(),
                world: (0, 0),
                collider: (0, 0),
                record,
                record_body: std::sync::Arc::new(Vec::new()),
                parked_pc: 0,
                cflags: 0,
            }
        };
        w.props.bank.props.insert((1, 1), prop(0));
        w.props.bank.props.insert((2, 2), prop(5));
        let door = |w: &World| w.props.bank.props[&(1, 1)].anim;

        // Held: the cursor does not move.
        w.tick_actor_anims();
        assert_eq!(door(&w).cursor, 0);

        // `CC 00 41 10`: the step, halved by the op (16 -> 8 is no change
        // from the spawn rate; a real change lands, a zero does not).
        w.poke_object_prop_words(0, (0x82, 0x82), (8, 12));
        assert_eq!(door(&w).rate, 12);
        w.poke_object_prop_words(0, (0x82, 0x82), (12, 0));
        assert_eq!(door(&w).rate, 12);
        // `AC 00 07`, `AC 00 01`, `AB 00 03`.
        w.poke_object_prop_words(0, (0x82, 0x02), (12, 12));
        w.poke_object_prop_words(0, (0x02, 0x00), (12, 12));
        w.poke_object_prop_words(0, (0x00, 0x08), (12, 12));
        assert_eq!(door(&w).flags, ANIM_CLAMP | 0x0004);

        // The clip now plays once and clamps on its last frame.
        w.tick_actor_anims();
        assert_eq!(door(&w).cursor, 12);
        for _ in 0..16 {
            w.tick_actor_anims();
        }
        assert_eq!(door(&w).cursor, 6 * 16 - 1);
        assert_ne!(door(&w).flags & ANIM_END, 0);

        // The other record's prop stayed parked.
        let other = w.props.bank.props[&(2, 2)].anim;
        assert_eq!(
            (other.cursor, other.flags, other.rate),
            (0, ANIM_HOLD | ANIM_REVERSE | 0x0004, 8)
        );
    }

    /// The number escapes resolve against the field VM's script-counter
    /// table - the `0x801C6460` array `4C CA/CB/CC` write - by escape operand.
    #[test]
    fn number_escapes_resolve_against_the_script_counter_table() {
        let mut w = World::new();
        w.field_vm.slot_table[0] = 12;
        w.field_vm.slot_table[3] = -4;
        let record = [0x1F, 0xCE, 0x0B, b' ', 0xCE, 0x0E, 0xCE, 0x05, 0x00];
        let subs = w.dialog_substitutions(&record).expect("two number escapes");
        let key = legaia_engine_dialog::dialog::SCRIPT_COUNTER_KEY;
        assert_eq!(subs.get(&(key, 0x0B)), Some(&b"12".to_vec()));
        assert_eq!(subs.get(&(key, 0x0E)), Some(&b"0".to_vec()));
        assert_eq!(
            subs.get(&(key, 0x05)),
            None,
            "a sprite escape is not a number"
        );
    }

    /// Issue #556: the item-name escapes resolve off the static item-name
    /// table, which names every id - a key item or a weapon from a chest
    /// included, which the usable-item catalog has no row for.
    #[test]
    fn item_name_escapes_name_items_the_catalog_does_not_hold() {
        let mut w = World::new();
        let mut names = vec![None; 256];
        names[0xA6] = Some("Key".to_string());
        w.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
            item_names: Some(legaia_game_tables::item_names::ItemNameTable::from_names(
                names,
            )),
            ..Default::default()
        });
        assert!(w.tables.item_catalog.get(0xA6).is_none());
        let record = [0x1F, 0xC2, 0xA6, b' ', 0xC4, 0xA6, 0x00];
        let subs = w.dialog_substitutions(&record).expect("item escapes");
        assert_eq!(subs.get(&(2, 0xA6)), Some(&b"Key".to_vec()));
    }

    /// `0xC1 0x63` is the field leader (`DAT_80084597`), not the first name:
    /// Noa opens her own chests in her chapter.
    #[test]
    fn the_leader_name_escape_names_the_field_leader() {
        let mut w = World::new();
        w.party.party_names = vec!["Vahn".into(), "Noa".into()];
        w.party.active_party = vec![1];
        let subs = w
            .dialog_substitutions(&[0x1F, 0xC1, 0x63, 0x00])
            .expect("a name escape");
        assert_eq!(subs.get(&(1, 0x63)), Some(&b"Noa".to_vec()));
    }

    #[test]
    fn a_prop_run_waits_on_the_players_clip_not_on_a_blank_word() {
        let (mut w, anchor) = world_with_switch();
        assert!(w.start_prop_interaction(anchor));
        let mut ticks = 0;
        while w.dialog.inline.is_some() && ticks < PROP_RUN_PARK_TIMEOUT as usize {
            w.tick_prop_interactions();
            ticks += 1;
        }
        assert!(
            w.system_flag_test(0x4F0),
            "the run must get past the player's end-latch spin to its flag write \
             (ended after {ticks} ticks)"
        );
        assert!(
            ticks < 200,
            "the spin ends when the player's clip latches its end, not on the park \
             timeout ({ticks} ticks)"
        );
        assert_eq!(
            w.props
                .bank
                .actor_clip(legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET)
                .map(|a| a.anim_id),
            Some(4),
            "the ExecMove binds the player's clip cursor"
        );
    }

    /// A prop record's `CC <id> ..` sub-ops on a placement's actor land on
    /// the placement, as the dispatcher prologue swaps the resolved actor
    /// into `s5` - the shape of `stone`'s Shadow Gate records (`P0[2..=5]`),
    /// which hide Gi Delilas (`CC 12 40 ..`), seat Noa and Gala on Vahn
    /// (`CC 09 37`) and turn heads (`CC 16 45 ..` in `jiji`). Stepped on the
    /// record's own context, nobody hid, moved or looked anywhere.
    #[test]
    fn a_prop_records_actor_sub_ops_land_on_the_placement() {
        use legaia_engine_field::actor_look::LookKey;
        let mut w = World::new();
        w.install_field_player(0);
        let p = w.player_actor_slot.unwrap() as usize;
        w.actors[p].move_state.world_x = 10 * 128 + 64;
        w.actors[p].move_state.world_z = 12 * 128 + 64;
        w.actors[p].move_state.render_26 = 0x0C00;
        w.field_vm
            .channels
            .push(legaia_engine_field::field_channels::FieldChannel {
                placement_index: 2,
                ctx: FieldCtx {
                    script_id: 0x11,
                    world_x: 3 * 128 + 64,
                    world_z: 3 * 128 + 64,
                    field_72: 0x1000,
                    ..FieldCtx::default()
                },
                record_offset: 0,
                pc: 0,
                done: true,
                object_bind: false,
            });
        w.npcs.positions.insert(2, (3 * 128 + 64, 3 * 128 + 64));
        let record = vec![
            0x00, 0x00, // header
            0xCC, 0x11, 0x40, 0x00, 0x00, 0x00, 0x00, // +0x72 = 0
            0xCC, 0x11, 0x42, 0x2C, 0x01, 0x00, 0x00, // +0x8E = 300
            0xCC, 0x11, 0x45, 0x00, 0x00, 0x00, 0xF4, 0x01, 0x00, 0x00, 0x00,
            0x00, // look: object 0, Y = 500
            0xCC, 0x11, 0x37, // the player's spot and heading
            0x21,
        ];
        let anchor = (16, 19);
        w.props.bank.props.insert(
            anchor,
            PropAnimState {
                anim: PropAnim::spawned(1, 4, false, 0),
                program: PropProgram::default(),
                world: (2240, 2496),
                collider: (2240, 2496),
                record: 2,
                record_body: std::sync::Arc::new(record),
                parked_pc: 2,
                cflags: 0x4000_0000,
            },
        );
        assert!(w.start_prop_interaction(anchor));
        for _ in 0..4 {
            w.step_prop_interaction();
        }
        let ctx = &w.field_vm.channels[0].ctx;
        assert_eq!(ctx.field_72, 0, "the placement is hidden");
        assert_eq!(ctx.field_8e, 300, "the placement's height slot");
        assert_eq!(
            w.npcs.looks.get(LookKey::Npc(2)).angles,
            [0, 500, 0],
            "the placement's head turns"
        );
        assert_eq!(
            w.npcs.positions.get(&2).copied(),
            Some((10 * 128 + 64, 12 * 128 + 64)),
            "the placement stands on the player's spot"
        );
        assert_eq!(
            w.npcs.headings.get(&2).copied(),
            Some(0x0C00),
            "facing the player's way"
        );
    }
}
