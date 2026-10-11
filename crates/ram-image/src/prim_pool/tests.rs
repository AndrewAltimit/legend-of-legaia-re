use super::*;

/// Build one packet: `[tag][payload...]`, returning the bytes.
fn packet(len: u8, next: u32, words: &[u32]) -> Vec<u8> {
    assert_eq!(len as usize, words.len());
    let mut v = Vec::new();
    v.extend_from_slice(&(((len as u32) << 24) | (next & 0x00FF_FFFF)).to_le_bytes());
    for w in words {
        v.extend_from_slice(&w.to_le_bytes());
    }
    v
}

fn xy(x: i16, y: i16) -> u32 {
    ((y as u16 as u32) << 16) | (x as u16 as u32)
}

/// The GP0 rectangle opcode encodes size in bits 4-3: `0x74..0x77` is 8x8
/// and `0x7C..0x7F` is 16x16. An earlier reading had these swapped, which
/// made every 8x8 sprite decode as `Sprt16` and draw at double size.
#[test]
fn sprite_opcode_sizes_follow_the_gp0_encoding() {
    let mut buf = packet(3, 0xFFFFFF, &[0x7400_0000, xy(10, 20), 0x0000_1234]);
    assert!(matches!(
        decode(&buf, 0x8000_0000).as_slice(),
        [Prim::Sprt8 { .. }]
    ));
    buf = packet(3, 0xFFFFFF, &[0x7C00_0000, xy(10, 20), 0x0000_1234]);
    assert!(matches!(
        decode(&buf, 0x8000_0000).as_slice(),
        [Prim::Sprt16 { .. }]
    ));
}

/// The free-size `SPRT` (GP0 `0x64`, 4 payload words) is the opcode
/// retail builds every window nine-slice out of - a 4x4 corner off the
/// system-UI page. Without this arm the decoder walks straight past the
/// whole frame chrome and reports a window with no border.
#[test]
fn free_size_sprite_carries_its_own_extent() {
    let buf = packet(
        4,
        0xFFFFFF,
        &[0x6480_8080, xy(8, 152), 0x7FC2_00A0, 0x0004_0004],
    );
    match decode(&buf, 0x8000_0000).as_slice() {
        [
            Prim::Sprt {
                pos,
                uv,
                clut,
                size,
                ..
            },
        ] => {
            assert_eq!(*pos, (8, 152));
            assert_eq!(*uv, (0xA0, 0));
            assert_eq!(*clut, 0x7FC2);
            assert_eq!(*size, (4, 4));
        }
        other => panic!("free-size SPRT not decoded: {other:?}"),
    }
}

#[test]
fn sprite_bounds_use_the_real_extent() {
    let s8 = &decode(
        &packet(3, 0xFFFFFF, &[0x7400_0000, xy(10, 20), 0]),
        0x8000_0000,
    )[0]
    .bounds();
    assert_eq!(*s8, (10, 20, 18, 28));
    let s16 = &decode(
        &packet(3, 0xFFFFFF, &[0x7C00_0000, xy(10, 20), 0]),
        0x8000_0000,
    )[0]
    .bounds();
    assert_eq!(*s16, (10, 20, 26, 36));
}

/// Untextured polys carry no texture page at all, so a reader that only
/// knows the textured opcodes silently drops them - and flat/Gouraud
/// geometry is a large share of a real field frame.
#[test]
fn untextured_polygon_families_decode() {
    let f3 = packet(4, 0xFFFFFF, &[0x2000_0000, xy(0, 0), xy(10, 0), xy(0, 10)]);
    let f4 = packet(
        5,
        0xFFFFFF,
        &[0x2800_0000, xy(0, 0), xy(10, 0), xy(0, 10), xy(10, 10)],
    );
    let g3 = packet(
        6,
        0xFFFFFF,
        &[0x3000_0000, xy(0, 0), 0, xy(10, 0), 0, xy(0, 10)],
    );
    let g4 = packet(
        8,
        0xFFFFFF,
        &[
            0x3800_0000,
            xy(0, 0),
            0,
            xy(10, 0),
            0,
            xy(0, 10),
            0,
            xy(10, 10),
        ],
    );
    for (bytes, want) in [
        (f3, "POLY_F3"),
        (f4, "POLY_F4"),
        (g3, "POLY_G3"),
        (g4, "POLY_G4"),
    ] {
        let got = decode(&bytes, 0x8000_0000);
        assert_eq!(got.len(), 1, "{want} did not decode");
        assert_eq!(got[0].kind(), want);
        assert!(!got[0].is_textured(), "{want} must not report a texture");
        assert!(got[0].clut_tpage().is_none());
    }
}

