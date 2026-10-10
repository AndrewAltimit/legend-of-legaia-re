//! The register-state emulator's transform ops, checked against references and a live COP2 capture.

use super::*;

// ----- Gte register-state emulator tests -----

#[test]
fn gte_default_state_is_identity_with_no_translation() {
    let g = Gte::new();
    assert_eq!(g.rot, GteMat3::IDENTITY);
    assert_eq!(g.trans, GteVec3::default());
    assert_eq!(g.h, DEFAULT_H);
    assert_eq!(g.flag, 0);
    assert_eq!(g.zsf3, ROT_ONE);
    assert_eq!(g.zsf4, ROT_ONE);
}

#[test]
fn gte_rtps_pushes_one_sxy_per_call() {
    let mut g = Gte::new();
    g.set_viewport(320, 240);
    g.trans = GteVec3::new(0, 0, ROT_ONE * 1024);
    g.v[0] = GteVec3::new(0, 0, 0);
    let xy = g.rtps();
    assert_eq!(xy.x, 160);
    assert_eq!(xy.y, 120);
    // SXY FIFO: latest in slot 2, slot 1 = previous (default), slot 0 = older.
    assert_eq!(g.sxy_fifo[2], xy);
}

#[test]
fn gte_rtpt_pushes_three_vertices_in_fifo_order() {
    let mut g = Gte::new();
    g.set_viewport(320, 240);
    g.trans = GteVec3::new(0, 0, ROT_ONE * 1024);
    g.v[0] = GteVec3::new(0, 0, 0);
    // V1 to the right.
    g.v[1] = GteVec3::from_f32_q12(100.0, 0.0, 0.0);
    // V2 up.
    g.v[2] = GteVec3::from_f32_q12(0.0, -100.0, 0.0);
    let [s0, s1, s2] = g.rtpt();
    // After 3 RTPS calls, FIFO holds [s0, s1, s2] in order.
    assert_eq!(g.sxy_fifo[0], s0);
    assert_eq!(g.sxy_fifo[1], s1);
    assert_eq!(g.sxy_fifo[2], s2);
    assert_eq!(s0.x, 160);
    assert_eq!(s0.y, 120);
    assert!(s1.x > 160, "V1 right of center: {}", s1.x);
    assert!(s2.y < 120, "V2 above center: {}", s2.y);
}

#[test]
fn gte_rtps_sets_mac_and_ir_registers() {
    let mut g = Gte::new();
    g.set_viewport(320, 240);
    g.trans = GteVec3::new(10, 20, ROT_ONE * 100);
    g.v[0] = GteVec3::new(0, 0, 0);
    let _ = g.rtps();
    // MAC = post-rotation view (rot=identity, so view = trans).
    assert_eq!(g.mac1, 10);
    assert_eq!(g.mac2, 20);
    assert_eq!(g.mac3, ROT_ONE as i64 * 100);
    // IR1 / IR2 fit in i16 (10, 20).
    assert_eq!(g.ir1, 10);
    assert_eq!(g.ir2, 20);
    // IR3 saturates to i16::MAX (mac3 = 409_600 > 32767).
    assert_eq!(g.ir3, i16::MAX as i32);
    assert_ne!(g.flag & flag_bits::IR3_SATURATED, 0);
    assert_ne!(g.flag & flag_bits::ANY_ERROR, 0);
}

#[test]
fn gte_rtps_behind_camera_overflows_divide_not_mac3_neg() {
    // A behind/on-plane vertex is NOT a special hardware case. SZ3 clamps to
    // 0 and the perspective divide overflows to the 0x1FFFF quotient, raising
    // DIVIDE_OVERFLOW. Hardware does not raise MAC3_OVERFLOW_NEG for a merely
    // behind-camera vertex - that bit is reserved for a genuine 44-bit MAC3
    // overflow. Matches gte_rtps_internal in the Beetle-validated reference.
    let mut g = Gte::new();
    g.set_viewport(320, 240);
    // view.z == 0 exactly: the divide overflows, but pushing SZ3 = 0 is not a
    // saturation, so SZ3_OTZ stays clear.
    g.v[0] = GteVec3::new(0, 0, 0);
    g.rtps();
    assert_ne!(g.flag & flag_bits::DIVIDE_OVERFLOW, 0);
    assert_eq!(g.flag & flag_bits::MAC3_OVERFLOW_NEG, 0);
    assert_eq!(g.flag & flag_bits::SZ3_OTZ_SATURATED, 0);

    // Strictly behind the camera: the negative MAC3 pushes SZ3 = 0 via the
    // clamp, so SZ3_OTZ joins DIVIDE_OVERFLOW; still no MAC3_OVERFLOW_NEG.
    let mut g = Gte::new();
    g.set_viewport(320, 240);
    g.trans = GteVec3::new(0, 0, -(ROT_ONE * 100));
    g.v[0] = GteVec3::new(0, 0, 0);
    g.rtps();
    assert_ne!(g.flag & flag_bits::DIVIDE_OVERFLOW, 0);
    assert_ne!(g.flag & flag_bits::SZ3_OTZ_SATURATED, 0);
    assert_eq!(g.flag & flag_bits::MAC3_OVERFLOW_NEG, 0);
}

