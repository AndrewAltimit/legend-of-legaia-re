//! Turning an entered Arts string into an action: art learning, the action-
//! queue build, art execution, the Spirit charge and the action arm.

use super::*;

impl World {
    /// `FUN_801EFBFC` over roster member `roster`'s record list
    /// (`+0x185` count, `+0x186..` ids): the membership scan, then the
    /// learn-on-use insert, for the art whose queue byte is `art_byte`. The
    /// record id is `art_byte - 0x1B` ([`ART_RECORD_ID_BIAS`]). `None` when
    /// the roster slot has no
    /// record.
    ///
    /// REF: FUN_801EFBFC (`0x801EFC54..0x801EFC98` scan, the insert after
    /// `0x801EFD2C`)
    pub(in crate::world) fn learn_art_on_record(
        &mut self,
        roster: u8,
        art_byte: u8,
    ) -> Option<legaia_engine_battle_vm::battle_action::ArtUseCheck> {
        let cap = self
            .party
            .tactical_arts
            .innate_cap(roster)
            .saturating_sub(ART_RECORD_ID_BIAS);
        let rec = self.party.roster.members.get_mut(usize::from(roster))?;
        let mut list = rec.displayed_skills();
        let verdict = legaia_engine_battle_vm::battle_action::check_and_learn_art(
            &mut list.count,
            &mut list.ids,
            art_byte.wrapping_sub(ART_RECORD_ID_BIAS),
            true,
            cap,
        );
        if verdict == legaia_engine_battle_vm::battle_action::ArtUseCheck::Learned {
            rec.set_displayed_skills(list);
        }
        Some(verdict)
    }

