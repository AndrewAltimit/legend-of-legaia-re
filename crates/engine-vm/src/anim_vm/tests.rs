use super::*;

/// Build a synthetic keyframe-style record: 8-byte header (a=0x06),
/// `bone_count` 8-byte output slots, `bone_count` 24-byte keyframe
/// entries.
fn synth_keyframe_record(bone_count: usize) -> Vec<u8> {
    let mut buf = vec![0u8; legaia_anm::RECORD_HEADER_SIZE + 8 * bone_count + 24 * bone_count];
    // header.a = 0x06 (Keyframe)
    buf[0..2].copy_from_slice(&0x0006u16.to_le_bytes());
    // header.b = 0x14 (frame count)
    buf[2..4].copy_from_slice(&0x0014u16.to_le_bytes());
    // marker_1 = 0x080C (canonical)
    buf[4..6].copy_from_slice(&0x080Cu16.to_le_bytes());
    // flag = 0x0002
    buf[6..8].copy_from_slice(&0x0002u16.to_le_bytes());
    // Set first bone keyframe so interpolation has something non-trivial
    let kf_off = legaia_anm::RECORD_HEADER_SIZE + 8 * bone_count;
    buf[kf_off..kf_off + 2].copy_from_slice(&10i16.to_le_bytes());
    buf[kf_off + 6..kf_off + 8].copy_from_slice(&100i16.to_le_bytes());
    buf
}

/// Build a synthetic opaque record with header `a` set to `kind_byte`.
fn synth_opaque_record(kind_byte: u16, body_len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; legaia_anm::RECORD_HEADER_SIZE + body_len];
    buf[0..2].copy_from_slice(&kind_byte.to_le_bytes());
    buf[4..6].copy_from_slice(&0x080Cu16.to_le_bytes());
    buf[6..8].copy_from_slice(&0x0002u16.to_le_bytes());
    buf
}

#[test]
fn dispatch_byte_round_trips_full_observed_range() {
    for b in 1u16..=7 {
        let d = DispatchByte::from_byte(b).expect("0x01..=0x07 must round-trip");
        assert_eq!(d.as_byte(), b);
    }
}

#[test]
fn dispatch_byte_rejects_out_of_range() {
    assert!(DispatchByte::from_byte(0x00).is_none());
    assert!(DispatchByte::from_byte(0x08).is_none());
    assert!(DispatchByte::from_byte(0xFF).is_none());
}

#[test]
fn dispatch_byte_handled_natively_only_for_keyframe() {
    assert!(DispatchByte::Keyframe.handled_natively());
    assert!(!DispatchByte::Snap.handled_natively());
    assert!(!DispatchByte::KeyframeAlt.handled_natively());
    assert!(!DispatchByte::Path.handled_natively());
    assert!(!DispatchByte::Damp.handled_natively());
    assert!(!DispatchByte::PathAlt.handled_natively());
    assert!(!DispatchByte::Spline.handled_natively());
}

#[test]
fn actor_field_offsets_match_documented_layout() {
    // Sanity: these are read by both this crate and the docs;
    // bumping them here without bumping the docs would silently
    // diverge.
    assert_eq!(ACTOR_RECORD_PTR_OFFSET, 0x4C);
    assert_eq!(ACTOR_DISPATCH_BYTE_OFFSET, 0x5A);
    assert_eq!(ACTOR_FRAME_COUNTER_OFFSET, 0x68);
}

#[test]
fn record_kind_from_header_a() {
    let r6 = synth_keyframe_record(2);
    assert_eq!(RecordKind::from_record(&r6), Some(RecordKind::Keyframe));
    let r2 = synth_opaque_record(0x02, 16);
    assert_eq!(RecordKind::from_record(&r2), Some(RecordKind::Kind2));
    let r3 = synth_opaque_record(0x03, 16);
    assert_eq!(RecordKind::from_record(&r3), Some(RecordKind::Kind3));
    let ra = synth_opaque_record(0x0A, 16);
    assert_eq!(RecordKind::from_record(&ra), Some(RecordKind::KindA));
    let other = synth_opaque_record(0x42, 16);
    assert_eq!(
        RecordKind::from_record(&other),
        Some(RecordKind::Other(0x42))
    );
    // Too-small buffer → None.
    assert!(RecordKind::from_record(&[0u8; 4]).is_none());
}

