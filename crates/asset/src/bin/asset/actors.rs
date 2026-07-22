use std::path::{Path, PathBuf};

use crate::common::*;
use anyhow::{Context, Result};
use legaia_prot::cdname;

pub(crate) fn character_pack_one(
    input: &Path,
    slot: Option<usize>,
    equip: Option<u8>,
    out: Option<&Path>,
) -> Result<()> {
    use legaia_asset::character_pack;
    let bytes = std::fs::read(input)
        .with_context(|| format!("read PROT 874 entry from {}", input.display()))?;
    let pack = character_pack::parse(&bytes)?;
    let active_patches = character_pack::equipment_swap::ACTIVE_PARTY_SLOTS;

    let print_slot = |s: &character_pack::CharacterSlot| {
        let label = character_pack::slot_label(s.slot);
        let patch = active_patches.iter().find(|p| (p.slot as usize) == s.slot);
        let patch_note = match patch {
            Some(p) => format!(
                "patched group {} @ record byte +0x{:03X}",
                p.patched_group_index, p.equip_byte_record_offset
            ),
            None => "auxiliary (no equipment swap)".to_string(),
        };
        println!(
            "  slot {} ({:<5}) disc-nobj {:2}  TMD bytes {:6}  {}",
            s.slot,
            label,
            s.disc_nobj,
            s.tmd_bytes.len(),
            patch_note,
        );
    };

    if let Some(idx) = slot {
        let slot = pack
            .slot(idx)
            .ok_or_else(|| anyhow::anyhow!("slot {idx} out of range (0..=4)"))?;
        print_slot(slot);
        if let Some(equip_byte) = equip {
            let Some(patch) = active_patches
                .iter()
                .find(|p| (p.slot as usize) == slot.slot)
            else {
                anyhow::bail!(
                    "slot {} ({}) is not an active-party slot; equipment swap only applies to 0..=2",
                    slot.slot,
                    character_pack::slot_label(slot.slot)
                );
            };
            let patched =
                character_pack::equipment_swap::apply(&slot.tmd_bytes, *patch, equip_byte);
            let template = if equip_byte == 0 { 11 } else { 10 };
            println!(
                "  applied swap: equip byte 0x{:02X} -> group-{} template overwrites visible group {}",
                equip_byte, template, patch.patched_group_index
            );
            if let Some(out_path) = out {
                std::fs::write(out_path, &patched)?;
                println!("  wrote patched TMD -> {}", out_path.display());
            }
        } else if let Some(out_path) = out {
            std::fs::write(out_path, &slot.tmd_bytes)?;
            println!("  wrote raw disc TMD -> {}", out_path.display());
        }
    } else {
        if equip.is_some() {
            anyhow::bail!("--equip requires --slot <N>");
        }
        if out.is_some() {
            anyhow::bail!("--out requires --slot <N>");
        }
        println!(
            "PROT {} (befect_data §0): {} character slots",
            character_pack::PROT_ENTRY_INDEX,
            pack.slots().len()
        );
        for s in pack.slots() {
            print_slot(s);
        }
    }
    Ok(())
}

