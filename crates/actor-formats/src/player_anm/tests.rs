use super::*;

/// Build a synthetic bundle that mirrors the real disc layout:
/// absolute offsets, marker at byte +4 of each record, and
/// `size = 8 + 8 * bone_count * frame_count + 8` (header + body +
/// 8-byte zero trailer).
fn synthetic_two_records() -> Vec<u8> {
    // Two records: (a=2 bones, b=3 frames, size = 8 + 8*2*3 + 8 = 64) twice.
    let count: u32 = 2;
    let table_end = 4 + 4 * count as usize;
    let rec_size = 64;
    let mut buf = Vec::new();
    buf.extend_from_slice(&count.to_le_bytes());
    let off0 = table_end as u32;
    let off1 = off0 + rec_size;
    buf.extend_from_slice(&off0.to_le_bytes());
    buf.extend_from_slice(&off1.to_le_bytes());
    for r in 0..2u16 {
        // header: a=2, b=3, marker=0x080C, flag=0x0002
        buf.extend_from_slice(&2u16.to_le_bytes());
        buf.extend_from_slice(&3u16.to_le_bytes());
        buf.extend_from_slice(&ANM_MARKER_1.to_le_bytes());
        buf.extend_from_slice(&0x0002u16.to_le_bytes());
        // 3 frames × 2 bones × 8 bytes = 48 bytes, tag with record index
        for f in 0..3u8 {
            for bidx in 0..2u8 {
                let v: u8 = (r as u8) * 100 + f * 10 + bidx;
                buf.extend_from_slice(&[v, 0, v + 1, 0, v + 2, 0, v + 3, 0]);
            }
        }
        // 8-byte trailer (zero in real records)
        buf.extend_from_slice(&[0u8; 8]);
    }
    buf
}

#[test]
fn parses_synthetic_two_records() {
    let buf = synthetic_two_records();
    let bundle = parse(&buf).expect("synthetic should parse");
    assert_eq!(bundle.record_count, 2);
    assert_eq!(bundle.record_offsets.len(), 2);
    assert_eq!(bundle.record_sizes, vec![64, 64]);
    assert_eq!(bundle.record_marker_1(0), Some(ANM_MARKER_1));
    assert_eq!(bundle.record_marker_1(1), Some(ANM_MARKER_1));
}

#[test]
fn record_decodes_header_and_sizes() {
    let buf = synthetic_two_records();
    let bundle = parse(&buf).unwrap();
    let r0 = bundle.record(0).unwrap();
    assert_eq!(r0.a, 2);
    assert_eq!(r0.b, 3);
    assert_eq!(r0.marker_1, ANM_MARKER_1);
    assert_eq!(r0.flag, 0x0002);
    assert_eq!(r0.bone_count, 2);
    assert_eq!(r0.frame_count, 3);
}

#[test]
fn frame_and_bone_indexing() {
    let buf = synthetic_two_records();
    let bundle = parse(&buf).unwrap();
    // Record 0, frame 0, bone 0: tagged with (0*100 + 0*10 + 0) = 0
    let bf = bundle.bone_frame_bytes(0, 0, 0);
    assert_eq!(bf.len(), 8);
    assert_eq!(bf[0], 0);
    // Record 0, frame 1, bone 0: tagged with (0*100 + 1*10 + 0) = 10
    let bf = bundle.bone_frame_bytes(0, 1, 0);
    assert_eq!(bf.len(), 8);
    assert_eq!(bf[0], 10);
    // Record 1, frame 2, bone 1: tagged with (1*100 + 2*10 + 1) = 121
    let bf = bundle.bone_frame_bytes(1, 2, 1);
    assert_eq!(bf.len(), 8);
    assert_eq!(bf[0], 121);
    // Out-of-range frame returns empty
    assert!(bundle.bone_frame_bytes(0, 99, 0).is_empty());
    // Out-of-range bone returns empty
    assert!(bundle.bone_frame_bytes(0, 0, 99).is_empty());
}

#[test]
fn bone_transform_decode_signed_12bit() {
    // Reproduce a known-good runtime decode: bytes from town01 record 17,
    // frame 0 bone 2: `E6 AB FF FE 0F C0 FD 43` should decode to
    // T=(-26, -85, -2) and R=(0xC00, 0xFD0, 0x430).
    let bf = [0xE6, 0xAB, 0xFF, 0xFE, 0x0F, 0xC0, 0xFD, 0x43];
    let t = BoneTransform::decode(&bf);
    assert_eq!(t.t_x, -26);
    assert_eq!(t.t_y, -85);
    assert_eq!(t.t_z, -2);
    assert_eq!(t.r_x, 0xC00);
    assert_eq!(t.r_y, 0xFD0);
    assert_eq!(t.r_z, 0x430);
}

