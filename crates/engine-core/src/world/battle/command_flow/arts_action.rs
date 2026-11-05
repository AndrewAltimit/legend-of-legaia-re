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
        seat: u8,
        art_byte: u8,
    ) -> Option<legaia_engine_battle_vm::battle_action::ArtUseCheck> {
        // Auto rarely teaches: a seat on its flag `ctx[+0x266 + seat]` draws
        // `rand() & 0x1FF` on every art it plays, known or not, and only a zero
        // lets it learn one. Tetsu's spar teaches regardless.
        // PORT: FUN_801EFBFC (the learn gate, `ctx[+0x266 + seat]`, `DAT_8007BD0C`)
        let auto = self
            .battle
            .auto_combo
            .flags
            .get(usize::from(seat))
            .copied()
            .unwrap_or(false);
        let learn_gate_open =
            !auto || self.next_rand() & 0x1FF == 0 || self.battle_first_monster_byte() == 0x4F;
        let cap = self.art_innate_cap(roster);
        let rec = self.party.roster.members.get_mut(usize::from(roster))?;
        let mut list = rec.displayed_skills();
        let verdict = legaia_engine_battle_vm::battle_action::check_and_learn_art(
            &mut list.count,
            &mut list.ids,
            art_byte.wrapping_sub(ART_RECORD_ID_BIAS),
            learn_gate_open,
            cap,
        );
        if verdict == legaia_engine_battle_vm::battle_action::ArtUseCheck::Learned {
            rec.set_displayed_skills(list);
        }
        Some(verdict)
    }

    /// The innate cap `FUN_801EFBFC` compares an art's record id against
    /// (`0x801F686C + char_id - 1`): the overlay's table once
    /// [`crate::world::DiscTables::art_innate_caps`] is installed, else
    /// whatever a caller gave the tracker.
    ///
    /// REF: FUN_801EFBFC (`0x801EFD0C..0x801EFD18`)
    pub(in crate::world) fn art_innate_cap(&self, roster: u8) -> u8 {
        match self.tables.art_innate_caps {
            Some(caps) => caps.get(usize::from(roster)).copied().unwrap_or(0),
            None => self
                .party
                .tactical_arts
                .innate_cap(roster)
                .saturating_sub(ART_RECORD_ID_BIAS),
        }
    }

    /// Build the action queue retail's queue-builder `FUN_801EED1C` writes
    /// into `actor[+0x1DF..]` for an entered arrow string - byte-exact, not
    /// structural:
    ///
    /// 1. the bank walk ([`legaia_art::tokenize_bank`]): the arrows become
    ///    `0x0C..0x0F` swings, each matched normal art gets the `0x19`
    ///    starter written over its **last** arrow and its constant inserted
    ///    after it, leading arrows stay and arts overlap (`↑↓↑` ->
    ///    `0F 0E 19 27`); a Hyper Art with the Ra-Seru marker up collapses
    ///    to `0x1A` and its constant (Lightning Storm -> `1A 1D`);
    /// 2. the walk's gate on every full match: Spirit against the art's price,
    ///    then the learn-on-use check (`FUN_801EFBFC`, `jal` at `0x801EF44C`),
    ///    whose verdict drops the run, or writes the `0x1A` starter of a newly
    ///    learned art (`addiu v1,t3,0x18` with `t3 = 2`, `0x801EF6F0`);
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
    /// the tokenizer body is `legaia_art::tokenize_bank`, the finish passes
    /// `legaia_engine_battle_vm::battle_action::queue_applier`)
    pub fn build_arts_action_queue(
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
        let miracle_armed = self.miracle_marker_armed_for(roster);
        // The character's art bank in walk order, record `0x1B` first (the
        // builder's inner loop starts at `s3 = 0xB`, `0x801EF2EC`): every
        // group takes its arm by its ordinal - the Miracle Art and the three
        // Hyper Arts the special one (`sltiu a1,a0,0x4` at `0x801EF330`),
        // gated on the slot's `+0x25F` Ra-Seru marker, the rest the normal
        // rewrite.
        let bank = self.art_bank_rows(character);
        let records = legaia_art::bank_view(&bank);
        // A rotted limb's swings leave the string before the walk reads it
        // (`0x801EEF44..0x801EF0A8`): status may have landed since the entry.
        let status = self.raw_status_word(caster);
        let commands: Vec<legaia_art::Command> = commands
            .iter()
            .copied()
            .filter(|&c| !legaia_engine_battle::arts_command_input::rot_blocks(status, c))
            .collect();
        let commands = &commands[..];
        let mut q = [0u8; legaia_art::QUEUE_LEN];
        for (dst, c) in q.iter_mut().zip(commands) {
            *dst = c.as_action().as_byte();
        }
        // The gate of the walk, in the builder's order. Spirit first: the run's
        // price against the gauge as earlier runs have left it
        // (`0x801EF40C..0x801EF41C`); the walk debits as it goes and the tail
        // restores it (`0x801EF988`), so only the accumulator `+0x224`, a byte,
        // leaves the builder. Then the learn check (`jal 0x801EFBFC`), whose `0`
        // drops the run and whose `1` / `2` name the starter
        // (`addiu v1,t3,0x18` at `0x801EF6F0`).
        let halved = self.character_ability_bits_word1(roster)
            & legaia_engine_battle::ap_gauge::AP_USED_DOWN_BIT
            != 0;
        let gauge = self
            .actors
            .get(usize::from(caster))
            .map_or(0, |a| a.battle.spirit_gauge);
        let mut spirit = gauge;
        let mut owed = 0u8;
        let groups = legaia_art::bank_group_starts(&records);
        let miracle_row = legaia_engine_battle_vm::battle_action::miracle_row_for(character);
        let mut built = legaia_art::tokenize_bank_gated(
            &records,
            q,
            [0u32; legaia_art::QUEUE_LEN],
            miracle_armed,
            Some(&miracle_row),
            |ordinal, len| {
                let cost = legaia_engine_battle::ap_gauge::art_spirit_cost(
                    usize::from(ordinal),
                    len.min(0xFF) as u8,
                    halved,
                );
                if (spirit as i16) < cost as i16 {
                    return legaia_art::ArtRun::Refuse;
                }
                let id = 0x1B + groups.get(usize::from(ordinal)).copied().unwrap_or(0);
                let starter = match self.learn_art_on_record(roster, caster, id) {
                    Some(legaia_engine_battle_vm::battle_action::ArtUseCheck::Known) => {
                        self.party.tactical_arts.mark_known(roster, id);
                        ActionConstant::RegularStarter.as_byte()
                    }
                    Some(legaia_engine_battle_vm::battle_action::ArtUseCheck::Learned) => {
                        self.notify_art_used(roster, id);
                        ActionConstant::SpecialStarter.as_byte()
                    }
                    Some(_) => return legaia_art::ArtRun::Drop,
                    // No character record behind the roster slot: the tracker
                    // alone decides, as before the record was read.
                    None => {
                        let known = self.party.tactical_arts.is_learned(roster, id);
                        self.notify_art_used(roster, id);
                        if !known && self.party.tactical_arts.is_learned(roster, id) {
                            ActionConstant::SpecialStarter.as_byte()
                        } else {
                            ActionConstant::RegularStarter.as_byte()
                        }
                    }
                };
                spirit = spirit.wrapping_sub(cost);
                owed = owed.wrapping_add(cost as u8);
                legaia_art::ArtRun::Take { starter }
            },
        );
        // The Miracle arm owes four more under AP Used Down
        // (`0x801EF4F8..0x801EF52C`), through both words.
        if built.miracle_matched && halved {
            owed = owed.wrapping_add(4);
        }
        self.battle.art_turn_spirit = owed;
        // The per-seat Miracle mark the starter commit's banner and voice read:
        // cleared at the builder's head (`0x801EED90`), raised by the Miracle
        // arm (`0x801EF5A8`).
        if let Some(flag) = self
            .battle_ctx
            .arts_banner_seat_flags
            .get_mut(usize::from(caster))
        {
            *flag = u8::from(built.miracle_matched);
        }
        // An expansion pack's known arts walk second, over the arrows the
        // retail walk left (`legaia_engine_expansion::arts`): their rows sit
        // past retail's 24, every retail row is blank in this pass, so a
        // retail art matches exactly as without them - and the retail
        // bank's one-arrow Super stubs, which end its walk at any start that
        // finds their arrow, cannot hide them. No pack art known: no pass.
        let extra = self.expansion_bank_rows(character);
        if !extra.is_empty() {
            let mut pack_bank: Vec<legaia_art::BankRow> = Vec::new();
            for (b, cmds, sources) in extra {
                let k = usize::from(b - 0x1B);
                if pack_bank.len() <= k {
                    pack_bank.resize(k + 1, (Vec::new(), [0; 4]));
                }
                pack_bank[k] = (cmds, sources);
            }
            let pack_records = legaia_art::bank_view(&pack_bank);
            let over =
                legaia_art::tokenize_bank_over(&pack_records, built.queue, built.marks, false);
            built.queue = over.queue;
            built.marks = over.marks;
        }
        let tokens = built.queue;
        log::debug!(
            "arts queue: {:?} (Ra-Seru {miracle_armed}) -> {:02x?}",
            commands,
            &tokens[..]
        );
        let mut bytes = [0u8; ACTION_QUEUE_CAP];
        bytes[..tokens.len()].copy_from_slice(&tokens);
        // The build loop's own side array: a normal art's starter carries
        // `1`, a Hyper Art's none.
        let mut marks = [0u32; ACTION_QUEUE_CAP];
        marks[..built.marks.len()].copy_from_slice(&built.marks);
        // The builder's special-trigger flag `0x801F696C` (cleared at its
        // head, set by the Miracle and Super arms) rides out on the context
        // for the strike loop's drift gate.
        let (marks, trigger) =
            legaia_engine_battle_vm::battle_action::finish_action_queue_with_marks(
                character, commands, false, &mut bytes, marks,
            );
        self.battle_ctx.super_trigger = trigger || built.miracle_matched;
        // The pack arts' further segments and chains, after the retail finish.
        let mut marks = marks;
        self.expansion.arts.turn_performed =
            self.expansion_finish_arts_queue(character, &mut bytes, &mut marks);
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
            legaia_engine_battle_loop::round_flow::PendingPartyAction::Art {
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
        self.arm_battle_art_action(caster, &queue, marks, target_row, target_slot);
    }

    /// Word 1 (`+0xF8`) of a **roster** member's 4-word accessory-passive
    /// ability bitfield - passive indices `0x20..=0x3F`.
    ///
    /// The battle-action host's `character_ability_bits_high` is the same
    /// word reached through a battle ordinal; this one takes the roster slot,
    /// which is what the arts paths already carry.
    pub fn character_ability_bits_word1(&self, roster: u8) -> u32 {
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
    /// The amount is what the builder's walk filled,
    /// [`legaia_engine_battle_loop::battle_state::BattleState::art_turn_spirit`]. It
    /// goes into the accumulator `actor[+0x224]` here, at the dispatch, and the
    /// gauge pays it in the battle-action cleanup arm (`0x801E5D74`,
    /// `battle_action::done`), so the AP plate holds its figure through the art.
    ///
    /// A plain attack charges nothing, because an unmatched arrow string
    /// fills nothing.
    ///
    /// PORT: FUN_801EED1C (Spirit-cost half) / FUN_801E295C (the `+0x224` spend)
    pub fn charge_art_spirit(&mut self, caster: u8, actions: &[legaia_art::ActionConstant]) {
        let performed = std::mem::take(&mut self.expansion.arts.turn_performed);
        self.expansion_charge_pack_arts(caster, &performed);
        let _ = actions;
        let owed = std::mem::take(&mut self.battle.art_turn_spirit);
        if let Some(a) = self.actors.get_mut(caster as usize) {
            // The walk adds to the byte (`sb v0,0x224(v1)` at `0x801EF4B4`): a
            // price an action never paid, its member felled mid-string, stays owed.
            a.battle.action_recoil = a.battle.action_recoil.wrapping_add(owed);
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

impl World {
    /// `character`'s art bank as the queue builder walks it: one row per
    /// constant from `0x1B` up to the highest installed record, its combo and
    /// its `+0x0A..+0x0D` chain bytes ([`legaia_art::ArtRecord::anim_extra`]).
    /// A constant with no installed record is an empty row, which matches
    /// nothing but still counts as a group.
    pub fn art_bank_rows(&self, character: legaia_art::Character) -> Vec<legaia_art::BankRow> {
        legaia_art::bank_rows(
            self.tables
                .art_records
                .iter()
                .filter(|((ch, _), _)| *ch == character)
                .map(|((_, a), r)| (a.as_byte(), r.commands.clone(), chain_bytes(r))),
        )
    }
}

/// An installed record's `+0x0A..+0x0D` chain bytes
/// ([`legaia_art::ArtRecord::anim_extra`]).
fn chain_bytes(r: &legaia_art::ArtRecord) -> [u8; 4] {
    let mut sources = [0u8; 4];
    for (d, s) in sources.iter_mut().zip(&r.anim_extra) {
        *d = *s;
    }
    sources
}
