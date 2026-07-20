//! GP0 `0x80` VRAM-to-VRAM rectangle copy: packet builder, ordering-table
//! emitter, and the field-VM sub-op `0x43`/`0x12` call sequencer.
//!
//! PORT: FUN_80057914, FUN_800468a4
//!
//! Three layers, matching the retail call chain:
//!
//! 1. [`build_packet`] (`FUN_80057914`) fills the six-word GP0 `0x80`
//!    primitive from a source rect plus a destination corner.
//! 2. [`enqueue`] (`FUN_800468A4`) bounds-checks the ordering-table slot,
//!    applies the back-buffer Y bias, and builds the packet for linking.
//! 3. [`op43_sub12_calls`] reproduces the field-VM arm that issues one or
//!    two [`enqueue`] calls depending on the copy width.
//!
//! The retail primitive is the same shape libgpu calls `DR_MOVE`. It is a
//! *blit*, not a draw: the GPU copies a rectangle of VRAM to another
//! location in VRAM with no texture mapping, shading or clipping.
//!
//! See [`docs/subsystems/script-vm.md`](../../../../docs/subsystems/script-vm.md)
//! for the sub-op encoding, and `ghidra/scripts/funcs/80057914.txt` /
//! `800468a4.txt` for the traced bodies.
//!
//! # Wiring status
//!
//! [`op43_sub12_calls`] is live: the field VM's sub-op `0x43`/`0x12` arm
//! calls it and hands the resolved calls to `FieldHost::op43_vram_rect_copy`.
//!
//! [`build_packet`] and [`enqueue`] are **NOT WIRED**. The host trait method
//! that receives the calls has a no-op default body and no renderer
//! implements it, so no real host runs a `RectCopyCall` through
//! [`enqueue`] - they are reachable only from this module's unit tests. A
//! wired caller would be a GP0-level host in `engine-render` that owns an
//! ordering table and a back-buffer flag to pass in. This costs nothing in
//! practice: no on-disc scene script uses sub-op `0x12`, so the arm never
//! fires on retail data.

/// A source rectangle for a VRAM-to-VRAM copy, in VRAM pixel coordinates.
///
/// Laid out in retail as the four halfwords `FUN_800468A4` builds on its
/// own stack (`local_18`/`local_16`/`local_14`/`local_12`) and passes to
/// the packet builder by pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SrcRect {
    /// Source X in VRAM pixels.
    pub x: i16,
    /// Source Y in VRAM pixels, **after** the back-buffer bias is applied
    /// by [`enqueue`].
    pub y: i16,
    /// Copy width in VRAM pixels.
    pub w: i16,
    /// Copy height in VRAM pixels.
    pub h: i16,
}

/// The assembled GP0 `0x80` primitive: six words, `0x18` bytes.
///
/// Word 0 is the ordering-table tag. The builder writes **only** its
/// high byte (the packet length); the low 24 bits are the OT next-pointer
/// and are filled in when the packet is linked into the table, which is
/// why [`tag_len`] is modelled separately from a full tag word.
///
/// [`tag_len`]: MoveImagePacket::tag_len
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MoveImagePacket {
    /// Byte `+3` of word 0: the packet length in words. `5` for a live
    /// packet, `0` when the copy is degenerate (see [`build_packet`]).
    pub tag_len: u8,
    /// Word `+0x04`, constant `0x0100_0000` in retail.
    pub word1: u32,
    /// Word `+0x08`: the GP0 command, constant `0x8000_0000` (GP0 `0x80`).
    pub command: u32,
    /// Word `+0x0C`: source corner, packed `y << 16 | x`.
    pub src: u32,
    /// Word `+0x10`: destination corner, packed `y << 16 | x`.
    pub dst: u32,
    /// Word `+0x14`: extent, packed `h << 16 | w`.
    pub extent: u32,
}

/// Word `+0x04` of the primitive. Retail writes this constant
/// unconditionally (`lui v0,0x100`).
pub const MOVE_IMAGE_WORD1: u32 = 0x0100_0000;

/// Word `+0x08`: GP0 command `0x80`, VRAM-to-VRAM copy.
pub const GP0_VRAM_TO_VRAM: u32 = 0x8000_0000;

