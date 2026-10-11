//! The [`FieldCameraHost`] supertrait of the field-VM host surface.

use crate::field::{CameraParam, FieldCtx};

/// The camera rig: configuration registers, camera zones, snaps and the view window.
///
/// One of the supertraits of [`FieldHost`](super::FieldHost), the surface the
/// field VM calls into the game through.
pub trait FieldCameraHost {
    /// Render-config write (op 0x46, long form `op0 == 0x24`). Original
    /// writes `pbVar47[1..5]` to scratchpad RGB cluster + the matching main
    /// RAM mirrors. The four bytes are passed as-is; semantic interpretation
    /// is host-side.
    fn view_window_long(&mut self, b1: u8, b2: u8, b3: u8, b4: u8) {
        let _ = (b1, b2, b3, b4);
    }

    /// Render-config write (op 0x46, short form). The VM has done the bitfield
    /// math: `r = !(op0 >> 1)`, `g = 2 - (op1 >> 1)`, `b = (op0 >> 1) - 1`,
    /// `packed = (op1 >> 1) + 2`. Hosts apply to their fog/render state.
    fn view_window_short(&mut self, r: u8, g: u8, b: u8, packed: u8) {
        let _ = (r, g, b, packed);
    }

    /// Camera op 0x45 CONFIGURE. The VM has decoded the 10-bit param mask
    /// from `[op0, op1]` and reads each set bit's `u16` value into `params`.
    /// `mode` is `(op0 >> 2) & 0xF` - passed to `FUN_801de084` originally as
    /// a third arg. `apply_trigger` is the `u16` at `operand+2` (between the
    /// mask bytes and the per-param value stream). Hosts apply the params
    /// to camera state; original calls `FUN_801ddfe4` (init) before the
    /// loop and `FUN_801de084` (apply) after.
    fn camera_configure(&mut self, params: &[CameraParam], apply_trigger: u16, mode: u8) {
        let _ = (params, apply_trigger, mode);
    }

    /// Camera op 0x45 LOAD. Payload is 18 raw bytes following `op0` -
    /// `FUN_801dbc20(operand + 1)` consumes them.
    fn camera_load(&mut self, payload: &[u8]) {
        let _ = payload;
    }

    /// Camera op 0x45 SAVE. `FUN_801de004(0x801c6ea8)` snapshots the camera
    /// scratch struct. No operands.
    fn camera_save(&mut self) {}

    /// Camera op 0x45 APPLY. The original calls `FUN_801dab90` (compose the
    /// camera from the player into the staging block `0x801C6EA8`) +
    /// `FUN_801daa50` (clamp the focus `0x80089118` / `0x80089120` to the
    /// visible tile window `0x1F800384..87` / `0x1F8003E8..EB`, then apply
    /// the `0x8007B628` / `0x8007B62A` overrides), then hands the unaligned `s16` at
    /// `operand + 1` to `FUN_801de084` as the apply trigger and advances the
    /// PC by four. It is NOT a jump: the `s16` is a trigger, not a target.
    /// `mode` is `(op0 >> 2) & 0xF`, the same curve argument CONFIGURE passes.
    fn camera_apply(&mut self, apply_trigger: i16, mode: u8) {
        let _ = (apply_trigger, mode);
    }

    /// Op 0x43 sub-3/4/5/6 (camera-register zone ramp).
    ///
    /// 10-byte instruction
    /// `[43, sub_op, x_lo, z_lo, x_hi, z_hi, s_lo, s_hi, e_lo, e_hi]`. Each
    /// sub-op picks one of the four field **camera-configuration** registers:
    /// - sub-3 → `DAT_8007B618`
    /// - sub-4 → `DAT_8007B614` (eye-back depth)
    /// - sub-5 → `DAT_8007B60C` (pitch)
    /// - sub-6 → `DAT_8007B610`
    ///
    /// The original calls
    /// `func_0x8003C6A4(dest, 4, x_lo, z_lo, x_hi, z_hi, start, end)`
    /// (`0x801DF6D4..0x801DF6EC`), spawning an actor whose `+0x0C` handler is
    /// `FUN_80037018`: while the player stands inside the tile rectangle, the
    /// register lerps from `start` to `end` on his **Z** position. Both
    /// halfwords come through `FUN_8003CE9C` and are sign-extended.
    ///
    /// The VM passes the sub-op verbatim so hosts pick the register
    /// themselves. PC += 10.
    fn spawn_camera_register_ramp(&mut self, sub_op: u8, zone: [u8; 4], start: i16, end: i16) {
        let _ = (sub_op, zone, start, end);
    }

