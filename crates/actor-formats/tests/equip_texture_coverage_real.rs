//! Disc-gated check that every equipment record **draws**: built through the
//! shared [`loadout::build`] path (characters page, shops page,
//! `export-glb --items`, and the same texture uploads the battle renderer
//! takes), no textured primitive of a record's own objects samples nothing
//! but unwritten VRAM.
//!
//! Two defects had that shape. Noa's Force Blade (`0x25`) is her Short
//! Sword record with the texture pool stripped, so its blade sampled an
//! empty band rect (`stripped_pool_donor`); and the TSB/CBA relocation put a
//! light-source row's texture block where a baked-colour row keeps it, so
//! Gala's lit primitives (the `100+` extras of Ra-Seru Ozma `0x19`) stayed
//! on the authoring page (`RELOCATION_TEXBLOCK_WORDS`).
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use std::path::PathBuf;

use legaia_actor_formats::battle_char_assembly::{equip_isolate, loadout};
use legaia_actor_formats::{battle_char_assembly as bca, battle_data_pack};

fn extracted_dir() -> Option<PathBuf> {
    [PathBuf::from("extracted"), PathBuf::from("../../extracted")]
        .into_iter()
        .find(|p| p.join("PROT").is_dir())
}

const FILES: [(&str, &str, usize); 3] = [
    ("0863_edstati3.BIN", "vahn", 0),
    ("0864_edstati3.BIN", "noa", 1),
    ("0865_battle_data.BIN", "gala", 2),
];

/// Is every texel the primitive covers unwritten VRAM (or its CLUT row
/// unwritten)? That is a primitive no host can draw.
fn samples_nothing(p: &equip_isolate::PrimRef, vram: &legaia_tim::Vram) -> bool {
    let cx = usize::from(p.cba & 0x3F) * 16;
    let cy = usize::from((p.cba >> 6) & 0x1FF);
    if !(cx..cx + 16).any(|x| vram.is_written(x, cy)) {
        return true;
    }
    let px = usize::from(p.tsb & 0xF) * 64;
    let py = usize::from((p.tsb >> 4) & 1) * 256;
    let shift = match (p.tsb >> 7) & 3 {
        0 => 2,
        1 => 1,
        _ => 0,
    };
    !p.uvs
        .iter()
        .any(|&(u, v)| vram.is_written(px + (usize::from(u) >> shift), py + usize::from(v)))
}

#[test]
fn every_equipment_record_samples_written_vram() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let mut failures = Vec::new();
    let mut total = 0usize;
    let mut donors = Vec::new();
    for (file, who) in FILES
        .iter()
        .map(|&(f, w, _)| (f, w))
        .chain([("0866_battle_data.BIN", "terra")])
    {
        let raw = std::fs::read(dir.join("PROT").join(file)).expect("player file");
        let pack = battle_data_pack::parse(&raw).expect("player file");
        for r in &pack.records {
            let entry = battle_data_pack::decode_record(&raw, &pack, r.index).expect("decode");
            if let Some(d) = bca::stripped_pool_donor(&raw, &pack, &entry).expect("donor walk") {
                donors.push((who, r.id, d.record.id));
            }
        }
    }
    for (file, who, cslot) in FILES {
        let raw = std::fs::read(dir.join("PROT").join(file)).expect("player file");
        let pack = battle_data_pack::parse(&raw).expect("player file");
        for (section, cat) in loadout::section_catalog(&pack).iter().enumerate() {
            for &id in &cat.ids {
                total += 1;
                let mut load = [0u8; bca::SECTION_COUNT];
                load[section] = id as u8;
                let c = loadout::build(&raw, cslot, load, false, &[])
                    .unwrap_or_else(|e| panic!("{who} {id:#x}: {e}"));
                let tmd = legaia_tmd::parse(&c.tmd_bytes).expect("tmd");
                let item = c
                    .items
                    .iter()
                    .find(|it| it.section == section)
                    .unwrap_or_else(|| panic!("{who} {id:#x}: no item cut"));
                let (mut dark, mut textured) = (0usize, 0usize);
                for &obj in &item.isolation.objects {
                    for p in equip_isolate::object_prim_refs(&tmd, &c.tmd_bytes, obj) {
                        if p.uvs.len() < 3 {
                            continue;
                        }
                        textured += 1;
                        if samples_nothing(&p, &c.vram) {
                            dark += 1;
                        }
                    }
                }
                if dark > 0 {
                    failures.push(format!(
                        "{who} s{section} {id:#x}: {dark} of {textured} textured primitives sample only unwritten VRAM"
                    ));
                }
            }
        }
    }
    assert_eq!(total, 132, "equipment records on the disc");
    // The one stripped pool on the disc, and its source.
    assert_eq!(donors, vec![("noa", 0x25, 0x24)], "stripped-pool records");
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The Force Blade on Noa: the item-alone cut is a blade with opaque texels
/// on every triangle and a blade-shaped bounding box.
#[test]
fn noa_force_blade_alone_cut_is_visible() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let raw = std::fs::read(dir.join("PROT/0864_edstati3.BIN")).expect("PLAYER2");
    let c = loadout::build(&raw, 1, [0, 0, 0, 0x25, 0], false, &[]).expect("loadout");
    let item = c.items.iter().find(|it| it.id == 0x25).expect("item cut");
    let m = &item.alone.mesh;
    let tris = m.indices.len() / 3;
    assert!(tris >= 40, "Force Blade cut keeps {tris} triangles");
    let mut opaque_tris = 0usize;
    for tri in m.indices.as_chunks::<3>().0 {
        let [cba, tsb] = m.cba_tsb[tri[0] as usize];
        let uv = |k: usize| m.uvs[tri[k] as usize];
        let any = bca::equip_skin_mask::triangle_texels(uv(0), uv(1), uv(2))
            .into_iter()
            .any(|(u, v)| {
                legaia_actor_formats::mesh_raster::texel_word(&c.vram, cba, tsb, u, v) & 0x7FFF != 0
            });
        if any {
            opaque_tris += 1;
        }
    }
    assert_eq!(
        opaque_tris, tris,
        "every Force Blade triangle has opaque texels"
    );
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in &m.positions {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let ext: Vec<f32> = (0..3).map(|k| hi[k] - lo[k]).collect();
    let long = ext.iter().cloned().fold(0.0f32, f32::max);
    let short = ext.iter().cloned().fold(f32::MAX, f32::min);
    assert!(
        (80.0..400.0).contains(&long) && short > 2.0 && long > 3.0 * short,
        "blade-shaped bounds, got extents {ext:?}"
    );
}
