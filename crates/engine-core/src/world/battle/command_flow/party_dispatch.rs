//! Dispatch of a committed party action into the battle loop, with its art
//! shout cues.

use super::*;

impl World {
    /// Dispatch the command `actor` committed this round - the engine's
    /// counterpart of the action SM's `0x0C` seed for a party slot, run when
    /// the initiative pick lands on the member. Everything the old commit
    /// sites armed on the spot is armed here instead: the swing stream
    /// (`FUN_801EED1C`), the art profile, the cast, the item effect + its
    /// cast band, the Spirit charge, the escape roll + run band.
    ///
    /// REF: FUN_801E295C (state `0x0C`, the party-slot `jal 0x801EED1C`)
    /// REF: FUN_801EED1C
    pub(in crate::world) fn dispatch_pending_party_action(
        &mut self,
        actor: u8,
        action: crate::battle_round::PendingPartyAction,
    ) {
        use crate::battle_round::PendingPartyAction as Pending;
        use legaia_engine_battle_vm::battle_action::ActionState;
        self.battle_ctx.active_actor = actor;
        match action {
            Pending::Attack { target } => {
                // A freshly-armed action starts from an empty strike script -
                // see [`World::clear_action_stream`] for the soft-lock a
                // carried-over byte produces.
                self.clear_action_stream(actor);
                if let Some(a) = self.actors.get_mut(actor as usize) {
                    a.battle.active_target = target;
                    a.battle.action_category = 3; // Attack
                }
                // ... and then seeds it, which is what makes the attack band's
                // strike loop a loop instead of an immediate exit.
                //
                // Retail seeds it twice over: the command SM preseeds the
                // window from the character's own saved **auto command
                // string** at the Attack confirm (`FUN_801DA34C` from
                // `0x801D15C8`), and the queue builder's no-directional-input
                // arm rolls a fresh two-swing stream only when that string is
                // empty. The port keeps the same order - a character that has
                // confirmed an arts combo replays it, one that has not takes
                // the roll.
                //
                // An **Auto** pick replaces both: the round-start pool arm of
                // `FUN_801F0450` rebuilt this seat's queue from the direction
                // commands and the learned arts (`super::super::auto_combo`), and
                // that is what the round plays.
                if let Some(queue) = self.take_auto_attack_queue(actor) {
                    if let Some(a) = self.actors.get_mut(actor as usize) {
                        a.battle.params =
                            [0; legaia_engine_battle_vm::battle_action::ACTION_PARAM_BYTES];
                        for (dst, src) in a.battle.params.iter_mut().zip(queue.iter()) {
                            *dst = *src;
                        }
                        a.battle.strike_index = 0;
                    }
                } else {
                    // The preseeded string is raw arrows, which retail's
                    // builder tokenizes at this dispatch like any entered
                    // string - arts, learn-on-use and the Spirit cost
                    // included.
                    let arrows = self.preseed_arts_entry_string(actor);
                    let commands: Vec<legaia_art::Command> = arrows
                        .iter()
                        .filter_map(|&b| legaia_art::Command::from_byte(b))
                        .collect();
                    if commands.is_empty() {
                        self.seed_basic_attack_queue(actor, target);
                    } else {
                        let (queue, actions, marks) =
                            self.build_arts_action_queue(actor, &commands);
                        self.charge_art_spirit(actor, &actions);
                        let party_count = self.party.party_count.clamp(1, 3);
                        let (row, slot) = if target >= party_count {
                            (
                                legaia_engine_battle::target_picker::CursorRow::Enemy,
                                target - party_count,
                            )
                        } else {
                            (legaia_engine_battle::target_picker::CursorRow::Ally, target)
                        };
                        self.arm_battle_art_action(actor, &queue, &actions, marks, row, slot);
                    }
                }
                self.battle_ctx.queued_action = 3;
                self.battle_ctx.action_state = ActionState::Begin.as_byte();
            }
            Pending::Art {
                sequence,
                target_row,
                target_slot,
            } => self.execute_battle_art(actor, &sequence, target_row, target_slot),
            Pending::Spell {
                spell_id,
                target_row,
                target_slot,
            } => {
                // The dispatch commits the category-2 action; the action SM's
                // Magic band carries it from here - facing, the MP debit and
                // the `0x14`-frame wait at `0x28`/`0x29`, the summon band for
                // a Seru id - and the outcome folds at retail's seam
                // (`World::settle_cast_band` / the stager's strike). An
                // escape spell's success ends the encounter from the live
                // loop the frame it folds, through the escape teardown.
                // Retail resolves any id off the spell table, a capture-class
                // special from a party seat included (a probe-injected `0x56`
                // runs PROT 0946 from seat 0), so the party seam takes the
                // same disc fallback the monster seam does.
                match self.monster_cast_def(spell_id) {
                    Some(def) => {
                        let targets = self.spell_targets_for(&def, target_row, target_slot);
                        self.arm_player_cast(actor, &def, targets);
                    }
                    None => {
                        // Not a catalog spell (the submenu only lists catalog
                        // ids, so this is defensive): the turn is spent.
                        self.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
                        self.cycle_battle_turn();
                    }
                }
            }
            Pending::Item {
                item_id,
                used_slots,
            } => {
                // Apply to every affected slot (the copy went at the commit).
                for &target_slot in &used_slots {
                    let outcome = self.apply_battle_item(item_id, target_slot);
                    self.push_item_use_fx(target_slot, outcome);
                }
                if self.battle.escaped {
                    // Escape item succeeded: leave the encounter (no loot, no
                    // game-over) through the escape teardown's fade + exit
                    // hold instead of cycling the turn.
                    self.battle.end = Some(BattleEndCause::Escaped);
                    self.begin_battle_end_sequence();
                    return;
                }
                // Using an item is the actor's whole turn - and in retail the
                // turn *is* the action SM's Item band: the committed
                // category-1 action seeds through `FUN_801E295C`'s item arm
                // (`item_seed_band`) into the `0x3C..0x40` cast states, which
                // fire the cast-audio cue (`FUN_801F3990` via `spirit_wait`),
                // stamp the item's effect-descriptor `(class, tier)` pair, and
                // expand the item's cue group (`FUN_800402F4`'s eleven
                // `FUN_801E22C8` sites via `place_cue_group`). The simulation
                // fold stays above; the band's own `apply_damage` hook is the
                // presentation seam. The two SummonFlute ids (`0x98`/`0x99`)
                // reroute inside `action_seed` to the summon band, which the
                // live loop's settle glue (`live_battle_tick`) walks to
                // completion.
                let target = if used_slots.len() > 1 {
                    legaia_engine_battle_vm::battle_cue_group::TARGET_PARTY_WIDE
                } else {
                    used_slots.first().copied().unwrap_or(actor)
                };
                self.clear_action_stream(actor);
                if let Some(a) = self.actors.get_mut(actor as usize) {
                    a.battle.active_target = target;
                    a.battle.action_category =
                        legaia_engine_battle_vm::battle_action::ActionCategory::Item.as_byte();
                    a.battle.params[0] = item_id;
                }
                self.battle_ctx.queued_action =
                    legaia_engine_battle_vm::battle_action::ActionCategory::Item.as_byte();
                self.battle_ctx.action_state = ActionState::Begin.as_byte();
            }
            Pending::Spirit => {
                // The AP charge (+5, idempotent per turn - the retail
                // Square-press kernel). The guard stance has been up since
                // the commit.
                if let Some(gauge) = self.battle.ap_gauges.get_mut(actor as usize) {
                    gauge.charge_spirit();
                }
                if let Some(guard) = self.battle.guarding.get_mut(actor as usize) {
                    *guard = true;
                }
                // Then the action SM plays the turn: the seed sends category
                // `4` to the spirit band unconditionally (`li v0,0x46` at
                // `0x801E2F5C`), which stages the committed spirit clip and
                // holds on it (`0x46..=0x48`) before the Done band. Ending the
                // action here instead skipped the whole band - the charge
                // pose, its hold and the round's pacing.
                if let Some(a) = self.actors.get_mut(actor as usize) {
                    a.battle.action_category = 4;
                    if a.battle.queued_anim_b == 0 {
                        a.battle.queued_anim_b = SPIRIT_COMMIT_CLIP;
                    }
                }
                self.battle_ctx.queued_action = 4;
                self.battle_ctx.action_state = ActionState::Begin.as_byte();
            }
            Pending::Run => {
                // Roll the escape and arm the action SM's run band (category
                // 5 -> RunBegin/RunWait/RunEscape, retail 0x64..0x66). The SM
                // carries the roll outcome on `absorbed_seru` (success
                // floors downed party HP at 1 and tears the battle down
                // `Escaped`; failure consumes the turn via the Done band).
                // The roll is the retail `FUN_801E791C` formula (the writer of
                // `_DAT_8007726C`): party SPD*1.5 + missing-HP/16 vs enemy SPD
                // + missing-HP/32, two rand draws, Chicken Heart/King accessory
                // bits folded from the living party members' second ability
                // word. Retail rolls it inside the run band (`0x801E57C8`),
                // once per party member that dispatches with category 5.
                let escaped = self.roll_battle_escape();
                if escaped {
                    self.stage_party_flee();
                }
                if let Some(a) = self.actors.get_mut(actor as usize) {
                    a.battle.action_category = 5; // Run band
                }
                self.battle_ctx.queued_action = 5;
                self.battle_ctx.absorbed_seru = u8::from(escaped);
                self.battle_ctx.action_state = ActionState::Begin.as_byte();
            }
            Pending::StandBy => {
                // Category 0 dispatches straight to the Done band.
                self.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
                self.cycle_battle_turn();
            }
        }
    }