/// Packet length written into the tag when the copy has a non-zero extent.
pub const MOVE_IMAGE_LEN_WORDS: u8 = 5;

/// Vertical bias added to the source Y while the back-buffer flag
/// (`DAT_8007B74C`) is set - the second framebuffer page starts 240 lines
/// down.
pub const BACK_BUFFER_Y_BIAS: i16 = 0xF0;

impl MoveImagePacket {
    /// `true` when the builder marked the packet dead (`tag_len == 0`).
    ///
    /// A zero-length tag makes the GPU skip the packet entirely while it
    /// still occupies its slot in the ordering table.
    pub const fn is_skipped(self) -> bool {
        self.tag_len == 0
    }
}

/// Pack a corner pair the way the GPU expects it: `y << 16 | x`.
///
/// Both halves are masked to 16 bits, mirroring the retail
/// `sll`/`andi`/`or` sequence, so negative coordinates wrap rather than
/// sign-extending into the neighbouring field.
const fn pack_yx(x: i16, y: i16) -> u32 {
    ((y as u16 as u32) << 16) | (x as u16 as u32)
}

/// Build the GP0 `0x80` packet - port of `FUN_80057914`.
///
/// The length byte is `5` for a real copy and `0` when **either** extent
/// is zero (`w == 0 || h == 0`). The sibling `MoveImage` queue
/// `FUN_80058490` in [`crate::title_prim`] kills on the *same* predicate -
/// its disassembly is the same `beq w,0` / `bne h,0` branch pair, only
/// spelled as nested `if`s by the decompiler. What differs is the
/// failure behaviour: this builder writes the packet body anyway and
/// tags it zero-length, whereas `FUN_80058490` does nothing and returns
/// `-1`.
///
/// Every other word is written unconditionally, so a skipped packet still
/// carries well-formed coordinates.
pub fn build_packet(src: SrcRect, dst_x: i16, dst_y: i16) -> MoveImagePacket {
    let tag_len = if src.w == 0 || src.h == 0 {
        0
    } else {
        MOVE_IMAGE_LEN_WORDS
    };

    MoveImagePacket {
        tag_len,
        word1: MOVE_IMAGE_WORD1,
        command: GP0_VRAM_TO_VRAM,
        src: pack_yx(src.x, src.y),
        dst: pack_yx(dst_x, dst_y),
        extent: pack_yx(src.w, src.h),
    }
}

/// One resolved `FUN_800468A4` invocation: the seven arguments the field
/// VM passes, before any bounds check or back-buffer bias.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RectCopyCall {
    /// Ordering-table slot to link the packet into. The field VM always
    /// passes `6`.
    pub ot_slot: i32,
    /// Source X, VRAM pixels.
    pub src_x: i16,
    /// Source Y, VRAM pixels, *before* the back-buffer bias.
    pub src_y: i16,
    /// Copy width, VRAM pixels.
    pub w: i16,
    /// Copy height, VRAM pixels.
    pub h: i16,
    /// Destination X, VRAM pixels.
    pub dst_x: i16,
    /// Destination Y, VRAM pixels.
    pub dst_y: i16,
}

/// Outcome of [`enqueue`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueOutcome {
    /// The ordering-table slot was outside `1 ..= ot_len - 1`; retail
    /// returns without touching the primitive buffer at all.
    SlotOutOfRange,
    /// The packet was built and is ready to link into `ot_slot`.
    Linked {
        /// Slot the caller links the packet into.
        ot_slot: i32,
        /// The assembled primitive.
        packet: MoveImagePacket,
    },
}

