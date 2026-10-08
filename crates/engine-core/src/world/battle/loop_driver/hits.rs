//! Battle hit resolution: the per-frame hit-event tick, hit apply modes and
//! power runs, zero-length clip hits, the melee landing, reaction, block roll
//! and juggle window, combo totals and the melee impact cue.
//! Split out of `loop_driver.rs`; no logic change.

use super::*;

impl World {
    /// The engine seat of retail's per-frame damage-kernel call. For every
    /// actor whose committed one-shot clip is in flight, ask the kernel's
    /// head ([`vm::battle_action::hit_event_admits`]) whether the frame the
    /// clip is on is one of its `+0x10..+0x13` beats; resolve the hit; then
    /// run the tick's **event-path commit** - with a byte staged behind the
    /// clip (`ADVANCE_DONE`, retail `+0x1DC` bit 1) and the entry's `+0x76`
    /// lock clear, the queued clip commits once the cursor is past the beat
    /// by more than two frames ([`vm::battle_action::event_commit_due`]),
    /// cutting the swing short. That cut is the swing chain's pacing: a
    /// retail two-arrow attack starts its second swing three frames after
    /// the first one's hit, not at the first clip's end. Locked entries
    /// (every art record on the disc carries `+0x76 = 1`) play to their
    /// natural end and the next byte commits there
    /// ([`Self::tick_battle_animations`]).
    ///
    /// PORT: FUN_80047430 (`0x8004787C..0x800478A4`: the per-frame
    /// `FUN_801EC3E4(actor, entry, cursor >> 4)` call; `0x80047900..
    /// 0x80047A44`: the bit-1 event-path commit)
    /// REF: FUN_801EC3E4 (head guard chain ported as
    /// `legaia_engine_vm::battle_action::hit_event_admits`)
    pub(in crate::world) fn tick_battle_hit_events(&mut self) {
        use vm::battle_action::{ActionState, ActorFlags, event_commit_due, hit_event_admits};
        let state = self.battle_ctx.action_state;
        // A fresh action starts a fresh combo total on every target.
        if state == ActionState::Begin.as_byte() {
            for a in self.actors.iter_mut() {
                a.battle.damage_accum = 0;
            }
        }
        for i in 0..self.actors.len() {
            let (source, rewound) = {
                let a = &mut self.actors[i];
                match (a.battle_staged_anim, a.battle_animation.as_mut()) {
                    (Some(_), Some(p)) => (
                        p.hit_source().map(|src| (src, p.current_frame())),
                        p.take_loop_rewound(),
                    ),
                    _ => (None, false),
                }
            };
            let Some((src, frame)) = source else {
                continue;
            };
            // The loop-window arm's re-zero (`0x80047840..0x80047878`): on a
            // rewind, a party slot playing dynamic slot `0x11` under a latched
            // Hyper / Super constant restarts its hit index and effect cursor,
            // so the windowed clip re-fires its hits each cycle.
            // PORT: FUN_80047430 (`0x80047840..0x80047878`)
            if rewound {
                let a = &mut self.actors[i];
                if a.battle_monster_id.is_none()
                    && a.battle.current_anim == vm::anim_vm::DYNAMIC_ART_SLOT_B
                    && a.battle.latched_anim >= vm::battle_action::LOOP_REZERO_LATCHED_MIN
                {
                    a.battle.input_cursor = 0;
                    a.battle_effect_cursor = 0;
                    a.battle_anim_cue_cursor = 0;
                }
            }
            let frame_u8 = frame.clamp(0, 255) as u8;
            let hit_index = self.actors[i].battle.input_cursor;
            if let Some(hit) = hit_event_admits(
                state,
                &src.power_run,
                &src.event_frames,
                hit_index,
                frame_u8,
            ) {
                // The epilogue bump (`0x801EECDC..0x801EECE8`), on every
                // resolved call.
                self.actors[i].battle.input_cursor = hit_index.wrapping_add(1);
                self.resolve_hit_event(i as u8, hit, src.power_run, src.event_frames);
            }
            // The event-path commit: only with a byte staged behind this clip.
            let staged_behind = self.actors[i]
                .battle
                .flag_bits
                .has(ActorFlags::ADVANCE_DONE);
            if staged_behind && event_commit_due(&src.event_frames, src.event_lock, frame) {
                // The cut takes the entry's `+0x0E` displacement pro-rated by
                // how far the clip got - no HP test on this path, unlike the
                // natural end (`+0x228` taken as clear, as there).
                // PORT: FUN_80047430 (`0x80047950..0x80047A28`)
                let a = &mut self.actors[i];
                if let Some(p) = a.battle_animation.as_ref() {
                    let step = p.end_root_step();
                    if step != 0 {
                        let frames = p.frame_count().min(255) as u8;
                        let (sin, cos) = vm::battle_action::motion::trig12(a.battle.facing_angle);
                        let (dx, dz) = vm::battle_action::motion::event_cut_root_step(
                            sin, cos, step, frame, frames,
                        );
                        a.move_state.world_x = a.move_state.world_x.wrapping_add(dx as i16);
                        a.move_state.world_z = a.move_state.world_z.wrapping_add(dz as i16);
                    }
                }
                self.commit_staged_battle_anim_at_boundary(i);
            }
        }
    }