    /// Build the action queue retail's queue-builder `FUN_801EED1C` writes
    /// into `actor[+0x1DF..]` for an entered arrow string - byte-exact, not
    /// structural:
    ///
    /// 1. the tokenizer pass ([`legaia_art::tokenize`]): the arrows become
    ///    `0x0C..0x0F` swings, each matched art gets the `0x19` starter
    ///    written over its **last** arrow and its constant inserted after
    ///    it, leading arrows stay and arts overlap (`↑↓↑` -> `0F 0E 19 27`);
    /// 2. the learn-on-use check per accepted art (`FUN_801EFBFC`, `jal` at
    ///    `0x801EF44C`): a newly learned art's starter is `0x1A` instead
    ///    (`addiu v1,t3,0x18` with `t3 = 2`, `0x801EF6F0`);
    /// 3. the finish ([`vm::battle_action::finish_action_queue`]): the
    ///    Miracle replacement, the MSB-clear sweep and the Super
    ///    tail-replace, in that order.
    ///
    /// Returns the 19-byte stream window, the art constants it performs in
    /// order (the shout-cue list), and the builder's per-token side array
    /// `0x801F6990` - the marks the Attack x2 refill reads a whole action
    /// later, which is why they are carried onto the actor rather than
    /// dropped here. A character with no art catalog gets its arrows as plain
    /// swings, which is retail's own answer for an unmatched string.
    ///
    /// PORT: FUN_801EED1C (the player path: normalise + learn + finish;
    /// the tokenizer body is `legaia_art::tokenize`, the finish passes
    /// `legaia_engine_battle_vm::battle_action::queue_applier`)
    pub(in crate::world) fn build_arts_action_queue(
        &mut self,
        caster: u8,
        commands: &[legaia_art::Command],
    ) -> (
        [u8; legaia_engine_battle_vm::battle_action::ACTION_QUEUE_CAP],
        Vec<legaia_art::ActionConstant>,
        [u32; legaia_engine_battle_vm::battle_action::ACTION_QUEUE_CAP],
    ) {
        use legaia_art::ActionConstant;
        use legaia_engine_battle_vm::battle_action::ACTION_QUEUE_CAP;
        let roster = self.party_roster_slot(caster as usize) as u8;
        let character = self.caster_character(roster);
        // The character's art catalog in grid order (ascending constant),
        // the order the builder's inner loop walks (`s3 = 0xB..`).
        let mut catalog: Vec<(ActionConstant, Vec<legaia_art::Command>)> = self
            .tables
            .art_records
            .iter()
            .filter(|((ch, action), rec)| {
                // Only the **normal** arts (ordinal `>= 4`, constants
                // `0x1F+`): the builder's inner loop routes the Miracle Art
                // and the three Hyper Arts (ordinals `0..=3`) through a
                // different arm (`sltiu a1,a0,0x4` at `0x801EF330`) that,
                // with the slot's `+0x25F` marker clear, writes nothing -
                // their combo bytes never tokenize as arts.
                // ... and only combos of two arrows or more: a fully matched
                // one-arrow string takes the builder's `s1 == 1` exit
                // (`0x801EF420..0x801EF434`) with no rewrite - the disc's
                // one-arrow record is the Miracle finisher's, and letting it
                // match would steal an arrow from every art containing it.
                *ch == character
                    && rec.commands.len() >= 2
                    && action.as_byte() >= NORMAL_ART_MIN_CONSTANT
            })
            .map(|((_, action), rec)| (*action, rec.commands.clone()))
            .collect();
        catalog.sort_by_key(|(a, _)| a.as_byte());
        let entries: Vec<legaia_art::tokenize::ArtEntry<'_>> =
            catalog.iter().map(|(a, c)| (*a, c.as_slice())).collect();
        let tokens = legaia_art::tokenize(&entries, commands);
        log::debug!(
            "arts queue: {:?} over {:?} -> {:02x?}",
            commands,
            catalog
                .iter()
                .map(|(a, c)| (a.as_byte(), c.as_slice()))
                .collect::<Vec<_>>(),
            &tokens[..]
        );
        let mut bytes = [0u8; ACTION_QUEUE_CAP];
        bytes[..tokens.len()].copy_from_slice(&tokens);
        // Learn-on-use per accepted art, in the builder's own **tail-first**
        // order (`s8` from 15 down, `0x801EF848`, restarting at `s8 + 1`
        // after each match): when one art appears twice in a queue, the
        // *last* occurrence is the one `FUN_801EFBFC` sees first and so the
        // one that gets the `0x1A` newly-learned starter. The marked-starter
        // reorder in `finish_action_queue` then walks it back to the art's
        // first occurrence, which is what makes that pass load-bearing rather
        // than a no-op. Retail's insert gate (`ctx[+0x266 + slot]`) has no
        // engine analogue and reads open.
        //
        // The list the verdict scans is the character record's own -
        // count `+0x185`, ascending ids `+0x186..` (retail
        // `0x80084140 + (char - 1) * 0x414 + 0x74D`, `lbu v0,0x74d(v0)` at
        // `0x801EFC54`) - keyed by the art's queue byte **less `0x1B`**
        // ([`ART_RECORD_ID_BIAS`]), the id space the auto-fill arm and the
        // pause menu's arts pager read the same list in. A save
        // lifted from retail therefore knows every art its record lists, and
        // a known art takes the plain `0x19` starter.
        for i in (0..tokens.len().saturating_sub(1)).rev() {
            if bytes[i] != ActionConstant::RegularStarter.as_byte() {
                continue;
            }
            let Some(art) = ActionConstant::from_byte(bytes[i + 1]).filter(|a| a.is_art()) else {
                continue;
            };
            let id = art.as_byte();
            let verdict = self.learn_art_on_record(roster, id);
            match verdict {
                Some(legaia_engine_battle_vm::battle_action::ArtUseCheck::Learned) => {
                    bytes[i] = ActionConstant::SpecialStarter.as_byte();
                    self.notify_art_used(roster, id);
                }
                Some(_) => self.party.tactical_arts.mark_known(roster, id),
                // No character record behind the roster slot: the tracker
                // alone decides, as before the record was read.
                None => {
                    let known = self.party.tactical_arts.is_learned(roster, id);
                    self.notify_art_used(roster, id);
                    if !known && self.party.tactical_arts.is_learned(roster, id) {
                        bytes[i] = ActionConstant::SpecialStarter.as_byte();
                    }
                }
            }
        }
        let miracle_armed = self.miracle_marker_armed_for(roster);
        // The builder's special-trigger flag `0x801F696C` (cleared at its
        // head, set by the Miracle and Super arms) rides out on the context
        // for the strike loop's drift gate.
        let (marks, trigger) =
            legaia_engine_battle_vm::battle_action::finish_action_queue_with_trigger(
                character,
                commands,
                miracle_armed,
                &mut bytes,
            );
        self.battle_ctx.super_trigger = trigger;
        let actions: Vec<ActionConstant> = bytes
            .iter()
            .take_while(|&&b| b != 0)
            .filter_map(|&b| ActionConstant::from_byte(b))
            .filter(|a| a.is_art())
            .collect();
        (bytes, actions, marks)
    }

