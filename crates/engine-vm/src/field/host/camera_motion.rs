//! The [`FieldCameraMotionHost`] supertrait of the field-VM host surface.

use super::FieldFlagsHost;
use crate::field::Sub9State;

/// Camera motion over time: the cutscene glide, the vertical-offset ease, zoom and shake.
///
/// One of the supertraits of [`FieldHost`](super::FieldHost), the surface the
/// field VM calls into the game through.
pub trait FieldCameraMotionHost: FieldFlagsHost {
    /// Op 0x4C outer-nibble-4 sub-9 story-flag dispatch state.
    ///
    /// The original at `0x801E2080+` branches on two bits of
    /// `_DAT_1F800394` (the global story-flag word in PSX scratchpad):
    ///
    /// | Bit `0x02000000` | Bit `0x01000000` | Path |
    /// |------------------|------------------|------|
    /// | clear            | clear            | `Default` - write/ramp `_DAT_801C6EA4 + 0x4A` |
    /// | clear            | set              | `PlayerRelative` - write/ramp `value + player[+0x16]` |
    /// | set              | (ignored)        | `Delta`   - write/ramp both target slot + delta global |
    ///
    /// The default impl reads the global flag word via [`global_flags`] and
    /// returns the matching variant. Hosts that don't model these specific
    /// bits can override to always return `Default`.
    ///
    /// [`global_flags`]: super::FieldFlagsHost::global_flags
    fn camera_offset_mode(&self) -> Sub9State {
        let f = self.global_flags();
        if f & 0x0200_0000 != 0 {
            Sub9State::Delta
        } else if f & 0x0100_0000 != 0 {
            Sub9State::PlayerRelative
        } else {
            Sub9State::Default
        }
    }

    /// Op 0x4C outer-nibble-4 sub-9 - player-relative write (bit 24 set).
    ///
    /// Fires when [`camera_offset_mode`] returns `PlayerRelative`. The arm
    /// writes **both** globals, not one: `_DAT_8007BCAC = target` at
    /// `0x801E1560` and `*(_DAT_801C6EA4 + 0x4A) = target + *(player + 0x16)`
    /// at `0x801E15B0` (each ramped via `func_0x8003C5F0` when `ticks != 0`).
    /// The second store is why this arm exists at all: bit 24 is also the
    /// input lock the per-frame easing `FUN_801DA390` returns on
    /// (`0x801DA398`), so while it is raised nothing else advances the
    /// accumulator. Default impl is a no-op.
    ///
    /// [`camera_offset_mode`]: Self::camera_offset_mode
    fn write_camera_offset_player_relative(&mut self, target: i16, ticks: u16) {
        let _ = (target, ticks);
    }

    /// Op 0x4C outer-nibble-4 sub-9 - default-path immediate write.
    ///
    /// Fires when [`camera_offset_mode`] returns `Default` and `ticks == 0`.
    /// The original writes `target` to `*(_DAT_801C6EA4 + 0x4A)` - a per-scene
    /// global. Default impl is a no-op.
    ///
    /// [`camera_offset_mode`]: Self::camera_offset_mode
    fn write_camera_offset(&mut self, target: i16) {
        let _ = target;
    }

    /// Op 0x4C outer-nibble-4 sub-9 - default-path ramp.
    ///
    /// Fires when [`camera_offset_mode`] returns `Default` and `ticks != 0`.
    /// The original schedules a ramp from the current value to `target` over
    /// `ticks` frames, then runs through the default-arm STATE_RESUME path.
    /// The VM surfaces a `Yield { resume_pc: pc }` after this hook fires.
    ///
    /// [`camera_offset_mode`]: Self::camera_offset_mode
    fn ramp_camera_offset(&mut self, target: i16, ticks: u16) {
        let _ = (target, ticks);
    }

    /// Op 0x4C outer-nibble-4 sub-9 - delta-path write or ramp.
    ///
    /// Fires when [`camera_offset_mode`] returns `Delta`. The original
    /// computes `delta = target - *(_DAT_8007C364 + 0x16)` (the delta from
    /// the active script-context's anchor) and writes both:
    /// - `target` → `*(_DAT_801C6EA4 + 0x4A)` (the same default-path slot).
    /// - `delta`  → `_DAT_8007BCAC` (the delta accumulator).
    ///
    /// On the ramp path (`ticks != 0`) the original schedules ramps for both
    /// slots then runs through STATE_RESUME. Engines compute the delta
    /// host-side from the target + active context - they own the anchor.
    ///
    /// [`camera_offset_mode`]: Self::camera_offset_mode
    fn write_camera_offset_delta(&mut self, target: i16, ticks: u16) {
        let _ = (target, ticks);
    }

    /// Op 0x4C outer-nibble-8 sub-4 - set the **screen-shake amplitude**.
    ///
    /// 3-byte instruction `[4C, 0x84, amplitude]`. The whole arm is five
    /// instructions (`0x801E2134`, jump-table slot `0x801CEF58`):
    ///
    /// ```text
    /// 801e2134  addiu s8,s8,0x3        ; PC += 3
    /// 801e2138  lbu   v1,0x1(s6)       ; the operand byte
    /// 801e213c  lui   v0,0x8008
    /// 801e2140  j     0x801e3624       ; the dispatcher's advance exit
    /// 801e2144  _sw   v1,-0x49d0(v0)   ; _DAT_8007B630 = operand
    /// ```
    ///
    /// The store is a full word of a zero-extended byte, so the global's
    /// value space is `0..=0xFF` and only the operand byte can set it.
    ///
    /// `_DAT_8007B630` is the amplitude `FUN_801D9D30`
    /// (`legaia_engine_camera::battle_camera::apply_shake`) turns into an LCG jitter on
    /// the camera translation pair `0x800840B8/BC`: `0` is "no shake" and
    /// `1..=0x15` widens the sample window. This opcode is that global's
    /// only writer, which is what makes the field script the sole source of
    /// a camera shake.
    ///
    /// REF: FUN_801D9D30
    fn set_shake_amplitude(&mut self, value: u8) {
        let _ = value;
    }

