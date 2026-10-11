//! The [`FieldTerrainHost`] supertrait of the field-VM host surface.

use crate::field::FieldCtx;

/// The walk grid: tile flags, region tables, the floor-height ladder and footing.
///
/// One of the supertraits of [`FieldHost`](super::FieldHost), the surface the
/// field VM calls into the game through.
pub trait FieldTerrainHost {
    /// Bounding-box test result (op 0x4D). When the active ctx is INSIDE
    /// `[x_min..=x_max] × [z_min..=z_max]` (in tile units), the VM advances
    /// PC; when OUTSIDE, the arm reads a 2-byte offset from operand+4..6 and
    /// jumps through the dispatcher's `0x801E3614` exit label (an interior
    /// `addiu v0,v0,-2; j 0x801E3624; addu s8,s8,v0`, not a routine) - a
    /// relative skip the VM computes itself.
    ///
    /// `world_to_tile_use_alt` controls the tile derivation: `false` → the
    /// default `(world - 0x40) >> 7`, `true` → the alternate `(world << 16) >> 23`
    /// path that fires when `_DAT_1F800394 & 0x20000` is set.
    fn world_to_tile_use_alt(&self) -> bool {
        false
    }

    /// Op 0x4C sub-3 sub-D - **walk-region attribute refresh** at the
    /// player's tile.
    ///
    /// 2-byte instruction `[4C, 0x3D]`; arm at `0x801E10F8`. Computes the
    /// same `(player.world - 0x40) >> 7` tile pair as sub-`8` but calls
    /// `FUN_800180EC` instead of `FUN_801DE3E0`: that walks the `.MAP`
    /// region table at the tile, rebuilds the region-type mask
    /// `_DAT_8007B8F4` and latches the covering type-0/1 region's box into
    /// scratchpad `0x1F800384..87` plus its type byte at `0x1F80037C`
    /// (falling back to the full-map fill when nothing covers the tile).
    /// That box is what the camera composer's sweeps span and what the
    /// zone query's kind-0 arm tests against, so this is an attribute
    /// refresh, not a camera load. PC += 2.
    ///
    /// The sub-`8` arm is [`super::FieldCameraHost::camera_zone_query_at_player`]: the two
    /// share only the tile arithmetic.
    fn region_attributes_refresh_at_player(&mut self) {}

    /// Op 0x4C outer-nibble-7 - VRAM tile-flag bulk operation.
    ///
    /// 7-byte instruction `[4C, 0x7N, x0, x1, z0, z1, mask]`. Iterates over
    /// the rectangle `[x0..x1) × [z0..z1)` in the tile-flag bitmap at
    /// `_DAT_1F8003EC + 0x4000` (one byte per tile). The sub-op selects how
    /// the per-tile byte is mutated:
    ///
    /// | Sub | Op |
    /// |---|---|
    /// | 0 | `byte &= 0x0F` (clear high nibble) - yield via STATE_RESUME |
    /// | 1 | `byte |= 0xF0` (set high nibble) - yield via STATE_RESUME |
    /// | 2 | `byte &= ~(mask << 4)` (clear bits) - advance |
    /// | 3 | `byte |= (mask << 4)` (set bits) - advance |
    ///
    /// Default impl is a no-op; the VM still drives the loop for hosts that
    /// own the tile-flag bitmap. PC advances by 7 in the advance branches;
    /// the yield branches surface `Yield { resume_pc: pc + 7 }`.
    fn paint_collision_walls(&mut self, sub: u8, x_range: (u8, u8), z_range: (u8, u8), mask: u8) {
        let _ = (sub, x_range, z_range, mask);
    }

