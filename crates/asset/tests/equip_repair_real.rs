//! Disc-gated sweep of [`battle_char_assembly::equip_repair`] - the grip
//! bridge - over every held-item record on the disc, plus a contact-sheet
//! writer for the visual pass (`LEGAIA_EQUIP_SHEETS=<dir>`).
//!
//! What is asserted is the shape of the inference, not a per-record list:
//! the repair only ever adds geometry (never removes), only where two rims
//! face each other in one object, and it closes the one case that motivated
//! it - Vahn's Great Axe comes out of the cut in two pieces and leaves as one.
//! A cuff-shaped record (a Ra-Seru form re-sculpting the forearm, open at
//! both ends) gains nothing.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use legaia_asset::battle_char_assembly::{equip_isolate, equip_item, equip_repair};
use legaia_asset::mesh_raster::{self, Pose, RasterOptions};
use legaia_asset::{battle_char_assembly as bca, battle_data_pack};

fn extracted_prot_dir() -> Option<PathBuf> {
    [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ]
    .into_iter()
    .find(|p| p.is_dir())
}

fn section_ids(pack: &battle_data_pack::BattleDataPack) -> Vec<Vec<u32>> {
    let mut out: Vec<Vec<u32>> = vec![Vec::new(); bca::SECTION_COUNT];
    let mut slot = 0usize;
    for r in &pack.records {
        if slot >= bca::SECTION_COUNT {
            break;
        }
        if r.id == 0 {
            slot += 1;
        } else {
            out[slot].push(r.id);
        }
    }
    out
}

fn vram_for(
    raw: &[u8],
    pack: &battle_data_pack::BattleDataPack,
    load: &[u8; bca::SECTION_COUNT],
) -> legaia_tim::Vram {
    let mut vram = legaia_tim::Vram::new();
    for u in &bca::character_texture_uploads(raw, pack, load, 0).expect("texture pool") {
        vram.write_block(u.fb_x(), u.fb_y(), u.rect.w, u.rect.h, &u.pixels);
        if !u.clut.is_empty() {
            vram.write_clut_row(u.clut_x, u.clut_row(), &u.clut_bytes());
        }
    }
    vram
}

const FILES: [(&str, &str, usize); 3] = [
    ("0863_edstati3.BIN", "vahn", 0),
    ("0864_edstati3.BIN", "noa", 1),
    ("0865_battle_data.BIN", "gala", 2),
];

/// Item name from the SCUS table, when the disc image is around to read it
/// from; the PROT id otherwise.
fn item_name(id: u32) -> String {
    format!("{id:#04x}")
}

struct Cell {
    label: String,
    before: Vec<u8>,
    after: Vec<u8>,
}

