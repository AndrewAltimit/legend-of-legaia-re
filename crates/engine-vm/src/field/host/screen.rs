//! The [`FieldScreenHost`] supertrait of the field-VM host surface.

use crate::field::SceneFadeResult;

/// Screen-space presentation: fades, tints, the PROT 0900 widgets, VRAM edits and lights.
///
/// One of the supertraits of [`FieldHost`](super::FieldHost), the surface the
/// field VM calls into the game through.
pub trait FieldScreenHost {
    /// Scene fade (op 0x36). The VM passes the two 16-bit operands raw and
    /// the host decides whether the fade applies immediately (PC += 5) or
    /// the scene is busy and the script must hold (PC stays).
    ///
    /// `op0_word` encodes the mode (high bit + sub-op nibble); `op1_word`
    /// carries the fade target / duration. See `docs/subsystems/script-vm.md`
    /// (opcode 0x36) for the sub-case table - the original branches based on
    /// `op0_word`'s `0xFFFF` sentinel, the `0x8000` flag bit, and the low-15
    /// sub-op.
    fn scene_fade(&mut self, op0_word: u16, op1_word: u16) -> SceneFadeResult {
        let _ = (op0_word, op1_word);
        SceneFadeResult::Done
    }

    /// Op 0x4C sub-1 - the scene colour words: ambient base colour (`0x10`),
    /// fog tint (`0x12`) and field clear colour (`0x13`).
    ///
    /// The original encodes a 7-byte instruction `[4C, op0, b1, b2, b3, b4, b5]`
    /// where `op0` selects the inner sub-op:
    /// - `0x10`: writes `_DAT_8007B7B0 = 24-bit-LE(b1..b3)`.
    /// - `0x12`: writes `DAT_8007BCB8/B9/BA = b1/b2/b3` (with optional
    ///   `func_0x8003C5F0` ramp via `LE_u16(b4..b5)`).
    /// - `0x13`: writes `DAT_8007B636/B635/B634 = b1/b2/b3` (with optional ramp).
    /// - `0x14`: the **actor clone** - see [`super::FieldActorSpawnHost::clone_actor`],
    ///   which is the hook that carries it and the one sub-op of this nibble
    ///   whose instruction is eight bytes rather than seven.
    ///
    /// PC advances by 7 (= `param_2 + 7` non-extended) for every sub-op but
    /// `0x14`. Hosts model the per-sub-op behaviour internally; the VM just
    /// hands them the operand bytes.
    fn set_scene_colour(&mut self, op0: u8, payload: &[u8; 5]) {
        let _ = (op0, payload);
    }

    /// Op 0x43 sub-0x10 (sprite-widget spawn, FUN_801F8004).
    ///
    /// 21-byte instruction. The original calls `FUN_801F8004(operand + 1)` -
    /// the PROT-0900 sprite-widget spawner with its inline 19-byte record
    /// (`engine-effects::screen_fx::SpriteRecord`). PC += 21. `payload` runs
    /// from the record to the end of the script: the widget's own script
    /// starts at record `+0x13`, on the bytes after this instruction.
    fn spawn_sprite_widget(&mut self, payload: &[u8]) {
        let _ = payload;
    }

    /// Op 0x43 sub-0x11 (screen-mask rect tween, FUN_801F8D4C).
    ///
    /// 12-byte instruction; reads 5 u16s and calls
    /// `FUN_801F8D4C(l, t, r, b, dur)` - the PROT-0900 mask (iris) widget
    /// control API (`engine-effects::screen_fx::MaskWidget`). PC += 12.
    fn tween_mask_rect(&mut self, words: [u16; 5]) {
        let _ = words;
    }

    /// Op 0x43 sub-0x15 (letterbox config, FUN_801F8F28).
    ///
    /// 14-byte instruction. The original calls `FUN_801F8F28(operand + 1)` -
    /// the PROT-0900 letterbox widget's six-i16 config
    /// (`engine-effects::screen_fx::Letterbox`). PC += 14.
    fn configure_letterbox(&mut self, payload: &[u8]) {
        let _ = payload;
    }