    /// Resolve one admitted hit event of `attacker`'s committed clip: the
    /// weapon fold, the melee roll with the clip's power byte, the
    /// accumulate, the art record's side data (status, hit cue) when the
    /// latched staged id names a Tactical Art, and the retail apply law -
    /// the hit landing once the band has left the strike loop (cursor parked
    /// at `0xFF`) that is its clip's last listed hit subtracts the whole
    /// accumulated total from live HP.
    ///
    /// PORT: FUN_801EC3E4 (`0x801EE984..0x801EEA40`: the apply gate -
    /// `ctx[+0x15] == 0xFF` and `entry[0x11 + idx] == 0 || idx == 3`)
    pub(super) fn resolve_hit_event(
        &mut self,
        attacker: u8,
        hit: vm::battle_action::HitEvent,
        power_run: [u8; 4],
        event_frames: [u8; 4],
    ) {
        use vm::battle_action::{
            STRIKE_CURSOR_PARKED, fold_weapon_atk_on_hit, staged_art_constant,
        };
        let state = self.battle_ctx.action_state;
        let (party, latched, chosen, committed) = {
            let a = &self.actors[attacker as usize];
            (
                a.battle_monster_id.is_none(),
                a.battle.latched_anim,
                a.battle.chosen_art,
                a.battle.current_anim,
            )
        };
        let art = staged_art_constant(latched, chosen, party);
        let Some(target) = self.resolve_attack_target(attacker) else {
            return;
        };
        // The limb-vs-height miss (`0x801EC488..0x801EC554`): ahead of the
        // equipment fold, the block roll and every damage stage, a party
        // hit whose power byte cannot reach the target's `+0x1E` class does
        // nothing but raise the effect-skip strobe and take the epilogue's
        // `+0x1F4` bump (already taken by the caller). No accumulate, no
        // apply, no flinch, no hit event - the earlier hit's apply-mode
        // look-ahead has already landed any total this one could strand.
        if vm::battle_action::limb_misses(party, hit.power_byte, self.attack_swing_class_of(target))
        {
            self.battle_ctx.effect_skip_strobe = 1;
            self.consume_effect_skip_strobe(attacker as usize);
            log::debug!(
                "battle hit: slot {attacker} -> {target} hit {} pb {:#04x} MISS (class {})",
                hit.hit_index,
                hit.power_byte,
                self.attack_swing_class_of(target)
            );
            return;
        }
        {
            let mut host = BattleHostImpl { world: self };
            fold_weapon_atk_on_hit(&mut host, attacker, state, &hit);
        }
        let cursor_parked =
            self.actors[attacker as usize].battle.strike_index == STRIKE_CURSOR_PARKED;
        let last_of_clip = hit.hit_index >= 3
            || event_frames
                .get(usize::from(hit.hit_index) + 1)
                .is_none_or(|&f| f == 0);
        // Retail's apply **mode**, computed after every admitted hit
        // (`0x801EE060..0x801EE128`). The ordinary arm is the cursor-parked /
        // last-beat pair above; the early arm lands the total now because
        // nothing left in the action can connect with the target's size
        // class; the carry arm lands nothing at all while the War God Icon's
        // Attack x2 pair is still running. It reads the look-ahead and the
        // cursor, never the hit's damage, so it is taken ahead of the roll:
        // the kill check that sits between the two needs it.
        let mode = self.hit_apply_mode(attacker, target, &power_run, hit.hit_index);
        let applied = match mode {
            vm::battle_action::APPLY_MODE_CARRY => false,
            vm::battle_action::APPLY_MODE_EARLY => true,
            _ => cursor_parked && last_of_clip,
        };
        // The kill check's own gates (`0x801EE128..0x801EE1A4`) are the apply
        // gate's: mode `0xFF` skips it, a non-zero mode on a monster target
        // takes it, and otherwise it needs the parked cursor
        // (`ctx[+0x15] == 0xFF`, `0x801EE15C`) and the clip's last beat
        // (`entry[0x11 + idx] == 0 || idx == 3`, `0x801EE180..0x801EE19C`).
        let dmg = self.land_melee_hit(
            attacker,
            target,
            hit.power_byte,
            committed,
            art.is_some(),
            applied,
        );
        if let Some(art) = art {
            self.apply_art_hit_side_data(attacker, target, art, hit.hit_index, dmg);
        }
        if applied {
            self.apply_combo_total(target);
        }
        let running_total = self.actors[target as usize].battle.damage_accum;
        log::debug!(
            "battle hit: slot {attacker} -> {target} hit {} pb {:#04x} dmg {dmg} total {running_total} applied {applied} (state {state:#04x}, cursor {:#04x}, committed {committed:#04x}, latched {latched:#04x})",
            hit.hit_index,
            hit.power_byte,
            self.actors[attacker as usize].battle.strike_index
        );
        self.battle
            .hit_events
            .push(crate::battle_events::BattleHitEvent {
                attacker_slot: attacker,
                target_slot: target,
                hit_index: hit.hit_index,
                power_byte: hit.power_byte,
                damage: dmg,
                running_total: running_total.min(u32::from(u16::MAX)) as u16,
                applied,
                is_art: art.is_some(),
            });
    }

    /// Consume the effect-record skip strobe `ctx[+0x263]` on `slot`: clear
    /// it and bump the actor's effect (`+0x1F5`) and cue (`+0x1F6`) cursors
    /// without walking a record - `FUN_801DEA50`'s `0x801DEBF4..0x801DEC48`
    /// arm. Retail's consumer is the effect-script call the anim tick makes
    /// for the same actor right after the kernel (`0x800478A0` ->
    /// `0x800478B8`); the engine walks the effect script earlier in the frame
    /// ([`Self::tick_battle_animations`]), so the strobe is consumed here, on
    /// the actor whose hit raised it, instead of on the next frame's first
    /// walk - which would be another seat's.
    ///
    /// PORT: FUN_801DEA50 (`0x801DEBF4..0x801DEC48`, the skip arm)
    pub(in crate::world) fn consume_effect_skip_strobe(&mut self, slot: usize) {
        if self.battle_ctx.effect_skip_strobe == 0 {
            return;
        }
        self.battle_ctx.effect_skip_strobe = 0;
        if let Some(a) = self.actors.get_mut(slot) {
            a.battle_effect_cursor = a.battle_effect_cursor.wrapping_add(1);
            a.battle_anim_cue_cursor = a.battle_anim_cue_cursor.wrapping_add(1);
        }
    }

    /// The apply mode of one admitted hit - retail's `s2`
    /// (`legaia_engine_vm::battle_action::apply_mode`).
    ///
    /// The look-ahead it feeds walks the rest of this clip's power run and
    /// then every stream byte the strike cursor has not reached, resolving
    /// each byte's action entry through the same clip lookup the anim commit
    /// uses. `hit.hit_index + 1` is the index retail seeds from
    /// `actor[+0x1F4]`: the engine bumps that counter before resolving, so the
    /// two agree.
    ///
    /// PORT: FUN_801EC3E4 (`0x801EDEE4..0x801EE128`)
    pub(in crate::world) fn hit_apply_mode(
        &self,
        attacker: u8,
        target: u8,
        power_run: &[u8; 4],
        hit_index: u8,
    ) -> u8 {
        let Some(a) = self.actors.get(attacker as usize) else {
            return vm::battle_action::APPLY_MODE_NORMAL;
        };
        // **Both** copies of the kernel gate on a monster target before they
        // compute anything: `sltiu v0,a0,0x3` on the target slot at
        // `0x801EDEB8` and `0x801EE724`, each branching past the look-ahead
        // *and* past the War God arm. Retail's literal is `3`; the port asks
        // for the seated party width. A party target therefore always takes
        // the ordinary arm - which is what makes the record-direct
        // `0x801C9348[target - 3]` read in the decision well-defined.
        if usize::from(target) < usize::from(self.party.party_count.clamp(1, 3)) {
            return vm::battle_action::APPLY_MODE_NORMAL;
        }
        let cursor = a.battle.strike_index;
        let queue = a.battle.params;
        let bits = vm::battle_action::remaining_hit_class_bits(
            power_run,
            hit_index.wrapping_add(1),
            &queue,
            cursor,
            |b| self.entry_power_run_for(attacker as usize, b),
        );
        // Retail reads the attacker's ability word `+0xF4` for the War God
        // Icon bit; a monster attacker has no character record, so the word
        // reads 0 and the carry arm cannot fire for it.
        let ability = if a.battle_monster_id.is_none() {
            self.party
                .character_ability_bits
                .get(attacker as usize)
                .copied()
                .unwrap_or(0)
        } else {
            0
        };
        vm::battle_action::apply_mode(
            bits,
            self.attack_swing_class_of(target),
            ability,
            self.battle_ctx.attack_x2_pass,
        )
    }

