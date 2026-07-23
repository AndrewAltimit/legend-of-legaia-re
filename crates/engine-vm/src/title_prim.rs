//! SCUS-side GPU primitive helpers the title overlay calls directly.
//!
//! PORT: FUN_80058298, FUN_80058490, FUN_800198E0
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
//! - `FUN_801E1C1C`, the largest of the overlay-side draw helpers
//!   (`overlay_801e1c1c.txt` is 8160 lines alone) and shared across the
//!   menu / battle / shop / save UI overlays. Its three siblings
//!   `FUN_801E373C` / `FUN_801E3EE0` / `FUN_801E36C4` **are** ported here,
//!   as [`exec_card_init`] / [`exec_centered_text`] / [`exec_centered_bar`]
//!   over the same [`PrimHost`].
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
//! ## NOT WIRED
//!
//! Nothing implements [`PrimHost`], so no `exec_*` entry point in this module
//! has a caller - including the title tick's own three
//! (`exec_clear_image` / `exec_move_image` / `exec_sprite_descriptor`) and
//! the four save/card-screen anchors (`FUN_801E36C4`, `FUN_801E373C`,
//! `FUN_801E3EE0`).
//!
//! The prerequisite is a **primitive-descriptor replay path**. The engine's
//! title and save screens are drawn by `legaia_engine_ui`'s `ui_title_save`
//! draw-list builders, which emit typed text / sprite draws straight from
//! screen state; they never materialise retail's GPU-packet descriptors, so
//! there is no queue for a `PrimHost` to push a `ClearImage` / `MoveImage` /
//! sprite record onto. Standing one up means giving the renderer a
//! packet-level ingest alongside the draw-list one, not implementing the
//! trait against the existing screens.
//!
//! No Sony bytes are stored in this module - only call shapes, struct
//! layouts (numeric offsets), and the dispatch control flow.
//! REF: FUN_800583C8, FUN_801DD35C, FUN_801E1C1C, FUN_801E36C4, FUN_801E373C, FUN_801E3EE0
//! REF: FUN_8002C69C, FUN_80035F04, FUN_80036888, FUN_8003CA38, FUN_801E0598
//! REF: FUN_801E435C

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

    // ------------------------------------------------------------------
    // Overlay-side helpers consumed by [`exec_centered_bar`] /
    // [`exec_centered_text`] / [`exec_card_init`]. These mirror the menu
    // / shop / save UI overlays' primitive emitters - they live behind
    // the same PrimHost so engines can wire them through a single GPU
    // back-end. Default impls are no-ops where the leaf is purely a
    // side-effect-free emit; engines override to wire to the renderer.
    // ------------------------------------------------------------------

    /// Allocate a GPU primitive packet of the given type tag, called
    /// by `FUN_801E36C4` with `packet_type = 0x44` before emitting its
    /// horizontal bar.
    ///
    /// (The retail allocator entry point for this is not yet pinned;
    /// `FUN_80034B6C` was an earlier mis-guess - that address is a
    /// one-instruction tail fragment of the number formatter
    /// `FUN_80034B78`, not a packet allocator.)
    fn prim_packet_alloc(&mut self, _packet_type: u8) {}

    /// Equivalent of `FUN_8002c69c(x, y, w, color)` - queue a horizontal
    /// bar at `(x, y)` of width `w` with the supplied color/style word.
    /// Consumed by [`exec_centered_bar`].
    fn queue_horizontal_bar(&mut self, _x: i16, _y: i16, _w: i16, _color: u32) {}

    /// Equivalent of `FUN_8003ca38()` - read the menu-text block height
    /// in pixel units. Consumed by [`exec_centered_text`].
    fn menu_text_block_height(&self) -> i32 {
        0
    }

    /// Equivalent of `FUN_80035f04(label_ptr)` - measure the text
    /// label's width in pixels. Consumed by [`exec_centered_text`].
    fn menu_text_label_width(&self, _label_ptr: u32) -> i32 {
        0
    }

    /// Equivalent of `FUN_80036888(label_ptr, mode_a, mode_b, x, y)` -
    /// queue a centered text label. Consumed by [`exec_centered_text`].
    fn queue_centered_text(
        &mut self,
        _label_ptr: u32,
        _mode_a: u16,
        _mode_b: u16,
        _x: i16,
        _y: i16,
    ) {
    }

    /// Equivalent of [`FUN_801E373C`] in full - the "card init"
    /// composite that clears three overlay globals
    /// (`DAT_801ef134`, `DAT_801ef148`, `_DAT_801f0218 = 1`), debug-
    /// prints `"init_card"`, calls `FUN_801E0598(arg)` for the
    /// card-specific pre-init, sets two timer globals
    /// (`_DAT_801f0228 = 0x78`, `_DAT_801f0224 = 1`), calls
    /// `FUN_801E435C` for the per-card finalize, and zeroes the
    /// 15-byte status buffer at `DAT_801F2A76` (rewinding from
    /// `+0xE` to `+0x0`).
    ///
    /// PORT: FUN_801E373C
    ///
    /// The retail body is a flat sequence of opaque global writes; the
    /// clean-room engine owns the UI state and rewires whatever
    /// representation it uses for the "card init" lifecycle. The trait
    /// method captures the spec; engine impls are free to map the
    /// sub-helpers (FUN_801E0598 / FUN_801E435C) to their own card
    /// state machine.
    fn init_card_state(&mut self, _arg: u32) {}
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