    /// Op 0x43 sub-0x12 (VRAM rect copy with 0x100 clamp + offset shift).
    ///
    /// 14-byte instruction `[43, 0x12, lo0, hi0, lo1, hi1, lo2, hi2, lo3, hi3,
    /// lo4, hi4, lo5, hi5]`. Reads six signed-16-bit words. The original:
    ///
    /// ```text
    /// c = words[2];                  // signed s16 from operand+5..7
    /// if (c > 0xFF) {
    ///     // First call uses (a+0xF0, b, c-0xE0, d, e+0x100, f), then c clamped.
    ///     func_0x800468a4(6, words[0]+0xF0, words[1], words[2]-0xE0,
    ///                     words[3], words[4]+0x100, words[5]);
    ///     c = 0x100;
    /// }
    /// // Second call uses (a, b, c_clamped, d, e, f).
    /// func_0x800468a4(6, words[0], words[1], c, words[3], words[4], words[5]);
    /// ```
    ///
    /// `func_0x800468a4(6, …)` enqueues a GP0 `0x80` VRAM->VRAM rectangle
    /// copy into OT slot 6 (packet builder `FUN_80057914`; `src_y += 0xF0`
    /// under the back-buffer flag) - the >256-wide dual call is the same
    /// two-page split the panel widget does. No on-disc scene script uses
    /// this sub-op. PC += 14.
    ///
    /// The VM resolves the split here (see
    /// [`vram_rect_copy::op43_sub12_calls`]) and hands the host the one or
    /// two calls in emission order, so a host only has to run each through
    /// [`vram_rect_copy::enqueue`] with its own ordering-table length and
    /// back-buffer flag.
    ///
    /// [`vram_rect_copy::op43_sub12_calls`]: crate::vram_rect_copy::op43_sub12_calls
    /// [`vram_rect_copy::enqueue`]: crate::vram_rect_copy::enqueue
    fn queue_vram_rect_copies(&mut self, calls: &[crate::vram_rect_copy::RectCopyCall]) {
        let _ = calls;
    }

    /// Op 0x43 sub-0x13 (image-panel spawn, FUN_801F88FC).
    ///
    /// 14-byte instruction `[43, 0x13, ...12 bytes]`. The original calls
    /// `FUN_801F88FC(operand)` - the PROT-0900 image-panel widget spawner,
    /// reading its `[x][y][w][h][tex_x][tex_y]` record past the sub-op byte
    /// (`engine-effects::screen_fx::PanelWidget`). PC += 14.
    fn spawn_panel_widget(&mut self, payload: &[u8; 13]) {
        let _ = payload;
    }

    /// Op 0x43 sub-0x14 (panel move/scale, FUN_801F8E6C).
    ///
    /// 10-byte instruction `[43, 0x14, lo0, hi0, lo1, hi1, lo2, hi2, lo3, hi3]`.
    /// The original calls `FUN_801F8E6C(x, y, scale, dur)` - the PROT-0900
    /// panel widget's move/scale API (`scale` is 4.12 fixed). PC += 10.
    fn move_panel_widget(&mut self, words: [i16; 4]) {
        let _ = words;
    }

    /// Op 0x34 sub-0 (effect-global colour + intensity setup).
    ///
    /// 7-byte instruction `[34, op0, r, g, b, intensity_lo, intensity_hi]`
    /// where `op0`'s low 3 bits select the fade mode. The original at
    /// `0x801E1FB0+`:
    ///
    /// - reads `intensity = signed_16(operand[3..5])` (after the 3 colour bytes),
    /// - if `_DAT_8007B62C != 0` (a fade actor is live): retires it
    ///   (`+0x10 |= 8`) and spawns a fade-out tween starting from its colour
    ///   (`FUN_801de2b0`: descriptor `0x801F2888`, handler `0x801DDC20`,
    ///   colour block copied into `+0xB8..+0xC6`) or calls
    ///   `func_0x80024e80`, depending on `_DAT_1F800394 & 0x800000`,
    /// - rewrites the `_DAT_8007BCCC..BCE0` colour-mode globals from the
    ///   operand: `BCE0 = 1` (or 2 if `op0 & 1`), `BCCC = 0x02` (or `0x00`
    ///   if `op0 & 4`, or `0x08` if `op0 & 2`), `BCCD..BCCF = r, g, b`,
    /// - if all three colour bytes are zero: clears `_DAT_8007B62C` (no
    ///   active fade); otherwise schedules a new fade with the colour and
    ///   intensity, applying the `iVar18 -= iVar18 >> 3` brightness clamp
    ///   when the colour is pure-white in mode 2.
    ///
    /// The host abstracts the fade pipeline; the VM hands over `op0`'s low
    /// nibble + RGB + intensity + the prior-fade-active hint. PC advances by 7.
    fn set_effect_colour(&mut self, op0: u8, rgb: [u8; 3], intensity: i16) {
        let _ = (op0, rgb, intensity);
    }

