//! glTF 2.0 model assets as map props: a modder's own mesh, converted when
//! the map loads into the renderer's form - a Legaia TMD whose textured
//! triangles sample a palettised copy of the model's base-colour texture in
//! free VRAM.
//!
//! The conversion is the PSX look by construction: the texture is quantised
//! to a 16- or 256-colour palette and stored as a 4bpp or 8bpp page, the
//! triangles draw through the same VRAM path as every disc mesh, and PSX mode
//! snaps and dithers them like the rest of the scene. Clean mode draws the
//! same quantised texels without the snap. A model is never drawn in full
//! colour; that keeps it inside the scene's one shading law.
//!
//! What is read: the default scene's node tree (`matrix` or TRS), each
//! mesh's triangle primitives (`POSITION`, `TEXCOORD_0`, `indices`), and each
//! material's `baseColorTexture` (a PNG, embedded in a `.glb` buffer view, a
//! `data:` URI, or a file beside the model under `assets/`) or its
//! `baseColorFactor`. Skins, morph targets, animations, normals and other
//! texture channels are ignored. One glTF unit is [`UNITS_PER_METRE`] world
//! units, `+Y` up (the engine's field frame is `Y` down; the conversion
//! flips it).

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use legaia_tmd::descriptor::PacketShape;
use legaia_tmd::encode::{LEGAIA_OBJECT_SCALE, ModelGroup, ModelObject, ModelPrim, encode};
use serde_json::Value;

use super::detail::VramPatch;
use super::vram_alloc::{VramAlloc, cba_word};

/// World units per glTF unit (metre). A tile is `128` units, so a one-metre
/// cube is a little under a tile's width.
pub const UNITS_PER_METRE: f32 = 100.0;

/// The most triangles a model converts.
pub const MAX_TRIANGLES: usize = 16_384;

/// The largest texture side, in texels, a model keeps; a larger texture is
/// box-filtered down.
pub const MAX_TEXTURE_SIDE: usize = 256;

/// Vertices one TMD object holds (its `u16` byte-offset indices).
const OBJECT_VERTICES: usize = 8000;

type Mat4 = [[f32; 4]; 4];

const IDENTITY: Mat4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

fn mat_mul(a: &Mat4, b: &Mat4) -> Mat4 {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..4).map(|k| a[i][k] * b[k][j]).sum()))
}

fn transform(m: &Mat4, p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + m[i][3])
}

/// A node's local matrix (row-major) from `matrix` (column-major in the
/// file) or `translation` / `rotation` / `scale`.
fn node_matrix(node: &Value) -> Mat4 {
    let floats = |k: &str| -> Option<Vec<f32>> {
        node.get(k)?
            .as_array()?
            .iter()
            .map(|v| v.as_f64().map(|f| f as f32))
            .collect()
    };
    if let Some(m) = floats("matrix").filter(|m| m.len() == 16) {
        return std::array::from_fn(|i| std::array::from_fn(|j| m[j * 4 + i]));
    }
    let t = floats("translation").unwrap_or_else(|| vec![0.0; 3]);
    let r = floats("rotation").unwrap_or_else(|| vec![0.0, 0.0, 0.0, 1.0]);
    let s = floats("scale").unwrap_or_else(|| vec![1.0; 3]);
    let (x, y, z, w) = (r[0], r[1], r[2], r[3]);
    let rot = [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ];
    std::array::from_fn(|i| {
        if i == 3 {
            [0.0, 0.0, 0.0, 1.0]
        } else {
            [rot[i][0] * s[0], rot[i][1] * s[1], rot[i][2] * s[2], t[i]]
        }
    })
}

/// A parsed glTF document and its binary buffers.
struct Doc {
    json: Value,
    buffers: Vec<Vec<u8>>,
}

fn decode_base64(s: &str) -> Result<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        })
    };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = val(c).context("bad base64")?;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