/// Independent reference projection mirroring the Beetle-validated
/// `gte_rtps_internal` (psxrecomp `runtime/src/gte.cpp`) in the HARDWARE
/// register scale and with the reference's OFX-before-shift ordering -
/// deliberately different arithmetic from `Gte::rtps`, which holds MAC/IR in
/// q19.12 and adds OFX *after* the `>>16` floor-shift. Returns the
/// hardware-comparable subset: the saturated SXY, the pushed SZ3, and the
/// FLAG bits that do not depend on the q19.12 MAC/IR representation. The
/// IR/MAC-saturation bits are excluded on purpose - they diverge by the
/// documented scale convention, which is exactly why a naive register-exact
/// cosim comparison cannot be used against this port.
fn reference_rtps(
    rot: &GteMat3,
    trans: GteVec3,
    v: GteVec3,
    h: i32,
    ofx: i32,
    ofy: i32,
) -> (ScreenXY, u16, u32) {
    // Reference MAC = (RT*V + TR<<12) >> 12. `rot_trans` yields RT*V + trans
    // with trans already in q19.12, so the `>>12` recovers the hardware-scale
    // MAC the real GTE latches.
    let view = rot_trans(rot, v, trans);
    let mac1 = (view.x >> ROT_FRAC_BITS) as i64;
    let mac2 = (view.y >> ROT_FRAC_BITS) as i64;
    let mac3 = view.z >> ROT_FRAC_BITS;

    let mut flag = 0u32;
    // push_sz: clamp MAC3 into the u16 SZ FIFO, flagging on saturation.
    let sz3 = if mac3 < 0 {
        flag |= flag_bits::SZ3_OTZ_SATURATED;
        0
    } else if mac3 > u16::MAX as i32 {
        flag |= flag_bits::SZ3_OTZ_SATURATED;
        u16::MAX as i32
    } else {
        mac3
    } as u16;

    // Screen numerators are the i16-saturated hardware IR1/IR2.
    let ir1 = mac1.clamp(SXY_MIN as i64, SXY_MAX as i64);
    let ir2 = mac2.clamp(SXY_MIN as i64, SXY_MAX as i64);

    let (recip, overflow) = gte_divide(h as u16, sz3);
    if overflow {
        flag |= flag_bits::DIVIDE_OVERFLOW;
    }

    // Reference ordering: add the fixed-point OFX/OFY (`<<16`), THEN
    // floor-shift by 16. The port does `(IR*recip)>>16` first and adds an
    // integer-pixel OFX after; the two agree because retail's OFX/OFY have
    // zero low 16 bits (SetGeomOffset, FUN_8005B7F8). This sweep machine-
    // checks that equivalence across the full numerator-sign space.
    let sx = (((ofx as i64) << 16) + ir1 * recip) >> 16;
    let sy = (((ofy as i64) << 16) + ir2 * recip) >> 16;

    // Final screen coords saturate to the hardware signed-11-bit range,
    // raising SX2/SY2 - distinct from the i16 IR-numerator clamp above.
    if sx < SX_MIN as i64 || sx > SX_MAX as i64 {
        flag |= flag_bits::SX2_SATURATED;
    }
    if sy < SX_MIN as i64 || sy > SX_MAX as i64 {
        flag |= flag_bits::SY2_SATURATED;
    }
    let sxy = ScreenXY::new(sx as i32, sy as i32).saturate_sxy();
    (sxy, sz3, flag)
}