    /// Op 0x4C outer-nibble-4 global-slot write or ramp.
    ///
    /// Sub-ops `6 / 7` (the rumble pair, when the vibration gate allows) and
    /// `0xA / 0xB / 0xC / 0xD` write or ramp engine globals: `_DAT_8007BCD0`
    /// / `_DAT_8007BCD4` / `_DAT_8007BCD8` are the world-map horizon gate's
    /// scale / step / layer, `_DAT_8007B910` the audio duck level.
    /// Sub-`0xD` additionally multiplies the input by
    /// `_DAT_8008457C` and shifts right 12 bits - engines model the
    /// transform host-side so this hook receives the raw `target` and `sub`.
    ///
    /// `ticks == 0` means immediate write; non-zero means ramp over `ticks`
    /// frames.
    ///
    /// PC always advances by 6.
    fn write_or_ramp_global_slot(&mut self, sub: u8, target: i32, ticks: u16) {
        let _ = (sub, target, ticks);
    }

    /// Op 0x4C outer-nibble-4 sub-6 / sub-7 gate: is event vibration off?
    ///
    /// Sub-6 (`_DAT_8007B92C`) and sub-7 (`_DAT_8007B930`) are a **paired**
    /// global-slot write - the live rumble state - whose normal write/ramp
    /// path is gated by `_DAT_800845A8`, the options screen's Dual Shock
    /// "Events" word (`docs/subsystems/field-menu.md`): when the gate is
    /// **set** (non-zero, vibration off), the original
    /// short-circuits the write and instead clears **both** globals to zero
    /// via the host's [`clear_rumble`]. When the gate is
    /// clear (zero), the VM proceeds with the regular
    /// [`write_or_ramp_global_slot`] dispatch on the selected slot.
    ///
    /// Default impl returns `false` (gate clear → write proceeds), which is
    /// the retail-default state at boot.
    ///
    /// [`clear_rumble`]: Self::clear_rumble
    /// [`write_or_ramp_global_slot`]: Self::write_or_ramp_global_slot
    fn event_vibration_off(&self) -> bool {
        false
    }

    /// Op 0x4C outer-nibble-4 sub-6 / sub-7 with vibration off: clear the
    /// rumble pair.
    ///
    /// Fires when [`event_vibration_off`] returns `true`. The
    /// original zeroes `_DAT_8007B92C` and `_DAT_8007B930` together - the
    /// hook abstracts both writes into one call.
    ///
    /// [`event_vibration_off`]: Self::event_vibration_off
    fn clear_rumble(&mut self) {}

    /// Op 0x4C outer-nibble 3, sub-0 / sub-1 - the **ambient-particle master
    /// gate**.
    ///
    /// 2-byte instruction `[4C, 0x30]` (raise) / `[4C, 0x31]` (clear). Both
    /// arms write `_DAT_8007B854` from a jump delay slot and exit via
    /// `caseD_4()` - the STATE_RESUME path: `0x801E0F38` is
    /// `sw v0,-0x47ac(v1)` with `v0 = 1`, `0x801E0F44` is
    /// `sw zero,-0x47ac(v0)`, both off the 16-entry jump table at
    /// `0x801CEEB8`.
    ///
    /// The global has six references disc-wide and **none of them is pad
    /// state**: two SCUS clears (`0x800259AC`, `0x8003B690`), the field
    /// render pass's particle-table stage (`0x80026EBC`, gated on game mode
    /// `3`), the ambient particle emitter's own first load
    /// (`0x801D605C` - `lui v0,0x8008` / `lw v0,-0x47ac(v0)`, the two opening
    /// instructions of `FUN_801D6058` in the field overlay), and these two
    /// writers. So this hook raises or clears scene ambience, not input.
    ///
    /// PC advances to `pc + header_size + 1` on resume; the VM surfaces this
    /// as a `Yield` so the host's state-resume layer decides when to advance
    /// (mirrors the existing sub-7 inverted-Y branch).
    ///
    /// REF: FUN_801D6058 (the gated emitter), FUN_801D6704 (spawns it)
    fn set_ambient_particle_gate(&mut self, enabled: bool) {
        let _ = enabled;
    }

    /// Op 0x4C outer-nibble-6 op0 == 0x60 - literal-operand VRAM `MoveImage`.
    ///
    /// 14-byte instruction `[4C, 0x60, src_x, src_y, w, h, dst_x, dst_y]` -
    /// six little-endian signed-16-bit words at arbitrary byte parity (retail
    /// reads them via the misaligned-u16 helper `FUN_8003CE9C`). The handler
    /// arm (0x801E1B28..0x801E1B90 in the field overlay) hands them straight
    /// to the libgpu `MoveImage` wrapper (`jal FUN_80058490` at 0x801E1B84):
    /// copy `w x h` VRAM halfwords from `(src_x, src_y)` to `(dst_x, dst_y)`.
    /// PC += 14. Retail scripts use it for one-shot face-frame stamps onto
    /// the player texture atlas (`docs/formats/character-mesh.md`, "Runtime
    /// scroll-cell residue"). Hosts that own a software VRAM override this
    /// to queue/apply the copy (engine-core queues on the world and drains
    /// via `World::apply_script_vram_moves`).
    fn vram_move_image(&mut self, words: [i16; 6]) {
        let _ = words;
    }

