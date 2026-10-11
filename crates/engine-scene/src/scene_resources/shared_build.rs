//! Builds that layer shared blocks under a scene, and the battle boot VRAM.

use super::*;

impl SceneResources {
    /// Same as [`SceneResources::build`] but also bulk-uploads every TIM
    /// and parses every TMD from a set of *shared* CDNAME-block scenes -
    /// the retail engine keeps these resident across scene transitions
    /// at VRAM slots the per-scene loader never touches (player TMD,
    /// shared UI sprite atlas, etc.).
    ///
    /// The shared scenes contribute to [`SceneResources::vram`] *first*,
    /// so scene-local TIMs that map to overlapping slots win the upload
    /// (mirrors the runtime "load shared at boot, then load scene"
    /// order). Their TIM/TMD counts feed [`SceneResources::shared_tim_count`]
    /// / [`SceneResources::shared_tmd_count`]. The TMDs and ANM packs
    /// are appended to the same flat pools so existing
    /// [`SceneResources::tmd_by_coords`] / [`SceneResources::anm_pack_for_actor`]
    /// lookups work unchanged.
    ///
    /// See [`FIELD_SHARED_BLOCKS`] for the names a field / town caller
    /// passes in.
    pub fn build_with_shared(scene: &Scene, shared_scenes: &[&Scene]) -> Result<Self> {
        let mut vram = Vram::new();
        let mut tim_count = 0usize;
        let mut tim_parse_failures = 0usize;
        let mut tmds = Vec::new();
        let mut anm_packs = Vec::new();
        let mut shared_tmd_count = 0usize;

        let sweep_scene = |s: &Scene,
                           vram: &mut Vram,
                           tmds: &mut Vec<ResolvedTmd>,
                           anm_packs: &mut Vec<ResolvedAnm>,
                           tim_count: &mut usize,
                           tim_parse_failures: &mut usize,
                           tmd_count: &mut usize| {
            let v12_host = v12_bundle_host_idx(s);
            for entry in &s.entries {
                if v12_sidecar_skip(entry, v12_host) || pochi_filler_skip(entry) {
                    continue;
                }
                let bytes: &[u8] = &entry.bytes;
                for hit in tim_scan::scan_buffer(bytes) {
                    let payload = &bytes[hit.offset..hit.offset + hit.byte_len];
                    match legaia_tim::parse(payload) {
                        Ok(tim) => {
                            vram.upload_tim(&tim);
                            *tim_count += 1;
                        }
                        Err(_) => *tim_parse_failures += 1,
                    }
                }
                for hit in tmd_scan::scan_buffer(bytes) {
                    let payload = bytes[hit.offset..hit.offset + hit.byte_len].to_vec();
                    if let Ok(tmd) = legaia_tmd::parse(&payload) {
                        tmds.push(ResolvedTmd {
                            entry_idx: entry.idx,
                            offset: hit.offset,
                            byte_len: hit.byte_len,
                            tmd,
                            raw: payload,
                        });
                        *tmd_count += 1;
                    }
                }
                if let Some(det) = anm_detect::detect(bytes) {
                    let payload = bytes[..det.size].to_vec();
                    if let Ok(pack) = legaia_anm::parse(&payload) {
                        anm_packs.push(ResolvedAnm {
                            entry_idx: entry.idx,
                            offset: 0,
                            byte_len: det.size,
                            pack,
                            payload,
                        });
                    }
                }
                // battle_data pack: pull every embedded TMD out of the
                // LZS-decoded records. Also rescan each record's bytes
                // for any standard TIM blocks - records whose texture
                // pool uses the standard TIM magic become reachable.
                if let Some(pack) = battle_data_pack::detect(bytes) {
                    for record in &pack.records {
                        let Ok(decoded) =
                            battle_data_pack::decode_record(bytes, &pack, record.index)
                        else {
                            continue;
                        };
                        for hit in tim_scan::scan_buffer(&decoded.bytes) {
                            let payload = &decoded.bytes[hit.offset..hit.offset + hit.byte_len];
                            if let Ok(tim) = legaia_tim::parse(payload) {
                                vram.upload_tim(&tim);
                                *tim_count += 1;
                            } else {
                                *tim_parse_failures += 1;
                            }
                        }
                        if let Some(rng) = decoded.tmd_range.clone() {
                            let payload = decoded.bytes[rng.clone()].to_vec();
                            if let Ok(tmd) = legaia_tmd::parse(&payload) {
                                tmds.push(ResolvedTmd {
                                    entry_idx: entry.idx,
                                    offset: rng.start,
                                    byte_len: payload.len(),
                                    tmd,
                                    raw: payload,
                                });
                                *tmd_count += 1;
                            }
                        }
                    }
                }
            }
        };

        // Shared blocks first so scene-local TIMs win any slot collision.
        for shared in shared_scenes {
            sweep_scene(
                shared,
                &mut vram,
                &mut tmds,
                &mut anm_packs,
                &mut tim_count,
                &mut tim_parse_failures,
                &mut shared_tmd_count,
            );
        }
        // After the shared sweep, tim_count IS the shared contribution -
        // snapshot it before the scene sweep mutates it further.
        let shared_tim_count = tim_count;

        sweep_scene(
            scene,
            &mut vram,
            &mut tmds,
            &mut anm_packs,
            &mut tim_count,
            &mut tim_parse_failures,
            &mut 0usize,
        );

        Ok(Self {
            vram,
            tim_count,
            tim_parse_failures,
            tmds,
            anm_packs,
            shared_tim_count,
            shared_tmd_count,
            // `build_with_shared` is the boot/general-purpose path; no
            // scene-kind hint is threaded through, so default to the
            // SCUS variant. Callers that need the world-map overlay
            // variant should go through `build_targeted_with_options`
            // instead.
            render_policy: legaia_render_kernels::prim_dispatch::SceneRenderPolicy::default(),
            // No scene-kind hint here; the world-map slot-4 pool is only
            // resolved on the `build_targeted_with_options` WorldMap path.
            world_map_slot4: None,
        })
    }

