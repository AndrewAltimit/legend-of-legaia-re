//! SCUS-side GPU primitive helpers the title overlay calls directly.
//!
//! The title-overlay per-frame tick `FUN_801DD35C` (modelled in
//! [`crate::title_overlay`]) reaches into three SCUS-side helpers to
//! emit GPU primitives:
//!
//! | Helper             | Purpose                                      |
//! | ------------------ | -------------------------------------------- |
//! | `FUN_80058298`     | Queue a `ClearImage` (GP0 fill-rect) for one |
//! |                    | 8-byte rect, color = `(r, g, b)`.            |
//! | `FUN_80058490`     | Queue a `MoveImage` (VRAM-to-VRAM copy) for  |
//! |                    | an 8-byte source rect to a destination point.|
//! | `FUN_800198E0`     | Dispatch a "sprite descriptor" - a header    |
//! |                    | record that selects between a simple sprite  |
//! |                    | emit (tag `0x11`) and a multi-pass variant   |
//! |                    | (alpha-OR + sprite emit).                    |
//!
//! All three call a fourth helper `FUN_800583C8` that actually issues
//! the GPU primitive. From the perspective of the title-overlay tick,
//! `FUN_800583C8` is the leaf - so this module's [`PrimHost`] trait
//! exposes the queue-a-sprite call as a single method
//! ([`PrimHost::emit_sprite`]) and the engine implementation wires
//! that to whatever GPU back-end it has.
//!
//! ## Why this lives in `engine-vm` rather than `engine-render`
//!
//! Clean-room boundary: the *protocol* (descriptor struct shape, tag
//! routing, alpha-OR pass) is a faithful port of the disassembled
//! control flow; the *implementation* (real wgpu draws) lives in the
//! engine layer that owns VRAM. Same pattern as
//! [`crate::actor_tick`] / [`crate::field`] / etc. - the VM defines
//! shapes and dispatch; the engine fulfils the host trait.
//!
//! ## What's deferred
//!
//! - The four overlay-side draw helpers
//!   (`FUN_801E1C1C` / `FUN_801E373C` / `FUN_801E3EE0` / `FUN_801E36C4`)
//!   are MUCH larger (`overlay_801e1c1c.txt` is 8160 lines alone) and
//!   shared across menu / battle / shop / save UI overlays. They warrant
//!   their own focused port. The title-tick body calls them by address;
//!   for now those calls can be stubbed via the same [`PrimHost`].
//! - The TPage-cache write at the tail of `FUN_800198E0` (writes
//!   `desc[+0x0E]` into a LUT indexed by `(x >> 6, tpage_byte)`).
//!   That's a deduplication optimisation - the host can replay every
//!   sprite emit without it.
//! - The `flags & 3` sub-dispatch inside the non-`0x11` descriptor
//!   path that picks a width-divisor variant (div-2 / div-4 / raw /
//!   skip). The default raw path is the only one the title tick is
//!   observed using; the others are gated for future tracing.
//!
//! ## Provenance
//!
//! - `FUN_80058298` decomp: `ghidra/scripts/funcs/80058298.txt`
//!   (37 instructions, 148 bytes).
//! - `FUN_80058490` decomp: `ghidra/scripts/funcs/80058490.txt`
//!   (49 instructions, 196 bytes).
//! - `FUN_800198E0` decomp: `ghidra/scripts/funcs/800198e0.txt`
//!   (146 instructions, 584 bytes).
//!
//! No Sony bytes are stored in this module - only call shapes, struct
//! layouts (numeric offsets), and the dispatch control flow.

#![forbid(unsafe_code)]

/// 8-byte source/destination rectangle, matching the layout the title
/// overlay tick builds on its stack frame at `sp+0x38..+0x40` and the
/// layout the SCUS helpers consume.
///
/// All four fields are signed 16-bit. The width / height fields are
/// `i16` because the dispatch path inside [`SpriteDescriptor`] uses
/// signed compares (`bgez`) and signed-divide adjustments.
///
/// Disassembly source: `FUN_80058298` reads x/y at `sp+0x38, +0x3A`
/// and w/h at `sp+0x3C, +0x3E` (16-bit fields).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect12 {
    pub x: i16,
    pub y: i16,
    pub w: i16,
    pub h: i16,
}