pub(crate) fn field_char_tex_one(
    input: &Path,
    entry: Option<usize>,
    out_tim: Option<&Path>,
) -> Result<()> {
    use legaia_asset::field_char_textures;
    let bytes = std::fs::read(input)
        .with_context(|| format!("read PROT 874 entry from {}", input.display()))?;
    let pack = field_char_textures::parse(&bytes)?;

    println!(
        "PROT {} (player.lzs §2): {} field-texture TIM entries",
        field_char_textures::PROT_ENTRY_INDEX,
        pack.textures.len()
    );
    let role = |i: usize| match i {
        1 => "Vahn atlas page (CLUT cols 0..63)",
        2 => "Noa atlas page (CLUT cols 64..127)",
        3 => "Gala atlas page (CLUT cols 128..191)",
        6 | 7 => "atlas extension (lower)",
        _ => "shared / auxiliary page",
    };
    for t in &pack.textures {
        let img = &t.tim.image;
        let clut = t.tim.clut.as_ref();
        let (cx, cy, cn) = clut.map_or((0, 0, 0), |c| (c.fb_x, c.fb_y, c.entries.len()));
        println!(
            "  entry {} img=({:>3},{:>3}) {:>3}w x {:>3}h  clut=({:>3},{:>3}) {:>3}col  {}",
            t.index,
            img.fb_x,
            img.fb_y,
            img.fb_w,
            img.h,
            cx,
            cy,
            cn,
            role(t.index),
        );
    }

    if let Some(idx) = entry {
        let t = pack
            .textures
            .get(idx)
            .ok_or_else(|| anyhow::anyhow!("entry {idx} out of range (0..=7)"))?;
        if let Some(out_path) = out_tim {
            // Re-extract the raw TIM bytes by re-walking the pack (the parsed
            // `Tim` is lossy on exact block padding; the raw slice is exact).
            let container =
                legaia_asset::parse_player_lzs(&bytes, field_char_textures::CONTAINER_DESCRIPTORS)?;
            let section = &container.descriptors[field_char_textures::CONTAINER_SECTION];
            let decoded = legaia_asset::decode(&bytes, section, legaia_asset::DecodeMode::Lzs)?;
            let bodies = legaia_asset::pack::extract_pack(&decoded)?;
            std::fs::write(out_path, bodies[idx])?;
            println!("  wrote entry {idx} TIM -> {}", out_path.display());
        } else {
            anyhow::bail!("--entry requires --out-tim <PATH>");
        }
        let _ = t;
    } else if out_tim.is_some() {
        anyhow::bail!("--out-tim requires --entry <N>");
    }
    Ok(())
}

pub(crate) fn battle_char_pack_one(
    input: &Path,
    slot: Option<usize>,
    out_tmd: Option<&Path>,
    atlas: Option<usize>,
    out_tim: Option<&Path>,
) -> Result<()> {
    use legaia_asset::battle_char_pack;
    let bytes = std::fs::read(input).with_context(|| format!("read {}", input.display()))?;
    let pack = battle_char_pack::parse(&bytes)?;
    let print_slot = |s: &battle_char_pack::BattleCharSlot| {
        let label = battle_char_pack::slot_label(s.slot);
        println!(
            "  slot {} ({:<7}) disc-nobj {:2}  TMD bytes {:6}  file offset 0x{:06X}",
            s.slot,
            label,
            s.disc_nobj,
            s.tmd_bytes.len(),
            s.file_offset
        );
    };
    let print_atlas = |a: &battle_char_pack::BattleCharAtlas| {
        println!(
            "  atlas {}  CLUT fb_y={:3}  file offset 0x{:06X}  {} bytes",
            a.atlas_index,
            a.clut_fb_y,
            a.file_offset,
            a.tim_bytes.len()
        );
    };
    if let Some(s_idx) = slot {
        let s = pack
            .slot(s_idx)
            .ok_or_else(|| anyhow::anyhow!("slot {s_idx} out of range (0..=4)"))?;
        print_slot(s);
        if let Some(p) = out_tmd {
            std::fs::write(p, &s.tmd_bytes).with_context(|| format!("write {}", p.display()))?;
            println!(
                "  wrote raw disc TMD ({}) -> {}",
                battle_char_pack::slot_label(s.slot),
                p.display()
            );
        }
    } else if atlas.is_none() && out_tim.is_none() {
        println!(
            "PROT {} (other5, battle character pack): {} slots + {} atlases",
            battle_char_pack::PROT_ENTRY_INDEX,
            pack.slots().len(),
            pack.atlases.len()
        );
        for s in pack.slots() {
            print_slot(s);
        }
        for a in &pack.atlases {
            print_atlas(a);
        }
    }
    if let Some(a_idx) = atlas {
        let a = pack
            .atlas(a_idx)
            .ok_or_else(|| anyhow::anyhow!("atlas {a_idx} out of range (0..=6)"))?;
        print_atlas(a);
        if let Some(p) = out_tim {
            std::fs::write(p, &a.tim_bytes).with_context(|| format!("write {}", p.display()))?;
            println!("  wrote raw atlas {} TIM -> {}", a.atlas_index, p.display());
        }
    }
    Ok(())
}