    /// Build a "battle boot VRAM" from the `battle_data` CDNAME block
    /// (PROT 865..869). The returned VRAM holds every standard-TIM-
    /// shaped contribution the engine can recover from inside the
    /// per-record LZS streams - the texture pools wrapped in PSX TIM
    /// magic - plus any future `clut_uploads()` placements the pack
    /// reverse-engineer surfaces.
    ///
    /// The retail engine performs an equivalent pre-pass via the
    /// `FUN_8001E890` chain when the player approaches a battle
    /// transition. Wiring the same data into the engine port lets
    /// battle-mode scene builds see battle character textures + CLUTs
    /// without having to redo the parse on every transition.
    ///
    /// Returns an empty VRAM when `battle_data` isn't in the index or
    /// none of its records carry standard-TIM-magic texture pools.
    /// `tmd_count` mirrors the parsed-TMD count for diagnostics. The
    /// stable contract: every byte this writes corresponds to a real
    /// retail VRAM write - no synthetic CLUT painting, no placeholder
    /// data.
    ///
    /// See [`docs/subsystems/asset-loader.md`] for the dispatch
    /// boundaries and [`BATTLE_BOOT_BLOCKS`] for the block list.
    pub fn build_battle_boot_vram(battle_data_scenes: &[&Scene]) -> (Vram, BattleBootStats) {
        let mut vram = Vram::new();
        let mut stats = BattleBootStats::default();
        for scene in battle_data_scenes {
            for entry in &scene.entries {
                let bytes: &[u8] = &entry.bytes;
                let Some(pack) = battle_data_pack::detect(bytes) else {
                    continue;
                };
                stats.packs += 1;
                for record in &pack.records {
                    let Ok(decoded) = battle_data_pack::decode_record(bytes, &pack, record.index)
                    else {
                        continue;
                    };
                    stats.records += 1;
                    // Standard-TIM-shaped textures inside the record.
                    for hit in tim_scan::scan_buffer(&decoded.bytes) {
                        let end = hit.offset + hit.byte_len;
                        if end > decoded.bytes.len() {
                            continue;
                        }
                        let payload = &decoded.bytes[hit.offset..end];
                        if let Ok(tim) = legaia_tim::parse(payload) {
                            vram.upload_tim(&tim);
                            stats.tims_uploaded += 1;
                        }
                    }
                    // Pinned descriptor placements (no-op until
                    // `battle_data_pack::clut_uploads` is wired).
                    for upload in battle_data_pack::clut_uploads(&decoded) {
                        let Some(row_bytes) = upload.bytes(&decoded) else {
                            continue;
                        };
                        vram.write_clut_row(upload.fb_x, upload.fb_y, row_bytes);
                        stats.cluts_uploaded += 1;
                    }
                }
            }
        }
        (vram, stats)
    }
}
