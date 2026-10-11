use super::*;

/// Build a `.MAP` `+0x10000` block: header up to `+0x12`, records at
/// `body_off`.
fn block(records: &[[u8; 5]]) -> Vec<u8> {
    let body_off = 0x20u16;
    let mut b = vec![0u8; 0x20 + records.len() * REGION_RECORD_STRIDE];
    b[0xE..0x10].copy_from_slice(&body_off.to_le_bytes());
    b[0x10..0x12].copy_from_slice(&(records.len() as u16).to_le_bytes());
    for (i, r) in records.iter().enumerate() {
        let off = body_off as usize + i * REGION_RECORD_STRIDE;
        b[off..off + 5].copy_from_slice(r);
    }
    b
}

#[test]
fn scan_matches_half_open_box() {
    // Record box x [4, 10), z [2, 8) (raw order x0,z0,x1,z1).
    let b = block(&[[4, 2, 10, 8, 3]]);
    let t = RegionTable::parse(&b).unwrap();
    // `min <= t < max` per the disassembly's slt chain:
    // x_min <= tx && tx < x_max && z_min <= tz && tz < z_max.
    let mut c = 0;
    assert!(t.scan(&mut c, 4, 2).is_some());
    let mut c = 0;
    assert!(t.scan(&mut c, 9, 7).is_some());
    let mut c = 0;
    assert!(t.scan(&mut c, 10, 7).is_none(), "x max is exclusive");
    let mut c = 0;
    assert!(t.scan(&mut c, 9, 8).is_none(), "z max is exclusive");
    let mut c = 0;
    assert!(t.scan(&mut c, 3, 7).is_none(), "x min is inclusive bound");
}

#[test]
fn scan_normalises_swapped_corners() {
    // Same box with swapped corner order: (10,8)-(4,2). The scan
    // normalises per-axis min/max (the lbu/sltu min-max pairs at
    // 0x80018030..0x80018088).
    let b = block(&[[10, 8, 4, 2, 3]]);
    let t = RegionTable::parse(&b).unwrap();
    let mut c = 0;
    assert!(t.scan(&mut c, 5, 5).is_some());
}

#[test]
fn scan_widens_degenerate_boxes() {
    // x0 == x1 == 6: x_max += 2 → x in [6, 8). z0 == z1 == 4:
    // z_min -= 2 → z in [2, 4).
    let b = block(&[[6, 4, 6, 4, 0]]);
    let t = RegionTable::parse(&b).unwrap();
    let mut c = 0;
    assert!(t.scan(&mut c, 6, 3).is_some());
    let mut c = 0;
    assert!(t.scan(&mut c, 7, 2).is_some());
    let mut c = 0;
    assert!(t.scan(&mut c, 8, 3).is_none());
    let mut c = 0;
    assert!(t.scan(&mut c, 6, 4).is_none(), "z max stays exclusive");
}

#[test]
fn scan_is_resumable() {
    // Two overlapping regions; the cursor resumes past the first match
    // exactly like the retail gp+0x608 iterator.
    let b = block(&[[0, 0, 0x20, 0x20, 2], [0, 0, 0x10, 0x10, 5]]);
    let t = RegionTable::parse(&b).unwrap();
    let mut c = 0;
    let first = t.scan(&mut c, 5, 5).unwrap();
    assert_eq!(first.index, 0);
    assert_eq!(first.kind(), 2);
    let second = t.scan(&mut c, 5, 5).unwrap();
    assert_eq!(second.index, 1);
    assert_eq!(second.kind(), 5);
    assert!(t.scan(&mut c, 5, 5).is_none());
}

#[test]
fn refresh_builds_mask_and_latches_type01_box() {
    // A type-0 region and a type-4 region both containing the tile:
    // mask = (1<<0) | (1<<4); attrs latch the type-0 record's raw bytes
    // in store order [+0, +3, +2, +1] (sb chain at 0x8001816c..0x80018190).
    let b = block(&[[4, 2, 10, 8, 0], [0, 0, 0x20, 0x20, 4]]);
    let t = RegionTable::parse(&b).unwrap();
    let (mask, attrs) = refresh_region_attributes(Some(&t), 5, 5, false);
    assert_eq!(mask, (1 << 0) | (1 << 4));
    assert_eq!(attrs.box_bytes, [4, 8, 10, 2]);
    assert_eq!(attrs.kind, 0);
}

#[test]
fn refresh_defaults_when_no_type01_match() {
    // Only a type-4 region: mask bit 4 set, but mask & 3 == 0 → the
    // attribute block falls back to the full-map default fill
    // (0,0,0x7F,0x7F, type 1 - the sb chain at 0x800181b8..0x800181d0).
    let b = block(&[[0, 0, 0x20, 0x20, 4]]);
    let t = RegionTable::parse(&b).unwrap();
    let (mask, attrs) = refresh_region_attributes(Some(&t), 5, 5, false);
    assert_eq!(mask, 1 << 4);
    assert_eq!(attrs, RegionAttributes::DEFAULT_FILL);
}