pub(crate) fn player_anm_one(input: &Path, desc_count: usize, out: Option<&Path>) -> Result<()> {
    use legaia_asset::player_anm;
    let bytes = std::fs::read(input).with_context(|| format!("read {}", input.display()))?;
    let bundles = player_anm::find_in_entry(&bytes, desc_count);
    if bundles.is_empty() {
        println!(
            "no player-ANM bundles found in {} (desc_count={}; try 3 / 5 / 7)",
            input.display(),
            desc_count
        );
        return Ok(());
    }
    let entry_stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "entry".into());
    println!(
        "{}: {} player-ANM bundle(s)",
        input.display(),
        bundles.len()
    );
    for (i, b) in bundles.iter().enumerate() {
        let r0 = b.record_marker_1(0).unwrap_or(0);
        println!(
            "  bundle {i}: count={}  decoded={} bytes  record0 marker_1=0x{r0:04X}",
            b.record_count,
            b.decoded.len()
        );
        if let Some(out_dir) = out {
            std::fs::create_dir_all(out_dir)
                .with_context(|| format!("create_dir_all {}", out_dir.display()))?;
            let p = out_dir.join(format!("{entry_stem}_sect{i}.anm"));
            std::fs::write(&p, &b.decoded).with_context(|| format!("write {}", p.display()))?;
            println!("    wrote {} ({} bytes)", p.display(), b.decoded.len());
        }
    }
    Ok(())
}

