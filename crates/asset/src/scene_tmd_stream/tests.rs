use super::*;

/// Synthesize a TMD-prefixed scene stream:
/// [u32 chunk0_header = (0<<24)|tmd_body_size]
/// [TMD magic / flags / nobj]
/// [object table]
/// [body padding zeros]
/// [streaming chunks]
/// [terminator]
fn synth(nobj: u32, body_bytes: usize, chunks: &[(u8, &[u8])]) -> Vec<u8> {
    // body_bytes must be 4-aligned for the chunk0 header to land cleanly.
    assert!(
        body_bytes.is_multiple_of(4),
        "test-only synth requires 4-aligned body"
    );
    let tmd_body_size = 12 + 28 * nobj as usize + body_bytes;
    let mut buf = Vec::with_capacity(4 + tmd_body_size + 1024);
    // chunk0 header: type=0, size = TMD body bytes
    let chunk0_header = tmd_body_size as u32 & 0x00FFFFFF;
    buf.extend_from_slice(&chunk0_header.to_le_bytes());
    buf.extend_from_slice(&0x80000002u32.to_le_bytes()); // magic
    buf.extend_from_slice(&0u32.to_le_bytes()); // flags
    buf.extend_from_slice(&nobj.to_le_bytes());
    for _ in 0..nobj {
        buf.extend_from_slice(&[0u8; 28]);
    }
    buf.extend(std::iter::repeat_n(0u8, body_bytes));
    // Streaming chunks
    for (type_byte, payload) in chunks {
        let header = ((*type_byte as u32) << 24) | (payload.len() as u32 & 0x00FFFFFF);
        buf.extend_from_slice(&header.to_le_bytes());
        buf.extend_from_slice(payload);
        // Pad to 4-byte boundary.
        while !buf.len().is_multiple_of(4) {
            buf.push(0);
        }
    }
    // Terminator (low 24 bits zero).
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf
}

#[test]
fn detects_minimal_synthetic() {
    let buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
    let r = detect(&buf).expect("should detect");
    assert_eq!(r.tmd_nobj, 2);
    // TMD body = 12 (header) + 2*28 (object table) + 64 (padding)
    assert_eq!(r.tmd_size, 12 + 28 * 2 + 64);
    assert_eq!(r.tmd_range(), 4..4 + r.tmd_size);
    assert_eq!(r.tail_chunks.len(), 1);
    assert!(matches!(r.tail_chunks[0].asset_type, AssetType::Tim));
    assert!(r.tail_terminated);
}

#[test]
fn rejects_wrong_magic() {
    let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
    buf[4..8].copy_from_slice(&0x80000041u32.to_le_bytes()); // PSX-standard TMD
    assert!(detect(&buf).is_none());
}

#[test]
fn rejects_zero_nobj() {
    let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
    buf[12..16].copy_from_slice(&0u32.to_le_bytes());
    assert!(detect(&buf).is_none());
}

#[test]
fn rejects_silly_nobj() {
    let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
    buf[12..16].copy_from_slice(&0x10000u32.to_le_bytes());
    assert!(detect(&buf).is_none());
}

#[test]
fn rejects_first_u32_oob() {
    let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
    // Set a TMD body size that exceeds the file.
    let oob = (buf.len() as u32) + 0x1000;
    buf[0..4].copy_from_slice(&oob.to_le_bytes());
    assert!(detect(&buf).is_none());
}

#[test]
fn rejects_nonzero_chunk0_type() {
    // chunk0 header type byte must be 0 (TIM dispatcher) - anything else
    // is a different streaming variant we're not detecting here.
    let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
    let mut hdr = u32::from_le_bytes(buf[0..4].try_into().unwrap());
    hdr |= 0x02_000000; // type = 2 (TMD)
    buf[0..4].copy_from_slice(&hdr.to_le_bytes());
    assert!(detect(&buf).is_none());
}

#[test]
fn rejects_first_u32_too_small() {
    let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
    buf[0..4].copy_from_slice(&8u32.to_le_bytes());
    assert!(detect(&buf).is_none());
}