    /// The four power bytes of the action entry a stream byte names
    /// (retail `0x801C9360[slot][byte]` -> `entry[+0x00..+0x04]`), resolved
    /// through the same clip lookup [`Self::staged_byte_has_clip`] uses.
    pub(super) fn entry_power_run_for(&self, slot: usize, staged: u8) -> Option<[u8; 4]> {
        use vm::anim_vm::{StagedAnimTarget, resolve_staged_anim};
        let actor = self.actors.get(slot)?;
        let clip = match resolve_staged_anim(staged) {
            StagedAnimTarget::ArtBank { record, .. } if actor.battle_art_bank.is_some() => actor
                .battle_art_bank
                .as_ref()
                .and_then(|b| b.get(record as usize))
                .and_then(|c| c.as_ref()),
            _ => actor
                .battle_action_clips
                .as_ref()
                .and_then(|cl| cl.get(staged as usize))
                .and_then(|c| c.as_ref()),
        }?;
        clip.entry_power_run()
    }

    /// Whether the byte the chain just staged on `slot` has a clip with an
    /// entry head to play - the test that separates a real, paced clip
    /// from the zero-length fallback. Mirrors the anim commit's lookup
    /// ([`Self::commit_staged_battle_anim_at_boundary`]).
    pub(super) fn staged_byte_has_clip(&self, slot: usize, staged: u8) -> bool {
        use vm::anim_vm::{StagedAnimTarget, resolve_staged_anim};
        let Some(actor) = self.actors.get(slot) else {
            return false;
        };
        let clip = match resolve_staged_anim(staged) {
            StagedAnimTarget::ArtBank { record, .. } if actor.battle_art_bank.is_some() => actor
                .battle_art_bank
                .as_ref()
                .and_then(|b| b.get(record as usize))
                .and_then(|c| c.as_ref()),
            _ => actor
                .battle_action_clips
                .as_ref()
                .and_then(|cl| cl.get(staged as usize))
                .and_then(|c| c.as_ref()),
        };
        clip.is_some_and(|c| c.has_entry_head() && c.frame_count > 0 && c.part_count > 0)
    }

    /// The zero-length-clip fallback: resolve every hit the staged byte
    /// would have carried, now. A party art constant walks its record's
    /// power list (the embedded entry's power run is that list); any other
    /// byte is one hit with the byte itself as the power byte - the swing
    /// command's own tier, the pre-clip reading. When the byte is the
    /// action's last (the terminator is next) the combo total lands.
    pub(super) fn resolve_zero_length_clip_hits(&mut self, attacker: u8, staged: u8) {
        use vm::battle_action::staged_art_constant;
        let (party, chosen, next_is_end) = {
            let a = &self.actors[attacker as usize];
            (
                a.battle_monster_id.is_none(),
                a.battle.chosen_art,
                a.battle.read_param(0) == 0,
            )
        };
        let Some(target) = self.resolve_attack_target(attacker) else {
            return;
        };
        let art = staged_art_constant(staged, chosen, party);
        let mut hits: Vec<u8> = Vec::new();
        if let Some(art) = art {
            let character = self.actors[attacker as usize].battle.character;
            if let Some(rec) = self.tables.art_records.get(&(character, art)) {
                hits.extend(rec.power.iter().filter_map(|p| match p {
                    // Re-encode the decoded power byte for the kernel.
                    legaia_art::PowerByte::Damage(ap) => Some(power_byte_of(*ap)),
                    legaia_art::PowerByte::NoDamage => None,
                }));
            }
            hits.truncate(usize::from(vm::battle_action::HIT_EVENT_SLOTS));
        }
        // Any other byte is one hit with the byte itself as the power byte -
        // except the `0x19` / `0x1A` art starters, whose records carry no
        // event frame (a retail starter clip plays with `+0x10 = 0`, capture
        // and disc alike), and bytes outside the kernel's admitted band.
        if hits.is_empty()
            && (0x0C..=0x1F).contains(&staged)
            && !legaia_art::ActionConstant::from_byte(staged).is_some_and(|a| a.is_starter())
        {
            hits.push(staged);
        }
        if hits.is_empty() {
            return;
        }
        let n = hits.len();
        for (i, power) in hits.into_iter().enumerate() {
            let applied = next_is_end && i + 1 == n;
            // The limb-vs-height miss gate (`vm::battle_action::limb_misses`)
            // holds here too. Retail's apply-mode look-ahead lands a total
            // early when nothing after it connects; the fallback has no
            // look-ahead, so a missed last hit lands what is already there.
            if vm::battle_action::limb_misses(party, power, self.attack_swing_class_of(target)) {
                if applied {
                    self.apply_combo_total(target);
                }
                continue;
            }
            let dmg = self.land_melee_hit(attacker, target, power, staged, art.is_some(), applied);
            if let Some(art) = art {
                self.apply_art_hit_side_data(attacker, target, art, i as u8, dmg);
            }
            if applied {
                self.apply_combo_total(target);
            }
            let running_total = self.actors[target as usize].battle.damage_accum;
            self.battle
                .hit_events
                .push(crate::battle_events::BattleHitEvent {
                    attacker_slot: attacker,
                    target_slot: target,
                    hit_index: i as u8,
                    power_byte: power,
                    damage: dmg,
                    running_total: running_total.min(u32::from(u16::MAX)) as u16,
                    applied,
                    is_art: art.is_some(),
                });
        }
    }

