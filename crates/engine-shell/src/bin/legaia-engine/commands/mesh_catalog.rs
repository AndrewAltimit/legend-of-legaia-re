//! `mesh-catalog`: measure a scene's geometry pack into the expansion-pack
//! mesh catalog and draw the contact sheet its labels are read from. Thin
//! driver over `engine-export::mesh_catalog`.

use super::*;
use legaia_engine_export::mesh_catalog::{contact_sheet, measure_scene, thumbnail};
use legaia_expansion_format::mesh_catalog::{MeshCatalog, MeshEntry, MeshKind};

fn write_png(path: &Path, w: usize, h: usize, rgba: &[u8]) -> Result<()> {
    let f = std::fs::File::create(path).with_context(|| format!("create {}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(rgba)?;
    Ok(())
}

pub(crate) fn cmd_mesh_catalog(
    scenes: &[String],
    catalog: Option<&Path>,
    sheet_dir: Option<&Path>,
    cell: usize,
    singles: bool,
    extracted_root: &Path,
    disc: Option<&Path>,
) -> Result<()> {
    let index = open_index_from_args(extracted_root, disc)?;
    let mut cat = match catalog {
        Some(p) if p.exists() => MeshCatalog::parse(&std::fs::read_to_string(p)?)
            .with_context(|| format!("parse {}", p.display()))?,
        _ => MeshCatalog::default(),
    };
    if let Some(d) = sheet_dir {
        std::fs::create_dir_all(d)?;
    }
    for name in scenes {
        let pack = measure_scene(&index, name)?;
        println!("{name}: {} meshes", pack.meshes.len());
        for m in &pack.meshes {
            let old = cat.get(name, m.object).cloned();
            println!(
                "  {:>3}  {:>5} x {:>5} x {:>5}  tris {:>4}  {}{}",
                m.object,
                m.size[0],
                m.size[1],
                m.size[2],
                m.tris,
                if m.solid { "solid " } else { "" },
                old.as_ref().map(|e| e.label.as_str()).unwrap_or("")
            );
            let (label, kind, note) = match old {
                Some(e) => (e.label, e.kind, e.note),
                None => ("unlabelled".to_string(), MeshKind::Other, String::new()),
            };
            cat.insert(MeshEntry {
                scene: name.clone(),
                object: m.object,
                label,
                kind,
                size: m.size,
                solid: m.solid,
                tris: m.tris,
                note,
            });
        }
        if let Some(d) = sheet_dir {
            let (w, h, img) = contact_sheet(&pack, cell, 8);
            write_png(&d.join(format!("{name}.png")), w, h, &img)?;
            if singles {
                for m in &pack.meshes {
                    let img = thumbnail(m, &pack.vram, cell * 2);
                    let p = d.join(format!("{name}_{:03}.png", m.object));
                    write_png(&p, cell * 2, cell * 2, &img)?;
                }
            }
        }
    }
    if let Some(p) = catalog {
        std::fs::write(p, cat.to_toml())?;
        println!("catalog: {} entries -> {}", cat.len(), p.display());
    }
    if sheet_dir.is_some() {
        println!(
            "note: the contact sheets draw Sony-derived meshes - keep them local, never commit them"
        );
    }
    Ok(())
}