#[test]
fn record_to_monster_animation_round_trips_every_bone_transform() {
    let buf = synthetic_two_records();
    let bundle = parse(&buf).unwrap();
    for rec_idx in 0..2 {
        let anim = bundle
            .record_to_monster_animation(rec_idx)
            .expect("record adapts");
        assert_eq!(anim.action_id, rec_idx as u8);
        assert_eq!(anim.part_count, 2);
        assert_eq!(anim.frame_count, 3);
        for f in 0..3 {
            for b in 0..2 {
                let t = bundle.bone_transform(rec_idx, f, b).unwrap();
                let p = anim.frames[f][b];
                // Same actor-local T + 12-bit R in both shapes.
                assert_eq!(p.tx as i32, t.t_x);
                assert_eq!(p.ty as i32, t.t_y);
                assert_eq!(p.tz as i32, t.t_z);
                assert_eq!(p.rx as i32, t.r_x & 0xFFF);
                assert_eq!(p.ry as i32, t.r_y & 0xFFF);
                assert_eq!(p.rz as i32, t.r_z & 0xFFF);
            }
        }
    }
    // Negative translations survive the i32 -> i16 narrowing: bone
    // entry `E6 AB FF FE 0F ...` decodes to T=(-26, -85, -2) (the
    // known-good runtime decode above).
    let mut buf2 = Vec::new();
    buf2.extend_from_slice(&1u32.to_le_bytes());
    buf2.extend_from_slice(&8u32.to_le_bytes());
    buf2.extend_from_slice(&1u16.to_le_bytes()); // a: 1 bone
    buf2.extend_from_slice(&1u16.to_le_bytes()); // b: 1 frame
    buf2.extend_from_slice(&ANM_MARKER_1.to_le_bytes());
    buf2.extend_from_slice(&0x0002u16.to_le_bytes());
    buf2.extend_from_slice(&[0xE6, 0xAB, 0xFF, 0xFE, 0x0F, 0xC0, 0xFD, 0x43]);
    buf2.extend_from_slice(&[0u8; 8]);
    let b2 = parse(&buf2).unwrap();
    let anim = b2.record_to_monster_animation(0).unwrap();
    let p = anim.frames[0][0];
    assert_eq!((p.tx, p.ty, p.tz), (-26, -85, -2));
    assert_eq!((p.rx, p.ry, p.rz), (0xC00, 0xFD0, 0x430));
    // Out-of-range record refuses.
    assert!(b2.record_to_monster_animation(1).is_none());
}

#[test]
fn rejects_wrong_marker() {
    // Build a buffer where marker at +4 is wrong
    let mut buf = Vec::new();
    buf.extend_from_slice(&1u32.to_le_bytes()); // count
    buf.extend_from_slice(&8u32.to_le_bytes()); // offset (absolute)
    buf.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xAA, 0xBB]); // wrong marker at +4
    buf.extend_from_slice(&[0u8; 64]);
    assert!(parse(&buf).is_err());
}

#[test]
fn rejects_offset_in_table() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&1u32.to_le_bytes()); // count
    buf.extend_from_slice(&4u32.to_le_bytes()); // offset points into table (< 8)
    buf.extend_from_slice(&[0u8; 64]);
    assert!(parse(&buf).is_err());
}

#[test]
fn record_with_bad_size_errors() {
    // Build a single record that claims a=2, b=3 (expected size 64) but
    // actually leaves only 32 bytes.
    let mut buf = Vec::new();
    buf.extend_from_slice(&1u32.to_le_bytes()); // count
    buf.extend_from_slice(&8u32.to_le_bytes()); // offset = 8 (absolute, table is 4..8)
    // header
    buf.extend_from_slice(&2u16.to_le_bytes());
    buf.extend_from_slice(&3u16.to_le_bytes());
    buf.extend_from_slice(&ANM_MARKER_1.to_le_bytes());
    buf.extend_from_slice(&0x0002u16.to_le_bytes());
    // 16 bytes of body (vs expected 8 + 8*2*3 + 8 = 64 trailing bytes)
    buf.extend_from_slice(&[0u8; 16]);
    let bundle = parse(&buf).unwrap();
    assert!(bundle.record(0).is_err());
}