/// The bytes a `uri` names: a `data:` URI, or a file relative to the model's
/// own directory under `assets/`.
fn uri_bytes(assets: &BTreeMap<String, Vec<u8>>, dir: &str, uri: &str) -> Result<Vec<u8>> {
    if let Some(rest) = uri.strip_prefix("data:") {
        let (_, data) = rest
            .split_once(";base64,")
            .context("a data URI must be base64")?;
        return decode_base64(data);
    }
    if uri.contains("..") || uri.starts_with('/') || uri.contains(':') {
        bail!("uri {uri:?} leaves the pack's assets");
    }
    let path = if dir.is_empty() {
        uri.to_string()
    } else {
        format!("{dir}/{uri}")
    };
    assets
        .get(&path)
        .cloned()
        .with_context(|| format!("the pack carries no `assets/{path}`"))
}

fn parse_doc(assets: &BTreeMap<String, Vec<u8>>, path: &str) -> Result<Doc> {
    let bytes = assets
        .get(path)
        .with_context(|| format!("the pack carries no `assets/{path}`"))?;
    let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
    let (json, bin) = if bytes.starts_with(b"glTF") {
        if bytes.len() < 12 || u32::from_le_bytes(bytes[4..8].try_into()?) != 2 {
            bail!("not a glTF 2.0 binary");
        }
        let mut at = 12;
        let (mut json, mut bin) = (None, None);
        while at + 8 <= bytes.len() {
            let len = u32::from_le_bytes(bytes[at..at + 4].try_into()?) as usize;
            let kind = u32::from_le_bytes(bytes[at + 4..at + 8].try_into()?);
            let body = bytes
                .get(at + 8..at + 8 + len)
                .context("a glb chunk runs past the file")?;
            match kind {
                0x4E4F_534A => json = Some(body),
                0x004E_4942 => bin = Some(body.to_vec()),
                _ => {}
            }
            at += 8 + len.div_ceil(4) * 4;
        }
        (json.context("a glb with no JSON chunk")?, bin)
    } else {
        (bytes.as_slice(), None)
    };
    let json: Value = serde_json::from_slice(json).context("glTF JSON")?;
    let mut buffers = Vec::new();
    for (i, b) in json["buffers"].as_array().into_iter().flatten().enumerate() {
        match b["uri"].as_str() {
            Some(uri) => buffers.push(uri_bytes(assets, dir, uri)?),
            None if i == 0 => buffers.push(bin.clone().context("buffer 0 has no glb BIN chunk")?),
            None => bail!("buffer {i} has no uri"),
        }
    }
    Ok(Doc { json, buffers })
}

impl Doc {
    /// The bytes of buffer view `i`, with its stride.
    fn view(&self, i: usize) -> Result<(&[u8], usize)> {
        let v = &self.json["bufferViews"][i];
        let buf = self
            .buffers
            .get(v["buffer"].as_u64().unwrap_or(0) as usize)
            .context("buffer view names no buffer")?;
        let off = v["byteOffset"].as_u64().unwrap_or(0) as usize;
        let len = v["byteLength"].as_u64().context("buffer view length")? as usize;
        let stride = v["byteStride"].as_u64().unwrap_or(0) as usize;
        Ok((
            buf.get(off..off + len)
                .context("buffer view past its buffer")?,
            stride,
        ))
    }

    /// Accessor `i` as `count` rows of `n` floats. Integer components are
    /// read as normalised when the accessor says so.
    fn floats(&self, i: usize, n: usize) -> Result<Vec<Vec<f32>>> {
        let a = &self.json["accessors"][i];
        let count = a["count"].as_u64().context("accessor count")? as usize;
        let comp = a["componentType"]
            .as_u64()
            .context("accessor component type")?;
        let norm = a["normalized"].as_bool().unwrap_or(false);
        let (size, read): (usize, fn(&[u8]) -> f32) = match comp {
            5126 => (4, |b| f32::from_le_bytes(b[..4].try_into().unwrap())),
            5121 => (1, |b| f32::from(b[0])),
            5123 => (2, |b| f32::from(u16::from_le_bytes([b[0], b[1]]))),
            _ => bail!("accessor component type {comp} is not read"),
        };
        let scale = match (comp, norm) {
            (5121, true) => 1.0 / 255.0,
            (5123, true) => 1.0 / 65535.0,
            _ => 1.0,
        };
        let view = a["bufferView"]
            .as_u64()
            .context("a sparse or empty accessor")? as usize;
        let (bytes, stride) = self.view(view)?;
        let base = a["byteOffset"].as_u64().unwrap_or(0) as usize;
        let stride = if stride == 0 { size * n } else { stride };
        (0..count)
            .map(|k| {
                let at = base + k * stride;
                let row = bytes
                    .get(at..at + size * n)
                    .context("accessor runs past its view")?;
                Ok((0..n).map(|c| read(&row[c * size..]) * scale).collect())
            })
            .collect()
    }