impl Rect12 {
    pub const fn new(x: i16, y: i16, w: i16, h: i16) -> Self {
        Self { x, y, w, h }
    }

    /// Decode from an 8-byte little-endian slice. Returns `None` if
    /// `bytes.len() < 8`.
    pub fn from_le_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 8 {
            return None;
        }
        Some(Self {
            x: i16::from_le_bytes([bytes[0], bytes[1]]),
            y: i16::from_le_bytes([bytes[2], bytes[3]]),
            w: i16::from_le_bytes([bytes[4], bytes[5]]),
            h: i16::from_le_bytes([bytes[6], bytes[7]]),
        })
    }

    /// Encode as 8 little-endian bytes.
    pub fn to_le_bytes(self) -> [u8; 8] {
        let [x0, x1] = self.x.to_le_bytes();
        let [y0, y1] = self.y.to_le_bytes();
        let [w0, w1] = self.w.to_le_bytes();
        let [h0, h1] = self.h.to_le_bytes();
        [x0, x1, y0, y1, w0, w1, h0, h1]
    }

    /// `true` when both `w == 0` and `h == 0` (the early-out condition
    /// inside [`exec_move_image`]).
    pub const fn is_zero_size(self) -> bool {
        self.w == 0 && self.h == 0
    }
}

/// Result code mirrored from `FUN_80058490` (`MoveImage` queue). The
/// retail return value is `0xFFFFFFFF` on early-out and the queue
/// helper's own return value otherwise; this module reduces that to a
/// boolean since the engine layer doesn't propagate the underlying
/// libgpu handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveImageOutcome {
    /// `w == 0` or `h == 0` - the original returns `0xFFFFFFFF`
    /// without queueing.
    SkippedZeroExtent,
    /// The MoveImage was queued. The retail tail returns the queue
    /// helper's own return value here; this port only reports the
    /// dispatch.
    Queued,
}

/// Sprite descriptor record consumed by [`exec_sprite_descriptor`].
///
/// On-disc / in-RAM, the descriptor is a variable-length byte stream;
/// this struct captures just the fields the dispatcher reads. Two
/// shape variants share the leading u32 `tag`:
///
/// - **Simple** (`tag == 0x11`): exactly one sprite. Inline rect at
///   bytes `+0x0C..+0x14`; pixel-data pointer at `+0x14..`.
/// - **Complex** (`tag != 0x11`): up to two sprites. `flags` (bytes
///   `+0x04..+0x08`):
///   - bit 3 set: the dispatcher first runs an "alpha-OR" pass on the
///     pixel array at `+0x14`, ORing `0x8000` into each non-zero u16
///     pixel (the PSX semi-transparency bit). This pass only fires
///     when global `_DAT_8007B998` is non-zero. Then it emits a
///     sprite from the same `+0x0C..+0x14` rect.
///   - bit 3 clear: skip the alpha pass.
///   - low 2 bits of `flags` select a width-divisor variant on the
///     main sprite emit (div-2 / div-4 / raw / skip). This module
///     only honours the raw / div-2 / div-4 paths because the title
///     tick is only observed using raw; the "skip" case is recorded
///     via [`SpriteEmitVariant::SkipMainSprite`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteDescriptor {
    /// `desc[0]` (u32). `0x11` selects the simple sprite variant;
    /// every other value selects the complex variant.
    pub tag: u32,
    /// `desc[+0x04]` (u32). Bit 3 (`& 8`) gates the alpha-OR pre-pass;
    /// the low 2 bits select a width-divisor variant on the main
    /// sprite. Ignored when [`tag`] == `0x11`.
    ///
    /// [`tag`]: SpriteDescriptor::tag
    pub flags: u32,
    /// `desc[+0x0C..+0x14]` - the inline 8-byte sprite rect. Both
    /// variants emit at least one sprite from this rect.
    pub rect: Rect12,
    /// `desc[+0x14..]` - the pixel-data pointer the dispatcher passes
    /// as the second arg of `FUN_800583C8`. Represented here as a
    /// `u32` (the PSX virtual address); the host translates it to
    /// real bytes / texture handles as it sees fit.
    pub pixel_data_ptr: u32,
}