#[test]
fn opaque_record_kind_byte_round_trips_known_kinds() {
    for b in [0x02u8, 0x04, 0x05, 0x07, 0x08] {
        let k = OpaqueRecordKind::from_byte(b);
        assert_eq!(k.as_byte(), b);
        assert!(k.has_side_effect());
    }
    let other = OpaqueRecordKind::from_byte(0x18);
    assert_eq!(other.as_byte(), 0x18);
    assert!(!other.has_side_effect());
}

#[test]
fn opaque_anim_record_reads_documented_offsets() {
    let mut buf = vec![0u8; 0x300];
    buf[0x00] = 0x18; // kind = Other(0x18) (somersault-class)
    buf[0x0E..0x10].copy_from_slice(&((-128i16).to_le_bytes()));
    buf[0x56..0x58].copy_from_slice(&7u16.to_le_bytes());
    buf[0x84] = 0x10;
    buf[0x85] = 12;
    buf[0x86] = 24;
    buf[0x87] = 0x42;
    buf[0x88..0x8C].copy_from_slice(&0x80AB_CDEFu32.to_le_bytes());
    buf[0x176..0x178].copy_from_slice(&255u16.to_le_bytes());

    let r = OpaqueAnimRecord::new(&buf);
    assert_eq!(r.kind(), Some(OpaqueRecordKind::Other(0x18)));
    assert_eq!(r.movement_scale(), Some(-128));
    assert_eq!(r.substate_counter(), Some(7));
    assert_eq!(r.depth_84(), Some(0x10));
    assert_eq!(r.count_85(), Some(12));
    assert_eq!(r.count_86(), Some(24));
    assert_eq!(r.effect_id(), Some(0x42));
    assert_eq!(r.nested_data_ptr_raw(), Some(0x80AB_CDEF));
    assert_eq!(r.anim_frame_176(), Some(255));
}

#[test]
fn actor_anim_state_lod_step_factor_folds_division() {
    // raw=0 → denom 1, factor 8/1 = 8 (clamped, 1..=8)
    // raw=2 → factor 4
    // raw=4 → factor 2
    // raw=8 → factor 1
    let cases = &[(0u8, 8u8), (2, 4), (4, 2), (8, 1)];
    for &(raw, expected) in cases {
        let mut buf = vec![0u8; 0x250];
        buf[ACTOR_LOD_STEP_OFFSET] = raw;
        let s = ActorAnimState::new(&buf);
        assert_eq!(
            s.lod_step_factor(),
            Some(expected),
            "raw 0x{raw:02x} should give factor {expected}",
        );
    }
}

#[test]
fn actor_anim_state_frame_counter_extracts_index_and_subframe() {
    // bits[4..15] = frame index, bits[0..3] = sub-frame factor.
    // 0x1234 → index = 0x123, sub = 0x4.
    let mut buf = vec![0u8; 0x250];
    buf[0x68..0x6A].copy_from_slice(&0x1234u16.to_le_bytes());
    buf[ACTOR_PREV_ACTION_OFFSET] = 0x55;
    buf[ACTOR_FRAME_CAP_OFFSET..ACTOR_FRAME_CAP_OFFSET + 2]
        .copy_from_slice(&0x0123u16.to_le_bytes());
    let s = ActorAnimState::new(&buf);
    assert_eq!(s.frame_counter(), Some(0x1234));
    assert_eq!(s.frame_index(), Some(0x123));
    assert_eq!(s.sub_frame_factor(), Some(0x4));
    assert_eq!(s.prev_action(), Some(0x55));
    assert_eq!(s.frame_cap(), Some(0x0123));
}