    /// The art record's per-hit side data retail's kernel does not carry
    /// (it reads the clip entry only): the status effect on a landing hit
    /// and the hit cue at this index, scheduled now (the clip is on the
    /// beat).
    pub(super) fn apply_art_hit_side_data(
        &mut self,
        attacker: u8,
        target: u8,
        art: legaia_art::ActionConstant,
        hit_index: u8,
        dmg: u16,
    ) {
        let character = self.actors[attacker as usize].battle.character;
        let Some(rec) = self.tables.art_records.get(&(character, art)) else {
            return;
        };
        let effect = rec.enemy_effect;
        let cue = rec.hit_cues.get(usize::from(hit_index)).copied();
        if dmg > 0
            && effect != legaia_art::EnemyEffect::None
            && self.actors[target as usize].battle.liveness != 0
        {
            let applied = self
                .battle
                .status_effects
                .apply_from_enemy_effect(target, effect);
            // Rot's applier rolls the disabled limb (`rand % 3`, the retail
            // `1 << (rand%3 + 3)` bit pick).
            if applied == Some(legaia_engine_vm::status_effects::StatusKind::Rot) {
                let limb = (self.next_rand() % 3) as u8;
                self.battle.status_effects.set_rot_limb(target, limb);
            }
        }
        if let Some(cue) = cue
            && cue.is_sound()
        {
            self.audio
                .battle_sfx_cues
                .push(crate::battle_events::BattleSfxCue {
                    kind: cue.kind,
                    timing_frames: 0,
                    actor_slot: attacker,
                    target_slot: target,
                });
        }
    }

