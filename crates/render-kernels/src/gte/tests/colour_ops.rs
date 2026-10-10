//! Lighting and colour ops, the RGB FIFO and the cycle accounting.

use super::*;

// ---------------------------------------------------------------------
// Lighting / colour ops (NCDS / NCDT / DCPL / DPCS / DPCT / INTPL /
// SQR / OP / GPF / GPL).
// ---------------------------------------------------------------------

#[test]
fn rgb_fifo_starts_empty() {
    let g = Gte::new();
    for entry in g.rgb_fifo {
        assert_eq!(entry, [0; 4]);
    }
}

#[test]
fn ncds_pushes_rgb_fifo_entry() {
    let mut g = Gte::new();
    g.rgbc = [0xFF, 0xFF, 0xFF, 0x00];
    g.ir0 = 0; // disable far-color blend so we get pure light pass.
    // Configure light so a unit normal becomes a small intensity.
    g.light = GteMat3::IDENTITY;
    g.light_color = GteMat3::IDENTITY;
    g.v[0] = GteVec3::new(ROT_ONE, 0, 0);
    let _ = g.ncds();
    // The newest RGB should be in slot 2.
    let r = g.rgb_fifo[2];
    // alpha (CODE byte) preserved
    assert_eq!(r[3], 0x00);
}

#[test]
fn ncdt_writes_three_fifo_entries() {
    let mut g = Gte::new();
    g.rgbc = [0x80, 0x80, 0x80, 0x10];
    g.ir0 = 0;
    g.light = GteMat3::IDENTITY;
    g.light_color = GteMat3::IDENTITY;
    g.v[0] = GteVec3::new(ROT_ONE, 0, 0);
    g.v[1] = GteVec3::new(0, ROT_ONE, 0);
    g.v[2] = GteVec3::new(0, 0, ROT_ONE);
    let _ = g.ncdt();
    // Each FIFO entry should preserve the alpha byte.
    for entry in g.rgb_fifo {
        assert_eq!(entry[3], 0x10);
    }
}

#[test]
fn dcpl_modulates_rgbc_through_ir() {
    let mut g = Gte::new();
    g.rgbc = [0xFF, 0x00, 0x00, 0x00];
    g.ir1 = 0x10;
    g.ir2 = 0x10;
    g.ir3 = 0x10;
    g.ir0 = 0; // no far-color blend
    let out = g.dcpl();
    // R = (IR1 * 0xFF) >> 4 = (16 * 255) >> 4 = 255; G/B = 0
    assert_eq!(out[0], 0xFF);
    assert_eq!(out[1], 0);
    assert_eq!(out[2], 0);
}

#[test]
fn dpcs_blends_rgbc_toward_far_color_at_ir0_max() {
    let mut g = Gte::new();
    g.rgbc = [0x00, 0x00, 0x00, 0x00];
    // Far color full white in q3.12.
    g.far_color = GteVec3::new(0xFF, 0xFF, 0xFF);
    // IR0 at full-blend. Conventional GTE max for IR0 is 4096 (1.0 in q12).
    g.ir0 = ROT_ONE;
    let out = g.dpcs();
    // Full blend toward far_color should deliver close to (255, 255, 255).
    // Allow ±1 for rounding.
    assert!(out[0] >= 254);
    assert!(out[1] >= 254);
    assert!(out[2] >= 254);
}

#[test]
fn dpcs_zero_blend_preserves_rgbc() {
    let mut g = Gte::new();
    g.rgbc = [0x80, 0x40, 0x20, 0x10];
    g.far_color = GteVec3::new(0xFF, 0xFF, 0xFF);
    g.ir0 = 0; // no blend
    let out = g.dpcs();
    assert_eq!(out[0], 0x80);
    assert_eq!(out[1], 0x40);
    assert_eq!(out[2], 0x20);
    assert_eq!(out[3], 0x10);
}