#[test]
fn rejects_unaligned_first_u32() {
    let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
    // Pick an unaligned TMD body size (low 24 bits).
    let unaligned: u32 = 129;
    buf[0..4].copy_from_slice(&unaligned.to_le_bytes());
    assert!(detect(&buf).is_none());
}

#[test]
fn rejects_no_streaming_tail() {
    // [u32 size][bare TMD] then random bytes that don't form a streaming chunk.
    let buf = synth(2, 64, &[]);
    // Replace terminator with garbage so neither chunk-walk nor terminator catches.
    let len = buf.len();
    let mut buf = buf;
    buf[len - 4..].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
    // Type byte 0xDE is unknown and chunk size huge → no good chunks, no terminator.
    assert!(detect(&buf).is_none());
}

#[test]
fn accepts_terminator_only_tail() {
    // Streaming tail consisting solely of a terminator.
    let buf = synth(1, 0, &[]);
    let r = detect(&buf).expect("should detect terminator-only tail");
    assert!(r.tail_terminated);
    assert!(r.tail_chunks.is_empty());
}

#[test]
fn accepts_truncated_tail() {
    // Build a stream then truncate before the terminator.
    let buf = synth(2, 16, &[(0x01, &[0u8; 0x40]), (0x02, &[0u8; 0x40])]);
    // Drop the trailing terminator bytes.
    let truncated = &buf[..buf.len() - 4];
    let r = detect(truncated).expect("truncated tail should still parse");
    assert_eq!(r.tail_chunks.len(), 2);
    assert!(!r.tail_terminated);
}

/// Build a synthetic two-list buffer: leading TMD, then `tail_count`
/// type-0x01 TIM chunks, zero-size terminator, `gap` bytes of zero
/// padding, then `cont_count` more type-0x01 chunks after the
/// terminator. Each TIM payload is a 16-byte stub starting with the PSX
/// TIM magic so the post-terminator gate (which checks payload magic)
/// recognises it.
///
/// No retail PROT entry has this shape - it models what an *over-read*
/// buffer looks like (one entry's stream plus the next entry's bytes),
/// which is what the `Continuation` arm exists to detect.
fn synth_two_list(tail_count: usize, gap: usize, cont_count: usize) -> Vec<u8> {
    // TIM payload: `[u32 0x10][12 bytes filler]` = 16 bytes.
    let tim_payload: Vec<u8> = {
        let mut v = Vec::with_capacity(16);
        v.extend_from_slice(&0x0000_0010u32.to_le_bytes());
        v.extend_from_slice(&[0u8; 12]);
        v
    };
    // The payload size we declare must clear the continuation gate's
    // `size >= 32` floor, even though the payload itself is only 16
    // bytes long. Pad the chunk body to 32 bytes.
    let chunk_body: Vec<u8> = {
        let mut v = Vec::with_capacity(32);
        v.extend_from_slice(&tim_payload);
        v.extend_from_slice(&[0u8; 16]);
        v
    };
    let tail_chunks: Vec<(u8, &[u8])> = (0..tail_count)
        .map(|_| (0x01, chunk_body.as_slice()))
        .collect();
    let mut buf = synth(2, 64, &tail_chunks);
    // Pad to gap.
    buf.extend(std::iter::repeat_n(0u8, gap));
    // Continuation chunks.
    for _ in 0..cont_count {
        let header = (0x01u32 << 24) | (chunk_body.len() as u32 & 0x00FF_FFFF);
        buf.extend_from_slice(&header.to_le_bytes());
        buf.extend_from_slice(&chunk_body);
    }
    buf
}

#[test]
fn battle_tim_chunks_finds_in_tail() {
    let buf = synth_two_list(2, 0, 0);
    let chunks = battle_tim_chunks(&buf);
    assert_eq!(chunks.len(), 2);
    assert!(chunks.iter().all(|c| c.source == WalkSource::Tail));
    for c in &chunks {
        assert_eq!(c.payload_len, 32);
        assert_eq!(c.payload_offset, c.header_offset + 4);
    }
}

