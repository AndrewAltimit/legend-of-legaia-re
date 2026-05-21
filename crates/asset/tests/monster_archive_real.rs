//! Disc-gated regression test: decode the monster stat archive (PROT entry
//! `0867_battle_data`) and assert known monster ids decode to the expected
//! name + HP/MP. These values were byte-validated against live battle RAM
//! (Gimard id 10, Killer Bee id 62, Queen Bee id 63 match the actor stats a
//! PCSX-Redux watchpoint captured during the Rim Elm scripted fights).
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.
//!
//! What this catches:
//! - The `(id-1)*0x14000` slot stride or the `[u32 dec_size][LZS]` slot
//!   framing regresses.
//! - The record byte layout (name offset / HP / MP) drifts.
//! - PROT entry 867 stops being the monster archive (e.g. an extractor
//!   change truncates it).

use legaia_asset::monster_archive;
use std::path::PathBuf;

fn entry_867() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        let f = PathBuf::from(p).join("0867_battle_data.BIN");
        if f.is_file() {
            return std::fs::read(f).ok();
        }
    }
    None
}

#[test]
fn known_monster_ids_decode_to_expected_records() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };

    // (id, name, hp, mp) - the Rim Elm scripted-battle monster set; HP/MP
    // for Gimard/Killer Bee/Queen Bee are byte-exact vs live battle RAM.
    let expected: &[(u16, &str, u16, u16)] = &[
        (4, "Gobu Gobu", 76, 15),
        (7, "Green Slime", 69, 24),
        (10, "Gimard", 99, 20),
        (61, "Hornet", 188, 88),
        (62, "Killer Bee", 288, 288),
        (63, "Queen Bee", 888, 888),
        (79, "Tetsu", 999, 999),
    ];

    for &(id, name, hp, mp) in expected {
        let rec = monster_archive::record(&entry, id)
            .unwrap_or_else(|e| panic!("id {id}: decode error {e:#}"))
            .unwrap_or_else(|| panic!("id {id}: expected a record, got None"));
        assert_eq!(rec.name, name, "id {id} name");
        assert_eq!(rec.hp, hp, "id {id} HP");
        assert_eq!(rec.mp, mp, "id {id} MP");
    }

    // The archive holds 194 fixed slots; a healthy fraction decode to real
    // records (the rest are filler / unused ids - e.g. index 78 "Comm").
    let all = monster_archive::records(&entry).expect("archive walk");
    assert!(
        all.len() > 100,
        "expected >100 populated monster records, got {}",
        all.len()
    );
}

#[test]
fn spell_list_decodes_from_record_offset_array() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };

    // Gimard (id 10): magic_count 9; the +0x4C offsets resolve to the
    // passive/affinity prefix (ids 0,1,2,4,5,0x0B at cost 0), two offensive
    // castable spells (0x0D @ 28 SP, 0x0F @ 32 SP, both <= its SP stat 60),
    // and the 0x23 ('#') special slot. Spirit (stats[0]) gates casting.
    let gimard = monster_archive::record(&entry, 10).unwrap().unwrap();
    assert_eq!(gimard.magic_count as usize, gimard.spells.len());
    assert_eq!(gimard.magic_count, 9);
    let castable: Vec<(u8, u8)> = gimard
        .spells
        .iter()
        .filter(|s| s.is_castable())
        .map(|s| (s.id, s.sp_cost))
        .collect();
    assert_eq!(castable, vec![(0x0D, 28), (0x0F, 32)]);
    for (id, cost) in &castable {
        assert!(
            (*cost as u16) <= gimard.spirit(),
            "Gimard spell 0x{id:02X} cost {cost} should be <= SP {}",
            gimard.spirit()
        );
    }

    // Every populated record's spell list length matches its declared count,
    // and no offset escaped the block (the parser would have dropped it).
    for r in monster_archive::records(&entry).unwrap() {
        assert_eq!(
            r.magic_count as usize,
            r.spells.len(),
            "id {} magic_count {} != decoded spells {}",
            r.id,
            r.magic_count,
            r.spells.len()
        );
    }
}