pub(crate) fn player_anm_scan(
    dir: &Path,
    cdname_path: Option<&Path>,
    desc_count: usize,
) -> Result<()> {
    use legaia_asset::player_anm;
    let cdname = cdname_path
        .map(|p| std::fs::read_to_string(p).with_context(|| format!("read CDNAME {}", p.display())))
        .transpose()?;
    let cdname_map: std::collections::HashMap<u32, String> =
        cdname.as_deref().map(parse_cdname_text).unwrap_or_default();

    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("read_dir {}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "BIN"))
        .collect();
    entries.sort();

    let mut total = 0usize;
    for path in &entries {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let bundles = player_anm::find_in_entry(&bytes, desc_count);
        if bundles.is_empty() {
            continue;
        }
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        // PROT index: parse the 4-digit prefix.
        let prot_idx: u32 = name
            .split('_')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let label = cdname_map.get(&prot_idx).cloned().unwrap_or_default();
        for (i, b) in bundles.iter().enumerate() {
            total += 1;
            println!(
                "  {name:32} bundle {i}: count={:3}  decoded={:6} bytes  {}",
                b.record_count,
                b.decoded.len(),
                label
            );
        }
    }
    println!(
        "\n{total} player-ANM bundle(s) across {} entries",
        entries.len()
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn monster_archive_one(
    input: &Path,
    id: Option<u16>,
    obj: Option<&Path>,
    texture_png: Option<&Path>,
    palette: Option<usize>,
    anim: bool,
    glb: Option<&Path>,
    dump_block: Option<&Path>,
    write_block: Option<&Path>,
) -> Result<()> {
    use legaia_asset::monster_archive;
    let bytes = crate::common::read_input(input)?;
    println!(
        "monster archive: {} bytes, {} slots of 0x{:X}",
        bytes.len(),
        monster_archive::slot_count(&bytes),
        monster_archive::SLOT_STRIDE
    );
    let print_rec = |r: &monster_archive::MonsterRecord| {
        println!(
            "  id {:3}  {:<22} HP {:5}  MP {:5}  stats {:?}  magic {}  \
             gold {:5}  exp {:5}  drop {:3}@{:3}%",
            r.id,
            r.name,
            r.hp,
            r.mp,
            r.stats,
            r.magic_count,
            r.gold,
            r.exp,
            r.drop_item,
            r.drop_chance_pct
        );
        if !r.spells.is_empty() {
            let spells: Vec<String> = r
                .spells
                .iter()
                .map(|s| {
                    let cost = if s.agl_cost == 0xFF {
                        "--".to_string()
                    } else {
                        s.agl_cost.to_string()
                    };
                    format!("0x{:02X}@{}", s.id, cost)
                })
                .collect();
            println!("        spells: {}", spells.join(" "));
        }
    };
    match id {
        Some(id) => match monster_archive::record(&bytes, id)? {
            Some(r) => print_rec(&r),
            None => println!("  id {id}: no record (out of range / filler slot)"),
        },
        None => {
            let recs = monster_archive::records(&bytes)?;
            println!("populated records: {}", recs.len());
            for r in &recs {
                print_rec(r);
            }
        }
    }
    if let Some(obj_path) = obj {
        let Some(id) = id else {
            anyhow::bail!("--obj requires --id <N>");
        };
        match monster_archive::mesh(&bytes, id)? {
            Some(m) => {
                let tmd = legaia_tmd::parse(m.tmd_bytes())?;
                let s = monster_mesh_to_obj(&tmd, m.tmd_bytes(), id);
                std::fs::write(obj_path, s)?;
                let st = tmd.stats();
                println!(
                    "  wrote mesh OBJ -> {} (TMD @ block+0x{:x}: {} verts, {} prims)",
                    obj_path.display(),
                    m.tmd_offset,
                    st.total_vertices,
                    st.total_primitives,
                );
            }
            None => println!("  id {id}: no mesh (out of range / filler / no TMD at +0x04)"),
        }
    }
    if let Some(png_path) = texture_png {
        let Some(id) = id else {
            anyhow::bail!("--texture-png requires --id <N>");
        };
        match monster_archive::mesh(&bytes, id)? {
            Some(m) => match m.texture() {
                Some(tex) => {
                    // Default to the palette the mesh's first textured prim
                    // samples (cba & 0x3F), so the page shows in real colours.
                    let pal = palette.unwrap_or_else(|| first_prim_palette(&m).unwrap_or(0));
                    let rgba = tex.to_rgba(pal);
                    write_rgba_png(png_path, tex.width as u32, tex.height as u32, &rgba)?;
                    println!(
                        "  wrote texture PNG -> {} ({}x{}, palette {}, {} palettes)",
                        png_path.display(),
                        tex.width,
                        tex.height,
                        pal,
                        tex.palettes.len(),
                    );
                }
                None => println!("  id {id}: no texture pool"),
            },
            None => println!("  id {id}: no mesh / texture (filler slot)"),
        }
    }
    if anim {
        let Some(id) = id else {
            anyhow::bail!("--anim requires --id <N>");
        };
        match monster_archive::animations(&bytes, id)? {
            Some(anims) if !anims.is_empty() => {
                println!("  action animations: {}", anims.len());
                for (i, a) in anims.iter().enumerate() {
                    // Per-part motion range over the whole animation, so the
                    // idle (index 0) is easy to eyeball vs the big move actions.
                    let (mut max_t, mut max_r) = (0i32, 0u16);
                    for f in &a.frames {
                        for p in f {
                            max_t = max_t
                                .max((p.tx as i32).abs())
                                .max((p.ty as i32).abs())
                                .max((p.tz as i32).abs());
                            max_r = max_r.max(p.rx).max(p.ry).max(p.rz);
                        }
                    }
                    println!(
                        "    [{i}] action 0x{:02X}{}  parts {:2}  frames {:3}  max|trans| {:5}  max rot {:5} (4096=turn)",
                        a.action_id,
                        if i == 0 { " (idle)" } else { "       " },
                        a.part_count,
                        a.frame_count,
                        max_t,
                        max_r,
                    );
                }
            }
            _ => println!("  id {id}: no action animations (filler slot / no mesh)"),
        }
    }
    if let Some(glb_path) = glb {
        let Some(id) = id else {
            anyhow::bail!("--glb requires --id <N>");
        };
        match legaia_asset::monster_gltf::export_glb(&bytes, id)? {
            Some(glb) => {
                std::fs::write(glb_path, &glb)?;
                println!(
                    "  wrote glTF -> {} ({} bytes, mesh + texture + animations)",
                    glb_path.display(),
                    glb.len(),
                );
            }
            None => println!("  id {id}: no exportable mesh (out of range / filler slot)"),
        }
    }
    if let Some(block_path) = dump_block {
        let Some(id) = id else {
            anyhow::bail!("--dump-block requires --id <N>");
        };
        match monster_archive::decode_block(&bytes, id)? {
            Some(block) => {
                std::fs::write(block_path, &block)?;
                println!(
                    "  wrote decoded block -> {} ({} bytes; stat record head at +0x00, \
                     element byte at +0x1D)",
                    block_path.display(),
                    block.len(),
                );
            }
            None => println!("  id {id}: no block (out of range / filler slot)"),
        }
    }
    if let Some(block_path) = write_block {
        let Some(id) = id else {
            anyhow::bail!("--write-block requires --id <N>");
        };
        let block = std::fs::read(block_path)?;
        let slot = monster_archive::encode_slot(&block)?;
        let off = (id as usize)
            .checked_sub(1)
            .map(|n| n * monster_archive::SLOT_STRIDE)
            .ok_or_else(|| anyhow::anyhow!("monster ids are 1-based"))?;
        if bytes.len() < off + slot.len() {
            anyhow::bail!("archive too small for id {id}'s slot");
        }
        let mut out = bytes.clone();
        out[off..off + slot.len()].copy_from_slice(&slot);
        std::fs::write(input, &out)?;
        println!(
            "  re-packed id {id} ({} bytes decoded) into its slot and rewrote {} in place",
            block.len(),
            input.display(),
        );
    }
    Ok(())
}

/// The palette index (`cba & 0x3F`) of the first textured primitive in the
/// monster's mesh, so a baked texture PNG uses the colours that prim expects.
pub(crate) fn first_prim_palette(m: &legaia_asset::monster_archive::MonsterMesh) -> Option<usize> {
    let tbuf = m.tmd_bytes();
    let tmd = legaia_tmd::parse(tbuf).ok()?;
    let vm = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, tbuf);
    vm.cba_tsb
        .iter()
        .find(|ct| ct[0] != 0)
        .map(|ct| (ct[0] & 0x3F) as usize)
}

/// Wavefront OBJ string for a monster's embedded TMD: all objects' vertices
/// concatenated, faces triangulated via the shared mesh builder.
pub(crate) fn monster_mesh_to_obj(tmd: &legaia_tmd::Tmd, buf: &[u8], id: u16) -> String {
    let mesh = legaia_tmd::mesh::tmd_to_mesh(tmd, buf);
    let mut s = format!(
        "# Legend of Legaia monster mesh (PROT 867 archive, id {id})\n# {} verts, {} tris\n",
        mesh.positions.len(),
        mesh.triangle_count(),
    );
    for p in &mesh.positions {
        s.push_str(&format!("v {} {} {}\n", p[0], p[1], p[2]));
    }
    // OBJ vertex indices are 1-based.
    for tri in mesh.indices.chunks_exact(3) {
        s.push_str(&format!("f {} {} {}\n", tri[0] + 1, tri[1] + 1, tri[2] + 1));
    }
    s
}

pub(crate) fn man_one(
    input: &Path,
    with_encounter: bool,
    max_formations: usize,
    max_regions: usize,
) -> Result<()> {
    let buf = crate::common::read_input(input)?;
    let (man_bytes, desc) = load_man_bytes(&buf)?;
    let man = legaia_asset::man_section::parse(&man_bytes)
        .map_err(|e| anyhow::anyhow!("MAN parse: {e}"))?;

    println!(
        "MAN @ {} (LZS in→out: {}→{})",
        input.display(),
        desc.size,
        man_bytes.len()
    );
    println!(
        "  status_flags    : 0x{:04X} (low_flag={}, world_map_bulk_terrain={})",
        man.header.status_flags,
        man.header.low_flag,
        man.header.world_map_bulk_terrain(),
    );
    print!("  depth_lut[16]   :");
    for v in man.header.depth_lut {
        print!(" {:>5}", v);
    }
    println!();
    println!(
        "  partitions      : N0={} N1={} N2={} (total {} records, 3-byte each)",
        man.header.partition_counts[0],
        man.header.partition_counts[1],
        man.header.partition_counts[2],
        man.header.total_records(),
    );
    println!(
        "  u24[0x28]       : 0x{:06X}  (section-0 byte offset into data region)",
        man.header.u24_at_28
    );
    println!("  data region @ 0x{:X}", man.data_region_offset);
    println!();
    println!("sections:");
    for (i, s) in man.sections.iter().enumerate() {
        let tag = match i {
            0 => " (encounter, ctrl[+0x20])",
            1 => " (ctrl[+0x00])",
            2 => " (_DAT_801C6EA0)",
            3 => " (ctrl[+0x04])",
            4 => " (DAT_80073ED8)",
            5 => " (terminator, DAT_80073EE0)",
            _ => "",
        };
        println!(
            "  [{}] @ 0x{:06X}  len=0x{:06X}  body=0x{:06X}..0x{:06X}{}",
            i,
            s.offset,
            s.length,
            s.body_offset(),
            s.end_offset(),
            tag,
        );
    }

    if with_encounter {
        println!();
        let body = man
            .encounter_section_body(&man_bytes)
            .ok_or_else(|| anyhow::anyhow!("encounter section body out of range"))?;
        let es = legaia_asset::man_section::parse_encounter_section(body)
            .map_err(|e| anyhow::anyhow!("encounter-section parse: {e}"))?;
        println!("encounter section (FUN_8003A110):");
        println!(
            "  strides: formation={} condition={} region={}",
            es.formation_stride, es.condition_stride, es.region_stride
        );
        println!(
            "  counts:  formation={} condition={} region={}  (uses {}/{} body bytes)",
            es.formation_count,
            es.condition_count,
            es.region_count,
            es.total_bytes(),
            body.len(),
        );

        let f_take = (es.formation_count as usize).min(max_formations);
        println!("  formations [{}/{}]", f_take, es.formation_count);
        for (i, f) in legaia_asset::man_section::formation_records(body, &es)
            .take(f_take)
            .enumerate()
        {
            match f {
                Some(f) => println!(
                    "    [{:3}] count={} ids=[{:>3}, {:>3}, {:>3}, {:>3}] hdr=[{:02X}, {:02X}, {:02X}] pad={}b",
                    i,
                    f.monster_count,
                    f.monster_ids[0],
                    f.monster_ids[1],
                    f.monster_ids[2],
                    f.monster_ids[3],
                    f.header_bytes[0],
                    f.header_bytes[1],
                    f.header_bytes[2],
                    f.trailing_byte_count,
                ),
                None => println!("    [{:3}] (malformed)", i),
            }
        }

        let r_take = (es.region_count as usize).min(max_regions);
        println!("  regions [{}/{}]", r_take, es.region_count);
        for (i, r) in legaia_asset::man_section::region_records(body, &es)
            .take(r_take)
            .enumerate()
        {
            match r {
                Some(r) => println!(
                    "    [{:3}] aabb=({:3},{:3})..({:3},{:3}) rate+={} formations=[{}..+{})",
                    i,
                    r.aabb_x_min,
                    r.aabb_y_min,
                    r.aabb_x_max,
                    r.aabb_y_max,
                    r.rate_increment,
                    r.formation_range_base,
                    r.formation_range_count,
                ),
                None => println!("    [{:3}] (malformed)", i),
            }
        }
    }
    Ok(())
}

pub(crate) fn man_scan(dir: &Path, cdname_path: Option<&Path>, json: bool) -> Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };

    #[derive(serde::Serialize)]
    struct ManScanEntry {
        entry: String,
        partition_counts: [i16; 3],
        section_lengths: [u32; 5],
        encounter_offset: usize,
        encounter_formations: Option<u8>,
        encounter_regions: Option<u8>,
        status_flags: u16,
    }

    let mut results: Vec<ManScanEntry> = Vec::new();

    for path in &entries {
        let Ok(buf) = std::fs::read(path) else {
            continue;
        };
        let Ok((man_bytes, _)) = load_man_bytes(&buf) else {
            continue;
        };
        let Ok(man) = legaia_asset::man_section::parse(&man_bytes) else {
            continue;
        };
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let display = display_name_for(&stem, names.as_ref());

        let (f_n, r_n) = match man.encounter_section_body(&man_bytes) {
            Some(body) => match legaia_asset::man_section::parse_encounter_section(body) {
                Ok(es) => (Some(es.formation_count), Some(es.region_count)),
                Err(_) => (None, None),
            },
            None => (None, None),
        };

        results.push(ManScanEntry {
            entry: display,
            partition_counts: man.header.partition_counts,
            section_lengths: [
                man.sections[0].length,
                man.sections[1].length,
                man.sections[2].length,
                man.sections[3].length,
                man.sections[4].length,
            ],
            encounter_offset: man.sections[0].offset,
            encounter_formations: f_n,
            encounter_regions: r_n,
            status_flags: man.header.status_flags,
        });
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        println!(
            "{:<28}  {:>3} {:>3} {:>3}  enc@      s0      s1     s2     s3     s4   forms regs  flags",
            "entry", "N0", "N1", "N2"
        );
        println!("{}", "-".repeat(110));
        for r in &results {
            println!(
                "{:<28}  {:>3} {:>3} {:>3}  {:>6X}  {:>5X}  {:>5X}  {:>5X}  {:>5X}  {:>5X}  {:>4}  {:>3}  0x{:04X}",
                r.entry,
                r.partition_counts[0],
                r.partition_counts[1],
                r.partition_counts[2],
                r.encounter_offset,
                r.section_lengths[0],
                r.section_lengths[1],
                r.section_lengths[2],
                r.section_lengths[3],
                r.section_lengths[4],
                r.encounter_formations.map(|n| n as i32).unwrap_or(-1),
                r.encounter_regions.map(|n| n as i32).unwrap_or(-1),
                r.status_flags,
            );
        }
        println!();
        println!("{} scenes with a parseable MAN", results.len());
    }
    Ok(())
}