    /// Op 0x4C sub-3 sub-9 - **camera-zone re-query, footing conform, snap**.
    ///
    /// 2-byte instruction `[4C, 0x39]`; arm at `0x801E1078` off the
    /// nibble-3 jump table `0x801CEEB8`. Three calls against the global
    /// player context `_DAT_8007C364`:
    /// 1. `FUN_801DE3E0((player.world_x - 0x40) >> 7, (player.world_z - 0x40) >> 7)`
    ///    - the **camera-region tile query + load**: `FUN_801DBA20` picks the
    ///      MAN section-3 record covering the tile and `FUN_801DBC20` splits
    ///      it into the camera parameter block, or the fixed zone-miss set
    ///      when nothing covers it.
    /// 2. `FUN_80019278(player)` → `player.world_y` - re-conform the footing
    ///    to the floor under the (possibly script-moved) tile.
    /// 3. Falls into sub-`0xE` at `0x801E10BC`: `FUN_801DB8EC(player)`
    ///    (compose + snap the camera globals) then `FUN_801DAA50()` (clamp
    ///    the focus into the walk region).
    ///
    /// The host owns the player context, so the VM passes `ctx` (the active
    /// script's context) for hosts that want to correlate; the calls operate
    /// on the global player context. PC += 2.
    fn camera_zone_query_conform_and_snap(&mut self, ctx: &mut FieldCtx) {
        let _ = ctx;
    }

    /// Op 0x4C sub-3 sub-8 - **camera-zone re-query at the player's tile**.
    ///
    /// 2-byte instruction `[4C, 0x38]`; arm at `0x801E1048`. The bare
    /// `FUN_801DE3E0((player.world_x - 0x40) >> 7, (player.world_z - 0x40) >> 7)`
    /// half of sub-`9`: load the camera parameter block from the record
    /// covering the player's tile, without conforming the footing and
    /// without snapping (the per-frame ease walks the globals to the new
    /// block's pose). PC += 2.
    ///
    /// This and sub-`9`, sub-`0xE` and nibble-C sub-`4` are retail's **only**
    /// camera-zone re-query sites outside the player-seat path: retail's
    /// field camera does not re-query on a tile crossing.
    fn camera_zone_query_at_player(&mut self) {}

    /// Op 0x4C sub-3 sub-E - **camera snap + focus clamp**.
    ///
    /// 2-byte instruction `[4C, 0x3E]`; the jump table sends it to
    /// `0x801E10BC`, which is inside sub-`9`'s arm - the tail after the
    /// query and the footing conform. `FUN_801DB8EC(player)` then
    /// `FUN_801DAA50()`: compose the resident block into the staging
    /// descriptor, copy it straight into the live camera globals instead of
    /// easing, and clamp the focus into the walk region. The block is not
    /// re-queried. PC += 2.
    fn camera_snap_and_clamp(&mut self) {}

    /// Op 0x4C outer-nibble-0xC sub-0xF - the script camera-focus override.
    ///
    /// 4-byte instruction `[4C, 0xCF, b1, b2]`. The original at lines
    /// 6912-6931 of the dump computes two 16-bit values and writes them to
    /// `_DAT_8007B628` (X) and `_DAT_8007B62A` (Z):
    /// - `b1 == 0xFF` → use `ctx.world_x`.
    /// - `b1 == 0` → 0.
    /// - else → `(b1 as u16) * 0x80 + 0x40` (the standard "tile center"
    ///   conversion seen elsewhere in the dispatcher).
    /// - `b2` is computed the same way but reads `ctx.world_z` for the 0xFF
    ///   case.
    ///
    /// The original's three exits (`iVar18 = FUN_801e3620(); return iVar18;`
    /// and `goto code_r0x801e3620;`) all converge on the same "PC += 4" label.
    /// PC += 4 in every path.
    fn set_camera_focus_override(&mut self, x_global: i16, z_global: i16) {
        let _ = (x_global, z_global);
    }

    /// Op 0x4C outer-nibble-C sub-4 - **camera-zone query at an explicit
    /// tile**.
    ///
    /// 4-byte instruction `[4C, 0xC4, x_byte, z_byte]`; arm at `0x801E2878`
    /// off the nibble-C jump table `0x801CEF88`. Calls
    /// `FUN_801DE3E0(x_byte & 0x7F, z_byte & 0x7F)` - the same
    /// camera-region query + load as sub-3 sub-`8`
    /// ([`Self::camera_zone_query_at_player`]) but at a tile the script
    /// names rather than the player's, so a scene can frame a shot from a
    /// record the player is not standing in. PC += 4.
    fn camera_zone_query_at_tile(&mut self, x: u8, z: u8) {
        let _ = (x, z);
    }

    /// Position-copy teleport onto a named actor (op 0x4C nE sub-3, 3 bytes).
    ///
    /// `[4C, 0xE3, actor_id]`. The original at lines 7208-7227 resolves
    /// the actor via `func_0x8003C83C(actor_id)`, then copies the active
    /// camera's world-space position (`_DAT_8007C364 + 0x14/+0x18`) into
    /// the actor's `+0x14/+0x18` slots and the camera's rotation
    /// (`_DAT_8007C364 + 0x16`) into the actor's `+0x16`. If the actor's
    /// `flags & 0x20000000` is set, the actor's `+0x8E` (inverted-Y mirror)
    /// is also negated to match.
    ///
    /// PC always advances by 3 (raw asm 0x801E3108: `addiu s8,s8,0x3` in
    /// the `j 0x801E00BC` branch-delay slot on the player path, the
    /// 0x801E00B8 +3 entry on the NPC path); if the actor lookup fails the
    /// original is a silent no-op. Hosts get the actor ID and decide
    /// whether to apply.
    fn teleport_to_actor(&mut self, ctx: &mut FieldCtx, actor_id: u8) {
        let _ = (ctx, actor_id);
    }
}