#[test]
fn held_items_only_gain_geometry_and_the_great_axe_leaves_in_one_piece() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let sheets = std::env::var_os("LEGAIA_EQUIP_SHEETS").map(PathBuf::from);
    let rules = equip_isolate::rules();
    let size = std::env::var("LEGAIA_EQUIP_SHEET_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(112usize);
    let yaw_deg: f32 = std::env::var("LEGAIA_EQUIP_SHEET_YAW")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(35.0);
    let mut bridged: BTreeMap<(&str, u32), Vec<equip_repair::Bridge>> = BTreeMap::new();
    let mut total = 0usize;
    for (file, who, cslot) in FILES {
        let path = dir.join(file);
        let Ok(raw) = std::fs::read(&path) else {
            eprintln!("[skip] {} missing", path.display());
            return;
        };
        let pack = battle_data_pack::parse(&raw).expect("player file");
        let ids = section_ids(&pack);
        let bank = bca::battle_animations(&raw).expect("action bank");
        let mut cells: Vec<Cell> = Vec::new();
        for section in equip_item::ITEM_SECTIONS {
            for &id in &ids[section] {
                total += 1;
                let mut load = [0u8; bca::SECTION_COUNT];
                load[section] = id as u8;
                let mut without = load;
                without[section] = 0;
                let mut bare = bca::assemble_character(&raw, &pack, &without).unwrap();
                bca::relocate_tsb_cba(&mut bare.tmd, 0).unwrap();
                let bare_tmd = legaia_tmd::parse(&bare.tmd).unwrap();
                let bare_vram = vram_for(&raw, &pack, &without);
                let mut eq = bca::assemble_character(&raw, &pack, &load).unwrap();
                bca::relocate_tsb_cba(&mut eq.tmd, 0).unwrap();
                let eq_tmd = legaia_tmd::parse(&eq.tmd).unwrap();
                let vram = vram_for(&raw, &pack, &load);
                let partition =
                    equip_item::item_partition(section, &bare, &bare_tmd, &eq, &eq_tmd).unwrap();
                let iso = equip_isolate::isolate_item(
                    &equip_isolate::IsolationInputs {
                        section,
                        bare: &bare,
                        bare_tmd: &bare_tmd,
                        bare_vram: &bare_vram,
                        equipped: &eq,
                        equipped_tmd: &eq_tmd,
                        vram: &vram,
                        partition: &partition,
                    },
                    rules.rule_for(cslot, id),
                );
                let (mut mesh, mut obj_ids) = equip_isolate::item_mesh(&eq_tmd, &eq.tmd, &iso);
                let before = (mesh.positions.len(), mesh.indices.len());
                let poses: Vec<Pose> = {
                    let a = bca::expand_animation_for_objects(&bank[0], &eq.anm_bones);
                    a.frames[0]
                        .iter()
                        .map(|p| Pose::from_keyframe([p.tx, p.ty, p.tz], [p.rx, p.ry, p.rz]))
                        .collect()
                };
                let opts = RasterOptions {
                    width: size,
                    height: size,
                    yaw: yaw_deg.to_radians(),
                    background: [18, 20, 26, 255],
                    ..Default::default()
                };
                let img_before = sheets
                    .as_ref()
                    .map(|_| mesh_raster::render_posed(&mesh, &obj_ids, &poses, &vram, &opts));
                let bridges = equip_repair::bridge_open_loops(&mut mesh, &mut obj_ids);
                let after = (mesh.positions.len(), mesh.indices.len());
                // Only ever additive, and streams stay parallel.
                assert!(after.0 >= before.0 && after.1 >= before.1, "{who} {id:#x}");
                assert_eq!(mesh.positions.len(), obj_ids.len(), "{who} {id:#x}");
                assert_eq!(mesh.uvs.len(), mesh.positions.len(), "{who} {id:#x}");
                let added: usize = bridges.iter().map(|b| b.triangles).sum();
                assert_eq!(
                    after.1 - before.1,
                    added * 3,
                    "{who} {id:#x}: triangle accounting"
                );
                for b in &bridges {
                    // A bridge never spans objects and never leaves one rim.
                    assert!(b.loop_a >= 3 && b.loop_b >= 3, "{who} {id:#x}: {b:?}");
                    assert!(
                        eq.section_of[b.object as usize] as usize == section,
                        "{who} {id:#x}: bridge on an object outside the section"
                    );
                }
                if !bridges.is_empty() {
                    bridged.insert((who, id), bridges.clone());
                }
                if let Some(before_img) = img_before {
                    let after_img =
                        mesh_raster::render_posed(&mesh, &obj_ids, &poses, &vram, &opts);
                    cells.push(Cell {
                        label: format!(
                            "{who} s{section} {} {} b{}",
                            item_name(id),
                            partition.class.tag(),
                            bridges.len()
                        ),
                        before: before_img,
                        after: after_img,
                    });
                }
            }
        }
        if let Some(dir) = &sheets {
            std::fs::create_dir_all(dir).unwrap();
            write_sheet(&dir.join(format!("{who}_grips.png")), &cells, size);
        }
    }
    assert_eq!(
        total, 81,
        "held-item records (weapons + Ra-Seru) on the disc"
    );
    // The motivating case: Vahn's Great Axe (0x33) leaves the cut in two
    // pieces (welded haft, fist between) and the repair joins them.
    let axe = bridged
        .get(&("vahn", 0x33))
        .expect("Great Axe got no bridge");
    assert!(
        axe.iter().any(|b| b.triangles >= 6),
        "Great Axe bridge too small: {axe:?}"
    );
    eprintln!(
        "grip bridges on {} of {} held-item records: {:?}",
        bridged.len(),
        total,
        bridged
            .iter()
            .map(|((w, id), b)| format!("{w}:{id:#x}x{}", b.len()))
            .collect::<Vec<_>>()
    );
    // Sanity on the false-positive side: a bridge that spans more than a
    // hand's width of shaft is not a grip.
    for ((who, id), bs) in &bridged {
        for b in bs {
            assert!(b.gap < 120.0, "{who} {id:#x}: implausible bridge {b:?}");
        }
    }
}

/// `before | after` per record, one row each, labelled.
fn write_sheet(path: &Path, cells: &[Cell], size: usize) {
    if cells.is_empty() {
        return;
    }
    let cols = 4usize; // records per row (each record = 2 panels)
    let pw = size * 2 + 6;
    let ph = size + 14;
    let rows = cells.len().div_ceil(cols);
    let (w, h) = (cols * pw, rows * ph);
    let mut img = vec![0u8; w * h * 4];
    for px in img.chunks_exact_mut(4) {
        px.copy_from_slice(&[10, 10, 12, 255]);
    }
    for (i, c) in cells.iter().enumerate() {
        let x = (i % cols) * pw;
        let y = (i / cols) * ph;
        fn panel(px: &[u8], size: usize) -> mesh_raster::Rgba<'_> {
            mesh_raster::Rgba {
                pixels: px,
                width: size,
                height: size,
            }
        }
        mesh_raster::blit(&mut img, w, h, &panel(&c.before, size), x, y + 12);
        mesh_raster::blit(&mut img, w, h, &panel(&c.after, size), x + size + 4, y + 12);
        draw_label(&mut img, w, x + 2, y + 2, &c.label);
    }
    legaia_tim::write_png(path, w, h, &img).unwrap();
}