    /// Roll one melee hit from `attacker` on `target` and **accumulate** it:
    /// the retail melee kernel `FUN_801EC3E4`'s body. Attacker ATK (plus the
    /// execution-time equipment fold of the committed command) is rolled
    /// against the defender's UDF / LDF with the underdog rewrite and the
    /// finisher's post stages; then the hit's damage goes into the target's
    /// combo accumulator (`target[+0x0]`, `0x801EDB40`) and HP-bar
    /// accumulator (`+0x10`, `0x801EDB58`), followed by the Spirit accrual,
    /// the popup, the impact cue and the flinch. Live HP is **not** touched;
    /// [`Self::apply_combo_total`] does that once per combo. Returns the
    /// damage the hit rolled.
    ///
    /// `power_byte` is the byte the kernel resolves the hit from - the clip
    /// entry's `entry[hit_index]` (`0x801EC494`): it picks the defence half
    /// (`(byte - 0x0C) % 10 < 5` -> UDF, [`vm::battle_formulas::physical_defense_is_udf`])
    /// and the power scalar (`0x801F64EC[(byte - 0x0C) % 5]`,
    /// [`vm::battle_formulas::command_power_scalar`]). A swing entry's byte 0
    /// is a real power byte (Vahn's high swing reads `0x18`, his low swing
    /// `0x1D`), not the command. `committed` is the attacker's `+0x1D9`
    /// (the committed anim id): the equipment fold dispatches on it and the
    /// art scale (`> 0x10`) reads it.
    ///
    /// **A melee swing always connects.** The routine contains no read of the
    /// accuracy / evasion halfword `+0x168` at all. `FUN_800402F4`'s
    /// selector-9 roll, which the port used to gate this strike on, is the
    /// **queued-action interrupt** check - a stun, not a miss. Retail's
    /// "Miss" on a normal attack is the limb-vs-height mismatch (`+0x1E`
    /// class 2 / 3 against the power byte's class, `0x801EC488..0x801EC554`,
    /// [`vm::battle_action::limb_misses`]), which the callers test before
    /// they get here.
    ///
    /// PORT: FUN_801EC3E4 (the accumulating body; the head is
    /// `legaia_engine_vm::battle_action::hit_event_admits`)
    /// REF: FUN_800402F4 (selector 9 = the action-interrupt roll, ported as
    /// `battle_formulas::accuracy_roll`)
    pub(super) fn land_melee_hit(
        &mut self,
        attacker: u8,
        target: u8,
        power_byte: u8,
        committed: u8,
        _is_art: bool,
        kill_check: bool,
    ) -> u16 {
        let attacker_i = attacker as usize;
        let target_i = target as usize;
        // Base ATK (`+0x158`, seeded without equipment) plus the execution-time
        // equipment fold: half of the one equipment slot the committed command
        // reads (`FUN_801EC3E4`'s `PTR_801CF4B4` arms - footwear for High /
        // Low, slot 2 / 3 for the two arm commands, all five for an art).
        // Party attackers only; the monster branch performs no fold.
        let mut attack = self.battle.attack.get(attacker_i).copied().unwrap_or(0);
        if attacker < self.party.party_count
            && let Some(bonuses) = self.battle.equip_atk.get(attacker_i)
        {
            let fold_command = if committed > vm::battle_formulas::ART_ANIM_THRESHOLD {
                vm::battle_formulas::ARMS_ART_COMMAND
            } else {
                committed
            };
            if let Some(fold) = vm::battle_formulas::arms_weapon_atk_fold(fold_command, bonuses) {
                attack = attack.saturating_add(fold);
            }
        }
        // The block roll runs ahead of the damage rolls (`0x801EC5A8..
        // 0x801EC878` before the `0x801ECB84` fold), and a blocked hit skips
        // the whole damage body: `bne s7,zero,0x801EE6D4` at `0x801ECB60`.
        let blocked = self.roll_block(attacker, target, power_byte);
        // The damage roll below reads the same two terms (`lh 0x6d2` at
        // `0x801ECED8`, `lh 0x6d4` at `0x801ED1E0`): the hit path zeroes them
        // only after it (`0x801EE3C4`), the block path before it skips the
        // damage body (`0x801EC888`).
        let (attack_ramp, guard_ramp) = (self.battle.attack_ramp, self.battle.guard_ramp);
        // `0x801EC888` (block) / `0x801EE3C4` (hit): the first hit through
        // the kernel spends the approach terms.
        self.battle.attack_ramp = 0;
        self.battle.guard_ramp = 0;
        if let Some(block_entry) = blocked {
            // `0x801EE6D4..0x801EE6F8`: the attacker's anim cue cursor steps
            // over one cue - the impact sound a landed hit would have made.
            if let Some(a) = self.actors.get_mut(attacker_i) {
                a.battle_anim_cue_cursor =
                    (a.battle_anim_cue_cursor + 1).min(crate::anim_cue::ANIM_CUE_TRACK_LEN);
            }
            // The sound tail is reached through the apply arm only, and the
            // grunt compares the committed pose against `+0x1F3`.
            self.fire_melee_impact_cue(attacker, target, kill_check.then_some(block_entry));
            // `0x801EEC30..0x801EEC6C`: the defender commits its block entry.
            self.commit_melee_reaction(target_i, attacker_i, block_entry);
            return 0;
        }
        let defense = self.physical_defense_of(target, power_byte);
        // Spirit guard stance on the defender (a party slot that picked
        // Spirit and hasn't started its next turn).
        let target_guarding = self.battle.guarding.get(target_i).copied().unwrap_or(false);
        let hp = self
            .actors
            .get(attacker_i)
            .map(|a| a.battle.hp)
            .unwrap_or_default();
        let hit_inputs = vm::battle_formulas::PhysicalHit {
            attacker_atk: attack,
            attacker_hp: hp,
            defender_def: defense,
            command_scalar: vm::battle_formulas::command_power_scalar(power_byte),
            staged_anim: committed,
            defender_guarding: target_guarding,
            attack_ramp,
            guard_ramp,
            ..Default::default()
        };
        let mut raw =
            vm::battle_formulas::physical_predamage(&hit_inputs, &mut || self.next_rand() as u16);
        if self.toggles.use_damage_finish {
            // The finisher's *post* stages only: the defender's equipment
            // elemental-guard / All-Guard ladder, the 9999 cap and the
            // rand-based no-damage floor. `defender_guarding` is passed
            // `false` because the melee kernel above already accounted for
            // the Spirit stance - taking the finisher's halve as well would
            // charge the stance twice. The floor draws a rand only when the
            // hit zeroes out, which the melee kernel's chip floor makes rare.
            let floor_rand = if raw == 0 { self.next_rand() as u16 } else { 0 };
            let attacker_is_party = attacker < self.party.party_count;
            let target_is_party = target < self.party.party_count;
            let defender_resist = self.defender_resist(target);
            raw = vm::battle_formulas::damage_finish(&vm::battle_formulas::DamageFinish {
                predamage: u32::from(raw),
                attacker_slot: if attacker_is_party { 0 } else { 3 },
                defender_slot: if target_is_party { 0 } else { 3 },
                attacker_element: 7, // basic attack is non-elemental
                defender_resist,
                defender_guarding: false,
                enemy_defender_halve: self.mystic_shield_up(),
                bypass_party_resist: false,
                summon_power_pct: 100,
                floor_rand,
            }) as u16;
        }
        let dmg = raw;
        // Spirit accrues from the pre-nullify hit: retail's finisher fills the
        // gauge before the nullify/absorb stage zeroes the HP loss, so a Stone
        // target's absorbed hit still charges its gauge.
        self.accrue_spirit_gauge(target, dmg);
        // A petrified target (Stone) absorbs the hit - no HP loss.
        let dmg = if self.actor_is_petrified(target) {
            0
        } else {
            dmg
        };
        // Accumulate: the combo total and the bar's owed delta, never live HP
        // (`0x801EDB40` / `0x801EDB58`; the bar ramp is what the player sees
        // falling hit by hit).
        let survives = {
            let t = &mut self.actors[target_i].battle;
            t.damage_accum = t.damage_accum.saturating_add(u32::from(dmg));
            t.arm_hp_bar();
            t.accumulate_hp_bar(i32::from(dmg));
            t.damage_accum < u32::from(t.hp)
        };
        // The kill check's Seru absorb (`0x801EE1C0..0x801EE2E8`), ahead of
        // the impact tint as in retail - its `rand()` sits between the
        // damage rolls and the tint. Retail reaches the kill compare only on
        // a hit its per-hit gates (`0x801EE134..0x801EE1A4`) pass, which the
        // caller has decided as `kill_check` (see `seru_absorb`'s module
        // note); the compare itself is the accumulated total against live
        // HP (`sltu v0,a0,a2` at `0x801EE1CC`).
        let absorbed_now = kill_check && !survives && self.roll_seru_absorb(attacker, target);
        // Surface the strike for HUD damage popups.
        self.battle.hit_fx.push(BattleHitFx {
            target_slot: target,
            amount: dmg,
            is_heal: false,
            is_crit: false,
        });
        // The impact-tint triple on the struck actor (`FUN_801EC3E4`
        // `0x801EE3D4..0x801EE43C`): the acting record's `+0x7A` class,
        // gated `0 < class < 6`. The routine has no exit ahead of this arm
        // - every connecting swing reaches it, so a Stone-absorbed hit
        // tints too. Party and monster attackers alike: the monster's basic
        // swing goes through the same routine with its archive entry as
        // the record.
        // REF: FUN_801EC3E4 (`sltiu v0,v0,0x6` at 0x801EE3E0)
        let class = self.attacker_impact_class(attacker_i);
        if class < crate::move_power::IMPACT_CLASS_LIMIT {
            self.arm_impact_tint(target_i, class);
        }
        // The reaction this strike commits on the defender (retail's `s7`,
        // committed to the defender's `+0x1DA`); the grunt below compares it
        // against the defender's block entry.
        let committed_reaction = if dmg > 0 {
            self.melee_reaction_entry(attacker_i, target_i, power_byte, kill_check, absorbed_now)
        } else {
            None
        };
        // ... and its sound.
        self.fire_melee_impact_cue(attacker, target, committed_reaction);
        // The flinch is staged for a target still standing on the accumulated
        // total (`target[+0x0] < hp`, `0x801EEC18..0x801EEC30`).
        if let Some(entry) = committed_reaction {
            self.commit_melee_reaction(target_i, attacker_i, entry);
        }
        dmg
    }

    /// The melee kernel's reaction commit on the defender
    /// (`0x801EEC34..0x801EECBC`): a non-zero `s7` on a defender that is not
    /// Stoned (`+0x16E` bit `0x4`, `0x801EEC58..0x801EEC64`) is staged into
    /// `+0x1DA`, and the defender is **turned to face the attacker** -
    /// `FUN_80019B28(defender[+0x40], defender[+0x3C], attacker[+0x38],
    /// attacker[+0x34]) & 0xFFF` stored to the defender's `+0x46`
    /// (`0x801EEC94..0x801EECBC`). The bearing runs from the defender's body
    /// pair to the attacker's live pair with no half turn, the opposite end
    /// of the attacker's own `+ 0x800` facing recompute, so the two stand
    /// face to face. Nothing turns the defender back: a struck monster keeps
    /// the heading for the rest of the fight, and case 8's dead-target yaw
    /// (`-target[+0x46]`) frames the killed one from it
    /// (`player_steal_skeleton_banner` reads its skeleton turned onto Vahn).
    ///
    /// PORT: FUN_801EC3E4 (`0x801EEC34..0x801EECBC`, the defender's reaction
    /// commit and turn)
    pub(super) fn commit_melee_reaction(&mut self, target_i: usize, attacker_i: usize, entry: u8) {
        if entry == 0 || self.actor_is_petrified(target_i as u8) {
            return;
        }
        self.commit_battle_reaction_entry(target_i, entry);
        let (Some(t), Some(a)) = (self.actors.get(target_i), self.actors.get(attacker_i)) else {
            return;
        };
        let (bx, bz) = t
            .battle
            .seat
            .unwrap_or((t.move_state.world_x, t.move_state.world_z));
        let facing = vm::battle_action::bearing_12bit_approx(
            bz,
            bx,
            a.move_state.world_z,
            a.move_state.world_x,
        ) & 0xFFF;
        self.actors[target_i].battle.facing_angle = facing;
    }

