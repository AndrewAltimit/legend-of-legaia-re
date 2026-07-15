//! Placed-prop touch / interact interactions: the door swing, the searchable
//! cupboard, and their collision-side effects.
//!
//! A placed field object's bind record is its field-VM script, and retail
//! runs it on the touched actor through the per-actor dialog SM
//! (`FUN_80039B7C` state 0 loops the dispatcher `FUN_801DE840` on
//! `actor[+0x90]`/`+0x9E`). The engine mirrors that by running the record
//! through the inline field-VM runner ([`crate::inline_dialogue`]) with a
//! **prop binding**: the executing context's `local_flags`/`flags`/`field_6a`
//! are the prop actor's `+0x62` / `+0x10` / `+0x6A`, synced into the
//! [`crate::field_env::PropAnimBank`] entry around every VM slice - so
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
//!   step and posts the touch - [`World::pending_prop_touch`] set by
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

use super::*;

/// Frames a prop run may stay parked on a waitable op (`2D 08`, `4A`, ...)
/// before it is force-completed - a safety net against a decode drift
/// soft-locking the player (the engaged flag suppresses locomotion while a
/// run is live). Generous: the longest retail door swing is under 40 frames.
const PROP_RUN_PARK_TIMEOUT: u32 = 1800;

impl World {
    /// Advance the placed-prop layer one field tick: step the clips, step an
    /// in-flight prop record run, and start a run for a movement touch posted
    /// by this tick's locomotion.
    pub fn tick_prop_interactions(&mut self) {
        // The per-actor anim tick runs unconditionally (`FUN_800204F8` from
        // the actor tick) - the windmill turns during dialogs too.
        self.field_prop_bank.tick_anims();
        self.step_prop_interaction();
        if let Some(anchor) = self.pending_prop_touch.take() {
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
        if self.inline_dialogue.is_some()
            || self.current_dialog.is_some()
            || self.cutscene_timeline_active()
        {
            return false;
        }
        let Some(prop) = self.field_prop_bank.props.get(&anchor) else {
            return false;
        };
        if prop.collision_exempt() {
            return false;
        }
        let mut runner = crate::inline_dialogue::InlineDialogue::new(
            std::sync::Arc::clone(&prop.record_body),
            prop.parked_pc,
        );
        runner.prop_anchor = Some(anchor);
        // The executing context IS the prop actor: seed its `+0x62` anim
        // word, `+0x10` flag word and `+0x6A` rate from the live prop.
        runner.ctx.local_flags = prop.anim.flags;
        runner.ctx.flags = prop.cflags;
        runner.ctx.field_6a = prop.anim.rate;
        self.inline_dialogue = Some(runner);
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
        for c in &self.field_prop_colliders {
            if !c.solid || !c.interact {
                continue;
            }
            let Some(anchor) = c.anchor else { continue };
            if !self.field_prop_bank.props.contains_key(&anchor) {
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
        use crate::input::PadButton;
        let is_prop_run = self
            .inline_dialogue
            .as_ref()
            .is_some_and(|id| id.prop_anchor.is_some());
        if !is_prop_run {
            return;
        }
        let confirm =
            self.input.just_pressed(PadButton::Cross) || self.input.just_pressed(PadButton::Circle);
        let up = self.input.just_pressed(PadButton::Up);
        let down = self.input.just_pressed(PadButton::Down);

        let Some(mut id) = self.inline_dialogue.take() else {
            return;
        };
        let anchor = id.prop_anchor.expect("checked above");

        // A box is open: tick the typewriter + route input. The prop's clip
        // keeps ticking underneath (the cupboard holds open at its clamp).
        if let Some(panel) = id.panel.as_mut() {
            if panel.menu_active() {
                if up {
                    panel.move_picker_cursor(-1);
                }
                if down {
                    panel.move_picker_cursor(1);
                }
            }
            panel.tick();
            if confirm {
                self.dialog_input_consumed = true;
                if panel.menu_active() {
                    let choice = panel.picker_cursor();
                    let target = panel.picker().and_then(|pk| pk.jump_target(choice));
                    id.last_choice = Some(choice);
                    id.visited.iter_mut().for_each(|v| *v = false);
                    match target {
                        Some(t) => id.pc = t,
                        None => id.done = true,
                    }
                    id.panel = None;
                } else if panel.is_waiting_for_input() || panel.is_done() {
                    // Dismissed: resume the record just past the segment -
                    // which for the cupboard is the close pass (`2B 07` set
                    // reverse, ...), so the doors swing shut as the window
                    // drops. Retail sequences it identically: the pager
                    // returns to SM state 0 and the script continues.
                    id.pc = panel.pc;
                    id.panel = None;
                }
            }
            if id.done {
                self.finish_prop_interaction(&mut id, anchor);
                return;
            }
            self.inline_dialogue = Some(id);
            return;
        }

        // No box: sync the prop's live state into the context (the per-frame
        // anim tick may have latched the clip's end since the last slice),
        // then run a VM slice.
        if let Some(prop) = self.field_prop_bank.props.get(&anchor) {
            id.ctx.local_flags = prop.anim.flags;
        }
        let mut parked = false;
        {
            let mut host = FieldHostImpl { world: self };
            let mut budget = crate::inline_dialogue::INLINE_DIALOGUE_STEP_BUDGET;
            while budget > 0 {
                budget -= 1;
                let b = id.bytecode.get(id.pc).copied().unwrap_or(0);
                if b & 0x7F < 0x20 {
                    if b == 0x1F {
                        // A text segment: open the real dialog panel here,
                        // with the record's name escapes resolved.
                        let mut panel = crate::dialog::OwnedDialogPanel::at_segment(
                            std::sync::Arc::clone(&id.bytecode),
                            id.pc,
                        );
                        panel.substitutions = host.world.dialog_substitutions(&id.bytecode);
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
                match vm::field::step(&mut host, &mut id.ctx, &id.bytecode, id.pc) {
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
        if parked {
            id.park_frames = id.park_frames.saturating_add(1);
            if id.park_frames > PROP_RUN_PARK_TIMEOUT {
                id.done = true;
            }
        } else {
            id.park_frames = 0;
        }
        if id.done {
            self.finish_prop_interaction(&mut id, anchor);
        } else {
            self.inline_dialogue = Some(id);
        }
    }

    /// Interaction teardown: write the resume cursor back onto the prop
    /// (retail parks `actor+0x9E` where the dispatcher stopped) and release
    /// the player (the dialog SM teardown clears the `0x80000` engaged flag).
    fn finish_prop_interaction(
        &mut self,
        id: &mut crate::inline_dialogue::InlineDialogue,
        anchor: (u8, u8),
    ) {
        self.sync_prop_from_ctx(anchor, &id.ctx);
        if let Some(prop) = self.field_prop_bank.props.get_mut(&anchor)
            && id.pc < prop.record_body.len()
        {
            prop.parked_pc = id.pc;
        }
        self.set_player_engaged(false);
        self.inline_dialogue = None;
    }

    /// Copy the executing context's actor words back onto the live prop and
    /// its collider row: `local_flags` -> `+0x62` (anim control), `field_6a`
    /// -> `+0x6A` (rate), `flags` -> `+0x10` - and when the script has set
    /// `+0x10 & 3` (`31 00`), drop the prop from the collision layer, exactly
    /// as `FUN_801CF754` / `FUN_801CF9F4` skip `flags & 3` actors from then
    /// on.
    fn sync_prop_from_ctx(&mut self, anchor: (u8, u8), ctx: &FieldCtx) {
        let Some(prop) = self.field_prop_bank.props.get_mut(&anchor) else {
            return;
        };
        prop.anim.flags = ctx.local_flags;
        if ctx.field_6a != 0 {
            prop.anim.rate = ctx.field_6a;
        }
        prop.cflags = ctx.flags;
        if prop.collision_exempt() {
            for c in &mut self.field_prop_colliders {
                if c.anchor == Some(anchor) {
                    c.solid = false;
                }
            }
        }
    }

    /// Set / clear the player's movement-disabled flag (`+0x10 & 0x80000`) -
    /// the engaged bit `FUN_801D5B5C` raises on the touch post and the dialog
    /// SM teardown clears.
    fn set_player_engaged(&mut self, engaged: bool) {
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

    /// Name-substitution table for a record's dialog escapes: every
    /// `0xC1`/`0xC2`/`0xC4` escape pair in `record` resolved against the
    /// engine's live tables (`0xC1 63` = the party leader, `0xC2 xx` = item
    /// name - the same tables retail's dialog renderer consults). `None`
    /// when the record carries no resolvable escape.
    fn dialog_substitutions(&self, record: &[u8]) -> Option<crate::dialog::PanelSubstitutions> {
        let mut map: std::collections::HashMap<(u8, u8), Vec<u8>> = Default::default();
        for w in record.windows(2) {
            let (esc, arg) = (w[0], w[1]);
            match esc {
                0xC1 => {
                    // `0xC1 99` = current party leader; other args index the
                    // roster order.
                    let name = if arg == 99 {
                        self.party_names.first()
                    } else {
                        self.party_names.get(arg as usize)
                    };
                    if let Some(name) = name {
                        map.entry((1, arg))
                            .or_insert_with(|| name.clone().into_bytes());
                    }
                }
                0xC2 | 0xC4 => {
                    if let Some(entry) = self.item_catalog.get(arg) {
                        map.entry((2, arg))
                            .or_insert_with(|| entry.name.as_bytes().to_vec());
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