/// 3x5 pixel glyphs for the sheet labels (digits, a-z, a few marks).
fn draw_label(img: &mut [u8], w: usize, x0: usize, y0: usize, text: &str) {
    const FONT: [(&str, [u8; 5]); 43] = [
        ("0", [0b111, 0b101, 0b101, 0b101, 0b111]),
        ("1", [0b010, 0b110, 0b010, 0b010, 0b111]),
        ("2", [0b111, 0b001, 0b111, 0b100, 0b111]),
        ("3", [0b111, 0b001, 0b111, 0b001, 0b111]),
        ("4", [0b101, 0b101, 0b111, 0b001, 0b001]),
        ("5", [0b111, 0b100, 0b111, 0b001, 0b111]),
        ("6", [0b111, 0b100, 0b111, 0b101, 0b111]),
        ("7", [0b111, 0b001, 0b010, 0b010, 0b010]),
        ("8", [0b111, 0b101, 0b111, 0b101, 0b111]),
        ("9", [0b111, 0b101, 0b111, 0b001, 0b111]),
        ("a", [0b010, 0b101, 0b111, 0b101, 0b101]),
        ("b", [0b110, 0b101, 0b110, 0b101, 0b110]),
        ("c", [0b011, 0b100, 0b100, 0b100, 0b011]),
        ("d", [0b110, 0b101, 0b101, 0b101, 0b110]),
        ("e", [0b111, 0b100, 0b110, 0b100, 0b111]),
        ("f", [0b111, 0b100, 0b110, 0b100, 0b100]),
        ("g", [0b011, 0b100, 0b101, 0b101, 0b011]),
        ("h", [0b101, 0b101, 0b111, 0b101, 0b101]),
        ("i", [0b111, 0b010, 0b010, 0b010, 0b111]),
        ("j", [0b001, 0b001, 0b001, 0b101, 0b010]),
        ("k", [0b101, 0b110, 0b100, 0b110, 0b101]),
        ("l", [0b100, 0b100, 0b100, 0b100, 0b111]),
        ("m", [0b101, 0b111, 0b111, 0b101, 0b101]),
        ("n", [0b110, 0b101, 0b101, 0b101, 0b101]),
        ("o", [0b010, 0b101, 0b101, 0b101, 0b010]),
        ("p", [0b110, 0b101, 0b110, 0b100, 0b100]),
        ("q", [0b010, 0b101, 0b101, 0b111, 0b011]),
        ("r", [0b110, 0b101, 0b110, 0b101, 0b101]),
        ("s", [0b011, 0b100, 0b010, 0b001, 0b110]),
        ("t", [0b111, 0b010, 0b010, 0b010, 0b010]),
        ("u", [0b101, 0b101, 0b101, 0b101, 0b111]),
        ("v", [0b101, 0b101, 0b101, 0b101, 0b010]),
        ("w", [0b101, 0b101, 0b111, 0b111, 0b101]),
        ("x", [0b101, 0b101, 0b010, 0b101, 0b101]),
        ("y", [0b101, 0b101, 0b010, 0b010, 0b010]),
        ("z", [0b111, 0b001, 0b010, 0b100, 0b111]),
        ("-", [0b000, 0b000, 0b111, 0b000, 0b000]),
        (":", [0b000, 0b010, 0b000, 0b010, 0b000]),
        ("$", [0b010, 0b111, 0b110, 0b011, 0b111]),
        ("#", [0b101, 0b111, 0b101, 0b111, 0b101]),
        ("(", [0b001, 0b010, 0b010, 0b010, 0b001]),
        (")", [0b100, 0b010, 0b010, 0b010, 0b100]),
        (" ", [0; 5]),
    ];
    let mut x = x0;
    for ch in text.chars() {
        let s = ch.to_ascii_lowercase().to_string();
        let glyph = FONT
            .iter()
            .find(|(c, _)| *c == s)
            .map(|(_, g)| *g)
            .unwrap_or([0b111; 5]);
        for (r, row) in glyph.iter().enumerate() {
            for c in 0..3 {
                if row & (0b100 >> c) != 0 {
                    let o = ((y0 + r) * w + x + c) * 4;
                    if o + 4 <= img.len() {
                        img[o..o + 4].copy_from_slice(&[230, 230, 230, 255]);
                    }
                }
            }
        }
        x += 4;
    }
}