#[test]
fn gte_rtps_matches_independent_reference_over_input_sweep() {
    // Closes the RTPS integration-validation gap: `Gte::rtps` (q19.12 MAC/IR,
    // OFX added after the floor-shift) is checked against `reference_rtps`, an
    // independent second implementation in the hardware register scale with
    // the reference's OFX-before-shift ordering. Everything that determines
    // the on-screen projection - SXY, SZ, divide overflow, SXY saturation,
    // SZ3 clamp - is compared with ZERO tolerance across a wide input sweep.
    //
    // The IR/MAC-saturation FLAG bits (and thus ANY_ERROR) are intentionally
    // outside the mask: the port keeps MAC/IR in q19.12, so those bits diverge
    // from hardware by the documented scale convention. This test pins exactly
    // which cop2 outputs are hardware-comparable - the contract a future
    // real-silicon/cosim oracle would compare against.
    const MASK: u32 = flag_bits::DIVIDE_OVERFLOW
        | flag_bits::SZ3_OTZ_SATURATED
        | flag_bits::SX2_SATURATED
        | flag_bits::SY2_SATURATED;

    let rots = [
        GteMat3::IDENTITY,
        GteMat3::rot_x(0.6),
        GteMat3::rot_y(1.2),
        GteMat3::rot_z(-0.9),
        GteMat3::rot_y(0.7).mul(&GteMat3::rot_x(0.3)),
    ];
    // Depths in q19.12: on/behind plane, near the H/2 overflow knee, and far.
    let depths = [
        -(ROT_ONE * 100),
        0,
        ROT_ONE / 2,
        ROT_ONE * 2,
        ROT_ONE * 50,
        ROT_ONE * 200,
        ROT_ONE * 1000,
        ROT_ONE * 8000,
    ];
    let laterals = [
        GteVec3::new(0, 0, 0),
        GteVec3::new(ROT_ONE * 30, -(ROT_ONE * 20), 0),
        // Far off-axis: under the identity rotation this drives the projected
        // SXY past the ±0x400 hardware screen range, exercising SX2/SY2.
        GteVec3::new(ROT_ONE * 25000, -(ROT_ONE * 25000), 0),
    ];
    let verts = [
        GteVec3::new(0, 0, 0),
        GteVec3::new(100, 0, 0),
        GteVec3::new(0, 80, 0),
        GteVec3::new(-64, 40, 128),
        GteVec3::new(500, -300, 200),
    ];
    let hs = [256, 320];
    let offsets = [(0, 0), (160, 120)];

    let mut cases = 0u32;
    let mut flag_coverage = 0u32;
    for rot in &rots {
        for &depth in &depths {
            for lat in &laterals {
                let trans = GteVec3::new(lat.x, lat.y, depth);
                for v in &verts {
                    for &h in &hs {
                        for &(ofx, ofy) in &offsets {
                            let mut g = Gte::new();
                            g.rot = *rot;
                            g.trans = trans;
                            g.h = h;
                            g.ofx = ofx;
                            g.ofy = ofy;
                            g.v[0] = *v;
                            let sxy_port = g.rtps();

                            let (sxy_ref, sz_ref, flag_ref) =
                                reference_rtps(rot, trans, *v, h, ofx, ofy);

                            let ctx = format!("depth={depth} v={v:?} h={h} of=({ofx},{ofy})");
                            assert_eq!(sxy_port, sxy_ref, "SXY mismatch @ {ctx}");
                            assert_eq!(g.sz_fifo[3], sz_ref, "SZ mismatch @ {ctx}");
                            assert_eq!(
                                g.flag & MASK,
                                flag_ref & MASK,
                                "FLAG-subset mismatch @ {ctx}"
                            );
                            flag_coverage |= flag_ref & MASK;
                            cases += 1;
                        }
                    }
                }
            }
        }
    }
    // Guard against the sweep silently collapsing to nothing, and confirm the
    // flag comparison is non-vacuous - every masked bit fires somewhere, so
    // the equality above is exercised on set bits, not just zeros.
    assert_eq!(cases, 2400, "expected the full cartesian sweep");
    assert_eq!(
        flag_coverage, MASK,
        "sweep did not exercise every masked bit"
    );
}

#[test]
fn gte_nclip_writes_mac0_and_returns_signed_area() {
    let mut g = Gte::new();
    // Manually populate SXY FIFO.
    g.sxy_fifo = [
        ScreenXY::new(0, 0),
        ScreenXY::new(10, 0),
        ScreenXY::new(0, 10),
    ];
    let r = g.nclip();
    assert_eq!(r, 100);
    assert_eq!(g.mac0, 100);
}