/// Bounds-check, bias and build - port of `FUN_800468A4`.
///
/// `ot_len` is the ordering-table length (`_DAT_1F8003A6`) and
/// `back_buffer` the framebuffer-page flag (`DAT_8007B74C`).
///
/// Retail guards with `0 < slot && slot < ot_len`, so **slot 0 is
/// rejected** along with anything past the end; the check happens before
/// the primitive buffer is advanced, so a rejected call allocates nothing.
/// When the guard passes, the source Y is biased by
/// [`BACK_BUFFER_Y_BIAS`] if the back-buffer flag is set, the packet is
/// built, and the caller links it into the table.
///
/// The bias lands on the **source** rect only - the destination corner is
/// passed through untouched.
pub fn enqueue(call: RectCopyCall, ot_len: i32, back_buffer: bool) -> EnqueueOutcome {
    if call.ot_slot <= 0 || call.ot_slot >= ot_len {
        return EnqueueOutcome::SlotOutOfRange;
    }

    let bias = if back_buffer { BACK_BUFFER_Y_BIAS } else { 0 };
    let src = SrcRect {
        x: call.src_x,
        y: call.src_y.wrapping_add(bias),
        w: call.w,
        h: call.h,
    };

    EnqueueOutcome::Linked {
        ot_slot: call.ot_slot,
        packet: build_packet(src, call.dst_x, call.dst_y),
    }
}

/// Ordering-table slot the field-VM arm targets.
pub const OP43_SUB12_OT_SLOT: i32 = 6;

/// Width above which the arm splits the copy across two VRAM pages.
pub const OP43_SUB12_SPLIT_WIDTH: i16 = 0xFF;

/// Width the second call is clamped to once a split has been emitted.
pub const OP43_SUB12_CLAMP_WIDTH: i16 = 0x100;

