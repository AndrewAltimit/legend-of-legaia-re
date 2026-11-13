//! The ring a persisting battle frame reads from, as both hosts size and read
//! it.
//!
//! Retail's battle draw environment never clears: an arm that arms the wash
//! (`World::cast_frame_persists`) builds each frame on the page its double
//! buffer drew two battle frames before. A host draws one frame a vsync, so it
//! keeps the frames it presented in a ring and loads the one twice the running
//! battle frame's span back (`World::cast_frame_persist_vsyncs`): eight at the
//! step a heavy arm runs at, ten under a streaming clip's overrun vsync. The
//! native renderer's ring (`Renderer::set_frame_persist_depth`) and the play
//! page's (`webgl-psx-target.js`) are the same one; the size and the clamp
//! live here so neither restates them. See
//! `docs/subsystems/renderer-port.md`.

/// The frames the ring keeps: the deepest read it serves, two pages of the
/// longest battle frame a host asks for.
pub const FRAME_RING_SLOTS: usize = 24;

/// How far back a persisting frame reads before a host passes a depth: two
/// pages at frame step `4`.
pub const DEFAULT_PERSIST_VSYNCS: usize = 8;

/// How many presented frames back a persisting frame reads for a depth of
/// `vsyncs`, clamped to the ring: at least the last frame, at most its oldest.
pub fn persist_depth(vsyncs: u64) -> usize {
    usize::try_from(vsyncs)
        .unwrap_or(usize::MAX)
        .clamp(1, FRAME_RING_SLOTS)
}

/// The slot a read `back` frames deep loads, for a ring of `len` slots whose
/// next write lands at `next`: the newest slot, `next - 1`, holds the last
/// frame, so the one `back` frames back sits `back` slots behind `next`.
/// `back` is clamped to the ring; `None` for an empty ring.
pub fn ring_read_slot(next: usize, len: usize, back: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let back = back.clamp(1, len);
    Some((next % len + len - back) % len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_is_two_spans_clamped_to_the_ring() {
        assert_eq!(persist_depth(8), 8);
        assert_eq!(persist_depth(10), 10);
        assert_eq!(persist_depth(0), 1);
        assert_eq!(persist_depth(u64::MAX), FRAME_RING_SLOTS);
    }

    #[test]
    fn the_read_sits_back_slots_behind_the_next_write() {
        // The last frame is the newest slot.
        assert_eq!(ring_read_slot(5, 24, 1), Some(4));
        // A full ring's depth reads the oldest slot, the next write's.
        assert_eq!(ring_read_slot(5, 24, 24), Some(5));
        assert_eq!(ring_read_slot(0, 24, 8), Some(16));
        assert_eq!(ring_read_slot(3, 8, 30), Some(3));
        assert_eq!(ring_read_slot(0, 0, 8), None);
    }
}