#[test]
fn refresh_world_map_mode_forces_default() {
    // World-map game modes (0xE/0xF) force the default fill even when a
    // type-1 region matched (0x800181d8..0x80018210).
    let b = block(&[[4, 2, 10, 8, 1]]);
    let t = RegionTable::parse(&b).unwrap();
    let (mask, attrs) = refresh_region_attributes(Some(&t), 5, 5, true);
    assert_eq!(mask, 1 << 1);
    assert_eq!(attrs, RegionAttributes::DEFAULT_FILL);
}

#[test]
fn region_type_mask_is_the_refresh_mask_without_the_latch() {
    // `FUN_801DE234`: OR `1 << type` over every containing region, and
    // nothing else - no attribute box, no world-map fallback.
    let b = block(&[[0, 0, 0x20, 0x20, 4], [4, 2, 10, 8, 1], [30, 30, 40, 40, 6]]);
    let t = RegionTable::parse(&b).unwrap();
    assert_eq!(region_type_mask(Some(&t), 5, 5), (1 << 4) | (1 << 1));
    assert_eq!(
        region_type_mask(Some(&t), 5, 5),
        refresh_region_attributes(Some(&t), 5, 5, true).0
    );
    assert_eq!(region_type_mask(Some(&t), 0x7E, 0x7E), 0);
    assert_eq!(region_type_mask(None, 5, 5), 0);
}

fn zone_table(records: &[[u8; ZONE_RECORD_STRIDE]]) -> Vec<u8> {
    let mut t = vec![records.len() as u8];
    for r in records {
        t.extend_from_slice(r);
    }
    t
}

#[test]
fn zone_query_empty_table_is_none() {
    assert!(zone_query(&[0u8], None, &RegionAttributes::DEFAULT_FILL, 5, 5).is_none());
    assert!(zone_query(&[], None, &RegionAttributes::DEFAULT_FILL, 5, 5).is_none());
}

#[test]
fn zone_query_kind1_inclusive_bbox() {
    // kind-1 arm: rec[1] <= tx && rec[2] <= tz && tx <= rec[3] &&
    // tz <= rec[4] (all inclusive - the slt chain at 0x801dbb84..).
    let mut rec = [0u8; ZONE_RECORD_STRIDE];
    rec[0] = 1;
    rec[1..5].copy_from_slice(&[4, 2, 10, 8]);
    let t = zone_table(&[rec]);
    let attrs = RegionAttributes::DEFAULT_FILL;
    let hit = zone_query(&t, None, &attrs, 10, 8).unwrap();
    assert!(hit.record.is_some(), "max corner is inclusive");
    let miss = zone_query(&t, None, &attrs, 11, 8).unwrap();
    assert!(miss.record.is_none());
}

#[test]
fn zone_query_kind0_uses_scratch_box() {
    // kind-0 arm: the record anchor (rec[1], rec[2]) AND the player tile
    // must both sit inside the latched attribute box.
    let mut rec = [0u8; ZONE_RECORD_STRIDE];
    rec[0] = 0;
    rec[1] = 6; // anchor x
    rec[2] = 6; // anchor z
    let t = zone_table(&[rec]);
    let inside = RegionAttributes {
        box_bytes: [4, 2, 10, 8],
        kind: 0,
    };
    assert!(
        zone_query(&t, None, &inside, 5, 5)
            .unwrap()
            .record
            .is_some()
    );
    // Tile outside the box → no match even though the anchor is inside.
    assert!(
        zone_query(&t, None, &inside, 11, 5)
            .unwrap()
            .record
            .is_none()
    );
    // Anchor outside the box → no match even though the tile is inside.
    let narrow = RegionAttributes {
        box_bytes: [4, 2, 5, 5],
        kind: 0,
    };
    assert!(
        zone_query(&t, None, &narrow, 5, 5)
            .unwrap()
            .record
            .is_none()
    );
}