#[test]
fn gte_avsz3_writes_otz_and_mac0() {
    let mut g = Gte::new();
    g.zsf3 = ROT_ONE; // sum-bucket scale (default)
    g.sz_fifo = [0, 100, 200, 300];
    let otz = g.avsz3();
    // (100 + 200 + 300) = 600. With zsf3=4096 ⇒ 600*4096 = 2_457_600.
    // OTZ = 2_457_600 >> 12 = 600. MAC0 = 2_457_600.
    assert_eq!(otz, 600);
    assert_eq!(g.otz, 600);
    assert_eq!(g.mac0, 2_457_600);
}

#[test]
fn gte_avsz4_uses_all_four_sz_entries() {
    let mut g = Gte::new();
    g.zsf4 = ROT_ONE;
    g.sz_fifo = [50, 100, 150, 200];
    let otz = g.avsz4();
    assert_eq!(otz, 500);
}

#[test]
fn gte_otz_saturates_high_to_u16_max() {
    let mut g = Gte::new();
    g.zsf3 = ROT_ONE;
    // 3 * 0xFFFF = 196_605, * 4096 = 805_273_600, >> 12 = 196_605.
    // 196_605 > 65_535 ⇒ clamp + flag.
    g.sz_fifo = [0, u16::MAX, u16::MAX, u16::MAX];
    let otz = g.avsz3();
    assert_eq!(otz, u16::MAX);
    assert_ne!(g.flag & flag_bits::SZ3_OTZ_SATURATED, 0);
}

#[test]
fn gte_mvmva_with_identity_passes_vector_through() {
    let mut g = Gte::new();
    g.mvmva(
        &GteMat3::IDENTITY,
        GteVec3::new(100, 200, 300),
        GteVec3::default(),
        true, // shift by ROT_FRAC_BITS
        false,
    );
    // identity (q3.12) * (100, 200, 300) gives (100*4096, 200*4096,
    // 300*4096) before the shift; shifted by 12 returns the original
    // vector. IR1/2/3 then take the same values (within i16 range).
    assert_eq!(g.mac1, 100);
    assert_eq!(g.mac2, 200);
    assert_eq!(g.mac3, 300);
    assert_eq!(g.ir1, 100);
    assert_eq!(g.ir2, 200);
    assert_eq!(g.ir3, 300);
}

#[test]
fn gte_mvmva_no_shift_keeps_full_precision() {
    let mut g = Gte::new();
    g.mvmva(
        &GteMat3::IDENTITY,
        GteVec3::new(100, 200, 300),
        GteVec3::default(),
        false,
        false,
    );
    // identity * v = q12 view. Without shift MAC keeps the full
    // q12 product (each element scaled by ROT_ONE).
    assert_eq!(g.mac1, 100 * ROT_ONE as i64);
    assert_eq!(g.mac2, 200 * ROT_ONE as i64);
    assert_eq!(g.mac3, 300 * ROT_ONE as i64);
    // IR clamps to i16::MAX.
    assert_eq!(g.ir1, i16::MAX as i32);
    assert_ne!(g.flag & flag_bits::IR1_SATURATED, 0);
}

#[test]
fn gte_mvmva_lm_clamps_to_zero_minimum() {
    let mut g = Gte::new();
    // Negative input + LM=true ⇒ IR clamps to 0, FLAG sets sat bit.
    g.mvmva(
        &GteMat3::IDENTITY,
        GteVec3::new(-50, -100, -200),
        GteVec3::default(),
        true,
        true, // LM
    );
    assert_eq!(g.ir1, 0);
    assert_eq!(g.ir2, 0);
    assert_eq!(g.ir3, 0);
    assert_ne!(g.flag & flag_bits::IR1_SATURATED, 0);
}

#[test]
fn gte_clear_flag_resets() {
    let mut g = Gte::new();
    g.flag = 0xFFFF_FFFF;
    g.clear_flag();
    assert_eq!(g.flag, 0);
}

