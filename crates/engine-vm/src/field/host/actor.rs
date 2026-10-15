//! The [`FieldActorHost`] supertrait of the field-VM host surface.

use crate::field::FieldCtx;

/// The stepped actor's motion and pose: moves, teleports, headings, ramps, model, arcs.
///
/// One of the supertraits of [`FieldHost`](super::FieldHost), the surface the
/// field VM calls into the game through.
pub trait FieldActorHost {
    /// Begin executing a move-table entry on `ctx`. Mirrors `func_0x800204F8`
    /// (the move-table consumer that `crates/mdt` parses). The VM has
    /// already written `ctx.move_id`, `ctx.field_5e`, and `ctx.move_substate`
    /// before this method fires; the host only needs to start the visual /
    /// animation side of the move (or override the substate for
    /// player-vs-NPC nuances).
    ///
    /// `move_id == 99` is the cancel sentinel on the player path.
    fn exec_move(&mut self, ctx: &mut FieldCtx, move_id: u8) {
        let _ = (ctx, move_id);
    }

    /// Teleport / position-set on `ctx`. `world_x` and `world_z` have already
    /// been computed from the bytecode's grid coordinates (formula
    /// `(b & 0x7F) * 0x80 + 0x40`, plus `0x40` if the high bit is set), and
    /// the VM has written `ctx.world_x` / `ctx.world_z` / `ctx.npc_x` /
    /// `ctx.npc_facing` / cleared `ctx.field_8b`. The host runs whatever
    /// post-step is appropriate (Y collision, camera scroll on the player
    /// path, NPC facing/movement init on the NPC path).
    ///
    /// `is_player` distinguishes the two paths in the original (player calls
    /// camera/scroll; NPC sets facing + movement init). The VM derives this
    /// from `ctx.flags & 0x1000000`; hosts can override based on richer
    /// bookkeeping.
    fn move_to(&mut self, ctx: &mut FieldCtx, world_x: u16, world_z: u16, is_player: bool) {
        let _ = (ctx, world_x, world_z, is_player);
    }

    /// Op `0x38` simple path: one entry of the **compass heading table**.
    /// When `op1 & 0x7F == 0`, the original copies
    /// `*(short *)(0x80073F04 + (op0 & 0xF) * 2)` into `ctx.field_26`, the
    /// actor's heading. The host owns the table - this method takes the
    /// `index = op0 & 0xF` and returns the u16 to write. Returning `None`
    /// keeps `ctx.field_26` unchanged (the VM still advances PC).
    fn compass_heading(&self, index: u8) -> Option<u16> {
        let _ = index;
        None
    }

    /// Op `0x38` simple path, the actor-facing half: the compass-LUT entry
    /// `index` (`op0 & 0xF`) lands in the **resolved actor's** `+0x26` - the
    /// heading its renderer draws it at. `player` is `true` for the extended
    /// target `0xF8`, which retail's prologue resolves to the player object
    /// (`FUN_8003C83C`) rather than to the context the caller passed. The
    /// actor is host state, so the host owns the write; called before
    /// [`Self::compass_heading`]'s context write.
    ///
    /// REF: FUN_801DE840 (case 0x38), FUN_8003C83C
    ///
    /// See also [`Self::set_actor_heading`], the `4C 48` sibling.
    fn face_compass(&mut self, ctx: &mut FieldCtx, index: u8, player: bool) {
        let _ = (ctx, index, player);
    }

    /// Set up multi-keyframe animation (op 0x4B). The VM has already populated
    /// `ctx.local_flags` with bit 0x1000 (animation gate), `ctx.flags` with
    /// bit 0x1000, and the per-frame slots `ctx[+0xB0..]` / `+0xB8` / `+0xC8`,
    /// but those layout slots are still opaque so we just hand the host the
    /// raw frame data. `frames` is `count` × 4 bytes.
    fn setup_animation(&mut self, ctx: &mut FieldCtx, count: u8, base_id: u8, frames: &[u8]) {
        let _ = (ctx, count, base_id, frames);
    }

