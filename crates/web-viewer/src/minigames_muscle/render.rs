//! The Muscle Dome's 3D exports: fighter / opponent meshes and poses, the arena backdrop, and dome SFX.
//! Split out of `minigames_muscle.rs`.

use super::*;

#[wasm_bindgen]
impl LegaiaMinigames {
    // ------------------------------------------------------------- dome 3D
    //
    // The opponent is the monster archive's own mesh + rigid-part animation
    // set (PROT 867), relocated to battle texture slot 0 exactly as the
    // battle loader does (`battle_render_mesh`, FUN_80055468); the player
    // fighter is the character's **assembled battle form** - retail fields
    // the party's normal fighter forms in the dome, not the Baka pack - the
    // same `battle_char_assembly` chain the arts viewer and the native
    // battles use: player battle file (PROT 863+char) equipment-id sections
    // assembled + TSB/CBA-relocated to band 0, posed from the file's own
    // record[0] action streams and per-command swing records. `muscle_vram`
    // merges the character's band-0 texture pool + battle palette with the
    // monster's texture pool so one TmdRenderer VRAM serves both bodies.

    /// Whether the dome's 3D scene decodes for `(monster_id, char_slot)`:
    /// the character's assembled battle form plus the monster's mesh + idle
    /// animation.
    pub fn muscle_scene_ready(&self, monster_id: u16, char_slot: u32) -> bool {
        let monster_ok = self.monster_archive_entry().is_some_and(|e| {
            matches!(monster_archive::mesh(e, monster_id), Ok(Some(_)))
                && matches!(monster_archive::idle_animation(e, monster_id), Ok(Some(_)))
        });
        monster_ok && self.muscle_fighter_build(char_slot).is_some()
    }

