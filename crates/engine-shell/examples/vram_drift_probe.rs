//! One-off probe for a VRAM static-mask divergence: dumps every static-mask
//! word where the engine pre-pass disagrees with retail for one scene, plus a
//! window around the first divergence, so the offending upload can be
//! identified. Usage:
//!
//!   cargo run --release -p legaia-engine-shell --example vram_drift_probe -- <scene>
//!
//! Needs `extracted/` + `scripts/scenarios.toml` + `saves/library`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_shell::vram_oracle::{
    NPC_CLUT_BAND_ROWS, TEXPAGE_Y_START, VRAM_HEIGHT, VRAM_WIDTH, build_engine_vram_bytes_prepass,
    compute_static_mask, load_runtime_vram_from_save,
};
use legaia_mednafen::ScenarioManifest;

fn word(buf: &[u8], x: usize, y: usize) -> u16 {
    let off = (y * VRAM_WIDTH + x) * 2;
    u16::from_le_bytes([buf[off], buf[off + 1]])
}

fn vram_to_words_at(vram: &legaia_tim::Vram, x: usize, y: usize) -> u16 {
    let bytes = legaia_engine_shell::vram_oracle::vram_to_le_bytes(vram);
    word(&bytes, x, y)
}

fn main() -> anyhow::Result<()> {
    let scene_name = std::env::args().nth(1).unwrap_or_else(|| "map01".into());
    let extracted = PathBuf::from("extracted");
    let manifest = ScenarioManifest::from_path("scripts/scenarios.toml")?;
    let library = PathBuf::from("saves/library");

    let mut by_scene: BTreeMap<String, Vec<(String, PathBuf)>> = BTreeMap::new();
    for scn in &manifest.scenarios {
        let Some(scene) = scn.expected_active_scene.as_deref() else {
            continue;
        };
        let Ok(save_path) = manifest.mednafen_save_path(scn, Some(library.as_path())) else {
            continue;
        };
        if !save_path.exists() {
            continue;
        }
        by_scene
            .entry(scene.to_owned())
            .or_default()
            .push((scn.label.clone(), save_path));
    }
    let captures = by_scene
        .get(&scene_name)
        .ok_or_else(|| anyhow::anyhow!("no captures for scene {scene_name}"))?;
    println!(
        "scene {scene_name}: {} captures: {:?}",
        captures.len(),
        captures.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>()
    );

    let engine = build_engine_vram_bytes_prepass(&scene_name, &extracted, None)?;
    let runtimes: Vec<Vec<u8>> = captures
        .iter()
        .map(|(_, p)| load_runtime_vram_from_save(p))
        .collect::<Result<_, _>>()?;
    let refs: Vec<&[u8]> = runtimes.iter().map(|v| v.as_slice()).collect();
    let mask = compute_static_mask(&refs);

    // Every static-mask divergence where the engine uploaded a non-zero word.
    let mut divergences = Vec::new();
    for y in TEXPAGE_Y_START..VRAM_HEIGHT {
        if NPC_CLUT_BAND_ROWS.contains(&y) {
            continue;
        }
        for x in 0..VRAM_WIDTH {
            if !mask[y * VRAM_WIDTH + x] {
                continue;
            }
            let e = word(&engine, x, y);
            let r = word(refs[0], x, y);
            if e != 0 && e != r {
                divergences.push((x, y, e, r));
            }
        }
    }
    println!(
        "{} static-mask divergences (engine non-zero)",
        divergences.len()
    );

    // Optional cell query: `vram_drift_probe <scene> <x> <y>` prints the word
    // at that cell in the engine build and each capture, plus mask membership.
    if let (Some(qx), Some(qy)) = (
        std::env::args()
            .nth(2)
            .and_then(|s| s.parse::<usize>().ok()),
        std::env::args()
            .nth(3)
            .and_then(|s| s.parse::<usize>().ok()),
    ) {
        // Row-dump mode: x == 9999 dumps words 0..48 of row y.
        if qx == 9999 {
            let dump = |buf: &[u8], tag: &str| {
                let words: Vec<String> = (0..48)
                    .map(|x| format!("{:04X}", word(buf, x, qy)))
                    .collect();
                println!("{tag:<28} {}", words.join(" "));
            };
            dump(&engine, "engine");
            for ((label, _), rt) in captures.iter().zip(&refs) {
                dump(rt, label);
            }
            let msk: String = (0..48)
                .map(|x| if mask[qy * VRAM_WIDTH + x] { 'S' } else { '.' })
                .collect();
            println!("{:<28} {msk}", "mask");
            return Ok(());
        }
        println!(
            "cell ({qx},{qy}): engine=0x{:04X} static={}",
            word(&engine, qx, qy),
            mask[qy * VRAM_WIDTH + qx]
        );
        for ((label, _), rt) in captures.iter().zip(&refs) {
            println!("  {label:<32} 0x{:04X}", word(rt, qx, qy));
        }

        // Which engine TIM covers the queried cell (image or CLUT rect)?
        use legaia_engine_core::scene::Scene;
        use legaia_engine_core::scene_resources::FIELD_SHARED_BLOCKS;
        let prot = std::fs::read(extracted.join("PROT.DAT"))?;
        let cdname = std::fs::read_to_string(extracted.join("CDNAME.TXT"))?;
        let index = legaia_engine_core::scene::ProtIndex::from_bytes(prot, Some(&cdname))?;
        let mut scenes: Vec<(String, Scene)> =
            vec![(scene_name.clone(), Scene::load(&index, &scene_name)?)];
        for name in FIELD_SHARED_BLOCKS {
            if let Ok(s) = Scene::load(&index, name) {
                scenes.push(((*name).to_string(), s));
            }
        }
        for (sname, s) in &scenes {
            for entry in &s.entries {
                let bytes: &[u8] = &entry.bytes;
                let report = |tag: &str, payload: &[u8], off: usize| {
                    if let Ok(tim) = legaia_tim::parse(payload) {
                        let img = &tim.image;
                        let img_cov = (img.fb_x as usize..img.fb_x as usize + img.fb_w as usize)
                            .contains(&qx)
                            && (img.fb_y as usize..img.fb_y as usize + img.h as usize)
                                .contains(&qy);
                        let clut_cov = tim.clut.as_ref().is_some_and(|c| {
                            (c.fb_x as usize..c.fb_x as usize + c.w as usize).contains(&qx)
                                && (c.fb_y as usize..c.fb_y as usize + c.h as usize).contains(&qy)
                        });
                        if img_cov || clut_cov {
                            println!(
                                "  {sname} entry {} {tag}@0x{off:05x}: img ({},{}) {}x{} clut {:?} covers: img={img_cov} clut={clut_cov}",
                                entry.idx,
                                img.fb_x,
                                img.fb_y,
                                img.fb_w,
                                img.h,
                                tim.clut.as_ref().map(|c| (c.fb_x, c.fb_y, c.w, c.h)),
                            );
                        }
                    }
                };
                if let Ok(slot0) = legaia_asset::kingdom_bundle::decode_slot(bytes, 0)
                    && let Ok(tim_slices) = legaia_asset::pack::extract_pack(&slot0)
                {
                    for (i, tslice) in tim_slices.iter().enumerate() {
                        report(&format!("kingdom-slot0[{i}]"), tslice, 0);
                    }
                }
                let scan = legaia_asset::tim_scan::scan_entry(bytes);
                for (source, hit) in &scan.hits {
                    let src: &[u8] = match source {
                        legaia_asset::tim_scan::Source::Raw => bytes,
                        legaia_asset::tim_scan::Source::Lzs(idx) => {
                            scan.lzs_sections[*idx].as_slice()
                        }
                    };
                    if hit.offset + hit.byte_len <= src.len() {
                        report(
                            "scan",
                            &src[hit.offset..hit.offset + hit.byte_len],
                            hit.offset,
                        );
                    }
                }
            }
        }
        return Ok(());
    }
    // Bounding box + per-row run summary.
    if let (Some(&(x0, ..)), Some(&(x1, ..))) = (
        divergences.iter().min_by_key(|d| d.0),
        divergences.iter().max_by_key(|d| d.0),
    ) {
        let y0 = divergences.iter().map(|d| d.1).min().unwrap();
        let y1 = divergences.iter().map(|d| d.1).max().unwrap();
        println!("bbox: x {x0}..={x1} y {y0}..={y1}");
    }
    for &(x, y, e, r) in divergences.iter().take(40) {
        println!("  ({x:4},{y:3}) engine=0x{e:04X} runtime=0x{r:04X}");
    }

    // Which upload writes the first divergence? Rebuild the scene resources
    // without the effect-texture pass and compare the word before/after.
    if let Some(&(dx, dy, ..)) = divergences.first() {
        use legaia_engine_core::scene::{Scene, upload_effect_textures_into_vram};
        use legaia_engine_core::scene_resources::{
            BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
        };
        let prot = std::fs::read(extracted.join("PROT.DAT"))?;
        let cdname = std::fs::read_to_string(extracted.join("CDNAME.TXT"))?;
        let index = legaia_engine_core::scene::ProtIndex::from_bytes(prot, Some(&cdname))?;
        let scene = Scene::load(&index, &scene_name)?;
        let mut shared = Vec::new();
        for name in FIELD_SHARED_BLOCKS {
            if let Ok(s) = Scene::load(&index, name) {
                shared.push(s);
            }
        }
        let shared_refs: Vec<&Scene> = shared.iter().collect();
        let kind = if legaia_engine_core::scene::is_world_map_scene(&scene_name) {
            SceneLoadKind::WorldMap
        } else {
            SceneLoadKind::Field
        };
        let system_ui = index.system_ui_bundle().ok();
        let options = BuildOptions {
            kind,
            upload_all_tims: true,
            system_ui: system_ui.as_deref(),
        };
        let (mut resources, _) =
            SceneResources::build_targeted_with_options(&scene, &shared_refs, options)?;
        let before = vram_to_words_at(&resources.vram, dx, dy);
        let _ = upload_effect_textures_into_vram(&index, &mut resources.vram, false);
        let after = vram_to_words_at(&resources.vram, dx, dy);
        println!(
            "word at ({dx},{dy}): scene-build=0x{before:04X} after-effect-upload=0x{after:04X}"
        );

        // Enumerate the befect cluster's TIM rects; flag every TIM whose image
        // rect covers the divergence cell.
        let raw = index.entry_bytes(874)?;
        let container = legaia_asset::parse_player_lzs(&raw, 3)?;
        let section = &container.descriptors[2];
        let decoded = legaia_asset::decode(&raw, section, legaia_asset::DecodeMode::Lzs)?;
        for target in legaia_asset::befect_cluster::scan_tims(&decoded) {
            if let Ok(tim) = legaia_tim::parse(&decoded[target.offset..]) {
                let img = &tim.image;
                let covers = (img.fb_x as usize..img.fb_x as usize + img.fb_w as usize)
                    .contains(&dx)
                    && (img.fb_y as usize..img.fb_y as usize + img.h as usize).contains(&dy);
                println!(
                    "  etim @0x{:05x} bpp_code={} img ({},{}) {}x{}{}",
                    target.offset,
                    tim.flags & 7,
                    img.fb_x,
                    img.fb_y,
                    img.fb_w,
                    img.h,
                    if covers {
                        "   <-- covers divergence"
                    } else {
                        ""
                    }
                );
                if covers {
                    // The word the TIM itself carries for that cell.
                    let row = (dy - img.fb_y as usize) * img.fb_w as usize * 2;
                    let col = (dx - img.fb_x as usize) * 2;
                    let off = row + col;
                    let w = u16::from_le_bytes([img.data[off], img.data[off + 1]]);
                    println!("      TIM word at ({dx},{dy}) = 0x{w:04X}");
                }
            }
        }
    }

    // Window dump around the first divergence: engine vs runtime[0] vs mask.
    if let Some(&(dx, dy, ..)) = divergences.first() {
        let xs = dx.saturating_sub(8)..(dx + 8).min(VRAM_WIDTH);
        for y in dy.saturating_sub(2)..(dy + 3).min(VRAM_HEIGHT) {
            let eng: Vec<String> = xs
                .clone()
                .map(|x| format!("{:04X}", word(&engine, x, y)))
                .collect();
            let run: Vec<String> = xs
                .clone()
                .map(|x| format!("{:04X}", word(refs[0], x, y)))
                .collect();
            let msk: String = xs
                .clone()
                .map(|x| if mask[y * VRAM_WIDTH + x] { 'S' } else { '.' })
                .collect();
            println!("y={y:3} eng {}", eng.join(" "));
            println!("      run {}", run.join(" "));
            println!("      msk {msk}");
        }
    }
    Ok(())
}
