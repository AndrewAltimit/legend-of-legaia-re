//! Disc-gated: transplant Vahn's Astral Sword (`0xBA`) into Noa's player
//! file and check the record the loader would splice. Skips without
//! `LEGAIA_DISC_BIN` + `extracted/PROT`. Set `LEGAIA_TRANSPLANT_DUMP=<dir>`
//! to also write the rebuilt file and posed renders.

use std::path::PathBuf;

use legaia_asset::battle_char_assembly as bca;
use legaia_asset::equip_transplant::{
    self, packed_len, rebuild_with_transplants, records_with_transplants, transplant_weapon,
};
use legaia_asset::mesh_raster::{self, Pose, RasterOptions};
use legaia_asset::{battle_data_pack, equip_transplant::record_sections};

const ASTRAL_SWORD: u32 = 0xBA;

fn prot_dir() -> Option<PathBuf> {
    [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ]
    .into_iter()
    .find(|p| p.is_dir())
}

fn load(name: &str) -> Option<Vec<u8>> {
    let dir = prot_dir()?;
    let path = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(name))
        })?;
    std::fs::read(path).ok()
}

fn vram_for(
    raw: &[u8],
    pack: &battle_data_pack::BattleDataPack,
    load: &[u8; 5],
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

fn render(raw: &[u8], load: [u8; 5], path: &std::path::Path) {
    let pack = battle_data_pack::parse(raw).unwrap();
    let mut asm = bca::assemble_character(raw, &pack, &load).unwrap();
    bca::relocate_tsb_cba(&mut asm.tmd, 0).unwrap();
    let tmd = legaia_tmd::parse(&asm.tmd).unwrap();
    let (mesh, obj_ids) = legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, &asm.tmd);
    let vram = vram_for(raw, &pack, &load);
    let bank = bca::battle_animations(raw).expect("action bank");
    let a = bca::expand_animation_for_objects(&bank[0], &asm.anm_bones);
    let poses: Vec<Pose> = a.frames[0]
        .iter()
        .map(|p| Pose::from_keyframe([p.tx, p.ty, p.tz], [p.rx, p.ry, p.rz]))
        .collect();
    let opts = RasterOptions {
        width: 512,
        height: 512,
        yaw: 35f32.to_radians(),
        background: [18, 20, 26, 255],
        shade: 0.3,
        ..Default::default()
    };
    let img = mesh_raster::render_posed(&mesh, &obj_ids, &poses, &vram, &opts);
    let f = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(f, 512, 512);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&img).unwrap();
}