/// The chain is the draw order, and it is not address order: a packet
/// linked later wins on a machine with no depth buffer. Build a pool whose
/// link order is the reverse of its address order and check the walk
/// follows the links.
#[test]
fn chain_walk_follows_links_not_addresses() {
    let base = 0x8000_0000u32;
    // Three F3 packets at offsets 0, 20, 40; link 40 -> 0 -> 20 -> end.
    let mut pool = Vec::new();
    pool.extend_from_slice(&packet(
        4,
        base + 20,
        &[0x2000_0000, xy(0, 0), xy(9, 0), xy(0, 9)],
    ));
    pool.extend_from_slice(&packet(
        4,
        0xFFFFFF,
        &[0x2000_0000, xy(1, 1), xy(9, 1), xy(1, 9)],
    ));
    pool.extend_from_slice(&packet(
        4,
        base,
        &[0x2000_0000, xy(2, 2), xy(9, 2), xy(2, 9)],
    ));
    let walked = chain_walk(&pool, base, 40);
    assert_eq!(walked.len(), 3);
    assert_eq!(
        walked.iter().map(|c| c.offset).collect::<Vec<_>>(),
        vec![40, 0, 20],
        "walk must follow next_addr, not ascending address"
    );
    assert_eq!(walked[2].order, 2, "last walked packet wins");
}

/// A malformed pool can link a packet back into the chain; the walk must
/// terminate rather than spin.
#[test]
fn chain_walk_terminates_on_a_link_cycle() {
    let base = 0x8000_0000u32;
    let mut pool = Vec::new();
    pool.extend_from_slice(&packet(
        4,
        base + 20,
        &[0x2000_0000, xy(0, 0), xy(9, 0), xy(0, 9)],
    ));
    pool.extend_from_slice(&packet(
        4,
        base,
        &[0x2000_0000, xy(1, 1), xy(9, 1), xy(1, 9)],
    ));
    let walked = chain_walk(&pool, base, 0);
    assert_eq!(walked.len(), 2, "cycle must stop after revisiting a packet");
}

/// The walk's blind spot is reported, not hidden: a link the decoder does
/// not accept (here a `TILE` and an F3 tagged at the wrong length) lands in
/// the undecoded census keyed by `(cmd, length)`, and a decoded one does
/// not.
#[test]
fn chain_undecoded_reports_the_links_the_walk_drops() {
    let base = 0x8000_0000u32;
    let mut pool = Vec::new();
    // 0: F3, decoded -> 20.
    pool.extend_from_slice(&packet(
        4,
        base + 20,
        &[0x2000_0000, xy(0, 0), xy(9, 0), xy(0, 9)],
    ));
    // 20: TILE (cmd 0x60, 3 words), not decoded -> 36.
    pool.extend_from_slice(&packet(3, base + 36, &[0x6000_0000, xy(0, 0), xy(8, 8)]));
    // 36: F3 tagged at 5 words, not decoded -> end.
    pool.extend_from_slice(&packet(
        5,
        0xFFFFFF,
        &[0x2000_0000, xy(1, 1), xy(9, 1), xy(1, 9), 0],
    ));
    assert_eq!(chain_walk(&pool, base, 0).len(), 1);
    let und = chain_undecoded(&pool, base, 0);
    assert_eq!(und.get(&(0x60, 3)), Some(&1));
    assert_eq!(und.get(&(0x20, 5)), Some(&1));
    assert_eq!(und.len(), 2, "the decoded F3 is not counted");
}

/// `ClearOTagR` leaves every empty bucket pointing at its own predecessor;
/// that signature is how the ordering table is told apart from the packet
/// pool, and the head is the highest bucket.
#[test]
fn ot_array_is_found_by_its_cleared_signature() {
    let base = 0x8000_0000u32;
    let buckets = 128usize;
    let mut ram = vec![0u8; buckets * 4 + 64];
    for i in 0..buckets {
        let addr = base + (i * 4) as u32;
        let prev = (addr.wrapping_sub(4)) & 0x00FF_FFFF;
        ram[i * 4..i * 4 + 4].copy_from_slice(&prev.to_le_bytes());
    }
    let found = find_ot_arrays(&ram, base, 64);
    assert_eq!(found.len(), 1, "expected exactly one table");
    assert_eq!(found[0].buckets, buckets);
    assert_eq!(
        found[0].head,
        base + ((buckets - 1) * 4) as u32,
        "head is the highest bucket (reverse-cleared table)"
    );
}

