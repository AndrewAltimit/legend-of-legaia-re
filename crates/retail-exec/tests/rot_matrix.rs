//! The GTE rotation builders off the disc: Legaia's in-place axis factors
//! `FUN_800461A4` (X), `FUN_8004629C` (Y) and `FUN_8004638C` (Z), which
//! post-multiply the GTE rotation by a factor read from the sine LUT at
//! `0x80070A2C` through three MVMVAs; `MulMatrix` `FUN_8005B3A8`
//! (`a1 = a0 * a1`, three MVMVAs); and the camera-relative rotation
//! `FUN_8001CF50` that chains them under a node's `+0x52` skip bits. Held
//! against `GteMat3::rot_x_psx` / `rot_y_psx` / `rot_z_psx` (the rounded
//! radian builders are counted, not held), `billboard::rot_z_psx`, `GteMat3::mul`,
//! `field_light::mul_matrix0` and `camera_view_rotation`.

use crate::common::{Rng, scus_cpu};
use legaia_render_kernels::billboard::rot_z_psx;
use legaia_render_kernels::field_light::mul_matrix0;
use legaia_render_kernels::gte::{GteMat3, camera_view_rotation};
use psx_retail_exec::{Cpu, Stop};

const ROT_X: u32 = 0x8004_61A4;
const ROT_Y: u32 = 0x8004_629C;
const ROT_Z: u32 = 0x8004_638C;
const MUL_MATRIX: u32 = 0x8005_B3A8;
const VIEW_ROT: u32 = 0x8001_CF50;
const PITCH: u32 = 0x8007_B790;

fn set_rt(cpu: &mut Cpu, m: &[[i16; 3]; 3]) {
    let p = |a: i16, b: i16| (a as u16 as u32) | (b as u16 as u32) << 16;
    cpu.gte.write_ctrl(0, p(m[0][0], m[0][1]));
    cpu.gte.write_ctrl(1, p(m[0][2], m[1][0]));
    cpu.gte.write_ctrl(2, p(m[1][1], m[1][2]));
    cpu.gte.write_ctrl(3, p(m[2][0], m[2][1]));
    cpu.gte.write_ctrl(4, m[2][2] as u16 as u32);
}

fn rt(cpu: &Cpu) -> [[i16; 3]; 3] {
    let c = |r: usize| cpu.gte.read_ctrl(r);
    let (lo, hi) = (|w: u32| w as i16, |w: u32| (w >> 16) as i16);
    [
        [lo(c(0)), hi(c(0)), lo(c(1))],
        [hi(c(1)), lo(c(2)), hi(c(2))],
        [lo(c(3)), hi(c(3)), lo(c(4))],
    ]
}

fn write_matrix(cpu: &mut Cpu, va: u32, m: &[[i16; 3]; 3]) {
    for (k, v) in m.iter().flatten().enumerate() {
        cpu.mem.wr16(va + 2 * k as u32, *v as u16);
    }
}

fn read_matrix(cpu: &mut Cpu, va: u32) -> [[i16; 3]; 3] {
    let mut m = [[0i16; 3]; 3];
    for (k, v) in m.iter_mut().flatten().enumerate() {
        *v = cpu.mem.rd16(va + 2 * k as u32) as i16;
    }
    m
}

/// How the hosts hand the camera's integer angles to the port.
fn rad(a: i16) -> f32 {
    f32::from(a) / 4096.0 * std::f32::consts::TAU
}

fn random_rotationish(rng: &mut Rng) -> [[i16; 3]; 3] {
    std::array::from_fn(|_| std::array::from_fn(|_| (rng.below(0x2001) as i32 - 0x1000) as i16))
}