#[test]
fn astral_sword_transplants_into_noas_file() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let (Some(vahn), Some(noa)) = (load("0863_"), load("0864_")) else {
        eprintln!("[skip] player files missing");
        return;
    };
    let t = transplant_weapon(&noa, &vahn, 0, 1, ASTRAL_SWORD).expect("transplant");
    assert_eq!(t.section, 3, "Noa's weapons live in section 3");
    assert_eq!(t.cost, 54, "the Astral Sword keeps its far-off-class price");
    assert!(t.weapon_prims >= 40, "cut claimed {} prims", t.weapon_prims);
    eprintln!(
        "record: {} prims, {} bytes decoded, {} bytes optimal LZS, aliased {}",
        t.weapon_prims,
        t.decoded.len(),
        legaia_lzs::compress_optimal(&t.decoded).len(),
        t.aliased
    );

    // Does not fit the retail footprint; fits with room.
    assert!(rebuild_with_transplants(&noa, std::slice::from_ref(&t), noa.len()).is_err());
    let (pack0, recs) = records_with_transplants(&noa, std::slice::from_ref(&t)).unwrap();
    let need = pack0.data_base + packed_len(&recs);
    eprintln!(
        "Noa + Astral Sword needs {need:#x} bytes ({} sectors), retail footprint {:#x} ({} sectors)",
        need.div_ceil(0x800),
        noa.len(),
        noa.len() / 0x800
    );
    let entry_len = need.div_ceil(0x800) * 0x800;
    let rebuilt =
        rebuild_with_transplants(&noa, std::slice::from_ref(&t), entry_len).expect("rebuild");
    assert_eq!(rebuilt.len(), entry_len);

    let pack = battle_data_pack::parse(&rebuilt).expect("rebuilt pack");
    assert_eq!(pack.records.len(), pack0.records.len() + 1);
    let secs = record_sections(&pack);
    let idx = pack
        .records
        .iter()
        .zip(&secs)
        .find(|(r, s)| r.id == ASTRAL_SWORD && **s == 3)
        .map(|(r, _)| r.index)
        .expect("0xBA in section 3");
    let dec = battle_data_pack::decode_record(&rebuilt, &pack, idx)
        .unwrap()
        .bytes;
    assert_eq!(
        u16::from_le_bytes([dec[0x12], dec[0x13]]),
        1,
        "upload flag on"
    );
    let body_end = u32::from_le_bytes(dec[0xC..0x10].try_into().unwrap()) as usize;
    assert_eq!(
        u16::from_le_bytes([dec[body_end], dec[body_end + 1]]),
        176,
        "Noa's weapon columns"
    );
    assert_eq!(
        u16::from_le_bytes([dec[body_end + 2], dec[body_end + 3]]),
        32
    );
    assert_eq!(
        dec.len() - body_end - 4 - 64,
        0x20 * 0x80 * 2,
        "one section tile"
    );
    let swing = u32::from_le_bytes(dec[4..8].try_into().unwrap()) as usize;
    assert_eq!(dec[swing + 0x74], 54);

    // The loader's view: the assembled hand carries more than the bare one.
    let bare = bca::assemble_character(&rebuilt, &pack, &[0; 5]).unwrap();
    let armed =
        bca::assemble_character(&rebuilt, &pack, &[0, 0, 0, ASTRAL_SWORD as u8, 0]).unwrap();
    assert_eq!(armed.sections[3].id, ASTRAL_SWORD);
    let prims = |a: &bca::AssembledCharacter| {
        let tmd = legaia_tmd::parse(&a.tmd).unwrap();
        tmd.objects
            .iter()
            .map(|o| o.primitives_byte_size)
            .sum::<usize>()
    };
    assert!(
        prims(&armed) > prims(&bare) + 1000,
        "{} vs {}",
        prims(&armed),
        prims(&bare)
    );
    // Every other record survived byte for byte.
    for (r, s) in pack.records.iter().zip(&secs) {
        if r.id == ASTRAL_SWORD && *s == 3 {
            continue;
        }
        let a = battle_data_pack::decode_record(&rebuilt, &pack, r.index)
            .unwrap()
            .bytes;
        let b = battle_data_pack::decode_record(
            &noa,
            &pack0,
            if r.index > idx { r.index - 1 } else { r.index },
        )
        .unwrap()
        .bytes;
        assert_eq!(a, b, "record {} (id {:#x}) changed", r.index, r.id);
    }
    // Idempotent: transplanting into the rebuilt file replaces, never duplicates.
    let again = transplant_weapon(&rebuilt, &vahn, 0, 1, ASTRAL_SWORD).unwrap();
    let (_, recs2) = records_with_transplants(&rebuilt, &[again]).unwrap();
    assert_eq!(recs2.len(), pack.records.len());
    assert!(equip_transplant::weapon_section(&pack) == Some(3));

    if let Some(dir) = std::env::var_os("LEGAIA_TRANSPLANT_DUMP") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("noa_0864_astral.bin"), &rebuilt).unwrap();
        render(
            &rebuilt,
            [0, 0, 0, ASTRAL_SWORD as u8, 0],
            &dir.join("noa_astral.png"),
        );
        render(&noa, [0, 0, 0, 0x22, 0], &dir.join("noa_0x22.png"));
        render(
            &vahn,
            [0, 0, ASTRAL_SWORD as u8, 0, 0],
            &dir.join("vahn_astral.png"),
        );
        eprintln!("dumped to {}", dir.display());
    }
}
