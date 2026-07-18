//! VDF/global-TMD registration, event/spawn drains, battle-event folding, and status-effect ticking.
//!
//! Split out of `world.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

impl World {
    /// Install the VDF ("set_mime") buffer for the active scene. The bytes
    /// must follow the `[u32 count][u32 byte_offsets[count]][body...]`
    /// layout the retail asset-dispatcher case 7 produces; see
    /// [`Self::vdf_buffer`] for citation. Engines that want the
    /// field-VM `0x4C 0xD8` opcode to surface real spawn bytecode call
    /// this on scene-load with the extracted asset-type-7 chunk's body.
    ///
    /// Passing `None` clears the buffer; the next `0x4C 0xD8` call will
    /// leave `Actor::spawn_record` empty.
    pub fn set_vdf_buffer(&mut self, bytes: Option<Vec<u8>>) {
        self.vdf_buffer = bytes;
    }

    /// Resolve a VDF body slice by index using the
    /// `[u32 count][u32 byte_offsets[count]][body...]` layout. Each
    /// returned slice starts at `byte_offsets[idx]` and runs to the
    /// next body's offset (or end-of-buffer for the last entry).
    ///
    /// Returns `None` when:
    ///  - no VDF buffer is set (the scene loader skipped the install),
    ///  - the buffer is too short to read the header,
    ///  - `idx >= count`, or
    ///  - the indexed offset walks past end-of-buffer.
    ///
    /// Mirrors the retail body at
    /// `ghidra/scripts/funcs/overlay_cutscene_dialogue_801d77f4.txt:152-203`:
    /// `puVar11 = (uint *)(iVar12 + *(int *)(((vdf_idx << 16) >> 14) + iVar12 + 4))`.
    pub fn vdf_record_bytes(&self, idx: u8) -> Option<&[u8]> {
        let buf = self.vdf_buffer.as_deref()?;
        if buf.len() < 4 {
            return None;
        }
        let count = u32::from_le_bytes(buf[0..4].try_into().ok()?);
        if (idx as u32) >= count {
            return None;
        }
        let table_byte = 4usize;
        let slot = table_byte + (idx as usize) * 4;
        if slot + 4 > buf.len() {
            return None;
        }
        let off = u32::from_le_bytes(buf[slot..slot + 4].try_into().ok()?) as usize;
        if off >= buf.len() {
            return None;
        }
        // Bound the body by the next *greater* offset (offsets aren't
        // guaranteed monotonic - we pick the smallest offset above
        // `off` from any later table slot, defaulting to EOB).
        let mut end = buf.len();
        for i in (idx as u32 + 1)..count {
            let s = table_byte + (i as usize) * 4;
            if s + 4 > buf.len() {
                break;
            }
            let next = u32::from_le_bytes(buf[s..s + 4].try_into().ok()?) as usize;
            if next > off && next <= buf.len() && next < end {
                end = next;
            }
        }
        Some(&buf[off..end])
    }

    /// Install a global TMD at pool index `idx`. The pool grows lazily on
    /// write to accommodate sparse loader-chain installs. Indices that
    /// later producers fill in stay `None` until they're explicitly set.
    ///
    /// Mirrors the retail `FUN_80026B4C` writer
    /// (`DAT_8007C018[DAT_8007B774++] = tmd_ptr`) but exposes the index
    /// directly rather than auto-bumping a counter - engines that want
    /// the retail behaviour can read the next free slot via
    /// [`Self::global_tmd_pool`]`.len()` and pass it here.
    pub fn set_global_tmd(&mut self, idx: usize, tmd: Arc<GlobalTmd>) {
        if idx >= self.global_tmd_pool.len() {
            self.global_tmd_pool.resize(idx + 1, None);
        }
        self.global_tmd_pool[idx] = Some(tmd);
    }

    /// Resolve a global TMD by pool index. Mirrors the retail field-VM
    /// allocator's `iVar13 = DAT_8007C018[(int16_t)tmd_idx]` read - the
    /// caller is responsible for clamping negative indices (the retail
    /// engine sign-extends the i16 then implicitly treats it as unsigned;
    /// the clean-room port returns `None` for negative or out-of-range
    /// indices via the `i16 → usize` cast guarded by the bounds check).
    ///
    /// Returns `None` when the slot is empty or `idx` is out of range.
    pub fn global_tmd(&self, idx: i16) -> Option<&Arc<GlobalTmd>> {
        if idx < 0 {
            return None;
        }
        self.global_tmd_pool.get(idx as usize)?.as_ref()
    }

    /// Drain emitted field-VM events. Engines call once per frame after
    /// [`World::tick`] to dispatch BGM, dialog, money, etc. Returns events
    /// in emission order.
    pub fn drain_field_events(&mut self) -> Vec<FieldEvent> {
        std::mem::take(&mut self.pending_field_events)
    }