#[test]
fn zone_query_kind_ge2_tests_region_mask() {
    // kind >= 2 arm: match iff bit `kind` of the rebuilt .MAP region
    // mask is set (`_DAT_8007B8F4 & 1 << (kind & 0x1f)`).
    let mut rec = [0u8; ZONE_RECORD_STRIDE];
    rec[0] = 4;
    let zt = zone_table(&[rec]);
    let attrs = RegionAttributes::DEFAULT_FILL;
    // .MAP table with a type-4 region containing the tile.
    let b = block(&[[0, 0, 0x20, 0x20, 4]]);
    let mt = RegionTable::parse(&b).unwrap();
    let hit = zone_query(&zt, Some(&mt), &attrs, 5, 5).unwrap();
    assert_eq!(hit.region_mask, 1 << 4);
    assert!(hit.record.is_some());
    // Tile outside the type-4 region → bit clear → no match.
    let miss = zone_query(&zt, Some(&mt), &attrs, 0x30, 5).unwrap();
    assert_eq!(miss.region_mask, 0);
    assert!(miss.record.is_none());
}

#[test]
fn zone_query_first_match_wins() {
    let mut a = [0u8; ZONE_RECORD_STRIDE];
    a[0] = 1;
    a[1..5].copy_from_slice(&[0, 0, 0x7F, 0x7F]);
    a[5] = 0xAA; // payload marker
    let mut b = a;
    b[5] = 0xBB;
    let t = zone_table(&[a, b]);
    let hit = zone_query(&t, None, &RegionAttributes::DEFAULT_FILL, 5, 5).unwrap();
    assert_eq!(hit.record.unwrap()[5], 0xAA);
}

/// A minimal synthetic `.MAP` image for the object-grid mark refresh.
fn synthetic_map() -> Vec<u8> {
    vec![0u8; 0x14000]
}

#[test]
fn grid_marks_mirror_descriptor_flags() {
    let mut map = synthetic_map();
    // Descriptor 5: flags bits 0+1 set, countdown alive.
    map[5 * 0x20 + 0x12] = 0x03;
    map[5 * 0x20 + 0x16] = 1;
    // Descriptor 7: flags bits 0+1 set, countdown expired -> bit 0 decays.
    map[7 * 0x20 + 0x12] = 0x03;
    // Tile (2, 3) owned by descriptor 5, tile (4, 4) by descriptor 7;
    // both cells start with stale 0x1000/0x2000 states.
    let o5 = MAP_OBJECT_INDEX_OFFSET + 2 * 2 + 3 * 0x100;
    let o7 = MAP_OBJECT_INDEX_OFFSET + 4 * 2 + 4 * 0x100;
    map[o5..o5 + 2].copy_from_slice(&5u16.to_le_bytes());
    map[o7..o7 + 2].copy_from_slice(&(7u16 | 0x1000).to_le_bytes());
    refresh_object_grid_marks(&mut map);
    let cell5 = u16::from_le_bytes([map[o5], map[o5 + 1]]);
    let cell7 = u16::from_le_bytes([map[o7], map[o7 + 1]]);
    assert_eq!(cell5, 5 | 0x1000 | 0x2000, "live flags mirror in");
    // Descriptor 7's bit 0 decayed (countdown 0), so 0x1000 clears and
    // only the flags-bit-1 mirror survives.
    assert_eq!(cell7, 7 | 0x2000);
    assert_eq!(
        u16::from_le_bytes([map[7 * 0x20 + 0x12], map[7 * 0x20 + 0x13]]),
        0x02,
        "expired descriptor loses flags bit 0"
    );
}

#[test]
fn grid_marks_stamp_trigger_presence_bits() {
    let mut map = synthetic_map();
    // Kind-0 table: one record at tile (10, 20); kind-1 at (10, 20) too;
    // kind-2 at (30, 40). Bodies packed after the header.
    let blk = MAP_REGION_BLOCK_OFFSET;
    for (kind, body) in [(0usize, 0x20i16), (1, 0x24), (2, 0x28)] {
        map[blk + kind * 4 + 2..blk + kind * 4 + 4].copy_from_slice(&body.to_le_bytes());
        map[blk + kind * 4 + 4..blk + kind * 4 + 6].copy_from_slice(&1i16.to_le_bytes());
    }
    map[blk + 0x20] = 10;
    map[blk + 0x21] = 20;
    map[blk + 0x24] = 10;
    map[blk + 0x25] = 20;
    map[blk + 0x28] = 30;
    map[blk + 0x29] = 40;
    refresh_object_grid_marks(&mut map);
    let cell_a = {
        let o = MAP_OBJECT_INDEX_OFFSET + 10 * 2 + 20 * 0x100;
        u16::from_le_bytes([map[o], map[o + 1]])
    };
    let cell_b = {
        let o = MAP_OBJECT_INDEX_OFFSET + 30 * 2 + 40 * 0x100;
        u16::from_le_bytes([map[o], map[o + 1]])
    };
    assert_eq!(cell_a & MAP_OBJECT_TRIGGER_BITS, 0x600, "kind 0 + kind 1");
    assert_eq!(cell_b, 0x800, "kind 2 = elevation-override marker");
}