    /// Op 0x4C outer-nibble-8 sub-0xA - set the **field light** (three
    /// angles and the back colour).
    ///
    /// 11-byte instruction `[4C, 0x8A, lo0, hi0, lo1, hi1, lo2, hi2, w_b0, w_b1, w_b2, w_b3]`.
    /// Writes:
    /// - `_DAT_8007B780 = signed_16(operand[1..3])`
    /// - `_DAT_8007B782 = signed_16(operand[3..5])`
    /// - `_DAT_8007B784 = signed_16(operand[5..7])`
    /// - `_DAT_8007B788 = LE_u32(operand[7..11])` (read via `func_0x8003CEB8`).
    ///
    /// PC += 11.
    fn set_field_light(&mut self, slots: [i16; 3], packed: u32) {
        let _ = (slots, packed);
    }

    /// Op 0x4C outer-nibble-E sub-0xD - set the dither word.
    ///
    /// 3-byte instruction `[4C, 0xED, value]`. Writes
    /// `_DAT_8007BA66 = value` (zero-extended u16), the word the frame-begin
    /// driver `FUN_80016B6C` copies into the DRAWENV `dtd` bit every frame
    /// (`docs/subsystems/shading.md`). PC += 3.
    fn set_dither(&mut self, value: u8) {
        let _ = value;
    }

    /// Op 0x4C outer-nibble-D sub-B - call the overlay-resident
    /// `FUN_801E57F0` then advance PC by 13 bytes.
    ///
    /// 13-byte instruction (the original's call site at line 7085 passes
    /// `pbVar47` and falls through to `LAB_801E2EA0: return param_2 + 0xD`).
    ///
    /// `FUN_801E57F0` (field overlay, 17 instructions) spawns descriptor
    /// `0x801F2930` through `FUN_80020DE0`, stores the operand pointer at
    /// `+0x90` and clears `+0x54`. The descriptor's handler is `0x801E4D8C`,
    /// the CLUT blend-to-target fade ([`legaia_engine_field::clut_fade_actor`]), which
    /// reads its 12-byte record through `+0x90`. (The Ghidra dump filed
    /// under this address is data rendered as code; the bytes in PROT 0897
    /// at this VA are the routine above.)
    ///
    /// `bytecode` is the operand stream starting at `operand + 0` (the same
    /// `pbVar47` the original passes); hosts that model this opcode read
    /// fields at `bytecode[1..=12]`.
    fn spawn_clut_blend_fade(&mut self, bytecode: &[u8]) {
        let _ = bytecode;
    }

    /// Op 0x4C outer-nibble-D sub-4 - VRAM rect read-modify-write that sets
    /// the PSX STP (semi-transparency) bit on a 16x1 pixel run.
    ///
    /// 6-byte instruction `[4C, 0xD4, x_lo, x_hi, y_lo, y_hi]`. The original
    /// (dispatcher dump lines 7621-7642) builds a `(vram_x, vram_y, w=0x10,
    /// h=1)` `RECT`, then runs the PsyQ libgs sequence
    /// `DrawSync(0); StoreImage(rect, buf16); DrawSync(0); for each of 16
    /// u16 pixels: if non-zero, OR with 0x8000; LoadImage(rect, buf16);` -
    /// i.e. it sets bit 15 (STP) on every non-zero pixel in the 16x1 rect.
    /// `StoreImage`/`LoadImage`/`DrawSync` correspond to
    /// `FUN_8005842c` / `FUN_800583c8` / `FUN_80058104` (string
    /// constants `s_StoreImage`/`s_LoadImage`/`s_DrawSync` confirm the
    /// shape). The host receives the rect origin and may emulate the
    /// read-modify-write against its own framebuffer. Default no-op.
    fn vram_stp_set(&mut self, vram_x: u16, vram_y: u16) {
        let _ = (vram_x, vram_y);
    }

    /// Op 0x4C outer-nibble-D sub-5 - VRAM rect read-modify-write that
    /// clears the PSX STP bit on a 16x1 pixel run.
    ///
    /// 6-byte instruction `[4C, 0xD5, x_lo, x_hi, y_lo, y_hi]`. Same libgs
    /// shape as sub-4 (StoreImage/LoadImage round-trip with intervening
    /// DrawSyncs) but the inner loop is `if pixel != 0x8000: pixel &=
    /// 0x7FFF` - i.e. it clears bit 15 (STP) on every pixel that isn't
    /// already "STP-only" (`0x8000` = transparent black with STP set).
    /// Default no-op.
    fn vram_stp_clear(&mut self, vram_x: u16, vram_y: u16) {
        let _ = (vram_x, vram_y);
    }
}
