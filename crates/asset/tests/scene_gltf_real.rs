//! Disc-gated smoke test for the scene `.glb` exporter: bake a real kingdom
//! bundle (Drake, PROT 0085/0086 `map01`) through [`legaia_asset::scene_gltf`]
//! and assert the container + texture atlas come out well-formed.
//!
//! Mirrors the site's world-overview composition: slot 0 TIM_LIST fills a
//! software VRAM, slot 1's TMD pack provides the landmark meshes, and each
//! mesh is placed as an instance - the same buffers + transforms the WASM
//! `scene_export_*` API receives from the page. Skips silently when
//! `extracted/PROT/` is missing.

use legaia_asset::scene_gltf::{SceneInstance, SceneMesh, build_scene_glb};
use std::path::PathBuf;

fn prot_dir() -> Option<PathBuf> {
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        let d = PathBuf::from(p);
        if d.is_dir() {
            return Some(d);
        }
    }
    None
}

fn entry_bytes(dir: &PathBuf, index: u32) -> Option<Vec<u8>> {
    for e in std::fs::read_dir(dir).ok()? {
        let e = e.ok()?;
        let name = e.file_name();
        let s = name.to_string_lossy().to_string();
        if s.starts_with(&format!("{index:04}_")) && s.ends_with(".BIN") {
            return std::fs::read(e.path()).ok();
        }
    }
    None
}

fn read_u32(buf: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(buf.get(off..off + 4)?.try_into().ok()?))
}

/// Locate the 7-asset scene table inside a kingdom bundle entry: scan
/// 0x800-aligned offsets for `[u32 count == 7]` with descriptor 0's
/// `data_offset == 0x40` (the same detection the web viewer uses).
fn find_asset_table(buf: &[u8]) -> Option<usize> {
    (0..buf.len())
        .step_by(0x800)
        .find(|&off| read_u32(buf, off) == Some(7) && read_u32(buf, off + 12) == Some(0x40))
}

#[test]
fn drake_kingdom_bundle_bakes_a_wellformed_glb() {
    let Some(dir) = prot_dir() else {
        eprintln!("[skip] extracted/PROT/ missing");
        return;
    };
    // Base entry (prescript variant) or base+1 (bare table).
    let (buf, which) = match (entry_bytes(&dir, 85), entry_bytes(&dir, 86)) {
        (Some(b), _) if find_asset_table(&b).is_some() => (b, 85),
        (_, Some(b)) => (b, 86),
        _ => {
            eprintln!("[skip] PROT 0085/0086 not extracted");
            return;
        }
    };
    let table_off = find_asset_table(&buf).expect("7-asset table in kingdom bundle");
    let table = &buf[table_off..];

    // Slot 0 = TIM_LIST (type 0x01) -> VRAM.
    let slot0_ts = read_u32(table, 8).unwrap();
    let slot0_off = read_u32(table, 12).unwrap() as usize;
    assert_eq!(slot0_ts >> 24, 0x01, "slot 0 is the TIM_LIST");
    let tims = legaia_lzs::decompress(&table[slot0_off..], (slot0_ts & 0xFF_FFFF) as usize)
        .expect("slot 0 LZS");
    let mut vram = legaia_tim::Vram::new();
    let count = read_u32(&tims, 0).unwrap() as usize;
    let mut uploaded = 0;
    for k in 0..count {
        let bo = read_u32(&tims, 4 + k * 4).unwrap() as usize * 4;
        if let Some(src) = tims.get(bo..)
            && let Ok(tim) = legaia_tim::parse(src)
        {
            vram.upload_tim(&tim);
            uploaded += 1;
        }
    }
    assert!(uploaded > 10, "kingdom TIM_LIST uploads many TIMs");

    // Slot 1 = TMD pack (type 0x02) -> landmark meshes.
    let slot1_ts = read_u32(table, 16).unwrap();
    let slot1_off = read_u32(table, 20).unwrap() as usize;
    assert_eq!(slot1_ts >> 24, 0x02, "slot 1 is the TMD pack");
    let pack = legaia_lzs::decompress(&table[slot1_off..], (slot1_ts & 0xFF_FFFF) as usize)
        .expect("slot 1 LZS");
    let nslots = read_u32(&pack, 0).unwrap() as usize;
    assert!(
        nslots > 4,
        "kingdom pack carries several TMDs (PROT {which})"
    );

    // Mesh the first few decodable slots and place each twice - one plain,
    // one rotated + scaled - exactly the shape the site's export path feeds.
    let mut meshes = Vec::new();
    let mut instances = Vec::new();
    for slot in 0..nslots.min(6) {
        let start = read_u32(&pack, 4 + slot * 4).unwrap() as usize * 4;
        let end = if slot + 1 < nslots {
            read_u32(&pack, 4 + (slot + 1) * 4).unwrap() as usize * 4
        } else {
            pack.len()
        };
        let Some(tmd_buf) = pack.get(start..end) else {
            continue;
        };
        let Ok(tmd) = legaia_tmd::parse(tmd_buf) else {
            continue;
        };
        let m = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, tmd_buf);
        if m.positions.is_empty() || m.indices.is_empty() {
            continue;
        }
        let mi = meshes.len();
        meshes.push(SceneMesh {
            name: format!("mesh_{slot}"),
            positions: m.positions.iter().flatten().copied().collect(),
            uvs: m.uvs.iter().flatten().copied().collect(),
            cba_tsb: m.cba_tsb.iter().flatten().copied().collect(),
            indices: m.indices.clone(),
            flat_rgba: Vec::new(),
        });
        instances.push(SceneInstance {
            mesh: mi,
            translation: [slot as f32 * 500.0, 0.0, 0.0],
            rot_y: 0.0,
            scale: 1.0,
        });
        instances.push(SceneInstance {
            mesh: mi,
            translation: [slot as f32 * 500.0, -32.0, 800.0],
            rot_y: std::f32::consts::FRAC_PI_2,
            scale: 6.0,
        });
    }
    assert!(!meshes.is_empty(), "at least one kingdom TMD meshes");

    let glb = build_scene_glb("drake", &meshes, &instances, &vram).expect("glb bakes");

    // GLB container invariants.
    assert_eq!(&glb[0..4], b"glTF");
    let total = u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize;
    assert_eq!(total, glb.len());
    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let root: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
    assert_eq!(root["asset"]["version"], "2.0");
    assert_eq!(
        root["meshes"].as_array().unwrap().len(),
        meshes.len(),
        "every placed mesh is emitted"
    );
    // instances + 1 root node.
    assert_eq!(root["nodes"].as_array().unwrap().len(), instances.len() + 1);
    // A real baked atlas: the PNG bufferView exists and holds real data
    // (kingdom pages are richly textured, so well past a blank PNG's size).
    let img_view = root["images"][0]["bufferView"].as_u64().unwrap() as usize;
    let png_len = root["bufferViews"][img_view]["byteLength"]
        .as_u64()
        .unwrap();
    assert!(
        png_len > 4_000,
        "atlas PNG is non-trivial ({png_len} bytes)"
    );
}