/// Width-divisor variant the dispatcher picks for the main sprite
/// emit based on `flags & 3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpriteEmitVariant {
    /// `flags & 3 == 2`: width is `desc[+0x10]` verbatim.
    /// (Also used as the default for the tag-`0x11` simple path.)
    Raw,
    /// `flags & 3 == 1`: width is `desc[+0x10] / 2` with signed
    /// rounding (matches `sra v1, t3, 1`).
    HalfWidth,
    /// `flags & 3 == 0`: width is `desc[+0x10] / 4` with signed
    /// rounding (matches `addiu v0, v0, 3; sra v0, 2`).
    QuarterWidth,
    /// `flags & 3 == 3`: skip the main sprite emit entirely (the
    /// width-decode arm has no `sh v0, 0x14(sp)` store).
    SkipMainSprite,
}

impl SpriteEmitVariant {
    /// Decode from the low 2 bits of [`SpriteDescriptor::flags`].
    pub const fn from_flags(flags: u32) -> Self {
        match flags & 0b11 {
            0 => Self::QuarterWidth,
            1 => Self::HalfWidth,
            2 => Self::Raw,
            _ => Self::SkipMainSprite, // 0b11
        }
    }
}

/// Engine-side callbacks the SCUS helpers dispatch into.
///
/// Implementations live in the engine layer (e.g. `engine-core` /
/// `engine-render`) and are free to back the primitive queue with any
/// GPU back-end. The trait methods deliberately mirror the helper
/// call shapes - one method per disassembled helper.
pub trait PrimHost {
    /// Equivalent of `FUN_80058298` - queue a `ClearImage`
    /// (GP0 fill-rect) for `rect` with color `(r, g, b)`.
    fn queue_clear_rect(&mut self, rect: Rect12, r: u8, g: u8, b: u8);

    /// Equivalent of `FUN_80058490` - queue a `MoveImage` (GP0
    /// VRAM-to-VRAM copy). The dispatch behaviour (early-out on
    /// zero extent) is handled by [`exec_move_image`]; this method
    /// only fires on the queue path.
    fn queue_move_image(&mut self, src: Rect12, dst_x: u16, dst_y: i16);

    /// Equivalent of `FUN_800583C8` - queue a sprite primitive
    /// described by `rect` with pixel data at `pixel_data_ptr`.
    ///
    /// `is_alpha_or_pass`: `true` when this emit is the pre-pass that
    /// `FUN_800198E0` runs under `flags & 8`. The host can use this
    /// to apply the appropriate STP / semi-transparency state.
    /// `variant`: the width-divisor variant selected by `flags & 3`
    /// (always [`SpriteEmitVariant::Raw`] for the alpha-OR pre-pass
    /// and the tag-`0x11` path).
    fn emit_sprite(
        &mut self,
        rect: Rect12,
        pixel_data_ptr: u32,
        is_alpha_or_pass: bool,
        variant: SpriteEmitVariant,
    );

    /// Read the global `_DAT_8007B998` gate that
    /// [`exec_sprite_descriptor`] checks before performing the
    /// alpha-OR pre-pass.
    ///
    /// In retail this global is set by an unrelated subsystem; the
    /// engine port has to decide whether to enable it (its purpose
    /// is to force every non-transparent pixel to gain the STP bit,
    /// effectively a "darken non-transparent pixels" toggle).
    fn alpha_or_gate_set(&self) -> bool;
}