/// Y-axis viewport gate used by [`exec_centered_bar`] / [`exec_centered_text`]
/// to short-circuit emits that would land past the bottom of the title /
/// menu drawable area. Retail tests `param_2 < 0xF1` (signed) at both call
/// sites.
pub const TITLE_PRIM_Y_GATE: i16 = 0xF1;

/// Port of `FUN_801E36C4` - emit a centered horizontal bar at
/// `(x - w/2 - 2, y + 6)` of width `w` with the supplied color/style
/// word. The Y-axis viewport gate short-circuits when `y >= 0xF1`,
/// matching retail's `slti v0,s1,0xf1; beq v0,zero,...` guard.
///
/// PORT: FUN_801E36C4
///
/// Sequence:
///
/// 1. Gate on `y < TITLE_PRIM_Y_GATE` (else no-op).
/// 2. Allocate a primitive packet of type `0x44` via
///    [`PrimHost::prim_packet_alloc`].
/// 3. Emit the bar at `(x - w/2 - 2, y + 6, w, color)` via
///    [`PrimHost::queue_horizontal_bar`].
///
/// The `w / 2` term is C-style signed-divide rounding toward zero
/// (retail uses the `srl + addu + sra` idiom for signed division).
pub fn exec_centered_bar<H: PrimHost + ?Sized>(host: &mut H, x: i16, y: i16, w: i16, color: u32) {
    if y >= TITLE_PRIM_Y_GATE {
        return;
    }
    host.prim_packet_alloc(0x44);
    // Signed-divide-toward-zero matches retail's `srl+addu+sra` idiom:
    // `(w + (w >> 31)) >> 1`. Rust's i16 `/` already rounds toward
    // zero so a direct division is byte-equivalent.
    let half_w = w / 2;
    let bar_x = x.wrapping_sub(half_w).wrapping_sub(2);
    let bar_y = y.wrapping_add(6);
    host.queue_horizontal_bar(bar_x, bar_y, w, color);
}