    /// Op 0x43 sub-8 (face / rotation reset). The original writes
    /// `*(byte*)(ctx + 0x6D) = 0` and `*(short*)(ctx + 0x7A) = 0`. We
    /// expose `face_rotation` directly on `FieldCtx`; the `+0x7A` slot is
    /// not yet a named field, so the host applies it.
    fn actor_face_reset(&mut self, ctx: &mut FieldCtx) {
        let _ = ctx;
    }

    /// Op 0x43 sub-7 (face / body rotation setup).
    ///
    /// 17-byte instruction `[43, 7, face_id, b0..b3, lo0..hi0, lo1..hi1,
    /// lo2..hi2, lo3..hi3, target_lo, target_hi]`. The original writes a
    /// 12-byte rotation-config struct at `&DAT_8007BE60 + face_id * 12`
    /// populated from the operand stream (applied by `FUN_8001B47C` ->
    /// `FUN_80029888` as a full 3-axis GTE rotation matrix lerped by
    /// `actor + 0x7A`), then schedules a ramp to `ctx + 0x7A` via
    /// `func_0x8003C5F0`. The VM also writes `ctx.face_rotation = face_id`
    /// before the host call.
    ///
    /// `payload_4` is the 32-bit value at operand+2..6 (read via
    /// `func_0x8003CED8` - likely a packed s24 + flag byte).
    /// `params` are four u16 values at operand+6..14.
    /// `target` is the signed target value at operand+14..16.
    fn actor_face_rotation_setup(
        &mut self,
        ctx: &mut FieldCtx,
        face_id: u8,
        payload_4: u32,
        params: [u16; 4],
        target: i16,
    ) {
        let _ = (ctx, face_id, payload_4, params, target);
    }

    /// Op 0x43 sub-9 (explicit position, optional collision tween).
    ///
    /// 10-byte instruction `[43, 9, x_lo, x_hi, y_lo, y_hi, z_lo, z_hi, t_lo, t_hi]`.
    /// When `t = LE_u16(operand[7..9])` is non-zero, the original calls
    /// `FUN_801de698(ctx, &ctx[+0x14], &target_xyz, t)` to tween from current
    /// coords to target over `t` ticks. The VM doesn't do any of the immediate
    /// writes itself in that branch - the host owns the tween path.
    ///
    /// (When `t == 0`, the VM writes x/y/z to ctx if not `0xFFFF`, and that
    /// path doesn't need this hook.)
    fn tween_actor_position(&mut self, ctx: &mut FieldCtx, x: u16, y: u16, z: u16, ticks: u16) {
        let _ = (ctx, x, y, z, ticks);
    }

    /// `4C 48` with `ticks == 0`: the VM has written `value` into the
    /// context's `+0x26`, the heading the resolved actor draws at. The actor
    /// is host state (a placement's heading table, the player's render
    /// heading), so the host mirrors the write. The ramp form arrives through
    /// [`Self::ramp_ctx_slot`] with `sub == 8`.
    ///
    /// REF: FUN_801DE840 (nibble-4 sub-8)
    fn set_actor_heading(&mut self, ctx: &mut FieldCtx, value: u16) {
        let _ = (ctx, value);
    }

    /// Op 0x4C outer-nibble-4 ctx-slot ramp.
    ///
    /// The 0x4C nibble-4 family is a 6-byte "write or ramp a slot" cluster.
    /// Encoding `[4C, op0, val_lo, val_hi, ticks_lo, ticks_hi]` where
    /// `(op0 & 0x0F)` selects the slot:
    ///
    /// | Sub | Slot | VM ctx field | Notes |
    /// |---|---|---|---|
    /// | 0 | `+0x72` | `ctx.field_72` | plain s16 write or ramp |
    /// | 1 | `+0x6A` | `ctx.field_6a` | input is `(target >> 1).max(1)` (signed halve, floor 1) |
    /// | 2 | `+0x8E` | `ctx.field_8e` | when ramp == 0 and `flags & 0x20000000`, also writes `world_y = -value` |
    /// | 3 | `+0x24` | `ctx.field_24` | plain s16 write or ramp |
    /// | 4 | `+0x28` | `ctx.field_28` | plain s16 write or ramp |
    /// | 8 | `+0x26` | `ctx.field_26` | plain s16 write or ramp |
    ///
    /// When `ticks == 0`, the VM writes the value directly to the ctx field
    /// (no host call). When `ticks != 0`, the original schedules a
    /// `func_0x8003C5F0` ramp from current value to target - the host
    /// drives the per-frame interpolation, so this hook fires.
    ///
    /// PC always advances by 6.
    ///
    /// Sub-8 aims at the actor's heading, which the renderer draws from host
    /// state: its ramp arrives here, and its immediate write additionally
    /// through [`Self::set_actor_heading`].
    fn ramp_ctx_slot(&mut self, ctx: &mut FieldCtx, sub: u8, target: i16, ticks: u16) {
        let _ = (ctx, sub, target, ticks);
    }