    /// The reaction one connecting melee hit commits on its defender -
    /// `FUN_801EC3E4`'s `s7`, not the damage primitive `FUN_800402F4`'s rule
    /// (which always knocks a survivor with a get-up entry down).
    ///
    /// * The default is a **flinch on the struck half**: the power byte's
    ///   defence half (`(byte - 0x0C) % 10 < 5`, the test the damage roll
    ///   uses) picks the high flinch `+0x1EF` or the low one `+0x1F0`
    ///   (`0x801EDE18..0x801EDE78`), and a record carrying only one of the
    ///   two falls back to it (`0x801EDE98..0x801EDEBC`).
    /// * Only a hit that reaches the kill compare (`kill_check`, the per-hit
    ///   gates `0x801EE128..0x801EE1A4`) can escalate. The War God Icon
    ///   carry (apply mode `0xFF`) never does: its `beq v0,v1,0x801EE3B8` at
    ///   `0x801EE12C` lands one instruction past the knockdown load at
    ///   `0x801EE3B4`, so the carried hit keeps the flinch - its callers pass
    ///   `kill_check = false`.
    /// * A **killing** total takes the knockdown `+0x1F1` unless a Seru sits
    ///   staged in `ctx[+0x269]` (`bne v0,zero,0x801EE3BC` at `0x801EE350`,
    ///   which jumps over the load at `0x801EE374`). On a party blow on a
    ///   monster, the absorb block runs first: a roll that stages the Seru
    ///   this hit loads the knockdown itself when the record carries a
    ///   get-up entry (`0x801EE2EC..0x801EE304`), and the first-monster-id
    ///   `0xB3` arm (`0x801EE30C..0x801EE338`) forces entry `2`. So an
    ///   absorbing kill on a monster with no get-up animation **flinches**,
    ///   and one with a get-up knocks down (the clip ladder's death arm then
    ///   swaps in the get-up so the fallen Seru rises for the absorb).
    /// * A surviving defender with a get-up entry takes the knockdown when
    ///   the combo total exceeds a quarter of its max HP or leaves it under
    ///   a quarter (`0x801EE380..0x801EE3B4`); any other survivor keeps the
    ///   flinch.
    ///
    /// Not modelled: the `0xB3` arm's side write
    /// (`sb 0x10` through monster record 0's `+0x54` -> `+0x88`, at
    /// `0x801EE338`).
    ///
    /// PORT: FUN_801EC3E4 (`0x801EDE18..0x801EDEBC`, `0x801EE1C0..0x801EE3B8`,
    /// the reaction pick)
    pub(super) fn melee_reaction_entry(
        &self,
        attacker: usize,
        target: usize,
        power_byte: u8,
        kill_check: bool,
        absorbed_now: bool,
    ) -> Option<u8> {
        let [high, low, knockdown, getup, _] = self.battle_reaction_map(target)?;
        let mut s7 = if vm::battle_formulas::physical_defense_is_udf(power_byte) {
            high
        } else {
            low
        };
        if low == 0 {
            s7 = high;
        }
        if high == 0 {
            s7 = low;
        }
        if kill_check {
            let t = &self.actors.get(target)?.battle;
            let accum = t.damage_accum;
            let hp = u32::from(t.hp);
            if accum >= hp {
                // Seat classes by identity, as the absorb roll reads them.
                let party_blow_on_monster = self.actors.get(target)?.battle_monster_id.is_some()
                    && self
                        .actors
                        .get(attacker)
                        .is_some_and(|a| a.battle_monster_id.is_none());
                if party_blow_on_monster {
                    if absorbed_now && getup != 0 {
                        s7 = knockdown;
                    }
                    if self.battle_first_monster_byte() == 0xB3 {
                        s7 = 2;
                    }
                }
                if self.battle_ctx.multi_cast_gate == 0 {
                    s7 = knockdown;
                }
            } else if getup != 0 {
                let quarter = u32::from(t.max_hp >> 2);
                if quarter < accum || hp - accum < quarter {
                    s7 = knockdown;
                }
            }
        }
        (s7 != 0).then_some(s7)
    }

