//! The [`FieldActorSpawnHost`] supertrait of the field-VM host surface.

use crate::field::FieldCtx;

/// The actor list: spawning, cloning, allocating and resolving actors by id.
///
/// One of the supertraits of [`FieldHost`](super::FieldHost), the surface the
/// field VM calls into the game through.
pub trait FieldActorSpawnHost {
    /// Spawn a MAN partition-2 record as a new field-VM context (op 0x44).
    /// The original calls `func_0x8003bde0(0, 0, global_index - N0 - N1, 1)`
    /// where N0/N1 are the scene MAN's partition-0/1 record counts - so the
    /// operand is a GLOBAL record index that the dispatcher re-bases into
    /// partition 2, gate forced 1 (spawn subject only to the record's own
    /// C1/C2 story-flag gates). Scene-entry system scripts use this to launch
    /// cutscene records (e.g. the opening chain's `opstati` / `opurud` legs).
    /// The host owns the partition math + the record install.
    // REF: FUN_8003BDE0
    fn spawn_scene_record(&mut self, global_index: u8) {
        let _ = global_index;
    }

    /// Op `0x4C` sub-1 sub-op `0x14` - clone the actor named by the
    /// instruction's **sixth** payload byte.
    ///
    /// The whole nibble enters on `addiu s8,s8,7` (`0x801E0C8C`) and the
    /// five-entry table at `0x801CEEA0` sends `op0 - 0x10` to its arm; slot 4
    /// is `0x801E0E80`, which reads `lbu a0,6(s6)` - a byte past the five the
    /// other arms use - resolves it through the context walk `FUN_8003C83C`,
    /// and calls `FUN_801D835C(src, u24, s16)`. Its exit
    /// `j 0x801E3624 / addiu s8,s8,1` puts the extra advance in the branch
    /// delay slot, so the eighth byte is consumed on the miss path too.
    ///
    /// * `tint_rgb` is `FUN_8003CEB8(&operand[1])`, the 24-bit LE word retail
    ///   stores into the clone's `+0x74` modulation colour.
    /// * `fade_rate` is `FUN_8003CE9C(&operand[4])`, the sign-extended `s16`
    ///   that lands in the clone's `+0x54` - the per-vsync rate the clone's
    ///   own tick [`crate::actor_tick::clip_fraction_step`] multiplies.
    /// * `src_id` is `operand[6]`, resolved in the same id space every
    ///   cross-context target byte uses.
    ///
    /// A host that cannot resolve `src_id` does nothing: retail's
    /// `beqz s5,0x801e0eb0` skips straight to the exit.
    // REF: FUN_801D835C (the clone helper), FUN_8003C83C (id resolve),
    // REF: FUN_8003CEB8 (u24 operand), FUN_8003CE9C (s16 operand)
    fn clone_actor(&mut self, src_id: u8, tint_rgb: u32, fade_rate: i16) {
        let _ = (src_id, tint_rgb, fade_rate);
    }

    /// Op 0x43 sub-12 - the shutter blackout (close / hold / open ticks).
    ///
    /// 5-byte instruction `[43, 0xC, b1, b2, b3]`. Mirrors `FUN_801de754`
    /// which allocates a fresh actor via `func_0x80020DE0(&DAT_801F2858,
    /// _DAT_8007C34C)` and writes the three operand bytes into the new
    /// actor's `+0xB8`, `+0xBA`, `+0xBC` u16 slots. The VM doesn't track
    /// the actor pool - hosts model their own scripted-actor system.
    fn spawn_shutter_blackout(&mut self, b1: u8, b2: u8, b3: u8) {
        let _ = (b1, b2, b3);
    }

