//! Disc-gated check that the **item-alone** cut carries no skin: over every
//! equipment record, built through the same [`loadout::build`] path the
//! characters page, the shops page and `export-glb --items` share, no kept
//! primitive is mostly the character's own flesh, and every cut keeps a
//! floor of geometry. Writes a front / back contact sheet per character when
//! `LEGAIA_ITEM_ALONE_SHEETS=<dir>` is set (per record: the per-primitive
//! cut, the same with the masked skin in magenta, and the final cut front and
//! back); `LEGAIA_ITEM_ALONE_DUMP=noa:0x52`
//! (comma-separated) prints every primitive's classification for a record.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use std::path::{Path, PathBuf};

use legaia_actor_formats::battle_char_assembly::{equip_isolate, loadout};
use legaia_actor_formats::mesh_raster::{self, RasterOptions};
use legaia_actor_formats::{battle_char_assembly as bca, battle_data_pack};
use legaia_game_tables::item_names::ItemNameTable;

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

/// Records the skin assertion is pinned on by name: the two the report
/// named, plus weapons, Ra-Seru, armour, headgear and footwear across the
/// three characters.
const PINNED: [(&str, &str); 10] = [
    ("noa", "Guardian Clip"),
    ("noa", "Fighting Robe"),
    ("vahn", "Great Axe"),
    ("vahn", "Hero Armor"),
    ("vahn", "Warrior Seal"),
    ("gala", "Power Plate"),
    ("gala", "Fighter's Band"),
    ("noa", "Green Clip"),
    ("gala", "Mace"),
    ("noa", "Battle Robe"),
];

/// Largest skin share (of opaque sampled texels) a pinned record's final cut
/// may keep - the shadowed rim a patch did not grow into.
const MAX_PINNED_SKIN_SHARE: f32 = 0.01;

/// Primitive-level floor every item-alone cut must keep.
const MIN_KEPT_PRIMS: usize = 4;

struct Cell {
    label: String,
    views: Vec<Vec<u8>>,
}