/// Port of `FUN_80058298` (`ClearImage` rect-fill queue).
///
/// Always queues - there is no early-out in the original.
pub fn exec_clear_image<H: PrimHost>(host: &mut H, rect: Rect12, r: u8, g: u8, b: u8) {
    host.queue_clear_rect(rect, r, g, b);
}

/// Port of `FUN_80058490` (`MoveImage` VRAM-copy queue).
///
/// Returns [`MoveImageOutcome::SkippedZeroExtent`] when either
/// dimension is zero, matching the original's `li v0, -0x1` early-out
/// path (`*(short *)(rect + 4) == 0 || *(short *)(rect + 6) == 0`).
pub fn exec_move_image<H: PrimHost>(
    host: &mut H,
    src: Rect12,
    dst_x: u16,
    dst_y: i16,
) -> MoveImageOutcome {
    if src.w == 0 || src.h == 0 {
        return MoveImageOutcome::SkippedZeroExtent;
    }
    host.queue_move_image(src, dst_x, dst_y);
    MoveImageOutcome::Queued
}

/// Port of `FUN_800198E0` (sprite-descriptor dispatcher).
///
/// Routes between the simple (tag `0x11`) and complex variants;
/// performs the alpha-OR pre-pass when `flags & 8` is set AND the
/// host's [`PrimHost::alpha_or_gate_set`] returns `true`.
pub fn exec_sprite_descriptor<H: PrimHost>(host: &mut H, desc: &SpriteDescriptor) {
    if desc.tag == 0x11 {
        // Simple variant: one sprite emit with `Raw` width.
        host.emit_sprite(
            desc.rect,
            desc.pixel_data_ptr,
            false,
            SpriteEmitVariant::Raw,
        );
        return;
    }

    // Complex variant. Optional alpha-OR pre-pass, then the main sprite.
    if (desc.flags & 0x08) != 0 && host.alpha_or_gate_set() {
        // The original's pre-pass emits a sprite of size (rect.w * rect.h, 1) -
        // i.e. it treats the pixel array as a 1-pixel-tall strip whose width is
        // the total pixel count. (Matches `local_1c = (short)param_1[4] *
        // *(short *)((int)param_1 + 0x12); local_1a = 1;`.)
        let strip = Rect12 {
            x: desc.rect.x,
            y: desc.rect.y,
            w: desc.rect.w.saturating_mul(desc.rect.h),
            h: 1,
        };
        host.emit_sprite(
            strip,
            desc.pixel_data_ptr,
            true, // is_alpha_or_pass
            SpriteEmitVariant::Raw,
        );
    }

    // Main sprite emit, picking the width-divisor variant from flags & 3.
    let variant = SpriteEmitVariant::from_flags(desc.flags);
    match variant {
        SpriteEmitVariant::SkipMainSprite => {
            // The `flags & 3 == 3` arm in the original has no
            // `sh v0, 0x14(sp)` write, so the sprite emit reads a
            // stale stack slot. The cleanest port is to skip the
            // emit; the host can opt in to a different behaviour by
            // checking the variant.
        }
        _ => {
            host.emit_sprite(desc.rect, desc.pixel_data_ptr, false, variant);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Event {
        Clear {
            rect: Rect12,
            rgb: (u8, u8, u8),
        },
        Move {
            src: Rect12,
            dst: (u16, i16),
        },
        Sprite {
            rect: Rect12,
            pixel_data_ptr: u32,
            is_alpha: bool,
            variant: SpriteEmitVariant,
        },
    }

    #[derive(Default)]
    struct RecHost {
        events: RefCell<Vec<Event>>,
        alpha_gate: bool,
    }

    impl RecHost {
        fn take(&self) -> Vec<Event> {
            std::mem::take(&mut self.events.borrow_mut())
        }
    }

    impl PrimHost for RecHost {
        fn queue_clear_rect(&mut self, rect: Rect12, r: u8, g: u8, b: u8) {
            self.events.borrow_mut().push(Event::Clear {
                rect,
                rgb: (r, g, b),
            });
        }
        fn queue_move_image(&mut self, src: Rect12, dst_x: u16, dst_y: i16) {
            self.events.borrow_mut().push(Event::Move {
                src,
                dst: (dst_x, dst_y),
            });
        }
        fn emit_sprite(
            &mut self,
            rect: Rect12,
            pixel_data_ptr: u32,
            is_alpha_or_pass: bool,
            variant: SpriteEmitVariant,
        ) {
            self.events.borrow_mut().push(Event::Sprite {
                rect,
                pixel_data_ptr,
                is_alpha: is_alpha_or_pass,
                variant,
            });
        }
        fn alpha_or_gate_set(&self) -> bool {
            self.alpha_gate
        }
    }

    #[test]
    fn rect12_round_trips_le_bytes() {
        let r = Rect12::new(-3, 4, 320, 240);
        let bytes = r.to_le_bytes();
        let back = Rect12::from_le_bytes(&bytes).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn rect12_from_le_bytes_rejects_short_input() {
        assert!(Rect12::from_le_bytes(&[0u8; 7]).is_none());
    }

    #[test]
    fn rect12_is_zero_size_only_when_both_dimensions_zero() {
        assert!(Rect12::new(0, 0, 0, 0).is_zero_size());
        assert!(!Rect12::new(0, 0, 1, 0).is_zero_size());
        assert!(!Rect12::new(0, 0, 0, 1).is_zero_size());
    }

    #[test]
    fn exec_clear_image_always_queues() {
        let mut host = RecHost::default();
        let r = Rect12::new(0, 0, 320, 4);
        exec_clear_image(&mut host, r, 0x10, 0x20, 0x30);
        assert_eq!(
            host.take(),
            vec![Event::Clear {
                rect: r,
                rgb: (0x10, 0x20, 0x30),
            }]
        );
    }

    #[test]
    fn exec_move_image_skips_zero_extent() {
        let mut host = RecHost::default();
        let outcome = exec_move_image(&mut host, Rect12::new(0, 0, 0, 64), 100, 50);
        assert_eq!(outcome, MoveImageOutcome::SkippedZeroExtent);
        let outcome = exec_move_image(&mut host, Rect12::new(0, 0, 64, 0), 100, 50);
        assert_eq!(outcome, MoveImageOutcome::SkippedZeroExtent);
        assert!(host.take().is_empty(), "no queue on zero extent");
    }

    #[test]
    fn exec_move_image_queues_when_both_dimensions_nonzero() {
        let mut host = RecHost::default();
        let src = Rect12::new(10, 20, 30, 40);
        let outcome = exec_move_image(&mut host, src, 100, 50);
        assert_eq!(outcome, MoveImageOutcome::Queued);
        assert_eq!(
            host.take(),
            vec![Event::Move {
                src,
                dst: (100, 50),
            }]
        );
    }

    #[test]
    fn tag_0x11_descriptor_emits_one_raw_sprite() {
        let mut host = RecHost::default();
        let desc = SpriteDescriptor {
            tag: 0x11,
            flags: 0xFFFF_FFFF, // intentionally ignored on tag-0x11
            rect: Rect12::new(64, 32, 128, 64),
            pixel_data_ptr: 0x8010_1234,
        };
        exec_sprite_descriptor(&mut host, &desc);
        assert_eq!(
            host.take(),
            vec![Event::Sprite {
                rect: desc.rect,
                pixel_data_ptr: desc.pixel_data_ptr,
                is_alpha: false,
                variant: SpriteEmitVariant::Raw,
            }]
        );
    }

    #[test]
    fn complex_descriptor_without_alpha_emits_one_sprite_with_variant() {
        let mut host = RecHost::default();
        let desc = SpriteDescriptor {
            tag: 0x42,
            flags: 0x02, // bit3 clear, low2 = 0b10 = Raw
            rect: Rect12::new(0, 0, 16, 16),
            pixel_data_ptr: 0x8020_0000,
        };
        exec_sprite_descriptor(&mut host, &desc);
        assert_eq!(
            host.take(),
            vec![Event::Sprite {
                rect: desc.rect,
                pixel_data_ptr: desc.pixel_data_ptr,
                is_alpha: false,
                variant: SpriteEmitVariant::Raw,
            }]
        );
    }

    #[test]
    fn complex_descriptor_with_alpha_bit_runs_pre_pass_when_gate_set() {
        let mut host = RecHost {
            alpha_gate: true,
            ..Default::default()
        };
        let desc = SpriteDescriptor {
            tag: 0x42,
            flags: 0x08 | 0x02, // bit3 set, low2 = Raw
            rect: Rect12::new(5, 7, 4, 3),
            pixel_data_ptr: 0x8030_0000,
        };
        exec_sprite_descriptor(&mut host, &desc);
        assert_eq!(
            host.take(),
            vec![
                Event::Sprite {
                    rect: Rect12::new(5, 7, 12, 1), // w * h, h=1
                    pixel_data_ptr: 0x8030_0000,
                    is_alpha: true,
                    variant: SpriteEmitVariant::Raw,
                },
                Event::Sprite {
                    rect: desc.rect,
                    pixel_data_ptr: 0x8030_0000,
                    is_alpha: false,
                    variant: SpriteEmitVariant::Raw,
                },
            ]
        );
    }

    #[test]
    fn complex_descriptor_with_alpha_bit_skips_pre_pass_when_gate_clear() {
        let mut host = RecHost::default();
        // alpha_gate defaults to false
        let desc = SpriteDescriptor {
            tag: 0x42,
            flags: 0x08 | 0x02,
            rect: Rect12::new(0, 0, 8, 4),
            pixel_data_ptr: 0x8040_0000,
        };
        exec_sprite_descriptor(&mut host, &desc);
        let events = host.take();
        assert_eq!(events.len(), 1, "no pre-pass when gate is clear");
        assert!(matches!(
            &events[0],
            Event::Sprite {
                is_alpha: false,
                ..
            }
        ));
    }

    #[test]
    fn sprite_emit_variant_decodes_low_two_flag_bits() {
        assert_eq!(
            SpriteEmitVariant::from_flags(0),
            SpriteEmitVariant::QuarterWidth
        );
        assert_eq!(
            SpriteEmitVariant::from_flags(1),
            SpriteEmitVariant::HalfWidth
        );
        assert_eq!(SpriteEmitVariant::from_flags(2), SpriteEmitVariant::Raw);
        assert_eq!(
            SpriteEmitVariant::from_flags(3),
            SpriteEmitVariant::SkipMainSprite
        );
        // Upper bits ignored.
        assert_eq!(
            SpriteEmitVariant::from_flags(0xFFFF_FFFC),
            SpriteEmitVariant::QuarterWidth
        );
    }

    #[test]
    fn skip_main_sprite_variant_emits_no_main_sprite() {
        let mut host = RecHost::default();
        let desc = SpriteDescriptor {
            tag: 0x42,
            flags: 0x03, // bit3 clear, low2 = 0b11 = SkipMainSprite
            rect: Rect12::new(0, 0, 16, 16),
            pixel_data_ptr: 0x8050_0000,
        };
        exec_sprite_descriptor(&mut host, &desc);
        assert!(
            host.take().is_empty(),
            "SkipMainSprite variant emits nothing"
        );
    }

    #[test]
    fn alpha_pre_pass_runs_even_when_main_sprite_skipped() {
        // flags = 0x08 | 0x03 → pre-pass fires (bit3+gate), but the
        // main sprite is skipped (low 2 == SkipMainSprite). The
        // dispatcher emits exactly one sprite (the pre-pass).
        let mut host = RecHost {
            alpha_gate: true,
            ..Default::default()
        };
        let desc = SpriteDescriptor {
            tag: 0x42,
            flags: 0x08 | 0x03,
            rect: Rect12::new(1, 2, 3, 4),
            pixel_data_ptr: 0x8060_0000,
        };
        exec_sprite_descriptor(&mut host, &desc);
        let events = host.take();
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], Event::Sprite { is_alpha: true, .. }));
    }
}