#[test]
fn monster_mesh_is_an_embedded_tmd_at_record_plus_4() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };

    // Gimard (id 10): the embedded mesh sits at block +0x7c (= the value of
    // stat record +0x04), parses as a Legaia TMD, and has real geometry.
    let m = monster_archive::mesh(&entry, 10)
        .expect("mesh decode")
        .expect("Gimard has a mesh");
    assert_eq!(m.id, 10);
    assert_eq!(m.tmd_offset, 0x7c);
    let tmd = legaia_tmd::parse(m.tmd_bytes()).expect("Gimard TMD parses");
    let st = tmd.stats();
    assert_eq!(st.total_vertices, 200, "Gimard vertex count");
    assert!(st.total_primitives > 0, "Gimard has primitives");
    // The texture/CLUT pool pointer (+0x08) lands inside the block.
    assert!(
        m.texture_pool_bytes().is_some(),
        "Gimard has a texture pool"
    );

    // Almost every populated stat record carries a parseable mesh; only a
    // handful of slots are empty/filler. Assert the overwhelming majority of
    // the roster has a TMD at +0x04 that the parser walks without error.
    let mut with_mesh = 0usize;
    let mut total = 0usize;
    for id in 1..=monster_archive::slot_count(&entry) as u16 {
        if monster_archive::record(&entry, id).unwrap().is_none() {
            continue; // empty / filler slot
        }
        total += 1;
        if let Some(mesh) = monster_archive::mesh(&entry, id).unwrap()
            && legaia_tmd::parse(mesh.tmd_bytes()).is_ok()
        {
            with_mesh += 1;
        }
    }
    assert!(total > 100, "expected >100 populated records, got {total}");
    assert!(
        with_mesh as f64 / total as f64 > 0.95,
        "expected >95% of populated records to carry a parseable mesh, got {with_mesh}/{total}"
    );
}

#[test]
fn monster_texture_pool_decodes_palettes_and_4bpp_page() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };

    // The texture-pool layout (`FUN_80055468`): 0x1E0-byte CLUT region of 15
    // sixteen-colour palettes, then a 4bpp page that is always 256 rows tall
    // and 128 (narrow) or 256 (wide) texels across. The byte arithmetic is
    // exact: pool_len == 0x1E0 + width_texels * 256 / 2.
    let gimard = monster_archive::mesh(&entry, 10).unwrap().unwrap();
    let gt = gimard.texture().expect("Gimard has a texture pool");
    assert_eq!(gt.height, 256, "page is 256 rows tall");
    assert_eq!(gt.width, 128, "Gimard is a narrow (128-texel) page");
    assert_eq!(gt.palettes.len(), 15, "15 palettes");
    assert_eq!(
        gt.indices.len(),
        gt.width * gt.height,
        "one index per texel"
    );
    assert!(
        gt.indices.iter().all(|&i| i < 16),
        "every texel index fits a 16-colour palette"
    );
    let pool_len = gimard.texture_pool_bytes().unwrap().len();
    assert_eq!(
        pool_len,
        0x1E0 + gt.width * gt.height / 2,
        "pool length is exactly CLUT region + 4bpp page"
    );

    // Tetsu (id 79) is a wide humanoid: a 256-texel page (double the bytes).
    let tetsu = monster_archive::mesh(&entry, 79).unwrap().unwrap();
    let tt = tetsu.texture().expect("Tetsu has a texture pool");
    assert_eq!(tt.width, 256, "Tetsu is a wide (256-texel) page");
    assert_eq!(tt.height, 256);
    assert_eq!(
        tetsu.texture_pool_bytes().unwrap().len(),
        0x1E0 + tt.width * tt.height / 2
    );

    // The baked RGBA image is the right size and carries opaque texels.
    let rgba = gt.to_rgba(0);
    assert_eq!(rgba.len(), gt.width * gt.height * 4);
    assert!(
        rgba.chunks_exact(4).any(|p| p[3] == 255),
        "atlas has opaque texels"
    );
    // PSX transparency is per-palette: a texel is transparent when its palette
    // colour is 0x0000. At least one of Gimard's palettes uses index 0 as the
    // transparent background (e.g. palette 1 starts `00 00`).
    assert!(
        gt.palettes.iter().any(|p| p[0] == [0, 0, 0, 0]),
        "some palette uses a transparent index-0"
    );
}