/// Resolve field-VM sub-op `0x43`/`0x12` into its one or two
/// `FUN_800468A4` calls.
///
/// `words` is the operand tuple `[src_x, src_y, w, h, dst_x, dst_y]`.
///
/// A copy wider than [`OP43_SUB12_SPLIT_WIDTH`] cannot be expressed as a
/// single GP0 `0x80` blit, so retail emits a **first, shifted** call
/// covering the far page and then clamps the main call's width:
///
/// ```text
/// if w > 0xFF {
///     emit(src_x + 0xF0, src_y, w - 0xE0, h, dst_x + 0x100, dst_y);
///     w = 0x100;
/// }
/// emit(src_x, src_y, w, h, dst_x, dst_y);
/// ```
///
/// The shifted call is issued **first**, so the unshifted copy lands over
/// it in the ordering table. Note the source advances by `0xF0` while the
/// destination advances by `0x100`; the two are deliberately different,
/// and the width shrinks by `0xE0` rather than the `0x100` a symmetric
/// split would use.
///
/// Returned in emission order. Arithmetic is wrapping to match the
/// retail 16-bit adds.
pub fn op43_sub12_calls(words: [i16; 6]) -> Vec<RectCopyCall> {
    let [src_x, src_y, w, h, dst_x, dst_y] = words;
    let mut calls = Vec::with_capacity(2);
    let mut main_w = w;

    if w > OP43_SUB12_SPLIT_WIDTH {
        calls.push(RectCopyCall {
            ot_slot: OP43_SUB12_OT_SLOT,
            src_x: src_x.wrapping_add(0xF0),
            src_y,
            w: w.wrapping_sub(0xE0),
            h,
            dst_x: dst_x.wrapping_add(0x100),
            dst_y,
        });
        main_w = OP43_SUB12_CLAMP_WIDTH;
    }

    calls.push(RectCopyCall {
        ot_slot: OP43_SUB12_OT_SLOT,
        src_x,
        src_y,
        w: main_w,
        h,
        dst_x,
        dst_y,
    });

    calls
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_word_layout_matches_retail() {
        let p = build_packet(
            SrcRect {
                x: 0x20,
                y: 0x30,
                w: 0x40,
                h: 0x50,
            },
            0x60,
            0x70,
        );

        assert_eq!(p.tag_len, MOVE_IMAGE_LEN_WORDS);
        assert_eq!(p.word1, 0x0100_0000);
        assert_eq!(p.command, 0x8000_0000);
        // src / dst / extent all pack as `high << 16 | low`.
        assert_eq!(p.src, 0x0030_0020);
        assert_eq!(p.dst, 0x0070_0060);
        assert_eq!(p.extent, 0x0050_0040);
        assert!(!p.is_skipped());
    }

    #[test]
    fn zero_extent_in_either_dimension_kills_the_tag() {
        // Retail: `w == 0 || h == 0` kills the packet - each alone is
        // enough. FUN_80058490 kills on the same predicate; it differs
        // only in that it queues nothing and returns -1, while this
        // builder writes the body with a zero-length tag (asserted
        // below).
        let zero_w = build_packet(
            SrcRect {
                x: 1,
                y: 2,
                w: 0,
                h: 8,
            },
            0,
            0,
        );
        let zero_h = build_packet(
            SrcRect {
                x: 1,
                y: 2,
                w: 8,
                h: 0,
            },
            0,
            0,
        );
        assert!(zero_w.is_skipped());
        assert!(zero_h.is_skipped());

        // A dead packet still carries well-formed coordinate words.
        assert_eq!(zero_w.command, GP0_VRAM_TO_VRAM);
        assert_eq!(zero_w.src, 0x0002_0001);
    }

    fn call(w: i16) -> RectCopyCall {
        RectCopyCall {
            ot_slot: 6,
            src_x: 10,
            src_y: 20,
            w,
            h: 30,
            dst_x: 40,
            dst_y: 50,
        }
    }

    #[test]
    fn slot_zero_and_overrun_are_rejected() {
        let mut c = call(8);
        c.ot_slot = 0;
        assert_eq!(enqueue(c, 16, false), EnqueueOutcome::SlotOutOfRange);

        c.ot_slot = 16;
        assert_eq!(enqueue(c, 16, false), EnqueueOutcome::SlotOutOfRange);

        c.ot_slot = -1;
        assert_eq!(enqueue(c, 16, false), EnqueueOutcome::SlotOutOfRange);

        // The last legal slot is ot_len - 1.
        c.ot_slot = 15;
        assert!(matches!(
            enqueue(c, 16, false),
            EnqueueOutcome::Linked { .. }
        ));
    }

    #[test]
    fn back_buffer_biases_source_y_only() {
        let front = enqueue(call(8), 16, false);
        let back = enqueue(call(8), 16, true);

        let EnqueueOutcome::Linked { packet: f, .. } = front else {
            panic!("front buffer should link");
        };
        let EnqueueOutcome::Linked { packet: b, .. } = back else {
            panic!("back buffer should link");
        };

        assert_eq!(f.src, pack_yx(10, 20));
        assert_eq!(b.src, pack_yx(10, 20 + BACK_BUFFER_Y_BIAS));
        // Destination is untouched by the bias.
        assert_eq!(f.dst, b.dst);
        assert_eq!(f.extent, b.extent);
    }

    #[test]
    fn narrow_copy_emits_a_single_call() {
        let calls = op43_sub12_calls([10, 20, 0xFF, 30, 40, 50]);
        assert_eq!(calls.len(), 1, "0xFF is not > 0xFF, so no split");
        assert_eq!(calls[0].w, 0xFF);
        assert_eq!(calls[0].src_x, 10);
        assert_eq!(calls[0].ot_slot, OP43_SUB12_OT_SLOT);
    }

    #[test]
    fn wide_copy_splits_shifted_call_first() {
        let calls = op43_sub12_calls([10, 20, 0x140, 30, 40, 50]);
        assert_eq!(calls.len(), 2);

        // Shifted page comes first...
        assert_eq!(calls[0].src_x, 10 + 0xF0);
        assert_eq!(calls[0].w, 0x140 - 0xE0);
        assert_eq!(calls[0].dst_x, 40 + 0x100);
        // ...and Y / height are never shifted.
        assert_eq!(calls[0].src_y, 20);
        assert_eq!(calls[0].h, 30);
        assert_eq!(calls[0].dst_y, 50);

        // ...then the clamped main copy at the original corner.
        assert_eq!(calls[1].src_x, 10);
        assert_eq!(calls[1].w, OP43_SUB12_CLAMP_WIDTH);
        assert_eq!(calls[1].dst_x, 40);
    }

    #[test]
    fn retail_fullscreen_panel_rect_splits() {
        // Every on-disc image-panel record is [0, 0, 0x140, 0xE0, 0x200, 0],
        // so the two-page split is the exercised path.
        let calls = op43_sub12_calls([0, 0, 0x140, 0xE0, 0x200, 0]);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].src_x, 0xF0);
        assert_eq!(calls[0].w, 0x60);
        assert_eq!(calls[0].dst_x, 0x300);
        assert_eq!(calls[1].w, 0x100);
        assert_eq!(calls[1].dst_x, 0x200);
    }
}
