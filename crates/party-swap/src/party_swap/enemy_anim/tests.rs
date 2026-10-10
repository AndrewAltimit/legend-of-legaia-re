use super::block_entries::{
    ARRAY_OFF, COUNT_OFF, ENTRY_STREAM_OFF, EntryRewrite, TABLE_WORD_BIAS, parse_entry_table,
    rebuild_block_entries, transform_head,
};
use super::fit_report::{affine_fit, mat_angle_deg};
use super::hero_retarget::resample_poses;
use super::*;

#[test]
fn resample_preserves_endpoints_and_wraps_rotation_short_path() {
    let mk = |tx: i16, ry: u16| {
        vec![PartPose {
            tx,
            ry,
            ..PartPose::default()
        }]
    };
    let frames = vec![mk(0, 4000), mk(100, 200)];
    let out = resample_poses(&frames, 5);
    assert_eq!(out.len(), 5);
    assert_eq!(out[0][0].tx, 0);
    assert_eq!(out[4][0].tx, 100);
    // Midpoint rotation goes the short way through 0, not through 2048.
    let mid = out[2][0].ry as i32;
    assert!(mid >= 4048 || mid <= 152, "mid ry {mid} took the long path");
}

#[test]
fn stretch_to_min_holds_duration() {
    let mk = |tx: i16| {
        vec![PartPose {
            tx,
            ..PartPose::default()
        }]
    };
    let rows: Vec<_> = (0..7i16).map(mk).collect();
    let (out, rate) = stretch_to_min(rows, 2, 23);
    // 7 frames -> ceil(23/7)=4x stretch, rate 2 -> 8.
    assert_eq!(out.len(), 28);
    assert_eq!(rate, 8);
    // frames * 8 / rate invariant: 7*8/2 == 28*8/8.
    assert_eq!(7 * 8 / 2, 28 * 8 / 8);
}

#[test]
fn payoff_split_widens_only_when_it_shrinks_the_stream() {
    // 58f / 3 stages, floor 23: uniform payoff (20f) stretches to 40;
    // a widened 23f payoff needs none - widen.
    assert_eq!(payoff_source_frames(58, 3, 23), 23);
    // 21f / 3 stages, floor 23: widening to 19f still stretches (x2 =
    // 38) and loses to the uniform 7f x4 = 28 - keep uniform.
    assert_eq!(payoff_source_frames(21, 3, 23), 7);
    // 20f / 2 stages, floor 11: widened 11f payoff needs no stretch
    // (11) vs uniform 10f doubled (20) - widen.
    assert_eq!(payoff_source_frames(20, 2, 11), 11);
    // Floor already met by the uniform share: unchanged.
    assert_eq!(payoff_source_frames(60, 3, 11), 20);
}

#[test]
fn halve_exact_is_duration_invariant_and_floor_guarded() {
    let mk = |n: usize| -> Vec<Vec<PartPose>> {
        (0..n as i16)
            .map(|f| {
                vec![PartPose {
                    tx: f,
                    ..PartPose::default()
                }]
            })
            .collect()
    };
    // 24f rate 4 -> 12f rate 2: duration 24*8/4 == 12*8/2.
    let mut rows = mk(24);
    let mut rate = 4u8;
    assert!(halve_exact(&mut rows, &mut rate, 11));
    assert_eq!((rows.len(), rate), (12, 2));
    // Odd rate: refused.
    let mut rows = mk(24);
    let mut rate = 1u8;
    assert!(!halve_exact(&mut rows, &mut rate, 1));
    // Floor guard: halving 24 under a 13 floor is refused.
    let mut rows = mk(24);
    let mut rate = 4u8;
    assert!(!halve_exact(&mut rows, &mut rate, 13));
}

#[test]
fn transform_head_rescales_frame_indexed_fields_only() {
    let mut head = vec![0u8; ENTRY_STREAM_OFF];
    head[0x00] = 0x23; // tag
    head[0x74] = 0x1C; // AGL
    head[0x76] = 0; // event-path gate
    head[0x77] = 0x07; // attach key
    head[0x78] = 2; // rate
    head[0x7A] = 0x01; // impact class
    head[0x84] = 0xFF; // loop seed
    head[0x85] = 9;
    head[0x86] = 10;
    head[0x87] = 0x01; // sound cue
    head[0x10] = 6; // event beat
    head[0x14] = 6; // fx gate
    head[0x15] = 0x84; // fx id
    head[0x0C] = 4; // approach speed word (low byte)
    let out = transform_head(&head, 11, 22, 4);
    assert_eq!(out[0x00], 0x23);
    assert_eq!(out[0x74], 0x1C);
    assert_eq!(out[0x77], 0x07);
    assert_eq!(out[0x78], 4, "rate byte rewritten");
    assert_eq!(out[0x7A], 0x01);
    assert_eq!(out[0x87], 0x01, "sound cue untouched");
    assert_eq!(out[0x0C], 4, "root motion untouched");
    assert_eq!(out[0x10], 12, "event beat rescaled");
    assert_eq!(out[0x14], 12, "fx gate rescaled");
    assert_eq!(out[0x15], 0x84, "fx id untouched");
    assert_eq!(out[0x84], 0xFF);
    assert_eq!(out[0x85], 18, "loop start rescaled");
    assert_eq!(out[0x86], 20, "loop window length rescaled");
}

#[test]
fn transform_head_keeps_single_frame_holds_single_frame() {
    let mut head = vec![0u8; ENTRY_STREAM_OFF];
    head[0x84] = 0xFF;
    head[0x85] = 15;
    head[0x86] = 15;
    let out = transform_head(&head, 16, 32, 1);
    assert_eq!(out[0x85], out[0x86], "zero-length hold stays zero-length");
    assert!(out[0x85] as usize <= 31);
}