    /// Index accessor `i`.
    fn indices(&self, i: usize) -> Result<Vec<u32>> {
        let a = &self.json["accessors"][i];
        let count = a["count"].as_u64().context("index count")? as usize;
        let comp = a["componentType"].as_u64().context("index type")?;
        let size = match comp {
            5121 => 1,
            5123 => 2,
            5125 => 4,
            _ => bail!("index component type {comp}"),
        };
        let (bytes, _) = self.view(a["bufferView"].as_u64().context("index view")? as usize)?;
        let base = a["byteOffset"].as_u64().unwrap_or(0) as usize;
        (0..count)
            .map(|k| {
                let b = bytes
                    .get(base + k * size..base + (k + 1) * size)
                    .context("indices run past their view")?;
                Ok(match size {
                    1 => u32::from(b[0]),
                    2 => u32::from(u16::from_le_bytes([b[0], b[1]])),
                    _ => u32::from_le_bytes(b.try_into()?),
                })
            })
            .collect()
    }

    /// The PNG bytes of image `i`.
    fn image(&self, assets: &BTreeMap<String, Vec<u8>>, dir: &str, i: usize) -> Result<Vec<u8>> {
        let img = &self.json["images"][i];
        if let Some(v) = img["bufferView"].as_u64() {
            return Ok(self.view(v as usize)?.0.to_vec());
        }
        uri_bytes(
            assets,
            dir,
            img["uri"]
                .as_str()
                .context("an image with no uri or view")?,
        )
    }
}

/// One converted triangle: retail-frame positions, texel UVs, and which
/// texture (or flat colour) it draws with.
struct Tri {
    pos: [[f32; 3]; 3],
    uv: [[f32; 2]; 3],
    material: Option<usize>,
}

/// A texture in VRAM: where its texels and palette went.
#[derive(Clone, Copy)]
struct Placed {
    tsb: u16,
    cba: u16,
    u0: u8,
    v0: u8,
    w: usize,
    h: usize,
}