    /// Op 0x4C outer-nibble-4 sub-5 - actor-field block (immediate write path).
    ///
    /// 11-byte instruction `[4C, 0x45, b1, w94_lo, w94_hi, w96_lo, w96_hi,
    /// w98_lo, w98_hi, ticks_lo, ticks_hi]`. The original at `0x801E12A4+`
    /// reads four sub-fields and writes them to the actor's `+0x44` pointer
    /// (the per-actor primary structure, typically the rendered object pool):
    /// - `b1` (u8) → `actor[+0x44][+0x9a]` (a control byte; mode select).
    /// - `w94` (s16) → `actor[+0x44][+0x94]`.
    /// - `w96` (s16) → `actor[+0x44][+0x96]`.
    /// - `w98` (s16) → `actor[+0x44][+0x98]`.
    ///
    /// Engines model the actor's `+0x44` pointer as their own per-actor
    /// rendered-object handle; the VM hands over the four field values and
    /// lets the host write them. Default impl is a no-op.
    ///
    /// PC advances by 11 (= `header_size + 10`).
    fn write_actor_look(&mut self, ctx: &mut FieldCtx, b1: u8, w94: i16, w96: i16, w98: i16) {
        let _ = (ctx, b1, w94, w96, w98);
    }

    /// Op 0x4C outer-nibble-4 sub-5 - actor-field block (ramp path).
    ///
    /// Same encoding as the immediate path but `ticks != 0`. The original
    /// schedules a `func_0x8003C5F0` ramp for each of the three s16 fields
    /// (`+0x94`, `+0x96`, `+0x98`) **only when** the new target differs from
    /// the current value, then runs through the default-arm STATE_RESUME
    /// path. The control byte at `+0x9a` is always written immediately - it
    /// isn't part of the ramp.
    ///
    /// Engines drive their own per-frame ramp interpolation; this hook fires
    /// once per `[4C, 0x45]` ramp instruction with the full target tuple +
    /// ticks. Default impl is a no-op.
    ///
    /// The VM advances past the 11-byte instruction after this hook fires:
    /// the ramp runs as a scheduled actor (`0x801E12A4..0x801E138C`), and
    /// the script does not wait on it.
    fn ramp_actor_look(
        &mut self,
        ctx: &mut FieldCtx,
        b1: u8,
        w94: i16,
        w96: i16,
        w98: i16,
        ticks: u16,
    ) {
        let _ = (ctx, b1, w94, w96, w98, ticks);
    }