#[test]
fn battle_tim_chunks_finds_continuation() {
    let buf = synth_two_list(2, 0x100, 2);
    let chunks = battle_tim_chunks(&buf);
    assert_eq!(chunks.len(), 4);
    let tail = chunks
        .iter()
        .filter(|c| c.source == WalkSource::Tail)
        .count();
    let cont = chunks
        .iter()
        .filter(|c| c.source == WalkSource::Continuation)
        .count();
    assert_eq!(tail, 2);
    assert_eq!(cont, 2);
}

#[test]
fn battle_tim_chunks_empty_for_non_scene_stream() {
    let buf = vec![0u8; 1024];
    assert!(battle_tim_chunks(&buf).is_empty());
}

/// Concatenate two full `[TMD][TIM][TIM][terminator]` sub-streams with a
/// zero-padded gap, each carrying its OWN leading TMD - i.e. what two
/// adjacent PROT entries look like when a buffer over-reads across the
/// boundary between them. Distinct from the bare post-terminator TIM
/// list `synth_two_list` models. Retail entries hold one sub-stream.
fn synth_two_sub_streams(gap: usize) -> Vec<u8> {
    let tim: &[u8] = &{
        let mut v = vec![0u8; 64];
        v[0..4].copy_from_slice(&0x0000_0010u32.to_le_bytes());
        v
    };
    let mut buf = synth(2, 64, &[(0x01, tim), (0x01, tim)]);
    buf.extend(std::iter::repeat_n(0u8, gap));
    let second = synth(3, 32, &[(0x01, tim), (0x01, tim)]);
    buf.extend_from_slice(&second);
    buf
}

#[test]
fn sub_streams_enumerates_concatenated_blocks() {
    let buf = synth_two_sub_streams(0x80);
    let subs = sub_streams(&buf);
    assert_eq!(subs.len(), 2, "should find both concatenated sub-streams");
    // Each sub-stream carries its OWN leading TMD with the expected nobj.
    assert_eq!(subs[0].base, 0);
    assert_eq!(subs[0].stream.tmd_nobj, 2);
    assert!(
        subs[1].base >= subs[0].stream.tail_end,
        "second block follows the first"
    );
    assert_eq!(subs[1].stream.tmd_nobj, 3);
    // Offsets are sub-stream-relative: the TMD always sits at +4.
    assert_eq!(subs[1].stream.tmd_range().start, 4);
}

#[test]
fn sub_streams_single_block_when_no_continuation() {
    let buf = synth(2, 64, &[(0x01, &[0x10u8; 64])]);
    let subs = sub_streams(&buf);
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].base, 0);
}

#[test]
fn sub_streams_empty_for_non_scene_stream() {
    assert!(sub_streams(&vec![0u8; 1024]).is_empty());
}

#[test]
fn battle_tim_chunks_stops_at_type_02_terminator() {
    // A type-0x02 chunk also terminates FUN_8001FE70's tail walk;
    // chunks after it should NOT be reported as Tail.
    let mut buf = synth(2, 64, &[(0x01, &[0u8; 32]), (0x02, &[0u8; 0])]);
    // Append a stray type-0x01 chunk past the type-0x02 terminator.
    // No padding gap here, so it should be reachable by the
    // continuation pass.
    let header = (0x01u32 << 24) | 32u32;
    buf.extend_from_slice(&header.to_le_bytes());
    // payload: TIM magic + filler.
    buf.extend_from_slice(&0x0000_0010u32.to_le_bytes());
    buf.extend_from_slice(&[0u8; 28]);
    let chunks = battle_tim_chunks(&buf);
    let tail: Vec<_> = chunks
        .iter()
        .filter(|c| c.source == WalkSource::Tail)
        .collect();
    let cont: Vec<_> = chunks
        .iter()
        .filter(|c| c.source == WalkSource::Continuation)
        .collect();
    assert_eq!(tail.len(), 1, "only the pre-type-0x02 chunk is in-tail");
    assert_eq!(
        cont.len(),
        1,
        "post-terminator chunk surfaces in continuation"
    );
}

