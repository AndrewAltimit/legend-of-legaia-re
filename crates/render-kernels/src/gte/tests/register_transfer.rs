//! Register-transfer and memory ops.

use super::*;

// ---------------- GTE Phase 6: register-transfer + memory ops ---------

#[test]
fn pack_unpack_round_trips_signed_pair() {
    let pairs: &[(i16, i16)] = &[
        (0, 0),
        (-1, -1),
        (i16::MIN, i16::MAX),
        (1234, -5678),
        (i16::MAX, i16::MIN),
    ];
    for (lo, hi) in pairs {
        let packed = pack_i16_lo_hi(*lo, *hi);
        let (l2, h2) = unpack_i16_lo_hi(packed);
        assert_eq!(*lo, l2);
        assert_eq!(*hi, h2);
    }
}

#[test]
fn mtc2_then_mfc2_round_trips_v0_xy() {
    let mut g = Gte::new();
    // V0.x = 0x1234, V0.y = -2 (0xFFFE) packed as low/high i16 in cop2cr0.
    let val = pack_i16_lo_hi(0x1234, -2);
    g.mtc2(0, val);
    assert_eq!(g.v[0].x, 0x1234);
    assert_eq!(g.v[0].y, -2);
    let read = g.mfc2(0);
    assert_eq!(read, val);
}

#[test]
fn mtc2_v0_z_sign_extends_low_half() {
    let mut g = Gte::new();
    g.mtc2(1, 0xFFFFu32); // -1 as i16 in low half
    assert_eq!(g.v[0].z, -1);
    // mfc2 sign-extends back to a 32-bit -1.
    assert_eq!(g.mfc2(1), 0xFFFFFFFFu32);
}

#[test]
fn mtc2_rgbc_writes_byte_lane_layout() {
    let mut g = Gte::new();
    g.mtc2(6, 0xCC_BB_AA_99); // [0x99, 0xAA, 0xBB, 0xCC] little-endian
    assert_eq!(g.rgbc, [0x99, 0xAA, 0xBB, 0xCC]);
    assert_eq!(g.mfc2(6), 0xCC_BB_AA_99);
}

#[test]
fn mtc2_sxyp_pushes_through_fifo() {
    let mut g = Gte::new();
    // Write three values via SXYP (cop2cr15) - older entries shift down.
    let a = pack_i16_lo_hi(10, 20);
    let b = pack_i16_lo_hi(30, 40);
    let c = pack_i16_lo_hi(50, 60);
    g.mtc2(15, a);
    g.mtc2(15, b);
    g.mtc2(15, c);
    // After three pushes, FIFO is [a, b, c] in slot 0..2.
    assert_eq!(g.sxy_fifo[0], ScreenXY::new(10, 20));
    assert_eq!(g.sxy_fifo[1], ScreenXY::new(30, 40));
    assert_eq!(g.sxy_fifo[2], ScreenXY::new(50, 60));
}

#[test]
fn ctc2_then_cfc2_round_trips_rotation_row() {
    let mut g = Gte::new();
    let val = pack_i16_lo_hi(2048, -1024);
    g.ctc2(0, val); // RT11RT12
    assert_eq!(g.rot.m[0][0], 2048);
    assert_eq!(g.rot.m[0][1], -1024);
    assert_eq!(g.cfc2(0), val);
}

#[test]
fn ctc2_then_cfc2_round_trips_translation_z() {
    let mut g = Gte::new();
    g.ctc2(7, 0x1234_5678); // TRZ
    assert_eq!(g.trans.z, 0x1234_5678u32 as i32);
    assert_eq!(g.cfc2(7), 0x1234_5678);
}

#[test]
fn ctc2_h_writes_low_16_bits() {
    let mut g = Gte::new();
    g.ctc2(26, 0xDEAD_0140); // H is 16-bit; only 0x0140 lands.
    assert_eq!(g.h, 0x0140);
    assert_eq!(g.cfc2(26), 0x0140);
}

#[test]
fn ctc2_zsf3_sign_extends_to_i32() {
    let mut g = Gte::new();
    g.ctc2(29, 0xFFFFu32); // -1 in low half
    assert_eq!(g.zsf3, -1);
    // cfc2 returns the low 16 bits.
    assert_eq!(g.cfc2(29), 0xFFFF);
}

#[test]
fn lwc2_loads_packed_vertex_xy_through_memory() {
    let mut g = Gte::new();
    let mut mem = VecMem::new(1024);
    // Stage V0.x=100, V0.y=-50 at addr 0x40.
    mem.write_u32_at(0x40, pack_i16_lo_hi(100, -50));
    g.lwc2(&mut mem, 0, 0x40);
    assert_eq!(g.v[0].x, 100);
    assert_eq!(g.v[0].y, -50);
}

#[test]
fn swc2_stores_data_register_to_memory() {
    let mut g = Gte::new();
    let mut mem = VecMem::new(1024);
    g.write_data(6, 0x11_22_33_44); // RGBC bytes
    g.swc2(&mut mem, 6, 0x80);
    assert_eq!(&mem.bytes[0x80..0x84], &[0x44, 0x33, 0x22, 0x11]);
}

