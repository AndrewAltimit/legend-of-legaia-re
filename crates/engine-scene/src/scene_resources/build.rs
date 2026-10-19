//! `SceneResources::build` and its targeted variants: one scene's VRAM, TMD pool and ANM packs.

use super::*;

impl SceneResources {
    /// Build a fresh resource set from a loaded [`Scene`]. Sweeps every
    /// entry's bytes once, runs the TIM and TMD scanners, populates VRAM
    /// for every TIM that parses, and parses every TMD that the scanner
    /// hits.
    ///
    /// The TIM and TMD scanners are conservative - they validate header
    /// shape before reporting a hit - so spurious parse failures are
    /// rare. When they happen the count is exposed via
    /// [`SceneResources::tim_parse_failures`] for diagnostic logging.
    pub fn build(scene: &Scene) -> Result<Self> {
        Self::build_with_shared(scene, &[])
    }

    /// Same as [`SceneResources::build_with_shared`] but uses the
    /// asset-viewer-style *targeted* VRAM-upload heuristic: every scene
    /// TMD's prim CBA/TSB/UV needs are collected first, then each TIM's
    /// image and CLUT blocks are uploaded *independently* based on
    /// whether they overlap with mesh-required regions and whether they
    /// would clobber some other mesh's data.
    ///
    /// Returns `None` for the upload stats when the scene has no
    /// textured prims (caller falls back to the unfiltered upload).
    /// This is the engine-side parity for what the retail field
    /// loader's asset-chain does: only DMA the TIM bytes the current
    /// frame's meshes need, leaving the rest of VRAM for other scene
    /// resources.
    ///
    /// See [`legaia_tmd::vram_targeted::build_vram_targeted_from_buffers`]
    /// for the per-TIM block-arbitration rule.
    ///
    /// This convenience method calls
    /// [`SceneResources::build_targeted_with_options`] with
    /// [`BuildOptions::default`] - i.e. it includes scene_tmd_stream
    /// PROT entries' type-0x01 TIM upload chunks in the upload set. For
    /// retail-field-parity dispatch (where those chunks are NOT uploaded
    /// at field load time), pass `BuildOptions { kind:
    /// SceneLoadKind::Field }` to [`build_targeted_with_options`].
    ///
    /// [`build_targeted_with_options`]: SceneResources::build_targeted_with_options
    pub fn build_targeted(
        scene: &Scene,
        shared_scenes: &[&Scene],
    ) -> Result<(Self, legaia_tmd::vram_targeted::VramUploadStats)> {
        Self::build_targeted_with_options(scene, shared_scenes, BuildOptions::default())
    }

