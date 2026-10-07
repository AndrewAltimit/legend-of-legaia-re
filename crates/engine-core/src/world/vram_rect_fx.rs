//! The field VM's op-`0x43` sub-`0x12` **VRAM rectangle copy**, run against
//! the software VRAM both hosts share.
//!
//! `legaia_engine_vm::vram_rect_copy` carries the whole retail chain - the
//! two-page split the arm resolves, the ordering-table bounds check and
//! back-buffer bias `FUN_800468A4` applies, and the GP0 `0x80` packet
//! `FUN_80057914` builds. What it had no seat for was a GPU: the host hook
//! `FieldHost::op43_vram_rect_copy` had a no-op default and no implementation,
//! so a resolved call reached nothing.
//!
//! This module is that seat, built the same way the sibling `4C 60`
//! `MoveImage` family is ([`crate::world::ScriptVramMove`]): the hook
//! *queues*, and a host drains the queue against its own
//! [`legaia_tim::Vram`] on the frame it redraws. Keeping `World` free of a
//! renderer is why the two halves are split; both hosts already call the
//! sibling drain at the same point, so this one sits beside it.
//!
//! **A queued copy is a real GP0 primitive, not a shortcut.** Each call goes
//! through [`legaia_engine_vm::vram_rect_copy::enqueue`], so the port keeps
//! retail's two rejections: an ordering-table slot outside `1 ..= len-1`
//! copies nothing, and the source rect is biased down a display page when the
//! back-buffer flag is up. The packet built from it is then executed - a
//! blit, with no texture mapping, shading or clipping, which is exactly what
//! the PSX GPU does with the command.

use legaia_engine_vm::vram_rect_copy::{EnqueueOutcome, RectCopyCall, enqueue};

use crate::world::World;

/// Undo the primitive's `y << 16 | x` word packing (and the extent's
/// `h << 16 | w`, which is the same layout).
fn unpack_yx(word: u32) -> (u16, u16) {
    (word as u16, (word >> 16) as u16)
}

/// The ordering-table length handed to the bounds check.
///
/// Retail's is the scratchpad halfword `_DAT_1F8003A6`, written by the GPU
/// environment setup; every reference found disc-wide **reads** it, and no
/// writer is in the dump corpus, so its runtime value is not pinned here. The
/// engine has no ordering table at all - it draws from typed draw lists, not
/// a linked GP0 display list - so there is no engine value to substitute
/// either.
///
/// The drain therefore keeps the half of retail's guard that is a property of
/// the *guard* (`slot > 0`: slot 0 and negatives are rejected) and leaves the
/// upper bound open. Dropping a copy against a length the port invented would
/// be a fabricated behaviour; not dropping it is the arm's own intent, since
/// the slot it passes is the constant `6`.
pub const OT_LEN_UNBOUNDED: i32 = i32::MAX;

/// The display buffers' VRAM extent: the two `320 x 240` pages at `(0, 0)`
/// and `(0, 240)` (`legaia_engine_vm::vram_rect_copy::BACK_BUFFER_Y_BIAS`).
const FRAMEBUFFER_W: i16 = 320;
const FRAMEBUFFER_H: i16 = 480;

/// Where a host lands its drawn frame for a framebuffer copy: the front
/// display page from its first drawn row. Retail's draw offset is `(0, 4)`
/// (the `320 x 224` drawing area, `docs/subsystems/renderer.md`), and the
/// port's logical row `y` is retail's draw row `y`, so the frame's rows land
/// at VRAM rows `4..240` - a host passes its frame's top `236` rows.
pub const FRAMEBUFFER_LAND_RECT: (u16, u16, u16, u16) = (0, 4, 320, 236);

fn reads_framebuffer(c: &RectCopyCall) -> bool {
    c.src_x < FRAMEBUFFER_W && c.src_y < FRAMEBUFFER_H && c.w > 0 && c.h > 0
}