#[test]
fn item_alone_cuts_carry_no_skin_and_keep_the_item() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let names = std::fs::read(dir.join("SCUS_942.54"))
        .ok()
        .and_then(|s| ItemNameTable::from_scus(&s));
    let sheets = std::env::var_os("LEGAIA_ITEM_ALONE_SHEETS").map(PathBuf::from);
    let dump: Vec<(String, u32)> = std::env::var("LEGAIA_ITEM_ALONE_DUMP")
        .ok()
        .map(|s| {
            s.split(',')
                .filter_map(|t| {
                    let (w, id) = t.split_once(':')?;
                    let id = u32::from_str_radix(id.trim().trim_start_matches("0x"), 16).ok()?;
                    Some((w.trim().to_string(), id))
                })
                .collect()
        })
        .unwrap_or_default();
    // `LEGAIA_ITEM_ALONE_ZOOM=<px>`: sheet only the dumped records, at that
    // size, as the highlight from four sides.
    let zoom: Option<usize> = std::env::var("LEGAIA_ITEM_ALONE_ZOOM")
        .ok()
        .and_then(|s| s.parse().ok());
    let size = zoom.unwrap_or(120usize);
    let mut pinned_seen = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut total = 0usize;
    for (file, who, cslot) in FILES {
        let path = dir.join("PROT").join(file);
        let Ok(raw) = std::fs::read(&path) else {
            eprintln!("[skip] {} missing", path.display());
            return;
        };
        let pack = battle_data_pack::parse(&raw).expect("player file");
        let cats = loadout::section_catalog(&pack);
        let mut cells: Vec<Cell> = Vec::new();
        let own = {
            let mut bare = bca::assemble_character(&raw, &pack, &[0; bca::SECTION_COUNT])
                .expect("bare assembly");
            bca::relocate_tsb_cba(&mut bare.tmd, loadout::VIEW_BAND).expect("relocate");
            let bare_tmd = legaia_tmd::parse(&bare.tmd).expect("bare TMD");
            let mut v = legaia_tim::Vram::new();
            for u in &bca::character_texture_uploads(
                &raw,
                &pack,
                &[0; bca::SECTION_COUNT],
                loadout::VIEW_BAND,
            )
            .expect("texture pool")
            {
                v.write_block(u.fb_x(), u.fb_y(), u.rect.w, u.rect.h, &u.pixels);
                if !u.clut.is_empty() {
                    v.write_clut_row(u.clut_x, u.clut_row(), &u.clut_bytes());
                }
            }
            legaia_actor_formats::battle_char_assembly::equip_skin_mask::face_colours(
                &equip_isolate::skin_colours(&bare, &bare_tmd, &v),
            )
        };
        for (section, cat) in cats.iter().enumerate() {
            for &id in &cat.ids {
                total += 1;
                let mut load = [0u8; bca::SECTION_COUNT];
                load[section] = id as u8;
                let c = loadout::build(&raw, cslot, load, false, &[])
                    .unwrap_or_else(|e| panic!("{who} {id:#x}: {e}"));
                let item = c
                    .items
                    .iter()
                    .find(|it| it.section == section)
                    .unwrap_or_else(|| panic!("{who} {id:#x}: no item cut"));
                let name = loadout::item_display_name(id, names.as_ref());
                let tmd = legaia_tmd::parse(&c.tmd_bytes).expect("tmd");
                let mut skin_kept = Vec::new();
                let want_dump = dump.iter().any(|(w, i)| w == who && *i == id);
                for &obj in &item.isolation.objects {
                    for p in equip_isolate::object_prim_refs(&tmd, &c.tmd_bytes, obj) {
                        let kept = item.isolation.claims(obj, p.ordinal);
                        let tex = equip_isolate::prim_texels(&p, &c.vram);
                        let n = tex.len().max(1) as f32;
                        let skin =
                            tex.iter().filter(|w| equip_isolate::skin_like(**w)).count() as f32 / n;
                        let ownf = tex.iter().filter(|w| own.contains(w)).count() as f32 / n;
                        if want_dump {
                            let o = &tmd.objects[obj];
                            let ctr = p.corners.iter().fold([0i32; 3], |a, &i| {
                                let v = &o.vertices[i];
                                [a[0] + v.x as i32, a[1] + v.y as i32, a[2] + v.z as i32]
                            });
                            let k = p.corners.len() as i32;
                            eprintln!(
                                "{who} {id:#x} {name}: tag {} ord {} col {} kept {} n {} skin {:.2} own {:.2} ctr {:?} tex {:?}",
                                c.bone_tags[obj],
                                p.ordinal,
                                p.column,
                                kept,
                                tex.len(),
                                skin,
                                ownf,
                                [ctr[0] / k, ctr[1] / k, ctr[2] / k],
                                tex.iter()
                                    .take(6)
                                    .map(|w| (w & 31, (w >> 5) & 31, (w >> 10) & 31))
                                    .collect::<Vec<_>>()
                            );
                        }
                        if kept && skin_dominant(skin, ownf) {
                            skin_kept.push(format!("{}:{}", c.bone_tags[obj], p.ordinal));
                        }
                    }
                }
                let pinned = PINNED.iter().any(|(w, n)| *w == who && *n == name);
                if pinned {
                    pinned_seen += 1;
                }
                // The mesh as the per-primitive cut left it (no texel mask),
                // for the before / highlight panels and the before share.
                let (raw_mesh, raw_ids, raw_prims) =
                    equip_isolate::item_mesh_with_prims(&tmd, &c.tmd_bytes, &item.isolation);
                let (before_share, _) = skin_share(&raw_mesh, &c.vram, &own);
                if want_dump {
                    let mut hist: std::collections::BTreeMap<u16, usize> = Default::default();
                    for tri in raw_mesh.indices.as_chunks::<3>().0 {
                        let [cba, tsb] = raw_mesh.cba_tsb[tri[0] as usize];
                        let uv = |k: usize| raw_mesh.uvs[tri[k] as usize];
                        for (u, v) in legaia_actor_formats::battle_char_assembly::equip_skin_mask::triangle_texels(uv(0), uv(1), uv(2)) {
                            let w = mesh_raster::texel_word(&c.vram, cba, tsb, u, v) & 0x7FFF;
                            if legaia_actor_formats::battle_char_assembly::equip_skin_mask::skin_texel(w, &own) {
                                *hist.entry(w).or_default() += 1;
                            }
                        }
                    }
                    let mut h: Vec<_> = hist.into_iter().collect();
                    h.sort_by_key(|x| std::cmp::Reverse(x.1));
                    eprintln!(
                        "HIST {who} {id:#x} {name}: {:?}",
                        h.iter()
                            .take(12)
                            .map(|(w, n)| ((w & 31, (w >> 5) & 31, (w >> 10) & 31), *n))
                            .collect::<Vec<_>>()
                    );
                }
                let (after_share, skin_tris) = skin_share(&item.alone.mesh, &c.vram, &own);
                let line = format!(
                    "{who} s{section} {id:#x} {name}: skin share {:.3} -> {:.3}, masked {} prims / {} texels, dropped {}, unplaced {}, skin tris {}, kept tris {} -> {}",
                    before_share,
                    after_share,
                    item.alone.skin_mask.masked_primitives,
                    item.alone.skin_mask.masked_texels,
                    item.alone.skin_mask.dropped_primitives,
                    item.alone.skin_mask.unplaced_primitives,
                    skin_tris,
                    raw_mesh.indices.len() / 3,
                    item.alone.mesh.indices.len() / 3,
                );
                eprintln!("{line}");
                if pinned && (skin_tris > 0 || after_share > MAX_PINNED_SKIN_SHARE) {
                    failures.push(format!("{line}; skin prims kept {skin_kept:?}"));
                }
                if item.alone.skin_mask.unplaced_primitives > 0 {
                    failures.push(format!("{line}: no VRAM page for the masked copy"));
                }
                if item.isolation.kept_primitives < MIN_KEPT_PRIMS
                    || item.alone.mesh.indices.len() / 3 < MIN_KEPT_PRIMS
                {
                    failures.push(format!("{line}: below the geometry floor"));
                }
                if sheets.is_some() && (zoom.is_none() || want_dump) {
                    let poses = loadout::rest_poses(&c);
                    let mut lit = c.vram.clone();
                    let (mut hl_mesh, mut hl_ids) = (raw_mesh.clone(), raw_ids.clone());
                    legaia_actor_formats::battle_char_assembly::equip_skin_mask::mask_skin(
                        &mut hl_mesh,
                        &mut hl_ids,
                        &raw_prims,
                        &mut lit,
                        &own,
                        true,
                    );
                    let render = |mesh: &legaia_tmd::mesh::VramMesh,
                                  ids: &[u32],
                                  vram: &legaia_tim::Vram,
                                  yaw: f32| {
                        let opts = RasterOptions {
                            width: size,
                            height: size,
                            yaw: yaw.to_radians(),
                            background: [18, 20, 26, 255],
                            ..Default::default()
                        };
                        mesh_raster::render_posed(mesh, ids, &poses, vram, &opts)
                    };
                    let views = if zoom.is_some() {
                        [35.0f32, 125.0, 215.0, 305.0]
                            .iter()
                            .map(|&y| render(&hl_mesh, &hl_ids, &lit, y))
                            .collect()
                    } else {
                        vec![
                            render(&raw_mesh, &raw_ids, &c.vram, 35.0),
                            render(&hl_mesh, &hl_ids, &lit, 35.0),
                            render(&item.alone.mesh, &item.alone.object_ids, &c.vram, 35.0),
                            render(&item.alone.mesh, &item.alone.object_ids, &c.vram, 215.0),
                        ]
                    };
                    cells.push(Cell {
                        label: format!(
                            "{id:02x} {} {:.2} {:.2}",
                            name.to_ascii_lowercase(),
                            before_share,
                            after_share
                        ),
                        views,
                    });
                }
            }
        }
        if let Some(d) = &sheets {
            std::fs::create_dir_all(d).unwrap();
            write_sheet(&d.join(format!("{who}_alone.png")), &cells, size);
        }
    }
    assert_eq!(total, 132, "equipment records on the disc");
    if names.is_some() {
        assert_eq!(pinned_seen, PINNED.len(), "every pinned record resolved");
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Share of the mesh's opaque sampled texels that are skin, and the number
/// of triangles (at least four texels) that are mostly skin.
fn skin_share(
    mesh: &legaia_tmd::mesh::VramMesh,
    vram: &legaia_tim::Vram,
    own: &std::collections::HashSet<u16>,
) -> (f32, usize) {
    use legaia_actor_formats::battle_char_assembly::equip_skin_mask::{
        skin_texel, triangle_texels,
    };
    let (mut skin, mut total, mut tris) = (0usize, 0usize, 0usize);
    for tri in mesh.indices.as_chunks::<3>().0 {
        let [cba, tsb] = mesh.cba_tsb[tri[0] as usize];
        let uv = |k: usize| mesh.uvs[tri[k] as usize];
        let mut s = 0usize;
        let mut t = 0usize;
        for (u, v) in triangle_texels(uv(0), uv(1), uv(2)) {
            let w = mesh_raster::texel_word(vram, cba, tsb, u, v) & 0x7FFF;
            if w == 0 {
                continue;
            }
            t += 1;
            if skin_texel(w, own) {
                s += 1;
            }
        }
        if t >= 4 && s * 2 >= t {
            tris += 1;
        }
        skin += s;
        total += t;
    }
    (skin as f32 / total.max(1) as f32, tris)
}

/// The assertion's reading of "skin": most of the primitive's texels are
/// peach-hued or the character's own (bare-head) flesh colours.
fn skin_dominant(skin: f32, own: f32) -> bool {
    skin >= 0.5 || own >= 0.5
}

fn write_sheet(path: &Path, cells: &[Cell], size: usize) {
    if cells.is_empty() {
        return;
    }
    let views = cells[0].views.len();
    let cols = 2usize;
    let pw = size * views + 4 * views + 6;
    let ph = size + 14;
    let rows = cells.len().div_ceil(cols);
    let (w, h) = (cols * pw, rows * ph);
    let mut img = vec![0u8; w * h * 4];
    for px in img.as_chunks_mut::<4>().0 {
        px.copy_from_slice(&[10, 10, 12, 255]);
    }
    for (i, c) in cells.iter().enumerate() {
        let x = (i % cols) * pw;
        let y = (i / cols) * ph;
        for (k, v) in c.views.iter().enumerate() {
            let p = mesh_raster::Rgba {
                pixels: v,
                width: size,
                height: size,
            };
            mesh_raster::blit(&mut img, w, h, &p, x + k * (size + 4), y + 12);
        }
        draw_label(&mut img, w, x + 2, y + 2, &c.label);
    }
    legaia_tim::write_png(path, w, h, &img).unwrap();
}

/// 3x5 pixel glyphs for the sheet labels.
fn draw_label(img: &mut [u8], w: usize, x0: usize, y0: usize, text: &str) {
    const ROWS: [&str; 40] = [
        "0111101101101111",
        "1010110010010111",
        "2111001111100111",
        "3111001111001111",
        "4101101111001001",
        "5111100111001111",
        "6111100111101111",
        "7111001010010010",
        "8111101111101111",
        "9111101111001111",
        "a010101111101101",
        "b110101110101110",
        "c011100100100011",
        "d110101101101110",
        "e111100110100111",
        "f111100110100100",
        "g011100101101011",
        "h101101111101101",
        "i111010010010111",
        "j001001001101010",
        "k101110100110101",
        "l100100100100111",
        "m101111111101101",
        "n110101101101101",
        "o010101101101010",
        "p110101110100100",
        "q010101101111011",
        "r110101110101101",
        "s011100010001110",
        "t111010010010010",
        "u101101101101111",
        "v101101101101010",
        "w101101111111101",
        "x101101010101101",
        "y101101010010010",
        "z111001010100111",
        "-000000111000000",
        "'010010000000000",
        ".000000000000010",
        " 000000000000000",
    ];
    let mut x = x0;
    for ch in text.chars() {
        let row = ROWS.iter().find(|r| r.starts_with(ch));
        let bits: Vec<bool> = match row {
            Some(r) => r[1..].chars().map(|b| b == '1').collect(),
            None => vec![true; 15],
        };
        for r in 0..5 {
            for col in 0..3 {
                if bits[r * 3 + col] {
                    let o = ((y0 + r) * w + x + col) * 4;
                    if o + 4 <= img.len() {
                        img[o..o + 4].copy_from_slice(&[230, 230, 230, 255]);
                    }
                }
            }
        }
        x += 4;
    }
}