    /// Same as [`SceneResources::build_targeted`] but with explicit
    /// [`BuildOptions`] - lets the caller distinguish field-mode (skip
    /// scene_tmd_stream battle TIM chunks) from battle-mode (include
    /// them). See [`SceneLoadKind`] for the semantics.
    pub fn build_targeted_with_options(
        scene: &Scene,
        shared_scenes: &[&Scene],
        options: BuildOptions<'_>,
    ) -> Result<(Self, legaia_tmd::vram_targeted::VramUploadStats)> {
        let mut tmds = Vec::new();
        let mut anm_packs = Vec::new();
        let mut shared_tmd_count = 0usize;

        // Pass 1: parse every TMD and ANM from shared + scene so we know
        // exactly what VRAM regions the meshes will sample. For PROT
        // entries shaped like a `battle_data` pack (PROT 0865 and similar),
        // also LZS-decompress every record and pull the embedded TMDs out
        // of the inner payloads - the raw TMD scanner can't see those
        // because the pack wraps each record in an LZS stream.
        //
        // Field-mode dispatch (`SceneLoadKind::Field`) excludes every TMD
        // from a `scene_tmd_stream`-shaped entry: those meshes are battle
        // character meshes routed by `FUN_8001FE70` into the battle
        // character TMD register, never rendered from field / town
        // scenes. Without this skip, the parsed TMD pool drags the prim
        // filter down to ~0% in field mode because all those meshes
        // sample CLUT rows whose TIM uploads (also battle-only) are
        // already correctly skipped.
        let kind = options.kind;
        let parse_scene_tmds_anms = |s: &Scene,
                                     tmds: &mut Vec<ResolvedTmd>,
                                     anm_packs: &mut Vec<ResolvedAnm>,
                                     tmd_count: &mut usize,
                                     is_main: bool| {
            // The walk-view world map draws exactly ONE kingdom bundle: the
            // scene's primary entry (PROT 0085/0244/0391). Its sibling range
            // also contains a kingdom-bundle-shaped *continent* pack (e.g.
            // PROT 0093, ~70 TMDs) that belongs to the zoomed-out world
            // OVERVIEW, not the per-kingdom walk view - the walk-view
            // resident pool is the landmark pack alone (savestate ground
            // truth, docs/subsystems/world-map.md). So accept only the first
            // kingdom bundle per scene - and no shared-block head TMDs at
            // all (the field pool keeps the `player_data` character pack
            // resident; the walk-view pool does not).
            if !is_main && matches!(kind, SceneLoadKind::WorldMap) {
                return;
            }
            // The field mesh pool comes from the retail enumeration: the
            // asset-table descriptor walk for the scene
            // ([`descriptor_walk_pool`]), the character pack for a shared
            // block ([`shared_head_pool`]). `walked` says the pool for this
            // block is settled, which turns the generic magic sweep below off
            // - that sweep is the fallback for blocks with no enumeration to
            // walk, not a supplement to one. A shared block is always settled
            // even when its head pool is empty: `init_data` carries no
            // character pack and contributes no meshes to the field pool.
            //
            // Only the field loader is modelled this way. `WorldMap` has its
            // own descriptor read (the kingdom bundle's slot-1 landmark pack,
            // decoded explicitly below) and would otherwise be walked twice;
            // `Battle` is a different retail loader (`FUN_8001FE70` over the
            // block's `scene_tmd_stream` entries, which is what supplies a
            // battle's backdrop dome), so it keeps the sweep.
            let mut walked = false;
            if matches!(kind, SceneLoadKind::Field) {
                if is_main {
                    let pool = descriptor_walk_pool(s);
                    if !pool.is_empty() {
                        *tmd_count += pool.len();
                        tmds.extend(pool);
                        walked = true;
                    }
                } else {
                    let head = shared_head_pool(s);
                    *tmd_count += head.len();
                    tmds.extend(head);
                    walked = true;
                }
            }
            let mut took_kingdom = false;
            let v12_host = v12_bundle_host_idx(s);
            for entry in &s.entries {
                if v12_sidecar_skip(entry, v12_host) || pochi_filler_skip(entry) {
                    continue;
                }
                let bytes: &[u8] = &entry.bytes;
                // World-map kingdom bundles carry their landmark geometry as
                // slot 1 (a Legaia-TMD pack) of a 7-asset descriptor table,
                // LZS-compressed. The generic raw/LZS magic sweep below can't
                // follow the descriptor table, so retail (and now this build)
                // reads the slot explicitly. This is authoritative for the
                // kingdom entry - skip the heuristic sweep so stray meshes
                // (the "battle-map look") don't leak in. Non-kingdom entries
                // in the same scene fail `decode_slot` and fall through.
                if matches!(kind, SceneLoadKind::WorldMap)
                    && !took_kingdom
                    && let Ok(slot1) = kingdom_bundle::decode_slot(bytes, 1)
                    && let Ok(pack_entries) = pack::parse_pack(&slot1)
                {
                    let mut added = 0usize;
                    for pe in pack_entries {
                        let Some(tslice) = slot1.get(pe.byte_offset..pe.byte_offset + pe.size)
                        else {
                            continue;
                        };
                        if let Ok(tmd) = legaia_tmd::parse(tslice) {
                            tmds.push(ResolvedTmd {
                                entry_idx: entry.idx,
                                offset: pe.byte_offset,
                                byte_len: pe.size,
                                tmd,
                                raw: tslice.to_vec(),
                            });
                            added += 1;
                        }
                    }
                    if added > 0 {
                        *tmd_count += added;
                        took_kingdom = true;
                        continue;
                    }
                }
                // On the world-map MAIN scene, the kingdom bundle (handled
                // above) is the sole geometry source. Its sibling PROT
                // entries hold sub-area / battle assets that retail does NOT
                // draw on the overworld (the resident pool is the slot-1
                // landmark pack + the shared player TMD - see the world-map
                // savestate ground truth in docs/subsystems/world-map.md).
                // Generic-scanning them re-introduces the stray "battle-map"
                // meshes and bloats `scene_aabb`, so skip them here. Shared
                // blocks (`is_main == false`) still scan normally for the
                // player mesh.
                if is_main && matches!(kind, SceneLoadKind::WorldMap) {
                    continue;
                }
                let skip_scene_tmd_stream = kind.excludes_scene_tmd_stream()
                    && scene_tmd_stream::is_scene_tmd_stream(bytes);
                if !walked && !skip_scene_tmd_stream {
                    // Fallback sweep, for blocks the descriptor walk cannot
                    // enumerate: scan raw bytes AND any LZS-decompressed
                    // sections. The count-4 tables of the v12-family and
                    // chapter-2 dungeons are walked like any other (see
                    // `scene_asset_table::mesh_pool`), so this only serves a
                    // block with no walkable table at all.
                    //
                    // This is a heuristic and it over-collects when a block
                    // *does* have a table - which is why it is off whenever
                    // one was walked. See [`descriptor_walk_pool`].
                    let scan = tmd_scan::scan_entry(bytes);
                    for (source, hit) in &scan.hits {
                        let src: &[u8] = match source {
                            tmd_scan::Source::Raw => bytes,
                            tmd_scan::Source::Lzs(idx) => scan.lzs_sections[*idx].as_slice(),
                        };
                        let end = hit.offset + hit.byte_len;
                        if end > src.len() {
                            continue;
                        }
                        let payload = src[hit.offset..end].to_vec();
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
                // battle_data pack: per-record LZS decompression exposes
                // embedded TMDs the raw scanner can't reach. Field mode
                // skips these too - the pack's meshes are battle
                // characters loaded by the retail engine's battle-init
                // path, not by field / town scene-load.
                if kind.excludes_scene_tmd_stream() {
                    continue;
                }
                if let Some(pack) = battle_data_pack::detect(bytes) {
                    for record in &pack.records {
                        let Ok(decoded) =
                            battle_data_pack::decode_record(bytes, &pack, record.index)
                        else {
                            continue;
                        };
                        let Some(rng) = decoded.tmd_range else {
                            continue;
                        };
                        let Some(tmd_bytes) = decoded.bytes.get(rng.clone()) else {
                            continue;
                        };
                        let payload = tmd_bytes.to_vec();
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
        };
        // Main scene first: placement `model_index` bytes index the scene
        // TMD list from 0 (the head slots are addressed via the `0xF0+`
        // special range), so the shared blocks' head TMDs (the retail
        // `player_data` character pack) append after the scene's own.
        parse_scene_tmds_anms(scene, &mut tmds, &mut anm_packs, &mut 0usize, true);
        for shared in shared_scenes {
            parse_scene_tmds_anms(
                shared,
                &mut tmds,
                &mut anm_packs,
                &mut shared_tmd_count,
                false,
            );
        }

        // Pass 2: collect prim targets from every TMD - the union is
        // what the targeted upload aims for.
        let mut needs = Vec::new();
        for rtmd in &tmds {
            needs.extend(legaia_tmd::vram_targeted::collect_prim_targets(
                &rtmd.tmd, &rtmd.raw,
            ));
        }

        // Pass 3: walk every TIM hit (across both shared and scene
        // entries) and feed the targeted builder. The scan walks both
        // raw entry bytes AND any LZS-decompressed sections inside the
        // entry - many battle / level-up bundles wrap their character
        // TIM bank in an LZS container, so a raw-only scan misses
        // them and leaves the dropping CLUT rows unsupplied.
        //
        // When `options.kind == SceneLoadKind::Field`, scene_tmd_stream
        // PROT entries (the dominant scene-asset layout - leading TMD +
        // streaming tail) have their type-0x01 TIM upload chunks
        // excluded. Those chunks are dispatched by `FUN_8001FE70` (the
        // battle scene loader's per-PROT walker), not by any field /
        // town load path. See [`docs/formats/scene-bundles.md`].
        let mut tim_bufs: Vec<Vec<u8>> = Vec::new();
        let mut tim_parse_failures = 0usize;
        // Indices into `tim_bufs` of the kingdom bundle's slot-0 atlas, whose
        // CLUT blocks are re-written last-write-wins after the merge pass
        // below (see the comment there).
        let mut kingdom_tims: Vec<usize> = Vec::new();
        let collect_tim_bufs = |s: &Scene,
                                tim_bufs: &mut Vec<Vec<u8>>,
                                tim_parse_failures: &mut usize,
                                kingdom_tims: &mut Vec<usize>,
                                is_main: bool| {
            // The shared `player_data` block (the raw-876 character pack,
            // extraction 0874) feeds the TMD pool head, but its §2 texture
            // bank is NOT blanket-uploaded by the retail field pre-pass -
            // live field VRAM has none of its extra CLUTs (e.g. the
            // `(0,474)` row). The rows retail does place (478 field-char
            // CLUTs) go through the dedicated `field_char_textures` path.
            if !is_main && s.name == "player_data" {
                return;
            }
            // Match the TMD pool: only the first (primary) kingdom bundle
            // per scene contributes its slot-0 atlas.
            let mut took_kingdom = false;
            let v12_host = v12_bundle_host_idx(s);
            for entry in &s.entries {
                if v12_sidecar_skip(entry, v12_host) || pochi_filler_skip(entry) {
                    continue;
                }
                let bytes: &[u8] = &entry.bytes;
                // World-map kingdom bundles: slot 0 is the terrain TIM
                // atlas (a tim-pack of ~50 TIMs) that textures the slot-1
                // landmark meshes. Decode it explicitly for the same
                // reason as slot 1 above - the descriptor table is opaque
                // to the generic scanner.
                if matches!(kind, SceneLoadKind::WorldMap)
                    && !took_kingdom
                    && let Ok(slot0) = kingdom_bundle::decode_slot(bytes, 0)
                    && let Ok(tim_slices) = pack::extract_pack(&slot0)
                {
                    let mut added = 0usize;
                    for tslice in &tim_slices {
                        if legaia_tim::parse(tslice).is_ok() {
                            kingdom_tims.push(tim_bufs.len());
                            tim_bufs.push(tslice.to_vec());
                            added += 1;
                        }
                    }
                    if added > 0 {
                        took_kingdom = true;
                        continue;
                    }
                }
                // World-map MAIN scene: only the slot-0 atlas textures the
                // (slot-1) landmark meshes. Sibling-entry TIMs texture
                // sub-area meshes we no longer load and would clobber the
                // atlas's VRAM pages, so skip them. Shared blocks still
                // upload (the player atlas). Mirrors the TMD-pool skip.
                if is_main && matches!(kind, SceneLoadKind::WorldMap) {
                    continue;
                }
                // Skip-set: payload offsets of scene_tmd_stream
                // type-0x01 TIM chunks. Only populated when the
                // build is field-mode AND the entry is shaped like
                // a scene_tmd_stream.
                let battle_tim_payload_skips: Vec<usize> = if kind.excludes_scene_tmd_stream() {
                    scene_tmd_stream::battle_tim_chunks(bytes)
                        .iter()
                        .map(|c| c.payload_offset)
                        .collect()
                } else {
                    Vec::new()
                };
                let scan = tim_scan::scan_entry(bytes);
                for (source, hit) in &scan.hits {
                    let src: &[u8] = match source {
                        tim_scan::Source::Raw => bytes,
                        tim_scan::Source::Lzs(idx) => scan.lzs_sections[*idx].as_slice(),
                    };
                    let end = hit.offset + hit.byte_len;
                    if end > src.len() {
                        continue;
                    }
                    // Field-mode: drop TIMs whose offset matches one
                    // of the scene_tmd_stream battle TIM chunks. The
                    // scanner walks raw entry bytes and the skip
                    // set is in raw-buffer coordinates, so only the
                    // `Source::Raw` arm needs the filter.
                    if matches!(source, tim_scan::Source::Raw)
                        && battle_tim_payload_skips.contains(&hit.offset)
                    {
                        continue;
                    }
                    let payload = &src[hit.offset..end];
                    if legaia_tim::parse(payload).is_ok() {
                        tim_bufs.push(payload.to_vec());
                    } else {
                        *tim_parse_failures += 1;
                    }
                }
                // battle_data pack: rescan decompressed record bytes
                // for any TIM headers. The post-TMD region inside
                // each record holds the character textures + CLUTs;
                // most retail records don't tag their texture pool
                // with the standard TIM magic, but the few that do
                // (or future records that gain it) are now reachable.
                // Field mode skips this entirely - battle_data is
                // battle-init resident, not part of field VRAM.
                if kind.excludes_scene_tmd_stream() {
                    continue;
                }
                if let Some(pack) = battle_data_pack::detect(bytes) {
                    for record in &pack.records {
                        let Ok(decoded) =
                            battle_data_pack::decode_record(bytes, &pack, record.index)
                        else {
                            continue;
                        };
                        for hit in tim_scan::scan_buffer(&decoded.bytes) {
                            let end = hit.offset + hit.byte_len;
                            if end > decoded.bytes.len() {
                                continue;
                            }
                            let payload = &decoded.bytes[hit.offset..end];
                            if legaia_tim::parse(payload).is_ok() {
                                tim_bufs.push(payload.to_vec());
                            } else {
                                *tim_parse_failures += 1;
                            }
                        }
                    }
                }
            }
        };
        for shared in shared_scenes {
            collect_tim_bufs(
                shared,
                &mut tim_bufs,
                &mut tim_parse_failures,
                &mut kingdom_tims,
                false,
            );
        }
        let shared_tim_count = tim_bufs.len();
        collect_tim_bufs(
            scene,
            &mut tim_bufs,
            &mut tim_parse_failures,
            &mut kingdom_tims,
            true,
        );
        let tim_count = tim_bufs.len();

        // The shared blocks' TIMs are the lower DMA layer: resident before the
        // scene loads, so a scene CLUT replaces their words outright, its
        // transparent `0x0000` entries included (`LayerMask` in
        // `legaia_tmd::vram_targeted`).
        let (mut vram, upload_stats) = if options.upload_all_tims {
            // Parity-oracle path: DMA-every-TIM, like the retail field loader.
            legaia_tmd::vram_targeted::build_vram_full_layered(
                tim_bufs.iter().map(|v| v.as_slice()),
                shared_tim_count,
            )
        } else {
            // Render path: only the subset a mesh prim samples.
            legaia_tmd::vram_targeted::build_vram_targeted_layered(
                tim_bufs.iter().map(|v| v.as_slice()),
                shared_tim_count,
                &needs,
            )
        };

        // The kingdom atlas is a known, ordered DMA list - the slot-0 pack
        // retail uploads member by member over the boot-resident rows - so
        // its CLUT blocks replace every word they cover, the zeros included,
        // as `LoadImage` does. The merge-zeros pass above exists for the
        // field sweep's over-collected TIMs and would otherwise keep a
        // boot-resident `init_data` word under a kingdom CLUT's transparent
        // entry 0: on `map03` the row-484 slot at `x = 240` (the tree-base
        // quads' CLUT) kept `0x8023`, an opaque near-black, where retail's
        // VRAM holds `0x0000`, and every tree stood on a dark square.
        for &i in &kingdom_tims {
            if let Ok(tim) = legaia_tim::parse(&tim_bufs[i]) {
                vram.upload_tim_partial(&tim, false, true);
            }
        }

        // Synthetic CLUT pass: for every `battle_data` pack entry in the
        // scene's CDNAME block (and any shared block), surface CLUT-row
        // contributions whose post-TMD descriptor places them at a
        // specific `(fb_x, fb_y)` in VRAM. Skipped in field mode -
        // battle_data lives in battle-init VRAM, not field VRAM. The
        // descriptor decoding is currently a no-op (see
        // `battle_data_pack::clut_uploads`); when it's pinned this
        // loop lights up automatically for battle-mode callers.
        let apply_clut_uploads = |s: &Scene, vram: &mut Vram| {
            for entry in &s.entries {
                let bytes: &[u8] = &entry.bytes;
                let Some(pack) = battle_data_pack::detect(bytes) else {
                    continue;
                };
                for record in &pack.records {
                    let Ok(decoded) = battle_data_pack::decode_record(bytes, &pack, record.index)
                    else {
                        continue;
                    };
                    for upload in battle_data_pack::clut_uploads(&decoded) {
                        let Some(row_bytes) = upload.bytes(&decoded) else {
                            continue;
                        };
                        vram.write_clut_row(upload.fb_x, upload.fb_y, row_bytes);
                    }
                }
            }
        };
        if matches!(kind, SceneLoadKind::Battle) {
            for shared in shared_scenes {
                apply_clut_uploads(shared, &mut vram);
            }
            apply_clut_uploads(scene, &mut vram);
        }

        // Row-479 NPC CLUTs are now uploaded via the normal targeted
        // upload path with CLUT merge-zeros semantics (see
        // `legaia_tmd::vram_targeted`). Previously a hardcoded canonical
        // hue-ramp was painted here unconditionally; that paint was from
        // an unknown scene and over-rendered every town.

        // Boot-resident system UI (raw PROT TOC entry 0, `init_data`): retail
        // uploads it at boot (`FUN_801CECD0`, PROT 0895), BEFORE any scene
        // load, and never evicts it. Raw entry 1 is the GAME OVER overlay's
        // page and is not part of the boot image, so it stays out.
        // The bundle's own members upload in pack order into a fresh VRAM
        // (last-write-wins, `FUN_800198E0` LoadImage semantics: image at
        // declared rect + flat-strip CLUT), then that boot image is layered
        // UNDER the scene build - scene words win any overlap, matching the
        // boot-then-scene DMA order. This is what makes CBA row 510 / the
        // (960,256) menu-glyph atlas resident for the field env meshes that
        // sample them (town01 env slots 21/26/74, rikuroa slots 50/51/63).
        if let Some(bundle) = options.system_ui {
            let mut boot = Vram::new();
            bundle.upload_boot_resident_to_vram(&mut boot);
            vram.underlay(&boot);
        }

        Ok((
            Self {
                vram,
                tim_count,
                tim_parse_failures,
                tmds,
                anm_packs,
                shared_tim_count,
                shared_tmd_count,
                render_policy:
                    legaia_render_kernels::prim_dispatch::SceneRenderPolicy::from_scene_kind(
                        options.kind.render_hint(),
                    ),
                world_map_slot4: parse_world_map_slot4(scene, options.kind),
            },
            upload_stats,
        ))
    }
}