/// Port of `FUN_801E3EE0` - emit a centered text label at
/// `(x - text_width/2, y + 7)` and return `(text_height + 1) / 2`
/// (half the text block height, useful for vertical-layout callers).
///
/// PORT: FUN_801E3EE0
///
/// Sequence:
///
/// 1. Gate on `y < TITLE_PRIM_Y_GATE` (else return `0`).
/// 2. Read the menu text block height via
///    [`PrimHost::menu_text_block_height`] (saved as `iVar1`).
/// 3. Measure the label width via
///    [`PrimHost::menu_text_label_width(label_ptr)`].
/// 4. Emit the label via
///    [`PrimHost::queue_centered_text(label, 0, 0, x - width/2, y + 7)`].
/// 5. Return `(iVar1 + 1) / 2`.
pub fn exec_centered_text<H: PrimHost + ?Sized>(
    host: &mut H,
    label_ptr: u32,
    x: i16,
    y: i16,
) -> i32 {
    if y >= TITLE_PRIM_Y_GATE {
        return 0;
    }
    let block_height = host.menu_text_block_height();
    let width = host.menu_text_label_width(label_ptr);
    // Signed-divide-toward-zero for the width.
    let half_w = (width / 2) as i16;
    let text_x = x.wrapping_sub(half_w);
    let text_y = y.wrapping_add(7);
    host.queue_centered_text(label_ptr, 0, 0, text_x, text_y);
    (block_height + 1) / 2
}