    /// Commit an entered Tactical-Arts string (`sequence` = the direction
    /// command bytes `1..=4`, the arts input's buffer or a saved chain's
    /// string) against the picked target: the arts screen's commit (`0x50`
    /// -> `0x5A` -> the ring walk). The entry executes at the caster's
    /// dispatch ([`Self::execute_battle_art`]), once every member has
    /// committed (retail `0x6E -> 0xFE`).
    pub(in crate::world) fn run_battle_art(
        &mut self,
        caster: u8,
        sequence: &[u8],
        target_row: legaia_engine_battle::target_picker::CursorRow,
        target_slot: u8,
    ) {
        if let Some(a) = self.actors.get_mut(caster as usize) {
            a.battle.action_category = 3;
            // The window as the gauge leaves it at the input confirm: one
            // swing byte `0x0C..=0x0F` per entered arrow (the direction table
            // `0x801F4B8C` reads `0C 0F 0E 0D` for Left / Up / Down / Right),
            // zero-terminated. Retail's write-back `FUN_801DA59C` runs here,
            // from `0x801D22BC` in the target confirm, **before** the queue
            // builder tokenizes anything at the dispatch - so the record holds
            // raw arrows, and every replay goes back through the builder.
            a.battle.params = [0; legaia_engine_battle_vm::battle_action::ACTION_PARAM_BYTES];
            let arrows = sequence
                .iter()
                .filter_map(|&b| legaia_art::Command::from_byte(b))
                .map(|c| c.as_action().as_byte());
            for (dst, src) in a
                .battle
                .params
                .iter_mut()
                .take(legaia_save::AUTO_COMMAND_STRING_LEN)
                .zip(arrows)
            {
                *dst = src;
            }
        }
        self.save_auto_command_string(caster);
        self.commit_party_command(
            caster,
            crate::battle_round::PendingPartyAction::Art {
                sequence: sequence.to_vec(),
                target_row,
                target_slot,
            },
        );
    }

    /// Execute a committed Tactical-Arts turn at the caster's dispatch:
    /// build retail's action queue ([`Self::build_arts_action_queue`]) and
    /// arm the action SM's attack band with it
    /// ([`Self::arm_battle_art_action`]). The band then stages the queue
    /// byte by byte - each swing, starter and art constant its own clip -
    /// and the hit-event driver resolves damage on each clip's own beats,
    /// so a three-arrow art is two swings and the art, paced by the clips,
    /// exactly as retail runs it.
    pub(super) fn execute_battle_art(
        &mut self,
        caster: u8,
        sequence: &[u8],
        target_row: legaia_engine_battle::target_picker::CursorRow,
        target_slot: u8,
    ) {
        let commands: Vec<legaia_art::Command> = sequence
            .iter()
            .filter_map(|&b| legaia_art::Command::from_byte(b))
            .collect();
        let (queue, actions, marks) = self.build_arts_action_queue(caster, &commands);
        self.charge_art_spirit(caster, &actions);
        self.arm_battle_art_action(caster, &queue, &actions, marks, target_row, target_slot);
    }

    /// Word 1 (`+0xF8`) of a **roster** member's 4-word accessory-passive
    /// ability bitfield - passive indices `0x20..=0x3F`.
    ///
    /// The battle-action host's `character_ability_bits_high` is the same
    /// word reached through a battle ordinal; this one takes the roster slot,
    /// which is what the arts paths already carry.
    pub(in crate::world) fn character_ability_bits_word1(&self, roster: u8) -> u32 {
        self.party
            .roster
            .members
            .get(roster as usize)
            .map(|m| {
                let b = m.ability_bits();
                u32::from_le_bytes([b[4], b[5], b[6], b[7]])
            })
            .unwrap_or(0)
    }