    /// Op 0x4C outer-nibble-5 sub-0 - set actor model (pool-select).
    ///
    /// 4-byte instruction `[4C, 0x50, lo, hi]`. Reads a signed-16-bit value
    /// from `operand+1..3` and resolves it to a model index against one of two
    /// model-pool bases: the low half (`< 0xF0`) indexes from `_DAT_8007B6F8`,
    /// the high half (`>= 0xF0`) from `_DAT_8007B824 + 0xFF10`. The resolved
    /// index is installed via `func_0x80024E08(actor, model_idx)` - the
    /// set-model primitive (writes `actor+0x64`, clears draw-flag bit `0x1000`,
    /// re-stages via `FUN_80020F88`; see `docs/reference/functions.md`). The
    /// high half also sets bit `0x01000000` in `ctx.flags` (recording which
    /// pool base was used); the low half clears it.
    ///
    /// The VM applies the flag-bit toggle itself (`ctx.flags |=` / `&= !`)
    /// and hands the host the raw value + the high/low selection. PC += 4.
    /// (Earlier mislabelled as a "directional sound emitter" - `FUN_80024E08`
    /// is a model-set, not an audio call.)
    ///
    /// This default body ports the observable state writes of `FUN_80024e08`:
    /// - `ctx.model_id` (actor `+0x64`) = the model index.
    /// - `ctx.move_id` (actor `+0x5c`) = 0.
    /// - `ctx.flags` (actor `+0x10`) clears draw-flag bit `0x1000`.
    /// - when the world-map model pool is active ([`model_pool_is_world_map`],
    ///   retail game-mode global `_DAT_8007B83C == 0xF`) the index is mirrored
    ///   into `ctx.model_id_high` (actor `+0x60`).
    ///
    /// Note `high` (the wrapper's value-pool selection, `value >= 0xF0`) is a
    /// concern of the dispatch and of pool-base resolution, **not** of this
    /// primitive: retail `FUN_80024e08` takes only `(actor, model)`. The
    /// dispatch already toggled the pool-record flag bit from `high`; a host
    /// that owns model-pool bases overrides this method to add the correct
    /// base before storing the index.
    ///
    /// The retail non-world-map branch also clears `+0x10` bits `0x00108000`
    /// *transiently* around the re-stage call `FUN_80020F88(actor)` and then
    /// restores `+0x10` to the `0x1000`-cleared value, so that masking has no
    /// persistent effect. The re-stage itself (`FUN_80020F88` - resolves the
    /// `+0x64` model index against the global TMD pool and recomputes the
    /// actor mesh + `+0x56` move-substate) has no callable engine equivalent
    /// yet; it is left as a follow-up. Hosts that own actor meshes override
    /// this method to re-stage after the state writes land.
    ///
    /// [`model_pool_is_world_map`]: Self::model_pool_is_world_map
    ///
    /// PORT: FUN_80024e08
    fn set_actor_model(&mut self, ctx: &mut FieldCtx, value: i16, high: bool) {
        let _ = high; // wrapper/dispatch concern; the primitive ignores it.
        ctx.model_id = value as u16; // actor +0x64
        ctx.move_id = 0; // actor +0x5c
        ctx.flags &= 0xffff_efff; // actor +0x10, clear draw-flag bit 0x1000
        if self.model_pool_is_world_map() {
            // Retail branch `_DAT_8007B83C == 0xF`: mirror the id into +0x60.
            ctx.model_id_high = value as u16;
        }
    }

    /// Is the world-map model pool active? Mirrors the retail game-mode global
    /// `_DAT_8007B83C == 0xF` that gates the `+0x60` model-index mirror in the
    /// set-actor-model primitive ([`set_actor_model`]). Default is
    /// `false` (general field mode); the world-map host overrides it to `true`.
    ///
    /// [`set_actor_model`]: Self::set_actor_model
    fn model_pool_is_world_map(&self) -> bool {
        false
    }

    /// Op 0x4C outer-nibble-8 sub-0xC - conditional jump on `ctx.field_68 == 0`.
    ///
    /// 4-byte instruction `[4C, 0x8C, lo, hi]`. When `ctx.field_68 == 0` the VM
    /// jumps to the absolute target `signed_16(operand[1..3])`; otherwise PC
    /// advances by 4. The host has no input - the VM reads `ctx.field_68`
    /// directly.
    ///
    /// PC += 4 on no-jump.
    fn op4c_n8_subc_field_68_is_zero(&self, ctx: &FieldCtx) -> bool {
        ctx.field_68 == 0
    }

    /// Op 0x4C outer-nibble-D sub-6 - `ctx.field_74` bitfield mutation.
    ///
    /// 3-byte instruction `[4C, 0xD6, b1]`. The original at line 7028 writes:
    /// - if `b1 == 4`: clear the top bit only (`field_74 &= 0x7FFFFFFF`)
    /// - else: clear bits `0x83000000`, then set bit `0x80000000` and OR in
    ///   `(b1 as u32) << 24` (low byte of `b1` into the top byte of `field_74`).
    ///
    /// Pure ctx mutation - host hook is a side-effect-only ack. Halt at PC
    /// (the dump exits via `goto LAB_801e00bc`).
    fn op4c_nd_sub6_field_74_ack(&mut self) {}