#[test]
fn bone_frame_round_trips_for_in_range_components() {
    // Components in [-2048, 2047] round-trip exactly through
    // the 9-byte packed form.
    let cases = &[
        BoneFrame {
            vec_a: [0, 0, 0],
            vec_b: [0, 0, 0],
        },
        BoneFrame {
            vec_a: [1, -1, 100],
            vec_b: [-100, 2047, -2048],
        },
        BoneFrame {
            vec_a: [0x07FF, -0x0800, 0x055],
            vec_b: [0x0123, -0x0456, 0x0789],
        },
    ];
    for original in cases {
        let bytes = original.to_9_bytes();
        let decoded = BoneFrame::from_9_bytes(&bytes);
        assert_eq!(*original, decoded, "round-trip failed for {original:?}");
    }
}

#[test]
fn bone_frame_sign_extends_high_nibble_bit() {
    // byte[2] = 0x80: low nibble 0x0 → vec_a[0]=0, high nibble 0x8 →
    // vec_a[1] = 0 | (0x8 << 8) = 0x800 → bit 11 set → sign-extends
    // to 0xF800 = -2048.
    let bytes = [0x00u8, 0x00, 0x80, 0, 0, 0, 0, 0, 0];
    let bf = BoneFrame::from_9_bytes(&bytes);
    assert_eq!(bf.vec_a, [0, -2048, 0]);
    // byte[2] = 0x07: high nibble 0, low nibble 7 → vec_a[0] =
    // 0 | (0x7 << 8) = 0x700, no sign extension (bit 11 clear).
    let bytes2 = [0u8, 0, 0x07, 0, 0, 0, 0, 0, 0];
    let bf2 = BoneFrame::from_9_bytes(&bytes2);
    assert_eq!(bf2.vec_a, [0x700, 0, 0]);
}

#[test]
fn bone_frame_pack_uses_correct_byte_pairings() {
    // Packing rules from FUN_8004998C lines 1049..1054:
    //
    //   k=0 ← byte[0] | (byte[2] & 0x0F) << 8
    //   k=1 ← byte[1] | (byte[2] & 0xF0) << 4
    //   k=2 ← byte[3] | (byte[5] & 0x0F) << 8
    //   k=3 ← byte[4] | (byte[5] & 0xF0) << 4
    //   k=4 ← byte[6] | (byte[8] & 0x0F) << 8
    //   k=5 ← byte[7] | (byte[8] & 0xF0) << 4
    //
    // Pick low nibbles only so the assertions stay sign-clean.
    let bytes = [0x11u8, 0x22, 0x21, 0x33, 0x44, 0x43, 0x55, 0x66, 0x65];
    let bf = BoneFrame::from_9_bytes(&bytes);
    assert_eq!(bf.vec_a[0], 0x0111); // byte[0]=0x11, low nibble of byte[2] (0x1)
    assert_eq!(bf.vec_a[1], 0x0222); // byte[1]=0x22, high nibble of byte[2] (0x2)
    assert_eq!(bf.vec_a[2], 0x0333); // byte[3]=0x33, low nibble of byte[5] (0x3)
    assert_eq!(bf.vec_b[0], 0x0444); // byte[4]=0x44, high nibble of byte[5] (0x4)
    assert_eq!(bf.vec_b[1], 0x0555); // byte[6]=0x55, low nibble of byte[8] (0x5)
    assert_eq!(bf.vec_b[2], 0x0666); // byte[7]=0x66, high nibble of byte[8] (0x6)
}

#[test]
fn nested_frame_data_round_trips_two_frames_three_bones() {
    // 2 frames × 3 bones × 9 bytes = 54 body bytes + 2 header = 56 total.
    let bones = 3usize;
    let frames = 2usize;
    let mut buf = vec![0u8; NESTED_HEADER_SIZE + frames * bones * NESTED_BONE_STRIDE];
    buf[0] = bones as u8;
    buf[1] = frames as u8;
    // Stamp distinct bone keyframes per (frame, bone) so we can
    // verify the indexing math.
    let mut written = Vec::new();
    for f in 0..frames {
        for b in 0..bones {
            let bone = BoneFrame {
                vec_a: [(f as i16) * 100 + b as i16, 0, 0],
                vec_b: [0, 0, (b as i16) * -10],
            };
            let off = NESTED_HEADER_SIZE + f * bones * NESTED_BONE_STRIDE + b * NESTED_BONE_STRIDE;
            buf[off..off + NESTED_BONE_STRIDE].copy_from_slice(&bone.to_9_bytes());
            written.push(bone);
        }
    }

    let nfd = NestedFrameData::from_bytes(&buf).expect("valid");
    assert_eq!(nfd.bones_per_frame(), 3);
    assert_eq!(nfd.frame_count(), 2);
    assert_eq!(nfd.expected_size(), buf.len());
    assert_eq!(nfd.frame_stride(), bones * NESTED_BONE_STRIDE);

    for f in 0..frames {
        for b in 0..bones {
            let bone = nfd.bone(f, b).unwrap();
            let expected = written[f * bones + b];
            assert_eq!(bone, expected, "(frame={f}, bone={b})");
        }
    }
}