#[test]
fn intpl_writes_macs_from_ir_and_far_color() {
    let mut g = Gte::new();
    g.ir1 = 100;
    g.ir2 = 200;
    g.ir3 = 50;
    g.far_color = GteVec3::new(500, 100, -50);
    g.ir0 = ROT_ONE; // full blend
    g.intpl();
    // MAC1 = IR1 + ((FC.x - IR1) * IR0 / 4096) = 100 + (400 * 1) = 500
    assert_eq!(g.mac1, 500);
    assert_eq!(g.mac2, 100);
    assert_eq!(g.mac3, -50);
    assert_eq!(g.ir1, 500);
    assert_eq!(g.ir2, 100);
    assert_eq!(g.ir3, -50);
}

#[test]
fn intpl_zero_blend_is_noop() {
    let mut g = Gte::new();
    g.ir1 = 100;
    g.ir2 = 200;
    g.ir3 = 50;
    g.far_color = GteVec3::new(500, 100, -50);
    g.ir0 = 0;
    g.intpl();
    assert_eq!(g.mac1, 100);
    assert_eq!(g.mac2, 200);
    assert_eq!(g.mac3, 50);
}

#[test]
fn sqr_squares_ir_and_writes_macs() {
    let mut g = Gte::new();
    g.ir1 = 30;
    g.ir2 = -40;
    g.ir3 = 50;
    g.sqr(false);
    assert_eq!(g.mac1, 900);
    assert_eq!(g.mac2, 1600);
    assert_eq!(g.mac3, 2500);
}

#[test]
fn op_cross_product_with_unit_diagonal() {
    let mut g = Gte::new();
    // D = (1,1,1) in q3.12; IR = (a, b, c).
    g.rot = GteMat3::IDENTITY;
    g.ir1 = 100;
    g.ir2 = 200;
    g.ir3 = 300;
    // Pre-shift so we don't have to undo q12 scaling for the unit test.
    g.op(true);
    // mac1 = D2 * IR3 - D3 * IR2 = 4096 * 300 - 4096 * 200 = 4096 * 100
    // After shift_frac: mac1 = 100, mac2 = D3*IR1 - D1*IR3 = 100-300 = -200,
    // mac3 = D1*IR2 - D2*IR1 = 200 - 100 = 100.
    assert_eq!(g.mac1, 100);
    assert_eq!(g.mac2, -200);
    assert_eq!(g.mac3, 100);
}

#[test]
fn gpf_multiplies_ir_by_ir0_and_writes_mac() {
    let mut g = Gte::new();
    g.ir0 = 2;
    g.ir1 = 5;
    g.ir2 = 10;
    g.ir3 = 15;
    g.gpf(false);
    assert_eq!(g.mac1, 10);
    assert_eq!(g.mac2, 20);
    assert_eq!(g.mac3, 30);
}

#[test]
fn gpl_accumulates_ir_times_ir0() {
    let mut g = Gte::new();
    g.mac1 = 100;
    g.mac2 = 200;
    g.mac3 = 300;
    g.ir0 = 3;
    g.ir1 = 4;
    g.ir2 = 5;
    g.ir3 = 6;
    g.gpl(false);
    assert_eq!(g.mac1, 100 + 12);
    assert_eq!(g.mac2, 200 + 15);
    assert_eq!(g.mac3, 300 + 18);
}

#[test]
fn intpl_chains_into_dpcs_pipeline() {
    // INTPL writes MAC/IR; DPCS reads IR0 + RGBC + FC. Verify the
    // composition makes sense.
    let mut g = Gte::new();
    g.ir1 = 100;
    g.ir2 = 100;
    g.ir3 = 100;
    g.far_color = GteVec3::new(200, 200, 200);
    g.ir0 = ROT_ONE / 2; // half blend
    g.intpl();
    assert_eq!(g.ir1, 150); // 100 + 50%*100
}

#[test]
fn rgb_fifo_advances_oldest_first() {
    let mut g = Gte::new();
    g.rgbc = [0x10, 0x20, 0x30, 0x40];
    g.far_color = GteVec3::default();
    g.ir0 = 0;
    let _ = g.dpcs();
    assert_eq!(g.rgb_fifo[2], [0x10, 0x20, 0x30, 0x40]);
    g.rgbc = [0x50, 0x60, 0x70, 0x80];
    let _ = g.dpcs();
    assert_eq!(g.rgb_fifo[2], [0x50, 0x60, 0x70, 0x80]);
    assert_eq!(g.rgb_fifo[1], [0x10, 0x20, 0x30, 0x40]);
}