    /// Op 0x4C outer-nibble-D sub-7 - retire every cutscene camera mover
    /// (the `FUN_8003CF40` kill sweep keyed on `FUN_801DC0BC`).
    ///
    /// 1-byte instruction `[4C, 0xD7]`. The original sets `pcVar33 =
    /// FUN_801DC0BC` (the "delete-if-flag" predicate) then jumps to
    /// `LAB_801E2DC4`, which calls `func_0x8003CF40(_DAT_8007C34C, pcVar33)`
    /// (a list-walk that invokes `pcVar33` per entry) and returns via the
    /// standard halt-acquire dispatcher. The script resumes when the walk
    /// finishes and re-arms the field VM. The sweep (`FUN_8003CF40`) raises
    /// bit `8` (dead) in `+0x10` of every actor whose `+0x0C` handler is
    /// `FUN_801DC0BC`, so a glide in flight stops where it stands.
    fn retire_camera_movers(&mut self) {}

    /// Op 0x4C outer-nibble-C sub-D - **wait for the camera glide**.
    ///
    /// 2-byte instruction `[4C, 0xCD]`, arm at `0x801E29E0`: `s8 += 2`, then
    /// `FUN_8003CF04(_DAT_8007C34C, 0x801DC0BC)` looks up the first node on
    /// actor list 0 whose tick word `+0x0C` is the cutscene camera mover
    /// `FUN_801DC0BC`. No mover, or a mover whose `+0x10 & 0x8` (dead) bit is
    /// up, returns the advanced PC (`0x801E29FC` / `0x801E2A10`); a live one
    /// takes the restore-PC exit (`j 0x801DEE50` / `move s8,s4`), so the op
    /// re-runs next frame. The mover raises its dead bit the frame its
    /// progress `+0x9C` reaches the duration `+0x9E` (`0x801DD238..0x801DD260`),
    /// so a script that arms an op-`0x45` glide and then issues `4C CD` holds
    /// until the shot has landed.
    ///
    /// `4C D1` (`0x801E2B3C`) asks the same lookup and takes its relative
    /// skip when the answer is a live mover.
    ///
    /// Returns `true` while a glide is in flight. The default reports one
    /// always, which parks the op - the behaviour of a host that does not
    /// model the mover.
    fn camera_glide_live(&mut self) -> bool {
        true
    }

    /// Write or ramp `_DAT_80073EE4` (op 0x4C nE sub-7, 7 bytes). The word's
    /// reader is not identified, so the hook keeps its opcode coordinate.
    ///
    /// `[4C, 0xE7, t0, t1, t2, d0, d1]`. Mixed-width operand: a 24-bit LE
    /// `target` at bytes 1..3 and a 16-bit LE `duration` at bytes 4..5.
    /// (The agent's first-pass spec said both were LE24; the actual
    /// dispatcher at lines 7282-7288 calls `func_0x8003ceb8` (LE24) at
    /// `pbVar47+1` and `func_0x8003ce9c` (LE16) at `pbVar47+4`.)
    ///
    /// Behaviour:
    /// - `duration == 0` → write `_DAT_80073EE4 = target` immediately.
    /// - `duration != 0` → call `func_0x8003C5F0(0, &_DAT_80073EE4, 0,
    ///   _DAT_80073EE4, target, duration)` to schedule the tween.
    ///
    /// PC advances by 7. Hosts model the camera tween however they like.
    fn op4c_ne_sub7_ramp_73ee4(&mut self, target: u32, duration: u16) {
        let _ = (target, duration);
    }

    /// Crawl-roller control (op 0x4C nE sub-8, 10 bytes): seed the roller's
    /// three geometry words, or pause / resume / retire the live roller
    /// (`docs/subsystems/cutscene.md`, "the crawl roller").
    ///
    /// `[4C, 0xE8, x0, x1, y0, y1, z0, z1, m0, m1]`. Four 16-bit LE values:
    /// `zoom_x`, `zoom_y`, `zoom_z`, and a `mode` selector. The agent's
    /// first-pass spec said three 24-bit LE values; the actual dispatcher
    /// at lines 7300-7303 reads `func_0x8003ce9c` (LE16) four times at
    /// `pbVar47 + 1/3/5/7`.
    ///
    /// Mode dispatch (line 7305+):
    /// - `mode == 0`: default zoom. Write `_DAT_801C6EA4 + 0x4C/+0x4E/+0x50
    ///   = (zoom_x or 0x40), (zoom_y or 8), (zoom_z or 4)` - each input is
    ///   replaced with the constant when zero.
    /// - `mode == 1` and `zoom_x == 0`: set bit `0x80000` on the resolved
    ///   actor's `+0x10` flags.
    /// - `mode == 1` and `zoom_x != 0`: set per-actor zoom at
    ///   `_DAT_801C6EA4 + 0x52 = zoom_x`.
    /// - `mode == 2`: clear bit `0x80000` on the resolved actor.
    /// - `mode == 3`: set bit `8` on the resolved actor and clear flag
    ///   `0x400` on its parent actor (`*(int *)(actor + 0x94)`).
    ///
    /// PC advances by 10. Default impl is a no-op; the host owns the camera
    /// struct.
    fn configure_crawl_roller(&mut self, zoom_x: i16, zoom_y: i16, zoom_z: i16, mode: i16) {
        let _ = (zoom_x, zoom_y, zoom_z, mode);
    }
}