    /// Op 0x43 sub-D / sub-F (actor allocation via FUN_801de7bc).
    ///
    /// 6-byte instruction `[43, sub_op, b1, b2, b3, b4]`. Mirrors
    /// `FUN_801de7bc` which allocates a fresh actor via
    /// `func_0x80020DE0(&DAT_801F2870, _DAT_8007C34C)` and writes:
    /// - `+0x72 = b1`
    /// - `+0xB8 = b2`
    /// - `+0xBA = b3`
    /// - `+0xBC = b4`
    /// - `+0x50 = mode` (3 for sub-D, 0 for sub-F).
    ///
    /// `mode` is decoded by the VM (3 if sub_op == 0xD else 0).
    fn op43_subdf_alloc_actor(&mut self, sub_op: u8, mode: u8, args: [u8; 4]) {
        let _ = (sub_op, mode, args);
    }

    /// Op 0x43 sub-E (mark currently-iterating actor with flag bit 0x8).
    ///
    /// 2-byte instruction `[43, 0xE]`. The original walks the actor list
    /// via `func_0x8003CF04(_DAT_8007C34C, FUN_801DD9D4)` - the first live
    /// node (`+0x10 & 8` clear) whose handler word `+0xC` equals the key
    /// `FUN_801DD9D4` (a compare, not a call) - and if found sets
    /// `actor[+0x10] |= 0x8`. The VM passes the
    /// active ctx so hosts can correlate. PC += 2.
    fn op43_sube_mark_actor_flag_8(&mut self, ctx: &mut FieldCtx) {
        let _ = ctx;
    }

    /// Op 0x34 sub-2 - **re-script an attached light**.
    ///
    /// `[34, 2N]`, then the stream's next byte peeked. The arm
    /// (`0x801E001C..0x801E0098`) walks the actor list `_DAT_8007C354` for a
    /// light actor (handler `FUN_801E4470`, the op-`34 1x` spawn) whose
    /// `+0x90` is this context's actor; when one is found **and** the next
    /// byte is a `0x40` block header, it points the light's keyframe script
    /// `+0x94` at the block body, zeroes the script cursor `+0x9E` and frame
    /// `+0x9C`, and steps past the block. It does not yield: both arms leave
    /// through `0x801DF098` (`s8 += 2`, return), so a capture returns
    /// `pc + 4 + len` and a miss `pc + 2` - the block then runs as op `0x40`,
    /// which skips itself.
    ///
    /// `ext` is the extended channel byte (`0xF8` = the player); `block` is
    /// the bytes after the `0x40` header, `None` when no block follows.
    /// Returns `true` when a light took the script. Default: no light.
    fn op34_sub2_capture_pc(
        &mut self,
        ctx: &FieldCtx,
        ext: Option<u8>,
        block: Option<&[u8]>,
    ) -> bool {
        let _ = (ctx, ext, block);
        false
    }

    /// Op 0x4C outer-nibble-D sub-8 - synchronous actor allocator,
    /// `FUN_801D77F4` (overlay-resident) 4-arg call.
    ///
    /// 9-byte instruction
    /// `[4C, 0xD8, vdf_idx, tmd_lo, tmd_hi, kind_lo, kind_hi, variant_lo, variant_hi]`.
    /// PC += 9.
    ///
    /// The retail dispatcher at `0x4C 0xD8` (case 8 of the outer-D dispatch,
    /// see `overlay_world_map_801de840.txt` line 7682-7687) reads:
    ///   - `vdf_idx`  (`b1` here): u8 packet-record index into the VDF
    ///     buffer at `_DAT_8007B7DC + 4 + vdf_idx*4`; the record contains
    ///     the per-actor bytecode that FUN_801D77F4 stores at `actor[+0x4C]`.
    ///   - `tmd_idx`  (`words[0]` here): i16 delta added to the global
    ///     `_DAT_8007B6F8` then truncated to i16; the sum indexes the global
    ///     TMD table at `DAT_8007C018 + tmd_idx*4` and is written to
    ///     `actor[+0x48]` (the spawned actor's TMD pointer).
    ///   - `kind`     (`words[1]` here): u16 stored at `actor[+0x3C]`.
    ///   - `variant`  (`words[2]` here): u16 stored at `actor[+0x3E]`.
    ///
    /// Unlike the halt-acquire-gated `0x4C 0x80` allocator path (which
    /// queues records and lets the engine materialize them on its own
    /// schedule via [`crate::field::FieldHost`]-side wiring), `0x4C 0xD8`
    /// spawns synchronously: FUN_801D77F4 returns with the slot already
    /// populated, and the parent script advances by 9 unconditionally.
    /// Hosts emit `FieldEvent::ActorSpawned` directly from this hook.
    fn spawn_morph_actor(&mut self, b1: u8, words: [i16; 3]) {
        let _ = (b1, words);
    }