/// A junk word directly above a table must not join the run. The head is
/// the top of the detected run, so a single over-extension puts the head on
/// a non-bucket and the whole walk yields nothing - a silent total loss, not
/// a degraded result. (Observed in a real capture: `0x00002020` above a
/// 2048-bucket table cost 1600 packets.)
#[test]
fn ot_run_does_not_extend_over_a_low_junk_word() {
    let base = 0x8000_0000u32;
    let buckets = 128usize;
    // Table occupies words 0..128; word 128 is junk with a zero length byte.
    let mut ram = vec![0u8; (buckets + 8) * 4];
    for i in 0..buckets {
        let addr = base + (i * 4) as u32;
        let prev = addr.wrapping_sub(4) & 0x00FF_FFFF;
        ram[i * 4..i * 4 + 4].copy_from_slice(&prev.to_le_bytes());
    }
    let junk = 0x0000_2020u32;
    ram[buckets * 4..buckets * 4 + 4].copy_from_slice(&junk.to_le_bytes());

    let found = find_ot_arrays(&ram, base, 64);
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].buckets, buckets,
        "run must stop at the real top, not swallow the junk word"
    );
    assert_eq!(
        found[0].head,
        base + ((buckets - 1) * 4) as u32,
        "head must be the last real bucket"
    );
}

/// The converse: a legitimately occupied bucket (pointing at a packet in
/// the pool, not at its own predecessor) must still count as part of the
/// table, or one filled bucket splits a table into two fragments.
#[test]
fn ot_run_spans_occupied_buckets() {
    let base = 0x8000_0000u32;
    let buckets = 128usize;
    let mut ram = vec![0u8; buckets * 4];
    for i in 0..buckets {
        let addr = base + (i * 4) as u32;
        let prev = addr.wrapping_sub(4) & 0x00FF_FFFF;
        ram[i * 4..i * 4 + 4].copy_from_slice(&prev.to_le_bytes());
    }
    // Bucket 64 received a primitive living far away in a work buffer.
    let into_pool = 0x000B_31B0u32;
    ram[64 * 4..64 * 4 + 4].copy_from_slice(&into_pool.to_le_bytes());

    let found = find_ot_arrays(&ram, base, 64);
    assert_eq!(
        found.len(),
        1,
        "an occupied bucket must not split the table"
    );
    assert_eq!(found[0].buckets, buckets);
}

#[test]
fn ft4_packet_round_trip() {
    // Synthesise one POLY_FT4: tag = (len=9, next=0xFFFFFF), then 9 words.
    let mut buf = vec![0u8; 64];
    let tag = 0x09FFFFFF_u32;
    buf[0..4].copy_from_slice(&tag.to_le_bytes());
    let cmd = 0x2C202020_u32; // cmd=0x2C, color = (0x20, 0x20, 0x20)
    buf[4..8].copy_from_slice(&cmd.to_le_bytes());
    // 4 verts at (10,10), (20,10), (10,20), (20,20)
    buf[8..12].copy_from_slice(&((10u32) | ((10u32) << 16)).to_le_bytes());
    buf[12..16].copy_from_slice(&((0xABCDu32 << 16) | 0x1020).to_le_bytes()); // uv0 + clut
    buf[16..20].copy_from_slice(&((20u32) | ((10u32) << 16)).to_le_bytes());
    buf[20..24].copy_from_slice(&((0x0019u32 << 16) | 0x3040).to_le_bytes()); // uv1 + tpage
    buf[24..28].copy_from_slice(&((10u32) | ((20u32) << 16)).to_le_bytes());
    buf[28..32].copy_from_slice(&(0x5060u32).to_le_bytes());
    buf[32..36].copy_from_slice(&((20u32) | ((20u32) << 16)).to_le_bytes());
    buf[36..40].copy_from_slice(&(0x7080u32).to_le_bytes());
    let prims = decode(&buf, 0x800AD400);
    assert_eq!(prims.len(), 1, "expected exactly one prim");
    match &prims[0] {
        Prim::PolyFt4 {
            cmd,
            color,
            verts,
            clut,
            tpage,
            ..
        } => {
            assert_eq!(*cmd, 0x2C);
            assert_eq!(*color, [0x20, 0x20, 0x20]);
            assert_eq!(verts[0], (10, 10));
            assert_eq!(verts[3], (20, 20));
            assert_eq!(*clut, 0xABCD);
            assert_eq!(*tpage, 0x0019);
        }
        _ => panic!("wrong variant"),
    }
}