#[test]
fn animation_streams_decode_one_part_per_tmd_object() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };

    // Gimard (id 10): action index 0 is the idle animation. Its packed stream
    // at entry +0x8c carries one part per TMD object (11) across 40 frames.
    let mesh = monster_archive::mesh(&entry, 10).unwrap().unwrap();
    let nobj = legaia_tmd::parse(mesh.tmd_bytes()).unwrap().objects.len();
    assert_eq!(nobj, 11, "Gimard TMD object count");

    let idle = monster_archive::idle_animation(&entry, 10)
        .unwrap()
        .expect("Gimard has an idle animation");
    assert_eq!(idle.action_id, 0, "action index 0 is the idle action");
    assert_eq!(idle.part_count, nobj, "one animated part per TMD object");

    // The site animates per object, so the VRAM mesh must expose a per-vertex
    // object id parallel to its positions, every id within the object count.
    let (vmesh, object_ids) = legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(
        &legaia_tmd::parse(mesh.tmd_bytes()).unwrap(),
        mesh.tmd_bytes(),
    );
    assert_eq!(
        object_ids.len(),
        vmesh.positions.len(),
        "one object id per vertex"
    );
    assert!(
        object_ids.iter().all(|&o| (o as usize) < nobj),
        "object ids in range"
    );
    assert_eq!(idle.frame_count, 40, "Gimard idle has 40 frames");
    assert_eq!(idle.frames.len(), 40);
    assert!(idle.frames.iter().all(|f| f.len() == nobj));
    // Rotations are 12-bit angles (< 4096); the idle has some motion.
    assert!(
        idle.frames
            .iter()
            .flatten()
            .all(|p| p.rx < 4096 && p.ry < 4096 && p.rz < 4096)
    );
    let f0 = &idle.frames[0];
    let f_mid = &idle.frames[idle.frame_count / 2];
    assert!(
        f0.iter().zip(f_mid).any(|(a, b)| a != b),
        "idle animation actually moves between frame 0 and the midpoint"
    );

    // Across the whole roster: every monster with a mesh decodes at least one
    // action animation, and (with one known exception) each action's part count
    // matches the monster's TMD object count.
    let (mut with_anim, mut total_mesh, mut part_match, mut action_total) = (0, 0, 0, 0);
    for id in 1..=monster_archive::slot_count(&entry) as u16 {
        let Some(mesh) = monster_archive::mesh(&entry, id).unwrap() else {
            continue;
        };
        let Ok(tmd) = legaia_tmd::parse(mesh.tmd_bytes()) else {
            continue;
        };
        total_mesh += 1;
        let nobj = tmd.objects.len();
        let anims = monster_archive::animations(&entry, id).unwrap().unwrap();
        if !anims.is_empty() {
            with_anim += 1;
        }
        for a in &anims {
            action_total += 1;
            if a.part_count == nobj {
                part_match += 1;
            }
        }
    }
    assert!(total_mesh > 100, "expected >100 meshes, got {total_mesh}");
    assert!(
        with_anim as f64 / total_mesh as f64 > 0.95,
        "expected >95% of meshes to carry action animations, got {with_anim}/{total_mesh}"
    );
    assert!(
        part_match as f64 / action_total as f64 > 0.98,
        "expected >98% of actions to animate one part per TMD object, got {part_match}/{action_total}"
    );
}

#[test]
fn battle_render_mesh_injects_pool_and_relocates_cba_tsb() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };
    use legaia_tim::Vram;

    // Render Gimard (id 10) into battle slot 2: inject its texture pool into a
    // fresh VRAM at the loader's per-slot coords and relocate every prim's
    // CBA/TSB to match. This is the bridge the native battle renderer uses.
    let slot = 2u8;
    let mesh = monster_archive::mesh(&entry, 10).unwrap().unwrap();
    let mut vram = Vram::new();
    let vmesh = mesh
        .battle_render_mesh(slot, &mut vram)
        .expect("Gimard renders");

    assert!(!vmesh.indices.is_empty(), "Gimard has textured prims");
    assert_eq!(vmesh.cba_tsb.len(), vmesh.positions.len());

    // Every relocated CBA points at the slot's CLUT row; every relocated TSB
    // at the slot's 4bpp page column with tpage_y=256.
    let (page_x, page_y) = monster_archive::monster_page_origin(slot);
    for ct in &vmesh.cba_tsb {
        assert_eq!(
            (ct[0] >> 6) & 0x1FF,
            monster_archive::MONSTER_CLUT_ROW_BASE + slot as u16,
            "CBA relocated to CLUT row 484 + slot"
        );
        assert_eq!((ct[1] & 0xF) * 64, page_x, "TSB page x = (5+slot)*64");
        assert_eq!((ct[1] >> 4) & 1, 1, "tpage_y = 256");
        assert_eq!((ct[1] >> 7) & 0x3, 0, "4bpp depth");
    }

    // The injection actually populated the loader's VRAM regions (Gimard is a
    // 128-texel = 32-cell-wide page) and the CLUT row.
    assert!(
        vram.region_has_data(page_x as usize, page_y as usize, 32, 256),
        "monster texture page injected at the slot's VRAM coords"
    );
    assert!(
        vram.region_has_data(
            0,
            (monster_archive::MONSTER_CLUT_ROW_BASE + slot as u16) as usize,
            240,
            1
        ),
        "monster CLUT region injected at row 484 + slot"
    );

    // The relocated prims now resolve against populated VRAM: sample the first
    // triangle's CBA/TSB + UVs through the same predicate the targeted VRAM
    // builder uses to decide a prim is renderable.
    let tri = [
        vmesh.indices[0] as usize,
        vmesh.indices[1] as usize,
        vmesh.indices[2] as usize,
    ];
    let uvs: Vec<(u8, u8)> = tri
        .iter()
        .map(|&i| (vmesh.uvs[i][0], vmesh.uvs[i][1]))
        .collect();
    let cba = vmesh.cba_tsb[tri[0]][0];
    let tsb = vmesh.cba_tsb[tri[0]][1];
    assert!(
        vram.prim_has_texture_data(cba, tsb, &uvs),
        "a relocated prim samples populated VRAM after injection"
    );
}