    /// Op 0x4C outer-nibble-E sub-6 - HSV colour grade of every resident mesh.
    ///
    /// 8-byte instruction `[4C, 0xE6, lo0, hi0, lo1, hi1, lo2, hi2]`. Calls
    /// `FUN_801D8280(dh, ds, dv)`, which walks the resident-object table
    /// `DAT_8007C018` and hands each object to `FUN_801D5E20`, a destructive
    /// hue / saturation / value shift of the TMD's packed colour words
    /// (`docs/subsystems/cutscene.md`). PC += 8.
    fn hsv_grade_resident_meshes(&mut self, words: [i16; 3]) {
        let _ = words;
    }

    /// Op 0x4C outer-nibble-D sub-2 - channel-lookup conditional spawn,
    /// then halt at PC.
    ///
    /// 2-byte instruction `[4C, 0xD2, b1]`. The original calls
    /// `func_0x8003C83C(b1)` (the script-context resolver - see
    /// `docs/subsystems/script-vm.md` "intra-function label catalogue"
    /// for the F8/FB system-channel idiom). When the result is **zero**
    /// (no resolved context), the original spawns a new script context via
    /// `func_0x8003D064` + `func_0x8003A1E4` with the rendering-busy
    /// guard (`*(_DAT_801C6EA4 + 8) = 1` around the call). Then halts.
    ///
    /// Hosts that don't yet model the channel resolver should leave this as
    /// a no-op - the halt-at-PC behaviour is handled by the dispatcher.
    fn spawn_placement_if_absent(&mut self, channel: u8) {
        let _ = channel;
    }

    /// Op 0x4C outer-nibble-E sub-B - conditional actor lookup with an
    /// embedded relative skip.
    ///
    /// 5-byte instruction `[4C, 0xEB, actor_id, skip_lo, skip_hi]`.
    /// Retail (`0x801E34DC..0x801E34F4`) calls `FUN_8003C83C(actor_id)`, the
    /// actor-list walk (`0xF8` = the player object).
    ///
    /// Two outcomes:
    /// - **actor resolved** → PC advances 5,
    /// - **actor not resolved** → relative skip to `pc + 5 + i16 - 2`
    ///   (signed LE16 at `operand+2..=operand+3`, the shared
    ///   `0x801E360C` skip exit).
    ///
    /// The host returns `Some(())` when the actor exists, `None` on a miss.
    /// Default impl returns `None` - engines without an actor pool always
    /// take the skip.
    fn actor_guard(&mut self, actor_id: u8) -> Option<()> {
        let _ = actor_id;
        None
    }

    /// Op 0x4C n8 sub-0 - actor allocator with halt-acquire prelude.
    ///
    /// 3-byte header `[4C, 0x80, count]` followed by `count` variable-length
    /// child-actor records. The VM has routed this through the standard
    /// halt-acquire predicate ([`super::FieldHaltHost::field_halt_acquire_predicate`] with
    /// `which = 0x80`) and performed the ctx mutation. `count` is the byte
    /// at operand+1 (immediately after the `0x80` sub-byte); `tail` is the
    /// raw bytecode slice starting at operand+2 (the first record byte) up
    /// to the bytecode end. Hosts walk the records themselves - the
    /// original uses `func_0x8003ca38` to advance through variable-length
    /// entries.
    ///
    /// The VM advances PC by 3 regardless of how many records the host
    /// consumes - the records are owned by the spawned actor at offset
    /// `+0x90`, not by the parent script's PC.
    ///
    /// Default no-op. Hosts that model actor allocation override.
    fn allocate_child_actors(&mut self, ctx: &mut FieldCtx, count: u8, tail: &[u8]) {
        let _ = (ctx, count, tail);
    }
}