#[test]
fn transform_head_keeps_event_count_and_ascending_order() {
    let mut head = vec![0u8; ENTRY_STREAM_OFF];
    head[0x10..0x14].copy_from_slice(&[3, 7, 11, 15]);
    let out = transform_head(&head, 19, 8, 1);
    let ev: Vec<u8> = out[0x10..0x14].to_vec();
    assert!(ev.iter().all(|&v| v != 0), "count preserved: {ev:?}");
    assert!(ev.windows(2).all(|w| w[0] < w[1]), "ascending: {ev:?}");
    assert!(
        ev.iter().all(|&v| (v as usize) < 8),
        "inside the clip: {ev:?}"
    );
}

#[test]
fn affine_fit_recovers_a_known_rotation_and_scales() {
    // Points on a box, rotated 30 deg about z and scaled [2, 1, 0.5].
    let rot = 30f32.to_radians();
    let (s, c) = rot.sin_cos();
    let src: Vec<[f32; 3]> = (0..64)
        .map(|i| {
            [
                ((i % 4) as f32) * 10.0,
                (((i / 4) % 4) as f32) * 7.0,
                ((i / 16) as f32) * 5.0,
            ]
        })
        .collect();
    let dst: Vec<[f32; 3]> = src
        .iter()
        .map(|p| {
            let scaled = [p[0] * 2.0, p[1] * 1.0, p[2] * 0.5];
            [
                c * scaled[0] - s * scaled[1] + 3.0,
                s * scaled[0] + c * scaled[1] - 8.0,
                scaled[2] + 1.0,
            ]
        })
        .collect();
    let (r, residual, scales) = affine_fit(&src, &dst).expect("fit");
    assert!(residual < 1e-3, "residual {residual}");
    assert!((scales[0] - 2.0).abs() < 0.01, "scales {scales:?}");
    assert!((scales[2] - 0.5).abs() < 0.01, "scales {scales:?}");
    // Rotation angle 30 deg.
    let ang = mat_angle_deg(&r);
    assert!((ang - 30.0).abs() < 0.5, "angle {ang}");
}

#[test]
fn rebuild_is_idempotent_on_a_synthetic_block() {
    // A tiny block in the retail layout: head, 2 entries, tail
    // descriptor, pool.
    let count = 2usize;
    let entry_area = 0x70usize;
    let mk_entry = |id: u8, idx4: u32, frames: u8| {
        let mut e = vec![0u8; ENTRY_STREAM_OFF + 2 + frames as usize * 9];
        e[0] = id;
        e[4..8].copy_from_slice(&idx4.to_le_bytes());
        e[0x78] = 2;
        e[ENTRY_STREAM_OFF] = 1;
        e[ENTRY_STREAM_OFF + 1] = frames;
        e
    };
    let e0 = mk_entry(0x00, 0, 4);
    let e1 = mk_entry(0x23, 1, 6);
    let o0 = entry_area;
    let o1 = (o0 + e0.len()).div_ceil(4) * 4;
    let tail = (o1 + e1.len()).div_ceil(4) * 4;
    let pool = tail + 8;
    let total = pool + 16;
    let mut b = vec![0u8; total];
    b[0..4].copy_from_slice(&0x40u32.to_le_bytes());
    b[4..8].copy_from_slice(&0x48u32.to_le_bytes());
    b[8..12].copy_from_slice(&(pool as u32).to_le_bytes());
    b[COUNT_OFF] = count as u8;
    for (k, o) in [o0, o1].iter().enumerate() {
        b[ARRAY_OFF + k * 4..ARRAY_OFF + k * 4 + 4].copy_from_slice(&(*o as u32).to_le_bytes());
    }
    let word = (1 + count + TABLE_WORD_BIAS) * 4;
    b[word..word + 4].copy_from_slice(&(tail as u32).to_le_bytes());
    b[o0..o0 + e0.len()].copy_from_slice(&e0);
    b[o1..o1 + e1.len()].copy_from_slice(&e1);
    b[tail..tail + 8].copy_from_slice(&[0xAA; 8]);
    b[pool..].fill(0x55);

    let rows: Vec<Vec<PartPose>> = (0..9i16)
        .map(|f| {
            vec![PartPose {
                tx: f * 3,
                ..PartPose::default()
            }]
        })
        .collect();
    let mut rw = BTreeMap::new();
    rw.insert(
        1usize,
        EntryRewrite {
            rows: rows.clone(),
            rate: 3,
        },
    );
    let once = rebuild_block_entries(&b, &b, &rw).expect("rebuild");
    // The rewritten entry decodes at the new shape through the raw parser.
    let (entries, ..) = parse_entry_table(&once).expect("parse rebuilt");
    assert_eq!(entries[1].frames, 9);
    assert_eq!(once[entries[1].off + 0x78], 3);
    // The tail descriptor is still reachable through the shifted word.
    let desc = u32::from_le_bytes(once[word..word + 4].try_into().unwrap()) as usize;
    assert_eq!(&once[desc..desc + 8], &[0xAA; 8]);
    // Pool intact.
    let p = u32::from_le_bytes(once[8..12].try_into().unwrap()) as usize;
    assert!(once[p..].iter().all(|&x| x == 0x55));
    // Idempotence: a second pass over the mirrored block reproduces it.
    let twice = rebuild_block_entries(&once, &b, &rw).expect("rebuild twice");
    assert_eq!(once, twice, "second pass must be byte-identical");
}