/// Port-thunk for `FUN_801E373C` (card-init composite).
///
/// PORT: FUN_801E373C
///
/// The retail body is a flat sequence of opaque global writes whose
/// representation is engine-specific, so the port is a single
/// trait-method dispatch into [`PrimHost::init_card_state`]. The
/// trait method's docstring carries the full retail spec.
pub fn exec_card_init<H: PrimHost + ?Sized>(host: &mut H, arg: u32) {
    host.init_card_state(arg);
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
        PrimAlloc(u8),
        HBar {
            x: i16,
            y: i16,
            w: i16,
            color: u32,
        },
        CenteredText {
            label_ptr: u32,
            mode_a: u16,
            mode_b: u16,
            x: i16,
            y: i16,
        },
        CardInit(u32),
    }

    #[derive(Default)]
    struct RecHost {
        events: RefCell<Vec<Event>>,
        alpha_gate: bool,
        text_block_height: i32,
        text_label_widths: std::collections::HashMap<u32, i32>,
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
        fn prim_packet_alloc(&mut self, packet_type: u8) {
            self.events.borrow_mut().push(Event::PrimAlloc(packet_type));
        }
        fn queue_horizontal_bar(&mut self, x: i16, y: i16, w: i16, color: u32) {
            self.events
                .borrow_mut()
                .push(Event::HBar { x, y, w, color });
        }
        fn menu_text_block_height(&self) -> i32 {
            self.text_block_height
        }
        fn menu_text_label_width(&self, label_ptr: u32) -> i32 {
            self.text_label_widths.get(&label_ptr).copied().unwrap_or(0)
        }
        fn queue_centered_text(
            &mut self,
            label_ptr: u32,
            mode_a: u16,
            mode_b: u16,
            x: i16,
            y: i16,
        ) {
            self.events.borrow_mut().push(Event::CenteredText {
                label_ptr,
                mode_a,
                mode_b,
                x,
                y,
            });
        }
        fn init_card_state(&mut self, arg: u32) {
            self.events.borrow_mut().push(Event::CardInit(arg));
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

    // ----- exec_centered_bar (FUN_801E36C4) ------------------------------

    #[test]
    fn exec_centered_bar_emits_packet_alloc_then_hbar() {
        let mut host = RecHost::default();
        exec_centered_bar(&mut host, 100, 50, 40, 0xAABBCCDD);
        let events = host.take();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], Event::PrimAlloc(0x44));
        // x_centered = x - w/2 - 2 = 100 - 20 - 2 = 78; y = 50 + 6 = 56.
        assert_eq!(
            events[1],
            Event::HBar {
                x: 78,
                y: 56,
                w: 40,
                color: 0xAABBCCDD,
            }
        );
    }

    #[test]
    fn exec_centered_bar_gates_off_screen_y() {
        let mut host = RecHost::default();
        // y == 0xF1 should gate (retail: `slti v0,s1,0xf1; beq v0,zero,...`).
        exec_centered_bar(&mut host, 100, 0xF1, 40, 0);
        // y > 0xF1 also gates.
        exec_centered_bar(&mut host, 100, 0xFF, 40, 0);
        assert!(host.take().is_empty(), "off-screen y skips both calls");
        // y = 0xF0 (just inside) does emit.
        exec_centered_bar(&mut host, 100, 0xF0, 40, 0);
        assert_eq!(host.take().len(), 2);
    }

    #[test]
    fn exec_centered_bar_signed_divide_matches_retail_idiom() {
        // The retail `srl+addu+sra` idiom is signed-divide-toward-zero.
        // For w = -5: -5/2 == -2 (toward zero), so x_centered = x - (-2) - 2 = x.
        let mut host = RecHost::default();
        exec_centered_bar(&mut host, 50, 10, -5, 0);
        let events = host.take();
        match &events[1] {
            Event::HBar { x, w, .. } => {
                assert_eq!(*w, -5);
                assert_eq!(*x, 50, "w=-5 → half_w=-2, x_centered = 50 - (-2) - 2 = 50");
            }
            other => panic!("expected HBar, got {other:?}"),
        }
    }

    // ----- exec_centered_text (FUN_801E3EE0) -----------------------------

    #[test]
    fn exec_centered_text_returns_half_height_and_emits_at_centered_position() {
        let mut host = RecHost {
            text_block_height: 11, // odd value to verify rounding
            ..Default::default()
        };
        host.text_label_widths.insert(0x8010_0000, 60);
        let half = exec_centered_text(&mut host, 0x8010_0000, 160, 100);
        // (11 + 1) / 2 = 6.
        assert_eq!(half, 6);
        // text_x = 160 - 60/2 = 130; text_y = 100 + 7 = 107.
        let events = host.take();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            Event::CenteredText {
                label_ptr: 0x8010_0000,
                mode_a: 0,
                mode_b: 0,
                x: 130,
                y: 107,
            }
        );
    }

    #[test]
    fn exec_centered_text_gated_off_screen_returns_zero_with_no_emit() {
        let mut host = RecHost {
            text_block_height: 14,
            ..Default::default()
        };
        host.text_label_widths.insert(0xDEAD_BEEF, 100);
        let r = exec_centered_text(&mut host, 0xDEAD_BEEF, 160, 0xF1);
        assert_eq!(r, 0, "off-screen y returns 0");
        assert!(host.take().is_empty(), "no emit on gate");
    }

    #[test]
    fn exec_centered_text_handles_zero_width_label() {
        let mut host = RecHost {
            text_block_height: 8,
            ..Default::default()
        };
        // No widths registered → default 0; text_x = 160 - 0 = 160.
        let r = exec_centered_text(&mut host, 0xFEED_FACE, 160, 50);
        assert_eq!(r, (8 + 1) / 2);
        let events = host.take();
        match &events[0] {
            Event::CenteredText { x, y, .. } => {
                assert_eq!(*x, 160);
                assert_eq!(*y, 57);
            }
            other => panic!("expected CenteredText, got {other:?}"),
        }
    }

    // ----- exec_card_init (FUN_801E373C) ---------------------------------

    #[test]
    fn exec_card_init_dispatches_through_trait() {
        let mut host = RecHost::default();
        exec_card_init(&mut host, 0x42);
        assert_eq!(host.take(), vec![Event::CardInit(0x42)]);
    }

    #[test]
    fn y_gate_constant_matches_retail_literal() {
        // Retail: `slti v0,s1,0xf1` in both FUN_801E36C4 and FUN_801E3EE0.
        assert_eq!(TITLE_PRIM_Y_GATE, 0xF1);
    }
}