/// Convert model asset `path` into a Legaia TMD, its texture quantised to
/// `colors` (16 or 256) and written through `patches` into VRAM `alloc`
/// hands out.
pub fn load_model(
    assets: &BTreeMap<String, Vec<u8>>,
    path: &str,
    colors: u16,
    alloc: &mut VramAlloc,
    patches: &mut Vec<VramPatch>,
) -> Result<Vec<u8>> {
    if colors != 16 && colors != 256 {
        bail!("colors {colors}: a model texture is 16 or 256 colours");
    }
    let doc = parse_doc(assets, path)?;
    let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
    let j = &doc.json;
    // The node tree's mesh instances.
    let mut instances: Vec<(usize, Mat4)> = Vec::new();
    fn walk(j: &Value, node: usize, parent: &Mat4, out: &mut Vec<(usize, Mat4)>, depth: u32) {
        if depth > 64 {
            return;
        }
        let n = &j["nodes"][node];
        let m = mat_mul(parent, &node_matrix(n));
        if let Some(mesh) = n["mesh"].as_u64() {
            out.push((mesh as usize, m));
        }
        for c in n["children"].as_array().into_iter().flatten() {
            if let Some(c) = c.as_u64() {
                walk(j, c as usize, &m, out, depth + 1);
            }
        }
    }
    let scene = j["scene"].as_u64().unwrap_or(0) as usize;
    match j["scenes"][scene]["nodes"].as_array() {
        Some(roots) => {
            for r in roots.iter().filter_map(Value::as_u64) {
                walk(j, r as usize, &IDENTITY, &mut instances, 0);
            }
        }
        None => {
            let n = j["meshes"].as_array().map_or(0, Vec::len);
            instances.extend((0..n).map(|m| (m, IDENTITY)));
        }
    }
    // Triangles, in the retail frame.
    let mut tris: Vec<Tri> = Vec::new();
    for (mesh, m) in &instances {
        for prim in j["meshes"][*mesh]["primitives"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if prim["mode"].as_u64().unwrap_or(4) != 4 {
                continue;
            }
            let attrs = &prim["attributes"];
            let pos = doc.floats(
                attrs["POSITION"]
                    .as_u64()
                    .context("a primitive with no POSITION")? as usize,
                3,
            )?;
            let uvs = match attrs["TEXCOORD_0"].as_u64() {
                Some(a) => Some(doc.floats(a as usize, 2)?),
                None => None,
            };
            let idx = match prim["indices"].as_u64() {
                Some(a) => doc.indices(a as usize)?,
                None => (0..pos.len() as u32).collect(),
            };
            let material = prim["material"].as_u64().map(|m| m as usize);
            for t in idx.as_chunks::<3>().0 {
                let corner = |k: usize| -> Result<([f32; 3], [f32; 2])> {
                    let i = t[k] as usize;
                    let p = pos.get(i).context("an index past the positions")?;
                    let w = transform(m, [p[0], p[1], p[2]]);
                    let uv = uvs
                        .as_ref()
                        .and_then(|u| u.get(i))
                        .map_or([0.0, 0.0], |u| [u[0], u[1]]);
                    Ok(([w[0], -w[1], w[2]].map(|c| c * UNITS_PER_METRE), uv))
                };
                // Flipping Y mirrors the model; reversing the corners keeps
                // each face's winding.
                let (a, b, c) = (corner(0)?, corner(2)?, corner(1)?);
                tris.push(Tri {
                    pos: [a.0, b.0, c.0],
                    uv: [a.1, b.1, c.1],
                    material,
                });
            }
        }
    }
    if tris.is_empty() {
        bail!("the model has no triangles");
    }
    if tris.len() > MAX_TRIANGLES {
        bail!(
            "{} triangles; a model converts at most {MAX_TRIANGLES}",
            tris.len()
        );
    }
    // Materials: a placed texture, or a flat colour.
    let mut placed: BTreeMap<usize, Placed> = BTreeMap::new();
    let mut flat: BTreeMap<Option<usize>, [u8; 3]> = BTreeMap::new();
    let mut by_image: BTreeMap<usize, Placed> = BTreeMap::new();
    for t in &tris {
        let Some(mi) = t.material else {
            flat.entry(None).or_insert([200, 200, 200]);
            continue;
        };
        if placed.contains_key(&mi) || flat.contains_key(&Some(mi)) {
            continue;
        }
        let mat = &j["materials"][mi]["pbrMetallicRoughness"];
        let image = mat["baseColorTexture"]["index"]
            .as_u64()
            .and_then(|t| j["textures"][t as usize]["source"].as_u64())
            .map(|i| i as usize);
        match image {
            Some(img) => {
                let p = match by_image.get(&img) {
                    Some(p) => *p,
                    None => {
                        let png = doc.image(assets, dir, img)?;
                        let p = place_texture(&png, colors, alloc, patches)
                            .with_context(|| format!("image {img}"))?;
                        by_image.insert(img, p);
                        p
                    }
                };
                placed.insert(mi, p);
            }
            None => {
                let f: Vec<f32> = mat["baseColorFactor"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_f64)
                            .map(|f| f as f32)
                            .collect()
                    })
                    .unwrap_or_else(|| vec![1.0, 1.0, 1.0]);
                let c = [0, 1, 2]
                    .map(|i| (f.get(i).copied().unwrap_or(1.0) * 200.0).clamp(0.0, 255.0) as u8);
                flat.insert(Some(mi), c);
            }
        }
    }
    // Objects of at most `OBJECT_VERTICES` vertices.
    let mut objects: Vec<ModelObject> = Vec::new();
    for chunk in tris.chunks(OBJECT_VERTICES / 3) {
        let mut vertices: Vec<[i16; 3]> = Vec::new();
        let mut lookup: BTreeMap<[i16; 3], u16> = BTreeMap::new();
        let mut vert = |p: [f32; 3]| -> Result<u16> {
            let q = p.map(|c| c.round());
            if q.iter().any(|c| !(-32768.0..=32767.0).contains(c)) {
                bail!("a vertex is out of the i16 range; scale the model down");
            }
            let q = q.map(|c| c as i16);
            Ok(*lookup.entry(q).or_insert_with(|| {
                vertices.push(q);
                (vertices.len() - 1) as u16
            }))
        };
        let mut textured = Vec::new();
        let mut plain = Vec::new();
        for t in chunk {
            let v = [vert(t.pos[0])?, vert(t.pos[1])?, vert(t.pos[2])?];
            match t.material.and_then(|m| placed.get(&m)) {
                Some(p) => {
                    let uv = t.uv.map(|[u, v]| {
                        let tu = (u.clamp(0.0, 1.0) * (p.w - 1) as f32).round() as u8;
                        let tv = (v.clamp(0.0, 1.0) * (p.h - 1) as f32).round() as u8;
                        (p.u0 + tu, p.v0 + tv)
                    });
                    textured.push(ModelPrim {
                        vertices: v.to_vec(),
                        uvs: uv.to_vec(),
                        cba: p.cba,
                        tsb: p.tsb,
                        colors: vec![[128, 128, 128]],
                    });
                }
                None => {
                    let c = flat.get(&t.material).copied().unwrap_or([200, 200, 200]);
                    plain.push(ModelPrim {
                        vertices: v.to_vec(),
                        uvs: Vec::new(),
                        cba: 0,
                        tsb: 0,
                        colors: vec![c, c, c],
                    });
                }
            }
        }
        let mut groups = Vec::new();
        if !textured.is_empty() {
            groups.push(ModelGroup {
                shape: PacketShape::Ft3,
                semi_transparent: false,
                prims: textured,
            });
        }
        if !plain.is_empty() {
            groups.push(ModelGroup {
                shape: PacketShape::G3,
                semi_transparent: false,
                prims: plain,
            });
        }
        objects.push(ModelObject {
            vertices,
            groups,
            scale: LEGAIA_OBJECT_SCALE,
        });
    }
    encode(&objects)
}