    /// Per-vertex positions of the character's assembled battle-form mesh
    /// (flat `f32`, 3 per vertex). Empty when the player file doesn't
    /// assemble on this image.
    pub fn muscle_fighter_positions(&self, char_slot: u32) -> Vec<f32> {
        let Some((_, mesh, _)) = self.muscle_fighter_build(char_slot) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.extend_from_slice(&[p[0], p[1], p[2]]);
        }
        out
    }

    /// Per-vertex `[u, v]` texel coords, parallel to the positions.
    pub fn muscle_fighter_uvs(&self, char_slot: u32) -> Vec<i32> {
        let Some((_, mesh, _)) = self.muscle_fighter_build(char_slot) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.extend_from_slice(&[uv[0] as i32, uv[1] as i32]);
        }
        out
    }

    /// Per-vertex `[cba, tsb]` (band-0 relocated), parallel to the positions.
    pub fn muscle_fighter_cba_tsb(&self, char_slot: u32) -> Vec<u32> {
        let Some((_, mesh, _)) = self.muscle_fighter_build(char_slot) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.cba_tsb.len() * 2);
        for ct in &mesh.cba_tsb {
            out.extend_from_slice(&[ct[0] as u32, ct[1] as u32]);
        }
        out
    }

    /// Triangle indices of the assembled battle-form mesh.
    pub fn muscle_fighter_indices(&self, char_slot: u32) -> Vec<u32> {
        self.muscle_fighter_build(char_slot)
            .map(|(_, m, _)| m.indices)
            .unwrap_or_default()
    }

    /// Per-vertex TMD object index (the rigid part a vertex hangs from).
    pub fn muscle_fighter_object_ids(&self, char_slot: u32) -> Vec<u32> {
        self.muscle_fighter_build(char_slot)
            .map(|(_, _, oids)| oids)
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, 255]` - the assembled form draws fully
    /// textured, so every vertex carries the prim's **packet colour**, the
    /// modulation half of retail's `texel * colour / 128`. Emitting white
    /// here instead of the colour word is not "unlit", it is `texel * 2.0`
    /// (see [`crate::packet_color`]) - which is what read as blown-out
    /// arena lighting.
    pub fn muscle_fighter_flat_rgba(&self, char_slot: u32) -> Vec<u8> {
        self.muscle_fighter_build(char_slot)
            .map(|(_, m, _)| crate::packet_color::textured(&m))
            .unwrap_or_default()
    }

    /// Assembled TMD object count (pose rig width).
    pub fn muscle_fighter_part_count(&self, char_slot: u32) -> u32 {
        self.muscle_fighter_build(char_slot)
            .map(|(asm, _, _)| asm.anm_bones.len() as u32)
            .unwrap_or(0)
    }

    /// Every battle-form clip the dome page plays, in runtime action-slot
    /// order: `[{"slot":0,"rate":r,"frame_count":f}, ...]` for the idle
    /// (slot 0), the light flinch (slot 2, the head of the party
    /// hit-reaction map `[2,3,4,5,0xB]` `FUN_80053CB8` writes to
    /// `+0x1EF..`), the knockdown-family entry (slot 4) and the four
    /// per-command swings (slots `0xC..=0xF` - the card ids themselves).
    /// A slot whose stream doesn't decode is omitted.
    pub fn muscle_fighter_anims_json(&self, char_slot: u32) -> String {
        let rows: Vec<String> = [0u32, 2, 4, 0xC, 0xD, 0xE, 0xF]
            .iter()
            .filter_map(|&slot| {
                let a = self.muscle_fighter_clip(char_slot, slot)?;
                Some(format!(
                    r#"{{"slot":{},"rate":{},"frame_count":{}}}"#,
                    slot, a.rate, a.frame_count
                ))
            })
            .collect();
        format!("[{}]", rows.join(","))
    }

    /// Battle-form clip `slot`'s pose frames: per (frame, part) absolute
    /// `[tx, ty, tz, rx, ry, rz]` (PSX 4096-unit angles), expanded per
    /// assembled object and padded to `target_part_count` - the same pose
    /// layout every other site animator consumes.
    pub fn muscle_fighter_pose_frames(
        &self,
        char_slot: u32,
        slot: u32,
        target_part_count: u32,
    ) -> Vec<i32> {
        let Some(anim) = self.muscle_fighter_clip(char_slot, slot) else {
            return Vec::new();
        };
        let parts = (target_part_count as usize).max(anim.part_count);
        let mut out = Vec::with_capacity(anim.frame_count * parts * 6);
        for frame in &anim.frames {
            for p in 0..parts {
                match frame.get(p) {
                    Some(t) => out.extend_from_slice(&[
                        t.tx as i32,
                        t.ty as i32,
                        t.tz as i32,
                        t.rx as i32,
                        t.ry as i32,
                        t.rz as i32,
                    ]),
                    None => out.extend_from_slice(&[0; 6]),
                }
            }
        }
        out
    }

    /// Resolve the player's **committed card queue** through the character's
    /// real Tactical-Arts tables: the greedy longest-match walk of the
    /// runtime art recognizer (`legaia_art::recognize`), with the span each
    /// matched art covers so the page can time the retail arts banner over
    /// the playback:
    ///
    /// ```json
    /// [ { "name": "Tornado Flame", "kind": "hyper", "start": 0, "len": 3 } ]
    /// ```
    ///
    /// `start`/`len` index the player's committed queue (= the player's
    /// playback events in order). Directions + names come from the disc's
    /// own SCUS arts-name table (curated-table fallback on a raw `PROT.DAT`
    /// load); the kind label joins the curated arts table by exact direction
    /// sequence. Empty when no contest is live or nothing in the queue
    /// performs an art.
    pub fn muscle_round_arts_json(&self) -> String {
        let Some(c) = self.muscle.as_ref() else {
            return "[]".to_string();
        };
        // Card ids 0xC..=0xF are the direction-command ids; the art tables'
        // direction bytes are 1..=4 in the same L/R/D/U order.
        let input: Vec<ArtCommand> = c
            .session
            .queue(0)
            .iter()
            .filter_map(|&cmd| ArtCommand::from_byte(cmd.wrapping_sub(0xB)))
            .collect();
        if input.is_empty() {
            return "[]".to_string();
        }
        let catalog = self.muscle_art_catalog(c.char_slot);
        // Greedy longest-match, left to right, connectors skipped - the
        // recognizer's documented walk (REF: legaia_art::recognize::
        // recognize_art_sequence), tracked here with span positions.
        let mut out: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < input.len() {
            let mut best: Option<(usize, usize)> = None;
            for (idx, (_, _, cmds)) in catalog.iter().enumerate() {
                if cmds.is_empty() || !input[i..].starts_with(cmds) {
                    continue;
                }
                if best.is_none_or(|(_, len)| len < cmds.len()) {
                    best = Some((idx, cmds.len()));
                }
            }
            match best {
                Some((idx, len)) => {
                    let (name, kind, _) = &catalog[idx];
                    out.push(format!(
                        r#"{{"name":{},"kind":{},"start":{},"len":{}}}"#,
                        jstr(name),
                        jstr(kind),
                        i,
                        len
                    ));
                    i += len;
                }
                None => i += 1,
            }
        }
        format!("[{}]", out.join(","))
    }

    /// The retail Triangle-list rows for the contest fighter: the character's
    /// arts out of the disc's own SCUS arts-name table (`DAT_80075EC4` -
    /// name, `+2` AP byte, directional command string), the exact source the
    /// retail battle input's "Hyper Arts list" overlay draws its
    /// name / arrow-string / AP columns from. Rows:
    ///
    /// ```json
    /// [ { "name": "Slash Kick", "ap": 40, "dirs": [4,3,2], "kind": "hyper" } ]
    /// ```
    ///
    /// `dirs` are the art-table direction bytes (1 Left, 2 Right, 3 Down,
    /// 4 Up). Miracle rows (marker-only command strings) are skipped, as the
    /// retail list skips them. The curated gamedata table is the fallback on
    /// a raw `PROT.DAT` load (no AP column there - `ap` reads 0) and the
    /// source of the `kind` label in both cases. The page does not model
    /// arts *learning*, so every table row lists (disclosed on the page);
    /// retail gates rows on the character's learned-art constant.
    pub fn muscle_arts_list_json(&self) -> String {
        let Some(c) = self.muscle.as_ref() else {
            return "[]".to_string();
        };
        let ap_by_name: std::collections::HashMap<String, u8> = self
            .scus
            .as_deref()
            .and_then(legaia_art::arts_table::parse_from_scus)
            .map(|entries| entries.into_iter().map(|e| (e.name, e.ap)).collect())
            .unwrap_or_default();
        let rows: Vec<String> = self
            .muscle_art_catalog(c.char_slot)
            .into_iter()
            .filter(|(name, _, _)| !name.is_empty())
            .map(|(name, kind, cmds)| {
                let dirs: Vec<String> = cmds.iter().map(|c| c.as_byte().to_string()).collect();
                format!(
                    r#"{{"name":{},"ap":{},"dirs":[{}],"kind":{}}}"#,
                    jstr(&name),
                    ap_by_name.get(&name).copied().unwrap_or(0),
                    dirs.join(","),
                    jstr(kind)
                )
            })
            .collect();
        format!("[{}]", rows.join(","))
    }

    /// Whether the player's selection is exhausted (no dealt direction is
    /// affordable): the retail auto-end of the command input.
    pub fn muscle_selection_exhausted(&self) -> bool {
        self.muscle
            .as_ref()
            .is_some_and(|c| c.session.selection_exhausted(0))
    }

    /// The retail confirm menu's "Reselect" arm: throw the player's committed
    /// queue away and restore the turn budget.
    pub fn muscle_reset_selection(&mut self) {
        if let Some(c) = self.muscle.as_mut() {
            c.session.reset_selection(0);
        }
    }

    /// Advance the round **time meter** one frame by the frame delta `dt`,
    /// returning the bar sprite's new Y offset (`-0x92` empty, `+0xE` full).
    /// The counter climbs while the direction-entry phase runs and drains
    /// otherwise - retail gates the ramp on `ctx+6 == 0x50`, the entry phase,
    /// **not** on the playback. Returns `0` with no contest up.
    ///
    /// PORT: FUN_801d3444, through
    /// [`MuscleDomeSession::tick_time_meter`](legaia_engine_core::muscle_dome::MuscleDomeSession::tick_time_meter)
    pub fn muscle_tick_time_meter(&mut self, dt: u8) -> i32 {
        self.muscle
            .as_mut()
            .map(|c| c.session.tick_time_meter(dt) as i32)
            .unwrap_or(0)
    }

    pub(super) fn muscle_monster_render_mesh(
        &self,
        monster_id: u16,
    ) -> Option<legaia_tmd::mesh::VramMesh> {
        let entry = self.monster_archive_entry()?;
        let mesh = monster_archive::mesh(entry, monster_id).ok()??;
        // Scratch VRAM: the geometry accessors only need the relocated
        // CBA/TSB; `muscle_vram` repeats the texture injection for real.
        let mut scratch = legaia_tim::Vram::new();
        mesh.battle_render_mesh(0, &mut scratch)
    }

    /// Per-vertex positions of monster `monster_id`'s battle mesh.
    pub fn muscle_monster_positions(&self, monster_id: u16) -> Vec<f32> {
        let Some(mesh) = self.muscle_monster_render_mesh(monster_id) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.extend_from_slice(&[p[0], p[1], p[2]]);
        }
        out
    }

    /// Per-vertex `[u, v]` texel coords, parallel to the positions.
    pub fn muscle_monster_uvs(&self, monster_id: u16) -> Vec<i32> {
        let Some(mesh) = self.muscle_monster_render_mesh(monster_id) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.extend_from_slice(&[uv[0] as i32, uv[1] as i32]);
        }
        out
    }

    /// Per-vertex `[cba, tsb]` (battle-slot relocated), parallel to the
    /// positions.
    pub fn muscle_monster_cba_tsb(&self, monster_id: u16) -> Vec<u32> {
        let Some(mesh) = self.muscle_monster_render_mesh(monster_id) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.cba_tsb.len() * 2);
        for ct in &mesh.cba_tsb {
            out.extend_from_slice(&[ct[0] as u32, ct[1] as u32]);
        }
        out
    }

    /// Triangle indices of the monster's battle mesh.
    pub fn muscle_monster_indices(&self, monster_id: u16) -> Vec<u32> {
        self.muscle_monster_render_mesh(monster_id)
            .map(|m| m.indices)
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, 255]` - monsters draw fully textured, so every
    /// vertex samples VRAM through its prim's **packet colour** (same
    /// convention as the fighter body; see [`crate::packet_color`]).
    pub fn muscle_monster_flat_rgba(&self, monster_id: u16) -> Vec<u8> {
        let Some(mesh) = self.muscle_monster_render_mesh(monster_id) else {
            return Vec::new();
        };
        crate::packet_color::textured(&mesh)
    }

    /// Per-vertex TMD object index (the rigid part a vertex hangs from).
    pub fn muscle_monster_object_ids(&self, monster_id: u16) -> Vec<u32> {
        let Some(entry) = self.monster_archive_entry() else {
            return Vec::new();
        };
        let Some(Some(mesh)) = monster_archive::mesh(entry, monster_id).ok() else {
            return Vec::new();
        };
        let Ok(tmd) = legaia_tmd::parse(mesh.tmd_bytes()) else {
            return Vec::new();
        };
        legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, mesh.tmd_bytes()).1
    }

    /// TMD object count (pose rig width) of the monster's mesh.
    pub fn muscle_monster_part_count(&self, monster_id: u16) -> u32 {
        let Some(entry) = self.monster_archive_entry() else {
            return 0;
        };
        let Some(Some(mesh)) = monster_archive::mesh(entry, monster_id).ok() else {
            return 0;
        };
        legaia_tmd::parse(mesh.tmd_bytes())
            .map(|t| t.objects.len() as u32)
            .unwrap_or(0)
    }

    /// Every decodable action animation of the monster, in action-table
    /// order: `[{"action_id":0,"rate":1,"part_count":P,"frame_count":F},…]`.
    /// `action_id` is the semantic tag (`0` idle, `2`/`3` hit reactions,
    /// `4` knockdown, `0x20`/`0x21` the attack family - see
    /// `docs/formats/monster-animation.md`); the array index is the handle
    /// for [`Self::muscle_monster_pose_frames`].
    pub fn muscle_monster_anims_json(&self, monster_id: u16) -> String {
        let Some(entry) = self.monster_archive_entry() else {
            return "[]".to_string();
        };
        let anims = match monster_archive::animations(entry, monster_id) {
            Ok(Some(a)) => a,
            _ => return "[]".to_string(),
        };
        let rows: Vec<serde_json::Value> = anims
            .iter()
            .map(|a| {
                serde_json::json!({
                    "action_id": a.action_id,
                    "rate": a.rate,
                    "part_count": a.part_count,
                    "frame_count": a.frame_count,
                })
            })
            .collect();
        serde_json::Value::Array(rows).to_string()
    }

    /// Monster action animation `index` decoded to absolute per-(frame, part)
    /// `[tx, ty, tz, rx, ry, rz]` (PSX 4096-unit angles), padded to
    /// `target_part_count` parts - the same pose-stream shape every other
    /// site animator consumes (`baka_anim_pose_frames` and siblings).
    pub fn muscle_monster_pose_frames(
        &self,
        monster_id: u16,
        index: u32,
        target_part_count: u32,
    ) -> Vec<i32> {
        let Some(entry) = self.monster_archive_entry() else {
            return Vec::new();
        };
        let anims = match monster_archive::animations(entry, monster_id) {
            Ok(Some(a)) => a,
            _ => return Vec::new(),
        };
        let Some(anim) = anims.get(index as usize) else {
            return Vec::new();
        };
        let parts = (target_part_count as usize).max(anim.part_count);
        let mut out = Vec::with_capacity(anim.frame_count * parts * 6);
        for frame in &anim.frames {
            for p in 0..parts {
                match frame.get(p) {
                    Some(t) => out.extend_from_slice(&[
                        t.tx as i32,
                        t.ty as i32,
                        t.tz as i32,
                        t.rx as i32,
                        t.ry as i32,
                        t.rz as i32,
                    ]),
                    None => out.extend_from_slice(&[0; 6]),
                }
            }
        }
        out
    }

    /// The dome duel's 1 MB PSX VRAM: the character's **battle-form texture
    /// pool** at the pinned band-0 placement (`FUN_80052FA0` uploads +
    /// the decoded battle palette overlaid on the CLUT rows the assembled
    /// mesh samples - the arts viewer's chain), plus monster `monster_id`'s
    /// texture pool injected at battle slot 0's coordinates (CLUT row 484,
    /// 4bpp page at `(320, 256)`) - the layout the retail battle loader
    /// builds - plus the arena backdrop's own TIM pages (PROT 1225 tail
    /// chunks: 4bpp pages at `(768, 0)` / `(832, 0)`, CLUT rows 473 / 479;
    /// all three bands are disjoint, so upload order doesn't matter).
    pub fn muscle_vram(&self, monster_id: u16, char_slot: u32) -> Vec<u8> {
        let mut vram = legaia_tim::Vram::new();
        if let Some(raw) = self.muscle_player_file(char_slot)
            && let Ok(pack) = legaia_asset::battle_data_pack::parse(raw)
        {
            if let Ok(uploads) = bca::character_texture_uploads(raw, &pack, &[0u8; 5], 0) {
                for u in &uploads {
                    vram.write_block(u.fb_x(), u.fb_y(), u.rect.w, u.rect.h, &u.pixels);
                    if !u.clut.is_empty() {
                        vram.write_clut_row(u.clut_x, u.clut_row(), &u.clut_bytes());
                    }
                }
            }
            // Battle palette on the CLUT rows/columns the relocated mesh
            // samples (Vahn = the byte-exact fixed-stride record parse; the
            // others = the equipment-robust collector).
            if let Some((_, mesh, _)) = self.muscle_fighter_build(char_slot) {
                let mut rows: Vec<u16> = mesh.cba_tsb.iter().map(|c| (c[0] >> 6) & 0x1FF).collect();
                rows.sort_unstable();
                rows.dedup();
                let mut cols: Vec<u16> = mesh.cba_tsb.iter().map(|c| (c[0] & 0x3F) * 16).collect();
                cols.sort_unstable();
                cols.dedup();
                let pal = if char_slot == 0 {
                    legaia_asset::battle_char_palette::find_record0(raw).and_then(|rec0| {
                        legaia_asset::battle_char_palette::parse_record(raw, rec0).ok()
                    })
                } else {
                    legaia_asset::battle_char_palette::collect_palette(raw, 0, &cols).ok()
                };
                if let Some(pal) = pal {
                    for &row in &rows {
                        for band in &pal.bands {
                            let bytes: Vec<u8> = band
                                .vram_words()
                                .iter()
                                .flat_map(|w| w.to_le_bytes())
                                .collect();
                            vram.write_clut_row(band.base, row, &bytes);
                        }
                    }
                }
            }
        }
        if let Some(entry) = self.monster_archive_entry()
            && let Ok(Some(mesh)) = monster_archive::mesh(entry, monster_id)
        {
            // Injects the pool at slot 0's CLUT row + page origin.
            let _ = mesh.battle_render_mesh(0, &mut vram);
        }
        if let Some(buf) = self.muscle_arena_entry() {
            for chunk in scene_tmd_stream::battle_tim_chunks(buf) {
                if let Some(bytes) = buf.get(chunk.payload_offset..)
                    && let Ok(tim) = legaia_tim::parse(bytes)
                {
                    vram.upload_tim(&tim);
                }
            }
        }
        vram.as_bytes().to_vec()
    }

    // -------------------------------------------------------- arena backdrop

    /// Status of the dome's arena backdrop (PROT 1225 - see
    /// [`ARENA_BACKDROP_PROT_INDEX`] for the pin):
    /// `{"ok":true,"prot":1225,"verts":N,"tris":N,"tims":2}`, or
    /// `{"ok":false}` when the entry is absent / doesn't match the
    /// scene_tmd_stream shape on this image.
    pub fn muscle_arena_json(&self) -> String {
        let Some((mesh, _)) = self.muscle_arena_hybrid() else {
            return r#"{"ok":false}"#.to_string();
        };
        let tims = self
            .muscle_arena_entry()
            .map(|b| scene_tmd_stream::battle_tim_chunks(b).len())
            .unwrap_or(0);
        format!(
            r#"{{"ok":true,"prot":{},"verts":{},"tris":{},"tims":{}}}"#,
            ARENA_BACKDROP_PROT_INDEX,
            mesh.positions.len(),
            mesh.triangle_count(),
            tims,
        )
    }

    /// Arena-shell vertex positions (`[x, y, z, ...]`, retail Y-down world
    /// coordinates, world-fixed - the shell is authored at `X >= 0` with the
    /// open side facing `-X`, and retail seats the fighters near the world
    /// origin). Empty when the backdrop doesn't decode.
    pub fn muscle_arena_positions(&self) -> Vec<f32> {
        let Some((mesh, _)) = self.muscle_arena_hybrid() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.extend_from_slice(&[p[0], p[1], p[2]]);
        }
        out
    }

    /// Per-vertex `[u, v]` texel coords for the arena shell.
    pub fn muscle_arena_uvs(&self) -> Vec<i32> {
        let Some((mesh, _)) = self.muscle_arena_hybrid() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.extend_from_slice(&[uv[0] as i32, uv[1] as i32]);
        }
        out
    }

    /// Per-vertex `[cba, tsb]` for the arena shell (its TIMs' own authored
    /// VRAM addresses - no battle-slot relocation applies to a backdrop).
    pub fn muscle_arena_cba_tsb(&self) -> Vec<u32> {
        let Some((mesh, _)) = self.muscle_arena_hybrid() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.cba_tsb.len() * 2);
        for ct in &mesh.cba_tsb {
            out.extend_from_slice(&[ct[0] as u32, ct[1] as u32]);
        }
        out
    }

    /// Triangle indices for the arena shell.
    pub fn muscle_arena_indices(&self) -> Vec<u32> {
        self.muscle_arena_hybrid()
            .map(|(m, _)| m.indices)
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, textured_flag]` for the arena's hybrid textured /
    /// vertex-colour render (same convention as the fighter bodies).
    pub fn muscle_arena_flat_rgba(&self) -> Vec<u8> {
        self.muscle_arena_hybrid()
            .map(|(_, flat)| flat)
            .unwrap_or_default()
    }

    // ------------------------------------------------------------- dome SFX

    /// The dome's pinned sound-cue rows and whether each decodes on this
    /// image:
    ///
    /// ```json
    /// { "ok": true,
    ///   "ui": [32, 33, 34],   // FUN_801d0748's own blips (call ids 0x21..0x23
    ///                         // through FUN_8004fcc8's id-1 leg; PROT 0868)
    ///   "hit": 9,             // shared battle/duel melee impact (PROT 0869)
    ///   "hit_voices": 2 }
    /// ```
    ///
    /// `ok` is false without a SCUS (raw `PROT.DAT` load - the descriptor
    /// table lives in the executable).
    pub fn muscle_sfx_json(&self) -> String {
        let ui: Vec<String> = MUSCLE_UI_CUE_CALL_IDS
            .iter()
            .map(|&id| (id - 1).to_string())
            .collect();
        let hit_ok = self.muscle_static_cue(MUSCLE_HIT_CUE_ROW, 0).is_some();
        let hit_voices = self
            .scus
            .as_ref()
            .and_then(|s| sfx_table::SfxTable::from_scus(s))
            .and_then(|t| t.get(MUSCLE_HIT_CUE_ROW).map(|d| d.voice_count()))
            .unwrap_or(0);
        format!(
            r#"{{"ok":{},"ui":[{}],"hit":{},"hit_voices":{}}}"#,
            hit_ok,
            ui.join(","),
            MUSCLE_HIT_CUE_ROW,
            hit_voices,
        )
    }

    /// Decode one voice layer of static SFX descriptor row `row` to mono i16
    /// PCM (the dome's cues are static-table rows - see
    /// [`Self::muscle_sfx_json`]). Empty when the row / voice / bank doesn't
    /// resolve.
    pub fn muscle_sfx_pcm(&self, row: u8, voice: u8) -> Vec<i16> {
        self.muscle_static_cue(row, voice)
            .map(|(pcm, _)| pcm)
            .unwrap_or_default()
    }

    /// Playback rate for [`Self::muscle_sfx_pcm`] (`0` when absent).
    pub fn muscle_sfx_rate(&self, row: u8, voice: u8) -> u32 {
        self.muscle_static_cue(row, voice)
            .map(|(_, rate)| rate)
            .unwrap_or(0)
    }
}