#[test]
fn nested_frame_data_frame_view_iterates_in_storage_order() {
    let bones = 2u8;
    let frames = 1u8;
    let mut buf = vec![
        0u8;
        NESTED_HEADER_SIZE
            + usize::from(bones) * usize::from(frames) * NESTED_BONE_STRIDE
    ];
    buf[0] = bones;
    buf[1] = frames;
    let bone_a = BoneFrame {
        vec_a: [10, 20, 30],
        vec_b: [-1, -2, -3],
    };
    let bone_b = BoneFrame {
        vec_a: [100, 200, 300],
        vec_b: [-100, -200, -300],
    };
    buf[2..11].copy_from_slice(&bone_a.to_9_bytes());
    buf[11..20].copy_from_slice(&bone_b.to_9_bytes());

    let nfd = NestedFrameData::from_bytes(&buf).unwrap();
    let frame = nfd.frame(0).unwrap();
    let bones_seen: Vec<_> = frame.iter_bones().collect();
    assert_eq!(bones_seen.len(), 2);
    assert_eq!(bones_seen[0], bone_a);
    assert_eq!(bones_seen[1], bone_b);
}

#[test]
fn nested_frame_data_rejects_zero_counts_and_truncated_body() {
    // Zero bone count.
    let buf = vec![0u8, 5, 0, 0, 0];
    assert_eq!(
        NestedFrameData::from_bytes(&buf).err(),
        Some(NestedFrameDataError::ZeroBonesPerFrame)
    );
    // Zero frame count.
    let buf = vec![3u8, 0, 0, 0, 0];
    assert_eq!(
        NestedFrameData::from_bytes(&buf).err(),
        Some(NestedFrameDataError::ZeroFrameCount)
    );
    // Truncated body: header says 2 bones × 1 frame = 18 bytes
    // body, but only 10 are present.
    let mut buf = vec![0u8; NESTED_HEADER_SIZE + 10];
    buf[0] = 2;
    buf[1] = 1;
    match NestedFrameData::from_bytes(&buf) {
        Err(NestedFrameDataError::BodyTruncated { needed, got }) => {
            assert_eq!(needed, NESTED_HEADER_SIZE + 2 * NESTED_BONE_STRIDE);
            assert_eq!(got, NESTED_HEADER_SIZE + 10);
        }
        other => panic!("expected BodyTruncated, got {other:?}"),
    }
    // Header too small.
    let buf = vec![0u8];
    assert_eq!(
        NestedFrameData::from_bytes(&buf).err(),
        Some(NestedFrameDataError::HeaderTooSmall)
    );
}

#[test]
fn nested_frame_data_interpolate_matches_runtime_lerp_formula() {
    let bones = 1u8;
    let frames = 2u8;
    let mut buf = vec![
        0u8;
        NESTED_HEADER_SIZE
            + usize::from(bones) * usize::from(frames) * NESTED_BONE_STRIDE
    ];
    buf[0] = bones;
    buf[1] = frames;
    let f0 = BoneFrame {
        vec_a: [0, 0, 0],
        vec_b: [0, 0, 0],
    };
    let f1 = BoneFrame {
        vec_a: [16, -16, 32],
        vec_b: [0, 0, 0],
    };
    buf[2..11].copy_from_slice(&f0.to_9_bytes());
    buf[11..20].copy_from_slice(&f1.to_9_bytes());

    let nfd = NestedFrameData::from_bytes(&buf).unwrap();
    // At frac=0, result equals frame 0.
    let r0 = nfd.interpolate(0, 1, 0).unwrap();
    assert_eq!(r0[0], f0);
    // At frac=8 (half), each component advances halfway:
    // 0 + (16 - 0) * 8 >> 4 = 8.
    let r8 = nfd.interpolate(0, 1, 8).unwrap();
    assert_eq!(r8[0].vec_a, [8, -8, 16]);
    // Frac is clamped to 15.
    let r15 = nfd.interpolate(0, 1, 15).unwrap();
    // 0 + (16 - 0) * 15 >> 4 = 240 / 16 = 15
    assert_eq!(r15[0].vec_a, [15, -15, 30]);
    // Out-of-range frac is clamped to 15 internally.
    let r99 = nfd.interpolate(0, 1, 99).unwrap();
    assert_eq!(r99[0].vec_a, [15, -15, 30]);
}