/// Box-filter `rgba` (`w x h`) down to fit [`MAX_TEXTURE_SIDE`].
fn fit(w: usize, h: usize, rgba: Vec<u8>) -> (usize, usize, Vec<u8>) {
    let f = w.max(h).div_ceil(MAX_TEXTURE_SIDE);
    if f <= 1 {
        return (w, h, rgba);
    }
    let (nw, nh) = (w.div_ceil(f), h.div_ceil(f));
    let mut out = vec![0u8; nw * nh * 4];
    for y in 0..nh {
        for x in 0..nw {
            let mut acc = [0u32; 4];
            let mut n = 0;
            for sy in y * f..((y + 1) * f).min(h) {
                for sx in x * f..((x + 1) * f).min(w) {
                    let p = &rgba[(sy * w + sx) * 4..][..4];
                    for c in 0..4 {
                        acc[c] += u32::from(p[c]);
                    }
                    n += 1;
                }
            }
            for c in 0..4 {
                out[(y * nw + x) * 4 + c] = (acc[c] / n.max(1)) as u8;
            }
        }
    }
    (nw, nh, out)
}

/// Median-cut quantisation of the opaque pixels to at most `n` colours.
/// Returns the palette (BGR555) and each pixel's index; with any transparent
/// pixel, index `0` is the transparent word `0x0000` and the colours take the
/// rest.
pub fn quantise(rgba: &[u8], n: usize) -> (Vec<u16>, Vec<u8>) {
    let px: Vec<[u8; 4]> = rgba.as_chunks::<4>().0.to_vec();
    let clear = px.iter().any(|p| p[3] < 128);
    let slots = if clear { n - 1 } else { n };
    let mut boxes: Vec<Vec<[u8; 3]>> = vec![
        px.iter()
            .filter(|p| p[3] >= 128)
            .map(|p| [p[0], p[1], p[2]])
            .collect(),
    ];
    while boxes.len() < slots {
        // Split the box with the widest channel range.
        let (bi, ch, range) = boxes
            .iter()
            .enumerate()
            .map(|(i, b)| {
                (0..3)
                    .map(|c| {
                        let lo = b.iter().map(|p| p[c]).min().unwrap_or(0);
                        let hi = b.iter().map(|p| p[c]).max().unwrap_or(0);
                        (i, c, hi - lo)
                    })
                    .max_by_key(|&(_, _, r)| r)
                    .unwrap_or((i, 0, 0))
            })
            .max_by_key(|&(_, _, r)| r)
            .unwrap_or((0, 0, 0));
        if range == 0 {
            break;
        }
        let mut b = std::mem::take(&mut boxes[bi]);
        b.sort_unstable_by_key(|p| p[ch]);
        let tail = b.split_off(b.len() / 2);
        boxes[bi] = b;
        boxes.push(tail);
    }
    let mean = |b: &[[u8; 3]]| -> [u8; 3] {
        if b.is_empty() {
            return [0; 3];
        }
        let s = b
            .iter()
            .fold([0u32; 3], |a, p| [0, 1, 2].map(|c| a[c] + u32::from(p[c])));
        s.map(|c| (c / b.len() as u32) as u8)
    };
    let colours: Vec<[u8; 3]> = boxes.iter().map(|b| mean(b)).collect();
    let mut palette: Vec<u16> = Vec::with_capacity(n);
    if clear {
        palette.push(0x0000);
    }
    palette.extend(
        colours
            .iter()
            .map(|c| legaia_tim::encode::rgba8_to_bgr555([c[0], c[1], c[2], 255])),
    );
    palette.resize(n, 0x0000);
    let first = usize::from(clear);
    let index = px
        .iter()
        .map(|p| {
            if p[3] < 128 {
                return 0;
            }
            let best = colours
                .iter()
                .enumerate()
                .min_by_key(|(_, c)| {
                    (0..3)
                        .map(|k| {
                            let d = i32::from(c[k]) - i32::from(p[k]);
                            d * d
                        })
                        .sum::<i32>()
                })
                .map_or(0, |(i, _)| i);
            (first + best) as u8
        })
        .collect();
    (palette, index)
}