/// The three axis factors over every angle, from identity and from a random
/// rotation, with TR left as it was.
#[test]
fn axis_factors_match_the_lut_port() {
    differential!(scus(ROT_X), scus(ROT_Y), scus(ROT_Z));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut rng = Rng(0x524F_544D);
    let mut radian_off = [0u32; 3];
    let mut bad = Vec::new();
    for (axis, entry) in [ROT_X, ROT_Y, ROT_Z].into_iter().enumerate() {
        for angle in 0..0x1000u16 {
            let tr = [rng.next(), rng.next(), rng.next()];
            for (i, t) in tr.iter().enumerate() {
                cpu.gte.write_ctrl(5 + i, *t);
            }
            set_rt(&mut cpu, &GteMat3::IDENTITY.m);
            assert_eq!(cpu.call(entry, &[u32::from(angle)]), Stop::Returned);
            assert!(cpu.gte_ops == 3, "three MVMVAs");
            let factor = rt(&cpu);
            assert_eq!((5..8).map(|r| cpu.gte.read_ctrl(r)).collect::<Vec<_>>(), tr);

            // The ports of the three factors, and the billboard's Z.
            let lut = match axis {
                0 => GteMat3::rot_x_psx(angle).m,
                1 => GteMat3::rot_y_psx(angle).m,
                _ => GteMat3::rot_z_psx(angle).m,
            };
            if axis == 2 {
                assert_eq!(rot_z_psx(angle).m, lut);
            }
            if factor != lut && bad.len() < 8 {
                bad.push(format!(
                    "axis {axis} angle {angle:#x}: retail {factor:?} lut {lut:?}"
                ));
            }
            let r = rad(angle as i16);
            let radian = match axis {
                0 => GteMat3::rot_x(r),
                1 => GteMat3::rot_y(r),
                _ => GteMat3::rot_z(r),
            };
            if radian.m != factor {
                radian_off[axis] += 1;
            }

            // Post-multiplication onto a live rotation.
            let base = random_rotationish(&mut rng);
            set_rt(&mut cpu, &base);
            cpu.call(entry, &[u32::from(angle)]);
            let want = GteMat3 { m: base }.mul(&GteMat3 { m: lut });
            if rt(&cpu) != want.m && bad.len() < 8 {
                bad.push(format!(
                    "axis {axis} angle {angle:#x} on {base:?}: retail {:?} port {:?}",
                    rt(&cpu),
                    want.m
                ));
            }
        }
    }
    eprintln!("radian builders off the LUT factor at {radian_off:?} of 4096 angles (X, Y, Z)");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// `MulMatrix`: `a1 = a0 * a1`, saturating each element like an `IR`.
#[test]
fn mul_matrix_matches_both_ports() {
    differential!(scus(MUL_MATRIX));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut rng = Rng(0x4D55_4C30);
    let (a_va, b_va) = (0x8010_0000, 0x8010_0040);
    for i in 0..4000 {
        let wide = i % 4 == 0;
        let make = |rng: &mut Rng| -> [[i16; 3]; 3] {
            if wide {
                std::array::from_fn(|_| std::array::from_fn(|_| rng.next() as i16))
            } else {
                random_rotationish(rng)
            }
        };
        let (a, b) = (make(&mut rng), make(&mut rng));
        write_matrix(&mut cpu, a_va, &a);
        write_matrix(&mut cpu, b_va, &b);
        assert_eq!(cpu.call(MUL_MATRIX, &[a_va, b_va]), Stop::Returned);
        let got = read_matrix(&mut cpu, b_va);
        assert_eq!(cpu.regs.r[psx_retail_exec::V0], b_va);
        let wide_a = a.map(|r| r.map(i32::from));
        let wide_b = b.map(|r| r.map(i32::from));
        let field = mul_matrix0(&wide_a, &wide_b).map(|r| r.map(|v| v as i16));
        assert_eq!(got, field, "field_light::mul_matrix0 on {a:?} * {b:?}");
        assert_eq!(
            got,
            GteMat3 { m: a }.mul(&GteMat3 { m: b }).m,
            "GteMat3::mul"
        );
        // The routine's last store is a whole `swc2` of IR3: the pad
        // halfword after m22 takes its sign.
        assert_eq!(
            cpu.mem.rd16(b_va + 0x12),
            if got[2][2] < 0 { 0xFFFF } else { 0 }
        );
    }
}

/// `FUN_8001CF50` over every skip-bit combination and random camera angles:
/// the rotation it leaves in its `a2` matrix against `camera_view_rotation`,
/// fed the radians the hosts feed it.
#[test]
fn camera_relative_rotation_matches_the_port() {
    differential!(scus(VIEW_ROT));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut rng = Rng(0x5649_4557);
    let (node, mat, out) = (0x8010_0000u32, 0x8010_0100u32, 0x8010_0200u32);
    let mut off = 0;
    let mut worst = 0i32;
    let mut example = None;
    let mut cases = 0;
    for i in 0..3000 {
        let flags = [0u16, 0x80, 0x100, 0x180, 0x200, 0x280, 0x300, 0x380][i % 8];
        let ang: [i16; 3] = std::array::from_fn(|_| rng.next() as i16);
        for (k, a) in ang.iter().enumerate() {
            cpu.mem.wr16(PITCH + 2 * k as u32, *a as u16);
        }
        cpu.mem.wr16(node + 0x52, flags);
        write_matrix(&mut cpu, mat, &GteMat3::IDENTITY.m);
        assert_eq!(cpu.call(VIEW_ROT, &[node, mat, out]), Stop::Returned);
        let retail = read_matrix(&mut cpu, out);
        // The node's own matrix comes back as the rotation times the 6x scale.
        let scaled = read_matrix(&mut cpu, mat);
        assert_eq!(scaled, retail.map(|r| r.map(|v| v.wrapping_mul(6))));

        // The LUT-exact composition retail runs.
        let mut lut = GteMat3::IDENTITY;
        let a = ang.map(|v| v as u16);
        let (cx, sx) = (
            legaia_render_kernels::gte::psx_cos(a[0]) as i16,
            legaia_render_kernels::gte::psx_sin(a[0]) as i16,
        );
        let (cy, sy) = (
            legaia_render_kernels::gte::psx_cos(a[1]) as i16,
            legaia_render_kernels::gte::psx_sin(a[1]) as i16,
        );
        if flags & 0x80 == 0 {
            lut = lut.mul(&GteMat3 {
                m: [[0x1000, 0, 0], [0, cx, -sx], [0, sx, cx]],
            });
        }
        if flags & 0x100 == 0 {
            lut = lut.mul(&GteMat3 {
                m: [[cy, 0, sy], [0, 0x1000, 0], [-sy, 0, cy]],
            });
        }
        if flags & 0x200 == 0 {
            lut = lut.mul(&rot_z_psx(a[2]));
        }
        assert_eq!(
            retail, lut.m,
            "LUT composition, flags {flags:#x} angles {ang:?}"
        );

        let port = camera_view_rotation(flags, rad(ang[0]), rad(ang[1]), rad(ang[2]))
            .expect("no 0x400 bit");
        cases += 1;
        if port.m != retail {
            off += 1;
            let d = (0..9)
                .map(|k| (i32::from(port.m[k / 3][k % 3]) - i32::from(retail[k / 3][k % 3])).abs())
                .max()
                .unwrap();
            if d > worst {
                worst = d;
                example = Some(format!(
                    "flags {flags:#x} angles {ang:?}: retail {retail:?} port {:?}",
                    port.m
                ));
            }
        }
    }
    eprintln!(
        "camera_view_rotation off retail on {off}/{cases}, worst element {worst}: {example:?}"
    );
    assert_eq!(off, 0, "{example:?}");
}