#[test]
fn gte_rtpt_matches_camera_transform() {
    // Verify the register-state RTPT produces the same SXY as the
    // higher-level Camera::transform shim.
    let mut g = Gte::new();
    g.set_viewport(320, 240);
    g.trans = GteVec3::new(0, 0, ROT_ONE * 512);
    g.rot = GteMat3::rot_y(0.3);
    let v = [
        GteVec3::from_f32_q12(50.0, 0.0, 0.0),
        GteVec3::from_f32_q12(-50.0, 0.0, 0.0),
        GteVec3::from_f32_q12(0.0, 50.0, 0.0),
    ];
    g.v = v;
    let [s0, s1, s2] = g.rtpt();

    let cam = Camera {
        rot: g.rot,
        trans: g.trans,
        h: g.h,
        ofx: g.ofx,
        ofy: g.ofy,
    };
    let p0 = cam.transform(v[0]).screen_xy.saturate_sxy();
    let p1 = cam.transform(v[1]).screen_xy.saturate_sxy();
    let p2 = cam.transform(v[2]).screen_xy.saturate_sxy();
    assert_eq!(s0, p0);
    assert_eq!(s1, p1);
    assert_eq!(s2, p2);
}

// ---------------------------------------------------------------------------
// Real-COP2 oracle: cross-check `Gte::rtpt` against a live PS1 execution.
//
// The in-repo `reference_rtps` sweep validates the UNR divide against a second
// independent implementation, but both share this crate's scale conventions, so
// a convention shared by BOTH is invisible to it. This test closes that gap by
// replaying real GTE RTPT (func 0x30) traffic captured from a Beetle-validated
// PS1 static recompilation's COP2 register file (its `gte.cpp` gte_divide is the
// same UNR algorithm this crate ports). Each capture record carries the inputs
// (RT, TR, H, OFX/OFY, V0/V1/V2) and the resulting SXY/SZ FIFOs + FLAG.
//
// Sony-free: the capture holds game-derived vertex/matrix bytes, so it lives
// OUTSIDE the repo and is supplied via `LEGAIA_RECOMP_GTE_CAPTURE` (a path to a
// JSON array of ring-dump entries). With the var unset the test skip-passes,
// exactly like the disc-gated integration tests - CI never needs the capture.
//
// Comparison is restricted to the hardware-scale-comparable subset (this crate
// holds MAC/IR in q19.12): the SZ FIFO, the SXY FIFO, and the FLAG bits
// {DIVIDE_OVERFLOW, SZ3_OTZ, SX2, SY2}. IR/MAC-saturation bits and ANY_ERROR
// diverge by the documented scale convention and are excluded.
//
// This crate saturates the SXY FIFO to the hardware signed-11-bit screen range
// (-0x400..=+0x3FF), so SXY value + SX2/SY2 flag are bit-exact against real
// COP2 on EVERY record, on- or off-screen. (A prior i16 SXY clamp - shared by
// this crate and its `reference_rtps`, hence invisible to the self-consistent
// in-repo sweep - diverged off-screen; this capture oracle is what surfaced and
// fixed it.) The SZ FIFO (the divide's own depth) is likewise checked on every
// record.

#[cfg(test)]
fn recomp_arr_i64(v: &serde_json::Value, key: &str) -> Vec<i64> {
    v[key]
        .as_array()
        .unwrap_or_else(|| panic!("capture entry missing array field {key}"))
        .iter()
        .map(|x| x.as_i64().expect("capture array element not an integer"))
        .collect()
}