    /// Drain queued actor-spawn requests emitted by field-VM op `0x4C 0x80`.
    /// Each entry is the variable-length bytecode stream for one child
    /// actor. Engines route these into their actor pool.
    pub fn drain_actor_spawns(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.pending_actor_spawns)
    }

    /// Engine-side consumer for queued actor-spawn requests.
    ///
    /// Drains [`Self::pending_actor_spawns`] (records queued by the
    /// `0x4C 0x80` halt-acquire-gated path) and, for each record:
    /// 1. Scans `actors[start_slot..MAX_ACTORS]` for the first inactive
    ///    slot. Slots `0..start_slot` are skipped so engines keep their
    ///    party / scripted actors out of the auto-allocation range.
    /// 2. Activates the slot and stores the record bytes on
    ///    [`Actor::spawn_record`]. The retail allocator writes the
    ///    bytecode pointer to `actor[+0x90]` (different from the `+0x4C`
    ///    VDF-body field that the synchronous `0x4C 0xD8` path uses);
    ///    the clean-room port stores the raw bytes on `spawn_record`
    ///    regardless and lets the engine route them as field-VM
    ///    bytecode for a child actor (the records are scripted-child
    ///    coroutines, not TMD-body or kind/variant tuples).
    /// 3. Emits a [`FieldEvent::ActorSpawned`] event for the engine.
    ///
    /// Leaves [`Actor::kind`] and [`Actor::variant`] at zero. The
    /// retail allocator for this opcode (overlay code at
    /// `overlay_world_map_801de840.txt:7080-7123` case `8 sub-0`)
    /// allocates from pool `0x801f28a0` and writes only
    /// `actor[+0x90]` (bytecode ptr), `actor[+0x94]` (parent
    /// back-pointer) and `actor[+0x54] = 0`; the `+0x3C`/`+0x3E`
    /// kind/variant fields are never written by this path, so zero
    /// matches retail.
    ///
    /// Mirrors the retail allocator's pool-exhausted branch: if no
    /// inactive slot is available, the record is dropped silently and
    /// a [`FieldEvent::ActorSpawnFailed`] event is emitted instead.
    ///
    /// Returns the count of slots that were actually allocated.
    pub fn materialize_actor_spawns(&mut self, start_slot: u8) -> usize {
        let start = (start_slot as usize).min(self.actors.len());
        let records = std::mem::take(&mut self.pending_actor_spawns);
        let mut allocated = 0usize;
        for record in records {
            match self
                .actors
                .iter()
                .enumerate()
                .skip(start)
                .find(|(_, a)| !a.active)
                .map(|(i, _)| i)
            {
                Some(slot_idx) => {
                    let actor = &mut self.actors[slot_idx];
                    actor.active = true;
                    actor.kind = 0;
                    actor.variant = 0;
                    actor.spawn_record = Some(record.clone());
                    self.pending_field_events.push(FieldEvent::ActorSpawned {
                        slot: slot_idx as u8,
                        kind: 0,
                        variant: 0,
                        record,
                    });
                    allocated += 1;
                }
                None => {
                    self.pending_field_events
                        .push(FieldEvent::ActorSpawnFailed { record });
                }
            }
        }
        allocated
    }

    /// Drain emitted battle action events. Engines call once per frame
    /// after [`World::tick`] to dispatch poses, UI elements, damage, etc.
    /// Returns events in emission order.
    pub fn drain_battle_events(&mut self) -> Vec<BattleEvent> {
        std::mem::take(&mut self.pending_battle_events)
    }

    /// Drain the presentation-only per-strike HP deltas queued by the live
    /// battle loop. Engines feed these into a damage-popup model; the HP
    /// mutation has already happened, so they are never re-applied. Returns
    /// the FX in the order they were resolved this frame.
    pub fn drain_battle_hit_fx(&mut self) -> Vec<BattleHitFx> {
        std::mem::take(&mut self.battle_hit_fx)
    }

    /// Drain the battle sound cues queued this frame (the art-strike `HitCue`
    /// sounds [`Self::fold_battle_event`] resolves). The host plays each through
    /// its `SfxBank::play_one_shot` at the cue's `timing_frames` delay; nothing
    /// here mutates gameplay state. Returns them in resolve order.
    pub fn drain_battle_sfx_cues(&mut self) -> Vec<BattleSfxCue> {
        std::mem::take(&mut self.battle_sfx_cues)
    }

    /// Drain the Tactical-Arts shout cues queued this frame (one per executed
    /// party art with a real action constant, pushed at art start). The host
    /// resolves each against its arts-voice bank and plays the CD-XA shout
    /// clip; nothing here mutates gameplay state.
    pub fn drain_battle_shout_cues(&mut self) -> Vec<crate::battle_events::BattleShoutCue> {
        std::mem::take(&mut self.battle_shout_cues)
    }

    /// Apply the gameplay-state side of a single battle event - currently
    /// `ApplyArtStrike` (subtracts the resolved damage from the target's
    /// `BattleActor::hp`, clamping at zero, and records the enemy effect on
    /// the target's `pending_status`). Engines that want both the visual
    /// dispatch and the gameplay-state update call this for each event
    /// drained from [`Self::drain_battle_events`].
    ///
    /// Returns `Some((target_slot, hp_after))` for events that changed HP,
    /// `None` otherwise - useful for HUD popups that want the post-hit HP.
    pub fn fold_battle_event(&mut self, event: &BattleEvent) -> Option<(u8, u16)> {
        match event {
            BattleEvent::ApplyArtStrike {
                actor_slot,
                target_slot,
                outcome,
                ..
            } => {
                // Surface the strike's sound cues for the host's SFX bank (these
                // were previously dropped). Hit-effect-only cues (kind 0x4C)
                // carry no sound, so only the `is_sound` cues are queued.
                for cue in &outcome.cues {
                    if cue.is_sound() {
                        self.battle_sfx_cues.push(BattleSfxCue {
                            kind: cue.kind,
                            timing_frames: cue.timing_frames,
                            actor_slot: *actor_slot,
                            target_slot: *target_slot,
                        });
                    }
                }
                // A petrified target (Stone) can't be damaged - the strike is
                // fully absorbed (and so doesn't wake a Sleep/Numb either).
                let petrified = self.actor_is_petrified(*target_slot);
                if let Some(target) = self.actors.get_mut(*target_slot as usize) {
                    if let Some(dmg) = outcome.damage
                        && !petrified
                    {
                        target.battle.hp = target.battle.hp.saturating_sub(dmg);
                        // Damage clears Sleep / Numb on the target (matches
                        // retail - the unit wakes when hit).
                        self.status_effects.on_damaged(*target_slot);
                    }
                    let applied = if outcome.enemy_effect != legaia_art::record::EnemyEffect::None {
                        target.pending_status = Some(outcome.enemy_effect);
                        // Push the status into the tracker so it
                        // subsequently ticks per-turn.
                        self.status_effects
                            .apply_from_enemy_effect(*target_slot, outcome.enemy_effect)
                    } else {
                        None
                    };
                    let hp = target.battle.hp;
                    // Rot's applier rolls the disabled limb (`rand % 3`,
                    // the retail `1 << (rand%3 + 3)` bit pick).
                    if applied == Some(legaia_engine_vm::status_effects::StatusKind::Rot) {
                        let limb = (self.next_rng() % 3) as u8;
                        self.status_effects.set_rot_limb(*target_slot, limb);
                    }
                    return Some((*target_slot, hp));
                }
                None
            }
            // Cast band (SM path): the per-actor action SM fires
            // `spell_anim_trigger` at `MagicPreCastWait`. For a player Seru-magic
            // id, request the summon spawn (the host resolves the overlay PROT
            // entry + spawns). Origin = the caster party slot's position when
            // available, else a default forward cast point.
            BattleEvent::BattleEnd {
                cause: BattleEndCause::Escaped,
            } => {
                // Retail escape teardown (battle SM state 0x66): stage the
                // 0x40-frame black→white screen fade the SM spawns through
                // the fade primitive before the battle unloads.
                self.screen_fade = Some(crate::fade::FadeState::load(
                    &crate::fade::escape_fade_template(),
                ));
                // A petrified member returns to normal when the party
                // escapes (retail's run band floors every downed party
                // slot's HP at 1 on a successful escape; the tracker-level
                // Stone clear is the engine's model of that restore).
                self.status_effects.cure_stone_on_escape();
                // The run band's floor writes the VM-side liveness (`+0x14C`);
                // mirror it onto the world HP so the live loop's hp==0 dead
                // scan doesn't re-down the member - a downed member leaves
                // the battle alive at 1 HP.
                for slot in 0..self.party_count.min(3) as usize {
                    let b = &mut self.actors[slot].battle;
                    if b.max_hp > 0 && b.hp == 0 {
                        b.hp = 1;
                        b.liveness = 1;
                    }
                }
                None
            }
            BattleEvent::SpellAnimTrigger {
                party_slot,
                spell_id,
            } => {
                let origin = self
                    .actors
                    .get(*party_slot as usize)
                    .map(|a| {
                        [
                            a.move_state.world_x,
                            a.move_state.world_y,
                            a.move_state.world_z,
                        ]
                    })
                    .unwrap_or([0, -300, -645]);
                self.request_summon_spawn(*spell_id, origin);
                None
            }
            _ => None,
        }
    }

    /// Step every actor's status effects forward one turn - folds the
    /// tick-damage into `BattleActor::hp` and emits per-status events.
    /// Called by engines once per battle round.
    pub fn tick_status_effects(&mut self) {
        let actor_count = self.actors.len();
        for slot in 0..actor_count as u8 {
            let (cur, max) = self
                .actors
                .get(slot as usize)
                .map(|a| (a.battle.hp, a.battle.max_hp))
                .unwrap_or((0, 0));
            if max == 0 {
                continue;
            }
            let dmg = self.status_effects.tick_actor(slot, cur, max);
            if dmg > 0
                && let Some(actor) = self.actors.get_mut(slot as usize)
            {
                actor.battle.hp = actor.battle.hp.saturating_sub(dmg);
                // A DoT kill is a death: pair HP==0 with liveness=0 like every
                // other damage entry point (fold_spell_outcome / apply_battle_art
                // / apply_basic_attack). Otherwise the corpse stays "alive" for
                // the liveness-keyed wipe checks + target/turn resolvers.
                if actor.battle.hp == 0 {
                    actor.battle.liveness = 0;
                }
            }
        }
    }
}