impl World {
    /// Queue the one or two `FUN_800468A4` calls the field-VM arm resolved -
    /// the `FieldHost::op43_vram_rect_copy` host hook.
    ///
    /// Drained by [`Self::apply_vram_rect_copies`].
    pub fn queue_vram_rect_copies(&mut self, calls: &[RectCopyCall]) {
        self.ambient.vram_rect_copies.extend_from_slice(calls);
    }

    /// Drain the queued rect copies into `vram`. Returns `true` when anything
    /// was copied, which is the host's signal to re-upload its GPU mirror -
    /// the same contract [`Self::apply_script_vram_moves`] has.
    ///
    /// `back_buffer` is the framebuffer-page flag `DAT_8007B74C`; a host that
    /// does not track a page passes `false`, which is the front-buffer case.
    ///
    /// PORT: FUN_800468a4 (the consumer seat: `FieldHost::op43_vram_rect_copy`
    /// through `legaia_engine_vm::vram_rect_copy::enqueue`)
    pub fn apply_vram_rect_copies(
        &mut self,
        vram: &mut legaia_tim::Vram,
        back_buffer: bool,
    ) -> bool {
        let mut wrote = false;
        let grab = self.ambient.frame_grab;
        let hold = grab.enabled && !grab.landed;
        let mut held = Vec::new();
        let mut read_frame = false;
        for call in std::mem::take(&mut self.ambient.vram_rect_copies) {
            if reads_framebuffer(&call) {
                if hold {
                    held.push(call);
                    continue;
                }
                read_frame = true;
            }
            let EnqueueOutcome::Linked { packet, .. } =
                enqueue(call, OT_LEN_UNBOUNDED, back_buffer)
            else {
                continue;
            };
            // A zero-extent packet is tagged zero-length by the builder and
            // the GPU skips it, so the drain does too.
            if packet.is_skipped() {
                continue;
            }
            let (sx, sy) = unpack_yx(packet.src);
            let (dx, dy) = unpack_yx(packet.dst);
            let (w, h) = unpack_yx(packet.extent);
            vram.move_image(sx, sy, w, h, dx, dy);
            wrote = true;
        }
        self.ambient.vram_rect_copies = held;
        if read_frame {
            self.ambient.frame_grab.landed = false;
        }
        wrote
    }

    /// `true` while a queued rect copy reads the display framebuffer and the
    /// host has not landed a frame for it yet - the host's cue to read its
    /// last drawn frame back into [`FRAMEBUFFER_LAND_RECT`] and call
    /// [`Self::land_framebuffer`]. Always `false` unless the host opted in
    /// ([`crate::world::FrameGrab::enabled`]).
    pub fn framebuffer_grab_pending(&self) -> bool {
        let g = self.ambient.frame_grab;
        g.enabled && !g.landed && self.ambient.vram_rect_copies.iter().any(reads_framebuffer)
    }

    /// The host has written its frame into the display rect: the held
    /// framebuffer copies run on the next drain.
    pub fn land_framebuffer(&mut self) {
        self.ambient.frame_grab.landed = true;
    }