#[test]
fn copop_cycle_counts_match_hardware_table() {
    // Spot-check the canonical Nocash entries.
    assert_eq!(CopOp::Rtps.cycles(), 15);
    assert_eq!(CopOp::Rtpt.cycles(), 23);
    assert_eq!(CopOp::Nclip.cycles(), 8);
    assert_eq!(CopOp::Avsz3.cycles(), 5);
    assert_eq!(CopOp::Avsz4.cycles(), 6);
    assert_eq!(CopOp::Mvmva.cycles(), 8);
    assert_eq!(CopOp::Ncds.cycles(), 19);
    assert_eq!(CopOp::Ncdt.cycles(), 44);
    assert_eq!(CopOp::Nccs.cycles(), 17);
    assert_eq!(CopOp::Ncct.cycles(), 39);
    assert_eq!(CopOp::Cc.cycles(), 11);
    assert_eq!(CopOp::Cdp.cycles(), 13);
    assert_eq!(CopOp::Ncs.cycles(), 14);
    assert_eq!(CopOp::Nct.cycles(), 30);
    assert_eq!(CopOp::Sqr.cycles(), 5);
    assert_eq!(CopOp::Op.cycles(), 6);
    assert_eq!(CopOp::Gpf.cycles(), 5);
    assert_eq!(CopOp::Gpl.cycles(), 5);
    assert_eq!(CopOp::Dcpl.cycles(), 8);
    assert_eq!(CopOp::Dpcs.cycles(), 8);
    assert_eq!(CopOp::Dpct.cycles(), 17);
    assert_eq!(CopOp::Intpl.cycles(), 8);
}

#[test]
fn cycle_accumulator_charges_per_op() {
    let mut g = Gte::new();
    assert_eq!(g.cycles, 0);
    let _ = g.rtps();
    assert_eq!(g.cycles, CopOp::Rtps.cycles() as u64);
    let _ = g.rtpt();
    assert_eq!(
        g.cycles,
        (CopOp::Rtps.cycles() + CopOp::Rtpt.cycles()) as u64
    );
    g.reset_cycles();
    assert_eq!(g.cycles, 0);
}

#[test]
fn cycle_accumulator_works_for_lighting_ops() {
    let mut g = Gte::new();
    g.rgbc = [0x80, 0x80, 0x80, 0];
    g.v[0] = GteVec3::new(0, 0, 0);
    let _ = g.ncds();
    assert_eq!(g.cycles, CopOp::Ncds.cycles() as u64);
    let _ = g.cdp();
    assert_eq!(
        g.cycles,
        (CopOp::Ncds.cycles() + CopOp::Cdp.cycles()) as u64
    );
}

#[test]
fn ncs_pushes_modulated_rgb() {
    let mut g = Gte::new();
    g.rgbc = [0xFF, 0x80, 0x40, 0x12];
    g.light = GteMat3::IDENTITY;
    g.light_color = GteMat3::IDENTITY;
    g.v[0] = GteVec3::new(ROT_ONE, ROT_ONE, ROT_ONE);
    let out = g.ncs();
    // Code byte should round-trip through the alpha channel.
    assert_eq!(out[3], 0x12);
    // RGB FIFO advanced.
    assert_eq!(g.rgb_fifo[2], out);
}

#[test]
fn nct_runs_three_pass_lighting() {
    let mut g = Gte::new();
    g.rgbc = [0x80, 0x80, 0x80, 0];
    g.light = GteMat3::IDENTITY;
    g.light_color = GteMat3::IDENTITY;
    g.v[0] = GteVec3::new(ROT_ONE, 0, 0);
    g.v[1] = GteVec3::new(0, ROT_ONE, 0);
    g.v[2] = GteVec3::new(0, 0, ROT_ONE);
    let outs = g.nct();
    assert_eq!(outs.len(), 3);
    // Three different normals → three different RGB outputs.
    assert!(outs[0] != outs[1] || outs[1] != outs[2]);
}