#[test]
fn nested_frame_data_lenient_accepts_short_buffer() {
    // Lenient view accepts any 2+ byte slice; out-of-range reads
    // return None.
    let buf = vec![3u8, 5, 0, 0, 0];
    let nfd = NestedFrameData::from_bytes_lenient(&buf).unwrap();
    assert_eq!(nfd.bones_per_frame(), 3);
    assert_eq!(nfd.frame_count(), 5);
    assert!(nfd.bone(0, 0).is_none()); // body too small
    assert!(nfd.frame(0).is_none());
}

#[test]
fn opaque_anim_record_short_buffer_returns_none_for_out_of_range() {
    let buf = vec![0x04u8, 0, 0, 0]; // only 4 bytes
    let r = OpaqueAnimRecord::new(&buf);
    assert_eq!(r.kind(), Some(OpaqueRecordKind::Kind4));
    assert_eq!(r.depth_84(), None);
    assert_eq!(r.nested_data_ptr_raw(), None);
}

#[test]
fn record_kind_handled_natively_only_for_keyframe() {
    assert!(RecordKind::Keyframe.handled_natively());
    assert!(!RecordKind::Kind2.handled_natively());
    assert!(!RecordKind::Kind3.handled_natively());
    assert!(!RecordKind::KindA.handled_natively());
    assert!(!RecordKind::Other(0x42).handled_natively());
}

#[test]
fn play_keyframe_creates_keyframe_slot() {
    let mut rt = AnimRuntime::with_slots(4);
    let kind = rt.play(2, synth_keyframe_record(3), 3).unwrap();
    assert_eq!(kind, RecordKind::Keyframe);
    assert!(matches!(rt.slot(2), Some(AnimSlot::Keyframe { .. })));
    assert!(rt.slot(0).map(|s| s.is_idle()).unwrap_or(false));
}

#[test]
fn play_opaque_creates_opaque_slot_with_init_counter_100() {
    let mut rt = AnimRuntime::with_slots(4);
    let kind = rt.play(1, synth_opaque_record(0x02, 32), 0).unwrap();
    assert_eq!(kind, RecordKind::Kind2);
    match rt.slot(1).unwrap() {
        AnimSlot::Opaque {
            kind,
            frame_counter,
            ..
        } => {
            assert_eq!(*kind, RecordKind::Kind2);
            assert_eq!(*frame_counter, 100);
        }
        other => panic!("expected Opaque slot, got {other:?}"),
    }
}

#[test]
fn play_replacing_busy_slot_emits_replaced_event() {
    let mut rt = AnimRuntime::with_slots(4);
    rt.play(0, synth_keyframe_record(2), 2).unwrap();
    rt.play(0, synth_keyframe_record(2), 2).unwrap();
    let evs = rt.take_events();
    assert!(evs.contains(&AnimEvent::Replaced { actor: 0 }));
}

#[test]
fn play_actor_out_of_range_errors() {
    let mut rt = AnimRuntime::with_slots(4);
    let err = rt
        .play(7, synth_keyframe_record(1), 1)
        .expect_err("should be an error");
    assert_eq!(err, AnimError::ActorOutOfRange { actor: 7 });
}

#[test]
fn play_too_small_buffer_errors() {
    let mut rt = AnimRuntime::with_slots(4);
    let err = rt.play(0, vec![0u8; 4], 1).expect_err("should error");
    assert_eq!(err, AnimError::HeaderTooSmall);
}