    /// The melee kernel's block decision for one hit: `Some(block entry)`
    /// when the defender blocks it. The roll itself is
    /// [`vm::battle_formulas::block_roll`]; around it sit the routine's own
    /// gates, in its order:
    ///
    /// * no roll (and no draw) without a block entry or once the
    ///   accumulated total has reached the defender's HP (`0x801EC5BC`,
    ///   `0x801EC5CC..0x801EC5DC`);
    /// * a defender already holding its block pose with the reaction timer
    ///   running keeps blocking (`0x801EC93C..0x801EC984`), and one mid-way
    ///   through any other reaction cannot block - the juggle arm clears
    ///   `s7` (`0x801ECA20..0x801ECA68`). Both read the defender's `+0x1F7`
    ///   juggle window as the last anim tick wrote it
    ///   ([`crate::world::Actor::battle_juggle_window`]);
    /// * a party attacker carrying ability bit `0x4000` cancels the block
    ///   after the verdict (`0x801ECAFC..0x801ECB5C`).
    ///
    /// The approach terms `ctx[+0x6D2]` / `+0x6D4` are
    /// [`crate::world::BattleState::attack_ramp`] / `guard_ramp`
    /// ([`Self::track_block_approach_terms`]); the damage roll that follows
    /// a hit reads the same pair before the kernel zeroes it.
    /// The Mystic Shield arm (`0x801ECA84..0x801ECAF8`, `s7 = 1` while
    /// `_DAT_8007BD84` is up) is not taken here.
    ///
    /// PORT: FUN_801EC3E4 (`0x801EC5A8..0x801ECB60`, the block gates)
    pub(super) fn roll_block(&mut self, attacker: u8, target: u8, power_byte: u8) -> Option<u8> {
        use vm::battle_formulas::{ABILITY_BLOCK_BREAK, BlockRoll, BlockSide, block_roll};
        let ti = usize::from(target);
        let block_entry = self.battle_reaction_map(ti)?[4];
        if block_entry == 0 {
            return None;
        }
        let t = self.actors.get(ti)?;
        if t.battle.damage_accum >= u32::from(t.battle.hp) {
            return None;
        }
        let reacting = t.battle_juggle_window;
        let in_block = t.battle_reaction_entry == Some(block_entry);
        let side = |w: &Self, slot: u8| -> BlockSide {
            let i = usize::from(slot);
            let own = w.actors.get(i).map(|a| a.battle.spd).unwrap_or(0);
            let spd = if own != 0 {
                own
            } else {
                w.battle.speed.get(i).copied().unwrap_or(0)
            };
            BlockSide {
                spd,
                atk: w.battle.attack.get(i).copied().unwrap_or(0),
                status: w.raw_status_word(slot),
                ability: (slot < w.party.party_count).then(|| w.party_ability_word(i)),
            }
        };
        let roll = BlockRoll {
            attacker: side(self, attacker),
            defender: side(self, target),
            attack_ramp: self.battle.attack_ramp,
            guard_ramp: self.battle.guard_ramp,
            power_byte,
            defender_spirit: self.battle.guarding.get(ti).copied().unwrap_or(false),
            attacker_art_slot: self
                .actors
                .get(usize::from(attacker))
                .is_some_and(|a| a.battle.current_anim == vm::anim_vm::DYNAMIC_ART_SLOT_B),
        };
        let mut blocked = block_roll(&roll, &mut || self.next_rand());
        if in_block && reacting {
            blocked = true;
        } else if reacting {
            blocked = false;
        }
        if blocked
            && roll
                .attacker
                .ability
                .is_some_and(|ab| ab & ABILITY_BLOCK_BREAK != 0)
        {
            blocked = false;
        }
        blocked.then_some(block_entry)
    }

    /// The defender's `+0x1F7` byte - the **juggle window**. Its one writer
    /// is the anim tick (`FUN_80047430` `0x80047E28..0x80047E54`), every
    /// tick for every actor: `+0x1F7 = (cursor >> 4) < entry[0x10 + idx]`,
    /// `idx` from `FUN_80050E00(entry + 0x10)` - the first event frame of
    /// the clip the actor is playing whenever its list has a zero in
    /// `+0x11..+0x13` ([`vm::battle_action::event_commit_gate_frame`]). So
    /// it is up only before the playing clip's first listed beat, and down
    /// for a clip whose list starts at `0` (every block clip) - not "a
    /// reaction is playing". A four-slot list (address-dependent in retail)
    /// reads as down.
    // REF: FUN_80047430, FUN_80050E00
    pub(in crate::world) fn juggle_window_open(a: &crate::world::Actor) -> bool {
        let (Some(player), Some(head)) =
            (a.battle_animation.as_ref(), a.battle_effect_script.as_ref())
        else {
            return false;
        };
        let Some(list) = head.get(0x10..0x14) else {
            return false;
        };
        let list = [list[0], list[1], list[2], list[3]];
        vm::battle_action::event_commit_gate_frame(&list)
            .is_some_and(|gate| player.current_frame() < i16::from(gate))
    }

    /// Keep the block roll's two approach terms in step with the action SM.
    /// State `0x14` seeds `ctx[+0x6D2]` from the facings it just set
    /// (`0x801E3068..0x801E30C8`), and every pass state `0x19` spends
    /// walking adds the frame step to `ctx[+0x6D4]` (`0x801E35DC..
    /// 0x801E35EC`, the stall arm) - one a vsync tick
    /// ([`super::commit_log_launch::BATTLE_PASS_STEP_PER_TICK`]), so the term
    /// counts the walk's vsyncs whatever the cadence.
    pub(in crate::world) fn track_block_approach_terms(
        &mut self,
        pre_state: u8,
        out: &vm::battle_action::StepOutcome,
    ) {
        use vm::battle_action::{ActionState, StepOutcome};
        if pre_state == ActionState::AttackFace.as_byte() {
            let slot = usize::from(self.battle_ctx.active_actor);
            let Some(a) = self.actors.get(slot) else {
                return;
            };
            let target = usize::from(a.battle.active_target);
            let Some(t) = self.actors.get(target) else {
                return;
            };
            // `diff = attacker[+0x46] - target[+0x46]`, folded to `diff` when
            // it is at least `0x800` and to `0x1000 - diff` below (the `sh`
            // in the `beq` delay slot at `0x801E30A4` stores `diff`, the
            // fall-through overwrites it), then `- 0x800`: the angular
            // distance from face-on, `0..=0x800` - zero when the two face
            // each other, `0x800` for a strike in the back.
            let diff = a.battle.facing_angle.wrapping_sub(t.battle.facing_angle) & 0xFFF;
            let folded = if diff < 0x800 { 0x1000 - diff } else { diff };
            self.battle.attack_ramp = (folded - 0x800) as i16;
        } else if pre_state == ActionState::AttackShortStep.as_byte()
            && !matches!(out, StepOutcome::Transition { .. })
        {
            let step = i16::from(super::commit_log_launch::BATTLE_PASS_STEP_PER_TICK);
            self.battle.guard_ramp = self.battle.guard_ramp.wrapping_add(step);
        }
    }

    /// A party member's ability word - the character record's `+0xF4` u32,
    /// the bitfield `FUN_801EC3E4` reads as `0x80084140 + id*0x414 + 0x6BC`.
    pub(super) fn party_ability_word(&self, member: usize) -> u32 {
        self.party
            .roster
            .members
            .get(self.party_roster_slot(member))
            .map(|r| {
                let b = r.ability_bits();
                u32::from_le_bytes([b[0], b[1], b[2], b[3]])
            })
            .unwrap_or(0)
    }

