//! The browser play page's GLSL, materialised for naga: read a shader's
//! JavaScript template out of `site/js`, resolve its `${...}` interpolations
//! against the file's own constants, and re-declare it as the
//! Vulkan-flavoured GLSL 4.50 naga's front end reads - without touching a
//! line of the shader's arithmetic. Shared by the parity tests that evaluate
//! the page's shaders against the native WGSL (`shader_eval`).

use std::collections::HashMap;

/// A `site/js` file's text.
pub(super) fn site_js(file: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../site/js")
        .join(file);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// The JS constants a shader template interpolates: numbers, number arrays
/// and template strings, keyed by name.
#[derive(Debug, Clone)]
pub(super) enum JsConst {
    Num(f64),
    Arr(Vec<f64>),
    Template(String),
}

/// Every top-level-looking `const NAME = <number | [numbers] | `template`>`
/// of a JS file, at any indentation.
pub(super) fn js_consts(js: &str) -> HashMap<String, JsConst> {
    let mut out = HashMap::new();
    let mut rest = js;
    while let Some(i) = rest.find("const ") {
        let at_line_start = rest[..i]
            .rsplit('\n')
            .next()
            .is_some_and(|pre| pre.trim().is_empty());
        rest = &rest[i + 6..];
        if !at_line_start {
            continue;
        }
        let Some(eq) = rest.find(" = ") else { break };
        let name = rest[..eq].trim().to_string();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let val = &rest[eq + 3..];
        if let Some(body) = val.strip_prefix('`') {
            let end = body.find('`').expect("template end");
            out.insert(name, JsConst::Template(body[..end].to_string()));
        } else if let Some(body) = val.strip_prefix('[') {
            let end = body.find(']').expect("array end");
            let nums: Option<Vec<f64>> = body[..end]
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.parse::<f64>().ok())
                .collect();
            if let Some(nums) = nums {
                out.insert(name, JsConst::Arr(nums));
            }
        } else {
            let end = val.find(';').unwrap_or(val.len());
            if let Ok(v) = val[..end].trim().parse::<f64>() {
                out.insert(name, JsConst::Num(v));
            }
        }
    }
    out
}

/// `glslFloat` from webgl-shaders.js: integers print with `.0`.
fn glsl_float(x: f64) -> String {
    if x.fract() == 0.0 {
        format!("{x:.1}")
    } else {
        format!("{x}")
    }
}

/// Resolve every `${...}` in a template against `consts`. Only the
/// interpolation forms the shaders use are understood; anything else fails,
/// so a new form needs this resolver taught it.
pub(super) fn resolve_template(t: &str, consts: &HashMap<String, JsConst>) -> String {
    let mut out = String::new();
    let mut rest = t;
    while let Some(i) = rest.find("${") {
        out.push_str(&rest[..i]);
        let end = rest[i..].find('}').expect("interpolation end") + i;
        let expr = rest[i + 2..end].trim();
        let num = |n: &str| match consts.get(n) {
            Some(JsConst::Num(v)) => *v,
            other => panic!("`{n}` is not a numeric constant: {other:?}"),
        };
        let arr = |n: &str| match consts.get(n) {
            Some(JsConst::Arr(v)) => v.clone(),
            other => panic!("`{n}` is not an array constant: {other:?}"),
        };
        let text = if let Some(n) = expr.strip_suffix(".map(glslFloat).join(', ')") {
            arr(n)
                .into_iter()
                .map(glsl_float)
                .collect::<Vec<_>>()
                .join(", ")
        } else if let Some(inner) = expr
            .strip_prefix("glslFloat(")
            .and_then(|s| s.strip_suffix(')'))
        {
            if let Some((n, idx)) = inner.split_once('[') {
                let idx: usize = idx.trim_end_matches(']').parse().unwrap();
                glsl_float(arr(n)[idx])
            } else {
                glsl_float(num(inner))
            }
        } else {
            match consts.get(expr) {
                Some(JsConst::Template(s)) => resolve_template(s, consts),
                other => panic!("unsupported interpolation `{expr}`: {other:?}"),
            }
        };
        out.push_str(&text);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// A template constant of `consts`, resolved.
pub(super) fn template(consts: &HashMap<String, JsConst>, name: &str) -> String {
    match consts.get(name) {
        Some(JsConst::Template(s)) => resolve_template(s, consts),
        _ => panic!("template `{name}` not found"),
    }
}

/// Rewrite a GLSL ES 3.00 shader into the Vulkan-flavoured GLSL 4.50 naga
/// reads: the version line and precision statements dropped, loose uniforms
/// gathered into one block, each combined image-sampler split into a texture
/// and a sampler (recombined at its call sites, which naga requires), and
/// stage inputs and outputs given explicit locations. No other line changes.
pub(super) fn es300_to_450(src: &str) -> String {
    let mut block = Vec::new();
    let mut body = Vec::new();
    let (mut loc_in, mut loc_out, mut binding) = (0u32, 0u32, 0u32);
    let mut combined: Vec<(String, &'static str)> = Vec::new();
    for line in src.lines() {
        let t = line.trim_start();
        if t.starts_with("#version") || t.starts_with("precision ") {
            continue;
        }
        if let Some(decl) = t.strip_prefix("uniform ") {
            let decl = decl.trim_start_matches("highp ");
            if decl.contains("sampler") {
                let mut it = decl.trim_end_matches(';').split_whitespace();
                let (ty, name) = (it.next().unwrap(), it.next().unwrap());
                let (tex, samp, comb) = match ty {
                    "usampler2D" => ("utexture2D", "sampler", "usampler2D"),
                    "sampler2D" => ("texture2D", "sampler", "sampler2D"),
                    "sampler2DArrayShadow" => {
                        ("texture2DArray", "samplerShadow", "sampler2DArrayShadow")
                    }
                    other => panic!("unmapped opaque uniform type {other}"),
                };
                body.push(format!(
                    "layout(set = 1, binding = {binding}) uniform {tex} {name};"
                ));
                body.push(format!(
                    "layout(set = 1, binding = {}) uniform {samp} {name}_s;",
                    binding + 1
                ));
                binding += 2;
                combined.push((name.to_string(), comb));
            } else {
                block.push(format!("    {decl}"));
            }
            continue;
        }
        if t.starts_with("layout(") {
            body.push(line.to_string());
            continue;
        }
        if t.starts_with("in ") || t.starts_with("flat in ") {
            body.push(format!("layout(location = {loc_in}) {t}"));
            loc_in += 1;
            continue;
        }
        if t.starts_with("out ") || t.starts_with("flat out ") {
            body.push(format!("layout(location = {loc_out}) {t}"));
            loc_out += 1;
            continue;
        }
        let mut l = line.to_string();
        for (name, comb) in &combined {
            l = l.replace(&format!("({name},"), &format!("({comb}({name}, {name}_s),"));
        }
        body.push(l);
    }
    format!(
        "#version 450 core\nlayout(set = 0, binding = 0, std140) uniform PageUniforms {{\n{}\n}};\n{}\n",
        block.join("\n"),
        body.join("\n")
    )
}

pub(super) fn parse_glsl(src: &str, stage: naga::ShaderStage) -> naga::Module {
    let mut fe = naga::front::glsl::Frontend::default();
    let opts = naga::front::glsl::Options::from(stage);
    fe.parse(&opts, src)
        .unwrap_or_else(|e| panic!("page GLSL does not parse:\n{}", e.emit_to_string(src)))
}

pub(super) fn parse_wgsl(src: &str) -> naga::Module {
    naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)))
}