/// Put a spawnable descriptor `d` on tile `(tx, tz)` with a zero footprint
/// offset, so its anchor tile is the tile itself.
fn place(map: &mut [u8], d: u16, tx: usize, tz: usize, extra_flags: u16) {
    let base = usize::from(d) * MAP_OBJECT_DESCRIPTOR_STRIDE;
    let flags = MAP_OBJECT_SPAWN_BIT as u16 | extra_flags;
    map[base + 0x12..base + 0x14].copy_from_slice(&flags.to_le_bytes());
    let o = MAP_OBJECT_INDEX_OFFSET + tx * 2 + tz * 0x100;
    map[o..o + 2].copy_from_slice(&d.to_le_bytes());
}

fn lut() -> [i16; 16] {
    let mut l = [0i16; 16];
    for (i, v) in l.iter_mut().enumerate() {
        *v = -(i as i16) * 16;
    }
    l
}

#[test]
fn window_sweep_only_visits_the_region_box() {
    let mut map = synthetic_map();
    place(&mut map, 5, 3, 4, 0);
    place(&mut map, 6, 40, 40, 0);
    // A box that contains only the first placement.
    let got = window_rebuild_spawns(&map, (0, 0, 10, 10), lut());
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].tile, (3, 4));
    assert_eq!(got[0].descriptor, 5);
    // The full-map default box picks up both.
    assert_eq!(
        window_rebuild_spawns(&map, (0, 0, 0x7F, 0x7F), lut()).len(),
        2
    );
    // Half-open bounds: an empty or inverted box spawns nothing.
    assert!(window_rebuild_spawns(&map, (3, 4, 3, 4), lut()).is_empty());
    assert!(window_rebuild_spawns(&map, (10, 10, 0, 0), lut()).is_empty());
}

#[test]
fn window_sweep_skips_bind_owned_anchors() {
    let mut map = synthetic_map();
    place(&mut map, 5, 3, 4, 0);
    // Mark the anchor tile as the init sweep's.
    let o = MAP_OBJECT_INDEX_OFFSET + 3 * 2 + 4 * 0x100;
    let cell = u16::from_le_bytes([map[o], map[o + 1]]) | CELL_BIND_OWNED;
    map[o..o + 2].copy_from_slice(&cell.to_le_bytes());
    assert!(window_rebuild_spawns(&map, (0, 0, 10, 10), lut()).is_empty());
    // The two sweeps are complementary: the init sweep still takes it.
    assert!(parse_map_objects(&map).iter().any(|o| o.tile == (3, 4)));
}

#[test]
fn window_sweep_takes_y_from_the_walk_grid_nibble() {
    let mut map = synthetic_map();
    place(&mut map, 5, 3, 4, 0);
    // Descriptor `+0x2` adds 7 on top of the LUT entry.
    map[5 * 0x20 + 2..5 * 0x20 + 4].copy_from_slice(&7i16.to_le_bytes());
    // Walk-grid nibble 3 -> lut[3] = -48.
    map[MAP_WALK_GRID_OFFSET + 3 + 4 * 0x80] = 0xA3;
    let got = window_rebuild_spawns(&map, (0, 0, 10, 10), lut());
    assert_eq!(got[0].world.1, -48 + 7);
    // X/Z carry the tile-centre bias, and Z *subtracts* the descriptor's
    // `+0x4` word.
    assert_eq!(got[0].world.0, 3 * 0x80 + 0x40);
    assert_eq!(got[0].world.2, 4 * 0x80 + 0x40);
}

#[test]
fn window_sweep_decodes_the_descriptor_flag_arms() {
    let mut map = synthetic_map();
    place(&mut map, 5, 1, 1, 0x2 | 0x800 | 0x1000);
    map[5 * 0x20 + 0x1E] = 1;
    map[5 * 0x20 + 8..5 * 0x20 + 0x0E].copy_from_slice(&[0x11, 0, 0x22, 0, 0x33, 0]);
    let got = window_rebuild_spawns(&map, (0, 0, 4, 4), lut());
    let s = got[0];
    assert_eq!(s.template_kind, 5, "flags bit 0x2 picks template 5");
    assert!(s.flag_40000000, "descriptor +0x1E non-zero");
    assert!(s.flag_10000000, "flags bit 0x800");
    assert!(s.flag_state_4, "flags bit 0x1000");
    assert_eq!(s.rotation, (0x11, 0x22, 0x33));
    // Non-spawnable descriptors are skipped outright.
    let mut map2 = synthetic_map();
    let o = MAP_OBJECT_INDEX_OFFSET + 2 + 0x100;
    map2[o..o + 2].copy_from_slice(&9u16.to_le_bytes());
    assert!(window_rebuild_spawns(&map2, (0, 0, 4, 4), lut()).is_empty());
}