#[test]
fn lerp_angle_12_endpoints_are_the_two_frames() {
    // frac 0 is the `to` frame verbatim; the mask is applied to both ends.
    assert_eq!(lerp_angle_12(0x400, 0x100, 0), 0x100);
    assert_eq!(lerp_angle_12(0x1400, 0x100, 0), 0x100);
    // 16/16 of the way is `from`, but the caller only ever passes 0..=15,
    // so the last representable step stops one sixteenth short.
    assert_eq!(lerp_angle_12(0x400, 0x100, 15), 0x100 + ((0x300 * 15) >> 4));
}

#[test]
fn lerp_angle_12_takes_the_short_arc_across_zero() {
    // 0xFF0 -> 0x010 is 32 units forward, not 4064 units backward.
    // Half-way must land on 0x000, and never in the 0x800 region.
    assert_eq!(lerp_angle_12(0x010, 0xFF0, 8), 0x000);
    assert_eq!(lerp_angle_12(0xFF0, 0x010, 8), 0x000);
    // Same arc, quarter of the way from each side.
    assert_eq!(lerp_angle_12(0x010, 0xFF0, 4), 0xFF8);
    assert_eq!(lerp_angle_12(0xFF0, 0x010, 4), 0x008);
}

#[test]
fn lerp_angle_12_unwrap_is_symmetric_and_stays_in_range() {
    for from in (0..0x1000).step_by(0x37) {
        for to in (0..0x1000).step_by(0x53) {
            for frac in 0..16 {
                let v = lerp_angle_12(from, to, frac);
                assert!(v < 0x1000, "{from:#x} {to:#x} {frac} -> {v:#x}");
            }
            // The unwrap brings the pair onto the short arc, so the step
            // taken per frac tick never exceeds half a turn / 16.
            let a = lerp_angle_12(from, to, 0) as i32;
            let b = lerp_angle_12(from, to, 1) as i32;
            let step = (a - b).rem_euclid(0x1000).min((b - a).rem_euclid(0x1000));
            assert!(step <= 0x800 / 16 + 1, "{from:#x} {to:#x} step {step:#x}");
        }
    }
}

#[test]
fn lerp_angle_12_half_turn_fires_both_guards() {
    // Exactly half a turn is the one input where BOTH guards fire, and
    // they cancel: `from - to == 0x800` bumps `to` to 0x1000, which makes
    // `to - from == 0x800` and bumps `from` to 0x1800, leaving the delta
    // back at +0x800. The walk therefore runs FORWARD, and half-way lands
    // on 0x400 rather than 0xC00. The second guard is re-evaluated on the
    // updated `to` (`subu v0,a1,a0` at 0x8001D0A4), which is what makes
    // this fall out; modelling the two as exclusive gets it backwards.
    assert_eq!(lerp_angle_12(0x800, 0x000, 8), 0x400);
}

/// One-record, one-bone bundle whose frames carry the given
/// `(translation, rotation-byte)` pairs; `a_high` is `a`'s high byte (bit 0
/// = the blend gate).
fn one_bone_bundle(a_high: u8, frames: &[([i16; 3], [u8; 3])]) -> PlayerAnmBundle {
    let mut buf = Vec::new();
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&8u32.to_le_bytes());
    buf.extend_from_slice(&[1, a_high]);
    buf.extend_from_slice(&(frames.len() as u16).to_le_bytes());
    buf.extend_from_slice(&ANM_MARKER_1.to_le_bytes());
    buf.extend_from_slice(&0x0002u16.to_le_bytes());
    for (t, r) in frames {
        let (x, y, z) = (t[0] as u16, t[1] as u16, t[2] as u16);
        buf.extend_from_slice(&[
            x as u8,
            y as u8,
            (((x >> 8) & 0xF) | (((y >> 8) & 0xF) << 4)) as u8,
            z as u8,
            ((z >> 8) & 0xF) as u8,
            r[0],
            r[1],
            r[2],
        ]);
    }
    buf.extend_from_slice(&[0u8; 8]);
    parse(&buf).expect("one-bone bundle")
}

const FRAMES3: [([i16; 3], [u8; 3]); 3] = [
    ([0, -16, 100], [0x10, 0x00, 0xF0]),
    ([32, -48, 100], [0x30, 0x20, 0x08]),
    ([-64, 0, -100], [0x50, 0x40, 0x10]),
];