    /// Op 0x4C outer-nibble-8 sub-3 - rectangular tile fill.
    ///
    /// 7-byte instruction `[4C, 0x83, col_start, row_start, col_end,
    /// row_end, value]`. The original at lines 6447-6493 of the dispatcher
    /// dump (`overlay_0897_801de840.txt`) runs two nested loops over the
    /// inclusive rectangle `[col_start..=col_end] × [row_start..=row_end]`,
    /// calling `FUN_801D5630(col, row, ...)` for each tile. On non-null
    /// return the inner body writes `tile[+0x3] = 0; tile[+0x2] = value`.
    /// The loop exits via the dispatcher's `j 0x801e3624` label with the
    /// 7-byte PC advance in its delay slot (`0x801E212C`) and writes nothing
    /// else - an earlier note claimed a post-loop `_DAT_8007B630 = col_start`
    /// trailer, but those bytes are the *next* arm (sub-4, `0x801E2134`),
    /// which this arm's unconditional jump never reaches.
    ///
    /// `FUN_801D5630(kind, x, z)` (field overlay `0x801D5630..0x801D56C0`)
    /// is a .MAP trigger-block lookup: it calls `FUN_801D5AE0(kind, x, z,
    /// *(0x1F8003EC) + 0x10000)` and, on a null result, retries against
    /// `+0x12000`, returning the record pointer. It writes no ctx flags and
    /// re-enters nothing - an earlier note here described a 9-instruction
    /// resolver that set `ctx.flags |= 8`, which is not these bytes. The port exposes the
    /// rectangle via the host hook and lets the engine implement its tile
    /// pool however it wants.
    ///
    /// Default impl is a no-op: the tile pool is engine-owned, and there
    /// is no shared in-VM scratch the rect-fill writes to.
    fn rect_tile_fill(
        &mut self,
        col_start: u8,
        row_start: u8,
        col_end: u8,
        row_end: u8,
        value: u8,
    ) {
        let _ = (col_start, row_start, col_end, row_end, value);
    }

    /// Op 0x4C outer-nibble-9 sub-0/1/2 - floor-wave actor spawn via
    /// FUN_801DDE34, which spawns descriptor `0x801F27EC` (handler
    /// `0x801DA930`) to animate the scratchpad floor-height ladder entry
    /// `0x1F80035C[b1]` (not a fade dispatch).
    ///
    /// 9-byte instruction `[4C, 0x9N, b1, lo0, hi0, lo1, hi1, lo2, hi2]` where
    /// `N ∈ {0, 1, 2}`. Reads `b1 = operand[1]`, three signed-16-bit words from
    /// `operand+2..8`, and calls `FUN_801DDE34(b1, op0 & 0x0F, w0, w1, w2)`.
    /// PC += 9.
    fn start_floor_ladder_oscillator(&mut self, sub: u8, b1: u8, words: [i16; 3]) {
        let _ = (sub, b1, words);
    }

    /// Op 0x4C outer-nibble-9 sub-0xE - 16-word table copy.
    ///
    /// 0x22-byte (34) instruction `[4C, 0x9E, lo0..hi0, ..., lo15..hi15]`.
    /// Reads 16 signed-16-bit words from the operand stream and writes pairs
    /// to two scratchpad/RAM tables:
    /// - `*(short *)(_DAT_8007B898 + 2 + i*2) = words[i]`
    /// - `*(short *)(0x1F800314 + 0x48 + i*2) = -words[i]`
    ///
    /// Engines model their own table; the VM hands over the 16 values. PC +=
    /// 0x22.
    fn install_floor_height_ladder(&mut self, words: [i16; 16]) {
        let _ = words;
    }

    /// Op 0x4C outer-nibble-9 sub-0xF - **retire** every floor-height-ladder
    /// oscillator.
    ///
    /// 2-byte instruction `[4C, 0x9F]`, the exact shape of nibble-8 sub-7
    /// with a different handler VA. Its arm at `0x801E2548` is five
    /// instructions - `lw a0,-0x3cb4(0x8008)` (`_DAT_8007C34C`),
    /// `a1 = 0x801DA930`, `j 0x801E2DC4` - and that shared exit is
    /// `jal 0x8003CF40` with `addiu s8,s8,2` in its delay slot.
    ///
    /// So it neither registers nor parks: the sweep ORs the kill bit into
    /// every node running that handler and the PC moves on two bytes,
    /// unconditionally. Fifteen shipped scenes issue it.
    fn retire_floor_ladder_oscillators(&mut self) {}