#[test]
fn nccs_runs_double_light_pass_relative_to_ncs() {
    // light_color matrix that scales by 0.5 per channel - the second
    // pass in NCCS should darken the result vs NCS.
    let mut lc = GteMat3::IDENTITY;
    lc.m[0][0] = (ROT_ONE / 2) as i16;
    lc.m[1][1] = (ROT_ONE / 2) as i16;
    lc.m[2][2] = (ROT_ONE / 2) as i16;

    // NCS reference run.
    let mut g = Gte::new();
    g.rgbc = [0xFF, 0xFF, 0xFF, 0];
    g.light = GteMat3::IDENTITY;
    g.light_color = lc;
    g.v[0] = GteVec3::new(ROT_ONE, ROT_ONE, ROT_ONE);
    let ncs_rgb = g.ncs();

    // NCCS - same inputs.
    let mut g2 = Gte::new();
    g2.rgbc = g.rgbc;
    g2.light = g.light;
    g2.light_color = g.light_color;
    g2.v[0] = g.v[0];
    let nccs_rgb = g2.nccs();
    assert!(nccs_rgb[0] <= ncs_rgb[0]);
    assert!(nccs_rgb[1] <= ncs_rgb[1]);
    assert!(nccs_rgb[2] <= ncs_rgb[2]);
}

#[test]
fn cdp_pushes_rgb_with_code_byte_preserved() {
    let mut g = Gte::new();
    g.rgbc = [0xFF, 0xFF, 0xFF, 0x42];
    g.ir1 = 0;
    g.ir2 = 0;
    g.ir3 = 0;
    g.light_color = GteMat3::IDENTITY;
    g.far_color = GteVec3::new(0x100, 0x200, 0x300);
    g.ir0 = ROT_ONE; // full blend toward far_color
    let _ = g.cdp();
    // RGB FIFO advanced and code byte preserved.
    assert_eq!(g.rgb_fifo[2][3], 0x42);
}

#[test]
fn cc_does_not_blend_against_far_color() {
    // CC should NOT touch far_color. Set IR very low so the post-modulate
    // result is well below saturation, then run CC with a large
    // far_color. If the implementation accidentally blended toward
    // far_color, the result would saturate to 0xFF; if it doesn't,
    // the modulated value stays small.
    let mut g = Gte::new();
    g.rgbc = [0x40, 0x40, 0x40, 0x12];
    g.ir1 = 0x10;
    g.ir2 = 0x10;
    g.ir3 = 0x10;
    g.light_color = GteMat3::IDENTITY;
    g.far_color = GteVec3::new(0xFFFF, 0xFFFF, 0xFFFF);
    g.ir0 = ROT_ONE;
    let out = g.cc();
    assert_eq!(out[3], 0x12);
    // Without far-color blending, the result should be small.
    assert!(
        out[0] < 0x80,
        "cc unexpectedly blended toward far_color (got {})",
        out[0]
    );
}

#[test]
fn ncds_saturates_overflow_to_ff() {
    let mut g = Gte::new();
    // Drive a big intensity through to force saturation.
    g.rgbc = [0xFF, 0xFF, 0xFF, 0x00];
    // Light matrix that amplifies aggressively.
    let mut amp = GteMat3::IDENTITY;
    amp.m[0][0] = i16::MAX; // 32767 - large q3.12 -> after >>12 stays positive.
    amp.m[1][1] = i16::MAX;
    amp.m[2][2] = i16::MAX;
    g.light = amp;
    g.light_color = amp;
    g.v[0] = GteVec3::new(ROT_ONE, ROT_ONE, ROT_ONE);
    g.ir0 = 0;
    let out = g.ncds();
    assert_eq!(out, [0xFF, 0xFF, 0xFF, 0x00]);
    assert!(g.flag & flag_bits::ANY_ERROR != 0);
}
