use super::pose_codec::{encode_channel_delta, pack_part};
use super::victory_loop::{LoopWindow, compose_base_stream};
use super::*;
use legaia_actor_formats::me_archive;

#[test]
fn channel_delta_roundtrips() {
    // A synthetic 3-part x 4-frame stream exercising every selector
    // arm: literals, small deltas, negative deltas, holds.
    let mut poses = Vec::new();
    for f in 0..4i16 {
        let mut row = Vec::new();
        for p in 0..3i16 {
            row.push(PartPose {
                tx: 100 * p + f,
                ty: -50 * p,
                tz: 2000 - 13 * f * p,
                rx: (300 * p as u16 + 7 * f as u16) & 0xFFF,
                ry: 4095 - (p as u16 * 2),
                rz: (f as u16 * 1000) & 0xFFF,
            });
        }
        poses.push(row);
    }
    let mut decoded = vec![3u8, 4u8];
    for row in &poses {
        for p in row {
            decoded.extend_from_slice(&pack_part(p));
        }
    }
    let enc = encode_channel_delta(&decoded).expect("encode");
    let dec = me_archive::decode_channel_delta(&enc).expect("decode");
    assert_eq!(dec, decoded, "codec round-trip");
}

fn marker(v: i16) -> Vec<PartPose> {
    vec![PartPose {
        tx: v,
        ..PartPose::default()
    }]
}

/// The property the whole composition exists for: the frame the tick's
/// wrap fires on carries the same pose as the frame it wraps back to,
/// so the loop cannot show a seam. Not observable on the disc (retail
/// ships no swapped stream), so a synthetic case holds it.
#[test]
fn the_composed_window_wraps_onto_its_own_first_frame() {
    let lead: Vec<Vec<PartPose>> = (0..7).map(marker).collect();
    let cycle: Vec<Vec<PartPose>> = (100..104).map(marker).collect();
    // `end < frames`: the wrap frame exists in the stream.
    let w = LoopWindow {
        count: 0xFF,
        start: 10,
        end: 29,
    };
    let out = compose_base_stream(&lead, &cycle, 30, w);
    assert_eq!(out.len(), 30);
    assert_eq!(out[0], lead[0], "lead-in starts on the clip's first frame");
    assert_eq!(out[9], lead[6], "lead-in ends on the clip's last frame");
    assert_eq!(out[29], out[10], "the wrap frame IS the wrap target");
    // Every window frame comes out of the cycle, none out of the lead.
    for f in &out[10..30] {
        assert!(cycle.contains(f), "window frame outside the cycle");
    }
}

/// A one-frame cycle - what a sibling declaring retail's single-frame
/// hold arm (`+0x85 == +0x86`) contributes - fills the window with a
/// motionless pose rather than replaying anything.
#[test]
fn a_one_frame_cycle_holds_the_window_still() {
    let lead: Vec<Vec<PartPose>> = (0..12).map(marker).collect();
    let cycle = vec![marker(99)];
    // `end == frames`: retail's other window shape, one past the last
    // frame, where the tick wraps before the natural end can commit.
    let w = LoopWindow {
        count: 0xFF,
        start: 5,
        end: 20,
    };
    let out = compose_base_stream(&lead, &cycle, 20, w);
    assert_eq!(out.len(), 20);
    assert!(
        out[5..].iter().all(|f| f == &cycle[0]),
        "the held window moved"
    );
}