#[test]
fn rtpt_matches_recomp_cop2_capture() {
    let path = match std::env::var("LEGAIA_RECOMP_GTE_CAPTURE") {
        Ok(p) if !p.is_empty() => p,
        _ => {
            eprintln!(
                "SKIP rtpt_matches_recomp_cop2_capture: set LEGAIA_RECOMP_GTE_CAPTURE \
                 to an external ring-dump JSON to run the real-COP2 cross-check"
            );
            return;
        }
    };
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read capture {path}: {e}"));
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(&raw).expect("capture is not a JSON array of ring entries");

    const SUBSET: u32 = flag_bits::DIVIDE_OVERFLOW
        | flag_bits::SZ3_OTZ_SATURATED
        | flag_bits::SX2_SATURATED
        | flag_bits::SY2_SATURATED;

    let (mut processed, mut sz_checked, mut strict_checked, mut div_ovf_seen) = (0, 0, 0, 0);

    for e in &entries {
        // Only RTPT (func 0x30): all three SXY/SZ slots come from this op.
        let cmd = i64::from_str_radix(
            e["cmd"].as_str().unwrap_or("0x0").trim_start_matches("0x"),
            16,
        )
        .unwrap_or(0);
        if cmd & 0x3F != 0x30 {
            continue;
        }
        processed += 1;

        let rt = recomp_arr_i64(e, "RT"); // 9, row-major, i16 q3.12
        let tr = recomp_arr_i64(e, "TR"); // 3, raw control-reg integer scale
        let h = e["H"].as_i64().expect("H") as i32;
        let ofx = e["OFX"].as_i64().expect("OFX") as i32;
        let ofy = e["OFY"].as_i64().expect("OFY") as i32;
        let vs = [
            recomp_arr_i64(e, "V0"),
            recomp_arr_i64(e, "V1"),
            recomp_arr_i64(e, "V2"),
        ];

        // Map the recomp's hardware-scale inputs into this crate's units:
        //   rot: i16 q3.12 verbatim.
        //   vertex: << 12 so `rot.mul_vec` (which >>12's the product) yields the
        //           rotated vector in q19.12 (this crate's MAC scale).
        //   trans: << 12 -> q19.12.
        let rot = GteMat3 {
            m: [
                [rt[0] as i16, rt[1] as i16, rt[2] as i16],
                [rt[3] as i16, rt[4] as i16, rt[5] as i16],
                [rt[6] as i16, rt[7] as i16, rt[8] as i16],
            ],
        };
        let mut g = Gte::new();
        g.rot = rot;
        g.trans = GteVec3::new(
            (tr[0] << ROT_FRAC_BITS) as i32,
            (tr[1] << ROT_FRAC_BITS) as i32,
            (tr[2] << ROT_FRAC_BITS) as i32,
        );
        g.h = h;
        // Retail OFX/OFY are s15.16 with zero fractional; this crate stores the
        // integer pixel offset.
        g.ofx = ofx >> 16;
        g.ofy = ofy >> 16;
        for (i, v) in vs.iter().enumerate() {
            g.v[i] = GteVec3::new(
                (v[0] << ROT_FRAC_BITS) as i32,
                (v[1] << ROT_FRAC_BITS) as i32,
                (v[2] << ROT_FRAC_BITS) as i32,
            );
        }
        g.rtpt();

        // Expected outputs from the real COP2 register file.
        let exp_s = [
            recomp_arr_i64(e, "S0"),
            recomp_arr_i64(e, "S1"),
            recomp_arr_i64(e, "S2"),
        ];
        let exp_sz = recomp_arr_i64(e, "SZ"); // [SZ1, SZ2, SZ3] = V0.z, V1.z, V2.z
        let exp_flag = u32::from_str_radix(
            e["FLAG"].as_str().unwrap_or("0x0").trim_start_matches("0x"),
            16,
        )
        .unwrap_or(0);

        // SZ FIFO: the divide's own depth bucket. Checked on EVERY record - this
        // is the perspective-divide input and it must be bit-exact.
        assert_eq!(
            [
                g.sz_fifo[1] as i64,
                g.sz_fifo[2] as i64,
                g.sz_fifo[3] as i64
            ],
            [exp_sz[0], exp_sz[1], exp_sz[2]],
            "SZ FIFO mismatch (seq {:?})",
            e["seq"]
        );
        sz_checked += 1;

        // Strict SXY + flag-subset check on EVERY record. The SXY FIFO now
        // saturates to the hardware ±0x400 screen range (SX2/SY2 flags and the
        // clamped value both match), so on- and off-screen verts are bit-exact.
        for (i, s) in exp_s.iter().enumerate() {
            assert_eq!(
                (g.sxy_fifo[i].x as i64, g.sxy_fifo[i].y as i64),
                (s[0], s[1]),
                "SXY[{i}] mismatch (seq {:?})",
                e["seq"]
            );
        }
        assert_eq!(
            g.flag & SUBSET,
            exp_flag & SUBSET,
            "FLAG subset mismatch (seq {:?})",
            e["seq"]
        );
        strict_checked += 1;
        if exp_flag & flag_bits::DIVIDE_OVERFLOW != 0 {
            div_ovf_seen += 1;
        }
    }

    eprintln!(
        "rtpt_matches_recomp_cop2_capture: processed={processed} sz_checked={sz_checked} \
         strict={strict_checked} div_overflow_exercised={div_ovf_seen}"
    );
    assert!(
        processed >= 8,
        "capture had too few RTPT records ({processed}) to be a meaningful oracle"
    );
    assert!(
        strict_checked > 0,
        "no on-screen RTPT records to strictly cross-check"
    );
}
