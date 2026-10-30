//! SSAO lockstep: the WGSL helpers of the native passes
//! (`renderer::ssao::SSAO_PASS_WGSL` - `spin`, `sample_offset`,
//! `radius_px`, `occlusion_term`, `ao_from_sum`, `blur_weight`) evaluated through `shader_eval` against
//! their CPU mirrors in `legaia_render_kernels::ssao`, which the play page's
//! WebGL2 twin transcribes. A drift in either text fails here.

use super::page_glsl::parse_wgsl;
use super::shader_eval::{Machine, Val};
use legaia_render_kernels::ssao::{self, SsaoParams};

fn close(a: f32, b: f32, what: &str) {
    assert!(
        (a - b).abs() <= 1e-4 * (1.0 + a.abs().max(b.abs())),
        "{what}: wgsl {a} vs kernel {b}"
    );
}

fn scalar(v: Option<Val>) -> f32 {
    v.expect("value").floats()[0]
}

#[test]
fn the_wgsl_helpers_match_the_kernel() {
    let src = crate::renderer::ssao::pass_source();
    let module = parse_wgsl(&src);
    let mut m = Machine::new(&module, Box::new(|_, _, _| [0; 4]));
    let p = SsaoParams::default();
    let [p0, p1, p2] = ssao::shader_words(&p);
    let k = [0.0021, 0.0027, 0.166];
    let viewport = [16.0, 8.0, 640.0, 480.0];
    m.set_uniform("view_k", Val::f(&[k[0], k[1], k[2], 0.0]));
    m.set_uniform("viewport", Val::f(&viewport));
    m.set_uniform("p0", Val::f(&p0));
    m.set_uniform("p1", Val::f(&p1));
    m.set_uniform("p2", Val::f(&p2));

    for px in [[0.0, 0.0], [17.0, 3.0], [319.0, 239.0], [1919.0, 1079.0]] {
        let f = m.function("spin");
        let got = scalar(m.call(f, vec![Val::f(&px)]).unwrap());
        close(got, ssao::spin(px), "spin");
    }
    for i in 0..p.samples {
        let f = m.function("sample_offset");
        let got = m
            .call(f, vec![Val::u(&[i]), Val::u(&[p.samples]), Val::f(&[0.37])])
            .unwrap()
            .unwrap();
        let want = ssao::sample_offset(i, p.samples, 0.37);
        close(got.floats()[0], want[0], "sample_offset.x");
        close(got.floats()[1], want[1], "sample_offset.y");
    }
    for z in [50.0, 400.0, 3000.0, 40_000.0] {
        let f = m.function("radius_px");
        let got = scalar(m.call(f, vec![Val::f(&[z])]).unwrap());
        close(got, ssao::radius_px(&p, z, k, viewport[3]), "radius_px");
    }
    let n = [0.0, 0.8, 0.6];
    for v in [
        [10.0, 20.0, 5.0],
        [-30.0, 4.0, 1.0],
        [0.0, 70.0, 0.0],
        [5.0, -20.0, 3.0],
    ] {
        let f = m.function("occlusion_term");
        let got = scalar(
            m.call(f, vec![Val::f(&v), Val::f(&n), Val::f(&[800.0])])
                .unwrap(),
        );
        close(got, ssao::occlusion_term(&p, v, n, 800.0), "occlusion_term");
    }
    for sum in [0.0, 1.5, 4.0, 30.0] {
        let f = m.function("ao_from_sum");
        let got = scalar(
            m.call(f, vec![Val::f(&[sum]), Val::u(&[p.samples])])
                .unwrap(),
        );
        close(got, ssao::ao_from_sum(&p, sum, p.samples), "ao_from_sum");
    }
    for (t, z, nn) in [
        (1.0, 801.0, [0.0, 0.8, 0.6]),
        (3.0, 830.0, [0.1, 0.7, 0.7]),
        (2.0, 0.0, [0.0, 0.8, 0.6]),
    ] {
        let f = m.function("blur_weight");
        let got = scalar(
            m.call(
                f,
                vec![
                    Val::f(&[t]),
                    Val::f(&[800.0]),
                    Val::f(&[z]),
                    Val::f(&n),
                    Val::f(&nn),
                ],
            )
            .unwrap(),
        );
        close(
            got,
            ssao::blur_weight(&p, t, 800.0, z, n, nn),
            "blur_weight",
        );
    }
}