    /// Op 0x4C outer-nibble-C sub-0 - **randomise the clip phase**.
    ///
    /// 2-byte instruction `[4C, 0xC0]` (3 as the cross-context `CC <t> C0`).
    /// The arm at `0x801E260C` reads the actor's move id `+0x5C`; when it is
    /// positive it runs the move-table consumer `FUN_800204F8` (which binds
    /// the clip a preceding `4C 51` / `A2` named, `+0x5E = +0x5C`,
    /// `+0x68 = 0`), then draws one BIOS `rand()` (`FUN_80056798`) and seats
    /// the frame cursor at `+0x68 = (rand() % frames) << 4`, `frames` being
    /// the bound clip's halfword `+2` (`0x801E2624..0x801E2670`). `town0d`'s
    /// festival crowd runs `CC <t> 51 .. <move>` then `CC <t> C0` per
    /// villager, so the dancers do not move in lockstep.
    ///
    /// The return value says whether the arm drew. Default: no draw. PC
    /// always advances by 2.
    fn randomize_clip_phase(&mut self, ctx: &mut FieldCtx) -> bool {
        let _ = ctx;
        false
    }

    /// Op 0x4C outer-nibble-C sub-3 - script-table teleport with tile-center
    /// math.
    ///
    /// 2-byte instruction `[4C, 0xC3]`. The original (dispatcher dump
    /// 6762-6810) calls `func_0x8003C8F0(ctx.field_50, 0)` to resolve the
    /// destination tile descriptor, then writes:
    /// - `world_x = (b & 0x7F) * 0x80 + 0x40` (with optional `+0x40` if high
    ///   bit is set) - the standard tile-center formula,
    /// - `world_z` similarly,
    /// - rotation, animation frame, sprite flags get reset.
    ///
    /// Both helpers (`func_0x8003C8F0` table lookup and `func_0x8003D0BC`
    /// descriptor walk) are overlay-resident; hosts that don't model the
    /// scene table leave the default no-op in place.
    ///
    /// PC always advances by 2.
    fn script_table_teleport(&mut self, ctx: &mut FieldCtx) {
        let _ = ctx;
    }

    /// Op 0x43 sub-0/1/A/B's arc spawn - retail `FUN_801D25EC(actor,
    /// &landing, apex, frames, release_ctx, 0x400, camera_follow)` at
    /// `0x801DF5AC`, called on the success side of the halt-acquire.
    ///
    /// `ctx` is the arced actor's context (already halted), `ext` the
    /// instruction's extended channel byte (`Some(0xF8)` = the player
    /// anchor), `req` the decoded operand. The host resolves the actor,
    /// samples the floor under a tile target, seeds the arc
    /// (`crate::field_ledge_hop_arc::spawn_arc_with_emitter`) and releases the
    /// halt when the arc lands (`FUN_801D5D60`). Default: no arc channel -
    /// the halt stays raised, which is the pre-arc behaviour.
    fn spawn_arc_jump(
        &mut self,
        ctx: &FieldCtx,
        ext: Option<u8>,
        req: &crate::field_ledge_hop_arc::ScriptArcRequest,
    ) {
        let _ = (ctx, ext, req);
    }

    /// Op 0x4C n5 sub-1 - NPC / player move-to-tile with run dispatch.
    ///
    /// 6-byte instruction `[4C, 0x51, x_enc, z_enc, depth_or_flags, move_id]`.
    /// The VM has already decoded `world_x` / `world_z` via the standard tile
    /// formula (`(b & 0x7F) * 0x80 + 0x40 + 0x40 if (b & 0x80)`). `depth_byte`
    /// is the lower 4 bits of byte +3, indexing the 8-entry depth table at
    /// `_DAT_80073F04` in retail. `move_id` is the move-table id passed into
    /// the move-table consumer (`func_0x800204f8`); `99` is the cancel sentinel
    /// on the player path.
    ///
    /// `is_player` is derived from `ctx == _DAT_8007c364` in the original;
    /// the VM passes `true` when `ctx.flags & 0x0100_0000` is set (the
    /// player-vs-NPC bit). Default no-op - hosts that own the move-table
    /// consumer override.
    fn run_to_tile(
        &mut self,
        ctx: &mut FieldCtx,
        world_x: u16,
        world_z: u16,
        depth_byte: u8,
        move_id: u8,
        is_player: bool,
    ) {
        let _ = (ctx, world_x, world_z, depth_byte, move_id, is_player);
    }
}
