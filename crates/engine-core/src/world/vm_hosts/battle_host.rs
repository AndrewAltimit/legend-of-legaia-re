//! The battle-action SM's `BattleActionHost` bridge (`BattleHostImpl`).
//! Split out of `vm_hosts.rs`.

use super::*;

// --- battle action host ----------------------------------------------------

pub(in crate::world) struct BattleHostImpl<'a> {
    pub(in crate::world) world: &'a mut World,
}

impl<'a> BattleActionHost for BattleHostImpl<'a> {
    fn display_tick(&self) -> u64 {
        self.world.clock.display_frames
    }

    fn battle_frame(&self) -> u64 {
        self.world.battle_frame_id()
    }

    fn actor(&self, slot: u8) -> Option<&BattleActor> {
        self.world.actors.get(slot as usize).map(|a| &a.battle)
    }
    fn actor_mut(&mut self, slot: u8) -> Option<&mut BattleActor> {
        self.world
            .actors
            .get_mut(slot as usize)
            .map(|a| &mut a.battle)
    }
    /// The engine plays hit reactions on a side channel beside the action
    /// channel's `current_anim`; retail commits both through `+0x1D9`, so
    /// the hold reads the merged id ([`World::battle_current_anim`]). The
    /// node test is the camera's (`battle_cam_inputs`'s `node_gone`).
    fn reaction_hold_view(&self, slot: u8) -> Option<(u8, bool)> {
        let a = self.world.actors.get(usize::from(slot))?;
        if !a.active {
            return Some((0, false));
        }
        let gone = a.battle.render_flag == vm::battle_formulas::STATE_DEFEAT_FADE
            && a.battle.render_color & 0x00FF_FFFF == 0;
        // Engine choice: a target whose animation rate `+0x21D` is `0` is
        // frozen - a starter commit stopped every slot - and its clip cannot
        // advance until the Done band restores the rates, which is past this
        // hold. Waiting on it would park the action forever; a queue that
        // reaches `0x20` with a non-acting slot still frozen is one an art
        // commit would have thawed in retail.
        if a.battle.anim_rate.get() == 0 {
            return Some((0, !gone));
        }
        Some((self.world.battle_current_anim(usize::from(slot)), !gone))
    }
    fn rng(&mut self) -> u32 {
        // Every draw the state machine takes is a retail `jal 0x80056798`.
        self.world.next_rand()
    }
    /// Retail reads `_DAT_8007B874 | _DAT_8007B938` and only tests it for
    /// zero-vs-non-zero (`0x801E6088..0x801E609C`). The port models the first
    /// of the pair - the newly-pressed mask the retail pad pump writes
    /// (`crate::retail_pad::RetailPadState::pressed`) - and has no analogue
    /// for the second, so this is the press edge alone. That is the stricter
    /// half: it can only ever *decline* to cut the banner short.
    fn pad_word(&self) -> u16 {
        self.world.input.retail_pad().pressed as u16
    }
    fn previous_action_cleared(&self, _: u8) -> bool {
        self.world.battle.prev_action_cleared
    }
    fn sound_bank_ready(&self, _: u8) -> bool {
        self.world.audio.sound_bank_ready
    }
    /// The disc spell table's class byte, read off the same
    /// [`crate::pause_screens::MenuTextTables`] copy the live cast path
    /// classifies capture-class moves from (`World::spell_table_class`).
    ///
    /// `is_capture_spell` is deliberately **not** overridden: the trait's
    /// default derives it from this byte, so the SM's capture route and the
    /// live path's kernel pick cannot disagree about the same record. Without
    /// a disc image there is no table and both answer "not capture", which is
    /// what the port did before this was wired.
    fn spell_class_byte(&self, id: u8) -> Option<u8> {
        self.world.spell_table_class(id)
    }
    /// The MP the engine charges for `id` - **the catalog**, i.e. literally
    /// the number [`World::cast_spell_on_slots`] deducts (`def.mp_cost`),
    /// before the shared ability-bit fold the SM applies on top.
    ///
    /// `World::tables.spell_catalog` is seeded from the user's `SCUS_942.54` at boot
    /// ([`crate::retail_magic::seru_magic_catalog_from_scus`]), so on a real
    /// disc this *is* the retail `+3` byte; disc-free it is the port's
    /// catalog. Either way there is one price per spell in this engine, and
    /// this is where the state machine reads it.
    ///
    /// A capture-class special the catalog does not carry (Cort's Mystic
    /// Circle `0xB7`) is priced off the same disc record its cast is built
    /// from ([`World::monster_cast_def`]): retail's state `0x28` reads the
    /// `+3` byte for every id, capture route included (`0x801E4500`).
    fn spell_mp_cost(&self, id: u8) -> u8 {
        if self.world.tables.spell_catalog.get(id).is_some() {
            return self.world.tables.spell_catalog.mp_cost(id);
        }
        self.world.monster_cast_def(id).map_or(0, |d| d.mp_cost)
    }
    fn character_ability_bits(&self, slot: u8) -> u32 {
        let i = slot as usize;
        self.world
            .party
            .character_ability_bits
            .get(i)
            .copied()
            .unwrap_or(0)
    }
    /// The retail range law (`FUN_8004E2F0`), computed from live state -
    /// see `World::battle_range_metric`
    /// (`crate::world::battle::locomotion`). Retail has no range *table* - the
    /// engine's former one was always empty, which short-circuited every
    /// approach state to "already in range"; it is gone.
    fn range_check(&self, attacker: u8, target: u8) -> u16 {
        self.world.battle_range_metric(attacker, target)
    }
    /// Retail's **live** position pair `actor[+0x34]` / `actor[+0x38]`. The
    /// engine keeps it on the actor's move state, where `World::enter_battle`
    /// puts it from [`crate::battle_seats`] and the locomotion drive
    /// (`World::tick_battle_locomotion`, the root-motion port) plus the
    /// separation pass move it - the same numbers `World::battle_target_rows`
    /// hands the target picker's angular enemy cursor.
    ///
    /// `None` is an unoccupied slot: the actor table holds exactly the seated
    /// combatants, which is what makes it the engine's reading of retail's
    /// roster-byte occupancy gate.
    fn actor_position(&self, slot: u8) -> Option<(i16, i16)> {
        self.world
            .actors
            .get(slot as usize)
            .map(|a| (a.move_state.world_x, a.move_state.world_z))
    }
    /// Mutation half of `actor_position` - the state-`0x16` arrival shove is
    /// its one SM caller.
    fn set_actor_position(&mut self, slot: u8, x: i16, z: i16) {
        if let Some(a) = self.world.actors.get_mut(slot as usize) {
            a.move_state.world_x = x;
            a.move_state.world_z = z;
        }
    }
    /// The seat (anchor) pair `+0x3C`/`+0x40` - `BattleActor::seat`, seeded
    /// by the first locomotion tick and cleared at battle teardown. Falls
    /// back to the live pair for a not-yet-seeded slot.
    fn actor_anchor(&self, slot: u8) -> Option<(i16, i16)> {
        let a = self.world.actors.get(slot as usize)?;
        Some(
            a.battle
                .seat
                .unwrap_or((a.move_state.world_x, a.move_state.world_z)),
        )
    }
    fn set_actor_anchor(&mut self, slot: u8, x: i16, z: i16) {
        if let Some(a) = self.world.actors.get_mut(slot as usize) {
            a.battle.seat = Some((x, z));
        }
    }
    fn battle_end(&mut self, cause: BattleEndCause) {
        self.world.battle.end = Some(cause);
        self.world
            .pending_battle_events
            .push(BattleEvent::BattleEnd { cause });
    }
    fn party_count(&self) -> u8 {
        self.world.party.party_count
    }
    /// The arena session's word ORed with the battle's own
    /// ([`World::special_battle_word`]) - what the special-battle wipe rule
    /// reads.
    fn special_battle_word(&self) -> u32 {
        self.world.special_battle_word()
    }
    /// The raw `+0x16E` word plus the typed tracker's packed bits
    /// ([`World::raw_status_word`]), so a Rot the tracker holds reaches the
    /// wipe rule's `& 0x38` test.
    fn status_word(&self, slot: u8) -> u16 {
        self.world.raw_status_word(slot)
    }
    /// Retail's wipe scan iterates the seated-count byte's worth of actor
    /// pointers (`*(0x8007BD24)+0` over `0x801C9370`, `0x801E6510..`), and
    /// that count is derived from the present-party list at battle load - a
    /// slot inside it always holds a projected character. The port's
    /// equivalent of "this party slot holds a combatant" is therefore: the
    /// roster projects a record onto the ordinal
    /// ([`World::party_roster_slot`], the mirror of retail's present-party
    /// list at `0x8007BD10`), or the battle mirrors carry a real combatant
    /// (`max_hp > 0`, the synthetic-battle path that stats slots directly).
    /// A stamped-but-hollow slot (no record, `max_hp == 0`) is the port-only
    /// unseeded state the wipe scan must not read as a dead party.
    fn slot_seated(&self, slot: u8) -> bool {
        if slot >= self.world.party.party_count {
            return true;
        }
        self.world
            .party
            .roster
            .members
            .get(self.world.party_roster_slot(slot as usize))
            .is_some()
            || self
                .world
                .actors
                .get(slot as usize)
                .is_some_and(|a| a.battle.max_hp > 0)
    }
    fn pose(&mut self, actor_id: u8, pose: Pose) {
        // `FUN_801D5854`'s invalid-slot guard (`0x801D58C8..0x801D58E8`): a
        // pose `>= 6` for a slot `>= 8` forces pose `9` and scrubs the
        // ghost bits off pool slots `0..=6` (`FUN_801DB9C4`).
        let pose = if actor_id >= 8 && pose as u8 >= 6 {
            let mut words: Vec<u32> = self
                .world
                .actors
                .iter()
                .map(|a| a.battle.flag_word)
                .collect();
            vm::battle_action::clear_pool_flag_words(&mut words);
            for (a, w) in self.world.actors.iter_mut().zip(words) {
                a.battle.flag_word = w;
            }
            Pose::Defeat
        } else {
            pose
        };
        self.world
            .pending_battle_events
            .push(BattleEvent::Pose { actor_id, pose });
        // Switch the actor's battle animation to the requested pose's action
        // clip (no-op for actors without installed action clips).
        self.world.apply_battle_pose(actor_id as usize, pose as u8);
    }
    fn ui_element(&mut self, effect_id: u8, mode: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::UiElement { effect_id, mode });
        // This is retail's HUD screen-element spawner `FUN_801D8DE8(id, mode)`:
        // it seats placement record `0x80076C10 + id * 0x18` as a text / chrome
        // widget (`FUN_8003541C`) and glides it between the record's two seats
        // (`FUN_801DB7B0`). It spawns **no effect script** - its only calls are
        // `FUN_8003541C`, `FUN_801DB7B0`, `FUN_8003563C`, `FUN_80035F04` and the
        // two string helpers, and none of the effect spawner `FUN_801DFDF0`'s
        // callers (`FUN_801DEA50`, `FUN_801E09F8`, `FUN_801E22C8`, SCUS
        // `FUN_8004998C` / `FUN_80047430`) is the action SM. The id is a
        // placement-record index, not an `efect.dat` script id, so routing it
        // into the effect pool played an unrelated script at every HUD raise.
        // Effect scripts reach the pool through `World::route_battle_effect_spawns`.
        //
        // The two message elements keep their line on the world here.
        self.world.message_banner_ui_element(effect_id, mode);
        // The actor / target plates the seed raises glide in from off
        // screen.
        self.world.note_action_plate_raise(effect_id, mode);
    }
    fn counter_ready(&self, slot: u8) -> bool {
        self.world.committed_attack(slot)
    }
    fn begin_counterattack(&mut self, counterer: u8, attacker: u8) -> bool {
        self.world.begin_counterattack(counterer, attacker)
    }
    fn camera_bounds(&mut self) {
        self.world
            .pending_battle_events
            .push(BattleEvent::CameraBounds);
    }

    fn monster_action_tags(&self, slot: u8) -> Option<Vec<u8>> {
        self.world.battle_monster_action_tags(slot)
    }

    fn learn_absorbed_seru(&mut self, slot: u8, seru: u8) {
        self.world.learn_absorbed_seru(slot, seru);
    }

    fn monster_size_class(&self, actor_slot: u8) -> u8 {
        // Retail reads `0x801C9348[slot - 3] + 0x1F`. The engine's equivalent
        // is the slot's seated monster id resolved through the catalog; a slot
        // with no monster (party slot, empty slot, or a synthetic catalog with
        // no disc record) yields 0, which clamps to the default framing.
        let Some(id) = self
            .world
            .actors
            .get(actor_slot as usize)
            .and_then(|a| a.battle_monster_id)
        else {
            return 0;
        };
        self.world
            .tables
            .monster_catalog
            .get(id)
            .map_or(0, |def| def.size_class)
    }
    fn summon_band_exit(&mut self) {
        self.world.casting.module_ctx_278 = 0;
        self.world.casting.module_caption = None;
    }
    fn camera_frame_height(&mut self, height: i16) {
        self.world.battle.camera_frame_height = height;
        self.world
            .pending_battle_events
            .push(BattleEvent::CameraFrameHeight { height });
    }
    fn party_setup(&mut self, actor_slot: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::PartySetup { actor_slot });
    }
    fn monster_setup(&mut self, actor_slot: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::MonsterSetup { actor_slot });
        // Faithful `FUN_801E7320`: expand the targeting class the action picker
        // left in `actor.active_target` into a concrete target slot.
        self.world.resolve_monster_target(actor_slot);
    }
    fn recompute_battle_order(&mut self) {
        self.world
            .pending_battle_events
            .push(BattleEvent::RecomputeBattleOrder);
    }
    /// The capture band's **pager** (`0x6E` arm): retail calls
    /// `FUN_8003EC70(record[+1] + 0x28)`, streaming the cast's own slot-B
    /// module in before `0x70` starts re-entering its tick
    /// (`jal 0x801f2160` at `0x801E50C8`).
    ///
    /// `idx` is the acting actor's `params[0]`, i.e. the queued action id - the
    /// same value retail indexes the spell table with to reach `+1`. So this is
    /// the seam where the module becomes resident, and it is where the engine
    /// stages that module's spawn records: `spawn_cast_module_fx` resolves the
    /// id through `FUN_801F2160`'s `935 + sub_id` row and seats the records at
    /// the caster. The paging *event* still goes to the host, which owns the
    /// capture archive itself.
    ///
    /// REF: FUN_8003EC70 (the pager this seam stands for; the pool holds the
    /// band's records instead of streaming one image)
    fn load_capture_archive(&mut self, idx: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::LoadCaptureArchive { idx });
        let slot = self.world.battle_ctx.active_actor as usize;
        let origin = self
            .world
            .actors
            .get(slot)
            .map(|a| {
                [
                    a.move_state.world_x,
                    a.move_state.world_y,
                    a.move_state.world_z,
                ]
            })
            .unwrap_or([0, 0, 0]);
        self.world.spawn_cast_module_fx(idx, origin);
        // ...and arm the per-frame tick phase `0x70` holds on. Retail's
        // `0x6F` exit zeroes `ctx[+0x279]` (`0x801E5048`) right before it
        // hands over, which is the same reset this does.
        self.world.arm_capture_cast_module(idx);
    }
    /// The capture band's **per-frame module tick** (`0x70`): retail's
    /// `jal 0x801f2160` at `0x801E50C8`, whose non-zero return holds the
    /// phase. See [`World::capture_stager_tick`].
    fn capture_stager_tick(&mut self) -> bool {
        self.world.capture_stager_tick()
    }
    /// The party cast trigger the pre-cast wait runs on its timer's expiry.
    ///
    /// Two arms on the spell id (`sltiu v0,a1,0x25` at `0x801DBFA0`), both
    /// writing the caster's action-parameter stream from `+0x1E0` - which is
    /// `params[1]`, since `params[0]` is `+0x1DF`:
    ///
    /// * `>= 0x25` - every player Seru id: `+0x1E0 = 9` (the summon sub-route
    ///   byte, which *is* `params[1]` in retail - the port also keeps it in
    ///   [`vm::battle_action::BattleActor::sub_route`]), the cast-effect id
    ///   `0x12` at `+0x1E1` and the terminator at `+0x1E2`
    ///   (`0x801DC064..0x801DC09C`). The engine arms its stager here; the
    ///   outcome is the stager's strike.
    /// * `< 0x25` - the per-spell `(anim, effect)` pair list the index at
    ///   `0x801F4E63 + id` picks out of the 8-byte records at `0x801F4EDC`,
    ///   copied pair by pair from `+0x1E0` and closed with `0xFF`
    ///   (`0x801DBFAC..0x801DC060`), read off the disc into
    ///   [`crate::world::BattleState::spell_anim_pairs`]. With pairs staged the
    ///   band walks them and the cast folds at its exit; an empty list (or no
    ///   disc read) leaves the terminator at `params[1]`, the band goes
    ///   straight to its cleanup, and the owed outcome folds here.
    ///
    /// PORT: FUN_801DBF9C
    fn spell_anim_trigger(&mut self, party_slot: u8, spell_id: u8) {
        use vm::battle_action::{SPELL_TRIGGER_SUMMON_MIN_ID, SUMMON_CAST_EFFECT_ID};
        self.world
            .pending_battle_events
            .push(BattleEvent::SpellAnimTrigger {
                party_slot,
                spell_id,
            });
        if spell_id >= SPELL_TRIGGER_SUMMON_MIN_ID {
            if let Some(a) = self.world.actors.get_mut(party_slot as usize) {
                a.battle.sub_route = 9;
                a.battle.params[1] = 9;
                a.battle.params[2] = SUMMON_CAST_EFFECT_ID;
                a.battle.params[3] = 0xFF;
            }
            self.world.arm_summon_stager(party_slot, spell_id);
        } else {
            let pairs: Vec<(u8, u8)> = self
                .world
                .battle
                .spell_anim_pairs
                .pairs(spell_id)
                .map(<[(u8, u8)]>::to_vec)
                .unwrap_or_default();
            if let Some(a) = self.world.actors.get_mut(party_slot as usize) {
                let n = a.battle.params.len();
                let mut end = 1usize;
                for (k, (anim, effect)) in pairs.iter().enumerate() {
                    let at = 1 + 2 * k;
                    if at + 2 >= n {
                        break;
                    }
                    a.battle.params[at] = *anim;
                    a.battle.params[at + 1] = *effect;
                    end = at + 2;
                }
                a.battle.params[end] = 0xFF;
            }
            if pairs.is_empty() {
                self.world.fold_pending_cast();
            }
        }
    }
    /// Stage a full-screen fade from the band's template - the summon
    /// band's flash-in (`0x33`) and flash-out (`0x34`) - on the world's one
    /// live fade, which both hosts composite through
    /// [`World::screen_fade_draw`]. Retail's spawn allocates a pool actor
    /// and runs the loader on its `+0x7C` block; the engine's
    /// [`crate::fade::FadeState`] is that block.
    ///
    /// REF: FUN_80024E80 (ported as [`crate::fade::spawn_fade`], which
    /// stamps `id` into the template's last word)
    fn spawn_screen_fade(&mut self, template: &vm::battle_action::SummonFadeTemplate, id: i16) {
        crate::fade::spawn_fade(
            &mut self.world.presentation.fade,
            &crate::fade::FadeTemplate {
                kind: template.kind,
                duration: template.duration,
                start_rgb: template.start_rgb,
                end_rgb: template.end_rgb,
                mode: [template.delay, template.hold, 0],
            },
            id,
        );
    }
    fn summon_stager_tick(&mut self) -> bool {
        self.world.summon_stager_tick()
    }
    /// The homing slots the move's effect-script terminator seeded.
    ///
    /// REF: FUN_801E09F8
    fn effect_child_slots(&self) -> ([u8; 4], [u8; 4]) {
        self.world.homing_child_slots()
    }
    /// `FUN_801DC0A0(actor, case)`: the cast-effect driver's camera script.
    /// The case's shot goes to the battle camera when the magic band's
    /// `0x2A..=0x2D` arms made the call (they arm no `FUN_801D5854` case),
    /// and its hand-off is written back over the queue byte under the cursor
    /// (`s3 = actor + 0x1DF + ctx[+0x15]`), which the next pass reads.
    ///
    /// Not modelled: case `1`'s `ctx[+0x24C] = 0xFD` (a host cannot write
    /// the context the step writes back) and case `0x13`'s `+0x21C` /
    /// `+0x21F` render stores.
    ///
    /// REF: FUN_801DC0A0
    fn spell_anim_sustain(&mut self, actor_id: u8, anim_id: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::SpellAnimSustain { actor_id, anim_id });
        let inputs = crate::battle_cam_inputs::spell_cam_inputs(self.world, actor_id, anim_id);
        let step = vm::battle_cam_script::spell_cam_case(&inputs);
        if let Some(a) = self.world.actors.get_mut(usize::from(actor_id)) {
            if let Some(next) = step.next_case {
                let i = usize::from(a.battle.strike_index);
                if let Some(b) = a.battle.params.get_mut(i) {
                    *b = next;
                }
            }
            if step.clear_hit_bound {
                a.battle.hit_count_bound = 0;
            }
        }
        if vm::battle_cam_script::SPELL_CAM_STATES.contains(&self.world.battle_ctx.action_state) {
            self.world.battle.spell_cam = Some(inputs);
        }
    }
    fn apply_damage(&mut self, icon: u8, page: u8, target_slot: u8, party_slot: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::ApplyDamage {
                icon,
                page,
                target_slot,
                party_slot,
            });
    }
    /// Retail's present-party list `DAT_8007BD10[slot]`, which the engine
    /// mirrors as [`World::party_roster_slot`] - 1-based, so `id - 1` is the
    /// roster index the character record sits at. Monster slots report `4`,
    /// the value the cast-cue dispatcher's enemy leg tests for.
    fn roster_character_id(&self, slot: u8) -> u8 {
        if slot < self.world.party.party_count {
            self.world.party_roster_slot(slot as usize) as u8 + 1
        } else {
            4
        }
    }
    /// `(class, tier)` of the item's descriptor in the disc item-effect table
    /// (`0x800752C0`, resolved through the item property record's `+1`
    /// subtype) - `World::tables.item_effects`, the same table the field/battle item
    /// menus gate usability on. `None` without a disc image.
    ///
    fn item_effect_class_pair(&self, item_id: u8) -> Option<(u8, u8)> {
        let eff = self.world.tables.item_effects.as_ref()?.effect(item_id)?;
        Some((eff.class, eff.tier))
    }
    /// The spell-side sibling: `+1` of the same 12-byte spell record
    /// `spell_class_byte` reads `+0` from, so a cast's class *and* tier both
    /// come from the disc. Together they are what the commit stamps into
    /// `actor[+0x1E8]` / `[+0x1E9]`, and what decides a healing/buff spell's
    /// cue group and `battle_cast_cue`'s class-`7` sub-class gate.
    fn spell_sub_class_byte(&self, spell_id: u8) -> Option<u8> {
        self.world.spell_table_sub_class(spell_id)
    }
    /// The two cue-group tables off PROT 0898, through the installed
    /// move-power catalog's [`EffectAuxTables`](legaia_asset::move_power::EffectAuxTables) -
    /// the same holder `World::spawn_action_table_effect` reads the effect
    /// prototypes from, so the group ids, the SFX map and the prototypes all
    /// come from one parse of one overlay.
    fn cue_tables(&self) -> Option<(&[u8], &[u8])> {
        let aux = self.world.tables.move_power.as_ref()?.aux_tables()?;
        Some((aux.cue_group_bytes(), aux.clut_map()))
    }
    /// Place one expanded cue.
    ///
    /// The two arms go to the two spawn paths retail's own arms go to, on the
    /// **world** rather than through a host-drained queue, so both the native
    /// window and the browser play page get them without either having to
    /// know the cue group exists: `Actor` (`id & 0x80` set) is the 2D effect
    /// pool (`FUN_801DFDF0` -> [`World::try_spawn_effect`]) and `Effect` is
    /// the `0x801F6324` prototype scene (`FUN_80050ED4` ->
    /// [`World::spawn_action_table_effect`]).
    ///
    /// The third table's non-zero byte is **not** a sound cue and is dropped
    /// here: `0x801F6418` is a CLUT source x and `FUN_80058490` is
    /// `MoveImage`, so retail's arm is a palette-row blit, not a sound
    /// submit. See the `CueSpawn::Effect` arm below and
    /// [`crate::battle_effect_clut`].
    ///
    /// The spawn position is the cue actor's own live position, which is what
    /// retail builds the transform from (`actor[+0x34]`/`+0x38` for the
    /// translation, `+0x44..+0x4A` for the rotation).
    fn spawn_cue(&mut self, actor_slot: u8, spawn: vm::battle_cue_group::CueSpawn) {
        use vm::battle_cue_group::CueSpawn;
        let at = self
            .world
            .actors
            .get(actor_slot as usize)
            .map(|a| {
                [
                    a.move_state.world_x,
                    a.move_state.world_y,
                    a.move_state.world_z,
                ]
            })
            .unwrap_or([0; 3]);
        match spawn {
            CueSpawn::Actor { id, yaw } => {
                self.world.try_spawn_effect(id, at, (yaw as u16) & 0xFFF);
            }
            // `clut_x` is a VRAM x coordinate, not a cue id: retail's arm
            // is `MoveImage({x = clut_x, y = 476, w = 16, h = 1}, 224, 476)`
            // - a 16-entry palette-row swap. It was pushed into
            // `World::audio.battle_sfx_cues` while the table was read as an SFX
            // map, which fed the SFX scheduler the values `0xB0` / `0xC0` /
            // `0xD0`. The engine has no VRAM CLUT-row swap on this seam, so
            // the copy is dropped rather than mis-routed.
            CueSpawn::Effect { effect_index, .. } => {
                self.world.spawn_action_table_effect(effect_index, at);
            }
        }
    }
    /// The cast-start one-shot (`FUN_8004FCC8`), run through the
    /// dispatcher's own split.
    ///
    /// Every id the cast-cue band produces on the party leg is
    /// `roster_id * 0x10 + 0xF8..0xFC` with a 1-based roster id, so `>= 0x108`:
    /// a **CD-XA** clip, not an SPU descriptor (`sltiu v0,s0,0x100` at the
    /// dispatcher's head). It is the character's voice on an ordinary item
    /// use - `0x0108` for Vahn's Healing Leaf - and was measured starting a
    /// clip through `FUN_8003D53C` on every driven item action
    /// (`docs/subsystems/battle-action.md`, "The one caller is state `0x3D`").
    /// So the id goes through [`vm::battle_cast_cue::admit_voice_cue`] (the
    /// busy-drive gate, the slot remap, the span table) onto
    /// [`crate::world::AudioState::battle_xa_cues`], the `(clip, channel,
    /// dur)` channel both hosts play through their XA lane. Pushing the raw id
    /// onto the SFX queue instead had both hosts classify it as a voice and
    /// decline it, so an item turn was silent apart from whatever clip it
    /// staged.
    ///
    /// An id below `0x100` keeps the SFX queue.
    fn one_shot_sfx(&mut self, cue_id: u16) {
        if cue_id >= 0x100 {
            let raw = self
                .world
                .audio
                .xa_cue_durations
                .as_deref()
                .and_then(|t| t.get(usize::from(cue_id - 0x100)).copied());
            let gates = vm::battle_cast_cue::VoiceCueGates {
                side_band_stage: 0,
                clip_span_left: self.world.audio.battle_xa_busy_frames,
            };
            if let vm::battle_cast_cue::VoiceCueVerdict::Play(req) =
                vm::battle_cast_cue::admit_voice_cue(cue_id, gates, raw)
            {
                self.world.push_battle_xa_cue(crate::sfx_cue::XaVoiceClip {
                    clip: req.clip_slot,
                    channel: req.channel,
                    duration_sectors: req.duration_sectors,
                });
            }
            return;
        }
        let slot = self.world.battle_ctx.active_actor;
        self.world
            .audio
            .battle_sfx_cues
            .push(crate::battle_events::BattleSfxCue {
                kind: cue_id,
                timing_frames: 0,
                actor_slot: slot,
                target_slot: slot,
            });
    }
    /// The acting character's learned-spell record, for the queued-magic
    /// follow-up guard. Party slots only - retail reaches the record through
    /// `DAT_8007BD10[slot] - 1` and a monster slot has none.
    fn caster_spell_list(&self, party_slot: u8) -> Option<(Vec<u8>, Vec<u8>)> {
        if party_slot >= self.world.party.party_count {
            return None;
        }
        let rslot = self.world.party_roster_slot(party_slot as usize);
        let list = self.world.party.roster.members.get(rslot)?.spell_list();
        Some((list.ids.to_vec(), list.levels.to_vec()))
    }
    /// Character record `+0xF8` - word 1 of the 4-word accessory-passive
    /// bitfield [`World::refresh_party_ability_bits`] rebuilds from equipment.
    /// Distinct from `character_ability_bits`, which is word 0 (`+0xF4`).
    fn character_ability_bits_high(&self, party_slot: u8) -> u32 {
        if party_slot >= self.world.party.party_count {
            return 0;
        }
        let rslot = self.world.party_roster_slot(party_slot as usize);
        let Some(member) = self.world.party.roster.members.get(rslot) else {
            return 0;
        };
        let bits = member.ability_bits();
        u32::from_le_bytes([bits[4], bits[5], bits[6], bits[7]])
    }
    /// Character record `+0x185` count / `+0x186..` ids - the displayed-skill
    /// list the AI auto-fill arm draws its queue bytes from.
    fn learned_arts(&self, party_slot: u8) -> Vec<u8> {
        if party_slot >= self.world.party.party_count {
            return Vec::new();
        }
        let rslot = self.world.party_roster_slot(party_slot as usize);
        let Some(member) = self.world.party.roster.members.get(rslot) else {
            return Vec::new();
        };
        let skills = member.displayed_skills();
        let n = (skills.count as usize).min(skills.ids.len());
        skills.ids[..n].to_vec()
    }
    /// The disc-parsed [`legaia_art::ArtRecord`] for `(character, action)` -
    /// `World::tables.art_records`, the same map the entry resolver reads its
    /// per-strike power profile out of.
    ///
    /// The record supplies an art hit's side data - the status effect and
    /// the per-hit cue (`legaia_engine_vm::battle_action::art_strike_info_for_hit`,
    /// `World::apply_art_hit_side_data`); the power byte itself is the clip
    /// entry's, so an art whose record is not loaded still resolves its
    /// damage. The trait default answers `None` for every pair.
    fn art_record(
        &self,
        character: legaia_art::Character,
        action: legaia_art::ActionConstant,
    ) -> Option<&legaia_art::ArtRecord> {
        self.world.tables.art_records.get(&(character, action))
    }
    fn apply_art_strike(&mut self, info: legaia_engine_vm::battle_action::ArtStrikeInfo) {
        // Resolve per-slot weapon attack and the defense the art targets.
        // Base ATK plus the art arm of the execution-time equipment fold:
        // command `0x11` adds half the sum of all five equipment slots'
        // attack bytes (`FUN_801EC3E4`, `PTR_801CF4B4[5]`).
        let mut attack = self
            .world
            .battle
            .attack
            .get(info.actor_slot as usize)
            .copied()
            .unwrap_or(0);
        if let Some(bonuses) = self.world.battle.equip_atk.get(info.actor_slot as usize)
            && let Some(fold) = legaia_engine_vm::battle_formulas::arms_weapon_atk_fold(
                legaia_engine_vm::battle_formulas::ARMS_ART_COMMAND,
                bonuses,
            )
        {
            attack = attack.saturating_add(fold);
        }
        let defense = self.world.resolve_battle_defense(info.target_slot, &info);
        let outcome = crate::art_strike::apply_art_strike(attack, defense, &info);
        // Seed the target's HP-bar ramp the way every other damage entry
        // point does (`World::apply_battle_hp_delta`). The HP itself is
        // applied downstream by `fold_battle_event`, so only the *bar* side
        // is armed here - without it an art landed through the state machine
        // moves live HP with no ramp, which is the one visible difference
        // between this seam and the melee one. A petrified target absorbs the
        // hit and owes the bar nothing, matching the fold's own guard.
        if let Some(dmg) = outcome.damage
            && dmg > 0
            && !self.world.actor_is_petrified(info.target_slot)
        {
            let petrified_free = i32::from(dmg);
            if let Some(t) = self.world.actors.get_mut(info.target_slot as usize) {
                t.battle.arm_hp_bar();
                t.battle.accumulate_hp_bar(petrified_free);
            }
        }
        self.world
            .pending_battle_events
            .push(BattleEvent::ApplyArtStrike {
                actor_slot: info.actor_slot,
                target_slot: info.target_slot,
                strike_index: info.strike_index,
                outcome,
            });
        // Retail runs its learn-on-use check (`FUN_801EFBFC`) per accepted art
        // inside the queue-builder; the engine reaches the same decision from
        // the strike that art produced, which is the first point on the port's
        // path where both the acting party slot and the art id are resolved.
        // Only a party slot has a character record with a learned-art list.
        if info.actor_slot < 3 {
            let art_id = info.art.as_byte();
            // Keyed by ROSTER slot, the same index `World::save_full` writes
            // `learned_arts_mask` under - not the battle ordinal.
            let roster = self.world.party_roster_slot(info.actor_slot as usize) as u8;
            self.world.notify_art_used(roster, art_id);
        }
    }
    fn duck_audio_level(&mut self, target_pct: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::DuckAudioLevel { target_pct });
    }
}