/// Quantise a PNG and write its texels and palette into free VRAM.
fn place_texture(
    png: &[u8],
    colors: u16,
    alloc: &mut VramAlloc,
    patches: &mut Vec<VramPatch>,
) -> Result<Placed> {
    let (w, h, rgba) = legaia_tim::encode::decode_png_rgba(png).context("a PNG texture")?;
    if w == 0 || h == 0 {
        bail!("an empty texture");
    }
    let (w, h, rgba) = fit(w, h, rgba);
    let (palette, index) = quantise(&rgba, usize::from(colors));
    let (depth, tpw) = if colors == 16 { (4u8, 4usize) } else { (8, 2) };
    let ww = w.div_ceil(tpw);
    let Some((rect, (px, py))) = alloc.alloc_texture(ww as u16, h as u16, depth, 1) else {
        bail!("no free VRAM for a {w} x {h} texture");
    };
    let mut words = vec![0u16; ww * h];
    for y in 0..h {
        for x in 0..w {
            let i = u16::from(index[y * w + x]);
            let shift = (x % tpw) * (16 / tpw);
            words[y * ww + x / tpw] |= i << shift;
        }
    }
    patches.push(VramPatch {
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: rect.h,
        words,
    });
    let clut = alloc
        .alloc_clut(colors)
        .context("no free VRAM for a palette")?;
    patches.push(VramPatch {
        x: clut.x,
        y: clut.y,
        w: clut.w,
        h: 1,
        words: palette,
    });
    let depth_bits: u16 = if depth == 4 { 0 } else { 1 };
    let tsb = (px / 64) | ((py / 256) << 4) | (depth_bits << 7);
    Ok(Placed {
        tsb,
        cba: cba_word(clut.x, clut.y),
        u0: ((rect.x - px) as usize * tpw) as u8,
        v0: (rect.y - py) as u8,
        w,
        h,
    })
}

#[cfg(test)]
mod tests;