    // Op 0x4C outer-nibble-D sub-9 - set inverted-Y mirror + clear flag.
    //
    // 4-byte instruction `[4C, 0xD9, lo, hi]`. Sets `ctx.flags |= 0x20000000`
    // (inverted-Y enable), reads `value = signed_16(operand[1..3])`. When
    // `value == 9999` the VM substitutes `-ctx.world_y` (re-mirror current
    // position). Then writes:
    // - `ctx.field_8e = value`
    // - `ctx.world_y = -value`
    //
    // All ctx writes are done by the VM directly; no host hook fires. PC += 4.

    /// Op 0x4C outer-nibble-D sub-0xA - clear inverted-Y mirror + collision Y.
    ///
    /// 2-byte instruction `[4C, 0xDA]`. Clears `ctx.flags & 0x20000000`, then
    /// calls `func_0x80019278(ctx)` (the collision-Y refresh helper) and
    /// writes the result into `ctx.world_y`. PC += 2.
    fn refresh_collision_y(&mut self, ctx: &mut FieldCtx) {
        let _ = ctx;
    }

    /// Op 0x4C outer-nibble-C sub-1 - reset every fog region's enable byte
    /// from its story flag (the MAN section-4 table the fog spawner
    /// `FUN_801D629C` searches).
    ///
    /// 1-byte instruction `[4C, 0xC1]`. The original walks the
    /// `_DAT_80073ED8` record array (count `DAT_80073EDC`, stride `0xB`):
    /// for each record, reads bytes 9 and 10 to form a 16-bit index,
    /// queries [`field_helpers::party_flag_test`] against the trigger-flag
    /// array, and writes 1 to record byte 0 if the bit is **clear**, else 0.
    /// Then jumps to `LAB_801df09c` (= PC += 2 via the standard "addiu s8,
    /// s8, 0x2; j epilogue" block at `0x801df09c`).
    ///
    /// When `_DAT_80073ED8` is null OR the count is zero, the original
    /// `goto code_r0x801df098` skips the loop and still advances PC += 2.
    ///
    /// `flags` is the trigger-flag byte array (the bit storage queried
    /// per-record). Hosts that don't model the record array can leave the
    /// default no-op in place - the dispatcher always advances PC += 2
    /// regardless. The two callers (gate set / clear) merge into a single
    /// hook because the original's two branches differ only in side-effect
    /// (the loop body) and the PC delta is identical.
    ///
    /// [`field_helpers::party_flag_test`]: crate::field::dispatch_leaves::party_flag_test
    fn reset_fog_region_enables(&mut self, flags: &[u8]) {
        let _ = flags;
    }

    /// Op 0x4C outer-nibble-E sub-4 - bounding-box collision query against
    /// the actor's world position.
    ///
    /// 8-byte instruction `[4C, 0xE4, x0, z0, x1, z1, skip_lo, skip_hi]`
    /// (`0x801E31C0..0x801E3288`). The corners are world units:
    /// `min = (b & 0x7F) * 0x80 + 0x20` (`+0x60` when the high bit is set)
    /// for `x0` / `z0`, `max = (b & 0x7F) * 0x80 + 0x60` (`+0xA0`) for
    /// `x1` / `z1`. The actor is outside when `x < min_x`, `z < min_z`,
    /// `max_x < x` or `max_z < z` (signed compares on `+0x14` / `+0x18`).
    /// Outside, the dispatcher takes the relative skip through the
    /// `0x801E3614` exit label (`pc + 6 + skip`); inside, PC advances by 8.
    /// `0x801E3614` is an interior label of the dispatcher (`addiu v0,v0,-2;
    /// j 0x801E3624; addu s8,s8,v0`), not a halt helper as this doc used to
    /// say.
    ///
    /// `bbox` is `[min_x, min_z, max_x, max_z]`, already converted. The
    /// default implementation is retail's test against the ctx's world
    /// position; hosts override only to observe or substitute it.
    fn outside_world_bbox(&self, ctx: &FieldCtx, bbox: [i16; 4]) -> bool {
        let x = ctx.world_x as i16;
        let z = ctx.world_z as i16;
        x < bbox[0] || z < bbox[1] || bbox[2] < x || bbox[3] < z
    }
}