#[test]
fn lwc2_swc2_round_trips_one_vertex() {
    let mut g = Gte::new();
    let mut mem = VecMem::new(1024);
    g.write_data(0, pack_i16_lo_hi(7, -8));
    g.write_data(1, sign_extend_i16(9));
    g.swc2(&mut mem, 0, 0x10);
    g.swc2(&mut mem, 1, 0x14);
    // Reset GTE then load back.
    let mut g2 = Gte::new();
    g2.lwc2(&mut mem, 0, 0x10);
    g2.lwc2(&mut mem, 1, 0x14);
    assert_eq!(g2.v[0].x, 7);
    assert_eq!(g2.v[0].y, -8);
    assert_eq!(g2.v[0].z, 9);
}

#[test]
fn load_vertices_pulls_three_packed_pairs() {
    let mut g = Gte::new();
    let mut mem = VecMem::new(64);
    // 8 bytes per vertex: u32 xy pair, u32 z (sign-extended low half).
    mem.write_u32_at(0, pack_i16_lo_hi(1, 2));
    mem.write_u32_at(4, sign_extend_i16(3));
    mem.write_u32_at(8, pack_i16_lo_hi(4, 5));
    mem.write_u32_at(12, sign_extend_i16(6));
    mem.write_u32_at(16, pack_i16_lo_hi(7, 8));
    mem.write_u32_at(20, sign_extend_i16(9));
    g.load_vertices(&mut mem, 0);
    assert_eq!(g.v[0], GteVec3::new(1, 2, 3));
    assert_eq!(g.v[1], GteVec3::new(4, 5, 6));
    assert_eq!(g.v[2], GteVec3::new(7, 8, 9));
}

#[test]
fn cycles_charge_per_register_op() {
    let mut g = Gte::new();
    g.reset_cycles();
    g.mtc2(0, 0);
    g.mfc2(0);
    g.ctc2(5, 0);
    g.cfc2(5);
    // 4 transfers at 1 cycle each.
    assert_eq!(g.cycles, 4);
}

#[test]
fn cycles_charge_per_memory_op() {
    let mut g = Gte::new();
    let mut mem = NullMem;
    g.reset_cycles();
    g.lwc2(&mut mem, 0, 0);
    g.swc2(&mut mem, 0, 0);
    // 2 memory transfers at 1 cycle each.
    assert_eq!(g.cycles, 2);
}

#[test]
fn read_data_irgb_packs_5bit_per_channel() {
    let mut g = Gte::new();
    // IR1 = 0x1F << 7 = 0x0F80 (clamps to 0x1F when shifted >>7).
    g.ir1 = 0x0F80;
    g.ir2 = 0x0F80;
    g.ir3 = 0x0F80;
    let v = g.read_data(28); // IRGB
    assert_eq!(v, 0x1F | (0x1F << 5) | (0x1F << 10));
}

#[test]
fn read_data_lzcr_counts_leading_zeros_for_positive_lzcs() {
    let mut g = Gte::new();
    g.lzcs = 1; // 0x00000001 has 31 leading zeros.
    assert_eq!(g.read_data(31), 31);
}

#[test]
fn read_data_lzcr_counts_leading_ones_for_negative_lzcs() {
    let mut g = Gte::new();
    g.lzcs = -1i32; // 0xFFFFFFFF has 32 leading ones.
    assert_eq!(g.read_data(31), 32);
}

#[test]
fn write_data_lzcs_caches_source_for_lzcr_read() {
    let mut g = Gte::new();
    // Round-trip via MTC2: writing 0x4000_0000 to LZCS gives LZCR = 1.
    g.mtc2(30, 0x4000_0000);
    assert_eq!(g.read_data(31), 1);
}

#[test]
fn ctc2_flag_is_writeable_for_capture_replay() {
    let mut g = Gte::new();
    // Flag is normally set by the GTE itself, but engines replaying a
    // captured trace need to write the FLAG through CTC2 to reproduce
    // the post-instruction state.
    g.ctc2(31, flag_bits::IR1_SATURATED | flag_bits::ANY_ERROR);
    assert_eq!(g.flag, flag_bits::IR1_SATURATED | flag_bits::ANY_ERROR);
    assert_eq!(g.cfc2(31), flag_bits::IR1_SATURATED | flag_bits::ANY_ERROR);
}

#[test]
fn lwc2_into_v0_then_rtps_matches_direct_setup() {
    // Set up two GTEs identically - one via direct `g.v[0] = ...`, one
    // via LWC2 from memory - and verify RTPS produces the same SXY.
    let mut g_direct = Gte::new();
    g_direct.set_viewport(320, 240);
    g_direct.trans = GteVec3::new(0, 0, ROT_ONE * 1024);
    g_direct.v[0] = GteVec3::new(0, 0, 0);
    let xy_direct = g_direct.rtps();

    let mut g_mem = Gte::new();
    g_mem.set_viewport(320, 240);
    g_mem.trans = GteVec3::new(0, 0, ROT_ONE * 1024);
    let mut mem = VecMem::new(64);
    mem.write_u32_at(0, pack_i16_lo_hi(0, 0));
    mem.write_u32_at(4, sign_extend_i16(0));
    g_mem.lwc2(&mut mem, 0, 0); // V0.xy
    g_mem.lwc2(&mut mem, 1, 4); // V0.z
    let xy_mem = g_mem.rtps();

    assert_eq!(xy_direct, xy_mem);
}

#[test]
fn null_mem_returns_zero_loads_and_drops_stores() {
    let mut g = Gte::new();
    let mut mem = NullMem;
    g.lwc2(&mut mem, 6, 0x100);
    assert_eq!(g.rgbc, [0; 4]);
    // Stores are silently dropped.
    g.write_data(6, 0xDEAD_BEEF);
    g.swc2(&mut mem, 6, 0x100); // no panic
}