    /// Opt a host in to the frame-grab handshake ([`crate::world::FrameGrab`]).
    pub fn enable_frame_grab(&mut self, on: bool) {
        self.ambient.frame_grab.enabled = on;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(src: (i16, i16), size: (i16, i16), dst: (i16, i16)) -> RectCopyCall {
        RectCopyCall {
            ot_slot: legaia_engine_vm::vram_rect_copy::OP43_SUB12_OT_SLOT,
            src_x: src.0,
            src_y: src.1,
            w: size.0,
            h: size.1,
            dst_x: dst.0,
            dst_y: dst.1,
        }
    }

    fn stamp(vram: &mut legaia_tim::Vram, x: u16, y: u16, v: u16) {
        vram.write_block(x, y, 1, 1, &v.to_le_bytes());
    }

    #[test]
    fn a_queued_copy_moves_the_rect() {
        let mut w = World::new();
        let mut vram = legaia_tim::Vram::new();
        stamp(&mut vram, 10, 20, 0xBEEF);
        w.queue_vram_rect_copies(&[call((10, 20), (1, 1), (300, 40))]);
        assert!(w.apply_vram_rect_copies(&mut vram, false));
        assert_eq!(vram.pixel(300, 40), 0xBEEF);
        // The queue is drained, so a second pass is a no-op.
        assert!(!w.apply_vram_rect_copies(&mut vram, false));
    }

    #[test]
    fn the_back_buffer_flag_biases_the_source_only() {
        let mut w = World::new();
        let mut vram = legaia_tim::Vram::new();
        // Same source X, one display page down.
        let bias = legaia_engine_vm::vram_rect_copy::BACK_BUFFER_Y_BIAS as u16;
        stamp(&mut vram, 10, 20 + bias, 0x1234);
        w.queue_vram_rect_copies(&[call((10, 20), (1, 1), (300, 41))]);
        assert!(w.apply_vram_rect_copies(&mut vram, true));
        assert_eq!(vram.pixel(300, 41), 0x1234);
    }

    #[test]
    fn an_out_of_range_ordering_slot_copies_nothing() {
        let mut w = World::new();
        let mut vram = legaia_tim::Vram::new();
        stamp(&mut vram, 10, 20, 0xAAAA);
        let mut c = call((10, 20), (1, 1), (300, 42));
        c.ot_slot = 0; // retail rejects slot 0 along with anything past the end
        w.queue_vram_rect_copies(&[c]);
        assert!(!w.apply_vram_rect_copies(&mut vram, false));
        assert_eq!(vram.pixel(300, 42), 0);
    }

    #[test]
    fn a_zero_extent_copy_writes_nothing() {
        let mut w = World::new();
        let mut vram = legaia_tim::Vram::new();
        stamp(&mut vram, 10, 20, 0x5555);
        w.queue_vram_rect_copies(&[call((10, 20), (0, 4), (300, 43))]);
        assert!(!w.apply_vram_rect_copies(&mut vram, false));
        assert_eq!(vram.pixel(300, 43), 0);
    }

    /// With the host opted in, a copy that reads the display framebuffer
    /// (the ending vignettes' `43 12` photo grab) waits until the host lands
    /// a frame; a copy elsewhere in VRAM runs at once either way.
    #[test]
    fn a_framebuffer_copy_waits_for_the_landed_frame() {
        let mut w = World::new();
        w.enable_frame_grab(true);
        let mut vram = legaia_tim::Vram::new();
        stamp(&mut vram, 600, 300, 0x0F0F);
        w.queue_vram_rect_copies(&[
            call((0, 0), (1, 1), (512, 0)),
            call((600, 300), (1, 1), (700, 300)),
        ]);
        assert!(w.framebuffer_grab_pending());
        assert!(w.apply_vram_rect_copies(&mut vram, false));
        assert_eq!(vram.pixel(700, 300), 0x0F0F);
        assert_eq!(vram.pixel(512, 0), 0);
        stamp(&mut vram, 0, 0, 0x7FFF);
        w.land_framebuffer();
        assert!(!w.framebuffer_grab_pending());
        assert!(w.apply_vram_rect_copies(&mut vram, false));
        assert_eq!(vram.pixel(512, 0), 0x7FFF);
        // The landing is spent on the copy it fed.
        w.queue_vram_rect_copies(&[call((0, 0), (1, 1), (513, 0))]);
        assert!(w.framebuffer_grab_pending());
    }

    /// The VM hands the arm's two-page split through as two calls; both land.
    #[test]
    fn both_calls_of_a_wide_split_land() {
        let mut w = World::new();
        let mut vram = legaia_tim::Vram::new();
        stamp(&mut vram, 5, 6, 0x0101);
        stamp(&mut vram, 7, 8, 0x0202);
        w.queue_vram_rect_copies(&[
            call((5, 6), (1, 1), (400, 10)),
            call((7, 8), (1, 1), (401, 10)),
        ]);
        assert!(w.apply_vram_rect_copies(&mut vram, false));
        assert_eq!(vram.pixel(400, 10), 0x0101);
        assert_eq!(vram.pixel(401, 10), 0x0202);
    }
}