/// `asset befect-cluster`: cluster-aware extraction of the `befect_data`
/// battle-effect cluster (footprint-bounded entries, LZS-container expansion,
/// per-part content classification).
pub(crate) fn befect_cluster_cmd(
    prot: &Path,
    cdname_path: &Path,
    out: Option<&Path>,
    json: bool,
) -> Result<()> {
    use legaia_asset::befect_cluster::{self, Component};
    use legaia_prot::archive::Archive;

    let mut archive = Archive::open(prot)?;
    let names = cdname::parse(cdname_path)?;
    let cluster = befect_cluster::extract(&mut archive, &names)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&cluster)?);
    } else {
        println!(
            "befect_data cluster: first PROT entry {}, {} parts",
            cluster.first_index,
            cluster.parts.len()
        );
        for p in &cluster.parts {
            let src = match p.lzs_section {
                Some(i) => format!("entry {} / lzs section {}", p.prot_index, i),
                None => format!("entry {}", p.prot_index),
            };
            let desc = match &p.component {
                Component::EffectScript2Pack {
                    atlas_entries,
                    anim_batches,
                    scripts,
                } => format!(
                    "efect.dat 2-pack: {atlas_entries} atlas entries, {anim_batches} anim batches, {scripts} scripts"
                ),
                Component::TmdPack { count } => format!("TMD pack: {count} effect models"),
                Component::TimImages { tims } => {
                    let mut s = format!("{} effect-texture TIM(s):", tims.len());
                    for t in tims {
                        let clut = t
                            .clut_fb
                            .map(|(x, y)| format!(" clut@({x},{y})"))
                            .unwrap_or_default();
                        s.push_str(&format!(
                            "\n        @0x{:x} {}bpp pix@fb({},{}) {}x{}hw{}",
                            t.offset, t.bpp, t.fb_x, t.fb_y, t.w_hw, t.h, clut
                        ));
                    }
                    s
                }
                Component::OffsetPack { count } => format!("offset pack: {count} entries"),
                Component::Raw => "raw / unclassified".to_string(),
            };
            println!("  [{src}] {} bytes  {desc}", p.len);
        }
    }

    if let Some(dir) = out {
        std::fs::create_dir_all(dir)?;
        for p in &cluster.parts {
            let tag = match &p.component {
                Component::EffectScript2Pack { .. } => "efect_2pack",
                Component::TmdPack { .. } => "effect_tmds",
                Component::TimImages { .. } => "effect_tims",
                Component::OffsetPack { .. } => "offset_pack",
                Component::Raw => "raw",
            };
            let name = match p.lzs_section {
                Some(i) => format!("{:04}_s{}_{}.bin", p.prot_index, i, tag),
                None => format!("{:04}_{}.bin", p.prot_index, tag),
            };
            std::fs::write(dir.join(&name), &p.data)?;
        }
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&cluster)?,
        )?;
        println!(
            "wrote {} parts + manifest.json to {}",
            cluster.parts.len(),
            dir.display()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `asset overlay` - static overlay-extraction pipeline.
// ---------------------------------------------------------------------------
