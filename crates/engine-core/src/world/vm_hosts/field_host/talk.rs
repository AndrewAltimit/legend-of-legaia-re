//! Field-host bodies for the talk surface: the three-actor talk setup, dialog
//! open and dialog advance.

use super::*;

impl FieldHostImpl<'_> {
    // Op-0x43 sub-2 three-actor talk setup - the talk-controller spawn.
    // Spec: `overlay_cutscene_dialogue_801d2d38.txt` (the 0897 static copy
    // of this VA is a garbled mid-function fragment). Gate = system flag
    // `0xD`, the talk-active lock this same function (re)sets on exit:
    //
    // - First arm (flag clear): retail collapses the story party list to
    //   its leader (count `0x80084594` = 1, ids `0x80084598..` =
    //   `[leader, 0, 0, 0]` from the leader byte `0x80084597`), clears the
    //   per-character talk flags `0x10/0x11/0x12`, sets flag
    //   `0x10 + leader`, and zeroes the dialog-busy byte `DAT_8007B648`
    //   (the engine's busy signal is `current_dialog`, left as-is). The
    //   fresh controller's SM state 0 (`FUN_801D27E0`) then captures the
    //   three participants' positions/headings into `0x800845E4` -
    //   mirrored here at arm time into the session record.
    // - Re-arm (flag set): restore the three participants' saved positions
    //   + headings from the table - retail's else-branch loop pairs saved
    //   record `i` with the new controller's actor `i`.
    //
    // Retail's controller countdown `+0x72` subtracts the scene-MAN header
    // pair (`FUN_8003D064(_DAT_8007B898 + 0x22)`) from `arg_byte`; the
    // session keeps the raw operand (no MAN header staged on this path).
    // PORT: FUN_801D2D38
    // REF: FUN_801D27E0 (controller SM; state 0 = the position capture)
    // REF: FUN_8003C83C (id resolve), FUN_8003D064 (MAN-header pair read)
    pub(super) fn stage_three_actor_talk(
        &mut self,
        actor_ids: [u8; 3],
        arg_word: u16,
        arg_byte: u8,
    ) {
        // Instruction ids resolve through the actor-list walk (retail
        // `FUN_8003C83C`) to the placement slots the engine's field-NPC
        // state is keyed by; an unmatched id passes through raw (tests /
        // channel-less scenes).
        fn resolve(world: &World, id: u8) -> u8 {
            legaia_engine_field::field_channels::resolve_target(&world.field_vm.channels, id)
                .map(|ci| world.field_vm.channels[ci].placement_index as u8)
                .unwrap_or(id)
        }
        let w = &mut *self.world;
        let (saved, saved_party, saved_party_len, saved_leader) = if !w.system_flag_test(0xD) {
            // First arm: snapshot the pre-collapse story party (the
            // engine-side record `World::end_three_actor_talk` restores when
            // the talk lock drops - retail's post-talk membership comes from
            // the script's own party ops), then collapse it to its leader.
            let mut saved_party = [None; 4];
            let saved_party_len = w.party.party_actor_slots.len().min(4) as u8;
            for (dst, src) in saved_party
                .iter_mut()
                .zip(w.party.party_actor_slots.iter().copied())
            {
                *dst = src;
            }
            let saved_leader = w.party.party_leader_slot;
            let leader = w.field_leader();
            w.party.party_actor_slots = vec![Some(leader)];
            w.party.party_leader_slot = Some(leader);
            w.system_flag_clear(0x10);
            w.system_flag_clear(0x11);
            w.system_flag_clear(0x12);
            w.system_flag_set(0x10 + u16::from(leader));
            // Capture the participants' live positions for the paired
            // restore (retail: controller SM state 0).
            let saved = actor_ids.map(|id| {
                let slot = resolve(w, id);
                w.npcs
                    .positions
                    .get(&slot)
                    .map(|&pos| (pos, w.npcs.heading(slot)))
            });
            (saved, saved_party, saved_party_len, saved_leader)
        } else {
            // Re-arm during an active talk: restore saved positions onto
            // this instruction's participants, positionally. The party
            // snapshot carries over unchanged - retail's else branch never
            // re-collapses (`FUN_801D2D38` `801d2dd4` skips the count/ids
            // stores when the flag is up).
            let prior = w
                .dialog
                .three_actor_talk
                .as_ref()
                .copied()
                .unwrap_or_default();
            for (i, &id) in actor_ids.iter().enumerate() {
                if let Some((pos, heading)) = prior.saved[i] {
                    let slot = resolve(w, id);
                    w.npcs.positions.insert(slot, pos);
                    w.npcs.headings.insert(slot, heading);
                }
            }
            (
                prior.saved,
                prior.saved_party,
                prior.saved_party_len,
                prior.saved_leader,
            )
        };
        w.system_flag_set(0xD);
        w.dialog.three_actor_talk = Some(ThreeActorTalk {
            actor_ids,
            script_id: arg_word,
            duration: arg_byte,
            saved,
            saved_party,
            saved_party_len,
            saved_leader,
            // Every (re)arm spawns a fresh controller in retail, so the SM
            // starts at state 0; its presence-flag base is the instruction's
            // u16 (controller `+0x50`).
            swap: legaia_engine_field::leader_swap::LeaderSwap {
                phase: 0,
                counter: 0,
                flag_base: arg_word,
            },
        });
    }

    pub(super) fn open_dialog(
        &mut self,
        text_id: u16,
        inline: &[u8],
        world_x: u16,
        world_z: u16,
        depth_id: u8,
    ) {
        let inline_vec = inline.to_vec();
        self.world.dialog.current = Some(DialogRequest {
            text_id,
            inline: inline_vec.clone(),
            world_x,
            world_z,
            depth_id,
        });
        self.world
            .pending_field_events
            .push(FieldEvent::OpenDialog {
                text_id,
                inline: inline_vec,
                world_x,
                world_z,
                depth_id,
            });
    }

    /// Field-VM op 0x4C n5 sub-4 - dialog-advance poll.
    ///
    /// The retail dispatcher calls `FUN_801D65D8(0)` (dialog "advance one
    /// frame" query); a non-zero return halts the VM at `pc`, a zero
    /// return advances `pc += 2`. Our world tracks dialog activity via
    /// `current_dialog` (cleared by the engine after the user dismisses
    /// the box). When a dismiss button (Cross / Circle) was just-pressed
    /// this frame, drop the dialog request inline so the VM transitions
    /// without the host having to round-trip another event.
    ///
    /// Returns `true` while a dialog is showing and the user has *not*
    /// dismissed it this frame. Returns `false` when there's no active
    /// dialog or when the dismiss button just fired (clears the request
    /// and unblocks the VM in one step).
    pub(super) fn op4c_n5_sub4_dialog_advance(&mut self, _ctx: &mut FieldCtx) -> bool {
        // When the inline-script field-VM runner owns the box, it advances /
        // dismisses it (see `World::drive_inline_dialogue`); the simplified
        // dialog-advance must not clear the box out from under it.
        if self.world.dialog.current.is_none() || self.world.dialog.inline.is_some() {
            return false;
        }
        // A carrier's spar menu owns the dialog input while it is up: navigate +
        // confirm the fight option (engages only then), vs the any-accept path.
        if self.world.carriers.menu.is_some() {
            self.world.handle_carrier_menu();
            return self.world.dialog.current.is_some();
        }
        let dismissed = (self.world.input.just_pressed(input::PadButton::Cross)
            || self.world.input.just_pressed(input::PadButton::Circle))
            && !self.world.dialog.input_consumed;
        if dismissed {
            self.world.dialog.input_consumed = true;
            self.world.dialog.current = None;
            self.world
                .pending_field_events
                .push(FieldEvent::DialogDismissed);
            // Accepting a scripted-encounter carrier's prompt engages it: this is
            // the dialogue-accept that advances the carrier SM (`FUN_801DA51C`)
            // to its scene-transition. The battle launches on the next
            // `tick_field_carriers`. (The tutorial fight is forced, so any
            // dismiss is the accept; the undecoded Yes/No box-selection logic
            // would gate this once pinned.)
            if let Some(idx) = self.world.carriers.pending_engage.take() {
                self.world.engage_field_carrier(idx);
            }
            return false;
        }
        true
    }
}