/// Build a one-object TMD whose object 0 holds the given vertices, plus a
/// second object with a deliberately different extent (so the object-0 vs
/// whole-pool split is observable).
fn shape_of(obj0: &[(i16, i16, i16)], rest: &[(i16, i16, i16)]) -> legaia_tmd::Tmd {
    fn obj(verts: &[(i16, i16, i16)]) -> legaia_tmd::Object {
        legaia_tmd::Object {
            header: legaia_tmd::ObjectHeader {
                vert_top: 0,
                n_vert: verts.len() as u32,
                normal_top: 0,
                n_normal: 0,
                prim_top: 0,
                n_primitive: 0,
                scale: 0,
            },
            vertices: verts
                .iter()
                .map(|&(x, y, z)| legaia_tmd::Vector { x, y, z, _pad: 0 })
                .collect(),
            normals: Vec::new(),
            primitives_byte_offset: 0,
            primitives_byte_size: 0,
            claimed_n_primitive: 0,
            primitives_psx_walk: None,
        }
    }
    let mut objects = vec![obj(obj0)];
    if !rest.is_empty() {
        objects.push(obj(rest));
    }
    legaia_tmd::Tmd {
        header: legaia_tmd::Header {
            id: 0x8000_0002,
            flags: 0,
            nobj: objects.len() as u32,
            flist_bit_set: false,
        },
        objects,
    }
}

#[test]
fn shell_shape_reads_a_strictly_one_sided_pool_as_open_toward_neg_x() {
    // town01's stage shell shape: everything at X >= 0, Z symmetric.
    let tmd = shape_of(&[(0, 0, -10751), (10751, -4000, 10751)], &[]);
    let s = shell_shape(&tmd).expect("shell shape");
    assert_eq!(s.open, OpenSide::NegX);
    assert_eq!(s.open_fraction, 0.0);
    assert!(s.is_half_shell());
    assert_eq!(
        s.describe(),
        "battle-stage backdrop (drawn twice; authored half open toward -X)"
    );
}

#[test]
fn shell_shape_tolerates_a_small_overhang_past_the_plane() {
    // The map-kingdom dome shape: X symmetric, Z barely crosses 0.
    let tmd = shape_of(&[(-12155, 0, -1260), (12155, -10522, 12155)], &[]);
    let s = shell_shape(&tmd).expect("shell shape");
    assert_eq!(s.open, OpenSide::NegZ);
    assert!(s.open_fraction > 0.0 && s.open_fraction < HALF_SHELL_MAX_OPEN_FRACTION);
    assert!(s.is_half_shell());
}

#[test]
fn shell_shape_rejects_a_symmetric_pool() {
    let tmd = shape_of(&[(-1000, 0, -1000), (1000, -500, 1000)], &[]);
    let s = shell_shape(&tmd).expect("shell shape");
    assert_eq!(s.open_fraction, 0.5);
    assert!(!s.is_half_shell());
    assert_eq!(s.describe(), "battle-stage backdrop (full surround)");
}

#[test]
fn shell_shape_measures_object_0_while_the_all_objects_form_sees_the_props() {
    // Object 0 is one-sided; the trailing near-prop object straddles the
    // plane. Retail draws only object 0, so the two forms disagree - which
    // is exactly why `shell_shape` is the object-0 one.
    let tmd = shape_of(
        &[(0, 0, -9535), (9535, -3000, 9535)],
        &[(-2574, 0, -500), (2574, -100, 500)],
    );
    let s0 = shell_shape(&tmd).expect("object 0 shape");
    assert_eq!(s0.open, OpenSide::NegX);
    assert!(s0.is_half_shell());

    let all = shell_shape_all_objects(&tmd).expect("whole-pool shape");
    assert_eq!(all.min[0], -2574);
    assert!(!all.is_half_shell());
}

#[test]
fn shell_shape_is_none_without_vertices() {
    assert!(shell_shape(&shape_of(&[], &[])).is_none());
}