#[test]
fn sampler_is_the_single_decode_at_integer_cursors() {
    for gate in [0u8, 1] {
        let b = one_bone_bundle(gate, &FRAMES3);
        for f in 0..3 {
            for hold in [false, true] {
                let s = b.sample_bone(0, (f * 16) as i16, hold, 0).unwrap();
                assert_eq!(s, b.bone_transform(0, f, 0).unwrap());
            }
        }
    }
}

#[test]
fn sampler_ignores_the_fraction_when_the_gate_is_clear() {
    let b = one_bone_bundle(0, &FRAMES3);
    assert!(!b.record(0).unwrap().blends());
    assert_eq!(
        b.sample_bone(0, 16 + 9, false, 0),
        b.bone_transform(0, 1, 0)
    );
}

#[test]
fn sampler_blends_toward_the_next_frame() {
    let b = one_bone_bundle(1, &FRAMES3);
    assert!(b.record(0).unwrap().blends());
    // Frame 0 -> 1 at 8/16: translations halfway, `sra` floors.
    let s = b.sample_bone(0, 8, false, 0).unwrap();
    assert_eq!((s.t_x, s.t_y, s.t_z), (16, -32, 100));
    // Z runs 0xF00 -> 0x080 the short way, across zero.
    assert_eq!((s.r_x, s.r_y, s.r_z), (0x200, 0x100, 0xFC0));
    // Frame 1 -> 2 at 3/16: -48 + ((48*3) >> 4) = -39; 100 + (-200*3 >> 4).
    let s = b.sample_bone(0, 16 + 3, false, 0).unwrap();
    assert_eq!(
        (s.t_x, s.t_y, s.t_z),
        (32 + ((-96 * 3) >> 4), -39, 100 + ((-200 * 3) >> 4))
    );
}

#[test]
fn last_frame_wraps_to_frame_zero_or_holds_on_the_clamp_bit() {
    let b = one_bone_bundle(1, &FRAMES3);
    let cursor = 2 * 16 + 8;
    // Loop: frame 2 blends toward frame 0.
    let s = b.sample_bone(0, cursor, false, 0).unwrap();
    assert_eq!(s.t_x, -64 + ((64 * 8) >> 4));
    // Clamp (`+0x62 & 8`): the next entry is the entry itself.
    let s = b.sample_bone(0, cursor, true, 0).unwrap();
    assert_eq!(s, b.bone_transform(0, 2, 0).unwrap());
}

#[test]
fn blend_at_zero_fraction_is_the_current_entry_even_on_the_flip_arm() {
    let cur = BoneTransform {
        t_x: 5,
        t_y: -7,
        t_z: 9,
        r_x: 0x000,
        r_y: 0x000,
        r_z: 0x000,
    };
    let next = BoneTransform {
        t_x: 50,
        t_y: 70,
        t_z: -90,
        r_x: 0x7F0,
        r_y: 0x7F0,
        r_z: 0x7F0,
    };
    assert_eq!(blend_bone_transform(cur, next, 0), cur);
}

#[test]
fn euler_flip_retry_runs_past_the_threshold() {
    let cur = BoneTransform {
        t_x: 0,
        t_y: 0,
        t_z: 0,
        r_x: 0x000,
        r_y: 0x000,
        r_z: 0x000,
    };
    // Summed short arcs 0x7F0 * 3 > 0xC00: retail retries against
    // (x + 0x800, -(y + 0x800), z + 0x800) = (0xFF0, 0x010, 0xFF0) - the
    // same orientation, 16 units from `cur` on every axis.
    let next = BoneTransform {
        t_x: 0,
        t_y: 0,
        t_z: 0,
        r_x: 0x7F0,
        r_y: 0x7F0,
        r_z: 0x7F0,
    };
    let s = blend_bone_transform(cur, next, 8);
    assert_eq!((s.r_x, s.r_y, s.r_z), (0xFF8, 0x008, 0xFF8));
    // Under the threshold: the plain per-axis blend.
    let next = BoneTransform {
        r_x: 0x400,
        r_y: 0x400,
        r_z: 0x400,
        ..next
    };
    let s = blend_bone_transform(cur, next, 8);
    assert_eq!((s.r_x, s.r_y, s.r_z), (0x200, 0x200, 0x200));
}
