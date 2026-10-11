//! Field-host bodies that allocate actors and re-stage their models.

use super::*;

impl FieldHostImpl<'_> {
    /// Op `4C 50` - the actor model set. The state writes are the VM
    /// default's (`+0x64`, `+0x5C = 0`, the `0x1000` clear, the world-map
    /// `+0x60` mirror); the re-stage `FUN_80024E08` runs through
    /// `FUN_80020F88` - the actor starts drawing the named TMD - is the live
    /// model id a placement carries on [`World::field_npc_live_model`],
    /// which both play hosts already re-bind mid-scene from (the same seat
    /// motion op `0x0E` writes). The id stays raw: the `>= 0xF0` player-bank
    /// select is [`legaia_engine_scene::model_bank::resolve_model_id`]'s, the `4C 50`
    /// arm's own `0x801E17AC..0x801E1824` select instruction for instruction.
    ///
    /// A placed object's bind context (an object-bind channel, its own op
    /// or a `CC <record> 50 ..` poke) lands on the live model its bind
    /// record draws with, [`World::object_live_models`] - the table op
    /// `0x0E` also writes, which both hosts redraw the record's object from.
    /// The credits walk on `map01` (`P2[40]`, `CC 08 50 20 00`) swaps Rim
    /// Elm's landmark, record 8, from its `.MAP` mesh to model `32`
    /// (`ending_vignette_rimelm_walkaway` holds `+0x64 = 37` over a bank base
    /// of `5`); without the route the overworld kept drawing the old village.
    /// An op aimed at the player (`CC F8 50 ..`, e.g. `jagaroom`'s costume
    /// swap) goes to [`World::field_player_set_model`] instead, through
    /// `FieldPlayerHost::player_set_model`.
    ///
    /// PORT: FUN_80024E08 (the model re-stage, through the live-model seat)
    pub(super) fn set_actor_model(&mut self, ctx: &mut FieldCtx, value: i16, _high: bool) {
        ctx.model_id = value as u16;
        ctx.move_id = 0;
        ctx.flags &= 0xffff_efff;
        if self.model_pool_is_world_map() {
            ctx.model_id_high = value as u16;
        }
        if let Some(record) = self.world.field_vm.executing_object {
            self.world
                .npcs
                .object_models
                .insert(usize::from(record), value);
        } else if let Some(slot) = self.world.field_vm.executing_channel {
            self.world.set_field_npc_live_model(slot, value);
        }
    }

    pub(super) fn allocate_child_actors(&mut self, _ctx: &mut FieldCtx, count: u8, tail: &[u8]) {
        // In the spawned opening-cutscene context (target 0xF8) this op is the
        // inline-narration text-draw, not an actor spawn - the separate
        // `CutsceneNarration` presenter owns those pages. Suppress the spawn
        // side-effect while the cutscene timeline steps; the VM still advances
        // the PC past the page bytes on its own.
        if self.world.cutscene.in_timeline {
            return;
        }
        // Walk `count` variable-length records out of `tail` using the
        // retail packet-length rule (FUN_8003CA38, mirrored in
        // `legaia_scene_formats::field_disasm::packet_length`): bytes <= 0x1E
        // terminate a record; bytes whose top nibble is 0xC consume one
        // extra byte. The walker stops when the tail is exhausted - the
        // retail original would over-read into adjacent memory, which the
        // port refuses by construction.
        let mut records: Vec<Vec<u8>> = Vec::with_capacity(count as usize);
        let mut cursor = 0usize;
        for _ in 0..count {
            if cursor >= tail.len() {
                break;
            }
            let len = legaia_scene_formats::field_disasm::packet_length(&tail[cursor..]);
            records.push(tail[cursor..cursor + len].to_vec());
            // Skip the terminator byte itself (the byte <= 0x1E that
            // closed the record); if the walker ran off the end without
            // seeing one, `cursor + len == tail.len()` and the next
            // iteration's bounds check exits the loop.
            cursor += len + 1;
        }
        for record in &records {
            self.world.pending_actor_spawns.push(record.clone());
        }
        self.world
            .pending_field_events
            .push(FieldEvent::ActorAllocate { records });
    }

    pub(super) fn spawn_morph_actor(&mut self, b1: u8, words: [i16; 3]) {
        // Synchronous actor allocator (see retail `FUN_801D77F4` body
        // dumped at `ghidra/scripts/funcs/overlay_cutscene_dialogue_801d77f4.txt`).
        // The dispatcher packs the four args
        //   `[vdf_idx: u8, tmd_idx: i16, kind: i16, variant: i16]`
        // into the 7 bytes after `[0x4C, 0xD8]`; FUN_801D77F4 then writes
        // `actor[+0x3C] = kind` and `actor[+0x3E] = variant` on the
        // allocated slot, plus `actor[+0x48] = DAT_8007C018[tmd_idx]`
        // (TMD pointer) and `actor[+0x4C] = VDF_body_ptr`. We mirror
        // all four writes here.
        let kind = words[1] as u16;
        let variant = words[2] as u16;
        // Mirror retail's `actor[+0x4C] = VDF_body_ptr`: look up the
        // VDF record body bytes for the emitted event payload. `None` when
        // no VDF buffer is installed or the index is OOR; the shared
        // allocator handles the empty-record case (synchronous spawn
        // semantics). `spawn_field_actor` performs the identical resolve
        // and stores the record on the actor.
        let record_bytes: Vec<u8> = self
            .world
            .vdf_record_bytes(b1)
            .map(|s| s.to_vec())
            .unwrap_or_default();
        match self
            .world
            .spawn_morph_weight_actor(words[0], b1, kind, variant)
        {
            Some(slot_idx) => {
                self.world
                    .pending_field_events
                    .push(FieldEvent::ActorSpawned {
                        slot: slot_idx as u8,
                        kind,
                        variant,
                        record: record_bytes,
                    });
            }
            None => {
                // Pool-exhausted: mirrors the retail bail-silently branch
                // where FUN_80020DE0 returns 0.
                self.world
                    .pending_field_events
                    .push(FieldEvent::ActorSpawnFailed {
                        record: record_bytes,
                    });
            }
        }
    }
}
