//! SSAO lockstep: the helpers of the native passes
//! (`renderer::ssao::SSAO_PASS_WGSL` - `spin`, `sample_offset`,
//! `radius_px`, `occlusion_term`, `ao_from_sum`, `blur_weight` and the
//! contact-shadow `contact_hit` / `contact_fade` / `contact_from_hit`) and
//! their transcriptions in the play page's WebGL2 twin
//! (`site/js/webgl-ssao.js`, `FS_AO`), each evaluated through `shader_eval`
//! against the CPU mirror in `legaia_render_kernels::ssao`. A drift in any
//! of the three texts fails here.

use super::page_glsl::{es300_to_450, js_consts, parse_glsl, parse_wgsl, site_js, template};
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
    let [p0, p1, p2, p3] = ssao::shader_words(&p);
    let k = [0.0021, 0.0027, 0.166];
    let viewport = [16.0, 8.0, 640.0, 480.0];
    m.set_uniform("view_k", Val::f(&[k[0], k[1], k[2], 0.0]));
    m.set_uniform("viewport", Val::f(&viewport));
    m.set_uniform("p0", Val::f(&p0));
    m.set_uniform("p1", Val::f(&p1));
    m.set_uniform("p2", Val::f(&p2));
    m.set_uniform("p3", Val::f(&p3));

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

/// The contact-shadow helpers of the native pass against the kernel.
#[test]
fn the_wgsl_contact_helpers_match_the_kernel() {
    let src = crate::renderer::ssao::pass_source();
    let module = parse_wgsl(&src);
    let mut m = Machine::new(&module, Box::new(|_, _, _| [0; 4]));
    let p = SsaoParams::default();
    let [_, _, _, p3] = ssao::shader_words(&p);
    m.set_uniform("p3", Val::f(&p3));
    contact_cases(&mut m, &p);
}

/// Shared by both hosts' contact checks: `contact_hit`, `contact_fade`,
/// `contact_from_hit` over a spread of inputs.
fn contact_cases(m: &mut Machine<'_>, p: &SsaoParams) {
    for (ray, scene) in [
        (500.0, 520.0),
        (520.0, 500.0),
        (500.0, 499.0),
        (700.0, 600.0),
        (3000.0, 2990.0),
    ] {
        let f = m.function("contact_hit");
        let got = scalar(m.call(f, vec![Val::f(&[ray]), Val::f(&[scene])]).unwrap());
        close(got, ssao::contact_hit(p, ray, scene), "contact_hit");
    }
    for t in [0.0, 10.0, 40.0, 72.0, 90.0] {
        let f = m.function("contact_fade");
        let got = scalar(m.call(f, vec![Val::f(&[t])]).unwrap());
        close(got, ssao::contact_fade(p, t), "contact_fade");
    }
    for (hit, ndl, st) in [
        (1.0, 1.0, 0.5),
        (0.5, 0.1, 0.4),
        (1.0, -0.2, 0.5),
        (0.0, 1.0, 0.5),
    ] {
        let f = m.function("contact_from_hit");
        let got = scalar(
            m.call(f, vec![Val::f(&[hit]), Val::f(&[ndl]), Val::f(&[st])])
                .unwrap(),
        );
        close(
            got,
            ssao::contact_from_hit(hit, ndl, st),
            "contact_from_hit",
        );
    }
}

/// The play page's GLSL helpers (`SSAO_COMMON_GLSL` inside `FS_AO`)
/// against the kernel: the same checks as the native helpers above, so the
/// two hosts shade from one law.
#[test]
fn the_page_glsl_helpers_match_the_kernel() {
    let consts = js_consts(&site_js("webgl-ssao.js"));
    let src = es300_to_450(&template(&consts, "FS_AO"));
    let module = parse_glsl(&src, naga::ShaderStage::Fragment);
    let mut m = Machine::new(&module, Box::new(|_, _, _| [0; 4]));
    let p = SsaoParams::default();
    let [p0, p1, p2, p3] = ssao::shader_words(&p);
    let k = [0.0021, 0.0027, 0.166];
    let viewport = [16.0, 8.0, 640.0, 480.0];
    m.set_uniform("u_view_k", Val::f(&[k[0], k[1], k[2], 0.0]));
    m.set_uniform("u_viewport", Val::f(&viewport));
    m.set_uniform("u_p0", Val::f(&p0));
    m.set_uniform("u_p1", Val::f(&p1));
    m.set_uniform("u_p2", Val::f(&p2));
    m.set_uniform("u_p3", Val::f(&p3));

    for px in [[0.0, 0.0], [17.0, 3.0], [319.0, 239.0], [1919.0, 1079.0]] {
        let f = m.function("spin");
        let got = scalar(m.call(f, vec![Val::f(&px)]).unwrap());
        close(got, ssao::spin(px), "page spin");
    }
    for i in 0..p.samples {
        let f = m.function("sample_offset");
        let got = m
            .call(
                f,
                vec![
                    Val::I(vec![i as i32]),
                    Val::I(vec![p.samples as i32]),
                    Val::f(&[0.37]),
                ],
            )
            .unwrap()
            .unwrap();
        let want = ssao::sample_offset(i, p.samples, 0.37);
        close(got.floats()[0], want[0], "page sample_offset.x");
        close(got.floats()[1], want[1], "page sample_offset.y");
    }
    for z in [50.0, 400.0, 3000.0, 40_000.0] {
        let f = m.function("radius_px");
        let got = scalar(m.call(f, vec![Val::f(&[z])]).unwrap());
        close(
            got,
            ssao::radius_px(&p, z, k, viewport[3]),
            "page radius_px",
        );
    }
    let n = [0.0, 0.8, 0.6];
    for v in [[10.0, 20.0, 5.0], [-30.0, 4.0, 1.0], [0.0, 70.0, 0.0]] {
        let f = m.function("occlusion_term");
        let got = scalar(
            m.call(f, vec![Val::f(&v), Val::f(&n), Val::f(&[800.0])])
                .unwrap(),
        );
        close(
            got,
            ssao::occlusion_term(&p, v, n, 800.0),
            "page occlusion_term",
        );
    }
    for sum in [0.0, 1.5, 4.0, 30.0] {
        let f = m.function("ao_from_sum");
        let got = scalar(
            m.call(f, vec![Val::f(&[sum]), Val::I(vec![p.samples as i32])])
                .unwrap(),
        );
        close(
            got,
            ssao::ao_from_sum(&p, sum, p.samples),
            "page ao_from_sum",
        );
    }
    for (t, z, nn) in [(1.0, 801.0, [0.0, 0.8, 0.6]), (3.0, 830.0, [0.1, 0.7, 0.7])] {
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
            "page blur_weight",
        );
    }
    // The page's view-position reconstruction inverts through `view_to_px`
    // (window y up), and agrees with the kernel's `view_to_ndc`.
    let q = ssao::view_position([0.25, -0.4], 900.0, k);
    let f = m.function("view_to_px");
    let px = m.call(f, vec![Val::f(&q)]).unwrap().unwrap();
    let ndc = ssao::view_to_ndc(q, k);
    close(
        px.floats()[0],
        viewport[0] + (ndc[0] * 0.5 + 0.5) * viewport[2],
        "page view_to_px.x",
    );
    close(
        px.floats()[1],
        viewport[1] + (ndc[1] * 0.5 + 0.5) * viewport[3],
        "page view_to_px.y",
    );
    contact_cases(&mut m, &p);
}