    /// Arts-voice shout: one cue **per art the turn performs**, on that
    /// art's animation-start frame, when the art carries a real action
    /// constant (a synthetic/demo art has none and stays silent - the
    /// retail degradation for arts with no cue-table entry). Retail
    /// stages each art's animation separately and the materialiser
    /// (`FUN_8004AD80`) calls the cue selector per staging, so an entry
    /// that performs three arts requests three shouts; the mixer queues
    /// a back-to-back request behind the sounding one rather than cutting
    /// it. The host resolves each (character, action) pair against the
    /// arts-voice tables + XA clip banks and plays the CD-XA shout with the
    /// modeled CD-response delay, so the audio trails this frame rather than
    /// leading it. REF: FUN_8004C140.
    pub(super) fn push_art_shout_cues(
        &mut self,
        caster: u8,
        actions: &[legaia_art::ActionConstant],
    ) {
        let character = self.caster_character(caster);
        let cslot = legaia_art::Character::all()
            .iter()
            .position(|c| *c == character)
            .unwrap_or(usize::MAX);
        if cslot >= 3 {
            return;
        }
        for action in actions {
            self.audio.battle_shout_cues.push(
                legaia_engine_battle::battle_events::BattleShoutCue {
                    cslot: cslot as u8,
                    action: action.as_byte(),
                },
            );
        }
    }
}
