//! Session plumbing behind the Muscle Dome exports: disc tables, fighter assembly, contest state.
//! Split out of `minigames_muscle.rs`.

use super::*;

impl LegaiaMinigames {
    /// Decode the battle overlay (PROT 0898) into the cached dome tables,
    /// returning the status object `load_disc` folds into its report.
    pub(in super::super) fn load_muscle_tables(&mut self) -> String {
        self.muscle = None;
        self.muscle_tables = None;
        // Hand ids live at VA-based offsets of the as-loaded image; the
        // move-power / affinity tables are pinned at raw-entry file offsets.
        // PROT 0898 is stored uncompressed, so both views see the same bytes.
        let loaded = overlay_image(
            &self.prot,
            &self.entries,
            md::MUSCLE_OVERLAY_PROT_INDEX as u32,
        );
        let raw = entry_bytes(
            &self.prot,
            &self.entries,
            md::MUSCLE_OVERLAY_PROT_INDEX as u32,
        );
        let tables = (|| {
            let hand = md::hand_command_ids(loaded.as_deref()?)?;
            let raw = raw?;
            let move_power = move_power::parse(raw)?;
            let move_map = move_power::parse_id_index_map(raw)?;
            let affinity = legaia_asset::element_affinity::parse(raw);
            Some(MuscleTables {
                hand,
                move_power,
                move_map,
                affinity,
            })
        })();
        match tables {
            Some(t) => {
                let list = t
                    .hand
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                let stats = if self.scus.is_some() {
                    "disc"
                } else {
                    "fallback"
                };
                self.muscle_tables = Some(t);
                format!(r#"{{"ok":true,"cards":[{list}],"stats":{}}}"#, jstr(stats))
            }
            None => format!(
                r#"{{"ok":false,"why":{}}}"#,
                jstr("Muscle Dome battle overlay (PROT 0898) or its tables did not decode")
            ),
        }
    }

    pub(super) fn monster_archive_entry(&self) -> Option<&[u8]> {
        entry_bytes(&self.prot, &self.entries, MONSTER_ARCHIVE_PROT_INDEX)
    }

    /// The player character's four swing-card AP costs (runtime slots
    /// `0xC..=0xF`), from their player battle file's **section-default**
    /// equipment records - the same `+0x74` bytes the Arts gauge reads.
    pub(super) fn muscle_swing_costs(&self, char_slot: usize) -> Option<[u16; 4]> {
        let raw = entry_bytes(
            &self.prot,
            &self.entries,
            PLAYER_BATTLE_FILE_BASE + char_slot as u32,
        )?;
        let pack = legaia_asset::battle_data_pack::parse(raw).ok()?;
        // Section defaults (id 0 per slot) - the browser has no save to read
        // an equipped set from. Read through the shared per-slot cost
        // function, the same one the battle Arts input prices its presses
        // with, so the dome and the battle cannot charge differently for
        // the same command.
        let costs =
            legaia_asset::battle_char_assembly::swing_command_costs(raw, &pack, &[0u8; 5]).ok()?;
        let mut out = [0u16; 4];
        for (i, c) in costs.iter().enumerate() {
            out[i] = (*c)? as u16;
        }
        Some(out)
    }

    /// The player fighter's record stats: SCUS new-game template leveled
    /// through the growth curves (jitter-free core), battle-load initialised.
    pub(super) fn muscle_player_fighter(
        &self,
        char_slot: usize,
        level: u32,
    ) -> Option<(MuscleFighter, String)> {
        let scus = self.scus.as_ref()?;
        let party = legaia_asset::new_game::StartingParty::from_scus(scus)?;
        let m = party.member(char_slot)?;
        let growth = legaia_asset::level_up_tables::growth_tables_from_scus(scus)?;
        let params = growth.char_params(char_slot)?;
        // Template stat order == growth-param record order:
        // [hp, mp, agl, atk, udf, ldf, spd, int].
        let base = [
            m.hp_max, m.mp_max, m.agl, m.atk, m.udf, m.ldf, m.spd, m.intel,
        ];
        let mut stats = base.map(u32::from);
        let level = level.clamp(1, legaia_asset::level_up_tables::MAX_LEVEL as u32);
        for from_level in 1..level as usize {
            for (i, p) in params.stats.iter().enumerate() {
                let gain = growth.level_gain_core(p, from_level).unwrap_or(0);
                stats[i] = (stats[i] + gain).min(p.max as u32);
            }
        }
        let record = RecordStats {
            hp_max: stats[0] as u16,
            hp_cur: stats[0] as u16,
            mp_max: stats[1] as u16,
            mp_cur: stats[1] as u16,
            spirit: 0,
            agl: stats[2] as u16,
            atk: stats[3] as u16,
            udf: stats[4] as u16,
            ldf: stats[5] as u16,
            spd: stats[6] as u16,
            int: stats[7] as u16,
        };
        // Battle-load stat init (FUN_80053CB8), no equipment bonuses.
        let actor = init_party_battle_stats(&record, &[None; 5]);
        let element = self
            .muscle_tables
            .as_ref()
            .and_then(|t| t.affinity.as_ref())
            .and_then(|a| a.character_element(char_slot as u8 + 1))
            .unwrap_or(ELEMENT_NEUTRAL);
        Some((
            MuscleFighter {
                hp_max: actor.hp_max,
                mp_max: actor.mp_max,
                budget_pool: actor.agl,
                int: actor.int,
                udf: actor.udf,
                ldf: actor.ldf,
                element,
            },
            m.name.clone(),
        ))
    }

    /// The arena backdrop entry's bytes, gated on the scene_tmd_stream shape
    /// (so a truncated / foreign image degrades to "no arena" rather than a
    /// garbage mesh).
    pub(super) fn muscle_arena_entry(&self) -> Option<&[u8]> {
        let buf = entry_bytes(&self.prot, &self.entries, ARENA_BACKDROP_PROT_INDEX)?;
        scene_tmd_stream::detect(buf).map(|_| buf)
    }

    /// The arena-shell TMD built as a hybrid VRAM mesh (textured prims sample
    /// the backdrop pages; untextured prims keep their baked colour word).
    pub(super) fn muscle_arena_hybrid(&self) -> Option<(legaia_tmd::mesh::VramMesh, Vec<u8>)> {
        let buf = self.muscle_arena_entry()?;
        let stream = scene_tmd_stream::detect(buf)?;
        let tmd_bytes = buf.get(stream.tmd_range())?;
        let tmd = legaia_tmd::parse(tmd_bytes).ok()?;
        let (mesh, oids, shading) =
            legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd, tmd_bytes);
        // A textured vert's colour is its MODULATION and lives on `mesh`;
        // `shading.colors` reports white there by design, so reading one
        // array for both halves uploads white and draws the shell at
        // `texel * 255/128`. [`crate::packet_color::hybrid`] is the split.
        let flat = crate::packet_color::hybrid(&mesh, &shading);
        // Drop TMD object 1 - the wall-base dust-decal object (12 ABE ABR-1
        // quads over the (128..190, 192..253) window of the (832, 0) page,
        // CLUT (16, 479)). Its texels are genuinely BRIGHT (whitish wisps up
        // to ~(208, 208, 248)), so even a correct additive draw reads as a
        // white cloud band ringing the arena - and the retail match capture
        // shows a mist-free interior, i.e. the retail backdrop path does not
        // draw this object as static geometry. The shell (object 0) keeps
        // its own ABE lamp-glow prims. See
        // docs/subsystems/minigame-muscle-dome.md (Arena backdrop).
        if oids.iter().any(|&o| o != 0) {
            let mut mesh2 = mesh.clone();
            mesh2.positions.clear();
            mesh2.uvs.clear();
            mesh2.cba_tsb.clear();
            mesh2.normals.clear();
            mesh2.colors.clear();
            mesh2.indices.clear();
            let mut remap = vec![u32::MAX; oids.len()];
            let mut flat2 = Vec::new();
            for (i, &o) in oids.iter().enumerate() {
                if o != 0 {
                    continue;
                }
                remap[i] = mesh2.positions.len() as u32;
                mesh2.positions.push(mesh.positions[i]);
                mesh2.uvs.push(mesh.uvs[i]);
                mesh2.cba_tsb.push(mesh.cba_tsb[i]);
                mesh2.normals.push(mesh.normals[i]);
                mesh2.colors.push(mesh.colors[i]);
                flat2.extend_from_slice(&flat[i * 4..i * 4 + 4]);
            }
            for t in mesh.indices.as_chunks::<3>().0 {
                let (a, b, c) = (
                    remap[t[0] as usize],
                    remap[t[1] as usize],
                    remap[t[2] as usize],
                );
                if a != u32::MAX && b != u32::MAX && c != u32::MAX {
                    mesh2.indices.extend_from_slice(&[a, b, c]);
                }
            }
            return Some((mesh2, flat2));
        }
        Some((mesh, flat))
    }

