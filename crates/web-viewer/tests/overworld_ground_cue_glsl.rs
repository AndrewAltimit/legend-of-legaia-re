//! The play page's GLSL ground cue (`overworldGroundCue` in
//! `site/js/webgl-shaders.js`) against the engine kernel it mirrors,
//! `legaia_engine_core::overworld_ground_cue`.
//!
//! The native WGSL twin is run on a GPU by `engine-render`'s
//! `overworld_flat_depth_gpu`; nothing runs the GLSL outside a browser. This
//! test is the host-free half: it reads the function out of the page's shader
//! source, checks it re-projects the same corner, lifts the numbers its
//! arithmetic uses (the near depth, the `IR0` shift, the far colour and the
//! `DPCS` shifts and clamps), and evaluates that arithmetic over every
//! 16-bit `SZ1` and every base byte against `ground_cue_color`. An edit to
//! either side's constants, or a re-ordered step in the GLSL, fails here.
//!
//! It does not execute GLSL: a change the lifted shape cannot express (a new
//! term, a different operator) fails the shape match rather than being
//! evaluated, which is the point - such a change needs this test revisited.

use legaia_engine_core::overworld_ground_cue::{
    GROUND_CUE_IR0_SHIFT, GROUND_CUE_NEAR_SZ, GROUND_FAR_COLOR, ground_cue_color,
};

fn shader_source() -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../site/js/webgl-shaders.js");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// The `overworldGroundCue` body, whitespace collapsed.
fn cue_body(src: &str) -> String {
    let start = src
        .find("vec3 overworldGroundCue(")
        .expect("overworldGroundCue is defined in the page's vertex shader");
    let open = start + src[start..].find('{').expect("function body");
    let mut depth = 0usize;
    let mut end = open;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = open + i;
                    break;
                }
            }
            _ => {}
        }
    }
    src[open + 1..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Split a statement into its shape (every integer literal replaced by `#`)
/// and the literals in order. Float literals (`0.0`, `255.0`, `0.5`) are part
/// of the shape.
fn shape(stmt: &str) -> (String, Vec<i64>) {
    let b = stmt.as_bytes();
    let (mut out, mut nums, mut i) = (String::new(), Vec::new(), 0);
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'.';
    while i < b.len() {
        let starts = b[i].is_ascii_digit() && (i == 0 || !word(b[i - 1]));
        if !starts {
            out.push(b[i] as char);
            i += 1;
            continue;
        }
        let mut j = i;
        while j < b.len() && word(b[j]) {
            j += 1;
        }
        let lit = &stmt[i..j];
        if lit.contains('.') {
            out.push_str(lit);
        } else {
            let v = match lit.strip_prefix("0x").or_else(|| lit.strip_prefix("0X")) {
                Some(h) => i64::from_str_radix(h, 16).unwrap(),
                None => lit.parse().unwrap(),
            };
            // A unary minus binds to the literal (`ivec3(-0x8000)`).
            let v = if out.ends_with("(-") {
                out.pop();
                -v
            } else {
                v
            };
            out.push('#');
            nums.push(v);
        }
        i = j;
    }
    (out, nums)
}

/// The literals of the one statement in `body` whose shape is `want`.
fn lits(body: &str, want: &str) -> Vec<i64> {
    body.split(';')
        .map(|s| shape(s.trim()))
        .find(|(sh, _)| sh == want)
        .unwrap_or_else(|| panic!("GLSL cue has no statement shaped `{want}`:\n{body}"))
        .1
}

/// The numbers the GLSL cue computes with, lifted from its source.
struct Lifted {
    near: i64,
    ir0_shift: i64,
    base_shift: i64,
    far: i64,
    far_shift: i64,
    ir_shift: i64,
    ir_lo: i64,
    ir_hi: i64,
    mac_shift: i64,
    out_shift: i64,
    out_lo: i64,
    out_hi: i64,
}

fn lift(body: &str) -> Lifted {
    let ir0 = lits(body, "int ir0 = max(sz1 - #, #) >> #");
    assert_eq!(ir0[1], 0, "IR0 floors at zero");
    let base = lits(body, "ivec3 base = ivec3(floor(rgb * 255.0 + 0.5)) << #");
    let ir = lits(
        body,
        "ivec3 ir = clamp(((ivec3(#) << #) - base) >> #, ivec3(#), ivec3(#))",
    );
    let mac = lits(body, "ivec3 mac = (base + ir * ir0) >> #");
    let out = lits(
        body,
        "return vec3(clamp(mac >> #, ivec3(#), ivec3(#))) / 255.0",
    );
    Lifted {
        near: ir0[0],
        ir0_shift: ir0[2],
        base_shift: base[0],
        far: ir[0],
        far_shift: ir[1],
        ir_shift: ir[2],
        ir_lo: ir[3],
        ir_hi: ir[4],
        mac_shift: mac[0],
        out_shift: out[0],
        out_lo: out[1],
        out_hi: out[2],
    }
}

/// The GLSL arithmetic, one channel, with the lifted numbers.
fn glsl_channel(l: &Lifted, c: u8, sz1: i64) -> u8 {
    let ir0 = (sz1 - l.near).max(0) >> l.ir0_shift;
    let base = i64::from(c) << l.base_shift;
    let ir = (((l.far << l.far_shift) - base) >> l.ir_shift).clamp(l.ir_lo, l.ir_hi);
    let mac = (base + ir * ir0) >> l.mac_shift;
    (mac >> l.out_shift).clamp(l.out_lo, l.out_hi) as u8
}

#[test]
fn the_page_cue_reprojects_the_kernel_corner() {
    let body = cue_body(&shader_source());
    // The corner `(x1, z0)` - `fa = [x0, z0, x1, z1]`, `fb = [y00, y10, y01,
    // y11]` - is vertex 1 of the cell, the one the kernel's `SZ1` is.
    assert!(
        body.contains("float w1 = (m * vec4(fa.z, fb.y, fa.y, 1.0)).w;"),
        "the GLSL cue must depth the (x1, z0) corner:\n{body}"
    );
    assert!(
        body.contains("int sz1 = clamp(int(floor(w1 * u_curve + 0.5)), 0, 0xFFFF);"),
        "SZ1 is the corner's clip w in SZ units, rounded and saturated:\n{body}"
    );
}

#[test]
fn the_page_cue_constants_are_the_kernels() {
    let l = lift(&cue_body(&shader_source()));
    assert_eq!(l.near, i64::from(GROUND_CUE_NEAR_SZ), "near SZ");
    assert_eq!(l.ir0_shift, i64::from(GROUND_CUE_IR0_SHIFT), "IR0 shift");
    assert_eq!(l.far, i64::from(GROUND_FAR_COLOR), "far colour");
}

#[test]
fn the_page_cue_arithmetic_matches_the_kernel_everywhere() {
    let l = lift(&cue_body(&shader_source()));
    for sz1 in 0..=0xFFFFu32 {
        for c in [0u8, 0x40, 0x7F, 0x80, 0x81, 0xC0, 0xFF] {
            let want = ground_cue_color([c; 3], sz1)[0];
            let got = glsl_channel(&l, c, i64::from(sz1));
            assert_eq!(got, want, "base {c:#x} SZ1 {sz1:#x}");
        }
    }
}