    /// Land the accumulated combo total on `target`'s live HP - retail's one
    /// `+0x14C` write per combo (`0x801EEA10..0x801EEA3C`: `hp - total`,
    /// floored at zero) followed by the accumulator clear (`0x801EEA74`).
    /// The bar was seeded hit by hit, so this does not touch it.
    ///
    /// PORT: FUN_801EC3E4 (`0x801EE9F8..0x801EEA78`, the apply arm)
    pub(in crate::world) fn apply_combo_total(&mut self, target: u8) {
        let Some(a) = self.actors.get_mut(target as usize) else {
            return;
        };
        let total = a.battle.damage_accum;
        a.battle.damage_accum = 0;
        if total == 0 {
            return;
        }
        let before = a.battle.hp;
        a.battle.hp = if total < u32::from(before) {
            before - total as u16
        } else {
            0
        };
        if a.battle.max_hp > 0 && a.battle.hp == 0 {
            a.battle.liveness = 0;
        }
    }

    /// The melee kernel's own sound - retail's `FUN_801EC3E4` tail
    /// (`0x801EEA80..0x801EEBEC`), **one** of two emissions, selected by the
    /// `_DAT_8007BD84` word:
    ///
    /// * **zero** - every ordinary battle: the battle-start sweep
    ///   `FUN_80055B6C` and the round reset `FUN_8004CE2C` both store zero
    ///   there and no dumped routine stores anything else - takes the
    ///   per-character **grunt**, `FUN_8003D53C(0x1D, chan, dur)` at
    ///   `0x801EEB44`, the seat's 1-based character id (`DAT_8007BD10[seat]`)
    ///   selecting `(0, 0x26)` Vahn / `(4, 0x2E)` Noa / `(6, 0x1A)` Gala
    ///   (`0x801EEAD0..0x801EEB40`); clip slot `0x1D` = `XA30`, the
    ///   ten-channel mono grunt bank. The fall-through re-reads the word at
    ///   `0x801EEB60` and, still zero, skips the cue at `0x801EEB68`.
    /// * **non-zero**: `bne v0,zero,0x801EEB70` at `0x801EEAC8` jumps over
    ///   the grunt into the cue path - `0x10C` through the battle overlay's
    ///   one sound funnel `FUN_8004FE5C` ([`crate::sfx_cue::route_sfx_cue`])
    ///   with the **target's actor-table index as the category**
    ///   (`0x801EEBD8`, `s4 = attacker[+0x1DD]`), so the two sides sound
    ///   different by construction: a struck party member takes the CD-XA
    ///   voice leg (`XA27` channel 4), a struck monster the runtime ring leg
    ///   (`0x2A8`).
    ///
    /// The engine mirrors the word as `MonsterAiState::flag_bd84` - the same
    /// cell the damage finisher reads as the enemy-defender halve and the
    /// `0xB4` boss-intro cast gates on.
    ///
    /// The grunt is the **block** grunt, not a swing sound: it goes out only
    /// when the reaction the strike commits on the defender (`s7`, the pose
    /// byte stored to the defender's `+0x1DA`) is non-zero and equals the
    /// defender's `+0x1F3` - its block entry (tag `0x0B`) -
    /// `0x801EEA88..0x801EEAA0`. `committed_reaction` is that byte; `None`
    /// when the strike commits no reaction. An ordinary swing commits the
    /// `+0x1EF` flinch or the `+0x1F1` knockdown and is silent here (a
    /// capture of four party swings took the skip every time, see
    /// `docs/subsystems/battle-action.md`). Firing it on every strike was the
    /// port's repeated "block" sound. The voice pass's in-flight counter
    /// `_DAT_8007BC20 < 2` (`0x801EEAB4`, the `xa_flag` debug counter) is
    /// not modelled. The cue arm keeps its retail gates: the
    /// attacker playing a plain action-table clip (`+0x1D9 < 0x10`,
    /// `0x801EEB88`) and, inside the funnel, the drive being idle
    /// (`FUN_8003DE7C(1) == 0` at `0x8004FE9C`, modelled as
    /// [`crate::world::AudioState::battle_xa_busy_frames`]).
    ///
    /// The ring is transient by design. Its slots are drained in retail by
    /// `FUN_80016B6C`, which the port does not model (the hosts' own SFX
    /// scheduler is the drain), and the only state a persistent ring would
    /// carry across calls is the `last_played` dedupe word that same drainer
    /// maintains - so a stored ring would sit at zero and dedupe nothing.
    ///
    /// PORT: FUN_801EC3E4 (`0x801EEA80..0x801EEBEC`, the two sound sites)
    pub(super) fn fire_melee_impact_cue(
        &mut self,
        attacker: u8,
        target: u8,
        committed_reaction: Option<u8>,
    ) {
        let category = self.retail_actor_category(attacker);
        if self.battle.monster_ai_state.flag_bd84 == 0 {
            // The grunt arm. `XA30` channel + read span per character id
            // (`DAT_8007BD10[seat]`, 1-based); a monster seat names no
            // character and falls out of the switch silent (`0x801EEAFC`).
            // Gated on the strike committing the defender's block entry
            // (`s7 != 0 && s7 == defender[+0x1F3]`).
            let block_entry = self
                .battle_reaction_map(target as usize)
                .map(|m| m[4])
                .unwrap_or(0);
            let blocked = committed_reaction.is_some_and(|r| r != 0 && r == block_entry);
            if category < 3 && blocked {
                let char_id = self.party_roster_slot(attacker as usize) as u8 + 1;
                let grunt = match char_id {
                    1 => Some((0u32, 0x26u32)),
                    2 => Some((4, 0x2E)),
                    3 => Some((6, 0x1A)),
                    _ => None,
                };
                if let Some((channel, duration_sectors)) = grunt {
                    self.push_battle_xa_cue(crate::sfx_cue::XaVoiceClip {
                        clip: GRUNT_CLIP_SLOT,
                        channel,
                        duration_sectors,
                    });
                }
            }
            return;
        }
        // The cue arm: `0x10C` through the funnel, gated on the **attacker**
        // playing a plain action-table clip (`lbu v0,0x1d9(v0)` off
        // `0x801C9370[s6]`, `0x801EEB88`), with the **target's** index as the
        // category (`andi s1,s4,0xff` / `move a1,s1`, `s4 = attacker[+0x1DD]`
        // loaded at `0x801EC450`). The funnel switches legs on that index, in
        // retail's actor-table space (party `0..=2`, monsters `3..=7`): a
        // struck party member raises the `XA27` sting, a struck monster the
        // `0x2A8` runtime row keyed through its own render-node category.
        let attacker_anim = self
            .actors
            .get(attacker as usize)
            .map(|a| a.battle.current_anim)
            .unwrap_or(0);
        if attacker_anim >= 0x10 {
            return;
        }
        let target_category = self.retail_actor_category(target);
        self.route_battle_cue(MELEE_IMPACT_CUE as u16, target_category);
    }
}