#[test]
fn tick_keyframe_emits_pose_updated_and_writes_pose() {
    let mut rt = AnimRuntime::with_slots(4);
    rt.play(0, synth_keyframe_record(1), 1).unwrap();
    let mut host = NullHost;
    rt.tick(&mut host);
    let evs = rt.take_events();
    assert!(matches!(evs[0], AnimEvent::PoseUpdated { actor: 0, .. }));
    assert!(rt.pose(0).is_some());
}

#[test]
fn tick_opaque_calls_host_and_emits_opaque_tick() {
    struct CountingHost {
        calls: Vec<(u8, RecordKind, u32)>,
    }
    impl Host for CountingHost {
        fn on_opaque_record(
            &mut self,
            actor: u8,
            kind: RecordKind,
            _record: &[u8],
            frame_counter: u32,
        ) -> bool {
            self.calls.push((actor, kind, frame_counter));
            true
        }
    }
    let mut rt = AnimRuntime::with_slots(4);
    rt.play(2, synth_opaque_record(0x02, 16), 0).unwrap();
    let mut host = CountingHost { calls: vec![] };
    rt.tick(&mut host);
    assert_eq!(host.calls.len(), 1);
    let (actor, kind, frame) = host.calls[0];
    assert_eq!(actor, 2);
    assert_eq!(kind, RecordKind::Kind2);
    // Initial counter 100 → after one tick it's 101.
    assert_eq!(frame, 101);
    let evs = rt.take_events();
    assert!(matches!(
        evs[0],
        AnimEvent::OpaqueTick {
            actor: 2,
            kind: RecordKind::Kind2,
            frame: 101
        }
    ));
}

#[test]
fn tick_opaque_host_returning_false_clears_slot_and_emits_finished() {
    struct EarlyExitHost;
    impl Host for EarlyExitHost {
        fn on_opaque_record(
            &mut self,
            _actor: u8,
            _kind: RecordKind,
            _record: &[u8],
            _frame: u32,
        ) -> bool {
            false
        }
    }
    let mut rt = AnimRuntime::with_slots(4);
    rt.play(0, synth_opaque_record(0x03, 16), 0).unwrap();
    let mut host = EarlyExitHost;
    rt.tick(&mut host);
    let evs = rt.take_events();
    assert!(evs.iter().any(|e| matches!(
        e,
        AnimEvent::Finished {
            actor: 0,
            kind: RecordKind::Kind3
        }
    )));
    assert!(rt.slot(0).unwrap().is_idle());
}

#[test]
fn tick_keyframe_finished_clears_slot_when_non_looping() {
    let mut rt = AnimRuntime::with_slots(4);
    // Build a keyframe record that finishes quickly.
    rt.play(1, synth_keyframe_record(1), 1).unwrap();
    // Force the embedded player into non-looping mode + max delta.
    if let Some(AnimSlot::Keyframe { player, .. }) = rt.slots.get_mut(1) {
        player.looping = false;
        player.frame_delta = 0xFF;
    } else {
        panic!("expected keyframe slot");
    }
    let mut host = NullHost;
    // 0xFF + 0xFF wraps past 0xFF on the second tick → finished=true.
    rt.tick(&mut host);
    rt.tick(&mut host);
    let evs = rt.take_events();
    assert!(evs.iter().any(|e| matches!(
        e,
        AnimEvent::Finished {
            actor: 1,
            kind: RecordKind::Keyframe
        }
    )));
    assert!(rt.slot(1).unwrap().is_idle());
}

#[test]
fn idle_slots_are_skipped_during_tick() {
    let mut rt = AnimRuntime::with_slots(4);
    // Don't register anything; tick should produce no events.
    let mut host = NullHost;
    rt.tick(&mut host);
    assert!(rt.take_events().is_empty());
    assert!(!rt.any_active());
}

#[test]
fn stop_clears_slot_and_pose() {
    let mut rt = AnimRuntime::with_slots(4);
    rt.play(0, synth_keyframe_record(1), 1).unwrap();
    let mut host = NullHost;
    rt.tick(&mut host);
    assert!(rt.pose(0).is_some());
    rt.stop(0);
    assert!(rt.slot(0).unwrap().is_idle());
    assert!(rt.pose(0).is_none());
}

