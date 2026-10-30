use super::*;

/// A one-quad glTF binary: two triangles in the `y = 0` plane, a metre on a
/// side, textured with a 4 x 4 PNG whose left half is red and right half
/// clear. Built here so the test carries no asset file.
pub(crate) fn quad_glb(nodes_scale: f32) -> Vec<u8> {
    let mut rgba = Vec::new();
    for _y in 0..4 {
        for x in 0..4 {
            rgba.extend_from_slice(if x < 2 {
                &[255, 0, 0, 255]
            } else {
                &[0, 0, 0, 0]
            });
        }
    }
    let png = legaia_tim::multi_palette::rgba_png(4, 4, &rgba).unwrap();
    let mut bin: Vec<u8> = Vec::new();
    let pos: [[f32; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
    ];
    for p in pos {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let uv: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
    for t in uv {
        for c in t {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    for i in [0u16, 1, 2, 1, 3, 2] {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    let png_at = bin.len();
    bin.extend_from_slice(&png);
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
"nodes":[{{"mesh":0,"scale":[{s},{s},{s}]}}],
"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"TEXCOORD_0":1}},"indices":2,"material":0}}]}}],
"materials":[{{"pbrMetallicRoughness":{{"baseColorTexture":{{"index":0}}}}}}],
"textures":[{{"source":0}}],"images":[{{"bufferView":3,"mimeType":"image/png"}}],
"accessors":[
 {{"bufferView":0,"componentType":5126,"count":4,"type":"VEC3"}},
 {{"bufferView":1,"componentType":5126,"count":4,"type":"VEC2"}},
 {{"bufferView":2,"componentType":5123,"count":6,"type":"SCALAR"}}],
"bufferViews":[
 {{"buffer":0,"byteOffset":0,"byteLength":48}},
 {{"buffer":0,"byteOffset":48,"byteLength":32}},
 {{"buffer":0,"byteOffset":80,"byteLength":12}},
 {{"buffer":0,"byteOffset":{png_at},"byteLength":{png_len}}}],
"buffers":[{{"byteLength":{bin_len}}}]}}"#,
        s = nodes_scale,
        png_len = png.len(),
        bin_len = bin.len()
    );
    let mut json = json.into_bytes();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut out = Vec::new();
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004E_4942u32.to_le_bytes());
    out.extend_from_slice(&bin);
    out
}

#[test]
fn a_textured_quad_converts_to_a_palettised_tmd() {
    let mut assets = BTreeMap::new();
    assets.insert("models/quad.glb".to_string(), quad_glb(2.0));
    let mut alloc = VramAlloc::empty();
    let mut patches = Vec::new();
    let raw = load_model(&assets, "models/quad.glb", 16, &mut alloc, &mut patches).unwrap();
    let tmd = legaia_tmd::parse(&raw).unwrap();
    let o = &tmd.objects[0];
    assert_eq!(o.vertices.len(), 4);
    // Two metres at 100 units a metre; `y` stays 0 (flipped to Y down).
    let xs: Vec<i16> = o.vertices.iter().map(|v| v.x).collect();
    assert!(xs.contains(&200) && xs.contains(&0));
    let g = legaia_tmd::legaia_prims::iter_groups(
        &raw,
        o.primitives_byte_offset,
        o.primitives_byte_size,
    )
    .unwrap();
    assert_eq!(g[0].prims.len(), 2);
    // A texel rectangle and a 16-entry palette went into VRAM.
    assert_eq!(patches.len(), 2);
    let pal = &patches[1];
    assert_eq!(pal.w, 16);
    assert_eq!(pal.words[0], 0x0000, "transparent pixels take index 0");
    assert!(pal.words.contains(&0x001F), "red survives quantisation");
    // The prim samples the placed palette and a 4bpp page.
    let p = &g[0].prims[0];
    assert_eq!(p.cba, cba_word(pal.x, pal.y));
    assert_eq!((p.tsb >> 7) & 3, 0, "4bpp");
}

#[test]
fn a_flipped_frame_keeps_the_winding() {
    let mut assets = BTreeMap::new();
    assets.insert("q.glb".to_string(), quad_glb(1.0));
    let mut alloc = VramAlloc::empty();
    let raw = load_model(&assets, "q.glb", 256, &mut alloc, &mut Vec::new()).unwrap();
    let tmd = legaia_tmd::parse(&raw).unwrap();
    let o = &tmd.objects[0];
    let g = legaia_tmd::legaia_prims::iter_groups(
        &raw,
        o.primitives_byte_offset,
        o.primitives_byte_size,
    )
    .unwrap();
    // First triangle (0, 1, 2) goes in as (0, 2, 1).
    let v = g[0].prims[0].vertex_indices();
    let at = |i: u16| o.vertices[usize::from(i)];
    assert_eq!((at(v[0]).x, at(v[0]).z), (0, 0));
    assert_eq!((at(v[1]).x, at(v[1]).z), (0, 100));
    assert_eq!((at(v[2]).x, at(v[2]).z), (100, 0));
}

#[test]
fn quantise_keeps_few_colours_exact() {
    let rgba = [
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255,
    ];
    let (pal, idx) = quantise(&rgba, 16);
    assert_eq!(pal.len(), 16);
    let word = |i: usize| pal[usize::from(idx[i])];
    assert_eq!(word(0), 0x001F);
    assert_eq!(word(1), 0x03E0);
    assert_eq!(word(2), 0x7C00);
    assert_eq!(idx[2], idx[3]);
}

#[test]
fn base64_decodes() {
    assert_eq!(decode_base64("aGVsbG8=").unwrap(), b"hello");
}

#[test]
fn junk_is_refused() {
    let mut assets = BTreeMap::new();
    assets.insert("x.glb".to_string(), b"glTF\x01\0\0\0".to_vec());
    let mut alloc = VramAlloc::empty();
    assert!(load_model(&assets, "x.glb", 16, &mut alloc, &mut Vec::new()).is_err());
    assert!(load_model(&assets, "missing.glb", 16, &mut alloc, &mut Vec::new()).is_err());
}