    /// The player battle file (`data\battle\PLAYER1..3`) for a dome fighter
    /// slot (0 = Vahn, 1 = Noa, 2 = Gala).
    pub(super) fn muscle_player_file(&self, char_slot: u32) -> Option<&[u8]> {
        entry_bytes(
            &self.prot,
            &self.entries,
            PLAYER_BATTLE_FILE_BASE + char_slot.min(2),
        )
    }

    /// Assemble the character's **battle form** (fighter form) - the same
    /// retail chain the arts viewer / native battles use: equipment-id
    /// sections spliced (`assemble_character`, all-default sections - the
    /// dome forbids equipment), TSB/CBA relocated to runtime band 0
    /// (`FUN_800513F0` registration pass). Returns the assembly (for
    /// `anm_bones`), the VRAM mesh and the per-vertex object ids.
    pub(super) fn muscle_fighter_build(
        &self,
        char_slot: u32,
    ) -> Option<(
        bca::AssembledCharacter,
        legaia_tmd::mesh::VramMesh,
        Vec<u32>,
    )> {
        let raw = self.muscle_player_file(char_slot)?;
        let pack = legaia_asset::battle_data_pack::parse(raw).ok()?;
        let mut asm = bca::assemble_character(raw, &pack, &[0u8; 5]).ok()?;
        bca::relocate_tsb_cba(&mut asm.tmd, 0).ok()?;
        let tmd = legaia_tmd::parse(&asm.tmd).ok()?;
        let (mesh, oids) = legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, &asm.tmd);
        (!mesh.indices.is_empty()).then_some((asm, mesh, oids))
    }

    /// One battle-form action clip by **runtime action slot**, expanded per
    /// assembled TMD object so channel `i` drives object `i`:
    ///
    /// - slot `0` - the record[0] idle loop (`idle_battle_animation`);
    /// - slots `0xC..=0xF` - the per-command **swing records** of the
    ///   equipment sections (`swing_battle_animations`, section defaults) -
    ///   the same entries the dome's card ids `0xC..=0xF` name, so the
    ///   card -> clip pairing is the disc's own, not a fit;
    /// - other slots - the record[0] action table by index (the party
    ///   hit-reaction family: `FUN_80053CB8` writes the constant map
    ///   `[2, 3, 4, 5, 0xB]` to `+0x1EF..`, and the damage primitive
    ///   `FUN_800402F4` stages the light flinch from `+0x1EF` = slot 2).
    pub(super) fn muscle_fighter_clip(
        &self,
        char_slot: u32,
        slot: u32,
    ) -> Option<monster_archive::MonsterAnimation> {
        let raw = self.muscle_player_file(char_slot)?;
        let (asm, _, _) = self.muscle_fighter_build(char_slot)?;
        let anim = if (0xC..=0xF).contains(&slot) {
            let pack = legaia_asset::battle_data_pack::parse(raw).ok()?;
            bca::swing_battle_animations(raw, &pack, &[0u8; 5])
                .ok()?
                .into_iter()
                .find(|s| s.slot as u32 == slot)?
                .anim
        } else if slot == 0 {
            bca::idle_battle_animation(raw).ok()??
        } else {
            bca::battle_animations(raw)
                .ok()?
                .into_iter()
                .find(|a| a.action_id as u32 == slot)?
        };
        Some(bca::expand_animation_for_objects(&anim, &asm.anm_bones))
    }

    /// The character's Tactical-Arts catalog for the queue -> art resolver:
    /// `(display name, kind, command string)` rows. Directions + names come
    /// from the disc's own SCUS arts-name table when an executable was
    /// loaded ([`legaia_art::arts_table`]); the curated
    /// [`legaia_gamedata`] arts table is the fallback catalog on a raw
    /// `PROT.DAT` load and, in both cases, the source of the **kind** label
    /// (regular / hyper / super / miracle - ground-truth walkthrough
    /// labels), joined by exact direction sequence.
    pub(super) fn muscle_art_catalog(
        &self,
        char_slot: usize,
    ) -> Vec<(String, &'static str, Vec<ArtCommand>)> {
        let art_char = match char_slot {
            1 => ArtCharacter::Noa,
            2 => ArtCharacter::Gala,
            _ => ArtCharacter::Vahn,
        };
        let gd_char = match char_slot {
            1 => legaia_gamedata::Character::Noa,
            2 => legaia_gamedata::Character::Gala,
            _ => legaia_gamedata::Character::Vahn,
        };
        let db = gamedata_db();
        let kind_str = |k: legaia_gamedata::ArtKind| match k {
            legaia_gamedata::ArtKind::Regular => "regular",
            legaia_gamedata::ArtKind::Hyper => "hyper",
            legaia_gamedata::ArtKind::Super => "super",
            legaia_gamedata::ArtKind::Miracle => "miracle",
        };
        let disc_rows: Vec<(String, &'static str, Vec<ArtCommand>)> = self
            .scus
            .as_deref()
            .and_then(legaia_art::arts_table::parse_from_scus)
            .map(|entries| {
                entries
                    .into_iter()
                    .filter(|e| e.character == art_char && !e.commands.is_empty())
                    .map(|e| {
                        let dirs: Vec<u8> = e.commands.iter().map(|c| c.as_byte()).collect();
                        let kind = db
                            .find_art_by_directions(gd_char, &dirs)
                            .map(|a| kind_str(a.kind))
                            .unwrap_or("regular");
                        (e.name, kind, e.commands)
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !disc_rows.is_empty() {
            return disc_rows;
        }
        db.arts_for(gd_char)
            .filter(|a| !a.directions.is_empty())
            .filter_map(|a| {
                let cmds: Option<Vec<ArtCommand>> = a
                    .directions
                    .iter()
                    .map(|&b| ArtCommand::from_byte(b))
                    .collect();
                Some((a.name.clone(), kind_str(a.kind), cmds?))
            })
            .collect()
    }

    /// Decode one static-table cue's voice layer to `(pcm, rate)`: descriptor
    /// row `row` of the SCUS SFX table keys VAB program `p` tones
    /// `t .. t+voices` at note `l` out of the bank its `+4` category routes to
    /// (slot 0 = PROT 0868, slot 2 = PROT 0869 - `docs/formats/sfx-table.md`).
    pub(super) fn muscle_static_cue(&self, row: u8, voice: u8) -> Option<(Vec<i16>, u32)> {
        let scus = self.scus.as_ref()?;
        let table = sfx_table::SfxTable::from_scus(scus)?;
        let desc = *table.get(row)?;
        if voice >= desc.voice_count() {
            return None;
        }
        let bank_prot = sfx_table::prot_index_for_slot(desc.vab_slot())?;
        let entry = entry_bytes(&self.prot, &self.entries, bank_prot)?;
        let off = *legaia_vab::find_vabs(entry).first()?;
        let report = legaia_vab::parse(entry, off).ok()?;
        // Multi-voice cues span consecutive tone regions (`sfx-table.md`:
        // "the per-voice loop adds the voice index").
        let atr = report
            .tones
            .get(desc.program as usize)?
            .get(desc.tone as usize + voice as usize)?;
        if atr.vag <= 0 {
            return None;
        }
        let span = report.vag_samples.get(atr.vag as usize - 1)?;
        let body = entry.get(span.byte_offset..span.byte_offset + span.size)?;
        let pcm = legaia_vab::decode_vag_aligned(body).ok()?;
        let semitones = desc.note as f64 - atr.center as f64;
        let rate = (44100.0 * 2f64.powf(semitones / 12.0)).round();
        Some((pcm, rate.clamp(4000.0, 96_000.0) as u32))
    }

    /// An opponent fighter out of the monster archive: the record's boosted
    /// battle-stat profile, its AGL as the budget pool, its own element.
    pub(super) fn muscle_monster_fighter(
        &self,
        monster_id: u16,
    ) -> Option<(MuscleFighter, String)> {
        let entry = self.monster_archive_entry()?;
        let rec = monster_archive::record(entry, monster_id).ok()??;
        // battle_stats() order: [AGL, ATK, UDF, LDF, INT, SPD] (boosted).
        let bs = rec.battle_stats();
        Some((
            MuscleFighter {
                hp_max: rec.hp,
                mp_max: 0,
                budget_pool: bs[0],
                int: bs[4],
                udf: bs[2],
                ldf: bs[3],
                element: rec.element.min(ELEMENT_NEUTRAL),
            },
            rec.name.clone(),
        ))
    }
}