#[test]
fn frame_counter_advances_each_tick() {
    let mut rt = AnimRuntime::with_slots(2);
    let mut host = NullHost;
    rt.tick(&mut host);
    rt.tick(&mut host);
    rt.tick(&mut host);
    assert_eq!(rt.frame, 3);
}

#[test]
fn header_a_round_trips_through_slot() {
    let mut rt = AnimRuntime::with_slots(2);
    rt.play(0, synth_keyframe_record(1), 1).unwrap();
    rt.play(1, synth_opaque_record(0x0A, 16), 0).unwrap();
    assert_eq!(rt.slot(0).unwrap().header_a(), Some(0x06));
    assert_eq!(rt.slot(1).unwrap().header_a(), Some(0x0A));
}

/// Host that records which per-kind dispatcher fired for each
/// actor. Used by the per-kind tests below.
#[derive(Default)]
struct PerKindHost {
    kind2: Vec<u8>,
    kind4: Vec<u8>,
    kind5: Vec<u8>,
    kind7: Vec<u8>,
    kind8: Vec<u8>,
    other: Vec<(u8, u8)>,
}

impl Host for PerKindHost {
    fn on_kind2_handshake(&mut self, actor: u8, _r: &[u8], _f: u32) -> bool {
        self.kind2.push(actor);
        true
    }
    fn on_kind4_action_engaged(&mut self, actor: u8, _r: &[u8], _f: u32) -> bool {
        self.kind4.push(actor);
        true
    }
    fn on_kind5_or_bit2(&mut self, actor: u8, _r: &[u8], _f: u32) -> bool {
        self.kind5.push(actor);
        true
    }
    fn on_kind7_set_da_8(&mut self, actor: u8, _r: &[u8], _f: u32) -> bool {
        self.kind7.push(actor);
        true
    }
    fn on_kind8_or_bit3(&mut self, actor: u8, _r: &[u8], _f: u32) -> bool {
        self.kind8.push(actor);
        true
    }
    fn on_other_kind(&mut self, actor: u8, byte: u8, _r: &[u8], _f: u32) -> bool {
        self.other.push((actor, byte));
        true
    }
}

#[test]
fn per_kind_dispatch_routes_kind2_through_on_kind2_handshake() {
    let mut rt = AnimRuntime::with_slots(4);
    rt.play(2, synth_opaque_record(0x02, 16), 0).unwrap();
    let mut host = PerKindHost::default();
    rt.tick(&mut host);
    assert_eq!(host.kind2, vec![2]);
    assert!(host.kind4.is_empty());
    assert!(host.kind5.is_empty());
    assert!(host.other.is_empty());
}

#[test]
fn per_kind_dispatch_routes_kind4_through_on_kind4_action_engaged() {
    let mut rt = AnimRuntime::with_slots(4);
    rt.play(1, synth_opaque_record(0x04, 16), 0).unwrap();
    let mut host = PerKindHost::default();
    rt.tick(&mut host);
    assert_eq!(host.kind4, vec![1]);
}

#[test]
fn per_kind_dispatch_routes_kind5_through_on_kind5_or_bit2() {
    let mut rt = AnimRuntime::with_slots(4);
    rt.play(0, synth_opaque_record(0x05, 16), 0).unwrap();
    let mut host = PerKindHost::default();
    rt.tick(&mut host);
    assert_eq!(host.kind5, vec![0]);
}

#[test]
fn per_kind_dispatch_routes_kind7_through_on_kind7_set_da_8() {
    let mut rt = AnimRuntime::with_slots(4);
    rt.play(3, synth_opaque_record(0x07, 16), 0).unwrap();
    let mut host = PerKindHost::default();
    rt.tick(&mut host);
    assert_eq!(host.kind7, vec![3]);
}

#[test]
fn per_kind_dispatch_routes_kind8_through_on_kind8_or_bit3() {
    let mut rt = AnimRuntime::with_slots(4);
    rt.play(2, synth_opaque_record(0x08, 16), 0).unwrap();
    let mut host = PerKindHost::default();
    rt.tick(&mut host);
    assert_eq!(host.kind8, vec![2]);
}