    /// Charge the turn's **art bodies** out of the caster's Spirit gauge
    /// (`actor[+0x170]`) - the second half of the two-gauge split. The
    /// direction commands are already paid, out of the entry pool
    /// (`ctx+0x6DC`, seeded from AGL); this is the price of the arts those
    /// directions matched, and until it was wired a turn's whole cost was its
    /// swings and an art body was free.
    ///
    /// The amount is [`legaia_engine_battle::ap_gauge::arts_turn_spirit_cost`] over the
    /// caster's builder-order catalog ([`legaia_engine_battle::battle_arts::spirit_catalog`])
    /// and the arts the queue actually performs. Retail accrues it into
    /// `actor[+0x224]` inside the builder and spends it once in the
    /// battle-action cleanup arm (`0x801E5D74`); the port charges it here, at
    /// the commit, which is the same turn and the same total.
    ///
    /// A plain attack charges nothing, because an unmatched arrow string
    /// performs no art and `actions` is empty.
    ///
    /// PORT: FUN_801EED1C (Spirit-cost half) / FUN_801E295C (the `+0x224` spend)
    pub(super) fn charge_art_spirit(&mut self, caster: u8, actions: &[legaia_art::ActionConstant]) {
        if actions.is_empty() {
            return;
        }
        let roster = self.party_roster_slot(caster as usize) as u8;
        let character = self.caster_character(roster);
        let catalog =
            legaia_engine_battle::battle_arts::spirit_catalog(&self.tables.art_records, character);
        // Retail's halving gate (`srl t4,t4,0x1` at `0x801EF378`) reads the
        // **character record's** `+0xF8` - word 1 of the accessory-passive
        // ability bitfield - and tests bit `0x800`, i.e. passive `0x2B`
        // "AP Used Down" (the Mettle Gem). See `ap_gauge::AP_USED_DOWN_BIT`
        // for the address arithmetic; it is not an actor flag and not a
        // per-turn condition, so it reads live off the equipped accessory.
        let halved = self.character_ability_bits_word1(roster)
            & legaia_engine_battle::ap_gauge::AP_USED_DOWN_BIT
            != 0;
        let cost = legaia_engine_battle::ap_gauge::arts_turn_spirit_cost(&catalog, actions, halved);
        if let Some(a) = self.actors.get_mut(caster as usize) {
            a.battle.spirit_gauge = a.battle.spirit_gauge.saturating_sub(cost);
        }
    }

    /// Stage a built action queue on the acting actor and arm the action
    /// SM's attack band for it: category `3`, the target, `Begin`. The
    /// stream is the queue **verbatim** (retail's `+0x1DF..` window), with
    /// the `0x00` terminator the band stops on guaranteed inside the
    /// stream. An empty queue still arms: the band reads its terminator on
    /// byte 0 and drops to recovery, consuming the turn - retail's own
    /// answer to an input with nothing in it.
    ///
    /// REF: FUN_801E295C (state `0x0C` ActionSeed, which calls the builder
    /// for the acting party slot and seeds category 3)
    pub(in crate::world) fn arm_battle_art_action(
        &mut self,
        caster: u8,
        queue: &[u8],
        actions: &[legaia_art::ActionConstant],
        starter_marks: [u32; legaia_engine_battle_vm::battle_action::ACTION_QUEUE_CAP],
        target_row: legaia_engine_battle::target_picker::CursorRow,
        target_slot: u8,
    ) {
        use legaia_engine_battle::target_picker::CursorRow;
        let party_count = self.party.party_count.clamp(1, 3);
        let target = match target_row {
            CursorRow::Enemy => party_count + target_slot,
            CursorRow::Ally => target_slot,
        };
        if usize::from(target) >= self.actors.len() {
            return;
        }
        // The **roster**-slot keying the queue was built under, not the
        // battle ordinal: the hit-event driver looks the art record up by
        // this key, so the two have to agree or a three-member party reads
        // the wrong character's table.
        let char_slot = self.party_roster_slot(caster as usize) as u8;
        let character = self.caster_character(char_slot);
        self.push_art_shout_cues(caster, actions);
        // A freshly-armed action starts from an empty strike script - see
        // `World::clear_action_stream` for the soft-lock a carried-over byte
        // produces.
        self.clear_action_stream(caster);
        let Some(a) = self.actors.get_mut(caster as usize) else {
            return;
        };
        let n = queue.len().min(a.battle.params.len().saturating_sub(1));
        a.battle.params[..n].copy_from_slice(&queue[..n]);
        a.battle.params[n] = 0;
        // The builder's side array travels with the queue it describes:
        // retail's `0x801F6990` is still standing when the Attack x2 refill
        // reads it, and only it can tell a Super's `0x1A` starter from a
        // newly-learned art's.
        a.battle.starter_marks = Some(starter_marks);
        a.battle.strike_index = 0;
        a.battle.active_target = target;
        a.battle.action_category = 3;
        // The art-record lookup key. Nothing else in the port writes it, so
        // an unset slot would resolve every character's arts against Vahn's
        // table.
        a.battle.character = character;
        a.battle.chosen_art = None;
        self.battle_ctx.active_actor = caster;
        self.battle_ctx.queued_action = 3;
        self.battle_ctx.action_state =
            legaia_engine_battle_vm::battle_action::ActionState::Begin.as_byte();
    }
}