#[test]
fn per_kind_dispatch_routes_unknown_byte_through_on_other_kind() {
    let mut rt = AnimRuntime::with_slots(4);
    // 0x18 is the canonical "Other" byte in OpaqueRecordKind.
    rt.play(1, synth_opaque_record(0x18, 16), 0).unwrap();
    let mut host = PerKindHost::default();
    rt.tick(&mut host);
    assert_eq!(host.other, vec![(1, 0x18)]);
}

#[test]
fn per_kind_default_routes_back_through_on_opaque_record() {
    // Verifies the back-compat path: a host that only overrides
    // `on_opaque_record` still receives every per-kind dispatch
    // because each per-kind method's default body routes there.
    struct LegacyHost {
        seen: Vec<u16>,
    }
    impl Host for LegacyHost {
        fn on_opaque_record(
            &mut self,
            _actor: u8,
            kind: RecordKind,
            _record: &[u8],
            _frame_counter: u32,
        ) -> bool {
            let header_a = match kind {
                RecordKind::Kind2 => 0x02,
                RecordKind::Kind3 => 0x03,
                RecordKind::KindA => 0x0A,
                RecordKind::Keyframe => 0x06,
                RecordKind::Other(v) => v,
            };
            self.seen.push(header_a);
            true
        }
    }
    let mut rt = AnimRuntime::with_slots(4);
    rt.play(0, synth_opaque_record(0x02, 16), 0).unwrap();
    rt.play(1, synth_opaque_record(0x05, 16), 0).unwrap();
    rt.play(2, synth_opaque_record(0x18, 16), 0).unwrap();
    let mut host = LegacyHost { seen: vec![] };
    rt.tick(&mut host);
    // All three per-kind paths funnelled into on_opaque_record.
    assert_eq!(host.seen, vec![0x02, 0x05, 0x18]);
}

#[test]
fn helper_methods_have_no_op_defaults() {
    // Smoke test: NullHost's per-helper defaults do nothing, but
    // the trait methods must still be callable (i.e. the trait
    // compiles + dispatches without engine-side scaffolding).
    let mut host = NullHost;
    host.on_movement_step(0, &[], 0);
    host.on_render_loop(0, &[]);
    host.on_child_step_iter(0, 1);
    host.on_per_bone_interp(0, &[], 0);
    host.on_special_effect_spawn(0, 0);
}

#[test]
fn staged_ids_below_0x10_resolve_direct() {
    // Idle / walk / reactions / poses / equipment swings all play
    // their action-table entry as-is.
    for q in 0u8..ART_ANIM_ID_BASE {
        assert_eq!(
            resolve_staged_anim(q),
            StagedAnimTarget::Direct { slot: q },
            "id {q:#x}"
        );
    }
}

#[test]
fn staged_art_ids_select_bank_record_q_minus_0x10() {
    for q in ART_ANIM_ID_BASE..=0x3F {
        match resolve_staged_anim(q) {
            StagedAnimTarget::ArtBank { record, .. } => {
                assert_eq!(record, q - 0x10, "id {q:#x}")
            }
            other => panic!("id {q:#x} resolved to {other:?}"),
        }
    }
}

#[test]
fn staged_ids_0x10_and_0x1a_install_at_slot_0x11_others_at_0x10() {
    // The FUN_8004AD80 rewrite: q == 0x10 / 0x1A -> dynamic slot B
    // (0x11); every other art id -> dynamic slot A (0x10).
    let slot_of = |q: u8| match resolve_staged_anim(q) {
        StagedAnimTarget::ArtBank { slot, .. } => slot,
        other => panic!("id {q:#x} resolved to {other:?}"),
    };
    assert_eq!(slot_of(0x10), DYNAMIC_ART_SLOT_B);
    assert_eq!(slot_of(0x1A), DYNAMIC_ART_SLOT_B);
    for q in [0x11u8, 0x12, 0x19, 0x1B, 0x1C, 0x2F, 0x3F] {
        assert_eq!(slot_of(q), DYNAMIC_ART_SLOT_A, "id {q:#x}");
    }
}
